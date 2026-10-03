// crates/hikaru_gui/tests/arranger_columns_fit.rs
//
// Encaje vertical de las tiras de canal del Arranger (OpenLive y OpenStudio).
//
// Mide layout real a 1280×720: el botón REC de la última columna (último
// elemento del strip) debe quedar dentro del área de columnas, o sea sin
// scrollbar vertical en la configuración default.

use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Arc;

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

/// Fondo del último control de la columna indicada vs fondo del área.
fn last_bottom(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    last_id: &'static str,
) -> (f32, f32) {
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        let area = window.find("arranger_columns");
        let last = window.find(last_id);
        let ab = area.bounds();
        let lb = last.bounds();
        (
            ab.origin.y.as_f32() + ab.size.height.as_f32(),
            lb.origin.y.as_f32() + lb.size.height.as_f32(),
        )
    })
    .unwrap()
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
                cx.notify();
            });
        });
    })
    .unwrap();
}

/// OpenLive + ArrangerView: 8 pistas de matriz + master, strip compacto.
#[gpui_kit::gpui::test]
fn openlive_columns_fit_without_vertical_scroll(cx: &mut TestAppContext) {
    let handle = open_arranger(cx);
    set_mode(handle, cx, AppMode::OpenLive, true);
    let (area_bottom, last_bottom) = last_bottom(handle, cx, "arr_track_7_rec");
    println!(
        "arranger-live slack vertical: {:.1}px (área {:.1}, última {:.1})",
        area_bottom - last_bottom,
        area_bottom,
        last_bottom
    );
    assert!(
        last_bottom <= area_bottom + 1.0,
        "la última columna (fondo en {last_bottom}) debería caber en el área (fondo en {area_bottom})"
    );
}

/// OpenStudio: master + 1 pista con waveform + mismo strip.
#[gpui_kit::gpui::test]
fn openstudio_columns_fit_without_vertical_scroll(cx: &mut TestAppContext) {
    let handle = open_arranger(cx);
    set_mode(handle, cx, AppMode::OpenStudio, false);
    let (area_bottom, last_bottom) = last_bottom(handle, cx, "arr_studio_1_rec");
    println!(
        "arranger-studio slack vertical: {:.1}px (área {:.1}, última {:.1})",
        area_bottom - last_bottom,
        area_bottom,
        last_bottom
    );
    assert!(
        last_bottom <= area_bottom + 1.0,
        "la última columna (fondo en {last_bottom}) debería caber en el área (fondo en {area_bottom})"
    );
}
