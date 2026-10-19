// Copyright (C) Hikaru Corporation - 2026
// Hikaru Clap Launcher
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/src/clap/hikaru_clap_launcher.rs

use crate::scratch::{NUM_CHANNELS, PlanarScratch};
use crate::{PluginInstance, PluginParamInfo};
use clack_extensions::gui::{GuiApiType, GuiConfiguration, GuiSize, HostGui, HostGuiImpl, PluginGui, Window};
use clack_extensions::log::{HostLog, HostLogImpl, LogSeverity};
use clack_extensions::params::PluginParams;
use clack_extensions::posix_fd::{FdFlags, HostPosixFd, HostPosixFdImpl};
use clack_extensions::timer::{HostTimer, HostTimerImpl, PluginTimer, TimerId};
use clack_host::events::event_types::ParamValueEvent;
use clack_host::events::io::{EventBuffer, OutputEvents};
use clack_host::prelude::*;
use hikaru_core::AudioBuffer;
use raw_window_handle::RawWindowHandle;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

#[cfg(target_os = "linux")]
use crate::platform::linux::X11Connection;

/// Lo que el plugin pidió al host desde otro hilo y hay que atender en el
/// hilo principal.
///
/// CLAP no deja llamar a la GUI desde el hilo de audio: el plugin deja una
/// bandera con `request_*` y el host tiene que decidir, desde su hilo
/// principal, si lo hace. Si nadie lee la bandera, el plugin queda esperando
/// para siempre.
#[derive(Debug, Default)]
struct GuiRequests {
    show: bool,
    hide: bool,
    resize: Option<GuiSize>,
}

struct HikaruShared {
    log_to_stdout: bool,
    /// El plugin pidió que le llamen al callback de hilo principal.
    ///
    /// Es lo que dispara `PluginInstance::call_on_main_thread_callback`, y es
    /// el mecanismo que usa un plugin CLAP para "despertar" su GUI cuando
    /// otro de sus hilos terminó un trabajo. Antes esto era un no-op: un
    /// plugin que pedía callback y no lo recibía se quedaba con el editor
    /// congelado esperando un mensaje que nunca llegaba.
    main_thread_callback: AtomicBool,
    /// Peticiones de GUI pendientes de atender en el tick.
    gui: Mutex<GuiRequests>,
}

impl HikaruShared {
    fn new() -> Self {
        Self {
            log_to_stdout: true,
            main_thread_callback: AtomicBool::new(false),
            gui: Mutex::new(GuiRequests::default()),
        }
    }

    /// Si el plugin pidió callback y limpia la bandera.
    ///
    /// Limpiar en el mismo paso que leer es lo que evita que se sirva dos
    /// veces por un solo pedido: la bandera es un "pendiente", no un
    /// contador, y perder uno es preferible a darle un callback con el
    /// plugin en un estado que ya cambió.
    fn take_callback_request(&self) -> bool {
        self.main_thread_callback.swap(false, Ordering::AcqRel)
    }
}

impl<'a> SharedHandler<'a> for HikaruShared {
    fn request_restart(&self) {
        eprintln!("[CLAP Host] Plugin request_restart");
    }
    fn request_process(&self) {}
    fn request_callback(&self) {
        // No se sirve acá: `Shared` también lo ve el hilo de audio, y la
        // llamada al callback tiene que salir del hilo principal. Se marca y
        // lo atiende `ClapInstance::gui_tick`.
        self.main_thread_callback.store(true, Ordering::Release);
    }
}

impl HostLogImpl for HikaruShared {
    fn log(&self, severity: LogSeverity, message: &str) {
        if self.log_to_stdout {
            match severity {
                LogSeverity::Info => println!("[CLAP Plugin] {}", message),
                LogSeverity::Warning => eprintln!("[CLAP Plugin Warning] {}", message),
                LogSeverity::Error => eprintln!("[CLAP Plugin Error] {}", message),
                LogSeverity::Debug => eprintln!("[CLAP Plugin Debug] {}", message),
                _ => eprintln!("[CLAP Plugin Log] {}", message),
            }
        }
    }
}

impl HostGuiImpl for HikaruShared {
    fn resize_hints_changed(&self) {}

    /// El plugin quiere otro tamaño.
    ///
    /// No se redimensiona desde acá: CLAP exige que la GUI sólo se toque desde
    /// el hilo principal, y `request_resize` puede llegar desde el hilo de
    /// audio. Se anota el tamaño y `gui_tick` lo aplica.
    fn request_resize(&self, new_size: GuiSize) -> Result<(), HostError> {
        if let Ok(mut gui) = self.gui.lock() {
            gui.resize = Some(new_size);
        }
        Ok(())
    }

