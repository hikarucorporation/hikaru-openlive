/*
 * Hikaru OpenLive / OpenStudio - Audio DAW
 * Copyright (C) 2026 Hikaru OpenStudio Developers
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU Affero General Public License for more details.
 */

//! **Hikaru OpenFlanger+**: procesador de *flanging* multimodal inspirado en
//! **Flanger+** de Bitwig Studio, con 4 arquitecturas de carácter:
//! - **`DP`** (*Digital / Gritty*): línea de retardo digital con mordida armónica,
//!   realimentación brillante y contorno angular.
//! - **`MX`** (*Classic Pedal*): *flanger* clásico estilo pedal BBD con enfoque en medios,
//!   amortiguamiento analógico y saturación par sutil.
//! - **`TFX`** (*Smooth Sparkly*, por defecto): interpolación cúbica ultra-limpia con brillo
//!   aéreo y filo definido en las muescas de peine.
//! - **`WA`** (*Wide Analog*): carácter profundo y envolvente de estudio con barrido amplio
//!   y calidez orgánica.
//!
//! Emplea interpolación cúbica de Hermite de 4 puntos en la línea de retardo fraccional,
//! saturación antialias **ADAA1** en el lazo de realimentación y suavizado anti-*zipper*.

use std::f64::consts::{PI, TAU};

use hikaru_core::AudioBuffer;
use hikaru_plugin_host::{HostTransportInfo, PluginInstance, PluginParamInfo};

pub const FLANGER_PLUS_MIN_SPEED_HZ: f32 = 0.02;
pub const FLANGER_PLUS_MAX_SPEED_HZ: f32 = 16.0;
const FLANGER_DELAY_BUF_SIZE: usize = 8192; // > 42 ms @ 192 kHz

pub const FLP_PARAM_MODE: u32 = 0;
pub const FLP_PARAM_COLOR: u32 = 1;
pub const FLP_PARAM_FEEDBACK: u32 = 2;
pub const FLP_PARAM_SPEED: u32 = 3;
pub const FLP_PARAM_DEPTH: u32 = 4;
pub const FLP_PARAM_PHASE_INVERT: u32 = 5;
pub const FLP_PARAM_STEREO: u32 = 6;
pub const FLP_PARAM_MIX: u32 = 7;
pub const FLP_PARAM_TEMPO_SYNC: u32 = 8;
pub const FLP_PARAM_SYNC_DIV: u32 = 9;
pub const FLANGER_PLUS_PARAM_COUNT: usize = 10;

pub const FLANGER_SYNC_LABELS: [&str; 10] = [
    "1/16", "1/8T", "1/8", "1/4T", "1/4", "1/2", "1/1", "2/1", "4/1", "8/1",
];

/// Duración de un ciclo completo de LFO en pulsos (negras = 1.0 beat).
pub const FLANGER_SYNC_BEATS: [f32; 10] = [
    0.25,
    1.0 / 3.0,
    0.5,
    2.0 / 3.0,
    1.0,
    2.0,
    4.0,
    8.0,
    16.0,
    32.0,
];

// =============================================================================
// Modelos de carácter (`DP`, `MX`, `TFX`, `WA`)
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlangerPlusMode {
    /// Arquitectura digital con grano y filo agresivo.
    Dp = 0,
    /// Flanger clásico firme estilo pedal BBD.
    Mx = 1,
    /// Modo suave y brillante con un toque de filo (por defecto).
    Tfx = 2,
    /// Carácter profundo y amplio con tacto delicado.
    Wa = 3,
}

impl FlangerPlusMode {
    pub const ALL: [Self; 4] = [Self::Dp, Self::Mx, Self::Tfx, Self::Wa];

    pub fn from_index(idx: usize) -> Self {
        match idx {
            0 => Self::Dp,
            1 => Self::Mx,
            2 => Self::Tfx,
            _ => Self::Wa,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Dp => "DP",
            Self::Mx => "MX",
            Self::Tfx => "TFX",
            Self::Wa => "WA",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Dp => "DP — Digital Gritty Flanger (sharp scrapy metallic edge)",
            Self::Mx => "MX — Classic Pedal Flanger (firm BBD midrange sweep)",
            Self::Tfx => "TFX — Smooth Sparkly Flanger (clean airy shimmer with crisp edge)",
            Self::Wa => "WA — Wide Studio Flanger (strong lush character with delicate touch)",
        }
    }

