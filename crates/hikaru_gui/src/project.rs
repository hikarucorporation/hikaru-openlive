// Hikaru OpenLive - Formato de proyecto (.hikaru)
// GNU AGPLv3
// crates/hikaru_gui/src/project.rs
//
// =========================================================================
// POR QUÉ UN MÓDULO ESPEJO Y NO `#[derive(Serialize)]` EN LOS ESTADOS VIVOS
// =========================================================================
//
// `AppState`, `mixer::Track`, `matrix::MatrixClip` y `playlist::PlaylistClip`
// NO son serializables, y no por capricho: arrastran handles de GPU
// (`ScrollHandle`), entidades de GPUI (`Entity<InputState>`), plugins externos
// vivos (`Box<dyn PluginInstance>`) y el editor de wavetable completo (meshes,
// cámara, canvas). Derivar el `Serialize` ahí metería `#[serde(skip)]` por
// todos lados y, peor, ataría el formato del archivo a la forma del estado en
// memoria: cualquier refactor interno rompería la compatibilidad de los
// proyectos guardados.
//
// Por eso el archivo se describe con structs PROPIAS ([`Project`] y sus
// sub-structs), que son el contrato público del formato. Hay conversión en
// los dos sentidos: [`capture`] (estado -> archivo) y [`apply`] (archivo ->
// estado). Si mañana cambia un campo del estado, sólo se toca esta capa.
//
// =========================================================================
// REGLAS DEL FORMATO
// =========================================================================
//
// 1. El audio NUNCA se embebe. Un `.hikaru` guarda la RUTA del sample y, al
//    abrir, el sample se vuelve a decodificar del disco. Es lo que hace un DAW
//    de verdad: el proyecto es liviano y los samples siguen siendo editables
//    con otra herramienta.
// 2. Todo lo que es caché o derivado (picos de waveform, `preview_mix`) se
//    recalcula al cargar, no se guarda.
// 3. Es un JSON con extensión `.hikaru`: se puede versionar en git, diffear a
//    ojo y reparar a mano. [`FORMAT_VERSION`] sube cuando cambie la forma.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app::{AppMode, AppState, OpenLiveView, OpenStudioView, PanMode};
use crate::audio_proxy::GuiCommand;
use crate::views::matrix;
use crate::views::mixer;
use crate::views::playlist::{self, ClipType, Color32, PlaylistClip, PlaylistState};

/// Versión del formato. Un `.hikaru` con versión mayor se rechaza al abrir en
/// vez de interpretarse a medias (que es como se corrompen los proyectos).
pub const FORMAT_VERSION: u32 = 1;

/// Extensión de los archivos de proyecto.
pub const PROJECT_EXTENSION: &str = "hikaru";

/// Errores de lectura/escritura de un proyecto.
#[derive(Debug)]
pub enum ProjectError {
    Io(std::io::Error),
    /// JSON inválido o con campos cuyo tipo no coincide con el del formato.
    Parse(serde_json::Error),
    /// El archivo es de otra versión del formato.
    UnsupportedVersion { found: u32, expected: u32 },
    /// La extensión no es `.hikaru` y el parseo falló (suele ser "abriste el
    /// WAV con el File > Open").
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
                "{} no parece un proyecto de Hikaru (extensión .{} ausente o contenido ilegible)",
                p.display(),
                PROJECT_EXTENSION
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

// =========================================================================
// EL ARCHIVO
// =========================================================================

/// Raíz del archivo `.hikaru`.
///
/// `format_version` es OBLIGATORIA y se valida antes de deserializar el resto:
/// es el primer campo del JSON justamente para poder cortocircuitar (ver
/// [`parse`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub format_version: u32,
    /// Versión del programa que escribió el archivo. Informativa: no se usa
    /// para migraciones (para eso está `format_version`).
    pub created_by: String,
    /// Ruta del sample de cada clip, relativa o absoluta, tal como quedó.
    pub transport: Transport,
    /// Vista/modo en que estaba el proyecto al guardarlo.
    pub ui: UiState,
    /// Pistas del modo OpenLive (master + canales de la Session Matrix).
    pub live_tracks: Vec<Track>,
    /// Pistas del modo OpenStudio (master + canales del Arranger/Mixer).
    pub studio_tracks: Vec<Track>,
    pub matrix: Matrix,
    pub playlist: Playlist,
}

