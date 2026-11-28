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
use crate::project::btwg_reader::{BtWgDocument, BtWgObject, BtWgReader, BtWgValue};
use crate::project::bwdevice_shim::{
    extract_modulators_to_bay, translate_device_document, BwDeviceTranslation,
};
use crate::project::common::{self, ProjectError};
use crate::views::automation_editor::{AutomationLane, AutomationPoint, AutomationTarget};
use crate::views::matrix::{ClipData, MatrixClip, MatrixSlot, SlotState, TrackMeta};
use crate::views::mixer::{DspSlot, ExternalParamState, Track};
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
pub struct BwTrackDevice {
    pub track_id: String,
    pub slot_index: usize,
    pub name: String,
    pub active: bool,
    pub preset_name: String,
    pub is_grid: bool,
    pub dsp_slot: Option<hikaru_project_file::DspSlot>,
    pub native_params: Vec<(String, f32)>,
    pub wet_fx_chain: Vec<BwTrackDevice>,
}

impl BwTrackDevice {
    fn to_json(&self) -> Value {
        json!({
            "track_id": self.track_id,
            "slot_index": self.slot_index,
            "name": self.name,
            "active": self.active,
            "preset_name": self.preset_name,
            "is_grid": self.is_grid,
            "dsp_slot": self.dsp_slot.as_ref().and_then(|s| serde_json::to_value(s).ok()),
            "native_params": self.native_params.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>(),
            "wet_fx_chain": self.wet_fx_chain.iter().map(|w| w.to_json()).collect::<Vec<_>>(),
        })
    }

    fn from_json(v: &Value) -> Option<Self> {
        let dsp_slot = v
            .get("dsp_slot")
            .filter(|d| !d.is_null())
            .and_then(|d| serde_json::from_value::<hikaru_project_file::DspSlot>(d.clone()).ok());
        let native_params = v
            .get("native_params")
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| {
                        let pair = p.as_array()?;
                        Some((pair.first()?.as_str()?.to_string(), pair.get(1)?.as_f64()? as f32))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let wet_fx_chain = v
            .get("wet_fx_chain")
            .and_then(|a| a.as_array())
            .map(|arr| arr.iter().filter_map(Self::from_json).collect())
            .unwrap_or_default();
        Some(Self {
            track_id: v.get("track_id")?.as_str()?.to_string(),
            slot_index: v.get("slot_index").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
            name: v.get("name")?.as_str()?.to_string(),
            active: v.get("active").and_then(|x| x.as_bool()).unwrap_or(true),
            preset_name: v
                .get("preset_name")
                .and_then(|x| x.as_str())
                .unwrap_or("init")
                .to_string(),
            is_grid: v.get("is_grid").and_then(|x| x.as_bool()).unwrap_or(false),
            dsp_slot,
            native_params,
            wet_fx_chain,
        })
    }
}

#[derive(Debug, Clone)]
pub struct BwAutomationLane {
    pub target_kind: String,
    pub track_id: Option<String>,
    pub fx_idx: Option<usize>,
    pub slot_idx: Option<usize>,
    pub param_id: Option<u32>,
    pub points: Vec<(u64, f32)>,
}

impl BwAutomationLane {
    fn to_json(&self) -> Value {
        json!({
            "target_kind": self.target_kind,
            "track_id": self.track_id,
            "fx_idx": self.fx_idx,
            "slot_idx": self.slot_idx,
            "param_id": self.param_id,
            "points": self.points.iter().map(|(t, v)| json!([t, v])).collect::<Vec<_>>(),
        })
    }

    fn from_json(v: &Value) -> Option<Self> {
        let points = v
            .get("points")
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| {
                        let pair = p.as_array()?;
                        Some((pair.first()?.as_u64()?, pair.get(1)?.as_f64()? as f32))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            target_kind: v.get("target_kind")?.as_str()?.to_string(),
            track_id: v.get("track_id").and_then(|x| x.as_str()).map(String::from),
            fx_idx: v.get("fx_idx").and_then(|x| x.as_u64()).map(|u| u as usize),
            slot_idx: v.get("slot_idx").and_then(|x| x.as_u64()).map(|u| u as usize),
            param_id: v.get("param_id").and_then(|x| x.as_u64()).map(|u| u as u32),
            points,
        })
    }
}

#[derive(Debug, Clone)]
pub struct BwTrackModulatorBay {
    pub track_id: String,
    pub slot_idx: usize,
    pub bay: hikaru_project_file::DeviceModulatorBayParams,
}

impl BwTrackModulatorBay {
    fn to_json(&self) -> Value {
        json!({
            "track_id": self.track_id,
            "slot_idx": self.slot_idx,
            "bay": serde_json::to_value(&self.bay).unwrap_or(Value::Null),
        })
    }

    fn from_json(v: &Value) -> Option<Self> {
        let bay = serde_json::from_value::<hikaru_project_file::DeviceModulatorBayParams>(
            v.get("bay")?.clone(),
        )
        .ok()?;
        Some(Self {
            track_id: v.get("track_id")?.as_str()?.to_string(),
            slot_idx: v.get("slot_idx").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
            bay,
        })
    }
}

