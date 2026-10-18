// Copyright (C) Hikaru Corporation - 2026
// Tape bus: el audio del motor hacia el editor nativo del plugin
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/src/tap.rs

//! Anillo lock-free SPSC que lleva el audio de la cadena hacia la ventana del
//! editor nativo.
//!
//! # Por qué existe
//!
//! La ventana que abre el botón `Editor` del DSP Rack corre en su propio hilo y
//! **no** usa la instancia que suena: hace su propio `Vst3Instance::load` (ver
//! `run_plugin_window` en `hikaru_gui::views::external_plugins_settings`), así
//! que ese plugin jamás recibe un bloque de audio. Los knobs se mueven porque
//! son derivados de parámetro, pero cualquier display reactivo al audio
//! —espectro, scope, forma de onda— queda permanentemente plano.
//!
//! Este módulo es la costura que le falta a esa ventana: el motor publica el
//! buffer ya procesado en el [`PluginTap`] del slot, y el hilo de la ventana lo
//! mete por [`PluginInstance::process`] en su instancia. El plugin de la
//! ventana recibe audio real y su display se mueve.
//!
//! # Qué NO resuelve
//!
//! Sigue habiendo **dos** instancias del plugin. La de la ventana no es la que
//! suena: su estado interno (preset, envolventes, LFO) es una copia, alimentada
//! por el espejo de parámetros que ya hace `send_param_to_open_plugin`. Para un
//! medidor de audio es exacto; para un synth no lo es. La solución de fondo es
//! una instancia sola (partir `PluginInstance` en mitad audio y mitad GUI), pero
//! eso es un rediseño del motor, no un parche.
//!
//! # Contrato RT
//!
//! [`PluginTap::publish`] corre en el hilo de audio: sin locks, sin
//! allocaciones, sin escribir sobre memoria que el consumidor esté leyendo. Si
//! el consumidor se atrasó, el bloque **nuevo se descarta** en vez de pisar uno
//! viejo: para un display perder un frame es invisible, y pisar el slot que el
//! otro hilo está copiando es una lectura a medio escribir.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU64, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::scratch::NUM_CHANNELS;

/// Bloques que caben en el anillo.
///
/// A 48 kHz con bloques de 1024 frames son ~21 ms por bloque y el hilo de la
/// ventana sondea cada 10 ms, así que ocho dan margen de sobra para un tirón de
/// GC o una llamada lenta del plugin.
const DEFAULT_CAPACITY: usize = 8;

/// Identifica el slot `(pista del motor, índice de la cadena)` al que pertenece
/// un tap.
///
/// Ojo con la pista: la UI trabaja con el índice del **mixer** y el motor con el
/// de la **cadena de efectos**, que es uno menos. La conversión es
/// `AppState::engine_track_index`, y si se olvida el editor se queda escuchando
/// la pista de al lado.
pub type TapKey = (usize, usize);

/// Clave que nunca existe en el registro: para abrir el editor de un plugin que
/// no está en ninguna cadena (el panel de plugins externos deja abrir cualquier
/// `.vst3` del sistema). La ventana funciona igual, sólo que sin audio.
pub const NO_SLOT: TapKey = (usize::MAX, usize::MAX);

/// Registro global de taps por slot.
///
/// Vive acá y no en `hikaru_audio_engine` porque el consumidor —el hilo de la
/// ventana del editor— vive en `hikaru_gui`, que no depende del motor. El crate
/// que define `PluginInstance` es el único que los dos comparten.
static TAPS: OnceLock<Mutex<Vec<(TapKey, Arc<PluginTap>)>>> = OnceLock::new();

fn registry() -> &'static Mutex<Vec<(TapKey, Arc<PluginTap>)>> {
    TAPS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Publica el tap de un slot recién cargado.
pub fn register(key: TapKey, tap: Arc<PluginTap>) {
    let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    reg.retain(|(k, _)| *k != key);
    reg.push((key, tap));
}

/// Devuelve el tap de un slot, si el motor tiene un plugin cargado ahí.
pub fn lookup(key: TapKey) -> Option<Arc<PluginTap>> {
    let reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    reg.iter().find(|(k, _)| *k == key).map(|(_, t)| Arc::clone(t))
}

/// Saca el tap del registro (el slot se vació o se quitó el plugin).
pub fn unregister(key: TapKey) {
    let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    reg.retain(|(k, _)| *k != key);
}

