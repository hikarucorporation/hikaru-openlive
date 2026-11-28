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
    Random,
    SampleAndHold,
    Vibrato,
    WavetableLfo,
    Macro,
    Macro4,
    Button,
    Buttons,
    Xy,
    Vector4,
    Vector8,
    Globals,
    Math,
    Mix,
    Quantize,
    Select4,
    Polynom,
    Steps,
    ParSeq8,
    Curves,
    Ramp,
    Segments,
    Adsr,
    Ahdsr,
    FourStage,
    AhdOnRelease,
    Expressions,
    Midi,
    KeytrackPlus,
    RelativeKeytrack,
    Pitch12,
    Channel16,
    NoteCounter,
    NoteSidechain,
    VoiceControl,
    StackSpread,
    EnvelopeFollower,
    AudioSidechain,
    AudioRate,
    HwCvIn,
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
            "random" => Some(Self::Random),
            "sample and hold" | "sample & hold" | "s&h" | "sampleandhold" => {
                Some(Self::SampleAndHold)
            }
            "vibrato" => Some(Self::Vibrato),
            "wavetable lfo" | "wavetablelfo" | "wt lfo" => Some(Self::WavetableLfo),
            "macro" => Some(Self::Macro),
            "macro-4" | "macro4" | "macro 4" => Some(Self::Macro4),
            "button" => Some(Self::Button),
            "buttons" | "buttons-4" | "buttons4" => Some(Self::Buttons),
            "xy" | "x-y" | "xy pad" => Some(Self::Xy),
            "vector-4" | "vector4" | "vector 4" => Some(Self::Vector4),
            "vector-8" | "vector8" | "vector 8" => Some(Self::Vector8),
            "globals" => Some(Self::Globals),
            "math" => Some(Self::Math),
            "mix" => Some(Self::Mix),
            "quantize" => Some(Self::Quantize),
            "select-4" | "select4" | "select 4" => Some(Self::Select4),
            "polynom" | "polynomial" => Some(Self::Polynom),
            "steps" => Some(Self::Steps),
            "parseq-8" | "parseq8" | "parseq 8" => Some(Self::ParSeq8),
            "curves" => Some(Self::Curves),
            "ramp" => Some(Self::Ramp),
            "segments" => Some(Self::Segments),
            "adsr" => Some(Self::Adsr),
            "ahdsr" => Some(Self::Ahdsr),
            "4-stage" | "4stage" | "four-stage" => Some(Self::FourStage),
            "ahd on release" | "ahdonrelease" => Some(Self::AhdOnRelease),
            "expressions" => Some(Self::Expressions),
            "midi" => Some(Self::Midi),
            "keytrack+" | "keytrack plus" | "keytrackplus" => Some(Self::KeytrackPlus),
            "relative keytrack" | "relativekeytrack" => Some(Self::RelativeKeytrack),
            "pitch-12" | "pitch12" | "pitch 12" => Some(Self::Pitch12),
            "channel-16" | "channel16" | "channel 16" => Some(Self::Channel16),
            "note counter" | "notecounter" => Some(Self::NoteCounter),
            "note sidechain" | "notesidechain" => Some(Self::NoteSidechain),
            "voice control" | "voicecontrol" => Some(Self::VoiceControl),
            "stack spread" | "stackspread" => Some(Self::StackSpread),
            "envelope follower" | "envelopefollower" => Some(Self::EnvelopeFollower),
            "audio sidechain" | "audiosidechain" => Some(Self::AudioSidechain),
            "audio rate" | "audiorate" => Some(Self::AudioRate),
            "hw cv in" | "hwcvin" | "cv in" => Some(Self::HwCvIn),
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
        kind: ModulatorKind::Random,
        name: "Random",
        short_name: "Random",
        category: ModulatorCategory::Lfo,
        description: "Stepped or smoothly interpolated random LFO with Free Hz / Tempo Sync, Smooth, Depth, and Bipolar/Unipolar output.",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::SampleAndHold,
        name: "Sample and Hold",
        short_name: "S&H",
        category: ModulatorCategory::Lfo,
        description: "Clocked Sample & Hold modulator sampling Noise, Sine, Triangle, or Saw with optional slew smoothing.",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::Vibrato,
        name: "Vibrato",
        short_name: "Vibrato",
        category: ModulatorCategory::Lfo,
        description: "Expressive vibrato LFO with Rate, Depth, Fade-in delay on note-on, and Mod Wheel scaling.",
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
        kind: ModulatorKind::Vector4,
        name: "Vector-4",
        short_name: "Vec-4",
        category: ModulatorCategory::Interface,
        description: "2D vector pad distributing bilinear weights across 4 independent corner outputs (A, B, C, D).",
        outputs: &["A", "B", "C", "D"],
    },
    ModulatorMeta {
        kind: ModulatorKind::Vector8,
        name: "Vector-8",
        short_name: "Vec-8",
        category: ModulatorCategory::Interface,
        description: "3D vector cube (X, Y, Z) distributing trilinear weights across 8 independent corner outputs.",
        outputs: &["1", "2", "3", "4", "5", "6", "7", "8"],
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
    ModulatorMeta {
        kind: ModulatorKind::Steps,
        name: "Steps",
        short_name: "Steps",
        category: ModulatorCategory::Sequence,
        description: "Up to 16-step modulation sequencer with editable bars, Free Hz / Tempo Sync, smoothing, and bipolar mode.",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::ParSeq8,
        name: "ParSeq-8",
        short_name: "ParSeq8",
        category: ModulatorCategory::Sequence,
        description: "8-step parallel sequencer with 8 independent per-step outputs firing sequentially on the clock.",
        outputs: &["1", "2", "3", "4", "5", "6", "7", "8"],
    },
    ModulatorMeta {
        kind: ModulatorKind::Curves,
        name: "Curves",
        short_name: "Curves",
        category: ModulatorCategory::Sequence,
        description: "Looping beat-synced 8-node curve modulator with smooth cosine or linear interpolation.",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::Ramp,
        name: "Ramp",
        short_name: "Ramp",
        category: ModulatorCategory::Envelope,
        description: "One-shot or looping curved ramp envelope with adjustable duration, curve tension, and up/down direction.",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::Segments,
        name: "Segments",
        short_name: "Segs",
        category: ModulatorCategory::Envelope,
        description: "Multi-segment breakpoint envelope (MSEG) with 6 editable nodes, curve tension, and loop/one-shot mode.",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::Adsr,
        name: "ADSR",
        short_name: "ADSR",
        category: ModulatorCategory::Envelope,
        description: "Classic Attack-Decay-Sustain-Release envelope triggered by note events or manual gate.",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::Ahdsr,
        name: "AHDSR",
        short_name: "AHDSR",
        category: ModulatorCategory::Envelope,
        description: "Attack-Hold-Decay-Sustain-Release envelope with peak hold stage.",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::FourStage,
        name: "4-Stage",
        short_name: "4-Stage",
        category: ModulatorCategory::Envelope,
        description: "Flexible 4-stage time/level envelope with optional looping and sustain stage.",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::AhdOnRelease,
        name: "AHD on Release",
        short_name: "AHD Rel",
        category: ModulatorCategory::Envelope,
        description: "Attack-Hold-Decay envelope that fires upon Note-Off / release events.",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::Expressions,
        name: "Expressions",
        short_name: "Expr",
        category: ModulatorCategory::NoteDriven,
        description: "Per-note expression outputs for Velocity (VEL), Timbre (TIMB), Release Velocity (REL), and Pressure (PRES).",
        outputs: &["VEL", "TMB", "REL", "PRS"],
    },
    ModulatorMeta {
        kind: ModulatorKind::Midi,
        name: "MIDI",
        short_name: "MIDI",
        category: ModulatorCategory::NoteDriven,
        description: "Maps MIDI CC, Mod Wheel, Pitch Bend, and Channel Aftertouch to four independent modulation outputs.",
        outputs: &["CC", "MW", "PB", "AT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::KeytrackPlus,
        name: "Keytrack+",
        short_name: "Keytrk+",
        category: ModulatorCategory::NoteDriven,
        description: "Curved keyboard pitch tracking around a configurable root note and semitone spread.",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::RelativeKeytrack,
        name: "Relative Keytrack",
        short_name: "RelKey",
        category: ModulatorCategory::NoteDriven,
        description: "Outputs the relative semitone interval between the current note and the previous note.",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::Pitch12,
        name: "Pitch-12",
        short_name: "Pitch12",
        category: ModulatorCategory::NoteDriven,
        description: "12-output pitch-class modulator routing each chromatic note (C..B) to its own modulation output.",
        outputs: &["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"],
    },
    ModulatorMeta {
        kind: ModulatorKind::NoteCounter,
        name: "Note Counter",
        short_name: "NoteCnt",
        category: ModulatorCategory::NoteDriven,
        description: "Steps through N values on every incoming note-on event (Up, Down, or Ping-Pong).",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::NoteSidechain,
        name: "Note Sidechain",
        short_name: "NoteSC",
        category: ModulatorCategory::NoteDriven,
        description: "Attack/Release sidechain envelope triggered by note events within an optional pitch range.",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::VoiceControl,
        name: "Voice Control",
        short_name: "Voice",
        category: ModulatorCategory::VoiceStacking,
        description: "Exposes polyphonic voice Index, Active Count, Age, and Velocity as 4 independent outputs.",
        outputs: &["IDX", "CNT", "AGE", "VEL"],
    },
    ModulatorMeta {
        kind: ModulatorKind::StackSpread,
        name: "Stack Spread",
        short_name: "Spread",
        category: ModulatorCategory::VoiceStacking,
        description: "Distributes modulation spread across stacked voices with Linear, Center, or Harmonic curves.",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::WavetableLfo,
        name: "Wavetable LFO",
        short_name: "WT LFO",
        category: ModulatorCategory::Lfo,
        description: "Morphable wavetable LFO scanning continuously across harmonic frames with Free Hz / Tempo Sync.",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::Channel16,
        name: "Channel-16",
        short_name: "Ch-16",
        category: ModulatorCategory::NoteDriven,
        description: "16-channel MIDI routing modulator exposing independent modulation outputs for channels 1..16.",
        outputs: &["1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16"],
    },
    ModulatorMeta {
        kind: ModulatorKind::Globals,
        name: "Globals",
        short_name: "Globals",
        category: ModulatorCategory::Interface,
        description: "Global transport modulator outputting normalized Tempo (BPM), Bar phase, Beat phase, and Play state.",
        outputs: &["BPM", "BAR", "BET", "PLY"],
    },
    ModulatorMeta {
        kind: ModulatorKind::EnvelopeFollower,
        name: "Envelope Follower",
        short_name: "EnvFol",
        category: ModulatorCategory::AudioDriven,
        description: "Audio-driven amplitude envelope follower with Gain, Attack, and Release smoothing.",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::AudioSidechain,
        name: "Audio Sidechain",
        short_name: "AudSC",
        category: ModulatorCategory::AudioDriven,
        description: "Sidechain audio envelope follower with source track selector, Threshold, Gain, and Duck/Invert mode.",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::AudioRate,
        name: "Audio Rate",
        short_name: "AudRate",
        category: ModulatorCategory::AudioDriven,
        description: "High-frequency audio-rate modulation source with selectable waveform, frequency, gain, and rectify.",
        outputs: &["OUT"],
    },
    ModulatorMeta {
        kind: ModulatorKind::HwCvIn,
        name: "HW CV In",
        short_name: "CV In",
        category: ModulatorCategory::AudioDriven,
        description: "Hardware DC-coupled CV input modulator with Channel selector, Gain, Offset, and Bipolar/Unipolar range.",
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

    // Random & Sample-and-Hold
    pub rand_smooth: f32,
    pub sh_source: u8, // 0=Noise, 1=Sine, 2=Triangle, 3=Saw
    pub sh_slew: f32,

    // Steps (16-step sequencer)
    pub steps_values: [f32; 16],
    pub steps_len: u8,
    pub steps_smooth: f32,
    pub steps_bipolar: bool,
    pub steps_last_idx: usize,

    // ParSeq-8 (8-step parallel sequencer)
    pub parseq_steps: [f32; 8],
    pub parseq_len: u8,
    pub parseq_smooth: f32,
    pub parseq_last_step: usize,

    // Curves (8-node looping curve)
    pub curves_nodes: [f32; 8],
    pub curves_smooth: bool,
    pub curves_bipolar: bool,

    // Ramp & Segments
    pub ramp_time_secs: f32,
    pub ramp_curve: f32,
    pub ramp_loop: bool,
    pub ramp_down: bool,
    pub ramp_bipolar: bool,
    pub ramp_start_time: f32,

    pub seg_levels: [f32; 6],
    pub seg_curve: f32,
    pub seg_loop: bool,
    pub seg_time_secs: f32,
    pub seg_start_time: f32,
    pub seg_bipolar: bool,

    // Vector-8 Z axis (reuses xy_x, xy_y for X/Y)
    pub vec8_z: f32,

    // Envelopes (ADSR, AHDSR, 4-Stage, AHD on Release, Note Sidechain)
    pub env_attack: f32,
    pub env_hold: f32,
    pub env_decay: f32,
    pub env_sustain: f32,
    pub env_release: f32,
    pub env_gate: bool,
    pub env_gate_on_time: f32,
    pub env_gate_off_time: f32,
    pub env_release_level: f32,
    pub env_loop: bool,
    pub env_4stage_times: [f32; 4],
    pub env_4stage_levels: [f32; 4],

    // Expressions (VEL, TIMB, REL, PRES)
    pub expr_vel: f32,
    pub expr_timbre: f32,
    pub expr_rel: f32,
    pub expr_pres: f32,

    // MIDI (CC, MW, PB, AT)
    pub midi_cc_num: u8,
    pub midi_cc_val: f32,
    pub midi_mw: f32,
    pub midi_pb: f32,
    pub midi_at: f32,

    // Keytrack+ / Relative Keytrack / Pitch-12
    pub key_note: u8,
    pub key_prev_note: u8,
    pub key_root: u8,
    pub key_spread_semi: f32,
    pub key_curve: f32,

    // Note Counter
    pub nc_steps: u8,
    pub nc_count: u32,
    pub nc_mode: u8, // 0=Up, 1=Down, 2=PingPong

    // Note Sidechain
    pub nsc_min_note: u8,
    pub nsc_max_note: u8,

    // Vibrato
    pub vib_rate_hz: f32,
    pub vib_depth: f32,
    pub vib_fade_secs: f32,
    pub vib_mw_scale: f32,

    // Voice Control & Stack Spread
    pub voice_index: u8,
    pub voice_count: u8,
    pub voice_age_secs: f32,
    pub spread_amount: f32,
    pub spread_mode: u8, // 0=Linear, 1=Center, 2=Harmonic

    // Wavetable LFO
    pub wt_index: f32,

    // Channel-16
    pub ch16_levels: [f32; 16],
    pub ch16_active: u8,
    pub ch16_exclusive: bool,

    // Audio-driven (Envelope Follower, Audio Sidechain, Audio Rate, HW CV In)
    pub audio_input_level: f32,
    pub audio_env_current: f32,
    pub audio_gain: f32,
    pub audio_threshold: f32,
    pub audio_sc_track: u8,
    pub audio_sc_invert: bool,
    pub ar_freq_hz: f32,
    pub ar_wave: u8, // 0=Sine, 1=Tri, 2=Saw, 3=AudioLive
    pub ar_rectify: bool,
    pub cv_channel: u8,
    pub cv_offset: f32,
    pub cv_level: f32,
    pub cv_bipolar: bool,

    // Rutas de modulación
    pub routes: Vec<BayModRoute>,
    pub last_outputs: [f32; 16],
}

impl ModulatorSlotState {
    pub fn new(id: u32, kind: ModulatorKind) -> Self {
        let default_sync = matches!(
            kind,
            ModulatorKind::BeatLfo
                | ModulatorKind::Steps
                | ModulatorKind::ParSeq8
                | ModulatorKind::Curves
        );
        Self {
            id,
            kind,
            enabled: true,
            lfo_waveform: LfoWaveform::Sine,
            lfo_sync_mode: if default_sync {
                LfoSyncMode::TempoSync
            } else {
                LfoSyncMode::FreeHz
            },
            lfo_rate_hz: 1.0,
            lfo_sync_rate: if matches!(kind, ModulatorKind::Steps | ModulatorKind::ParSeq8) {
                SyncRate::Eighth
            } else {
                SyncRate::Quarter
            },
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
            rand_smooth: 0.35,
            sh_source: 0,
            sh_slew: 0.0,
            steps_values: [
                0.85, 0.30, 0.65, 0.95, 0.25, 0.70, 0.45, 0.55,
                0.80, 0.20, 0.60, 0.90, 0.35, 0.75, 0.40, 0.50,
            ],
            steps_len: 8,
            steps_smooth: 0.0,
            steps_bipolar: false,
            steps_last_idx: 0,
            parseq_steps: [1.0; 8],
            parseq_len: 8,
            parseq_smooth: 0.0,
            parseq_last_step: 0,
            curves_nodes: [0.0, 0.85, 0.35, 1.0, 0.55, 0.90, 0.20, 0.45],
            curves_smooth: true,
            curves_bipolar: false,
            ramp_time_secs: 2.0,
            ramp_curve: 0.0,
            ramp_loop: true,
            ramp_down: false,
            ramp_bipolar: false,
            ramp_start_time: 0.0,
            seg_levels: [0.0, 1.0, 0.65, 0.85, 0.25, 0.0],
            seg_curve: 0.0,
            seg_loop: true,
            seg_time_secs: 2.0,
            seg_start_time: 0.0,
            seg_bipolar: false,
            vec8_z: 0.5,
            env_attack: 0.05,
            env_hold: 0.10,
            env_decay: 0.35,
            env_sustain: 0.65,
            env_release: 0.40,
            env_gate: false,
            env_gate_on_time: -10.0,
            env_gate_off_time: -10.0,
            env_release_level: 0.0,
            env_loop: false,
            env_4stage_times: [0.10, 0.25, 0.35, 0.40],
            env_4stage_levels: [1.0, 0.55, 0.80, 0.0],
            expr_vel: 0.8,
            expr_timbre: 0.5,
            expr_rel: 0.0,
            expr_pres: 0.0,
            midi_cc_num: 74,
            midi_cc_val: 0.5,
            midi_mw: 0.5,
            midi_pb: 0.0,
            midi_at: 0.0,
            key_note: 60,
            key_prev_note: 60,
            key_root: 60,
            key_spread_semi: 24.0,
            key_curve: 0.0,
            nc_steps: 8,
            nc_count: 0,
            nc_mode: 0,
            nsc_min_note: 0,
            nsc_max_note: 127,
            vib_rate_hz: 5.5,
            vib_depth: 0.75,
            vib_fade_secs: 0.30,
            vib_mw_scale: 1.0,
            voice_index: 0,
            voice_count: 4,
            voice_age_secs: 0.0,
            spread_amount: 1.0,
            spread_mode: 0,
            wt_index: 0.35,
            ch16_levels: {
                let mut arr = [0.0_f32; 16];
                arr[0] = 1.0;
                arr
            },
            ch16_active: 0,
            ch16_exclusive: true,
            audio_input_level: 0.0,
            audio_env_current: 0.0,
            audio_gain: 1.0,
            audio_threshold: 0.0,
            audio_sc_track: 0,
            audio_sc_invert: false,
            ar_freq_hz: 110.0,
            ar_wave: 0,
            ar_rectify: false,
            cv_channel: 1,
            cv_offset: 0.0,
            cv_level: 0.0,
            cv_bipolar: true,
            routes: Vec::new(),
            last_outputs: [0.0; 16],
        }
    }

    /// Evalúa una tabla de ondas morfeable de 4 frames (Sine -> Formant -> Saw -> Fold).
    pub fn eval_wavetable_lfo(phase: f32, wt_index: f32) -> f32 {
        let p = phase.rem_euclid(1.0);
        let a = p * std::f32::consts::TAU;
        let f0 = a.sin();
        let f1 = (a.sin() + 0.5 * (2.0 * a).sin() + 0.25 * (3.0 * a).sin()) / 1.5;
        let f2 = 2.0 * p - 1.0;
        let f3 = (a.sin() * 2.2).sin();
        let frames = [f0, f1, f2, f3];
        let pos = wt_index.clamp(0.0, 1.0) * 3.0;
        let i0 = (pos.floor() as usize).min(2);
        let frac = (pos - i0 as f32).clamp(0.0, 1.0);
        (frames[i0] * (1.0 - frac) + frames[i0 + 1] * frac).clamp(-1.0, 1.0)
    }

    pub fn trigger_gate(&mut self, on: bool, time_secs: f32) {
        if on {
            self.env_gate = true;
            self.env_gate_on_time = time_secs;
            if self.kind == ModulatorKind::NoteCounter {
                self.nc_count = self.nc_count.wrapping_add(1);
            }
        } else if self.env_gate {
            let cur = self.last_outputs[0].clamp(0.0, 1.0);
            self.env_release_level = cur;
            self.env_gate = false;
            self.env_gate_off_time = time_secs;
        }
    }

    pub fn eval_adsr_like(&self, time_secs: f32, with_hold: bool) -> f32 {
        let a = self.env_attack.max(0.002);
        let h = if with_hold { self.env_hold.max(0.0) } else { 0.0 };
        let d = self.env_decay.max(0.002);
        let s = self.env_sustain.clamp(0.0, 1.0);
        let r = self.env_release.max(0.002);
        if self.env_gate {
            let dt = (time_secs - self.env_gate_on_time).max(0.0);
            if dt < a {
                (dt / a).clamp(0.0, 1.0)
            } else if dt < a + h {
                1.0
            } else if dt < a + h + d {
                let fd = ((dt - a - h) / d).clamp(0.0, 1.0);
                1.0 - (1.0 - s) * fd
            } else {
                s
            }
        } else {
            let dt = (time_secs - self.env_gate_off_time).max(0.0);
            if dt >= r {
                0.0
            } else {
                (self.env_release_level * (1.0 - dt / r)).clamp(0.0, 1.0)
            }
        }
    }

    pub fn eval_four_stage(&self, time_secs: f32) -> f32 {
        let t0 = self.env_4stage_times[0].max(0.005);
        let t1 = self.env_4stage_times[1].max(0.005);
        let t2 = self.env_4stage_times[2].max(0.005);
        let t3 = self.env_4stage_times[3].max(0.005);
        let total = t0 + t1 + t2 + t3;
        let dt_raw = (time_secs - self.env_gate_on_time).max(0.0);
        if !self.env_gate && !self.env_loop {
            let dt_off = (time_secs - self.env_gate_off_time).max(0.0);
            if dt_off >= t3 {
                return self.env_4stage_levels[3].clamp(0.0, 1.0);
            }
            let f = (dt_off / t3).clamp(0.0, 1.0);
            return (self.env_release_level
                + (self.env_4stage_levels[3] - self.env_release_level) * f)
                .clamp(0.0, 1.0);
        }
        let dt = if self.env_loop {
            dt_raw % total
        } else {
            dt_raw.min(t0 + t1 + t2)
        };
        if dt < t0 {
            let f = dt / t0;
            (self.env_4stage_levels[0] * f).clamp(0.0, 1.0)
        } else if dt < t0 + t1 {
            let f = (dt - t0) / t1;
            (self.env_4stage_levels[0]
                + (self.env_4stage_levels[1] - self.env_4stage_levels[0]) * f)
                .clamp(0.0, 1.0)
        } else if dt < t0 + t1 + t2 {
            let f = (dt - t0 - t1) / t2;
            (self.env_4stage_levels[1]
                + (self.env_4stage_levels[2] - self.env_4stage_levels[1]) * f)
                .clamp(0.0, 1.0)
        } else {
            let f = ((dt - t0 - t1 - t2) / t3).clamp(0.0, 1.0);
            (self.env_4stage_levels[2]
                + (self.env_4stage_levels[3] - self.env_4stage_levels[2]) * f)
                .clamp(0.0, 1.0)
        }
    }

    pub fn eval_ahd_release(&self, time_secs: f32) -> f32 {
        if self.env_gate || self.env_gate_off_time < 0.0 {
            return 0.0;
        }
        let a = self.env_attack.max(0.002);
        let h = self.env_hold.max(0.0);
        let d = self.env_decay.max(0.002);
        let dt = (time_secs - self.env_gate_off_time).max(0.0);
        if dt < a {
            (dt / a).clamp(0.0, 1.0)
        } else if dt < a + h {
            1.0
        } else if dt < a + h + d {
            (1.0 - (dt - a - h) / d).clamp(0.0, 1.0)
        } else {
            0.0
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

    pub fn compute_clock_cycles(
        &self,
        transport: &LfoTransportState,
        time_secs: f32,
        force_sync: bool,
    ) -> (u64, f32) {
        let phase_offset = (self.lfo_phase_deg / 360.0).rem_euclid(1.0);
        let use_sync = force_sync || self.lfo_sync_mode == LfoSyncMode::TempoSync;
        let total_cycles = if use_sync {
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
        } else {
            let hz = self.lfo_rate_hz.clamp(0.02, 30.0) as f64;
            time_secs as f64 * hz + phase_offset as f64
        };
        let cycle_idx = total_cycles.floor().max(0.0) as u64;
        let phase = (total_cycles.fract() as f32).rem_euclid(1.0);
        (cycle_idx, phase)
    }

    pub fn eval_curve_at(&self, phase: f32) -> f32 {
        let p = phase.rem_euclid(1.0) * 8.0;
        let i0 = (p.floor() as usize) % 8;
        let i1 = (i0 + 1) % 8;
        let f = p.fract();
        let mu = if self.curves_smooth {
            0.5 - 0.5 * (f * std::f32::consts::PI).cos()
        } else {
            f
        };
        (self.curves_nodes[i0] + (self.curves_nodes[i1] - self.curves_nodes[i0]) * mu)
            .clamp(0.0, 1.0)
    }

    pub fn eval_segments_at(&self, t: f32) -> f32 {
        let pos = t.clamp(0.0, 1.0) * 5.0;
        let i0 = (pos.floor() as usize).min(4);
        let f = (pos - i0 as f32).clamp(0.0, 1.0);
        let exp = 2.0_f32.powf(-self.seg_curve.clamp(-1.0, 1.0) * 1.5);
        let wf = f.powf(exp);
        (self.seg_levels[i0] + (self.seg_levels[i0 + 1] - self.seg_levels[i0]) * wf)
            .clamp(0.0, 1.0)
    }

    pub fn eval_ramp_at(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        let exp = 2.0_f32.powf(-self.ramp_curve.clamp(-1.0, 1.0) * 2.0);
        let shaped = t.powf(exp);
        if self.ramp_down {
            1.0 - shaped
        } else {
            shaped
        }
    }

    /// Evalúa las salidas en vivo del modulador (rango normalizado -1.0..+1.0 o 0.0..1.0).
    pub fn evaluate(&mut self, transport: &LfoTransportState, time_secs: f32) -> [f32; 16] {
        if !self.enabled {
            self.last_outputs = [0.0; 16];
            return self.last_outputs;
        }
        let mut outs = [0.0_f32; 16];
        match self.kind {
            ModulatorKind::ClassicLfo => {
                let (cycle_idx, phase) = self.compute_clock_cycles(transport, time_secs, false);
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
                let (cycle_idx, raw_phase) = self.compute_clock_cycles(transport, time_secs, true);
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
            ModulatorKind::Random => {
                let (cycle_idx, phase) = self.compute_clock_cycles(transport, time_secs, false);
                self.lfo_last_phase = phase;
                let seed = self.id as u64 * 173;
                let cur = pseudo_random_step(cycle_idx.wrapping_add(seed));
                let prev = pseudo_random_step(cycle_idx.saturating_sub(1).wrapping_add(seed));
                let raw = if self.rand_smooth > 0.01 {
                    let t = (phase / self.rand_smooth.clamp(0.01, 1.0)).clamp(0.0, 1.0);
                    let mu = 0.5 - 0.5 * (t * std::f32::consts::PI).cos();
                    prev + (cur - prev) * mu
                } else {
                    cur
                };
                let shaped = if self.lfo_bipolar {
                    raw
                } else {
                    (raw + 1.0) * 0.5
                };
                outs[0] = (shaped * self.lfo_depth.clamp(0.0, 1.0)).clamp(-1.0, 1.0);
            }
            ModulatorKind::SampleAndHold => {
                let (cycle_idx, phase) = self.compute_clock_cycles(transport, time_secs, false);
                self.lfo_last_phase = phase;
                let sample_at = |c: u64| -> f32 {
                    match self.sh_source {
                        1 => ((c as f32 * 0.618034).fract() * std::f32::consts::TAU).sin(),
                        2 => {
                            let p = (c as f32 * 0.381966).fract();
                            evaluate_waveform(LfoWaveform::Triangle, p, 0.0)
                        }
                        3 => {
                            let p = (c as f32 * 0.271828).fract();
                            evaluate_waveform(LfoWaveform::SawUp, p, 0.0)
                        }
                        _ => pseudo_random_step(c.wrapping_add(self.id as u64 * 251)),
                    }
                };
                let cur = sample_at(cycle_idx);
                let prev = sample_at(cycle_idx.saturating_sub(1));
                let raw = if self.sh_slew > 0.01 {
                    let t = (phase / self.sh_slew.clamp(0.01, 1.0)).clamp(0.0, 1.0);
                    prev + (cur - prev) * t
                } else {
                    cur
                };
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
            ModulatorKind::Vector4 => {
                let x = self.xy_x.clamp(0.0, 1.0);
                let y = self.xy_y.clamp(0.0, 1.0);
                outs[0] = (1.0 - x) * y;
                outs[1] = x * y;
                outs[2] = (1.0 - x) * (1.0 - y);
                outs[3] = x * (1.0 - y);
            }
            ModulatorKind::Vector8 => {
                let x = self.xy_x.clamp(0.0, 1.0);
                let y = self.xy_y.clamp(0.0, 1.0);
                let z = self.vec8_z.clamp(0.0, 1.0);
                let fz = 1.0 - z;
                let a = (1.0 - x) * y;
                let b = x * y;
                let c = (1.0 - x) * (1.0 - y);
                let d = x * (1.0 - y);
                outs[0] = a * fz;
                outs[1] = b * fz;
                outs[2] = c * fz;
                outs[3] = d * fz;
                outs[4] = a * z;
                outs[5] = b * z;
                outs[6] = c * z;
                outs[7] = d * z;
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
            ModulatorKind::Steps => {
                let (cycle_idx, phase) = self.compute_clock_cycles(transport, time_secs, false);
                let len = (self.steps_len.clamp(2, 16)) as usize;
                let idx = (cycle_idx as usize) % len;
                let prev_idx = (idx + len - 1) % len;
                self.steps_last_idx = idx;
                self.lfo_last_phase = phase;
                let cur = self.steps_values[idx].clamp(0.0, 1.0);
                let prev = self.steps_values[prev_idx].clamp(0.0, 1.0);
                let v = if self.steps_smooth > 0.01 {
                    let t = (phase / self.steps_smooth.clamp(0.01, 1.0)).clamp(0.0, 1.0);
                    prev + (cur - prev) * t
                } else {
                    cur
                };
                outs[0] = if self.steps_bipolar { v * 2.0 - 1.0 } else { v };
            }
            ModulatorKind::ParSeq8 => {
                let (cycle_idx, phase) = self.compute_clock_cycles(transport, time_secs, false);
                let len = (self.parseq_len.clamp(2, 8)) as usize;
                let active = (cycle_idx as usize) % len;
                let next = (active + 1) % len;
                self.parseq_last_step = active;
                self.lfo_last_phase = phase;
                if self.parseq_smooth > 0.01 {
                    let s = self.parseq_smooth.clamp(0.01, 1.0);
                    let fade_start = 1.0 - s;
                    if phase >= fade_start {
                        let f = ((phase - fade_start) / s).clamp(0.0, 1.0);
                        outs[active] = self.parseq_steps[active].clamp(0.0, 1.0) * (1.0 - f);
                        outs[next] = self.parseq_steps[next].clamp(0.0, 1.0) * f;
                    } else {
                        outs[active] = self.parseq_steps[active].clamp(0.0, 1.0);
                    }
                } else {
                    outs[active] = self.parseq_steps[active].clamp(0.0, 1.0);
                }
            }
            ModulatorKind::Curves => {
                let (_cycle_idx, phase) = self.compute_clock_cycles(transport, time_secs, false);
                self.lfo_last_phase = phase;
                let v = self.eval_curve_at(phase);
                let shaped = if self.curves_bipolar {
                    v * 2.0 - 1.0
                } else {
                    v
                };
                outs[0] = (shaped * self.lfo_depth.clamp(0.0, 1.0)).clamp(-1.0, 1.0);
            }
            ModulatorKind::Ramp => {
                let dur = self.ramp_time_secs.clamp(0.05, 16.0);
                let elapsed = (time_secs - self.ramp_start_time).max(0.0);
                let t = if self.ramp_loop {
                    (elapsed / dur).fract()
                } else {
                    (elapsed / dur).min(1.0)
                };
                self.lfo_last_phase = t;
                let v = self.eval_ramp_at(t);
                outs[0] = if self.ramp_bipolar { v * 2.0 - 1.0 } else { v };
            }
            ModulatorKind::Segments => {
                let dur = self.seg_time_secs.clamp(0.05, 16.0);
                let elapsed = (time_secs - self.seg_start_time).max(0.0);
                let t = if self.seg_loop {
                    (elapsed / dur).fract()
                } else {
                    (elapsed / dur).min(1.0)
                };
                self.lfo_last_phase = t;
                let v = self.eval_segments_at(t);
                outs[0] = if self.seg_bipolar { v * 2.0 - 1.0 } else { v };
            }
            ModulatorKind::Adsr => {
                outs[0] = self.eval_adsr_like(time_secs, false);
            }
            ModulatorKind::Ahdsr => {
                outs[0] = self.eval_adsr_like(time_secs, true);
            }
            ModulatorKind::FourStage => {
                outs[0] = self.eval_four_stage(time_secs);
            }
            ModulatorKind::AhdOnRelease => {
                outs[0] = self.eval_ahd_release(time_secs);
            }
            ModulatorKind::Expressions => {
                outs[0] = self.expr_vel.clamp(0.0, 1.0);
                outs[1] = self.expr_timbre.clamp(0.0, 1.0);
                outs[2] = self.expr_rel.clamp(0.0, 1.0);
                outs[3] = self.expr_pres.clamp(0.0, 1.0);
            }
            ModulatorKind::Midi => {
                outs[0] = self.midi_cc_val.clamp(0.0, 1.0);
                outs[1] = self.midi_mw.clamp(0.0, 1.0);
                outs[2] = self.midi_pb.clamp(-1.0, 1.0);
                outs[3] = self.midi_at.clamp(0.0, 1.0);
            }
            ModulatorKind::KeytrackPlus => {
                let spread = self.key_spread_semi.max(1.0);
                let raw = ((self.key_note as f32 - self.key_root as f32) / spread).clamp(-1.0, 1.0);
                let exp = 2.0_f32.powf(-self.key_curve.clamp(-1.0, 1.0) * 1.5);
                outs[0] = raw.signum() * raw.abs().powf(exp);
            }
            ModulatorKind::RelativeKeytrack => {
                let spread = self.key_spread_semi.max(1.0);
                let diff = self.key_note as f32 - self.key_prev_note as f32;
                outs[0] = (diff / spread).clamp(-1.0, 1.0);
            }
            ModulatorKind::Pitch12 => {
                let pc = (self.key_note % 12) as usize;
                outs[pc] = 1.0;
            }
            ModulatorKind::NoteCounter => {
                let steps = self.nc_steps.clamp(2, 16) as u32;
                let raw_step = match self.nc_mode {
                    1 => (steps - 1) - (self.nc_count % steps),
                    2 => {
                        let period = (steps - 1) * 2;
                        let p = self.nc_count % period.max(1);
                        if p < steps {
                            p
                        } else {
                            period - p
                        }
                    }
                    _ => self.nc_count % steps,
                };
                outs[0] = raw_step as f32 / (steps - 1).max(1) as f32;
            }
            ModulatorKind::NoteSidechain => {
                if self.key_note >= self.nsc_min_note && self.key_note <= self.nsc_max_note {
                    outs[0] = self.eval_adsr_like(time_secs, false);
                } else {
                    outs[0] = 0.0;
                }
            }
            ModulatorKind::Vibrato => {
                let hz = self.vib_rate_hz.clamp(0.1, 20.0);
                let phase = (time_secs * hz).fract();
                self.lfo_last_phase = phase;
                let sine = (phase * std::f32::consts::TAU).sin();
                let fade = if self.vib_fade_secs > 0.005 && self.env_gate_on_time >= 0.0 {
                    ((time_secs - self.env_gate_on_time).max(0.0) / self.vib_fade_secs)
                        .clamp(0.0, 1.0)
                } else {
                    1.0
                };
                let scale = self.vib_depth.clamp(0.0, 1.0) * fade * self.vib_mw_scale.clamp(0.0, 1.0);
                outs[0] = (sine * scale).clamp(-1.0, 1.0);
            }
            ModulatorKind::VoiceControl => {
                let cnt = self.voice_count.clamp(1, 16) as f32;
                let idx = (self.voice_index as f32).min(cnt - 1.0);
                outs[0] = if cnt > 1.0 { idx / (cnt - 1.0) } else { 0.0 };
                outs[1] = (cnt / 16.0).clamp(0.0, 1.0);
                outs[2] = (self.voice_age_secs / 4.0).clamp(0.0, 1.0);
                outs[3] = self.expr_vel.clamp(0.0, 1.0);
            }
            ModulatorKind::StackSpread => {
                let cnt = self.voice_count.clamp(1, 16) as f32;
                let idx = (self.voice_index as f32).min(cnt - 1.0);
                let norm = if cnt > 1.0 { idx / (cnt - 1.0) } else { 0.5 };
                let base = match self.spread_mode {
                    1 => 1.0 - (norm * 2.0 - 1.0).abs(), // Center peak
                    2 => ((idx + 1.0) / cnt).clamp(0.0, 1.0), // Harmonic
                    _ => norm * 2.0 - 1.0, // Linear bipolar -1..+1
                };
                outs[0] = (base * self.spread_amount.clamp(-1.0, 1.0)).clamp(-1.0, 1.0);
            }
            ModulatorKind::WavetableLfo => {
                let (_cycle_idx, phase) = self.compute_clock_cycles(transport, time_secs, false);
                self.lfo_last_phase = phase;
                let raw = Self::eval_wavetable_lfo(phase, self.wt_index);
                let shaped = if self.lfo_bipolar {
                    raw
                } else {
                    (raw + 1.0) * 0.5
                };
                outs[0] = (shaped * self.lfo_depth.clamp(0.0, 1.0)).clamp(-1.0, 1.0);
            }
            ModulatorKind::Channel16 => {
                if self.ch16_exclusive {
                    let ch = (self.ch16_active as usize).min(15);
                    outs[ch] = self.ch16_levels[ch].clamp(0.0, 1.0);
                } else {
                    for i in 0..16 {
                        outs[i] = self.ch16_levels[i].clamp(0.0, 1.0);
                    }
                }
            }
            ModulatorKind::Globals => {
                let bpm_norm = ((transport.bpm - 40.0) / 200.0).clamp(0.0, 1.0);
                let beats = if transport.is_playing {
                    let elapsed = transport
                        .sample_count
                        .saturating_sub(transport.playback_start_sample);
                    let secs = elapsed as f64 / transport.sample_rate.max(1.0) as f64;
                    secs * (transport.bpm.max(20.0) as f64 / 60.0)
                } else {
                    time_secs as f64 * (transport.bpm.max(20.0) as f64 / 60.0)
                };
                let beat_phase = (beats.fract() as f32).rem_euclid(1.0);
                let bar_phase = ((beats / 4.0).fract() as f32).rem_euclid(1.0);
                outs[0] = bpm_norm;
                outs[1] = bar_phase;
                outs[2] = beat_phase;
                outs[3] = if transport.is_playing { 1.0 } else { 0.0 };
            }
            ModulatorKind::EnvelopeFollower => {
                let target = (self.audio_input_level * self.audio_gain).clamp(0.0, 1.0);
                let coeff = if target > self.audio_env_current {
                    (0.02 / self.env_attack.max(0.002)).clamp(0.05, 1.0)
                } else {
                    (0.02 / self.env_release.max(0.002)).clamp(0.02, 1.0)
                };
                self.audio_env_current += (target - self.audio_env_current) * coeff;
                outs[0] = self.audio_env_current.clamp(0.0, 1.0);
            }
            ModulatorKind::AudioSidechain => {
                let above = (self.audio_input_level - self.audio_threshold).max(0.0);
                let target = (above * self.audio_gain).clamp(0.0, 1.0);
                let coeff = if target > self.audio_env_current {
                    (0.02 / self.env_attack.max(0.002)).clamp(0.05, 1.0)
                } else {
                    (0.02 / self.env_release.max(0.002)).clamp(0.02, 1.0)
                };
                self.audio_env_current += (target - self.audio_env_current) * coeff;
                let env = self.audio_env_current.clamp(0.0, 1.0);
                outs[0] = if self.audio_sc_invert { -env } else { env };
            }
            ModulatorKind::AudioRate => {
                let hz = self.ar_freq_hz.clamp(20.0, 2000.0);
                let phase = (time_secs * hz).fract();
                self.lfo_last_phase = phase;
                let raw = match self.ar_wave {
                    1 => evaluate_waveform(LfoWaveform::Triangle, phase, 0.0),
                    2 => evaluate_waveform(LfoWaveform::SawUp, phase, 0.0),
                    3 => self.audio_input_level * (phase * std::f32::consts::TAU).sin(),
                    _ => (phase * std::f32::consts::TAU).sin(),
                };
                let gained = (raw * self.audio_gain).clamp(-1.0, 1.0);
                outs[0] = if self.ar_rectify { gained.abs() } else { gained };
            }
            ModulatorKind::HwCvIn => {
                let raw = self.cv_level * self.audio_gain + self.cv_offset;
                outs[0] = if self.cv_bipolar {
                    raw.clamp(-1.0, 1.0)
                } else {
                    raw.clamp(0.0, 1.0)
                };
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
            ModulatorKind::Random => {
                let sync_idx = SyncRate::ALL
                    .iter()
                    .position(|&r| r == self.lfo_sync_rate)
                    .unwrap_or(4) as f32;
                (
                    vec![self.lfo_rate_hz, self.lfo_depth, self.rand_smooth, sync_idx],
                    vec![
                        self.lfo_sync_mode == LfoSyncMode::TempoSync,
                        self.lfo_bipolar,
                    ],
                    0,
                )
            }
            ModulatorKind::SampleAndHold => {
                let sync_idx = SyncRate::ALL
                    .iter()
                    .position(|&r| r == self.lfo_sync_rate)
                    .unwrap_or(4) as f32;
                (
                    vec![self.lfo_rate_hz, self.lfo_depth, self.sh_slew, sync_idx],
                    vec![
                        self.lfo_sync_mode == LfoSyncMode::TempoSync,
                        self.lfo_bipolar,
                    ],
                    self.sh_source,
                )
            }
            ModulatorKind::Vector4 => (vec![self.xy_x, self.xy_y], vec![], 0),
            ModulatorKind::Vector8 => (vec![self.xy_x, self.xy_y, self.vec8_z], vec![], 0),
            ModulatorKind::Steps => {
                let sync_idx = SyncRate::ALL
                    .iter()
                    .position(|&r| r == self.lfo_sync_rate)
                    .unwrap_or(5) as f32;
                let mut params = vec![self.lfo_rate_hz, self.steps_smooth, sync_idx];
                params.extend_from_slice(&self.steps_values);
                (
                    params,
                    vec![
                        self.lfo_sync_mode == LfoSyncMode::TempoSync,
                        self.steps_bipolar,
                    ],
                    self.steps_len.clamp(2, 16),
                )
            }
            ModulatorKind::ParSeq8 => {
                let sync_idx = SyncRate::ALL
                    .iter()
                    .position(|&r| r == self.lfo_sync_rate)
                    .unwrap_or(5) as f32;
                let mut params = vec![self.lfo_rate_hz, self.parseq_smooth, sync_idx];
                params.extend_from_slice(&self.parseq_steps);
                (
                    params,
                    vec![self.lfo_sync_mode == LfoSyncMode::TempoSync],
                    self.parseq_len.clamp(2, 8),
                )
            }
            ModulatorKind::Curves => {
                let sync_idx = SyncRate::ALL
                    .iter()
                    .position(|&r| r == self.lfo_sync_rate)
                    .unwrap_or(4) as f32;
                let mut params = vec![self.lfo_rate_hz, self.lfo_depth, sync_idx];
                params.extend_from_slice(&self.curves_nodes);
                (
                    params,
                    vec![
                        self.lfo_sync_mode == LfoSyncMode::TempoSync,
                        self.curves_smooth,
                        self.curves_bipolar,
                    ],
                    0,
                )
            }
            ModulatorKind::Ramp => (
                vec![self.ramp_time_secs, self.ramp_curve],
                vec![self.ramp_loop, self.ramp_down, self.ramp_bipolar],
                0,
            ),
            ModulatorKind::Segments => {
                let mut params = vec![self.seg_time_secs, self.seg_curve];
                params.extend_from_slice(&self.seg_levels);
                (params, vec![self.seg_loop, self.seg_bipolar], 0)
            }
            ModulatorKind::Adsr => (
                vec![self.env_attack, self.env_decay, self.env_sustain, self.env_release],
                vec![self.env_gate],
                0,
            ),
            ModulatorKind::Ahdsr => (
                vec![
                    self.env_attack,
                    self.env_hold,
                    self.env_decay,
                    self.env_sustain,
                    self.env_release,
                ],
                vec![self.env_gate],
                0,
            ),
            ModulatorKind::FourStage => {
                let mut params = self.env_4stage_times.to_vec();
                params.extend_from_slice(&self.env_4stage_levels);
                (params, vec![self.env_loop, self.env_gate], 0)
            }
            ModulatorKind::AhdOnRelease => (
                vec![self.env_attack, self.env_hold, self.env_decay],
                vec![],
                0,
            ),
            ModulatorKind::Expressions => (
                vec![self.expr_vel, self.expr_timbre, self.expr_rel, self.expr_pres],
                vec![],
                0,
            ),
            ModulatorKind::Midi => (
                vec![self.midi_cc_val, self.midi_mw, self.midi_pb, self.midi_at],
                vec![],
                self.midi_cc_num,
            ),
            ModulatorKind::KeytrackPlus => (
                vec![self.key_spread_semi, self.key_curve, self.key_note as f32],
                vec![],
                self.key_root,
            ),
            ModulatorKind::RelativeKeytrack => (
                vec![self.key_spread_semi, self.key_note as f32, self.key_prev_note as f32],
                vec![],
                0,
            ),
            ModulatorKind::Pitch12 => (vec![], vec![], self.key_note),
            ModulatorKind::NoteCounter => (
                vec![self.nc_count as f32],
                vec![],
                (self.nc_steps.clamp(2, 16) << 2) | (self.nc_mode & 0x03),
            ),
            ModulatorKind::NoteSidechain => (
                vec![
                    self.env_attack,
                    self.env_release,
                    self.nsc_min_note as f32,
                    self.nsc_max_note as f32,
                ],
                vec![],
                0,
            ),
            ModulatorKind::Vibrato => (
                vec![
                    self.vib_rate_hz,
                    self.vib_depth,
                    self.vib_fade_secs,
                    self.vib_mw_scale,
                ],
                vec![],
                0,
            ),
            ModulatorKind::VoiceControl => (
                vec![
                    self.voice_index as f32,
                    self.voice_count as f32,
                    self.voice_age_secs,
                    self.expr_vel,
                ],
                vec![],
                0,
            ),
            ModulatorKind::StackSpread => (
                vec![
                    self.spread_amount,
                    self.voice_index as f32,
                    self.voice_count as f32,
                ],
                vec![],
                self.spread_mode,
            ),
            ModulatorKind::WavetableLfo => {
                let sync_idx = SyncRate::ALL
                    .iter()
                    .position(|&r| r == self.lfo_sync_rate)
                    .unwrap_or(4) as f32;
                (
                    vec![self.lfo_rate_hz, self.lfo_depth, self.wt_index, sync_idx],
                    vec![
                        self.lfo_sync_mode == LfoSyncMode::TempoSync,
                        self.lfo_bipolar,
                    ],
                    0,
                )
            }
            ModulatorKind::Channel16 => (
                self.ch16_levels.to_vec(),
                vec![self.ch16_exclusive],
                self.ch16_active,
            ),
            ModulatorKind::Globals => (vec![], vec![], 0),
            ModulatorKind::EnvelopeFollower => (
                vec![self.audio_gain, self.env_attack, self.env_release, self.audio_input_level],
                vec![],
                0,
            ),
            ModulatorKind::AudioSidechain => (
                vec![
                    self.audio_gain,
                    self.audio_threshold,
                    self.env_attack,
                    self.env_release,
                    self.audio_input_level,
                ],
                vec![self.audio_sc_invert],
                self.audio_sc_track,
            ),
            ModulatorKind::AudioRate => (
                vec![self.ar_freq_hz, self.audio_gain],
                vec![self.ar_rectify],
                self.ar_wave,
            ),
            ModulatorKind::HwCvIn => (
                vec![self.audio_gain, self.cv_offset, self.cv_level],
                vec![self.cv_bipolar],
                self.cv_channel,
            ),
        };
        let routes = self
            .routes
            .iter()
            .filter(|r| r.target != ModulationTarget::None && r.amount.abs() >= 0.005)
            .map(|r| ModulatorRouteParams {
                out_idx: r.out_idx,
                target: r.target.save_key(),
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
            ModulatorKind::Random => {
                if let Some(&hz) = p.params.first() {
                    st.lfo_rate_hz = hz.clamp(0.02, 30.0);
                }
                if let Some(&d) = p.params.get(1) {
                    st.lfo_depth = d.clamp(0.0, 1.0);
                }
                if let Some(&sm) = p.params.get(2) {
                    st.rand_smooth = sm.clamp(0.0, 1.0);
                }
                if let Some(&sr) = p.params.get(3) {
                    st.lfo_sync_rate = SyncRate::ALL
                        .get(sr.round().max(0.0) as usize)
                        .copied()
                        .unwrap_or(SyncRate::Quarter);
                }
                if let Some(&sync) = p.bool_flags.first() {
                    st.lfo_sync_mode = if sync {
                        LfoSyncMode::TempoSync
                    } else {
                        LfoSyncMode::FreeHz
                    };
                }
                if let Some(&bip) = p.bool_flags.get(1) {
                    st.lfo_bipolar = bip;
                }
            }
            ModulatorKind::SampleAndHold => {
                st.sh_source = p.mode_idx.min(3);
                if let Some(&hz) = p.params.first() {
                    st.lfo_rate_hz = hz.clamp(0.02, 30.0);
                }
                if let Some(&d) = p.params.get(1) {
                    st.lfo_depth = d.clamp(0.0, 1.0);
                }
                if let Some(&sl) = p.params.get(2) {
                    st.sh_slew = sl.clamp(0.0, 1.0);
                }
                if let Some(&sr) = p.params.get(3) {
                    st.lfo_sync_rate = SyncRate::ALL
                        .get(sr.round().max(0.0) as usize)
                        .copied()
                        .unwrap_or(SyncRate::Quarter);
                }
                if let Some(&sync) = p.bool_flags.first() {
                    st.lfo_sync_mode = if sync {
                        LfoSyncMode::TempoSync
                    } else {
                        LfoSyncMode::FreeHz
                    };
                }
                if let Some(&bip) = p.bool_flags.get(1) {
                    st.lfo_bipolar = bip;
                }
            }
            ModulatorKind::Vector4 => {
                if let Some(&x) = p.params.first() {
                    st.xy_x = x.clamp(0.0, 1.0);
                }
                if let Some(&y) = p.params.get(1) {
                    st.xy_y = y.clamp(0.0, 1.0);
                }
            }
            ModulatorKind::Vector8 => {
                if let Some(&x) = p.params.first() {
                    st.xy_x = x.clamp(0.0, 1.0);
                }
                if let Some(&y) = p.params.get(1) {
                    st.xy_y = y.clamp(0.0, 1.0);
                }
                if let Some(&z) = p.params.get(2) {
                    st.vec8_z = z.clamp(0.0, 1.0);
                }
            }
            ModulatorKind::Steps => {
                st.steps_len = p.mode_idx.clamp(2, 16);
                if let Some(&hz) = p.params.first() {
                    st.lfo_rate_hz = hz.clamp(0.02, 30.0);
                }
                if let Some(&sm) = p.params.get(1) {
                    st.steps_smooth = sm.clamp(0.0, 1.0);
                }
                if let Some(&sr) = p.params.get(2) {
                    st.lfo_sync_rate = SyncRate::ALL
                        .get(sr.round().max(0.0) as usize)
                        .copied()
                        .unwrap_or(SyncRate::Eighth);
                }
                for i in 0..16 {
                    if let Some(&v) = p.params.get(3 + i) {
                        st.steps_values[i] = v.clamp(0.0, 1.0);
                    }
                }
                if let Some(&sync) = p.bool_flags.first() {
                    st.lfo_sync_mode = if sync {
                        LfoSyncMode::TempoSync
                    } else {
                        LfoSyncMode::FreeHz
                    };
                }
                if let Some(&bip) = p.bool_flags.get(1) {
                    st.steps_bipolar = bip;
                }
            }
            ModulatorKind::ParSeq8 => {
                st.parseq_len = p.mode_idx.clamp(2, 8);
                if let Some(&hz) = p.params.first() {
                    st.lfo_rate_hz = hz.clamp(0.02, 30.0);
                }
                if let Some(&sm) = p.params.get(1) {
                    st.parseq_smooth = sm.clamp(0.0, 1.0);
                }
                if let Some(&sr) = p.params.get(2) {
                    st.lfo_sync_rate = SyncRate::ALL
                        .get(sr.round().max(0.0) as usize)
                        .copied()
                        .unwrap_or(SyncRate::Eighth);
                }
                for i in 0..8 {
                    if let Some(&v) = p.params.get(3 + i) {
                        st.parseq_steps[i] = v.clamp(0.0, 1.0);
                    }
                }
                if let Some(&sync) = p.bool_flags.first() {
                    st.lfo_sync_mode = if sync {
                        LfoSyncMode::TempoSync
                    } else {
                        LfoSyncMode::FreeHz
                    };
                }
            }
            ModulatorKind::Curves => {
                if let Some(&hz) = p.params.first() {
                    st.lfo_rate_hz = hz.clamp(0.02, 30.0);
                }
                if let Some(&d) = p.params.get(1) {
                    st.lfo_depth = d.clamp(0.0, 1.0);
                }
                if let Some(&sr) = p.params.get(2) {
                    st.lfo_sync_rate = SyncRate::ALL
                        .get(sr.round().max(0.0) as usize)
                        .copied()
                        .unwrap_or(SyncRate::Quarter);
                }
                for i in 0..8 {
                    if let Some(&v) = p.params.get(3 + i) {
                        st.curves_nodes[i] = v.clamp(0.0, 1.0);
                    }
                }
                if let Some(&sync) = p.bool_flags.first() {
                    st.lfo_sync_mode = if sync {
                        LfoSyncMode::TempoSync
                    } else {
                        LfoSyncMode::FreeHz
                    };
                }
                if let Some(&sm) = p.bool_flags.get(1) {
                    st.curves_smooth = sm;
                }
                if let Some(&bip) = p.bool_flags.get(2) {
                    st.curves_bipolar = bip;
                }
            }
            ModulatorKind::Ramp => {
                if let Some(&t) = p.params.first() {
                    st.ramp_time_secs = t.clamp(0.05, 16.0);
                }
                if let Some(&c) = p.params.get(1) {
                    st.ramp_curve = c.clamp(-1.0, 1.0);
                }
                if let Some(&lp) = p.bool_flags.first() {
                    st.ramp_loop = lp;
                }
                if let Some(&dn) = p.bool_flags.get(1) {
                    st.ramp_down = dn;
                }
                if let Some(&bip) = p.bool_flags.get(2) {
                    st.ramp_bipolar = bip;
                }
            }
            ModulatorKind::Segments => {
                if let Some(&t) = p.params.first() {
                    st.seg_time_secs = t.clamp(0.05, 16.0);
                }
                if let Some(&c) = p.params.get(1) {
                    st.seg_curve = c.clamp(-1.0, 1.0);
                }
                for i in 0..6 {
                    if let Some(&v) = p.params.get(2 + i) {
                        st.seg_levels[i] = v.clamp(0.0, 1.0);
                    }
                }
                if let Some(&lp) = p.bool_flags.first() {
                    st.seg_loop = lp;
                }
                if let Some(&bip) = p.bool_flags.get(1) {
                    st.seg_bipolar = bip;
                }
            }
            ModulatorKind::Adsr => {
                if let Some(&a) = p.params.first() {
                    st.env_attack = a.clamp(0.002, 10.0);
                }
                if let Some(&d) = p.params.get(1) {
                    st.env_decay = d.clamp(0.002, 10.0);
                }
                if let Some(&s) = p.params.get(2) {
                    st.env_sustain = s.clamp(0.0, 1.0);
                }
                if let Some(&r) = p.params.get(3) {
                    st.env_release = r.clamp(0.002, 10.0);
                }
                if let Some(&g) = p.bool_flags.first() {
                    st.env_gate = g;
                }
            }
            ModulatorKind::Ahdsr => {
                if let Some(&a) = p.params.first() {
                    st.env_attack = a.clamp(0.002, 10.0);
                }
                if let Some(&h) = p.params.get(1) {
                    st.env_hold = h.clamp(0.0, 5.0);
                }
                if let Some(&d) = p.params.get(2) {
                    st.env_decay = d.clamp(0.002, 10.0);
                }
                if let Some(&s) = p.params.get(3) {
                    st.env_sustain = s.clamp(0.0, 1.0);
                }
                if let Some(&r) = p.params.get(4) {
                    st.env_release = r.clamp(0.002, 10.0);
                }
                if let Some(&g) = p.bool_flags.first() {
                    st.env_gate = g;
                }
            }
            ModulatorKind::FourStage => {
                for i in 0..4 {
                    if let Some(&t) = p.params.get(i) {
                        st.env_4stage_times[i] = t.clamp(0.005, 10.0);
                    }
                    if let Some(&l) = p.params.get(4 + i) {
                        st.env_4stage_levels[i] = l.clamp(0.0, 1.0);
                    }
                }
                if let Some(&lp) = p.bool_flags.first() {
                    st.env_loop = lp;
                }
                if let Some(&g) = p.bool_flags.get(1) {
                    st.env_gate = g;
                }
            }
            ModulatorKind::AhdOnRelease => {
                if let Some(&a) = p.params.first() {
                    st.env_attack = a.clamp(0.002, 10.0);
                }
                if let Some(&h) = p.params.get(1) {
                    st.env_hold = h.clamp(0.0, 5.0);
                }
                if let Some(&d) = p.params.get(2) {
                    st.env_decay = d.clamp(0.002, 10.0);
                }
            }
            ModulatorKind::Expressions => {
                if let Some(&v) = p.params.first() {
                    st.expr_vel = v.clamp(0.0, 1.0);
                }
                if let Some(&t) = p.params.get(1) {
                    st.expr_timbre = t.clamp(0.0, 1.0);
                }
                if let Some(&r) = p.params.get(2) {
                    st.expr_rel = r.clamp(0.0, 1.0);
                }
                if let Some(&pr) = p.params.get(3) {
                    st.expr_pres = pr.clamp(0.0, 1.0);
                }
            }
            ModulatorKind::Midi => {
                st.midi_cc_num = p.mode_idx.min(127);
                if let Some(&v) = p.params.first() {
                    st.midi_cc_val = v.clamp(0.0, 1.0);
                }
                if let Some(&mw) = p.params.get(1) {
                    st.midi_mw = mw.clamp(0.0, 1.0);
                }
                if let Some(&pb) = p.params.get(2) {
                    st.midi_pb = pb.clamp(-1.0, 1.0);
                }
                if let Some(&at) = p.params.get(3) {
                    st.midi_at = at.clamp(0.0, 1.0);
                }
            }
            ModulatorKind::KeytrackPlus => {
                st.key_root = p.mode_idx.min(127);
                if let Some(&sp) = p.params.first() {
                    st.key_spread_semi = sp.clamp(1.0, 64.0);
                }
                if let Some(&cv) = p.params.get(1) {
                    st.key_curve = cv.clamp(-1.0, 1.0);
                }
                if let Some(&kn) = p.params.get(2) {
                    st.key_note = (kn.round() as i32).clamp(0, 127) as u8;
                }
            }
            ModulatorKind::RelativeKeytrack => {
                if let Some(&sp) = p.params.first() {
                    st.key_spread_semi = sp.clamp(1.0, 64.0);
                }
                if let Some(&kn) = p.params.get(1) {
                    st.key_note = (kn.round() as i32).clamp(0, 127) as u8;
                }
                if let Some(&kp) = p.params.get(2) {
                    st.key_prev_note = (kp.round() as i32).clamp(0, 127) as u8;
                }
            }
            ModulatorKind::Pitch12 => {
                st.key_note = p.mode_idx.min(127);
            }
            ModulatorKind::NoteCounter => {
                st.nc_steps = (p.mode_idx >> 2).clamp(2, 16);
                st.nc_mode = p.mode_idx & 0x03;
                if let Some(&c) = p.params.first() {
                    st.nc_count = c.max(0.0) as u32;
                }
            }
            ModulatorKind::NoteSidechain => {
                if let Some(&a) = p.params.first() {
                    st.env_attack = a.clamp(0.002, 5.0);
                }
                if let Some(&r) = p.params.get(1) {
                    st.env_release = r.clamp(0.002, 5.0);
                }
                if let Some(&mn) = p.params.get(2) {
                    st.nsc_min_note = (mn.round() as i32).clamp(0, 127) as u8;
                }
                if let Some(&mx) = p.params.get(3) {
                    st.nsc_max_note = (mx.round() as i32).clamp(0, 127) as u8;
                }
            }
            ModulatorKind::Vibrato => {
                if let Some(&hz) = p.params.first() {
                    st.vib_rate_hz = hz.clamp(0.1, 20.0);
                }
                if let Some(&d) = p.params.get(1) {
                    st.vib_depth = d.clamp(0.0, 1.0);
                }
                if let Some(&fd) = p.params.get(2) {
                    st.vib_fade_secs = fd.clamp(0.0, 5.0);
                }
                if let Some(&mw) = p.params.get(3) {
                    st.vib_mw_scale = mw.clamp(0.0, 1.0);
                }
            }
            ModulatorKind::VoiceControl => {
                if let Some(&vi) = p.params.first() {
                    st.voice_index = (vi.round() as i32).clamp(0, 15) as u8;
                }
                if let Some(&vc) = p.params.get(1) {
                    st.voice_count = (vc.round() as i32).clamp(1, 16) as u8;
                }
                if let Some(&va) = p.params.get(2) {
                    st.voice_age_secs = va.max(0.0);
                }
                if let Some(&vl) = p.params.get(3) {
                    st.expr_vel = vl.clamp(0.0, 1.0);
                }
            }
            ModulatorKind::StackSpread => {
                st.spread_mode = p.mode_idx.min(2);
                if let Some(&amt) = p.params.first() {
                    st.spread_amount = amt.clamp(-1.0, 1.0);
                }
                if let Some(&vi) = p.params.get(1) {
                    st.voice_index = (vi.round() as i32).clamp(0, 15) as u8;
                }
                if let Some(&vc) = p.params.get(2) {
                    st.voice_count = (vc.round() as i32).clamp(1, 16) as u8;
                }
            }
            ModulatorKind::WavetableLfo => {
                if let Some(&hz) = p.params.first() {
                    st.lfo_rate_hz = hz.clamp(0.02, 30.0);
                }
                if let Some(&d) = p.params.get(1) {
                    st.lfo_depth = d.clamp(0.0, 1.0);
                }
                if let Some(&wt) = p.params.get(2) {
                    st.wt_index = wt.clamp(0.0, 1.0);
                }
                if let Some(&sr) = p.params.get(3) {
                    st.lfo_sync_rate = SyncRate::ALL
                        .get(sr.round().max(0.0) as usize)
                        .copied()
                        .unwrap_or(SyncRate::Quarter);
                }
                if let Some(&sync) = p.bool_flags.first() {
                    st.lfo_sync_mode = if sync {
                        LfoSyncMode::TempoSync
                    } else {
                        LfoSyncMode::FreeHz
                    };
                }
                if let Some(&bip) = p.bool_flags.get(1) {
                    st.lfo_bipolar = bip;
                }
            }
            ModulatorKind::Channel16 => {
                st.ch16_active = p.mode_idx.min(15);
                for i in 0..16 {
                    if let Some(&v) = p.params.get(i) {
                        st.ch16_levels[i] = v.clamp(0.0, 1.0);
                    }
                }
                if let Some(&ex) = p.bool_flags.first() {
                    st.ch16_exclusive = ex;
                }
            }
            ModulatorKind::Globals => {}
            ModulatorKind::EnvelopeFollower => {
                if let Some(&g) = p.params.first() {
                    st.audio_gain = g.clamp(0.0, 4.0);
                }
                if let Some(&a) = p.params.get(1) {
                    st.env_attack = a.clamp(0.002, 2.0);
                }
                if let Some(&r) = p.params.get(2) {
                    st.env_release = r.clamp(0.002, 3.0);
                }
                if let Some(&lv) = p.params.get(3) {
                    st.audio_input_level = lv.clamp(0.0, 1.0);
                }
            }
            ModulatorKind::AudioSidechain => {
                st.audio_sc_track = p.mode_idx;
                if let Some(&g) = p.params.first() {
                    st.audio_gain = g.clamp(0.0, 4.0);
                }
                if let Some(&th) = p.params.get(1) {
                    st.audio_threshold = th.clamp(0.0, 1.0);
                }
                if let Some(&a) = p.params.get(2) {
                    st.env_attack = a.clamp(0.002, 2.0);
                }
                if let Some(&r) = p.params.get(3) {
                    st.env_release = r.clamp(0.002, 3.0);
                }
                if let Some(&lv) = p.params.get(4) {
                    st.audio_input_level = lv.clamp(0.0, 1.0);
                }
                if let Some(&inv) = p.bool_flags.first() {
                    st.audio_sc_invert = inv;
                }
            }
            ModulatorKind::AudioRate => {
                st.ar_wave = p.mode_idx.min(3);
                if let Some(&hz) = p.params.first() {
                    st.ar_freq_hz = hz.clamp(20.0, 2000.0);
                }
                if let Some(&g) = p.params.get(1) {
                    st.audio_gain = g.clamp(0.0, 4.0);
                }
                if let Some(&rec) = p.bool_flags.first() {
                    st.ar_rectify = rec;
                }
            }
            ModulatorKind::HwCvIn => {
                st.cv_channel = p.mode_idx.clamp(1, 8);
                if let Some(&g) = p.params.first() {
                    st.audio_gain = g.clamp(0.0, 4.0);
                }
                if let Some(&off) = p.params.get(1) {
                    st.cv_offset = off.clamp(-1.0, 1.0);
                }
                if let Some(&lv) = p.params.get(2) {
                    st.cv_level = lv.clamp(-1.0, 1.0);
                }
                if let Some(&bip) = p.bool_flags.first() {
                    st.cv_bipolar = bip;
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
    super::lfo_mod::clear_slot_param_sent_cache(track_idx, slot_idx);
}

/// Inicializa el Device Modulator Bay de `(track_idx, slot_idx)` vacío (3 celdas `+`
/// como Bitwig en plugins externos VST3/CLAP) si todavía no existe entrada en el mapa.
pub fn ensure_empty_bay_for_slot(track_idx: usize, slot_idx: usize) {
    let mut map = bay_state_map().lock().unwrap();
    map.entry((track_idx, slot_idx))
        .or_insert_with(DeviceModulatorBay::empty);
}

pub fn bay_width_for_slot(track_idx: usize, slot_idx: usize) -> f32 {
    let mut map = bay_state_map().lock().unwrap();
    let entry = map
        .entry((track_idx, slot_idx))
        .or_insert_with(DeviceModulatorBay::default);
    entry.width_px()
}

pub fn get_bay_route_amount_target(
    track_idx: usize,
    slot_idx: usize,
    mod_id: u32,
    out_idx: u8,
    target: ModulationTarget,
) -> f32 {
    let map = bay_state_map().lock().unwrap();
    map.get(&(track_idx, slot_idx))
        .and_then(|bay| bay.slots.iter().find(|s| s.id == mod_id))
        .map(|s| s.route_amount(out_idx, target))
        .unwrap_or(0.0)
}

pub fn set_bay_route_amount_target(
    track_idx: usize,
    slot_idx: usize,
    mod_id: u32,
    out_idx: u8,
    target: ModulationTarget,
    amount: f32,
) {
    update_bay_state(track_idx, slot_idx, |bay| {
        if let Some(s) = bay.slots.iter_mut().find(|s| s.id == mod_id) {
            s.set_route(out_idx, target, amount);
        }
    });
}

pub fn clear_bay_routes_for_target(track_idx: usize, target: ModulationTarget) {
    let project_slot = crate::views::remote_controls::PROJECT_MODULATOR_SLOT;
    let mut map = bay_state_map().lock().unwrap();
    for (&(tr, sl), bay) in map.iter_mut() {
        if tr != track_idx && !(tr == 0 && sl == project_slot) {
            continue;
        }
        for s in &mut bay.slots {
            s.routes.retain(|r| r.target != target);
        }
    }
}

/// Devuelve `true` si algún modulador habilitado en la pista o en el Project Bay
/// está modulando algún parámetro del plugin externo en `(track_idx, slot_idx)`.
pub fn has_active_external_modulations(track_idx: usize, slot_idx: usize) -> bool {
    let project_slot = crate::views::remote_controls::PROJECT_MODULATOR_SLOT;
    let map = bay_state_map().lock().unwrap();
    for (&(tr, sl), bay) in map.iter() {
        if tr != track_idx && !(tr == 0 && sl == project_slot) {
            continue;
        }
        for m in &bay.slots {
            if !m.enabled || m.routes.is_empty() {
                continue;
            }
            if m.routes.iter().any(|r| {
                matches!(r.target, ModulationTarget::ExternalParam { slot_idx: s, .. } if s == slot_idx)
                    && r.amount.abs() >= 0.005
            }) {
                return true;
            }
        }
    }
    false
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
    get_bay_route_amount_target(track_idx, slot_idx, mod_id, out_idx, target)
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
    set_bay_route_amount_target(track_idx, slot_idx, mod_id, out_idx, target, amount);
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

/// Notifica a todos los moduladores sensibles a notas en `track_idx` sobre un Note-On.
pub fn notify_bay_note_on(track_idx: usize, note: u8, velocity: f32) {
    let now = clock_time_secs();
    let vel = velocity.clamp(0.0, 1.0);
    let mut map = bay_state_map().lock().unwrap();
    for (&(tr, _slot), bay) in map.iter_mut() {
        if tr != track_idx {
            continue;
        }
        for m in &mut bay.slots {
            match m.kind {
                ModulatorKind::Adsr
                | ModulatorKind::Ahdsr
                | ModulatorKind::FourStage
                | ModulatorKind::AhdOnRelease
                | ModulatorKind::Vibrato => {
                    m.trigger_gate(true, now);
                }
                ModulatorKind::NoteSidechain => {
                    m.key_note = note;
                    if note >= m.nsc_min_note && note <= m.nsc_max_note {
                        m.trigger_gate(true, now);
                    }
                }
                ModulatorKind::Expressions => {
                    m.expr_vel = vel;
                }
                ModulatorKind::KeytrackPlus | ModulatorKind::Pitch12 => {
                    m.key_note = note;
                }
                ModulatorKind::RelativeKeytrack => {
                    m.key_prev_note = m.key_note;
                    m.key_note = note;
                }
                ModulatorKind::NoteCounter => {
                    m.nc_count = m.nc_count.wrapping_add(1);
                }
                ModulatorKind::VoiceControl => {
                    m.expr_vel = vel;
                    m.voice_age_secs = 0.0;
                }
                _ => {}
            }
        }
    }
}

/// Notifica a todos los moduladores sensibles a notas en `track_idx` sobre un Note-Off.
pub fn notify_bay_note_off(track_idx: usize, _note: u8, release_vel: f32) {
    let now = clock_time_secs();
    let rvel = release_vel.clamp(0.0, 1.0);
    let mut map = bay_state_map().lock().unwrap();
    for (&(tr, _slot), bay) in map.iter_mut() {
        if tr != track_idx {
            continue;
        }
        for m in &mut bay.slots {
            match m.kind {
                ModulatorKind::Adsr
                | ModulatorKind::Ahdsr
                | ModulatorKind::FourStage
                | ModulatorKind::AhdOnRelease
                | ModulatorKind::NoteSidechain => {
                    m.trigger_gate(false, now);
                }
                ModulatorKind::Expressions => {
                    m.expr_rel = rvel;
                }
                _ => {}
            }
        }
    }
}

/// Alimenta el nivel de audio en vivo (peak/rms) de un track hacia los moduladores
/// `Envelope Follower`, `Audio Sidechain` y `Audio Rate`.
pub fn notify_bay_audio_level(track_idx: usize, peak: f32, rms: f32) {
    let lvl = peak.max(rms).clamp(0.0, 1.0);
    let mut map = bay_state_map().lock().unwrap();
    for (&(tr, _slot), bay) in map.iter_mut() {
        for m in &mut bay.slots {
            match m.kind {
                ModulatorKind::EnvelopeFollower | ModulatorKind::AudioRate if tr == track_idx => {
                    m.audio_input_level = lvl;
                }
                ModulatorKind::AudioSidechain if m.audio_sc_track as usize == track_idx => {
                    m.audio_input_level = lvl;
                }
                _ => {}
            }
        }
    }
}

/// Suma las contribuciones de todos los moduladores de bay activos en `track_idx`
/// (y en el Project Modulator Bay) para `target`. Devuelve `(matched, sum_raw, sum_delta, sum_amount, last_phase)`.
pub fn evaluate_bay_modulation_for_target(
    track_idx: usize,
    target: ModulationTarget,
    transport: &LfoTransportState,
) -> (bool, f32, f32, f32, f32) {
    let project_slot = crate::views::remote_controls::PROJECT_MODULATOR_SLOT;
    let time_secs = clock_time_secs();
    let mut map = bay_state_map().lock().unwrap();

    let mut matched = false;
    let mut sum_raw = 0.0_f32;
    let mut sum_delta = 0.0_f32;
    let mut sum_amount = 0.0_f32;
    let mut last_phase = 0.0_f32;

    for (&(tr, sl), bay) in map.iter_mut() {
        if tr != track_idx && !(tr == 0 && sl == project_slot) {
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
                    if matches!(
                        m.kind,
                        ModulatorKind::ClassicLfo
                            | ModulatorKind::BeatLfo
                            | ModulatorKind::Random
                            | ModulatorKind::SampleAndHold
                            | ModulatorKind::Vibrato
                            | ModulatorKind::Steps
                            | ModulatorKind::ParSeq8
                            | ModulatorKind::Curves
                            | ModulatorKind::Ramp
                            | ModulatorKind::Segments
                    ) {
                        last_phase = m.lfo_last_phase;
                    }
                }
            }
        }
    }

    (matched, sum_raw, sum_delta, sum_amount, last_phase)
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
    evaluate_bay_modulation_for_target(track_idx, target, transport)
}
