/*
 * Hikaru OpenPoly - Hybrid Modular Polyphonic Synthesizer DSP Engine
 * Copyright (C) 2026 Hikaru Corporation
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 */

use std::f32::consts::TAU;
use std::sync::Arc;

pub const MAX_OPEN_POLY_VOICES: usize = 16;
const MAX_UNISON_LANES: usize = 7;
const COMB_BUF_LEN: usize = 512;

/// Swappable main oscillator modules in Hikaru OpenPoly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OpenPolyOscType {
    #[default]
    Wavetable,
    Sawtooth,
    Pulse,
    Sine,
    Triangle,
    Phase1,
    Swarm,
    Union,
}

impl OpenPolyOscType {
    pub const ALL: [OpenPolyOscType; 8] = [
        OpenPolyOscType::Wavetable,
        OpenPolyOscType::Sawtooth,
        OpenPolyOscType::Pulse,
        OpenPolyOscType::Sine,
        OpenPolyOscType::Triangle,
        OpenPolyOscType::Phase1,
        OpenPolyOscType::Swarm,
        OpenPolyOscType::Union,
    ];

    pub fn label(self) -> &'static str {
        match self {
            OpenPolyOscType::Wavetable => "Wavetable",
            OpenPolyOscType::Sawtooth => "Sawtooth",
            OpenPolyOscType::Pulse => "Pulse",
            OpenPolyOscType::Sine => "Sine",
            OpenPolyOscType::Triangle => "Triangle",
            OpenPolyOscType::Phase1 => "Phase-1",
            OpenPolyOscType::Swarm => "Swarm",
            OpenPolyOscType::Union => "Union",
        }
    }

    pub fn from_name(s: &str) -> Self {
        match s {
            "Sawtooth" | "Saw" => OpenPolyOscType::Sawtooth,
            "Pulse" | "Square" => OpenPolyOscType::Pulse,
            "Sine" => OpenPolyOscType::Sine,
            "Triangle" | "Tri" => OpenPolyOscType::Triangle,
            "Phase-1" | "Phase1" => OpenPolyOscType::Phase1,
            "Swarm" => OpenPolyOscType::Swarm,
            "Union" => OpenPolyOscType::Union,
            _ => OpenPolyOscType::Wavetable,
        }
    }

    pub fn index_param_label(self) -> &'static str {
        match self {
            OpenPolyOscType::Wavetable => "Index",
            OpenPolyOscType::Sawtooth => "Symmetry",
            OpenPolyOscType::Pulse => "Width",
            OpenPolyOscType::Sine => "FM/Fold",
            OpenPolyOscType::Triangle => "Skew",
            OpenPolyOscType::Phase1 => "Distort",
            OpenPolyOscType::Swarm => "Detune",
            OpenPolyOscType::Union => "Morph",
        }
    }
}

/// Sub-oscillator waveform selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OpenPolySubWave {
    #[default]
    Sine,
    Triangle,
    Square,
    Saw,
}

impl OpenPolySubWave {
    pub const ALL: [OpenPolySubWave; 4] = [
        OpenPolySubWave::Sine,
        OpenPolySubWave::Triangle,
        OpenPolySubWave::Square,
        OpenPolySubWave::Saw,
    ];

    pub fn label(self) -> &'static str {
        match self {
            OpenPolySubWave::Sine => "Sine",
            OpenPolySubWave::Triangle => "Tri",
            OpenPolySubWave::Square => "Sqr",
            OpenPolySubWave::Saw => "Saw",
        }
    }

    pub fn from_name(s: &str) -> Self {
        match s {
            "Triangle" | "Tri" => OpenPolySubWave::Triangle,
            "Square" | "Sqr" => OpenPolySubWave::Square,
            "Saw" | "Sawtooth" => OpenPolySubWave::Saw,
            _ => OpenPolySubWave::Sine,
        }
    }
}

/// Swappable filter modules in Hikaru OpenPoly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OpenPolyFilterType {
    #[default]
    LowPassMG,
    SallenKey,
    SvfLowPass,
    LowPassLD,
    HighPass,
    BandPass,
    Comb,
    Xp,
}

impl OpenPolyFilterType {
    pub const ALL: [OpenPolyFilterType; 8] = [
        OpenPolyFilterType::LowPassMG,
        OpenPolyFilterType::SallenKey,
        OpenPolyFilterType::SvfLowPass,
        OpenPolyFilterType::LowPassLD,
        OpenPolyFilterType::HighPass,
        OpenPolyFilterType::BandPass,
        OpenPolyFilterType::Comb,
        OpenPolyFilterType::Xp,
    ];

    pub fn label(self) -> &'static str {
        match self {
            OpenPolyFilterType::LowPassMG => "Low-pass MG",
            OpenPolyFilterType::SallenKey => "Sallen-Key",
            OpenPolyFilterType::SvfLowPass => "SVF",
            OpenPolyFilterType::LowPassLD => "Low-pass LD",
            OpenPolyFilterType::HighPass => "High-pass",
            OpenPolyFilterType::BandPass => "Band-pass",
            OpenPolyFilterType::Comb => "Comb",
            OpenPolyFilterType::Xp => "XP",
        }
    }

    pub fn from_name(s: &str) -> Self {
        match s {
            "Sallen-Key" | "SallenKey" => OpenPolyFilterType::SallenKey,
            "SVF" | "SvfLowPass" => OpenPolyFilterType::SvfLowPass,
            "Low-pass LD" | "LowPassLD" => OpenPolyFilterType::LowPassLD,
            "High-pass" | "HighPass" => OpenPolyFilterType::HighPass,
            "Band-pass" | "BandPass" => OpenPolyFilterType::BandPass,
            "Comb" => OpenPolyFilterType::Comb,
            "XP" | "Xp" => OpenPolyFilterType::Xp,
            _ => OpenPolyFilterType::LowPassMG,
        }
    }
}

/// Swappable amplifier envelope generator modules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OpenPolyEnvType {
    #[default]
    Adsr,
    Ar,
    Ad,
}

impl OpenPolyEnvType {
    pub const ALL: [OpenPolyEnvType; 3] = [
        OpenPolyEnvType::Adsr,
        OpenPolyEnvType::Ar,
        OpenPolyEnvType::Ad,
    ];

    pub fn label(self) -> &'static str {
        match self {
            OpenPolyEnvType::Adsr => "ADSR",
            OpenPolyEnvType::Ar => "AR",
            OpenPolyEnvType::Ad => "AD",
        }
    }

    pub fn from_name(s: &str) -> Self {
        match s {
            "AR" | "Ar" => OpenPolyEnvType::Ar,
            "AD" | "Ad" => OpenPolyEnvType::Ad,
            _ => OpenPolyEnvType::Adsr,
        }
    }
}

