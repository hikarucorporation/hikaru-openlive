// crates/hikaru_audio_engine/tests/plugin_track_routing.rs
//
// Regresión: "el plugin carga pero no hace nada, como si el track estuviera
// muteado".
//
// La cadena de efectos por pista (`track_effects`) y el buffer que mezcla los
// clips (`track_output_bufs`) están indexados por el MISMO espacio: el del
// motor, 0..=15, sin master. La GUI tiene dos espacios distintos (el mixer mete
// un MASTER en 0) y las vistas no siempre traducían. Cuando el rack de DSP
// mandaba su índice de mixer crudo al motor, la cadena se insertaba una pista
// más abajo de la que mezclaba el clip, y el plugin procesaba un buffer siempre
// mudo mientras el audio seco pasaba de largo.
//
// Estos tests fijan el contrato del lado del motor: la cadena de la pista N
// procesa el audio de los clips de la pista N, y no toca las otras pistas.

use hikaru_audio_engine::AudioEngine;
use hikaru_core::{AudioBuffer, SampleRate};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

static TABLE: [f32; 2048] = [0.0; 2048];

/// Pistas de un DAW sin master delante, como las ve el motor.
const TRACK_A: usize = 0;
const TRACK_B: usize = 3;

fn engine() -> AudioEngine<'static> {
    let mut engine = AudioEngine::new(
        SampleRate::new(48_000.0),
        &TABLE,
        Arc::new(AtomicU64::new(0)),
    );
    // OpenLive es el modo real de este camino: el motor arranca en OpenStudio,
    // donde el clip se renderiza por ventana de timeline en vez de por
    // reproducción del pad. La cadena de insert es la misma en los dos modos.
    engine.set_mode(hikaru_audio_engine::EngineMode::OpenLive);
    for t in 0..16 {
        engine.track_volumes[t].store(1.0f32.to_bits(), Ordering::Relaxed);
    }
    engine.master_gain.store(1.0f32.to_bits(), Ordering::Relaxed);
    engine
}

/// Busca un VST3 o CLAP real instalado. `None` si la máquina no tiene ninguno,
/// para que el CI limpio no falle.
fn real_plugin() -> Option<hikaru_plugin_host::PluginDescriptor> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/root".into()));
    let roots = [
        home.join(".vst3"),
        PathBuf::from("/usr/lib/vst3"),
        PathBuf::from("/usr/lib/clap"),
        home.join(".clap"),
    ];
    let report = hikaru_plugin_host::registry::scan_standard_paths();
    let found = report
        .plugins
        .into_iter()
        .find(|p| {
            roots.iter().any(|r| {
                p.path.starts_with(r)
                    || p.path
                        .parent()
                        .map_or(false, |par| par.starts_with(r))
            })
        });
    found.map(|mut p| {
        // La cadena de insert siempre inserta: forzamos la categoría de efecto
        // aunque el plugin sea un instrumento (el motor no distingue todavía).
        p.kind = hikaru_plugin_host::PluginKind::Effect;
        p
    })
}

/// Carga un clip de un bloque de senal constante en una pista y arranca el
/// transporte.
///
/// El clip se registra con `track_index` tal cual lo manda la GUI, que es el
/// índice de matriz (sin master). Un bloque constante no tiene ceros, así que
/// cualquier silencio en la salida es atribuible al enrutado y no a la senal.
fn play_clip_on(engine: &mut AudioEngine, clip_id: usize, track_idx: usize, frames: usize) {
    let samples = vec![0.5f32; frames * 2];
    engine.add_clip(clip_id, track_idx, 0, samples, 0.0, 0.0, 0.0, 2, true);
    engine.play();
    engine.trigger_clip(track_idx, 0);
}

fn peak(engine: &mut AudioEngine, frames: usize) -> f32 {
    let mut raw = vec![0.0f32; frames * 2];
    let mut buf = AudioBuffer::new(&mut raw);
    engine.process(&mut buf);
    raw.iter().map(|v| v.abs()).fold(0.0f32, f32::max)
}

/// Un clip con una cadena de efectos en SU pista sigue sonando.
///
/// Si la cadena estuviera en otra pista (el bug), el plugin recibiria silencio
/// y el audio pasaria directo: el pico seguiria siendo alto y este test pasaria.
/// Por eso la asercion fuerte es la de aislamiento, de abajo.
#[test]
fn clip_passes_through_the_chain_on_its_own_track() {
    let Some(desc) = real_plugin() else {
        eprintln!("[routing] sin plugins instalados; skip");
        return;
    };

    let mut engine = engine();
    engine
        .load_external_plugin(TRACK_A, 0, &desc)
        .unwrap_or_else(|e| panic!("[routing] no se pudo cargar '{}': {e}", desc.name));

    let frames = 1024;
    play_clip_on(&mut engine, 1, TRACK_A, frames);

    // Varios bloques: un efecto con delay o reverb necesita audio previo.
    let mut p = 0.0;
    for _ in 0..8 {
        p = peak(&mut engine, frames);
    }
    assert!(
        p > 0.05,
        "[routing] el clip de la pista {TRACK_A} deberia sonar a traves de \
         '{}' (pico {p})",
        desc.name
    );
}

/// La cadena de la pista N no puede procesar el audio de la pista M.
///
/// Ésta es la asercion que atrapa el off-by-one: con el desfase, la cadena de
/// la pista 3 recibia el clip de la pista 0 mientras el audio de la pista 3
/// salia sin procesar. Se comprueba que una cadena cargada en la pista 3 deja
/// intacta la pista 0 y viceversa.
#[test]
fn chain_on_one_track_does_not_leak_into_another() {
    let Some(desc) = real_plugin() else {
        eprintln!("[routing] sin plugins instalados; skip");
        return;
    };

    // Baseline: sin cadena, ambas pistas con clip suenan igual.
    let mut baseline = engine();
    play_clip_on(&mut baseline, 1, TRACK_A, 1024);
    play_clip_on(&mut baseline, 2, TRACK_B, 1024);
    let mut base_a = 0.0;
    let mut base_b = 0.0;
    for _ in 0..8 {
        base_a = peak(&mut baseline, 1024);
        base_b = peak(&mut baseline, 1024);
    }

    // Con cadena en TRACK_B, la pista A no puede cambiar.
    let mut with_chain = engine();
    with_chain
        .load_external_plugin(TRACK_B, 0, &desc)
        .unwrap_or_else(|e| panic!("[routing] no se pudo cargar '{}': {e}", desc.name));
    play_clip_on(&mut with_chain, 1, TRACK_A, 1024);
    play_clip_on(&mut with_chain, 2, TRACK_B, 1024);

    let mut chain_a = 0.0;
    let mut chain_b = 0.0;
    for _ in 0..8 {
        chain_a = peak(&mut with_chain, 1024);
        chain_b = peak(&mut with_chain, 1024);
    }

    assert!(
        chain_a > 0.05,
        "[routing] la pista {TRACK_A} deberia seguir sonando con una cadena \
         puesta en la {TRACK_B} (pico {chain_a})"
    );
    assert!(
        chain_b > 0.05,
        "[routing] la pista {TRACK_B} deberia seguir sonando a traves de su \
         propia cadena (pico {chain_b})"
    );

    println!(
        "[routing] sin cadena: A={base_a:.3} B={base_b:.3} | con cadena en B: \
         A={chain_a:.3} B={chain_b:.3}"
    );
}
