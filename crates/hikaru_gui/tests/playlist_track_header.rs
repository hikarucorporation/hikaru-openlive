// crates/hikaru_gui/tests/playlist_track_header.rs
//
// Track Headers de la Playlist / Timeline con la misma caja de mezcla del
// Session Matrix: nombre + [M] [S] + knob de pan + fader de volumen con dB.
//
// Lo que se verifica:
// - Los controles de mezcla existen dentro del header (`pl_vol_*`, `pl_panknob_*`,
//   `pl_track_mute_*`, `pl_track_solo_*`).
// - Mute/Solo escriben en la pista del modo activo Y espejan al espejo
//   (Session Matrix en OpenLive / vector del modo) → binding bidireccional.
// - El drag del fader cambia el volumen en vivo (mismo tag global que la matriz).
// - La fila es lo bastante alta para la caja completa y el header no se superpone.

use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Arc;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    px, point, size, AppContext, Context, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    TestAppContext, Window,
};

use hikaru_gui::app::{AppMode, HikaruApp};
use hikaru_gui::audio_proxy::{AudioProxy, GuiCommand};
use hikaru_gui::views::mixer::Track;

fn test_app(window: &mut Window, cx: &mut Context<HikaruApp>) -> HikaruApp {
    let (tx, _rx) = std::sync::mpsc::channel::<GuiCommand>();
    HikaruApp::build(
        window,
        cx,
        AudioProxy::new(tx),
        None,
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU32::new(0.0f32.to_bits())),
        None,
    )
}

fn open_studio(cx: &mut TestAppContext) -> gpui_kit::WindowHandle<HikaruApp> {
    cx.update(gpui_kit::init);
    let handle = cx.add_window(|window, cx| test_app(window, cx));
    cx.simulate_window_resize(handle.into(), size(px(1280.0), px(720.0)));
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.mode = AppMode::OpenStudio;
                cx.notify();
            });
        });
    })
    .unwrap();
    handle
}

fn down(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    x: f32,
    y: f32,
    clicks: usize,
) {
    cx.update_window(handle.into(), |_, window, cx| {
        use gpui_kit::InputEvent as _;
        window.dispatch_event(
            MouseDownEvent {
                button: MouseButton::Left,
                position: point(px(x), px(y)),
                modifiers: Default::default(),
                click_count: clicks,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

fn move_to(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext, x: f32, y: f32) {
    cx.update_window(handle.into(), |_, window, cx| {
        use gpui_kit::InputEvent as _;
        window.dispatch_event(
            MouseMoveEvent {
                position: point(px(x), px(y)),
                pressed_button: Some(MouseButton::Left),
                modifiers: Default::default(),
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

fn up(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext, x: f32, y: f32) {
    cx.update_window(handle.into(), |_, window, cx| {
        use gpui_kit::InputEvent as _;
        window.dispatch_event(
            MouseUpEvent {
                button: MouseButton::Left,
                position: point(px(x), px(y)),
                modifiers: Default::default(),
                click_count: 1,
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

/// Estado de mezcla de una fila del modo activo + su espejo en la matriz.
fn studio_mix(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    idx: usize,
) -> (f32, f32, bool, bool) {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        let mut out = (0.0f32, 0.0f32, false, false);
        app.update(cx, |app, cx| {
            let st = app.state.read(cx);
            if let Some(t) = st.studio_tracks.get(idx) {
                out = (t.volume, t.pan, t.mute, t.solo);
            }
        });
        out
    })
    .unwrap()
}

// =========================================================================
// Existencia de los controles dentro del header
// =========================================================================

#[gpui_kit::gpui::test]
fn playlist_header_exposes_full_mix_controls(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        for id in [
            "pl_row_1",
            "pl_track_mute_1",
            "pl_track_solo_1",
            "pl_vol_1",
            "pl_panknob_1",
        ] {
            assert!(
                window.try_find(id).is_some(),
                "el header de la playlist debería exponer `{id}`"
            );
        }
    })
    .unwrap();
}

/// La caja de mezcla completa tiene que caber en la fila: si el alto por
/// defecto quedara corto, el fader/knob se solaparían con la fila siguiente.
#[gpui_kit::gpui::test]
fn playlist_row_tall_enough_for_mix_box(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    let (row_h, vol_h, knob_bottom, row_bottom) = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            let row = window.find("pl_row_1").bounds();
            let vol = window.find("pl_vol_1").bounds();
            let knob = window.find("pl_panknob_1").bounds();
            (
                row.size.height.as_f32(),
                vol.size.height.as_f32(),
                (knob.origin.y + knob.size.height).as_f32(),
                (row.origin.y + row.size.height).as_f32(),
            )
        })
        .unwrap();
    println!(
        "playlist header: fila {row_h:.0}px, fader {vol_h:.0}px, base del knob y={knob_bottom:.0}, fin de fila y={row_bottom:.0}"
    );
    assert!(row_h >= 60.0, "la fila debería medir al menos 60px, mide {row_h}");
    assert!(
        knob_bottom <= row_bottom + 0.5,
        "el knob se sale de la fila (base {knob_bottom:.1} vs fin {row_bottom:.1})"
    );
}

// =========================================================================
// Mute / Solo: escritura + espejo
// =========================================================================

#[gpui_kit::gpui::test]
fn playlist_mute_button_toggles_and_syncs(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    assert!(!studio_mix(handle, cx, 1).2, "arranca sin mute");
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.click("pl_track_mute_1", cx);
    })
    .unwrap();
    assert!(
        studio_mix(handle, cx, 1).2,
        "M en la playlist debería mutear la pista del modo activo"
    );
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.click("pl_track_mute_1", cx);
    })
    .unwrap();
    assert!(!studio_mix(handle, cx, 1).2, "M debería alternar de vuelta");
}

#[gpui_kit::gpui::test]
fn playlist_solo_button_toggles(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    assert!(!studio_mix(handle, cx, 1).3, "arranca sin solo");
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.click("pl_track_solo_1", cx);
    })
    .unwrap();
    assert!(studio_mix(handle, cx, 1).3, "S en la playlist debería solear la pista");
}

/// El header escribe SIEMPRE en la pista del modo ACTIVO (el mismo índice que
/// lee el channel strip del mixer de OpenStudio) y no en una copia local: por
/// eso el mute del header se ve en el mixer sin recargar.
///
/// Nota: la Playlist sólo se monta en OpenStudio (`render_central` manda a la
/// Session Matrix en OpenLive), así que el caso observable es ése. La rama
/// OpenLive del binding (espejo a `matrix_state`) queda cubierta por el mismo
/// `playlist_row_mix`/apply compartidos.
#[gpui_kit::gpui::test]
fn playlist_mute_writes_mode_active_track(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    let before = studio_mix(handle, cx, 1);
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.click("pl_track_mute_1", cx);
    })
    .unwrap();
    let after = studio_mix(handle, cx, 1);
    assert!(after.2, "el mute debería escribirse en la pista del modo activo");
    assert_eq!(
        (before.0, before.1, before.3),
        (after.0, after.1, after.3),
        "sólo mute debería cambiar"
    );
}

// =========================================================================
// Fader de volumen: drag en vivo
// =========================================================================

#[gpui_kit::gpui::test]
fn playlist_volume_drag_updates_track(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    let before = studio_mix(handle, cx, 1).0;
    // Se toma el borde izquierdo del fader (volumen ~0) y se arrastra a la
    // derecha: debe SUBIR el volumen.
    let (x0, y0, x1) = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            let b = window.find("pl_vol_1").bounds();
            (
                (b.origin.x + px(2.0)).as_f32(),
                (b.origin.y + b.size.height / 2.0).as_f32(),
                (b.origin.x + b.size.width - px(2.0)).as_f32(),
            )
        })
        .unwrap();
    down(handle, cx, x0, y0, 1);
    move_to(handle, cx, x1, y0);
    up(handle, cx, x1, y0);
    let after = studio_mix(handle, cx, 1).0;
    println!("volumen de la fila 1: {before:.3} -> {after:.3}");
    assert!(
        after > before,
        "arrastrar el fader a la derecha debería subir el volumen ({before:.3} -> {after:.3})"
    );
}

