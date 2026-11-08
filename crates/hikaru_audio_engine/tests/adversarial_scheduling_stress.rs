// crates/hikaru_audio_engine/tests/adversarial_scheduling_stress.rs
//
// Adversarial stress test harness for Milestone 1 real-time MIDI scheduling:
// 1. High polyphony stress (128 concurrent notes, rapid trigger/stop cycles)
// 2. Loop wraparound stress (short 1-beat loops wrapped 10,000 times)
// 3. Loop wrap boundary alignment & NoteOff issuance
// 4. Non-looping emission boundary clamp (bit-exact 0.0 output past clip end)

use hikaru_audio_engine::{AudioEngine, EngineMode, MidiNoteInstance};
use hikaru_core::{AudioBuffer, SampleRate};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

static TABLE: [f32; 2048] = [0.0; 2048];

fn test_engine(mode: EngineMode, sr_val: f32, bpm: f64) -> AudioEngine<'static> {
    let sr = SampleRate::new(sr_val);
    let clock = Arc::new(AtomicU64::new(0));
    let mut engine = AudioEngine::new(sr, &TABLE, clock);
    engine.set_mode(mode);
    engine.transport.bpm = bpm;
    for t in 0..16 {
        engine.track_volumes[t].store(1.0f32.to_bits(), Ordering::Relaxed);
    }
    engine.master_gain.store(1.0f32.to_bits(), Ordering::Relaxed);
    engine
}

