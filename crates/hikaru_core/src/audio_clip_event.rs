// crates/hikaru_core/src/audio_clip_event.rs
//
// Modelo unificado de evento de audio para clips de audio (Audio Clip Events).
// Paridad arquitectónica con Bitwig Studio 5.1.7.

use std::sync::Arc;

/// Modos de estiramiento temporal y pitch del evento de audio.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StretchMode {
    #[default]
    Raw,
    Hd,
    Smooth,
    Beats,
}

impl StretchMode {
    pub fn label(&self) -> &'static str {
        match self {
            StretchMode::Raw => "RAW",
            StretchMode::Hd => "HD",
            StretchMode::Smooth => "Smooth",
            StretchMode::Beats => "Beats",
        }
    }
}

/// Representa un evento de audio dentro de un clip de audio.
///
/// Soporta referencia compartida al buffer PCM decodificado, delimitadores
/// absolutos de frame, puntos de bucle en muestras/ticks, nivel de ganancia
/// en decibelios y envolvente de fundidos.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioClipEvent {
    pub id: u64,
    pub name: String,
    /// Puntero o referencia compartida al buffer de audio PCM cargado.
    pub sample_buffer: Arc<Vec<f32>>,
    pub channels: usize,
    pub sample_rate: u32,
    /// Delimitador absoluto inicial dentro del archivo/buffer (frames).
    pub start_frame: u64,
    /// Delimitador absoluto final dentro del archivo/buffer (frames).
    pub end_frame: u64,
    /// Punto de inicio de bucle expresado en muestras.
    pub loop_start: u64,
    /// Punto de fin de bucle expresado en muestras.
    pub loop_end: u64,
    /// Si el bucle está habilitado para este evento.
    pub loop_enabled: bool,
    /// Configuración de volumen expresada en decibelios (dB). 0.0 dB = ganancia unitaria.
    pub gain_db: f32,
    /// Duración del fundido de entrada (Fade In) en segundos.
    pub fade_in_secs: f64,
    /// Duración del fundido de salida (Fade Out) en segundos.
    pub fade_out_secs: f64,
    /// Modo de algoritmo de stretch.
    pub stretch_mode: StretchMode,
}

impl AudioClipEvent {
    pub fn new(
        id: u64,
        name: impl Into<String>,
        sample_buffer: Arc<Vec<f32>>,
        channels: usize,
        sample_rate: u32,
    ) -> Self {
        let ch = channels.max(1);
        let total_frames = (sample_buffer.len() / ch) as u64;
        Self {
            id,
            name: name.into(),
            sample_buffer,
            channels: ch,
            sample_rate: sample_rate.max(1),
            start_frame: 0,
            end_frame: total_frames,
            loop_start: 0,
            loop_end: total_frames,
            loop_enabled: false,
            gain_db: 0.0,
            fade_in_secs: 0.0,
            fade_out_secs: 0.0,
            stretch_mode: StretchMode::Raw,
        }
    }

    /// Duración total visible en frames.
    #[inline]
    pub fn visible_frames(&self) -> u64 {
        self.end_frame.saturating_sub(self.start_frame)
    }

    /// Duración en segundos calculada a partir de los frames visibles y sample rate.
    #[inline]
    pub fn duration_secs(&self) -> f64 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.visible_frames() as f64 / self.sample_rate as f64
        }
    }

    /// Ganancia lineal a partir de `gain_db`.
    #[inline]
    pub fn linear_gain(&self) -> f32 {
        10.0f32.powf(self.gain_db / 20.0)
    }

    /// Asigna la ganancia a partir de un valor lineal.
    pub fn set_linear_gain(&mut self, linear: f32) {
        if linear <= 0.00001 {
            self.gain_db = -60.0;
        } else {
            self.gain_db = 20.0 * linear.log10();
        }
    }
}

