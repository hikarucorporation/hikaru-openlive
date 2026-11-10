// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_openmodulation/mod.rs
//! **Hikaru OpenModulation**: Moduladores nativos de Hikaru OpenLive.
//!
//! Este subsistema provee módulos de modulación interactiva Bitwig-style (LFO MOD, etc.)
//! capaces de modular cualquier knob de dispositivos nativos o externos (como `WT POS` en Hikaru OpenWavetable).

pub mod lfo_mod;

pub use lfo_mod::{
    get_lfo_state, get_modulation, handle_knob_mapping_click, is_mapping_target, render_module,
    update_lfo_state, ActiveModulation, LfoModState, ModulationTarget, MODULATION_CARD_WIDTH,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modulation_target_matching() {
        let wt_pos = ModulationTarget::OpenWavetableWtPos;
        assert!(wt_pos.matches("OpenWavetable", "WT POS"));
        assert!(wt_pos.matches("Hikaru OpenWavetable", "wt pos"));
        assert!(!wt_pos.matches("OpenWavetable", "DETUNE"));
        assert!(!wt_pos.matches("OpenSpectral Phaser", "WT POS"));

        let detune = ModulationTarget::OpenWavetableDetune;
        assert!(detune.matches("OpenWavetable", "DETUNE"));

        let offset = ModulationTarget::SpectralPhaserOffset;
        assert!(offset.matches("OpenSpectral Phaser", "Offset"));
        assert!(offset.matches("OpenSpectralPhaser", "offset"));
    }

    #[test]
    fn test_target_from_device_and_param() {
        let t1 = ModulationTarget::from_device_and_param("OpenWavetable", "WT POS");
        assert_eq!(t1, Some(ModulationTarget::OpenWavetableWtPos));

        let t2 = ModulationTarget::from_device_and_param("OpenSpectral Phaser", "Depth");
        assert_eq!(t2, Some(ModulationTarget::SpectralPhaserDepth));

        let t_none = ModulationTarget::from_device_and_param("NonExistent", "Foo");
        assert_eq!(t_none, None);
    }

    #[test]
    fn test_lfo_state_live_sample_computation() {
        let mut state = LfoModState::default();
        state.rate_hz = 2.0;
        state.depth = 1.0;
        state.amount = 0.5;
        state.bipolar = true;

        // At time = 0.0 with Sine, value is 0.0
        let (phase, val, delta) = state.compute_live_sample(0.0, 120.0);
        assert!((phase - 0.0).abs() < 1e-4);
        assert!(val.abs() < 1e-4);
        assert!(delta.abs() < 1e-4);

        // At time = 0.125s (1/8s at 2Hz => phase 0.25 => peak of sine is +1.0)
        let (phase_peak, val_peak, delta_peak) = state.compute_live_sample(0.125, 120.0);
        assert!((phase_peak - 0.25).abs() < 1e-4);
        assert!((val_peak - 1.0).abs() < 1e-4);
        assert!((delta_peak - 0.5).abs() < 1e-4);
    }
}
