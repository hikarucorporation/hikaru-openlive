/*
 * Hikaru OpenStudio - Pro Dynamic Mixer (Standard Event Architecture)
 * License: AGPL-3.0-or-later
 */

use egui::{Ui, Vec2, Color32, RichText, Stroke, Frame, Rect, Pos2, Sense, Button, ScrollArea, TextEdit, Align};
use std::f32::consts::PI;
use crate::app::{PanMode, AppMode};
use crate::views::open_wavetable::{WavetableOscillator, ModulatorNode};

// --- DATA STRUCTURES ---

#[derive(Clone, Debug)] // <--- AGREGAR AQUÍ
pub struct DspSlot {
    pub id: usize,
    pub name: String,
    pub active: bool,
    pub is_open: bool,
    pub cam_x: f32,
    pub cam_y: f32,
    pub cam_z: f32,
    pub wavetable_oscillators: Vec<WavetableOscillator>,
    pub modulators: Vec<ModulatorNode>,
}

impl DspSlot {
    pub fn new(id: usize, name: String) -> Self {
        Self {
            id,
            name,
            active: true,
            is_open: false,
            cam_x: 0.35,
            cam_y: 0.0,
            cam_z: 0.5,
            wavetable_oscillators: vec![
                WavetableOscillator::new(0, "OSC A", egui::Pos2::new(40.0, 60.0)),
            ],
            modulators: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)] // <--- AGREGAR AQUÍ
pub struct SendConnection {
    pub target_id: usize,
    pub amount: f32, 
}

#[derive(Clone, Debug)]
pub struct Track {
    pub id: usize, 
    pub name: String,
    pub volume: f32,
    pub pan: f32,
    pub pan_mode: PanMode,
    pub mute: bool,
    pub solo: bool,
    pub is_master: bool,
    pub route_destination_id: usize, 
    pub sends: Vec<SendConnection>,
    pub effects: Vec<DspSlot>,
    pub matrix_idx: Option<usize>,
}

impl Track {
    pub fn new(id: usize, name: String, is_master: bool) -> Self {
        Self {
            id, name, volume: 0.75, pan: 0.0, pan_mode: PanMode::Stereo,
            mute: false, solo: false, is_master,
            route_destination_id: 0,
            sends: Vec::new(),
            effects: Vec::new(),
            matrix_idx: None,
        }
    }
}

// --- MAIN MIXER RENDER ---

pub fn show(ui: &mut Ui, tracks: &mut Vec<Track>, selected_idx: &mut usize, current_mode: &mut AppMode, output_level: f32, track_peaks: &[f32; 16]) {
    let shift_pressed = ui.input(|i| i.modifiers.shift);

    let left_pressed = ui.input(|i| i.key_pressed(egui::Key::ArrowLeft));
    let right_pressed = ui.input(|i| i.key_pressed(egui::Key::ArrowRight));
    let nav_event = left_pressed || right_pressed;

    // FLECHA IZQUIERDA
    if left_pressed {
        if shift_pressed {
            if *selected_idx > 1 {
                tracks.swap(*selected_idx, *selected_idx - 1);
                *selected_idx -= 1;
            }
        } else if *selected_idx > 0 {
            *selected_idx -= 1;
        }
    }

    // FLECHA DERECHA
    if right_pressed {
        if shift_pressed {
            if *selected_idx > 0 && *selected_idx < tracks.len() - 1 {
                tracks.swap(*selected_idx, *selected_idx + 1);
                *selected_idx += 1;
            }
        } else if *selected_idx < tracks.len() - 1 {
            *selected_idx += 1;
        }
    }

    let tracks_map: Vec<(usize, String)> = tracks.iter()
        .map(|t| (t.id, t.name.clone()))
        .collect();

    // Alto total disponible en la ventana actual
    let total_h = ui.available_height();
    // Restamos el padding y los widgets fijos (labels, knobs, mute/solo, routing)
    // Se usa max(40.0) para garantizar que los faders se adapten bien al achicar
    let fader_height = (total_h - 270.0).max(10.0);

    ui.vertical(|ui| {
        // --- TOOLBAR SUPERIOR ---
        ui.horizontal(|ui| {
            ui.add_space(5.0);

            // TOGGLE SINCRONIZADO OPENLIVE / OPENSTUDIO
            let is_live = *current_mode == AppMode::OpenLive;
            let live_bg = if is_live { Color32::from_rgb(0, 180, 216) } else { Color32::from_gray(35) };
            let live_txt = if is_live { Color32::BLACK } else { Color32::GRAY };

            if ui.add(Button::new(RichText::new("OPENLIVE").strong().size(10.0).color(live_txt)).fill(live_bg)).clicked() {
                *current_mode = AppMode::OpenLive;
            }

            let is_studio = *current_mode == AppMode::OpenStudio;
            let studio_bg = if is_studio { Color32::from_rgb(255, 110, 0) } else { Color32::from_gray(35) };
            let studio_txt = if is_studio { Color32::BLACK } else { Color32::GRAY };

            if ui.add(Button::new(RichText::new("OPENSTUDIO").strong().size(10.0).color(studio_txt)).fill(studio_bg)).clicked() {
                *current_mode = AppMode::OpenStudio;
            }

            ui.separator();

            if ui.button(RichText::new(" [ + ] ").strong()).clicked() {
                let new_id = tracks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
                tracks.push(Track::new(new_id, format!("Track {:02}", new_id), false));
            }
            ui.set_enabled(tracks.len() > 1);
            if ui.button(RichText::new(" [ - ] ").strong()).clicked() {
                tracks.pop();
                if *selected_idx >= tracks.len() { *selected_idx = tracks.len() - 1; }
            }
            ui.set_enabled(true);

            ui.separator();
            
            // --- INDICADOR & EDICIÓN DEL TRACK ACTIVO EN TOOLBAR ---
            if let Some(current_track) = tracks.get_mut(*selected_idx) {
                let toolbar_editing_id = ui.id().with(("toolbar_editing_track", current_track.id));
                let is_editing_toolbar = ui.data_mut(|d| d.get_temp::<bool>(toolbar_editing_id).unwrap_or(false));

                if is_editing_toolbar {
                    let text_res = ui.add(
                        TextEdit::singleline(&mut current_track.name)
                            .frame(true)
                            .desired_width(160.0)
                            .font(egui::FontId::proportional(12.0))
                    );

                    if text_res.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        ui.data_mut(|d| d.insert_temp(toolbar_editing_id, false));
                        ui.memory_mut(|m| m.stop_text_input());
                    }
                } else {
                    let track_prefix = if current_track.is_master { "MASTER".to_string() } else { format!("TRK {:02}", current_track.id) };
                    let lbl_text = format!("[ {}: {} ]", track_prefix, current_track.name);
                    
                    let lbl_res = ui.add(
                        Button::new(
                            RichText::new(lbl_text)
                                .strong()
                                .color(Color32::from_rgb(255, 110, 0))
                        )
                        .frame(false)
                    );

                    if lbl_res.double_clicked() {
                        ui.data_mut(|d| d.insert_temp(toolbar_editing_id, true));
                    }
                }
            } else {
                ui.label(RichText::new("[ --- ]").strong().color(Color32::GRAY));
            }

            ui.separator();
            ui.label(RichText::new(format!("TOTAL: {}", tracks.len())).small().color(Color32::GRAY));
        });

        ui.add_space(5.0);
        ui.separator();

        // --- MIXER BODY ---
        // El vúmetro del MASTER muestra el pico REAL del engine
        // (`output_level`: 0.0 == -inf dB en silencio). Antes dibujaba
        // `track.volume` (el fader) y mentía en compases sin audio.
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
            if let Some(master) = tracks.get_mut(0) {
                render_channel_strip(ui, master, fader_height, &tracks_map, *selected_idx == 0, selected_idx, 0, nav_event, output_level);
            }

            ui.add_space(2.0);
            ui.separator();
            ui.add_space(2.0);

            ScrollArea::horizontal()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                        for i in 1..tracks.len() {
                            if let Some(track) = tracks.get_mut(i) {
                                // VU meter por pista: peak Aislado de esa pista
                                // (el engine calcula el peak ANTES del master fader).
                                let ch_level = if let Some(mx_idx) = track.matrix_idx {
                                    track_peaks.get(mx_idx).copied().unwrap_or(0.0)
                                } else {
                                    0.0
                                };
                                render_channel_strip(ui, track, fader_height, &tracks_map, i == *selected_idx, selected_idx, i, nav_event, ch_level);
                                ui.add_space(4.0);
                            }
                        }
                    });
                });
        });
    });
}

