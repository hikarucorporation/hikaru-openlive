// crates/hikaru_audio_engine/tests/tier_s_fx_e2e.rs

use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use hikaru_audio_engine::AudioEngine;
use hikaru_core::SampleRate;

#[test]
fn tier_s_native_effects_load_in_engine() {
    let wt: Vec<f32> = (0..2048).map(|i| (i as f32 / 1024.0) - 1.0).collect();
    let clock = Arc::new(AtomicU64::new(0));
    let mut engine = AudioEngine::new(SampleRate::new(48_000.0), &wt, clock);
    let names = [
        "Tool",
        "DC Offset",
        "Dual Pan",
        "Test Tone",
        "Time Shift",
        "Tremolo",
        "Ring-Mod",
        "Hikaru Open8-Bit",
        "Bit-8",
        "Hikaru OpenFreqShifter+",
        "Freq Shifter+",
        "Hikaru OpenPhaser+",
        "Phaser+",
        "Hikaru OpenFlanger+",
        "Flanger+",
    ];
    for (slot_idx, name) in names.iter().enumerate() {
        engine
            .load_native_effect(0, slot_idx, name)
            .unwrap_or_else(|e| panic!("Failed to load native effect '{name}': {e}"));
    }
}

#[test]
fn open_freq_shifter_plus_wet_fx_nested_chain_loads_and_updates_in_engine() {
    let wt: Vec<f32> = (0..2048).map(|i| (i as f32 / 1024.0) - 1.0).collect();
    let clock = Arc::new(AtomicU64::new(0));
    let mut engine = AudioEngine::new(SampleRate::new(48_000.0), &wt, clock);

    engine
        .load_native_effect(0, 0, "Hikaru OpenFreqShifter+")
        .expect("load OpenFreqShifter+");
    engine
        .load_bit8_wet_native_effect(0, 0, 0, "Tool")
        .expect("load Tool in OpenFreqShifter+ Wet FX");
    engine
        .load_bit8_wet_native_effect(0, 0, 1, "Ring-Mod")
        .expect("load Ring-Mod in OpenFreqShifter+ Wet FX");

    engine.set_bit8_wet_fx_param(0, 0, 0, 0, 0.75);
    engine.set_bit8_wet_fx_active(0, 0, 0, false);
    engine.remove_bit8_wet_fx(0, 0, 0);
}

#[test]
fn open8bit_wet_fx_nested_chain_loads_and_updates_in_engine() {
    let wt: Vec<f32> = (0..2048).map(|i| (i as f32 / 1024.0) - 1.0).collect();
    let clock = Arc::new(AtomicU64::new(0));
    let mut engine = AudioEngine::new(SampleRate::new(48_000.0), &wt, clock);

    engine
        .load_native_effect(0, 0, "Hikaru Open8-Bit")
        .expect("load Open8-Bit");
    engine
        .load_bit8_wet_native_effect(0, 0, 0, "Tool")
        .expect("load Tool in Wet FX");
    engine
        .load_bit8_wet_native_effect(0, 0, 1, "Ring-Mod")
        .expect("load Ring-Mod in Wet FX");

    engine.set_bit8_wet_fx_param(0, 0, 0, 0, 0.75);
    engine.set_bit8_wet_fx_active(0, 0, 0, false);
    engine.remove_bit8_wet_fx(0, 0, 0);
}
