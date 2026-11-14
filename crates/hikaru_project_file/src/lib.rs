// Primitivas compartidas por los formatos de proyecto de Hikaru.
// SPDX-License-Identifier: LGPL-3.0-or-later
// crates/hikaru_project_file/src/lib.rs
//
// =========================================================================
// POR QUÉ ESTE CRATE ES LGPL Y NO AGPL
// =========================================================================
//
// El ejecutable principal de Hikaru OpenLive/OpenStudio y su motor son AGPLv3.
// Los formatos de archivo, en cambio, son LGPLv3, y la razón es de
// interoperabilidad, no estética:
//
// Un DAW de CÓDIGO CERRADO (Image-Line, Ableton, Bitwig) que quiera leer o
// escribir proyectos `.oplf`/`.opsf` no debería tener que abrir el código del
// motor. Con LGPLv3 alcanza con que use esta librería tal cual, sin tocar el
// código AGPL del DAW propietario; si necesita MODIFICAR el formato, tiene que
// publicar sus cambios al formato bajo LGPLv3, y sólo a esa parte.
//
// La frontera se sostiene mientras el código del FORMATO no dependa del motor:
// si un struct del formato arrastrara un tipo de GPUI o del secuenciador, el
// "formato" dejaría de ser una librería y volvería a ser parte de la app
// AGPL. Por eso este crate depende sólo de `serde`.
//
// Ver docs/hikaru-project-files/hikaru-openlive-file/OPLF-README.md §4.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Versión del formato de archivo.
///
/// Sube cuando cambie la FORMA del JSON (un campo nuevo obligatorio, un tipo
/// distinto). Un archivo con versión mayor se rechaza al abrir en vez de
/// interpretarse a medias, que es como se corrompen los proyectos.
///
/// NO confundirse con la versión del programa: esa va informativa en la
/// cabecera (`created_by`) y la resuelve el binario, no la librería.
pub const FORMAT_VERSION: u32 = 1;

/// Errores de lectura/escritura de un proyecto.
#[derive(Debug)]
pub enum ProjectError {
    Io(std::io::Error),
    /// JSON inválido o con campos cuyo tipo no coincide con el del formato.
    Parse(serde_json::Error),
    /// El archivo es de otra versión del formato.
    UnsupportedVersion { found: u32, expected: u32 },
    /// La extensión no es de ningún formato de Hikaru, o el contenido no es
    /// JSON con la cabecera que corresponde.
    NotAProject(PathBuf),
}

impl std::fmt::Display for ProjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProjectError::Io(e) => write!(f, "no se pudo escribir el proyecto: {}", e),
            ProjectError::Parse(e) => write!(f, "el archivo no es un proyecto válido: {}", e),
            ProjectError::UnsupportedVersion { found, expected } => write!(
                f,
                "el proyecto es de la versión {} y esta build sólo lee hasta la {}",
                found, expected
            ),
            ProjectError::NotAProject(p) => write!(
                f,
                "{} no parece un proyecto de Hikaru (se esperaba un .oplf o un .opsf)",
                p.display()
            ),
        }
    }
}

impl From<std::io::Error> for ProjectError {
    fn from(e: std::io::Error) -> Self {
        ProjectError::Io(e)
    }
}

impl From<serde_json::Error> for ProjectError {
    fn from(e: serde_json::Error) -> Self {
        ProjectError::Parse(e)
    }
}

impl std::error::Error for ProjectError {}

// =========================================================================
// CANAL DE MEZCLA
// =========================================================================

/// Un canal de mezcla, tal como aparece en `live_tracks` (`.oplf`) y en
/// `studio_tracks` (`.opsf`).
///
/// Es el MISMO tipo en los dos formatos a propósito: el motor tiene un solo
/// modelo de bus, y duplicar la struct obligaría a mantener dos conversores
/// idénticos que divergen en seis meses. Lo que cambia entre modos no es el
/// canal sino QUIÉN lo lista: la fila de la matriz o la pista del arranger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: usize,
    pub name: String,
    pub volume: f32,
    pub pan: f32,
    /// 0 = estéreo, 1 = mid/side.
    pub pan_mode: u8,
    pub mute: bool,
    pub solo: bool,
    pub arm: bool,
    pub is_master: bool,
    pub route_destination_id: usize,
    pub sends: Vec<Send>,
    pub effects: Vec<DspSlot>,
    /// Índice de fila en la Session Matrix, o `None` para el master. Sólo
    /// tiene sentido en `.oplf`; en un `.opsf` viene `None` y no se usa.
    pub matrix_idx: Option<usize>,
}

/// Envío a otro bus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Send {
    pub target_id: usize,
    pub amount: f32,
}

