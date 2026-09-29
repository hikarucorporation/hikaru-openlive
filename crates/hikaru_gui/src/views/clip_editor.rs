use std::path::PathBuf;

use gpui_kit::component::*;
use gpui_kit::component::button::Button;
use gpui_kit::component::label::Label;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, HikaruApp};
use crate::audio_proxy::AudioProxy;
use crate::views::matrix::{self, AudioEvent, MatrixClip, SessionMatrixState};
use crate::views::waveform;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SnapValue {
    None,
    Bar1,
    Beat1_2,
    Beat1_4,
    Beat1_8,
    Beat1_16,
}

impl SnapValue {
    pub fn label(&self) -> &'static str {
        match self {
            SnapValue::None => "Off (Free)",
            SnapValue::Bar1 => "1 Bar",
            SnapValue::Beat1_2 => "1/2 Beat",
            SnapValue::Beat1_4 => "1/4 Beat",
            SnapValue::Beat1_8 => "1/8 Beat",
            SnapValue::Beat1_16 => "1/16 Beat",
        }
    }

    pub fn interval_secs(&self, bpm: f32) -> f64 {
        let current_bpm = if bpm > 0.0 { bpm as f64 } else { 120.0 };
        let sec_per_beat = 60.0 / current_bpm;

        match self {
            SnapValue::None => 0.0,
            SnapValue::Bar1 => sec_per_beat * 4.0,
            SnapValue::Beat1_2 => sec_per_beat * 2.0,
            SnapValue::Beat1_4 => sec_per_beat,
            SnapValue::Beat1_8 => sec_per_beat / 2.0,
            SnapValue::Beat1_16 => sec_per_beat / 4.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InspectorToolMode {
    AudioEvents,
    Comping,
    Stretch,
    Onsets,
    Gain,
    Pan,
    Pitch,
    Formant,
}

pub fn event_signature(clip: &MatrixClip) -> Vec<(u64, u64, u64, u32)> {
    clip.audio_events()
        .iter()
        .map(|e| {
            (
                e.id,
                (e.start_secs * 1_000_000.0).round() as u64,
                e.frames(),
                e.gain.to_bits(),
            )
        })
        .collect()
}

pub fn render(cx: &mut Context<HikaruApp>) -> AnyElement {
    let app = state(cx).read(cx);
    let sel = app.matrix_state.selected_slot;
    drop(app);

    let (clip_name, track_idx, scene_idx, has_clip, is_midi) = if let Some((t, s)) = sel {
        let st = state(cx).read(cx);
        let slot = &st.matrix_state.grid[t][s];
        let name = slot.clip.as_ref().map(|c| c.name.clone()).unwrap_or_default();
        let midi = matches!(
            slot.clip.as_ref().map(|c| &c.content),
            Some(matrix::ClipData::Midi { .. })
        );
        (name, t, s, slot.clip.is_some(), midi)
    } else {
        (String::new(), 0, 0, false, false)
    };

    if !has_clip {
        return v_flex()
            .id("clip_editor")
            .size_full()
            .items_center()
            .justify_center()
            .child(Label::new("Seleccioná un clip en la Session Matrix para editarlo.").text_sm())
            .into_any_element();
    }

    let st = state(cx);
    let (events_count, loop_enabled, bpm, sample_rate, duration) = {
        let app = st.read(cx);
        if let Some((t, s)) = sel {
            let clip = app.matrix_state.grid[t][s].clip.as_ref().unwrap();
            let evs = clip.audio_events().len();
            (evs, clip.loop_enabled, app.transport.bpm, app.transport.sample_rate.get() as u32, clip.duration_secs)
        } else {
            (0, false, 120.0, 44100, 0.0)
        }
    };

    let snap = SnapValue::Beat1_4;
    let tool = InspectorToolMode::AudioEvents;

    v_flex()
        .id("clip_editor")
        .size_full()
        .gap(px(4.0))
        .bg(rgb(0x121216))
        .p(px(6.0))
        .child(
            h_flex()
                .items_center()
                .gap(px(8.0))
                .child(Label::new(format!("🎵 {}", clip_name)).text_sm().font_weight(FontWeight::BOLD))
                .child(Label::new(format!("({:.2}s)", duration)).text_xs())
                .child(div().flex_1())
                .child(
                    Button::new("ce_loop_toggle").rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("🔁 Loop")
                        .compact()
                        .when(loop_enabled, |b| b.text_color(rgb(0x00B4DC)))
                        .on_click(move |_, _, cx| {
                            let st2 = state(cx);
                            st2.update(cx, |state, cx| {
                                if let Some((t, s)) = sel {
                                    if let Some(clip) = state.matrix_state.grid[t][s].clip.as_mut() {
                                        clip.loop_enabled = !clip.loop_enabled;
                                    }
                                }
                                cx.notify();
                            });
                        }),
                )
                .child(Label::new(format!("🧲 Snap: {}", snap.label())).text_xs()),
        )
        .child(
            h_flex()
                .items_center()
                .gap(px(6.0))
                .child(Label::new(format!("🧩 Events: {}", events_count)).text_xs())
                .child(
                    Button::new("ce_add").rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("➕ Add")
                        .compact()
                        .on_click(move |_, _, cx| {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("Audio Files", &["wav", "mp3", "flac", "ogg"])
                                .pick_file()
                            {
                                let st2 = state(cx);
                                st2.update(cx, |state, cx| {
                                    if let Some((t, s)) = sel {
                                        matrix::load_clip_into_slot(
                                            &mut state.matrix_state,
                                            &state.audio_proxy,
                                            t,
                                            s,
                                            path,
                                            state.transport.bpm,
                                        );
                                    }
                                    cx.notify();
                                });
                            }
                        }),
                ),
        )
        .child(
            h_flex()
                .gap(px(8.0))
                .child(
                    v_flex()
                        .w(px(130.0))
                        .bg(rgb(0x19191E))
                        .p(px(6.0))
                        .gap(px(4.0))
                        .child(Label::new("Inspector").text_xs().font_weight(FontWeight::BOLD))
                        .child(
                            Button::new("ce_tool_events").rounded(gpui_kit::component::button::ButtonRounded::None)
                                .label("Audio Events")
                                .compact()
                                .w_full(),
                        )
                        .child(
                            Button::new("ce_tool_comping").rounded(gpui_kit::component::button::ButtonRounded::None)
                                .label("Comping")
                                .compact()
                                .w_full(),
                        )
                        .child(
                            Button::new("ce_tool_stretch").rounded(gpui_kit::component::button::ButtonRounded::None)
                                .label("Stretch")
                                .compact()
                                .w_full(),
                        )
                        .child(
                            Button::new("ce_tool_onsets").rounded(gpui_kit::component::button::ButtonRounded::None)
                                .label("Onsets")
                                .compact()
                                .w_full(),
                        ),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(4.0))
                        .child(
                            h_flex()
                                .items_center()
                                .gap(px(6.0))
                                .child(Label::new("🔍").text_xs())
                                .child(
                                    Button::new("ce_zoom_out").rounded(gpui_kit::component::button::ButtonRounded::None)
                                        .label("➖")
                                        .compact()
                                        .on_click(move |_, _, cx| {}),
                                )
                                .child(Label::new("100%").text_xs())
                                .child(
                                    Button::new("ce_zoom_in").rounded(gpui_kit::component::button::ButtonRounded::None)
                                        .label("➕")
                                        .compact()
                                        .on_click(move |_, _, cx| {}),
                                )
                                .child(
                                    Button::new("ce_zoom_fit").rounded(gpui_kit::component::button::ButtonRounded::None)
                                        .label("Fit")
                                        .compact()
                                        .on_click(move |_, _, cx| {}),
                                ),
                        )
                        .child(render_clip_canvas(sel, cx)),
                ),
        )
        .into_any_element()
}

fn render_clip_canvas(sel: Option<(usize, usize)>, cx: &mut Context<HikaruApp>) -> AnyElement {
    let (clip_name, bpm, sample_rate, events) = {
        let app = state(cx).read(cx);
        if let Some((t, s)) = sel {
            if let Some(clip) = app.matrix_state.grid[t][s].clip.as_ref() {
                let evs: Vec<(f64, f64, Vec<f32>)> = clip
                    .audio_events()
                    .iter()
                    .map(|e| (e.start_secs, e.end_secs(), e.mono_mixed()))
                    .collect();
                (clip.name.clone(), app.transport.bpm, app.transport.sample_rate.get() as u32, evs)
            } else {
                (String::new(), 120.0, 44100, Vec::new())
            }
        } else {
            (String::new(), 120.0, 44100, Vec::new())
        }
    };

    let evs = events.clone();
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let bg = rgb(0x0C0C0F);
            window.paint_quad(PaintQuad {
                bounds,
                background: bg.into(),
                border_color: Hsla::default(),
                corner_radii: gpui_kit::Corners::default(),
                border_widths: gpui_kit::Edges::default(),
                border_style: BorderStyle::default(),
            });
            if evs.is_empty() {
                let cx0 = bounds.origin.x + px(bounds.size.width.as_f32() / 2.0);
                let cy = bounds.origin.y + px(bounds.size.height.as_f32() / 2.0);
                let tri = [
                    point(cx0 - px(6.0), cy - px(6.0)),
                    point(cx0 - px(6.0), cy + px(6.0)),
                    point(cx0 + px(6.0), cy),
                ];
                let mut tri_path = PathBuilder::fill();
                tri_path.add_polygon(&tri, true);
                window.paint_path(tri_path.build().unwrap(), rgb(0xFFFFFF));
                return;
            }
            let ruler_h = px(18.0);
            let wave_y = bounds.origin.y + ruler_h;
            let wave_h = bounds.size.height - ruler_h;
            let current_bpm = if bpm > 0.0 { bpm } else { 120.0 };
            let sec_per_bar = (60.0 / current_bpm) * 4.0;
            let visible_end = evs.iter().map(|(_, e, _)| *e).fold(0.0f64, f64::max);
            let px_per_sec = bounds.size.width.as_f32() / visible_end.max(1.0) as f32;
            let wave_rect = Bounds::new(
                point(bounds.origin.x, wave_y),
                size(bounds.size.width, wave_h),
            );
            let mut bar = 0;
            let mut bar_time = 0.0;
            while bar_time < visible_end {
                let x = bounds.origin.x + px((bar_time as f32 * px_per_sec).min(bounds.size.width.as_f32()));
                let mut bar_path = PathBuilder::stroke(px(1.0));
                bar_path.move_to(point(x, wave_y));
                bar_path.line_to(point(x, wave_y + wave_h));
                window.paint_path(bar_path.build().unwrap(), rgba(0xFFFFFF33));
                for beat in 1..4 {
                    let bt = bar_time + beat as f64 * (sec_per_bar / 4.0);
                    if bt >= visible_end {
                        break;
                    }
                    let bx = bounds.origin.x + px((bt as f32 * px_per_sec).min(bounds.size.width.as_f32()));
                    let mut beat_path = PathBuilder::stroke(px(1.0));
                    beat_path.move_to(point(bx, wave_y));
                    beat_path.line_to(point(bx, wave_y + wave_h));
                    window.paint_path(beat_path.build().unwrap(), rgba(0xFFFFFF14));
                }
                bar += 1;
                bar_time = bar as f64 * sec_per_bar;
            }
            for (es, ee, mono) in &evs {
                let ex0 = bounds.origin.x + px((*es as f32 * px_per_sec).min(bounds.size.width.as_f32()));
                let ex1 = bounds.origin.x + px((*ee as f32 * px_per_sec).min(bounds.size.width.as_f32()));
                let er = Bounds::new(
                    point(ex0, wave_y),
                    size((ex1 - ex0).max(px(1.0)), wave_h),
                );
                window.paint_quad(PaintQuad {
                    bounds: er,
                    background: rgb(0x123C78).into(),
                    border_color: Hsla::default(),
                    corner_radii: gpui_kit::Corners::default(),
                    border_widths: gpui_kit::Edges::default(),
                    border_style: BorderStyle::default(),
                });
                waveform::draw_waveform_bounds(window, er, mono, rgb(0x00C8FF).into());
            }
            let _ = clip_name;
            let _ = sample_rate;
        },
    )
    .w_full()
    .h(px(180.0))
    .into_any_element()
}
