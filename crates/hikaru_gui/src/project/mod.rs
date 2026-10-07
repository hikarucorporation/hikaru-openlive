// Formatos de proyecto de Hikaru: `.oplf` (OpenLive) y `.opsf` (OpenStudio).
// GNU AGPLv3
// crates/hikaru_gui/src/project/mod.rs
//
// =========================================================================
// POR QUÉ DOS FORMATOS Y NO UNO
// =========================================================================
//
// Existió un formato único, `.hikaru`, que guardaba las dos mitades de la app
// en el mismo archivo: la matriz de la Session Matrix Y el timeline del
// Arranger. Era un formato monolítico, y como tal tenía tres problemas:
//
//  1. Guardaba datos que no correspondían al modo en que se estaba trabajando.
//     Guardar desde la matriz arrastraba el timeline entero (y al revés), así
//     que la mitad del archivo era basura para la sesión que acababas de
//     guardar.
//  2. Era ilegible fuera de esta app. Un DAW de código cerrado que quisiera
//     leer una sesión de performance tenía que entender el timeline de un
//     study para poder descartar la parte que no le interesaba.
//  3. Su cabecera mentía: `created_by` decía "Hikaru OpenLive" incluso cuando
//     lo había escrito un proyecto de OpenStudio.
//
// Ahora:
//
//     .oplf  -> Session Matrix, live_tracks, transporte.  [OpenLive]
//     .opsf  -> Playlist/Timeline, ppqn, studio_tracks.  [OpenStudio]
//
// Son disjuntos y están tipados por separado (`openlive::OpenLiveProject` y
// `openstudio::OpenStudioProject`). No existe un struct `Project` genérico, y
// no se puede volver a agregar sin perder la garantía de que un archivo no
// mezcla las dos mitades. Ver docs/hikaru-project-files/.
//
// =========================================================================
// POR QUÉ MÓDULOS ESPEJO Y NO `#[derive(Serialize)]` EN LOS ESTADOS VIVOS
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
// Por eso cada archivo se describe con structs PROPIAS, que son el contrato
// público del formato. Hay conversión en los dos sentidos: `capture`
// (estado -> archivo) y `apply` (archivo -> estado). Si mañana cambia un campo
// del estado, sólo se toca esa capa.
//
// =========================================================================
// REGLAS DEL FORMATO (comunes a `.oplf` y `.opsf`)
// =========================================================================
//
// 1. El audio NUNCA se embebe. Se guarda la RUTA del sample y, al abrir, el
//    sample se vuelve a decodificar del disco. Es lo que hace un DAW de verdad:
//    el proyecto es liviano y los samples siguen siendo editables con otra
//    herramienta.
// 2. Todo lo que es caché o derivado (picos de waveform, `preview_mix`) se
//    recalcula al cargar, no se guarda.
// 3. Es un JSON con extensión `.oplf`/`.opsf`: se puede versionar en git,
//    diffear a ojo y reparar a mano. `FORMAT_VERSION` sube cuando cambie la
//    forma.

use std::path::{Path, PathBuf};

use crate::app::{AppMode, AppState};

pub mod common;
pub mod openlive;
pub mod openstudio;

pub use common::{ProjectError, FORMAT_VERSION};

/// Extensión del formato OpenLive (Session Matrix, performance en vivo).
///
/// Vive en el crate LGPL del formato, no acá: la extensión es parte de la
/// especificación, y un lector externo tiene que poder consultarla sin linkear
/// la app.
pub const OPENLIVE_EXTENSION: &str = hikaru_openlive_file::EXTENSION;

/// Extensión del formato OpenStudio (timeline lineal, arreglos).
pub const OPENSTUDIO_EXTENSION: &str = hikaru_openstudio_file::EXTENSION;

/// Etiqueta del filtro `rfd` de OpenLive.
///
/// El texto EXACTO importa: el OPLF-README.md §3.3 lo fija como contrato de la
/// UI. Cambiarlo rompe la expectation de quien integró la app con un script.
/// También sale del crate del formato, por el mismo motivo que la extensión.
const OPENLIVE_FILTER: &str = hikaru_openlive_file::FILE_FILTER;

/// Etiqueta del filtro `rfd` de OpenStudio.
const OPENSTUDIO_FILTER: &str = hikaru_openstudio_file::FILE_FILTER;

/// Extensiones que ofrece el diálogo. UNA por formato.
///
/// Que no haya alternativa (ni siquiera `.json`) no es estética: ofrecer `.json`
/// hacía que un texto sin extensión se presentara como proyecto válido y
/// fallara recién al abrirlo.
const OPENLIVE_EXTS: &[&str] = &[OPENLIVE_EXTENSION];
const OPENSTUDIO_EXTS: &[&str] = &[OPENSTUDIO_EXTENSION];

