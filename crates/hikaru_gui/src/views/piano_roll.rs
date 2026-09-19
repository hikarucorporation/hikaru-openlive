// Copyright (C) Hikaru Corporation - 2026
// GNU Affero General Public License v3
// crates/hikaru_gui/src/views/piano_roll.rs

use egui::*;
use std::collections::{HashMap, HashSet};
use crate::audio_proxy::{AudioProxy, GuiCommand};
use super::open_dms::OpenDms;
use super::mixer::Track;

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
    pub prev_playhead_tick: u64,
    triggered_notes: HashSet<(u8, u64)>,
}

impl Default for PianoRollState {
    fn default() -> Self {
        Self {
            mode: PianoRollMode::Keys,
            zoom_x: 0.15,
            key_height: 16.0,
            notes: Vec::new(),
            drum_map: HashMap::new(),
            playhead_tick: 0,
            prev_playhead_tick: 0,
            triggered_notes: HashSet::new(),
        }
    }
}

pub fn sync_drum_map_from_opendms(state: &mut PianoRollState, opendms: &OpenDms) {
    let pad_entries: Vec<(u8, String)> = opendms.pads.iter().map(|pad| {
        let display_name = if pad.sample_path.is_some() {
            let filename = pad.display_filename();
            format!("{}: {}", pad.name, filename)
        } else {
            pad.name.clone()
        };
        (pad.midi_note, display_name)
    }).collect();

    state.drum_map.clear();
    for (pitch, name) in pad_entries {
        state.drum_map.insert(pitch, name);
    }
}

fn find_opendms_in_track(track: &Track) -> Option<&OpenDms> {
    track.effects.iter().find_map(|slot| {
        if slot.name == "Hikaru OpenDMS" {
            slot.dms_state.as_ref()
        } else {
            None
        }
    })
}

const QUANTIZE_TICKS: u64 = 240; // 1/16 note a 960 PPQ
const NOTE_INSERT_VELOCITY: u8 = 100;

