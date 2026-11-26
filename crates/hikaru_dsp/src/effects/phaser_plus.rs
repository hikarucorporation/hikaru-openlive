/*
 * Hikaru OpenLive / OpenStudio - Audio DAW
 * Copyright (C) 2026 Hikaru OpenStudio Developers
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 *
 * Stereo LFO phase-offset laws and multi-stage all-pass phaser concepts draw
 * inspiration from Lemon Studio (`crates/os-engine/src/effects/phaser.rs`,
 * GPL-3.0-or-later, compatible with AGPL-3.0-or-later).
 */

//! **Hikaru OpenPhaser+**: procesador de *phasing* analógico multimodal inspirado en
//! **Phaser+** de Bitwig Studio, con 4 arquitecturas clásicas de desfase:
//! - **`GS`** (*Grand Stereo Optical*): cascada óptica de 6 etapas estilo Krautrock Bi-Phase,
//!   barrido líquido con suavizado LDR y desfase estéreo de `180°`.
//! - **`EHx`** (*Small Stone OTA*, por defecto): cascada OTA de 4 etapas con escalonamiento
//!   asimétrico de condensadores, calidez no lineal `tanh` y resonancia vocal.
//! - **`MX`** (*Phase 90/100 JFET*): cascada JFET de 4 etapas enfocada en medios con asimetría
//!   armónica par sutil y corte de graves en el lazo de realimentación.
//! - **`MF`** (*Moogerfooger 12-Stage*): banco denso de 12 etapas *all-pass* con 6 muescas
//!   armónicas profundas y saturación cremosa en realimentación.
//!
//! Todas las etapas emplean integradores trapezoidales de preservación de topología (**TPT**)
//! con pre-deformación bilineal exacta (`tan(π·fc/fs)`), garantizando latencia cero (`0 samples`),
//! coincidencia de fase perfecta con la rama *dry* en mezcla paralela e invarianza frente a
//! modulación rápida, junto con saturación antialias de primer orden (**ADAA1**, `ln(cosh(x))`).

use std::f64::consts::{PI, TAU};

use hikaru_core::AudioBuffer;
use hikaru_plugin_host::{HostTransportInfo, PluginInstance, PluginParamInfo};

pub const PHASER_PLUS_MIN_SPEED_HZ: f32 = 0.02;
pub const PHASER_PLUS_MAX_SPEED_HZ: f32 = 16.0;
pub const PHASER_PLUS_MAX_STAGES: usize = 12;

pub const PHP_PARAM_MODE: u32 = 0;
pub const PHP_PARAM_COLOR: u32 = 1;
pub const PHP_PARAM_FEEDBACK: u32 = 2;
pub const PHP_PARAM_SPEED: u32 = 3;
pub const PHP_PARAM_DEPTH: u32 = 4;
pub const PHP_PARAM_PHASE_INVERT: u32 = 5;
pub const PHP_PARAM_STEREO: u32 = 6;
pub const PHP_PARAM_MIX: u32 = 7;
pub const PHP_PARAM_TEMPO_SYNC: u32 = 8;
pub const PHP_PARAM_SYNC_DIV: u32 = 9;
pub const PHASER_PLUS_PARAM_COUNT: usize = 10;

pub const PHASER_SYNC_LABELS: [&str; 10] = [
    "1/16", "1/8T", "1/8", "1/4T", "1/4", "1/2", "1/1", "2/1", "4/1", "8/1",
];