    fn request_show(&self) -> Result<(), HostError> {
        if let Ok(mut gui) = self.gui.lock() {
            gui.show = true;
        }
        Ok(())
    }

    fn request_hide(&self) -> Result<(), HostError> {
        if let Ok(mut gui) = self.gui.lock() {
            gui.hide = true;
        }
        Ok(())
    }

    fn closed(&self, was_destroyed: bool) {
        println!(
            "[CLAP Host] GUI closed (was_destroyed={})",
            was_destroyed
        );
    }
}

/// Temporizadores que el plugin registró, con su próxima hora de disparo.
///
/// Antes el host guardaba `Vec<TimerId>` y `TimerId` se construía con el
/// **período** como valor. Eso rompía dos cosas a la vez:
///
/// - Dos temporizadores con el mismo período recibían el mismo id, así que
///   desregistrar uno desataba también al otro.
/// - No había ninguna hora de disparo, con lo cual `gui_tick` no tenía forma
///   de saber a quién tocarle el timbre: el vector era una lista de
///   suspendingers.
///
/// CLAP garantiza que los ids son únicos *por instancia de plugin*, así que
/// acá van con un contador propio.
struct TimerRegistry {
    /// `(id, período en ms, próximo disparo en ms)`.
    entries: Vec<(TimerId, u32, u64)>,
    next_id: u32,
}

impl TimerRegistry {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            // 0 está reservado como "sin temporizador" en varias implementaciones.
            next_id: 1,
        }
    }

    fn register(&mut self, period_ms: u32, now_ms: u64) -> TimerId {
        let id = TimerId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.entries.push((id, period_ms, now_ms.saturating_add(period_ms as u64)));
        id
    }

    fn unregister(&mut self, id: TimerId) {
        self.entries.retain(|(entry, _, _)| *entry != id);
    }

    /// Devuelve los ids que vencen en este instante y reprograma el siguiente.
    ///
    /// Se agrupan para que `gui_tick` pueda disparar un solo `on_timer` por
    /// id vencido, en vez de recalcular el vencimiento en el bucle del host.
    fn take_due(&mut self, now_ms: u64) -> Vec<TimerId> {
        let mut due = Vec::new();
        for (id, period, next) in &mut self.entries {
            if now_ms >= *next {
                due.push(*id);
                *next = now_ms.saturating_add(*period as u64);
            }
        }
        due
    }

    fn len(&self) -> usize {
        self.entries.len()
    }
}

struct HikaruMainThread<'a> {
    #[allow(dead_code)]
    shared: &'a HikaruShared,
    timers: Mutex<TimerRegistry>,
    /// Reloj del hilo. Sólo para los temporizadores: se usa
    /// `Instant` y no el reloj de pared para que un ajuste de hora no los
    /// dispare en bucle.
    started: std::time::Instant,
}

impl<'a> MainThreadHandler<'a> for HikaruMainThread<'a> {}

impl HikaruMainThread<'_> {
    fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    /// Temporizadores a los que hay que tocarle el timbre ahora.
    ///
    /// Vive en el handler de hilo principal a propósito: es el único sitio
    /// donde CLAP garantiza que se está en el hilo correcto para llamar al
    /// callback del plugin.
    fn take_due_timers(&self) -> Vec<TimerId> {
        match self.timers.lock() {
            Ok(mut timers) => timers.take_due(self.elapsed_ms()),
            Err(_) => Vec::new(),
        }
    }
}

impl HostTimerImpl for HikaruMainThread<'_> {
    fn register_timer(&self, period_ms: u32) -> Result<TimerId, HostError> {
        let mut timers = self
            .timers
            .lock()
            .map_err(|_| HostError::Message("el registro de temporizadores está envenenado"))?;
        let id = timers.register(period_ms, self.elapsed_ms());
        println!(
            "[CLAP Host] Timer registrado: {}ms (id={}, {} activos)",
            period_ms,
            id.0,
            timers.len()
        );
        Ok(id)
    }

    fn unregister_timer(&self, timer_id: TimerId) -> Result<(), HostError> {
        if let Ok(mut timers) = self.timers.lock() {
            timers.unregister(timer_id);
        }
        println!("[CLAP Host] Timer desregistrado: {}", timer_id);
        Ok(())
    }
}

impl HostPosixFdImpl for HikaruMainThread<'_> {
    fn register_fd(&self, fd: std::os::unix::io::RawFd, flags: FdFlags) -> Result<(), HostError> {
        println!("[CLAP Host] POSIX FD registered: fd={}, flags={:?}", fd, flags);
        Ok(())
    }

    fn modify_fd(&self, fd: std::os::unix::io::RawFd, flags: FdFlags) -> Result<(), HostError> {
        println!("[CLAP Host] POSIX FD modified: fd={}, flags={:?}", fd, flags);
        Ok(())
    }

    fn unregister_fd(&self, fd: std::os::unix::io::RawFd) -> Result<(), HostError> {
        println!("[CLAP Host] POSIX FD unregistered: fd={}", fd);
        Ok(())
    }
}

