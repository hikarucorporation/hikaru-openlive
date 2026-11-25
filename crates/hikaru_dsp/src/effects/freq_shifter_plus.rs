// crates/hikaru_dsp/src/effects/freq_shifter_plus.rs

//! **Hikaru OpenFreqShifter+**: desplazador lineal de frecuencias de banda lateral única (**SSB**)
//! mediante redes IIR *all-pass* en cuadratura (transformada de Hilbert equiripple de Chebyshev)
//! con órdenes seleccionables (**`6P` / `12P` / `18P` / `24P`**), sobremuestreo polifásico
//! (**`Eco` 1x / `High` 2x / `Excellent` 4x**), inversión estéreo (**`↔`**), lazo de realimentación
//! con retardo interpolado (`ms` / sincronizado a BPM), cruce estéreo (**`↱↲`**), filtros **`FB Cuts`**
//! (HP/LP), **`FB Shift`** independiente, cadena anidada **`Wet FX`**, **`Width`** (Mid/Side) y **`Mix`**.
//!
//! Implementación 100% *clean-room* basada en literatura estándar de procesamiento digital de señales:
//! - Síntesis analítica de desfase constante de 90° vía media aritmético-geométrica (**AGM**) de
//!   funciones elípticas de Jacobi (Darlington 1950; Bedrosian 1960; Abramowitz & Stegun §16.4).
//! - Filtros *half-band* polifásicos *all-pass* de dos ramas para sobremuestreo libre de *aliasing*.
//! - Filtros SVF trapezoidales (Simper) en el lazo de realimentación + limitador suave `tanh`.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use hikaru_core::AudioBuffer;
use hikaru_plugin_host::{HostTransportInfo, PluginInstance, PluginParamInfo};

use crate::effects::WetFxChainHost;
use crate::plugin_node::DspPluginNode;

pub const FREQ_SHIFTER_MIN_CUT_HZ: f32 = 20.0;
pub const FREQ_SHIFTER_MAX_CUT_HZ: f32 = 20_000.0;
pub const FREQ_SHIFTER_MAX_DELAY_MS: f32 = 1_000.0;
pub const FREQ_SHIFTER_MAX_FEEDBACK: f32 = 0.98;

pub const FSP_PARAM_SHIFT_NORM: u32 = 0;
pub const FSP_PARAM_SHIFT_UNIT: u32 = 1;
pub const FSP_PARAM_STEREO_TWIST: u32 = 2;
pub const FSP_PARAM_POLE_ORDER: u32 = 3;
pub const FSP_PARAM_FB_CROSS: u32 = 4;
pub const FSP_PARAM_QUALITY: u32 = 5;
pub const FSP_PARAM_FB_HP_HZ: u32 = 6;
pub const FSP_PARAM_FB_LP_HZ: u32 = 7;
pub const FSP_PARAM_DELAY_MS: u32 = 8;
pub const FSP_PARAM_DELAY_SYNC: u32 = 9;
pub const FSP_PARAM_DELAY_SYNC_DIV: u32 = 10;
pub const FSP_PARAM_FEEDBACK: u32 = 11;
pub const FSP_PARAM_FB_SHIFT_NORM: u32 = 12;
pub const FSP_PARAM_FB_SHIFT_INDEP: u32 = 13;
pub const FSP_PARAM_WIDTH: u32 = 14;
pub const FSP_PARAM_MIX: u32 = 15;
pub const FSP_PARAM_WET_GAIN_DB: u32 = 16;
pub const FREQ_SHIFTER_PLUS_PARAM_COUNT: usize = 17;

const MAX_POLES_PER_BRANCH: usize = 12; // 24P total = 12 polos Rama A + 12 polos Rama B

// =============================================================================
// Enumeraciones de modos y escalas
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreqShiftUnit {
    /// Rango fino/medio `-500 .. +500 Hz` con ley cúbica firmada (`0.01 Hz` cerca de cero).
    Hz = 0,
    /// Rango amplio `-5.00 .. +5.00 kHz` para desplazamientos metálicos/inarmónicos tipo Bode.
    Khz = 1,
    /// Rango ultrafino `-5.00 .. +5.00 Hz` para *Barberpole phasers* y espacialización lenta.
    SubHz = 2,
    /// Sincronizado al tempo del proyecto (ciclos por pulso musical).
    Beat = 3,
}

impl FreqShiftUnit {
    pub const ALL: [Self; 4] = [Self::Hz, Self::Khz, Self::SubHz, Self::Beat];

