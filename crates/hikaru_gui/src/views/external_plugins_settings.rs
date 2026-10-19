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
use hikaru_plugin_host::hikaru_wrapper::{
    FeedbackOrigin, HikaruWrapper, LoopExit, ParamFeedback, WrapperConfig, WrapperRuntime,
};

/// Comando del rack hacia el hilo de la ventana de un plugin abierto.
///
/// Re-export del tipo del envoltorio: el hilo de la ventana y el hilo del DAW
/// tienen que hablar **exactamente** el mismo idioma, y duplicar el enum
/// obligaría a mantener una tabla de conversión que nadie va a leer cuando
/// se agregue un variante.
pub use hikaru_plugin_host::hikaru_wrapper::WrapperCommand as PluginWindowCommand;

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

    let mut wrapper = match open_result {
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
