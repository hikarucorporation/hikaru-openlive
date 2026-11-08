// crates/hikaru_audio_engine/tests/midi_clip_scheduling.rs
//
// Tests de integración para la programación en tiempo real de MIDI Clips,
// disparos sample-accurate, bucles cuantizados a compás y garantía
// absoluta de cero fuga de voz (anti-leakage / ghost voice) en Hikaru OpenLive.

use hikaru_audio_engine::{AudioEngine, EngineMode, MidiNoteInstance};
use hikaru_core::{AudioBuffer, SampleRate};
use hikaru_transport::TransportPlaybackState;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

static TABLE: [f32; 2048] = [0.0; 2048];

fn test_engine(mode: EngineMode) -> AudioEngine<'static> {
    let sr = SampleRate::new(44100.0);
    let clock = Arc::new(AtomicU64::new(0));
    let mut engine = AudioEngine::new(sr, &TABLE, clock);
    engine.set_mode(mode);
    engine.transport.bpm = 120.0; // 960 ticks/beat = 0.5s = 22050 samples/beat
    for t in 0..16 {
        engine.track_volumes[t].store(1.0f32.to_bits(), Ordering::Relaxed);
    }
    engine.master_gain.store(1.0f32.to_bits(), Ordering::Relaxed);
    engine
}

fn process_frames(engine: &mut AudioEngine, frames: usize) -> f32 {
    let mut raw = vec![0.0f32; frames * 2];
    let mut buf = AudioBuffer::new(&mut raw);
    engine.process(&mut buf);
    raw.iter().map(|v| v.abs()).fold(0.0f32, f32::max)
}

#[test]
fn test_midi_clip_playback_triggers_notes() {
    let mut engine = test_engine(EngineMode::OpenLive);

    // Cargar muestra de percusión para pad 36 (Kick) en Track 0
    engine.load_dms_sample_for_track(0, 36, vec![0.8f32; 22050], 1);

    // Crear clip MIDI en Track 0, Scene 0 con una nota en tick 0
    let notes = vec![
        MidiNoteInstance::new(0, 36, 100, 480),
        MidiNoteInstance::new(960, 36, 100, 480),
    ];
    engine.update_midi_clip(0, 0, notes);

    // Disparar clip e iniciar reproducción
    engine.trigger_clip(0, 0);
    assert!(engine.voice_active(0, 0));
    engine.play();
    assert_eq!(engine.transport.playback_state, TransportPlaybackState::Playing);

    // Procesar los primeros 512 frames: la nota en tick 0 debe sonar de inmediato
    let peak = process_frames(&mut engine, 512);
    assert!(peak > 0.1, "El clip MIDI debía emitir sonido al disparar la nota (peak={peak})");
}

#[test]
fn test_midi_clip_non_looping_past_emission_silent() {
    let mut engine = test_engine(EngineMode::OpenLive);

    // Cargar muestra corta para pad 36
    engine.load_dms_sample_for_track(0, 36, vec![0.8f32; 4000], 1);

    // Clip MIDI de 1 beat (960 ticks = 22050 samples a 120 BPM)
    let notes = vec![
        MidiNoteInstance::new(0, 36, 100, 240),
    ];
    engine.update_midi_clip(0, 0, notes);

    // Desactivar loop explícitamente y fijar duración a 1 beat
    if let Some(clip) = engine.midi_clips.iter_mut().find(|c| c.track_index == 0 && c.scene_index == 0) {
        clip.clip_loop_enabled = false;
        clip.duration_ticks = 960;
    }

    engine.trigger_clip(0, 0);
    engine.play();

    // Renderizar primer beat (22050 samples)
    let mut elapsed = 0;
    let mut peak_during_clip = 0.0f32;
    while elapsed < 22050 {
        let step = (22050 - elapsed).min(512);
        let p = process_frames(&mut engine, step);
        if p > peak_during_clip {
            peak_during_clip = p;
        }
        elapsed += step;
    }
    assert!(peak_during_clip > 0.1, "El clip debía sonar durante su ventana de emisión");

    // Verificar que una vez concluida la duración, el clip se desactivó
    assert!(!engine.voice_active(0, 0), "El clip no-looping debe marcarse inactivo tras su duración");

    // Renderizar segundo y tercer beat (samples 22050 a 66150):
    // Cero absoluto garantizado (< 1e-6) — Anti-leakage / Zero ghost voice.
    let mut peak_after_clip = 0.0f32;
    for _ in 0..40 {
        let p = process_frames(&mut engine, 512);
        if p > peak_after_clip {
            peak_after_clip = p;
        }
    }
    assert!(
        peak_after_clip < 1e-6,
        "Fuga de voz detectada tras el fin del clip MIDI: peak={peak_after_clip}"
    );
}

