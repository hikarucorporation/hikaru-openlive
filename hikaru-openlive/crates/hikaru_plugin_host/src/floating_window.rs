// Copyright (C) Hikaru Corporation - 2026
// Hikaru Floating Plugin Editor Window
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/src/floating_window.rs

//! Ventanas flotantes para editores de plugin, servidas por un hilo propio.
//!
//! # Por qué existe
//!
//! `winit::event_loop::EventLoop::run()` **bloquea el hilo** hasta que la
//! ventana se cierra. La primera versión de `show_gui_floating` en el launcher
//! VST3 lo llamaba directo, y `HikaruApp::open_plugin_floating_gui` lo invocaba
//! desde el hilo de control de GPUI: abrir el editor de un plugin congelaba la
//! DAW entera (sin painting, sin entrada, sin reloj de transporte) hasta que el
//! usuario cerrara esa ventana.
//!
//! # El problema sutil de winit
//!
//! `EventLoop::run_return()` no bloquea, pero exige que el `run()` esté
//! ejecutándose en el mismo hilo para bombear eventos; si nadie lo llama, la
//! ventana aparece y se congela. Y winit restringe la creación de event loops
//! por proceso: crear uno por cada ventana de plugin es frágil (y en algunas
//! plataformas directamente ilegal).
//!
//! La solución: **un solo event loop global**, corriendo en un hilo dedicado, y
//! todas las ventanas de plugin multiplexadas dentro de él como `UserEvent`.
//! Abrir un segundo plugin no crea un segundo event loop — agrega una ventana al
//! que ya está corriendo. El hilo de control nunca entra al event loop, así que
//! puede seguir pintando la DAW mientras el plugin tiene su ventana abierta.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, Once, OnceLock};
use std::time::Duration;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::event::{Event as WinitEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop, EventLoopProxy};
use winit::window::Window;

/// Id de un editor flotante, asignado por el event loop.
type EditorKey = u64;

/// `RawWindowHandle` no es `Send` porque sus variantes llevan `NonNull<c_void>`.
///
/// Pero un handle de ventana es sólo un identificador opaco que ya cruzó de
/// hilo cuando la ventana se creó: el event loop vive en otro hilo y el
/// `open_editor` del plugin corre en el de control. Envolverlo para poder
/// mandarlo por el canal es seguro, y la alternativa sería quedarnos sin ability
/// de adjuntar el editor a la ventana que acabamos de crear.
#[derive(Debug, Clone)]
pub struct SendHandle(pub RawWindowHandle);

// SAFETY: ver la nota de arriba. `RawWindowHandle` sólo transporta IDs y
// punteros a datos que el backend mantiene vivos mientras la ventana exista,
// y la ventana vive en el event loop hasta que el handle se suelta.
unsafe impl Send for SendHandle {}
unsafe impl Sync for SendHandle {}

/// Eventos que el hilo de control envía al hilo del event loop.
///
/// Se envuelven con el id del editor al que pertenecen, porque `UserEvent` de
/// winit es un único `T` y varios editores comparten el mismo event loop.
#[derive(Debug)]
struct Routed {
    key: EditorKey,
    command: Command,
}

/// Comandos que el hilo de control envía al hilo del event loop.
#[derive(Debug)]
enum Command {
    Open {
        title: String,
        width: u32,
        height: u32,
        /// Canal por el que el event loop reporta el handle nativo y los
        /// eventos de esa ventana. Vive en el comando, no en un registro
        /// global: así no puede desincronizarse del estado real del loop.
        reply: Sender<FloatingEvent>,
    },
    Resize {
        width: u32,
        height: u32,
    },
    Redraw,
    Close,
}

/// Eventos que el hilo del event loop envía al de control.
#[derive(Debug)]
pub enum FloatingEvent {
    /// La ventana nativa está creada. `raw_handle` es lo que hay que pasarle a
    /// `open_editor` / `gui.create` + `gui.set_parent` del plugin.
    NativeWindow { raw_handle: SendHandle },
    /// El plugin pidió cambiar de tamaño.
    ResizeRequested { width: u32, height: u32 },
    /// La ventana se cerró (botón de cerrar o WM).
    WindowClosed,
    /// Error irrecuperable creando o consultando la ventana.
    Failed(String),
}

/// Estado compartido entre el hilo de control y el hilo del event loop.
struct Hub {
    /// Proxy para mandar comandos desde cualquier hilo al event loop.
    proxy: EventLoopProxy<Routed>,
    next_key: EditorKey,
}

