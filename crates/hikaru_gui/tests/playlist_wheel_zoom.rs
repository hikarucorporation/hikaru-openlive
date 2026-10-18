// crates/hikaru_gui/tests/playlist_wheel_zoom.rs
//
// Zoom de la Playlist / Timeline con rueda del mouse + modificadores:
// - `Ctrl` + rueda arriba/abajo: zoom horizontal (`zoom_x`), anclado al cursor.
// - `Alt` + rueda (o `Ctrl` + `Shift` + rueda): zoom vertical (`row_h`).
// - Sin modificadores la rueda sigue scrolleando (no hace zoom).
// - Ambos zooms están acotados para no romper el layout.

use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Arc;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    px, point, size, AppContext, Context, Modifiers, ScrollDelta, ScrollWheelEvent, TestAppContext,
    Window,
};

use hikaru_gui::app::{AppMode, HikaruApp};
use hikaru_gui::audio_proxy::{AudioProxy, GuiCommand};
use hikaru_gui::views::mixer::Track;
use hikaru_gui::views::playlist::{
    anchor_h_offset, anchor_v_offset, clamped_row_h, smooth_row_h, smooth_zoom, step_row_h,
    wheel_notches, wheel_zoom_in, LINES_PER_NOTCH, MAX_ROW_H, MAX_ZOOM_X, MIN_ROW_H, MIN_ZOOM_X,
    PX_PER_NOTCH, TRACK_ROW_H, ZOOM_STEP_FACTOR,
};

// =========================================================================
// Matemática pura (sin ventana)
// =========================================================================

#[test]
fn row_h_clamps_to_limits() {
    assert_eq!(clamped_row_h(TRACK_ROW_H), TRACK_ROW_H);
    assert_eq!(clamped_row_h(0.0), MIN_ROW_H);
    assert_eq!(clamped_row_h(-50.0), MIN_ROW_H);
    assert_eq!(clamped_row_h(10_000.0), MAX_ROW_H);
    assert_eq!(clamped_row_h(f32::NAN), TRACK_ROW_H);
    assert_eq!(clamped_row_h(f32::INFINITY), TRACK_ROW_H);
}

#[test]
fn step_row_h_moves_both_ways_and_clamps() {
    assert!(step_row_h(TRACK_ROW_H, true) > TRACK_ROW_H);
    assert!(step_row_h(TRACK_ROW_H, false) < TRACK_ROW_H);
    assert_eq!(step_row_h(MAX_ROW_H, true), MAX_ROW_H);
    assert_eq!(step_row_h(MIN_ROW_H, false), MIN_ROW_H);
}

#[test]
fn wheel_up_is_zoom_in_down_is_zoom_out() {
    // Píxeles (trackpads): subir resta.
    assert!(wheel_zoom_in(&ScrollDelta::Pixels(point(px(0.0), px(-60.0)))));
    assert!(!wheel_zoom_in(&ScrollDelta::Pixels(point(px(0.0), px(60.0)))));
    // Líneas (rueda clásica).
    assert!(wheel_zoom_in(&ScrollDelta::Lines(point(0.0, -3.0))));
    assert!(!wheel_zoom_in(&ScrollDelta::Lines(point(0.0, 3.0))));
}

#[test]
fn anchor_h_keeps_tick_under_cursor() {
    // Cursor a 400px del viewport, headers de 180, zoom 0.1 → 1.25×.
    // El tick bajo el cursor antes y después debe coincidir.
    let cursor_vx = 400.0f32;
    let header_w = 180.0;
    let old_zoom = 0.1;
    let new_zoom = 0.125;
    let old_offset = -200.0;
    let new_offset = anchor_h_offset(cursor_vx, header_w, old_zoom, new_zoom, old_offset, 800.0, 5000.0);
    let tick_before = (cursor_vx - old_offset - header_w) / old_zoom;
    let tick_after = (cursor_vx - new_offset - header_w) / new_zoom;
    assert!(
        (tick_before - tick_after).abs() < 0.01,
        "tick antes {tick_before} vs después {tick_after}"
    );
}

