// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_openmodulation/mod.rs
//! **Hikaru OpenLFO MOD**: Modulador Dual LFO nativo estilo Bitwig Studio.
//!
//! Este subsistema provee el dispositivo **Hikaru OpenLFO MOD** (LFO1 + LFO2, osciloscopio dual,
//! Phase, Depth, Retrigger, Polaridad ±/+, Sync/Hz, slot FX y Mix) capaz de modular cualquier
//! knob de dispositivos nativos o externos (como `INDEX` / `WT POS` en Hikaru OpenPoly).

pub mod bay;
pub mod bay_ui;
pub mod lfo_mod;

pub use bay::{
    add_bay_modulator, bay_width_for_slot, capture_bay_params, clear_bay_for_slot,
    ensure_empty_bay_for_slot, get_bay_state, has_active_external_modulations,
    notify_bay_audio_level, notify_bay_note_off, notify_bay_note_on, remove_bay_modulator,
    restore_bay_params, update_bay_state, DeviceModulatorBay, MathOp, ModulatorCategory,
    ModulatorKind, ModulatorMeta, ModulatorSlotState, MODULATOR_CATALOG,
};
pub use bay_ui::render_device_modulator_bay;
pub use lfo_mod::{
    active_mapping_for_track, active_mapping_source_for_external, active_mapping_source_for_track,
    clear_external_param_routes, clear_mapping_mode, get_channel_route_amount,
    get_external_param_modulation, get_external_route_amount, get_global_transport, get_lfo_state,
    get_modulation, handle_knob_mapping_click, is_any_mapping_active_for_track,
    is_bay_mapping_active, is_mapping_target, modulatable_rotary_knob, on_transport_pause_or_stop,
    on_transport_play, render_module, render_preset_menu, set_channel_route_amount,
    set_external_route_amount, toggle_bay_mapping_mode, update_global_transport, update_lfo_state,
    ActiveModulation, LfoChannelState, LfoModState, LfoTransportState, ModSourceId,
    ModulationTarget, MODULATION_CARD_WIDTH, MOD_ARROW_CYAN,
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

    #[test]
    fn test_dual_lfo_mod_channels_mix_and_params_roundtrip() {
        use hikaru_dsp::effects::lfo::{LfoSyncMode, LfoWaveform, SyncRate};

        let mut state = LfoModState::default();
        state.lfo1.waveform = LfoWaveform::Triangle;
        state.lfo1.rate_hz = 2.5;
        state.lfo1.depth = 0.8;
        state.lfo1.phase_deg = 90.0;
        state.lfo1.retrigger = false;
        state.lfo1.target = ModulationTarget::OpenPolyCutoff;

        state.lfo2.waveform = LfoWaveform::Square;
        state.lfo2.sync_mode = LfoSyncMode::TempoSync;
        state.lfo2.set_sync_rate(SyncRate::Eighth);
        state.lfo2.depth = 0.6;
        state.lfo2.phase_deg = 45.0;
        state.lfo2.bipolar = false;
        state.lfo2.retrigger = true;
        state.lfo2.amount = -0.75;
        state.lfo2.target = ModulationTarget::OpenWavetableVolume;

        state.mix = 0.65;

        let params = state.to_params();
        let mut restored = LfoModState::default();
        restored.apply_params(&params);

        assert_eq!(restored.lfo1.waveform, LfoWaveform::Triangle);
        assert!((restored.lfo1.rate_hz - 2.5).abs() < 1e-4);
        assert!(!restored.lfo1.retrigger);
        assert_eq!(restored.lfo1.target, ModulationTarget::OpenPolyCutoff);

        assert_eq!(restored.lfo2.waveform, LfoWaveform::Square);
        assert_eq!(restored.lfo2.sync_mode, LfoSyncMode::TempoSync);
        assert_eq!(restored.lfo2.sync_rate, SyncRate::Eighth);
        assert!(!restored.lfo2.bipolar);
        assert!(restored.lfo2.retrigger);
        assert!((restored.lfo2.amount - (-0.75)).abs() < 1e-4);
        assert_eq!(restored.lfo2.target, ModulationTarget::OpenWavetableVolume);
        assert!((restored.mix - 0.65).abs() < 1e-4);
    }

    #[test]
    fn test_device_modulator_bay_classic_lfo_macro_button_eval_and_columns() {
        let mut bay = DeviceModulatorBay::empty();
        assert_eq!(bay.column_count(), 1); // just '+'

        let lfo_id = bay.add_modulator(ModulatorKind::ClassicLfo);
        let macro_id = bay.add_modulator(ModulatorKind::Macro);
        let btn_id = bay.add_modulator(ModulatorKind::Button);
        // 3 slots + '+' = 4 cells -> expands to 2 columns
        assert_eq!(bay.column_count(), 2);

        let transport = LfoTransportState::default();

        // Classic LFO at 2 Hz, t = 0.125s => sine peak = +1.0
        let lfo = bay.slots.iter_mut().find(|s| s.id == lfo_id).unwrap();
        lfo.lfo_rate_hz = 2.0;
        lfo.lfo_depth = 1.0;
        lfo.lfo_bipolar = true;
        let lfo_out = lfo.evaluate(&transport, 0.125)[0];
        assert!((lfo_out - 1.0).abs() < 1e-3, "Classic LFO sine peak expected ~1.0, got {lfo_out}");

        // Macro unipolar and bipolar
        let mac = bay.slots.iter_mut().find(|s| s.id == macro_id).unwrap();
        mac.macro_value = 0.75;
        mac.macro_bipolar = false;
        assert!((mac.evaluate(&transport, 0.0)[0] - 0.75).abs() < 1e-4);
        mac.macro_bipolar = true;
        assert!((mac.evaluate(&transport, 0.0)[0] - 0.5).abs() < 1e-4);
        mac.set_route(0, ModulationTarget::OpenPolyCutoff, 0.42);

        // Button OFF -> 0.0, ON -> 1.0
        let btn = bay.slots.iter_mut().find(|s| s.id == btn_id).unwrap();
        btn.button_on = false;
        assert_eq!(btn.evaluate(&transport, 0.0)[0], 0.0);
        btn.button_on = true;
        assert_eq!(btn.evaluate(&transport, 0.0)[0], 1.0);

        // Project params roundtrip
        let saved = bay.to_project_params(true);
        let mut restored = DeviceModulatorBay::empty();
        restored.apply_project_params(&saved);
        assert_eq!(restored.slots.len(), 3);
        let r_mac = restored.slots.iter().find(|s| s.kind == ModulatorKind::Macro).unwrap();
        assert!((r_mac.macro_value - 0.75).abs() < 1e-4);
        assert!(r_mac.macro_bipolar);
        assert!((r_mac.route_amount(0, ModulationTarget::OpenPolyCutoff) - 0.42).abs() < 1e-4);
    }

    #[test]
    fn test_bay_mapping_mode_and_get_modulation() {
        let track_idx = 99;
        let slot_idx = 3;
        clear_bay_for_slot(track_idx, slot_idx);
        let macro_id = add_bay_modulator(track_idx, slot_idx, ModulatorKind::Macro);
        update_bay_state(track_idx, slot_idx, |bay| {
            if let Some(m) = bay.slots.iter_mut().find(|s| s.id == macro_id) {
                m.macro_value = 0.8;
            }
        });

        toggle_bay_mapping_mode(track_idx, slot_idx, macro_id, 0);
        assert!(is_bay_mapping_active(track_idx, slot_idx, macro_id, 0));
        assert!(is_any_mapping_active_for_track(track_idx));
        assert!(handle_knob_mapping_click(track_idx, "OpenPoly", "CUTOFF"));
        assert!(!is_bay_mapping_active(track_idx, slot_idx, macro_id, 0));

        let m = get_modulation(track_idx, "OpenPoly", "CUTOFF")
            .expect("Macro modulation should be visible via get_modulation");
        assert!((m.delta - 0.4).abs() < 1e-3, "0.8 * 0.5 expected 0.4, got {}", m.delta);

        clear_bay_for_slot(track_idx, slot_idx);
    }

    #[test]
    fn test_multi_output_modulators_macro4_buttons_xy() {
        let mut bay = DeviceModulatorBay::empty();
        let m4_id = bay.add_modulator(ModulatorKind::Macro4);
        let btns_id = bay.add_modulator(ModulatorKind::Buttons);
        let xy_id = bay.add_modulator(ModulatorKind::Xy);
        let transport = LfoTransportState::default();

        // Macro-4: 4 independent outputs (unipolar + bipolar)
        let m4 = bay.slots.iter_mut().find(|s| s.id == m4_id).unwrap();
        m4.macro4_values = [0.25, 0.5, 0.75, 1.0];
        m4.macro4_bipolar = [false, true, true, false];
        let m4_outs = m4.evaluate(&transport, 0.0);
        assert!((m4_outs[0] - 0.25).abs() < 1e-4);
        assert!(m4_outs[1].abs() < 1e-4); // 0.5 bipolar -> 0.0
        assert!((m4_outs[2] - 0.5).abs() < 1e-4); // 0.75 bipolar -> +0.5
        assert!((m4_outs[3] - 1.0).abs() < 1e-4);
        m4.set_route(2, ModulationTarget::OpenPolyCutoff, 0.6);

        // Buttons: 4 independent ON/OFF pads
        let btns = bay.slots.iter_mut().find(|s| s.id == btns_id).unwrap();
        btns.buttons_on = [true, false, true, false];
        btns.buttons_momentary = [false, true, false, true];
        let b_outs = btns.evaluate(&transport, 0.0);
        assert_eq!(&b_outs[..4], &[1.0, 0.0, 1.0, 0.0]);

        // XY: 2 independent outputs (X=0, Y=1)
        let xy = bay.slots.iter_mut().find(|s| s.id == xy_id).unwrap();
        xy.xy_x = 0.25;
        xy.xy_y = 0.75;
        xy.xy_bipolar = true;
        let xy_outs = xy.evaluate(&transport, 0.0);
        assert!((xy_outs[0] - (-0.5)).abs() < 1e-4);
        assert!((xy_outs[1] - 0.5).abs() < 1e-4);
        xy.set_route(0, ModulationTarget::OpenPolyReso, 0.5);
        xy.set_route(1, ModulationTarget::OpenPolyCutoff, -0.4);

        // Roundtrip persistence
        let saved = bay.to_project_params(true);
        let mut restored = DeviceModulatorBay::empty();
        restored.apply_project_params(&saved);
        assert_eq!(restored.slots.len(), 3);
        let r_m4 = restored.slots.iter().find(|s| s.kind == ModulatorKind::Macro4).unwrap();
        assert!((r_m4.macro4_values[2] - 0.75).abs() < 1e-4);
        assert!(r_m4.macro4_bipolar[2]);
        assert!((r_m4.route_amount(2, ModulationTarget::OpenPolyCutoff) - 0.6).abs() < 1e-4);

        let r_btns = restored.slots.iter().find(|s| s.kind == ModulatorKind::Buttons).unwrap();
        assert_eq!(r_btns.buttons_on, [true, false, true, false]);
        assert_eq!(r_btns.buttons_momentary, [false, true, false, true]);

        let r_xy = restored.slots.iter().find(|s| s.kind == ModulatorKind::Xy).unwrap();
        assert!((r_xy.xy_x - 0.25).abs() < 1e-4);
        assert!((r_xy.xy_y - 0.75).abs() < 1e-4);
        assert!(r_xy.xy_bipolar);
        assert!((r_xy.route_amount(1, ModulationTarget::OpenPolyCutoff) - (-0.4)).abs() < 1e-4);
    }

    #[test]
    fn test_block1_modulators_beat_lfo_math_mix_quantize_select4_polynom() {
        use hikaru_dsp::effects::lfo::{LfoWaveform, SyncRate};

        let mut bay = DeviceModulatorBay::empty();
        let blfo_id = bay.add_modulator(ModulatorKind::BeatLfo);
        let math_id = bay.add_modulator(ModulatorKind::Math);
        let mix_id = bay.add_modulator(ModulatorKind::Mix);
        let quant_id = bay.add_modulator(ModulatorKind::Quantize);
        let sel4_id = bay.add_modulator(ModulatorKind::Select4);
        let poly_id = bay.add_modulator(ModulatorKind::Polynom);

        // Transport at 120 BPM (2 beats/sec), quarter note = 2 Hz, elapsed 0.125s = 0.25 cycles -> sine +1.0
        let transport = LfoTransportState {
            bpm: 120.0,
            sample_count: 5512, // ~0.125s at 44100 Hz
            sample_rate: 44100.0,
            is_playing: true,
            playback_start_sample: 0,
        };

        // 1. Beat LFO
        let blfo = bay.slots.iter_mut().find(|s| s.id == blfo_id).unwrap();
        blfo.lfo_waveform = LfoWaveform::Sine;
        blfo.lfo_sync_rate = SyncRate::Quarter;
        blfo.beat_lfo_skew = 0.5;
        blfo.lfo_depth = 1.0;
        blfo.lfo_bipolar = true;
        let blfo_out = blfo.evaluate(&transport, 0.0)[0];
        assert!((blfo_out - 1.0).abs() < 0.02, "Beat LFO expected ~1.0 at 1/4 cycle, got {blfo_out}");

        // 2. Math (Multiply: 0.5 * -0.4 = -0.2)
        let math = bay.slots.iter_mut().find(|s| s.id == math_id).unwrap();
        math.math_op = MathOp::Multiply;
        math.math_a = 0.5;
        math.math_b = -0.4;
        assert!((math.evaluate(&transport, 0.0)[0] - (-0.2)).abs() < 1e-4);

        // 3. Mix (A=-0.5, B=0.5, mix=0.75 => +0.25)
        let mix = bay.slots.iter_mut().find(|s| s.id == mix_id).unwrap();
        mix.mix_a = -0.5;
        mix.mix_b = 0.5;
        mix.mix_amount = 0.75;
        assert!((mix.evaluate(&transport, 0.0)[0] - 0.25).abs() < 1e-4);

        // 4. Quantize (5 steps: 0, 0.25, 0.5, 0.75, 1.0; input 0.62 -> 0.5)
        let quant = bay.slots.iter_mut().find(|s| s.id == quant_id).unwrap();
        quant.quant_steps = 5;
        quant.quant_bipolar = false;
        quant.quant_in = 0.62;
        assert!((quant.evaluate(&transport, 0.0)[0] - 0.5).abs() < 1e-4);

        // 5. Select-4 (smooth midpoint between In2=0.2 and In3=0.8 at index=0.5 -> 0.5)
        let sel4 = bay.slots.iter_mut().find(|s| s.id == sel4_id).unwrap();
        sel4.sel4_values = [0.0, 0.2, 0.8, 1.0];
        sel4.sel4_index = 0.5; // pos = 1.5 -> midpoint between idx 1 and 2
        sel4.sel4_smooth = true;
        assert!((sel4.evaluate(&transport, 0.0)[0] - 0.5).abs() < 1e-4);
        sel4.sel4_smooth = false;
        sel4.sel4_index = 1.0; // idx 3 -> 1.0
        assert!((sel4.evaluate(&transport, 0.0)[0] - 1.0).abs() < 1e-4);

        // 6. Polynom (c0=0.1, c1=0.0, c2=1.0, c3=0.0 at x=0.5 -> 0.1 + 0.25 = 0.35)
        let poly = bay.slots.iter_mut().find(|s| s.id == poly_id).unwrap();
        poly.poly_in = 0.5;
        poly.poly_c0 = 0.1;
        poly.poly_c1 = 0.0;
        poly.poly_c2 = 1.0;
        poly.poly_c3 = 0.0;
        assert!((poly.evaluate(&transport, 0.0)[0] - 0.35).abs() < 1e-4);

        // Persistence roundtrip for all 6
        let saved = bay.to_project_params(true);
        let mut restored = DeviceModulatorBay::empty();
        restored.apply_project_params(&saved);
        assert_eq!(restored.slots.len(), 6);
        let r_math = restored.slots.iter().find(|s| s.kind == ModulatorKind::Math).unwrap();
        assert_eq!(r_math.math_op, MathOp::Multiply);
        assert!((r_math.math_b - (-0.4)).abs() < 1e-4);

        let r_quant = restored.slots.iter().find(|s| s.kind == ModulatorKind::Quantize).unwrap();
        assert_eq!(r_quant.quant_steps, 5);

        let r_poly = restored.slots.iter().find(|s| s.kind == ModulatorKind::Polynom).unwrap();
        assert!((r_poly.poly_c2 - 1.0).abs() < 1e-4);
    }

    #[test]
    fn test_block2_modulators_clock_random_sequencers_vectors() {
        use hikaru_dsp::effects::lfo::LfoSyncMode;

        let mut bay = DeviceModulatorBay::empty();
        let rand_id = bay.add_modulator(ModulatorKind::Random);
        let sh_id = bay.add_modulator(ModulatorKind::SampleAndHold);
        let steps_id = bay.add_modulator(ModulatorKind::Steps);
        let parseq_id = bay.add_modulator(ModulatorKind::ParSeq8);
        let curves_id = bay.add_modulator(ModulatorKind::Curves);
        let ramp_id = bay.add_modulator(ModulatorKind::Ramp);
        let segs_id = bay.add_modulator(ModulatorKind::Segments);
        let vec4_id = bay.add_modulator(ModulatorKind::Vector4);
        let vec8_id = bay.add_modulator(ModulatorKind::Vector8);
        let transport = LfoTransportState::default();

        // 1. Random & SampleAndHold produce bounded outputs
        let r = bay.slots.iter_mut().find(|s| s.id == rand_id).unwrap();
        let rv = r.evaluate(&transport, 1.25)[0];
        assert!((-1.0..=1.0).contains(&rv));

        let sh = bay.slots.iter_mut().find(|s| s.id == sh_id).unwrap();
        sh.sh_source = 1; // Sine
        let shv = sh.evaluate(&transport, 1.25)[0];
        assert!((-1.0..=1.0).contains(&shv));

        // 2. Steps: FreeHz 1 Hz at t=2.1s => step index 2
        let st = bay.slots.iter_mut().find(|s| s.id == steps_id).unwrap();
        st.lfo_sync_mode = LfoSyncMode::FreeHz;
        st.lfo_rate_hz = 1.0;
        st.steps_len = 4;
        st.steps_smooth = 0.0;
        st.steps_values[2] = 0.77;
        let stv = st.evaluate(&transport, 2.1)[0];
        assert!((stv - 0.77).abs() < 1e-4, "Steps idx 2 expected 0.77, got {stv}");

        // 3. ParSeq-8: FreeHz 1 Hz at t=5.1s => out_idx 5 active, others 0
        let ps = bay.slots.iter_mut().find(|s| s.id == parseq_id).unwrap();
        ps.lfo_sync_mode = LfoSyncMode::FreeHz;
        ps.lfo_rate_hz = 1.0;
        ps.parseq_len = 8;
        ps.parseq_smooth = 0.0;
        ps.parseq_steps[5] = 0.88;
        let ps_outs = ps.evaluate(&transport, 5.1);
        assert!((ps_outs[5] - 0.88).abs() < 1e-4);
        assert!(ps_outs[0].abs() < 1e-4);

        // 4. Curves: 8 nodes, linear mode at phase=0.0 => node 0
        let crv = bay.slots.iter_mut().find(|s| s.id == curves_id).unwrap();
        crv.lfo_sync_mode = LfoSyncMode::FreeHz;
        crv.lfo_rate_hz = 1.0;
        crv.curves_smooth = false;
        crv.curves_nodes[0] = 0.42;
        let cv = crv.evaluate(&transport, 0.0)[0];
        assert!((cv - 0.42).abs() < 1e-4);

        // 5. Ramp: 2s linear ramp at t=1.0s => 0.5
        let rmp = bay.slots.iter_mut().find(|s| s.id == ramp_id).unwrap();
        rmp.ramp_time_secs = 2.0;
        rmp.ramp_curve = 0.0;
        rmp.ramp_loop = false;
        rmp.ramp_down = false;
        rmp.ramp_start_time = 0.0;
        assert!((rmp.evaluate(&transport, 1.0)[0] - 0.5).abs() < 1e-4);
        assert!((rmp.evaluate(&transport, 3.0)[0] - 1.0).abs() < 1e-4);

        // 6. Segments: 2s linear MSEG, at t=0.4s (pos=1.0) => seg_levels[1]
        let sg = bay.slots.iter_mut().find(|s| s.id == segs_id).unwrap();
        sg.seg_time_secs = 2.0;
        sg.seg_curve = 0.0;
        sg.seg_loop = false;
        sg.seg_levels = [0.0, 0.9, 0.5, 0.7, 0.2, 0.0];
        assert!((sg.evaluate(&transport, 0.4)[0] - 0.9).abs() < 1e-4);

        // 7. Vector-4: bilinear corner weights sum to 1.0
        let v4 = bay.slots.iter_mut().find(|s| s.id == vec4_id).unwrap();
        v4.xy_x = 0.25;
        v4.xy_y = 0.75;
        let v4_outs = v4.evaluate(&transport, 0.0);
        let sum4: f32 = v4_outs[..4].iter().sum();
        assert!((sum4 - 1.0).abs() < 1e-4);
        assert!((v4_outs[0] - 0.5625).abs() < 1e-4); // (1-0.25)*0.75

        // 8. Vector-8: trilinear cube weights sum to 1.0 across all 8 outputs
        let v8 = bay.slots.iter_mut().find(|s| s.id == vec8_id).unwrap();
        v8.xy_x = 0.5;
        v8.xy_y = 0.5;
        v8.vec8_z = 0.25;
        let v8_outs = v8.evaluate(&transport, 0.0);
        let sum8: f32 = v8_outs[..8].iter().sum();
        assert!((sum8 - 1.0).abs() < 1e-4);
        assert!((v8_outs[0] - 0.1875).abs() < 1e-4); // 0.25 * 0.75
        assert!((v8_outs[4] - 0.0625).abs() < 1e-4); // 0.25 * 0.25

        // Persistence roundtrip for all 9
        let saved = bay.to_project_params(true);
        let mut restored = DeviceModulatorBay::empty();
        restored.apply_project_params(&saved);
        assert_eq!(restored.slots.len(), 9);
        let r_v8 = restored.slots.iter().find(|s| s.kind == ModulatorKind::Vector8).unwrap();
        assert!((r_v8.vec8_z - 0.25).abs() < 1e-4);
        let r_ps = restored.slots.iter().find(|s| s.kind == ModulatorKind::ParSeq8).unwrap();
        assert!((r_ps.parseq_steps[5] - 0.88).abs() < 1e-4);
    }

    #[test]
    fn test_block3_modulators_envelopes_note_midi_voice() {
        let mut bay = DeviceModulatorBay::empty();
        let adsr_id = bay.add_modulator(ModulatorKind::Adsr);
        let ahdsr_id = bay.add_modulator(ModulatorKind::Ahdsr);
        let s4_id = bay.add_modulator(ModulatorKind::FourStage);
        let ahd_rel_id = bay.add_modulator(ModulatorKind::AhdOnRelease);
        let expr_id = bay.add_modulator(ModulatorKind::Expressions);
        let midi_id = bay.add_modulator(ModulatorKind::Midi);
        let kt_id = bay.add_modulator(ModulatorKind::KeytrackPlus);
        let rkt_id = bay.add_modulator(ModulatorKind::RelativeKeytrack);
        let p12_id = bay.add_modulator(ModulatorKind::Pitch12);
        let nc_id = bay.add_modulator(ModulatorKind::NoteCounter);
        let nsc_id = bay.add_modulator(ModulatorKind::NoteSidechain);
        let vib_id = bay.add_modulator(ModulatorKind::Vibrato);
        let vc_id = bay.add_modulator(ModulatorKind::VoiceControl);
        let sp_id = bay.add_modulator(ModulatorKind::StackSpread);
        let transport = LfoTransportState::default();

        // ADSR: Attack 0.1s -> at t=0.05s => 0.5; after decay => sustain 0.6
        let adsr = bay.slots.iter_mut().find(|s| s.id == adsr_id).unwrap();
        adsr.env_attack = 0.1;
        adsr.env_decay = 0.2;
        adsr.env_sustain = 0.6;
        adsr.trigger_gate(true, 1.0);
        assert!((adsr.evaluate(&transport, 1.05)[0] - 0.5).abs() < 1e-3);
        assert!((adsr.evaluate(&transport, 1.50)[0] - 0.6).abs() < 1e-3);

        // AHDSR: Attack 0.1s + Hold 0.2s -> at t=1.2s still 1.0
        let ahdsr = bay.slots.iter_mut().find(|s| s.id == ahdsr_id).unwrap();
        ahdsr.env_attack = 0.1;
        ahdsr.env_hold = 0.2;
        ahdsr.env_decay = 0.2;
        ahdsr.trigger_gate(true, 1.0);
        assert!((ahdsr.evaluate(&transport, 1.2)[0] - 1.0).abs() < 1e-3);

        // 4-Stage
        let s4 = bay.slots.iter_mut().find(|s| s.id == s4_id).unwrap();
        s4.env_4stage_times = [0.1, 0.1, 0.1, 0.1];
        s4.env_4stage_levels = [0.8, 0.4, 0.9, 0.0];
        s4.trigger_gate(true, 0.0);
        assert!((s4.evaluate(&transport, 0.1)[0] - 0.8).abs() < 1e-3);

        // AHD on Release: fires when gate turns off
        let ahdr = bay.slots.iter_mut().find(|s| s.id == ahd_rel_id).unwrap();
        ahdr.env_attack = 0.1;
        ahdr.env_hold = 0.1;
        ahdr.env_decay = 0.2;
        ahdr.trigger_gate(true, 0.0);
        assert_eq!(ahdr.evaluate(&transport, 0.5)[0], 0.0);
        ahdr.trigger_gate(false, 1.0);
        assert!((ahdr.evaluate(&transport, 1.15)[0] - 1.0).abs() < 1e-3);

        // Expressions & MIDI 4-output
        let expr = bay.slots.iter_mut().find(|s| s.id == expr_id).unwrap();
        expr.expr_vel = 0.9;
        expr.expr_timbre = 0.4;
        assert!((expr.evaluate(&transport, 0.0)[0] - 0.9).abs() < 1e-4);
        assert!((expr.evaluate(&transport, 0.0)[1] - 0.4).abs() < 1e-4);

        let midi = bay.slots.iter_mut().find(|s| s.id == midi_id).unwrap();
        midi.midi_pb = -0.5;
        assert!((midi.evaluate(&transport, 0.0)[2] - (-0.5)).abs() < 1e-4);

        // Keytrack+ & Relative Keytrack & Pitch-12
        let kt = bay.slots.iter_mut().find(|s| s.id == kt_id).unwrap();
        kt.key_root = 60;
        kt.key_spread_semi = 24.0;
        kt.key_curve = 0.0;
        kt.key_note = 72; // +12 semi => +0.5
        assert!((kt.evaluate(&transport, 0.0)[0] - 0.5).abs() < 1e-4);

        let rkt = bay.slots.iter_mut().find(|s| s.id == rkt_id).unwrap();
        rkt.key_prev_note = 60;
        rkt.key_note = 48; // -12 / 24 => -0.5
        rkt.key_spread_semi = 24.0;
        assert!((rkt.evaluate(&transport, 0.0)[0] - (-0.5)).abs() < 1e-4);

        let p12 = bay.slots.iter_mut().find(|s| s.id == p12_id).unwrap();
        p12.key_note = 69; // A4 -> pitch class 9
        let p12_outs = p12.evaluate(&transport, 0.0);
        assert_eq!(p12_outs[9], 1.0);
        assert_eq!(p12_outs[0], 0.0);

        // Note Counter (5 steps: count=2 => 2/4 = 0.5)
        let nc = bay.slots.iter_mut().find(|s| s.id == nc_id).unwrap();
        nc.nc_steps = 5;
        nc.nc_mode = 0;
        nc.nc_count = 2;
        assert!((nc.evaluate(&transport, 0.0)[0] - 0.5).abs() < 1e-4);

        // Note Sidechain & Vibrato
        let nsc = bay.slots.iter_mut().find(|s| s.id == nsc_id).unwrap();
        nsc.nsc_min_note = 36;
        nsc.nsc_max_note = 48;
        nsc.key_note = 40;
        nsc.env_attack = 0.05;
        nsc.trigger_gate(true, 0.0);
        assert!(nsc.evaluate(&transport, 0.05)[0] > 0.9);

        let vib = bay.slots.iter_mut().find(|s| s.id == vib_id).unwrap();
        vib.vib_rate_hz = 4.0;
        vib.vib_depth = 1.0;
        vib.vib_fade_secs = 0.0;
        vib.vib_mw_scale = 1.0;
        assert!((vib.evaluate(&transport, 0.0625)[0] - 1.0).abs() < 1e-3);

        // Voice Control & Stack Spread
        let vc = bay.slots.iter_mut().find(|s| s.id == vc_id).unwrap();
        vc.voice_index = 2;
        vc.voice_count = 5;
        assert!((vc.evaluate(&transport, 0.0)[0] - 0.5).abs() < 1e-4);

        let sp = bay.slots.iter_mut().find(|s| s.id == sp_id).unwrap();
        sp.voice_index = 4;
        sp.voice_count = 5;
        sp.spread_mode = 0;
        sp.spread_amount = 0.8;
        assert!((sp.evaluate(&transport, 0.0)[0] - 0.8).abs() < 1e-4);

        // Persistence roundtrip for all 14
        let saved = bay.to_project_params(true);
        let mut restored = DeviceModulatorBay::empty();
        restored.apply_project_params(&saved);
        assert_eq!(restored.slots.len(), 14);
        let r_p12 = restored.slots.iter().find(|s| s.kind == ModulatorKind::Pitch12).unwrap();
        assert_eq!(r_p12.key_note, 69);
    }

    #[test]
    fn test_block4_modulators_audio_wt_ch16_globals_cv_and_full_catalog() {
        // 42 Device Bay modulators + OpenLFO MOD device = 43/43 (100% Bitwig 5.1.7 Modulators)
        assert_eq!(MODULATOR_CATALOG.len(), 42);

        let mut bay = DeviceModulatorBay::empty();
        let wt_id = bay.add_modulator(ModulatorKind::WavetableLfo);
        let ch16_id = bay.add_modulator(ModulatorKind::Channel16);
        let glob_id = bay.add_modulator(ModulatorKind::Globals);
        let ef_id = bay.add_modulator(ModulatorKind::EnvelopeFollower);
        let asc_id = bay.add_modulator(ModulatorKind::AudioSidechain);
        let ar_id = bay.add_modulator(ModulatorKind::AudioRate);
        let cv_id = bay.add_modulator(ModulatorKind::HwCvIn);

        let transport = LfoTransportState {
            bpm: 140.0,
            sample_count: 22050,
            sample_rate: 44100.0,
            is_playing: true,
            playback_start_sample: 0,
        };

        // 1. Wavetable LFO: frame 0 (wt_index=0.0) is Sine -> at 1Hz, t=0.25s => +1.0
        let wt = bay.slots.iter_mut().find(|s| s.id == wt_id).unwrap();
        wt.lfo_rate_hz = 1.0;
        wt.wt_index = 0.0;
        wt.lfo_depth = 1.0;
        wt.lfo_bipolar = true;
        assert!((wt.evaluate(&transport, 0.25)[0] - 1.0).abs() < 1e-3);

        // 2. Channel-16: exclusive channel 11 (index 10)
        let ch = bay.slots.iter_mut().find(|s| s.id == ch16_id).unwrap();
        ch.ch16_active = 10;
        ch.ch16_levels[10] = 0.77;
        ch.ch16_exclusive = true;
        let ch_outs = ch.evaluate(&transport, 0.0);
        assert!((ch_outs[10] - 0.77).abs() < 1e-4);
        assert_eq!(ch_outs[0], 0.0);

        // 3. Globals: BPM=140 -> (140-40)/200 = 0.5, PLAY=1.0
        let gl = bay.slots.iter_mut().find(|s| s.id == glob_id).unwrap();
        let gl_outs = gl.evaluate(&transport, 0.0);
        assert!((gl_outs[0] - 0.5).abs() < 1e-4);
        assert_eq!(gl_outs[3], 1.0);

        // 4. Envelope Follower: tracks audio_input_level
        let ef = bay.slots.iter_mut().find(|s| s.id == ef_id).unwrap();
        ef.audio_gain = 1.0;
        ef.env_attack = 0.01;
        ef.audio_input_level = 0.8;
        let ef_out = ef.evaluate(&transport, 0.0)[0];
        assert!(ef_out > 0.5, "Envelope Follower should rise toward 0.8, got {ef_out}");

        // 5. Audio Sidechain: inverted ducking produces negative output
        let asc = bay.slots.iter_mut().find(|s| s.id == asc_id).unwrap();
        asc.audio_gain = 1.5;
        asc.audio_threshold = 0.2;
        asc.env_attack = 0.01;
        asc.audio_sc_invert = true;
        asc.audio_input_level = 0.8;
        assert!(asc.evaluate(&transport, 0.0)[0] < -0.5);

        // 6. Audio Rate: rectified output is non-negative
        let ar = bay.slots.iter_mut().find(|s| s.id == ar_id).unwrap();
        ar.ar_freq_hz = 100.0;
        ar.ar_rectify = true;
        let ar_out = ar.evaluate(&transport, 0.0075)[0];
        assert!((0.0..=1.0).contains(&ar_out));

        // 7. HW CV In: gain * level + offset
        let cv = bay.slots.iter_mut().find(|s| s.id == cv_id).unwrap();
        cv.cv_channel = 3;
        cv.cv_level = 0.5;
        cv.audio_gain = 1.0;
        cv.cv_offset = 0.25;
        cv.cv_bipolar = true;
        assert!((cv.evaluate(&transport, 0.0)[0] - 0.75).abs() < 1e-4);

        // Roundtrip persistence
        let saved = bay.to_project_params(true);
        let mut restored = DeviceModulatorBay::empty();
        restored.apply_project_params(&saved);
        assert_eq!(restored.slots.len(), 7);
        let r_cv = restored.slots.iter().find(|s| s.kind == ModulatorKind::HwCvIn).unwrap();
        assert_eq!(r_cv.cv_channel, 3);
        assert!((r_cv.cv_offset - 0.25).abs() < 1e-4);
    }

    #[test]
    fn external_plugin_param_modulation_routes_evaluate_persist_and_clear() {
        let tr = 812usize;
        let sl = 2usize;
        let pid = 42u32;

        clear_bay_for_slot(tr, sl);
        ensure_empty_bay_for_slot(tr, sl);
        let bay = get_bay_state(tr, sl);
        assert!(bay.slots.is_empty(), "external plugin bay starts empty with 3 '+' cells");

        let mod_id = add_bay_modulator(tr, sl, ModulatorKind::Macro);
        update_bay_state(tr, sl, |b| {
            let m = b.slots.iter_mut().find(|s| s.id == mod_id).unwrap();
            m.macro_value = 0.8;
            m.macro_bipolar = false;
        });

        let src = ModSourceId::DeviceBay {
            track_idx: tr,
            slot_idx: sl,
            mod_id,
            out_idx: 0,
        };
        set_external_route_amount(src, sl, pid, 0.5);
        assert!((get_external_route_amount(src, sl, pid) - 0.5).abs() < 1e-4);
        assert!(has_active_external_modulations(tr, sl));

        let active = get_external_param_modulation(tr, sl, pid)
            .expect("Macro routing should produce live modulation on external param");
        assert!((active.delta - 0.4).abs() < 1e-3, "0.8 * 0.5 = 0.4, got {}", active.delta);
        assert!((active.amount - 0.5).abs() < 1e-4);

        // Save/restore roundtrip for ModulationTarget::ExternalParam
        let target = ModulationTarget::ExternalParam {
            slot_idx: sl,
            param_id: pid,
        };
        let key = target.save_key();
        assert_eq!(key, "ext:2:42");
        assert_eq!(
            ModulationTarget::parse_saved(&key, ModulationTarget::None),
            target
        );

        let saved_bay = capture_bay_params(tr, sl, true).unwrap();
        let mut restored = DeviceModulatorBay::empty();
        restored.apply_project_params(&saved_bay);
        assert_eq!(restored.slots[0].route_amount(0, target), 0.5);

        clear_external_param_routes(tr, sl, pid);
        assert!(get_external_param_modulation(tr, sl, pid).is_none());
        assert!(!has_active_external_modulations(tr, sl));
        clear_bay_for_slot(tr, sl);
    }
}
