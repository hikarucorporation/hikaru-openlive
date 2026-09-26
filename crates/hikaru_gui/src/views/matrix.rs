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

// use crate::audio::AudioProxy; // esto iba en el clip editor kjj

use crate::audio_proxy::{AudioProxy, GuiCommand};

// use crate::views::matrix::{self, MatrixClip, SessionMatrixState}; // Lo mismo que lo del audio proxy xd

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
pub struct AudioEvent {
    pub id: u64,
    pub name: String,
    /// Buffer ORIGINAL completo. El trim es no-destructivo: este buffer
    /// nunca se muta, solo la ventana visible (se puede re-estirar).
    pub samples: Vec<f32>,
    pub channels: usize,
    pub sample_rate: u32,
    /// Posición dentro del timeline del pad (segundos).
    pub start_secs: f64,
    /// Recorte no-destructivo: frames ocultos al inicio del buffer.
    pub trim_left_frames: usize,
    /// Frames visibles a partir de `trim_left_frames`.
    pub visible_frames: usize,
    pub gain: f32,
    pub fade_in_secs: f64,
    pub fade_out_secs: f64,
}

impl AudioEvent {
    /// Constructor con ventana completa (sin recorte).
    pub fn new_full(
        id: u64,
        name: String,
        samples: Vec<f32>,
        channels: usize,
        sample_rate: u32,
        start_secs: f64,
    ) -> Self {
        let visible = samples.len() / channels.max(1);
        Self {
            id,
            name,
            samples,
            channels,
            sample_rate,
            start_secs,
            trim_left_frames: 0,
            visible_frames: visible,
            gain: 1.0,
            fade_in_secs: 0.0,
            fade_out_secs: 0.0,
        }
    }

    /// Frames totales del buffer original.
    pub fn total_frames(&self) -> usize {
        self.samples.len() / self.channels.max(1)
    }

    /// Frames visibles (ventana de trim aplicada).
    pub fn frames(&self) -> u64 {
        let avail = self
            .total_frames()
            .saturating_sub(self.trim_left_frames);
        avail.min(self.visible_frames) as u64
    }

    /// Ventana visible del buffer (interleaved, lista para motor/preview).
    pub fn window_samples(&self) -> &[f32] {
        let ch = self.channels.max(1);
        let start = (self.trim_left_frames * ch).min(self.samples.len());
        let end = (start + self.frames() as usize * ch).min(self.samples.len());
        &self.samples[start..end]
    }

    /// Dónde empieza el contenido original en el timeline (para re-estirar).
    pub fn content_start_secs(&self) -> f64 {
        if self.sample_rate == 0 {
            return self.start_secs;
        }
        self.start_secs - self.trim_left_frames as f64 / self.sample_rate as f64
    }

    /// Dónde termina el contenido original en el timeline.
    pub fn content_end_secs(&self) -> f64 {
        if self.sample_rate == 0 {
            return self.end_secs();
        }
        self.content_start_secs() + self.total_frames() as f64 / self.sample_rate as f64
    }

    pub fn duration_secs(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.frames() as f64 / self.sample_rate as f64
    }

    pub fn end_secs(&self) -> f64 {
        self.start_secs + self.duration_secs()
    }

    pub fn contains(&self, t: f64) -> bool {
        t >= self.start_secs && t < self.end_secs()
    }

    /// Divide el evento en `at_secs` (tiempo del timeline). Retorna la mitad
    /// derecha (la izquierda muta in-place). `None` si cae fuera.
    /// No-destructivo: ambas mitades comparten el buffer original.
    pub fn split_at(&mut self, at_secs: f64, new_id: u64, new_name: String) -> Option<AudioEvent> {
        if at_secs <= self.start_secs || at_secs >= self.end_secs() || self.sample_rate == 0 {
            return None;
        }
        let cut_frames = ((at_secs - self.start_secs) * self.sample_rate as f64).round() as usize;
        let left_visible = self.frames() as usize;
        if cut_frames == 0 || cut_frames >= left_visible {
            return None;
        }
        let abs_cut = self.trim_left_frames + cut_frames;
        let right = AudioEvent {
            id: new_id,
            name: new_name,
            samples: self.samples.clone(),
            channels: self.channels,
            sample_rate: self.sample_rate,
            start_secs: at_secs,
            trim_left_frames: abs_cut,
            visible_frames: left_visible - cut_frames,
            gain: self.gain,
            fade_in_secs: 0.0,
            fade_out_secs: self.fade_out_secs,
        };
        self.visible_frames = cut_frames;
        self.fade_out_secs = 0.0;
        Some(right)
    }

