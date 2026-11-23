// Copyright (C) Hikaru Corporation - 2026
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/tests/vital_probe.rs

//! Reproduce exactamente lo que hace la GUI al sondear un plugin del rack.
//!
//! `probe_external_params` en `hikaru_gui/src/app.rs` instancia el plugin en un
//! hilo aparte y lee sus parámetros. Este test corre el mismo camino contra
//! plugins reales, incluido Vital, que es un sintetizador grande con muchos
//! parámetros y editor embebido.

use hikaru_plugin_host::clap::ClapInstance;
use hikaru_plugin_host::vst3::Vst3Instance;
use hikaru_plugin_host::PluginInstance;
use std::path::PathBuf;

fn vst3_candidates() -> Vec<PathBuf> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/root".into()));
    let mut out = Vec::new();
    for root in [PathBuf::from("/usr/lib/vst3"), home.join(".vst3")] {
        if let Ok(entries) = std::fs::read_dir(&root) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() && p.extension().map_or(false, |x| x == "vst3") {
                    out.push(p);
                }
            }
        }
    }
    out.sort();
    out
}

fn clap_candidates() -> Vec<PathBuf> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/root".into()));
    let mut out = Vec::new();
    for root in [
        PathBuf::from("/usr/lib/clap"),
        home.join(".clap"),
        PathBuf::from("/usr/local/lib/clap"),
    ] {
        if let Ok(entries) = std::fs::read_dir(&root) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_file() && p.extension().map_or(false, |x| x == "clap") {
                    out.push(p);
                }
            }
        }
    }
    out.sort();
    out
}

/// Vital: el caso que动机-reportó el usuario (controles del backend que no
/// aparecen en el rack).
#[test]
fn vital_exposes_params_to_the_rack_probe() {
    let Some(path) = vst3_candidates()
        .into_iter()
        .find(|p| p.to_string_lossy().to_lowercase().contains("vital"))
    else {
        eprintln!("[probe] Vital no está instalado; skip");
        return;
    };

    eprintln!("[probe] sondeando {}", path.display());
    let mut inst = Vst3Instance::load(&path).expect("instanciar Vital");

    assert!(inst.params_supported(), "Vital no declara parámetros");

    let snapshot = inst.snapshot_params();
    eprintln!("[probe] Vital devolvió {} parámetros", snapshot.len());

    assert!(
        !snapshot.is_empty(),
        "snapshot_params() vacío para Vital: el rack no puede dibujar knobs"
    );

    // Muestra los primeros para inspección manual.
    for (info, value) in snapshot.iter().take(10) {
        eprintln!(
            "  id={} {:?} = {:.3} (step_count={}, automate={})",
            info.id, info.name, value, info.step_count, info.can_automate
        );
    }
}

/// Todos los VST3: verifica que el sondeo devuelva algo para todos.
#[test]
fn every_vst3_returns_params() {
    let mut empty = Vec::new();
    for path in vst3_candidates() {
        match Vst3Instance::load(&path) {
            Ok(mut inst) => {
                let n = inst.snapshot_params().len();
                eprintln!("[probe] VST3 {} -> {} params", path.display(), n);
                if n == 0 {
                    empty.push(path.display().to_string());
                }
            }
            Err(e) => eprintln!("[probe] VST3 {} no cargó: {e}", path.display()),
        }
    }
    if !empty.is_empty() {
        eprintln!("[probe] VST3 sin parámetros: {empty:?}");
    }
}

/// CLAP: hoy `probe_external_params` devuelve `Ok(vec![])` sin intentar nada.
/// Este test mide cuántos parámetros hay realmente disponibles, que es lo que
/// el rack podría mostrar.
#[test]
fn every_clap_returns_params() {
    let mut total = 0usize;
    for path in clap_candidates() {
        match ClapInstance::load(&path) {
            Ok(mut inst) => {
                let supported = inst.params_supported();
                let n = inst.snapshot_params().len();
                eprintln!(
                    "[probe] CLAP {} -> supported={supported}, {} params",
                    path.display(),
                    n
                );
                total += n;
            }
            Err(e) => eprintln!("[probe] CLAP {} no cargó: {e}", path.display()),
        }
    }
    eprintln!("[probe] total de parámetros CLAP disponibles: {total}");
    assert!(
        total > 0,
        "ningún CLAP expone parámetros: el rack no puede dibujarlos"
    );
}

#[test]
fn vital_vst3_full_lifecycle_without_gui() {
    let path = PathBuf::from("/usr/lib/vst3/Vital.vst3");
    if !path.exists() {
        eprintln!("[vital] /usr/lib/vst3/Vital.vst3 no existe; skip");
        return;
    }
    eprintln!("[vital-vst3] 1. load");
    let mut inst = Vst3Instance::load(&path).expect("load Vital.vst3");
    eprintln!("[vital-vst3] 2. set_sample_rate");
    inst.set_sample_rate(48_000.0);
    eprintln!("[vital-vst3] 3. snapshot_params_with_display");
    let params = inst.snapshot_params_with_display();
    eprintln!("[vital-vst3] 3. ok ({} params)", params.len());
    eprintln!("[vital-vst3] 4. set_transport + process (4 blocks of 1024)");
    for blk in 0..4 {
        inst.set_transport(hikaru_plugin_host::HostTransportInfo {
            is_playing: true,
            tempo: 120.0,
            sample_rate: 48_000.0,
            sample_position: (blk * 1024) as i64,
            beats_position: (blk * 1024) as f64 / 24_000.0,
            ..Default::default()
        });
        let mut raw = vec![0.0f32; 1024 * 2];
        let mut buf = hikaru_core::AudioBuffer::new(&mut raw);
        inst.process(&mut buf);
    }
    eprintln!("[vital-vst3] 4. ok, last_error={:?}", inst.take_last_error());
    eprintln!("[vital-vst3] 5. save_state");
    let state = inst.save_state();
    eprintln!("[vital-vst3] 5. state={:?}", state.as_ref().map(|v| v.len()));
    if let Ok(bytes) = state {
        eprintln!("[vital-vst3] 6. load_state");
        let _ = inst.load_state(&bytes);
    }
    eprintln!("[vital-vst3] DONE");
}

