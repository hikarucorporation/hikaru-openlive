/*
 * Hikaru OpenLive - DSP Rack View
 * License: AGPL-3.0-or-later
 */

use std::path::PathBuf;
use egui::{Ui, RichText, Color32, ScrollArea, Frame, Stroke, Button, Align, Slider};
use crate::views::mixer::{Track, DspSlot};
use crate::views::open_dms;
use crate::audio_proxy::AudioProxy;

pub fn show(
    ui: &mut egui::Ui,
    tracks: &mut Vec<Track>,
    selected_idx: usize,
    selected_slot: &mut usize,
    selected_matrix_slot: Option<(usize, usize)>, // <--- Agregamos la celda de la matriz (Track, Scene)
    dragged_sample: &mut Option<PathBuf>,
    audio_proxy: &AudioProxy,
) {
    if tracks.is_empty() {
        ui.label(RichText::new("No active track selected.").color(Color32::GRAY));
        return;
    }

    let track_idx = selected_idx.min(tracks.len() - 1);
    let track = &mut tracks[track_idx];
    let num_slots = track.effects.len();

    let mut swap_action: Option<(usize, usize)> = None;
    let mut should_scroll = false;

    // Teclado: Navegación entre slots con Flecha Izquierda / Derecha (Shift para mover)
    ui.input(|i| {
        let shift = i.modifiers.shift;

        if i.key_pressed(egui::Key::ArrowLeft) {
            if shift && *selected_slot > 0 {
                swap_action = Some((*selected_slot, *selected_slot - 1));
                *selected_slot -= 1;
                should_scroll = true;
            } else if !shift && *selected_slot > 0 {
                *selected_slot -= 1;
                should_scroll = true;
            }
        }

        if i.key_pressed(egui::Key::ArrowRight) {
            if shift && *selected_slot + 1 < num_slots {
                swap_action = Some((*selected_slot, *selected_slot + 1));
                *selected_slot += 1;
                should_scroll = true;
            } else if !shift && *selected_slot + 1 < num_slots {
                *selected_slot += 1;
                should_scroll = true;
            }
        }
    });

    if let Some((from, to)) = swap_action {
        track.effects.swap(from, to);
    }

    // Armamos la etiqueta con Track y Scene
    let rack_title = if let Some((_t_idx, s_idx)) = selected_matrix_slot {
        format!("DSP RACK: {} | Scene {}", track.name, s_idx + 1)
    } else {
        format!("DSP RACK: {}", track.name)
    };;

    // Contenedor raíz: ScrollArea horizontal directamente sobre ui
    ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| {
        ui.horizontal(|ui| {
            // Controles de cabecera a la izquierda
            ui.vertical(|ui| {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(rack_title)
                        .strong()
                        .size(14.0)
                        .color(Color32::from_rgb(0, 255, 255))
                );

                ui.add_space(4.0);
                if ui.button(RichText::new(" [ + ] Add Slot ").small().strong()).clicked() {
                    let new_id = track.effects.len();
                    track.effects.push(DspSlot::new(new_id, "Empty Slot".to_string()));
                    *selected_slot = track.effects.len() - 1;
                    should_scroll = true;
                }

                ui.set_enabled(!track.effects.is_empty());
                if ui.button(RichText::new(" [ - ] Remove ").small().strong()).clicked() {
                    track.effects.pop();
                    if *selected_slot >= track.effects.len() && !track.effects.is_empty() {
                        *selected_slot = track.effects.len() - 1;
                    }
                }
                ui.set_enabled(true);
            });

            ui.separator();

            // Cadena Horizontal de Dispositivos (Efectos / Sintes)
            if track.effects.is_empty() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.add_space(15.0);
                    ui.label(RichText::new("No modules in chain.").color(Color32::GRAY));
                    ui.label(RichText::new("Click [+] Add Slot to insert plugins.").small().color(Color32::from_gray(60)));
                });
            } else {
                let mut swap_to_trigger: Option<(usize, usize)> = None;
                let total_slots = track.effects.len();

                for (idx, slot) in track.effects.iter_mut().enumerate() {
                    let is_selected = idx == *selected_slot;
                    
                    if let Some(swap) = render_slot_card(ui, slot, idx, total_slots, is_selected, selected_slot, should_scroll, dragged_sample, audio_proxy) {
                        swap_to_trigger = Some(swap);
                    }
                    
                    ui.add_space(6.0);
                }

                if let Some((from, to)) = swap_to_trigger {
                    track.effects.swap(from, to);
                }
            }
        });
    });
}

