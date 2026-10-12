// crates/hikaru_gui/tests/transport_logo.rs
//
// Tests del logo "Energy Core" embebido en la barra de transporte.
//
// El logo es la única imagen de la botonera, así que hay dos cosas que pueden
// romperse sin que se note leyendo el código: que el recorte del PNG deje el
// logo chiquito o descentrado dentro de su caja, y que el control se desalinee
// de los botones vecinos. Acá se miden los bounds reales del layout.

use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Arc;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{point, px, AppContext, Context, TestAppContext, Window};

use hikaru_gui::app::HikaruApp;
use hikaru_gui::audio_proxy::{AudioProxy, GuiCommand};

const LOGO_ID: &str = "transport_energy_core_logo";
const LOGO_IMG_ID: &str = "transport_energy_core_img";
const PLAY_ID: &str = "transport_play_pause";
const STOP_ID: &str = "transport_stop";

fn test_app(window: &mut Window, cx: &mut Context<HikaruApp>) -> HikaruApp {
    // El receptor se descarta a propósito: `AudioProxy::send` ignora el error de
    // envío, así que los handlers del header no bloquean ni rompen el test.
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
    // Igual que el binario (`main.rs`): inicializar las capas de gpui-kit
    // (tema de componentes) antes de abrir la ventana.
    cx.update(gpui_kit::init);
    let handle = cx.add_window(|window, cx| test_app(window, cx));
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        // El header se dibuja siempre: el logo no depende de la vista central.
        // El Play se usa de ancla para que un fallo acá signale "el header no
        // se dibujó", y no "el logo no está".
        assert!(
            window.try_find(PLAY_ID).is_some(),
            "el botón de Play debería estar: el header no se dibujó"
        );
        assert!(window.try_find(LOGO_ID).is_some());
    })
    .unwrap();
    handle
}

fn dsp_rack_open(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) -> bool {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.read(cx).state.read(cx).show_dsp_rack
    })
    .unwrap()
}

/// El control del logo comparte la altura de los botones vecinos.
///
/// Antes del recorte, el PNG venía en un lienzo 512x512 con el dibujo metido en
/// una franja central: la caja medía lo mismo pero el logo se veía a medias. El
/// alto es la mitad de esa historia (la caja); el `object_fit` del `img`, la
/// otra mitad, y este test es el que la vigila.
#[gpui_kit::gpui::test]
fn transport_logo_height_matches_neighbours(cx: &mut TestAppContext) {
    let handle = open_app(cx);

    let (logo_h, play_h, stop_h) = cx
        .update_window(handle.into(), |_, window, _cx| {
            // `find` pide `&'static str`, así que el id se pasa como tal.
            let h = |id: &'static str| window.find(id).bounds().size.height.as_f32();
            (h(LOGO_ID), h(PLAY_ID), h(STOP_ID))
        })
        .unwrap();

    assert!(
        (logo_h - play_h).abs() < 1.0,
        "alto del logo ({logo_h:.1}) != alto de Play ({play_h:.1})"
    );
    assert!(
        (logo_h - stop_h).abs() < 1.0,
        "alto del logo ({logo_h:.1}) != alto de Stop ({stop_h:.1})"
    );
}

/// El logo va pegado al Play y no al otro extremo de la barra: es el control
/// de entrada de la botonera, igual que el botón de estudio en un DAW.
#[gpui_kit::gpui::test]
fn transport_logo_sits_left_of_play_button(cx: &mut TestAppContext) {
    let handle = open_app(cx);

    let (logo_x, play_x, gap) = cx
        .update_window(handle.into(), |_, window, _cx| {
            let logo = window.find(LOGO_ID).bounds();
            let play = window.find(PLAY_ID).bounds();
            (
                logo.origin.x.as_f32(),
                play.origin.x.as_f32(),
                play.origin.x.as_f32() - (logo.origin.x.as_f32() + logo.size.width.as_f32()),
            )
        })
        .unwrap();

    assert!(
        logo_x < play_x,
        "el logo debería estar a la izquierda de Play (logo en {logo_x}, play en {play_x})"
    );
    // El `gap` de la barra es de 6px. Un hueco mayor significaría que se coló
    // un separador o padding extra entre el logo y el Play.
    assert!(
        (0.0..=8.0).contains(&gap),
        "el logo debería estar pegado al Play con el gap de la barra (gap = {gap:.1}px)"
    );
}

/// El logo queda centrado dentro de su control, en los dos ejes.
///
/// Este es el test que atrapa el bug real: `div()` en `gpui-pre` no arranca con
/// `display: flex`, así que sin un `.flex()` explícito `align_items` y
/// `justify_content` se ignoran y el `img` se dibuja pegado a la esquina
/// superior izquierda del control. El síntoma es un logo descentrado y pegado
/// al borde, no un contenedor desalineado: el contenedor siempre midió bien.
#[gpui_kit::gpui::test]
fn logo_is_centered_in_its_control(cx: &mut TestAppContext) {
    let handle = open_app(cx);

    let (cx_box, cy_box, cx_img, cy_img) = cx
        .update_window(handle.into(), |_, window, _cx| {
            let box_ = window.find(LOGO_ID).bounds();
            let img = window.find(LOGO_IMG_ID).bounds();
            (
                box_.origin.x.as_f32() + box_.size.width.as_f32() / 2.0,
                box_.origin.y.as_f32() + box_.size.height.as_f32() / 2.0,
                img.origin.x.as_f32() + img.size.width.as_f32() / 2.0,
                img.origin.y.as_f32() + img.size.height.as_f32() / 2.0,
            )
        })
        .unwrap();

    assert!(
        (cx_box - cx_img).abs() < 0.5,
        "el logo debería estar centrado en horizontal (caja {cx_box:.2} vs img {cx_img:.2})"
    );
    assert!(
        (cy_box - cy_img).abs() < 0.5,
        "el logo debería estar centrado en vertical (caja {cy_box:.2} vs img {cy_img:.2})"
    );
}

/// Clic en el logo: abre el panel del motor (el DSP Rack), igual que el botón
/// "DSP RACK (F10)" de la misma barra.
#[gpui_kit::gpui::test]
fn clicking_transport_logo_toggles_dsp_rack(cx: &mut TestAppContext) {
    let handle = open_app(cx);
    assert!(!dsp_rack_open(handle, cx), "el rack arranca cerrado");

    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.click_at(LOGO_ID, point(px(20.0), px(16.0)), cx);
    })
    .unwrap();
    assert!(dsp_rack_open(handle, cx), "el clic en el logo debería abrir el rack");

    // Y el toggle es reversible: un segundo clic lo cierra.
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.click_at(LOGO_ID, point(px(20.0), px(16.0)), cx);
    })
    .unwrap();
    assert!(
        !dsp_rack_open(handle, cx),
        "el segundo clic en el logo debería cerrar el rack"
    );
}