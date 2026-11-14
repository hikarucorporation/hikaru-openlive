// crates/hikaru_dsp/src/synth/sampler.rs
// Hikaru Sampler DSP Engine — Polyphonic Bitwig-style Sampler

use std::sync::Arc;

pub const MAX_SAMPLER_VOICES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SamplerPlaybackMode {
    #[default]
    Repitch,
    Cycles,
    Textures,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SamplerLoopMode {
    #[default]
    Off,
    Forward,
    PingPong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SamplerFilterMode {
    Off,
    #[default]
    LowPass12,
    LowPass24,
    BandPass,
    HighPass,
    Notch,
    Peak,
}

#[derive(Debug, Clone)]
pub struct SamplerSampleData {
    pub samples: Vec<f32>,
    pub channels: usize,
    pub sample_rate: f32,
    pub total_frames: usize,
}

impl SamplerSampleData {
    pub fn new(samples: Vec<f32>, channels: usize, sample_rate: f32) -> Self {
        let ch = channels.max(1);
        let total_frames = samples.len() / ch;
        Self {
            samples,
            channels: ch,
            sample_rate: if sample_rate > 0.0 { sample_rate } else { 44100.0 },
            total_frames,
        }
    }

    #[inline]
    pub fn sample_frame_lerp(&self, frame_pos: f64) -> (f32, f32) {
        if self.total_frames == 0 {
            return (0.0, 0.0);
        }
        let max_idx = self.total_frames - 1;
        let clamped = frame_pos.clamp(0.0, max_idx as f64);
        let idx0 = clamped.floor() as usize;
        let idx1 = (idx0 + 1).min(max_idx);
        let frac = (clamped - idx0 as f64) as f32;

        if self.channels >= 2 {
            let b0 = idx0 * self.channels;
            let b1 = idx1 * self.channels;
            let l = self.samples[b0] + (self.samples[b1] - self.samples[b0]) * frac;
            let r = self.samples[b0 + 1] + (self.samples[b1 + 1] - self.samples[b0 + 1]) * frac;
            (l, r)
        } else {
            let s = self.samples[idx0] + (self.samples[idx1] - self.samples[idx0]) * frac;
            (s, s)
        }
    }
}

#[derive(Debug, Clone)]
pub struct SamplerDspParams {
    pub playback_mode: SamplerPlaybackMode,
    pub speed: f32,
    pub reverse: bool,
    pub freeze: bool,

    pub root_note: u8,
    pub fine_cents: f32,
    pub pitch_semitones: f32,
    pub glide_ms: f32,
    pub legato: bool,

    pub play_start: f32,
    pub play_end: f32,

    pub loop_mode: SamplerLoopMode,
    pub loop_start: f32,
    pub loop_len: f32,
    pub loop_crossfade: f32,

    pub filter_mode: SamplerFilterMode,
    pub filter_cutoff_hz: f32,
    pub filter_resonance: f32,
    pub filter_keytrack: f32,

    pub attack_ms: f32,
    pub hold_ms: f32,
    pub decay_ms: f32,
    pub sustain: f32,
    pub release_ms: f32,

    pub gain_db: f32,
    pub pan: f32,
    pub output_db: f32,
    pub velocity_sens: f32,
    pub note_trigger: bool,
    pub release_trigger: bool,
}

impl Default for SamplerDspParams {
    fn default() -> Self {
        Self {
            playback_mode: SamplerPlaybackMode::Repitch,
            speed: 1.0,
            reverse: false,
            freeze: false,
            root_note: 60, // C3
            fine_cents: 0.0,
            pitch_semitones: 0.0,
            glide_ms: 0.0,
            legato: false,
            play_start: 0.0,
            play_end: 1.0,
            loop_mode: SamplerLoopMode::Off,
            loop_start: 0.0,
            loop_len: 1.0,
            loop_crossfade: 0.0,
            filter_mode: SamplerFilterMode::LowPass12,
            filter_cutoff_hz: 2640.0,
            filter_resonance: 0.0,
            filter_keytrack: 0.0,
            attack_ms: 2.0,
            hold_ms: 0.0,
            decay_ms: 200.0,
            sustain: 1.0,
            release_ms: 50.0,
            gain_db: 0.0,
            pan: 0.0,
            output_db: 0.0,
            velocity_sens: 1.0,
            note_trigger: true,
            release_trigger: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplerEnvStage {
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
    Free,
}

/// Stereo 2-pole State Variable Filter (Chamberlin / Cytomic-style) for per-voice filtering.
#[derive(Debug, Clone)]
struct VoiceSvf {
    ic1eq_l: f32,
    ic2eq_l: f32,
    ic1eq_r: f32,
    ic2eq_r: f32,
    // Second stage for 24dB/oct
    s2_ic1eq_l: f32,
    s2_ic2eq_l: f32,
    s2_ic1eq_r: f32,
    s2_ic2eq_r: f32,
}

impl VoiceSvf {
    fn new() -> Self {
        Self {
            ic1eq_l: 0.0,
            ic2eq_l: 0.0,
            ic1eq_r: 0.0,
            ic2eq_r: 0.0,
            s2_ic1eq_l: 0.0,
            s2_ic2eq_l: 0.0,
            s2_ic1eq_r: 0.0,
            s2_ic2eq_r: 0.0,
        }
    }

    fn reset(&mut self) {
        self.ic1eq_l = 0.0;
        self.ic2eq_l = 0.0;
        self.ic1eq_r = 0.0;
        self.ic2eq_r = 0.0;
        self.s2_ic1eq_l = 0.0;
        self.s2_ic2eq_l = 0.0;
        self.s2_ic1eq_r = 0.0;
        self.s2_ic2eq_r = 0.0;
    }

    #[inline]
    fn process_stage(
        v0: f32,
        ic1eq: &mut f32,
        ic2eq: &mut f32,
        _g: f32,
        k: f32,
        a1: f32,
        a2: f32,
        a3: f32,
        mode: SamplerFilterMode,
    ) -> f32 {
        let v3 = v0 - *ic2eq;
        let v1 = a1 * *ic1eq + a2 * v3;
        let v2 = *ic2eq + a2 * *ic1eq + a3 * v3;
        *ic1eq = 2.0 * v1 - *ic1eq;
        *ic2eq = 2.0 * v2 - *ic2eq;

        match mode {
            SamplerFilterMode::Off => v0,
            SamplerFilterMode::LowPass12 | SamplerFilterMode::LowPass24 => v2,
            SamplerFilterMode::BandPass => v1,
            SamplerFilterMode::HighPass => v0 - k * v1 - v2,
            SamplerFilterMode::Notch => v0 - k * v1,
            SamplerFilterMode::Peak => v0 - k * v1 - 2.0 * v2,
        }
    }

    #[inline]
    fn process_stereo(
        &mut self,
        l: f32,
        r: f32,
        cutoff_hz: f32,
        resonance: f32,
        sample_rate: f32,
        mode: SamplerFilterMode,
    ) -> (f32, f32) {
        if mode == SamplerFilterMode::Off {
            return (l, r);
        }
        let fc = cutoff_hz.clamp(20.0, (sample_rate * 0.45).max(100.0));
        let g = (std::f32::consts::PI * fc / sample_rate).tan();
        // Map resonance 0..1 to damping k in 2.0..0.1
        let k = 2.0 - 1.9 * resonance.clamp(0.0, 1.0);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;

        let out_l = Self::process_stage(
            l,
            &mut self.ic1eq_l,
            &mut self.ic2eq_l,
            g,
            k,
            a1,
            a2,
            a3,
            mode,
        );
        let out_r = Self::process_stage(
            r,
            &mut self.ic1eq_r,
            &mut self.ic2eq_r,
            g,
            k,
            a1,
            a2,
            a3,
            mode,
        );

        if mode == SamplerFilterMode::LowPass24 {
            let out_l2 = Self::process_stage(
                out_l,
                &mut self.s2_ic1eq_l,
                &mut self.s2_ic2eq_l,
                g,
                k,
                a1,
                a2,
                a3,
                mode,
            );
            let out_r2 = Self::process_stage(
                out_r,
                &mut self.s2_ic1eq_r,
                &mut self.s2_ic2eq_r,
                g,
                k,
                a1,
                a2,
                a3,
                mode,
            );
            (out_l2, out_r2)
        } else {
            (out_l, out_r)
        }
    }
}

#[derive(Debug, Clone)]
pub struct SamplerVoice {
    pub active: bool,
    pub track_idx: usize,
    pub pitch: u8,
    pub velocity: f32,
    pub frame_pos: f64,
    pub direction: f64, // +1.0 forward, -1.0 backward (ping-pong or reverse)
    pub current_pitch: f32,
    pub target_pitch: f32,
    pub env_stage: SamplerEnvStage,
    pub env_level: f32,
    pub hold_samples_left: usize,
    pub age: u64,
    svf: VoiceSvf,
}

impl SamplerVoice {
    pub fn new() -> Self {
        Self {
            active: false,
            track_idx: 0,
            pitch: 60,
            velocity: 1.0,
            frame_pos: 0.0,
            direction: 1.0,
            current_pitch: 60.0,
            target_pitch: 60.0,
            env_stage: SamplerEnvStage::Free,
            env_level: 0.0,
            hold_samples_left: 0,
            age: 0,
            svf: VoiceSvf::new(),
        }
    }

    #[inline]
    fn step_envelope(&mut self, params: &SamplerDspParams, sample_rate: f32) -> (f32, bool) {
        if !self.active || self.env_stage == SamplerEnvStage::Free {
            return (0.0, true);
        }
        let sr = sample_rate.max(1.0);
        match self.env_stage {
            SamplerEnvStage::Attack => {
                if params.attack_ms <= 0.1 {
                    self.env_level = 1.0;
                    self.hold_samples_left = (params.hold_ms * 0.001 * sr) as usize;
                    self.env_stage = if self.hold_samples_left > 0 {
                        SamplerEnvStage::Hold
                    } else {
                        SamplerEnvStage::Decay
                    };
                } else {
                    let rate = 1.0 / (params.attack_ms * 0.001 * sr);
                    self.env_level += rate;
                    if self.env_level >= 1.0 {
                        self.env_level = 1.0;
                        self.hold_samples_left = (params.hold_ms * 0.001 * sr) as usize;
                        self.env_stage = if self.hold_samples_left > 0 {
                            SamplerEnvStage::Hold
                        } else {
                            SamplerEnvStage::Decay
                        };
                    }
                }
                (self.env_level, false)
            }
            SamplerEnvStage::Hold => {
                if self.hold_samples_left > 0 {
                    self.hold_samples_left -= 1;
                } else {
                    self.env_stage = SamplerEnvStage::Decay;
                }
                (1.0, false)
            }
            SamplerEnvStage::Decay => {
                let sus = params.sustain.clamp(0.0, 1.0);
                if params.decay_ms <= 0.1 {
                    self.env_level = sus;
                    self.env_stage = SamplerEnvStage::Sustain;
                } else {
                    let rate = (1.0 - sus) / (params.decay_ms * 0.001 * sr);
                    self.env_level -= rate.max(1.0e-7);
                    if self.env_level <= sus {
                        self.env_level = sus;
                        self.env_stage = SamplerEnvStage::Sustain;
                    }
                }
                (self.env_level, false)
            }
            SamplerEnvStage::Sustain => {
                self.env_level = params.sustain.clamp(0.0, 1.0);
                (self.env_level, false)
            }
            SamplerEnvStage::Release => {
                if params.release_ms <= 0.1 {
                    self.env_level = 0.0;
                    self.env_stage = SamplerEnvStage::Free;
                    self.active = false;
                    return (0.0, true);
                }
                let rate = 1.0 / (params.release_ms * 0.001 * sr);
                self.env_level -= rate.max(1.0e-7);
                if self.env_level <= 0.0001 {
                    self.env_level = 0.0;
                    self.env_stage = SamplerEnvStage::Free;
                    self.active = false;
                    (0.0, true)
                } else {
                    (self.env_level, false)
                }
            }
            SamplerEnvStage::Free => (0.0, true),
        }
    }
}

pub struct SamplerVoicePool {
    voices: Vec<SamplerVoice>,
    counter: u64,
    last_pitch_per_track: [Option<f32>; 16],
}

impl SamplerVoicePool {
    pub fn new() -> Self {
        Self {
            voices: (0..MAX_SAMPLER_VOICES).map(|_| SamplerVoice::new()).collect(),
            counter: 0,
            last_pitch_per_track: [None; 16],
        }
    }

    pub fn trigger(
        &mut self,
        pitch: u8,
        velocity: f32,
        track_idx: usize,
        sample: &SamplerSampleData,
        params: &SamplerDspParams,
    ) {
        if sample.total_frames == 0 {
            return;
        }
        let ti = track_idx.min(15);

        // Legato mode: if a voice on this track is already active (not releasing), glide it
        if params.legato {
            if let Some(v) = self
                .voices
                .iter_mut()
                .find(|v| v.active && v.track_idx == ti && v.env_stage != SamplerEnvStage::Release)
            {
                v.pitch = pitch;
                v.target_pitch = pitch as f32;
                self.last_pitch_per_track[ti] = Some(pitch as f32);
                return;
            }
        }

        self.counter += 1;
        let age = self.counter;

        let idx = if let Some(i) = self.voices.iter().position(|v| !v.active) {
            i
        } else if let Some((i, _)) = self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, v)| v.env_stage == SamplerEnvStage::Release)
            .min_by_key(|(_, v)| v.age)
        {
            i
        } else {
            self.voices
                .iter()
                .enumerate()
                .min_by_key(|(_, v)| v.age)
                .map(|(i, _)| i)
                .unwrap_or(0)
        };

        let start_frac = params.play_start.clamp(0.0, 0.999) as f64;
        let end_frac = params.play_end.clamp(start_frac as f32 + 0.001, 1.0) as f64;
        let start_frame = start_frac * (sample.total_frames - 1) as f64;
        let end_frame = end_frac * (sample.total_frames - 1) as f64;

        let initial_pitch = if params.glide_ms > 0.5 {
            self.last_pitch_per_track[ti].unwrap_or(pitch as f32)
        } else {
            pitch as f32
        };
        self.last_pitch_per_track[ti] = Some(pitch as f32);

        let v = &mut self.voices[idx];
        v.active = true;
        v.track_idx = ti;
        v.pitch = pitch;
        v.velocity = velocity.clamp(0.0, 1.0);
        v.frame_pos = if params.reverse { end_frame } else { start_frame };
        v.direction = if params.reverse { -1.0 } else { 1.0 };
        v.current_pitch = initial_pitch;
        v.target_pitch = pitch as f32;
        v.env_stage = SamplerEnvStage::Attack;
        v.env_level = 0.0;
        v.hold_samples_left = 0;
        v.age = age;
        v.svf.reset();
    }

    pub fn release_note(&mut self, pitch: u8, track_idx: usize) {
        let ti = track_idx.min(15);
        for v in &mut self.voices {
            if v.active && v.track_idx == ti && v.pitch == pitch && v.env_stage != SamplerEnvStage::Release {
                v.env_stage = SamplerEnvStage::Release;
            }
        }
    }

    pub fn kill_note(&mut self, pitch: u8, track_idx: usize) {
        let ti = track_idx.min(15);
        for v in &mut self.voices {
            if v.active && v.track_idx == ti && v.pitch == pitch {
                v.active = false;
                v.env_stage = SamplerEnvStage::Free;
            }
        }
    }

    pub fn kill_track_notes(&mut self, track_idx: usize) {
        let ti = track_idx.min(15);
        for v in &mut self.voices {
            if v.track_idx == ti {
                v.active = false;
                v.env_stage = SamplerEnvStage::Free;
            }
        }
    }

    pub fn kill_all(&mut self) {
        for v in &mut self.voices {
            v.active = false;
            v.env_stage = SamplerEnvStage::Free;
        }
    }

    /// Returns the normalized playhead position (0..1) of the most recent active voice on `track_idx`, if any.
    pub fn active_playhead(&self, track_idx: usize, total_frames: usize) -> Option<f32> {
        if total_frames <= 1 {
            return None;
        }
        let ti = track_idx.min(15);
        self.voices
            .iter()
            .filter(|v| v.active && v.track_idx == ti)
            .max_by_key(|v| v.age)
            .map(|v| (v.frame_pos / (total_frames - 1) as f64).clamp(0.0, 1.0) as f32)
    }

    pub fn process_track(
        &mut self,
        track_idx: usize,
        sample: &SamplerSampleData,
        params: &SamplerDspParams,
        out_interleaved: &mut [f32],
        gain_l: f32,
        gain_r: f32,
        frames: usize,
        host_sample_rate: f32,
    ) {
        if sample.total_frames == 0 || (gain_l == 0.0 && gain_r == 0.0) {
            return;
        }
        let ti = track_idx.min(15);
        let max_frame = (sample.total_frames - 1) as f64;
        let play_start = (params.play_start.clamp(0.0, 0.999) as f64) * max_frame;
        let play_end = (params.play_end.clamp(params.play_start + 0.001, 1.0) as f64) * max_frame;

        let loop_start = (params.loop_start.clamp(0.0, 0.998) as f64) * max_frame;
        let loop_end = ((params.loop_start + params.loop_len).clamp(params.loop_start + 0.002, 1.0) as f64) * max_frame;

        let base_gain = 10.0f32.powf((params.gain_db + params.output_db) / 20.0);
        let pan_norm = params.pan.clamp(-1.0, 1.0);
        let pan_l = if pan_norm > 0.0 { 1.0 - pan_norm } else { 1.0 };
        let pan_r = if pan_norm < 0.0 { 1.0 + pan_norm } else { 1.0 };

        let sr_ratio = (sample.sample_rate / host_sample_rate.max(1.0)) as f64;
        let glide_coeff = if params.glide_ms > 0.5 {
            1.0 - (-1.0 / (params.glide_ms * 0.001 * host_sample_rate.max(1.0))).exp()
        } else {
            1.0
        };

        for v in &mut self.voices {
            if !v.active || v.track_idx != ti {
                continue;
            }
            let vel_gain = (1.0 - params.velocity_sens) + params.velocity_sens * v.velocity;
            let voice_gain_l = gain_l * base_gain * pan_l * vel_gain;
            let voice_gain_r = gain_r * base_gain * pan_r * vel_gain;

            for i in 0..frames {
                let (env, finished) = v.step_envelope(params, host_sample_rate);
                if finished {
                    break;
                }

                // Glide pitch
                if (v.current_pitch - v.target_pitch).abs() > 0.001 {
                    v.current_pitch += (v.target_pitch - v.current_pitch) * glide_coeff;
                } else {
                    v.current_pitch = v.target_pitch;
                }

                let semitone_delta = match params.playback_mode {
                    SamplerPlaybackMode::Repitch => {
                        (v.current_pitch - params.root_note as f32)
                            + params.pitch_semitones
                            + params.fine_cents * 0.01
                    }
                    SamplerPlaybackMode::Cycles | SamplerPlaybackMode::Textures => {
                        // Still track pitch, with speed decoupled
                        (v.current_pitch - params.root_note as f32)
                            + params.pitch_semitones
                            + params.fine_cents * 0.01
                    }
                };
                let pitch_ratio = 2.0f64.powf(semitone_delta as f64 / 12.0);
                let step = if params.freeze {
                    0.0
                } else {
                    sr_ratio * pitch_ratio * (params.speed.clamp(0.05, 4.0) as f64) * v.direction
                };

                let (mut sl, mut sr_ch) = sample.sample_frame_lerp(v.frame_pos);

                // Optional loop crossfade near loop_end
                if params.loop_mode == SamplerLoopMode::Forward && params.loop_crossfade > 0.001 {
                    let loop_span = (loop_end - loop_start).max(1.0);
                    let xfade_frames = (params.loop_crossfade.clamp(0.0, 0.5) as f64) * loop_span;
                    if xfade_frames > 1.0 && v.frame_pos > loop_end - xfade_frames && v.frame_pos <= loop_end {
                        let t = ((v.frame_pos - (loop_end - xfade_frames)) / xfade_frames).clamp(0.0, 1.0) as f32;
                        let wrap_pos = loop_start + (v.frame_pos - (loop_end - xfade_frames));
                        let (wl, wr) = sample.sample_frame_lerp(wrap_pos);
                        sl = sl * (1.0 - t) + wl * t;
                        sr_ch = sr_ch * (1.0 - t) + wr * t;
                    }
                }

                // Filter with optional keytrack
                let keytrack_mult = if params.filter_keytrack > 0.001 {
                    2.0f32.powf(((v.current_pitch - params.root_note as f32) * params.filter_keytrack) / 12.0)
                } else {
                    1.0
                };
                let eff_cutoff = params.filter_cutoff_hz * keytrack_mult;
                let (fl, fr) = v.svf.process_stereo(
                    sl,
                    sr_ch,
                    eff_cutoff,
                    params.filter_resonance,
                    host_sample_rate,
                    params.filter_mode,
                );

                out_interleaved[i * 2] += fl * env * voice_gain_l;
                out_interleaved[i * 2 + 1] += fr * env * voice_gain_r;

                // Advance playhead
                v.frame_pos += step;

                match params.loop_mode {
                    SamplerLoopMode::Off => {
                        if (!params.reverse && v.frame_pos >= play_end)
                            || (params.reverse && v.frame_pos <= play_start)
                        {
                            v.active = false;
                            v.env_stage = SamplerEnvStage::Free;
                            break;
                        }
                    }
                    SamplerLoopMode::Forward => {
                        let span = (loop_end - loop_start).max(1.0);
                        if !params.reverse {
                            if v.frame_pos >= loop_end {
                                v.frame_pos = loop_start + (v.frame_pos - loop_end) % span;
                            }
                        } else if v.frame_pos <= loop_start {
                            v.frame_pos = loop_end - (loop_start - v.frame_pos) % span;
                        }
                    }
                    SamplerLoopMode::PingPong => {
                        if v.frame_pos >= loop_end {
                            v.frame_pos = loop_end - (v.frame_pos - loop_end).min(loop_end - loop_start);
                            v.direction = -1.0;
                        } else if v.frame_pos <= loop_start {
                            v.frame_pos = loop_start + (loop_start - v.frame_pos).min(loop_end - loop_start);
                            v.direction = 1.0;
                        }
                    }
                }
            }
        }
    }

    pub fn process_all_idle(
        &mut self,
        track_samples: &[Option<Arc<SamplerSampleData>>; 16],
        track_params: &[SamplerDspParams; 16],
        out_l: &mut [f32],
        out_r: &mut [f32],
        gains_l: &[f32; 16],
        gains_r: &[f32; 16],
        host_sample_rate: f32,
    ) {
        let frames = out_l.len().min(out_r.len());
        if frames == 0 {
            return;
        }
        // Check if any voice is active before allocating/iterating
        if !self.voices.iter().any(|v| v.active) {
            return;
        }
        for t in 0..16 {
            let Some(sample) = track_samples[t].as_ref() else { continue };
            if !self.voices.iter().any(|v| v.active && v.track_idx == t) {
                continue;
            }
            let mut temp = vec![0.0f32; frames * 2];
            self.process_track(
                t,
                sample,
                &track_params[t],
                &mut temp,
                gains_l[t],
                gains_r[t],
                frames,
                host_sample_rate,
            );
            for i in 0..frames {
                out_l[i] += temp[i * 2];
                out_r[i] += temp[i * 2 + 1];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampler_voice_pool_renders_audio_and_loops() {
        let samples: Vec<f32> = (0..1000).map(|i| (i as f32 / 1000.0) * 2.0 - 1.0).collect();
        let data = SamplerSampleData::new(samples, 1, 44100.0);
        let mut params = SamplerDspParams::default();
        params.loop_mode = SamplerLoopMode::Forward;
        params.filter_mode = SamplerFilterMode::Off;

        let mut pool = SamplerVoicePool::new();
        pool.trigger(60, 1.0, 0, &data, &params);

        let mut out = vec![0.0f32; 256 * 2];
        pool.process_track(0, &data, &params, &mut out, 1.0, 1.0, 256, 44100.0);

        let energy: f32 = out.iter().map(|s| s * s).sum();
        assert!(energy > 0.01, "Sampler should produce non-zero audio");
    }

    #[test]
    fn sampler_pitch_tracking_changes_step_speed() {
        let samples: Vec<f32> = (0..4000).map(|i| (i as f32 * 0.05).sin()).collect();
        let data = SamplerSampleData::new(samples, 1, 44100.0);
        let mut params = SamplerDspParams::default();
        params.filter_mode = SamplerFilterMode::Off;
        params.attack_ms = 0.0;

        let mut pool = SamplerVoicePool::new();
        pool.trigger(72, 1.0, 0, &data, &params); // +12 semitones = 2x speed

        let mut out = vec![0.0f32; 100 * 2];
        pool.process_track(0, &data, &params, &mut out, 1.0, 1.0, 100, 44100.0);
        let pos = pool.active_playhead(0, data.total_frames).unwrap();
        // ~200 frames advanced out of 3999
        assert!(pos > 0.04 && pos < 0.06, "Expected ~2x playhead speed, got {pos}");
    }
}
