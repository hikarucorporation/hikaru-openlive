/*
 * Hikaru OpenGrid - Polyphonic Modular Patch Graph Synthesizer DSP Engine
 * Copyright (C) 2026 Hikaru Corporation
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 */

use std::collections::HashMap;
use std::f32::consts::{PI, TAU};
use std::sync::Arc;

use super::open_poly::{eval_osc_preview, OpenPolyOscType, OpenPolyWavetableData};

pub const MAX_OPEN_GRID_VOICES: usize = 16;
pub const MAX_GRID_PORTS: usize = 4;
const DELAY_LINE_LEN: usize = 4096;
const SCOPE_LEN: usize = 128;

/// All 16 module categories matching the OpenGrid palette strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GridCategory {
    Io,
    Display,
    Phase,
    Data,
    Oscillator,
    Random,
    Lfo,
    Envelope,
    Filter,
    Shaper,
    DelayFx,
    Mix,
    Level,
    Pitch,
    Math,
    Logic,
}

impl GridCategory {
    /// Ordered in 4x4 layout matching the palette strip in OpenGrid.
    pub const GRID_4X4: [[GridCategory; 4]; 4] = [
        [GridCategory::Io, GridCategory::Display, GridCategory::Phase, GridCategory::Data],
        [GridCategory::Oscillator, GridCategory::Random, GridCategory::Lfo, GridCategory::Envelope],
        [GridCategory::Filter, GridCategory::Shaper, GridCategory::DelayFx, GridCategory::Mix],
        [GridCategory::Level, GridCategory::Pitch, GridCategory::Math, GridCategory::Logic],
    ];

    pub const ALL: [GridCategory; 16] = [
        GridCategory::Io,
        GridCategory::Display,
        GridCategory::Phase,
        GridCategory::Data,
        GridCategory::Oscillator,
        GridCategory::Random,
        GridCategory::Lfo,
        GridCategory::Envelope,
        GridCategory::Filter,
        GridCategory::Shaper,
        GridCategory::DelayFx,
        GridCategory::Mix,
        GridCategory::Level,
        GridCategory::Pitch,
        GridCategory::Math,
        GridCategory::Logic,
    ];

    pub fn label(self) -> &'static str {
        match self {
            GridCategory::Io => "I/O",
            GridCategory::Display => "Display",
            GridCategory::Phase => "Phase",
            GridCategory::Data => "Data",
            GridCategory::Oscillator => "Oscillator",
            GridCategory::Random => "Random",
            GridCategory::Lfo => "LFO",
            GridCategory::Envelope => "Envelope",
            GridCategory::Filter => "Filter",
            GridCategory::Shaper => "Shaper",
            GridCategory::DelayFx => "Delay/FX",
            GridCategory::Mix => "Mix",
            GridCategory::Level => "Level",
            GridCategory::Pitch => "Pitch",
            GridCategory::Math => "Math",
            GridCategory::Logic => "Logic",
        }
    }

    pub fn accent_rgb(self) -> [u8; 3] {
        match self {
            GridCategory::Io => [0xE6, 0x4E, 0x52],
            GridCategory::Display => [0x9A, 0xA8, 0xB8],
            GridCategory::Phase => [0xA4, 0x70, 0xFF],
            GridCategory::Data => [0x38, 0xB6, 0xFF],
            GridCategory::Oscillator => [0xE8, 0x6A, 0x38],
            GridCategory::Random => [0x58, 0xC8, 0xA8],
            GridCategory::Lfo => [0xFF, 0x8A, 0x24],
            GridCategory::Envelope => [0x30, 0xC4, 0x98],
            GridCategory::Filter => [0x2B, 0xB8, 0xC8],
            GridCategory::Shaper => [0xE8, 0xA8, 0x24],
            GridCategory::DelayFx => [0x5A, 0x98, 0xF0],
            GridCategory::Mix => [0x8C, 0x94, 0xA4],
            GridCategory::Level => [0xD4, 0xB8, 0x48],
            GridCategory::Pitch => [0xF0, 0x94, 0x28],
            GridCategory::Math => [0x70, 0xB8, 0xE8],
            GridCategory::Logic => [0x38, 0xC8, 0x70],
        }
    }
}

/// Signal kind for color-coded jacks and Bezier patch cables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridSignalKind {
    Audio,
    Mod,
    Pitch,
    Gate,
    Phase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridPortSpec {
    pub name: &'static str,
    pub kind: GridSignalKind,
}

/// Parameter metadata so the OpenGrid canvas can render interactive knobs/controls for any module.
#[derive(Debug, Clone, Copy)]
pub struct GridParamSpec {
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub unit: &'static str,
    pub log_scale: bool,
    pub bipolar: bool,
}

/// All 97 genuinely implemented polyphonic DSP modules in Hikaru OpenGrid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GridModuleKind {
    // I/O (9)
    GateIn,
    PitchIn,
    VelocityIn,
    PhaseIn,
    AudioIn,
    AudioOut,
    ModulatorOut,
    NoteIn,
    PressureIn,
    // Oscillator (9)
    Wavetable,
    Sawtooth,
    Pulse,
    Sine,
    Triangle,
    Sub,
    Phase1,
    Swarm,
    Noise,
    // Envelope (4)
    Adsr,
    Ar,
    Ad,
    Follower,
    // Filter (8)
    LowPassMG,
    LowPass,
    LowPassLD,
    HighPass,
    Svf,
    SallenKey,
    Comb,
    Xp,
    // LFO (3)
    Lfo,
    WavetableLfo,
    ShLfo,
    // Shaper (8)
    Distortion,
    Saturator,
    Wavefolder,
    HardClip,
    Clip,
    Rectifier,
    Quantizer,
    Diode,
    // Delay/FX (5)
    Delay,
    ModDelay,
    AllPass,
    ChorusPlus,
    FlangerPlus,
    // Mix (5)
    Mixer,
    Blend,
    Pan,
    StereoWidth,
    StereoSplit,
    // Level (8)
    Amplify,
    Attenuate,
    GainDb,
    GainVol,
    Invert,
    Bias,
    BiToUni,
    UniToBi,
    // Pitch (5)
    Transpose,
    Octaver,
    Pitch,
    PitchQuantize,
    BySemitone,
    // Math (12)
    Add,
    Subtract,
    Multiply,
    Divide,
    Product,
    Sum,
    Average,
    Abs,
    MinMax,
    Constant,
    Value,
    Round,
    // Logic (7)
    And,
    Or,
    Xor,
    Not,
    Nand,
    Nor,
    Latch,
    // Phase (5)
    Phasor,
    PhaseReverse,
    PhaseShift,
    PhaseBend,
    PhasePinch,
    // Random (3)
    Dice,
    Chance,
    SampleHold,
    // Data (3)
    Steps,
    Triggers,
    Pitches,
    // Display (3)
    Oscilloscope,
    VuMeter,
    ValueReadout,
}

impl GridModuleKind {
    pub const ALL: [GridModuleKind; 97] = [
        // I/O (9)
        Self::GateIn, Self::PitchIn, Self::VelocityIn, Self::PhaseIn, Self::AudioIn,
        Self::AudioOut, Self::ModulatorOut, Self::NoteIn, Self::PressureIn,
        // Oscillator (9)
        Self::Wavetable, Self::Sawtooth, Self::Pulse, Self::Sine, Self::Triangle,
        Self::Sub, Self::Phase1, Self::Swarm, Self::Noise,
        // Envelope (4)
        Self::Adsr, Self::Ar, Self::Ad, Self::Follower,
        // Filter (8)
        Self::LowPassMG, Self::LowPass, Self::LowPassLD, Self::HighPass,
        Self::Svf, Self::SallenKey, Self::Comb, Self::Xp,
        // LFO (3)
        Self::Lfo, Self::WavetableLfo, Self::ShLfo,
        // Shaper (8)
        Self::Distortion, Self::Saturator, Self::Wavefolder, Self::HardClip,
        Self::Clip, Self::Rectifier, Self::Quantizer, Self::Diode,
        // Delay/FX (5)
        Self::Delay, Self::ModDelay, Self::AllPass, Self::ChorusPlus, Self::FlangerPlus,
        // Mix (5)
        Self::Mixer, Self::Blend, Self::Pan, Self::StereoWidth, Self::StereoSplit,
        // Level (8)
        Self::Amplify, Self::Attenuate, Self::GainDb, Self::GainVol,
        Self::Invert, Self::Bias, Self::BiToUni, Self::UniToBi,
        // Pitch (5)
        Self::Transpose, Self::Octaver, Self::Pitch, Self::PitchQuantize, Self::BySemitone,
        // Math (12)
        Self::Add, Self::Subtract, Self::Multiply, Self::Divide,
        Self::Product, Self::Sum, Self::Average, Self::Abs,
        Self::MinMax, Self::Constant, Self::Value, Self::Round,
        // Logic (7)
        Self::And, Self::Or, Self::Xor, Self::Not, Self::Nand, Self::Nor, Self::Latch,
        // Phase (5)
        Self::Phasor, Self::PhaseReverse, Self::PhaseShift, Self::PhaseBend, Self::PhasePinch,
        // Random (3)
        Self::Dice, Self::Chance, Self::SampleHold,
        // Data (3)
        Self::Steps, Self::Triggers, Self::Pitches,
        // Display (3)
        Self::Oscilloscope, Self::VuMeter, Self::ValueReadout,
    ];

