// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_openmodulation/bay.rs
// Bitwig-style Device Modulator Bay engine (GNU AGPL v3)
//
// Provee el motor genérico y reutilizable de slots de moduladores acoplados a cualquier
// dispositivo del DSP Rack (OpenPoly, OpenGrid, OpenEQ3, OpenSpectral Phaser, etc.).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use hikaru_dsp::effects::lfo::{
    evaluate_waveform, pseudo_random_step, LfoSyncMode, LfoWaveform, SyncRate,
};
use hikaru_project_file::{DeviceModulatorBayParams, ModulatorRouteParams, ModulatorSlotParams};

use super::lfo_mod::{clock_time_secs, LfoTransportState, ModulationTarget};

/// Ancho de una columna de slots dentro del Device Modulator Bay (px).
pub const BAY_COL_WIDTH: f32 = 62.0;
/// Separación entre columnas y entre el bay y el cuerpo del dispositivo (px).
pub const BAY_GAP: f32 = 4.0;
/// Cantidad de slots apilados verticalmente por columna (igual que Bitwig Studio 5.1.7).
pub const BAY_SLOTS_PER_COL: usize = 3;

/// Las 8 categorías oficiales de moduladores de Bitwig Studio 5.1.7.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModulatorCategory {
    AudioDriven,
    Envelope,
    Interface,
    Lfo,
    Modifier,
    NoteDriven,
    Sequence,
    VoiceStacking,
}

impl ModulatorCategory {
    pub const ALL: [ModulatorCategory; 8] = [
        ModulatorCategory::AudioDriven,
        ModulatorCategory::Envelope,
        ModulatorCategory::Interface,
        ModulatorCategory::Lfo,
        ModulatorCategory::Modifier,
        ModulatorCategory::NoteDriven,
        ModulatorCategory::Sequence,
        ModulatorCategory::VoiceStacking,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::AudioDriven => "Audio-driven",
            Self::Envelope => "Envelope",
            Self::Interface => "Interface",
            Self::Lfo => "LFO",
            Self::Modifier => "Modifier",
            Self::NoteDriven => "Note-driven",
            Self::Sequence => "Sequence",
            Self::VoiceStacking => "Voice Stacking",
        }
    }
}

/// Operación matemática del modulador `Math`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MathOp {
    Add,
    Subtract,
    Multiply,
    Min,
    Max,
    Average,
    AbsDiff,
}

impl MathOp {
    pub const ALL: [MathOp; 7] = [
        MathOp::Add,
        MathOp::Subtract,
        MathOp::Multiply,
        MathOp::Min,
        MathOp::Max,
        MathOp::Average,
        MathOp::AbsDiff,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Add => "Add (A+B)",
            Self::Subtract => "Sub (A-B)",
            Self::Multiply => "Mul (A×B)",
            Self::Min => "Min",
            Self::Max => "Max",
            Self::Average => "Avg",
            Self::AbsDiff => "|A-B|",
        }
    }

    pub fn short_symbol(self) -> &'static str {
        match self {
            Self::Add => "A+B",
            Self::Subtract => "A-B",
            Self::Multiply => "A×B",
            Self::Min => "MIN",
            Self::Max => "MAX",
            Self::Average => "AVG",
            Self::AbsDiff => "|Δ|",
        }
    }

    pub fn apply(self, a: f32, b: f32) -> f32 {
        match self {
            Self::Add => (a + b).clamp(-1.0, 1.0),
            Self::Subtract => (a - b).clamp(-1.0, 1.0),
            Self::Multiply => (a * b).clamp(-1.0, 1.0),
            Self::Min => a.min(b).clamp(-1.0, 1.0),
            Self::Max => a.max(b).clamp(-1.0, 1.0),
            Self::Average => ((a + b) * 0.5).clamp(-1.0, 1.0),
            Self::AbsDiff => (a - b).abs().clamp(0.0, 1.0),
        }
    }
}

/// Tipos de moduladores nativos soportados en el Device Modulator Bay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModulatorKind {
    ClassicLfo,
    BeatLfo,
    Macro,
    Macro4,
    Button,
    Buttons,
    Xy,
    Math,
    Mix,
    Quantize,
    Select4,
    Polynom,
}

impl ModulatorKind {
    pub fn meta(self) -> &'static ModulatorMeta {
        MODULATOR_CATALOG
            .iter()
            .find(|m| m.kind == self)
            .unwrap_or(&MODULATOR_CATALOG[0])
    }

    pub fn from_name(name: &str) -> Option<Self> {
        let lower = name.trim().to_ascii_lowercase();
        match lower.as_str() {
            "classic lfo" | "classiclfo" | "lfo" => Some(Self::ClassicLfo),
            "beat lfo" | "beatlfo" => Some(Self::BeatLfo),
            "macro" => Some(Self::Macro),
            "macro-4" | "macro4" | "macro 4" => Some(Self::Macro4),
            "button" => Some(Self::Button),
            "buttons" | "buttons-4" | "buttons4" => Some(Self::Buttons),
            "xy" | "x-y" | "xy pad" => Some(Self::Xy),
            "math" => Some(Self::Math),
            "mix" => Some(Self::Mix),
            "quantize" => Some(Self::Quantize),
            "select-4" | "select4" | "select 4" => Some(Self::Select4),
            "polynom" | "polynomial" => Some(Self::Polynom),
            _ => None,
        }
    }
}

