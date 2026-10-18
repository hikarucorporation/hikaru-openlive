// Hikaru OpenLive File (`.oplf`) — esquema del formato.
// SPDX-License-Identifier: LGPL-3.0-only
// crates/hikaru_openlive_file/src/lib.rs
//
// Especificación: docs/hikaru-project-files/hikaru-openlive-file/OPLF-README.md
// (formato 1.0).
//
// =========================================================================
// EL FORMATO Y SU FRONTERA
// =========================================================================
//
// Un `.oplf` describe UNA SOLA COSA: una sesión de performance no lineal. Lo
// que entra:
//
//   * `matrix`       -> Session Matrix View: escenas, pistas, clips de la grilla.
//   * `live_tracks`  -> los canales de mezcla del modo OpenLive.
//   * `transport`    -> BPM, métrica, cuantización de disparo, loop global.
//   * `metadata`     -> cabecera informativa.
//
// Lo que NO entra, nunca, bajo ningún modo (OPLF-README.md §3.2):
//
//   * `playlist`     -> el timeline del Arranger. Es de OpenStudio (`.opsf`).
//   * `ppqn`         -> ídem: el `.oplf` usa el PPQN del motor, no lo guarda.
//   * `studio_tracks`-> las pistas del modo lineal. Es de OpenStudio.
//   * `arranger_clips`-> no existe este concepto acá.
//
// =========================================================================
// POR QUÉ LGPL Y QUÉ IMPLICA
// =========================================================================
//
// Este crate es LGPLv3 a propósito (ver OPLF-README.md §4). El programa y su
// motor son AGPLv3; el formato no. Así, un DAW de código cerrado puede leer y
// escribir `.oplf` sin abrir el motor, y el artista puede guardar sus sesiones
// con sus plugins propietarios intactos.
//
// La consecuencia práctica es que este crate NO puede depender de la app. Si un
// struct del formato arrastrara un tipo de GPUI o del secuenciador, dejaría de
// ser una librería y volvería a ser parte del programa AGPL, y toda la frontera
// se caería. Acá sólo hay `serde`.
//
// Lo que SÍ necesita la app --convertir su estado vivo a estos structs y
// viceversa-- vive en `hikaru_gui/src/project/openlive.rs`, que sí es AGPL.
// La frontera es exactamente esa línea.

use std::path::{Path, PathBuf};

use hikaru_project_file::{parse_with_probe, write_json, DspSlot, ProjectError, Track};
use serde::{Deserialize, Serialize};

/// Extensión del formato OpenLive.
pub const EXTENSION: &str = "oplf";

/// Etiqueta del filtro de archivo. El texto EXACTO es contrato de UI.
pub const FILE_FILTER: &str = "Hikaru OpenLive File (*.oplf)"; // Ahí corregí el "Project" por el "File".

/// Etiqueta de modo que va en la cabecera.
///
/// Es la que le dice a un lector externo (y a la app al abrir) de qué mitad de
// la herramienta es este archivo. No es decoración: es lo que impide tratar un
/// `.opsf` como si fuera una sesión.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineMode {
    OpenLive,
}

/// Raíz del archivo `.oplf`.
///
/// `format_version` es OBLIGATORIA y se valida antes de deserializar el resto:
/// es el primer campo del JSON justamente para poder cortocircuitar (ver
/// [`parse`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenLiveProject {
    pub format_version: u32,
    /// Versión del programa que escribió el archivo. Informativa: no se usa
    /// para migraciones (para eso está `format_version`).
    ///
    /// La resuelve el binario (`hikaru_gui`), NO esta librería: una librería de
    /// formato no tiene por qué saber qué versión se está compilando.
    pub created_by: String,
    /// Siempre [`EngineMode::OpenLive`].
    pub engine_mode: EngineMode,
    pub metadata: Metadata,
    pub transport: Transport,
    pub dsp_global: DspGlobal,
    /// Canales de mezcla del modo OpenLive (master + filas de la matriz).
    pub live_tracks: Vec<Track>,
    pub matrix: Matrix,
    /// Vista activa al guardar. Es del modo OpenLive, no del timeline.
    pub ui: UiState,
}