    /// Rango de retardo base `(min_ms, max_ms)` gobernado por `Color` (menor retardo = peine más agudo).
    #[inline]
    fn color_delay_bounds_ms(self) -> (f64, f64) {
        match self {
            Self::Dp => (0.22, 5.50),
            Self::Mx => (0.55, 8.20),
            Self::Tfx => (0.28, 10.50),
            Self::Wa => (0.40, 14.00),
        }
    }

    /// Excursión máxima del LFO en milisegundos cuando `Depth = 100%`.
    #[inline]
    fn max_sweep_ms(self) -> f64 {
        match self {
            Self::Dp => 3.80,
            Self::Mx => 5.60,
            Self::Tfx => 7.20,
            Self::Wa => 9.50,
        }
    }

    /// Ganancia máxima interna del lazo de realimentación cuando `FB = 100%`.
    #[inline]
    fn max_feedback_gain(self) -> f64 {
        match self {
            Self::Dp => 0.90,
            Self::Mx => 0.87,
            Self::Tfx => 0.91,
            Self::Wa => 0.89,
        }
    }

    /// Desfase estéreo (en fracción de ciclo `0..1`) aplicado al LFO derecho cuando `Stereo` está activo.
    #[inline]
    fn stereo_phase_offset(self) -> f64 {
        match self {
            Self::Dp => 0.50,  // 180° oposición digital
            Self::Mx => 0.33,  // 120° apertura de pedal
            Self::Tfx => 0.25, // 90° cuadratura cristalina
            Self::Wa => 0.50,  // 180° campo amplio
        }
    }
}

// =============================================================================
// Parámetros públicos y conversiones normalizadas
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlangerPlusParams {
    pub mode: FlangerPlusMode,
    /// Frecuencia / tono del peine de retardo (`0.0 ..= 1.0`, default `0.40`).
    pub color: f32,
    /// Intensidad de realimentación resonante (`0.0 ..= 1.0`, default `0.50`).
    pub feedback: f32,
    /// Velocidad normalizada del LFO (`0.0 ..= 1.0`, default `0.48` ≈ `0.50 Hz`).
    pub speed_norm: f32,
    /// Profundidad de modulación del barrido (`0.0 ..= 1.0`, default `0.55`).
    pub depth: f32,
    /// Inversión de polaridad `[∅]` sobre la rama húmeda y el lazo de realimentación.
    pub phase_invert: bool,
    /// Desfase estéreo `[⚭]` entre los LFOs de los canales izquierdo y derecho.
    pub stereo: bool,
    /// Mezcla Dry/Wet (`0.0 ..= 1.0`, default `0.50`).
    pub mix: f32,
    /// Sincronización opcional al tempo del proyecto (`false` por defecto = Hz).
    pub tempo_sync: bool,
    /// Índice de división rítmica `0..9` cuando `tempo_sync == true`.
    pub sync_div: u8,
}

impl Default for FlangerPlusParams {
    fn default() -> Self {
        Self {
            mode: FlangerPlusMode::Tfx,
            color: 0.40,
            feedback: 0.50,
            speed_norm: 0.48,
            depth: 0.55,
            phase_invert: false,
            stereo: true,
            mix: 0.50,
            tempo_sync: false,
            sync_div: 6, // 1/1 compás
        }
    }
}

impl FlangerPlusParams {
    pub fn clamped(self) -> Self {
        let clean = |v: f32, def: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { def };
        let def = Self::default();
        Self {
            mode: self.mode,
            color: clean(self.color, def.color),
            feedback: clean(self.feedback, def.feedback),
            speed_norm: clean(self.speed_norm, def.speed_norm),
            depth: clean(self.depth, def.depth),
            phase_invert: self.phase_invert,
            stereo: self.stereo,
            mix: clean(self.mix, def.mix),
            tempo_sync: self.tempo_sync,
            sync_div: self.sync_div.min((FLANGER_SYNC_BEATS.len() - 1) as u8),
        }
    }

    #[inline]
    pub fn speed_norm_to_hz(norm: f32) -> f32 {
        let ratio = FLANGER_PLUS_MAX_SPEED_HZ / FLANGER_PLUS_MIN_SPEED_HZ;
        FLANGER_PLUS_MIN_SPEED_HZ * ratio.powf(norm.clamp(0.0, 1.0))
    }

