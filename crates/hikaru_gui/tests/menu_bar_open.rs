// crates/hikaru_gui/tests/menu_bar_open.rs
//
// Tests de la barra de menús (FILE / EDIT / VIEW / SETTINGS / HELP).
//
// Dos regresiones quedan fijadas acá:
//
// 1. El desplegable NO ABRÍA. `menu_bar::render` creaba el estado del menú con
//    `cx.new(...)` DENTRO del render; como la barra se redibuja en cada frame,
//    el click actualizaba una entidad que al frame siguiente ya no se dibujaba.
//    El estado pasó a `AppState::open_menu` (espejo del popover).
//
// 2. El desplegable se ABRÍA APLASTADO a una franja de unos pocos píxeles. Un
//    `div().absolute()` posicionado a mano se layoutaba contra el bloque
//    contenedor del botón, que mide lo que el botón; con `top: 24px` no
//    quedaba espacio y GPUI lo encogía. Ahora usa `Button::dropdown_menu` de
//    gpui-kit. El test mide los BOUNDS del popover: si vuelve a aplastarse, el
//    alto no da.

use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Arc;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{px, size, AppContext, Context, TestAppContext, Window};

use hikaru_gui::app::{AppMode, AppState, HikaruApp, OpenStudioView};
use hikaru_gui::audio_proxy::{AudioProxy, GuiCommand};
use hikaru_gui::views::menu_bar::POPUP_ID;

/// Alto mínimo por ítem del desplegable, en px. Es un piso generoso: el bug
/// era de 8px para 6 ítems, no de 1px por ítem.
const MIN_H_PER_ITEM: f32 = 18.0;

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
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
    })
    .unwrap();
    handle
}

/// Click real (con hit-testing) sobre un elemento por id, y redibuque.
fn click(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext, id: &'static str) {
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.click(id, cx);
        window.draw(cx).clear(cx);
    })
    .unwrap();
}

/// Click sobre el ítem `index` del popover abierto.
fn click_item(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    index: usize,
) {
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.within(POPUP_ID).click(index, cx);
        window.draw(cx).clear(cx);
    })
    .unwrap();
}

fn open_menu(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) -> Option<usize> {
    set_state(handle, cx, |state| state.open_menu)
}

fn set_state<R>(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    f: impl FnOnce(&mut AppState) -> R,
) -> R {
    cx.update_window(handle.into(), |view, _, cx| {
        let state = view
            .downcast::<HikaruApp>()
            .expect("raíz")
            .read(cx)
            .state
            .clone();
        let out = state.update(cx, |state, _| f(state));
        cx.notify(state.entity_id());
        out
    })
    .unwrap()
}

/// ¿Está el popover en pantalla con un alto que corresponda a su cantidad de
/// ítems?
///
/// El conteo va como parámetro porque los menús no tienen el mismo tamaño:
/// HELP tiene un ítem y su popover legitimamente mide 34px, mientras que FILE
/// tiene seis. Lo que no puede pasar es que un menú de seis ítems mida 8px.
fn assert_popup_open(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    items: usize,
) {
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        let Some(popup) = window.try_find(POPUP_ID) else {
            panic!("el desplegable no está en pantalla");
        };
        let height = popup.bounds().size.height;
        let min = px(MIN_H_PER_ITEM * items as f32);
        assert!(
            height >= min,
            "el desplegable de {items} ítems mide {:?}: se ve aplastado. Un \
             `absolute()` a mano se layouta contra el bloque del botón y se \
             encoge; hay que usar `Button::dropdown_menu`",
            height
        );
    })
    .unwrap();
}

fn assert_popup_closed(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) {
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        assert!(
            window.try_find(POPUP_ID).is_none(),
            "el desplegable sigue abierto"
        );
    })
    .unwrap();
}

fn assert_visible(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext, id: &'static str) {
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        assert!(
            window.try_find(id).is_some(),
            "esperaba que {:?} estuviera en pantalla",
            id
        );
    })
    .unwrap();
}

fn assert_hidden(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext, id: &'static str) {
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        assert!(
            window.try_find(id).is_none(),
            "esperaba que {:?} NO estuviera en pantalla",
            id
        );
    })
    .unwrap();
}

// =========================================================================
// ABRIR / CERRAR
// =========================================================================

/// Un click en FILE abre el desplegable, con un alto de verdad.
#[gpui_kit::gpui::test]
fn click_en_file_abre_el_desplegable(cx: &mut TestAppContext) {
    let handle = open_app(cx);
    assert_popup_closed(handle, cx);

    click(handle, cx, "menu_file");

    assert_eq!(open_menu(handle, cx), Some(0));
    assert_popup_open(handle, cx, 6);
}

/// Esc lo cierra, como cualquier menú: es el atajo que se usa de memoria.
#[gpui_kit::gpui::test]
fn escape_cierra_el_desplegable(cx: &mut TestAppContext) {
    let handle = open_app(cx);

    click(handle, cx, "menu_view");
    assert_popup_open(handle, cx, 5);

    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.press("escape", cx);
    })
    .unwrap();

    assert_eq!(open_menu(handle, cx), None);
    assert_popup_closed(handle, cx);
}

