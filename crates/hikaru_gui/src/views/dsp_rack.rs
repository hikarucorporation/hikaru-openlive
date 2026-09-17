/*
 * Hikaru OpenStudio - DSP Rack View
 * License: AGPL-3.0-or-later
 */

use egui::{Ui, RichText, Color32, ScrollArea, Frame, Stroke, Vec2, Button, Align, CursorIcon};
use crate::views::mixer::{Track, DspSlot};

pub fn show(
    ui: &mut Ui, 
    tracks: &mut Vec<Track>, 
    selected_idx: usize, 
    selected_slot: &mut usize,
) {
    if let Some(track) = tracks.get_mut(selected_idx) {
        let mut should_scroll = false;
        let mut swap_action: Option<(usize, usize)> = None;

        if !track.effects.is_empty() {
            let num_slots = track.effects.len();
            if *selected_slot >= num_slots {
                *selected_slot = num_slots.saturating_sub(1);
            }

            ui.input(|i| {
                let shift = i.modifiers.shift;

                if i.key_pressed(egui::Key::ArrowUp) {
                    if shift && *selected_slot > 0 {
                        swap_action = Some((*selected_slot, *selected_slot - 1));
                        *selected_slot -= 1;
                        should_scroll = true;
                    } else if !shift && *selected_slot > 0 {
                        *selected_slot -= 1;
                        should_scroll = true;
                    }
                }

                if i.key_pressed(egui::Key::ArrowDown) {
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
        }

        if let Some((from, to)) = swap_action {
            track.effects.swap(from, to);
        }

        ui.vertical(|ui| {
            ui.add_space(10.0);
            
            ui.vertical_centered(|ui| {
                let header_btn = ui.add(
                    Button::new(
                        RichText::new(format!("FX CHAIN: {}", track.name))
                            .strong()
                            .size(16.0)
                            .color(Color32::from_rgb(0, 255, 255))
                    )
                    .frame(false)
                    .sense(egui::Sense::hover())
                );
                
                if header_btn.hovered() {
                    ui.output_mut(|o| o.cursor_icon = CursorIcon::Default);
                }

                ui.separator();
            });

            ui.add_space(10.0);

            ui.horizontal(|ui| {
                ui.add_space(10.0);
                
                if ui.button(RichText::new(" [ + ] New Slot ").strong()).clicked() {
                    let new_id = track.effects.len();
                    track.effects.push(DspSlot::new(new_id, "Empty Slot".to_string()));
                    *selected_slot = track.effects.len() - 1;
                    should_scroll = true;
                }

                ui.set_enabled(!track.effects.is_empty());
                if ui.button(RichText::new(" [ - ] Remove Slot ").strong()).clicked() {
                    track.effects.pop();
                    if *selected_slot >= track.effects.len() && !track.effects.is_empty() {
                        *selected_slot = track.effects.len() - 1;
                    }
                }
                ui.set_enabled(true);
            });

            ui.add_space(10.0);
            ui.separator();

            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                if track.effects.is_empty() {
                    ui.add_space(40.0);
                    ui.vertical_centered(|ui| {
                        ui.add(
                            Button::new(RichText::new("No modules loaded.").color(Color32::GRAY))
                                .frame(false)
                                .sense(egui::Sense::hover())
                        );
                        ui.add(
                            Button::new(RichText::new("Click [+] to start your modular chain.").small().color(Color32::from_gray(60)))
                                .frame(false)
                                .sense(egui::Sense::hover())
                        );
                    });
                } else {
                    for (idx, slot) in track.effects.iter_mut().enumerate() {
                        let is_selected = idx == *selected_slot;
                        render_slot_placeholder(ui, slot, idx, is_selected, selected_slot, should_scroll);
                        ui.add_space(4.0);
                    }
                }
            });
        });
    }
}

fn render_slot_placeholder(
    ui: &mut Ui, 
    slot: &mut DspSlot, 
    idx: usize, 
    is_selected: bool, 
    selected_slot: &mut usize,
    should_scroll: bool,
) {
    let border_color = if is_selected { Color32::from_rgb(255, 110, 0) } else { Color32::from_gray(50) };
    let bg_color = if is_selected { Color32::from_rgb(45, 45, 45) } else { Color32::from_rgb(28, 28, 32) };

    ui.push_id(slot.id, |ui| {
        let frame_res = Frame::none()
            .fill(bg_color)
            .stroke(Stroke::new(1.0_f32, border_color))
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                ui.horizontal(|ui| {
                    // 1. Índice
                    let num_btn = ui.add(
                        Button::new(RichText::new(format!("{:02}", idx + 1)).color(Color32::from_rgb(0, 255, 255)).strong())
                            .frame(false)
                    );
                    if num_btn.clicked() {
                        *selected_slot = idx;
                    }

                    ui.separator();

                    // 2. Desplegable de módulos [ 🔻 ] con 4 Secciones VST3/CLAP + Nativos
                    ui.menu_button(RichText::new("🔻").small(), |ui| {
                        ui.set_min_width(180.0);
                        
                        // Nativos
                        ui.label(RichText::new("Native Generators").small().color(Color32::from_rgb(0, 255, 255)));
                        if ui.button(" OpenWavetable").clicked() {
                            slot.name = "OpenWavetable".to_string();
                            *selected_slot = idx;
                            ui.close_menu();
                        }

                        ui.separator();

                        ui.label(RichText::new("Native FX").small().color(Color32::from_rgb(255, 110, 0)));
                        if ui.button(" OpenSpectralFX").clicked() {
                            slot.name = "OpenSpectralFX".to_string();
                            *selected_slot = idx;
                            ui.close_menu();
                        }

                        ui.separator();

                        // 1. VST3 Generators
                        ui.label(RichText::new("VST3 Generators").small().color(Color32::from_rgb(100, 200, 255)));
                        if ui.button(" Vital (VST3)").clicked() {
                            slot.name = "Vital (VST3)".to_string();
                            *selected_slot = idx;
                            ui.close_menu();
                        }

                        ui.separator();

                        // 2. VST3 FX
                        ui.label(RichText::new("VST3 FX").small().color(Color32::from_rgb(255, 180, 100)));
                        if ui.button(" External VST3 FX...").clicked() {
                            slot.name = "VST3 FX".to_string();
                            *selected_slot = idx;
                            ui.close_menu();
                        }

                        ui.separator();

                        // 3. CLAP Generators
                        ui.label(RichText::new("CLAP Generators").small().color(Color32::from_rgb(180, 100, 255)));
                        if ui.button(" External CLAP Synth...").clicked() {
                            slot.name = "CLAP Synth".to_string();
                            *selected_slot = idx;
                            ui.close_menu();
                        }

                        ui.separator();

                        // 4. CLAP FX
                        ui.label(RichText::new("CLAP FX").small().color(Color32::from_rgb(255, 100, 180)));
                        if ui.button(" External CLAP FX...").clicked() {
                            slot.name = "CLAP FX".to_string();
                            *selected_slot = idx;
                            ui.close_menu();
                        }
                    });

                    // 3. Nombre del Slot
                    let name_btn = ui.add_sized(
                        Vec2::new(140.0, 20.0),
                        Button::new(RichText::new(&slot.name).color(if is_selected { Color32::WHITE } else { Color32::from_gray(200) }))
                            .fill(Color32::TRANSPARENT)
                            .frame(false)
                    );
                    
                    if name_btn.clicked() {
                        *selected_slot = idx;
                    }

                    // CONTROLES DE LA DERECHA (Bypass y Toggle GUI)
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        // Checkbox de Active
                        let chk = ui.checkbox(&mut slot.active, "");
                        if chk.clicked() {
                            *selected_slot = idx;
                        }

                        // Botón Toggle para abrir/cerrar la GUI (sea Nativo, VST3 o CLAP)
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
            });

        if frame_res.response.hovered() {
            ui.output_mut(|o| o.cursor_icon = CursorIcon::Default);
        }

        if is_selected && should_scroll {
            frame_res.response.scroll_to_me(Some(Align::Center));
        }
    });
}