static HUB: OnceLock<Mutex<Hub>> = OnceLock::new();
static INIT: Once = Once::new();
static INIT_ERROR: OnceLock<String> = OnceLock::new();

/// Levanta (una sola vez) el hilo dedicado que corre el event loop.
///
/// Idempotente y segura de llamar desde varios hilos: el primero hace el
/// trabajo, los demás esperan y observan el mismo resultado. Devuelve `Err`
/// si el event loop no se pudo levantar.
pub fn spawn_event_loop_thread() -> Result<(), String> {
    INIT.call_once(|| {
        if let Err(e) = boot_event_loop_thread() {
            let _ = INIT_ERROR.set(e);
        }
    });

    match INIT_ERROR.get() {
        Some(e) => Err(e.clone()),
        None => Ok(()),
    }
}

fn boot_event_loop_thread() -> Result<(), String> {
    // `EventLoop` de winit **no es `Send`**: en Wayland envuelve
    // `Rc<RefCell<...>>`, así que no se puede crear en un hilo y moverlo a
    // otro. Por eso se crea *dentro* del hilo que lo va a correr.
    //
    // El handshake con el hilo de control es un canal de Result: el hilo
    // dedicado manda el proxy una vez que el loop está vivo.
    let (ready_tx, ready_rx) = channel::<Result<EventLoopProxy<Routed>, String>>();

    std::thread::Builder::new()
        .name("hikaru-plugin-editor".into())
        .spawn(move || run_event_loop(ready_tx))
        .map_err(|e| format!("no se pudo crear el hilo de editores: {e}"))?;

    match ready_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(proxy)) => {
            HUB.set(Mutex::new(Hub { proxy, next_key: 1 }))
                .map_err(|_| "otro hub de ventanas flotantes ya está inicializado")?;
            Ok(())
        }
        Ok(Err(e)) => Err(e),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            Err("el hilo del event loop no respondió el handshake a tiempo".into())
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err("el hilo del event loop murió antes del handshake".into())
        }
    }
}

