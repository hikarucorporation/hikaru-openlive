use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

use gpui_kit::component::button::Button;
use gpui_kit::component::label::Label;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, HikaruApp};

pub use hikaru_plugin_host::PluginDescriptor as DiscoveredPlugin;
pub use hikaru_plugin_host::{HostParentKind, PluginFormat};
use hikaru_plugin_host::PluginInstance as _;

/// Comando del rack hacia el hilo de la ventana de un plugin abierto.
///
/// Viaja por `mpsc` sin cota: `send` no bloquea y el hilo lo drena con
/// `try_recv` en cada tick, así el gesto del usuario no espera al servidor
/// X11 ni viceversa.
#[derive(Debug, Clone)]
pub enum PluginWindowCommand {
    /// Un knob del rack movió un parámetro (id, normalizado 0..1).
    SetParam { id: u32, value: f32 },
    /// Cerrar la ventana (el usuario la cerró desde el DAW).
    Close,
}

/// Reduce el handle nativo de la ventana de GPUI al enum `Send` que el hilo
/// de la ventana entiende.
///
/// Se llama en el handler del click (ahí sí hay `&mut Window`): el render
/// puro no ve la ventana, así que el padre se captura en el gesto y viaja
/// como dato plano. Sin handle (o con error) → `Unknown` → flotante de
/// XWayland. La llamada es cualificada al trait a propósito: `Window` tiene
/// un método inherente `window_handle` que devuelve otro tipo y taparía al
/// del trait en resolución normal.
pub fn capture_host_parent(window: &mut gpui_kit::Window) -> HostParentKind {
    // Sin `as _`: la ruta cualificada de abajo necesita el NOMBRE del trait
    // en scope (el método inherente `Window::window_handle` lo taparía).
    use raw_window_handle::HasWindowHandle;
    <gpui_kit::Window as HasWindowHandle>::window_handle(window)
        .map(|handle| HostParentKind::from_raw(handle.as_raw()))
        .unwrap_or(HostParentKind::Unknown)
}

/// Lo que el editor nativo movió por su cuenta, hacia la UI.
///
/// Lo produce el hilo de la ventana sondeando `poll_param_changes` y lo
/// consume `AppState::apply_param_feedback` en el hilo principal, que mueve
/// los knobs del rack para que no mientan. `descriptor_id` (estable entre
/// escaneos) empareja con el slot aunque haya renombres de por medio.
#[derive(Debug, Clone)]
pub struct ParamFeedbackEvent {
    pub descriptor_id: String,
    pub changes: Vec<(u32, f32)>,
}

enum ScanMessage {
    /// Progreso del escaneo, sólo para la etiqueta de estado.
    Progress(String),
    Found(hikaru_plugin_host::PluginDescriptor),
    Finished {
        found: usize,
        errors: Vec<String>,
    },
}

struct PluginClosedNotification {
    name: String,
    /// Mensaje para la UI cuando la ventana nunca llegó a mostrar el editor
    /// (`None` = el usuario la cerró tras usarla con normalidad).
    ///
    /// Viaja por este canal —y no por `eprintln!`— porque el hilo de la
    /// ventana no puede tocar el estado de GPUI: `poll_updates` lo recoge en
    /// el hilo principal y lo guarda en `gui_errors`, donde el rack lo pinta.
    error: Option<String>,
}

pub struct PluginSettingsState {
    pub is_open: bool,
    /// Rutas donde buscar. Arranca con los directorios estándar de Linux; el
    /// usuario puede agregar más desde el panel.
    pub custom_paths: Vec<String>,
    pub status_message: String,
    pub is_scanning: bool,
    /// Plugins encontrados en el último escaneo.
    pub discovered_plugins: Vec<DiscoveredPlugin>,
    /// Rutas que se saltaron o fallaron. No bloquean el escaneo: se muestran
    /// para que un plugin que "no aparece" tenga una explicación.
    pub scan_errors: Vec<String>,

    tx: Sender<ScanMessage>,
    rx: Receiver<ScanMessage>,

    open_plugins: HashMap<String, std::thread::JoinHandle<()>>,
    close_rx: Receiver<PluginClosedNotification>,
    close_tx: Sender<PluginClosedNotification>,
    /// Último error de GUI por nombre de plugin, para pintarlo en la UI.
    ///
    /// Lo escribe `poll_updates` (hilo principal) al recibir una
    /// notificación con `error`. Un plugin roto queda así marcado en el
    /// panel y en su tarjeta del rack en vez de cerrar o colgar el DAW.
    pub gui_errors: HashMap<String, String>,
    /// Plugins con ventana en apertura (hilo lanzado, ventana aún no
    /// confirmada). La tarjeta muestra "Abriendo…" en vez de no decir nada
    /// durante el `dlopen`.
    opening: HashSet<String>,
    /// Comandos hacia cada ventana abierta (rack → editor nativo).
    cmd_tx: HashMap<String, Sender<PluginWindowCommand>>,
    /// Feedback del editor nativo hacia la UI (ventana → rack).
    param_tx: Sender<ParamFeedbackEvent>,
    param_rx: Receiver<ParamFeedbackEvent>,
    /// Feedback pendiente de aplicar a los slots. `poll_updates` lo acumula;
    /// `take_param_feedback` lo entrega a `AppState`, que es quien posee las
    /// pistas. El rodeo existe porque este estado no ve los tracks.
    pending_param_feedback: Vec<ParamFeedbackEvent>,
}

