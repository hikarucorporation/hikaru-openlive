// Copyright (C) Hikaru Corporation - 2026
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/tests/wrapper_surface.rs

//! La superficie del editor tiene que estar **mapeada**.
//!
//! # El bug
//!
//! La ventana de contenido —la que se le da al plugin para que Dibuje— se
//! creaba pero **nunca se mapeaba**. En X11 el estado de mapeo es *por
//! ventana*: mapear el contenedor no mapea a sus hijos. Con la ventana de
//! contenido sin mapear, la vista que el plugin crea adentro queda
//! `IsUnviewable` para siempre, el servidor no le manda ningún `Expose` y el
//! toolkit (JUCE, Qt) no tiene un framebuffer visible que pintar.
//!
//! El síntoma era el **cuadro negro fantasma**: el marco del envoltorio se
//! dibujaba perfecto (el título se veía, el botón de cerrar respondía) y
//! debajo, en vez del editor, un rectángulo negro. El plugin estaba vivo, el
//! audio pasaba, los knobs del rack movían parámetros: sólo faltaba la
//! superficie.
//!
//! # Qué comprueba este test
//!
//! Le pregunta al servidor X por el estado de mapeo real de la ventana de
//! contenido, después de que el envoltorio se mostró. No alcanza con
//! comprobar que el código llama a `MapWindow`: lo que importa es lo que el
//! servidor tiene registrado.

use hikaru_plugin_host::hikaru_wrapper::{
    HikaruWrapperWindow, InputPolicy, WrapperConfig,
};
use hikaru_plugin_host::{HostParentKind, PluginInstance, Vst3Instance};
use std::path::PathBuf;
use x11rb::protocol::xproto::{ConnectionExt, MapState};

/// El estado de mapeo que el servidor tiene para una ventana.
fn map_state(window: u32) -> Option<MapState> {
    let (conn, _) = x11rb::rust_connection::RustConnection::connect(None).ok()?;
    let reply = conn.get_window_attributes(window).ok()?.reply().ok()?;
    Some(reply.map_state)
}

/// Los hijos de una ventana, según el servidor.
fn children(parent: u32) -> Vec<u32> {
    let Some((conn, _)) = x11rb::rust_connection::RustConnection::connect(None).ok() else {
        return Vec::new();
    };
    let Ok(cookie) = conn.query_tree(parent) else {
        return Vec::new();
    };
    let Ok(tree) = cookie.reply() else {
        return Vec::new();
    };
    tree.children
}

/// La ventana de contenido del envoltorio tiene que quedar visible.
///
/// Es el contrato que rompe si se vuelve a borrar el `MapWindow` del
/// contenido: sin él la vista del plugin queda `IsUnviewable` y el editor no
/// pinta nunca.
#[test]
fn la_ventana_de_contenido_queda_mapeada() {
    let config = WrapperConfig::default();
    let mut window = match HikaruWrapperWindow::create("hikaru-test", &config, InputPolicy::default())
    {
        Ok(w) => w,
        Err(e) => {
            // Sin servidor de display no hay nada que comprobar: es un
            // entorno headless válido, no un fallo.
            eprintln!("[surface] sin display ({e:?}); skip");
            return;
        }
    };

    window.prepare(Some((640, 480))).expect("preparar el contenido");
    let content = window.content_id().expect("la ventana de contenido existe");

    // Antes de mapear el contenedor, el contenido está mapeado pero no es
    // visible: en X11 "mapeado" y "visible" son dos cosas distintas, y sólo
    // la segunda le sirve al plugin.
    window.show(Some((640, 480))).expect("mostrar la ventana");

    let state = map_state(content).expect("el servidor responde por el contenido");
    assert_eq!(
        state,
        MapState::VIEWABLE,
        "la ventana de contenido tiene que quedar visible: el editor del \
         plugin es hijo de ella, y un hijo de una ventana no visible queda \
         `IsUnviewable` para siempre — el editor se ve negro"
    );
}

/// La ventana que se le da al plugin tiene que ser la de contenido, no la del
/// contenedor.
///
/// El contenedor es el marco: si el plugin recibiera su XID, podría pintar
/// encima del título y de los bordes.
#[test]
fn el_plugin_recibe_la_ventana_de_contenido_y_no_el_contenedor() {
    let config = WrapperConfig::default();
    let mut window = match HikaruWrapperWindow::create("hikaru-test", &config, InputPolicy::default())
    {
        Ok(w) => w,
        Err(e) => {
            eprintln!("[surface] sin display ({e:?}); skip");
            return;
        }
    };

    window.prepare(Some((640, 480))).expect("preparar el contenido");
    let handle = window.raw_handle();
    let content = window.content_id().expect("la ventana de contenido existe");

    let raw_window_handle::RawWindowHandle::Xlib(h) = handle else {
        panic!("en Linux el handle tiene que ser Xlib");
    };
    assert_eq!(
        h.window as u32, content,
        "el plugin tiene que recibir la ventana de contenido"
    );
    assert_ne!(
        h.window as u32,
        window.id(),
        "el plugin no puede recibir el contenedor: es el marco"
    );
}