#[test]
fn anchor_h_clamps_to_content_bounds() {
    // Contenido más chico que el viewport: offset 0.
    assert_eq!(
        anchor_h_offset(400.0, 180.0, 0.1, 0.125, 0.0, 1000.0, 500.0),
        0.0
    );
    // Pasado del inicio: no baja de -max.
    let off = anchor_h_offset(400.0, 180.0, 0.1, 10.0, 0.0, 800.0, 5000.0);
    assert!(off >= -(5000.0 - 800.0) && off <= 0.0, "offset {off}");
}

#[test]
fn anchor_v_keeps_content_under_cursor() {
    // Filas de 54 → 67.5 (×1.25): el punto de contenido bajo el cursor
    // debe quedar bajo el cursor tras compensar el offset.
    let cursor_vy = 200.0f32;
    let ruler_h = 24.0;
    let old_h = 54.0;
    let new_h = 67.5;
    let old_offset = -30.0;
    let new_offset = anchor_v_offset(cursor_vy, ruler_h, old_h, new_h, old_offset, 600.0, 800.0);
    let content_before = cursor_vy - old_offset;
    let content_after = cursor_vy - new_offset;
    let expected = ruler_h + (content_before - ruler_h) * (new_h / old_h);
    assert!(
        (content_after - expected).abs() < 0.01,
        "contenido {content_after} vs esperado {expected}"
    );
}

#[test]
fn anchor_v_clamps_to_content_bounds() {
    assert_eq!(anchor_v_offset(200.0, 24.0, 54.0, 67.5, 0.0, 900.0, 300.0), 0.0);
    let off = anchor_v_offset(200.0, 24.0, 54.0, 160.0, -100.0, 600.0, 800.0);
    assert!(off >= -(800.0 - 600.0) && off <= 0.0, "offset {off}");
}

// =========================================================================
// Zoom continuo: la magnitud del delta se preserva
// =========================================================================

#[test]
fn full_notch_matches_the_old_fixed_step() {
    // Compatibilidad: 3 líneas arriba debe dar exactamente el mismo ×1.25
    // que daba el paso fijo, para que la rueda clásica no cambie de feel.
    let n = wheel_notches(&ScrollDelta::Lines(point(0.0, -LINES_PER_NOTCH)));
    assert!((n - 1.0).abs() < 1e-6, "3 líneas arriba deberían ser 1 notch, dio {n}");
    let z = smooth_zoom(0.1, n);
    assert!((z - 0.125).abs() < 1e-6, "0.1 × 1.25 = 0.125, dio {z}");

    let n_out = wheel_notches(&ScrollDelta::Lines(point(0.0, LINES_PER_NOTCH)));
    let z_out = smooth_zoom(0.125, n_out);
    assert!((z_out - 0.1).abs() < 1e-6, "0.125 ÷ 1.25 = 0.1, dio {z_out}");
}

#[test]
fn trackpad_nudge_moves_less_than_a_full_notch() {
    // Ésta es la razón del cambio: antes `wheel_zoom_in` tiraba la magnitud
    // y un nudge de 2px saltaba el 25% entero igual que un notch completo.
    let nudge = wheel_notches(&ScrollDelta::Pixels(point(px(0.0), px(-2.0))));
    let notch = wheel_notches(&ScrollDelta::Pixels(point(px(0.0), px(-PX_PER_NOTCH))));
    assert!(
        nudge > 0.0 && nudge < 0.1,
        "un nudge de 2px debería ser una fracción de notch, dio {nudge}"
    );
    assert!((notch - 1.0).abs() < 1e-6, "un notch completo debería dar 1.0, dio {notch}");

    let z0 = 0.1f32;
    let zn = smooth_zoom(z0, nudge);
    let zf = smooth_zoom(z0, notch);
    assert!(zn < zf, "el nudge debería acercar menos que el notch ({zn} vs {zf})");
    // Y el movimiento del nudge debe ser una fracción correspondientemente chica.
    assert!(
        (zn - z0) < (zf - z0) * 0.15,
        "el nudge ({zn}) no debería acercarse al salto del notch ({zf})"
    );
}

