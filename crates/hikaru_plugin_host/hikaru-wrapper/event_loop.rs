// Copyright (C) Hikaru Corporation - 2026
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/src/hikaru_wrapper/event_loop.rs

//! El bucle que mantiene vivo el editor del plugin.
//!
//! # Por qué existe
//!
//! El editor de un plugin es un proceso ajeno que hay que bombear a mano: no
//! tiene hilo propio, no redibuja solo y no recibe eventos si nadie se los
//! entrega. Antes de este módulo el bucle era un `sleep(10ms)` pegado dentro
//! de `hikaru_gui`, mezclado con la creación de la ventana, la carga del
//! plugin y el apagado. Eso dejaba dos fallos:
//!
//! 1. **`process()` sólo se llamaba si el tap de audio entregaba un bloque.**
//!    Y el tap sólo entrega con el transporte corriendo. Con el transporte
//!    parado, el anillo de control de VST3 nunca se drenaba: los cambios de
//!    parámetro que mandaba el rack se acumulaban sin aplicarse nunca, y el
//!    analizador del editor se quedaba plano —"Play audio to see the
//!    spectrum"— aunque el audio se oyera perfecto en la cadena. El síntoma se
//!    veía en el editor; la causa era que el bucle no bombeba.
//! 2. **El latido no tenía frecuencia declarada.** `sleep(10)` es un
//!    *piso*, no un ritmo: si un tick tardaba 8 ms de audio, el siguiente
//!    llegaba 18 ms después y la FFT del plugin veía jitter. Aquí el ritmo lo
//!    fija [`TickBudget`] y cada tick hace *siempre* las mismas cinco cosas,
//!    en orden, para que ninguna se quede afuera por accidente.
//!
//! # Qué hace en cada tick
//!
//! 1. Drena los comandos del rack (`SetParam`, `Close`, `Raise`, `Focus`).
//! 2. Llama a `process()` **siempre**, no sólo cuando hay audio nuevo: es lo
//!    que drena el anillo de control de VST3 y lo que arranca el plugin.
//! 3. Llama a [`PluginInstance::gui_tick`], que es el equivalente por formato
//!    de un `idle()`/`on_timer()` del host.
//! 4. Drena los eventos de X11 (que incluyen la redirección de entrada).
//! 5. Re-afirma el stacking y devuelve el feedback de parámetros.

use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::tap::PluginTap;
use crate::{AudioBuffer, PluginInstance};

use super::config::HostFrame;

use super::window::{HikaruWrapperWindow, WindowEvent};

/// Comando del rack hacia el hilo de la ventana de un plugin abierto.
///
/// Viaja por `mpsc` sin cota: `send` no bloquea y el hilo lo drena con
/// `try_recv` en cada tick, así el gesto del usuario no espera al servidor
/// X11 ni viceversa.
#[derive(Debug, Clone)]
pub enum WrapperCommand {
    /// Un knob del rack movió un parámetro (id, normalizado 0..1).
    SetParam { id: u32, value: f32 },
    /// Desplazamiento de modulación en vivo hacia la instancia del editor
    /// (`CLAP_EVENT_PARAM_MOD` cuando el plugin lo soporta).
    SetParamMod { id: u32, delta: f32 },
    /// Cerrar la ventana (el usuario la cerró desde el DAW).
    Close,
    /// Re-elevar la ventana sin robarle el foco al DAW.
    ///
    /// Lo manda la ventana del DAW cuando recupera la activación: sin esto el
    /// gestor de ventanas sube su propia ventana y deja el editor del plugin
    /// detrás (o en otro workspace).
    Raise,
    /// Que el editor tome el foco de teclado.
    Focus,
    /// Mover y redimensionar el editor dentro de la ventana del DAW.
    ///
    /// Lo manda la vista del rack cuando el layout cambia: el usuario agranda
    /// el panel, cambia de pestaña o redimensiona el DAW. El rect va en
    /// coordenadas de cliente del DAW; el bucle lo traduce a coordenadas de
    /// raíz en [`HikaruWrapperWindow::sync_with_host`].
    SetHostFrame(HostFrame),
}

