use gpui_kit::component::*;
use gpui_kit::component::button::Button;
use gpui_kit::component::label::Label;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, HikaruApp};
use crate::views::mixer::DspSlot;
use crate::views::open_dms;

pub fn render(cx: &mut Context<HikaruApp>) -> impl IntoElement {
    let app = state(cx).read(cx);
    let selected_idx = app.selected_track_index;
    let selected_slot = app.selected_slot_index;
    let selected_matrix_slot = app.matrix_state.selected_slot;
    let dragged_sample = app.dragged_sample.clone();
    let audio_proxy = app.audio_proxy.clone();
    let tracks = match app.mode {
        crate::app::AppMode::OpenLive => &app.live_tracks,
        crate::app::AppMode::OpenStudio => &app.studio_tracks,
    };
    let track_idx = selected_idx.min(tracks.len().saturating_sub(1));
    let track = &tracks[track_idx];
    let track_name = track.name.clone();
    let effects = track.effects.clone();
    drop(app);

    let rack_title = if let Some((_t, s)) = selected_matrix_slot {
        format!("DSP RACK: {} | Scene {}", track_name, s + 1)
    } else {
        format!("DSP RACK: {}", track_name)
    };

    h_flex()
        .id("dsp_rack")
        .size_full()
        .bg(rgb(0x14141A))
        .p(px(8.0))
        .gap(px(8.0))
        .child(
            v_flex()
                .gap(px(4.0))
                .child(Label::new(rack_title).text_sm().font_weight(FontWeight::BOLD))
                .child(
                    Button::new("dsp_add_slot")
                        .label(" [ + ] Add Slot ")
                        .compact()
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            st.update(cx, |state, cx| {
                                let tracks = match state.mode {
                                    crate::app::AppMode::OpenLive => &mut state.live_tracks,
                                    crate::app::AppMode::OpenStudio => &mut state.studio_tracks,
                                };
                                if let Some(track) = tracks.get_mut(track_idx) {
                                    let new_id = track.effects.len();
                                    track.effects.push(DspSlot::new(new_id, "Empty Slot".to_string()));
                                    state.selected_slot_index = track.effects.len() - 1;
                                }
                                cx.notify();
                            });
                        }),
                )
                .child(
                    Button::new("dsp_remove_slot")
                        .label(" [ - ] Remove ")
                        .compact()
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            st.update(cx, |state, cx| {
                                let tracks = match state.mode {
                                    crate::app::AppMode::OpenLive => &mut state.live_tracks,
                                    crate::app::AppMode::OpenStudio => &mut state.studio_tracks,
                                };
                                if let Some(track) = tracks.get_mut(track_idx) {
                                    track.effects.pop();
                                    if state.selected_slot_index >= track.effects.len()
                                        && !track.effects.is_empty()
                                    {
                                        state.selected_slot_index = track.effects.len() - 1;
                                    }
                                }
                                cx.notify();
                            });
                        }),
                ),
        )
        .child(
            div()
                .flex_1()
                .overflow_x_scrollbar()
                .child(
                    h_flex()
                        .gap(px(6.0))
                        .children(effects.iter().enumerate().map(|(idx, slot)| {
                            let is_sel = idx == selected_slot;
                            let slot_name = slot.name.clone();
                            let slot_active = slot.active;
                            let slot_open = slot.is_open;
                            let card_w = match slot.name.as_str() {
                                "OpenWavetable" => 240.0,
                                "Hikaru OpenDMS" => 540.0,
                                "OpenSpectralFX" => 200.0,
                                "Empty Slot" => 170.0,
                                _ => 190.0,
                            };
                            let card_h = if slot.name == "Hikaru OpenDMS" { 220.0 } else { 100.0 };

                            v_flex()
                                .w(px(card_w))
                                .h(px(card_h))
                                .bg(if is_sel { rgb(0x28282D) } else { rgb(0x19191C) })
                                .border_1()
                                .border_color(if is_sel { rgb(0xFF6E00) } else { rgb(0x323232) })
                                .rounded(px(4.0))
                                .p(px(6.0))
                                .gap(px(4.0))
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap(px(4.0))
                                        .child(
                                            Button::new(format!("dsp_slot_num_{}", idx))
                                                .label(format!("{:02}", idx + 1))
                                                .compact()
                                                .on_click(move |_, _, cx| {
                                                    let st = state(cx);
                                                    st.update(cx, |state, cx| {
                                                        state.selected_slot_index = idx;
                                                        cx.notify();
                                                    });
                                                }),
                                        )
                                        .child(Label::new(slot_name.clone()).text_xs())
                                        .child(div().flex_1())
                                        .child(
                                            Button::new(format!("dsp_slot_active_{}", idx))
                                                .label(if slot_active { "●" } else { "○" })
                                                .compact()
                                                .on_click(move |_, _, cx| {
                                                    let st = state(cx);
                                                    st.update(cx, |state, cx| {
                                                        let tracks = match state.mode {
                                                            crate::app::AppMode::OpenLive => &mut state.live_tracks,
                                                            crate::app::AppMode::OpenStudio => &mut state.studio_tracks,
                                                        };
                                                        if let Some(track) = tracks.get_mut(track_idx) {
                                                            if let Some(s) = track.effects.get_mut(idx) {
                                                                s.active = !s.active;
                                                            }
                                                        }
                                                        cx.notify();
                                                    });
                                                }),
                                        )
                                        .child(
                                            Button::new(format!("dsp_slot_open_{}", idx))
                                                .label(if slot_open { "▣" } else { "□" })
                                                .compact()
                                                .on_click(move |_, _, cx| {
                                                    let st = state(cx);
                                                    st.update(cx, |state, cx| {
                                                        let tracks = match state.mode {
                                                            crate::app::AppMode::OpenLive => &mut state.live_tracks,
                                                            crate::app::AppMode::OpenStudio => &mut state.studio_tracks,
                                                        };
                                                        if let Some(track) = tracks.get_mut(track_idx) {
                                                            if let Some(s) = track.effects.get_mut(idx) {
                                                                s.is_open = !s.is_open;
                                                            }
                                                        }
                                                        cx.notify();
                                                    });
                                                }),
                                        ),
                                )
                                .child(match slot.name.as_str() {
                                    "OpenWavetable" => v_flex()
                                        .gap(px(2.0))
                                        .child(Label::new("Wavetable Synth").text_xs())
                                        .child(Label::new("WT Pos / Cutoff").text_xs())
                                        .into_any_element(),
                                    "Hikaru OpenDMS" => {
                                        let dms_state = slot.dms_state.clone();
                                        match dms_state {
                                            Some(dms) => open_dms::render_dms_compact(cx, &dms, &dragged_sample, &audio_proxy),
                                            None => Label::new("No DMS state").text_xs().into_any_element(),
                                        }
                                    }
                                    "OpenSpectralFX" => v_flex()
                                        .gap(px(2.0))
                                        .child(Label::new("Spectral Processor").text_xs())
                                        .child(Label::new("FFT Size / Mix").text_xs())
                                        .into_any_element(),
                                    _ => v_flex()
                                        .gap(px(2.0))
                                        .child(Label::new("Empty Slot").text_xs())
                                        .child(
                                            Button::new("dsp_empty_select")
                                                .label("Select Plugin")
                                                .compact()
                                                .on_click(move |_, _, cx| {
                                                    let st = state(cx);
                                                    st.update(cx, |state, cx| {
                                                        let tracks = match state.mode {
                                                            crate::app::AppMode::OpenLive => &mut state.live_tracks,
                                                            crate::app::AppMode::OpenStudio => &mut state.studio_tracks,
                                                        };
                                                        if let Some(track) = tracks.get_mut(track_idx) {
                                                            if let Some(s) = track.effects.get_mut(idx) {
                                                                s.name = "OpenWavetable".to_string();
                                                            }
                                                        }
                                                        cx.notify();
                                                    });
                                                }),
                                        )
                                        .into_any_element(),
                                })
                        })),
                ),
        )
}
