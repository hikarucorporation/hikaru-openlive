use gpui_kit::component::*;
use gpui_kit::component::button::Button;
use gpui_kit::component::label::Label;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::Styled as _;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, AppMode, HikaruApp};
use crate::audio_proxy::GuiCommand;
use crate::views::open_wavetable::{WavetableOscillator, ModulatorNode};
use crate::views::open_dms::OpenDms;

#[derive(Clone, Debug)]
pub struct DspSlot {
    pub id: usize,
    pub name: String,
    pub active: bool,
    pub is_open: bool,
    pub cam_x: f32,
    pub cam_y: f32,
    pub cam_z: f32,
    pub wavetable_oscillators: Vec<WavetableOscillator>,
    pub modulators: Vec<ModulatorNode>,
    pub dms_state: Option<OpenDms>,
}

impl DspSlot {
    pub fn new(id: usize, name: String) -> Self {
        Self {
            id,
            name,
            active: true,
            is_open: false,
            cam_x: 0.35,
            cam_y: 0.0,
            cam_z: 0.5,
            wavetable_oscillators: vec![WavetableOscillator::new(
                0,
                "OSC A",
                point(px(40.0), px(60.0)),
            )],
            modulators: Vec::new(),
            dms_state: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SendConnection {
    pub target_id: usize,
    pub amount: f32,
}

#[derive(Clone, Debug)]
pub struct Track {
    pub id: usize,
    pub name: String,
    pub volume: f32,
    pub pan: f32,
    pub pan_mode: crate::app::PanMode,
    pub mute: bool,
    pub solo: bool,
    pub is_master: bool,
    pub route_destination_id: usize,
    pub sends: Vec<SendConnection>,
    pub effects: Vec<DspSlot>,
    pub matrix_idx: Option<usize>,
}

impl Track {
    pub fn new(id: usize, name: String, is_master: bool) -> Self {
        Self {
            id,
            name,
            volume: 0.75,
            pan: 0.0,
            pan_mode: crate::app::PanMode::Stereo,
            mute: false,
            solo: false,
            is_master,
            route_destination_id: 0,
            sends: Vec::new(),
            effects: Vec::new(),
            matrix_idx: None,
        }
    }
}

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

pub fn render(cx: &mut Context<HikaruApp>) -> impl IntoElement {
    let app = state(cx).read(cx);
    let mode = app.mode;
    let selected_idx = app.selected_track_index;
    let tracks = match mode {
        AppMode::OpenLive => &app.live_tracks,
        AppMode::OpenStudio => &app.studio_tracks,
    };
    let raw_master = f32::from_bits(app.output_level_bits.load(std::sync::atomic::Ordering::Relaxed));
    let master_level = if raw_master.is_finite() { raw_master } else { 0.0 };
    let mut track_peaks = [0.0f32; 16];
    for (i, arc) in app.track_peak_bits.iter().enumerate().take(16) {
        let raw = f32::from_bits(arc.load(std::sync::atomic::Ordering::Relaxed));
        let raw = if raw.is_finite() { raw } else { 0.0 };
        track_peaks[i] = raw;
    }
    drop(app);

    let is_live = mode == AppMode::OpenLive;

    v_flex()
        .id("mixer")
        .size_full()
        .bg(rgb(0x14141A))
        .p(px(8.0))
        .gap(px(6.0))
        .child(
            h_flex()
                .items_center()
                .gap(px(6.0))
                .child(
                    Button::new("mixer_openlive").rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("OPENLIVE")
                        .compact()
                        .when(is_live, |b| b.text_color(rgb(0x00B4D8)))
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            st.update(cx, |state, _| {
                                state.mode = AppMode::OpenLive;
                            });
                        }),
                )
                .child(
                    Button::new("mixer_openstudio").rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("OPENSTUDIO")
                        .compact()
                        .when(!is_live, |b| b.text_color(rgb(0xFF6E00)))
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            st.update(cx, |state, _| {
                                state.mode = AppMode::OpenStudio;
                            });
                        }),
                )
                .child(
                    Button::new("mixer_add_track").rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label(" [ + ] ")
                        .compact()
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            st.update(cx, |state, cx| {
                                let new_id = state
                                    .live_tracks
                                    .iter()
                                    .map(|t| t.id)
                                    .max()
                                    .unwrap_or(0)
                                    + 1;
                                state
                                    .live_tracks
                                    .push(Track::new(new_id, format!("Track {:02}", new_id), false));
                                cx.notify();
                            });
                        }),
                )
                .child(
                    Button::new("mixer_remove_track").rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label(" [ - ] ")
                        .compact()
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            st.update(cx, |state, cx| {
                                if state.live_tracks.len() > 1 {
                                    state.live_tracks.pop();
                                }
                                cx.notify();
                            });
                        }),
                )
                .child(Label::new(format!("TOTAL: {}", tracks.len())).text_xs()),
        )
        .child(
            div()
                .flex_1()
                .overflow_x_scrollbar()
                .child(
                    h_flex()
                        .gap(px(6.0))
                        .children(tracks.iter().enumerate().map(|(idx, track)| {
                            let is_sel = idx == selected_idx;
                            let is_master = track.is_master;
                            let name = track.name.clone();
                            let volume = track.volume;
                            let pan = track.pan;
                            let mute = track.mute;
                            let solo = track.solo;
                            let vu = if is_master {
                                master_level
                            } else if let Some(mx) = track.matrix_idx {
                                track_peaks.get(mx).copied().unwrap_or(0.0)
                            } else {
                                0.0
                            };

                            v_flex()
                                .w(px(85.0))
                                .h_full()
                                 .bg(if is_master {
                                     rgb(0x1E1E2D)
                                 } else if is_sel {
                                     rgb(0x232323)
                                 } else {
                                     rgb(0x19191E)
                                 })
                                 .border_1()
                                 .border_color(if is_sel {
                                     rgb(0xFF6E00)
                                 } else {
                                     rgb(0x282828)
                                })
                                .rounded(px(4.0))
                                .p(px(8.0))
                                .gap(px(4.0))
                                .items_center()
                                .child(
                                    Button::new(format!("mixer_sel_{}", idx)).rounded(gpui_kit::component::button::ButtonRounded::None)
                                        .label(if is_master {
                                            "MASTER".to_string()
                                        } else {
                                            format!("TRK {:02}: {}", track.id, name)
                                        })
                                        .compact()
                                        .w_full()
                                        .when(is_sel, |b| b.text_color(rgb(0x0078D7)))
                                        .on_click(move |_, _, cx| {
                                            let st = state(cx);
                                            st.update(cx, |state, cx| {
                                                state.selected_track_index = idx;
                                                cx.notify();
                                            });
                                        }),
                                )
                                .child(
                                    Button::new(format!("mixer_panmode_{}", idx)).rounded(gpui_kit::component::button::ButtonRounded::None)
                                        .label(if track.pan_mode == crate::app::PanMode::MidSide {
                                            "M/S"
                                        } else {
                                            "L/R"
                                        })
                                        .compact()
                                        .on_click(move |_, _, cx| {
                                            let st = state(cx);
                                            st.update(cx, |state, cx| {
                                                let t = if idx < state.live_tracks.len() {
                                                    Some(&mut state.live_tracks[idx])
                                                } else {
                                                    None
                                                };
                                                if let Some(t) = t {
                                                    t.pan_mode = match t.pan_mode {
                                                        crate::app::PanMode::MidSide => crate::app::PanMode::Stereo,
                                                        crate::app::PanMode::Stereo => crate::app::PanMode::MidSide,
                                                    };
                                                }
                                                cx.notify();
                                            });
                                        }),
                                )
                                .child(
                                    canvas(
                                        |_, _, _| {},
                                        move |bounds, _, window, _| {
                                            let cx0 = bounds.origin.x + px(bounds.size.width.as_f32() / 2.0);
                                            let cy = bounds.origin.y + px(bounds.size.height.as_f32() / 2.0);
                                            let r = px(bounds.size.width.as_f32().min(bounds.size.height.as_f32()) / 2.0 - px(2.0).as_f32());
                                            let mut path = PathBuilder::stroke(px(1.5));
                                            path.arc_to(
                                                point(r, r),
                                                px(0.0),
                                                false,
                                                true,
                                                point(cx0, cy),
                                            );
                                            window.paint_path(path.build().unwrap(), rgb(0x505055));
                                            let norm = ((pan.clamp(-1.0, 1.0) + 1.0) / 2.0).clamp(0.0, 1.0);
                                            let angle = (norm - 0.5) * (std::f32::consts::PI * 1.5);
                                            let hx = cx0 + px(angle.sin() * (r.as_f32() - px(4.0).as_f32()));
                                            let hy = cy - px(angle.cos() * (r.as_f32() - px(4.0).as_f32()));
                                            let mut path2 = PathBuilder::stroke(px(2.0));
                                            path2.move_to(point(cx0, cy));
                                            path2.line_to(point(hx, hy));
                                            window.paint_path(path2.build().unwrap(), rgb(0xFF6E00));
                                        },
                                    )
                                    .w(px(28.0))
                                    .h(px(28.0))
                                    .into_any_element(),
                                )
                                .child(Label::new(format!("{:+.0}", pan * 100.0)).text_xs())
                                .child(
                                    canvas(
                                        |_, _, _| {},
                                        move |bounds, _, window, _| {
                                            let fx = bounds.origin.x;
                                            let fy = bounds.origin.y;
                                            let fw = bounds.size.width;
                                            let fh = bounds.size.height;
                                            window.paint_quad(PaintQuad {
                                                bounds: Bounds::new(
                                                    point(fx, fy),
                                                    size(fw, fh),
                                                ),
                                                background: rgb(0x0A0A0A).into(),
                                                border_color: Hsla::default(),
                                                corner_radii: gpui_kit::Corners::default(),
                                                border_widths: gpui_kit::Edges::default(),
                                                border_style: BorderStyle::default(),
                                            });
                                            let level = vu.clamp(0.0, 1.0);
                                            let sig_h = fh * level;
                                            let col = if level > 0.9 {
                                                rgb(0xFF3C3C)
                                            } else if level > 0.75 {
                                                rgb(0xFFC800)
                                            } else {
                                                rgb(0x00FF64)
                                            };
                                            window.paint_quad(PaintQuad {
                                                bounds: Bounds::new(
                                                    point(fx, fy + fh - sig_h),
                                                    size(fw, sig_h),
                                                ),
                                                background: col.into(),
                                                border_color: Hsla::default(),
                                                corner_radii: gpui_kit::Corners::default(),
                                                border_widths: gpui_kit::Edges::default(),
                                                border_style: BorderStyle::default(),
                                            });
                                        },
                                    )
                                    .w(px(14.0))
                                    .h_full()
                                    .min_h(px(80.0))
                                    .into_any_element(),
                                )
                                .child(
                                    div()
                                        .w(px(24.0))
                                        .h_full()
                                        .min_h(px(120.0))
                                        .on_mouse_down(gpui_kit::MouseButton::Left, move |event, _, cx| {
                                            let y = event.position.y.as_f32();
                                            let fh = 120.0_f32;
                                            let vol = (1.0 - (y / fh)).clamp(0.0, 1.0);
                                            let st = state(cx);
                                            st.update(&mut *cx, |state, cx| {
                                                let t = if idx < state.live_tracks.len() {
                                                    Some(&mut state.live_tracks[idx])
                                                } else {
                                                    None
                                                };
                                                if let Some(t) = t {
                                                    t.volume = vol;
                                                }
                                                cx.notify();
                                            });
                                        })
                                        .child(
                                            canvas(
                                                |_, _, _| {},
                                                move |bounds, _, window, _| {
                                                    let fx = bounds.origin.x;
                                                    let fw = bounds.size.width;
                                                    let fh = bounds.size.height;
                                                    window.paint_quad(PaintQuad {
                                                        bounds,
                                                        background: rgb(0x2D2D2D).into(),
                                                        border_color: Hsla::default(),
                                                        corner_radii: gpui_kit::Corners::default(),
                                                        border_widths: gpui_kit::Edges::default(),
                                                        border_style: BorderStyle::default(),
                                                    });
                                                    let ty = fx + fh - volume.clamp(0.0, 1.0) * fh;
                                                    window.paint_quad(PaintQuad {
                                                        bounds: Bounds::new(
                                                            point(fx, ty),
                                                            size(fw, fh - (ty - fx)),
                                                        ),
                                                        background: rgb(0x0096BE).into(),
                                                        border_color: Hsla::default(),
                                                        corner_radii: gpui_kit::Corners::default(),
                                                        border_widths: gpui_kit::Edges::default(),
                                                        border_style: BorderStyle::default(),
                                                    });
                                                    window.paint_quad(PaintQuad {
                                                        bounds: Bounds::new(
                                                            point(fx - px(4.0), ty),
                                                            size(fw + px(8.0), px(8.0)),
                                                        ),
                                                        background: rgb(0x00A2E8).into(),
                                                        border_color: Hsla::default(),
                                                        corner_radii: gpui_kit::Corners::default(),
                                                        border_widths: gpui_kit::Edges::default(),
                                                        border_style: BorderStyle::default(),
                                                    });
                                                },
                                            )
                                        ),
                                )
                                .child(Label::new(db_text(volume)).text_xs())
                                .child(
                                    h_flex()
                                        .gap(px(4.0))
                                        .child(
                                            Button::new(format!("mixer_mute_{}", idx)).rounded(gpui_kit::component::button::ButtonRounded::None)
                                                .label("M")
                                                .compact()
                                                .when(mute, |b| b.text_color(rgb(0xC80000)))
                                                .on_click(move |_, _, cx| {
                                                    let st = state(cx);
                                                    st.update(cx, |state, cx| {
                                                        let t = if idx < state.live_tracks.len() {
                                                            Some(&mut state.live_tracks[idx])
                                                        } else {
                                                            None
                                                        };
                                                        if let Some(t) = t {
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
                                        .child(
                                            Button::new(format!("mixer_solo_{}", idx)).rounded(gpui_kit::component::button::ButtonRounded::None)
                                                .label("S")
                                                .compact()
                                                .when(solo, |b| b.text_color(rgb(0xC8A000)))
                                                .on_click(move |_, _, cx| {
                                                    let st = state(cx);
                                                    st.update(cx, |state, cx| {
                                                        let t = if idx < state.live_tracks.len() {
                                                            Some(&mut state.live_tracks[idx])
                                                        } else {
                                                            None
                                                        };
                                                        if let Some(t) = t {
                                                            t.solo = !t.solo;
                                                            state.audio_proxy.send(GuiCommand::SetTrackSolo {
                                                                track_idx: idx,
                                                                solo: t.solo,
                                                            });
                                                        }
                                                        cx.notify();
                                                    });
                                                }),
                                        ),
                                )
                        })),
                ),
        )
}