/// Cabecera informativa del archivo.
///
/// Los campos que el motor todavía no tiene (`artist`, `genre`, `key`) van
/// como `null` en vez de inventarse un string vacío: un lector externo puede
/// distinguir "no declarado" de "vacío a propósito". Cuando se agreguen tags
/// reales, aparecen acá y el archivo viejo sigue leyéndose.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Metadata {
    /// Nombre del proyecto.
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
    /// Conteos derivados. Ver [`Metadata::stats`].
    #[serde(default)]
    pub stats: Stats,
}

/// Recuentos derivados. Se recalculan al guardar: no son una fuente de verdad,
/// son un resumen para el CLI y los lectores externos.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Stats {
    pub total_scenes: usize,
    pub total_tracks: usize,
    pub audio_clips: usize,
    pub midi_clips: usize,
}

/// Transporte: todo lo que NO es posición en el tiempo.
///
/// La posición (`sample_count`) NO se guarda: al abrir un proyecto arranca en
/// cero, que es lo que espera cualquiera que abre una sesión en vivo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transport {
    pub bpm: f64,
    /// `[numerador, denominador]`, tal como lo muestra el OPLF-README.md §2.
    pub time_signature: [u32; 2],
    /// Cuantización de disparo en notación musical: `"1/1"`, `"1/4"`, etc.
    ///
    /// Es el campo canónico del formato. El motor divide el compás por este
    /// valor, así que un lector externo puede reconstruir la cuadrícula de
    /// disparo sin conocer nada de la implementación.
    pub launch_quantization: String,
    /// El transporte todavía no tiene estado de tap tempo. El campo existe por
    /// la forma que fija la especificación; `#[serde(default)]` para que un
    /// archivo que lo omita siga siendo válido.
    #[serde(default)]
    pub tap_tempo_enabled: bool,
    /// Región de loop global en TICKS (no en samples: los samples dependen del
    /// sample rate de la máquina que abra el proyecto).
    pub loop_start_ticks: u64,
    pub loop_end_ticks: u64,
    pub loop_enabled: bool,
}

impl Transport {
    /// El denominador de la cuantización de disparo, o 1 si no se puede leer.
    ///
    /// Tolera `"4"` (sin barra) y `"1/4"`, y se clampa a 1: una división en 0
    /// rompe la conversión a segundos y deja NaN en el motor.
    pub fn quantization_denominator(&self) -> u32 {
        self.launch_quantization
            .rsplit('/')
            .next()
            .unwrap_or("1")
            .trim()
            .parse::<u32>()
            .unwrap_or(1)
            .max(1)
    }
}

/// Estado de master y del rack global del modo en vivo.
///
/// ORIGEN DE VERDAD: estos campos son una VISTA del canal master que aparece en
/// [`OpenLiveProject::live_tracks`], no un segundo master. Quien escribe tiene
/// que copiarlos de ahí, y quien lee tiene que escribirlos sobre ese mismo
/// canal, para que no diverjan.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DspGlobal {
    pub master_volume: f32,
    pub master_pan: f32,
    pub effects_rack: Vec<DspSlot>,
}

/// Modo y vistas activas del modo OpenLive.
///
/// No incluye nada del modo OpenStudio: eso va en el `.opsf`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiState {
    pub openlive_view: u8,
    pub show_dsp_rack: bool,
    pub show_explorer: bool,
}

/// Estado de la Session Matrix (OPENLIVE).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Matrix {
    /// `tracks.len() == grid.len()` y `scenes.len() == grid[0].len()` siempre:
    /// la grilla es rectangular y las pistas son FILAS.
    pub tracks: Vec<TrackMeta>,
    pub scenes: Vec<SceneMeta>,
    /// Filas = pistas, columnas = escenas. `None` = pad vacío.
    pub grid: Vec<Vec<Option<Clip>>>,
    pub next_clip_id: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackMeta {
    pub name: String,
    pub muted: bool,
    pub soloed: bool,
    pub volume: f32,
    pub pan: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneMeta {
    pub name: String,
}

/// Un clip de un pad de la Session Matrix.
///
/// `path` es el sample base. Los eventos editables ([`Clip::events`]) guardan
/// su propia ruta para poder recargar samples distintos en un mismo pad, que es
/// justo lo que habilita el multi-sample del Clip Editor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clip {
    pub id: usize,
    pub name: String,
    pub path: String,
    pub loop_start: u64,
    pub loop_end: u64,
    pub loop_enabled: bool,
    /// Volumen/pan/mute del canalito PROPIO del clip, que es distinto del canal
    /// de la pista que lo dispara.
    pub volume: f32,
    pub pan: f32,
    pub mute: bool,
    /// Eventos de audio apilados en el pad, en orden de arranque.
    pub events: Vec<Event>,
    /// Notas MIDI del clip, si el pad es MIDI. Vacío para pads de audio.
    pub midi_notes: Vec<MidiNote>,
}

