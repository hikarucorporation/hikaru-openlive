//! **Hikaru OpenEQ+**: ecualizador paramétrico de 8 bandas con analizador de espectro
//! Pre/Post, desplazamiento global en semitonos (`Shift`), ganancia de salida (`Gain`),
//! rango visual (`±10 / ±20 / ±30 dB`), `Adaptive Q`, `Solo` de banda y 15 tipos de filtro
//! compatibles con el contrato público de **EQ+** de Bitwig Studio.

use std::collections::HashMap;
use std::f64::consts::{LN_2, PI, SQRT_2};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use hikaru_core::AudioBuffer;
use hikaru_plugin_host::{HostTransportInfo, PluginInstance, PluginParamInfo};
use realfft::RealFftPlanner;

pub const EQ_PLUS_BAND_COUNT: usize = 8;
pub const EQ_PLUS_MIN_FREQ_HZ: f32 = 10.0;
pub const EQ_PLUS_MAX_FREQ_HZ: f32 = 22_000.0;
pub const EQ_PLUS_MIN_GAIN_DB: f32 = -30.0;
pub const EQ_PLUS_MAX_GAIN_DB: f32 = 30.0;
pub const EQ_PLUS_MIN_Q: f32 = 0.1;
pub const EQ_PLUS_MAX_Q: f32 = 20.0;
pub const EQ_PLUS_MIN_SHIFT_ST: f32 = -36.0;
pub const EQ_PLUS_MAX_SHIFT_ST: f32 = 36.0;
pub const EQ_PLUS_MIN_OUT_GAIN_DB: f32 = -30.0;
pub const EQ_PLUS_MAX_OUT_GAIN_DB: f32 = 30.0;

/// Parámetros por banda: `TYPE`, `FREQ`, `GAIN`, `Q`, `ENABLE` (5 * 8 = 40).
pub const EQ_PLUS_PARAMS_PER_BAND: u32 = 5;
pub const EQ_PLUS_BAND_PARAM_TYPE: u32 = 0;
pub const EQ_PLUS_BAND_PARAM_FREQ: u32 = 1;
pub const EQ_PLUS_BAND_PARAM_GAIN: u32 = 2;
pub const EQ_PLUS_BAND_PARAM_Q: u32 = 3;
pub const EQ_PLUS_BAND_PARAM_ENABLE: u32 = 4;

/// Parámetros globales (40..=45).
pub const EQ_PLUS_PARAM_SHIFT: u32 = 40;
pub const EQ_PLUS_PARAM_OUTPUT_GAIN: u32 = 41;
pub const EQ_PLUS_PARAM_SELECTED_BAND: u32 = 42;
pub const EQ_PLUS_PARAM_SOLO: u32 = 43;
pub const EQ_PLUS_PARAM_ADAPTIVE_Q: u32 = 44;
pub const EQ_PLUS_PARAM_DECIBEL_RANGE: u32 = 45;
pub const EQ_PLUS_PARAM_COUNT: usize = 46;

pub const EQ_PLUS_DEFAULT_BAND_FREQS_HZ: [f32; EQ_PLUS_BAND_COUNT] = [
    80.0, 160.0, 320.0, 640.0, 1280.0, 2500.0, 5000.0, 10_000.0,
];

/// Tipos de filtro de cada banda (15 modos idénticos a Bitwig EQ+).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum EqPlusBandType {
    #[default]
    Off = 0,
    LowCut1 = 1,
    LowCut2 = 2,
    LowCut4 = 3,
    LowCut6 = 4,
    LowCut8 = 5,
    LowShelf = 6,
    Bell = 7,
    HighCut1 = 8,
    HighCut2 = 9,
    HighCut4 = 10,
    HighCut6 = 11,
    HighCut8 = 12,
    HighShelf = 13,
    Notch = 14,
}

impl EqPlusBandType {
    pub const ALL: [EqPlusBandType; 15] = [
        Self::Off,
        Self::LowCut1,
        Self::LowCut2,
        Self::LowCut4,
        Self::LowCut6,
        Self::LowCut8,
        Self::LowShelf,
        Self::Bell,
        Self::HighCut1,
        Self::HighCut2,
        Self::HighCut4,
        Self::HighCut6,
        Self::HighCut8,
        Self::HighShelf,
        Self::Notch,
    ];

    pub fn from_index(idx: usize) -> Self {
        Self::ALL.get(idx).copied().unwrap_or(Self::Off)
    }

    pub fn from_normalized(norm: f32) -> Self {
        let idx = ((norm.clamp(0.0, 1.0) * 14.0).round() as usize).min(14);
        Self::from_index(idx)
    }

