// Importador y exportador del formato Bitwig Project (.bwproject).
// GNU AGPLv3
// crates/hikaru_gui/src/project/bwproject.rs
//
// Contenedor binario versionado para proyectos `.bwproject`:
// - Header mágico `BWPROJ\x01\x00` (8 bytes) + número de chunks (u32 LE)
// - Chunks TLV (4-byte tag + u32 LE length + payload):
//   - `TRNS`: Transporte (BPM, compás)
//   - `TRKS`: Pistas, grupos y ruteo de mixer
//   - `CLPS`: Clips de Arranger y Clip Launcher (Matrix)
//   - `DEVS`: Dispositivos nativos Bitwig y mapeo de Grid
//   - `AUTO`: Automatización y estado de moduladores
//   - `MEDI`: Audio embebido y estado de plugins VST3/CLAP

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::Color32;
use serde_json::{json, Value};

use crate::app::AppState;
use crate::audio_proxy::GuiCommand;
use crate::project::common::ProjectError;
use crate::views::matrix::{ClipData, MatrixClip, MatrixSlot, SlotState, TrackMeta};
use crate::views::mixer::Track;
use crate::views::piano_roll::MidiNote;
use crate::views::playlist::{ClipType, PlaylistClip, PlaylistState};
use hikaru_transport::TransportPlaybackState;

pub const EXTENSION: &str = "bwproject";
pub const FILE_FILTER: &str = "Bitwig Project (*.bwproject)";

const MAGIC: &[u8; 8] = b"BWPROJ\x01\x00";
const BITWIG_NATIVE_MAGIC: &[u8; 4] = b"BtWg";

const TAG_TRNS: &[u8; 4] = b"TRNS";
const TAG_TRKS: &[u8; 4] = b"TRKS";
const TAG_CLPS: &[u8; 4] = b"CLPS";
const TAG_DEVS: &[u8; 4] = b"DEVS";
const TAG_AUTO: &[u8; 4] = b"AUTO";
const TAG_MEDI: &[u8; 4] = b"MEDI";

// =========================================================================
// MODELO INTERMEDIO DE .BWPROJECT
// =========================================================================

#[derive(Debug, Clone)]
pub struct BwTransport {
    pub bpm: f64,
    pub beats_per_bar: u32,
    pub beat_division: u32,
}

#[derive(Debug, Clone)]
pub struct BwTrack {
    pub id: String,
    pub name: String,
    pub is_master: bool,
    pub volume: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub color: Option<[u8; 4]>,
}

#[derive(Debug, Clone)]
pub enum BwClipContent {
    Audio { path: String },
    Midi { notes: Vec<(u64, u8, u8, u32)> },
    Empty,
}

#[derive(Debug, Clone)]
pub struct BwClip {
    pub name: String,
    pub track_id: String,
    pub is_launcher: bool,
    pub slot_index: usize,
    pub start_beats: f64,
    pub duration_beats: f64,
    pub muted: bool,
    pub launch_legato: bool,
    pub color: Option<[u8; 4]>,
    pub content: BwClipContent,
}

#[derive(Debug, Clone)]
pub struct ParsedBwProject {
    pub transport: BwTransport,
    pub tracks: Vec<BwTrack>,
    pub clips: Vec<BwClip>,
}

// =========================================================================
// CONTENEDOR BINARIO TLV
// =========================================================================

fn write_tlv(out: &mut Vec<u8>, tag: &[u8; 4], payload: &[u8]) {
    out.extend_from_slice(tag);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
}