/// Un slot del rack de DSP.
///
/// OJO con el alcance: se guardan identidad y bypass (`id`, `name`, `active`),
/// NO el contenido del sintetizador. La tabla de wavetable sí se referencia por
/// ruta ([`DspSlot::wavetable_path`]); el resto de los parámetros del panel
/// todavía no entra en el formato. Cuando entren, es agregar campos acá.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DspSlot {
    pub id: usize,
    pub name: String,
    pub active: bool,
    pub wavetable_path: Option<String>,
    /// Ruta del plugin externo (VST3/CLAP) cargado en el slot, si lo hay.
    ///
    /// Se guarda la ruta y no el estado del plugin: el estado de un VST3 es un
    /// blob opaco cuyo formato depende del plugin, y meterlo en el proyecto
    /// ataría el archivo a la versión exacta del host. Al reabrir, el DAW
    /// busca este binario en disco; si no está, el slot se muestra con un
    /// error en vez de romper la apertura.
    ///
    /// `#[serde(default)]` es obligatorio: sin él, un proyecto guardado antes de
    /// que existiera el campo no se abriría.
    #[serde(default)]
    pub plugin_path: Option<String>,
    /// "VST3" o "CLAP". Determina con qué cargador se instancia.
    #[serde(default)]
    pub plugin_format: Option<String>,
    /// Parámetros de síntesis si el slot contiene un Hikaru OpenWavetable.
    #[serde(default)]
    pub open_wavetable: Option<OpenWavetableParams>,
    /// Parámetros de modulación si el slot contiene un Hikaru OpenModulation.
    #[serde(default)]
    pub open_modulation: Option<OpenModulationParams>,
    /// Parámetros del sampler si el slot contiene un Hikaru Sampler.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampler: Option<SamplerParams>,
}

/// Parámetros de Hikaru Sampler para persistencia en `.oplf`, `.opsf` y `.hdp`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SamplerParams {
    pub sample_path: Option<String>,
    pub sample_name: String,
    pub playback_mode: String,
    pub speed: f32,
    pub reverse: bool,
    pub freeze: bool,
    pub ram_mode: bool,
    pub root_note: u8,
    pub fine_cents: f32,
    pub pitch_semitones: f32,
    pub glide_ms: f32,
    pub legato: bool,
    pub select_pos: f32,
    pub play_start: f32,
    pub play_end: f32,
    pub loop_mode: String,
    pub loop_start: f32,
    pub loop_len: f32,
    pub loop_crossfade: f32,
    pub offset_mode: String,
    pub filter_mode: String,
    pub filter_cutoff_hz: f32,
    pub filter_resonance: f32,
    pub filter_keytrack: f32,
    pub attack_ms: f32,
    pub hold_ms: f32,
    pub decay_ms: f32,
    pub sustain: f32,
    pub release_ms: f32,
    pub gain_db: f32,
    pub pan: f32,
    pub output_db: f32,
    pub velocity_sens: f32,
    pub note_trigger: bool,
    pub release_trigger: bool,
}

impl Default for SamplerParams {
    fn default() -> Self {
        Self {
            sample_path: None,
            sample_name: String::new(),
            playback_mode: "Repitch".to_string(),
            speed: 1.0,
            reverse: false,
            freeze: false,
            ram_mode: true,
            root_note: 60,
            fine_cents: 0.0,
            pitch_semitones: 0.0,
            glide_ms: 0.0,
            legato: false,
            select_pos: 0.0,
            play_start: 0.0,
            play_end: 1.0,
            loop_mode: "Off".to_string(),
            loop_start: 0.0,
            loop_len: 1.0,
            loop_crossfade: 0.0,
            offset_mode: "Play".to_string(),
            filter_mode: "LowPass12".to_string(),
            filter_cutoff_hz: 2640.0,
            filter_resonance: 0.0,
            filter_keytrack: 0.0,
            attack_ms: 2.0,
            hold_ms: 0.0,
            decay_ms: 200.0,
            sustain: 1.0,
            release_ms: 50.0,
            gain_db: 0.0,
            pan: 0.0,
            output_db: 0.0,
            velocity_sens: 1.0,
            note_trigger: true,
            release_trigger: false,
        }
    }
}

/// Parámetros de síntesis de Hikaru OpenWavetable para persistencia en `.oplf` y `.hdp`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenWavetableParams {
    #[serde(default)]
    pub table_name: String,
    pub position: f32,
    #[serde(default)]
    pub morph: f32,
    pub frame: usize,
    #[serde(default)]
    pub detune: f32,
    #[serde(default)]
    pub phase: f32,
    #[serde(default = "default_unison")]
    pub unison: u8,
    #[serde(default)]
    pub octave: i8,
    #[serde(default)]
    pub pitch: i8,
    #[serde(default = "default_voices")]
    pub voices: String,
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default)]
    pub render_mode_3d: bool,
    #[serde(default)]
    pub render_mode_lines: bool,
    #[serde(default)]
    pub color_primary: [f32; 3],
    #[serde(default)]
    pub color_secondary: [f32; 3],
    #[serde(default)]
    pub color_active: [f32; 3],
}

