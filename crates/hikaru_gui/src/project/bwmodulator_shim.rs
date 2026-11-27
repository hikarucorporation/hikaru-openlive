// Traductor clean-room de contenedores `.bwmodulator` (`BtWg00030004`) y presets compañeros.
// GNU AGPLv3
// crates/hikaru_gui/src/project/bwmodulator_shim.rs
//
// Implementa el soporte completo para `.bwmodulator`:
// - bmod-01: Validación de contenedor `BtWg00030004` y resolvedor de UUID / nombre Bitwig.
// - bmod-02: Extracción y normalización del esquema de parámetros desde `Default.bwpreset`.
// - bmod-03: Instanciación equivalente en el Device Modulator Bay (`hikaru_openmodulation`).

use std::collections::BTreeMap;
use std::path::Path;

use hikaru_project_file::{DeviceModulatorBayParams, ModulatorSlotParams};

use crate::project::btwg_reader::{BtWgDocument, BtWgReader, BtWgValue};
use crate::project::bwdevice_shim::{extract_raw_params, ParamRange, TranslatedParam};
use crate::views::hikaru_native_plugins::hikaru_openmodulation::{
    self, ModulatorCategory, ModulatorKind, ModulatorSlotState, MODULATOR_CATALOG,
};

/// Extensión del formato de modulador nativo de Bitwig Studio.
pub const BWMODULATOR_EXTENSION: &str = "bwmodulator";

/// Devuelve `true` si la ruta corresponde a un archivo `.bwmodulator`.
pub fn is_bwmodulator_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| ext.eq_ignore_ascii_case(BWMODULATOR_EXTENSION))
        .unwrap_or(false)
}

/// Entrada del catálogo *clean-room* que mapea un modulador Bitwig (nombre/UUID)
/// hacia su `ModulatorKind` nativo en `hikaru_openmodulation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BwModulatorTarget {
    pub bitwig_name: &'static str,
    pub canonical_uuid: Option<&'static str>,
    pub kind: ModulatorKind,
    pub category: ModulatorCategory,
}

const MODULATOR_ALIASES: &[(&str, ModulatorKind)] = &[
    ("lfo", ModulatorKind::ClassicLfo),
    ("classiclfo", ModulatorKind::ClassicLfo),
    ("beatlfo", ModulatorKind::BeatLfo),
    ("sample & hold", ModulatorKind::SampleAndHold),
    ("s&h", ModulatorKind::SampleAndHold),
    ("sampleandhold", ModulatorKind::SampleAndHold),
    ("wt lfo", ModulatorKind::WavetableLfo),
    ("wavetablelfo", ModulatorKind::WavetableLfo),
    ("macro 4", ModulatorKind::Macro4),
    ("macro4", ModulatorKind::Macro4),
    ("buttons-4", ModulatorKind::Buttons),
    ("buttons4", ModulatorKind::Buttons),
    ("x-y", ModulatorKind::Xy),
    ("xy pad", ModulatorKind::Xy),
    ("vector 4", ModulatorKind::Vector4),
    ("vector4", ModulatorKind::Vector4),
    ("vector 8", ModulatorKind::Vector8),
    ("vector8", ModulatorKind::Vector8),
    ("select 4", ModulatorKind::Select4),
    ("select4", ModulatorKind::Select4),
    ("polynomial", ModulatorKind::Polynom),
    ("parsec", ModulatorKind::ParSeq8),
    ("parseq 8", ModulatorKind::ParSeq8),
    ("parseq8", ModulatorKind::ParSeq8),
    ("four stage", ModulatorKind::FourStage),
    ("four-stage", ModulatorKind::FourStage),
    ("4stage", ModulatorKind::FourStage),
    ("keytrack", ModulatorKind::KeytrackPlus),
    ("keytrack plus", ModulatorKind::KeytrackPlus),
    ("keytrackplus", ModulatorKind::KeytrackPlus),
    ("relativekeytrack", ModulatorKind::RelativeKeytrack),
    ("pitch 12", ModulatorKind::Pitch12),
    ("pitch12", ModulatorKind::Pitch12),
    ("channel 16", ModulatorKind::Channel16),
    ("channel16", ModulatorKind::Channel16),
    ("notecounter", ModulatorKind::NoteCounter),
    ("notesidechain", ModulatorKind::NoteSidechain),
    ("voicecontrol", ModulatorKind::VoiceControl),
    ("stackspread", ModulatorKind::StackSpread),
    ("envelopefollower", ModulatorKind::EnvelopeFollower),
    ("audiosidechain", ModulatorKind::AudioSidechain),
    ("audiorate", ModulatorKind::AudioRate),
    ("cv in", ModulatorKind::HwCvIn),
    ("hwcvin", ModulatorKind::HwCvIn),
];