#[test]
fn test_midi_clip_loop_wrap_no_stuck_notes() {
    let mut engine = test_engine(EngineMode::OpenLive);
    engine.load_dms_sample_for_track(0, 40, vec![0.5f32; 22050], 1);

    // Clip con loop de 1 beat (960 ticks)
    let notes = vec![
        MidiNoteInstance::new(0, 40, 100, 480),
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

    // Avanzar a través de 4 ciclos de loop (4 beats = 88200 samples)
    let mut total_rendered = 0;
    while total_rendered < 88200 {
        process_frames(&mut engine, 512);
        total_rendered += 512;
    }

    // El clip debe seguir activo y reproduciendo sin notas colgadas
    assert!(engine.voice_active(0, 0));

    // Si pausamos o frenamos, las notas activas deben morir de inmediato
    engine.stop();
    assert_eq!(engine.transport.playback_state, TransportPlaybackState::Stopped);
    assert!(!engine.voice_active(0, 0));

    let stop_peak = process_frames(&mut engine, 512);
    assert!(stop_peak < 1e-6, "Tras stop no debe haber notas colgadas: peak={stop_peak}");
}

#[test]
fn test_midi_clip_scene_trigger_phase_restart() {
    let mut engine = test_engine(EngineMode::OpenLive);
    engine.load_dms_sample_for_track(0, 36, vec![0.7f32; 22050], 1);

    let notes1 = vec![MidiNoteInstance::new(0, 36, 100, 480)];
    let notes2 = vec![MidiNoteInstance::new(0, 36, 120, 480)];

    engine.update_midi_clip(0, 0, notes1);
    engine.update_midi_clip(0, 1, notes2);

    engine.trigger_scene(0);
    engine.play();

    assert!(engine.voice_active(0, 0));
    assert!(!engine.voice_active(0, 1));

    // Procesar 2000 frames
    process_frames(&mut engine, 2000);

    // Disparar escena 1: debe reiniciar la fase y frenar escena 0
    engine.trigger_scene(1);
    assert!(!engine.voice_active(0, 0));
    assert!(engine.voice_active(0, 1));

    let clip1 = engine.midi_clips.iter().find(|c| c.track_index == 0 && c.scene_index == 1).unwrap();
    assert_eq!(clip1.start_absolute, engine.absolute_frame);
}

#[test]
fn test_midi_clip_openstudio_mode_scheduling() {
    let mut engine = test_engine(EngineMode::OpenStudio);
    engine.load_dms_sample_for_track(0, 38, vec![0.8f32; 10000], 1);

    // Clip MIDI colocado a partir del frame 22050 (inicio del compás 2)
    let notes = vec![MidiNoteInstance::new(0, 38, 100, 480)];
    let mut clip = hikaru_audio_engine::MidiClipInstance::new(0, 0, notes);
    clip.start_frame = 22050;
    clip.duration_ticks = 960; // 22050 samples de duración
    engine.midi_clips.push(clip);

    engine.play();

    // Durante compás 1 (0..22050): debe ser silencio absoluto
    let mut peak_before = 0.0f32;
    let mut rendered = 0;
    while rendered < 22050 {
        let step = (22050 - rendered).min(512);
        let p = process_frames(&mut engine, step);
        if p > peak_before {
            peak_before = p;
        }
        rendered += step;
    }
    assert!(peak_before < 1e-6, "Antes del inicio del clip en OpenStudio debe ser silencio (peak={peak_before})");

    // Al entrar a compás 2 (frame 22050..): la nota en tick 0 se activa
    let peak_during = process_frames(&mut engine, 512);
    assert!(peak_during > 0.1, "Al llegar al clip en OpenStudio la nota debe sonar (peak={peak_during})");
}