    pub fn display_name(self) -> &'static str {
        match self {
            Self::GateIn => "Gate In",
            Self::PitchIn => "Pitch In",
            Self::VelocityIn => "Velocity In",
            Self::PhaseIn => "Phase In",
            Self::AudioIn => "Audio In",
            Self::AudioOut => "Audio Out",
            Self::ModulatorOut => "Modulator Out",
            Self::NoteIn => "Note In",
            Self::PressureIn => "Pressure In",

            Self::Wavetable => "Wavetable",
            Self::Sawtooth => "Sawtooth",
            Self::Pulse => "Pulse",
            Self::Sine => "Sine",
            Self::Triangle => "Triangle",
            Self::Sub => "Sub",
            Self::Phase1 => "Phase-1",
            Self::Swarm => "Swarm",
            Self::Noise => "Noise",

            Self::Adsr => "ADSR",
            Self::Ar => "AR",
            Self::Ad => "AD",
            Self::Follower => "Follower",

            Self::LowPassMG => "Low-pass MG",
            Self::LowPass => "Low-pass",
            Self::LowPassLD => "Low-pass LD",
            Self::HighPass => "High-pass",
            Self::Svf => "SVF",
            Self::SallenKey => "Sallen-Key",
            Self::Comb => "Comb",
            Self::Xp => "XP",

            Self::Lfo => "LFO",
            Self::WavetableLfo => "Wavetable LFO",
            Self::ShLfo => "S/H LFO",

            Self::Distortion => "Distortion",
            Self::Saturator => "Saturator",
            Self::Wavefolder => "Wavefolder",
            Self::HardClip => "Hard Clip",
            Self::Clip => "Clip",
            Self::Rectifier => "Rectifier",
            Self::Quantizer => "Quantizer",
            Self::Diode => "Diode",

            Self::Delay => "Delay",
            Self::ModDelay => "Mod Delay",
            Self::AllPass => "All-pass",
            Self::ChorusPlus => "Chorus+",
            Self::FlangerPlus => "Flanger+",

            Self::Mixer => "Mixer",
            Self::Blend => "Blend",
            Self::Pan => "Pan",
            Self::StereoWidth => "Stereo Width",
            Self::StereoSplit => "Stereo Split",

            Self::Amplify => "Amplify",
            Self::Attenuate => "Attenuate",
            Self::GainDb => "Gain - dB",
            Self::GainVol => "Gain - Vol",
            Self::Invert => "Invert",
            Self::Bias => "Bias",
            Self::BiToUni => "Bi To Uni",
            Self::UniToBi => "Uni To Bi",

            Self::Transpose => "Transpose",
            Self::Octaver => "Octaver",
            Self::Pitch => "Pitch",
            Self::PitchQuantize => "Pitch Quantize",
            Self::BySemitone => "by Semitone",

            Self::Add => "Add",
            Self::Subtract => "Subtract",
            Self::Multiply => "Multiply",
            Self::Divide => "Divide",
            Self::Product => "Product",
            Self::Sum => "Sum",
            Self::Average => "Average",
            Self::Abs => "Abs",
            Self::MinMax => "MinMax",
            Self::Constant => "Constant",
            Self::Value => "Value",
            Self::Round => "Round",

            Self::And => "AND",
            Self::Or => "OR",
            Self::Xor => "XOR",
            Self::Not => "NOT",
            Self::Nand => "NAND",
            Self::Nor => "NOR",
            Self::Latch => "Latch",

            Self::Phasor => "Phasor",
            Self::PhaseReverse => "Phase Reverse",
            Self::PhaseShift => "Phase Shift",
            Self::PhaseBend => "Phase Bend",
            Self::PhasePinch => "Phase Pinch",

            Self::Dice => "Dice",
            Self::Chance => "Chance",
            Self::SampleHold => "Sample / Hold",

            Self::Steps => "Steps",
            Self::Triggers => "Triggers",
            Self::Pitches => "Pitches",

            Self::Oscilloscope => "Oscilloscope",
            Self::VuMeter => "VU Meter",
            Self::ValueReadout => "Value Readout",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|k| k.display_name().eq_ignore_ascii_case(s))
    }

    pub fn category(self) -> GridCategory {
        match self {
            Self::GateIn | Self::PitchIn | Self::VelocityIn | Self::PhaseIn | Self::AudioIn
            | Self::AudioOut | Self::ModulatorOut | Self::NoteIn | Self::PressureIn => GridCategory::Io,

            Self::Wavetable | Self::Sawtooth | Self::Pulse | Self::Sine | Self::Triangle
            | Self::Sub | Self::Phase1 | Self::Swarm | Self::Noise => GridCategory::Oscillator,

            Self::Adsr | Self::Ar | Self::Ad | Self::Follower => GridCategory::Envelope,

            Self::LowPassMG | Self::LowPass | Self::LowPassLD | Self::HighPass
            | Self::Svf | Self::SallenKey | Self::Comb | Self::Xp => GridCategory::Filter,

            Self::Lfo | Self::WavetableLfo | Self::ShLfo => GridCategory::Lfo,

            Self::Distortion | Self::Saturator | Self::Wavefolder | Self::HardClip
            | Self::Clip | Self::Rectifier | Self::Quantizer | Self::Diode => GridCategory::Shaper,

            Self::Delay | Self::ModDelay | Self::AllPass | Self::ChorusPlus | Self::FlangerPlus => {
                GridCategory::DelayFx
            }

            Self::Mixer | Self::Blend | Self::Pan | Self::StereoWidth | Self::StereoSplit => {
                GridCategory::Mix
            }

            Self::Amplify | Self::Attenuate | Self::GainDb | Self::GainVol
            | Self::Invert | Self::Bias | Self::BiToUni | Self::UniToBi => GridCategory::Level,

            Self::Transpose | Self::Octaver | Self::Pitch | Self::PitchQuantize | Self::BySemitone => {
                GridCategory::Pitch
            }

            Self::Add | Self::Subtract | Self::Multiply | Self::Divide | Self::Product
            | Self::Sum | Self::Average | Self::Abs | Self::MinMax | Self::Constant
            | Self::Value | Self::Round => GridCategory::Math,

            Self::And | Self::Or | Self::Xor | Self::Not | Self::Nand | Self::Nor | Self::Latch => {
                GridCategory::Logic
            }

            Self::Phasor | Self::PhaseReverse | Self::PhaseShift | Self::PhaseBend | Self::PhasePinch => {
                GridCategory::Phase
            }

            Self::Dice | Self::Chance | Self::SampleHold => GridCategory::Random,
            Self::Steps | Self::Triggers | Self::Pitches => GridCategory::Data,
            Self::Oscilloscope | Self::VuMeter | Self::ValueReadout => GridCategory::Display,
        }
    }

    pub fn in_category(cat: GridCategory) -> Vec<GridModuleKind> {
        Self::ALL.iter().copied().filter(|m| m.category() == cat).collect()
    }

    pub fn inputs(self) -> &'static [GridPortSpec] {
        use GridSignalKind::*;
        const EMPTY: &[GridPortSpec] = &[];
        const AUDIO_1: &[GridPortSpec] = &[GridPortSpec { name: "In", kind: Audio }];
        const AUDIO_3: &[GridPortSpec] = &[
            GridPortSpec { name: "In 1", kind: Audio },
            GridPortSpec { name: "In 2", kind: Audio },
            GridPortSpec { name: "In 3", kind: Audio },
        ];
        const OSC_IN: &[GridPortSpec] = &[
            GridPortSpec { name: "Pitch", kind: Pitch },
            GridPortSpec { name: "Mod", kind: Mod },
            GridPortSpec { name: "Phase", kind: Phase },
        ];
        const ENV_IN: &[GridPortSpec] = &[
            GridPortSpec { name: "Audio", kind: Audio },
            GridPortSpec { name: "Gate", kind: Gate },
        ];
        const FLT_IN: &[GridPortSpec] = &[
            GridPortSpec { name: "In", kind: Audio },
            GridPortSpec { name: "Cut", kind: Mod },
            GridPortSpec { name: "Pitch", kind: Pitch },
        ];
        const BLEND_IN: &[GridPortSpec] = &[
            GridPortSpec { name: "A", kind: Audio },
            GridPortSpec { name: "B", kind: Audio },
            GridPortSpec { name: "Mix", kind: Mod },
        ];
        const PAN_IN: &[GridPortSpec] = &[
            GridPortSpec { name: "In", kind: Audio },
            GridPortSpec { name: "Pan", kind: Mod },
        ];
        const MATH_2: &[GridPortSpec] = &[
            GridPortSpec { name: "A", kind: Mod },
            GridPortSpec { name: "B", kind: Mod },
        ];
        const MATH_1: &[GridPortSpec] = &[GridPortSpec { name: "In", kind: Mod }];
        const LOGIC_2: &[GridPortSpec] = &[
            GridPortSpec { name: "A", kind: Gate },
            GridPortSpec { name: "B", kind: Gate },
        ];
        const LOGIC_1: &[GridPortSpec] = &[GridPortSpec { name: "In", kind: Gate }];
        const PHASE_IN: &[GridPortSpec] = &[
            GridPortSpec { name: "Ph", kind: Phase },
            GridPortSpec { name: "Mod", kind: Mod },
        ];
        const SH_IN: &[GridPortSpec] = &[
            GridPortSpec { name: "In", kind: Mod },
            GridPortSpec { name: "Trig", kind: Gate },
        ];
        const PITCH_IN: &[GridPortSpec] = &[GridPortSpec { name: "Pitch", kind: Pitch }];
        const DELAY_IN: &[GridPortSpec] = &[
            GridPortSpec { name: "In", kind: Audio },
            GridPortSpec { name: "Mod", kind: Mod },
        ];

        match self {
            Self::GateIn | Self::PitchIn | Self::VelocityIn | Self::PhaseIn | Self::AudioIn
            | Self::NoteIn | Self::PressureIn | Self::Noise | Self::Lfo | Self::WavetableLfo
            | Self::ShLfo | Self::Constant | Self::Value | Self::Pitch | Self::Phasor
            | Self::Steps | Self::Triggers | Self::Pitches => EMPTY,

            Self::AudioOut | Self::ModulatorOut | Self::Oscilloscope | Self::VuMeter
            | Self::ValueReadout | Self::Follower | Self::StereoWidth | Self::StereoSplit => AUDIO_1,

            Self::Wavetable | Self::Sawtooth | Self::Pulse | Self::Sine | Self::Triangle
            | Self::Sub | Self::Phase1 | Self::Swarm => OSC_IN,

            Self::Adsr | Self::Ar | Self::Ad => ENV_IN,

            Self::LowPassMG | Self::LowPass | Self::LowPassLD | Self::HighPass
            | Self::Svf | Self::SallenKey | Self::Comb | Self::Xp => FLT_IN,

            Self::Distortion | Self::Saturator | Self::Wavefolder | Self::HardClip
            | Self::Clip | Self::Rectifier | Self::Quantizer | Self::Diode
            | Self::Amplify | Self::Attenuate | Self::GainDb | Self::GainVol
            | Self::Invert | Self::Bias | Self::BiToUni | Self::UniToBi => AUDIO_1,

            Self::Delay | Self::ModDelay | Self::AllPass | Self::ChorusPlus | Self::FlangerPlus => {
                DELAY_IN
            }

            Self::Mixer => AUDIO_3,
            Self::Blend => BLEND_IN,
            Self::Pan => PAN_IN,

            Self::Transpose | Self::Octaver | Self::PitchQuantize | Self::BySemitone => PITCH_IN,

            Self::Add | Self::Subtract | Self::Multiply | Self::Divide | Self::Product
            | Self::Sum | Self::Average | Self::MinMax => MATH_2,
            Self::Abs | Self::Round => MATH_1,

            Self::And | Self::Or | Self::Xor | Self::Nand | Self::Nor | Self::Latch => LOGIC_2,
            Self::Not => LOGIC_1,

            Self::PhaseReverse | Self::PhaseShift | Self::PhaseBend | Self::PhasePinch => PHASE_IN,

            Self::Dice | Self::Chance => LOGIC_1,
            Self::SampleHold => SH_IN,
        }
    }

    pub fn outputs(self) -> &'static [GridPortSpec] {
        use GridSignalKind::*;
        const EMPTY: &[GridPortSpec] = &[];
        const AUDIO_1: &[GridPortSpec] = &[GridPortSpec { name: "Out", kind: Audio }];
        const AUDIO_2: &[GridPortSpec] = &[
            GridPortSpec { name: "L", kind: Audio },
            GridPortSpec { name: "R", kind: Audio },
        ];
        const MOD_1: &[GridPortSpec] = &[GridPortSpec { name: "Out", kind: Mod }];
        const PITCH_1: &[GridPortSpec] = &[GridPortSpec { name: "Pitch", kind: Pitch }];
        const GATE_1: &[GridPortSpec] = &[GridPortSpec { name: "Gate", kind: Gate }];
        const PHASE_1: &[GridPortSpec] = &[GridPortSpec { name: "Ph", kind: Phase }];
        const ENV_OUT: &[GridPortSpec] = &[
            GridPortSpec { name: "Out", kind: Audio },
            GridPortSpec { name: "Env", kind: Mod },
        ];
        const NOTE_OUT: &[GridPortSpec] = &[
            GridPortSpec { name: "Pitch", kind: Pitch },
            GridPortSpec { name: "Gate", kind: Gate },
            GridPortSpec { name: "Vel", kind: Mod },
        ];
        const MINMAX_OUT: &[GridPortSpec] = &[
            GridPortSpec { name: "Min", kind: Mod },
            GridPortSpec { name: "Max", kind: Mod },
        ];

        match self {
            Self::AudioOut | Self::ModulatorOut => EMPTY,
            Self::GateIn | Self::And | Self::Or | Self::Xor | Self::Not | Self::Nand
            | Self::Nor | Self::Latch | Self::Chance | Self::Triggers => GATE_1,
            Self::PitchIn | Self::Transpose | Self::Octaver | Self::Pitch
            | Self::PitchQuantize | Self::BySemitone | Self::Pitches => PITCH_1,
            Self::PhaseIn | Self::Phasor | Self::PhaseReverse | Self::PhaseShift
            | Self::PhaseBend | Self::PhasePinch => PHASE_1,
            Self::NoteIn => NOTE_OUT,
            Self::Adsr | Self::Ar | Self::Ad => ENV_OUT,
            Self::StereoSplit => AUDIO_2,
            Self::MinMax => MINMAX_OUT,
            Self::VelocityIn | Self::PressureIn | Self::Follower | Self::Lfo
            | Self::WavetableLfo | Self::ShLfo | Self::Add | Self::Subtract | Self::Multiply
            | Self::Divide | Self::Product | Self::Sum | Self::Average | Self::Abs
            | Self::Constant | Self::Value | Self::Round | Self::Dice | Self::SampleHold
            | Self::Steps | Self::BiToUni | Self::UniToBi => MOD_1,
            _ => AUDIO_1,
        }
    }

    pub fn param_specs(self) -> &'static [GridParamSpec] {
        const NONE: &[GridParamSpec] = &[];
        const OSC_WT: &[GridParamSpec] = &[
            GridParamSpec { name: "Index", min: 0.0, max: 1.0, default: 0.0, unit: "%", log_scale: false, bipolar: false },
            GridParamSpec { name: "Fold", min: 0.0, max: 1.0, default: 0.0, unit: "%", log_scale: false, bipolar: false },
            GridParamSpec { name: "Semi", min: -24.0, max: 24.0, default: 0.0, unit: "st", log_scale: false, bipolar: true },
            GridParamSpec { name: "Detune", min: -50.0, max: 50.0, default: 0.0, unit: "Hz", log_scale: false, bipolar: true },
        ];
        const SUB_P: &[GridParamSpec] = &[
            GridParamSpec { name: "Oct", min: -2.0, max: 0.0, default: -1.0, unit: "oct", log_scale: false, bipolar: false },
            GridParamSpec { name: "Wave", min: 0.0, max: 3.0, default: 0.0, unit: "", log_scale: false, bipolar: false },
            GridParamSpec { name: "Level", min: 0.0, max: 1.0, default: 0.8, unit: "%", log_scale: false, bipolar: false },
        ];
        const NOISE_P: &[GridParamSpec] = &[
            GridParamSpec { name: "Color", min: 0.0, max: 1.0, default: 0.5, unit: "%", log_scale: false, bipolar: false },
            GridParamSpec { name: "Level", min: 0.0, max: 1.0, default: 0.6, unit: "%", log_scale: false, bipolar: false },
        ];
        const ADSR_P: &[GridParamSpec] = &[
            GridParamSpec { name: "A", min: 0.5, max: 5000.0, default: 5.0, unit: "ms", log_scale: true, bipolar: false },
            GridParamSpec { name: "D", min: 1.0, max: 8000.0, default: 250.0, unit: "ms", log_scale: true, bipolar: false },
            GridParamSpec { name: "S", min: 0.0, max: 1.0, default: 0.78, unit: "%", log_scale: false, bipolar: false },
            GridParamSpec { name: "R", min: 1.0, max: 8000.0, default: 180.0, unit: "ms", log_scale: true, bipolar: false },
        ];
        const AR_P: &[GridParamSpec] = &[
            GridParamSpec { name: "A", min: 0.5, max: 5000.0, default: 5.0, unit: "ms", log_scale: true, bipolar: false },
            GridParamSpec { name: "R", min: 1.0, max: 8000.0, default: 200.0, unit: "ms", log_scale: true, bipolar: false },
        ];
        const FLT_P: &[GridParamSpec] = &[
            GridParamSpec { name: "Cutoff", min: 20.0, max: 20000.0, default: 1400.0, unit: "Hz", log_scale: true, bipolar: false },
            GridParamSpec { name: "Reso", min: 0.0, max: 0.96, default: 0.25, unit: "%", log_scale: false, bipolar: false },
            GridParamSpec { name: "Drive", min: 0.0, max: 1.0, default: 0.08, unit: "%", log_scale: false, bipolar: false },
            GridParamSpec { name: "Mod", min: -1.0, max: 1.0, default: 0.5, unit: "%", log_scale: false, bipolar: true },
        ];
        const LFO_P: &[GridParamSpec] = &[
            GridParamSpec { name: "Rate", min: 0.05, max: 30.0, default: 2.0, unit: "Hz", log_scale: true, bipolar: false },
            GridParamSpec { name: "Depth", min: 0.0, max: 1.0, default: 1.0, unit: "%", log_scale: false, bipolar: false },
            GridParamSpec { name: "Shape", min: 0.0, max: 1.0, default: 0.0, unit: "%", log_scale: false, bipolar: false },
        ];
        const SHAPER_P: &[GridParamSpec] = &[
            GridParamSpec { name: "Amount", min: 0.0, max: 1.0, default: 0.45, unit: "%", log_scale: false, bipolar: false },
            GridParamSpec { name: "Mix", min: 0.0, max: 1.0, default: 1.0, unit: "%", log_scale: false, bipolar: false },
        ];
        const DELAY_P: &[GridParamSpec] = &[
            GridParamSpec { name: "Time", min: 1.0, max: 80.0, default: 24.0, unit: "ms", log_scale: true, bipolar: false },
            GridParamSpec { name: "Fdbk", min: 0.0, max: 0.92, default: 0.38, unit: "%", log_scale: false, bipolar: false },
            GridParamSpec { name: "Mix", min: 0.0, max: 1.0, default: 0.45, unit: "%", log_scale: false, bipolar: false },
        ];
        const MIX3_P: &[GridParamSpec] = &[
            GridParamSpec { name: "Ch 1", min: 0.0, max: 1.0, default: 0.8, unit: "%", log_scale: false, bipolar: false },
            GridParamSpec { name: "Ch 2", min: 0.0, max: 1.0, default: 0.8, unit: "%", log_scale: false, bipolar: false },
            GridParamSpec { name: "Ch 3", min: 0.0, max: 1.0, default: 0.8, unit: "%", log_scale: false, bipolar: false },
        ];
        const SINGLE_UNI: &[GridParamSpec] = &[
            GridParamSpec { name: "Amount", min: 0.0, max: 1.0, default: 0.5, unit: "%", log_scale: false, bipolar: false },
        ];
        const SINGLE_BI: &[GridParamSpec] = &[
            GridParamSpec { name: "Value", min: -1.0, max: 1.0, default: 0.0, unit: "", log_scale: false, bipolar: true },
        ];
        const GAIN_DB_P: &[GridParamSpec] = &[
            GridParamSpec { name: "dB", min: -36.0, max: 18.0, default: 0.0, unit: "dB", log_scale: false, bipolar: true },
        ];
        const PITCH_ST_P: &[GridParamSpec] = &[
            GridParamSpec { name: "Semi", min: -24.0, max: 24.0, default: 0.0, unit: "st", log_scale: false, bipolar: true },
        ];
        const OCT_P: &[GridParamSpec] = &[
            GridParamSpec { name: "Oct", min: -3.0, max: 3.0, default: 0.0, unit: "oct", log_scale: false, bipolar: true },
        ];
        const DATA_P: &[GridParamSpec] = &[
            GridParamSpec { name: "Rate", min: 0.5, max: 16.0, default: 4.0, unit: "Hz", log_scale: true, bipolar: false },
            GridParamSpec { name: "Steps", min: 2.0, max: 8.0, default: 4.0, unit: "", log_scale: false, bipolar: false },
            GridParamSpec { name: "Range", min: 0.0, max: 1.0, default: 0.7, unit: "%", log_scale: false, bipolar: false },
        ];
        const AUDIO_OUT_P: &[GridParamSpec] = &[
            GridParamSpec { name: "Gain", min: 0.0, max: 2.0, default: 1.0, unit: "%", log_scale: false, bipolar: false },
        ];

        match self {
            Self::Wavetable | Self::Sawtooth | Self::Pulse | Self::Sine | Self::Triangle
            | Self::Phase1 | Self::Swarm => OSC_WT,
            Self::Sub => SUB_P,
            Self::Noise => NOISE_P,
            Self::Adsr => ADSR_P,
            Self::Ar | Self::Ad | Self::Follower => AR_P,
            Self::LowPassMG | Self::LowPass | Self::LowPassLD | Self::HighPass
            | Self::Svf | Self::SallenKey | Self::Comb | Self::Xp => FLT_P,
            Self::Lfo | Self::WavetableLfo | Self::ShLfo => LFO_P,
            Self::Distortion | Self::Saturator | Self::Wavefolder | Self::HardClip
            | Self::Clip | Self::Quantizer | Self::Diode => SHAPER_P,
            Self::Delay | Self::ModDelay | Self::AllPass | Self::ChorusPlus | Self::FlangerPlus => DELAY_P,
            Self::Mixer => MIX3_P,
            Self::Blend | Self::StereoWidth | Self::Attenuate | Self::GainVol
            | Self::Chance | Self::PhaseShift | Self::PhaseBend | Self::PhasePinch => SINGLE_UNI,
            Self::Pan | Self::Bias | Self::Constant | Self::Value => SINGLE_BI,
            Self::GainDb => GAIN_DB_P,
            Self::Amplify => AUDIO_OUT_P,
            Self::Transpose | Self::Pitch | Self::BySemitone => PITCH_ST_P,
            Self::Octaver => OCT_P,
            Self::Phasor => LFO_P,
            Self::Steps | Self::Triggers | Self::Pitches => DATA_P,
            Self::AudioOut => AUDIO_OUT_P,
            _ => NONE,
        }
    }

    pub fn default_params(self) -> [f32; 6] {
        let mut out = [0.0f32; 6];
        for (i, sp) in self.param_specs().iter().enumerate().take(6) {
            out[i] = sp.default;
        }
        out
    }

    pub fn default_size(self) -> [f32; 2] {
        match self {
            Self::Wavetable | Self::Sawtooth | Self::Pulse | Self::Sine | Self::Triangle
            | Self::Phase1 | Self::Swarm => [186.0, 126.0],
            Self::Adsr => [182.0, 124.0],
            Self::LowPassMG | Self::LowPass | Self::LowPassLD | Self::HighPass
            | Self::Svf | Self::SallenKey | Self::Comb | Self::Xp => [164.0, 118.0],
            Self::AudioOut => [68.0, 74.0],
            Self::Oscilloscope => [156.0, 102.0],
            Self::Steps | Self::Triggers | Self::Pitches => [154.0, 104.0],
            _ if self.param_specs().len() >= 3 => [146.0, 104.0],
            _ if !self.param_specs().is_empty() => [118.0, 88.0],
            _ => [94.0, 68.0],
        }
    }
}

