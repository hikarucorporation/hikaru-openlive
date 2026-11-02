// Hikaru OpenLive - Footer / Status Bar
// GNU AGPLv3
// crates/hikaru_gui/src/views/footer.rs

use gpui_kit::component::*;
use gpui_kit::component::label::Label;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, update_state, AppMode, HikaruApp, OpenLiveView, OpenStudioView};
use crate::theme;

/// Pestañas de vista principales (`ARRANGE`, `MIX`, `EDIT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusViewTab {
    Arrange,
    Mix,
    Edit,
}

// =========================================================================
// HELPERS DE ICONOGRAFÍA MILIMÉTRICA ESTILO BITWIG STUDIO
// =========================================================================

/// Separador vertical delgado (1px) entre grupos de la barra inferior.
fn separator_v() -> impl IntoElement {
    div()
        .w(px(1.0))
        .h(px(14.0))
        .bg(rgb(0x404040))
        .mx(px(3.0))
        .flex_shrink_0()
}

/// Icono `i` de ayuda e información contextual.
fn icon_info(fg: Rgba) -> impl IntoElement {
    div()
        .w(px(12.0))
        .h(px(14.0))
        .relative()
        .flex_shrink_0()
        // Punto superior
        .child(
            div()
                .absolute()
                .top(px(1.0))
                .left(px(5.0))
                .w(px(2.0))
                .h(px(2.0))
                .rounded(px(0.5))
                .bg(fg),
        )
        // Vástago inferior
        .child(
            div()
                .absolute()
                .top(px(4.5))
                .left(px(5.0))
                .w(px(2.0))
                .h(px(7.5))
                .rounded(px(0.5))
                .bg(fg),
        )
}

/// Icono de Detail Editor (Notas MIDI / Edición de Clips).
/// Cuatro notas/bloques escalonados representando edición en piano roll.
fn icon_detail_editor(fg: Rgba) -> impl IntoElement {
    div()
        .w(px(14.0))
        .h(px(14.0))
        .relative()
        .flex_shrink_0()
        // Nota 1 (superior izquierda)
        .child(
            div()
                .absolute()
                .left(px(1.0))
                .top(px(2.0))
                .w(px(3.5))
                .h(px(2.5))
                .rounded(px(0.5))
                .bg(fg),
        )
        // Nota 2 (central superior)
        .child(
            div()
                .absolute()
                .left(px(5.0))
                .top(px(5.5))
                .w(px(3.5))
                .h(px(2.5))
                .rounded(px(0.5))
                .bg(fg),
        )
        // Nota 3 (derecha superior)
        .child(
            div()
                .absolute()
                .left(px(9.0))
                .top(px(2.0))
                .w(px(3.5))
                .h(px(2.5))
                .rounded(px(0.5))
                .bg(fg),
        )
        // Nota 4 (derecha inferior)
        .child(
            div()
                .absolute()
                .left(px(9.0))
                .top(px(9.0))
                .w(px(3.5))
                .h(px(2.5))
                .rounded(px(0.5))
                .bg(fg),
        )
}

/// Icono de Automatización (dos nodos conectados por una envolvente curva).
fn icon_automation(fg: Rgba) -> impl IntoElement {
    div()
        .w(px(14.0))
        .h(px(14.0))
        .relative()
        .flex_shrink_0()
        // Nodo inferior izquierdo
        .child(
            div()
                .absolute()
                .left(px(1.0))
                .bottom(px(1.5))
                .w(px(4.0))
                .h(px(4.0))
                .rounded_full()
                .border_1()
                .border_color(fg),
        )
        // Trazo inferior horizontal
        .child(
            div()
                .absolute()
                .left(px(5.0))
                .bottom(px(2.8))
                .w(px(2.5))
                .h(px(1.4))
                .bg(fg),
        )
        // Trazo vertical / ascendente
        .child(
            div()
                .absolute()
                .left(px(6.5))
                .top(px(5.5))
                .w(px(1.4))
                .h(px(4.0))
                .bg(fg),
        )
        // Trazo superior horizontal
        .child(
            div()
                .absolute()
                .right(px(5.0))
                .top(px(2.8))
                .w(px(2.5))
                .h(px(1.4))
                .bg(fg),
        )
        // Nodo superior derecho
        .child(
            div()
                .absolute()
                .right(px(1.0))
                .top(px(1.5))
                .w(px(4.0))
                .h(px(4.0))
                .rounded_full()
                .border_1()
                .border_color(fg),
        )
}

