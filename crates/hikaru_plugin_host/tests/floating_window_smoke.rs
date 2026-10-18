// Copyright (C) Hikaru Corporation - 2026
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/tests/floating_window_smoke.rs

//! Prueba que la ventana flotante de un plugin **no** bloquea el hilo.
//!
//! El bug original: `show_gui_floating` llamaba `event_loop.run()` en línea,
//! así que abrir el editor de un plugin congelaba la DAW hasta que el usuario
//! cerrara la ventana. Este test mide que `show_gui_floating` retorna rápido.

use hikaru_plugin_host::clap::ClapInstance;
use hikaru_plugin_host::PluginInstance;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn find_clap() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/root".into()));
    for root in [
        PathBuf::from("/usr/lib/clap"),
        home.join(".clap"),
        PathBuf::from("/usr/local/lib/clap"),
    ] {
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

/// `show_gui_floating` tiene que retornar en milisegundos, no cuando el
/// usuario cierre la ventana.
///
/// En un entorno headless puede no haber display, así que el plugin puede
/// legitimately fallar al abrir el editor; lo que se mide es que **retorne**,
/// no que tenga éxito.
#[test]
fn show_gui_floating_returns_without_blocking() {
    let Some(path) = find_clap() else {
        eprintln!("[floating] sin CLAP instalados; skip");
        return;
    };

    let mut inst = ClapInstance::load(&path).expect("cargar CLAP");

    let start = Instant::now();
    let result = inst.show_gui_floating();
    let elapsed = start.elapsed();

    eprintln!(
        "[floating] show_gui_floating retornó en {:?} (resultado: {})",
        elapsed,
        match &result {
            Ok(_) => "ok".to_string(),
            Err(e) => format!("err: {e}"),
        }
    );

    // El umbral es holgado a propósito: sólo queremos detectar un bloqueo del
    // event loop, que sería de segundos o infinito. Un fallo rápido de display
    // headless cae acá sin problema.
    assert!(
        elapsed < Duration::from_millis(2000),
        "show_gui_floating tardó {elapsed:?}: sigue bloqueando el hilo"
    );

    // Limpieza: si llegó a abrir, cerramos para no dejar ventanas ni hilos.
    inst.hide_gui();
}

/// Abrir y cerrar varias ventanas seguidas no debe acumular handles ni
/// colgarse: es el caso "abrir el mismo plugin dos veces".
#[test]
fn floating_editor_handle_is_reusable_across_opens() {
    use hikaru_plugin_host::floating_window::FloatingEditor;

    // Levantar el event loop global una vez.
    if let Err(e) = FloatingEditor::open("hikaru-test", 320, 240) {
        // Sin display no hay event loop: es un entorno headless válido.
        eprintln!("[floating] sin display para event loop ({e}); skip");
        return;
    }

    // Segundo uso del hub: tiene que devolver el mismo loop, no crear otro.
    match FloatingEditor::open("hikaru-test-2", 320, 240) {
        Ok(editor) => {
            eprintln!("[floating] segunda ventana encolada");
            drop(editor);
        }
        Err(e) => panic!("[floating] la segunda apertura falló: {e}"),
    }
}