/// Metadatos estáticos de un modulador para el Pop-up Browser y la UI del bay.
#[derive(Debug, Clone, Copy)]
pub struct ModulatorMeta {
    pub kind: ModulatorKind,
    pub name: &'static str,
    pub short_name: &'static str,
    pub category: ModulatorCategory,
    pub description: &'static str,
    pub outputs: &'static [&'static str],
}

pub const MODULATOR_CATALOG: &[ModulatorMeta] = &[
    ModulatorMeta {
        kind: ModulatorKind::ClassicLfo,
        name: "Classic LFO",
        short_name: "Classic",
        category: ModulatorCategory::Lfo,
        description: "LFO with discrete waveshapes (Sine, Triangle, Saw Up/Down, Square, S&H), Free Hz / Tempo Sync, Phase, Depth and Bipolar/Unipolar polarity.",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::BeatLfo,
        name: "Beat LFO",
        short_name: "BeatLFO",
        category: ModulatorCategory::Lfo,
        description: "Tempo-synced musical LFO locked to project beats/bars with phase skew, depth, and bipolar/unipolar output.",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::Macro,
        name: "Macro",
        short_name: "Macro",
        category: ModulatorCategory::Interface,
        description: "Single macro control knob that modulates any number of device parameters simultaneously.",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::Macro4,
        name: "Macro-4",
        short_name: "Macro-4",
        category: ModulatorCategory::Interface,
        description: "Four independent macro knobs with individual bipolar/unipolar polarity and modulation outputs.",
        outputs: &["M1", "M2", "M3", "M4"],
    },
    ModulatorMeta {
        kind: ModulatorKind::Button,
        name: "Button",
        short_name: "Button",
        category: ModulatorCategory::Interface,
        description: "Toggle or momentary on/off switch modulator with optional smoothing.",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::Buttons,
        name: "Buttons",
        short_name: "Buttons",
        category: ModulatorCategory::Interface,
        description: "Four independent toggle or momentary on/off switch modulators in a single slot.",
        outputs: &["B1", "B2", "B3", "B4"],
    },
    ModulatorMeta {
        kind: ModulatorKind::Xy,
        name: "XY",
        short_name: "XY",
        category: ModulatorCategory::Interface,
        description: "2D XY pad modulator with independent X and Y modulation outputs.",
        outputs: &["X", "Y"],
    },
    ModulatorMeta {
        kind: ModulatorKind::Math,
        name: "Math",
        short_name: "Math",
        category: ModulatorCategory::Modifier,
        description: "Combines two modulation values A and B using Add, Subtract, Multiply, Min, Max, Average, or AbsDiff.",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::Mix,
        name: "Mix",
        short_name: "Mix",
        category: ModulatorCategory::Modifier,
        description: "Crossfades smoothly between two modulation values A and B controlled by a Mix knob.",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::Quantize,
        name: "Quantize",
        short_name: "Quant",
        category: ModulatorCategory::Modifier,
        description: "Stepped quantizer that snaps a continuous input signal into N discrete levels (2..32 steps).",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::Select4,
        name: "Select-4",
        short_name: "Sel-4",
        category: ModulatorCategory::Modifier,
        description: "Selects or smoothly morphs across four modulation values using a continuous selector.",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::Polynom,
        name: "Polynom",
        short_name: "Poly",
        category: ModulatorCategory::Modifier,
        description: "Cubic polynomial waveshaper (c0 + c1·x + c2·x² + c3·x³) for custom nonlinear modulation curves.",
        outputs: &["OUT"],
    },
];

/// Ruta de modulación desde una salida del modulador hacia un parámetro de dispositivo.
#[derive(Debug, Clone, PartialEq)]
pub struct BayModRoute {
    pub out_idx: u8,
    pub target: ModulationTarget,
    pub amount: f32,
}

/// Estado en vivo de una instancia de modulador dentro del Device Modulator Bay.
#[derive(Debug, Clone)]
pub struct ModulatorSlotState {
    pub id: u32,
    pub kind: ModulatorKind,
    pub enabled: bool,

    // Classic LFO
    pub lfo_waveform: LfoWaveform,
    pub lfo_sync_mode: LfoSyncMode,
    pub lfo_rate_hz: f32,
    pub lfo_sync_rate: SyncRate,
    pub lfo_phase_deg: f32,
    pub lfo_depth: f32,
    pub lfo_bipolar: bool,
    pub lfo_retrigger: bool,
    pub lfo_per_voice: bool,
    pub lfo_last_phase: f32,
    lfo_last_sh_cycle: u64,
    lfo_last_sh_val: f32,

    // Macro
    pub macro_value: f32,
    pub macro_bipolar: bool,

    // Macro-4
    pub macro4_values: [f32; 4],
    pub macro4_bipolar: [bool; 4],

    // Button
    pub button_on: bool,
    pub button_momentary: bool,

    // Buttons (4 pads)
    pub buttons_on: [bool; 4],
    pub buttons_momentary: [bool; 4],

