// crates/hikaru_gui/tests/matrix_header_click.rs
//
// Tests de interacción del Track Header de la Session Matrix.
//
// Reproducen clics reales (con hit-testing) sobre los controles de mezcla
// del header — fader de volumen, botones S/M y knob de pan — y verifican que
// el estado de la matriz cambia. Si un control deja de recibir eventos del
// mouse, estos tests lo detectan sin necesidad de abrir la app.

use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Arc;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{point, px, AppContext, Context, TestAppContext, Window};

use hikaru_gui::app::HikaruApp;
use hikaru_gui::audio_proxy::{AudioProxy, GuiCommand};

fn test_app(window: &mut Window, cx: &mut Context<HikaruApp>) -> HikaruApp {
    // El receptor se descarta a propósito: `AudioProxy::send` ignora el
    // error de envío, así que los comandos del header no bloquean ni
    // rompen el test.
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

fn open_matrix(cx: &mut TestAppContext) -> gpui_kit::WindowHandle<HikaruApp> {
    // Igual que el binario (`main.rs`): inicializar las capas de gpui-kit
    // (tema de componentes) antes de abrir la ventana.
    cx.update(gpui_kit::init);
    let handle = cx.add_window(|window, cx| test_app(window, cx));
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        // La vista por defecto ya es la Session Matrix; basta con que el
        // header de la pista 0 esté laid out y visible.
        assert!(window.try_find("matrix_vol_0").is_some());
        assert!(window.try_find("matrix_panknob_0").is_some());
    })
    .unwrap();
    handle
}

fn track_mix(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) -> (f32, f32, bool) {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        let st = app.read(cx).state.clone();
        let t = &st.read(cx).matrix_state.tracks[0];
        (t.volume, t.pan, t.muted)
    })
    .unwrap()
}

/// Clic en el extremo izquierdo del fader de volumen: salta al mínimo.
#[gpui_kit::gpui::test]
fn clicking_volume_fader_jumps_to_point(cx: &mut TestAppContext) {
    let handle = open_matrix(cx);
    let (before, _, _) = track_mix(handle, cx);
    assert!((before - 0.75).abs() < f32::EPSILON);

    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.click_at("matrix_vol_0", point(px(4.0), px(8.0)), cx);
    })
    .unwrap();

    let (after, _, _) = track_mix(handle, cx);
    assert!(
        after < 0.5,
        "el clic a la izquierda del fader debería bajar el volumen (quedó en {after})"
    );
}

/// El botón M del header sigue alternando el mute (control de la prueba).
#[gpui_kit::gpui::test]
fn header_mute_button_toggles(cx: &mut TestAppContext) {
    let handle = open_matrix(cx);
    assert!(!track_mix(handle, cx).2);

    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.click("track_mute_0", cx);
    })
    .unwrap();

    assert!(track_mix(handle, cx).2);
}

/// Clic en el knob: salta al ángulo apuntado (derecha → R, izquierda → L,
/// arriba → centro). Antes el clic simple era un no-op silencioso y parecía
/// que el knob no respondía.
#[gpui_kit::gpui::test]
fn clicking_pan_knob_jumps_to_angle(cx: &mut TestAppContext) {
    let handle = open_matrix(cx);
    assert_eq!(track_mix(handle, cx).1, 0.0);

    // Lado derecho del knob (hitbox 38×38, visual centrada en 19,19):
    // ~+90° → R.
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.click_at("matrix_panknob_0", point(px(32.0), px(19.0)), cx);
    })
    .unwrap();
    let right = track_mix(handle, cx).1;
    assert!(
        right > 0.5,
        "clic a la derecha del knob debería panea a R (quedó en {right})"
    );

    // Lado izquierdo: ~−90° → L.
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.click_at("matrix_panknob_0", point(px(6.0), px(19.0)), cx);
    })
    .unwrap();
    let left = track_mix(handle, cx).1;
    assert!(
        left < -0.5,
        "clic a la izquierda del knob debería panear a L (quedó en {left})"
    );

    // Arriba en punto: centro.
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.click_at("matrix_panknob_0", point(px(19.0), px(9.0)), cx);
    })
    .unwrap();
    assert!(
        track_mix(handle, cx).1.abs() < 0.05,
        "clic arriba del knob debería volver al centro"
    );
}
