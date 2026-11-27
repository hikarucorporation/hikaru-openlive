// Importador y serializador clean-room de clips nativos de Bitwig Studio (`.bwclip`).
// GNU AGPLv3
// crates/hikaru_gui/src/project/bwclip.rs
//
// Implementa las cuatro capacidades de paridad `.bwclip`:
// - bcl-01: Parser de contenedor binario `.bwclip` (`BtWg00030002` plano o ZIP con media)
// - bcl-02: Importación de notas MIDI, micro-pitch (`0x2e62`) y expresión por nota
// - bcl-03: Clip de audio (`0xee` / `0x105`), warp markers (`0x43`) y media embebida
// - bcl-04: Envolventes de automatización del clip y ajustes de lanzamiento (`loop`, `legato`, quantize)

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::project::btwg::{parse_wav_metadata, sample_hash16, BtWgWriter};
use crate::project::btwg_reader::{BtWgContainerKind, BtWgDocument, BtWgObject, BtWgReader, BtWgValue};
use crate::project::dawproject::{build_zip_store, read_zip_entries};
use crate::views::matrix::{AudioEvent, ClipData, MatrixClip};
use crate::views::playlist::{ClipType, CurvePoint, PlaylistClip, PlaylistState};
use crate::views::mixer::Track;

pub const EXTENSION: &str = "bwclip";
pub const FILE_FILTER: &str = "Bitwig Clip (*.bwclip)";

/// Devuelve `true` si la ruta corresponde a un archivo `.bwclip`.
pub fn is_bwclip_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| ext.eq_ignore_ascii_case(EXTENSION))
        .unwrap_or(false)
}

/// Tipo de contenido del clip `.bwclip`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BwClipKind {
    Midi,
    Audio,
    Hybrid,
    #[default]
    Empty,
}

impl BwClipKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Midi => "MIDI Clip",
            Self::Audio => "Audio Clip",
            Self::Hybrid => "Hybrid Clip",
            Self::Empty => "Clip",
        }
    }
}

/// Punto de expresión por nota (micro-pitch, timbre, presión) dentro de una nota MIDI.
#[derive(Debug, Clone, PartialEq)]
pub struct BwClipExpressionPoint {
    pub offset_beats: f64,
    pub value: f64,
}

/// Nota MIDI extraída de un `.bwclip` (`bcl-02`), incluyendo micro-pitch y expresión.
#[derive(Debug, Clone, PartialEq)]
pub struct BwClipMidiNote {
    pub start_beats: f64,
    pub duration_beats: f64,
    pub key: u8,
    pub velocity: f64,
    pub release_velocity: f64,
    pub muted: bool,
    pub chance: f64,
    pub pitch_offset_semitones: f64,
    pub timbre: f64,
    pub pressure: f64,
    pub gain_db: f64,
    pub pitch_envelope: Vec<BwClipExpressionPoint>,
}

impl Default for BwClipMidiNote {
    fn default() -> Self {
        Self {
            start_beats: 0.0,
            duration_beats: 0.25,
            key: 60,
            velocity: 0.8,
            release_velocity: 0.5,
            muted: false,
            chance: 1.0,
            pitch_offset_semitones: 0.0,
            timbre: 0.0,
            pressure: 0.0,
            gain_db: 0.0,
            pitch_envelope: Vec::new(),
        }
    }
}

/// Marcador de warp / time-stretch (`cls = 0x43`) dentro de un `.bwclip` de audio (`bcl-03`).
#[derive(Debug, Clone, PartialEq)]
pub struct BwClipWarpMarker {
    pub beat_time: f64,
    pub sample_time_secs: f64,
}

/// Datos de audio, warp markers y media embebida de un `.bwclip` (`bcl-03`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BwClipAudioData {
    pub sample_path: Option<String>,
    pub embedded_samples: Vec<(String, Vec<u8>)>,
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_secs: f64,
    pub gain_db: f64,
    pub stretch_mode: String,
    pub warp_markers: Vec<BwClipWarpMarker>,
    pub onset_times_secs: Vec<f64>,
}

/// Punto de una envolvente de automatización del clip (`bcl-04`).
#[derive(Debug, Clone, PartialEq)]
pub struct BwClipAutomationPoint {
    pub time_beats: f64,
    pub value: f64,
    pub slope: f32,
}

/// Pista de automatización interna del clip (`bcl-04`).
#[derive(Debug, Clone, PartialEq)]
pub struct BwClipAutomationLane {
    pub target_param: String,
    pub points: Vec<BwClipAutomationPoint>,
}

/// Ajustes de loop y lanzamiento del clip (`bcl-04`).
#[derive(Debug, Clone, PartialEq)]
pub struct BwClipLaunchSettings {
    pub loop_enabled: bool,
    pub loop_start_beats: f64,
    pub loop_length_beats: f64,
    pub play_start_beats: f64,
    pub play_stop_beats: f64,
    pub launch_legato: bool,
    pub launch_quantize: String,
    pub launch_mode: String,
    pub next_action: Option<String>,
    pub next_action_bars: f64,
}

impl Default for BwClipLaunchSettings {
    fn default() -> Self {
        Self {
            loop_enabled: true,
            loop_start_beats: 0.0,
            loop_length_beats: 4.0,
            play_start_beats: 0.0,
            play_stop_beats: 4.0,
            launch_legato: false,
            launch_quantize: "Default".to_string(),
            launch_mode: "From Start".to_string(),
            next_action: None,
            next_action_bars: 1.0,
        }
    }
}

/// Representación completa de un archivo `.bwclip` parseado (`bcl-01`..`bcl-04`).
#[derive(Debug, Clone)]
pub struct BwClipDocument {
    pub name: String,
    pub kind: BwClipKind,
    pub creator: String,
    pub category: String,
    pub tags: Vec<String>,
    pub comment: String,
    pub bpm: f64,
    pub time_sig_num: u8,
    pub time_sig_den: u8,
    pub duration_beats: f64,
    pub duration_secs: f64,
    pub color: Option<[f32; 4]>,
    pub muted: bool,
    pub notes: Vec<BwClipMidiNote>,
    pub audio: Option<BwClipAudioData>,
    pub automation: Vec<BwClipAutomationLane>,
    pub launch: BwClipLaunchSettings,
}

