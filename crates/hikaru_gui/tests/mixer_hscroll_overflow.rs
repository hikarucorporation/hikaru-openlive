// crates/hikaru_gui/tests/mixer_hscroll_overflow.rs
//
// Regresión del "muro invisible": con suficientes pistas, la última columna
// debe extenderse más allá del borde derecho del viewport horizontal
// (`arranger_mixer_hscroll`) y el scroll con rueda debe mover el contenido.
// Si el contenido midiera lo mismo que el viewport, el `scroll_max`
// horizontal quedaría en ~0 y ni la rueda ni el pan con botón central
// podrían llegar a las últimas pistas.

use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Arc;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{px, point, size, AppContext, Context, ScrollDelta, TestAppContext, Window};
use gpui_kit::{MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent};

use hikaru_gui::app::{AppMode, HikaruApp};
use hikaru_gui::audio_proxy::{AudioProxy, GuiCommand};

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

fn open_arranger(cx: &mut TestAppContext) -> gpui_kit::WindowHandle<HikaruApp> {
    cx.update(gpui_kit::init);
    let handle = cx.add_window(|window, cx| test_app(window, cx));
    cx.simulate_window_resize(handle.into(), size(px(1280.0), px(720.0)));
    handle
}

fn set_openlive_arranger(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.mode = AppMode::OpenLive;
                s.openlive_view = hikaru_gui::app::OpenLiveView::ArrangerView;
                cx.notify();
            });
        });
    })
    .unwrap();
}

/// Índice de la última pista de matriz (las columnas usan `arr_track_{idx}_rec`).
fn last_track_idx(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) -> usize {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        let mut len = 0;
        app.update(cx, |app, cx| {
            len = app.state.read(cx).matrix_state.tracks.len();
        });
        len.saturating_sub(1)
    })
    .unwrap()
}

#[gpui_kit::gpui::test]
fn mixer_content_overflows_viewport_with_many_tracks(cx: &mut TestAppContext) {
    let handle = open_arranger(cx);
    set_openlive_arranger(handle, cx);
    // 8 pistas default (~1136px + SCENES) casi encajan a 1280: se agregan 4
    // para garantizar desborde real (~570px de contenido extra).
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        for _ in 0..4 {
            window.click("arr_add_track", cx);
        }
    })
    .unwrap();

    let idx = last_track_idx(handle, cx);
    let last_id = format!("arr_track_{}_rec", idx);
    // El viewport horizontal ocupa todo el ancho del área de columnas (los
    // tres niveles son `w_full` encadenados): el borde derecho de
    // `arranger_columns` (observable) equivale al del scroller.
    let (vp_right, last_right) = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            let vp = window.find("arranger_columns");
            let last = window.find(last_id.clone());
            let vb = vp.bounds();
            let lb = last.bounds();
            (
                vb.origin.x.as_f32() + vb.size.width.as_f32(),
                lb.origin.x.as_f32() + lb.size.width.as_f32(),
            )
        })
        .unwrap();
    println!(
        "mixer hscroll: viewport right {:.1}, last col right {:.1} (overflow {:.1})",
        vp_right,
        last_right,
        last_right - vp_right
    );
    assert!(
        last_right > vp_right + 100.0,
        "la última columna (der. en {last_right}) debería desbordar el viewport (der. en {vp_right}): sin desborde no hay scroll horizontal posible"
    );
}

#[gpui_kit::gpui::test]
fn mixer_horizontal_scroll_moves_content(cx: &mut TestAppContext) {
    let handle = open_arranger(cx);
    set_openlive_arranger(handle, cx);
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        for _ in 0..4 {
            window.click("arr_add_track", cx);
        }
    })
    .unwrap();

    let idx = last_track_idx(handle, cx);
    let last_id = format!("arr_track_{}_rec", idx);
    // La rueda se dispara sobre un control observable dentro del área (el
    // evento burbujea hasta el scroller horizontal).
    let (before, after) = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            let b = window.find(last_id.clone()).bounds();
            let before = b.origin.x.as_f32();
            window.scroll(
                "arr_track_0_rec",
                ScrollDelta::Pixels(point(px(-300.0), px(0.0))),
                cx,
            );
            let a = window.find(last_id.clone()).bounds();
            (before, a.origin.x.as_f32())
        })
        .unwrap();
    println!(
        "mixer hscroll wheel: x antes {:.1}, después {:.1} (Δ {:.1})",
        before,
        after,
        after - before
    );
    assert!(
        after < before - 50.0,
        "la rueda horizontal debería mover el contenido a la izquierda (antes {before}, después {after}): scroll_max ~0 = muro invisible"
    );
}

