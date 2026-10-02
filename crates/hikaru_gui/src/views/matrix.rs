// Copyright (C) Hikaru Corporation - 2026
// Hikaru OpenLive - Session Matrix with GPUI Kit Interface
// Bitwig's Clip Launcher-like made full in Rust.
// GNU Affero General Public License v3
// crates/hikaru_gui/src/views/matrix.rs

use std::path::PathBuf;

use hikaru_audio_engine::AudioEngine;

use crate::audio_proxy::{AudioProxy, GuiCommand};
pub use crate::views::clipboard::MatrixClipboard;
use crate::views::mixer::Track;
use crate::views::playlist::PlaylistState;

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
    pub has_time_selection: bool,
}

impl MatrixClip {
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
            events.push(AudioEvent::new_full(id, name, samples, channels, sr, start));
        }
        clip.refresh_preview();
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

use std::sync::{Arc, Mutex};

use gpui_kit::component::*;
use gpui_kit::component::button::Button;
use gpui_kit::component::label::Label;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::prelude::{InteractiveElement as _, Styled as _};
use gpui_kit::*;
use crate::app::{state, HikaruApp};

fn slot_colors(slot_state: &SlotState, has_clip: bool, is_selected: bool) -> (Hsla, Hsla) {
    if is_selected {
        // Focus/selección: resaltado azul profesional.
        return (rgb(0x2D3A4A).into(), rgb(0x5AB4FF).into());
    }
    if !has_clip {
        return (rgb(0x252525).into(), rgb(0x3A3A3A).into());
    }
    match slot_state {
        SlotState::Stopped => (rgb(0x35404F).into(), rgb(0x6B9FD4).into()),
        SlotState::QueuedToPlay => (rgb(0x5C4A1A).into(), rgb(0xE6C84C).into()),
        // En reproducción: verde vibrante + borde resaltado.
        SlotState::Playing => (rgb(0x1DB954).into(), rgb(0x4CFF8A).into()),
        SlotState::QueuedToStop => (rgb(0x6B2A2A).into(), rgb(0xE06060).into()),
        SlotState::Empty => (rgb(0x252525).into(), rgb(0x3A3A3A).into()),
    }
}

fn slot_hover_border(slot_state: &SlotState, has_clip: bool) -> Hsla {
    if !has_clip {
        // Vacío: borde sutilmente encendido al hover para invitar a crear/cargar.
        return rgb(0x0096BE).into();
    }
    match slot_state {
        SlotState::Playing => rgb(0xB6FFD2).into(),
        SlotState::QueuedToPlay | SlotState::QueuedToStop => rgb(0xFFFFFF).into(),
        _ => rgb(0x9CC8FF).into(),
    }
}

fn slot_display_text(slot_state: &SlotState, has_clip: bool, clip_name: &str) -> String {
    if !has_clip {
        // Celda vacía: pad oscuro limpio, sin texto genérico.
        return String::new();
    }
    match slot_state {
        SlotState::Playing => {
            if clip_name.is_empty() {
                "▶ Playing".to_string()
            } else {
                format!("▶ {}", clip_name)
            }
        }
        SlotState::QueuedToPlay => "… Play".to_string(),
        SlotState::QueuedToStop => "■ Stop".to_string(),
        _ => clip_name.to_string(),
    }
}

fn play_glyph(slot_state: &SlotState, has_clip: bool) -> &'static str {
    if !has_clip {
        // Vacío: no se usa glyph de texto; se dibuja cuadrado Stop sólido (ver render_pad).
        return "■";
    }
    match slot_state {
        SlotState::Playing => "■",
        SlotState::QueuedToPlay => "…",
        SlotState::QueuedToStop => "…",
        _ => "▶",
    }
}