/// Duración de un ciclo completo de LFO en pulsos (negras = 1.0 beat).
pub const PHASER_SYNC_BEATS: [f32; 10] = [
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
// Modelos de carácter analógico (`GS`, `EHx`, `MX`, `MF`)
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaserPlusMode {
    /// 6 etapas ópticas suaves estilo Bi-Phase / Krautrock.
    Gs = 0,
    /// 4 etapas OTA con escalonamiento vocal asimétrico estilo Small Stone (por defecto).
    Ehx = 1,
    /// 4 etapas JFET enfocadas en medios con calidez de armónicos pares estilo Phase 90/100.
    Mx = 2,
    /// 12 etapas densas de sintetizador estilo Moogerfooger MF-103.
    Mf = 3,
}

impl PhaserPlusMode {
    pub const ALL: [Self; 4] = [Self::Gs, Self::Ehx, Self::Mx, Self::Mf];

    pub fn from_index(idx: usize) -> Self {
        match idx {
            0 => Self::Gs,
            1 => Self::Ehx,
            2 => Self::Mx,
            _ => Self::Mf,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Gs => "GS",
            Self::Ehx => "EHx",
            Self::Mx => "MX",
            Self::Mf => "MF",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Gs => "GS — 6-Stage Optical Stereo Phaser (silky wide Krautrock sweep)",
            Self::Ehx => "EHx — 4-Stage OTA Analog Phaser (classic vocal resonant sweep)",
            Self::Mx => "MX — 4-Stage JFET Pedal Phaser (focused punchy midrange)",
            Self::Mf => "MF — 12-Stage Studio Synth Phaser (dense 6-notch harmonic comb)",
        }
    }

    #[inline]
    pub fn stage_count(self) -> usize {
        match self {
            Self::Gs => 6,
            Self::Ehx => 4,
            Self::Mx => 4,
            Self::Mf => 12,
        }
    }

    /// Rango de frecuencia central `(f_min_hz, f_max_hz)` gobernado por `Color`.
    #[inline]
    fn color_freq_bounds(self) -> (f64, f64) {
        match self {
            Self::Gs => (95.0, 4_200.0),
            Self::Ehx => (85.0, 3_600.0),
            Self::Mx => (165.0, 2_900.0),
            Self::Mf => (55.0, 5_800.0),
        }
    }

    /// Excursión máxima del LFO en octavas (`±octavas`) cuando `Depth = 100%`.
    #[inline]
    fn max_sweep_octaves(self) -> f64 {
        match self {
            Self::Gs => 2.85,
            Self::Ehx => 3.10,
            Self::Mx => 2.35,
            Self::Mf => 3.25,
        }
    }

    /// Ganancia máxima interna del lazo de realimentación cuando `FB = 100%`.
    #[inline]
    fn max_feedback_gain(self) -> f64 {
        match self {
            Self::Gs => 0.89,
            Self::Ehx => 0.92,
            Self::Mx => 0.85,
            Self::Mf => 0.91,
        }
    }

    /// Desfase estéreo (en fracción de ciclo `0..1`) aplicado al LFO derecho cuando `Stereo` está activo.
    #[inline]
    fn stereo_phase_offset(self) -> f64 {
        match self {
            Self::Gs => 0.50,  // 180° oposición Bi-Phase
            Self::Ehx => 0.25, // 90° cuadratura clásica
            Self::Mx => 0.33,  // 120° apertura focalizada
            Self::Mf => 0.50,  // 180° campo ancho de 12 polos
        }
    }

    /// Multiplicadores de dispersión de polos por etapa alrededor de la frecuencia central.
    #[inline]
    fn stage_ratios(self) -> &'static [f64; PHASER_PLUS_MAX_STAGES] {
        const GS_RATIOS: [f64; 12] = [
            0.70, 0.83, 0.96, 1.05, 1.21, 1.42, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0,
        ];
        // Escalonamiento asimétrico inspirado en redes OTA clásicas (tipo Small Stone)
        const EHX_RATIOS: [f64; 12] = [
            0.56, 0.90, 1.14, 1.76, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0,
        ];
        // Agrupamiento JFET compacto en medios
        const MX_RATIOS: [f64; 12] = [
            0.86, 0.95, 1.05, 1.16, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0,
        ];
        // 12 etapas distribuidas logarítmicamente
        const MF_RATIOS: [f64; 12] = [
            0.48, 0.56, 0.65, 0.76, 0.88, 0.96, 1.04, 1.14, 1.31, 1.52, 1.78, 2.08,
        ];
        match self {
            Self::Gs => &GS_RATIOS,
            Self::Ehx => &EHX_RATIOS,
            Self::Mx => &MX_RATIOS,
            Self::Mf => &MF_RATIOS,
        }
    }
}

