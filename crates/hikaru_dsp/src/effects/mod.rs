// crates/hikaru_dsp/src/effects/mod.rs
pub mod filter;
pub mod ultracomb;
pub mod phaser;
pub mod flanger;
pub mod open_harmonic; // ¡Nuevo integrante!
pub mod openspectral_phaser; // OpenSpectral Phaser (EBPhaselock)
pub mod spectral_phaser {
    pub use super::openspectral_phaser::*;
}
pub mod phaselock {
    pub use super::openspectral_phaser::*;
}

pub use filter::StateVariableFilter;
pub use ultracomb::CombFilter;
pub use phaser::Phaser;
pub use flanger::Flanger;
pub use open_harmonic::OpenHarmonicProcessor;
pub use openspectral_phaser::*;