// Importador y exportador del formato abierto DAWproject (.dawproject).
// GNU AGPLv3
// crates/hikaru_gui/src/project/dawproject.rs
//
// Un `.dawproject` es un archivo ZIP estándar (STORE o DEFLATE) que contiene:
// - `metadata.xml` (metadatos opcionales)
// - `project.xml` (transporte, estructura de pistas, arreglo con clips de audio y MIDI)
// - archivos de audio embebidos opcionales (p. ej. `audio/sample.wav`)

use std::collections::HashMap;
use std::io::{Error, ErrorKind, Read};
use std::path::{Path, PathBuf};

use egui::Color32;
use flate2::read::DeflateDecoder;
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::app::AppState;
use crate::audio_proxy::GuiCommand;
use crate::project::common::{self, ProjectError};
use crate::views::matrix::{ClipData, MatrixClip, MatrixSlot, SlotState, TrackMeta};
use crate::views::mixer::Track;
use crate::views::playlist::{self, ClipType, PlaylistClip, PlaylistState};
use hikaru_transport::TransportPlaybackState;

pub const EXTENSION: &str = "dawproject";
pub const FILE_FILTER: &str = "DAWproject (*.dawproject)";

// =========================================================================
// MODELO INTERMEDIO DE DAWPROJECT
// =========================================================================

#[derive(Debug, Clone)]
struct ParsedTrack {
    xml_id: String,
    name: String,
    is_master: bool,
    volume: f32,
    pan: f32,
    mute: bool,
    color: Option<Color32>,
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
    Empty,
}

#[derive(Debug, Clone)]
struct ParsedClip {
    name: String,
    track_xml_id: Option<String>,
    start_beats: f64,
    duration_beats: f64,
    muted: bool,
    color: Option<Color32>,
    content: ParsedClipContent,
}

#[derive(Debug, Clone)]
struct ParsedDawProject {
    bpm: f64,
    beats_per_bar: u32,
    beat_division: u32,
    tracks: Vec<ParsedTrack>,
    clips: Vec<ParsedClip>,
}

// =========================================================================
// EXPORTAR (.dawproject)
// =========================================================================

