// Copyright (C) Hikaru Corporation - 2026
// Hikaru OpenLive - Arranger View (Timeline View)
// GNU Affero General Public License v3
// crates/hikaru_gui/src/views/arranger_view.rs

use std::path::PathBuf;
use hikaru_audio_engine::AudioEngine;

use crate::audio_proxy::{AudioProxy, GuiCommand};
use crate::theme;

#[derive(Clone, Debug)]
pub struct AudioEvent {
    pub id: u64,
    pub name: String,
    pub samples: Vec<f32>,
    pub channels: usize,
    pub sample_rate: u32,
    pub start_secs: f64,
    pub trim_left_frames: usize,
    pub visible_frames: usize,
    pub gain: f32,
    pub fade_in_secs: f64,
    pub fade_out_secs: f64,
}

impl AudioEvent {
    pub fn new_full(
        id: u64,
        name: String,
        samples: Vec<f32>,
        channels: usize,
        sample_rate: u32,
        start_secs: f64,
    ) -> Self {
        let visible = samples.len() / channels.max(1);
        Self {
            id,
            name,
            samples,
            channels,
            sample_rate,
            start_secs,
            trim_left_frames: 0,
            visible_frames: visible,
            gain: 1.0,
            fade_in_secs: 0.0,
            fade_out_secs: 0.0,
        }
    }

    pub fn total_frames(&self) -> usize {
        self.samples.len() / self.channels.max(1)
    }

    pub fn frames(&self) -> u64 {
        let avail = self.total_frames().saturating_sub(self.trim_left_frames);
        avail.min(self.visible_frames) as u64
    }

    pub fn duration_secs(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.frames() as f64 / self.sample_rate as f64
    }
}

#[derive(Clone, Debug)]
pub enum ClipData {
    Audio {
        events: Vec<AudioEvent>,
        next_event_id: u64,
    },
    Midi {
        notes: Vec<(u64, u8, u8, u32)>,
    },
}

#[derive(Clone, Debug)]
pub struct ArrangerClip {
    pub id: u64,
    pub name: String,
    pub start_secs: f64,
    pub duration_secs: f64,
    pub content: ClipData,
}

#[derive(Clone, Debug)]
pub struct ArrangerTrackMeta {
    pub id: usize,
    pub name: String,
    pub muted: bool,
    pub soloed: bool,
    pub volume: f32,
    pub pan: f32,
    pub clips: Vec<ArrangerClip>,
}

pub struct ArrangerViewState {
    pub tracks: Vec<ArrangerTrackMeta>,
    pub next_clip_id: u64,
    pub selected_clip: Option<(usize, usize)>,
    pub playhead_secs: f64,
    pub is_playing: bool,
    pub pixels_per_sec: f32,
}

impl Default for ArrangerViewState {
    fn default() -> Self {
        let initial_tracks = 8;
        let tracks = (0..initial_tracks)
            .map(|i| ArrangerTrackMeta {
                id: i,
                name: format!("Track {}", i + 1),
                muted: false,
                soloed: false,
                volume: 0.75,
                pan: 0.0,
                clips: Vec::new(),
            })
            .collect();

        Self {
            tracks,
            next_clip_id: 1,
            selected_clip: None,
            playhead_secs: 0.0,
            is_playing: false,
            pixels_per_sec: 50.0,
        }
    }
}

impl ArrangerViewState {
    pub fn add_track(&mut self) {
        let track_num = self.tracks.len() + 1;
        self.tracks.push(ArrangerTrackMeta {
            id: self.tracks.len(),
            name: format!("Audio {}", track_num),
            muted: false,
            soloed: false,
            volume: 0.75,
            pan: 0.0,
            clips: Vec::new(),
        });
    }

    pub fn remove_track(&mut self) {
        if self.tracks.len() > 1 {
            self.tracks.pop();
            if let Some((t, _)) = self.selected_clip {
                if t >= self.tracks.len() {
                    self.selected_clip = None;
                }
            }
        }
    }
}

use gpui_kit::component::button::Button;
use gpui_kit::component::label::Label;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::prelude::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, HikaruApp};

fn db_text(volume: f32) -> String {
    let db_val = if volume <= 0.0 {
        -60.0
    } else if volume <= 0.75 {
        -60.0 + (volume / 0.75) * 60.0
    } else {
        ((volume - 0.75) / 0.25) * 6.0
    };
    format!("{:.1} dB", db_val)
}

