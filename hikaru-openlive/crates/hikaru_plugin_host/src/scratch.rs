// Copyright (C) Hikaru Corporation - 2026
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/src/scratch.rs
//
// Puente entre el `AudioBuffer` interleaved de Hikaru y los buffers PLANAR que
// exigen tanto VST3 como CLAP.
//
// Por qué hace falta: `hikaru_core::AudioBuffer` es un único `&mut [f32]`
// interleaved, sin canales ni sample rate ni cantidad de frames
// (ver crates/hikaru_core/src/audio_buffer.rs). VST3 espera `*mut *mut f32`
// planar y un `ProcessContext` con sampleRate/numSamples. CLAP espera
// descriptores `ClapAudioBuffer` con punteros por canal. Ninguno de los dos
// acepta interleaved, así que hay que des-intercalar antes y re-intercalar
// después.
//
// Por qué NO se aloca en cada bloque: este código corre en el hilo de audio.
// Un `Vec::new()` por bloque es una llamada al allocator general, que puede
// tomar un lock interno del malloc. Si el hilo de audio se bloquea ahí, el
// deadline del quantum de PipeWire se pasa y el stream xrunea. Por eso
// [`PlanarScratch`] reserva todo una vez en [`PlanarScratch::new`] y después
// `process` sólo hace memcpy de f32, que es la única operación RT-segura que
// necesitamos.
//
// El scratch es persistente por *instancia de plugin*, no global ni
// compartilhado: un `&mut self` exclusivo ya garantiza que no haya aliasing,
// así que no hace falta ni `Mutex` ni atomics para mantenerlo consistente.

/// Frames máximos que pre-reservamos. Cubre con holgura el quantum típico de
/// PipeWire (el más grande por defecto son 8192, pero ningún host de plugins
/// usa bloques así; 4096 deja margen de sobra y son 32 KiB por canal).
pub const MAX_BLOCK_FRAMES: usize = 4096;

/// Número de canales que manejamos. El motor de Hikaru es estéreo
/// (`let num_channels = 2` en hikaru_audio_engine/src/lib.rs:1377).
pub const NUM_CHANNELS: usize = 2;

/// Buffers planares persistentes para adaptar el audio del host a un plugin.
///
/// Los dos conjuntos de canales (`inputs` y `outputs`) están pre-asignados por
/// [`new`]. [`deinterleave_stereo`] rellena `inputs` desde el buffer del host;
/// tras procesar, [`reinterleave_stereo`] copia `outputs` de vuelta al buffer
/// del host.
pub struct PlanarScratch {
    inputs: Vec<Vec<f32>>,
    outputs: Vec<Vec<f32>>,
    /// Capacidad en frames de cada canal.
    frames: usize,
    /// Se activa si alguna vez se pidió un bloque más grande que la
    /// capacidad. Sirviendo de bandera observable desde el hilo de control sin
    /// necesidad de loguear: loguear desde el hilo de audio es mala práctica,
    /// porque `println!` toma el lock de stderr.
    overflowed: bool,
}

impl PlanarScratch {
    /// Reserva los buffers. `frames` es el máximo de samples por bloque que
    /// vamos a tolerar; no se vuelve a alocar después.
    pub fn new(frames: usize) -> Self {
        let frames = frames.clamp(1, MAX_BLOCK_FRAMES);
        Self {
            inputs: (0..NUM_CHANNELS).map(|_| vec![0.0f32; frames]).collect(),
            outputs: (0..NUM_CHANNELS).map(|_| vec![0.0f32; frames]).collect(),
            frames,
            overflowed: false,
        }
    }

    /// Número de frames que el scratch puede sostener.
    pub fn capacity(&self) -> usize {
        self.frames
    }

    /// ¿Hubo alguna vez un bloque más grande que la capacidad?
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// Canales de entrada, listos para handing a `process_audio` (VST3) o
    /// para tomar sus punteros (CLAP).
    pub fn inputs(&mut self) -> &mut [Vec<f32>] {
        &mut self.inputs
    }

    /// Canales de salida, listos para que el plugin los escriba.
    pub fn outputs(&mut self) -> &mut [Vec<f32>] {
        &mut self.outputs
    }

    /// ¿Este bloque cabe en la capacidad? Si no, hay que saltarlo.
    pub fn fits(&mut self, frames: usize) -> bool {
        if frames > self.frames {
            self.overflowed = true;
            return false;
        }
        true
    }

