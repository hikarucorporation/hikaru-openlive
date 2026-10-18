// crates/hikaru_gui/tests/playlist_hover_guide.rs
//
// Playhead fantasma de la Playlist / Timeline (la guía que sigue al puntero):
// - Empieza apagado: sin `mousemove` sobre la grilla no hay guía.
// - Sigue al puntero: el tick crece con la x y coincide con la conversión
//   `px / zoom` que usa el resto de la vista (seek, snap, drop).
// - Se apaga en cuanto hay un gesto en curso (pan, drag, trim, marquee): con
//   el botón apretado la guía compite con el feedback del gesto.
// - Se recalcula sola si el zoom cambia (guarda el TICK, no la x en píxeles).
//
// El cursor de cruz NO se prueba acá: `Window::cursor_style` es `pub(crate)` en
// GPUI, así que no hay forma de observarlo desde un test de integración. Lo que
// sí cubre este archivo es el estado que la guía depende.

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
use hikaru_gui::views::playlist::{self, PlaylistState};

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

fn open_playlist(cx: &mut TestAppContext) -> gpui_kit::WindowHandle<HikaruApp> {
    cx.update(gpui_kit::init);
    let handle = cx.add_window(test_app);
    cx.simulate_window_resize(handle.into(), size(px(900.0), px(600.0)));
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.mode = AppMode::OpenStudio;
                while s.studio_tracks.len() < 6 {
                    let next_id = s.studio_tracks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
                    let n = s.studio_tracks.iter().filter(|t| !t.is_master).count();
                    s.studio_tracks
                        .push(Track::new(next_id, format!("TRACK {:02}", n + 1), false));
                }
                s.playlist_state = PlaylistState {
                    zoom_x: 0.4,
                    row_h: playlist::TRACK_ROW_H,
                    ..Default::default()
                };
                cx.notify();
            });
        });
    })
    .unwrap();
    handle
}

fn hover_tick(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) -> Option<u64> {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        let mut out = None;
        app.update(cx, |app, cx| {
            out = app.state.read(cx).playlist_state.hover_tick;
        });
        out
    })
    .unwrap()
}

fn zoom_x(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) -> f32 {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        let mut out = 0.0f32;
        app.update(cx, |app, cx| {
            out = app.state.read(cx).playlist_state.zoom_x;
        });
        out
    })
    .unwrap()
}

/// Origen x de la zona temporal.
///
/// Se usa el de `playlist_grid` y NO el de `pl_seek_zone` (que es quien
/// escribe `zone_bounds`): `pl_seek_zone` es hijo `absolute()` de la grilla, y
/// los hijos absolutos de `playlist_grid` no llegan al árbol de observación de
/// `window.find` — la grilla aparece registrada como hoja. Su origen x es el
/// mismo (ambos son left=0 del contenido scrolleable), y el test lo verifica:
/// si la conversión `px / zoom` no cuadra, la aserción de `guide_tick_follows`
/// falla.
fn zone_origin_x(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) -> f32 {
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.find("playlist_grid").bounds().origin.x.as_f32()
    })
    .unwrap()
}

/// Punto visible dentro de la grilla, a media altura de la ventana.
/// Importante que el puntero NO se salga del viewport: fuera de él no hay hit
/// test y ningún handler corre.
fn grid_point(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) -> (f32, f32) {
    let zx = zone_origin_x(handle, cx);
    (zx + 200.0, 300.0)
}