#[test]
fn wheel_notches_sign_matches_wheel_zoom_in() {
    // `wheel_notches` debe concordar en signo con el viejo booleano: arriba
    // (delta negativo en GPUI) siempre es positivo = zoom in.
    for d in [-1.0f32, -3.0, -53.0] {
        assert!(wheel_zoom_in(&ScrollDelta::Lines(point(0.0, d))));
        assert!(wheel_notches(&ScrollDelta::Lines(point(0.0, d))) > 0.0);
    }
    for d in [1.0f32, 3.0, 53.0] {
        assert!(!wheel_zoom_in(&ScrollDelta::Lines(point(0.0, d))));
        assert!(wheel_notches(&ScrollDelta::Lines(point(0.0, d))) < 0.0);
    }
    // Píxeles: mismo criterio.
    assert!(wheel_notches(&ScrollDelta::Pixels(point(px(0.0), px(-10.0)))) > 0.0);
    assert!(wheel_notches(&ScrollDelta::Pixels(point(px(0.0), px(10.0)))) < 0.0);
}

#[test]
fn wheel_notches_is_zero_and_safe_on_degenerate_input() {
    assert_eq!(wheel_notches(&ScrollDelta::Lines(point(0.0, 0.0))), 0.0);
    assert_eq!(wheel_notches(&ScrollDelta::Pixels(point(px(0.0), px(0.0)))), 0.0);
    // NaN / infinito no deben producir NaN que rompa el offset.
    assert_eq!(wheel_notches(&ScrollDelta::Lines(point(0.0, f32::NAN))), 0.0);
    assert_eq!(wheel_notches(&ScrollDelta::Lines(point(0.0, f32::INFINITY))), 0.0);
}

#[test]
fn wheel_notches_falls_back_to_horizontal_delta() {
    // Sin componente vertical manda el horizontal (trackpad en diagonal o
    // rueda doble eje), igual que `wheel_zoom_in`.
    let n = wheel_notches(&ScrollDelta::Pixels(point(px(-PX_PER_NOTCH), px(0.0))));
    assert!((n - 1.0).abs() < 1e-6, "el delta horizontal debería mandar, dio {n}");
}

#[test]
fn smooth_zoom_is_continuous_and_clamped() {
    // Monótono creciente con los notches.
    let mut prev = smooth_zoom(0.1, -2.0);
    for n in [-1.5f32, -1.0, -0.5, 0.0, 0.5, 1.0, 1.5] {
        let z = smooth_zoom(0.1, n);
        assert!(z >= prev, "el zoom debería crecer con los notches ({prev} -> {z})");
        prev = z;
    }
    // Un notch partido da el cuadrado del factor, no el doble.
    let half_then_half = smooth_zoom(smooth_zoom(0.1, 0.5), 0.5);
    let one_shot = smooth_zoom(0.1, 1.0);
    assert!(
        (half_then_half - one_shot).abs() < 1e-6,
        "dos medios notches equivalen a uno entero ({half_then_half} vs {one_shot})"
    );
    // Clamps en los extremos.
    assert_eq!(smooth_zoom(MAX_ZOOM_X, 5.0), MAX_ZOOM_X);
    assert_eq!(smooth_zoom(MIN_ZOOM_X, -5.0), MIN_ZOOM_X);
    // Notches degenerados no rompen nada.
    assert_eq!(smooth_zoom(0.1, f32::NAN), 0.1);
    assert_eq!(smooth_zoom(0.1, 0.0), 0.1);
}

#[test]
fn smooth_row_h_is_continuous_and_clamped() {
    let mut prev = smooth_row_h(TRACK_ROW_H, -2.0);
    for n in [-1.0f32, -0.5, 0.0, 0.5, 1.0] {
        let h = smooth_row_h(TRACK_ROW_H, n);
        assert!(h >= prev, "la fila debería crecer con los notches ({prev} -> {h})");
        prev = h;
    }
    assert_eq!(smooth_row_h(MAX_ROW_H, 3.0), MAX_ROW_H);
    assert_eq!(smooth_row_h(MIN_ROW_H, -3.0), MIN_ROW_H);
    assert_eq!(smooth_row_h(TRACK_ROW_H, f32::NAN), TRACK_ROW_H);
}