    // XY Pad
    pub xy_x: f32,
    pub xy_y: f32,
    pub xy_bipolar: bool,

    // Beat LFO (uses lfo_waveform, lfo_sync_rate, lfo_phase_deg, lfo_depth, lfo_bipolar + skew)
    pub beat_lfo_skew: f32,

    // Math
    pub math_op: MathOp,
    pub math_a: f32,
    pub math_b: f32,

    // Mix
    pub mix_a: f32,
    pub mix_b: f32,
    pub mix_amount: f32,

    // Quantize
    pub quant_in: f32,
    pub quant_steps: u8,
    pub quant_bipolar: bool,

    // Select-4
    pub sel4_values: [f32; 4],
    pub sel4_index: f32,
    pub sel4_smooth: bool,

    // Polynom (c0 + c1*x + c2*x^2 + c3*x^3)
    pub poly_in: f32,
    pub poly_c0: f32,
    pub poly_c1: f32,
    pub poly_c2: f32,
    pub poly_c3: f32,

    // Rutas de modulación
    pub routes: Vec<BayModRoute>,
    pub last_outputs: [f32; 4],
}

impl ModulatorSlotState {
    pub fn new(id: u32, kind: ModulatorKind) -> Self {
        Self {
            id,
            kind,
            enabled: true,
            lfo_waveform: LfoWaveform::Sine,
            lfo_sync_mode: if kind == ModulatorKind::BeatLfo {
                LfoSyncMode::TempoSync
            } else {
                LfoSyncMode::FreeHz
            },
            lfo_rate_hz: 1.0,
            lfo_sync_rate: SyncRate::Quarter,
            lfo_phase_deg: 0.0,
            lfo_depth: 1.0,
            lfo_bipolar: true,
            lfo_retrigger: false,
            lfo_per_voice: false,
            lfo_last_phase: 0.0,
            lfo_last_sh_cycle: 0,
            lfo_last_sh_val: 0.0,
            macro_value: 0.5,
            macro_bipolar: false,
            macro4_values: [0.5; 4],
            macro4_bipolar: [false; 4],
            button_on: false,
            button_momentary: false,
            buttons_on: [false; 4],
            buttons_momentary: [false; 4],
            xy_x: 0.5,
            xy_y: 0.5,
            xy_bipolar: false,
            beat_lfo_skew: 0.5,
            math_op: MathOp::Add,
            math_a: 0.5,
            math_b: 0.5,
            mix_a: 0.0,
            mix_b: 1.0,
            mix_amount: 0.5,
            quant_in: 0.5,
            quant_steps: 4,
            quant_bipolar: false,
            sel4_values: [0.0, 0.33, 0.66, 1.0],
            sel4_index: 0.0,
            sel4_smooth: true,
            poly_in: 0.5,
            poly_c0: 0.0,
            poly_c1: 1.0,
            poly_c2: 0.0,
            poly_c3: 0.0,
            routes: Vec::new(),
            last_outputs: [0.0; 4],
        }
    }

    pub fn eval_polynom(&self, x: f32) -> f32 {
        let x = x.clamp(-1.0, 1.0);
        let x2 = x * x;
        let x3 = x2 * x;
        (self.poly_c0 + self.poly_c1 * x + self.poly_c2 * x2 + self.poly_c3 * x3).clamp(-1.0, 1.0)
    }

    pub fn eval_quantize(&self, input: f32) -> f32 {
        let steps = (self.quant_steps.clamp(2, 32) - 1) as f32;
        if self.quant_bipolar {
            let norm = (input.clamp(-1.0, 1.0) + 1.0) * 0.5;
            let q = (norm * steps).round() / steps;
            (q * 2.0 - 1.0).clamp(-1.0, 1.0)
        } else {
            let norm = input.clamp(0.0, 1.0);
            ((norm * steps).round() / steps).clamp(0.0, 1.0)
        }
    }

    pub fn warp_beat_phase(phase: f32, skew: f32) -> f32 {
        let p = phase.rem_euclid(1.0);
        let s = skew.clamp(0.05, 0.95);
        if p < s {
            0.5 * (p / s)
        } else {
            0.5 + 0.5 * ((p - s) / (1.0 - s))
        }
    }

    pub fn effective_lfo_hz(&self, bpm: f32) -> f32 {
        match self.lfo_sync_mode {
            LfoSyncMode::FreeHz => self.lfo_rate_hz.clamp(0.02, 30.0),
            LfoSyncMode::TempoSync => self.lfo_sync_rate.hz_at_bpm(bpm),
        }
    }

