use gpui_kit::component::*;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, HikaruApp};
use crate::audio_proxy::{AudioProxy, GuiCommand};

pub fn render(cx: &mut Context<HikaruApp>) -> impl IntoElement {
    let app = state(cx).read(cx);
    let tracks = match app.mode {
        crate::app::AppMode::OpenLive => &app.live_tracks,
        crate::app::AppMode::OpenStudio => &app.studio_tracks,
    };
    let proxy = app.audio_proxy.clone();
    drop(app);

    v_flex()
        .id("mixer_view")
        .size_full()
        .child(Label::new("📊 Mixer").text_sm().font_weight(FontWeight::BOLD))
        .child(
            div()
                .flex_1()
                .overflow_x_scrollbar()
                .child(
                    h_flex()
                        .gap(px(4.0))
                        .children(tracks.iter().enumerate().map(|(idx, track)| {
                            let tid = track.id;
                            let tname = track.name.clone();
                            let tvol = track.volume;
                            let tpan = track.pan;
                            let tmuted = track.mute;
                            let proxy = proxy.clone();
                            v_flex()
                                .w(px(60.0))
                                .gap(px(4.0))
                                .items_center()
                                .child(Label::new(tname.clone()).text_xs())
                                .child(
                                    canvas(
                                        |_, _, _| {},
                                        move |bounds, _, window, _| {
                                            let fx: f32 = bounds.origin.x.into();
                                            let fy: f32 = bounds.origin.y.into();
                                            let fw: f32 = bounds.size.width.into();
                                            let fh: f32 = bounds.size.height.into();
                                            window.paint_quad(PaintQuad {
                                                bounds,
                                                background: rgb(0x2D2D2D).into(),
                                                border_color: Hsla::default(),
                                                corner_radii: gpui_kit::Corners::default(),
                                                border_widths: gpui_kit::Edges::default(),
                                                border_style: BorderStyle::default(),
                                            });
                                            let ty = fy + fh - tvol.clamp(0.0, 1.0) * fh;
                                            window.paint_quad(PaintQuad {
                                                bounds: Bounds::new(
                                                    point(px(fx), px(ty)),
                                                    size(px(fw), px(fh - (ty - fy))),
                                                ),
                                                background: rgb(0x0096BE).into(),
                                                border_color: Hsla::default(),
                                                corner_radii: gpui_kit::Corners::default(),
                                                border_widths: gpui_kit::Edges::default(),
                                                border_style: BorderStyle::default(),
                                            });
                                        },
                                    )
                                    .w_full()
                                    .h(px(120.0))
                                    .on_mouse_down(move |event, _, cx| {
                                        let y: f32 = event.position.y.into();
                                        let fh = 120.0_f32;
                                        let vol = (1.0 - y / fh).clamp(0.0, 1.0);
                                        let st = state(cx);
                                        st.update(cx, |state, cx| {
                                            if let Some(t) = state.live_tracks.iter_mut().find(|t| t.id == tid) {
                                                t.volume = vol;
                                            }
                                            cx.notify();
                                        });
                                    })
                                    .into_any_element(),
                                )
                                .child(
                                    Button::new(format!("mv_mute_{}", idx))
                                        .label("M")
                                        .compact()
                                        .when(tmuted, |b| b.color(rgb(0xC80000)))
                                        .on_click(move |_, _, cx| {
                                            let st = state(cx);
                                            st.update(cx, |state, cx| {
                                                if let Some(t) = state.live_tracks.iter_mut().find(|t| t.id == tid) {
                                                    t.mute = !t.mute;
                                                    state.audio_proxy.send(GuiCommand::SetTrackMute {
                                                        track_idx: idx,
                                                        mute: t.mute,
                                                    });
                                                }
                                                cx.notify();
                                            });
                                        }),
                                )
                        })),
                ),
        )
}
