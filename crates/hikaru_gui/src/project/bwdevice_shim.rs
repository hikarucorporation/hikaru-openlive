// Traductor clean-room de parámetros `CONTENTS` y `Default.bwremotecontrols` para `.bwdevice`.
// GNU AGPLv3
// crates/hikaru_gui/src/project/bwdevice_shim.rs
//
// Implementa la capa `bd-02` del diseño `BwDeviceShim` (tipo Wine/Proton):
// - Registro `BwCompatibilityRegistry` que resuelve `(device_name, UUID)` hacia
//   los dispositivos nativos de `hikaru_dsp` / `dsp_rack.rs` o hacia un
//   `PassthroughPlaceholderShim` cuando aún no existe equivalente nativo.
// - Extractor y normalizador de parámetros `CONTENTS` desde el AST de `btwg_reader`.
// - Lector de páginas de macros / controles remotos (`Default.bwremotecontrols`,
//   8 slots por página) vinculándolos a los parámetros del dispositivo.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use hikaru_project_file::{
    DeviceModulatorBayParams, ModulatorRouteParams, ModulatorSlotParams, OpenGridCableData,
    OpenGridNodeData, OpenGridParams,
};

use crate::project::btwg::BtWgWriter;
use crate::project::btwg_reader::{
    BtWgContainerKind, BtWgDocument, BtWgHeader, BtWgObject, BtWgReader, BtWgValue,
};
use crate::views::hikaru_native_plugins::hikaru_openmodulation;

/// Límite de profundidad al recorrer el AST para extraer parámetros.
const MAX_PARAM_SCAN_DEPTH: usize = 32;

/// Rango de normalización para convertir un valor crudo de Bitwig a `0.0..=1.0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamRange {
    pub min: f64,
    pub max: f64,
}

impl ParamRange {
    pub const UNIT: Self = Self { min: 0.0, max: 1.0 };

    pub fn normalize(self, raw: f64) -> f32 {
        if !raw.is_finite() || (self.max - self.min).abs() < f64::EPSILON {
            return 0.0;
        }
        ((raw - self.min) / (self.max - self.min)).clamp(0.0, 1.0) as f32
    }
}

/// Entrada del registro *clean-room* de compatibilidad de dispositivos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BwCompatTarget {
    pub bitwig_name: &'static str,
    pub canonical_uuid: Option<&'static str>,
    pub hikaru_slot_name: &'static str,
    pub category: &'static str,
}

/// Tabla de dispositivos nativos de Hikaru OpenLive disponibles como "Builtin DLLs".
pub fn compatibility_targets() -> &'static [BwCompatTarget] {
    &[
        BwCompatTarget {
            bitwig_name: "Bit-8",
            canonical_uuid: None,
            hikaru_slot_name: "Hikaru Open8-Bit",
            category: "Audio FX",
        },
        BwCompatTarget {
            bitwig_name: "DC Offset",
            canonical_uuid: None,
            hikaru_slot_name: "DC Offset",
            category: "Utility",
        },
        BwCompatTarget {
            bitwig_name: "Tool",
            canonical_uuid: None,
            hikaru_slot_name: "Tool",
            category: "Utility",
        },
        BwCompatTarget {
            bitwig_name: "Dual Pan",
            canonical_uuid: None,
            hikaru_slot_name: "Dual Pan",
            category: "Utility",
        },
        BwCompatTarget {
            bitwig_name: "Test Tone",
            canonical_uuid: None,
            hikaru_slot_name: "Test Tone",
            category: "Utility",
        },
        BwCompatTarget {
            bitwig_name: "Time Shift",
            canonical_uuid: None,
            hikaru_slot_name: "Time Shift",
            category: "Utility",
        },
        BwCompatTarget {
            bitwig_name: "Tremolo",
            canonical_uuid: None,
            hikaru_slot_name: "Tremolo",
            category: "Audio FX",
        },
        BwCompatTarget {
            bitwig_name: "Ring-Mod",
            canonical_uuid: None,
            hikaru_slot_name: "Ring-Mod",
            category: "Audio FX",
        },
        BwCompatTarget {
            bitwig_name: "Phaser+",
            canonical_uuid: None,
            hikaru_slot_name: "Hikaru OpenPhaser+",
            category: "Audio FX",
        },
        BwCompatTarget {
            bitwig_name: "Freq Shifter+",
            canonical_uuid: None,
            hikaru_slot_name: "Hikaru OpenFreqShifter+",
            category: "Audio FX",
        },
        BwCompatTarget {
            bitwig_name: "Flanger+",
            canonical_uuid: None,
            hikaru_slot_name: "Hikaru OpenFlanger+",
            category: "Audio FX",
        },
        BwCompatTarget {
            bitwig_name: "Comb",
            canonical_uuid: Some("20e18780-8438-48d3-b234-40dcbaa947b8"),
            hikaru_slot_name: "Hikaru OpenComb",
            category: "Audio FX",
        },
        BwCompatTarget {
            bitwig_name: "EQ-2",
            canonical_uuid: Some("01af068e-1e49-4777-a6e6-7f1dc679227a"),
            hikaru_slot_name: "OpenEQ3",
            category: "EQ",
        },
        BwCompatTarget {
            bitwig_name: "EQ-5",
            canonical_uuid: None,
            hikaru_slot_name: "OpenEQ3",
            category: "EQ",
        },
        BwCompatTarget {
            bitwig_name: "Polymer",
            canonical_uuid: None,
            hikaru_slot_name: "OpenPoly",
            category: "Synth",
        },
        BwCompatTarget {
            bitwig_name: "Polysynth",
            canonical_uuid: None,
            hikaru_slot_name: "OpenPoly",
            category: "Synth",
        },
        BwCompatTarget {
            bitwig_name: "Sampler",
            canonical_uuid: None,
            hikaru_slot_name: "Sampler",
            category: "Sampler",
        },
        BwCompatTarget {
            bitwig_name: "Drum Machine",
            canonical_uuid: None,
            hikaru_slot_name: "Hikaru OpenDMS",
            category: "Drums",
        },
        BwCompatTarget {
            bitwig_name: "Poly Grid",
            canonical_uuid: None,
            hikaru_slot_name: "OpenGrid",
            category: "The Grid",
        },
        BwCompatTarget {
            bitwig_name: "FX Grid",
            canonical_uuid: None,
            hikaru_slot_name: "OpenGrid",
            category: "The Grid",
        },
        BwCompatTarget {
            bitwig_name: "Note Grid",
            canonical_uuid: Some("264d6f4e-5067-46c9-a4fa-a75a295d9e01"),
            hikaru_slot_name: "OpenGrid",
            category: "The Grid",
        },
    ]
}

/// Parámetro extraído y normalizado a `0.0..=1.0`.
#[derive(Debug, Clone, PartialEq)]
pub struct TranslatedParam {
    pub bitwig_name: String,
    pub hikaru_name: String,
    pub raw_value: f64,
    pub normalized: f32,
    pub mapped: bool,
}

/// Asignación de un control remoto / macro de `.bwremotecontrols` (8 slots por página).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroMapping {
    pub page_name: String,
    pub slot_index: usize,
    pub param_name: String,
}

/// Slot individual (0..8) dentro de una página de `.bwremotecontrols`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteControlSlotSpec {
    pub slot_index: usize,
    pub param_name: String,
    pub param_id: Option<String>,
    pub midi_cc: Option<u8>,
}

/// Página nombrada de 8 controles remotos dentro de un `.bwremotecontrols`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteControlPageSpec {
    pub name: String,
    pub tags: Vec<String>,
    pub slots: Vec<RemoteControlSlotSpec>,
}

/// Documento completo `.bwremotecontrols` (`BtWg00030002`) parseado o listo para exportar.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BwRemoteControlsSpec {
    pub device_name: Option<String>,
    pub device_id: Option<String>,
    pub creator: Option<String>,
    pub pages: Vec<RemoteControlPageSpec>,
}

impl BwRemoteControlsSpec {
    /// Aplana las páginas a `Vec<MacroMapping>` para compatibilidad con `BwDeviceTranslation`.
    pub fn to_macro_mappings(&self) -> Vec<MacroMapping> {
        let mut out = Vec::new();
        for page in &self.pages {
            for slot in &page.slots {
                out.push(MacroMapping {
                    page_name: page.name.clone(),
                    slot_index: slot.slot_index,
                    param_name: slot.param_name.clone(),
                });
            }
        }
        out
    }
}

/// Placeholder *lossless* para dispositivos Bitwig que aún no tienen equivalente nativo.
#[derive(Debug, Clone)]
pub struct PassthroughPlaceholderShim {
    pub bitwig_name: String,
    pub device_id: Option<String>,
    pub preserved_ast: BtWgDocument,
    pub read_only_params: Vec<TranslatedParam>,
    pub macro_mappings: Vec<MacroMapping>,
}

/// Resultado de traducir un `.bwpreset` / `Default.bwpreset` hacia el rack de Hikaru.
#[derive(Debug, Clone)]
pub enum BwDeviceTranslation {
    /// Mapeado a un dispositivo nativo de `hikaru_dsp` / `dsp_rack.rs`.
    Builtin {
        bitwig_name: String,
        hikaru_slot_name: String,
        device_id: Option<String>,
        params: BTreeMap<String, TranslatedParam>,
        macro_mappings: Vec<MacroMapping>,
    },
    /// Dispositivo aún sin equivalente nativo: se preserva en bypass limpio.
    Placeholder(PassthroughPlaceholderShim),
}

/// Traduce un documento `BtWgDocument` (p. ej. `.bwpreset` o `Default.bwpreset`) y
/// opcionalmente su `.bwremotecontrols` asociado al espacio de parámetros `0.0..=1.0`.
pub fn translate_device_document(
    doc: &BtWgDocument,
    remote_controls: Option<&BtWgDocument>,
) -> BwDeviceTranslation {
    let bitwig_name = doc
        .device_name()
        .unwrap_or("Unknown Bitwig Device")
        .to_string();
    let device_id = doc.device_id().map(ToOwned::to_owned);

    let target = lookup_compat_target(Some(&bitwig_name), device_id.as_deref());
    let raw_params = extract_raw_params(doc);
    let macro_mappings = remote_controls
        .map(extract_remote_controls)
        .unwrap_or_default();

    match target {
        Some(compat) => {
            let mut params = BTreeMap::new();
            for (name, raw) in raw_params {
                let translated = translate_param_for_device(compat.bitwig_name, &name, raw);
                params.insert(translated.hikaru_name.clone(), translated);
            }
            BwDeviceTranslation::Builtin {
                bitwig_name,
                hikaru_slot_name: compat.hikaru_slot_name.to_string(),
                device_id,
                params,
                macro_mappings,
            }
        }
        None => {
            let read_only_params = raw_params
                .into_iter()
                .map(|(name, raw)| TranslatedParam {
                    hikaru_name: name.clone(),
                    bitwig_name: name,
                    raw_value: raw,
                    normalized: ParamRange::UNIT.normalize(raw),
                    mapped: false,
                })
                .collect();

            BwDeviceTranslation::Placeholder(PassthroughPlaceholderShim {
                bitwig_name,
                device_id,
                preserved_ast: doc.clone(),
                read_only_params,
                macro_mappings,
            })
        }
    }
}