/// Formato que le toca al modo en que está la app.
fn extension_for(mode: AppMode) -> &'static str {
    match mode {
        AppMode::OpenLive => OPENLIVE_EXTENSION,
        AppMode::OpenStudio => OPENSTUDIO_EXTENSION,
    }
}

/// El formato de un archivo ya guardado, deducido de la extensión.
///
/// Es lo que permite abrir un `.opsf` estando en Modo OpenLive y viceversa: el
/// archivo manda, y [`load`] deja la app en el modo que corresponde.
fn format_of(path: &Path) -> Option<AppMode> {
    match path.extension().and_then(|e| e.to_str()) {
        Some(OPENLIVE_EXTENSION) => Some(AppMode::OpenLive),
        Some(OPENSTUDIO_EXTENSION) => Some(AppMode::OpenStudio),
        _ => None,
    }
}

/// Agrega la extensión si el usuario la escribió sin ella.
///
/// Si escribió otra extensión explícita se respeta: el `rfd` ya pone la
/// correcta, y pisar lo que el usuario tipeó a mano en el nombre sería peor
/// que aceptar un nombre raro.
fn with_extension(path: &Path, extension: &str) -> PathBuf {
    if path.extension().is_some() {
        return path.to_path_buf();
    }
    PathBuf::from(format!("{}.{}", path.display(), extension))
}

/// Corrige la extensión del camino para que sea la del modo.
///
/// Hace falta porque `File > Save` reusa el `project_path` guardado, y ese
/// camino puede ser de OTRO formato: el usuario abre un `.oplf`, cambia a
/// Modo OpenStudio y le da Save. Sin esto se escribiría un `.opsf` (con el
/// timeline adentro) dentro de un archivo llamado `sesion.oplf`, que al
/// reabrirse se interpretaría como OpenLive y perdería todo el arreglo.
///
/// Es un `File > Save As` implícito en la única situación en que el nombre
/// viejo no puede describir lo que se está guardando.
fn retarget_extension(path: &Path, mode: AppMode) -> PathBuf {
    let wanted = extension_for(mode);
    if path.extension().and_then(|e| e.to_str()) == Some(wanted) {
        return path.to_path_buf();
    }
    path.with_extension(wanted)
}

// =========================================================================
// GUARDAR
// =========================================================================

/// Serializa el estado y lo escribe en `path`, en el formato del MODO ACTUAL.
///
/// El modo decide el formato, no al revés: si el usuario está en la Session
/// Matrix, el archivo va a ser `.oplf` y contendrá SÓLO la matriz. Si está en
/// el Arranger, `.opsf` y SÓLO el timeline. Esa es la frontera de la
/// especificación, y es también la que evita la pérdida de datos que
/// produciría guardar el Arranger en un formato que no tiene dónde poner los
/// clips.
///
/// Devuelve el camino escrito (puede diferir del pedido si se le agregó la
/// extensión).
pub fn save(state: &AppState, path: &Path) -> Result<PathBuf, ProjectError> {
    let path = retarget_extension(path, state.mode);
    match state.mode {
        AppMode::OpenLive => openlive::save(state, &path),
        AppMode::OpenStudio => openstudio::save(state, &path),
    }
}

/// Guarda explícitamente en un formato dado, ignorando el modo.
///
/// Existe para los tests y para las herramientas de conversión, que sí saben
/// qué formato quieren.
pub fn save_as(state: &AppState, mode: AppMode, path: &Path) -> Result<PathBuf, ProjectError> {
    let path = retarget_extension(path, mode);
    match mode {
        AppMode::OpenLive => openlive::save(state, &path),
        AppMode::OpenStudio => openstudio::save(state, &path),
    }
}

// =========================================================================
// ABRIR
// =========================================================================

/// Lee un proyecto y lo aplica sobre el estado vivo.
///
/// El formato se saca de la EXTENSIÓN del archivo, así que se puede abrir un
/// `.opsf` estando en Modo OpenLive: la app cambia sola al modo que corresponde
/// al contenido (ver [`openlive::apply`] / [`openstudio::apply`]).
pub fn load(state: &mut AppState, path: &Path) -> Result<(), ProjectError> {
    let base_dir = path.parent().map(|p| p.to_path_buf());
    match format_of(path) {
        Some(AppMode::OpenLive) => {
            let project = openlive::parse(path)?;
            openlive::apply(state, &project, base_dir.as_deref());
        }
        Some(AppMode::OpenStudio) => {
            let project = openstudio::parse(path)?;
            openstudio::apply(state, &project, base_dir.as_deref());
        }
        None => return Err(ProjectError::NotAProject(path.to_path_buf())),
    }
    Ok(())
}

// =========================================================================
// PROYECTO EN BLANCO
// =========================================================================

