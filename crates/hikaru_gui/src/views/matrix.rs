// Copyright (C) Hikaru Corporation - 2026
// Hikaru OpenLive - Bitwig 5.1.7 1:1 Arrange Workspace (Track Headers + Horizontal Clip Launcher + Arranger Timeline)
// GNU Affero General Public License v3
// crates/hikaru_gui/src/views/matrix.rs

use std::path::PathBuf;

use egui::{pos2, vec2, Button, Color32, Rect, RichText, Sense, Stroke, Ui};
use hikaru_audio_engine::AudioEngine;
use hikaru_sequencer::{QuantizationEngine, QuantizationGrid};

use crate::app::{AppMode, AppState};
use crate::audio_proxy::{AudioClipData, AudioProxy, GuiCommand};
use crate::theme::*;
use crate::ui::bitwig_kit::{self, JoinSide};
use crate::ui::bitwig_wgpu;
use crate::ui::fonts;
use crate::ui::icons;
pub use crate::views::clipboard::MatrixClipboard;
use crate::views::header::bitwig_layout_toggles_widget;
use crate::views::mixer::Track;
use crate::views::playlist::{
    self, build_audio_clip, px_to_ticks, snap_ticks, ticks_per_bar, ticks_to_px,
    ticks_to_secs_precise, ClipType, PlaylistClipDrag, PlaylistState,
};

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
    /// De qué archivo se decodificó el sample.
    pub source_path: Option<PathBuf>,
    pub samples: Vec<f32>,
    pub channels: usize,
    pub sample_rate: u32,
    pub start_secs: f64,
    pub trim_left_frames: usize,
    pub visible_frames: usize,
    pub gain: f32,
    pub fade_in_secs: f64,
    pub fade_out_secs: f64,
}

impl AudioEvent {
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
            source_path: None,
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

    pub fn total_frames(&self) -> usize {
        self.samples.len() / self.channels.max(1)
    }

    pub fn frames(&self) -> u64 {
        let avail = self.total_frames().saturating_sub(self.trim_left_frames);
        avail.min(self.visible_frames) as u64
    }

    pub fn window_samples(&self) -> &[f32] {
        let ch = self.channels.max(1);
        let start = (self.trim_left_frames * ch).min(self.samples.len());
        let end = (start + self.frames() as usize * ch).min(self.samples.len());
        &self.samples[start..end]
    }

    pub fn content_start_secs(&self) -> f64 {
        if self.sample_rate == 0 {
            return self.start_secs;
        }
        self.start_secs - self.trim_left_frames as f64 / self.sample_rate as f64
    }

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
            source_path: self.source_path.clone(),
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
        events: Vec<AudioEvent>,
        next_event_id: u64,
        preview_mix: Vec<f32>,
        preview_sr: u32,
    },
    Midi {
        notes: Vec<(u64, u8, u8, u32)>,
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
    pub launch_legato: bool,
    pub has_time_selection: bool,
    pub peaks: Vec<f32>,
    pub muted: bool,
    pub color: Option<egui::Color32>,
}

impl MatrixClip {
    pub const PEAK_BINS: usize = 256;

    pub fn duration_secs(&self) -> f64 {
        self.duration_secs
    }

    pub fn pcm_data(&self) -> &[f32] {
        match &self.content {
            ClipData::Audio { preview_mix, .. } => preview_mix,
            _ => &[],
        }
    }

    pub fn pcm_data_mut(&mut self) -> Option<&mut Vec<f32>> {
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

    pub fn audio_total_secs(&self) -> f64 {
        match &self.content {
            ClipData::Audio { events, .. } => events
                .iter()
                .map(|e| e.end_secs())
                .fold(0.0f64, f64::max),
            _ => 0.0,
        }
    }

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
    pub has_stop_button: bool,
}

impl Default for MatrixSlot {
    fn default() -> Self {
        Self {
            state: SlotState::Empty,
            clip: None,
            has_stop_button: true,
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

impl TrackMeta {
    pub fn stop(&mut self, _audio_proxy: &AudioProxy) {}
}

#[derive(Clone, Debug)]
pub struct SceneMeta {
    pub name: String,
    /// Override opcional de cuantización de lanzamiento para esta escena.
    /// `None` hereda `SessionMatrixState::global_launch_quantization`.
    pub launch_quantization: Option<QuantizationGrid>,
    /// Color opcional asignado a la cabecera de esta escena.
    pub color: Option<Color32>,
}

#[derive(Clone, Debug)]
pub struct SceneContextMenu {
    pub scene_idx: usize,
    pub click_pos: egui::Pos2,
}

#[derive(Clone, Debug)]
pub struct MatrixClipDrag {
    pub from_track: usize,
    pub from_scene: usize,
    pub clip: MatrixClip,
    pub grab_offset: egui::Vec2,
    pub is_copy: bool,
    pub moved: bool,
}

pub struct SessionMatrixState {
    pub grid: Vec<Vec<MatrixSlot>>,
    pub tracks: Vec<TrackMeta>,
    pub scenes: Vec<SceneMeta>,
    pub next_clip_id: usize,
    pub selected_slot: Option<(usize, usize)>,
    pub selected_scene: Option<usize>,
    pub editor_height: f32,
    pub editor_zoom_x: f32,
    pub show_editor: bool,
    pub global_launch_quantization: QuantizationGrid,
    pub context_menu: Option<crate::views::clip_context_menu::ClipLauncherContextMenu>,
    pub scene_context_menu: Option<SceneContextMenu>,
    pub clip_drag: Option<MatrixClipDrag>,
    pub renaming_scene: Option<(usize, String)>,
    pub scene_drag: Option<usize>,
    pub clip_launch_modes: std::collections::HashMap<(usize, usize), hikaru_sequencer::ClipLaunchMode>,
    pub clip_gains: std::collections::HashMap<(usize, usize), f32>,
}

impl Default for SessionMatrixState {
    fn default() -> Self {
        let default = hikaru_openlive_file::default_matrix();

        let tracks: Vec<TrackMeta> = default
            .tracks
            .iter()
            .map(|t| TrackMeta {
                name: t.name.clone(),
                muted: t.muted,
                soloed: t.soloed,
                volume: t.volume,
                pan: t.pan,
            })
            .collect();

        let scenes: Vec<SceneMeta> = default
            .scenes
            .iter()
            .map(|s| SceneMeta {
                name: s.name.clone(),
                launch_quantization: s
                    .launch_quantization
                    .as_deref()
                    .and_then(QuantizationGrid::from_notation),
                color: s.color.map(|[r, g, b]| Color32::from_rgb(r, g, b)),
            })
            .collect();

        let grid = vec![vec![MatrixSlot::default(); scenes.len()]; tracks.len()];

        Self {
            grid,
            tracks,
            scenes,
            next_clip_id: 1,
            selected_slot: None,
            selected_scene: None,
            editor_height: 104.0,
            editor_zoom_x: 0.04,
            show_editor: true,
            global_launch_quantization: QuantizationGrid::Quarter,
            context_menu: None,
            scene_context_menu: None,
            clip_drag: None,
            renaming_scene: None,
            scene_drag: None,
            clip_launch_modes: std::collections::HashMap::new(),
            clip_gains: std::collections::HashMap::new(),
        }
    }
}

impl SessionMatrixState {
    pub fn slot(&self, track: usize, scene: usize) -> Option<&MatrixSlot> {
        self.grid.get(track).and_then(|row| row.get(scene))
    }

    pub fn slot_mut(&mut self, track: usize, scene: usize) -> Option<&mut MatrixSlot> {
        self.grid.get_mut(track).and_then(|row| row.get_mut(scene))
    }

    pub fn stop_all_clips(&mut self, audio_proxy: &AudioProxy) {
        for track_idx in 0..self.tracks.len() {
            self.stop_track(audio_proxy, track_idx);
        }
    }

    pub fn stop_track(&mut self, audio_proxy: &AudioProxy, track_idx: usize) {
        if let Some(row) = self.grid.get_mut(track_idx) {
            for slot in row.iter_mut() {
                if slot.state == SlotState::Playing || slot.state == SlotState::QueuedToPlay {
                    slot.state = SlotState::Stopped;
                }
            }
        }
        audio_proxy.send(GuiCommand::StopTrack { track_idx });
    }

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

    pub fn insert_track(&mut self, idx: usize, name: String) {
        let idx = idx.min(self.tracks.len());
        self.tracks.insert(
            idx,
            TrackMeta {
                name,
                muted: false,
                soloed: false,
                volume: 0.75,
                pan: 0.0,
            },
        );
        let scene_count = self.scenes.len();
        self.grid.insert(idx, vec![MatrixSlot::default(); scene_count]);
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
            launch_quantization: None,
            color: None,
        });

        for track_row in &mut self.grid {
            track_row.push(MatrixSlot::default());
        }
    }

    pub fn add_scene_synced(&mut self, audio_proxy: &AudioProxy) -> usize {
        self.add_scene();
        audio_proxy.send(GuiCommand::AddScene);
        self.scenes.len().saturating_sub(1)
    }

    pub fn insert_scene_at(&mut self, scene_idx: usize) -> usize {
        let idx = scene_idx.min(self.scenes.len());
        let scene_num = self.scenes.len() + 1;
        self.scenes.insert(
            idx,
            SceneMeta {
                name: format!("Scene {}", scene_num),
                launch_quantization: None,
                color: None,
            },
        );
        for track_row in &mut self.grid {
            let col = idx.min(track_row.len());
            track_row.insert(col, MatrixSlot::default());
        }
        if let Some((t, s)) = self.selected_slot {
            if s >= idx {
                self.selected_slot = Some((t, s + 1));
            }
        }
        if let Some(sel) = self.selected_scene {
            if sel >= idx {
                self.selected_scene = Some(sel + 1);
            }
        }
        self.context_menu = None;
        self.scene_context_menu = None;
        self.clip_drag = None;
        idx
    }

    pub fn insert_scene_synced(&mut self, scene_idx: usize, audio_proxy: &AudioProxy) -> usize {
        let idx = self.insert_scene_at(scene_idx);
        audio_proxy.send(GuiCommand::InsertScene { scene_idx: idx });
        idx
    }

    pub fn remove_scene(&mut self) {
        let last = self.scenes.len().saturating_sub(1);
        let _ = self.remove_scene_at(last);
    }

    pub fn remove_scene_at(&mut self, scene_idx: usize) -> bool {
        if self.scenes.len() <= 1 || scene_idx >= self.scenes.len() {
            return false;
        }
        self.scenes.remove(scene_idx);
        for track_row in &mut self.grid {
            if scene_idx < track_row.len() {
                track_row.remove(scene_idx);
            }
        }
        if let Some((t, s)) = self.selected_slot {
            if s == scene_idx {
                self.selected_slot = None;
            } else if s > scene_idx {
                self.selected_slot = Some((t, s - 1));
            }
        }
        if let Some(sel) = self.selected_scene {
            if sel == scene_idx {
                self.selected_scene = Some(scene_idx.min(self.scenes.len().saturating_sub(1)));
            } else if sel > scene_idx {
                self.selected_scene = Some(sel - 1);
            }
        }
        self.context_menu = None;
        self.scene_context_menu = None;
        self.clip_drag = None;
        true
    }

    pub fn remove_scene_synced(&mut self, scene_idx: usize, audio_proxy: &AudioProxy) -> bool {
        if self.remove_scene_at(scene_idx) {
            audio_proxy.send(GuiCommand::RemoveScene { scene_idx });
            true
        } else {
            false
        }
    }

    pub fn set_scene_color(&mut self, scene_idx: usize, color: Color32) -> bool {
        let Some(scene) = self.scenes.get_mut(scene_idx) else {
            return false;
        };
        scene.color = Some(color);
        true
    }

    pub fn rename_scene(&mut self, scene_idx: usize, new_name: impl Into<String>) -> bool {
        let trimmed = new_name.into().trim().to_string();
        if trimmed.is_empty() {
            return false;
        }
        let Some(scene) = self.scenes.get_mut(scene_idx) else {
            return false;
        };
        scene.name = trimmed;
        true
    }

    pub fn move_scene(&mut self, from_idx: usize, to_idx: usize) -> bool {
        if from_idx >= self.scenes.len() || to_idx >= self.scenes.len() || from_idx == to_idx {
            return false;
        }
        let scene = self.scenes.remove(from_idx);
        self.scenes.insert(to_idx, scene);
        for track_row in &mut self.grid {
            if from_idx < track_row.len() && to_idx < track_row.len() {
                let slot = track_row.remove(from_idx);
                track_row.insert(to_idx, slot);
            }
        }
        let remap = |idx: usize| -> usize {
            if idx == from_idx {
                to_idx
            } else if from_idx < to_idx && idx > from_idx && idx <= to_idx {
                idx - 1
            } else if to_idx < from_idx && idx >= to_idx && idx < from_idx {
                idx + 1
            } else {
                idx
            }
        };
        if let Some((t, s)) = self.selected_slot {
            self.selected_slot = Some((t, remap(s)));
        }
        if let Some(sel) = self.selected_scene {
            self.selected_scene = Some(remap(sel));
        }
        if let Some((idx, buf)) = self.renaming_scene.take() {
            self.renaming_scene = Some((remap(idx), buf));
        }
        self.context_menu = None;
        self.scene_context_menu = None;
        self.clip_drag = None;
        self.scene_drag = None;
        true
    }

    pub fn move_scene_synced(
        &mut self,
        from_idx: usize,
        to_idx: usize,
        audio_proxy: &AudioProxy,
    ) -> bool {
        if self.move_scene(from_idx, to_idx) {
            audio_proxy.send(GuiCommand::MoveScene { from_idx, to_idx });
            true
        } else {
            false
        }
    }

    pub fn copy_scene(&self, scene_idx: usize) -> Option<crate::views::clipboard::CopiedScene> {
        let meta = self.scenes.get(scene_idx)?.clone();
        let slots = self
            .grid
            .iter()
            .map(|row| row.get(scene_idx).cloned().unwrap_or_default())
            .collect();
        Some(crate::views::clipboard::CopiedScene { meta, slots })
    }

    pub fn paste_scene(
        &mut self,
        scene_idx: usize,
        copied: &crate::views::clipboard::CopiedScene,
        audio_proxy: &AudioProxy,
    ) -> bool {
        if scene_idx >= self.scenes.len() {
            return false;
        }
        self.scenes[scene_idx].color = copied.meta.color;
        self.scenes[scene_idx].launch_quantization = copied.meta.launch_quantization;
        audio_proxy.send(GuiCommand::SetSceneLaunchQuantization {
            scene_idx,
            grid: copied.meta.launch_quantization,
        });
        for (track_idx, row) in self.grid.iter_mut().enumerate() {
            if let Some(target_slot) = row.get_mut(scene_idx) {
                if let Some(src_slot) = copied.slots.get(track_idx) {
                    target_slot.clip = src_slot.clip.clone();
                    target_slot.state = if target_slot.clip.is_some() {
                        SlotState::Stopped
                    } else {
                        SlotState::Empty
                    };
                    target_slot.has_stop_button = src_slot.has_stop_button;
                } else {
                    target_slot.clip = None;
                    target_slot.state = SlotState::Empty;
                }
            }
        }
        true
    }

    pub fn clear_scene_clips(&mut self, scene_idx: usize) {
        for row in &mut self.grid {
            if let Some(slot) = row.get_mut(scene_idx) {
                slot.clip = None;
                slot.state = SlotState::Empty;
            }
        }
    }

    pub fn set_global_launch_quantization(
        &mut self,
        grid: QuantizationGrid,
        audio_proxy: &AudioProxy,
    ) {
        self.global_launch_quantization = grid;
        audio_proxy.send(GuiCommand::SetGlobalLaunchQuantization { grid });
    }

    pub fn set_scene_launch_quantization(
        &mut self,
        scene_idx: usize,
        grid: Option<QuantizationGrid>,
        audio_proxy: &AudioProxy,
    ) -> bool {
        let Some(scene) = self.scenes.get_mut(scene_idx) else {
            return false;
        };
        scene.launch_quantization = grid;
        audio_proxy.send(GuiCommand::SetSceneLaunchQuantization { scene_idx, grid });
        true
    }

    pub fn effective_scene_quantization(&self, scene_idx: usize) -> QuantizationGrid {
        let override_grid = self
            .scenes
            .get(scene_idx)
            .and_then(|s| s.launch_quantization);
        QuantizationEngine::resolve_scene_grid(self.global_launch_quantization, override_grid)
    }

    pub fn set_clip_legato(
        &mut self,
        track_idx: usize,
        scene_idx: usize,
        legato: bool,
        audio_proxy: &AudioProxy,
    ) -> bool {
        let Some(clip) = self.slot_mut(track_idx, scene_idx).and_then(|s| s.clip.as_mut()) else {
            return false;
        };
        clip.launch_legato = legato;
        audio_proxy.send(GuiCommand::SetClipLegato {
            track_idx,
            scene_idx,
            legato,
        });
        true
    }

    pub fn clip_launch_mode(
        &self,
        track_idx: usize,
        scene_idx: usize,
    ) -> hikaru_sequencer::ClipLaunchMode {
        self.clip_launch_modes
            .get(&(track_idx, scene_idx))
            .copied()
            .unwrap_or(hikaru_sequencer::ClipLaunchMode::Trigger)
    }

    pub fn set_clip_launch_mode(
        &mut self,
        track_idx: usize,
        scene_idx: usize,
        mode: hikaru_sequencer::ClipLaunchMode,
        audio_proxy: &AudioProxy,
    ) -> bool {
        if self.slot(track_idx, scene_idx).and_then(|s| s.clip.as_ref()).is_none() {
            return false;
        }
        self.clip_launch_modes.insert((track_idx, scene_idx), mode);
        audio_proxy.send(GuiCommand::SetClipLaunchMode {
            track_idx,
            scene_idx,
            mode,
        });
        true
    }

    pub fn release_pad(
        &mut self,
        audio_proxy: &AudioProxy,
        track_idx: usize,
        scene_idx: usize,
    ) -> bool {
        if self.clip_launch_mode(track_idx, scene_idx) != hikaru_sequencer::ClipLaunchMode::Gate {
            return false;
        }
        let Some(slot) = self.slot_mut(track_idx, scene_idx) else {
            return false;
        };
        if slot.clip.is_none() {
            return false;
        }
        slot.state = SlotState::Stopped;
        audio_proxy.send(GuiCommand::ReleaseClip {
            track_idx,
            scene_idx,
        });
        true
    }

    pub fn clip_gain(&self, track_idx: usize, scene_idx: usize) -> f32 {
        self.clip_gains
            .get(&(track_idx, scene_idx))
            .copied()
            .unwrap_or(1.0)
    }

    pub fn clip_gain_db(&self, track_idx: usize, scene_idx: usize) -> f32 {
        let g = self.clip_gain(track_idx, scene_idx);
        if g <= 1e-4 {
            -60.0
        } else {
            (20.0 * g.log10()).clamp(-60.0, 6.0)
        }
    }

    pub fn set_clip_gain(
        &mut self,
        track_idx: usize,
        scene_idx: usize,
        gain: f32,
        audio_proxy: &AudioProxy,
    ) -> bool {
        if !gain.is_finite()
            || self.slot(track_idx, scene_idx).and_then(|s| s.clip.as_ref()).is_none()
        {
            return false;
        }
        let clamped = gain.clamp(0.0, 2.0);
        self.clip_gains.insert((track_idx, scene_idx), clamped);
        audio_proxy.send(GuiCommand::SetClipGain {
            track_idx,
            scene_idx,
            gain: clamped,
        });
        true
    }

    pub fn set_clip_gain_db(
        &mut self,
        track_idx: usize,
        scene_idx: usize,
        db: f32,
        audio_proxy: &AudioProxy,
    ) -> bool {
        if !db.is_finite() {
            return false;
        }
        let clamped = db.clamp(-60.0, 6.0);
        let lin = if clamped <= -59.5 {
            0.0
        } else {
            10.0_f32.powf(clamped / 20.0).clamp(0.0, 2.0)
        };
        self.set_clip_gain(track_idx, scene_idx, lin, audio_proxy)
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
                        speed_ratio: 1.0,
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
                        speed_ratio: 1.0,
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
            *slot = MatrixSlot::default();
            audio_proxy.send(GuiCommand::ClearMatrixSlot { track_idx, scene_idx });
        }
    }

    pub fn move_slot(
        &mut self,
        src_track: usize,
        src_scene: usize,
        dst_track: usize,
        dst_scene: usize,
        audio_proxy: &AudioProxy,
    ) {
        if src_track == dst_track && src_scene == dst_scene {
            return;
        }
        if src_track >= self.grid.len() || src_scene >= self.scenes.len() {
            return;
        }
        if dst_track >= self.grid.len() || dst_scene >= self.scenes.len() {
            return;
        }

        let mut moving_slot = std::mem::take(&mut self.grid[src_track][src_scene]);
        if moving_slot.clip.is_none() {
            return;
        }

        moving_slot.state = SlotState::Stopped;

        // Avisar al AudioEngine para mover el clip de forma atómica en el backend:
        // detiene reproducción en origen y destino, limpia el slot destino si estaba ocupado
        // y transfiere las instancias de clip (audio y midi) a las nuevas coordenadas.
        audio_proxy.send(GuiCommand::MoveMatrixClip {
            src_track,
            src_scene,
            dst_track,
            dst_scene,
        });

        self.grid[dst_track][dst_scene] = moving_slot;

        if let Some(ref clip) = self.grid[dst_track][dst_scene].clip {
            match &clip.content {
                ClipData::Midi { .. } => {
                    sync_midi_clip(self, audio_proxy, dst_track, dst_scene);
                }
                ClipData::Audio { .. } => {
                    sync_audio_events_to_engine(clip, audio_proxy, dst_track, dst_scene);
                    audio_proxy.set_clip_loop(
                        dst_track,
                        dst_scene,
                        clip.loop_start as f32,
                        clip.loop_end as f32,
                        clip.loop_enabled,
                    );
                }
            }
        }

        if self.selected_slot == Some((src_track, src_scene)) {
            self.selected_slot = Some((dst_track, dst_scene));
        }
    }

