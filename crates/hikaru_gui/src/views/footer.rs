// Hikaru OpenLive - Footer / Status Bar
// GNU AGPLv3
// crates/hikaru_gui/src/views/footer.rs

use gpui_kit::component::*;
use gpui_kit::component::label::Label;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{
    clear_footer_help, set_footer_cues, state, update_state, AppMode, HikaruApp, OpenLiveView,
    OpenStudioView,
};
use crate::theme;

/// Determina si una etiqueta de acción (`CTRL+CLICK:`, `SHIFT+DRAG:`, `ALT+CLICK:`)
/// coincide con un modificador de teclado activo actualmente.
fn cue_matches_modifiers(action: &str, mods: gpui_kit::Modifiers) -> bool {
    let upper = action.to_ascii_uppercase();
    let has_shift = upper.contains("SHIFT");
    let has_ctrl = upper.contains("CTRL") || upper.contains("CMD");
    let has_alt = upper.contains("ALT");
    if !has_shift && !has_ctrl && !has_alt {
        return false;
    }
    let ctrl_down = mods.control || mods.platform;
    (!has_shift || mods.shift) && (!has_ctrl || ctrl_down) && (!has_alt || mods.alt)
}

/// Normaliza la clave de acción asegurando el sufijo `:` estilo Bitwig (`CLICK:`).
fn normalize_action_key(action: &str) -> String {
    let trimmed = action.trim();
    if trimmed.ends_with(':') {
        trimmed.to_string()
    } else {
        format!("{trimmed}:")
    }
}

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
    let show_automation = app.panel_layout.show_automation;
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
    let context_cues = app.footer_context_cues.clone();
    let active_mods = app.active_modifiers;

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

    // Colores de acento y neutros estilo Bitwig Studio 5.1.7
    let accent_orange = rgb(theme::DAW_ACCENT_ORANGE);
    let normal_fg = rgb(0x8F969E);
    let border_col = rgb(0x121212);

    // Pistas efectivas a mostrar en la franja central del Footer
    let display_cues: Vec<(String, String)> = if !context_cues.is_empty() {
        context_cues
    } else if let Some((act, desc)) = context_help {
        vec![(act, desc)]
    } else {
        vec![
            ("CLICK:".into(), "Select Track".into()),
            ("DOUBLE-CLICK:".into(), "Show Devices".into()),
            ("ALT+CLICK:".into(), "Rename".into()),
            ("SHIFT+DRAG:".into(), "Fine Adjust".into()),
        ]
    };

    let mut active_mod_badges: Vec<&'static str> = Vec::new();
    if active_mods.control || active_mods.platform {
        active_mod_badges.push("CTRL");
    }
    if active_mods.shift {
        active_mod_badges.push("SHIFT");
    }
    if active_mods.alt {
        active_mod_badges.push("ALT");
    }

    h_flex()
        .id("footer_bar")
        .h(px(26.0))
        .w_full()
        .bg(rgb(0x1E1F23))
        .border_t_1()
        .border_color(border_col)
        .items_center()
        .px(px(6.0))
        .gap(px(2.0))
        .overflow_hidden()
        // Reset de ayuda contextual al mover el mouse en el fondo neutro del footer
        .on_mouse_move(|_, _, cx| {
            clear_footer_help(cx);
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
                .on_mouse_move(|e, _, cx| {
                    cx.stop_propagation();
                    set_footer_cues(
                        cx,
                        Some(e.modifiers),
                        &[("CLICK:", "Toggle Inspector & Context Help (F11)")],
                    );
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
                .on_mouse_move(|e, _, cx| {
                    cx.stop_propagation();
                    set_footer_cues(
                        cx,
                        Some(e.modifiers),
                        &[("CLICK:", "Show Arrange View"), ("TAB / F9:", "Toggle Mix View")],
                    );
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
                .on_mouse_move(|e, _, cx| {
                    cx.stop_propagation();
                    set_footer_cues(
                        cx,
                        Some(e.modifiers),
                        &[("CLICK:", "Show Mix View"), ("TAB / F9:", "Toggle Arrange View")],
                    );
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
                .on_mouse_move(|e, _, cx| {
                    cx.stop_propagation();
                    set_footer_cues(
                        cx,
                        Some(e.modifiers),
                        &[("CLICK:", "Show Detail Edit View")],
                    );
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
                .on_mouse_move(move |e, _, cx| {
                    cx.stop_propagation();
                    set_footer_cues(
                        cx,
                        Some(e.modifiers),
                        &[(
                            "CLICK:",
                            if is_active {
                                "Hide Detail Editor Panel"
                            } else {
                                "Show Detail Editor Panel"
                            },
                        )],
                    );
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
            let is_active = show_automation;
            div()
                .id("footer_panel_automation")
                .px(px(5.0))
                .py(px(3.0))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0x303030)).rounded(px(2.0)))
                .on_mouse_move(move |e, _, cx| {
                    cx.stop_propagation();
                    set_footer_cues(
                        cx,
                        Some(e.modifiers),
                        &[(
                            "CLICK:",
                            if is_active {
                                "Hide Automation Editor Panel"
                            } else {
                                "Show Automation Editor Panel"
                            },
                        )],
                    );
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
                .on_mouse_move(move |e, _, cx| {
                    cx.stop_propagation();
                    set_footer_cues(
                        cx,
                        Some(e.modifiers),
                        &[(
                            "CLICK:",
                            if is_active {
                                "Hide Device Panel (F10)"
                            } else {
                                "Show Device Panel (F10)"
                            },
                        )],
                    );
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
                .on_mouse_move(move |e, _, cx| {
                    cx.stop_propagation();
                    set_footer_cues(
                        cx,
                        Some(e.modifiers),
                        &[(
                            "CLICK:",
                            if is_active {
                                "Hide Mixer Panel"
                            } else {
                                "Show Mixer Panel"
                            },
                        )],
                    );
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
        // 2. GRUPO CENTRAL: AYUDA CONTEXTUAL DINÁMICA (MULTI-CUE BITWIG 5.1.7)
        // ─────────────────────────────────────────────────────────────────
        .child(
            h_flex()
                .items_center()
                .ml(px(8.0))
                .gap(px(12.0))
                .min_w(px(0.0))
                .overflow_hidden()
                .when(!active_mod_badges.is_empty(), |this| {
                    this.child(
                        div()
                            .px(px(4.0))
                            .py(px(0.5))
                            .rounded(px(2.0))
                            .bg(rgb(0x2F2216))
                            .border_1()
                            .border_color(accent_orange)
                            .font_family("monospace")
                            .text_size(px(9.5))
                            .font_weight(FontWeight::BOLD)
                            .text_color(accent_orange)
                            .flex_shrink_0()
                            .child(active_mod_badges.join("+")),
                    )
                })
                .children(display_cues.into_iter().map(move |(action, desc)| {
                    let mod_active = cue_matches_modifiers(&action, active_mods);
                    let action_key = normalize_action_key(&action);
                    h_flex()
                        .items_center()
                        .gap(px(4.0))
                        .flex_shrink_0()
                        .child(
                            Label::new(action_key)
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .text_color(if mod_active {
                                    accent_orange
                                } else {
                                    rgb(0x7EB8D4)
                                }),
                        )
                        .child(
                            Label::new(desc)
                                .text_xs()
                                .text_color(if mod_active {
                                    rgb(0xEAECEF)
                                } else {
                                    rgb(0xA4AAB2)
                                }),
                        )
                })),
        )

        // Espaciador elástico que divide izquierda/contexto de telemetría/derecha
        .child(div().flex_1())

        // ─────────────────────────────────────────────────────────────────
        // 3. TELEMETRÍA Y MOTOR DE AUDIO MONOESPACIADA:
        //    [DSP LED] | CPU: 12% | 48 kHz | 128 spl
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
                .on_mouse_move(|e, _, cx| {
                    cx.stop_propagation();
                    set_footer_cues(
                        cx,
                        Some(e.modifiers),
                        &[("CLICK:", "Open Audio Engine Settings")],
                    );
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
                    div()
                        .font_family("monospace")
                        .text_size(px(10.5))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0xD0D4DC))
                        .child("DSP"),
                ),
        )

        .child(separator_v())

        // Indicador de Carga de CPU monoespaciado (`CPU: 12%`) con micro-medidor horizontal
        .child(
            h_flex()
                .id("footer_cpu_meter")
                .items_center()
                .gap(px(5.0))
                .px(px(6.0))
                .py(px(2.0))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0x2D2D2D)).rounded(px(2.0)))
                .on_mouse_move(|e, _, cx| {
                    cx.stop_propagation();
                    set_footer_cues(
                        cx,
                        Some(e.modifiers),
                        &[("CLICK:", "Show Engine Performance & DSP Load")],
                    );
                })
                .on_click(|_, _, cx| {
                    update_state(cx, |s| {
                        s.control_panel_state.is_open = true;
                    });
                })
                .child(
                    div()
                        .font_family("monospace")
                        .text_size(px(10.5))
                        .text_color(rgb(0xD0D4DC))
                        .child(format!("CPU: {:.0}%", cpu_usage * 100.0)),
                )
                // Barra medidora horizontal compacta
                .child(
                    div()
                        .w(px(26.0))
                        .h(px(4.0))
                        .bg(rgb(0x121212))
                        .rounded(px(1.0))
                        .border_1()
                        .border_color(rgb(0x34363C))
                        .overflow_hidden()
                        .child(
                            div()
                                .h_full()
                                .w(relative(cpu_usage))
                                .bg(if cpu_usage > 0.85 {
                                    rgb(0xFF3B30)
                                } else {
                                    rgb(0x2CE64B)
                                }),
                        ),
                ),
        )

        .child(separator_v())

        // Indicadores monoespaciados de Frecuencia (`48 kHz`) y Buffer (`128 spl`)
        .child(
            h_flex()
                .id("footer_buffer_status")
                .items_center()
                .gap(px(6.0))
                .px(px(6.0))
                .py(px(2.0))
                .bg(rgb(0x141518))
                .border_1()
                .border_color(rgb(0x2E3138))
                .rounded(px(2.0))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0x26282E)).border_color(rgb(0x484C56)))
                .on_mouse_move(|e, _, cx| {
                    cx.stop_propagation();
                    set_footer_cues(
                        cx,
                        Some(e.modifiers),
                        &[("CLICK:", "Configure Audio Sample Rate & Buffer Size")],
                    );
                })
                .on_click(|_, _, cx| {
                    update_state(cx, |s| {
                        s.audio_settings_state.is_open = true;
                    });
                })
                .child(
                    div()
                        .font_family("monospace")
                        .text_size(px(10.5))
                        .text_color(rgb(0xD0D4DC))
                        .child(format!("{} kHz", sr / 1000)),
                )
                .child(div().w(px(1.0)).h(px(10.0)).bg(rgb(0x32353E)))
                .child(
                    div()
                        .font_family("monospace")
                        .text_size(px(10.5))
                        .text_color(rgb(0xD0D4DC))
                        .child(format!("{} spl", buffer_size)),
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
                .on_mouse_move(move |e, _, cx| {
                    cx.stop_propagation();
                    set_footer_cues(
                        cx,
                        Some(e.modifiers),
                        &[(
                            "CLICK:",
                            if is_active {
                                "Hide Browser Panel (F11)"
                            } else {
                                "Show Browser Panel (F11)"
                            },
                        )],
                    );
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
                .on_mouse_move(|e, _, cx| {
                    cx.stop_propagation();
                    set_footer_cues(cx, Some(e.modifiers), &[("CLICK:", "Show Project Panel")]);
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
                .on_mouse_move(|e, _, cx| {
                    cx.stop_propagation();
                    set_footer_cues(
                        cx,
                        Some(e.modifiers),
                        &[("CLICK:", "Show Studio & Monitor Settings")],
                    );
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
                .on_mouse_move(|e, _, cx| {
                    cx.stop_propagation();
                    set_footer_cues(
                        cx,
                        Some(e.modifiers),
                        &[("CLICK:", "Show Controller & MCP Mappings")],
                    );
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
                .on_mouse_move(move |e, _, cx| {
                    cx.stop_propagation();
                    set_footer_cues(
                        cx,
                        Some(e.modifiers),
                        &[(
                            "CLICK:",
                            if is_active {
                                "Hide Virtual Keyboard"
                            } else {
                                "Show Virtual Keyboard"
                            },
                        )],
                    );
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