#[derive(Debug, Clone)]
pub struct BwEmbeddedAudio {
    pub original_path: String,
    pub filename: String,
    pub data_b64: String,
}

impl BwEmbeddedAudio {
    fn to_json(&self) -> Value {
        json!({
            "original_path": self.original_path,
            "filename": self.filename,
            "data_b64": self.data_b64,
        })
    }

    fn from_json(v: &Value) -> Option<Self> {
        Some(Self {
            original_path: v.get("original_path")?.as_str()?.to_string(),
            filename: v.get("filename")?.as_str()?.to_string(),
            data_b64: v.get("data_b64")?.as_str()?.to_string(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct BwPluginState {
    pub track_id: String,
    pub slot_index: usize,
    pub name: String,
    pub plugin_path: String,
    pub plugin_format: String,
    pub state_b64: Option<String>,
    pub cached_params: Vec<(u32, String, f32)>,
    pub rack_overrides: Vec<(u32, f32)>,
}

impl BwPluginState {
    fn to_json(&self) -> Value {
        json!({
            "track_id": self.track_id,
            "slot_index": self.slot_index,
            "name": self.name,
            "plugin_path": self.plugin_path,
            "plugin_format": self.plugin_format,
            "state_b64": self.state_b64,
            "cached_params": self.cached_params.iter().map(|(id, n, v)| json!([id, n, v])).collect::<Vec<_>>(),
            "rack_overrides": self.rack_overrides.iter().map(|(id, v)| json!([id, v])).collect::<Vec<_>>(),
        })
    }

    fn from_json(v: &Value) -> Option<Self> {
        let cached_params = v
            .get("cached_params")
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| {
                        let tup = p.as_array()?;
                        Some((
                            tup.first()?.as_u64()? as u32,
                            tup.get(1)?.as_str()?.to_string(),
                            tup.get(2)?.as_f64()? as f32,
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let rack_overrides = v
            .get("rack_overrides")
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| {
                        let pair = p.as_array()?;
                        Some((pair.first()?.as_u64()? as u32, pair.get(1)?.as_f64()? as f32))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            track_id: v.get("track_id")?.as_str()?.to_string(),
            slot_index: v.get("slot_index").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
            name: v.get("name")?.as_str()?.to_string(),
            plugin_path: v
                .get("plugin_path")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            plugin_format: v
                .get("plugin_format")
                .and_then(|x| x.as_str())
                .unwrap_or("VST3")
                .to_string(),
            state_b64: v.get("state_b64").and_then(|x| x.as_str()).map(String::from),
            cached_params,
            rack_overrides,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ParsedBwProject {
    pub transport: BwTransport,
    pub tracks: Vec<BwTrack>,
    pub clips: Vec<BwClip>,
    pub devices: Vec<BwTrackDevice>,
    pub automation: Vec<BwAutomationLane>,
    pub modulators: Vec<BwTrackModulatorBay>,
    pub embedded_audio: Vec<BwEmbeddedAudio>,
    pub plugin_states: Vec<BwPluginState>,
}

// =========================================================================
// BASE64 HELPERS (sin dependencias externas)
// =========================================================================

const B64_TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn encode_base64(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64_TABLE[((n >> 18) & 63) as usize] as char);
        out.push(B64_TABLE[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            B64_TABLE[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64_TABLE[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
    let bytes: Vec<u8> = input
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    if bytes.len() % 4 != 0 {
        return Err("longitud base64 inválida".to_string());
    }
    let mut out = Vec::with_capacity((bytes.len() / 4) * 3);
    for chunk in bytes.chunks_exact(4) {
        let mut vals = [0u32; 4];
        let mut pad = 0usize;
        for (i, &b) in chunk.iter().enumerate() {
            if b == b'=' {
                vals[i] = 0;
                pad += 1;
            } else if let Some(pos) = B64_TABLE.iter().position(|&c| c == b) {
                vals[i] = pos as u32;
            } else {
                return Err(format!("carácter base64 inválido: {}", b as char));
            }
        }
        let n = (vals[0] << 18) | (vals[1] << 12) | (vals[2] << 6) | vals[3];
        out.push(((n >> 16) & 0xff) as u8);
        if pad < 2 {
            out.push(((n >> 8) & 0xff) as u8);
        }
        if pad == 0 {
            out.push((n & 0xff) as u8);
        }
    }
    Ok(out)
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
    let devs_val = Value::Array(project.devices.iter().map(|d| d.to_json()).collect());
    let auto_val = json!({
        "lanes": project.automation.iter().map(|a| a.to_json()).collect::<Vec<_>>(),
        "modulators": project.modulators.iter().map(|m| m.to_json()).collect::<Vec<_>>(),
    });
    let medi_val = json!({
        "embedded_audio": project.embedded_audio.iter().map(|e| e.to_json()).collect::<Vec<_>>(),
        "plugin_states": project.plugin_states.iter().map(|p| p.to_json()).collect::<Vec<_>>(),
    });

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
    let mut devices: Vec<BwTrackDevice> = Vec::new();
    let mut automation: Vec<BwAutomationLane> = Vec::new();
    let mut modulators: Vec<BwTrackModulatorBay> = Vec::new();
    let mut embedded_audio: Vec<BwEmbeddedAudio> = Vec::new();
    let mut plugin_states: Vec<BwPluginState> = Vec::new();

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
        } else if tag == TAG_DEVS {
            if let Ok(arr) = serde_json::from_slice::<Vec<Value>>(payload) {
                devices.extend(arr.iter().filter_map(BwTrackDevice::from_json));
            }
        } else if tag == TAG_AUTO {
            if let Ok(v) = serde_json::from_slice::<Value>(payload) {
                if let Some(arr) = v.as_array() {
                    automation.extend(arr.iter().filter_map(BwAutomationLane::from_json));
                } else if v.is_object() {
                    if let Some(arr) = v.get("lanes").and_then(|x| x.as_array()) {
                        automation.extend(arr.iter().filter_map(BwAutomationLane::from_json));
                    }
                    if let Some(arr) = v.get("modulators").and_then(|x| x.as_array()) {
                        modulators.extend(arr.iter().filter_map(BwTrackModulatorBay::from_json));
                    }
                }
            }
        } else if tag == TAG_MEDI {
            if let Ok(v) = serde_json::from_slice::<Value>(payload) {
                if let Some(arr) = v.get("embedded_audio").and_then(|x| x.as_array()) {
                    embedded_audio.extend(arr.iter().filter_map(BwEmbeddedAudio::from_json));
                }
                if let Some(arr) = v.get("plugin_states").and_then(|x| x.as_array()) {
                    plugin_states.extend(arr.iter().filter_map(BwPluginState::from_json));
                }
            }
        }
    }

    Ok(ParsedBwProject {
        transport,
        tracks,
        clips,
        devices,
        automation,
        modulators,
        embedded_audio,
        plugin_states,
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
    let mut devices = Vec::new();
    let mut modulators = Vec::new();
    let mut plugin_states = Vec::new();

    for tr in &state.live_tracks {
        let track_id = format!("trk_{}", tr.id);
        tracks.push(BwTrack {
            id: track_id.clone(),
            name: tr.name.clone(),
            is_master: tr.is_master,
            volume: tr.volume,
            pan: tr.pan,
            mute: tr.mute,
            solo: tr.solo,
            color: Some(tr.color.to_array()),
        });

        let captured = common::capture_track(tr);
        for (slot_idx, (live_slot, file_slot)) in
            tr.effects.iter().zip(captured.effects.iter()).enumerate()
        {
            if let Some(ref bay) = file_slot.modulator_bay {
                if !bay.slots.is_empty() || bay.open {
                    modulators.push(BwTrackModulatorBay {
                        track_id: track_id.clone(),
                        slot_idx,
                        bay: bay.clone(),
                    });
                }
            }
            if live_slot.external.is_some() || live_slot.saved_state.is_some() {
                let plugin_path = live_slot
                    .external
                    .as_ref()
                    .map(|p| p.path.to_string_lossy().to_string())
                    .unwrap_or_default();
                let plugin_format = live_slot
                    .external
                    .as_ref()
                    .map(|p| match p.format {
                        hikaru_plugin_host::PluginFormat::VST3 => "VST3".to_string(),
                        hikaru_plugin_host::PluginFormat::CLAP => "CLAP".to_string(),
                    })
                    .unwrap_or_else(|| "VST3".to_string());
                plugin_states.push(BwPluginState {
                    track_id: track_id.clone(),
                    slot_index: slot_idx,
                    name: live_slot.name.clone(),
                    plugin_path,
                    plugin_format,
                    state_b64: live_slot.saved_state.as_deref().map(encode_base64),
                    cached_params: live_slot
                        .cached_params
                        .iter()
                        .map(|p| (p.info.id, p.info.name.clone(), p.value))
                        .collect(),
                    rack_overrides: live_slot.rack_overrides_since_state.clone(),
                });
            }
            if live_slot.name != crate::views::mixer::EMPTY_SLOT_NAME {
                devices.push(capture_bw_device(
                    &track_id,
                    slot_idx,
                    live_slot,
                    file_slot.clone(),
                ));
            }
        }
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

    let mut automation = Vec::new();
    for lane in &state.automation.lanes {
        let track_id_for = |idx: usize| -> String {
            state
                .live_tracks
                .get(idx)
                .map(|t| format!("trk_{}", t.id))
                .unwrap_or_else(|| format!("trk_{}", idx))
        };
        let (target_kind, track_id, fx_idx, slot_idx, param_id) = match &lane.target {
            AutomationTarget::TransportTempo => {
                ("transport_tempo".to_string(), None, None, None, None)
            }
            AutomationTarget::MixerVolume { track_idx } => (
                "mixer_volume".to_string(),
                Some(track_id_for(*track_idx)),
                None,
                None,
                None,
            ),
            AutomationTarget::MixerPan { track_idx } => (
                "mixer_pan".to_string(),
                Some(track_id_for(*track_idx)),
                None,
                None,
                None,
            ),
            AutomationTarget::TrackSend { track_idx, fx_idx } => (
                "track_send".to_string(),
                Some(track_id_for(*track_idx)),
                Some(*fx_idx),
                None,
                None,
            ),
            AutomationTarget::ExternalPluginParam {
                track_idx,
                slot_idx,
                param_id,
            } => (
                "external_plugin_param".to_string(),
                Some(track_id_for(*track_idx)),
                None,
                Some(*slot_idx),
                Some(*param_id),
            ),
        };
        automation.push(BwAutomationLane {
            target_kind,
            track_id,
            fx_idx,
            slot_idx,
            param_id,
            points: lane.points.iter().map(|p| (p.tick, p.value)).collect(),
        });
    }

    let mut embedded_audio = Vec::new();
    let mut seen_audio_paths = std::collections::HashSet::new();
    for c in &clips {
        if let BwClipContent::Audio { path } = &c.content {
            if !path.is_empty() && seen_audio_paths.insert(path.clone()) {
                let p = Path::new(path);
                if let Ok(bytes) = std::fs::read(p) {
                    let filename = p
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "audio.wav".to_string());
                    embedded_audio.push(BwEmbeddedAudio {
                        original_path: path.clone(),
                        filename,
                        data_b64: encode_base64(&bytes),
                    });
                }
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
        devices,
        automation,
        modulators,
        embedded_audio,
        plugin_states,
    };

    encode_bwproject(&project)
}

fn capture_bw_device(
    track_id: &str,
    slot_index: usize,
    live_slot: &DspSlot,
    file_slot: hikaru_project_file::DspSlot,
) -> BwTrackDevice {
    let wet_fx_chain = live_slot
        .wet_fx_chain
        .iter()
        .enumerate()
        .map(|(w_idx, w_slot)| {
            let mut dummy_track = Track::new(0, "Wet".to_string(), false);
            dummy_track.effects.push(w_slot.clone());
            let captured = common::capture_track(&dummy_track);
            capture_bw_device(
                track_id,
                w_idx,
                w_slot,
                captured.effects.into_iter().next().unwrap_or(hikaru_project_file::DspSlot {
                    id: w_slot.id,
                    name: w_slot.name.clone(),
                    active: w_slot.active,
                    wavetable_path: None,
                    plugin_path: None,
                    plugin_format: None,
                    open_wavetable: None,
                    open_modulation: None,
                    sampler: None,
                    open_poly: None,
                    open_grid: None,
                    modulator_bay: None,
                }),
            )
        })
        .collect();
    BwTrackDevice {
        track_id: track_id.to_string(),
        slot_index,
        name: live_slot.name.clone(),
        active: live_slot.active,
        preset_name: live_slot.preset_name.clone(),
        is_grid: live_slot.is_open_grid() || DspSlot::is_open_grid_name(&live_slot.name),
        dsp_slot: Some(file_slot),
        native_params: Vec::new(),
        wet_fx_chain,
    }
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

    // bw-04: Restaurar dispositivos nativos y Grid por pista
    let mut devs_by_track: HashMap<String, Vec<&BwTrackDevice>> = HashMap::new();
    for dev in &parsed.devices {
        devs_by_track
            .entry(dev.track_id.clone())
            .or_default()
            .push(dev);
    }
    for (tid_str, &track_idx) in &id_to_track_idx {
        if let Some(track) = all_tracks.get_mut(track_idx) {
            if let Some(mut list) = devs_by_track.remove(tid_str) {
                list.sort_by_key(|d| d.slot_index);
                for dev in list {
                    while track.effects.len() < dev.slot_index {
                        let next_slot_id = track.effects.len() + 1;
                        track.effects.push(DspSlot::new(
                            next_slot_id,
                            crate::views::mixer::EMPTY_SLOT_NAME.to_string(),
                        ));
                    }
                    let slot = build_dsp_slot_from_bw_device(track.id, dev);
                    if dev.slot_index < track.effects.len() {
                        track.effects[dev.slot_index] = slot;
                    } else {
                        track.effects.push(slot);
                    }
                }
            }
        }
    }

    // bw-06: Restaurar estado de plugins externos VST3/CLAP
    for pstate in &parsed.plugin_states {
        let Some(&track_idx) = id_to_track_idx.get(&pstate.track_id) else {
            continue;
        };
        let Some(track) = all_tracks.get_mut(track_idx) else {
            continue;
        };
        while track.effects.len() <= pstate.slot_index {
            let next_slot_id = track.effects.len() + 1;
            track.effects.push(DspSlot::new(
                next_slot_id,
                crate::views::mixer::EMPTY_SLOT_NAME.to_string(),
            ));
        }
        let slot = &mut track.effects[pstate.slot_index];
        if slot.name == crate::views::mixer::EMPTY_SLOT_NAME {
            slot.name = pstate.name.clone();
        }
        if !pstate.plugin_path.is_empty() {
            let path = PathBuf::from(&pstate.plugin_path);
            let format = match pstate.plugin_format.as_str() {
                "CLAP" => hikaru_plugin_host::PluginFormat::CLAP,
                _ => hikaru_plugin_host::PluginFormat::VST3,
            };
            slot.external = Some(hikaru_plugin_host::registry::PluginDescriptor {
                id: format!("{format:?}:{}", path.display()).to_lowercase(),
                name: pstate.name.clone(),
                vendor: String::new(),
                path,
                format,
                kind: hikaru_plugin_host::PluginKind::Effect,
                has_gui: false,
                input_channels: None,
            });
        }
        if let Some(ref b64) = pstate.state_b64 {
            if let Ok(raw) = decode_base64(b64) {
                slot.saved_state = Some(raw);
            }
        }
        slot.rack_overrides_since_state = pstate.rack_overrides.clone();
        if !pstate.cached_params.is_empty() {
            slot.cached_params = pstate
                .cached_params
                .iter()
                .map(|(pid, pname, val)| ExternalParamState {
                    info: hikaru_plugin_host::PluginParamInfo {
                        id: *pid,
                        name: pname.clone(),
                        unit: String::new(),
                        min: 0.0,
                        max: 1.0,
                        default_value: *val,
                        step_count: 0,
                        can_automate: true,
                        is_modulatable: true,
                    },
                    value: *val,
                    pending_to_engine: false,
                    display_text: None,
                    last_sent_effective: None,
                    last_sent_mod: 0.0,
                })
                .collect();
        }
    }

    // bw-05: Restaurar estado de moduladores acoplados a slots
    for m in &parsed.modulators {
        if let Some(&track_idx) = id_to_track_idx.get(&m.track_id) {
            if let Some(track) = all_tracks.get_mut(track_idx) {
                if let Some(slot) = track.effects.get_mut(m.slot_idx) {
                    slot.modulators_open = m.bay.open;
                }
                crate::views::hikaru_native_plugins::hikaru_openmodulation::restore_bay_params(
                    track.id,
                    m.slot_idx,
                    &m.bay,
                );
            }
        }
    }

    state.live_tracks = all_tracks.clone();
    state.studio_tracks = all_tracks;

    // bw-05: Restaurar carriles de automatización
    state.automation.lanes.clear();
    for lane in &parsed.automation {
        let resolve_tidx = |tid: &Option<String>| -> Option<usize> {
            tid.as_ref().and_then(|s| id_to_track_idx.get(s).copied())
        };
        let target = match lane.target_kind.as_str() {
            "transport_tempo" => Some(AutomationTarget::TransportTempo),
            "mixer_volume" => resolve_tidx(&lane.track_id)
                .map(|track_idx| AutomationTarget::MixerVolume { track_idx }),
            "mixer_pan" => resolve_tidx(&lane.track_id)
                .map(|track_idx| AutomationTarget::MixerPan { track_idx }),
            "track_send" => resolve_tidx(&lane.track_id).map(|track_idx| {
                AutomationTarget::TrackSend {
                    track_idx,
                    fx_idx: lane.fx_idx.unwrap_or(0),
                }
            }),
            "external_plugin_param" => resolve_tidx(&lane.track_id).map(|track_idx| {
                AutomationTarget::ExternalPluginParam {
                    track_idx,
                    slot_idx: lane.slot_idx.unwrap_or(0),
                    param_id: lane.param_id.unwrap_or(0),
                }
            }),
            _ => None,
        };
        if let Some(target) = target {
            let mut restored = AutomationLane::new(target);
            restored.points = lane
                .points
                .iter()
                .map(|&(tick, value)| AutomationPoint {
                    tick,
                    value: if value.is_finite() {
                        value.clamp(0.0, 1.0)
                    } else {
                        0.5
                    },
                })
                .collect();
            restored.sort_points();
            state.automation.lanes.push(restored);
        }
    }

    // bw-06: Extraer audio embebido a carpeta temporal para resolución de clips
    let mut embedded_audio_map: HashMap<String, PathBuf> = HashMap::new();
    if !parsed.embedded_audio.is_empty() {
        let proj_hash = bwproject_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("project");
        let media_dir = std::env::temp_dir()
            .join("hikaru_bwproject_media")
            .join(proj_hash);
        let _ = std::fs::create_dir_all(&media_dir);
        for (idx, aud) in parsed.embedded_audio.iter().enumerate() {
            if let Ok(raw) = decode_base64(&aud.data_b64) {
                let safe_name = Path::new(&aud.filename)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("sample.wav");
                let out_path = media_dir.join(format!("{idx}_{safe_name}"));
                if std::fs::write(&out_path, &raw).is_ok() {
                    embedded_audio_map.insert(aud.original_path.clone(), out_path);
                }
            }
        }
    }

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
                BwClipContent::Audio { path } => {
                    let resolved = resolve_path(base_dir, path);
                    if resolved.exists() {
                        resolved
                    } else if let Some(emb) = embedded_audio_map.get(path) {
                        emb.clone()
                    } else {
                        resolved
                    }
                }
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
                    let resolved = resolve_path(base_dir, path);
                    let p = if resolved.exists() {
                        resolved
                    } else if let Some(emb) = embedded_audio_map.get(path) {
                        emb.clone()
                    } else {
                        resolved
                    };
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

fn build_dsp_slot_from_bw_device(track_num_id: usize, dev: &BwTrackDevice) -> DspSlot {
    let mut slot = if let Some(ref file_slot) = dev.dsp_slot {
        let mut dummy = common::Track {
            id: track_num_id,
            name: String::new(),
            volume: 1.0,
            pan: 0.0,
            pan_mode: 0,
            mute: false,
            solo: false,
            arm: false,
            is_master: false,
            route_destination_id: 0,
            sends: Vec::new(),
            effects: vec![file_slot.clone()],
            matrix_idx: None,
        };
        dummy.effects[0].id = dev.slot_index + 1;
        let built = common::build_track(&dummy);
        built
            .effects
            .into_iter()
            .next()
            .unwrap_or_else(|| DspSlot::new(dev.slot_index + 1, dev.name.clone()))
    } else {
        let mapped_name = if dev.is_grid && !DspSlot::is_open_grid_name(&dev.name) {
            "Hikaru OpenGrid".to_string()
        } else {
            dev.name.clone()
        };
        let mut s = DspSlot::new(dev.slot_index + 1, mapped_name);
        s.active = dev.active;
        s
    };
    slot.preset_name = dev.preset_name.clone();
    slot.wet_fx_chain = dev
        .wet_fx_chain
        .iter()
        .map(|w| build_dsp_slot_from_bw_device(track_num_id, w))
        .collect();
    slot
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

    // 4. Dispositivos nativos, Grid, moduladores y media desde el AST `BtWgDocument`
    let mut devices: Vec<BwTrackDevice> = Vec::new();
    let mut automation: Vec<BwAutomationLane> = Vec::new();
    let mut modulators: Vec<BwTrackModulatorBay> = Vec::new();
    let mut embedded_audio: Vec<BwEmbeddedAudio> = Vec::new();
    let mut plugin_states: Vec<BwPluginState> = Vec::new();

    if let Ok(doc) = BtWgReader::parse_document(bytes) {
        let default_track_id = tracks
            .iter()
            .find(|t| !t.is_master)
            .map(|t| t.id.clone())
            .unwrap_or_else(|| "trk_0".to_string());

        if let Some(root) = &doc.body {
            let mut per_track_slot: HashMap<String, usize> = HashMap::new();
            collect_btwg_devices_and_media(
                root,
                &doc,
                &default_track_id,
                &mut per_track_slot,
                &mut devices,
                &mut automation,
                &mut modulators,
                &mut embedded_audio,
                &mut plugin_states,
                0,
            );
        }

        // Si el documento entero representa un dispositivo/preset raíz con moduladores
        if devices.is_empty() {
            let trans = translate_device_document(&doc, None);
            let dev_name = match &trans {
                BwDeviceTranslation::Builtin { bitwig_name, .. } => bitwig_name.clone(),
                BwDeviceTranslation::Placeholder(p) => p.bitwig_name.clone(),
            };
            if !dev_name.is_empty() && dev_name != "Unknown Device" {
                let bay = extract_modulators_to_bay(&doc, &dev_name);
                if !bay.slots.is_empty() {
                    modulators.push(BwTrackModulatorBay {
                        track_id: default_track_id.clone(),
                        slot_idx: 0,
                        bay,
                    });
                }
                let is_grid = dev_name.to_ascii_lowercase().contains("grid");
                devices.push(BwTrackDevice {
                    track_id: default_track_id,
                    slot_index: 0,
                    name: if is_grid {
                        "Hikaru OpenGrid".to_string()
                    } else {
                        dev_name
                    },
                    active: true,
                    preset_name: "init".to_string(),
                    is_grid,
                    dsp_slot: None,
                    native_params: Vec::new(),
                    wet_fx_chain: Vec::new(),
                });
            }
        }
    }

    Ok(ParsedBwProject {
        transport: BwTransport {
            bpm,
            beats_per_bar: 4,
            beat_division: 4,
        },
        tracks,
        clips,
        devices,
        automation,
        modulators,
        embedded_audio,
        plugin_states,
    })
}

#[allow(clippy::too_many_arguments)]
fn collect_btwg_devices_and_media(
    obj: &BtWgObject,
    doc: &BtWgDocument,
    current_track_id: &str,
    per_track_slot: &mut HashMap<String, usize>,
    devices: &mut Vec<BwTrackDevice>,
    automation: &mut Vec<BwAutomationLane>,
    modulators: &mut Vec<BwTrackModulatorBay>,
    embedded_audio: &mut Vec<BwEmbeddedAudio>,
    plugin_states: &mut Vec<BwPluginState>,
    depth: usize,
) {
    if depth > 24 {
        return;
    }

    let active_track = current_track_id.to_string();
    let mut device_name: Option<String> = None;
    let mut is_plugin_blob: Option<Vec<u8>> = None;
    let mut plugin_path: Option<String> = None;

    for (_, val) in &obj.fields {
        if let BtWgValue::Str(s) = val {
            if s.eq_ignore_ascii_case("Poly Grid")
                || s.eq_ignore_ascii_case("FX Grid")
                || s.eq_ignore_ascii_case("Note Grid")
                || s.eq_ignore_ascii_case("Polymer")
                || s.eq_ignore_ascii_case("Polysynth")
                || s.eq_ignore_ascii_case("Sampler")
                || s.eq_ignore_ascii_case("EQ+")
                || s.eq_ignore_ascii_case("Tool")
                || s.eq_ignore_ascii_case("Delay-2")
                || s.eq_ignore_ascii_case("Reverb")
                || s.eq_ignore_ascii_case("Compressor")
            {
                device_name = Some(s.clone());
            } else if s.ends_with(".vst3") || s.ends_with(".clap") {
                plugin_path = Some(s.clone());
            }
        } else if let BtWgValue::Bytes(raw) = val {
            if raw.len() > 16 && raw.len() <= 2 * 1024 * 1024 {
                is_plugin_blob = Some(raw.clone());
            }
        }
    }

    if let Some(dname) = device_name {
        let slot_entry = per_track_slot.entry(active_track.clone()).or_insert(0);
        let slot_index = *slot_entry;
        *slot_entry += 1;

        let is_grid = dname.to_ascii_lowercase().contains("grid");
        let mapped_name = if is_grid {
            "Hikaru OpenGrid".to_string()
        } else {
            dname.clone()
        };
        devices.push(BwTrackDevice {
            track_id: active_track.clone(),
            slot_index,
            name: mapped_name,
            active: true,
            preset_name: dname.clone(),
            is_grid,
            dsp_slot: None,
            native_params: Vec::new(),
            wet_fx_chain: Vec::new(),
        });

        let sub_doc = BtWgDocument {
            header: doc.header.clone(),
            kind: doc.kind,
            meta: None,
            body: Some(obj.clone()),
            objects_by_id: doc.objects_by_id.clone(),
        };
        let bay = extract_modulators_to_bay(&sub_doc, &dname);
        if !bay.slots.is_empty() {
            modulators.push(BwTrackModulatorBay {
                track_id: active_track.clone(),
                slot_idx: slot_index,
                bay,
            });
        }
    }

    if let (Some(ppath), Some(blob)) = (plugin_path, is_plugin_blob) {
        let slot_index = per_track_slot.get(&active_track).copied().unwrap_or(0);
        let format = if ppath.ends_with(".clap") {
            "CLAP".to_string()
        } else {
            "VST3".to_string()
        };
        let pname = Path::new(&ppath)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("External Plugin")
            .to_string();
        plugin_states.push(BwPluginState {
            track_id: active_track.clone(),
            slot_index,
            name: pname,
            plugin_path: ppath,
            plugin_format: format,
            state_b64: Some(encode_base64(&blob)),
            cached_params: Vec::new(),
            rack_overrides: Vec::new(),
        });
    }

    for (_, val) in &obj.fields {
        match val {
            BtWgValue::Obj(child) | BtWgValue::ObjKeyInline { obj: child } => {
                collect_btwg_devices_and_media(
                    child,
                    doc,
                    current_track_id,
                    per_track_slot,
                    devices,
                    automation,
                    modulators,
                    embedded_audio,
                    plugin_states,
                    depth + 1,
                );
            }
            BtWgValue::ObjList(list) => {
                for child in list {
                    collect_btwg_devices_and_media(
                        child,
                        doc,
                        current_track_id,
                        per_track_slot,
                        devices,
                        automation,
                        modulators,
                        embedded_audio,
                        plugin_states,
                        depth + 1,
                    );
                }
            }
            _ => {}
        }
    }
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
            devices: Vec::new(),
            automation: Vec::new(),
            modulators: Vec::new(),
            embedded_audio: Vec::new(),
            plugin_states: Vec::new(),
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

    fn make_test_app_state() -> AppState {
        use std::sync::atomic::{AtomicU32, AtomicU64};
        use std::sync::mpsc::channel;
        use std::sync::Arc;
        use crate::audio_proxy::AudioProxy;

        let (tx, _rx) = channel();
        AppState::new(
            AudioProxy::new(tx),
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU32::new(0)),
            None,
        )
    }

    #[test]
    fn bw_04_devices_and_grid_round_trip() {
        let mut src = make_test_app_state();
        let mut grid_slot = DspSlot::new(1, "Hikaru OpenGrid".to_string());
        grid_slot.preset_name = "Poly Grid Patch".to_string();
        let mut wet_fx = DspSlot::new(2, "Reverb".to_string());
        wet_fx.active = false;
        grid_slot.wet_fx_chain.push(wet_fx);

        src.live_tracks[1].effects.push(grid_slot);

        let bytes = build_bwproject_bytes(&src).expect("build bwproject");
        let parsed = parse_bwproject_bytes(&bytes).expect("parse bwproject");
        assert_eq!(parsed.devices.len(), 1);
        assert!(parsed.devices[0].is_grid);
        assert_eq!(parsed.devices[0].preset_name, "Poly Grid Patch");
        assert_eq!(parsed.devices[0].wet_fx_chain.len(), 1);

        let mut dst = make_test_app_state();
        apply_parsed_bwproject(&mut dst, &parsed, Path::new("/tmp/test_bw04.bwproject"));
        assert_eq!(dst.live_tracks[1].effects.len(), 1);
        assert!(dst.live_tracks[1].effects[0].is_open_grid());
        assert_eq!(dst.live_tracks[1].effects[0].preset_name, "Poly Grid Patch");
        assert_eq!(dst.live_tracks[1].effects[0].wet_fx_chain.len(), 1);
        assert!(!dst.live_tracks[1].effects[0].wet_fx_chain[0].active);
    }

    #[test]
    fn bw_05_automation_and_modulators_round_trip() {
        let mut src = make_test_app_state();
        src.live_tracks[1]
            .effects
            .push(DspSlot::new(1, "Hikaru OpenPoly".to_string()));

        let mut tempo_lane = AutomationLane::new(AutomationTarget::TransportTempo);
        tempo_lane.insert_point(0, 0.25);
        tempo_lane.insert_point(960, 0.75);
        let mut vol_lane = AutomationLane::new(AutomationTarget::MixerVolume { track_idx: 1 });
        vol_lane.insert_point(480, 0.6);
        src.automation.lanes.push(tempo_lane);
        src.automation.lanes.push(vol_lane);

        let bay = hikaru_project_file::DeviceModulatorBayParams {
            open: true,
            slots: vec![hikaru_project_file::ModulatorSlotParams {
                id: 1,
                kind: "LFO".to_string(),
                enabled: true,
                params: vec![2.5],
                bool_flags: vec![true],
                mode_idx: 0,
                routes: vec![hikaru_project_file::ModulatorRouteParams {
                    out_idx: 0,
                    target: "Filter Cutoff".to_string(),
                    amount: 0.42,
                }],
            }],
        };
        crate::views::hikaru_native_plugins::hikaru_openmodulation::restore_bay_params(
            src.live_tracks[1].id,
            0,
            &bay,
        );

        let bytes = build_bwproject_bytes(&src).expect("build bwproject");
        let parsed = parse_bwproject_bytes(&bytes).expect("parse bwproject");
        assert_eq!(parsed.automation.len(), 2);
        assert_eq!(parsed.modulators.len(), 1);
        assert_eq!(parsed.modulators[0].bay.slots[0].kind, "Classic LFO");

        let mut dst = make_test_app_state();
        apply_parsed_bwproject(&mut dst, &parsed, Path::new("/tmp/test_bw05.bwproject"));
        assert_eq!(dst.automation.lanes.len(), 2);
        assert!(dst.automation.has_tempo_automation());
        assert_eq!(
            dst.automation
                .lane_for(&AutomationTarget::MixerVolume { track_idx: 1 })
                .map(|l| l.points.len()),
            Some(1)
        );
        assert!(dst.live_tracks[1].effects[0].modulators_open);
    }

    #[test]
    fn bw_06_embedded_audio_and_plugin_state_round_trip() {
        let mut src = make_test_app_state();

        // Crear un WAV temporal para probar audio embebido
        let wav_path = std::env::temp_dir().join("hikaru_bw06_embed_test.wav");
        {
            let spec = hound::WavSpec {
                channels: 1,
                sample_rate: 44100,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let mut writer = hound::WavWriter::create(&wav_path, spec).expect("create wav");
            for _ in 0..441 {
                writer.write_sample(1000i16).expect("write sample");
            }
            writer.finalize().expect("finalize wav");
        }

        src.playlist_state.clips.push((
            1,
            PlaylistClip {
                id: 1,
                name: "Embedded Kick".to_string(),
                start_tick: 0,
                duration_ticks: 960,
                clip_type: ClipType::Audio {
                    sample_path: wav_path.to_string_lossy().to_string(),
                    peaks: Vec::new(),
                    sample_offset_ticks: 0,
                    total_sample_ticks: 960,
                },
                color: Color32::from_rgb(255, 85, 0),
                muted: false,
                loop_enabled: false,
            },
        ));

        let mut vst_slot = DspSlot::new(1, "TestSynth".to_string());
        vst_slot.external = Some(hikaru_plugin_host::registry::PluginDescriptor {
            id: "vst3:/plugins/TestSynth.vst3".to_string(),
            name: "TestSynth".to_string(),
            vendor: "Hikaru".to_string(),
            path: PathBuf::from("/plugins/TestSynth.vst3"),
            format: hikaru_plugin_host::PluginFormat::VST3,
            kind: hikaru_plugin_host::PluginKind::Effect,
            has_gui: true,
            input_channels: Some(2),
        });
        vst_slot.saved_state = Some(vec![0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02]);
        vst_slot.rack_overrides_since_state = vec![(7, 0.65)];
        src.live_tracks[1].effects.push(vst_slot);

        let bytes = build_bwproject_bytes(&src).expect("build bwproject");
        let _ = std::fs::remove_file(&wav_path);

        let parsed = parse_bwproject_bytes(&bytes).expect("parse bwproject");
        assert_eq!(parsed.embedded_audio.len(), 1);
        assert_eq!(parsed.plugin_states.len(), 1);

        let mut dst = make_test_app_state();
        apply_parsed_bwproject(&mut dst, &parsed, Path::new("/tmp/test_bw06.bwproject"));

        // El audio embebido debe haberse extraído y resuelto aunque el archivo original fue borrado
        let (_, restored_clip) = &dst.playlist_state.clips[0];
        if let ClipType::Audio { sample_path, .. } = &restored_clip.clip_type {
            assert!(Path::new(sample_path).exists());
            let _ = std::fs::remove_file(sample_path);
        } else {
            panic!("expected audio clip");
        }

        let restored_slot = &dst.live_tracks[1].effects[0];
        assert!(restored_slot.external.is_some());
        assert_eq!(
            restored_slot.saved_state.as_deref(),
            Some(&[0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02][..])
        );
        assert_eq!(restored_slot.rack_overrides_since_state, vec![(7, 0.65)]);
    }
}
