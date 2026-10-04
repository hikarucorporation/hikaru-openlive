// crates/hikaru_gui/tests/studio_playlist_layout.rs
//
// Layout del modo OpenStudio: Vista de Mezcla + Vista de Arreglo.
//
// La Playlist / Timeline es SÓLO tiempo (ruler, filas limpias, grilla,
// clips, playhead): ningún control de mezcla (S/M/R, faders, dB, pan) puede
// sangrar adentro del lienzo. La mezcla vive en `studio_mixer_panel` y el
// botón MIXER conmuta entre pantalla dividida y timeline a todo el ancho.
//
// Regresión 1: el `Scrollable` de gpui-component consume el `id` de su
// contenido y no debe usarse directamente como flex-item del layout —
// hacerlo encogía el panel a su padding (12px) y empujaba toda la playlist
// fuera de la ventana (x ≈ 1280). Por eso el scroll vive en un hijo interno
// y el flex-item es un `div` plano (mismo patrón que `arranger_columns`).

use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Arc;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{px, size, AppContext, Context, TestAppContext, Window};

use hikaru_gui::app::{AppMode, HikaruApp};
use hikaru_gui::audio_proxy::{AudioProxy, GuiCommand};
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

/// En OpenStudio la Playlist / Timeline comparte el área central con el
/// mixer lateral y queda visible dentro de la ventana, sin controles de
/// mezcla adentro del lienzo temporal.
#[gpui_kit::gpui::test]
fn openstudio_playlist_panel_is_visible_beside_mixer(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        let area = window.find("arranger_columns").bounds();
        let mixer = window.find("studio_mixer_panel").bounds();
        let panel = window.find("studio_playlist_panel").bounds();
        println!(
            "studio layout: área {:.0}x{:.0}, mixer x={:.0} w={:.0}, playlist x={:.0} w={:.0}",
            area.size.width.as_f32(),
            area.size.height.as_f32(),
            mixer.origin.x.as_f32(),
            mixer.size.width.as_f32(),
            panel.origin.x.as_f32(),
            panel.size.width.as_f32(),
        );
        // El mixer lateral conserva su ancho de columnas…
        assert!(
            mixer.size.width.as_f32() > 200.0,
            "mixer lateral colapsado: {:?}",
            mixer.size.width
        );
        // …y la playlist ocupa el resto (a 1280, más de 400px)…
        assert!(
            panel.size.width.as_f32() > 400.0,
            "panel playlist colapsado (se vio de 12px): {:?}",
            panel.size.width
        );
        // …empezando donde termina el mixer, no fuera de la ventana.
        let mixer_right = mixer.origin.x.as_f32() + mixer.size.width.as_f32();
        assert!(
            (panel.origin.x.as_f32() - mixer_right).abs() < 2.0,
            "playlist desalineada del mixer: panel x={:.0}, mixer right={:.0}",
            panel.origin.x.as_f32(),
            mixer_right
        );
        assert!(
            panel.origin.x.as_f32() + panel.size.width.as_f32() <= area.size.width.as_f32() + 1.0,
            "playlist fuera de la ventana: origen {:?} tamaño {:?}",
            panel.origin,
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
        // …y ningún control de mezcla adentro del lienzo de la playlist.
        for id in ["pl_track_solo_1", "pl_track_mute_1"] {
            assert!(
                window.try_find(id).is_none(),
                "sangrado del mixer en la playlist: {id} no debería existir"
            );
        }
    })
    .unwrap();
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

/// El botón MIXER oculta la consola y la Playlist pasa a todo el ancho.
#[gpui_kit::gpui::test]
fn openstudio_mixer_toggle_gives_playlist_full_width(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.show_studio_mixer = false;
                cx.notify();
            });
        });
    })
    .unwrap();
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        assert!(
            window.try_find("studio_mixer_panel").is_none(),
            "el mixer debería estar oculto con show_studio_mixer=false"
        );
        let area = window.find("arranger_columns").bounds();
        let panel = window.find("studio_playlist_panel").bounds();
        assert!(
            panel.origin.x.as_f32() < 2.0,
            "playlist debería empezar en x≈0: {:?}",
            panel.origin
        );
        assert!(
            panel.size.width.as_f32() >= area.size.width.as_f32() - 24.0,
            "playlist debería ocupar casi todo el ancho: {:?} vs área {:?}",
            panel.size,
            area.size
        );
        // La cabecera de la playlist sigue operativa a todo el ancho.
        let zoom = window.try_find("pl_zoom_in").expect("zoom debe existir");
        assert!(zoom.visible(), "zoom no visible a todo el ancho");
    })
    .unwrap();
}
