// Importador y exportador del formato abierto DAWproject (.dawproject).
// GNU AGPLv3
// crates/hikaru_gui/src/project/dawproject.rs
//
// Un `.dawproject` es un archivo ZIP estándar (STORE o DEFLATE) que contiene:
// - `metadata.xml` (metadatos opcionales)
// - `project.xml` (transporte, estructura de pistas, dispositivos, escenas, arreglo, automatización y marcadores)
// - archivos de audio embebidos opcionales (`audio/...`)
// - blobs de estado de plugins/dispositivos opcionales (`plugins/...`, `devices/...`)

use std::collections::{HashMap, HashSet};
use std::io::{Error, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

use egui::Color32;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::app::AppState;
use crate::audio_proxy::GuiCommand;
use crate::project::common::{self, ProjectError};
use crate::views::automation_editor::{
    bpm_to_norm, norm_to_bpm, AutomationLane, AutomationPoint, AutomationTarget,
};
use crate::views::detail_editor_panel::ClipEditorTarget;
use crate::views::matrix::{
    self, ClipData, MatrixClip, MatrixSlot, SceneMeta, SlotState, TrackMeta,
};
use crate::views::mixer::{
    DspSlot, ExternalParamState, SendConnection, Track, EMPTY_SLOT_NAME, ROUTE_FX_BASE,
};
use crate::views::playlist::{self, ClipType, CueMarker, CurvePoint, PlaylistClip, PlaylistState};
use hikaru_plugin_host::{PluginDescriptor, PluginFormat, PluginKind, PluginParamInfo};
use hikaru_sequencer::QuantizationGrid;
use hikaru_transport::TransportPlaybackState;

pub const EXTENSION: &str = "dawproject";
pub const FILE_FILTER: &str = "DAWproject (*.dawproject)";

// =========================================================================
// MODELO INTERMEDIO DE DAWPROJECT
// =========================================================================

#[derive(Debug, Clone)]
struct ParsedSend {
    dest_xml_id: String,
    volume: f32,
}

#[derive(Debug, Clone)]
struct ParsedDeviceParam {
    param_id: u32,
    name: String,
    value: f32,
}

#[derive(Debug, Clone)]
struct ParsedDevice {
    name: String,
    kind: String, // "vst3" | "clap" | "au" | "builtin"
    plugin_path: String,
    plugin_format: String,
    enabled: bool,
    state_path: Option<String>,
    params: Vec<ParsedDeviceParam>,
}

#[derive(Debug, Clone)]
struct ParsedTrack {
    xml_id: String,
    name: String,
    is_master: bool,
    is_effect: bool,
    is_group: bool,
    parent_xml_id: Option<String>,
    dest_xml_id: Option<String>,
    volume: f32,
    pan: f32,
    mute: bool,
    solo: bool,
    color: Option<Color32>,
    sends: Vec<ParsedSend>,
    devices: Vec<ParsedDevice>,
}

#[derive(Debug, Clone)]
enum ParsedClipContent {
    Audio {
        path: String,
        external: bool,
        play_start_beats: f64,
    },
    Midi {
        notes: Vec<(u64, u8, u8, u32)>,
    },
    Automation {
        target_param: String,
        points: Vec<(f64, f32)>,
    },
    Empty,
}

#[derive(Debug, Clone)]
struct ParsedClip {
    name: String,
    track_xml_id: Option<String>,
    start_beats: f64,
    duration_beats: f64,
    muted: bool,
    loop_enabled: bool,
    color: Option<Color32>,
    content: ParsedClipContent,
}

#[derive(Debug, Clone)]
struct ParsedSceneSlot {
    track_xml_id: Option<String>,
    clip: ParsedClip,
}

#[derive(Debug, Clone)]
struct ParsedScene {
    name: String,
    color: Option<Color32>,
    launch_quantization: Option<String>,
    slots: Vec<ParsedSceneSlot>,
}

#[derive(Debug, Clone)]
struct ParsedAutomationLane {
    target_id: String,
    unit: String,
    points: Vec<(f64, f32)>,
}

#[derive(Debug, Clone)]
struct ParsedMarker {
    name: String,
    time_beats: f64,
    color: Option<Color32>,
}

#[derive(Debug, Clone)]
struct ParsedDawProject {
    bpm: f64,
    beats_per_bar: u32,
    beat_division: u32,
    loop_enabled: bool,
    loop_start_beats: f64,
    loop_end_beats: f64,
    tempo_xml_ids: HashSet<String>,
    vol_xml_to_track: HashMap<String, String>,
    pan_xml_to_track: HashMap<String, String>,
    send_xml_to_ref: HashMap<String, (String, String)>,
    param_xml_to_ref: HashMap<String, (String, usize, u32)>,
    tracks: Vec<ParsedTrack>,
    clips: Vec<ParsedClip>,
    scenes: Vec<ParsedScene>,
    automation: Vec<ParsedAutomationLane>,
    markers: Vec<ParsedMarker>,
}

// =========================================================================
// EXPORTAR (.dawproject)
// =========================================================================

pub fn export_to_path(state: &AppState, target: &Path) -> Result<(), String> {
    let archive = build_dawproject_bytes(state);
    std::fs::write(target, archive).map_err(|e| e.to_string())
}

struct ExportEntry {
    name: String,
    data: Vec<u8>,
    compress: bool,
}

pub fn build_dawproject_bytes(state: &AppState) -> Vec<u8> {
    let title = state
        .project_path
        .as_ref()
        .and_then(|p| p.file_stem())
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Untitled".to_string());
    let title_esc = xml_escape(&title);

    let metadata_xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <MetaData>\n\
         \x20\x20<Title>{title_esc}</Title>\n\
         \x20\x20<Comment>Exported from Hikaru OpenLive</Comment>\n\
         </MetaData>\n"
    );

    let bpm = state.transport.bpm;
    let num = state.transport.beats_per_bar;
    let den = state.transport.beat_division;
    let ppqn = state.playlist_state.ppqn.max(1) as f64;

    let mut extra_entries: Vec<ExportEntry> = Vec::new();
    let mut seen_audio: HashMap<String, String> = HashMap::new();
    let mut used_audio_names: HashSet<String> = HashSet::new();

    // 1. Estructura de pistas (regulares/master/grupo + FX tracks)
    let mut structure_xml = String::new();
    for track in &state.live_tracks {
        append_track_xml(
            &mut structure_xml,
            track,
            false,
            &state.fx_tracks,
            &mut extra_entries,
        );
    }
    for fx in &state.fx_tracks {
        append_track_xml(
            &mut structure_xml,
            fx,
            true,
            &state.fx_tracks,
            &mut extra_entries,
        );
    }

    // Índice rápido de notas MIDI para patrones del Arranger
    let mut pattern_notes: HashMap<usize, &Vec<(u64, u8, u8, u32)>> = HashMap::new();
    for col in &state.matrix_state.grid {
        for slot in col {
            if let Some(clip) = &slot.clip {
                if let ClipData::Midi { notes } = &clip.content {
                    pattern_notes.insert(clip.id, notes);
                }
            }
        }
    }
    let active_playlist_notes: Option<(usize, Vec<(u64, u8, u8, u32)>)> =
        match state.clip_editor.active_target {
            Some(ClipEditorTarget::Playlist { clip_id })
                if !state.piano_roll_state.notes.is_empty() =>
            {
                let notes = state
                    .piano_roll_state
                    .notes
                    .iter()
                    .map(|n| {
                        (
                            n.start_tick,
                            n.pitch,
                            n.velocity,
                            n.duration_ticks.max(1) as u32,
                        )
                    })
                    .collect();
                Some((clip_id, notes))
            }
            _ => None,
        };

    // 2. Clips del Arranger agrupados por pista en <Lanes>
    let mut clips_by_track: HashMap<usize, Vec<&PlaylistClip>> = HashMap::new();
    for (track_idx, clip) in &state.playlist_state.clips {
        let track_id = state
            .live_tracks
            .get(*track_idx)
            .map(|t| t.id)
            .unwrap_or(*track_idx);
        clips_by_track.entry(track_id).or_default().push(clip);
    }

    let mut lanes_xml = String::new();
    let mut track_ids: Vec<usize> = clips_by_track.keys().copied().collect();
    track_ids.sort_unstable();

    for track_id in track_ids {
        let Some(clips) = clips_by_track.get(&track_id) else {
            continue;
        };
        lanes_xml.push_str(&format!(
            "      <Lanes track=\"track-{track_id}\" timeUnit=\"beats\">\n\
             \x20\x20\x20\x20\x20\x20\x20\x20<Clips>\n"
        ));
        for clip in clips {
            let start_beats = clip.start_tick as f64 / ppqn;
            let dur_beats = clip.duration_ticks as f64 / ppqn;
            let c_hex = color_to_hex(clip.color);
            match &clip.clip_type {
                ClipType::Audio {
                    sample_path,
                    sample_offset_ticks,
                    ..
                } => {
                    let play_start = *sample_offset_ticks as f64 / ppqn;
                    let (resolved, external) = embed_audio_file(
                        sample_path,
                        &mut seen_audio,
                        &mut used_audio_names,
                        &mut extra_entries,
                    );
                    write_audio_clip_xml(
                        &mut lanes_xml,
                        "          ",
                        &clip.name,
                        start_beats,
                        dur_beats,
                        play_start,
                        !clip.muted,
                        clip.loop_enabled,
                        Some(&c_hex),
                        &resolved,
                        external,
                    );
                }
                ClipType::Pattern { pattern_id } => {
                    let notes_slice: Option<&[(u64, u8, u8, u32)]> = pattern_notes
                        .get(pattern_id)
                        .map(|v| v.as_slice())
                        .or_else(|| {
                            active_playlist_notes
                                .as_ref()
                                .filter(|(cid, _)| *cid == clip.id || *cid == *pattern_id)
                                .map(|(_, v)| v.as_slice())
                        });
                    write_midi_clip_xml(
                        &mut lanes_xml,
                        "          ",
                        &clip.name,
                        start_beats,
                        dur_beats,
                        !clip.muted,
                        clip.loop_enabled,
                        Some(&c_hex),
                        notes_slice.unwrap_or(&[]),
                        ppqn,
                    );
                }
                ClipType::Automation {
                    points,
                    target_param,
                } => {
                    let t_esc = xml_escape(target_param);
                    let cname = xml_escape(&clip.name);
                    let enable = if clip.muted { "false" } else { "true" };
                    lanes_xml.push_str(&format!(
                        "          <Clip name=\"{cname}\" time=\"{start_beats:.6}\" duration=\"{dur_beats:.6}\" enable=\"{enable}\" color=\"{c_hex}\">\n\
                         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20<Lanes timeUnit=\"beats\">\n\
                         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20<Points target=\"{t_esc}\" unit=\"normalized\">\n"
                    ));
                    for pt in points {
                        let t_b = pt.rel_tick as f64 / ppqn;
                        lanes_xml.push_str(&format!(
                            "                <RealPoint time=\"{t_b:.6}\" value=\"{:.6}\" interpolation=\"linear\"/>\n",
                            pt.value.clamp(0.0, 1.0)
                        ));
                    }
                    lanes_xml.push_str(
                        "              </Points>\n\
                         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20</Lanes>\n\
                         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20</Clip>\n",
                    );
                }
            }
        }
        lanes_xml.push_str(
            "        </Clips>\n\
             \x20\x20\x20\x20\x20\x20</Lanes>\n",
        );
    }

    // 3. Carriles de automatización globales y por pista (dp-07)
    for lane in &state.automation.lanes {
        if lane.points.is_empty() {
            continue;
        }
        let (target_id, unit, is_tempo) = match &lane.target {
            AutomationTarget::TransportTempo => ("tempo".to_string(), "bpm", true),
            AutomationTarget::MixerVolume { track_idx } => {
                let tid = state
                    .live_tracks
                    .get(*track_idx)
                    .map(|t| t.id)
                    .unwrap_or(*track_idx);
                (format!("vol-{tid}"), "linear", false)
            }
            AutomationTarget::MixerPan { track_idx } => {
                let tid = state
                    .live_tracks
                    .get(*track_idx)
                    .map(|t| t.id)
                    .unwrap_or(*track_idx);
                (format!("pan-{tid}"), "normalized", false)
            }
            AutomationTarget::TrackSend { track_idx, fx_idx } => {
                let tid = state
                    .live_tracks
                    .get(*track_idx)
                    .map(|t| t.id)
                    .unwrap_or(*track_idx);
                (format!("send-{tid}-{fx_idx}"), "linear", false)
            }
            AutomationTarget::ExternalPluginParam {
                track_idx,
                slot_idx,
                param_id,
            } => {
                let tid = state
                    .live_tracks
                    .get(*track_idx)
                    .map(|t| t.id)
                    .unwrap_or(*track_idx);
                (
                    format!("param-{tid}-{slot_idx}-{param_id}"),
                    "normalized",
                    false,
                )
            }
        };
        lanes_xml.push_str(&format!(
            "      <Points target=\"{target_id}\" unit=\"{unit}\">\n"
        ));
        for pt in &lane.points {
            let t_b = pt.tick as f64 / ppqn;
            let val = if is_tempo {
                norm_to_bpm(pt.value)
            } else {
                pt.value.clamp(0.0, 1.0) as f64
            };
            lanes_xml.push_str(&format!(
                "        <RealPoint time=\"{t_b:.6}\" value=\"{val:.6}\" interpolation=\"linear\"/>\n"
            ));
        }
        lanes_xml.push_str("      </Points>\n");
    }

    // 4. Marcadores del Arranger (dp-10)
    let mut markers_xml = String::new();
    if !state.playlist_state.cue_markers.is_empty() {
        markers_xml.push_str("    <Markers>\n");
        for m in &state.playlist_state.cue_markers {
            let t_b = m.tick as f64 / ppqn;
            let mname = xml_escape(&m.name);
            let mcol = format!("#{:02x}{:02x}{:02x}", m.color[0], m.color[1], m.color[2]);
            markers_xml.push_str(&format!(
                "      <Marker id=\"marker-{}\" name=\"{mname}\" time=\"{t_b:.6}\" color=\"{mcol}\"/>\n",
                m.id
            ));
        }
        markers_xml.push_str("    </Markers>\n");
    }

    // 5. Escenas y slots del Clip Launcher (dp-09)
    let mut scenes_xml = String::new();
    if !state.matrix_state.scenes.is_empty() {
        scenes_xml.push_str("  <Scenes>\n");
        for (scene_idx, scene) in state.matrix_state.scenes.iter().enumerate() {
            let sname = xml_escape(&scene.name);
            let scol_attr = scene
                .color
                .map(|c| format!(" color=\"{}\"", color_to_hex(c)))
                .unwrap_or_default();
            let quant_attr = scene
                .launch_quantization
                .map(|q| format!(" launchQuantization=\"{}\"", q.to_notation()))
                .unwrap_or_default();
            scenes_xml.push_str(&format!(
                "    <Scene id=\"scene-{scene_idx}\" name=\"{sname}\"{scol_attr}{quant_attr}>\n\
                 \x20\x20\x20\x20\x20\x20<Lanes timeUnit=\"beats\">\n"
            ));
            for (col_idx, col) in state.matrix_state.grid.iter().enumerate() {
                let Some(slot) = col.get(scene_idx) else {
                    continue;
                };
                let Some(mclip) = &slot.clip else {
                    continue;
                };
                let track_id = state
                    .live_tracks
                    .iter()
                    .find(|t| !t.is_master && t.matrix_idx == Some(col_idx))
                    .or_else(|| state.live_tracks.iter().filter(|t| !t.is_master).nth(col_idx))
                    .map(|t| t.id)
                    .unwrap_or(col_idx + 1);
                let dur_beats = if mclip.duration_secs > 0.0 {
                    (mclip.duration_secs * (bpm / 60.0)).max(0.25)
                } else {
                    num.max(1) as f64
                };
                let c_hex = mclip.color.map(color_to_hex);
                scenes_xml.push_str(&format!(
                    "        <ClipSlot track=\"track-{track_id}\">\n"
                ));
                match &mclip.content {
                    ClipData::Audio { events, .. } => {
                        let raw_path = if !mclip.path.as_os_str().is_empty() {
                            mclip.path.to_string_lossy().to_string()
                        } else {
                            events
                                .iter()
                                .find_map(|e| e.source_path.as_ref())
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_default()
                        };
                        let (resolved, external) = embed_audio_file(
                            &raw_path,
                            &mut seen_audio,
                            &mut used_audio_names,
                            &mut extra_entries,
                        );
                        write_audio_clip_xml(
                            &mut scenes_xml,
                            "          ",
                            &mclip.name,
                            0.0,
                            dur_beats,
                            0.0,
                            !mclip.muted,
                            mclip.loop_enabled,
                            c_hex.as_deref(),
                            &resolved,
                            external,
                        );
                    }
                    ClipData::Midi { notes } => {
                        let max_tick = notes
                            .iter()
                            .map(|(st, _, _, dur)| st.saturating_add(*dur as u64))
                            .max()
                            .unwrap_or(960 * num.max(1) as u64);
                        let midi_dur = (max_tick as f64 / ppqn).max(dur_beats).max(0.25);
                        write_midi_clip_xml(
                            &mut scenes_xml,
                            "          ",
                            &mclip.name,
                            0.0,
                            midi_dur,
                            !mclip.muted,
                            mclip.loop_enabled,
                            c_hex.as_deref(),
                            notes,
                            ppqn,
                        );
                    }
                }
                scenes_xml.push_str("        </ClipSlot>\n");
            }
            scenes_xml.push_str("      </Lanes>\n    </Scene>\n");
        }
        scenes_xml.push_str("  </Scenes>\n");
    }

    let loop_start_beats = state.playlist_state.loop_start_ticks as f64 / ppqn;
    let loop_end_beats = state.playlist_state.loop_end_ticks as f64 / ppqn;
    let loop_xml = if state.playlist_state.loop_region_active
        || state.playlist_state.loop_end_ticks > state.playlist_state.loop_start_ticks
    {
        format!(
            "    <Loop start=\"{loop_start_beats:.6}\" end=\"{loop_end_beats:.6}\" enable=\"{}\"/>\n",
            state.playlist_state.loop_region_active
        )
    } else {
        String::new()
    };

    let project_xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <Project version=\"1.0\">\n\
         \x20\x20<Application name=\"Hikaru OpenLive\" version=\"{ver}\"/>\n\
         \x20\x20<Transport>\n\
         \x20\x20\x20\x20<Tempo id=\"tempo\" value=\"{bpm:.3}\" unit=\"bpm\"/>\n\
         \x20\x20\x20\x20<TimeSignature id=\"timesig\" numerator=\"{num}\" denominator=\"{den}\"/>\n\
         {loop_xml}\
         \x20\x20</Transport>\n\
         \x20\x20<Structure>\n\
         {structure_xml}\
         \x20\x20</Structure>\n\
         \x20\x20<Arrangement id=\"arrangement\">\n\
         {markers_xml}\
         \x20\x20\x20\x20<Lanes timeUnit=\"beats\">\n\
         {lanes_xml}\
         \x20\x20\x20\x20</Lanes>\n\
         \x20\x20</Arrangement>\n\
         {scenes_xml}\
         </Project>\n",
        ver = crate::version::VERSION,
    );

    let mut entries = Vec::with_capacity(2 + extra_entries.len());
    entries.push(ExportEntry {
        name: "metadata.xml".to_string(),
        data: metadata_xml.into_bytes(),
        compress: true,
    });
    entries.push(ExportEntry {
        name: "project.xml".to_string(),
        data: project_xml.into_bytes(),
        compress: true,
    });
    entries.extend(extra_entries);

    build_zip_archive(&entries)
}