/// Busca el dispositivo en `compatibility_targets()` por UUID o por nombre.
pub fn lookup_compat_target(
    name: Option<&str>,
    uuid: Option<&str>,
) -> Option<&'static BwCompatTarget> {
    let targets = compatibility_targets();
    if let Some(id) = uuid {
        if let Some(found) = targets.iter().find(|t| {
            t.canonical_uuid
                .map(|cu| cu.eq_ignore_ascii_case(id))
                .unwrap_or(false)
        }) {
            return Some(found);
        }
    }
    let clean_name = name?.trim();
    targets
        .iter()
        .find(|t| t.bitwig_name.eq_ignore_ascii_case(clean_name))
}

/// Traduce y normaliza un parámetro individual `(device_name, param_name, raw_value)`
/// al rango `0.0..=1.0` de `PluginInstance`.
pub fn translate_param_for_device(
    bitwig_device: &str,
    param_name: &str,
    raw_value: f64,
) -> TranslatedParam {
    let dev_key = bitwig_device.trim().to_ascii_lowercase();
    let param_key = param_name.trim().to_ascii_uppercase();

    let mapping: Option<(&str, ParamRange)> = match (dev_key.as_str(), param_key.as_str()) {
        ("tool", "GAIN" | "VOLUME") => Some(("gain", ParamRange { min: -36.0, max: 36.0 })),
        ("tool", "WIDTH") => Some(("width", ParamRange { min: 0.0, max: 200.0 })),
        ("tool", "PAN") => Some(("pan", ParamRange { min: -100.0, max: 100.0 })),
        ("tool", "PHASE_L" | "INVERT_L") => Some(("invert_left", ParamRange::UNIT)),
        ("tool", "PHASE_R" | "INVERT_R") => Some(("invert_right", ParamRange::UNIT)),

        ("bit-8", "DRIVE") => Some(("drive", ParamRange::UNIT)),
        ("bit-8", "CRUSH" | "BITS" | "BIT_DEPTH") => {
            Some(("bit_depth", ParamRange { min: 1.0, max: 24.0 }))
        }
        ("bit-8", "DOWNSAMPLE" | "RATE" | "SAMPLE_RATE") => {
            Some(("sample_rate_div", ParamRange { min: 1.0, max: 64.0 }))
        }
        ("bit-8", "MIX") => Some(("mix", ParamRange::UNIT)),

        ("comb", "FREQUENCY" | "PITCH") => Some(("frequency", ParamRange { min: 20.0, max: 20000.0 })),
        ("comb", "FEEDBACK") => Some(("feedback", ParamRange { min: -100.0, max: 100.0 })),
        ("comb", "POLARITY") => Some(("polarity", ParamRange::UNIT)),

        ("eq-2" | "eq-5", "LO_GAIN") => Some(("low_gain", ParamRange { min: -24.0, max: 24.0 })),
        ("eq-2" | "eq-5", "LO_FREQ") => Some(("low_freq", ParamRange { min: 20.0, max: 2000.0 })),
        ("eq-2" | "eq-5", "HI_GAIN") => Some(("high_gain", ParamRange { min: -24.0, max: 24.0 })),
        ("eq-2" | "eq-5", "HI_FREQ") => Some(("high_freq", ParamRange { min: 500.0, max: 20000.0 })),

        ("tremolo", "RATE") => Some(("rate", ParamRange { min: 0.05, max: 40.0 })),
        ("tremolo", "DEPTH") => Some(("depth", ParamRange::UNIT)),
        ("tremolo", "SHAPE") => Some(("shape", ParamRange::UNIT)),

        ("dual pan", "PAN_L") => Some(("pan_left", ParamRange { min: -100.0, max: 100.0 })),
        ("dual pan", "PAN_R") => Some(("pan_right", ParamRange { min: -100.0, max: 100.0 })),

        ("polymer" | "polysynth", "CUTOFF" | "FILTER_CUTOFF" | "FREQUENCY" | "FREQ") => {
            Some(("cutoff", ParamRange { min: 20.0, max: 20000.0 }))
        }
        ("polymer" | "polysynth", "RESONANCE" | "RESO") => Some(("resonance", ParamRange::UNIT)),
        ("polymer" | "polysynth", "DRIVE") => Some(("drive", ParamRange::UNIT)),
        ("polymer" | "polysynth", "OUTPUT" | "GAIN" | "VOLUME") => {
            Some(("output_gain", ParamRange { min: -60.0, max: 6.0 }))
        }
        ("polymer" | "polysynth", "WIDTH") => Some(("width", ParamRange { min: 0.0, max: 200.0 })),
        ("polymer" | "polysynth", "SUB_LEVEL" | "SUB") => Some(("sub_level", ParamRange::UNIT)),
        ("polymer" | "polysynth", "NOISE_LEVEL" | "NOISE") => {
            Some(("noise_level", ParamRange::UNIT))
        }
        ("polymer" | "polysynth", "ATTACK") => Some(("attack", ParamRange::UNIT)),
        ("polymer" | "polysynth", "DECAY") => Some(("decay", ParamRange::UNIT)),
        ("polymer" | "polysynth", "SUSTAIN") => Some(("sustain", ParamRange::UNIT)),
        ("polymer" | "polysynth", "RELEASE") => Some(("release", ParamRange::UNIT)),

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
            normalized: ParamRange::UNIT.normalize(raw_value),
            mapped: false,
        },
    }
}

/// Extrae pares `(nombre_parametro, valor_numerico)` desde el árbol `body` del `BtWgDocument`.
pub fn extract_raw_params(doc: &BtWgDocument) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    if let Some(root) = &doc.body {
        scan_object_params(root, &mut out, 0);
    }
    out
}

fn scan_object_params(obj: &BtWgObject, out: &mut Vec<(String, f64)>, depth: usize) {
    if depth > MAX_PARAM_SCAN_DEPTH || obj.cls == 0x0500 {
        return;
    }

    let mut first_str: Option<&str> = None;
    let mut first_num: Option<f64> = None;

    for (_, val) in &obj.fields {
        match val {
            BtWgValue::Str(s) if !s.is_empty() && first_str.is_none() => {
                first_str = Some(s.as_str());
            }
            BtWgValue::F64(v) if v.is_finite() && first_num.is_none() => {
                first_num = Some(*v);
            }
            BtWgValue::F32(v) if v.is_finite() && first_num.is_none() => {
                first_num = Some(*v as f64);
            }
            BtWgValue::I32(v) if first_num.is_none() => {
                first_num = Some(*v as f64);
            }
            BtWgValue::Bool(v) if first_num.is_none() => {
                first_num = Some(if *v { 1.0 } else { 0.0 });
            }
            BtWgValue::Obj(child) | BtWgValue::ObjKeyInline { obj: child } => {
                scan_object_params(child, out, depth + 1);
            }
            BtWgValue::ObjList(list) => {
                for child in list {
                    scan_object_params(child, out, depth + 1);
                }
            }
            _ => {}
        }
    }

    if let (Some(name), Some(val)) = (first_str, first_num) {
        if out.len() < 512 {
            out.push((name.to_string(), val));
        }
    }
}

/// Devuelve `true` si la ruta corresponde a un archivo `.bwremotecontrols`.
pub fn is_bwremotecontrols_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("bwremotecontrols"))
        .unwrap_or(false)
}

/// Parsea un documento `BtWg00030002` (`.bwremotecontrols`) en páginas con nombre,
/// tags, slots 0..8, IDs de parámetro y CC MIDI opcionales.
pub fn parse_bwremotecontrols_doc(doc: &BtWgDocument) -> BwRemoteControlsSpec {
    let mut spec = BwRemoteControlsSpec {
        device_name: doc.device_name().map(str::to_string),
        device_id: doc.device_id().map(str::to_string),
        creator: None,
        pages: Vec::new(),
    };
    if let Some(meta) = &doc.meta {
        for (k, v) in &meta.named_fields {
            if k == "creator" {
                if let BtWgValue::Str(s) = v {
                    if !s.trim().is_empty() {
                        spec.creator = Some(s.clone());
                    }
                }
            }
        }
    }

    let mut raw_mappings = Vec::new();
    let mut page_tags_map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut slot_meta_map: BTreeMap<(String, usize), (Option<String>, Option<u8>)> =
        BTreeMap::new();

    if let Some(root) = &doc.body {
        scan_remote_pages_detailed(
            root,
            "Perform",
            &mut raw_mappings,
            &mut page_tags_map,
            &mut slot_meta_map,
            0,
        );
    }

    let mut page_order: Vec<String> = Vec::new();
    let mut pages_by_name: BTreeMap<String, Vec<RemoteControlSlotSpec>> = BTreeMap::new();
    for m in raw_mappings {
        if !pages_by_name.contains_key(&m.page_name) {
            page_order.push(m.page_name.clone());
        }
        let (param_id, midi_cc) = slot_meta_map
            .get(&(m.page_name.clone(), m.slot_index))
            .cloned()
            .unwrap_or((None, None));
        let slots = pages_by_name.entry(m.page_name).or_default();
        if let Some(existing) = slots.iter_mut().find(|s| s.slot_index == m.slot_index) {
            existing.param_name = m.param_name;
            if param_id.is_some() {
                existing.param_id = param_id;
            }
            if midi_cc.is_some() {
                existing.midi_cc = midi_cc;
            }
        } else {
            slots.push(RemoteControlSlotSpec {
                slot_index: m.slot_index,
                param_name: m.param_name,
                param_id,
                midi_cc,
            });
        }
    }

    for pname in page_order {
        let mut slots = pages_by_name.remove(&pname).unwrap_or_default();
        slots.sort_by_key(|s| s.slot_index);
        let tags = page_tags_map.remove(&pname).unwrap_or_default();
        spec.pages.push(RemoteControlPageSpec {
            name: pname,
            tags,
            slots,
        });
    }

    spec
}