#[allow(clippy::too_many_arguments)]
fn render_pad(
    track_idx: usize,
    scene_idx: usize,
    slot_state: SlotState,
    clip_name: String,
    has_clip: bool,
    is_selected: bool,
    drop_sample_ready: bool,
    clipboard_ready: bool,
    engine_handle: Option<Arc<Mutex<AudioEngine<'static>>>>,
) -> AnyElement {
    let (bg, border) = slot_colors(&slot_state, has_clip, is_selected);
    let hover_border = slot_hover_border(&slot_state, has_clip);
    let display_text = slot_display_text(&slot_state, has_clip, &clip_name);
    let glyph = play_glyph(&slot_state, has_clip);
    let is_playing = slot_state == SlotState::Playing;
    let is_empty = !has_clip;
    let status_dot: Hsla = if is_empty {
        rgb(0x5A5A5A).into()
    } else {
        match slot_state {
            SlotState::Playing => rgb(0x0AFF6B).into(),
            SlotState::QueuedToPlay => rgb(0xE6C84C).into(),
            SlotState::QueuedToStop => rgb(0xE06060).into(),
            _ => rgb(0x6B9FD4).into(),
        }
    };

    let pad = div()
        .id(SharedString::from(format!("matrix_pad_{}_{}", track_idx, scene_idx)))
        .relative()
        .w(px(110.0))
        .h(px(54.0))
        .bg(bg)
        .border_1()
        .border_color(border)
        .rounded(px(3.0))
        .overflow_hidden()
        .cursor_pointer()
        // Hover: borde azul sutilmente encendido + leve variación de fondo.
        .hover(move |this| this.border_color(hover_border).bg(rgb(0x2E3A4A)))
        .when(is_selected, |d| d.border_2().border_color(rgb(0x5AB4FF)))
        .when(drop_sample_ready && is_empty, |d| {
            d.border_color(rgb(0x0096BE)).border_2()
        })
        .when(is_playing, |d| {
            d.border_2().border_color(rgb(0x4CFF8A))
        })
        // Click en el cuerpo del pad: seleccionar + drop de sample o trigger.
        .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| {
            let st = state(cx);
            st.update(cx, |s, cx| {
                // Si hay un sample arrastrado desde el explorer, cargarlo aquí.
                if let Some(path) = s.dragged_sample.take() {
                    if crate::views::explorer::is_audio_file(&path)
                        || crate::views::explorer::is_midi_file(&path)
                    {
                        s.matrix_state.selected_slot = Some((track_idx, scene_idx));
                        let bpm = s.transport.bpm;
                        let proxy = s.audio_proxy.clone();
                        load_clip_into_slot(
                            &mut s.matrix_state,
                            &proxy,
                            track_idx,
                            scene_idx,
                            path,
                            bpm,
                        );
                        cx.notify();
                        return;
                    } else {
                        // No era un archivo válido: devolverlo para no perderlo.
                        s.dragged_sample = Some(path);
                    }
                }
                s.matrix_state.selected_slot = Some((track_idx, scene_idx));
                if has_clip {
                    // Con clip (Stopped/Playing/...): el pad completo es disparador.
                    let proxy = s.audio_proxy.clone();
                    trigger_pad(&mut s.matrix_state, &proxy, track_idx, scene_idx);
                } else {
                    // Vacío: queda seleccionado para que el Clip Editor ofrezca
                    // "crear/grabar clip" o cargar un sample/MIDI.
                    // No se dispara nada al engine (trigger_pad lo ignoraría).
                }
                cx.notify();
            });
        })
        // Progreso del loop / playhead + fondo pulsante en Playing.
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    if is_playing {
                        // Barra de progreso inferior (loop).
                        if let Some(ref handle) = engine_handle {
                            if let Ok(engine) = handle.try_lock() {
                                if let Some((frame, total)) =
                                    engine.voice_playhead_frame(track_idx, scene_idx)
                                {
                                    if total > 0 {
                                        let progress =
                                            (frame as f32 / total as f32).clamp(0.0, 1.0);
                                        // Relleno de progreso.
                                        window.paint_quad(PaintQuad {
                                            bounds: Bounds::new(
                                                bounds.origin,
                                                size(
                                                    bounds.size.width * progress,
                                                    bounds.size.height,
                                                ),
                                            ),
                                            background: rgb(0xFFFFFF).into(),
                                            border_color: Hsla::default(),
                                            corner_radii: gpui_kit::Corners::default(),
                                            border_widths: gpui_kit::Edges::default(),
                                            border_style: BorderStyle::default(),
                                        });
                                        // Playhead vertical.
                                        let x = bounds.origin.x
                                            + bounds.size.width * progress;
                                        let mut path = PathBuilder::stroke(px(1.5));
                                        path.move_to(point(x, bounds.origin.y));
                                        path.line_to(point(
                                            x,
                                            bounds.origin.y + bounds.size.height,
                                        ));
                                        if let Ok(p) = path.build() {
                                            window.paint_path(p, rgb(0x062B16));
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
            )
            .absolute()
            .inset_0(),
        )
        // Botón Play/Stop pequeño (estilo Arranger: cuadrado sólido en vacíos).
        .child(
            div()
                .absolute()
                .left(px(5.0))
                .top(px(5.0))
                .w(px(20.0))
                .h(px(20.0))
                .rounded(px(3.0))
                .bg(if has_clip {
                    if is_playing {
                        rgb(0x062B16)
                    } else {
                        rgb(0x1B6FB5)
                    }
                } else {
                    rgb(0x32323C)
                })
                .border_1()
                .border_color(if is_playing {
                    rgb(0x4CFF8A)
                } else {
                    rgb(0x5A5A5A)
                })
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|this| this.border_color(rgb(0x5AB4FF)))
                .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |s, cx| {
                        s.matrix_state.selected_slot = Some((track_idx, scene_idx));
                        if has_clip {
                            let proxy = s.audio_proxy.clone();
                            trigger_pad(&mut s.matrix_state, &proxy, track_idx, scene_idx);
                        }
                        // En vacío el mini-botón no dispara: sólo selecciona.
                        cx.notify();
                    });
                })
                .child(if has_clip {
                    Label::new(glyph)
                        .text_size(px(10.0))
                        .text_color(rgb(0xFFFFFF))
                        .into_any_element()
                } else {
                    // Stop: cuadrado sólido limpio, al estilo del Arranger.
                    div().w(px(7.0)).h(px(7.0)).bg(rgb(0xFFFFFF)).into_any_element()
                }),
        )
        // Punto de estado + nombre del clip (solo si hay clip; vacío = pad limpio).
        .child(
            div()
                .absolute()
                .left(px(30.0))
                .right(px(4.0))
                .top_0()
                .bottom_0()
                .flex()
                .flex_col()
                .justify_center()
                .gap(px(1.0))
                .overflow_hidden()
                .when(has_clip, move |this| {
                    let status_label = match slot_state {
                        SlotState::Playing => "PLAYING".to_string(),
                        SlotState::QueuedToPlay => "QUEUED ▶".to_string(),
                        SlotState::QueuedToStop => "STOPPING".to_string(),
                        _ => "STOPPED".to_string(),
                    };
                    this.child(
                        h_flex()
                            .items_center()
                            .gap(px(4.0))
                            .child(div().w(px(6.0)).h(px(6.0)).rounded_full().bg(status_dot))
                            .child(
                                Label::new(status_label)
                                    .text_size(px(7.0))
                                    .text_color(rgb(0xFFFFFF)),
                            ),
                    )
                    .child(
                        Label::new(display_text.clone())
                            .text_size(px(9.0))
                            .text_color(rgb(0xFFFFFF)),
                    )
                }),
        );

    pad.context_menu(move |menu: PopupMenu, _window, _cx| {
        menu.item(
            PopupMenuItem::new("Copiar")
                .disabled(!has_clip)
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |s, cx| {
                        let clipboard = &mut s.matrix_clipboard;
                        s.matrix_state.copy_slot(track_idx, scene_idx, clipboard);
                        cx.notify();
                    });
                }),
        )
        .item(
            PopupMenuItem::new("Cortar")
                .disabled(!has_clip)
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |s, cx| {
                        let proxy = s.audio_proxy.clone();
                        let clipboard = &mut s.matrix_clipboard;
                        s.matrix_state
                            .cut_slot(track_idx, scene_idx, clipboard, &proxy);
                        cx.notify();
                    });
                }),
        )
        .item(
            PopupMenuItem::new("Pegar")
                .disabled(!clipboard_ready)
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |s, cx| {
                        let proxy = s.audio_proxy.clone();
                        let clipboard = &s.matrix_clipboard;
                        s.matrix_state
                            .paste_slot(track_idx, scene_idx, clipboard, &proxy);
                        cx.notify();
                    });
                }),
        )
        .item(PopupMenuItem::separator())
        .item(
            PopupMenuItem::new("Duplicar")
                .disabled(!has_clip)
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |s, cx| {
                        let proxy = s.audio_proxy.clone();
                        s.matrix_state
                            .duplicate_slot(track_idx, scene_idx, &proxy);
                        cx.notify();
                    });
                }),
        )
        .item(
            PopupMenuItem::new("Eliminar")
                .disabled(!has_clip)
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |s, cx| {
                        let proxy = s.audio_proxy.clone();
                        s.matrix_state
                            .delete_slot(track_idx, scene_idx, &proxy);
                        cx.notify();
                    });
                }),
        )
    })
    .into_any_element()
}