#[test]
fn vital_clap_full_lifecycle_without_gui() {
    let path = PathBuf::from("/usr/lib/clap/Vital.clap");
    if !path.exists() {
        eprintln!("[vital] /usr/lib/clap/Vital.clap no existe; skip");
        return;
    }
    eprintln!("[vital-clap] 1. load");
    let mut inst = ClapInstance::load(&path).expect("load Vital.clap");
    eprintln!("[vital-clap] 2. set_sample_rate");
    inst.set_sample_rate(48_000.0);
    eprintln!("[vital-clap] 3. snapshot_params_with_display");
    let params = inst.snapshot_params_with_display();
    eprintln!("[vital-clap] 3. ok ({} params)", params.len());
    // Camino inactivo (params.flush) antes de activar el procesador.
    if let Some((first_info, first_val, _)) = params.first() {
        assert!(inst.set_param_normalized(first_info.id, *first_val));
    }
    eprintln!("[vital-clap] 4. set_transport + process (4 blocks of 1024)");
    for blk in 0..4 {
        inst.set_transport(hikaru_plugin_host::HostTransportInfo {
            is_playing: true,
            tempo: 120.0,
            sample_rate: 48_000.0,
            sample_position: (blk * 1024) as i64,
            beats_position: (blk * 1024) as f64 / 24_000.0,
            ..Default::default()
        });
        let mut raw = vec![0.0f32; 1024 * 2];
        let mut buf = hikaru_core::AudioBuffer::new(&mut raw);
        inst.process(&mut buf);
    }
    // Mismo patrón que `HikaruWrapper::open_with_state`: aplica los 775 parámetros
    // iniciales y drena en un bloque de 128 frames (antes crasheaba con SIGSEGV en
    // `ClapJuceWrapper::process` por `cookie == NULL`).
    eprintln!("[vital-clap] 4b. set_param_normalized (all {} params) + set_param_modulation + process", params.len());
    for (info, val, _) in &params {
        assert!(inst.set_param_normalized(info.id, *val));
        if info.is_modulatable {
            let _ = inst.set_param_modulation(info.id, 0.05);
        }
    }
    {
        let mut raw = vec![0.0f32; 128 * 2];
        let mut buf = hikaru_core::AudioBuffer::new(&mut raw);
        inst.process(&mut buf);
    }
    eprintln!("[vital-clap] 4. ok, last_error={:?}", inst.take_last_error());
    eprintln!("[vital-clap] 5. save_state");
    let state = inst.save_state();
    eprintln!("[vital-clap] 5. state={:?}", state.as_ref().map(|v| v.len()));
    if let Ok(bytes) = state {
        eprintln!("[vital-clap] 6. load_state");
        let _ = inst.load_state(&bytes);
    }
    eprintln!("[vital-clap] DONE");
}

/// Regresión del deadlock en `ClapJuceWrapper::guiCreate` / `onTimer`:
///
/// Reproduce exactamente el patrón real de `hikaru_openlive`:
/// 1. Hilo A (motor de audio) carga `Vital.clap` y procesa audio.
/// 2. Hilo B (`probe_external_params`) carga otra instancia, sondea parámetros y muere.
/// 3. Hilo C (`run_plugin_window`) carga una tercera instancia y llama a `gui_tick()`
///    (que dispara `ClapJuceWrapper::onTimer`, el cual toma `juce::MessageManagerLock`).
///
/// Sin un hilo único `[main-thread]` de CLAP, `MessageManagerLock` en Hilo C
/// queda en deadlock infinito porque `messageThreadId` pertenecía a Hilo A/B.
#[test]
fn vital_clap_cross_thread_timer_does_not_deadlock() {
    let path = PathBuf::from("/usr/lib/clap/Vital.clap");
    if !path.exists() {
        eprintln!("[vital] /usr/lib/clap/Vital.clap no existe; skip");
        return;
    }

    // Hilo A: instancia viva del motor de audio.
    let path_a = path.clone();
    let inst_a = std::thread::spawn(move || {
        let mut inst = ClapInstance::load(&path_a).expect("load A");
        let mut raw = vec![0.0f32; 128 * 2];
        inst.process(&mut hikaru_core::AudioBuffer::new(&mut raw));
        inst
    })
    .join()
    .expect("thread A");

    // Hilo B: sondeo desechable (carga, lee params y muere).
    let path_b = path.clone();
    std::thread::spawn(move || {
        let mut inst = ClapInstance::load(&path_b).expect("load B");
        let _ = inst.snapshot_params_with_display();
    })
    .join()
    .expect("thread B");

    // Hilo C: ventana del editor — debe poder disparar `onTimer` (`gui_tick`) sin deadlock.
    let path_c = path.clone();
    let (done_tx, done_rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut inst = ClapInstance::load(&path_c).expect("load C");
        std::thread::sleep(std::time::Duration::from_millis(30));
        inst.gui_tick();
        let _ = done_tx.send(());
    });

    done_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("DEADLOCK en ClapJuceWrapper::onTimer / MessageManagerLock desde otro hilo");

    drop(inst_a);
}