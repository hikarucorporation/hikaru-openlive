// crates/hikaru_gui/tests/piano_roll_view.rs
//
// Regresión del Piano Roll: rendimiento y encuadre.
//
// El bug era doble y los dos teléfonos eran la misma forma:
//
// 1) CONGELAMIENTO. El loop de filas creaba UN canvas por fila MIDI (128) y
//    cada uno llevaba `.w(content_w).h(grid_h)`: 128 capas de ~73.000 x 2.048 px
//    superpuestas, con layout y `paint_path` propios, en cada frame (y la
//    reproducción dispara un `cx.notify()` por frame). Además se instanciaban
//    las 128 teclas del sidebar (con `format!` de label e id) y TODAS las notas
//    sin recortar contra el viewport.
//
// 2) ENCUADRE. El scroller era sólo horizontal, así que las 128 filas no se
//    podían scrollear: la vista quedaba clavada arriba (G9) con el resto del
//    área vacío. Y el zoom por defecto (0.15) hacía que un compás midiera 576 px.
//
// Lo que se verifica acá:
//  - la geometría pura recorta filas/ticks a un rango acotado (virtualización),
//  - el contenido tiene alto suficiente para que exista scroll vertical,
//  - la vista abre enfocada en C4 (no en G9),
//  - las teclas fuera del viewport NO se instancian, y aparecen al scrollear.

use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Arc;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{ScrollDelta, point, px, size, AppContext, Context, TestAppContext, Window};

use hikaru_gui::app::HikaruApp;
use hikaru_gui::audio_proxy::{AudioProxy, GuiCommand};
use hikaru_gui::views::piano_roll::{
    self, compute_grid_viewport, fit_zoom_x, focus_scroll_y, note_visible_in, total_ticks_for,
    DEFAULT_KEY_HEIGHT, FOCUS_PITCH, MIDI_KEY_COUNT, RULER_HEIGHT, TICKS_PER_BAR,
};

// =========================================================================
// GEOMETRÍA PURA (sin ventana)
// =========================================================================

/// Filas por compás invertidas: la fila 0 es el pitch más alto (G9).
fn row_of(pitch: u8) -> u32 {
    MIDI_KEY_COUNT - 1 - u32::from(pitch)
}

#[test]
fn viewport_recorta_las_filas_a_un_rango_acotado() {
    // Panel de 1280 x 192 con el scroll en 0 (G9 arriba).
    let vp = compute_grid_viewport(
        point(px(0.0), px(0.0)),
        size(px(1280.0), px(192.0)),
        DEFAULT_KEY_HEIGHT,
        0.06,
    );

    assert_eq!(vp.row_start, 0, "en scroll 0 arranca en G9");
    assert!(vp.row_end <= MIDI_KEY_COUNT);
    // Con ~192 px de alto y 16 px por fila entran ~12 filas: el viewport tiene
    // que recortar. Si `row_end` fuera 128 estaríamos instanciando las 128
    // teclas (y los 128 canvas) otra vez.
    assert!(
        vp.row_end <= 32,
        "se recortaron {} filas de 128: la virtualización no está recortando",
        vp.row_end - vp.row_start
    );
}

#[test]
fn viewport_con_scroll_al_medio_trae_c4_y_no_g9() {
    // Enfocar C4 es el layout inicial: el borde superior del contenido queda
    // arriba de la fila de C4.
    let top = focus_scroll_y(192.0, DEFAULT_KEY_HEIGHT, FOCUS_PITCH);
    let vp = compute_grid_viewport(
        point(px(0.0), px(-top)),
        size(px(1280.0), px(192.0)),
        DEFAULT_KEY_HEIGHT,
        0.06,
    );

    assert!(
        vp.row_start <= row_of(FOCUS_PITCH) && row_of(FOCUS_PITCH) < vp.row_end,
        "C4 (fila {}) debería estar visible con filas [{}, {})",
        row_of(FOCUS_PITCH),
        vp.row_start,
        vp.row_end
    );
    assert!(
        vp.row_start > 0,
        "C4 es la fila {}, así que con el foco no debería verse G9 (fila 0)",
        row_of(FOCUS_PITCH)
    );
}