/// Carga y parsea un archivo `.bwremotecontrols` desde disco.
pub fn load_bwremotecontrols_file(path: &Path) -> Result<BwRemoteControlsSpec, String> {
    let bytes =
        fs::read(path).map_err(|err| format!("no se pudo leer '{}': {err}", path.display()))?;
    let header = BtWgHeader::parse(&bytes)?;
    if header.kind() != BtWgContainerKind::PublicAst {
        return Err(format!(
            "'{}' no es un contenedor BtWg00030002 válido",
            path.display()
        ));
    }
    let doc = BtWgReader::parse_document(&bytes)?;
    Ok(parse_bwremotecontrols_doc(&doc))
}

/// Busca un archivo `.bwremotecontrols` / `Default.bwremotecontrols` compañero para un
/// `.bwdevice` o `.bwpreset` en disco o en las carpetas `device-settings`.
pub fn find_companion_bwremotecontrols(
    artifact_path: &Path,
    library_dir: Option<&Path>,
    device_id: Option<&str>,
) -> Option<BtWgDocument> {
    let mut candidates = Vec::new();
    candidates.push(artifact_path.with_extension("bwremotecontrols"));
    if let Some(parent) = artifact_path.parent() {
        candidates.push(parent.join("Default.bwremotecontrols"));
    }

    let mut lib_roots = Vec::new();
    if let Some(dir) = library_dir {
        lib_roots.push(dir.to_path_buf());
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        lib_roots.push(home.join("Bitwig Studio/Library"));
        lib_roots.push(home.join("Documents/Bitwig Studio/Library"));
    }
    lib_roots.push(PathBuf::from("/opt/bitwig-studio/Library"));

    if let Some(id) = device_id.filter(|s| !s.trim().is_empty()) {
        for root in &lib_roots {
            candidates.push(root.join("device-settings").join(id).join("Default.bwremotecontrols"));
            candidates.push(root.join(id).join("Default.bwremotecontrols"));
        }
    }

    for cand in candidates {
        if cand.is_file() {
            if let Ok(bytes) = fs::read(&cand) {
                if let Ok(doc) = BtWgReader::parse_document(&bytes) {
                    return Some(doc);
                }
            }
        }
    }
    None
}

/// Extrae asignaciones de páginas de 8 controles remotos desde un `.bwremotecontrols`.
pub fn extract_remote_controls(doc: &BtWgDocument) -> Vec<MacroMapping> {
    parse_bwremotecontrols_doc(doc).to_macro_mappings()
}