fn render_scene_launcher(scene_idx: usize, name: String, has_any_clip: bool) -> AnyElement {
    div()
        .id(SharedString::from(format!("matrix_scene_{}", scene_idx)))
        .w(px(110.0))
        .h(px(28.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(3.0))
        .bg(rgb(0x1E1E28))
        .border_1()
        .border_color(rgb(0x0096BE))
        .cursor_pointer()
        // Hover: borde encendido para feedback visual.
        .hover(|this| this.border_color(rgb(0x4CD6FF)).bg(rgb(0x2A2A3A)))
        // Click: dispara simultáneamente todos los clips activos de la fila.
        .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| {
            let st = state(cx);
            st.update(cx, |s, cx| {
                let proxy = s.audio_proxy.clone();
                trigger_scene(&mut s.matrix_state, &proxy, scene_idx);
                cx.notify();
            });
        })
        .child(
            Label::new(format!("▶ {}", name))
                .text_xs()
                .text_color(if has_any_clip {
                    rgb(0xFFFFFF)
                } else {
                    rgb(0x808080)
                }),
        )
        .into_any_element()
}

fn db_text(volume: f32) -> String {
    let db_val = if volume <= 0.0 {
        -60.0
    } else if volume <= 0.75 {
        -60.0 + (volume / 0.75) * 60.0
    } else {
        ((volume - 0.75) / 0.25) * 6.0
    };
    format!("{:.1}dB", db_val)
}