/// Icono de Panel de Dispositivos / DSP Rack (chasis modular Bitwig).
fn icon_device(fg: Rgba) -> impl IntoElement {
    div()
        .w(px(14.0))
        .h(px(14.0))
        .relative()
        .flex_shrink_0()
        .child(
            div()
                .absolute()
                .top(px(1.0))
                .left(px(0.5))
                .w(px(13.0))
                .h(px(12.0))
                .rounded(px(2.0))
                .border_1()
                .border_color(fg)
                .overflow_hidden()
                // Banda lateral izquierda de la tarjeta/dispositivo
                .child(
                    div()
                        .w(px(3.5))
                        .h_full()
                        .bg(fg),
                ),
        )
}

/// Icono de Mezclador (3 tiras de fader con deslizadores en distintas posiciones).
fn icon_mixer(fg: Rgba) -> impl IntoElement {
    div()
        .w(px(15.0))
        .h(px(14.0))
        .relative()
        .flex_shrink_0()
        // Guía vertical 1 (izq)
        .child(
            div()
                .absolute()
                .left(px(2.5))
                .top(px(1.0))
                .w(px(1.0))
                .h(px(12.0))
                .bg(fg),
        )
        // Slider 1 (medio)
        .child(
            div()
                .absolute()
                .left(px(0.5))
                .top(px(4.5))
                .w(px(5.0))
                .h(px(2.2))
                .rounded(px(0.5))
                .bg(fg),
        )
        // Guía vertical 2 (centro)
        .child(
            div()
                .absolute()
                .left(px(7.5))
                .top(px(1.0))
                .w(px(1.0))
                .h(px(12.0))
                .bg(fg),
        )
        // Slider 2 (abajo)
        .child(
            div()
                .absolute()
                .left(px(5.5))
                .top(px(8.5))
                .w(px(5.0))
                .h(px(2.2))
                .rounded(px(0.5))
                .bg(fg),
        )
        // Guía vertical 3 (der)
        .child(
            div()
                .absolute()
                .left(px(12.5))
                .top(px(1.0))
                .w(px(1.0))
                .h(px(12.0))
                .bg(fg),
        )
        // Slider 3 (arriba)
        .child(
            div()
                .absolute()
                .left(px(10.5))
                .top(px(2.5))
                .w(px(5.0))
                .h(px(2.2))
                .rounded(px(0.5))
                .bg(fg),
        )
}

/// Icono de Lupa / Explorador de archivos (Browser Panel).
fn icon_search(fg: Rgba) -> impl IntoElement {
    div()
        .w(px(14.0))
        .h(px(14.0))
        .relative()
        .flex_shrink_0()
        // Aro de la lupa
        .child(
            div()
                .absolute()
                .top(px(1.0))
                .left(px(1.0))
                .w(px(8.5))
                .h(px(8.5))
                .rounded_full()
                .border_2()
                .border_color(fg),
        )
        // Mango diagonal
        .child(
            div()
                .absolute()
                .bottom(px(0.5))
                .right(px(0.5))
                .w(px(4.0))
                .h(px(2.0))
                .rounded(px(0.5))
                .bg(fg),
        )
}

/// Icono de Hoja de Proyecto / Metadata.
fn icon_project(fg: Rgba) -> impl IntoElement {
    div()
        .w(px(13.0))
        .h(px(14.0))
        .relative()
        .flex_shrink_0()
        // Hoja base
        .child(
            div()
                .absolute()
                .top(px(1.0))
                .left(px(1.0))
                .w(px(10.0))
                .h(px(12.0))
                .rounded(px(1.0))
                .border_1()
                .border_color(fg)
                .overflow_hidden()
                // Líneas sutiles de texto dentro de la hoja
                .child(
                    div()
                        .absolute()
                        .top(px(4.0))
                        .left(px(2.0))
                        .w(px(6.0))
                        .h(px(1.0))
                        .bg(fg),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(7.0))
                        .left(px(2.0))
                        .w(px(6.0))
                        .h(px(1.0))
                        .bg(fg),
                ),
        )
}

