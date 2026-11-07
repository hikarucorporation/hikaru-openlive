use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

use egui::{Button, Color32, Context, RichText, ScrollArea, Window};

use crate::app::AppState;
use crate::theme::*;

pub use hikaru_plugin_host::PluginDescriptor as DiscoveredPlugin;
pub use hikaru_plugin_host::{HostParentKind, PluginFormat};
use hikaru_plugin_host::hikaru_wrapper::{
    FeedbackOrigin, HikaruWrapper, LoopExit, ParamFeedback, WrapperConfig, WrapperRuntime,
};

pub use hikaru_plugin_host::hikaru_wrapper::WrapperCommand as PluginWindowCommand;

pub fn capture_host_parent() -> HostParentKind {
    HostParentKind::Unknown
}

/// Lo que el editor nativo movió por su cuenta, hacia la UI.
///
/// Lo produce el hilo de la ventana al bombear el editor y lo consume
/// `AppState::apply_param_feedback` en el hilo principal, que mueve los knobs
/// del rack para que no mientan. `descriptor_id` (estable entre escaneos)
/// empareja con el slot aunque haya renombrados de por medio.
///
/// `origin` distingue el eco del host de un cambio real del editor: ver
/// [`FeedbackOrigin`].
#[derive(Debug, Clone)]
pub struct ParamFeedbackEvent {
    pub descriptor_id: String,
    pub changes: Vec<(u32, f32)>,
    pub origin: FeedbackOrigin,
}

enum ScanMessage {
    /// Progreso del escaneo, sólo para la etiqueta de estado.
    #[allow(dead_code)]
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
    /// ventana de GPUI capturado en el click.
    ///
    /// La envoltura de ventana ([`WrapperConfig`], con los defaults tipo
    /// Fruity Wrapper) se arma acá y viaja al hilo hijo: `always_on_top` y
    /// `keep_focused_with_parent` son lo que hace que la ventana del plugin no
    /// desaparezca detrás del DAW cuando el usuario clickea cualquier parte de
    /// la interfaz.
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
        let wrapper_config = WrapperConfig::with_parent(parent);

        self.opening.insert(plugin.name.clone());
        self.cmd_tx.insert(plugin.name.clone(), cmd_tx);

        let handle = std::thread::spawn(move || {
            run_plugin_window(
                plugin_path,
                plugin_name,
                descriptor_id,
                format,
                wrapper_config,
                cmd_rx,
                param_tx,
                close_tx,
                tap_key,
            );
        });