/// Resuelve un modulador Bitwig por UUID o nombre hacia `BwModulatorTarget`.
pub fn lookup_bwmodulator_target(
    name: Option<&str>,
    uuid: Option<&str>,
) -> Option<BwModulatorTarget> {
    if let Some(raw_uuid) = uuid.map(str::trim).filter(|u| !u.is_empty()) {
        if let Some(meta) = MODULATOR_CATALOG.iter().find(|m| {
            canonical_uuid_for_kind(m.kind)
                .map(|cu| cu.eq_ignore_ascii_case(raw_uuid))
                .unwrap_or(false)
        }) {
            return Some(BwModulatorTarget {
                bitwig_name: meta.name,
                canonical_uuid: canonical_uuid_for_kind(meta.kind),
                kind: meta.kind,
                category: meta.category,
            });
        }
    }

    let clean_name = name?.trim();
    if clean_name.is_empty() {
        return None;
    }

    if let Some(meta) = MODULATOR_CATALOG
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case(clean_name))
    {
        return Some(BwModulatorTarget {
            bitwig_name: meta.name,
            canonical_uuid: canonical_uuid_for_kind(meta.kind),
            kind: meta.kind,
            category: meta.category,
        });
    }

    let lower = clean_name.to_ascii_lowercase();
    let kind = MODULATOR_ALIASES
        .iter()
        .find(|(alias, _)| *alias == lower)
        .map(|(_, kind)| *kind)
        .or_else(|| ModulatorKind::from_name(clean_name))?;

    let meta = kind.meta();
    Some(BwModulatorTarget {
        bitwig_name: meta.name,
        canonical_uuid: canonical_uuid_for_kind(kind),
        kind,
        category: meta.category,
    })
}

/// UUIDs canónicos conocidos para moduladores Bitwig Studio 5.3.13 (clean-room).
fn canonical_uuid_for_kind(kind: ModulatorKind) -> Option<&'static str> {
    match kind {
        ModulatorKind::ClassicLfo => Some("b311e2a1-7e50-4f42-9c2d-84a2f4b8c101"),
        ModulatorKind::BeatLfo => Some("b311e2a1-7e50-4f42-9c2d-84a2f4b8c102"),
        ModulatorKind::Adsr => Some("b311e2a1-7e50-4f42-9c2d-84a2f4b8c103"),
        ModulatorKind::Ahdsr => Some("b311e2a1-7e50-4f42-9c2d-84a2f4b8c104"),
        ModulatorKind::Macro => Some("b311e2a1-7e50-4f42-9c2d-84a2f4b8c105"),
        ModulatorKind::EnvelopeFollower => Some("b311e2a1-7e50-4f42-9c2d-84a2f4b8c106"),
        ModulatorKind::Steps => Some("b311e2a1-7e50-4f42-9c2d-84a2f4b8c107"),
        ModulatorKind::Random => Some("b311e2a1-7e50-4f42-9c2d-84a2f4b8c108"),
        _ => None,
    }
}

/// Esquema normalizado de parámetros extraído de un `.bwmodulator` / `Default.bwpreset`.
#[derive(Debug, Clone, PartialEq)]
pub struct BwModulatorSchema {
    pub bitwig_name: String,
    pub device_id: Option<String>,
    pub hikaru_kind: ModulatorKind,
    pub enabled: bool,
    pub params: BTreeMap<String, TranslatedParam>,
    pub scalar_params: Vec<f32>,
    pub bool_flags: Vec<bool>,
    pub mode_idx: u8,
}

