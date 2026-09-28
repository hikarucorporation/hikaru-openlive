use gpui_kit::component::*;
use gpui_kit::component::button::Button;
use gpui_kit::component::label::Label;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, AppMode, HikaruApp};
use crate::audio_proxy::{AudioClipData, GuiCommand};
use crate::views::playlist::ClipType;
use hikaru_transport::transport_state::TransportPlaybackState;

fn format_timecode(bars: f32, bpm: f64) -> String {
    let beats = (bars - 1.0).max(0.0) * 4.0;
    let total_seconds = (beats / bpm as f32) * 60.0;

    let hours = (total_seconds / 3600.0) as u32;
    let minutes = ((total_seconds % 3600.0) / 60.0) as u32;
    let seconds = (total_seconds % 60.0) as u32;
    let millis = (total_seconds.fract() * 1000.0) as u32;

    format!("{:02}:{:02}:{:02}.{:03}", hours, minutes, seconds, millis)
}

pub fn render(cx: &mut Context<HikaruApp>) -> impl IntoElement {
    let app = state(cx).read(cx);
    let transport = &app.transport;
    let is_playing = transport.playback_state == TransportPlaybackState::Playing;
    let is_paused = transport.playback_state == TransportPlaybackState::Paused;
    let is_recording = app.is_recording;
    let is_looping = app.is_looping;
    let is_live = app.mode == AppMode::OpenLive;
    let is_studio = app.mode == AppMode::OpenStudio;
    let show_mixer = app.show_mixer;
    let show_dsp_rack = app.show_dsp_rack;
    let show_explorer = app.show_explorer;
    let bpm = transport.bpm;
    let samples_per_beat = (transport.sample_rate.get() as f64 * 60.0) / transport.bpm;
    let samples_per_bar = samples_per_beat * transport.beats_per_bar as f64;
    let current_bar = 1.0 + (transport.sample_count as f64 / samples_per_bar) as f32;
    let audio_proxy = app.audio_proxy.clone();
    drop(app);

    h_flex()
        .id("header_bar")
        .h(px(40.0))
        .bg(rgb(0x2A2A2A))
        .border_b_1()
        .border_color(rgb(0x3D3D3D))
        .items_center()
        .px(px(8.0))
        .gap(px(6.0))
        .child(
            Button::new("transport_rewind_start")
                .label("⏮")
                .compact()
                .bg(rgb(0x3D3D3D))
                .text_color(rgb(0xE0E0E0))
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, _| {
                        state.transport.sample_count = 0;
                        state.audio_proxy.send(GuiCommand::Seek { sample_count: 0 });
                    });
                }),
        )
        .child(
            Button::new("transport_rewind_bars")
                .label("⏪")
                .compact()
                .bg(rgb(0x3D3D3D))
                .text_color(rgb(0xE0E0E0))
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, _| {
                        let samples_to_sub = (samples_per_bar * 4.0) as u64;
                        state.transport.sample_count = state
                            .transport
                            .sample_count
                            .saturating_sub(samples_to_sub);
                        state.audio_proxy.send(GuiCommand::Seek {
                            sample_count: state.transport.sample_count,
                        });
                    });
                }),
        )
        .child(
            Button::new("transport_play")
                .label("▶")
                .compact()
                .bg(rgb(0x3D3D3D))
                .text_color(if is_playing {
                    rgb(0x4CAF50)
                } else {
                    rgb(0xE0E0E0)
                })
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, _| {
                        state.transport.playback_state = TransportPlaybackState::Playing;
                        state.audio_proxy.send(GuiCommand::StopPreview);
                        state.audio_proxy.send(GuiCommand::SetBpm(state.transport.bpm as f32));

                        if state.mode == AppMode::OpenStudio {
                            let sr = state.transport.sample_rate.get().max(1.0);
                            let clips_to_sync: Vec<AudioClipData> = state
                                .playlist_state
                                .clips
                                .iter()
                                .filter_map(|(track_id, clip)| {
                                    if let ClipType::Audio {
                                        sample_path,
                                        sample_offset_ticks,
                                        ..
                                    } = &clip.clip_type
                                    {
                                        let clip_length_frames =
                                            state.transport.clip_length_frames(clip.duration_ticks);
                                        let duration_secs = clip_length_frames as f32 / sr;
                                        Some(AudioClipData {
                                            clip_id: clip.id,
                                            path: sample_path.clone(),
                                            start_secs: state.transport.ticks_to_samples(clip.start_tick)
                                                as f32
                                                / sr,
                                            duration_secs,
                                            offset_secs: state
                                                .transport
                                                .ticks_to_samples(*sample_offset_ticks)
                                                as f32
                                                / sr,
                                            track_index: *track_id,
                                        })
                                    } else {
                                        None
                                    }
                                })
                                .collect();

                            state.audio_proxy.send(GuiCommand::SyncPlaylistClips { clips: clips_to_sync });
                        } else {
                            state.audio_proxy.send(GuiCommand::SetAppMode(false));
                        }

                        state.audio_proxy.send(GuiCommand::Seek {
                            sample_count: state.transport.sample_count,
                        });
                        state.audio_proxy.send(GuiCommand::Play);
                    });
                }),
        )
        .child(
            Button::new("transport_pause")
                .label("⏸")
                .compact()
                .bg(rgb(0x3D3D3D))
                .text_color(if is_paused {
                    rgb(0xFFC107)
                } else {
                    rgb(0xE0E0E0)
                })
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, _| {
                        if state.transport.playback_state == TransportPlaybackState::Playing {
                            state.transport.playback_state = TransportPlaybackState::Paused;
                            state.audio_proxy.send(GuiCommand::Pause);
                        } else if state.transport.playback_state == TransportPlaybackState::Paused {
                            state.transport.playback_state = TransportPlaybackState::Playing;
                            state.audio_proxy.send(GuiCommand::StopPreview);
                            state.audio_proxy.send(GuiCommand::SetBpm(state.transport.bpm as f32));
                            state.audio_proxy.send(GuiCommand::Play);
                        }
                    });
                }),
        )
        .child(
            Button::new("transport_stop")
                .label("⏹")
                .compact()
                .bg(rgb(0x3D3D3D))
                .text_color(rgb(0xE0E0E0))
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, _| {
                        state.transport.playback_state = TransportPlaybackState::Stopped;
                        state.transport.sample_count = 0;
                        state.audio_proxy.send(GuiCommand::Stop);
                    });
                }),
        )
        .child(
            Button::new("transport_record")
                .label("⏺")
                .compact()
                .bg(rgb(0x3D3D3D))
                .text_color(if is_recording {
                    rgb(0xFF5252)
                } else {
                    rgb(0xE0E0E0)
                })
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, cx| {
                        state.is_recording = !state.is_recording;
                        cx.notify();
                    });
                }),
        )
        .child(
            Button::new("transport_loop")
                .label("🔁")
                .compact()
                .bg(rgb(0x3D3D3D))
                .text_color(if is_looping {
                    rgb(0x00BCD4)
                } else {
                    rgb(0xE0E0E0)
                })
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, cx| {
                        state.is_looping = !state.is_looping;
                        cx.notify();
                    });
                }),
        )
        .child(
            Button::new("transport_forward_bars")
                .label("⏩")
                .compact()
                .bg(rgb(0x3D3D3D))
                .text_color(rgb(0xE0E0E0))
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, _| {
                        state.transport.sample_count += (samples_per_bar * 4.0) as u64;
                        state.audio_proxy.send(GuiCommand::Seek {
                            sample_count: state.transport.sample_count,
                        });
                    });
                }),
        )
        .child(
            Button::new("transport_forward_16bars")
                .label("⏭")
                .compact()
                .bg(rgb(0x3D3D3D))
                .text_color(rgb(0xE0E0E0))
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, _| {
                        state.transport.sample_count += (samples_per_bar * 16.0) as u64;
                        state.audio_proxy.send(GuiCommand::Seek {
                            sample_count: state.transport.sample_count,
                        });
                    });
                }),
        )
        .child(
            div()
                .bg(rgb(0x0F1216))
                .border_1()
                .border_color(rgb(0x2D3741))
                .rounded(px(4.0))
                .px(px(8.0))
                .py(px(5.0))
                .child(
                    Label::new(format_timecode(current_bar, bpm))
                        .text_sm()
                        .text_color(rgb(0x00DCFF)),
                ),
        )
        .child(
            Button::new("mode_openlive")
                .label("OPENLIVE")
                .compact()
                .bg(rgb(0x3D3D3D))
                .text_color(if is_live {
                    rgb(0x00BCD4)
                } else {
                    rgb(0xE0E0E0)
                })
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, _| {
                        if state.mode != AppMode::OpenLive {
                            state.mode = AppMode::OpenLive;
                            state.transport.playback_state = TransportPlaybackState::Stopped;
                            state.transport.sample_count = 0;
                            state.audio_proxy.send(GuiCommand::Stop);
                            state.audio_proxy.send(GuiCommand::SetAppMode(false));
                        }
                    });
                }),
        )
        .child(
            Button::new("mode_openstudio")
                .label("OPENSTUDIO")
                .compact()
                .bg(rgb(0x3D3D3D))
                .text_color(if is_studio {
                    rgb(0xFF9100)
                } else {
                    rgb(0xE0E0E0)
                })
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, _| {
                        if state.mode != AppMode::OpenStudio {
                            state.mode = AppMode::OpenStudio;
                            state.transport.playback_state = TransportPlaybackState::Stopped;
                            state.transport.sample_count = 0;
                            state.audio_proxy.send(GuiCommand::Stop);
                            state.audio_proxy.send(GuiCommand::SetAppMode(true));
                        }
                    });
                }),
        )
        .child(Label::new("BPM").text_xs().text_color(rgb(0xE0E0E0)))
        .child(
            Button::new("bpm_display")
                .label(format!("{:.0}", bpm))
                .compact()
                .bg(rgb(0x3D3D3D))
                .text_color(rgb(0xE0E0E0))
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, _| {
                        state.transport.bpm = (state.transport.bpm + 1.0).min(300.0);
                        state.audio_proxy.send(GuiCommand::SetBpm(state.transport.bpm as f32));
                    });
                }),
        )
        .child(div().flex_1())
        .child(
            Button::new("toggle_mixer")
                .label("MIXER (F9)")
                .compact()
                .bg(rgb(0x3D3D3D))
                .when(show_mixer, |this| this.text_color(rgb(0x00BCD4)))
                .when(!show_mixer, |this| this.text_color(rgb(0xE0E0E0)))
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, cx| {
                        state.show_mixer = !state.show_mixer;
                        cx.notify();
                    });
                }),
        )
        .child(
            Button::new("toggle_dsp_rack")
                .label("DSP RACK (F10)")
                .compact()
                .bg(rgb(0x3D3D3D))
                .when(show_dsp_rack, |this| this.text_color(rgb(0x00BCD4)))
                .when(!show_dsp_rack, |this| this.text_color(rgb(0xE0E0E0)))
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, cx| {
                        state.show_dsp_rack = !state.show_dsp_rack;
                        cx.notify();
                    });
                }),
        )
        .child(
            Button::new("toggle_explorer")
                .label("EXPLORER (F11)")
                .compact()
                .bg(rgb(0x3D3D3D))
                .when(show_explorer, |this| this.text_color(rgb(0x00BCD4)))
                .when(!show_explorer, |this| this.text_color(rgb(0xE0E0E0)))
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, cx| {
                        state.show_explorer = !state.show_explorer;
                        cx.notify();
                    });
                }),
        )
}