pub fn show(
    ui: &mut Ui,
    state: &mut PianoRollState,
    tracks: &[Track],
    selected_track_index: usize,
    audio_proxy: &AudioProxy,
) {
    if let Some(track) = tracks.get(selected_track_index) {
        let has_opendms = track.effects.iter().any(|s| s.name == "Hikaru OpenDMS");
        if has_opendms && state.mode == PianoRollMode::Keys && state.drum_map.is_empty() {
            state.mode = PianoRollMode::Drums;
        }
    }

    if let Some(track) = tracks.get(selected_track_index) {
        if let Some(opendms) = find_opendms_in_track(track) {
            sync_drum_map_from_opendms(state, opendms);
        }
    }

    ui.vertical(|ui| {
        // --- Toolbar ---
        ui.horizontal(|ui| {
            ui.selectable_value(&mut state.mode, PianoRollMode::Keys, "\u{1F3B9} Keys");
            ui.selectable_value(&mut state.mode, PianoRollMode::Drums, "\u{1F941} Drums");
            ui.separator();
            ui.label(RichText::new("Zoom H:").small());
            ui.add(Slider::new(&mut state.zoom_x, 0.02..=0.8).text("X"));
            ui.add(Slider::new(&mut state.key_height, 10.0..=28.0).text("Y"));
            ui.separator();
            ui.label(RichText::new(format!("Tick: {}", state.playhead_tick)).small());
        });

        ui.separator();

        let sidebar_width = 140.0;
        let total_height = 128.0 * state.key_height;

        let max_note_tick = state
            .notes
            .iter()
            .map(|n| n.start_tick + n.duration_ticks)
            .max()
            .unwrap_or(0);
        let total_ticks = max_note_tick.max(19200) + 9600;
        let total_grid_width = total_ticks as f32 * state.zoom_x;

        ScrollArea::both()
            .id_source("piano_roll_master_scroll")
            .show_viewport(ui, |ui, _viewport| {
                let content_width = (sidebar_width + total_grid_width).max(ui.available_width());
                let (content_rect, _) = ui.allocate_exact_size(
                    vec2(content_width, total_height),
                    Sense::hover(),
                );

                let sidebar_rect = Rect::from_min_size(
                    content_rect.min,
                    vec2(sidebar_width, total_height),
                );
                let grid_rect = Rect::from_min_size(
                    pos2(content_rect.min.x + sidebar_width, content_rect.min.y),
                    vec2(total_grid_width, total_height),
                );

                // --- 1. DIBUJO: fondo grilla y notas existentes ---
                if ui.is_rect_visible(grid_rect) {
                    let painter = ui.painter_at(grid_rect);
                    draw_grid_background(&painter, grid_rect, state.key_height, state.zoom_x);

                    for note in &state.notes {
                        let rect = note_rect(grid_rect, note, state.zoom_x, state.key_height);
                        if rect.intersects(grid_rect) {
                            painter.rect_filled(rect, 2.0, Color32::from_rgb(255, 140, 0));
                            painter.rect_stroke(rect, 1.0, Stroke::new(1.0_f32, Color32::WHITE));
                        }
                    }
                }

                // --- 2. INTERACCIÓN Y CREACIÓN/ELIMINACIÓN DE NOTAS ---
                let grid_response = ui.interact(
                    grid_rect,
                    ui.id().with("piano_roll_grid_interaction"),
                    Sense::click_and_drag(),
                );

                // Regla superior para mover el playhead con clic/drag
                if let Some(pointer_pos) = grid_response.interact_pointer_pos() {
                    let local_x = pointer_pos.x - grid_rect.min.x;
                    let local_y = pointer_pos.y - grid_rect.min.y;

                    if local_y <= 20.0 && local_x >= 0.0 {
                        let clicked_tick = (local_x / state.zoom_x).max(0.0) as u64;
                        state.playhead_tick = clicked_tick;
                        ui.ctx().request_repaint();
                    }
                }

                // Inserción / Eliminación de notas MIDI
                if let Some(hover_pos) = grid_response.hover_pos() {
                    let local_x = hover_pos.x - grid_rect.min.x;
                    let local_y = hover_pos.y - grid_rect.min.y;

                    if local_x >= 0.0 && local_y > 20.0 {
                        let raw_tick = (local_x / state.zoom_x).max(0.0) as u64;
                        let quantized_tick = (raw_tick / QUANTIZE_TICKS) * QUANTIZE_TICKS;

                        let row = (local_y / state.key_height).max(0.0) as i32;
                        let pitch = (127 - row).clamp(0, 127) as u8;

                        // Ghost preview
                        if ui.is_rect_visible(grid_rect) {
                            let ghost_rect = Rect::from_min_size(
                                pos2(
                                    grid_rect.min.x + (quantized_tick as f32 * state.zoom_x),
                                    grid_rect.min.y + (row.clamp(0, 127) as f32 * state.key_height),
                                ),
                                vec2(
                                    QUANTIZE_TICKS as f32 * state.zoom_x,
                                    state.key_height - 1.0,
                                ),
                            );
                            ui.painter_at(grid_rect).rect_filled(
                                ghost_rect,
                                2.0,
                                Color32::from_rgba_premultiplied(255, 140, 0, 60),
                            );
                        }

                        // Clic izquierdo: INSERTAR NOTA MIDI
                        if grid_response.clicked() {
                            let already_exists = state.notes.iter().any(|n| {
                                n.pitch == pitch
                                    && quantized_tick >= n.start_tick
                                    && quantized_tick < n.start_tick + n.duration_ticks
                            });

                            if !already_exists {
                                state.notes.push(MidiNote {
                                    pitch,
                                    start_tick: quantized_tick,
                                    duration_ticks: QUANTIZE_TICKS,
                                    velocity: NOTE_INSERT_VELOCITY,
                                });
                                ui.ctx().request_repaint();

                                if state.mode == PianoRollMode::Drums {
                                    preview_drum_pad(tracks, selected_track_index, pitch, 1.0, audio_proxy);
                                }
                            }
                        }

                        // Clic derecho: ELIMINAR NOTA MIDI
                        if grid_response.secondary_clicked() {
                            let prev_len = state.notes.len();
                            state.notes.retain(|n| {
                                !(n.pitch == pitch
                                    && quantized_tick >= n.start_tick
                                    && quantized_tick < n.start_tick + n.duration_ticks)
                            });
                            if state.notes.len() != prev_len {
                                ui.ctx().request_repaint();
                            }
                        }
                    }
                }

                // --- 3. DIBUJAR PLAYHEAD (Línea + Cabeza del marcador) ---
                if ui.is_rect_visible(grid_rect) {
                    let painter = ui.painter_at(grid_rect);
                    let playhead_x = grid_rect.min.x + (state.playhead_tick as f32 * state.zoom_x);

                    // Línea vertical
                    painter.line_segment(
                        [
                            pos2(playhead_x, grid_rect.min.y),
                            pos2(playhead_x, grid_rect.max.y),
                        ],
                        Stroke::new(2.0_f32, Color32::from_rgb(0, 200, 255)),
                    );

                    // Cabeza triangular del Playhead
                    let head_size = 6.0_f32;
                    let head_triangle = vec![
                        pos2(playhead_x - head_size, grid_rect.min.y),
                        pos2(playhead_x + head_size, grid_rect.min.y),
                        pos2(playhead_x, grid_rect.min.y + 10.0),
                    ];
                    
                    painter.add(Shape::convex_polygon(
                        head_triangle,
                        Color32::from_rgb(0, 200, 255),
                        Stroke::NONE,
                    ));
                }

                // --- 4. SIDEBAR + PREESCUCHA ---
                let sidebar_clicked_pitch = draw_sidebar(ui, sidebar_rect, state);
                if let Some(clicked_pitch) = sidebar_clicked_pitch {
                    preview_drum_pad(tracks, selected_track_index, clicked_pitch, 1.0, audio_proxy);
                }

                // --- 5. PLAYHEAD AUDIO TRIGGERING ---
                let current_tick = state.playhead_tick;
                let prev_tick = state.prev_playhead_tick;

                if current_tick != prev_tick {
                    for note in &state.notes {
                        let just_crossed = (prev_tick < note.start_tick || prev_tick > current_tick) 
                            && current_tick >= note.start_tick 
                            && current_tick < note.start_tick + 240;

                        if just_crossed {
                            let velocity_scale = note.velocity as f32 / 127.0;
                            preview_drum_pad(
                                tracks,
                                selected_track_index,
                                note.pitch,
                                velocity_scale,
                                audio_proxy,
                            );
                        }
                    }
                    state.prev_playhead_tick = current_tick;
                }
            });
    });
}