fn color_to_hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

fn append_track_xml(
    out: &mut String,
    track: &Track,
    is_effect: bool,
    fx_tracks: &[Track],
    extra_entries: &mut Vec<ExportEntry>,
) {
    let prefix = if is_effect { "fx" } else { "track" };
    let xml_id = format!("{prefix}-{}", track.id);
    let role = if is_effect {
        "effect"
    } else if track.is_master {
        "master"
    } else {
        "regular"
    };
    let content_type = if track.is_group {
        "tracks"
    } else {
        "audio notes"
    };
    let tname = xml_escape(&track.name);
    let color_hex = color_to_hex(track.color);
    let parent_attr = track
        .parent_group_id
        .map(|gid| format!(" parentGroup=\"track-{gid}\""))
        .unwrap_or_default();
    let dest_attr = if !is_effect && !track.is_master && track.route_destination_id > 0 {
        format!(" destination=\"track-{}\"", track.route_destination_id)
    } else {
        String::new()
    };

    out.push_str(&format!(
        "    <Track id=\"{xml_id}\" name=\"{tname}\" color=\"{color_hex}\" contentType=\"{content_type}\" loaded=\"true\"{parent_attr}>\n\
         \x20\x20\x20\x20\x20\x20<Channel id=\"chan-{prefix}-{id}\" role=\"{role}\" audioChannels=\"2\"{dest_attr}>\n\
         \x20\x20\x20\x20\x20\x20\x20\x20<Volume id=\"vol-{prefix_id}\" value=\"{vol:.4}\" unit=\"linear\"/>\n\
         \x20\x20\x20\x20\x20\x20\x20\x20<Pan id=\"pan-{prefix_id}\" value=\"{pan:.4}\" unit=\"normalized\"/>\n\
         \x20\x20\x20\x20\x20\x20\x20\x20<Mute id=\"mute-{prefix_id}\" value=\"{mute}\"/>\n\
         \x20\x20\x20\x20\x20\x20\x20\x20<Solo id=\"solo-{prefix_id}\" value=\"{solo}\"/>\n",
        id = track.id,
        prefix_id = if is_effect {
            format!("fx-{}", track.id)
        } else {
            track.id.to_string()
        },
        vol = track.volume,
        pan = (track.pan + 1.0) * 0.5,
        mute = track.mute,
        solo = track.solo,
    ));

    if !track.sends.is_empty() {
        out.push_str("        <Sends>\n");
        for s in &track.sends {
            let fx_idx = s.target_id.saturating_sub(ROUTE_FX_BASE);
            let dest_fx_id = fx_tracks.get(fx_idx).map(|f| f.id).unwrap_or(fx_idx + 1);
            out.push_str(&format!(
                "          <Send id=\"send-{}-{fx_idx}\" destination=\"fx-{dest_fx_id}\" volume=\"{:.4}\" unit=\"linear\"/>\n",
                track.id, s.amount
            ));
        }
        out.push_str("        </Sends>\n");
    }

    let captured = common::capture_track(track);
    let non_empty_slots: Vec<_> = track
        .effects
        .iter()
        .zip(captured.effects.iter())
        .enumerate()
        .filter(|(_, (live_slot, _))| live_slot.name != EMPTY_SLOT_NAME)
        .collect();

    if !non_empty_slots.is_empty() {
        out.push_str("        <Devices>\n");
        for (slot_idx, (live_slot, file_slot)) in non_empty_slots {
            let dev_id = format!("dev-{prefix}-{}-{slot_idx}", track.id);
            let dname = xml_escape(&live_slot.name);
            if let Some(ref desc) = live_slot.external {
                let (tag, fmt_str) = match desc.format {
                    PluginFormat::CLAP => ("ClapPlugin", "CLAP"),
                    PluginFormat::VST3 => ("Vst3Plugin", "VST3"),
                };
                let ppath = xml_escape(&desc.path.to_string_lossy());
                out.push_str(&format!(
                    "          <{tag} id=\"{dev_id}\" name=\"{dname}\" pluginPath=\"{ppath}\" pluginFormat=\"{fmt_str}\" loaded=\"true\">\n\
                     \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20<Enabled value=\"{}\"/>\n",
                    live_slot.active
                ));
                if let Some(ref blob) = live_slot.saved_state {
                    let state_path = format!("plugins/{dev_id}.bin");
                    extra_entries.push(ExportEntry {
                        name: state_path.clone(),
                        data: blob.clone(),
                        compress: false,
                    });
                    out.push_str(&format!(
                        "            <State path=\"{state_path}\"/>\n"
                    ));
                }
                if !live_slot.cached_params.is_empty() {
                    out.push_str("            <Parameters>\n");
                    for p in &live_slot.cached_params {
                        let pname = xml_escape(&p.info.name);
                        out.push_str(&format!(
                            "              <RealParameter id=\"param-{}-{slot_idx}-{}\" parameterID=\"{}\" name=\"{pname}\" value=\"{:.6}\" unit=\"normalized\"/>\n",
                            track.id, p.info.id, p.info.id, p.value
                        ));
                    }
                    out.push_str("            </Parameters>\n");
                }
                out.push_str(&format!("          </{tag}>\n"));
            } else {
                out.push_str(&format!(
                    "          <BuiltinDevice id=\"{dev_id}\" name=\"{dname}\" deviceID=\"hikaru.{dname}\" loaded=\"true\">\n\
                     \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20<Enabled value=\"{}\"/>\n",
                    live_slot.active
                ));
                if let Ok(json_bytes) = serde_json::to_vec(file_slot) {
                    let state_path = format!("devices/{dev_id}.json");
                    extra_entries.push(ExportEntry {
                        name: state_path.clone(),
                        data: json_bytes,
                        compress: true,
                    });
                    out.push_str(&format!(
                        "            <State path=\"{state_path}\"/>\n"
                    ));
                }
                out.push_str("          </BuiltinDevice>\n");
            }
        }
        out.push_str("        </Devices>\n");
    }

    out.push_str(
        "      </Channel>\n\
         \x20\x20\x20\x20</Track>\n",
    );
}

