// Copyright (C) Hikaru Corporation - 2026
// GNU Affero General Public License v3
// crates/hikaru_gui/src/views/piano_roll.rs

use egui::*;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PianoRollMode {
    Keys,
    Drums,
}

#[derive(Debug, Clone)]
pub struct MidiNote {
    pub pitch: u8,
    pub start_tick: u64,
    pub duration_ticks: u64,
    pub velocity: u8,
}

pub struct PianoRollState {
    pub mode: PianoRollMode,
    pub zoom_x: f32,
    pub key_height: f32,
    pub notes: Vec<MidiNote>,
    pub drum_map: HashMap<u8, String>,
    pub playhead_tick: u64,
}

impl Default for PianoRollState {
    fn default() -> Self {
        let mut drum_map = HashMap::new();
        drum_map.insert(36, "Kick 01".to_string());
        drum_map.insert(37, "Rimshot".to_string());
        drum_map.insert(38, "Snare 01".to_string());
        drum_map.insert(39, "Clap 01".to_string());
        drum_map.insert(42, "Closed Hat".to_string());
        drum_map.insert(46, "Open Hat".to_string());

        Self {
            mode: PianoRollMode::Keys,
            zoom_x: 0.15,
            key_height: 16.0,
            notes: Vec::new(),
            drum_map,
            playhead_tick: 0,
        }
    }
}

pub fn show(ui: &mut Ui, state: &mut PianoRollState) {
    ui.vertical(|ui| {
        // --- Toolbar ---
        ui.horizontal(|ui| {
            ui.selectable_value(&mut state.mode, PianoRollMode::Keys, "🎹 Keys");
            ui.selectable_value(&mut state.mode, PianoRollMode::Drums, "🥁 Drums");
            ui.separator();
            ui.label(RichText::new("Zoom H:").small());
            ui.add(Slider::new(&mut state.zoom_x, 0.02..=0.8).text("X"));
            ui.add(Slider::new(&mut state.key_height, 10.0..=28.0).text("Y"));
        });

        ui.separator();

        let sidebar_width = 100.0;
        let total_height = 128.0 * state.key_height;

        let max_note_tick = state.notes.iter().map(|n| n.start_tick + n.duration_ticks).max().unwrap_or(0);
        let min_ticks = 19200; 
        let total_ticks = max_note_tick.max(min_ticks) + 9600;
        let total_grid_width = total_ticks as f32 * state.zoom_x;

        // ScrollArea envuelve todo el área (Sidebar + Grid) para mantener Y sincronizado en nativo
        ScrollArea::both()
            .id_source("piano_roll_master_scroll")
            .show(ui, |ui| {
                let available_width = ui.available_width().max(sidebar_width + total_grid_width);
                let (total_rect, _) = ui.allocate_exact_size(
                    Vec2::new(available_width, total_height),
                    Sense::hover(),
                );

                let sidebar_rect = Rect::from_min_size(
                    total_rect.min,
                    vec2(sidebar_width, total_height),
                );

                let grid_rect = Rect::from_min_size(
                    pos2(total_rect.min.x + sidebar_width, total_rect.min.y),
                    vec2(total_grid_width, total_height),
                );

                // --- 1. RENDER SIDEBAR (Teclado) ---
                draw_sidebar(ui, sidebar_rect, state);

                // --- 2. RENDER GRILLA Y NOTAS ---
                let response = ui.interact(grid_rect, ui.id().with("grid_interact"), Sense::click_and_drag());

                if ui.is_rect_visible(grid_rect) {
                    let painter = ui.painter_at(grid_rect);
                    draw_grid_background(&painter, grid_rect, state.key_height, state.zoom_x);

                    // Render de notas (Pitch 127 arriba -> Pitch 0 abajo)
                    for note in &state.notes {
                        let note_x = grid_rect.min.x + (note.start_tick as f32 * state.zoom_x);
                        let note_w = (note.duration_ticks as f32 * state.zoom_x).max(3.0);
                        let note_y = grid_rect.min.y + ((127 - note.pitch) as f32 * state.key_height);

                        let note_rect = Rect::from_min_size(
                            pos2(note_x, note_y),
                            vec2(note_w, state.key_height - 1.0),
                        );

                        painter.rect_filled(note_rect, 2.0, Color32::from_rgb(255, 140, 0));
                        painter.rect_stroke(note_rect, 1.0, Stroke::new(1.0_f32, Color32::WHITE));
                    }

                    // Playhead Cursor
                    let playhead_x = grid_rect.min.x + (state.playhead_tick as f32 * state.zoom_x);
                    painter.line_segment(
                        [pos2(playhead_x, grid_rect.min.y), pos2(playhead_x, grid_rect.max.y)],
                        Stroke::new(1.5_f32, Color32::from_rgb(0, 200, 255)),
                    );
                }

                // Interacción Clics (Crear / Eliminar con Cuantización 1/16th)
                if response.clicked() || response.secondary_clicked() {
                    if let Some(pos) = response.interact_pointer_pos() {
                        let rel_x = pos.x - grid_rect.min.x;
                        let rel_y = pos.y - grid_rect.min.y;

                        let row = (rel_y / state.key_height) as i32;
                        let pitch = (127 - row).clamp(0, 127) as u8;

                        let raw_tick = (rel_x / state.zoom_x).max(0.0) as u64;
                        let grid_step = 240; // 1/16th note (con PPQN 960)
                        let quantized_start_tick = (raw_tick / grid_step) * grid_step;

                        if response.secondary_clicked() {
                            state.notes.retain(|n| {
                                !(n.pitch == pitch 
                                  && quantized_start_tick >= n.start_tick 
                                  && quantized_start_tick < n.start_tick + n.duration_ticks)
                            });
                        } else {
                            state.notes.push(MidiNote {
                                pitch,
                                start_tick: quantized_start_tick,
                                duration_ticks: 240, // 1/16th note
                                velocity: 100,
                            });
                        }
                    }
                }
            });
    });
}

