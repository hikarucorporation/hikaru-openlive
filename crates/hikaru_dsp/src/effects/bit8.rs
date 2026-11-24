// crates/hikaru_dsp/src/effects/bit8.rs

//! **Hikaru Open8-Bit**: degradador digital de frecuencia de reloj, conformador y
//! cuantizador con anti-aliasing de primera derivada (**ADAA1**).
//!
//! Implementación limpia en Rust con paridad de comportamiento:
//! - **Clock**: reloj de muestreo (`200 Hz .. 100.0 kHz`, default `12.9 kHz`) con
//!   interpolación fraccional sub-muestra en los flancos, **Jitter** (`0 .. 100%`,
//!   default `25.5%`) y **Chance** (`0 .. 100%`, probabilidad de captura por pulso).
//! - **Shape**: rectificación (`Off`, `Half`, `Full`) y clipping/plegado (`Off`,
//!   `Soft Clip`, `Hard Clip`, `Fold`, `Wrap`) con **Drive** (`0 .. +36 dB`) sobre
//!   umbral ±0.5, todo con soporte ADAA1 cuando **Anti-alias** está activo.
//! - **Quantize**: nivel de cuantización (`-96 .. 0 dB`, default `-39.1 dB`),
//!   modos `Off-zero` (mid-riser), `On-zero` (mid-tread), `Diffusion` (difusión de
//!   error binomial de 3er orden con control **Diffusion Brightness**) y `Dither`
//!   (ruido triangular TPDF mono o **Stereo Dither**), además de **Squared Domain**
//!   (`x²`, compansión cuadrática alrededor del cuantizador).
//! - **Gate**: puerta instantánea post-cuantización controlada por la amplitud de
//!   entrada (`-96 .. 0 dB`, default `-58.2 dB`), suavizada mediante ADAA1 cuando
//!   **Anti-alias** está activo (silencia ruido de dithering/difusión y DC `Off-zero`
//!   en silencio).
//! - **Output**: cadena **Wet FX** (HP, LP, Gain), **Width** estéreo Mid/Side y
//!   **Mix** Equal-Gain Dry/Wet.
//! - **Telemetría RT lock-free**: LEDs de actividad (`Gate`, `Shape`, `Quantize`) y
//!   trazas desplazables de 3 segundos (`gate_disp`, `clip_disp`, `quant_disp`).

use std::f32::consts::PI;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use hikaru_core::AudioBuffer;
use hikaru_plugin_host::{PluginInstance, PluginParamInfo};

pub const BIT8_MIN_CLOCK_HZ: f32 = 200.0;
pub const BIT8_MAX_CLOCK_HZ: f32 = 100_000.0;
pub const BIT8_DEFAULT_CLOCK_HZ: f32 = 12_878.27;
pub const BIT8_MIN_DB: f32 = -96.0;
pub const BIT8_MAX_DB: f32 = 0.0;
pub const BIT8_MAX_DRIVE_DB: f32 = 36.0;

pub const BIT8_PARAM_CLOCK_HZ: u32 = 0;
pub const BIT8_PARAM_JITTER: u32 = 1;
pub const BIT8_PARAM_CHANCE: u32 = 2;
pub const BIT8_PARAM_GATE_DB: u32 = 3;
pub const BIT8_PARAM_RECTIFY: u32 = 4;
pub const BIT8_PARAM_SHAPE_MODE: u32 = 5;
pub const BIT8_PARAM_SHAPE_DRIVE_DB: u32 = 6;
pub const BIT8_PARAM_QUANTIZE_MODE: u32 = 7;
pub const BIT8_PARAM_QUANTIZE_DB: u32 = 8;
pub const BIT8_PARAM_DIFFUSION: u32 = 9;
pub const BIT8_PARAM_STEREO_DITHER: u32 = 10;
pub const BIT8_PARAM_SQUARED_DOMAIN: u32 = 11;
pub const BIT8_PARAM_ANTI_ALIAS: u32 = 12;
pub const BIT8_PARAM_WIDTH: u32 = 13;
pub const BIT8_PARAM_MIX: u32 = 14;
pub const BIT8_PARAM_WET_HP_HZ: u32 = 15;
pub const BIT8_PARAM_WET_LP_HZ: u32 = 16;
pub const BIT8_PARAM_WET_GAIN_DB: u32 = 17;
pub const BIT8_PARAM_COUNT: usize = 18;

/// Número de muestras en la historia desplazable del display (~3.0 s a 60 Hz).
pub const BIT8_DISPLAY_HISTORY: usize = 180;

// =============================================================================
// Telemetría lock-free (audio -> GUI)
// =============================================================================

#[derive(Debug, Clone, Copy)]
pub struct Bit8TelemetrySnapshot {
    pub gate_led: f32,
    pub clip_led: f32,
    pub quant_led: f32,
    pub write_seq: u64,
    pub gate_trace: [f32; BIT8_DISPLAY_HISTORY],
    pub clip_trace: [f32; BIT8_DISPLAY_HISTORY],
    pub quant_trace: [f32; BIT8_DISPLAY_HISTORY],
}

impl Default for Bit8TelemetrySnapshot {
    fn default() -> Self {
        Self {
            gate_led: 0.0,
            clip_led: 0.0,
            quant_led: 0.0,
            write_seq: 0,
            gate_trace: [-1.0; BIT8_DISPLAY_HISTORY],
            clip_trace: [-1.0; BIT8_DISPLAY_HISTORY],
            quant_trace: [-1.0; BIT8_DISPLAY_HISTORY],
        }
    }
}

pub struct Bit8Telemetry {
    gate_led: AtomicU32,
    clip_led: AtomicU32,
    quant_led: AtomicU32,
    write_seq: AtomicU64,
    gate_ring: [AtomicU32; BIT8_DISPLAY_HISTORY],
    clip_ring: [AtomicU32; BIT8_DISPLAY_HISTORY],
    quant_ring: [AtomicU32; BIT8_DISPLAY_HISTORY],
}

impl Bit8Telemetry {
    pub fn new() -> Self {
        let neg_one = (-1.0_f32).to_bits();
        Self {
            gate_led: AtomicU32::new(0.0_f32.to_bits()),
            clip_led: AtomicU32::new(0.0_f32.to_bits()),
            quant_led: AtomicU32::new(0.0_f32.to_bits()),
            write_seq: AtomicU64::new(0),
            gate_ring: std::array::from_fn(|_| AtomicU32::new(neg_one)),
            clip_ring: std::array::from_fn(|_| AtomicU32::new(neg_one)),
            quant_ring: std::array::from_fn(|_| AtomicU32::new(neg_one)),
        }
    }