impl Default for PluginSettingsState {
    fn default() -> Self {
        let (tx, rx) = channel();
        let (close_tx, close_rx) = channel();
        let (param_tx, param_rx) = channel();
        Self {
            is_open: false,
            custom_paths: hikaru_plugin_host::registry::standard_search_paths()
                .iter()
                .map(|p| p.display().to_string())
                .collect(),
            status_message: "Listo para escanear.".to_string(),
            is_scanning: false,
            discovered_plugins: Vec::new(),
            scan_errors: Vec::new(),
            tx,
            rx,
            open_plugins: HashMap::new(),
            close_rx,
            close_tx,
            gui_errors: HashMap::new(),
            opening: HashSet::new(),
            cmd_tx: HashMap::new(),
            param_tx,
            param_rx,
            pending_param_feedback: Vec::new(),
        }
    }
}

impl PluginSettingsState {
    /// Lanza el escaneo en un hilo aparte.
    ///
    /// Es fundamental que no sea en el hilo de la GUI: recorrer `/usr/lib` con
    /// stat de cada entrada puede tardar cientos de ms, y hacerlo acá congelaría
    /// la ventana mientras el usuario espera.
    pub fn start_scan(&mut self) {
        if self.is_scanning {
            return;
        }

        self.is_scanning = true;
        self.discovered_plugins.clear();
        self.scan_errors.clear();
        self.status_message = "Iniciando escaneo de plugins...".to_string();

        let paths = self.custom_paths.clone();
        let tx = self.tx.clone();

        std::thread::spawn(move || {
            // Todo el trabajo real (recorrido, dedup, orden) vive en el
            // registry del host. Esta función sólo_TRANSPORTA los resultados
            // al hilo de la GUI por el canal.
            let report = hikaru_plugin_host::registry::scan_paths(paths);

            let found = report.plugin_count();
            let errors: Vec<String> =
                report.errors.iter().take(20).map(|e| e.to_string()).collect();

            for descriptor in report.plugins {
                let _ = tx.send(ScanMessage::Found(descriptor));
            }

            let _ = tx.send(ScanMessage::Finished { found, errors });
        });
    }

