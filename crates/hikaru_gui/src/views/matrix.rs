// Copyright (C) Hikaru Corporation - 2026
// GNU Affero General Public License v3
// Bitwig-style Clip Launcher (OpenLive Dynamic Matrix)
// crates/hikaru_gui/src/views/matrix.rs

use crate::views::clip_editor;

use egui::{
    Align2, Button, Color32, CursorIcon, Frame, Grid, RichText, ScrollArea, Sense, Stroke, Ui, Vec2
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use hikaru_audio_engine::AudioEngine;
use crate::audio_proxy::{AudioProxy, GuiCommand};
pub use crate::views::clipboard::MatrixClipboard;
use crate::views::mixer::Track;
use crate::views::playlist::{self, PlaylistState};

#[derive(Clone, Debug, PartialEq)]
pub enum SlotState {
    Empty,
    Stopped,
    QueuedToPlay,
    Playing,
    QueuedToStop,
}

#[derive(Clone, Debug)]
pub enum ClipData {
    Audio {
        pcm_data: Vec<f32>,
    },
    Midi {
        notes: Vec<(u64, u8, u8, u32)>, // (start_tick, pitch, velocity, duration_ticks)
    },
}

#[derive(Clone, Debug)]
pub struct MatrixClip {
    pub id: usize,
    pub name: String,
    pub path: PathBuf,
    pub duration_secs: f64,
    pub content: ClipData,
    pub local_state: PlaylistState,
    pub local_track: Track,
    pub local_bar: f32,
    pub loop_start: u64,
    pub loop_end: u64,
    pub loop_enabled: bool,
}

impl MatrixClip {
    pub fn pcm_data(&self) -> &[f32] {
        match &self.content {
            ClipData::Audio { pcm_data } => pcm_data,
            _ => &[],
        }
    }

    pub fn pcm_data_mut(&mut self) -> Option<&mut Vec<f32>> {
        match &mut self.content {
            ClipData::Audio { pcm_data } => Some(pcm_data),
            _ => None,
        }
    }

    pub fn loop_length_ticks(&self) -> u64 {
        self.loop_end.saturating_sub(self.loop_start)
    }

    pub fn has_valid_clip_loop(&self) -> bool {
        self.loop_enabled && self.loop_end > self.loop_start
    }
}

#[derive(Clone, Debug)]
pub struct MatrixSlot {
    pub state: SlotState,
    pub clip: Option<MatrixClip>,
}

impl Default for MatrixSlot {
    fn default() -> Self {
        Self {
            state: SlotState::Empty,
            clip: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TrackMeta {
    pub name: String,
    pub muted: bool,
    pub soloed: bool,
    pub volume: f32,
    pub pan: f32,
}

#[derive(Clone, Debug)]
pub struct SceneMeta {
    pub name: String,
}

pub struct SessionMatrixState {
    pub grid: Vec<Vec<MatrixSlot>>,
    pub tracks: Vec<TrackMeta>,
    pub scenes: Vec<SceneMeta>,
    pub next_clip_id: usize,
    pub selected_slot: Option<(usize, usize)>,
    pub editor_height: f32,
    pub editor_zoom_x: f32,
    pub show_editor: bool,
}

impl Default for SessionMatrixState {
    fn default() -> Self {
        let initial_tracks = 8;
        let initial_scenes = 8;

        let tracks = (0..initial_tracks)
            .map(|i| TrackMeta {
                name: format!("Track {}", i + 1),
                muted: false,
                soloed: false,
                volume: 0.75,
                pan: 0.0,
            })
            .collect();

        let scenes = (0..initial_scenes)
            .map(|i| SceneMeta {
                name: format!("Scene {}", i + 1),
            })
            .collect();

        let grid = vec![vec![MatrixSlot::default(); initial_scenes]; initial_tracks];

        Self {
            grid,
            tracks,
            scenes,
            next_clip_id: 1,
            selected_slot: None,
            editor_height: 220.0,
            editor_zoom_x: 0.04,
            show_editor: true,
        }
    }
}

impl SessionMatrixState {
    pub fn add_track(&mut self) {
        let track_num = self.tracks.len() + 1;
        self.tracks.push(TrackMeta {
            name: format!("Audio {}", track_num),
            muted: false,
            soloed: false,
            volume: 0.75,
            pan: 0.0,
        });

        let scene_count = self.scenes.len();
        self.grid.push(vec![MatrixSlot::default(); scene_count]);
    }

    pub fn remove_track(&mut self) {
        if self.tracks.len() > 1 {
            self.tracks.pop();
            self.grid.pop();

            if let Some((t, _)) = self.selected_slot {
                if t >= self.tracks.len() {
                    self.selected_slot = None;
                }
            }
        }
    }

    pub fn add_scene(&mut self) {
        let scene_num = self.scenes.len() + 1;
        self.scenes.push(SceneMeta {
            name: format!("Scene {}", scene_num),
        });

        for track_row in &mut self.grid {
            track_row.push(MatrixSlot::default());
        }
    }

    pub fn remove_scene(&mut self) {
        if self.scenes.len() > 1 {
            self.scenes.pop();
            for track_row in &mut self.grid {
                track_row.pop();
            }

            if let Some((_, s)) = self.selected_slot {
                if s >= self.scenes.len() {
                    self.selected_slot = None;
                }
            }
        }
    }

    pub fn copy_slot(&self, track_idx: usize, scene_idx: usize, clipboard: &mut MatrixClipboard) {
        if let Some(slot) = self.grid.get(track_idx).and_then(|r| r.get(scene_idx)) {
            clipboard.copy(slot);
        }
    }

    pub fn cut_slot(&mut self, track_idx: usize, scene_idx: usize, clipboard: &mut MatrixClipboard, audio_proxy: &AudioProxy) {
        self.copy_slot(track_idx, scene_idx, clipboard);
        self.delete_slot(track_idx, scene_idx, audio_proxy);
    }

    pub fn paste_slot(&mut self, track_idx: usize, scene_idx: usize, clipboard: &MatrixClipboard, audio_proxy: &AudioProxy) {
        if let Some(copied) = &clipboard.copied_slot {
            let mut target_slot = copied.clone();
            target_slot.state = SlotState::Stopped;

            let next_id = self.next_clip_id;
            self.next_clip_id += 1;

            let is_midi = matches!(
                target_slot.clip.as_ref().map(|c| &c.content),
                Some(ClipData::Midi { .. })
            );

            if let Some(clip) = target_slot.clip.as_mut() {
                clip.id = next_id;

                if !is_midi {
                    let path_str = clip.path.to_string_lossy().to_string();
                    audio_proxy.send(GuiCommand::LoadClip {
                        clip_id: next_id,
                        path: path_str,
                        position_secs: 0.0,
                        duration_secs: 0.0,
                        offset_secs: 0.0,
                        track_index: track_idx,
                        scene_index: scene_idx,
                    });
                }
            }

            self.grid[track_idx][scene_idx] = target_slot;
            if is_midi {
                sync_midi_clip(self, audio_proxy, track_idx, scene_idx);
            }
        }
    }

    pub fn duplicate_slot(&mut self, track_idx: usize, scene_idx: usize, audio_proxy: &AudioProxy) {
        let target_scene = (scene_idx + 1).min(self.scenes.len() - 1);
        if target_scene != scene_idx {
            let current_slot = self.grid[track_idx][scene_idx].clone();
            let mut clip_copy = current_slot;
            
            let next_id = self.next_clip_id;
            self.next_clip_id += 1;

            let is_midi = matches!(
                clip_copy.clip.as_ref().map(|c| &c.content),
                Some(ClipData::Midi { .. })
            );

            if let Some(clip) = clip_copy.clip.as_mut() {
                clip.id = next_id;
                if !is_midi {
                    let path_str = clip.path.to_string_lossy().to_string();
                    audio_proxy.send(GuiCommand::LoadClip {
                        clip_id: next_id,
                        path: path_str,
                        position_secs: 0.0,
                        duration_secs: 0.0,
                        offset_secs: 0.0,
                        track_index: track_idx,
                        scene_index: target_scene,
                    });
                }
            }
            self.grid[track_idx][target_scene] = clip_copy;
            if is_midi {
                sync_midi_clip(self, audio_proxy, track_idx, target_scene);
            }
        }
    }

    pub fn delete_slot(&mut self, track_idx: usize, scene_idx: usize, audio_proxy: &AudioProxy) {
        if let Some(slot) = self.grid.get_mut(track_idx).and_then(|r| r.get_mut(scene_idx)) {
            let was_active = matches!(
                slot.state,
                SlotState::Playing | SlotState::QueuedToPlay | SlotState::QueuedToStop
            );
            *slot = MatrixSlot::default();
            if was_active {
                audio_proxy.send(GuiCommand::TriggerClip { track_idx, scene_idx });
            }
        }
    }
}

pub(crate) fn sync_midi_clip(
    state: &SessionMatrixState,
    audio_proxy: &AudioProxy,
    track_idx: usize,
    scene_idx: usize,
) {
    if let Some(ClipData::Midi { notes }) = state
        .grid
        .get(track_idx)
        .and_then(|row| row.get(scene_idx))
        .and_then(|slot| slot.clip.as_ref())
        .map(|clip| &clip.content)
    {
        audio_proxy.send(GuiCommand::UpdateMidiClipNotes {
            track_idx,
            scene_idx,
            notes: notes.clone(),
        });
    }
}

pub(crate) fn trigger_pad(state: &mut SessionMatrixState, audio_proxy: &AudioProxy, track_idx: usize, scene_idx: usize) {
    let has_clip = state
        .grid
        .get(track_idx)
        .and_then(|row| row.get(scene_idx))
        .map_or(false, |slot| slot.clip.is_some());

    if !has_clip {
        return;
    }

    let current_state = state.grid[track_idx][scene_idx].state.clone();

    match current_state {
        SlotState::Stopped => {
            for s in 0..state.scenes.len() {
                if s != scene_idx && state.grid[track_idx][s].state == SlotState::Playing {
                    state.grid[track_idx][s].state = SlotState::Stopped;
                }
            }

            sync_midi_clip(state, audio_proxy, track_idx, scene_idx);

            if let Some(slot) = state.grid.get_mut(track_idx).and_then(|row| row.get_mut(scene_idx)) {
                slot.state = SlotState::Playing;
                if let Some(clip) = slot.clip.as_mut() {
                    clip.local_bar = 1.0;
                }
            }

            audio_proxy.send(GuiCommand::TriggerClip {
                track_idx,
                scene_idx,
            });
        }
        SlotState::Playing | SlotState::QueuedToPlay | SlotState::QueuedToStop => {
            if let Some(slot) = state.grid.get_mut(track_idx).and_then(|row| row.get_mut(scene_idx)) {
                slot.state = SlotState::Stopped;
            }

            audio_proxy.send(GuiCommand::TriggerClip {
                track_idx,
                scene_idx,
            });
        }
        _ => {}
    }
}

pub(crate) fn trigger_scene(state: &mut SessionMatrixState, audio_proxy: &AudioProxy, scene_idx: usize) {
    for track_idx in 0..state.tracks.len() {
        if state.grid[track_idx][scene_idx].clip.is_some() {
            for s in 0..state.scenes.len() {
                if s != scene_idx && state.grid[track_idx][s].state == SlotState::Playing {
                    state.grid[track_idx][s].state = SlotState::Stopped;
                }
            }
            state.grid[track_idx][scene_idx].state = SlotState::Playing;
            if let Some(clip) = state.grid[track_idx][scene_idx].clip.as_mut() {
                clip.local_bar = 1.0;
            }
            sync_midi_clip(state, audio_proxy, track_idx, scene_idx);
        }
    }

    audio_proxy.send(GuiCommand::TriggerScene { scene_idx });
}

pub fn poll_finished_voices<F>(state: &mut SessionMatrixState, is_active: F) -> usize 
where
    F: Fn(usize, usize) -> bool,
{
    let mut deactivated = 0;
    for track_idx in 0..state.tracks.len() {
        for scene_idx in 0..state.scenes.len() {
            if state.grid[track_idx][scene_idx].state == SlotState::Playing {
                let slot = &state.grid[track_idx][scene_idx];
                let is_midi = matches!(
                    slot.clip.as_ref().map(|c| &c.content),
                    Some(ClipData::Midi { .. })
                );

                // Los clips MIDI no dependen del bus de voces PCM, se mantienen activos en GUI
                // a menos que el usuario los detenga explícitamente o cambie de escena.
                if !is_midi && !is_active(track_idx, scene_idx) {
                    state.grid[track_idx][scene_idx].state = SlotState::Stopped;
                    deactivated += 1;
                }
            }
        }
    }
    deactivated
}

pub fn poll_engine_slots(state: &mut SessionMatrixState, engine: &AudioEngine) -> usize {
    poll_finished_voices(state, |track_idx, scene_idx| {
        engine.voice_active(track_idx, scene_idx)
    })
}

pub fn show(
    ui: &mut Ui,
    state: &mut SessionMatrixState,
    clipboard: &mut MatrixClipboard,
    dragged_sample: &mut Option<PathBuf>,
    audio_proxy: &AudioProxy,
    _current_tick: u64,
    _ppqn: u64,
    bpm: f64,
    sample_rate: u32,
    transport_sample_count: u64,
    global_loop_enabled: bool,
    global_loop_start_ticks: u64,
    global_loop_end_ticks: u64,
    engine_handle: Option<&Arc<Mutex<AudioEngine<'static>>>>,
) {
    if let Some(handle) = engine_handle {
        if let Ok(engine) = handle.try_lock() {
            let deactivated = poll_engine_slots(state, &engine);
            if deactivated > 0 {
                ui.ctx().request_repaint();
            }
        } else {
            ui.ctx().request_repaint();
        }
    }

    ui.horizontal(|ui| {
        ui.heading("SESSION MATRIX");
        ui.add_space(20.0);

        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(format!("Pistas (Tracks): {}", state.tracks.len()));
                if ui.button("➕").on_hover_text("Añadir Pista").clicked() {
                    state.add_track();
                }
                if ui.button("➖").on_hover_text("Quitar Pista").clicked() {
                    state.remove_track();
                }
            });
        });

        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(format!("Escenas (Scenes): {}", state.scenes.len()));
                if ui.button("➕").on_hover_text("Añadir Escena").clicked() {
                    state.add_scene();
                }
                if ui.button("➖").on_hover_text("Quitar Escena").clicked() {
                    state.remove_scene();
                }
            });
        });
    });

    let any_clip_playing = state.grid.iter().flatten()
        .any(|slot| slot.state == SlotState::Playing);
    if any_clip_playing {
        ui.ctx().request_repaint();
    }

    ui.add_space(6.0);

    ui.vertical(|ui| {
        let remaining_height = if state.show_editor {
            let editor_h = state.editor_height + 10.0;
            (ui.available_height() - editor_h).max(100.0)
        } else {
            ui.available_height()
        };

        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), remaining_height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ScrollArea::both().show(ui, |ui| {
                    Grid::new("bitwig_clip_launcher_grid")
                        .spacing(Vec2::new(4.0, 4.0))
                        .show(ui, |ui| {
                            ui.label("");

                            for scene_idx in 0..state.scenes.len() {
                                let scene_name = &state.scenes[scene_idx].name;
                                let scene_btn = Button::new(format!("▶ {}", scene_name))
                                    .fill(Color32::from_rgb(45, 45, 55))
                                    .min_size(Vec2::new(110.0, 28.0));

                                if ui.add(scene_btn).clicked() {
                                    trigger_scene(state, audio_proxy, scene_idx);
                                }
                            }

                            if ui.button("➕").on_hover_text("Añadir nueva Escena").clicked() {
                                state.add_scene();
                            }

                            ui.end_row();

                            for track_idx in 0..state.tracks.len() {
                                Frame::none()
                                    .fill(Color32::from_rgb(28, 28, 32))
                                    .stroke(Stroke::new(1.0_f32, Color32::from_gray(45)))
                                    .inner_margin(4.0)
                                    .show(ui, |ui| {
                                        ui.set_min_size(Vec2::new(126.0, 50.0));
                                        ui.vertical(|ui| {
                                            ui.horizontal(|ui| {
                                                let text_width = 70.0_f32;
                                                ui.add(
                                                    egui::TextEdit::singleline(&mut state.tracks[track_idx].name)
                                                        .text_color(Color32::WHITE)
                                                        .font(egui::FontId::proportional(11.0))
                                                        .frame(false)
                                                        .desired_width(text_width),
                                                );

                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                    let solo_btn_text = if state.tracks[track_idx].soloed {
                                                        RichText::new("S").strong().color(Color32::from_rgb(255, 200, 0))
                                                    } else {
                                                        RichText::new("S").color(Color32::from_gray(160))
                                                    };
                                                    if ui.toggle_value(&mut state.tracks[track_idx].soloed, solo_btn_text).clicked() {
                                                        audio_proxy.send(GuiCommand::SetTrackSolo {
                                                            track_idx,
                                                            solo: state.tracks[track_idx].soloed,
                                                        });
                                                    }

                                                    let mute_btn_text = if state.tracks[track_idx].muted {
                                                        RichText::new("M").strong().color(Color32::from_rgb(255, 80, 80))
                                                    } else {
                                                        RichText::new("M").color(Color32::from_gray(160))
                                                    };
                                                    if ui.toggle_value(&mut state.tracks[track_idx].muted, mute_btn_text).clicked() {
                                                        audio_proxy.send(GuiCommand::SetTrackMute {
                                                            track_idx,
                                                            mute: state.tracks[track_idx].muted,
                                                        });
                                                    }
                                                });
                                            });

                                            ui.add_space(2.0);

                                            ui.horizontal(|ui| {
                                                playlist::knob_ui(ui, &mut state.tracks[track_idx].pan, 6.0);

                                                let pan_text = if state.tracks[track_idx].pan < -1.0 {
                                                    format!("L{:.0}", state.tracks[track_idx].pan.abs())
                                                } else if state.tracks[track_idx].pan > 1.0 {
                                                    format!("R{:.0}", state.tracks[track_idx].pan)
                                                } else {
                                                    "C".to_string()
                                                };
                                                ui.label(RichText::new(pan_text).size(9.0).color(Color32::from_rgb(0, 255, 255)));

                                                ui.add_space(2.0);

                                                let slider_width = 38.0_f32;
                                                playlist::custom_h_slider(ui, &mut state.tracks[track_idx].volume, slider_width);

                                                let db_val = if state.tracks[track_idx].volume <= 0.0 {
                                                    -60.0
                                                } else if state.tracks[track_idx].volume <= 0.75 {
                                                    -60.0 + (state.tracks[track_idx].volume / 0.75) * 60.0
                                                } else {
                                                    ((state.tracks[track_idx].volume - 0.75) / 0.25) * 6.0
                                                };
                                                ui.label(RichText::new(format!("{:.1}dB", db_val)).size(8.5).weak());
                                            });
                                        });
                                    });

                                for scene_idx in 0..state.scenes.len() {
                                    render_pad(
                                        ui,
                                        state,
                                        clipboard,
                                        dragged_sample,
                                        audio_proxy,
                                        track_idx,
                                        scene_idx,
                                        bpm,
                                        sample_rate,
                                        transport_sample_count, // Pasa el contador global de muestras
                                        engine_handle,
                                    );
                                }

                                ui.end_row();
                            }

                            if ui.button("➕ Añadir Pista").clicked() {
                                state.add_track();
                            }
                            ui.end_row();
                        });
                });
            },
        );

        if state.show_editor {
            let (resizer_rect, resizer_response) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), 6.0),
                Sense::drag(),
            );

            if resizer_response.hovered() || resizer_response.dragged() {
                ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeVertical);
            }

            if resizer_response.dragged() {
                let delta_y = resizer_response.drag_delta().y;
                state.editor_height = (state.editor_height - delta_y).clamp(80.0, 500.0);
            }

            ui.painter().rect_filled(resizer_rect, 0.0, Color32::from_gray(35));
            if resizer_response.hovered() || resizer_response.dragged() {
                ui.painter().rect_filled(resizer_rect, 0.0, Color32::from_rgb(0, 255, 255));
            }

            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), state.editor_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    render_clip_editor_track_view(
                        ui,
                        state,
                        dragged_sample,
                        audio_proxy,
                        bpm,
                        sample_rate,
                        transport_sample_count,
                        _ppqn,
                        global_loop_enabled,
                        global_loop_start_ticks,
                        global_loop_end_ticks,
                        engine_handle,
                    );
                },
            );
        }
    });
}