/// `mousemove` SIN botón apretado (el estado normal de "el puntero está acá").
fn hover(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext, x: f32, y: f32) {
    cx.update_window(handle.into(), |_, window, cx| {
        use gpui_kit::InputEvent as _;
        window.dispatch_event(
            MouseMoveEvent {
                position: point(px(x), px(y)),
                pressed_button: None,
                modifiers: Default::default(),
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

fn mdown(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    x: f32,
    y: f32,
    button: MouseButton,
) {
    cx.update_window(handle.into(), |_, window, cx| {
        use gpui_kit::InputEvent as _;
        window.dispatch_event(
            MouseDownEvent {
                button,
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

fn mmove_held(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    x: f32,
    y: f32,
) {
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

fn mup(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    x: f32,
    y: f32,
    button: MouseButton,
) {
    cx.update_window(handle.into(), |_, window, cx| {
        use gpui_kit::InputEvent as _;
        window.dispatch_event(
            MouseUpEvent {
                button,
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

// =========================================================================
// Gesto real
// =========================================================================

/// Sin `mousemove` no hay guía: no se dibuja una línea fantasma clavada en el
/// tick 0 que no significaría nada.
#[gpui_kit::gpui::test]
fn guide_is_off_before_any_hover(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    assert_eq!(
        hover_tick(handle, cx),
        None,
        "la guía debería arrancar apagada"
    );
}

/// Un `mousemove` sobre la grilla enciende la guía.
#[gpui_kit::gpui::test]
fn hover_over_grid_turns_the_guide_on(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    let (x, y) = grid_point(handle, cx);
    hover(handle, cx, x, y);
    let t = hover_tick(handle, cx);
    println!("tick bajo el puntero: {t:?}");
    assert!(
        t.is_some(),
        "un mousemove sobre la grilla debería encender la guía"
    );
}

/// El tick sigue a la x: monótono y coherente con `px / zoom`.
#[gpui_kit::gpui::test]
fn guide_tick_follows_the_cursor(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    let (x, y) = grid_point(handle, cx);
    let zx = zone_origin_x(handle, cx);
    let z = zoom_x(handle, cx);

    hover(handle, cx, x, y);
    let t0 = hover_tick(handle, cx).expect("guía encendida");

    // 100px a la derecha, con zoom 0.4 px/tick, son 250 ticks más.
    hover(handle, cx, x + 100.0, y);
    let t1 = hover_tick(handle, cx).expect("guía encendida");
    println!("x {x:.1} -> x+100: tick {t0} -> {t1} (zoom {z})");
    assert!(t1 > t0, "mover el puntero a la derecha debe avanzar el tick");

    let expected = ((x + 100.0 - zx) / z) as u64;
    assert!(
        t1.abs_diff(expected) <= 1,
        "el tick debería ser px/zoom ({t1} vs {expected})"
    );

    // Y a la izquierda vuelve atrás.
    hover(handle, cx, x, y);
    let t2 = hover_tick(handle, cx).expect("guía encendida");
    assert!(t2 < t1, "volver a la izquierda debe retroceder el tick");
    assert!(
        t2.abs_diff(t0) <= 1,
        "volver al punto de partida debería recuperar el tick ({t0} vs {t2})"
    );
}

/// Al empezar a navegar (botón central) la guía se apaga: con el botón
/// apretado compite con el feedback del gesto.
#[gpui_kit::gpui::test]
fn guide_turns_off_while_navigating(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    let (x, y) = grid_point(handle, cx);
    hover(handle, cx, x, y);
    assert!(hover_tick(handle, cx).is_some(), "precondición: guía encendida");

    mdown(handle, cx, x, y, MouseButton::Middle);
    mmove_held(handle, cx, x + 60.0, y - 40.0);
    let during = hover_tick(handle, cx);
    println!("tick durante el pan: {during:?}");
    assert_eq!(
        during, None,
        "la guía debe apagarse mientras se navega con el botón central"
    );

    // Al soltar y volver a mover el puntero, vuelve.
    mup(handle, cx, x + 60.0, y - 40.0, MouseButton::Middle);
    hover(handle, cx, x + 60.0, y - 40.0);
    assert!(
        hover_tick(handle, cx).is_some(),
        "al soltar el botón la guía debe volver"
    );
}

/// La guía guarda el TICK, no la x: si cambia el zoom bajo el puntero, el
/// tick reportado sigue siendo el que estaba bajo el cursor.
#[gpui_kit::gpui::test]
fn guide_survives_a_zoom_change(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    let (x, y) = grid_point(handle, cx);
    hover(handle, cx, x, y);
    let before = hover_tick(handle, cx).expect("guía encendida");

    // Cambiar el zoom no borra el estado del hover (no llega ningún evento).
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("raíz");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.playlist_state.zoom_x = 0.8;
                cx.notify();
            });
        });
    })
    .unwrap();

    assert_eq!(
        hover_tick(handle, cx),
        Some(before),
        "cambiar el zoom no debería tocar el tick guardado"
    );
    assert_eq!(zoom_x(handle, cx), 0.8, "el zoom cambió de verdad");
}

/// Un click simple no apaga la guía: el puntero sigue ahí, así que la guía
/// debe seguir mostrándose.
#[gpui_kit::gpui::test]
fn guide_stays_on_after_a_simple_click(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    let (x, y) = grid_point(handle, cx);
    hover(handle, cx, x, y);
    let before = hover_tick(handle, cx).expect("guía encendida");

    mdown(handle, cx, x, y, MouseButton::Left);
    mup(handle, cx, x, y, MouseButton::Left);

    assert_eq!(
        hover_tick(handle, cx),
        Some(before),
        "un click simple no debería apagar la guía (el puntero sigue encima)"
    );
}