fn embed_audio_file(
    raw_path: &str,
    seen: &mut HashMap<String, String>,
    used_names: &mut HashSet<String>,
    extra_entries: &mut Vec<ExportEntry>,
) -> (String, bool) {
    if raw_path.is_empty() {
        return (String::new(), true);
    }
    if let Some(zip_path) = seen.get(raw_path) {
        return (zip_path.clone(), false);
    }
    let p = Path::new(raw_path);
    if let Ok(bytes) = std::fs::read(p) {
        let base_name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "sample.wav".to_string());
        let mut candidate = format!("audio/{base_name}");
        let mut idx = 1usize;
        while !used_names.insert(candidate.clone()) {
            candidate = format!("audio/{idx}_{base_name}");
            idx += 1;
        }
        extra_entries.push(ExportEntry {
            name: candidate.clone(),
            data: bytes,
            compress: false,
        });
        seen.insert(raw_path.to_string(), candidate.clone());
        return (candidate, false);
    }
    (raw_path.to_string(), true)
}

#[allow(clippy::too_many_arguments)]
fn write_audio_clip_xml(
    out: &mut String,
    indent: &str,
    name: &str,
    start_beats: f64,
    dur_beats: f64,
    play_start_beats: f64,
    enabled: bool,
    loop_enabled: bool,
    color_hex: Option<&str>,
    path: &str,
    external: bool,
) {
    let cname = xml_escape(name);
    let enable_str = if enabled { "true" } else { "false" };
    let color_attr = color_hex
        .map(|c| format!(" color=\"{c}\""))
        .unwrap_or_default();
    let loop_attr = if loop_enabled {
        format!(" loopStart=\"0.000000\" loopEnd=\"{dur_beats:.6}\"")
    } else {
        String::new()
    };
    let p_esc = xml_escape(path);
    let ext_str = if external { "true" } else { "false" };
    out.push_str(&format!(
        "{indent}<Clip name=\"{cname}\" time=\"{start_beats:.6}\" duration=\"{dur_beats:.6}\" playStart=\"{play_start_beats:.6}\" enable=\"{enable_str}\"{color_attr}{loop_attr}>\n\
         {indent}  <Audio channels=\"2\">\n\
         {indent}    <File path=\"{p_esc}\" external=\"{ext_str}\"/>\n\
         {indent}  </Audio>\n\
         {indent}</Clip>\n"
    ));
}

#[allow(clippy::too_many_arguments)]
fn write_midi_clip_xml(
    out: &mut String,
    indent: &str,
    name: &str,
    start_beats: f64,
    dur_beats: f64,
    enabled: bool,
    loop_enabled: bool,
    color_hex: Option<&str>,
    notes: &[(u64, u8, u8, u32)],
    ppqn: f64,
) {
    let cname = xml_escape(name);
    let enable_str = if enabled { "true" } else { "false" };
    let color_attr = color_hex
        .map(|c| format!(" color=\"{c}\""))
        .unwrap_or_default();
    let loop_attr = if loop_enabled {
        format!(" loopStart=\"0.000000\" loopEnd=\"{dur_beats:.6}\"")
    } else {
        String::new()
    };
    out.push_str(&format!(
        "{indent}<Clip name=\"{cname}\" time=\"{start_beats:.6}\" duration=\"{dur_beats:.6}\" enable=\"{enable_str}\"{color_attr}{loop_attr}>\n\
         {indent}  <Notes>\n"
    ));
    for &(st, key, vel, dur) in notes {
        let t_b = st as f64 / ppqn;
        let d_b = dur as f64 / ppqn;
        let v_norm = (vel as f64 / 127.0).clamp(0.0, 1.0);
        out.push_str(&format!(
            "{indent}    <Note time=\"{t_b:.6}\" duration=\"{d_b:.6}\" key=\"{key}\" vel=\"{v_norm:.4}\"/>\n"
        ));
    }
    out.push_str(&format!("{indent}  </Notes>\n{indent}</Clip>\n"));
}

// =========================================================================
// IMPORTAR / ABRIR (.dawproject)
// =========================================================================

pub fn load(state: &mut AppState, path: &Path) -> Result<(), ProjectError> {
    let raw = std::fs::read(path).map_err(ProjectError::Io)?;
    let entries = read_zip_entries(&raw).map_err(|msg| {
        ProjectError::Io(Error::new(
            ErrorKind::InvalidData,
            format!("DAWproject ZIP inválido ({}): {}", path.display(), msg),
        ))
    })?;

    let project_xml_bytes = entries
        .get("project.xml")
        .or_else(|| {
            entries
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("project.xml"))
                .map(|(_, v)| v)
        })
        .ok_or_else(|| {
            ProjectError::Io(Error::new(
                ErrorKind::InvalidData,
                "el archivo .dawproject no contiene project.xml",
            ))
        })?;

    let xml_str = std::str::from_utf8(project_xml_bytes).map_err(|e| {
        ProjectError::Io(Error::new(
            ErrorKind::InvalidData,
            format!("project.xml no es UTF-8 válido: {}", e),
        ))
    })?;

    let parsed = parse_project_xml(xml_str).map_err(|msg| {
        ProjectError::Io(Error::new(
            ErrorKind::InvalidData,
            format!("error parseando project.xml: {}", msg),
        ))
    })?;

    apply_parsed(state, &parsed, path, &entries);
    Ok(())
}

struct XmlParseState {
    bpm: f64,
    beats_per_bar: u32,
    beat_division: u32,
    loop_enabled: bool,
    loop_start_beats: f64,
    loop_end_beats: f64,
    tempo_xml_ids: HashSet<String>,
    vol_xml_to_track: HashMap<String, String>,
    pan_xml_to_track: HashMap<String, String>,
    send_xml_to_ref: HashMap<String, (String, String)>,
    param_xml_to_ref: HashMap<String, (String, usize, u32)>,
    tracks: Vec<ParsedTrack>,
    track_stack: Vec<ParsedTrack>,
    current_device: Option<ParsedDevice>,
    lane_track_stack: Vec<Option<String>>,
    lane_time_unit_stack: Vec<String>,
    clips: Vec<ParsedClip>,
    current_clip: Option<ParsedClip>,
    in_notes: bool,
    in_scenes: bool,
    scenes: Vec<ParsedScene>,
    current_scene: Option<ParsedScene>,
    current_slot_track: Option<String>,
    in_clip_slot: bool,
    in_markers: bool,
    markers: Vec<ParsedMarker>,
    current_points: Option<ParsedAutomationLane>,
    automation: Vec<ParsedAutomationLane>,
}

impl XmlParseState {
    fn new() -> Self {
        Self {
            bpm: 120.0,
            beats_per_bar: 4,
            beat_division: 4,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
            tempo_xml_ids: HashSet::new(),
            vol_xml_to_track: HashMap::new(),
            pan_xml_to_track: HashMap::new(),
            send_xml_to_ref: HashMap::new(),
            param_xml_to_ref: HashMap::new(),
            tracks: Vec::new(),
            track_stack: Vec::new(),
            current_device: None,
            lane_track_stack: Vec::new(),
            lane_time_unit_stack: Vec::new(),
            clips: Vec::new(),
            current_clip: None,
            in_notes: false,
            in_scenes: false,
            scenes: Vec::new(),
            current_scene: None,
            current_slot_track: None,
            in_clip_slot: false,
            in_markers: false,
            markers: Vec::new(),
            current_points: None,
            automation: Vec::new(),
        }
    }

    fn time_to_beats(&self, val: f64) -> f64 {
        let unit = self
            .lane_time_unit_stack
            .last()
            .map(|s| s.as_str())
            .unwrap_or("beats");
        if unit.eq_ignore_ascii_case("seconds") {
            val * (self.bpm / 60.0)
        } else {
            val
        }
    }
}

fn parse_project_xml(xml: &str) -> Result<ParsedDawProject, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut st = XmlParseState::new();
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag = local_name(e.name().as_ref());
                let attrs = collect_attrs(e);
                handle_open_tag(&mut st, &tag, &attrs, false);
            }
            Ok(Event::Empty(ref e)) => {
                let tag = local_name(e.name().as_ref());
                let attrs = collect_attrs(e);
                handle_open_tag(&mut st, &tag, &attrs, true);
            }
            Ok(Event::End(ref e)) => {
                let tag = local_name(e.name().as_ref());
                handle_close_tag(&mut st, &tag);
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(format!(
                    "XML error en posición {}: {}",
                    reader.error_position(),
                    e
                ))
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(ParsedDawProject {
        bpm: st.bpm,
        beats_per_bar: st.beats_per_bar,
        beat_division: st.beat_division,
        loop_enabled: st.loop_enabled,
        loop_start_beats: st.loop_start_beats,
        loop_end_beats: st.loop_end_beats,
        tempo_xml_ids: st.tempo_xml_ids,
        vol_xml_to_track: st.vol_xml_to_track,
        pan_xml_to_track: st.pan_xml_to_track,
        send_xml_to_ref: st.send_xml_to_ref,
        param_xml_to_ref: st.param_xml_to_ref,
        tracks: st.tracks,
        clips: st.clips,
        scenes: st.scenes,
        automation: st.automation,
        markers: st.markers,
    })
}

fn handle_close_tag(st: &mut XmlParseState, tag: &str) {
    match tag {
        "Track" => {
            if let Some(tr) = st.track_stack.pop() {
                st.tracks.push(tr);
            }
        }
        "Vst3Plugin" | "ClapPlugin" | "AuPlugin" | "BuiltinDevice" | "Device" => {
            if let Some(dev) = st.current_device.take() {
                if let Some(tr) = st.track_stack.last_mut() {
                    tr.devices.push(dev);
                }
            }
        }
        "Lanes" => {
            st.lane_track_stack.pop();
            st.lane_time_unit_stack.pop();
        }
        "Notes" => {
            st.in_notes = false;
        }
        "Points" => {
            if let Some(lane) = st.current_points.take() {
                if let Some(cl) = st.current_clip.as_mut() {
                    if matches!(cl.content, ParsedClipContent::Empty) {
                        cl.content = ParsedClipContent::Automation {
                            target_param: lane.target_id,
                            points: lane.points,
                        };
                    }
                } else if !lane.points.is_empty() {
                    st.automation.push(lane);
                }
            }
        }
        "Clip" => {
            if let Some(cl) = st.current_clip.take() {
                if st.in_clip_slot {
                    if let Some(scene) = st.current_scene.as_mut() {
                        let track_xml_id = cl
                            .track_xml_id
                            .clone()
                            .or_else(|| st.current_slot_track.clone());
                        scene.slots.push(ParsedSceneSlot {
                            track_xml_id,
                            clip: cl,
                        });
                    }
                } else {
                    st.clips.push(cl);
                }
            }
        }
        "ClipSlot" => {
            st.in_clip_slot = false;
            st.current_slot_track = None;
        }
        "Scene" => {
            if let Some(sc) = st.current_scene.take() {
                st.scenes.push(sc);
            }
        }
        "Scenes" => {
            st.in_scenes = false;
        }
        "Markers" => {
            st.in_markers = false;
        }
        _ => {}
    }
}

