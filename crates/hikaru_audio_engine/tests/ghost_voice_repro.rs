// Repro del bug reportado: clip termina en compás 5, del 5 al 8 (Time
// Selection activo, sin sample) la voz se seguía escuchando en loop.
// Exigencia: ni un milivoltio en el mixer fuera del clip sin loop explícito.
use hikaru_audio_engine::{AudioEngine, EngineMode};
use hikaru_core::{AudioBuffer, SampleRate};
use hikaru_transport::TransportPlaybackState;
use std::sync::{Arc, atomic::AtomicU64};

static TABLE: [f32; 2048] = [0.0; 2048];

fn engine_with_5bar_clip(mode: EngineMode, global_loop: Option<(u64, u64)>) -> (AudioEngine<'static>, u64, u64) {
    let sr = SampleRate::new(44100.0);
    let clock = Arc::new(AtomicU64::new(0));
    let mut engine = AudioEngine::new(sr, &TABLE, clock);
    engine.set_mode(mode);
    // Compás exacto con el transporte real del engine (BPM 128 por defecto).
    let spb = engine.transport.samples_per_bar();
    let bar5_start = (spb * 4.0).round() as u64; // compases 1-4 = 4 compases
    let bar8_end = (spb * 7.0).round() as u64; // hasta inicio del compás 8
    let five_bars = bar5_start; // el clip ocupa exactamente compás 1..5
    assert!(five_bars > 0);

    // Sample audible constante: cualquier fuga se detecta de inmediato.
    let samples = vec![0.5f32; (five_bars as usize) * 2]; // estéreo
    let duration_secs = five_bars as f32 / 44100.0;
    engine.add_clip(1, 0, 0, samples, 0.0, duration_secs, 0.0, 2, false);
    assert!(!engine.clips[0].has_valid_clip_loop());

    if mode == EngineMode::OpenLive {
        engine.transport.sample_count = 0;
        engine.trigger_clip(0, 0);
        assert!(engine.clips[0].is_playing);
    }
    if let Some((s, e)) = global_loop {
        engine.set_global_loop(s, e, true);
    }
    engine.play();
    assert_eq!(engine.transport.playback_state, TransportPlaybackState::Playing);
    (engine, bar5_start, bar8_end)
}

fn max_abs_in_range(engine: &mut AudioEngine, from: u64, frames: u64) -> f32 {
    let mut peak = 0.0f32;
    // Avanzar con buffers de 512 frames hasta `from` (descartar audio).
    // Solo cuando NO hay loop global activo (si lo hay, el cursor hace wrap
    // y nunca "llega": se posiciona por seek directo). Las voces OpenLive
    // viven en el reloj LINEAL (`absolute_frame`, sin wrap): al hacer seek
    // del cursor se avanza también el reloj lineal al mismo punto, que es
    // el estado real tras haber renderizado 0..`from` (voz agotada si
    // `from` supera su emisión).
    if !engine.transport.has_valid_loop() {
        while engine.transport.sample_count < from {
            let step = (from - engine.transport.sample_count).min(512);
            let mut raw = vec![0.0f32; (step * 2) as usize];
            let mut buf = AudioBuffer::new(&mut raw);
            engine.process(&mut buf);
        }
    } else {
        engine.transport.sample_count = from;
        engine.absolute_frame = from;
    }
    // Medir exactamente `frames` frames de audio: ni un milivoltio permitido.
    // (Conteo por frames, no por cursor: con loop global el cursor hace wrap.)
    let mut remaining = frames;
    while remaining > 0 {
        let step = remaining.min(512);
        let mut raw = vec![0.0f32; (step * 2) as usize];
        let mut buf = AudioBuffer::new(&mut raw);
        engine.process(&mut buf);
        for s in raw.iter() {
            peak = peak.max(s.abs());
        }
        remaining -= step;
    }
    peak
}

#[test]
fn repro_openstudio_bars5_to_8_are_silent_no_loop() {
    let (mut engine, bar5, bar8) = engine_with_5bar_clip(EngineMode::OpenStudio, None);
    let peak = max_abs_in_range(&mut engine, bar5, bar8 - bar5);
    assert_eq!(peak, 0.0, "BUG VIVO: fuga de {} en compases 5-8 (OpenStudio sin loop)", peak);
}