/// Click afuera cierra: el popover se descarta solo, no hace falta un overlay.
#[gpui_kit::gpui::test]
fn click_fuera_cierra_el_desplegable(cx: &mut TestAppContext) {
    let handle = open_app(cx);

    click(handle, cx, "menu_help");
    assert_popup_open(handle, cx, 1);

    // Un elemento del centro de la sesión, lejos del menú.
    click(handle, cx, "matrix_pad_0_0");

    assert_eq!(open_menu(handle, cx), None);
    assert_popup_closed(handle, cx);
}

/// Abrir otro menú deja abierto el nuevo y no el anterior.
#[gpui_kit::gpui::test]
fn cambiar_de_menu(cx: &mut TestAppContext) {
    let handle = open_app(cx);

    click(handle, cx, "menu_file");
    assert_popup_open(handle, cx, 6);

    click(handle, cx, "menu_settings");
    assert_eq!(open_menu(handle, cx), Some(3));
    assert_popup_open(handle, cx, 2);
}

// =========================================================================
// LAS ACCIONES
// =========================================================================

/// Los índices son parte del contrato de los tests: cada ítem está donde dice
/// [`hikaru_gui::views::menu_bar::menu_items`].
#[gpui_kit::gpui::test]
fn un_item_cambia_el_estado_y_cierra_el_menu(cx: &mut TestAppContext) {
    let handle = open_app(cx);
    set_state(handle, cx, |state| state.show_dsp_rack = false);

    click(handle, cx, "menu_view");
    click_item(handle, cx, 4); // 🎚 DSP Rack (F10)

    assert!(set_state(handle, cx, |s| s.show_dsp_rack));
    assert_eq!(open_menu(handle, cx), None);
    assert_popup_closed(handle, cx);
}

/// `VIEW > Playlist / Timeline` cambia de modo, igual que el botón OPENSTUDIO
/// de la barra.
#[gpui_kit::gpui::test]
fn view_playlist_cambia_a_openstudio(cx: &mut TestAppContext) {
    let handle = open_app(cx);

    click(handle, cx, "menu_view");
    click_item(handle, cx, 2); // 🎹 Playlist / Timeline

    assert_eq!(set_state(handle, cx, |s| s.mode), AppMode::OpenStudio);
}

/// `SETTINGS` abre sus paneles: que el `is_open` quede en true prueba que el
/// ítem no es un no-op disfrazado.
#[gpui_kit::gpui::test]
fn settings_abre_el_panel_de_audio(cx: &mut TestAppContext) {
    let handle = open_app(cx);
    set_state(handle, cx, |state| state.audio_settings_state.is_open = false);

    click(handle, cx, "menu_settings");
    click_item(handle, cx, 0); // 🔊 Audio Setup...

    assert!(set_state(handle, cx, |s| s.audio_settings_state.is_open));
}

/// El rótulo de About depende del modo, y el popover lo arma al abrir: si se
/// armara en el render, quedaría clavado en el modo del frame anterior.
#[gpui_kit::gpui::test]
fn about_cambia_el_rotulo_segun_el_modo(cx: &mut TestAppContext) {
    let handle = open_app(cx);

    click(handle, cx, "menu_help");
    let live = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            window.within(POPUP_ID).find(0usize).label().map(str::to_string)
        })
        .unwrap();
    assert!(
        live.as_deref().unwrap_or_default().contains("OpenLive"),
        "rótulo en OpenLive: {:?}",
        live
    );
    cx.update_window(handle.into(), |_, window, cx| {
        window.press("escape", cx);
    })
    .unwrap();

    set_state(handle, cx, |state| {
        state.mode = AppMode::OpenStudio;
        state.openstudio_view = OpenStudioView::Playlist;
    });

    click(handle, cx, "menu_help");
    let studio = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            window.within(POPUP_ID).find(0usize).label().map(str::to_string)
        })
        .unwrap();
    assert!(
        studio.as_deref().unwrap_or_default().contains("OpenStudio"),
        "rótulo en OpenStudio: {:?}",
        studio
    );
}

// =========================================================================
// EL PUNTO DE "SIN GUARDAR"
// =========================================================================

/// Con cambios sin guardar, FILE muestra el punto y al guardar se va. No es
/// decoración: es lo que avisa que el trabajo está en memoria.
#[gpui_kit::gpui::test]
fn file_marca_los_cambios_sin_guardar(cx: &mut TestAppContext) {
    let handle = open_app(cx);

    assert_hidden(handle, cx, "menu_file_dirty");

    set_state(handle, cx, |state| state.project_dirty = true);
    assert_visible(handle, cx, "menu_file_dirty");

    set_state(handle, cx, |state| state.project_dirty = false);
    assert_hidden(handle, cx, "menu_file_dirty");
}