// =============================================================================
// Parámetros públicos y conversiones normalizadas
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhaserPlusParams {
    pub mode: PhaserPlusMode,
    /// Frecuencia central / timbre de las muescas (`0.0 ..= 1.0`, default `0.35`).
    pub color: f32,
    /// Intensidad de realimentación resonante (`0.0 ..= 1.0`, default `0.50`).
    pub feedback: f32,
    /// Velocidad normalizada del LFO (`0.0 ..= 1.0`, default `0.65` ≈ `1.56 Hz`).
    pub speed_norm: f32,
    /// Profundidad de modulación del barrido (`0.0 ..= 1.0`, default `0.50`).
    pub depth: f32,
    /// Inversión de polaridad `[∅]` sobre la rama desfasada y el lazo de realimentación.
    pub phase_invert: bool,
    /// Desfase estéreo `[⚭]` entre los LFOs de los canales izquierdo y derecho.
    pub stereo: bool,
    /// Mezcla Dry/Wet (`0.0 ..= 1.0`, default `0.50` = máxima profundidad de cancelación).
    pub mix: f32,
    /// Sincronización opcional al tempo del proyecto (`false` por defecto = Hz).
    pub tempo_sync: bool,
    /// Índice de división rítmica `0..9` cuando `tempo_sync == true`.
    pub sync_div: u8,
}

impl Default for PhaserPlusParams {
    fn default() -> Self {
        Self {
            mode: PhaserPlusMode::Ehx,
            color: 0.35,
            feedback: 0.50,
            speed_norm: 0.65,
            depth: 0.50,
            phase_invert: false,
            stereo: false,
            mix: 0.50,
            tempo_sync: false,
            sync_div: 6, // 1/1 compás
        }
    }
}

impl PhaserPlusParams {
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
            sync_div: self.sync_div.min((PHASER_SYNC_BEATS.len() - 1) as u8),
        }
    }

    #[inline]
    pub fn speed_norm_to_hz(norm: f32) -> f32 {
        let ratio = PHASER_PLUS_MAX_SPEED_HZ / PHASER_PLUS_MIN_SPEED_HZ;
        PHASER_PLUS_MIN_SPEED_HZ * ratio.powf(norm.clamp(0.0, 1.0))
    }

    #[inline]
    pub fn speed_hz_to_norm(hz: f32) -> f32 {
        let ratio = PHASER_PLUS_MAX_SPEED_HZ / PHASER_PLUS_MIN_SPEED_HZ;
        ((hz.clamp(PHASER_PLUS_MIN_SPEED_HZ, PHASER_PLUS_MAX_SPEED_HZ) / PHASER_PLUS_MIN_SPEED_HZ)
            .ln()
            / ratio.ln())
        .clamp(0.0, 1.0)
    }

    /// Frecuencia efectiva del LFO en Hz para el modo actual (`Hz` libre o sincronizado a `BPM`).
    #[inline]
    pub fn effective_speed_hz(&self, tempo_bpm: f64) -> f64 {
        if self.tempo_sync {
            let idx = (self.sync_div as usize).min(PHASER_SYNC_BEATS.len() - 1);
            let cycle_beats = PHASER_SYNC_BEATS[idx] as f64;
            let bpm = tempo_bpm.clamp(20.0, 400.0);
            ((bpm / 60.0) / cycle_beats).clamp(0.005, 30.0)
        } else {
            Self::speed_norm_to_hz(self.speed_norm) as f64
        }
    }

    /// Frecuencia central nominal en Hz gobernada por `Color` según el modelo activo.
    #[inline]
    pub fn center_frequency_hz(&self) -> f32 {
        let (f_lo, f_hi) = self.mode.color_freq_bounds();
        (f_lo * (f_hi / f_lo).powf(self.color.clamp(0.0, 1.0) as f64)) as f32
    }

    pub fn format_speed_readout(&self, tempo_bpm: f64) -> String {
        if self.tempo_sync {
            let idx = (self.sync_div as usize).min(PHASER_SYNC_LABELS.len() - 1);
            format!(
                "{} ({:.2} Hz)",
                PHASER_SYNC_LABELS[idx],
                self.effective_speed_hz(tempo_bpm)
            )
        } else {
            let hz = Self::speed_norm_to_hz(self.speed_norm);
            if hz < 1.0 {
                format!("{hz:.2} Hz")
            } else if hz < 10.0 {
                format!("{hz:.2} Hz")
            } else {
                format!("{hz:.1} Hz")
            }
        }
    }

    pub fn format_color_readout(&self) -> String {
        let hz = self.center_frequency_hz();
        if hz >= 1000.0 {
            format!("{:.2} kHz", hz / 1000.0)
        } else {
            format!("{hz:.0} Hz")
        }
    }

    pub fn get_normalized(&self, id: u32) -> Option<f32> {
        match id {
            PHP_PARAM_MODE => Some((self.mode as u32) as f32 / 3.0),
            PHP_PARAM_COLOR => Some(self.color.clamp(0.0, 1.0)),
            PHP_PARAM_FEEDBACK => Some(self.feedback.clamp(0.0, 1.0)),
            PHP_PARAM_SPEED => Some(self.speed_norm.clamp(0.0, 1.0)),
            PHP_PARAM_DEPTH => Some(self.depth.clamp(0.0, 1.0)),
            PHP_PARAM_PHASE_INVERT => Some(if self.phase_invert { 1.0 } else { 0.0 }),
            PHP_PARAM_STEREO => Some(if self.stereo { 1.0 } else { 0.0 }),
            PHP_PARAM_MIX => Some(self.mix.clamp(0.0, 1.0)),
            PHP_PARAM_TEMPO_SYNC => Some(if self.tempo_sync { 1.0 } else { 0.0 }),
            PHP_PARAM_SYNC_DIV => {
                Some((self.sync_div as f32 / (PHASER_SYNC_BEATS.len() - 1) as f32).clamp(0.0, 1.0))
            }
            _ => None,
        }
    }

    pub fn set_normalized(&mut self, id: u32, value: f32) -> bool {
        if !value.is_finite() {
            return false;
        }
        let v = value.clamp(0.0, 1.0);
        match id {
            PHP_PARAM_MODE => self.mode = PhaserPlusMode::from_index((v * 3.0).round() as usize),
            PHP_PARAM_COLOR => self.color = v,
            PHP_PARAM_FEEDBACK => self.feedback = v,
            PHP_PARAM_SPEED => self.speed_norm = v,
            PHP_PARAM_DEPTH => self.depth = v,
            PHP_PARAM_PHASE_INVERT => self.phase_invert = v >= 0.5,
            PHP_PARAM_STEREO => self.stereo = v >= 0.5,
            PHP_PARAM_MIX => self.mix = v,
            PHP_PARAM_TEMPO_SYNC => self.tempo_sync = v >= 0.5,
            PHP_PARAM_SYNC_DIV => {
                self.sync_div = ((v * (PHASER_SYNC_BEATS.len() - 1) as f32).round() as u8)
                    .min((PHASER_SYNC_BEATS.len() - 1) as u8);
            }
            _ => return false,
        }
        *self = self.clamped();
        true
    }
}

