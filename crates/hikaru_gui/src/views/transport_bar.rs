use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, HikaruApp};

pub fn render(cx: &mut Context<HikaruApp>) -> impl IntoElement {
    let app = state(cx).read(cx);
    let is_playing = app.transport.playback_state == hikaru_transport::transport_state::TransportPlaybackState::Playing;
    let is_recording = app.is_recording;
    drop(app);

    h_flex()
        .id("transport_bar")
        .gap(px(4.0))
        .items_center()
        .child(
            Button::new("tb_start")
                .label("⏮")
                .compact()
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, _| {
                        state.transport.sample_count = 0;
                    });
                }),
        )
        .child(
            Button::new("tb_play")
                .label("▶")
                .compact()
                .when(is_playing, |b| b.color(rgb(0x28B450)))
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, _| {
                        state.transport.playback_state = hikaru_transport::transport_state::TransportPlaybackState::Playing;
                        state.audio_proxy.send(GuiCommand::Play);
                    });
                }),
        )
        .child(
            Button::new("tb_stop")
                .label("⏹")
                .compact()
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, _| {
                        state.transport.playback_state = hikaru_transport::transport_state::TransportPlaybackState::Stopped;
                        state.audio_proxy.send(GuiCommand::Stop);
                    });
                }),
        )
        .child(
            Button::new("tb_rec")
                .label("⏺")
                .compact()
                .when(is_recording, |b| b.color(rgb(0xDC1E1E)))
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, cx| {
                        state.is_recording = !state.is_recording;
                        cx.notify();
                    });
                }),
        )
}