fn default_unison() -> u8 {
    1
}

fn default_volume() -> f32 {
    1.0
}

fn default_voices() -> String {
    "Poly".to_string()
}

/// Parámetros de modulación de Hikaru OpenModulation para persistencia en `.oplf` y `.hdp`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenModulationParams {
    pub waveform: String,
    pub sync_mode: String,
    pub rate_hz: f32,
    pub sync_rate: String,
    pub depth: f32,
    pub phase_deg: f32,
    pub bipolar: bool,
    pub amount: f32,
    pub target: String,
}

// =========================================================================
// PRESETS DE DEVICE PANEL (*.hdp)
// =========================================================================

/// Extensión estándar para presets del Device Panel de Hikaru.
pub const HDP_EXTENSION: &str = "hdp";

/// Filtro del selector de archivos para presets `.hdp`.
pub const HDP_FILE_FILTER: &str = "Hikaru Device Panel (*.hdp)";

/// Tipo de preset almacenado en un archivo `.hdp`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PresetType {
    /// Preset para un único dispositivo (ej. OpenWavetable u OpenModulation).
    SingleDevice,
    /// Preset para una cadena de dispositivos completa de una pista.
    Chain,
}

/// Estructura de archivo para presets de Device Panel (`.hdp`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HikaruDevicePreset {
    pub format_version: u32,
    pub created_by: String,
    pub preset_type: PresetType,
    pub name: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Dispositivo individual (cuando `preset_type == PresetType::SingleDevice`).
    #[serde(default)]
    pub slot: Option<DspSlot>,
    /// Cadena de dispositivos (cuando `preset_type == PresetType::Chain`).
    #[serde(default)]
    pub chain: Option<Vec<DspSlot>>,
}

/// Escribe un archivo `.hdp` en disco.
pub fn save_hdp(path: &Path, preset: &HikaruDevicePreset) -> Result<PathBuf, ProjectError> {
    write_json(path, preset)
}

/// Lee y parsea un archivo de preset `.hdp` desde disco validando la versión de formato.
pub fn parse_hdp(path: &Path) -> Result<HikaruDevicePreset, ProjectError> {
    parse_with_probe(path)
}

// =========================================================================
// COLOR
// =========================================================================

/// Color en HSLA plano.
///
/// Deliberadamente NO es el `Hsla` de GPUI: un formato de archivo que depende
/// de la librería de UI de la app deja de ser una librería y vuelve a ser parte
/// del programa AGPL. El consumidor decide cómo convertirlo.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Color {
    pub h: f32,
    pub s: f32,
    pub l: f32,
    pub a: f32,
}

// =========================================================================
// NOTA MIDI (PERSISTENCIA)
// =========================================================================

/// Nota MIDI persistible en archivos de proyecto (.oplf y .opsf).
///
/// `(inicio_en_ticks, nota, velocity, duración_en_ticks)`.
/// El campo `note` admite el alias serde `"pitch"` para interoperabilidad completa
/// con modelos del motor y de la interfaz de usuario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidiNote {
    pub start_tick: u64,
    #[serde(alias = "pitch")]
    pub note: u8,
    pub velocity: u8,
    pub duration_ticks: u32,
}

impl MidiNote {
    pub fn new(start_tick: u64, note: u8, velocity: u8, duration_ticks: u32) -> Self {
        Self {
            start_tick,
            note,
            velocity,
            duration_ticks,
        }
    }
}

// =========================================================================
// I/O
// =========================================================================

/// Escribe JSON lindo a propósito.
///
/// Los `.oplf`/`.opsf` están pensados para leerse y diffearse a ojo, y pesan
/// kilobytes (no megabytes, porque el audio va por ruta).
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<PathBuf, ProjectError> {
    let json = serde_json::to_string_pretty(value)?;
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    std::fs::write(path, json)?;
    Ok(path.to_path_buf())
}