/// El bucle. Corre entero en el hilo dedicado, que es el único lugar donde
/// `EventLoop` puede existir.
fn run_event_loop(ready: Sender<Result<EventLoopProxy<Routed>, String>>) {
    // En winit 0.29 esto devuelve `Result`.
    // Por defecto winit **panica** si el event loop se crea fuera del hilo
    // principal: lo considera un riesgo de portabilidad. Acá es deliberado —
    // la ventana del editor de un plugin vive en un hilo dedicado para no
    // bloquear el hilo de control de la DAW — así que hay que desactivar ese
    // chequeo explícitamente.
    let mut builder: winit::event_loop::EventLoopBuilder<Routed> =
        winit::event_loop::EventLoopBuilder::with_user_event();

    // En Linux, `winit` compila con las features `x11` y `wayland` por defecto, y
    // cada backend expone su propio trait con `with_any_thread`. Los dos tocan
    // el mismo atributo del builder, así que invocarlos ambos es idempotente y
    // evita tener que adivinar qué backend va a elegir el runtime.
    #[cfg(target_os = "linux")]
    {
        // Se desambigúa con UFCS porque ambos traits traen un método con el
        // mismo nombre y el compilador no puede elegir.
        use winit::platform::wayland::EventLoopBuilderExtWayland;
        use winit::platform::x11::EventLoopBuilderExtX11;
        EventLoopBuilderExtWayland::with_any_thread(&mut builder, true);
        EventLoopBuilderExtX11::with_any_thread(&mut builder, true);
    }

    let event_loop: EventLoop<Routed> = match builder.build() {
        Ok(loop_) => loop_,
        Err(e) => {
            let _ = ready.send(Err(format!("no se pudo crear el event loop: {e}")));
            return;
        }
    };

    let proxy: EventLoopProxy<Routed> = event_loop.create_proxy();

    // `EventLoop` se presta por `Deref` al target que necesitan los
    // `WindowBuilder`. Se lo sacamos una vez acá para no tener que pelear el
    // borrow de `event_loop` contra `&mut windows` dentro del closure.
    let target: *const winit::event_loop::EventLoopWindowTarget<Routed> = &*event_loop;

    // Ventanas vivas y el editor al que pertenecen.
    let mut windows: Vec<(EditorKey, Window)> = Vec::new();
    // Canal de salida por editor.
    let mut replies: Vec<(EditorKey, Sender<FloatingEvent>)> = Vec::new();
    // Eventos acumulados en este tick. Se vacían fuera del `match` porque
    // necesitan `&mut replies` y `&mut windows` a la vez, y el borrow del
    // `event` los tiene tomados.
    let mut outbox: Vec<(EditorKey, FloatingEvent)> = Vec::new();

    event_loop.set_control_flow(ControlFlow::Wait);

    // Handshake: recién ahora el loop existe y se le puede hablar. Si el
    // llamador no recibe esto, cuelga hasta su timeout y aborta.
    let _ = ready.send(Ok(proxy));

    // `run` devuelve `!`: nunca retorna. El `Result` que reporta winit sobre
    // errores del backend se descarta a propósito porque no hay a quién
    // avisarle desde acá (el hilo vive hasta que muere el proceso), pero hay
    // que consumirlo explícitamente para no dejar un `unused Result`.
    let _ = event_loop.run(move |event, _elwt| {
        match event {
            WinitEvent::UserEvent(Routed { key, command }) => match command {
                Command::Open {
                    title,
                    width,
                    height,
                    reply,
                } => {
                    // El target se re-presta acá. Es el mismo `&*` de arriba y
                    // sigue válido mientras viva el event loop.
                    let target = unsafe { &*target };
                    match build_window_from_target(target, title, width, height) {
                        Ok(window) => {
                            // Se registra el canal **antes** de avisar, para que
                            // un `WindowClosed` que llegue en el mismo tick no se
                            // pierda por no encontrar destino.
                            replies.push((key, reply));

                            match window.window_handle() {
                                Ok(h) => {
                                    let raw_handle = SendHandle(h.as_raw());
                                    windows.push((key, window));
                                    outbox.push((
                                        key,
                                        FloatingEvent::NativeWindow { raw_handle },
                                    ));
                                }
                                Err(e) => {
                                    outbox.push((
                                        key,
                                        FloatingEvent::Failed(format!("window_handle: {e}")),
                                    ));
                                    replies.retain(|(k, _)| *k != key);
                                }
                            }
                        }
                        Err(e) => {
                            // Sin ventana no hay editor: se avisa por el mismo
                            // canal y no queda una entrada colgada.
                            let _ = reply.send(FloatingEvent::Failed(e));
                        }
                    }
                }
                Command::Resize { width, height } => {
                    if let Some((_, w)) = windows.iter_mut().find(|(k, _)| *k == key) {
                        let _ = w.request_inner_size(winit::dpi::LogicalSize::new(
                            f64::from(width),
                            f64::from(height),
                        ));
                    }
                }
                Command::Redraw => {
                    if let Some((_, w)) = windows.iter_mut().find(|(k, _)| *k == key) {
                        let _ = w.request_redraw();
                    }
                }
                Command::Close => {
                    if let Some(pos) = windows.iter().position(|(k, _)| *k == key) {
                        windows.remove(pos);
                        outbox.push((key, FloatingEvent::WindowClosed));
                        replies.retain(|(k, _)| *k != key);
                    }
                }
            },

            WinitEvent::WindowEvent { window_id, event } => {
                // Cualquier evento de ventana que nos concierne se traduce al
                // editor dueño de esa ventana.
                let key = windows
                    .iter()
                    .find(|(_, w)| w.id() == window_id)
                    .map(|(k, _)| *k);

                if let Some(key) = key {
                    match event {
                        WindowEvent::CloseRequested => {
                            if let Some(pos) =
                                windows.iter().position(|(k, _)| *k == key)
                            {
                                windows.remove(pos);
                            }
                            outbox.push((key, FloatingEvent::WindowClosed));
                            replies.retain(|(k, _)| *k != key);
                        }
                        WindowEvent::Resized(size) => {
                            outbox.push((
                                key,
                                FloatingEvent::ResizeRequested {
                                    width: size.width,
                                    height: size.height,
                                },
                            ));
                        }
                        WindowEvent::RedrawRequested => {
                            // El plugin dibuja en su propia ventana; el
                            // `VST3`/`CLAP` pedir redibujar es responsabilidad
                            // del editor embebido, no del loop de winit.
                        }
                        _ => {}
                    }
                }
            }

            _ => {}
        }

        // Entrega de eventos a los hilos de control. Fuera del `match` para no
        // pelear el borrow de `replies`.
        for (key, ev) in std::mem::take(&mut outbox) {
            if let Some((_, tx)) = replies.iter().find(|(k, _)| *k == key) {
                let _ = tx.send(ev);
            }
        }
    });
}