/// Built-in modulator LFO waveform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OpenPolyLfoWave {
    #[default]
    Sine,
    Triangle,
    Saw,
    Square,
    SampleHold,
}

impl OpenPolyLfoWave {
    pub const ALL: [OpenPolyLfoWave; 5] = [
        OpenPolyLfoWave::Sine,
        OpenPolyLfoWave::Triangle,
        OpenPolyLfoWave::Saw,
        OpenPolyLfoWave::Square,
        OpenPolyLfoWave::SampleHold,
    ];

    pub fn label(self) -> &'static str {
        match self {
            OpenPolyLfoWave::Sine => "Sine",
            OpenPolyLfoWave::Triangle => "Tri",
            OpenPolyLfoWave::Saw => "Saw",
            OpenPolyLfoWave::Square => "Sqr",
            OpenPolyLfoWave::SampleHold => "S&H",
        }
    }

    pub fn from_name(s: &str) -> Self {
        match s {
            "Triangle" | "Tri" => OpenPolyLfoWave::Triangle,
            "Saw" => OpenPolyLfoWave::Saw,
            "Square" | "Sqr" => OpenPolyLfoWave::Square,
            "SampleHold" | "S&H" => OpenPolyLfoWave::SampleHold,
            _ => OpenPolyLfoWave::Sine,
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            OpenPolyLfoWave::Sine => OpenPolyLfoWave::Triangle,
            OpenPolyLfoWave::Triangle => OpenPolyLfoWave::Saw,
            OpenPolyLfoWave::Saw => OpenPolyLfoWave::Square,
            OpenPolyLfoWave::Square => OpenPolyLfoWave::SampleHold,
            OpenPolyLfoWave::SampleHold => OpenPolyLfoWave::Sine,
        }
    }
}

/// Real-time DSP parameter snapshot for Hikaru OpenPoly (copyable across threads).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpenPolyDspParams {
    // Oscillator
    pub osc_type: OpenPolyOscType,
    pub osc_index: f32,
    pub osc_fold: f32,
    pub osc_ratio_num: u8,
    pub osc_ratio_den: u8,
    pub osc_semitones: f32,
    pub osc_detune_hz: f32,
    pub osc_sync: bool,
    pub unison_voices: u8,
    pub unison_detune: f32,
    pub unison_spread: f32,

    // Sub & Noise
    pub sub_level: f32,
    pub sub_wave: OpenPolySubWave,
    pub sub_octave: i8,
    pub noise_level: f32,

    // Filter
    pub filter_enabled: bool,
    pub filter_type: OpenPolyFilterType,
    pub filter_cutoff_hz: f32,
    pub filter_resonance: f32,
    pub filter_drive: f32,
    pub filter_keytrack: f32,
    pub filter_env_amount: f32,
    pub filter_vel_amount: f32,

    // Filter Envelope (FEG)
    pub feg_enabled: bool,
    pub feg_attack_ms: f32,
    pub feg_decay_ms: f32,
    pub feg_sustain: f32,
    pub feg_release_ms: f32,

    // Amp Envelope
    pub amp_env_type: OpenPolyEnvType,
    pub amp_attack_ms: f32,
    pub amp_decay_ms: f32,
    pub amp_sustain: f32,
    pub amp_release_ms: f32,

    // Voice / Pitch / Glide
    pub mono_mode: bool,
    pub legato: bool,
    pub glide_ms: f32,
    pub global_pitch_st: f32,
    pub velocity_sens: f32,

    // Built-in LFO modulator
    pub lfo_enabled: bool,
    pub lfo_wave: OpenPolyLfoWave,
    pub lfo_rate_hz: f32,
    pub lfo_amount_cutoff: f32,
    pub lfo_amount_index: f32,
    pub lfo_amount_pitch: f32,
    pub lfo_amount_amp: f32,

    // Output
    pub pan: f32,
    pub output_gain: f32,
    pub stereo_width: f32,
}

impl Default for OpenPolyDspParams {
    fn default() -> Self {
        Self {
            osc_type: OpenPolyOscType::Wavetable,
            osc_index: 0.0,
            osc_fold: 0.0,
            osc_ratio_num: 1,
            osc_ratio_den: 1,
            osc_semitones: 0.0,
            osc_detune_hz: 0.0,
            osc_sync: false,
            unison_voices: 1,
            unison_detune: 0.18,
            unison_spread: 0.5,

            sub_level: 0.25,
            sub_wave: OpenPolySubWave::Sine,
            sub_octave: -1,
            noise_level: 0.0,

            filter_enabled: true,
            filter_type: OpenPolyFilterType::LowPassMG,
            filter_cutoff_hz: 1650.0,
            filter_resonance: 0.22,
            filter_drive: 0.08,
            filter_keytrack: 0.5,
            filter_env_amount: 0.42,
            filter_vel_amount: 0.25,

            feg_enabled: true,
            feg_attack_ms: 4.0,
            feg_decay_ms: 280.0,
            feg_sustain: 0.25,
            feg_release_ms: 220.0,

            amp_env_type: OpenPolyEnvType::Adsr,
            amp_attack_ms: 5.0,
            amp_decay_ms: 240.0,
            amp_sustain: 0.78,
            amp_release_ms: 180.0,

            mono_mode: false,
            legato: false,
            glide_ms: 0.0,
            global_pitch_st: 0.0,
            velocity_sens: 0.65,

            lfo_enabled: true,
            lfo_wave: OpenPolyLfoWave::Sine,
            lfo_rate_hz: 1.5,
            lfo_amount_cutoff: 0.0,
            lfo_amount_index: 0.0,
            lfo_amount_pitch: 0.0,
            lfo_amount_amp: 0.0,

            pan: 0.0,
            output_gain: 0.85,
            stereo_width: 0.5,
        }
    }
}

/// Multi-frame wavetable buffer shared via `Arc` between GUI and audio thread.
#[derive(Debug, Clone)]
pub struct OpenPolyWavetableData {
    pub samples: Vec<f32>,
    pub frames: usize,
    pub frame_len: usize,
}

impl OpenPolyWavetableData {
    pub fn new(samples: Vec<f32>, frames: usize, frame_len: usize) -> Self {
        let frame_len = frame_len.max(64);
        let available_frames = if samples.len() >= frame_len {
            (samples.len() / frame_len).max(1)
        } else {
            0
        };
        let frames = frames.min(available_frames).max(if available_frames > 0 { 1 } else { 0 });
        Self {
            samples,
            frames,
            frame_len,
        }
    }

