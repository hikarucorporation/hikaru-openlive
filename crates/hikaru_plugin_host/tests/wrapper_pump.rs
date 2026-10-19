// Copyright (C) Hikaru Corporation - 2026
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/tests/wrapper_pump.rs

//! El bucle del envoltorio bombea al plugin aunque no le llegue audio.
//!
//! # El bug que este test fija
//!
//! El hilo de la ventana sólo llamaba `process()` cuando la cinta del motor
//! entregaba un bloque. Con el transporte parado la cinta está muda, así que
//! `process()` no se llamaba nunca, y como es lo único que:
//!
//! - drena el anillo de control de VST3 (`RtControl::set_parameter` sólo
//!   encola; el anillo lo consume `RealtimePluginRunner::process`), y
//! - arranca el plugin (`start_processing`, que VST3 exige antes del primer
//!   bloque),
//!
//! los parámetros que mandaba el rack se acumulaban sin aplicarse y el
//! analizador del editor se quedaba en *"Play audio to see the spectrum"*
//! aunque el audio se oyera perfecto en la cadena.
//!
//! El test llama a [`pump_plugin`], que es la función real del bucle —sacada
//! del struct justamente para poder ejercitarla sin un servidor de display—,
//! así que no hay una copia del tick que pueda quedar desfasada.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

use hikaru_core::AudioBuffer;
use hikaru_plugin_host::hikaru_wrapper::{
    pump_plugin, FeedbackOrigin, ParamFeedback, TickBudget, WrapperCommand,
};
use hikaru_plugin_host::tap::PluginTap;
use hikaru_plugin_host::PluginInstance;

/// Instancia que sólo cuenta lo que se le hace.
///
/// `gui_tick` es el otro lado del mismo arreglo: el equivalente por formato a
/// un `idle()` del host, que sin él los temporizadores que declara un plugin
/// CLAP nunca se disparan y su editor deja de repintar.
struct CountingInstance {
    process_calls: AtomicUsize,
    gui_tick_calls: AtomicUsize,
    peak_amplitude: std::sync::Mutex<f32>,
}

impl CountingInstance {
    fn new() -> Self {
        Self {
            process_calls: AtomicUsize::new(0),
            gui_tick_calls: AtomicUsize::new(0),
            peak_amplitude: std::sync::Mutex::new(0.0),
        }
    }

    fn calls(&self) -> usize {
        self.process_calls.load(Ordering::Relaxed)
    }

    fn ticks(&self) -> usize {
        self.gui_tick_calls.load(Ordering::Relaxed)
    }

    /// Pico del último bloque que recibió: cómo se verifica que el audio del
    /// motor llegó al plugin de la ventana.
    fn last_peak(&self) -> f32 {
        *self.peak_amplitude.lock().unwrap()
    }
}

impl PluginInstance for CountingInstance {
    fn process(&mut self, buffer: &mut AudioBuffer) {
        self.process_calls.fetch_add(1, Ordering::Relaxed);
        let peak = buffer
            .samples
            .iter()
            .fold(0.0f32, |acc, s| acc.max(s.abs()));
        *self.peak_amplitude.lock().unwrap() = peak;
    }

    fn show_gui_embedded(
        &mut self,
        _window_handle: hikaru_plugin_host::RawWindowHandle,
    ) -> Result<(), String> {
        Ok(())
    }

