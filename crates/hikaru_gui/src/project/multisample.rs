// Parser y exportador clean-room para contenedores `.multisample` de Bitwig Studio.
// GNU AGPLv3
// crates/hikaru_gui/src/project/multisample.rs
//
// Implementa el soporte completo para `.multisample`:
// - bms-01: Lector de contenedor ZIP y parser XML (`multisample.xml`) con metadatos.
// - bms-02: Zonas de teclado (keyzones), capas de velocidad (velocity layers) y puntos de loop.
// - bms-03: Decodificación de audio WAV embebido y exportación round-trip a `.multisample`.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;

use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};

use crate::project::dawproject::{build_zip_store, read_zip_entries};

/// Extensión del formato de instrumento multi-sample nativo de Bitwig Studio.
pub const MULTISAMPLE_EXTENSION: &str = "multisample";

/// Devuelve `true` si la ruta corresponde a un archivo `.multisample`.
pub fn is_multisample_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| ext.eq_ignore_ascii_case(MULTISAMPLE_EXTENSION))
        .unwrap_or(false)
}

/// Modo de loop de una zona dentro de un `.multisample`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MultisampleLoopMode {
    #[default]
    Off,
    Forward,
    PingPong,
}

impl MultisampleLoopMode {
    pub fn from_xml(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "forward" | "loop" | "on" | "normal" => Self::Forward,
            "ping-pong" | "ping_pong" | "pingpong" | "alternate" => Self::PingPong,
            _ => Self::Off,
        }
    }

    pub fn as_xml(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Forward => "forward",
            Self::PingPong => "ping-pong",
        }
    }
}

/// Rango de teclas y afinación de una zona (`<key .../>`).
#[derive(Debug, Clone, PartialEq)]
pub struct MultisampleKeyZone {
    pub root: u8,
    pub low: u8,
    pub high: u8,
    pub low_fade: u8,
    pub high_fade: u8,
    pub tune_cents: f32,
    pub track: f32,
}

impl Default for MultisampleKeyZone {
    fn default() -> Self {
        Self {
            root: 60,
            low: 0,
            high: 127,
            low_fade: 0,
            high_fade: 0,
            tune_cents: 0.0,
            track: 1.0,
        }
    }
}

/// Rango de velocidad MIDI de una zona (`<velocity .../>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MultisampleVelocityZone {
    pub low: u8,
    pub high: u8,
    pub low_fade: u8,
    pub high_fade: u8,
}

impl Default for MultisampleVelocityZone {
    fn default() -> Self {
        Self {
            low: 0,
            high: 127,
            low_fade: 0,
            high_fade: 0,
        }
    }
}

/// Configuración de loop en frames de sample (`<loop .../>`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MultisampleLoop {
    pub mode: MultisampleLoopMode,
    pub start_frame: usize,
    pub stop_frame: Option<usize>,
    pub fade: f32,
}

impl Default for MultisampleLoop {
    fn default() -> Self {
        Self {
            mode: MultisampleLoopMode::Off,
            start_frame: 0,
            stop_frame: None,
            fade: 0.0,
        }
    }
}

/// Audio PCM decodificado en memoria para una zona de `.multisample`.
#[derive(Debug, Clone)]
pub struct MultisampleAudio {
    pub samples: Vec<f32>,
    pub channels: usize,
    pub sample_rate: f32,
    pub total_frames: usize,
}

impl MultisampleAudio {
    pub fn new(samples: Vec<f32>, channels: usize, sample_rate: f32) -> Self {
        let ch = channels.max(1);
        let total_frames = samples.len() / ch;
        Self {
            samples,
            channels: ch,
            sample_rate: if sample_rate > 0.0 { sample_rate } else { 44100.0 },
            total_frames,
        }
    }
}

/// Zona individual (`<sample ...>`) dentro de un `.multisample`.
#[derive(Debug, Clone)]
pub struct MultisampleZone {
    pub file: String,
    pub layer: String,
    pub gain_db: f32,
    pub pan: f32,
    pub sample_start: Option<usize>,
    pub sample_stop: Option<usize>,
    pub reverse: bool,
    pub zone_logic: String,
    pub key: MultisampleKeyZone,
    pub velocity: MultisampleVelocityZone,
    pub loop_spec: MultisampleLoop,
    pub audio: Option<MultisampleAudio>,
    /// Bytes crudos originales del archivo dentro del ZIP (para round-trip sin pérdida si aplica).
    pub raw_bytes: Option<Vec<u8>>,
}

