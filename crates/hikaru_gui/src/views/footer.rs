use gpui_kit::component::*;
use gpui_kit::component::label::Label;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, AppMode, HikaruApp, OpenLiveView, OpenStudioView};
use crate::theme;

/// Pestañas de vista inferiores (`ARRANGE`, `MIX`, `EDIT`).
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
    let active_mode = app.mode;
    let openlive_view = app.openlive_view;
    let openstudio_view = app.openstudio_view;

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
}