// =========================================================================
// SNAPSHOTS
// =========================================================================

/// Transporte: todo lo que no es posición en el tiempo.
///
/// La posición (`sample_count`) NO se guarda: al abrir un proyecto arranca en
/// cero, que es lo que espera cualquiera que abra una sesión.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transport {
    pub bpm: f64,
    pub beats_per_bar: u32,
    pub beat_division: u32,
    /// Región de loop global en TICKS (no en samples: los samples dependen del
    /// sample rate de la máquina que abra el proyecto).
    pub loop_start_ticks: u64,
    pub loop_end_ticks: u64,
    pub loop_enabled: bool,
}

/// Modo y vistas activas. Se guardan para reabrir el proyecto donde estaba,
/// que es un detalle chico que se agradece mucho.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiState {
    pub mode: ModeTag,
    pub openlive_view: u8,
    pub openstudio_view: u8,
    pub show_dsp_rack: bool,
    pub show_explorer: bool,
}

/// Etiquetas de enum en el archivo.
///
/// Los enums de `app.rs` NO se serializan directo (quedaría atado el archivo a
/// las variantes del código). Van como `u8` con conversores explícitos en cada
/// lado: si mañana aparece `OpenStudioView::PianoRoll`, el archivo viejo sigue
/// leyéndose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModeTag {
    OpenLive,
    OpenStudio,
}

impl From<AppMode> for ModeTag {
    fn from(m: AppMode) -> Self {
        match m {
            AppMode::OpenLive => ModeTag::OpenLive,
            AppMode::OpenStudio => ModeTag::OpenStudio,
        }
    }
}

impl From<ModeTag> for AppMode {
    fn from(m: ModeTag) -> Self {
        match m {
            ModeTag::OpenLive => AppMode::OpenLive,
            ModeTag::OpenStudio => AppMode::OpenStudio,
        }
    }
}

/// Un canal del mixer o de la Session Matrix.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: usize,
    pub name: String,
    pub volume: f32,
    pub pan: f32,
    /// 0 = [`PanMode::Stereo`], 1 = [`PanMode::MidSide`].
    pub pan_mode: u8,
    pub mute: bool,
    pub solo: bool,
    pub arm: bool,
    pub is_master: bool,
    pub route_destination_id: usize,
    pub sends: Vec<Send>,
    pub effects: Vec<DspSlot>,
    /// Índice de fila en la Session Matrix, o `None` para el master (que no
    /// tiene fila propia).
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
/// ruta ([`DspSlot::wavetable_path`]) y se recarga al abrir, pero el resto de
/// los parámetros del panel (unison, detune, envolvente, cámara del visor)
/// todavía no entran en el formato. Cuando entren, es agregar campos acá: el
/// `DspSlot` del estado no se toca.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DspSlot {
    pub id: usize,
    pub name: String,
    pub active: bool,
    pub wavetable_path: Option<String>,
}

/// Estado de la Session Matrix (OPENLIVE).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Matrix {
    /// `tracks.len() == grid.len()` y `scenes.len() == grid[0].len()` siempre:
    /// la grilla es rectangular y las pistas son filas.
    pub tracks: Vec<TrackMeta>,
    pub scenes: Vec<SceneMeta>,
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
    /// Volumen/pan/mute del canalito PROPIO del clip (`MatrixClip::local_track`),
    /// que es distinto del canal de la pista que lo dispara.
    pub volume: f32,
    pub pan: f32,
    pub mute: bool,
    /// Eventos de audio apilados en el pad, en orden de arranque.
    pub events: Vec<Event>,
    /// Notas MIDI del clip, si el pad es MIDI. Vacío para pads de audio.
    pub midi_notes: Vec<MidiNote>,
}

/// Un sample dentro de un pad de la Session Matrix.
///
/// Se guarda TODO menos `samples`: los samples se vuelven a leer de
/// `source_path` al abrir el proyecto.
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

/// Estado de la Playlist / Timeline (OPENSTUDIO).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    /// Pares `(índice de pista, clip)`. Es la misma forma que usa
    /// `PlaylistState::clips`.
    pub clips: Vec<(usize, PlaylistClipSaved)>,
    pub ppqn: u64,
    pub next_clip_id: usize,
    pub loop_start_ticks: u64,
    pub loop_end_ticks: u64,
    pub loop_region_active: bool,
}