    pub fn copy_to_slot(
        &mut self,
        src_track: usize,
        src_scene: usize,
        dst_track: usize,
        dst_scene: usize,
        audio_proxy: &AudioProxy,
    ) {
        if src_track >= self.grid.len() || src_scene >= self.scenes.len() {
            return;
        }
        if dst_track >= self.grid.len() || dst_scene >= self.scenes.len() {
            return;
        }

        let current_slot = self.grid[src_track][src_scene].clone();
        if current_slot.clip.is_none() {
            return;
        }
        let mut clip_copy = current_slot;
        clip_copy.state = SlotState::Stopped;

        let next_id = self.next_clip_id;
        self.next_clip_id += 1;

        // Limpiar cualquier clip previo en el destino en el backend
        audio_proxy.send(GuiCommand::ClearMatrixSlot {
            track_idx: dst_track,
            scene_idx: dst_scene,
        });

        let is_midi = matches!(
            clip_copy.clip.as_ref().map(|c| &c.content),
            Some(ClipData::Midi { .. })
        );

        if let Some(clip) = clip_copy.clip.as_mut() {
            clip.id = next_id;
            if !is_midi {
                sync_audio_events_to_engine(clip, audio_proxy, dst_track, dst_scene);
                let path_str = clip.path.to_string_lossy().to_string();
                if !path_str.is_empty() {
                    audio_proxy.send(GuiCommand::LoadClip {
                        clip_id: next_id,
                        path: path_str,
                        position_secs: 0.0,
                        duration_secs: clip.duration_secs() as f32,
                        offset_secs: 0.0,
                        track_index: dst_track,
                        scene_index: dst_scene,
                        speed_ratio: 1.0,
                    });
                }
                audio_proxy.set_clip_loop(
                    dst_track,
                    dst_scene,
                    clip.loop_start as f32,
                    clip.loop_end as f32,
                    clip.loop_enabled,
                );
            }
        }

        self.grid[dst_track][dst_scene] = clip_copy;
        if is_midi {
            sync_midi_clip(self, audio_proxy, dst_track, dst_scene);
        } else if let Some(ref c) = self.grid[dst_track][dst_scene].clip {
            sync_audio_events_to_engine(c, audio_proxy, dst_track, dst_scene);
        }

        self.selected_slot = Some((dst_track, dst_scene));
    }
}

pub(crate) fn sync_midi_clip(
    state: &SessionMatrixState,
    audio_proxy: &AudioProxy,
    track_idx: usize,
    scene_idx: usize,
) {
    if let Some(clip) = state
        .grid
        .get(track_idx)
        .and_then(|row| row.get(scene_idx))
        .and_then(|slot| slot.clip.as_ref())
    {
        if let ClipData::Midi { notes } = &clip.content {
            audio_proxy.send(GuiCommand::UpdateMidiClipNotes {
                track_idx,
                scene_idx,
                notes: notes.clone(),
            });
            audio_proxy.send(GuiCommand::SetMidiClipLoop {
                track_idx,
                scene_idx,
                loop_start_ticks: clip.loop_start,
                loop_end_ticks: clip.loop_end.max(clip.loop_start + 240),
            });
            audio_proxy.send(GuiCommand::SetMidiClipLoopEnabled {
                track_idx,
                scene_idx,
                enabled: clip.loop_enabled,
            });
            audio_proxy.send(GuiCommand::SetClipLegato {
                track_idx,
                scene_idx,
                legato: clip.launch_legato,
            });
        }
    }
}

/// Configura los límites de loop (en segundos) de un clip de audio de la
/// matriz, validando `0 <= start < end <= duration_secs` y sincronizando
/// inmediatamente con el motor de audio.
pub fn set_audio_clip_loop_secs(
    state: &mut SessionMatrixState,
    audio_proxy: &AudioProxy,
    track_idx: usize,
    scene_idx: usize,
    start_secs: f64,
    end_secs: f64,
    enabled: bool,
) -> bool {
    if !start_secs.is_finite() || !end_secs.is_finite() {
        return false;
    }
    let Some(clip) = state.slot_mut(track_idx, scene_idx).and_then(|s| s.clip.as_mut()) else {
        return false;
    };
    let sr = match &clip.content {
        ClipData::Audio { events, preview_sr, .. } => {
            events.first().map(|e| e.sample_rate).unwrap_or(*preview_sr).max(1) as f64
        }
        ClipData::Midi { .. } => return false,
    };
    let max_secs = clip.duration_secs.max(0.01);
    let min_len = 0.01_f64.min(max_secs * 0.5).max(1.0 / sr);
    let clamped_start = start_secs.clamp(0.0, (max_secs - min_len).max(0.0));
    let clamped_end = end_secs.clamp((clamped_start + min_len).min(max_secs), max_secs);
    let start_frames = (clamped_start * sr).round() as u64;
    let end_frames = ((clamped_end * sr).round() as u64).max(start_frames + 1);

    clip.loop_start = start_frames;
    clip.loop_end = end_frames;
    clip.loop_enabled = enabled;
    audio_proxy.set_clip_loop(
        track_idx,
        scene_idx,
        clamped_start as f32,
        clamped_end as f32,
        enabled,
    );
    true
}

/// Configura los límites de loop (en ticks a 960 PPQN) de un clip MIDI de la
/// matriz, garantizando una duración mínima de 1/16 (240 ticks) y sincronizando
/// inmediatamente con el motor de audio.
pub fn set_midi_clip_loop_ticks(
    state: &mut SessionMatrixState,
    audio_proxy: &AudioProxy,
    track_idx: usize,
    scene_idx: usize,
    start_ticks: u64,
    end_ticks: u64,
    enabled: bool,
    bpm: f64,
) -> bool {
    let Some(clip) = state.slot_mut(track_idx, scene_idx).and_then(|s| s.clip.as_mut()) else {
        return false;
    };
    if !matches!(clip.content, ClipData::Midi { .. }) {
        return false;
    }
    let min_ticks = 240u64;
    let clamped_end = end_ticks.max(start_ticks.saturating_add(min_ticks));
    clip.loop_start = start_ticks;
    clip.loop_end = clamped_end;
    clip.loop_enabled = enabled;
    clip.duration_secs = (clamped_end as f64 / 960.0) * (60.0 / bpm.max(1.0));

    audio_proxy.send(GuiCommand::SetMidiClipLoop {
        track_idx,
        scene_idx,
        loop_start_ticks: start_ticks,
        loop_end_ticks: clamped_end,
    });
    audio_proxy.send(GuiCommand::SetMidiClipLoopEnabled {
        track_idx,
        scene_idx,
        enabled,
    });
    true
}

/// Alterna el estado `loop_enabled` de cualquier clip (audio o MIDI) y lo
/// sincroniza inmediatamente al motor de audio.
pub fn toggle_clip_loop(
    state: &mut SessionMatrixState,
    audio_proxy: &AudioProxy,
    track_idx: usize,
    scene_idx: usize,
) -> Option<bool> {
    let (new_enabled, is_midi, sr, loop_start, loop_end, dur_secs) = {
        let clip = state.slot_mut(track_idx, scene_idx)?.clip.as_mut()?;
        clip.loop_enabled = !clip.loop_enabled;
        match &clip.content {
            ClipData::Audio { events, preview_sr, .. } => {
                let sr = events.first().map(|e| e.sample_rate).unwrap_or(*preview_sr).max(1) as f64;
                (clip.loop_enabled, false, sr, clip.loop_start, clip.loop_end, clip.duration_secs)
            }
            ClipData::Midi { .. } => {
                (clip.loop_enabled, true, 1.0, clip.loop_start, clip.loop_end, clip.duration_secs)
            }
        }
    };
    if is_midi {
        audio_proxy.send(GuiCommand::SetMidiClipLoopEnabled {
            track_idx,
            scene_idx,
            enabled: new_enabled,
        });
    } else {
        let start_secs = (loop_start as f64 / sr) as f32;
        let end_secs = if loop_end > loop_start {
            (loop_end as f64 / sr) as f32
        } else {
            dur_secs as f32
        };
        audio_proxy.set_clip_loop(track_idx, scene_idx, start_secs, end_secs, new_enabled);
    }
    Some(new_enabled)
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

    // Ensure audio engine is in Clip Launcher mode when triggering a pad
    audio_proxy.send(GuiCommand::SetAppMode(false));

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
    audio_proxy.send(GuiCommand::SetAppMode(false));
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

pub fn load_clip_into_slot(
    state: &mut SessionMatrixState,
    audio_proxy: &AudioProxy,
    track_idx: usize,
    scene_idx: usize,
    path: PathBuf,
    bpm: f64,
) {
    if track_idx >= state.grid.len() {
        return;
    }
    if scene_idx >= state.scenes.len() {
        return;
    }

    let name = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let new_id = state.next_clip_id;
    state.next_clip_id += 1;

    let path_str = path.to_string_lossy().to_string();
    let is_midi = crate::views::explorer::is_midi_file(&path);

    let speed_ratio = if is_midi {
        1.0
    } else {
        crate::audio_proxy::compute_tempo_sync_ratio(&path, bpm)
    };

    let (content, real_dur, total_frames) = if is_midi {
        let parsed_notes: Vec<(u64, u8, u8, u32)> = if let Ok(bytes) = std::fs::read(&path) {
            hikaru_midi::parse_midi_file_to_notes(&bytes)
                .map(|notes| {
                    notes
                        .into_iter()
                        .map(|n| (n.start_tick, n.pitch, n.velocity, n.duration_ticks))
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let max_tick = parsed_notes
            .iter()
            .map(|(st, _, _, dur)| st + *dur as u64)
            .max()
            .unwrap_or(3840);
        let bars = (max_tick as f64 / 3840.0).ceil().max(1.0) as u64;
        let total_ticks = bars * 3840;
        let effective_bpm = if bpm > 0.0 { bpm as f64 } else { 120.0 };
        let dur = (total_ticks as f64 / 960.0) * (60.0 / effective_bpm);
        (ClipData::Midi { notes: parsed_notes }, dur, total_ticks)
    } else {
        let mut events = Vec::new();
        if let Some((samples, channels, sr)) = decode_audio_file(&path) {
            let mut ev = AudioEvent::new_full(1, name.clone(), samples, channels, sr, 0.0);
            ev.source_path = Some(path.clone());
            events.push(ev);
        }
        let dur_calc = if let Ok(reader) = crate::wav_io::open(path_str.as_str()) {
            let spec = reader.spec();
            let samples = reader.duration();
            let dur = if spec.sample_rate > 0 {
                (samples as f64 / spec.sample_rate as f64) / speed_ratio as f64
            } else {
                0.0
            };
            let adjusted_frames = (samples as f64 / speed_ratio as f64).round() as u64;
            (dur, adjusted_frames)
        } else {
            (0.0, 0)
        };
        (
            ClipData::Audio {
                events,
                next_event_id: 2,
                preview_mix: Vec::new(),
                preview_sr: 44100,
            },
            dur_calc.0,
            dur_calc.1,
        )
    };

    let mut local_state = PlaylistState::default();
    local_state.zoom_x = state.editor_zoom_x;

    let peaks = if is_midi {
        Vec::new()
    } else {
        crate::views::hikaru_native_plugins::hikaru_opendms::open_dms_sampler::load_peaks_from_wav(
            path_str.as_str(),
            MatrixClip::PEAK_BINS,
        )
    };

    state.grid[track_idx][scene_idx] = MatrixSlot {
        state: SlotState::Stopped,
        has_stop_button: true,
        clip: Some(MatrixClip {
            id: new_id,
            name: name.clone(),
            path,
            duration_secs: real_dur,
            content,
            local_state,
            local_track: Track::new(0, name, false),
            local_bar: 1.0,
            loop_start: 0,
            loop_end: total_frames,
            loop_enabled: true,
            launch_legato: true,
            has_time_selection: true,
            peaks,
            muted: false,
            color: None,
        }),
    };

    state.selected_slot = Some((track_idx, scene_idx));

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
            speed_ratio,
        });
    }
}

pub fn create_empty_midi_clip(
    state: &mut SessionMatrixState,
    audio_proxy: &crate::audio_proxy::AudioProxy,
    track_idx: usize,
    scene_idx: usize,
) {
    if track_idx >= state.tracks.len() || scene_idx >= state.scenes.len() {
        return;
    }
    let new_id = state.next_clip_id;
    state.next_clip_id += 1;
    let name = format!("MIDI Clip {}", new_id);
    let mut local_state = PlaylistState::default();
    local_state.zoom_x = state.editor_zoom_x;
    let total_ticks = 3840u64 * 4; // 4 compases por defecto (16 beats × 960 ticks)
    let dur = (total_ticks as f64 / 960.0) * (60.0 / 120.0);

    state.grid[track_idx][scene_idx] = MatrixSlot {
        state: SlotState::Stopped,
        has_stop_button: true,
        clip: Some(MatrixClip {
            id: new_id,
            name: name.clone(),
            path: PathBuf::new(),
            duration_secs: dur,
            content: ClipData::Midi { notes: Vec::new() },
            local_state,
            local_track: Track::new(0, name, false),
            local_bar: 1.0,
            loop_start: 0,
            loop_end: total_ticks,
            loop_enabled: true,
            launch_legato: true,
            has_time_selection: true,
            peaks: Vec::new(),
            muted: false,
            color: None,
        }),
    };
    state.selected_slot = Some((track_idx, scene_idx));
    sync_midi_clip(state, audio_proxy, track_idx, scene_idx);
}

pub fn decode_audio_file(path: &std::path::Path) -> Option<(Vec<f32>, usize, u32)> {
    match crate::wav_io::read_file(path) {
        Ok((samples, channels, sample_rate)) if !samples.is_empty() => {
            Some((samples, channels, sample_rate))
        }
        _ => None,
    }
}

pub fn append_sample_as_event(
    clip: &mut MatrixClip,
    audio_proxy: &AudioProxy,
    track_idx: usize,
    scene_idx: usize,
    path: PathBuf,
    path_str: String,
) {
    if let Some((samples, channels, sr)) = decode_audio_file(&path) {
        let start = clip.audio_total_secs();
        let id = clip.alloc_event_id();
        let name = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if let Some(events) = clip.audio_events_mut() {
            let mut ev = AudioEvent::new_full(id, name, samples, channels, sr, start);
            ev.source_path = Some(path.clone());
            events.push(ev);
        }
        clip.refresh_preview();
        clip.peaks = crate::views::hikaru_native_plugins::hikaru_opendms::open_dms_sampler::load_peaks_from_wav(
            path_str.as_str(),
            MatrixClip::PEAK_BINS,
        );
        sync_audio_events_to_engine(clip, audio_proxy, track_idx, scene_idx);
    } else {
        audio_proxy.send(GuiCommand::LoadClip {
            clip_id: clip.id,
            path: path_str,
            position_secs: 0.0,
            duration_secs: 0.0,
            offset_secs: 0.0,
            track_index: track_idx,
            scene_index: scene_idx,
            speed_ratio: 1.0,
        });
    }
}

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

pub fn db_text(volume: f32) -> String {
    let db_val = if volume <= 0.0 {
        -60.0
    } else if volume <= 0.75 {
        -60.0 + (volume / 0.75) * 60.0
    } else {
        ((volume - 0.75) / 0.25) * 6.0
    };
    format!("{:.1} dB", db_val)
}

pub fn pan_text(pan: f32) -> String {
    let pan = pan.clamp(-1.0, 1.0);
    if pan.abs() < 0.02 {
        "C".to_string()
    } else if pan < 0.0 {
        format!("{:.0}L", -pan * 100.0)
    } else {
        format!("{:.0}R", pan * 100.0)
    }
}

pub fn mixer_index_of_matrix_track(matrix_idx: usize) -> usize {
    matrix_idx + 1
}

pub fn select_matrix_track(s: &mut AppState, track_idx: usize) {
    s.selected_track_index = mixer_index_of_matrix_track(track_idx);
    s.matrix_state.selected_scene = None;
}

/// Calcula los colores `(fondo, texto, triángulo play)` para la cabecera de una escena,
/// asegurando contraste legible cuando la escena tiene un color personalizado.
pub fn scene_header_colors(
    custom_color: Option<Color32>,
    is_queued: bool,
    is_hovered: bool,
    is_selected: bool,
    play_hovered: bool,
) -> (Color32, Color32, Color32) {
    let bg = if is_queued {
        Color32::from_rgb(0x6E, 0x4A, 0x24)
    } else if let Some(c) = custom_color {
        if is_hovered {
            Color32::from_rgb(
                c.r().saturating_add(16),
                c.g().saturating_add(16),
                c.b().saturating_add(16),
            )
        } else {
            c
        }
    } else if is_hovered {
        Color32::from_rgb(0x5E, 0x60, 0x66)
    } else if is_selected {
        Color32::from_rgb(0x55, 0x57, 0x5E)
    } else {
        Color32::from_rgb(0x4B, 0x4D, 0x53)
    };

    let luma = 0.299 * bg.r() as f32 + 0.587 * bg.g() as f32 + 0.114 * bg.b() as f32;
    let light_bg = luma > 155.0;
    let text_col = if light_bg {
        Color32::from_rgb(0x14, 0x15, 0x18)
    } else {
        Color32::WHITE
    };
    let play_col = if is_queued {
        ACCENT_ORANGE
    } else if play_hovered {
        if light_bg {
            Color32::BLACK
        } else {
            Color32::WHITE
        }
    } else if light_bg {
        Color32::from_rgb(0x28, 0x2A, 0x30)
    } else {
        Color32::from_rgb(0xBF, 0xC2, 0xC7)
    };

    (bg, text_col, play_col)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatrixMixTarget {
    Volume(usize),
    Pan(usize),
}

pub fn step_mix_slider_gesture(_s: &mut AppState, _target: MatrixMixTarget, _delta: f32) {}
pub fn step_mix_pan_gesture(_s: &mut AppState, _target: MatrixMixTarget, _delta: f32) {}
pub fn end_mix_drag(_s: &mut AppState) {}

pub fn h_mix_slider_ex(ui: &mut Ui, volume: &mut f32, width: f32, height: f32) {
    bitwig_kit::fader_horizontal_range(
        ui,
        vec2(width.max(68.0), height.max(12.0)),
        volume,
        0.0..=1.0,
        0.75,
        BW_FADER_AMBER,
        BW_FADER_ORANGE,
        false,
    );
}

pub fn pan_knob_ex(ui: &mut Ui, pan: &mut f32, size: f32) {
    bitwig_kit::rotary_knob(
        ui,
        size.max(22.0),
        pan,
        -1.0..=1.0,
        0.0,
        BW_FADER_AMBER,
        true,
    );
}

/// Darker clip-tinted body background for Clip Launcher pads (Bitwig-style).
fn pad_body_color(clip_color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(
        ((clip_color.r() as u16 * 3) / 10) as u8,
        ((clip_color.g() as u16 * 3) / 10) as u8,
        ((clip_color.b() as u16 * 3) / 10) as u8,
        alpha,
    )
}

/// Renders a high-density, clip-tinted peak + RMS waveform inside a Clip Launcher pad.
fn paint_pad_waveform(
    painter: &egui::Painter,
    wave_rect: Rect,
    peaks: &[f32],
    clip_color: Color32,
    muted: bool,
    alpha_scale: f32,
) {
    let width = wave_rect.width();
    let height = wave_rect.height();
    if peaks.is_empty() || width <= 1.0 || height <= 2.0 {
        return;
    }

    let base = if muted {
        Color32::from_rgb(0x78, 0x7C, 0x84)
    } else {
        clip_color
    };
    let scale = alpha_scale.clamp(0.0, 1.0);
    let zero_col = Color32::from_rgba_unmultiplied(
        base.r(),
        base.g(),
        base.b(),
        (42.0 * scale) as u8,
    );
    let peak_col = Color32::from_rgba_unmultiplied(
        base.r(),
        base.g(),
        base.b(),
        (95.0 * scale) as u8,
    );
    let rms_col = Color32::from_rgba_unmultiplied(
        base.r(),
        base.g(),
        base.b(),
        (165.0 * scale) as u8,
    );

    let cy = wave_rect.center().y;
    let hh = height * 0.44;

    // Subtle zero-crossing baseline
    painter.line_segment(
        [pos2(wave_rect.min.x, cy), pos2(wave_rect.max.x, cy)],
        Stroke::new(1.0_f32, zero_col),
    );

    let cols = (width.floor() as usize).clamp(1, 512);
    let step = width / cols as f32;
    for col in 0..cols {
        let p0 = (col * peaks.len()) / cols;
        let p1 = (((col + 1) * peaks.len()) / cols)
            .max(p0 + 1)
            .min(peaks.len());
        let mut peak = 0.0_f32;
        let mut sum_sq = 0.0_f32;
        for &v in &peaks[p0..p1] {
            let av = if v.is_finite() { v.abs().clamp(0.0, 1.0) } else { 0.0 };
            if av > peak {
                peak = av;
            }
            sum_sq += av * av;
        }
        let rms = (sum_sq / (p1 - p0) as f32).sqrt();
        let x = wave_rect.min.x + (col as f32 + 0.5) * step;
        let peak_h = (peak * hh).max(0.75);
        let rms_h = (rms * 0.72 * hh).clamp(0.5, peak_h);

        painter.line_segment(
            [pos2(x, cy - peak_h), pos2(x, cy + peak_h)],
            Stroke::new(1.0_f32, peak_col),
        );
        if rms_h > 0.75 {
            painter.line_segment(
                [pos2(x, cy - rms_h), pos2(x, cy + rms_h)],
                Stroke::new(1.0_f32, rms_col),
            );
        }
    }
}

pub fn compact_level_meter(ui: &mut Ui, level: f32, height: f32) {
    let lvl = if level.is_finite() { level.clamp(0.0, 1.0) } else { 0.0 };
    let (rect, _) = ui.allocate_exact_size(vec2(4.0, height.max(16.0)), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 1.0, Color32::from_rgb(0x12, 0x12, 0x12));
    if lvl > 0.002 {
        let fill_h = rect.height() * lvl;
        let fill_rect = Rect::from_min_max(pos2(rect.left(), rect.bottom() - fill_h), rect.max);
        let col = if lvl > 0.85 {
            Color32::from_rgb(0xFF, 0x3B, 0x30)
        } else if lvl > 0.65 {
            Color32::from_rgb(0xF2, 0xCA, 0x19)
        } else {
            Color32::from_rgb(0x1F, 0xEB, 0x54)
        };
        painter.rect_filled(fill_rect, 1.0, col);
    }
}

pub fn ms_button_sized(
    ui: &mut Ui,
    _id: &str,
    glyph: &'static str,
    width: f32,
    height: f32,
    active: bool,
    active_color: Color32,
    mut on_toggle: impl FnMut(),
) {
    let (bg_col, fg_col) = if active {
        (active_color, Color32::from_rgb(0x11, 0x11, 0x11))
    } else {
        (Color32::from_rgb(0x33, 0x33, 0x33), Color32::from_rgb(0xD4, 0xD4, 0xD4))
    };
    let btn = Button::new(RichText::new(glyph).size(9.5).strong().color(fg_col))
        .fill(bg_col)
        .min_size(vec2(width, height));
    if ui.add(btn).clicked() {
        on_toggle();
    }
}

pub fn ms_button(
    ui: &mut Ui,
    id: &str,
    glyph: &'static str,
    active: bool,
    active_color: Color32,
    on_toggle: impl FnMut(),
) {
    ms_button_sized(ui, id, glyph, 22.0, 18.0, active, active_color, on_toggle);
}

const BITWIG_HEADER_W: f32 = 186.0;
const BITWIG_TOOLBAR_H: f32 = 28.0;
const BITWIG_ROW_H: f32 = 52.0;
const BITWIG_SCENE_W: f32 = 96.0;
const BITWIG_MASTER_H: f32 = 44.0;

fn track_color_for(idx: usize) -> Color32 {
    let palette = [
        Color32::from_rgb(0xFF, 0x55, 0x00), // Bitwig Orange (Track 1)
        Color32::from_rgb(0xE5, 0xAE, 0x19), // Bitwig Yellow (Track 2)
        Color32::from_rgb(0x3A, 0xA8, 0xE0), // Bitwig Sky Blue (Track 3)
        Color32::from_rgb(0x28, 0xD9, 0x59), // Bitwig Emerald (Track 4)
        Color32::from_rgb(0xB8, 0x6B, 0xF5), // Bitwig Violet (Track 5)
        Color32::from_rgb(0xF2, 0x44, 0x3D), // Bitwig Coral Red (Track 6)
        Color32::from_rgb(0x00, 0xC7, 0xE6), // Bitwig Cyan (Track 7)
        Color32::from_rgb(0xFF, 0x9E, 0x1B), // Bitwig Amber (Track 8)
    ];
    palette[idx % palette.len()]
}

/// Renders the 1:1 Bitwig Studio 5.1.7 Arrange Workspace (`bitwig-clip-launcher-and-playlist.png`):
/// - Left column: Top-left layout toggles (`Clip Launcher` | `Arranger Timeline`) + warm grey Track Headers + `#0A0A0C` void + pinned `Master` track.
/// - Middle column (`show_clip_launcher`): Horizontal Clip Launcher (`Rows = Tracks`, `Cols = Scenes`) over `#0A0A0C` void.
/// - Right column (`show_arranger`): Bitwig signature neutral medium-grey (`#73757A` / `#6D6F74` lanes, `#5C5E62` void) Arranger Timeline with continuous dark bar/beat grid lines and GPU controls.
pub fn render(ui: &mut Ui, state: &mut AppState) {
    if !state.panel_layout.show_clip_launcher && !state.panel_layout.show_arranger {
        state.panel_layout.show_clip_launcher = true;
    }

    let show_launcher = state.panel_layout.show_clip_launcher;
    let show_timeline = state.panel_layout.show_arranger;

    let tracks_count = state.matrix_state.tracks.len();
    let scenes_count = state.matrix_state.scenes.len();

    let total_rect = ui.available_rect_before_wrap();
    let (workspace_rect, workspace_resp) =
        ui.allocate_exact_size(total_rect.size(), Sense::click_and_drag());
    let painter = ui.painter_at(workspace_rect);
    painter.rect_filled(workspace_rect, 0.0, BW_DARK_WELL);

    // Calculate horizontal section widths
    let header_w = BITWIG_HEADER_W;
    let remaining_w = (workspace_rect.width() - header_w).max(100.0);

    let (launcher_w, timeline_w) = match (show_launcher, show_timeline) {
        (true, true) => {
            let natural_launcher_w = scenes_count as f32 * BITWIG_SCENE_W + 34.0;
            let lw = natural_launcher_w.min(remaining_w * 0.46).max(180.0);
            (lw, (remaining_w - lw).max(160.0))
        }
        (true, false) => (remaining_w, 0.0),
        (false, true) => (0.0, remaining_w),
        (false, false) => (remaining_w, 0.0),
    };

    let header_x0 = workspace_rect.min.x;
    let header_x1 = header_x0 + header_w;
    let launcher_x0 = header_x1;
    let launcher_x1 = launcher_x0 + launcher_w;
    let timeline_x0 = launcher_x1;
    let timeline_x1 = timeline_x0 + timeline_w;

    let toolbar_y0 = workspace_rect.min.y;
    let toolbar_y1 = toolbar_y0 + BITWIG_TOOLBAR_H;
    let master_y0 = (workspace_rect.max.y - BITWIG_MASTER_H).max(toolbar_y1 + 60.0);
    let master_y1 = workspace_rect.max.y;
    let tracks_y0 = toolbar_y1;
    let tracks_y1 = master_y0;

    let row_h = BITWIG_ROW_H;
    let scroll_y = state.playlist_state.scroll_offset_y;

    let pointer_pos = ui.input(|i| i.pointer.interact_pos().or_else(|| i.pointer.hover_pos()));
    let primary_clicked = workspace_resp.clicked();
    let secondary_clicked = workspace_resp.secondary_clicked();
    let double_clicked = workspace_resp.double_clicked();
    let pointer_released = ui.input(|i| i.pointer.any_released());
    let is_dragging_sample = state.dragged_sample.is_some();

    // Scroll & Zoom handling (Bitwig-style anchored zoom + Shift horizontal pan)
    let timeline_full_rect = Rect::from_min_max(pos2(timeline_x0, toolbar_y0), pos2(timeline_x1, tracks_y1));
    if workspace_resp.hovered() {
        let scroll_delta = ui.input(|i| i.smooth_scroll_delta);
        if scroll_delta.x != 0.0 || scroll_delta.y != 0.0 {
            let ctrl_held = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
            let shift_held = ui.input(|i| i.modifiers.shift);
            if ctrl_held && show_timeline {
                let notches = scroll_delta.y / playlist::PX_PER_NOTCH;
                let old_zoom = state.playlist_state.zoom_x;
                let new_zoom = playlist::smooth_zoom(old_zoom, notches);
                let anchor_x = pointer_pos
                    .map(|p| p.x)
                    .unwrap_or((timeline_x0 + timeline_x1) * 0.5);
                let rel_x = (anchor_x - timeline_x0 - state.playlist_state.scroll_offset_x).max(0.0);
                let anchor_tick = rel_x / old_zoom.max(1e-6);
                state.playlist_state.zoom_x = new_zoom;
                state.playlist_state.scroll_offset_x =
                    (anchor_x - timeline_x0 - anchor_tick * new_zoom).min(0.0);
            } else if shift_held && show_timeline {
                let pan_dx = if scroll_delta.x != 0.0 {
                    scroll_delta.x
                } else {
                    scroll_delta.y
                };
                state.playlist_state.scroll_offset_x =
                    (state.playlist_state.scroll_offset_x + pan_dx).min(0.0);
            } else {
                let max_scroll_y =
                    ((tracks_count as f32 + 1.0) * row_h - (tracks_y1 - tracks_y0)).max(0.0);
                state.playlist_state.scroll_offset_y =
                    (state.playlist_state.scroll_offset_y + scroll_delta.y).clamp(-max_scroll_y, 0.0);
                if show_timeline {
                    state.playlist_state.scroll_offset_x =
                        (state.playlist_state.scroll_offset_x + scroll_delta.x).min(0.0);
                }
            }
        }
    }

    // Middle-Mouse Zoom & Pan + Middle-Click Set Playback Start Time (Bitwig 1:1)
    if show_timeline && timeline_w > 0.0 {
        let mmb_pressed = ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Middle));
        let mmb_down = ui.input(|i| i.pointer.button_down(egui::PointerButton::Middle));
        let shift_held = ui.input(|i| i.modifiers.shift);

        if mmb_pressed {
            if let Some(pos) = pointer_pos {
                if timeline_full_rect.contains(pos) {
                    state.playlist_pan = Some(playlist::PlaylistPanState {
                        start: (pos.x, pos.y),
                        last: (pos.x, pos.y),
                        moved: false,
                        geometry: playlist::PlaylistNavGeometry {
                            header_width: timeline_x0,
                            ruler_h: BITWIG_TOOLBAR_H,
                            total_ticks: state.playlist_state.total_project_ticks(),
                            row_count: tracks_count,
                        },
                    });
                }
            }
        }

        if let Some(mut pan) = state.playlist_pan {
            if mmb_down {
                if let Some(pos) = pointer_pos {
                    let dx = pos.x - pan.last.0;
                    let dy = pos.y - pan.last.1;
                    let dist_sq = (pos.x - pan.start.0).powi(2) + (pos.y - pan.start.1).powi(2);
                    if dist_sq > 9.0 {
                        pan.moved = true;
                    }
                    if dx != 0.0 {
                        state.playlist_state.scroll_offset_x =
                            (state.playlist_state.scroll_offset_x + dx).min(0.0);
                    }
                    if dy != 0.0 {
                        let notches = playlist::mmb_drag_notches(dy);
                        let old_zoom = state.playlist_state.zoom_x;
                        let new_zoom = playlist::smooth_zoom(old_zoom, notches);
                        let rel_x = (pos.x - timeline_x0 - state.playlist_state.scroll_offset_x).max(0.0);
                        let anchor_tick = rel_x / old_zoom.max(1e-6);
                        state.playlist_state.zoom_x = new_zoom;
                        state.playlist_state.scroll_offset_x =
                            (pos.x - timeline_x0 - anchor_tick * new_zoom).min(0.0);
                    }
                    pan.last = (pos.x, pos.y);
                    state.playlist_pan = Some(pan);
                    ui.ctx().set_cursor_icon(egui::CursorIcon::None);
                    crate::app::set_footer_cues(state, None, &[("MIDDLE-DRAG", "Zoom & pan")]);
                }
            } else {
                // Middle button released: if it was a click (not a drag), set playback start time!
                if !pan.moved {
                    let ppqn = state.playlist_state.ppqn.max(1);
                    let beats_per_bar = state.transport.beats_per_bar.max(1);
                    let (adaptive_step, _, _) = playlist::adaptive_grid_step(
                        ppqn,
                        beats_per_bar,
                        state.playlist_state.zoom_x,
                    );
                    let snap = if shift_held { 1 } else { adaptive_step };
                    let rel_px =
                        (pan.start.0 - timeline_x0 - state.playlist_state.scroll_offset_x).max(0.0);
                    let tick = snap_ticks(px_to_ticks(rel_px, state.playlist_state.zoom_x), snap);
                    state.playlist_state.playhead_tick = tick;
                    let sample_count = state.transport.ticks_to_samples(tick);
                    state.transport.sample_count = sample_count;
                    state.audio_proxy.send(GuiCommand::Seek { sample_count });
                }
                state.playlist_pan = None;
            }
        }
    }

    // =========================================================================
    // 1. TOP-LEFT SUB-BAR (Bitwig Layout Toggles + Pointer Tool + Stop All)
    // =========================================================================
    let tl_corner_rect = Rect::from_min_max(pos2(header_x0, toolbar_y0), pos2(header_x1, toolbar_y1));
    painter.rect_filled(tl_corner_rect, 0.0, Color32::from_rgb(0x38, 0x3A, 0x3F));
    painter.line_segment(
        [tl_corner_rect.left_bottom(), tl_corner_rect.right_bottom()],
        Stroke::new(1.0_f32, BW_CAPSULE_BORDER),
    );

    let mut toggle_launcher_clicked = false;
    let mut toggle_timeline_clicked = false;
    let mut stop_all_clicked = false;

    let mut toolbar_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(tl_corner_rect.shrink2(vec2(6.0, 3.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    toolbar_ui.spacing_mut().item_spacing.x = 4.0;
    let (l_resp, t_resp) =
        bitwig_layout_toggles_widget(&mut toolbar_ui, show_launcher, show_timeline);
    if l_resp
        .on_hover_text("Toggle Clip Launcher (Horizontal Matrix)")
        .clicked()
    {
        toggle_launcher_clicked = true;
    }
    if t_resp.on_hover_text("Toggle Arranger Timeline").clicked() {
        toggle_timeline_clicked = true;
    }

    let rem_toolbar_w = (toolbar_ui.available_width() - 44.0).max(0.0);
    toolbar_ui.add_space(rem_toolbar_w);

    // Active Playlist Tool indicator (Bitwig sub-bar right icon)
    let tool_label = match state.playlist_state.active_tool {
        playlist::PlaylistTool::Pointer => "Pointer Tool (1)",
        playlist::PlaylistTool::Time => "Time Selection Tool (2)",
        playlist::PlaylistTool::Pen => "Pen Tool (3)",
        playlist::PlaylistTool::Eraser => "Eraser Tool (4)",
        playlist::PlaylistTool::Knife => "Knife Tool (5)",
    };
    let (ptr_rect, ptr_resp) = toolbar_ui.allocate_exact_size(vec2(18.0, 20.0), Sense::click());
    icons::paint_pointer_cursor(
        toolbar_ui.painter(),
        ptr_rect,
        if state.playlist_state.active_tool == playlist::PlaylistTool::Pointer {
            Color32::from_rgb(0xEE, 0xF0, 0xF4)
        } else {
            ACCENT_ORANGE
        },
    );
    if ptr_resp.on_hover_text(tool_label).clicked() {
        state.playlist_state.active_tool = match state.playlist_state.active_tool {
            playlist::PlaylistTool::Pointer => playlist::PlaylistTool::Time,
            playlist::PlaylistTool::Time => playlist::PlaylistTool::Pen,
            playlist::PlaylistTool::Pen => playlist::PlaylistTool::Eraser,
            playlist::PlaylistTool::Eraser => playlist::PlaylistTool::Knife,
            playlist::PlaylistTool::Knife => playlist::PlaylistTool::Pointer,
        };
    }

    // Stop All Clips vector button (zero Unicode!)
    if bitwig_kit::capsule_icon_button(
        &mut toolbar_ui,
        vec2(20.0, 20.0),
        false,
        ACCENT_ORANGE,
        JoinSide::Standalone,
        |p, r, fg| icons::paint_stop_square(p, r, fg),
    )
    .on_hover_text("Stop All Clips")
    .clicked()
    {
        stop_all_clicked = true;
    }

    if toggle_launcher_clicked {
        state.panel_layout.show_clip_launcher = !state.panel_layout.show_clip_launcher;
        if !state.panel_layout.show_clip_launcher && !state.panel_layout.show_arranger {
            state.panel_layout.show_arranger = true;
        }
        state.mode = if state.panel_layout.show_clip_launcher {
            AppMode::OpenLive
        } else {
            AppMode::OpenStudio
        };
    }
    if toggle_timeline_clicked {
        state.panel_layout.show_arranger = !state.panel_layout.show_arranger;
        if !state.panel_layout.show_arranger && !state.panel_layout.show_clip_launcher {
            state.panel_layout.show_clip_launcher = true;
        }
        state.mode = if state.panel_layout.show_arranger && !state.panel_layout.show_clip_launcher {
            AppMode::OpenStudio
        } else {
            AppMode::OpenLive
        };
    }
    if stop_all_clicked {
        state.matrix_state.stop_all_clips(&state.audio_proxy);
    }

    // =========================================================================
    // 2. TRACK HEADERS COLUMN (Warm Grey Bitwig Strips + `#0A0A0C` Black Void)
    // =========================================================================
    let tracks_col_rect = Rect::from_min_max(pos2(header_x0, tracks_y0), pos2(header_x1, tracks_y1));
    let tracks_painter = painter.with_clip_rect(tracks_col_rect);
    // Pure Bitwig dark void below user tracks!
    tracks_painter.rect_filled(tracks_col_rect, 0.0, BW_DARK_WELL);

    let mut trigger_slot_action: Option<(usize, usize)> = None;
    let mut select_slot_action: Option<(usize, usize)> = None;
    let mut create_midi_slot_action: Option<(usize, usize)> = None;
    let mut add_group_action = false;
    let mut add_fx_action = false;
    let mut group_track_action: Option<usize> = None;
    let mut toggle_group_expand_action: Option<usize> = None;
    let mut trigger_group_scene_action: Option<(usize, usize)> = None; // (group_mixer_idx, scene_idx)

    let visible_tracks: Vec<usize> = (0..tracks_count)
        .filter(|&ti| {
            let mi = mixer_index_of_matrix_track(ti);
            crate::views::mixer::is_track_visible(&state.live_tracks, mi)
        })
        .collect();
    let mut open_piano_roll_action = false;
    let mut toggle_mute_action: Option<usize> = None;
    let mut toggle_solo_action: Option<usize> = None;
    let mut toggle_arm_action: Option<usize> = None;
    let mut stop_track_action: Option<usize> = None;
    let mut trigger_scene_action: Option<usize> = None;
    let mut select_scene_action: Option<usize> = None;
    let mut add_track_action = false;
    let mut delete_track_action: Option<usize> = None;
    let mut add_scene_action = false;
    let mut open_scene_menu_action: Option<(usize, egui::Pos2)> = None;
    let mut drop_slot_action: Option<(usize, usize, PathBuf)> = None;
    let mut drop_timeline_action: Option<(usize, u64, PathBuf)> = None;
    let mut open_context_menu_action: Option<(usize, usize, egui::Pos2)> = None;
    let mut start_matrix_drag_action: Option<(usize, usize, MatrixClip, egui::Vec2)> = None;
    let mut drop_matrix_clip_action: Option<(usize, usize)> = None;

    for (vis_idx, &track_idx) in visible_tracks.iter().enumerate() {
        let row_y0 = tracks_y0 + scroll_y + vis_idx as f32 * row_h;
        let row_y1 = row_y0 + row_h;
        if row_y1 < tracks_y0 || row_y0 > tracks_y1 {
            continue;
        }

        let row_rect = Rect::from_min_max(pos2(header_x0, row_y0), pos2(header_x1, row_y1));
        let mixer_idx = mixer_index_of_matrix_track(track_idx);
        let is_selected = state.selected_track_index == mixer_idx;
        let track_color = state
            .live_tracks
            .get(mixer_idx)
            .map(|t| t.color)
            .unwrap_or_else(|| track_color_for(track_idx));
        let is_armed = state.live_tracks.get(mixer_idx).map(|t| t.arm).unwrap_or(false);
        let has_synth = state
            .live_tracks
            .get(mixer_idx)
            .is_some_and(|t| t.effects.iter().any(|e| e.name.contains("Wavetable") || e.name.contains("DMS")));
        let peak = state.smoothed_track_peaks.get(track_idx).copied().unwrap_or(0.0);

        let (track_name, muted, soloed, vol) = {
            let tm = &state.matrix_state.tracks[track_idx];
            (tm.name.clone(), tm.muted, tm.soloed, tm.volume)
        };

        // Bitwig signature warm anodized grey track header (`#8A8C90` selected, `#67696E` unselected)
        let bg = if is_selected {
            Color32::from_rgb(0x8B, 0x8D, 0x92)
        } else {
            Color32::from_rgb(0x67, 0x69, 0x6E)
        };
        tracks_painter.rect_filled(row_rect, 0.0, bg);
        tracks_painter.line_segment(
            [row_rect.left_top(), row_rect.right_top()],
            Stroke::new(
                1.0_f32,
                if is_selected {
                    Color32::from_rgb(0xA4, 0xA6, 0xAC)
                } else {
                    Color32::from_rgb(0x78, 0x7A, 0x80)
                },
            ),
        );
        tracks_painter.line_segment(
            [row_rect.left_bottom(), row_rect.right_bottom()],
            Stroke::new(1.0_f32, Color32::from_rgb(0x16, 0x17, 0x1A)),
        );

        let group_depth = crate::views::mixer::track_group_depth(&state.live_tracks, mixer_idx);
        let is_group = state.live_tracks.get(mixer_idx).is_some_and(|t| t.is_group);
        let is_expanded = state.live_tracks.get(mixer_idx).is_some_and(|t| t.is_expanded);
        let parent_group_color = state
            .live_tracks
            .get(mixer_idx)
            .and_then(|t| t.parent_group_id)
            .and_then(|pid| state.live_tracks.iter().find(|p| p.id == pid))
            .map(|p| p.color);
        let indent_x = group_depth as f32 * 8.0;

        // If inside a group, draw the outer group color rail on the left
        if let Some(gcol) = parent_group_color {
            tracks_painter.rect_filled(
                Rect::from_min_size(row_rect.min, vec2(4.0, row_h)),
                0.0,
                gcol,
            );
        }

        // 5px solid Bitwig Track Color Bar on left edge (indented if inside a group)
        tracks_painter.rect_filled(
            Rect::from_min_size(pos2(row_rect.min.x + indent_x, row_rect.min.y), vec2(5.0, row_h - 1.0)),
            0.0,
            track_color,
        );

        let mut title_x = header_x0 + indent_x + 11.0;
        let folder_rect = Rect::from_min_size(pos2(title_x - 2.0, row_y0 + 4.0), vec2(16.0, 16.0));
        if is_group {
            let arrow = if is_expanded { "▼" } else { "▶" };
            tracks_painter.text(
                pos2(title_x, row_y0 + 5.0),
                egui::Align2::LEFT_TOP,
                arrow,
                fonts::semibold(10.0),
                Color32::from_rgb(0x10, 0x11, 0x13),
            );
            title_x += 13.0;
            // Small folder badge
            let f_rect = Rect::from_min_size(pos2(title_x, row_y0 + 6.0), vec2(12.0, 9.0));
            tracks_painter.rect_filled(f_rect, 1.5, Color32::from_rgb(0x1E, 0x20, 0x24));
            tracks_painter.rect_filled(
                Rect::from_min_size(pos2(f_rect.min.x, f_rect.min.y - 2.0), vec2(5.0, 3.0)),
                1.0,
                Color32::from_rgb(0x1E, 0x20, 0x24),
            );
            title_x += 16.0;
        }

        // Track Title in crisp dark `#101113` ink (exact to `bitwig-clip-launcher-and-playlist.png`)
        tracks_painter.text(
            pos2(title_x, row_y0 + 5.0),
            egui::Align2::LEFT_TOP,
            &track_name,
            fonts::semibold(11.5),
            Color32::from_rgb(0x10, 0x11, 0x13),
        );

        // Joined 3-Segment GPU `[● | S | M]` Capsules on top-right (before 22px Stop column)
        let btn_w = 18.0;
        let btn_h = 16.0;
        let btn_y = row_y0 + 4.0;
        let m_rect = Rect::from_min_size(pos2(header_x1 - 24.0 - btn_w, btn_y), vec2(btn_w, btn_h));
        let s_rect = Rect::from_min_size(pos2(m_rect.min.x - btn_w, btn_y), vec2(btn_w, btn_h));
        let r_rect = Rect::from_min_size(pos2(s_rect.min.x - btn_w, btn_y), vec2(btn_w, btn_h));

        let r_hov = pointer_pos.is_some_and(|p| tracks_col_rect.contains(p) && r_rect.contains(p));
        let s_hov = pointer_pos.is_some_and(|p| tracks_col_rect.contains(p) && s_rect.contains(p));
        let m_hov = pointer_pos.is_some_and(|p| tracks_col_rect.contains(p) && m_rect.contains(p));

        for (rect, active, act_col, join, hov) in [
            (r_rect, is_armed, REC_RED, JoinSide::Left, r_hov),
            (s_rect, soloed, SOLO_YELLOW, JoinSide::Middle, s_hov),
            (m_rect, muted, MUTE_BLUE, JoinSide::Right, m_hov),
        ] {
            if rect.min.y >= tracks_y0 && rect.max.y <= tracks_y1 {
                let mut flags = join.to_flags();
                if active {
                    flags |= bitwig_wgpu::FLAG_ACTIVE;
                }
                if hov {
                    flags |= bitwig_wgpu::FLAG_HOVERED;
                }
                bitwig_wgpu::paint_capsule(
                    &tracks_painter,
                    rect,
                    3.0,
                    Color32::from_rgb(0x46, 0x48, 0x4E),
                    act_col,
                    flags,
                );
            } else {
                tracks_painter.rect_filled(
                    rect,
                    2.0,
                    if active {
                        act_col
                    } else {
                        Color32::from_rgb(0x46, 0x48, 0x4E)
                    },
                );
            }
        }
        // Paint `[● | S | M]` foregrounds on top of GPU capsules
        icons::paint_record_circle(
            &tracks_painter,
            r_rect.shrink(3.5),
            if is_armed {
                Color32::from_rgb(0x11, 0x11, 0x13)
            } else {
                Color32::from_rgb(0xE0, 0xE2, 0xE8)
            },
            true,
        );
        tracks_painter.text(
            s_rect.center(),
            egui::Align2::CENTER_CENTER,
            "S",
            fonts::semibold(9.5),
            if soloed {
                Color32::from_rgb(0x11, 0x11, 0x13)
            } else {
                Color32::from_rgb(0xE4, 0xE6, 0xEC)
            },
        );
        tracks_painter.text(
            m_rect.center(),
            egui::Align2::CENTER_CENTER,
            "M",
            fonts::semibold(9.5),
            if muted {
                Color32::from_rgb(0x11, 0x11, 0x13)
            } else {
                Color32::from_rgb(0xE4, 0xE6, 0xEC)
            },
        );

        // Bottom row of Track Header: GPU Copper-Amber Horizontal Fader + Stereo Peak Bar + Track Icon
        let slider_rect = Rect::from_min_size(pos2(header_x0 + 11.0, row_y0 + 26.0), vec2(114.0, 11.0));
        let slider_hov =
            pointer_pos.is_some_and(|p| tracks_col_rect.contains(p) && slider_rect.expand(3.0).contains(p));
        if slider_rect.min.y >= tracks_y0 && slider_rect.max.y <= tracks_y1 {
            bitwig_wgpu::paint_fader_horizontal(
                &tracks_painter,
                slider_rect,
                vol.clamp(0.0, 1.0),
                BW_FADER_AMBER,
                BW_FADER_ORANGE,
                slider_hov,
                false,
            );
        } else {
            tracks_painter.rect_filled(slider_rect, 2.0, BW_SUNKEN_DARK);
            tracks_painter.rect_filled(
                Rect::from_min_size(
                    slider_rect.min,
                    vec2(slider_rect.width() * vol.clamp(0.0, 1.0), slider_rect.height()),
                ),
                2.0,
                BW_FADER_AMBER,
            );
        }

        // Peak meter bar under horizontal volume fader
        let meter_rect = Rect::from_min_size(pos2(header_x0 + 11.0, row_y0 + 40.0), vec2(114.0, 4.0));
        tracks_painter.rect_filled(meter_rect, 1.0, Color32::from_rgb(0x18, 0x19, 0x1C));
        if peak > 0.002 {
            let pw = meter_rect.width() * peak.clamp(0.0, 1.0);
            let p_col = if peak > 0.88 {
                REC_RED
            } else if peak > 0.68 {
                SOLO_YELLOW
            } else {
                PLAY_GREEN
            };
            tracks_painter.rect_filled(
                Rect::from_min_size(meter_rect.min, vec2(pw, meter_rect.height())),
                1.0,
                p_col,
            );
        }

        // Track type vector icon (Piano for instrument track, Sine wave for audio track)
        let type_icon_rect =
            Rect::from_min_size(pos2(header_x1 - 42.0, row_y0 + 28.0), vec2(14.0, 14.0));
        icons::paint_icon(
            &tracks_painter,
            type_icon_rect,
            if has_synth || track_idx == 0 {
                icons::PIANO
            } else {
                icons::WAVE_SINE
            },
            Color32::from_rgb(0x23, 0x25, 0x29),
            1.6,
        );

        // Right-edge Track Stop Clip column (22px wide, slate background + crisp vector `■`)
        let stop_col_rect =
            Rect::from_min_max(pos2(header_x1 - 22.0, row_y0), pos2(header_x1, row_y1 - 1.0));
        let stop_hov =
            pointer_pos.is_some_and(|p| tracks_col_rect.contains(p) && stop_col_rect.contains(p));
        tracks_painter.rect_filled(
            stop_col_rect,
            0.0,
            if stop_hov {
                Color32::from_rgb(0x5E, 0x60, 0x66)
            } else {
                Color32::from_rgb(0x4C, 0x4E, 0x54)
            },
        );
        tracks_painter.line_segment(
            [stop_col_rect.left_top(), stop_col_rect.left_bottom()],
            Stroke::new(1.0_f32, Color32::from_rgb(0x22, 0x24, 0x28)),
        );
        let stop_btn_rect = Rect::from_center_size(stop_col_rect.center(), vec2(12.0, 12.0));
        icons::paint_stop_square(
            &tracks_painter,
            stop_btn_rect,
            Color32::from_rgb(0x16, 0x17, 0x1A),
        );

        // Hit testing on Track Header
        if let Some(pos) = pointer_pos {
            if tracks_col_rect.contains(pos) && row_rect.contains(pos) {
                if primary_clicked {
                    if is_group && folder_rect.contains(pos) {
                        toggle_group_expand_action = Some(mixer_idx);
                    } else if r_rect.contains(pos) {
                        toggle_arm_action = Some(track_idx);
                    } else if s_rect.contains(pos) {
                        toggle_solo_action = Some(track_idx);
                    } else if m_rect.contains(pos) {
                        toggle_mute_action = Some(track_idx);
                    } else if stop_col_rect.contains(pos) {
                        stop_track_action = Some(track_idx);
                    } else {
                        select_matrix_track(state, track_idx);
                    }
                }
                if secondary_clicked {
                    select_matrix_track(state, track_idx);
                    crate::ui::track_context_menu::open(
                        ui.ctx(),
                        egui::Id::new(("matrix_trk_ctx", track_idx)),
                        pos,
                    );
                }
                if double_clicked
                    && !r_rect.contains(pos)
                    && !s_rect.contains(pos)
                    && !m_rect.contains(pos)
                {
                    if slider_rect.expand(4.0).contains(pos) {
                        state.matrix_state.tracks[track_idx].volume = 0.75;
                        if let Some(tr) = state.live_tracks.get_mut(mixer_idx) {
                            tr.volume = 0.75;
                        }
                        state.audio_proxy.send(GuiCommand::SetTrackVolume {
                            track_idx,
                            volume_db: 0.75,
                        });
                    } else {
                        select_matrix_track(state, track_idx);
                        state.show_dsp_rack = !state.show_dsp_rack;
                    }
                }
                // Dragging horizontal GPU volume fader
                if workspace_resp.dragged_by(egui::PointerButton::Primary)
                    && slider_rect.expand(4.0).contains(pos)
                {
                    let new_vol =
                        ((pos.x - slider_rect.min.x) / slider_rect.width()).clamp(0.0, 1.0);
                    state.matrix_state.tracks[track_idx].volume = new_vol;
                    if let Some(tr) = state.live_tracks.get_mut(mixer_idx) {
                        tr.volume = new_vol;
                    }
                    state.audio_proxy.send(GuiCommand::SetTrackVolume {
                        track_idx,
                        volume_db: new_vol,
                    });
                }
            }
        }

        // Menú contextual (click derecho) estilo Bitwig en el Clip Launcher.
        {
            use crate::ui::track_context_menu as ctxm;
            let menu_key = egui::Id::new(("matrix_trk_ctx", track_idx));
            let ectx = ui.ctx();
            let mon_id = menu_key.with("monitor_mode");
            let monitor = ectx
                .data(|d| d.get_temp::<u8>(mon_id))
                .map(|v| match v {
                    0 => ctxm::MonitorMode::Off,
                    2 => ctxm::MonitorMode::On,
                    _ => ctxm::MonitorMode::Auto,
                })
                .unwrap_or(ctxm::MonitorMode::Auto);
            let is_group_track = state.live_tracks.get(mixer_idx).is_some_and(|t| t.is_group);
            let is_in_group = state.live_tracks.get(mixer_idx).is_some_and(|t| t.parent_group_id.is_some());
            let mc = ctxm::MenuContext {
                current_color: track_color,
                monitor,
                can_enter_group: is_group_track,
                can_exit_group: is_in_group || is_group_track,
                ..Default::default()
            };
            match ctxm::show(ectx, menu_key, &mc) {
                Some(ctxm::MenuAction::SetColor(c)) => {
                    if let Some(tr) = state.live_tracks.get_mut(mixer_idx) {
                        tr.color = c;
                    }
                }
                Some(ctxm::MenuAction::ToggleActive) => {
                    toggle_mute_action = Some(track_idx);
                }
                Some(ctxm::MenuAction::Delete) => {
                    delete_track_action = Some(mixer_idx);
                }
                Some(ctxm::MenuAction::AddInstrumentTrack) => {
                    add_track_action = true;
                }
                Some(ctxm::MenuAction::AddFxTrack) => {
                    add_fx_action = true;
                }
                Some(ctxm::MenuAction::AddGroupTrack) => {
                    add_group_action = true;
                }
                Some(ctxm::MenuAction::Group) => {
                    group_track_action = Some(mixer_idx);
                }
                Some(ctxm::MenuAction::EnterGroup) | Some(ctxm::MenuAction::ExitGroup) => {
                    if is_group_track {
                        toggle_group_expand_action = Some(mixer_idx);
                    } else if let Some(pid) = state.live_tracks.get(mixer_idx).and_then(|t| t.parent_group_id) {
                        if let Some(p_idx) = state.live_tracks.iter().position(|t| t.id == pid) {
                            toggle_group_expand_action = Some(p_idx);
                        }
                    }
                }
                Some(ctxm::MenuAction::SetMonitor(m)) => {
                    let v = match m {
                        ctxm::MonitorMode::Off => 0u8,
                        ctxm::MonitorMode::Auto => 1,
                        ctxm::MonitorMode::On => 2,
                    };
                    ectx.data_mut(|d| d.insert_temp(mon_id, v));
                }
                _ => {}
            }
        }
    }

    // Compact Bitwig `+` Add Track button inside the `#0A0A0C` black void below last track
    let add_track_y0 = tracks_y0 + scroll_y + visible_tracks.len() as f32 * row_h;
    if add_track_y0 + 26.0 < tracks_y1 {
        let add_tr_rect = Rect::from_min_size(
            pos2((header_x0 + header_x1 - 20.0) * 0.5, add_track_y0 + 4.0),
            vec2(20.0, 20.0),
        );
        let add_hov =
            pointer_pos.is_some_and(|p| tracks_col_rect.contains(p) && add_tr_rect.contains(p));
        tracks_painter.rect_filled(
            add_tr_rect,
            2.0,
            if add_hov {
                Color32::from_rgb(0x30, 0x32, 0x38)
            } else {
                Color32::from_rgb(0x18, 0x19, 0x1D)
            },
        );
        icons::paint_icon(
            &tracks_painter,
            add_tr_rect.shrink(4.0),
            icons::PLUS,
            if add_hov {
                Color32::WHITE
            } else {
                Color32::from_rgb(0x98, 0x9C, 0xA4)
            },
            1.8,
        );
        if primary_clicked && add_hov {
            add_track_action = true;
        }
    }

    // =========================================================================
    // Pinned Master Track Row across the bottom (`master_y0..master_y1`)
    // =========================================================================
    let master_is_sel = state.selected_track_index == 0;
    let master_header_rect =
        Rect::from_min_max(pos2(header_x0, master_y0), pos2(header_x1, master_y1));
    painter.rect_filled(
        master_header_rect,
        0.0,
        if master_is_sel {
            Color32::from_rgb(0x8B, 0x8D, 0x92)
        } else {
            Color32::from_rgb(0x67, 0x69, 0x6E)
        },
    );
    painter.rect_filled(
        Rect::from_min_size(master_header_rect.min, vec2(5.0, master_header_rect.height())),
        0.0,
        Color32::from_rgb(0x92, 0x96, 0x9E),
    );
    painter.text(
        pos2(header_x0 + 11.0, master_y0 + 5.0),
        egui::Align2::LEFT_TOP,
        "Master",
        fonts::semibold(11.5),
        Color32::from_rgb(0x10, 0x11, 0x13),
    );
    icons::paint_crown(
        &painter,
        Rect::from_min_size(pos2(header_x1 - 24.0, master_y0 + 5.0), vec2(14.0, 12.0)),
        Color32::from_rgb(0x22, 0x24, 0x28),
    );
    let master_vol = state.live_tracks.first().map(|t| t.volume).unwrap_or(0.75);
    let master_slider =
        Rect::from_min_size(pos2(header_x0 + 11.0, master_y0 + 24.0), vec2(118.0, 11.0));
    let master_slider_hov = pointer_pos.is_some_and(|p| master_slider.expand(3.0).contains(p));
    bitwig_wgpu::paint_fader_horizontal(
        &painter,
        master_slider,
        master_vol.clamp(0.0, 1.0),
        BW_FADER_AMBER,
        BW_FADER_ORANGE,
        master_slider_hov,
        false,
    );
    if let Some(pos) = pointer_pos {
        if master_header_rect.contains(pos) {
            if primary_clicked {
                state.selected_track_index = 0;
            }
            if workspace_resp.dragged_by(egui::PointerButton::Primary)
                && master_slider.expand(4.0).contains(pos)
            {
                let nv = ((pos.x - master_slider.min.x) / master_slider.width()).clamp(0.0, 1.0);
                if let Some(mt) = state.live_tracks.first_mut() {
                    mt.volume = nv;
                }
                state.audio_proxy.send(GuiCommand::SetTrackVolume {
                    track_idx: 0,
                    volume_db: nv,
                });
            }
        }
    }

    // Master row lane continuation across Launcher & Timeline
    let master_right_rect =
        Rect::from_min_max(pos2(header_x1, master_y0), pos2(workspace_rect.max.x, master_y1));
    painter.rect_filled(master_right_rect, 0.0, Color32::from_rgb(0x52, 0x54, 0x59));
    painter.line_segment(
        [master_header_rect.left_top(), pos2(workspace_rect.max.x, master_y0)],
        Stroke::new(1.5_f32, Color32::from_rgb(0x11, 0x12, 0x14)),
    );
    painter.line_segment(
        [pos2(header_x1, toolbar_y0), pos2(header_x1, workspace_rect.max.y)],
        Stroke::new(1.0_f32, Color32::from_rgb(0x11, 0x12, 0x14)),
    );

    // =========================================================================
    // 3. HORIZONTAL CLIP LAUNCHER (When `show_launcher == true`)
    // =========================================================================
    if show_launcher && launcher_w > 0.0 {
        let scene_header_rect =
            Rect::from_min_max(pos2(launcher_x0, toolbar_y0), pos2(launcher_x1, toolbar_y1));
        let scene_painter = painter.with_clip_rect(scene_header_rect);
        scene_painter.rect_filled(scene_header_rect, 0.0, Color32::from_rgb(0x28, 0x2A, 0x2E));
        scene_painter.line_segment(
            [scene_header_rect.left_bottom(), scene_header_rect.right_bottom()],
            Stroke::new(1.0_f32, BW_CAPSULE_BORDER),
        );

        // Scene Column Headers (`[▶ Scene 1]`, `[▶ Scene 2]`...) with selection border & play triangle
        for scene_idx in 0..scenes_count {
            let sx0 = launcher_x0 + scene_idx as f32 * BITWIG_SCENE_W;
            let sx1 = sx0 + BITWIG_SCENE_W;
            if sx0 > launcher_x1 {
                break;
            }
            let s_rect = Rect::from_min_max(
                pos2(sx0 + 1.5, toolbar_y0 + 2.0),
                pos2(sx1 - 1.5, toolbar_y1 - 2.0),
            );
            let s_hovered =
                pointer_pos.is_some_and(|p| scene_header_rect.contains(p) && s_rect.contains(p));
            let play_hit_rect = Rect::from_min_max(
                s_rect.min,
                pos2((s_rect.min.x + 18.0).min(s_rect.max.x), s_rect.max.y),
            );
            let play_hovered = s_hovered && pointer_pos.is_some_and(|p| play_hit_rect.contains(p));
            let is_queued = state.queued_scene_index == Some(scene_idx);
            let is_selected = state.matrix_state.selected_scene == Some(scene_idx);
            let (s_name, q_override, custom_color) = state
                .matrix_state
                .scenes
                .get(scene_idx)
                .map(|s| (s.name.as_str(), s.launch_quantization, s.color))
                .unwrap_or(("Scene", None, None));

            let (s_bg, s_txt_col, play_col) = scene_header_colors(
                custom_color,
                is_queued,
                s_hovered,
                is_selected,
                play_hovered,
            );
            scene_painter.rect_filled(s_rect, 1.5, s_bg);
            if is_selected {
                scene_painter.rect_stroke(
                    s_rect,
                    1.5,
                    Stroke::new(1.5_f32, Color32::WHITE),
                    egui::StrokeKind::Inside,
                );
            } else if is_queued {
                scene_painter.rect_stroke(
                    s_rect,
                    1.5,
                    Stroke::new(1.2_f32, ACCENT_ORANGE),
                    egui::StrokeKind::Inside,
                );
            }
            let play_r = Rect::from_min_size(
                pos2(s_rect.min.x + 4.0, s_rect.center().y - 6.0),
                vec2(12.0, 12.0),
            );
            icons::paint_play_triangle(&scene_painter, play_r, play_col);
            scene_painter.text(
                pos2(s_rect.min.x + 18.0, s_rect.center().y),
                egui::Align2::LEFT_CENTER,
                s_name,
                fonts::semibold(10.5),
                s_txt_col,
            );
            if let Some(ov) = q_override {
                let badge_r = Rect::from_min_max(
                    pos2(s_rect.max.x - 27.0, s_rect.min.y + 3.0),
                    pos2(s_rect.max.x - 3.0, s_rect.max.y - 3.0),
                );
                scene_painter.rect_filled(badge_r, 2.0, Color32::from_rgb(0x23, 0x26, 0x2C));
                scene_painter.text(
                    badge_r.center(),
                    egui::Align2::CENTER_CENTER,
                    ov.short_label(),
                    fonts::semibold(8.5),
                    ACCENT_ORANGE,
                );
            }

            if double_clicked && s_hovered {
                select_scene_action = Some(scene_idx);
                trigger_scene_action = Some(scene_idx);
            } else if primary_clicked && s_hovered {
                select_scene_action = Some(scene_idx);
                if play_hovered {
                    trigger_scene_action = Some(scene_idx);
                }
            } else if secondary_clicked && s_hovered {
                select_scene_action = Some(scene_idx);
                let menu_pos = pointer_pos.unwrap_or(pos2(s_rect.min.x, s_rect.max.y + 2.0));
                open_scene_menu_action = Some((scene_idx, menu_pos));
            }
        }

        // Vector `+` button at end of Scene headers (Bitwig Studio style — no `-` button; use Delete on selected scene)
        let add_scene_x = launcher_x0 + scenes_count as f32 * BITWIG_SCENE_W + 3.0;
        if add_scene_x + 22.0 <= launcher_x1 {
            let add_s_rect = Rect::from_min_size(
                pos2(add_scene_x, toolbar_y0 + 3.0),
                vec2(20.0, BITWIG_TOOLBAR_H - 6.0),
            );
            let add_s_hov = pointer_pos.is_some_and(|p| add_s_rect.contains(p));
            if add_s_hov {
                scene_painter.rect_filled(
                    add_s_rect,
                    2.0,
                    Color32::from_rgb(0x44, 0x46, 0x4E),
                );
            }
            icons::paint_icon(
                &scene_painter,
                add_s_rect.shrink(4.5),
                icons::PLUS,
                if add_s_hov {
                    Color32::WHITE
                } else {
                    Color32::from_rgb(0xB8, 0xBA, 0xC2)
                },
                1.6,
            );
            if primary_clicked && add_s_hov {
                add_scene_action = true;
            }
        }

        // Horizontal Clip Slots Grid (Rows = Tracks, Cols = Scenes; `#0A0A0C` void below tracks!)
        let launcher_grid_rect =
            Rect::from_min_max(pos2(launcher_x0, tracks_y0), pos2(launcher_x1, tracks_y1));
        let lg_painter = painter.with_clip_rect(launcher_grid_rect);
        lg_painter.rect_filled(launcher_grid_rect, 0.0, BW_DARK_WELL);

        for (vis_idx, &track_idx) in visible_tracks.iter().enumerate() {
            let row_y0 = tracks_y0 + scroll_y + vis_idx as f32 * row_h;
            let row_y1 = row_y0 + row_h;
            if row_y1 < tracks_y0 || row_y0 > tracks_y1 {
                continue;
            }

            let mixer_idx = mixer_index_of_matrix_track(track_idx);
            let track_color = state
                .live_tracks
                .get(mixer_idx)
                .map(|t| t.color)
                .unwrap_or_else(|| track_color_for(track_idx));
            let is_armed = state.live_tracks.get(mixer_idx).map(|t| t.arm).unwrap_or(false);
            let is_group = state.live_tracks.get(mixer_idx).is_some_and(|t| t.is_group);

            // Track row slot bed
            lg_painter.rect_filled(
                Rect::from_min_max(pos2(launcher_x0, row_y0), pos2(launcher_x1, row_y1)),
                0.0,
                Color32::from_rgb(0x1E, 0x1F, 0x23),
            );
            lg_painter.line_segment(
                [pos2(launcher_x0, row_y1), pos2(launcher_x1, row_y1)],
                Stroke::new(1.0_f32, Color32::from_rgb(0x10, 0x11, 0x13)),
            );

            for scene_idx in 0..scenes_count {
                let sx0 = launcher_x0 + scene_idx as f32 * BITWIG_SCENE_W;
                let sx1 = sx0 + BITWIG_SCENE_W;
                if sx0 > launcher_x1 {
                    break;
                }

                let cell_rect = Rect::from_min_max(
                    pos2(sx0 + 1.5, row_y0 + 1.5),
                    pos2(sx1 - 1.5, row_y1 - 2.0),
                );
                let slot = state.matrix_state.slot(track_idx, scene_idx);
                let slot_state = slot.map(|s| s.state.clone()).unwrap_or(SlotState::Empty);
                let clip_opt = slot.and_then(|s| s.clip.as_ref());
                let has_clip = clip_opt.is_some();
                let is_selected_slot =
                    state.matrix_state.selected_slot == Some((track_idx, scene_idx));
                let cell_hovered = pointer_pos
                    .is_some_and(|p| launcher_grid_rect.contains(p) && cell_rect.contains(p));

                if is_group {
                    let g_bg = if cell_hovered {
                        Color32::from_rgb(0x52, 0x54, 0x5A)
                    } else {
                        Color32::from_rgb(0x42, 0x44, 0x4A)
                    };
                    lg_painter.rect_filled(cell_rect, 2.0, g_bg);
                    let tri_rect = Rect::from_min_size(
                        pos2(cell_rect.min.x + 6.0, cell_rect.center().y - 4.0),
                        vec2(7.0, 8.0),
                    );
                    icons::paint_play_triangle(&lg_painter, tri_rect, Color32::from_rgb(0xAA, 0xAC, 0xB2));
                    let scene_label = state
                        .matrix_state
                        .scenes
                        .get(scene_idx)
                        .map(|s| s.name.as_str())
                        .unwrap_or("Scene");
                    lg_painter.text(
                        pos2(tri_rect.max.x + 5.0, cell_rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        scene_label,
                        fonts::regular(11.0),
                        Color32::from_rgb(0xD2, 0xD4, 0xDA),
                    );
                    if cell_hovered && primary_clicked {
                        trigger_group_scene_action = Some((mixer_idx, scene_idx));
                    }
                    continue;
                }

                let is_drag_source = state.matrix_state.clip_drag.as_ref().is_some_and(|d| {
                    d.from_track == track_idx && d.from_scene == scene_idx
                });
                let is_matrix_drag_target = state.matrix_state.clip_drag.is_some() && cell_hovered;

                if let Some(clip) = clip_opt {
                    // Populated Bitwig Clip Card
                    let is_playing = slot_state == SlotState::Playing;
                    let effective_color = clip.color.unwrap_or(track_color);
                    let card_bg = if is_drag_source {
                        Color32::from_rgba_unmultiplied(30, 32, 36, 110)
                    } else if clip.muted {
                        Color32::from_rgb(0x28, 0x2A, 0x2E)
                    } else {
                        pad_body_color(effective_color, 242)
                    };
                    lg_painter.rect_filled(cell_rect, 2.5, card_bg);

                    // Top colored header strip (16px tall) with vector play/stop icon
                    let top_strip =
                        Rect::from_min_size(cell_rect.min, vec2(cell_rect.width(), 16.0));
                    let strip_col = if is_drag_source {
                        Color32::from_rgba_unmultiplied(
                            effective_color.r() / 3,
                            effective_color.g() / 3,
                            effective_color.b() / 3,
                            120,
                        )
                    } else if clip.muted {
                        Color32::from_rgb(0x40, 0x43, 0x48)
                    } else {
                        effective_color
                    };
                    lg_painter.rect_filled(top_strip, 2.5, strip_col);

                    let ic_rect = Rect::from_min_size(
                        pos2(top_strip.min.x + 3.0, top_strip.min.y + 2.0),
                        vec2(12.0, 12.0),
                    );
                    if is_playing {
                        icons::paint_stop_square(
                            &lg_painter,
                            ic_rect,
                            Color32::from_rgb(0x10, 0x10, 0x12),
                        );
                    } else {
                        icons::paint_play_triangle(
                            &lg_painter,
                            ic_rect,
                            Color32::from_rgb(0x10, 0x10, 0x12),
                        );
                    }
                    lg_painter.text(
                        pos2(top_strip.min.x + 17.0, top_strip.center().y),
                        egui::Align2::LEFT_CENTER,
                        &clip.name,
                        fonts::semibold(9.5),
                        if is_drag_source {
                            Color32::from_rgba_unmultiplied(16, 16, 18, 120)
                        } else {
                            Color32::from_rgb(0x10, 0x10, 0x12)
                        },
                    );

                    // Mini waveform or Mini-Notes inside slot body
                    match &clip.content {
                        ClipData::Audio { .. } => {
                            let wave_rect = Rect::from_min_max(
                                pos2(cell_rect.min.x + 2.0, cell_rect.min.y + 17.5),
                                pos2(cell_rect.max.x - 2.0, cell_rect.max.y - 2.5),
                            );
                            let alpha_scale = if is_drag_source { 0.30 } else { 1.0 };
                            paint_pad_waveform(
                                &lg_painter,
                                wave_rect,
                                &clip.peaks,
                                effective_color,
                                clip.muted,
                                alpha_scale,
                            );
                        }
                        ClipData::Midi { notes } => {
                            if !notes.is_empty() {
                                let body_rect = Rect::from_min_max(
                                    pos2(cell_rect.min.x + 3.0, cell_rect.min.y + 18.0),
                                    pos2(cell_rect.max.x - 3.0, cell_rect.max.y - 3.0),
                                );
                                let max_t = (clip.loop_end.max(3840)) as f32;
                                let min_p = notes.iter().map(|(_, p, _, _)| *p).min().unwrap_or(36).saturating_sub(2);
                                let max_p = notes.iter().map(|(_, p, _, _)| *p).max().unwrap_or(84).saturating_add(2);
                                let p_range = (max_p.saturating_sub(min_p)).max(12) as f32;
                                let note_alpha = if is_drag_source { 70 } else { 220 };

                                for &(st, pitch, _, dur) in notes.iter().take(120) {
                                    let nx = body_rect.min.x + (st as f32 / max_t) * body_rect.width();
                                    let nw = ((dur as f32 / max_t) * body_rect.width()).clamp(2.0, body_rect.width());
                                    let norm_pitch = (pitch.saturating_sub(min_p) as f32 / p_range).clamp(0.0, 1.0);
                                    let ny = body_rect.max.y - norm_pitch * (body_rect.height() - 3.0) - 2.0;

                                    if nx < body_rect.max.x {
                                        let note_rect = Rect::from_min_size(
                                            pos2(nx, ny),
                                            vec2(nw.min(body_rect.max.x - nx), 2.0),
                                        );
                                        lg_painter.rect_filled(
                                            note_rect,
                                            0.5,
                                            Color32::from_rgba_unmultiplied(255, 255, 255, note_alpha),
                                        );
                                    }
                                }
                            }
                        }
                    }

                    // Internal clip playhead line when actively playing
                    if is_playing {
                        if let Some(frame) = state.clip_playhead_frame(track_idx, scene_idx) {
                            let engine_sr = state.transport.sample_rate.get().max(1.0) as f64;
                            let norm_pos = match &clip.content {
                                ClipData::Audio { preview_sr, .. } => {
                                    let playhead_secs = frame as f64 / engine_sr;
                                    let total_secs = if clip.loop_end > 0 && *preview_sr > 0 {
                                        clip.loop_end as f64 / (*preview_sr as f64)
                                    } else {
                                        clip.duration_secs
                                    };
                                    if total_secs > 0.0 {
                                        Some((playhead_secs / total_secs).clamp(0.0, 1.0))
                                    } else {
                                        None
                                    }
                                }
                                ClipData::Midi { .. } => {
                                    let bpm = state.transport.bpm.max(20.0) as f64;
                                    let spb = 60.0 / bpm;
                                    let playhead_ticks = ((frame as f64 / engine_sr) / spb) * 960.0;
                                    let max_t = clip.loop_end.max(3840) as f64;
                                    Some((playhead_ticks / max_t).clamp(0.0, 1.0))
                                }
                            };
                            if let Some(t) = norm_pos {
                                let x0 = cell_rect.min.x + 2.0;
                                let x1 = cell_rect.max.x - 2.0;
                                let px = x0 + (x1 - x0) * t as f32;
                                let py0 = cell_rect.min.y + 16.5;
                                let py1 = cell_rect.max.y - 1.5;
                                lg_painter.line_segment(
                                    [pos2(px, py0), pos2(px, py1)],
                                    Stroke::new(1.5_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 230)),
                                );
                            }
                        }
                    }

                    let border_col = if is_matrix_drag_target {
                        ACCENT_ORANGE
                    } else if is_drag_source {
                        Color32::from_rgba_unmultiplied(100, 102, 108, 120)
                    } else if is_playing {
                        PLAY_GREEN
                    } else if is_selected_slot {
                        Color32::WHITE
                    } else {
                        Color32::from_rgb(0x14, 0x15, 0x18)
                    };
                    lg_painter.rect_stroke(
                        cell_rect,
                        2.5,
                        Stroke::new(
                            if is_matrix_drag_target || is_playing || is_selected_slot {
                                1.5_f32
                            } else {
                                1.0_f32
                            },
                            border_col,
                        ),
                        egui::StrokeKind::Inside,
                    );
                } else {
                    // Empty Bitwig Slot: gris neutro #2E2E2E como en Bitwig
                    // (antes tenía un tinte azulado/slate más oscuro).
                    let empty_bg = if is_matrix_drag_target {
                        Color32::from_rgba_unmultiplied(255, 138, 0, 40)
                    } else if cell_hovered {
                        Color32::from_rgb(0x3A, 0x3A, 0x3A)
                    } else {
                        Color32::from_rgb(0x2E, 0x2E, 0x2E)
                    };
                    lg_painter.rect_filled(cell_rect, 2.0, empty_bg);
                    if is_selected_slot || is_matrix_drag_target {
                        lg_painter.rect_stroke(
                            cell_rect,
                            2.0,
                            Stroke::new(1.5_f32, ACCENT_ORANGE),
                            egui::StrokeKind::Inside,
                        );
                    }

                    let ic_rect = Rect::from_min_size(
                        pos2(cell_rect.min.x + 5.0, cell_rect.center().y - 6.0),
                        vec2(12.0, 12.0),
                    );
                    let has_stop_btn = slot.map(|s| s.has_stop_button).unwrap_or(true);
                    if is_armed {
                        icons::paint_record_circle(&lg_painter, ic_rect, REC_RED, false);
                    } else if has_stop_btn {
                        // Mismo gris medio que usa Bitwig para el cuadradito
                        // del slot vacío.
                        icons::paint_stop_square(
                            &lg_painter,
                            ic_rect,
                            Color32::from_rgb(0x53, 0x53, 0x53),
                        );
                    }
                }

                // Drag Target Highlight when a clip is being dragged over this slot
                if is_matrix_drag_target {
                    lg_painter.rect_stroke(
                        cell_rect,
                        2.5,
                        Stroke::new(2.0_f32, ACCENT_ORANGE),
                        egui::StrokeKind::Inside,
                    );
                    lg_painter.rect_filled(
                        cell_rect,
                        2.5,
                        Color32::from_rgba_unmultiplied(255, 138, 0, 35),
                    );
                }

                // Highlight slot when dragging a WAV sample from explorer over it
                if is_dragging_sample && cell_hovered {
                    lg_painter.rect_stroke(
                        cell_rect,
                        2.5,
                        Stroke::new(2.0_f32, ACCENT_ORANGE),
                        egui::StrokeKind::Inside,
                    );
                }

                // Drag Start Detection for Populated Pads
                let press_origin = ui.input(|i| i.pointer.press_origin());
                let pressed_in_cell = press_origin
                    .is_some_and(|p| launcher_grid_rect.contains(p) && cell_rect.contains(p));

                if has_clip
                    && (cell_hovered || pressed_in_cell)
                    && workspace_resp.drag_started_by(egui::PointerButton::Primary)
                {
                    if let Some(pos) = pointer_pos.or(press_origin) {
                        start_matrix_drag_action = Some((
                            track_idx,
                            scene_idx,
                            clip_opt.unwrap().clone(),
                            pos - cell_rect.min,
                        ));
                    }
                }

                if cell_hovered {
                    if state.matrix_state.clip_drag.is_some() {
                        if pointer_released {
                            drop_matrix_clip_action = Some((track_idx, scene_idx));
                        }
                    } else {
                        if double_clicked {
                            if has_clip {
                                select_slot_action = Some((track_idx, scene_idx));
                                open_piano_roll_action = true;
                            } else {
                                create_midi_slot_action = Some((track_idx, scene_idx));
                                open_piano_roll_action = true;
                            }
                        } else if primary_clicked {
                            if has_clip {
                                trigger_slot_action = Some((track_idx, scene_idx));
                                select_slot_action = Some((track_idx, scene_idx));
                            } else {
                                select_slot_action = Some((track_idx, scene_idx));
                            }
                        }
                        if secondary_clicked {
                            select_slot_action = Some((track_idx, scene_idx));
                            open_context_menu_action = Some((
                                track_idx,
                                scene_idx,
                                pointer_pos.unwrap_or(cell_rect.min),
                            ));
                        }
                        if pointer_released {
                            if let Some(ref sample_path) = state.dragged_sample {
                                drop_slot_action = Some((track_idx, scene_idx, sample_path.clone()));
                            }
                        }
                    }
                }
            }
        }

        // Register drag start if initiated this frame
        if let Some((from_t, from_s, clip, grab_offset)) = start_matrix_drag_action {
            let is_copy = ui.input(|i| i.modifiers.ctrl || i.modifiers.alt);
            state.matrix_state.clip_drag = Some(MatrixClipDrag {
                from_track: from_t,
                from_scene: from_s,
                clip,
                grab_offset,
                is_copy,
                moved: true,
            });
            state.matrix_state.selected_slot = Some((from_t, from_s));
        }

        // Update drag state while dragging
        if let Some(ref mut drag) = state.matrix_state.clip_drag {
            if workspace_resp.dragged_by(egui::PointerButton::Primary) {
                drag.moved = true;
                drag.is_copy = ui.input(|i| i.modifiers.ctrl || i.modifiers.alt);
            }
        }

        // Handle drop on mouse release
        if pointer_released {
            if let Some(drag) = state.matrix_state.clip_drag.take() {
                if drag.moved {
                    let target_slot = drop_matrix_clip_action.or_else(|| {
                        if let Some(pos) = pointer_pos {
                            if launcher_grid_rect.contains(pos) {
                                let t = ((pos.y - tracks_y0 - scroll_y) / row_h).floor() as isize;
                                let s = ((pos.x - launcher_x0) / BITWIG_SCENE_W).floor() as isize;
                                if t >= 0 && (t as usize) < tracks_count && s >= 0 && (s as usize) < scenes_count {
                                    Some((t as usize, s as usize))
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    });

                    if let Some((dst_track, dst_scene)) = target_slot {
                        if (dst_track, dst_scene) != (drag.from_track, drag.from_scene) {
                            if drag.is_copy {
                                state.matrix_state.copy_to_slot(
                                    drag.from_track,
                                    drag.from_scene,
                                    dst_track,
                                    dst_scene,
                                    &state.audio_proxy,
                                );
                            } else {
                                state.matrix_state.move_slot(
                                    drag.from_track,
                                    drag.from_scene,
                                    dst_track,
                                    dst_scene,
                                    &state.audio_proxy,
                                );
                            }
                            select_slot_action = Some((dst_track, dst_scene));
                            select_matrix_track(state, dst_track);
                        }
                    }
                }
            }
        }

        // Render Floating Ghost Clip Card attached to cursor
        if let Some(ref drag) = state.matrix_state.clip_drag {
            if let Some(pos) = pointer_pos {
                let ghost_w = BITWIG_SCENE_W - 3.0;
                let ghost_h = row_h - 3.5;
                let ghost_rect = Rect::from_min_size(pos - drag.grab_offset, vec2(ghost_w, ghost_h));
                let ghost_painter = ui.ctx().layer_painter(egui::LayerId::new(
                    egui::Order::Tooltip,
                    egui::Id::new("matrix_clip_drag_ghost"),
                ));

                // Soft ambient drop shadow
                ghost_painter.rect_filled(
                    ghost_rect.translate(vec2(2.0, 3.5)),
                    3.0,
                    Color32::from_black_alpha(110),
                );

                let eff_col = drag.clip.color.unwrap_or_else(|| track_color_for(drag.from_track));
                let ghost_bg = if drag.clip.muted {
                    Color32::from_rgba_unmultiplied(40, 42, 46, 230)
                } else {
                    pad_body_color(eff_col, 230)
                };
                ghost_painter.rect_filled(ghost_rect, 2.5, ghost_bg);

                // Ghost Top colored strip
                let ghost_top = Rect::from_min_size(ghost_rect.min, vec2(ghost_rect.width(), 16.0));
                let ghost_strip_col = if drag.clip.muted {
                    Color32::from_rgb(0x40, 0x43, 0x48)
                } else {
                    eff_col
                };
                ghost_painter.rect_filled(ghost_top, 2.5, ghost_strip_col);

                let ic_rect = Rect::from_min_size(
                    pos2(ghost_top.min.x + 3.0, ghost_top.min.y + 2.0),
                    vec2(12.0, 12.0),
                );
                icons::paint_play_triangle(
                    &ghost_painter,
                    ic_rect,
                    Color32::from_rgb(0x10, 0x10, 0x12),
                );
                ghost_painter.text(
                    pos2(ghost_top.min.x + 17.0, ghost_top.center().y),
                    egui::Align2::LEFT_CENTER,
                    &drag.clip.name,
                    fonts::semibold(9.5),
                    Color32::from_rgb(0x10, 0x10, 0x12),
                );

                // Waveform or MIDI miniature in ghost
                match &drag.clip.content {
                    ClipData::Audio { .. } => {
                        let wave_rect = Rect::from_min_max(
                            pos2(ghost_rect.min.x + 2.0, ghost_rect.min.y + 17.5),
                            pos2(ghost_rect.max.x - 2.0, ghost_rect.max.y - 2.5),
                        );
                        paint_pad_waveform(
                            &ghost_painter,
                            wave_rect,
                            &drag.clip.peaks,
                            eff_col,
                            drag.clip.muted,
                            0.92,
                        );
                    }
                    ClipData::Midi { notes } => {
                        if !notes.is_empty() {
                            let body_rect = Rect::from_min_max(
                                pos2(ghost_rect.min.x + 3.0, ghost_rect.min.y + 18.0),
                                pos2(ghost_rect.max.x - 3.0, ghost_rect.max.y - 3.0),
                            );
                            let max_t = drag.clip.loop_end.max(3840) as f32;
                            let min_p = notes.iter().map(|(_, p, _, _)| *p).min().unwrap_or(36).saturating_sub(2);
                            let max_p = notes.iter().map(|(_, p, _, _)| *p).max().unwrap_or(84).saturating_add(2);
                            let p_range = (max_p.saturating_sub(min_p)).max(12) as f32;

                            for &(st, pitch, _, dur) in notes.iter().take(80) {
                                let nx = body_rect.min.x + (st as f32 / max_t) * body_rect.width();
                                let nw = ((dur as f32 / max_t) * body_rect.width()).clamp(2.0, body_rect.width());
                                let norm_pitch = (pitch.saturating_sub(min_p) as f32 / p_range).clamp(0.0, 1.0);
                                let ny = body_rect.max.y - norm_pitch * (body_rect.height() - 3.0) - 2.0;

                                if nx < body_rect.max.x {
                                    let note_rect = Rect::from_min_size(
                                        pos2(nx, ny),
                                        vec2(nw.min(body_rect.max.x - nx), 2.0),
                                    );
                                    ghost_painter.rect_filled(
                                        note_rect,
                                        0.5,
                                        Color32::from_rgba_unmultiplied(255, 255, 255, 230),
                                    );
                                }
                            }
                        }
                    }
                }

                // Ghost outline
                ghost_painter.rect_stroke(
                    ghost_rect,
                    2.5,
                    Stroke::new(1.5_f32, if drag.is_copy { Color32::WHITE } else { ACCENT_ORANGE }),
                    egui::StrokeKind::Inside,
                );

                // Copy badge
                if drag.is_copy {
                    let badge_rect = Rect::from_center_size(
                        ghost_rect.right_top() + vec2(-5.0, 5.0),
                        vec2(14.0, 14.0),
                    );
                    ghost_painter.circle_filled(badge_rect.center(), 7.0, ACCENT_ORANGE);
                    icons::paint_icon(
                        &ghost_painter,
                        badge_rect.shrink(3.0),
                        icons::PLUS,
                        Color32::WHITE,
                        1.8,
                    );
                }
            }
        }

        // Crisp dark divider between Clip Launcher and Arranger Timeline
        if show_timeline {
            painter.line_segment(
                [pos2(launcher_x1, toolbar_y0), pos2(launcher_x1, workspace_rect.max.y)],
                Stroke::new(2.0_f32, Color32::from_rgb(0x0E, 0x0F, 0x11)),
            );
        }
    }

    // =========================================================================
    // 4. ARRANGER TIMELINE (Bitwig Signature Medium-Grey Lanes & Crisp Grid)
    // =========================================================================
    if show_timeline && timeline_w > 0.0 {
        let ppqn = state.playlist_state.ppqn.max(1);
        let zoom_x = state.playlist_state.zoom_x;
        let scroll_x = state.playlist_state.scroll_offset_x;
        let beats_per_bar = state.transport.beats_per_bar.max(1);
        let ticks_per_bar_val = ticks_per_bar(ppqn, beats_per_bar);
        let (adaptive_step, grid_label, adaptive_den) =
            playlist::adaptive_grid_step(ppqn, beats_per_bar, zoom_x);
        state.playlist_state.grid_denominator = adaptive_den;
        let shift_held = ui.input(|i| i.modifiers.shift);
        let alt_held = ui.input(|i| i.modifiers.alt);
        let ctrl_held = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
        let snap_step = if shift_held { 1 } else { adaptive_step };

        // 4A. Timeline Beat Ruler (Top, `#212226` dark Bitwig ruler)
        let ruler_rect =
            Rect::from_min_max(pos2(timeline_x0, toolbar_y0), pos2(timeline_x1, toolbar_y1));
        let loop_lane_rect =
            Rect::from_min_max(ruler_rect.min, pos2(ruler_rect.max.x, ruler_rect.min.y + 10.5));
        let ruler_seek_rect =
            Rect::from_min_max(pos2(ruler_rect.min.x, ruler_rect.min.y + 10.5), ruler_rect.max);

        let ruler_painter = painter.with_clip_rect(ruler_rect);
        ruler_painter.rect_filled(ruler_rect, 0.0, Color32::from_rgb(0x21, 0x22, 0x26));
        ruler_painter.line_segment(
            [
                pos2(ruler_rect.min.x, ruler_rect.min.y + 10.5),
                pos2(ruler_rect.max.x, ruler_rect.min.y + 10.5),
            ],
            Stroke::new(1.0_f32, Color32::from_rgb(0x18, 0x19, 0x1C)),
        );
        ruler_painter.line_segment(
            [ruler_rect.left_bottom(), ruler_rect.right_bottom()],
            Stroke::new(1.0_f32, BW_CAPSULE_BORDER),
        );

        let start_tick = px_to_ticks((-scroll_x).max(0.0), zoom_x);
        let end_tick = px_to_ticks((-scroll_x).max(0.0) + ruler_rect.width(), zoom_x);
        let first_bar = start_tick / ticks_per_bar_val;
        let last_bar = (end_tick / ticks_per_bar_val) + 2;
        let bar_px_w = ticks_to_px(ticks_per_bar_val, zoom_x);
        let beat_ticks = ppqn;
        let beat_px_w = ticks_to_px(beat_ticks, zoom_x);

        // Loop Region Interaction (Bitwig-style top loop bar: drag left/right/body or drag empty lane to create)
        let has_loop = state.playlist_state.loop_end_ticks > state.playlist_state.loop_start_ticks;
        let l0_px = timeline_x0 + scroll_x + ticks_to_px(state.playlist_state.loop_start_ticks, zoom_x);
        let l1_px = timeline_x0 + scroll_x + ticks_to_px(state.playlist_state.loop_end_ticks, zoom_x);

        let hit_loop_handle = |pos: egui::Pos2| -> playlist::LoopDragHandle {
            if !loop_lane_rect.contains(pos) {
                return playlist::LoopDragHandle::None;
            }
            if has_loop {
                if (pos.x - l0_px).abs() <= 6.0 && (l1_px - l0_px >= 10.0 || pos.x <= l0_px + 3.0) {
                    return playlist::LoopDragHandle::Left;
                }
                if (pos.x - l1_px).abs() <= 6.0 {
                    return playlist::LoopDragHandle::Right;
                }
                if pos.x > l0_px && pos.x < l1_px {
                    return playlist::LoopDragHandle::Body;
                }
            }
            playlist::LoopDragHandle::Create
        };

        let mut loop_changed = false;
        if state.playlist_state.loop_drag_handle == playlist::LoopDragHandle::None {
            if let Some(pos) = pointer_pos {
                if loop_lane_rect.contains(pos) && state.playlist_pan.is_none() {
                    let hover_handle = hit_loop_handle(pos);
                    match hover_handle {
                        playlist::LoopDragHandle::Left => {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                            crate::app::set_footer_cues(state, None, &[("DRAG", "Adjust start"), ("SHIFT", "Disable snapping")]);
                        }
                        playlist::LoopDragHandle::Right => {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                            crate::app::set_footer_cues(state, None, &[("DRAG", "Adjust end"), ("SHIFT", "Disable snapping")]);
                        }
                        playlist::LoopDragHandle::Body => {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                            crate::app::set_footer_cues(state, None, &[("DRAG", "Move loop"), ("CLICK", "Toggle loop")]);
                        }
                        playlist::LoopDragHandle::Create => {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                            crate::app::set_footer_cues(state, None, &[("DRAG", "Set loop range"), ("SHIFT", "Disable snapping")]);
                        }
                        playlist::LoopDragHandle::None => {}
                    }
                    if workspace_resp.drag_started_by(egui::PointerButton::Primary)
                        || ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary))
                    {
                        let rel_px = (pos.x - timeline_x0 - scroll_x).max(0.0);
                        let cur_tick = snap_ticks(px_to_ticks(rel_px, zoom_x), snap_step);
                        state.playlist_state.loop_drag_handle = hover_handle;
                        state.playlist_state.loop_dragging = true;
                        state.playlist_state.loop_drag_moved = false;
                        state.playlist_state.loop_drag_origin_tick = cur_tick;
                        state.playlist_state.loop_drag_offset_ticks =
                            cur_tick.saturating_sub(state.playlist_state.loop_start_ticks);
                    }
                }
            }
        } else if ui.input(|i| i.pointer.button_down(egui::PointerButton::Primary)) {
            if let Some(pos) = pointer_pos {
                let rel_px = (pos.x - timeline_x0 - scroll_x).max(0.0);
                let cur_tick = snap_ticks(px_to_ticks(rel_px, zoom_x), snap_step);
                let min_loop = adaptive_step.max(ppqn / 4);
                match state.playlist_state.loop_drag_handle {
                    playlist::LoopDragHandle::Left => {
                        let max_start = state.playlist_state.loop_end_ticks.saturating_sub(min_loop);
                        let new_start = cur_tick.min(max_start);
                        if new_start != state.playlist_state.loop_start_ticks {
                            state.playlist_state.loop_start_ticks = new_start;
                            state.playlist_state.loop_drag_moved = true;
                            loop_changed = true;
                        }
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                        crate::app::set_footer_cues(state, None, &[("DRAG", "Adjust start"), ("SHIFT", "Disable snapping")]);
                    }
                    playlist::LoopDragHandle::Right => {
                        let min_end = state.playlist_state.loop_start_ticks.saturating_add(min_loop);
                        let new_end = cur_tick.max(min_end);
                        if new_end != state.playlist_state.loop_end_ticks {
                            state.playlist_state.loop_end_ticks = new_end;
                            state.playlist_state.loop_drag_moved = true;
                            loop_changed = true;
                        }
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                        crate::app::set_footer_cues(state, None, &[("DRAG", "Adjust end"), ("SHIFT", "Disable snapping")]);
                    }
                    playlist::LoopDragHandle::Body => {
                        let len = state.playlist_state.loop_length_ticks().max(min_loop);
                        let new_start = cur_tick.saturating_sub(state.playlist_state.loop_drag_offset_ticks);
                        if new_start != state.playlist_state.loop_start_ticks {
                            state.playlist_state.loop_start_ticks = new_start;
                            state.playlist_state.loop_end_ticks = new_start.saturating_add(len);
                            state.playlist_state.loop_drag_moved = true;
                            loop_changed = true;
                        }
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                        crate::app::set_footer_cues(state, None, &[("DRAG", "Move loop"), ("SHIFT", "Disable snapping")]);
                    }
                    playlist::LoopDragHandle::Create => {
                        let orig = state.playlist_state.loop_drag_origin_tick;
                        if cur_tick != orig {
                            let (a, b) = if cur_tick >= orig {
                                (orig, cur_tick.max(orig + min_loop))
                            } else {
                                (cur_tick, orig.max(cur_tick + min_loop))
                            };
                            state.playlist_state.loop_start_ticks = a;
                            state.playlist_state.loop_end_ticks = b;
                            state.playlist_state.loop_region_active = true;
                            state.is_looping = true;
                            state.playlist_state.loop_drag_moved = true;
                            loop_changed = true;
                        }
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                    playlist::LoopDragHandle::None => {}
                }
            }
        } else {
            // Released primary button while in loop drag
            if !state.playlist_state.loop_drag_moved
                && state.playlist_state.loop_drag_handle == playlist::LoopDragHandle::Body
            {
                state.is_looping = !state.is_looping;
                state.playlist_state.loop_region_active = state.is_looping;
                state.playlist_state.ensure_minimum_global_loop(state.is_looping);
                loop_changed = true;
            }
            state.playlist_state.loop_drag_handle = playlist::LoopDragHandle::None;
            state.playlist_state.loop_dragging = false;
            state.playlist_state.loop_drag_moved = false;
        }

        if loop_changed {
            let start_samples = state.transport.ticks_to_samples(state.playlist_state.loop_start_ticks);
            let end_samples = state.transport.ticks_to_samples(state.playlist_state.loop_end_ticks);
            state.transport.loop_start_samples = start_samples;
            state.transport.loop_end_samples = end_samples;
            state.audio_proxy.send(GuiCommand::SetGlobalLoop {
                start_samples,
                end_samples,
                enabled: state.playlist_state.loop_region_active,
            });
        }

        // Loop Region Bracket in Ruler (Bitwig grey/white loop bar at top of ruler)
        if state.playlist_state.loop_end_ticks > state.playlist_state.loop_start_ticks {
            let l0 = timeline_x0
                + scroll_x
                + ticks_to_px(state.playlist_state.loop_start_ticks, zoom_x);
            let l1 =
                timeline_x0 + scroll_x + ticks_to_px(state.playlist_state.loop_end_ticks, zoom_x);
            let l_rect = Rect::from_min_max(
                pos2(l0.max(timeline_x0), ruler_rect.min.y + 2.0),
                pos2(l1.min(timeline_x1), ruler_rect.min.y + 8.5),
            );
            if l_rect.width() > 0.0 {
                let loop_fill = if state.playlist_state.loop_region_active {
                    Color32::from_rgb(0xE8, 0xEA, 0xEE)
                } else {
                    Color32::from_rgb(0x6E, 0x70, 0x76)
                };
                ruler_painter.rect_filled(l_rect, 1.5, loop_fill);
            }
        }

        // Adaptive Ruler Bar, Beat & 16th Labels (Bitwig 1:1)
        let bar_step = playlist::ruler_bar_step(bar_px_w);
        let first_labeled_bar = (first_bar / bar_step) * bar_step;
        for b in (first_labeled_bar..=last_bar).step_by(bar_step as usize) {
            let bar_tick = b * ticks_per_bar_val;
            let px = timeline_x0 + scroll_x + ticks_to_px(bar_tick, zoom_x);
            if px >= timeline_x0 && px <= timeline_x1 {
                ruler_painter.line_segment(
                    [pos2(px, ruler_rect.min.y + 11.0), pos2(px, ruler_rect.max.y)],
                    Stroke::new(1.0_f32, Color32::from_rgb(0x5A, 0x5C, 0x64)),
                );
                ruler_painter.text(
                    pos2(px + 4.0, ruler_rect.min.y + 11.5),
                    egui::Align2::LEFT_TOP,
                    format!("{}", b + 1),
                    fonts::semibold(10.0),
                    Color32::from_rgb(0xC6, 0xC8, 0xD0),
                );
            }
            // Sub-bar beat labels when zoomed in (e.g. 6.2, 6.3, 6.4)
            if bar_step == 1 && bar_px_w >= 140.0 {
                for sub in 1..beats_per_bar {
                    let sub_tick = bar_tick + sub as u64 * beat_ticks;
                    let spx = timeline_x0 + scroll_x + ticks_to_px(sub_tick, zoom_x);
                    if spx >= timeline_x0 && spx <= timeline_x1 {
                        ruler_painter.line_segment(
                            [pos2(spx, ruler_rect.min.y + 15.0), pos2(spx, ruler_rect.max.y)],
                            Stroke::new(1.0_f32, Color32::from_rgb(0x42, 0x44, 0x4A)),
                        );
                        ruler_painter.text(
                            pos2(spx + 3.0, ruler_rect.min.y + 12.0),
                            egui::Align2::LEFT_TOP,
                            format!("{}.{}", b + 1, sub + 1),
                            fonts::regular(9.0),
                            Color32::from_rgb(0x90, 0x92, 0x9A),
                        );
                    }
                }
            }
            // Sub-beat 16th labels when zoomed way in (e.g. 6.2.2, 6.2.3, 6.2.4)
            if bar_step == 1 && beat_px_w >= 140.0 {
                let sixteenth = (beat_ticks / 4).max(1);
                for beat in 0..beats_per_bar {
                    for s16 in 1..4u64 {
                        let t16 = bar_tick + beat as u64 * beat_ticks + s16 * sixteenth;
                        let x16 = timeline_x0 + scroll_x + ticks_to_px(t16, zoom_x);
                        if x16 >= timeline_x0 && x16 <= timeline_x1 {
                            ruler_painter.line_segment(
                                [pos2(x16, ruler_rect.min.y + 18.0), pos2(x16, ruler_rect.max.y)],
                                Stroke::new(1.0_f32, Color32::from_rgb(0x36, 0x38, 0x3E)),
                            );
                            ruler_painter.text(
                                pos2(x16 + 2.0, ruler_rect.min.y + 12.5),
                                egui::Align2::LEFT_TOP,
                                format!("{}.{}.{}", b + 1, beat + 1, s16 + 1),
                                fonts::regular(8.5),
                                Color32::from_rgb(0x76, 0x78, 0x80),
                            );
                        }
                    }
                }
            }
        }

        // Cue Markers on Arranger Ruler (Bitwig style flag + label)
        let mut cue_seek_tick: Option<u64> = None;
        let mut cue_remove_id: Option<usize> = None;
        let mut cue_hover_name: Option<String> = None;
        let mut cue_hit = false;
        for marker in &state.playlist_state.cue_markers {
            let mx = timeline_x0 + scroll_x + ticks_to_px(marker.tick, zoom_x);
            if mx < timeline_x0 - 40.0 || mx > timeline_x1 + 8.0 {
                continue;
            }
            let col = Color32::from_rgb(marker.color[0], marker.color[1], marker.color[2]);
            let flag_w = (marker.name.len() as f32 * 5.6 + 10.0).clamp(24.0, 92.0);
            let flag_rect = Rect::from_min_max(
                pos2(mx, ruler_rect.min.y + 1.0),
                pos2(mx + flag_w, ruler_rect.min.y + 11.0),
            );
            // Vertical guide tick on the ruler
            ruler_painter.line_segment(
                [pos2(mx, ruler_rect.min.y + 1.0), pos2(mx, ruler_rect.max.y)],
                Stroke::new(1.25_f32, col),
            );
            // Flag pill
            ruler_painter.rect_filled(flag_rect, 1.5, col);
            ruler_painter.text(
                pos2(flag_rect.min.x + 3.5, flag_rect.center().y),
                egui::Align2::LEFT_CENTER,
                &marker.name,
                fonts::semibold(8.5),
                Color32::from_rgb(0x16, 0x16, 0x18),
            );
            if let Some(pos) = pointer_pos {
                if flag_rect.contains(pos) {
                    cue_hit = true;
                    cue_hover_name = Some(marker.name.clone());
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    if primary_clicked {
                        cue_seek_tick = Some(marker.tick);
                    } else if workspace_resp.secondary_clicked() {
                        cue_remove_id = Some(marker.id);
                    }
                }
            }
        }
        if let Some(ref name) = cue_hover_name {
            crate::app::set_footer_cues(
                state,
                Some(("CUE MARKER", name)),
                &[("CLICK", "Jump to Cue Marker"), ("RIGHT-CLICK", "Delete Cue Marker")],
            );
        }
        if let Some(tick) = cue_seek_tick {
            state.playlist_state.playhead_tick = tick;
            let sample_count = state.transport.ticks_to_samples(tick);
            state.transport.sample_count = sample_count;
            state.audio_proxy.send(GuiCommand::Seek { sample_count });
        }
        if let Some(id) = cue_remove_id {
            state.playlist_state.remove_cue_marker(id);
            state.project_dirty = true;
        }

        // Seek playhead when clicking/dragging on lower ruler (when not dragging loop)
        if !cue_hit && state.playlist_state.loop_drag_handle == playlist::LoopDragHandle::None {
            if let Some(pos) = pointer_pos {
                if ruler_seek_rect.contains(pos) && state.playlist_pan.is_none() {
                    crate::app::set_footer_cues(
                        state,
                        None,
                        &[("CLICK", "Set playback start time"), ("MIDDLE-DRAG", "Zoom & pan")],
                    );
                    if primary_clicked || workspace_resp.dragged_by(egui::PointerButton::Primary) {
                        let rel_px = (pos.x - timeline_x0 - scroll_x).max(0.0);
                        let tick = snap_ticks(px_to_ticks(rel_px, zoom_x), snap_step);
                        state.playlist_state.playhead_tick = tick;
                        let sample_count = state.transport.ticks_to_samples(tick);
                        state.transport.sample_count = sample_count;
                        state.audio_proxy.send(GuiCommand::Seek { sample_count });
                    }
                }
            }
        }

        // 4B. Bitwig Neutral Medium-Grey Timeline Grid & Track Lanes
        let tl_grid_rect =
            Rect::from_min_max(pos2(timeline_x0, tracks_y0), pos2(timeline_x1, tracks_y1));
        let tl_painter = painter.with_clip_rect(tl_grid_rect);
        // Empty space below tracks in Bitwig Arranger is `#5C5E62` slate grey!
        tl_painter.rect_filled(tl_grid_rect, 0.0, Color32::from_rgb(0x5C, 0x5E, 0x62));

        // Active Track Lanes in Bitwig's signature light-medium neutral grey (`#74767B` / `#6E7075`)
        for (vis_idx, &track_idx) in visible_tracks.iter().enumerate() {
            let row_y0 = tracks_y0 + scroll_y + vis_idx as f32 * row_h;
            let row_y1 = row_y0 + row_h;
            if row_y1 < tracks_y0 || row_y0 > tracks_y1 {
                continue;
            }
            let mixer_idx = mixer_index_of_matrix_track(track_idx);
            let is_sel = state.selected_track_index == mixer_idx;
            let lane_fill = if is_sel {
                Color32::from_rgb(0x7E, 0x80, 0x85)
            } else if track_idx % 2 == 0 {
                Color32::from_rgb(0x74, 0x76, 0x7B)
            } else {
                Color32::from_rgb(0x6E, 0x70, 0x75)
            };
            tl_painter.rect_filled(
                Rect::from_min_max(pos2(timeline_x0, row_y0), pos2(timeline_x1, row_y1)),
                0.0,
                lane_fill,
            );
            tl_painter.line_segment(
                [pos2(timeline_x0, row_y1), pos2(timeline_x1, row_y1)],
                Stroke::new(1.0_f32, Color32::from_rgb(0x46, 0x48, 0x4C)),
            );
        }

        // Continuous Dark Vertical Adaptive Grid Lines from `tracks_y0` to `tracks_y1`
        for b in first_bar..=last_bar {
            let bar_tick = b * ticks_per_bar_val;
            let px = timeline_x0 + scroll_x + ticks_to_px(bar_tick, zoom_x);
            if px >= timeline_x0 && px <= timeline_x1 && (bar_step == 1 || b % bar_step == 0 || bar_px_w >= 10.0) {
                tl_painter.line_segment(
                    [pos2(px, tracks_y0), pos2(px, tracks_y1)],
                    Stroke::new(
                        1.0_f32,
                        if b % 4 == 0 {
                            Color32::from_rgb(0x36, 0x38, 0x3C)
                        } else {
                            Color32::from_rgb(0x43, 0x45, 0x49)
                        },
                    ),
                );
            }
            // Sub-bar beat lines
            if beat_px_w >= 12.0 {
                for sub in 1..beats_per_bar {
                    let sub_tick = bar_tick + sub as u64 * beat_ticks;
                    let spx = timeline_x0 + scroll_x + ticks_to_px(sub_tick, zoom_x);
                    if spx >= timeline_x0 && spx <= timeline_x1 {
                        tl_painter.line_segment(
                            [pos2(spx, tracks_y0), pos2(spx, tracks_y1)],
                            Stroke::new(1.0_f32, Color32::from_rgb(0x52, 0x54, 0x59)),
                        );
                    }
                }
            }
            // Adaptive sub-beat lines (1/8, 1/16, 1/32, 1/64)
            if adaptive_step < beat_ticks {
                let sub_count = (ticks_per_bar_val / adaptive_step) as usize;
                let subs_per_beat = (beat_ticks / adaptive_step).max(1) as usize;
                for s in 1..sub_count {
                    if s % subs_per_beat == 0 {
                        continue;
                    }
                    let ts = bar_tick + s as u64 * adaptive_step;
                    let xs = timeline_x0 + scroll_x + ticks_to_px(ts, zoom_x);
                    if xs >= timeline_x0 && xs <= timeline_x1 {
                        tl_painter.line_segment(
                            [pos2(xs, tracks_y0), pos2(xs, tracks_y1)],
                            Stroke::new(1.0_f32, Color32::from_rgb(0x60, 0x62, 0x67)),
                        );
                    }
                }
            }
        }

        // Draw Timeline Clips (Bitwig high-contrast colored clip cards)
        let mut clicked_clip_id: Option<usize> = None;
        let mut dbl_clicked_clip_id: Option<usize> = None;
        let mut right_clicked_clip_id: Option<usize> = None;
        let mut start_drag_clip: Option<(usize, u64, usize, f32, f32)> = None;
        let mut start_trim_clip: Option<playlist::TrimDrag> = None;
        let mut hovered_clip_id: Option<usize> = None;
        let mut hovered_trim_edge: Option<(usize, playlist::TrimEdge)> = None;
        let mut visible_clip_rects: Vec<(usize, Rect)> = Vec::new();

        for (clip_track_id, clip) in &state.playlist_state.clips {
            let row_idx = if *clip_track_id > 0 && *clip_track_id <= tracks_count {
                clip_track_id - 1
            } else {
                (*clip_track_id).min(tracks_count.saturating_sub(1))
            };

            let clip_x = timeline_x0 + scroll_x + ticks_to_px(clip.start_tick, zoom_x);
            let clip_w = ticks_to_px(clip.duration_ticks, zoom_x).max(6.0);
            let clip_y = tracks_y0 + scroll_y + row_idx as f32 * row_h + 1.5;
            let clip_h = row_h - 3.5;

            let clip_rect = Rect::from_min_size(pos2(clip_x, clip_y), vec2(clip_w, clip_h));
            visible_clip_rects.push((clip.id, clip_rect));
            if clip_rect.max.x < timeline_x0
                || clip_rect.min.x > timeline_x1
                || clip_rect.max.y < tracks_y0
                || clip_rect.min.y > tracks_y1
            {
                continue;
            }

            let is_sel = state.playlist_state.selected_clips.contains(&clip.id);
            let mixer_idx = mixer_index_of_matrix_track(row_idx);
            let base_col = state
                .live_tracks
                .get(mixer_idx)
                .map(|t| t.color)
                .unwrap_or_else(|| track_color_for(row_idx));
            let body_col = Color32::from_rgba_unmultiplied(
                ((base_col.r() as u16 * 3 + 70) / 4) as u8,
                ((base_col.g() as u16 * 3 + 70) / 4) as u8,
                ((base_col.b() as u16 * 3 + 75) / 4) as u8,
                238,
            );
            tl_painter.rect_filled(clip_rect, 2.5, body_col);

            let hdr_rect = Rect::from_min_size(clip_rect.min, vec2(clip_w, 15.0));
            tl_painter.rect_filled(hdr_rect, 2.5, base_col);
            tl_painter.text(
                pos2(clip_rect.min.x + 5.0, clip_rect.min.y + 1.5),
                egui::Align2::LEFT_TOP,
                &clip.name,
                fonts::semibold(9.5),
                Color32::from_rgb(0x10, 0x10, 0x12),
            );

            if let ClipType::Audio { ref peaks, .. } = clip.clip_type {
                if !peaks.is_empty() {
                    let wy = clip_rect.center().y + 6.5;
                    let wh = (clip_h - 17.0).max(4.0) * 0.46;
                    let step = clip_w / peaks.len() as f32;
                    for (i, &pk) in peaks.iter().enumerate() {
                        let px = clip_x + i as f32 * step;
                        if px >= timeline_x0 && px <= timeline_x1 {
                            let h = (pk * wh).max(1.0);
                            tl_painter.line_segment(
                                [pos2(px, wy - h), pos2(px, wy + h)],
                                Stroke::new(1.1_f32, Color32::from_rgb(0x16, 0x17, 0x1A)),
                            );
                        }
                    }
                }
            }

            tl_painter.rect_stroke(
                clip_rect,
                2.5,
                Stroke::new(
                    if is_sel { 2.0_f32 } else { 1.0_f32 },
                    if is_sel {
                        Color32::WHITE
                    } else {
                        Color32::from_rgb(0x1A, 0x1B, 0x1E)
                    },
                ),
                egui::StrokeKind::Inside,
            );

            if let Some(pos) = pointer_pos {
                if tl_grid_rect.contains(pos) && clip_rect.contains(pos) && state.playlist_pan.is_none() {
                    hovered_clip_id = Some(clip.id);
                    let edge = if clip_w >= 14.0 {
                        playlist::trim_edge_at(pos.x, clip_x, clip_w, 6.0)
                    } else {
                        None
                    };
                    if let Some(e) = edge {
                        hovered_trim_edge = Some((clip.id, e));
                    }
                    if double_clicked {
                        dbl_clicked_clip_id = Some(clip.id);
                    } else if primary_clicked {
                        clicked_clip_id = Some(clip.id);
                    }
                    if secondary_clicked {
                        right_clicked_clip_id = Some(clip.id);
                    }
                    if workspace_resp.drag_started_by(egui::PointerButton::Primary)
                        && matches!(
                            state.playlist_state.active_tool,
                            playlist::PlaylistTool::Pointer | playlist::PlaylistTool::Pen
                        )
                    {
                        if let Some(e) = edge {
                            let (orig_offset, orig_total) = match clip.clip_type {
                                ClipType::Audio {
                                    sample_offset_ticks,
                                    total_sample_ticks,
                                    ..
                                } => (sample_offset_ticks, total_sample_ticks.max(clip.duration_ticks)),
                                _ => (0, u64::MAX / 2),
                            };
                            let edge_tick = match e {
                                playlist::TrimEdge::Left => clip.start_tick,
                                playlist::TrimEdge::Right => clip.start_tick.saturating_add(clip.duration_ticks),
                            };
                            let grab_dx = match e {
                                playlist::TrimEdge::Left => pos.x - clip_x,
                                playlist::TrimEdge::Right => pos.x - (clip_x + clip_w),
                            };
                            start_trim_clip = Some(playlist::TrimDrag {
                                clip_id: clip.id,
                                edge: e,
                                orig_edge_tick: edge_tick,
                                orig_start: clip.start_tick,
                                orig_duration: clip.duration_ticks,
                                orig_offset,
                                orig_total,
                                grab_dx_px: grab_dx,
                                moved: false,
                            });
                        } else {
                            start_drag_clip = Some((
                                clip.id,
                                clip.start_tick,
                                *clip_track_id,
                                pos.x - clip_x,
                                pos.y - clip_y,
                            ));
                        }
                    }
                }
            }
        }

        // Hover cursor & footer cues for Trim / Clip / Empty Grid
        if state.playlist_state.trim_drag.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            crate::app::set_footer_cues(state, None, &[("DRAG", "Trim clip"), ("SHIFT", "Disable snapping")]);
        } else if state.playlist_state.clip_drag.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            crate::app::set_footer_cues(state, None, &[("DRAG", "Move clip"), ("ALT+DRAG", "Duplicate")]);
        } else if state.playlist_state.marquee.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            crate::app::set_footer_cues(state, None, &[("DRAG", "Rectangular selection")]);
        } else if let Some((_, edge)) = hovered_trim_edge {
            ui.ctx().set_cursor_icon(edge.cursor());
            crate::app::set_footer_cues(state, None, &[("DRAG", "Trim clip"), ("SHIFT", "Disable snapping")]);
        } else if hovered_clip_id.is_some() {
            crate::app::set_footer_cues(
                state,
                None,
                &[("DRAG", "Move clip"), ("DOUBLE-CLICK", "Open editor")],
            );
        } else if let Some(pos) = pointer_pos {
            if workspace_resp.hovered()
                && tl_grid_rect.contains(pos)
                && state.playlist_pan.is_none()
                && !is_dragging_sample
            {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                crate::app::set_footer_cues(
                    state,
                    None,
                    &[("DRAG", "Rectangular selection"), ("DOUBLE-CLICK", "Insert clip")],
                );
            }
        }

        // Handle Trim Start & Update
        if let Some(td) = start_trim_clip {
            state.playlist_state.selected_clips = vec![td.clip_id];
            state.playlist_state.trim_drag = Some(td);
        }
        if let Some(mut td) = state.playlist_state.trim_drag {
            if workspace_resp.dragged_by(egui::PointerButton::Primary) {
                if let Some(pos) = pointer_pos {
                    let edge_x = (pos.x - td.grab_dx_px - timeline_x0 - scroll_x).max(0.0);
                    let raw_tick = px_to_ticks(edge_x, zoom_x);
                    let snapped_tick = snap_ticks(raw_tick, snap_step);
                    let min_ticks = playlist::min_clip_ticks(ppqn);
                    let (new_start, new_dur, new_offset) = playlist::trim_apply(
                        td.orig_start,
                        td.orig_duration,
                        td.orig_offset,
                        td.orig_total,
                        td.edge,
                        snapped_tick,
                        snap_step,
                        min_ticks,
                    );
                    if let Some((_, clip)) = state
                        .playlist_state
                        .clips
                        .iter_mut()
                        .find(|(_, c)| c.id == td.clip_id)
                    {
                        clip.start_tick = new_start;
                        clip.duration_ticks = new_dur;
                        if let ClipType::Audio {
                            ref mut sample_offset_ticks,
                            ..
                        } = clip.clip_type
                        {
                            *sample_offset_ticks = new_offset;
                        }
                        td.moved = true;
                        state.playlist_state.trim_drag = Some(td);
                        state.playlist_state.needs_full_sync = true;
                    }
                }
            }
            if pointer_released {
                state.playlist_state.trim_drag = None;
            }
        }

        // Handle Clip Click / Double-Click / Drag Start (with Shift multi-select & Alt+Drag duplicate)
        if let Some(cid) = dbl_clicked_clip_id {
            state.playlist_state.selected_clips = vec![cid];
            state.clip_editor.active_target =
                Some(crate::views::detail_editor_panel::ClipEditorTarget::Playlist { clip_id: cid });
            state.show_piano_roll = true;
            state.clip_editor.show_editor = false;
        } else if let Some(cid) = clicked_clip_id {
            match state.playlist_state.active_tool {
                playlist::PlaylistTool::Eraser => {
                    state.playlist_state.clips.retain(|(_, c)| c.id != cid);
                    state.playlist_state.selected_clips.retain(|&id| id != cid);
                    state.playlist_state.needs_full_sync = true;
                }
                playlist::PlaylistTool::Knife => {
                    state.playlist_state.selected_clips = vec![cid];
                    if let Some(pos) = pointer_pos {
                        let rel_x = (pos.x - timeline_x0 - scroll_x).max(0.0);
                        let raw_tick = px_to_ticks(rel_x, zoom_x);
                        let cut_tick = if shift_held {
                            raw_tick
                        } else {
                            snap_ticks(raw_tick, snap_step)
                        };
                        playlist::split_selected_clips_at(&mut state.playlist_state, cut_tick);
                    }
                }
                _ => {
                    if shift_held {
                        if let Some(pos_idx) =
                            state.playlist_state.selected_clips.iter().position(|&id| id == cid)
                        {
                            state.playlist_state.selected_clips.remove(pos_idx);
                        } else {
                            state.playlist_state.selected_clips.push(cid);
                        }
                    } else {
                        state.playlist_state.selected_clips = vec![cid];
                    }
                    state.clip_editor.active_target =
                        Some(crate::views::detail_editor_panel::ClipEditorTarget::Playlist {
                            clip_id: cid,
                        });
                }
            }
        }

        if let Some((mut cid, mut orig_tick, mut orig_track, gx, gy)) = start_drag_clip {
            if !state.playlist_state.selected_clips.contains(&cid) {
                state.playlist_state.selected_clips = vec![cid];
            }
            // Alt+Drag duplicates selected clips in place before dragging
            if alt_held {
                let mut dup_ids = Vec::new();
                let mut new_entries = Vec::new();
                for (tr_id, c) in &state.playlist_state.clips {
                    if state.playlist_state.selected_clips.contains(&c.id) {
                        let new_id = state.playlist_state.next_clip_id;
                        state.playlist_state.next_clip_id += 1;
                        let mut cloned = c.clone();
                        cloned.id = new_id;
                        if c.id == cid {
                            cid = new_id;
                            orig_tick = cloned.start_tick;
                            orig_track = *tr_id;
                        }
                        dup_ids.push(new_id);
                        new_entries.push((*tr_id, cloned));
                    }
                }
                state.playlist_state.clips.extend(new_entries);
                state.playlist_state.selected_clips = dup_ids;
                state.playlist_state.needs_full_sync = true;
            }
            let peers: Vec<(usize, u64, usize)> = state
                .playlist_state
                .clips
                .iter()
                .filter(|(_, c)| state.playlist_state.selected_clips.contains(&c.id) && c.id != cid)
                .map(|(tr_id, c)| (c.id, c.start_tick, *tr_id))
                .collect();
            state.playlist_state.clip_drag_peers = peers;
            state.playlist_state.clip_drag = Some(PlaylistClipDrag {
                clip_id: cid,
                orig_tick,
                orig_track,
                grab_dx_px: gx,
                grab_dy_px: gy,
                moved: false,
            });
        }

        // Update active multi-clip drag on timeline
        if let Some(mut drag) = state.playlist_state.clip_drag {
            if workspace_resp.dragged_by(egui::PointerButton::Primary) {
                if let Some(pos) = pointer_pos {
                    let new_x = (pos.x - drag.grab_dx_px - timeline_x0 - scroll_x).max(0.0);
                    let new_tick = snap_ticks(px_to_ticks(new_x, zoom_x), snap_step);
                    let new_row = ((pos.y - tracks_y0 - scroll_y) / row_h)
                        .floor()
                        .clamp(0.0, tracks_count.saturating_sub(1) as f32) as usize;
                    let new_track_id = new_row + 1;

                    let delta_tick = new_tick as i64 - drag.orig_tick as i64;
                    let delta_track = new_track_id as isize - drag.orig_track as isize;
                    let peers = state.playlist_state.clip_drag_peers.clone();

                    for (tr_id, clip) in &mut state.playlist_state.clips {
                        if clip.id == drag.clip_id {
                            clip.start_tick = new_tick;
                            *tr_id = new_track_id;
                        } else if let Some(&(_, p_orig_tick, p_orig_track)) =
                            peers.iter().find(|&&(pid, _, _)| pid == clip.id)
                        {
                            clip.start_tick = (p_orig_tick as i64 + delta_tick).max(0) as u64;
                            *tr_id = (p_orig_track as isize + delta_track)
                                .clamp(1, tracks_count.max(1) as isize) as usize;
                        }
                    }
                    drag.moved = true;
                    state.playlist_state.clip_drag = Some(drag);
                    state.playlist_state.needs_full_sync = true;
                }
            }
            if pointer_released {
                state.playlist_state.clip_drag = None;
                state.playlist_state.clip_drag_peers.clear();
            }
        }

        // Empty Grid: Click clear selection, Double-click insert MIDI clip, Drag Marquee selection
        if let Some(pos) = pointer_pos {
            if tl_grid_rect.contains(pos)
                && hovered_clip_id.is_none()
                && state.playlist_state.clip_drag.is_none()
                && state.playlist_state.trim_drag.is_none()
                && state.playlist_state.loop_drag_handle == playlist::LoopDragHandle::None
                && state.playlist_pan.is_none()
                && !is_dragging_sample
            {
                if (double_clicked || (primary_clicked && state.playlist_state.active_tool == playlist::PlaylistTool::Pen))
                    && tracks_count > 0
                {
                    let target_row = ((pos.y - tracks_y0 - scroll_y) / row_h)
                        .floor()
                        .clamp(0.0, tracks_count.saturating_sub(1) as f32) as usize;
                    let rel_x = (pos.x - timeline_x0 - scroll_x).max(0.0);
                    let start_tick = snap_ticks(px_to_ticks(rel_x, zoom_x), snap_step);
                    let new_id = state.playlist_state.next_clip_id;
                    state.playlist_state.next_clip_id += 1;
                    let track_id = target_row + 1;
                    let clip = playlist::PlaylistClip {
                        id: new_id,
                        name: format!("Clip {}", new_id),
                        start_tick,
                        duration_ticks: ticks_per_bar_val,
                        clip_type: ClipType::Pattern { pattern_id: new_id },
                        color: track_color_for(target_row),
                        muted: false,
                        loop_enabled: true,
                    };
                    state.playlist_state.clips.push((track_id, clip));
                    state.playlist_state.selected_clips = vec![new_id];
                    state.clip_editor.active_target =
                        Some(crate::views::detail_editor_panel::ClipEditorTarget::Playlist {
                            clip_id: new_id,
                        });
                    state.show_piano_roll = true;
                    state.clip_editor.show_editor = false;
                    state.playlist_state.needs_full_sync = true;
                } else if primary_clicked
                    && state.playlist_state.active_tool == playlist::PlaylistTool::Time
                {
                    let rel_x = (pos.x - timeline_x0 - scroll_x).max(0.0);
                    let tick = snap_ticks(px_to_ticks(rel_x, zoom_x), snap_step);
                    state.playlist_state.playhead_tick = tick;
                    state.playlist_state.selected_clips.clear();
                } else if primary_clicked && !shift_held && state.playlist_state.marquee.is_none() {
                    state.playlist_state.selected_clips.clear();
                } else if workspace_resp.drag_started_by(egui::PointerButton::Primary) {
                    state.playlist_state.marquee = Some(playlist::MarqueeState {
                        start_x: pos.x,
                        start_y: pos.y,
                        cur_x: pos.x,
                        cur_y: pos.y,
                    });
                }
            }
        }

        // Update & Draw Marquee Rectangular Selection
        if let Some(mut mq) = state.playlist_state.marquee {
            if workspace_resp.dragged_by(egui::PointerButton::Primary) {
                if let Some(pos) = pointer_pos {
                    mq.cur_x = pos.x.clamp(tl_grid_rect.min.x, tl_grid_rect.max.x);
                    mq.cur_y = pos.y.clamp(tl_grid_rect.min.y, tl_grid_rect.max.y);
                    state.playlist_state.marquee = Some(mq);
                    if mq.is_drag() {
                        let (x0, y0, x1, y1) = mq.normalized();
                        let mq_rect = Rect::from_min_max(pos2(x0, y0), pos2(x1, y1));
                        tl_painter.rect_filled(
                            mq_rect,
                            0.0,
                            Color32::from_rgba_unmultiplied(255, 255, 255, 34),
                        );
                        tl_painter.rect_stroke(
                            mq_rect,
                            0.0,
                            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 180)),
                            egui::StrokeKind::Inside,
                        );
                        let hit_ids: Vec<usize> = visible_clip_rects
                            .iter()
                            .filter(|(_, r)| r.intersects(mq_rect))
                            .map(|(id, _)| *id)
                            .collect();
                        if shift_held {
                            for id in hit_ids {
                                if !state.playlist_state.selected_clips.contains(&id) {
                                    state.playlist_state.selected_clips.push(id);
                                }
                            }
                        } else {
                            state.playlist_state.selected_clips = hit_ids;
                        }
                    }
                }
            }
            if pointer_released {
                state.playlist_state.marquee = None;
            }
        }

        // Right-click Context Menu on Clip or Empty Grid (Bitwig Studio 5.x look)
        let ctx_menu_id = ui.id().with("playlist_tl_ctx_menu");
        let audio_clip_ctx_menu_id = ui.id().with("playlist_audio_clip_ctx_menu");
        if let Some(cid) = right_clicked_clip_id {
            if !state.playlist_state.selected_clips.contains(&cid) {
                state.playlist_state.selected_clips = vec![cid];
            }
            if let Some(pos) = pointer_pos {
                let rel_x = (pos.x - timeline_x0 - scroll_x).max(0.0);
                let raw_tick = px_to_ticks(rel_x, zoom_x);
                let click_tick = if shift_held {
                    raw_tick
                } else {
                    snap_ticks(raw_tick, snap_step)
                };
                let is_audio_clip = state
                    .playlist_state
                    .clips
                    .iter()
                    .find(|(_, c)| c.id == cid)
                    .map(|(_, c)| matches!(c.clip_type, ClipType::Audio { .. }))
                    .unwrap_or(false);

                if is_audio_clip {
                    crate::ui::playlist_context_menu::close(ui.ctx(), ctx_menu_id);
                    crate::ui::playlist_audio_clip_context_menu::open(
                        ui.ctx(),
                        audio_clip_ctx_menu_id,
                        crate::ui::playlist_audio_clip_context_menu::PlaylistAudioClipMenuData {
                            pos,
                            clip_id: cid,
                            click_tick,
                        },
                    );
                } else {
                    crate::ui::playlist_audio_clip_context_menu::close(
                        ui.ctx(),
                        audio_clip_ctx_menu_id,
                    );
                    crate::ui::playlist_context_menu::open(
                        ui.ctx(),
                        ctx_menu_id,
                        crate::ui::playlist_context_menu::PlaylistContextMenuData {
                            pos,
                            click_tick,
                            on_clip: true,
                        },
                    );
                }
            }
        } else if secondary_clicked {
            if let Some(pos) = pointer_pos {
                if tl_grid_rect.contains(pos) {
                    let rel_x = (pos.x - timeline_x0 - scroll_x).max(0.0);
                    let raw_tick = px_to_ticks(rel_x, zoom_x);
                    let click_tick = if shift_held {
                        raw_tick
                    } else {
                        snap_ticks(raw_tick, snap_step)
                    };
                    crate::ui::playlist_audio_clip_context_menu::close(
                        ui.ctx(),
                        audio_clip_ctx_menu_id,
                    );
                    crate::ui::playlist_context_menu::open(
                        ui.ctx(),
                        ctx_menu_id,
                        crate::ui::playlist_context_menu::PlaylistContextMenuData {
                            pos,
                            click_tick,
                            on_clip: false,
                        },
                    );
                }
            }
        }

        if let Some(menu_data) = crate::ui::playlist_context_menu::get_data(ui.ctx(), ctx_menu_id) {
            let min_ticks = playlist::min_clip_ticks(ppqn);
            let sel_ids = &state.playlist_state.selected_clips;
            let playhead = state.playlist_state.playhead_tick;
            let can_split = state.playlist_state.clips.iter().any(|(_, c)| {
                if !sel_ids.contains(&c.id) {
                    return false;
                }
                let end = c.start_tick.saturating_add(c.duration_ticks);
                let click_inside = menu_data.click_tick > c.start_tick.saturating_add(min_ticks)
                    && menu_data.click_tick < end.saturating_sub(min_ticks);
                let playhead_inside = playhead > c.start_tick.saturating_add(min_ticks)
                    && playhead < end.saturating_sub(min_ticks);
                click_inside || playhead_inside
            });
            let can_set_clip_start = state.playlist_state.clips.iter().any(|(_, c)| {
                if !sel_ids.contains(&c.id) {
                    return false;
                }
                let end = c.start_tick.saturating_add(c.duration_ticks);
                menu_data.click_tick > c.start_tick
                    && menu_data.click_tick < end.saturating_sub(min_ticks)
            });
            let has_time_range =
                state.playlist_state.is_loop_region_valid() || !sel_ids.is_empty();

            let caps = crate::ui::playlist_context_menu::PlaylistContextMenuCapabilities {
                active_tool: state.playlist_state.active_tool,
                can_consolidate: !sel_ids.is_empty(),
                can_split,
                can_set_clip_start,
                can_bounce: false,
                can_slice: false,
                can_insert_silence: true,
                can_duplicate_time: has_time_range,
                can_remove_time: has_time_range,
            };

            if let Some(act) = crate::ui::playlist_context_menu::show(ui.ctx(), ctx_menu_id, caps) {
                use crate::ui::playlist_context_menu::PlaylistContextMenuAction;
                let selected_bounds = || -> Option<(u64, u64)> {
                    if state.playlist_state.is_loop_region_valid() {
                        return Some((
                            state.playlist_state.loop_start_ticks,
                            state.playlist_state.loop_end_ticks,
                        ));
                    }
                    let sel = &state.playlist_state.selected_clips;
                    let mut min_s: Option<u64> = None;
                    let mut max_e: Option<u64> = None;
                    for (_, c) in &state.playlist_state.clips {
                        if sel.contains(&c.id) {
                            let end = c.start_tick.saturating_add(c.duration_ticks);
                            min_s = Some(min_s.map_or(c.start_tick, |m| m.min(c.start_tick)));
                            max_e = Some(max_e.map_or(end, |m| m.max(end)));
                        }
                    }
                    match (min_s, max_e) {
                        (Some(s), Some(e)) if e > s => Some((s, e)),
                        _ => None,
                    }
                };

                match act {
                    PlaylistContextMenuAction::SelectTool(tool) => {
                        state.playlist_state.active_tool = tool;
                    }
                    PlaylistContextMenuAction::Consolidate => {
                        playlist::consolidate_selected_clips(&mut state.playlist_state);
                    }
                    PlaylistContextMenuAction::Split => {
                        let split_at_click = state.playlist_state.clips.iter().any(|(_, c)| {
                            state.playlist_state.selected_clips.contains(&c.id)
                                && menu_data.click_tick > c.start_tick.saturating_add(min_ticks)
                                && menu_data.click_tick
                                    < c.start_tick
                                        .saturating_add(c.duration_ticks)
                                        .saturating_sub(min_ticks)
                        });
                        let split_tick = if split_at_click {
                            menu_data.click_tick
                        } else {
                            state.playlist_state.playhead_tick
                        };
                        playlist::split_selected_clips_at(&mut state.playlist_state, split_tick);
                    }
                    PlaylistContextMenuAction::SetClipStart => {
                        playlist::set_clip_start_at(
                            &mut state.playlist_state,
                            menu_data.click_tick,
                        );
                    }
                    PlaylistContextMenuAction::InsertSilence => {
                        let dur = if state.playlist_state.is_loop_region_valid() {
                            state.playlist_state.loop_length_ticks()
                        } else {
                            ticks_per_bar_val
                        };
                        playlist::insert_silence_at(
                            &mut state.playlist_state,
                            menu_data.click_tick,
                            dur,
                        );
                    }
                    PlaylistContextMenuAction::DuplicateTime => {
                        if let Some((rs, re)) = selected_bounds() {
                            playlist::duplicate_time_range(&mut state.playlist_state, rs, re);
                        }
                    }
                    PlaylistContextMenuAction::RemoveTime => {
                        if let Some((rs, re)) = selected_bounds() {
                            playlist::remove_time_range(&mut state.playlist_state, rs, re);
                        }
                    }
                    PlaylistContextMenuAction::BounceInPlace
                    | PlaylistContextMenuAction::Bounce
                    | PlaylistContextMenuAction::SliceToDrumMachine
                    | PlaylistContextMenuAction::SliceToMultisample => {}
                }
            }
        }

        if crate::ui::playlist_audio_clip_context_menu::is_open(ui.ctx(), audio_clip_ctx_menu_id) {
            let menu_data = crate::ui::playlist_audio_clip_context_menu::get_data(
                ui.ctx(),
                audio_clip_ctx_menu_id,
            );
            let target_clip_id = menu_data.map(|d| d.clip_id);
            let (current_color, muted, looping) = target_clip_id
                .and_then(|cid| state.playlist_state.clips.iter().find(|(_, c)| c.id == cid))
                .map(|(_, c)| (c.color, c.muted, c.loop_enabled))
                .unwrap_or((Color32::from_rgb(0xE8, 0x4A, 0x24), false, true));

            let menu_state =
                crate::ui::playlist_audio_clip_context_menu::PlaylistAudioClipMenuState {
                    current_color,
                    muted,
                    looping,
                    has_clipboard: !state.playlist_state.clipboard.is_empty(),
                };

            if let Some(act) = crate::ui::playlist_audio_clip_context_menu::show(
                ui.ctx(),
                audio_clip_ctx_menu_id,
                menu_state,
            ) {
                use crate::ui::playlist_audio_clip_context_menu::PlaylistAudioClipMenuAction;
                let min_ticks = playlist::min_clip_ticks(ppqn);
                match act {
                    PlaylistAudioClipMenuAction::Recolor(col) => {
                        let sel = state.playlist_state.selected_clips.clone();
                        for (_, c) in &mut state.playlist_state.clips {
                            if sel.contains(&c.id) {
                                c.color = col;
                            }
                        }
                    }
                    PlaylistAudioClipMenuAction::ToggleMute => {
                        let sel = state.playlist_state.selected_clips.clone();
                        let new_muted = !muted;
                        for (_, c) in &mut state.playlist_state.clips {
                            if sel.contains(&c.id) {
                                c.muted = new_muted;
                            }
                        }
                        state.playlist_state.needs_full_sync = true;
                    }
                    PlaylistAudioClipMenuAction::ToggleLoop => {
                        let sel = state.playlist_state.selected_clips.clone();
                        let new_loop = !looping;
                        for (_, c) in &mut state.playlist_state.clips {
                            if sel.contains(&c.id) {
                                c.loop_enabled = new_loop;
                            }
                        }
                        state.playlist_state.needs_full_sync = true;
                    }
                    PlaylistAudioClipMenuAction::Cut => {
                        playlist::cut_selected_clips(&mut state.playlist_state);
                    }
                    PlaylistAudioClipMenuAction::Copy => {
                        playlist::copy_selected_clips(&mut state.playlist_state);
                    }
                    PlaylistAudioClipMenuAction::Paste => {
                        let target_tick = menu_data
                            .map(|d| d.click_tick)
                            .unwrap_or_else(|| state.playlist_state.paste_tick());
                        playlist::paste_clips(
                            &mut state.playlist_state,
                            target_tick,
                            state.selected_track_index,
                        );
                    }
                    PlaylistAudioClipMenuAction::Duplicate => {
                        let sel = state.playlist_state.selected_clips.clone();
                        let mut max_end: u64 = 0;
                        let mut min_start: u64 = u64::MAX;
                        let mut to_dup = Vec::new();
                        for (tid, c) in &state.playlist_state.clips {
                            if sel.contains(&c.id) {
                                min_start = min_start.min(c.start_tick);
                                max_end = max_end.max(c.start_tick.saturating_add(c.duration_ticks));
                                to_dup.push((*tid, c.clone()));
                            }
                        }
                        if !to_dup.is_empty() && max_end > min_start {
                            let shift = max_end - min_start;
                            let mut new_sel = Vec::new();
                            for (tid, mut c) in to_dup {
                                let new_id = state.playlist_state.next_clip_id;
                                state.playlist_state.next_clip_id += 1;
                                c.id = new_id;
                                c.start_tick = c.start_tick.saturating_add(shift);
                                new_sel.push(new_id);
                                state.playlist_state.clips.push((tid, c));
                            }
                            state.playlist_state.selected_clips = new_sel;
                            state.playlist_state.needs_full_sync = true;
                        }
                    }
                    PlaylistAudioClipMenuAction::Delete => {
                        let sel = state.playlist_state.selected_clips.clone();
                        state
                            .playlist_state
                            .clips
                            .retain(|(_, c)| !sel.contains(&c.id));
                        state.playlist_state.selected_clips.clear();
                        state.playlist_state.needs_full_sync = true;
                    }
                    PlaylistAudioClipMenuAction::LoopSelectedRegion => {
                        let sel = &state.playlist_state.selected_clips;
                        let mut min_s: Option<u64> = None;
                        let mut max_e: Option<u64> = None;
                        for (_, c) in &state.playlist_state.clips {
                            if sel.contains(&c.id) {
                                let end = c.start_tick.saturating_add(c.duration_ticks);
                                min_s = Some(min_s.map_or(c.start_tick, |m| m.min(c.start_tick)));
                                max_e = Some(max_e.map_or(end, |m| m.max(end)));
                            }
                        }
                        if let (Some(s), Some(e)) = (min_s, max_e) {
                            if e > s {
                                state.playlist_state.loop_start_ticks = s;
                                state.playlist_state.loop_end_ticks = e;
                                state.playlist_state.loop_region_active = true;
                                state.is_looping = true;
                            }
                        }
                    }
                    PlaylistAudioClipMenuAction::Consolidate => {
                        playlist::consolidate_selected_clips(&mut state.playlist_state);
                    }
                    PlaylistAudioClipMenuAction::DoubleContent
                    | PlaylistAudioClipMenuAction::ScaleEach200
                    | PlaylistAudioClipMenuAction::Scale200 => {
                        let sel = state.playlist_state.selected_clips.clone();
                        for (_, c) in &mut state.playlist_state.clips {
                            if sel.contains(&c.id) {
                                c.duration_ticks = c.duration_ticks.saturating_mul(2).max(min_ticks);
                            }
                        }
                        state.playlist_state.needs_full_sync = true;
                    }
                    PlaylistAudioClipMenuAction::ScaleEach50
                    | PlaylistAudioClipMenuAction::Scale50 => {
                        let sel = state.playlist_state.selected_clips.clone();
                        for (_, c) in &mut state.playlist_state.clips {
                            if sel.contains(&c.id) {
                                c.duration_ticks = (c.duration_ticks / 2).max(min_ticks);
                            }
                        }
                        state.playlist_state.needs_full_sync = true;
                    }
                    PlaylistAudioClipMenuAction::Normalize => {
                        let sel = state.playlist_state.selected_clips.clone();
                        for (_, c) in &mut state.playlist_state.clips {
                            if sel.contains(&c.id) {
                                if let ClipType::Audio { ref mut peaks, .. } = c.clip_type {
                                    let max_p = peaks
                                        .iter()
                                        .copied()
                                        .fold(0.0_f32, |a, b| a.max(b.abs()));
                                    if max_p > 1e-4 {
                                        let gain = 1.0 / max_p;
                                        for p in peaks.iter_mut() {
                                            *p = (*p * gain).clamp(-1.0, 1.0);
                                        }
                                    }
                                }
                            }
                        }
                        state.playlist_state.needs_full_sync = true;
                    }
                    PlaylistAudioClipMenuAction::Quantize
                    | PlaylistAudioClipMenuAction::QuantizeAudio => {
                        let sel = state.playlist_state.selected_clips.clone();
                        for (_, c) in &mut state.playlist_state.clips {
                            if sel.contains(&c.id) {
                                c.start_tick = snap_ticks(c.start_tick, snap_step);
                            }
                        }
                        state.playlist_state.needs_full_sync = true;
                    }
                    PlaylistAudioClipMenuAction::ScaleCustom
                    | PlaylistAudioClipMenuAction::BounceInPlace
                    | PlaylistAudioClipMenuAction::Bounce
                    | PlaylistAudioClipMenuAction::SliceInPlace
                    | PlaylistAudioClipMenuAction::SliceToDrumMachine
                    | PlaylistAudioClipMenuAction::SliceToMultisample
                    | PlaylistAudioClipMenuAction::FoldToTakes
                    | PlaylistAudioClipMenuAction::ResetFades
                    | PlaylistAudioClipMenuAction::AutoFade
                    | PlaylistAudioClipMenuAction::AutoCrossfade
                    | PlaylistAudioClipMenuAction::TransposeOctaveUp
                    | PlaylistAudioClipMenuAction::TransposeOctaveDown
                    | PlaylistAudioClipMenuAction::QuantizeOptions
                    | PlaylistAudioClipMenuAction::QuantizeAudioOptions
                    | PlaylistAudioClipMenuAction::MakeLegato
                    | PlaylistAudioClipMenuAction::SaveArrangerClipToLibrary => {}
                }
            }
        }

        // Handle WAV Drag & Drop onto Timeline Grid
        if let Some(pos) = pointer_pos {
            if tl_grid_rect.contains(pos) && is_dragging_sample {
                let target_row = ((pos.y - tracks_y0 - scroll_y) / row_h)
                    .floor()
                    .clamp(0.0, tracks_count.saturating_sub(1) as f32) as usize;
                let rel_x = (pos.x - timeline_x0 - scroll_x).max(0.0);
                let target_tick = snap_ticks(px_to_ticks(rel_x, zoom_x), snap_step);
                let preview_x = timeline_x0 + scroll_x + ticks_to_px(target_tick, zoom_x);
                let preview_y = tracks_y0 + scroll_y + target_row as f32 * row_h + 2.0;
                let preview_w = ticks_to_px(ticks_per_bar_val, zoom_x).max(48.0);
                let preview_rect =
                    Rect::from_min_size(pos2(preview_x, preview_y), vec2(preview_w, row_h - 5.0));

                tl_painter.rect_filled(
                    preview_rect,
                    2.5,
                    Color32::from_rgba_unmultiplied(255, 85, 0, 95),
                );
                tl_painter.rect_stroke(
                    preview_rect,
                    2.5,
                    Stroke::new(2.0_f32, ACCENT_ORANGE),
                    egui::StrokeKind::Inside,
                );

                if pointer_released {
                    if let Some(ref sample_path) = state.dragged_sample {
                        drop_timeline_action =
                            Some((target_row + 1, target_tick, sample_path.clone()));
                    }
                }
            }
        }

        // Vertical White Guideline during Middle-Drag Zoom & Pan (Bitwig 1:1)
        if let Some(pan) = state.playlist_pan {
            if pan.moved {
                let gx = pan.last.0.clamp(timeline_x0, timeline_x1);
                tl_painter.line_segment(
                    [pos2(gx, tracks_y0), pos2(gx, tracks_y1)],
                    Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 175)),
                );
            }
        }