// =============================================================================
// Primitivas DSP internas: All-Pass TPT, Filtros de Lazo y Sobremuestreo 2x
// =============================================================================

/// Etapa *all-pass* trapezoidal (TPT) de primer orden:
/// `H(s) = (wc - s) / (wc + s)` -> `+1` (`0°`) en DC, `-90°` en `fc`, `-1` (`-180°`) en Nyquist.
#[derive(Clone, Copy, Default)]
struct TptAllPass1 {
    ic1eq: f64,
}

impl TptAllPass1 {
    #[inline]
    fn process(&mut self, x: f64, g: f64) -> f64 {
        let big_g = g / (1.0 + g);
        let v = (x - self.ic1eq) * big_g;
        let lp = v + self.ic1eq;
        let next_s = lp + v;
        self.ic1eq = if next_s.abs() < 1.0e-30 { 0.0 } else { next_s };
        2.0 * lp - x
    }
}

/// Bloqueador DC / paso-alto suave de primer orden para estabilizar asimetría armónica y feedback.
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

/// Suavizador exponencial de un polo (por ejemplo, inercia óptica LDR en modo `GS`).
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

/// Saturador suave `tanh` con antialiasing por antiderivada de primer orden (**ADAA1**):
/// `f(x) = tanh(d * x) / tanh(d)`, antiderivada `F(x) = ln(cosh(d * x)) / (d * tanh(d))`,
/// latencia de grupo `0.5` muestras solo dentro del lazo no lineal y cero desfase en la ruta lineal.
#[derive(Clone, Copy, Default)]
struct AdaaTanh {
    prev_x: f64,
    prev_f: f64,
}

