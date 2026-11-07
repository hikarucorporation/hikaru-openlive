// Hikaru OpenLive - Control Panel / Dashboard (egui)
// GNU AGPLv3
// crates/hikaru_gui/src/views/control_panel.rs

use egui::{Button, Color32, Context, RichText, Ui, Window};

use crate::app::AppState;
use crate::theme::*;
use crate::version::VERSION;
use crate::views::audio_settings::AudioBackend;
use crate::views::branding::energy_core_logo_widget;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlPanelTab {
    Audio,
    Plugins,
    Mcp,
    About,
}

impl Default for ControlPanelTab {
    fn default() -> Self {
        Self::Audio
    }
}

pub struct ControlPanelState {
    pub is_open: bool,
    pub active_tab: ControlPanelTab,
}

impl Default for ControlPanelState {
    fn default() -> Self {
        Self {
            is_open: false,
            active_tab: ControlPanelTab::Audio,
        }
    }
}

pub fn render(ctx: &Context, state: &mut AppState) {
    if !state.control_panel_state.is_open {
        return;
    }

    let mut is_open = state.control_panel_state.is_open;

    Window::new(RichText::new("HIKARU DASHBOARD · PANEL DE CONTROL").strong().color(Color32::from_rgb(0xE0, 0xE0, 0xE0)))
        .open(&mut is_open)
        .default_size([820.0, 560.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Sidebar
                ui.vertical(|ui| {
                    ui.set_width(190.0);
                    ui.spacing_mut().item_spacing.y = 4.0;

                    let tab_btn = |ui: &mut Ui, label: &str, tab: ControlPanelTab, current: ControlPanelTab| -> bool {
                        let active = tab == current;
                        let text = RichText::new(label)
                            .size(11.5)
                            .color(if active { ACCENT_ORANGE } else { TEXT_MUTED });
                        let bg = if active { Color32::from_rgb(0x2B, 0x23, 0x1D) } else { SURFACE_BG };
                        ui.add_sized([180.0, 28.0], Button::new(text).fill(bg)).clicked()
                    };

                    let current = state.control_panel_state.active_tab;
                    if tab_btn(ui, "Audio & Hardware", ControlPanelTab::Audio, current) {
                        state.control_panel_state.active_tab = ControlPanelTab::Audio;
                    }
                    if tab_btn(ui, "Plugins Externos", ControlPanelTab::Plugins, current) {
                        state.control_panel_state.active_tab = ControlPanelTab::Plugins;
                    }
                    if tab_btn(ui, "Servidores MCP", ControlPanelTab::Mcp, current) {
                        state.control_panel_state.active_tab = ControlPanelTab::Mcp;
                    }
                    if tab_btn(ui, "Acerca de Hikaru", ControlPanelTab::About, current) {
                        state.control_panel_state.active_tab = ControlPanelTab::About;
                    }

                    ui.add_space(20.0);
                    energy_core_logo_widget(ui, 48.0);
                });

                ui.separator();

                // Main Content
                ui.vertical(|ui| {
                    match state.control_panel_state.active_tab {
                        ControlPanelTab::Audio => {
                            ui.heading("Configuración de Motor de Audio y Dispositivos");
                            ui.add_space(10.0);

                            ui.label(RichText::new("Driver de Audio / Backend:").size(12.0).color(TEXT_MUTED));
                            let backend_label = format!("{:?}", state.audio_settings_state.selected_backend);
                            if ui.add(Button::new(backend_label).fill(SURFACE_BG)).clicked() {
                                state.audio_settings_state.selected_backend = match state.audio_settings_state.selected_backend {
                                    AudioBackend::HikaruNative => AudioBackend::PipeWire,
                                    AudioBackend::PipeWire => AudioBackend::Jack,
                                    AudioBackend::Jack => AudioBackend::Alsa,
                                    AudioBackend::Alsa => AudioBackend::PulseAudio,
                                    AudioBackend::PulseAudio => AudioBackend::HikaruNative,
                                };
                            }

                            ui.add_space(8.0);
                            ui.label(RichText::new("Dispositivo de Salida:").size(12.0).color(TEXT_MUTED));
                            let dev_name = state.audio_settings_state.selected_device.clone();
                            if ui.add(Button::new(dev_name).fill(SURFACE_BG)).clicked() {
                                let devs = &state.audio_settings_state.available_devices;
                                if !devs.is_empty() {
                                    let idx = devs.iter().position(|d| *d == state.audio_settings_state.selected_device).unwrap_or(0);
                                    state.audio_settings_state.selected_device = devs[(idx + 1) % devs.len()].clone();
                                }
                            }

                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(RichText::new("Sample Rate:").size(12.0).color(TEXT_MUTED));
                                    let sr_str = format!("{} Hz", state.audio_settings_state.sample_rate);
                                    if ui.add(Button::new(sr_str).fill(SURFACE_BG)).clicked() {
                                        state.audio_settings_state.sample_rate = match state.audio_settings_state.sample_rate {
                                            44100 => 48000,
                                            48000 => 96000,
                                            96000 => 192000,
                                            _ => 44100,
                                        };
                                    }
                                });

                                ui.add_space(16.0);

                                ui.vertical(|ui| {
                                    ui.label(RichText::new("Tamaño de Buffer:").size(12.0).color(TEXT_MUTED));
                                    let buf_str = format!("{} samples", state.audio_settings_state.buffer_size);
                                    if ui.add(Button::new(buf_str).fill(SURFACE_BG)).clicked() {
                                        state.audio_settings_state.buffer_size = match state.audio_settings_state.buffer_size {
                                            64 => 128,
                                            128 => 256,
                                            256 => 512,
                                            512 => 1024,
                                            _ => 64,
                                        };
                                    }
                                });
                            });
                        }
                        ControlPanelTab::Plugins => {
                            ui.heading("Gestor de Plugins Externos (VST3 / CLAP)");
                            ui.add_space(10.0);

                            ui.horizontal(|ui| {
                                let scan_label = if state.plugin_settings_state.is_scanning { "Escaneando…" } else { "Rescan plugins" };
                                if ui.add(Button::new(scan_label).fill(SURFACE_BG)).clicked() {
                                    state.plugin_settings_state.start_scan();
                                }
                                ui.label(RichText::new(&state.plugin_settings_state.status_message).size(11.0).color(TEXT_MUTED));
                            });

                            let count = state.plugin_settings_state.discovered_plugins.len();
                            ui.label(RichText::new(format!("Plugins escaneados: {}", count)).size(11.0).color(TEXT_PRIMARY));
                        }
                        ControlPanelTab::Mcp => {
                            ui.heading("Servidores de Conexión MCP (Model Context Protocol)");
                            ui.add_space(10.0);
                            ui.label(RichText::new(&state.mcp_port_settings_state.status_message).size(11.0).color(TEXT_MUTED));

                            if ui.add(Button::new("Abrir configuración de puertos MCP").fill(SURFACE_BG)).clicked() {
                                state.mcp_port_settings_state.is_open = true;
                            }
                        }
                        ControlPanelTab::About => {
                            ui.heading("Hikaru OpenLive & OpenStudio");
                            ui.add_space(10.0);
                            ui.label(RichText::new(format!("Versión: {}", VERSION)).size(12.0).color(TEXT_PRIMARY));
                            ui.label(RichText::new("DAW híbrido de clips y arreglos de código abierto.").size(11.0).color(TEXT_MUTED));
                            ui.label(RichText::new("Licencia: AGPL-3.0").size(10.5).color(TEXT_MUTED));
                        }
                    }
                });
            });
        });

    state.control_panel_state.is_open = is_open;
}
