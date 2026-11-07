// Hikaru OpenLive - MCP Port Settings (egui)
// GNU AGPLv3
// crates/hikaru_gui/src/views/mcp_port_settings.rs

use egui::{Button, Color32, Context, RichText, ScrollArea, Window};
use hikaru_mcp_port::{McpPortConfig, McpServerConfig, McpTransport, PortSlotId};

use crate::app::AppState;
use crate::theme::*;

pub struct McpPortSettingsState {
    pub is_open: bool,
    pub config: McpServerConfig,
    pub selected_port: Option<PortSlotId>,
    pub show_advanced: bool,
    pub status_message: String,
}

impl Default for McpPortSettingsState {
    fn default() -> Self {
        Self {
            is_open: false,
            config: McpServerConfig::default(),
            selected_port: None,
            show_advanced: false,
            status_message: "Servidor MCP detenido.".to_string(),
        }
    }
}

impl McpPortSettingsState {
    pub fn open(&mut self) {
        self.is_open = true;
    }

    pub fn close(&mut self) {
        self.is_open = false;
    }

    pub fn selected(&self) -> Option<&McpPortConfig> {
        match self.selected_port {
            Some(id) => self.config.slot(id),
            None => self.config.ports.first(),
        }
    }

    pub fn select(&mut self, id: PortSlotId) {
        self.selected_port = Some(id);
    }

    pub fn add_port(&mut self) -> PortSlotId {
        let id = self.config.add_port();
        self.selected_port = Some(id);
        self.status_message = format!("MCP Port {id} creado.");
        id
    }

    pub fn remove_port(&mut self, id: PortSlotId) {
        if self.config.remove_port(id) {
            if self.selected_port == Some(id) {
                self.selected_port = self.config.ports.first().map(|p| p.id);
            }
            self.status_message = format!("MCP Port {id} eliminado.");
        }
    }

    pub fn cycle_transport(&mut self, id: PortSlotId) {
        let socket = self.config.socket_path_for(id);
        let flags = self.config.server_flags.clone();

        let Some(port) = self.config.slot_mut(id) else {
            return;
        };
        port.transport = match &port.transport {
            McpTransport::UnixSocket { .. } => McpTransport::Stdio,
            McpTransport::Stdio => McpTransport::ServerFlags(flags),
            McpTransport::ServerFlags(_) => McpTransport::UnixSocket { path: socket },
        };
    }

    pub fn toggle_port(&mut self, id: PortSlotId) {
        if let Some(port) = self.config.slot_mut(id) {
            port.enabled = !port.enabled;
            let enabled = port.enabled;
            self.status_message = if enabled {
                format!("MCP Port {id} habilitado.")
            } else {
                format!("MCP Port {id} deshabilitado.")
            };
        }
    }
}

