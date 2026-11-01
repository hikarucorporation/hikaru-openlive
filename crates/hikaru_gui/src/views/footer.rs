use gpui_kit::component::*;
use gpui_kit::component::button::{Button, ButtonRounded};
use gpui_kit::component::label::Label;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, AppMode, HikaruApp, OpenLiveView, OpenStudioView};
use crate::theme;

/// Pestañas de vista inferiores (`ARRANGE`, `MIX`, `EDIT`) + toggles de paneles acoplados.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusViewTab {
    Arrange,
    Mix,
    Edit,
}

fn view_tab_pill(
    id: &'static str,
    label: &'static str,
    active: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let (bg_col, fg_col, border_col) = if active {
        (
            rgb(theme::DAW_ACCENT_ORANGE),
            rgb(0x141414),
            rgb(0xFF9848),
        )
    } else {
        (
            rgb(0x262626),
            rgb(0xB8B8B8),
            rgb(theme::DAW_BORDER_LIGHT),
        )
    };

    h_flex()
        .id(id)
        .h(px(18.0))
        .px(px(8.0))
        .items_center()
        .justify_center()
        .bg(bg_col)
        .border_1()
        .border_color(border_col)
        .cursor_pointer()
        .hover(move |s| {
            if active {
                s.bg(rgb(0xF08436))
            } else {
                s.bg(rgb(0x323232)).text_color(rgb(0xEEEEEE))
            }
        })
        .on_click(on_click)
        .child(
            Label::new(label)
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(fg_col),
        )
}

