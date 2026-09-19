// Copyright (C) Hikaru Corporation - 2026
// Miyu's Audio Engine
// GNU Affero General Public License v3
// crates/hikaru_audio_engine/src/lib.rs

pub mod preview_player;
pub mod audio_drivers;

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use hikaru_core::{AudioBuffer, SampleRate};
use hikaru_dsp::effects::filter::StateVariableFilter;
use hikaru_dsp::synth::wavetable::WavetableOscillator;
use hikaru_sequencer::TrackMatrix;
use hikaru_transport::{TransportPlaybackState, TransportPosition};

const MAX_TRACK_BUF: usize = 2048 * 2;

// --- Definición de estructuras de clips MIDI ---
#[derive(Clone, Debug)]
pub struct MidiNoteInstance {
    pub start_tick: u64,
    pub pitch: u8,
    pub velocity: u8,
    pub duration_ticks: u32,
}

pub struct MidiClipInstance {
    pub track_index: usize,
    pub scene_index: usize,
    pub notes: Vec<MidiNoteInstance>,
    pub is_playing: bool,
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
static ORIGINAL_MXCSR: std::sync::OnceLock<u32> = std::sync::OnceLock::new();

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub fn enable_ftz_daz() {
    unsafe {
        let mut mxcsr: u32 = 0;
        std::arch::asm!("stmxcsr [{}]", in(reg) &mut mxcsr as *mut u32, options(nostack));
        let _ = ORIGINAL_MXCSR.set(mxcsr);
        mxcsr |= (1 << 15) | (1 << 6);
        std::arch::asm!("ldmxcsr [{}]", in(reg) &mxcsr as *const u32, options(nostack));
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub fn restore_mxcsr() {
    if let Some(&original) = ORIGINAL_MXCSR.get() {
        unsafe {
            std::arch::asm!("ldmxcsr [{}]", in(reg) &original as *const u32);
        }
    }
}

#[inline(always)]
pub fn flush_denormals(buf: &mut [f32]) {
    for s in buf.iter_mut() {
        let bits = s.to_bits();
        if (bits & 0x7F800000) == 0 && bits != 0 {
            *s = 0.0;
        }
    }
}

use crate::preview_player::PreviewPlayer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineMode {
    OpenLive,
    OpenStudio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceState {
    Active,
    Finished,
}

pub struct AudioClipInstance {
    pub id: usize,
    pub track_index: usize,
    pub scene_index: usize,
    pub samples: Vec<f32>,
    pub start_frame: u64,
    pub start_absolute: u64,
    pub duration_frames: u64,
    pub sample_offset: usize,
    pub channels: usize,
    pub is_playing: bool,
    pub clip_loop_enabled: bool,
    pub clip_loop_start: u64,
    pub clip_loop_end: u64,
    pub is_matrix_clip: bool,
    pub prev_frame: usize,
    pub xfade_remaining: u32,
    pub xfade_len: u32,
}

impl AudioClipInstance {
    pub fn natural_frames(&self) -> u64 {
        (self.samples.len() / self.channels.max(1)) as u64
    }

    pub fn has_valid_clip_loop(&self) -> bool {
        self.clip_loop_enabled
            && self.clip_loop_end > self.clip_loop_start
            && self.clip_loop_start < self.natural_frames()
    }

    pub fn playback_length_frames(&self) -> u64 {
        if self.has_valid_clip_loop() {
            let end = self.clip_loop_end.min(self.natural_frames());
            end.saturating_sub(self.clip_loop_start)
        } else {
            self.natural_frames()
        }
    }

    pub fn voice_frame(
        &self,
        global_pos: u64,
        transport: &TransportPosition,
    ) -> Option<usize> {
        if global_pos < self.start_frame {
            return None;
        }
        let natural = self.natural_frames();
        if natural == 0 {
            return None;
        }
        let elapsed = global_pos - self.start_frame;

        if self.has_valid_clip_loop() {
            let loop_start = self.clip_loop_start.min(natural);
            let loop_end = self.clip_loop_end.min(natural.max(1)).max(loop_start + 1);
            let loop_len = loop_end.saturating_sub(loop_start).max(1);
            if elapsed < loop_end {
                return Some(elapsed as usize);
            }
            if transport.has_valid_loop() {
                let since_global = global_pos.saturating_sub(transport.loop_start_samples);
                return Some((loop_start + (since_global % loop_len)) as usize);
            }
            return Some((loop_start + ((elapsed - loop_start) % loop_len)) as usize);
        }

        let emission_len = self.emission_len_frames();
        if elapsed >= emission_len {
            return None;
        }
        Some(elapsed as usize)
    }

    pub fn voice_frame_linear(&self, abs_pos: u64) -> Option<usize> {
        if abs_pos < self.start_absolute {
            return None;
        }
        let natural = self.natural_frames();
        if natural == 0 {
            return None;
        }
        let elapsed = abs_pos - self.start_absolute;

        if self.has_valid_clip_loop() {
            let loop_start = self.clip_loop_start.min(natural);
            let loop_end = self.clip_loop_end.min(natural.max(1)).max(loop_start + 1);
            let loop_len = loop_end.saturating_sub(loop_start).max(1);
            
            // Fuerza a que la lectura inicie directamente en loop_start
            return Some((loop_start + (elapsed % loop_len)) as usize);
        }

        let emission_len = self.emission_len_frames();
        if elapsed >= emission_len {
            return None;
        }
        Some(elapsed as usize)
    }

    pub fn voice_state_linear(&self, abs_pos: u64) -> VoiceState {
        match self.voice_frame_linear(abs_pos) {
            Some(_) => VoiceState::Active,
            None => VoiceState::Finished,
        }
    }

    pub fn emission_len_frames(&self) -> u64 {
        let natural = self.natural_frames();
        if self.duration_frames > 0 {
            self.duration_frames.min(natural)
        } else {
            natural
        }
    }

    pub fn voice_state(&self, global_pos: u64, transport: &TransportPosition) -> VoiceState {
        match self.voice_frame(global_pos, transport) {
            Some(_) => VoiceState::Active,
            None => VoiceState::Finished,
        }
    }

    pub fn read_sample(&self, current_sample_index: usize) -> f32 {
        if !self.has_valid_clip_loop() {
            if (current_sample_index as u64) >= self.natural_frames() {
                return 0.0;
            }
        }
        let ch = self.channels.max(1);
        let idx = current_sample_index.saturating_mul(ch);
        match self.samples.get(idx) {
            Some(&s) if s.is_finite() => s,
            _ => 0.0,
        }
    }

    pub fn read_sample_r(&self, current_sample_index: usize) -> f32 {
        if !self.has_valid_clip_loop() {
            if (current_sample_index as u64) >= self.natural_frames() {
                return 0.0;
            }
        }
        let ch = self.channels.max(1);
        let idx = current_sample_index.saturating_mul(ch);
        if ch > 1 {
            match self.samples.get(idx + 1) {
                Some(&s) if s.is_finite() => s,
                _ => 0.0,
            }
        } else {
            self.read_sample(current_sample_index)
        }
    }
}

pub struct AudioEngine<'a> {
    pub transport: TransportPosition,
    pub mode: EngineMode,
    pub matrix: TrackMatrix,
    pub main_filter: StateVariableFilter,
    pub oscillator: WavetableOscillator<'a>,
    pub sample_rate: f32,
    pub clips: Vec<AudioClipInstance>,
    pub preview_player: PreviewPlayer,
    pub position_clock: Arc<AtomicU64>,
    pub absolute_frame: u64,
    pub output_level_bits: Arc<AtomicU32>,
    pub track_peak_bits: [Arc<AtomicU32>; 16],
    track_output_bufs: Vec<Vec<f32>>,
    pub track_volumes: [AtomicU32; 16],
    pub track_pans: [AtomicU32; 16],
    pub track_mutes: [AtomicBool; 16],
    pub track_solos: [AtomicBool; 16],
    pub master_gain: AtomicU32,
    pub midi_clips: Vec<MidiClipInstance>, // Almacenamiento de clips MIDI activos
}

impl<'a> AudioEngine<'a> {
    pub fn new(sr: SampleRate, wavetable: &'a [f32], position_clock: Arc<AtomicU64>) -> Self {
        let mut filter = StateVariableFilter::new();
        filter.set_params(2000.0, 0.707, sr.get());

        Self {
            transport: TransportPosition::new(sr, 128.0),
            mode: EngineMode::OpenStudio,
            matrix: TrackMatrix::new(),
            main_filter: filter,
            oscillator: WavetableOscillator::new(wavetable, sr),
            sample_rate: sr.get(),
            clips: Vec::new(),
            preview_player: PreviewPlayer::new(),
            position_clock,
            absolute_frame: 0,
            output_level_bits: Arc::new(AtomicU32::new(0.0f32.to_bits())),
            track_peak_bits: std::array::from_fn(|_| Arc::new(AtomicU32::new(0.0f32.to_bits()))),
            track_output_bufs: (0..16).map(|_| Vec::with_capacity(MAX_TRACK_BUF)).collect(),
            track_volumes: std::array::from_fn(|_| AtomicU32::new(0.75f32.to_bits())),
            track_pans: std::array::from_fn(|_| AtomicU32::new(0.0f32.to_bits())),
            track_mutes: std::array::from_fn(|_| AtomicBool::new(false)),
            track_solos: std::array::from_fn(|_| AtomicBool::new(false)),
            master_gain: AtomicU32::new(0.75f32.to_bits()),
            midi_clips: Vec::new(),
        }
    }

    pub fn output_peak(&self) -> f32 {
        f32::from_bits(self.output_level_bits.load(Ordering::Relaxed))
    }

    pub fn track_peak(&self, track_idx: usize) -> f32 {
        if track_idx < 16 {
            f32::from_bits(self.track_peak_bits[track_idx].load(Ordering::Relaxed))
        } else {
            0.0
        }
    }

    pub fn output_db(&self) -> f32 {
        let peak = self.output_peak();
        if peak <= 0.0 || !peak.is_finite() {
            f32::NEG_INFINITY
        } else {
            20.0 * peak.log10()
        }
    }

    pub fn set_mode(&mut self, mode: EngineMode) {
        self.mode = mode;
    }

    pub fn set_sample_rate(&mut self, new_sr: f32) {
        self.sample_rate = new_sr;
        self.transport.sample_rate = hikaru_core::SampleRate::new(new_sr);
        self.main_filter.set_params(2000.0, 0.707, new_sr);
    }

    pub fn set_track_volume(&self, track_idx: usize, volume: f32) {
        if track_idx < 16 {
            self.track_volumes[track_idx].store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        }
    }

    pub fn set_track_pan(&self, track_idx: usize, pan: f32) {
        if track_idx < 16 {
            self.track_pans[track_idx].store(pan.clamp(-100.0, 100.0).to_bits(), Ordering::Relaxed);
        }
    }

    pub fn set_track_mute(&self, track_idx: usize, mute: bool) {
        if track_idx < 16 {
            self.track_mutes[track_idx].store(mute, Ordering::Relaxed);
        }
    }

    pub fn set_track_solo(&self, track_idx: usize, solo: bool) {
        if track_idx < 16 {
            self.track_solos[track_idx].store(solo, Ordering::Relaxed);
        }
    }

    pub fn set_master_gain(&self, volume: f32) {
        self.master_gain.store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn sync_clips(&mut self, new_clips: Vec<AudioClipInstance>) {
        self.clips = new_clips;
    }

    pub fn update_clip_bounds(
        &mut self,
        clip_id: usize,
        track_index: usize,
        scene_index: usize,
        start_secs: f32,
        duration_secs: f32,
        offset_secs: f32,
    ) {
        let is_studio = self.mode == EngineMode::OpenStudio;
        if let Some(clip) = self
            .clips
            .iter_mut()
            .find(|c| {
                if is_studio {
                    c.id == clip_id
                } else {
                    c.track_index == track_index && c.scene_index == scene_index
                }
            })
        {
            clip.start_frame = (start_secs * self.sample_rate) as u64;
            let natural = clip.natural_frames();
            clip.duration_frames = if duration_secs > 0.0 {
                (duration_secs * self.sample_rate) as u64
            } else {
                natural
            };
            clip.sample_offset = (offset_secs * self.sample_rate) as usize * clip.channels;
        }
    }

    pub fn set_clip_loop(
        &mut self,
        track_index: usize,
        scene_index: usize,
        start_secs: f32,
        end_secs: f32,
        enabled: bool,
    ) {
        let sr = self.sample_rate.max(1.0);
        if let Some(clip) = self
            .clips
            .iter_mut()
            .find(|c| c.track_index == track_index && c.scene_index == scene_index)
        {
            let natural = clip.natural_frames();
            let start = (start_secs.max(0.0) * sr) as u64;
            let mut end = (end_secs.max(0.0) * sr) as u64;
            if natural > 0 {
                end = end.min(natural);
            }
            if enabled && end > start && start < natural {
                clip.clip_loop_enabled = true;
                clip.clip_loop_start = start;
                clip.clip_loop_end = end;
            } else {
                clip.clip_loop_enabled = false;
                clip.clip_loop_start = 0;
                clip.clip_loop_end = 0;
            }
        }
    }

    pub fn add_clip(
        &mut self,
        id: usize,
        track_index: usize,
        scene_index: usize,
        samples: Vec<f32>,
        start_secs: f32,
        duration_secs: f32,
        offset_secs: f32,
        channels: usize,
        is_matrix_clip: bool,
    ) {
        let ch = channels.max(1);
        let start_frame = (start_secs * self.sample_rate) as u64;
        let natural_frames = (samples.len() / ch) as u64;
        let duration_frames = if duration_secs > 0.0 {
            (duration_secs * self.sample_rate) as u64
        } else {
            natural_frames
        };
        let sample_offset = (offset_secs * self.sample_rate) as usize * ch;

        let instance = AudioClipInstance {
            id,
            track_index,
            scene_index,
            samples,
            start_frame,
            start_absolute: 0,
            duration_frames,
            sample_offset,
            channels: ch,
            is_playing: false,
            clip_loop_enabled: false,
            clip_loop_start: 0,
            clip_loop_end: 0,
            is_matrix_clip,
            prev_frame: usize::MAX,
            xfade_remaining: 0,
            xfade_len: 0,
        };

        let is_studio = self.mode == EngineMode::OpenStudio;
        if let Some(existing) = self
            .clips
            .iter_mut()
            .find(|c| {
                if is_studio {
                    c.id == id
                } else {
                    c.track_index == track_index && c.scene_index == scene_index
                }
            })
        {
            *existing = instance;
        } else {
            self.clips.push(instance);
        }
    }

    pub fn remove_clip(&mut self, clip_id: usize, track_index: usize, scene_index: usize) {
        let is_studio = self.mode == EngineMode::OpenStudio;
        self.clips.retain(|c| {
            if is_studio {
                c.id != clip_id
            } else {
                !(c.track_index == track_index && c.scene_index == scene_index)
            }
        });
    }

    pub fn play(&mut self) {
        self.preview_player.stop();
        self.transport.playback_state = TransportPlaybackState::Playing;
    }

    pub fn pause(&mut self) {
        self.transport.playback_state = TransportPlaybackState::Paused;
    }

    pub fn stop(&mut self) {
        self.transport.playback_state = TransportPlaybackState::Stopped;
        self.transport.sample_count = 0;
    }

    pub fn set_global_loop(&mut self, start_samples: u64, end_samples: u64, enabled: bool) {
        self.transport.set_loop_region_samples(start_samples, end_samples);
        self.transport.set_loop_enabled(enabled);
    }

    pub fn seek(&mut self, position_secs: f32) {
        self.transport.sample_count = (position_secs * self.sample_rate) as u64;
    }

    pub fn trigger_clip(&mut self, track_index: usize, scene_index: usize) {
        let now = self.transport.sample_count;
        let now_abs = self.absolute_frame;
        let target_was_playing = self
            .clips
            .iter()
            .find(|c| c.track_index == track_index && c.scene_index == scene_index)
            .map(|c| c.is_playing)
            .unwrap_or(false);
        for clip in self.clips.iter_mut().filter(|c| c.track_index == track_index) {
            if clip.scene_index == scene_index {
                clip.is_playing = !target_was_playing;
                if clip.is_playing {
                    clip.start_frame = now;
                    clip.start_absolute = now_abs;
                }
            } else {
                clip.is_playing = false;
            }
        }
    }

    pub fn trigger_scene(&mut self, scene_index: usize) {
        let now = self.transport.sample_count;
        let now_abs = self.absolute_frame;
        let mut tracks_with_clip = std::collections::HashSet::new();
        for clip in self.clips.iter() {
            if clip.scene_index == scene_index {
                tracks_with_clip.insert(clip.track_index);
            }
        }
        for clip in self.clips.iter_mut() {
            if clip.scene_index == scene_index {
                if clip.is_playing && clip.start_absolute == now_abs {
                    continue;
                }
                clip.is_playing = true;
                clip.start_frame = now;
                clip.start_absolute = now_abs;
            } else if tracks_with_clip.contains(&clip.track_index) {
                clip.is_playing = false;
            }
        }
    }

    pub fn voice_active(&self, track_index: usize, scene_index: usize) -> bool {
        match self
            .clips
            .iter()
            .find(|c| c.track_index == track_index && c.scene_index == scene_index)
        {
            None => false,
            Some(clip) => {
                if !clip.is_playing {
                    return false;
                }
                clip.voice_state_linear(self.absolute_frame) == VoiceState::Active
            }
        }
    }

    pub fn voice_elapsed_frames(
        &self,
        track_index: usize,
        scene_index: usize,
    ) -> Option<u64> {
        let clip = self
            .clips
            .iter()
            .find(|c| c.track_index == track_index && c.scene_index == scene_index)?;
        if !clip.is_playing {
            return None;
        }
        if clip.voice_state_linear(self.absolute_frame) != VoiceState::Active {
            return None;
        }
        Some(self.absolute_frame.saturating_sub(clip.start_absolute))
    }

    pub fn stop_track(&mut self, track_index: usize) {
        for clip in self.clips.iter_mut().filter(|c| c.track_index == track_index) {
            clip.is_playing = false;
        }
    }

    pub fn process(&mut self, out_buffer: &mut AudioBuffer<'_>) {
        let samples = out_buffer.get_samples_mut();
        let num_channels = 2;

        samples.fill(0.0);

        if self.preview_player.is_active() {
            let preview = self.preview_player.shared_buffer();
            preview.process(samples);
        }

        if self.transport.playback_state == TransportPlaybackState::Playing {
            let buffer_frames = (samples.len() / num_channels) as u64;
            if buffer_frames == 0 {
                self.output_level_bits.store(0.0f32.to_bits(), Ordering::Relaxed);
                for p in &self.track_peak_bits {
                    p.store(0.0f32.to_bits(), Ordering::Relaxed);
                }
                return;
            }
            let buf_len = samples.len();
            let frame_start = self.transport.sample_count;
            let abs_start = self.absolute_frame;
            let transport = self.transport;
            let mode = self.mode;

            let any_solo = self.track_solos.iter().any(|s| s.load(Ordering::Relaxed));
            let mut track_gain_l = [0.0f32; 16];
            let mut track_gain_r = [0.0f32; 16];
            for t in 0..16 {
                let vol = f32::from_bits(self.track_volumes[t].load(Ordering::Relaxed));
                let muted = self.track_mutes[t].load(Ordering::Relaxed);
                let soloed = self.track_solos[t].load(Ordering::Relaxed);
                let effective_gain = if muted {
                    0.0
                } else if any_solo && !soloed {
                    0.0
                } else {
                    vol
                };
                let pan_norm = f32::from_bits(self.track_pans[t].load(Ordering::Relaxed)) / 100.0;
                track_gain_l[t] = effective_gain * if pan_norm > 0.0 {
                    1.0 - pan_norm
                } else {
                    1.0
                };
                track_gain_r[t] = effective_gain * if pan_norm < 0.0 {
                    1.0 + pan_norm
                } else {
                    1.0
                };
            }

            for buf in self.track_output_bufs.iter_mut() {
                if buf.len() != buf_len {
                    buf.resize(buf_len, 0.0);
                }
                buf.fill(0.0);
            }

            for clip in self.clips.iter_mut() {
                let ti = clip.track_index.min(15);
                let gl = track_gain_l[ti];
                let gr = track_gain_r[ti];
                if gl == 0.0 && gr == 0.0 {
                    continue;
                }
                let track_buf = &mut self.track_output_bufs[ti];

                let ch = clip.channels.max(1);
                let buf_ptr = clip.samples.as_ptr();
                let src_len = clip.samples.len();
                let offset_frames = clip.sample_offset / ch;
                let has_loop = clip.has_valid_clip_loop();

                if mode == EngineMode::OpenStudio {
                    let clip_frame_end = clip.start_frame.saturating_add(clip.duration_frames);
                    for f in 0..buffer_frames as usize {
                        let global_frame = transport.wrap_sample_count(frame_start + f as u64);
                        if global_frame < clip.start_frame || global_frame >= clip_frame_end {
                            continue;
                        }
                        let relative_frame = (global_frame - clip.start_frame) as usize;
                        let idx = (offset_frames + relative_frame) * ch;

                        if idx + ch <= src_len {
                            let l = unsafe { *buf_ptr.add(idx) } * gl;
                            let r = if ch > 1 {
                                (unsafe { *buf_ptr.add(idx + 1) }) * gr
                            } else {
                                l
                            };
                            track_buf[f * 2] += l;
                            track_buf[f * 2 + 1] += r;
                        }
                    }
                } else {
                    if !clip.is_playing {
                        continue;
                    }

                    if abs_start < clip.start_absolute {
                        continue;
                    }

                    let elapsed_start = abs_start - clip.start_absolute;
                    let emission = clip.emission_len_frames();

                    if !has_loop && elapsed_start >= emission {
                        clip.is_playing = false;
                        continue;
                    }

                    let natural = clip.natural_frames();
                    if natural == 0 {
                        continue;
                    }

                    let (loop_start, loop_len) = if has_loop {
                        let ls = clip.clip_loop_start.min(natural);
                        let le = clip.clip_loop_end.min(natural.max(1)).max(ls + 1);
                        (ls, le.saturating_sub(ls).max(1))
                    } else {
                        (0, 0)
                    };

                    let xfade_frames: usize = if has_loop { 32 } else { 0 };
                    let mut clip_finished = false;

                    for f in 0..buffer_frames as usize {
                        let elapsed = elapsed_start + f as u64;

                        if !has_loop && elapsed >= emission {
                            clip_finished = true;
                            break;
                        }

                        let relative_frame = if has_loop {
                            (loop_start + (elapsed % loop_len)) as usize
                        } else {
                            elapsed as usize
                        };

                        let idx = (offset_frames + relative_frame) * ch;

                        if xfade_frames > 0
                            && clip.prev_frame != usize::MAX
                            && relative_frame < clip.prev_frame
                            && clip.prev_frame - relative_frame > (emission / 2) as usize
                        {
                            clip.xfade_remaining = xfade_frames as u32;
                            clip.xfade_len = xfade_frames as u32;
                        }

                        if clip.xfade_remaining > 0 && idx + ch <= src_len {
                            let fade_pos = clip.xfade_len - clip.xfade_remaining;
                            let fade_total = clip.xfade_len as f32;
                            let out_gain = fade_pos as f32 / fade_total;
                            let in_gain = 1.0 - out_gain;

                            let l_new = unsafe { *buf_ptr.add(idx) } * gl;
                            let r_new = if ch > 1 {
                                (unsafe { *buf_ptr.add(idx + 1) }) * gr
                            } else {
                                l_new
                            };

                            let prev_idx = (offset_frames + clip.prev_frame) * ch;
                            let (l_old, r_old) = if prev_idx + ch <= src_len {
                                let lo = unsafe { *buf_ptr.add(prev_idx) } * gl;
                                let ro = if ch > 1 {
                                    (unsafe { *buf_ptr.add(prev_idx + 1) }) * gr
                                } else {
                                    lo
                                };
                                (lo, ro)
                            } else {
                                (0.0, 0.0)
                            };

                            track_buf[f * 2] += l_old * in_gain + l_new * out_gain;
                            track_buf[f * 2 + 1] += r_old * in_gain + r_new * out_gain;
                            clip.xfade_remaining -= 1;
                        } else if idx + ch <= src_len {
                            let l = unsafe { *buf_ptr.add(idx) } * gl;
                            let r = if ch > 1 {
                                (unsafe { *buf_ptr.add(idx + 1) }) * gr
                            } else {
                                l
                            };
                            track_buf[f * 2] += l;
                            track_buf[f * 2 + 1] += r;
                        }

                        clip.prev_frame = relative_frame;
                    }

                    if clip_finished {
                        clip.is_playing = false;
                    }
                }
            }

            for t in 0..16 {
                let track_buf = &self.track_output_bufs[t];
                let mut track_peak = 0.0f32;

                for i in 0..buf_len {
                    let v = track_buf[i];
                    let a = if v.is_finite() { v.abs() } else { 0.0 };
                    if a > track_peak {
                        track_peak = a;
                    }
                    samples[i] += v;
                }

                self.track_peak_bits[t].store(track_peak.to_bits(), Ordering::Relaxed);
            }

            self.transport.sample_count = self.transport.wrap_sample_count(frame_start + buffer_frames);
            self.absolute_frame = self.absolute_frame.saturating_add(buffer_frames);
        } else {
            for p in &self.track_peak_bits {
                p.store(0.0f32.to_bits(), Ordering::Relaxed);
            }
        }

        flush_denormals(samples);

        let mg = f32::from_bits(self.master_gain.load(Ordering::Relaxed));
        if mg != 1.0 {
            for s in samples.iter_mut() {
                *s *= mg;
            }
        }

        let mut peak = 0.0f32;
        for s in samples.iter_mut() {
            *s = s.tanh();
            let a = (*s).abs();
            if a > peak {
                peak = a;
            }
        }
        self.output_level_bits.store(peak.to_bits(), Ordering::Relaxed);
        self.position_clock.store(self.transport.sample_count, Ordering::Relaxed);
    }

        // Método para sincronizar y actualizar las notas de un clip MIDI en tiempo real
        pub fn update_midi_clip(
            &mut self,
            track_index: usize,
            scene_index: usize,
            notes: Vec<MidiNoteInstance>,
        ) {
            if let Some(clip) = self
                .midi_clips
                .iter_mut()
                .find(|c| c.track_index == track_index && c.scene_index == scene_index)
            {
                clip.notes = notes;
            } else {
                self.midi_clips.push(MidiClipInstance {
                    track_index,
                    scene_index,
                    notes,
                    is_playing: false,
                });
            }
        }
    //
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    static TABLE: [f32; 2048] = [0.0; 2048];

    fn test_engine() -> AudioEngine<'static> {
        let clock = Arc::new(AtomicU64::new(0));
        let engine = AudioEngine::new(SampleRate::new(44100.0), &TABLE, clock);
        for t in 0..16 {
            engine.track_volumes[t].store(1.0f32.to_bits(), Ordering::Relaxed);
        }
        engine.master_gain.store(1.0f32.to_bits(), Ordering::Relaxed);
        engine
    }

    fn run_frames(engine: &mut AudioEngine, frames: u64) {
        let mut raw = vec![0.0f32; (frames * 2) as usize];
        let mut buf = AudioBuffer::new(&mut raw);
        engine.process(&mut buf);
    }

    #[test]
    fn openlive_clip_duration_is_real_audio_length() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        let samples = vec![0.5f32; 44100 * 2];
        engine.add_clip(1, 0, 0, samples, 0.0, 0.0, 0.0, 2, false);
        assert_eq!(engine.clips[0].duration_frames, 44100);
        assert_eq!(engine.clips[0].playback_length_frames(), 44100);
    }

    #[test]
    fn trigger_clip_toggles_and_restarts_phase() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.transport.sample_count = 8000;
        engine.add_clip(1, 0, 0, vec![0.5f32; 100 * 2], 0.0, 0.0, 0.0, 2, false);
        engine.trigger_clip(0, 0);
        assert!(engine.clips[0].is_playing);
        assert_eq!(engine.clips[0].start_frame, 8000);
        engine.trigger_clip(0, 0);
        assert!(!engine.clips[0].is_playing);
    }

    #[test]
    fn trigger_scene_plays_whole_row() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.add_clip(1, 0, 0, vec![0.5f32; 100 * 2], 0.0, 0.0, 0.0, 2, false);
        engine.add_clip(2, 1, 0, vec![0.5f32; 100 * 2], 0.0, 0.0, 0.0, 2, false);
        engine.add_clip(3, 0, 1, vec![0.5f32; 100 * 2], 0.0, 0.0, 0.0, 2, false);
        engine.trigger_scene(1);
        assert!(!engine.clips[0].is_playing);
        assert!(!engine.clips[1].is_playing);
        assert!(engine.clips[2].is_playing);
    }

