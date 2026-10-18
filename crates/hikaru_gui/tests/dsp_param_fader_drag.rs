// crates/hikaru_gui/tests/dsp_param_fader_drag.rs
//
// El fader horizontal de los parámetros de un VST3/CLAP "tenía lag".
// No era el motor: el seguimiento del arrastre estaba en el `on_mouse_move` de
// la tarjeta, y los listeners de elemento de GPUI Kit sólo se despachan si el
// hitbox está bajo el cursor (`div.rs`: `phase == Bubble && hitbox.is_hovered`).
// En cuanto el cursor salía de la tarjeta el gesto se congelaba y el `mouse_up`
// se perdía con él. La barra además es horizontal y el gesto era vertical.
//
// El seguimiento va a nivel de ventana (`dsp_rack::param_drag_keepalive`) y el
// gesto en X o Y según el eje que fije el primer movimiento. Estos tests fijan
// las dos cosas.

use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Arc;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{px, point, size, AppContext, Context, TestAppContext, Window};

use hikaru_gui::app::{HikaruApp, PARAM_DRAG_RANGE_PX};
use hikaru_gui::audio_proxy::{AudioProxy, GuiCommand};
use hikaru_gui::views::mixer::{DspSlot, ExternalParamState, ExternalParamsStatus};
use hikaru_plugin_host::PluginParamInfo;

const PARAM_ID: u32 = 7;

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

/// Rack abierto con un plugin externo de mentira en el slot 0 de la pista que el
/// rack muestra, y un parámetro continuo en 0.5. Devuelve el índice de esa pista:
/// `tracks()[0]` es el MASTER y el rack dibuja `safe_track_index()`.
fn open_rack(cx: &mut TestAppContext) -> (gpui_kit::WindowHandle<HikaruApp>, usize) {
    cx.update(gpui_kit::init);
    let handle = cx.add_window(|window, cx| test_app(window, cx));
    cx.simulate_window_resize(handle.into(), size(px(1280.0), px(720.0)));

    let track = cx
        .update_window(handle.into(), |view, _, cx| {
            let st = view.downcast::<HikaruApp>().expect("raíz HikaruApp").read(cx).state.clone();
            st.read(cx).safe_track_index()
        })
        .unwrap();

    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("raíz HikaruApp");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, _| {
                s.show_dsp_rack = true;
                let mut slot = DspSlot::new(0, "Uzu".to_string());
                slot.external = Some(hikaru_plugin_host::PluginDescriptor {
                    id: "fake.uzu".into(),
                    name: "Uzu".into(),
                    vendor: "Hikaru".into(),
                    format: hikaru_plugin_host::PluginFormat::VST3,
                    kind: hikaru_plugin_host::PluginKind::Effect,
                    path: "/tmp/fake.uzu".into(),
                    has_gui: true,
                    input_channels: None,
                });
                slot.params_status = ExternalParamsStatus::Ready;
                slot.external_params = vec![ExternalParamState {
                    info: PluginParamInfo {
                        id: PARAM_ID,
                        name: "Width".into(),
                        unit: String::new(),
                        min: 0.0,
                        max: 1.0,
                        default_value: 0.0,
                        step_count: 0,
                        can_automate: true,
                    },
                    value: 0.5,
                    dirty: false,
                }];
                s.tracks_mut()[track].effects.push(slot);
            });
            // El notify va en el contexto de HikaruApp: sin él la ventana no se
            // invalida y `find` sigue viendo el árbol viejo.
            cx.notify();
        });
    })
    .unwrap();
    (handle, track)
}

fn param_value(handle: gpui_kit::WindowHandle<HikaruApp>, track: usize, cx: &mut TestAppContext) -> f32 {
    cx.update_window(handle.into(), |view, _, cx| {
        let st = view.downcast::<HikaruApp>().expect("raíz HikaruApp").read(cx).state.clone();
        let s = st.read(cx);
        s.tracks()[track].effects[0]
            .external_params
            .iter()
            .find(|p| p.info.id == PARAM_ID)
            .expect("el slot conserva el parámetro")
            .value
    })
    .unwrap()
}

/// Centro de la barra del parámetro, ya dibujada (hace falta un frame antes).
fn bar_center(handle: gpui_kit::WindowHandle<HikaruApp>, track: usize, cx: &mut TestAppContext) -> gpui_kit::Point<gpui_kit::Pixels> {
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.find(format!("dsp_parambar_{track}_0_{PARAM_ID}")).bounds().center()
    })
    .unwrap()
}

/// El bug reportado: el cursor sale de la tarjeta y el valor se queda congelado.
/// Con el gesto atado al hitbox, 160px a la derecha no llegaban ni al 1.0.
#[gpui_kit::gpui::test]
fn drag_follows_the_cursor_all_the_way(cx: &mut TestAppContext) {
    let (handle, track) = open_rack(cx);
    assert_eq!(param_value(handle, track, cx), 0.5);
    let from = bar_center(handle, track, cx);

    cx.update_window(handle.into(), |_, window, cx| {
        window.drag(from, from + point(px(PARAM_DRAG_RANGE_PX), px(0.0)), cx);
    })
    .unwrap();

    let value = param_value(handle, track, cx);
    assert!(
        (value - 1.0).abs() < 1.0e-4,
        "{PARAM_DRAG_RANGE_PX}px a la derecha desde 0.5 tenía que dar 1.0, dio {value}: \
         el gesto se cortó al salir de la tarjeta"
    );
}

/// Una barra horizontal responde en X (antes el gesto leía sólo la Y), y
/// soltar fuera de la tarjeta cierra el gesto en vez de dejarlo colgado.
#[gpui_kit::gpui::test]
fn horizontal_drag_and_release_outside(cx: &mut TestAppContext) {
    let (handle, track) = open_rack(cx);
    let from = bar_center(handle, track, cx);

    // 40px = un cuarto de los 160px del recorrido.
    cx.update_window(handle.into(), |_, window, cx| {
        window.drag(from, from + point(px(40.0), px(0.0)), cx);
    })
    .unwrap();
    let dragged = param_value(handle, track, cx);
    assert!(
        (dragged - 0.75).abs() < 1.0e-3,
        "40px horizontales tienen que dar 0.5 + 0.25 = 0.75, dieron {dragged}"
    );

    let stuck = cx.update_window(handle.into(), |view, _, cx| {
        view.downcast::<HikaruApp>().expect("raíz HikaruApp").read(cx).state.read(cx).param_drag.is_some()
    })
    .unwrap();
    assert!(!stuck, "soltar el mouse tiene que limpiar `param_drag`, quedó colgado");
}