pub fn render(cx: &mut Context<HikaruApp>) -> impl IntoElement {
    let app = state(cx).read(cx);

    let show_clip_editor = app.clip_editor.show_editor;
    let show_piano_roll = app.show_piano_roll;
    let show_dsp_rack = app.show_dsp_rack;
    let active_mode = app.mode;
    let openlive_view = app.openlive_view;
    let openstudio_view = app.openstudio_view;
    let is_playing = app.transport.playback_state
        == hikaru_transport::transport_state::TransportPlaybackState::Playing;
    let cpu_usage = app.cpu_usage;
    let selected_track = app.selected_track_index;

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

    let context_help = match active_tab {
        StatusViewTab::Arrange => match active_mode {
            AppMode::OpenLive => {
                "DOUBLE-CLICK  Insert audio clip in slot   •   CLICK ▶  Launch clip or scene   •   DRAG  Adjust track fader / pan"
            }
            AppMode::OpenStudio => {
                "DOUBLE-CLICK  Create clip on timeline   •   ALT+DRAG  Duplicate clip   •   CTRL+WHEEL  Zoom timeline"
            }
        },
        StatusViewTab::Mix => {
            "CLICK  Select channel strip   •   DRAG FADER  Adjust gain (dB)   •   TAB  Switch Arrange / Mix view"
        }
        StatusViewTab::Edit => {
            "SCROLL  Zoom detail waveform / notes   •   DRAG MARKERS  Adjust loop & clip boundaries"
        }
    };

    let clip_icon = if show_clip_editor {
        "CLIP [▼]"
    } else {
        "CLIP [▲]"
    };
    let pr_icon = if show_piano_roll {
        "NOTES [▼]"
    } else {
        "NOTES [▲]"
    };
    let dsp_icon = if show_dsp_rack {
        "DEVICES [▼]"
    } else {
        "DEVICES [▲]"
    };

    let engine_text = if is_playing {
        "ENGINE: ACTIVE"
    } else {
        "ENGINE: READY"
    };
    let engine_col = if is_playing {
        rgb(0x3CD070)
    } else {
        rgb(0x9A9A9A)
    };

    drop(app);

    h_flex()
        .id("footer_bar")
        .h(px(24.0))
        .bg(rgb(theme::DAW_STATUS_BG))
        .border_t_1()
        .border_color(rgb(theme::DAW_BORDER_LIGHT))
        .items_center()
        .px(px(6.0))
        .gap(px(4.0))
        // Grupo izquierdo: Pestañas de vista (ARRANGE | MIX | EDIT)
        .child(
            h_flex()
                .items_center()
                .gap(px(2.0))
                .child(view_tab_pill(
                    "footer_tab_arrange",
                    "ARRANGE",
                    active_tab == StatusViewTab::Arrange,
                    move |_, _, cx| {
                        state(cx).update(cx, |s, cx| {
                            s.clip_editor.show_editor = false;
                            s.show_piano_roll = false;
                            match s.mode {
                                AppMode::OpenLive => s.openlive_view = OpenLiveView::SessionMatrix,
                                AppMode::OpenStudio => s.openstudio_view = OpenStudioView::Playlist,
                            }
                            cx.notify();
                        });
                    },
                ))
                .child(view_tab_pill(
                    "footer_tab_mix",
                    "MIX",
                    active_tab == StatusViewTab::Mix,
                    move |_, _, cx| {
                        state(cx).update(cx, |s, cx| {
                            s.clip_editor.show_editor = false;
                            s.show_piano_roll = false;
                            match s.mode {
                                AppMode::OpenLive => s.openlive_view = OpenLiveView::ArrangerView,
                                AppMode::OpenStudio => {
                                    s.openstudio_view = OpenStudioView::ArrangerMixer
                                }
                            }
                            cx.notify();
                        });
                    },
                ))
                .child(view_tab_pill(
                    "footer_tab_edit",
                    "EDIT",
                    active_tab == StatusViewTab::Edit,
                    move |_, _, cx| {
                        state(cx).update(cx, |s, cx| {
                            if !s.clip_editor.show_editor && !s.show_piano_roll {
                                s.clip_editor.show_editor = true;
                                s.show_dsp_rack = false;
                            } else {
                                s.clip_editor.show_editor = false;
                                s.show_piano_roll = false;
                            }
                            cx.notify();
                        });
                    },
                )),
        )
        // Separador vertical
        .child(
            div()
                .w(px(1.0))
                .h(px(14.0))
                .mx(px(2.0))
                .bg(rgb(theme::DAW_BORDER_LIGHT)),
        )
        // Toggles de paneles inferiores (IDs preservados para tests y atajos)
        .child(
            Button::new("footer_clip_editor")
                .rounded(ButtonRounded::None)
                .label(clip_icon)
                .compact()
                .bg(if show_clip_editor {
                    rgb(theme::DAW_ACCENT_ORANGE)
                } else {
                    rgb(0x2B2B2B)
                })
                .text_color(if show_clip_editor {
                    rgb(0x141414)
                } else {
                    rgb(0xCCCCCC)
                })
                .on_click(move |_, _, cx| {
                    let state = state(cx);
                    state.update(cx, |state, cx| {
                        state.clip_editor.show_editor = !state.clip_editor.show_editor;
                        if state.clip_editor.show_editor {
                            state.show_piano_roll = false;
                            state.show_dsp_rack = false;
                        }
                        cx.notify();
                    });
                }),
        )
        .child(
            Button::new("footer_piano_roll")
                .rounded(ButtonRounded::None)
                .label(pr_icon)
                .compact()
                .bg(if show_piano_roll {
                    rgb(theme::DAW_ACCENT_ORANGE)
                } else {
                    rgb(0x2B2B2B)
                })
                .text_color(if show_piano_roll {
                    rgb(0x141414)
                } else {
                    rgb(0xCCCCCC)
                })
                .on_click(move |_, _, cx| {
                    let state = state(cx);
                    state.update(cx, |state, cx| {
                        state.show_piano_roll = !state.show_piano_roll;
                        if state.show_piano_roll {
                            state.clip_editor.show_editor = false;
                            state.show_dsp_rack = false;
                        }
                        cx.notify();
                    });
                }),
        )
        .child(
            Button::new("footer_dsp_rack")
                .rounded(ButtonRounded::None)
                .label(dsp_icon)
                .compact()
                .bg(if show_dsp_rack {
                    rgb(theme::DAW_ACCENT_ORANGE)
                } else {
                    rgb(0x2B2B2B)
                })
                .text_color(if show_dsp_rack {
                    rgb(0x141414)
                } else {
                    rgb(0xCCCCCC)
                })
                .on_click(move |_, _, cx| {
                    let state = state(cx);
                    state.update(cx, |state, cx| {
                        state.show_dsp_rack = !state.show_dsp_rack;
                        if state.show_dsp_rack {
                            state.clip_editor.show_editor = false;
                            state.show_piano_roll = false;
                        }
                        cx.notify();
                    });
                }),
        )
        // Separador + Icono de ayuda contextual y mensaje estilo Bitwig StatusBar
        .child(
            div()
                .w(px(1.0))
                .h(px(14.0))
                .mx(px(4.0))
                .bg(rgb(theme::DAW_BORDER_LIGHT)),
        )
        .child(
            h_flex()
                .items_center()
                .gap(px(6.0))
                .flex_1()
                .overflow_hidden()
                .child(
                    div()
                        .w(px(14.0))
                        .h(px(14.0))
                        .bg(rgb(0x2E2E2E))
                        .border_1()
                        .border_color(rgb(0x484848))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            Label::new("i")
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(theme::DAW_ACCENT_ORANGE)),
                        ),
                )
                .child(
                    Label::new(context_help)
                        .text_xs()
                        .text_color(rgb(0x9E9E9E)),
                ),
        )
        // Indicadores técnicos de motor, pista seleccionada y licencia AGPLv3
        .child(
            Label::new(format!("TRK #{}", selected_track + 1))
                .text_xs()
                .text_color(rgb(0x888888)),
        )
        .child(
            div()
                .w(px(1.0))
                .h(px(12.0))
                .mx(px(3.0))
                .bg(rgb(0x333333)),
        )
        .child(
            Label::new(format!("DSP {:.1}%", cpu_usage * 100.0))
                .text_xs()
                .text_color(rgb(0xBDBDBD)),
        )
        .child(
            div()
                .w(px(1.0))
                .h(px(12.0))
                .mx(px(3.0))
                .bg(rgb(0x333333)),
        )
        .child(Label::new(engine_text).text_xs().text_color(engine_col))
        .child(
            div()
                .w(px(1.0))
                .h(px(12.0))
                .mx(px(3.0))
                .bg(rgb(0x333333)),
        )
        .child(
            Label::new("AGPLv3")
                .text_xs()
                .text_color(rgb(0x707070)),
        )
}