    pub fn from_index(idx: usize) -> Self {
        match idx {
            1 => Self::Khz,
            2 => Self::SubHz,
            3 => Self::Beat,
            _ => Self::Hz,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Hz => "Hz",
            Self::Khz => "kHz",
            Self::SubHz => "Sub",
            Self::Beat => "Beat",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Hz => "±500 Hz (fine cubic resolution around 0.00 Hz)",
            Self::Khz => "±5.00 kHz wide-range metallic frequency shift",
            Self::SubHz => "±5.00 Hz ultra-fine stereo Barberpole phaser range",
            Self::Beat => "Tempo-synced frequency shift (cycles per beat)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreqShiftPoles {
    P6 = 0,
    P12 = 1,
    P18 = 2,
    P24 = 3,
}

impl FreqShiftPoles {
    pub const ALL: [Self; 4] = [Self::P6, Self::P12, Self::P18, Self::P24];

    pub fn from_index(idx: usize) -> Self {
        match idx {
            0 => Self::P6,
            1 => Self::P12,
            2 => Self::P18,
            _ => Self::P24,
        }
    }

    pub fn sections_per_branch(self) -> usize {
        match self {
            Self::P6 => 3,
            Self::P12 => 6,
            Self::P18 => 9,
            Self::P24 => 12,
        }
    }

    pub fn base_label(self) -> &'static str {
        match self {
            Self::P6 => "6",
            Self::P12 => "12",
            Self::P18 => "18",
            Self::P24 => "24",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::P6 => "6-Pole Hilbert all-pass network (warm vintage Bode phase response)",
            Self::P12 => "12-Pole Hilbert all-pass network (~70 dB sideband rejection)",
            Self::P18 => "18-Pole Hilbert all-pass network (~105 dB sideband rejection)",
            Self::P24 => "24-Pole Hilbert all-pass network (>130 dB surgical sideband rejection)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreqShiftQuality {
    Eco = 0,
    High = 1,
    Excellent = 2,
}

impl FreqShiftQuality {
    pub const ALL: [Self; 3] = [Self::Eco, Self::High, Self::Excellent];

    pub fn from_index(idx: usize) -> Self {
        match idx {
            0 => Self::Eco,
            1 => Self::High,
            _ => Self::Excellent,
        }
    }

    pub fn oversample_factor(self) -> usize {
        match self {
            Self::Eco => 1,
            Self::High => 2,
            Self::Excellent => 4,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Eco => "Eco",
            Self::High => "High",
            Self::Excellent => "Excellent",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Eco => "1x native sample rate (lowest CPU)",
            Self::High => "2x polyphase half-band oversampling",
            Self::Excellent => "4x polyphase half-band oversampling (maximum Nyquist clarity)",
        }
    }
}

pub const DELAY_SYNC_LABELS: [&str; 10] = [
    "1/64", "1/32", "1/16T", "1/16", "1/8T", "1/8", "1/4T", "1/4", "1/2", "1/1",
];

pub const DELAY_SYNC_BEATS: [f32; 10] = [
    0.0625,
    0.125,
    1.0 / 6.0,
    0.25,
    1.0 / 3.0,
    0.5,
    2.0 / 3.0,
    1.0,
    2.0,
    4.0,
];

// =============================================================================
// Parámetros públicos y conversiones normalizadas
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FreqShifterPlusParams {
    /// Posición bipolar normalizada `0.0 .. 1.0` (`0.5` = `0.00`).
    pub shift_norm: f32,
    pub shift_unit: FreqShiftUnit,
    /// Inversión estéreo (`↔`): cuando está activo, el canal R desplaza en dirección opuesta (`-Δf`).
    pub stereo_twist: bool,
    pub pole_order: FreqShiftPoles,
    /// Cruce L↔R en el lazo de realimentación (`↱↲`).
    pub fb_cross: bool,
    pub quality: FreqShiftQuality,
    /// Corte paso-alto del lazo de realimentación (`20 .. 20_000 Hz`).
    pub fb_hp_hz: f32,
    /// Corte paso-bajo del lazo de realimentación (`20 .. 20_000 Hz`).
    pub fb_lp_hz: f32,
    /// Tiempo de retardo en milisegundos (`0.0 .. 1000.0 ms`).
    pub delay_ms: f32,
    /// `false` = modo `D` en milisegundos; `true` = modo sincronizado al tempo (`1/64 .. 1/1`).
    pub delay_sync: bool,
    /// Índice de división rítmica `0 .. 9` cuando `delay_sync == true`.
    pub delay_sync_div: u8,
    /// Realimentación bipolar (`-0.98 .. +0.98`, `0.0` = sin realimentación).
    pub feedback: f32,
    /// Desplazamiento adicional en el lazo de realimentación (`0.0 .. 1.0` bipolar, `0.5 = 0 Hz`).
    pub fb_shift_norm: f32,
    /// Cuando está activo (`||┌||`), el retorno de feedback no vuelve a pasar por `Main Shift`
    /// y se desplaza únicamente según `FB Shift`.
    pub fb_shift_independent: bool,
    /// Anchura estéreo Mid/Side de la señal wet (`0.0 .. 2.0`, `1.0` = 100%).
    pub width: f32,
    /// Mezcla Dry/Wet (`0.0 .. 1.0`, `1.0` = 100% Wet).
    pub mix: f32,
    /// Ganancia interna opcional post-Wet (`-12.0 .. +12.0 dB`).
    pub wet_gain_db: f32,
}

impl Default for FreqShifterPlusParams {
    fn default() -> Self {
        Self {
            shift_norm: 0.5,
            shift_unit: FreqShiftUnit::Hz,
            stereo_twist: false,
            pole_order: FreqShiftPoles::P24,
            fb_cross: true,
            quality: FreqShiftQuality::Excellent,
            fb_hp_hz: FREQ_SHIFTER_MIN_CUT_HZ,
            fb_lp_hz: FREQ_SHIFTER_MAX_CUT_HZ,
            delay_ms: 0.0,
            delay_sync: false,
            delay_sync_div: 3, // 1/16
            feedback: 0.0,
            fb_shift_norm: 0.5,
            fb_shift_independent: false,
            width: 1.0,
            mix: 1.0,
            wet_gain_db: 0.0,
        }
    }
}

impl FreqShifterPlusParams {
    pub fn clamped(self) -> Self {
        Self {
            shift_norm: if self.shift_norm.is_finite() {
                self.shift_norm.clamp(0.0, 1.0)
            } else {
                0.5
            },
            shift_unit: self.shift_unit,
            stereo_twist: self.stereo_twist,
            pole_order: self.pole_order,
            fb_cross: self.fb_cross,
            quality: self.quality,
            fb_hp_hz: if self.fb_hp_hz.is_finite() {
                self.fb_hp_hz
                    .clamp(FREQ_SHIFTER_MIN_CUT_HZ, FREQ_SHIFTER_MAX_CUT_HZ)
            } else {
                FREQ_SHIFTER_MIN_CUT_HZ
            },
            fb_lp_hz: if self.fb_lp_hz.is_finite() {
                self.fb_lp_hz
                    .clamp(FREQ_SHIFTER_MIN_CUT_HZ, FREQ_SHIFTER_MAX_CUT_HZ)
            } else {
                FREQ_SHIFTER_MAX_CUT_HZ
            },
            delay_ms: if self.delay_ms.is_finite() {
                self.delay_ms.clamp(0.0, FREQ_SHIFTER_MAX_DELAY_MS)
            } else {
                0.0
            },
            delay_sync: self.delay_sync,
            delay_sync_div: self.delay_sync_div.min((DELAY_SYNC_BEATS.len() - 1) as u8),
            feedback: if self.feedback.is_finite() {
                self.feedback
                    .clamp(-FREQ_SHIFTER_MAX_FEEDBACK, FREQ_SHIFTER_MAX_FEEDBACK)
            } else {
                0.0
            },
            fb_shift_norm: if self.fb_shift_norm.is_finite() {
                self.fb_shift_norm.clamp(0.0, 1.0)
            } else {
                0.5
            },
            fb_shift_independent: self.fb_shift_independent,
            width: if self.width.is_finite() {
                self.width.clamp(0.0, 2.0)
            } else {
                1.0
            },
            mix: if self.mix.is_finite() {
                self.mix.clamp(0.0, 1.0)
            } else {
                1.0
            },
            wet_gain_db: if self.wet_gain_db.is_finite() {
                self.wet_gain_db.clamp(-12.0, 12.0)
            } else {
                0.0
            },
        }
    }

    #[inline]
    pub fn cut_hz_to_norm(hz: f32) -> f32 {
        let ratio = FREQ_SHIFTER_MAX_CUT_HZ / FREQ_SHIFTER_MIN_CUT_HZ;
        ((hz.clamp(FREQ_SHIFTER_MIN_CUT_HZ, FREQ_SHIFTER_MAX_CUT_HZ) / FREQ_SHIFTER_MIN_CUT_HZ)
            .ln()
            / ratio.ln())
        .clamp(0.0, 1.0)
    }

    #[inline]
    pub fn cut_norm_to_hz(norm: f32) -> f32 {
        let ratio = FREQ_SHIFTER_MAX_CUT_HZ / FREQ_SHIFTER_MIN_CUT_HZ;
        FREQ_SHIFTER_MIN_CUT_HZ * ratio.powf(norm.clamp(0.0, 1.0))
    }

    #[inline]
    pub fn delay_ms_to_norm(ms: f32) -> f32 {
        (ms.clamp(0.0, FREQ_SHIFTER_MAX_DELAY_MS) / FREQ_SHIFTER_MAX_DELAY_MS)
            .cbrt()
            .clamp(0.0, 1.0)
    }

    #[inline]
    pub fn delay_norm_to_ms(norm: f32) -> f32 {
        let n = norm.clamp(0.0, 1.0);
        FREQ_SHIFTER_MAX_DELAY_MS * n * n * n
    }

    /// Calcula el desplazamiento principal en Hz para el modo actual y el BPM del host.
    #[inline]
    pub fn shift_hz(&self, tempo_bpm: f64) -> f32 {
        let u = (self.shift_norm.clamp(0.0, 1.0) * 2.0 - 1.0) as f64;
        if u.abs() < 1.0e-6 {
            return 0.0;
        }
        let hz = match self.shift_unit {
            FreqShiftUnit::Hz => 500.0 * u * u * u,
            FreqShiftUnit::Khz => 5_000.0 * u.signum() * u.abs().powf(2.2),
            FreqShiftUnit::SubHz => 5.0 * u,
            FreqShiftUnit::Beat => {
                let bpm = tempo_bpm.clamp(20.0, 400.0);
                let beats_per_sec = bpm / 60.0;
                (8.0 * u * u * u) * beats_per_sec
            }
        };
        hz as f32
    }

    /// Valor numérico mostrado en el display verde junto al selector de unidad.
    pub fn shift_display_number(&self, tempo_bpm: f64) -> f32 {
        let u = (self.shift_norm.clamp(0.0, 1.0) * 2.0 - 1.0) as f64;
        if u.abs() < 1.0e-6 {
            return 0.0;
        }
        match self.shift_unit {
            FreqShiftUnit::Hz => (500.0 * u * u * u) as f32,
            FreqShiftUnit::Khz => (5.0 * u.signum() * u.abs().powf(2.2)) as f32,
            FreqShiftUnit::SubHz => (5.0 * u) as f32,
            FreqShiftUnit::Beat => {
                let _ = tempo_bpm;
                (8.0 * u * u * u) as f32
            }
        }
    }

    pub fn format_shift_readout(&self, tempo_bpm: f64) -> String {
        let v = self.shift_display_number(tempo_bpm);
        let abs_v = v.abs();
        if abs_v < 0.005 {
            "0.00".to_string()
        } else if abs_v < 10.0 {
            format!("{v:.2}")
        } else if abs_v < 100.0 {
            format!("{v:.1}")
        } else {
            format!("{v:.0}")
        }
    }

    /// Convierte un valor escrito por el usuario (en la unidad activa) a `shift_norm` (`0..1`).
    pub fn set_shift_from_display_value(&mut self, val: f32) {
        if !val.is_finite() {
            return;
        }
        let u = match self.shift_unit {
            FreqShiftUnit::Hz => {
                let ratio = (val.clamp(-500.0, 500.0) / 500.0) as f64;
                ratio.signum() * ratio.abs().cbrt()
            }
            FreqShiftUnit::Khz => {
                let ratio = (val.clamp(-5.0, 5.0) / 5.0) as f64;
                ratio.signum() * ratio.abs().powf(1.0 / 2.2)
            }
            FreqShiftUnit::SubHz => (val.clamp(-5.0, 5.0) / 5.0) as f64,
            FreqShiftUnit::Beat => {
                let ratio = (val.clamp(-8.0, 8.0) / 8.0) as f64;
                ratio.signum() * ratio.abs().cbrt()
            }
        };
        self.shift_norm = ((u * 0.5 + 0.5) as f32).clamp(0.0, 1.0);
    }

    /// Desplazamiento adicional dentro del lazo de realimentación (`-500 .. +500 Hz`).
    #[inline]
    pub fn fb_shift_hz(&self) -> f32 {
        let u = (self.fb_shift_norm.clamp(0.0, 1.0) * 2.0 - 1.0) as f64;
        if u.abs() < 1.0e-6 {
            0.0
        } else {
            (500.0 * u * u * u) as f32
        }
    }

    /// Tiempo de retardo efectivo en segundos.
    #[inline]
    pub fn delay_seconds(&self, tempo_bpm: f64) -> f32 {
        if self.delay_sync {
            let idx = (self.delay_sync_div as usize).min(DELAY_SYNC_BEATS.len() - 1);
            let beats = DELAY_SYNC_BEATS[idx];
            let bpm = tempo_bpm.clamp(20.0, 400.0) as f32;
            ((60.0 / bpm) * beats).clamp(0.0, 2.4)
        } else {
            (self.delay_ms.clamp(0.0, FREQ_SHIFTER_MAX_DELAY_MS) * 0.001).clamp(0.0, 1.0)
        }
    }

    pub fn format_delay_readout(&self) -> String {
        if self.delay_sync {
            let idx = (self.delay_sync_div as usize).min(DELAY_SYNC_LABELS.len() - 1);
            DELAY_SYNC_LABELS[idx].to_string()
        } else if self.delay_ms < 10.0 {
            format!("{:.2} ms", self.delay_ms)
        } else if self.delay_ms < 100.0 {
            format!("{:.1} ms", self.delay_ms)
        } else {
            format!("{:.0} ms", self.delay_ms)
        }
    }

    pub fn get_normalized(&self, id: u32) -> Option<f32> {
        match id {
            FSP_PARAM_SHIFT_NORM => Some(self.shift_norm.clamp(0.0, 1.0)),
            FSP_PARAM_SHIFT_UNIT => Some((self.shift_unit as u32) as f32 / 3.0),
            FSP_PARAM_STEREO_TWIST => Some(if self.stereo_twist { 1.0 } else { 0.0 }),
            FSP_PARAM_POLE_ORDER => Some((self.pole_order as u32) as f32 / 3.0),
            FSP_PARAM_FB_CROSS => Some(if self.fb_cross { 1.0 } else { 0.0 }),
            FSP_PARAM_QUALITY => Some((self.quality as u32) as f32 / 2.0),
            FSP_PARAM_FB_HP_HZ => Some(Self::cut_hz_to_norm(self.fb_hp_hz)),
            FSP_PARAM_FB_LP_HZ => Some(Self::cut_hz_to_norm(self.fb_lp_hz)),
            FSP_PARAM_DELAY_MS => Some(Self::delay_ms_to_norm(self.delay_ms)),
            FSP_PARAM_DELAY_SYNC => Some(if self.delay_sync { 1.0 } else { 0.0 }),
            FSP_PARAM_DELAY_SYNC_DIV => {
                Some((self.delay_sync_div as f32 / (DELAY_SYNC_BEATS.len() - 1) as f32).clamp(0.0, 1.0))
            }
            FSP_PARAM_FEEDBACK => Some(
                ((self.feedback / FREQ_SHIFTER_MAX_FEEDBACK) * 0.5 + 0.5).clamp(0.0, 1.0),
            ),
            FSP_PARAM_FB_SHIFT_NORM => Some(self.fb_shift_norm.clamp(0.0, 1.0)),
            FSP_PARAM_FB_SHIFT_INDEP => Some(if self.fb_shift_independent { 1.0 } else { 0.0 }),
            FSP_PARAM_WIDTH => Some((self.width * 0.5).clamp(0.0, 1.0)),
            FSP_PARAM_MIX => Some(self.mix.clamp(0.0, 1.0)),
            FSP_PARAM_WET_GAIN_DB => Some(((self.wet_gain_db + 12.0) / 24.0).clamp(0.0, 1.0)),
            _ => None,
        }
    }

    pub fn set_normalized(&mut self, id: u32, value: f32) -> bool {
        if !value.is_finite() {
            return false;
        }
        let v = value.clamp(0.0, 1.0);
        match id {
            FSP_PARAM_SHIFT_NORM => self.shift_norm = v,
            FSP_PARAM_SHIFT_UNIT => {
                self.shift_unit = FreqShiftUnit::from_index((v * 3.0).round() as usize);
            }
            FSP_PARAM_STEREO_TWIST => self.stereo_twist = v >= 0.5,
            FSP_PARAM_POLE_ORDER => {
                self.pole_order = FreqShiftPoles::from_index((v * 3.0).round() as usize);
            }
            FSP_PARAM_FB_CROSS => self.fb_cross = v >= 0.5,
            FSP_PARAM_QUALITY => {
                self.quality = FreqShiftQuality::from_index((v * 2.0).round() as usize);
            }
            FSP_PARAM_FB_HP_HZ => self.fb_hp_hz = Self::cut_norm_to_hz(v),
            FSP_PARAM_FB_LP_HZ => self.fb_lp_hz = Self::cut_norm_to_hz(v),
            FSP_PARAM_DELAY_MS => self.delay_ms = Self::delay_norm_to_ms(v),
            FSP_PARAM_DELAY_SYNC => self.delay_sync = v >= 0.5,
            FSP_PARAM_DELAY_SYNC_DIV => {
                self.delay_sync_div =
                    ((v * (DELAY_SYNC_BEATS.len() - 1) as f32).round() as u8)
                        .min((DELAY_SYNC_BEATS.len() - 1) as u8);
            }
            FSP_PARAM_FEEDBACK => {
                self.feedback = (v * 2.0 - 1.0) * FREQ_SHIFTER_MAX_FEEDBACK;
            }
            FSP_PARAM_FB_SHIFT_NORM => self.fb_shift_norm = v,
            FSP_PARAM_FB_SHIFT_INDEP => self.fb_shift_independent = v >= 0.5,
            FSP_PARAM_WIDTH => self.width = v * 2.0,
            FSP_PARAM_MIX => self.mix = v,
            FSP_PARAM_WET_GAIN_DB => self.wet_gain_db = -12.0 + v * 24.0,
            _ => return false,
        }
        *self = self.clamped();
        true
    }
}

// =============================================================================
// Síntesis AGM de red IIR All-Pass de Hilbert (90° equiripple de Chebyshev)
// =============================================================================

/// Calcula `sc(alpha * K(k), k) = tan(am(alpha * K(k), k))` mediante la media
/// aritmético-geométrica (AGM / recurrencia de Landen, Abramowitz & Stegun §16.4.3).
fn jacobi_sc_quarter_fraction(alpha: f64, k_prime: f64) -> f64 {
    const STEPS: usize = 10;
    let kp = k_prime.clamp(1.0e-9, 1.0);
    if kp >= 1.0 - 1.0e-12 {
        return (alpha * FRAC_PI_2).tan();
    }

    let mut a = [0.0_f64; STEPS + 1];
    let mut b = [0.0_f64; STEPS + 1];
    let mut c = [0.0_f64; STEPS + 1];
    a[0] = 1.0;
    b[0] = kp;
    c[0] = (1.0 - kp * kp).max(0.0).sqrt();

    for i in 0..STEPS {
        a[i + 1] = 0.5 * (a[i] + b[i]);
        b[i + 1] = (a[i] * b[i]).sqrt();
        c[i + 1] = 0.5 * (a[i] - b[i]);
    }

    let mut phi = (1_u64 << STEPS) as f64 * alpha * FRAC_PI_2;
    for i in (1..=STEPS).rev() {
        let ratio = (c[i] / a[i]).clamp(-1.0, 1.0);
        let arg = (ratio * phi.sin()).clamp(-1.0, 1.0);
        phi = 0.5 * (phi + arg.asin());
    }
    phi.tan().max(1.0e-12)
}

/// Diseña los coeficientes bilineales `alpha_m` de las dos ramas *all-pass* `A` y `B`
/// para mantener `phi_A(w) - phi_B(w) ≈ +pi/2` sobre `[f_low, f_high]`.
fn design_hilbert_allpass_branches(
    sample_rate: f64,
    poles: FreqShiftPoles,
) -> (
    [f64; MAX_POLES_PER_BRANCH],
    [f64; MAX_POLES_PER_BRANCH],
    usize,
) {
    let sr = sample_rate.max(8_000.0);
    let n = poles.sections_per_branch();
    let total_poles = 2 * n;

    // Banda objetivo pre-deformada bilinealmente: desde sub-graves (12 Hz) hasta cerca de Nyquist.
    let f_low = 12.0_f64.min(sr * 0.05);
    let f_high = (sr * 0.485).min(24_000.0 * (sr / 48_000.0).max(1.0));
    let w1 = 2.0 * sr * (PI * f_low / sr).tan();
    let w2 = 2.0 * sr * (PI * f_high / sr).tan();
    let k_prime = (w1 / w2).clamp(1.0e-8, 0.999_999);

    let mut branch_a = [0.0_f64; MAX_POLES_PER_BRANCH];
    let mut branch_b = [0.0_f64; MAX_POLES_PER_BRANCH];

    for m in 0..total_poles {
        let frac = (2 * m + 1) as f64 / (4 * n) as f64;
        let sc = jacobi_sc_quarter_fraction(frac, k_prime);
        let omega_m = w2 / sc;
        let alpha = ((2.0 * sr - omega_m) / (2.0 * sr + omega_m)).clamp(-0.999_999_5, 0.999_999_5);
        if m % 2 == 0 {
            branch_a[m / 2] = alpha;
        } else {
            branch_b[m / 2] = alpha;
        }
    }

    (branch_a, branch_b, n)
}

#[derive(Clone, Copy, Default)]
struct AllPass1 {
    prev_x: f64,
    prev_y: f64,
}

impl AllPass1 {
    /// Función de transferencia `H(z) = (z^-1 - alpha) / (1 - alpha * z^-1)`:
    /// `y[n] = -alpha * x[n] + x[n-1] + alpha * y[n-1]`.
    #[inline]
    fn process(&mut self, x: f64, alpha: f64) -> f64 {
        let y = -alpha * x + self.prev_x + alpha * self.prev_y;
        self.prev_x = x;
        // Guard contra denormales en silencio prolongado
        self.prev_y = if y.abs() < 1.0e-30 { 0.0 } else { y };
        self.prev_y
    }
}

#[derive(Clone, Copy)]
struct HilbertNetwork {
    coeff_a: [f64; MAX_POLES_PER_BRANCH],
    coeff_b: [f64; MAX_POLES_PER_BRANCH],
    state_a: [[AllPass1; MAX_POLES_PER_BRANCH]; 2],
    state_b: [[AllPass1; MAX_POLES_PER_BRANCH]; 2],
    sections: usize,
}

impl HilbertNetwork {
    fn new(sample_rate: f64, poles: FreqShiftPoles) -> Self {
        let (coeff_a, coeff_b, sections) = design_hilbert_allpass_branches(sample_rate, poles);
        Self {
            coeff_a,
            coeff_b,
            state_a: [[AllPass1::default(); MAX_POLES_PER_BRANCH]; 2],
            state_b: [[AllPass1::default(); MAX_POLES_PER_BRANCH]; 2],
            sections,
        }
    }

    fn reconfigure(&mut self, sample_rate: f64, poles: FreqShiftPoles) {
        let (coeff_a, coeff_b, sections) = design_hilbert_allpass_branches(sample_rate, poles);
        let order_changed = self.sections != sections;
        self.coeff_a = coeff_a;
        self.coeff_b = coeff_b;
        self.sections = sections;
        if order_changed {
            self.reset();
        }
    }

    fn reset(&mut self) {
        self.state_a = [[AllPass1::default(); MAX_POLES_PER_BRANCH]; 2];
        self.state_b = [[AllPass1::default(); MAX_POLES_PER_BRANCH]; 2];
    }

    /// Devuelve el par analítico `(I, Q)` para el canal `ch` (`0 = L`, `1 = R`),
    /// donde `Q` presenta un desfase de `-90°` respecto a `I` (`Q ≈ Hilbert(I)`).
    #[inline]
    fn process_channel(&mut self, ch: usize, input: f64) -> (f64, f64) {
        let mut i_sig = input;
        let mut q_sig = input;
        for s in 0..self.sections {
            i_sig = self.state_a[ch][s].process(i_sig, self.coeff_a[s]);
            q_sig = self.state_b[ch][s].process(q_sig, self.coeff_b[s]);
        }
        (i_sig, q_sig)
    }
}

// =============================================================================
// Sobremuestreador polifásico Half-Band All-Pass (2x / 4x)
// =============================================================================

/// Sección *all-pass* de primer orden para filtro *half-band* polifásico:
/// `A(w) = (a + w^-1) / (1 + a * w^-1)` -> ganancia unitaria `+1` en DC (`w = 1`).
#[derive(Clone, Copy, Default)]
struct HbAllPass1 {
    state: f64,
}

impl HbAllPass1 {
    #[inline]
    fn process(&mut self, x: f64, a: f64) -> f64 {
        // Forma canónica de 1 multiplicador:
        // y[n] = a * x[n] + s[n-1]; s[n] = x[n] - a * y[n]
        let out = a * x + self.state;
        let next_s = x - a * out;
        self.state = if next_s.abs() < 1.0e-30 { 0.0 } else { next_s };
        out
    }
}

/// Filtro *half-band* elíptico polifásico de 8º orden (2 secciones por rama, >78 dB rechazo).
const HB_COEFF_A0: [f64; 2] = [0.079_866_426_236_357_51, 0.545_353_651_071_132_2];
const HB_COEFF_A1: [f64; 2] = [0.283_829_344_874_109_93, 0.834_411_891_480_737_9];

#[derive(Clone, Copy, Default)]
struct HalfbandStageStereo {
    up_b0: [[HbAllPass1; 2]; 2],
    up_b1: [[HbAllPass1; 2]; 2],
    dn_b0: [[HbAllPass1; 2]; 2],
    dn_b1: [[HbAllPass1; 2]; 2],
}

impl HalfbandStageStereo {
    fn reset(&mut self) {
        *self = Self::default();
    }

    #[inline]
    fn upsample2(&mut self, frame: [f64; 2]) -> [[f64; 2]; 2] {
        let mut even = [0.0_f64; 2];
        let mut odd = [0.0_f64; 2];
        for ch in 0..2 {
            let mut e = frame[ch];
            let mut o = frame[ch];
            for s in 0..2 {
                e = self.up_b0[ch][s].process(e, HB_COEFF_A0[s]);
                o = self.up_b1[ch][s].process(o, HB_COEFF_A1[s]);
            }
            even[ch] = e;
            odd[ch] = o;
        }
        [even, odd]
    }

    #[inline]
    fn downsample2(&mut self, s0: [f64; 2], s1: [f64; 2]) -> [f64; 2] {
        let mut out = [0.0_f64; 2];
        for ch in 0..2 {
            // H(z) = 0.5 * (A0(z^2) + z^-1 * A1(z^2)):
            // En m = 2n + 1, la muestra impar `s1` entra en A0 y la par previa `s0` entra en A1.
            let mut a = s1[ch];
            let mut b = s0[ch];
            for s in 0..2 {
                a = self.dn_b0[ch][s].process(a, HB_COEFF_A0[s]);
                b = self.dn_b1[ch][s].process(b, HB_COEFF_A1[s]);
            }
            out[ch] = 0.5 * (a + b);
        }
        out
    }
}

// =============================================================================
// Filtros SVF Trapezoidales (Butterworth Q = 1/sqrt(2)) para FB Cuts (HP / LP)
// =============================================================================

#[derive(Clone, Copy, Default)]
struct CytomicSvfStereo {
    ic1eq: [f64; 2],
    ic2eq: [f64; 2],
}

impl CytomicSvfStereo {
    #[inline]
    fn process_hp(&mut self, x: [f64; 2], cutoff_hz: f64, sr: f64) -> [f64; 2] {
        let g = (PI * cutoff_hz.clamp(10.0, sr * 0.49) / sr).tan();
        let k = std::f64::consts::SQRT_2; // Q = 0.7071 (Butterworth)
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let mut out = [0.0_f64; 2];
        for ch in 0..2 {
            let v3 = x[ch] - self.ic2eq[ch];
            let v1 = a1 * self.ic1eq[ch] + a2 * v3;
            let v2 = self.ic2eq[ch] + a2 * self.ic1eq[ch] + a3 * v3;
            let c1 = 2.0 * v1 - self.ic1eq[ch];
            let c2 = 2.0 * v2 - self.ic2eq[ch];
            self.ic1eq[ch] = if c1.abs() < 1.0e-30 { 0.0 } else { c1 };
            self.ic2eq[ch] = if c2.abs() < 1.0e-30 { 0.0 } else { c2 };
            out[ch] = x[ch] - k * v1 - v2;
        }
        out
    }

    #[inline]
    fn process_lp(&mut self, x: [f64; 2], cutoff_hz: f64, sr: f64) -> [f64; 2] {
        let g = (PI * cutoff_hz.clamp(10.0, sr * 0.49) / sr).tan();
        let k = std::f64::consts::SQRT_2;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let mut out = [0.0_f64; 2];
        for ch in 0..2 {
            let v3 = x[ch] - self.ic2eq[ch];
            let v1 = a1 * self.ic1eq[ch] + a2 * v3;
            let v2 = self.ic2eq[ch] + a2 * self.ic1eq[ch] + a3 * v3;
            let c1 = 2.0 * v1 - self.ic1eq[ch];
            let c2 = 2.0 * v2 - self.ic2eq[ch];
            self.ic1eq[ch] = if c1.abs() < 1.0e-30 { 0.0 } else { c1 };
            self.ic2eq[ch] = if c2.abs() < 1.0e-30 { 0.0 } else { c2 };
            out[ch] = v2;
        }
        out
    }
}

// =============================================================================
// Procesador principal FreqShifterPlusEffect
// =============================================================================

pub struct FreqShifterPlusEffect {
    params: FreqShifterPlusParams,
    sample_rate: f32,
    tempo_bpm: f64,

    main_hilbert: HilbertNetwork,
    fb_hilbert: HilbertNetwork,
    main_osc_phase: [f64; 2],
    fb_osc_phase: [f64; 2],

    hb_stage_2x: HalfbandStageStereo,
    hb_stage_4x_a: HalfbandStageStereo,
    hb_stage_4x_b: HalfbandStageStereo,

    delay_buf: Vec<[f32; 2]>,
    delay_write: usize,
    fb_state: [f64; 2],
    fb_dc_prev_in: [f64; 2],
    fb_dc_prev_out: [f64; 2],
    fb_hp_svf: CytomicSvfStereo,
    fb_lp_svf: CytomicSvfStereo,

    // Suavizado libre de zippers para parámetros continuos
    smooth_shift_hz: f64,
    smooth_fb_shift_hz: f64,
    smooth_delay_samp: f64,
    smooth_feedback: f64,

    wet_chain: Vec<DspPluginNode>,
    dry_scratch: Vec<f32>,
}

impl FreqShifterPlusEffect {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        let params = FreqShifterPlusParams::default();
        let internal_sr = sr as f64 * params.quality.oversample_factor() as f64;
        let max_delay_frames = ((sr as usize) * 3).max(8_192);

        Self {
            params,
            sample_rate: sr,
            tempo_bpm: 120.0,
            main_hilbert: HilbertNetwork::new(internal_sr, params.pole_order),
            fb_hilbert: HilbertNetwork::new(sr as f64, FreqShiftPoles::P12),
            main_osc_phase: [0.0; 2],
            fb_osc_phase: [0.0; 2],
            hb_stage_2x: HalfbandStageStereo::default(),
            hb_stage_4x_a: HalfbandStageStereo::default(),
            hb_stage_4x_b: HalfbandStageStereo::default(),
            delay_buf: vec![[0.0; 2]; max_delay_frames],
            delay_write: 0,
            fb_state: [0.0; 2],
            fb_dc_prev_in: [0.0; 2],
            fb_dc_prev_out: [0.0; 2],
            fb_hp_svf: CytomicSvfStereo::default(),
            fb_lp_svf: CytomicSvfStereo::default(),
            smooth_shift_hz: 0.0,
            smooth_fb_shift_hz: 0.0,
            smooth_delay_samp: 0.0,
            smooth_feedback: 0.0,
            wet_chain: Vec::new(),
            dry_scratch: Vec::new(),
        }
    }

    pub fn params(&self) -> FreqShifterPlusParams {
        self.params
    }

    pub fn wet_chain(&self) -> &[DspPluginNode] {
        &self.wet_chain
    }

    pub fn set_params(&mut self, next: FreqShifterPlusParams) {
        let clamped = next.clamped();
        let prev = self.params;
        self.params = clamped;
        if prev.pole_order != clamped.pole_order || prev.quality != clamped.quality {
            let internal_sr =
                self.sample_rate as f64 * clamped.quality.oversample_factor() as f64;
            self.main_hilbert.reconfigure(internal_sr, clamped.pole_order);
            if prev.quality != clamped.quality {
                self.hb_stage_2x.reset();
                self.hb_stage_4x_a.reset();
                self.hb_stage_4x_b.reset();
            }
        }
    }

    fn rebuild_for_sample_rate(&mut self) {
        let sr = self.sample_rate as f64;
        let internal_sr = sr * self.params.quality.oversample_factor() as f64;
        self.main_hilbert
            .reconfigure(internal_sr, self.params.pole_order);
        self.fb_hilbert.reconfigure(sr, FreqShiftPoles::P12);
        let needed = ((self.sample_rate as usize) * 3).max(8_192);
        if self.delay_buf.len() < needed {
            self.delay_buf.resize(needed, [0.0; 2]);
            self.delay_write %= self.delay_buf.len();
        }
    }

    /// Ejecuta una muestra a la frecuencia interna sobremuestreada sobre el desplazador SSB principal.
    #[inline]
    fn process_main_ssb_subframe(
        &mut self,
        x: [f64; 2],
        shift_hz_l: f64,
        shift_hz_r: f64,
        inv_internal_sr: f64,
    ) -> [f64; 2] {
        let shifts = [shift_hz_l, shift_hz_r];
        let mut out = [0.0_f64; 2];
        for ch in 0..2 {
            let (i_sig, q_sig) = self.main_hilbert.process_channel(ch, x[ch]);
            let df = shifts[ch];
            if df.abs() < 1.0e-7 && self.main_osc_phase[ch].abs() < 1.0e-7 {
                out[ch] = i_sig;
            } else {
                let ph = self.main_osc_phase[ch];
                let (sin_p, cos_p) = ph.sin_cos();
                out[ch] = i_sig * cos_p - q_sig * sin_p;
                let mut next_ph = ph + TAU * df * inv_internal_sr;
                if next_ph >= TAU || next_ph <= -TAU {
                    next_ph %= TAU;
                }
                self.main_osc_phase[ch] = next_ph;
            }
        }
        out
    }

    /// Interpolación cúbica Hermite (Catmull-Rom) sobre la línea de retardo circular.
    #[inline]
    fn read_delay_hermite(&self, delay_samples: f64) -> [f64; 2] {
        let len = self.delay_buf.len();
        if delay_samples <= 1.0e-4 {
            let prev_idx = if self.delay_write == 0 {
                len - 1
            } else {
                self.delay_write - 1
            };
            let s = self.delay_buf[prev_idx];
            return [s[0] as f64, s[1] as f64];
        }

        let max_d = (len.saturating_sub(4)) as f64;
        let d = delay_samples.clamp(0.0, max_d);
        let d_int = d.floor() as usize;
        let frac = d - d_int as f64;

        // `delay_write` apunta al slot que se escribirá en esta muestra; `delay_write - 1` es `z^-1`.
        let base = (self.delay_write + len - 1 - d_int) % len;
        let xm1 = self.delay_buf[(base + 1) % len];
        let x0 = self.delay_buf[base];
        let x1 = self.delay_buf[(base + len - 1) % len];
        let x2 = self.delay_buf[(base + len - 2) % len];

        let mut out = [0.0_f64; 2];
        for ch in 0..2 {
            let ym1 = xm1[ch] as f64;
            let y0 = x0[ch] as f64;
            let y1 = x1[ch] as f64;
            let y2 = x2[ch] as f64;
            let c0 = y0;
            let c1 = 0.5 * (y1 - ym1);
            let c2 = ym1 - 2.5 * y0 + 2.0 * y1 - 0.5 * y2;
            let c3 = 0.5 * (y2 - ym1) + 1.5 * (y0 - y1);
            out[ch] = ((c3 * frac + c2) * frac + c1) * frac + c0;
        }
        out
    }

    /// Procesa el núcleo completo del desplazador de frecuencias + lazo de realimentación,
    /// devolviendo únicamente la señal `wet` (antes de `Wet FX`, `Width` y `Mix`).
    #[inline]
    pub fn process_wet_stereo(&mut self, in_l: f32, in_r: f32) -> (f32, f32) {
        let sr = self.sample_rate as f64;
        let p = self.params;

        let target_shift = p.shift_hz(self.tempo_bpm) as f64;
        let target_fb_shift = p.fb_shift_hz() as f64;
        let target_delay_samp = (p.delay_seconds(self.tempo_bpm) as f64) * sr;
        let target_fb = p.feedback as f64;

        // Suavizado exponencial (~4 ms) para eliminar artefactos al mover perillas
        let slew = 1.0 - (-1.0 / (0.004 * sr)).exp();
        self.smooth_shift_hz += slew * (target_shift - self.smooth_shift_hz);
        self.smooth_fb_shift_hz += slew * (target_fb_shift - self.smooth_fb_shift_hz);
        self.smooth_delay_samp += slew * (target_delay_samp - self.smooth_delay_samp);
        self.smooth_feedback += slew * (target_fb - self.smooth_feedback);

        let fb_gain = self.smooth_feedback;
        let fb_return = if p.fb_shift_independent {
            [0.0, 0.0]
        } else {
            [self.fb_state[0] * fb_gain, self.fb_state[1] * fb_gain]
        };

        let main_in = [
            (in_l as f64 + fb_return[0]).clamp(-8.0, 8.0),
            (in_r as f64 + fb_return[1]).clamp(-8.0, 8.0),
        ];

        let shift_l = self.smooth_shift_hz;
        let shift_r = if p.stereo_twist {
            -self.smooth_shift_hz
        } else {
            self.smooth_shift_hz
        };

        // Etapa principal SSB con sobremuestreo seleccionable (1x / 2x / 4x)
        let mut wet = match p.quality {
            FreqShiftQuality::Eco => {
                self.process_main_ssb_subframe(main_in, shift_l, shift_r, 1.0 / sr)
            }
            FreqShiftQuality::High => {
                let inv_sr2 = 1.0 / (sr * 2.0);
                let up = self.hb_stage_2x.upsample2(main_in);
                let y0 = self.process_main_ssb_subframe(up[0], shift_l, shift_r, inv_sr2);
                let y1 = self.process_main_ssb_subframe(up[1], shift_l, shift_r, inv_sr2);
                self.hb_stage_2x.downsample2(y0, y1)
            }
            FreqShiftQuality::Excellent => {
                let inv_sr4 = 1.0 / (sr * 4.0);
                let up2 = self.hb_stage_4x_a.upsample2(main_in);
                let up4_a = self.hb_stage_4x_b.upsample2(up2[0]);
                let up4_b = self.hb_stage_4x_b.upsample2(up2[1]);
                let z0 = self.process_main_ssb_subframe(up4_a[0], shift_l, shift_r, inv_sr4);
                let z1 = self.process_main_ssb_subframe(up4_a[1], shift_l, shift_r, inv_sr4);
                let z2 = self.process_main_ssb_subframe(up4_b[0], shift_l, shift_r, inv_sr4);
                let z3 = self.process_main_ssb_subframe(up4_b[1], shift_l, shift_r, inv_sr4);
                let d2_0 = self.hb_stage_4x_b.downsample2(z0, z1);
                let d2_1 = self.hb_stage_4x_b.downsample2(z2, z3);
                self.hb_stage_4x_a.downsample2(d2_0, d2_1)
            }
        };

        if p.fb_shift_independent {
            wet[0] += self.fb_state[0] * fb_gain;
            wet[1] += self.fb_state[1] * fb_gain;
        }

        // Escribir salida actual en la línea de retardo y leer tap interpolado para el próximo ciclo
        self.delay_buf[self.delay_write] = [wet[0] as f32, wet[1] as f32];
        let delayed = self.read_delay_hermite(self.smooth_delay_samp);
        self.delay_write += 1;
        if self.delay_write >= self.delay_buf.len() {
            self.delay_write = 0;
        }

        // Cadena de acondicionamiento del lazo de realimentación:
        // 1) Cruce estéreo L↔R (`fb_cross`)
        let mut fb_sig = if p.fb_cross {
            [delayed[1], delayed[0]]
        } else {
            delayed
        };

        // 2) FB Cuts: High-Pass y Low-Pass Butterworth SVF
        if p.fb_hp_hz > 20.5 {
            fb_sig = self.fb_hp_svf.process_hp(fb_sig, p.fb_hp_hz as f64, sr);
        }
        if p.fb_lp_hz < 19_900.0 {
            fb_sig = self.fb_lp_svf.process_lp(fb_sig, p.fb_lp_hz as f64, sr);
        }

        // 3) FB Shift independiente dentro del lazo de realimentación
        let fb_df_l = self.smooth_fb_shift_hz;
        let fb_df_r = if p.stereo_twist {
            -self.smooth_fb_shift_hz
        } else {
            self.smooth_fb_shift_hz
        };
        let fb_dfs = [fb_df_l, fb_df_r];
        if fb_df_l.abs() > 1.0e-5 || self.fb_osc_phase[0].abs() > 1.0e-6 {
            let inv_sr = 1.0 / sr;
            for ch in 0..2 {
                let (fi, fq) = self.fb_hilbert.process_channel(ch, fb_sig[ch]);
                let ph = self.fb_osc_phase[ch];
                let (sin_p, cos_p) = ph.sin_cos();
                fb_sig[ch] = fi * cos_p - fq * sin_p;
                let mut next_ph = ph + TAU * fb_dfs[ch] * inv_sr;
                if next_ph >= TAU || next_ph <= -TAU {
                    next_ph %= TAU;
                }
                self.fb_osc_phase[ch] = next_ph;
            }
        }

        // 4) Bloqueador de DC + limitador suave `tanh` para auto-oscilación controlada
        for ch in 0..2 {
            let dc_out = fb_sig[ch] - self.fb_dc_prev_in[ch] + 0.9995 * self.fb_dc_prev_out[ch];
            self.fb_dc_prev_in[ch] = fb_sig[ch];
            self.fb_dc_prev_out[ch] = if dc_out.abs() < 1.0e-30 { 0.0 } else { dc_out };
            let soft = (dc_out / 1.15).tanh() * 1.15;
            self.fb_state[ch] = if soft.is_finite() { soft } else { 0.0 };
        }

        // Ganancia opcional post-etapa
        let gain = if p.wet_gain_db.abs() > 0.01 {
            10.0_f64.powf(p.wet_gain_db as f64 / 20.0)
        } else {
            1.0
        };
        let wl = (wet[0] * gain) as f32;
        let wr = (wet[1] * gain) as f32;
        (
            if wl.is_finite() { wl.clamp(-4.0, 4.0) } else { 0.0 },
            if wr.is_finite() { wr.clamp(-4.0, 4.0) } else { 0.0 },
        )
    }

    #[inline]
    fn apply_output_stage(&self, dry_l: f32, dry_r: f32, wet_l: f32, wet_r: f32) -> (f32, f32) {
        let wl_in = if wet_l.is_finite() { wet_l } else { 0.0 };
        let wr_in = if wet_r.is_finite() { wet_r } else { 0.0 };

        // Stereo Width Mid/Side (0.0 = Mono Mid, 1.0 = 100% Stereo, 2.0 = 200% Side)
        let mid = 0.5 * (wl_in + wr_in);
        let side = 0.5 * (wl_in - wr_in) * self.params.width;
        let wl = mid + side;
        let wr = mid - side;

        // Equal-Gain Dry/Wet Mix
        let m = self.params.mix;
        (dry_l * (1.0 - m) + wl * m, dry_r * (1.0 - m) + wr * m)
    }

    #[inline]
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        let in_l = if left.is_finite() { left } else { 0.0 };
        let in_r = if right.is_finite() { right } else { 0.0 };
        let (wet_l, wet_r) = self.process_wet_stereo(in_l, in_r);
        self.apply_output_stage(in_l, in_r, wet_l, wet_r)
    }
}

impl WetFxChainHost for FreqShifterPlusEffect {
    fn wet_chain(&self) -> &[DspPluginNode] {
        &self.wet_chain
    }