fn draw_mini_waveform(
    painter: &egui::Painter,
    rect: egui::Rect,
    pcm_data: &[f32],
    color: Color32,
) {
    if pcm_data.is_empty() {
        return;
    }

    let width = rect.width() as usize;
    if width == 0 {
        return;
    }

    let samples_per_pixel = (pcm_data.len() / width).max(1);
    let center_y = rect.center().y;
    let max_h = rect.height() * 0.38;

    for x in 0..width {
        let start = x * samples_per_pixel;
        let end = ((x + 1) * samples_per_pixel).min(pcm_data.len());
        if start >= pcm_data.len() {
            break;
        }

        let mut max_val = 0.0_f32;
        for &s in &pcm_data[start..end] {
            let abs_s = s.abs();
            if abs_s > max_val {
                max_val = abs_s;
            }
        }

        let line_h = (max_val * max_h).max(1.0);
        let px = rect.min.x + x as f32;

        painter.line_segment(
            [
                egui::pos2(px, center_y - line_h),
                egui::pos2(px, center_y + line_h),
            ],
            Stroke::new(1.0_f32, color),
        );
    }
}

fn draw_mini_midi_notes(
    painter: &egui::Painter,
    rect: egui::Rect,
    notes: &[(u64, u8, u8, u32)],
    color: Color32,
) {
    if notes.is_empty() {
        return;
    }

    let max_tick = notes.iter().map(|(s, _, _, d)| s + *d as u64).max().unwrap_or(1920) as f32;
    let min_pitch = notes.iter().map(|(_, p, _, _)| *p).min().unwrap_or(36) as f32;
    let max_pitch = notes.iter().map(|(_, p, _, _)| *p).max().unwrap_or(84) as f32;

    let pitch_range = (max_pitch - min_pitch).max(12.0);
    let pad_w = rect.width();
    let pad_h = rect.height();

    for &(start_tick, pitch, _vel, duration_ticks) in notes {
        let x_norm = start_tick as f32 / max_tick;
        let w_norm = (duration_ticks as f32 / max_tick).max(0.02);
        let y_norm = 1.0 - ((pitch as f32 - min_pitch) / pitch_range).clamp(0.0, 1.0);

        let x = rect.min.x + (x_norm * pad_w);
        let y = rect.min.y + (y_norm * (pad_h - 4.0));
        let note_w = (w_norm * pad_w).max(2.0);
        let note_h = 2.5_f32;

        let note_rect = egui::Rect::from_min_size(
            egui::pos2(x, y),
            egui::vec2(note_w, note_h),
        );

        if note_rect.intersects(rect) {
            painter.rect_filled(note_rect, 0.5, color);
        }
    }
}