#[test]
fn smooth_zoom_still_anchors_the_tick_under_the_cursor() {
    // La parte importante: el anclaje no se rompió al pasar a zoom continuo.
    // Con un zoom fraccionario el tick bajo el cursor debe seguir fijo.
    let cursor_vx = 400.0f32;
    let header_w = 180.0;
    let old_zoom = 0.1f32;
    let old_offset = -200.0f32;
    for n in [-0.75f32, -0.25, 0.25, 0.75, 1.25] {
        let new_zoom = smooth_zoom(old_zoom, n);
        let new_offset =
            anchor_h_offset(cursor_vx, header_w, old_zoom, new_zoom, old_offset, 800.0, 5000.0);
        let tick_before = (cursor_vx - old_offset - header_w) / old_zoom;
        let tick_after = (cursor_vx - new_offset - header_w) / new_zoom;
        assert!(
            (tick_before - tick_after).abs() < 0.01,
            "notches {n}: tick antes {tick_before} vs después {tick_after}"
        );
    }
}

#[test]
fn zoom_step_factor_is_the_documented_one() {
    // Si alguien cambia la constante, el resto de las aserciones de
    // continuidad tienen que seguir siendo coherentes con ella.
    assert!((ZOOM_STEP_FACTOR - 1.25).abs() < 1e-6);
    const { assert!(LINES_PER_NOTCH > 0.0 && PX_PER_NOTCH > 0.0) };
}

// =========================================================================
// GUI: rueda real sobre la playlist
// =========================================================================

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

/// Punto visible sobre la grilla: centro del header de la primera fila
/// (los headers viajan dentro del contenido de `playlist_grid`, así que el
/// evento burbujea por la grilla y el handler de zoom lo ve).
fn grid_point(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) -> (f32, f32) {
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        let b = window.find("pl_row_1").bounds();
        (
            (b.origin.x + b.size.width / 2.0).as_f32(),
            (b.origin.y + b.size.height / 2.0).as_f32(),
        )
    })
    .unwrap()
}

