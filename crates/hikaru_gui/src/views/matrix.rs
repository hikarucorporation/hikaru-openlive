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
            // Altura inicial del panel del Clip Editor: 104px para que la
            // grilla default de 8×8 (filas de ~51px) quepa en 720p sin
            // scrollbar (el panel es solo un placeholder con el hint del
            // slot actual).
            editor_height: 104.0,
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
use crate::app::{state, AppState, HikaruApp};

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
        .test_support()
        .relative()
        .w(px(110.0))
        .h(px(PAD_HEIGHT))
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
                    let pad_y = 4.0_f32;
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
                // El Clip Editor sigue a la selección de la matriz: seleccionar
                // un pad lo carga acá (es el binding bidireccional pedido).
                crate::views::clip_editor::select_target(
                    s,
                    Some(crate::views::clip_editor::ClipEditorTarget::Matrix {
                        track: track_idx,
                        scene: scene_idx,
                    }),
                );
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
                .top(px(3.0))
                .left(px(4.0))
                .right(px(4.0))
                .h(px(16.0))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(4.0))
                .overflow_hidden()
                .child(
                    div()
                        .w(px(16.0))
                        .h(px(16.0))
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
                                crate::views::clip_editor::select_target(
                                    s,
                                    Some(crate::views::clip_editor::ClipEditorTarget::Matrix {
                                        track: track_idx,
                                        scene: scene_idx,
                                    }),
                                );
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
                                .text_size(px(8.0))
                                .text_color(rgb(0xFFFFFF))
                                .into_any_element()
                        } else {
                            // Stop: cuadrado sólido limpio, al estilo del Arranger.
                            div().w(px(6.0)).h(px(6.0)).bg(rgb(0xFFFFFF)).into_any_element()
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
        .h(px(SCENE_HEADER_HEIGHT))
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

/// Texto de dB del volumen lineal (misma curva que el mixer): lo reutiliza el
/// header de la Playlist para que ambas vistas lean igual.
pub fn db_text(volume: f32) -> String {
    let db_val = if volume <= 0.0 {
        -60.0
    } else if volume <= 0.75 {
        -60.0 + (volume / 0.75) * 60.0
    } else {
        ((volume - 0.75) / 0.25) * 6.0
    };
    format!("{:.1}dB", db_val)
}

/// Texto de posición de paneo (`C` / `Lxx` / `Rxx`): compartido con la Playlist.
pub fn pan_text(pan: f32) -> String {
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
// GEOMETRÍA DEL KNOB DE PAN (helpers puros, testeados en tests/pan_geometry)
// =========================================================================

/// Detent central de display: por debajo de ±0.02 el paneo se muestra como
/// centro exacto.
///
/// Un drag o clic que termina en 0.01 deja la aguja a ~1.4° (apariencia de
/// "12:02" con la etiqueta todavía en valores mínimos): el detent unifica
/// aguja y etiqueta en `C` / vertical. Solo afecta al display; el valor de
/// audio no se toca.
pub const PAN_CENTER_DETENT: f32 = 0.02;

pub fn snap_center_pan(pan: f32) -> f32 {
    if pan.is_finite() && pan.abs() < PAN_CENTER_DETENT {
        0.0
    } else {
        pan
    }
}

/// Ángulo estándar del paneo, en radianes:
///
/// - `L100` (`-1.0`) → `-3/4 * PI`
/// - Centro (`0.0`) → `-1/2 * PI` (12 en punto exacto)
/// - `R100` (`1.0`) → `-1/4 * PI`
///
/// Es la parametrización del barrido visual de ±135° del knob: el ángulo de
/// pantalla (horario desde las 12) es `3 * (θ + PI/2)`, que en los extremos
/// da `∓3/4 * PI`. Con `pan == 0.0` el resultado es bit-exacto (`-PI/2`), así
/// que `sin == 0.0` y la punta de la aguja cae sobre `center_x` sin deriva.
pub fn pan_standard_angle(pan: f32) -> f32 {
    use std::f32::consts::PI;
    -PI / 2.0 + pan.clamp(-1.0, 1.0) * PI / 4.0
}

/// Ángulo de barrido en pantalla (horario desde las 12, radianes) para el pan
/// ya con detent aplicado. En `0.0` devuelve `0.0` bit-exacto.
pub fn pan_sweep_angle(snapped_pan: f32) -> f32 {
    use std::f32::consts::PI;
    3.0 * (pan_standard_angle(snapped_pan) + PI / 2.0)
}

/// Punta de la aguja para un centro y largo dados. Función pura para poder
/// testear que en el centro `tip_x == center_x` bit-exacto.
pub fn pan_needle_tip(snapped_pan: f32, center_x: f32, center_y: f32, len: f32) -> (f32, f32) {
    let a = pan_sweep_angle(snapped_pan);
    (center_x + len * a.sin(), center_y - len * a.cos())
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
/// Alto de cada pad de clip (ver presupuesto en `render`).
const PAD_HEIGHT: f32 = 44.0;
/// Alto de los launchers de escena: compacto y alineado con las celdas.
const SCENE_HEADER_HEIGHT: f32 = 24.0;
/// Ancho del espaciador sobre la columna de cabeceras (= ancho − padding raíz).
const HEADER_SPACER_W: f32 = 188.0;
/// Botones M/S compactos: altura y padding fijos (el `Button` del kit fija su
/// alto por tema y no baja de ~24px).
const MS_BTN_W: f32 = 22.0;
const MS_BTN_H: f32 = 18.0;
/// Ancho fijo del riel de volumen: el thumb se posiciona en px deterministas.
const MIX_SLIDER_W: f32 = 72.0;
const MIX_SLIDER_H: f32 = 12.0;
const MIX_THUMB_W: f32 = 8.0;
/// Padding vertical extra alrededor del slider: la hitbox queda en 20px de
/// alto para que el drag horizontal no se corte por 1px arriba o abajo.
const MIX_SLIDER_PAD_Y: f32 = 2.0;

/// 0.0dB en la curva de `db_text` (lineal 0..=1 con el 0.75 como 0 dB).
/// Valor del doble-clic en los sliders de volumen (matriz y playlist).
pub const VOLUME_RESET: f32 = 0.75;

/// Destino del drag de mezcla en curso en los headers.
///
/// Vive en `AppState` y no en un flag local del widget porque el header se
/// reconstruye en cada frame: el primer `notify` del drag mataba el flag
/// local y los `mouse_move` siguientes caían en closures nuevos con el flag
/// en `false` (solo sobrevivían los clics discretos).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatrixMixTarget {
    Volume(usize),
    Pan(usize),
}

/// Escritura efectiva de volumen (matriz + espejo `live_tracks` + motor).
/// Trabaja sobre `&mut AppState` para reutilizarla en el drag continuo sin
/// re-emitir `notify` por paso intermedio.
fn apply_matrix_volume(s: &mut AppState, track_idx: usize, volume: f32) {
    let volume = volume.clamp(0.0, 1.0);
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
}

/// Escritura efectiva de pan. Igual que el volumen: reutilizable en el drag.
fn apply_matrix_pan(s: &mut AppState, track_idx: usize, pan: f32) {
    let pan = pan.clamp(-1.0, 1.0);
    if let Some(t) = s.matrix_state.tracks.get_mut(track_idx) {
        t.pan = pan;
    }
    if let Some(live) = s.live_tracks.get_mut(track_idx + 1) {
        live.pan = pan;
    }
    s.audio_proxy.send(GuiCommand::SetTrackPan { track_idx, pan });
}

fn norm_from_slider_x(bounds: [f32; 4], x: f32) -> f32 {
    let travel = (bounds[2] - MIX_THUMB_W).max(1.0);
    ((x - bounds[0] - MIX_THUMB_W / 2.0) / travel).clamp(0.0, 1.0)
}

/// Inicia un gesto de slider de mezcla con un apply genérico.
///
/// Misma máquina que `start_vol_drag` pero con la escritura parametrizada:
/// la reutilizan los headers de la Playlist (mismo gesto global
/// `matrix_mix_drag`, otra tienda destino). El tag es opaco y vive sólo lo
/// que dura el gesto de un único mouse, así que no colisiona entre vistas.
pub fn begin_mix_slider_gesture(
    cx: &mut App,
    track_idx: usize,
    bounds: [f32; 4],
    x: f32,
    apply: impl FnOnce(&mut AppState, usize, f32),
) {
    let st = state(cx);
    st.update(cx, |s, cx| {
        s.matrix_mix_drag = Some(MatrixMixTarget::Volume(track_idx));
        s.matrix_mix_bounds = bounds;
        apply(s, track_idx, norm_from_slider_x(bounds, x));
        cx.notify();
    });
}

/// Paso del drag de volumen (`delta_x`): el overlay y el propio slider llaman
/// acá leyendo el gesto global, así ningún re-render lo corta.
fn continue_vol_drag(cx: &mut App, track_idx: usize, x: f32) {
    step_mix_slider_gesture(cx, track_idx, x, apply_matrix_volume);
}

/// Paso de un gesto de slider con apply genérico (ver
/// `begin_mix_slider_gesture`): ignora el evento si el tag global no es este
/// control y mapea con los bounds congelados al iniciar el gesto.
pub fn step_mix_slider_gesture(
    cx: &mut App,
    track_idx: usize,
    x: f32,
    apply: impl FnOnce(&mut AppState, usize, f32),
) {
    let st = state(cx);
    st.update(cx, |s, cx| {
        if s.matrix_mix_drag != Some(MatrixMixTarget::Volume(track_idx)) {
            return;
        }
        let bounds = s.matrix_mix_bounds;
        apply(s, track_idx, norm_from_slider_x(bounds, x));
        cx.notify();
    });
}

/// Inicia el drag de pan: solo siembra la base relativa con el valor actual
/// (como cualquier knob de DAW: agarrar NO cambia el valor, solo el drag).
/// Ya es genérico (no escribe nada): lo comparten matriz y playlist.
pub fn start_pan_drag(cx: &mut App, track_idx: usize, y: f32, current: f32) {
    let st = state(cx);
    st.update(cx, |s, cx| {
        s.matrix_mix_drag = Some(MatrixMixTarget::Pan(track_idx));
        s.matrix_pan_gesture = Some((y, current));
        cx.notify();
    });
}

/// Paso del drag de pan (`delta_y` relativo a la base global).
fn continue_pan_drag(cx: &mut App, track_idx: usize, y: f32) {
    step_mix_pan_gesture(cx, track_idx, y, apply_matrix_pan);
}

/// Paso de un gesto de pan con apply genérico (ver
/// `begin_mix_slider_gesture`): drag vertical relativo a la base sembrada al
/// agarrar, recorrido completo en `PAN_KNOB_TRAVEL` px.
pub fn step_mix_pan_gesture(
    cx: &mut App,
    track_idx: usize,
    y: f32,
    apply: impl FnOnce(&mut AppState, usize, f32),
) {
    let st = state(cx);
    st.update(cx, |s, cx| {
        if s.matrix_mix_drag != Some(MatrixMixTarget::Pan(track_idx)) {
            return;
        }
        if let Some((y0, p0)) = s.matrix_pan_gesture {
            apply(s, track_idx, p0 + (y0 - y) / PAN_KNOB_TRAVEL);
        }
        cx.notify();
    });
}

/// Cierra cualquier drag de mezcla (soltar el botón en cualquier lado).
/// Genérico: lo comparten matriz y playlist (mismo gesto global).
pub fn end_mix_drag(cx: &mut App) {
    let st = state(cx);
    st.update(cx, |s, cx| {
        if s.matrix_mix_drag.is_some() {
            s.matrix_mix_drag = None;
            s.matrix_pan_gesture = None;
            cx.notify();
        }
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
    track_idx: usize,
    norm: f32,
    dragging_this: bool,
) -> AnyElement {
    h_mix_slider_ex(
        id,
        track_idx,
        norm,
        dragging_this,
        VOLUME_RESET,
        apply_matrix_volume,
    )
}

/// Versión reutilizable del slider con escritura parametrizada.
///
/// Idéntica máquina de gestos (mismo tag/bounds globales), pero `apply`
/// decide la tienda destino: la matriz la fija a `apply_matrix_volume` y la
/// Playlist le pasa su apply por modo (matriz+live o studio). `reset_value`
/// es el valor del doble-clic (0.75 lineal = 0.0dB en ambas vistas).
pub fn h_mix_slider_ex(
    id: String,
    track_idx: usize,
    norm: f32,
    dragging_this: bool,
    reset_value: f32,
    apply: impl Fn(&mut AppState, usize, f32) + Copy + 'static,
) -> AnyElement {
    let norm = norm.clamp(0.0, 1.0);
    // Solo se miden bounds (canvas invisible); el flag de drag es global
    // (`matrix_mix_drag`) para que los re-renders no corten el gesto.
    let bounds_slot: Rc<Cell<[f32; 4]>> = Rc::new(Cell::new([0.0; 4]));
    let b_down = bounds_slot.clone();
    let b_paint = bounds_slot.clone();

    let thumb_x = MIX_THUMB_W / 2.0 + norm * (MIX_SLIDER_W - MIX_THUMB_W);

    div()
        .id(SharedString::from(id))
        .test_support()
        .flex()
        .items_center()
        .py(px(MIX_SLIDER_PAD_Y)) // ACÁ ESTABAS HDP, ME REFIERO AL SLIDER DE LOS HEADERS
        .flex_shrink_0()
        .rounded(px(3.0))
        .when(dragging_this, |d| d.cursor_grabbing())
        .when(!dragging_this, |d| d.cursor_pointer())
        .hover(|this| this.bg(rgb(0x232329)))
        .on_mouse_down(gpui_kit::MouseButton::Left, move |event, _, cx| {
            if event.click_count >= 2 {
                end_mix_drag(cx);
                let st = state(cx);
                st.update(cx, |s, cx| {
                    apply(s, track_idx, reset_value);
                    cx.notify();
                });
                return;
            }
            begin_mix_slider_gesture(cx, track_idx, b_down.get(), event.position.x.as_f32(), apply);
        })
        .on_mouse_move(move |event, _, cx| {
            step_mix_slider_gesture(cx, track_idx, event.position.x.as_f32(), apply);
        })
        .on_mouse_up(gpui_kit::MouseButton::Left, move |_, _, cx| {
            end_mix_drag(cx);
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
                        .top(px((MIX_SLIDER_H - 3.0) / 2.0))
                        .w(px(MIX_SLIDER_W))
                        .h(px(3.0))
                        .bg(rgb(0x2D2D2D))
                        .rounded(px(1.0)),
                )
                // Relleno desde la izquierda
                .child(
                    div()
                        .absolute()
                        .left(px(0.0))
                        .top(px((MIX_SLIDER_H - 3.0) / 2.0))
                        .w(px(thumb_x.max(1.0)))
                        .h(px(3.0))
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
const PAN_KNOB_SIZE: f32 = 22.0;
/// Padding alrededor del knob: la hitbox queda en 28×28 para que el agarre
/// no exija puntería de 1px.
const PAN_KNOB_PAD: f32 = 3.0;
/// Píxeles de drag vertical para recorrer el paneo entero (L→R).
const PAN_KNOB_TRAVEL: f32 = 150.0;

/// Knob rotativo de pan para el header (`Pan: ( O ) C`).
///
/// Disco + aguja pintados en canvas (misma técnica que el dial de
/// `controls.rs`): el ángulo va de −135° (L) a +135° (R) medidos desde las
/// 12, con el centro arriba.
///
/// Interacción estilo DAW estándar:
/// - Agarrar (mouse_down) NO cambia el valor: solo siembra la base del gesto.
/// - Drag vertical desde ahí: arriba → R, abajo → L (relativo al punto de
///   agarre, recorrido completo en `PAN_KNOB_TRAVEL` px).
/// - Doble-clic: vuelve exacto al centro.
fn pan_knob(track_idx: usize, pan: f32, dragging_this: bool) -> AnyElement {
    pan_knob_ex(
        SharedString::from(format!("matrix_panknob_{}", track_idx)),
        track_idx,
        pan,
        dragging_this,
        apply_matrix_pan,
    )
}

/// Versión reutilizable del knob con escritura parametrizada (ver
/// `h_mix_slider_ex`): misma máquina de gestos, otro destino. El doble-clic
/// siempre vuelve al centro exacto en ambas vistas.
pub fn pan_knob_ex(
    id: SharedString,
    track_idx: usize,
    pan: f32,
    dragging_this: bool,
    apply: impl Fn(&mut AppState, usize, f32) + Copy + 'static,
) -> AnyElement {
    // Display con detent central: aguja y etiqueta ven el mismo valor.
    let pan = snap_center_pan(if pan.is_finite() {
        pan.clamp(-1.0, 1.0)
    } else {
        0.0
    });
    // Sin bounds locales: agarrar no mapea posición a valor, así que el knob
    // no necesita medir nada (el gesto relativo vive en el estado global).
    div()
        .id(id)
        .test_support()
        .flex()
        .items_center()
        .justify_center()
        .p(px(PAN_KNOB_PAD))
        .flex_shrink_0()
        .rounded(px(14.0))
        .when(dragging_this, |d| d.cursor_grabbing())
        .when(!dragging_this, |d| d.cursor_pointer())
        .hover(|this| this.bg(rgb(0x232329)))
        .on_mouse_down(gpui_kit::MouseButton::Left, move |event, _, cx| {
            if event.click_count >= 2 {
                end_mix_drag(cx);
                let st = state(cx);
                st.update(cx, |s, cx| {
                    apply(s, track_idx, 0.0);
                    cx.notify();
                });
                return;
            }
            start_pan_drag(cx, track_idx, event.position.y.as_f32(), pan);
        })
        .on_mouse_move(move |event, _, cx| {
            step_mix_pan_gesture(cx, track_idx, event.position.y.as_f32(), apply);
        })
        .on_mouse_up(gpui_kit::MouseButton::Left, move |_, _, cx| {
            end_mix_drag(cx);
        })
        .child(
            div()
                .w(px(PAN_KNOB_SIZE))
                .h(px(PAN_KNOB_SIZE))
                .flex_shrink_0()
                .child(
                    canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    // Eje de origen explícito en f32 (ver requisito de
                    // bounding-box centering del knob).
                    let center_x =
                        bounds.origin.x.as_f32() + bounds.size.width.as_f32() / 2.0;
                    let center_y =
                        bounds.origin.y.as_f32() + bounds.size.height.as_f32() / 2.0;
                    let side = bounds
                        .size
                        .width
                        .as_f32()
                        .min(bounds.size.height.as_f32());
                    let radius = side / 2.0 - 1.0;
                    // Disco: quad con los cuatro radios a la mitad del lado.
                    window.paint_quad(PaintQuad {
                        bounds: Bounds::new(
                            point(px(center_x - radius), px(center_y - radius)),
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
                    // Aguja: el ángulo sale de `pan_needle_tip` (anchors del
                    // spec: L→−3π/4, C→−π/2, R→−π/4; barrido ±135°). En C la
                    // punta es bit-exacta sobre `center_x`.
                    //
                    // Nota sobre el stroke: la línea se pinta CENTRADA sobre
                    // el path (1.5px por lado), así que es simétrica por
                    // construcción y NO lleva compensación de `stroke/2`:
                    // restarla DESPLAZARÍA la aguja y crearía el offset
                    // reportado. Esa compensación solo aplica a fills
                    // alineados a borde.
                    let len = (radius - 4.0).max(0.0);
                    let (tip_x, tip_y) = pan_needle_tip(pan, center_x, center_y, len);
                    let mut needle = PathBuilder::stroke(px(3.0));
                    needle.move_to(point(px(center_x), px(center_y)));
                    needle.line_to(point(px(tip_x), px(tip_y)));
                    if let Ok(needle) = needle.build() {
                        window.paint_path(needle, rgb(0x00A2E8));
                    }
                    // Punto central: generoso y del mismo cian que la aguja
                    // para que pivote + aguja lean como una sola masa
                    // centrada (ancla visual contra offsets de 1px por
                    // reescalado del display).
                    window.paint_quad(PaintQuad {
                        bounds: Bounds::new(
                            point(px(center_x - 2.5), px(center_y - 2.5)),
                            size(px(5.0), px(5.0)),
                        ),
                        background: rgb(0x00A2E8).into(),
                        border_color: Hsla::default(),
                        corner_radii: gpui_kit::Corners {
                            top_left: px(2.5),
                            top_right: px(2.5),
                            bottom_right: px(2.5),
                            bottom_left: px(2.5),
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

/// Mini-toggle M/S de altura fija para el header.
///
/// El `Button` del kit fija su alto por tema (~24px+); este div de 22×18
/// compacta la fila del título y con ella toda la caja del header. Mismo
/// contrato que los botones originales (id + `test_support` + click).
/// También lo reutiliza el channel strip del Arranger (S/M/R compactos).
pub fn ms_button(
    id: String,
    glyph: &'static str,
    active: bool,
    active_color: Rgba,
    on_toggle: impl Fn(&mut App) + 'static,
) -> AnyElement {
    div()
        .id(SharedString::from(id))
        .test_support()
        .w(px(MS_BTN_W))
        .h(px(MS_BTN_H))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(2.0))
        .bg(rgb(0x3D3D3D))
        .hover(|this| this.bg(rgb(0x4D4D4D)))
        .active(|this| this.bg(rgb(0x5A5A5A)))
        .cursor_pointer()
        .child(
            Label::new(glyph)
                .text_size(px(10.0))
                .font_weight(FontWeight::BOLD)
                .text_color(if active {
                    active_color
                } else {
                    rgb(0xE0E0E0)
                }),
        )
        .on_click(move |_, _, cx| on_toggle(cx))
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
    mix_drag: Option<MatrixMixTarget>,
) -> AnyElement {
    v_flex()
        .w(px(HEADER_WIDTH))
        .bg(rgb(0x1C1C20))
        .border_1()
        .border_color(rgb(0x2D2D37))
        .rounded(px(4.0))
        .p(px(1.0))
        .gap(px(1.0))
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
                        .child(ms_button(
                            format!("track_mute_{}", track_idx),
                            "M",
                            muted,
                            rgb(0xFF5050),
                            move |cx| {
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
                            },
                        ))
                        .child(ms_button(
                            format!("track_solo_{}", track_idx),
                            "S",
                            soloed,
                            rgb(0xFFC800),
                            move |cx| {
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
                            },
                        )),
                ),
        )
        // Fila única de mezcla (layout legacy):
        //   [Knob Pan] [Pan Text] | [Vol Slider] [Vol dB Text]
        .child(
            h_flex()
                .items_center()
                .gap(px(4.0))
                .child(pan_knob(
                    track_idx,
                    pan,
                    mix_drag == Some(MatrixMixTarget::Pan(track_idx)),
                ))
                .child(
                    Label::new(pan_text(snap_center_pan(pan)))
                        .text_size(px(9.0))
                        .text_color(rgb(0xE0E0E0))
                        .w(px(24.0)),
                )
                .child(h_mix_slider(
                    format!("matrix_vol_{}", track_idx),
                    track_idx,
                    volume,
                    mix_drag == Some(MatrixMixTarget::Volume(track_idx)),
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
    // Gesto de mezcla en curso (para cursores y el overlay de captura).
    let mix_drag = app.matrix_mix_drag;
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
                    mix_drag,
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
        .relative()
        .size_full()
        .bg(rgb(0x1E1E1E))
        .gap(px(4.0))
        .child(
            h_flex()
                .items_center()
                .gap(px(8.0))
                .px(px(8.0))
                .py(px(2.0))
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
            // Wrapper observable del área de filas: el `Scrollable` interno
            // sobrescribe el id de su contenido con uno propio, así que la
            // medición (y el `flex_1` que habilita el scroll condicional)
            // viven en este `div` externo.
            div()
                .id("matrix_rows")
                .test_support()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .child(
                    v_flex()
                        .overflow_y_scrollbar()
                        .gap(px(3.0))
                        .children(track_rows),
                ),
        )
        // Capturador de drag a ventana completa: mientras hay un gesto de
        // mezcla en curso, este overlay invisible recibe TODOS los
        // mouse_move/mouse_up aunque el cursor se salga del control, así el
        // arrastre nunca se corta por 1px. Se monta solo durante el gesto.
        .when(mix_drag.is_some(), |this| {
            this.child(
                div()
                    .absolute()
                    .inset_0()
                    .id("matrix_mix_drag_catcher")
                    .cursor_grabbing()
                    .on_mouse_move(move |event, _, cx| {
                        // Si el botón se soltó fuera de la ventana, el próximo
                        // move llega sin botón: se cierra el gesto en vez de
                        // dejarlo colgado.
                        if event.pressed_button != Some(gpui_kit::MouseButton::Left) {
                            end_mix_drag(cx);
                            return;
                        }
                        let drag = state(cx).read(cx).matrix_mix_drag;
                        match drag {
                            Some(MatrixMixTarget::Volume(idx)) => {
                                continue_vol_drag(cx, idx, event.position.x.as_f32())
                            }
                            Some(MatrixMixTarget::Pan(idx)) => {
                                continue_pan_drag(cx, idx, event.position.y.as_f32())
                            }
                            None => {}
                        }
                    })
                    .on_mouse_up(gpui_kit::MouseButton::Left, move |_, _, cx| {
                        end_mix_drag(cx);
                    }),
            )
        })
        .into_any_element()
}

fn pitch_to_note_name(pitch: u8) -> &'static str {
    let names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    names[(pitch % 12) as usize]
}