impl Default for MultisampleZone {
    fn default() -> Self {
        Self {
            file: String::new(),
            layer: "Default".to_string(),
            gain_db: 0.0,
            pan: 0.0,
            sample_start: None,
            sample_stop: None,
            reverse: false,
            zone_logic: "always-play".to_string(),
            key: MultisampleKeyZone::default(),
            velocity: MultisampleVelocityZone::default(),
            loop_spec: MultisampleLoop::default(),
            audio: None,
            raw_bytes: None,
        }
    }
}

/// Documento completo `.multisample` parseado o listo para exportar.
#[derive(Debug, Clone, Default)]
pub struct MultisampleDocument {
    pub name: String,
    pub generator: String,
    pub category: String,
    pub creator: String,
    pub description: String,
    pub keywords: Vec<String>,
    pub zones: Vec<MultisampleZone>,
}

/// Decodifica bytes WAV en memoria a `MultisampleAudio` usando `hound`.
pub fn decode_wav_bytes(bytes: &[u8]) -> Result<MultisampleAudio, String> {
    let reader = hound::WavReader::new(Cursor::new(bytes))
        .map_err(|e| format!("no se pudo leer WAV embebido: {e}"))?;
    let spec = reader.spec();
    let channels = spec.channels as usize;
    let sample_rate = spec.sample_rate as f32;

    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .filter_map(|s| s.ok())
            .collect(),
        hound::SampleFormat::Int => {
            let max_val = (1i64 << (spec.bits_per_sample.saturating_sub(1).max(1))) as f32;
            reader
                .into_samples::<i32>()
                .filter_map(|s| s.ok())
                .map(|s| s as f32 / max_val)
                .collect()
        }
    };

    if samples.is_empty() {
        return Err("el WAV embebido no contiene muestras".to_string());
    }

    Ok(MultisampleAudio::new(samples, channels, sample_rate))
}

/// Codifica `MultisampleAudio` a bytes WAV float32 mediante `hound`.
pub fn encode_wav_bytes(audio: &MultisampleAudio) -> Result<Vec<u8>, String> {
    let spec = hound::WavSpec {
        channels: audio.channels.max(1) as u16,
        sample_rate: audio.sample_rate.max(1.0).round() as u32,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)
            .map_err(|e| format!("no se pudo crear WavWriter: {e}"))?;
        for &s in &audio.samples {
            writer
                .write_sample(s)
                .map_err(|e| format!("error escribiendo muestra WAV: {e}"))?;
        }
        writer
            .finalize()
            .map_err(|e| format!("error finalizando WAV: {e}"))?;
    }
    Ok(cursor.into_inner())
}

fn find_zip_entry<'a>(entries: &'a HashMap<String, Vec<u8>>, target: &str) -> Option<&'a Vec<u8>> {
    let norm = target.trim().trim_start_matches("./").replace('\\', "/");
    if let Some(exact) = entries.get(&norm) {
        return Some(exact);
    }
    let norm_lower = norm.to_ascii_lowercase();
    for (k, v) in entries {
        let k_norm = k.trim_start_matches("./").replace('\\', "/").to_ascii_lowercase();
        if k_norm == norm_lower {
            return Some(v);
        }
    }
    let file_only = norm_lower.rsplit('/').next().unwrap_or(&norm_lower);
    for (k, v) in entries {
        let k_file = k
            .replace('\\', "/")
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if k_file == file_only {
            return Some(v);
        }
    }
    None
}

fn parse_attr_str(e: &BytesStart<'_>, name: &str) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref().eq_ignore_ascii_case(name.as_bytes()) {
            if let Ok(val) = attr.normalized_value(Default::default()) {
                return Some(val.into_owned());
            }
        }
    }
    None
}

fn parse_attr_u8(e: &BytesStart<'_>, name: &str) -> Option<u8> {
    parse_attr_str(e, name)?.trim().parse::<i32>().ok().map(|v| v.clamp(0, 127) as u8)
}

