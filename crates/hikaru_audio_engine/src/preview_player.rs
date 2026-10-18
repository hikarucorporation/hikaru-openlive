// crates/hikaru_audio_engine/src/preview_player.rs
// Reproductor de preview lock-free para el callback de CPAL.
//
// Arquitectura:
//   - `PreviewBuffer` se comparte vía `Arc` entre el command handler (writer)
//     y el callback de CPAL (reader).
//   - El callback avanza un `AtomicUsize` cursor y lee del buffer — cero allocs.
//   - Los campos no atómicos (`data`, `channels`, `length`) se escriben bajo
//     el engine Mutex y se leen bajo el engine Mutex (via `try_lock`), así que
//     `UnsafeCell` es seguro aquí (misma garantía que `Cell` pero sin `Copy`).
//   - Un `generation: AtomicU64` previene que un decode viejo sobreescriba uno nuevo.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;

/// Buffer de preview compartido entre command handler y CPAL callback.
///
/// **Writer** (command handler / decode thread): llama `start()` o `stop()`
/// mientras sostiene el engine Mutex.
///
/// **Reader** (CPAL callback): avanza `cursor` con `fetch_add` y lee samples.
/// El engine Mutex es non-blocking (`try_lock`); si está ocupado → silencio.
pub struct PreviewBuffer {
    /// Samples interleaved (mono o stereo), escritos bajo engine Mutex.
    data: UnsafeCell<Vec<f32>>,
    /// Cantidad de canales (1 = mono, 2 = stereo).
    channels: UnsafeCell<usize>,
    /// Cantidad válida de samples en `data`.
    length: UnsafeCell<usize>,
    /// Cursor de lectura — avanzado por el CPAL callback con `fetch_add`.
    pub cursor: AtomicUsize,
    /// Ganancia lineal [0.0, 1.0] — almacenada como bits de f32.
    pub volume_bits: AtomicU32,
    /// Flag de reproducción activa.
    pub is_playing: AtomicBool,
    /// Generación: previene que decodes viejos sobreescriban nuevos.
    pub generation: AtomicU64,
}

// Safe: `data`/`channels`/`length` solo se escriben bajo engine Mutex
// y se leen bajo engine Mutex (try_lock en CPAL callback).
unsafe impl Send for PreviewBuffer {}
unsafe impl Sync for PreviewBuffer {}

impl PreviewBuffer {
    fn new() -> Self {
        Self {
            data: UnsafeCell::new(Vec::new()),
            channels: UnsafeCell::new(1),
            length: UnsafeCell::new(0),
            cursor: AtomicUsize::new(0),
            volume_bits: AtomicU32::new(0.8f32.to_bits()),
            is_playing: AtomicBool::new(false),
            generation: AtomicU64::new(0),
        }
    }

    /// Activa la reproducción con datos pre-decoded.
    /// Llamado bajo engine Mutex — seguro para escribir UnsafeCell.
    pub fn start(&self, samples: Vec<f32>, channels: usize, generation: u64) {
        let len = samples.len();
        unsafe {
            *self.data.get() = samples;
            *self.channels.get() = channels.max(1);
            *self.length.get() = len;
        }
        self.cursor.store(0, Ordering::Relaxed);
        self.generation.store(generation, Ordering::Relaxed);
        // Release: data/channels/length visibles antes de is_playing = true.
        self.is_playing.store(true, Ordering::Release);
    }

    /// Detiene la reproducción.
    pub fn stop(&self) {
        self.is_playing.store(false, Ordering::Release);
        self.cursor.store(0, Ordering::Relaxed);
    }