/// Construye la ventana a partir del target prestado por el event loop.
///
/// Separada de `run_event_loop` para que el target se pase por parámetro en vez
/// de capturarse: es lo que evita que `&EventLoop` y `&mut windows` vivan en el
/// mismo borrow.
fn build_window_from_target(
    target: &winit::event_loop::EventLoopWindowTarget<Routed>,
    title: String,
    width: u32,
    height: u32,
) -> Result<Window, String> {
    winit::window::WindowBuilder::new()
        .with_title(title)
        .with_inner_size(winit::dpi::LogicalSize::new(f64::from(width), f64::from(height)))
        .build(target)
        .map_err(|e| format!("no se pudo crear la ventana flotante: {e}"))
}

// ───────────────────────────── Handle de control ─────────────────────────────

/// Controla una ventana flotante desde el hilo que llama al plugin.
///
/// Crear el handle **no** bloquea: la ventana se crea en el hilo del event loop
/// y el handle nativo llega por el canal. Es lo que permite abrir el editor sin
/// congelar la DAW.
pub struct FloatingEditor {
    key: EditorKey,
    events: Receiver<FloatingEvent>,
}

impl FloatingEditor {
    /// Abre una ventana para un editor y devuelve el handle de control.
    ///
    /// No bloquea. Para obtener el `RawWindowHandle` hay que llamar después a
    /// [`Self::wait_native_window`].
    pub fn open(title: &str, width: u32, height: u32) -> Result<Self, String> {
        // El event loop tiene que estar corriendo antes de poder enrutar.
        spawn_event_loop_thread()?;

        let (tx, rx) = channel::<FloatingEvent>();

        let key = {
            let mut hub = HUB
                .get()
                .ok_or("el hub de ventanas flotantes no está inicializado")?
                .lock()
                .map_err(|_| "el hub de ventanas flotantes está envenenado")?;
            let key = hub.next_key;
            hub.next_key += 1;

            hub.proxy
                .send_event(Routed {
                    key,
                    command: Command::Open {
                        title: title.to_string(),
                        width,
                        height,
                        reply: tx,
                    },
                })
                .map_err(|e| format!("no se pudo encolar la apertura: {e}"))?;

            key
        };

        Ok(Self { key, events: rx })
    }

    /// Espera a que la ventana exista y devuelve su handle nativo.
    ///
    /// El plugin necesita ese handle para `open_editor` (VST3) o
    /// `gui.create` + `gui.set_parent` (CLAP). El timeout es corto a propósito:
    /// si el loop no está bombando eventos, fallar rápido es mejor que colgar
    /// el hilo de control.
    pub fn wait_native_window(&self, timeout: Duration) -> Result<RawWindowHandle, String> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err("timeout esperando la ventana nativa del editor".into());
            }

            match self.events.recv_timeout(remaining.min(Duration::from_millis(50))) {
                Ok(FloatingEvent::NativeWindow { raw_handle }) => return Ok(raw_handle.0),
                Ok(FloatingEvent::Failed(e)) => return Err(e),
                Ok(FloatingEvent::WindowClosed) => {
                    return Err("la ventana se cerró antes de estar lista".into())
                }
                Ok(FloatingEvent::ResizeRequested { .. }) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("el hilo del event loop se desconectó".into())
                }
            }
        }
    }

    /// Redimensiona la ventana del editor.
    pub fn resize(&self, width: u32, height: u32) -> Result<(), String> {
        self.send(Command::Resize { width, height })
    }

    /// Pide al loop que redibuje la ventana.
    pub fn request_redraw(&self) -> Result<(), String> {
        self.send(Command::Redraw)
    }

    /// Cierra la ventana y libera el editor.
    pub fn close(&self) -> Result<(), String> {
        self.send(Command::Close)
    }

    /// Toma un evento pendiente sin bloquear más de `timeout`.
    pub fn poll_event(&self, timeout: Duration) -> Option<FloatingEvent> {
        self.events.recv_timeout(timeout).ok()
    }

    /// Si el usuario cerró la ventana.
    pub fn is_closed(&self) -> bool {
        matches!(self.events.try_recv(), Ok(FloatingEvent::WindowClosed))
    }

    fn send(&self, command: Command) -> Result<(), String> {
        let hub = HUB
            .get()
            .ok_or("el hub de ventanas flotantes no está inicializado")?
            .lock()
            .map_err(|_| "el hub de ventanas flotantes está envenenado")?;
        hub.proxy
            .send_event(Routed {
                key: self.key,
                command,
            })
            .map_err(|e| format!("no se pudo encolar el comando: {e}"))
    }
}

impl Drop for FloatingEditor {
    fn drop(&mut self) {
        // Sin esto la ventana quedaría huérfana, visible, sin plugin detrás,
        // hasta que muera el proceso.
        let _ = self.close();
    }
}