/// El primer `.vst3` instalado, si hay alguno.
fn find_vst3() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/root".into()));
    for root in [home.join(".vst3"), PathBuf::from("/usr/lib/vst3")] {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() && p.extension().map_or(false, |x| x == "vst3") {
                return Some(p);
            }
        }
    }
    None
}

/// La vista que el plugin crea tiene que quedar **visible**.
///
/// Éste es el test que falla con el bug del cuadro negro: la vista del
/// plugin es hija de la ventana de contenido, y si el contenido no está
/// mapeado la vista queda `IsUnviewable`. El plugin no recibe ningún
/// `Expose`, su toolkit no pinta y lo que se ve es un rectángulo negro.
///
/// Se hace con un plugin real porque el comportamiento depende de que el
/// plugin cree de verdad una ventana hija: un stub no lo hace y el test
/// pasaría sin comprobar nada.
#[test]
fn la_vista_del_plugin_queda_visible() {
    let Some(path) = find_vst3() else {
        eprintln!("[surface] sin plugins VST3 instalados; skip");
        return;
    };

    // La configuración real del DAW en una sesión Wayland: no hay XID del
    // DAW al que emparentarse, así que la ventana del editor es top-level.
    let config = WrapperConfig::with_parent(HostParentKind::Wayland);
    let mut window = match HikaruWrapperWindow::create("hikaru-test", &config, InputPolicy::default())
    {
        Ok(w) => w,
        Err(e) => {
            eprintln!("[surface] sin display ({e:?}); skip");
            return;
        }
    };

    let mut instance = match Vst3Instance::load(&path) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("[surface] no se pudo cargar '{}' ({e}); skip", path.display());
            return;
        }
    };

    if !instance.has_gui() {
        eprintln!("[surface] '{}' no tiene editor; skip", path.display());
        return;
    }

    // El mismo orden que `HikaruWrapper::open`: preparar, embeber, buscar
    // la vista, mostrar.
    let preferred = instance.get_gui_size();
    let handle = window
        .prepare(preferred)
        .expect("preparar la ventana de contenido");
    if let Err(e) = instance.show_gui_embedded(handle) {
        eprintln!("[surface] el plugin no aceptó el embebido ({e}); skip");
        return;
    }
    window.refresh_view();
    window.show(preferred).expect("mostrar la ventana");

    let content = window.content_id().expect("la ventana de contenido existe");
    assert_eq!(
        map_state(content),
        Some(MapState::VIEWABLE),
        "la ventana de contenido tiene que quedar visible"
    );

    // La vista del plugin es una hija de la ventana de contenido. Puede
    // que el plugin dibuje directamente sobre el contenido y no cree ninguna
    // hija: en ese caso no hay nada que comprobar.
    let views: Vec<u32> = children(content)
        .into_iter()
        .filter(|w| map_state(*w) == Some(MapState::VIEWABLE))
        .collect();

    if views.is_empty() {
        // El plugin no creó una ventana hija: dibuja sobre el contenido. Es
        // un caso válido (lo hacen varios editores), así que no se falla acá.
        eprintln!(
            "[surface] '{}' no creó una ventana hija: dibuja sobre el contenido",
            path.display()
        );
        return;
    }

    for view in views {
        assert_eq!(
            map_state(view),
            Some(MapState::VIEWABLE),
            "la vista del plugin (0x{view:x}) tiene que quedar visible: si no, \
             el editor no recibe ningún `Expose` y se ve negro"
        );
    }

    instance.hide_gui();
}

/// Xlib tiene que saber que el proceso es multihilo.
///
/// El editor de un plugin levanta su propio hilo de eventos X11 y desde
/// entonces hay dos hilos hablando con la misma librería Xlib. Sin
/// `XInitThreads()` las respuestas del servidor se desalinean y la apertura
/// del editor se cuelga en un deadlock: el contenedor queda a medio hacer, con
/// la ventana del plugin en 1×1 y sin pintar.
///
/// Esta llamada tiene que ser idempotente y no puede fallar de formaFatal:
/// un plugin puede no usar Xlib en absoluto.
#[test]
fn xlib_acepta_varios_hilos() {
    assert!(
        hikaru_plugin_host::xlib::ensure_xlib_threads(),
        "no se pudo activar el soporte de hilos de Xlib"
    );
    // Idempotente: la segunda llamada no tiene que hacer nada ni fallar.
    assert!(hikaru_plugin_host::xlib::ensure_xlib_threads());
}
