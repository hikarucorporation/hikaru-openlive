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
use gpui_kit::{point, px, size, AppContext, Context, TestAppContext, Window};

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

/// La grilla default de 8×8 a 1280×720 no debe desbordar el contenedor de
/// filas (sin scrollbar vertical): la última fila queda dentro del área.
#[gpui_kit::gpui::test]
fn default_8x8_grid_fits_without_vertical_scroll(cx: &mut TestAppContext) {
    let handle = open_matrix(cx);
    cx.simulate_window_resize(handle.into(), size(px(1280.0), px(720.0)));
    let (rows_bottom, last_top, last_bottom) = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            let rows = window.find("matrix_rows");
            let last = window.find("matrix_pad_7_7");
            let rb = rows.bounds();
            let lb = last.bounds();
            (
                rb.origin.y.as_f32() + rb.size.height.as_f32(),
                lb.origin.y.as_f32(),
                lb.origin.y.as_f32() + lb.size.height.as_f32(),
            )
        })
        .unwrap();
    assert!(
        last_top >= 0.0 && last_bottom <= rows_bottom + 1.0,
        "la fila 8 (fondo en {last_bottom}) debería caber en el contenedor (fondo en {rows_bottom})"
    );
    println!(
        "8x8 slack vertical: {:.1}px (contenedor {:.1}, fila8 {:.1})",
        rows_bottom - last_bottom,
        rows_bottom,
        last_bottom
    );
}

/// El gesto de mezcla nunca queda colgado tras soltar el botón.
fn mix_drag(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) -> bool {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        let st = app.read(cx).state.clone();
        st.read(cx).matrix_mix_drag.is_some()
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
    assert!(
        !mix_drag(handle, cx),
        "el mouse_up debería haber cerrado el gesto de mezcla"
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

    // Lado derecho del knob (hitbox 28×28, visual centrada en 14,14):
    // ~+90° → R.
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.click_at("matrix_panknob_0", point(px(24.0), px(14.0)), cx);
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
        window.click_at("matrix_panknob_0", point(px(4.0), px(14.0)), cx);
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
        window.click_at("matrix_panknob_0", point(px(14.0), px(4.0)), cx);
    })
    .unwrap();
    assert!(
        track_mix(handle, cx).1.abs() < 0.05,
        "clic arriba del knob debería volver al centro"
    );
}
