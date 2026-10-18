// crates/hikaru_gui/tests/studio_playlist_layout.rs
//
// Vistas centrales de OpenStudio: Playlist / Timeline vs Arranger / Mixer.
//
// `Tab` / `F9` conmutan entre ambas SIN cambiar de modo (`toggle_central_view`);
// en OpenLive conmutan Session Matrix ↔ Arranger. La Playlist es SÓLO tiempo
// (ruler, grilla, clips, playhead): sus Track Headers replican la caja de
// mezcla del Session Matrix (M/S + pan + volumen, replicated on purpose), pero
// la CONSOLA completa de channel strips (faders verticales, dB por canal, DSP)
// vive exclusivamente en la vista ArrangerMixer.
//
// Regresión layout: el `Scrollable` de gpui-component consume el `id` de su
// contenido y no debe usarse directamente como flex-item — hacerlo encogía el
// panel a su padding (12px) y empujaba la vista fuera de la ventana. Por eso
// el scroll vive en un hijo interno y el flex-item es un `div` plano (mismo
// patrón que `arranger_columns`).

use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Arc;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{px, size, AppContext, Context, TestAppContext, Window};

use hikaru_gui::app::{AppMode, HikaruApp, OpenLiveView, OpenStudioView};
use hikaru_gui::audio_proxy::{AudioProxy, GuiCommand};
use hikaru_gui::views::mixer::{duplicate_device_chain, DspSlot};
use hikaru_gui::views::playlist::{ClipType, PlaylistClip};

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

fn open_live_arranger(cx: &mut TestAppContext) -> gpui_kit::WindowHandle<HikaruApp> {
    cx.update(gpui_kit::init);
    let handle = cx.add_window(|window, cx| test_app(window, cx));
    cx.simulate_window_resize(handle.into(), size(px(1280.0), px(720.0)));
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.mode = AppMode::OpenLive;
                s.openlive_view = OpenLiveView::ArrangerView;
                cx.notify();
            });
        });
    })
    .unwrap();
    handle
}

fn studio_view(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
) -> (AppMode, OpenStudioView) {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.update(cx, |app, cx| {
            let st = app.state.read(cx);
            (st.mode, st.openstudio_view)
        })
    })
    .unwrap()
}

/// Vista default de OpenStudio: Playlist a todo el ancho, sin mixer y sin
/// controles de mezcla adentro del lienzo temporal.
#[gpui_kit::gpui::test]
fn openstudio_defaults_to_full_width_playlist(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    assert_eq!(studio_view(handle, cx).1, OpenStudioView::Playlist);
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        let area = window.find("arranger_columns").bounds();
        let panel = window.find("studio_playlist_panel").bounds();
        println!(
            "studio playlist: área {:.0}x{:.0}, panel x={:.0} w={:.0}",
            area.size.width.as_f32(),
            area.size.height.as_f32(),
            panel.origin.x.as_f32(),
            panel.size.width.as_f32(),
        );
        assert!(
            panel.origin.x.as_f32() < 2.0,
            "la playlist debería empezar en x≈0: {:?}",
            panel.origin
        );
        assert!(
            panel.size.width.as_f32() >= area.size.width.as_f32() - 24.0,
            "la playlist debería ocupar casi todo el ancho: {:?}",
            panel.size
        );
        // Cabecera (zoom/quantize), fila limpia y pie visibles y en ventana.
        for id in ["pl_zoom_in", "pl_zoom_out", "pl_row_1", "pl_add_track"] {
            let el = window.try_find(id).expect("control playlist debe existir");
            assert!(el.visible(), "{id} no visible");
            assert!(
                el.bounds().origin.x.as_f32() < area.size.width.as_f32(),
                "{id} fuera de la ventana: {:?}",
                el.bounds()
            );
        }
        // Los Track Headers de la Playlist replican la caja de mezcla del
        // Session Matrix (M/S/pan/volumen): es el diseño pedido, no un
        // "sangrado" del mixer lateral. Lo que NO debe aparecer es la CONSOLA
        // completa de channel strips (faders verticales, dB por canal, DSP).
        for id in ["pl_track_mute_1", "pl_track_solo_1", "pl_vol_1"] {
            assert!(
                window.try_find(id).is_some(),
                "el track header de la playlist debería traer el control {id}"
            );
        }
        assert!(
            window.try_find("studio_mixer_panel").is_none(),
            "la consola de mezcla completa no debería renderizarse en la vista Playlist"
        );
    })
    .unwrap();
}

/// `Tab` en OpenStudio alterna Playlist ↔ Arranger/Mixer sin salir del modo.
#[gpui_kit::gpui::test]
fn openstudio_tab_toggles_playlist_and_mixer(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    assert_eq!(
        studio_view(handle, cx),
        (AppMode::OpenStudio, OpenStudioView::Playlist)
    );
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.press("tab", cx);
    })
    .unwrap();
    assert_eq!(
        studio_view(handle, cx),
        (AppMode::OpenStudio, OpenStudioView::ArrangerMixer),
        "Tab debería llevar al mixer SIN salir de OpenStudio"
    );
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        // Consola visible a todo el ancho, playlist fuera del árbol.
        let panel = window.find("studio_mixer_panel").bounds();
        assert!(panel.size.width.as_f32() > 400.0, "mixer colapsado");
        assert!(
            window.try_find("studio_playlist_panel").is_none(),
            "la playlist no debería renderizarse en la vista Mixer"
        );
        let rec = window.try_find("arr_studio_1_rec").expect("strip TRK 01");
        assert!(rec.visible(), "canal TRK 01 no visible en el mixer");
        window.press("tab", cx);
    })
    .unwrap();
    assert_eq!(
        studio_view(handle, cx),
        (AppMode::OpenStudio, OpenStudioView::Playlist),
        "el segundo Tab debería volver a la Playlist"
    );
}