fn handle_open_tag(
    st: &mut XmlParseState,
    tag: &str,
    attrs: &HashMap<String, String>,
    is_empty: bool,
) {
    match tag {
        "Tempo" => {
            if let Some(id) = attrs.get("id") {
                st.tempo_xml_ids.insert(id.clone());
            }
            if let Some(v) = attrs.get("value").and_then(|s| s.parse::<f64>().ok()) {
                if v > 1.0 && v < 999.0 {
                    st.bpm = v;
                }
            }
        }
        "TimeSignature" => {
            if let Some(n) = attrs.get("numerator").and_then(|s| s.parse::<u32>().ok()) {
                st.beats_per_bar = n.clamp(1, 32);
            }
            if let Some(d) = attrs.get("denominator").and_then(|s| s.parse::<u32>().ok()) {
                st.beat_division = d.clamp(1, 32);
            }
        }
        "Loop" => {
            if let Some(s) = attrs.get("start").and_then(|v| v.parse::<f64>().ok()) {
                st.loop_start_beats = s.max(0.0);
            }
            if let Some(e) = attrs.get("end").and_then(|v| v.parse::<f64>().ok()) {
                st.loop_end_beats = e.max(0.0);
            }
            st.loop_enabled = attrs
                .get("enable")
                .is_some_and(|v| v.eq_ignore_ascii_case("true") || v == "1");
        }
        "Track" => {
            let xml_id = attrs
                .get("id")
                .cloned()
                .unwrap_or_else(|| format!("track-{}", st.tracks.len() + st.track_stack.len()));
            let name = attrs
                .get("name")
                .cloned()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("Track {}", st.tracks.len() + st.track_stack.len() + 1));
            let color = attrs.get("color").and_then(|c| parse_hex_color(c));
            let is_group = attrs
                .get("contentType")
                .is_some_and(|ct| ct.split_whitespace().any(|w| w.eq_ignore_ascii_case("tracks")));
            let parent_xml_id = attrs
                .get("parentGroup")
                .cloned()
                .or_else(|| st.track_stack.last().map(|p| p.xml_id.clone()));
            if let Some(parent) = st.track_stack.last_mut() {
                parent.is_group = true;
            }
            let tr = ParsedTrack {
                xml_id,
                name,
                is_master: false,
                is_effect: false,
                is_group,
                parent_xml_id,
                dest_xml_id: None,
                volume: 0.85,
                pan: 0.0,
                mute: false,
                solo: false,
                color,
                sends: Vec::new(),
                devices: Vec::new(),
            };
            if is_empty {
                st.tracks.push(tr);
            } else {
                st.track_stack.push(tr);
            }
        }
        "Channel" => {
            if let Some(tr) = st.track_stack.last_mut() {
                if let Some(role) = attrs.get("role") {
                    if role.eq_ignore_ascii_case("master") {
                        tr.is_master = true;
                    } else if role.eq_ignore_ascii_case("effect") {
                        tr.is_effect = true;
                    }
                }
                if let Some(dest) = attrs.get("destination") {
                    tr.dest_xml_id = Some(dest.clone());
                }
            }
        }
        "Volume" => {
            if let Some(tr) = st.track_stack.last_mut() {
                if let Some(id) = attrs.get("id") {
                    st.vol_xml_to_track.insert(id.clone(), tr.xml_id.clone());
                }
                if let Some(v) = attrs.get("value").and_then(|s| s.parse::<f32>().ok()) {
                    let is_db = attrs
                        .get("unit")
                        .is_some_and(|u| u.eq_ignore_ascii_case("decibel"));
                    tr.volume = if is_db {
                        10.0f32.powf(v / 20.0).clamp(0.0, 2.0)
                    } else {
                        v.clamp(0.0, 2.0)
                    };
                }
            }
        }
        "Pan" => {
            if let Some(tr) = st.track_stack.last_mut() {
                if let Some(id) = attrs.get("id") {
                    st.pan_xml_to_track.insert(id.clone(), tr.xml_id.clone());
                }
                if let Some(v) = attrs.get("value").and_then(|s| s.parse::<f32>().ok()) {
                    tr.pan = if (0.0..=1.0).contains(&v) {
                        (v * 2.0 - 1.0).clamp(-1.0, 1.0)
                    } else {
                        v.clamp(-1.0, 1.0)
                    };
                }
            }
        }
        "Mute" => {
            if let Some(tr) = st.track_stack.last_mut() {
                if let Some(v) = attrs.get("value") {
                    tr.mute = v.eq_ignore_ascii_case("true") || v == "1";
                }
            }
        }
        "Solo" => {
            if let Some(tr) = st.track_stack.last_mut() {
                if let Some(v) = attrs.get("value") {
                    tr.solo = v.eq_ignore_ascii_case("true") || v == "1";
                }
            }
        }
        "Send" => {
            if let Some(tr) = st.track_stack.last_mut() {
                let xml_id = attrs.get("id").cloned().unwrap_or_default();
                let dest_xml_id = attrs.get("destination").cloned().unwrap_or_default();
                let volume = attrs
                    .get("volume")
                    .or_else(|| attrs.get("value"))
                    .and_then(|s| s.parse::<f32>().ok())
                    .unwrap_or(0.0)
                    .clamp(0.0, 1.0);
                if !xml_id.is_empty() && !dest_xml_id.is_empty() {
                    st.send_xml_to_ref
                        .insert(xml_id.clone(), (tr.xml_id.clone(), dest_xml_id.clone()));
                }
                if !dest_xml_id.is_empty() {
                    tr.sends.push(ParsedSend {
                        dest_xml_id,
                        volume,
                    });
                }
            }
        }
        "Vst3Plugin" | "ClapPlugin" | "AuPlugin" | "BuiltinDevice" | "Device" => {
            if st.track_stack.last().is_some() {
                let kind = match tag {
                    "Vst3Plugin" => "vst3",
                    "ClapPlugin" => "clap",
                    "AuPlugin" => "au",
                    _ => "builtin",
                }
                .to_string();
                let name = attrs
                    .get("name")
                    .or_else(|| attrs.get("deviceName"))
                    .cloned()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "Device".to_string());
                let plugin_path = attrs.get("pluginPath").cloned().unwrap_or_default();
                let plugin_format = attrs
                    .get("pluginFormat")
                    .cloned()
                    .unwrap_or_else(|| match tag {
                        "ClapPlugin" => "CLAP".to_string(),
                        _ => "VST3".to_string(),
                    });
                let state_path = attrs.get("state").cloned();
                let dev = ParsedDevice {
                    name,
                    kind,
                    plugin_path,
                    plugin_format,
                    enabled: true,
                    state_path,
                    params: Vec::new(),
                };
                if is_empty {
                    if let Some(tr) = st.track_stack.last_mut() {
                        tr.devices.push(dev);
                    }
                } else {
                    st.current_device = Some(dev);
                }
            }
        }
        "Enabled" => {
            if let Some(dev) = st.current_device.as_mut() {
                if let Some(v) = attrs.get("value") {
                    dev.enabled = !(v.eq_ignore_ascii_case("false") || v == "0");
                }
            }
        }
        "State" => {
            if let Some(dev) = st.current_device.as_mut() {
                if let Some(p) = attrs.get("path") {
                    dev.state_path = Some(p.clone());
                }
            }
        }
        "RealParameter" => {
            if let (Some(tr), Some(dev)) = (st.track_stack.last(), st.current_device.as_mut()) {
                let xml_id = attrs.get("id").cloned().unwrap_or_default();
                let param_id = attrs
                    .get("parameterID")
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(dev.params.len() as u32);
                let name = attrs
                    .get("name")
                    .cloned()
                    .unwrap_or_else(|| format!("Param {param_id}"));
                let value = attrs
                    .get("value")
                    .and_then(|s| s.parse::<f32>().ok())
                    .unwrap_or(0.5)
                    .clamp(0.0, 1.0);
                let slot_idx = tr.devices.len();
                if !xml_id.is_empty() {
                    st.param_xml_to_ref
                        .insert(xml_id.clone(), (tr.xml_id.clone(), slot_idx, param_id));
                }
                dev.params.push(ParsedDeviceParam {
                    param_id,
                    name,
                    value,
                });
            }
        }
        "Scenes" => {
            if !is_empty {
                st.in_scenes = true;
            }
        }
        "Scene" => {
            if st.in_scenes {
                let name = attrs
                    .get("name")
                    .cloned()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| format!("Scene {}", st.scenes.len() + 1));
                let color = attrs.get("color").and_then(|c| parse_hex_color(c));
                let launch_quantization = attrs.get("launchQuantization").cloned();
                let sc = ParsedScene {
                    name,
                    color,
                    launch_quantization,
                    slots: Vec::new(),
                };
                if is_empty {
                    st.scenes.push(sc);
                } else {
                    st.current_scene = Some(sc);
                }
            }
        }
        "ClipSlot" => {
            if st.current_scene.is_some() && !is_empty {
                st.in_clip_slot = true;
                st.current_slot_track = attrs.get("track").cloned();
            }
        }
        "Markers" => {
            if !is_empty {
                st.in_markers = true;
            }
        }
        "Marker" => {
            if st.in_markers {
                let name = attrs
                    .get("name")
                    .cloned()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| format!("Marker {}", st.markers.len() + 1));
                let time_beats = attrs
                    .get("time")
                    .and_then(|s| s.parse::<f64>().ok())
                    .map(|v| st.time_to_beats(v))
                    .unwrap_or(0.0)
                    .max(0.0);
                let color = attrs.get("color").and_then(|c| parse_hex_color(c));
                st.markers.push(ParsedMarker {
                    name,
                    time_beats,
                    color,
                });
            }
        }
        "Lanes" => {
            if !is_empty {
                let inherited_track = st.lane_track_stack.last().cloned().flatten();
                let track_attr = attrs.get("track").cloned().or(inherited_track);
                let inherited_unit = st
                    .lane_time_unit_stack
                    .last()
                    .cloned()
                    .unwrap_or_else(|| "beats".to_string());
                let unit = attrs
                    .get("timeUnit")
                    .cloned()
                    .unwrap_or(inherited_unit);
                st.lane_track_stack.push(track_attr);
                st.lane_time_unit_stack.push(unit);
            }
        }
        "Points" => {
            if !is_empty {
                let target_id = attrs.get("target").cloned().unwrap_or_default();
                let unit = attrs
                    .get("unit")
                    .cloned()
                    .unwrap_or_else(|| "normalized".to_string());
                st.current_points = Some(ParsedAutomationLane {
                    target_id,
                    unit,
                    points: Vec::new(),
                });
            }
        }
        "RealPoint" | "BoolPoint" => {
            let t_raw = attrs
                .get("time")
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0)
                .max(0.0);
            let t_beats = st.time_to_beats(t_raw);
            let val = if tag == "BoolPoint" {
                if attrs
                    .get("value")
                    .is_some_and(|v| v.eq_ignore_ascii_case("true") || v == "1")
                {
                    1.0
                } else {
                    0.0
                }
            } else {
                attrs
                    .get("value")
                    .and_then(|s| s.parse::<f32>().ok())
                    .unwrap_or(0.0)
            };
            if let Some(lane) = st.current_points.as_mut() {
                lane.points.push((t_beats, val));
            }
        }
        "Clip" => {
            let start_beats = attrs
                .get("time")
                .and_then(|s| s.parse::<f64>().ok())
                .map(|v| st.time_to_beats(v))
                .unwrap_or(0.0)
                .max(0.0);
            let duration_beats = attrs
                .get("duration")
                .and_then(|s| s.parse::<f64>().ok())
                .map(|v| st.time_to_beats(v))
                .unwrap_or(4.0)
                .max(0.25);
            let play_start_beats = attrs
                .get("playStart")
                .and_then(|s| s.parse::<f64>().ok())
                .map(|v| st.time_to_beats(v))
                .unwrap_or(0.0)
                .max(0.0);
            let name = attrs
                .get("name")
                .cloned()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Clip".to_string());
            let muted = attrs
                .get("enable")
                .is_some_and(|e| e.eq_ignore_ascii_case("false"));
            let loop_enabled =
                attrs.contains_key("loopStart") || attrs.contains_key("loopEnd");
            let color = attrs.get("color").and_then(|c| parse_hex_color(c));
            let track_xml_id = attrs
                .get("track")
                .cloned()
                .or_else(|| st.current_slot_track.clone())
                .or_else(|| st.lane_track_stack.last().cloned().flatten());

            let mut cl = ParsedClip {
                name,
                track_xml_id,
                start_beats,
                duration_beats,
                muted,
                loop_enabled,
                color,
                content: ParsedClipContent::Empty,
            };
            if play_start_beats > 0.0 {
                cl.content = ParsedClipContent::Audio {
                    path: String::new(),
                    external: true,
                    play_start_beats,
                };
            }
            if is_empty {
                if matches!(&cl.content, ParsedClipContent::Audio { path, .. } if path.is_empty()) {
                    cl.content = ParsedClipContent::Empty;
                }
                if st.in_clip_slot {
                    if let Some(scene) = st.current_scene.as_mut() {
                        let tid = cl.track_xml_id.clone();
                        scene.slots.push(ParsedSceneSlot {
                            track_xml_id: tid,
                            clip: cl,
                        });
                    }
                } else {
                    st.clips.push(cl);
                }
            } else {
                st.current_clip = Some(cl);
            }
        }
        "File" => {
            if let Some(cl) = st.current_clip.as_mut() {
                if let Some(path) = attrs.get("path") {
                    let external = attrs
                        .get("external")
                        .is_some_and(|e| e.eq_ignore_ascii_case("true"));
                    let play_start_beats = match &cl.content {
                        ParsedClipContent::Audio {
                            play_start_beats, ..
                        } => *play_start_beats,
                        _ => 0.0,
                    };
                    cl.content = ParsedClipContent::Audio {
                        path: path.clone(),
                        external,
                        play_start_beats,
                    };
                }
            }
        }
        "Notes" => {
            if !is_empty && st.current_clip.is_some() {
                st.in_notes = true;
                if let Some(cl) = st.current_clip.as_mut() {
                    if !matches!(cl.content, ParsedClipContent::Midi { .. }) {
                        cl.content = ParsedClipContent::Midi { notes: Vec::new() };
                    }
                }
            }
        }
        "Note" => {
            if st.in_notes {
                if let Some(cl) = st.current_clip.as_mut() {
                    let t_beats = attrs
                        .get("time")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0)
                        .max(0.0);
                    let d_beats = attrs
                        .get("duration")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.25)
                        .max(0.01);
                    let key = attrs
                        .get("key")
                        .and_then(|s| s.parse::<u8>().ok())
                        .unwrap_or(60)
                        .min(127);
                    let vel_raw = attrs
                        .get("vel")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.8);
                    let vel = if vel_raw <= 1.0 {
                        (vel_raw * 127.0).round().clamp(1.0, 127.0) as u8
                    } else {
                        vel_raw.round().clamp(1.0, 127.0) as u8
                    };
                    let start_tick = (t_beats * 960.0).round() as u64;
                    let dur_ticks = (d_beats * 960.0).round().max(1.0) as u32;
                    if let ParsedClipContent::Midi { notes } = &mut cl.content {
                        notes.push((start_tick, key, vel, dur_ticks));
                    }
                }
            }
        }
        _ => {}
    }
}