fn scan_remote_pages_detailed(
    obj: &BtWgObject,
    current_page: &str,
    out: &mut Vec<MacroMapping>,
    page_tags: &mut BTreeMap<String, Vec<String>>,
    slot_meta: &mut BTreeMap<(String, usize), (Option<String>, Option<u8>)>,
    depth: usize,
) {
    if depth > MAX_PARAM_SCAN_DEPTH || obj.cls == 0x0500 {
        return;
    }

    let mut page_name = current_page.to_string();
    let mut slot_index: Option<usize> = None;
    let mut param_name: Option<String> = None;
    let mut param_id: Option<String> = None;
    let mut midi_cc: Option<u8> = None;

    for (fid, val) in &obj.fields {
        match val {
            BtWgValue::Str(s) if !s.is_empty() => {
                if obj.cls == 0x0810 && *fid == 0x01 {
                    page_name = s.clone();
                } else if obj.cls == 0x0810 && *fid == 0x02 {
                    page_tags
                        .entry(page_name.clone())
                        .or_default()
                        .extend(
                            s.split(',')
                                .map(str::trim)
                                .filter(|t| !t.is_empty())
                                .map(str::to_string),
                        );
                } else if obj.cls == 0x0800 && *fid == 0x03 {
                    param_id = Some(s.clone());
                } else if param_name.is_none() {
                    param_name = Some(s.clone());
                } else if obj.cls != 0x0800 {
                    page_name = s.clone();
                } else if param_id.is_none() {
                    param_id = Some(s.clone());
                }
            }
            BtWgValue::I32(idx) if (0..8).contains(idx) && slot_index.is_none() && *fid != 0x04 => {
                slot_index = Some(*idx as usize);
            }
            BtWgValue::I32(cc) if *fid == 0x04 && (0..=127).contains(cc) => {
                midi_cc = Some(*cc as u8);
            }
            BtWgValue::Obj(child) | BtWgValue::ObjKeyInline { obj: child } => {
                scan_remote_pages_detailed(
                    child,
                    &page_name,
                    out,
                    page_tags,
                    slot_meta,
                    depth + 1,
                );
            }
            BtWgValue::ObjList(list) => {
                for (i, child) in list.iter().enumerate() {
                    let before = out.len();
                    scan_remote_pages_detailed(
                        child,
                        &page_name,
                        out,
                        page_tags,
                        slot_meta,
                        depth + 1,
                    );
                    if out.len() == before {
                        if let Some(BtWgValue::Str(s)) = child.fields.iter().find_map(|(_, v)| {
                            matches!(v, BtWgValue::Str(st) if !st.is_empty()).then_some(v)
                        }) {
                            out.push(MacroMapping {
                                page_name: page_name.clone(),
                                slot_index: i % 8,
                                param_name: s.clone(),
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if let (Some(idx), Some(name)) = (slot_index, param_name) {
        if param_id.is_some() || midi_cc.is_some() {
            slot_meta.insert((page_name.clone(), idx), (param_id, midi_cc));
        }
        out.push(MacroMapping {
            page_name,
            slot_index: idx,
            param_name: name,
        });
    }
}

/// Normaliza un nombre de modulador de Bitwig al identificador aceptado por
/// `hikaru_openmodulation::ModulatorKind::from_name`.
pub fn normalize_bitwig_modulator_kind(bitwig_name: &str) -> Option<&'static str> {
    match bitwig_name.trim().to_ascii_lowercase().as_str() {
        "classic lfo" | "lfo" => Some("Classic LFO"),
        "beat lfo" => Some("Beat LFO"),
        "random" => Some("Random"),
        "sample and hold" | "sample & hold" | "s&h" => Some("Sample and Hold"),
        "vibrato" => Some("Vibrato"),
        "wavetable lfo" => Some("Wavetable LFO"),
        "macro" => Some("Macro"),
        "macro-4" | "macro 4" => Some("Macro-4"),
        "button" => Some("Button"),
        "buttons" => Some("Buttons"),
        "xy" => Some("XY"),
        "vector-4" | "vector 4" => Some("Vector-4"),
        "vector-8" | "vector 8" => Some("Vector-8"),
        "globals" => Some("Globals"),
        "math" => Some("Math"),
        "adsr" => Some("ADSR"),
        "ahdsr" => Some("AHDSR"),
        "4-stage" | "four stage" => Some("4-Stage"),
        "ahd on release" => Some("AHD on Release"),
        "steps" => Some("Steps"),
        "parsec" => Some("Parsec"),
        "envelope follower" => Some("Envelope Follower"),
        "audio sidechain" => Some("Audio Sidechain"),
        "note sidechain" => Some("Note Sidechain"),
        "expressions" => Some("Expressions"),
        "keytrack+" | "keytrack" => Some("Keytrack+"),
        "relative keytrack" => Some("Relative Keytrack"),
        "pitch-12" | "pitch 12" => Some("Pitch-12"),
        "note counter" => Some("Note Counter"),
        "voice control" => Some("Voice Control"),
        _ => None,
    }
}

/// Extrae los slots de moduladores (`MODULATORS`) y las tuplas `ObjKeyRef` (`0x1a`)
/// desde un `BtWgDocument`, resolviendo referencias en la tabla 1-based DFS y
/// construyendo un `DeviceModulatorBayParams` listo para `hikaru_openmodulation`.
pub fn extract_modulators_to_bay(
    doc: &BtWgDocument,
    bitwig_device_name: &str,
) -> DeviceModulatorBayParams {
    let mut slots_by_obj_id: BTreeMap<u32, ModulatorSlotParams> = BTreeMap::new();
    let mut pending_routes: Vec<(u32, ModulatorRouteParams)> = Vec::new();

    if let Some(root) = &doc.body {
        scan_modulators_and_keys(
            root,
            doc,
            bitwig_device_name,
            &mut slots_by_obj_id,
            &mut pending_routes,
            0,
        );
    }

    // Asignar cada ruta resuelta al slot correspondiente (o crear un Macro fallback
    // si la referencia apunta a un objeto sin nombre explícito de modulador).
    for (target_obj_id, route) in pending_routes {
        if let Some(slot) = slots_by_obj_id.get_mut(&target_obj_id) {
            slot.routes.push(route);
        } else {
            let inferred_kind = doc
                .resolve_ref(target_obj_id)
                .and_then(|obj| {
                    obj.fields.iter().find_map(|(_, v)| match v {
                        BtWgValue::Str(s) => normalize_bitwig_modulator_kind(s),
                        _ => None,
                    })
                })
                .unwrap_or("Macro");

            let entry = slots_by_obj_id
                .entry(target_obj_id)
                .or_insert_with(|| ModulatorSlotParams {
                    id: target_obj_id.max(1),
                    kind: inferred_kind.to_string(),
                    enabled: true,
                    params: Vec::new(),
                    bool_flags: Vec::new(),
                    mode_idx: 0,
                    routes: Vec::new(),
                });
            entry.routes.push(route);
        }
    }

    let slots: Vec<ModulatorSlotParams> = slots_by_obj_id.into_values().collect();
    DeviceModulatorBayParams {
        open: !slots.is_empty(),
        slots,
    }
}

fn scan_modulators_and_keys(
    obj: &BtWgObject,
    doc: &BtWgDocument,
    bitwig_device_name: &str,
    slots_by_obj_id: &mut BTreeMap<u32, ModulatorSlotParams>,
    pending_routes: &mut Vec<(u32, ModulatorRouteParams)>,
    depth: usize,
) {
    if depth > MAX_PARAM_SCAN_DEPTH || obj.cls == 0x0500 {
        return;
    }

    // 1. Detectar si este objeto representa un slot de modulador conocido.
    let mut mod_kind: Option<&'static str> = None;
    let mut enabled = true;
    let mut scalar_params: Vec<f32> = Vec::new();
    let mut local_amount: Option<f32> = None;

    for (_, val) in &obj.fields {
        match val {
            BtWgValue::Str(s) if mod_kind.is_none() => {
                mod_kind = normalize_bitwig_modulator_kind(s);
            }
            BtWgValue::Bool(b) => {
                enabled = *b;
            }
            BtWgValue::F64(v) if v.is_finite() => {
                let fv = *v as f32;
                scalar_params.push(fv);
                if local_amount.is_none() && (-1.0..=1.0).contains(&fv) {
                    local_amount = Some(fv);
                }
            }
            BtWgValue::F32(v) if v.is_finite() => {
                scalar_params.push(*v);
                if local_amount.is_none() && (-1.0..=1.0).contains(v) {
                    local_amount = Some(*v);
                }
            }
            _ => {}
        }
    }

    if let Some(kind_name) = mod_kind {
        slots_by_obj_id
            .entry(obj.obj_id)
            .or_insert_with(|| ModulatorSlotParams {
                id: obj.obj_id.max(1),
                kind: kind_name.to_string(),
                enabled,
                params: scalar_params,
                bool_flags: Vec::new(),
                mode_idx: 0,
                routes: Vec::new(),
            });
    }

    // 2. Recorrer tuplas `ObjKeyRef` (`0x1a`) e hijos recursivos.
    for (_, val) in &obj.fields {
        match val {
            BtWgValue::ObjKeyRef { obj_id, key } => {
                let translated_target =
                    translate_param_for_device(bitwig_device_name, key, 0.0).hikaru_name;
                let amount = local_amount.unwrap_or(0.25);
                pending_routes.push((
                    *obj_id,
                    ModulatorRouteParams {
                        out_idx: 0,
                        target: translated_target,
                        amount,
                    },
                ));
            }
            BtWgValue::Obj(child) | BtWgValue::ObjKeyInline { obj: child } => {
                scan_modulators_and_keys(
                    child,
                    doc,
                    bitwig_device_name,
                    slots_by_obj_id,
                    pending_routes,
                    depth + 1,
                );
            }
            BtWgValue::ObjList(list) => {
                for child in list {
                    scan_modulators_and_keys(
                        child,
                        doc,
                        bitwig_device_name,
                        slots_by_obj_id,
                        pending_routes,
                        depth + 1,
                    );
                }
            }
            _ => {}
        }
    }
}

/// Extrae los moduladores y rutas de un `BtWgDocument` y los aplica directamente
/// al Device Modulator Bay de `hikaru_openmodulation` para `(track_idx, slot_idx)`.
pub fn apply_modulators_to_rack_slot(
    track_idx: usize,
    slot_idx: usize,
    doc: &BtWgDocument,
    bitwig_device_name: &str,
) -> DeviceModulatorBayParams {
    let bay_params = extract_modulators_to_bay(doc, bitwig_device_name);
    if !bay_params.slots.is_empty() {
        hikaru_openmodulation::restore_bay_params(track_idx, slot_idx, &bay_params);
    }
    bay_params
}

// =========================================================================
// bd-04: Cadenas anidadas (`Chain`, `FX Layer`, `Wet FX`, `Multiband`)
// =========================================================================

/// Tipo de cadena anidada dentro de un dispositivo Bitwig.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NestedChainKind {
    WetFx,
    FxLayer,
    DeviceChain,
    Multiband,
}

/// Cadena anidada de dispositivos hijos (equivale a `wet_fx_chain` / layers en `dsp_rack.rs`).
#[derive(Debug, Clone)]
pub struct NestedDeviceChain {
    pub kind: NestedChainKind,
    pub label: String,
    pub devices: Vec<BwDeviceNode>,
}

/// Nodo completo de dispositivo traducido (parámetros + moduladores + cadenas anidadas).
#[derive(Debug, Clone)]
pub struct BwDeviceNode {
    pub translation: BwDeviceTranslation,
    pub modulators: DeviceModulatorBayParams,
    pub nested_chains: Vec<NestedDeviceChain>,
}

/// Traduce un `BtWgDocument` completo incluyendo parámetros, moduladores y
/// cadenas anidadas (`Wet FX`, `FX Layer`, `Chain`, `Multiband`).
pub fn translate_device_tree(
    doc: &BtWgDocument,
    remote_controls: Option<&BtWgDocument>,
) -> BwDeviceNode {
    let translation = translate_device_document(doc, remote_controls);
    let dev_name = match &translation {
        BwDeviceTranslation::Builtin { bitwig_name, .. } => bitwig_name.as_str(),
        BwDeviceTranslation::Placeholder(p) => p.bitwig_name.as_str(),
    };
    let modulators = extract_modulators_to_bay(doc, dev_name);
    let mut nested_chains = Vec::new();
    if let Some(root) = &doc.body {
        collect_nested_chains(root, doc, &mut nested_chains, 0);
    }
    BwDeviceNode {
        translation,
        modulators,
        nested_chains,
    }
}

fn classify_chain_label(label: &str) -> Option<NestedChainKind> {
    let lower = label.trim().to_ascii_lowercase();
    if lower.contains("wet") {
        Some(NestedChainKind::WetFx)
    } else if lower.contains("layer") {
        Some(NestedChainKind::FxLayer)
    } else if lower.contains("multiband") || lower.contains("band") {
        Some(NestedChainKind::Multiband)
    } else if lower == "fx" || lower == "chain" || lower.contains("chain") {
        Some(NestedChainKind::DeviceChain)
    } else {
        None
    }
}

fn collect_nested_chains(
    obj: &BtWgObject,
    doc: &BtWgDocument,
    out: &mut Vec<NestedDeviceChain>,
    depth: usize,
) {
    if depth > MAX_PARAM_SCAN_DEPTH || obj.cls == 0x0500 {
        return;
    }

    let chain_label = obj.fields.iter().find_map(|(_, v)| match v {
        BtWgValue::Str(s) if classify_chain_label(s).is_some() => Some(s.clone()),
        _ => None,
    });

    for (fid, val) in &obj.fields {
        match val {
            BtWgValue::ObjList(list) if !list.is_empty() => {
                let kind_opt = chain_label
                    .as_deref()
                    .and_then(classify_chain_label)
                    .or((*fid == 0x144).then_some(NestedChainKind::DeviceChain));

                if let Some(kind) = kind_opt {
                    let mut devices = Vec::new();
                    for child in list {
                        if let Some(node) = build_nested_device_node(child, doc, depth + 1) {
                            devices.push(node);
                        }
                    }
                    if !devices.is_empty() {
                        out.push(NestedDeviceChain {
                            kind,
                            label: chain_label.clone().unwrap_or_else(|| "Chain".to_string()),
                            devices,
                        });
                        continue;
                    }
                }

                for child in list {
                    collect_nested_chains(child, doc, out, depth + 1);
                }
            }
            BtWgValue::Obj(child) | BtWgValue::ObjKeyInline { obj: child } => {
                collect_nested_chains(child, doc, out, depth + 1);
            }
            _ => {}
        }
    }
}

fn build_nested_device_node(
    obj: &BtWgObject,
    doc: &BtWgDocument,
    depth: usize,
) -> Option<BwDeviceNode> {
    if depth > MAX_PARAM_SCAN_DEPTH {
        return None;
    }
    let child_name = obj.fields.iter().find_map(|(_, v)| match v {
        BtWgValue::Str(s) if lookup_compat_target(Some(s), None).is_some() => Some(s.clone()),
        _ => None,
    })?;

    let mut sub_doc = BtWgDocument {
        header: doc.header.clone(),
        kind: BtWgContainerKind::PublicAst,
        meta: None,
        body: Some(obj.clone()),
        objects_by_id: doc.objects_by_id.clone(),
    };
    // Si el objeto hijo no tiene `meta`, construimos la traducción directamente
    let raw_params = extract_raw_params(&sub_doc);
    let target = lookup_compat_target(Some(&child_name), None);
    let translation = match target {
        Some(compat) => {
            let mut params = BTreeMap::new();
            for (name, raw) in raw_params {
                let translated = translate_param_for_device(compat.bitwig_name, &name, raw);
                params.insert(translated.hikaru_name.clone(), translated);
            }
            BwDeviceTranslation::Builtin {
                bitwig_name: child_name.clone(),
                hikaru_slot_name: compat.hikaru_slot_name.to_string(),
                device_id: None,
                params,
                macro_mappings: Vec::new(),
            }
        }
        None => unreachable!(),
    };

    let modulators = extract_modulators_to_bay(&sub_doc, &child_name);
    let mut nested_chains = Vec::new();
    collect_nested_chains(obj, &sub_doc, &mut nested_chains, depth + 1);
    sub_doc.body = None;

    Some(BwDeviceNode {
        translation,
        modulators,
        nested_chains,
    })
}

// =========================================================================
// bd-05: Traducción de Grid patches (`OpenGridParams`) y Browser (.bwpreset/.bwdevice)
// =========================================================================

fn normalize_grid_module_kind(raw: &str) -> Option<&'static str> {
    let lower = raw.trim().to_ascii_lowercase();
    match lower.as_str() {
        "wavetable" => Some("Wavetable"),
        "osc" | "sine" | "saw" | "triangle" | "pulse" => Some("Oscillator"),
        "svf" | "filter" | "lowpass" | "highpass" => Some("Filter"),
        "adsr" | "envelope" => Some("ADSR"),
        "lfo" | "classic lfo" => Some("LFO"),
        "mixer" | "mix" => Some("Mixer"),
        "audio out" | "out" => Some("Audio Out"),
        "note in" | "pitch" => Some("Note In"),
        "noise" => Some("Noise"),
        "delay" => Some("Delay"),
        "gain" | "level" => Some("Gain"),
        _ => None,
    }
}

/// Traduce un patch público de `Poly Grid` / `FX Grid` / `Note Grid` (`.bwpreset`)
/// hacia `OpenGridParams` de `hikaru_opengrid`.
pub fn translate_grid_patch(doc: &BtWgDocument) -> OpenGridParams {
    let mut params = OpenGridParams {
        preset_name: doc
            .device_name()
            .unwrap_or("Imported Bitwig Grid")
            .to_string(),
        ..OpenGridParams::default()
    };

    let mut obj_to_node_id: BTreeMap<u32, u32> = BTreeMap::new();
    let mut raw_cables: Vec<(u32, u32)> = Vec::new();

    if let Some(root) = &doc.body {
        scan_grid_nodes(
            root,
            &mut params.nodes,
            &mut obj_to_node_id,
            &mut raw_cables,
            0,
        );
    }

    for (from_obj_id, to_obj_id) in raw_cables {
        if let (Some(&from_node), Some(&to_node)) = (
            obj_to_node_id.get(&from_obj_id),
            obj_to_node_id.get(&to_obj_id),
        ) {
            if from_node != to_node {
                params.cables.push(OpenGridCableData {
                    from_node,
                    from_port: 0,
                    to_node,
                    to_port: 0,
                });
            }
        }
    }

    params
}

fn scan_grid_nodes(
    obj: &BtWgObject,
    nodes: &mut Vec<OpenGridNodeData>,
    obj_to_node_id: &mut BTreeMap<u32, u32>,
    raw_cables: &mut Vec<(u32, u32)>,
    depth: usize,
) {
    if depth > MAX_PARAM_SCAN_DEPTH || obj.cls == 0x0500 {
        return;
    }

    let mut module_kind: Option<&'static str> = None;
    let mut scalar_params = [0.0f32; 6];
    let mut param_idx = 0usize;

    for (_, val) in &obj.fields {
        match val {
            BtWgValue::Str(s) if module_kind.is_none() => {
                module_kind = normalize_grid_module_kind(s);
            }
            BtWgValue::F64(v) if v.is_finite() && param_idx < 6 => {
                scalar_params[param_idx] = *v as f32;
                param_idx += 1;
            }
            BtWgValue::F32(v) if v.is_finite() && param_idx < 6 => {
                scalar_params[param_idx] = *v;
                param_idx += 1;
            }
            BtWgValue::Ref(target_id) => {
                raw_cables.push((*target_id, obj.obj_id));
            }
            _ => {}
        }
    }

    if let Some(kind) = module_kind {
        let node_id = (nodes.len() as u32) + 1;
        let col = (nodes.len() % 4) as f32;
        let row = (nodes.len() / 4) as f32;
        nodes.push(OpenGridNodeData {
            id: node_id,
            kind: kind.to_string(),
            pos: [120.0 + col * 160.0, 90.0 + row * 120.0],
            params: scalar_params,
            bypassed: false,
        });
        obj_to_node_id.insert(obj.obj_id, node_id);
    }

    for (_, val) in &obj.fields {
        match val {
            BtWgValue::Obj(child) | BtWgValue::ObjKeyInline { obj: child } => {
                scan_grid_nodes(child, nodes, obj_to_node_id, raw_cables, depth + 1);
            }
            BtWgValue::ObjList(list) => {
                for child in list {
                    scan_grid_nodes(child, nodes, obj_to_node_id, raw_cables, depth + 1);
                }
            }
            _ => {}
        }
    }
}

/// Devuelve `true` si la ruta corresponde a un `.bwdevice` o `.bwpreset` cargable desde el browser.
pub fn is_bitwig_browser_artifact(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("bwdevice") || ext.eq_ignore_ascii_case("bwpreset"))
        .unwrap_or(false)
}

/// Carga un `.bwpreset` o `.bwdevice` desde el browser (`explorer.rs` / `popup_browser.rs`)
/// devolviendo el árbol traducido `BwDeviceNode` listo para instanciar en el rack.
pub fn load_bitwig_browser_item(
    path: &Path,
    library_dir: Option<&Path>,
) -> Result<BwDeviceNode, String> {
    let bytes =
        fs::read(path).map_err(|err| format!("no se pudo leer '{}': {err}", path.display()))?;
    let header = BtWgHeader::parse(&bytes)?;

    match header.kind() {
        BtWgContainerKind::PublicAst => {
            let doc = BtWgReader::parse_document(&bytes)?;
            let companion_rc = find_companion_bwremotecontrols(path, library_dir, doc.device_id());
            Ok(translate_device_tree(&doc, companion_rc.as_ref()))
        }
        BtWgContainerKind::ProtectedDevice => {
            let meta = BtWgReader::inspect_bwdevice_file(path, library_dir)?;
            let companion_id = meta
                .companion_preset
                .as_ref()
                .and_then(|d| d.device_id())
                .map(str::to_string);
            let companion_rc = find_companion_bwremotecontrols(
                path,
                library_dir,
                companion_id.as_deref(),
            );
            if let Some(companion) = meta.companion_preset {
                Ok(translate_device_tree(&companion, companion_rc.as_ref()))
            } else {
                // Sin companion preset: resolvemos por nombre del archivo hacia `Builtin` o `Placeholder`.
                let compat = lookup_compat_target(Some(&meta.device_stem), None);
                let translation = match compat {
                    Some(c) => BwDeviceTranslation::Builtin {
                        bitwig_name: meta.device_stem.clone(),
                        hikaru_slot_name: c.hikaru_slot_name.to_string(),
                        device_id: None,
                        params: BTreeMap::new(),
                        macro_mappings: Vec::new(),
                    },
                    None => BwDeviceTranslation::Placeholder(PassthroughPlaceholderShim {
                        bitwig_name: meta.device_stem.clone(),
                        device_id: None,
                        preserved_ast: BtWgDocument {
                            header: meta.header,
                            kind: BtWgContainerKind::ProtectedDevice,
                            meta: None,
                            body: None,
                            objects_by_id: BTreeMap::new(),
                        },
                        read_only_params: Vec::new(),
                        macro_mappings: Vec::new(),
                    }),
                };
                Ok(BwDeviceNode {
                    translation,
                    modulators: DeviceModulatorBayParams::default(),
                    nested_chains: Vec::new(),
                })
            }
        }
        BtWgContainerKind::Unknown => Err(format!(
            "tipo BtWg '{}' no soportado en '{}'",
            header.format_type,
            path.display()
        )),
    }
}

// =========================================================================
// bpr-05: Browser discovery, preview & preset export (.bwpreset)
// =========================================================================

/// Entrada descubierta de preset/dispositivo/modulador Bitwig en disco para el browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableBitwigPreset {
    pub id: String,
    pub name: String,
    pub device_name: String,
    pub category: String,
    pub creator: String,
    pub location: String,
    pub file_kind: String,
    pub tags: Vec<String>,
    pub description: String,
    pub path: PathBuf,
    pub is_grid: bool,
    pub is_modulator: bool,
    pub is_fx: bool,
}

/// Resumen ligero para previsualización en el panel derecho del Pop-up Browser.
#[derive(Debug, Clone, Default)]
pub struct BwPresetPreview {
    pub device_name: String,
    pub preset_name: String,
    pub creator: String,
    pub category: String,
    pub container_kind: String,
    pub param_count: usize,
    pub sample_params: Vec<(String, f32)>,
    pub modulator_count: usize,
    pub modulator_kinds: Vec<String>,
    pub grid_node_count: usize,
    pub grid_cable_count: usize,
    pub error: Option<String>,
}

/// Especificación para exportar un dispositivo / rack / patch Grid de Hikaru a `.bwpreset`.
#[derive(Debug, Clone)]
pub struct BwPresetExportSpec {
    pub device_name: String,
    pub preset_name: String,
    pub creator: String,
    pub category: String,
    pub comment: String,
    pub params: Vec<(String, f64)>,
    pub modulators: DeviceModulatorBayParams,
    pub grid: Option<OpenGridParams>,
    pub remote_labels: Vec<(usize, String)>,
}

fn is_bitwig_catalog_ext(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "bwpreset" => Some(".bwpreset"),
        "bwdevice" => Some(".bwdevice"),
        "bwmodulator" => Some(".bwmodulator"),
        "bwremotecontrols" => Some(".bwremotecontrols"),
        _ => None,
    }
}

fn classify_bitwig_device_role(device_name: &str, category: &str, file_kind: &str) -> (bool, bool, bool) {
    let dev_low = device_name.to_ascii_lowercase();
    let cat_low = category.to_ascii_lowercase();
    let is_grid = dev_low.contains("grid");
    let is_mod = file_kind == ".bwmodulator" || cat_low.contains("modulat");
    let is_inst = cat_low.contains("synth")
        || cat_low.contains("instrument")
        || cat_low.contains("sampler")
        || cat_low.contains("drum")
        || matches!(
            dev_low.as_str(),
            "polymer" | "polysynth" | "phase-4" | "fm-4" | "sampler" | "drum machine" | "poly grid"
        );
    let is_fx = !is_mod && !is_inst;
    (is_grid, is_mod, is_fx)
}

fn scan_bitwig_preset_dir(
    dir: &Path,
    location_label: &str,
    depth: usize,
    max_depth: usize,
    max_items: usize,
    out: &mut Vec<AvailableBitwigPreset>,
) {
    if depth > max_depth || out.len() >= max_items || !dir.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();

    for path in paths {
        if out.len() >= max_items {
            break;
        }
        if path.is_dir() {
            scan_bitwig_preset_dir(&path, location_label, depth + 1, max_depth, max_items, out);
            continue;
        }
        let Some(file_kind) = is_bitwig_catalog_ext(&path) else {
            continue;
        };
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Preset")
            .to_string();

        let mut device_name = stem.clone();
        let mut category = String::from("Bitwig Preset");
        let mut creator = String::from("Bitwig");
        let mut tags = vec![file_kind.trim_start_matches('.').to_string()];

        if file_kind == ".bwpreset" {
            if let Ok(bytes) = fs::read(&path) {
                if let Ok(doc) = BtWgReader::parse_document(&bytes) {
                    if let Some(dn) = doc.device_name() {
                        device_name = dn.to_string();
                    }
                    if let Some(meta) = &doc.meta {
                        for (k, v) in &meta.named_fields {
                            match (k.as_str(), v) {
                                ("preset_category", BtWgValue::Str(s)) if !s.trim().is_empty() => {
                                    category = s.clone();
                                }
                                ("creator", BtWgValue::Str(s)) if !s.trim().is_empty() => {
                                    creator = s.clone();
                                }
                                ("tags", BtWgValue::Str(s)) if !s.trim().is_empty() => {
                                    for t in s.split(|c: char| c == ',' || c.is_whitespace()) {
                                        let trimmed = t.trim();
                                        if !trimmed.is_empty() {
                                            tags.push(trimmed.to_string());
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        } else if file_kind == ".bwdevice" {
            category = "Bitwig Device".to_string();
        } else if file_kind == ".bwmodulator" {
            category = "Modulator".to_string();
        } else if file_kind == ".bwremotecontrols" {
            category = "Remote Controls".to_string();
            if let Ok(rc_spec) = load_bwremotecontrols_file(&path) {
                if let Some(dn) = rc_spec.device_name {
                    device_name = dn;
                }
                if let Some(cr) = rc_spec.creator {
                    creator = cr;
                }
                for page in &rc_spec.pages {
                    for t in &page.tags {
                        if !tags.contains(t) {
                            tags.push(t.clone());
                        }
                    }
                }
            }
        }

        if let Some(compat) = lookup_compat_target(Some(&device_name), None) {
            if category == "Bitwig Preset" || category == "Bitwig Device" {
                category = compat.category.to_string();
            }
        }

        let (is_grid, is_modulator, is_fx) =
            classify_bitwig_device_role(&device_name, &category, file_kind);

        let description = format!(
            "{} ({}) · {} · {}",
            device_name,
            file_kind,
            category,
            path.display()
        );

        out.push(AvailableBitwigPreset {
            id: format!("bw:{}:{}", file_kind, path.display()),
            name: stem,
            device_name,
            category,
            creator,
            location: location_label.to_string(),
            file_kind: file_kind.to_string(),
            tags,
            description,
            path,
            is_grid,
            is_modulator,
            is_fx,
        });
    }
}

/// Descubre `.bwpreset`, `.bwdevice` y `.bwmodulator` en las bibliotecas estándar de Bitwig
/// y carpetas locales del workspace (con límite de profundidad para mantener el browser fluido).
pub fn discover_bitwig_presets(extra_dir: Option<&Path>) -> Vec<AvailableBitwigPreset> {
    let mut candidates: Vec<(PathBuf, &'static str)> = Vec::new();
    if let Some(dir) = extra_dir {
        candidates.push((dir.to_path_buf(), "Custom Library"));
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        candidates.push((home.join("Bitwig Studio/Library/Presets"), "Bitwig Library"));
        candidates.push((home.join("Bitwig Studio/Library/Devices"), "Bitwig Library"));
        candidates.push((home.join("Bitwig Studio/Library/Modulators"), "Bitwig Library"));
        candidates.push((
            home.join("Bitwig Studio/Library/Remote Controls"),
            "Bitwig Library",
        ));
        candidates.push((
            home.join("Documents/Bitwig Studio/Library/Presets"),
            "User Presets",
        ));
        candidates.push((
            home.join("Documents/Bitwig Studio/Library/Remote Controls"),
            "User Remote Controls",
        ));
        candidates.push((home.join("Music/Bitwig Studio/Presets"), "User Presets"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push((cwd.join("presets"), "Workspace Presets"));
        candidates.push((cwd.join("assets/presets"), "Workspace Presets"));
    }

    let mut out = Vec::new();
    for (dir, label) in candidates {
        scan_bitwig_preset_dir(&dir, label, 0, 4, 400, &mut out);
        if out.len() >= 400 {
            break;
        }
    }
    out
}

/// Extrae una vista previa estructurada de un `.bwpreset` / `.bwdevice` para el Pop-up Browser.
pub fn inspect_bwpreset_preview(path: &Path) -> BwPresetPreview {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Preset")
        .to_string();
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(err) => {
            return BwPresetPreview {
                preset_name: stem,
                error: Some(err.to_string()),
                ..BwPresetPreview::default()
            };
        }
    };
    let header = match BtWgHeader::parse(&bytes) {
        Ok(h) => h,
        Err(err) => {
            return BwPresetPreview {
                preset_name: stem,
                error: Some(err),
                ..BwPresetPreview::default()
            };
        }
    };

    match header.kind() {
        BtWgContainerKind::PublicAst => match BtWgReader::parse_document(&bytes) {
            Ok(doc) => {
                let device_name = doc.device_name().unwrap_or(&stem).to_string();
                let mut creator = "Bitwig".to_string();
                let mut category = "Preset".to_string();
                let mut preset_name = stem.clone();
                if let Some(meta) = &doc.meta {
                    for (k, v) in &meta.named_fields {
                        if let BtWgValue::Str(s) = v {
                            if !s.trim().is_empty() {
                                match k.as_str() {
                                    "creator" => creator = s.clone(),
                                    "preset_category" => category = s.clone(),
                                    "preset_name" => preset_name = s.clone(),
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                let node = translate_device_tree(&doc, None);
                let (param_count, sample_params) = match &node.translation {
                    BwDeviceTranslation::Builtin { params, .. } => {
                        let list = params
                            .iter()
                            .take(6)
                            .map(|(k, p)| (k.clone(), p.normalized))
                            .collect();
                        (params.len(), list)
                    }
                    BwDeviceTranslation::Placeholder(p) => {
                        let list = p
                            .read_only_params
                            .iter()
                            .take(6)
                            .map(|tp| (tp.bitwig_name.clone(), tp.normalized))
                            .collect();
                        (p.read_only_params.len(), list)
                    }
                };
                let modulator_kinds: Vec<String> = node
                    .modulators
                    .slots
                    .iter()
                    .map(|s| s.kind.clone())
                    .collect();
                let is_grid = device_name.to_ascii_lowercase().contains("grid");
                let (grid_node_count, grid_cable_count) = if is_grid {
                    let g = translate_grid_patch(&doc);
                    (g.nodes.len(), g.cables.len())
                } else {
                    (0, 0)
                };

                BwPresetPreview {
                    device_name,
                    preset_name,
                    creator,
                    category,
                    container_kind: "BtWg00030002 (Public AST)".to_string(),
                    param_count,
                    sample_params,
                    modulator_count: modulator_kinds.len(),
                    modulator_kinds,
                    grid_node_count,
                    grid_cable_count,
                    error: None,
                }
            }
            Err(err) => BwPresetPreview {
                preset_name: stem,
                container_kind: "BtWg00030002".to_string(),
                error: Some(err),
                ..BwPresetPreview::default()
            },
        },
        BtWgContainerKind::ProtectedDevice => BwPresetPreview {
            device_name: stem.clone(),
            preset_name: stem,
            creator: "Bitwig".to_string(),
            category: "Protected Device".to_string(),
            container_kind: "BtWg00030004 (Protected)".to_string(),
            ..BwPresetPreview::default()
        },
        BtWgContainerKind::Unknown => BwPresetPreview {
            device_name: stem.clone(),
            preset_name: stem,
            container_kind: format!("Unknown ({})", header.format_type),
            error: Some(format!("formato BtWg '{}' no soportado", header.format_type)),
            ..BwPresetPreview::default()
        },
    }
}

/// Serializa una especificación de preset de dispositivo / rack / Grid a bytes `.bwpreset` (`BtWg00030002`).
pub fn serialize_bwpreset_bytes(spec: &BwPresetExportSpec) -> Vec<u8> {
    let mut meta = BtWgWriter::new();
    meta.begin_obj(4);
    meta.raw_str("meta");
    meta.named_field_str("device_name", &spec.device_name);
    meta.named_field_str("preset_name", &spec.preset_name);
    meta.named_field_str(
        "creator",
        if spec.creator.trim().is_empty() {
            "Hikaru OpenLive"
        } else {
            &spec.creator
        },
    );
    meta.named_field_str(
        "preset_category",
        if spec.category.trim().is_empty() {
            "Hikaru Preset"
        } else {
            &spec.category
        },
    );
    if !spec.comment.trim().is_empty() {
        meta.named_field_str("comment", &spec.comment);
    }
    meta.end_obj();
    let meta_bytes = meta.into_bytes();

    let mut body = BtWgWriter::new();
    body.begin_obj(0x0010);
    body.field_str(0x01, &spec.device_name);
    body.field_str(0x02, &spec.preset_name);

    // Parámetros del dispositivo (cada parámetro en su propio subobjeto nombre + f64)
    if spec.params.is_empty() {
        body.field_empty_obj_list(0x10);
    } else {
        body.field_obj_list_start(0x10);
        for (pname, pval) in &spec.params {
            body.begin_obj(0x0500);
            body.field_str(0x01, pname);
            body.field_f64(0x02, *pval);
            body.end_obj();
        }
        body.obj_list_end();
    }

    // Grafo OpenGrid (si aplica): nodos + cables con `Ref` 1-based DFS
    if let Some(grid) = &spec.grid {
        if grid.nodes.is_empty() {
            body.field_empty_obj_list(0x20);
        } else {
            body.field_obj_list_start(0x20);
            let mut node_to_obj: BTreeMap<u32, u32> = BTreeMap::new();
            for node in &grid.nodes {
                let obj_id = body.begin_obj(0x0900);
                node_to_obj.insert(node.id, obj_id);
                body.field_str(0x01, &node.kind);
                for (pidx, pv) in node.params.iter().enumerate() {
                    body.field_f64((0x10 + pidx) as u32, *pv as f64);
                }
                for cable in &grid.cables {
                    if cable.to_node == node.id {
                        if let Some(&src_obj_id) = node_to_obj.get(&cable.from_node) {
                            body.field_ref(0x03, src_obj_id);
                        }
                    }
                }
                body.end_obj();
            }
            body.obj_list_end();
        }
    }

    // Moduladores y rutas de modulación `0x1a`
    if !spec.modulators.slots.is_empty() {
        body.field_obj_list_start(0x30);
        for mslot in &spec.modulators.slots {
            let mod_obj_id = body.begin_obj(0x0700);
            body.field_str(0x01, &mslot.kind);
            if !mslot.routes.is_empty() {
                body.field_obj_list_start(0x1a);
                for route in &mslot.routes {
                    body.begin_obj(0x0710);
                    body.field_obj_key_ref(0x02, mod_obj_id, &route.target);
                    body.field_f64(0x05, route.amount as f64);
                    body.end_obj();
                }
                body.obj_list_end();
            }
            body.end_obj();
        }
        body.obj_list_end();
    }

    // Etiquetas de controles remotos / macros
    if !spec.remote_labels.is_empty() {
        body.field_obj_list_start(0x40);
        for (slot_idx, label) in &spec.remote_labels {
            body.begin_obj(0x0800);
            body.field_i32(0x01, *slot_idx as i32);
            body.field_str(0x02, label);
            body.end_obj();
        }
        body.obj_list_end();
    }

    body.end_obj();
    let body_bytes = body.into_bytes();

    // Header estándar BtWg00030002 (43 bytes con '\n' + meta + body)
    let header_len = 43usize;
    let body_offset = header_len + meta_bytes.len();
    let header = format!("BtWg0003000200ba{body_offset:08x}000000000000000000\n");
    let mut out = Vec::with_capacity(body_offset + body_bytes.len());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(&meta_bytes);
    out.extend_from_slice(&body_bytes);
    out
}

/// Exporta una especificación a un archivo `.bwpreset` en disco.
pub fn export_bwpreset_file(spec: &BwPresetExportSpec, path: &Path) -> Result<(), String> {
    let bytes = serialize_bwpreset_bytes(spec);
    fs::write(path, bytes)
        .map_err(|err| format!("no se pudo escribir '{}': {err}", path.display()))
}

/// Serializa una especificación de páginas de controles remotos a bytes `.bwremotecontrols`
/// (`BtWg00030002`), compatible con `BtWgReader::parse_document` y `parse_bwremotecontrols_doc`.
pub fn serialize_bwremotecontrols_bytes(spec: &BwRemoteControlsSpec) -> Vec<u8> {
    let mut meta = BtWgWriter::new();
    meta.begin_obj(4);
    meta.raw_str("meta");
    if let Some(dev_name) = spec.device_name.as_deref().filter(|s| !s.trim().is_empty()) {
        meta.named_field_str("device_name", dev_name);
    }
    if let Some(dev_id) = spec.device_id.as_deref().filter(|s| !s.trim().is_empty()) {
        meta.named_field_str("device_id", dev_id);
    }
    meta.named_field_str(
        "creator",
        spec.creator
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("Hikaru OpenLive"),
    );
    meta.named_field_str("preset_category", "Remote Controls");
    meta.end_obj();
    let meta_bytes = meta.into_bytes();

    let mut body = BtWgWriter::new();
    body.begin_obj(0x0012);
    if let Some(dev_name) = spec.device_name.as_deref() {
        body.field_str(0x01, dev_name);
    }
    if let Some(dev_id) = spec.device_id.as_deref() {
        body.field_str(0x02, dev_id);
    }

    if spec.pages.is_empty() {
        body.field_empty_obj_list(0x40);
    } else {
        body.field_obj_list_start(0x40);
        for page in &spec.pages {
            body.begin_obj(0x0810);
            body.field_str(0x01, &page.name);
            if !page.tags.is_empty() {
                body.field_str(0x02, &page.tags.join(", "));
            }
            if page.slots.is_empty() {
                body.field_empty_obj_list(0x10);
            } else {
                body.field_obj_list_start(0x10);
                for slot in &page.slots {
                    body.begin_obj(0x0800);
                    body.field_i32(0x01, (slot.slot_index % 8) as i32);
                    body.field_str(0x02, &slot.param_name);
                    if let Some(pid) = slot.param_id.as_deref().filter(|s| !s.trim().is_empty()) {
                        body.field_str(0x03, pid);
                    }
                    if let Some(cc) = slot.midi_cc {
                        body.field_i32(0x04, cc as i32);
                    }
                    body.end_obj();
                }
                body.obj_list_end();
            }
            body.end_obj();
        }
        body.obj_list_end();
    }

    body.end_obj();
    let body_bytes = body.into_bytes();

    let header_len = 43usize;
    let body_offset = header_len + meta_bytes.len();
    let header = format!("BtWg0003000200ba{body_offset:08x}000000000000000000\n");
    let mut out = Vec::with_capacity(body_offset + body_bytes.len());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(&meta_bytes);
    out.extend_from_slice(&body_bytes);
    out
}

/// Exporta una especificación `.bwremotecontrols` a disco.
pub fn export_bwremotecontrols_file(
    spec: &BwRemoteControlsSpec,
    path: &Path,
) -> Result<(), String> {
    let bytes = serialize_bwremotecontrols_bytes(spec);
    fs::write(path, bytes)
        .map_err(|err| format!("no se pudo escribir '{}': {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::btwg::BtWgWriter;
    use crate::project::btwg_reader::BtWgReader;

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

    fn build_tool_preset_bytes() -> Vec<u8> {
        let mut meta = BtWgWriter::new();
        meta.begin_obj(4);
        meta.raw_str("meta");
        meta.named_field_str("device_name", "Tool");
        meta.named_field_str("device_id", "00000000-0000-4000-8000-000000000002");
        meta.end_obj();

        let mut body = BtWgWriter::new();
        body.begin_obj(0x561);
        body.field_obj_list_start(0x140);

        body.begin_obj(0x2c1);
        body.field_str(0x10, "GAIN");
        body.field_f64(0x11, 0.0); // 0 dB -> 0.5 normalizado en [-36, 36]
        body.end_obj();

        body.begin_obj(0x2c1);
        body.field_str(0x10, "WIDTH");
        body.field_f64(0x11, 100.0); // 100% -> 0.5 normalizado en [0, 200]
        body.end_obj();

        body.begin_obj(0x2c1);
        body.field_str(0x10, "PHASE_L");
        body.field_bool(0x11, true); // 1.0
        body.end_obj();

        body.obj_list_end();
        body.end_obj();

        wrap_btwg_0002(&meta.into_bytes(), &body.into_bytes())
    }

    fn build_remote_controls_bytes() -> Vec<u8> {
        let mut meta = BtWgWriter::new();
        meta.begin_obj(4);
        meta.raw_str("meta");
        meta.named_field_str("device_name", "Tool");
        meta.end_obj();

        let mut body = BtWgWriter::new();
        body.begin_obj(0x600);
        body.field_str(0x01, "Main");
        body.field_obj_list_start(0x02);

        body.begin_obj(0x601);
        body.field_i32(0x10, 0);
        body.field_str(0x11, "GAIN");
        body.end_obj();

        body.begin_obj(0x601);
        body.field_i32(0x10, 1);
        body.field_str(0x11, "WIDTH");
        body.end_obj();

        body.obj_list_end();
        body.end_obj();

        wrap_btwg_0002(&meta.into_bytes(), &body.into_bytes())
    }

    #[test]
    fn traduce_parametros_tool_y_remote_controls() {
        let preset_doc = BtWgReader::parse_document(&build_tool_preset_bytes())
            .expect("parsea preset Tool");
        let rc_doc = BtWgReader::parse_document(&build_remote_controls_bytes())
            .expect("parsea remote controls");

        let translated = translate_device_document(&preset_doc, Some(&rc_doc));
        match translated {
            BwDeviceTranslation::Builtin {
                hikaru_slot_name,
                params,
                macro_mappings,
                ..
            } => {
                assert_eq!(hikaru_slot_name, "Tool");
                let gain = params.get("gain").expect("parámetro gain");
                assert!(gain.mapped);
                assert!((gain.normalized - 0.5).abs() < 1e-5);

                let width = params.get("width").expect("parámetro width");
                assert!(width.mapped);
                assert!((width.normalized - 0.5).abs() < 1e-5);

                let inv_l = params.get("invert_left").expect("parámetro invert_left");
                assert_eq!(inv_l.normalized, 1.0);

                assert_eq!(macro_mappings.len(), 2);
                assert_eq!(macro_mappings[0].slot_index, 0);
                assert_eq!(macro_mappings[0].param_name, "GAIN");
                assert_eq!(macro_mappings[1].slot_index, 1);
                assert_eq!(macro_mappings[1].param_name, "WIDTH");
            }
            BwDeviceTranslation::Placeholder(_) => panic!("Tool debe mapearse a Builtin"),
        }
    }

    #[test]
    fn dispositivo_sin_equivalente_usa_passthrough_placeholder_shim() {
        let mut meta = BtWgWriter::new();
        meta.begin_obj(4);
        meta.raw_str("meta");
        meta.named_field_str("device_name", "Note Transpose");
        meta.named_field_str("device_id", "0815cd9e-3a31-4429-a268-dabd952a3b68");
        meta.end_obj();

        let mut body = BtWgWriter::new();
        body.begin_obj(0x561);
        body.field_str(0x10, "SEMITONES");
        body.field_f64(0x11, 0.75);
        body.end_obj();

        let bytes = wrap_btwg_0002(&meta.into_bytes(), &body.into_bytes());
        let doc = BtWgReader::parse_document(&bytes).expect("parsea Note Transpose");

        match translate_device_document(&doc, None) {
            BwDeviceTranslation::Placeholder(shim) => {
                assert_eq!(shim.bitwig_name, "Note Transpose");
                assert_eq!(shim.read_only_params.len(), 1);
                assert_eq!(shim.read_only_params[0].bitwig_name, "SEMITONES");
                assert!(!shim.read_only_params[0].mapped);
                assert_eq!(shim.preserved_ast.device_name(), Some("Note Transpose"));
            }
            BwDeviceTranslation::Builtin { .. } => {
                panic!("Note Transpose debe caer en PassthroughPlaceholderShim")
            }
        }
    }

    #[test]
    fn extrae_y_aplica_moduladores_y_rutas_0x1a_hacia_openmodulation() {
        let mut meta = BtWgWriter::new();
        meta.begin_obj(4);
        meta.raw_str("meta");
        meta.named_field_str("device_name", "Tool");
        meta.end_obj();

        let mut body = BtWgWriter::new();
        let root_id = body.begin_obj(0x561);
        assert_eq!(root_id, 1);

        body.field_obj_list_start(0x150);
        let lfo_id = body.begin_obj(0x700);
        assert_eq!(lfo_id, 2);
        body.field_str(0x10, "Classic LFO");
        body.field_f64(0x11, 2.5);
        body.end_obj();

        let macro_id = body.begin_obj(0x700);
        assert_eq!(macro_id, 3);
        body.field_str(0x10, "Macro");
        body.field_f64(0x11, 0.5);
        body.end_obj();
        body.obj_list_end();

        // Tuplas 0x1a `obj_key_ref` apuntando a los moduladores 2 y 3 con amount
        body.field_obj_header(0x160, 0x710);
        body.field_f64(0x01, 0.45);
        body.field_obj_key_ref(0x02, lfo_id, "GAIN");
        body.end_obj();

        body.field_obj_header(0x160, 0x710);
        body.field_f64(0x01, -0.30);
        body.field_obj_key_ref(0x02, macro_id, "WIDTH");
        body.end_obj();

        body.end_obj();

        let bytes = wrap_btwg_0002(&meta.into_bytes(), &body.into_bytes());
        let doc = BtWgReader::parse_document(&bytes).expect("parsea preset con moduladores");

        let bay = apply_modulators_to_rack_slot(99, 7, &doc, "Tool");
        assert!(bay.open);
        assert_eq!(bay.slots.len(), 2);

        let lfo_slot = bay.slots.iter().find(|s| s.kind == "Classic LFO").expect("slot LFO");
        assert_eq!(lfo_slot.routes.len(), 1);
        assert_eq!(lfo_slot.routes[0].target, "gain");
        assert!((lfo_slot.routes[0].amount - 0.45).abs() < 1e-5);

        let macro_slot = bay.slots.iter().find(|s| s.kind == "Macro").expect("slot Macro");
        assert_eq!(macro_slot.routes.len(), 1);
        assert_eq!(macro_slot.routes[0].target, "width");
        assert!((macro_slot.routes[0].amount - (-0.30)).abs() < 1e-5);

        // Verifica que `hikaru_openmodulation` restauró el estado en vivo
        let live_bay = crate::views::hikaru_native_plugins::hikaru_openmodulation::bay::get_bay_state(99, 7);
        assert_eq!(live_bay.slots.len(), 2);
    }

    #[test]
    fn traduce_cadenas_anidadas_wet_fx_y_fx_layer() {
        let mut meta = BtWgWriter::new();
        meta.begin_obj(4);
        meta.raw_str("meta");
        meta.named_field_str("device_name", "Bit-8");
        meta.end_obj();

        let mut body = BtWgWriter::new();
        body.begin_obj(0x561);
        body.field_str(0x10, "DRIVE");
        body.field_f64(0x11, 0.4);

        // Cadena anidada "Wet FX" con un Comb y un Tool adentro
        body.field_obj_header(0x143, 0x800);
        body.field_str(0x01, "Wet FX");
        body.field_obj_list_start(0x144);

        body.begin_obj(0x561);
        body.field_str(0x01, "Comb");
        body.field_str(0x10, "FREQUENCY");
        body.field_f64(0x11, 440.0);
        body.end_obj();

        body.begin_obj(0x561);
        body.field_str(0x01, "Tool");
        body.field_str(0x10, "WIDTH");
        body.field_f64(0x11, 150.0);
        body.end_obj();

        body.obj_list_end();
        body.end_obj();
        body.end_obj();

        let bytes = wrap_btwg_0002(&meta.into_bytes(), &body.into_bytes());
        let doc = BtWgReader::parse_document(&bytes).expect("parsea Bit-8 con Wet FX");
        let tree = translate_device_tree(&doc, None);

        assert_eq!(tree.nested_chains.len(), 1);
        let wet = &tree.nested_chains[0];
        assert_eq!(wet.kind, NestedChainKind::WetFx);
        assert_eq!(wet.devices.len(), 2);

        match &wet.devices[0].translation {
            BwDeviceTranslation::Builtin {
                hikaru_slot_name, ..
            } => assert_eq!(hikaru_slot_name, "Hikaru OpenComb"),
            _ => panic!("Comb debe mapearse a Hikaru OpenComb"),
        }
        match &wet.devices[1].translation {
            BwDeviceTranslation::Builtin {
                hikaru_slot_name, ..
            } => assert_eq!(hikaru_slot_name, "Tool"),
            _ => panic!("Tool debe mapearse a Tool"),
        }
    }

    #[test]
    fn traduce_patch_grid_y_carga_desde_browser() {
        let mut meta = BtWgWriter::new();
        meta.begin_obj(4);
        meta.raw_str("meta");
        meta.named_field_str("device_name", "Poly Grid");
        meta.end_obj();

        let mut body = BtWgWriter::new();
        let root_id = body.begin_obj(0x561);
        assert_eq!(root_id, 1);

        body.field_obj_list_start(0x200);
        let osc_id = body.begin_obj(0x900);
        assert_eq!(osc_id, 2);
        body.field_str(0x01, "Wavetable");
        body.field_f64(0x02, 0.5);
        body.end_obj();

        let filt_id = body.begin_obj(0x900);
        assert_eq!(filt_id, 3);
        body.field_str(0x01, "SVF");
        body.field_f64(0x02, 0.8);
        body.field_ref(0x03, osc_id);
        body.end_obj();

        let out_id = body.begin_obj(0x900);
        assert_eq!(out_id, 4);
        body.field_str(0x01, "Audio Out");
        body.field_ref(0x03, filt_id);
        body.end_obj();

        body.obj_list_end();
        body.end_obj();

        let bytes = wrap_btwg_0002(&meta.into_bytes(), &body.into_bytes());
        let doc = BtWgReader::parse_document(&bytes).expect("parsea Poly Grid");
        let grid = translate_grid_patch(&doc);

        assert_eq!(grid.preset_name, "Poly Grid");
        assert_eq!(grid.nodes.len(), 3);
        assert_eq!(grid.nodes[0].kind, "Wavetable");
        assert_eq!(grid.nodes[1].kind, "Filter");
        assert_eq!(grid.nodes[2].kind, "Audio Out");
        assert_eq!(grid.cables.len(), 2);
        assert_eq!(grid.cables[0].from_node, 1);
        assert_eq!(grid.cables[0].to_node, 2);
        assert_eq!(grid.cables[1].from_node, 2);
        assert_eq!(grid.cables[1].to_node, 3);

        // Prueba carga unificada desde browser con archivo temporal .bwpreset
        let dir = std::env::temp_dir().join(format!("hikaru_bw_browser_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("crea temp dir");
        let preset_path = dir.join("Poly Grid.bwpreset");
        fs::write(&preset_path, &bytes).expect("escribe preset");

        assert!(is_bitwig_browser_artifact(&preset_path));
        let loaded = load_bitwig_browser_item(&preset_path, None).expect("carga desde browser");
        match loaded.translation {
            BwDeviceTranslation::Builtin {
                hikaru_slot_name, ..
            } => assert_eq!(hikaru_slot_name, "OpenGrid"),
            _ => panic!("Poly Grid debe mapearse a OpenGrid"),
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn bpr05_exporta_descubre_y_previsualiza_bwpreset_roundtrip() {
        let mut grid = OpenGridParams::default();
        grid.preset_name = "Acid Lead".to_string();
        grid.nodes = vec![
            OpenGridNodeData {
                id: 1,
                kind: "Wavetable".to_string(),
                pos: [120.0, 90.0],
                params: [0.4, 0.0, 0.0, 0.0, 0.0, 0.0],
                bypassed: false,
            },
            OpenGridNodeData {
                id: 2,
                kind: "Filter".to_string(),
                pos: [280.0, 90.0],
                params: [0.75, 0.25, 0.0, 0.0, 0.0, 0.0],
                bypassed: false,
            },
        ];
        grid.cables = vec![OpenGridCableData {
            from_node: 1,
            from_port: 0,
            to_node: 2,
            to_port: 0,
        }];

        let spec = BwPresetExportSpec {
            device_name: "Poly Grid".to_string(),
            preset_name: "Acid Lead".to_string(),
            creator: "Hikaru OpenLive".to_string(),
            category: "Synth".to_string(),
            comment: "Exported from test".to_string(),
            params: vec![("GAIN".to_string(), 0.75), ("CUTOFF".to_string(), 0.62)],
            modulators: DeviceModulatorBayParams {
                open: true,
                slots: vec![ModulatorSlotParams {
                    kind: "Macro".to_string(),
                    routes: vec![ModulatorRouteParams {
                        target: "cutoff".to_string(),
                        amount: 0.45,
                        out_idx: 0,
                    }],
                    ..ModulatorSlotParams::default()
                }],
            },
            grid: Some(grid),
            remote_labels: vec![(0, "Cutoff".to_string())],
        };

        let dir = std::env::temp_dir().join(format!("hikaru_bpr05_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("crea temp dir");
        let preset_path = dir.join("Acid Lead.bwpreset");

        export_bwpreset_file(&spec, &preset_path).expect("exporta .bwpreset");

        let discovered = discover_bitwig_presets(Some(&dir));
        let found = discovered
            .iter()
            .find(|p| p.path == preset_path)
            .expect("bwpreset visible en discover_bitwig_presets");
        assert_eq!(found.file_kind, ".bwpreset");
        assert_eq!(found.device_name, "Poly Grid");
        assert!(found.is_grid);

        let preview = inspect_bwpreset_preview(&preset_path);
        assert!(preview.error.is_none(), "preview error: {:?}", preview.error);
        assert_eq!(preview.device_name, "Poly Grid");
        assert_eq!(preview.preset_name, "Acid Lead");
        assert_eq!(preview.modulator_count, 1);
        assert_eq!(preview.grid_node_count, 2);
        assert_eq!(preview.grid_cable_count, 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn brc_exporta_descubre_y_carga_bwremotecontrols_roundtrip() {
        let spec = BwRemoteControlsSpec {
            device_name: Some("Polymer".to_string()),
            device_id: Some("uuid-polymer-123".to_string()),
            creator: Some("Hikaru OpenLive".to_string()),
            pages: vec![
                RemoteControlPageSpec {
                    name: "Perform".to_string(),
                    tags: vec!["Synth".to_string(), "Main".to_string()],
                    slots: vec![
                        RemoteControlSlotSpec {
                            slot_index: 0,
                            param_name: "Cutoff".to_string(),
                            param_id: Some("CONTENTS/FILTER/CUTOFF".to_string()),
                            midi_cc: Some(21),
                        },
                        RemoteControlSlotSpec {
                            slot_index: 1,
                            param_name: "Resonance".to_string(),
                            param_id: Some("CONTENTS/FILTER/RESO".to_string()),
                            midi_cc: Some(22),
                        },
                    ],
                },
                RemoteControlPageSpec {
                    name: "Envelope".to_string(),
                    tags: vec!["Env".to_string()],
                    slots: vec![RemoteControlSlotSpec {
                        slot_index: 0,
                        param_name: "Attack".to_string(),
                        param_id: Some("CONTENTS/ENV/ATTACK".to_string()),
                        midi_cc: Some(23),
                    }],
                },
            ],
        };

        let dir = std::env::temp_dir().join(format!("hikaru_brc_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("crea temp dir");
        let rc_path = dir.join("Polymer.bwremotecontrols");

        export_bwremotecontrols_file(&spec, &rc_path).expect("exporta .bwremotecontrols");
        assert!(is_bwremotecontrols_file(&rc_path));

        let parsed = load_bwremotecontrols_file(&rc_path).expect("carga .bwremotecontrols");
        assert_eq!(parsed.device_name.as_deref(), Some("Polymer"));
        assert_eq!(parsed.pages.len(), 2);
        assert_eq!(parsed.pages[0].name, "Perform");
        assert_eq!(parsed.pages[0].slots.len(), 2);
        assert_eq!(parsed.pages[0].slots[0].param_name, "Cutoff");
        assert_eq!(
            parsed.pages[0].slots[0].param_id.as_deref(),
            Some("CONTENTS/FILTER/CUTOFF")
        );
        assert_eq!(parsed.pages[0].slots[0].midi_cc, Some(21));
        assert_eq!(parsed.pages[1].name, "Envelope");

        let discovered = discover_bitwig_presets(Some(&dir));
        let found = discovered
            .iter()
            .find(|p| p.path == rc_path)
            .expect(".bwremotecontrols visible en discover_bitwig_presets");
        assert_eq!(found.file_kind, ".bwremotecontrols");
        assert_eq!(found.category, "Remote Controls");

        let _ = fs::remove_dir_all(&dir);
    }
}
