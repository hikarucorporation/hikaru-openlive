use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use hikaru_audio_engine::{AudioEngine, EngineMode};
use hikaru_core::{AudioBuffer, SampleRate};

#[test]
fn ultracomb_native_effect_processes_track_audio_in_engine() {
    let wt: Vec<f32> = (0..2048).map(|i| (i as f32 / 1024.0) - 1.0).collect();
    let clock = Arc::new(AtomicU64::new(0));

    let mut dry_engine = AudioEngine::new(SampleRate::new(48000.0), &wt, clock.clone());
    dry_engine.set_mode(EngineMode::OpenLive);
    dry_engine.set_track_wavetable(0, wt.clone(), 0);
    dry_engine.play();
    dry_engine.trigger_virtual_note(0, 36, 1.0);

    let mut wet_engine = AudioEngine::new(SampleRate::new(48000.0), &wt, clock);
    wet_engine.set_mode(EngineMode::OpenLive);
    wet_engine.set_track_wavetable(0, wt.clone(), 0);
    wet_engine.load_native_effect(0, 1, "Hikaru Ultracomb").unwrap();
    wet_engine.set_external_plugin_param(0, 1, 0, 0.35);
    wet_engine.set_external_plugin_param(0, 1, 1, 1.0);
    wet_engine.play();
    wet_engine.trigger_virtual_note(0, 36, 1.0);

    let mut dry_samples = vec![0.0_f32; 1024 * 2];
    let mut wet_samples = vec![0.0_f32; 1024 * 2];
    {
        let mut dry_buf = AudioBuffer::new(&mut dry_samples);
        let mut wet_buf = AudioBuffer::new(&mut wet_samples);
        dry_engine.process(&mut dry_buf);
        wet_engine.process(&mut wet_buf);
    }

    let dry_energy: f32 = dry_samples.iter().map(|s| s * s).sum();
    let diff: f32 = dry_samples
        .iter()
        .zip(wet_samples.iter())
        .map(|(a, b)| (a - b).abs())
        .sum();
    assert!(dry_energy > 0.1, "Dry wavetable should produce audio (energy={dry_energy})");
    assert!(diff > 1.0, "Ultracomb should audibly alter track 0 output (diff={diff})");
}