/// `File > New Project`: deja el estado como recién salido de la app.
///
/// Cada módulo tiene su `blank()`, y acá se elige el del modo. Pasa por
/// `apply` a propósito (y no escribiendo campos sueltos) para que un proyecto
/// en blanco y un proyecto guardado compartan el mismo saneamiento: si mañana
/// `apply` aprende a descartar clips huérfanos, el `New Project` lo hereda.
pub fn reset(state: &mut AppState) {
    // El transporte arranca en cero: abrir un proyecto en el compás 47 sería
    // una sorpresa.
    state.transport.sample_count = 0;
    state.transport.playback_state = hikaru_transport::TransportPlaybackState::Stopped;
    match state.mode {
        AppMode::OpenLive => openlive::apply(state, &openlive::blank(), None),
        AppMode::OpenStudio => openstudio::apply(state, &openstudio::blank(), None),
    }
    state.playlist_state.needs_full_sync = false;
    state.selected_track_index = 1;
    state.selected_slot_index = 0;
}

// =========================================================================
// DIÁLOGOS
// =========================================================================

/// Etiqueta y extensiones del diálogo, según el modo.
///
/// Que el filtro sea ÚNICO no es un detalle de estética: ofrecer `.json` como
/// alternativa hacía que un archivo de texto sin extensión se presentara como
/// proyecto válido y fallara recién al abrirlo.
fn filter_for(mode: AppMode) -> (&'static str, &'static [&'static str]) {
    match mode {
        AppMode::OpenLive => (OPENLIVE_FILTER, OPENLIVE_EXTS),
        AppMode::OpenStudio => (OPENSTUDIO_FILTER, OPENSTUDIO_EXTS),
    }
}

/// Diálogo "guardar como". `None` si el usuario cancela.
///
/// `mode` es el modo en que está la app, y por lo tanto el formato (y la
/// extensión) que se ofrece.
pub fn ask_save_path(mode: AppMode, current: Option<&Path>) -> Option<PathBuf> {
    let (label, exts) = filter_for(mode);
    let extension = extension_for(mode);

    let mut dialog = rfd::FileDialog::new()
        .set_title("Guardar proyecto Hikaru")
        .add_filter(label, exts);
    if let Some(current) = current {
        if let Some(dir) = current.parent() {
            dialog = dialog.set_directory(dir);
        }
        if let Some(stem) = current.file_stem() {
            dialog = dialog.set_file_name(format!("{}.{}", stem.to_string_lossy(), extension));
        }
    }
    dialog.save_file()
}

/// Diálogo "abrir". `None` si el usuario cancela.
///
/// Ofrece LOS DOS formatos: un `.oplf` y un `.opsf` son ambos proyectos
/// válidos, y sólo el contenido del archivo dice cuál es cuál.
pub fn ask_open_path(current: Option<&Path>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .set_title("Abrir proyecto Hikaru")
        .add_filter(OPENLIVE_FILTER, OPENLIVE_EXTS)
        .add_filter(OPENSTUDIO_FILTER, OPENSTUDIO_EXTS);
    if let Some(current) = current.and_then(|p| p.parent()) {
        dialog = dialog.set_directory(current);
    }
    dialog.pick_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_extension_depende_del_modo() {
        assert_eq!(extension_for(AppMode::OpenLive), "oplf");
        assert_eq!(extension_for(AppMode::OpenStudio), "opsf");
    }

    #[test]
    fn el_formato_se_saca_de_la_extension_del_archivo() {
        assert_eq!(format_of(Path::new("a.oplf")), Some(AppMode::OpenLive));
        assert_eq!(format_of(Path::new("a.opsf")), Some(AppMode::OpenStudio));
        // Nada de esto es un proyecto: ni extensión, extensión de otra cosa,
        // ni la extensión legacy que este formato reemplaza.
        assert_eq!(format_of(Path::new("a")), None);
        assert_eq!(format_of(Path::new("a.json")), None);
        assert_eq!(format_of(Path::new("a.wav")), None);
        assert_eq!(format_of(Path::new("a.hikaru")), None);
    }

    #[test]
    fn con_extension_no_pisa_la_que_ya_esta() {
        assert_eq!(
            with_extension(Path::new("sesion"), "oplf"),
            PathBuf::from("sesion.oplf")
        );
        assert_eq!(
            with_extension(Path::new("sesion.oplf"), "oplf"),
            PathBuf::from("sesion.oplf")
        );
        // Si el usuario escribió algo con extensión, se respeta.
        assert_eq!(
            with_extension(Path::new("sesion.mio"), "oplf"),
            PathBuf::from("sesion.mio")
        );
    }

    #[test]
    fn cada_modo_ofrece_su_propio_filtro_y_ningun_otro() {
        let (live_label, live_exts) = filter_for(AppMode::OpenLive);
        assert_eq!(live_label, "Hikaru OpenLive Project (*.oplf)");
        assert_eq!(live_exts, &["oplf"]);

        let (studio_label, studio_exts) = filter_for(AppMode::OpenStudio);
        assert_eq!(studio_label, "Hikaru OpenStudio Project (*.opsf)");
        assert_eq!(studio_exts, &["opsf"]);
    }
}