pub fn render(ctx: &Context, state: &mut AppState) {
    if !state.mcp_port_settings_state.is_open {
        return;
    }

    let mut is_open = state.mcp_port_settings_state.is_open;

    Window::new(RichText::new("HIKARU MCP PORT SETTINGS").strong().color(Color32::from_rgb(0, 255, 255)))
        .open(&mut is_open)
        .default_size([520.0, 440.0])
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;

            // Advanced Settings Toggle
            let adv_text = if state.mcp_port_settings_state.show_advanced {
                "[-] Ocultar Configuración Avanzada"
            } else {
                "[+] Mostrar Configuración Avanzada..."
            };
            if ui.add(Button::new(RichText::new(adv_text).size(11.0).color(TEXT_MUTED)).fill(SURFACE_BG)).clicked() {
                state.mcp_port_settings_state.show_advanced = !state.mcp_port_settings_state.show_advanced;
            }

            if state.mcp_port_settings_state.show_advanced {
                if let Some(sel) = state.mcp_port_settings_state.selected() {
                    let sel_id = sel.id;
                    let sel_transport = format!("{:?}", sel.transport);
                    let sel_enabled = sel.enabled;

                    egui::Frame::NONE
                        .fill(Color32::from_rgb(0x1F, 0x22, 0x2A))
                        .stroke(egui::Stroke::new(1.0_f32, BORDER_COLOR))
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            ui.label(RichText::new(format!("Detalles: MCP Port {sel_id}")).size(11.5).strong().color(ACCENT_ORANGE));
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Estado:").size(11.0).color(TEXT_MUTED));
                                if ui.add(Button::new(if sel_enabled { "Habilitado" } else { "Deshabilitado" }).fill(SURFACE_BG)).clicked() {
                                    state.mcp_port_settings_state.toggle_port(sel_id);
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Transporte:").size(11.0).color(TEXT_MUTED));
                                if ui.add(Button::new(sel_transport).fill(SURFACE_BG)).clicked() {
                                    state.mcp_port_settings_state.cycle_transport(sel_id);
                                }
                            });
                        });
                }
            }

            ui.separator();

            ui.horizontal(|ui| {
                ui.label(RichText::new("Puertos MCP Disponibles").size(11.5).strong().color(TEXT_PRIMARY));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(Button::new("+ Agregar Puerto").fill(SURFACE_BG)).clicked() {
                        state.mcp_port_settings_state.add_port();
                    }
                });
            });

            // Ports list
            let ports: Vec<(PortSlotId, bool, String)> = state
                .mcp_port_settings_state
                .config
                .ports
                .iter()
                .map(|p| (p.id, p.enabled, format!("{:?}", p.transport)))
                .collect();

            let mut to_toggle = None;
            let mut to_cycle = None;
            let mut to_remove = None;
            let mut to_select = None;

            ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                for (id, enabled, transport_name) in ports {
                    let is_sel = state.mcp_port_settings_state.selected_port == Some(id);
                    let bg = if is_sel { Color32::from_rgb(0x2B, 0x2E, 0x38) } else { Color32::from_rgb(0x20, 0x23, 0x2C) };

                    egui::Frame::NONE
                        .fill(bg)
                        .stroke(egui::Stroke::new(1.0_f32, BORDER_COLOR))
                        .inner_margin(6.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let dot_color = if enabled { PLAY_GREEN } else { TEXT_DISABLED };
                                let (dot_r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                                ui.painter().circle_filled(dot_r.center(), 3.5, dot_color);
                                ui.label(RichText::new(format!("Port {id}")).strong().color(TEXT_PRIMARY));

                                if ui.add(Button::new(if enabled { "Activo" } else { "Inactivo" }).small().fill(SURFACE_BG)).clicked() {
                                    to_toggle = Some(id);
                                }

                                if ui.add(Button::new(transport_name).small().fill(SURFACE_BG)).clicked() {
                                    to_cycle = Some(id);
                                }

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.add(Button::new("x").small()).clicked() {
                                        to_remove = Some(id);
                                    }
                                    if ui.add(Button::new("Editar").small().fill(SURFACE_BG)).clicked() {
                                        to_select = Some(id);
                                    }
                                });
                            });
                        });
                }
            });

            if let Some(id) = to_toggle {
                state.mcp_port_settings_state.toggle_port(id);
            }
            if let Some(id) = to_cycle {
                state.mcp_port_settings_state.cycle_transport(id);
            }
            if let Some(id) = to_remove {
                state.mcp_port_settings_state.remove_port(id);
            }
            if let Some(id) = to_select {
                state.mcp_port_settings_state.select(id);
                state.mcp_port_settings_state.show_advanced = true;
            }

            ui.separator();

            ui.label(RichText::new(&state.mcp_port_settings_state.status_message).size(10.5).color(TEXT_MUTED));

            ui.horizontal(|ui| {
                if ui.add(Button::new("Guardar y Lanzar Servidores").fill(Color32::from_rgb(0x1E, 0x88, 0xE5))).clicked() {
                    let default_path = hikaru_mcp_port::config::default_config_path();
                    let _ = hikaru_mcp_port::config::save_config(&state.mcp_port_settings_state.config, &default_path);
                    crate::mcp_port_launcher::lanzar_slots_habilitados();
                    state.mcp_port_settings_state.status_message = format!(
                        "Configuración guardada. Servidores lanzados: {}",
                        crate::mcp_port_launcher::lanzados()
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(Button::new("Cerrar").fill(SURFACE_BG)).clicked() {
                        state.mcp_port_settings_state.is_open = false;
                    }
                });
            });
        });

    state.mcp_port_settings_state.is_open = is_open;
}
