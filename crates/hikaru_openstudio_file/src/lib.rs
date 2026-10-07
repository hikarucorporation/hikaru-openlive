// Hikaru OpenStudio File (`.opsf`) — esquema del formato.
// SPDX-License-Identifier: LGPL-3.0-only
// crates/hikaru_openstudio_file/src/lib.rs
//
// =========================================================================
// POR QUÉ ESTE FORMATO EXISTE
// =========================================================================
//
// La especificación de `.oplf` (docs/.../OPLF-README.md §1.2) prohíbe que un
// archivo de OpenLive lleve el timeline del Arranger adentro. Está bien: son
// dos cosas distintas, y mezclarlas obligaba a que TODO proyecto guardara las
// dos mitades de la app.
//
// El costo de esa corrección es que el timeline lineal necesita un formato
// propio. Éste es. Es el hermano de `.oplf` y comparte con él los tipos de
// canal de mezcla (ver `hikaru_project_file`), pero guarda lo OPUESTO:
//
//   * `playlist`      -> clips de la Playlist/Timeline, con sus ticks.
//   * `ppqn`          -> la resolución del timeline. Acá SÍ es dato del archivo.
//   * `studio_tracks` -> los canales del Arranger.
//   * `transport`     -> BPM, métrica, región de loop del timeline.
//
// Lo que NO entra, nunca (es de OpenLive):
//
//   * `matrix`            -> Session Matrix, escenas y grilla de clips.
//   * `launch_quantization` -> cuantización de disparo: no hay "disparo" sin
//     matriz. En un `.opsf` el equivalente es la división del compás, que ya
//     está en `beat_division`.
//
// Misma frontera de licencia que `.oplf`: este crate es LGPLv3 y NO depende de
// la app. Ver `hikaru_openlive_file` y el OPLF-README.md §4.

use std::path::{Path, PathBuf};

use hikaru_project_file::{parse_with_probe, write_json, Color, ProjectError, Track};
use serde::{Deserialize, Serialize};

/// Extensión del formato OpenStudio.
pub const EXTENSION: &str = "opsf";

/// Etiqueta del filtro de archivo. El texto EXACTO es contrato de UI.
pub const FILE_FILTER: &str = "Hikaru OpenStudio Project (*.opsf)";

/// Etiqueta de modo que va en la cabecera.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineMode {
    OpenStudio,
}

/// Raíz del archivo `.opsf`.
///
/// Mismo contrato de `format_version` que `.oplf` (obligatoria, validada antes
/// de deserializar el resto).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenStudioProject {
    pub format_version: u32,
    /// Versión del programa que escribió el archivo. La resuelve el binario
    /// (`hikaru_gui`), no esta librería.
    pub created_by: String,
    /// Siempre [`EngineMode::OpenStudio`].
    pub engine_mode: EngineMode,
    pub metadata: Metadata,
    pub transport: Transport,
    /// Canales de mezcla del modo OpenStudio (master + pistas del Arranger).
    pub studio_tracks: Vec<Track>,
    pub playlist: Playlist,
    /// Vista activa al guardar. Es del modo OpenStudio, no de OpenLive.
    pub ui: UiState,
}

/// Cabecera informativa del archivo.
///
/// Misma forma que la de `.oplf` y por el mismo motivo: los campos que el
/// motor todavía no tiene van como `null` en vez de string vacío.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Metadata {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
    /// Recuentos derivados del playlist. Ver [`Metadata::stats`].
    #[serde(default)]
    pub stats: Stats,
}

/// Recuentos derivados. Se recalculan al guardar: son un resumen para el CLI.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Stats {
    pub total_tracks: usize,
    pub total_clips: usize,
    pub audio_clips: usize,
    pub midi_clips: usize,
}

/// Transporte del timeline.
///
/// A diferencia del `.oplf`, acá la resolución del compás se llama
/// `beat_division` y no `launch_quantization` porque no hay clips que disparar:
/// es la cuadrícula del secuenciador.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transport {
    pub bpm: f64,
    pub beats_per_bar: u32,
    pub beat_division: u32,
    /// Región de loop global en TICKS. En el timeline SÍ es un dato con
    /// sentido propio (la región A-B del arranger), a diferencia del loop del
    /// modo en vivo.
    pub loop_start_ticks: u64,
    pub loop_end_ticks: u64,
    pub loop_enabled: bool,
}