/// Lee el texto y valida la cabecera antes de deserializar el cuerpo entero.
///
/// La versión se mira ANTES que el resto: si el archivo es de otra generación,
/// el error tiene que ser "versión" y no un `missing field` de serde que no
/// dice nada.
pub fn parse_with_probe<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, ProjectError> {
    let text = std::fs::read_to_string(path)?;

    #[derive(Deserialize)]
    struct VersionProbe {
        format_version: Option<u32>,
    }
    let probe: VersionProbe = serde_json::from_str(&text)
        .map_err(|_| ProjectError::NotAProject(path.to_path_buf()))?;
    let version = probe
        .format_version
        .ok_or_else(|| ProjectError::NotAProject(path.to_path_buf()))?;
    if version > FORMAT_VERSION {
        return Err(ProjectError::UnsupportedVersion {
            found: version,
            expected: FORMAT_VERSION,
        });
    }

    let value: T = serde_json::from_str(&text)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_json_sin_cabecera_no_es_un_proyecto() {
        let mut path = std::env::temp_dir();
        path.push(format!("hikaru_sonda_{}.json", std::process::id()));
        std::fs::write(&path, b"RIFF....WAVE esto es audio").unwrap();

        let err = parse_with_probe::<Track>(&path);
        assert!(
            matches!(err, Err(ProjectError::NotAProject(_))),
            "un WAV tiene que rechazarse como proyecto, no parsearse a medias"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn una_version_futura_se_rechaza_antes_de_parsear() {
        let mut path = std::env::temp_dir();
        path.push(format!("hikaru_futuro_{}.json", std::process::id()));
        std::fs::write(&path, format!("{{ \"format_version\": {} }}", FORMAT_VERSION + 1))
            .unwrap();

        match parse_with_probe::<Track>(&path) {
            Err(ProjectError::UnsupportedVersion { found, expected }) => {
                assert_eq!(found, FORMAT_VERSION + 1);
                assert_eq!(expected, FORMAT_VERSION);
            }
            other => panic!("error inesperado: {:?}", other),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn midi_note_serde_alias_support() {
        let json_with_note = r#"{"start_tick":480,"note":60,"velocity":100,"duration_ticks":240}"#;
        let parsed_note: MidiNote = serde_json::from_str(json_with_note).unwrap();
        assert_eq!(parsed_note.note, 60);

        let json_with_pitch = r#"{"start_tick":480,"pitch":64,"velocity":110,"duration_ticks":480}"#;
        let parsed_pitch: MidiNote = serde_json::from_str(json_with_pitch).unwrap();
        assert_eq!(parsed_pitch.note, 64);

        let serialized = serde_json::to_string(&parsed_pitch).unwrap();
        assert!(serialized.contains(r#""note":64"#));
    }

    #[test]
    fn hdp_single_device_preset_roundtrip() {
        let preset = HikaruDevicePreset {
            format_version: FORMAT_VERSION,
            created_by: "Hikaru OpenLive 2.24.0".to_string(),
            preset_type: PresetType::SingleDevice,
            name: "Skrillex Growl".to_string(),
            author: Some("Skrillex".to_string()),
            category: Some("Bass / Growl".to_string()),
            description: Some("Heavy vocal growl patch".to_string()),
            slot: Some(DspSlot {
                id: 1,
                name: "OpenWavetable".to_string(),
                active: true,
                wavetable_path: Some("/presets/Uber Growl.wav".to_string()),
                plugin_path: None,
                plugin_format: None,
                open_wavetable: Some(OpenWavetableParams {
                    table_name: "Uber Growl".to_string(),
                    position: 0.64,
                    morph: 0.0,
                    frame: 41,
                    detune: 0.0,
                    phase: 0.0,
                    unison: 1,
                    octave: -3,
                    pitch: 0,
                    voices: "Poly".to_string(),
                    volume: 0.85,
                    render_mode_3d: true,
                    render_mode_lines: true,
                    color_primary: [0.9, 0.5, 0.2],
                    color_secondary: [0.1, 0.6, 0.9],
                    color_active: [1.0, 0.7, 0.0],
                }),
                open_modulation: None,
                sampler: None,
            }),
            chain: None,
        };

        let mut path = std::env::temp_dir();
        path.push(format!("test_preset_{}.hdp", std::process::id()));
        save_hdp(&path, &preset).expect("save_hdp failed");

        let loaded = parse_hdp(&path).expect("parse_hdp failed");
        assert_eq!(loaded.name, "Skrillex Growl");
        assert_eq!(loaded.preset_type, PresetType::SingleDevice);
        let slot = loaded.slot.unwrap();
        assert_eq!(slot.name, "OpenWavetable");
        let wt = slot.open_wavetable.unwrap();
        assert_eq!(wt.octave, -3);
        assert_eq!(wt.frame, 41);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn legacy_dsp_slot_deserialization_without_new_fields() {
        let legacy_json = r#"{
            "id": 0,
            "name": "OpenWavetable",
            "active": true,
            "wavetable_path": null
        }"#;
        let slot: DspSlot = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(slot.name, "OpenWavetable");
        assert!(slot.open_wavetable.is_none());
        assert!(slot.open_modulation.is_none());
    }
}