fn note_rect(grid_rect: Rect, note: &MidiNote, zoom_x: f32, key_height: f32) -> Rect {
    let x = grid_rect.min.x + (note.start_tick as f32 * zoom_x);
    let y = grid_rect.min.y + ((127 - note.pitch) as f32 * key_height);
    Rect::from_min_size(
        pos2(x, y),
        vec2(
            (note.duration_ticks as f32 * zoom_x).max(4.0),
            key_height - 1.0,
        ),
    )
}

fn preview_drum_pad(
    tracks: &[Track],
    selected_track_index: usize,
    pitch: u8,
    velocity_scale: f32,
    audio_proxy: &AudioProxy,
) {
    if let Some(track) = tracks.get(selected_track_index) {
        if let Some(opendms) = find_opendms_in_track(track) {
            if let Some(pad) = opendms.pads.iter().find(|p| p.midi_note == pitch) {
                if let Some(ref path) = pad.sample_path {
                    audio_proxy.send(GuiCommand::PreviewSample {
                        path: path.clone(),
                        volume: pad.volume * velocity_scale,
                        speed: 2.0_f32.powf(pad.pitch / 12.0),
                    });
                }
            }
        }
    }
}

fn draw_sidebar(
    ui: &mut Ui,
    sidebar_rect: Rect,
    state: &PianoRollState,
) -> Option<u8> {
    let mut clicked_pitch = None;

    if ui.is_rect_visible(sidebar_rect) {
        let painter = ui.painter_at(sidebar_rect);

        // Fondo del sidebar
        painter.rect_filled(sidebar_rect, 0.0, Color32::from_rgb(25, 25, 28));
        
        // CORREGIDO: uso de right_top() y right_bottom() con sufijo f32 para el Stroke
        painter.line_segment(
            [sidebar_rect.right_top(), sidebar_rect.right_bottom()],
            Stroke::new(1.0_f32, Color32::from_gray(50)),
        );

        for row in 0..128 {
            let pitch = (127 - row) as u8;
            let row_y = sidebar_rect.min.y + (row as f32 * state.key_height);
            let row_rect = Rect::from_min_size(
                pos2(sidebar_rect.min.x, row_y),
                vec2(sidebar_rect.width(), state.key_height),
            );

            if !row_rect.intersects(sidebar_rect) {
                continue;
            }

            let is_black_key = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
            let bg_color = match state.mode {
                PianoRollMode::Keys => {
                    if is_black_key {
                        Color32::from_rgb(40, 40, 45)
                    } else {
                        Color32::from_rgb(220, 220, 225)
                    }
                }
                PianoRollMode::Drums => Color32::from_rgb(35, 35, 40),
            };

            let text_color = match state.mode {
                PianoRollMode::Keys => {
                    if is_black_key {
                        Color32::WHITE
                    } else {
                        Color32::BLACK
                    }
                }
                PianoRollMode::Drums => Color32::from_rgb(200, 200, 200),
            };

            painter.rect_filled(row_rect, 0.0, bg_color);
            painter.rect_stroke(
                row_rect,
                0.0,
                Stroke::new(0.5_f32, Color32::from_gray(60)),
            );

            let label = match state.mode {
                PianoRollMode::Keys => {
                    let note_names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
                    let octave = (pitch / 12) as i32 - 1;
                    format!("{}{}", note_names[(pitch % 12) as usize], octave)
                }
                PianoRollMode::Drums => {
                    if let Some(name) = state.drum_map.get(&pitch) {
                        format!("{} ({})", name, pitch)
                    } else {
                        format!("Pad {}", pitch)
                    }
                }
            };

            painter.text(
                pos2(row_rect.min.x + 6.0, row_rect.center().y),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional((state.key_height * 0.65).clamp(8.0, 12.0)),
                text_color,
            );

            // Interacción de clic en la tecla/pad
            let row_response = ui.interact(
                row_rect,
                ui.id().with(("sidebar_row", pitch)),
                Sense::click(),
            );

            if row_response.clicked() {
                clicked_pitch = Some(pitch);
            }
        }
    }

    clicked_pitch
}

