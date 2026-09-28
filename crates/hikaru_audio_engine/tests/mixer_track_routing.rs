// Regresión del bug "el mixer solo baja el volumen del Track 1 y el
// paneo no anda": las voces DMS (batería MIDI) se mezclaban con un único
// `dms_track_idx` global (= 0) y el paneo de la GUI (-1..1) se guardaba
// en escala -100..100 sin convertir (1.0 / 100 = 0.01 inaudible).
use hikaru_audio_engine::{AudioEngine, DmsAdsrParams};
use hikaru_core::{AudioBuffer, SampleRate};
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

static TABLE: [f32; 2048] = [0.0; 2048];

fn loud_engine() -> AudioEngine<'static> {
    let clock = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let engine = AudioEngine::new(SampleRate::new(44100.0), &TABLE, clock);
    for t in 0..16 {
        engine.track_volumes[t].store(1.0f32.to_bits(), Ordering::Relaxed);
    }
    engine.master_gain.store(1.0f32.to_bits(), Ordering::Relaxed);
    engine
}

fn instant_adsr(sr: f32) -> DmsAdsrParams {
    DmsAdsrParams {
        attack_rate: sr,
        decay_rate: sr,
        sustain_level: 1.0,
        release_rate: 0.0,
    }
}

fn dms_peak(engine: &mut AudioEngine, track_idx: usize, frames: usize) -> f32 {
    engine.load_dms_sample(0, vec![1.0f32; 44100], 1);
    let sr = engine.sample_rate;
    engine.trigger_dms_note(0, 1.0, 0.0, 1.0, 1.0, &instant_adsr(sr), track_idx);
    let mut raw = vec![0.0f32; frames * 2];
    let mut buf = AudioBuffer::new(&mut raw);
    engine.process(&mut buf);
    raw.iter().map(|v| v.abs()).fold(0.0f32, f32::max)
}

#[test]
fn dms_volume_follows_its_own_track_not_track_1() {
    // Pista 2 muteada por volumen: su voz debe ser silencio...
    let mut engine = loud_engine();
    engine.set_track_volume(1, 0.0);
    assert!(dms_peak(&mut engine, 1, 512) < 1e-6);

    // ...pero la pista 1 con volumen 1.0 suena con normalidad.
    let mut engine = loud_engine();
    assert!(dms_peak(&mut engine, 0, 512) > 0.1);
}

#[test]
fn track_pan_accepts_normalized_gui_range() {
    let engine = loud_engine();
    engine.set_track_pan(0, 1.0); // full-R en escala GUI -1..1
    let stored = f32::from_bits(engine.track_pans[0].load(Ordering::Relaxed));
    assert!((stored - 100.0).abs() < 1e-6, "pan 1.0 debe guardarse como 100, quedó {stored}");

    engine.set_track_pan(0, -0.5);
    let stored = f32::from_bits(engine.track_pans[0].load(Ordering::Relaxed));
    assert!((stored + 50.0).abs() < 1e-6, "pan -0.5 debe guardarse como -50, quedó {stored}");

    // Escala legacy -100..100 sigue funcionando tal cual.
    engine.set_track_pan(0, -100.0);
    let stored = f32::from_bits(engine.track_pans[0].load(Ordering::Relaxed));
    assert!((stored + 100.0).abs() < 1e-6);

    let _ = AtomicU32::new(0);
}

#[test]
fn track_pan_full_right_kills_left_channel_on_dms() {
    let mut engine = loud_engine();
    engine.set_track_pan(0, 1.0);
    engine.load_dms_sample(0, vec![1.0f32; 44100], 1);
    let sr = engine.sample_rate;
    engine.trigger_dms_note(0, 1.0, 0.0, 1.0, 1.0, &instant_adsr(sr), 0);
    let mut raw = vec![0.0f32; 512 * 2];
    let mut buf = AudioBuffer::new(&mut raw);
    engine.process(&mut buf);
    let peak_l: f32 = raw.iter().step_by(2).map(|v| v.abs()).fold(0.0, f32::max);
    let peak_r: f32 = raw.iter().skip(1).step_by(2).map(|v| v.abs()).fold(0.0, f32::max);
    assert!(peak_l < 1e-6, "full-R no debe sonar en L (pico L={peak_l})");
    assert!(peak_r > 0.1, "full-R debe sonar en R (pico R={peak_r})");
}
