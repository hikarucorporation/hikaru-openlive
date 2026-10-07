// Conversión entre el estado vivo y el Hikaru OpenStudio File (`.opsf`).
// GNU AGPLv3
// crates/hikaru_gui/src/project/openstudio.rs
//
// =========================================================================
// POR QUÉ ESTE FORMATO EXISTE
// =========================================================================
//
// La especificación de `.oplf` (docs/.../OPLF-README.md §1.2) prohíbe que un
// archivo de OpenLive lleve el timeline del Arranger adentro. Está bien: son
// dos cosas distintas y mezclarlas obligaba a que TODO proyecto guardara las
// dos mitades de la app.
//
// El costo de esa corrección es que el timeline lineal necesita un formato
// propio. Éste es. Es el hermano de `.oplf` y comparte con él los tipos de
// canal de mezcla, pero guarda lo OPUESTO:
//
//   * `playlist`      -> clips de la Playlist/Timeline, con sus ticks.
//   * `ppqn`          -> la resolución del timeline. Acá SÍ es dato del archivo.
//   * `studio_tracks` -> los canales del Arranger.
//   * `transport`     -> BPM, métrica, región de loop del timeline.
//
// Lo que NO entra, nunca (es de OpenLive): `matrix`, `scenes`, `grid`,
// `live_tracks` y `launch_quantization` (no hay "disparo" sin matriz).
//
// Abrir un `.opsf` pone la app en Modo OpenStudio (y viceversa con `.oplf`),
// así que el archivo y el modo no pueden quedar desalineados.
//
// =========================================================================
// DÓNDE ESTÁ LA FRONTERA DE LICENCIA
// =========================================================================
//
// El ESQUEMA del archivo vive en `hikaru_openstudio_file` (LGPLv3, sin
// dependencias de esta app). Acá sólo queda la conversión con el estado vivo.

use std::path::{Path, PathBuf};

use hikaru_openstudio_file::{
    ClipKind, CurvePoint, EngineMode, Metadata, OpenStudioProject, Playlist, PlaylistClipSaved,
    Stats, Transport, UiState,
};

use crate::app::{AppMode, AppState, OpenStudioView};
use crate::audio_proxy::GuiCommand;
use crate::views::playlist::{self, ClipType, PlaylistClip, PlaylistState};
use crate::version::VERSION;

use super::common::{self, ProjectError, Track};

/// Extensión del formato OpenStudio, desde el crate LGPL del formato.
pub const EXTENSION: &str = hikaru_openstudio_file::EXTENSION;

// =========================================================================
// CAPTURE: ESTADO -> ARCHIVO
// =========================================================================

/// Toma un snapshot del estado de OpenStudio.
///
/// AISLAMIENTO: lee únicamente `studio_tracks` y `playlist_state`. No toca la
/// Session Matrix ni los `live_tracks` del modo en vivo.
pub fn capture(state: &AppState) -> OpenStudioProject {
    let studio_tracks: Vec<Track> =
        state.studio_tracks.iter().map(common::capture_track).collect();
    let playlist = capture_playlist(&state.playlist_state);

    OpenStudioProject {
        format_version: common::FORMAT_VERSION,
        created_by: format!("Hikaru OpenStudio {}", VERSION),
        engine_mode: EngineMode::OpenStudio,
        metadata: capture_metadata(&studio_tracks, &playlist),
        transport: Transport {
            bpm: state.transport.bpm,
            beats_per_bar: state.transport.beats_per_bar,
            beat_division: state.transport.beat_division,
            loop_start_ticks: state.transport.samples_to_ticks(state.transport.loop_start_samples),
            loop_end_ticks: state.transport.samples_to_ticks(state.transport.loop_end_samples),
            loop_enabled: state.transport.loop_enabled,
        },
        studio_tracks,
        playlist,
        ui: UiState {
            openstudio_view: state.openstudio_view as u8,
            show_dsp_rack: state.show_dsp_rack,
            show_explorer: state.show_explorer,
        },
    }
}

/// Los recuentos se SACAN del snapshot recién capturado, no del estado vivo.
fn capture_metadata(studio_tracks: &[Track], playlist: &Playlist) -> Metadata {
    let mut audio_clips = 0;
    let mut midi_clips = 0;
    for (_, clip) in &playlist.clips {
        match clip.kind {
            ClipKind::Audio { .. } => audio_clips += 1,
            ClipKind::Pattern { .. } | ClipKind::Automation { .. } => midi_clips += 1,
        }
    }
    Metadata {
        title: String::new(),
        artist: None,
        genre: None,
        key: None,
        stats: Stats {
            total_tracks: studio_tracks.len(),
            total_clips: playlist.clips.len(),
            audio_clips,
            midi_clips,
        },
    }
}