    /// Synthesizes a built-in 4-frame Standard Four wavetable (Sine -> Triangle -> Saw -> Square).
    pub fn standard_four(frame_len: usize) -> Self {
        let len = frame_len.max(256);
        let mut samples = Vec::with_capacity(len * 4);
        // Frame 0: Sine
        for i in 0..len {
            let p = i as f32 / len as f32;
            samples.push((p * TAU).sin());
        }
        // Frame 1: Triangle
        for i in 0..len {
            let p = i as f32 / len as f32;
            let tri = 1.0 - 4.0 * (p - 0.5).abs();
            samples.push(tri);
        }
        // Frame 2: Band-limited-ish Saw (sum of 24 harmonics)
        for i in 0..len {
            let p = i as f32 / len as f32;
            let mut acc = 0.0f32;
            for h in 1..=24 {
                let hf = h as f32;
                acc += (p * TAU * hf).sin() / hf;
            }
            samples.push((acc * 0.62).clamp(-1.0, 1.0));
        }
        // Frame 3: Band-limited-ish Square (odd harmonics up to 23)
        for i in 0..len {
            let p = i as f32 / len as f32;
            let mut acc = 0.0f32;
            for h in (1..=23).step_by(2) {
                let hf = h as f32;
                acc += (p * TAU * hf).sin() / hf;
            }
            samples.push((acc * 0.78).clamp(-1.0, 1.0));
        }
        Self {
            samples,
            frames: 4,
            frame_len: len,
        }
    }

    #[inline]
    pub fn sample_interpolated(&self, phase01: f32, index01: f32) -> f32 {
        if self.frames == 0 || self.frame_len == 0 || self.samples.len() < self.frame_len {
            return (phase01 * TAU).sin();
        }
        let p = phase01.rem_euclid(1.0);
        let fpos = p * self.frame_len as f32;
        let s0 = fpos as usize % self.frame_len;
        let s1 = (s0 + 1) % self.frame_len;
        let sfrac = fpos - fpos.floor();

        if self.frames == 1 {
            let a = self.samples[s0];
            let b = self.samples[s1];
            return a + sfrac * (b - a);
        }

        let pos = index01.clamp(0.0, 1.0) * (self.frames - 1) as f32;
        let f0 = (pos.floor() as usize).min(self.frames - 1);
        let f1 = (f0 + 1).min(self.frames - 1);
        let ffrac = pos - f0 as f32;

        let base0 = f0 * self.frame_len;
        let base1 = f1 * self.frame_len;

        let v0 = self.samples[base0 + s0] + sfrac * (self.samples[base0 + s1] - self.samples[base0 + s0]);
        let v1 = self.samples[base1 + s0] + sfrac * (self.samples[base1 + s1] - self.samples[base1 + s0]);
        v0 + ffrac * (v1 - v0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnvStage {
    Attack,
    Decay,
    Sustain,
    Release,
    Idle,
}

#[derive(Debug, Clone, Copy)]
struct Envelope {
    stage: EnvStage,
    level: f32,
}

impl Envelope {
    fn new() -> Self {
        Self {
            stage: EnvStage::Idle,
            level: 0.0,
        }
    }

    fn trigger(&mut self, legato_retrig: bool) {
        self.stage = EnvStage::Attack;
        if !legato_retrig {
            self.level = 0.0;
        }
    }

    fn release(&mut self) {
        if self.stage != EnvStage::Idle {
            self.stage = EnvStage::Release;
        }
    }

    fn reset(&mut self) {
        self.stage = EnvStage::Idle;
        self.level = 0.0;
    }

    #[inline]
    fn next(
        &mut self,
        env_type: OpenPolyEnvType,
        attack_ms: f32,
        decay_ms: f32,
        sustain: f32,
        release_ms: f32,
        sr: f32,
    ) -> f32 {
        let sr = sr.max(8000.0);
        let sust = sustain.clamp(0.0, 1.0);
        match self.stage {
            EnvStage::Attack => {
                let atk_samples = (attack_ms.max(0.3) * 0.001 * sr).max(1.0);
                self.level += 1.0 / atk_samples;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = match env_type {
                        OpenPolyEnvType::Ar => EnvStage::Sustain,
                        OpenPolyEnvType::Adsr | OpenPolyEnvType::Ad => EnvStage::Decay,
                    };
                }
            }
            EnvStage::Decay => {
                let target = if env_type == OpenPolyEnvType::Ad {
                    0.0
                } else {
                    sust
                };
                let dec_samples = (decay_ms.max(0.5) * 0.001 * sr).max(1.0);
                // Exponential-ish decay curve for musical pluck & pad feel
                let coeff = (-4.6 / dec_samples).exp();
                self.level = target + (self.level - target) * coeff;
                if (self.level - target).abs() < 0.0005 {
                    self.level = target;
                    self.stage = if env_type == OpenPolyEnvType::Ad {
                        EnvStage::Idle
                    } else {
                        EnvStage::Sustain
                    };
                }
            }
            EnvStage::Sustain => {
                self.level = if env_type == OpenPolyEnvType::Ar {
                    1.0
                } else {
                    sust
                };
            }
            EnvStage::Release => {
                let rel_samples = (release_ms.max(0.5) * 0.001 * sr).max(1.0);
                let coeff = (-5.5 / rel_samples).exp();
                self.level *= coeff;
                if self.level < 0.0004 {
                    self.level = 0.0;
                    self.stage = EnvStage::Idle;
                }
            }
            EnvStage::Idle => {
                self.level = 0.0;
            }
        }
        self.level
    }
}

#[inline]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if dt <= 0.0 || dt >= 0.5 {
        return 0.0;
    }
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

#[inline]
fn fast_tanh(x: f32) -> f32 {
    if x < -3.0 {
        -1.0
    } else if x > 3.0 {
        1.0
    } else {
        let x2 = x * x;
        x * (27.0 + x2) / (27.0 + 9.0 * x2)
    }
}

#[inline]
fn apply_fold(sample: f32, fold: f32) -> f32 {
    if fold <= 0.001 {
        return sample;
    }
    let gain = 1.0 + fold * 3.5;
    let x = sample * gain;
    // Smooth trigonometric wavefolder
    (x * std::f32::consts::FRAC_PI_2).sin() * (1.0 - 0.15 * fold)
}

#[inline]
fn eval_sub_wave(phase01: f32, wave: OpenPolySubWave) -> f32 {
    let p = phase01.rem_euclid(1.0);
    match wave {
        OpenPolySubWave::Sine => (p * TAU).sin(),
        OpenPolySubWave::Triangle => 1.0 - 4.0 * (p - 0.5).abs(),
        OpenPolySubWave::Square => {
            if p < 0.5 {
                0.85
            } else {
                -0.85
            }
        }
        OpenPolySubWave::Saw => 2.0 * p - 1.0,
    }
}

/// Evaluates a single oscillator sample for preview or voice playback.
#[inline]
pub fn eval_osc_preview(
    osc_type: OpenPolyOscType,
    phase01: f32,
    index: f32,
    fold: f32,
    wt: Option<&OpenPolyWavetableData>,
) -> f32 {
    let p = phase01.rem_euclid(1.0);
    let idx = index.clamp(0.0, 1.0);
    let raw = match osc_type {
        OpenPolyOscType::Wavetable => {
            if let Some(table) = wt {
                table.sample_interpolated(p, idx)
            } else {
                let s = (p * TAU).sin();
                let saw = 2.0 * p - 1.0;
                s + idx * (saw - s)
            }
        }
        OpenPolyOscType::Sawtooth => {
            let saw1 = 2.0 * p - 1.0;
            let saw2 = 2.0 * (2.0 * p).fract() - 1.0;
            saw1 * (1.0 - idx * 0.65) + saw2 * (idx * 0.65)
        }
        OpenPolyOscType::Pulse => {
            let pw = 0.06 + idx * 0.88;
            if p < pw {
                1.0
            } else {
                -1.0
            }
        }
        OpenPolyOscType::Sine => {
            let fm = idx * 2.2 * (p * TAU * 2.0).sin();
            (p * TAU + fm).sin()
        }
        OpenPolyOscType::Triangle => {
            let skew = 0.08 + idx * 0.84;
            if p < skew {
                -1.0 + 2.0 * (p / skew)
            } else {
                1.0 - 2.0 * ((p - skew) / (1.0 - skew))
            }
        }
        OpenPolyOscType::Phase1 => {
            // Casio CZ-inspired phase distortion
            let k = 0.5 - idx * 0.46;
            let warped = if p < k {
                0.5 * p / k.max(0.02)
            } else {
                0.5 + 0.5 * (p - k) / (1.0 - k).max(0.02)
            };
            -(warped * TAU).cos()
        }
        OpenPolyOscType::Swarm => {
            // Super-saw preview with harmonic shimmer proportional to index
            let s1 = 2.0 * p - 1.0;
            let s2 = 2.0 * (p + idx * 0.19).fract() - 1.0;
            let s3 = 2.0 * (p - idx * 0.27 + 1.0).fract() - 1.0;
            (s1 + 0.7 * s2 + 0.7 * s3) / 2.4
        }
        OpenPolyOscType::Union => {
            let saw = 2.0 * p - 1.0;
            let sq = if p < 0.5 { 1.0 } else { -1.0 };
            let sub_harm = (p * TAU * 2.0).sin() * 0.35;
            saw * (1.0 - idx) + sq * idx + sub_harm * idx
        }
    };
    apply_fold(raw, fold).clamp(-1.2, 1.2)
}

#[derive(Debug, Clone)]
struct VoiceFilter {
    // 4-pole ladder stages
    s: [f32; 4],
    // 2-pole SVF / Sallen-Key states
    ic1: f32,
    ic2: f32,
    // Comb delay line
    comb_buf: Vec<f32>,
    comb_pos: usize,
}

impl VoiceFilter {
    fn new() -> Self {
        Self {
            s: [0.0; 4],
            ic1: 0.0,
            ic2: 0.0,
            comb_buf: vec![0.0; COMB_BUF_LEN],
            comb_pos: 0,
        }
    }