/// Lo que el editor nativo movió por su cuenta, hacia el rack.
///
/// Es el camino de vuelta del intercambio bidireccional: lo que el usuario
/// tocó en la ventana del plugin vuelve al hilo principal para que los knobs
/// del rack no mientan.
#[derive(Debug, Clone)]
pub struct ParamFeedback {
    /// Cambios `(id, normalizado)`.
    pub changes: Vec<(u32, f32)>,
    /// De dónde salen.
    ///
    /// `Editor` = el usuario movió algo dentro del plugin. `Host` = el rack
    /// se lo mandó y el bucle lo confirma de vuelta. El hilo principal usa
    /// esto para no pelearse con el gesto del usuario: si el último valor
    /// conocido vino del host, no lo pisa con un eco desfasado.
    pub origin: FeedbackOrigin,
    /// Snapshot binario opcional (`save_state`) emitido en vivo cuando el
    /// usuario elige un preset o cambia estado interno (wavetables, matriz de
    /// modulación) en el editor, para mantener sincronizado el motor de audio
    /// sin tener que cerrar la ventana.
    pub state: Option<Vec<u8>>,
}

/// De dónde viene un [`ParamFeedback`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedbackOrigin {
    /// El plugin lo cambió por su cuenta (el usuario usó su editor).
    Editor,
    /// El host se lo mandó; el bucle confirma que llegó.
    Host,
}

/// Cada cuántos ticks el envoltorio le recuerda al plugin que su superficie
/// es visible.
///
/// X11 no tiene un "repintá": la única forma de que un toolkit vuelva a
/// dibujar es que le llegue un `Expose`. El servidor lo genera cuando la
/// ventana se mapea, pero hay plugins que se quedan con la bandera de
/// "no necesito repintar" puesta y nunca más vuelven a dibujar.
///
/// El latido es la red de seguridad: cada dos segundos se le manda un
/// `Expose` sintético a la vista del plugin. Es barato (32 bytes) y lo
/// bastante raro como para no gastar CPU en un repintado continuo.
const SURFACE_HEARTBEAT_TICKS: u64 = 120;

/// Cada cuántos ticks se sondea al editor por cambios de parámetro.
///
/// Es más seguido que la cadencia del feedback (5 ticks) porque el valor que
/// se pollá es el que el usuario está moviendo **ahora**: con 83 ms de
/// espera, el knob del rack y el del plugin van con medio paso de retraso y
/// el motor de audio tarda un instante en enterarse.
///
/// Dos ticks (30 Hz con el bucle de 60 Hz) es un buen compromiso: el sondeo
/// por diferencia de VST3 lee el valor de cada parámetro del controlador, y a
/// 30 Hz eso son unas treinta llamadas por segundo — nada comparado con el
/// resto del trabajo del tick (X11, `process()` del plugin, repintados).
const PARAM_POLL_TICKS: u64 = 2;

/// Ritmo del bucle.
///
/// Se declara en vez de dejar un `sleep` suelto porque es una decisión que
/// hay que poder justificar: el valor son los frames que el plugin ve por
/// segundo, y de eso dependen tanto los medidores como los temporizadores que
/// el propio plugin registra a través del host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickBudget {
    fps: u32,
    feedback_every: u32,
}

impl Default for TickBudget {
    /// 60 Hz con feedback cada 5 ticks (~83 ms).
    ///
    /// 60 y no 30 porque varios analizadores de plugin son de banda estrecha
    /// y a 30 Hz se ven a saltos; y 60 y no 120 porque cada tick puede
    /// invocar `process()` sobre el plugin y el coste se paga en el hilo de
    /// la ventana. 5 ticks de feedback es suficiente para que un knob del
    /// rack llegue a versemovido y no alcanza para inundar el canal.
    fn default() -> Self {
        Self {
            fps: 60,
            feedback_every: 5,
        }
    }
}

impl TickBudget {
    /// Un ritmo a medida. `fps == 0` cae a 60.
    pub fn new(fps: u32, feedback_every: u32) -> Self {
        Self {
            fps: fps.max(1),
            feedback_every: feedback_every.max(1),
        }
    }

    pub fn fps(&self) -> u32 {
        self.fps
    }

    /// Duración del tick.
    pub fn period(&self) -> Duration {
        Duration::from_nanos(1_000_000_000 / u64::from(self.fps))
    }

    /// Si en este tick hay que mandar feedback a la UI.
    pub fn feedback_due(&self, tick: u64) -> bool {
        tick % u64::from(self.feedback_every) == 0
    }
}

/// Por qué terminó el bucle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopExit {
    /// El DAW pidió cerrar la ventana.
    Requested,
    /// El usuario la cerró desde el titlebar.
    UserClosed,
    /// El WM o alguien destruyó la ventana.
    Destroyed,
    /// Se perdió la conexión con el servidor X11.
    ConnectionLost(String),
}