fn draw_sidebar(ui: &Ui, rect: Rect, state: &PianoRollState) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::from_rgb(20, 20, 24));

    for pitch in (0..=127).rev() {
        let row = 127 - pitch;
        let y = rect.min.y + (row as f32 * state.key_height);
        let key_rect = Rect::from_min_size(pos2(rect.min.x, y), vec2(rect.width(), state.key_height));

        match state.mode {
            PianoRollMode::Keys => {
                let is_black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
                let bg = if is_black { Color32::from_rgb(30, 30, 35) } else { Color32::from_rgb(210, 210, 215) };
                let txt = if is_black { Color32::WHITE } else { Color32::BLACK };

                painter.rect_filled(key_rect, 0.0, bg);
                painter.rect_stroke(key_rect, 0.0, Stroke::new(0.5_f32, Color32::from_rgb(60, 60, 60)));

                let note_name = get_note_name(pitch);
                if note_name.starts_with('C') || state.key_height >= 16.0 {
                    painter.text(
                        pos2(key_rect.max.x - 4.0, key_rect.center().y),
                        Align2::RIGHT_CENTER,
                        note_name,
                        FontId::proportional((state.key_height * 0.65).clamp(8.0, 11.0)),
                        txt,
                    );
                }
            }
            PianoRollMode::Drums => {
                let display_name = state.drum_map
                    .get(&pitch)
                    .cloned()
                    .unwrap_or_else(|| format!("Pad {} ({})", pitch, get_note_name(pitch)));

                painter.rect_filled(key_rect, 1.0, Color32::from_rgb(35, 38, 48));
                painter.rect_stroke(key_rect, 1.0, Stroke::new(0.5_f32, Color32::from_rgb(55, 60, 72)));

                painter.text(
                    pos2(key_rect.min.x + 4.0, key_rect.center().y),
                    Align2::LEFT_CENTER,
                    format!("▶ {}", display_name),
                    FontId::proportional(10.0),
                    Color32::LIGHT_GRAY,
                );
            }
        }
    }
}

fn draw_grid_background(painter: &Painter, rect: Rect, key_height: f32, zoom_x: f32) {
    painter.rect_filled(rect, 0.0, Color32::from_rgb(14, 14, 18));

    // Líneas horizontales (Pitch: 0 a 127)
    for i in 0..=128 {
        let y = rect.min.y + (i as f32 * key_height);
        let pitch = 127 - i;
        let is_c = pitch >= 0 && pitch <= 127 && (pitch % 12 == 0);
        
        let stroke = if is_c {
            Stroke::new(0.8_f32, Color32::from_rgb(45, 45, 55))
        } else {
            Stroke::new(0.5_f32, Color32::from_rgb(24, 24, 30))
        };

        painter.line_segment(
            [pos2(rect.min.x, y), pos2(rect.max.x, y)],
            stroke,
        );
    }

    // Líneas verticales (Compases y semicorcheas en ticks)
    let px_per_tick = zoom_x;
    let step_ticks = 240.0_f32; // 1/16 note
    let bar_ticks = 3840.0_f32; // 4/4 bar (960 * 4)

    let mut current_tick = 0.0_f32;
    while (current_tick * px_per_tick) < rect.width() {
        let x = rect.min.x + (current_tick * px_per_tick);
        let is_bar = (current_tick % bar_ticks).abs() < 1.0_f32;
        let is_beat = (current_tick % 960.0_f32).abs() < 1.0_f32;

        let stroke = if is_bar {
            Stroke::new(1.0_f32, Color32::from_rgb(80, 80, 100))
        } else if is_beat {
            Stroke::new(0.5_f32, Color32::from_rgb(45, 45, 60))
        } else {
            Stroke::new(0.5_f32, Color32::from_rgb(28, 28, 35))
        };

        painter.line_segment([pos2(x, rect.min.y), pos2(x, rect.max.y)], stroke);
        current_tick += step_ticks;
    }
}

fn get_note_name(pitch: u8) -> String {
    let names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let octave = (pitch / 12) as i8 - 1;
    format!("{}{}", names[(pitch % 12) as usize], octave)
}