/// Estado que el host guarda del lado del hilo de audio.
///
/// CLAP obliga a declarar un tipo concreto como `AudioProcessor`: es el valor
/// que el host entrega a `activate()` y que CLAP devuelve cuando llama al
/// callback de audio. Antes era `()`, lo cual está bien mientras no se
/// procese nada, pero deja sin lugar donde guardar el sample rate que el
/// plugin necesita.
///
/// Almacenarlo acá (y no en `Shared`) importa: `Shared` lo ven todos los
/// hilos, incluido el de la GUI, así que escribir el sample rate desde el hilo
/// de audio a través de él sería una carrera de datos.
pub struct ClapAudioProcessor {
    /// Sample rate con el que se activó el plugin. Inmutable durante el
    /// procesamiento: cambiarlo exigiría re-activar el plugin, que es una
    /// operación de la GUI, no del callback.
    pub sample_rate: f32,
}

impl<'a> AudioProcessorHandler<'a> for ClapAudioProcessor {}

struct HikaruClapHost;

impl HostHandlers for HikaruClapHost {
    type Shared<'a> = HikaruShared;
    type MainThread<'a> = HikaruMainThread<'a>;
    type AudioProcessor<'a> = ClapAudioProcessor;

    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &Self::Shared<'_>) {
        builder
            .register::<HostLog>()
            .register::<HostGui>()
            .register::<HostTimer>()
            .register::<HostPosixFd>();
    }
}

/// Instancia CLAP alojada por Hikaru.
///
/// A diferencia de VST3, acá el plugin no se puede "poseer" en el hilo de audio
/// sin más: la API de CLAP separa la instancia principal (que se usa desde el
/// hilo de la GUI para el editor) del `AudioProcessor` que es lo que corre el
/// `process`. Por eso el processor vive en su propio campo y se activa de forma
/// perezosa en el primer bloque, cuando ya conocemos el sample rate real.
pub struct ClapInstance {
    name: String,
    plugin: clack_host::plugin::PluginInstance<HikaruClapHost>,
    /// El processor de audio. `None` hasta el primer `process()`, porque
    /// `activate` necesita el sample rate y en `load()` todavía no lo
    /// necesitamos.
    processor: Option<PluginAudioProcessor<HikaruClapHost>>,
    /// Buffers planares pre-asignados, igual que en el lado VST3.
    scratch: PlanarScratch,
    sample_rate: f32,
    last_error: Option<String>,
    gui: Option<PluginGui>,
    /// Extensión de parámetros CLAP. `None` si el plugin no la expone.
    params: Option<PluginParams>,
    /// Cambios de parámetro que llegan del hilo de control y se entregan al
    /// plugin como eventos en el próximo bloque de audio.
    ///
    /// CLAP no tiene una vía para cambiar un parámetro mientras el plugin
    /// está sonando: `params.flush` exige el handle de hilo principal
    /// *inactivo*, y un plugin que ya está sonando no lo tiene. La única vía
    /// legítima es meter un `ParamValueEvent` en la cola de entrada que se le
    /// pasa a `process()`. `set_param_normalized` publica acá y `process`
    /// drena; el anillo no bloquea, así que el hilo de audio no se frena
    /// esperando a que la GUI mueva un knob.
    param_queue: ParamQueue,
    /// `EventBuffer` reusado para construir la cola de entrada de cada bloque.
    /// Vive en el hilo de audio y no se aloca por bloque.
    event_buffer: EventBuffer,
    window_attached: bool,
    #[cfg(target_os = "linux")]
    _x11_conn: Option<X11Connection>,
}

/// Capacidad del anillo de cambios de parámetro.
///
/// Un knob genera un cambio por evento de puntero; 1024 holgadamente cubre
/// un barrido de automation entre dos bloques. Si se llena, el cambio se
/// descarta y `set_param_normalized` devuelve `false` en vez de bloquear al
/// hilo de GUI.
const PARAM_QUEUE_CAPACITY: usize = 1024;

/// Anillo SPSC para cambios de parámetro hacia el hilo de audio.
///
/// Productor: hilo de control (`set_param_normalized`).
/// Consumidor: hilo de audio (`process`).
pub struct ParamQueue {
    tx: rtrb::Producer<PendingParam>,
    rx: rtrb::Consumer<PendingParam>,
}

/// Un cambio de parámetro pendiente, en el formato que el host lo maneja:
/// id crudo y valor normalizado 0..=1.
#[derive(Clone, Copy)]
struct PendingParam {
    id: u32,
    value: f32,
}