fn render_channel_strip(
    ui: &mut Ui, 
    track: &mut Track, 
    fader_h: f32, 
    tracks_map: &[(usize, String)], 
    is_selected: bool, 
    selected_idx: &mut usize, 
    current_idx: usize,
    should_scroll: bool,
    vu_level: f32,
) {
    let border_color = if is_selected { Color32::from_rgb(255, 110, 0) } else { Color32::from_gray(40) };
    let bg_color = if track.is_master { Color32::from_rgb(30, 30, 45) } else if is_selected { Color32::from_rgb(35, 35, 35) } else { Color32::from_rgb(25, 25, 30) };

    ui.push_id(track.id, |ui| {
        let frame_res = Frame::none()
            .fill(bg_color)
            .stroke(Stroke::new(1.0_f32, border_color))
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.set_width(85.0);
                ui.vertical_centered(|ui| {
                    
                    // --- CABECERA DE CANAL LIMPIA (TRK XX: Name) ---
                    let strip_label = if track.is_master {
                        "MASTER".to_string()
                    } else {
                        format!("TRK {:02}: {}", track.id, track.name)
                    };

                    let is_this_selected = current_idx == *selected_idx;
                    let label_res = ui.add_sized(
                        Vec2::new(85.0, 20.0),
                        Button::new(
                            RichText::new(strip_label)
                                .strong()
                                .size(11.0)
                                .color(if is_this_selected { Color32::WHITE } else { Color32::from_gray(200) })
                        )
                        .fill(if is_this_selected { Color32::from_rgb(0, 120, 215) } else { Color32::TRANSPARENT })
                        .frame(false)
                    );

                    if label_res.clicked() {
                        *selected_idx = current_idx;
                    }

                    ui.separator();

                    // PAN MODE & KNOB
                    ui.add_space(2.0);
                    let is_ms = track.pan_mode == PanMode::MidSide;
                    let pm_color = if is_ms { Color32::from_rgb(0, 255, 255) } else { Color32::GRAY };
                    if ui.button(RichText::new(if is_ms { "M/S" } else { "L/R" }).small().color(pm_color)).clicked() {
                        track.pan_mode = if is_ms { PanMode::Stereo } else { PanMode::MidSide };
                    }
                    
                    // --- EN render_channel_strip() ---

                    // Perilla de paneo adaptada al rango normalizado -1.0..=1.0
                    custom_knob(ui, &mut track.pan, -1.0..=1.0);

                    let p_dv = ui.add(
                        egui::DragValue::new(&mut track.pan)
                            .speed(0.01)
                            .clamp_range(-1.0..=1.0)
                            .fixed_decimals(2)
                            .custom_formatter(|n, _| {
                                let val = (n * 100.0).round() as i32;
                                if val == 0 {
                                    "C".to_string()
                                } else if val < 0 {
                                    format!("{}L", val.abs())
                                } else {
                                    format!("{}R", val)
                                }
                            })
                            .custom_parser(|s| {
                                let s = s.trim().to_uppercase();
                                if s == "C" {
                                    Some(0.0)
                                } else if s.ends_with('L') {
                                    s[..s.len() - 1].parse::<f64>().ok().map(|v| -v / 100.0)
                                } else if s.ends_with('R') {
                                    s[..s.len() - 1].parse::<f64>().ok().map(|v| v / 100.0)
                                } else {
                                    s.parse::<f64>().ok().map(|v| v / 100.0)
                                }
                            })
                    );

                    if p_dv.double_clicked() { 
                        track.pan = 0.0; 
                    }

                    if p_dv.double_clicked() { 
                        track.pan = 0.0; 
                    }

                    ui.add_space(5.0);

                    // FADER & VU: el VU muestra señal REAL, no el fader.
                    // En silencio (compás sin audio) vu_level == 0.0 == -inf.
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        draw_vu_meter(ui, vu_level, fader_h);
                        custom_v_fader(ui, &mut track.volume, Vec2::new(35.0, fader_h));
                    });

                    ui.add_space(5.0);

                    // DB DISPLAY
                    let mut db_val = if track.volume <= 0.0 { -60.0 } else if track.volume <= 0.75 { -60.0 + (track.volume / 0.75) * 60.0 } else { ((track.volume - 0.75) / 0.25) * 6.0 };
                    let db_dv = ui.add(egui::DragValue::new(&mut db_val).speed(0.2).clamp_range(-60.0..=6.0).suffix(" dB").fixed_decimals(1));
                    
                    if db_dv.double_clicked() {
                        track.volume = 0.75; 
                    } else if db_dv.changed() {
                        if db_val <= 0.0 { track.volume = ((db_val + 60.0) / 60.0 * 0.75).clamp(0.0, 0.75); } else { track.volume = ((db_val / 6.0 * 0.25) + 0.75).clamp(0.75, 1.0); }
                    }

                    ui.add_space(5.0);

                    // MUTE / SOLO
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let b_size = Vec2::new(32.0, 22.0);
                        if ui.add_sized(b_size, Button::new("M").fill(if track.mute { Color32::from_rgb(200, 0, 0) } else { Color32::from_gray(40) })).clicked() { track.mute = !track.mute; }
                        if ui.add_sized(b_size, Button::new("S").fill(if track.solo { Color32::from_rgb(200, 160, 0) } else { Color32::from_gray(40) })).clicked() { track.solo = !track.solo; }
                    });

                    ui.add_space(5.0);
                    ui.separator();

                    // ROUTE
                    if track.is_master {
                        ui.add_space(6.0);
                        ui.label(RichText::new("MASTER OUT").strong().size(10.0).color(Color32::from_gray(100)));
                        ui.add_space(6.0);
                    } else {
                        ui.menu_button(RichText::new("󰒄 ROUTE").small(), |ui| {
                            ui.set_min_width(180.0);
                            for (id, name) in tracks_map {
                                if *id == track.id { continue; } 
                                let label = if name.is_empty() || name.starts_with("Track") { format!("Track {}", id) } else { format!("Track {}: {}", id, name) };
                                if ui.selectable_label(track.route_destination_id == *id, label).clicked() {
                                    track.route_destination_id = *id;
                                    ui.close_menu();
                                }
                            }
                        });
                        let dest_label = if track.route_destination_id == 0 { "-> MASTER".to_string() } else { format!("-> TRK {:02}", track.route_destination_id) };
                        ui.label(RichText::new(dest_label).size(9.0).color(Color32::from_rgb(100, 150, 255)));
                    }
                });
            });

        if is_selected && should_scroll {
            frame_res.response.scroll_to_me(Some(Align::Center));
        }
    });
}