fn apply_parsed(
    state: &mut AppState,
    parsed: &ParsedDawProject,
    dawproject_path: &Path,
    zip_entries: &HashMap<String, Vec<u8>>,
) {
    // 1. Transporte
    state.transport.sample_count = 0;
    state.transport.playback_state = TransportPlaybackState::Stopped;
    state.transport.bpm = parsed.bpm;
    state.transport.beats_per_bar = parsed.beats_per_bar;
    state.transport.beat_division = parsed.beat_division;
    state
        .audio_proxy
        .send(GuiCommand::SetBpm(parsed.bpm as f32));

    // 2. Pistas (Master en índice 0 + regulares 1..N + FX tracks)
    let mut master_track = Track::new(0, "Master".to_string(), true);
    let mut regular_tracks: Vec<Track> = Vec::new();
    let mut fx_tracks: Vec<Track> = Vec::new();
    let mut xml_id_to_track_idx: HashMap<String, usize> = HashMap::new();
    let mut xml_id_to_track_id: HashMap<String, usize> = HashMap::new();
    let mut xml_id_to_fx_idx: HashMap<String, usize> = HashMap::new();

    for pt in &parsed.tracks {
        if pt.is_effect {
            let fx_idx = fx_tracks.len();
            let mut tr = Track::new_fx(fx_idx + 1, pt.name.clone());
            tr.volume = pt.volume;
            tr.pan = pt.pan;
            tr.mute = pt.mute;
            tr.solo = pt.solo;
            if let Some(c) = pt.color {
                tr.color = c;
            }
            tr.effects = restore_devices(&pt.devices, zip_entries);
            xml_id_to_fx_idx.insert(pt.xml_id.clone(), fx_idx);
            fx_tracks.push(tr);
        } else if pt.is_master {
            master_track.name = pt.name.clone();
            master_track.volume = pt.volume;
            master_track.pan = pt.pan;
            master_track.mute = pt.mute;
            master_track.solo = pt.solo;
            if let Some(c) = pt.color {
                master_track.color = c;
            }
            master_track.effects = restore_devices(&pt.devices, zip_entries);
            xml_id_to_track_idx.insert(pt.xml_id.clone(), 0);
            xml_id_to_track_id.insert(pt.xml_id.clone(), 0);
        } else {
            let idx = regular_tracks.len() + 1;
            let mut tr = if pt.is_group {
                Track::new_group(idx, pt.name.clone())
            } else {
                Track::new(idx, pt.name.clone(), false)
            };
            tr.volume = pt.volume;
            tr.pan = pt.pan;
            tr.mute = pt.mute;
            tr.solo = pt.solo;
            tr.matrix_idx = Some(idx - 1);
            if let Some(c) = pt.color {
                tr.color = c;
            }
            tr.effects = restore_devices(&pt.devices, zip_entries);
            xml_id_to_track_idx.insert(pt.xml_id.clone(), idx);
            xml_id_to_track_id.insert(pt.xml_id.clone(), idx);
            regular_tracks.push(tr);
        }
    }

    // Resolver parent_group_id, route_destination_id y sends
    for pt in &parsed.tracks {
        if pt.is_effect {
            continue;
        }
        let Some(&t_idx) = xml_id_to_track_idx.get(&pt.xml_id) else {
            continue;
        };
        if t_idx == 0 {
            continue;
        }
        let tr = &mut regular_tracks[t_idx - 1];
        if let Some(ref p_xml) = pt.parent_xml_id {
            if let Some(&gid) = xml_id_to_track_id.get(p_xml) {
                tr.parent_group_id = Some(gid);
            }
        }
        if let Some(ref d_xml) = pt.dest_xml_id {
            if let Some(&did) = xml_id_to_track_id.get(d_xml) {
                tr.route_destination_id = did;
            }
        }
        for s in &pt.sends {
            if let Some(&fx_idx) = xml_id_to_fx_idx.get(&s.dest_xml_id) {
                tr.sends.push(SendConnection {
                    target_id: ROUTE_FX_BASE + fx_idx,
                    amount: s.volume,
                });
            }
        }
    }

    if regular_tracks.is_empty() {
        let mut tr = Track::new(1, "Track 1".to_string(), false);
        tr.matrix_idx = Some(0);
        regular_tracks.push(tr);
    }

    let mut all_tracks = Vec::with_capacity(1 + regular_tracks.len());
    all_tracks.push(master_track);
    all_tracks.extend(regular_tracks);

    state.live_tracks = all_tracks.clone();
    state.studio_tracks = all_tracks;
    if !fx_tracks.is_empty() {
        state.fx_tracks = fx_tracks;
    }

    // 3. Sincronizar Session Matrix (escenas + slots si existen)
    let num_regular = state.live_tracks.len().saturating_sub(1).max(1);
    let num_scenes = if !parsed.scenes.is_empty() {
        parsed.scenes.len()
    } else {
        state.matrix_state.scenes.len().max(8)
    };
    state.matrix_state.tracks = (0..num_regular)
        .map(|i| {
            let tr = &state.live_tracks[i + 1];
            TrackMeta {
                name: tr.name.clone(),
                muted: tr.mute,
                soloed: tr.solo,
                volume: tr.volume,
                pan: tr.pan,
            }
        })
        .collect();
    if !parsed.scenes.is_empty() {
        state.matrix_state.scenes = parsed
            .scenes
            .iter()
            .map(|sc| SceneMeta {
                name: sc.name.clone(),
                launch_quantization: sc
                    .launch_quantization
                    .as_deref()
                    .and_then(QuantizationGrid::from_notation),
                color: sc.color,
            })
            .collect();
    }
    state.matrix_state.grid = (0..num_regular)
        .map(|_| vec![MatrixSlot::default(); num_scenes])
        .collect();
    state.matrix_state.selected_slot = None;

    // 4. Clips en Playlist / Arranger y Escenas
    let mut pl = PlaylistState::default();
    let ppqn = pl.ppqn.max(1) as f64;
    let base_dir = dawproject_path.parent();
    let stem = dawproject_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "dawproject".to_string());

    if parsed.loop_end_beats > parsed.loop_start_beats {
        pl.loop_start_ticks = (parsed.loop_start_beats * ppqn).round() as u64;
        pl.loop_end_ticks = (parsed.loop_end_beats * ppqn).round() as u64;
        pl.loop_region_active = parsed.loop_enabled;
    }

    let mut next_id = 1usize;

    // 4a. Poblar slots de escenas (dp-09)
    for (scene_idx, scene) in parsed.scenes.iter().enumerate() {
        for slot in &scene.slots {
            let track_idx = slot
                .track_xml_id
                .as_deref()
                .and_then(|id| xml_id_to_track_idx.get(id).copied())
                .filter(|&i| i > 0 && i < state.live_tracks.len())
                .unwrap_or(1);
            let col_idx = track_idx.saturating_sub(1);
            let Some(m_slot) = state
                .matrix_state
                .grid
                .get_mut(col_idx)
                .and_then(|c| c.get_mut(scene_idx))
            else {
                continue;
            };
            let color = slot
                .clip
                .color
                .unwrap_or_else(|| state.live_tracks[track_idx].color);
            let dur_beats = slot.clip.duration_beats.max(0.25);
            let dur_secs = dur_beats * (60.0 / parsed.bpm.max(1.0));
            let dur_ticks = (dur_beats * ppqn).round().max(1.0) as u64;
            match &slot.clip.content {
                ParsedClipContent::Midi { notes } => {
                    m_slot.state = SlotState::Stopped;
                    m_slot.clip = Some(MatrixClip {
                        id: next_id,
                        name: slot.clip.name.clone(),
                        path: PathBuf::new(),
                        duration_secs: dur_secs,
                        content: ClipData::Midi {
                            notes: notes.clone(),
                        },
                        local_state: PlaylistState::default(),
                        local_track: state.live_tracks[track_idx].clone(),
                        local_bar: 0.0,
                        loop_start: 0,
                        loop_end: dur_ticks,
                        loop_enabled: slot.clip.loop_enabled,
                        launch_legato: false,
                        has_time_selection: false,
                        peaks: Vec::new(),
                        muted: slot.clip.muted,
                        color: Some(color),
                    });
                    next_id += 1;
                }
                ParsedClipContent::Audio { path, external, .. } if !path.is_empty() => {
                    let resolved =
                        resolve_dawproject_audio(path, *external, base_dir, &stem, zip_entries);
                    let mut peaks = Vec::new();
                    let mut final_dur_secs = dur_secs;
                    let mut loop_end = (dur_secs * 44100.0).round().max(1.0) as u64;
                    let mut preview_sr = 44100u32;
                    if let Some((samples, channels, sr)) = matrix::decode_audio_file(&resolved) {
                        let total_frames = samples.len() / channels.max(1);
                        if sr > 0 && total_frames > 0 {
                            final_dur_secs = total_frames as f64 / sr as f64;
                            loop_end = total_frames as u64;
                            preview_sr = sr;
                        }
                        let (_, p) = playlist::load_sample_info(&resolved, pl.ppqn, parsed.bpm);
                        peaks = p;
                    }
                    m_slot.state = SlotState::Stopped;
                    m_slot.clip = Some(MatrixClip {
                        id: next_id,
                        name: slot.clip.name.clone(),
                        path: resolved,
                        duration_secs: final_dur_secs,
                        content: ClipData::Audio {
                            events: Vec::new(),
                            next_event_id: 1,
                            preview_mix: Vec::new(),
                            preview_sr,
                        },
                        local_state: PlaylistState::default(),
                        local_track: state.live_tracks[track_idx].clone(),
                        local_bar: 0.0,
                        loop_start: 0,
                        loop_end,
                        loop_enabled: slot.clip.loop_enabled,
                        launch_legato: false,
                        has_time_selection: false,
                        peaks,
                        muted: slot.clip.muted,
                        color: Some(color),
                    });
                    next_id += 1;
                }
                _ => {}
            }
        }
    }

    // 4b. Clips del Arranger
    for pc in &parsed.clips {
        let track_idx = pc
            .track_xml_id
            .as_deref()
            .and_then(|id| xml_id_to_track_idx.get(id).copied())
            .filter(|&i| i > 0 && i < state.live_tracks.len())
            .unwrap_or(1);

        let start_tick = (pc.start_beats * ppqn).round() as u64;
        let duration_ticks = (pc.duration_beats * ppqn).round().max(1.0) as u64;
        let color = pc
            .color
            .unwrap_or_else(|| state.live_tracks[track_idx].color);

        match &pc.content {
            ParsedClipContent::Audio {
                path,
                external,
                play_start_beats,
            } if !path.is_empty() => {
                let resolved_path =
                    resolve_dawproject_audio(path, *external, base_dir, &stem, zip_entries);
                let (file_dur_ticks, peaks) =
                    playlist::load_sample_info(&resolved_path, pl.ppqn, parsed.bpm);
                let offset_ticks = (*play_start_beats * ppqn).round() as u64;
                let total_ticks = file_dur_ticks.max(duration_ticks);
                pl.clips.push((
                    track_idx,
                    PlaylistClip {
                        id: next_id,
                        name: pc.name.clone(),
                        start_tick,
                        duration_ticks,
                        clip_type: ClipType::Audio {
                            sample_path: resolved_path.to_string_lossy().to_string(),
                            peaks,
                            sample_offset_ticks: offset_ticks,
                            total_sample_ticks: total_ticks,
                        },
                        color,
                        muted: pc.muted,
                        loop_enabled: pc.loop_enabled,
                    },
                ));
                next_id += 1;
            }
            ParsedClipContent::Midi { notes } => {
                pl.clips.push((
                    track_idx,
                    PlaylistClip {
                        id: next_id,
                        name: pc.name.clone(),
                        start_tick,
                        duration_ticks,
                        clip_type: ClipType::Pattern {
                            pattern_id: next_id,
                        },
                        color,
                        muted: pc.muted,
                        loop_enabled: pc.loop_enabled,
                    },
                ));

                // Compatibilidad: si no había <Scenes>, colocar el clip MIDI también en el primer slot libre
                if parsed.scenes.is_empty() {
                    let col_idx = track_idx.saturating_sub(1);
                    if let Some(col) = state.matrix_state.grid.get_mut(col_idx) {
                        if let Some(slot) = col.iter_mut().find(|s| s.clip.is_none()) {
                            let dur_secs = pc.duration_beats * (60.0 / parsed.bpm.max(1.0));
                            slot.state = SlotState::Stopped;
                            slot.clip = Some(MatrixClip {
                                id: next_id,
                                name: pc.name.clone(),
                                path: PathBuf::new(),
                                duration_secs: dur_secs,
                                content: ClipData::Midi {
                                    notes: notes.clone(),
                                },
                                local_state: PlaylistState::default(),
                                local_track: state.live_tracks[track_idx].clone(),
                                local_bar: 0.0,
                                loop_start: 0,
                                loop_end: duration_ticks,
                                loop_enabled: true,
                                launch_legato: false,
                                has_time_selection: false,
                                peaks: Vec::new(),
                                muted: pc.muted,
                                color: Some(color),
                            });
                        }
                    }
                }
                next_id += 1;
            }
            ParsedClipContent::Automation {
                target_param,
                points,
            } => {
                pl.clips.push((
                    track_idx,
                    PlaylistClip {
                        id: next_id,
                        name: pc.name.clone(),
                        start_tick,
                        duration_ticks,
                        clip_type: ClipType::Automation {
                            points: points
                                .iter()
                                .map(|&(t_b, val)| CurvePoint {
                                    rel_tick: (t_b * ppqn).round() as u64,
                                    value: val.clamp(0.0, 1.0),
                                    tension: 0.0,
                                })
                                .collect(),
                            target_param: target_param.clone(),
                        },
                        color,
                        muted: pc.muted,
                        loop_enabled: pc.loop_enabled,
                    },
                ));
                next_id += 1;
            }
            _ => {
                pl.clips.push((
                    track_idx,
                    PlaylistClip {
                        id: next_id,
                        name: pc.name.clone(),
                        start_tick,
                        duration_ticks,
                        clip_type: ClipType::Pattern {
                            pattern_id: next_id,
                        },
                        color,
                        muted: pc.muted,
                        loop_enabled: pc.loop_enabled,
                    },
                ));
                next_id += 1;
            }
        }
    }

    // 5. Marcadores del Arranger (dp-10)
    let mut next_marker_id = 1usize;
    for m in &parsed.markers {
        let tick = (m.time_beats * ppqn).round() as u64;
        let c = m.color.unwrap_or(Color32::from_rgb(0xFF, 0x9E, 0x1B));
        pl.cue_markers.push(CueMarker {
            id: next_marker_id,
            tick,
            name: m.name.clone(),
            color: [c.r(), c.g(), c.b()],
        });
        next_marker_id += 1;
    }
    pl.next_cue_marker_id = next_marker_id;

    pl.next_clip_id = next_id;
    pl.needs_full_sync = !pl.clips.is_empty();
    let has_clips = !pl.clips.is_empty();
    state.playlist_state = pl;
    state.matrix_state.next_clip_id = next_id.max(state.matrix_state.next_clip_id);

    // 6. Carriles de automatización (dp-07)
    state.automation.lanes.clear();
    for al in &parsed.automation {
        let target = resolve_automation_target(
            &al.target_id,
            parsed,
            &xml_id_to_track_idx,
            &xml_id_to_fx_idx,
        );
        let Some(target) = target else {
            continue;
        };
        let is_tempo = matches!(target, AutomationTarget::TransportTempo);
        let mut lane = AutomationLane::new(target);
        for &(t_beats, raw_val) in &al.points {
            let tick = (t_beats * ppqn).round().max(0.0) as u64;
            let norm = if is_tempo && !al.unit.eq_ignore_ascii_case("normalized") {
                bpm_to_norm(raw_val as f64)
            } else {
                raw_val.clamp(0.0, 1.0)
            };
            lane.points.push(AutomationPoint { tick, value: norm });
        }
        lane.sort_points();
        state.automation.lanes.push(lane);
    }

    state.selected_track_index = 1.min(state.live_tracks.len().saturating_sub(1));
    state.selected_slot_index = 0;
    if has_clips {
        state.panel_layout.show_arranger = true;
    }

    common::push_track_mix_to_engine(state);
}

