// Analizador y puente de ejecución clean-room para módulos `.nitro` / `.nitrobin`.
// GNU AGPLv3
// crates/hikaru_gui/src/project/nitro_shim.rs
//
// Implementa el soporte clean-room para `.nitro` y `.nitrobin`:
// - bnt-01: Análisis estructural de fuentes `.nitro`, bitcode `.nitrobin` y bundles `nitro-std`.
// - bnt-02: Tabla de mapeo de primitivas estándar (`nitro-std`) hacia `GridModuleKind`.
// - bnt-03: Puente de traducción a grafos `OpenGrid` (`OpenGridNodeData`) ejecutables en `hikaru_dsp`.

use std::path::Path;

use hikaru_dsp::synth::open_grid::{GridModuleKind, GridSignalKind};
use hikaru_project_file::OpenGridNodeData;

use crate::project::dawproject::read_zip_entries;

pub const NITRO_EXTENSION: &str = "nitro";
pub const NITROBIN_EXTENSION: &str = "nitrobin";

/// Devuelve `true` si la ruta corresponde a un archivo `.nitro` o `.nitrobin`.
pub fn is_nitro_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| {
            ext.eq_ignore_ascii_case(NITRO_EXTENSION)
                || ext.eq_ignore_ascii_case(NITROBIN_EXTENSION)
        })
        .unwrap_or(false)
}

/// Formato detectado para un artefacto Nitro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NitroFormatKind {
    /// Módulo textual / grafo declarativo `.nitro`.
    NitroSource,
    /// Bitcode precompilado `.nitrobin` (LLVM BC / wrapper, solo inspección de cabecera).
    NitroBitcode,
    /// Contenedor ZIP con módulos `.nitro` / `.nitrobin` (p. ej. `nitro-std`).
    NitroBundleZip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NitroPortDirection {
    Input,
    Output,
}

/// Puerto lógico declarado o inferido en un módulo Nitro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NitroPortSpec {
    pub name: String,
    pub direction: NitroPortDirection,
    pub signal_kind: GridSignalKind,
}

/// Primitiva DSP estándar (`nitro-std`) referenciada dentro de un módulo `.nitro`.
#[derive(Debug, Clone, PartialEq)]
pub struct NitroPrimitiveRef {
    pub symbol: String,
    pub mapped_kind: Option<GridModuleKind>,
    pub params: Vec<f32>,
}

/// Especificación clean-room extraída de un `.nitro`, `.nitrobin` o bundle ZIP.
#[derive(Debug, Clone, PartialEq)]
pub struct NitroModuleSpec {
    pub module_name: String,
    pub format: NitroFormatKind,
    pub size_bytes: usize,
    pub imports: Vec<String>,
    pub ports: Vec<NitroPortSpec>,
    pub primitives: Vec<NitroPrimitiveRef>,
    pub bundled_entries: Vec<String>,
    pub clean_room_note: String,
}

/// Resultado del puente hacia `OpenGrid` (`hikaru_dsp`).
#[derive(Debug, Clone)]
pub struct NitroOpenGridBridge {
    pub module_name: String,
    pub nodes: Vec<OpenGridNodeData>,
    pub cables: Vec<(u32, u8, u32, u8)>,
    pub mapped_primitives: Vec<(String, GridModuleKind)>,
    pub unmapped_primitives: Vec<String>,
}