impl Default for BwClipDocument {
    fn default() -> Self {
        Self {
            name: "Bitwig Clip".to_string(),
            kind: BwClipKind::Empty,
            creator: "Bitwig".to_string(),
            category: "Clip".to_string(),
            tags: vec!["bwclip".to_string()],
            comment: String::new(),
            bpm: 120.0,
            time_sig_num: 4,
            time_sig_den: 4,
            duration_beats: 4.0,
            duration_secs: 2.0,
            color: None,
            muted: false,
            notes: Vec::new(),
            audio: None,
            automation: Vec::new(),
            launch: BwClipLaunchSettings::default(),
        }
    }
}

/// Carga y parsea un `.bwclip` desde disco.
pub fn load_bwclip_file(path: &Path) -> Result<BwClipDocument, String> {
    let bytes = fs::read(path)
        .map_err(|e| format!("no se pudo leer '{}': {e}", path.display()))?;
    let mut doc = parse_bwclip_bytes(&bytes)?;
    if doc.name == "Bitwig Clip" || doc.name.trim().is_empty() {
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            if !stem.trim().is_empty() {
                doc.name = stem.to_string();
            }
        }
    }
    Ok(doc)
}

/// Parsea bytes de un `.bwclip` (`BtWg00030002` directo o paquete ZIP con media embebida).
pub fn parse_bwclip_bytes(bytes: &[u8]) -> Result<BwClipDocument, String> {
    if bytes.starts_with(b"PK\x03\x04") {
        return parse_zip_bwclip(bytes);
    }
    if bytes.starts_with(b"BtWg") {
        let doc = BtWgReader::parse_document(bytes)?;
        return parse_btwg_clip_document(&doc, Vec::new());
    }
    Err("el archivo .bwclip no tiene cabecera BtWg ni contenedor ZIP válido".to_string())
}

fn parse_zip_bwclip(zip_bytes: &[u8]) -> Result<BwClipDocument, String> {
    let entries = read_zip_entries(zip_bytes)?;
    let mut embedded_samples: Vec<(String, Vec<u8>)> = Vec::new();
    let mut clip_entry_bytes: Option<&[u8]> = None;

    for (name, data) in &entries {
        let lower = name.to_ascii_lowercase();
        if (lower.ends_with(".bwclip") || lower == "clip" || lower == "clip.btwg")
            && data.starts_with(b"BtWg")
        {
            clip_entry_bytes = Some(data.as_slice());
        } else if lower.ends_with(".wav")
            || lower.ends_with(".aiff")
            || lower.ends_with(".aif")
            || lower.ends_with(".flac")
            || lower.ends_with(".ogg")
            || lower.ends_with(".mp3")
        {
            embedded_samples.push((name.clone(), data.clone()));
        }
    }

    if clip_entry_bytes.is_none() {
        for (_, data) in &entries {
            if data.starts_with(b"BtWg") {
                clip_entry_bytes = Some(data.as_slice());
                break;
            }
        }
    }

    let raw_clip = clip_entry_bytes
        .ok_or_else(|| "el contenedor ZIP .bwclip no contiene un documento BtWg".to_string())?;
    let doc = BtWgReader::parse_document(raw_clip)?;
    parse_btwg_clip_document(&doc, embedded_samples)
}