fn render_pad(
    ui: &mut Ui,
    state: &mut SessionMatrixState,
    clipboard: &mut MatrixClipboard,
    dragged_sample: &mut Option<PathBuf>,
    audio_proxy: &AudioProxy,
    track_idx: usize,
    scene_idx: usize,
    bpm: f64,
    sample_rate: u32,
    transport_sample_count: u64,
    engine_handle: Option<&Arc<Mutex<AudioEngine<'static>>>>,
) {
    let slot = &state.grid[track_idx][scene_idx];
    let is_selected = state.selected_slot == Some((track_idx, scene_idx));
    let has_clip = slot.clip.is_some();

    let (bg_color, mut border_color, text) = if !has_clip {
        (Color32::from_gray(25), Color32::from_gray(40), "".to_string())
    } else {
        match &slot.state {
            SlotState::Stopped => (
                Color32::from_rgb(45, 55, 75),
                Color32::from_rgb(90, 130, 190),
                slot.clip.as_ref().map(|c| c.name.clone()).unwrap_or_default(),
            ),
            SlotState::QueuedToPlay => (
                Color32::from_rgb(120, 100, 30),
                Color32::YELLOW,
                format!("⌛ {}", slot.clip.as_ref().map(|c| &c.name).unwrap_or(&"".into())),
            ),
            SlotState::Playing => (
                Color32::from_rgb(35, 135, 60),
                Color32::GREEN,
                format!("▶ {}", slot.clip.as_ref().map(|c| &c.name).unwrap_or(&"".into())),
            ),
            SlotState::QueuedToStop => (
                Color32::from_rgb(130, 45, 45),
                Color32::RED,
                "⏹ Stop".to_string(),
            ),
            SlotState::Empty => (Color32::from_gray(25), Color32::from_gray(40), "".to_string()),
        }
    };

    if is_selected {
        border_color = Color32::WHITE;
    }

    let size = Vec2::new(110.0, 54.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(rect, 3.0, bg_color);

        if let Some(clip) = &slot.clip {
            let inner_rect = rect.shrink(2.0);
            let clipped_painter = ui.painter().with_clip_rect(inner_rect);

            match &clip.content {
                ClipData::Audio { pcm_data } => {
                    let wave_color = match slot.state {
                        SlotState::Playing => Color32::from_rgba_unmultiplied(5, 25, 10, 220),
                        SlotState::QueuedToPlay => Color32::from_rgba_unmultiplied(40, 30, 5, 200),
                        _ => Color32::from_rgba_unmultiplied(120, 160, 220, 180),
                    };
                    draw_mini_waveform(&clipped_painter, inner_rect, pcm_data, wave_color);
                }
                ClipData::Midi { notes } => {
                    let note_color = match slot.state {
                        SlotState::Playing => Color32::from_rgb(0, 255, 150),
                        _ => Color32::from_rgb(255, 180, 50),
                    };

                    if notes.is_empty() {
                        clipped_painter.text(
                            inner_rect.left_bottom() + egui::vec2(4.0, -4.0),
                            Align2::LEFT_BOTTOM,
                            "🎹 MIDI (Vacío)",
                            egui::FontId::proportional(9.0),
                            Color32::from_gray(120),
                        );
                    } else {
                        draw_mini_midi_notes(&clipped_painter, inner_rect, notes, note_color);
                    }
                }
            }

            if slot.state == SlotState::Playing {
                let mut play_progress: Option<f32> = None;

                // 1. Intentar consultar las muestras transcurridas directas del motor
                let elapsed_frames = engine_handle.and_then(|handle| {
                    handle.try_lock().ok()?.voice_elapsed_frames(track_idx, scene_idx)
                });

                if let Some(frames) = elapsed_frames {
                    play_progress = match &clip.content {
                        ClipData::Audio { pcm_data } if !pcm_data.is_empty() => {
                            Some((frames as f32 / pcm_data.len() as f32).clamp(0.0, 1.0))
                        }
                        ClipData::Midi { notes } => {
                            let max_ticks = notes
                                .iter()
                                .map(|(s, _, _, d)| s + *d as u64)
                                .max()
                                .unwrap_or(1920);
                            let samples_per_tick = (sample_rate as f64 * 60.0) / (bpm * 960.0);
                            let total_clip_frames = (max_ticks as f64 * samples_per_tick) as u64;

                            if total_clip_frames > 0 {
                                Some(((frames % total_clip_frames) as f32 / total_clip_frames as f32).clamp(0.0, 1.0))
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                }

                // 2. Sincronización exacta con el reloj/transporte de audio (BPM + Sample Rate)
                if play_progress.is_none() {
                    let samples_per_beat = (sample_rate as f64 * 60.0) / bpm.max(1.0);
                    let clip_length_samples = match &clip.content {
                        ClipData::Audio { pcm_data } if !pcm_data.is_empty() => pcm_data.len() as f64,
                        ClipData::Midi { notes } => {
                            let max_ticks = notes
                                .iter()
                                .map(|(s, _, _, d)| s + *d as u64)
                                .max()
                                .unwrap_or(1920);
                            (max_ticks as f64 / 960.0) * (samples_per_beat * 4.0) // 4 beats por compás
                        }
                        _ => samples_per_beat * 4.0,
                    };

                    if clip_length_samples > 0.0 {
                        let current_frame = transport_sample_count as f64;
                        let progress = (current_frame % clip_length_samples) / clip_length_samples;
                        play_progress = Some(progress as f32);
                    }
                }

                // Renderizado de la aguja de reproducción (Playhead)
                if let Some(progress) = play_progress {
                    let playhead_x = inner_rect.min.x + (inner_rect.width() * progress);

                    clipped_painter.line_segment(
                        [
                            egui::pos2(playhead_x, inner_rect.min.y),
                            egui::pos2(playhead_x, inner_rect.max.y),
                        ],
                        Stroke::new(1.5_f32, Color32::WHITE),
                    );
                }
            }
        }

        let stroke_width = if is_selected { 2.0_f32 } else { 1.0_f32 };
        ui.painter().rect_stroke(rect, 3.0, Stroke::new(stroke_width, border_color));

        if !text.is_empty() {
            let display_text = if text.len() > 14 {
                format!("{}...", &text[..11])
            } else {
                text
            };

            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                display_text,
                egui::FontId::proportional(11.0),
                Color32::WHITE,
            );
        }
    }

    // Context menu, drags & clicks...
    // (Mantener el resto del método context_menu y click handlers como los tenías)

    response.context_menu(|ui| {
        ui.style_mut().spacing.button_padding = Vec2::new(8.0, 4.0);

        ui.menu_button("➕ Insertar", |ui| {
            if ui.button("🎵 Clip de Audio...").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Audio Files", &["wav", "mp3", "flac", "ogg"])
                    .pick_file() 
                {
                    state.selected_slot = Some((track_idx, scene_idx));
                    load_clip_into_slot(state, audio_proxy, track_idx, scene_idx, path, bpm);
                }
                ui.close_menu();
            }

            ui.menu_button("🎹 MIDI Clip...", |ui| {
                if ui.button("✨ Nuevo MIDI Clip").clicked() {
                    let new_id = state.next_clip_id;
                    state.next_clip_id += 1;

                    state.grid[track_idx][scene_idx] = MatrixSlot {
                        state: SlotState::Stopped,
                        clip: Some(MatrixClip {
                            id: new_id,
                            name: "Nuevo MIDI".to_string(),
                            path: PathBuf::new(),
                            duration_secs: 4.0,
                            content: ClipData::Midi { notes: Vec::new() },
                            local_state: PlaylistState::default(),
                            local_track: Track::new(0, "Nuevo MIDI".to_string(), false),
                            local_bar: 1.0,
                            loop_start: 0,
                            loop_end: 1920,
                            loop_enabled: true,
                        }),
                    };
                    state.selected_slot = Some((track_idx, scene_idx));

                    audio_proxy.send(GuiCommand::LoadMidiClip {
                        clip_id: new_id,
                        track_index: track_idx,
                        scene_index: scene_idx,
                        notes: vec![],
                    });

                    ui.close_menu();
                }

                if ui.button("📂 Abrir MIDI Clip (.mid/.midi)").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Archivos MIDI", &["mid", "midi"])
                        .pick_file() 
                    {
                        state.selected_slot = Some((track_idx, scene_idx));
                        load_clip_into_slot(state, audio_proxy, track_idx, scene_idx, path, bpm);
                    }
                    ui.close_menu();
                }
            });
        });

        ui.separator();

        if ui.add_enabled(has_clip, Button::new("📋 Copiar")).clicked() {
            state.copy_slot(track_idx, scene_idx, clipboard);
            ui.close_menu();
        }
        if ui.add_enabled(has_clip, Button::new("✂ Cortar")).clicked() {
            state.cut_slot(track_idx, scene_idx, clipboard, audio_proxy);
            ui.close_menu();
        }
        if ui.add_enabled(clipboard.has_content(), Button::new("📥 Pegar")).clicked() {
            state.paste_slot(track_idx, scene_idx, clipboard, audio_proxy);
            ui.close_menu();
        }
        ui.separator();
        if ui.add_enabled(has_clip, Button::new("📑 Duplicar")).clicked() {
            state.duplicate_slot(track_idx, scene_idx, audio_proxy);
            ui.close_menu();
        }
        if ui.add_enabled(has_clip, Button::new("🗑 Eliminar")).clicked() {
            state.delete_slot(track_idx, scene_idx, audio_proxy);
            ui.close_menu();
        }
    });
    
    if response.clicked() {
        state.selected_slot = Some((track_idx, scene_idx));
        trigger_pad(state, audio_proxy, track_idx, scene_idx);
    }

    if ui.rect_contains_pointer(rect) {
        ui.output_mut(|o| o.cursor_icon = CursorIcon::PointingHand);

        if ui.input(|i| i.pointer.any_released()) {
            if let Some(sample_path) = dragged_sample.take() {
                if crate::views::explorer::is_supported_file(&sample_path) {
                    state.selected_slot = Some((track_idx, scene_idx));
                    load_clip_into_slot(state, audio_proxy, track_idx, scene_idx, sample_path, bpm);
                }
            }
        }

        let dropped_files = ui.input(|i| i.raw.dropped_files.clone());
        if let Some(file) = dropped_files.first() {
            if let Some(path) = &file.path {
                if crate::views::explorer::is_supported_file(path) {
                    state.selected_slot = Some((track_idx, scene_idx));
                    load_clip_into_slot(state, audio_proxy, track_idx, scene_idx, path.clone(), bpm);
                }
            }
        }
    }
}