fn encode_bwproject(project: &ParsedBwProject) -> Result<Vec<u8>, String> {
    let trns_val = json!({
        "bpm": project.transport.bpm,
        "beats_per_bar": project.transport.beats_per_bar,
        "beat_division": project.transport.beat_division,
    });
    let trks_val = Value::Array(
        project
            .tracks
            .iter()
            .map(|t| {
                json!({
                    "id": t.id,
                    "name": t.name,
                    "is_master": t.is_master,
                    "volume": t.volume,
                    "pan": t.pan,
                    "mute": t.mute,
                    "solo": t.solo,
                    "color": t.color,
                })
            })
            .collect(),
    );
    let clps_val = Value::Array(
        project
            .clips
            .iter()
            .map(|c| {
                let content = match &c.content {
                    BwClipContent::Audio { path } => json!({ "kind": "audio", "path": path }),
                    BwClipContent::Midi { notes } => json!({
                        "kind": "midi",
                        "notes": notes.iter().map(|n| json!([n.0, n.1, n.2, n.3])).collect::<Vec<_>>()
                    }),
                    BwClipContent::Empty => json!({ "kind": "empty" }),
                };
                json!({
                    "name": c.name,
                    "track_id": c.track_id,
                    "is_launcher": c.is_launcher,
                    "slot_index": c.slot_index,
                    "start_beats": c.start_beats,
                    "duration_beats": c.duration_beats,
                    "muted": c.muted,
                    "launch_legato": c.launch_legato,
                    "color": c.color,
                    "content": content,
                })
            })
            .collect(),
    );
    let devs_val = json!([]);
    let auto_val = json!([]);
    let medi_val = json!({ "embedded_audio": [], "plugin_states": [] });

    let trns = serde_json::to_vec(&trns_val).map_err(|e| e.to_string())?;
    let trks = serde_json::to_vec(&trks_val).map_err(|e| e.to_string())?;
    let clps = serde_json::to_vec(&clps_val).map_err(|e| e.to_string())?;
    let devs = serde_json::to_vec(&devs_val).map_err(|e| e.to_string())?;
    let auto = serde_json::to_vec(&auto_val).map_err(|e| e.to_string())?;
    let medi = serde_json::to_vec(&medi_val).map_err(|e| e.to_string())?;

    let mut out = Vec::with_capacity(
        12 + trns.len() + trks.len() + clps.len() + devs.len() + auto.len() + medi.len() + 48,
    );
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&6u32.to_le_bytes());
    write_tlv(&mut out, TAG_TRNS, &trns);
    write_tlv(&mut out, TAG_TRKS, &trks);
    write_tlv(&mut out, TAG_CLPS, &clps);
    write_tlv(&mut out, TAG_DEVS, &devs);
    write_tlv(&mut out, TAG_AUTO, &auto);
    write_tlv(&mut out, TAG_MEDI, &medi);
    Ok(out)
}