    fn wet_chain_mut(&mut self) -> &mut Vec<DspPluginNode> {
        &mut self.wet_chain
    }

    fn insert_wet_fx(&mut self, mut node: DspPluginNode) {
        node.instance.set_sample_rate(self.sample_rate);
        if let Some(pos) = self.wet_chain.iter().position(|n| n.slot_idx == node.slot_idx) {
            self.wet_chain.remove(pos);
        }
        let insert_pos = self
            .wet_chain
            .iter()
            .position(|n| n.slot_idx > node.slot_idx)
            .unwrap_or(self.wet_chain.len());
        self.wet_chain.insert(insert_pos, node);
    }

    fn remove_wet_fx(&mut self, wet_idx: usize) {
        if let Some(pos) = self.wet_chain.iter().position(|n| n.slot_idx == wet_idx) {
            self.wet_chain.remove(pos);
        }
        for n in &mut self.wet_chain {
            if n.slot_idx > wet_idx {
                n.slot_idx -= 1;
            }
        }
    }

    fn set_wet_fx_active(&mut self, wet_idx: usize, active: bool) {
        if let Some(node) = self.wet_chain.iter_mut().find(|n| n.slot_idx == wet_idx) {
            node.is_active = active;
        }
    }

    fn set_wet_fx_param(&mut self, wet_idx: usize, param_id: u32, value: f32) {
        if let Some(node) = self.wet_chain.iter_mut().find(|n| n.slot_idx == wet_idx) {
            node.set_param_normalized(param_id, value);
        }
    }

