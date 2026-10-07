use gpui_kit::component::button::Button;
use gpui_kit::component::label::Label;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, AppMode, HikaruApp};
use crate::version::VERSION;

const BUILDER_NAME: &str = "Hikaru Corporation";

pub fn render(cx: &mut Context<HikaruApp>) -> AnyElement {
    let mode = state(cx).read(cx).mode;

    let title = match mode {
        AppMode::OpenLive => "Hikaru OpenLive",
        AppMode::OpenStudio => "Hikaru OpenStudio",
    };
    let daw_name = title;

    v_flex()
        .id("about_window")
        .absolute()
        .left(px(20.0))
        .top(px(80.0))
        .w(px(420.0))
        .h(px(420.0))
        .bg(rgb(0x181A20))
        .border_1()
        .border_color(rgb(0x2A2D37))
        .rounded(px(6.0))
        .p(px(16.0))
        .gap(px(8.0))
        .items_center()
        .child(
            div()
                .w(px(64.0))
                .h(px(64.0))
                .bg(rgb(0x00B4D8))
                .rounded(px(8.0))
                .flex()
                .items_center()
                .justify_center()
                .child(Label::new("⚡").text_2xl())
                .into_any_element(),
        )
        .child(Label::new(title).text_xl().font_weight(FontWeight::BOLD))
        .child(Label::new("An Advanced Digital Audio Workstation for UNIX sysadmins.").text_xs())
        .child(Label::new("Copyright © Hikaru Corporation - 2026").text_xs())
        .child(Label::new("This is a free software protected by GNU AGPLv3:").text_xs())
        .child(
            Button::new("about-license").rounded(gpui_kit::component::button::ButtonRounded::None)
                .label("GNU AGPLv3 License")
                .compact()
                .on_click(move |_, _, _| {}),
        )
        .child(Label::new("Coming Soon").text_sm().font_weight(FontWeight::BOLD))
        .child(Label::new("Available on Windows, Linux, BSD distros like Free/Open/NetBSD, microwaves, calculator, potatoes, your mother, apache helicopter, anything shit that run Rust, etc.").text_xs())
        .child(div().flex_1())
        .child(
            h_flex()
                .w_full()
                .justify_between()
                .child(Label::new(daw_name).text_xs())
                .child(Label::new(format!("Compiled by {} | Version {}", BUILDER_NAME, VERSION)).text_xs()),
        )
        .child(
            Button::new("about_close").rounded(gpui_kit::component::button::ButtonRounded::None)
                .label("Cerrar")
                .compact()
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    cx.update_entity(&st, |state, cx| {
                        state.show_about = false;
                        cx.notify();
                    });
                }),
        )
        .into_any_element()
}