    #[inline]
    pub fn speed_hz_to_norm(hz: f32) -> f32 {
        let ratio = FLANGER_PLUS_MAX_SPEED_HZ / FLANGER_PLUS_MIN_SPEED_HZ;
        ((hz.clamp(FLANGER_PLUS_MIN_SPEED_HZ, FLANGER_PLUS_MAX_SPEED_HZ)
            / FLANGER_PLUS_MIN_SPEED_HZ)
            .ln()
            / ratio.ln())
        .clamp(0.0, 1.0)
    }

    #[inline]
    pub fn effective_speed_hz(&self, tempo_bpm: f64) -> f64 {
        if self.tempo_sync {
            let idx = (self.sync_div as usize).min(FLANGER_SYNC_BEATS.len() - 1);
            let cycle_beats = FLANGER_SYNC_BEATS[idx] as f64;
            let bpm = tempo_bpm.clamp(20.0, 400.0);
            ((bpm / 60.0) / cycle_beats).clamp(0.005, 30.0)
        } else {
            Self::speed_norm_to_hz(self.speed_norm) as f64
        }
    }

    /// Retardo nominal en milisegundos gobernado por `Color` (`Color = 100%` -> retardo más corto y agudo).
    #[inline]
    pub fn center_delay_ms(&self) -> f32 {
        let (d_min, d_max) = self.mode.color_delay_bounds_ms();
        let inv = 1.0 - self.color.clamp(0.0, 1.0) as f64;
        (d_min * (d_max / d_min).powf(inv)) as f32
    }

    pub fn format_speed_readout(&self, tempo_bpm: f64) -> String {
        if self.tempo_sync {
            let idx = (self.sync_div as usize).min(FLANGER_SYNC_LABELS.len() - 1);
            format!(
                "{} ({:.2} Hz)",
                FLANGER_SYNC_LABELS[idx],
                self.effective_speed_hz(tempo_bpm)
            )
        } else {
            let hz = Self::speed_norm_to_hz(self.speed_norm);
            if hz < 10.0 {
                format!("{hz:.2} Hz")
            } else {
                format!("{hz:.1} Hz")
            }
        }
    }

    pub fn format_color_readout(&self) -> String {
        format!("{:.2} ms", self.center_delay_ms())
    }

    pub fn get_normalized(&self, id: u32) -> Option<f32> {
        match id {
            FLP_PARAM_MODE => Some((self.mode as u32) as f32 / 3.0),
            FLP_PARAM_COLOR => Some(self.color.clamp(0.0, 1.0)),
            FLP_PARAM_FEEDBACK => Some(self.feedback.clamp(0.0, 1.0)),
            FLP_PARAM_SPEED => Some(self.speed_norm.clamp(0.0, 1.0)),
            FLP_PARAM_DEPTH => Some(self.depth.clamp(0.0, 1.0)),
            FLP_PARAM_PHASE_INVERT => Some(if self.phase_invert { 1.0 } else { 0.0 }),
            FLP_PARAM_STEREO => Some(if self.stereo { 1.0 } else { 0.0 }),
            FLP_PARAM_MIX => Some(self.mix.clamp(0.0, 1.0)),
            FLP_PARAM_TEMPO_SYNC => Some(if self.tempo_sync { 1.0 } else { 0.0 }),
            FLP_PARAM_SYNC_DIV => Some(
                (self.sync_div as f32 / (FLANGER_SYNC_BEATS.len() - 1) as f32).clamp(0.0, 1.0),
            ),
            _ => None,
        }
    }