#[test]
fn el_zoom_inicial_mete_compases_en_el_panel() {
    let zoom = fit_zoom_x(1280.0, 4.0);
    let bar_w = TICKS_PER_BAR as f32 * zoom;
    // 4 compases tienen que entrar en el ancho útil del panel.
    assert!(
        bar_w * 4.0 <= 1280.0 - piano_roll::SIDEBAR_WIDTH + 1.0,
        "4 compases miden {} px y no entran en el panel",
        bar_w * 4.0
    );
    // Y un compás no puede ser una franja de 576 px como con el zoom anterior.
    assert!(
        bar_w < 300.0,
        "un compás mide {bar_w} px: el zoom sigue demasiado alejado"
    );
}

#[test]
fn el_contenido_no_mide_128_compases() {
    // Sin notas el piso son DEFAULT_BARS compases, no 128.
    let total = total_ticks_for(0, 0);
    assert_eq!(total, TICKS_PER_BAR * piano_roll::DEFAULT_BARS);
    // Con una nota lejana, el contenido crece hasta la nota + 2 compases.
    let far = TICKS_PER_BAR * 100;
    assert_eq!(total_ticks_for(far + 480, 0), far + 480 + TICKS_PER_BAR * 2);
}

#[test]
fn el_filtro_de_notas_usa_la_geometria_del_viewport() {
    let mut vp = compute_grid_viewport(
        point(px(0.0), px(0.0)),
        size(px(1280.0), px(192.0)),
        DEFAULT_KEY_HEIGHT,
        0.06,
    );
    // Restringimos a un rango conocido para que el assert sea sobre el filtro y
    // no sobre el redondeo del viewport.
    vp.row_start = 60;
    vp.row_end = 68;
    vp.tick_start = 0;
    vp.tick_end = TICKS_PER_BAR * 2;

    // pitch 60 -> fila 67, dentro del rango.
    assert!(note_visible_in(0, 480, 60, &vp));
    // pitch 127 -> fila 0, fuera.
    assert!(!note_visible_in(0, 480, 127, &vp));
    // pitch 59 -> fila 68, fuera (límite superior exclusivo).
    assert!(!note_visible_in(0, 480, 59, &vp));
    // En el rango de filas pero muy a la derecha en el tiempo.
    assert!(!note_visible_in(TICKS_PER_BAR * 8, 480, 60, &vp));
    // Alcanzando el tick final sigue siendo visible (borde inclusivo).
    assert!(note_visible_in(TICKS_PER_BAR * 2, 480, 60, &vp));
}

// =========================================================================
// REGRESIÓN HEADLESS (ventana real)
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

/// Abre la app con el piano roll desplegado y la deja dibujar unos cuantos
/// frames (el layout inicial necesita un prepaint antes de poder encuadrar).
fn open_piano_roll(cx: &mut TestAppContext) -> gpui_kit::WindowHandle<HikaruApp> {
    cx.update(gpui_kit::init);
    let handle = cx.add_window(|window, cx| test_app(window, cx));
    cx.simulate_window_resize(handle.into(), size(px(1280.0), px(720.0)));
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.show_piano_roll = true;
                cx.notify();
            });
        });
    })
    .unwrap();

    for _ in 0..6 {
        cx.update_window(handle.into(), |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
    }
    handle
}

/// `(offset_y, max_offset_y, layout_initialized)` del scroller de la grilla.
fn grid_scroll(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) -> (f32, f32, bool) {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        let mut out = (0.0f32, 0.0f32, false);
        app.update(cx, |app, cx| {
            let prs = &app.state.read(cx).piano_roll_state;
            out = (
                prs.grid_scroll.offset().y.as_f32(),
                prs.grid_scroll.max_offset().y.as_f32(),
                prs.layout_initialized,
            );
        });
        out
    })
    .unwrap()
}

