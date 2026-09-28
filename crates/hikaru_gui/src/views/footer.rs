use gpui_kit::component::*;
use gpui_kit::component::button::Button;
use gpui_kit::component::label::Label;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, HikaruApp};

pub fn render(cx: &mut Context<HikaruApp>) -> impl IntoElement {
    let app = state(cx).read(cx);

    let clip_icon = if app.matrix_state.show_editor {
        "🎛 CLIP EDITOR [▼]"
    } else {
        "🎛 CLIP EDITOR [▲]"
    };
    let pr_icon = if app.show_piano_roll {
        "🎹 PIANO ROLL [▼]"
    } else {
        "🎹 PIANO ROLL [▲]"
    };
    let dsp_icon = if app.show_dsp_rack {
        "🎚 DSP RACK [▼]"
    } else {
        "🎚 DSP RACK [▲]"
    };

    let cpu_usage = app.cpu_usage;
    drop(app);

    h_flex()
        .id("footer_bar")
        .h(px(24.0))
        .bg(rgb(0x2A2A2A))
        .border_t_1()
        .border_color(rgb(0x3D3D3D))
        .items_center()
        .px(px(6.0))
        .gap(px(4.0))
        .child(
            Button::new("footer_clip_editor")
                .label(clip_icon)
                .compact()
                .bg(rgb(0x3D3D3D))
                .text_color(rgb(0xE0E0E0))
                .on_click(move |_, _, cx| {
                    let state = state(cx);
                    state.update(cx, |state, cx| {
                        state.matrix_state.show_editor = !state.matrix_state.show_editor;
                        if state.matrix_state.show_editor {
                            state.show_piano_roll = false;
                            state.show_dsp_rack = false;
                        }
                        cx.notify();
                    });
                }),
        )
        .child(
            Button::new("footer_piano_roll")
                .label(pr_icon)
                .compact()
                .bg(rgb(0x3D3D3D))
                .text_color(rgb(0xE0E0E0))
                .on_click(move |_, _, cx| {
                    let state = state(cx);
                    state.update(cx, |state, cx| {
                        state.show_piano_roll = !state.show_piano_roll;
                        if state.show_piano_roll {
                            state.matrix_state.show_editor = false;
                            state.show_dsp_rack = false;
                        }
                        cx.notify();
                    });
                }),
        )
        .child(
            Button::new("footer_dsp_rack")
                .label(dsp_icon)
                .compact()
                .bg(rgb(0x3D3D3D))
                .text_color(rgb(0xE0E0E0))
                .on_click(move |_, _, cx| {
                    let state = state(cx);
                    state.update(cx, |state, cx| {
                        state.show_dsp_rack = !state.show_dsp_rack;
                        if state.show_dsp_rack {
                            state.matrix_state.show_editor = false;
                            state.show_piano_roll = false;
                        }
                        cx.notify();
                    });
                }),
        )
        .child(Label::new("Hikaru OpenLive | AGPLv3").text_xs().text_color(rgb(0xE0E0E0)))
        .child(Label::new(format!("CPU: {:.1}%", cpu_usage * 100.0)).text_xs().text_color(rgb(0xE0E0E0)))
        .child(div().flex_1())
        .child(Label::new("ENGINE: IDLE").text_xs().text_color(rgb(0xE0E0E0)))
}
