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
use hikaru_gui::views::playlist::{
    anchor_h_offset, anchor_v_offset, clamped_row_h, step_row_h, wheel_zoom_in, MAX_ROW_H,
    MAX_ZOOM_X, MIN_ROW_H, MIN_ZOOM_X, TRACK_ROW_H,
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

#[gpui_kit::gpui::test]
fn plain_wheel_does_not_zoom(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    let (x, y) = grid_point(handle, cx);
    let before = zooms(handle, cx);
    wheel(handle, cx, x, y, -3.0, Modifiers::default());
    let after = zooms(handle, cx);
    assert_eq!(before, after, "la rueda sin modificadores no debería cambiar el zoom");
}