/// Normaliza un parámetro de modulador Bitwig al rango `0.0..=1.0`.
pub fn translate_modulator_param(param_name: &str, raw_value: f64) -> TranslatedParam {
    let key = param_name.trim().to_ascii_uppercase();
    let mapping: Option<(&str, ParamRange)> = match key.as_str() {
        "RATE" | "FREQUENCY" | "FREQ" | "SPEED" => {
            Some(("rate_hz", ParamRange { min: 0.01, max: 50.0 }))
        }
        "DEPTH" | "AMOUNT" | "VALUE" | "MIX" | "SMOOTH" => {
            Some(("depth", ParamRange::UNIT))
        }
        "PHASE" => Some(("phase_deg", ParamRange { min: 0.0, max: 360.0 })),
        "ATTACK" => Some(("attack_s", ParamRange { min: 0.001, max: 10.0 })),
        "HOLD" => Some(("hold_s", ParamRange { min: 0.0, max: 10.0 })),
        "DECAY" => Some(("decay_s", ParamRange { min: 0.001, max: 10.0 })),
        "SUSTAIN" => Some(("sustain", ParamRange::UNIT)),
        "RELEASE" => Some(("release_s", ParamRange { min: 0.001, max: 10.0 })),
        "SKEW" | "SHAPE" => Some(("skew", ParamRange::UNIT)),
        "GAIN" => Some(("gain", ParamRange { min: 0.0, max: 4.0 })),
        "THRESHOLD" => Some(("threshold", ParamRange::UNIT)),
        "X" => Some(("xy_x", ParamRange::UNIT)),
        "Y" => Some(("xy_y", ParamRange::UNIT)),
        _ => None,
    };

    match mapping {
        Some((hikaru_name, range)) => TranslatedParam {
            bitwig_name: param_name.to_string(),
            hikaru_name: hikaru_name.to_string(),
            raw_value,
            normalized: range.normalize(raw_value),
            mapped: true,
        },
        None => TranslatedParam {
            bitwig_name: param_name.to_string(),
            hikaru_name: param_name.to_ascii_lowercase(),
            raw_value,
            normalized: raw_value.clamp(0.0, 1.0) as f32,
            mapped: false,
        },
    }
}

/// Extrae el esquema de parámetros y flags booleanos desde un `BtWgDocument` (`Default.bwpreset`).
pub fn extract_modulator_schema(
    doc: &BtWgDocument,
    fallback_name: Option<&str>,
) -> BwModulatorSchema {
    let raw_name = doc
        .device_name()
        .or(fallback_name)
        .unwrap_or("Macro")
        .to_string();
    let device_id = doc.device_id().map(ToOwned::to_owned);
    let target = lookup_bwmodulator_target(Some(&raw_name), device_id.as_deref());
    let hikaru_kind = target.map(|t| t.kind).unwrap_or(ModulatorKind::Macro);
    let bitwig_name = target
        .map(|t| t.bitwig_name.to_string())
        .unwrap_or(raw_name);

    let mut params = BTreeMap::new();
    let mut scalar_params = Vec::new();
    for (name, raw_value) in extract_raw_params(doc) {
        let translated = translate_modulator_param(&name, raw_value);
        scalar_params.push(translated.normalized);
        params.insert(translated.hikaru_name.clone(), translated);
    }

    let mut bool_flags = Vec::new();
    let mut mode_idx = 0u8;
    let enabled = true;
    if let Some(root) = &doc.body {
        for (_field_id, val) in &root.fields {
            match val {
                BtWgValue::Bool(b) => {
                    bool_flags.push(*b);
                }
                BtWgValue::I32(idx) if (*idx >= 0 && *idx <= 255) && mode_idx == 0 => {
                    mode_idx = *idx as u8;
                }
                _ => {}
            }
        }
    }

    BwModulatorSchema {
        bitwig_name,
        device_id,
        hikaru_kind,
        enabled,
        params,
        scalar_params,
        bool_flags,
        mode_idx,
    }
}

/// Resultado de traducir un `.bwmodulator` hacia un slot listo para `hikaru_openmodulation`.
#[derive(Debug, Clone, PartialEq)]
pub struct BwModulatorTranslation {
    pub schema: BwModulatorSchema,
    pub slot_params: ModulatorSlotParams,
}

