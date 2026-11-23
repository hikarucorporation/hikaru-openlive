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

pub use bit8::{Bit8Effect, Bit8Params};
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