// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_openmodulation/mod.rs
//! **Hikaru OpenModulation**: Moduladores nativos de Hikaru OpenLive.
//!
//! Este subsistema provee módulos de modulación interactiva Bitwig-style (LFO MOD, etc.)
//! capaces de modular cualquier knob de dispositivos nativos o externos (como `WT POS` en Hikaru OpenWavetable).

pub mod lfo_mod;

pub use lfo_mod::{
    get_global_transport, get_lfo_state, get_modulation, handle_knob_mapping_click,
    is_mapping_target, on_transport_pause_or_stop, on_transport_play, render_module,
    update_global_transport, update_lfo_state, ActiveModulation, LfoModState, LfoTransportState,
    ModulationTarget, MODULATION_CARD_WIDTH,
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

        let vol = ModulationTarget::OpenWavetableVolume;
        assert!(vol.matches("OpenWavetable", "VOL"));
        assert!(vol.matches("OpenWavetable", "vol"));

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

    #[test]
    fn test_lfo_state_sync_rate_continuous_accumulation() {
        use hikaru_dsp::effects::lfo::SyncRate;

        let mut state = LfoModState::default();
        assert_eq!(state.sync_rate, SyncRate::Quarter);
        assert_eq!(state.sync_continuous, 4.0);

        // Small incremental drags accumulate without being discarded
        for _ in 0..10 {
            state.sync_continuous += 0.06; // fractional mouse drag
        }
        assert!(state.sync_continuous > 4.5);
        let idx = state.sync_continuous.round() as usize;
        state.sync_rate = SyncRate::ALL[idx];
        assert_eq!(state.sync_rate, SyncRate::Eighth);

        // Setting sync rate snaps continuous accumulator cleanly
        state.set_sync_rate(SyncRate::Whole1);
        assert_eq!(state.sync_rate, SyncRate::Whole1);
        assert_eq!(state.sync_continuous, 2.0);
    }

    #[test]
    fn test_lfo_transport_bpm_sync_and_pause_reset() {
        use hikaru_dsp::effects::lfo::{LfoSyncMode, SyncRate};

        let mut state = LfoModState::default();
        state.sync_mode = LfoSyncMode::TempoSync;
        state.sync_rate = SyncRate::Quarter; // 1 beat per cycle
        state.depth = 1.0;
        state.amount = 1.0;
        state.phase_deg = 0.0;

        let sr = 44100.0_f32;
        let bpm = 120.0_f32;
        let samples_per_beat = (sr * 60.0) / bpm; // 22050 samples

        // 1. When paused / stopped: LFO is frozen at 0.0
        let stopped_transport = LfoTransportState {
            bpm,
            sample_count: 5000,
            sample_rate: sr,
            is_playing: false,
            playback_start_sample: 0,
        };
        let (phase, _, _) = state.compute_live_sample_from_transport(&stopped_transport);
        assert_eq!(phase, 0.0, "LFO debe frenar en 0.0 cuando el transporte está detenido/pausado");

        // 2. Play begins at sample 10000: phase must start at 0.0
        let play_transport_start = LfoTransportState {
            bpm,
            sample_count: 10000,
            sample_rate: sr,
            is_playing: true,
            playback_start_sample: 10000,
        };
        let (phase_start, _, _) = state.compute_live_sample_from_transport(&play_transport_start);
        assert_eq!(phase_start, 0.0, "LFO debe comenzar desde el principio (fase 0.0) al dar play");

        // 3. Exactly half a beat later (11025 samples elapsed): phase must be 0.5
        let play_half_beat = LfoTransportState {
            bpm,
            sample_count: 10000 + (samples_per_beat as u64 / 2),
            sample_rate: sr,
            is_playing: true,
            playback_start_sample: 10000,
        };
        let (phase_half, _, _) = state.compute_live_sample_from_transport(&play_half_beat);
        assert!((phase_half - 0.5).abs() < 1e-4, "LFO debe estar exactamente a medio ciclo en medio beat");

        // 4. Exactly one full beat later (22050 samples): phase wraps cleanly to 0.0 (exact quarter note)
        let play_full_beat = LfoTransportState {
            bpm,
            sample_count: 10000 + (samples_per_beat as u64),
            sample_rate: sr,
            is_playing: true,
            playback_start_sample: 10000,
        };
        let (phase_full, _, _) = state.compute_live_sample_from_transport(&play_full_beat);
        assert!(phase_full.abs() < 1e-4, "LFO debe completar exactamente un ciclo en 1 beat");

        // 5. When project pauses, LFO immediately halts and resets to 0.0
        let mut paused_transport = play_half_beat;
        paused_transport.is_playing = false;
        let (phase_paused, _, _) = state.compute_live_sample_from_transport(&paused_transport);
        assert_eq!(phase_paused, 0.0, "Al pausar el proyecto, el LFO debe frenar y volver al principio");
    }
}
