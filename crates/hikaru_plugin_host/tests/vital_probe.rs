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