pub fn export_to_path(state: &AppState, target: &Path) -> Result<(), String> {
    let archive = build_dawproject_bytes(state);
    std::fs::write(target, archive).map_err(|e| e.to_string())
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

    let mut structure_xml = String::new();
    for track in &state.live_tracks {
        let role = if track.is_master { "master" } else { "regular" };
        let tname = xml_escape(&track.name);
        let color_hex = format!(
            "#{:02x}{:02x}{:02x}",
            track.color.r(),
            track.color.g(),
            track.color.b()
        );
        structure_xml.push_str(&format!(
            "    <Track id=\"track-{id}\" name=\"{tname}\" color=\"{color_hex}\" contentType=\"audio notes\" loaded=\"true\">\n\
             \x20\x20\x20\x20\x20\x20<Channel id=\"chan-{id}\" role=\"{role}\" audioChannels=\"2\">\n\
             \x20\x20\x20\x20\x20\x20\x20\x20<Volume id=\"vol-{id}\" value=\"{vol:.4}\" unit=\"linear\"/>\n\
             \x20\x20\x20\x20\x20\x20\x20\x20<Pan id=\"pan-{id}\" value=\"{pan:.4}\" unit=\"normalized\"/>\n\
             \x20\x20\x20\x20\x20\x20\x20\x20<Mute id=\"mute-{id}\" value=\"{mute}\"/>\n\
             \x20\x20\x20\x20\x20\x20</Channel>\n\
             \x20\x20\x20\x20</Track>\n",
            id = track.id,
            vol = track.volume,
            pan = (track.pan + 1.0) * 0.5,
            mute = track.mute,
        ));
    }

    // Agrupar clips por pista en lanes estándar de DAWproject
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
            let cname = xml_escape(&clip.name);
            let enable = if clip.muted { "false" } else { "true" };
            let c_hex = format!(
                "#{:02x}{:02x}{:02x}",
                clip.color.r(),
                clip.color.g(),
                clip.color.b()
            );
            match &clip.clip_type {
                ClipType::Audio {
                    sample_path,
                    sample_offset_ticks,
                    ..
                } => {
                    let play_start = *sample_offset_ticks as f64 / ppqn;
                    let p_esc = xml_escape(sample_path);
                    lanes_xml.push_str(&format!(
                        "          <Clip name=\"{cname}\" time=\"{start_beats:.6}\" duration=\"{dur_beats:.6}\" playStart=\"{play_start:.6}\" enable=\"{enable}\" color=\"{c_hex}\">\n\
                         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20<Audio channels=\"2\">\n\
                         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20<File path=\"{p_esc}\" external=\"true\"/>\n\
                         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20</Audio>\n\
                         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20</Clip>\n"
                    ));
                }
                _ => {
                    lanes_xml.push_str(&format!(
                        "          <Clip name=\"{cname}\" time=\"{start_beats:.6}\" duration=\"{dur_beats:.6}\" enable=\"{enable}\" color=\"{c_hex}\"/>\n"
                    ));
                }
            }
        }
        lanes_xml.push_str(
            "        </Clips>\n\
             \x20\x20\x20\x20\x20\x20</Lanes>\n",
        );
    }

    // Incluir clips MIDI de la Session Matrix si existen
    for (col_idx, col) in state.matrix_state.grid.iter().enumerate() {
        let track_id = state
            .live_tracks
            .get(col_idx + 1)
            .map(|t| t.id)
            .unwrap_or(col_idx + 1);
        let mut midi_clips_xml = String::new();
        let mut cursor_beats = 0.0f64;
        for slot in col {
            let Some(clip) = &slot.clip else { continue };
            if let ClipData::Midi { notes } = &clip.content {
                if notes.is_empty() {
                    continue;
                }
                let cname = xml_escape(&clip.name);
                let max_tick = notes
                    .iter()
                    .map(|(st, _, _, dur)| st.saturating_add(*dur as u64))
                    .max()
                    .unwrap_or(960 * 4);
                let dur_beats = (max_tick as f64 / 960.0).max(1.0);
                midi_clips_xml.push_str(&format!(
                    "          <Clip name=\"{cname}\" time=\"{cursor_beats:.6}\" duration=\"{dur_beats:.6}\">\n\
                     \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20<Notes>\n"
                ));
                for &(st, key, vel, dur) in notes {
                    let t_b = st as f64 / 960.0;
                    let d_b = dur as f64 / 960.0;
                    let v_norm = (vel as f64 / 127.0).clamp(0.0, 1.0);
                    midi_clips_xml.push_str(&format!(
                        "              <Note time=\"{t_b:.6}\" duration=\"{d_b:.6}\" key=\"{key}\" vel=\"{v_norm:.4}\"/>\n"
                    ));
                }
                midi_clips_xml.push_str(
                    "            </Notes>\n\
                     \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20</Clip>\n",
                );
                cursor_beats += dur_beats;
            }
        }
        if !midi_clips_xml.is_empty() {
            lanes_xml.push_str(&format!(
                "      <Lanes track=\"track-{track_id}\" timeUnit=\"beats\">\n\
                 \x20\x20\x20\x20\x20\x20\x20\x20<Clips>\n\
                 {midi_clips_xml}\
                 \x20\x20\x20\x20\x20\x20\x20\x20</Clips>\n\
                 \x20\x20\x20\x20\x20\x20</Lanes>\n"
            ));
        }
    }

    let project_xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <Project version=\"1.0\">\n\
         \x20\x20<Application name=\"Hikaru OpenLive\" version=\"{ver}\"/>\n\
         \x20\x20<Transport>\n\
         \x20\x20\x20\x20<Tempo id=\"tempo\" value=\"{bpm:.3}\" unit=\"bpm\"/>\n\
         \x20\x20\x20\x20<TimeSignature id=\"timesig\" numerator=\"{num}\" denominator=\"{den}\"/>\n\
         \x20\x20</Transport>\n\
         \x20\x20<Structure>\n\
         {structure_xml}\
         \x20\x20</Structure>\n\
         \x20\x20<Arrangement id=\"arrangement\">\n\
         \x20\x20\x20\x20<Lanes timeUnit=\"beats\">\n\
         {lanes_xml}\
         \x20\x20\x20\x20</Lanes>\n\
         \x20\x20</Arrangement>\n\
         </Project>\n",
        ver = crate::version::VERSION,
    );

    build_zip_store(&[
        ("metadata.xml", metadata_xml.as_bytes()),
        ("project.xml", project_xml.as_bytes()),
    ])
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

