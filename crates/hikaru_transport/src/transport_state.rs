// crates/hikaru_transport/src/transport_state.rs

use hikaru_core::SampleRate;

/// Resolución única del secuenciador: ticks por negra (quarter note).
/// ÚNICA fuente de verdad del PPQN. La GUI (playlist/app) NO debe
/// hardcodear 960: debe usar `DEFAULT_PPQN` / `transport.ppqn()`.
pub const DEFAULT_PPQN: u64 = 960;

/// Posibles estados de reproducción.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransportPlaybackState {
    Stopped,
    Playing,
    Paused,
}

/// Estado global del transporte de audio.
#[derive(Debug, Clone, Copy)]
pub struct TransportPosition {
    pub sample_rate: SampleRate,
    pub bpm: f64,
    pub beats_per_bar: u32,
    pub beat_division: u32,
    pub sample_count: u64, // Nombre unificado
    pub playback_state: TransportPlaybackState,
    /// Loop global del transporte (región en SAMPLES, no en ticks).
    /// La GUI configura esta región vía `set_loop_region_samples` con los
    /// mismos ticks que muestra la barra (`ticks_to_samples`), y el motor
    /// (`hikaru_audio_engine::AudioEngine::process`) hace el wrap en el
    /// callback de audio. Dueño único del wrap: el engine. La GUI nunca
    /// reescribe `sample_count` por su cuenta para loopear.
    pub loop_enabled: bool,
    pub loop_start_samples: u64,
    pub loop_end_samples: u64,
}

impl TransportPosition {
    /// Crea un nuevo estado de transporte.
    pub fn new(sample_rate: SampleRate, bpm: f64) -> Self {
        Self {
            sample_rate,
            bpm,
            beats_per_bar: 4,
            beat_division: 4,
            sample_count: 0,
            playback_state: TransportPlaybackState::Stopped,
            loop_enabled: false,
            loop_start_samples: 0,
            loop_end_samples: 0,
        }
    }

    /// Actualiza la posición sumando el número de samples procesados.
    pub fn advance(&mut self, samples: u64) {
        if self.playback_state == TransportPlaybackState::Playing {
            self.sample_count += samples;
        }
    }

    /// Reinicia la posición a cero.
    pub fn reset(&mut self) {
        self.sample_count = 0;
    }

    /// Cambia el BPM de forma segura.
    pub fn set_bpm(&mut self, new_bpm: f64) {
        if new_bpm > 0.0 {
            self.bpm = new_bpm;
        }
    }

    /// PPQN activo del transporte (constante única `DEFAULT_PPQN`).
    /// La GUI debe llamar a esto en lugar de hardcodear 960.
    pub fn ppqn(&self) -> u64 {
        DEFAULT_PPQN
    }

    /// Calcula la duración de un compás en samples (f64 para precisión).
    pub fn samples_per_bar(&self) -> f64 {
        self.samples_per_beat() * self.beats_per_bar as f64
    }

    /// Samples por negra con el BPM activo y el Sample Rate real.
    pub fn samples_per_beat(&self) -> f64 {
        let bpm = self.bpm.max(1.0);
        let sr = self.sample_rate.get() as f64;
        if sr <= 0.0 {
            return 0.0;
        }
        (sr * 60.0) / bpm
    }

    /// Segundos por tick con el BPM activo (guarda bpm <= 0).
    pub fn seconds_per_tick(&self) -> f64 {
        let bpm = self.bpm.max(1.0);
        (60.0 / bpm) / self.ppqn() as f64
    }

    /// Ticks -> samples con BPM activo + SR real + PPQN único.
    pub fn ticks_to_samples(&self, ticks: u64) -> u64 {
        let sr = self.sample_rate.get() as f64;
        if sr <= 0.0 {
            return 0;
        }
        (ticks as f64 * self.seconds_per_tick() * sr).round() as u64
    }