pub fn render(cx: &mut Context<HikaruApp>) -> AnyElement {
    let app = state(cx).read(cx);
    let arranger = &app.arranger_state;
    drop(app);

    let tracks_len = arranger.tracks.len();
    let pps = arranger.pixels_per_sec;
    let playhead = arranger.playhead_secs;

    let mut track_headers: Vec<AnyElement> = Vec::new();
    let mut track_lanes: Vec<AnyElement> = Vec::new();

    for track_idx in 0..tracks_len {
        let meta = &arranger.tracks[track_idx];
        let track_name = meta.name.clone();
        let track_muted = meta.muted;
        let track_soloed = meta.soloed;
        let track_volume = meta.volume;

        // Cabezal de pista
        track_headers.push(
            v_flex()
                .w(px(160.0))
                .h(px(64.0))
                .bg(theme::PANEL_BG)
                .border_1()
                .border_color(theme::BORDER_COLOR)
                .p(px(6.0))
                .justify_between()
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .child(Label::new(track_name).text_xs().font_weight(FontWeight::BOLD))
                        .child(
                            h_flex()
                                .gap(px(2.0))
                                .child(
                                    Button::new(format!("arr_solo_{}", track_idx))
                                        .label("S")
                                        .compact()
                                        .when(track_soloed, |b| b.text_color(rgb(0xEAB308)))
                                        .on_click(move |_, _, cx| {
                                            let st = state(cx);
                                            st.update(cx, |app, cx| {
                                                app.arranger_state.tracks[track_idx].soloed =
                                                    !app.arranger_state.tracks[track_idx].soloed;
                                                app.audio_proxy.send(GuiCommand::SetTrackSolo {
                                                    track_idx,
                                                    solo: app.arranger_state.tracks[track_idx].soloed,
                                                });
                                                cx.notify();
                                            });
                                        }),
                                )
                                .child(
                                    Button::new(format!("arr_mute_{}", track_idx))
                                        .label("M")
                                        .compact()
                                        .when(track_muted, |b| b.text_color(rgb(0xEF4444)))
                                        .on_click(move |_, _, cx| {
                                            let st = state(cx);
                                            st.update(cx, |app, cx| {
                                                app.arranger_state.tracks[track_idx].muted =
                                                    !app.arranger_state.tracks[track_idx].muted;
                                                app.audio_proxy.send(GuiCommand::SetTrackMute {
                                                    track_idx,
                                                    mute: app.arranger_state.tracks[track_idx].muted,
                                                });
                                                cx.notify();
                                            });
                                        }),
                                ),
                        ),
                )
                .child(Label::new(db_text(track_volume)).text_xs().text_color(rgb(0x71717A)))
                .into_any_element(),
        );

        // Clips en la carril de tiempo
        let mut clip_elements: Vec<AnyElement> = Vec::new();
        for (clip_idx, clip) in meta.clips.iter().enumerate() {
            let clip_left = (clip.start_secs * pps as f64) as f32;
            let clip_width = (clip.duration_secs * pps as f64).max(20.0) as f32;
            let clip_name = clip.name.clone();

            clip_elements.push(
                Button::new(format!("clip_{}_{}", track_idx, clip_idx))
                    .label(clip_name)
                    .compact()
                    .on_click(move |_, _, cx| {
                        let st = state(cx);
                        st.update(cx, |app, cx| {
                            app.arranger_state.selected_clip = Some((track_idx, clip_idx));
                            cx.notify();
                        });
                    })
                    .into_any_element(),
            );
        }

        track_lanes.push(
            div()
                .relative()
                .w_full()
                .h(px(64.0))
                .bg(theme::SURFACE_BG)
                .border_1()
                .border_color(theme::BORDER_COLOR)
                .children(clip_elements)
                .into_any_element(),
        );
    }

    let playhead_x = (playhead * pps as f64) as f32;

    v_flex()
        .id("arranger_view")
        .size_full()
        .gap(px(4.0))
        .p(px(6.0))
        .bg(theme::WINDOW_BG)
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .p(px(4.0))
                .child(
                    h_flex()
                        .gap(px(8.0))
                        .items_center()
                        .child(Label::new("ARRANGER VIEW").text_sm().font_weight(FontWeight::BOLD))
                        .child(
                            Button::new("arr_add_track")
                                .label("+ Track")
                                .compact()
                                .on_click(move |_, _, cx| {
                                    let st = state(cx);
                                    st.update(cx, |app, cx| {
                                        app.arranger_state.add_track();
                                        cx.notify();
                                    });
                                }),
                        )
                        .child(
                            Button::new("arr_remove_track")
                                .label("- Track")
                                .compact()
                                .on_click(move |_, _, cx| {
                                    let st = state(cx);
                                    st.update(cx, |app, cx| {
                                        app.arranger_state.remove_track();
                                        cx.notify();
                                    });
                                }),
                        ),
                ),
        )
        .child(
            h_flex()
                .size_full()
                .child(
                    v_flex()
                        .w(px(160.0))
                        .gap(px(2.0))
                        .child(div().h(px(24.0)).bg(theme::PANEL_BG))
                        .children(track_headers),
                )
                .child(
                    v_flex()
                        .size_full()
                        .overflow_x_scrollbar()
                        .child(
                            div()
                                .w_full()
                                .h(px(24.0))
                                .bg(theme::PANEL_BG)
                                .border_1()
                                .border_color(theme::BORDER_COLOR)
                                .child(
                                    Label::new(format!("Playhead: {:.2}s", playhead))
                                        .text_xs()
                                        .text_color(rgb(0xA1A1AA)),
                                ),
                        )
                        .child(
                            div()
                                .relative()
                                .size_full()
                                .child(v_flex().w_full().gap(px(2.0)).children(track_lanes))
                                .child(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .bottom_0()
                                        .left(px(playhead_x))
                                        .w(px(2.0))
                                        .bg(rgb(0xEF4444)),
                                ),
                        ),
                ),
        )
        .into_any_element()
}