    #[test]
    fn global_loop_wraps_in_engine_not_gui() {
        let mut engine = test_engine();
        engine.play();
        engine.set_global_loop(0, 44100, true);
        for _ in 0..100 {
            run_frames(&mut engine, 512);
        }
        assert_eq!(engine.transport.sample_count, 51200 - 44100);
        assert_eq!(
            engine.position_clock.load(Ordering::Relaxed),
            engine.transport.sample_count
        );
    }

    #[test]
    fn global_loop_disabled_never_wraps() {
        let mut engine = test_engine();
        engine.play();
        for _ in 0..100 {
            run_frames(&mut engine, 512);
        }
        assert_eq!(engine.transport.sample_count, 51200);
    }

    #[test]
    fn voice_frame_mutes_past_natural_without_clip_loop() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.add_clip(1, 0, 0, vec![0.5f32; 1000 * 2], 0.0, 0.0, 0.0, 2, false);
        let t = engine.transport;
        assert!(!engine.clips[0].has_valid_clip_loop());
        assert_eq!(engine.clips[0].voice_frame(0, &t), Some(0));
        assert_eq!(engine.clips[0].voice_frame(999, &t), Some(999));
        assert_eq!(engine.clips[0].voice_frame(1000, &t), None);
        assert_eq!(engine.clips[0].voice_frame(5000, &t), None);
    }

    #[test]
    fn voice_frame_with_explicit_clip_loop_wraps() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.add_clip(1, 0, 0, vec![0.5f32; 1000 * 2], 0.0, 0.0, 0.0, 2, false);
        engine.set_clip_loop(0, 0, 0.0, 0.25, true);
        assert!(engine.clips[0].has_valid_clip_loop());
        let t = engine.transport;
        let wrapped = engine.clips[0].voice_frame(5000, &t).unwrap();
        assert!((0..1000).contains(&(wrapped as u64)));
    }

    #[test]
    fn clip_loop_region_is_honored() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.add_clip(1, 0, 0, vec![0.5f32; 44100 * 2], 0.0, 0.0, 0.0, 2, false);
        engine.set_clip_loop(0, 0, 0.0, 0.5, true);
        let clip = &engine.clips[0];
        assert!(clip.has_valid_clip_loop());
        assert_eq!(clip.clip_loop_start, 0);
        assert_eq!(clip.clip_loop_end, 22050);
        assert_eq!(clip.playback_length_frames(), 22050);
        engine.set_clip_loop(0, 0, 0.0, 0.0, false);
        assert!(!engine.clips[0].has_valid_clip_loop());
        assert_eq!(engine.clips[0].playback_length_frames(), 44100);
    }

    #[test]
    fn play_stops_preview_immediately() {
        let mut engine = test_engine();
        engine.preview_player.play(vec![0.5f32; 1024], 1);
        assert!(engine.preview_player.is_playing());
        engine.play();
        assert!(!engine.preview_player.is_playing());
    }

    #[test]
    fn process_in_playing_always_mixes_preview() {
        let mut engine = test_engine();
        engine.preview_player.play(vec![0.5f32; 4096], 1);
        engine.play();
        engine.preview_player.play(vec![0.5f32; 4096], 1);
        assert!(engine.preview_player.is_playing());
        let mut raw = vec![0.0f32; 512 * 2];
        let mut buf = AudioBuffer::new(&mut raw);
        engine.process(&mut buf);
        let peak = raw.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert!(peak > 0.0);
        assert!(engine.output_peak() > 0.0);
    }

    #[test]
    fn clip_length_frames_uses_real_transport_tempo() {
        let mut engine = test_engine();
        engine.transport.set_bpm(120.0);
        let len_120 = engine.transport.clip_length_frames(3840);
        engine.transport.set_bpm(135.0);
        let len_135 = engine.transport.clip_length_frames(3840);
        assert_ne!(len_120, len_135);
        assert_eq!(len_120, 88200);
        assert_eq!(len_135, 78400);
        assert_eq!(len_135, engine.transport.ticks_to_samples(3840));
    }

    #[test]
    fn openlive_bar10_is_silent_with_15bar_time_selection() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        let spb = engine.transport.samples_per_bar();
        let bar_frames = spb.round() as u64;
        let nine_bars = bar_frames * 9;
        let fifteen_bars = bar_frames * 15;
        engine.add_clip(
            1, 0, 0,
            vec![0.5f32; (nine_bars as usize) * 2],
            0.0, 0.0, 0.0, 2, false,
        );
        assert!(!engine.clips[0].has_valid_clip_loop());
        engine.set_global_loop(0, fifteen_bars, true);
        engine.trigger_clip(0, 0);
        engine.play();
        let mut remaining = nine_bars;
        while remaining > 0 {
            let step = remaining.min(512);
            run_frames(&mut engine, step);
            remaining -= step;
        }
        let mut raw = vec![0.777f32; 512 * 2];
        let mut buf = AudioBuffer::new(&mut raw);
        engine.process(&mut buf);
        let peak = raw.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert_eq!(peak, 0.0);
        assert_eq!(engine.output_peak(), 0.0);
        assert_eq!(engine.output_db(), f32::NEG_INFINITY);
        assert!(!engine.clips[0].is_playing);
    }

    #[test]
    fn openlive_short_time_selection_does_not_extend_voice() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        let spb = engine.transport.samples_per_bar();
        let bar_frames = spb.round() as u64;
        let nine_bars = bar_frames * 9;
        engine.add_clip(
            1, 0, 0,
            vec![0.5f32; (nine_bars as usize) * 2],
            0.0, 0.0, 0.0, 2, false,
        );
        engine.set_global_loop(0, bar_frames * 4, true);
        engine.trigger_clip(0, 0);
        engine.play();
        let mut remaining = bar_frames * 10;
        let mut last_peak = 1.0f32;
        while remaining > 0 {
            let step = remaining.min(512);
            let mut raw = vec![0.0f32; (step * 2) as usize];
            let mut buf = AudioBuffer::new(&mut raw);
            engine.process(&mut buf);
            last_peak = raw.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
            remaining -= step;
        }
        assert_eq!(last_peak, 0.0);
        assert!(!engine.clips[0].is_playing);
        assert_eq!(engine.output_db(), f32::NEG_INFINITY);
    }

    #[test]
    fn openlive_explicit_clip_loop_keeps_sounding_past_bar9() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        let spb = engine.transport.samples_per_bar();
        let bar_frames = spb.round() as u64;
        let nine_bars = bar_frames * 9;
        engine.add_clip(
            1, 0, 0,
            vec![0.5f32; (nine_bars as usize) * 2],
            0.0, 0.0, 0.0, 2, false,
        );
        let sr = engine.sample_rate;
        engine.set_clip_loop(0, 0, 0.0, (bar_frames as f32 / sr) as f32, true);
        assert!(engine.clips[0].has_valid_clip_loop());
        engine.set_global_loop(0, bar_frames * 15, true);
        engine.trigger_clip(0, 0);
        engine.play();
        let mut remaining = bar_frames * 10;
        let mut last_peak = 0.0f32;
        while remaining > 0 {
            let step = remaining.min(512);
            let mut raw = vec![0.0f32; (step * 2) as usize];
            let mut buf = AudioBuffer::new(&mut raw);
            engine.process(&mut buf);
            last_peak = raw.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
            remaining -= step;
        }
        let expected_peak = 0.5f32.tanh();
        assert!(
            (last_peak - expected_peak).abs() < 1e-4,
            "peak {} debe ser ≈ tanh(0.5) = {}",
            last_peak, expected_peak
        );
        assert!(engine.clips[0].is_playing);
    }

    #[test]
    fn openlive_trigger_starts_at_sample_zero() {
        for flow in 0..3 {
            let mut engine = test_engine();
            engine.set_mode(EngineMode::OpenLive);
            let n = 8192u64;
            let mut samples = Vec::with_capacity((n * 2) as usize);
            for i in 0..n {
                let v = i as f32 / n as f32;
                samples.push(v);
                samples.push(v);
            }
            engine.add_clip(1, 0, 0, samples, 0.0, 0.0, 0.0, 2, false);
            match flow {
                0 => {
                    engine.trigger_clip(0, 0);
                    engine.play();
                }
                1 => {
                    engine.play();
                    run_frames(&mut engine, 200_000);
                    engine.trigger_clip(0, 0);
                }
                _ => {
                    engine.trigger_clip(0, 0);
                    engine.play();
                    run_frames(&mut engine, 1000);
                    engine.stop();
                    engine.trigger_clip(0, 0);
                    engine.trigger_clip(0, 0);
                    engine.play();
                }
            }
            let mut raw = vec![0.0f32; 512 * 2];
            let mut buf = AudioBuffer::new(&mut raw);
            engine.process(&mut buf);
            for f in 0..512usize {
                let expect = f as f32 / n as f32;
                assert!(
                    (raw[f * 2] - expect).abs() < 2e-3,
                    "flow {}: frame L{} = {} (esperado {})",
                    flow, f, raw[f * 2], expect
                );
                assert!(
                    (raw[f * 2 + 1] - expect).abs() < 2e-3,
                    "flow {}: frame R{} = {} (esperado {})",
                    flow, f, raw[f * 2 + 1], expect
                );
            }
        }
    }

    #[test]
    fn openlive_strict_close_past_native_length() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        let n = 4096u64;
        engine.add_clip(1, 0, 0, vec![0.5f32; (n * 2) as usize], 0.0, 0.0, 0.0, 2, false);
        engine.trigger_clip(0, 0);
        engine.play();
        let mut remaining = n;
        while remaining > 0 {
            let step = remaining.min(512);
            run_frames(&mut engine, step);
            remaining -= step;
        }
        for _ in 0..3 {
            let mut raw = vec![0.777f32; 512 * 2];
            let mut buf = AudioBuffer::new(&mut raw);
            engine.process(&mut buf);
            let peak = raw.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
            assert_eq!(peak, 0.0);
        }
        assert!(!engine.clips[0].is_playing);
        assert_eq!(engine.output_db(), f32::NEG_INFINITY);
    }

    #[test]
    fn openstudio_bar1_starts_at_sample_zero_and_ends_silent() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenStudio);
        let n = 4096u64;
        let mut samples = Vec::with_capacity((n * 2) as usize);
        for i in 0..n {
            let v = i as f32 / n as f32;
            samples.push(v);
            samples.push(v);
        }
        let duration_secs = n as f32 / engine.sample_rate;
        engine.add_clip(1, 0, 0, samples, 0.0, duration_secs, 0.0, 2, false);
        engine.transport.sample_count = 0;
        engine.play();
        let mut raw = vec![0.0f32; 512 * 2];
        let mut buf = AudioBuffer::new(&mut raw);
        engine.process(&mut buf);
        for f in 0..512usize {
            let expect = f as f32 / n as f32;
            assert!(
                (raw[f * 2] - expect).abs() < 2e-3,
                "frame L{} = {} (esperado {})",
                f, raw[f * 2], expect
            );
        }
        let mut remaining = n;
        while remaining > 0 {
            let step = remaining.min(512);
            run_frames(&mut engine, step);
            remaining -= step;
        }
        let mut raw = vec![0.777f32; 512 * 2];
        let mut buf = AudioBuffer::new(&mut raw);
        engine.process(&mut buf);
        let peak = raw.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert_eq!(peak, 0.0);
        assert_eq!(engine.output_db(), f32::NEG_INFINITY);
    }

    #[test]
    fn voice_elapsed_frames_tracks_linear_voice_clock() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.add_clip(1, 0, 0, vec![0.5f32; 8192 * 2], 0.0, 0.0, 0.0, 2, false);
        assert_eq!(engine.voice_elapsed_frames(0, 0), None);
        assert_eq!(engine.voice_elapsed_frames(9, 9), None);
        engine.trigger_clip(0, 0);
        engine.transport.sample_count = 200_000;
        assert_eq!(engine.voice_elapsed_frames(0, 0), Some(0));
        engine.play();
        run_frames(&mut engine, 512);
        assert_eq!(engine.voice_elapsed_frames(0, 0), Some(512));
        engine.transport.sample_count = 0;
        assert_eq!(engine.voice_elapsed_frames(0, 0), Some(512));
    }
}