const NITRO_STD_PRIMITIVES: &[(&str, GridModuleKind)] = &[
    ("smoother", GridModuleKind::LowPass),
    ("one_pole", GridModuleKind::LowPass),
    ("lowpass", GridModuleKind::LowPass),
    ("highpass", GridModuleKind::HighPass),
    ("svf", GridModuleKind::Svf),
    ("state_variable_filter", GridModuleKind::Svf),
    ("sallen_key", GridModuleKind::SallenKey),
    ("allpass", GridModuleKind::AllPass),
    ("comb", GridModuleKind::Comb),
    ("osc_sine", GridModuleKind::Sine),
    ("sine", GridModuleKind::Sine),
    ("osc_saw", GridModuleKind::Sawtooth),
    ("saw", GridModuleKind::Sawtooth),
    ("osc_tri", GridModuleKind::Triangle),
    ("triangle", GridModuleKind::Triangle),
    ("osc_pulse", GridModuleKind::Pulse),
    ("pulse", GridModuleKind::Pulse),
    ("square", GridModuleKind::Pulse),
    ("noise", GridModuleKind::Noise),
    ("phase_1", GridModuleKind::Phase1),
    ("lfo", GridModuleKind::Lfo),
    ("sh_lfo", GridModuleKind::ShLfo),
    ("wavetable_lfo", GridModuleKind::WavetableLfo),
    ("adsr", GridModuleKind::Adsr),
    ("ar", GridModuleKind::Ar),
    ("delay", GridModuleKind::Delay),
    ("chorus", GridModuleKind::ChorusPlus),
    ("flanger", GridModuleKind::FlangerPlus),
    ("phaser", GridModuleKind::AllPass),
    ("tanh", GridModuleKind::Saturator),
    ("saturator", GridModuleKind::Saturator),
    ("hard_clip", GridModuleKind::HardClip),
    ("clip", GridModuleKind::HardClip),
    ("gain", GridModuleKind::GainDb),
    ("gain_db", GridModuleKind::GainDb),
    ("gain_vol", GridModuleKind::GainVol),
    ("mix", GridModuleKind::Blend),
    ("blend", GridModuleKind::Blend),
    ("multiply", GridModuleKind::Multiply),
    ("add", GridModuleKind::Sum),
    ("sum", GridModuleKind::Sum),
    ("sample_hold", GridModuleKind::SampleHold),
    ("quantize", GridModuleKind::PitchQuantize),
    ("pitch_quantize", GridModuleKind::PitchQuantize),
    ("note_parameters", GridModuleKind::PitchIn),
    ("pitch_in", GridModuleKind::PitchIn),
    ("gate_in", GridModuleKind::GateIn),
    ("velocity_in", GridModuleKind::VelocityIn),
    ("phase_in", GridModuleKind::PhaseIn),
    ("audio_in", GridModuleKind::AudioIn),
    ("audio_out", GridModuleKind::AudioOut),
    ("modulator_out", GridModuleKind::ModulatorOut),
    ("bi_to_uni", GridModuleKind::BiToUni),
    ("uni_to_bi", GridModuleKind::UniToBi),
];

/// Resuelve un símbolo estándar de `nitro-std` (p. ej. `nitro::smoother` o `smoother.nitro`)
/// hacia su `GridModuleKind` nativo en `hikaru_dsp`.
pub fn map_nitro_primitive(symbol: &str) -> Option<GridModuleKind> {
    let norm = symbol
        .trim()
        .trim_end_matches(".nitro")
        .trim_end_matches(".nitrobin")
        .rsplit("::")
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .replace(['-', ' '], "_");

    for (key, kind) in NITRO_STD_PRIMITIVES {
        if *key == norm {
            return Some(*kind);
        }
    }
    if norm.contains("fft") || norm.contains("window_fill_buffer") {
        return Some(GridModuleKind::LowPass);
    }
    None
}

fn infer_signal_kind(port_name: &str) -> GridSignalKind {
    let low = port_name.to_ascii_lowercase();
    if low.contains("gate") || low.contains("trig") {
        GridSignalKind::Gate
    } else if low.contains("pitch") || low.contains("freq") || low.contains("note") {
        GridSignalKind::Pitch
    } else if low.contains("phase") {
        GridSignalKind::Phase
    } else if low.contains("mod") || low.contains("lfo") || low.contains("env") {
        GridSignalKind::Mod
    } else {
        GridSignalKind::Audio
    }
}

fn is_llvm_bitcode(bytes: &[u8]) -> bool {
    bytes.starts_with(b"BC\xc0\xde") || bytes.starts_with(&[0xde, 0xc0, 0x17, 0x0b])
}

