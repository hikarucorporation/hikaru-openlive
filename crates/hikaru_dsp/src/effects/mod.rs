// crates/hikaru_dsp/src/effects/mod.rs
pub mod bit8;
pub mod dc_offset;
pub mod dual_pan;
pub mod filter;
pub mod ultracomb;
pub mod phaser;
pub mod flanger;
pub mod lfo;
pub mod open_harmonic; // ¡Nuevo integrante!
pub mod openspectral_phaser; // OpenSpectral Phaser (EBPhaselock)
pub mod ring_mod;
pub mod test_tone;
pub mod time_shift;
pub mod tool;
pub mod tremolo;
pub mod spectral_phaser {
    pub use super::openspectral_phaser::*;
}
pub mod phaselock {
    pub use super::openspectral_phaser::*;
}

pub use bit8::{
    Bit8Effect, Bit8Params, Bit8QuantizeMode, Bit8Rectify, Bit8ShapeMode, Bit8Telemetry,
    Bit8TelemetrySnapshot, BIT8_DEFAULT_CLOCK_HZ, BIT8_DISPLAY_HISTORY, BIT8_MAX_CLOCK_HZ,
    BIT8_MAX_DRIVE_DB, BIT8_MIN_CLOCK_HZ, BIT8_PARAM_ANTI_ALIAS, BIT8_PARAM_CHANCE,
    BIT8_PARAM_CLOCK_HZ, BIT8_PARAM_COUNT, BIT8_PARAM_DIFFUSION, BIT8_PARAM_GATE_DB,
    BIT8_PARAM_JITTER, BIT8_PARAM_MIX, BIT8_PARAM_QUANTIZE_DB, BIT8_PARAM_QUANTIZE_MODE,
    BIT8_PARAM_RECTIFY, BIT8_PARAM_SHAPE_DRIVE_DB, BIT8_PARAM_SHAPE_MODE,
    BIT8_PARAM_SQUARED_DOMAIN, BIT8_PARAM_STEREO_DITHER, BIT8_PARAM_WET_GAIN_DB,
    BIT8_PARAM_WET_HP_HZ, BIT8_PARAM_WET_LP_HZ, BIT8_PARAM_WIDTH,
};
pub use dc_offset::{DcOffsetEffect, DcOffsetParams};
pub use dual_pan::{DualPanEffect, DualPanParams};
pub use filter::StateVariableFilter;
pub use ultracomb::{CombFilter, UltracombFilter, UltracombParams};
pub use phaser::Phaser;
pub use flanger::Flanger;
pub use lfo::{evaluate_waveform, LfoOscillator, LfoSyncMode, LfoWaveform, SyncRate};
pub use open_harmonic::OpenHarmonicProcessor;
pub use openspectral_phaser::*;
pub use ring_mod::{RingModEffect, RingModParams, RingModWave};
pub use test_tone::{TestToneEffect, TestToneParams, TestToneWave};
pub use time_shift::{TimeShiftEffect, TimeShiftParams};
pub use tool::{ToolEffect, ToolParams};
pub use tremolo::{TremoloEffect, TremoloParams, TremoloShape};