/// `F9` comparte el comportamiento contextual de `Tab` en OpenStudio.
#[gpui_kit::gpui::test]
fn openstudio_f9_matches_tab(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.press("f9", cx);
    })
    .unwrap();
    assert_eq!(
        studio_view(handle, cx),
        (AppMode::OpenStudio, OpenStudioView::ArrangerMixer),
        "F9 debería llevar al mixer SIN salir de OpenStudio"
    );
}

/// `Tab` en OpenLive sigue alternando Session Matrix ↔ Arranger sin
/// tocar OpenStudio.
#[gpui_kit::gpui::test]
fn openlive_tab_still_toggles_matrix_and_arranger(cx: &mut TestAppContext) {
    let handle = open_live_arranger(cx);
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.press("tab", cx);
    })
    .unwrap();
    let (mode, live_view) = cx
        .update_window(handle.into(), |view, _, cx| {
            let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
            app.update(cx, |app, cx| {
                let st = app.state.read(cx);
                (st.mode, st.openlive_view)
            })
        })
        .unwrap();
    assert_eq!(mode, AppMode::OpenLive);
    assert_eq!(
        live_view,
        OpenLiveView::SessionMatrix,
        "Tab en OpenLive debería volver a la Session Matrix"
    );
}

/// Un clip que arranca en el tick 0 se renderiza a la derecha de la columna
/// de headers — nunca debajo de `TRK 01`. El área bajo el compás 1 queda
/// limpia para clips.
#[gpui_kit::gpui::test]
fn openstudio_clip_at_tick_zero_starts_right_of_headers(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                // Pista 1 del vector (`TRK 01` en el Studio default) + clip de
                // 4 compases desde el tick 0.
                s.playlist_state.clips.push((
                    1,
                    PlaylistClip {
                        id: 99,
                        name: "Kick".to_string(),
                        start_tick: 0,
                        duration_ticks: 4 * s.playlist_state.ppqn.max(1),
                        clip_type: ClipType::Pattern { pattern_id: 0 },
                        color: gpui_kit::rgb(0x205F91).into(),
                    },
                ));
                cx.notify();
            });
        });
    })
    .unwrap();
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        let panel = window.find("studio_playlist_panel").bounds();
        let clip = window.try_find("pl_clip_99").expect("el clip debe existir");
        assert!(clip.visible(), "clip no visible");
        let clip_local_x = clip.bounds().origin.x.as_f32() - panel.origin.x.as_f32();
        println!("clip bajo compás 1: x local al panel = {clip_local_x:.1}");
        // La columna de headers mide 180px (+6 de padding del panel): el clip
        // en tick 0 debe arrancar a su derecha, no debajo.
        assert!(
            clip_local_x >= 180.0,
            "clip pisando los headers: x local {clip_local_x:.1}"
        );
    })
    .unwrap();
}

/// Clic en la cabecera de la Playlist selecciona el canal del Mixer lateral:
/// `selected_track_index` es la única fuente que miran el strip (`S/M/R`,
/// faders, DEVICES) y el DSP Rack (`safe_track_index`), así que un clic
/// deja el canal listo para editar.
#[gpui_kit::gpui::test]
fn openstudio_playlist_header_click_selects_mixer_channel(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    // Selección inicial en el master para que el clic cambie algo observable.
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.selected_track_index = 0;
                cx.notify();
            });
        });
    })
    .unwrap();
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.click("pl_row_1", cx);
    })
    .unwrap();
    let (selected, safe, name) = cx
        .update_window(handle.into(), |view, _, cx| {
            let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
            app.update(cx, |app, cx| {
                let st = app.state.read(cx);
                (
                    st.selected_track_index,
                    st.safe_track_index(),
                    st.tracks()[st.safe_track_index()].name.clone(),
                )
            })
        })
        .unwrap();
    assert_eq!(
        selected, 1,
        "el clic en TRK 01 debería seleccionar el canal 1 (quedó en {selected})"
    );
    assert_eq!(safe, 1, "el DSP Rack debería resolver el mismo canal");
    assert!(
        name.contains("TRACK 01"),
        "el canal seleccionado debería ser TRACK 01 (es {name})"
    );
}

/// La cadena de DEVICES se comparte entre canales por valor: clonar la de
/// una pista la deja lista en el destino (mismo u otro modo) con nombres,
/// bypass y orden intactos.
#[test]
fn device_chain_duplicates_transparently_across_channels() {
    let mut src = DspSlot::new(0, "OpenWavetable".to_string());
    src.active = true;
    let mut fx = DspSlot::new(1, "OpenSpectralFX".to_string());
    fx.active = false;
    let chain = vec![src, fx];

    let copy = duplicate_device_chain(&chain);

    assert_eq!(copy.len(), 2);
    assert_eq!(copy[0].name, "OpenWavetable");
    assert!(copy[0].active);
    assert_eq!(copy[1].name, "OpenSpectralFX");
    assert!(!copy[1].active);
    // Sin aliasing: mutar la copia no toca el origen.
    let mut mutated = copy;
    mutated[0].name = "Otro".to_string();
    assert_eq!(chain[0].name, "OpenWavetable");
}
