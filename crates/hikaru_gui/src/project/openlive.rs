// Conversión entre el estado vivo y el Hikaru OpenLive File (`.oplf`).
// GNU AGPLv3
// crates/hikaru_gui/src/project/openlive.rs
//
// =========================================================================
// DÓNDE ESTÁ LA FRONTERA DE LICENCIA
// =========================================================================
//
// El ESQUEMA del archivo (los structs, el parseo, el I/O) NO está acá: vive en
// `hikaru_openlive_file`, que es LGPLv3 y no depende de esta app. Ese crate es
// el que puede usar un DAW de código cerrado sin abrir el motor AGPL.
//
// Acá sólo queda la conversión en los dos sentidos:
//
//   estado vivo  --capture-->  OpenLiveProject  --> hikaru_openlive_file::save
//   estado vivo  <--apply-----  OpenLiveProject  <-- hikaru_openlive_file::parse
//
// =========================================================================
// AISLAMIENTO DEL MODO
// =========================================================================
//
// `capture` lee SÓLO `live_tracks`, `matrix_state` y `transport`.
// `apply` escribe SÓLO esos mismos, más las vistas. Jamás toca
// [`AppState::playlist_state`] ni [`AppState::studio_tracks`]: el timeline
// lineal es de OpenStudio y va en el `.opsf`.
//
// El `.oplf` no puede llevar `playlist`, `ppqn`, `studio_tracks` ni
// `arranger_clips` (OPLF-README.md §3.2), y esto es lo que lo garantiza.

use std::path::{Path, PathBuf};

use hikaru_openlive_file::{
    Clip, DspGlobal, EngineMode, Event, Matrix, Metadata, MidiNote, OpenLiveProject, SceneMeta,
    Stats, TrackMeta, Transport, UiState,
};

use crate::app::{AppMode, AppState, OpenLiveView};
use crate::audio_proxy::GuiCommand;
use crate::views::matrix;
use crate::views::mixer;
use crate::views::playlist::{self, PlaylistState};
use crate::version::VERSION;

use super::common::{self, ProjectError, Track};

/// Extensión del formato OpenLive, desde el crate LGPL del formato.
pub const EXTENSION: &str = hikaru_openlive_file::EXTENSION;

// =========================================================================
// CAPTURE: ESTADO -> ARCHIVO
// =========================================================================

/// Toma un snapshot del estado de OpenLive.
///
/// Es una COPIA: sigue siendo válido aunque el usuario siga moviendo faders,
/// que es lo que pasa si se guarda desde un frame ya capturado.
///
/// Lo único que NO se toca es [`AppState::playlist_state`] y
/// [`AppState::studio_tracks`]: no se leen, no se escriben, no se infieren.
/// Ese es el aislamiento que pide la especificación.
pub fn capture(state: &AppState) -> OpenLiveProject {
    let live_tracks: Vec<Track> = state.live_tracks.iter().map(common::capture_track).collect();

    // La matriz se captura tal cual. Los canales de OpenLive son los mismos
    // objetos en el mixer y en el header (el gesto escribe en los dos, ver
    // `sync_matrix_mixer_bidirectional`), así que no hace falta reconciliar
    // nada acá: lo que hay en uno está en el otro. Lo que SÍ pasa al abrir
    // es que la matriz gane, y por eso `apply` la aplica después del mixer.
    let matrix = capture_matrix(&state.matrix_state);

    OpenLiveProject {
        format_version: common::FORMAT_VERSION,
        // La cabecera se arma con la versión del BINARIO, nunca con la del
        // formato: son dos números distintos y confundirlos hace que un
        // `.oplf` nuevo parezca viejo. Ver OPLF-README.md §3.1.
        created_by: format!("Hikaru OpenLive {}", VERSION),
        engine_mode: EngineMode::OpenLive,
        metadata: capture_metadata(&matrix),
        transport: Transport {
            bpm: state.transport.bpm,
            time_signature: [state.transport.beats_per_bar, 4],
            launch_quantization: format!("1/{}", state.transport.beat_division),
            tap_tempo_enabled: false,
            loop_start_ticks: state.transport.samples_to_ticks(state.transport.loop_start_samples),
            loop_end_ticks: state.transport.samples_to_ticks(state.transport.loop_end_samples),
            loop_enabled: state.transport.loop_enabled,
        },
        dsp_global: capture_dsp_global(&live_tracks),
        live_tracks,
        matrix,
        ui: UiState {
            openlive_view: state.openlive_view as u8,
            show_dsp_rack: state.show_dsp_rack,
            show_explorer: state.show_explorer,
        },
    }
}