/// Clip de la Playlist.
///
/// Los picos de la waveform NO se guardan: se recalculan con
/// [`playlist::build_audio_clip`] al abrir, igual que cuando se arrastra un
/// sample desde el explorer.
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
/// Color en HSLA plano.
///
/// `Hsla` es de GPUI y no implementa `Serialize`; ida y vuelta con este
/// structito para no atar el archivo al tipo de la librería de UI.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Color {
    pub h: f32,
    pub s: f32,
    pub l: f32,
    pub a: f32,
}

impl From<Color32> for Color {
    fn from(c: Color32) -> Self {
        Color { h: c.h, s: c.s, l: c.l, a: c.a }
    }
}

impl From<Color> for Color32 {
    fn from(c: Color) -> Self {
        Color32 { h: c.h, s: c.s, l: c.l, a: c.a }
    }
}

// =========================================================================
// CAPTURE: ESTADO -> ARCHIVO
// =========================================================================

/// Toma un snapshot del estado actual.
///
/// Es una COPIA: sigue siendo válido aunque el usuario siga moviendo faders,
/// que es lo que pasa si se guarda desde un frame ya capturado.
pub fn capture(state: &AppState) -> Project {
    Project {
        format_version: FORMAT_VERSION,
        created_by: format!("Hikaru OpenLive {}", env!("CARGO_PKG_VERSION")),
        transport: Transport {
            bpm: state.transport.bpm,
            beats_per_bar: state.transport.beats_per_bar,
            beat_division: state.transport.beat_division,
            loop_start_ticks: state.transport.samples_to_ticks(state.transport.loop_start_samples),
            loop_end_ticks: state.transport.samples_to_ticks(state.transport.loop_end_samples),
            loop_enabled: state.transport.loop_enabled,
        },
        ui: UiState {
            mode: state.mode.into(),
            openlive_view: state.openlive_view as u8,
            openstudio_view: state.openstudio_view as u8,
            show_dsp_rack: state.show_dsp_rack,
            show_explorer: state.show_explorer,
        },
        live_tracks: state.live_tracks.iter().map(capture_track).collect(),
        studio_tracks: state.studio_tracks.iter().map(capture_track).collect(),
        // La matriz se captura tal cual. Los canales de OpenLive son los mismos
        // objetos en el mixer y en el header (el gesto escribe en los dos, ver
        // `sync_matrix_mixer_bidirectional`), así que no hace falta reconciliar
        // nada acá: lo que hay en uno está en el otro. Lo que SÍ pasa al abrir
        // es que la matriz gane, y por eso `apply` la aplica después del mixer.
        matrix: capture_matrix(&state.matrix_state),
        playlist: capture_playlist(&state.playlist_state),
    }
}

fn capture_track(t: &mixer::Track) -> Track {
    Track {
        id: t.id,
        name: t.name.clone(),
        volume: t.volume,
        pan: t.pan,
        pan_mode: match t.pan_mode {
            PanMode::Stereo => 0,
            PanMode::MidSide => 1,
        },
        mute: t.mute,
        solo: t.solo,
        arm: t.arm,
        is_master: t.is_master,
        route_destination_id: t.route_destination_id,
        sends: t
            .sends
            .iter()
            .map(|s| Send { target_id: s.target_id, amount: s.amount })
            .collect(),
        effects: t
            .effects
            .iter()
            .map(|e| DspSlot {
                id: e.id,
                name: e.name.clone(),
                active: e.active,
                wavetable_path: e
                    .wavetable
                    .table
                    .path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string()),
            })
            .collect(),
        matrix_idx: t.matrix_idx,
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
        color: clip.color.into(),
        kind,
    }
}

// =========================================================================
// APPLY: ARCHIVO -> ESTADO
// =========================================================================

