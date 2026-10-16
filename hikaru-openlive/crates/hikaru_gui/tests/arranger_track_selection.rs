// crates/hikaru_gui/tests/arranger_track_selection.rs
//
// Selección de pista desde el Arranger (OpenLive y OpenStudio).
//
// Regresión: las columnas del Arranger no tenían handler de clic ni rama de
// selección en el render. Lo único que escribía `selected_track_index` era una
// fila de 14px del rack de devices (`arr_track_N_dev_M`), imposible de acertar
// a ojo, y una pista sin devices no era seleccionable de ninguna manera. Como
// tampoco había indicador visual, el clic no producía ninguna señal observable.

use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Arc;

use gpui_kit::point;
use gpui_kit::test::TestWindowExt;
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

fn open_arranger(cx: &mut TestAppContext) -> gpui_kit::WindowHandle<HikaruApp> {
    cx.update(gpui_kit::init);
    let handle = cx.add_window(|window, cx| test_app(window, cx));
    cx.simulate_window_resize(handle.into(), size(px(1280.0), px(720.0)));
    handle
}

fn set_mode(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    mode: AppMode,
    live_view_arranger: bool,
) {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.mode = mode;
                if live_view_arranger {
                    s.openlive_view = hikaru_gui::app::OpenLiveView::ArrangerView;
                }
                if mode == AppMode::OpenStudio {
                    s.openstudio_view = hikaru_gui::app::OpenStudioView::ArrangerMixer;
                }
                cx.notify();
            });
        });
    })
    .unwrap();
}

/// Lee (track, slot) seleccionados desde el estado real de la app.
fn selection(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) -> (usize, usize) {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        let st = app.read(cx).state.clone();
        let s = st.read(cx);
        (s.selected_track_index, s.selected_slot_index)
    })
    .unwrap()
}

/// Clic en la columna, en una banda que no pertenece a ningún control hijo.
///
/// Los faders, pads y botones S/M/R tienen hitbox propia y se comen el clic
/// antes que el handler de la columna, así que hay que apuntar al relleno. Se
/// usa la esquina inferior izquierda: la columna tiene `p(px(3.0))`, así que los
/// 3 primeros píxeles de cada lado son suyos y no de ningún hijo.
///
/// Ojo: `click_at` toma el punto RELATIVO al elemento, no coordenadas de ventana.
fn click_column(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    id: &'static str,
) {
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        let height = window.find(id).bounds().size.height.as_f32();
        window.click_at(id, point(px(2.0), px(height - 2.0)), cx);
    })
    .unwrap();
}

/// OpenLive: la columna `arr_col_live_N` selecciona la pista N de la matriz.
///
/// El +1 es por MASTER (`live_tracks[0]`), que no tiene contraparte en la
/// matriz. Sin ese offset el rack de DSP apuntaría una pista más arriba.
#[gpui_kit::gpui::test]
fn openlive_column_selects_track(cx: &mut TestAppContext) {
    let handle = open_arranger(cx);
    set_mode(handle, cx, AppMode::OpenLive, true);

    click_column(handle, cx, "arr_col_live_0");
    assert_eq!(selection(handle, cx).0, 1, "columna 0 -> índice de mixer 1");

    click_column(handle, cx, "arr_col_live_3");
    assert_eq!(selection(handle, cx).0, 4, "columna 3 -> índice de mixer 4");

    click_column(handle, cx, "arr_col_live_1");
    assert_eq!(selection(handle, cx).0, 2);
}

/// La columna de SCENES comparte sitio con el strip del master, así que
/// seleccionarla tiene que llevar al master (índice 0).
#[gpui_kit::gpui::test]
fn openlive_scenes_column_selects_master(cx: &mut TestAppContext) {
    let handle = open_arranger(cx);
    set_mode(handle, cx, AppMode::OpenLive, true);

    click_column(handle, cx, "arr_col_live_2");
    assert_eq!(selection(handle, cx).0, 3);

    click_column(handle, cx, "arr_col_scenes");
    assert_eq!(selection(handle, cx).0, 0, "la columna de SCENES es el master");
}

/// OpenStudio no tiene el offset de MASTER: `studio_tracks` no lo incluye.
#[gpui_kit::gpui::test]
fn openstudio_column_selects_track_without_offset(cx: &mut TestAppContext) {
    let handle = open_arranger(cx);
    set_mode(handle, cx, AppMode::OpenStudio, false);

    click_column(handle, cx, "arr_col_studio_1");
    assert_eq!(
        selection(handle, cx).0,
        1,
        "en OpenStudio el índice va directo, sin el +1 del master"
    );
}

/// Seleccionar una columna cambia la PISTA pero no el slot del rack.
///
/// Si el clic de la columna pisara `selected_slot_index`, cambiar de pista
/// saltaría además al device 0 de esa pista.
#[gpui_kit::gpui::test]
fn column_click_does_not_move_selected_slot(cx: &mut TestAppContext) {
    let handle = open_arranger(cx);
    set_mode(handle, cx, AppMode::OpenLive, true);

    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.selected_slot_index = 2;
                cx.notify();
            });
        });
    })
    .unwrap();

    click_column(handle, cx, "arr_col_live_4");
    assert_eq!(selection(handle, cx), (5, 2));
}
