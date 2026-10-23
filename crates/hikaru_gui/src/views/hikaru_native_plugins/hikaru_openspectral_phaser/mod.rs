// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_openspectral_phaser/mod.rs

//! **Hikaru OpenSpectral Phaser**: phaser espectral (ingeniería inversa del
//! UZU.vst3 / parche Pure Data "EBPhaselock").
//!
//! El motor DSP vive en `hikaru_dsp::effects::openspectral_phaser`; acá está
//! el panel nativo del rack con los 11 parámetros del plugin original.

pub mod openspectral_phaser;
pub mod spectral_phaser {
    pub use super::openspectral_phaser::*;
}
pub mod phaselock {
    pub use super::openspectral_phaser::*;
}

pub use openspectral_phaser::*;