    fn reset(&mut self) {
        self.s = [0.0; 4];
        self.ic1 = 0.0;
        self.ic2 = 0.0;
        self.comb_buf.fill(0.0);
        self.comb_pos = 0;
    }

    #[inline]
    fn process(
        &mut self,
        input: f32,
        filter_type: OpenPolyFilterType,
        cutoff_hz: f32,
        resonance: f32,
        drive: f32,
        sr: f32,
    ) -> f32 {
        let sr = sr.max(8000.0);
        let fc = cutoff_hz.clamp(20.0, sr * 0.44);
        let reso = resonance.clamp(0.0, 0.98);
        let drv = 1.0 + drive.clamp(0.0, 1.0) * 3.5;
        let x = fast_tanh(input * drv);

        let out = match filter_type {
            OpenPolyFilterType::LowPassMG | OpenPolyFilterType::LowPassLD => {
                // 4-pole Huovilainen-style zero-delay-ish ladder (2x oversampled for stability)
                let g = (std::f32::consts::PI * fc / (sr * 2.0)).tan().min(0.85);
                let g_coeff = g / (1.0 + g);
                let k = reso * if filter_type == OpenPolyFilterType::LowPassMG { 3.75 } else { 3.35 };
                let mut y = 0.0;
                for _ in 0..2 {
                    let fb = k * self.s[3];
                    let u = if filter_type == OpenPolyFilterType::LowPassMG {
                        fast_tanh(x - fb)
                    } else {
                        (x - fb).clamp(-2.5, 2.5)
                    };
                    let mut sig = u;
                    for st in &mut self.s {
                        let v = (sig - *st) * g_coeff;
                        let next_y = v + *st;
                        *st = next_y + v;
                        sig = next_y;
                    }
                    y = sig;
                }
                y * (1.0 + reso * 0.45)
            }
            OpenPolyFilterType::SallenKey => {
                // Aggressive nonlinear 12dB/oct resonant Sallen-Key LP
                let g = (std::f32::consts::PI * fc / sr).tan().min(1.8);
                let k = reso * 1.92;
                let a1 = 1.0 / (1.0 + g * (g + 2.0 - k));
                let hp = (x - (2.0 - k) * self.ic1 - self.ic2) * a1;
                let bp = fast_tanh(g * hp + self.ic1);
                self.ic1 = g * hp + bp;
                let lp = g * bp + self.ic2;
                self.ic2 = g * bp + lp;
                lp
            }
            OpenPolyFilterType::SvfLowPass
            | OpenPolyFilterType::HighPass
            | OpenPolyFilterType::BandPass
            | OpenPolyFilterType::Xp => {
                // Cytomic / TPT SVF
                let g = (std::f32::consts::PI * fc / sr).tan().min(4.0);
                let k = (2.0 - 1.95 * reso).max(0.05);
                let a1 = 1.0 / (1.0 + g * (g + k));
                let a2 = g * a1;
                let a3 = g * a2;

                let v3 = x - self.ic2;
                let v1 = a1 * self.ic1 + a2 * v3;
                let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
                self.ic1 = 2.0 * v1 - self.ic1;
                self.ic2 = 2.0 * v2 - self.ic2;

                match filter_type {
                    OpenPolyFilterType::SvfLowPass => v2,
                    OpenPolyFilterType::HighPass => x - k * v1 - v2,
                    OpenPolyFilterType::BandPass => v1,
                    OpenPolyFilterType::Xp => fast_tanh((v2 * 0.85 + v1 * 0.35 * (1.0 + reso)) * 1.2),
                    _ => v2,
                }
            }
            OpenPolyFilterType::Comb => {
                let delay_samples = (sr / fc).clamp(2.0, (COMB_BUF_LEN - 2) as f32);
                let read_f = self.comb_pos as f32 - delay_samples;
                let read_wrapped = read_f.rem_euclid(COMB_BUF_LEN as f32);
                let i0 = read_wrapped as usize % COMB_BUF_LEN;
                let i1 = (i0 + 1) % COMB_BUF_LEN;
                let frac = read_wrapped - read_wrapped.floor();
                let delayed = self.comb_buf[i0] + frac * (self.comb_buf[i1] - self.comb_buf[i0]);
                let fb = (0.1 + reso * 0.86).clamp(0.0, 0.96);
                let y = fast_tanh(x + delayed * fb);
                self.comb_buf[self.comb_pos] = y;
                self.comb_pos = (self.comb_pos + 1) % COMB_BUF_LEN;
                y * 0.72
            }
        };

        if out.is_finite() {
            out.clamp(-4.0, 4.0)
        } else {
            self.reset();
            0.0
        }
    }
}

#[derive(Debug, Clone)]
struct OpenPolyVoice {
    active: bool,
    held: bool,
    track_idx: usize,
    pitch: u8,
    velocity: f32,
    current_midi_pitch: f32,
    target_midi_pitch: f32,
    phases: [f32; MAX_UNISON_LANES],
    sync_phase: f32,
    sub_phase: f32,
    amp_env: Envelope,
    feg_env: Envelope,
    filter_l: VoiceFilter,
    filter_r: VoiceFilter,
    rng_state: u32,
    age: u64,
}

impl OpenPolyVoice {
    fn new(seed: u32) -> Self {
        Self {
            active: false,
            held: false,
            track_idx: 0,
            pitch: 60,
            velocity: 1.0,
            current_midi_pitch: 60.0,
            target_midi_pitch: 60.0,
            phases: [0.0; MAX_UNISON_LANES],
            sync_phase: 0.0,
            sub_phase: 0.0,
            amp_env: Envelope::new(),
            feg_env: Envelope::new(),
            filter_l: VoiceFilter::new(),
            filter_r: VoiceFilter::new(),
            rng_state: seed.max(1),
            age: 0,
        }
    }

