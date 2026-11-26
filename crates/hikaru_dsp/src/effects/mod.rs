// crates/hikaru_dsp/src/effects/mod.rs
pub mod bit8;
pub mod dc_offset;
pub mod dual_pan;
pub mod filter;
pub mod freq_shifter_plus;
pub mod ultracomb;
pub mod phaser;
pub mod phaser_plus;
pub mod flanger;
pub mod flanger_plus;
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

/// Interfaz común para efectos nativos que alojan una cadena anidada `Wet FX`
/// (por ejemplo, `Hikaru Open8-Bit` y `Hikaru OpenFreqShifter+`).
pub trait WetFxChainHost {
    fn wet_chain(&self) -> &[crate::plugin_node::DspPluginNode];
    fn wet_chain_mut(&mut self) -> &mut Vec<crate::plugin_node::DspPluginNode>;
    fn insert_wet_fx(&mut self, node: crate::plugin_node::DspPluginNode);
    fn remove_wet_fx(&mut self, wet_idx: usize);
    fn set_wet_fx_active(&mut self, wet_idx: usize, active: bool);
    fn set_wet_fx_param(&mut self, wet_idx: usize, param_id: u32, value: f32);
    fn sync_wet_fx_params(&mut self, wet_idx: usize, params: &[(u32, f32)]);
}

pub use bit8::{
    Bit8Effect, Bit8Params, Bit8QuantizeMode, Bit8Rectify, Bit8ShapeMode, Bit8Telemetry,
    Bit8TelemetrySnapshot, BIT8_DEFAULT_CLOCK_HZ, BIT8_DISPLAY_HISTORY, BIT8_MAX_CLOCK_HZ,
    BIT8_MAX_DRIVE_DB, BIT8_MIN_CLOCK_HZ, BIT8_PARAM_ANTI_ALIAS, BIT8_PARAM_CHANCE,
    BIT8_PARAM_CLOCK_HZ, BIT8_PARAM_COUNT, BIT8_PARAM_DIFFUSION, BIT8_PARAM_GATE_DB,
    BIT8_PARAM_JITTER, BIT8_PARAM_MIX, BIT8_PARAM_QUANTIZE_DB, BIT8_PARAM_QUANTIZE_MODE,
    BIT8_PARAM_RECTIFY, BIT8_PARAM_SHAPE_MODE, BIT8_PARAM_SQUARED_DOMAIN, BIT8_PARAM_STEREO_DITHER,
    BIT8_PARAM_WET_GAIN_DB, BIT8_PARAM_WET_HP_HZ, BIT8_PARAM_WET_LP_HZ, BIT8_PARAM_WIDTH,
};
pub use dc_offset::{DcOffsetEffect, DcOffsetParams};
pub use dual_pan::{DualPanEffect, DualPanParams};
pub use filter::StateVariableFilter;
pub use freq_shifter_plus::{
    FreqShiftPoles, FreqShiftQuality, FreqShiftUnit, FreqShifterPlusEffect, FreqShifterPlusParams,
    DELAY_SYNC_BEATS, DELAY_SYNC_LABELS, FREQ_SHIFTER_MAX_CUT_HZ, FREQ_SHIFTER_MAX_DELAY_MS,
    FREQ_SHIFTER_MAX_FEEDBACK, FREQ_SHIFTER_MIN_CUT_HZ, FREQ_SHIFTER_PLUS_PARAM_COUNT,
    FSP_PARAM_DELAY_MS, FSP_PARAM_DELAY_SYNC, FSP_PARAM_DELAY_SYNC_DIV, FSP_PARAM_FB_CROSS,
    FSP_PARAM_FB_HP_HZ, FSP_PARAM_FB_LP_HZ, FSP_PARAM_FB_SHIFT_INDEP, FSP_PARAM_FB_SHIFT_NORM,
    FSP_PARAM_FEEDBACK, FSP_PARAM_MIX, FSP_PARAM_POLE_ORDER, FSP_PARAM_QUALITY,
    FSP_PARAM_SHIFT_NORM, FSP_PARAM_SHIFT_UNIT, FSP_PARAM_STEREO_TWIST, FSP_PARAM_WET_GAIN_DB,
    FSP_PARAM_WIDTH,
};
pub use ultracomb::{CombFilter, UltracombFilter, UltracombParams};
pub use phaser::Phaser;
pub use phaser_plus::{
    PhaserPlusEffect, PhaserPlusMode, PhaserPlusParams, PHASER_PLUS_MAX_SPEED_HZ,
    PHASER_PLUS_MAX_STAGES, PHASER_PLUS_MIN_SPEED_HZ, PHASER_PLUS_PARAM_COUNT,
    PHASER_SYNC_BEATS, PHASER_SYNC_LABELS, PHP_PARAM_COLOR, PHP_PARAM_DEPTH,
    PHP_PARAM_FEEDBACK, PHP_PARAM_MIX, PHP_PARAM_MODE, PHP_PARAM_PHASE_INVERT,
    PHP_PARAM_SPEED, PHP_PARAM_STEREO, PHP_PARAM_SYNC_DIV, PHP_PARAM_TEMPO_SYNC,
};
pub use flanger::Flanger;
pub use flanger_plus::{
    FlangerPlusEffect, FlangerPlusMode, FlangerPlusParams, FLANGER_PLUS_MAX_SPEED_HZ,
    FLANGER_PLUS_MIN_SPEED_HZ, FLANGER_PLUS_PARAM_COUNT, FLANGER_SYNC_BEATS, FLANGER_SYNC_LABELS,
    FLP_PARAM_COLOR, FLP_PARAM_DEPTH, FLP_PARAM_FEEDBACK, FLP_PARAM_MIX, FLP_PARAM_MODE,
    FLP_PARAM_PHASE_INVERT, FLP_PARAM_SPEED, FLP_PARAM_STEREO, FLP_PARAM_SYNC_DIV,
    FLP_PARAM_TEMPO_SYNC,
};
pub use lfo::{evaluate_waveform, LfoOscillator, LfoSyncMode, LfoWaveform, SyncRate};
pub use open_harmonic::OpenHarmonicProcessor;
pub use openspectral_phaser::*;
pub use ring_mod::{RingModEffect, RingModParams, RingModWave};
pub use test_tone::{TestToneEffect, TestToneParams, TestToneWave};
pub use time_shift::{TimeShiftEffect, TimeShiftParams};
pub use tool::{ToolEffect, ToolParams};
pub use tremolo::{TremoloEffect, TremoloParams, TremoloShape};