fn resolve_automation_target(
    target_id: &str,
    parsed: &ParsedDawProject,
    xml_id_to_track_idx: &HashMap<String, usize>,
    xml_id_to_fx_idx: &HashMap<String, usize>,
) -> Option<AutomationTarget> {
    if target_id == "tempo" || parsed.tempo_xml_ids.contains(target_id) {
        return Some(AutomationTarget::TransportTempo);
    }
    if let Some(track_xml) = parsed.vol_xml_to_track.get(target_id) {
        if let Some(&track_idx) = xml_id_to_track_idx.get(track_xml) {
            return Some(AutomationTarget::MixerVolume { track_idx });
        }
    }
    if let Some(track_xml) = parsed.pan_xml_to_track.get(target_id) {
        if let Some(&track_idx) = xml_id_to_track_idx.get(track_xml) {
            return Some(AutomationTarget::MixerPan { track_idx });
        }
    }
    if let Some((track_xml, fx_xml)) = parsed.send_xml_to_ref.get(target_id) {
        if let (Some(&track_idx), Some(&fx_idx)) = (
            xml_id_to_track_idx.get(track_xml),
            xml_id_to_fx_idx.get(fx_xml),
        ) {
            return Some(AutomationTarget::TrackSend { track_idx, fx_idx });
        }
    }
    if let Some((track_xml, slot_idx, param_id)) = parsed.param_xml_to_ref.get(target_id) {
        if let Some(&track_idx) = xml_id_to_track_idx.get(track_xml) {
            return Some(AutomationTarget::ExternalPluginParam {
                track_idx,
                slot_idx: *slot_idx,
                param_id: *param_id,
            });
        }
    }
    // Fallback por convención de nombres (`vol-{id}`, `pan-{id}`, `send-{id}-{fx_idx}`, `param-{id}-{slot}-{pid}`)
    if let Some(rest) = target_id.strip_prefix("vol-") {
        if let Some(&track_idx) = xml_id_to_track_idx.get(&format!("track-{rest}")) {
            return Some(AutomationTarget::MixerVolume { track_idx });
        }
    }
    if let Some(rest) = target_id.strip_prefix("pan-") {
        if let Some(&track_idx) = xml_id_to_track_idx.get(&format!("track-{rest}")) {
            return Some(AutomationTarget::MixerPan { track_idx });
        }
    }
    if let Some(rest) = target_id.strip_prefix("send-") {
        if let Some((tid, fx)) = rest.rsplit_once('-') {
            if let (Some(&track_idx), Ok(fx_idx)) = (
                xml_id_to_track_idx.get(&format!("track-{tid}")),
                fx.parse::<usize>(),
            ) {
                return Some(AutomationTarget::TrackSend { track_idx, fx_idx });
            }
        }
    }
    if let Some(rest) = target_id.strip_prefix("param-") {
        let parts: Vec<&str> = rest.split('-').collect();
        if parts.len() == 3 {
            if let (Some(&track_idx), Ok(slot_idx), Ok(param_id)) = (
                xml_id_to_track_idx.get(&format!("track-{}", parts[0])),
                parts[1].parse::<usize>(),
                parts[2].parse::<u32>(),
            ) {
                return Some(AutomationTarget::ExternalPluginParam {
                    track_idx,
                    slot_idx,
                    param_id,
                });
            }
        }
    }
    None
}

fn find_zip_entry<'a>(
    zip_entries: &'a HashMap<String, Vec<u8>>,
    raw_path: &str,
) -> Option<&'a [u8]> {
    let norm = raw_path.trim_start_matches("./").replace('\\', "/");
    zip_entries
        .get(&norm)
        .or_else(|| {
            zip_entries
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(&norm))
                .map(|(_, v)| v)
        })
        .map(|v| v.as_slice())
}