    /// Evalúa las salidas en vivo del modulador (rango normalizado -1.0..+1.0 o 0.0..1.0).
    pub fn evaluate(&mut self, transport: &LfoTransportState, time_secs: f32) -> [f32; 4] {
        if !self.enabled {
            self.last_outputs = [0.0; 4];
            return self.last_outputs;
        }
        let mut outs = [0.0_f32; 4];
        match self.kind {
            ModulatorKind::ClassicLfo => {
                let phase_offset = (self.lfo_phase_deg / 360.0).rem_euclid(1.0);
                let total_cycles = match self.lfo_sync_mode {
                    LfoSyncMode::TempoSync => {
                        if transport.is_playing {
                            let elapsed_samples = transport
                                .sample_count
                                .saturating_sub(transport.playback_start_sample);
                            let elapsed_secs =
                                elapsed_samples as f64 / transport.sample_rate.max(1.0) as f64;
                            let beats = elapsed_secs * (transport.bpm.max(20.0) as f64 / 60.0);
                            let bpc = self.lfo_sync_rate.beats_per_cycle().max(1.0e-4) as f64;
                            beats / bpc + phase_offset as f64
                        } else {
                            let hz = self.lfo_sync_rate.hz_at_bpm(transport.bpm) as f64;
                            time_secs as f64 * hz + phase_offset as f64
                        }
                    }
                    LfoSyncMode::FreeHz => {
                        let hz = self.lfo_rate_hz.clamp(0.02, 30.0) as f64;
                        time_secs as f64 * hz + phase_offset as f64
                    }
                };
                let cycle_idx = total_cycles.floor().max(0.0) as u64;
                let phase = (total_cycles.fract() as f32).rem_euclid(1.0);
                if cycle_idx != self.lfo_last_sh_cycle {
                    self.lfo_last_sh_cycle = cycle_idx;
                    self.lfo_last_sh_val =
                        pseudo_random_step(cycle_idx.wrapping_add(self.id as u64 * 97));
                }
                self.lfo_last_phase = phase;
                let raw = evaluate_waveform(self.lfo_waveform, phase, self.lfo_last_sh_val);
                let shaped = if self.lfo_bipolar {
                    raw
                } else {
                    (raw + 1.0) * 0.5
                };
                outs[0] = (shaped * self.lfo_depth.clamp(0.0, 1.0)).clamp(-1.0, 1.0);
            }
            ModulatorKind::BeatLfo => {
                let phase_offset = (self.lfo_phase_deg / 360.0).rem_euclid(1.0);
                let total_cycles = if transport.is_playing {
                    let elapsed_samples = transport
                        .sample_count
                        .saturating_sub(transport.playback_start_sample);
                    let elapsed_secs =
                        elapsed_samples as f64 / transport.sample_rate.max(1.0) as f64;
                    let beats = elapsed_secs * (transport.bpm.max(20.0) as f64 / 60.0);
                    let bpc = self.lfo_sync_rate.beats_per_cycle().max(1.0e-4) as f64;
                    beats / bpc + phase_offset as f64
                } else {
                    let hz = self.lfo_sync_rate.hz_at_bpm(transport.bpm) as f64;
                    time_secs as f64 * hz + phase_offset as f64
                };
                let cycle_idx = total_cycles.floor().max(0.0) as u64;
                let raw_phase = (total_cycles.fract() as f32).rem_euclid(1.0);
                if cycle_idx != self.lfo_last_sh_cycle {
                    self.lfo_last_sh_cycle = cycle_idx;
                    self.lfo_last_sh_val =
                        pseudo_random_step(cycle_idx.wrapping_add(self.id as u64 * 131));
                }
                self.lfo_last_phase = raw_phase;
                let warped = Self::warp_beat_phase(raw_phase, self.beat_lfo_skew);
                let raw = evaluate_waveform(self.lfo_waveform, warped, self.lfo_last_sh_val);
                let shaped = if self.lfo_bipolar {
                    raw
                } else {
                    (raw + 1.0) * 0.5
                };
                outs[0] = (shaped * self.lfo_depth.clamp(0.0, 1.0)).clamp(-1.0, 1.0);
            }
            ModulatorKind::Macro => {
                let v = self.macro_value.clamp(0.0, 1.0);
                outs[0] = if self.macro_bipolar { v * 2.0 - 1.0 } else { v };
            }
            ModulatorKind::Macro4 => {
                for i in 0..4 {
                    let v = self.macro4_values[i].clamp(0.0, 1.0);
                    outs[i] = if self.macro4_bipolar[i] { v * 2.0 - 1.0 } else { v };
                }
            }
            ModulatorKind::Button => {
                outs[0] = if self.button_on { 1.0 } else { 0.0 };
            }
            ModulatorKind::Buttons => {
                for i in 0..4 {
                    outs[i] = if self.buttons_on[i] { 1.0 } else { 0.0 };
                }
            }
            ModulatorKind::Xy => {
                let x = self.xy_x.clamp(0.0, 1.0);
                let y = self.xy_y.clamp(0.0, 1.0);
                outs[0] = if self.xy_bipolar { x * 2.0 - 1.0 } else { x };
                outs[1] = if self.xy_bipolar { y * 2.0 - 1.0 } else { y };
            }
            ModulatorKind::Math => {
                outs[0] = self.math_op.apply(self.math_a, self.math_b);
            }
            ModulatorKind::Mix => {
                let m = self.mix_amount.clamp(0.0, 1.0);
                outs[0] = (self.mix_a * (1.0 - m) + self.mix_b * m).clamp(-1.0, 1.0);
            }
            ModulatorKind::Quantize => {
                outs[0] = self.eval_quantize(self.quant_in);
            }
            ModulatorKind::Select4 => {
                let pos = self.sel4_index.clamp(0.0, 1.0) * 3.0;
                if self.sel4_smooth {
                    let i0 = (pos.floor() as usize).min(2);
                    let frac = (pos - i0 as f32).clamp(0.0, 1.0);
                    let v = self.sel4_values[i0] * (1.0 - frac) + self.sel4_values[i0 + 1] * frac;
                    outs[0] = v.clamp(-1.0, 1.0);
                } else {
                    let idx = (pos.round() as usize).min(3);
                    outs[0] = self.sel4_values[idx].clamp(-1.0, 1.0);
                }
            }
            ModulatorKind::Polynom => {
                outs[0] = self.eval_polynom(self.poly_in);
            }
        }
        self.last_outputs = outs;
        outs
    }

