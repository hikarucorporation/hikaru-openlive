// crates/hikaru_dsp/src/effects/mod.rs
pub mod filter;
pub mod ultracomb;
pub mod phaser;
pub mod flanger;
pub mod open_harmonic; // ¡Nuevo integrante!

pub use filter::StateVariableFilter;
pub use ultracomb::CombFilter;
pub use phaser::Phaser;
pub use flanger::Flanger;
pub use open_harmonic::OpenHarmonicProcessor;