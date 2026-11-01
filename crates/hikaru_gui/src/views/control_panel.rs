// crates/hikaru_gui/src/views/control_panel.rs
//
// Panel de Control / Dashboard estilo Bitwig Studio.
// Centraliza Audio, Plugins externos, Servidores MCP y Acerca de Hikaru.

use gpui_kit::component::button::Button;
use gpui_kit::component::label::Label;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, AppMode, HikaruApp};
use crate::version::VERSION;
use crate::views::branding::energy_core_logo_img;
use crate::views::modal::{self, ModalId, ModalRoot};
use crate::views::audio_settings::AudioBackend;

const BASE_LEFT: f32 = 60.0;
const BASE_TOP: f32 = 60.0;
const PANEL_WIDTH: f32 = 820.0;
const PANEL_HEIGHT: f32 = 560.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlPanelTab {
    Audio,
    Plugins,
    Mcp,
    About,
}

impl Default for ControlPanelTab {
    fn default() -> Self {
        Self::Audio
    }
}

pub struct ControlPanelState {
    pub is_open: bool,
    pub active_tab: ControlPanelTab,
}

impl Default for ControlPanelState {
    fn default() -> Self {
        Self {
            is_open: false,
            active_tab: ControlPanelTab::Audio,
        }
    }
}

pub fn render(cx: &mut Context<HikaruApp>) -> AnyElement {
    let app = state(cx).read(cx);
    let tab = app.control_panel_state.active_tab;
    let drag = app.modal_drags.get(ModalId::ControlPanel);
    let offset = drag.offset();
    let dragging = drag.is_dragging();

    let (left, top) = modal::modal_position(BASE_LEFT, BASE_TOP, offset);

    let panel = v_flex()
        .id("control_panel")
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(PANEL_WIDTH))
        .h(px(PANEL_HEIGHT))
        .bg(rgb(0x1E1E22))
        .border_1()
        .border_color(rgb(0x383C4A))
        .rounded(px(8.0))
        .shadow_lg()
        .overflow_hidden()
        // Encabezado con pestaña y botón de cierre
        .child(render_header(tab, dragging, cx))
        // Cuerpo principal: Barra de navegación lateral + Contenido
        .child(
            h_flex()
                .flex_1()
                .w_full()
                .child(render_sidebar(tab, cx))
                .child(
                    div()
                        .flex_1()
                        .h_full()
                        .bg(rgb(0x23242B))
                        .p(px(20.0))
                        .child(render_tab_content(tab, cx)),
                ),
        )
        .absorb_clicks()
        .test_support()
        .into_any_element();

    modal::overlay("control_panel_backdrop", panel)
}

fn render_header(
    _tab: ControlPanelTab,
    dragging: bool,
    _cx: &mut Context<HikaruApp>,
) -> impl IntoElement {
    modal::drag_handle(
        h_flex()
            .id("control_panel_header")
            .w_full()
            .h(px(42.0))
            .bg(rgb(0x18191E))
            .border_b_1()
            .border_color(rgb(0x2E313C))
            .px(px(14.0))
            .items_center()
            .justify_between()
            .child(
                h_flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(energy_core_logo_img("control_panel_logo", 22.0))
                    .child(
                        Label::new("HIKARU DASHBOARD · PANEL DE CONTROL")
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0xE0E0E0)),
                    ),
            )
            .child(
                Button::new("control_panel_close")
                    .rounded(gpui_kit::component::button::ButtonRounded::None)
                    .label("✕")
                    .compact()
                    .on_click(move |_, _, cx| {
                        let st = state(cx);
                        st.update(cx, |state, cx| {
                            state.control_panel_state.is_open = false;
                            state.modal_drags.reset(ModalId::ControlPanel);
                            cx.notify();
                        });
                    }),
            ),
        ModalId::ControlPanel,
        dragging,
    )
}

fn render_sidebar(current_tab: ControlPanelTab, _cx: &mut Context<HikaruApp>) -> impl IntoElement {
    v_flex()
        .w(px(190.0))
        .h_full()
        .bg(rgb(0x18191E))
        .border_r_1()
        .border_color(rgb(0x2E313C))
        .p(px(8.0))
        .gap(px(4.0))
        .child(tab_button("Audio & Hardware", ControlPanelTab::Audio, current_tab))
        .child(tab_button("Plugins VST3 / CLAP", ControlPanelTab::Plugins, current_tab))
        .child(tab_button("MCP & AI Agents", ControlPanelTab::Mcp, current_tab))
        .child(div().flex_1())
        .child(tab_button("Acerca de Hikaru", ControlPanelTab::About, current_tab))
}