fn restore_devices(
    devices: &[ParsedDevice],
    zip_entries: &HashMap<String, Vec<u8>>,
) -> Vec<DspSlot> {
    let mut out = Vec::with_capacity(devices.len());
    for (idx, dev) in devices.iter().enumerate() {
        // Si es un dispositivo nativo guardado con JSON completo, delegar en common::build_track
        if dev.kind == "builtin" {
            if let Some(ref sp) = dev.state_path {
                if let Some(bytes) = find_zip_entry(zip_entries, sp) {
                    if let Ok(file_slot) = serde_json::from_slice::<hikaru_project_file::DspSlot>(bytes) {
                        let dummy = hikaru_project_file::Track {
                            id: 0,
                            name: String::new(),
                            volume: 0.75,
                            pan: 0.0,
                            pan_mode: 0,
                            mute: false,
                            solo: false,
                            arm: false,
                            is_master: false,
                            route_destination_id: 0,
                            sends: Vec::new(),
                            effects: vec![file_slot],
                            matrix_idx: None,
                        };
                        let rebuilt = common::build_track(&dummy);
                        if let Some(mut slot) = rebuilt.effects.into_iter().next() {
                            slot.active = dev.enabled;
                            out.push(slot);
                            continue;
                        }
                    }
                }
            }
        }

        let mut slot = DspSlot::new(idx + 1, dev.name.clone());
        slot.active = dev.enabled;
        if dev.kind != "builtin" || !dev.plugin_path.is_empty() {
            let path = PathBuf::from(&dev.plugin_path);
            let format = match dev.plugin_format.as_str() {
                "CLAP" => PluginFormat::CLAP,
                _ => PluginFormat::VST3,
            };
            if !dev.plugin_path.is_empty() {
                slot.external = Some(PluginDescriptor {
                    id: format!("{format:?}:{}", path.display()).to_lowercase(),
                    name: dev.name.clone(),
                    vendor: String::new(),
                    path,
                    format,
                    kind: PluginKind::Effect,
                    has_gui: false,
                    input_channels: None,
                });
            }
        }
        if let Some(ref sp) = dev.state_path {
            if let Some(bytes) = find_zip_entry(zip_entries, sp) {
                slot.saved_state = Some(bytes.to_vec());
            }
        }
        if !dev.params.is_empty() {
            slot.cached_params = dev
                .params
                .iter()
                .map(|p| ExternalParamState {
                    info: PluginParamInfo {
                        id: p.param_id,
                        name: p.name.clone(),
                        unit: String::new(),
                        min: 0.0,
                        max: 1.0,
                        default_value: p.value,
                        step_count: 0,
                        can_automate: true,
                        is_modulatable: true,
                    },
                    value: p.value,
                    pending_to_engine: false,
                    display_text: None,
                    last_sent_effective: None,
                    last_sent_mod: 0.0,
                })
                .collect();
        }
        out.push(slot);
    }
    out
}

fn resolve_dawproject_audio(
    raw_path: &str,
    external: bool,
    base_dir: Option<&Path>,
    project_stem: &str,
    zip_entries: &HashMap<String, Vec<u8>>,
) -> PathBuf {
    let normalized = raw_path.trim_start_matches("./").replace('\\', "/");

    if !external || zip_entries.contains_key(&normalized) {
        if let Some(bytes) = find_zip_entry(zip_entries, &normalized) {
            let out_dir = std::env::temp_dir()
                .join("hikaru_dawproject_media")
                .join(project_stem);
            let out_path = out_dir.join(&normalized);
            if let Some(parent) = out_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::write(&out_path, bytes).is_ok() {
                return out_path;
            }
        }
    }

    let p = PathBuf::from(raw_path);
    if p.is_absolute() {
        return p;
    }
    if let Some(base) = base_dir {
        base.join(p)
    } else {
        p
    }
}

// =========================================================================
// HELPERS XML Y ZIP (LECTURA STORE + DEFLATE, ESCRITURA STORE/DEFLATE)
// =========================================================================

fn local_name(raw: &[u8]) -> String {
    let s = std::str::from_utf8(raw).unwrap_or("");
    s.rsplit(':').next().unwrap_or(s).to_string()
}

fn collect_attrs(e: &quick_xml::events::BytesStart<'_>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for attr in e.attributes().flatten() {
        let key = local_name(attr.key.as_ref());
        if let Ok(val) = attr.normalized_value(Default::default()) {
            map.insert(key, val.into_owned());
        }
    }
    map
}

fn parse_hex_color(s: &str) -> Option<Color32> {
    let hex = s.trim().trim_start_matches('#');
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        return Some(Color32::from_rgb(r, g, b));
    }
    if hex.len() == 8 {
        let r = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let g = u8::from_str_radix(&hex[4..6], 16).ok()?;
        let b = u8::from_str_radix(&hex[6..8], 16).ok()?;
        return Some(Color32::from_rgb(r, g, b));
    }
    None
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Lee entradas de un archivo ZIP soportando STORE (0) y DEFLATE (8).
pub(crate) fn read_zip_entries(data: &[u8]) -> Result<HashMap<String, Vec<u8>>, String> {
    let mut entries = HashMap::new();

    let eocd_pos = find_eocd(data).ok_or_else(|| "no se encontró cabecera ZIP EOCD".to_string())?;
    let cd_entries = u16::from_le_bytes([data[eocd_pos + 10], data[eocd_pos + 11]]) as usize;
    let cd_offset = u32::from_le_bytes([
        data[eocd_pos + 16],
        data[eocd_pos + 17],
        data[eocd_pos + 18],
        data[eocd_pos + 19],
    ]) as usize;

    let mut pos = cd_offset;
    for _ in 0..cd_entries {
        if pos + 46 > data.len() {
            return Err("directorio central ZIP truncado".to_string());
        }
        let sig = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        if sig != 0x0201_4b50 {
            return Err("firma de entrada de directorio central ZIP inválida".to_string());
        }
        let method = u16::from_le_bytes([data[pos + 10], data[pos + 11]]);
        let comp_size = u32::from_le_bytes([
            data[pos + 20],
            data[pos + 21],
            data[pos + 22],
            data[pos + 23],
        ]) as usize;
        let name_len = u16::from_le_bytes([data[pos + 28], data[pos + 29]]) as usize;
        let extra_len = u16::from_le_bytes([data[pos + 30], data[pos + 31]]) as usize;
        let comment_len = u16::from_le_bytes([data[pos + 32], data[pos + 33]]) as usize;
        let local_offset = u32::from_le_bytes([
            data[pos + 42],
            data[pos + 43],
            data[pos + 44],
            data[pos + 45],
        ]) as usize;

        if pos + 46 + name_len > data.len() {
            return Err("nombre de entrada ZIP truncado".to_string());
        }
        let name = String::from_utf8_lossy(&data[pos + 46..pos + 46 + name_len])
            .replace('\\', "/");
        pos += 46 + name_len + extra_len + comment_len;

        if name.ends_with('/') {
            continue;
        }

        if local_offset + 30 > data.len() {
            return Err(format!("local header truncado para {}", name));
        }
        let lsig = u32::from_le_bytes([
            data[local_offset],
            data[local_offset + 1],
            data[local_offset + 2],
            data[local_offset + 3],
        ]);
        if lsig != 0x0403_4b50 {
            return Err(format!("firma local ZIP inválida para {}", name));
        }
        let l_name_len =
            u16::from_le_bytes([data[local_offset + 26], data[local_offset + 27]]) as usize;
        let l_extra_len =
            u16::from_le_bytes([data[local_offset + 28], data[local_offset + 29]]) as usize;
        let payload_start = local_offset + 30 + l_name_len + l_extra_len;
        let payload_end = payload_start.saturating_add(comp_size);
        if payload_end > data.len() {
            return Err(format!("datos comprimidos truncados para {}", name));
        }
        let compressed = &data[payload_start..payload_end];

        let content = match method {
            0 => compressed.to_vec(),
            8 => {
                let mut decoder = DeflateDecoder::new(compressed);
                let mut out = Vec::new();
                decoder
                    .read_to_end(&mut out)
                    .map_err(|e| format!("error descomprimiendo {}: {}", name, e))?;
                out
            }
            other => {
                return Err(format!(
                    "método de compresión ZIP no soportado ({}) en {}",
                    other, name
                ))
            }
        };
        entries.insert(name, content);
    }

    Ok(entries)
}

fn find_eocd(data: &[u8]) -> Option<usize> {
    if data.len() < 22 {
        return None;
    }
    let min_pos = data.len().saturating_sub(22 + 65535);
    for pos in (min_pos..=data.len() - 22).rev() {
        if data[pos] == 0x50
            && data[pos + 1] == 0x4b
            && data[pos + 2] == 0x05
            && data[pos + 3] == 0x06
        {
            return Some(pos);
        }
    }
    None
}

/// Escritor ZIP estándar soportando STORE (0) y DEFLATE (8).
fn build_zip_archive(entries: &[ExportEntry]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central_dir = Vec::new();

    for entry in entries {
        let name_bytes = entry.name.as_bytes();
        let crc = crc32_ieee(&entry.data);
        let uncomp_size = entry.data.len() as u32;

        let (method, payload): (u16, Vec<u8>) = if entry.compress {
            let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
            if enc.write_all(&entry.data).is_ok() {
                if let Ok(compressed) = enc.finish() {
                    if compressed.len() < entry.data.len() {
                        (8, compressed)
                    } else {
                        (0, entry.data.clone())
                    }
                } else {
                    (0, entry.data.clone())
                }
            } else {
                (0, entry.data.clone())
            }
        } else {
            (0, entry.data.clone())
        };
        let comp_size = payload.len() as u32;
        let local_offset = out.len() as u32;

        // Local file header
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&method.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&comp_size.to_le_bytes());
        out.extend_from_slice(&uncomp_size.to_le_bytes());
        out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(&payload);

        // Central directory header
        central_dir.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central_dir.extend_from_slice(&20u16.to_le_bytes());
        central_dir.extend_from_slice(&20u16.to_le_bytes());
        central_dir.extend_from_slice(&0u16.to_le_bytes());
        central_dir.extend_from_slice(&method.to_le_bytes());
        central_dir.extend_from_slice(&0u16.to_le_bytes());
        central_dir.extend_from_slice(&0u16.to_le_bytes());
        central_dir.extend_from_slice(&crc.to_le_bytes());
        central_dir.extend_from_slice(&comp_size.to_le_bytes());
        central_dir.extend_from_slice(&uncomp_size.to_le_bytes());
        central_dir.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        central_dir.extend_from_slice(&0u16.to_le_bytes());
        central_dir.extend_from_slice(&0u16.to_le_bytes());
        central_dir.extend_from_slice(&0u16.to_le_bytes());
        central_dir.extend_from_slice(&0u16.to_le_bytes());
        central_dir.extend_from_slice(&0u32.to_le_bytes());
        central_dir.extend_from_slice(&local_offset.to_le_bytes());
        central_dir.extend_from_slice(name_bytes);
    }

    let cd_offset = out.len() as u32;
    let cd_size = central_dir.len() as u32;
    out.extend_from_slice(&central_dir);

    // End of central directory record
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());

    out
}

/// Escritor ZIP (método STORE, sin compresión) estándar y autocontenido.
pub(crate) fn build_zip_store(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let wrapped: Vec<ExportEntry> = entries
        .iter()
        .map(|&(name, data)| ExportEntry {
            name: name.to_string(),
            data: data.to_vec(),
            compress: false,
        })
        .collect();
    build_zip_archive(&wrapped)
}

fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppState;
    use crate::audio_proxy::AudioProxy;
    use std::sync::atomic::{AtomicU32, AtomicU64};
    use std::sync::mpsc;
    use std::sync::Arc;

    fn make_test_app_state() -> AppState {
        let (tx, _rx) = mpsc::channel();
        AppState::new(
            AudioProxy::new(tx),
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU32::new(0)),
            None,
        )
    }

    #[test]
    fn zip_store_round_trip() {
        let zip = build_zip_store(&[
            ("metadata.xml", b"<MetaData/>"),
            ("project.xml", b"<Project version=\"1.0\"/>"),
        ]);
        let entries = read_zip_entries(&zip).expect("read zip");
        assert_eq!(entries.get("metadata.xml").unwrap(), b"<MetaData/>");
        assert_eq!(
            entries.get("project.xml").unwrap(),
            b"<Project version=\"1.0\"/>"
        );
    }

    #[test]
    fn zip_deflate_entry_is_readable() {
        let raw_xml = b"<?xml version=\"1.0\"?><Project version=\"1.0\"><Transport><Tempo value=\"140.0\" unit=\"bpm\"/></Transport></Project>";
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
        enc.write_all(raw_xml).unwrap();
        let compressed = enc.finish().unwrap();

        let name = b"project.xml";
        let crc = crc32_ieee(raw_xml);
        let comp_sz = compressed.len() as u32;
        let uncomp_sz = raw_xml.len() as u32;

        let mut out = Vec::new();
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&8u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&comp_sz.to_le_bytes());
        out.extend_from_slice(&uncomp_sz.to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(&compressed);

        let cd_offset = out.len() as u32;
        out.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&8u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&comp_sz.to_le_bytes());
        out.extend_from_slice(&uncomp_sz.to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(name);
        let cd_size = (out.len() as u32) - cd_offset;

        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());

        let entries = read_zip_entries(&out).expect("read deflate zip");
        let xml = std::str::from_utf8(entries.get("project.xml").unwrap()).unwrap();
        let parsed = parse_project_xml(xml).expect("parse xml");
        assert!((parsed.bpm - 140.0).abs() < 0.01);
    }

    #[test]
    fn parse_bitwig_style_project_xml() {
        let xml = r##"<?xml version="1.0" encoding="UTF-8"?>
<Project version="1.0">
  <Application name="Bitwig Studio" version="5.1.7"/>
  <Transport>
    <Tempo id="t0" value="150.0" unit="bpm"/>
    <TimeSignature id="ts0" numerator="4" denominator="4"/>
  </Transport>
  <Structure>
    <Track id="t-master" name="Master">
      <Channel id="c-master" role="master">
        <Volume value="0.9" unit="linear"/>
      </Channel>
    </Track>
    <Track id="t-drums" name="Drums" color="#ff5500">
      <Channel id="c-drums" role="regular">
        <Volume id="v-drums" value="0.8" unit="linear"/>
        <Pan value="0.5" unit="normalized"/>
        <Mute value="false"/>
        <Solo value="true"/>
        <Devices>
          <ClapPlugin id="dev-1" name="Vital" loaded="true">
            <Enabled value="true"/>
            <Parameters>
              <RealParameter id="p-cutoff" parameterID="42" name="Cutoff" value="0.65"/>
            </Parameters>
          </ClapPlugin>
        </Devices>
      </Channel>
    </Track>
  </Structure>
  <Arrangement id="arr">
    <Markers>
      <Marker id="m1" name="Drop" time="16.0" color="#ff0000"/>
    </Markers>
    <Lanes timeUnit="beats">
      <Lanes track="t-drums">
        <Clips>
          <Clip name="Riddim Loop" time="0.0" duration="8.0">
            <Audio channels="2">
              <File path="audio/loop.wav" external="false"/>
            </Audio>
          </Clip>
          <Clip name="Bass MIDI" time="8.0" duration="4.0">
            <Notes>
              <Note time="0.0" duration="1.0" key="36" vel="0.9"/>
              <Note time="2.0" duration="0.5" key="43" vel="0.75"/>
            </Notes>
          </Clip>
        </Clips>
      </Lanes>
      <Points target="v-drums" unit="linear">
        <RealPoint time="0.0" value="0.25"/>
        <RealPoint time="4.0" value="0.9"/>
      </Points>
    </Lanes>
  </Arrangement>
  <Scenes>
    <Scene id="sc-0" name="Intro" launchQuantization="1/1">
      <Lanes timeUnit="beats">
        <ClipSlot track="t-drums">
          <Clip name="Intro Beat" time="0.0" duration="4.0">
            <Notes>
              <Note time="0.0" duration="0.5" key="36" vel="1.0"/>
            </Notes>
          </Clip>
        </ClipSlot>
      </Lanes>
    </Scene>
  </Scenes>
</Project>"##;

        let parsed = parse_project_xml(xml).expect("parse bitwig xml");
        assert!((parsed.bpm - 150.0).abs() < 0.01);
        assert_eq!(parsed.beats_per_bar, 4);
        assert_eq!(parsed.tracks.len(), 2);
        assert!(parsed.tracks[0].is_master);
        assert_eq!(parsed.tracks[1].name, "Drums");
        assert!(parsed.tracks[1].solo);
        assert_eq!(parsed.tracks[1].devices.len(), 1);
        assert_eq!(parsed.tracks[1].devices[0].name, "Vital");
        assert_eq!(parsed.tracks[1].devices[0].params.len(), 1);
        assert_eq!(parsed.markers.len(), 1);
        assert_eq!(parsed.markers[0].name, "Drop");
        assert_eq!(parsed.automation.len(), 1);
        assert_eq!(parsed.automation[0].points.len(), 2);
        assert_eq!(parsed.scenes.len(), 1);
        assert_eq!(parsed.scenes[0].name, "Intro");
        assert_eq!(parsed.scenes[0].slots.len(), 1);
        assert_eq!(parsed.clips.len(), 2);
    }

    #[test]
    fn full_dawproject_round_trip_automation_devices_scenes_markers_and_media() {
        let mut src = make_test_app_state();
        src.transport.bpm = 138.0;
        src.transport.beats_per_bar = 3;
        src.transport.beat_division = 4;

        // Track 1 con Solo, Send, Plugin VST3 con blob + params
        let mut tr1 = Track::new(1, "Lead Synth".to_string(), false);
        tr1.solo = true;
        tr1.sends.push(SendConnection {
            target_id: ROUTE_FX_BASE,
            amount: 0.42,
        });
        let mut vst_slot = DspSlot::new(1, "Serum".to_string());
        vst_slot.external = Some(PluginDescriptor {
            id: "vst3:serum.vst3".to_string(),
            name: "Serum".to_string(),
            vendor: "Xfer".to_string(),
            path: PathBuf::from("/opt/vst3/Serum.vst3"),
            format: PluginFormat::VST3,
            kind: PluginKind::Instrument,
            has_gui: true,
            input_channels: Some(0),
        });
        vst_slot.saved_state = Some(vec![0xDE, 0xAD, 0xBE, 0xEF]);
        vst_slot.cached_params.push(ExternalParamState {
            info: PluginParamInfo {
                id: 7,
                name: "Filter Cutoff".to_string(),
                unit: "Hz".to_string(),
                min: 0.0,
                max: 1.0,
                default_value: 0.5,
                step_count: 0,
                can_automate: true,
                is_modulatable: true,
            },
            value: 0.77,
            pending_to_engine: false,
            display_text: None,
            last_sent_effective: None,
            last_sent_mod: 0.0,
        });
        tr1.effects.push(vst_slot);
        src.live_tracks = vec![Track::new(0, "Master".to_string(), true), tr1];
        src.fx_tracks = vec![Track::new_fx(1, "Reverb FX".to_string())];

        // Marcadores + Loop
        src.playlist_state.cue_markers.push(CueMarker {
            id: 1,
            tick: 960 * 8,
            name: "Chorus".to_string(),
            color: [0x28, 0xD9, 0x59],
        });
        src.playlist_state.loop_start_ticks = 960 * 4;
        src.playlist_state.loop_end_ticks = 960 * 12;
        src.playlist_state.loop_region_active = true;

        // Audio embebido real desde archivo temporal
        let tmp_wav = std::env::temp_dir().join("hikaru_dp_test_sample.wav");
        let wav_bytes = b"RIFF\x24\x00\x00\x00WAVEfmt \x10\x00\x00\x00\x01\x00\x01\x00\x44\xAC\x00\x00\x88\x58\x01\x00\x02\x00\x10\x00data\x00\x00\x00\x00";
        std::fs::write(&tmp_wav, wav_bytes).unwrap();
        src.playlist_state.clips.push((
            1,
            PlaylistClip {
                id: 1,
                name: "Vox Sample".to_string(),
                start_tick: 0,
                duration_ticks: 960 * 4,
                clip_type: ClipType::Audio {
                    sample_path: tmp_wav.to_string_lossy().to_string(),
                    peaks: Vec::new(),
                    sample_offset_ticks: 0,
                    total_sample_ticks: 960 * 4,
                },
                color: Color32::from_rgb(0xFF, 0x55, 0x00),
                muted: false,
                loop_enabled: true,
            },
        ));

        // Escena en Matrix con clip MIDI
        src.matrix_state.scenes = vec![SceneMeta {
            name: "Verse A".to_string(),
            launch_quantization: Some(QuantizationGrid::HalfNote),
            color: Some(Color32::from_rgb(0x3A, 0xA8, 0xE0)),
        }];
        src.matrix_state.grid = vec![vec![MatrixSlot {
            state: SlotState::Stopped,
            clip: Some(MatrixClip {
                id: 10,
                name: "Arp Clip".to_string(),
                path: PathBuf::new(),
                duration_secs: 2.0,
                content: ClipData::Midi {
                    notes: vec![(0, 60, 100, 480), (480, 64, 90, 480)],
                },
                local_state: PlaylistState::default(),
                local_track: src.live_tracks[1].clone(),
                local_bar: 0.0,
                loop_start: 0,
                loop_end: 960 * 4,
                loop_enabled: true,
                launch_legato: false,
                has_time_selection: false,
                peaks: Vec::new(),
                muted: false,
                color: Some(Color32::from_rgb(0x3A, 0xA8, 0xE0)),
            }),
            has_stop_button: true,
        }]];

        // Automatización (Tempo + Mixer Volume + Plugin Param)
        let mut tempo_lane = AutomationLane::new(AutomationTarget::TransportTempo);
        tempo_lane.insert_point(0, bpm_to_norm(120.0));
        tempo_lane.insert_point(960 * 4, bpm_to_norm(145.0));
        let mut vol_lane = AutomationLane::new(AutomationTarget::MixerVolume { track_idx: 1 });
        vol_lane.insert_point(0, 0.3);
        vol_lane.insert_point(960 * 8, 0.9);
        let mut param_lane = AutomationLane::new(AutomationTarget::ExternalPluginParam {
            track_idx: 1,
            slot_idx: 0,
            param_id: 7,
        });
        param_lane.insert_point(0, 0.2);
        param_lane.insert_point(960 * 2, 0.85);
        src.automation.lanes = vec![tempo_lane, vol_lane, param_lane];

        let bytes = build_dawproject_bytes(&src);
        let entries = read_zip_entries(&bytes).expect("read exported zip");
        assert!(entries.contains_key("audio/hikaru_dp_test_sample.wav"));
        assert!(entries.contains_key("plugins/dev-track-1-0.bin"));

        let out_file = std::env::temp_dir().join("hikaru_dp_roundtrip.dawproject");
        std::fs::write(&out_file, &bytes).unwrap();

        let mut dst = make_test_app_state();
        load(&mut dst, &out_file).expect("load roundtrip dawproject");

        assert!((dst.transport.bpm - 138.0).abs() < 0.01);
        assert_eq!(dst.transport.beats_per_bar, 3);
        assert_eq!(dst.live_tracks.len(), 2);
        assert!(dst.live_tracks[1].solo);
        assert_eq!(dst.live_tracks[1].sends.len(), 1);
        assert!((dst.live_tracks[1].sends[0].amount - 0.42).abs() < 0.01);
        assert_eq!(dst.live_tracks[1].effects.len(), 1);
        assert_eq!(dst.live_tracks[1].effects[0].name, "Serum");
        assert_eq!(
            dst.live_tracks[1].effects[0].saved_state.as_deref(),
            Some(&[0xDE, 0xAD, 0xBE, 0xEF][..])
        );
        assert_eq!(dst.playlist_state.cue_markers.len(), 1);
        assert_eq!(dst.playlist_state.cue_markers[0].name, "Chorus");
        assert!(dst.playlist_state.loop_region_active);
        assert_eq!(dst.matrix_state.scenes.len(), 1);
        assert_eq!(dst.matrix_state.scenes[0].name, "Verse A");
        assert_eq!(
            dst.matrix_state.scenes[0].launch_quantization,
            Some(QuantizationGrid::HalfNote)
        );
        assert!(dst.matrix_state.grid[0][0].clip.is_some());
        assert_eq!(dst.automation.lanes.len(), 3);

        let _ = std::fs::remove_file(&tmp_wav);
        let _ = std::fs::remove_file(&out_file);
    }
}