fn render_slot_card(
    ui: &mut Ui, 
    slot: &mut DspSlot, 
    idx: usize, 
    total_slots: usize,
    is_selected: bool, 
    selected_slot: &mut usize,
    should_scroll: bool,
    dragged_sample: &mut Option<PathBuf>,
    audio_proxy: &AudioProxy,
) -> Option<(usize, usize)> {
    let mut swap_req = None;
    let border_color = if is_selected { Color32::from_rgb(255, 110, 0) } else { Color32::from_gray(50) };
    let bg_color = if is_selected { Color32::from_rgb(40, 40, 45) } else { Color32::from_rgb(25, 25, 28) };

    ui.push_id(slot.id, |ui| {
        let frame_res = Frame::none()
            .fill(bg_color)
            .stroke(Stroke::new(1.0_f32, border_color))
            .inner_margin(6.0)
            .show(ui, |ui| {
                // Adaptamos el ancho de la tarjeta según el tipo de dispositivo
                let card_width = match slot.name.as_str() {
                    "OpenWavetable" => 240.0,
                    "Hikaru OpenDMS" => 540.0,
                    "OpenSpectralFX" => 200.0,
                    "Empty Slot" => 170.0,
                    _ => 190.0,
                };

                ui.set_width(card_width);
                let card_height = if slot.name == "Hikaru OpenDMS" { 220.0 } else { 100.0 };
                ui.set_height(card_height);

                ui.vertical(|ui| {
                    // Cabecera: Controles de orden, índice, bypass y selector de plugin
                    ui.horizontal(|ui| {
                        ui.set_enabled(idx > 0);
                        if ui.button(RichText::new("◀").small()).clicked() {
                            swap_req = Some((idx, idx - 1));
                            *selected_slot = idx - 1;
                        }

                        ui.set_enabled(idx + 1 < total_slots);
                        if ui.button(RichText::new("▶").small()).clicked() {
                            swap_req = Some((idx, idx + 1));
                            *selected_slot = idx + 1;
                        }
                        ui.set_enabled(true);

                        let num_btn = ui.add(
                            Button::new(RichText::new(format!("{:02}", idx + 1)).color(Color32::from_rgb(0, 255, 255)).strong())
                                .frame(false)
                        );
                        if num_btn.clicked() {
                            *selected_slot = idx;
                        }

                        ui.menu_button(RichText::new("🔻").small(), |ui| {
                            ui.set_min_width(180.0);

                            ui.label(RichText::new("Native Generators").small().color(Color32::from_rgb(0, 255, 255)));
                            ui.indent("hdr_native_gen", |ui| {
                                ui.label(RichText::new("Synths").small().color(Color32::from_rgb(0, 200, 200)));
                                if ui.button(" OpenWavetable").clicked() {
                                    slot.name = "OpenWavetable".to_string();
                                    *selected_slot = idx;
                                    ui.close_menu();
                                }
                                ui.label(RichText::new("Drums").small().color(Color32::from_rgb(0, 200, 200)));
                                if ui.button(" Hikaru OpenDMS").clicked() {
                                    slot.name = "Hikaru OpenDMS".to_string();
                                    *selected_slot = idx;
                                    ui.close_menu();
                                }
                            });

                            ui.separator();

                            ui.label(RichText::new("Native FX").small().color(Color32::from_rgb(255, 110, 0)));
                            if ui.button(" OpenSpectralFX").clicked() {
                                slot.name = "OpenSpectralFX".to_string();
                                *selected_slot = idx;
                                ui.close_menu();
                            }

                            ui.separator();

                            ui.label(RichText::new("VST3 / External").small().color(Color32::from_rgb(100, 200, 255)));
                            if ui.button(" Vital (VST3)").clicked() {
                                slot.name = "Vital (VST3)".to_string();
                                *selected_slot = idx;
                                ui.close_menu();
                            }
                            if ui.button(" External CLAP...").clicked() {
                                slot.name = "CLAP Plugin".to_string();
                                *selected_slot = idx;
                                ui.close_menu();
                            }
                        });

                        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                            let chk = ui.checkbox(&mut slot.active, "");
                            if chk.clicked() {
                                *selected_slot = idx;
                            }

                            if slot.name != "Empty Slot" {
                                let icon_text = if slot.is_open { "▣" } else { "□" };
                                let gui_btn = ui.add(
                                    Button::new(RichText::new(icon_text).strong().size(13.0).color(Color32::from_rgb(0, 255, 255)))
                                        .frame(false)
                                );

                                if gui_btn.clicked() {
                                    slot.is_open = !slot.is_open;
                                    *selected_slot = idx;
                                }
                            }
                        });
                    });

                    ui.separator();

                    // Renderizado dinámico de la UI integrada según el módulo cargado
                    match slot.name.as_str() {
                        "OpenWavetable" => {
                            ui.label(RichText::new("Wavetable Synth").small().strong().color(Color32::from_rgb(0, 255, 255)));
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(RichText::new("WT Pos").size(10.0));
                                    ui.add(Slider::new(&mut 0.5_f32, 0.0..=1.0).show_value(false));
                                });
                                ui.vertical(|ui| {
                                    ui.label(RichText::new("Cutoff").size(10.0));
                                    ui.add(Slider::new(&mut 0.8_f32, 0.0..=1.0).show_value(false));
                                });
                            });
                        }
                        "Hikaru OpenDMS" => {
                            if slot.dms_state.is_none() {
                                slot.dms_state = Some(open_dms::OpenDms::default());
                            }
                            if let Some(ref mut dms) = slot.dms_state {
                                open_dms::render_dms_ui(ui, dms, 120.0, dragged_sample, audio_proxy);
                            }
                        }
                        "OpenSpectralFX" => {
                            ui.label(RichText::new("Spectral Processor").small().strong().color(Color32::from_rgb(255, 110, 0)));
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(RichText::new("FFT Size").size(10.0));
                                    let _ = ui.button(RichText::new("2048").small());
                                });
                                ui.vertical(|ui| {
                                    ui.label(RichText::new("Mix").size(10.0));
                                    ui.add(Slider::new(&mut 0.5_f32, 0.0..=1.0).show_value(false));
                                });
                            });
                        }
                        "Empty Slot" => {
                            ui.vertical_centered(|ui| {
                                ui.add_space(10.0);
                                ui.menu_button(RichText::new("Select Plugin 🔻").small().color(Color32::GRAY), |ui| {
                                    ui.set_min_width(180.0);

                                    ui.label(RichText::new("Native Generators").small().color(Color32::from_rgb(0, 255, 255)));
                                    ui.indent("empty_native_gen", |ui| {
                                        ui.label(RichText::new("Synths").small().color(Color32::from_rgb(0, 200, 200)));
                                        if ui.button(" OpenWavetable").clicked() {
                                            slot.name = "OpenWavetable".to_string();
                                            *selected_slot = idx;
                                            ui.close_menu();
                                        }
                                        ui.label(RichText::new("Drums").small().color(Color32::from_rgb(0, 200, 200)));
                                        if ui.button(" Hikaru OpenDMS").clicked() {
                                            slot.name = "Hikaru OpenDMS".to_string();
                                            *selected_slot = idx;
                                            ui.close_menu();
                                        }
                                    });

                                    ui.separator();

                                    ui.label(RichText::new("Native FX").small().color(Color32::from_rgb(255, 110, 0)));
                                    if ui.button(" OpenSpectralFX").clicked() {
                                        slot.name = "OpenSpectralFX".to_string();
                                        *selected_slot = idx;
                                        ui.close_menu();
                                    }

                                    ui.separator();

                                    ui.label(RichText::new("VST3 / External").small().color(Color32::from_rgb(100, 200, 255)));
                                    if ui.button(" Vital (VST3)").clicked() {
                                        slot.name = "Vital (VST3)".to_string();
                                        *selected_slot = idx;
                                        ui.close_menu();
                                    }
                                    if ui.button(" External CLAP...").clicked() {
                                        slot.name = "CLAP Plugin".to_string();
                                        *selected_slot = idx;
                                        ui.close_menu();
                                    }
                                });
                            });
                        }
                        _ => {
                            ui.vertical(|ui| {
                                ui.label(RichText::new(&slot.name).small().strong().color(Color32::WHITE));
                                ui.add_space(4.0);
                                if ui.button(RichText::new("Open Floating Window").small()).clicked() {
                                    slot.is_open = true;
                                }
                            });
                        }
                    }
                });
            });

        if is_selected && should_scroll {
            frame_res.response.scroll_to_me(Some(Align::Center));
        }
    });

    swap_req
}