    pub fn route_amount(&self, out_idx: u8, target: ModulationTarget) -> f32 {
        if target == ModulationTarget::None {
            return 0.0;
        }
        self.routes
            .iter()
            .find(|r| r.out_idx == out_idx && r.target == target)
            .map(|r| r.amount)
            .unwrap_or(0.0)
    }

    pub fn set_route(&mut self, out_idx: u8, target: ModulationTarget, amount: f32) {
        if target == ModulationTarget::None {
            return;
        }
        let clamped = amount.clamp(-1.0, 1.0);
        if clamped.abs() < 0.005 {
            self.routes
                .retain(|r| !(r.out_idx == out_idx && r.target == target));
        } else if let Some(existing) = self
            .routes
            .iter_mut()
            .find(|r| r.out_idx == out_idx && r.target == target)
        {
            existing.amount = clamped;
        } else {
            self.routes.push(BayModRoute {
                out_idx,
                target,
                amount: clamped,
            });
        }
    }

    pub fn has_routes_for_output(&self, out_idx: u8) -> bool {
        self.routes
            .iter()
            .any(|r| r.out_idx == out_idx && r.amount.abs() >= 0.005)
    }

    pub fn clear_routes(&mut self) {
        self.routes.clear();
    }

    pub fn clear_routes_for_output(&mut self, out_idx: u8) {
        self.routes.retain(|r| r.out_idx != out_idx);
    }

    pub fn to_project_params(&self) -> ModulatorSlotParams {
        let (params, bool_flags, mode_idx) = match self.kind {
            ModulatorKind::ClassicLfo => {
                let wf_idx = LfoWaveform::ALL
                    .iter()
                    .position(|&w| w == self.lfo_waveform)
                    .unwrap_or(0) as u8;
                let sync_idx = SyncRate::ALL
                    .iter()
                    .position(|&r| r == self.lfo_sync_rate)
                    .unwrap_or(4) as f32;
                (
                    vec![self.lfo_rate_hz, self.lfo_depth, self.lfo_phase_deg, sync_idx],
                    vec![
                        self.lfo_sync_mode == LfoSyncMode::TempoSync,
                        self.lfo_bipolar,
                        self.lfo_retrigger,
                        self.lfo_per_voice,
                    ],
                    wf_idx,
                )
            }
            ModulatorKind::BeatLfo => {
                let wf_idx = LfoWaveform::ALL
                    .iter()
                    .position(|&w| w == self.lfo_waveform)
                    .unwrap_or(0) as u8;
                let sync_idx = SyncRate::ALL
                    .iter()
                    .position(|&r| r == self.lfo_sync_rate)
                    .unwrap_or(4) as f32;
                (
                    vec![sync_idx, self.lfo_depth, self.lfo_phase_deg, self.beat_lfo_skew],
                    vec![self.lfo_bipolar],
                    wf_idx,
                )
            }
            ModulatorKind::Macro => (vec![self.macro_value], vec![self.macro_bipolar], 0),
            ModulatorKind::Macro4 => (
                self.macro4_values.to_vec(),
                self.macro4_bipolar.to_vec(),
                0,
            ),
            ModulatorKind::Button => (vec![], vec![self.button_on, self.button_momentary], 0),
            ModulatorKind::Buttons => {
                let mut flags = Vec::with_capacity(8);
                flags.extend_from_slice(&self.buttons_on);
                flags.extend_from_slice(&self.buttons_momentary);
                (vec![], flags, 0)
            }
            ModulatorKind::Xy => (vec![self.xy_x, self.xy_y], vec![self.xy_bipolar], 0),
            ModulatorKind::Math => {
                let op_idx = MathOp::ALL
                    .iter()
                    .position(|&op| op == self.math_op)
                    .unwrap_or(0) as u8;
                (vec![self.math_a, self.math_b], vec![], op_idx)
            }
            ModulatorKind::Mix => (vec![self.mix_a, self.mix_b, self.mix_amount], vec![], 0),
            ModulatorKind::Quantize => (
                vec![self.quant_in],
                vec![self.quant_bipolar],
                self.quant_steps.clamp(2, 32),
            ),
            ModulatorKind::Select4 => {
                let mut params = self.sel4_values.to_vec();
                params.push(self.sel4_index);
                (params, vec![self.sel4_smooth], 0)
            }
            ModulatorKind::Polynom => (
                vec![self.poly_in, self.poly_c0, self.poly_c1, self.poly_c2, self.poly_c3],
                vec![],
                0,
            ),
        };
        let routes = self
            .routes
            .iter()
            .filter(|r| r.target != ModulationTarget::None && r.amount.abs() >= 0.005)
            .map(|r| ModulatorRouteParams {
                out_idx: r.out_idx,
                target: r.target.label().to_string(),
                amount: r.amount,
            })
            .collect();
        ModulatorSlotParams {
            id: self.id,
            kind: self.kind.meta().name.to_string(),
            enabled: self.enabled,
            params,
            bool_flags,
            mode_idx,
            routes,
        }
    }