#[gpui_kit::gpui::test]
fn el_piano_roll_tiene_scroll_vertical_real(cx: &mut TestAppContext) {
    let handle = open_piano_roll(cx);
    let (offset, max_offset, initialized) = grid_scroll(handle, cx);

    assert!(
        initialized,
        "el layout inicial (zoom + foco) tiene que haberse aplicado"
    );
    // 128 filas de 16 px + ruler = 2.072 px de contenido contra ~192 px de
    // viewport. Sin esto el usuario no puede llegar a las octavas bajas.
    assert!(
        max_offset > 500.0,
        "scroll vertical máximo de {max_offset} px: las octavas bajas siguen inaccesibles"
    );
    assert!(
        offset < -100.0,
        "la vista debería abrir scrolleada (offset {offset}), no clavada arriba en G9"
    );
}

#[gpui_kit::gpui::test]
fn la_vista_abre_en_c4_y_no_instancia_las_teclas_fuera_de_pantalla(cx: &mut TestAppContext) {
    let handle = open_piano_roll(cx);

    let (c4, c1) = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            (
                window.try_find(("pr_key", 60u64)).is_some(),
                window.try_find(("pr_key", 0u64)).is_some(),
            )
        })
        .unwrap();

    assert!(c4, "C4 tiene que estar visible al abrir el piano roll");
    assert!(
        !c1,
        "C1 (pitch 0) está a ~2.000 px de scroll: no debería instanciarse. \
         Si aparece, se están creando las 128 teclas otra vez."
    );
}

#[gpui_kit::gpui::test]
fn scrollear_abajo_hace_aparecer_las_teclas_bajas(cx: &mut TestAppContext) {
    let handle = open_piano_roll(cx);

    // La rueda tiene que mover el contenido en Y.
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.scroll(
            "pr_scroll_area",
            ScrollDelta::Pixels(point(px(0.0), px(-1200.0))),
            cx,
        );
    })
    .unwrap();

    let c1 = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            window.try_find(("pr_key", 0u64)).is_some()
        })
        .unwrap();

    let offset_after = grid_scroll(handle, cx).0;
    assert!(
        offset_after < -600.0,
        "la rueda vertical debería scrollear (offset {offset_after})"
    );
    assert!(
        c1,
        "tras scrollear abajo, C1 tiene que aparecer en el árbol (virtualización \
         bajo demanda, no un slice fijo)"
    );
}

#[gpui_kit::gpui::test]
fn la_grilla_no_crea_128_capas_de_tamano_completo(cx: &mut TestAppContext) {
    let handle = open_piano_roll(cx);

    // `pr_content` es el contenedor scrolleable: su ancho es el del contenido
    // completo (barras + overscan), pero la capa de grilla (`pr_grid_interaction`)
    // tiene que medir como máximo el viewport. Antes media grilla entera.
    let (content_w, interaction_w, interaction_h) = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            let content = window.find("pr_content").bounds();
            let interaction = window.find("pr_grid_interaction").bounds();
            (
                content.size.width.as_f32(),
                interaction.size.width.as_f32(),
                interaction.size.height.as_f32(),
            )
        })
        .unwrap();

    assert!(
        content_w < 12_000.0,
        "el contenido mide {content_w} px de ancho: el zoom/content sigue\
         generando un canvas gigante"
    );
    assert!(
        interaction_w <= 1400.0,
        "la capa de interacción mide {interaction_w} px: debería estar acotada al viewport"
    );
    assert!(
        interaction_h <= 400.0,
        "la capa de interacción mide {interaction_h} px de alto: debería estar\
         acotada al viewport, no a las 128 filas"
    );
}

#[test]
fn el_ruler_offset_empieza_por_la_tecla_mas_alta() {
    // Guarda contra un cambio accidental en el mapeo fila <-> pitch, que es la
    // causa raíz del "sólo veo G9".
    let vp = compute_grid_viewport(
        point(px(0.0), px(0.0)),
        size(px(1280.0), px(192.0)),
        DEFAULT_KEY_HEIGHT,
        0.06,
    );
    let top_pitch = (MIDI_KEY_COUNT as i64 - 1 - i64::from(vp.row_start)) as u8;
    assert_eq!(top_pitch, 127, "la fila 0 tiene que ser G9");
    assert!(RULER_HEIGHT > 0.0);
}