    pub fn to_normalized(self) -> f32 {
        (self as u8 as f32) / 14.0
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::LowCut1 => "Low-cut 1°",
            Self::LowCut2 => "Low-cut 2°",
            Self::LowCut4 => "Low-cut 4°",
            Self::LowCut6 => "Low-cut 6°",
            Self::LowCut8 => "Low-cut 8°",
            Self::LowShelf => "Low-shelf",
            Self::Bell => "Bell",
            Self::HighCut1 => "High-cut 1°",
            Self::HighCut2 => "High-cut 2°",
            Self::HighCut4 => "High-cut 4°",
            Self::HighCut6 => "High-cut 6°",
            Self::HighCut8 => "High-cut 8°",
            Self::HighShelf => "High-shelf",
            Self::Notch => "Notch",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Off => "Band disabled",
            Self::LowCut1 => "6dB/oct slope",
            Self::LowCut2 => "12dB/oct slope",
            Self::LowCut4 => "24dB/oct slope",
            Self::LowCut6 => "36dB/oct slope",
            Self::LowCut8 => "48dB/oct slope",
            Self::LowShelf => "Gain-variable filter",
            Self::Bell => "Gain-variable node",
            Self::HighCut1 => "6dB/oct slope",
            Self::HighCut2 => "12dB/oct slope",
            Self::HighCut4 => "24dB/oct slope",
            Self::HighCut6 => "36dB/oct slope",
            Self::HighCut8 => "48dB/oct slope",
            Self::HighShelf => "Gain-variable filter",
            Self::Notch => "Steep cut at frequency",
        }
    }

    pub fn uses_gain(self) -> bool {
        matches!(self, Self::LowShelf | Self::Bell | Self::HighShelf)
    }

    pub fn is_active(self) -> bool {
        self != Self::Off
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EqPlusRange {
    #[default]
    Db10 = 0,
    Db20 = 1,
    Db30 = 2,
}

impl EqPlusRange {
    pub const ALL: [EqPlusRange; 3] = [Self::Db10, Self::Db20, Self::Db30];

    pub fn max_db(self) -> f32 {
        match self {
            Self::Db10 => 10.0,
            Self::Db20 => 20.0,
            Self::Db30 => 30.0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Db10 => "±10",
            Self::Db20 => "±20",
            Self::Db30 => "±30",
        }
    }

    pub fn from_normalized(norm: f32) -> Self {
        let idx = ((norm.clamp(0.0, 1.0) * 2.0).round() as usize).min(2);
        Self::ALL[idx]
    }

    pub fn to_normalized(self) -> f32 {
        (self as u8 as f32) / 2.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnalyzerResolution {
    Small = 0,
    #[default]
    Medium = 1,
    Large = 2,
    Huge = 3,
}

impl AnalyzerResolution {
    pub const ALL: [AnalyzerResolution; 4] =
        [Self::Small, Self::Medium, Self::Large, Self::Huge];

    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
            Self::Huge => "Huge",
        }
    }

    pub fn fft_size(self) -> usize {
        match self {
            Self::Small => 1024,
            Self::Medium => 2048,
            Self::Large => 4096,
            Self::Huge => 8192,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnalyzerSpeed {
    VeryFast = 0,
    Fast = 1,
    #[default]
    Medium = 2,
    Slow = 3,
    Freeze = 4,
}

impl AnalyzerSpeed {
    pub const ALL: [AnalyzerSpeed; 5] = [
        Self::VeryFast,
        Self::Fast,
        Self::Medium,
        Self::Slow,
        Self::Freeze,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::VeryFast => "Very Fast",
            Self::Fast => "Fast",
            Self::Medium => "Medium",
            Self::Slow => "Slow",
            Self::Freeze => "Freeze",
        }
    }

    pub fn smoothing_coeff(self) -> f32 {
        match self {
            Self::VeryFast => 0.65,
            Self::Fast => 0.42,
            Self::Medium => 0.25,
            Self::Slow => 0.10,
            Self::Freeze => 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EqPlusBandParams {
    pub enabled: bool,
    pub band_type: EqPlusBandType,
    pub freq_hz: f32,
    pub gain_db: f32,
    pub q: f32,
}

impl Default for EqPlusBandParams {
    fn default() -> Self {
        Self {
            enabled: true,
            band_type: EqPlusBandType::Off,
            freq_hz: 1000.0,
            gain_db: 0.0,
            q: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EqPlusParams {
    pub bands: [EqPlusBandParams; EQ_PLUS_BAND_COUNT],
    pub shift_st: f32,
    pub output_gain_db: f32,
    pub selected_band: usize,
    pub solo: bool,
    pub adaptive_q: bool,
    pub range: EqPlusRange,
    pub show_pre: bool,
    pub show_post: bool,
    pub analyzer_resolution: AnalyzerResolution,
    pub analyzer_speed: AnalyzerSpeed,
}

impl Default for EqPlusParams {
    fn default() -> Self {
        let mut bands = [EqPlusBandParams::default(); EQ_PLUS_BAND_COUNT];
        for (i, band) in bands.iter_mut().enumerate() {
            band.freq_hz = EQ_PLUS_DEFAULT_BAND_FREQS_HZ[i];
        }
        Self {
            bands,
            shift_st: 0.0,
            output_gain_db: 0.0,
            selected_band: 0,
            solo: false,
            adaptive_q: true,
            range: EqPlusRange::Db10,
            show_pre: true,
            show_post: true,
            analyzer_resolution: AnalyzerResolution::Medium,
            analyzer_speed: AnalyzerSpeed::Medium,
        }
    }
}

impl EqPlusParams {
    #[inline]
    pub fn freq_hz_to_norm(hz: f32) -> f32 {
        let clamped = hz.clamp(EQ_PLUS_MIN_FREQ_HZ, EQ_PLUS_MAX_FREQ_HZ);
        let min_l = EQ_PLUS_MIN_FREQ_HZ.ln();
        let max_l = EQ_PLUS_MAX_FREQ_HZ.ln();
        ((clamped.ln() - min_l) / (max_l - min_l)).clamp(0.0, 1.0)
    }

    #[inline]
    pub fn norm_to_freq_hz(norm: f32) -> f32 {
        let min_l = EQ_PLUS_MIN_FREQ_HZ.ln();
        let max_l = EQ_PLUS_MAX_FREQ_HZ.ln();
        (min_l + norm.clamp(0.0, 1.0) * (max_l - min_l)).exp()
    }

    #[inline]
    pub fn gain_db_to_norm(db: f32) -> f32 {
        ((db - EQ_PLUS_MIN_GAIN_DB) / (EQ_PLUS_MAX_GAIN_DB - EQ_PLUS_MIN_GAIN_DB)).clamp(0.0, 1.0)
    }

    #[inline]
    pub fn norm_to_gain_db(norm: f32) -> f32 {
        EQ_PLUS_MIN_GAIN_DB + norm.clamp(0.0, 1.0) * (EQ_PLUS_MAX_GAIN_DB - EQ_PLUS_MIN_GAIN_DB)
    }

    #[inline]
    pub fn q_to_norm(q: f32) -> f32 {
        let clamped = q.clamp(EQ_PLUS_MIN_Q, EQ_PLUS_MAX_Q);
        let min_l = EQ_PLUS_MIN_Q.ln();
        let max_l = EQ_PLUS_MAX_Q.ln();
        ((clamped.ln() - min_l) / (max_l - min_l)).clamp(0.0, 1.0)
    }

    #[inline]
    pub fn norm_to_q(norm: f32) -> f32 {
        let min_l = EQ_PLUS_MIN_Q.ln();
        let max_l = EQ_PLUS_MAX_Q.ln();
        (min_l + norm.clamp(0.0, 1.0) * (max_l - min_l)).exp()
    }

    #[inline]
    pub fn shift_st_to_norm(st: f32) -> f32 {
        ((st - EQ_PLUS_MIN_SHIFT_ST) / (EQ_PLUS_MAX_SHIFT_ST - EQ_PLUS_MIN_SHIFT_ST))
            .clamp(0.0, 1.0)
    }

    #[inline]
    pub fn norm_to_shift_st(norm: f32) -> f32 {
        EQ_PLUS_MIN_SHIFT_ST
            + norm.clamp(0.0, 1.0) * (EQ_PLUS_MAX_SHIFT_ST - EQ_PLUS_MIN_SHIFT_ST)
    }

    #[inline]
    pub fn output_gain_db_to_norm(db: f32) -> f32 {
        ((db - EQ_PLUS_MIN_OUT_GAIN_DB) / (EQ_PLUS_MAX_OUT_GAIN_DB - EQ_PLUS_MIN_OUT_GAIN_DB))
            .clamp(0.0, 1.0)
    }

    #[inline]
    pub fn norm_to_output_gain_db(norm: f32) -> f32 {
        EQ_PLUS_MIN_OUT_GAIN_DB
            + norm.clamp(0.0, 1.0) * (EQ_PLUS_MAX_OUT_GAIN_DB - EQ_PLUS_MIN_OUT_GAIN_DB)
    }

    pub fn get_normalized(&self, id: u32) -> Option<f32> {
        let band_total = (EQ_PLUS_BAND_COUNT as u32) * EQ_PLUS_PARAMS_PER_BAND;
        if id < band_total {
            let b = (id / EQ_PLUS_PARAMS_PER_BAND) as usize;
            let sub = id % EQ_PLUS_PARAMS_PER_BAND;
            let band = &self.bands[b];
            return match sub {
                EQ_PLUS_BAND_PARAM_TYPE => Some(band.band_type.to_normalized()),
                EQ_PLUS_BAND_PARAM_FREQ => Some(Self::freq_hz_to_norm(band.freq_hz)),
                EQ_PLUS_BAND_PARAM_GAIN => Some(Self::gain_db_to_norm(band.gain_db)),
                EQ_PLUS_BAND_PARAM_Q => Some(Self::q_to_norm(band.q)),
                EQ_PLUS_BAND_PARAM_ENABLE => Some(if band.enabled { 1.0 } else { 0.0 }),
                _ => None,
            };
        }
        match id {
            EQ_PLUS_PARAM_SHIFT => Some(Self::shift_st_to_norm(self.shift_st)),
            EQ_PLUS_PARAM_OUTPUT_GAIN => Some(Self::output_gain_db_to_norm(self.output_gain_db)),
            EQ_PLUS_PARAM_SELECTED_BAND => {
                Some((self.selected_band.min(EQ_PLUS_BAND_COUNT - 1) as f32) / 7.0)
            }
            EQ_PLUS_PARAM_SOLO => Some(if self.solo { 1.0 } else { 0.0 }),
            EQ_PLUS_PARAM_ADAPTIVE_Q => Some(if self.adaptive_q { 1.0 } else { 0.0 }),
            EQ_PLUS_PARAM_DECIBEL_RANGE => Some(self.range.to_normalized()),
            _ => None,
        }
    }

    pub fn set_normalized(&mut self, id: u32, value: f32) -> bool {
        let v = value.clamp(0.0, 1.0);
        let band_total = (EQ_PLUS_BAND_COUNT as u32) * EQ_PLUS_PARAMS_PER_BAND;
        if id < band_total {
            let b = (id / EQ_PLUS_PARAMS_PER_BAND) as usize;
            let sub = id % EQ_PLUS_PARAMS_PER_BAND;
            let band = &mut self.bands[b];
            match sub {
                EQ_PLUS_BAND_PARAM_TYPE => band.band_type = EqPlusBandType::from_normalized(v),
                EQ_PLUS_BAND_PARAM_FREQ => band.freq_hz = Self::norm_to_freq_hz(v),
                EQ_PLUS_BAND_PARAM_GAIN => band.gain_db = Self::norm_to_gain_db(v),
                EQ_PLUS_BAND_PARAM_Q => band.q = Self::norm_to_q(v),
                EQ_PLUS_BAND_PARAM_ENABLE => band.enabled = v >= 0.5,
                _ => return false,
            }
            return true;
        }
        match id {
            EQ_PLUS_PARAM_SHIFT => {
                self.shift_st = Self::norm_to_shift_st(v);
                true
            }
            EQ_PLUS_PARAM_OUTPUT_GAIN => {
                self.output_gain_db = Self::norm_to_output_gain_db(v);
                true
            }
            EQ_PLUS_PARAM_SELECTED_BAND => {
                self.selected_band = ((v * 7.0).round() as usize).min(EQ_PLUS_BAND_COUNT - 1);
                true
            }
            EQ_PLUS_PARAM_SOLO => {
                self.solo = v >= 0.5;
                true
            }
            EQ_PLUS_PARAM_ADAPTIVE_Q => {
                self.adaptive_q = v >= 0.5;
                true
            }
            EQ_PLUS_PARAM_DECIBEL_RANGE => {
                self.range = EqPlusRange::from_normalized(v);
                true
            }
            _ => false,
        }
    }

    /// Calcula la respuesta en magnitud (dB) total de todas las bandas activas a `freq_hz`.
    pub fn total_magnitude_db(&self, freq_hz: f64, sample_rate: f64) -> f64 {
        let mut mag_sq = 1.0_f64;
        let shift_mul = 2.0_f64.powf((self.shift_st as f64) / 12.0);
        for band in &self.bands {
            if band.enabled && band.band_type.is_active() {
                mag_sq *= band_magnitude_sq(band, shift_mul, self.adaptive_q, freq_hz, sample_rate);
            }
        }
        let total_db = 10.0 * mag_sq.max(1e-18).log10() + (self.output_gain_db as f64);
        total_db.clamp(-120.0, 48.0)
    }

    /// Calcula la respuesta en magnitud (dB) de una sola banda (para relleno de banda seleccionada).
    pub fn band_magnitude_db(&self, band_idx: usize, freq_hz: f64, sample_rate: f64) -> f64 {
        let Some(band) = self.bands.get(band_idx) else {
            return 0.0;
        };
        if !band.enabled || !band.band_type.is_active() {
            return 0.0;
        }
        let shift_mul = 2.0_f64.powf((self.shift_st as f64) / 12.0);
        let mag_sq = band_magnitude_sq(band, shift_mul, self.adaptive_q, freq_hz, sample_rate);
        (10.0 * mag_sq.max(1e-18).log10()).clamp(-120.0, 48.0)
    }

    /// Convierte el arreglo de parámetros a pares `(id, normalized)`.
    pub fn to_param_pairs(&self) -> Vec<(u32, f32)> {
        (0..EQ_PLUS_PARAM_COUNT as u32)
            .filter_map(|id| self.get_normalized(id).map(|v| (id, v)))
            .collect()
    }
}

// =========================================================================
// Biquad DSP kernel (RBJ / Butterworth cascades en f64)
// =========================================================================

#[derive(Debug, Clone, Copy, Default)]
struct BiquadCoeffs {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl BiquadCoeffs {
    const fn bypass() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        }
    }

    /// Evalúa `|H(e^{j w})|^2` exactamente en la frecuencia `freq_hz`.
    #[inline]
    fn magnitude_sq(&self, freq_hz: f64, sample_rate: f64) -> f64 {
        let w = 2.0 * PI * (freq_hz / sample_rate.max(8000.0)).clamp(0.0, 0.499);
        let cos_w = w.cos();
        let cos_2w = (2.0 * w).cos();
        let sin_w = w.sin();
        let sin_2w = (2.0 * w).sin();

        let num_re = self.b0 + self.b1 * cos_w + self.b2 * cos_2w;
        let num_im = -self.b1 * sin_w - self.b2 * sin_2w;
        let den_re = 1.0 + self.a1 * cos_w + self.a2 * cos_2w;
        let den_im = -self.a1 * sin_w - self.a2 * sin_2w;

        let num_sq = num_re * num_re + num_im * num_im;
        let den_sq = (den_re * den_re + den_im * den_im).max(1e-24);
        num_sq / den_sq
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct BiquadState {
    z1: [f64; 2],
    z2: [f64; 2],
}

impl BiquadState {
    #[inline]
    fn process_sample(&mut self, ch: usize, x: f64, c: &BiquadCoeffs) -> f64 {
        let y = c.b0 * x + self.z1[ch];
        self.z1[ch] = c.b1 * x - c.a1 * y + self.z2[ch];
        self.z2[ch] = c.b2 * x - c.a2 * y;
        if self.z1[ch].abs() < 1e-30 {
            self.z1[ch] = 0.0;
        }
        if self.z2[ch].abs() < 1e-30 {
            self.z2[ch] = 0.0;
        }
        y
    }

    fn reset(&mut self) {
        self.z1 = [0.0; 2];
        self.z2 = [0.0; 2];
    }
}

#[inline]
fn effective_q(band: &EqPlusBandParams, adaptive_q: bool) -> f64 {
    let base_q = (band.q as f64).clamp(EQ_PLUS_MIN_Q as f64, EQ_PLUS_MAX_Q as f64);
    if adaptive_q && band.band_type == EqPlusBandType::Bell {
        // Proporcional-Q suave estilo consola / Bitwig EQ+: aumenta la selectividad con |gain|.
        let gain_abs = (band.gain_db as f64).abs();
        (base_q * (1.0 + 0.045 * gain_abs)).clamp(EQ_PLUS_MIN_Q as f64, 24.0)
    } else {
        base_q
    }
}

fn design_biquad_stages(
    band: &EqPlusBandParams,
    shift_mul: f64,
    adaptive_q: bool,
    sample_rate: f64,
) -> ([BiquadCoeffs; 4], usize) {
    let mut stages = [BiquadCoeffs::bypass(); 4];
    if !band.enabled || !band.band_type.is_active() {
        return (stages, 0);
    }
    let sr = sample_rate.max(8000.0);
    let freq = ((band.freq_hz as f64) * shift_mul).clamp(5.0, sr * 0.495);
    let w0 = 2.0 * PI * freq / sr;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let q = effective_q(band, adaptive_q);

    match band.band_type {
        EqPlusBandType::Off => (stages, 0),
        EqPlusBandType::LowCut1 => {
            // 1er orden HPF (transformada bilineal): H(s) = s / (s + wc)
            let k = (w0 * 0.5).tan();
            let norm = 1.0 / (1.0 + k);
            stages[0] = BiquadCoeffs {
                b0: norm,
                b1: -norm,
                b2: 0.0,
                a1: (k - 1.0) * norm,
                a2: 0.0,
            };
            (stages, 1)
        }
        EqPlusBandType::LowCut2 => {
            stages[0] = rbj_hpf(cos_w0, sin_w0, q);
            (stages, 1)
        }
        EqPlusBandType::LowCut4 => {
            // 4to orden Butterworth: 2 biquads con Q = 0.5411961, 1.3065630
            stages[0] = rbj_hpf(cos_w0, sin_w0, 0.541_196_100_146_197);
            stages[1] = rbj_hpf(cos_w0, sin_w0, 1.306_562_964_876_376_6);
            (stages, 2)
        }
        EqPlusBandType::LowCut6 => {
            // 6to orden Butterworth: 3 biquads
            stages[0] = rbj_hpf(cos_w0, sin_w0, 0.517_638_090_205_041_5);
            stages[1] = rbj_hpf(cos_w0, sin_w0, SQRT_2 * 0.5);
            stages[2] = rbj_hpf(cos_w0, sin_w0, 1.931_851_652_578_136_6);
            (stages, 3)
        }
        EqPlusBandType::LowCut8 => {
            // 8vo orden Butterworth: 4 biquads
            stages[0] = rbj_hpf(cos_w0, sin_w0, 0.509_795_579_104_159_2);
            stages[1] = rbj_hpf(cos_w0, sin_w0, 0.601_344_886_935_045_3);
            stages[2] = rbj_hpf(cos_w0, sin_w0, 0.899_976_223_136_415_7);
            stages[3] = rbj_hpf(cos_w0, sin_w0, 2.562_915_447_741_505_5);
            (stages, 4)
        }
        EqPlusBandType::HighCut1 => {
            // 1er orden LPF (transformada bilineal): H(s) = wc / (s + wc)
            let k = (w0 * 0.5).tan();
            let norm = 1.0 / (1.0 + k);
            stages[0] = BiquadCoeffs {
                b0: k * norm,
                b1: k * norm,
                b2: 0.0,
                a1: (k - 1.0) * norm,
                a2: 0.0,
            };
            (stages, 1)
        }
        EqPlusBandType::HighCut2 => {
            stages[0] = rbj_lpf(cos_w0, sin_w0, q);
            (stages, 1)
        }
        EqPlusBandType::HighCut4 => {
            stages[0] = rbj_lpf(cos_w0, sin_w0, 0.541_196_100_146_197);
            stages[1] = rbj_lpf(cos_w0, sin_w0, 1.306_562_964_876_376_6);
            (stages, 2)
        }
        EqPlusBandType::HighCut6 => {
            stages[0] = rbj_lpf(cos_w0, sin_w0, 0.517_638_090_205_041_5);
            stages[1] = rbj_lpf(cos_w0, sin_w0, SQRT_2 * 0.5);
            stages[2] = rbj_lpf(cos_w0, sin_w0, 1.931_851_652_578_136_6);
            (stages, 3)
        }
        EqPlusBandType::HighCut8 => {
            stages[0] = rbj_lpf(cos_w0, sin_w0, 0.509_795_579_104_159_2);
            stages[1] = rbj_lpf(cos_w0, sin_w0, 0.601_344_886_935_045_3);
            stages[2] = rbj_lpf(cos_w0, sin_w0, 0.899_976_223_136_415_7);
            stages[3] = rbj_lpf(cos_w0, sin_w0, 2.562_915_447_741_505_5);
            (stages, 4)
        }
        EqPlusBandType::Bell => {
            let a = 10.0_f64.powf((band.gain_db as f64) / 40.0);
            let alpha = sin_w0 / (2.0 * q);
            let a0 = 1.0 + alpha / a;
            let inv_a0 = 1.0 / a0;
            stages[0] = BiquadCoeffs {
                b0: (1.0 + alpha * a) * inv_a0,
                b1: (-2.0 * cos_w0) * inv_a0,
                b2: (1.0 - alpha * a) * inv_a0,
                a1: (-2.0 * cos_w0) * inv_a0,
                a2: (1.0 - alpha / a) * inv_a0,
            };
            (stages, 1)
        }
        EqPlusBandType::LowShelf => {
            let a = 10.0_f64.powf((band.gain_db as f64) / 40.0);
            let slope_q = q.clamp(0.2, 4.0);
            let alpha = (sin_w0 * 0.5) * ((a + 1.0 / a) * (1.0 / slope_q - 1.0) + 2.0).max(0.01).sqrt();
            let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
            let a0 = (a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha;
            let inv_a0 = 1.0 / a0;
            stages[0] = BiquadCoeffs {
                b0: a * ((a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha) * inv_a0,
                b1: 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0) * inv_a0,
                b2: a * ((a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha) * inv_a0,
                a1: -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0) * inv_a0,
                a2: ((a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha) * inv_a0,
            };
            (stages, 1)
        }
        EqPlusBandType::HighShelf => {
            let a = 10.0_f64.powf((band.gain_db as f64) / 40.0);
            let slope_q = q.clamp(0.2, 4.0);
            let alpha = (sin_w0 * 0.5) * ((a + 1.0 / a) * (1.0 / slope_q - 1.0) + 2.0).max(0.01).sqrt();
            let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
            let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha;
            let inv_a0 = 1.0 / a0;
            stages[0] = BiquadCoeffs {
                b0: a * ((a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha) * inv_a0,
                b1: -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0) * inv_a0,
                b2: a * ((a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha) * inv_a0,
                a1: 2.0 * ((a - 1.0) - (a + 1.0) * cos_w0) * inv_a0,
                a2: ((a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha) * inv_a0,
            };
            (stages, 1)
        }
        EqPlusBandType::Notch => {
            let alpha = sin_w0 / (2.0 * q);
            let a0 = 1.0 + alpha;
            let inv_a0 = 1.0 / a0;
            stages[0] = BiquadCoeffs {
                b0: inv_a0,
                b1: (-2.0 * cos_w0) * inv_a0,
                b2: inv_a0,
                a1: (-2.0 * cos_w0) * inv_a0,
                a2: (1.0 - alpha) * inv_a0,
            };
            (stages, 1)
        }
    }
}

#[inline]
fn rbj_hpf(cos_w0: f64, sin_w0: f64, q: f64) -> BiquadCoeffs {
    let alpha = sin_w0 / (2.0 * q);
    let a0 = 1.0 + alpha;
    let inv_a0 = 1.0 / a0;
    BiquadCoeffs {
        b0: (1.0 + cos_w0) * 0.5 * inv_a0,
        b1: -(1.0 + cos_w0) * inv_a0,
        b2: (1.0 + cos_w0) * 0.5 * inv_a0,
        a1: (-2.0 * cos_w0) * inv_a0,
        a2: (1.0 - alpha) * inv_a0,
    }
}

#[inline]
fn rbj_lpf(cos_w0: f64, sin_w0: f64, q: f64) -> BiquadCoeffs {
    let alpha = sin_w0 / (2.0 * q);
    let a0 = 1.0 + alpha;
    let inv_a0 = 1.0 / a0;
    BiquadCoeffs {
        b0: (1.0 - cos_w0) * 0.5 * inv_a0,
        b1: (1.0 - cos_w0) * inv_a0,
        b2: (1.0 - cos_w0) * 0.5 * inv_a0,
        a1: (-2.0 * cos_w0) * inv_a0,
        a2: (1.0 - alpha) * inv_a0,
    }
}

#[inline]
fn band_magnitude_sq(
    band: &EqPlusBandParams,
    shift_mul: f64,
    adaptive_q: bool,
    freq_hz: f64,
    sample_rate: f64,
) -> f64 {
    let (stages, count) = design_biquad_stages(band, shift_mul, adaptive_q, sample_rate);
    let mut mag_sq = 1.0;
    for stage in stages.iter().take(count) {
        mag_sq *= stage.magnitude_sq(freq_hz, sample_rate);
    }
    mag_sq
}

// =========================================================================
// Telemetría lock-free Pre/Post para el analizador de espectro de EQ+
// =========================================================================

pub const EQ_PLUS_RING_SIZE: usize = 8192;

pub struct EqPlusTelemetry {
    sample_rate_bits: AtomicU32,
    write_seq: AtomicU64,
    pre_ring: Box<[AtomicU32; EQ_PLUS_RING_SIZE]>,
    post_ring: Box<[AtomicU32; EQ_PLUS_RING_SIZE]>,
}

impl EqPlusTelemetry {
    pub fn new(sample_rate: f32) -> Self {
        let zero_bits = 0.0_f32.to_bits();
        let mk_ring = || -> Box<[AtomicU32; EQ_PLUS_RING_SIZE]> {
            let v: Vec<AtomicU32> = (0..EQ_PLUS_RING_SIZE)
                .map(|_| AtomicU32::new(zero_bits))
                .collect();
            v.into_boxed_slice().try_into().unwrap_or_else(|_| unreachable!())
        };
        Self {
            sample_rate_bits: AtomicU32::new(sample_rate.max(8000.0).to_bits()),
            write_seq: AtomicU64::new(0),
            pre_ring: mk_ring(),
            post_ring: mk_ring(),
        }
    }

    pub fn set_sample_rate(&self, sample_rate: f32) {
        self.sample_rate_bits
            .store(sample_rate.max(8000.0).to_bits(), Ordering::Relaxed);
    }

    #[inline]
    pub fn push_mono(&self, pre: f32, post: f32) {
        let seq = self.write_seq.load(Ordering::Relaxed);
        let idx = (seq as usize) & (EQ_PLUS_RING_SIZE - 1);
        self.pre_ring[idx].store(pre.to_bits(), Ordering::Relaxed);
        self.post_ring[idx].store(post.to_bits(), Ordering::Relaxed);
        self.write_seq.store(seq.wrapping_add(1), Ordering::Release);
    }

    pub fn sample_rate(&self) -> f32 {
        f32::from_bits(self.sample_rate_bits.load(Ordering::Relaxed)).max(8000.0)
    }

    /// Calcula el espectro en dB para `num_bins` frecuencias espaciadas logarítmicamente
    /// entre `min_hz` y `max_hz`, con pendiente visual de `4.5 dB/oct` (tilt).
    pub fn compute_spectrum_db(
        &self,
        fft_size: usize,
        num_bins: usize,
        min_hz: f32,
        max_hz: f32,
    ) -> (Vec<f32>, Vec<f32>) {
        let n = fft_size.clamp(512, EQ_PLUS_RING_SIZE).next_power_of_two();
        let sr = self.sample_rate();
        let seq = self.write_seq.load(Ordering::Acquire);
        let start = seq.wrapping_sub(n as u64) as usize;

        let mut pre_buf = vec![0.0_f32; n];
        let mut post_buf = vec![0.0_f32; n];
        for i in 0..n {
            let ri = (start.wrapping_add(i)) & (EQ_PLUS_RING_SIZE - 1);
            let win = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * (i as f32) / (n as f32)).cos());
            pre_buf[i] = f32::from_bits(self.pre_ring[ri].load(Ordering::Relaxed)) * win;
            post_buf[i] = f32::from_bits(self.post_ring[ri].load(Ordering::Relaxed)) * win;
        }

        let mut planner = RealFftPlanner::<f32>::new();
        let r2c = planner.plan_fft_forward(n);
        let mut pre_spec = r2c.make_output_vec();
        let mut post_spec = r2c.make_output_vec();
        let _ = r2c.process(&mut pre_buf, &mut pre_spec);
        let _ = r2c.process(&mut post_buf, &mut post_spec);

        let scale = 2.0 / (n as f32);
        let bin_hz = sr / (n as f32);
        let min_l = min_hz.max(10.0).ln();
        let max_l = max_hz.max(min_hz + 1.0).ln();

        let mut pre_out = Vec::with_capacity(num_bins);
        let mut post_out = Vec::with_capacity(num_bins);

        for b in 0..num_bins {
            let t = if num_bins > 1 {
                (b as f32) / ((num_bins - 1) as f32)
            } else {
                0.0
            };
            let f = (min_l + t * (max_l - min_l)).exp();
            let k_f = (f / bin_hz).clamp(1.0, (pre_spec.len() - 2) as f32);
            let k0 = k_f.floor() as usize;
            let frac = k_f - (k0 as f32);

            let m_pre = pre_spec[k0].norm() * (1.0 - frac) + pre_spec[k0 + 1].norm() * frac;
            let m_post = post_spec[k0].norm() * (1.0 - frac) + post_spec[k0 + 1].norm() * frac;

            // Tilt estándar de +4.5 dB/oct respecto a 1 kHz para lectura musical
            let tilt_db = 4.5 * (f / 1000.0).log2();
            let db_pre = (20.0 * (m_pre * scale).max(1e-6).log10() + tilt_db).clamp(-96.0, 24.0);
            let db_post = (20.0 * (m_post * scale).max(1e-6).log10() + tilt_db).clamp(-96.0, 24.0);
            pre_out.push(db_pre);
            post_out.push(db_post);
        }

        (pre_out, post_out)
    }
}

pub mod telemetry {
    use super::*;

    type Key = (usize, usize);

    fn registry() -> &'static Mutex<HashMap<Key, Arc<EqPlusTelemetry>>> {
        static REG: OnceLock<Mutex<HashMap<Key, Arc<EqPlusTelemetry>>>> = OnceLock::new();
        REG.get_or_init(|| Mutex::new(HashMap::new()))
    }

    pub fn register(key: Key, handle: Arc<EqPlusTelemetry>) {
        if let Ok(mut map) = registry().lock() {
            map.insert(key, handle);
        }
    }

    pub fn unregister(key: Key) {
        if let Ok(mut map) = registry().lock() {
            map.remove(&key);
        }
    }

    pub fn get(key: Key) -> Option<Arc<EqPlusTelemetry>> {
        registry().lock().ok().and_then(|map| map.get(&key).cloned())
    }
}

// =========================================================================
// Procesador principal EqPlusEffect
// =========================================================================

pub struct EqPlusEffect {
    params: EqPlusParams,
    sample_rate: f32,
    stages: [[BiquadCoeffs; 4]; EQ_PLUS_BAND_COUNT],
    stage_counts: [usize; EQ_PLUS_BAND_COUNT],
    states: [[BiquadState; 4]; EQ_PLUS_BAND_COUNT],
    solo_band_coeffs: BiquadCoeffs,
    solo_band_state: BiquadState,
    smooth_out_gain: f64,
    slew_coeff: f64,
    coeffs_dirty: bool,
    telemetry: Arc<EqPlusTelemetry>,
}

impl EqPlusEffect {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        let telemetry = Arc::new(EqPlusTelemetry::new(sr));
        let mut fx = Self {
            params: EqPlusParams::default(),
            sample_rate: sr,
            stages: [[BiquadCoeffs::bypass(); 4]; EQ_PLUS_BAND_COUNT],
            stage_counts: [0; EQ_PLUS_BAND_COUNT],
            states: [[BiquadState::default(); 4]; EQ_PLUS_BAND_COUNT],
            solo_band_coeffs: BiquadCoeffs::bypass(),
            solo_band_state: BiquadState::default(),
            smooth_out_gain: 1.0,
            slew_coeff: 1.0 - (-1.0 / (0.005 * (sr as f64))).exp(),
            coeffs_dirty: true,
            telemetry,
        };
        fx.rebuild_coefficients();
        fx
    }

    pub fn telemetry(&self) -> Arc<EqPlusTelemetry> {
        Arc::clone(&self.telemetry)
    }

    pub fn params(&self) -> &EqPlusParams {
        &self.params
    }

    pub fn set_params(&mut self, params: EqPlusParams) {
        self.params = params;
        self.coeffs_dirty = true;
    }

    fn rebuild_coefficients(&mut self) {
        let sr = self.sample_rate as f64;
        let shift_mul = 2.0_f64.powf((self.params.shift_st as f64) / 12.0);
        for b in 0..EQ_PLUS_BAND_COUNT {
            let (st, cnt) = design_biquad_stages(
                &self.params.bands[b],
                shift_mul,
                self.params.adaptive_q,
                sr,
            );
            self.stages[b] = st;
            self.stage_counts[b] = cnt;
        }
        // Filtro pasa-banda para audición `SOLO` de la banda seleccionada
        let sel = self.params.selected_band.min(EQ_PLUS_BAND_COUNT - 1);
        let solo_band = &self.params.bands[sel];
        let solo_freq = ((solo_band.freq_hz as f64) * shift_mul).clamp(15.0, sr * 0.48);
        let w0 = 2.0 * PI * solo_freq / sr;
        let q = (solo_band.q as f64).clamp(0.4, 12.0);
        let alpha = w0.sin() / (2.0 * q);
        let a0 = 1.0 + alpha;
        let inv_a0 = 1.0 / a0;
        self.solo_band_coeffs = BiquadCoeffs {
            b0: alpha * inv_a0,
            b1: 0.0,
            b2: -alpha * inv_a0,
            a1: (-2.0 * w0.cos()) * inv_a0,
            a2: (1.0 - alpha) * inv_a0,
        };
        self.coeffs_dirty = false;
    }

    #[inline]
    pub fn process_stereo(&mut self, in_l: f32, in_r: f32) -> (f32, f32) {
        if self.coeffs_dirty {
            self.rebuild_coefficients();
        }
        let target_gain = 10.0_f64.powf((self.params.output_gain_db as f64) / 20.0);
        self.smooth_out_gain += (target_gain - self.smooth_out_gain) * self.slew_coeff;

        let mut x_l = in_l as f64;
        let mut x_r = in_r as f64;
        let pre_mono = 0.5 * (in_l + in_r);

        if self.params.solo {
            let sel = self.params.selected_band.min(EQ_PLUS_BAND_COUNT - 1);
            let band = &self.params.bands[sel];
            if band.enabled && band.band_type.is_active() {
                match band.band_type {
                    EqPlusBandType::LowCut1
                    | EqPlusBandType::LowCut2
                    | EqPlusBandType::LowCut4
                    | EqPlusBandType::LowCut6
                    | EqPlusBandType::LowCut8
                    | EqPlusBandType::HighCut1
                    | EqPlusBandType::HighCut2
                    | EqPlusBandType::HighCut4
                    | EqPlusBandType::HighCut6
                    | EqPlusBandType::HighCut8 => {
                        let cnt = self.stage_counts[sel];
                        for s in 0..cnt {
                            x_l = self.states[sel][s].process_sample(0, x_l, &self.stages[sel][s]);
                            x_r = self.states[sel][s].process_sample(1, x_r, &self.stages[sel][s]);
                        }
                    }
                    _ => {
                        x_l = self
                            .solo_band_state
                            .process_sample(0, x_l, &self.solo_band_coeffs);
                        x_r = self
                            .solo_band_state
                            .process_sample(1, x_r, &self.solo_band_coeffs);
                    }
                }
            }
        } else {
            for b in 0..EQ_PLUS_BAND_COUNT {
                let cnt = self.stage_counts[b];
                for s in 0..cnt {
                    x_l = self.states[b][s].process_sample(0, x_l, &self.stages[b][s]);
                    x_r = self.states[b][s].process_sample(1, x_r, &self.stages[b][s]);
                }
            }
        }

        let out_l = (x_l * self.smooth_out_gain).clamp(-8.0, 8.0) as f32;
        let out_r = (x_r * self.smooth_out_gain).clamp(-8.0, 8.0) as f32;
        let clean_l = if out_l.is_finite() { out_l } else { 0.0 };
        let clean_r = if out_r.is_finite() { out_r } else { 0.0 };
        self.telemetry.push_mono(pre_mono, 0.5 * (clean_l + clean_r));
        (clean_l, clean_r)
    }
}

impl PluginInstance for EqPlusEffect {
    fn process(&mut self, buffer: &mut AudioBuffer) {
        if self.coeffs_dirty {
            self.rebuild_coefficients();
        }
        for frame in buffer.samples.chunks_exact_mut(2) {
            let (l, r) = self.process_stereo(frame[0], frame[1]);
            frame[0] = l;
            frame[1] = r;
        }
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(8_000.0);
        self.slew_coeff = 1.0 - (-1.0 / (0.005 * (self.sample_rate as f64))).exp();
        self.telemetry.set_sample_rate(self.sample_rate);
        for band_states in &mut self.states {
            for st in band_states {
                st.reset();
            }
        }
        self.solo_band_state.reset();
        self.coeffs_dirty = true;
    }

    fn set_transport(&mut self, _transport: HostTransportInfo) {}

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
        "Hikaru OpenEQ+"
    }

    fn param_count(&mut self) -> usize {
        EQ_PLUS_PARAM_COUNT
    }

    fn param_info(&mut self, index: usize) -> Option<PluginParamInfo> {
        let def = EqPlusParams::default();
        let id = index as u32;
        let band_total = (EQ_PLUS_BAND_COUNT as u32) * EQ_PLUS_PARAMS_PER_BAND;
        if id < band_total {
            let b = (id / EQ_PLUS_PARAMS_PER_BAND) + 1;
            let sub = id % EQ_PLUS_PARAMS_PER_BAND;
            let default_value = def.get_normalized(id).unwrap_or(0.0);
            let (name, unit, min, max, step_count) = match sub {
                EQ_PLUS_BAND_PARAM_TYPE => (format!("Band {b} Type"), "", 0.0, 14.0, 14),
                EQ_PLUS_BAND_PARAM_FREQ => (
                    format!("Band {b} Freq"),
                    "Hz",
                    EQ_PLUS_MIN_FREQ_HZ,
                    EQ_PLUS_MAX_FREQ_HZ,
                    0,
                ),
                EQ_PLUS_BAND_PARAM_GAIN => (
                    format!("Band {b} Gain"),
                    "dB",
                    EQ_PLUS_MIN_GAIN_DB,
                    EQ_PLUS_MAX_GAIN_DB,
                    0,
                ),
                EQ_PLUS_BAND_PARAM_Q => (format!("Band {b} Q"), "", EQ_PLUS_MIN_Q, EQ_PLUS_MAX_Q, 0),
                EQ_PLUS_BAND_PARAM_ENABLE => (format!("Band {b} Enable"), "", 0.0, 1.0, 1),
                _ => return None,
            };
            return Some(PluginParamInfo {
                id,
                name,
                unit: unit.to_string(),
                default_value,
                min,
                max,
                step_count,
                can_automate: true,
                is_modulatable: true,
            });
        }

        let mk = |name: &str, unit: &str, min: f32, max: f32, step_count: i32| PluginParamInfo {
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
        match id {
            EQ_PLUS_PARAM_SHIFT => Some(mk(
                "Shift",
                "st",
                EQ_PLUS_MIN_SHIFT_ST,
                EQ_PLUS_MAX_SHIFT_ST,
                0,
            )),
            EQ_PLUS_PARAM_OUTPUT_GAIN => Some(mk(
                "Gain",
                "dB",
                EQ_PLUS_MIN_OUT_GAIN_DB,
                EQ_PLUS_MAX_OUT_GAIN_DB,
                0,
            )),
            EQ_PLUS_PARAM_SELECTED_BAND => Some(mk("Band", "", 1.0, 8.0, 7)),
            EQ_PLUS_PARAM_SOLO => Some(mk("Solo", "", 0.0, 1.0, 1)),
            EQ_PLUS_PARAM_ADAPTIVE_Q => Some(mk("Adaptive Q", "", 0.0, 1.0, 1)),
            EQ_PLUS_PARAM_DECIBEL_RANGE => Some(mk("Range", "", 0.0, 2.0, 2)),
            _ => None,
        }
    }

    fn get_param_normalized(&mut self, param_id: u32) -> Option<f32> {
        self.params.get_normalized(param_id)
    }

    fn set_param_normalized(&mut self, param_id: u32, value: f32) -> bool {
        if self.params.set_normalized(param_id, value) {
            self.coeffs_dirty = true;
            true
        } else {
            false
        }
    }
}

/// Conversión de nota MIDI / valor `FREQ` de `.bwpreset` (`39.48` -> `80 Hz`) a Hz.
#[inline]
pub fn bitwig_freq_midi_to_hz(midi: f64) -> f32 {
    let hz = 440.0 * (LN_2 * ((midi - 69.0) / 12.0)).exp();
    (hz as f32).clamp(EQ_PLUS_MIN_FREQ_HZ, EQ_PLUS_MAX_FREQ_HZ)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measure_sine_gain_db(fx: &mut EqPlusEffect, freq_hz: f32, sr: f32) -> f32 {
        let total = 8192;
        let settle = 4096;
        let mut sum_sq = 0.0_f64;
        for i in 0..total {
            let x = (2.0 * std::f32::consts::PI * freq_hz * (i as f32) / sr).sin();
            let (y, _) = fx.process_stereo(x, x);
            if i >= settle {
                sum_sq += (y as f64) * (y as f64);
            }
        }
        let rms_out = (sum_sq / ((total - settle) as f64)).sqrt();
        let rms_in = 0.5_f64.sqrt();
        (20.0 * (rms_out / rms_in).max(1e-12).log10()) as f32
    }

    #[test]
    fn default_bypass_es_unitario() {
        let mut fx = EqPlusEffect::new(48_000.0);
        let gain_1k = measure_sine_gain_db(&mut fx, 1000.0, 48_000.0);
        assert!(gain_1k.abs() < 0.05, "gain={gain_1k}");
    }

    #[test]
    fn bell_boost_y_cut_coinciden_con_curva() {
        let mut fx = EqPlusEffect::new(48_000.0);
        let mut p = EqPlusParams::default();
        p.bands[0] = EqPlusBandParams {
            enabled: true,
            band_type: EqPlusBandType::Bell,
            freq_hz: 1000.0,
            gain_db: 6.0,
            q: 1.0,
        };
        fx.set_params(p.clone());
        let measured = measure_sine_gain_db(&mut fx, 1000.0, 48_000.0);
        let theoretical = p.total_magnitude_db(1000.0, 48_000.0) as f32;
        assert!((measured - 6.0).abs() < 0.15, "measured={measured}");
        assert!((theoretical - 6.0).abs() < 0.05, "theoretical={theoretical}");
    }

    #[test]
    fn low_cut_8_atenua_fuertemente_por_debajo_de_fc() {
        let mut fx = EqPlusEffect::new(48_000.0);
        let mut p = EqPlusParams::default();
        p.bands[0] = EqPlusBandParams {
            enabled: true,
            band_type: EqPlusBandType::LowCut8,
            freq_hz: 1000.0,
            gain_db: 0.0,
            q: 0.707,
        };
        fx.set_params(p);
        let gain_250 = measure_sine_gain_db(&mut fx, 250.0, 48_000.0);
        let gain_4k = measure_sine_gain_db(&mut fx, 4000.0, 48_000.0);
        assert!(gain_250 < -60.0, "250 Hz should be heavily attenuated: {gain_250}");
        assert!(gain_4k.abs() < 0.2, "4 kHz passband should be flat: {gain_4k}");
    }

    #[test]
    fn notch_rechaza_frecuencia_central() {
        let mut fx = EqPlusEffect::new(48_000.0);
        let mut p = EqPlusParams::default();
        p.bands[0] = EqPlusBandParams {
            enabled: true,
            band_type: EqPlusBandType::Notch,
            freq_hz: 2000.0,
            gain_db: 0.0,
            q: 4.0,
        };
        fx.set_params(p);
        let gain_2k = measure_sine_gain_db(&mut fx, 2000.0, 48_000.0);
        let gain_200 = measure_sine_gain_db(&mut fx, 200.0, 48_000.0);
        assert!(gain_2k < -35.0, "Notch center should be rejected: {gain_2k}");
        assert!(gain_200.abs() < 0.1, "200 Hz should pass cleanly: {gain_200}");
    }

    #[test]
    fn global_shift_desplaza_frecuencia_en_semitonos() {
        let mut fx = EqPlusEffect::new(48_000.0);
        let mut p = EqPlusParams::default();
        p.bands[0] = EqPlusBandParams {
            enabled: true,
            band_type: EqPlusBandType::Bell,
            freq_hz: 500.0,
            gain_db: 6.0,
            q: 2.0,
        };
        p.shift_st = 12.0; // +1 octava -> 1000 Hz
        fx.set_params(p);
        let gain_1k = measure_sine_gain_db(&mut fx, 1000.0, 48_000.0);
        assert!((gain_1k - 6.0).abs() < 0.2, "Shifted +12 st should peak at 1kHz: {gain_1k}");
    }

    #[test]
    fn bitwig_midi_freq_defaults_coinciden() {
        let hz1 = bitwig_freq_midi_to_hz(39.48);
        assert!((hz1 - 80.0).abs() < 0.5, "hz1={hz1}");
        let hz8 = bitwig_freq_midi_to_hz(123.076);
        assert!((hz8 - 10_000.0).abs() < 50.0, "hz8={hz8}");
    }
}
