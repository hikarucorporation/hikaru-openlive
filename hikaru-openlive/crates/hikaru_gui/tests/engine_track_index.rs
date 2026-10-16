// crates/hikaru_gui/tests/engine_track_index.rs
//
// Regresión del off-by-one entre el mixer y el motor de audio.
//
// Síntoma: "el plugin carga pero no hace nada, como si el track estuviera
// muteado". El motor mezcla el clip de la pista visible N en
// `track_output_bufs[N]` y corre la cadena en `track_effects[N]`, pero la GUI
// tiene dos espacios de índices: el mixer mete un MASTER en la posición 0, así
// que la pista visible N vive en `tracks()[N + 1]`. El rack de DSP mandaba su
// índice de mixer crudo al motor, con lo cual la cadena se insertaba en
// `track_effects[N + 1]` mientras el clip se mezclaba en `[N]`: el plugin
// procesaba un buffer siempre mudo y el audio seco pasaba de largo.
//
// Estos tests fijan la conversión en un solo lugar (`engine_track_index`) y
// comprueban que la pista que la GUI considera seleccionada es exactamente la
// que el motor va a procesar.

use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Arc;

use gpui_kit::{px, size, AppContext, Context, TestAppContext, Window};

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

fn open_app(cx: &mut TestAppContext) -> gpui_kit::WindowHandle<HikaruApp> {
    cx.update(gpui_kit::init);
    let handle = cx.add_window(|window, cx| test_app(window, cx));
    cx.simulate_window_resize(handle.into(), size(px(1280.0), px(720.0)));
    handle
}

/// Selecciona la pista visible `matrix_idx` como lo haría un clic en el
/// Session Matrix, y devuelve el índice que el motor debería usar.
fn engine_index_for_matrix_track(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    matrix_idx: usize,
) -> Option<usize> {
    // Escribir y leer en dos pasadas: `update_window` consume la vista, así que
    // no se puede usar dos veces dentro del mismo closure.
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.mode = AppMode::OpenLive;
                s.selected_track_index =
                    hikaru_gui::views::matrix::mixer_index_of_matrix_track(matrix_idx);
                cx.notify();
            });
        });
    })
    .unwrap();

    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.read(cx).state.read(cx).engine_track_index()
    })
    .unwrap()
}

/// La pista que la GUI muestra como N tiene que ser la pista N del motor.
///
/// Este es el invariante completo del bug: la GUI selecciona la pista visible N
/// (vía `mixer_index_of_matrix_track`) y el motor tiene que recibir N, no N+1.
#[gpui_kit::gpui::test]
fn matrix_track_n_maps_to_engine_track_n(cx: &mut TestAppContext) {
    let handle = open_app(cx);
    for n in 0..8 {
        assert_eq!(
            engine_index_for_matrix_track(handle, cx, n),
            Some(n),
            "la pista visible {n} debe llegar al motor como {n}, no como {}",
            n + 1
        );
    }
}

/// La primera pista visible del Session Matrix (índice 0) es la pista 0 del
/// motor. Antes del fix el rack insertaba la cadena en 1.
#[gpui_kit::gpui::test]
fn first_visible_track_is_engine_track_zero(cx: &mut TestAppContext) {
    let handle = open_app(cx);
    assert_eq!(engine_index_for_matrix_track(handle, cx, 0), Some(0));
}

/// El master no existe en el motor: no hay cadena donde insertarse.
///
/// `tracks()[0]` es el MASTER, así que `engine_track_index()` tiene que
/// devolver `None` y el llamador abortar el comando en vez de mandar un 0 que
/// el motor interpretaría como "la primera pista".
#[gpui_kit::gpui::test]
fn master_has_no_engine_track(cx: &mut TestAppContext) {
    let handle = open_app(cx);
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.mode = AppMode::OpenLive;
                s.selected_track_index = 0; // MASTER
                cx.notify();
            });
        });
    })
    .unwrap();

    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        assert_eq!(app.read(cx).state.read(cx).engine_track_index(), None);
    })
    .unwrap();
}

/// `safe_track_index` recorta al rango válido, pero `engine_track_index` tiene
/// que seguir restando el master después del recorte. Un índice guardado
/// veloz (pista borrada) no puede convertirse en el master ni en la pista -1.
#[gpui_kit::gpui::test]
fn stale_index_is_clamped_without_underflowing(cx: &mut TestAppContext) {
    let handle = open_app(cx);
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.mode = AppMode::OpenLive;
                // Índice muy por encima del número de pistas.
                s.selected_track_index = 9_999;
                cx.notify();
            });
        });
    })
    .unwrap();

    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        let st = app.read(cx).state.clone();
        let s = st.read(cx);
        let engine = s.engine_track_index();
        assert!(
            engine.is_some(),
            "una pista real siempre tiene índice de motor"
        );
        assert!(
            engine.expect("ya verificado") < 16,
            "el índice de motor tiene que caber en las 16 pistas del motor, \
             quedó {engine:?}"
        );
    })
    .unwrap();
}