    fn sync_wet_fx_params(&mut self, wet_idx: usize, params: &[(u32, f32)]) {
        if let Some(node) = self.wet_chain.iter_mut().find(|n| n.slot_idx == wet_idx) {
            node.sync_params(params);
        }
    }
}

impl PluginInstance for FreqShifterPlusEffect {
    fn process(&mut self, buffer: &mut AudioBuffer) {
        if self.wet_chain.is_empty() {
            for frame in buffer.samples.chunks_exact_mut(2) {
                let (l, r) = self.process_stereo(frame[0], frame[1]);
                frame[0] = l;
                frame[1] = r;
            }
            return;
        }

        let n = buffer.samples.len();
        if self.dry_scratch.len() < n {
            self.dry_scratch.resize(n, 0.0);
        }
        self.dry_scratch[..n].copy_from_slice(buffer.samples);

        for frame in buffer.samples.chunks_exact_mut(2) {
            let in_l = if frame[0].is_finite() { frame[0] } else { 0.0 };
            let in_r = if frame[1].is_finite() { frame[1] } else { 0.0 };
            let (wl, wr) = self.process_wet_stereo(in_l, in_r);
            frame[0] = wl;
            frame[1] = wr;
        }

        for node in &mut self.wet_chain {
            if node.is_active {
                node.process(buffer, &[]);
            }
        }

        for (i, frame) in buffer.samples.chunks_exact_mut(2).enumerate() {
            let dry_l = self.dry_scratch[i * 2];
            let dry_r = self.dry_scratch[i * 2 + 1];
            let (out_l, out_r) = self.apply_output_stage(dry_l, dry_r, frame[0], frame[1]);
            frame[0] = out_l;
            frame[1] = out_r;
        }
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(8_000.0);
        self.rebuild_for_sample_rate();
        for node in &mut self.wet_chain {
            node.instance.set_sample_rate(self.sample_rate);
        }
    }

