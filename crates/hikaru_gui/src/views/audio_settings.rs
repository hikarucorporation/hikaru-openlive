// Hikaru OpenLive - Audio Settings (egui)
// GNU AGPLv3
// crates/hikaru_gui/src/views/audio_settings.rs

use egui::{Button, Color32, Context, RichText, Window};

use crate::app::AppState;
use crate::theme::*;

#[derive(Clone, Debug, PartialEq)]
pub enum AudioBackend {
    PipeWire,
    Jack,
    Alsa,
    PulseAudio,
    HikaruNative,
}

pub struct AudioSettingsState {
    pub is_open: bool,
    pub selected_backend: AudioBackend,
    pub selected_device: String,
    pub sample_rate: u32,
    pub buffer_size: u32,
    pub available_devices: Vec<String>,
}

impl Default for AudioSettingsState {
    fn default() -> Self {
        Self {
            is_open: false,
            selected_backend: AudioBackend::HikaruNative,
            selected_device: "Hikaru Low-Latency Engine".to_string(),
            sample_rate: 44100,
            buffer_size: 128,
            available_devices: vec![
                "Hikaru Low-Latency Engine".to_string(),
                "Default Output Device".to_string(),
                "ALSA: PulseAudio / PipeWire Sound Server".to_string(),
                "JACK Audio Connection Kit".to_string(),
            ],
        }
    }
}

pub fn render(ctx: &Context, state: &mut AppState) {
    if !state.audio_settings_state.is_open {
        return;
    }

    let mut is_open = state.audio_settings_state.is_open;

    Window::new(RichText::new("CONFIGURACIÓN DE AUDIO & HARDWARE").strong().color(Color32::from_rgb(0, 255, 255)))
        .open(&mut is_open)
        .resizable(false)
        .default_size([440.0, 300.0])
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;

            // Driver / Backend
            ui.horizontal(|ui| {
                ui.label(RichText::new("Driver / Subsistema:").size(12.0).color(TEXT_MUTED));
                let label = format!("{:?}", state.audio_settings_state.selected_backend);
                if ui.add(Button::new(label).fill(SURFACE_BG)).clicked() {
                    state.audio_settings_state.selected_backend = match state.audio_settings_state.selected_backend {
                        AudioBackend::HikaruNative => AudioBackend::PipeWire,
                        AudioBackend::PipeWire => AudioBackend::Jack,
                        AudioBackend::Jack => AudioBackend::Alsa,
                        AudioBackend::Alsa => AudioBackend::PulseAudio,
                        AudioBackend::PulseAudio => AudioBackend::HikaruNative,
                    };
                }
            });

            // Dispositivo
            ui.horizontal(|ui| {
                ui.label(RichText::new("Dispositivo de Salida:").size(12.0).color(TEXT_MUTED));
                let dev_name = state.audio_settings_state.selected_device.clone();
                if ui.add(Button::new(dev_name).fill(SURFACE_BG)).clicked() {
                    let devs = &state.audio_settings_state.available_devices;
                    if !devs.is_empty() {
                        let idx = devs.iter().position(|d| *d == state.audio_settings_state.selected_device).unwrap_or(0);
                        state.audio_settings_state.selected_device = devs[(idx + 1) % devs.len()].clone();
                    }
                }
            });

            // Sample Rate
            ui.horizontal(|ui| {
                ui.label(RichText::new("Sample Rate:").size(12.0).color(TEXT_MUTED));
                let sr_label = format!("{} Hz", state.audio_settings_state.sample_rate);
                if ui.add(Button::new(sr_label).fill(SURFACE_BG)).clicked() {
                    state.audio_settings_state.sample_rate = match state.audio_settings_state.sample_rate {
                        44100 => 48000,
                        48000 => 88200,
                        88200 => 96000,
                        _ => 44100,
                    };
                }
            });

            // Buffer Size
            ui.horizontal(|ui| {
                ui.label(RichText::new("Buffer Size:").size(12.0).color(TEXT_MUTED));
                let buf_label = format!("{} samples", state.audio_settings_state.buffer_size);
                if ui.add(Button::new(buf_label).fill(SURFACE_BG)).clicked() {
                    state.audio_settings_state.buffer_size = match state.audio_settings_state.buffer_size {
                        128 => 256,
                        256 => 512,
                        512 => 1024,
                        1024 => 2048,
                        2048 => 4096,
                        _ => 128,
                    };
                }
            });

            ui.separator();

            ui.horizontal(|ui| {
                if ui.add(Button::new("Reiniciar Driver Audio").fill(SURFACE_BG)).clicked() {
                    // No-op or trigger reload
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(Button::new("Cerrar").fill(SURFACE_BG)).clicked() {
                        state.audio_settings_state.is_open = false;
                    }
                });
            });
        });

    state.audio_settings_state.is_open = is_open;
}
