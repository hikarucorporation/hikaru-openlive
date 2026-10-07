// Pegamento entre el estado vivo de Hikaru y los formatos de archivo.
// GNU AGPLv3
// crates/hikaru_gui/src/project/common.rs
//
// =========================================================================
// QUÉ ES ESTE MÓDULO Y DÓNDE TERMINA LA FRONTERA DE LICENCIA
// =========================================================================
//
// Los ESQUEMAS de los archivos (.oplf / .opsf) viven en crates LGPLv3 aparte:
// `hikaru_openlive_file` y `hikaru_openstudio_file`, que no dependen de esta
// app y por lo tanto puede usar un DAW de código cerrado sin abrir el motor
// AGPL. Ver docs/hikaru-project-files/.
//
// Lo que queda ACÁ es sólo la conversión entre el estado vivo y esos structs:
// `mixer::Track` <-> `Track`, `MatrixClip` <-> `Clip`, `PlaylistState` <->
// `Playlist`. Eso sí necesita GPUI, el secuenciador y el proxy de audio, así
// que sí es AGPL.
//
// Si alguna vez un struct del formato necesita un tipo de esta app, la frontera
// se rompió: ese struct tiene que volver al crate LGPL con un tipo neutro.
//
// =========================================================================
// POR QUÉ NO `#[derive(Serialize)]` EN LOS ESTADOS VIVOS
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
// =========================================================================
// REGLAS COMUNES A `.oplf` Y `.opsf`
// =========================================================================
//
// 1. El audio NUNCA se embebe. Se guarda la RUTA del sample y, al abrir, el
//    sample se vuelve a decodificar del disco. Es lo que hace un DAW de verdad:
//    el proyecto es liviano y los samples siguen siendo editables con otra
//    herramienta.
// 2. Todo lo que es caché o derivado (picos de waveform, `preview_mix`) se
//    recalcula al cargar, no se guarda.

use std::path::{Path, PathBuf};

use hikaru_project_file::{Color, DspSlot};

use crate::app::PanMode;
use crate::views::mixer;
use crate::views::playlist::Color32;

/// Reexportado para que `src/project/` no tenga que saber de qué crate viene
/// cada tipo de archivo.
pub use hikaru_project_file::{ProjectError, Track, FORMAT_VERSION};

/// Convierte un canal del mixer al struct de archivo.
pub fn capture_track(t: &mixer::Track) -> Track {
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
            .map(|s| hikaru_project_file::Send { target_id: s.target_id, amount: s.amount })
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

/// Reconstruye un canal del mixer desde el struct de archivo.
pub fn build_track(t: &Track) -> mixer::Track {
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

/// Tracks de mezcla vacíos pero válidos.
///
/// Un archivo recién hecho, o uno al que le vaciaron la lista, no puede dejar
/// al motor sin ningún canal: sin master no hay salida.
pub fn default_live_tracks() -> Vec<Track> {
    vec![Track {
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
    }]
}

pub fn default_studio_tracks() -> Vec<Track> {
    vec![
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
    ]
}

// =========================================================================
// COLOR
// =========================================================================

/// `Color` es del crate LGPL y `Color32` es de GPUI: los dos son EXTRANOS a
/// esta app, así que no se puede `impl From<>` entre ellos (regla de orfandad).
/// Son funciones libres, entonces.
///
/// Que esta conversión viva del lado AGPL es la razón de que `Color` sea un
/// structito plano y no el `Hsla` de GPUI: un crate de formato que hubiera
/// metido GPUI adentro ya no sería reutilizable por un DAW de código cerrado.

/// Color del archivo -> color de GPUI.
pub fn color_to_gpui(c: Color) -> Color32 {
    Color32 { h: c.h, s: c.s, l: c.l, a: c.a }
}

/// Color de GPUI -> color del archivo.
pub fn color_from_gpui(c: Color32) -> Color {
    Color { h: c.h, s: c.s, l: c.l, a: c.a }
}

// =========================================================================
// RUTAS Y MOTOR
// =========================================================================

/// Resuelve una ruta del archivo contra el directorio del proyecto.
///
/// Absolutas se usan tal cual; relativas cuelgan del proyecto. Es lo que hace
/// portable un `.oplf`/`.opsf` que se guarda junto a una carpeta `samples/`.
pub fn resolve_path(base_dir: Option<&Path>, raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        return path;
    }
    match base_dir {
        Some(dir) => dir.join(path),
        None => path,
    }
}

/// Reenvía volumen/pan/mute/solo al motor.
///
/// Hace falta porque los fades del mixer son estado de GUI: el motor sólo se
/// entera por comando.
/// [`crate::app::sync_matrix_mixer_bidirectional`] ya manda algunos, pero no
/// todos ni para las pistas del modo que no está en la matriz.
pub fn push_track_mix_to_engine(state: &crate::app::AppState) {
    use crate::audio_proxy::GuiCommand;

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

/// El nombre del proyecto sale del nombre del archivo.
///
/// Es lo único que se sabe del proyecto en el momento de guardar, y deja
/// `metadata.title` con algo útil para el CLI sin obligar a primero_editarlo a
/// mano.
pub fn title_from_path(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

/// Borra el código muerto de una lista de tracks y garantiza un master.
pub fn tracks_or_default(saved: &[Track], fallback: fn() -> Vec<Track>) -> Vec<mixer::Track> {
    let mut out: Vec<mixer::Track> = saved.iter().map(build_track).collect();
    if out.is_empty() {
        out = fallback().iter().map(build_track).collect();
    }
    out
}