// --- WIDGETS CUSTOM ---

fn custom_knob(ui: &mut Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>) {
    let size = Vec2::splat(28.0);
    let (rect, response) = ui.allocate_at_least(size, Sense::click_and_drag());

    let min = *range.start();
    let max = *range.end();
    let span = max - min;

    if response.double_clicked() {
        *value = 0.0;
    } else if response.dragged() {
        let delta = response.drag_delta().x - response.drag_delta().y;
        let change = (delta / 150.0) * span;
        *value = (*value + change).clamp(min, max);
    }

    let painter = ui.painter();
    painter.circle_filled(rect.center(), 10.0, Color32::from_gray(15));

    let normalized = ((*value - min) / span).clamp(0.0, 1.0);
    let angle = (normalized - 0.5) * (PI * 1.5);

    let handle_pos = rect.center() + Vec2::new(angle.sin(), -angle.cos()) * 9.0;
    let center_pos = rect.center() + Vec2::new(angle.sin(), -angle.cos()) * 4.0;
    painter.line_segment([center_pos, handle_pos], Stroke::new(2.0_f32, Color32::from_rgb(255, 110, 0)));
}

fn custom_v_fader(ui: &mut Ui, value: &mut f32, size: Vec2) {
    let (rect, response) = ui.allocate_at_least(size, Sense::click_and_drag());
    
    if response.double_clicked() {
        *value = 0.75; 
    } else if response.dragged() {
        let delta_y = response.drag_delta().y;
        *value = (*value - delta_y / rect.height()).clamp(0.0, 1.0);
    } else if response.clicked() {
        if let Some(mouse_pos) = response.interact_pointer_pos() {
            *value = ((rect.bottom() - mouse_pos.y) / rect.height()).clamp(0.0, 1.0);
        }
    }

    let painter = ui.painter();
    painter.rect_filled(Rect::from_center_size(rect.center(), Vec2::new(16.0, rect.height())), 1.0, Color32::from_gray(10));
    
    // CORRECCIÓN: Validamos que min_y <= max_y para prevenir el panic (min > max) al achicar la ventana
    let min_y = (rect.top() + 9.0).min(rect.bottom());
    let max_y = (rect.bottom() - 9.0).max(min_y);
    let target_y = (rect.bottom() - (*value * rect.height())).clamp(min_y, max_y);

    let cap_rect = Rect::from_center_size(Pos2::new(rect.center().x, target_y), Vec2::new(size.x, 40.0));
    painter.rect_filled(cap_rect, 2.0, if response.dragged() { Color32::from_gray(200) } else { Color32::from_gray(160) });
    painter.rect_stroke(cap_rect, 1.5_f32, Stroke::new(1.0_f32, Color32::BLACK));
    painter.rect_filled(Rect::from_center_size(cap_rect.center(), Vec2::new(cap_rect.width() - 16.0, 1.5_f32)), 0.0, Color32::BLACK);
}

fn draw_vu_meter(ui: &mut Ui, level: f32, height: f32) {
    let (rect, _) = ui.allocate_at_least(Vec2::new(14.0, height), egui::Sense::hover());
    ui.painter().rect_filled(rect, 1.0, Color32::from_rgb(10, 10, 10));
    let signal_rect = Rect::from_min_max(Pos2::new(rect.min.x, rect.max.y - (level.clamp(0.0, 1.0) * rect.height())), rect.max);
    ui.painter().rect_filled(signal_rect, 1.0, if level > 0.9 { Color32::RED } else if level > 0.75 { Color32::from_rgb(255, 200, 0) } else { Color32::from_rgb(0, 255, 100) });
}