impl ParamQueue {
    fn new(capacity: usize) -> Self {
        let (tx, rx) = rtrb::RingBuffer::new(capacity.max(1));
        Self { tx, rx }
    }

    /// Publica un cambio. `false` si la cola está llena (se lo descarta, no bloquea).
    fn push(&mut self, param: PendingParam) -> bool {
        self.tx.push(param).is_ok()
    }

    /// Vacía la cola en el lado del audio. El iterator no aloca.
    fn drain(&mut self) -> impl Iterator<Item = PendingParam> + '_ {
        std::iter::from_fn(move || self.rx.pop().ok())
    }
}


unsafe impl Send for ClapInstance {}

impl ClapInstance {
    pub fn load(path: &Path) -> Result<Self, String> {
        let _load_guard = crate::load_lock().lock().unwrap_or_else(|e| e.into_inner());

        println!("[CLAP] Cargando '{}'...", path.display());

        let entry = unsafe { PluginEntry::load(path) }
            .map_err(|e| format!("[CLAP] Fallo al cargar entry: {}", e))?;

        let plugin_factory = entry
            .get_plugin_factory()
            .ok_or("[CLAP] Entry no tiene plugin factory")?;

        let plugin_count = plugin_factory.plugin_count();
        if plugin_count == 0 {
            return Err("[CLAP] Factory sin plugins".into());
        }

        let desc = plugin_factory
            .plugin_descriptors()
            .next()
            .ok_or("[CLAP] Sin descriptor")?;

        let plugin_id = desc.id().ok_or("[CLAP] Descriptor sin id")?;
        let plugin_name = desc
            .name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| {
                path.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            });

        println!(
            "[CLAP] Plugin: '{}' (id={})",
            plugin_name,
            plugin_id.to_string_lossy()
        );

        let host_info = HostInfo::new("Hikaru", "Hikaru Corporation", "https://hikaru.dev", "0.1.0")
            .map_err(|e| format!("[CLAP] HostInfo: {}", e))?;

        let plugin = clack_host::plugin::PluginInstance::<HikaruClapHost>::new(
            |_| HikaruShared::new(),
            |shared| HikaruMainThread {
                shared,
                timers: Mutex::new(TimerRegistry::new()),
                started: std::time::Instant::now(),
            },
            &entry,
            plugin_id,
            &host_info,
        )
        .map_err(|e| format!("[CLAP] Instantiation failed: {:?}", e))?;

        println!("[CLAP] Plugin instanciado OK");

        let shared_handle = plugin.plugin_shared_handle();

        let gui = shared_handle.get_extension::<PluginGui>();
        let params = shared_handle.get_extension::<PluginParams>();

        if gui.is_some() {
            println!("[CLAP] GUI extension disponible para '{}'", plugin_name);
        } else {
            println!(
                "[CLAP] '{}' no tiene GUI extension",
                plugin_name
            );
        }

        if params.is_some() {
            println!("[CLAP] Params extension disponible para '{}'", plugin_name);
        }