impl AdaaTanh {
    #[inline]
    fn antideriv(x: f64, drive: f64, inv_norm: f64) -> f64 {
        let u = (x * drive).clamp(-40.0, 40.0);
        // ln(cosh(u)) numéricamente estable: |u| + ln(1 + e^{-2|u|}) - ln(2)
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

#[inline]
fn soft_sat(x: f64, drive: f64) -> f64 {
    if drive <= 1.0e-6 {
        x
    } else {
        (x * drive).tanh() / drive.tanh().max(1.0e-6)
    }
}

/// Contorno de LFO según el circuito modelado (`phase` en `[0, 1)` -> salida bipolar `[-1, +1]`).
#[inline]
fn shaped_lfo_bipolar(mode: PhaserPlusMode, phase: f64) -> f64 {
    let p = phase - phase.floor();
    let s = (TAU * p).sin();
    let tri = 1.0 - 4.0 * ((p + 0.25).fract() - 0.5).abs();
    match mode {
        PhaserPlusMode::Gs => {
            // Célula óptica LDR: subida rápida de lámpara y caída exponencial suave
            let blend = 0.68 * s + 0.32 * tri;
            blend + 0.08 * (blend * blend - 0.5)
        }
        PhaserPlusMode::Ehx => {
            // Oscilador de relajación OTA ligeramente redondeado
            0.55 * s + 0.45 * tri
        }
        PhaserPlusMode::Mx => {
            // LFO triangular suavizado típico de pedal JFET
            0.35 * s + 0.65 * tri
        }
        PhaserPlusMode::Mf => {
            // Senoide pura de estudio
            s
        }
    }
}

// =============================================================================
// Procesador principal PhaserPlusEffect
// =============================================================================

pub struct PhaserPlusEffect {
    params: PhaserPlusParams,
    mod_offsets: [f32; PHASER_PLUS_PARAM_COUNT],
    sample_rate: f32,
    tempo_bpm: f64,

    // Fase maestra del LFO en [0, 1)
    lfo_phase: f64,
    // Suavizado óptico del LFO por canal
    ldr_smooth: [OnePoleLp; 2],

    // Estados TPT de hasta 12 etapas por canal
    stages: [[TptAllPass1; PHASER_PLUS_MAX_STAGES]; 2],
    // Retorno de realimentación de 1 muestra + filtros y ADAA1 del lazo
    fb_state: [f64; 2],
    fb_hp: [OnePoleHp; 2],
    fb_damp: [OnePoleLp; 2],
    fb_adaa: [AdaaTanh; 2],

    // Parámetros suavizados contra zipper-noise
    smooth_color: f64,
    smooth_fb: f64,
    smooth_depth: f64,
    smooth_mix: f64,
    slew_coeff: f64,

    last_mode: PhaserPlusMode,
}

impl PhaserPlusEffect {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        let params = PhaserPlusParams::default();
        let slew_coeff = 1.0 - (-1.0 / (0.0025 * (sr as f64))).exp();
        Self {
            params,
            mod_offsets: [0.0; PHASER_PLUS_PARAM_COUNT],
            sample_rate: sr,
            tempo_bpm: 120.0,
            lfo_phase: 0.0,
            ldr_smooth: [OnePoleLp::default(); 2],
            stages: [[TptAllPass1::default(); PHASER_PLUS_MAX_STAGES]; 2],
            fb_state: [0.0; 2],
            fb_hp: [OnePoleHp::default(); 2],
            fb_damp: [OnePoleLp::default(); 2],
            fb_adaa: [AdaaTanh::default(); 2],
            smooth_color: params.color as f64,
            smooth_fb: params.feedback as f64,
            smooth_depth: params.depth as f64,
            smooth_mix: params.mix as f64,
            slew_coeff,
            last_mode: params.mode,
        }
    }

    pub fn params(&self) -> PhaserPlusParams {
        self.params
    }

    pub fn set_params(&mut self, next: PhaserPlusParams) {
        let clamped = next.clamped();
        if clamped.mode != self.last_mode {
            let keep = clamped.mode.stage_count();
            for ch in 0..2 {
                for s in keep..PHASER_PLUS_MAX_STAGES {
                    self.stages[ch][s] = TptAllPass1::default();
                }
            }
            self.last_mode = clamped.mode;
        }
        self.params = clamped;
    }

    pub fn reset(&mut self) {
        self.lfo_phase = 0.0;
        self.ldr_smooth = [OnePoleLp::default(); 2];
        self.stages = [[TptAllPass1::default(); PHASER_PLUS_MAX_STAGES]; 2];
        self.fb_state = [0.0; 2];
        self.fb_hp = [OnePoleHp::default(); 2];
        self.fb_damp = [OnePoleLp::default(); 2];
        self.fb_adaa = [AdaaTanh::default(); 2];
    }

    #[inline]
    fn effective_params(&self) -> PhaserPlusParams {
        let mut p = self.params;
        let apply = |base: f32, idx: usize| (base + self.mod_offsets[idx]).clamp(0.0, 1.0);
        p.color = apply(p.color, PHP_PARAM_COLOR as usize);
        p.feedback = apply(p.feedback, PHP_PARAM_FEEDBACK as usize);
        p.speed_norm = apply(p.speed_norm, PHP_PARAM_SPEED as usize);
        p.depth = apply(p.depth, PHP_PARAM_DEPTH as usize);
        p.mix = apply(p.mix, PHP_PARAM_MIX as usize);
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
        let n_stages = mode.stage_count();
        let (f_lo, f_hi) = mode.color_freq_bounds();
        let base_fc = f_lo * (f_hi / f_lo).powf(self.smooth_color.clamp(0.0, 1.0));
        let max_oct = mode.max_sweep_octaves();
        let fb_sign = if eff.phase_invert { -1.0 } else { 1.0 };
        let fb_gain = self.smooth_fb.clamp(0.0, 1.0) * mode.max_feedback_gain() * fb_sign;
        let ratios = mode.stage_ratios();

        let stereo_off = if eff.stereo {
            mode.stereo_phase_offset()
        } else {
            0.0
        };

        let fb_hp_hz: f64 = match mode {
            PhaserPlusMode::Mx => 135.0,
            PhaserPlusMode::Ehx => 42.0,
            PhaserPlusMode::Gs => 28.0,
            PhaserPlusMode::Mf => 24.0,
        };
        let hp_g = (PI * fb_hp_hz.min(sr * 0.42) / sr).tan();
        let damp_coeff = 1.0 - (-TAU * 9_500.0_f64.min(sr * 0.44) / sr).exp();
        let ldr_coeff = 1.0 - (-TAU * 28.0 / sr).exp();

        let mut wet_out = [0.0_f64; 2];
        let nyquist_guard = (sr * 0.45).min(20_500.0);

        for ch in 0..2 {
            let ch_phase = self.lfo_phase + if ch == 1 { stereo_off } else { 0.0 };
            let raw_lfo = shaped_lfo_bipolar(mode, ch_phase);
            let mod_lfo = if mode == PhaserPlusMode::Gs {
                self.ldr_smooth[ch].process(raw_lfo, ldr_coeff)
            } else {
                raw_lfo
            };

            let comp_detune = if eff.stereo && ch == 1 { 1.018 } else { 1.0 };
            let sweep_oct = mod_lfo * self.smooth_depth * max_oct;
            let swept_fc = (base_fc * comp_detune * (2.0_f64).powf(sweep_oct)).clamp(20.0, nyquist_guard);

            let fb_filtered = self.fb_damp[ch].process(
                self.fb_hp[ch].process(self.fb_state[ch], hp_g),
                damp_coeff,
            );
            let fb_sat = match mode {
                PhaserPlusMode::Gs => self.fb_adaa[ch].process(fb_filtered, 0.75),
                PhaserPlusMode::Ehx => self.fb_adaa[ch].process(fb_filtered, 1.35) * 0.90,
                PhaserPlusMode::Mx => {
                    let biased = fb_filtered + 0.06 * fb_filtered * fb_filtered;
                    self.fb_adaa[ch].process(biased, 1.15)
                }
                PhaserPlusMode::Mf => self.fb_adaa[ch].process(fb_filtered, 1.25) * 0.92,
            };

            let mut sig = in_frame[ch] + fb_gain * fb_sat;
            let res_comp = (1.0 - 0.22 * self.smooth_fb * self.smooth_fb).sqrt();
            sig *= res_comp;

            for st_idx in 0..n_stages {
                let stage_fc = (swept_fc * ratios[st_idx]).clamp(18.0, nyquist_guard);
                let g = (PI * stage_fc / sr).tan();
                sig = self.stages[ch][st_idx].process(sig, g);
                if mode == PhaserPlusMode::Ehx && st_idx == 1 {
                    sig = soft_sat(sig, 0.55);
                }
            }

            self.fb_state[ch] = if sig.abs() < 1.0e-30 { 0.0 } else { sig };
            wet_out[ch] = sig;
        }

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

impl PluginInstance for PhaserPlusEffect {
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
        "Hikaru OpenPhaser+"
    }

    fn param_count(&mut self) -> usize {
        PHASER_PLUS_PARAM_COUNT
    }

    fn param_info(&mut self, index: usize) -> Option<PluginParamInfo> {
        let def = PhaserPlusParams::default();
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
            PHP_PARAM_MODE => Some(mk(0, "Character Mode", "", 0.0, 3.0, 3)),
            PHP_PARAM_COLOR => Some(mk(1, "Color", "%", 0.0, 100.0, 0)),
            PHP_PARAM_FEEDBACK => Some(mk(2, "FB", "%", 0.0, 100.0, 0)),
            PHP_PARAM_SPEED => Some(mk(
                3,
                "Speed",
                "Hz",
                PHASER_PLUS_MIN_SPEED_HZ,
                PHASER_PLUS_MAX_SPEED_HZ,
                0,
            )),
            PHP_PARAM_DEPTH => Some(mk(4, "Depth", "%", 0.0, 100.0, 0)),
            PHP_PARAM_PHASE_INVERT => Some(mk(5, "Polarity Invert", "", 0.0, 1.0, 1)),
            PHP_PARAM_STEREO => Some(mk(6, "Stereo Phase", "", 0.0, 1.0, 1)),
            PHP_PARAM_MIX => Some(mk(7, "Mix", "%", 0.0, 100.0, 0)),
            PHP_PARAM_TEMPO_SYNC => Some(mk(8, "Tempo Sync", "", 0.0, 1.0, 1)),
            PHP_PARAM_SYNC_DIV => Some(mk(9, "Sync Division", "", 0.0, 9.0, 9)),
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
        if idx < PHASER_PLUS_PARAM_COUNT && delta_normalized.is_finite() {
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
        let mut p = PhaserPlusParams::default();
        for mode in PhaserPlusMode::ALL {
            let norm = (mode as u32) as f32 / 3.0;
            assert!(p.set_normalized(PHP_PARAM_MODE, norm));
            assert_eq!(p.mode, mode);
        }
        for &(id, val) in &[
            (PHP_PARAM_COLOR, 0.73),
            (PHP_PARAM_FEEDBACK, 0.82),
            (PHP_PARAM_SPEED, 0.41),
            (PHP_PARAM_DEPTH, 0.67),
            (PHP_PARAM_PHASE_INVERT, 1.0),
            (PHP_PARAM_STEREO, 1.0),
            (PHP_PARAM_MIX, 0.45),
            (PHP_PARAM_TEMPO_SYNC, 1.0),
            (PHP_PARAM_SYNC_DIV, 4.0 / 9.0),
        ] {
            assert!(p.set_normalized(id, val));
            let got = p.get_normalized(id).expect("valid id");
            assert!((got - val).abs() < 1.0e-4, "id={id}: got={got}, expected={val}");
        }
    }

    #[test]
    fn dry_mix_zero_passes_signal_unchanged() {
        let mut fx = PhaserPlusEffect::new(48_000.0);
        fx.set_params(PhaserPlusParams {
            mix: 0.0,
            ..PhaserPlusParams::default()
        });
        // Dejar asentar el suavizador de mix
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
    fn all_four_modes_create_notches_and_remain_bounded_at_max_feedback() {
        let sr = 48_000.0;
        for mode in PhaserPlusMode::ALL {
            let mut fx = PhaserPlusEffect::new(sr);
            fx.set_params(PhaserPlusParams {
                mode,
                color: 0.50,
                feedback: 1.0,
                speed_norm: 0.60,
                depth: 0.85,
                phase_invert: false,
                stereo: true,
                mix: 0.50,
                ..PhaserPlusParams::default()
            });

            let mut max_abs = 0.0_f32;
            let mut diff_lr_energy = 0.0_f64;
            for i in 0..12_000 {
                // Señal rica en armónicos (diente de sierra suave)
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
    fn polarity_invert_cancels_sub_bass_at_fifty_percent_mix() {
        let sr = 48_000.0;
        let run = |invert: bool| -> f64 {
            let mut fx = PhaserPlusEffect::new(sr);
            fx.set_params(PhaserPlusParams {
                mode: PhaserPlusMode::Ehx,
                color: 0.90, // Muescas altas (~2.8 kHz) -> sub-grave (40 Hz) casi en fase 0°
                feedback: 0.0,
                depth: 0.0,
                phase_invert: invert,
                stereo: false,
                mix: 0.50,
                ..PhaserPlusParams::default()
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