    pub fn from_project_params(p: &ModulatorSlotParams) -> Option<Self> {
        let kind = ModulatorKind::from_name(&p.kind)?;
        let mut st = Self::new(p.id.max(1), kind);
        st.enabled = p.enabled;
        match kind {
            ModulatorKind::ClassicLfo => {
                st.lfo_waveform = LfoWaveform::ALL
                    .get(p.mode_idx as usize)
                    .copied()
                    .unwrap_or(LfoWaveform::Sine);
                if let Some(&hz) = p.params.first() {
                    st.lfo_rate_hz = hz.clamp(0.02, 30.0);
                }
                if let Some(&depth) = p.params.get(1) {
                    st.lfo_depth = depth.clamp(0.0, 1.0);
                }
                if let Some(&phase) = p.params.get(2) {
                    st.lfo_phase_deg = phase.clamp(0.0, 360.0);
                }
                if let Some(&sr_idx) = p.params.get(3) {
                    st.lfo_sync_rate = SyncRate::ALL
                        .get(sr_idx.round().max(0.0) as usize)
                        .copied()
                        .unwrap_or(SyncRate::Quarter);
                }
                if let Some(&tempo_sync) = p.bool_flags.first() {
                    st.lfo_sync_mode = if tempo_sync {
                        LfoSyncMode::TempoSync
                    } else {
                        LfoSyncMode::FreeHz
                    };
                }
                if let Some(&bip) = p.bool_flags.get(1) {
                    st.lfo_bipolar = bip;
                }
                if let Some(&ret) = p.bool_flags.get(2) {
                    st.lfo_retrigger = ret;
                }
                if let Some(&pv) = p.bool_flags.get(3) {
                    st.lfo_per_voice = pv;
                }
            }
            ModulatorKind::BeatLfo => {
                st.lfo_sync_mode = LfoSyncMode::TempoSync;
                st.lfo_waveform = LfoWaveform::ALL
                    .get(p.mode_idx as usize)
                    .copied()
                    .unwrap_or(LfoWaveform::Sine);
                if let Some(&sr_idx) = p.params.first() {
                    st.lfo_sync_rate = SyncRate::ALL
                        .get(sr_idx.round().max(0.0) as usize)
                        .copied()
                        .unwrap_or(SyncRate::Quarter);
                }
                if let Some(&depth) = p.params.get(1) {
                    st.lfo_depth = depth.clamp(0.0, 1.0);
                }
                if let Some(&phase) = p.params.get(2) {
                    st.lfo_phase_deg = phase.clamp(0.0, 360.0);
                }
                if let Some(&skew) = p.params.get(3) {
                    st.beat_lfo_skew = skew.clamp(0.05, 0.95);
                }
                if let Some(&bip) = p.bool_flags.first() {
                    st.lfo_bipolar = bip;
                }
            }
            ModulatorKind::Macro => {
                if let Some(&v) = p.params.first() {
                    st.macro_value = v.clamp(0.0, 1.0);
                }
                if let Some(&bip) = p.bool_flags.first() {
                    st.macro_bipolar = bip;
                }
            }
            ModulatorKind::Macro4 => {
                for i in 0..4 {
                    if let Some(&v) = p.params.get(i) {
                        st.macro4_values[i] = v.clamp(0.0, 1.0);
                    }
                    if let Some(&bip) = p.bool_flags.get(i) {
                        st.macro4_bipolar[i] = bip;
                    }
                }
            }
            ModulatorKind::Button => {
                if let Some(&on) = p.bool_flags.first() {
                    st.button_on = on;
                }
                if let Some(&mom) = p.bool_flags.get(1) {
                    st.button_momentary = mom;
                }
            }
            ModulatorKind::Buttons => {
                for i in 0..4 {
                    if let Some(&on) = p.bool_flags.get(i) {
                        st.buttons_on[i] = on;
                    }
                    if let Some(&mom) = p.bool_flags.get(4 + i) {
                        st.buttons_momentary[i] = mom;
                    }
                }
            }
            ModulatorKind::Xy => {
                if let Some(&x) = p.params.first() {
                    st.xy_x = x.clamp(0.0, 1.0);
                }
                if let Some(&y) = p.params.get(1) {
                    st.xy_y = y.clamp(0.0, 1.0);
                }
                if let Some(&bip) = p.bool_flags.first() {
                    st.xy_bipolar = bip;
                }
            }
            ModulatorKind::Math => {
                st.math_op = MathOp::ALL
                    .get(p.mode_idx as usize)
                    .copied()
                    .unwrap_or(MathOp::Add);
                if let Some(&a) = p.params.first() {
                    st.math_a = a.clamp(-1.0, 1.0);
                }
                if let Some(&b) = p.params.get(1) {
                    st.math_b = b.clamp(-1.0, 1.0);
                }
            }
            ModulatorKind::Mix => {
                if let Some(&a) = p.params.first() {
                    st.mix_a = a.clamp(-1.0, 1.0);
                }
                if let Some(&b) = p.params.get(1) {
                    st.mix_b = b.clamp(-1.0, 1.0);
                }
                if let Some(&amt) = p.params.get(2) {
                    st.mix_amount = amt.clamp(0.0, 1.0);
                }
            }
            ModulatorKind::Quantize => {
                if let Some(&inp) = p.params.first() {
                    st.quant_in = inp.clamp(-1.0, 1.0);
                }
                if let Some(&bip) = p.bool_flags.first() {
                    st.quant_bipolar = bip;
                }
                st.quant_steps = p.mode_idx.clamp(2, 32);
            }
            ModulatorKind::Select4 => {
                for i in 0..4 {
                    if let Some(&v) = p.params.get(i) {
                        st.sel4_values[i] = v.clamp(-1.0, 1.0);
                    }
                }
                if let Some(&idx) = p.params.get(4) {
                    st.sel4_index = idx.clamp(0.0, 1.0);
                }
                if let Some(&sm) = p.bool_flags.first() {
                    st.sel4_smooth = sm;
                }
            }
            ModulatorKind::Polynom => {
                if let Some(&x) = p.params.first() {
                    st.poly_in = x.clamp(-1.0, 1.0);
                }
                if let Some(&c0) = p.params.get(1) {
                    st.poly_c0 = c0.clamp(-2.0, 2.0);
                }
                if let Some(&c1) = p.params.get(2) {
                    st.poly_c1 = c1.clamp(-2.0, 2.0);
                }
                if let Some(&c2) = p.params.get(3) {
                    st.poly_c2 = c2.clamp(-2.0, 2.0);
                }
                if let Some(&c3) = p.params.get(4) {
                    st.poly_c3 = c3.clamp(-2.0, 2.0);
                }
            }
        }
        st.routes.clear();
        for r in &p.routes {
            let target = ModulationTarget::parse_saved(&r.target, ModulationTarget::None);
            if target != ModulationTarget::None && r.amount.abs() >= 0.005 {
                st.routes.push(BayModRoute {
                    out_idx: r.out_idx,
                    target,
                    amount: r.amount.clamp(-1.0, 1.0),
                });
            }
        }
        Some(st)
    }
}