        Ok(Self {
            name: plugin_name,
            plugin,
            processor: None,
            scratch: PlanarScratch::new(crate::scratch::MAX_BLOCK_FRAMES),
            sample_rate: 48_000.0,
            last_error: None,
            gui,
            params,
            param_queue: ParamQueue::new(PARAM_QUEUE_CAPACITY),
            event_buffer: EventBuffer::with_capacity(PARAM_QUEUE_CAPACITY),
            window_attached: false,
            #[cfg(target_os = "linux")]
            _x11_conn: None,
        })
    }

    /// Último error de procesamiento, para que el hilo de control lo muestre.
    pub fn take_last_error(&mut self) -> Option<String> {
        self.last_error.take()
    }

    /// Activa el processor de audio de forma perezosa.
    ///
    /// Va acá y no en `load()` porque `activate` exige el sample rate, y en
    /// `load()` todavía no sabemos el del host de audio (el motor lo fija más
    /// tarde, cuando abre el stream). También evita activar plugins que el
    /// usuario nunca llegó a procesar.
    fn ensure_processor(&mut self, frames: usize) -> bool {
        if self.processor.is_some() {
            // Ya activado: sólo falta arrancar el procesamiento.
            let Some(processor) = self.processor.as_mut() else {
                return false;
            };
            if let Err(e) = processor.start_processing() {
                // `ProcessingStarted` significa que ya estaba corriendo, que no
                // es un error: sólo que llegamos por el camino ya iniciado.
                if !matches!(e, PluginInstanceError::ProcessingStarted) {
                    self.last_error = Some(format!("[CLAP] start_processing: {e:?}"));
                    return false;
                }
            }
            return true;
        }

        let config = PluginAudioConfiguration {
            sample_rate: self.sample_rate as f64,
            // Declaramos un mínimo de 1 y un máximo igual a la capacidad del
            // scratch: el host tiene que poder entregar cualquier tamaño de
            // bloque que le llegue del stream de audio.
            min_frames_count: 1,
            max_frames_count: self.scratch.capacity().min(frames) as u32,
        };

        let sample_rate = self.sample_rate;
        match self.plugin.activate(|_, _| ClapAudioProcessor { sample_rate }, config) {
            Ok(processor) => {
                self.processor = Some(processor.into());

                // `activate()` entrega el processor en estado `Stopped`. El
                // primer `start_processing()` es lo que lo pone en
                // `ProcessingStarted`, y sin él el `match` de `process()`
                // cae en la rama `Stopped` y devuelve "process con el
                // processor detenido" en el primer bloque. Antes esto sólo
                // pasaba en las llamadas siguientes, porque el camino
                // "ya activado" de arriba sí arrancaba el procesamiento.
                let start_result = self
                    .processor
                    .as_mut()
                    .expect("processor recién asignado")
                    .start_processing();

                if let Err(e) = start_result {
                    // `ProcessingStarted` sólo puede aparecer si el plugin se
                    // auto-arrancó en `activate()`, que es legal.
                    if !matches!(e, PluginInstanceError::ProcessingStarted) {
                        self.last_error = Some(format!("[CLAP] start_processing: {e:?}"));
                        return false;
                    }
                }
                true
            }
            Err(e) => {
                self.last_error = Some(format!("[CLAP] activate: {e:?}"));
                false
            }
        }
    }

    /// Aplica lo que el plugin pidió en su extensión de GUI.
    ///
    /// Se separa de [`Self::gui_tick`] porque tiene su propio camino de
    /// salida (si el plugin pidió `hide`, la ventana se oculta) y porque
    /// tomar los `Mutex` compartidos fuera del resto del tick deja claro que
    /// no se mantienen bloqueados mientras se habla con el plugin.
    fn service_gui_requests(&mut self) {
        let Some(requests) = self.plugin.access_shared_handler(|shared| {
            let Ok(mut gui) = shared.gui.lock() else {
                return None;
            };
            let requests = GuiRequests {
                show: std::mem::take(&mut gui.show),
                hide: std::mem::take(&mut gui.hide),
                resize: gui.resize.take(),
            };
            if requests.show || requests.hide || requests.resize.is_some() {
                Some(requests)
            } else {
                None
            }
        }) else {
            return;
        };

        let Some(gui) = &self.gui else {
            return;
        };
        let handle = self.plugin.plugin_handle();

        if let Some(size) = requests.resize {
            if gui.can_resize(&handle) {
                let _ = gui.set_size(&handle, size);
            } else {
                eprintln!(
                    "[CLAP] '{}' pidió un resize a {}x{} pero declara que no se redimensiona",
                    self.name, size.width, size.height
                );
            }
        }
        // `hide` va después de `show`: si el plugin pidió las dos en el mismo
        // tick, la última intención gana y una ventana oculta es más segura
        // que una que quedó visible detrás de todo.
        if requests.show {
            let _ = gui.show(&handle);
        }
        if requests.hide {
            let _ = gui.hide(&handle);
        }
    }
}
impl PluginInstance for ClapInstance {
    /// Procesa un bloque interleaved estéreo a través del plugin CLAP.
    ///
    /// Mismo contrato RT que el lado VST3: sin alocaciones, sin locks, sin
    /// logueo. La diferencia es que CLAP no lleva el audio en el `clap_process`
    /// sino en descriptores `clap_audio_buffer` que el host arma antes de
    /// llamar a `process`, así que hay que construir esos descriptores en cada
    /// bloque. Los `AudioPorts` que los envuelven están pre-asignados en el
    /// scratch, entonces acá no se aloca.
    fn process(&mut self, buffer: &mut AudioBuffer) {
        let frames = buffer.samples.len() / NUM_CHANNELS;
        if frames == 0 {
            return;
        }
        if !self.scratch.fits(frames) {
            return;
        }
        if !self.ensure_processor(frames) {
            return;
        }

        self.scratch.deinterleave_stereo(buffer.samples, frames);

        // Todo el borrow de `scratch` y `processor` vive dentro de este bloque
        // para poder escribir en `self.last_error` y volver a usar `self`
        // fuera. Los `AudioPorts` son locales, pero no alocan por bloque:
        // construyen una lista de punteros a los buffers del scratch, y su
        // `Vec` interno se reserva una vez con capacidad 2×1.
        let outcome = {
            let Self {
                scratch,
                processor,
                param_queue,
                event_buffer,
                ..
            } = self;

            // Un solo puerto estéreo (dos canales). Es lo que espera la
            // mayoría de los efectos; un plugin con más puertos se
            // desalinearía en vez de recibir audio incorrecto.
            let mut input_audio = AudioPorts::with_capacity(2, 1);
            let mut output_audio = AudioPorts::with_capacity(2, 1);

            let (inputs, outputs) = scratch.channels_mut();

            // `split_at_mut` parte el slice en dos tramos disjuntos y así
            // obtenemos los dos `&mut` de canal. Indexar dos veces el mismo
            // slice (`&mut inputs[0]`, `&mut inputs[1]`) no compila porque el
            // borrow checker no puede probar que los índices difieran.
            let (in_l, in_r) = inputs.split_at_mut(1);
            let (out_l, out_r) = outputs.split_at_mut(1);

            let input_audio = input_audio.with_input_buffers([AudioPortBuffer {
                latency: 0,
                channels: AudioPortBufferType::f32_input_only([
                    InputChannel {
                        buffer: &mut in_l[0][..frames],
                        is_constant: false,
                    },
                    InputChannel {
                        buffer: &mut in_r[0][..frames],
                        is_constant: false,
                    },
                ]),
            }]);

            let mut output_audio = output_audio.with_output_buffers([AudioPortBuffer {
                latency: 0,
                channels: AudioPortBufferType::f32_output_only([
                    &mut out_l[0][..frames],
                    &mut out_r[0][..frames],
                ]),
            }]);

            // Eventos de entrada: los cambios de parámetro que publicó el hilo
            // de control, más lo que se enchufe más adelante (el MIDI del
            // secuenciador todavía no entra). El `EventBuffer` es reutilizado y
            // tiene capacidad de sobra, así que armarlo no aloca por bloque.
            event_buffer.clear();
            for pending in param_queue.drain() {
                let Some(clap_id) = ClapId::from_raw(pending.id) else {
                    continue;
                };
                let event = ParamValueEvent::new(
                    0, // time
                    clap_id,
                    Pckn::match_all(),
                    pending.value as f64,
                );
                event_buffer.push(&event);
            }
            let input_events = event_buffer.as_input();

            // `void()` descarta lo que el plugin produzca, en vez de obligar al
            // host a reservar un buffer de eventos que hoy no va a usar nadie.
            let mut output_events = OutputEvents::void();

            match processor.as_mut() {
                Some(PluginAudioProcessor::Started(started)) => started
                    .process(
                        &input_audio,
                        &mut output_audio,
                        &input_events,
                        &mut output_events,
                        None,
                        None,
                    )
                    .map(|_| ())
                    .map_err(|e| format!("[CLAP] process: {e:?}")),
                Some(PluginAudioProcessor::Stopped(_)) => {
                    Err("[CLAP] process con el processor detenido".to_string())
                }
                None => Err("[CLAP] process sin processor".to_string()),
            }
        };

        match outcome {
            Ok(()) => self.scratch.reinterleave_stereo(buffer.samples, frames),
            Err(e) => self.last_error = Some(e),
        }
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        // Cambiar el sample rate con el processor ya activado no es posible en
        // CLAP sin re-activar. Guardamos el valor nuevo para el próximo
        // activate y avisamos, en vez de fingir que el cambio surtió efecto.
        if self.processor.is_some() && (sample_rate - self.sample_rate).abs() > f32::EPSILON {
            eprintln!(
                "[CLAP] sample rate cambió a {} con el plugin ya activado; \
                 se aplicará al recargar el plugin",
                sample_rate
            );
        }
        self.sample_rate = sample_rate;
    }

