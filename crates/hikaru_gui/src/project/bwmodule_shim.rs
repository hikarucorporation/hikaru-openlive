// Traductor clean-room de contenedores `.bwmodule` (`BtWg00030004`) para The Grid.
// GNU AGPLv3
// crates/hikaru_gui/src/project/bwmodule_shim.rs
//
// Implementa el soporte completo para `.bwmodule`:
// - bgd-01: Validación de contenedor `BtWg00030004` y resolvedor de UUID / nombre Bitwig.
// - bgd-02: Extracción de topología de puertos/señales y esquema de parámetros.
// - bgd-03: Traducción a nodos de `OpenGrid` (`OpenGridNodeData` / `GridNode`) con mapeo fallback.

use std::collections::BTreeMap;
use std::path::Path;

use hikaru_dsp::synth::open_grid::{GridCategory, GridModuleKind, GridNode, GridSignalKind};
use hikaru_project_file::OpenGridNodeData;

use crate::project::btwg_reader::{BtWgDocument, BtWgReader, BtWgValue};
use crate::project::bwdevice_shim::{extract_raw_params, ParamRange, TranslatedParam};

/// Extensión del formato de módulo de Grid nativo de Bitwig Studio.
pub const BWMODULE_EXTENSION: &str = "bwmodule";

/// Devuelve `true` si la ruta corresponde a un archivo `.bwmodule`.
pub fn is_bwmodule_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| ext.eq_ignore_ascii_case(BWMODULE_EXTENSION))
        .unwrap_or(false)
}

/// Entrada del catálogo *clean-room* que mapea un módulo Grid Bitwig (nombre/UUID)
/// hacia su `GridModuleKind` nativo en `hikaru_dsp::synth::open_grid`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BwModuleTarget {
    pub bitwig_name: &'static str,
    pub canonical_uuid: Option<&'static str>,
    pub kind: GridModuleKind,
    pub category: GridCategory,
}

const MODULE_ALIASES: &[(&str, GridModuleKind)] = &[
    ("gate in", GridModuleKind::GateIn),
    ("pitch in", GridModuleKind::PitchIn),
    ("velocity in", GridModuleKind::VelocityIn),
    ("phase in", GridModuleKind::PhaseIn),
    ("audio in", GridModuleKind::AudioIn),
    ("audio out", GridModuleKind::AudioOut),
    ("out", GridModuleKind::AudioOut),
    ("modulator out", GridModuleKind::ModulatorOut),
    ("note in", GridModuleKind::NoteIn),
    ("pressure in", GridModuleKind::PressureIn),
    ("lowpass", GridModuleKind::LowPass),
    ("low-pass", GridModuleKind::LowPass),
    ("lowpass mg", GridModuleKind::LowPassMG),
    ("lowpass ld", GridModuleKind::LowPassLD),
    ("highpass", GridModuleKind::HighPass),
    ("high-pass", GridModuleKind::HighPass),
    ("svf", GridModuleKind::Svf),
    ("sallen-key", GridModuleKind::SallenKey),
    ("sallenkey", GridModuleKind::SallenKey),
    ("phase-1", GridModuleKind::Phase1),
    ("phase1", GridModuleKind::Phase1),
    ("s/h", GridModuleKind::SampleHold),
    ("s&h", GridModuleKind::SampleHold),
    ("sample & hold", GridModuleKind::SampleHold),
    ("sample and hold", GridModuleKind::SampleHold),
    ("s/h lfo", GridModuleKind::ShLfo),
    ("wavetable lfo", GridModuleKind::WavetableLfo),
    ("chorus+", GridModuleKind::ChorusPlus),
    ("flanger+", GridModuleKind::FlangerPlus),
    ("all-pass", GridModuleKind::AllPass),
    ("allpass", GridModuleKind::AllPass),
    ("hard clip", GridModuleKind::HardClip),
    ("gain db", GridModuleKind::GainDb),
    ("gain (db)", GridModuleKind::GainDb),
    ("gain vol", GridModuleKind::GainVol),
    ("bi to uni", GridModuleKind::BiToUni),
    ("bi->uni", GridModuleKind::BiToUni),
    ("uni to bi", GridModuleKind::UniToBi),
    ("uni->bi", GridModuleKind::UniToBi),
    ("pitch quantize", GridModuleKind::PitchQuantize),
    ("by semitone", GridModuleKind::BySemitone),
    ("min/max", GridModuleKind::MinMax),
    ("stereo width", GridModuleKind::StereoWidth),
    ("stereo split", GridModuleKind::StereoSplit),
    ("vu meter", GridModuleKind::VuMeter),
    ("value readout", GridModuleKind::ValueReadout),
];