fn parse_project_xml(xml: &str) -> Result<ParsedDawProject, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut bpm = 120.0f64;
    let mut beats_per_bar = 4u32;
    let mut beat_division = 4u32;
    let mut tracks: Vec<ParsedTrack> = Vec::new();
    let mut clips: Vec<ParsedClip> = Vec::new();

    let mut current_track: Option<ParsedTrack> = None;
    let mut lane_track_stack: Vec<Option<String>> = Vec::new();
    let mut lane_time_unit_stack: Vec<String> = Vec::new();
    let mut current_clip: Option<ParsedClip> = None;
    let mut in_notes = false;

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag = local_name(e.name().as_ref());
                let attrs = collect_attrs(e);
                handle_open_tag(
                    &tag,
                    &attrs,
                    false,
                    &mut bpm,
                    &mut beats_per_bar,
                    &mut beat_division,
                    &mut tracks,
                    &mut current_track,
                    &mut lane_track_stack,
                    &mut lane_time_unit_stack,
                    &mut clips,
                    &mut current_clip,
                    &mut in_notes,
                );
            }
            Ok(Event::Empty(ref e)) => {
                let tag = local_name(e.name().as_ref());
                let attrs = collect_attrs(e);
                handle_open_tag(
                    &tag,
                    &attrs,
                    true,
                    &mut bpm,
                    &mut beats_per_bar,
                    &mut beat_division,
                    &mut tracks,
                    &mut current_track,
                    &mut lane_track_stack,
                    &mut lane_time_unit_stack,
                    &mut clips,
                    &mut current_clip,
                    &mut in_notes,
                );
            }
            Ok(Event::End(ref e)) => {
                let tag = local_name(e.name().as_ref());
                match tag.as_str() {
                    "Track" => {
                        if let Some(tr) = current_track.take() {
                            tracks.push(tr);
                        }
                    }
                    "Lanes" => {
                        lane_track_stack.pop();
                        lane_time_unit_stack.pop();
                    }
                    "Notes" => {
                        in_notes = false;
                    }
                    "Clip" => {
                        if let Some(cl) = current_clip.take() {
                            clips.push(cl);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML error en posición {}: {}", reader.error_position(), e)),
            _ => {}
        }
        buf.clear();
    }

    Ok(ParsedDawProject {
        bpm,
        beats_per_bar,
        beat_division,
        tracks,
        clips,
    })
}

#[allow(clippy::too_many_arguments)]
fn handle_open_tag(
    tag: &str,
    attrs: &HashMap<String, String>,
    is_empty: bool,
    bpm: &mut f64,
    beats_per_bar: &mut u32,
    beat_division: &mut u32,
    tracks: &mut Vec<ParsedTrack>,
    current_track: &mut Option<ParsedTrack>,
    lane_track_stack: &mut Vec<Option<String>>,
    lane_time_unit_stack: &mut Vec<String>,
    clips: &mut Vec<ParsedClip>,
    current_clip: &mut Option<ParsedClip>,
    in_notes: &mut bool,
) {
    match tag {
        "Tempo" => {
            if let Some(v) = attrs.get("value").and_then(|s| s.parse::<f64>().ok()) {
                if v > 1.0 && v < 999.0 {
                    *bpm = v;
                }
            }
        }
        "TimeSignature" => {
            if let Some(n) = attrs.get("numerator").and_then(|s| s.parse::<u32>().ok()) {
                *beats_per_bar = n.clamp(1, 32);
            }
            if let Some(d) = attrs.get("denominator").and_then(|s| s.parse::<u32>().ok()) {
                *beat_division = d.clamp(1, 32);
            }
        }
        "Track" => {
            let xml_id = attrs
                .get("id")
                .cloned()
                .unwrap_or_else(|| format!("track-{}", tracks.len()));
            let name = attrs
                .get("name")
                .cloned()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("Track {}", tracks.len() + 1));
            let color = attrs.get("color").and_then(|c| parse_hex_color(c));
            let tr = ParsedTrack {
                xml_id,
                name,
                is_master: false,
                volume: 0.85,
                pan: 0.0,
                mute: false,
                color,
            };
            if is_empty {
                tracks.push(tr);
            } else {
                *current_track = Some(tr);
            }
        }
        "Channel" => {
            if let Some(tr) = current_track.as_mut() {
                if attrs
                    .get("role")
                    .is_some_and(|r| r.eq_ignore_ascii_case("master"))
                {
                    tr.is_master = true;
                }
            }
        }
        "Volume" => {
            if let Some(tr) = current_track.as_mut() {
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
            if let Some(tr) = current_track.as_mut() {
                if let Some(v) = attrs.get("value").and_then(|s| s.parse::<f32>().ok()) {
                    // En DAWproject Pan normalized suele ser 0..1 (0.5 = centro)
                    tr.pan = if (0.0..=1.0).contains(&v) {
                        (v * 2.0 - 1.0).clamp(-1.0, 1.0)
                    } else {
                        v.clamp(-1.0, 1.0)
                    };
                }
            }
        }
        "Mute" => {
            if let Some(tr) = current_track.as_mut() {
                if let Some(v) = attrs.get("value") {
                    tr.mute = v.eq_ignore_ascii_case("true") || v == "1";
                }
            }
        }
        "Lanes" => {
            if !is_empty {
                let inherited_track = lane_track_stack.last().cloned().flatten();
                let track_attr = attrs.get("track").cloned().or(inherited_track);
                let inherited_unit = lane_time_unit_stack
                    .last()
                    .cloned()
                    .unwrap_or_else(|| "beats".to_string());
                let unit = attrs
                    .get("timeUnit")
                    .cloned()
                    .unwrap_or(inherited_unit);
                lane_track_stack.push(track_attr);
                lane_time_unit_stack.push(unit);
            }
        }
        "Clip" => {
            let unit = lane_time_unit_stack
                .last()
                .map(|s| s.as_str())
                .unwrap_or("beats");
            let to_beats = |val: f64| -> f64 {
                if unit.eq_ignore_ascii_case("seconds") {
                    val * (*bpm / 60.0)
                } else {
                    val
                }
            };
            let start_beats = attrs
                .get("time")
                .and_then(|s| s.parse::<f64>().ok())
                .map(to_beats)
                .unwrap_or(0.0)
                .max(0.0);
            let duration_beats = attrs
                .get("duration")
                .and_then(|s| s.parse::<f64>().ok())
                .map(to_beats)
                .unwrap_or(4.0)
                .max(0.25);
            let play_start_beats = attrs
                .get("playStart")
                .and_then(|s| s.parse::<f64>().ok())
                .map(to_beats)
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
            let color = attrs.get("color").and_then(|c| parse_hex_color(c));
            let track_xml_id = attrs
                .get("track")
                .cloned()
                .or_else(|| lane_track_stack.last().cloned().flatten());

            let cl = ParsedClip {
                name,
                track_xml_id,
                start_beats,
                duration_beats,
                muted,
                color,
                content: ParsedClipContent::Empty,
            };
            // Guardar play_start temporalmente si viene en un atributo del Clip
            let mut cl = cl;
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
                clips.push(cl);
            } else {
                *current_clip = Some(cl);
            }
        }
        "File" => {
            if let Some(cl) = current_clip.as_mut() {
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
            if !is_empty && current_clip.is_some() {
                *in_notes = true;
                if let Some(cl) = current_clip.as_mut() {
                    if !matches!(cl.content, ParsedClipContent::Midi { .. }) {
                        cl.content = ParsedClipContent::Midi { notes: Vec::new() };
                    }
                }
            }
        }
        "Note" => {
            if *in_notes {
                if let Some(cl) = current_clip.as_mut() {
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

    // 2. Pistas (Master en índice 0 + pistas regulares en 1..N)
    let mut master_track = Track::new(0, "Master".to_string(), true);
    let mut regular_tracks: Vec<Track> = Vec::new();
    let mut xml_id_to_track_idx: HashMap<String, usize> = HashMap::new();

    for pt in &parsed.tracks {
        if pt.is_master {
            master_track.name = pt.name.clone();
            master_track.volume = pt.volume;
            master_track.pan = pt.pan;
            master_track.mute = pt.mute;
            if let Some(c) = pt.color {
                master_track.color = c;
            }
            xml_id_to_track_idx.insert(pt.xml_id.clone(), 0);
        } else {
            let idx = regular_tracks.len() + 1;
            let mut tr = Track::new(idx, pt.name.clone(), false);
            tr.volume = pt.volume;
            tr.pan = pt.pan;
            tr.mute = pt.mute;
            tr.matrix_idx = Some(idx - 1);
            if let Some(c) = pt.color {
                tr.color = c;
            }
            xml_id_to_track_idx.insert(pt.xml_id.clone(), idx);
            regular_tracks.push(tr);
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

    // 3. Sincronizar Session Matrix con las pistas cargadas
    let num_regular = state.live_tracks.len().saturating_sub(1).max(1);
    let num_scenes = state.matrix_state.scenes.len().max(8);
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
    state.matrix_state.grid = (0..num_regular)
        .map(|_| vec![MatrixSlot::default(); num_scenes])
        .collect();
    state.matrix_state.selected_slot = None;

    // 4. Clips en Playlist / Arranger (y clips MIDI también en Session Matrix)
    let mut pl = PlaylistState::default();
    let ppqn = pl.ppqn.max(1) as f64;
    let base_dir = dawproject_path.parent();
    let stem = dawproject_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "dawproject".to_string());

    let mut next_id = 1usize;
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
                let resolved_path = resolve_dawproject_audio(
                    path,
                    *external,
                    base_dir,
                    &stem,
                    zip_entries,
                );
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
                        loop_enabled: false,
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
                        loop_enabled: false,
                    },
                ));

                // Colocar también el clip MIDI en el primer slot libre de esa pista
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
                        loop_enabled: false,
                    },
                ));
                next_id += 1;
            }
        }
    }

    pl.next_clip_id = next_id;
    pl.needs_full_sync = !pl.clips.is_empty();
    let has_clips = !pl.clips.is_empty();
    state.playlist_state = pl;
    state.selected_track_index = 1.min(state.live_tracks.len().saturating_sub(1));
    state.selected_slot_index = 0;
    if has_clips {
        state.panel_layout.show_arranger = true;
    }

    common::push_track_mix_to_engine(state);
}