        self.open_plugins.insert(plugin.name.clone(), handle);
    }

    /// Re-eleva todas las ventanas de plugin abiertas, sin robarles el foco.
    ///
    /// La llama el DAW cuando recupera la activación (ver
    /// `dsp_rack::keep_plugin_editors_above`). Cada hilo de ventana ejecuta
    /// `ConfigureWindow(stack=Above)`, que devuelve el editor a su sitio sin
    /// activarlo: el usuario acaba de clickear un knob del DAW y no puede
    /// perder el foco a mitad del gesto.
    ///
    /// Cuesta un `send` por ventana abierta sobre un canal sin cota, así que
    /// es barato de llamar aunque se termine invocando en cada activación.
    /// Las ventanas en proceso de apertura todavía no tienen hilo al que
    /// hablar y no aparecen en `cmd_tx`; no necesitan re-elevación porque
    /// nacen con los hints ya puestos.
    pub fn raise_open_plugins(&self) {
        if self.cmd_tx.is_empty() {
            return;
        }
        for tx in self.cmd_tx.values() {
            let _ = tx.send(PluginWindowCommand::Raise);
        }
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
/// Todo lo que hace es **orquestar**: crear el envoltorio, cargar el plugin,
/// embebedarlo y handing-le el bucle. La política de ventana y el tick viven en
/// [`hikaru_plugin_host::hikaru_wrapper`]; lo que queda acá es el pegamento que
/// necesita conocer el estado del DAW: el handle del plugin, la cinta de audio
/// y los canales hacia el hilo principal.
///
/// # Ciclo de vida (el orden importa, es lo que estaba roto)
///
/// 1. Crear la ventana del envoltorio **sin mapear**
///    ([`HikaruWrapperWindow::create`]). Nada visible todavía: si la carga
///    falla, no hay flash de ventana vacía.
/// 2. Cargar el binario (`dlopen`). Es lo que más falla (arquitectura, símbolos,
///    dependencias) y por eso la ventana sigue invisible.
/// 3. Preguntar el tamaño preferido y embebederse.
/// 4. Mapear + activar. Recién ahora se muestra, con el primer frame ya
///    teniendo contenido del plugin detrás.
/// 5. Correr el bucle de 60 Hz hasta que alguien cierre la ventana.
///
/// Todo fallo manda `PluginClosedNotification { error: Some(..) }`; el cierre
/// normal del usuario manda `error: None`.
fn run_plugin_window(
    plugin_path: PathBuf,
    plugin_name: String,
    descriptor_id: String,
    format: PluginFormat,
    wrapper_config: WrapperConfig,
    cmd_rx: Receiver<PluginWindowCommand>,
    param_tx: Sender<ParamFeedbackEvent>,
    close_tx: Sender<PluginClosedNotification>,
    tap_key: hikaru_plugin_host::tap::TapKey,
) {
    let notify = |error: Option<String>| {
        let _ = close_tx.send(PluginClosedNotification {
            name: plugin_name.clone(),
            error,
        });
    };

    // Todo el ciclo de vida —ventana, `dlopen`, embebido, apagado— vive en
    // `HikaruWrapper::open`. Acá sólo se le pasa la cinta de audio del motor,
    // se traduce el resultado al canal de la UI y se corre.
    let (feedback_tx, feedback_rx) = channel::<ParamFeedback>();
    let open_result = HikaruWrapper::open(
        &plugin_path,
        format,
        &format!("{}", plugin_name),
        wrapper_config,
        WrapperRuntime::default(),
        cmd_rx,
        feedback_tx,
        tap_key,
    );

    let wrapper = match open_result {
        Ok(wrapper) => wrapper,
        Err(e) => {
            let message = e.to_string();
            eprintln!("[PluginWindow] '{}': {}", plugin_name, message);
            notify(Some(message));
            return;
        }
    };

    println!(
        "[PluginWindow] '{}' abierto: {} (hija del DAW: {}, sin decoraciones: {})",
        plugin_name,
        wrapper.plugin_name(),
        wrapper.is_child_of_daw(),
        wrapper.is_borderless()
    );

    // El feedback se reenvía con el id del descriptor para que el hilo
    // principal sepa a qué slot pertenece. Va en su propio hilo porque el
    // canal del bucle no lo consume nadie más y sin drainedor se llenaría: los
    // temporizadores de CLAP y los analizadores generan cambios a 60 Hz.
    let feedback_forwarder = std::thread::Builder::new()
        .name("hikaru-plugin-feedback".into())
        .spawn(move || {
            while let Ok(feedback) = feedback_rx.recv() {
                if feedback.changes.is_empty() {
                    continue;
                }
                let _ = param_tx.send(ParamFeedbackEvent {
                    descriptor_id: descriptor_id.clone(),
                    changes: feedback.changes,
                    origin: feedback.origin,
                });
            }
        });

    let exit = wrapper.run();

    // El hilo del feedback termina solo: `feedback_tx` se suelta con el
    // bucle, así que `recv` devuelve error y el hilo muere. No se hace join
    // porque `param_tx` vive en el estado de la app y sólo se cae con ella.
    drop(feedback_forwarder);

    // Se reporta el motivo real: un cierre pedido o del usuario no es un
    // fallo y no debe pintar una línea roja en el rack, pero perder la
    // conexión con el servidor sí es algo que el usuario tiene que saber.
    match exit {
        LoopExit::Requested | LoopExit::UserClosed => notify(None),
        LoopExit::Destroyed => {
            notify(Some("La ventana del editor fue destruida por el sistema de ventanas.".into()))
        }
        LoopExit::ConnectionLost(e) => {
            notify(Some(format!("Se perdió la conexión con el servidor X11: {e}")))
        }
    }
}

pub fn render(ctx: &Context, state: &mut AppState) {
    if !state.plugin_settings_state.is_open {
        return;
    }

    let mut is_open = state.plugin_settings_state.is_open;

    Window::new(RichText::new("PLUGINS EXTERNOS · VST3 / CLAP").strong().color(Color32::from_rgb(0xE8, 0xEA, 0xF0)))
        .open(&mut is_open)
        .default_size([560.0, 480.0])
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;

            ui.label(RichText::new("Rutas de búsqueda").size(11.0).strong().color(TEXT_MUTED));

            let mut to_remove = None;
            ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                for (idx, p) in state.plugin_settings_state.custom_paths.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(p).size(11.0).color(Color32::from_rgb(0xB8, 0xBC, 0xC8)));
                        if ui.add(Button::new("x").small()).clicked() {
                            to_remove = Some(idx);
                        }
                    });
                }
            });
            if let Some(idx) = to_remove {
                state.plugin_settings_state.custom_paths.remove(idx);
            }

            ui.horizontal(|ui| {
                if ui.add(Button::new("+ Agregar ruta…").fill(SURFACE_BG)).clicked() {
                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                        state.plugin_settings_state.custom_paths.push(folder.display().to_string());
                    }
                }

                let scan_label = if state.plugin_settings_state.is_scanning { "Escaneando…" } else { "Rescan plugins" };
                if ui.add(Button::new(scan_label).fill(SURFACE_BG)).clicked() {
                    state.plugin_settings_state.start_scan();
                }

                ui.label(RichText::new(&state.plugin_settings_state.status_message).size(11.0).color(TEXT_MUTED));
            });

            ui.separator();

            let plugins_count = state.plugin_settings_state.discovered_plugins.len();
            ui.label(RichText::new(format!("Plugins encontrados ({})", plugins_count)).size(11.0).strong().color(TEXT_MUTED));

            let plugins = state.plugin_settings_state.discovered_plugins.clone();
            let open_names = state.plugin_settings_state.open_plugin_names();

            ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                if plugins.is_empty() {
                    ui.label(RichText::new("Ninguno todavía — pulsa «Rescan plugins».").size(11.0).color(TEXT_DISABLED));
                } else {
                    for plugin in &plugins {
                        let is_plugin_open = open_names.contains(&plugin.name);
                        let (badge, badge_col) = match plugin.format {
                            PluginFormat::CLAP => ("CLAP", Color32::from_rgb(0xB4, 0x64, 0xFF)),
                            PluginFormat::VST3 => ("VST3", Color32::from_rgb(0x64, 0xB4, 0xFF)),
                        };

                        ui.horizontal(|ui| {
                            ui.label(RichText::new(badge).size(10.0).strong().color(badge_col));
                            ui.vertical(|ui| {
                                ui.label(RichText::new(&plugin.name).size(11.0).color(TEXT_PRIMARY));
                                if !plugin.vendor.is_empty() {
                                    ui.label(RichText::new(&plugin.vendor).size(9.5).color(TEXT_MUTED));
                                }
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let btn_label = if is_plugin_open { "Abierto" } else { "Abrir" };
                                if ui.add(Button::new(btn_label).fill(SURFACE_BG)).clicked() {
                                    let parent = capture_host_parent();
                                    state.plugin_settings_state.open_plugin(
                                        plugin,
                                        parent,
                                        hikaru_plugin_host::tap::NO_SLOT,
                                    );
                                }
                            });
                        });
                        ui.separator();
                    }
                }
            });

            if !state.plugin_settings_state.scan_errors.is_empty() {
                ui.separator();
                ui.label(RichText::new(format!("Omitidos ({}):", state.plugin_settings_state.scan_errors.len())).size(11.0).color(Color32::from_rgb(0xB4, 0x64, 0xFF)));
                for err in state.plugin_settings_state.scan_errors.iter().take(5) {
                    ui.label(RichText::new(err).size(10.0).color(TEXT_MUTED));
                }
            }

            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(Button::new("Cerrar").fill(SURFACE_BG)).clicked() {
                    state.plugin_settings_state.is_open = false;
                }
            });
        });

    state.plugin_settings_state.is_open = is_open;
}