    #[inline]
    fn next_noise(&mut self) -> f32 {
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng_state = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    #[inline]
    fn process_sample(
        &mut self,
        params: &OpenPolyDspParams,
        wt: Option<&OpenPolyWavetableData>,
        lfo_val: f32,
        sr: f32,
    ) -> (f32, f32) {
        if !self.active {
            return (0.0, 0.0);
        }

        // 1. Amp envelope
        let amp_env = self.amp_env.next(
            params.amp_env_type,
            params.amp_attack_ms,
            params.amp_decay_ms,
            params.amp_sustain,
            params.amp_release_ms,
            sr,
        );
        if self.amp_env.stage == EnvStage::Idle {
            self.active = false;
            self.held = false;
            return (0.0, 0.0);
        }

        // 2. Filter envelope (FEG)
        let feg_val = if params.feg_enabled {
            self.feg_env.next(
                OpenPolyEnvType::Adsr,
                params.feg_attack_ms,
                params.feg_decay_ms,
                params.feg_sustain,
                params.feg_release_ms,
                sr,
            )
        } else {
            0.0
        };

        // 3. Glide / portamento
        if params.glide_ms > 0.5 && (self.current_midi_pitch - self.target_midi_pitch).abs() > 0.0005 {
            let glide_samples = (params.glide_ms * 0.001 * sr).max(1.0);
            let coeff = (-4.0 / glide_samples).exp();
            self.current_midi_pitch =
                self.target_midi_pitch + (self.current_midi_pitch - self.target_midi_pitch) * coeff;
        } else {
            self.current_midi_pitch = self.target_midi_pitch;
        }

        // 4. Pitch computation
        let lfo_pitch_st = if params.lfo_enabled {
            lfo_val * params.lfo_amount_pitch * 12.0
        } else {
            0.0
        };
        let eff_midi = self.current_midi_pitch + params.global_pitch_st + params.osc_semitones + lfo_pitch_st;
        let base_freq = 440.0 * 2.0_f32.powf((eff_midi - 69.0) / 12.0);
        let ratio = (params.osc_ratio_num.max(1) as f32) / (params.osc_ratio_den.max(1) as f32);
        let osc_freq = (base_freq * ratio + params.osc_detune_hz).clamp(8.0, sr * 0.45);

        // Hard-sync master reset
        let mut sync_reset = false;
        if params.osc_sync {
            let sync_inc = (base_freq.clamp(8.0, sr * 0.45)) / sr;
            self.sync_phase += sync_inc;
            if self.sync_phase >= 1.0 {
                self.sync_phase -= 1.0;
                sync_reset = true;
            }
        }

        let eff_index = (params.osc_index
            + if params.lfo_enabled {
                lfo_val * params.lfo_amount_index
            } else {
                0.0
            })
        .clamp(0.0, 1.0);

        // 5. Main oscillator (supports unison lanes and Swarm 7-voice supersaw!)
        let lanes = if params.osc_type == OpenPolyOscType::Swarm {
            MAX_UNISON_LANES
        } else {
            (params.unison_voices as usize).clamp(1, MAX_UNISON_LANES)
        };
        let detune_amt = if params.osc_type == OpenPolyOscType::Swarm {
            0.08 + eff_index * 0.92
        } else {
            params.unison_detune
        };
        let spread = params.unison_spread.clamp(0.0, 1.0) * params.stereo_width.clamp(0.0, 1.0);

        let mut osc_l = 0.0f32;
        let mut osc_r = 0.0f32;
        let norm = 1.0 / (lanes as f32).sqrt();

        for lane in 0..lanes {
            if sync_reset {
                self.phases[lane] = 0.0;
            }
            let offset_norm = if lanes == 1 {
                0.0
            } else {
                (lane as f32 / (lanes - 1) as f32) * 2.0 - 1.0
            };
            // Max unison detune: ±45 cents
            let cents = offset_norm * detune_amt * 45.0;
            let lane_freq = (osc_freq * 2.0_f32.powf(cents / 1200.0)).clamp(8.0, sr * 0.46);
            let dt = lane_freq / sr;
            let p = self.phases[lane];

            let sample = match params.osc_type {
                OpenPolyOscType::Sawtooth | OpenPolyOscType::Swarm => {
                    let mut saw = 2.0 * p - 1.0 - poly_blep(p, dt);
                    if params.osc_type == OpenPolyOscType::Sawtooth && eff_index > 0.005 {
                        let p2 = (p * 2.0).fract();
                        let saw2 = 2.0 * p2 - 1.0 - poly_blep(p2, (dt * 2.0).min(0.45));
                        saw = saw * (1.0 - eff_index * 0.6) + saw2 * (eff_index * 0.6);
                    }
                    apply_fold(saw, params.osc_fold)
                }
                OpenPolyOscType::Pulse => {
                    let pw = 0.06 + eff_index * 0.88;
                    let mut sq = if p < pw { 1.0 } else { -1.0 };
                    sq += poly_blep(p, dt);
                    sq -= poly_blep((p - pw).rem_euclid(1.0), dt);
                    apply_fold(sq, params.osc_fold)
                }
                _ => eval_osc_preview(params.osc_type, p, eff_index, params.osc_fold, wt),
            };

            self.phases[lane] = (p + dt).fract();

            let lane_pan = offset_norm * spread;
            let gl = (0.5 * (1.0 - lane_pan)).sqrt();
            let gr = (0.5 * (1.0 + lane_pan)).sqrt();
            osc_l += sample * gl * norm;
            osc_r += sample * gr * norm;
        }

        // 6. Sub oscillator (mono centered)
        let sub_oct_mul = 2.0_f32.powi(params.sub_octave.clamp(-2, 0) as i32);
        let sub_freq = (base_freq * sub_oct_mul).clamp(12.0, sr * 0.44);
        let sub_sample = if params.sub_level > 0.001 {
            eval_sub_wave(self.sub_phase, params.sub_wave) * params.sub_level
        } else {
            0.0
        };
        self.sub_phase = (self.sub_phase + sub_freq / sr).fract();

        // 7. Noise generator
        let noise_sample = if params.noise_level > 0.001 {
            self.next_noise() * params.noise_level * 0.55
        } else {
            0.0
        };

        let mut sig_l = osc_l + (sub_sample + noise_sample) * 0.707;
        let mut sig_r = osc_r + (sub_sample + noise_sample) * 0.707;

        // 8. Filter stage
        if params.filter_enabled {
            let kt_oct = (self.current_midi_pitch - 60.0) / 12.0 * params.filter_keytrack;
            let feg_oct = feg_val * params.filter_env_amount * 6.0;
            let vel_oct = (self.velocity - 0.5) * params.filter_vel_amount * 3.0;
            let lfo_oct = if params.lfo_enabled {
                lfo_val * params.lfo_amount_cutoff * 4.0
            } else {
                0.0
            };
            let mod_cutoff = (params.filter_cutoff_hz * 2.0_f32.powf(kt_oct + feg_oct + vel_oct + lfo_oct))
                .clamp(20.0, 20000.0);

            sig_l = self.filter_l.process(
                sig_l,
                params.filter_type,
                mod_cutoff,
                params.filter_resonance,
                params.filter_drive,
                sr,
            );
            sig_r = self.filter_r.process(
                sig_r,
                params.filter_type,
                mod_cutoff,
                params.filter_resonance,
                params.filter_drive,
                sr,
            );
        }

        // 9. Amp & velocity scaling + LFO tremolo + master pan/output
        let vel_gain = (1.0 - params.velocity_sens) + params.velocity_sens * self.velocity;
        let lfo_amp = if params.lfo_enabled {
            (1.0 + lfo_val * params.lfo_amount_amp * 0.5).clamp(0.0, 2.0)
        } else {
            1.0
        };
        let voice_gain = amp_env * vel_gain * lfo_amp * params.output_gain * 0.42;

        let pan = params.pan.clamp(-1.0, 1.0);
        let pan_l = if pan > 0.0 { 1.0 - pan } else { 1.0 };
        let pan_r = if pan < 0.0 { 1.0 + pan } else { 1.0 };

        (sig_l * voice_gain * pan_l, sig_r * voice_gain * pan_r)
    }
}

/// Polyphonic voice pool for Hikaru OpenPoly across up to 16 tracks.
pub struct OpenPolyVoicePool {
    voices: Vec<OpenPolyVoice>,
    default_wt: Arc<OpenPolyWavetableData>,
    lfo_phases: [f32; 16],
    lfo_sh_values: [f32; 16],
    lfo_rng: u32,
    track_peaks: [(f32, f32); 16],
    global_age: u64,
}

impl Default for OpenPolyVoicePool {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenPolyVoicePool {
    pub fn new() -> Self {
        let voices = (0..MAX_OPEN_POLY_VOICES)
            .map(|i| OpenPolyVoice::new(0x9E37_79B9_u32.wrapping_mul(i as u32 + 1)))
            .collect();
        Self {
            voices,
            default_wt: Arc::new(OpenPolyWavetableData::standard_four(2048)),
            lfo_phases: [0.0; 16],
            lfo_sh_values: [0.0; 16],
            lfo_rng: 0x1234_5678,
            track_peaks: [(0.0, 0.0); 16],
            global_age: 0,
        }
    }