/// Aplica un proyecto sobre el estado vivo.
///
/// Deja el motor sincronizado (BPM, loop global, clips de la playlist, volumen
/// /pan/mute/solo por pista), así que después de esto la app suena como el
/// proyecto guardado sin intervention del usuario.
///
/// `base_dir` es el directorio del archivo: las rutas relativas de los samples
/// se resuelven contra él para que un proyecto con sus samples en la misma
/// carpeta se pueda mover entero.
pub fn apply(state: &mut AppState, project: &Project, base_dir: Option<&Path>) {
    // --- Transporte -------------------------------------------------------
    // El BPM se manda al motor SIEMPRE: es lo que define el tempo con el que
    // corren los clips, y mandar un `SetBpm` es barato (una sola f64 por
    // comando, no estamos en el hilo de audio).
    let bpm = project.transport.bpm.clamp(crate::app::BPM_MIN, crate::app::BPM_MAX);
    state.transport.bpm = bpm;
    state.transport.beats_per_bar = project.transport.beats_per_bar.max(1);
    state.transport.beat_division = project.transport.beat_division.max(1);
    state.audio_proxy.send(GuiCommand::SetBpm(bpm as f32));

    // --- Pistas -----------------------------------------------------------
    state.live_tracks = project.live_tracks.iter().map(build_track).collect();
    state.studio_tracks = project.studio_tracks.iter().map(build_track).collect();
    if state.live_tracks.is_empty() {
        state.live_tracks.push(mixer::Track::new(0, "MASTER".to_string(), true));
    }
    if state.studio_tracks.is_empty() {
        state.studio_tracks.push(mixer::Track::new(0, "MASTER".to_string(), true));
        state.studio_tracks.push(mixer::Track::new(1, "TRACK 01".to_string(), false));
    }
    state.selected_track_index = state.selected_track_index.min(state.tracks().len().saturating_sub(1));

    // La matriz manda sobre el mixer del modo OpenLive (es el que se edita
    // desde los headers de la Session Matrix), así que se re-sincroniza en
    // ambos sentidos con los valores recién cargados.
    //
    // El PPQN se pasa explícito porque la matriz no tiene PPQN propio y el de
    // la playlist todavía no está aplicado en el estado en este punto.
    apply_matrix(state, &project.matrix, project.playlist.ppqn.max(1), base_dir);

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

    push_track_mix_to_engine(state);

    // --- Playlist ---------------------------------------------------------
    apply_playlist(state, &project.playlist, base_dir);

    // --- Vista ------------------------------------------------------------
    state.mode = project.ui.mode.into();
    state.openlive_view = openlive_view_from_u8(project.ui.openlive_view);
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

/// `matches!` de los enums de vista contra un `u8` del archivo. Desconocido =
/// el default, para que un archivo de una build futura no rompa la apertura.
fn openlive_view_from_u8(v: u8) -> OpenLiveView {
    match v {
        1 => OpenLiveView::ArrangerView,
        _ => OpenLiveView::SessionMatrix,
    }
}

fn openstudio_view_from_u8(v: u8) -> OpenStudioView {
    match v {
        1 => OpenStudioView::ArrangerMixer,
        _ => OpenStudioView::Playlist,
    }
}

fn build_track(t: &Track) -> mixer::Track {
    let mut track = mixer::Track::new(t.id, t.name.clone(), t.is_master);
    track.volume = t.volume;
    track.pan = t.pan;
    track.pan_mode = if t.pan_mode == 1 { PanMode::MidSide } else { PanMode::Stereo };
    track.mute = t.mute;
    track.solo = t.solo;
    track.arm = t.arm;
    track.route_destination_id = t.route_destination_id;
    track.matrix_idx = t.matrix_idx;
    track.sends = t
        .sends
        .iter()
        .map(|s| mixer::SendConnection { target_id: s.target_id, amount: s.amount })
        .collect();
    track.effects = t
        .effects
        .iter()
        .map(|e| {
            let mut slot = mixer::DspSlot::new(e.id, e.name.clone());
            slot.active = e.active;
            slot
        })
        .collect();
    track
}

/// Reenvía volumen/pan/mute/solo al motor.
///
/// Hace falta porque los fades del mixer son estado de GUI: el motor sólo se
/// entera por comando. [`crate::app::sync_matrix_mixer_bidirectional`] ya
/// manda algunos, pero no todos ni para las pistas de OpenStudio.
fn push_track_mix_to_engine(state: &AppState) {
    // Sólo el set del modo activo suena: el motor tiene un solo set de
    // canales y el otro modo está en silencio.
    for (idx, track) in state.tracks().iter().enumerate() {
        state.audio_proxy.send(GuiCommand::SetTrackVolume {
            track_idx: idx,
            volume_db: track.volume,
        });
        state.audio_proxy.send(GuiCommand::SetTrackPan {
            track_idx: idx,
            pan: track.pan,
        });
        state.audio_proxy.send(GuiCommand::SetTrackMute {
            track_idx: idx,
            mute: track.mute,
        });
        state.audio_proxy.send(GuiCommand::SetTrackSolo {
            track_idx: idx,
            solo: track.solo,
        });
    }
}

fn apply_matrix(state: &mut AppState, saved: &Matrix, ppqn: u64, base_dir: Option<&Path>) {
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
    let path = resolve_path(base_dir, &saved.path);

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
        let event_path = resolve_path(base_dir, &event.source_path);
        let Some((samples, channels, sr)) = matrix::decode_audio_file(&event_path) else {
            eprintln!(
                "[PROJECT] el sample {} del clip '{}' no se pudo leer; el pad queda sin ese evento",
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
                "[PROJECT] el sample {} del clip '{}' no se pudo leer al abrir el proyecto",
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
    let color: Color32 = saved.color.into();
    let clip_type = match &saved.kind {
        ClipKind::Pattern { pattern_id } => ClipType::Pattern {
            pattern_id: *pattern_id,
        },
        ClipKind::Audio {
            sample_path,
            sample_offset_ticks,
            total_sample_ticks,
        } => {
            let path = resolve_path(base_dir, sample_path);
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

/// Resuelve una ruta del archivo contra el directorio del proyecto.
///
/// Absolutas se usan tal cual; relativas cuelgan del proyecto. Es lo que hace
/// portable un `.hikaru` que se guarda junto a una carpeta `samples/`.
fn resolve_path(base_dir: Option<&Path>, raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        return path;
    }
    match base_dir {
        Some(dir) => dir.join(path),
        None => path,
    }
}

// =========================================================================
// PROYECTO EN BLANCO
// =========================================================================

/// La sesión con la que arranca la app, como [`Project`].
///
/// Sale de los `Default` de la matriz y la playlist (los mismos que usa
/// `HikaruApp::build`) en vez de estar escrita a mano: si mañana el default de
/// la Session Matrix pasa a 16 pistas, el `New Project` las tiene sin tocar este
/// archivo.
pub fn blank() -> Project {
    let playlist = capture_playlist(&PlaylistState::default());

    // El master + un canal por fila de la matriz, que es la correspondencia
    // que hace `sync_matrix_mixer_bidirectional`.
    let mut live_tracks = vec![Track {
        id: 0,
        name: "MASTER".to_string(),
        volume: 0.70,
        pan: 0.0,
        pan_mode: 0,
        mute: false,
        solo: false,
        arm: false,
        is_master: true,
        route_destination_id: 0,
        sends: Vec::new(),
        effects: Vec::new(),
        matrix_idx: None,
    }];
    let default_matrix = matrix::SessionMatrixState::default();
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

    let matrix = capture_matrix(&default_matrix);

    Project {
        format_version: FORMAT_VERSION,
        created_by: format!("Hikaru OpenLive {}", env!("CARGO_PKG_VERSION")),
        transport: Transport {
            bpm: 140.0,
            beats_per_bar: 4,
            beat_division: 4,
            loop_start_ticks: 0,
            loop_end_ticks: 0,
            loop_enabled: false,
        },
        ui: UiState {
            mode: ModeTag::OpenLive,
            openlive_view: 0,
            openstudio_view: 0,
            show_dsp_rack: false,
            show_explorer: false,
        },
        live_tracks,
        studio_tracks: vec![
            Track {
                id: 0,
                name: "MASTER".to_string(),
                volume: 0.70,
                pan: 0.0,
                pan_mode: 0,
                mute: false,
                solo: false,
                arm: false,
                is_master: true,
                route_destination_id: 0,
                sends: Vec::new(),
                effects: Vec::new(),
                matrix_idx: None,
            },
            Track {
                id: 1,
                name: "TRACK 01".to_string(),
                volume: 0.70,
                pan: 0.0,
                pan_mode: 0,
                mute: false,
                solo: false,
                arm: false,
                is_master: false,
                route_destination_id: 0,
                sends: Vec::new(),
                effects: Vec::new(),
                matrix_idx: None,
            },
        ],
        matrix,
        playlist,
    }
}

/// `File > New Project`: deja el estado como recién salido de la app.
///
/// Pasa por [`apply`] a propósito (y no escribiendo campos sueltos) para que
/// un proyecto en blanco y un proyecto guardado compartan el mismo
/// saneamiento: si mañana `apply` aprende a descartar clips huérfanos, el
/// `New Project` lo hereda.
pub fn reset(state: &mut AppState) {
    // El transporte arranca en cero: abrir un proyecto en el compás 47 sería
    // una sorpresa.
    state.transport.sample_count = 0;
    state.transport.playback_state = hikaru_transport::TransportPlaybackState::Stopped;
    apply(state, &blank(), None);
    state.playlist_state.needs_full_sync = false;
    state.selected_track_index = 1;
    state.selected_slot_index = 0;
}

// =========================================================================
// I/O
// =========================================================================

/// Serializa el estado y lo escribe en `path`.
///
/// Devuelve el camino escrito (puede diferir del pedido si se le agregó la
/// extensión).
pub fn save(state: &AppState, path: &Path) -> Result<PathBuf, ProjectError> {
    let path = with_extension(path);
    let project = capture(state);
    // JSON lindo a propósito: el `.hikaru` está pensado para leerse y diffearse,
    // y pesa kilobytes (no megabytes, porque el audio va por ruta).
    let json = serde_json::to_string_pretty(&project)?;
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    std::fs::write(&path, json)?;
    Ok(path)
}

/// Agrega `.hikaru` si el usuario lo escribió sin extensión.
fn with_extension(path: &Path) -> PathBuf {
    if path.extension().is_some() {
        path.to_path_buf()
    } else {
        let mut s = path.as_os_str().to_os_string();
        s.push(".");
        s.push(PROJECT_EXTENSION);
        PathBuf::from(s)
    }
}

/// Lee y valida un proyecto de disco.
pub fn parse(path: &Path) -> Result<Project, ProjectError> {
    let text = std::fs::read_to_string(path)?;

    // La versión se mira ANTES de deserializar el resto: si el archivo es de
    // otra generación, el error tiene que ser "versión" y no un `missing
    // field` de serde que no dice nada.
    #[derive(Deserialize)]
    struct VersionProbe {
        format_version: Option<u32>,
    }
    let probe: VersionProbe = serde_json::from_str(&text)
        .map_err(|_| ProjectError::NotAProject(path.to_path_buf()))?;
    let version = probe.format_version.ok_or_else(|| ProjectError::NotAProject(path.to_path_buf()))?;
    if version > FORMAT_VERSION {
        return Err(ProjectError::UnsupportedVersion {
            found: version,
            expected: FORMAT_VERSION,
        });
    }

    let project: Project = serde_json::from_str(&text)?;
    Ok(project)
}

/// Lee un proyecto y lo aplica sobre el estado vivo.
pub fn load(state: &mut AppState, path: &Path) -> Result<(), ProjectError> {
    let project = parse(path)?;
    let base_dir = path.parent().map(|p| p.to_path_buf());
    apply(state, &project, base_dir.as_deref());
    Ok(())
}

// =========================================================================
// DIÁLOGOS
// =========================================================================

/// Diálogo "guardar como". `None` si el usuario cancela.
pub fn ask_save_path(current: Option<&Path>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .set_title("Guardar proyecto Hikaru")
        .add_filter(
            "Proyecto Hikaru",
            &[PROJECT_EXTENSION, "json"],
        );
    if let Some(current) = current {
        if let Some(dir) = current.parent() {
            dialog = dialog.set_directory(dir);
        }
        if let Some(stem) = current.file_stem() {
            dialog = dialog.set_file_name(format!(
                "{}.{}",
                stem.to_string_lossy(),
                PROJECT_EXTENSION
            ));
        }
    }
    dialog.save_file()
}

/// Diálogo "abrir". `None` si el usuario cancela.
pub fn ask_open_path(current: Option<&Path>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .set_title("Abrir proyecto Hikaru")
        .add_filter("Proyecto Hikaru", &[PROJECT_EXTENSION, "json"]);
    if let Some(current) = current.and_then(|p| p.parent()) {
        dialog = dialog.set_directory(current);
    }
    dialog.pick_file()
}