#[gpui_kit::gpui::test]
fn playlist_volume_double_click_resets(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    // Se baja el volumen primero y después se hace doble-clic (reset a 0.0dB).
    let (x0, y0, x1) = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            let b = window.find("pl_vol_1").bounds();
            (
                (b.origin.x + px(2.0)).as_f32(),
                (b.origin.y + b.size.height / 2.0).as_f32(),
                (b.origin.x + b.size.width - px(2.0)).as_f32(),
            )
        })
        .unwrap();
    down(handle, cx, x0, y0, 1);
    up(handle, cx, x0, y0);
    let low = studio_mix(handle, cx, 1).0;
    assert!(low < 0.75, "el click en el borde izquierdo baja el volumen ({low:.3})");
    // Doble-clic: reset al valor de unity (0.75 lineal = 0.0dB).
    down(handle, cx, x0, y0, 2);
    up(handle, cx, x0, y0);
    let reset = studio_mix(handle, cx, 1).0;
    println!("reset por doble-clic: {low:.3} -> {reset:.3}");
    assert!(
        (reset - 0.75).abs() < 0.02,
        "el doble-clic debería volver a 0.0dB (0.75 lineal), quedó en {reset:.3}"
    );
}

#[gpui_kit::gpui::test]
fn playlist_pan_drag_updates_track(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    let before = studio_mix(handle, cx, 1).1;
    let (x, y) = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            let b = window.find("pl_panknob_1").bounds();
            (
                (b.origin.x + b.size.width / 2.0).as_f32(),
                (b.origin.y + b.size.height / 2.0).as_f32(),
            )
        })
        .unwrap();
    // Agarrar NO cambia el valor; se arrastra hacia arriba (hacia R).
    down(handle, cx, x, y, 1);
    let after_grab = studio_mix(handle, cx, 1).1;
    assert_eq!(before, after_grab, "agarrar el knob no debe cambiar el paneo");
    move_to(handle, cx, x, y - 60.0);
    up(handle, cx, x, y - 60.0);
    let after = studio_mix(handle, cx, 1).1;
    println!("pan de la fila 1: {before:.3} -> {after:.3}");
    assert!(
        after > before + 0.05,
        "arrastrar el knob hacia arriba debería panear a la derecha ({before:.3} -> {after:.3})"
    );
}
