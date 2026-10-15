// Copyright (C) Hikaru Corporation - 2026
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/tests/real_plugin_smoke.rs

//! Test de humo contra plugins REALES instalados en el sistema.
//!
//! A diferencia de `src/lib.rs` (que usa `StubInstance`), este test carga
//! binarios de verdad desde `~/.vst3`, `/usr/lib/vst3`, `/usr/lib/clap`, llama
//! a `process()` con un bloque de audio y comprueba que el plugin no crashea
//! el host y que devuelve algo audible o al menos no envenena el buffer.
//!
//! Los tests se saltan (no fallan) si no hay plugins instalados, para que el
//! CI limpio no se rompa.

use hikaru_core::AudioBuffer;
use hikaru_plugin_host::clap::ClapInstance;
use hikaru_plugin_host::vst3::Vst3Instance;
use hikaru_plugin_host::PluginInstance;
use std::path::PathBuf;

const BLOCK_FRAMES: usize = 512;
const NUM_CHANNELS: usize = 2;

/// Rutas de plugins reales ya instaladas. El primero que exista gana.
fn candidate_roots() -> Vec<PathBuf> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/root".into()));
    vec![
        home.join(".vst3"),
        PathBuf::from("/usr/lib/vst3"),
        PathBuf::from("/usr/lib/clap"),
        home.join(".clap"),
    ]
}

/// Devuelve el primer bundle `.vst3` (directorio) o binario suelto encontrado.
fn find_vst3() -> Option<PathBuf> {
    for root in candidate_roots() {
        if let Ok(entries) = std::fs::read_dir(&root) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() && p.extension().map_or(false, |x| x == "vst3") {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Devuelve el primer `.clap` (archivo suelto) encontrado.
fn find_clap() -> Option<PathBuf> {
    for root in candidate_roots() {
        if let Ok(entries) = std::fs::read_dir(&root) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_file() && p.extension().map_or(false, |x| x == "clap") {
                    return Some(p);
                }
            }
        }
    }
    None
}

fn make_test_buffer(amp: f32) -> Vec<f32> {
    (0..(BLOCK_FRAMES * NUM_CHANNELS))
        .map(|i| amp * ((i as f32) * 0.01).sin())
        .collect()
}

/// VST3 real: cargar, listar params, procesar un bloque.
#[test]
fn vst3_real_plugin_loads_and_processes() {
    let Some(path) = find_vst3() else {
        eprintln!("[smoke] sin VST3 instalados; skip");
        return;
    };
    eprintln!("[smoke] probando VST3: {}", path.display());

    let mut inst = match Vst3Instance::load(&path) {
        Ok(i) => i,
        Err(e) => panic!("[smoke] VST3 load fallo en {}: {e}", path.display()),
    };

    let name = inst.get_name().to_string();
    assert!(!name.is_empty(), "[smoke] VST3 sin nombre");
    eprintln!("[smoke] VST3 cargó: name={name} has_gui={}", inst.has_gui());

    // Snapshot de parámetros (no debe entrar en panic aunque el plugin no
    // declare ninguno).
    let params = inst.snapshot_params();
    eprintln!("[smoke] VST3 {name} expone {} parámetros", params.len());

    // Procesar un bloque real. Si el plugin toma ownership raro, que rompa
    // acá es exactamente el bug que queremos detectar.
    let mut samples = make_test_buffer(0.5);
    {
        let mut buf = AudioBuffer::new(&mut samples);
        inst.process(&mut buf);
    }

    let finite = samples.iter().all(|s| s.is_finite());
    assert!(finite, "[smoke] VST3 {name} produjo NaN/Inf");

    // Si el launcher registró un error de start/process, fallamos.
    if let Some(err) = inst.take_last_error() {
        panic!("[smoke] VST3 {name} reportó error: {err}");
    }

    eprintln!("[smoke] VST3 {name} procesó OK ({} muestras finitas)", samples.len());
}

/// CLAP real: cargar, listar params, procesar un bloque.
#[test]
fn clap_real_plugin_loads_and_processes() {
    let Some(path) = find_clap() else {
        eprintln!("[smoke] sin CLAP instalados; skip");
        return;
    };
    eprintln!("[smoke] probando CLAP: {}", path.display());

    let mut inst = match ClapInstance::load(&path) {
        Ok(i) => i,
        Err(e) => panic!("[smoke] CLAP load fallo en {}: {e}", path.display()),
    };

    let name = inst.get_name().to_string();
    assert!(!name.is_empty(), "[smoke] CLAP sin nombre");
    eprintln!("[smoke] CLAP cargó: name={name} has_gui={}", inst.has_gui());

    let params = inst.snapshot_params();
    eprintln!("[smoke] CLAP {name} expone {} parámetros", params.len());

    let mut samples = make_test_buffer(0.5);
    {
        let mut buf = AudioBuffer::new(&mut samples);
        inst.process(&mut buf);
    }

    let finite = samples.iter().all(|s| s.is_finite());
    assert!(finite, "[smoke] CLAP {name} produjo NaN/Inf");

    if let Some(err) = inst.take_last_error() {
        panic!("[smoke] CLAP {name} reportó error: {err}");
    }

    eprintln!("[smoke] CLAP {name} procesó OK ({} muestras finitas)", samples.len());
}

/// Verifica que el escaneo del registro encuentra plugins reales en disco.
#[test]
fn registry_scan_finds_real_plugins() {
    let report = hikaru_plugin_host::registry::scan_standard_paths();
    eprintln!(
        "[smoke] registry encontró {} plugins y {} errores",
        report.plugins.len(),
        report.errors.len()
    );
    // En esta máquina debe encontrar al menos uno; si no, el entorno cambió.
    assert!(
        !report.plugins.is_empty() || find_vst3().is_none(),
        "[smoke] registry no vio plugins pero hay bundles en disco"
    );
}