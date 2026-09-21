// crates/hikaru_audio_drivers/src/lib.rs
// LGPLv3 (GNU Lesser General Public License v3)
// Copyright (C) Hikaru Corporation - 2026

use alsa_sys::*;
use std::ptr;

pub struct HikaruAlsaDriver {
    pcm_handle: *mut snd_pcm_t,
}

/*

// Esto:

#[derive(Debug, Clone, Copy)]
pub struct MidiEvent {
    pub frame_offset: usize, // En qué frame relativo al buffer actual ocurre
    pub channel: u8,
    pub note: u8,
    pub velocity: u8,
    pub is_note_on: bool,
}
    // Debería estár en el `lib.rs` del crate `hikaru_audio_engine`, no acá KJJ,
*/

impl HikaruAlsaDriver {
    pub fn open_direct_device(device_name: &str, sample_rate: u32, buffer_size: u32) -> Result<Self, String> {
        let mut pcm_handle: *mut snd_pcm_t = ptr::null_mut();
        let c_dev = std::ffi::CString::new(device_name).unwrap();

        unsafe {
            // Abrimos el dispositivo en modo Playback directo[cite: 4]
            let err = snd_pcm_open(
                &mut pcm_handle,
                c_dev.as_ptr(),
                SND_PCM_STREAM_PLAYBACK,
                0, // Cambiamos a blocking para hilo audio dedicado, o mantenemos SND_PCM_NONBLOCK[cite: 4]
            );

            if err < 0 {
                return Err(format!("Error abriendo ALSA directo: {}", err));
            }

            // Configuración de Hardware Params
            let mut hw_params: *mut snd_pcm_hw_params_t = ptr::null_mut();
            snd_pcm_hw_params_malloc(&mut hw_params);
            snd_pcm_hw_params_any(pcm_handle, hw_params);

            // Access format: Interleaved (estándar para buffers estéreo PCM)
            snd_pcm_hw_params_set_access(pcm_handle, hw_params, SND_PCM_ACCESS_RW_INTERLEAVED);
            // Formato de muestras: 32-bit Float o 16-bit
            snd_pcm_hw_params_set_format(pcm_handle, hw_params, SND_PCM_FORMAT_S16_LE);
            
            // Frecuencia de muestreo (Ej: 44100 Hz)
            let mut rate = sample_rate;
            snd_pcm_hw_params_set_rate_near(pcm_handle, hw_params, &mut rate, ptr::null_mut());

            // Tamaño del Buffer (Frames)
            let mut frames = buffer_size as u64;
            snd_pcm_hw_params_set_buffer_size_near(pcm_handle, hw_params, &mut frames);

            // Aplicamos parámetros al PCM
            snd_pcm_hw_params(pcm_handle, hw_params);
            snd_pcm_hw_params_free(hw_params);

            // Preparamos la placa para reproducir
            snd_pcm_prepare(pcm_handle);
        }

        Ok(Self { pcm_handle })
    }

    pub fn write_samples(&self, buffer: &[i16]) -> i64 {
        unsafe {
            let frames = (buffer.len() / 2) as u64;
            snd_pcm_writei(self.pcm_handle, buffer.as_ptr() as *const _, frames)
        }
    }
}

impl Drop for HikaruAlsaDriver {
    fn drop(&mut self) {
        unsafe {
            if !self.pcm_handle.is_null() {
                snd_pcm_drain(self.pcm_handle);
                snd_pcm_close(self.pcm_handle);
            }
        }
    }
}