fn draw_grid_background(
    painter: &Painter,
    grid_rect: Rect,
    key_height: f32,
    zoom_x: f32,
) {
    // Fondo de la grilla
    painter.rect_filled(grid_rect, 0.0, Color32::from_rgb(18, 18, 20));

    // Filas horizontales
    for row in 0..128 {
        let pitch = (127 - row) as u8;
        let y = grid_rect.min.y + (row as f32 * key_height);
        let is_black_key = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);

        if is_black_key {
            let row_rect = Rect::from_min_size(
                pos2(grid_rect.min.x, y),
                vec2(grid_rect.width(), key_height),
            );
            painter.rect_filled(row_rect, 0.0, Color32::from_rgba_premultiplied(0, 0, 0, 40));
        }

        painter.line_segment(
            [pos2(grid_rect.min.x, y), pos2(grid_rect.max.x, y)],
            Stroke::new(0.5_f32, Color32::from_gray(30)),
        );
    }

    // Líneas verticales (Subdivisiones de tiempo/ticks)
    let subdivision_ticks = QUANTIZE_TICKS; // 240 ticks (1/16)
    let step_px = subdivision_ticks as f32 * zoom_x;

    if step_px > 3.0 {
        let mut x = grid_rect.min.x;
        let mut tick = 0u64;

        while x < grid_rect.max.x {
            let is_bar = tick % 3840 == 0; // 1 Compás a 960 PPQ
            let is_beat = tick % 960 == 0; // 1 Tiempo (1/4)

            let (stroke_width, color) = if is_bar {
                (1.5_f32, Color32::from_gray(80))
            } else if is_beat {
                (1.0_f32, Color32::from_gray(50))
            } else {
                (0.5_f32, Color32::from_gray(32))
            };

            painter.line_segment(
                [pos2(x, grid_rect.min.y), pos2(x, grid_rect.max.y)],
                Stroke::new(stroke_width, color),
            );

            tick += subdivision_ticks;
            x += step_px;
        }
    }
}

fn get_note_name(pitch: u8) -> String {
    let names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let octave = (pitch / 12) as i8 - 1;
    format!("{}{}", names[(pitch % 12) as usize], octave)
}