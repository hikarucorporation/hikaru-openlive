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
            prev_playhead_tick: 0, // <--- Agregar esta línea
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

// =========================================================================================
// NOTA IMPORTANTE DE ARQUITECTURA / REGISTRO DE NOTAS MIDI:
// Esta función (`show`) gestiona la renderización completa de la grilla del Piano Roll y
// procesa la inserción (Clic Izquierdo) y eliminación (Clic Derecho) de notas MIDI dentro
// del vector `state.notes`, ajustando las coordenadas globales al espacio local de la grilla.
// =========================================================================================
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

                // --- 1. DIBUJO: fondo grilla, notas existentes y playhead ---
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

                    let playhead_x = grid_rect.min.x + (state.playhead_tick as f32 * state.zoom_x);
                    painter.line_segment(
                        [
                            pos2(playhead_x, grid_rect.min.y),
                            pos2(playhead_x, grid_rect.max.y),
                        ],
                        Stroke::new(1.5_f32, Color32::from_rgb(0, 200, 255)),
                    );
                }

                // --- 2. INTERACCIÓN Y CREACIÓN/ELIMINACIÓN DE NOTAS ---
                let grid_response = ui.interact(
                    grid_rect,
                    ui.id().with("piano_roll_grid_interaction"),
                    Sense::click(),
                );

                // Corregimos el offset restando min de grid_rect a la posición absoluta en pantalla
                if let Some(hover_pos) = grid_response.hover_pos() {
                    let local_x = hover_pos.x - grid_rect.min.x;
                    let local_y = hover_pos.y - grid_rect.min.y;

                    if local_x >= 0.0 && local_y >= 0.0 {
                        let raw_tick = (local_x / state.zoom_x).max(0.0) as u64;
                        let quantized_tick = (raw_tick / QUANTIZE_TICKS) * QUANTIZE_TICKS;

                        let row = (local_y / state.key_height).max(0.0) as i32;
                        let pitch = (127 - row).clamp(0, 127) as u8;

                        // Vista previa traslúcida (Ghost preview) sobre la celda actual
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

                // --- 3. SIDEBAR + PREESCUCHA ---
                let sidebar_clicked_pitch = draw_sidebar(ui, sidebar_rect, state);
                if let Some(clicked_pitch) = sidebar_clicked_pitch {
                    preview_drum_pad(tracks, selected_track_index, clicked_pitch, 1.0, audio_proxy);
                }

                // --- 4. PLAYHEAD AUDIO TRIGGERING ---
                let current_tick = state.playhead_tick;
                let prev_tick = state.prev_playhead_tick;

                if current_tick != prev_tick {
                    for note in &state.notes {
                        // La nota solo se dispara SI Y SOLO SI el playhead cruzó su start_tick en este frame
                        let just_crossed = (prev_tick < note.start_tick || prev_tick > current_tick) 
                            && current_tick >= note.start_tick 
                            && current_tick < note.start_tick + 240; // tolerancia razonable de ventana

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

// =========================================================================================
// NOTA IMPORTANTE DE ARQUITECTURA / REGISTRO DE NOTAS MIDI EN EL SIDEBAR:
// Esta función (`draw_sidebar`) renderiza el panel izquierdo con el listado de teclas/pads.
// Permite la interacción directa al hacer clic sobre una nota o pad para emitir el evento
// de preescucha mediante `clicked_pitch`.
// =========================================================================================
fn draw_sidebar(ui: &Ui, rect: Rect, state: &PianoRollState) -> Option<u8> {
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, Color32::from_rgb(20, 20, 24));

    let mut clicked_pitch: Option<u8> = None;

    for pitch in (0..=127).rev() {
        let row = 127 - pitch;
        let y = rect.min.y + (row as f32 * state.key_height);
        let key_rect = Rect::from_min_size(
            pos2(rect.min.x, y),
            vec2(rect.width(), state.key_height),
        );

        if !ui.is_rect_visible(key_rect) {
            continue;
        }

        match state.mode {
            PianoRollMode::Keys => {
                let is_black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
                let bg = if is_black {
                    Color32::from_rgb(30, 30, 35)
                } else {
                    Color32::from_rgb(210, 210, 215)
                };
                let txt = if is_black { Color32::WHITE } else { Color32::BLACK };

                painter.rect_filled(key_rect, 0.0, bg);
                painter.rect_stroke(
                    key_rect,
                    0.0,
                    Stroke::new(0.5_f32, Color32::from_rgb(60, 60, 60)),
                );

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
                let display_name = state
                    .drum_map
                    .get(&pitch)
                    .cloned()
                    .unwrap_or_else(|| format!("Pad {} ({})", pitch, get_note_name(pitch)));

                let has_sample = state.drum_map.contains_key(&pitch);

                let response =
                    ui.interact(key_rect, ui.id().with(("drum_pad_side", pitch)), Sense::click());

                let bg = if response.hovered() && has_sample {
                    Color32::from_rgb(50, 55, 70)
                } else if has_sample {
                    Color32::from_rgb(35, 38, 48)
                } else {
                    Color32::from_rgb(28, 30, 38)
                };

                let label_color = if response.hovered() && has_sample {
                    Color32::from_rgb(220, 240, 255)
                } else if has_sample {
                    Color32::from_rgb(180, 220, 255)
                } else {
                    Color32::from_rgb(90, 95, 110)
                };

                painter.rect_filled(key_rect, 1.0, bg);
                painter.rect_stroke(
                    key_rect,
                    1.0,
                    Stroke::new(0.5_f32, Color32::from_rgb(55, 60, 72)),
                );

                painter.text(
                    pos2(key_rect.min.x + 4.0, key_rect.center().y),
                    Align2::LEFT_CENTER,
                    format!("\u{25B6} {}", display_name),
                    FontId::proportional(10.0),
                    label_color,
                );

                if response.clicked() && has_sample {
                    clicked_pitch = Some(pitch);
                }
            }
        }
    }

    clicked_pitch
}

fn draw_grid_background(painter: &Painter, rect: Rect, key_height: f32, zoom_x: f32) {
    painter.rect_filled(rect, 0.0, Color32::from_rgb(14, 14, 18));

    for i in 0..=128 {
        let y = rect.min.y + (i as f32 * key_height);
        let pitch = 127 - i;
        let is_c = (0..=127).contains(&pitch) && (pitch % 12 == 0);

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

    let px_per_tick = zoom_x;
    let step_ticks = 240.0_f32; // 1/16 note
    let bar_ticks = 3840.0_f32; // 4/4 bar

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