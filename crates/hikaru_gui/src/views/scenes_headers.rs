use gpui_kit::component::*;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, HikaruApp};

pub fn render(cx: &mut Context<HikaruApp>) -> impl IntoElement {
    let app = state(cx).read(cx);
    let scenes = app.matrix_state.scenes.len();
    drop(app);

    h_flex()
        .id("scenes_headers")
        .gap(px(4.0))
        .children((0..scenes).map(|i| {
            Button::new(format!("scene_hdr_{}", i))
                .label(format!("▶ Scene {}", i + 1))
                .compact()
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, cx| {
                        crate::views::matrix::trigger_scene(
                            &mut state.matrix_state,
                            &state.audio_proxy,
                            i,
                        );
                        cx.notify();
                    });
                })
                .into_any_element()
        }))
}