/// Un sample dentro de un pad.
///
/// Se guarda TODO menos los samples en sí: se vuelven a leer de
/// [`Event::source_path`] al abrir el proyecto. Por eso el formato no empodera
/// audio y pesa kilobytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: u64,
    pub name: String,
    pub source_path: String,
    pub start_secs: f64,
    pub trim_left_frames: usize,
    pub visible_frames: usize,
    pub gain: f32,
    pub fade_in_secs: f64,
    pub fade_out_secs: f64,
}

/// Nota MIDI: `(inicio_en_ticks, nota, velocity, duración_en_ticks)`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MidiNote {
    pub start_tick: u64,
    pub note: u8,
    pub velocity: u8,
    pub duration_ticks: u32,
}

// =========================================================================
// I/O
// =========================================================================

/// Escribe un `.oplf` en `path`.
pub fn save(path: &Path, project: &OpenLiveProject) -> Result<PathBuf, ProjectError> {
    write_json(path, project)
}

/// Lee un `.oplf` de disco, validando la cabecera.
pub fn parse(path: &Path) -> Result<OpenLiveProject, ProjectError> {
    parse_with_probe(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transporte_de_ejemplo() -> Transport {
        Transport {
            bpm: 160.0,
            time_signature: [4, 4],
            launch_quantization: "1/4".into(),
            tap_tempo_enabled: false,
            loop_start_ticks: 0,
            loop_end_ticks: 0,
            loop_enabled: false,
        }
    }

    #[test]
    fn la_cuantizacion_se_lee_en_notacion_musical() {
        let mut t = transporte_de_ejemplo();
        assert_eq!(t.quantization_denominator(), 4);

        for (escrito, esperado) in
            [("1/1", 1u32), ("1/16", 16), ("1/32", 32), ("16", 16), ("1/x", 1)]
        {
            t.launch_quantization = escrito.into();
            assert_eq!(
                t.quantization_denominator(),
                esperado,
                "no se leyó bien {:?}",
                escrito
            );
        }
    }

    #[test]
    fn una_cuantizacion_en_cero_no_deja_la_division_en_cero() {
        let mut t = transporte_de_ejemplo();
        t.launch_quantization = "1/0".into();
        assert!(
            t.quantization_denominator() >= 1,
            "una división en cero rompe la conversión a segundos"
        );
    }

    #[test]
    fn un_oplf_que_se_guarda_trae_la_version_que_dijo() {
        let mut path = std::env::temp_dir();
        path.push(format!("hikaru_oplf_{}.oplf", std::process::id()));

        let project = OpenLiveProject {
            format_version: hikaru_project_file::FORMAT_VERSION,
            created_by: "Hikaru OpenLive 2.22.2".into(),
            engine_mode: EngineMode::OpenLive,
            metadata: Metadata::default(),
            transport: transporte_de_ejemplo(),
            dsp_global: DspGlobal::default(),
            live_tracks: Vec::new(),
            matrix: Matrix {
                tracks: Vec::new(),
                scenes: Vec::new(),
                grid: Vec::new(),
                next_clip_id: 1,
            },
            ui: UiState {
                openlive_view: 0,
                show_dsp_rack: false,
                show_explorer: false,
            },
        };
        save(&path, &project).unwrap();

        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(json["engine_mode"], "OpenLive");
        assert_eq!(json["created_by"], "Hikaru OpenLive 2.22.2");
        assert_eq!(json["format_version"], hikaru_project_file::FORMAT_VERSION);

        // Y vuelve a leerse igual.
        let leido = parse(&path).unwrap();
        assert_eq!(leido.created_by, project.created_by);
        assert!((leido.transport.bpm - 160.0).abs() < 1e-9);

        let _ = std::fs::remove_file(&path);
    }
}