// crates/hikaru_core/src/audio_buffer.rs

use crate::AudioSample; // Ahora sí lo encuentra en el padre

/// Estructura de buffer de audio optimizada.
/// Usamos un lifetime 'a para indicar que este buffer referencia datos que viven en otro lado.
pub struct AudioBuffer<'a> {
    pub samples: &'a mut [AudioSample],
}

impl<'a> AudioBuffer<'a> {
    pub fn new(samples: &'a mut [AudioSample]) -> Self {
        Self { samples }
    }

    pub fn get_samples_mut(&mut self) -> &mut [AudioSample] {
        self.samples
    }
}

/// Basado en tus errores, parece que tenés algo así para Stereo:
pub struct StereoBuffer<'a> {
    pub left: AudioBuffer<'a>,  // <--- Ahora sí acepta el <'a>
    pub right: AudioBuffer<'a>,
}

impl<'a> StereoBuffer<'a> {
    pub fn left(&mut self) -> &mut AudioBuffer<'a> {
        &mut self.left
    }

    pub fn right(&mut self) -> &mut AudioBuffer<'a> {
        &mut self.right
    }
}