    pub fn set_normalized(&mut self, id: u32, value: f32) -> bool {
        if !value.is_finite() {
            return false;
        }
        let v = value.clamp(0.0, 1.0);
        match id {
            FLP_PARAM_MODE => self.mode = FlangerPlusMode::from_index((v * 3.0).round() as usize),
            FLP_PARAM_COLOR => self.color = v,
            FLP_PARAM_FEEDBACK => self.feedback = v,
            FLP_PARAM_SPEED => self.speed_norm = v,
            FLP_PARAM_DEPTH => self.depth = v,
            FLP_PARAM_PHASE_INVERT => self.phase_invert = v >= 0.5,
            FLP_PARAM_STEREO => self.stereo = v >= 0.5,
            FLP_PARAM_MIX => self.mix = v,
            FLP_PARAM_TEMPO_SYNC => self.tempo_sync = v >= 0.5,
            FLP_PARAM_SYNC_DIV => {
                self.sync_div = ((v * (FLANGER_SYNC_BEATS.len() - 1) as f32).round() as u8)
                    .min((FLANGER_SYNC_BEATS.len() - 1) as u8);
            }
            _ => return false,
        }
        *self = self.clamped();
        true
    }
}

// =============================================================================
// Primitivas DSP internas
// =============================================================================

#[derive(Clone, Copy, Default)]
struct OnePoleHp {
    ic1eq: f64,
}

impl OnePoleHp {
    #[inline]
    fn process(&mut self, x: f64, g: f64) -> f64 {
        let big_g = g / (1.0 + g);
        let v = (x - self.ic1eq) * big_g;
        let lp = v + self.ic1eq;
        let next_s = lp + v;
        self.ic1eq = if next_s.abs() < 1.0e-30 { 0.0 } else { next_s };
        x - lp
    }
}

#[derive(Clone, Copy, Default)]
struct OnePoleLp {
    state: f64,
}

impl OnePoleLp {
    #[inline]
    fn process(&mut self, x: f64, coeff: f64) -> f64 {
        let y = self.state + coeff * (x - self.state);
        self.state = if y.abs() < 1.0e-30 { 0.0 } else { y };
        self.state
    }
}

#[derive(Clone, Copy, Default)]
struct AdaaTanh {
    prev_x: f64,
    prev_f: f64,
}

impl AdaaTanh {
    #[inline]
    fn antideriv(x: f64, drive: f64, inv_norm: f64) -> f64 {
        let u = (x * drive).clamp(-40.0, 40.0);
        let ax = u.abs();
        let ln_cosh = ax + (-2.0 * ax).exp().ln_1p() - std::f64::consts::LN_2;
        (ln_cosh / drive) * inv_norm
    }

    #[inline]
    fn process(&mut self, x: f64, drive: f64) -> f64 {
        if drive <= 1.0e-5 {
            self.prev_x = x;
            self.prev_f = 0.0;
            return x;
        }
        let inv_norm = 1.0 / drive.tanh().max(1.0e-6);
        let cur_f = Self::antideriv(x, drive, inv_norm);
        let dx = x - self.prev_x;
        let y = if dx.abs() > 1.0e-6 {
            (cur_f - self.prev_f) / dx
        } else {
            let mid = 0.5 * (x + self.prev_x);
            (mid * drive).tanh() * inv_norm
        };
        self.prev_x = if x.abs() < 1.0e-30 { 0.0 } else { x };
        self.prev_f = if cur_f.abs() < 1.0e-30 { 0.0 } else { cur_f };
        y
    }
}

/// Interpolación cúbica de Hermite de 4 puntos en buffer circular potencia de 2.
#[inline]
fn read_hermite(buf: &[f64; FLANGER_DELAY_BUF_SIZE], write_pos: usize, delay_samples: f64) -> f64 {
    let mask = FLANGER_DELAY_BUF_SIZE - 1;
    let clamped = delay_samples.clamp(1.0, (FLANGER_DELAY_BUF_SIZE - 4) as f64);
    let int_delay = clamped.floor() as usize;
    let frac = clamped - int_delay as f64;

    let i0 = write_pos.wrapping_sub(int_delay.wrapping_sub(1)) & mask;
    let i1 = write_pos.wrapping_sub(int_delay) & mask;
    let i2 = write_pos.wrapping_sub(int_delay + 1) & mask;
    let i3 = write_pos.wrapping_sub(int_delay + 2) & mask;

    let ym1 = buf[i0];
    let y0 = buf[i1];
    let y1 = buf[i2];
    let y2 = buf[i3];

    let c0 = y0;
    let c1 = 0.5 * (y1 - ym1);
    let c2 = ym1 - 2.5 * y0 + 2.0 * y1 - 0.5 * y2;
    let c3 = 0.5 * (y2 - ym1) + 1.5 * (y0 - y1);

    ((c3 * frac + c2) * frac + c1) * frac + c0
}

