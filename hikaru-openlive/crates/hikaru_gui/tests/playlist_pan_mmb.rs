// crates/hikaru_gui/tests/playlist_pan_mmb.rs
//
// Navegación de la Playlist con botón central (ruedita + arrastrar), al estilo
// Bitwig: los dos ejes hacen cosas DISTINTAS.
// - Eje X → pan temporal: el viewport sigue al cursor, con offsets acotados al
//   recorrido real del contenido (`[-max, 0]`), sin "pared invisible" ni zonas
//   muertas al invertir en un borde.
// - Eje Y → zoom continuo de la vista temporal, anclado al cursor: el punto que
//   estaba bajo el puntero queda bajo el puntero. NO hace pan vertical.
// - El cursor pasa a puño cerrado mientras dura el gesto y vuelve al soltarlo.
// - El gesto continúa aunque el cursor salga del panel.

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
use hikaru_gui::views::playlist::{
    self, clamp_scroll_offset, mmb_drag_notches, smooth_zoom, MAX_ZOOM_X, MIN_ZOOM_X, PlaylistState,
    MMB_DRAG_PX_PER_NOTCH, PAN_SENSITIVITY,
};

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

/// Playlist con zoom alto para que el contenido desborde en ambos ejes y haya
/// recorrido real (si no, el clamp a 0 hiding el bug).
fn open_playlist(cx: &mut TestAppContext) -> gpui_kit::WindowHandle<HikaruApp> {
    cx.update(gpui_kit::init);
    let handle = cx.add_window(|window, cx| test_app(window, cx));
    cx.simulate_window_resize(handle.into(), size(px(900.0), px(600.0)));
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.mode = AppMode::OpenStudio;
                // Muchas pistas + zoom máximo ⇒ desborde vertical y horizontal.
                while s.studio_tracks.len() < 8 {
                    let next_id = s.studio_tracks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
                    let n = s.studio_tracks.iter().filter(|t| !t.is_master).count();
                    s.studio_tracks
                        .push(Track::new(next_id, format!("TRACK {:02}", n + 1), false));
                }
                // Zoom medio: suficiente para desbordar los 900px de ancho
                // sin crear un canvas de cientos de miles de px (que hundía el
                // layout y dejaba el contenido fuera de la ventana).
                s.playlist_state = PlaylistState {
                    zoom_x: 0.4,
                    row_h: playlist::MAX_ROW_H,
                    ..Default::default()
                };
                cx.notify();
            });
        });
    })
    .unwrap();
    handle
}

/// `(offset_x, max_x, offset_y, max_y)` de los handles de scroll.
fn scroll_state(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) -> (f32, f32, f32, f32) {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        let mut out = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        app.update(cx, |app, cx| {
            let s = app.state.read(cx);
            let h = &s.playlist_scroll_h;
            let v = &s.playlist_scroll_v;
            out = (
                h.offset().x.as_f32(),
                h.max_offset().x.as_f32(),
                v.offset().y.as_f32(),
                v.max_offset().y.as_f32(),
            );
        });
        out
    })
    .unwrap()
}

/// `zoom_x` actual de la playlist.
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

fn is_panning(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) -> bool {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        let mut out = false;
        app.update(cx, |app, cx| {
            out = app.state.read(cx).playlist_pan.is_some();
        });
        out
    })
    .unwrap()
}

/// Punto VISIBLE dentro del panel temporal (viewport), no del centro del
/// canvas de contenido: el canvas scrolleado puede medir cientos de miles de
/// px y su centro cae muy lejos de la ventana.
fn grid_center(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) -> (f32, f32) {
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        let b = window.find("playlist_grid").bounds();
        // Se recorta al viewport real (el canvas de contenido es más ancho).
        let w = b.size.width.as_f32().min(600.0);
        let h = b.size.height.as_f32().min(300.0);
        (
            (b.origin.x + px(w / 2.0)).as_f32(),
            (b.origin.y + px(h / 2.0)).as_f32(),
        )
    })
    .unwrap()
}