/// Modo y vistas activas del modo OpenStudio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiState {
    pub openstudio_view: u8,
    pub show_dsp_rack: bool,
    pub show_explorer: bool,
}

/// Estado de la Playlist / Timeline (OPENSTUDIO).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    /// Pares `(índice de pista, clip)`.
    pub clips: Vec<(usize, PlaylistClipSaved)>,
    /// Resolución del timeline en TICKS por negra.
    ///
    /// OJO: es lo que un `.oplf` tiene PROHIBIDO guardar, porque no hay
    /// timeline que cuantizar. Acá es el número que define dónde cae el tick 960
    /// de cada clip, así que perderlo descentra la sesión entera.
    pub ppqn: u64,
    pub next_clip_id: usize,
    pub loop_start_ticks: u64,
    pub loop_end_ticks: u64,
    pub loop_region_active: bool,
}

/// Clip de la Playlist.
///
/// Los picos de la waveform NO se guardan: los recalcula quien abre, leyendo el
/// sample al que apunta [`ClipKind::Audio`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistClipSaved {
    pub id: usize,
    pub name: String,
    pub start_tick: u64,
    pub duration_ticks: u64,
    pub color: Color,
    pub kind: ClipKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClipKind {
    /// Clip de patrón: el contenido lo resuelve el secuenciador.
    Pattern { pattern_id: usize },
    /// Clip de audio: se recarga del sample al abrir.
    Audio {
        sample_path: String,
        sample_offset_ticks: u64,
        total_sample_ticks: u64,
    },
    /// Clip de automatización.
    Automation {
        points: Vec<CurvePoint>,
        target_param: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CurvePoint {
    pub rel_tick: u64,
    pub value: f32,
    pub tension: f32,
}

// =========================================================================
// I/O
// =========================================================================

/// Escribe un `.opsf` en `path`.
pub fn save(path: &Path, project: &OpenStudioProject) -> Result<PathBuf, ProjectError> {
    write_json(path, project)
}

/// Lee un `.opsf` de disco, validando la cabecera.
pub fn parse(path: &Path) -> Result<OpenStudioProject, ProjectError> {
    parse_with_probe(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proyecto_de_ejemplo() -> OpenStudioProject {
        OpenStudioProject {
            format_version: hikaru_project_file::FORMAT_VERSION,
            created_by: "Hikaru OpenStudio 2.22.2".into(),
            engine_mode: EngineMode::OpenStudio,
            metadata: Metadata::default(),
            transport: Transport {
                bpm: 140.0,
                beats_per_bar: 4,
                beat_division: 4,
                loop_start_ticks: 0,
                loop_end_ticks: 0,
                loop_enabled: false,
            },
            studio_tracks: Vec::new(),
            playlist: Playlist {
                clips: vec![(
                    1,
                    PlaylistClipSaved {
                        id: 1,
                        name: "stab".into(),
                        start_tick: 960,
                        duration_ticks: 960,
                        color: Color { h: 0.57, s: 0.6, l: 0.4, a: 1.0 },
                        kind: ClipKind::Audio {
                            sample_path: "/samples/stab.wav".into(),
                            sample_offset_ticks: 480,
                            total_sample_ticks: 1920,
                        },
                    },
                )],
                ppqn: 960,
                next_clip_id: 2,
                loop_start_ticks: 0,
                loop_end_ticks: 7680,
                loop_region_active: true,
            },
            ui: UiState {
                openstudio_view: 0,
                show_dsp_rack: false,
                show_explorer: false,
            },
        }
    }

    #[test]
    fn un_opsf_conserva_el_ppqn_y_los_ticks() {
        let mut path = std::env::temp_dir();
        path.push(format!("hikaru_opsf_{}.opsf", std::process::id()));

        save(&path, &proyecto_de_ejemplo()).unwrap();

        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(json["engine_mode"], "OpenStudio");
        assert_eq!(json["playlist"]["ppqn"], 960);
        assert_eq!(json["playlist"]["clips"][0][1]["start_tick"], 960);

        let leido = parse(&path).unwrap();
        assert_eq!(leido.playlist.ppqn, 960);
        assert_eq!(leido.playlist.clips.len(), 1);
        assert_eq!(leido.playlist.clips[0].0, 1);

        let _ = std::fs::remove_file(&path);
    }
}