    fn latency_samples(&self) -> u32 {
        self.wet_chain.iter().map(|n| n.latency_samples()).sum()
    }

    fn set_transport(&mut self, transport: HostTransportInfo) {
        if transport.tempo.is_finite() && transport.tempo > 10.0 {
            self.tempo_bpm = transport.tempo;
        }
        for node in &mut self.wet_chain {
            node.set_transport(transport);
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
        "Hikaru OpenFreqShifter+"
    }

    fn param_count(&mut self) -> usize {
        FREQ_SHIFTER_PLUS_PARAM_COUNT
    }

    fn param_info(&mut self, index: usize) -> Option<PluginParamInfo> {
        let def = FreqShifterPlusParams::default();
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
            FSP_PARAM_SHIFT_NORM => Some(mk(0, "Frequency Shift Amount", "%", -100.0, 100.0, 0)),
            FSP_PARAM_SHIFT_UNIT => Some(mk(1, "Shift Unit", "", 0.0, 3.0, 3)),
            FSP_PARAM_STEREO_TWIST => Some(mk(2, "Stereo Twist", "", 0.0, 1.0, 1)),
            FSP_PARAM_POLE_ORDER => Some(mk(3, "Hilbert Poles", "", 0.0, 3.0, 3)),
            FSP_PARAM_FB_CROSS => Some(mk(4, "Feedback Stereo Cross", "", 0.0, 1.0, 1)),
            FSP_PARAM_QUALITY => Some(mk(5, "Quality Mode", "", 0.0, 2.0, 2)),
            FSP_PARAM_FB_HP_HZ => Some(mk(
                6,
                "FB Low Cut",
                "Hz",
                FREQ_SHIFTER_MIN_CUT_HZ,
                FREQ_SHIFTER_MAX_CUT_HZ,
                0,
            )),
            FSP_PARAM_FB_LP_HZ => Some(mk(
                7,
                "FB High Cut",
                "Hz",
                FREQ_SHIFTER_MIN_CUT_HZ,
                FREQ_SHIFTER_MAX_CUT_HZ,
                0,
            )),
            FSP_PARAM_DELAY_MS => Some(mk(8, "Feedback Delay", "ms", 0.0, 1000.0, 0)),
            FSP_PARAM_DELAY_SYNC => Some(mk(9, "Delay Beat Sync", "", 0.0, 1.0, 1)),
            FSP_PARAM_DELAY_SYNC_DIV => Some(mk(10, "Delay Sync Division", "", 0.0, 9.0, 9)),
            FSP_PARAM_FEEDBACK => Some(mk(11, "Feedback", "%", -98.0, 98.0, 0)),
            FSP_PARAM_FB_SHIFT_NORM => Some(mk(12, "FB Shift", "Hz", -500.0, 500.0, 0)),
            FSP_PARAM_FB_SHIFT_INDEP => Some(mk(13, "FB Shift Independent", "", 0.0, 1.0, 1)),
            FSP_PARAM_WIDTH => Some(mk(14, "Stereo Width", "%", 0.0, 200.0, 0)),
            FSP_PARAM_MIX => Some(mk(15, "Mix", "%", 0.0, 100.0, 0)),
            FSP_PARAM_WET_GAIN_DB => Some(mk(16, "Wet Gain", "dB", -12.0, 12.0, 0)),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Devuelve la amplitud de pico estimada en `target_hz` corrigiendo la ganancia coherente
    /// de la ventana de Hann (`0.5`) y la proyección unilateral (`0.5`), de modo que un tono
    /// puro de amplitud `0.5` produce `~0.50`.
    fn goertzel_power(samples: &[f32], target_hz: f64, sr: f64) -> f64 {
        let mut re = 0.0_f64;
        let mut im = 0.0_f64;
        let n = samples.len() as f64;
        for (i, &s) in samples.iter().enumerate() {
            // Ventana de Hann para evitar fuga espectral entre bins no enteros
            let w = 0.5 * (1.0 - (TAU * i as f64 / (n - 1.0)).cos());
            let ang = TAU * target_hz * (i as f64) / sr;
            re += (s as f64) * w * ang.cos();
            im -= (s as f64) * w * ang.sin();
        }
        4.0 * (re * re + im * im).sqrt() / n
    }

    #[test]
    fn hilbert_phase_error_shrinks_monotonically_from_6p_to_24p() {
        let sr = 48_000.0;
        let mut prev_err = f64::MAX;
        for poles in FreqShiftPoles::ALL {
            let (ca, cb, n) = design_hilbert_allpass_branches(sr, poles);
            let mut max_err_deg = 0.0_f64;
            for step in 0..=60 {
                let f = 25.0 * (18_000.0_f64 / 25.0).powf(step as f64 / 60.0);
                let wd = TAU * f / sr;
                let (sin_w, cos_w) = wd.sin_cos();
                let eval_branch = |coeffs: &[f64; MAX_POLES_PER_BRANCH]| -> f64 {
                    let mut total_phase = 0.0_f64;
                    for &a in coeffs.iter().take(n) {
                        // H(e^jw) = (e^-jw - a) / (1 - a*e^-jw)
                        let num_re = cos_w - a;
                        let num_im = -sin_w;
                        let den_re = 1.0 - a * cos_w;
                        let den_im = a * sin_w;
                        let re = num_re * den_re + num_im * den_im;
                        let im = num_im * den_re - num_re * den_im;
                        total_phase += im.atan2(re);
                    }
                    total_phase
                };
                let mut diff = eval_branch(&ca) - eval_branch(&cb);
                while diff <= -PI {
                    diff += TAU;
                }
                while diff > PI {
                    diff -= TAU;
                }
                let err_deg = (diff - FRAC_PI_2).abs().to_degrees();
                max_err_deg = max_err_deg.max(err_deg);
            }
            assert!(
                max_err_deg < prev_err,
                "{poles:?} phase error {max_err_deg}° did not improve over {prev_err}°"
            );
            prev_err = max_err_deg;
        }
        // 24P debe mantener la cuadratura dentro de una fracción minúscula de grado en 25 Hz..18 kHz.
        assert!(prev_err < 0.05, "24P max phase error too high: {prev_err}°");
    }

    #[test]
    fn freq_shifter_plus_shifts_single_sideband_up_and_down_cleanly() {
        let sr = 48_000.0;
        let mut fx = FreqShifterPlusEffect::new(sr as f32);
        let mut p = FreqShifterPlusParams {
            pole_order: FreqShiftPoles::P24,
            quality: FreqShiftQuality::Excellent,
            mix: 1.0,
            ..FreqShifterPlusParams::default()
        };
        p.set_shift_from_display_value(250.0); // +250 Hz -> 1000 Hz pasa a 1250 Hz
        fx.set_params(p);

        let mut out_up = Vec::with_capacity(4096);
        for i in 0..5000 {
            let s = (TAU * 1000.0 * (i as f64) / sr).sin() as f32 * 0.5;
            let (l, _) = fx.process_stereo(s, s);
            if i >= 904 {
                out_up.push(l);
            }
        }
        let wanted_up = goertzel_power(&out_up, 1250.0, sr);
        let unwanted_down = goertzel_power(&out_up, 750.0, sr);
        let carrier_leak = goertzel_power(&out_up, 1000.0, sr);
        assert!(wanted_up > 0.18, "expected strong +250 Hz shifted tone, got {wanted_up}");
        assert!(
            wanted_up / unwanted_down.max(1.0e-9) > 200.0,
            "expected >46 dB image suppression, wanted={wanted_up}, image={unwanted_down}"
        );
        assert!(carrier_leak < 0.002, "unexpected carrier leak: {carrier_leak}");

        // Ahora desplazamiento negativo (-250 Hz -> 1000 Hz pasa a 750 Hz)
        p.set_shift_from_display_value(-250.0);
        fx.set_params(p);
        let mut out_dn = Vec::with_capacity(4096);
        for i in 0..5000 {
            let s = (TAU * 1000.0 * (i as f64) / sr).sin() as f32 * 0.5;
            let (l, _) = fx.process_stereo(s, s);
            if i >= 904 {
                out_dn.push(l);
            }
        }
        let wanted_dn = goertzel_power(&out_dn, 750.0, sr);
        let unwanted_up = goertzel_power(&out_dn, 1250.0, sr);
        assert!(wanted_dn > 0.18, "expected strong -250 Hz shifted tone, got {wanted_dn}");
        assert!(wanted_dn / unwanted_up.max(1.0e-9) > 200.0);
    }

    #[test]
    fn stereo_twist_shifts_left_and_right_in_opposite_directions() {
        let sr = 48_000.0;
        let mut fx = FreqShifterPlusEffect::new(sr as f32);
        let mut p = FreqShifterPlusParams {
            stereo_twist: true,
            quality: FreqShiftQuality::High,
            ..FreqShifterPlusParams::default()
        };
        p.set_shift_from_display_value(200.0);
        fx.set_params(p);

        let mut left_buf = Vec::with_capacity(4096);
        let mut right_buf = Vec::with_capacity(4096);
        for i in 0..5000 {
            let s = (TAU * 1000.0 * (i as f64) / sr).sin() as f32 * 0.5;
            let (l, r) = fx.process_stereo(s, s);
            if i >= 904 {
                left_buf.push(l);
                right_buf.push(r);
            }
        }
        assert!(goertzel_power(&left_buf, 1200.0, sr) > 0.18);
        assert!(goertzel_power(&right_buf, 800.0, sr) > 0.18);
    }

    #[test]
    fn high_feedback_with_cuts_and_cross_stays_bounded_and_finite() {
        let mut fx = FreqShifterPlusEffect::new(48_000.0);
        let mut p = FreqShifterPlusParams {
            delay_ms: 18.0,
            feedback: 0.96,
            fb_cross: true,
            fb_hp_hz: 120.0,
            fb_lp_hz: 6_500.0,
            ..FreqShifterPlusParams::default()
        };
        p.set_shift_from_display_value(35.0);
        fx.set_params(p);

        let mut tail_energy_r = 0.0_f32;
        for i in 0..8_192 {
            let imp_l = if i < 64 { (i as f32 * 0.2).sin() * 0.9 } else { 0.0 };
            let (l, r) = fx.process_stereo(imp_l, 0.0);
            assert!(l.is_finite() && r.is_finite());
            assert!(l.abs() <= 4.0 && r.abs() <= 4.0);
            if i > 1024 {
                tail_energy_r += r * r;
            }
        }
        // Con entrada solo en L y `fb_cross = true`, la cola debe aparecer también en R.
        assert!(tail_energy_r > 0.05, "expected stereo cross-feedback tail in R, got {tail_energy_r}");
    }

    #[test]
    fn param_normalized_roundtrip() {
        let mut fx = FreqShifterPlusEffect::new(48_000.0);
        for id in 0..(FREQ_SHIFTER_PLUS_PARAM_COUNT as u32) {
            let v = fx.get_param_normalized(id).expect("valid param id");
            assert!(fx.set_param_normalized(id, v));
            let v2 = fx.get_param_normalized(id).unwrap();
            assert!((v - v2).abs() < 1.0e-3, "param {id} drifted: {v} vs {v2}");
        }
    }

    #[test]
    fn wet_fx_chain_processes_only_wet_signal() {
        use crate::effects::ToolEffect;
        use crate::plugin_node::PluginCategory;

        let mut fx = FreqShifterPlusEffect::new(48_000.0);
        fx.set_params(FreqShifterPlusParams {
            quality: FreqShiftQuality::Eco,
            mix: 1.0,
            ..FreqShifterPlusParams::default()
        });

        // Silenciar la rama Wet insertando Tool con Gain = -36 dB (norm 0.0)
        let mut tool = ToolEffect::new(48_000.0);
        tool.set_param_normalized(0, 0.0);
        fx.insert_wet_fx(
            DspPluginNode::new("Tool".to_string(), PluginCategory::Vst3Fx, Box::new(tool))
                .with_slot_idx(0),
        );

        let mut wet_samples: Vec<f32> = (0..256)
            .flat_map(|i| {
                let v = (i as f32 * 0.15).sin() * 0.8;
                [v, v]
            })
            .collect();
        let dry_ref = wet_samples.clone();
        {
            let mut buf = AudioBuffer {
                samples: &mut wet_samples,
            };
            fx.process(&mut buf);
        }
        let wet_peak = wet_samples.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        assert!(wet_peak < 0.03, "expected attenuated wet chain, got {wet_peak}");

        // Con Mix = 0% (100% Dry), la señal original debe pasar intacta
        let mut p = fx.params();
        p.mix = 0.0;
        fx.set_params(p);
        let mut dry_out = dry_ref.clone();
        {
            let mut buf = AudioBuffer {
                samples: &mut dry_out,
            };
            fx.process(&mut buf);
        }
        for (a, b) in dry_out.iter().zip(dry_ref.iter()) {
            assert!((a - b).abs() < 1.0e-5);
        }
    }
}
