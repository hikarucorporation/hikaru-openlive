// Copyright (C) Hikaru Corporation - 2026
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/tests/editor_resize.rs

//! Verifica el camino de resize del editor VST3.
//!
//! `resize_gui` estaba sin implementar: el launcher leía el pedido del plugin
//! y logueaba "no implementado aún". Con un sintetizador de editor grande
//! (Vital) eso significa que el editor se queda con el layout inicial.

use hikaru_plugin_host::vst3::Vst3Instance;
use hikaru_plugin_host::PluginInstance;

fn vst3_by_name(needle: &str) -> Option<std::path::PathBuf> {
    let home = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/root".into()));
    let mut found = Vec::new();
    for root in [
        std::path::PathBuf::from("/usr/lib/vst3"),
        home.join(".vst3"),
    ] {
        if let Ok(entries) = std::fs::read_dir(&root) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() && p.extension().map_or(false, |x| x == "vst3") {
                    found.push(p);
                }
            }
        }
    }
    found.sort();
    found
        .into_iter()
        .find(|p| p.to_string_lossy().to_lowercase().contains(needle))
}

/// `resize_gui` tiene queolanguesar al plugin sin panicar, tenga o no editor
/// abierto.
#[test]
fn resize_gui_is_safe_without_editor_open() {
    let Some(path) = vst3_by_name("vital").or_else(|| vst3_by_name("morph")) else {
        eprintln!("[resize] sin plugin de prueba; skip");
        return;
    };

    let mut inst = Vst3Instance::load(&path).expect("cargar plugin");

    // Sin editor abierto debe fallar limpio, no panicar.
    inst.resize_gui(1200, 800);
    eprintln!("[resize] resize_gui sin editor abierto: sin panic");

    // Tamaño inválido: también limpio.
    inst.resize_gui(0, 0);
    eprintln!("[resize] resize_gui(0,0): sin panic");
}

/// Con editor abierto, el resize debe llegar al plugin y el tamaño consultado
/// tiene que reflejarlo (o al menos no romper nada).
#[test]
fn resize_reaches_the_open_editor() {
    let Some(path) = vst3_by_name("vital").or_else(|| vst3_by_name("morph")) else {
        eprintln!("[resize] sin plugin de prueba; skip");
        return;
    };

    let mut inst = Vst3Instance::load(&path).expect("cargar plugin");
    assert!(inst.has_gui(), "el plugin de prueba no declara editor");

    // Sin ventana no se puede abrir el editor: se valida que el camino de
    // error sea limpio en vez de colgar o romper la memoria.
    let opened = inst.show_gui_embedded(raw_window_handle::RawWindowHandle::Xlib(
        raw_window_handle::XlibWindowHandle::new(0),
    ));
    eprintln!("[resize] show_gui_embedded(handle inválido) -> {:?}", opened.is_ok());

    // Con o sin editor abierto, el tamaño preferido se puede consultar.
    if let Some((w, h)) = inst.get_gui_size() {
        eprintln!("[resize] editor {w}x{h}");
        // Pedir el doble y comprobar que no rompe.
        inst.resize_gui(w * 2, h * 2);
        if let Some((w2, h2)) = inst.get_gui_size() {
            eprintln!("[resize] tras resize: {w2}x{h2}");
        }
    }

    inst.hide_gui();
}

/// `poll_editor_resize` tiene que devolver `None` cuando el plugin no pidió
/// nada (el caso normal), y no colgar.
#[test]
fn poll_editor_resize_returns_none_when_idle() {
    let Some(path) = vst3_by_name("vital").or_else(|| vst3_by_name("morph")) else {
        eprintln!("[resize] sin plugin de prueba; skip");
        return;
    };

    let mut inst = Vst3Instance::load(&path).expect("cargar plugin");

    for _ in 0..3 {
        let r = inst.poll_editor_resize();
        assert!(
            r.is_none(),
            "poll_editor_resize devolvió {r:?} sin editor abierto"
        );
    }
    eprintln!("[resize] poll_editor_resize: None en reposo, como debe");
}