/// Anillo SPSC de bloques estéreo interleaved.
pub struct PluginTap {
    /// `capacity` bloques de `max_frames * NUM_CHANNELS` samples.
    ///
    /// `UnsafeCell` porque el productor escribe con `&self`: es lo que permite
    /// que `publish` no necesite `&mut` y el motor la pueda llamar sin quitarle
    /// el nodo de la cadena. La exclusión es la del SPSC, no la de este tipo:
    /// ningún slot se escribe mientras el consumidor lo copia, y el largo que
    /// lo gobierna (`lens` + `write`) se publica con `Release`.
    ring: UnsafeCell<Box<[f32]>>,
    /// Frames reales de cada bloque publicado. El motor no siempre entrega
    /// `max_frames` (el último bloque del dispositivo suele venir corto) y sin
    /// esto el consumidor leería el relleno viejo del slot.
    lens: Box<[AtomicU32]>,
    capacity: usize,
    max_frames: usize,
    /// Índice del próximo bloque a escribir (productor, hilo de audio).
    write: AtomicUsize,
    /// Índice del próximo bloque a leer (consumidor, hilo de la ventana).
    read: AtomicUsize,
    /// Tasa con la que se publicó el último bloque, en bits de `f32`. La lee
    /// el consumidor para configurarle la instancia de la GUI.
    sample_rate_bits: AtomicU32,
    /// Consumidores conectados. En 0, `publish` no copia nada.
    consumers: AtomicUsize,
    /// Bloques que el productor tuvo que tirar por ir el consumidor atrasado.
    dropped: AtomicU64,
}

impl PluginTap {
    /// Crea el tap del slot.
    ///
    /// `sample_rate` es la del motor: la instancia de la GUI necesita la misma
    /// o su `setupProcessing` (VST3) sella otra y el display se dibuja a una
    /// frecuencia equivocada.
    pub fn new(sample_rate: f32, max_frames: usize) -> Self {
        Self::with_capacity(sample_rate, max_frames, DEFAULT_CAPACITY)
    }

    /// Igual que [`Self::new`] pero con el número de bloques del anillo. Existe
    /// para poder ejercitar la saturación en un test sin publicar 9 bloques.
    pub fn with_capacity(sample_rate: f32, max_frames: usize, capacity: usize) -> Self {
        let max_frames = max_frames.max(1);
        let capacity = capacity.max(2);
        Self {
            ring: UnsafeCell::new(
                vec![0.0f32; capacity * max_frames * NUM_CHANNELS].into_boxed_slice(),
            ),
            lens: (0..capacity)
                .map(|_| AtomicU32::new(0))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            capacity,
            max_frames,
            write: AtomicUsize::new(0),
            read: AtomicUsize::new(0),
            sample_rate_bits: AtomicU32::new(sample_rate.to_bits()),
            consumers: AtomicUsize::new(0),
            dropped: AtomicU64::new(0),
        }
    }

    pub fn sample_rate(&self) -> f32 {
        f32::from_bits(self.sample_rate_bits.load(Ordering::Relaxed))
    }

    /// El motor cambió de tasa (cambio de dispositivo).
    ///
    /// El consumidor la lee una sola vez al arrancar su hilo, así que esto sólo
    /// importa para una ventana abierta *antes* del cambio: sin propagarlo esa
    /// ventana sigue dibujando el display con la escala de la tasa anterior.
    pub fn set_sample_rate(&self, sample_rate: f32) {
        self.sample_rate_bits
            .store(sample_rate.to_bits(), Ordering::Relaxed);
    }

    pub fn max_frames(&self) -> usize {
        self.max_frames
    }

    /// El consumidor (hilo de la ventana) dice que hay alguien escuchando.
    ///
    /// Sin esto el motor pagaría la copia de cada bloque al pedo aunque nadie
    /// tenga el editor abierto.
    pub fn attach_consumer(&self) {
        self.consumers.fetch_add(1, Ordering::AcqRel);
    }

    pub fn detach_consumer(&self) {
        self.consumers.fetch_sub(1, Ordering::AcqRel);
    }