/// Icono de Monitor de Estudio / Altavoz (Studio Panel).
fn icon_studio_monitor(fg: Rgba) -> impl IntoElement {
    div()
        .w(px(14.0))
        .h(px(15.0))
        .relative()
        .flex_shrink_0()
        // Caja acústica exterior con bordes curvados
        .child(
            div()
                .absolute()
                .top(px(0.5))
                .left(px(1.0))
                .w(px(11.5))
                .h(px(14.0))
                .rounded(px(2.5))
                .border_1()
                .border_color(fg)
                // Tweeter superior
                .child(
                    div()
                        .absolute()
                        .top(px(2.0))
                        .left(px(4.5))
                        .w(px(2.5))
                        .h(px(2.5))
                        .rounded_full()
                        .bg(fg),
                )
                // Woofer inferior
                .child(
                    div()
                        .absolute()
                        .bottom(px(1.8))
                        .left(px(2.7))
                        .w(px(6.0))
                        .h(px(6.0))
                        .rounded_full()
                        .border_1()
                        .border_color(fg)
                        .child(
                            div()
                                .absolute()
                                .top(px(1.5))
                                .left(px(1.5))
                                .w(px(2.0))
                                .h(px(2.0))
                                .rounded_full()
                                .bg(fg),
                        ),
                ),
        )
}

/// Icono de Mano / Mapeos de Controladores (Mappings Panel).
fn icon_hand(fg: Rgba) -> impl IntoElement {
    div()
        .w(px(14.0))
        .h(px(15.0))
        .relative()
        .flex_shrink_0()
        // Dedo índice apuntando arriba
        .child(
            div()
                .absolute()
                .top(px(1.0))
                .left(px(4.0))
                .w(px(2.5))
                .h(px(6.0))
                .rounded(px(1.0))
                .bg(fg),
        )
        // Palma y dedos agrupados
        .child(
            div()
                .absolute()
                .top(px(5.5))
                .left(px(2.0))
                .w(px(8.5))
                .h(px(8.0))
                .rounded(px(1.5))
                .bg(fg),
        )
}

/// Icono de Teclado Virtual (Piano Keys con 3 teclas blancas y 2 negras).
fn icon_keyboard(fg: Rgba) -> impl IntoElement {
    div()
        .w(px(15.0))
        .h(px(14.0))
        .relative()
        .flex_shrink_0()
        // Fondo de teclas blancas
        .child(
            div()
                .absolute()
                .top(px(1.0))
                .left(px(0.5))
                .w(px(14.0))
                .h(px(12.0))
                .rounded(px(1.0))
                .bg(fg)
                .relative()
                .overflow_hidden()
                // Tecla negra 1
                .child(
                    div()
                        .absolute()
                        .top(px(0.0))
                        .left(px(3.0))
                        .w(px(2.2))
                        .h(px(7.0))
                        .bg(rgb(0x222222)),
                )
                // Tecla negra 2
                .child(
                    div()
                        .absolute()
                        .top(px(0.0))
                        .left(px(8.5))
                        .w(px(2.2))
                        .h(px(7.0))
                        .bg(rgb(0x222222)),
                )
                // Separador inferior blanco 1
                .child(
                    div()
                        .absolute()
                        .bottom(px(0.0))
                        .left(px(4.5))
                        .w(px(0.8))
                        .h(px(5.0))
                        .bg(rgb(0x222222)),
                )
                // Separador inferior blanco 2
                .child(
                    div()
                        .absolute()
                        .bottom(px(0.0))
                        .left(px(9.2))
                        .w(px(0.8))
                        .h(px(5.0))
                        .bg(rgb(0x222222)),
                ),
        )
}

// =========================================================================
// COMPONENTE PRINCIPAL RENDER
// =========================================================================