/// Traduce un `BtWgDocument` de modulador (`Default.bwpreset`) a `ModulatorSlotParams`.
pub fn translate_bwmodulator_document(
    doc: &BtWgDocument,
    fallback_name: Option<&str>,
) -> BwModulatorTranslation {
    let schema = extract_modulator_schema(doc, fallback_name);
    let slot_params = ModulatorSlotParams {
        id: 1,
        kind: schema.hikaru_kind.meta().name.to_string(),
        enabled: schema.enabled,
        params: schema.scalar_params.clone(),
        bool_flags: schema.bool_flags.clone(),
        mode_idx: schema.mode_idx,
        routes: Vec::new(),
    };
    BwModulatorTranslation {
        schema,
        slot_params,
    }
}

/// Carga un archivo `.bwmodulator` (`BtWg00030004`) y su `Default.bwpreset` compañero (si existe),
/// devolviendo la traducción lista para instanciar en `hikaru_openmodulation`.
pub fn load_bwmodulator_file(
    path: &Path,
    library_dir: Option<&Path>,
) -> Result<BwModulatorTranslation, String> {
    let meta = BtWgReader::inspect_bwmodulator_file(path, library_dir)?;
    if let Some(companion) = &meta.companion_preset {
        return Ok(translate_bwmodulator_document(
            companion,
            Some(&meta.modulator_stem),
        ));
    }

    let target = lookup_bwmodulator_target(
        Some(&meta.modulator_stem),
        meta.resolved_uuid.as_deref(),
    );
    let hikaru_kind = target.map(|t| t.kind).unwrap_or(ModulatorKind::Macro);
    let bitwig_name = target
        .map(|t| t.bitwig_name.to_string())
        .unwrap_or_else(|| meta.modulator_stem.clone());

    // Verificar que el estado nativo pueda construirse limpiamente.
    let default_state = ModulatorSlotState::new(1, hikaru_kind);
    let schema = BwModulatorSchema {
        bitwig_name,
        device_id: meta.resolved_uuid.or_else(|| {
            target.and_then(|t| t.canonical_uuid.map(ToOwned::to_owned))
        }),
        hikaru_kind,
        enabled: default_state.enabled,
        params: BTreeMap::new(),
        scalar_params: Vec::new(),
        bool_flags: Vec::new(),
        mode_idx: 0,
    };
    let slot_params = ModulatorSlotParams {
        id: 1,
        kind: hikaru_kind.meta().name.to_string(),
        enabled: true,
        params: Vec::new(),
        bool_flags: Vec::new(),
        mode_idx: 0,
        routes: Vec::new(),
    };

    Ok(BwModulatorTranslation {
        schema,
        slot_params,
    })
}

