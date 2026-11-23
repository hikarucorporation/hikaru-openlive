// crates/hikaru_dsp/src/effects/bit8.rs

//! **Bit-8**: degradador digital y bitcrusher estilo **Bitwig Studio**.
//!
//! Replica la arquitectura y parámetros del dispositivo **Bit-8** de Bitwig:
//! - **Clock**: reducción de frecuencia de muestreo (`10 Hz .. 100 kHz`), **Jitter**
//!   (inestabilidad aleatoria del período de reloj) y **Chance** (probabilidad de captura).
//! - **Gate**: puerta de amplitud (`-96 dB .. 0 dB`, default `-58.2 dB`) con interruptor.
//! - **Shape**: rectificador de onda (`Normal`, `Half-wave`, `Full-wave`) y conformador /
//!   saturador (`Off`, `Clip`, `Soft`, `Fold`, `Wrap`, `Over`) con drive (`0 .. 36 dB`).
//! - **Quantize**: cuantización de amplitud en dB (`-96 dB .. 0 dB`, default `-39.1 dB`),
//!   modos `Off-zero` (mid-riser), `At-zero` (mid-tread), `Truncate` y `Ceil`, además de
//!   **Squared Domain** (compansión cuadrática no lineal).
//! - **Output**: **Anti-alias** (interpolación de reloj + filtro pre-aliasing + oversampling 2x
//!   en waveshaper), **Width** (ancho estéreo M/S + descorrelación L/R), **Mix** dry/wet y
//!   cadena interna **Wet FX** (HP, LP y Gain sobre la señal procesada).

use std::f32::consts::PI;

use hikaru_core::AudioBuffer;
use hikaru_plugin_host::{PluginInstance, PluginParamInfo};

pub const BIT8_MIN_CLOCK_HZ: f32 = 10.0;
pub const BIT8_MAX_CLOCK_HZ: f32 = 100_000.0;
pub const BIT8_MIN_DB: f32 = -96.0;
pub const BIT8_MAX_DB: f32 = 0.0;
pub const BIT8_MAX_DRIVE_DB: f32 = 36.0;

pub const BIT8_PARAM_CLOCK_HZ: u32 = 0;
pub const BIT8_PARAM_JITTER: u32 = 1;
pub const BIT8_PARAM_CHANCE: u32 = 2;
pub const BIT8_PARAM_GATE_ON: u32 = 3;
pub const BIT8_PARAM_GATE_DB: u32 = 4;
pub const BIT8_PARAM_SHAPE_ON: u32 = 5;
pub const BIT8_PARAM_RECTIFY: u32 = 6;
pub const BIT8_PARAM_SHAPE_MODE: u32 = 7;
pub const BIT8_PARAM_SHAPE_DRIVE_DB: u32 = 8;
pub const BIT8_PARAM_QUANTIZE_ON: u32 = 9;
pub const BIT8_PARAM_QUANTIZE_MODE: u32 = 10;
pub const BIT8_PARAM_QUANTIZE_DB: u32 = 11;
pub const BIT8_PARAM_SQUARED_DOMAIN: u32 = 12;
pub const BIT8_PARAM_ANTI_ALIAS: u32 = 13;
pub const BIT8_PARAM_WIDTH: u32 = 14;
pub const BIT8_PARAM_MIX: u32 = 15;
pub const BIT8_PARAM_WET_HP_HZ: u32 = 16;
pub const BIT8_PARAM_WET_LP_HZ: u32 = 17;
pub const BIT8_PARAM_WET_GAIN_DB: u32 = 18;
pub const BIT8_PARAM_COUNT: usize = 19;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bit8Rectify {
    /// Onda bipolar normal (`x`).
    Normal = 0,
    /// Rectificación de media onda (`x.max(0.0)`).
    HalfWave = 1,
    /// Rectificación de onda completa (`x.abs()`).
    FullWave = 2,
}

impl Bit8Rectify {
    pub const ALL: [Self; 3] = [Self::Normal, Self::HalfWave, Self::FullWave];

    pub fn from_norm(v: f32) -> Self {
        match (v.clamp(0.0, 1.0) * 2.0).round() as usize {
            0 => Self::Normal,
            1 => Self::HalfWave,
            _ => Self::FullWave,
        }
    }