fn render_clip_editor_track_view(
    ui: &mut Ui,
    state: &mut SessionMatrixState,
    _dragged_sample: &mut Option<PathBuf>,
    _audio_proxy: &AudioProxy,
    bpm: f64,
    sample_rate: u32,
    _transport_sample_count: u64,
    _ppqn: u64,
    _global_loop_enabled: bool,
    _global_loop_start_ticks: u64,
    _global_loop_end_ticks: u64,
    engine_handle: Option<&Arc<Mutex<AudioEngine<'static>>>>,
) {
    Frame::none()
        .fill(Color32::from_rgb(20, 20, 24))
        .stroke(Stroke::new(1.0_f32, Color32::from_gray(45)))
        .show(ui, |ui| {
            let Some((track_idx, scene_idx)) = state.selected_slot else {
                ui.centered_and_justified(|ui| {
                    ui.label("Seleccioná un clip de la matriz para desplegar su Clip Editor.");
                });
                return;
            };

            if track_idx >= state.grid.len() || scene_idx >= state.grid[track_idx].len() {
                return;
            }

            let Some(slot) = &mut state.grid[track_idx][scene_idx].clip else {
                ui.horizontal(|ui| {
                    ui.heading("CLIP EDITOR");
                    ui.label(format!(
                        "- {} | {}",
                        state.tracks[track_idx].name, state.scenes[scene_idx].name
                    ));
                });
                ui.separator();
                ui.centered_and_justified(|ui| {
                    ui.label("Slot vacío. Arrastrá un sample o MIDI para crear un Clip.");
                });
                return;
            };

            let elapsed_frames = (|| {
                let handle = engine_handle?;
                let engine = handle.try_lock().ok()?;
                engine.voice_elapsed_frames(track_idx, scene_idx)
            })();

            if elapsed_frames.is_some() {
                ui.ctx().request_repaint();
            }

            match &mut slot.content {
                ClipData::Audio { .. } => {
                    clip_editor::show(
                        ui,
                        slot,
                        elapsed_frames,
                        sample_rate,
                        bpm as f32,
                    );
                }
                ClipData::Midi { notes } => {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("🎹 PIANO ROLL EDITOR")
                                    .strong()
                                    .color(Color32::from_rgb(255, 180, 0)),
                            );
                            ui.separator();
                            ui.label(format!("Notas registradas: {}", notes.len()));
                        });
                        ui.separator();

                        ScrollArea::both().show(ui, |ui| {
                            ui.set_min_size(Vec2::new(ui.available_width(), 120.0));
                            if notes.is_empty() {
                                ui.centered_and_justified(|ui| {
                                    ui.label("Clip MIDI sin notas. Agregá notas desde la vista Piano Roll principal.");
                                });
                            } else {
                                Grid::new("mini_piano_roll_grid")
                                    .striped(true)
                                    .show(ui, |ui| {
                                        ui.label(RichText::new("Tick").strong());
                                        ui.label(RichText::new("Pitch (Nota)").strong());
                                        ui.label(RichText::new("Velocidad").strong());
                                        ui.label(RichText::new("Duración (Ticks)").strong());
                                        ui.end_row();

                                        for (tick, pitch, vel, dur) in notes.iter() {
                                            ui.label(tick.to_string());
                                            ui.label(format!("{} ({})", pitch, pitch_to_note_name(*pitch)));
                                            ui.label(vel.to_string());
                                            ui.label(dur.to_string());
                                            ui.end_row();
                                        }
                                    });
                            }
                        });
                    });
                }
            }
        });
}