    /// Longitud del clip en frames con el tempo REAL del transporte.
    ///
    /// FUGA #2 (135 BPM vs 120 BPM): los compases visuales del Clip Editor
    /// (en ticks) solo coinciden con la parada real del engine si los frames
    /// se derivan de `ticks_to_samples` con el BPM activo del transporte.
    /// Calcular con otro tempo o con un `samples_per_bar` rancio desplaza la
    /// parada en miles de samples. La GUI debe usar esto (vía
    /// `transport.clip_length_frames(duration_ticks)`) y propagar el BPM con
    /// `SetBpm` ANTES de sincronizar clips.
    #[inline]
    pub fn clip_length_frames(&self, duration_ticks: u64) -> u64 {
        self.ticks_to_samples(duration_ticks)
    }

    /// Samples -> ticks con BPM activo + SR real + PPQN único.
    /// Inversa exacta de `ticks_to_samples` (salvo redondeo).
    pub fn samples_to_ticks(&self, samples: u64) -> u64 {
        let sr = self.sample_rate.get() as f64;
        if sr <= 0.0 {
            return 0;
        }
        let seconds = samples as f64 / sr;
        let spt = self.seconds_per_tick();
        if spt <= 0.0 {
            return 0;
        }
        (seconds / spt).round() as u64
    }
    
    /// Define la región de loop global en samples.
    /// `start`/`end` deben venir de `ticks_to_samples` con los mismos ticks
    /// que muestra la barra del transporte de la GUI, así ambos coinciden
    /// exactamente. Si `end <= start` la región queda inválida (el wrap es
    /// identidad hasta que se configure una región válida).
    pub fn set_loop_region_samples(&mut self, start: u64, end: u64) {
        self.loop_start_samples = start;
        self.loop_end_samples = end;
    }

    /// Activa/desactiva el loop global del transporte.
    pub fn set_loop_enabled(&mut self, enabled: bool) {
        self.loop_enabled = enabled;
    }

    /// Indica si hay una región de loop global válida para el wrap.
    pub fn has_valid_loop(&self) -> bool {
        self.loop_enabled && self.loop_end_samples > self.loop_start_samples
    }

    /// Longitud del loop global en samples (0 si es inválido).
    pub fn loop_length_samples(&self) -> u64 {
        self.loop_end_samples.saturating_sub(self.loop_start_samples)
    }

    /// Mapea una posición absoluta de samples a su posición dentro del loop
    /// global (`[loop_start, loop_end)`). Si el loop no está habilitado o la
    /// región es inválida, devuelve `pos` sin cambios (identidad).
    /// Función pura: el engine la usa por frame en el callback de audio y la
    /// GUI la usa para derivar el playhead, así ambos ven el mismo reloj.
    pub fn wrap_sample_count(&self, pos: u64) -> u64 {
        if !self.has_valid_loop() || pos < self.loop_end_samples {
            return pos;
        }
        let len = self.loop_length_samples();
        if len == 0 {
            return pos;
        }
        self.loop_start_samples + ((pos - self.loop_start_samples) % len)
    }
    
    // Getter para BPM si lo necesitás como f64
    pub fn get_bpm(&self) -> f64 {
        self.bpm
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_transport() -> TransportPosition {
        TransportPosition::new(SampleRate::new(44100.0), 120.0)
    }

    #[test]
    fn loop_disabled_is_identity() {
        let t = test_transport();
        assert!(!t.has_valid_loop());
        assert_eq!(t.wrap_sample_count(1_000_000), 1_000_000);
    }

    #[test]
    fn loop_wraps_into_region() {
        let mut t = test_transport();
        t.set_loop_region_samples(1000, 5000);
        t.set_loop_enabled(true);
        assert!(t.has_valid_loop());
        assert_eq!(t.loop_length_samples(), 4000);
        // Antes del fin: identidad.
        assert_eq!(t.wrap_sample_count(4999), 4999);
        // En el fin: vuelve al inicio; más allá: módulo.
        assert_eq!(t.wrap_sample_count(5000), 1000);
        assert_eq!(t.wrap_sample_count(9000), 1000);
        assert_eq!(t.wrap_sample_count(9500), 1500);
    }

    #[test]
    fn invalid_region_is_identity() {
        let mut t = test_transport();
        t.set_loop_region_samples(5000, 5000);
        t.set_loop_enabled(true);
        assert!(!t.has_valid_loop());
        assert_eq!(t.wrap_sample_count(6000), 6000);
    }
}