/// A module instance placed on the OpenGrid canvas.
#[derive(Debug, Clone, PartialEq)]
pub struct GridNode {
    pub id: u32,
    pub kind: GridModuleKind,
    pub pos: [f32; 2],
    pub params: [f32; 6],
    pub bypassed: bool,
}

impl GridNode {
    pub fn new(id: u32, kind: GridModuleKind, pos: [f32; 2]) -> Self {
        Self {
            id,
            kind,
            pos,
            params: kind.default_params(),
            bypassed: false,
        }
    }
}

/// A directed patch cable connecting an output jack to an input jack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GridCable {
    pub from_node: u32,
    pub from_port: usize,
    pub to_node: u32,
    pub to_port: usize,
}

/// Complete patch graph for Hikaru OpenGrid (shared via `Arc` with the audio thread).
#[derive(Debug, Clone, PartialEq)]
pub struct OpenGridPatch {
    pub nodes: Vec<GridNode>,
    pub cables: Vec<GridCable>,
    pub global_pitch_st: f32,
    pub glide_ms: f32,
    pub legato: bool,
    pub mono_mode: bool,
    pub output_gain: f32,
    pub lfo_enabled: bool,
    pub lfo_rate_hz: f32,
    pub lfo_depth: f32,
}

impl Default for OpenGridPatch {
    fn default() -> Self {
        Self::default_poly_grid()
    }
}