    fn has_gui(&self) -> bool {
        self.gui.is_some()
    }

    fn get_gui_size(&self) -> Option<(u32, u32)> {
        // En CLAP gui.get_size requiere un gui.create previo.
        // Devolvemos None aquí para consultar el tamaño tras la creación en show_gui_embedded.
        None
    }

    fn notify_gui_embedded(&mut self) -> Option<(u32, u32)> {
        // After gui.create + gui.set_parent + gui.show, query the actual native size.
        let gui = self.gui.as_ref()?;
        let mut plugin_handle = self.plugin.plugin_handle();
        gui.get_size(&mut plugin_handle)
            .map(|size| (size.width, size.height))
    }

    fn show_gui_embedded(&mut self, handle: RawWindowHandle) -> Result<(), String> {
        let Some(gui) = &self.gui else {
            return Err(format!(
                "'{}' no expone editor gráfico: suena igual, pero no hay ventana que abrir",
                self.name
            ));
        };

        let mut plugin_handle = self.plugin.plugin_handle();

        #[cfg(target_os = "linux")]
        let (window_id, x11_conn) = match crate::platform::linux::get_or_create_x11_window(handle, 1024, 700) {
            Ok((wid, conn)) => (wid, conn),
            Err(e) => {
                return Err(format!("No se pudo preparar la ventana para '{}': {}", self.name, e));
            }
        };

        #[cfg(not(target_os = "linux"))]
        let window_id = match handle {
            RawWindowHandle::Xlib(h) => h.window as u32,
            RawWindowHandle::Xcb(h) => h.window.get().into(),
            _ => return Err(format!("[CLAP] Handle no soportado: {:?}", handle)),
        };

        // 1. gui.create siempre antes de cualquier otra llamada a la GUI
        let config = GuiConfiguration {
            api_type: GuiApiType::X11,
            is_floating: false,
        };

        if let Err(e) = gui.create(&mut plugin_handle, config) {
            return Err(format!(
                "El plugin '{}' rechazó crear su editor: {:?}",
                self.name, e
            ));
        }

        // 2. Consultar y aplicar el tamaño nativo del plugin
        if let Some(native_size) = gui.get_size(&mut plugin_handle) {
            println!(
                "[CLAP] Ajustando GUI de '{}' a tamaño nativo: {}x{}",
                self.name, native_size.width, native_size.height
            );
            let _ = gui.set_size(&mut plugin_handle, native_size);
        }

        // 3. Vincular ventana padre
        let parent_window = Window::from_x11_handle(window_id as std::ffi::c_ulong);

        if let Err(e) = unsafe { gui.set_parent(&mut plugin_handle, parent_window) } {
            let _ = gui.destroy(&mut plugin_handle);
            return Err(format!(
                "El plugin '{}' no aceptó la ventana del host: {:?}",
                self.name, e
            ));
        }

        // 4. Mostrar la GUI
        if let Err(e) = gui.show(&mut plugin_handle) {
            let _ = gui.destroy(&mut plugin_handle);
            return Err(format!(
                "El plugin '{}' no pudo mostrar su editor: {:?}",
                self.name, e
            ));
        }

        self.window_attached = true;

        #[cfg(target_os = "linux")]
        {
            self._x11_conn = x11_conn;
        }

        println!("[CLAP] GUI de '{}' abierta OK", self.name);
        Ok(())
    }