fn wheel(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    x: f32,
    y: f32,
    dy_lines: f32,
    modifiers: Modifiers,
) {
    cx.update_window(handle.into(), |_, window, cx| {
        use gpui_kit::{InputEvent as _, MouseMoveEvent};
        window.dispatch_event(
            MouseMoveEvent {
                position: point(px(x), px(y)),
                pressed_button: None,
                modifiers,
            }
            .to_platform_input(),
            cx,
        );
        window.dispatch_event(
            ScrollWheelEvent {
                position: point(px(x), px(y)),
                delta: ScrollDelta::Lines(point(0.0, dy_lines)),
                modifiers,
                ..Default::default()
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

/// Rueda en **píxeles** (como manda un trackpad) con modificadores.
fn wheel_px(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    x: f32,
    y: f32,
    dy_px: f32,
    modifiers: Modifiers,
) {
    cx.update_window(handle.into(), |_, window, cx| {
        use gpui_kit::{InputEvent as _, MouseMoveEvent};
        window.dispatch_event(
            MouseMoveEvent {
                position: point(px(x), px(y)),
                pressed_button: None,
                modifiers,
            }
            .to_platform_input(),
            cx,
        );
        window.dispatch_event(
            ScrollWheelEvent {
                position: point(px(x), px(y)),
                delta: ScrollDelta::Pixels(point(px(0.0), px(dy_px))),
                modifiers,
                ..Default::default()
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

fn zooms(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) -> (f32, f32) {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        let mut out = (0.0f32, 0.0f32);
        app.update(cx, |app, cx| {
            let pl = &app.state.read(cx).playlist_state;
            out = (pl.zoom_x, pl.row_h);
        });
        out
    })
    .unwrap()
}

#[gpui_kit::gpui::test]
fn ctrl_wheel_up_zooms_in_horizontally(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    let (x, y) = grid_point(handle, cx);
    let (zx0, _) = zooms(handle, cx);
    wheel(handle, cx, x, y, -3.0, Modifiers { control: true, ..Default::default() });
    let (zx1, _) = zooms(handle, cx);
    assert!(zx1 > zx0, "Ctrl+rueda arriba debería acercar ({zx0} -> {zx1})");
}

#[gpui_kit::gpui::test]
fn ctrl_wheel_down_zooms_out_horizontally(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    let (x, y) = grid_point(handle, cx);
    let (zx0, _) = zooms(handle, cx);
    wheel(handle, cx, x, y, 3.0, Modifiers { control: true, ..Default::default() });
    let (zx1, _) = zooms(handle, cx);
    assert!(zx1 < zx0, "Ctrl+rueda abajo debería alejar ({zx0} -> {zx1})");
}

#[gpui_kit::gpui::test]
fn horizontal_zoom_clamps_to_limits(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    let (x, y) = grid_point(handle, cx);
    let mods = Modifiers { control: true, ..Default::default() };
    for _ in 0..60 {
        wheel(handle, cx, x, y, -3.0, mods);
    }
    let (zx_max, _) = zooms(handle, cx);
    assert!(
        (zx_max - MAX_ZOOM_X).abs() < 0.001,
        "el zoom in debería clampear en {MAX_ZOOM_X}, quedó en {zx_max}"
    );
    for _ in 0..120 {
        wheel(handle, cx, x, y, 3.0, mods);
    }
    let (zx_min, _) = zooms(handle, cx);
    assert!(
        (zx_min - MIN_ZOOM_X).abs() < 0.001,
        "el zoom out debería clampear en {MIN_ZOOM_X}, quedó en {zx_min}"
    );
}

#[gpui_kit::gpui::test]
fn alt_wheel_up_grows_row_height(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    let (x, y) = grid_point(handle, cx);
    let (zx0, rh0) = zooms(handle, cx);
    wheel(handle, cx, x, y, -3.0, Modifiers { alt: true, ..Default::default() });
    let (zx1, rh1) = zooms(handle, cx);
    assert!(rh1 > rh0, "Alt+rueda arriba debería agrandar filas ({rh0} -> {rh1})");
    assert_eq!(zx0, zx1, "el zoom vertical no debería tocar el zoom horizontal");
}

#[gpui_kit::gpui::test]
fn alt_wheel_down_shrinks_row_height(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    let (x, y) = grid_point(handle, cx);
    let (_, rh0) = zooms(handle, cx);
    wheel(handle, cx, x, y, 3.0, Modifiers { alt: true, ..Default::default() });
    let (_, rh1) = zooms(handle, cx);
    assert!(rh1 < rh0, "Alt+rueda abajo debería achicar filas ({rh0} -> {rh1})");
}

#[gpui_kit::gpui::test]
fn ctrl_shift_wheel_grows_row_height(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    let (x, y) = grid_point(handle, cx);
    let (zx0, rh0) = zooms(handle, cx);
    wheel(
        handle,
        cx,
        x,
        y,
        -3.0,
        Modifiers { control: true, shift: true, ..Default::default() },
    );
    let (zx1, rh1) = zooms(handle, cx);
    assert!(rh1 > rh0, "Ctrl+Shift+rueda arriba debería agrandar filas ({rh0} -> {rh1})");
    assert_eq!(zx0, zx1, "Ctrl+Shift+rueda no debería tocar el zoom horizontal");
}

/// El comportamiento que motivó el cambio, verificado end-to-end por el
/// camino real de eventos: un nudge de trackpad mueve el zoom mucho menos que
/// un notch completo. Antes ambos movían exactamente ×1.25.
#[gpui_kit::gpui::test]
fn trackpad_nudge_zooms_less_than_a_full_notch(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    let (x, y) = grid_point(handle, cx);
    let (z0, _) = zooms(handle, cx);

    wheel_px(handle, cx, x, y, -4.0, Modifiers { control: true, ..Default::default() });
    let (z1, _) = zooms(handle, cx);

    let nudge = z1 - z0;
    assert!(nudge > 0.0, "el nudge de 4px debería acercar un poco ({z0} -> {z1})");

    // Cuánto movería un notch completo desde el mismo zoom inicial.
    let notch = smooth_zoom(z0, 1.0) - z0;
    assert!(
        nudge < notch * 0.2,
        "el nudge ({nudge}) debería ser mucho menor que un notch completo ({notch})"
    );
}

/// Con zoom continuo, `Ctrl` + rueda (rueda clásica de 3 líneas) debe seguir
/// dando exactamente el ×1.25 de antes: no se pierde el feel previo.
#[gpui_kit::gpui::test]
fn classic_wheel_notch_keeps_the_legacy_step(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    let (x, y) = grid_point(handle, cx);
    let (z0, _) = zooms(handle, cx);
    wheel(handle, cx, x, y, -LINES_PER_NOTCH, Modifiers { control: true, ..Default::default() });
    let (z1, _) = zooms(handle, cx);
    let expected = smooth_zoom(z0, 1.0);
    assert!(
        (z1 - expected).abs() < 1e-6,
        "3 líneas deberían dar ×{ZOOM_STEP_FACTOR} exacto ({z1} vs {expected})"
    );
}

#[gpui_kit::gpui::test]
fn plain_wheel_does_not_zoom(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    let (x, y) = grid_point(handle, cx);
    let before = zooms(handle, cx);
    wheel(handle, cx, x, y, -3.0, Modifiers::default());
    let after = zooms(handle, cx);
    assert_eq!(before, after, "la rueda sin modificadores no debería cambiar el zoom");
}

// =========================================================================
// Headers fijos (sticky): la columna izquierda no scrollea en X pero sí en Y
// =========================================================================

/// Punto visible sobre el panel temporal: 100px a la derecha del borde
/// derecho del header de la primera fila, a media altura de esa fila.
fn grid_visible_point(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) -> (f32, f32) {
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        let b = window.find("pl_row_1").bounds();
        (
            (b.origin.x + b.size.width).as_f32() + 100.0,
            (b.origin.y + b.size.height / 2.0).as_f32(),
        )
    })
    .unwrap()
}

/// Rueda SIN modificadores (scroll puro) en un punto visible del panel.
fn scroll_at(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    x: f32,
    y: f32,
    dx: f32,
    dy: f32,
) {
    cx.update_window(handle.into(), |_, window, cx| {
        use gpui_kit::{InputEvent as _, MouseMoveEvent};
        window.dispatch_event(
            MouseMoveEvent {
                position: point(px(x), px(y)),
                pressed_button: None,
                modifiers: Modifiers::default(),
            }
            .to_platform_input(),
            cx,
        );
        window.dispatch_event(
            ScrollWheelEvent {
                position: point(px(x), px(y)),
                delta: ScrollDelta::Pixels(point(px(dx), px(dy))),
                modifiers: Modifiers::default(),
                ..Default::default()
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

/// `(x del header, x de la grilla, y del header, y de la grilla)`.
fn header_and_grid(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
) -> (f32, f32, f32, f32) {
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        let h = window.find("pl_row_1").bounds();
        let g = window.find("playlist_grid").bounds();
        (
            h.origin.x.as_f32(),
            g.origin.x.as_f32(),
            h.origin.y.as_f32(),
            g.origin.y.as_f32(),
        )
    })
    .unwrap()
}

#[gpui_kit::gpui::test]
fn headers_stay_fixed_on_horizontal_scroll(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    let (x, y) = grid_visible_point(handle, cx);
    let (hx0, gx0, _, _) = header_and_grid(handle, cx);
    scroll_at(handle, cx, x, y, -400.0, 0.0);
    let (hx1, gx1, _, _) = header_and_grid(handle, cx);
    assert!(
        (hx1 - hx0).abs() < 1.0,
        "los headers deberían quedar fijos en X ({hx0} -> {hx1})"
    );
    assert!(
        gx1 < gx0 - 200.0,
        "la grilla temporal debería scrollear en X ({gx0} -> {gx1})"
    );
}

#[gpui_kit::gpui::test]
fn headers_scroll_vertically_in_sync(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    // Filas de sobra para que haya recorrido vertical.
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                while s.studio_tracks.iter().filter(|t| !t.is_master).count() < 12 {
                    let next_id = s.studio_tracks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
                    let n = s.studio_tracks.iter().filter(|t| !t.is_master).count();
                    s.studio_tracks.push(Track::new(
                        next_id,
                        format!("TRACK {:02}", n + 1),
                        false,
                    ));
                }
                cx.notify();
            });
        });
    })
    .unwrap();
    let (x, y) = grid_visible_point(handle, cx);
    let (_, _, hy0, gy0) = header_and_grid(handle, cx);
    scroll_at(handle, cx, x, y, 0.0, -150.0);
    let (_, _, hy1, gy1) = header_and_grid(handle, cx);
    let dh = hy1 - hy0;
    let dg = gy1 - gy0;
    assert!(dh < -50.0, "el scroll vertical debería mover los headers ({hy0} -> {hy1})");
    assert!(
        (dh - dg).abs() < 1.0,
        "headers y grilla deberían moverse juntos en Y ({dh} vs {dg})"
    );
}