/// Estado completo del Device Modulator Bay acoplado a un slot `(track_idx, slot_idx)`.
#[derive(Debug, Clone)]
pub struct DeviceModulatorBay {
    pub slots: Vec<ModulatorSlotState>,
    pub next_id: u32,
    pub selected_slot_id: Option<u32>,
}

impl Default for DeviceModulatorBay {
    fn default() -> Self {
        // Inicia con un Classic LFO listo (igual que Bitwig Polymer / Poly Grid)
        Self {
            slots: vec![ModulatorSlotState::new(1, ModulatorKind::ClassicLfo)],
            next_id: 2,
            selected_slot_id: None,
        }
    }
}

impl DeviceModulatorBay {
    pub fn empty() -> Self {
        Self {
            slots: Vec::new(),
            next_id: 1,
            selected_slot_id: None,
        }
    }

    pub fn add_modulator(&mut self, kind: ModulatorKind) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.slots.push(ModulatorSlotState::new(id, kind));
        self.selected_slot_id = Some(id);
        id
    }

    pub fn remove_modulator(&mut self, id: u32) {
        self.slots.retain(|s| s.id != id);
        if self.selected_slot_id == Some(id) {
            self.selected_slot_id = None;
        }
    }

    /// Cantidad de columnas necesarias para alojar `slots.len()` moduladores + el botón `+`.
    pub fn column_count(&self) -> usize {
        let total_cells = self.slots.len() + 1;
        total_cells.div_ceil(BAY_SLOTS_PER_COL).max(1)
    }

    /// Ancho total en píxeles que ocupa el bay (incluyendo el gap derecho hacia el dispositivo).
    pub fn width_px(&self) -> f32 {
        let cols = self.column_count() as f32;
        cols * BAY_COL_WIDTH + cols * BAY_GAP
    }

    pub fn to_project_params(&self, open: bool) -> DeviceModulatorBayParams {
        DeviceModulatorBayParams {
            open,
            slots: self.slots.iter().map(|s| s.to_project_params()).collect(),
        }
    }

    pub fn apply_project_params(&mut self, p: &DeviceModulatorBayParams) {
        self.slots = p
            .slots
            .iter()
            .filter_map(ModulatorSlotState::from_project_params)
            .collect();
        let max_id = self.slots.iter().map(|s| s.id).max().unwrap_or(0);
        self.next_id = max_id.saturating_add(1);
        self.selected_slot_id = None;
    }
}

type BayMap = Mutex<HashMap<(usize, usize), DeviceModulatorBay>>;
fn bay_state_map() -> &'static BayMap {
    static CELL: OnceLock<BayMap> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn get_bay_state(track_idx: usize, slot_idx: usize) -> DeviceModulatorBay {
    let mut map = bay_state_map().lock().unwrap();
    map.entry((track_idx, slot_idx))
        .or_insert_with(DeviceModulatorBay::default)
        .clone()
}