        // Bitwig-style Adaptive Grid Resolution Pill in bottom-right corner of timeline
        let badge_rect = Rect::from_min_size(
            pos2(timeline_x1 - 52.0, tracks_y1 - 20.0),
            vec2(46.0, 16.0),
        );
        tl_painter.rect_filled(
            badge_rect,
            3.0,
            Color32::from_rgba_unmultiplied(0x1E, 0x20, 0x24, 210),
        );
        tl_painter.text(
            badge_rect.center(),
            egui::Align2::CENTER_CENTER,
            grid_label,
            fonts::semibold(9.5),
            Color32::from_rgb(0xC8, 0xCA, 0xD2),
        );

        // Cyan Bitwig Playhead Line + Flag on Timeline
        let playhead_px =
            timeline_x0 + scroll_x + ticks_to_px(state.playlist_state.playhead_tick, zoom_x);
        if playhead_px >= timeline_x0 && playhead_px <= timeline_x1 {
            painter.line_segment(
                [pos2(playhead_px, toolbar_y0 + 10.0), pos2(playhead_px, master_y1)],
                Stroke::new(1.4_f32, PLAYHEAD_COLOR),
            );
            let tri = [
                pos2(playhead_px - 4.5, toolbar_y0 + 10.0),
                pos2(playhead_px + 4.5, toolbar_y0 + 10.0),
                pos2(playhead_px, toolbar_y0 + 18.0),
            ];
            painter.add(egui::Shape::convex_polygon(
                tri.to_vec(),
                PLAYHEAD_COLOR,
                Stroke::NONE,
            ));
        }