fn tab_button(label: &'static str, tab: ControlPanelTab, current_tab: ControlPanelTab) -> impl IntoElement {
    let is_active = tab == current_tab;
    let bg = if is_active { rgb(0x2C2F3B) } else { rgb(0x18191E) };
    let text_color = if is_active { rgb(0x00BCD4) } else { rgb(0xA0A0A5) };

    div()
        .id(ElementId::Name(format!("tab_{label}").into()))
        .w_full()
        .px(px(12.0))
        .py(px(8.0))
        .rounded(px(4.0))
        .bg(bg)
        .cursor_pointer()
        .items_center()
        .hover(|style| style.bg(rgb(0x252732)))
        .on_click(move |_, _, cx| {
            let st = state(cx);
            st.update(cx, |state, cx| {
                state.control_panel_state.active_tab = tab;
                cx.notify();
            });
        })
        .child(
            Label::new(label)
                .text_xs()
                .font_weight(if is_active { FontWeight::BOLD } else { FontWeight::NORMAL })
                .text_color(text_color),
        )
}

fn render_tab_content(tab: ControlPanelTab, cx: &mut Context<HikaruApp>) -> AnyElement {
    match tab {
        ControlPanelTab::Audio => render_audio_tab(cx).into_any_element(),
        ControlPanelTab::Plugins => render_plugins_tab(cx).into_any_element(),
        ControlPanelTab::Mcp => render_mcp_tab(cx).into_any_element(),
        ControlPanelTab::About => render_about_tab(cx).into_any_element(),
    }
}

fn render_audio_tab(cx: &mut Context<HikaruApp>) -> impl IntoElement {
    let app = state(cx).read(cx);
    let st = &app.audio_settings_state;
    let backend = st.selected_backend.clone();
    let device = st.selected_device.clone();
    let sample_rate = st.sample_rate;
    let buffer_size = st.buffer_size;

    v_flex()
        .w_full()
        .gap(px(16.0))
        .child(
            Label::new("Configuración de Motor de Audio y Dispositivos")
                .text_base()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0xFFFFFF)),
        )
        // Backend
        .child(
            v_flex()
                .gap(px(6.0))
                .child(Label::new("Driver de Audio / Backend:").text_xs().text_color(rgb(0xAAAAAA)))
                .child(
                    h_flex()
                        .gap(px(8.0))
                        .items_center()
                        .child(
                            div()
                                .px(px(10.0))
                                .py(px(6.0))
                                .bg(rgb(0x18191E))
                                .border_1()
                                .border_color(rgb(0x383C4A))
                                .rounded(px(4.0))
                                .child(Label::new(format!("{:?}", backend)).text_xs()),
                        )
                        .child(
                            Button::new("cp_switch_backend")
                                .rounded(gpui_kit::component::button::ButtonRounded::None)
                                .label("Cambiar Backend")
                                .compact()
                                .on_click(move |_, _, cx| {
                                    let st = state(cx);
                                    st.update(cx, |state, cx| {
                                        state.audio_settings_state.selected_backend =
                                            match state.audio_settings_state.selected_backend {
                                                AudioBackend::HikaruNative => AudioBackend::PipeWire,
                                                AudioBackend::PipeWire => AudioBackend::Jack,
                                                AudioBackend::Jack => AudioBackend::Alsa,
                                                AudioBackend::Alsa => AudioBackend::PulseAudio,
                                                AudioBackend::PulseAudio => AudioBackend::HikaruNative,
                                            };
                                        cx.notify();
                                    });
                                }),
                        ),
                ),
        )
        // Dispositivo
        .child(
            v_flex()
                .gap(px(6.0))
                .child(Label::new("Dispositivo de Salida:").text_xs().text_color(rgb(0xAAAAAA)))
                .child(
                    h_flex()
                        .gap(px(8.0))
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .px(px(10.0))
                                .py(px(6.0))
                                .bg(rgb(0x18191E))
                                .border_1()
                                .border_color(rgb(0x383C4A))
                                .rounded(px(4.0))
                                .child(Label::new(device).text_xs()),
                        )
                        .child(
                            Button::new("cp_switch_device")
                                .rounded(gpui_kit::component::button::ButtonRounded::None)
                                .label("Siguiente")
                                .compact()
                                .on_click(move |_, _, cx| {
                                    let st = state(cx);
                                    st.update(cx, |state, cx| {
                                        let devs = &state.audio_settings_state.available_devices;
                                        if let Some(pos) = devs.iter().position(|d| d == &state.audio_settings_state.selected_device) {
                                            let next = devs[(pos + 1) % devs.len()].clone();
                                            state.audio_settings_state.selected_device = next;
                                        }
                                        cx.notify();
                                    });
                                }),
                        ),
                ),
        )
        // Frecuencia y Buffer
        .child(
            h_flex()
                .gap(px(24.0))
                .child(
                    v_flex()
                        .gap(px(6.0))
                        .child(Label::new("Frecuencia de Muestreo:").text_xs().text_color(rgb(0xAAAAAA)))
                        .child(
                            Button::new("cp_sample_rate")
                                .rounded(gpui_kit::component::button::ButtonRounded::None)
                                .label(format!("{sample_rate} Hz"))
                                .compact()
                                .on_click(move |_, _, cx| {
                                    let st = state(cx);
                                    st.update(cx, |state, cx| {
                                        state.audio_settings_state.sample_rate =
                                            match state.audio_settings_state.sample_rate {
                                                44100 => 48000,
                                                48000 => 96000,
                                                96000 => 192000,
                                                _ => 44100,
                                            };
                                        cx.notify();
                                    });
                                }),
                        ),
                )
                .child(
                    v_flex()
                        .gap(px(6.0))
                        .child(Label::new("Tamaño de Buffer:").text_xs().text_color(rgb(0xAAAAAA)))
                        .child(
                            Button::new("cp_buffer_size")
                                .rounded(gpui_kit::component::button::ButtonRounded::None)
                                .label(format!("{buffer_size} samples"))
                                .compact()
                                .on_click(move |_, _, cx| {
                                    let st = state(cx);
                                    st.update(cx, |state, cx| {
                                        state.audio_settings_state.buffer_size =
                                            match state.audio_settings_state.buffer_size {
                                                64 => 128,
                                                128 => 256,
                                                256 => 512,
                                                512 => 1024,
                                                _ => 64,
                                            };
                                        cx.notify();
                                    });
                                }),
                        ),
                ),
        )
}