/// Analiza en modo *clean-room* bytes de `.nitro`, `.nitrobin` o un bundle ZIP `nitro-std`.
pub fn inspect_nitro_bytes(bytes: &[u8], fallback_name: &str) -> Result<NitroModuleSpec, String> {
    if bytes.is_empty() {
        return Err("el archivo .nitro/.nitrobin está vacío".to_string());
    }

    // 1. Bundle ZIP (`nitro-std` / `nitro-image`)
    if bytes.starts_with(b"PK\x03\x04") {
        let entries = read_zip_entries(bytes)?;
        let mut bundled_entries: Vec<String> = entries
            .keys()
            .filter(|k| {
                let low = k.to_ascii_lowercase();
                low.ends_with(".nitro") || low.ends_with(".nitrobin")
            })
            .cloned()
            .collect();
        bundled_entries.sort();

        let mut primitives = Vec::new();
        for entry in &bundled_entries {
            let sym = Path::new(entry)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(entry)
                .to_string();
            let mapped = map_nitro_primitive(&sym);
            primitives.push(NitroPrimitiveRef {
                symbol: sym,
                mapped_kind: mapped,
                params: Vec::new(),
            });
        }

        return Ok(NitroModuleSpec {
            module_name: fallback_name.to_string(),
            format: NitroFormatKind::NitroBundleZip,
            size_bytes: bytes.len(),
            imports: Vec::new(),
            ports: vec![
                NitroPortSpec {
                    name: "in".to_string(),
                    direction: NitroPortDirection::Input,
                    signal_kind: GridSignalKind::Audio,
                },
                NitroPortSpec {
                    name: "out".to_string(),
                    direction: NitroPortDirection::Output,
                    signal_kind: GridSignalKind::Audio,
                },
            ],
            primitives,
            bundled_entries,
            clean_room_note: "Bundle ZIP Nitro inspeccionado en modo clean-room; primitivas mapeadas a OpenGrid.".to_string(),
        });
    }

    // 2. Bitcode `.nitrobin`
    if is_llvm_bitcode(bytes) {
        let inferred_kind = map_nitro_primitive(fallback_name);
        let primitives = vec![NitroPrimitiveRef {
            symbol: fallback_name.to_string(),
            mapped_kind: inferred_kind,
            params: Vec::new(),
        }];
        return Ok(NitroModuleSpec {
            module_name: fallback_name.to_string(),
            format: NitroFormatKind::NitroBitcode,
            size_bytes: bytes.len(),
            imports: Vec::new(),
            ports: vec![
                NitroPortSpec {
                    name: "in".to_string(),
                    direction: NitroPortDirection::Input,
                    signal_kind: GridSignalKind::Audio,
                },
                NitroPortSpec {
                    name: "out".to_string(),
                    direction: NitroPortDirection::Output,
                    signal_kind: GridSignalKind::Audio,
                },
            ],
            primitives,
            bundled_entries: Vec::new(),
            clean_room_note: "Cabecera LLVM Bitcode (.nitrobin) detectada; no se ejecuta código propietario, se resuelve por contrato simbólico clean-room.".to_string(),
        });
    }

    // 3. Fuente / grafo textual `.nitro` (clean-room)
    let text = String::from_utf8_lossy(bytes);
    let mut module_name = fallback_name.to_string();
    let mut imports = Vec::new();
    let mut ports = Vec::new();
    let mut primitives = Vec::new();

    for raw_line in text.lines() {
        let line = raw_line.split("//").next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("module ") {
            let name = line["module ".len()..].trim().trim_end_matches('{').trim();
            if !name.is_empty() {
                module_name = name.to_string();
            }
            let _ = rest;
        } else if lower.starts_with("import ") || lower.starts_with("use ") {
            let imp = line
                .split_whitespace()
                .nth(1)
                .unwrap_or("")
                .trim_end_matches(';');
            if !imp.is_empty() {
                imports.push(imp.to_string());
                if let Some(mapped) = map_nitro_primitive(imp) {
                    primitives.push(NitroPrimitiveRef {
                        symbol: imp.to_string(),
                        mapped_kind: Some(mapped),
                        params: Vec::new(),
                    });
                }
            }
        } else if lower.starts_with("in ") || lower.starts_with("input ") {
            let pname = line
                .split_whitespace()
                .nth(1)
                .unwrap_or("in")
                .trim_end_matches([':', ';', ',']);
            ports.push(NitroPortSpec {
                name: pname.to_string(),
                direction: NitroPortDirection::Input,
                signal_kind: infer_signal_kind(pname),
            });
        } else if lower.starts_with("out ") || lower.starts_with("output ") {
            let pname = line
                .split_whitespace()
                .nth(1)
                .unwrap_or("out")
                .trim_end_matches([':', ';', ',']);
            ports.push(NitroPortSpec {
                name: pname.to_string(),
                direction: NitroPortDirection::Output,
                signal_kind: infer_signal_kind(pname),
            });
        } else {
            // Detectar primitivas estándar en llamadas o nodos: `svf(...)`, `node smoother`, etc.
            for token in line.split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != ':') {
                if token.is_empty() {
                    continue;
                }
                let tok_low = token.to_ascii_lowercase();
                if matches!(
                    tok_low.as_str(),
                    "audio_in" | "audio_out" | "pitch_in" | "gate_in" | "velocity_in" | "phase_in" | "modulator_out"
                ) {
                    continue;
                }
                if let Some(mapped) = map_nitro_primitive(token) {
                    if !primitives.iter().any(|p| p.symbol == token) {
                        primitives.push(NitroPrimitiveRef {
                            symbol: token.to_string(),
                            mapped_kind: Some(mapped),
                            params: Vec::new(),
                        });
                    }
                }
            }
        }
    }

    if ports.is_empty() {
        ports.push(NitroPortSpec {
            name: "in".to_string(),
            direction: NitroPortDirection::Input,
            signal_kind: GridSignalKind::Audio,
        });
        ports.push(NitroPortSpec {
            name: "out".to_string(),
            direction: NitroPortDirection::Output,
            signal_kind: GridSignalKind::Audio,
        });
    }

    if primitives.is_empty() {
        primitives.push(NitroPrimitiveRef {
            symbol: module_name.clone(),
            mapped_kind: map_nitro_primitive(&module_name),
            params: Vec::new(),
        });
    }

    Ok(NitroModuleSpec {
        module_name,
        format: NitroFormatKind::NitroSource,
        size_bytes: bytes.len(),
        imports,
        ports,
        primitives,
        bundled_entries: Vec::new(),
        clean_room_note: "Módulo .nitro analizado en modo clean-room y listo para puente OpenGrid.".to_string(),
    })
}