        // Suppress unused warning if ctrl_held not used in this block
        let _ = ctrl_held;
    }

    // =========================================================================
    // 5. DEFERRED ACTIONS (Clips, Drops, Track/Scene Mutations)
    // =========================================================================
    if add_fx_action {
        let fx_idx = state.add_fx_track();
        state.selected_fx_index = Some(fx_idx);
    }
    if add_group_action {
        let next_id = state.live_tracks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        let name = format!("Group {}", next_id);
        state.matrix_state.add_track();
        if let Some(last) = state.matrix_state.tracks.last_mut() {
            last.name = name.clone();
        }
        let tr = crate::views::mixer::Track::new_group(next_id, name);
        state.live_tracks.push(tr);
    }
    if let Some(target_mixer_idx) = group_track_action {
        if target_mixer_idx > 0 && target_mixer_idx < state.live_tracks.len() {
            let next_id = state.live_tracks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
            let name = format!("Group {}", next_id);
            let parent_of_target = state.live_tracks[target_mixer_idx].parent_group_id;
            let parent_route = state.live_tracks[target_mixer_idx].route_destination_id;
            let mut grp = crate::views::mixer::Track::new_group(next_id, name.clone());
            grp.parent_group_id = parent_of_target;
            grp.route_destination_id = parent_route;
            state.live_tracks[target_mixer_idx].parent_group_id = Some(next_id);
            state.live_tracks[target_mixer_idx].route_destination_id = next_id;
            state.live_tracks.insert(target_mixer_idx, grp);
            let matrix_idx = target_mixer_idx - 1;
            state.matrix_state.insert_track(matrix_idx, name);
            state.selected_track_index = target_mixer_idx;
        }
    }
    if let Some(grp_mixer_idx) = toggle_group_expand_action {
        if let Some(tr) = state.live_tracks.get_mut(grp_mixer_idx) {
            if tr.is_group {
                tr.is_expanded = !tr.is_expanded;
            }
        }
    }
    if let Some((grp_mixer_idx, scene_idx)) = trigger_group_scene_action {
        if let Some(grp_id) = state.live_tracks.get(grp_mixer_idx).map(|t| t.id) {
            let children = crate::views::mixer::group_children_indices(&state.live_tracks, grp_id);
            for child_mixer_idx in children {
                if child_mixer_idx > 0 {
                    let child_matrix_idx = child_mixer_idx - 1;
                    trigger_pad(&mut state.matrix_state, &state.audio_proxy, child_matrix_idx, scene_idx);
                }
            }
        }
    }
    if let Some(mixer_idx) = delete_track_action {
        state.delete_track(mixer_idx);
    }

    if add_track_action {
        state.popup_browser.open_new_instrument();
    }

    if add_scene_action {
        let new_idx = state.matrix_state.add_scene_synced(&state.audio_proxy);
        state.matrix_state.selected_scene = Some(new_idx);
        state.matrix_state.selected_slot = None;
    }

    if let Some(s_idx) = select_scene_action {
        state.matrix_state.selected_scene = Some(s_idx);
        state.matrix_state.selected_slot = None;
        state.playlist_state.selected_clips.clear();
        state.selected_fx_index = None;
        if let Some(focused) = ui.ctx().memory(|m| m.focused()) {
            ui.ctx().memory_mut(|m| m.surrender_focus(focused));
        }
    }

    if let Some((s_idx, click_pos)) = open_scene_menu_action {
        state.matrix_state.selected_scene = Some(s_idx);
        state.matrix_state.selected_slot = None;
        state.matrix_state.context_menu = None;
        state.matrix_state.scene_context_menu = Some(SceneContextMenu {
            scene_idx: s_idx,
            click_pos,
        });
    }

    if let Some((t_idx, s_idx, path)) = drop_slot_action {
        load_clip_into_slot(
            &mut state.matrix_state,
            &state.audio_proxy,
            t_idx,
            s_idx,
            path,
            state.transport.bpm,
        );
        select_matrix_track(state, t_idx);
        state.dragged_sample = None;
    }

    if let Some((track_id, start_tick, path)) = drop_timeline_action {
        let new_id = state.playlist_state.next_clip_id;
        state.playlist_state.next_clip_id += 1;
        let name = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let ppqn = state.playlist_state.ppqn.max(1);
        let bpm = state.transport.bpm;
        let mut clip = build_audio_clip(new_id, name, start_tick, path.clone(), ppqn, bpm);
        clip.color = track_color_for(track_id.saturating_sub(1));
        let duration_ticks = clip.duration_ticks;
        state.playlist_state.clips.push((track_id, clip));
        state.playlist_state.selected_clips = vec![new_id];
        state.playlist_state.needs_full_sync = true;

        // Immediately sync to audio engine
        let position_secs = ticks_to_secs_precise(start_tick, ppqn, bpm);
        let duration_secs = ticks_to_secs_precise(duration_ticks, ppqn, bpm);
        let speed_ratio = crate::audio_proxy::compute_tempo_sync_ratio(&path, bpm);
        state.audio_proxy.send(GuiCommand::LoadClip {
            clip_id: new_id,
            path: path.to_string_lossy().to_string(),
            position_secs,
            duration_secs,
            offset_secs: 0.0,
            track_index: track_id.saturating_sub(1),
            scene_index: 0,
            speed_ratio,
        });

        sync_playlist_to_engine(state);

        state.selected_track_index = track_id;
        state.dragged_sample = None;
    }

    if state.playlist_state.needs_full_sync
        && state.playlist_state.clip_drag.is_none()
        && state.playlist_state.trim_drag.is_none()
    {
        sync_playlist_to_engine(state);
    }

    if let Some((t_idx, s_idx)) = trigger_slot_action {
        state.matrix_state.selected_slot = Some((t_idx, s_idx));
        select_matrix_track(state, t_idx);

        if let Some(slot) = state.matrix_state.slot(t_idx, s_idx) {
            if let Some(ref clip) = slot.clip {
                state.clip_editor.active_target =
                    Some(crate::views::detail_editor_panel::ClipEditorTarget::Matrix {
                        track_idx: t_idx,
                        slot_idx: s_idx,
                    });
                state.clip_editor.cached_name = clip.name.clone();
                state.clip_editor.cached_duration_secs = clip.duration_secs();
                let first_ev = clip.audio_events().first();
                state.clip_editor.cached_sample_rate = first_ev.map(|e| e.sample_rate).unwrap_or(44100);
                state.clip_editor.cached_gain = first_ev.map(|e| e.gain).unwrap_or(1.0);
                state.clip_editor.cached_pan = 0.0;
                state.clip_editor.cached_stretch_mode = hikaru_core::audio_clip_event::StretchMode::Raw;
            }
        }

        trigger_pad(&mut state.matrix_state, &state.audio_proxy, t_idx, s_idx);
    }

    if let Some((t_idx, s_idx)) = select_slot_action {
        state.matrix_state.selected_slot = Some((t_idx, s_idx));
        select_matrix_track(state, t_idx);

        if let Some(slot) = state.matrix_state.slot(t_idx, s_idx) {
            if let Some(ref clip) = slot.clip {
                state.clip_editor.active_target =
                    Some(crate::views::detail_editor_panel::ClipEditorTarget::Matrix {
                        track_idx: t_idx,
                        slot_idx: s_idx,
                    });
                state.clip_editor.cached_name = clip.name.clone();
                state.clip_editor.cached_duration_secs = clip.duration_secs();
                let first_ev = clip.audio_events().first();
                state.clip_editor.cached_sample_rate = first_ev.map(|e| e.sample_rate).unwrap_or(44100);
                state.clip_editor.cached_gain = first_ev.map(|e| e.gain).unwrap_or(1.0);
                state.clip_editor.cached_pan = 0.0;
                state.clip_editor.cached_stretch_mode = hikaru_core::audio_clip_event::StretchMode::Raw;
            } else {
                state.clip_editor.active_target = None;
            }
        } else {
            state.clip_editor.active_target = None;
        }
    }

    if let Some((t_idx, s_idx, click_pos)) = open_context_menu_action {
        state.matrix_state.context_menu = Some(crate::views::clip_context_menu::ClipLauncherContextMenu {
            track_idx: t_idx,
            scene_idx: s_idx,
            click_pos,
        });
    }

    if let Some((t_idx, s_idx)) = create_midi_slot_action {
        create_empty_midi_clip(&mut state.matrix_state, &state.audio_proxy, t_idx, s_idx);
        select_matrix_track(state, t_idx);
        state.show_piano_roll = true;
        state.show_dsp_rack = false;
        state.clip_editor.show_editor = false;
    }

    if open_piano_roll_action {
        state.show_piano_roll = true;
        state.show_dsp_rack = false;
        state.clip_editor.show_editor = false;
    }

    if let Some(t_idx) = stop_track_action {
        state.matrix_state.stop_track(&state.audio_proxy, t_idx);
    }

    if let Some(s_idx) = trigger_scene_action {
        trigger_scene(&mut state.matrix_state, &state.audio_proxy, s_idx);
    }

    if let Some(t_idx) = toggle_mute_action {
        if let Some(t) = state.matrix_state.tracks.get_mut(t_idx) {
            t.muted = !t.muted;
            let mixer_idx = mixer_index_of_matrix_track(t_idx);
            if let Some(lt) = state.live_tracks.get_mut(mixer_idx) {
                lt.mute = t.muted;
            }
            state.audio_proxy.send(GuiCommand::SetTrackMute {
                track_idx: t_idx,
                mute: t.muted,
            });
        }
    }

    if let Some(t_idx) = toggle_solo_action {
        if let Some(t) = state.matrix_state.tracks.get_mut(t_idx) {
            t.soloed = !t.soloed;
            let mixer_idx = mixer_index_of_matrix_track(t_idx);
            if let Some(lt) = state.live_tracks.get_mut(mixer_idx) {
                lt.solo = t.soloed;
            }
            state.audio_proxy.send(GuiCommand::SetTrackSolo {
                track_idx: t_idx,
                solo: t.soloed,
            });
        }
    }

    if let Some(t_idx) = toggle_arm_action {
        let m_idx = mixer_index_of_matrix_track(t_idx);
        if let Some(tr) = state.live_tracks.get_mut(m_idx) {
            tr.arm = !tr.arm;
        }
    }

    crate::views::clip_context_menu::render_clip_context_menu(ui, state);
    render_scene_context_menu(ui, state);
}