fn render_plugins_tab(cx: &mut Context<HikaruApp>) -> impl IntoElement {
    let app = state(cx).read(cx);
    let plugins_count = app.plugin_settings_state.discovered_plugins.len();
    let is_scanning = app.plugin_settings_state.is_scanning;
    let paths = app.plugin_settings_state.custom_paths.clone();

    v_flex()
        .w_full()
        .gap(px(16.0))
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .child(
                    Label::new("Explorador y Sandbox de Plugins (VST3 / CLAP)")
                        .text_base()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0xFFFFFF)),
                )
                .child(
                    Button::new("cp_rescan_plugins")
                        .rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label(if is_scanning { "Escaneando..." } else { "Re-escanear Plugins" })
                        .compact()
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            st.update(cx, |state, cx| {
                                state.plugin_settings_state.is_scanning = true;
                                cx.notify();
                            });
                        }),
                ),
        )
        .child(
            v_flex()
                .gap(px(6.0))
                .child(Label::new("Rutas de búsqueda configuradas:").text_xs().text_color(rgb(0xAAAAAA)))
                .children(paths.into_iter().map(|p| {
                    div()
                        .px(px(8.0))
                        .py(px(4.0))
                        .bg(rgb(0x18191E))
                        .rounded(px(3.0))
                        .child(Label::new(p).text_xs())
                })),
        )
        .child(
            div()
                .p(px(10.0))
                .bg(rgb(0x18191E))
                .rounded(px(4.0))
                .child(Label::new(format!("Plugins instalados detectados: {plugins_count}")).text_xs()),
        )
}

fn render_mcp_tab(cx: &mut Context<HikaruApp>) -> impl IntoElement {
    let app = state(cx).read(cx);
    let ports_count = app.mcp_port_settings_state.config.ports.len();
    let enabled = app.mcp_port_settings_state.config.enabled;

    v_flex()
        .w_full()
        .gap(px(16.0))
        .child(
            Label::new("Hikaru Model Context Protocol (MCP) Server")
                .text_base()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0xFFFFFF)),
        )
        .child(
            Label::new("Permite a agentes de IA locales y remotos (Antigravity, Claude, Cursor) controlar sesiones, automatizar mezclas y consultar telemetría.")
                .text_xs()
                .text_color(rgb(0xAAAAAA)),
        )
        .child(
            h_flex()
                .gap(px(16.0))
                .items_center()
                .child(
                    Label::new(format!("Estado del Servidor: {}", if enabled { "ACTIVO (Escuchando)" } else { "DETENIDO" }))
                        .text_xs()
                        .text_color(if enabled { rgb(0x4CAF50) } else { rgb(0xF44336) }),
                )
                .child(
                    Button::new("cp_toggle_mcp")
                        .rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label(if enabled { "Desactivar MCP" } else { "Iniciar MCP" })
                        .compact()
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            st.update(cx, |state, cx| {
                                state.mcp_port_settings_state.config.enabled = !state.mcp_port_settings_state.config.enabled;
                                cx.notify();
                            });
                        }),
                ),
        )
        .child(
            div()
                .px(px(10.0))
                .py(px(6.0))
                .bg(rgb(0x18191E))
                .rounded(px(4.0))
                .child(Label::new(format!("Slots de puerto configurados: {ports_count}")).text_xs()),
        )
}

fn render_about_tab(cx: &mut Context<HikaruApp>) -> impl IntoElement {
    let app = state(cx).read(cx);
    let mode = app.mode;
    let title = match mode {
        AppMode::OpenLive => "Hikaru OpenLive",
        AppMode::OpenStudio => "Hikaru OpenStudio",
    };

    v_flex()
        .w_full()
        .gap(px(14.0))
        .items_center()
        .child(energy_core_logo_img("about_tab_logo", 80.0))
        .child(Label::new(title).text_lg().font_weight(FontWeight::BOLD).text_color(rgb(0xFFFFFF)))
        .child(Label::new("An Advanced Digital Audio Workstation for UNIX sysadmins.").text_xs())
        .child(Label::new("Copyright © Hikaru Corporation - 2026").text_xs())
        .child(Label::new(format!("Versión actual: {}", VERSION)).text_xs().text_color(rgb(0x00BCD4)))
        .child(Label::new("Licencia: GNU AGPLv3 (Software Libre)").text_xs())
}
