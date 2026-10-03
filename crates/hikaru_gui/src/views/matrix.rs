// Copyright (C) Hikaru Corporation - 2026
// Hikaru OpenLive - Session Matrix with GPUI Kit Interface
// Bitwig's Clip Launcher-like made full in Rust.
// GNU Affero General Public License v3
// crates/hikaru_gui/src/views/matrix.rs

use std::path::PathBuf;
use std::cell::Cell;
use std::rc::Rc;

use hikaru_audio_engine::AudioEngine;

use crate::audio_proxy::{AudioProxy, GuiCommand};
pub use crate::views::clipboard::MatrixClipboard;
use crate::views::explorer::ExplorerAudioDrag;
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
    /// Picos normalizados 0..=1 para la mini waveform del pad.
    ///
    /// Se calcula una sola vez al cargar el archivo (leyendo el WAV por
    /// streaming, sin decodificarlo entero) y el render solo lo copia para
    /// pintarlo: recalcular la forma de onda en cada frame costaría una
    /// pasada completa sobre el audio por pad y por frame.
    pub peaks: Vec<f32>,
}

impl MatrixClip {
    ///_bins de la mini waveform del pad. A 110px de ancho basta uno cada dos
    /// píxeles: más bins no se distinguen y solo multiplican el path.
    pub const PEAK_BINS: usize = 56;
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
                // Reset del playhead local: el engine rearranca además la
                // fase de la voz desde la muestra 0 (`TriggerScene`), así
                // que el avance visual vuelve a 0% de inmediato.
                clip.local_bar = 1.0;
            }
            sync_midi_clip(state, audio_proxy, track_idx, scene_idx);
        }
        // Pistas sin clip en esta escena: se dejan en su estado actual.
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

    // Picos para la mini waveform del pad: se calculan una sola vez al cargar
    // el archivo, no en cada frame de pintado.
    let peaks = if is_midi {
        Vec::new()
    } else {
        crate::views::open_dms_sampler::load_peaks_from_wav(
            path_str.as_str(),
            MatrixClip::PEAK_BINS,
        )
    };

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
            peaks,
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
        // La waveform del pad refleja el clip ya extendido con el sample nuevo.
        clip.peaks = crate::views::open_dms_sampler::load_peaks_from_wav(
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
// Registro para el harness de tests headless (`tests/matrix_header_click`):
// sin la feature `test-support` es identidad y no cambia nada en producción.
use gpui_kit::TestSupportExt as _;
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
        // En reproducción: fondo oscuro verdoso sutil para que el nombre del
        // clip siga legible; el progreso lo marca el playhead del canvas.
        SlotState::Playing => (rgb(0x14301F).into(), rgb(0x4CFF8A).into()),
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

fn slot_display_text(_slot_state: &SlotState, has_clip: bool, clip_name: &str) -> String {
    if !has_clip {
        // Celda vacía: pad oscuro limpio, sin texto genérico.
        return String::new();
    }
    // Nombre limpio, sin prefijos de estado (▶/■/…): el mini-botón de la
    // esquina es el indicador de estado único y el label se trunca con
    // ellipsis en el layout (una sola línea pequeña).
    clip_name.to_string()
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
    peaks: Vec<f32>,
) -> AnyElement {
    let (bg, border) = slot_colors(&slot_state, has_clip, is_selected);
    let hover_border = slot_hover_border(&slot_state, has_clip);
    let display_text = slot_display_text(&slot_state, has_clip, &clip_name);
    let glyph = play_glyph(&slot_state, has_clip);
    let is_playing = slot_state == SlotState::Playing;
    let is_empty = !has_clip;

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
        // Capas 2 y 3 (contenedor oscuro + picos) en un solo canvas absoluto:
        // el contenedor se pinta primero como lienzo de la waveform y los picos
        // encima, ambos por debajo del playhead (capa 4) y de la cabecera con
        // el nombre y el botón (capa 5), que son hijos posteriores del pad.
        .when(!peaks.is_empty(), |d| {
            d.child(canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let left = bounds.origin.x.as_f32();
                    let width = bounds.size.width.as_f32().max(1.0);
                    let height = bounds.size.height.as_f32().max(1.0);
                    // Margen interno: ni el contenedor ni la silueta tocan los
                    // bordes del pad, así el marco y el nombre siguen libres.
                    let pad_x = 4.0_f32;
                    let pad_y = 6.0_f32;
                    let inner_w = (width - pad_x * 2.0).max(1.0);
                    let inner_h = (height - pad_y * 2.0).max(1.0);
                    let inner_y = bounds.origin.y.as_f32() + pad_y;

                    // Capa 2: lienzo oscuro del clip. En Playing lleva un tinte
                    // verde muy sutil para diferenciar el estado sin perder el
                    // contraste de los picos.
                    let container_color = if is_playing {
                        rgb(0x0E1F16)
                    } else {
                        rgb(0x121418)
                    };
                    window.paint_quad(PaintQuad {
                        bounds: Bounds::new(
                            point(px(left + pad_x), px(inner_y)),
                            size(px(inner_w), px(inner_h)),
                        ),
                        background: container_color.into(),
                        border_color: Hsla::default(),
                        corner_radii: gpui_kit::Corners::all(px(2.0)),
                        border_widths: gpui_kit::Edges::default(),
                        border_style: BorderStyle::default(),
                    });

                    // Capa 3: picos. Tonos fríos al detener, verde claro en
                    // reproducción; ambos al ~50% para no competir con el texto.
                    let peak_color: Hsla = if is_playing {
                        rgba(0x8CFFC7).into()
                    } else {
                        rgba(0x7FA8D8).into()
                    };
                    let middle = bounds.origin.y.as_f32() + height / 2.0;
                    // Un path por mitad: silueta simétrica legible con menos
                    // segmentos que una barra por bin.
                    for mirror in [false, true] {
                        let mut path = PathBuilder::stroke(px(1.0));
                        for (index, peak) in peaks.iter().enumerate() {
                            let x = left + pad_x + inner_w * index as f32 / peaks.len() as f32;
                            let amplitude = inner_h * 0.5 * peak.clamp(0.0, 1.0);
                            let y = if mirror {
                                middle + amplitude
                            } else {
                                middle - amplitude
                            };
                            if index == 0 {
                                path.move_to(point(px(x), px(y)));
                            } else {
                                path.line_to(point(px(x), px(y)));
                            }
                        }
                        if let Ok(path) = path.build() {
                            window.paint_path(path, peak_color);
                        }
                    }
                },
            )
            .absolute()
            .inset_0())
        })
        // Drop target del Drag & Drop desde el Explorer: acepta solo el
        // payload tipado `ExplorerAudioDrag` y carga el clip en este slot.
        .can_drop(|payload: &dyn std::any::Any, _, _| {
            payload.is::<ExplorerAudioDrag>()
        })
        .on_drop(move |payload: &ExplorerAudioDrag, _, cx| {
            let path = payload.0.clone();
            let st = state(cx);
            st.update(cx, |s, cx| {
                // El gesto nativo ya trae el path; limpiar el flag legacy
                // para apagar el resaltado y el fantasma flotante.
                s.dragged_sample = None;
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
            });
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
        // Playhead discreto en Playing: línea vertical fina + barra inferior
        // de progreso. Solo pinta dos primitivas pequeñas cuyas coordenadas X
        // derivan del progreso (sin tocar el layout: el canvas es un overlay
        // absoluto y el fondo del pad permanece oscuro para no tapar el texto).
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    if !is_playing {
                        return;
                    }
                    // Lectura no bloqueante: si el motor está ocupado este
                    // frame, se omite el pintado en vez de frenar la UI.
                    let Some(ref handle) = engine_handle else {
                        return;
                    };
                    let Ok(engine) = handle.try_lock() else {
                        return;
                    };
                    let Some((frame, total)) =
                        engine.voice_playhead_frame(track_idx, scene_idx)
                    else {
                        return;
                    };
                    if total == 0 {
                        return;
                    }
                    let progress = (frame as f32 / total as f32).clamp(0.0, 1.0);
                    let origin = bounds.origin;
                    let width: f32 = bounds.size.width.into();
                    let height: f32 = bounds.size.height.into();
                    // Opción B: barra horizontal fina (3px) clavada abajo,
                    // rellenada de izquierda a derecha según el progreso.
                    let bar_h = 3.0_f32;
                    let fill_w = width * progress;
                    if fill_w > 0.5 {
                        window.paint_quad(PaintQuad {
                            bounds: Bounds::new(
                                point(origin.x, origin.y + px(height - bar_h)),
                                size(px(fill_w), px(bar_h)),
                            ),
                            background: rgb(0x4CFF8A).into(),
                            border_color: Hsla::default(),
                            corner_radii: gpui_kit::Corners::default(),
                            border_widths: gpui_kit::Edges::default(),
                            border_style: BorderStyle::default(),
                        });
                    }
                    // Opción A: línea vertical delgada (2px) que avanza con el
                    // progreso, en blanco/verde claro.
                    let x = origin.x + px(width * progress);
                    let mut path = PathBuilder::stroke(px(2.0));
                    path.move_to(point(x, origin.y));
                    path.line_to(point(x, origin.y + px(height)));
                    if let Ok(p) = path.build() {
                        window.paint_path(p, rgb(0xB6FFD2));
                    }
                },
            )
            .absolute()
            .inset_0(),
        )
        // Cabecera del pad: mini-botón Play/Stop + nombre del clip en una
        // sola fila (padding 4px arriba/izquierda, alineados con items_center).
        // El botón es el indicador de estado único (▶/■/… + color); el nombre
        // va en una sola línea pequeña truncada con ellipsis (`Cymatics - …`)
        // y el resto de la superficie queda limpia para progreso y selección.
        .child(
            div()
                .absolute()
                .top(px(4.0))
                .left(px(4.0))
                .right(px(4.0))
                .h(px(20.0))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(4.0))
                .overflow_hidden()
                .child(
                    div()
                        .w(px(20.0))
                        .h(px(20.0))
                        .flex_shrink_0()
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
                .when(has_clip, move |this| {
                    this.child(
                        Label::new(display_text.clone())
                            .text_size(px(9.0))
                            .text_color(rgb(0xFFFFFF))
                            .flex_1()
                            .truncate(),
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

fn pan_text(pan: f32) -> String {
    let v = (pan * 100.0).round() as i32;
    if v == 0 {
        "C".to_string()
    } else if v < 0 {
        format!("L{}", v.abs())
    } else {
        format!("R{}", v)
    }
}

// =========================================================================
// CONTROLES DE MEZCLA DEL TRACK HEADER (Session Matrix)
// =========================================================================
//
// Cada cabecera es una columna compacta:
//
//   | Track 1                     [S] [M] |
//   | Vol: [|======------] -3.2dB         |
//   | Pan: [---o---] C                    |
//
// Los sliders son horizontales con drag en tiempo real y doble-clic para
// resetear (volumen → 0.0dB = 0.75 lineal, pan → centro). Escriben en la
// matriz y espejan a `live_tracks` + motor, igual que los faders del Arranger.

/// Ancho total de la cabecera (ver `HEADER_SPACER_W` para el espejo).
const HEADER_WIDTH: f32 = 196.0;
/// Ancho del espaciador sobre la columna de cabeceras (= ancho − padding raíz).
const HEADER_SPACER_W: f32 = 188.0;
/// Ancho fijo del riel de volumen: el thumb se posiciona en px deterministas.
const MIX_SLIDER_W: f32 = 72.0;
const MIX_SLIDER_H: f32 = 16.0;
const MIX_THUMB_W: f32 = 10.0;
/// Padding vertical extra alrededor del slider: la hitbox queda en 22px de
/// alto para que el drag horizontal no se corte por 1px arriba o abajo.
const MIX_SLIDER_PAD_Y: f32 = 3.0;

/// 0.0dB en la curva de `db_text` (lineal 0..=1 con el 0.75 como 0 dB).
const VOLUME_RESET: f32 = 0.75;

fn set_matrix_volume(cx: &mut App, track_idx: usize, volume: f32) {
    let volume = volume.clamp(0.0, 1.0);
    let st = state(cx);
    st.update(cx, |s, cx| {
        if let Some(t) = s.matrix_state.tracks.get_mut(track_idx) {
            t.volume = volume;
        }
        if let Some(live) = s.live_tracks.get_mut(track_idx + 1) {
            live.volume = volume;
        }
        s.audio_proxy.send(GuiCommand::SetTrackVolume {
            track_idx,
            volume_db: volume,
        });
        cx.notify();
    });
}

fn set_matrix_pan(cx: &mut App, track_idx: usize, pan: f32) {
    let pan = pan.clamp(-1.0, 1.0);
    let st = state(cx);
    st.update(cx, |s, cx| {
        if let Some(t) = s.matrix_state.tracks.get_mut(track_idx) {
            t.pan = pan;
        }
        if let Some(live) = s.live_tracks.get_mut(track_idx + 1) {
            live.pan = pan;
        }
        s.audio_proxy.send(GuiCommand::SetTrackPan { track_idx, pan });
        cx.notify();
    });
}

/// Slider horizontal compacto para el volumen del header.
///
/// `norm` es el volumen lineal 0..=1. Click salta al punto, drag horizontal
/// (`delta_x`) ajusta en tiempo real y doble-clic llama `on_reset`.
///
/// La hitbox es un wrapper con padding vertical extra (22px de alto total):
/// los eventos viven afuera del dibujo para que el hover/click no se corte
/// en los bordes del riel. El canvas registra los bounds del dibujo interno,
/// así que el padding no desplaza el mapeo puntero→valor.
fn h_mix_slider(
    id: String,
    norm: f32,
    on_change: impl Fn(f32, &mut App) + 'static,
    on_reset: impl Fn(&mut App) + 'static,
) -> AnyElement {
    let norm = norm.clamp(0.0, 1.0);
    // El canvas invisible registra los bounds para traducir el puntero a
    // valor; el drag vive en un flag local (mismo patrón que los faders del
    // Arranger, pero sin estado global: el header es efímero por frame).
    let bounds_slot: Rc<Cell<[f32; 4]>> = Rc::new(Cell::new([0.0; 4]));
    let dragging: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let b_down = bounds_slot.clone();
    let b_move = bounds_slot.clone();
    let b_paint = bounds_slot.clone();
    let d_down = dragging.clone();
    let d_move = dragging.clone();
    let d_up = dragging.clone();

    let norm_from_x = move |b: [f32; 4], x: f32| {
        let travel = (b[2] - MIX_THUMB_W).max(1.0);
        ((x - b[0] - MIX_THUMB_W / 2.0) / travel).clamp(0.0, 1.0)
    };
    let norm_down = norm_from_x;
    let norm_move = norm_from_x;
    // `Fn` no es `Copy`: se comparte por `Rc` entre los handlers de
    // mouse-down (salto + inicio de drag) y mouse-move (drag).
    let on_change: Rc<dyn Fn(f32, &mut App)> = Rc::new(on_change);
    let on_down = on_change.clone();
    let on_move = on_change.clone();

    let thumb_x = MIX_THUMB_W / 2.0 + norm * (MIX_SLIDER_W - MIX_THUMB_W);

    div()
        .id(SharedString::from(id))
        .test_support()
        .flex()
        .items_center()
        .py(px(MIX_SLIDER_PAD_Y))
        .flex_shrink_0()
        .rounded(px(3.0))
        .cursor_pointer()
        .hover(|this| this.bg(rgb(0x232329)))
        .on_mouse_down(gpui_kit::MouseButton::Left, move |event, _, cx| {
            if event.click_count >= 2 {
                d_down.set(false);
                on_reset(cx);
                return;
            }
            d_down.set(true);
            let v = norm_down(b_down.get(), event.position.x.as_f32());
            on_down(v, cx);
        })
        .on_mouse_move(move |event, _, cx| {
            if !d_move.get() {
                return;
            }
            let v = norm_move(b_move.get(), event.position.x.as_f32());
            on_move(v, cx);
        })
        .on_mouse_up(gpui_kit::MouseButton::Left, move |_, _, _| {
            d_up.set(false);
        })
        .child(
            div()
                .relative()
                .w(px(MIX_SLIDER_W))
                .h(px(MIX_SLIDER_H))
                .flex_shrink_0()
                .child(
                    canvas(
                        move |bounds, _, _| {
                            b_paint.set([
                                bounds.origin.x.as_f32(),
                                bounds.origin.y.as_f32(),
                                bounds.size.width.as_f32(),
                                bounds.size.height.as_f32(),
                            ]);
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
                // Riel
                .child(
                    div()
                        .absolute()
                        .left(px(0.0))
                        .top(px((MIX_SLIDER_H - 4.0) / 2.0))
                        .w(px(MIX_SLIDER_W))
                        .h(px(4.0))
                        .bg(rgb(0x2D2D2D))
                        .rounded(px(1.0)),
                )
                // Relleno desde la izquierda
                .child(
                    div()
                        .absolute()
                        .left(px(0.0))
                        .top(px((MIX_SLIDER_H - 4.0) / 2.0))
                        .w(px(thumb_x.max(1.0)))
                        .h(px(4.0))
                        .bg(rgb(0x0096BE)),
                )
                // Thumb
                .child(
                    div()
                        .absolute()
                        .left(px(thumb_x - MIX_THUMB_W / 2.0))
                        .top(px(1.0))
                        .w(px(MIX_THUMB_W))
                        .h(px(MIX_SLIDER_H - 2.0))
                        .bg(rgb(0x00A2E8))
                        .border_1()
                        .border_color(rgb(0x000000)),
                ),
        )
        .into_any_element()
}

/// Diámetro del knob de pan, en píxeles.
const PAN_KNOB_SIZE: f32 = 28.0;
/// Padding alrededor del knob: la hitbox queda en 38×38 para que el agarre
/// no exija puntería de 1px.
const PAN_KNOB_PAD: f32 = 5.0;
/// Píxeles de drag vertical para recorrer el paneo entero (L→R).
const PAN_KNOB_TRAVEL: f32 = 150.0;

/// Knob rotativo de pan para el header (`Pan: ( O ) C`).
///
/// Disco + aguja pintados en canvas (misma técnica que el dial de
/// `controls.rs`): el ángulo va de −135° (L) a +135° (R) medidos desde las
/// 12, con el centro arriba.
///
/// Interacción (las tres dan feedback inmediato en la aguja y la lectura):
/// - Clic: salta al ángulo apuntado (0° = arriba/C, ±135° = extremos).
/// - Drag vertical desde ahí: arriba → R, abajo → L (relativo al punto de
///   agarre, recorrido completo en `PAN_KNOB_TRAVEL` px).
/// - Doble-clic: vuelve exacto al centro.
fn pan_knob(track_idx: usize, pan: f32) -> AnyElement {
    let pan = if pan.is_finite() {
        pan.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    // Bounds del knob para traducir el clic a ángulo. Los registra el canvas
    // en pre-paint (igual que los sliders de volumen).
    let bounds_slot: Rc<Cell<[f32; 4]>> = Rc::new(Cell::new([0.0; 4]));
    let b_down = bounds_slot.clone();
    let b_paint = bounds_slot.clone();
    // Estado del gesto: `[y_inicial, pan_inicial]`. Local al widget porque el
    // header se reconstruye en cada frame (igual que los sliders de volumen).
    let gesture: Rc<Cell<Option<[f32; 2]>>> = Rc::new(Cell::new(None));
    let g_down = gesture.clone();
    let g_move = gesture.clone();
    let g_up = gesture.clone();

    div()
        .id(SharedString::from(format!("matrix_panknob_{}", track_idx)))
        .test_support()
        .flex()
        .items_center()
        .justify_center()
        .p(px(PAN_KNOB_PAD))
        .flex_shrink_0()
        .rounded(px(19.0))
        .cursor_pointer()
        .hover(|this| this.bg(rgb(0x232329)))
        .on_mouse_down(gpui_kit::MouseButton::Left, move |event, _, cx| {
            if event.click_count >= 2 {
                g_down.set(None);
                set_matrix_pan(cx, track_idx, 0.0);
                return;
            }
            // Salto al ángulo apuntado: 0° arriba, +horario hacia R. Fuera
            // del arco (±135°) se acota al extremo más cercano.
            let y = event.position.y.as_f32();
            let b = b_down.get();
            let dx = event.position.x.as_f32() - (b[0] + b[2] / 2.0);
            let dy = y - (b[1] + b[3] / 2.0);
            let jumped = (dx.atan2(-dy).to_degrees() / 135.0).clamp(-1.0, 1.0);
            set_matrix_pan(cx, track_idx, jumped);
            // El drag continúa en relativo desde el punto de agarre.
            g_down.set(Some([y, jumped]));
        })
        .on_mouse_move(move |event, _, cx| {
            let Some([y0, p0]) = g_move.get() else {
                return;
            };
            let v = p0 + (y0 - event.position.y.as_f32()) / PAN_KNOB_TRAVEL;
            set_matrix_pan(cx, track_idx, v);
        })
        .on_mouse_up(gpui_kit::MouseButton::Left, move |_, _, _| {
            g_up.set(None);
        })
        .child(
            div()
                .w(px(PAN_KNOB_SIZE))
                .h(px(PAN_KNOB_SIZE))
                .flex_shrink_0()
                .child(
                    canvas(
                move |bounds, _, _| {
                    b_paint.set([
                        bounds.origin.x.as_f32(),
                        bounds.origin.y.as_f32(),
                        bounds.size.width.as_f32(),
                        bounds.size.height.as_f32(),
                    ]);
                },
                move |bounds, _, window, _| {
                    let side = bounds
                        .size
                        .width
                        .as_f32()
                        .min(bounds.size.height.as_f32());
                    let cx0 = bounds.origin.x.as_f32() + bounds.size.width.as_f32() / 2.0;
                    let cy0 = bounds.origin.y.as_f32() + bounds.size.height.as_f32() / 2.0;
                    let radius = side / 2.0 - 1.0;
                    // Disco: quad con los cuatro radios a la mitad del lado.
                    window.paint_quad(PaintQuad {
                        bounds: Bounds::new(
                            point(px(cx0 - radius), px(cy0 - radius)),
                            size(px(radius * 2.0), px(radius * 2.0)),
                        ),
                        background: rgb(0x232329).into(),
                        border_color: rgb(0x3D3D3D).into(),
                        corner_radii: gpui_kit::Corners {
                            top_left: px(radius),
                            top_right: px(radius),
                            bottom_right: px(radius),
                            bottom_left: px(radius),
                        },
                        border_widths: gpui_kit::Edges {
                            top: px(1.0),
                            right: px(1.0),
                            bottom: px(1.0),
                            left: px(1.0),
                        },
                        border_style: BorderStyle::default(),
                    });
                    // Aguja: −135° (L) .. +135° (R) desde las 12 en punto.
                    let angle = pan * 135.0 * std::f32::consts::PI / 180.0;
                    let len = (radius - 4.0).max(0.0);
                    let mut needle = PathBuilder::stroke(px(2.0));
                    needle.move_to(point(px(cx0), px(cy0)));
                    needle.line_to(point(
                        px(cx0 + len * angle.sin()),
                        px(cy0 - len * angle.cos()),
                    ));
                    if let Ok(needle) = needle.build() {
                        window.paint_path(needle, rgb(0x00A2E8));
                    }
                    // Punto central.
                    window.paint_quad(PaintQuad {
                        bounds: Bounds::new(
                            point(px(cx0 - 2.0), px(cy0 - 2.0)),
                            size(px(4.0), px(4.0)),
                        ),
                        background: rgb(0x808080).into(),
                        border_color: Hsla::default(),
                        corner_radii: gpui_kit::Corners {
                            top_left: px(2.0),
                            top_right: px(2.0),
                            bottom_right: px(2.0),
                            bottom_left: px(2.0),
                        },
                        border_widths: gpui_kit::Edges::default(),
                        border_style: BorderStyle::default(),
                    });
                },
            )
            .w(px(PAN_KNOB_SIZE))
            .h(px(PAN_KNOB_SIZE)),
                )
        )
        .into_any_element()
}

/// Cabecera de pista con mezcla integrada (layout legacy de una sola fila):
///
///   Fila superior: nombre (ellipsis) + [M] [S].
///   Fila de mezcla: knob de pan + lectura C/Lxx/Rxx + fader de volumen + dB.
///
/// ```text
/// +-------------------------------------+
/// | Track 1                     [M] [S] |
/// | (O) C   [|======------] -3.2dB      |
/// +-------------------------------------+
/// ```
fn track_header(
    track_idx: usize,
    name: String,
    muted: bool,
    soloed: bool,
    volume: f32,
    pan: f32,
) -> AnyElement {
    v_flex()
        .w(px(HEADER_WIDTH))
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
                .gap(px(4.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .child(
                            Label::new(name)
                                .text_xs()
                                .text_color(rgb(0xE0E0E0))
                                .text_ellipsis(),
                        ),
                )
                .child(
                    h_flex()
                        .gap(px(2.0))
                        .flex_shrink_0()
                        .child(
                            Button::new(format!("track_mute_{}", track_idx)).rounded(gpui_kit::component::button::ButtonRounded::None)
                                .label("M")
                                .compact()
                                .bg(rgb(0x3D3D3D))
                                .text_color(rgb(0xE0E0E0))
                                .when(muted, |b| b.text_color(rgb(0xFF5050)))
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
                        )
                        .child(
                            Button::new(format!("track_solo_{}", track_idx)).rounded(gpui_kit::component::button::ButtonRounded::None)
                                .label("S")
                                .compact()
                                .bg(rgb(0x3D3D3D))
                                .text_color(rgb(0xE0E0E0))
                                .when(soloed, |b| b.text_color(rgb(0xFFC800)))
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
                        ),
                ),
        )
        // Fila única de mezcla (layout legacy):
        //   [Knob Pan] [Pan Text] | [Vol Slider] [Vol dB Text]
        .child(
            h_flex()
                .items_center()
                .gap(px(4.0))
                .child(pan_knob(track_idx, pan))
                .child(
                    Label::new(pan_text(pan))
                        .text_size(px(9.0))
                        .text_color(rgb(0xE0E0E0))
                        .w(px(24.0)),
                )
                .child(h_mix_slider(
                    format!("matrix_vol_{}", track_idx),
                    volume,
                    move |v, cx| set_matrix_volume(cx, track_idx, v),
                    move |cx| set_matrix_volume(cx, track_idx, VOLUME_RESET),
                ))
                .child(
                    Label::new(db_text(volume))
                        .text_size(px(9.0))
                        .text_color(rgb(0xE0E0E0))
                        .w(px(42.0)),
                ),
        )
        .into_any_element()
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
    // El snapshot incluye los picos ya cacheados del clip: el render solo
    // copia el `Vec` para pintarlo, nunca recalcula la forma de onda.
    let slots: Vec<Vec<(SlotState, Option<String>, Vec<f32>)>> = app
        .matrix_state
        .grid
        .iter()
        .map(|row| {
            row.iter()
                .map(|slot| {
                    (
                        slot.state.clone(),
                        slot.clip.as_ref().map(|c| c.name.clone()),
                        slot.clip.as_ref().map(|c| c.peaks.clone()).unwrap_or_default(),
                    )
                })
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
        let (track_name, track_muted, track_soloed, track_volume, track_pan) =
            tracks_meta[track_idx].clone();
        let mut scene_cells: Vec<AnyElement> = Vec::new();

        for scene_idx in 0..scenes_len {
            let (slot_state, clip_name_opt, peaks) = slots
                .get(track_idx)
                .and_then(|r| r.get(scene_idx))
                .cloned()
                .unwrap_or((SlotState::Empty, None, Vec::new()));
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
                peaks,
            ));
        }

        track_rows.push(
            h_flex()
                .gap(px(4.0))
                .child(track_header(
                    track_idx,
                    track_name.clone(),
                    track_muted,
                    track_soloed,
                    track_volume,
                    track_pan,
                ))
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
            .any(|row| row.get(scene_idx).map(|(_, n, _)| n.is_some()).unwrap_or(false));
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
                .child(div().w(px(HEADER_SPACER_W)))
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
                        .and_then(|(_, n, _)| n.clone())
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
