// crates/hikaru_gui/tests/arranger_track_width_isolation.rs
//
// Verificación del aislamiento de ancho de canales en el Arranger View.
//
// Requisito Bitwig 5.1.7: Cada pista/canal en la consola de mezcla del Arranger
// debe poseer un ancho independiente y redimensionable de forma totalmente autónoma.
// Modificar el ancho de una pista no debe alterar el ancho de las demás pistas
// ni del canal Master o canales FX.

use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Arc;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{px, size, AppContext, Context, TestAppContext, Window};

use hikaru_gui::app::{AppMode, HikaruApp};
use hikaru_gui::audio_proxy::{AudioProxy, GuiCommand};
use hikaru_gui::views::arranger_view::{
    DEFAULT_TRACK_WIDTH, MAX_TRACK_WIDTH, MIN_TRACK_WIDTH, StripTarget,
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

fn open_arranger(cx: &mut TestAppContext) -> gpui_kit::WindowHandle<HikaruApp> {
    cx.update(gpui_kit::init);
    let handle = cx.add_window(|window, cx| test_app(window, cx));
    cx.simulate_window_resize(handle.into(), size(px(1440.0), px(800.0)));
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

#[gpui_kit::gpui::test]
fn test_track_width_isolation_openlive(cx: &mut TestAppContext) {
    let handle = open_arranger(cx);
    set_mode(handle, cx, AppMode::OpenLive, true);

    // 1. Verificar anchos iniciales por defecto
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().unwrap();
        let st = app.read(cx).state.clone();
        let s = st.read(cx);

        assert_eq!(s.track_width(StripTarget::Master), DEFAULT_TRACK_WIDTH);
        assert_eq!(s.track_width(StripTarget::Live(0)), DEFAULT_TRACK_WIDTH);
        assert_eq!(s.track_width(StripTarget::Live(1)), DEFAULT_TRACK_WIDTH);
        assert_eq!(s.track_width(StripTarget::Live(2)), DEFAULT_TRACK_WIDTH);
        assert_eq!(s.track_width(StripTarget::Fx(0)), DEFAULT_TRACK_WIDTH);
    })
    .unwrap();

    // 2. Modificar el ancho únicamente de Live(1)
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().unwrap();
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.set_track_width(StripTarget::Live(1), 220.0);
                cx.notify();
            });
        });
    })
    .unwrap();

    // 3. Confirmar que solo Live(1) cambió, y los demás permanecen intactos
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().unwrap();
        let st = app.read(cx).state.clone();
        let s = st.read(cx);

        assert_eq!(s.track_width(StripTarget::Master), DEFAULT_TRACK_WIDTH);
        assert_eq!(s.track_width(StripTarget::Live(0)), DEFAULT_TRACK_WIDTH);
        assert_eq!(s.track_width(StripTarget::Live(1)), 220.0);
        assert_eq!(s.track_width(StripTarget::Live(2)), DEFAULT_TRACK_WIDTH);
        assert_eq!(s.track_width(StripTarget::Fx(0)), DEFAULT_TRACK_WIDTH);

        // Validar live_tracks
        assert_eq!(s.live_tracks[2].width, 220.0); // live_tracks[0] es master, 2 es live(1)
        assert_eq!(s.live_tracks[1].width, DEFAULT_TRACK_WIDTH);
    })
    .unwrap();

    // 4. Probar límites de clamping (mínimo y máximo)
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().unwrap();
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                // Intentar valor menor a MIN_TRACK_WIDTH
                s.set_track_width(StripTarget::Live(0), 40.0);
                // Intentar valor mayor a MAX_TRACK_WIDTH
                s.set_track_width(StripTarget::Live(2), 600.0);
                cx.notify();
            });
        });
    })
    .unwrap();

    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().unwrap();
        let st = app.read(cx).state.clone();
        let s = st.read(cx);

        assert_eq!(s.track_width(StripTarget::Live(0)), MIN_TRACK_WIDTH);
        assert_eq!(s.track_width(StripTarget::Live(1)), 220.0);
        assert_eq!(s.track_width(StripTarget::Live(2)), MAX_TRACK_WIDTH);
    })
    .unwrap();

    // 5. Verificar que el render layout refleja el ancho individual de cada columna
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        let col0_w = window.find("arr_col_live_0").bounds().size.width.as_f32();
        let col1_w = window.find("arr_col_live_1").bounds().size.width.as_f32();
        let col2_w = window.find("arr_col_live_2").bounds().size.width.as_f32();

        assert!(
            (col0_w - MIN_TRACK_WIDTH).abs() < 1.0,
            "col_0 width debería ser {MIN_TRACK_WIDTH}, obtenido {col0_w}"
        );
        assert!(
            (col1_w - 220.0).abs() < 1.0,
            "col_1 width debería ser 220.0, obtenido {col1_w}"
        );
        assert!(
            (col2_w - MAX_TRACK_WIDTH).abs() < 1.0,
            "col_2 width debería ser {MAX_TRACK_WIDTH}, obtenido {col2_w}"
        );
    })
    .unwrap();
}

#[gpui_kit::gpui::test]
fn test_track_width_isolation_openstudio(cx: &mut TestAppContext) {
    let handle = open_arranger(cx);
    set_mode(handle, cx, AppMode::OpenStudio, false);

    // Modificar Studio(2)
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().unwrap();
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.studio_tracks.push(hikaru_gui::views::mixer::Track::new(2, "TRACK 02".to_string(), false));
                s.set_track_width(StripTarget::Studio(2), 175.0);
                cx.notify();
            });
        });
    })
    .unwrap();

    // Validar aislamiento
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().unwrap();
        let st = app.read(cx).state.clone();
        let s = st.read(cx);

        assert_eq!(s.track_width(StripTarget::Master), DEFAULT_TRACK_WIDTH);
        assert_eq!(s.track_width(StripTarget::Studio(1)), DEFAULT_TRACK_WIDTH);
        assert_eq!(s.track_width(StripTarget::Studio(2)), 175.0);
        assert_eq!(s.studio_tracks[2].width, 175.0);
        assert_eq!(s.studio_tracks[1].width, DEFAULT_TRACK_WIDTH);
    })
    .unwrap();
}

#[gpui_kit::gpui::test]
fn test_clip_launcher_track_header_unaffected_by_arranger_width(cx: &mut TestAppContext) {
    let handle = open_arranger(cx);
    set_mode(handle, cx, AppMode::OpenLive, true);

    // 1. Modificar el ancho de la pista 1 en Arranger
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().unwrap();
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.set_track_width(StripTarget::Live(1), 260.0);
                cx.notify();
            });
        });
    })
    .unwrap();

    // 2. Cambiar a la vista Clip Launcher (SessionMatrix)
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().unwrap();
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.openlive_view = hikaru_gui::app::OpenLiveView::SessionMatrix;
                cx.notify();
            });
        });
    })
    .unwrap();

    // 3. Comprobar que en el Clip Launcher (matrix.rs), los headers de pista
    // NO se alargaron a 260px, sino que conservan el ancho uniforme de cabecera (196px).
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        let hdr0_w = window.find("matrix_track_hdr_0").bounds().size.width.as_f32();
        let hdr1_w = window.find("matrix_track_hdr_1").bounds().size.width.as_f32();

        assert_eq!(hdr0_w, 196.0);
        assert_eq!(hdr1_w, 196.0);
    })
    .unwrap();
}