#[test]
fn repro_openstudio_bars5_to_8_silent_with_time_selection_loop() {
    // Time Selection = compases 5..8 con loop global activo.
    let sr = SampleRate::new(44100.0);
    let clock = Arc::new(AtomicU64::new(0));
    let tmp = AudioEngine::new(sr, &TABLE, clock);
    let spb = tmp.transport.samples_per_bar();
    let bar5 = (spb * 4.0).round() as u64;
    let bar8 = (spb * 7.0).round() as u64;
    drop(tmp);
    let (mut engine, _, _) =
        engine_with_5bar_clip(EngineMode::OpenStudio, Some((bar5, bar8)));
    let peak = max_abs_in_range(&mut engine, bar5, bar8 - bar5);
    assert_eq!(peak, 0.0, "BUG VIVO: fuga de {} en Time Selection 5-8 (OpenStudio)", peak);
}

#[test]
fn repro_openlive_bars5_to_8_are_silent_no_clip_loop() {
    let (mut engine, bar5, bar8) = engine_with_5bar_clip(EngineMode::OpenLive, None);
    // voice_frame directo: pasado el natural debe ser None (silencio).
    let t = engine.transport;
    assert_eq!(engine.clips[0].voice_frame(bar5, &t), None);
    assert_eq!(engine.clips[0].voice_frame(bar5 + 1000, &t), None);
    let peak = max_abs_in_range(&mut engine, bar5, bar8 - bar5);
    assert_eq!(peak, 0.0, "BUG VIVO: fuga de {} en compases 5-8 (OpenLive sin clip-loop)", peak);
}

#[test]
fn repro_openlive_bars5_to_8_silent_with_time_selection_loop() {
    let sr = SampleRate::new(44100.0);
    let clock = Arc::new(AtomicU64::new(0));
    let tmp = AudioEngine::new(sr, &TABLE, clock);
    let spb = tmp.transport.samples_per_bar();
    let bar5 = (spb * 4.0).round() as u64;
    let bar8 = (spb * 7.0).round() as u64;
    drop(tmp);
    let (mut engine, _, _) = engine_with_5bar_clip(EngineMode::OpenLive, Some((bar5, bar8)));
    let peak = max_abs_in_range(&mut engine, bar5, bar8 - bar5);
    assert_eq!(peak, 0.0, "BUG VIVO: fuga de {} en Time Selection 5-8 (OpenLive)", peak);
}

#[test]
fn bar6_is_minus_inf_db_no_ghost_voice() {
    // Exigencia del reporte: clip termina en compás 5, playhead en compás
    // 6-7 sin datos → vúmetro en -inf dB (pico 0.0), voz dormida.
    for mode in [EngineMode::OpenStudio, EngineMode::OpenLive] {
        let (mut engine, bar5, _) = engine_with_5bar_clip(mode, None);
        let spb = engine.transport.samples_per_bar();
        let bar6 = bar5 + (spb.round() as u64);
        if mode == EngineMode::OpenLive {
            // Voces OpenLive viven en el reloj LINEAL: para llevar el
            // playhead al compás 6 hay que RENDERIZAR 0..bar6 (la voz se
            // agota en bar5). Un seek del cursor no adelanta la voz.
            let mut remaining = bar6;
            while remaining > 0 {
                let step = remaining.min(512);
                let mut raw = vec![0.0f32; (step * 2) as usize];
                let mut buf = AudioBuffer::new(&mut raw);
                engine.process(&mut buf);
                remaining -= step;
            }
        } else {
            // OpenStudio es timeline: posicionar el transporte alcanza.
            engine.transport.sample_count = bar6;
        }
        let mut raw = vec![0.0f32; 512 * 2];
        // Ensсарiar el buffer con basura: el engine DEBE rellenar con 0.0.
        for s in raw.iter_mut() {
            *s = 0.777;
        }
        let mut buf = AudioBuffer::new(&mut raw);
        engine.process(&mut buf);
        let peak = raw.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert_eq!(peak, 0.0, "BUG VIVO en {:?}: compás 6 con fuga {}", mode, peak);
        assert_eq!(engine.output_peak(), 0.0, "pico publicado debe ser 0.0 en {:?}", mode);
        assert_eq!(
            engine.output_db(),
            f32::NEG_INFINITY,
            "compás 6 debe marcar -inf dB en {:?}",
            mode
        );
        if mode == EngineMode::OpenLive {
            assert!(
                !engine.clips[0].is_playing,
                "voz OpenLive agotada debe dormir (is_playing=false)"
            );
        }
    }
}