    #[inline]
    pub fn store_leds(&self, gate: f32, clip: f32, quant: f32) {
        self.gate_led.store(gate.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        self.clip_led.store(clip.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        self.quant_led.store(quant.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    #[inline]
    pub fn push_display_point(&self, gate_disp: f32, clip_disp: f32, quant_disp: f32) {
        let seq = self.write_seq.load(Ordering::Relaxed);
        let idx = (seq as usize) % BIT8_DISPLAY_HISTORY;
        self.gate_ring[idx].store(gate_disp.clamp(-1.0, 1.0).to_bits(), Ordering::Relaxed);
        self.clip_ring[idx].store(clip_disp.clamp(-1.0, 1.0).to_bits(), Ordering::Relaxed);
        self.quant_ring[idx].store(quant_disp.clamp(-1.0, 1.0).to_bits(), Ordering::Relaxed);
        self.write_seq.store(seq.wrapping_add(1), Ordering::Release);
    }

    pub fn snapshot(&self) -> Bit8TelemetrySnapshot {
        let seq = self.write_seq.load(Ordering::Acquire);
        let mut gate_trace = [-1.0_f32; BIT8_DISPLAY_HISTORY];
        let mut clip_trace = [-1.0_f32; BIT8_DISPLAY_HISTORY];
        let mut quant_trace = [-1.0_f32; BIT8_DISPLAY_HISTORY];
        let start = (seq as usize) % BIT8_DISPLAY_HISTORY;
        for i in 0..BIT8_DISPLAY_HISTORY {
            let ri = (start + i) % BIT8_DISPLAY_HISTORY;
            gate_trace[i] = f32::from_bits(self.gate_ring[ri].load(Ordering::Relaxed));
            clip_trace[i] = f32::from_bits(self.clip_ring[ri].load(Ordering::Relaxed));
            quant_trace[i] = f32::from_bits(self.quant_ring[ri].load(Ordering::Relaxed));
        }
        Bit8TelemetrySnapshot {
            gate_led: f32::from_bits(self.gate_led.load(Ordering::Relaxed)),
            clip_led: f32::from_bits(self.clip_led.load(Ordering::Relaxed)),
            quant_led: f32::from_bits(self.quant_led.load(Ordering::Relaxed)),
            write_seq: seq,
            gate_trace,
            clip_trace,
            quant_trace,
        }
    }
}

impl Default for Bit8Telemetry {
    fn default() -> Self {
        Self::new()
    }
}

/// Registro global `(engine_track_idx, slot_idx) -> Arc<Bit8Telemetry>`.
///
/// Solo lo toca el hilo de control/GUI al cargar, mover o quitar slots; el hilo
/// de audio escribe únicamente sobre su propio `Arc<Bit8Telemetry>` sin locks.
pub mod telemetry {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};

    use super::Bit8Telemetry;

    type Key = (usize, usize);

    fn registry() -> &'static Mutex<HashMap<Key, Arc<Bit8Telemetry>>> {
        static REG: OnceLock<Mutex<HashMap<Key, Arc<Bit8Telemetry>>>> = OnceLock::new();
        REG.get_or_init(|| Mutex::new(HashMap::new()))
    }

    pub fn register(key: Key, handle: Arc<Bit8Telemetry>) {
        if let Ok(mut map) = registry().lock() {
            map.insert(key, handle);
        }
    }

    pub fn unregister(key: Key) {
        if let Ok(mut map) = registry().lock() {
            map.remove(&key);
        }
    }

    pub fn lookup(key: Key) -> Option<Arc<Bit8Telemetry>> {
        registry().lock().ok().and_then(|map| map.get(&key).cloned())
    }
}

// =============================================================================
// Modos y parámetros
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bit8Rectify {
    /// Sin rectificación (`x`).
    Off = 0,
    /// Rectificación de media onda (`max(0, x)`).
    Half = 1,
    /// Rectificación de onda completa (`|x|`).
    Full = 2,
}

impl Bit8Rectify {
    pub const ALL: [Self; 3] = [Self::Off, Self::Half, Self::Full];

    pub fn from_norm(v: f32) -> Self {
        match (v.clamp(0.0, 1.0) * 2.0).round() as usize {
            0 => Self::Off,
            1 => Self::Half,
            _ => Self::Full,
        }
    }

    pub fn to_norm(self) -> f32 {
        (self as usize) as f32 / 2.0
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Half => "Half",
            Self::Full => "Full",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Off => "Leaves signal unchanged",
            Self::Half => "Negative values become zero",
            Self::Full => "Negative values flip positive",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bit8ShapeMode {
    Off = 0,
    SoftClip = 1,
    HardClip = 2,
    Fold = 3,
    Wrap = 4,
}

impl Bit8ShapeMode {
    pub const ALL: [Self; 5] = [
        Self::Off,
        Self::SoftClip,
        Self::HardClip,
        Self::Fold,
        Self::Wrap,
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
            Self::SoftClip => "Soft Clip",
            Self::HardClip => "Hard Clip",
            Self::Fold => "Fold",
            Self::Wrap => "Wrap",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Off => "No clipping",
            Self::SoftClip => "Symmetric distortion",
            Self::HardClip => "Flatlines beyond ±0.5",
            Self::Fold => "Reflects when beyond ±0.5",
            Self::Wrap => "Flips polarity beyond ±0.5",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bit8QuantizeMode {
    /// Mid-riser: pasos desplazados medio escalón respecto al cero.
    OffZero = 0,
    /// Mid-tread: escalón centrado en cero.
    OnZero = 1,
    /// Difusión de error binomial de 3er orden (controlada por `diffusion_brightness`).
    Diffusion = 2,
    /// Dithering TPDF (ruido triangular mono o estéreo).
    Dither = 3,
}

impl Bit8QuantizeMode {
    pub const ALL: [Self; 4] = [Self::OffZero, Self::OnZero, Self::Diffusion, Self::Dither];

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
            Self::OnZero => "On-zero",
            Self::Diffusion => "Diffusion",
            Self::Dither => "Dither",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::OffZero => "Offsets grid",
            Self::OnZero => "Grids around zero",
            Self::Diffusion => "Spreads errors out",
            Self::Dither => "Adds noise for better fit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bit8Params {
    /// Frecuencia de reloj de muestreo (`200.0 .. 100_000.0 Hz`).
    pub clock_hz: f32,
    /// Fluctuación aleatoria del período de reloj (`0.0 .. 1.0`).
    pub jitter: f32,
    /// Probabilidad de captura por pulso de reloj (`0.0 .. 1.0`).
    pub chance: f32,
    /// Umbral del Gate post-cuantización (`-96.0 .. 0.0 dB`).
    pub gate_db: f32,
    /// Modo de rectificación (`Off`, `Half`, `Full`).
    pub rectify: Bit8Rectify,
    /// Modo de clipping/waveshaping (`Off`, `SoftClip`, `HardClip`, `Fold`, `Wrap`).
    pub shape_mode: Bit8ShapeMode,
    /// Ganancia pre-clipping (`0.0 .. 36.0 dB`).
    pub shape_drive_db: f32,
    /// Algoritmo de cuantización (`OffZero`, `OnZero`, `Diffusion`, `Dither`).
    pub quantize_mode: Bit8QuantizeMode,
    /// Piso de ruido / paso de cuantización en dB (`-96.0 .. 0.0 dB`).
    pub quantize_db: f32,
    /// Brillo / dispersión de error para modo `Diffusion` (`0.0 .. 1.0`).
    pub diffusion_brightness: f32,
    /// Ruido independiente L/R para modo `Dither`.
    pub stereo_dither: bool,
    /// Cuantización en dominio cuadrático (`x²`).
    pub squared_domain: bool,
    /// Anti-aliasing ADAA1 sobre Rectify, Clip, Quantize y Gate.
    pub anti_alias: bool,
    /// Ancho estéreo Mid/Side (`0.0 .. 1.0`).
    pub width: f32,
    /// Mezcla Equal-Gain Dry/Wet (`0.0 .. 1.0`).
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
        Self {
            clock_hz: BIT8_DEFAULT_CLOCK_HZ, // ~12.9 kHz
            jitter: 0.255,                   // 25.5%
            chance: 1.0,                     // 100%
            gate_db: -58.20213,              // -58.2 dB
            rectify: Bit8Rectify::Off,
            shape_mode: Bit8ShapeMode::Off,
            shape_drive_db: 0.0,
            quantize_mode: Bit8QuantizeMode::OffZero,
            quantize_db: -39.10627,          // -39.1 dB
            diffusion_brightness: 0.5,       // 50.0%
            stereo_dither: false,
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

    /// Devuelve la nota musical aproximada (convención C3 = 261.63 Hz = MIDI 60).
    pub fn clock_hz_note_label(hz: f32) -> String {
        const NAMES: [&str; 12] = [
            "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
        ];
        let midi = (69.0 + 12.0 * (hz.max(1.0) / 440.0).log2()).round() as i32;
        let pc = midi.rem_euclid(12) as usize;
        let oct = midi.div_euclid(12) - 2; // C3 = MIDI 60
        format!("{} {}", NAMES[pc], oct)
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
            gate_db: self.gate_db.clamp(BIT8_MIN_DB, BIT8_MAX_DB),
            rectify: self.rectify,
            shape_mode: self.shape_mode,
            shape_drive_db: self.shape_drive_db.clamp(0.0, BIT8_MAX_DRIVE_DB),
            quantize_mode: self.quantize_mode,
            quantize_db: self.quantize_db.clamp(BIT8_MIN_DB, BIT8_MAX_DB),
            diffusion_brightness: self.diffusion_brightness.clamp(0.0, 1.0),
            stereo_dither: self.stereo_dither,
            squared_domain: self.squared_domain,
            anti_alias: self.anti_alias,
            width: self.width.clamp(0.0, 1.0),
            mix: self.mix.clamp(0.0, 1.0),
            wet_hp_hz: self.wet_hp_hz.clamp(20.0, 2000.0),
            wet_lp_hz: self.wet_lp_hz.clamp(1000.0, 20_000.0),
            wet_gain_db: self.wet_gain_db.clamp(-12.0, 12.0),
        }
    }

    /// Tamaño de paso lineal correspondiente a `quantize_db`.
    #[inline]
    pub fn quantize_step(&self) -> f32 {
        db_to_lin(self.quantize_db).clamp(1.584_893_2e-5, 1.0)
    }

    /// Evalúa la función de transferencia instantánea (sin memoria ni ruido aleatorio).
    pub fn eval_transfer(&self, input: f32) -> f32 {
        let mut x = apply_rectify_direct(input as f64, self.rectify);
        if self.shape_mode != Bit8ShapeMode::Off {
            let g = db_to_lin(self.shape_drive_db) as f64 * 2.0;
            x = apply_clip_direct(x * g, self.shape_mode) * 0.5;
        }
        if self.squared_domain {
            x = x.abs().sqrt().copysign(x);
        }
        let q = self.quantize_step() as f64;
        let mut out = match self.quantize_mode {
            Bit8QuantizeMode::OffZero => ((x / q + 0.5).round() - 0.5) * q,
            Bit8QuantizeMode::OnZero => (x / q).round() * q,
            Bit8QuantizeMode::Diffusion | Bit8QuantizeMode::Dither => (x / q).floor() * q,
        };
        if self.squared_domain {
            out = (out * out).copysign(out);
        }
        let thresh = db_to_lin(self.gate_db) as f64;
        if (input as f64).abs() <= thresh {
            out = 0.0;
        }
        out as f32
    }

    pub fn get_normalized(&self, id: u32) -> Option<f32> {
        match id {
            BIT8_PARAM_CLOCK_HZ => Some(Self::clock_hz_to_norm(self.clock_hz)),
            BIT8_PARAM_JITTER => Some(self.jitter),
            BIT8_PARAM_CHANCE => Some(self.chance),
            BIT8_PARAM_GATE_DB => Some(Self::db_val_to_norm(self.gate_db)),
            BIT8_PARAM_RECTIFY => Some(self.rectify.to_norm()),
            BIT8_PARAM_SHAPE_MODE => Some(self.shape_mode.to_norm()),
            BIT8_PARAM_SHAPE_DRIVE_DB => Some((self.shape_drive_db / BIT8_MAX_DRIVE_DB).clamp(0.0, 1.0)),
            BIT8_PARAM_QUANTIZE_MODE => Some(self.quantize_mode.to_norm()),
            BIT8_PARAM_QUANTIZE_DB => Some(Self::db_val_to_norm(self.quantize_db)),
            BIT8_PARAM_DIFFUSION => Some(self.diffusion_brightness),
            BIT8_PARAM_STEREO_DITHER => Some(if self.stereo_dither { 1.0 } else { 0.0 }),
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

    pub fn set_normalized(&mut self, id: u32, value: f32) -> bool {
        let v = value.clamp(0.0, 1.0);
        match id {
            BIT8_PARAM_CLOCK_HZ => self.clock_hz = Self::clock_norm_to_hz(v),
            BIT8_PARAM_JITTER => self.jitter = v,
            BIT8_PARAM_CHANCE => self.chance = v,
            BIT8_PARAM_GATE_DB => self.gate_db = Self::db_norm_to_val(v),
            BIT8_PARAM_RECTIFY => self.rectify = Bit8Rectify::from_norm(v),
            BIT8_PARAM_SHAPE_MODE => self.shape_mode = Bit8ShapeMode::from_norm(v),
            BIT8_PARAM_SHAPE_DRIVE_DB => self.shape_drive_db = v * BIT8_MAX_DRIVE_DB,
            BIT8_PARAM_QUANTIZE_MODE => self.quantize_mode = Bit8QuantizeMode::from_norm(v),
            BIT8_PARAM_QUANTIZE_DB => self.quantize_db = Self::db_norm_to_val(v),
            BIT8_PARAM_DIFFUSION => self.diffusion_brightness = v,
            BIT8_PARAM_STEREO_DITHER => self.stereo_dither = v >= 0.5,
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

// =============================================================================
// Primitivas matemáticas y ADAA1 (Anti-Derivative Anti-Aliasing de 1er orden)
// =============================================================================

const ADAA_EPS: f64 = 1.0e-4;

#[inline]
fn db_to_lin(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

#[inline]
fn apply_rectify_direct(x: f64, rectify: Bit8Rectify) -> f64 {
    match rectify {
        Bit8Rectify::Off => x,
        Bit8Rectify::Half => x.max(0.0),
        Bit8Rectify::Full => x.abs(),
    }
}

#[inline]
fn rectify_antideriv(x: f64, rectify: Bit8Rectify) -> f64 {
    match rectify {
        Bit8Rectify::Off => 0.5 * x * x,
        Bit8Rectify::Half => 0.5 * x.max(0.0) * x,
        Bit8Rectify::Full => 0.5 * x.abs() * x,
    }
}

#[inline]
fn apply_clip_direct(x: f64, mode: Bit8ShapeMode) -> f64 {
    match mode {
        Bit8ShapeMode::Off => x,
        Bit8ShapeMode::HardClip => x.clamp(-1.0, 1.0),
        Bit8ShapeMode::SoftClip => {
            // Derivada exacta de F(x) = x^2*(1+|x|) / (2 + 1.5|x| + x^2)
            let xa = x.abs();
            let d = 2.0 * x * x + 3.0 * xa + 4.0;
            let num = 2.0 * xa * (2.0 * xa * x * x + 6.0 * x * x + 15.0 * xa + 8.0);
            (num / (d * d)).copysign(x)
        }
        Bit8ShapeMode::Fold => {
            // Wavefolder bipolar continuo con reflexión en ±1
            let xw = x - 2.0 * (0.5 * x).floor();
            (xw - 2.0 * (xw - 1.0).max(0.0)).copysign(x)
        }
        Bit8ShapeMode::Wrap => {
            // Envolvente bipolar (overflow de complemento a 2 en [-1, 1])
            let t = 0.5 * x - 0.5;
            (t - t.floor()) * 2.0 - 1.0
        }
    }
}

#[inline]
fn clip_antideriv(x: f64, mode: Bit8ShapeMode) -> f64 {
    match mode {
        Bit8ShapeMode::Off => 0.5 * x * x,
        Bit8ShapeMode::HardClip => {
            let ax = x.abs();
            let y = (1.0 - ax).max(0.0);
            ax + 0.5 * y * y
        }
        Bit8ShapeMode::SoftClip => {
            let xa = x.abs();
            (x * x * (1.0 + xa)) / (2.0 + 1.5 * xa + x * x)
        }
        Bit8ShapeMode::Fold => {
            let fl = (0.5 * x).floor();
            let xw = x - 2.0 * fl;
            let ov = (xw - 1.0).max(0.0);
            0.5 * ((xw * xw - 2.0 * ov * ov) + 2.0 * fl).abs()
        }
        Bit8ShapeMode::Wrap => {
            let xw = x - 2.0 * (0.5 * x).floor();
            0.5 * xw * xw - 2.0 * (xw - 1.0).max(0.0)
        }
    }
}

#[inline]
fn round_direct(x: f64) -> f64 {
    (x + 0.5).floor()
}

#[inline]
fn round_antideriv(x: f64) -> f64 {
    let y = (x + 0.5).floor();
    y * x - 0.5 * y * y
}

#[inline]
fn threshold_gate_direct(x: f64, threshold: f64) -> f64 {
    if x.abs() >= threshold { 1.0 } else { 0.0 }
}

#[inline]
fn threshold_gate_antideriv(x: f64, threshold: f64) -> f64 {
    (x.abs() - threshold).max(0.0).copysign(x)
}

#[derive(Debug, Clone, Copy)]
struct AdaaStereo {
    last_in: [f64; 2],
    last_f: [f64; 2],
    primed: bool,
}

impl AdaaStereo {
    fn new() -> Self {
        Self {
            last_in: [0.0; 2],
            last_f: [0.0; 2],
            primed: false,
        }
    }

    fn reset(&mut self) {
        self.last_in = [0.0; 2];
        self.last_f = [0.0; 2];
        self.primed = false;
    }

    #[inline]
    fn process<F, D>(&mut self, x: [f64; 2], antideriv: F, direct: D) -> [f64; 2]
    where
        F: Fn(f64) -> f64,
        D: Fn(f64) -> f64,
    {
        if !self.primed {
            self.primed = true;
            self.last_in = x;
            self.last_f = [antideriv(x[0]), antideriv(x[1])];
            return [direct(x[0]), direct(x[1])];
        }
        let mut out = [0.0_f64; 2];
        for ch in 0..2 {
            let xn = x[ch];
            let dx = xn - self.last_in[ch];
            let fn_val = antideriv(xn);
            out[ch] = if dx.abs() > ADAA_EPS {
                (fn_val - self.last_f[ch]) / dx
            } else {
                direct(0.5 * (xn + self.last_in[ch]))
            };
            self.last_in[ch] = xn;
            self.last_f[ch] = fn_val;
        }
        out
    }
}

// =============================================================================
// PRNG Xorshift32 (idéntico rango bipolar [-1, 1] y unipolar [0, 1])
// =============================================================================

#[derive(Debug, Clone, Copy)]
struct XorShift32 {
    state: u32,
}

impl XorShift32 {
    fn new(seed: u32) -> Self {
        Self { state: seed.max(1) }
    }

    #[inline]
    fn next_i32(&mut self) -> i32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        if x == 0 {
            x = 0x9E37_79B9;
        }
        self.state = x;
        x as i32
    }

    #[inline]
    fn next_bipolar(&mut self) -> f32 {
        let v = (self.next_i32() as f64) * (1.0 / 2_147_483_648.0);
        (v as f32).clamp(-1.0, 1.0)
    }

    #[inline]
    fn next_unipolar(&mut self) -> f32 {
        let u = (self.next_i32() & 0x7FFF_FFFF) as f64;
        ((u * (1.0 / 2_147_483_648.0)) as f32).clamp(0.0, 1.0)
    }
}

// =============================================================================
// Procesador principal Bit8Effect
// =============================================================================

pub struct Bit8Effect {
    sample_rate: f32,
    params: Bit8Params,
    telemetry: Arc<Bit8Telemetry>,

    // Estado del reloj y sample-and-hold sub-muestra
    phase: f32,
    hold: [f32; 2],
    last_hold: [f32; 2],

    // ADAA1 independientes por etapa no lineal
    adaa_rectify: AdaaStereo,
    adaa_clip: AdaaStereo,
    adaa_quant: AdaaStereo,
    adaa_gate: AdaaStereo,

    // Retardos de alineación para medición de error bajo ADAA1 (0.5 / 1.0 muestras)
    align_delay: [[f64; 2]; 2],

    // Historia de error para difusión binomial de 3er orden
    err1: [f64; 2],
    err2: [f64; 2],
    err3: [f64; 2],
    err4: [f64; 2],

    // Estado de puerta y suavizadores de telemetría
    gated: [bool; 2],
    gated_mean: f32,
    clip_disp_mean: f32,
    quant_disp_mean: f32,
    disp_accum: f32,

    // PRNGs independientes para reloj (jitter/chance) y dithering
    freq_rng: XorShift32,
    dither_rng: XorShift32,

    // Filtros de la cadena Wet FX
    wet_hp_prev_in: [f32; 2],
    wet_hp_prev_out: [f32; 2],
    wet_lp_state: [f32; 2],
}

impl Bit8Effect {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate: sample_rate.max(8_000.0),
            params: Bit8Params::default(),
            telemetry: Arc::new(Bit8Telemetry::new()),
            phase: 0.0,
            hold: [0.0; 2],
            last_hold: [0.0; 2],
            adaa_rectify: AdaaStereo::new(),
            adaa_clip: AdaaStereo::new(),
            adaa_quant: AdaaStereo::new(),
            adaa_gate: AdaaStereo::new(),
            align_delay: [[0.0; 2]; 2],
            err1: [0.0; 2],
            err2: [0.0; 2],
            err3: [0.0; 2],
            err4: [0.0; 2],
            gated: [false; 2],
            gated_mean: 1.0,
            clip_disp_mean: 0.0,
            quant_disp_mean: 0.0,
            disp_accum: 0.0,
            freq_rng: XorShift32::new(0x5EED_C10C),
            dither_rng: XorShift32::new(0xD174_E8A5),
            wet_hp_prev_in: [0.0; 2],
            wet_hp_prev_out: [0.0; 2],
            wet_lp_state: [0.0; 2],
        }
    }

    pub fn telemetry(&self) -> Arc<Bit8Telemetry> {
        Arc::clone(&self.telemetry)
    }

    pub fn params(&self) -> Bit8Params {
        self.params
    }

    pub fn set_params(&mut self, next: Bit8Params) {
        let clamped = next.clamped();
        self.reset_on_mode_change(self.params, clamped);
        self.params = clamped;
    }

    fn reset_on_mode_change(&mut self, prev: Bit8Params, next: Bit8Params) {
        if prev.anti_alias != next.anti_alias {
            self.adaa_rectify.reset();
            self.adaa_clip.reset();
            self.adaa_quant.reset();
            self.adaa_gate.reset();
            self.align_delay = [[0.0; 2]; 2];
        } else {
            if prev.rectify != next.rectify {
                self.adaa_rectify.reset();
            }
            if prev.shape_mode != next.shape_mode {
                self.adaa_clip.reset();
            }
            if prev.quantize_mode != next.quantize_mode || prev.squared_domain != next.squared_domain {
                self.adaa_quant.reset();
                self.err1 = [0.0; 2];
                self.err2 = [0.0; 2];
                self.err3 = [0.0; 2];
                self.err4 = [0.0; 2];
            }
        }
    }

    #[inline]
    fn delay_half(&mut self, x: [f64; 2], idx: usize) -> [f64; 2] {
        let prev = self.align_delay[idx];
        self.align_delay[idx] = x;
        [(x[0] + prev[0]) * 0.5, (x[1] + prev[1]) * 0.5]
    }

    #[inline]
    fn delay_one(&mut self, x: [f64; 2], idx: usize) -> [f64; 2] {
        let prev = self.align_delay[idx];
        self.align_delay[idx] = x;
        prev
    }

    #[inline]
    fn process_rectify(&mut self, input: [f64; 2], rectify: Bit8Rectify, aa: bool) -> [f64; 2] {
        match rectify {
            Bit8Rectify::Off => input,
            Bit8Rectify::Half | Bit8Rectify::Full => {
                if aa {
                    self.adaa_rectify.process(
                        input,
                        |v| rectify_antideriv(v, rectify),
                        |v| apply_rectify_direct(v, rectify),
                    )
                } else {
                    [
                        apply_rectify_direct(input[0], rectify),
                        apply_rectify_direct(input[1], rectify),
                    ]
                }
            }
        }
    }

    #[inline]
    fn process_clip(
        &mut self,
        x: [f64; 2],
        mode: Bit8ShapeMode,
        aa: bool,
        clip_level: f64,
    ) -> [f64; 2] {
        if mode == Bit8ShapeMode::Off {
            return x;
        }
        let g = clip_level * 2.0;
        let driven = [x[0] * g, x[1] * g];
        let shaped = if aa {
            self.adaa_clip.process(
                driven,
                |v| clip_antideriv(v, mode),
                |v| apply_clip_direct(v, mode),
            )
        } else {
            [
                apply_clip_direct(driven[0], mode),
                apply_clip_direct(driven[1], mode),
            ]
        };
        [shaped[0] * 0.5, shaped[1] * 0.5]
    }

    #[inline]
    fn process_quant(
        &mut self,
        x: [f64; 2],
        mode: Bit8QuantizeMode,
        quanta: f64,
        diffusion: f64,
        stereo_dither: bool,
        aa: bool,
    ) -> [f64; 2] {
        let inv_q = 1.0 / quanta;
        match mode {
            Bit8QuantizeMode::OffZero => {
                let offset = 0.5;
                let shifted = [x[0] * inv_q + offset, x[1] * inv_q + offset];
                let q = if aa {
                    self.adaa_quant.process(shifted, round_antideriv, round_direct)
                } else {
                    [shifted[0].round(), shifted[1].round()]
                };
                [(q[0] - offset) * quanta, (q[1] - offset) * quanta]
            }
            Bit8QuantizeMode::OnZero => {
                let scaled = [x[0] * inv_q, x[1] * inv_q];
                let q = if aa {
                    self.adaa_quant.process(scaled, round_antideriv, round_direct)
                } else {
                    [scaled[0].round(), scaled[1].round()]
                };
                [q[0] * quanta, q[1] * quanta]
            }
            Bit8QuantizeMode::Diffusion => {
                let d = 1.0 - diffusion;
                let om = 1.0 - d;
                let mut y = [0.0_f64; 2];
                for ch in 0..2 {
                    let a = self.err1[ch] * om + self.err2[ch] * d;
                    let b = self.err2[ch] * om + self.err3[ch] * d;
                    let c = self.err3[ch] * om + self.err4[ch] * d;
                    y[ch] = x[ch]
                        + (a * om + b * d) * om
                        + (b * om + c * d) * om * d;
                }
                let z = if aa {
                    let shifted = [y[0] * inv_q - 0.5, y[1] * inv_q - 0.5];
                    let q = self.adaa_quant.process(shifted, round_antideriv, round_direct);
                    [q[0] * quanta, q[1] * quanta]
                } else {
                    [(y[0] * inv_q).floor() * quanta, (y[1] * inv_q).floor() * quanta]
                };
                self.err4 = self.err3;
                self.err3 = self.err2;
                self.err2 = self.err1;
                self.err1 = [
                    (y[0] - z[0]).clamp(-0.5, 0.5),
                    (y[1] - z[1]).clamp(-0.5, 0.5),
                ];
                z
            }
            Bit8QuantizeMode::Dither => {
                let tri_l = (self.dither_rng.next_bipolar() + self.dither_rng.next_bipolar()) as f64;
                let tri_r = if stereo_dither {
                    (self.dither_rng.next_bipolar() + self.dither_rng.next_bipolar()) as f64
                } else {
                    tri_l
                };
                let y = [
                    x[0] + quanta * tri_l * 0.25,
                    x[1] + quanta * tri_r * 0.25,
                ];
                if aa {
                    let shifted = [y[0] * inv_q - 0.5, y[1] * inv_q - 0.5];
                    let q = self.adaa_quant.process(shifted, round_antideriv, round_direct);
                    [q[0] * quanta, q[1] * quanta]
                } else {
                    [(y[0] * inv_q).floor() * quanta, (y[1] * inv_q).floor() * quanta]
                }
            }
        }
    }

    #[inline]
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        let p = self.params;
        let sr = self.sample_rate;

        let in_l = if left.is_finite() { left } else { 0.0 };
        let in_r = if right.is_finite() { right } else { 0.0 };
        let inp = [in_l as f64, in_r as f64];

        let quanta = p.quantize_step() as f64;
        let threshold = db_to_lin(p.gate_db) as f64;
        let clip_level = db_to_lin(p.shape_drive_db) as f64;

        // 1. Reloj con Jitter, Chance e interpolación fraccional sub-muestra.
        let rnd = self.freq_rng.next_bipolar();
        let rate = ((1.0 + p.jitter * rnd) * p.clock_hz / sr).max(1.0e-6);
        self.phase += rate;

        let mut sub_phase = 0.0_f32;
        let mut clip_display = 0.0_f32;
        let mut quant_display = 0.0_f32;

        if self.phase > 1.0 {
            let r = self.freq_rng.next_unipolar();
            if r < p.chance {
                sub_phase = ((self.phase - 1.0) / rate).clamp(0.0, 1.0);

                // 2. Shape: Rectify + Clip (con ADAA1 opcional).
                let mut x = self.process_rectify(inp, p.rectify, p.anti_alias);
                x = self.process_clip(x, p.shape_mode, p.anti_alias, clip_level);

                let ref_gain = if p.shape_mode != Bit8ShapeMode::Off {
                    clip_level
                } else {
                    1.0
                };
                let mut x_before_clip = [inp[0] * ref_gain, inp[1] * ref_gain];
                if p.anti_alias {
                    if p.rectify != Bit8Rectify::Off && p.shape_mode != Bit8ShapeMode::Off {
                        x_before_clip = self.delay_one(x_before_clip, 0);
                    } else if p.rectify != Bit8Rectify::Off || p.shape_mode != Bit8ShapeMode::Off {
                        x_before_clip = self.delay_half(x_before_clip, 0);
                    }
                }
                let dc = (x_before_clip[0] - x[0]).abs() + (x_before_clip[1] - x[1]).abs();
                clip_display = (1000.0 * dc as f32).min(1.0);

                // 3. Squared Domain (compansión sqrt pre-cuantización).
                if p.squared_domain {
                    x = [
                        x[0].abs().sqrt().copysign(x[0]),
                        x[1].abs().sqrt().copysign(x[1]),
                    ];
                }

                // 4. Quantize (Off-zero / On-zero / Diffusion / Dither).
                let mut h = self.process_quant(
                    x,
                    p.quantize_mode,
                    quanta,
                    p.diffusion_brightness as f64,
                    p.stereo_dither,
                    p.anti_alias,
                );

                let x_for_disp = if p.anti_alias {
                    self.delay_half(x, 1)
                } else {
                    x
                };
                quant_display =
                    ((h[0] - x_for_disp[0]).abs() + (h[1] - x_for_disp[1]).abs()) as f32;

                // 5. Squared Domain (expansión x² post-cuantización).
                if p.squared_domain {
                    h = [
                        (h[0] * h[0]).copysign(h[0]),
                        (h[1] * h[1]).copysign(h[1]),
                    ];
                }

                // 6. Gate post-cuantización controlado por la entrada (ADAA1 si Anti-alias).
                self.gated = [inp[0].abs() > threshold, inp[1].abs() > threshold];
                if p.anti_alias {
                    let g_aa = self.adaa_gate.process(
                        inp,
                        |v| threshold_gate_antideriv(v, threshold),
                        |v| threshold_gate_direct(v, threshold),
                    );
                    h = [h[0] * g_aa[0], h[1] * g_aa[1]];
                } else {
                    h = [
                        if self.gated[0] { h[0] } else { 0.0 },
                        if self.gated[1] { h[1] } else { 0.0 },
                    ];
                }

                self.hold = [h[0] as f32, h[1] as f32];
            }
            self.phase = (self.phase - 1.0).min(1.0);
        }

        let mut wet = [
            sub_phase * self.hold[0] + (1.0 - sub_phase) * self.last_hold[0],
            sub_phase * self.hold[1] + (1.0 - sub_phase) * self.last_hold[1],
        ];
        self.last_hold = self.hold;

        // 7. Telemetría (LEDs y trazas de 3s suavizadas de forma consistente).
        let c = (1.0 - (-1.0 / (0.011_338 * sr)).exp()).clamp(1.0e-5, 0.25);
        let gated_f = if !(self.gated[0] || self.gated[1]) {
            1.0
        } else {
            0.0
        };
        self.gated_mean = (1.0 - c) * self.gated_mean + c * gated_f;
        self.clip_disp_mean = (1.0 - c) * self.clip_disp_mean + c * clip_display;
        self.quant_disp_mean =
            (1.0 - c) * self.quant_disp_mean + c * (quant_display * 4.0).min(1.0);

        let gate_led = 1.0 - self.gated_mean;
        let clip_led = gate_led * self.clip_disp_mean;
        let quant_led = gate_led * self.quant_disp_mean;

        let gate_disp = -self.gated_mean;
        let clip_disp = gate_disp + 0.5 * self.clip_disp_mean;
        let quant_disp = gate_disp + 0.5 * (self.clip_disp_mean + self.quant_disp_mean);

        self.disp_accum += 60.0 / sr;
        if self.disp_accum >= 1.0 {
            self.disp_accum -= self.disp_accum.floor();
            self.telemetry.store_leds(gate_led, clip_led, quant_led);
            self.telemetry.push_display_point(gate_disp, clip_disp, quant_disp);
        }

        // 8. Cadena interna Wet FX (HP, LP y Gain sobre la señal wet).
        for ch in 0..2 {
            let mut s = wet[ch];
            if p.wet_hp_hz > 20.5 {
                let rc = 1.0 / (2.0 * PI * p.wet_hp_hz);
                let dt = 1.0 / sr;
                let a = rc / (rc + dt);
                let hp = a * (self.wet_hp_prev_out[ch] + s - self.wet_hp_prev_in[ch]);
                self.wet_hp_prev_in[ch] = s;
                self.wet_hp_prev_out[ch] = hp;
                s = hp;
            } else {
                self.wet_hp_prev_in[ch] = s;
                self.wet_hp_prev_out[ch] = s;
            }
            if p.wet_lp_hz < 19_900.0 {
                let rc = 1.0 / (2.0 * PI * p.wet_lp_hz);
                let dt = 1.0 / sr;
                let a = dt / (rc + dt);
                self.wet_lp_state[ch] += a * (s - self.wet_lp_state[ch]);
                s = self.wet_lp_state[ch];
            } else {
                self.wet_lp_state[ch] = s;
            }
            if p.wet_gain_db.abs() > 0.01 {
                s *= db_to_lin(p.wet_gain_db);
            }
            wet[ch] = if s.is_finite() { s.clamp(-4.0, 4.0) } else { 0.0 };
        }

        // 9. Stereo Width (Mid/Side post Wet FX).
        let half_l = 0.5 * wet[0];
        let half_r = 0.5 * wet[1];
        let w = p.width;
        let wl = half_l * (1.0 + w) + half_r * (1.0 - w);
        let wr = half_r * (1.0 + w) + half_l * (1.0 - w);

        // 10. Equal-Gain Dry / Wet Mix.
        let m = p.mix;
        (in_l * (1.0 - m) + wl * m, in_r * (1.0 - m) + wr * m)
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
        "Hikaru Open8-Bit"
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
            BIT8_PARAM_CLOCK_HZ => Some(mk(0, "Clock Frequency", "Hz", BIT8_MIN_CLOCK_HZ, BIT8_MAX_CLOCK_HZ, 0)),
            BIT8_PARAM_JITTER => Some(mk(1, "Clock Jitter", "%", 0.0, 100.0, 0)),
            BIT8_PARAM_CHANCE => Some(mk(2, "Clock Chance", "%", 0.0, 100.0, 0)),
            BIT8_PARAM_GATE_DB => Some(mk(3, "Gate Threshold", "dB", BIT8_MIN_DB, BIT8_MAX_DB, 0)),
            BIT8_PARAM_RECTIFY => Some(mk(4, "Rectify Mode", "", 0.0, 2.0, 2)),
            BIT8_PARAM_SHAPE_MODE => Some(mk(5, "Clip Mode", "", 0.0, 4.0, 4)),
            BIT8_PARAM_SHAPE_DRIVE_DB => Some(mk(6, "Drive", "dB", 0.0, BIT8_MAX_DRIVE_DB, 0)),
            BIT8_PARAM_QUANTIZE_MODE => Some(mk(7, "Quantization Mode", "", 0.0, 3.0, 3)),
            BIT8_PARAM_QUANTIZE_DB => Some(mk(8, "Quantization Level", "dB", BIT8_MIN_DB, BIT8_MAX_DB, 0)),
            BIT8_PARAM_DIFFUSION => Some(mk(9, "Diffusion Brightness", "%", 0.0, 100.0, 0)),
            BIT8_PARAM_STEREO_DITHER => Some(mk(10, "Stereo Dither", "", 0.0, 1.0, 1)),
            BIT8_PARAM_SQUARED_DOMAIN => Some(mk(11, "Squared Domain", "", 0.0, 1.0, 1)),
            BIT8_PARAM_ANTI_ALIAS => Some(mk(12, "Anti-alias", "", 0.0, 1.0, 1)),
            BIT8_PARAM_WIDTH => Some(mk(13, "Stereo Width", "%", 0.0, 100.0, 0)),
            BIT8_PARAM_MIX => Some(mk(14, "Mix", "%", 0.0, 100.0, 0)),
            BIT8_PARAM_WET_HP_HZ => Some(mk(15, "Wet HP", "Hz", 20.0, 2000.0, 0)),
            BIT8_PARAM_WET_LP_HZ => Some(mk(16, "Wet LP", "Hz", 1000.0, 20_000.0, 0)),
            BIT8_PARAM_WET_GAIN_DB => Some(mk(17, "Wet Gain", "dB", -12.0, 12.0, 0)),
            _ => None,
        }
    }

    fn get_param_normalized(&mut self, id: u32) -> Option<f32> {
        self.params.get_normalized(id)
    }

    fn set_param_normalized(&mut self, id: u32, value: f32) -> bool {
        let prev = self.params;
        let ok = self.params.set_normalized(id, value);
        if ok {
            self.reset_on_mode_change(prev, self.params);
        }
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit8_quantize_on_zero_and_off_zero_differ() {
        let mut fx = Bit8Effect::new(48_000.0);
        let base = Bit8Params {
            clock_hz: 100_000.0,
            jitter: 0.0,
            gate_db: -96.0,
            quantize_db: -12.0, // step ~= 0.2512
            ..Bit8Params::default()
        };

        fx.set_params(Bit8Params {
            quantize_mode: Bit8QuantizeMode::OnZero,
            ..base
        });
        let mut on_zero = 0.0;
        for _ in 0..4 {
            on_zero = fx.process_stereo(0.10, 0.10).0;
        }

        fx.set_params(Bit8Params {
            quantize_mode: Bit8QuantizeMode::OffZero,
            ..base
        });
        let mut off_zero = 0.0;
        for _ in 0..4 {
            off_zero = fx.process_stereo(0.10, 0.10).0;
        }

        assert!(on_zero.abs() < 1e-4, "On-zero should snap 0.10 to 0.0, got {on_zero}");
        assert!(off_zero > 0.10, "Off-zero should snap 0.10 to +0.5*step (~0.1256), got {off_zero}");
    }

    #[test]
    fn bit8_rectify_half_and_full_wave() {
        let mut fx = Bit8Effect::new(48_000.0);
        let base = Bit8Params {
            clock_hz: 100_000.0,
            jitter: 0.0,
            gate_db: -96.0,
            quantize_db: -96.0,
            ..Bit8Params::default()
        };

        fx.set_params(Bit8Params {
            rectify: Bit8Rectify::Half,
            ..base
        });
        let mut half = 1.0;
        for _ in 0..4 {
            half = fx.process_stereo(-0.5, -0.5).0;
        }
        assert!(half.abs() < 1e-3, "Half rectify should zero negative, got {half}");

        fx.set_params(Bit8Params {
            rectify: Bit8Rectify::Full,
            ..base
        });
        let mut full = 0.0;
        for _ in 0..4 {
            full = fx.process_stereo(-0.5, -0.5).0;
        }
        assert!((full - 0.5).abs() < 1e-3, "Full rectify should flip -0.5 to +0.5, got {full}");
    }

    #[test]
    fn bit8_clip_modes_operate_around_half_amplitude() {
        // A 0 dB drive, Hard Clip aplana en ±0.5 (g = 2.0, clamp ±1, * 0.5).
        let p_hard = Bit8Params {
            gate_db: -96.0,
            quantize_db: -96.0,
            shape_mode: Bit8ShapeMode::HardClip,
            shape_drive_db: 0.0,
            ..Bit8Params::default()
        };
        assert!((p_hard.eval_transfer(0.8) - 0.5).abs() < 1e-3);
        assert!((p_hard.eval_transfer(-0.8) + 0.5).abs() < 1e-3);

        // Soft Clip (curva racional de saturación suave) es impar y converge asintóticamente a ±0.5.
        let p_soft = Bit8Params {
            shape_mode: Bit8ShapeMode::SoftClip,
            shape_drive_db: 12.0,
            ..p_hard
        };
        let s_pos = p_soft.eval_transfer(0.5);
        let s_neg = p_soft.eval_transfer(-0.5);
        assert!(s_pos > 0.35 && s_pos <= 0.515, "softclip out of range: {s_pos}");
        assert!((s_pos + s_neg).abs() < 1e-3, "softclip should be symmetric odd");
        let p_soft_hi = Bit8Params {
            shape_drive_db: 36.0,
            ..p_soft
        };
        assert!((p_soft_hi.eval_transfer(0.8) - 0.5).abs() < 0.01);

        // Fold refleja al superar ±0.5: x=0.75 -> g*x=1.5 -> fold=0.5 -> *0.5 = 0.25.
        let p_fold = Bit8Params {
            shape_mode: Bit8ShapeMode::Fold,
            shape_drive_db: 0.0,
            ..p_hard
        };
        assert!((p_fold.eval_transfer(0.75) - 0.25).abs() < 1e-3);

        // Wrap invierte polaridad al superar +0.5: x=0.75 -> g*x=1.5 -> wrap=-0.5 -> *0.5 = -0.25.
        let p_wrap = Bit8Params {
            shape_mode: Bit8ShapeMode::Wrap,
            shape_drive_db: 0.0,
            ..p_hard
        };
        assert!((p_wrap.eval_transfer(0.75) + 0.25).abs() < 1e-3);
    }

    #[test]
    fn bit8_gate_post_quantize_silences_off_zero_dc() {
        let mut fx = Bit8Effect::new(48_000.0);
        fx.set_params(Bit8Params {
            clock_hz: 100_000.0,
            jitter: 0.0,
            gate_db: -20.0,
            quantize_mode: Bit8QuantizeMode::OffZero,
            quantize_db: -12.0,
            ..Bit8Params::default()
        });
        let mut last = 1.0;
        for _ in 0..16 {
            last = fx.process_stereo(0.0, 0.0).0;
        }
        assert!(last.abs() < 1e-6, "silence under gate must have exact zero DC, got {last}");
    }

    #[test]
    fn bit8_diffusion_and_stereo_dither_produce_valid_output() {
        let mut fx = Bit8Effect::new(48_000.0);
        fx.set_params(Bit8Params {
            clock_hz: 48_000.0,
            jitter: 0.0,
            gate_db: -96.0,
            quantize_mode: Bit8QuantizeMode::Diffusion,
            quantize_db: -18.0,
            diffusion_brightness: 0.75,
            anti_alias: true,
            ..Bit8Params::default()
        });
        let mut energy = 0.0;
        for i in 0..512 {
            let s = (i as f32 * 0.09).sin() * 0.6;
            let (l, r) = fx.process_stereo(s, s);
            assert!(l.is_finite() && r.is_finite());
            energy += l * l;
        }
        assert!(energy > 1.0);

        // Stereo Dither debe producir canales distintos para entrada mono idéntica.
        fx.set_params(Bit8Params {
            clock_hz: 100_000.0,
            jitter: 0.0,
            gate_db: -96.0,
            quantize_mode: Bit8QuantizeMode::Dither,
            quantize_db: -6.0,
            stereo_dither: true,
            anti_alias: false,
            ..Bit8Params::default()
        });
        let mut diff_sum = 0.0;
        for i in 0..256 {
            let s = (i as f32 * 0.05).sin() * 0.5;
            let (l, r) = fx.process_stereo(s, s);
            diff_sum += (l - r).abs();
        }
        assert!(diff_sum > 0.5, "Stereo Dither should decorrelate L/R, diff_sum={diff_sum}");
    }

    #[test]
    fn bit8_adaa_antiderivatives_match_numerical_derivatives() {
        // Verifica F'(x) == f(x) para todas las no linealidades ADAA1.
        let h = 1.0e-6;
        for &x in &[-2.35, -1.25, -0.45, 0.35, 1.35, 2.65] {
            for mode in [
                Bit8ShapeMode::HardClip,
                Bit8ShapeMode::SoftClip,
                Bit8ShapeMode::Fold,
                Bit8ShapeMode::Wrap,
            ] {
                let num = (clip_antideriv(x + h, mode) - clip_antideriv(x - h, mode)) / (2.0 * h);
                let exact = apply_clip_direct(x, mode);
                assert!(
                    (num - exact).abs() < 1e-4,
                    "ADAA clip derivative mismatch for {mode:?} at x={x}: num={num}, exact={exact}"
                );
            }
            for rect in [Bit8Rectify::Half, Bit8Rectify::Full] {
                let num = (rectify_antideriv(x + h, rect) - rectify_antideriv(x - h, rect)) / (2.0 * h);
                let exact = apply_rectify_direct(x, rect);
                assert!(
                    (num - exact).abs() < 1e-4,
                    "ADAA rectify derivative mismatch for {rect:?} at x={x}: num={num}, exact={exact}"
                );
            }
            let num_r = (round_antideriv(x + h) - round_antideriv(x - h)) / (2.0 * h);
            let exact_r = round_direct(x);
            assert!(
                (num_r - exact_r).abs() < 1e-4,
                "ADAA round mismatch at x={x}: num={num_r}, exact={exact_r}"
            );
            let num_g = (threshold_gate_antideriv(x + h, 0.5) - threshold_gate_antideriv(x - h, 0.5)) / (2.0 * h);
            let exact_g = threshold_gate_direct(x, 0.5);
            assert!(
                (num_g - exact_g).abs() < 1e-4,
                "ADAA gate mismatch at x={x}: num={num_g}, exact={exact_g}"
            );
        }
    }

    #[test]
    fn bit8_telemetry_updates_leds_and_traces() {
        let mut fx = Bit8Effect::new(48_000.0);
        fx.set_params(Bit8Params {
            clock_hz: 100_000.0,
            jitter: 0.0,
            gate_db: -40.0,
            shape_mode: Bit8ShapeMode::HardClip,
            shape_drive_db: 12.0,
            quantize_db: -18.0,
            ..Bit8Params::default()
        });
        for i in 0..4096 {
            let s = (i as f32 * 0.1).sin() * 0.8;
            let _ = fx.process_stereo(s, s);
        }
        let snap = fx.telemetry().snapshot();
        assert!(snap.write_seq > 0);
        assert!(snap.gate_led > 0.7, "expected gate LED active, got {}", snap.gate_led);
        assert!(snap.clip_led > 0.5, "expected clip LED active, got {}", snap.clip_led);
        assert!(snap.quant_led > 0.1, "expected quant LED active, got {}", snap.quant_led);
    }

    #[test]
    fn bit8_squared_domain_preserves_finer_low_level_steps() {
        let p_lin = Bit8Params {
            gate_db: -96.0,
            quantize_mode: Bit8QuantizeMode::OnZero,
            quantize_db: -12.0,
            squared_domain: false,
            ..Bit8Params::default()
        };
        let p_sq = Bit8Params {
            squared_domain: true,
            ..p_lin
        };
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