    /// Bloques perdidos por saturación. Sólo diagnóstico.
    pub fn dropped_blocks(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Publica un bloque interleaved ya procesado. Hilo de audio.
    ///
    /// `samples` se trunca a `max_frames` frames. Si el anillo está lleno el
    /// bloque se descarta y se cuenta: nunca se pisa un slot que el consumidor
    /// pueda estar copiando.
    pub fn publish(&self, samples: &[f32]) {
        if self.consumers.load(Ordering::Relaxed) == 0 {
            return;
        }

        let frames = (samples.len() / NUM_CHANNELS).min(self.max_frames);
        if frames == 0 {
            return;
        }

        let w = self.write.load(Ordering::Relaxed);
        let r = self.read.load(Ordering::Acquire);
        if w.saturating_sub(r) >= self.capacity {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }

        let slot = w % self.capacity;
        let base = slot * self.max_frames * NUM_CHANNELS;
        let len = frames * NUM_CHANNELS;
        // SAFETY: es el único slice que este hilo escribe, y el slot `w % cap`
        // no lo está leyendo nadie: el productor se asegura antes de que
        // `w - read < capacity`, y el consumidor sólo copia slots
        // estrictamente menores que `write` (ver `take`).
        unsafe {
            let ring = &mut *self.ring.get();
            ring[base..base + len].copy_from_slice(&samples[..len]);
        }
        self.lens[slot].store(frames as u32, Ordering::Relaxed);

        // Release: el consumidor que vea este `write` con Acquire ve también
        // los samples y el largo escritos arriba.
        self.write.store(w + 1, Ordering::Release);
    }

    /// Toma el bloque **más reciente** disponible y lo deja en `out`.
    ///
    /// Hilo de la ventana. Descarta los intermedios a propósito: para un display
    /// interesa el ahora, no hacer cola y mostrar con 200 ms de atraso. Devuelve
    /// `false` si no había nada nuevo.
    pub fn take(&self, out: &mut Vec<f32>) -> bool {
        let w = self.write.load(Ordering::Acquire);
        let r = self.read.load(Ordering::Relaxed);
        if w <= r {
            return false;
        }

        // El bloque más reciente es `w - 1`, y su slot nunca puede ser el que el
        // productor está escribiendo ahora (`w % cap` es el próximo, y los
        // índices difieren en uno), así que la copia no se teariza.
        let newest = w - 1;
        let slot = newest % self.capacity;
        let frames = (self.lens[slot].load(Ordering::Relaxed) as usize).min(self.max_frames);
        let len = frames * NUM_CHANNELS;
        if len == 0 {
            self.read.store(newest + 1, Ordering::Release);
            return false;
        }

        let base = slot * self.max_frames * NUM_CHANNELS;
        out.clear();
        // SAFETY: el bloque que se copia tiene índice `w - 1`, estrictamente
        // menor que el `write` que este hilo acaba de adquirir con `Acquire`, así
        // que el productor ya terminó de escribirlo y no vuelve a tocar ese slot
        // hasta el próximo bloque (`w % cap`, que es otro).
        unsafe {
            let ring = &*self.ring.get();
            out.extend_from_slice(&ring[base..base + len]);
        }
        self.read.store(newest + 1, Ordering::Release);
        true
    }
}

// SAFETY: `ring` es `!Sync` por el `UnsafeCell`, pero el acceso está gobernado
// por la disciplina SPSC del anillo y no por el sistema de tipos: el hilo de
// audio sólo escribe el slot `write % capacity` —y se asegura antes de que
// `write - read < capacity`—, y el hilo de la ventana sólo copia slots
// estrictamente menores que `write`, ya publicados con `Release`. Nunca hay dos
// hilos en el mismo slot, así que la copia no puede tearizarse. El resto de los
// campos son atómicos, que son `Sync` por definición.
unsafe impl Sync for PluginTap {}
unsafe impl Send for PluginTap {}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(v: f32, frames: usize) -> Vec<f32> {
        vec![v; frames * NUM_CHANNELS]
    }

    #[test]
    fn sin_consumidor_no_hay_trabajo() {
        let tap = PluginTap::new(48_000.0, 8);
        tap.publish(&block(1.0, 8));
        let mut out = Vec::new();
        assert!(!tap.take(&mut out));
    }

    #[test]
    fn ida_y_vuelta_de_un_bloque() {
        let tap = PluginTap::new(48_000.0, 4);
        tap.attach_consumer();
        tap.publish(&block(0.5, 4));

        let mut out = Vec::new();
        assert!(tap.take(&mut out));
        assert_eq!(out.len(), 4 * NUM_CHANNELS);
        assert!(out.iter().all(|s| *s == 0.5));
        assert!(!tap.take(&mut out), "no debe repetir el mismo bloque");
    }

    #[test]
    fn saturar_descarta_en_vez_de_pisar() {
        let tap = PluginTap::with_capacity(48_000.0, 2, 2);
        tap.attach_consumer();

        // Dos bloques caben; el tercero se descarta.
        tap.publish(&block(1.0, 2));
        tap.publish(&block(2.0, 2));
        tap.publish(&block(3.0, 2));
        assert_eq!(tap.dropped_blocks(), 1);

        // El consumidor ve el último de los dos que entraron, no el descartado
        // ni un bloque a medio escribir.
        let mut out = Vec::new();
        assert!(tap.take(&mut out));
        assert!(out.iter().all(|s| *s == 2.0));
    }

    #[test]
    fn take_devuelve_el_mas_reciente() {
        let tap = PluginTap::new(48_000.0, 4);
        tap.attach_consumer();
        tap.publish(&block(1.0, 4));
        tap.publish(&block(2.0, 4));
        tap.publish(&block(3.0, 4));

        let mut out = Vec::new();
        assert!(tap.take(&mut out));
        assert!(out.iter().all(|s| *s == 3.0));
        // El consumidor se puso al día: los dos viejos también salen del índice.
        assert!(!tap.take(&mut out));
    }

    #[test]
    fn conserva_el_largo_real_del_bloque() {
        let tap = PluginTap::new(48_000.0, 8);
        tap.attach_consumer();
        tap.publish(&block(1.0, 3));

        let mut out = Vec::new();
        assert!(tap.take(&mut out));
        assert_eq!(out.len(), 3 * NUM_CHANNELS);
    }

    #[test]
    fn trunca_al_maximo_de_frames() {
        let tap = PluginTap::new(48_000.0, 4);
        tap.attach_consumer();
        tap.publish(&block(1.0, 64));

        let mut out = Vec::new();
        assert!(tap.take(&mut out));
        assert_eq!(out.len(), 4 * NUM_CHANNELS);
    }
}