#[inline]
fn shaped_lfo_unipolar(mode: FlangerPlusMode, phase: f64) -> f64 {
    let p = phase - phase.floor();
    let s = 0.5 + 0.5 * (TAU * p).sin();
    let tri = 1.0 - 2.0 * ((p + 0.25).fract() - 0.5).abs();
    match mode {
        FlangerPlusMode::Dp => 0.25 * s + 0.75 * tri,
        FlangerPlusMode::Mx => 0.45 * s + 0.55 * tri,
        FlangerPlusMode::Tfx => 0.75 * s + 0.25 * tri,
        FlangerPlusMode::Wa => s,
    }
}

// =============================================================================
// Procesador principal FlangerPlusEffect
// =============================================================================

pub struct FlangerPlusEffect {
    params: FlangerPlusParams,
    mod_offsets: [f32; FLANGER_PLUS_PARAM_COUNT],
    sample_rate: f32,
    tempo_bpm: f64,

    lfo_phase: f64,
    delay_buf: [[f64; FLANGER_DELAY_BUF_SIZE]; 2],
    write_pos: usize,

    fb_state: [f64; 2],
    fb_hp: [OnePoleHp; 2],
    fb_damp: [OnePoleLp; 2],
    fb_adaa: [AdaaTanh; 2],

    smooth_color: f64,
    smooth_fb: f64,
    smooth_depth: f64,
    smooth_mix: f64,
    slew_coeff: f64,
}