pub fn update_bay_state<F: FnOnce(&mut DeviceModulatorBay)>(
    track_idx: usize,
    slot_idx: usize,
    f: F,
) {
    let mut map = bay_state_map().lock().unwrap();
    let entry = map
        .entry((track_idx, slot_idx))
        .or_insert_with(DeviceModulatorBay::default);
    f(entry);
}

pub fn add_bay_modulator(track_idx: usize, slot_idx: usize, kind: ModulatorKind) -> u32 {
    let mut new_id = 0;
    update_bay_state(track_idx, slot_idx, |bay| {
        new_id = bay.add_modulator(kind);
    });
    new_id
}

pub fn remove_bay_modulator(track_idx: usize, slot_idx: usize, mod_id: u32) {
    update_bay_state(track_idx, slot_idx, |bay| {
        bay.remove_modulator(mod_id);
    });
}

pub fn clear_bay_for_slot(track_idx: usize, slot_idx: usize) {
    let mut map = bay_state_map().lock().unwrap();
    map.remove(&(track_idx, slot_idx));
}

pub fn bay_width_for_slot(track_idx: usize, slot_idx: usize) -> f32 {
    let mut map = bay_state_map().lock().unwrap();
    let entry = map
        .entry((track_idx, slot_idx))
        .or_insert_with(DeviceModulatorBay::default);
    entry.width_px()
}

pub fn get_bay_route_amount(
    track_idx: usize,
    slot_idx: usize,
    mod_id: u32,
    out_idx: u8,
    device: &str,
    param: &str,
) -> f32 {
    let Some(target) = ModulationTarget::from_device_and_param(device, param) else {
        return 0.0;
    };
    let map = bay_state_map().lock().unwrap();
    map.get(&(track_idx, slot_idx))
        .and_then(|bay| bay.slots.iter().find(|s| s.id == mod_id))
        .map(|s| s.route_amount(out_idx, target))
        .unwrap_or(0.0)
}

pub fn set_bay_route_amount(
    track_idx: usize,
    slot_idx: usize,
    mod_id: u32,
    out_idx: u8,
    device: &str,
    param: &str,
    amount: f32,
) {
    let Some(target) = ModulationTarget::from_device_and_param(device, param) else {
        return;
    };
    update_bay_state(track_idx, slot_idx, |bay| {
        if let Some(s) = bay.slots.iter_mut().find(|s| s.id == mod_id) {
            s.set_route(out_idx, target, amount);
        }
    });
}

/// Captura los parámetros del bay para persistencia en `.oplf`/`.opsf`/`.hdp`.
pub fn capture_bay_params(
    track_idx: usize,
    slot_idx: usize,
    open: bool,
) -> Option<DeviceModulatorBayParams> {
    let map = bay_state_map().lock().unwrap();
    map.get(&(track_idx, slot_idx))
        .map(|bay| bay.to_project_params(open))
}

/// Restaura los parámetros del bay al cargar un proyecto o preset.
pub fn restore_bay_params(track_idx: usize, slot_idx: usize, params: &DeviceModulatorBayParams) {
    let mut map = bay_state_map().lock().unwrap();
    let entry = map
        .entry((track_idx, slot_idx))
        .or_insert_with(DeviceModulatorBay::empty);
    entry.apply_project_params(params);
}

/// Suma las contribuciones de todos los moduladores de bay activos en `track_idx`
/// para un parámetro `(device, param)`. Devuelve `(matched, sum_raw, sum_delta, sum_amount, last_phase)`.
pub fn evaluate_bay_modulation_for_track(
    track_idx: usize,
    device: &str,
    param: &str,
    transport: &LfoTransportState,
) -> (bool, f32, f32, f32, f32) {
    let Some(target) = ModulationTarget::from_device_and_param(device, param) else {
        return (false, 0.0, 0.0, 0.0, 0.0);
    };
    let time_secs = clock_time_secs();
    let mut map = bay_state_map().lock().unwrap();

    let mut matched = false;
    let mut sum_raw = 0.0_f32;
    let mut sum_delta = 0.0_f32;
    let mut sum_amount = 0.0_f32;
    let mut last_phase = 0.0_f32;

    for (&(tr, _slot), bay) in map.iter_mut() {
        if tr != track_idx {
            continue;
        }
        for m in &mut bay.slots {
            if !m.enabled || m.routes.is_empty() {
                continue;
            }
            let has_match = m
                .routes
                .iter()
                .any(|r| r.target == target && r.amount.abs() >= 0.005);
            if !has_match {
                continue;
            }
            let outs = m.evaluate(transport, time_secs);
            for r in &m.routes {
                if r.target == target && r.amount.abs() >= 0.005 {
                    let raw = outs.get(r.out_idx as usize).copied().unwrap_or(0.0);
                    matched = true;
                    sum_raw += raw;
                    sum_delta += raw * r.amount;
                    sum_amount += r.amount;
                    if matches!(m.kind, ModulatorKind::ClassicLfo | ModulatorKind::BeatLfo) {
                        last_phase = m.lfo_last_phase;
                    }
                }
            }
        }
    }

    (matched, sum_raw, sum_delta, sum_amount, last_phase)
}