impl LoopExit {
    /// `true` si fue el usuario cerrando la ventana y no un fallo.
    ///
    /// La UI lo usa para distinguir "cerró el editor" de "el editor se
    /// rompió": el primero no merece una línea roja en el rack.
    pub fn is_clean(&self) -> bool {
        matches!(self, Self::Requested | Self::UserClosed)
    }
}

/// Bompea un bloque al plugin de la ventana.
///
/// # Por qué es una función libre
///
/// Es la mitad del arreglo del espectro muerto y por lo tanto lo que más
/// merece un test. Vive afuera del struct para poder llamarla sin una
/// ventana X11: el resto del bucle necesita un servidor de display, esta
/// parte no.
///
/// `scratch` y `primed` son el estado del bucle (el bloque reutilizado y si
/// alguna vez se recibió algo). Se pasan por referencia en vez de devolverlos
/// para que quien llama decida dónde viven.
///
/// # El contenido del bloque es irrelevante para el display
///
/// Lo que importa es que el plugin **suene**, porque sus analizadores se
/// llenan en `process()`. Cuando la cinta está muda —transporte parado, o
/// plugin que no está en ninguna cadena— se reprocesa el último bloque
/// conocido en vez de no hacer nada, para que el bombéo (y por lo tanto el
/// anillo de control) siga ocurriendo.
/// Ticks extra durante los cuales se sigue enviando audio al retorno de
/// monitoreo tras caer por debajo del umbral, para no cortar colas de release.
const MONITOR_TAIL_TICKS: u8 = 12;

/// Ventana durante la cual asumimos que el motor de audio sigue reproduciendo
/// señal real aunque el callback de CPAL (~21-23 ms) todavía no haya publicado
/// el siguiente bloque en este tick de 60 Hz (~16.7 ms). Sin esta retención,
/// cada 3er-4to tick `tap.take()` devuelve `false`, la ventana procesaba un
/// bloque de silencio y volcaba colas viejas de reverb/delay/filtro de vuelta
/// al `monitor_ring` mezclándolas fuera de fase sobre el audio real.
const ENGINE_AUDIO_HOLD: Duration = Duration::from_millis(180);

/// Colchón objetivo en frames para el anillo de monitoreo cuando el usuario
/// está tocando el teclado en pantalla del plugin (~32 ms a 48 kHz = 1.5 bloques
/// CPAL de 1024 frames), evitando underruns por jitter de `sleep`/X11.
const MONITOR_TARGET_BUFFER_FRAMES: usize = 1536;

/// Debounce para sincronizar `save_state()` en vivo hacia el motor de audio.
const STATE_SYNC_DEBOUNCE: Duration = Duration::from_millis(450);