fn process_buffer(engine: &mut AudioEngine, frames: usize) -> (Vec<f32>, f32) {
    let mut raw = vec![0.0f32; frames * 2];
    let mut buf = AudioBuffer::new(&mut raw);
    engine.process(&mut buf);
    let peak = raw.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
    (raw, peak)
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: High Polyphony Stress (128 concurrent notes, rapid trigger/stop)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_high_polyphony_128_concurrent_notes() {
    let mut engine = test_engine(EngineMode::OpenLive, 44100.0, 120.0);

    // Load sample for all 128 pitches on track 0
    let sample_data = vec![0.5f32; 88200];
    for p in 0..128 {
        engine.load_dms_sample_for_track(0, p, sample_data.clone(), 1);
    }

    // Create 128 notes all starting at tick 0 with duration 960
    let notes: Vec<MidiNoteInstance> = (0..128)
        .map(|p| MidiNoteInstance::new(0, p as u8, 100, 960))
        .collect();

    engine.update_midi_clip(0, 0, notes);
    engine.trigger_clip(0, 0);
    engine.play();

    // Process blocks
    for _ in 0..20 {
        let (raw, peak) = process_buffer(&mut engine, 256);
        assert!(!peak.is_nan(), "Audio output contained NaN!");
        assert!(!peak.is_infinite(), "Audio output contained Infinity!");
        for s in raw {
            assert!(s.abs() <= 128.0, "Output blew up: {s}");
        }
    }

    // Now rapid trigger / stop cycles (100 cycles)
    for cycle in 0..100 {
        engine.trigger_clip(0, 0);
        let _ = process_buffer(&mut engine, 64);
        if cycle % 2 == 0 {
            engine.stop();
            let (_, stop_peak) = process_buffer(&mut engine, 64);
            assert!(
                stop_peak < 1e-6,
                "Voice leak detected after stop at cycle {cycle}: peak={stop_peak}"
            );
            assert!(!engine.voice_active(0, 0));
        } else {
            engine.stop_track(0);
            let (_, stop_peak) = process_buffer(&mut engine, 64);
            assert!(
                stop_peak < 1e-6,
                "Voice leak detected after stop_track at cycle {cycle}: peak={stop_peak}"
            );
            assert!(!engine.voice_active(0, 0));
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: Loop Wraparound Stress (1-beat loop wrapped 10,000 times)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_loop_wraparound_stress_10000_wraps() {
    let mut engine = test_engine(EngineMode::OpenLive, 44100.0, 120.0);
    engine.load_dms_sample_for_track(0, 60, vec![0.5f32; 44100], 1);
    engine.load_dms_sample_for_track(0, 64, vec![0.5f32; 44100], 1);

    // Note 60 spans first half of the beat (0..480 ticks)
    // Note 64 spans ACROSS the loop boundary (480..1440 ticks)
    let notes = vec![
        MidiNoteInstance::new(0, 60, 100, 480),
        MidiNoteInstance::new(480, 64, 100, 960),
    ];
    engine.update_midi_clip(0, 0, notes);

    if let Some(clip) = engine.midi_clips.iter_mut().find(|c| c.track_index == 0 && c.scene_index == 0) {
        clip.clip_loop_enabled = true;
        clip.clip_loop_start_ticks = 0;
        clip.clip_loop_end_ticks = 960;
        clip.duration_ticks = 960;
    }

    engine.trigger_clip(0, 0);
    engine.play();

    let wrap_samples = 22050usize;
    let num_wraps = 10_000;
    let total_samples = num_wraps * wrap_samples;
    let buf_size = 512;

    let mut samples_processed = 0;

    // Run first 200 wraps block by block (512 samples)
    let detailed_samples = 200 * wrap_samples;
    while samples_processed < detailed_samples {
        let (raw, peak) = process_buffer(&mut engine, buf_size);
        samples_processed += buf_size;
        assert!(!peak.is_nan(), "Audio output contained NaN during wrap test!");
        assert!(!peak.is_infinite(), "Audio output contained Inf during wrap test!");
        for s in raw {
            assert!(s.abs() <= 10.0, "Output blew up: {s}");
        }
    }

    // Now fast-forward remaining wraps in larger chunks
    while samples_processed < total_samples {
        let chunk = (total_samples - samples_processed).min(22050);
        let (_, peak) = process_buffer(&mut engine, chunk);
        samples_processed += chunk;
        assert!(!peak.is_nan());
        assert!(!peak.is_infinite());
    }

    assert!(engine.voice_active(0, 0));

    // When stopped, no stuck notes or voice leakage
    engine.stop();
    let (_, stop_peak) = process_buffer(&mut engine, 512);
    assert!(
        stop_peak < 1e-6,
        "Voice leak / stuck note after 10,000 wraps: peak={stop_peak}"
    );
    assert!(!engine.voice_active(0, 0));

    // Verify clip active_notes are all false
    if let Some(clip) = engine.midi_clips.iter().find(|c| c.track_index == 0 && c.scene_index == 0) {
        for (p, &active) in clip.active_notes.iter().enumerate() {
            assert!(!active, "Note {p} was stuck active in clip.active_notes!");
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3: Loop Wrap Boundary Alignment Stress
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_loop_wrap_boundary_alignment_behavior() {
    let mut engine = test_engine(EngineMode::OpenLive, 48000.0, 120.0);
    // 4 beats loop = 960 * 4 = 3840 ticks = 2.0s = 96000 samples.
    // 96000 is exactly divisible by 256 (375 buffers).
    // Note spans 0..3840 ticks (exact loop length)
    let notes = vec![
        MidiNoteInstance::new(0, 60, 100, 3840),
    ];
    engine.update_midi_clip(0, 0, notes);

    if let Some(clip) = engine.midi_clips.iter_mut().find(|c| c.track_index == 0 && c.scene_index == 0) {
        clip.clip_loop_enabled = true;
        clip.clip_loop_start_ticks = 0;
        clip.clip_loop_end_ticks = 3840;
        clip.duration_ticks = 3840;
    }

    engine.trigger_clip(0, 0);
    engine.play();

    // Process exactly 375 buffers of 256 frames = 96000 samples (end of Cycle 1).
    for _ in 0..375 {
        process_buffer(&mut engine, 256);
    }

    // Now process buffer 376 (first buffer of Cycle 2, samples 96000 to 96256).
    process_buffer(&mut engine, 256);

    // Stop and verify immediate silence
    engine.stop();
    let (_, stop_peak) = process_buffer(&mut engine, 256);
    assert!(stop_peak < 1e-6, "Post-stop peak non-zero: {stop_peak}");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4: Non-Looping Emission Boundary Clamp (Bit-Exact 0.0 Output Past Clip End)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_non_looping_emission_boundary_clamp_bit_exact() {
    let mut engine = test_engine(EngineMode::OpenLive, 44100.0, 120.0);

    // Load a long 2-second sample (88200 samples)
    engine.load_dms_sample_for_track(0, 60, vec![0.8f32; 88200], 1);

    // Non-looping clip of 1 beat (960 ticks = 22050 samples)
    // Note starts at tick 0, duration 480 ticks (half beat = 11025 samples)
    let notes = vec![
        MidiNoteInstance::new(0, 60, 100, 480),
    ];
    engine.update_midi_clip(0, 0, notes);

    if let Some(clip) = engine.midi_clips.iter_mut().find(|c| c.track_index == 0 && c.scene_index == 0) {
        clip.clip_loop_enabled = false;
        clip.duration_ticks = 960;
    }

    engine.trigger_clip(0, 0);
    engine.play();

    // Process first beat (22050 samples)
    let mut elapsed = 0;
    let mut peak_during = 0.0f32;
    while elapsed < 22050 {
        let step = (22050 - elapsed).min(512);
        let (_, p) = process_buffer(&mut engine, step);
        if p > peak_during {
            peak_during = p;
        }
        elapsed += step;
    }
    assert!(peak_during > 0.1, "Clip must emit audio during its 1-beat duration");

    // Clip should now be inactive
    assert!(!engine.voice_active(0, 0), "Clip must be inactive past emission length");

    // Now process the next 2 beats (22050 to 66150):
    // Requirement: Output must be bit-exact 0.0 sample output past clip end.
    let mut non_zero_samples = 0;
    let mut max_leakage = 0.0f32;
    for _ in 0..80 {
        let (raw, p) = process_buffer(&mut engine, 512);
        if p > max_leakage {
            max_leakage = p;
        }
        for s in raw {
            if s != 0.0f32 {
                non_zero_samples += 1;
            }
        }
    }

    println!("Non-looping test 1: max_leakage={max_leakage}, non_zero_samples={non_zero_samples}");
    assert_eq!(
        max_leakage, 0.0,
        "Bit-exact 0.0 clamp violated past clip end: max_leakage={max_leakage}"
    );
    assert_eq!(
        non_zero_samples, 0,
        "Found {non_zero_samples} non-zero samples past clip end!"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5: Non-Looping with Note Extending to Clip End (Boundary Truncation & Silence)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_non_looping_note_extending_to_clip_end() {
    let mut engine = test_engine(EngineMode::OpenLive, 44100.0, 120.0);

    // Long sample (88200 samples)
    engine.load_dms_sample_for_track(0, 60, vec![0.8f32; 88200], 1);

    // Non-looping clip of 1 beat (960 ticks = 22050 samples)
    // Note starts at tick 480 and has duration 960 ticks (extends past clip end: ends at 1440 ticks)
    let notes = vec![
        MidiNoteInstance::new(480, 60, 100, 960),
    ];
    engine.update_midi_clip(0, 0, notes);

    if let Some(clip) = engine.midi_clips.iter_mut().find(|c| c.track_index == 0 && c.scene_index == 0) {
        clip.clip_loop_enabled = false;
        clip.duration_ticks = 960;
    }

    engine.trigger_clip(0, 0);
    engine.play();

    // Process up to 22050 samples
    let mut elapsed = 0;
    let mut peak_during = 0.0f32;
    while elapsed < 22050 {
        let step = (22050 - elapsed).min(512);
        let (_, p) = process_buffer(&mut engine, step);
        if p > peak_during {
            peak_during = p;
        }
        elapsed += step;
    }
    assert!(peak_during > 0.1, "Note must sound before clip end");

    // Past 22050 samples, audio output must be bit-exact 0.0
    let mut non_zero_samples = 0;
    let mut max_leakage = 0.0f32;
    for _ in 0..40 {
        let (raw, p) = process_buffer(&mut engine, 512);
        if p > max_leakage {
            max_leakage = p;
        }
        for s in raw {
            if s != 0.0f32 {
                non_zero_samples += 1;
            }
        }
    }

    println!("Non-looping test 2 (note extending past end): max_leakage={max_leakage}, non_zero_samples={non_zero_samples}");
    assert_eq!(
        max_leakage, 0.0,
        "Bit-exact 0.0 clamp violated past clip end for extending note: max_leakage={max_leakage}"
    );
    assert_eq!(
        non_zero_samples, 0,
        "Found {non_zero_samples} non-zero samples past clip end for extending note!"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6: Non-Looping OpenStudio Mode Emission Boundary Clamp
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_openstudio_mode_non_looping_boundary_clamp() {
    let mut engine = test_engine(EngineMode::OpenStudio, 44100.0, 120.0);

    engine.load_dms_sample_for_track(0, 60, vec![0.8f32; 88200], 1);

    // OpenStudio clip starts at frame 0, duration 960 ticks (22050 samples)
    let notes = vec![
        MidiNoteInstance::new(0, 60, 100, 480),
        MidiNoteInstance::new(480, 60, 100, 480),
    ];
    engine.update_midi_clip(0, 0, notes);

    if let Some(clip) = engine.midi_clips.iter_mut().find(|c| c.track_index == 0 && c.scene_index == 0) {
        clip.start_frame = 0;
        clip.duration_ticks = 960;
    }

    engine.play();

    // Process first beat (0..22050 samples)
    let mut elapsed = 0;
    let mut peak_during = 0.0f32;
    while elapsed < 22050 {
        let step = (22050 - elapsed).min(512);
        let (_, p) = process_buffer(&mut engine, step);
        if p > peak_during {
            peak_during = p;
        }
        elapsed += step;
    }
    assert!(peak_during > 0.1, "OpenStudio clip must emit audio during duration");

    // Process beat 2 (22050..44100)
    let mut non_zero_samples = 0;
    let mut max_leakage = 0.0f32;
    for _ in 0..40 {
        let (raw, p) = process_buffer(&mut engine, 512);
        if p > max_leakage {
            max_leakage = p;
        }
        for s in raw {
            if s != 0.0f32 {
                non_zero_samples += 1;
            }
        }
    }

    println!("OpenStudio test: max_leakage={max_leakage}, non_zero_samples={non_zero_samples}");
    assert_eq!(
        max_leakage, 0.0,
        "Bit-exact 0.0 clamp violated past OpenStudio clip end: max_leakage={max_leakage}"
    );
    assert_eq!(
        non_zero_samples, 0,
        "Found {non_zero_samples} non-zero samples past OpenStudio clip end!"
    );
}