/// Origen del gesto: punto visible dentro de la grilla **con espacio para
/// arrastrar en los dos sentidos**.
///
/// Importa más de lo que parece: si el puntero sale de la ventana no hay hit
/// test, ningún handler corre y el gesto se pierde en silencio. `grid_center`
/// devuelve un `cy0` muy arriba (la grilla arranca justo debajo del header),
/// así que arrastrar hacia arriba por más de ~120px se salía del viewport y
/// el zoom nunca llegaba a aplicarse.
fn drag_origin(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) -> (f32, f32) {
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        let g = window.find("playlist_grid").bounds();
        let x = (g.origin.x + px(200.0)).as_f32();
        // La ventana se redimensiona a 900x600 en `open_playlist`. El centro
        // vertical deja ~300px de recorrido en cada sentido. NO sirve tomar el
        // centro de la grilla: con 8 pistas a `MAX_ROW_H` la grilla mide miles
        // de px y su centro cae muy por debajo de la ventana, donde no hay hit
        // test y el gesto se pierde en silencio.
        let y = 300.0f32.max(g.origin.y.as_f32() + 40.0);
        (x, y)
    })
    .unwrap()
}

/// Fija `zoom_x` directamente, para arrancar un test cerca de un límite sin
/// depender de arrastres largos (que no entran en la ventana).
fn set_zoom(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext, z: f32) {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.playlist_state.zoom_x = z;
                cx.notify();
            });
        });
    })
    .unwrap();
}