    /// Drena los resultados del escaneo. La tiene que llamar el render loop de
    /// la app: el canal es un `mpsc` común, no asíncrono, así que los mensajes
    /// se quedan encolados hasta que alguien los lea.
    pub fn poll_updates(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                ScanMessage::Progress(file) => {
                    self.status_message = format!("Escaneando: {}", file);
                }
                ScanMessage::Found(plugin) => {
                    // Un mismo plugin puede aparecer dos veces si el usuario
                    // agregó un directorio que se solapa con uno por defecto.
                    // El id es único por plugin, así que deduplicar por id es
                    // exacto (a diferencia de comparar por nombre).
                    if !self.discovered_plugins.iter().any(|p| p.id == plugin.id) {
                        self.discovered_plugins.push(plugin);
                    }
                }
                ScanMessage::Finished { found, errors } => {
                    self.is_scanning = false;
                    self.scan_errors = errors;
                    self.status_message = if found == 0 {
                        "Escaneo finalizado. No se encontraron plugins.".to_string()
                    } else {
                        format!("Escaneo finalizado. {} plugins listos.", found)
                    };
                }
            }
        }

        let mut closed = Vec::new();
        while let Ok(notif) = self.close_rx.try_recv() {
            closed.push(notif);
        }
        for notif in closed {
            self.open_plugins.remove(&notif.name);
            self.cmd_tx.remove(&notif.name);
            self.opening.remove(&notif.name);
            match notif.error {
                Some(message) => {
                    eprintln!("[PluginSettings] GUI de '{}' falló: {}", notif.name, message);
                    self.gui_errors.insert(notif.name, message);
                }
                None => {
                    println!("[PluginSettings] Plugin '{}' window closed", notif.name);
                }
            }
        }

        while let Ok(event) = self.param_rx.try_recv() {
            self.pending_param_feedback.push(event);
        }
    }

    /// Feedback del editor nativo pendiente de aplicar a los slots. Lo llama
    /// `sync_frame` y lo entrega a `AppState::apply_param_feedback`.
    pub fn take_param_feedback(&mut self) -> Vec<ParamFeedbackEvent> {
        std::mem::take(&mut self.pending_param_feedback)
    }


    pub fn open_plugin_names(&self) -> Vec<String> {
        self.open_plugins.keys().cloned().collect()
    }

    pub fn is_plugin_open(&self, name: &str) -> bool {
        self.open_plugins.contains_key(name)
    }

    /// Si hay **alguna** ventana de plugin abierta.
    ///
    /// Existe para las rutas calientes del arrastre de un knob: clonar el
    /// nombre del plugin para preguntar si su ventana está abierta cuesta una
    /// alloc por píxel, y esto contesta lo mismo con un `HashMap::is_empty`.
    pub fn any_plugin_open(&self) -> bool {
        !self.open_plugins.is_empty()
    }

    /// Si la ventana está en apertura (hilo lanzado, editor aún no
    /// confirmado). La tarjeta muestra "Abriendo…" en ese estado.
    pub fn is_plugin_opening(&self, name: &str) -> bool {
        self.opening.contains(name)
    }

    /// Manda un cambio de parámetro de un knob del rack a la ventana abierta
    /// del plugin (sincronía visual con el editor nativo). No bloquea: el
    /// canal es `std::mpsc` sin cota (`send` no espera) y si la ventana ya
    /// cayó el error se ignora (la caché del slot, que sí guarda todo,
    /// manda para el motor).
    pub fn send_param_to_open_plugin(&self, name: &str, id: u32, value: f32) {
        if let Some(tx) = self.cmd_tx.get(name) {
            let _ = tx.send(PluginWindowCommand::SetParam { id, value });
        }
    }

    /// Último error de GUI de un plugin, si su ventana falló al abrir.
    pub fn gui_error_for(&self, name: &str) -> Option<String> {
        self.gui_errors.get(name).cloned()
    }

    pub fn clear_gui_error(&mut self, name: &str) {
        self.gui_errors.remove(name);
    }

    /// Abre el editor nativo en su propio hilo con el padre del DAW.
    ///
    /// Vuelve de inmediato: el `dlopen`, la creación X11 y el embebido corren
    /// en el hilo hijo, así el hilo principal nunca se cuelga aunque el
    /// plugin tarde (o muera) al abrir. `parent` es el handle nativo de la
    /// ventana de GPUI capturado en el click: en X11 la ventana del plugin
    /// nace hija de la del DAW (transient, centrada); en Wayland cae al
    /// flotante de XWayland porque una superficie Wayland no puede
    /// emparentar ventanas X11.
    pub fn open_plugin(
        &mut self,
        plugin: &DiscoveredPlugin,
        parent: HostParentKind,
        tap_key: hikaru_plugin_host::tap::TapKey,
    ) {
        if self.open_plugins.contains_key(&plugin.name) || self.opening.contains(&plugin.name) {
            println!(
                "[PluginSettings] '{}' ya tiene una ventana abierta o en apertura",
                plugin.name
            );
            return;
        }

        // Reintentar limpia el error anterior: era de otro intento y ya no
        // aplica. Si vuelve a fallar, el hilo mandará uno nuevo.
        self.gui_errors.remove(&plugin.name);

        let plugin_path = plugin.path.clone();
        let plugin_name = plugin.name.clone();
        let descriptor_id = plugin.id.clone();
        let format = plugin.format;
        let close_tx = self.close_tx.clone();
        let param_tx = self.param_tx.clone();
        let (cmd_tx, cmd_rx) = channel();

        self.opening.insert(plugin.name.clone());
        self.cmd_tx.insert(plugin.name.clone(), cmd_tx);

        let handle = std::thread::spawn(move || {
            run_plugin_window(
                plugin_path,
                plugin_name,
                descriptor_id,
                format,
                parent,
                cmd_rx,
                param_tx,
                close_tx,
                tap_key,
            );
        });

        self.open_plugins.insert(plugin.name.clone(), handle);
    }

    pub fn close_plugin(&mut self, name: &str) {
        // Pedir cierre elegante primero: el hilo destruye el editor con el
        // padre aún vivo en vez de que le maten la conexión por debajo.
        if let Some(tx) = self.cmd_tx.remove(name) {
            let _ = tx.send(PluginWindowCommand::Close);
        }
        self.opening.remove(name);
        if let Some(handle) = self.open_plugins.remove(name) {
            println!("[PluginSettings] Closing plugin '{}' (thread will exit)", name);
            let _ = handle;
        }
    }

}

