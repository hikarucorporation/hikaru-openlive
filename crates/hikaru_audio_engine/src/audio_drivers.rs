/*
 * Hikaru OpenLive - Audio Driver Host (Linux)
 * License: AGPL-3.0-or-later
 * Path: crates/hikaru_audio_engine/src/audio_drivers.rs
 */

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Host, Stream, StreamConfig};
use std::sync::{Arc, Mutex};
use hikaru_core::AudioBuffer;
use crate::AudioEngine;

pub enum DriverType {
    PipeWire,
    Jack,
    Alsa,
    PulseAudio,
}

pub struct AudioDriverHost {
    host: Host,
    device: Option<Device>,
    stream: Option<Stream>,
}

impl AudioDriverHost {
    pub fn new() -> Self {
        let host = cpal::default_host();
        Self {
            host,
            device: None,
            stream: None,
        }
    }

    /// Obtiene la lista de dispositivos de salida disponibles
    pub fn list_output_devices(&self) -> Vec<String> {
        let mut devices = Vec::new();
        if let Ok(devs) = self.host.output_devices() {
            for dev in devs {
                if let Ok(desc) = dev.description() {
                    devices.push(desc.name().to_string());
                }
            }
        }
        if devices.is_empty() {
            devices.push("Default Output Device".to_string());
        }
        devices
    }

    /// Inicializa e inicia el stream de audio conectando el AudioEngine con el driver
    pub fn start_stream(
        &mut self,
        device_name: &str,
        sample_rate: u32,
        buffer_size: u32,
        engine: Arc<Mutex<AudioEngine<'static>>>,
    ) -> Result<(), String> {
        // Seleccionar dispositivo
        let device = if device_name == "Default Output Device" {
            self.host
                .default_output_device()
                .ok_or_else(|| "No se encontró dispositivo de salida por defecto".to_string())?
        } else {
            self.host
                .output_devices()
                .map_err(|e| e.to_string())?
                .find(|d| {
                    d.description()
                        .map(|desc| desc.name() == device_name)
                        .unwrap_or(false)
                })
                .ok_or_else(|| format!("Dispositivo '{}' no encontrado", device_name))?
        };

        // Configuración de audio (en cpal 0.18 SampleRate se puede inicializar directo o como campo)
        let config = StreamConfig {
            channels: 2,
            sample_rate: sample_rate,
            buffer_size: cpal::BufferSize::Fixed(buffer_size),
        };

        // Construir stream en el callback de tiempo real (config va por valor)
        let stream = device
            .build_output_stream(
                config,
                move |data: &mut [f32], _| {
                    let mut buffer = AudioBuffer::new(data);
                    if let Ok(mut engine_lock) = engine.lock() {
                        engine_lock.process(&mut buffer);
                    }
                },
                |err| eprintln!("Error en el stream de audio: {}", err),
                None,
            )
            .map_err(|e| format!("Error al crear el stream: {}", e))?;

        stream.play().map_err(|e| format!("Error al reproducir stream: {}", e))?;

        self.device = Some(device);
        self.stream = Some(stream);

        Ok(())
    }

    pub fn stop_stream(&mut self) {
        self.stream = None;
    }
}