fn resolve_dawproject_audio(
    raw_path: &str,
    external: bool,
    base_dir: Option<&Path>,
    project_stem: &str,
    zip_entries: &HashMap<String, Vec<u8>>,
) -> PathBuf {
    let normalized = raw_path.trim_start_matches("./").replace('\\', "/");

    // Si el archivo viene embebido dentro del ZIP del `.dawproject`, extraerlo a disco temporal
    if !external || zip_entries.contains_key(&normalized) {
        if let Some(bytes) = zip_entries.get(&normalized).or_else(|| {
            zip_entries
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(&normalized))
                .map(|(_, v)| v)
        }) {
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
// HELPERS XML Y ZIP (LECTURA STORE + DEFLATE, ESCRITURA STORE)
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

    // Buscar End of Central Directory (0x06054b50) desde el final
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

        // Leer payload desde el Local File Header
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

/// Escritor ZIP (método STORE, sin compresión) estándar y autocontenido para `.dawproject`.
pub(crate) fn build_zip_store(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central_dir = Vec::new();

    for &(name, data) in entries {
        let name_bytes = name.as_bytes();
        let crc = crc32_ieee(data);
        let size = data.len() as u32;
        let local_offset = out.len() as u32;

        // Local file header
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // STORE
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(data);

        // Central directory header
        central_dir.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central_dir.extend_from_slice(&20u16.to_le_bytes());
        central_dir.extend_from_slice(&20u16.to_le_bytes());
        central_dir.extend_from_slice(&0u16.to_le_bytes());
        central_dir.extend_from_slice(&0u16.to_le_bytes()); // STORE
        central_dir.extend_from_slice(&0u16.to_le_bytes());
        central_dir.extend_from_slice(&0u16.to_le_bytes());
        central_dir.extend_from_slice(&crc.to_le_bytes());
        central_dir.extend_from_slice(&size.to_le_bytes());
        central_dir.extend_from_slice(&size.to_le_bytes());
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
    use flate2::write::DeflateEncoder;
    use flate2::Compression;
    use std::io::Write;

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
        out.extend_from_slice(&8u16.to_le_bytes()); // DEFLATE
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
        out.extend_from_slice(&8u16.to_le_bytes()); // DEFLATE
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
        <Volume value="0.8" unit="linear"/>
        <Pan value="0.5" unit="normalized"/>
        <Mute value="false"/>
      </Channel>
    </Track>
  </Structure>
  <Arrangement id="arr">
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
    </Lanes>
  </Arrangement>
</Project>"##;

        let parsed = parse_project_xml(xml).expect("parse bitwig xml");
        assert!((parsed.bpm - 150.0).abs() < 0.01);
        assert_eq!(parsed.beats_per_bar, 4);
        assert_eq!(parsed.tracks.len(), 2);
        assert!(parsed.tracks[0].is_master);
        assert_eq!(parsed.tracks[1].name, "Drums");
        assert_eq!(parsed.clips.len(), 2);
        assert_eq!(parsed.clips[0].name, "Riddim Loop");
        match &parsed.clips[0].content {
            ParsedClipContent::Audio { path, external, .. } => {
                assert_eq!(path, "audio/loop.wav");
                assert!(!external);
            }
            other => panic!("esperaba Audio, obtuve {:?}", other),
        }
        match &parsed.clips[1].content {
            ParsedClipContent::Midi { notes } => {
                assert_eq!(notes.len(), 2);
                assert_eq!(notes[0].1, 36);
                assert_eq!(notes[1].1, 43);
            }
            other => panic!("esperaba Midi, obtuve {:?}", other),
        }
    }
}
