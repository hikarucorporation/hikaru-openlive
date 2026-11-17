// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_ultracomb/mod.rs

//! **Hikaru Ultracomb Filter**: filtro comb resonante estéreo inspirado en Au5.
//!
//! El motor DSP vive en `hikaru_dsp::effects::ultracomb::UltracombFilter`; acá
//! está el panel nativo del rack con los 2 knobs principales: `Cutoff` y `Resonance`.

pub mod ultracomb;

pub use ultracomb::*;