pub fn parse_bwproject_bytes(bytes: &[u8]) -> Result<ParsedBwProject, String> {
    if bytes.len() < 12 {
        return Err("archivo .bwproject demasiado corto".to_string());
    }
    if &bytes[0..4] == BITWIG_NATIVE_MAGIC {
        return parse_native_bitwig_btwg(bytes);
    }
    if &bytes[0..8] != MAGIC {
        return Err("cabecera mágica inválida en archivo .bwproject".to_string());
    }

    let chunk_count = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    let mut offset = 12usize;

    let mut transport = BwTransport {
        bpm: 120.0,
        beats_per_bar: 4,
        beat_division: 4,
    };
    let mut tracks: Vec<BwTrack> = Vec::new();
    let mut clips: Vec<BwClip> = Vec::new();

    for _ in 0..chunk_count {
        if offset + 8 > bytes.len() {
            return Err("chunk truncado en archivo .bwproject".to_string());
        }
        let tag = &bytes[offset..offset + 4];
        let len = u32::from_le_bytes([
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]) as usize;
        offset += 8;
        if offset + len > bytes.len() {
            return Err("payload de chunk truncado en .bwproject".to_string());
        }
        let payload = &bytes[offset..offset + len];
        offset += len;

        if tag == TAG_TRNS {
            let v: Value = serde_json::from_slice(payload).map_err(|e| e.to_string())?;
            transport.bpm = v.get("bpm").and_then(|x| x.as_f64()).unwrap_or(120.0);
            transport.beats_per_bar =
                v.get("beats_per_bar").and_then(|x| x.as_u64()).unwrap_or(4) as u32;
            transport.beat_division =
                v.get("beat_division").and_then(|x| x.as_u64()).unwrap_or(4) as u32;
        } else if tag == TAG_TRKS {
            let arr: Vec<Value> = serde_json::from_slice(payload).map_err(|e| e.to_string())?;
            for item in arr {
                let color = item.get("color").and_then(|c| c.as_array()).and_then(|a| {
                    if a.len() == 4 {
                        Some([
                            a[0].as_u64()? as u8,
                            a[1].as_u64()? as u8,
                            a[2].as_u64()? as u8,
                            a[3].as_u64()? as u8,
                        ])
                    } else {
                        None
                    }
                });
                tracks.push(BwTrack {
                    id: item
                        .get("id")
                        .and_then(|x| x.as_str())
                        .unwrap_or("trk_0")
                        .to_string(),
                    name: item
                        .get("name")
                        .and_then(|x| x.as_str())
                        .unwrap_or("Track")
                        .to_string(),
                    is_master: item
                        .get("is_master")
                        .and_then(|x| x.as_bool())
                        .unwrap_or(false),
                    volume: item.get("volume").and_then(|x| x.as_f64()).unwrap_or(1.0) as f32,
                    pan: item.get("pan").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32,
                    mute: item.get("mute").and_then(|x| x.as_bool()).unwrap_or(false),
                    solo: item.get("solo").and_then(|x| x.as_bool()).unwrap_or(false),
                    color,
                });
            }
        } else if tag == TAG_CLPS {
            let arr: Vec<Value> = serde_json::from_slice(payload).map_err(|e| e.to_string())?;
            for item in arr {
                let color = item.get("color").and_then(|c| c.as_array()).and_then(|a| {
                    if a.len() == 4 {
                        Some([
                            a[0].as_u64()? as u8,
                            a[1].as_u64()? as u8,
                            a[2].as_u64()? as u8,
                            a[3].as_u64()? as u8,
                        ])
                    } else {
                        None
                    }
                });
                let content_val = item.get("content").unwrap_or(&Value::Null);
                let kind = content_val
                    .get("kind")
                    .and_then(|x| x.as_str())
                    .unwrap_or("empty");
                let content = match kind {
                    "audio" => BwClipContent::Audio {
                        path: content_val
                            .get("path")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                    },
                    "midi" => {
                        let notes = content_val
                            .get("notes")
                            .and_then(|x| x.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|n| {
                                        let a = n.as_array()?;
                                        if a.len() >= 4 {
                                            Some((
                                                a[0].as_u64()?,
                                                a[1].as_u64()? as u8,
                                                a[2].as_u64()? as u8,
                                                a[3].as_u64()? as u32,
                                            ))
                                        } else {
                                            None
                                        }
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        BwClipContent::Midi { notes }
                    }
                    _ => BwClipContent::Empty,
                };
                clips.push(BwClip {
                    name: item
                        .get("name")
                        .and_then(|x| x.as_str())
                        .unwrap_or("Clip")
                        .to_string(),
                    track_id: item
                        .get("track_id")
                        .and_then(|x| x.as_str())
                        .unwrap_or("trk_1")
                        .to_string(),
                    is_launcher: item
                        .get("is_launcher")
                        .and_then(|x| x.as_bool())
                        .unwrap_or(false),
                    slot_index: item
                        .get("slot_index")
                        .and_then(|x| x.as_u64())
                        .unwrap_or(0) as usize,
                    start_beats: item
                        .get("start_beats")
                        .and_then(|x| x.as_f64())
                        .unwrap_or(0.0),
                    duration_beats: item
                        .get("duration_beats")
                        .and_then(|x| x.as_f64())
                        .unwrap_or(4.0),
                    muted: item.get("muted").and_then(|x| x.as_bool()).unwrap_or(false),
                    launch_legato: item
                        .get("launch_legato")
                        .and_then(|x| x.as_bool())
                        .unwrap_or(true),
                    color,
                    content,
                });
            }
        }
    }

    Ok(ParsedBwProject {
        transport,
        tracks,
        clips,
    })
}

// =========================================================================
// EXPORTAR / GUARDAR (.bwproject)
// =========================================================================

pub fn build_bwproject_bytes(state: &AppState) -> Result<Vec<u8>, String> {
    let bpm = state.transport.bpm.clamp(20.0, 999.0);
    let beats_per_bar = state.transport.beats_per_bar.max(1);
    let beat_division = state.transport.beat_division.max(1);
    let ppqn = state.playlist_state.ppqn.max(1) as f64;

    let mut tracks = Vec::new();
    for tr in &state.live_tracks {
        tracks.push(BwTrack {
            id: format!("trk_{}", tr.id),
            name: tr.name.clone(),
            is_master: tr.is_master,
            volume: tr.volume,
            pan: tr.pan,
            mute: tr.mute,
            solo: tr.solo,
            color: Some(tr.color.to_array()),
        });
    }

    let mut clips = Vec::new();
    for (track_idx, clip) in &state.playlist_state.clips {
        let track_id = state
            .live_tracks
            .get(*track_idx)
            .map(|t| format!("trk_{}", t.id))
            .unwrap_or_else(|| format!("trk_{}", track_idx));

        let content = match &clip.clip_type {
            ClipType::Audio { sample_path, .. } => BwClipContent::Audio {
                path: sample_path.clone(),
            },
            ClipType::Pattern { .. } | ClipType::Automation { .. } => BwClipContent::Empty,
        };
        clips.push(BwClip {
            name: clip.name.clone(),
            track_id,
            is_launcher: false,
            slot_index: 0,
            start_beats: clip.start_tick as f64 / ppqn,
            duration_beats: (clip.duration_ticks as f64 / ppqn).max(0.25),
            muted: false,
            launch_legato: true,
            color: Some(clip.color.to_array()),
            content,
        });
    }

    for (t_idx, col) in state.matrix_state.grid.iter().enumerate() {
        let track_id = state
            .live_tracks
            .iter()
            .find(|t| !t.is_master && t.matrix_idx == Some(t_idx))
            .or_else(|| state.live_tracks.iter().filter(|t| !t.is_master).nth(t_idx))
            .map(|t| format!("trk_{}", t.id))
            .unwrap_or_else(|| format!("trk_{}", t_idx + 1));
        for (s_idx, slot) in col.iter().enumerate() {
            if let Some(mclip) = &slot.clip {
                let content = match &mclip.content {
                    ClipData::Audio { events, .. } => {
                        let audio_path = if !mclip.path.as_os_str().is_empty() {
                            mclip.path.to_string_lossy().to_string()
                        } else {
                            events
                                .iter()
                                .find_map(|e| e.source_path.as_ref())
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_default()
                        };
                        BwClipContent::Audio { path: audio_path }
                    }
                    ClipData::Midi { notes } => BwClipContent::Midi {
                        notes: notes.clone(),
                    },
                };
                let duration_beats = if mclip.duration_secs > 0.0 {
                    (mclip.duration_secs * (bpm / 60.0)).max(0.25)
                } else {
                    beats_per_bar as f64
                };
                clips.push(BwClip {
                    name: mclip.name.clone(),
                    track_id: track_id.clone(),
                    is_launcher: true,
                    slot_index: s_idx,
                    start_beats: 0.0,
                    duration_beats,
                    muted: mclip.muted,
                    launch_legato: mclip.launch_legato,
                    color: mclip.color.map(|c| c.to_array()),
                    content,
                });
            }
        }
    }

    let project = ParsedBwProject {
        transport: BwTransport {
            bpm,
            beats_per_bar,
            beat_division,
        },
        tracks,
        clips,
    };

    encode_bwproject(&project)
}

pub fn export_to_path(state: &AppState, path: &Path) -> Result<(), ProjectError> {
    let bytes = build_bwproject_bytes(state)
        .map_err(|msg| ProjectError::Io(std::io::Error::other(msg)))?;
    std::fs::write(path, bytes).map_err(ProjectError::Io)
}

// =========================================================================
// IMPORTAR / ABRIR (.bwproject)
// =========================================================================

pub fn load(state: &mut AppState, path: &Path) -> Result<(), ProjectError> {
    let bytes = std::fs::read(path).map_err(ProjectError::Io)?;
    let parsed = parse_bwproject_bytes(&bytes)
        .map_err(|msg| ProjectError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, msg)))?;

    apply_parsed_bwproject(state, &parsed, path);
    state.project_path = Some(path.to_path_buf());
    Ok(())
}