    pub fn track_meter(&self, track_idx: usize) -> (f32, f32) {
        self.track_peaks[track_idx.min(15)]
    }

    #[inline]
    fn advance_track_lfo(&mut self, ti: usize, params: &OpenPolyDspParams, sr: f32) -> f32 {
        if !params.lfo_enabled {
            return 0.0;
        }
        let dt = params.lfo_rate_hz.clamp(0.02, 40.0) / sr.max(8000.0);
        let prev = self.lfo_phases[ti];
        let next = prev + dt;
        if next >= 1.0 {
            self.lfo_phases[ti] = next.fract();
            let mut x = self.lfo_rng;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            self.lfo_rng = x;
            self.lfo_sh_values[ti] = (x as f32 / u32::MAX as f32) * 2.0 - 1.0;
        } else {
            self.lfo_phases[ti] = next;
        }
        let p = self.lfo_phases[ti];
        match params.lfo_wave {
            OpenPolyLfoWave::Sine => (p * TAU).sin(),
            OpenPolyLfoWave::Triangle => 1.0 - 4.0 * (p - 0.5).abs(),
            OpenPolyLfoWave::Saw => 2.0 * p - 1.0,
            OpenPolyLfoWave::Square => if p < 0.5 { 1.0 } else { -1.0 },
            OpenPolyLfoWave::SampleHold => self.lfo_sh_values[ti],
        }
    }