fn capture_playlist(state: &PlaylistState) -> Playlist {
    Playlist {
        clips: state
            .clips
            .iter()
            .map(|(track_idx, clip)| (*track_idx, capture_playlist_clip(clip)))
            .collect(),
        ppqn: state.ppqn,
        next_clip_id: state.next_clip_id,
        loop_start_ticks: state.loop_start_ticks,
        loop_end_ticks: state.loop_end_ticks,
        loop_region_active: state.loop_region_active,
    }
}

fn capture_playlist_clip(clip: &PlaylistClip) -> PlaylistClipSaved {
    let kind = match &clip.clip_type {
        ClipType::Pattern { pattern_id } => ClipKind::Pattern {
            pattern_id: *pattern_id,
        },
        ClipType::Audio {
            sample_path,
            sample_offset_ticks,
            total_sample_ticks,
            ..
        } => ClipKind::Audio {
            sample_path: sample_path.clone(),
            sample_offset_ticks: *sample_offset_ticks,
            total_sample_ticks: *total_sample_ticks,
        },
        ClipType::Automation { points, target_param } => ClipKind::Automation {
            points: points
                .iter()
                .map(|p| CurvePoint {
                    rel_tick: p.rel_tick,
                    value: p.value,
                    tension: p.tension,
                })
                .collect(),
            target_param: target_param.clone(),
        },
    };

    PlaylistClipSaved {
        id: clip.id,
        name: clip.name.clone(),
        start_tick: clip.start_tick,
        duration_ticks: clip.duration_ticks,
        color: common::color_from_gpui(clip.color),
        kind,
    }
}

// =========================================================================
// APPLY: ARCHIVO -> ESTADO
// =========================================================================

/// Aplica un `.opsf` sobre el estado vivo.
///
/// AISLAMIENTO: escribe únicamente en `studio_tracks`, `playlist_state`,
/// `transport` y las vistas. Jamás toca [`AppState::matrix_state`] ni
/// [`AppState::live_tracks`].
pub fn apply(state: &mut AppState, project: &OpenStudioProject, base_dir: Option<&Path>) {
    // --- Transporte -------------------------------------------------------
    let bpm = project.transport.bpm.clamp(crate::app::BPM_MIN, crate::app::BPM_MAX);
    state.transport.bpm = bpm;
    state.transport.beats_per_bar = project.transport.beats_per_bar.max(1);
    state.transport.beat_division = project.transport.beat_division.max(1);
    state.audio_proxy.send(GuiCommand::SetBpm(bpm as f32));

    // --- Canales de mezcla ------------------------------------------------
    state.studio_tracks =
        common::tracks_or_default(&project.studio_tracks, common::default_studio_tracks);
    state.selected_track_index = state
        .selected_track_index
        .min(state.tracks().len().saturating_sub(1));

    // --- Playlist ---------------------------------------------------------
    // Va ANTES que el push del mixer, no después: `apply_playlist` decide qué
    // clips son válidos según cuántas pistas hay, y tiene que ver el set nuevo.
    apply_playlist(state, &project.playlist, base_dir);

    common::push_track_mix_to_engine(state);

    // --- Vista ------------------------------------------------------------
    // El modo se fuerza a OpenStudio: un `.opsf` sólo puede abrirse en
    // OpenStudio.
    state.mode = AppMode::OpenStudio;
    state.openstudio_view = openstudio_view_from_u8(project.ui.openstudio_view);
    state.show_dsp_rack = project.ui.show_dsp_rack;
    state.show_explorer = project.ui.show_explorer;

    // El loop global se manda al final: su región depende del BPM que se acaba
    // de cargar, y el wrap lo aplica el motor.
    let start_samples = state.transport.ticks_to_samples(project.transport.loop_start_ticks);
    let end_samples = state.transport.ticks_to_samples(project.transport.loop_end_ticks);
    state.transport.loop_enabled = project.transport.loop_enabled;
    state.transport.loop_start_samples = start_samples;
    state.transport.loop_end_samples = end_samples;
    state.global_loop_synced_to_engine = None;
    state.audio_proxy.send(GuiCommand::SetGlobalLoop {
        start_samples,
        end_samples,
        enabled: project.transport.loop_enabled,
    });
}

fn openstudio_view_from_u8(v: u8) -> OpenStudioView {
    match v {
        1 => OpenStudioView::ArrangerMixer,
        _ => OpenStudioView::Playlist,
    }
}