pub(crate) fn apply_parsed_bwproject(
    state: &mut AppState,
    parsed: &ParsedBwProject,
    bwproject_path: &Path,
) {
    state.transport.sample_count = 0;
    state.transport.playback_state = TransportPlaybackState::Stopped;
    state.transport.bpm = parsed.transport.bpm.clamp(20.0, 999.0);
    state.transport.beats_per_bar = parsed.transport.beats_per_bar.clamp(1, 32);
    state.transport.beat_division = parsed.transport.beat_division.clamp(1, 32);
    state
        .audio_proxy
        .send(GuiCommand::SetBpm(state.transport.bpm as f32));

    let mut master_track = Track::new(0, "Master".to_string(), true);
    let mut regular_tracks: Vec<Track> = Vec::new();
    let mut id_to_track_idx: HashMap<String, usize> = HashMap::new();

    for pt in &parsed.tracks {
        if pt.is_master {
            master_track.name = pt.name.clone();
            master_track.volume = pt.volume.clamp(0.0, 1.5);
            master_track.pan = pt.pan.clamp(-1.0, 1.0);
            master_track.mute = pt.mute;
            master_track.solo = pt.solo;
            if let Some(c) = pt.color {
                master_track.color = Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
            }
            id_to_track_idx.insert(pt.id.clone(), 0);
        } else {
            let idx = regular_tracks.len() + 1;
            let mut tr = Track::new(idx, pt.name.clone(), false);
            tr.volume = pt.volume.clamp(0.0, 1.5);
            tr.pan = pt.pan.clamp(-1.0, 1.0);
            tr.mute = pt.mute;
            tr.solo = pt.solo;
            tr.matrix_idx = Some(idx - 1);
            if let Some(c) = pt.color {
                tr.color = Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
            }
            id_to_track_idx.insert(pt.id.clone(), idx);
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

    let num_regular = state.live_tracks.len().saturating_sub(1).max(1);
    let max_clip_slot = parsed
        .clips
        .iter()
        .filter(|c| c.is_launcher)
        .map(|c| c.slot_index + 1)
        .max()
        .unwrap_or(0);
    let num_scenes = state.matrix_state.scenes.len().max(8).max(max_clip_slot);
    while state.matrix_state.scenes.len() < num_scenes {
        let idx = state.matrix_state.scenes.len() + 1;
        state
            .matrix_state
            .scenes
            .push(crate::views::matrix::SceneMeta {
                name: format!("Scene {}", idx),
                color: None,
                launch_quantization: None,
            });
    }
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

    let mut pl = PlaylistState::default();
    let ppqn = pl.ppqn.max(1) as f64;
    let base_dir = bwproject_path.parent();
    let mut next_id = 1usize;

    for pc in &parsed.clips {
        let track_idx = id_to_track_idx
            .get(&pc.track_id)
            .copied()
            .filter(|&i| i > 0 && i < state.live_tracks.len())
            .unwrap_or(1);
        let color = pc
            .color
            .map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
            .unwrap_or_else(|| state.live_tracks[track_idx].color);

        if pc.is_launcher {
            let m_idx = track_idx - 1;
            let s_idx = pc.slot_index.min(num_scenes - 1);
            let bpm = parsed.transport.bpm.max(1.0);
            let dur_secs = pc.duration_beats * (60.0 / bpm);
            let content = match &pc.content {
                BwClipContent::Audio { .. } => ClipData::Audio {
                    events: Vec::new(),
                    next_event_id: 1,
                    preview_mix: Vec::new(),
                    preview_sr: 44100,
                },
                BwClipContent::Midi { notes } => ClipData::Midi {
                    notes: notes.clone(),
                },
                BwClipContent::Empty => continue,
            };
            let path = match &pc.content {
                BwClipContent::Audio { path } => resolve_path(base_dir, path),
                _ => PathBuf::new(),
            };
            let initial_loop_end = match &content {
                ClipData::Audio { preview_sr, .. } => {
                    (dur_secs * (*preview_sr).max(1) as f64).round().max(1.0) as u64
                }
                ClipData::Midi { .. } => (pc.duration_beats * ppqn).round().max(1.0) as u64,
            };
            let mut mclip = MatrixClip {
                id: next_id,
                name: pc.name.clone(),
                path: path.clone(),
                duration_secs: dur_secs,
                content,
                local_state: PlaylistState::default(),
                local_track: state.live_tracks[track_idx].clone(),
                local_bar: 0.0,
                loop_start: 0,
                loop_end: initial_loop_end,
                loop_enabled: true,
                launch_legato: pc.launch_legato,
                has_time_selection: false,
                peaks: Vec::new(),
                muted: pc.muted,
                color: Some(color),
            };

            if matches!(mclip.content, ClipData::Audio { .. }) && !path.as_os_str().is_empty() {
                if let Some((samples, channels, sr)) = crate::views::matrix::decode_audio_file(&path)
                {
                    let total_frames = samples.len() / channels.max(1);
                    if sr > 0 && total_frames > 0 {
                        mclip.duration_secs = total_frames as f64 / sr as f64;
                        mclip.loop_end = total_frames as u64;
                    }
                    let ev_name = path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let mut ev = crate::views::matrix::AudioEvent::new_full(
                        mclip.alloc_event_id(),
                        ev_name,
                        samples,
                        channels,
                        sr,
                        0.0,
                    );
                    ev.source_path = Some(path.clone());
                    if let Some(events) = mclip.audio_events_mut() {
                        events.push(ev);
                    }
                    mclip.refresh_preview();
                }
                mclip.peaks =
                    crate::views::hikaru_native_plugins::hikaru_opendms::open_dms_sampler::load_peaks_from_wav(
                        &path.to_string_lossy(),
                        MatrixClip::PEAK_BINS,
                    );

                // Registrar el clip en el motor de audio para que suene al disparar el pad.
                state.audio_proxy.send(GuiCommand::LoadClip {
                    clip_id: mclip.id,
                    path: path.to_string_lossy().to_string(),
                    position_secs: 0.0,
                    duration_secs: 0.0,
                    offset_secs: 0.0,
                    track_index: m_idx,
                    scene_index: s_idx,
                    speed_ratio: crate::audio_proxy::compute_tempo_sync_ratio(&path, bpm),
                });
                if !mclip.audio_events().is_empty() {
                    crate::views::matrix::sync_audio_events_to_engine(
                        &mclip,
                        &state.audio_proxy,
                        m_idx,
                        s_idx,
                    );
                }
                let clip_sr = match &mclip.content {
                    ClipData::Audio { preview_sr, .. } if *preview_sr > 0 => *preview_sr as f64,
                    _ => 44100.0,
                };
                state.audio_proxy.send(GuiCommand::SetClipLoop {
                    track_idx: m_idx,
                    scene_idx: s_idx,
                    start_secs: (mclip.loop_start as f64 / clip_sr) as f32,
                    end_secs: (mclip.loop_end as f64 / clip_sr) as f32,
                    enabled: mclip.loop_enabled,
                });
            } else if let ClipData::Midi { notes } = &mclip.content {
                state.audio_proxy.send(GuiCommand::UpdateMidiClipNotes {
                    track_idx: m_idx,
                    scene_idx: s_idx,
                    notes: notes.clone(),
                });
                state.audio_proxy.send(GuiCommand::SetMidiClipLoop {
                    track_idx: m_idx,
                    scene_idx: s_idx,
                    loop_start_ticks: mclip.loop_start,
                    loop_end_ticks: mclip.loop_end.max(mclip.loop_start + 240),
                });
                state.audio_proxy.send(GuiCommand::SetMidiClipLoopEnabled {
                    track_idx: m_idx,
                    scene_idx: s_idx,
                    enabled: mclip.loop_enabled,
                });
            }
            state.audio_proxy.send(GuiCommand::SetClipLegato {
                track_idx: m_idx,
                scene_idx: s_idx,
                legato: mclip.launch_legato,
            });

            state.matrix_state.grid[m_idx][s_idx].state = SlotState::Stopped;
            state.matrix_state.grid[m_idx][s_idx].clip = Some(mclip);
            next_id += 1;
        } else {
            let start_tick = (pc.start_beats * ppqn).round() as u64;
            let duration_ticks = (pc.duration_beats * ppqn).round().max(1.0) as u64;
            let clip_type = match &pc.content {
                BwClipContent::Audio { path } => {
                    let p = resolve_path(base_dir, path);
                    let peaks = if !p.as_os_str().is_empty() {
                        crate::views::hikaru_native_plugins::hikaru_opendms::open_dms_sampler::load_peaks_from_wav(
                            &p.to_string_lossy(),
                            MatrixClip::PEAK_BINS,
                        )
                    } else {
                        Vec::new()
                    };
                    ClipType::Audio {
                        sample_path: p.to_string_lossy().to_string(),
                        peaks,
                        sample_offset_ticks: 0,
                        total_sample_ticks: duration_ticks,
                    }
                }
                BwClipContent::Midi { .. } | BwClipContent::Empty => ClipType::Pattern {
                    pattern_id: next_id,
                },
            };
            pl.clips.push((
                track_idx,
                PlaylistClip {
                    id: next_id,
                    name: pc.name.clone(),
                    start_tick,
                    duration_ticks,
                    clip_type,
                    color,
                    muted: pc.muted,
                    loop_enabled: false,
                },
            ));
            next_id += 1;
        }
    }

    pl.next_clip_id = next_id;
    pl.needs_full_sync = true;
    state.playlist_state = pl;
    state.matrix_state.next_clip_id = next_id;
}

/// Parser para el formato binario nativo de Bitwig Studio (`BtWg`).
///
/// Cabecera ASCII de 42 bytes:
/// `BtWg` + version(4 hex) + type(4 hex) + rev(4 hex) + body_offset(8 hex) + ...
///
/// Extrae del cuerpo binario sin compresión:
/// - BPM: campo `0x000002c8` + tag `0x07` + `f64 BE`
/// - Pistas: campo `0x0000015b` + tag `0x08` + string `u32 BE` (opcionalmente
///   seguido de color `0x0000054c` + tag `0x16` + `4 x f32 BE`)
/// - Clips del Launcher: campo `0x00000286` + tag `0x08` + string `u32 BE`
///   asociados a la pista inmediatamente anterior, con detección opcional de
///   ruta de sample `0x00000d3a` + tag `0x08`.
fn parse_native_bitwig_btwg(bytes: &[u8]) -> Result<ParsedBwProject, String> {
    if bytes.len() < 42 {
        return Err("cabecera BtWg truncada".to_string());
    }
    let body_offset = std::str::from_utf8(&bytes[16..24])
        .ok()
        .and_then(|s| usize::from_str_radix(s, 16).ok())
        .filter(|&off| off >= 42 && off < bytes.len())
        .unwrap_or(42);
    let body = &bytes[body_offset..];

    // 1. BPM: campo 0x000002c8 + tag 0x07 + f64 BE
    let mut bpm = 120.0f64;
    let bpm_pat = [0x00, 0x00, 0x02, 0xc8, 0x07];
    let scan_end = body.len().min(16384);
    if let Some(pos) = body[..scan_end]
        .windows(bpm_pat.len())
        .position(|w| w == bpm_pat)
    {
        if pos + 13 <= body.len() {
            let val = f64::from_be_bytes([
                body[pos + 5],
                body[pos + 6],
                body[pos + 7],
                body[pos + 8],
                body[pos + 9],
                body[pos + 10],
                body[pos + 11],
                body[pos + 12],
            ]);
            if val.is_finite() && (20.0..=999.0).contains(&val) {
                bpm = val;
            }
        }
    }

    // 2. Pistas: campo 0x0000015b + tag 0x08 + u32 BE len + UTF-8
    //    Opcionalmente seguido de color 0x0000054c + tag 0x16 + 4 x f32 BE.
    let trk_pat = [0x00, 0x00, 0x01, 0x5b, 0x08];
    let col_pat = [0x00, 0x00, 0x05, 0x4c, 0x16];
    let mut tracks: Vec<BwTrack> = Vec::new();
    let mut track_offsets: Vec<(usize, String, Option<[u8; 4]>)> = Vec::new();
    let mut pos = 0usize;
    let mut next_trk_num = 1usize;

    while pos + 9 <= body.len() {
        let Some(rel) = body[pos..].windows(trk_pat.len()).position(|w| w == trk_pat) else {
            break;
        };
        let i = pos + rel;
        pos = i + 5;
        if i + 9 > body.len() {
            break;
        }
        let slen = u32::from_be_bytes([body[i + 5], body[i + 6], body[i + 7], body[i + 8]]) as usize;
        if slen == 0 || slen > 128 || i + 9 + slen > body.len() {
            continue;
        }
        let Ok(name) = std::str::from_utf8(&body[i + 9..i + 9 + slen]) else {
            continue;
        };
        if name == "Project" || !name.chars().all(|c| !c.is_control()) {
            continue;
        }

        let after_off = i + 9 + slen;
        let mut color = None;
        if after_off + 21 <= body.len() && body[after_off..after_off + 5] == col_pat {
            let r = f32::from_be_bytes([
                body[after_off + 5],
                body[after_off + 6],
                body[after_off + 7],
                body[after_off + 8],
            ]);
            let g = f32::from_be_bytes([
                body[after_off + 9],
                body[after_off + 10],
                body[after_off + 11],
                body[after_off + 12],
            ]);
            let b = f32::from_be_bytes([
                body[after_off + 13],
                body[after_off + 14],
                body[after_off + 15],
                body[after_off + 16],
            ]);
            let a = f32::from_be_bytes([
                body[after_off + 17],
                body[after_off + 18],
                body[after_off + 19],
                body[after_off + 20],
            ]);
            if a > 0.01 {
                color = Some([
                    (r.clamp(0.0, 1.0) * 255.0).round() as u8,
                    (g.clamp(0.0, 1.0) * 255.0).round() as u8,
                    (b.clamp(0.0, 1.0) * 255.0).round() as u8,
                    (a.clamp(0.0, 1.0) * 255.0).round() as u8,
                ]);
            }
        }

        let is_master = name.eq_ignore_ascii_case("Master");
        // En BtWg, los buses FX (Mid Reverb, Delay) tienen color alpha=0; los
        // omitimos de las pistas regulares del Launcher si no tienen color.
        if !is_master && color.is_none() {
            continue;
        }
        let id = if is_master {
            "trk_0".to_string()
        } else {
            let s = format!("trk_{}", next_trk_num);
            next_trk_num += 1;
            s
        };
        if !is_master {
            track_offsets.push((i, id.clone(), color));
        }
        tracks.push(BwTrack {
            id,
            name: name.to_string(),
            is_master,
            volume: 0.85,
            pan: 0.0,
            mute: false,
            solo: false,
            color,
        });
    }

    if !tracks.iter().any(|t| t.is_master) {
        tracks.insert(
            0,
            BwTrack {
                id: "trk_0".to_string(),
                name: "Master".to_string(),
                is_master: true,
                volume: 1.0,
                pan: 0.0,
                mute: false,
                solo: false,
                color: None,
            },
        );
    }

    // 3. Clips del Launcher: campo 0x00000286 + tag 0x08 + u32 BE len + UTF-8
    let clip_pat = [0x00, 0x00, 0x02, 0x86, 0x08];
    let sample_pat = [0x00, 0x00, 0x0d, 0x3a, 0x08];
    let mut clips: Vec<BwClip> = Vec::new();
    let mut per_track_slot: HashMap<String, usize> = HashMap::new();
    let mut cpos = 0usize;

    while cpos + 9 <= body.len() {
        let Some(rel) = body[cpos..].windows(clip_pat.len()).position(|w| w == clip_pat) else {
            break;
        };
        let i = cpos + rel;
        cpos = i + 5;
        if i + 9 > body.len() {
            break;
        }
        let slen = u32::from_be_bytes([body[i + 5], body[i + 6], body[i + 7], body[i + 8]]) as usize;
        if slen == 0 || slen > 128 || i + 9 + slen > body.len() {
            continue;
        }
        let Ok(cname) = std::str::from_utf8(&body[i + 9..i + 9 + slen]) else {
            continue;
        };
        if !cname.chars().all(|c| !c.is_control()) {
            continue;
        }

        // Asociar a la pista regular inmediatamente anterior por offset.
        let owner = track_offsets.iter().rev().find(|(toff, _, _)| *toff < i);
        let Some((_, track_id, track_color)) = owner else {
            continue;
        };

        let slot_entry = per_track_slot.entry(track_id.clone()).or_insert(0);
        let slot_index = *slot_entry;
        *slot_entry += 1;

        // Buscar si el clip referencia un sample cercano (`0x00000d3a 0x08`)
        let search_limit = (i + 9 + slen + 2048).min(body.len());
        let mut content = BwClipContent::Midi { notes: Vec::new() };
        if let Some(srel) = body[i + 9 + slen..search_limit]
            .windows(sample_pat.len())
            .position(|w| w == sample_pat)
        {
            let si = i + 9 + slen + srel;
            if si + 9 <= body.len() {
                let plen =
                    u32::from_be_bytes([body[si + 5], body[si + 6], body[si + 7], body[si + 8]])
                        as usize;
                if plen > 0 && plen < 256 && si + 9 + plen <= body.len() {
                    if let Ok(spath) = std::str::from_utf8(&body[si + 9..si + 9 + plen]) {
                        if spath.ends_with(".wav")
                            || spath.ends_with(".flac")
                            || spath.ends_with(".ogg")
                            || spath.ends_with(".mp3")
                        {
                            content = BwClipContent::Audio {
                                path: spath.to_string(),
                            };
                        }
                    }
                }
            }
        }

        clips.push(BwClip {
            name: cname.to_string(),
            track_id: track_id.clone(),
            is_launcher: true,
            slot_index,
            start_beats: 0.0,
            duration_beats: 4.0,
            muted: false,
            launch_legato: true,
            color: *track_color,
            content,
        });
    }

    Ok(ParsedBwProject {
        transport: BwTransport {
            bpm,
            beats_per_bar: 4,
            beat_division: 4,
        },
        tracks,
        clips,
    })
}

fn resolve_path(base_dir: Option<&Path>, raw: &str) -> PathBuf {
    let p = PathBuf::from(raw);
    if p.is_absolute() {
        p
    } else if let Some(dir) = base_dir {
        dir.join(p)
    } else {
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bwproject_tlv_round_trip() {
        let project = ParsedBwProject {
            transport: BwTransport {
                bpm: 128.0,
                beats_per_bar: 4,
                beat_division: 4,
            },
            tracks: vec![
                BwTrack {
                    id: "trk_0".to_string(),
                    name: "Master".to_string(),
                    is_master: true,
                    volume: 1.0,
                    pan: 0.0,
                    mute: false,
                    solo: false,
                    color: None,
                },
                BwTrack {
                    id: "trk_1".to_string(),
                    name: "Bass".to_string(),
                    is_master: false,
                    volume: 0.85,
                    pan: -0.1,
                    mute: false,
                    solo: false,
                    color: Some([0xE8, 0x6F, 0x4C, 0xFF]),
                },
            ],
            clips: vec![BwClip {
                name: "Bassline".to_string(),
                track_id: "trk_1".to_string(),
                is_launcher: false,
                slot_index: 0,
                start_beats: 0.0,
                duration_beats: 4.0,
                muted: false,
                launch_legato: true,
                color: None,
                content: BwClipContent::Midi {
                    notes: vec![(0, 36, 100, 480), (480, 48, 90, 480)],
                },
            }],
        };

        let bytes = encode_bwproject(&project).expect("encode should succeed");
        let parsed = parse_bwproject_bytes(&bytes).expect("parse should succeed");
        assert!((parsed.transport.bpm - 128.0).abs() < 1e-6);
        assert_eq!(parsed.tracks.len(), 2);
        assert_eq!(parsed.clips.len(), 1);
        assert_eq!(parsed.clips[0].name, "Bassline");
    }

    #[test]
    fn rejects_invalid_magic() {
        let err = parse_bwproject_bytes(b"NOTABWPROJECT").unwrap_err();
        assert!(err.contains("cabecera"));
    }
}