impl FlangerPlusEffect {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        let params = FlangerPlusParams::default();
        let slew_coeff = 1.0 - (-1.0 / (0.0025 * (sr as f64))).exp();
        Self {
            params,
            mod_offsets: [0.0; FLANGER_PLUS_PARAM_COUNT],
            sample_rate: sr,
            tempo_bpm: 120.0,
            lfo_phase: 0.0,
            delay_buf: [[0.0; FLANGER_DELAY_BUF_SIZE]; 2],
            write_pos: 0,
            fb_state: [0.0; 2],
            fb_hp: [OnePoleHp::default(); 2],
            fb_damp: [OnePoleLp::default(); 2],
            fb_adaa: [AdaaTanh::default(); 2],
            smooth_color: params.color as f64,
            smooth_fb: params.feedback as f64,
            smooth_depth: params.depth as f64,
            smooth_mix: params.mix as f64,
            slew_coeff,
        }
    }

    pub fn params(&self) -> FlangerPlusParams {
        self.params
    }

    pub fn set_params(&mut self, next: FlangerPlusParams) {
        self.params = next.clamped();
    }

    pub fn reset(&mut self) {
        self.lfo_phase = 0.0;
        self.delay_buf = [[0.0; FLANGER_DELAY_BUF_SIZE]; 2];
        self.write_pos = 0;
        self.fb_state = [0.0; 2];
        self.fb_hp = [OnePoleHp::default(); 2];
        self.fb_damp = [OnePoleLp::default(); 2];
        self.fb_adaa = [AdaaTanh::default(); 2];
    }

    #[inline]
    fn effective_params(&self) -> FlangerPlusParams {
        let mut p = self.params;
        let apply = |base: f32, idx: usize| (base + self.mod_offsets[idx]).clamp(0.0, 1.0);
        p.color = apply(p.color, FLP_PARAM_COLOR as usize);
        p.feedback = apply(p.feedback, FLP_PARAM_FEEDBACK as usize);
        p.speed_norm = apply(p.speed_norm, FLP_PARAM_SPEED as usize);
        p.depth = apply(p.depth, FLP_PARAM_DEPTH as usize);
        p.mix = apply(p.mix, FLP_PARAM_MIX as usize);
        p
    }

    #[inline]
    pub fn process_stereo(&mut self, in_l: f32, in_r: f32) -> (f32, f32) {
        let dry_l = if in_l.is_finite() { in_l as f64 } else { 0.0 };
        let dry_r = if in_r.is_finite() { in_r as f64 } else { 0.0 };
        let in_frame = [dry_l, dry_r];

        let eff = self.effective_params();
        let sr = (self.sample_rate as f64).max(8_000.0);
        let speed_hz = eff.effective_speed_hz(self.tempo_bpm);
        let phase_inc = speed_hz / sr;

        let c = self.slew_coeff;
        self.smooth_color += c * ((eff.color as f64) - self.smooth_color);
        self.smooth_fb += c * ((eff.feedback as f64) - self.smooth_fb);
        self.smooth_depth += c * ((eff.depth as f64) - self.smooth_depth);
        self.smooth_mix += c * ((eff.mix as f64) - self.smooth_mix);

        let mode = eff.mode;
        let (d_min_ms, d_max_ms) = mode.color_delay_bounds_ms();
        let base_ms = d_min_ms * (d_max_ms / d_min_ms).powf(1.0 - self.smooth_color.clamp(0.0, 1.0));
        let sweep_ms = mode.max_sweep_ms() * self.smooth_depth.clamp(0.0, 1.0);

        let fb_sign = if eff.phase_invert { -1.0 } else { 1.0 };
        let fb_gain = self.smooth_fb.clamp(0.0, 1.0) * mode.max_feedback_gain() * fb_sign;

        let stereo_off = if eff.stereo {
            mode.stereo_phase_offset()
        } else {
            0.0
        };

        let (fb_hp_hz, fb_lp_hz, drive): (f64, f64, f64) = match mode {
            FlangerPlusMode::Dp => (70.0, 16_500.0, 1.45),
            FlangerPlusMode::Mx => (95.0, 6_800.0, 1.25),
            FlangerPlusMode::Tfx => (42.0, 14_500.0, 0.95),
            FlangerPlusMode::Wa => (32.0, 11_000.0, 1.15),
        };
        let hp_g = (PI * fb_hp_hz.min(sr * 0.42) / sr).tan();
        let damp_coeff = 1.0 - (-TAU * fb_lp_hz.min(sr * 0.44) / sr).exp();

        let mut wet_out = [0.0_f64; 2];

        for ch in 0..2 {
            let ch_phase = self.lfo_phase + if ch == 1 { stereo_off } else { 0.0 };
            let lfo = shaped_lfo_unipolar(mode, ch_phase);
            let comp_detune = if eff.stereo && ch == 1 { 1.02 } else { 1.0 };
            let delay_ms = (base_ms + lfo * sweep_ms) * comp_detune;
            let delay_samples = (delay_ms * 0.001 * sr).clamp(1.0, (FLANGER_DELAY_BUF_SIZE - 4) as f64);

            let fb_filtered = self.fb_damp[ch].process(
                self.fb_hp[ch].process(self.fb_state[ch], hp_g),
                damp_coeff,
            );
            let fb_sat = match mode {
                FlangerPlusMode::Dp => {
                    let gritty = fb_filtered + 0.08 * fb_filtered * fb_filtered.abs();
                    self.fb_adaa[ch].process(gritty, drive)
                }
                FlangerPlusMode::Mx => {
                    let biased = fb_filtered + 0.05 * fb_filtered * fb_filtered;
                    self.fb_adaa[ch].process(biased, drive)
                }
                _ => self.fb_adaa[ch].process(fb_filtered, drive),
            };

            let res_comp = (1.0 - 0.18 * self.smooth_fb * self.smooth_fb).sqrt();
            let write_val = (in_frame[ch] + fb_gain * fb_sat) * res_comp;
            self.delay_buf[ch][self.write_pos] = if write_val.abs() < 1.0e-30 {
                0.0
            } else {
                write_val
            };

            let delayed = read_hermite(&self.delay_buf[ch], self.write_pos, delay_samples);
            self.fb_state[ch] = if delayed.abs() < 1.0e-30 { 0.0 } else { delayed };
            wet_out[ch] = delayed;
        }

        self.write_pos = (self.write_pos + 1) & (FLANGER_DELAY_BUF_SIZE - 1);
        self.lfo_phase += phase_inc;
        if self.lfo_phase >= 1.0 {
            self.lfo_phase -= self.lfo_phase.floor();
        }

        let mix = self.smooth_mix.clamp(0.0, 1.0);
        let wet_pol = if eff.phase_invert { -1.0 } else { 1.0 };
        let out_l = (1.0 - mix) * dry_l + mix * (wet_pol * wet_out[0]);
        let out_r = (1.0 - mix) * dry_r + mix * (wet_pol * wet_out[1]);

        let clean_out = |v: f64| -> f32 {
            if !v.is_finite() || v.abs() < 1.0e-30 {
                0.0
            } else {
                v.clamp(-4.0, 4.0) as f32
            }
        };
        (clean_out(out_l), clean_out(out_r))
    }
}