impl OpenGridPatch {
    /// Default Bitwig Poly Grid-style patch: Wavetable -> ADSR -> Audio Out (ready to play!).
    pub fn default_poly_grid() -> Self {
        Self {
            nodes: vec![
                GridNode::new(1, GridModuleKind::Wavetable, [180.0, 120.0]),
                GridNode::new(2, GridModuleKind::Adsr, [420.0, 120.0]),
                GridNode::new(3, GridModuleKind::AudioOut, [660.0, 146.0]),
            ],
            cables: vec![
                GridCable { from_node: 1, from_port: 0, to_node: 2, to_port: 0 },
                GridCable { from_node: 2, from_port: 0, to_node: 3, to_port: 0 },
            ],
            global_pitch_st: 0.0,
            glide_ms: 0.0,
            legato: false,
            mono_mode: false,
            output_gain: 0.85,
            lfo_enabled: true,
            lfo_rate_hz: 1.5,
            lfo_depth: 0.5,
        }
    }

    pub fn preset_subtractive_mg() -> Self {
        let mut flt = GridNode::new(2, GridModuleKind::LowPassMG, [395.0, 120.0]);
        flt.params[0] = 680.0;
        flt.params[1] = 0.36;
        flt.params[3] = 0.65;
        let mut feg = GridNode::new(5, GridModuleKind::Adsr, [395.0, 265.0]);
        feg.params[1] = 320.0;
        feg.params[2] = 0.18;
        Self {
            nodes: vec![
                GridNode::new(1, GridModuleKind::Sawtooth, [160.0, 120.0]),
                flt,
                GridNode::new(3, GridModuleKind::Adsr, [610.0, 120.0]),
                GridNode::new(4, GridModuleKind::AudioOut, [840.0, 146.0]),
                feg,
            ],
            cables: vec![
                GridCable { from_node: 1, from_port: 0, to_node: 2, to_port: 0 },
                GridCable { from_node: 5, from_port: 1, to_node: 2, to_port: 1 },
                GridCable { from_node: 2, from_port: 0, to_node: 3, to_port: 0 },
                GridCable { from_node: 3, from_port: 0, to_node: 4, to_port: 0 },
            ],
            ..Self::default_poly_grid()
        }
    }

    pub fn preset_swarm_chorus() -> Self {
        let mut osc = GridNode::new(1, GridModuleKind::Swarm, [150.0, 120.0]);
        osc.params[0] = 0.55;
        let mut flt = GridNode::new(2, GridModuleKind::Svf, [385.0, 120.0]);
        flt.params[0] = 1850.0;
        let mut chr = GridNode::new(3, GridModuleKind::ChorusPlus, [595.0, 120.0]);
        chr.params[2] = 0.55;
        let mut env = GridNode::new(4, GridModuleKind::Adsr, [765.0, 120.0]);
        env.params[0] = 280.0;
        env.params[3] = 750.0;
        Self {
            nodes: vec![
                osc,
                flt,
                chr,
                env,
                GridNode::new(5, GridModuleKind::AudioOut, [985.0, 146.0]),
            ],
            cables: vec![
                GridCable { from_node: 1, from_port: 0, to_node: 2, to_port: 0 },
                GridCable { from_node: 2, from_port: 0, to_node: 3, to_port: 0 },
                GridCable { from_node: 3, from_port: 0, to_node: 4, to_port: 0 },
                GridCable { from_node: 4, from_port: 0, to_node: 5, to_port: 0 },
            ],
            ..Self::default_poly_grid()
        }
    }

    pub fn preset_acid_wavefolder() -> Self {
        let mut osc = GridNode::new(1, GridModuleKind::Pulse, [150.0, 120.0]);
        osc.params[0] = 0.35;
        let mut wf = GridNode::new(2, GridModuleKind::Wavefolder, [375.0, 120.0]);
        wf.params[0] = 0.55;
        let mut flt = GridNode::new(3, GridModuleKind::SallenKey, [535.0, 120.0]);
        flt.params[0] = 480.0;
        flt.params[1] = 0.68;
        let mut lfo = GridNode::new(6, GridModuleKind::Lfo, [375.0, 250.0]);
        lfo.params[0] = 3.2;
        Self {
            nodes: vec![
                osc,
                wf,
                flt,
                GridNode::new(4, GridModuleKind::Adsr, [740.0, 120.0]),
                GridNode::new(5, GridModuleKind::AudioOut, [960.0, 146.0]),
                lfo,
            ],
            cables: vec![
                GridCable { from_node: 1, from_port: 0, to_node: 2, to_port: 0 },
                GridCable { from_node: 2, from_port: 0, to_node: 3, to_port: 0 },
                GridCable { from_node: 6, from_port: 0, to_node: 3, to_port: 1 },
                GridCable { from_node: 3, from_port: 0, to_node: 4, to_port: 0 },
                GridCable { from_node: 4, from_port: 0, to_node: 5, to_port: 0 },
            ],
            mono_mode: true,
            legato: true,
            glide_ms: 55.0,
            ..Self::default_poly_grid()
        }
    }

    pub fn next_node_id(&self) -> u32 {
        self.nodes.iter().map(|n| n.id).max().unwrap_or(0) + 1
    }