    pub fn mono_mixed(&self) -> Vec<f32> {
        let ch = self.channels.max(1);
        let window = self.window_samples();
        if ch == 1 {
            return window.to_vec();
        }
        window
            .chunks(ch)
            .map(|c| c.iter().sum::<f32>() / c.len() as f32)
            .collect()
    }
}

#[derive(Clone, Debug)]
pub enum ClipData {
    Audio {
        /// Eventos (regiones) dentro del pad. Un pad recién cargado tiene
        /// un solo evento; el editor permite dividir, duplicar y apilar
        /// varios (overlaps se mezclan por suma en el motor).
        events: Vec<AudioEvent>,
        next_event_id: u64,
        /// PCM mono mezclado para mini-waveforms (caché, se regenera).
        preview_mix: Vec<f32>,
        preview_sr: u32,
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
    /// Time Selection visible (región de loop). Con Ctrl+Alt+A se oculta,
    /// con Ctrl+A se restaura al clip completo. Por defecto: true.
    pub has_time_selection: bool,
}

impl MatrixClip {
    /// Mezcla mono de todos los eventos (para waveforms y previsualización).
    /// Usa la caché `preview_mix` si sigue vigente, si no la regenera.
    pub fn pcm_data(&self) -> &[f32] {
        match &self.content {
            ClipData::Audio { preview_mix, .. } => preview_mix,
            _ => &[],
        }
    }

    pub fn pcm_data_mut(&mut self) -> Option<&mut Vec<f32>> {
        // Legacy: acceso directo al mix de previsualización (solo lectura
        // real; la edición debe usar `audio_events_mut` + `refresh_preview`).
        match &mut self.content {
            ClipData::Audio { preview_mix, .. } => Some(preview_mix),
            _ => None,
        }
    }

    pub fn audio_events(&self) -> &[AudioEvent] {
        match &self.content {
            ClipData::Audio { events, .. } => events,
            _ => &[],
        }
    }

    pub fn audio_events_mut(&mut self) -> Option<&mut Vec<AudioEvent>> {
        match &mut self.content {
            ClipData::Audio { events, .. } => Some(events),
            _ => None,
        }
    }

    /// Duración del pad = fin del evento más lejano.
    pub fn audio_total_secs(&self) -> f64 {
        match &self.content {
            ClipData::Audio { events, .. } => events
                .iter()
                .map(|e| e.end_secs())
                .fold(0.0f64, f64::max),
            _ => 0.0,
        }
    }

    /// Recalcula `preview_mix` (mono) y `duration_secs` desde los eventos.
    pub fn refresh_preview(&mut self) {
        if let ClipData::Audio {
            events,
            preview_mix,
            preview_sr,
            ..
        } = &mut self.content
        {
            let sr = events.first().map(|e| e.sample_rate).unwrap_or(44100);
            *preview_sr = sr;
            if events.is_empty() || sr == 0 {
                preview_mix.clear();
                self.duration_secs = 0.0;
                return;
            }
            let total_secs = events.iter().map(|e| e.end_secs()).fold(0.0f64, f64::max);
            let total_frames = (total_secs * sr as f64).ceil() as usize;
            let mut mix = vec![0.0f32; total_frames];
            for ev in events.iter() {
                let mono = ev.mono_mixed();
                // El evento puede empezar en negativo (count-in): lo previo
                // al 0 no se mezcla, se salta.
                let start_frame = (ev.start_secs * sr as f64).round() as i64;
                let fi = (ev.fade_in_secs * sr as f64).round() as usize;
                let fo = (ev.fade_out_secs * sr as f64).round() as usize;
                for (i, s) in mono.iter().enumerate() {
                    let dst = start_frame + i as i64;
                    if dst < 0 {
                        continue;
                    }
                    let dst = dst as usize;
                    if dst >= total_frames {
                        break;
                    }
                    let mut g = ev.gain;
                    if fi > 0 && i < fi {
                        g *= i as f32 / fi as f32;
                    }
                    let from_end = mono.len().saturating_sub(i);
                    if fo > 0 && from_end < fo {
                        g *= from_end as f32 / fo as f32;
                    }
                    mix[dst] += s * g;
                }
            }
            *preview_mix = mix;
            self.duration_secs = total_secs;
        }
    }