/// Hilo dueño de la ventana nativa de un plugin externo.
///
/// Ciclo de vida (el orden importa, es lo que estaba roto):
///
/// 1. Conectar a X11 (XWayland bajo Wayland) y crear la ventana **sin
///    mapear**, hija de la ventana del DAW cuando la sesión es X11
///    (`parent.x11_id()`): transient para el DAW y centrada sobre él, así no
///    es un huérfano del WM sino una tool-window del DAW. En Wayland no hay
///    XID que emparentar y cae al top-level de XWayland. Nada visible
///    todavía: si la carga falla, no hay flash de ventana vacía.
/// 2. Cargar el binario (`dlopen`). Si falla, destruir la ventana invisible
///    y avisar a la UI con el motivo: el usuario ve el error en el rack, el
///    hilo principal del DAW ni se entera.
/// 3. Preguntar el tamaño preferido y embebido (`show_gui_embedded`, que
///    devuelve `Result`). Sólo si el plugin confirma que su editor quedó
///    adjunto (`is_gui_open`) se mapea la ventana.
/// 4. Bucle por sondeo (nunca bloqueante ni con `unwrap`): drena los
///    comandos del rack (`SetParam`/`Close`), los eventos X11 y cada ~100ms
///    los cambios que el editor nativo hizo por su cuenta (feedback a la UI
///    para mover los knobs del rack). Un error del servidor cierra la
///    ventana, nunca el DAW.
/// 5. Al salir: `hide_gui()` **antes** de destruir la ventana (el editor
///    tiene que desengancharse mientras su padre sigue vivo), y después
///    `destroy_window`. El `Drop` de la conexión cierra el contexto gráfico
///    en último lugar, así ni el plugin ni el host pisan un XID muerto.
///
/// Todo fallo manda `PluginClosedNotification { error: Some(..) }`; el
/// cierre normal del usuario manda `error: None`.
fn run_plugin_window(
    plugin_path: PathBuf,
    plugin_name: String,
    descriptor_id: String,
    format: PluginFormat,
    parent: HostParentKind,
    cmd_rx: Receiver<PluginWindowCommand>,
    param_tx: Sender<ParamFeedbackEvent>,
    close_tx: Sender<PluginClosedNotification>,
    tap_key: hikaru_plugin_host::tap::TapKey,
) {
    use raw_window_handle::{RawWindowHandle, XlibWindowHandle};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::*;
    use x11rb::protocol::Event;

    let notify = |error: Option<String>| {
        let _ = close_tx.send(PluginClosedNotification {
            name: plugin_name.clone(),
            error,
        });
    };
    let fail = |message: String| {
        eprintln!("[PluginWindow] '{}': {}", plugin_name, message);
        notify(Some(message));
    };

    // --- 1. Conexión X11. Vive hasta el final de la función: es lo que
    // mantiene vivo el contexto gráfico y la ventana.
    let (conn, screen_num) = match x11rb::rust_connection::RustConnection::connect(None) {
        Ok(c) => c,
        Err(e) => {
            fail(format!(
                "Sin conexión X11/XWayland, no se puede abrir su ventana: {:?}. \
                 Si estás en Wayland puro, revisa que XWayland esté activo.",
                e
            ));
            return;
        }
    };
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;
    let screen_w = screen.width_in_pixels as i16;
    let screen_h = screen.height_in_pixels as i16;

    // Padre del DAW cuando la sesión es X11: la ventana del plugin nace como
    // hija suya (una ventana X11 puede tener por padre la de otro cliente de
    // la misma DISPLAY). Se usa su geometría para centrar y su depth para no
    // pedir un visual incompatible.
    let daw_parent: Option<u32> = parent.x11_id();
    let (parent_geom, parent_depth): (Option<(i16, i16)>, u8) = match daw_parent {
        Some(xid) => match conn.get_geometry(xid) {
            Ok(cookie) => match cookie.reply() {
                Ok(geom) => (Some((geom.width as i16, geom.height as i16)), geom.depth),
                Err(_) => (None, screen.root_depth),
            },
            Err(_) => (None, screen.root_depth),
        },
        None => (None, screen.root_depth),
    };
    if daw_parent.is_some() && parent_geom.is_none() {
        eprintln!(
            "[PluginWindow] El XID del DAW no respondió a GetGeometry; se usa top-level."
        );
    }

    // --- 2. Ventana SIN mapear. Sin `RESIZE_REDIRECT` (esa máscara pide al
    // servidor redirigir los resizes y bajo algunos WMs dejaba la ventana
    // sin mapear para siempre: la ventana "no mostraba nada").
    let window = match conn.generate_id() {
        Ok(id) => id,
        Err(e) => {
            fail(format!("No se pudo reservar la ventana nativa: {:?}", e));
            return;
        }
    };
    // Si algo falla a partir de acá hay que destruir la ventana creada.
    // (`window_created` sólo existe para no destruir dos veces lo que el
    // servidor ya destruyó vía DestroyNotify.)
    let width: u16 = 1024;
    let height: u16 = 700;
    // Posición: centrada sobre el DAW si es hija suya (coords relativas al
    // padre), o sobre la pantalla si es top-level (coords relativas a root).
    let (pos_x, pos_y) = match (daw_parent, parent_geom) {
        (Some(_), Some((pw, ph))) => (
            ((pw - width as i16) / 2).max(0),
            ((ph - height as i16) / 2).max(0),
        ),
        _ => (
            ((screen_w - width as i16) / 2).max(0),
            ((screen_h - height as i16) / 2).max(0),
        ),
    };
    let window_parent = match (daw_parent, parent_geom) {
        (Some(xid), Some(_)) => xid,
        _ => root,
    };
    let window_aux = CreateWindowAux::new()
        .event_mask(EventMask::EXPOSURE | EventMask::KEY_PRESS | EventMask::STRUCTURE_NOTIFY);

    if let Err(e) = conn.create_window(
        parent_depth,
        window,
        window_parent,
        pos_x,
        pos_y,
        width,
        height,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &window_aux,
    ) {
        fail(format!("No se pudo crear la ventana nativa: {:?}", e));
        return;
    }
    let mut window_created = true;

    // Metadatos de ventana (best-effort: si fallan, la ventana sigue válida).
    let title = format!("{} — Hikaru", plugin_name);
    if let Ok(cookie) = conn.intern_atom(false, b"_NET_WM_NAME") {
        if let Ok(reply) = cookie.reply() {
            let _ = conn.change_property(
                PropMode::REPLACE,
                window,
                reply.atom,
                u32::from(AtomEnum::STRING),
                8,
                title.len() as u32,
                title.as_bytes(),
            );
        }
    }
    if let Ok(cookie) = conn.intern_atom(false, b"WM_CLASS") {
        if let Ok(reply) = cookie.reply() {
            let class = b"hikaru-plugin\0Hikaru\0";
            let _ = conn.change_property(
                PropMode::REPLACE,
                window,
                reply.atom,
                u32::from(AtomEnum::STRING),
                8,
                class.len() as u32,
                &class[..],
            );
        }
    }
    // Protocolo de cierre: se guarda el átomo para el bucle. Si no se puede
    // negociar, la ventana se cierra igual por DestroyNotify.
    let wm_delete: Option<u32> = conn
        .intern_atom(false, b"WM_DELETE_WINDOW")
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map(|reply| {
            let atom = reply.atom;
            if let Ok(proto_cookie) = conn.intern_atom(false, b"WM_PROTOCOLS") {
                if let Ok(proto_reply) = proto_cookie.reply() {
                    let data = atom.to_ne_bytes();
                    let _ = conn.change_property(
                        PropMode::REPLACE,
                        window,
                        proto_reply.atom,
                        u32::from(AtomEnum::ATOM),
                        32,
                        1,
                        &data[..],
                    );
                }
            }
            atom
        });

    // Emparentado con el DAW (sólo X11): transient + tipo diálogo para que el
    // WM la apile, minimice y enfoque junto a la ventana principal en vez de
    // tratarla como una app ajena. Todo best-effort: sin esto la ventana
    // sigue abriendo, sólo menos integrada.
    if let Some(daw) = daw_parent {
        if let Ok(cookie) = conn.intern_atom(false, b"WM_TRANSIENT_FOR") {
            if let Ok(reply) = cookie.reply() {
                let data = daw.to_ne_bytes();
                let _ = conn.change_property(
                    PropMode::REPLACE,
                    window,
                    reply.atom,
                    u32::from(AtomEnum::WINDOW),
                    32,
                    1,
                    &data[..],
                );
            }
        }
        if let (Ok(type_cookie), Ok(dialog_cookie)) = (
            conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE"),
            conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE_DIALOG"),
        ) {
            if let (Ok(type_reply), Ok(dialog_reply)) =
                (type_cookie.reply(), dialog_cookie.reply())
            {
                let data = dialog_reply.atom.to_ne_bytes();
                let _ = conn.change_property(
                    PropMode::REPLACE,
                    window,
                    type_reply.atom,
                    u32::from(AtomEnum::ATOM),
                    32,
                    1,
                    &data[..],
                );
            }
        }
        println!(
            "[PluginWindow] Ventana {} emparentada al DAW (XID {})",
            window, daw
        );
    } else {
        println!(
            "[PluginWindow] Sin padre X11 (Wayland u otro): ventana flotante de XWayland."
        );
    }

    // --- 3. Cargar el binario. Es lo que más falla (arquitectura, símbolos,
    // dependencias): por eso la ventana sigue invisible.
    let instance: Option<Box<dyn hikaru_plugin_host::PluginInstance>> = match format {
        PluginFormat::CLAP => match hikaru_plugin_host::ClapInstance::load(&plugin_path) {
            Ok(inst) => {
                println!("[PluginWindow] CLAP cargado para '{}'", plugin_name);
                Some(Box::new(inst))
            }
            Err(e) => {
                let _ = conn.destroy_window(window);
                let _ = conn.flush();
                fail(format!("No se pudo cargar el plugin: {}", e));
                return;
            }
        },
        PluginFormat::VST3 => match hikaru_plugin_host::Vst3Instance::load(&plugin_path) {
            Ok(inst) => {
                println!("[PluginWindow] VST3 cargado para '{}'", plugin_name);
                Some(Box::new(inst))
            }
            Err(e) => {
                let _ = conn.destroy_window(window);
                let _ = conn.flush();
                fail(format!("No se pudo cargar el plugin: {}", e));
                return;
            }
        },
    };
    let mut inst = match instance {
        Some(i) => i,
        None => {
            let _ = conn.destroy_window(window);
            let _ = conn.flush();
            fail("El plugin no devolvió una instancia válida.".to_string());
            return;
        }
    };

    // Cinta de audio del motor (tape bus).
    //
    // Esta ventana corre con una instancia PROPIA del plugin —la que suena
    // vive dentro de `DspPluginNode`, en el hilo de audio— así que sin esto el
    // editor no recibe un solo sample: los knobs andan (son derivados de
    // parámetro) pero el display reactivo al audio queda plano para siempre.
    //
    // Con el tap, el motor publica en cada bloque el buffer ya procesado y acá
    // se lo pasamos a la instancia de la ventana. Es el mismo patrón que un
    // host mandando la entrada del plugin a su propio editor.
    //
    // La tasa sale del tap y no de una constante: VST3 sella la frecuencia en
    // `setupProcessing`, así que si esta instancia arrancara a 44100 mientras
    // el motor corre a 48000 el display se dibujaría con la escala mal.
    let tap = hikaru_plugin_host::tap::lookup(tap_key);
    let mut feed: Option<(std::sync::Arc<hikaru_plugin_host::PluginTap>, Vec<f32>)> = match &tap {
        Some(t) => {
            println!(
                "[PluginWindow] '{}' alimentado desde el motor a {} Hz (tape bus)",
                plugin_name,
                t.sample_rate() as u32
            );
            Some((std::sync::Arc::clone(t), Vec::new()))
        }
        None => {
            // El slot no tiene plugin en el motor, o ya se quitó. La ventana
            // igual sirve para editar parámetros; sólo no hay display reactivo
            // al audio.
            println!(
                "[PluginWindow] '{}' sin plugin en el motor: el editor abre, pero sin audio (display plano)",
                plugin_name
            );
            None
        }
    };
    if let Some((t, _)) = &feed {
        inst.set_sample_rate(t.sample_rate());
    }

    // Tamaño preferido conocido antes de embebido (VST3 lo sabe; CLAP lo
    // confirma después en `notify_gui_embedded`).
    if let Some((pref_w, pref_h)) = inst.get_gui_size() {
        if pref_w > 0 && pref_h > 0 {
            let values = ConfigureWindowAux::new()
                .width(pref_w)
                .height(pref_h);
            let _ = conn.configure_window(window, &values);
            let _ = conn.flush();
        }
    }

    // --- 4. Embebido. Si el plugin lo rechaza, la ventana nunca se muestra:
    // se destruye invisible y la UI pinta el motivo en el rack.
    let xlib_handle = XlibWindowHandle::new(window as u64);
    let raw_handle = RawWindowHandle::Xlib(xlib_handle);
    if let Err(e) = inst.show_gui_embedded(raw_handle) {
        inst.hide_gui();
        let _ = conn.destroy_window(window);
        let _ = conn.flush();
        fail(e);
        return;
    }
    if !inst.is_gui_open() {
        eprintln!(
            "[PluginWindow] '{}' aceptó el embebido pero no confirma editor abierto",
            plugin_name
        );
        inst.hide_gui();
        let _ = conn.destroy_window(window);
        let _ = conn.flush();
        fail("El plugin no abrió su editor (lo rechazó sin dar un motivo).".to_string());
        return;
    }

    if let Some((actual_w, actual_h)) = inst.notify_gui_embedded() {
        if actual_w > 0 && actual_h > 0 {
            let values = ConfigureWindowAux::new()
                .width(actual_w)
                .height(actual_h);
            let _ = conn.configure_window(window, &values);
            let _ = conn.flush();
            println!(
                "[PluginWindow] Ventana ajustada al tamaño del editor: {}x{}",
                actual_w, actual_h
            );
        }
    }

    // --- 5. Recién ahora se muestra: el primer frame ya tiene contenido del
    // plugin detrás, no una ventana vacía.
    if let Err(e) = conn.map_window(window) {
        inst.hide_gui();
        let _ = conn.destroy_window(window);
        let _ = conn.flush();
        fail(format!("La ventana nativa no se pudo mostrar: {:?}", e));
        return;
    }
    if let Err(e) = conn.flush() {
        eprintln!("[PluginWindow] Aviso: flush tras map falló: {:?}", e);
    }
    println!(
        "[PluginWindow] Ventana {} visible para '{}'",
        window, plugin_name
    );

    // El consumidor se conecta recién acá, con la ventana ya visible y el
    // editor ya embebido. Antes habría que acordarse de desconectarlo en cada
    // `return` temprano (el fallo del `dlopen` es el caso normal, no el
    // excepcional), y un consumidor colgado hace que el motor siga copiando
    // bloques al pedo.
    if let Some((t, _)) = &feed {
        t.attach_consumer();
    }

    // --- 6. Bucle por sondeo (10ms): nunca bloquea ni paniquea. Drena tres
    // fuentes sin que ninguna muera de hambre: comandos del rack (un knob
    // movido llega al editor nativo en el siguiente tick), eventos X11 y, cada
    // ~100ms, lo que el editor movió por su cuenta (feedback a los knobs del
    // rack). Un error del servidor cierra la ventana, nunca el DAW (un panic
    // dejaría la entrada de `open_plugins` colgada y la ventana zombie).
    // Nota: esto NO toca el hilo principal ni el de audio; todo vive acá.
    let mut tick: u64 = 0;
    loop {
        // 6a. Comandos del rack. `try_recv` no bloquea: el gesto del usuario
        // no espera al servidor X11 ni viceversa.
        let mut close_requested = false;
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                PluginWindowCommand::SetParam { id, value } => {
                    if !inst.set_param_normalized(id, value) {
                        eprintln!(
                            "[PluginWindow] '{}' rechazó set_param(id={id})",
                            plugin_name
                        );
                    }
                }
                PluginWindowCommand::Close => {
                    println!(
                        "[PluginWindow] Cierre pedido desde el DAW para '{}'",
                        plugin_name
                    );
                    close_requested = true;
                }
            }
        }
        if close_requested {
            break;
        }

        // 6a-bis. Audio del motor → esta instancia del plugin.
        //
        // Es lo que hace que el display del editor se mueva. Sin este paso el
        // plugin de la ventana nunca recibe un bloque y su espectro/oscope
        // queda permanentemente plano aunque el audio suene perfecto en la
        // cadena.
        //
        // La salida se descarta a propósito: quien produce el audio es el
        // motor, con su propia instancia. Acá sólo se necesita que el plugin
        // "escuche" para que su medidor reaccione.
        //
        // Ojo: esto corre en el hilo de la ventana, no en el de audio, así que
        // un plugin lentísimo estira el tick — de ahí que la cinta descarte
        // bloques en vez de acumular (ver `PluginTap::publish`).
        if let Some((t, scratch)) = &mut feed {
            if t.take(scratch) {
                inst.process(&mut hikaru_core::AudioBuffer::new(scratch.as_mut_slice()));
            }
        }

        // 6b. Eventos X11 pendientes (tampoco bloquea).
        let mut user_closed = false;
        loop {
            match conn.poll_for_event() {
                Ok(None) => break,
                Ok(Some(Event::ClientMessage(msg))) => {
                    if wm_delete.is_some_and(|atom| msg.type_ == atom) {
                        println!(
                            "[PluginWindow] Cierre pedido por el WM para '{}'",
                            plugin_name
                        );
                        user_closed = true;
                        break;
                    }
                }
                Ok(Some(Event::DestroyNotify(ev))) => {
                    if ev.window == window {
                        println!(
                            "[PluginWindow] Ventana destruida desde fuera para '{}'",
                            plugin_name
                        );
                        window_created = false;
                        user_closed = true;
                        break;
                    }
                }
                Ok(Some(Event::ConfigureNotify(ev))) => {
                    if ev.window == window && ev.width > 0 && ev.height > 0 {
                        inst.resize_gui(ev.width as u32, ev.height as u32);
                    }
                }
                Ok(Some(_)) => {}
                Err(e) => {
                    eprintln!(
                        "[PluginWindow] Conexión X11 perdida para '{}': {:?}",
                        plugin_name, e
                    );
                    window_created = false;
                    user_closed = true;
                    break;
                }
            }
        }
        if user_closed {
            break;
        }

        // 6c. Feedback del editor nativo (~100ms): lo que el usuario movió en
        // la ventana del plugin viaja a la UI para mover los knobs del rack.
        // Se manda aunque esté vacío? No: sólo con cambios, para no spamear
        // el canal y despertar al hilo principal en cada tick.
        tick += 1;
        if tick % 10 == 0 {
            let changes = inst.poll_param_changes();
            if !changes.is_empty() {
                let _ = param_tx.send(ParamFeedbackEvent {
                    descriptor_id: descriptor_id.clone(),
                    changes,
                });
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    // --- 7. Apagado en orden: primero el editor (padre aún vivo), después
    // la ventana, y la conexión cae con la función. `hide_gui` es
    // idempotente y `Drop` la repite sin daño.
    //
    // El consumidor de la cinta se suelta antes que el editor: si no, el motor
    // seguiría Publicando bloques hacia una ventana que ya está apagándose.
    if let Some((t, _)) = &feed {
        let dropped = t.dropped_blocks();
        if dropped > 0 {
            eprintln!(
                "[PluginWindow] '{}': la cinta del motor tiró {} bloques (esta ventana no llegó a procesarlos)",
                plugin_name, dropped
            );
        }
        t.detach_consumer();
    }
    inst.hide_gui();
    if window_created {
        let _ = conn.destroy_window(window);
        let _ = conn.flush();
    }
    drop(inst);

    notify(None);
}

pub fn render(cx: &mut Context<HikaruApp>) -> AnyElement {
    let app = state(cx).read(cx);
    let is_open = app.plugin_settings_state.is_open;
    let paths = app.plugin_settings_state.custom_paths.clone();
    let status = app.plugin_settings_state.status_message.clone();
    let is_scanning = app.plugin_settings_state.is_scanning;
    let plugins = app.plugin_settings_state.discovered_plugins.clone();
    let scan_errors = app.plugin_settings_state.scan_errors.clone();
    let open_names: Vec<String> = app
        .plugin_settings_state
        .open_plugin_names();
    let gui_errors = app.plugin_settings_state.gui_errors.clone();
    drop(app);

    if !is_open {
        return div().into_any_element();
    }

    let mut path_rows: Vec<AnyElement> = Vec::new();
    for (idx, p) in paths.iter().enumerate() {
        path_rows.push(
            h_flex()
                .w_full()
                .gap(px(6.0))
                .items_center()
                .py(px(2.0))
                .px(px(6.0))
                .bg(rgb(0x20232C))
                .rounded(px(4.0))
                .child(
                    Label::new(p.clone())
                        .text_xs()
                        .truncate()
                        .flex_1()
                        .text_color(rgb(0xB8BCC8)),
                )
                .child(
                    Button::new(format!("plugin_path_remove_{}", idx)).rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("✕")
                        .compact()
                        .flex_shrink_0()
                        .w(px(28.0))
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            cx.update_entity(&st, |state, cx| {
                                state.plugin_settings_state.custom_paths.remove(idx);
                                cx.notify();
                            });
                        }),
                )
                .into_any_element(),
        );
    }

    let mut plugin_rows: Vec<AnyElement> = Vec::new();
    for plugin in plugins.iter() {
        let is_open = open_names.contains(&plugin.name);
        let badge = match plugin.format {
            PluginFormat::CLAP => "CLAP",
            PluginFormat::VST3 => "VST3",
        };
        let badge_col = match plugin.format {
            PluginFormat::CLAP => rgb(0xB464FF),
            PluginFormat::VST3 => rgb(0x64B4FF),
        };
        let name = plugin.name.clone();
        let vendor = plugin.vendor.clone();
        let plugin = plugin.clone();
        let gui_error = gui_errors.get(&plugin.name).cloned();
        plugin_rows.push(
            v_flex()
                .w_full()
                .gap(px(1.0))
                .py(px(4.0))
                .px(px(6.0))
                .bg(rgb(0x20232C))
                .rounded(px(4.0))
                .child(
                    h_flex()
                        .w_full()
                        .gap(px(8.0))
                        .items_center()
                        .child(
                            Label::new(badge)
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .text_color(badge_col)
                                .flex_shrink_0()
                                .w(px(44.0)),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .overflow_hidden()
                                .child(
                                    Label::new(name.clone())
                                        .text_xs()
                                        .truncate()
                                        .text_color(rgb(0xE8EAF0)),
                                )
                                .when(!vendor.is_empty(), |this| {
                                    this.child(
                                        Label::new(vendor.clone())
                                            .text_xs()
                                            .truncate()
                                            .text_color(rgb(0x6A7080)),
                                    )
                                }),
                        )
                        .child(
                            Button::new(format!("plugin_open_{}", name)).rounded(gpui_kit::component::button::ButtonRounded::None)
                                .label(if is_open { "Abierto" } else { "Abrir" })
                                .compact()
                                .flex_shrink_0()
                                .on_click(move |_, window, cx| {
                                    let parent = capture_host_parent(window);
                                    // Este panel abre plugins que pueden no
                                    // estar en ninguna cadena, así que no hay
                                    // slot del que sacar audio: el editor abre
                                    // para editar parámetros, sin display
                                    // reactivo. Quien sí está en el rack usa el
                                    // botón `Editor` de la tarjeta.
                                    let st = state(cx);
                                    cx.update_entity(&st, |state, cx| {
                                        state.plugin_settings_state.open_plugin(
                                            &plugin,
                                            parent,
                                            hikaru_plugin_host::tap::NO_SLOT,
                                        );
                                        cx.notify();
                                    });
                                }),
                        ),
                )
                .when_some(gui_error, |this, message| {
                    this.child(
                        Label::new(message)
                            .text_xs()
                            .truncate()
                            .text_color(rgb(0xFF7A5C)),
                    )
                })
                .into_any_element(),
        );
    }

    v_flex()
        .id("plugin_settings")
        .absolute()
        .left(px(20.0))
        .top(px(80.0))
        .w(px(560.0))
        .max_h(px(480.0))
        .bg(rgb(0x181A20))
        .border_1()
        .border_color(rgb(0x2A2D37))
        .rounded(px(8.0))
        .shadow(vec![gpui_kit::BoxShadow {
            color: gpui_kit::black().opacity(0.5),
            offset: point(px(0.0), px(8.0)),
            blur_radius: px(24.0),
            spread_radius: px(0.0),
            inset: false,
        }])
        .p(px(14.0))
        .gap(px(10.0))
        .child(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .child(Label::new("PLUGINS EXTERNOS · VST3 / CLAP").text_sm().font_weight(FontWeight::BOLD).text_color(rgb(0xE8EAF0)))
                .child(
                    Button::new("plugin_close_top").rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("✕")
                        .compact()
                        .flex_shrink_0()
                        .w(px(28.0))
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            cx.update_entity(&st, |state, cx| {
                                state.plugin_settings_state.is_open = false;
                                cx.notify();
                            });
                        }),
                ),
        )
        .child(Label::new("Rutas de búsqueda").text_xs().font_weight(FontWeight::BOLD).text_color(rgb(0x8A90A0)))
        .child(
            v_flex()
                .w_full()
                .gap(px(4.0))
                .max_h(px(120.0))
                .overflow_y_scrollbar()
                .children(path_rows),
        )
        .child(
            h_flex()
                .w_full()
                .gap(px(8.0))
                .items_center()
                .child(
                    Button::new("plugin_add_path").rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("+ Agregar ruta…")
                        .compact()
                        .flex_shrink_0()
                        .on_click(move |_, _, cx| {
                            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                                let st = state(cx);
                                cx.update_entity(&st, |state, cx| {
                                    state
                                        .plugin_settings_state
                                        .custom_paths
                                        .push(folder.display().to_string());
                                    cx.notify();
                                });
                            }
                        }),
                )
                .child(
                    Button::new("plugin_rescan").rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label(if is_scanning { "Escaneando…" } else { "Rescan plugins" })
                        .compact()
                        .flex_shrink_0()
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            cx.update_entity(&st, |state, cx| {
                                state.plugin_settings_state.start_scan();
                                cx.notify();
                            });
                        }),
                )
                .child(
                    Label::new(status.clone())
                        .text_xs()
                        .truncate()
                        .flex_1()
                        .text_color(rgb(0x8A90A0)),
                ),
        )
        .child(
            h_flex()
                .w_full()
                .items_center()
                .gap(px(6.0))
                .child(Label::new(format!("Plugins encontrados ({})", plugins.len())).text_xs().font_weight(FontWeight::BOLD).text_color(rgb(0x8A90A0))),
        )
        .child(
            v_flex()
                .w_full()
                .gap(px(4.0))
                .max_h(px(150.0))
                .overflow_y_scrollbar()
                .when(plugins.is_empty(), |this| {
                    this.child(
                        Label::new("Ninguno todavía — pulsa «Rescan plugins».")
                            .text_xs()
                            .text_color(rgb(0x4A5060)),
                    )
                })
                .children(plugin_rows),
        )
        // Errores de escaneo. No interrumpen nada: un bundle roto convive con
        // los demás plugins en el rack, y lo que se quiere es que el usuario
        // sepa por qué el suyo no aparece.
        .when(!scan_errors.is_empty(), |this| {
            this.child(
                v_flex()
                    .gap(px(2.0))
                    .child(
                        Label::new(format!("Omitidos ({}):", scan_errors.len()))
                            .text_xs()
                            .text_color(rgb(0xB464FF)),
                    )
                    .children(scan_errors.iter().take(5).map(|e| {
                        Label::new(e.clone()).text_xs().text_color(rgb(0x6A7080))
                    })),
            )
        })
        .child(
            Button::new("plugin_close").rounded(gpui_kit::component::button::ButtonRounded::None)
                .label("Cerrar")
                .compact()
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    cx.update_entity(&st, |state, cx| {
                        state.plugin_settings_state.is_open = false;
                        cx.notify();
                    });
                }),
        )
        .into_any_element()
}