/// Menú contextual de cabecera de escena (click derecho, estilo Bitwig Studio):
/// paleta de colores 9x3 + acciones CUT / COPY / PASTE / DUPL. / DELETE.
pub fn render_scene_context_menu(ui: &mut Ui, state: &mut AppState) {
    let Some(menu) = state.matrix_state.scene_context_menu.clone() else {
        return;
    };
    let scene_idx = menu.scene_idx;
    if scene_idx >= state.matrix_state.scenes.len() {
        state.matrix_state.scene_context_menu = None;
        return;
    }

    let mc = crate::ui::scene_context_menu::SceneMenuContext {
        current_color: state.matrix_state.scenes[scene_idx].color,
        can_paste: state.matrix_clipboard.has_scene(),
        can_delete: state.matrix_state.scenes.len() > 1,
    };

    let (action, should_close) = crate::ui::scene_context_menu::show(
        ui.ctx(),
        egui::Id::new("bitwig_scene_context_menu_area"),
        menu.click_pos,
        &mc,
    );

    if should_close {
        state.matrix_state.scene_context_menu = None;
    }

    if let Some(act) = action {
        use crate::ui::scene_context_menu::SceneMenuAction;
        match act {
            SceneMenuAction::SetColor(col) => {
                state.matrix_state.set_scene_color(scene_idx, col);
            }
            SceneMenuAction::Rename => {
                if let Some(scene) = state.matrix_state.scenes.get(scene_idx) {
                    state.matrix_state.renaming_scene = Some((scene_idx, scene.name.clone()));
                }
            }
            SceneMenuAction::Cut => {
                state.cut_scene_at(scene_idx);
            }
            SceneMenuAction::Copy => {
                state.copy_scene_at(scene_idx);
            }
            SceneMenuAction::Paste => {
                state.paste_scene_at(scene_idx);
            }
            SceneMenuAction::Duplicate => {
                state.duplicate_scene_at(scene_idx);
            }
            SceneMenuAction::Delete => {
                state.remove_scene_at(scene_idx);
            }
        }
    }
}