    /// Mezcla el preview en el buffer de CPAL (interleaved stereo).
    ///
    /// - **Stereo** (channels=2): copia pares [L,R] directo.
    /// - **Mono** (channels=1): duplica cada sample a L y R.
    /// - Aplica volumen y clamping. Mezcla aditiva.
    ///
    /// # Safety
    /// Los campos `data`/`channels`/`length` solo se leen aquí. Se garantiza
    /// que fueron escritos bajo engine Mutex antes de `is_playing = true`
    /// (Release/Acquire pair). El CPAL callback lee con Acquire en `is_playing`.
    pub fn process(&self, output: &mut [f32]) {
        if !self.is_playing.load(Ordering::Acquire) {
            return;
        }

        let data = unsafe { &*self.data.get() };
        let channels = unsafe { *self.channels.get() };
        let length = unsafe { *self.length.get() };
        let vol = f32::from_bits(self.volume_bits.load(Ordering::Relaxed));
        let active_len = length.min(data.len());

        if channels == 2 {
            let mut i = 0;
            while i + 1 < output.len() {
                let pos = self.cursor.fetch_add(2, Ordering::Relaxed);
                if pos + 1 < active_len {
                    let l = (data[pos] * vol).clamp(-1.0, 1.0);
                    let r = (data[pos + 1] * vol).clamp(-1.0, 1.0);
                    output[i] += l;
                    output[i + 1] += r;
                    i += 2;
                } else {
                    self.is_playing.store(false, Ordering::Release);
                    break;
                }
            }
        } else {
            let mut i = 0;
            while i + 1 < output.len() {
                let pos = self.cursor.fetch_add(1, Ordering::Relaxed);
                if pos < active_len {
                    let s = (data[pos] * vol).clamp(-1.0, 1.0);
                    output[i] += s;
                    output[i + 1] += s;
                    i += 2;
                } else {
                    self.is_playing.store(false, Ordering::Release);
                    break;
                }
            }
        }

        // Post-loop: si el buffer de salida se llenó exactamente con todos
        // los samples, el loop termina sin entrar al else. Verificamos acá.
        if self.is_playing.load(Ordering::Relaxed) {
            let pos = self.cursor.load(Ordering::Relaxed);
            if pos >= active_len {
                self.is_playing.store(false, Ordering::Release);
            }
        }
    }
}

/// Reproductor de preview — API del engine.
///
/// Contiene un `Arc<PreviewBuffer>` que el CPAL callback lee directamente.
/// `play()` y `stop()` se llaman bajo engine Mutex.
pub struct PreviewPlayer {
    /// Buffer activo compartido con el CPAL callback.
    shared: Arc<PreviewBuffer>,
    /// Generación actual — para detectar decodes obsoletos.
    generation: u64,
    /// Volumen local (copiado al buffer atómico en `set_volume`).
    volume: f32,
}

impl PreviewPlayer {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(PreviewBuffer::new()),
            generation: 0,
            volume: 0.8,
        }
    }

    /// Devuelve una referencia compartida al buffer para el CPAL callback.
    pub fn shared_buffer(&self) -> Arc<PreviewBuffer> {
        self.shared.clone()
    }

    /// Returns true if a preview is currently playing.
    /// Used to skip the Arc::clone in process() when no preview is active.
    pub fn is_active(&self) -> bool {
        self.shared.is_playing.load(Ordering::Relaxed)
    }

    pub fn set_volume(&mut self, vol: f32) {
        self.volume = vol.clamp(0.0, 1.0);
        self.shared
            .volume_bits
            .store(self.volume.to_bits(), Ordering::Relaxed);
    }

    /// Carga samples pre-decoded y comienza la reproducción.
    /// Llamado bajo engine Mutex (command handler o decode thread).
    pub fn play(&mut self, samples: Vec<f32>, channels: usize) {
        self.generation = self.generation.wrapping_add(1);
        self.shared.start(samples, channels, self.generation);
    }

    /// Detiene la reproducción.
    pub fn stop(&mut self) {
        self.shared.stop();
    }

    #[inline]
    pub fn is_playing(&self) -> bool {
        self.shared.is_playing.load(Ordering::Acquire)
    }
}