fn mdown(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    x: f32,
    y: f32,
) {
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

fn mmove(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    x: f32,
    y: f32,
    pressed: bool,
) {
    cx.update_window(handle.into(), |_, window, cx| {
        use gpui_kit::InputEvent as _;
        window.dispatch_event(
            MouseMoveEvent {
                position: point(px(x), px(y)),
                pressed_button: if pressed { Some(MouseButton::Middle) } else { None },
                modifiers: Default::default(),
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

fn mup(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext, x: f32, y: f32) {
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

// =========================================================================
// Clamp (puro)
// =========================================================================

#[test]
fn scroll_clamp_bounds_to_real_range() {
    // Con max 500 el rango válido es [-500, 0].
    assert_eq!(clamp_scroll_offset(-100.0, 500.0), -100.0);
    assert_eq!(clamp_scroll_offset(50.0, 500.0), 0.0, "nunca positivo");
    assert_eq!(clamp_scroll_offset(-900.0, 500.0), -500.0, "acota al máximo");
    // Sin desborde (max 0) el offset queda clavado en 0.
    assert_eq!(clamp_scroll_offset(-300.0, 0.0), 0.0);
    assert_eq!(clamp_scroll_offset(120.0, 0.0), 0.0);
}

// =========================================================================
// Gesto real
// =========================================================================

/// El contenido tiene que desbordar: si no, el clamp a 0 oculta el bug.
#[gpui_kit::gpui::test]
fn playlist_has_real_scroll_range(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    let (_, max_x, _, max_y) = scroll_state(handle, cx);
    println!("recorrido: x máx {max_x:.1}, y máx {max_y:.1}");
    assert!(max_x > 50.0, "debería haber recorrido horizontal, hay {max_x}");
    assert!(max_y > 50.0, "debería haber recorrido vertical, hay {max_y}");
}

#[gpui_kit::gpui::test]
fn mmb_drag_scrolls_horizontally(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    let (cx0, cy0) = grid_center(handle, cx);
    let (x0, ..) = scroll_state(handle, cx);
    mdown(handle, cx, cx0, cy0);
    // El viewport SIGUE al cursor: arrastrar a la derecha avanza en el tiempo,
    // que con offsets negativos es quedar más negativo.
    mmove(handle, cx, cx0 + 120.0, cy0, true);
    mup(handle, cx, cx0 + 120.0, cy0);
    let (x1, ..) = scroll_state(handle, cx);
    println!("offset_x {x0:.1} -> {x1:.1}");
    assert!(
        x1 < x0 - 50.0,
        "arrastrar a la derecha debería avanzar el viewport ({x0} -> {x1})"
    );
}

/// El eje Y del gesto ya NO hace pan vertical: hace zoom del tiempo.
/// Este test es el que documenta ese cambio de semántica.
#[gpui_kit::gpui::test]
fn mmb_drag_vertically_zooms_instead_of_panning(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    let (cx0, cy0) = drag_origin(handle, cx);
    let (_, _, y0, _) = scroll_state(handle, cx);
    let z0 = zoom_x(handle, cx);

    mdown(handle, cx, cx0, cy0);
    // Arrastrar 180px hacia arriba = 1.5 notches de zoom in.
    mmove(handle, cx, cx0, cy0 - 180.0, true);
    mup(handle, cx, cx0, cy0 - 180.0);

    let (_, _, y1, _) = scroll_state(handle, cx);
    let z1 = zoom_x(handle, cx);
    println!("offset_y {y0:.1} -> {y1:.1}; zoom_x {z0:.4} -> {z1:.4}");
    assert!(
        (z1 - z0).abs() > 1e-6,
        "arrastrar hacia arriba debería hacer zoom ({z0} -> {z1})"
    );
    assert_eq!(
        y1, y0,
        "el eje Y no debe hacer pan vertical ({y0} -> {y1})"
    );
}

/// Arrastrar hacia arriba acerca, hacia abajo aleja.
#[gpui_kit::gpui::test]
fn mmb_drag_up_zooms_in_and_down_zooms_out(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    let (cx0, cy0) = drag_origin(handle, cx);

    let z0 = zoom_x(handle, cx);
    mdown(handle, cx, cx0, cy0);
    mmove(handle, cx, cx0, cy0 - 150.0, true);
    mup(handle, cx, cx0, cy0 - 150.0);
    let z_up = zoom_x(handle, cx);
    assert!(z_up > z0, "arrastrar arriba debería acercar ({z0} -> {z_up})");

    // Ahora al revés, desde el zoom ya ampliado.
    mdown(handle, cx, cx0, cy0);
    mmove(handle, cx, cx0, cy0 + 150.0, true);
    mup(handle, cx, cx0, cy0 + 150.0);
    let z_down = zoom_x(handle, cx);
    assert!(
        z_down < z_up,
        "arrastrar abajo debería alejar ({z_up} -> {z_down})"
    );
}

/// El pan de X no debe tocar el zoom: son ejes independientes.
#[gpui_kit::gpui::test]
fn mmb_horizontal_drag_does_not_zoom(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    let (cx0, cy0) = drag_origin(handle, cx);
    let z0 = zoom_x(handle, cx);
    mdown(handle, cx, cx0, cy0);
    mmove(handle, cx, cx0 + 250.0, cy0, true);
    mup(handle, cx, cx0 + 250.0, cy0);
    assert_eq!(
        zoom_x(handle, cx),
        z0,
        "arrastrar sólo en X no debería cambiar el zoom"
    );
}

/// El zoom del gesto es CONTINUO: la magnitud del arrastre vertical se
/// respeta. 240px arriba deben dar exactamente 2 notches (`×1.25²`).
#[gpui_kit::gpui::test]
fn mmb_drag_zoom_is_continuous_and_matches_drag_distance(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    let (cx0, cy0) = drag_origin(handle, cx);
    let z0 = zoom_x(handle, cx);

    // 180px hacia abajo = 1.5 notches de zoom out (abajo hay room en ventana).
    mdown(handle, cx, cx0, cy0);
    mmove(handle, cx, cx0, cy0 + 180.0, true);
    mup(handle, cx, cx0, cy0 + 180.0);

    let z1 = zoom_x(handle, cx);
    let expected = smooth_zoom(z0, mmb_drag_notches(180.0));
    println!("zoom {z0:.5} -> {z1:.5}, esperado {expected:.5}");
    assert!(
        (z1 - expected).abs() < 1e-5,
        "180px abajo deberían ser 1.5 notches exactos ({z1} vs {expected})"
    );

    // Dos arrastres de 90px componen un arrastre de 180px (continuidad real,
    // sin escalones): el zoom es una función de la distancia, no del evento.
    set_zoom(handle, cx, z0);
    mdown(handle, cx, cx0, cy0);
    mmove(handle, cx, cx0, cy0 + 90.0, true);
    mmove(handle, cx, cx0, cy0 + 180.0, true);
    mup(handle, cx, cx0, cy0 + 180.0);
    assert!(
        (zoom_x(handle, cx) - expected).abs() < 1e-5,
        "dos pasos de 90px deberían equivaler a uno de 180px"
    );
}

/// El zoom del gesto queda acotado en `MAX_ZOOM_X` / `MIN_ZOOM_X`.
#[gpui_kit::gpui::test]
fn mmb_drag_zoom_clamps_to_limits(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    let (cx0, cy0) = drag_origin(handle, cx);

    // Cerca del tope: 150px arriba son 1.25 notches, que empuja más allá.
    set_zoom(handle, cx, MAX_ZOOM_X * 0.8);
    mdown(handle, cx, cx0, cy0);
    mmove(handle, cx, cx0, cy0 - 150.0, true);
    mup(handle, cx, cx0, cy0 - 150.0);
    let z_max = zoom_x(handle, cx);
    assert!(
        (z_max - MAX_ZOOM_X).abs() < 1e-4,
        "el zoom in debería clampear en {MAX_ZOOM_X}, quedó en {z_max}"
    );

    // Y lo mismo por el otro extremo. Desde `MIN * 1.5` los 1.5 notches del
    // arrastre no alcanzan el piso (0.00525 / 1.25^1.5 ≈ 0.0054), así que se
    // parte más cerca del límite: 1.2 notches de margen alcanza y sobra.
    set_zoom(handle, cx, MIN_ZOOM_X * 1.2);
    mdown(handle, cx, cx0, cy0);
    mmove(handle, cx, cx0, cy0 + 180.0, true);
    mup(handle, cx, cx0, cy0 + 180.0);
    let z_min = zoom_x(handle, cx);
    assert!(
        (z_min - MIN_ZOOM_X).abs() < 1e-6,
        "el zoom out debería clampear en {MIN_ZOOM_X}, quedó en {z_min}"
    );
}

/// El zoom del gesto queda ANCLADO al cursor: tras ampliar, el tick que estaba
/// bajo el puntero sigue bajo el puntero.
#[gpui_kit::gpui::test]
fn mmb_drag_zoom_stays_anchored_to_cursor(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    let (cx0, cy0) = drag_origin(handle, cx);
    /// Tick bajo el cursor. Usa EXACTAMENTE la fórmula de `anchor_h_offset`
    /// (`tick = (cursor_vx - offset - header_w) / zoom`, con `cursor_vx`
    /// relativo al origen del handle), porque el invariante que se quiere
    /// verificar es precisamente que ese tick no cambie. Omitir `header_w`
    /// haría que la comprobación fuera falsa por construcción: el error
    /// constante se divide por un zoom que está cambiando.
    fn tick_under(
        handle: gpui_kit::WindowHandle<HikaruApp>,
        cx: &mut TestAppContext,
        cursor_x: f32,
    ) -> f32 {
        let (origin_x, off, z, header_w) = cx
            .update_window(handle.into(), |view, _, cx| {
                let app = view.downcast::<HikaruApp>().expect("raíz");
                let mut o = (0.0f32, 0.0f32, 1.0f32, 0.0f32);
                app.update(cx, |app, cx| {
                    let s = app.state.read(cx);
                    o = (
                        s.playlist_scroll_h.bounds().origin.x.as_f32(),
                        s.playlist_scroll_h.offset().x.as_f32(),
                        s.playlist_state.zoom_x,
                        s.playlist_state.header_width,
                    );
                });
                o
            })
            .unwrap();
        (cursor_x - origin_x - header_w - off) / z
    }

    let before = tick_under(handle, cx, cx0);
    mdown(handle, cx, cx0, cy0);
    mmove(handle, cx, cx0, cy0 - 150.0, true);
    mup(handle, cx, cx0, cy0 - 150.0);
    let after = tick_under(handle, cx, cx0);
    println!("tick bajo el cursor: {before:.2} -> {after:.2}");
    assert!(
        (before - after).abs() < 1.0,
        "el tick bajo el cursor debería quedar fijo ({before} -> {after})"
    );
}

/// Sentido inverso: arrastrar a la derecha/abajo avanza el viewport.
#[gpui_kit::gpui::test]
fn mmb_drag_backwards_advances_viewport(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    let (cx0, cy0) = grid_center(handle, cx);
    // Primero avanza para dejar recorrido en el sentido contrario.
    mdown(handle, cx, cx0, cy0);
    mmove(handle, cx, cx0 + 200.0, cy0, true);
    mup(handle, cx, cx0 + 200.0, cy0);
    let (x0, ..) = scroll_state(handle, cx);
    // Ahora arrastra a la izquierda: debe acercarse a 0 (volver al inicio).
    mdown(handle, cx, cx0 + 200.0, cy0);
    mmove(handle, cx, cx0 + 100.0, cy0, true);
    mup(handle, cx, cx0 + 100.0, cy0);
    let (x1, ..) = scroll_state(handle, cx);
    println!("offset_x {x0:.1} -> {x1:.1} (volviendo a la izquierda)");
    assert!(
        x1 > x0 + 50.0,
        "arrastrar a la izquierda debe retroceder el viewport ({x0} -> {x1})"
    );
}

/// El offset no puede pasar del máximo ni volverse positivo (el "no hay áreas
/// negativas" del spec, que en este codebase es `[-max, 0]`).
#[gpui_kit::gpui::test]
fn mmb_drag_clamps_to_scroll_bounds(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    let (cx0, cy0) = grid_center(handle, cx);
    // Arrastre enorme a la derecha: no debe pasar de 0.
    mdown(handle, cx, cx0, cy0);
    mmove(handle, cx, cx0 + 4000.0, cy0 + 4000.0, true);
    mup(handle, cx, cx0 + 4000.0, cy0 + 4000.0);
    let (x1, max_x, y1, max_y) = scroll_state(handle, cx);
    println!("tras arrastre enorme: x={x1:.1} (máx {max_x:.1}), y={y1:.1} (máx {max_y:.1})");
    assert!(x1 <= 0.5, "el offset_x no puede ser positivo: {x1}");
    // `y1` no se mueve: el arrastre vertical de 4000px es ahora zoom, no pan.
    assert_eq!(y1, 0.0, "el arrastre vertical no debe mover el offset_y: {y1}");
    // Y arrastra al revés del tope: debe clampear en -max, sin pasarse.
    mdown(handle, cx, cx0 + 4000.0, cy0);
    mmove(handle, cx, cx0 - 4000.0, cy0, true);
    mup(handle, cx, cx0 - 4000.0, cy0);
    let (x2, max_x2, ..) = scroll_state(handle, cx);
    println!("tras arrastre al revés: x={x2:.1} (máx {max_x2:.1})");
    assert!(
        x2 >= -max_x2 - 0.5,
        "no debe pasar de -max: {x2} vs -({max_x2})"
    );
}

/// Sin zona muerta: invertir el sentido en un borde responde de inmediato.
/// Un esquema absoluto acumularía el delta crudo y tardaría en volver.
#[gpui_kit::gpui::test]
fn mmb_reverse_direction_responds_at_edge(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    let (cx0, cy0) = grid_center(handle, cx);
    mdown(handle, cx, cx0, cy0);
    // Pasos de 40px (mantiéndose dentro de la ventana de 900px).
    for i in 1..=6 {
        mmove(handle, cx, cx0 + i as f32 * 40.0, cy0, true);
    }
    let x_at_edge = scroll_state(handle, cx).0;
    // Invierto un poco, siempre pegado al tope negativo.
    mmove(handle, cx, cx0 + 120.0, cy0, true);
    let x_after = scroll_state(handle, cx).0;
    mup(handle, cx, cx0 + 120.0, cy0);
    println!("en el tope {x_at_edge:.1} -> al invertir {x_after:.1}");
    assert!(
        x_after > x_at_edge + 50.0,
        "invertir en el borde debe responder al instante ({x_at_edge} -> {x_after})"
    );
}

/// El gesto sigue vivo aunque el cursor pase por encima de un CLIP: el
/// catcher global está por encima y el clip (que también tiene handlers de
/// mouse) no puede interceptar el pan.
///
/// Se mantiene dentro de la ventana a propósito: fuera de ella no hay hit test
/// y ningún handler corre, que es un caso distinto.
#[gpui_kit::gpui::test]
fn mmb_gesture_survives_cursor_over_a_clip(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    // Con un clip en la fila 1, su bbox es un punto de cruce real del gesto.
    let _clip_insert = cx
        .update_window(handle.into(), |view, _, cx| {
            let app = view.downcast::<HikaruApp>().expect("raíz");
            app.update(cx, |app, cx| {
                app.state.update(cx, |s, cx| {
                    let ppqn = s.playlist_state.ppqn.max(1);
                    s.playlist_state.zoom_x = 0.04;
                    s.playlist_state.clips.push((1, hikaru_gui::views::playlist::PlaylistClip {
                        id: 1,
                        name: "C".into(),
                        start_tick: ppqn,
                        duration_ticks: ppqn * 8,
                        clip_type: hikaru_gui::views::playlist::ClipType::Audio {
                            sample_path: "c.wav".into(),
                            peaks: vec![0.5; 32],
                            sample_offset_ticks: 0,
                            total_sample_ticks: ppqn * 8,
                        },
                        color: gpui_kit::rgb(0x205F91).into(),
                    }));
                    cx.notify();
                });
            });
        })
        .unwrap();
    let _ = grid_center(handle, cx);
    let clip_center = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            let b = window.find("pl_clip_1").bounds();
            ((b.origin.x + b.size.width / 2.0).as_f32(), (b.origin.y + b.size.height / 2.0).as_f32())
        })
        .unwrap();
    // Arranca SOBRE el clip y sigue hacia la derecha (dirección que sí avanza):
    // si el clip interceptara el gesto, el offset no se movería.
    mdown(handle, cx, clip_center.0, clip_center.1);
    let before = scroll_state(handle, cx).0;
    mmove(handle, cx, clip_center.0 + 120.0, clip_center.1, true);
    let over_clip = scroll_state(handle, cx).0;
    mup(handle, cx, clip_center.0, clip_center.1);
    println!("offset_x: {before:.1} -> {over_clip:.1} cruzando el clip");
    assert!(
        over_clip < before - 10.0,
        "el pan debe continuar por encima del clip ({before} -> {over_clip})"
    );
}

/// Soltar el botón cierra el gesto (y restaura el cursor).
#[gpui_kit::gpui::test]
fn mmb_release_ends_pan(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    let (cx0, cy0) = grid_center(handle, cx);
    mdown(handle, cx, cx0, cy0);
    assert!(is_panning(handle, cx), "el pan debe arrancar con MMB");
    mmove(handle, cx, cx0 + 80.0, cy0, true);
    mup(handle, cx, cx0 + 80.0, cy0);
    assert!(!is_panning(handle, cx), "soltar debe cerrar el pan");
    let after_release = scroll_state(handle, cx).0;
    // Un move con el botón suelto ya no debe mover nada.
    mmove(handle, cx, cx0 + 400.0, cy0, false);
    assert_eq!(
        scroll_state(handle, cx).0,
        after_release,
        "sin botón presionado no debe haber desplazamiento"
    );
}

/// Un move sin botón (el `mouse_up` se perdió fuera de la ventana) también
/// cierra el gesto en vez de dejarlo colgado.
#[gpui_kit::gpui::test]
fn mmb_lost_release_closes_pan(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    let (cx0, cy0) = grid_center(handle, cx);
    mdown(handle, cx, cx0, cy0);
    mmove(handle, cx, cx0 + 50.0, cy0, true);
    assert!(is_panning(handle, cx));
    mmove(handle, cx, cx0 + 100.0, cy0, false);
    assert!(
        !is_panning(handle, cx),
        "un move sin botón debe cerrar el gesto colgado"
    );
}


/// La sensibilidad es 1:1 (px de mouse = px de viewport).
#[test]
fn pan_sensitivity_is_one_to_one() {
    assert_eq!(PAN_SENSITIVITY, 1.0);
}

// =========================================================================
// Notches de zoom del gesto (puro)
// =========================================================================

#[test]
fn drag_up_is_zoom_in_and_down_is_zoom_out() {
    // Arrastrar arriba (dy negativo) acerca: signo de notches positivo.
    assert!(mmb_drag_notches(-MMB_DRAG_PX_PER_NOTCH) > 0.0);
    assert!(mmb_drag_notches(MMB_DRAG_PX_PER_NOTCH) < 0.0);
    assert_eq!(mmb_drag_notches(0.0), 0.0);
}

#[test]
fn drag_notches_scale_linearly_with_distance() {
    // El gesto es continuo: 240px arriba son exactamente 2 notches, y la
    // mitad de la distancia da la mitad de notches.
    assert!((mmb_drag_notches(-240.0) - 2.0).abs() < 1e-6);
    assert!((mmb_drag_notches(-60.0) - 0.5).abs() < 1e-6);
    assert!((mmb_drag_notches(-30.0) - 0.25).abs() < 1e-6);
}

#[test]
fn drag_notches_is_safe_on_degenerate_input() {
    assert_eq!(mmb_drag_notches(f32::NAN), 0.0);
    assert_eq!(mmb_drag_notches(f32::INFINITY), 0.0);
    assert_eq!(mmb_drag_notches(f32::NEG_INFINITY), 0.0);
    const { assert!(MMB_DRAG_PX_PER_NOTCH > 0.0) };
}

#[test]
fn drag_notches_composes_into_the_expected_zoom() {
    // La magnitud del drag entra por el mismo `smooth_zoom` que la rueda, así
    // que 4 notches de drag dan exactamente `×1.25⁴`.
    let z = smooth_zoom(0.1, mmb_drag_notches(-4.0 * MMB_DRAG_PX_PER_NOTCH));
    assert!((z - 0.1 * 1.25f32.powi(4)).abs() < 1e-5, "0.1 × 1.25⁴ = {z}");
}