#[allow(dead_code)]
fn pitch_to_note_name(pitch: u8) -> &'static str {
    let names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    names[(pitch % 12) as usize]
}

pub fn pan_standard_angle(pan: f32) -> f32 {
    -std::f32::consts::PI / 2.0 + pan * (std::f32::consts::PI / 4.0)
}

pub fn pan_sweep_angle(pan: f32) -> f32 {
    if pan == 0.0 {
        0.0
    } else {
        pan * (3.0 * std::f32::consts::PI / 4.0)
    }
}

pub fn snap_center_pan(pan: f32) -> f32 {
    if pan.abs() < 0.02 {
        0.0
    } else {
        pan
    }
}

pub fn pan_needle_tip(pan: f32, cx: f32, cy: f32, len: f32) -> (f32, f32) {
    if pan == 0.0 {
        (cx, cy - len)
    } else {
        let angle = pan_sweep_angle(pan);
        (cx + len * angle.sin(), cy - len * angle.cos())
    }
}

/// Sync all audio clips in `state.playlist_state` to the audio engine and clear `needs_full_sync`.
pub fn sync_playlist_to_engine(state: &mut AppState) {
    let sr = state.transport.sample_rate.get().max(1.0);
    let clips_to_sync: Vec<AudioClipData> = state
        .playlist_state
        .clips
        .iter()
        .filter_map(|(t_id, c)| {
            if let ClipType::Audio {
                sample_path,
                sample_offset_ticks,
                ..
            } = &c.clip_type
            {
                let len_frames = state.transport.clip_length_frames(c.duration_ticks);
                Some(AudioClipData {
                    clip_id: c.id,
                    path: sample_path.clone(),
                    start_secs: state.transport.ticks_to_samples(c.start_tick) as f32 / sr,
                    duration_secs: len_frames as f32 / sr,
                    offset_secs: state.transport.ticks_to_samples(*sample_offset_ticks) as f32 / sr,
                    track_index: t_id.saturating_sub(1),
                    speed_ratio: crate::audio_proxy::compute_tempo_sync_ratio(
                        std::path::Path::new(sample_path),
                        state.transport.bpm,
                    ),
                })
            } else {
                None
            }
        })
        .collect();
    state
        .audio_proxy
        .send(GuiCommand::SyncPlaylistClips { clips: clips_to_sync });
    state.playlist_state.needs_full_sync = false;
}