fn apply_playlist(state: &mut AppState, saved: &Playlist, base_dir: Option<&Path>) {
    let mut next = PlaylistState::default();
    next.ppqn = saved.ppqn.max(1);
    next.next_clip_id = saved.next_clip_id.max(1);
    next.loop_start_ticks = saved.loop_start_ticks;
    next.loop_end_ticks = saved.loop_end_ticks;
    next.loop_region_active = saved.loop_region_active;
    next.selected_clips.clear();
    next.clipboard.clear();

    for (track_idx, saved_clip) in &saved.clips {
        // Una pista que no existe en el proyecto no puede alojar un clip:
        // se descarta (y el proyecto sigue siendo válido).
        if *track_idx >= state.tracks().len() {
            continue;
        }
        let clip = build_playlist_clip(saved_clip, &next, state.transport.bpm, base_dir);
        next.clips.push((*track_idx, clip));
    }

    // `needs_full_sync` lo consume el ciclo normal de render, que es quien
    // manda los `LoadClip` al motor. No hace falta mandarlos acá: evita el
    // doble trabajo de cargar el mismo sample dos veces.
    next.needs_full_sync = !next.clips.is_empty();
    state.playlist_state = next;
    state.dragged_sample = None;
}

fn build_playlist_clip(
    saved: &PlaylistClipSaved,
    pl: &PlaylistState,
    bpm: f64,
    base_dir: Option<&Path>,
) -> PlaylistClip {
    let color = common::color_to_gpui(saved.color);
    let clip_type = match &saved.kind {
        ClipKind::Pattern { pattern_id } => ClipType::Pattern {
            pattern_id: *pattern_id,
        },
        ClipKind::Audio {
            sample_path,
            sample_offset_ticks,
            total_sample_ticks,
        } => {
            let path = common::resolve_path(base_dir, sample_path);
            // Los picos y la duración salen del archivo: si el sample cambió
            // desde el guardado, el clip refleja el archivo nuevo, no un
            // snapshot congelado de su waveform.
            let (file_duration_ticks, peaks) = playlist::load_sample_info(&path, pl.ppqn, bpm);
            let duration_ticks = if file_duration_ticks > 0 {
                saved.duration_ticks
            } else {
                file_duration_ticks.max(1)
            };
            ClipType::Audio {
                sample_path: path.to_string_lossy().to_string(),
                peaks,
                sample_offset_ticks: *sample_offset_ticks,
                total_sample_ticks: if *total_sample_ticks > 0 {
                    *total_sample_ticks
                } else {
                    duration_ticks
                },
            }
        }
        ClipKind::Automation { points, target_param } => ClipType::Automation {
            points: points
                .iter()
                .map(|p| playlist::CurvePoint {
                    rel_tick: p.rel_tick,
                    value: p.value,
                    tension: p.tension,
                })
                .collect(),
            target_param: target_param.clone(),
        },
    };

    PlaylistClip {
        id: saved.id,
        name: saved.name.clone(),
        start_tick: saved.start_tick,
        duration_ticks: saved.duration_ticks.max(1),
        clip_type,
        color,
    }
}

// =========================================================================
// SESIÓN EN BLANCO
// =========================================================================

/// La sesión de OpenStudio con la que arranca la app, como
/// [`OpenStudioProject`].
///
/// Sale de [`PlaylistState::default`] en vez de estar escrita a mano, igual que
/// en el módulo de `.oplf`.
pub fn blank() -> OpenStudioProject {
    let playlist = capture_playlist(&PlaylistState::default());
    let studio_tracks = common::default_studio_tracks();

    OpenStudioProject {
        format_version: common::FORMAT_VERSION,
        created_by: format!("Hikaru OpenStudio {}", VERSION),
        engine_mode: EngineMode::OpenStudio,
        metadata: capture_metadata(&studio_tracks, &playlist),
        transport: Transport {
            bpm: 140.0,
            beats_per_bar: 4,
            beat_division: 4,
            loop_start_ticks: 0,
            loop_end_ticks: 0,
            loop_enabled: false,
        },
        studio_tracks,
        playlist,
        ui: UiState {
            openstudio_view: 0,
            show_dsp_rack: false,
            show_explorer: false,
        },
    }
}

// =========================================================================
// I/O
// =========================================================================

/// Serializa el estado de OpenStudio y lo escribe en `path`.
///
/// Devuelve el camino escrito (puede diferir del pedido si se le agregó la
/// extensión).
pub fn save(state: &AppState, path: &Path) -> Result<PathBuf, ProjectError> {
    let path = super::with_extension(path, EXTENSION);
    let mut project = capture(state);
    project.metadata.title = common::title_from_path(&path);

    hikaru_openstudio_file::save(&path, &project)
}

/// Lee un `.opsf` de disco, validando la cabecera.
pub fn parse(path: &Path) -> Result<OpenStudioProject, ProjectError> {
    hikaru_openstudio_file::parse(path)
}