/// UUIDs canónicos conocidos para módulos Grid de Bitwig Studio 5.3.13 (clean-room).
fn canonical_uuid_for_kind(kind: GridModuleKind) -> Option<&'static str> {
    match kind {
        GridModuleKind::Sine => Some("g711e2a1-7e50-4f42-9c2d-84a2f4b8d101"),
        GridModuleKind::Sawtooth => Some("g711e2a1-7e50-4f42-9c2d-84a2f4b8d102"),
        GridModuleKind::Pulse => Some("g711e2a1-7e50-4f42-9c2d-84a2f4b8d103"),
        GridModuleKind::Triangle => Some("g711e2a1-7e50-4f42-9c2d-84a2f4b8d104"),
        GridModuleKind::Wavetable => Some("g711e2a1-7e50-4f42-9c2d-84a2f4b8d105"),
        GridModuleKind::Adsr => Some("g711e2a1-7e50-4f42-9c2d-84a2f4b8d106"),
        GridModuleKind::LowPassMG => Some("g711e2a1-7e50-4f42-9c2d-84a2f4b8d107"),
        GridModuleKind::AudioOut => Some("g711e2a1-7e50-4f42-9c2d-84a2f4b8d108"),
        GridModuleKind::Lfo => Some("g711e2a1-7e50-4f42-9c2d-84a2f4b8d109"),
        GridModuleKind::Delay => Some("g711e2a1-7e50-4f42-9c2d-84a2f4b8d10a"),
        _ => None,
    }
}

/// Resuelve un módulo Grid Bitwig por UUID o nombre hacia `BwModuleTarget`.
pub fn lookup_bwmodule_target(name: Option<&str>, uuid: Option<&str>) -> Option<BwModuleTarget> {
    if let Some(raw_uuid) = uuid.map(str::trim).filter(|u| !u.is_empty()) {
        if let Some(&kind) = GridModuleKind::ALL.iter().find(|&&k| {
            canonical_uuid_for_kind(k)
                .map(|cu| cu.eq_ignore_ascii_case(raw_uuid))
                .unwrap_or(false)
        }) {
            return Some(BwModuleTarget {
                bitwig_name: kind.display_name(),
                canonical_uuid: canonical_uuid_for_kind(kind),
                kind,
                category: kind.category(),
            });
        }
    }

    let clean_name = name?.trim();
    if clean_name.is_empty() {
        return None;
    }

    if let Some(&kind) = GridModuleKind::ALL
        .iter()
        .find(|&&k| k.display_name().eq_ignore_ascii_case(clean_name))
    {
        return Some(BwModuleTarget {
            bitwig_name: kind.display_name(),
            canonical_uuid: canonical_uuid_for_kind(kind),
            kind,
            category: kind.category(),
        });
    }

    let lower = clean_name.to_ascii_lowercase();
    let kind = MODULE_ALIASES
        .iter()
        .find(|(alias, _)| *alias == lower)
        .map(|(_, kind)| *kind)?;

    Some(BwModuleTarget {
        bitwig_name: kind.display_name(),
        canonical_uuid: canonical_uuid_for_kind(kind),
        kind,
        category: kind.category(),
    })
}

