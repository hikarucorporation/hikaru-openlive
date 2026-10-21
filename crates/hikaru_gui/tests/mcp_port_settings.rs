// crates/hikaru_gui/tests/mcp_port_settings.rs
//
// Tests del panel `Hikaru MCP Port Settings...`.
//
// El panel es la puerta de entrada de los agentes de IA al DAW: si la lista
// de puertos sale vacía, o si el botón de agregar no agrega, el DAW queda
// inaccesible para el agente y el fallo se descubre tarde (cuando el agente
// intenta conectarse y no hay a quién).

use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Arc;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{px, size, AppContext, Context, TestAppContext, Window};

use hikaru_gui::app::{AppState, HikaruApp};
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

/// Abre el panel desde el estado (sin pasar por el menú: eso lo testean los
/// tests de la barra de menús).
fn open_panel(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) {
    set_state(handle, cx, |state| state.mcp_port_settings_state.open());
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
    })
    .unwrap();
}

// =========================================================================
// EL PANEL
// =========================================================================

/// El panel no se monta mientras está cerrado: se dibuja vacío.
#[gpui_kit::gpui::test]
fn el_panel_no_se_monta_si_esta_cerrado(cx: &mut TestAppContext) {
    let handle = open_app(cx);

    assert_hidden(handle, cx, "mcp_port_settings");
}

/// Abierto, el panel se dibuja con su lista de puertos.
#[gpui_kit::gpui::test]
fn el_panel_se_abre_con_la_lista_de_puertos(cx: &mut TestAppContext) {
    let handle = open_app(cx);

    open_panel(handle, cx);

    assert_visible(handle, cx, "mcp_port_settings");
    assert_visible(handle, cx, "mcp_port_select_1");
    assert_visible(handle, cx, "mcp_port_select_10");
    assert_visible(handle, cx, "mcp_add_port");
}

/// La sección de transporte arranca plegada: `Advanced Settings...` la
/// despliega.
#[gpui_kit::gpui::test]
fn advanced_settings_se_despliega(cx: &mut TestAppContext) {
    let handle = open_app(cx);
    open_panel(handle, cx);

    assert_hidden(handle, cx, "mcp_transport_cycle");

    click(handle, cx, "mcp_advanced_toggle");

    assert_visible(handle, cx, "mcp_transport_cycle");
}

// =========================================================================
// LOS PUERTOS
// =========================================================================

/// De fábrica hay diez slots: del 1 al 10.
#[gpui_kit::gpui::test]
fn hay_diez_puertos_por_defecto(cx: &mut TestAppContext) {
    let handle = open_app(cx);

    let count = set_state(handle, cx, |state| state.mcp_port_settings_state.config.port_count());
    assert_eq!(count, 10);
}

/// `Add MCP Port...` agrega el slot 11.
#[gpui_kit::gpui::test]
fn add_mcp_port_agrega_un_slot_mas(cx: &mut TestAppContext) {
    let handle = open_app(cx);
    open_panel(handle, cx);

    click(handle, cx, "mcp_add_port");

    let (count, last_id) = set_state(handle, cx, |state| {
        let config = &state.mcp_port_settings_state.config;
        (config.port_count(), config.ports.last().unwrap().id)
    });
    assert_eq!(count, 11);
    assert_eq!(last_id, 11);
    assert_visible(handle, cx, "mcp_port_select_11");
}

/// El botón `✕` borra el slot.
#[gpui_kit::gpui::test]
fn se_puede_borrar_un_puerto(cx: &mut TestAppContext) {
    let handle = open_app(cx);
    open_panel(handle, cx);

    click(handle, cx, "mcp_port_remove_10");

    let count = set_state(handle, cx, |state| state.mcp_port_settings_state.config.port_count());
    assert_eq!(count, 9);
    assert_hidden(handle, cx, "mcp_port_select_10");
}

/// El transporte del slot cambia al cyclearlo.
#[gpui_kit::gpui::test]
fn el_transporte_cambia_al_cyclearlo(cx: &mut TestAppContext) {
    let handle = open_app(cx);
    open_panel(handle, cx);

    // El slot 1 arranca en Unix Socket.
    let before = set_state(handle, cx, |state| {
        state.mcp_port_settings_state.config.slot(1).unwrap().transport.label()
    });
    assert_eq!(before, "Unix Socket");

    click(handle, cx, "mcp_advanced_toggle");
    click(handle, cx, "mcp_transport_cycle");

    let after = set_state(handle, cx, |state| {
        state.mcp_port_settings_state.config.slot(1).unwrap().transport.label()
    });
    assert_eq!(after, "Stdio");
}

/// El interruptor habilita y deshabilita el slot.
#[gpui_kit::gpui::test]
fn el_slot_se_puede_habilitar(cx: &mut TestAppContext) {
    let handle = open_app(cx);
    open_panel(handle, cx);

    click(handle, cx, "mcp_advanced_toggle");
    click(handle, cx, "mcp_port_toggle_selected");

    let enabled = set_state(handle, cx, |state| {
        state.mcp_port_settings_state.config.slot(1).unwrap().enabled
    });
    assert!(enabled);
}

/// Cerrar el panel lo saca de la pantalla.
#[gpui_kit::gpui::test]
fn cerrar_el_panel_lo_saca_de_la_pantalla(cx: &mut TestAppContext) {
    let handle = open_app(cx);
    open_panel(handle, cx);
    assert_visible(handle, cx, "mcp_port_settings");

    click(handle, cx, "mcp_close");

    assert!(!set_state(handle, cx, |state| state.mcp_port_settings_state.is_open));
    assert_hidden(handle, cx, "mcp_port_settings");
}
