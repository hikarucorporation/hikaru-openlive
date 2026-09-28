use gpui_kit::component::*;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, HikaruApp};

pub fn render(cx: &mut Context<HikaruApp>) -> impl IntoElement {
    let app = state(cx).read(cx);
    let sr = app.transport.sample_rate.get() as u32;
    drop(app);

    h_flex()
        .id("footer_bar")
        .h(px(24.0))
        .bg(rgb(0x181A20))
        .border_t_1()
        .border_color(rgb(0x2A2D37))
        .items_center()
        .px(px(6.0))
        .gap(px(4.0))
        .child(Label::new(format!("Sample Rate: {} Hz", sr)).text_xs())
        .child(div().flex_1())
        .child(Label::new("Hikaru OpenStudio | AGPLv3").text_xs())
}