    /// Computes a cycle-safe topological evaluation order of node indices.
    pub fn topo_order(&self) -> Vec<usize> {
        let n = self.nodes.len();
        let mut id_to_idx = HashMap::with_capacity(n);
        for (i, node) in self.nodes.iter().enumerate() {
            id_to_idx.insert(node.id, i);
        }
        let mut indeg = vec![0usize; n];
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
        for c in &self.cables {
            if let (Some(&u), Some(&v)) = (id_to_idx.get(&c.from_node), id_to_idx.get(&c.to_node)) {
                if u != v {
                    adj[u].push(v);
                    indeg[v] += 1;
                }
            }
        }
        let mut queue: Vec<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
        let mut order = Vec::with_capacity(n);
        let mut qi = 0;
        while qi < queue.len() {
            let u = queue[qi];
            qi += 1;
            order.push(u);
            for &v in &adj[u] {
                indeg[v] = indeg[v].saturating_sub(1);
                if indeg[v] == 0 {
                    queue.push(v);
                }
            }
        }
        // Append any remaining nodes involved in feedback cycles
        if order.len() < n {
            for i in 0..n {
                if !order.contains(&i) {
                    order.push(i);
                }
            }
        }
        order
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GEnvStage {
    Attack,
    Decay,
    Sustain,
    Release,
    Idle,
}

#[derive(Debug, Clone)]
struct NodeRuntime {
    phase: f32,
    lfo_phase: f32,
    env_stage: GEnvStage,
    env_level: f32,
    prev_gate: bool,
    s: [f32; 4],
    ic1: f32,
    ic2: f32,
    delay_buf: Vec<f32>,
    delay_pos: usize,
    latch: bool,
    sh_val: f32,
    rng: u32,
    outs: [f32; MAX_GRID_PORTS],
    out_r: f32,
}

impl NodeRuntime {
    fn new(seed: u32) -> Self {
        Self {
            phase: 0.0,
            lfo_phase: 0.0,
            env_stage: GEnvStage::Idle,
            env_level: 0.0,
            prev_gate: false,
            s: [0.0; 4],
            ic1: 0.0,
            ic2: 0.0,
            delay_buf: Vec::new(),
            delay_pos: 0,
            latch: false,
            sh_val: 0.0,
            rng: seed.max(1),
            outs: [0.0; MAX_GRID_PORTS],
            out_r: 0.0,
        }
    }

    fn ensure_delay_buf(&mut self) {
        if self.delay_buf.is_empty() {
            self.delay_buf.resize(DELAY_LINE_LEN, 0.0);
        }
    }

    fn next_rand(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    fn reset(&mut self) {
        self.phase = 0.0;
        self.env_stage = GEnvStage::Idle;
        self.env_level = 0.0;
        self.prev_gate = false;
        self.s = [0.0; 4];
        self.ic1 = 0.0;
        self.ic2 = 0.0;
        self.delay_buf.fill(0.0);
        self.delay_pos = 0;
        self.outs = [0.0; MAX_GRID_PORTS];
        self.out_r = 0.0;
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

#[derive(Debug, Clone)]
struct OpenGridVoice {
    active: bool,
    held: bool,
    track_idx: usize,
    pitch: u8,
    velocity: f32,
    current_midi: f32,
    target_midi: f32,
    voice_phase: f32,
    release_tail_frames: u32,
    age: u64,
    runtimes: Vec<NodeRuntime>,
}

impl OpenGridVoice {
    fn new(voice_idx: usize) -> Self {
        Self {
            active: false,
            held: false,
            track_idx: 0,
            pitch: 60,
            velocity: 1.0,
            current_midi: 60.0,
            target_midi: 60.0,
            voice_phase: 0.0,
            release_tail_frames: 0,
            age: 0,
            runtimes: (0..16)
                .map(|j| NodeRuntime::new(0x9E37_79B9_u32.wrapping_mul((voice_idx * 31 + j + 1) as u32)))
                .collect(),
        }
    }

    fn ensure_nodes(&mut self, count: usize) {
        while self.runtimes.len() < count {
            let seed = 0x9E37_79B9_u32.wrapping_mul((self.runtimes.len() + 7) as u32);
            self.runtimes.push(NodeRuntime::new(seed));
        }
    }
}

struct CompiledPatch {
    patch: Arc<OpenGridPatch>,
    order: Vec<usize>,
    /// For each node index and input port: Vec<(src_node_idx, src_port)>
    inputs_map: Vec<[Vec<(usize, usize)>; MAX_GRID_PORTS]>,
    has_envelope: bool,
}

impl CompiledPatch {
    fn compile(patch: Arc<OpenGridPatch>) -> Self {
        let n = patch.nodes.len();
        let order = patch.topo_order();
        let mut id_to_idx = HashMap::with_capacity(n);
        for (i, node) in patch.nodes.iter().enumerate() {
            id_to_idx.insert(node.id, i);
        }
        let mut inputs_map: Vec<[Vec<(usize, usize)>; MAX_GRID_PORTS]> = (0..n)
            .map(|_| std::array::from_fn(|_| Vec::new()))
            .collect();
        for c in &patch.cables {
            if let (Some(&src), Some(&dst)) = (id_to_idx.get(&c.from_node), id_to_idx.get(&c.to_node)) {
                if c.to_port < MAX_GRID_PORTS && c.from_port < MAX_GRID_PORTS {
                    inputs_map[dst][c.to_port].push((src, c.from_port));
                }
            }
        }
        let has_envelope = patch
            .nodes
            .iter()
            .any(|nd| matches!(nd.kind, GridModuleKind::Adsr | GridModuleKind::Ar | GridModuleKind::Ad));
        Self {
            patch,
            order,
            inputs_map,
            has_envelope,
        }
    }
}

/// Polyphonic modular graph voice pool across up to 16 tracks.
pub struct OpenGridVoicePool {
    voices: Vec<OpenGridVoice>,
    compiled: [Option<CompiledPatch>; 16],
    default_wt: Arc<OpenPolyWavetableData>,
    track_peaks: [(f32, f32); 16],
    track_scopes: Vec<Vec<f32>>,
    scope_pos: [usize; 16],
    global_age: u64,
}

impl Default for OpenGridVoicePool {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenGridVoicePool {
    pub fn new() -> Self {
        Self {
            voices: (0..MAX_OPEN_GRID_VOICES).map(OpenGridVoice::new).collect(),
            compiled: std::array::from_fn(|_| None),
            default_wt: Arc::new(OpenPolyWavetableData::standard_four(2048)),
            track_peaks: [(0.0, 0.0); 16],
            track_scopes: (0..16).map(|_| vec![0.0f32; SCOPE_LEN]).collect(),
            scope_pos: [0; 16],
            global_age: 0,
        }
    }

    pub fn set_track_patch(&mut self, track_idx: usize, patch: Option<Arc<OpenGridPatch>>) {
        let ti = track_idx.min(15);
        match patch {
            Some(p) => {
                self.compiled[ti] = Some(CompiledPatch::compile(p));
            }
            None => {
                self.compiled[ti] = None;
                self.kill_track_notes(ti);
            }
        }
    }

    pub fn track_meter(&self, track_idx: usize) -> (f32, f32) {
        self.track_peaks[track_idx.min(15)]
    }

    pub fn track_scope(&self, track_idx: usize) -> Vec<f32> {
        let ti = track_idx.min(15);
        let pos = self.scope_pos[ti];
        let buf = &self.track_scopes[ti];
        let mut out = Vec::with_capacity(SCOPE_LEN);
        out.extend_from_slice(&buf[pos..]);
        out.extend_from_slice(&buf[..pos]);
        out
    }

    pub fn trigger(&mut self, pitch: u8, velocity: f32, track_idx: usize) {
        let ti = track_idx.min(15);
        let Some(comp) = self.compiled[ti].as_ref() else { return };
        let node_count = comp.patch.nodes.len();
        let mono = comp.patch.mono_mode;
        let legato = comp.patch.legato;
        let glide_ms = comp.patch.glide_ms;

        self.global_age = self.global_age.wrapping_add(1);
        let idx = if mono {
            self.voices
                .iter()
                .position(|v| v.active && v.track_idx == ti)
                .or_else(|| self.voices.iter().position(|v| !v.active))
                .unwrap_or(0)
        } else {
            self.voices
                .iter()
                .position(|v| v.active && v.track_idx == ti && v.pitch == pitch)
                .or_else(|| self.voices.iter().position(|v| !v.active))
                .unwrap_or_else(|| {
                    self.voices
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, v)| v.age)
                        .map(|(i, _)| i)
                        .unwrap_or(0)
                })
        };

        let last_pitch = self
            .voices
            .iter()
            .filter(|v| v.active && v.track_idx == ti)
            .max_by_key(|v| v.age)
            .map(|v| v.current_midi);

        let v = &mut self.voices[idx];
        let is_legato = mono && legato && v.active && v.held;
        v.ensure_nodes(node_count);
        v.active = true;
        v.held = true;
        v.track_idx = ti;
        v.pitch = pitch;
        v.velocity = velocity.clamp(0.01, 1.0);
        v.target_midi = pitch as f32;
        if glide_ms > 0.5 && (is_legato || last_pitch.is_some()) {
            if !is_legato {
                v.current_midi = last_pitch.unwrap_or(pitch as f32);
            }
        } else {
            v.current_midi = pitch as f32;
        }
        v.release_tail_frames = 0;
        if !is_legato {
            v.voice_phase = 0.0;
            for rt in v.runtimes.iter_mut().take(node_count) {
                rt.reset();
            }
        }
        v.age = self.global_age;
    }

    pub fn release_note(&mut self, pitch: u8, track_idx: usize) {
        let ti = track_idx.min(15);
        for v in &mut self.voices {
            if v.active && v.track_idx == ti && v.pitch == pitch {
                v.held = false;
            }
        }
    }

    pub fn kill_note(&mut self, pitch: u8, track_idx: usize) {
        let ti = track_idx.min(15);
        for v in &mut self.voices {
            if v.active && v.track_idx == ti && v.pitch == pitch {
                v.active = false;
                v.held = false;
            }
        }
    }

    pub fn kill_track_notes(&mut self, track_idx: usize) {
        let ti = track_idx.min(15);
        for v in &mut self.voices {
            if v.track_idx == ti {
                v.active = false;
                v.held = false;
            }
        }
        self.track_peaks[ti] = (0.0, 0.0);
    }

    pub fn kill_all(&mut self) {
        for v in &mut self.voices {
            v.active = false;
            v.held = false;
        }
        self.track_peaks = [(0.0, 0.0); 16];
    }

    pub fn active_voice_count(&self, track_idx: usize) -> usize {
        let ti = track_idx.min(15);
        self.voices.iter().filter(|v| v.active && v.track_idx == ti).count()
    }

    #[inline]
    fn eval_voice_sample(
        v: &mut OpenGridVoice,
        comp: &CompiledPatch,
        wt: &OpenPolyWavetableData,
        sr: f32,
    ) -> (f32, f32) {
        let patch = &comp.patch;
        let sr = sr.max(8000.0);
        v.ensure_nodes(patch.nodes.len());

        // Portamento glide
        if patch.glide_ms > 0.5 && (v.current_midi - v.target_midi).abs() > 0.0005 {
            let gs = (patch.glide_ms * 0.001 * sr).max(1.0);
            let coeff = (-4.0 / gs).exp();
            v.current_midi = v.target_midi + (v.current_midi - v.target_midi) * coeff;
        } else {
            v.current_midi = v.target_midi;
        }

        let voice_midi = v.current_midi + patch.global_pitch_st;
        // Standard 1V/oct normalized around C3 (MIDI 60 -> 0.0, +1 octave -> +0.1)
        let voice_pitch_sig = (voice_midi - 60.0) / 120.0;
        let base_freq = 440.0 * 2.0_f32.powf((voice_midi - 69.0) / 12.0);
        let voice_gate = if v.held { 1.0f32 } else { 0.0f32 };
        v.voice_phase = (v.voice_phase + base_freq.clamp(8.0, sr * 0.45) / sr).fract();

        let mut out_l = 0.0f32;
        let mut out_r = 0.0f32;
        let mut any_env_active = false;

        for &ni in &comp.order {
            let node = &patch.nodes[ni];
            let p = node.params;

            // Gather summed inputs from connected cables
            let mut in_val = [0.0f32; MAX_GRID_PORTS];
            let mut in_r = 0.0f32;
            let mut in_connected = [false; MAX_GRID_PORTS];
            for port in 0..MAX_GRID_PORTS {
                let srcs = &comp.inputs_map[ni][port];
                if !srcs.is_empty() {
                    in_connected[port] = true;
                    let mut sum = 0.0f32;
                    for &(src_ni, src_port) in srcs {
                        let s = v.runtimes[src_ni].outs[src_port];
                        sum += s;
                        if port == 0 {
                            in_r += if src_port == 0 { v.runtimes[src_ni].out_r } else { s };
                        }
                    }
                    in_val[port] = sum;
                }
            }

            let rt = &mut v.runtimes[ni];
            if node.bypassed {
                rt.outs[0] = in_val[0];
                rt.out_r = if in_connected[0] { in_r } else { in_val[0] };
                continue;
            }

            match node.kind {
                // ── I/O ──────────────────────────────────────────────────
                GridModuleKind::GateIn => {
                    rt.outs[0] = voice_gate;
                    rt.out_r = voice_gate;
                }
                GridModuleKind::PitchIn => {
                    rt.outs[0] = voice_pitch_sig;
                    rt.out_r = voice_pitch_sig;
                }
                GridModuleKind::VelocityIn => {
                    rt.outs[0] = v.velocity;
                    rt.out_r = v.velocity;
                }
                GridModuleKind::PressureIn => {
                    rt.outs[0] = v.velocity * 0.8;
                    rt.out_r = rt.outs[0];
                }
                GridModuleKind::PhaseIn => {
                    rt.outs[0] = v.voice_phase;
                    rt.out_r = v.voice_phase;
                }
                GridModuleKind::AudioIn => {
                    rt.outs[0] = 0.0;
                    rt.out_r = 0.0;
                }
                GridModuleKind::NoteIn => {
                    rt.outs[0] = voice_pitch_sig;
                    rt.outs[1] = voice_gate;
                    rt.outs[2] = v.velocity;
                    rt.out_r = voice_pitch_sig;
                }
                GridModuleKind::AudioOut => {
                    let g = p[0].clamp(0.0, 2.0);
                    let l = in_val[0] * g;
                    let r = (if in_connected[0] { in_r } else { in_val[0] }) * g;
                    out_l += l;
                    out_r += r;
                    rt.outs[0] = (l + r) * 0.5;
                    rt.out_r = r;
                }
                GridModuleKind::ModulatorOut => {
                    rt.outs[0] = in_val[0];
                    rt.out_r = in_val[0];
                }

                // ── OSCILLATORS ──────────────────────────────────────────
                GridModuleKind::Wavetable
                | GridModuleKind::Sawtooth
                | GridModuleKind::Pulse
                | GridModuleKind::Sine
                | GridModuleKind::Triangle
                | GridModuleKind::Phase1
                | GridModuleKind::Swarm => {
                    let pitch_sig = if in_connected[0] { in_val[0] } else { voice_pitch_sig };
                    let midi = 60.0 + pitch_sig * 120.0 + p[2];
                    let freq = (440.0 * 2.0_f32.powf((midi - 69.0) / 12.0) + p[3]).clamp(8.0, sr * 0.45);
                    let ph = if in_connected[2] {
                        in_val[2].rem_euclid(1.0)
                    } else {
                        let cur = rt.phase;
                        rt.phase = (cur + freq / sr).fract();
                        cur
                    };
                    let idx = (p[0] + in_val[1] * 0.5).clamp(0.0, 1.0);
                    let fold = p[1].clamp(0.0, 1.0);
                    let osc_kind = match node.kind {
                        GridModuleKind::Wavetable => OpenPolyOscType::Wavetable,
                        GridModuleKind::Sawtooth => OpenPolyOscType::Sawtooth,
                        GridModuleKind::Pulse => OpenPolyOscType::Pulse,
                        GridModuleKind::Sine => OpenPolyOscType::Sine,
                        GridModuleKind::Triangle => OpenPolyOscType::Triangle,
                        GridModuleKind::Phase1 => OpenPolyOscType::Phase1,
                        _ => OpenPolyOscType::Swarm,
                    };
                    let s = eval_osc_preview(osc_kind, ph, idx, fold, Some(wt));
                    rt.outs[0] = s;
                    rt.out_r = s;
                }
                GridModuleKind::Sub => {
                    let pitch_sig = if in_connected[0] { in_val[0] } else { voice_pitch_sig };
                    let oct = p[0].round().clamp(-2.0, 0.0);
                    let midi = 60.0 + pitch_sig * 120.0 + oct * 12.0;
                    let freq = (440.0 * 2.0_f32.powf((midi - 69.0) / 12.0)).clamp(12.0, sr * 0.44);
                    let ph = rt.phase;
                    rt.phase = (ph + freq / sr).fract();
                    let w = p[1].round() as i32;
                    let raw = match w {
                        1 => 1.0 - 4.0 * (ph - 0.5).abs(),
                        2 => if ph < 0.5 { 0.85 } else { -0.85 },
                        3 => 2.0 * ph - 1.0,
                        _ => (ph * TAU).sin(),
                    };
                    let s = raw * p[2].clamp(0.0, 1.0);
                    rt.outs[0] = s;
                    rt.out_r = s;
                }
                GridModuleKind::Noise => {
                    let white = rt.next_rand();
                    // Simple low-pass tint controlled by color
                    let alpha = 0.05 + p[0].clamp(0.0, 1.0) * 0.95;
                    rt.ic1 = rt.ic1 + alpha * (white - rt.ic1);
                    let s = rt.ic1 * p[1].clamp(0.0, 1.0);
                    rt.outs[0] = s;
                    rt.out_r = s;
                }

                // ── ENVELOPES ────────────────────────────────────────────
                GridModuleKind::Adsr | GridModuleKind::Ar | GridModuleKind::Ad => {
                    let gate_hi = if in_connected[1] {
                        in_val[1] > 0.25
                    } else {
                        v.held
                    };
                    if gate_hi && !rt.prev_gate {
                        rt.env_stage = GEnvStage::Attack;
                    } else if !gate_hi && rt.prev_gate && node.kind != GridModuleKind::Ad {
                        if rt.env_stage != GEnvStage::Idle {
                            rt.env_stage = GEnvStage::Release;
                        }
                    }
                    rt.prev_gate = gate_hi;

                    let atk_ms = p[0].max(0.4);
                    let dec_ms = p[1].max(0.8);
                    let sust = match node.kind {
                        GridModuleKind::Ar => 1.0,
                        GridModuleKind::Ad => 0.0,
                        _ => p[2].clamp(0.0, 1.0),
                    };
                    let rel_ms = match node.kind {
                        GridModuleKind::Ar => p[1].max(0.8),
                        _ => p[3].max(0.8),
                    };

                    match rt.env_stage {
                        GEnvStage::Attack => {
                            rt.env_level += 1.0 / (atk_ms * 0.001 * sr).max(1.0);
                            if rt.env_level >= 1.0 {
                                rt.env_level = 1.0;
                                rt.env_stage = if node.kind == GridModuleKind::Ar {
                                    GEnvStage::Sustain
                                } else {
                                    GEnvStage::Decay
                                };
                            }
                        }
                        GEnvStage::Decay => {
                            let c = (-4.6 / (dec_ms * 0.001 * sr).max(1.0)).exp();
                            rt.env_level = sust + (rt.env_level - sust) * c;
                            if (rt.env_level - sust).abs() < 0.0005 {
                                rt.env_level = sust;
                                rt.env_stage = if node.kind == GridModuleKind::Ad {
                                    GEnvStage::Idle
                                } else {
                                    GEnvStage::Sustain
                                };
                            }
                        }
                        GEnvStage::Sustain => {
                            rt.env_level = sust;
                        }
                        GEnvStage::Release => {
                            let c = (-5.5 / (rel_ms * 0.001 * sr).max(1.0)).exp();
                            rt.env_level *= c;
                            if rt.env_level < 0.0004 {
                                rt.env_level = 0.0;
                                rt.env_stage = GEnvStage::Idle;
                            }
                        }
                        GEnvStage::Idle => {
                            rt.env_level = 0.0;
                        }
                    }

                    if rt.env_stage != GEnvStage::Idle {
                        any_env_active = true;
                    }
                    let env = rt.env_level;
                    let sig_l = if in_connected[0] { in_val[0] * env } else { env };
                    let sig_r = if in_connected[0] { in_r * env } else { env };
                    rt.outs[0] = sig_l;
                    rt.outs[1] = env;
                    rt.out_r = sig_r;
                }
                GridModuleKind::Follower => {
                    let x = in_val[0].abs();
                    let atk = (-1.0 / (p[0].max(0.5) * 0.001 * sr)).exp();
                    let rel = (-1.0 / (p[1].max(1.0) * 0.001 * sr)).exp();
                    let c = if x > rt.env_level { atk } else { rel };
                    rt.env_level = x + (rt.env_level - x) * c;
                    rt.outs[0] = rt.env_level;
                    rt.out_r = rt.env_level;
                }

                // ── FILTERS ──────────────────────────────────────────────
                GridModuleKind::LowPassMG
                | GridModuleKind::LowPass
                | GridModuleKind::LowPassLD
                | GridModuleKind::HighPass
                | GridModuleKind::Svf
                | GridModuleKind::SallenKey
                | GridModuleKind::Comb
                | GridModuleKind::Xp => {
                    let mod_oct = in_val[1] * p[3] * 5.0
                        + if in_connected[2] { in_val[2] * 10.0 } else { 0.0 };
                    let fc = (p[0] * 2.0_f32.powf(mod_oct)).clamp(20.0, sr * 0.43);
                    let reso = p[1].clamp(0.0, 0.96);
                    let drv = 1.0 + p[2].clamp(0.0, 1.0) * 3.0;
                    let x = fast_tanh(in_val[0] * drv);
                    let y = match node.kind {
                        GridModuleKind::LowPassMG | GridModuleKind::LowPassLD => {
                            let g = (PI * fc / (sr * 2.0)).tan().min(0.85);
                            let gc = g / (1.0 + g);
                            let k = reso * if node.kind == GridModuleKind::LowPassMG { 3.7 } else { 3.3 };
                            let mut out = 0.0;
                            for _ in 0..2 {
                                let u = fast_tanh(x - k * rt.s[3]);
                                let mut sig = u;
                                for st in &mut rt.s {
                                    let dv = (sig - *st) * gc;
                                    let ny = dv + *st;
                                    *st = ny + dv;
                                    sig = ny;
                                }
                                out = sig;
                            }
                            out * (1.0 + reso * 0.4)
                        }
                        GridModuleKind::SallenKey => {
                            let g = (PI * fc / sr).tan().min(1.8);
                            let k = reso * 1.9;
                            let a1 = 1.0 / (1.0 + g * (g + 2.0 - k));
                            let hp = (x - (2.0 - k) * rt.ic1 - rt.ic2) * a1;
                            let bp = fast_tanh(g * hp + rt.ic1);
                            rt.ic1 = g * hp + bp;
                            let lp = g * bp + rt.ic2;
                            rt.ic2 = g * bp + lp;
                            lp
                        }
                        GridModuleKind::Comb => {
                            rt.ensure_delay_buf();
                            let d = (sr / fc).clamp(2.0, (DELAY_LINE_LEN - 2) as f32);
                            let rf = (rt.delay_pos as f32 - d).rem_euclid(DELAY_LINE_LEN as f32);
                            let i0 = rf as usize % DELAY_LINE_LEN;
                            let i1 = (i0 + 1) % DELAY_LINE_LEN;
                            let frac = rf - rf.floor();
                            let del = rt.delay_buf[i0] + frac * (rt.delay_buf[i1] - rt.delay_buf[i0]);
                            let out = fast_tanh(x + del * (0.1 + reso * 0.85));
                            rt.delay_buf[rt.delay_pos] = out;
                            rt.delay_pos = (rt.delay_pos + 1) % DELAY_LINE_LEN;
                            out * 0.72
                        }
                        _ => {
                            let g = (PI * fc / sr).tan().min(4.0);
                            let k = (2.0 - 1.92 * reso).max(0.06);
                            let a1 = 1.0 / (1.0 + g * (g + k));
                            let a2 = g * a1;
                            let a3 = g * a2;
                            let v3 = x - rt.ic2;
                            let v1 = a1 * rt.ic1 + a2 * v3;
                            let v2 = rt.ic2 + a2 * rt.ic1 + a3 * v3;
                            rt.ic1 = 2.0 * v1 - rt.ic1;
                            rt.ic2 = 2.0 * v2 - rt.ic2;
                            match node.kind {
                                GridModuleKind::HighPass => x - k * v1 - v2,
                                GridModuleKind::Xp => fast_tanh((v2 * 0.85 + v1 * 0.35) * 1.2),
                                _ => v2,
                            }
                        }
                    };
                    let y = if y.is_finite() { y.clamp(-4.0, 4.0) } else { 0.0 };
                    rt.outs[0] = y;
                    rt.out_r = y;
                }

                // ── LFOS ─────────────────────────────────────────────────
                GridModuleKind::Lfo | GridModuleKind::WavetableLfo | GridModuleKind::ShLfo => {
                    let rate = p[0].clamp(0.02, 40.0);
                    let prev = rt.lfo_phase;
                    let next = prev + rate / sr;
                    if next >= 1.0 {
                        rt.lfo_phase = next.fract();
                        rt.sh_val = rt.next_rand();
                    } else {
                        rt.lfo_phase = next;
                    }
                    let ph = rt.lfo_phase;
                    let raw = match node.kind {
                        GridModuleKind::ShLfo => rt.sh_val,
                        GridModuleKind::WavetableLfo => wt.sample_interpolated(ph, p[2]),
                        _ => {
                            let sin_v = (ph * TAU).sin();
                            let tri_v = 1.0 - 4.0 * (ph - 0.5).abs();
                            sin_v * (1.0 - p[2]) + tri_v * p[2]
                        }
                    };
                    let y = raw * p[1].clamp(0.0, 1.0);
                    rt.outs[0] = y;
                    rt.out_r = y;
                }

                // ── SHAPERS ──────────────────────────────────────────────
                GridModuleKind::Distortion
                | GridModuleKind::Saturator
                | GridModuleKind::Wavefolder
                | GridModuleKind::HardClip
                | GridModuleKind::Clip
                | GridModuleKind::Rectifier
                | GridModuleKind::Quantizer
                | GridModuleKind::Diode => {
                    let x = in_val[0];
                    let amt = p[0].clamp(0.0, 1.0);
                    let mix = p[1].clamp(0.0, 1.0);
                    let shaped = match node.kind {
                        GridModuleKind::Distortion => fast_tanh(x * (1.0 + amt * 12.0)),
                        GridModuleKind::Saturator => fast_tanh(x * (1.0 + amt * 4.5)) * 0.95,
                        GridModuleKind::Wavefolder => ((x * (1.0 + amt * 5.0)) * std::f32::consts::FRAC_PI_2).sin(),
                        GridModuleKind::HardClip => (x * (1.0 + amt * 8.0)).clamp(-1.0, 1.0),
                        GridModuleKind::Clip => x.clamp(-(1.0 - amt * 0.85), 1.0 - amt * 0.85),
                        GridModuleKind::Rectifier => x.abs(),
                        GridModuleKind::Quantizer => {
                            let steps = 2.0 + (1.0 - amt) * 30.0;
                            (x * steps).round() / steps
                        }
                        GridModuleKind::Diode => {
                            let g = x * (1.0 + amt * 4.0);
                            if g > 0.0 { fast_tanh(g) } else { fast_tanh(g * 0.35) }
                        }
                        _ => x,
                    };
                    let y = x * (1.0 - mix) + shaped * mix;
                    rt.outs[0] = y;
                    rt.out_r = y;
                }

                // ── DELAY / FX ───────────────────────────────────────────
                GridModuleKind::Delay
                | GridModuleKind::ModDelay
                | GridModuleKind::AllPass
                | GridModuleKind::ChorusPlus
                | GridModuleKind::FlangerPlus => {
                    rt.ensure_delay_buf();
                    let x = in_val[0];
                    let base_ms = match node.kind {
                        GridModuleKind::FlangerPlus => (1.0 + p[0] * 0.12).clamp(0.5, 12.0),
                        GridModuleKind::ChorusPlus => (6.0 + p[0] * 0.25).clamp(4.0, 30.0),
                        _ => p[0].clamp(0.5, 80.0),
                    };
                    let lfo_mod = if matches!(
                        node.kind,
                        GridModuleKind::ModDelay | GridModuleKind::ChorusPlus | GridModuleKind::FlangerPlus
                    ) {
                        rt.lfo_phase = (rt.lfo_phase + 0.65 / sr).fract();
                        (rt.lfo_phase * TAU).sin() * 2.5 + in_val[1] * 4.0
                    } else {
                        in_val[1] * 8.0
                    };
                    let del_s = ((base_ms + lfo_mod).max(0.5) * 0.001 * sr)
                        .clamp(1.0, (DELAY_LINE_LEN - 2) as f32);
                    let rf = (rt.delay_pos as f32 - del_s).rem_euclid(DELAY_LINE_LEN as f32);
                    let i0 = rf as usize % DELAY_LINE_LEN;
                    let i1 = (i0 + 1) % DELAY_LINE_LEN;
                    let frac = rf - rf.floor();
                    let delayed = rt.delay_buf[i0] + frac * (rt.delay_buf[i1] - rt.delay_buf[i0]);
                    let fb = p[1].clamp(0.0, 0.92);
                    let mix = p[2].clamp(0.0, 1.0);
                    let y = if node.kind == GridModuleKind::AllPass {
                        let v_ap = x + fb * delayed;
                        rt.delay_buf[rt.delay_pos] = fast_tanh(v_ap);
                        delayed - fb * v_ap
                    } else {
                        rt.delay_buf[rt.delay_pos] = fast_tanh(x + delayed * fb);
                        x * (1.0 - mix) + delayed * mix
                    };
                    rt.delay_pos = (rt.delay_pos + 1) % DELAY_LINE_LEN;
                    rt.outs[0] = y;
                    rt.out_r = if node.kind == GridModuleKind::ChorusPlus {
                        x * (1.0 - mix) - delayed * mix * 0.85
                    } else {
                        y
                    };
                }

                // ── MIX ──────────────────────────────────────────────────
                GridModuleKind::Mixer => {
                    let y = (in_val[0] * p[0] + in_val[1] * p[1] + in_val[2] * p[2]) * 0.5;
                    rt.outs[0] = y;
                    rt.out_r = y;
                }
                GridModuleKind::Blend => {
                    let m = (p[0] + in_val[2] * 0.5).clamp(0.0, 1.0);
                    let y = in_val[0] * (1.0 - m) + in_val[1] * m;
                    rt.outs[0] = y;
                    rt.out_r = y;
                }
                GridModuleKind::Pan => {
                    let pan = (p[0] + in_val[1]).clamp(-1.0, 1.0);
                    let gl = if pan > 0.0 { 1.0 - pan } else { 1.0 };
                    let gr = if pan < 0.0 { 1.0 + pan } else { 1.0 };
                    rt.outs[0] = in_val[0] * gl;
                    rt.out_r = (if in_connected[0] { in_r } else { in_val[0] }) * gr;
                }
                GridModuleKind::StereoWidth => {
                    let l = in_val[0];
                    let r = if in_connected[0] { in_r } else { l };
                    let mid = 0.5 * (l + r);
                    let side = 0.5 * (l - r) * (p[0] * 2.0);
                    rt.outs[0] = mid + side;
                    rt.out_r = mid - side;
                }
                GridModuleKind::StereoSplit => {
                    rt.outs[0] = in_val[0];
                    rt.outs[1] = if in_connected[0] { in_r } else { in_val[0] };
                    rt.out_r = rt.outs[1];
                }

                // ── LEVEL ────────────────────────────────────────────────
                GridModuleKind::Amplify => {
                    let g = p[0].clamp(0.0, 2.0);
                    rt.outs[0] = in_val[0] * g;
                    rt.out_r = (if in_connected[0] { in_r } else { in_val[0] }) * g;
                }
                GridModuleKind::Attenuate => {
                    let g = p[0].clamp(0.0, 1.0);
                    rt.outs[0] = in_val[0] * g;
                    rt.out_r = (if in_connected[0] { in_r } else { in_val[0] }) * g;
                }
                GridModuleKind::GainDb => {
                    let g = 10.0_f32.powf(p[0].clamp(-36.0, 18.0) / 20.0);
                    rt.outs[0] = in_val[0] * g;
                    rt.out_r = (if in_connected[0] { in_r } else { in_val[0] }) * g;
                }
                GridModuleKind::GainVol => {
                    let g = p[0].clamp(0.0, 1.0);
                    let g2 = g * g;
                    rt.outs[0] = in_val[0] * g2;
                    rt.out_r = (if in_connected[0] { in_r } else { in_val[0] }) * g2;
                }
                GridModuleKind::Invert => {
                    rt.outs[0] = -in_val[0];
                    rt.out_r = -(if in_connected[0] { in_r } else { in_val[0] });
                }
                GridModuleKind::Bias => {
                    let y = in_val[0] + p[0];
                    rt.outs[0] = y;
                    rt.out_r = y;
                }
                GridModuleKind::BiToUni => {
                    let y = in_val[0] * 0.5 + 0.5;
                    rt.outs[0] = y;
                    rt.out_r = y;
                }
                GridModuleKind::UniToBi => {
                    let y = in_val[0] * 2.0 - 1.0;
                    rt.outs[0] = y;
                    rt.out_r = y;
                }

                // ── PITCH ────────────────────────────────────────────────
                GridModuleKind::Transpose | GridModuleKind::BySemitone => {
                    let src = if in_connected[0] { in_val[0] } else { voice_pitch_sig };
                    let y = src + p[0] / 120.0;
                    rt.outs[0] = y;
                    rt.out_r = y;
                }
                GridModuleKind::Octaver => {
                    let src = if in_connected[0] { in_val[0] } else { voice_pitch_sig };
                    let y = src + p[0].round() * 0.1;
                    rt.outs[0] = y;
                    rt.out_r = y;
                }
                GridModuleKind::Pitch => {
                    let y = p[0] / 120.0;
                    rt.outs[0] = y;
                    rt.out_r = y;
                }
                GridModuleKind::PitchQuantize => {
                    let src = if in_connected[0] { in_val[0] } else { voice_pitch_sig };
                    let y = (src * 120.0).round() / 120.0;
                    rt.outs[0] = y;
                    rt.out_r = y;
                }

                // ── MATH ─────────────────────────────────────────────────
                GridModuleKind::Add | GridModuleKind::Sum => {
                    let y = in_val[0] + in_val[1];
                    rt.outs[0] = y;
                    rt.out_r = y;
                }
                GridModuleKind::Subtract => {
                    let y = in_val[0] - in_val[1];
                    rt.outs[0] = y;
                    rt.out_r = y;
                }
                GridModuleKind::Multiply | GridModuleKind::Product => {
                    let y = in_val[0] * in_val[1];
                    rt.outs[0] = y;
                    rt.out_r = y;
                }
                GridModuleKind::Divide => {
                    let d = if in_val[1].abs() < 1.0e-4 { 1.0 } else { in_val[1] };
                    let y = (in_val[0] / d).clamp(-16.0, 16.0);
                    rt.outs[0] = y;
                    rt.out_r = y;
                }
                GridModuleKind::Average => {
                    let y = 0.5 * (in_val[0] + in_val[1]);
                    rt.outs[0] = y;
                    rt.out_r = y;
                }
                GridModuleKind::Abs => {
                    let y = in_val[0].abs();
                    rt.outs[0] = y;
                    rt.out_r = y;
                }
                GridModuleKind::MinMax => {
                    rt.outs[0] = in_val[0].min(in_val[1]);
                    rt.outs[1] = in_val[0].max(in_val[1]);
                    rt.out_r = rt.outs[1];
                }
                GridModuleKind::Constant | GridModuleKind::Value => {
                    rt.outs[0] = p[0];
                    rt.out_r = p[0];
                }
                GridModuleKind::Round => {
                    let y = in_val[0].round();
                    rt.outs[0] = y;
                    rt.out_r = y;
                }

                // ── LOGIC ────────────────────────────────────────────────
                GridModuleKind::And => {
                    let y = if in_val[0] > 0.5 && in_val[1] > 0.5 { 1.0 } else { 0.0 };
                    rt.outs[0] = y;
                    rt.out_r = y;
                }
                GridModuleKind::Or => {
                    let y = if in_val[0] > 0.5 || in_val[1] > 0.5 { 1.0 } else { 0.0 };
                    rt.outs[0] = y;
                    rt.out_r = y;
                }
                GridModuleKind::Xor => {
                    let a = in_val[0] > 0.5;
                    let b = in_val[1] > 0.5;
                    let y = if a ^ b { 1.0 } else { 0.0 };
                    rt.outs[0] = y;
                    rt.out_r = y;
                }
                GridModuleKind::Not => {
                    let y = if in_val[0] > 0.5 { 0.0 } else { 1.0 };
                    rt.outs[0] = y;
                    rt.out_r = y;
                }
                GridModuleKind::Nand => {
                    let y = if !(in_val[0] > 0.5 && in_val[1] > 0.5) { 1.0 } else { 0.0 };
                    rt.outs[0] = y;
                    rt.out_r = y;
                }
                GridModuleKind::Nor => {
                    let y = if !(in_val[0] > 0.5 || in_val[1] > 0.5) { 1.0 } else { 0.0 };
                    rt.outs[0] = y;
                    rt.out_r = y;
                }
                GridModuleKind::Latch => {
                    let trig = in_val[0] > 0.5;
                    if trig && !rt.prev_gate {
                        rt.latch = !rt.latch;
                    }
                    if in_val[1] > 0.5 {
                        rt.latch = false;
                    }
                    rt.prev_gate = trig;
                    let y = if rt.latch { 1.0 } else { 0.0 };
                    rt.outs[0] = y;
                    rt.out_r = y;
                }

                // ── PHASE ────────────────────────────────────────────────
                GridModuleKind::Phasor => {
                    let rate = p[0].clamp(0.05, 40.0);
                    rt.phase = (rt.phase + rate / sr).fract();
                    rt.outs[0] = rt.phase;
                    rt.out_r = rt.phase;
                }
                GridModuleKind::PhaseReverse => {
                    let ph = if in_connected[0] { in_val[0] } else { v.voice_phase };
                    let y = (1.0 - ph.rem_euclid(1.0)).rem_euclid(1.0);
                    rt.outs[0] = y;
                    rt.out_r = y;
                }
                GridModuleKind::PhaseShift => {
                    let ph = if in_connected[0] { in_val[0] } else { v.voice_phase };
                    let y = (ph + p[0] + in_val[1] * 0.5).rem_euclid(1.0);
                    rt.outs[0] = y;
                    rt.out_r = y;
                }
                GridModuleKind::PhaseBend => {
                    let ph = (if in_connected[0] { in_val[0] } else { v.voice_phase }).rem_euclid(1.0);
                    let bend = (0.1 + (p[0] + in_val[1] * 0.4).clamp(0.0, 1.0) * 0.8).clamp(0.05, 0.95);
                    let y = if ph < bend {
                        0.5 * ph / bend
                    } else {
                        0.5 + 0.5 * (ph - bend) / (1.0 - bend)
                    };
                    rt.outs[0] = y;
                    rt.out_r = y;
                }
                GridModuleKind::PhasePinch => {
                    let ph = (if in_connected[0] { in_val[0] } else { v.voice_phase }).rem_euclid(1.0);
                    let exp_k = 0.25 + p[0].clamp(0.0, 1.0) * 3.5;
                    let y = ph.powf(exp_k);
                    rt.outs[0] = y;
                    rt.out_r = y;
                }

                // ── RANDOM ───────────────────────────────────────────────
                GridModuleKind::Dice => {
                    let trig = if in_connected[0] { in_val[0] > 0.5 } else { v.held };
                    if trig && !rt.prev_gate {
                        rt.sh_val = rt.next_rand() * 0.5 + 0.5;
                    }
                    rt.prev_gate = trig;
                    rt.outs[0] = rt.sh_val;
                    rt.out_r = rt.sh_val;
                }
                GridModuleKind::Chance => {
                    let trig = if in_connected[0] { in_val[0] > 0.5 } else { v.held };
                    if trig && !rt.prev_gate {
                        let r = rt.next_rand() * 0.5 + 0.5;
                        rt.latch = r <= p[0].clamp(0.0, 1.0);
                    }
                    rt.prev_gate = trig;
                    let y = if trig && rt.latch { 1.0 } else { 0.0 };
                    rt.outs[0] = y;
                    rt.out_r = y;
                }
                GridModuleKind::SampleHold => {
                    let trig = in_val[1] > 0.5;
                    if trig && !rt.prev_gate {
                        rt.sh_val = in_val[0];
                    }
                    rt.prev_gate = trig;
                    rt.outs[0] = rt.sh_val;
                    rt.out_r = rt.sh_val;
                }

                // ── DATA SEQUENCERS ──────────────────────────────────────
                GridModuleKind::Steps | GridModuleKind::Triggers | GridModuleKind::Pitches => {
                    let rate = p[0].clamp(0.25, 32.0);
                    let steps = p[1].round().clamp(2.0, 8.0) as usize;
                    rt.phase = (rt.phase + rate / sr) % steps as f32;
                    let idx = (rt.phase as usize).min(steps - 1);
                    const PATTERN: [f32; 8] = [0.0, 0.3, 0.6, 0.4, 0.85, 0.5, 0.95, 0.2];
                    const SCALE_ST: [f32; 8] = [0.0, 2.0, 3.0, 7.0, 10.0, 12.0, 7.0, 5.0];
                    let y = match node.kind {
                        GridModuleKind::Steps => PATTERN[idx] * p[2].clamp(0.0, 1.0),
                        GridModuleKind::Triggers => {
                            let frac = rt.phase - rt.phase.floor();
                            if frac < 0.35 && (idx % 2 == 0 || p[2] > 0.6) { 1.0 } else { 0.0 }
                        }
                        GridModuleKind::Pitches => (SCALE_ST[idx] * p[2].clamp(0.0, 1.0)) / 120.0,
                        _ => 0.0,
                    };
                    rt.outs[0] = y;
                    rt.out_r = y;
                }

                // ── DISPLAY (pass-through + telemetry) ───────────────────
                GridModuleKind::Oscilloscope | GridModuleKind::VuMeter | GridModuleKind::ValueReadout => {
                    rt.outs[0] = in_val[0];
                    rt.out_r = if in_connected[0] { in_r } else { in_val[0] };
                }
            }
        }

        // Voice auto-free logic
        if comp.has_envelope {
            if !v.held && !any_env_active {
                v.active = false;
            }
        } else if !v.held {
            v.release_tail_frames += 1;
            if v.release_tail_frames > 256 {
                v.active = false;
            }
        }

        let gain = patch.output_gain.clamp(0.0, 2.0) * 0.42;
        (
            (out_l * gain).clamp(-2.5, 2.5),
            (out_r * gain).clamp(-2.5, 2.5),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn process_track(
        &mut self,
        track_idx: usize,
        out_interleaved: &mut [f32],
        gain_l: f32,
        gain_r: f32,
        frames: usize,
        sr: f32,
    ) {
        let ti = track_idx.min(15);
        let Some(comp) = self.compiled[ti].as_ref() else {
            self.track_peaks[ti].0 *= 0.75;
            self.track_peaks[ti].1 *= 0.75;
            return;
        };
        let has_active = self.voices.iter().any(|v| v.active && v.track_idx == ti);
        if !has_active {
            self.track_peaks[ti].0 *= 0.75;
            self.track_peaks[ti].1 *= 0.75;
            return;
        }

        let mut peak_l = 0.0f32;
        let mut peak_r = 0.0f32;
        let wt = &self.default_wt;
        let scope = &mut self.track_scopes[ti];
        let mut spos = self.scope_pos[ti];

        for f in 0..frames {
            let mut sum_l = 0.0f32;
            let mut sum_r = 0.0f32;
            for v in &mut self.voices {
                if v.active && v.track_idx == ti {
                    let (vl, vr) = Self::eval_voice_sample(v, comp, wt, sr);
                    sum_l += vl;
                    sum_r += vr;
                }
            }
            peak_l = peak_l.max(sum_l.abs());
            peak_r = peak_r.max(sum_r.abs());
            if f % 4 == 0 {
                scope[spos] = (sum_l + sum_r) * 0.5;
                spos = (spos + 1) % SCOPE_LEN;
            }
            out_interleaved[f * 2] += sum_l * gain_l;
            out_interleaved[f * 2 + 1] += sum_r * gain_r;
        }
        self.scope_pos[ti] = spos;
        self.track_peaks[ti] = (
            peak_l.max(self.track_peaks[ti].0 * 0.8),
            peak_r.max(self.track_peaks[ti].1 * 0.8),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn process_all_idle(
        &mut self,
        enabled: &[bool; 16],
        out_l: &mut [f32],
        out_r: &mut [f32],
        gain_l: &[f32; 16],
        gain_r: &[f32; 16],
        sr: f32,
    ) {
        let frames = out_l.len().min(out_r.len());
        let mut peaks = [(0.0f32, 0.0f32); 16];
        let mut active_tracks = [false; 16];
        for v in &self.voices {
            if v.active && enabled[v.track_idx] && self.compiled[v.track_idx].is_some() {
                active_tracks[v.track_idx] = true;
            }
        }
        let wt = &self.default_wt;

        for f in 0..frames {
            for v in &mut self.voices {
                if v.active && enabled[v.track_idx] {
                    let ti = v.track_idx;
                    if let Some(comp) = self.compiled[ti].as_ref() {
                        let (vl, vr) = Self::eval_voice_sample(v, comp, wt, sr);
                        peaks[ti].0 = peaks[ti].0.max(vl.abs());
                        peaks[ti].1 = peaks[ti].1.max(vr.abs());
                        if f % 4 == 0 {
                            let sp = self.scope_pos[ti];
                            self.track_scopes[ti][sp] = (vl + vr) * 0.5;
                            self.scope_pos[ti] = (sp + 1) % SCOPE_LEN;
                        }
                        out_l[f] += vl * gain_l[ti];
                        out_r[f] += vr * gain_r[ti];
                    }
                }
            }
        }
        for ti in 0..16 {
            if active_tracks[ti] {
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
    fn default_poly_grid_patch_produces_audio_and_releases_cleanly() {
        let mut pool = OpenGridVoicePool::new();
        pool.set_track_patch(0, Some(Arc::new(OpenGridPatch::default_poly_grid())));
        pool.trigger(60, 0.9, 0);
        assert_eq!(pool.active_voice_count(0), 1);

        let mut buf = vec![0.0f32; 2048 * 2];
        pool.process_track(0, &mut buf, 1.0, 1.0, 2048, 48000.0);
        let peak = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.05, "Default Poly Grid patch should emit audio (peak={peak})");

        pool.release_note(60, 0);
        let mut tail = vec![0.0f32; 16384 * 2];
        pool.process_track(0, &mut tail, 1.0, 1.0, 16384, 48000.0);
        assert_eq!(pool.active_voice_count(0), 0, "Voice must finish after ADSR release");
    }

    #[test]
    fn all_97_grid_modules_compile_and_execute_finitely() {
        assert_eq!(GridModuleKind::ALL.len(), 97);
        for &kind in &GridModuleKind::ALL {
            assert_eq!(GridModuleKind::from_name(kind.display_name()), Some(kind));

            let mut patch = OpenGridPatch {
                nodes: vec![
                    GridNode::new(1, GridModuleKind::Sawtooth, [0.0, 0.0]),
                    GridNode::new(2, kind, [200.0, 0.0]),
                    GridNode::new(3, GridModuleKind::AudioOut, [400.0, 0.0]),
                ],
                cables: Vec::new(),
                ..OpenGridPatch::default_poly_grid()
            };
            if !kind.inputs().is_empty() {
                patch.cables.push(GridCable { from_node: 1, from_port: 0, to_node: 2, to_port: 0 });
            }
            if !kind.outputs().is_empty() {
                patch.cables.push(GridCable { from_node: 2, from_port: 0, to_node: 3, to_port: 0 });
            } else if kind != GridModuleKind::AudioOut {
                patch.cables.push(GridCable { from_node: 1, from_port: 0, to_node: 3, to_port: 0 });
            }

            let mut pool = OpenGridVoicePool::new();
            pool.set_track_patch(0, Some(Arc::new(patch)));
            pool.trigger(60, 0.85, 0);
            let mut buf = vec![0.0f32; 512 * 2];
            pool.process_track(0, &mut buf, 1.0, 1.0, 512, 48000.0);
            assert!(
                buf.iter().all(|s| s.is_finite()),
                "Grid module {} produced NaN/Inf",
                kind.display_name()
            );
        }
    }

    #[test]
    fn factory_presets_all_produce_audible_signal() {
        let presets = [
            OpenGridPatch::default_poly_grid(),
            OpenGridPatch::preset_subtractive_mg(),
            OpenGridPatch::preset_swarm_chorus(),
            OpenGridPatch::preset_acid_wavefolder(),
        ];
        for (idx, preset) in presets.into_iter().enumerate() {
            let mut pool = OpenGridVoicePool::new();
            pool.set_track_patch(0, Some(Arc::new(preset)));
            pool.trigger(57, 0.9, 0);
            let mut buf = vec![0.0f32; 2048 * 2];
            pool.process_track(0, &mut buf, 1.0, 1.0, 2048, 48000.0);
            let peak = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            assert!(peak > 0.02, "Factory OpenGrid preset #{idx} was silent (peak={peak})");
        }
    }
}