impl PluginInstance for FlangerPlusEffect {
    fn process(&mut self, buffer: &mut AudioBuffer) {
        for frame in buffer.samples.chunks_exact_mut(2) {
            let (l, r) = self.process_stereo(frame[0], frame[1]);
            frame[0] = l;
            frame[1] = r;
        }
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(8_000.0);
        self.slew_coeff = 1.0 - (-1.0 / (0.0025 * (self.sample_rate as f64))).exp();
    }

    fn set_transport(&mut self, transport: HostTransportInfo) {
        if transport.tempo.is_finite() && transport.tempo > 10.0 {
            self.tempo_bpm = transport.tempo;
        }
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn show_gui_embedded(&mut self, _: hikaru_plugin_host::RawWindowHandle) -> Result<(), String> {
        Err("Native plugin".to_string())
    }

    fn show_gui_floating(&mut self) -> Result<(), String> {
        Err("Native plugin".to_string())
    }

    fn hide_gui(&mut self) {}

    fn get_name(&self) -> &str {
        "Hikaru OpenFlanger+"
    }

    fn param_count(&mut self) -> usize {
        FLANGER_PLUS_PARAM_COUNT
    }

    fn param_info(&mut self, index: usize) -> Option<PluginParamInfo> {
        let def = FlangerPlusParams::default();
        let mk =
            |id: u32, name: &str, unit: &str, min: f32, max: f32, step_count: i32| PluginParamInfo {
                id,
                name: name.to_string(),
                unit: unit.to_string(),
                default_value: def.get_normalized(id).unwrap_or(0.0),
                min,
                max,
                step_count,
                can_automate: true,
                is_modulatable: true,
            };
        match index as u32 {
            FLP_PARAM_MODE => Some(mk(0, "Character Mode", "", 0.0, 3.0, 3)),
            FLP_PARAM_COLOR => Some(mk(1, "Color", "%", 0.0, 100.0, 0)),
            FLP_PARAM_FEEDBACK => Some(mk(2, "FB", "%", 0.0, 100.0, 0)),
            FLP_PARAM_SPEED => Some(mk(
                3,
                "Speed",
                "Hz",
                FLANGER_PLUS_MIN_SPEED_HZ,
                FLANGER_PLUS_MAX_SPEED_HZ,
                0,
            )),
            FLP_PARAM_DEPTH => Some(mk(4, "Depth", "%", 0.0, 100.0, 0)),
            FLP_PARAM_PHASE_INVERT => Some(mk(5, "Polarity Invert", "", 0.0, 1.0, 1)),
            FLP_PARAM_STEREO => Some(mk(6, "Stereo Phase", "", 0.0, 1.0, 1)),
            FLP_PARAM_MIX => Some(mk(7, "Mix", "%", 0.0, 100.0, 0)),
            FLP_PARAM_TEMPO_SYNC => Some(mk(8, "Tempo Sync", "", 0.0, 1.0, 1)),
            FLP_PARAM_SYNC_DIV => Some(mk(9, "Sync Division", "", 0.0, 9.0, 9)),
            _ => None,
        }
    }

    fn get_param_normalized(&mut self, id: u32) -> Option<f32> {
        self.params.get_normalized(id)
    }

    fn set_param_normalized(&mut self, id: u32, value: f32) -> bool {
        let mut next = self.params;
        if next.set_normalized(id, value) {
            self.set_params(next);
            true
        } else {
            false
        }
    }

    fn set_param_modulation(&mut self, id: u32, delta_normalized: f32) -> bool {
        let idx = id as usize;
        if idx < FLANGER_PLUS_PARAM_COUNT && delta_normalized.is_finite() {
            self.mod_offsets[idx] = delta_normalized.clamp(-1.0, 1.0);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_roundtrip_covers_all_parameters() {
        let mut p = FlangerPlusParams::default();
        for mode in FlangerPlusMode::ALL {
            let norm = (mode as u32) as f32 / 3.0;
            assert!(p.set_normalized(FLP_PARAM_MODE, norm));
            assert_eq!(p.mode, mode);
        }
        for &(id, val) in &[
            (FLP_PARAM_COLOR, 0.73),
            (FLP_PARAM_FEEDBACK, 0.82),
            (FLP_PARAM_SPEED, 0.41),
            (FLP_PARAM_DEPTH, 0.67),
            (FLP_PARAM_PHASE_INVERT, 1.0),
            (FLP_PARAM_STEREO, 1.0),
            (FLP_PARAM_MIX, 0.45),
            (FLP_PARAM_TEMPO_SYNC, 1.0),
            (FLP_PARAM_SYNC_DIV, 4.0 / 9.0),
        ] {
            assert!(p.set_normalized(id, val));
            let got = p.get_normalized(id).expect("valid id");
            assert!((got - val).abs() < 1.0e-4, "id={id}: got={got}, expected={val}");
        }
    }

    #[test]
    fn dry_mix_zero_passes_signal_unchanged() {
        let mut fx = FlangerPlusEffect::new(48_000.0);
        fx.set_params(FlangerPlusParams {
            mix: 0.0,
            ..FlangerPlusParams::default()
        });
        for _ in 0..2048 {
            let _ = fx.process_stereo(0.0, 0.0);
        }
        for i in 0..512 {
            let x = (i as f32 * 0.13).sin() * 0.7;
            let (l, r) = fx.process_stereo(x, -x);
            assert!((l - x).abs() < 1.0e-4);
            assert!((r + x).abs() < 1.0e-4);
        }
    }

    #[test]
    fn all_four_modes_create_comb_sweep_and_remain_bounded_at_max_feedback() {
        let sr = 48_000.0;
        for mode in FlangerPlusMode::ALL {
            let mut fx = FlangerPlusEffect::new(sr);
            fx.set_params(FlangerPlusParams {
                mode,
                color: 0.50,
                feedback: 1.0,
                speed_norm: 0.60,
                depth: 0.85,
                phase_invert: false,
                stereo: true,
                mix: 0.50,
                ..FlangerPlusParams::default()
            });

            let mut max_abs = 0.0_f32;
            let mut diff_lr_energy = 0.0_f64;
            for i in 0..12_000 {
                let t = i as f32 / sr;
                let saw = 2.0 * (t * 220.0 - (t * 220.0 + 0.5).floor()) * 0.5;
                let (l, r) = fx.process_stereo(saw, saw);
                assert!(l.is_finite() && r.is_finite(), "NaN/Inf in mode {:?}", mode);
                max_abs = max_abs.max(l.abs()).max(r.abs());
                diff_lr_energy += ((l - r) as f64).powi(2);
            }
            assert!(
                max_abs > 0.05 && max_abs <= 4.0,
                "mode {:?} peak out of range: {max_abs}",
                mode
            );
            assert!(
                diff_lr_energy > 1.0,
                "Stereo mode in {:?} must decorrelate L and R sweeps (diff_energy={diff_lr_energy})",
                mode
            );
        }
    }

    #[test]
    fn polarity_invert_attenuates_low_frequency_sum_at_fifty_percent_mix() {
        let sr = 48_000.0;
        let run = |invert: bool| -> f64 {
            let mut fx = FlangerPlusEffect::new(sr);
            fx.set_params(FlangerPlusParams {
                mode: FlangerPlusMode::Tfx,
                color: 1.0, // Retardo mínimo (~0.28 ms) -> sub-grave (40 Hz) casi en fase 0°
                feedback: 0.0,
                depth: 0.0,
                phase_invert: invert,
                stereo: false,
                mix: 0.50,
                ..FlangerPlusParams::default()
            });
            let mut energy = 0.0_f64;
            for i in 0..4800 {
                let x = (TAU as f32 * 40.0 * (i as f32) / sr).sin() * 0.5;
                let (l, _) = fx.process_stereo(x, x);
                if i >= 2400 {
                    energy += (l as f64) * (l as f64);
                }
            }
            energy
        };

        let normal_energy = run(false);
        let inverted_energy = run(true);
        assert!(
            inverted_energy < normal_energy * 0.15,
            "Polarity invert [∅] should strongly attenuate low frequencies when Mix=50%: normal={normal_energy}, inv={inverted_energy}"
        );
    }
}