/// Lee `(offset_x, max_offset_x)` del handle horizontal del mixer.
fn mixer_h_offset(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) -> (f32, f32) {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        let mut out = (0.0f32, 0.0f32);
        app.update(cx, |app, cx| {
            let h = app.state.read(cx).mixer_scroll_h.clone();
            out = (h.offset().x.as_f32(), h.max_offset().x.as_f32());
        });
        out
    })
    .unwrap()
}

fn middle_down(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext, x: f32, y: f32) {
    cx.update_window(handle.into(), |_, window, cx| {
        use gpui_kit::InputEvent as _;
        window.dispatch_event(
            MouseDownEvent {
                button: MouseButton::Middle,
                position: point(px(x), px(y)),
                modifiers: Default::default(),
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

fn middle_move(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext, x: f32, y: f32) {
    cx.update_window(handle.into(), |_, window, cx| {
        use gpui_kit::InputEvent as _;
        window.dispatch_event(
            MouseMoveEvent {
                position: point(px(x), px(y)),
                pressed_button: Some(MouseButton::Middle),
                modifiers: Default::default(),
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

fn middle_up(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext, x: f32, y: f32) {
    cx.update_window(handle.into(), |_, window, cx| {
        use gpui_kit::InputEvent as _;
        window.dispatch_event(
            MouseUpEvent {
                button: MouseButton::Middle,
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

/// Middle-drag real sobre la grilla: el offset queda clampeado a
/// `[-max, 0]` (el thumb de la scrollbar lee el mismo handle, así queda
/// sincronizado) e invertir la dirección en el borde responde de inmediato,
/// sin zona muerta.
#[gpui_kit::gpui::test]
fn mixer_middle_drag_pans_and_clamps(cx: &mut TestAppContext) {
    let handle = open_arranger(cx);
    set_openlive_arranger(handle, cx);
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        for _ in 0..4 {
            window.click("arr_add_track", cx);
        }
    })
    .unwrap();

    // Punto de partida: centro del REC de la primera pista (dentro del área).
    let (sx, sy) = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            let b = window.find("arr_track_0_rec").bounds();
            (
                (b.origin.x + b.size.width / 2.0).as_f32(),
                (b.origin.y + b.size.height / 2.0).as_f32(),
            )
        })
        .unwrap();

    // Arrastre a la derecha en pasos (2x sensibilidad: 600px de cursor piden
    // ~1200px de scroll, más que el máximo con 12 pistas → debe clampear).
    middle_down(handle, cx, sx, sy);
    for i in 1..=6 {
        middle_move(handle, cx, sx + i as f32 * 100.0, sy);
    }
    let (ox, maxx) = mixer_h_offset(handle, cx);
    println!(
        "mixer middle-drag: offset_x {:.1}, max {:.1}",
        ox, maxx
    );
    assert!(maxx > 100.0, "con 12 pistas debería haber scroll horizontal real");
    assert!(
        ox <= 0.0 && ox >= -maxx - 0.5,
        "el offset ({ox}) debe estar clampeado a [-{maxx}, 0]: si no, el thumb se desincroniza"
    );
    assert!(
        ox < -100.0,
        "el arrastre debería haber desplazado el viewport (offset {ox})"
    );

    // Invertir un poco en el borde: con clamp incremental responde al instante
    // (con el esquema absoluto viejo seguiría clampeado = zona muerta).
    middle_move(handle, cx, sx + 500.0, sy);
    let (ox2, _) = mixer_h_offset(handle, cx);
    middle_up(handle, cx, sx + 500.0, sy);
    println!("mixer middle-drag reverse: {:.1} -> {:.1}", ox, ox2);
    assert!(
        ox2 > ox + 50.0,
        "al invertir la dirección el viewport debería volver de inmediato ({ox} -> {ox2})"
    );
}