pub fn render(cx: &mut Context<HikaruApp>) -> AnyElement {
    let app = state(cx).read(cx);
    // Snapshot mínimo para no mantener el borrow durante el armado de la UI.
    let tracks_meta: Vec<(String, bool, bool, f32, f32)> = app
        .matrix_state
        .tracks
        .iter()
        .map(|t| (t.name.clone(), t.muted, t.soloed, t.volume, t.pan))
        .collect();
    let scene_names: Vec<String> = app
        .matrix_state
        .scenes
        .iter()
        .map(|s| s.name.clone())
        .collect();
    let slots: Vec<Vec<(SlotState, Option<String>)>> = app
        .matrix_state
        .grid
        .iter()
        .map(|row| {
            row.iter()
                .map(|slot| (slot.state.clone(), slot.clip.as_ref().map(|c| c.name.clone())))
                .collect()
        })
        .collect();
    let selected = app.matrix_state.selected_slot;
    let show_editor = app.matrix_state.show_editor;
    let editor_height = app.matrix_state.editor_height;
    let clipboard_ready = app.matrix_clipboard.has_content();
    let drop_sample_ready = app.dragged_sample.is_some();
    let engine_handle = app.engine_handle.clone();
    drop(app);

    let tracks_len = tracks_meta.len();
    let scenes_len = scene_names.len();

    let mut track_rows: Vec<AnyElement> = Vec::new();
    for track_idx in 0..tracks_len {
        let (track_name, track_muted, track_soloed, track_volume, _track_pan) =
            tracks_meta[track_idx].clone();
        let mut scene_cells: Vec<AnyElement> = Vec::new();

        for scene_idx in 0..scenes_len {
            let (slot_state, clip_name_opt) = slots
                .get(track_idx)
                .and_then(|r| r.get(scene_idx))
                .cloned()
                .unwrap_or((SlotState::Empty, None));
            let clip_name = clip_name_opt.unwrap_or_default();
            let has_clip = !clip_name.is_empty();
            let is_selected = selected == Some((track_idx, scene_idx));

            scene_cells.push(render_pad(
                track_idx,
                scene_idx,
                slot_state,
                clip_name,
                has_clip,
                is_selected,
                drop_sample_ready,
                clipboard_ready,
                engine_handle.clone(),
            ));
        }

        track_rows.push(
            h_flex()
                .gap(px(4.0))
                .child(
                    v_flex()
                        .w(px(126.0))
                        .bg(rgb(0x1C1C20))
                        .border_1()
                        .border_color(rgb(0x2D2D37))
                        .rounded(px(4.0))
                        .p(px(4.0))
                        .gap(px(2.0))
                        .child(
                            h_flex()
                                .items_center()
                                .justify_between()
                                .child(Label::new(track_name.clone()).text_xs().text_color(rgb(0xE0E0E0)))
                                .child(
                                    h_flex()
                                        .gap(px(2.0))
                                        .child(
                                            Button::new(format!("track_solo_{}", track_idx)).rounded(gpui_kit::component::button::ButtonRounded::None)
                                                .label("S")
                                                .compact()
                                                .bg(rgb(0x3D3D3D))
                                                .text_color(rgb(0xE0E0E0))
                                                .when(track_soloed, |b| b.text_color(rgb(0xFFC800)))
                                                .on_click(move |_, _, cx| {
                                                    let st = state(cx);
                                                    st.update(cx, |state, cx| {
                                                        state.matrix_state.tracks[track_idx].soloed =
                                                            !state.matrix_state.tracks[track_idx].soloed;
                                                        state.audio_proxy.send(GuiCommand::SetTrackSolo {
                                                            track_idx,
                                                            solo: state.matrix_state.tracks[track_idx].soloed,
                                                        });
                                                        cx.notify();
                                                    });
                                                }),
                                        )
                                        .child(
                                            Button::new(format!("track_mute_{}", track_idx)).rounded(gpui_kit::component::button::ButtonRounded::None)
                                                .label("M")
                                                .compact()
                                                .bg(rgb(0x3D3D3D))
                                                .text_color(rgb(0xE0E0E0))
                                                .when(track_muted, |b| b.text_color(rgb(0xFF5050)))
                                                .on_click(move |_, _, cx| {
                                                    let st = state(cx);
                                                    st.update(cx, |state, cx| {
                                                        state.matrix_state.tracks[track_idx].muted =
                                                            !state.matrix_state.tracks[track_idx].muted;
                                                        state.audio_proxy.send(GuiCommand::SetTrackMute {
                                                            track_idx,
                                                            mute: state.matrix_state.tracks[track_idx].muted,
                                                        });
                                                        cx.notify();
                                                    });
                                                }),
                                        ),
                                ),
                        )
                        .child(Label::new(db_text(track_volume)).text_xs().text_color(rgb(0xE0E0E0))),
                )
                .children(scene_cells)
                .into_any_element(),
        );
    }

    let mut scene_headers: Vec<AnyElement> = Vec::new();
    for scene_idx in 0..scenes_len {
        let name = scene_names[scene_idx].clone();
        // ¿Hay al menos un clip en esta escena? Atenúa el header si está vacía.
        let has_any_clip = slots
            .iter()
            .any(|row| row.get(scene_idx).map(|(_, n)| n.is_some()).unwrap_or(false));
        scene_headers.push(render_scene_launcher(scene_idx, name, has_any_clip));
    }

    v_flex()
        .id("session_matrix")
        .size_full()
        .bg(rgb(0x1E1E1E))
        .gap(px(4.0))
        .child(
            h_flex()
                .items_center()
                .gap(px(8.0))
                .px(px(8.0))
                .py(px(4.0))
                .child(Label::new("SESSION MATRIX").text_sm().font_weight(FontWeight::BOLD).text_color(rgb(0xE0E0E0)))
                .child(
                    Button::new("matrix_add_track").rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("➕")
                        .compact()
                        .bg(rgb(0x3D3D3D))
                        .text_color(rgb(0xE0E0E0))
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            st.update(cx, |state, cx| {
                                state.matrix_state.add_track();
                                cx.notify();
                            });
                        }),
                )
                .child(
                    Button::new("matrix_remove_track").rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("➖")
                        .compact()
                        .bg(rgb(0x3D3D3D))
                        .text_color(rgb(0xE0E0E0))
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            st.update(cx, |state, cx| {
                                state.matrix_state.remove_track();
                                cx.notify();
                            });
                        }),
                )
                .child(
                    Button::new("matrix_add_scene").rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("➕ Scene")
                        .compact()
                        .bg(rgb(0x3D3D3D))
                        .text_color(rgb(0xE0E0E0))
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            st.update(cx, |state, cx| {
                                state.matrix_state.add_scene();
                                cx.notify();
                            });
                        }),
                )
                .child(
                    Button::new("matrix_remove_scene").rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("➖ Scene")
                        .compact()
                        .bg(rgb(0x3D3D3D))
                        .text_color(rgb(0xE0E0E0))
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            st.update(cx, |state, cx| {
                                state.matrix_state.remove_scene();
                                cx.notify();
                            });
                        }),
                ),
        )
        .child(
            h_flex()
                .gap(px(4.0))
                .px(px(8.0))
                .child(div().w(px(118.0)))
                .children(scene_headers),
        )
        .child(
            v_flex()
                .overflow_y_scrollbar()
                .gap(px(4.0))
                .children(track_rows),
        )
        .when(show_editor, |this| {
            // Barra del Clip Editor: muestra explícitamente el slot actual.
            let hint = match selected {
                Some((t, s)) => {
                    let clip_label = slots
                        .get(t)
                        .and_then(|r| r.get(s))
                        .and_then(|(_, n)| n.clone())
                        .unwrap_or_default();
                    if clip_label.is_empty() {
                        format!("CLIP EDITOR — Track {} | Scene {}", t + 1, s + 1)
                    } else {
                        format!(
                            "CLIP EDITOR — Track {} | Scene {} — {}",
                            t + 1,
                            s + 1,
                            clip_label
                        )
                    }
                }
                None => "CLIP EDITOR".to_string(),
            };
            this.child(
                div()
                    .h(px(editor_height))
                    .bg(rgb(0x141418))
                    .border_1()
                    .border_color(rgb(0x2D3741))
                    .rounded(px(4.0))
                    .p(px(6.0))
                    .child(Label::new(hint).text_xs().text_color(rgb(0x9AA4B2))),
            )
        })
        .into_any_element()
}

fn pitch_to_note_name(pitch: u8) -> &'static str {
    let names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    names[(pitch % 12) as usize]
}