/// Los recuentos seSACAN de la matriz recién capturada.
///
/// Contarlos del estado en vez del snapshot garantiza que el resumen del
/// archivo describa exactamente lo que el archivo contiene.
fn capture_metadata(matrix: &Matrix) -> Metadata {
    let mut audio_clips = 0;
    let mut midi_clips = 0;
    for row in &matrix.grid {
        for slot in row {
            let Some(clip) = slot else { continue };
            if !clip.midi_notes.is_empty() {
                midi_clips += 1;
            } else {
                audio_clips += 1;
            }
        }
    }
    Metadata {
        title: String::new(),
        artist: None,
        genre: None,
        key: None,
        stats: Stats {
            total_scenes: matrix.scenes.len(),
            total_tracks: matrix.tracks.len(),
            audio_clips,
            midi_clips,
        },
    }
}

/// El master de `dsp_global` sale del canal master de `live_tracks`.
fn capture_dsp_global(live_tracks: &[Track]) -> DspGlobal {
    match live_tracks.iter().find(|t| t.is_master) {
        Some(master) => DspGlobal {
            master_volume: master.volume,
            master_pan: master.pan,
            effects_rack: master.effects.clone(),
        },
        None => DspGlobal::default(),
    }
}

/// Captura la Session Matrix.
///
/// Los `TrackMeta` se copian tal cual. El nombre de la fila vive acá y no en
/// el mixer: es el header de la matriz el que lo muestra y edita, y
/// [`crate::app::sync_matrix_mixer_bidirectional`] lo propaga al canal del
/// mixer. Guardar el del mixer en su lugar y el de la matriz acá sería guardar
/// la misma fila dos veces.
fn capture_matrix(state: &matrix::SessionMatrixState) -> Matrix {
    Matrix {
        tracks: state
            .tracks
            .iter()
            .map(|t| TrackMeta {
                name: t.name.clone(),
                muted: t.muted,
                soloed: t.soloed,
                volume: t.volume,
                pan: t.pan,
            })
            .collect(),
        scenes: state
            .scenes
            .iter()
            .map(|s| SceneMeta { name: s.name.clone() })
            .collect(),
        grid: state
            .grid
            .iter()
            .map(|row| {
                row.iter()
                    .map(|slot| slot.clip.as_ref().map(capture_clip))
                    .collect()
            })
            .collect(),
        next_clip_id: state.next_clip_id,
    }
}

fn capture_clip(clip: &matrix::MatrixClip) -> Clip {
    let (events, midi_notes) = match &clip.content {
        matrix::ClipData::Audio { events, .. } => (
            events
                .iter()
                // Un evento sin archivo de origen no se puede recargar al
                // abrir, así que no entra al archivo. El resto de los edits del
                // pad (trim, gain, fades) sí se guardan.
                .filter_map(|e| {
                    e.source_path.as_ref().map(|p| Event {
                        id: e.id,
                        name: e.name.clone(),
                        source_path: p.to_string_lossy().to_string(),
                        start_secs: e.start_secs,
                        trim_left_frames: e.trim_left_frames,
                        visible_frames: e.visible_frames,
                        gain: e.gain,
                        fade_in_secs: e.fade_in_secs,
                        fade_out_secs: e.fade_out_secs,
                    })
                })
                .collect(),
            Vec::new(),
        ),
        matrix::ClipData::Midi { notes } => (
            Vec::new(),
            notes
                .iter()
                .map(|(start, note, vel, dur)| MidiNote {
                    start_tick: *start,
                    note: *note,
                    velocity: *vel,
                    duration_ticks: *dur,
                })
                .collect(),
        ),
    };

    Clip {
        id: clip.id,
        name: clip.name.clone(),
        path: clip.path.to_string_lossy().to_string(),
        loop_start: clip.loop_start,
        loop_end: clip.loop_end,
        loop_enabled: clip.loop_enabled,
        volume: clip.local_track.volume,
        pan: clip.local_track.pan,
        mute: clip.local_track.mute,
        events,
        midi_notes,
    }
}

// =========================================================================
// APPLY: ARCHIVO -> ESTADO
// =========================================================================