fn parse_btwg_clip_document(
    doc: &BtWgDocument,
    embedded_samples: Vec<(String, Vec<u8>)>,
) -> Result<BwClipDocument, String> {
    if doc.kind != BtWgContainerKind::PublicAst {
        return Err(format!(
            "tipo BtWg no soportado para .bwclip: {}",
            doc.header.format_type
        ));
    }

    let mut out = BwClipDocument::default();

    // Metadatos del objeto `meta` (`cls = 4`)
    if let Some(meta) = &doc.meta {
        for (k, v) in &meta.named_fields {
            match (k.as_str(), v) {
                ("name" | "clip_name", BtWgValue::Str(s)) if !s.trim().is_empty() => {
                    out.name = s.clone();
                }
                ("creator", BtWgValue::Str(s)) if !s.trim().is_empty() => {
                    out.creator = s.clone();
                }
                ("preset_category" | "category", BtWgValue::Str(s)) if !s.trim().is_empty() => {
                    out.category = s.clone();
                }
                ("comment", BtWgValue::Str(s)) if !s.trim().is_empty() => {
                    out.comment = s.clone();
                }
                ("tags", BtWgValue::Str(s)) if !s.trim().is_empty() => {
                    for t in s.split(|c: char| c == ',' || c.is_whitespace()) {
                        let trimmed = t.trim();
                        if !trimmed.is_empty() && !out.tags.iter().any(|x| x == trimmed) {
                            out.tags.push(trimmed.to_string());
                        }
                    }
                }
                ("bpm", val) => {
                    if let Some(bpm) = val.as_f64() {
                        if (20.0..=400.0).contains(&bpm) {
                            out.bpm = bpm;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // Buscar el objeto raíz del clip en `body` (`0xee`, `0x47`, `0x105`, `0xbf` o envuelto)
    if let Some(body) = &doc.body {
        extract_clip_from_object(doc, body, &mut out);
    }

    // Si hay samples embebidos en el ZIP y el clip aún no tiene `audio`, crear `BwClipAudioData`
    if !embedded_samples.is_empty() {
        let audio = out.audio.get_or_insert_with(BwClipAudioData::default);
        if audio.sample_path.is_none() {
            audio.sample_path = Some(embedded_samples[0].0.clone());
        }
        if audio.duration_secs <= 0.0 {
            let (sr, ch, dur) = parse_wav_metadata(&embedded_samples[0].1, out.duration_secs);
            audio.sample_rate = sr.max(1) as u32;
            audio.channels = ch.max(1) as u16;
            audio.duration_secs = dur;
        }
        audio.embedded_samples = embedded_samples;
    }

    // Clasificar tipo de clip (`Midi`, `Audio`, `Hybrid`, `Empty`)
    let has_notes = !out.notes.is_empty();
    let has_audio = out.audio.is_some();
    out.kind = match (has_notes, has_audio) {
        (true, true) => BwClipKind::Hybrid,
        (true, false) => BwClipKind::Midi,
        (false, true) => BwClipKind::Audio,
        (false, false) => {
            if out.category.to_ascii_lowercase().contains("audio") {
                BwClipKind::Audio
            } else if out.category.to_ascii_lowercase().contains("midi")
                || out.category.to_ascii_lowercase().contains("note")
            {
                BwClipKind::Midi
            } else {
                BwClipKind::Empty
            }
        }
    };

    if out.category == "Clip" {
        out.category = out.kind.label().to_string();
    }
    if out.duration_beats <= 0.0 {
        out.duration_beats = out.launch.loop_length_beats.max(1.0);
    }
    if out.duration_secs <= 0.0 {
        let bpm = out.bpm.max(20.0);
        out.duration_secs = (out.duration_beats * (60.0 / bpm)).max(0.25);
    }

    Ok(out)
}

fn extract_clip_from_object(doc: &BtWgDocument, obj: &BtWgObject, out: &mut BwClipDocument) {
    match obj.cls {
        // Audio Launcher Clip (`0xee`) o Note Launcher Clip (`0x47`)
        0xee | 0x47 => {
            if let Some(dur) = obj.field(0x26).and_then(|v| v.as_f64()) {
                if dur > 0.0 {
                    out.duration_beats = dur;
                }
            }
            if let Some(muted) = obj.field(0x10f8).and_then(|v| v.as_bool()) {
                out.muted = muted;
            }
            if let Some(BtWgValue::Str(name)) = obj.field(0x286) {
                if !name.trim().is_empty() {
                    out.name = name.clone();
                }
            }
            if let Some(BtWgValue::Color(rgba)) = obj.field(0x280) {
                if rgba[3] > 0.0 {
                    out.color = Some(*rgba);
                }
            }
            if let Some(legato) = obj.field(0xbe1).and_then(|v| v.as_bool()) {
                out.launch.launch_legato = legato;
            }
            if let Some(q) = obj.field(0x3772).and_then(|v| v.as_i32()) {
                out.launch.launch_quantize = match q {
                    0 => "Off".to_string(),
                    1 => "Default".to_string(),
                    2 => "1 Beat".to_string(),
                    3 => "1 Bar".to_string(),
                    _ => format!("Mode {q}"),
                };
            } else if let Some(BtWgValue::I8(q)) = obj.field(0x3772) {
                out.launch.launch_quantize = match *q {
                    0 => "Off".to_string(),
                    1 => "Default".to_string(),
                    2 => "1 Beat".to_string(),
                    3 => "1 Bar".to_string(),
                    other => format!("Mode {other}"),
                };
            }
            if let Some(BtWgValue::Str(next_act)) = obj.field(0xb8e) {
                if !next_act.trim().is_empty() {
                    out.launch.next_action = Some(next_act.clone());
                }
            }
            if let Some(bars) = obj.field(0x1709).and_then(|v| v.as_f64()) {
                if bars > 0.0 {
                    out.launch.next_action_bars = bars;
                }
            }
            if let Some(BtWgValue::Obj(content)) = obj.field(0x288) {
                extract_clip_from_object(doc, content, out);
            }
        }
        // Audio Clip Content (`0x105`) o Note Clip Content (`0xbf`)
        0x105 | 0xbf => {
            if let Some(BtWgValue::Obj(loop_obj)) = obj.field(0x98f) {
                parse_loop_header(loop_obj, out);
            }
            if let Some(BtWgValue::I32(packed)) = obj.field(0x1d4b) {
                let num = ((*packed >> 24) & 0xff).max(1) as u8;
                let den = (*packed & 0xff).max(1) as u8;
                out.time_sig_num = num;
                out.time_sig_den = den;
            }
            // Notas MIDI (`0x6cc`) o Audio Events (`0x6ce`) en `0x49c`
            if let Some(BtWgValue::Obj(content_container)) = obj.field(0x49c) {
                match content_container.cls {
                    0x6cc => parse_midi_note_container(content_container, out),
                    0x6ce => parse_audio_events_container(doc, content_container, out),
                    _ => {
                        parse_midi_note_container(content_container, out);
                        parse_audio_events_container(doc, content_container, out);
                    }
                }
            }
            // Envolventes de automatización en `0x49d`, `0xbac`, `0xbad`
            for fid in [0x49d_u32, 0xbac, 0xbad] {
                if let Some(BtWgValue::ObjList(lanes)) = obj.field(fid) {
                    parse_automation_lanes(lanes, out);
                }
            }
        }
        _ => {
            // Buscar recursivamente en campos hijos por si viene envuelto en un contenedor
            for (_, val) in &obj.fields {
                match val {
                    BtWgValue::Obj(child) => extract_clip_from_object(doc, child, out),
                    BtWgValue::ObjList(list) => {
                        for child in list {
                            extract_clip_from_object(doc, child, out);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn parse_loop_header(loop_obj: &BtWgObject, out: &mut BwClipDocument) {
    if let Some(enabled) = loop_obj.field(0x991).and_then(|v| v.as_bool()) {
        out.launch.loop_enabled = enabled;
    }
    if let Some(BtWgValue::Obj(range)) = loop_obj.field(0x992) {
        if let Some(start) = range.field(0x2af).and_then(|v| v.as_f64()) {
            out.launch.loop_start_beats = start.max(0.0);
        }
        if let Some(len) = range.field(0x26).and_then(|v| v.as_f64()) {
            if len > 0.0 {
                out.launch.loop_length_beats = len;
                if out.duration_beats <= 0.0 {
                    out.duration_beats = len;
                }
            }
        }
    }
    if let Some(BtWgValue::Obj(play_start)) = loop_obj.field(0x98c) {
        if let Some(start) = play_start.field(0x2af).and_then(|v| v.as_f64()) {
            out.launch.play_start_beats = start.max(0.0);
        }
    }
    if let Some(BtWgValue::Obj(play_stop)) = loop_obj.field(0x98d) {
        if let Some(stop) = play_stop.field(0x2af).and_then(|v| v.as_f64()) {
            if stop > 0.0 {
                out.launch.play_stop_beats = stop;
            }
        }
    }
}

fn parse_midi_note_container(container: &BtWgObject, out: &mut BwClipDocument) {
    let Some(BtWgValue::ObjList(pitch_groups)) = container.field(0x18cb) else {
        return;
    };
    for group in pitch_groups {
        let group_key = match group.field(0xee) {
            Some(BtWgValue::I8(k)) => *k,
            Some(BtWgValue::I32(k)) => (*k).clamp(0, 127) as u8,
            _ => 60,
        };
        let Some(BtWgValue::ObjList(notes)) = group.field(0x21f) else {
            continue;
        };
        for note_obj in notes {
            let start_beats = note_obj
                .field(0x2af)
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0)
                .max(0.0);
            let duration_beats = note_obj
                .field(0x26)
                .and_then(|v| v.as_f64())
                .unwrap_or(0.25)
                .max(0.01);
            let muted = note_obj
                .field(0x10f8)
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let velocity = note_obj
                .field(0xef)
                .and_then(|v| v.as_f64())
                .unwrap_or(0.8)
                .clamp(0.0, 1.0);
            let release_velocity = note_obj
                .field(0xf0)
                .and_then(|v| v.as_f64())
                .unwrap_or(0.5)
                .clamp(0.0, 1.0);
            let chance = note_obj
                .field(0x2dfc)
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0)
                .clamp(0.0, 1.0);
            let pitch_offset_semitones = note_obj
                .field(0x2e62)
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let timbre = note_obj
                .field(0x2e6a)
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let pressure = note_obj
                .field(0x2e6e)
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let gain_db = note_obj
                .field(0x2e6d)
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);

            let mut pitch_envelope = Vec::new();
            if let Some(BtWgValue::Obj(expr_obj)) = note_obj.field(0x288) {
                for fid in [0xc33_u32, 0xc34, 0xc35] {
                    if let Some(BtWgValue::ObjList(pts)) = expr_obj.field(fid) {
                        for pt in pts {
                            let offset_beats =
                                pt.field(0x2af).and_then(|v| v.as_f64()).unwrap_or(0.0);
                            let value = pt.field(0xa6).and_then(|v| v.as_f64()).unwrap_or(0.0);
                            pitch_envelope.push(BwClipExpressionPoint {
                                offset_beats,
                                value,
                            });
                        }
                    }
                }
            }

            out.notes.push(BwClipMidiNote {
                start_beats,
                duration_beats,
                key: group_key,
                velocity,
                release_velocity,
                muted,
                chance,
                pitch_offset_semitones,
                timbre,
                pressure,
                gain_db,
                pitch_envelope,
            });
        }
    }
    out.notes.sort_by(|a, b| {
        a.start_beats
            .partial_cmp(&b.start_beats)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.key.cmp(&b.key))
    });
}

fn parse_audio_events_container(
    doc: &BtWgDocument,
    container: &BtWgObject,
    out: &mut BwClipDocument,
) {
    let Some(BtWgValue::ObjList(tracks)) = container.field(0x18cb) else {
        return;
    };
    let mut audio_data = out.audio.take().unwrap_or_default();
    let mut found_audio = false;

    for track_obj in tracks {
        let Some(BtWgValue::ObjList(events)) = track_obj.field(0x21f) else {
            continue;
        };
        for ev_obj in events {
            found_audio = true;
            if let Some(dur_secs) = ev_obj.field(0xc05).and_then(|v| v.as_f64()) {
                if dur_secs > 0.0 {
                    audio_data.duration_secs = dur_secs;
                    out.duration_secs = dur_secs;
                }
            }
            if let Some(gain_db) = ev_obj.field(0x2e6d).and_then(|v| v.as_f64()) {
                audio_data.gain_db = gain_db;
            }
            if let Some(BtWgValue::Obj(playback)) = ev_obj.field(0x288) {
                // Sample path / stream ref (`0x20d` -> `0x1db` -> `0x4b4` / `0x286`)
                if let Some(BtWgValue::Obj(stream_ref)) = playback.field(0x20d) {
                    if let Some(BtWgValue::Str(p)) = stream_ref.field(0x286) {
                        if !p.trim().is_empty() {
                            audio_data.sample_path = Some(p.clone());
                        }
                    } else if let Some(BtWgValue::Ref(ref_id)) = stream_ref.field(0x4b4) {
                        if let Some(target) = doc.resolve_ref(*ref_id) {
                            if let Some(BtWgValue::Str(p)) = target.field(0x286) {
                                if !p.trim().is_empty() {
                                    audio_data.sample_path = Some(p.clone());
                                }
                            }
                        }
                    }
                }
                if let Some(BtWgValue::Str(direct_path)) = playback.field(0x286) {
                    if !direct_path.trim().is_empty() {
                        audio_data.sample_path = Some(direct_path.clone());
                    }
                }
                if let Some(BtWgValue::Bytes(raw_wav)) = playback.field(0x1cd9) {
                    if !raw_wav.is_empty() {
                        let (sr, ch, dur) = parse_wav_metadata(raw_wav, audio_data.duration_secs);
                        audio_data.sample_rate = sr.max(1) as u32;
                        audio_data.channels = ch.max(1) as u16;
                        audio_data.duration_secs = dur;
                        let name = audio_data
                            .sample_path
                            .clone()
                            .unwrap_or_else(|| "embedded_clip.wav".to_string());
                        audio_data.sample_path = Some(name.clone());
                        audio_data.embedded_samples.push((name, raw_wav.clone()));
                    }
                }
                // Stretch mode (`0x4b0`)
                if let Some(BtWgValue::I8(mode)) = playback.field(0x4b0) {
                    audio_data.stretch_mode = match *mode {
                        0 => "Raw".to_string(),
                        1 => "Stretch".to_string(),
                        2 => "Stretch HD".to_string(),
                        3 => "Elastique".to_string(),
                        other => format!("Mode {other}"),
                    };
                }
                // Warp / Stretch markers (`0x192` -> `0x8b` -> `0x43`)
                if let Some(BtWgValue::Obj(warp_list)) = playback.field(0x192) {
                    if let Some(BtWgValue::ObjList(markers)) = warp_list.field(0x21f) {
                        for m in markers {
                            let beat_time = m.field(0x2af).and_then(|v| v.as_f64()).unwrap_or(0.0);
                            let sample_time_secs =
                                m.field(0xa6).and_then(|v| v.as_f64()).unwrap_or(0.0);
                            audio_data.warp_markers.push(BwClipWarpMarker {
                                beat_time,
                                sample_time_secs,
                            });
                        }
                    }
                }
                // Onset markers (`0x10ea` -> `0x36b` -> `0xb6`)
                if let Some(BtWgValue::Obj(onset_wrap)) = playback.field(0x10ea) {
                    if let Some(BtWgValue::Obj(onset_list)) = onset_wrap.field(0x10e9) {
                        if let Some(BtWgValue::ObjList(onsets)) = onset_list.field(0x21f) {
                            for o in onsets {
                                if let Some(t) = o.field(0xa6).and_then(|v| v.as_f64()) {
                                    audio_data.onset_times_secs.push(t);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if found_audio {
        out.audio = Some(audio_data);
    }
}

fn parse_automation_lanes(lanes: &[BtWgObject], out: &mut BwClipDocument) {
    for lane in lanes {
        let target_param = lane
            .field(0x286)
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("Clip Automation")
            .to_string();

        let mut points = Vec::new();
        let pts_list = lane
            .field(0x21f)
            .or_else(|| lane.field(0x18cb))
            .and_then(|v| match v {
                BtWgValue::ObjList(l) => Some(l.as_slice()),
                _ => None,
            });
        if let Some(pts) = pts_list {
            for pt in pts {
                let time_beats = pt.field(0x2af).and_then(|v| v.as_f64()).unwrap_or(0.0);
                let value = pt
                    .field(0xa6)
                    .or_else(|| pt.field(0xef))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let slope = pt
                    .field(0x2e62)
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0) as f32;
                points.push(BwClipAutomationPoint {
                    time_beats,
                    value,
                    slope,
                });
            }
        }
        if !points.is_empty() {
            out.automation.push(BwClipAutomationLane {
                target_param,
                points,
            });
        }
    }
}

/// Materializa la primera muestra de audio embebida en un directorio temporal determinístico
/// para que el motor de audio y el generador de picos puedan leerla desde disco.
pub fn materialize_embedded_sample(clip: &BwClipDocument) -> Option<PathBuf> {
    let audio = clip.audio.as_ref()?;
    if let Some((rel_name, bytes)) = audio.embedded_samples.first() {
        let hash = sample_hash16(bytes);
        let hex: String = hash.iter().take(6).map(|b| format!("{b:02x}")).collect();
        let dir = std::env::temp_dir().join("hikaru_bwclip_media").join(hex);
        let file_name = Path::new(rel_name)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("clip_audio.wav");
        let dest = dir.join(file_name);
        if !dest.exists() {
            if fs::create_dir_all(&dir).is_ok() {
                let _ = fs::write(&dest, bytes);
            }
        }
        if dest.exists() {
            return Some(dest);
        }
    }
    audio
        .sample_path
        .as_deref()
        .map(PathBuf::from)
        .filter(|p| p.exists())
}

/// Convierte un `BwClipDocument` importado en un `MatrixClip` listo para el Clip Launcher.
pub fn bwclip_to_matrix_clip(doc: &BwClipDocument, clip_id: usize, project_bpm: f64) -> MatrixClip {
    let bpm = if project_bpm > 0.0 {
        project_bpm
    } else {
        doc.bpm.max(20.0)
    };
    let ppqn = 960.0_f64;
    let loop_start = (doc.launch.loop_start_beats.max(0.0) * ppqn).round() as u64;
    let loop_end = ((doc.launch.loop_start_beats + doc.launch.loop_length_beats).max(0.25) * ppqn)
        .round() as u64;
    let color = doc.color.map(|rgba| {
        egui::Color32::from_rgba_unmultiplied(
            (rgba[0].clamp(0.0, 1.0) * 255.0).round() as u8,
            (rgba[1].clamp(0.0, 1.0) * 255.0).round() as u8,
            (rgba[2].clamp(0.0, 1.0) * 255.0).round() as u8,
            (rgba[3].clamp(0.0, 1.0) * 255.0).round() as u8,
        )
    });

    let materialized_path = materialize_embedded_sample(doc);
    let has_audio = doc.kind == BwClipKind::Audio
        || (doc.notes.is_empty() && materialized_path.is_some());

    let (content, duration_secs, peaks, path) = if has_audio {
        let mut events = Vec::new();
        let mut dur_secs = doc.duration_secs.max(0.25);
        let mut peaks = Vec::new();
        let sample_path = materialized_path.clone().unwrap_or_default();

        if let Some(ref p) = materialized_path {
            if let Some((samples, channels, sr)) = crate::views::matrix::decode_audio_file(p) {
                let mut ev = AudioEvent::new_full(1, doc.name.clone(), samples, channels, sr, 0.0);
                ev.source_path = Some(p.clone());
                dur_secs = ev.end_secs().max(dur_secs);
                events.push(ev);
                peaks = crate::views::hikaru_native_plugins::hikaru_opendms::open_dms_sampler::load_peaks_from_wav(
                    &p.to_string_lossy(),
                    MatrixClip::PEAK_BINS,
                );
            }
        }

        (
            ClipData::Audio {
                events,
                next_event_id: 2,
                preview_mix: Vec::new(),
                preview_sr: 44100,
            },
            dur_secs,
            peaks,
            sample_path,
        )
    } else {
        let notes: Vec<(u64, u8, u8, u32)> = doc
            .notes
            .iter()
            .filter(|n| !n.muted)
            .map(|n| {
                let start_tick = (n.start_beats.max(0.0) * ppqn).round() as u64;
                // Aplicar micro-pitch entero si supera medio semitono; el ajuste fino queda en BwClipDocument
                let pitch = (n.key as i32 + n.pitch_offset_semitones.round() as i32)
                    .clamp(0, 127) as u8;
                let vel = (n.velocity.clamp(0.0, 1.0) * 127.0).round().clamp(1.0, 127.0) as u8;
                let dur_ticks = (n.duration_beats.max(0.0625) * ppqn)
                    .round()
                    .clamp(1.0, u32::MAX as f64) as u32;
                (start_tick, pitch, vel, dur_ticks)
            })
            .collect();
        let dur_secs = (doc.duration_beats.max(1.0) * (60.0 / bpm)).max(0.25);
        (ClipData::Midi { notes }, dur_secs, Vec::new(), PathBuf::new())
    };

    // Mapear envolventes de automatización del clip dentro de `local_state`
    let mut local_state = PlaylistState::default();
    for (idx, lane) in doc.automation.iter().enumerate() {
        let pts: Vec<CurvePoint> = lane
            .points
            .iter()
            .map(|p| CurvePoint {
                rel_tick: (p.time_beats.max(0.0) * ppqn).round() as u64,
                value: p.value as f32,
                tension: p.slope,
            })
            .collect();
        let lane_dur = pts.last().map(|p| p.rel_tick).unwrap_or(loop_end).max(960);
        local_state.clips.push((
            idx + 1,
            PlaylistClip {
                id: idx + 1,
                name: lane.target_param.clone(),
                start_tick: 0,
                duration_ticks: lane_dur,
                clip_type: ClipType::Automation {
                    points: pts,
                    target_param: lane.target_param.clone(),
                },
                color: color.unwrap_or(egui::Color32::from_rgb(0xE8, 0x6A, 0x17)),
                muted: doc.muted,
                loop_enabled: doc.launch.loop_enabled,
            },
        ));
    }

    MatrixClip {
        id: clip_id,
        name: doc.name.clone(),
        path,
        duration_secs,
        content,
        local_state,
        local_track: Track::new(0, doc.name.clone(), false),
        local_bar: 0.0,
        loop_start,
        loop_end: loop_end.max(loop_start + 960),
        loop_enabled: doc.launch.loop_enabled,
        launch_legato: doc.launch.launch_legato,
        has_time_selection: false,
        peaks,
        muted: doc.muted,
        color,
    }
}

/// Convierte un `BwClipDocument` en un `PlaylistClip` para el Arranger Timeline.
pub fn bwclip_to_playlist_clip(
    doc: &BwClipDocument,
    clip_id: usize,
    start_tick: u64,
    ppqn: u64,
) -> PlaylistClip {
    let ppqn_f = ppqn.max(1) as f64;
    let duration_ticks = (doc.duration_beats.max(0.25) * ppqn_f).round() as u64;
    let color = doc
        .color
        .map(|rgba| {
            egui::Color32::from_rgba_unmultiplied(
                (rgba[0].clamp(0.0, 1.0) * 255.0).round() as u8,
                (rgba[1].clamp(0.0, 1.0) * 255.0).round() as u8,
                (rgba[2].clamp(0.0, 1.0) * 255.0).round() as u8,
                (rgba[3].clamp(0.0, 1.0) * 255.0).round() as u8,
            )
        })
        .unwrap_or(egui::Color32::from_rgb(0x3A, 0x9E, 0xD8));

    let materialized = materialize_embedded_sample(doc);
    let clip_type = if let Some(ref p) = materialized {
        let peaks = crate::views::hikaru_native_plugins::hikaru_opendms::open_dms_sampler::load_peaks_from_wav(
            &p.to_string_lossy(),
            MatrixClip::PEAK_BINS,
        );
        ClipType::Audio {
            sample_path: p.to_string_lossy().to_string(),
            peaks,
            sample_offset_ticks: 0,
            total_sample_ticks: duration_ticks,
        }
    } else if let Some(lane) = doc.automation.first() {
        if doc.notes.is_empty() {
            ClipType::Automation {
                points: lane
                    .points
                    .iter()
                    .map(|p| CurvePoint {
                        rel_tick: (p.time_beats.max(0.0) * ppqn_f).round() as u64,
                        value: p.value as f32,
                        tension: p.slope,
                    })
                    .collect(),
                target_param: lane.target_param.clone(),
            }
        } else {
            ClipType::Pattern { pattern_id: clip_id }
        }
    } else {
        ClipType::Pattern { pattern_id: clip_id }
    };

    PlaylistClip {
        id: clip_id,
        name: doc.name.clone(),
        start_tick,
        duration_ticks: duration_ticks.max(ppqn.max(1)),
        clip_type,
        color,
        muted: doc.muted,
        loop_enabled: doc.launch.loop_enabled,
    }
}

/// Serializa un `BwClipDocument` a bytes `.bwclip` (`BtWg00030002` o ZIP si tiene media embebida).
pub fn serialize_bwclip_bytes(doc: &BwClipDocument) -> Vec<u8> {
    let raw_btwg = serialize_btwg_clip_bytes(doc);
    if let Some(audio) = &doc.audio {
        if !audio.embedded_samples.is_empty() {
            let mut entries: Vec<(&str, &[u8])> = Vec::with_capacity(1 + audio.embedded_samples.len());
            entries.push(("clip.bwclip", &raw_btwg));
            for (name, bytes) in &audio.embedded_samples {
                entries.push((name.as_str(), bytes.as_slice()));
            }
            return build_zip_store(&entries);
        }
    }
    raw_btwg
}

fn serialize_btwg_clip_bytes(doc: &BwClipDocument) -> Vec<u8> {
    let mut meta = BtWgWriter::new();
    meta.begin_obj(4);
    meta.raw_str("meta");
    meta.named_field_str("name", &doc.name);
    meta.named_field_str("creator", &doc.creator);
    meta.named_field_str("preset_category", &doc.category);
    if !doc.comment.is_empty() {
        meta.named_field_str("comment", &doc.comment);
    }
    if !doc.tags.is_empty() {
        meta.named_field_str("tags", &doc.tags.join(", "));
    }
    meta.end_obj();
    let meta_bytes = meta.into_bytes();

    let mut body = BtWgWriter::new();
    let is_audio = doc.audio.is_some() && doc.notes.is_empty();
    let launcher_cls = if is_audio { 0xee } else { 0x47 };
    let content_cls = if is_audio { 0x105 } else { 0xbf };

    body.begin_obj(launcher_cls);
    body.field_f64(0x2af, 0.0);
    body.field_f64(0x26, doc.duration_beats.max(0.25));
    body.field_bool(0x10f8, doc.muted);

    body.field_obj_header(0x288, content_cls);
    body.field_i8(0x1fd, 0);

    // Loop header (`0x98f` -> `0x267`)
    body.field_obj_header(0x98f, 0x267);
    body.field_i8(0x1fd, 0);
    body.field_empty_obj_list(0x21f);
    body.field_bool(0x991, doc.launch.loop_enabled);
    body.field_obj_header(0x992, 0x58);
    body.field_f64(0x2af, doc.launch.loop_start_beats);
    body.field_f64(0x26, doc.launch.loop_length_beats.max(0.25));
    body.end_obj();
    body.field_obj_header(0x98c, 0x269);
    body.field_f64(0x2af, doc.launch.play_start_beats);
    body.end_obj();
    body.field_obj_header(0x98d, 0x268);
    body.field_f64(0x2af, doc.launch.play_stop_beats);
    body.end_obj();
    body.end_obj(); // 0x267

    // Automation lanes (`0x49d`)
    if doc.automation.is_empty() {
        body.field_empty_obj_list(0x49d);
    } else {
        body.field_obj_list_start(0x49d);
        for lane in &doc.automation {
            body.begin_obj(0x26a);
            body.field_str(0x286, &lane.target_param);
            body.field_obj_list_start(0x21f);
            for pt in &lane.points {
                body.begin_obj(0x26b);
                body.field_f64(0x2af, pt.time_beats);
                body.field_f64(0xa6, pt.value);
                body.field_f64(0x2e62, pt.slope as f64);
                body.end_obj();
            }
            body.obj_list_end();
            body.end_obj();
        }
        body.obj_list_end();
    }

    if is_audio {
        let audio = doc.audio.as_ref().unwrap();
        body.field_obj_header(0x49c, 0x6ce);
        body.field_i8(0x1fd, 0);
        body.field_obj_list_start(0x18cb);
        body.begin_obj(0xce);
        body.field_i8(0x1fd, 0);
        body.field_obj_list_start(0x21f);
        body.begin_obj(0xd4);
        body.field_f64(0x2af, 0.0);
        body.field_f64(0x26, doc.duration_beats.max(0.25));
        body.field_bool(0x10f8, false);
        body.field_obj_header(0x288, 0x9f);
        body.field_i8(0x1fd, 0);
        if let Some(ref sp) = audio.sample_path {
            body.field_str(0x286, sp);
        }
        body.field_obj_header(0x192, 0x8b);
        body.field_i8(0x1fd, 0);
        if audio.warp_markers.is_empty() {
            body.field_empty_obj_list(0x21f);
        } else {
            body.field_obj_list_start(0x21f);
            for wm in &audio.warp_markers {
                body.begin_obj(0x43);
                body.field_f64(0x2af, wm.beat_time);
                body.field_f64(0xa6, wm.sample_time_secs);
                body.end_obj();
            }
            body.obj_list_end();
        }
        body.end_obj(); // 0x8b
        body.end_obj(); // 0x9f
        body.field_f64(0xc05, audio.duration_secs.max(0.1));
        body.field_f64(0x2e6d, audio.gain_db);
        body.end_obj(); // 0xd4
        body.obj_list_end();
        body.end_obj(); // 0xce
        body.obj_list_end();
        body.end_obj(); // 0x6ce
    } else {
        body.field_obj_header(0x49c, 0x6cc);
        body.field_i8(0x1fd, 0);
        body.field_obj_list_start(0x18cb);
        let mut by_key: BTreeMap<u8, Vec<&BwClipMidiNote>> = BTreeMap::new();
        for n in &doc.notes {
            by_key.entry(n.key).or_default().push(n);
        }
        for (key, key_notes) in by_key {
            body.begin_obj(0x42);
            body.field_i8(0x1fd, 0);
            body.field_obj_list_start(0x21f);
            for n in key_notes {
                body.begin_obj(0x66);
                body.field_f64(0x2af, n.start_beats);
                body.field_f64(0x26, n.duration_beats.max(0.0625));
                body.field_bool(0x10f8, n.muted);
                body.field_f64(0xef, n.velocity);
                body.field_f64(0xf0, n.release_velocity);
                body.field_f64(0x2dfc, n.chance);
                body.field_f64(0x2e62, n.pitch_offset_semitones);
                body.field_f64(0x2e6a, n.timbre);
                body.field_f64(0x2e6e, n.pressure);
                body.field_f64(0x2e6d, n.gain_db);
                body.end_obj();
            }
            body.obj_list_end();
            body.field_i8(0xee, key);
            body.end_obj();
        }
        body.obj_list_end();
        body.end_obj(); // 0x6cc
    }

    body.end_obj(); // content_cls

    body.field_str(0x286, &doc.name);
    body.field_color(0x280, doc.color.unwrap_or([0.23, 0.62, 0.85, 1.0]));
    body.field_bool(0xbe1, doc.launch.launch_legato);
    body.end_obj(); // launcher_cls

    let body_bytes = body.into_bytes();
    let header_len = 43usize;
    let body_offset = header_len + meta_bytes.len();
    let header = format!("BtWg0003000200b8{body_offset:08x}000000000000000000\n");
    let mut out = Vec::with_capacity(body_offset + body_bytes.len());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(&meta_bytes);
    out.extend_from_slice(&body_bytes);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bcl_01_and_02_roundtrip_midi_clip_with_micropitch_and_expression() {
        let doc = BwClipDocument {
            name: "Acid Lead".to_string(),
            kind: BwClipKind::Midi,
            creator: "Hikaru Test".to_string(),
            category: "MIDI Clip".to_string(),
            tags: vec!["acid".to_string(), "lead".to_string()],
            bpm: 128.0,
            duration_beats: 4.0,
            notes: vec![
                BwClipMidiNote {
                    start_beats: 0.0,
                    duration_beats: 0.5,
                    key: 60,
                    velocity: 0.9,
                    release_velocity: 0.4,
                    muted: false,
                    chance: 0.85,
                    pitch_offset_semitones: 0.25,
                    timbre: 0.75,
                    pressure: 0.6,
                    gain_db: -1.5,
                    pitch_envelope: Vec::new(),
                },
                BwClipMidiNote {
                    start_beats: 1.0,
                    duration_beats: 0.5,
                    key: 64,
                    velocity: 0.75,
                    ..Default::default()
                },
            ],
            launch: BwClipLaunchSettings {
                loop_enabled: true,
                loop_start_beats: 0.5,
                loop_length_beats: 3.5,
                launch_legato: true,
                ..Default::default()
            },
            ..Default::default()
        };

        let bytes = serialize_bwclip_bytes(&doc);
        assert!(bytes.starts_with(b"BtWg00030002"));

        let parsed = parse_bwclip_bytes(&bytes).expect("parse midi .bwclip");
        assert_eq!(parsed.name, "Acid Lead");
        assert_eq!(parsed.kind, BwClipKind::Midi);
        assert_eq!(parsed.notes.len(), 2);
        assert!((parsed.notes[0].pitch_offset_semitones - 0.25).abs() < 1e-6);
        assert!((parsed.notes[0].timbre - 0.75).abs() < 1e-6);
        assert!((parsed.notes[0].pressure - 0.6).abs() < 1e-6);
        assert!((parsed.notes[0].chance - 0.85).abs() < 1e-6);
        assert!(parsed.launch.launch_legato);
        assert!(parsed.launch.loop_enabled);
        assert!((parsed.launch.loop_start_beats - 0.5).abs() < 1e-6);

        let mclip = bwclip_to_matrix_clip(&parsed, 1, 128.0);
        assert!(mclip.launch_legato);
        assert!(mclip.loop_enabled);
        match mclip.content {
            ClipData::Midi { notes } => assert_eq!(notes.len(), 2),
            _ => panic!("expected Midi ClipData"),
        }
    }

    #[test]
    fn bcl_03_and_04_roundtrip_audio_zip_warp_markers_and_automation() {
        // Minimal valid 16-bit mono WAV header + 4 frames
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36u32 + 8).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&44100u32.to_le_bytes());
        wav.extend_from_slice(&(44100u32 * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&8u32.to_le_bytes());
        wav.extend_from_slice(&[0u8; 8]);

        let doc = BwClipDocument {
            name: "Breakbeat Loop".to_string(),
            kind: BwClipKind::Audio,
            duration_beats: 4.0,
            duration_secs: 2.0,
            audio: Some(BwClipAudioData {
                sample_path: Some("samples/break.wav".to_string()),
                embedded_samples: vec![("samples/break.wav".to_string(), wav)],
                sample_rate: 44100,
                channels: 1,
                duration_secs: 2.0,
                gain_db: 0.0,
                stretch_mode: "Elastique".to_string(),
                warp_markers: vec![
                    BwClipWarpMarker {
                        beat_time: 0.0,
                        sample_time_secs: 0.0,
                    },
                    BwClipWarpMarker {
                        beat_time: 4.0,
                        sample_time_secs: 2.0,
                    },
                ],
                onset_times_secs: Vec::new(),
            }),
            automation: vec![BwClipAutomationLane {
                target_param: "Filter Cutoff".to_string(),
                points: vec![
                    BwClipAutomationPoint {
                        time_beats: 0.0,
                        value: 0.2,
                        slope: 0.0,
                    },
                    BwClipAutomationPoint {
                        time_beats: 4.0,
                        value: 0.9,
                        slope: 0.5,
                    },
                ],
            }],
            launch: BwClipLaunchSettings {
                loop_enabled: true,
                loop_start_beats: 0.0,
                loop_length_beats: 4.0,
                launch_legato: false,
                ..Default::default()
            },
            ..Default::default()
        };

        let bytes = serialize_bwclip_bytes(&doc);
        assert!(bytes.starts_with(b"PK\x03\x04"));

        let parsed = parse_bwclip_bytes(&bytes).expect("parse zip .bwclip");
        assert_eq!(parsed.kind, BwClipKind::Audio);
        let audio = parsed.audio.as_ref().expect("audio data");
        assert_eq!(audio.warp_markers.len(), 2);
        assert_eq!(audio.embedded_samples.len(), 1);
        assert_eq!(parsed.automation.len(), 1);
        assert_eq!(parsed.automation[0].target_param, "Filter Cutoff");
        assert_eq!(parsed.automation[0].points.len(), 2);

        let mclip = bwclip_to_matrix_clip(&parsed, 7, 120.0);
        assert_eq!(mclip.local_state.clips.len(), 1);
        assert!(mclip.path.exists());
    }
}