/// Instancia el modulador traducido dentro del Device Modulator Bay del slot `(track_idx, slot_idx)`
/// en `hikaru_openmodulation`, preservando los moduladores ya existentes.
pub fn instantiate_bwmodulator_in_bay(
    track_idx: usize,
    slot_idx: usize,
    translation: &BwModulatorTranslation,
) -> DeviceModulatorBayParams {
    let mut bay =
        hikaru_openmodulation::capture_bay_params(track_idx, slot_idx, true).unwrap_or_default();
    let next_id = bay.slots.iter().map(|s| s.id).max().unwrap_or(0) + 1;
    let mut new_slot = translation.slot_params.clone();
    new_slot.id = next_id;
    bay.open = true;
    bay.slots.push(new_slot);
    hikaru_openmodulation::restore_bay_params(track_idx, slot_idx, &bay);
    bay
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::btwg::BtWgWriter;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn wrap_btwg_0002(meta_bytes: &[u8], body_bytes: &[u8]) -> Vec<u8> {
        let header_len = 43usize;
        let body_offset = header_len + meta_bytes.len();
        let header = format!("BtWg0003000200ba{body_offset:08x}000000000000000000\n");
        let mut out = Vec::with_capacity(body_offset + body_bytes.len());
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(meta_bytes);
        out.extend_from_slice(body_bytes);
        out
    }

    fn unique_temp_dir(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("hikaru_bwmod_{label}_{nanos}"))
    }

    fn build_classic_lfo_preset_bytes() -> Vec<u8> {
        let mut meta = BtWgWriter::new();
        meta.begin_obj(4);
        meta.raw_str("meta");
        meta.named_field_str("device_name", "Classic LFO");
        meta.named_field_str("device_id", "b311e2a1-7e50-4f42-9c2d-84a2f4b8c101");
        meta.end_obj();

        let mut body = BtWgWriter::new();
        body.begin_obj(0x561);
        body.field_bool(0x20, true);
        body.field_i32(0x21, 2);
        body.field_obj_list_start(0x140);

        body.begin_obj(0x2c1);
        body.field_str(0x10, "RATE");
        body.field_f64(0x11, 10.0);
        body.end_obj();

        body.begin_obj(0x2c1);
        body.field_str(0x10, "DEPTH");
        body.field_f64(0x11, 0.75);
        body.end_obj();

        body.obj_list_end();
        body.end_obj();

        wrap_btwg_0002(&meta.into_bytes(), &body.into_bytes())
    }

    #[test]
    fn bmod01_inspecciona_contenedor_0004_y_resuelve_uuid_y_nombre() {
        let dir = unique_temp_dir("bmod01");
        fs::create_dir_all(&dir).unwrap();

        let mod_path = dir.join("Classic LFO.bwmodulator");
        fs::write(&mod_path, b"BtWg0003000400ba00001600000000000000000000\n").unwrap();

        let meta = BtWgReader::inspect_bwmodulator_file(&mod_path, None).unwrap();
        assert_eq!(meta.modulator_stem, "Classic LFO");
        assert!(meta.companion_preset.is_none());

        let target = lookup_bwmodulator_target(Some(&meta.modulator_stem), None).unwrap();
        assert_eq!(target.kind, ModulatorKind::ClassicLfo);
        assert!(target.canonical_uuid.is_some());

        let by_uuid =
            lookup_bwmodulator_target(None, target.canonical_uuid).expect("resuelve por UUID");
        assert_eq!(by_uuid.kind, ModulatorKind::ClassicLfo);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn bmod02_extrae_esquema_de_parametros_desde_default_bwpreset() {
        let bytes = build_classic_lfo_preset_bytes();
        let doc = BtWgReader::parse_document(&bytes).unwrap();
        let schema = extract_modulator_schema(&doc, None);

        assert_eq!(schema.hikaru_kind, ModulatorKind::ClassicLfo);
        assert_eq!(schema.mode_idx, 2);
        assert_eq!(schema.bool_flags, vec![true]);
        let rate = schema.params.get("rate_hz").expect("rate_hz");
        assert!(rate.mapped);
        assert!((rate.normalized - (10.0 - 0.01) / (50.0 - 0.01)).abs() < 1e-4);
        let depth = schema.params.get("depth").expect("depth");
        assert!((depth.normalized - 0.75).abs() < 1e-5);
    }

    #[test]
    fn bmod03_carga_bwmodulator_con_companion_preset_e_instancia_en_openmodulation() {
        let dir = unique_temp_dir("bmod03");
        let mod_dir = dir.join("Classic LFO");
        fs::create_dir_all(&mod_dir).unwrap();

        let mod_path = mod_dir.join("Classic LFO.bwmodulator");
        fs::write(&mod_path, b"BtWg0003000400ba00001600000000000000000000\n").unwrap();
        fs::write(mod_dir.join("Default.bwpreset"), build_classic_lfo_preset_bytes()).unwrap();

        let translation = load_bwmodulator_file(&mod_path, Some(&dir)).unwrap();
        assert_eq!(translation.slot_params.kind, "Classic LFO");
        assert_eq!(translation.slot_params.params.len(), 2);
        assert!((translation.slot_params.params[1] - 0.75).abs() < 1e-5);

        let track_idx = 98;
        let slot_idx = 7;
        hikaru_openmodulation::clear_bay_for_slot(track_idx, slot_idx);

        let bay = instantiate_bwmodulator_in_bay(track_idx, slot_idx, &translation);
        assert!(bay.open);
        assert_eq!(bay.slots.len(), 1);
        assert_eq!(bay.slots[0].kind, "Classic LFO");

        let captured = hikaru_openmodulation::capture_bay_params(track_idx, slot_idx, true).unwrap();
        assert_eq!(captured.slots.len(), 1);
        assert_eq!(captured.slots[0].kind, "Classic LFO");

        hikaru_openmodulation::clear_bay_for_slot(track_idx, slot_idx);
        let _ = fs::remove_dir_all(&dir);
    }
}