fn hash_state_bytes(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

fn has_audible_signal(samples: &[f32]) -> bool {
    samples.iter().any(|s| s.is_finite() && s.abs() > 1e-6)
}

pub fn pump_plugin(
    instance: &mut dyn PluginInstance,
    feed: Option<&PluginTap>,
    scratch: &mut Vec<f32>,
    primed: &mut bool,
) {
    let mut tail = 0u8;
    let mut last_engine_audio = None;
    pump_plugin_paced(
        instance,
        feed,
        scratch,
        primed,
        None,
        &mut tail,
        Instant::now(),
        &mut last_engine_audio,
    );
}

#[allow(clippy::too_many_arguments)]
fn pump_plugin_paced(
    instance: &mut dyn PluginInstance,
    feed: Option<&PluginTap>,
    scratch: &mut Vec<f32>,
    primed: &mut bool,
    paced_frames: Option<usize>,
    monitor_tail_ticks: &mut u8,
    now: Instant,
    last_engine_audio: &mut Option<Instant>,
) {
    let mut input_silent = true;
    let frames = match feed {
        Some(tap) => {
            let capacity = tap.max_frames() * crate::scratch::NUM_CHANNELS;
            if scratch.len() < capacity {
                scratch.resize(capacity, 0.0);
            }
            if tap.take(scratch) {
                *primed = true;
                if has_audible_signal(scratch) {
                    *last_engine_audio = Some(now);
                    input_silent = false;
                } else {
                    input_silent = last_engine_audio
                        .is_none_or(|t| now.duration_since(t) >= ENGINE_AUDIO_HOLD);
                }
            } else if last_engine_audio
                .is_some_and(|t| now.duration_since(t) < ENGINE_AUDIO_HOLD)
            {
                // El motor está reproduciendo audio activamente y simplemente
                // todavía no terminó el próximo bloque CPAL (~21-23 ms): no
                // inyectar silencio ni empujar colas de la ventana al monitor.
                *monitor_tail_ticks = 0;
                return;
            }
            if input_silent {
                // El motor está en silencio (p. ej. transporte parado o
                // sintetizador sin audio de entrada): procesamos silencio del
                // tamaño exacto del período del tick para mantener la tasa de
                // muestreo real cuando el usuario toca el teclado del plugin.
                let target_frames = paced_frames
                    .unwrap_or_else(|| {
                        if *primed {
                            scratch.len() / crate::scratch::NUM_CHANNELS
                        } else {
                            tap.max_frames()
                        }
                    })
                    .clamp(1, tap.max_frames());
                scratch.resize(target_frames * crate::scratch::NUM_CHANNELS, 0.0);
                scratch.fill(0.0);
            }
            scratch.len() / crate::scratch::NUM_CHANNELS
        }
        None => {
            // Sin cinta, ni siquiera hace falta bombear audio: lo que hay que
            // bombear son los parámetros, y para eso `process()` con un bloque
            // de silencio alcanza. Es el camino del panel de plugins externos
            // (abrir un editor para tocar parámetros).
            if !*primed {
                let capacity = crate::scratch::MAX_BLOCK_FRAMES * crate::scratch::NUM_CHANNELS;
                *scratch = vec![0.0f32; capacity];
                *primed = true;
            } else {
                scratch.fill(0.0);
            }
            scratch.len() / crate::scratch::NUM_CHANNELS
        }
    };

    if frames == 0 {
        return;
    }
    instance.process(&mut AudioBuffer::new(scratch));

    // Si entró silencio pero el plugin de la ventana produjo señal audible
    // (p. ej. el usuario hizo click en el teclado de Vital), la devolvemos al
    // anillo de monitoreo del tap para que el motor la mezcle en la pista.
    if input_silent {
        if let Some(tap) = feed {
            if has_audible_signal(scratch) {
                *monitor_tail_ticks = MONITOR_TAIL_TICKS;
                tap.push_monitor(scratch);
            } else if *monitor_tail_ticks > 0 {
                *monitor_tail_ticks -= 1;
                tap.push_monitor(scratch);
            }
        }
    } else {
        *monitor_tail_ticks = 0;
    }
}

/// Configuración del bucle.
pub struct WrapperEventLoop {
    window: HikaruWrapperWindow,
    commands: Receiver<WrapperCommand>,
    feedback: Sender<ParamFeedback>,
    feed: Option<Arc<PluginTap>>,
    budget: TickBudget,
    /// Scratch del bloque que se le pasa a `process()`.
    ///
    /// Vive acá y no se aloca por tick. Cuando no hay audio nuevo del motor se
    /// reutiliza el último bloque (o silencio en el primero), que es
    /// justamente lo que hace falta para que el plugin siga siendo bombado.
    scratch: Vec<f32>,
    /// Si el `scratch` alguna vez recibió un bloque.
    primed: bool,
    /// Reloj del último bombeo y acumulador fraccionario de frames para que el
    /// monitoreo del teclado en pantalla corra a exactamente `sample_rate` Hz.
    last_pump: Option<Instant>,
    frac_frames: f64,
    monitor_tail_ticks: u8,
    /// Último instante en que el motor publicó audio audible por el tap.
    last_engine_audio: Option<Instant>,
    /// Cuadro pendiente de aplicar, puesto por [`Self::set_host_frame`].
    ///
    /// Vive acá y no se aplica al vuelo porque reposicionar la ventana exige
    /// preguntar la posición del DAW, y eso lo hace
    /// [`HikaruWrapperWindow::sync_with_host`] una vez por tick.
    pending_host_frame: Option<HostFrame>,
}

impl WrapperEventLoop {
    /// Arma el bucle sobre una ventana ya creada.
    ///
    /// `feed` es la cinta de audio del motor (ver [`crate::tap`]). `None`
    /// significa "este plugin no está en ninguna cadena": el editor abre y
    /// los parámetros se mueven, pero no hay display reactivo al audio.
    pub fn new(
        window: HikaruWrapperWindow,
        commands: Receiver<WrapperCommand>,
        feedback: Sender<ParamFeedback>,
        feed: Option<Arc<PluginTap>>,
        budget: TickBudget,
    ) -> Self {
        let scratch = Vec::new();
        Self {
            window,
            commands,
            feedback,
            feed,
            budget,
            scratch,
            primed: false,
            last_pump: None,
            frac_frames: 0.0,
            monitor_tail_ticks: 0,
            last_engine_audio: None,
            pending_host_frame: None,
        }
    }

    /// Corre hasta que la ventana se cierra.
    ///
    /// `instance` es la copia del plugin que vive en esta ventana —no la que
    /// suena en el motor— y es a la que se le entrega el audio del `feed`.
    pub fn run(&mut self, instance: &mut dyn PluginInstance) -> LoopExit {
        // El consumidor se conecta acá, con la ventana visible y el editor
        // embebido. Antes habría que acordarse de desconectarlo en cada
        // salida temprana, y un consumidor colgado hace que el motor siga
        // copiando bloques al pedo.
        if let Some(tap) = &self.feed {
            tap.attach_consumer();
        }

        let mut tick: u64 = 0;
        let mut raise_requested = false;
        let mut focus_requested = false;
        // Buffer de eco del host. Vive acá afuera del bucle y se limpia cada
        // tick: un `Vec::new()` por tick serían 60 allocaciones por ventana y
        // por segundo para algo que casi siempre está vacío.
        let mut host_echo: Vec<(u32, f32)> = Vec::new();
        // Debounce y deduplicación para sincronizar `save_state()` en vivo hacia
        // el motor de audio cuando el usuario elige un preset o edita estado
        // interno (wavetables, matriz de modulación) desde la ventana del plugin.
        let mut state_sync_due: Option<Instant> = None;
        let mut last_synced_state_hash: Option<u64> = None;

        let exit = loop {
            let tick_start = Instant::now();
            tick = tick.wrapping_add(1);
            host_echo.clear();

            // 1. Comandos del rack. `try_recv` no bloquea: el gesto del
            //    usuario no espera al servidor X11 ni viceversa.
            let mut close_requested = false;
            while let Ok(cmd) = self.commands.try_recv() {
                match cmd {
                    WrapperCommand::SetParam { id, value } => {
                        // Un rechazo se avisa una vez por cambio, no por tick:
                        // repetir el mismo error cada 16 ms llena el log de
                        // ruido y no aporta nada.
                        if instance.set_param_normalized(id, value) {
                            host_echo.push((id, value));
                        } else {
                            eprintln!(
                                "[hikaru-wrapper] '{}' rechazó set_param(id={id})",
                                instance.get_name()
                            );
                        }
                    }
                    WrapperCommand::SetParamMod { id, delta } => {
                        let _ = instance.set_param_modulation(id, delta);
                    }
                    WrapperCommand::Close => close_requested = true,
                    // Se marca y se aplica después de drenar la cola, para no
                    // mandar N viajes al servidor si llegaron varios `Raise`.
                    WrapperCommand::Raise => raise_requested = true,
                    WrapperCommand::Focus => focus_requested = true,
                    WrapperCommand::SetHostFrame(frame) => self.window.set_host_frame(frame),
                }
            }
            if close_requested {
                break LoopExit::Requested;
            }

            // 2. Bombear el plugin. SIEMPRE, no sólo cuando hay audio nuevo.
            //
            //    Este es el arreglo del espectro muerto. `process()` es lo
            //    único que drena el anillo de control de VST3 y lo único que
            //    arranca el `start_processing`; si se lo saltea cuando el
            //    tap está mudo (transporte parado, plugin fuera de la
            //    cadena), los parámetros del rack se encolan para siempre y
            //    el editor nunca se entera de nada.
            self.pump_audio(instance);

            // 3. El equivalente por formato a un `idle()`/`on_timer()`.
            //    VST3 no expone un idle de host en el SDK que se usa, así que
            //    la implementación no hace nada; CLAP sí tiene temporizadores
            //    declarados por el plugin y hay que dispararlos.
            instance.gui_tick();

            // 3-a. Si el propio editor pidió cambiar de tamaño (VST3
            //      `IPlugFrame::resizeView` o CLAP `host_gui.request_resize`),
            //      ajustar la ventana del envoltorio para acompañarlo.
            if let Some((w, h)) = instance.poll_editor_resize() {
                self.window.resize_for_editor(w, h);
            }

            // 3-bis. Cambios de parámetro que nacen dentro del editor del
            //       plugin (el usuario movió un knob de la GUI nativa).
            //
            //       Es el flujo inverso del que el rack manda: acá el valor
            //       lo puso el usuario, en la ventana del plugin, y tiene que
            //       llegar al rack y al motor de audio.
            //
            //       Se manda **ya**, sin esperar la cadencia del feedback:
            //       durante un arrastre, 83 ms de retraso se ven.
            if tick.is_multiple_of(PARAM_POLL_TICKS) {
                let editor = instance.poll_param_changes();
                let state_dirty = instance.take_state_dirty();
                // Los cambios normales de 1..7 knobs ya viajan lock-free vía
                // `SetExternalPluginParam`: evitar llamar `save_state()`/`load_state()`
                // en cada gesto porque `load_state()` bloquea el mutex del motor de
                // audio y resetea voces activas. Sólo sincronizamos snapshot binario
                // ante una ráfaga de preset (>=8 parámetros) o un `mark_dirty` sin
                // cambios de parámetro (wavetables, matriz de modulación, etc.).
                if editor.len() >= 8 || (state_dirty && editor.is_empty()) {
                    state_sync_due = Some(Instant::now() + STATE_SYNC_DEBOUNCE);
                }
                if !editor.is_empty() {
                    let _ = self.feedback.send(ParamFeedback {
                        changes: editor,
                        origin: FeedbackOrigin::Editor,
                        state: None,
                    });
                }
            }

            if let Some(due) = state_sync_due {
                if Instant::now() >= due {
                    state_sync_due = None;
                    if let Ok(bytes) = instance.save_state() {
                        if !bytes.is_empty() {
                            let h = hash_state_bytes(&bytes);
                            if last_synced_state_hash != Some(h) {
                                last_synced_state_hash = Some(h);
                                let _ = self.feedback.send(ParamFeedback {
                                    changes: Vec::new(),
                                    origin: FeedbackOrigin::Editor,
                                    state: Some(bytes),
                                });
                            }
                        }
                    }
                }
            }

            // 4. Eventos de la ventana: cierre, resize y redirección de
            //    entrada hacia el editor del plugin.
            match self.window.poll_events() {
                WindowEvent::Idle => {}
                WindowEvent::Resized { width, height } => {
                    // Lo que cambia es el **interior** del marco, no la
                    // ventana entera. Mandarle el tamaño exterior haría que el
                    // plugin dibujara más grande que su hueco, y con el marco
                    // puesto se vería el plugin tapando el titlebar.
                    let (iw, ih) = self.window.inner_size();
                    if iw > 0 && ih > 0 {
                        instance.resize_gui(u32::from(iw), u32::from(ih));
                    }
                    let _ = (width, height);
                }
                WindowEvent::CloseRequested => break LoopExit::UserClosed,
                WindowEvent::Destroyed => break LoopExit::Destroyed,
                WindowEvent::ConnectionLost(e) => break LoopExit::ConnectionLost(e),
            }

            if let Some(frame) = self.pending_host_frame.take() {
                self.window.set_host_frame(frame);
            }

            // 4-bis. Descubrir la vista del plugin mientras no aparezca, y
            //        re-enganchar al DAW: posición, tamaño y visibilidad. Va
            //        antes del apilado porque `sync_with_host` puede
            //        desmapear la ventana, y apilar una ventana desmapada es
            //        un viaje al servidor que no arregla nada.
            self.window.sync_with_host();
            self.window.maintain(tick);

            // 4-ter. Latido de superficie: le recuerda al plugin que su
            //        ventana es visible. Hay plugins que se quedan con la
            //        bandera de "no hay que repintar" puesta y congelan el
            //        editor en el color de fondo para siempre.
            if tick.is_multiple_of(SURFACE_HEARTBEAT_TICKS) {
                self.window.request_repaint();
            }

            if focus_requested && self.window.should_repair_focus() {
                self.window.repair_focus();
            }
            focus_requested = false;

            // 5. Envoltura (stacking) + cursor + feedback hacia el rack.
            self.window.tick_keep_alive(raise_requested);
            raise_requested = false;

            // El cursor sólo se escribe cuando cambia. X11 lo lleva por el
            // átomo `_NET_WM_CURSOR` con el nombre del puntero, así que cada
            // cambio es un viaje al servidor.
            if let Some(hint) = self.window.cursor() {
                self.window.set_cursor(hint);
            }

            if self.budget.feedback_due(tick) {
                self.emit_feedback(&host_echo);
            }

            let sleep_for = self.budget.period().saturating_sub(tick_start.elapsed());
            if !sleep_for.is_zero() {
                std::thread::sleep(sleep_for);
            }
        };

        // El consumidor de la cinta se suelta antes que el editor: si no, el
        // motor seguiría publicando bloques hacia una ventana que ya se está
        // apagando.
        if let Some(tap) = &self.feed {
            let dropped = tap.dropped_blocks();
            if dropped > 0 {
                eprintln!(
                    "[hikaru-wrapper] la cinta del motor tiró {dropped} bloques \
                     (esta ventana no llegó a procesarlos)"
                );
            }
            tap.detach_consumer();
        }

        exit
    }

    /// Pide al bucle que recoloque el editor dentro de la ventana del DAW.
    ///
    /// Atajo para el llamador, que es el hilo de la GUI: encola el comando
    /// y lo aplica en el próximo tick, junto con la traducción de
    /// coordenadas que hace [`HikaruWrapperWindow::sync_with_host`].
    pub fn set_host_frame(&mut self, frame: HostFrame) {
        self.pending_host_frame = Some(frame);
    }

    /// Las medidas del marco del editor.
    pub fn set_frame_metrics(&mut self, metrics: super::frame::FrameMetrics) {
        self.window.set_frame_metrics(metrics);
    }

    /// Si la ventana del envoltorio se creó sin decoraciones del WM.
    pub fn is_borderless(&self) -> bool {
        self.window.is_borderless()
    }

    /// Si la ventana quedó como hija del DAW en el árbol de X11.
    pub fn is_child_of_daw(&self) -> bool {
        self.window.is_child_of_daw()
    }

    /// Si la ventana está mapeada ahora mismo.
    pub fn is_visible(&self) -> bool {
        self.window.is_visible()
    }

    /// Le pasa un bloque de audio al plugin de la ventana.
    fn pump_audio(&mut self, instance: &mut dyn PluginInstance) {
        let now = Instant::now();
        let total_frames = self.feed.as_ref().map(|tap| {
            let sr = tap.sample_rate().max(8_000.0) as f64;
            let dt = match self.last_pump {
                Some(prev) => now.duration_since(prev).as_secs_f64().clamp(0.001, 0.06),
                None => self.budget.period().as_secs_f64(),
            };
            let exact = dt * sr + self.frac_frames;
            let max_total = tap.max_frames().saturating_mul(2).max(64);
            let whole = (exact.floor() as usize).clamp(64, max_total);
            self.frac_frames = (exact - whole as f64).clamp(0.0, 1.0);
            whole
        });
        self.last_pump = Some(now);

        match (self.feed.as_deref(), total_frames) {
            (Some(tap), Some(mut remaining)) => {
                let max_chunk = tap.max_frames().max(64);
                let mut chunks = 0usize;
                while remaining > 0 && chunks < 2 {
                    let chunk = remaining.min(max_chunk);
                    remaining -= chunk;
                    chunks += 1;
                    pump_plugin_paced(
                        instance,
                        Some(tap),
                        &mut self.scratch,
                        &mut self.primed,
                        Some(chunk),
                        &mut self.monitor_tail_ticks,
                        now,
                        &mut self.last_engine_audio,
                    );
                    // Si el motor está reproduciendo audio real, no hace falta
                    // un segundo sub-bloque de monitoreo en el mismo tick.
                    if self
                        .last_engine_audio
                        .is_some_and(|t| now.duration_since(t) < ENGINE_AUDIO_HOLD)
                    {
                        break;
                    }
                }
                // Si el usuario está tocando el teclado del plugin (monitoreo
                // activo) y el anillo cayó por debajo del colchón objetivo,
                // completamos un bloque extra de pre-buffer para que el
                // callback CPAL de 1024 frames nunca sufra underrun.
                if self.monitor_tail_ticks > 0
                    && self
                        .last_engine_audio
                        .is_none_or(|t| now.duration_since(t) >= ENGINE_AUDIO_HOLD)
                {
                    let buffered = tap.monitor_buffered_frames();
                    if buffered < MONITOR_TARGET_BUFFER_FRAMES {
                        let deficit = (MONITOR_TARGET_BUFFER_FRAMES - buffered).min(max_chunk);
                        if deficit >= 64 {
                            pump_plugin_paced(
                                instance,
                                Some(tap),
                                &mut self.scratch,
                                &mut self.primed,
                                Some(deficit),
                                &mut self.monitor_tail_ticks,
                                now,
                                &mut self.last_engine_audio,
                            );
                        }
                    }
                }
            }
            _ => {
                pump_plugin_paced(
                    instance,
                    self.feed.as_deref(),
                    &mut self.scratch,
                    &mut self.primed,
                    total_frames,
                    &mut self.monitor_tail_ticks,
                    now,
                    &mut self.last_engine_audio,
                );
            }
        }
    }

    /// Manda a la UI el estado de los parámetros que el editor tiene ahora.
    ///
    /// El intercambio es **bidireccional** y son dos fuentes distintas:
    ///
    /// - Lo que el plugin movió por su cuenta ([`poll_param_changes`]): el
    ///   usuario tocó un knob dentro del editor y el rack tiene que enterarse.
    ///   Eso se manda en el tick, sin esperar: durante un arrastre la espera
    ///   se ve (ver [`PARAM_POLL_TICKS`]).
    /// - Lo que el host mandó y el editor aceptó (`host_echo`): cierra el
    ///   círculo. Sin esto, un knob del rack que el plugin recorta o cuantiza
    ///   queda mostrando un valor que el plugin nunca tuvo, y el usuario ve
    ///   "el knob salta solo" al siguiente frame.
    ///
    /// Se manda sólo cuando hay algo: un evento vacío cada 83 ms despierta al
    /// hilo principal para nada.
    fn emit_feedback(&mut self, host_echo: &[(u32, f32)]) {
        if !host_echo.is_empty() {
            let _ = self.feedback.send(ParamFeedback {
                changes: host_echo.to_vec(),
                origin: FeedbackOrigin::Host,
                state: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_budget_is_sixty_hertz() {
        let b = TickBudget::default();
        assert_eq!(b.fps(), 60);
        assert_eq!(b.period(), Duration::from_nanos(16_666_666));
    }

    #[test]
    fn a_zero_fps_does_not_divide_by_zero() {
        // Un `fps(0)` con `1_000_000_000 / 0` es un panic en el hilo de la
        // ventana, y cae en el peor momento: cuando el DAW ya está corriendo.
        let b = TickBudget::new(0, 0);
        assert_eq!(b.fps(), 1);
        assert!(b.period() > Duration::ZERO);
    }

    #[test]
    fn feedback_is_periodic_and_not_every_tick() {
        let b = TickBudget::default();
        assert!(!b.feedback_due(1));
        assert!(!b.feedback_due(4));
        assert!(b.feedback_due(5));
        assert!(!b.feedback_due(6));
        assert!(b.feedback_due(10));
    }

    #[test]
    fn feedback_every_zero_would_never_be_due() {
        // Un `feedback_every` de 0 es módulo por cero: otra división que sólo
        // explota en producción.
        let b = TickBudget::new(60, 0);
        assert!(b.feedback_due(1));
    }

    #[test]
    fn only_clean_exits_are_clean() {
        assert!(LoopExit::Requested.is_clean());
        assert!(LoopExit::UserClosed.is_clean());
        assert!(!LoopExit::Destroyed.is_clean());
        assert!(!LoopExit::ConnectionLost("x".into()).is_clean());
    }

    #[test]
    fn missed_tap_tick_during_active_playback_does_not_leak_tail_into_monitor() {
        struct TailPlugin {
            had_audio: bool,
        }
        impl PluginInstance for TailPlugin {
            fn process(&mut self, buffer: &mut AudioBuffer) {
                if has_audible_signal(buffer.samples) {
                    self.had_audio = true;
                } else if self.had_audio {
                    // Simula cola de reverb/delay/filtro cuando entra silencio.
                    for s in buffer.samples.iter_mut() {
                        *s = 0.25;
                    }
                }
            }
            fn show_gui_embedded(&mut self, _h: crate::RawWindowHandle) -> Result<(), String> {
                Ok(())
            }
            fn show_gui_floating(&mut self) -> Result<(), String> {
                Ok(())
            }
            fn hide_gui(&mut self) {}
            fn get_name(&self) -> &str {
                "TailPlugin"
            }
        }

        let tap = PluginTap::new(48_000.0, 256);
        tap.attach_consumer();
        let mut plugin = TailPlugin { had_audio: false };
        let mut scratch = Vec::new();
        let mut primed = false;
        let mut tail = 0u8;
        let mut last_engine_audio = None;
        let t0 = Instant::now();

        // Tick 1: el motor publica audio real.
        tap.publish(&vec![0.6f32; 256 * 2]);
        pump_plugin_paced(
            &mut plugin,
            Some(&tap),
            &mut scratch,
            &mut primed,
            Some(256),
            &mut tail,
            t0,
            &mut last_engine_audio,
        );
        assert_eq!(tap.monitor_buffered_frames(), 0);

        // Tick 2 (16 ms después): CPAL aún no publicó el próximo bloque (`tap.take() == false`).
        // NO debe inyectar silencio ni volcar la cola del plugin al `monitor_ring`.
        pump_plugin_paced(
            &mut plugin,
            Some(&tap),
            &mut scratch,
            &mut primed,
            Some(256),
            &mut tail,
            t0 + Duration::from_millis(16),
            &mut last_engine_audio,
        );
        assert_eq!(
            tap.monitor_buffered_frames(),
            0,
            "un tick intermedio entre bloques CPAL nunca debe contaminar monitor_ring"
        );
    }
}