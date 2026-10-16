// crates/hikaru_gui/tests/dsp_param_drag_cost.rs
//
// El fader de parámetros de un plugin externo se sentía "lento como una tortuga".
// No era el motor ni el plugin: cada evento de mousemove del drag pasaba por
// `commit_external_param`, que notificaba siempre. Un `cx.notify()` reconstruye
// la ventana entera —con el clonado de `track.effects`, que incluye la
// wavetable de cada slot— así que un clic sin movimiento pagaba dos
// reconstrucciones enteras (armar y desarmar el gesto) y un drag pagaba una por
// píxel, aunque la mayoría de esos píxeles no cambian el valor tras el snapping
// a un parámetro discreto.
//
// Estos tests fijan que un gesto que no mueve nada no hace trabajo.

use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Arc;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{px, size, AppContext, Context, TestAppContext, Window};

use hikaru_gui::app::HikaruApp;
use hikaru_gui::audio_proxy::{AudioProxy, GuiCommand};
use hikaru_gui::views::mixer::ExternalParamState;

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

/// Info de parámetro continuo (sin steps), que es el caso habitual.
fn continuous(id: u32, name: &str) -> ExternalParamState {
    ExternalParamState {
        info: hikaru_plugin_host::PluginParamInfo {
            id,
            name: name.to_string(),
            unit: String::new(),
            min: 0.0,
            max: 1.0,
            default_value: 0.0,
            step_count: 0,
            can_automate: true,
        },
        value: 0.0,
        dirty: false,
    }
}

/// Info de parámetro discreto de 8 escalones: el caso donde más píxeles del
/// drag se traducen al mismo valor.
fn stepped(id: u32, name: &str) -> ExternalParamState {
    let mut p = continuous(id, name);
    p.info.step_count = 7; // 8 posiciones
    p
}

/// Deja el slot 0 de la pista 0 con la lista de parámetros indicada.
///
/// La app de test arranca sin slots, así que hay que crear uno antes de poder
/// escribir en él.
fn set_param(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    params: Vec<ExternalParamState>,
    id: u32,
    value: f32,
) -> Option<f32> {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, _| {
                let Some(track) = s.tracks_mut().first_mut() else {
                    return;
                };
                if track.effects.is_empty() {
                    track
                        .effects
                        .push(hikaru_gui::views::mixer::DspSlot::new(0, "test".to_string()));
                }
                let slot = track.effects.first_mut().expect("recién creado");
                slot.external_params = params;
                slot.set_external_param(id, value);
            });
        });
    })
    .unwrap();

    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        let st = app.read(cx).state.clone();
        let s = st.read(cx);
        s.tracks()
            .first()
            .and_then(|t| t.effects.first())
            .and_then(|slot| slot.external_params.iter().find(|p| p.info.id == id))
            .map(|p| p.value)
    })
    .unwrap()
}

/// Aplicar el mismo valor dos veces: la segunda no debe cambiar nada.
///
/// Es el caso que paga el arrastre lento. En un parámetro discreto, ~7 de cada 8
/// píxeles del drag caen en el mismo escalón; si cada uno contara como cambio,
/// cada uno reconstruiría la ventana.
#[gpui_kit::gpui::test]
fn applying_the_same_value_reports_no_change(cx: &mut TestAppContext) {
    let handle = open_app(cx);

    let v = set_param(handle, cx, vec![continuous(7, "gain")], 7, 0.5);
    assert_eq!(v, Some(0.5));

    // Mismo valor exacto.
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, _| {
                if let Some(slot) = s.tracks_mut().first_mut().and_then(|t| t.effects.first_mut())
                {
                    assert!(
                        slot.set_external_param(7, 0.5).is_none(),
                        "reaplicar el mismo valor tiene que reportar que no cambió"
                    );
                }
            });
        });
    })
    .unwrap();
}

/// Un parámetro discreto reporta "cambió" sólo cuando el snapping mueve de
/// escalón. Dos valores crudos distintos que caen en el mismo escalón no cuentan.
#[gpui_kit::gpui::test]
fn stepped_param_reports_change_only_per_step(cx: &mut TestAppContext) {
    let handle = open_app(cx);
    set_param(handle, cx, vec![stepped(3, "mode")], 3, 0.0);

    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, _| {
                let slot = s
                    .tracks_mut()
                    .first_mut()
                    .and_then(|t| t.effects.first_mut())
                    .expect("slot 0");

                // Primer movimiento real: 0.0 -> 0.5 (medio escalón de 8).
                let applied = slot.set_external_param(3, 0.5);
                assert_eq!(applied, Some(0.5), "el primer medio escalón sí cambia");

                // 0.6 cae en el mismo escalón que 0.5 (8 escalones: 0.125 de paso,
                // 0.5 -> 0.625). No debe reportar cambio.
                let again = slot.set_external_param(3, 0.6);
                assert_eq!(
                    again,
                    None,
                    "0.6 se snappea al mismo escalón que 0.5: no es un cambio"
                );
            });
        });
    })
    .unwrap();
}

/// Un id inexistente devuelve `None` en vez de crear un parámetro fantasma.
#[gpui_kit::gpui::test]
fn unknown_param_id_reports_no_change(cx: &mut TestAppContext) {
    let handle = open_app(cx);
    set_param(handle, cx, vec![continuous(7, "gain")], 7, 0.0);

    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, _| {
                let slot = s
                    .tracks_mut()
                    .first_mut()
                    .and_then(|t| t.effects.first_mut())
                    .expect("slot 0");
                assert!(
                    slot.set_external_param(999, 0.5).is_none(),
                    "un id que no existe no debe reportar un cambio"
                );
                assert_eq!(slot.external_params.len(), 1, "ni crear un parámetro nuevo");
            });
        });
    })
    .unwrap();
}

/// `set_external_param` devuelve el valor efectivo, ya redondeado.
///
/// El motor recibía antes el valor crudo del drag mientras la GUI mostraba y el
/// editor nativo recibían el redondeado: en un parámetro discreto, lo que se
/// veía y lo que sonaba no coincidían.
#[gpui_kit::gpui::test]
fn returned_value_is_the_snapped_one_not_the_raw_one(cx: &mut TestAppContext) {
    let handle = open_app(cx);
    set_param(handle, cx, vec![stepped(5, "mode")], 5, 0.0);

    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, _| {
                let slot = s
                    .tracks_mut()
                    .first_mut()
                    .and_then(|t| t.effects.first_mut())
                    .expect("slot 0");
                let applied = slot.set_external_param(5, 0.4);
                let stored = slot
                    .external_params
                    .iter()
                    .find(|p| p.info.id == 5)
                    .map(|p| p.value);
                assert_eq!(
                    applied, stored,
                    "lo que devuelve tiene que ser lo que queda guardado (el \
                     valor redondeado, no el crudo del drag)"
                );
            });
        });
    })
    .unwrap();
}