    /// Saca los buffers fuera del scratch para envolverlos en el tipo que
    /// pida el plugin (`AudioBuffers` en VST3, descriptores en CLAP).
    ///
    /// Es una **movida**, no una copia: los `Vec` que ya estaban pre-asignados
    /// cambian de dueño sin tocar el allocator, así que sigue siendo RT-seguro.
    /// Hay que devolverlos con [`PlanarScratch::restore`] para el siguiente
    /// bloque.
    pub fn take_buffers(&mut self) -> (Vec<Vec<f32>>, Vec<Vec<f32>>) {
        (
            std::mem::take(&mut self.inputs),
            std::mem::take(&mut self.outputs),
        )
    }

    /// Devuelve los buffers al scratch tras procesarlos.
    pub fn restore_buffers(&mut self, inputs: Vec<Vec<f32>>, outputs: Vec<Vec<f32>>) {
        self.inputs = inputs;
        self.outputs = outputs;
    }

    /// Acceso simultáneo a los canales de entrada y de salida.
    ///
    /// Hace falta porque los dos getters por separado cada uno pedirían
    /// `&mut self`, y no pueden convivir. Devolverlos juntos en una tupla hace
    /// el borrow explícito y el compilador verifica que no se solapen.
    pub fn channels_mut(&mut self) -> (&mut [Vec<f32>], &mut [Vec<f32>]) {
        (&mut self.inputs, &mut self.outputs)
    }

    /// Copia el audio interleaved del host a los canales planares de entrada.
    ///
    /// Asume layout interleaved estéreo, que es como el motor indexa
    /// (`samples[i*2]` / `samples[i*2+1]`, ver hikaru_audio_engine/src/lib.rs:1645).
    /// `frames` debe ser el número de frames (samples / 2), no de samples.
    pub fn deinterleave_stereo(&mut self, samples: &[f32], frames: usize) {
        debug_assert!(frames <= self.frames);
        let len = frames.min(samples.len() / NUM_CHANNELS);

        for (ch, buffer) in self.inputs.iter_mut().enumerate() {
            for i in 0..len {
                // Bucle plano con índice constante por canal: el compilador lo
                // vectoriza a un strided-copy de f32, que no asigna ni bloquea.
                buffer[i] = samples[i * NUM_CHANNELS + ch];
            }
            // Si el host mandó un buffer impar o más corto de lo declarado, el
            // resto del canal queda en silencio en vez de con datos viejos.
            for sample in buffer[len..].iter_mut() {
                *sample = 0.0;
            }
        }
    }

    /// Copia los canales de salida del plugin de vuelta al buffer del host.
    ///
    /// Es el camino inverso: el plugin escribió en `outputs`, el host necesita
    /// su buffer interleaved original modificado.
    pub fn reinterleave_stereo(&self, samples: &mut [f32], frames: usize) {
        let len = frames.min(samples.len() / NUM_CHANNELS);
        if len == 0 {
            return;
        }

        for i in 0..len {
            samples[i * NUM_CHANNELS] = self.outputs[0][i];
            samples[i * NUM_CHANNELS + 1] = self.outputs[1][i];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_interleaved_is_lossless() {
        let mut scratch = PlanarScratch::new(64);
        let mut samples: Vec<f32> = (0..64).map(|i| i as f32).collect();

        scratch.deinterleave_stereo(&samples, 32);
        // Canal L = índices pares del original.
        assert_eq!(scratch.inputs()[0][0], 0.0);
        assert_eq!(scratch.inputs()[0][1], 2.0);
        assert_eq!(scratch.inputs()[1][0], 1.0);
        assert_eq!(scratch.inputs()[1][1], 3.0);

        // El plugin "copia" entrada a salida (sólo los 32 frames del bloque; el resto
        // del canal queda allocated pero no se usa).
        for ch in 0..NUM_CHANNELS {
            let src = scratch.inputs()[ch][..32].to_vec();
            scratch.outputs()[ch][..32].copy_from_slice(&src);
        }
        scratch.reinterleave_stereo(&mut samples, 32);

        assert_eq!(samples, (0..64).map(|i| i as f32).collect::<Vec<_>>());
    }

    #[test]
    fn oversized_block_is_refused_not_overflowed() {
        let mut scratch = PlanarScratch::new(16);
        assert!(!scratch.fits(17));
        assert!(scratch.overflowed(), "la bandera debería quedar activa");
        assert!(scratch.fits(16));
    }

    #[test]
    fn deinterleave_zero_fills_tail() {
        // Si el host dice 0 frames pero el buffer tiene datos viejos, no deben
        // quedar residues del bloque anterior (harían "clic" en la salida).
        let mut scratch = PlanarScratch::new(8);
        scratch.deinterleave_stereo(&[1.0, 1.0, 1.0, 1.0], 2);
        scratch.deinterleave_stereo(&[0.0, 0.0, 0.0, 0.0], 0);
        assert_eq!(scratch.inputs()[0][3], 0.0);
    }
}