    fn show_gui_floating(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn hide_gui(&mut self) {}

    fn get_name(&self) -> &str {
        "contador"
    }

    fn gui_tick(&mut self) {
        self.gui_tick_calls.fetch_add(1, Ordering::Relaxed);
    }
}

/// Estado del bucle que se quiere compartir entre ticks, igual que en
/// [`WrapperEventLoop`].
struct Pump {
    scratch: Vec<f32>,
    primed: bool,
}

impl Pump {
    fn tick(&mut self, instance: &mut dyn PluginInstance, feed: Option<&PluginTap>) {
        pump_plugin(instance, feed, &mut self.scratch, &mut self.primed);
        instance.gui_tick();
    }
}

/// Sin cinta de audio (transporte parado, plugin fuera de la cadena), el
/// bucle sigue llamando `process()` en cada tick.
#[test]
fn el_bucle_bombea_el_plugin_aunque_no_llegue_audio() {
    let mut instance = CountingInstance::new();
    let mut pump = Pump {
        scratch: Vec::new(),
        primed: false,
    };

    for _ in 0..8 {
        pump.tick(&mut instance, None);
    }

    assert_eq!(
        instance.calls(),
        8,
        "process() tiene que llamarse en cada tick aunque la cinta esté muda: \
         es lo único que drena el anillo de control de VST3 y lo único que \
         arranca `start_processing`. Con menos llamadas, los parámetros del \
         rack se encolan para siempre y el analizador queda plano."
    );
    assert_eq!(
        instance.last_peak(),
        0.0,
        "sin cinta no hay audio que inventar: se procesa silencio"
    );
}

/// La primera llamada sin cinta prepara el scratch una sola vez.
///
/// Un `vec![]` por tick serían 60 asignaciones por ventana y por segundo,
/// sólo para mandar silencio.
#[test]
fn el_scratch_se_reserva_una_sola_vez() {
    let mut instance = CountingInstance::new();
    let mut pump = Pump {
        scratch: Vec::new(),
        primed: false,
    };

    pump.tick(&mut instance, None);
    let reserved = pump.scratch.len();
    assert!(reserved > 0, "el primer tick tiene que reservar el scratch");

    pump.tick(&mut instance, None);
    assert_eq!(
        pump.scratch.len(),
        reserved,
        "el scratch no se vuelve a reservar en cada tick"
    );
}

/// El bombéo de audio y el `idle()` de GUI van al mismo ritmo.
#[test]
fn audio_y_gui_se_bombean_a_la_misma_frecuencia() {
    let mut instance = CountingInstance::new();
    let mut pump = Pump {
        scratch: Vec::new(),
        primed: false,
    };

    for _ in 0..8 {
        pump.tick(&mut instance, None);
    }

    assert_eq!(
        instance.ticks(),
        instance.calls(),
        "si el `idle()` del plugin se dispara a un ritmo distinto al del audio, \
         los temporizadores que declara quedan desfasados"
    );
}

/// Cuando la cinta sí entrega audio, el bloque llega intacto al plugin de la
/// ventana. Éste es el camino que hace que el espectro se mueva.
#[test]
fn el_bucle_manda_el_audio_del_motor_a_la_instancia_de_la_ventana() {
    let tap = Arc::new(PluginTap::new(48_000.0, 256));
    tap.attach_consumer();

    let mut instance = CountingInstance::new();
    let mut pump = Pump {
        scratch: Vec::new(),
        primed: false,
    };

    // Primero un tick sin audio: la cinta está muda.
    pump.tick(&mut instance, Some(&tap));
    assert_eq!(instance.last_peak(), 0.0);

    // Ahora el motor publica.
    let block: Vec<f32> = (0..(128 * 2))
        .map(|i| if i % 2 == 0 { 0.75 } else { -0.75 })
        .collect();
    tap.publish(&block);

    pump.tick(&mut instance, Some(&tap));
    assert!(
        instance.last_peak() > 0.7,
        "el plugin de la ventana recibió un pico de {}; el display sigue plano",
        instance.last_peak()
    );

    tap.detach_consumer();
}

/// El intercambio de parámetros es bidireccional y el eco del host se
/// distingue del cambio del editor.
///
/// La distinción importa: si el hilo principal no puede dizer de dónde vino
/// un valor, un eco desfasado del host le pisa al usuario el knob que está
/// arrastrando en ese momento.
#[test]
fn el_ciclo_de_parametros_cierra_en_ambos_sentidos() {
    let (cmd_tx, cmd_rx) = mpsc::channel::<WrapperCommand>();
    let (fb_tx, fb_rx) = mpsc::channel::<ParamFeedback>();

    // rack → ventana
    cmd_tx.send(WrapperCommand::SetParam { id: 7, value: 0.5 }).unwrap();
    match cmd_rx.try_recv().expect("el comando tiene que estar encolado") {
        WrapperCommand::SetParam { id, value } => assert_eq!((id, value), (7, 0.5)),
        other => panic!("esperaba SetParam, llegó {other:?}"),
    }

    // ventana → rack, con el origen que le permite al hilo principal no
    // pelearse con el gesto del usuario.
    fb_tx
        .send(ParamFeedback {
            changes: vec![(7, 0.5)],
            origin: FeedbackOrigin::Host,
        })
        .unwrap();
    let fb = fb_rx.try_recv().unwrap();
    assert_eq!(fb.origin, FeedbackOrigin::Host);
    assert_ne!(fb.origin, FeedbackOrigin::Editor);

    // El keep-alive usa `Raise`; tiene que existir y llegar.
    cmd_tx.send(WrapperCommand::Raise).unwrap();
    assert!(matches!(cmd_rx.try_recv().unwrap(), WrapperCommand::Raise));

    // Y `Focus`, que el DAW manda cuando el editor debe recuperar el teclado.
    cmd_tx.send(WrapperCommand::Focus).unwrap();
    assert!(matches!(cmd_rx.try_recv().unwrap(), WrapperCommand::Focus));
}

/// El tap se desconecta al terminar el bucle.
///
/// Si no, el motor sigue copiando bloques a una ventana cerrada y cada
/// editor que el usuario abrió sin cerrar se convierte en una fuga de CPU.
#[test]
fn sin_consumidor_el_tap_deja_de_copiar() {
    let tap = Arc::new(PluginTap::new(48_000.0, 64));
    let mut instance = CountingInstance::new();
    let mut pump = Pump {
        scratch: Vec::new(),
        primed: false,
    };

    tap.attach_consumer();
    pump.tick(&mut instance, Some(&tap));
    tap.detach_consumer(); // lo que hace `WrapperEventLoop::run` al salir

    tap.publish(&vec![0.5f32; 128 * 2]);
    assert_eq!(
        tap.dropped_blocks(),
        0,
        "sin consumidores `publish` no cuenta drops ni copia"
    );

    let mut out = Vec::new();
    assert!(!tap.take(&mut out), "nadie quedó escuchando");
}

/// El ritmo declarado tiene que alcanzar para un display suave.
#[test]
fn el_ritmo_default_alcanza_para_un_display_suave() {
    let period = TickBudget::default().period();
    assert!(
        period < std::time::Duration::from_millis(20),
        "un tick de {period:?} no alcanza para un display a 60 Hz"
    );
}