    fn show_gui_floating(&mut self) -> Result<(), String> {
        let Some(gui) = &self.gui else {
            return Err(format!("[CLAP] '{}' sin GUI", self.name));
        };

        let mut plugin_handle = self.plugin.plugin_handle();

        let config = GuiConfiguration {
            api_type: GuiApiType::X11,
            is_floating: true,
        };

        gui.create(&mut plugin_handle, config)
            .map_err(|e| format!("[CLAP] gui.create(floating) failed: {:?}", e))?;

        gui.show(&mut plugin_handle)
            .map_err(|e| format!("[CLAP] gui.show(floating) failed: {:?}", e))?;

        self.window_attached = true;
        Ok(())
    }

    fn hide_gui(&mut self) {
        if self.window_attached {
            if let Some(gui) = &self.gui {
                let mut plugin_handle = self.plugin.plugin_handle();
                let _ = gui.hide(&mut plugin_handle);
                let _ = gui.destroy(&mut plugin_handle);
            }
            self.window_attached = false;
            #[cfg(target_os = "linux")]
            {
                self._x11_conn = None;
            }
        }
    }

    fn get_name(&self) -> &str {
        &self.name
    }

    fn is_gui_open(&self) -> bool {
        self.window_attached
    }

    fn resize_gui(&mut self, width: u32, height: u32) {
        if let Some(gui) = &self.gui {
            let mut plugin_handle = self.plugin.plugin_handle();
            if gui.can_resize(&plugin_handle) {
                let _ = gui.set_size(&mut plugin_handle, GuiSize { width, height });
            }
        }
    }

    fn params_supported(&self) -> bool {
        self.params.is_some()
    }

    fn param_count(&mut self) -> usize {
        self.params
            .as_ref()
            .map(|params| {
                let plugin_handle = self.plugin.plugin_handle();
                params.count(&plugin_handle) as usize
            })
            .unwrap_or(0)
    }

    fn param_info(&mut self, index: usize) -> Option<PluginParamInfo> {
        let params = self.params.as_ref()?;
        let plugin_handle = self.plugin.plugin_handle();
        let mut buffer = clack_extensions::params::ParamInfoBuffer::new();
        let info = params.get_info(&plugin_handle, index as u32, &mut buffer)?;

        let name = String::from_utf8_lossy(info.name).into_owned();

        Some(PluginParamInfo {
            id: info.id.get(),
            name,
            unit: String::new(),
            min: info.min_value as f32,
            max: info.max_value as f32,
            default_value: info.default_value as f32,
            step_count: 0,
            can_automate: info.flags.contains(clack_extensions::params::ParamInfoFlags::IS_AUTOMATABLE),
        })
    }