/// Aplica un `.oplf` sobre el estado vivo.
///
/// Deja el motor sincronizado (BPM, loop global, volumen/pan/mute/solo por
/// pista), así que después de esto la app suena como el proyecto guardado sin
/// intervención del usuario.
///
/// AISLAMIENTO: esta función escribe únicamente en `live_tracks`,
/// `matrix_state`, `transport` y las vistas. Jamás toca
/// [`AppState::playlist_state`] ni [`AppState::studio_tracks`].
///
/// `base_dir` es el directorio del archivo: las rutas relativas de los samples
/// se resuelven contra él para que un proyecto con sus samples en la misma
/// carpeta se pueda mover entero.
pub fn apply(state: &mut AppState, project: &OpenLiveProject, base_dir: Option<&Path>) {
    // --- Transporte -------------------------------------------------------
    // El BPM se manda al motor SIEMPRE: es lo que define el tempo con el que
    // corren los clips, y mandar un `SetBpm` es barato (una sola f64 por
    // comando, no estamos en el hilo de audio).
    let bpm = project.transport.bpm.clamp(crate::app::BPM_MIN, crate::app::BPM_MAX);
    state.transport.bpm = bpm;
    state.transport.beats_per_bar = project.transport.time_signature[0].max(1);
    state.transport.beat_division = project.transport.quantization_denominator();
    state.audio_proxy.send(GuiCommand::SetBpm(bpm as f32));

    // --- Canales de mezcla ------------------------------------------------
    state.live_tracks = common::tracks_or_default(&project.live_tracks, common::default_live_tracks);
    // `dsp_global` manda sobre el canal master (ver su doc comment): es la
    // misma sesión, escrita por el lado del rack global.
    if let Some(master) = state.live_tracks.iter_mut().find(|t| t.is_master) {
        master.volume = project.dsp_global.master_volume;
        master.pan = project.dsp_global.master_pan;
    }
    state.selected_track_index = state.selected_track_index.min(state.tracks().len().saturating_sub(1));

    // La matriz manda sobre el mixer del modo OpenLive (es el que se edita
    // desde los headers de la Session Matrix), así que se re-sincroniza en
    // ambos sentidos con los valores recién cargados.
    apply_matrix(state, &project.matrix, base_dir);

    // El orden importa y es el inverso al del arranque. Al abrir, el archivo
    // manda: primero se arma el mixer desde `live_tracks`, después la matriz
    // desde `matrix`, y recién ahí se los reconcilia. El sync propaga el
    // nombre y el resto de la fila desde la matriz hacia el canal del mixer
    // (que es de donde los saca el arranger) sin pisar los slots de efecto ni
    // el ruteo, que no existen en la matriz.
    crate::app::sync_matrix_mixer_bidirectional(
        &mut state.live_tracks,
        &mut state.matrix_state,
        &[],
        &[],
        &state.audio_proxy,
    );

    common::push_track_mix_to_engine(state);

    // --- Vista ------------------------------------------------------------
    // El modo se fuerza a OpenLive: un `.oplf` sólo puede abrirse en OpenLive.
    // (Un `.opsf` hace lo simétrico en su módulo.)
    state.mode = AppMode::OpenLive;
    state.openlive_view = openlive_view_from_u8(project.ui.openlive_view);
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

/// `matches!` del enum de vista contra un `u8` del archivo. Desconocido = el
/// default, para que un archivo de una build futura no rompa la apertura.
fn openlive_view_from_u8(v: u8) -> OpenLiveView {
    match v {
        1 => OpenLiveView::ArrangerView,
        _ => OpenLiveView::SessionMatrix,
    }
}

fn apply_matrix(state: &mut AppState, saved: &Matrix, base_dir: Option<&Path>) {
    let mut next = matrix::SessionMatrixState::default();
    next.next_clip_id = saved.next_clip_id.max(1);

    next.tracks = saved
        .tracks
        .iter()
        .map(|t| matrix::TrackMeta {
            name: t.name.clone(),
            muted: t.muted,
            soloed: t.soloed,
            volume: t.volume,
            pan: t.pan,
        })
        .collect();
    next.scenes = saved
        .scenes
        .iter()
        .map(|s| matrix::SceneMeta { name: s.name.clone() })
        .collect();

    // La grilla del archivo manda en las dimensiones. Si viene vacía (proyecto
    // a medio hacer) se deja la grilla por defecto.
    if !saved.tracks.is_empty() && !saved.scenes.is_empty() {
        next.grid = saved
            .grid
            .iter()
            .map(|row| {
                row.iter()
                    .map(|slot| matrix::MatrixSlot {
                        // Todos los pads abren DETENIDOS: el estado de
                        // reproducción (`SlotState::Playing`) es del motor, no
                        // del proyecto.
                        state: match slot {
                            Some(_) => matrix::SlotState::Stopped,
                            None => matrix::SlotState::Empty,
                        },
                        clip: slot.as_ref().map(|c| build_clip(c, base_dir)),
                    })
                    .collect()
            })
            .collect();
    }

    // Un pad sin fila o columna propia no se puede mostrar: se descarta en
    // vez de romper el layout.
    next.grid.truncate(next.tracks.len());
    if let Some(first) = next.grid.first_mut() {
        first.truncate(next.scenes.len());
    }
    next.selected_slot = next
        .selected_slot
        .filter(|(t, s)| *t < next.tracks.len() && *s < next.scenes.len());

    // Avisa al motor de los loops de clip, que es estado que el motor usa al
    // disparar y no al leer el archivo.
    //
    // El PPQN sale del TRANSPORTE (`transport.ppqn()`, la constante única del
    // motor) y no del archivo: el PPQN es del timeline lineal y un `.oplf` no
    // lo guarda. Es también lo correcto, porque la conversión tick->segundo que
    // necesita el motor depende del PPQN con el que va a correr la sesión, no
    // del que tuviera guardada la máquina que exportó.
    let ppqn = state.transport.ppqn();
    let bpm = state.transport.bpm;
    for (track_idx, row) in next.grid.iter().enumerate() {
        for (scene_idx, slot) in row.iter().enumerate() {
            let Some(clip) = &slot.clip else { continue };
            state.audio_proxy.send(GuiCommand::SetClipLoop {
                track_idx,
                scene_idx,
                start_secs: playlist::ticks_to_secs_precise(clip.loop_start, ppqn, bpm),
                end_secs: playlist::ticks_to_secs_precise(clip.loop_end, ppqn, bpm),
                enabled: clip.loop_enabled,
            });
        }
    }

    state.matrix_state = next;
    state.matrix_clipboard = matrix::MatrixClipboard::default();
}

fn build_clip(saved: &Clip, base_dir: Option<&Path>) -> matrix::MatrixClip {
    let path = common::resolve_path(base_dir, &saved.path);

    let content = if !saved.midi_notes.is_empty() || crate::views::explorer::is_midi_file(&path) {
        matrix::ClipData::Midi {
            notes: saved
                .midi_notes
                .iter()
                .map(|n| (n.start_tick, n.note, n.velocity, n.duration_ticks))
                .collect(),
        }
    } else {
        matrix::ClipData::Audio {
            events: Vec::new(),
            next_event_id: saved.events.iter().map(|e| e.id + 1).max().unwrap_or(1),
            preview_mix: Vec::new(),
            preview_sr: 44100,
        }
    };

    let mut clip = matrix::MatrixClip {
        id: saved.id,
        name: saved.name.clone(),
        path,
        duration_secs: 0.0,
        content,
        local_state: PlaylistState::default(),
        local_track: mixer::Track::new(0, saved.name.clone(), false),
        local_bar: 1.0,
        loop_start: saved.loop_start,
        loop_end: saved.loop_end,
        loop_enabled: saved.loop_enabled,
        has_time_selection: true,
        peaks: Vec::new(),
    };
    clip.local_track.volume = saved.volume;
    clip.local_track.pan = saved.pan;
    clip.local_track.mute = saved.mute;

    // Recarga de los samples: se leen del disco y se re-decodifican. Los picos
    // y el preview se derivan de ahí (`refresh_preview`).
    for event in &saved.events {
        let event_path = common::resolve_path(base_dir, &event.source_path);
        let Some((samples, channels, sr)) = matrix::decode_audio_file(&event_path) else {
            eprintln!(
                "[OPLF] el sample {} del clip '{}' no se pudo leer; el pad queda sin ese evento",
                event_path.display(),
                saved.name
            );
            continue;
        };
        let mut ev = matrix::AudioEvent::new_full(
            event.id,
            event.name.clone(),
            samples,
            channels,
            sr,
            event.start_secs,
        );
        ev.source_path = Some(event_path);
        ev.trim_left_frames = event.trim_left_frames;
        ev.visible_frames = event.visible_frames;
        ev.gain = event.gain;
        ev.fade_in_secs = event.fade_in_secs;
        ev.fade_out_secs = event.fade_out_secs;
        if let Some(events) = clip.audio_events_mut() {
            events.push(ev);
        }
    }

    // Un clip recién arrastrado al pad tiene la lista de eventos VACÍA: los
    // samples los decodifica y los manda el motor, de forma asíncrona. Si sólo
    // se recargaran los eventos, ese clip --el caso más común-- volvería
    // mudo al abrir el proyecto. Por eso, sin eventos, se recarga el sample
    // base del clip.
    if clip.audio_events().is_empty() {
        if let Some((samples, channels, sr)) = matrix::decode_audio_file(&clip.path) {
            let name = clip
                .path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let mut ev =
                matrix::AudioEvent::new_full(clip.alloc_event_id(), name, samples, channels, sr, 0.0);
            ev.source_path = Some(clip.path.clone());
            if let Some(events) = clip.audio_events_mut() {
                events.push(ev);
            }
        } else if !crate::views::explorer::is_midi_file(&clip.path) {
            eprintln!(
                "[OPLF] el sample {} del clip '{}' no se pudo leer al abrir el proyecto",
                clip.path.display(),
                saved.name
            );
        }
    }

    clip.refresh_preview();
    clip.peaks = crate::views::open_dms_sampler::load_peaks_from_wav(
        &clip.path.to_string_lossy(),
        matrix::MatrixClip::PEAK_BINS,
    );

    clip
}

// =========================================================================
// SESIÓN EN BLANCO
// =========================================================================

/// La sesión de OpenLive con la que arranca la app, como [`OpenLiveProject`].
///
/// Sale de los `Default` de la matriz (los mismos que usa `HikaruApp::build`)
/// en vez de estar escrita a mano: si mañana el default de la Session Matrix
/// pasa a 16 pistas, el `New Project` las tiene sin tocar este archivo.
pub fn blank() -> OpenLiveProject {
    let default_matrix = matrix::SessionMatrixState::default();
    let matrix = capture_matrix(&default_matrix);

    // El master + un canal por fila de la matriz, que es la correspondencia
    // que hace `sync_matrix_mixer_bidirectional`.
    let mut live_tracks = common::default_live_tracks();
    for (idx, meta) in default_matrix.tracks.iter().enumerate() {
        live_tracks.push(Track {
            id: idx + 1,
            name: meta.name.clone(),
            volume: meta.volume,
            pan: meta.pan,
            pan_mode: 0,
            mute: meta.muted,
            solo: meta.soloed,
            arm: false,
            is_master: false,
            route_destination_id: 0,
            sends: Vec::new(),
            effects: Vec::new(),
            matrix_idx: Some(idx),
        });
    }

    OpenLiveProject {
        format_version: common::FORMAT_VERSION,
        created_by: format!("Hikaru OpenLive {}", VERSION),
        engine_mode: EngineMode::OpenLive,
        metadata: capture_metadata(&matrix),
        transport: Transport {
            bpm: 140.0,
            time_signature: [4, 4],
            launch_quantization: "1/4".to_string(),
            tap_tempo_enabled: false,
            loop_start_ticks: 0,
            loop_end_ticks: 0,
            loop_enabled: false,
        },
        dsp_global: capture_dsp_global(&live_tracks),
        live_tracks,
        matrix,
        ui: UiState {
            openlive_view: 0,
            show_dsp_rack: false,
            show_explorer: false,
        },
    }
}

// =========================================================================
// I/O
// =========================================================================

/// Serializa el estado de OpenLive y lo escribe en `path`.
///
/// Devuelve el camino escrito (puede diferir del pedido si se le agregó la
/// extensión).
pub fn save(state: &AppState, path: &Path) -> Result<PathBuf, ProjectError> {
    let path = super::with_extension(path, EXTENSION);
    let mut project = capture(state);
    project.metadata.title = common::title_from_path(&path);

    hikaru_openlive_file::save(&path, &project)
}

/// Lee un `.oplf` de disco, validando la cabecera.
pub fn parse(path: &Path) -> Result<OpenLiveProject, ProjectError> {
    hikaru_openlive_file::parse(path)
}