    pub fn alloc_event_id(&mut self) -> u64 {
        if let ClipData::Audio { next_event_id, .. } = &mut self.content {
            let id = (*next_event_id).max(1);
            *next_event_id = id + 1;
            id
        } else {
            0
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
    _sample_rate: u32,
    _transport_sample_count: u64,
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
                slot.clip.as_ref().map(|c| c.name.clone()).unwrap_or_default(),
            ),
            SlotState::Playing => (
                Color32::from_rgb(30, 110, 210),
                Color32::from_rgb(90, 180, 255),
                slot.clip.as_ref().map(|c| c.name.clone()).unwrap_or_default(),
            ),
            SlotState::QueuedToStop => (
                Color32::from_rgb(130, 45, 45),
                Color32::RED,
                slot.clip.as_ref().map(|c| c.name.clone()).unwrap_or_default(),
            ),
            SlotState::Empty => (Color32::from_gray(25), Color32::from_gray(40), "".to_string()),
        }
    };

    if is_selected {
        border_color = Color32::WHITE;
    }

    let size = Vec2::new(110.0, 54.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let mut btn_clicked = false;

    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(rect, 3.0, bg_color);

        if let Some(clip) = &slot.clip {
            let inner_rect = rect.shrink(2.0);
            let clipped_painter = ui.painter().with_clip_rect(inner_rect);

            match &clip.content {
                ClipData::Audio {
                    preview_mix: pcm_data,
                    events,
                    ..
                } => {
                    let wave_color = match slot.state {
                        SlotState::Playing => Color32::from_rgba_unmultiplied(10, 30, 70, 220),
                        SlotState::QueuedToPlay => Color32::from_rgba_unmultiplied(40, 30, 5, 200),
                        _ => Color32::from_rgba_unmultiplied(120, 160, 220, 180),
                    };
                    draw_mini_waveform(&clipped_painter, inner_rect, pcm_data, wave_color);
                    // Indicador de multi-evento: "×N" si hay más de una región.
                    if events.len() > 1 {
                        clipped_painter.text(
                            inner_rect.right_top() + egui::vec2(-4.0, 2.0),
                            Align2::RIGHT_TOP,
                            format!("×{}", events.len()),
                            egui::FontId::proportional(9.0),
                            Color32::from_rgb(0, 255, 200),
                        );
                    }
                }
                ClipData::Midi { notes } => {
                    let note_color = match slot.state {
                        SlotState::Playing => Color32::from_rgb(90, 180, 255),
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
                // Posición exacta del motor (voice_frame_linear + natural_frames).
                // Sin fallback de transporte: si el motor no responde, no se dibuja playhead.
                let play_progress: Option<f32> = engine_handle.and_then(|handle| {
                    let (frame, total) =
                        handle.try_lock().ok()?.voice_playhead_frame(track_idx, scene_idx)?;
                    if total == 0 {
                        return None;
                    }
                    Some((frame as f32 / total as f32).clamp(0.0, 1.0))
                });

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

        // Botón Play/Stop estilo Bitwig en la esquina superior izquierda
        if has_clip {
            let btn_size = Vec2::new(16.0, 16.0);
            let btn_rect = egui::Rect::from_min_size(rect.min + Vec2::new(3.0, 3.0), btn_size);
            let btn_id = ui.make_persistent_id(format!("pad_play_{}_{}", track_idx, scene_idx));
            let btn_response = ui.interact(btn_rect, btn_id, Sense::click());
            let btn_hovered = btn_response.hovered();

            let btn_bg = match slot.state {
                SlotState::Playing => Color32::from_rgb(35, 120, 220),
                SlotState::QueuedToPlay => Color32::from_rgb(180, 150, 40),
                SlotState::QueuedToStop => Color32::from_rgb(160, 55, 55),
                _ => {
                    if btn_hovered {
                        Color32::from_rgb(70, 95, 130)
                    } else {
                        Color32::from_rgb(30, 35, 45)
                    }
                }
            };
            ui.painter().rect_filled(btn_rect, 2.0, btn_bg);
            ui.painter().rect_stroke(
                btn_rect,
                2.0,
                Stroke::new(1.0_f32, Color32::from_gray(110)),
            );

            match slot.state {
                SlotState::Playing | SlotState::QueuedToStop => {
                    // Icono Stop (cuadrado)
                    let stop_rect =
                        egui::Rect::from_center_size(btn_rect.center(), Vec2::new(7.0, 7.0));
                    ui.painter().rect_filled(stop_rect, 1.0, Color32::WHITE);
                }
                _ => {
                    // Icono Play (triángulo)
                    let p1 = egui::pos2(btn_rect.min.x + 5.0, btn_rect.min.y + 4.0);
                    let p2 = egui::pos2(btn_rect.min.x + 5.0, btn_rect.max.y - 4.0);
                    let p3 = egui::pos2(btn_rect.max.x - 4.0, btn_rect.center().y);
                    ui.painter().add(egui::epaint::PathShape::convex_polygon(
                        vec![p1, p2, p3],
                        Color32::WHITE,
                        Stroke::NONE,
                    ));
                }
            }

            if btn_response.clicked() {
                btn_clicked = true;
            }
        }

        if !text.is_empty() {
            let display_text = if text.len() > 14 {
                format!("{}...", &text[..11])
            } else {
                text
            };

            // Nombre del sample al lado del botón Play/Stop
            let text_pos = if has_clip {
                egui::pos2(rect.min.x + 22.0, rect.min.y + 11.0)
            } else {
                rect.center()
            };
            let align = if has_clip {
                Align2::LEFT_CENTER
            } else {
                Align2::CENTER_CENTER
            };

            ui.painter().text(
                text_pos,
                align,
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
                            has_time_selection: true,
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
    
    if btn_clicked {
        state.selected_slot = Some((track_idx, scene_idx));
        trigger_pad(state, audio_proxy, track_idx, scene_idx);
    } else if response.clicked() {
        state.selected_slot = Some((track_idx, scene_idx));
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
    audio_proxy: &AudioProxy,
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

            let playhead = (|| {
                let handle = engine_handle?;
                let engine = handle.try_lock().ok()?;
                engine.voice_playhead_frame(track_idx, scene_idx)
            })();

            if playhead.is_some() {
                ui.ctx().request_repaint();
            }

            match &mut slot.content {
                ClipData::Audio { .. } => {
                    let old_loop = slot.loop_enabled;
                    let old_start = slot.loop_start;
                    let old_end = slot.loop_end;
                    let old_sel = slot.has_time_selection;
                    let old_sig: Vec<(u64, u64, u64, u32)> = slot
                        .audio_events()
                        .iter()
                        .map(|e| {
                            (
                                e.id,
                                (e.start_secs * 1_000_000.0).round() as u64,
                                e.frames(),
                                e.gain.to_bits(),
                            )
                        })
                        .collect();

                    clip_editor::show(
                        ui,
                        slot,
                        track_idx,
                        scene_idx,
                        None,
                        playhead,
                        sample_rate,
                        bpm as f32,
                    );

                    if slot.loop_enabled != old_loop
                        || slot.loop_start != old_start
                        || slot.loop_end != old_end
                        || slot.has_time_selection != old_sel
                    {
                        let start_secs = slot.loop_start as f32 / sample_rate as f32;
                        let end_secs = slot.loop_end as f32 / sample_rate as f32;
                        audio_proxy.set_clip_loop(
                            track_idx,
                            scene_idx,
                            start_secs,
                            end_secs,
                            slot.loop_enabled && slot.has_time_selection,
                        );
                    }
                    let new_sig: Vec<(u64, u64, u64, u32)> = slot
                        .audio_events()
                        .iter()
                        .map(|e| {
                            (
                                e.id,
                                (e.start_secs * 1_000_000.0).round() as u64,
                                e.frames(),
                                e.gain.to_bits(),
                            )
                        })
                        .collect();
                    if new_sig != old_sig {
                        sync_audio_events_to_engine(slot, audio_proxy, track_idx, scene_idx);
                    }
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

pub fn load_clip_into_slot(
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
        ClipData::Audio {
            events: Vec::new(),
            next_event_id: 1,
            preview_mix: Vec::new(),
            preview_sr: 44100,
        }
    };

    // Si el pad ya contiene un clip de AUDIO, no lo reemplazamos: apilamos
    // el nuevo sample como otro evento dentro del mismo pad (al final del
    // timeline). Así se pueden poner varios samples en un solo pad.
    if !is_midi {
        if let Some(existing) = state
            .grid
            .get_mut(track_idx)
            .and_then(|r| r.get_mut(scene_idx))
            .and_then(|s| s.clip.as_mut())
        {
            if matches!(existing.content, ClipData::Audio { .. }) {
                append_sample_as_event(existing, audio_proxy, track_idx, scene_idx, path, path_str);
                return;
            }
        }
    }

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
            loop_enabled: true,
            has_time_selection: true,
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

/// Decodifica un WAV a PCM interleaved + metadatos (helper compartido).
pub fn decode_audio_file(path: &std::path::Path) -> Option<(Vec<f32>, usize, u32)> {
    let reader = hound::WavReader::open(path).ok()?;
    let spec = reader.spec();
    let channels = spec.channels as usize;
    let bits = spec.bits_per_sample;
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.into_samples::<f32>().filter_map(Result::ok).collect(),
        hound::SampleFormat::Int => {
            let max_val = if bits <= 16 {
                i16::MAX as f32
            } else {
                (1 << (bits - 1)) as f32
            };
            reader
                .into_samples::<i32>()
                .filter_map(Result::ok)
                .map(|s| s as f32 / max_val)
                .collect()
        }
    };
    if samples.is_empty() {
        return None;
    }
    Some((samples, channels, spec.sample_rate))
}

/// Apila un nuevo sample como evento al final del timeline del pad.
/// Usado cuando se arrastra un segundo (o tercer...) archivo sobre un pad
/// que ya tiene audio: en vez de reemplazar, se suma.
pub fn append_sample_as_event(
    clip: &mut MatrixClip,
    audio_proxy: &AudioProxy,
    track_idx: usize,
    scene_idx: usize,
    path: PathBuf,
    path_str: String,
) {
    // Decodificar aquí para crear el evento de inmediato en la GUI.
    if let Some((samples, channels, sr)) = decode_audio_file(&path) {
        let start = clip.audio_total_secs();
        let id = clip.alloc_event_id();
        let name = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if let Some(events) = clip.audio_events_mut() {
            events.push(AudioEvent::new_full(
                id, name, samples, channels, sr, start,
            ));
        }
        clip.refresh_preview();
        sync_audio_events_to_engine(clip, audio_proxy, track_idx, scene_idx);
    } else {
        // Fallback: que el motor lo intente cargar como clip legacy.
        audio_proxy.send(GuiCommand::LoadClip {
            clip_id: clip.id,
            path: path_str,
            position_secs: 0.0,
            duration_secs: 0.0,
            offset_secs: 0.0,
            track_index: track_idx,
            scene_index: scene_idx,
        });
    }
}

/// Envía los eventos editados del pad al motor para mezcla multi-región.
pub fn sync_audio_events_to_engine(
    clip: &MatrixClip,
    audio_proxy: &AudioProxy,
    track_idx: usize,
    scene_idx: usize,
) {
    if let ClipData::Audio { events, .. } = &clip.content {
        let payload = events
            .iter()
            .map(|e| crate::audio_proxy::EngineEventData {
                id: e.id,
                // Ventana visible (el trim es no-destructivo en la GUI).
                samples: e.window_samples().to_vec(),
                channels: e.channels,
                clip_start_secs: e.start_secs as f32,
                sample_rate: e.sample_rate,
                gain: e.gain,
                fade_in_secs: e.fade_in_secs as f32,
                fade_out_secs: e.fade_out_secs as f32,
            })
            .collect();
        audio_proxy.send(GuiCommand::SetClipEvents {
            track_idx,
            scene_idx,
            events: payload,
        });
    }
}