fn parse_attr_usize(e: &BytesStart<'_>, name: &str) -> Option<usize> {
    parse_attr_str(e, name)?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|v| *v >= 0.0)
        .map(|v| v.round() as usize)
}

fn parse_attr_f32(e: &BytesStart<'_>, name: &str) -> Option<f32> {
    parse_attr_str(e, name)?.trim().parse::<f32>().ok()
}

fn parse_attr_bool(e: &BytesStart<'_>, name: &str) -> Option<bool> {
    let raw = parse_attr_str(e, name)?;
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Some(true),
        "false" | "0" | "no" => Some(false),
        _ => None,
    }
}

/// Parsea el XML `multisample.xml` de Bitwig Studio.
pub fn parse_multisample_xml(xml_bytes: &[u8]) -> Result<MultisampleDocument, String> {
    let xml_str = std::str::from_utf8(xml_bytes)
        .map_err(|e| format!("multisample.xml no es UTF-8 válido: {e}"))?;
    let mut reader = Reader::from_str(xml_str);
    reader.config_mut().trim_text(true);

    let mut doc = MultisampleDocument::default();
    let mut current_tag: Option<String> = None;
    let mut current_layer = String::from("Default");
    let mut current_zone: Option<MultisampleZone> = None;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_ascii_lowercase();
                match tag.as_str() {
                    "multisample" => {
                        if let Some(name) = parse_attr_str(e, "name") {
                            doc.name = name;
                        }
                    }
                    "layer" | "group" => {
                        if let Some(name) = parse_attr_str(e, "name") {
                            current_layer = name;
                        }
                    }
                    "sample" => {
                        current_zone = Some(zone_from_sample_attrs(e, &current_layer));
                    }
                    "key" => {
                        if let Some(z) = current_zone.as_mut() {
                            apply_key_attrs(e, &mut z.key);
                        }
                    }
                    "velocity" => {
                        if let Some(z) = current_zone.as_mut() {
                            apply_velocity_attrs(e, &mut z.velocity);
                        }
                    }
                    "loop" => {
                        if let Some(z) = current_zone.as_mut() {
                            apply_loop_attrs(e, &mut z.loop_spec);
                        }
                    }
                    _ => {
                        current_tag = Some(tag);
                    }
                }
            }
            Ok(Event::Empty(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_ascii_lowercase();
                match tag.as_str() {
                    "sample" => {
                        doc.zones.push(zone_from_sample_attrs(e, &current_layer));
                    }
                    "key" => {
                        if let Some(z) = current_zone.as_mut() {
                            apply_key_attrs(e, &mut z.key);
                        }
                    }
                    "velocity" => {
                        if let Some(z) = current_zone.as_mut() {
                            apply_velocity_attrs(e, &mut z.velocity);
                        }
                    }
                    "loop" => {
                        if let Some(z) = current_zone.as_mut() {
                            apply_loop_attrs(e, &mut z.loop_spec);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                if let Some(tag) = current_tag.as_deref() {
                    let text = std::str::from_utf8(e.as_ref()).unwrap_or("").trim().to_string();
                    if !text.is_empty() {
                        match tag {
                            "generator" => doc.generator = text,
                            "category" => doc.category = text,
                            "creator" => doc.creator = text,
                            "description" => doc.description = text,
                            "keyword" => {
                                if !doc.keywords.contains(&text) {
                                    doc.keywords.push(text);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_ascii_lowercase();
                if tag == "sample" {
                    if let Some(z) = current_zone.take() {
                        doc.zones.push(z);
                    }
                } else if tag == "layer" || tag == "group" {
                    current_layer = String::from("Default");
                }
                current_tag = None;
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("error parseando multisample.xml: {e}")),
            _ => {}
        }
        buf.clear();
    }

    Ok(doc)
}

fn zone_from_sample_attrs(e: &BytesStart<'_>, layer: &str) -> MultisampleZone {
    let mut zone = MultisampleZone::default();
    zone.layer = layer.to_string();
    if let Some(file) = parse_attr_str(e, "file") {
        zone.file = file;
    }
    if let Some(gain) = parse_attr_f32(e, "gain") {
        zone.gain_db = gain;
    }
    if let Some(pan) = parse_attr_f32(e, "pan") {
        zone.pan = pan.clamp(-1.0, 1.0);
    }
    zone.sample_start = parse_attr_usize(e, "sample-start").or_else(|| parse_attr_usize(e, "start"));
    zone.sample_stop = parse_attr_usize(e, "sample-stop").or_else(|| parse_attr_usize(e, "stop"));
    if let Some(rev) = parse_attr_bool(e, "reverse") {
        zone.reverse = rev;
    }
    if let Some(logic) = parse_attr_str(e, "zone-logic") {
        zone.zone_logic = logic;
    }
    zone
}

fn apply_key_attrs(e: &BytesStart<'_>, key: &mut MultisampleKeyZone) {
    if let Some(v) = parse_attr_u8(e, "root") {
        key.root = v;
    }
    if let Some(v) = parse_attr_u8(e, "low") {
        key.low = v;
    }
    if let Some(v) = parse_attr_u8(e, "high") {
        key.high = v;
    }
    if let Some(v) = parse_attr_u8(e, "low-fade") {
        key.low_fade = v;
    }
    if let Some(v) = parse_attr_u8(e, "high-fade") {
        key.high_fade = v;
    }
    if let Some(v) = parse_attr_f32(e, "tune") {
        key.tune_cents = v;
    }
    if let Some(v) = parse_attr_f32(e, "track") {
        key.track = v;
    }
}

fn apply_velocity_attrs(e: &BytesStart<'_>, vel: &mut MultisampleVelocityZone) {
    if let Some(v) = parse_attr_u8(e, "low") {
        vel.low = v;
    }
    if let Some(v) = parse_attr_u8(e, "high") {
        vel.high = v;
    }
    if let Some(v) = parse_attr_u8(e, "low-fade") {
        vel.low_fade = v;
    }
    if let Some(v) = parse_attr_u8(e, "high-fade") {
        vel.high_fade = v;
    }
}

fn apply_loop_attrs(e: &BytesStart<'_>, lp: &mut MultisampleLoop) {
    if let Some(mode) = parse_attr_str(e, "mode") {
        lp.mode = MultisampleLoopMode::from_xml(&mode);
    }
    if let Some(start) = parse_attr_usize(e, "start") {
        lp.start_frame = start;
    }
    if let Some(stop) = parse_attr_usize(e, "stop").or_else(|| parse_attr_usize(e, "end")) {
        lp.stop_frame = Some(stop);
    }
    if let Some(fade) = parse_attr_f32(e, "fade").or_else(|| parse_attr_f32(e, "crossfade")) {
        lp.fade = fade.clamp(0.0, 1.0);
    }
}

/// Parsea un archivo `.multisample` completo (ZIP + `multisample.xml` + audio WAV por zona).
pub fn parse_multisample_bytes(bytes: &[u8]) -> Result<MultisampleDocument, String> {
    let entries = read_zip_entries(bytes)?;
    let xml_bytes = find_zip_entry(&entries, "multisample.xml")
        .ok_or_else(|| "el contenedor .multisample no contiene multisample.xml".to_string())?;
    let mut doc = parse_multisample_xml(xml_bytes)?;

    for zone in &mut doc.zones {
        if zone.file.trim().is_empty() {
            continue;
        }
        if let Some(raw) = find_zip_entry(&entries, &zone.file) {
            zone.raw_bytes = Some(raw.clone());
            if let Ok(audio) = decode_wav_bytes(raw) {
                zone.audio = Some(audio);
            }
        }
    }

    Ok(doc)
}

/// Carga y parsea un archivo `.multisample` desde disco.
pub fn load_multisample_file(path: &Path) -> Result<MultisampleDocument, String> {
    let bytes = std::fs::read(path)
        .map_err(|e| format!("no se pudo leer {}: {e}", path.display()))?;
    let mut doc = parse_multisample_bytes(&bytes)?;
    if doc.name.trim().is_empty() {
        doc.name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Multisample")
            .to_string();
    }
    Ok(doc)
}

/// Serializa un `MultisampleDocument` a XML `multisample.xml`.
pub fn serialize_multisample_xml(doc: &MultisampleDocument) -> Result<Vec<u8>, String> {
    let mut writer = Writer::new_with_indent(Cursor::new(Vec::new()), b' ', 2);
    writer
        .write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(|e| format!("error escribiendo declaración XML: {e}"))?;

    let mut root = BytesStart::new("multisample");
    let inst_name = if doc.name.trim().is_empty() {
        "Hikaru Multisample"
    } else {
        doc.name.as_str()
    };
    root.push_attribute(("name", inst_name));
    writer
        .write_event(Event::Start(root))
        .map_err(|e| format!("error XML: {e}"))?;

    let write_text_elem = |w: &mut Writer<Cursor<Vec<u8>>>, tag: &str, val: &str| -> Result<(), String> {
        if val.trim().is_empty() {
            return Ok(());
        }
        w.write_event(Event::Start(BytesStart::new(tag)))
            .map_err(|e| e.to_string())?;
        w.write_event(Event::Text(BytesText::new(val)))
            .map_err(|e| e.to_string())?;
        w.write_event(Event::End(BytesEnd::new(tag)))
            .map_err(|e| e.to_string())?;
        Ok(())
    };

    let generator = if doc.generator.trim().is_empty() {
        "Hikaru OpenLive"
    } else {
        doc.generator.as_str()
    };
    write_text_elem(&mut writer, "generator", generator)?;
    write_text_elem(&mut writer, "category", &doc.category)?;
    write_text_elem(&mut writer, "creator", &doc.creator)?;
    write_text_elem(&mut writer, "description", &doc.description)?;

    if !doc.keywords.is_empty() {
        writer
            .write_event(Event::Start(BytesStart::new("keywords")))
            .map_err(|e| e.to_string())?;
        for kw in &doc.keywords {
            write_text_elem(&mut writer, "keyword", kw)?;
        }
        writer
            .write_event(Event::End(BytesEnd::new("keywords")))
            .map_err(|e| e.to_string())?;
    }

    let mut layer = BytesStart::new("layer");
    layer.push_attribute(("name", "Default"));
    writer
        .write_event(Event::Start(layer))
        .map_err(|e| e.to_string())?;

    for (idx, zone) in doc.zones.iter().enumerate() {
        let mut s_elem = BytesStart::new("sample");
        let file_ref = if zone.file.trim().is_empty() {
            format!("sample_{idx}.wav")
        } else {
            zone.file.clone()
        };
        s_elem.push_attribute(("file", file_ref.as_str()));
        let gain_s = format!("{:.3}", zone.gain_db);
        s_elem.push_attribute(("gain", gain_s.as_str()));
        if zone.pan.abs() > 0.0001 {
            let pan_s = format!("{:.3}", zone.pan);
            s_elem.push_attribute(("pan", pan_s.as_str()));
        }
        if let Some(st) = zone.sample_start {
            let st_s = st.to_string();
            s_elem.push_attribute(("sample-start", st_s.as_str()));
        }
        if let Some(sp) = zone.sample_stop {
            let sp_s = sp.to_string();
            s_elem.push_attribute(("sample-stop", sp_s.as_str()));
        }
        if zone.reverse {
            s_elem.push_attribute(("reverse", "true"));
        }
        if !zone.zone_logic.is_empty() {
            s_elem.push_attribute(("zone-logic", zone.zone_logic.as_str()));
        }
        writer
            .write_event(Event::Start(s_elem))
            .map_err(|e| e.to_string())?;

        let mut k_elem = BytesStart::new("key");
        let root_s = zone.key.root.to_string();
        let low_s = zone.key.low.to_string();
        let high_s = zone.key.high.to_string();
        let tune_s = format!("{:.2}", zone.key.tune_cents);
        let track_s = format!("{:.3}", zone.key.track);
        k_elem.push_attribute(("root", root_s.as_str()));
        k_elem.push_attribute(("low", low_s.as_str()));
        k_elem.push_attribute(("high", high_s.as_str()));
        k_elem.push_attribute(("tune", tune_s.as_str()));
        k_elem.push_attribute(("track", track_s.as_str()));
        writer
            .write_event(Event::Empty(k_elem))
            .map_err(|e| e.to_string())?;

        let mut v_elem = BytesStart::new("velocity");
        let vlow_s = zone.velocity.low.to_string();
        let vhigh_s = zone.velocity.high.to_string();
        v_elem.push_attribute(("low", vlow_s.as_str()));
        v_elem.push_attribute(("high", vhigh_s.as_str()));
        writer
            .write_event(Event::Empty(v_elem))
            .map_err(|e| e.to_string())?;

        let mut l_elem = BytesStart::new("loop");
        l_elem.push_attribute(("mode", zone.loop_spec.mode.as_xml()));
        let ls_s = zone.loop_spec.start_frame.to_string();
        l_elem.push_attribute(("start", ls_s.as_str()));
        if let Some(stop) = zone.loop_spec.stop_frame {
            let lsp_s = stop.to_string();
            l_elem.push_attribute(("stop", lsp_s.as_str()));
        }
        let lf_s = format!("{:.3}", zone.loop_spec.fade);
        l_elem.push_attribute(("fade", lf_s.as_str()));
        writer
            .write_event(Event::Empty(l_elem))
            .map_err(|e| e.to_string())?;

        writer
            .write_event(Event::End(BytesEnd::new("sample")))
            .map_err(|e| e.to_string())?;
    }

    writer
        .write_event(Event::End(BytesEnd::new("layer")))
        .map_err(|e| e.to_string())?;
    writer
        .write_event(Event::End(BytesEnd::new("multisample")))
        .map_err(|e| e.to_string())?;

    Ok(writer.into_inner().into_inner())
}

/// Empaqueta un `MultisampleDocument` en bytes `.multisample` (ZIP).
pub fn export_multisample_bytes(doc: &MultisampleDocument) -> Result<Vec<u8>, String> {
    let xml = serialize_multisample_xml(doc)?;
    let mut owned_blobs: Vec<(String, Vec<u8>)> = Vec::with_capacity(doc.zones.len() + 1);
    owned_blobs.push(("multisample.xml".to_string(), xml));

    for (idx, zone) in doc.zones.iter().enumerate() {
        let rel_path = if zone.file.trim().is_empty() {
            format!("sample_{idx}.wav")
        } else {
            zone.file.trim_start_matches("./").replace('\\', "/")
        };
        if let Some(raw) = &zone.raw_bytes {
            owned_blobs.push((rel_path, raw.clone()));
        } else if let Some(audio) = &zone.audio {
            let wav = encode_wav_bytes(audio)?;
            owned_blobs.push((rel_path, wav));
        }
    }

    let refs: Vec<(&str, &[u8])> = owned_blobs
        .iter()
        .map(|(name, data)| (name.as_str(), data.as_slice()))
        .collect();
    Ok(build_zip_store(&refs))
}

/// Exporta un `MultisampleDocument` a disco en formato `.multisample`.
pub fn export_multisample_file(doc: &MultisampleDocument, path: &Path) -> Result<(), String> {
    let bytes = export_multisample_bytes(doc)?;
    std::fs::write(path, bytes)
        .map_err(|e| format!("no se pudo escribir {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_sine_wav(freq_hz: f32, frames: usize) -> Vec<u8> {
        let samples: Vec<f32> = (0..frames)
            .map(|i| (2.0 * std::f32::consts::PI * freq_hz * i as f32 / 44100.0).sin() * 0.5)
            .collect();
        let audio = MultisampleAudio::new(samples, 1, 44100.0);
        encode_wav_bytes(&audio).expect("encode wav")
    }

    #[test]
    fn bms01_parses_multisample_zip_and_xml_metadata() {
        let wav = synthetic_sine_wav(261.63, 1024);
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<multisample name="Studio Grand">
  <generator>Bitwig Studio</generator>
  <category>Piano</category>
  <creator>Hikaru QA</creator>
  <description>Clean-room fixture</description>
  <keywords>
    <keyword>acoustic</keyword>
    <keyword>keys</keyword>
  </keywords>
  <layer name="Main">
    <sample file="samples/C3.wav" gain="-1.5">
      <key root="60" low="0" high="127" tune="0.0" track="1.0"/>
      <velocity low="0" high="127"/>
      <loop mode="off" start="0"/>
    </sample>
  </layer>
</multisample>"#;

        let zip = build_zip_store(&[
            ("multisample.xml", xml.as_bytes()),
            ("samples/C3.wav", &wav),
        ]);

        let doc = parse_multisample_bytes(&zip).expect("parse multisample zip");
        assert_eq!(doc.name, "Studio Grand");
        assert_eq!(doc.category, "Piano");
        assert_eq!(doc.creator, "Hikaru QA");
        assert_eq!(doc.keywords, vec!["acoustic", "keys"]);
        assert_eq!(doc.zones.len(), 1);
        assert_eq!(doc.zones[0].audio.as_ref().unwrap().total_frames, 1024);
    }

    #[test]
    fn bms02_parses_keyzones_velocity_layers_and_loop_points() {
        let wav_low = synthetic_sine_wav(130.81, 2048);
        let wav_high = synthetic_sine_wav(523.25, 2048);
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<multisample name="Layered Synth">
  <layer name="Soft">
    <sample file="C2_soft.wav" gain="-3.0" sample-start="16" sample-stop="2000">
      <key root="48" low="36" high="59" tune="-5.0"/>
      <velocity low="0" high="79"/>
      <loop mode="forward" start="256" stop="1800" fade="0.15"/>
    </sample>
  </layer>
  <layer name="Hard">
    <sample file="C4_hard.wav" gain="0.0">
      <key root="72" low="60" high="96" tune="2.5"/>
      <velocity low="80" high="127"/>
      <loop mode="ping-pong" start="100" stop="1900" fade="0.05"/>
    </sample>
  </layer>
</multisample>"#;

        let zip = build_zip_store(&[
            ("multisample.xml", xml.as_bytes()),
            ("C2_soft.wav", &wav_low),
            ("C4_hard.wav", &wav_high),
        ]);
        let doc = parse_multisample_bytes(&zip).expect("parse multi-zone");
        assert_eq!(doc.zones.len(), 2);

        let z0 = &doc.zones[0];
        assert_eq!(z0.key.root, 48);
        assert_eq!((z0.key.low, z0.key.high), (36, 59));
        assert_eq!((z0.velocity.low, z0.velocity.high), (0, 79));
        assert_eq!(z0.loop_spec.mode, MultisampleLoopMode::Forward);
        assert_eq!(z0.loop_spec.start_frame, 256);
        assert_eq!(z0.loop_spec.stop_frame, Some(1800));
        assert!((z0.loop_spec.fade - 0.15).abs() < 0.001);

        let z1 = &doc.zones[1];
        assert_eq!(z1.key.root, 72);
        assert_eq!((z1.velocity.low, z1.velocity.high), (80, 127));
        assert_eq!(z1.loop_spec.mode, MultisampleLoopMode::PingPong);
    }

    #[test]
    fn bms03_roundtrips_multisample_zip_with_wav_audio() {
        let audio = MultisampleAudio::new(vec![0.25, -0.25, 0.5, -0.5], 1, 44100.0);
        let doc = MultisampleDocument {
            name: "Roundtrip Lead".to_string(),
            generator: "Hikaru OpenLive".to_string(),
            category: "Synth".to_string(),
            creator: "Hikaru".to_string(),
            description: "Roundtrip test".to_string(),
            keywords: vec!["lead".to_string()],
            zones: vec![MultisampleZone {
                file: "lead_c3.wav".to_string(),
                gain_db: -2.0,
                key: MultisampleKeyZone {
                    root: 60,
                    low: 48,
                    high: 72,
                    ..Default::default()
                },
                velocity: MultisampleVelocityZone {
                    low: 1,
                    high: 127,
                    ..Default::default()
                },
                loop_spec: MultisampleLoop {
                    mode: MultisampleLoopMode::Forward,
                    start_frame: 1,
                    stop_frame: Some(3),
                    fade: 0.1,
                },
                audio: Some(audio),
                ..Default::default()
            }],
        };

        let bytes = export_multisample_bytes(&doc).expect("export multisample");
        assert_eq!(&bytes[0..4], b"PK\x03\x04");
        let parsed = parse_multisample_bytes(&bytes).expect("re-parse exported multisample");
        assert_eq!(parsed.name, "Roundtrip Lead");
        assert_eq!(parsed.zones.len(), 1);
        assert_eq!(parsed.zones[0].key.root, 60);
        assert_eq!(parsed.zones[0].loop_spec.mode, MultisampleLoopMode::Forward);
        assert_eq!(parsed.zones[0].audio.as_ref().unwrap().total_frames, 4);
    }
}