/// Carga e inspecciona un archivo `.nitro` o `.nitrobin` desde disco.
pub fn load_nitro_file(path: &Path) -> Result<NitroModuleSpec, String> {
    let bytes = std::fs::read(path)
        .map_err(|e| format!("no se pudo leer {}: {e}", path.display()))?;
    let fallback_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("NitroModule");
    inspect_nitro_bytes(&bytes, fallback_name)
}

/// Construye un grafo ejecutable de `OpenGrid` (`OpenGridNodeData` + cables) a partir de
/// un `NitroModuleSpec` usando el mapeo de primitivas `nitro-std` -> `hikaru_dsp`.
pub fn translate_nitro_to_opengrid(spec: &NitroModuleSpec) -> NitroOpenGridBridge {
    let mut nodes: Vec<OpenGridNodeData> = Vec::new();
    let mut cables: Vec<(u32, u8, u32, u8)> = Vec::new();
    let mut mapped_primitives = Vec::new();
    let mut unmapped_primitives = Vec::new();

    let mut next_id: u32 = 1;
    let mut x = 80.0f32;
    let y = 140.0f32;

    // Nodo de entrada (AudioIn o PitchIn según puertos)
    let in_kind = if spec
        .ports
        .iter()
        .any(|p| p.direction == NitroPortDirection::Input && p.signal_kind == GridSignalKind::Pitch)
    {
        GridModuleKind::PitchIn
    } else {
        GridModuleKind::AudioIn
    };
    let in_id = next_id;
    next_id += 1;
    nodes.push(OpenGridNodeData {
        id: in_id,
        kind: in_kind.display_name().to_string(),
        pos: [x, y],
        params: [0.5, 0.0, 0.0, 0.0, 0.0, 0.0],
        bypassed: false,
    });
    x += 180.0;

    let mut prev_id = in_id;
    for prim in &spec.primitives {
        let kind = match prim.mapped_kind {
            Some(k) => {
                mapped_primitives.push((prim.symbol.clone(), k));
                k
            }
            None => {
                unmapped_primitives.push(prim.symbol.clone());
                GridModuleKind::GainDb
            }
        };
        let nid = next_id;
        next_id += 1;
        nodes.push(OpenGridNodeData {
            id: nid,
            kind: kind.display_name().to_string(),
            pos: [x, y],
            params: [0.5, 0.0, 0.0, 0.0, 0.0, 0.0],
            bypassed: false,
        });
        cables.push((prev_id, 0, nid, 0));
        prev_id = nid;
        x += 180.0;
    }

    let out_id = next_id;
    nodes.push(OpenGridNodeData {
        id: out_id,
        kind: GridModuleKind::AudioOut.display_name().to_string(),
        pos: [x, y],
        params: [0.5, 0.0, 0.0, 0.0, 0.0, 0.0],
        bypassed: false,
    });
    cables.push((prev_id, 0, out_id, 0));

    NitroOpenGridBridge {
        module_name: spec.module_name.clone(),
        nodes,
        cables,
        mapped_primitives,
        unmapped_primitives,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::dawproject::build_zip_store;

    #[test]
    fn bnt01_analyzes_nitro_source_bitcode_and_zip_bundle() {
        let src = br#"
module SmootherFilter {
  import nitro::smoother;
  in audio_in
  out audio_out
  svf(audio_in)
}
"#;
        let spec = inspect_nitro_bytes(src, "SmootherFilter").expect("inspect .nitro source");
        assert_eq!(spec.format, NitroFormatKind::NitroSource);
        assert_eq!(spec.module_name, "SmootherFilter");
        assert_eq!(spec.ports.len(), 2);
        assert!(spec.primitives.iter().any(|p| p.symbol.contains("smoother")));

        let bc = b"BC\xc0\xde\x21\x0c\x00\x00";
        let bc_spec = inspect_nitro_bytes(bc, "svf").expect("inspect .nitrobin");
        assert_eq!(bc_spec.format, NitroFormatKind::NitroBitcode);

        let zip = build_zip_store(&[
            ("smoother.nitro", b"module smoother {}"),
            ("nonlinear/tanh.nitrobin", b"BC\xc0\xde"),
        ]);
        let zip_spec = inspect_nitro_bytes(&zip, "nitro-std").expect("inspect nitro-std zip");
        assert_eq!(zip_spec.format, NitroFormatKind::NitroBundleZip);
        assert_eq!(zip_spec.bundled_entries.len(), 2);
    }

    #[test]
    fn bnt02_maps_nitro_std_primitives_to_opengrid_kinds() {
        assert_eq!(map_nitro_primitive("smoother.nitro"), Some(GridModuleKind::LowPass));
        assert_eq!(map_nitro_primitive("nitro::svf"), Some(GridModuleKind::Svf));
        assert_eq!(map_nitro_primitive("osc_sine"), Some(GridModuleKind::Sine));
        assert_eq!(map_nitro_primitive("adsr"), Some(GridModuleKind::Adsr));
        assert_eq!(map_nitro_primitive("note_parameters.nitro"), Some(GridModuleKind::PitchIn));
        assert_eq!(map_nitro_primitive("unknown_custom_op"), None);
    }

    #[test]
    fn bnt03_bridges_nitro_module_to_opengrid_graph() {
        let src = br#"
module AcidChain {
  in pitch_in
  out audio_out
  osc_saw(pitch_in)
  svf()
  tanh()
}
"#;
        let spec = inspect_nitro_bytes(src, "AcidChain").expect("inspect AcidChain");
        let bridge = translate_nitro_to_opengrid(&spec);
        assert_eq!(bridge.module_name, "AcidChain");
        // Audio/Pitch In + 3 primitives + Audio Out = 5 nodes, 4 cables
        assert_eq!(bridge.nodes.len(), 5);
        assert_eq!(bridge.cables.len(), 4);
        assert!(bridge.unmapped_primitives.is_empty());
    }
}