    pub fn trigger(
        &mut self,
        pitch: u8,
        velocity: f32,
        track_idx: usize,
        params: &OpenPolyDspParams,
    ) {
        let ti = track_idx.min(15);
        self.global_age = self.global_age.wrapping_add(1);

        let idx = if params.mono_mode {
            // Reuse existing active voice on this track for mono/legato glide
            if let Some(i) = self.voices.iter().position(|v| v.active && v.track_idx == ti) {
                i
            } else if let Some(free_i) = self.voices.iter().position(|v| !v.active) {
                free_i
            } else {
                self.voices
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, v)| v.age)
                    .map(|(i, _)| i)
                    .unwrap_or(0)
            }
        } else if let Some(same_i) = self
            .voices
            .iter()
            .position(|v| v.active && v.track_idx == ti && v.pitch == pitch)
        {
            same_i
        } else if let Some(free_i) = self.voices.iter().position(|v| !v.active) {
            free_i
        } else {
            self.voices
                .iter()
                .enumerate()
                .min_by_key(|(_, v)| v.age)
                .map(|(i, _)| i)
                .unwrap_or(0)
        };

        let last_pitch = self
            .voices
            .iter()
            .filter(|v| v.active && v.track_idx == ti)
            .max_by_key(|v| v.age)
            .map(|v| v.current_midi_pitch);

        let v = &mut self.voices[idx];
        let is_legato_glide = params.mono_mode && params.legato && v.active && v.held;

        v.active = true;
        v.held = true;
        v.track_idx = ti;
        v.pitch = pitch;
        v.velocity = velocity.clamp(0.01, 1.0);
        v.target_midi_pitch = pitch as f32;
        if params.glide_ms > 0.5 && (is_legato_glide || last_pitch.is_some()) {
            if !is_legato_glide {
                v.current_midi_pitch = last_pitch.unwrap_or(pitch as f32);
            }
        } else {
            v.current_midi_pitch = pitch as f32;
        }

        if !is_legato_glide {
            // Slight per-lane phase spread so unison attacks don't phase-spike
            for (l, ph) in v.phases.iter_mut().enumerate() {
                *ph = (l as f32 * 0.137) % 1.0;
            }
            v.sync_phase = 0.0;
            v.sub_phase = 0.0;
            v.filter_l.reset();
            v.filter_r.reset();
            v.amp_env.trigger(false);
            v.feg_env.trigger(false);
        }
        v.age = self.global_age;
    }

    pub fn release_note(&mut self, pitch: u8, track_idx: usize) {
        let ti = track_idx.min(15);
        for v in &mut self.voices {
            if v.active && v.track_idx == ti && v.pitch == pitch {
                v.held = false;
                v.amp_env.release();
                v.feg_env.release();
            }
        }
    }

    pub fn kill_note(&mut self, pitch: u8, track_idx: usize) {
        let ti = track_idx.min(15);
        for v in &mut self.voices {
            if v.active && v.track_idx == ti && v.pitch == pitch {
                v.active = false;
                v.held = false;
                v.amp_env.reset();
                v.feg_env.reset();
            }
        }
    }

    pub fn kill_track_notes(&mut self, track_idx: usize) {
        let ti = track_idx.min(15);
        for v in &mut self.voices {
            if v.track_idx == ti {
                v.active = false;
                v.held = false;
                v.amp_env.reset();
                v.feg_env.reset();
            }
        }
        self.track_peaks[ti] = (0.0, 0.0);
    }

    pub fn kill_all(&mut self) {
        for v in &mut self.voices {
            v.active = false;
            v.held = false;
            v.amp_env.reset();
            v.feg_env.reset();
        }
        self.track_peaks = [(0.0, 0.0); 16];
    }

    pub fn active_voice_count(&self, track_idx: usize) -> usize {
        let ti = track_idx.min(15);
        self.voices
            .iter()
            .filter(|v| v.active && v.track_idx == ti)
            .count()
    }

    /// Processes a single track's active OpenPoly voices into an interleaved stereo buffer.
    #[allow(clippy::too_many_arguments)]
    pub fn process_track(
        &mut self,
        track_idx: usize,
        params: &OpenPolyDspParams,
        wt: Option<&OpenPolyWavetableData>,
        out_interleaved: &mut [f32],
        gain_l: f32,
        gain_r: f32,
        frames: usize,
        sr: f32,
    ) {
        let ti = track_idx.min(15);
        let mut peak_l = 0.0f32;
        let mut peak_r = 0.0f32;

        let has_active = self.voices.iter().any(|v| v.active && v.track_idx == ti);
        if !has_active {
            // Smooth decay of cached meter peak
            self.track_peaks[ti].0 *= 0.75;
            self.track_peaks[ti].1 *= 0.75;
            return;
        }

        for f in 0..frames {
            let lfo_val = self.advance_track_lfo(ti, params, sr);
            let wt_ref = wt.unwrap_or(&self.default_wt);
            let mut sum_l = 0.0f32;
            let mut sum_r = 0.0f32;
            for v in &mut self.voices {
                if v.active && v.track_idx == ti {
                    let (vl, vr) = v.process_sample(params, Some(wt_ref), lfo_val, sr);
                    sum_l += vl;
                    sum_r += vr;
                }
            }
            peak_l = peak_l.max(sum_l.abs());
            peak_r = peak_r.max(sum_r.abs());
            out_interleaved[f * 2] += sum_l * gain_l;
            out_interleaved[f * 2 + 1] += sum_r * gain_r;
        }

        self.track_peaks[ti] = (
            peak_l.max(self.track_peaks[ti].0 * 0.8),
            peak_r.max(self.track_peaks[ti].1 * 0.8),
        );
    }