// ============================================================================
// Tests
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_volume_clamps() {
        let mut p = PreviewPlayer::new();
        p.set_volume(0.0);
        assert_eq!(p.volume, 0.0);
        p.set_volume(1.5);
        assert_eq!(p.volume, 1.0);
    }

    #[test]
    fn test_stop_resets() {
        let mut p = PreviewPlayer::new();
        p.play(vec![0.5; 100], 1);
        p.stop();
        assert!(!p.is_playing());
        assert_eq!(p.shared.cursor.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_mono_play_and_process() {
        let mut p = PreviewPlayer::new();
        p.play(vec![0.5; 4], 1);
        p.set_volume(1.0);

        let buf = p.shared_buffer();
        assert!(buf.is_playing.load(Ordering::Acquire));

        let mut out = vec![0.0; 8];
        buf.process(&mut out);

        assert!(!buf.is_playing.load(Ordering::Acquire));
        assert_eq!(buf.cursor.load(Ordering::Relaxed), 4);
        for frame in 0..4 {
            assert_eq!(out[frame * 2], 0.5);
            assert_eq!(out[frame * 2 + 1], 0.5);
        }
    }

    #[test]
    fn test_stereo_play_and_process() {
        let mut p = PreviewPlayer::new();
        p.play(vec![0.3, 0.7, 0.1, 0.9], 2);
        p.set_volume(1.0);

        let buf = p.shared_buffer();
        let mut out = vec![0.0; 4];
        buf.process(&mut out);

        assert_eq!(out[0], 0.3);
        assert_eq!(out[1], 0.7);
        assert_eq!(out[2], 0.1);
        assert_eq!(out[3], 0.9);
    }

    #[test]
    fn test_stereo_not_half_speed() {
        let mut p = PreviewPlayer::new();
        p.play(vec![1.0, -1.0, 0.5, -0.5], 2);
        p.set_volume(1.0);

        let buf = p.shared_buffer();
        let mut out = vec![0.0; 4];
        buf.process(&mut out);

        assert_eq!(buf.cursor.load(Ordering::Relaxed), 4);
        assert_eq!(out[0], 1.0);
        assert_eq!(out[1], -1.0);
        assert_eq!(out[2], 0.5);
        assert_eq!(out[3], -0.5);
    }

    #[test]
    fn test_clamping() {
        let mut p = PreviewPlayer::new();
        p.play(vec![2.0, -2.0], 2);
        p.set_volume(1.0);

        let buf = p.shared_buffer();
        let mut out = vec![0.0; 2];
        buf.process(&mut out);

        assert_eq!(out[0], 1.0);
        assert_eq!(out[1], -1.0);
    }

    #[test]
    fn test_volume_applied() {
        let mut p = PreviewPlayer::new();
        p.play(vec![0.5, 0.5], 2);
        p.set_volume(0.5);

        let buf = p.shared_buffer();
        let mut out = vec![0.0; 2];
        buf.process(&mut out);

        assert_eq!(out[0], 0.25);
        assert_eq!(out[1], 0.25);
    }

    #[test]
    fn test_additive_mix() {
        let mut p = PreviewPlayer::new();
        p.play(vec![0.5, 0.5], 2);
        p.set_volume(1.0);

        let buf = p.shared_buffer();
        let mut out = vec![0.3, 0.3];
        buf.process(&mut out);

        assert_eq!(out[0], 0.8);
        assert_eq!(out[1], 0.8);
    }

    #[test]
    fn test_mute_at_zero_volume() {
        let mut p = PreviewPlayer::new();
        p.play(vec![1.0; 4], 1);
        p.set_volume(0.0);

        let buf = p.shared_buffer();
        let mut out = vec![0.5; 4];
        buf.process(&mut out);

        assert!(out.iter().all(|&s| s == 0.5));
    }

    #[test]
    fn test_generation_increments() {
        let mut p = PreviewPlayer::new();
        let gen0 = p.generation;
        p.play(vec![0.5; 10], 1);
        let gen1 = p.generation;
        assert!(gen1 > gen0);
        p.play(vec![0.5; 20], 1);
        let gen2 = p.generation;
        assert!(gen2 > gen1);
    }

    #[test]
    fn test_partial_process() {
        let mut p = PreviewPlayer::new();
        p.play(vec![0.5; 10], 2); // 10 interleaved = 5 stereo frames
        p.set_volume(1.0);

        let buf = p.shared_buffer();
        // Output only room for 2 frames (4 samples)
        let mut out = vec![0.0; 4];
        buf.process(&mut out);

        assert!(buf.is_playing.load(Ordering::Acquire)); // still has data
        assert_eq!(buf.cursor.load(Ordering::Relaxed), 4);

        // Process remaining
        let mut out2 = vec![0.0; 20];
        buf.process(&mut out2);

        assert!(!buf.is_playing.load(Ordering::Acquire)); // finished
    }

    #[test]
    fn test_new_preview_replaces_old() {
        let mut p = PreviewPlayer::new();
        p.set_volume(1.0);

        // Play first sample
        p.play(vec![0.1; 4], 2);
        let buf = p.shared_buffer();

        // Process half
        let mut out = vec![0.0; 4];
        buf.process(&mut out);

        // Load new sample (simulates rapid click)
        p.play(vec![0.9; 4], 2);

        // Should hear new sample, not old
        let mut out2 = vec![0.0; 4];
        buf.process(&mut out2);

        assert_eq!(out2[0], 0.9);
        assert_eq!(out2[1], 0.9);
    }
}
