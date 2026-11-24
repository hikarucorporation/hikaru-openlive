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
    ];
    for (slot_idx, name) in names.iter().enumerate() {
        engine
            .load_native_effect(0, slot_idx, name)
            .unwrap_or_else(|e| panic!("Failed to load native effect '{name}': {e}"));
    }
}