    fn get_param_normalized(&mut self, id: u32) -> Option<f32> {
        let params = self.params.as_ref()?;
        let plugin_handle = self.plugin.plugin_handle();
        let clap_id = clack_host::prelude::ClapId::from_raw(id)?;
        let value = params.get_value(&plugin_handle, clap_id)?;
        Some(value.clamp(0.0, 1.0) as f32)
    }

    /// Publica un cambio de parámetro para el próximo bloque de audio.
    ///
    /// Dos caminos, según el estado del plugin:
    ///
    /// - **Inactivo** (aún no sonó): `params.flush` con el handle de hilo
    ///   principal es la vía directa y el valor aplica al instante.
    /// - **Activo** (sonando): CLAP no permite tocar la extensión de
    ///   parámetros desde el hilo principal mientras el plugin corre; el
    ///   handle inactivo no existe. El evento se encola y `process` lo
    ///   entrega como `ParamValueEvent` en la cola de entrada del bloque.
    ///
    /// Antes este camino devolvía `true` sin hacer nada — el caller creía
    /// que el knob se había movido y el DSP nunca se enteraba.
    fn set_param_normalized(&mut self, id: u32, value: f32) -> bool {
        if self.params.is_none() {
            return false;
        }
        let pending = PendingParam {
            id,
            value: value.clamp(0.0, 1.0),
        };

        // Camino directo para un plugin que todavía no está sonando.
        if let Some(mut inactive_handle) = self.plugin.inactive_plugin_handle() {
            let Some(clap_id) = ClapId::from_raw(id) else {
                return false;
            };
            let event = ParamValueEvent::new(0, clap_id, Pckn::match_all(), pending.value as f64);
            let mut buffer = EventBuffer::with_capacity(1);
            buffer.push(&event);
            let mut output_buffer = EventBuffer::with_capacity(1);
            let mut output_events = OutputEvents::from_buffer(&mut output_buffer);
            let Some(params) = self.params.as_mut() else {
                return false;
            };
            params.flush(
                &mut inactive_handle,
                &buffer.as_input(),
                &mut output_events,
            );
            return true;
        }

        // Plugin sonando: que lo aplique el hilo de audio.
        self.param_queue.push(pending)
    }

    fn poll_param_changes(&mut self) -> Vec<(u32, f32)> {
        Vec::new()
    }

    /// Bompea el editor del plugin desde el hilo principal.
    ///
    /// Hace las tres cosas que CLAP deja explícitamente en manos del host y
    /// que antes nadie hacía. Son la razón por la que el display de un plugin
    /// CLAP puede quedarse plano aunque el audio suene perfecto:
    ///
    /// 1. **Dispara los temporizadores** que el plugin registró a través de la
    ///    extensión `timer`. Un editor que invalida su display desde
    ///    `on_timer` no vuelve a pintar nunca si el host no llama. Antes el
    ///    host guardaba el registro y no disparaba nada: la extensión estaba
    ///    declarada pero muerta.
    /// 2. **Atiende el callback de hilo principal** que el plugin pidió con
    ///    `request_callback`. Es como un plugin "despierta" su GUI cuando otro
    ///    de sus hilos terminó algo.
    /// 3. **Aplica las peticiones de GUI** (`request_resize`, `request_show`,
    ///    `request_hide`), que por contrato de CLAP pueden llegar desde el
    ///    hilo de audio y sólo se tocan desde el principal.
    ///
    /// Si el plugin no declaró ninguna de las tres, el tick no hace nada: son
    /// unos `if` sobre banderas, no un coste fijo.
    fn gui_tick(&mut self) {
        // 1. Temporizadores. Se leen del handler de hilo principal porque es
        //    el único lugar donde se garantiza que estamos en el hilo que
        //    CLAP permite usar para llamar al plugin.
        let due = self.plugin.access_handler(|handler| handler.take_due_timers());
        if !due.is_empty() {
            let handle = self.plugin.plugin_handle();
            if let Some(timer) = handle.get_extension::<PluginTimer>() {
                for id in due {
                    timer.on_timer(&handle, id);
                }
            } else {
                eprintln!(
                    "[CLAP] '{}' registró temporizadores pero no expone la extensión timer",
                    self.name
                );
            }
        }

        // 2. Callback de hilo principal.
        let wants_callback = self
            .plugin
            .access_shared_handler(|shared| shared.take_callback_request());
        if wants_callback {
            self.plugin.call_on_main_thread_callback();
        }

        // 3. Peticiones de GUI.
        self.service_gui_requests();
    }
}

impl Drop for ClapInstance {
    fn drop(&mut self) {
        self.hide_gui();
    }
}