    pub fn to_norm(self) -> f32 {
        (self as usize) as f32 / 2.0
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::HalfWave => "Half-wave",
            Self::FullWave => "Full-wave",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bit8ShapeMode {
    Off = 0,
    Clip = 1,
    Soft = 2,
    Fold = 3,
    Wrap = 4,
    Over = 5,
}

impl Bit8ShapeMode {
    pub const ALL: [Self; 6] = [
        Self::Off,
        Self::Clip,
        Self::Soft,
        Self::Fold,
        Self::Wrap,
        Self::Over,
    ];

    pub fn from_norm(v: f32) -> Self {
        let idx = (v.clamp(0.0, 1.0) * (Self::ALL.len() - 1) as f32).round() as usize;
        Self::ALL[idx.min(Self::ALL.len() - 1)]
    }

    pub fn to_norm(self) -> f32 {
        (self as usize) as f32 / (Self::ALL.len() - 1) as f32
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Clip => "Clip",
            Self::Soft => "Soft",
            Self::Fold => "Fold",
            Self::Wrap => "Wrap",
            Self::Over => "Over",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bit8QuantizeMode {
    /// Mid-riser: pasos desplazados del cero `(floor(u / q) + 0.5) * q`.
    OffZero = 0,
    /// Mid-tread: paso centrado en cero `round(u / q) * q`.
    AtZero = 1,
    /// Truncado hacia cero `trunc(u / q) * q`.
    Truncate = 2,
    /// Redondeo alejándose de cero `sign(u) * ceil(|u| / q) * q`.
    Ceil = 3,
}

impl Bit8QuantizeMode {
    pub const ALL: [Self; 4] = [Self::OffZero, Self::AtZero, Self::Truncate, Self::Ceil];

    pub fn from_norm(v: f32) -> Self {
        let idx = (v.clamp(0.0, 1.0) * (Self::ALL.len() - 1) as f32).round() as usize;
        Self::ALL[idx.min(Self::ALL.len() - 1)]
    }

    pub fn to_norm(self) -> f32 {
        (self as usize) as f32 / (Self::ALL.len() - 1) as f32
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::OffZero => "Off-zero",
            Self::AtZero => "At-zero",
            Self::Truncate => "Truncate",
            Self::Ceil => "Ceil",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bit8Params {
    /// Frecuencia de reloj de muestreo (`10.0 .. 100_000.0 Hz`).
    pub clock_hz: f32,
    /// Inestabilidad aleatoria del reloj (`0.0 .. 1.0`).
    pub jitter: f32,
    /// Probabilidad de captura por pulso de reloj (`0.0 .. 1.0`).
    pub chance: f32,
    /// Activa la sección Gate.
    pub gate_enabled: bool,
    /// Umbral del Gate (`-96.0 .. 0.0 dB`).
    pub gate_db: f32,
    /// Activa la sección Shape.
    pub shape_enabled: bool,
    /// Rectificación (`Normal`, `HalfWave`, `FullWave`).
    pub rectify: Bit8Rectify,
    /// Algoritmo de waveshaping (`Off`, `Clip`, `Soft`, `Fold`, `Wrap`, `Over`).
    pub shape_mode: Bit8ShapeMode,
    /// Drive del waveshaper (`0.0 .. 36.0 dB`).
    pub shape_drive_db: f32,
    /// Activa la sección Quantize.
    pub quantize_enabled: bool,
    /// Modo de cuantización (`OffZero`, `AtZero`, `Truncate`, `Ceil`).
    pub quantize_mode: Bit8QuantizeMode,
    /// Tamaño de paso de cuantización en dB (`-96.0 .. 0.0 dB`).
    pub quantize_db: f32,
    /// Cuantización en dominio cuadrático no lineal (`Squared Domain`).
    pub squared_domain: bool,
    /// Procesamiento con reducción de aliasing (`Anti-alias`).
    pub anti_alias: bool,
    /// Ancho estéreo y descorrelación L/R (`0.0 .. 1.0`).
    pub width: f32,
    /// Mezcla Dry/Wet (`0.0 .. 1.0`).
    pub mix: f32,
    /// Filtro paso-alto en Wet FX (`20.0 .. 2000.0 Hz`, 20 Hz = bypass).
    pub wet_hp_hz: f32,
    /// Filtro paso-bajo en Wet FX (`1000.0 .. 20_000.0 Hz`, 20 kHz = bypass).
    pub wet_lp_hz: f32,
    /// Ganancia de salida en Wet FX (`-12.0 .. 12.0 dB`).
    pub wet_gain_db: f32,
}

impl Default for Bit8Params {
    fn default() -> Self {
        // Valores por defecto idénticos al Bit-8 de Bitwig Studio:
        // Clock 100.0 kHz, Jitter 100%, Chance 100%, Gate -58.2 dB,
        // Shape Normal/Off, Quantize Off-zero -39.1 dB, Width 100%, Mix 100%.
        Self {
            clock_hz: 100_000.0,
            jitter: 1.0,
            chance: 1.0,
            gate_enabled: true,
            gate_db: -58.2,
            shape_enabled: true,
            rectify: Bit8Rectify::Normal,
            shape_mode: Bit8ShapeMode::Off,
            shape_drive_db: 12.0,
            quantize_enabled: true,
            quantize_mode: Bit8QuantizeMode::OffZero,
            quantize_db: -39.1,
            squared_domain: false,
            anti_alias: false,
            width: 1.0,
            mix: 1.0,
            wet_hp_hz: 20.0,
            wet_lp_hz: 20_000.0,
            wet_gain_db: 0.0,
        }
    }
}

impl Bit8Params {
    #[inline]
    pub fn clock_norm_to_hz(norm: f32) -> f32 {
        let n = norm.clamp(0.0, 1.0);
        BIT8_MIN_CLOCK_HZ * (BIT8_MAX_CLOCK_HZ / BIT8_MIN_CLOCK_HZ).powf(n)
    }

    #[inline]
    pub fn clock_hz_to_norm(hz: f32) -> f32 {
        let clamped = hz.clamp(BIT8_MIN_CLOCK_HZ, BIT8_MAX_CLOCK_HZ);
        (clamped / BIT8_MIN_CLOCK_HZ).ln() / (BIT8_MAX_CLOCK_HZ / BIT8_MIN_CLOCK_HZ).ln()
    }

    #[inline]
    pub fn db_norm_to_val(norm: f32) -> f32 {
        BIT8_MIN_DB + norm.clamp(0.0, 1.0) * (BIT8_MAX_DB - BIT8_MIN_DB)
    }

    #[inline]
    pub fn db_val_to_norm(db: f32) -> f32 {
        ((db - BIT8_MIN_DB) / (BIT8_MAX_DB - BIT8_MIN_DB)).clamp(0.0, 1.0)
    }

    #[inline]
    pub fn wet_hp_norm_to_hz(norm: f32) -> f32 {
        20.0 * (2000.0_f32 / 20.0).powf(norm.clamp(0.0, 1.0))
    }

    #[inline]
    pub fn wet_hp_hz_to_norm(hz: f32) -> f32 {
        (hz.clamp(20.0, 2000.0) / 20.0).ln() / (2000.0_f32 / 20.0).ln()
    }

    #[inline]
    pub fn wet_lp_norm_to_hz(norm: f32) -> f32 {
        1000.0 * (20_000.0_f32 / 1000.0).powf(norm.clamp(0.0, 1.0))
    }

    #[inline]
    pub fn wet_lp_hz_to_norm(hz: f32) -> f32 {
        (hz.clamp(1000.0, 20_000.0) / 1000.0).ln() / (20_000.0_f32 / 1000.0).ln()
    }

    #[inline]
    pub fn clamped(self) -> Self {
        Self {
            clock_hz: self.clock_hz.clamp(BIT8_MIN_CLOCK_HZ, BIT8_MAX_CLOCK_HZ),
            jitter: self.jitter.clamp(0.0, 1.0),
            chance: self.chance.clamp(0.0, 1.0),
            gate_enabled: self.gate_enabled,
            gate_db: self.gate_db.clamp(BIT8_MIN_DB, BIT8_MAX_DB),
            shape_enabled: self.shape_enabled,
            rectify: self.rectify,
            shape_mode: self.shape_mode,
            shape_drive_db: self.shape_drive_db.clamp(0.0, BIT8_MAX_DRIVE_DB),
            quantize_enabled: self.quantize_enabled,
            quantize_mode: self.quantize_mode,
            quantize_db: self.quantize_db.clamp(BIT8_MIN_DB, BIT8_MAX_DB),
            squared_domain: self.squared_domain,
            anti_alias: self.anti_alias,
            width: self.width.clamp(0.0, 1.0),
            mix: self.mix.clamp(0.0, 1.0),
            wet_hp_hz: self.wet_hp_hz.clamp(20.0, 2000.0),
            wet_lp_hz: self.wet_lp_hz.clamp(1000.0, 20_000.0),
            wet_gain_db: self.wet_gain_db.clamp(-12.0, 12.0),
        }
    }

    /// Evalua la función de transferencia estática (`Gate` + `Shape` + `Quantize`)
    /// para la visualización en tiempo real del display de Bit-8.
    pub fn eval_transfer(&self, x: f32) -> f32 {
        let mut s = x;
        if self.gate_enabled && self.gate_db > BIT8_MIN_DB + 0.5 {
            let thresh = db_to_lin(self.gate_db);
            if s.abs() < thresh {
                return 0.0;
            }
        }
        if self.shape_enabled {
            s = apply_rectify(s, self.rectify);
            s = apply_shape_mode(s, self.shape_mode, db_to_lin(self.shape_drive_db));
        }
        if self.quantize_enabled && self.quantize_db > BIT8_MIN_DB + 0.5 {
            s = apply_quantize(
                s,
                db_to_lin(self.quantize_db),
                self.quantize_mode,
                self.squared_domain,
            );
        }
        s.clamp(-1.0, 1.0)
    }

    /// Devuelve el tamaño de paso lineal correspondiente a `quantize_db`.
    #[inline]
    pub fn quantize_step(&self) -> f32 {
        db_to_lin(self.quantize_db).clamp(1.0e-5, 1.0)
    }

    /// Convierte el parámetro `id` a valor normalizado `0.0 ..= 1.0`.
    pub fn get_normalized(&self, id: u32) -> Option<f32> {
        match id {
            BIT8_PARAM_CLOCK_HZ => Some(Self::clock_hz_to_norm(self.clock_hz)),
            BIT8_PARAM_JITTER => Some(self.jitter),
            BIT8_PARAM_CHANCE => Some(self.chance),
            BIT8_PARAM_GATE_ON => Some(if self.gate_enabled { 1.0 } else { 0.0 }),
            BIT8_PARAM_GATE_DB => Some(Self::db_val_to_norm(self.gate_db)),
            BIT8_PARAM_SHAPE_ON => Some(if self.shape_enabled { 1.0 } else { 0.0 }),
            BIT8_PARAM_RECTIFY => Some(self.rectify.to_norm()),
            BIT8_PARAM_SHAPE_MODE => Some(self.shape_mode.to_norm()),
            BIT8_PARAM_SHAPE_DRIVE_DB => Some((self.shape_drive_db / BIT8_MAX_DRIVE_DB).clamp(0.0, 1.0)),
            BIT8_PARAM_QUANTIZE_ON => Some(if self.quantize_enabled { 1.0 } else { 0.0 }),
            BIT8_PARAM_QUANTIZE_MODE => Some(self.quantize_mode.to_norm()),
            BIT8_PARAM_QUANTIZE_DB => Some(Self::db_val_to_norm(self.quantize_db)),
            BIT8_PARAM_SQUARED_DOMAIN => Some(if self.squared_domain { 1.0 } else { 0.0 }),
            BIT8_PARAM_ANTI_ALIAS => Some(if self.anti_alias { 1.0 } else { 0.0 }),
            BIT8_PARAM_WIDTH => Some(self.width),
            BIT8_PARAM_MIX => Some(self.mix),
            BIT8_PARAM_WET_HP_HZ => Some(Self::wet_hp_hz_to_norm(self.wet_hp_hz)),
            BIT8_PARAM_WET_LP_HZ => Some(Self::wet_lp_hz_to_norm(self.wet_lp_hz)),
            BIT8_PARAM_WET_GAIN_DB => Some(((self.wet_gain_db + 12.0) / 24.0).clamp(0.0, 1.0)),
            _ => None,
        }
    }

    /// Aplica un valor normalizado `0.0 ..= 1.0` al parámetro `id`.
    pub fn set_normalized(&mut self, id: u32, value: f32) -> bool {
        let v = value.clamp(0.0, 1.0);
        match id {
            BIT8_PARAM_CLOCK_HZ => self.clock_hz = Self::clock_norm_to_hz(v),
            BIT8_PARAM_JITTER => self.jitter = v,
            BIT8_PARAM_CHANCE => self.chance = v,
            BIT8_PARAM_GATE_ON => self.gate_enabled = v >= 0.5,
            BIT8_PARAM_GATE_DB => self.gate_db = Self::db_norm_to_val(v),
            BIT8_PARAM_SHAPE_ON => self.shape_enabled = v >= 0.5,
            BIT8_PARAM_RECTIFY => self.rectify = Bit8Rectify::from_norm(v),
            BIT8_PARAM_SHAPE_MODE => self.shape_mode = Bit8ShapeMode::from_norm(v),
            BIT8_PARAM_SHAPE_DRIVE_DB => self.shape_drive_db = v * BIT8_MAX_DRIVE_DB,
            BIT8_PARAM_QUANTIZE_ON => self.quantize_enabled = v >= 0.5,
            BIT8_PARAM_QUANTIZE_MODE => self.quantize_mode = Bit8QuantizeMode::from_norm(v),
            BIT8_PARAM_QUANTIZE_DB => self.quantize_db = Self::db_norm_to_val(v),
            BIT8_PARAM_SQUARED_DOMAIN => self.squared_domain = v >= 0.5,
            BIT8_PARAM_ANTI_ALIAS => self.anti_alias = v >= 0.5,
            BIT8_PARAM_WIDTH => self.width = v,
            BIT8_PARAM_MIX => self.mix = v,
            BIT8_PARAM_WET_HP_HZ => self.wet_hp_hz = Self::wet_hp_norm_to_hz(v),
            BIT8_PARAM_WET_LP_HZ => self.wet_lp_hz = Self::wet_lp_norm_to_hz(v),
            BIT8_PARAM_WET_GAIN_DB => self.wet_gain_db = -12.0 + v * 24.0,
            _ => return false,
        }
        *self = self.clamped();
        true
    }
}

#[inline]
fn db_to_lin(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

#[inline]
fn apply_rectify(x: f32, rectify: Bit8Rectify) -> f32 {
    match rectify {
        Bit8Rectify::Normal => x,
        Bit8Rectify::HalfWave => x.max(0.0),
        Bit8Rectify::FullWave => x.abs(),
    }
}

#[inline]
fn fold_triangle(x: f32) -> f32 {
    // Pliega continuamente cualquier amplitud dentro de [-1.0, 1.0].
    let t = (x + 1.0).rem_euclid(4.0);
    if t <= 2.0 {
        t - 1.0
    } else {
        3.0 - t
    }
}

#[inline]
fn apply_shape_mode(x: f32, mode: Bit8ShapeMode, drive: f32) -> f32 {
    match mode {
        Bit8ShapeMode::Off => x,
        Bit8ShapeMode::Clip => (x * drive).clamp(-1.0, 1.0),
        Bit8ShapeMode::Soft => (x * drive).tanh(),
        Bit8ShapeMode::Fold => fold_triangle(x * drive),
        Bit8ShapeMode::Wrap => ((x * drive + 1.0).rem_euclid(2.0)) - 1.0,
        Bit8ShapeMode::Over => {
            let d = x * drive;
            (d + 0.18 * d * d).tanh()
        }
    }
}

#[inline]
fn apply_quantize(x: f32, step: f32, mode: Bit8QuantizeMode, squared_domain: bool) -> f32 {
    if x.abs() < 1.0e-7 {
        return 0.0;
    }
    let step = step.clamp(1.0e-5, 1.0);
    let u = if squared_domain {
        x.signum() * x.abs().sqrt()
    } else {
        x
    };
    let q = match mode {
        Bit8QuantizeMode::OffZero => ((u / step).floor() + 0.5) * step,
        Bit8QuantizeMode::AtZero => (u / step).round() * step,
        Bit8QuantizeMode::Truncate => (u / step).trunc() * step,
        Bit8QuantizeMode::Ceil => u.signum() * (u.abs() / step).ceil() * step,
    };
    let out = if squared_domain {
        q.signum() * q * q
    } else {
        q
    };
    if out.is_finite() {
        out.clamp(-2.0, 2.0)
    } else {
        0.0
    }
}

#[derive(Debug, Clone)]
struct ChannelState {
    phase: f32,
    step_inc: f32,
    hold: f32,
    prev_hold: f32,
    prev_shaper_in: f32,
    aa_pre_lp: f32,
    aa_post_lp: f32,
    gate_env: f32,
    gate_gain: f32,
    wet_hp_prev_in: f32,
    wet_hp_prev_out: f32,
    wet_lp_state: f32,
    rng_state: u64,
}

impl ChannelState {
    fn new(seed: u64) -> Self {
        Self {
            phase: 1.0,
            step_inc: 1.0,
            hold: 0.0,
            prev_hold: 0.0,
            prev_shaper_in: 0.0,
            aa_pre_lp: 0.0,
            aa_post_lp: 0.0,
            gate_env: 0.0,
            gate_gain: 1.0,
            wet_hp_prev_in: 0.0,
            wet_hp_prev_out: 0.0,
            wet_lp_state: 0.0,
            rng_state: seed.max(1),
        }
    }

    #[inline]
    fn next_u32(&mut self) -> u32 {
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng_state = x;
        (x >> 32) as u32
    }

    #[inline]
    fn next_unit(&mut self) -> f32 {
        (self.next_u32() as f32) * (1.0 / (u32::MAX as f32))
    }

    #[inline]
    fn next_bipolar(&mut self) -> f32 {
        self.next_unit() * 2.0 - 1.0
    }
}

pub struct Bit8Effect {
    sample_rate: f32,
    params: Bit8Params,
    ch_l: ChannelState,
    ch_r: ChannelState,
}

impl Bit8Effect {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate: sample_rate.max(8_000.0),
            params: Bit8Params::default(),
            ch_l: ChannelState::new(0x9E37_79B9_7F4A_7C15),
            ch_r: ChannelState::new(0xD1B5_4A32_D192_ED03),
        }
    }

    pub fn params(&self) -> Bit8Params {
        self.params
    }

    pub fn set_params(&mut self, params: Bit8Params) {
        self.params = params.clamped();
    }

    #[inline]
    fn process_channel(
        ch: &mut ChannelState,
        input: f32,
        params: &Bit8Params,
        sr: f32,
        shared_rand: Option<(f32, f32)>,
    ) -> f32 {
        let x_in = if input.is_finite() { input } else { 0.0 };

        // 1. Pre-clock anti-alias low-pass cuando Anti-alias está activo y Clock < sr.
        let clock_active = params.clock_hz < sr * 0.995;
        let pre_x = if params.anti_alias && clock_active {
            let fc = (params.clock_hz * 0.45).clamp(20.0, sr * 0.48);
            let alpha = (2.0 * PI * fc / sr).clamp(0.001, 1.0);
            ch.aa_pre_lp += alpha * (x_in - ch.aa_pre_lp);
            ch.aa_pre_lp
        } else {
            ch.aa_pre_lp = x_in;
            x_in
        };

        // 2. Clock (Sample & Hold + Jitter + Chance).
        let after_clock = if !clock_active && params.chance >= 0.999 {
            ch.prev_hold = ch.hold;
            ch.hold = pre_x;
            pre_x
        } else {
            let base_inc = (params.clock_hz / sr).clamp(1.0e-5, 1.0);
            if !clock_active {
                // Solo Chance activo a tasa nativa.
                let r_chance = shared_rand.map(|(_, c)| c).unwrap_or_else(|| ch.next_unit());
                if r_chance <= params.chance {
                    ch.prev_hold = ch.hold;
                    ch.hold = pre_x;
                }
                ch.hold
            } else {
                if ch.step_inc <= 0.0 || ch.step_inc > 1.0 {
                    ch.step_inc = base_inc;
                }
                ch.phase += ch.step_inc;
                if ch.phase >= 1.0 {
                    ch.phase -= ch.phase.floor();
                    let (r_jit, r_chance) = shared_rand.unwrap_or_else(|| {
                        (ch.next_bipolar(), ch.next_unit())
                    });
                    let jitter_mod = (1.0 + r_jit * params.jitter * 0.45).max(0.15);
                    ch.step_inc = (base_inc / jitter_mod).clamp(1.0e-5, 1.0);
                    if r_chance <= params.chance {
                        ch.prev_hold = ch.hold;
                        ch.hold = pre_x;
                    }
                }
                if params.anti_alias {
                    // Interpolación lineal (first-order hold) para suavizar escalones duros.
                    let t = ch.phase.clamp(0.0, 1.0);
                    ch.prev_hold + (ch.hold - ch.prev_hold) * t
                } else {
                    ch.hold
                }
            }
        };

        // 3. Gate (seguidor de envolvente rápido para evitar clics y silenciar piso de ruido).
        let gate_gain = if params.gate_enabled && params.gate_db > BIT8_MIN_DB + 0.5 {
            let thresh = db_to_lin(params.gate_db);
            let env_in = after_clock.abs();
            let atk = 1.0 - (-1.0 / (0.0003 * sr)).exp();
            let rel = 1.0 - (-1.0 / (0.018 * sr)).exp();
            let c = if env_in > ch.gate_env { atk } else { rel };
            ch.gate_env += c * (env_in - ch.gate_env);
            let target = if ch.gate_env >= thresh { 1.0 } else { 0.0 };
            let g_c = if target > ch.gate_gain { atk } else { rel };
            ch.gate_gain += g_c * (target - ch.gate_gain);
            ch.gate_gain
        } else {
            ch.gate_env = after_clock.abs();
            ch.gate_gain = 1.0;
            1.0
        };

        let after_gate = after_clock * gate_gain;

        // 4. Shape (Rectify + Waveshaper, con 2x oversampling si Anti-alias está ON).
        let after_shape = if params.shape_enabled {
            let drive = db_to_lin(params.shape_drive_db);
            if params.anti_alias && params.shape_mode != Bit8ShapeMode::Off {
                let mid = 0.5 * (ch.prev_shaper_in + after_gate);
                ch.prev_shaper_in = after_gate;
                let y0 = apply_shape_mode(apply_rectify(mid, params.rectify), params.shape_mode, drive);
                let y1 = apply_shape_mode(apply_rectify(after_gate, params.rectify), params.shape_mode, drive);
                0.5 * (y0 + y1)
            } else {
                ch.prev_shaper_in = after_gate;
                apply_shape_mode(apply_rectify(after_gate, params.rectify), params.shape_mode, drive)
            }
        } else {
            ch.prev_shaper_in = after_gate;
            after_gate
        };

        // 5. Quantize (Off-zero / At-zero / Truncate / Ceil + Squared Domain).
        let after_quant = if params.quantize_enabled && params.quantize_db > BIT8_MIN_DB + 0.5 {
            if gate_gain < 1.0e-4 {
                0.0
            } else {
                let step = params.quantize_step();
                apply_quantize(after_shape, step, params.quantize_mode, params.squared_domain) * gate_gain
            }
        } else {
            after_shape
        };

        // 6. Post anti-alias smoothing suave cuando Anti-alias está activo.
        let after_aa = if params.anti_alias {
            let fc = (sr * 0.38).min(18_000.0);
            let alpha = (2.0 * PI * fc / sr).clamp(0.05, 1.0);
            ch.aa_post_lp += alpha * (after_quant - ch.aa_post_lp);
            ch.aa_post_lp
        } else {
            ch.aa_post_lp = after_quant;
            after_quant
        };

        // 7. Wet FX interno (HP, LP y Gain sobre la señal wet).
        let mut wet = after_aa;
        if params.wet_hp_hz > 20.5 {
            let rc = 1.0 / (2.0 * PI * params.wet_hp_hz);
            let dt = 1.0 / sr;
            let a = rc / (rc + dt);
            let hp = a * (ch.wet_hp_prev_out + wet - ch.wet_hp_prev_in);
            ch.wet_hp_prev_in = wet;
            ch.wet_hp_prev_out = hp;
            wet = hp;
        } else {
            ch.wet_hp_prev_in = wet;
            ch.wet_hp_prev_out = wet;
        }
        if params.wet_lp_hz < 19_900.0 {
            let rc = 1.0 / (2.0 * PI * params.wet_lp_hz);
            let dt = 1.0 / sr;
            let a = dt / (rc + dt);
            ch.wet_lp_state += a * (wet - ch.wet_lp_state);
            wet = ch.wet_lp_state;
        } else {
            ch.wet_lp_state = wet;
        }
        if params.wet_gain_db.abs() > 0.01 {
            wet *= db_to_lin(params.wet_gain_db);
        }

        if wet.is_finite() {
            wet.clamp(-2.0, 2.0)
        } else {
            0.0
        }
    }

    #[inline]
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        let p = self.params;
        let sr = self.sample_rate;

        // Cuando Width == 0 (mono), L y R comparten exactamente los mismos eventos de Jitter/Chance.
        let linked = p.width <= 0.001;
        let shared = if linked {
            Some((self.ch_l.next_bipolar(), self.ch_l.next_unit()))
        } else {
            None
        };

        let wl_raw = Self::process_channel(&mut self.ch_l, left, &p, sr, shared);
        let wr_raw = Self::process_channel(&mut self.ch_r, right, &p, sr, shared);

        // 8. Width (Mid/Side sobre la señal wet).
        let mid = 0.5 * (wl_raw + wr_raw);
        let side = 0.5 * (wl_raw - wr_raw) * p.width;
        let wl = mid + side;
        let wr = mid - side;

        // 9. Dry / Wet Mix.
        let m = p.mix;
        (
            left * (1.0 - m) + wl * m,
            right * (1.0 - m) + wr * m,
        )
    }
}

impl PluginInstance for Bit8Effect {
    fn process(&mut self, buffer: &mut AudioBuffer) {
        for frame in buffer.samples.chunks_exact_mut(2) {
            let (l, r) = self.process_stereo(frame[0], frame[1]);
            frame[0] = l;
            frame[1] = r;
        }
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(8_000.0);
    }

    fn show_gui_embedded(&mut self, _: hikaru_plugin_host::RawWindowHandle) -> Result<(), String> {
        Err("Native plugin".to_string())
    }

    fn show_gui_floating(&mut self) -> Result<(), String> {
        Err("Native plugin".to_string())
    }

    fn hide_gui(&mut self) {}

    fn get_name(&self) -> &str {
        "Bit-8"
    }

    fn param_count(&mut self) -> usize {
        BIT8_PARAM_COUNT
    }

    fn param_info(&mut self, index: usize) -> Option<PluginParamInfo> {
        let def = Bit8Params::default();
        let mk = |id: u32, name: &str, unit: &str, min: f32, max: f32, step_count: i32| PluginParamInfo {
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
            BIT8_PARAM_CLOCK_HZ => Some(mk(0, "Clock", "Hz", BIT8_MIN_CLOCK_HZ, BIT8_MAX_CLOCK_HZ, 0)),
            BIT8_PARAM_JITTER => Some(mk(1, "Jitter", "%", 0.0, 100.0, 0)),
            BIT8_PARAM_CHANCE => Some(mk(2, "Chance", "%", 0.0, 100.0, 0)),
            BIT8_PARAM_GATE_ON => Some(mk(3, "Gate On", "", 0.0, 1.0, 1)),
            BIT8_PARAM_GATE_DB => Some(mk(4, "Gate", "dB", BIT8_MIN_DB, BIT8_MAX_DB, 0)),
            BIT8_PARAM_SHAPE_ON => Some(mk(5, "Shape On", "", 0.0, 1.0, 1)),
            BIT8_PARAM_RECTIFY => Some(mk(6, "Rectify", "", 0.0, 2.0, 2)),
            BIT8_PARAM_SHAPE_MODE => Some(mk(7, "Shape Mode", "", 0.0, 5.0, 5)),
            BIT8_PARAM_SHAPE_DRIVE_DB => Some(mk(8, "Shape Drive", "dB", 0.0, BIT8_MAX_DRIVE_DB, 0)),
            BIT8_PARAM_QUANTIZE_ON => Some(mk(9, "Quantize On", "", 0.0, 1.0, 1)),
            BIT8_PARAM_QUANTIZE_MODE => Some(mk(10, "Quantize Mode", "", 0.0, 3.0, 3)),
            BIT8_PARAM_QUANTIZE_DB => Some(mk(11, "Quantize", "dB", BIT8_MIN_DB, BIT8_MAX_DB, 0)),
            BIT8_PARAM_SQUARED_DOMAIN => Some(mk(12, "Squared Domain", "", 0.0, 1.0, 1)),
            BIT8_PARAM_ANTI_ALIAS => Some(mk(13, "Anti-alias", "", 0.0, 1.0, 1)),
            BIT8_PARAM_WIDTH => Some(mk(14, "Width", "%", 0.0, 100.0, 0)),
            BIT8_PARAM_MIX => Some(mk(15, "Mix", "%", 0.0, 100.0, 0)),
            BIT8_PARAM_WET_HP_HZ => Some(mk(16, "Wet HP", "Hz", 20.0, 2000.0, 0)),
            BIT8_PARAM_WET_LP_HZ => Some(mk(17, "Wet LP", "Hz", 1000.0, 20_000.0, 0)),
            BIT8_PARAM_WET_GAIN_DB => Some(mk(18, "Wet Gain", "dB", -12.0, 12.0, 0)),
            _ => None,
        }
    }

    fn get_param_normalized(&mut self, id: u32) -> Option<f32> {
        self.params.get_normalized(id)
    }

    fn set_param_normalized(&mut self, id: u32, value: f32) -> bool {
        self.params.set_normalized(id, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit8_quantize_at_zero_and_off_zero_differ() {
        let mut fx = Bit8Effect::new(48_000.0);
        let base = Bit8Params {
            gate_enabled: false,
            quantize_db: -12.0, // step ~= 0.251
            ..Bit8Params::default()
        };

        fx.set_params(Bit8Params {
            quantize_mode: Bit8QuantizeMode::AtZero,
            ..base
        });
        let (at_zero, _) = fx.process_stereo(0.10, 0.10);

        fx.set_params(Bit8Params {
            quantize_mode: Bit8QuantizeMode::OffZero,
            ..base
        });
        let (off_zero, _) = fx.process_stereo(0.10, 0.10);

        // Con paso ~0.251, 0.10 en At-zero cae en 0.0; en Off-zero cae en +0.1255.
        assert!(at_zero.abs() < 1e-4, "expected ~0.0, got {at_zero}");
        assert!(off_zero > 0.1, "expected mid-riser offset > 0.1, got {off_zero}");
    }

    #[test]
    fn bit8_rectify_half_and_full_wave() {
        let mut fx = Bit8Effect::new(48_000.0);
        let base = Bit8Params {
            gate_enabled: false,
            quantize_enabled: false,
            ..Bit8Params::default()
        };

        fx.set_params(Bit8Params {
            rectify: Bit8Rectify::HalfWave,
            ..base
        });
        let (half, _) = fx.process_stereo(-0.5, -0.5);
        assert!(half.abs() < 1e-5, "half-wave should zero negative, got {half}");

        fx.set_params(Bit8Params {
            rectify: Bit8Rectify::FullWave,
            ..base
        });
        let (full, _) = fx.process_stereo(-0.5, -0.5);
        assert!((full - 0.5).abs() < 1e-5, "full-wave should flip negative to +0.5, got {full}");
    }

    #[test]
    fn bit8_gate_silences_quiet_signal_and_prevents_dc() {
        let mut fx = Bit8Effect::new(48_000.0);
        fx.set_params(Bit8Params {
            gate_enabled: true,
            gate_db: -20.0,
            quantize_enabled: true,
            quantize_mode: Bit8QuantizeMode::OffZero,
            quantize_db: -12.0,
            ..Bit8Params::default()
        });
        // Dejar decaer la envolvente de gate sobre silencio.
        let mut last = 1.0;
        for _ in 0..4096 {
            let (l, _) = fx.process_stereo(0.0, 0.0);
            last = l;
        }
        assert!(last.abs() < 1e-5, "silence under gate must have zero DC, got {last}");
    }

    #[test]
    fn bit8_clock_downsamples_and_width_collapses_stereo() {
        let mut fx = Bit8Effect::new(48_000.0);
        fx.set_params(Bit8Params {
            clock_hz: 1_000.0,
            jitter: 0.0,
            chance: 1.0,
            gate_enabled: false,
            quantize_enabled: false,
            width: 0.0,
            ..Bit8Params::default()
        });
        let (l, r) = fx.process_stereo(0.8, -0.4);
        assert!((l - r).abs() < 1e-5, "width 0 should collapse to mono: L={l}, R={r}");
        assert!((l - 0.2).abs() < 1e-4, "expected mono average 0.2, got {l}");
    }

    #[test]
    fn bit8_squared_domain_preserves_finer_low_level_steps() {
        let p_lin = Bit8Params {
            gate_enabled: false,
            quantize_enabled: true,
            quantize_mode: Bit8QuantizeMode::AtZero,
            quantize_db: -12.0,
            squared_domain: false,
            ..Bit8Params::default()
        };
        let p_sq = Bit8Params {
            squared_domain: true,
            ..p_lin
        };
        // En señal baja (0.08), lineal con paso 0.251 la aplasta a 0.0;
        // Squared Domain toma sqrt(0.08) ~= 0.283 -> cuantiza a 0.251 -> eleva al cuadrado ~= 0.063.
        let y_lin = p_lin.eval_transfer(0.08);
        let y_sq = p_sq.eval_transfer(0.08);
        assert!(y_lin.abs() < 1e-5);
        assert!(y_sq > 0.04, "squared domain should keep low-level detail, got {y_sq}");
    }

    #[test]
    fn bit8_param_normalized_roundtrip() {
        let mut fx = Bit8Effect::new(48_000.0);
        for id in 0..(BIT8_PARAM_COUNT as u32) {
            let v = fx.get_param_normalized(id).expect("valid param id");
            assert!(fx.set_param_normalized(id, v));
            let v2 = fx.get_param_normalized(id).unwrap();
            assert!((v - v2).abs() < 1e-3, "param {id} drifted: {v} vs {v2}");
        }
    }
}