pub fn render(cx: &mut Context<HikaruApp>) -> impl IntoElement {
    let app = state(cx).read(cx);

    let show_clip_editor = app.clip_editor.show_editor;
    let show_piano_roll = app.show_piano_roll;
    let show_dsp_rack = app.show_dsp_rack;
    let show_explorer = app.show_explorer;
    let show_keyboard = app.panel_layout.show_keyboard;
    let show_mixer_panel = app.panel_layout.show_mixer;
    let control_panel_open = app.control_panel_state.is_open;
    let mcp_port_settings_open = app.mcp_port_settings_state.is_open;
    let show_about = app.show_about;

    let active_mode = app.mode;
    let openlive_view = app.openlive_view;
    let openstudio_view = app.openstudio_view;

    let sr = app.transport.sample_rate.get() as u32;
    let buffer_size = app.audio_settings_state.buffer_size;
    let cpu_usage = app.cpu_usage.clamp(0.0, 1.0);
    let context_help = app.footer_context_help.clone();

    // Determinación de la pestaña activa inferior
    let active_tab = if show_clip_editor || show_piano_roll {
        StatusViewTab::Edit
    } else {
        match active_mode {
            AppMode::OpenLive => match openlive_view {
                OpenLiveView::SessionMatrix => StatusViewTab::Arrange,
                OpenLiveView::ArrangerView => StatusViewTab::Mix,
            },
            AppMode::OpenStudio => match openstudio_view {
                OpenStudioView::Playlist => StatusViewTab::Arrange,
                OpenStudioView::ArrangerMixer => StatusViewTab::Mix,
            },
        }
    };

    let _ = app;

    // Colores de acento y neutros estilo Bitwig Studio
    let accent_orange = rgb(theme::DAW_ACCENT_ORANGE);
    let normal_fg = rgb(0x8F969E);
    let border_col = rgb(0x383838);

    h_flex()
        .id("footer_bar")
        .h(px(26.0))
        .w_full()
        .bg(rgb(0x242424))
        .border_t_1()
        .border_color(border_col)
        .items_center()
        .px(px(6.0))
        .gap(px(2.0))
        .overflow_hidden()
        // Reset de ayuda contextual al mover el mouse en el fondo neutro del footer
        .on_mouse_move(|_, _, cx| {
            update_state(cx, |s| {
                if s.footer_context_help.is_some() {
                    s.footer_context_help = None;
                }
            });
        })

        // ─────────────────────────────────────────────────────────────────
        // 1. GRUPO IZQUIERDO: [i] | ARRANGE  MIX  EDIT | [4 PANELES INFERIORES]
        // ─────────────────────────────────────────────────────────────────

        // Botón [i] Info / Ayuda contextual
        .child(
            div()
                .id("footer_btn_info")
                .px(px(4.0))
                .py(px(2.0))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0x303030)).rounded(px(2.0)))
                .on_mouse_move(|_, _, cx| {
                    update_state(cx, |s| {
                        let cue = Some(("CLICK".into(), "Toggle Context Help".into()));
                        if s.footer_context_help != cue {
                            s.footer_context_help = cue;
                        }
                    });
                })
                .on_click(|_, _, cx| {
                    update_state(cx, |s| {
                        s.show_explorer = !s.show_explorer;
                    });
                })
                .child(icon_info(normal_fg)),
        )

        .child(separator_v())

        // Selector ARRANGE
        .child(
            div()
                .id("footer_tab_arrange")
                .px(px(6.0))
                .py(px(2.0))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0x2D2D2D)).rounded(px(2.0)))
                .on_mouse_move(|_, _, cx| {
                    update_state(cx, |s| {
                        let cue = Some(("CLICK".into(), "Show Arrange View".into()));
                        if s.footer_context_help != cue {
                            s.footer_context_help = cue;
                        }
                    });
                })
                .on_click(|_, _, cx| {
                    update_state(cx, |s| {
                        s.clip_editor.show_editor = false;
                        s.show_piano_roll = false;
                        match s.mode {
                            AppMode::OpenLive => s.openlive_view = OpenLiveView::SessionMatrix,
                            AppMode::OpenStudio => s.openstudio_view = OpenStudioView::Playlist,
                        }
                    });
                })
                .child(
                    Label::new("ARRANGE")
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(if active_tab == StatusViewTab::Arrange {
                            accent_orange
                        } else {
                            normal_fg
                        }),
                ),
        )

        // Selector MIX
        .child(
            div()
                .id("footer_tab_mix")
                .px(px(6.0))
                .py(px(2.0))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0x2D2D2D)).rounded(px(2.0)))
                .on_mouse_move(|_, _, cx| {
                    update_state(cx, |s| {
                        let cue = Some(("CLICK".into(), "Show Mix View".into()));
                        if s.footer_context_help != cue {
                            s.footer_context_help = cue;
                        }
                    });
                })
                .on_click(|_, _, cx| {
                    update_state(cx, |s| {
                        s.clip_editor.show_editor = false;
                        s.show_piano_roll = false;
                        match s.mode {
                            AppMode::OpenLive => s.openlive_view = OpenLiveView::ArrangerView,
                            AppMode::OpenStudio => {
                                s.openstudio_view = OpenStudioView::ArrangerMixer
                            }
                        }
                    });
                })
                .child(
                    Label::new("MIX")
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(if active_tab == StatusViewTab::Mix {
                            accent_orange
                        } else {
                            normal_fg
                        }),
                ),
        )

        // Selector EDIT
        .child(
            div()
                .id("footer_tab_edit")
                .px(px(6.0))
                .py(px(2.0))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0x2D2D2D)).rounded(px(2.0)))
                .on_mouse_move(|_, _, cx| {
                    update_state(cx, |s| {
                        let cue = Some(("CLICK".into(), "Show Detail Edit View".into()));
                        if s.footer_context_help != cue {
                            s.footer_context_help = cue;
                        }
                    });
                })
                .on_click(|_, _, cx| {
                    update_state(cx, |s| {
                        if !s.clip_editor.show_editor && !s.show_piano_roll {
                            s.clip_editor.show_editor = true;
                            s.show_dsp_rack = false;
                        } else {
                            s.clip_editor.show_editor = false;
                            s.show_piano_roll = false;
                        }
                    });
                })
                .child(
                    Label::new("EDIT")
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(if active_tab == StatusViewTab::Edit {
                            accent_orange
                        } else {
                            normal_fg
                        }),
                ),
        )

        .child(separator_v())

        // ── Cuatro Iconos de Paneles Inferiores ──
        // 1. Detail Editor Panel (Notas / Audio)
        .child({
            let is_active = show_clip_editor || show_piano_roll;
            div()
                .id("footer_panel_detail_editor")
                .px(px(5.0))
                .py(px(3.0))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0x303030)).rounded(px(2.0)))
                .on_mouse_move(move |_, _, cx| {
                    update_state(cx, |s| {
                        let cue = Some((
                            "CLICK".into(),
                            if is_active {
                                "Hide Detail Editor Panel".into()
                            } else {
                                "Show Detail Editor Panel".into()
                            },
                        ));
                        if s.footer_context_help != cue {
                            s.footer_context_help = cue;
                        }
                    });
                })
                .on_click(|_, _, cx| {
                    update_state(cx, |s| {
                        if !s.clip_editor.show_editor && !s.show_piano_roll {
                            s.clip_editor.show_editor = true;
                            s.show_dsp_rack = false;
                        } else {
                            s.clip_editor.show_editor = false;
                            s.show_piano_roll = false;
                        }
                    });
                })
                .child(icon_detail_editor(if is_active {
                    accent_orange
                } else {
                    normal_fg
                }))
        })

        // 2. Automation Editor Panel
        .child({
            let is_active = app.panel_layout.show_automation;
            div()
                .id("footer_panel_automation")
                .px(px(5.0))
                .py(px(3.0))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0x303030)).rounded(px(2.0)))
                .on_mouse_move(move |_, _, cx| {
                    update_state(cx, |s| {
                        let cue = Some((
                            "CLICK".into(),
                            if is_active {
                                "Hide Automation Editor Panel".into()
                            } else {
                                "Show Automation Editor Panel".into()
                            },
                        ));
                        if s.footer_context_help != cue {
                            s.footer_context_help = cue;
                        }
                    });
                })
                .on_click(|_, _, cx| {
                    update_state(cx, |s| {
                        s.panel_layout.show_automation = !s.panel_layout.show_automation;
                    });
                })
                .child(icon_automation(if is_active {
                    accent_orange
                } else {
                    normal_fg
                }))
        })

        // 3. Device Panel (DSP Rack)
        .child({
            let is_active = show_dsp_rack;
            div()
                .id("footer_panel_device")
                .px(px(5.0))
                .py(px(3.0))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0x303030)).rounded(px(2.0)))
                .on_mouse_move(move |_, _, cx| {
                    update_state(cx, |s| {
                        let cue = Some((
                            "CLICK".into(),
                            if is_active {
                                "Hide Device Panel".into()
                            } else {
                                "Show Device Panel".into()
                            },
                        ));
                        if s.footer_context_help != cue {
                            s.footer_context_help = cue;
                        }
                    });
                })
                .on_click(|_, _, cx| {
                    update_state(cx, |s| {
                        s.show_dsp_rack = !s.show_dsp_rack;
                        if s.show_dsp_rack {
                            s.clip_editor.show_editor = false;
                            s.show_piano_roll = false;
                        }
                    });
                })
                .child(icon_device(if is_active {
                    accent_orange
                } else {
                    normal_fg
                }))
        })

        // 4. Mixer Panel
        .child({
            let is_active = show_mixer_panel || active_tab == StatusViewTab::Mix;
            div()
                .id("footer_panel_mixer")
                .px(px(5.0))
                .py(px(3.0))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0x303030)).rounded(px(2.0)))
                .on_mouse_move(move |_, _, cx| {
                    update_state(cx, |s| {
                        let cue = Some((
                            "CLICK".into(),
                            if is_active {
                                "Hide Mixer Panel".into()
                            } else {
                                "Show Mixer Panel".into()
                            },
                        ));
                        if s.footer_context_help != cue {
                            s.footer_context_help = cue;
                        }
                    });
                })
                .on_click(|_, _, cx| {
                    update_state(cx, |s| {
                        s.panel_layout.show_mixer = !s.panel_layout.show_mixer;
                    });
                })
                .child(icon_mixer(if is_active {
                    accent_orange
                } else {
                    normal_fg
                }))
        })

        // ─────────────────────────────────────────────────────────────────
        // 2. GRUPO CENTRAL: AYUDA CONTEXTUAL DINÁMICA (STATUS TEXT)
        // ─────────────────────────────────────────────────────────────────
        .child(
            h_flex()
                .items_center()
                .ml(px(8.0))
                .gap(px(6.0))
                .overflow_hidden()
                .when_some(context_help.clone(), |this, (action, desc)| {
                    this.child(
                        Label::new(action)
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0x7EB8D4)),
                    )
                    .child(
                        Label::new(desc)
                            .text_xs()
                            .text_color(rgb(0x9E9E9E)),
                    )
                })
                .when(context_help.is_none(), |this| {
                    this.child(
                        Label::new("DOUBLE-CLICK")
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0x7EB8D4)),
                    )
                    .child(
                        Label::new("Insert content")
                            .text_xs()
                            .text_color(rgb(0x9E9E9E)),
                    )
                    .child(div().w(px(12.0)))
                    .child(
                        Label::new("SHIFT+DOUBLE-CLICK")
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0x7EB8D4)),
                    )
                    .child(
                        Label::new("Insert favorites")
                            .text_xs()
                            .text_color(rgb(0x9E9E9E)),
                    )
                }),
        )

        // Espaciador elástico que divide izquierda/contexto de telemetría/derecha
        .child(div().flex_1())

        // ─────────────────────────────────────────────────────────────────
        // 3. TELEMETRÍA Y MOTOR DE AUDIO: [DSP LED] | CPU xx% | [SR | BUF]
        // ─────────────────────────────────────────────────────────────────

        // Indicador de Motor de Audio (DSP Engine)
        .child(
            h_flex()
                .id("footer_audio_engine_status")
                .items_center()
                .gap(px(4.0))
                .px(px(6.0))
                .py(px(2.0))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0x2D2D2D)).rounded(px(2.0)))
                .on_mouse_move(|_, _, cx| {
                    update_state(cx, |s| {
                        let cue = Some(("CLICK".into(), "Open Audio Engine Settings".into()));
                        if s.footer_context_help != cue {
                            s.footer_context_help = cue;
                        }
                    });
                })
                .on_click(|_, _, cx| {
                    update_state(cx, |s| {
                        s.audio_settings_state.is_open = true;
                    });
                })
                // LED Verde de motor activo
                .child(
                    div()
                        .w(px(6.0))
                        .h(px(6.0))
                        .rounded_full()
                        .bg(rgb(0x1FEB54)),
                )
                .child(
                    Label::new("DSP")
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0xCCCCCC)),
                ),
        )

        .child(separator_v())

        // Indicador de Carga de CPU con micro-medidor horizontal
        .child(
            h_flex()
                .id("footer_cpu_meter")
                .items_center()
                .gap(px(4.0))
                .px(px(6.0))
                .py(px(2.0))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0x2D2D2D)).rounded(px(2.0)))
                .on_mouse_move(|_, _, cx| {
                    update_state(cx, |s| {
                        let cue = Some(("CLICK".into(), "Show Engine Performance".into()));
                        if s.footer_context_help != cue {
                            s.footer_context_help = cue;
                        }
                    });
                })
                .on_click(|_, _, cx| {
                    update_state(cx, |s| {
                        s.control_panel_state.is_open = true;
                    });
                })
                .child(
                    Label::new("CPU")
                        .text_xs()
                        .text_color(rgb(0x888888)),
                )
                // Barra medidora horizontal compacta
                .child(
                    div()
                        .w(px(26.0))
                        .h(px(4.0))
                        .bg(rgb(0x1A1A1A))
                        .rounded(px(1.0))
                        .border_1()
                        .border_color(rgb(0x383838))
                        .overflow_hidden()
                        .child(
                            div()
                                .h_full()
                                .w(relative(cpu_usage))
                                .bg(if cpu_usage > 0.85 {
                                    rgb(0xD93838)
                                } else {
                                    rgb(0x1FEB54)
                                }),
                        ),
                )
                .child(
                    Label::new(format!("{:.0}%", cpu_usage * 100.0))
                        .text_xs()
                        .text_color(rgb(0xCCCCCC)),
                ),
        )

        .child(separator_v())

        // Selector / Indicador de Frecuencia y Buffer (ej: 48 kHz | 128 spl)
        .child(
            h_flex()
                .id("footer_buffer_status")
                .items_center()
                .px(px(6.0))
                .py(px(2.0))
                .bg(rgb(0x1E1E20))
                .border_1()
                .border_color(rgb(0x343438))
                .rounded(px(2.0))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0x28282C)).border_color(rgb(0x4C4C54)))
                .on_mouse_move(|_, _, cx| {
                    update_state(cx, |s| {
                        let cue = Some(("CLICK".into(), "Configure Audio Buffer & Latency".into()));
                        if s.footer_context_help != cue {
                            s.footer_context_help = cue;
                        }
                    });
                })
                .on_click(|_, _, cx| {
                    update_state(cx, |s| {
                        s.audio_settings_state.is_open = true;
                    });
                })
                .child(
                    Label::new(format!("{} kHz | {} spl", sr / 1000, buffer_size))
                        .text_xs()
                        .text_color(rgb(0xBBBBBB)),
                ),
        )

        .child(separator_v())

        // ─────────────────────────────────────────────────────────────────
        // 4. GRUPO DERECHO: HERRAMIENTAS LATERALES Y TECLADO
        //    [Lupa] [Proyecto] [Monitor] [Mapeo] | [Teclado]
        // ─────────────────────────────────────────────────────────────────

        // 1. Browser Panel (Lupa)
        .child({
            let is_active = show_explorer;
            div()
                .id("footer_tool_browser")
                .px(px(5.0))
                .py(px(3.0))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0x303030)).rounded(px(2.0)))
                .on_mouse_move(move |_, _, cx| {
                    update_state(cx, |s| {
                        let cue = Some((
                            "CLICK".into(),
                            if is_active {
                                "Hide Browser Panel".into()
                            } else {
                                "Show Browser Panel".into()
                            },
                        ));
                        if s.footer_context_help != cue {
                            s.footer_context_help = cue;
                        }
                    });
                })
                .on_click(|_, _, cx| {
                    update_state(cx, |s| {
                        s.show_explorer = !s.show_explorer;
                        s.panel_layout.show_inspector = s.show_explorer;
                    });
                })
                .child(icon_search(if is_active {
                    accent_orange
                } else {
                    normal_fg
                }))
        })

        // 2. Project Panel (Documento)
        .child({
            let is_active = show_about;
            div()
                .id("footer_tool_project")
                .px(px(5.0))
                .py(px(3.0))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0x303030)).rounded(px(2.0)))
                .on_mouse_move(|_, _, cx| {
                    update_state(cx, |s| {
                        let cue = Some(("CLICK".into(), "Show Project Panel".into()));
                        if s.footer_context_help != cue {
                            s.footer_context_help = cue;
                        }
                    });
                })
                .on_click(|_, _, cx| {
                    update_state(cx, |s| {
                        s.show_about = !s.show_about;
                    });
                })
                .child(icon_project(if is_active {
                    accent_orange
                } else {
                    normal_fg
                }))
        })

        // 3. Studio Panel (Monitor de Estudio)
        .child({
            let is_active = control_panel_open;
            div()
                .id("footer_tool_studio")
                .px(px(5.0))
                .py(px(3.0))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0x303030)).rounded(px(2.0)))
                .on_mouse_move(|_, _, cx| {
                    update_state(cx, |s| {
                        let cue = Some(("CLICK".into(), "Show Studio & Monitor Settings".into()));
                        if s.footer_context_help != cue {
                            s.footer_context_help = cue;
                        }
                    });
                })
                .on_click(|_, _, cx| {
                    update_state(cx, |s| {
                        s.control_panel_state.is_open = !s.control_panel_state.is_open;
                    });
                })
                .child(icon_studio_monitor(if is_active {
                    accent_orange
                } else {
                    normal_fg
                }))
        })

        // 4. Mappings Panel (Mano / MCP Port Settings)
        .child({
            let is_active = mcp_port_settings_open;
            div()
                .id("footer_tool_mappings")
                .px(px(5.0))
                .py(px(3.0))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0x303030)).rounded(px(2.0)))
                .on_mouse_move(|_, _, cx| {
                    update_state(cx, |s| {
                        let cue = Some(("CLICK".into(), "Show Controller & MCP Mappings".into()));
                        if s.footer_context_help != cue {
                            s.footer_context_help = cue;
                        }
                    });
                })
                .on_click(|_, _, cx| {
                    update_state(cx, |s| {
                        s.mcp_port_settings_state.is_open = !s.mcp_port_settings_state.is_open;
                    });
                })
                .child(icon_hand(if is_active {
                    accent_orange
                } else {
                    normal_fg
                }))
        })

        .child(separator_v())

        // 5. Virtual Keyboard (Teclado de Piano)
        .child({
            let is_active = show_keyboard;
            div()
                .id("footer_tool_keyboard")
                .px(px(5.0))
                .py(px(3.0))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0x303030)).rounded(px(2.0)))
                .on_mouse_move(move |_, _, cx| {
                    update_state(cx, |s| {
                        let cue = Some((
                            "CLICK".into(),
                            if is_active {
                                "Hide Keyboard".into()
                            } else {
                                "Show Keyboard".into()
                            },
                        ));
                        if s.footer_context_help != cue {
                            s.footer_context_help = cue;
                        }
                    });
                })
                .on_click(|_, _, cx| {
                    update_state(cx, |s| {
                        s.panel_layout.show_keyboard = !s.panel_layout.show_keyboard;
                    });
                })
                .child(icon_keyboard(if is_active {
                    accent_orange
                } else {
                    normal_fg
                }))
        })
}