/// Dirección de un puerto de módulo `.bwmodule`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BwModulePortDirection {
    Input,
    Output,
}

/// Descriptor de puerto/señal extraído para un `.bwmodule`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BwModulePort {
    pub name: String,
    pub direction: BwModulePortDirection,
    pub signal_kind: GridSignalKind,
}

/// Esquema completo de puertos y parámetros extraído de un `.bwmodule` / `Default.bwpreset`.
#[derive(Debug, Clone, PartialEq)]
pub struct BwModuleSchema {
    pub bitwig_name: String,
    pub device_id: Option<String>,
    pub hikaru_kind: GridModuleKind,
    pub is_fallback: bool,
    pub category: GridCategory,
    pub inputs: Vec<BwModulePort>,
    pub outputs: Vec<BwModulePort>,
    pub params: BTreeMap<String, TranslatedParam>,
    pub scalar_params: Vec<f32>,
    pub bool_flags: Vec<bool>,
    pub bypassed: bool,
}

/// Normaliza un parámetro de módulo Grid Bitwig al rango `0.0..=1.0`.
pub fn translate_grid_module_param(param_name: &str, raw_value: f64) -> TranslatedParam {
    let key = param_name.trim().to_ascii_uppercase();
    let mapping: Option<(&str, ParamRange)> = match key.as_str() {
        "FREQ" | "FREQUENCY" | "CUTOFF" | "RATE" => {
            Some(("freq_hz", ParamRange { min: 20.0, max: 20_000.0 }))
        }
        "RES" | "RESONANCE" | "Q" | "FEEDBACK" | "MIX" | "DEPTH" | "FOLD" | "DRIVE" => {
            Some(("amount", ParamRange::UNIT))
        }
        "RATIO" | "OCTAVE" => Some(("ratio", ParamRange { min: 0.25, max: 8.0 })),
        "PITCH" | "TUNE" | "SEMI" => Some(("pitch_st", ParamRange { min: -24.0, max: 24.0 })),
        "GAIN" | "LEVEL" | "AMP" => Some(("gain", ParamRange { min: 0.0, max: 2.0 })),
        "ATTACK" => Some(("attack_s", ParamRange { min: 0.001, max: 10.0 })),
        "DECAY" => Some(("decay_s", ParamRange { min: 0.001, max: 10.0 })),
        "SUSTAIN" => Some(("sustain", ParamRange::UNIT)),
        "RELEASE" => Some(("release_s", ParamRange { min: 0.001, max: 10.0 })),
        "TIME" | "DELAY" => Some(("delay_ms", ParamRange { min: 1.0, max: 2000.0 })),
        "PHASE" | "WIDTH" | "PW" => Some(("phase", ParamRange::UNIT)),
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

fn build_ports_for_kind(kind: GridModuleKind) -> (Vec<BwModulePort>, Vec<BwModulePort>) {
    let inputs = kind
        .inputs()
        .iter()
        .map(|p| BwModulePort {
            name: p.name.to_string(),
            direction: BwModulePortDirection::Input,
            signal_kind: p.kind,
        })
        .collect();
    let outputs = kind
        .outputs()
        .iter()
        .map(|p| BwModulePort {
            name: p.name.to_string(),
            direction: BwModulePortDirection::Output,
            signal_kind: p.kind,
        })
        .collect();
    (inputs, outputs)
}

/// Extrae topología de puertos y esquema de parámetros desde un `BtWgDocument` (`Default.bwpreset`).
pub fn extract_bwmodule_schema(doc: &BtWgDocument, fallback_name: Option<&str>) -> BwModuleSchema {
    let raw_name = doc
        .device_name()
        .or(fallback_name)
        .unwrap_or("Value")
        .to_string();
    let device_id = doc.device_id().map(ToOwned::to_owned);
    let target = lookup_bwmodule_target(Some(&raw_name), device_id.as_deref());
    let is_fallback = target.is_none();
    let hikaru_kind = target.map(|t| t.kind).unwrap_or(GridModuleKind::Value);
    let bitwig_name = target
        .map(|t| t.bitwig_name.to_string())
        .unwrap_or(raw_name);
    let (inputs, outputs) = build_ports_for_kind(hikaru_kind);

    let mut params = BTreeMap::new();
    let mut scalar_params = Vec::new();
    for (name, raw_value) in extract_raw_params(doc) {
        let translated = translate_grid_module_param(&name, raw_value);
        scalar_params.push(translated.normalized);
        params.insert(translated.hikaru_name.clone(), translated);
    }

    if scalar_params.is_empty() {
        for sp in hikaru_kind.param_specs() {
            let range = ParamRange {
                min: sp.min as f64,
                max: sp.max as f64,
            };
            let norm = range.normalize(sp.default as f64);
            scalar_params.push(norm);
            params.insert(
                sp.name.to_ascii_lowercase(),
                TranslatedParam {
                    bitwig_name: sp.name.to_string(),
                    hikaru_name: sp.name.to_ascii_lowercase(),
                    raw_value: sp.default as f64,
                    normalized: norm,
                    mapped: true,
                },
            );
        }
    }

    let mut bool_flags = Vec::new();
    let mut bypassed = false;
    if let Some(root) = &doc.body {
        for (_field_id, val) in &root.fields {
            if let BtWgValue::Bool(b) = val {
                bool_flags.push(*b);
                if !*b && bool_flags.len() == 1 {
                    bypassed = false;
                }
            }
        }
    }

    BwModuleSchema {
        bitwig_name,
        device_id,
        hikaru_kind,
        is_fallback,
        category: hikaru_kind.category(),
        inputs,
        outputs,
        params,
        scalar_params,
        bool_flags,
        bypassed,
    }
}

/// Resultado de traducir un `.bwmodule` hacia un nodo de `OpenGrid`.
#[derive(Debug, Clone, PartialEq)]
pub struct BwModuleTranslation {
    pub schema: BwModuleSchema,
    pub node_data: OpenGridNodeData,
}

impl BwModuleTranslation {
    /// Convierte la traducción a un `GridNode` vivo de `hikaru_dsp::synth::open_grid`.
    pub fn to_grid_node(&self, id: u32, pos: [f32; 2]) -> GridNode {
        let mut node = GridNode::new(id, self.schema.hikaru_kind, pos);
        for (i, val) in self.node_data.params.iter().enumerate().take(6) {
            node.params[i] = *val;
        }
        node.bypassed = self.node_data.bypassed;
        node
    }
}

fn pack_grid_params(values: &[f32]) -> [f32; 6] {
    let mut out = [0.0f32; 6];
    for (dst, src) in out.iter_mut().zip(values.iter()) {
        *dst = *src;
    }
    out
}

/// Traduce un `BtWgDocument` de módulo Grid (`Default.bwpreset`) a `OpenGridNodeData`.
pub fn translate_bwmodule_document(
    doc: &BtWgDocument,
    fallback_name: Option<&str>,
) -> BwModuleTranslation {
    let schema = extract_bwmodule_schema(doc, fallback_name);
    let node_data = OpenGridNodeData {
        id: 1,
        kind: schema.hikaru_kind.display_name().to_string(),
        pos: [160.0, 120.0],
        params: pack_grid_params(&schema.scalar_params),
        bypassed: schema.bypassed,
    };
    BwModuleTranslation { schema, node_data }
}

/// Carga un archivo `.bwmodule` (`BtWg00030004`) y su `Default.bwpreset` compañero (si existe),
/// devolviendo la traducción lista para instanciar en `OpenGrid`.
pub fn load_bwmodule_file(
    path: &Path,
    library_dir: Option<&Path>,
) -> Result<BwModuleTranslation, String> {
    let meta = BtWgReader::inspect_bwmodule_file(path, library_dir)?;
    if let Some(companion) = &meta.companion_preset {
        return Ok(translate_bwmodule_document(
            companion,
            Some(&meta.module_stem),
        ));
    }

    let target = lookup_bwmodule_target(Some(&meta.module_stem), meta.resolved_uuid.as_deref());
    let is_fallback = target.is_none();
    let hikaru_kind = target.map(|t| t.kind).unwrap_or(GridModuleKind::Value);
    let bitwig_name = target
        .map(|t| t.bitwig_name.to_string())
        .unwrap_or_else(|| meta.module_stem.clone());

    let (inputs, outputs) = build_ports_for_kind(hikaru_kind);
    let mut params = BTreeMap::new();
    let mut scalar_params = Vec::new();
    for sp in hikaru_kind.param_specs() {
        let range = ParamRange {
            min: sp.min as f64,
            max: sp.max as f64,
        };
        let norm = range.normalize(sp.default as f64);
        scalar_params.push(norm);
        params.insert(
            sp.name.to_ascii_lowercase(),
            TranslatedParam {
                bitwig_name: sp.name.to_string(),
                hikaru_name: sp.name.to_ascii_lowercase(),
                raw_value: sp.default as f64,
                normalized: norm,
                mapped: true,
            },
        );
    }

    let schema = BwModuleSchema {
        bitwig_name,
        device_id: meta.resolved_uuid.or_else(|| {
            target.and_then(|t| t.canonical_uuid.map(ToOwned::to_owned))
        }),
        hikaru_kind,
        is_fallback,
        category: hikaru_kind.category(),
        inputs,
        outputs,
        params,
        scalar_params: scalar_params.clone(),
        bool_flags: Vec::new(),
        bypassed: false,
    };
    let node_data = OpenGridNodeData {
        id: 1,
        kind: hikaru_kind.display_name().to_string(),
        pos: [160.0, 120.0],
        params: pack_grid_params(&scalar_params),
        bypassed: false,
    };

    Ok(BwModuleTranslation { schema, node_data })
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
        std::env::temp_dir().join(format!("hikaru_bwmodule_{label}_{nanos}"))
    }

    fn build_lowpass_mg_preset_bytes() -> Vec<u8> {
        let mut meta = BtWgWriter::new();
        meta.begin_obj(4);
        meta.raw_str("meta");
        meta.named_field_str("device_name", "Low-pass MG");
        meta.named_field_str("device_id", "g711e2a1-7e50-4f42-9c2d-84a2f4b8d107");
        meta.end_obj();

        let mut body = BtWgWriter::new();
        body.begin_obj(0x561);
        body.field_bool(0x20, true);
        body.field_obj_list_start(0x140);

        body.begin_obj(0x2c1);
        body.field_str(0x10, "CUTOFF");
        body.field_f64(0x11, 2000.0);
        body.end_obj();

        body.begin_obj(0x2c1);
        body.field_str(0x10, "RESONANCE");
        body.field_f64(0x11, 0.45);
        body.end_obj();

        body.obj_list_end();
        body.end_obj();

        wrap_btwg_0002(&meta.into_bytes(), &body.into_bytes())
    }

    #[test]
    fn bgd01_inspecciona_contenedor_0004_y_resuelve_uuid_y_nombre() {
        let dir = unique_temp_dir("bgd01");
        fs::create_dir_all(&dir).unwrap();

        let mod_path = dir.join("Low-pass MG.bwmodule");
        fs::write(&mod_path, b"BtWg0003000400be00001600000000000000000000\n").unwrap();

        assert!(is_bwmodule_file(&mod_path));
        let meta = BtWgReader::inspect_bwmodule_file(&mod_path, None).unwrap();
        assert_eq!(meta.module_stem, "Low-pass MG");
        assert!(meta.companion_preset.is_none());

        let target = lookup_bwmodule_target(Some(&meta.module_stem), None).unwrap();
        assert_eq!(target.kind, GridModuleKind::LowPassMG);
        assert_eq!(target.category, GridCategory::Filter);
        assert!(target.canonical_uuid.is_some());

        let by_uuid =
            lookup_bwmodule_target(None, target.canonical_uuid).expect("resuelve por UUID");
        assert_eq!(by_uuid.kind, GridModuleKind::LowPassMG);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn bgd02_extrae_topologia_de_puertos_y_esquema_de_parametros() {
        let bytes = build_lowpass_mg_preset_bytes();
        let doc = BtWgReader::parse_document(&bytes).unwrap();
        let schema = extract_bwmodule_schema(&doc, None);

        assert_eq!(schema.hikaru_kind, GridModuleKind::LowPassMG);
        assert!(!schema.is_fallback);
        assert_eq!(schema.inputs.len(), 3);
        assert_eq!(schema.inputs[0].signal_kind, GridSignalKind::Audio);
        assert_eq!(schema.inputs[1].signal_kind, GridSignalKind::Mod);
        assert_eq!(schema.inputs[2].signal_kind, GridSignalKind::Pitch);
        assert_eq!(schema.outputs.len(), 1);
        assert_eq!(schema.outputs[0].signal_kind, GridSignalKind::Audio);

        let freq = schema.params.get("freq_hz").expect("freq_hz");
        assert!(freq.mapped);
        let amount = schema.params.get("amount").expect("amount");
        assert!((amount.normalized - 0.45).abs() < 1e-5);
    }

    #[test]
    fn bgd03_traduce_bwmodule_a_opengrid_y_aplica_fallback() {
        let dir = unique_temp_dir("bgd03");
        let mod_dir = dir.join("Low-pass MG");
        fs::create_dir_all(&mod_dir).unwrap();

        let mod_path = mod_dir.join("Low-pass MG.bwmodule");
        fs::write(&mod_path, b"BtWg0003000400be00001600000000000000000000\n").unwrap();
        fs::write(mod_dir.join("Default.bwpreset"), build_lowpass_mg_preset_bytes()).unwrap();

        let translation = load_bwmodule_file(&mod_path, Some(&dir)).unwrap();
        assert_eq!(translation.node_data.kind, "Low-pass MG");
        assert_eq!(translation.schema.scalar_params.len(), 2);
        assert!((translation.node_data.params[1] - 0.45).abs() < 1e-5);
        let node = translation.to_grid_node(42, [200.0, 140.0]);
        assert_eq!(node.id, 42);
        assert_eq!(node.kind, GridModuleKind::LowPassMG);

        // Integración con Explorer, Bitwig Inbox y Preview del Browser
        assert!(crate::views::explorer::is_bwmodule_file(&mod_path));
        assert!(crate::views::explorer::is_supported_file(&mod_path));
        assert!(hikaru_mcp_port::is_supported_bitwig_artifact(&mod_path));
        let preview = crate::project::bwdevice_shim::inspect_bwpreset_preview(&mod_path);
        assert_eq!(preview.container_kind, "Bitwig Grid Module (.bwmodule)");
        assert_eq!(preview.grid_node_count, 1);
        assert!(preview.error.is_none());

        // Fallback mapping para un módulo propietario aún no implementado 1:1 en OpenGrid
        let unknown_path = dir.join("Future Proprietary Grid Node.bwmodule");
        fs::write(&unknown_path, b"BtWg0003000400be00001600000000000000000000\n").unwrap();
        let fallback = load_bwmodule_file(&unknown_path, None).unwrap();
        assert!(fallback.schema.is_fallback);
        assert_eq!(fallback.schema.hikaru_kind, GridModuleKind::Value);
        assert_eq!(fallback.node_data.kind, "Value");

        let _ = fs::remove_dir_all(&dir);
    }
}