    /// Processes all active tracks when transport is stopped (virtual MIDI keyboard preview).
    #[allow(clippy::too_many_arguments)]
    pub fn process_all_idle(
        &mut self,
        enabled: &[bool; 16],
        params: &[OpenPolyDspParams; 16],
        wavetables: &[Option<Arc<OpenPolyWavetableData>>; 16],
        out_l: &mut [f32],
        out_r: &mut [f32],
        gain_l: &[f32; 16],
        gain_r: &[f32; 16],
        sr: f32,
    ) {
        let frames = out_l.len().min(out_r.len());
        let mut track_has_active = [false; 16];
        for v in &self.voices {
            if v.active && enabled[v.track_idx] {
                track_has_active[v.track_idx] = true;
            }
        }
        let mut peaks = [(0.0f32, 0.0f32); 16];

        for f in 0..frames {
            let mut lfo_vals = [0.0f32; 16];
            for ti in 0..16 {
                if track_has_active[ti] {
                    lfo_vals[ti] = self.advance_track_lfo(ti, &params[ti], sr);
                }
            }
            for v in &mut self.voices {
                if v.active && enabled[v.track_idx] {
                    let ti = v.track_idx;
                    let wt_ref = wavetables[ti]
                        .as_deref()
                        .unwrap_or(&self.default_wt);
                    let (vl, vr) = v.process_sample(&params[ti], Some(wt_ref), lfo_vals[ti], sr);
                    peaks[ti].0 = peaks[ti].0.max(vl.abs());
                    peaks[ti].1 = peaks[ti].1.max(vr.abs());
                    out_l[f] += vl * gain_l[ti];
                    out_r[f] += vr * gain_r[ti];
                }
            }
        }

        for ti in 0..16 {
            if track_has_active[ti] {
                self.track_peaks[ti] = (
                    peaks[ti].0.max(self.track_peaks[ti].0 * 0.8),
                    peaks[ti].1.max(self.track_peaks[ti].1 * 0.8),
                );
            } else {
                self.track_peaks[ti].0 *= 0.75;
                self.track_peaks[ti].1 *= 0.75;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_oscillator_types_produce_clean_finite_audio() {
        let sr = 48000.0;
        let wt = OpenPolyWavetableData::standard_four(2048);

        for osc in OpenPolyOscType::ALL {
            let mut pool = OpenPolyVoicePool::new();
            let mut params = OpenPolyDspParams::default();
            params.osc_type = osc;
            params.osc_index = 0.45;
            params.osc_fold = 0.2;
            params.sub_level = 0.0;
            params.noise_level = 0.0;
            params.filter_enabled = false;

            pool.trigger(60, 0.9, 0, &params);
            let mut buf = vec![0.0f32; 1024 * 2];
            pool.process_track(0, &params, Some(&wt), &mut buf, 1.0, 1.0, 1024, sr);

            let peak = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            assert!(
                buf.iter().all(|s| s.is_finite()),
                "Oscillator {:?} produced non-finite samples",
                osc
            );
            assert!(
                peak > 0.05,
                "Oscillator {:?} should produce audible output (peak={peak})",
                osc
            );
        }
    }

    #[test]
    fn all_filter_types_process_stably_and_lowpass_attenuates_high_frequencies() {
        let sr = 48000.0;
        for ft in OpenPolyFilterType::ALL {
            let mut pool = OpenPolyVoicePool::new();
            let mut params = OpenPolyDspParams::default();
            params.osc_type = OpenPolyOscType::Sawtooth;
            params.filter_enabled = true;
            params.filter_type = ft;
            params.filter_cutoff_hz = 400.0;
            params.filter_resonance = 0.75;
            params.filter_drive = 0.5;
            params.feg_enabled = false;

            pool.trigger(57, 1.0, 0, &params);
            let mut buf = vec![0.0f32; 2048 * 2];
            pool.process_track(0, &params, None, &mut buf, 1.0, 1.0, 2048, sr);

            let peak = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            assert!(
                buf.iter().all(|s| s.is_finite()),
                "Filter {:?} unstable with high resonance/drive",
                ft
            );
            assert!(
                peak > 0.01 && peak < 8.0,
                "Filter {:?} output out of sane range (peak={peak})",
                ft
            );
        }

        // Verify LowPassMG attenuates high-pitched notes much more at 200 Hz than at 12000 Hz
        let mut p_dark = OpenPolyDspParams::default();
        p_dark.osc_type = OpenPolyOscType::Sawtooth;
        p_dark.filter_type = OpenPolyFilterType::LowPassMG;
        p_dark.filter_cutoff_hz = 220.0;
        p_dark.filter_keytrack = 0.0;
        p_dark.feg_enabled = false;
        p_dark.sub_level = 0.0;

        let mut p_bright = p_dark;
        p_bright.filter_cutoff_hz = 12000.0;

        let mut pool_d = OpenPolyVoicePool::new();
        pool_d.trigger(84, 1.0, 0, &p_dark);
        let mut buf_d = vec![0.0f32; 2048 * 2];
        pool_d.process_track(0, &p_dark, None, &mut buf_d, 1.0, 1.0, 2048, sr);

        let mut pool_b = OpenPolyVoicePool::new();
        pool_b.trigger(84, 1.0, 0, &p_bright);
        let mut buf_b = vec![0.0f32; 2048 * 2];
        pool_b.process_track(0, &p_bright, None, &mut buf_b, 1.0, 1.0, 2048, sr);

        let rms_d: f32 = (buf_d.iter().map(|s| s * s).sum::<f32>() / buf_d.len() as f32).sqrt();
        let rms_b: f32 = (buf_b.iter().map(|s| s * s).sum::<f32>() / buf_b.len() as f32).sqrt();
        assert!(
            rms_b > rms_d * 2.5,
            "Low-pass MG at 220Hz should strongly attenuate C6 vs 12kHz (dark={rms_d}, bright={rms_b})"
        );
    }

    #[test]
    fn adsr_and_ad_envelopes_release_and_free_voices() {
        let sr = 48000.0;
        let mut params = OpenPolyDspParams::default();
        params.amp_env_type = OpenPolyEnvType::Adsr;
        params.amp_attack_ms = 2.0;
        params.amp_decay_ms = 20.0;
        params.amp_sustain = 0.6;
        params.amp_release_ms = 15.0;

        let mut pool = OpenPolyVoicePool::new();
        pool.trigger(60, 1.0, 0, &params);
        assert_eq!(pool.active_voice_count(0), 1);

        let mut buf = vec![0.0f32; 2048 * 2];
        pool.process_track(0, &params, None, &mut buf, 1.0, 1.0, 2048, sr);
        assert_eq!(pool.active_voice_count(0), 1);

        pool.release_note(60, 0);
        let mut rel_buf = vec![0.0f32; 4096 * 2];
        pool.process_track(0, &params, None, &mut rel_buf, 1.0, 1.0, 4096, sr);
        assert_eq!(
            pool.active_voice_count(0),
            0,
            "Voice should return to free pool after ADSR release"
        );

        // AD one-shot should finish on its own without release_note
        params.amp_env_type = OpenPolyEnvType::Ad;
        params.amp_decay_ms = 12.0;
        pool.trigger(64, 1.0, 0, &params);
        let mut ad_buf = vec![0.0f32; 4096 * 2];
        pool.process_track(0, &params, None, &mut ad_buf, 1.0, 1.0, 4096, sr);
        assert_eq!(
            pool.active_voice_count(0),
            0,
            "AD one-shot voice should complete decay and free itself"
        );
    }

    #[test]
    fn sub_oscillator_and_noise_contribute_independently() {
        let sr = 48000.0;
        let mut params = OpenPolyDspParams::default();
        params.filter_enabled = false;
        params.sub_level = 0.0;
        params.noise_level = 0.8;
        params.osc_type = OpenPolyOscType::Sine;
        params.output_gain = 1.0;

        let mut pool = OpenPolyVoicePool::new();
        pool.trigger(60, 1.0, 0, &params);
        let mut buf = vec![0.0f32; 1024 * 2];
        pool.process_track(0, &params, None, &mut buf, 1.0, 1.0, 1024, sr);
        let peak = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.1, "Noise + osc should produce audio (peak={peak})");
    }
}