fn pitch_to_note_name(pitch: u8) -> &'static str {
    let names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    names[(pitch % 12) as usize]
}

fn load_clip_into_slot(
    state: &mut SessionMatrixState,
    audio_proxy: &AudioProxy,
    track_idx: usize,
    scene_idx: usize,
    path: PathBuf,
    _bpm: f64,
) {
    let name = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let new_id = state.next_clip_id;
    state.next_clip_id += 1;

    let path_str = path.to_string_lossy().to_string();
    let is_midi = crate::views::explorer::is_midi_file(&path);

    let content = if is_midi {
        ClipData::Midi { notes: Vec::new() }
    } else {
        ClipData::Audio { pcm_data: Vec::new() }
    };

    let mut local_state = PlaylistState::default();
    local_state.zoom_x = state.editor_zoom_x;

    state.grid[track_idx][scene_idx] = MatrixSlot {
        state: SlotState::Stopped,
        clip: Some(MatrixClip {
            id: new_id,
            name: name.clone(),
            path,
            duration_secs: 0.0,
            content,
            local_state,
            local_track: Track::new(0, name, false),
            local_bar: 1.0,
            loop_start: 0,
            loop_end: 0,
            loop_enabled: false,
        }),
    };

    if is_midi {
        sync_midi_clip(state, audio_proxy, track_idx, scene_idx);
    } else {
        audio_proxy.send(GuiCommand::LoadClip {
            clip_id: new_id,
            path: path_str,
            position_secs: 0.0,
            duration_secs: 0.0,
            offset_secs: 0.0,
            track_index: track_idx,
            scene_index: scene_idx,
        });
    }
}