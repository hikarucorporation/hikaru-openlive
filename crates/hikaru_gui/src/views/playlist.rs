use std::path::PathBuf;

use gpui_kit::component::*;
use gpui_kit::component::button::Button;
use gpui_kit::component::label::Label;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::prelude::StatefulInteractiveElement as _;
use gpui_kit::*;

use crate::app::{state, HikaruApp};
use crate::audio_proxy::{AudioProxy, GuiCommand};
use crate::views::mixer::Track;
use hikaru_transport::DEFAULT_PPQN;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewMode<'a> {
    Arranger,
    ClipEditor { title: &'a str },
}

impl<'a> ViewMode<'a> {
    #[inline]
    fn drives_engine(&self) -> bool {
        matches!(self, ViewMode::Arranger)
    }

    #[inline]
    fn allows_track_add_remove(&self) -> bool {
        matches!(self, ViewMode::Arranger)
    }

    fn header_label(&self) -> String {
        match self {
            ViewMode::Arranger => "PLAYLIST / TIMELINE".to_string(),
            ViewMode::ClipEditor { title } => title.to_string(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct CurvePoint {
    pub rel_tick: u64,
    pub value: f32,
    pub tension: f32,
}

#[derive(Clone, Debug)]
pub enum ClipType {
    Pattern { pattern_id: usize },
    Audio {
        sample_path: String,
        peaks: Vec<f32>,
        sample_offset_ticks: u64,
        total_sample_ticks: u64,
    },
    Automation {
        points: Vec<CurvePoint>,
        target_param: String,
    },
}

pub type Color32 = Hsla;

#[derive(Clone, Debug)]
pub struct PlaylistClip {
    pub id: usize,
    pub name: String,
    pub start_tick: u64,
    pub duration_ticks: u64,
    pub clip_type: ClipType,
    pub color: Hsla,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopDragHandle {
    None,
    Left,
    Right,
    Body,
}

#[derive(Clone, Debug)]
pub struct PlaylistState {
    pub clips: Vec<(usize, PlaylistClip)>,
    pub playhead_tick: u64,
    pub ppqn: u64,
    pub next_clip_id: usize,
    pub header_width: f32,
    pub grid_numerator: u32,
    pub grid_denominator: u32,
    pub zoom_x: f32,
    pub selected_clips: Vec<usize>,
    pub clipboard: Vec<(usize, PlaylistClip)>,
    pub needs_full_sync: bool,
    pub loop_start_ticks: u64,
    pub loop_end_ticks: u64,
    pub loop_region_active: bool,
    pub loop_dragging: bool,
    pub loop_preview_start_ticks: u64,
    pub loop_preview_end_ticks: u64,
    pub loop_preview_active: bool,
    pub loop_drag_handle: LoopDragHandle,
    pub loop_drag_completed_this_frame: bool,
}

impl Default for PlaylistState {
    fn default() -> Self {
        Self {
            clips: Vec::new(),
            playhead_tick: 0,
            ppqn: DEFAULT_PPQN,
            next_clip_id: 1,
            header_width: 180.0,
            grid_numerator: 1,
            grid_denominator: 4,
            zoom_x: 0.04,
            selected_clips: Vec::new(),
            clipboard: Vec::new(),
            needs_full_sync: true,
            loop_start_ticks: 0,
            loop_end_ticks: 0,
            loop_region_active: false,
            loop_dragging: false,
            loop_preview_start_ticks: 0,
            loop_preview_end_ticks: 0,
            loop_preview_active: false,
            loop_drag_handle: LoopDragHandle::None,
            loop_drag_completed_this_frame: false,
        }
    }
}

impl PlaylistState {
    pub fn loop_length_ticks(&self) -> u64 {
        self.loop_end_ticks.saturating_sub(self.loop_start_ticks)
    }

    pub fn is_loop_region_valid(&self) -> bool {
        self.loop_region_active && self.loop_end_ticks > self.loop_start_ticks
    }

    pub fn clear_loop_region(&mut self) {
        self.loop_start_ticks = 0;
        self.loop_end_ticks = 0;
        self.loop_region_active = false;
        self.loop_dragging = false;
        self.loop_preview_start_ticks = 0;
        self.loop_preview_end_ticks = 0;
        self.loop_preview_active = false;
        self.loop_drag_handle = LoopDragHandle::None;
        self.loop_drag_completed_this_frame = true;
    }

    pub fn total_project_ticks(&self) -> u64 {
        self.clips
            .iter()
            .map(|(_, c)| c.start_tick.saturating_add(c.duration_ticks))
            .max()
            .unwrap_or(0)
    }

    pub fn sanitize_loop_region(&mut self) {
        let ppqn = self.ppqn.max(1);
        let start = self.loop_start_ticks;
        let mut end = self.loop_end_ticks;
        if end <= start {
            end = start.saturating_add(4u64.saturating_mul(ppqn));
        }
        if end.saturating_sub(start) < ppqn {
            end = start.saturating_add(ppqn);
        }
        self.loop_start_ticks = start;
        self.loop_end_ticks = end;
        if self.loop_end_ticks > self.loop_start_ticks {
            self.loop_region_active = true;
        }
    }

    pub fn ensure_minimum_global_loop(&mut self, loop_enabled: bool) {
        if !loop_enabled || self.loop_dragging {
            return;
        }
        let ppqn = self.ppqn.max(1);
        let len = self.loop_end_ticks.saturating_sub(self.loop_start_ticks);
        let needs_init = !self.loop_region_active
            || self.loop_end_ticks <= self.loop_start_ticks
            || len < ppqn;
        if needs_init {
            let total = self.total_project_ticks();
            let start = 0u64;
            let mut end = total.max(4u64.saturating_mul(ppqn));
            if end <= start {
                end = start.saturating_add(4u64.saturating_mul(ppqn));
            }
            if end.saturating_sub(start) < ppqn {
                end = start.saturating_add(ppqn);
            }
            if end > start {
                self.loop_start_ticks = start;
                self.loop_end_ticks = end;
                self.loop_preview_start_ticks = start;
                self.loop_preview_end_ticks = end;
                self.loop_region_active = true;
                self.loop_drag_completed_this_frame = true;
            }
        } else {
            self.sanitize_loop_region();
        }
    }

    pub fn sync_all_clips_to_engine(&mut self, audio_proxy: &AudioProxy, bpm: f64) {
        for (track_id, clip) in &self.clips {
            if let ClipType::Audio { ref sample_path, sample_offset_ticks, .. } = clip.clip_type {
                let position_secs = ticks_to_secs_precise(clip.start_tick, self.ppqn, bpm);
                let duration_secs = ticks_to_secs_precise(clip.duration_ticks, self.ppqn, bpm);
                let offset_secs = ticks_to_secs_precise(sample_offset_ticks, self.ppqn, bpm);

                audio_proxy.send(GuiCommand::LoadClip {
                    clip_id: clip.id,
                    path: sample_path.clone(),
                    position_secs,
                    duration_secs,
                    offset_secs,
                    track_index: *track_id,
                    scene_index: 0,
                });
            }
        }
        self.needs_full_sync = false;
    }
}

#[inline]
fn px_to_ticks(px: f32, zoom_x: f32) -> u64 {
    (px.max(0.0) / zoom_x) as u64
}

#[inline]
fn ticks_to_px(ticks: u64, zoom_x: f32) -> f32 {
    ticks as f32 * zoom_x
}

pub fn snap_ticks(ticks: u64, grid_ticks: u64) -> u64 {
    if grid_ticks == 0 {
        return ticks;
    }
    ((ticks as f64 / grid_ticks as f64).round() as u64).saturating_mul(grid_ticks)
}

fn ticks_to_pixel_x(ticks: u64, ppqn: u64, zoom_x: f32, playlist_offset_x: f32) -> f32 {
    let ppqn = ppqn.max(1) as f32;
    let pixels_per_beat = ppqn * zoom_x;
    (ticks as f32 / ppqn) * pixels_per_beat + playlist_offset_x
}

pub fn loop_display_tick(
    current_tick: u64,
    loop_start_ticks: u64,
    loop_end_ticks: u64,
    loop_enabled: bool,
) -> u64 {
    if !loop_enabled {
        return current_tick;
    }
    let len = loop_end_ticks.saturating_sub(loop_start_ticks);
    if len == 0 || loop_end_ticks <= loop_start_ticks {
        return current_tick;
    }
    if current_tick < loop_start_ticks {
        return current_tick;
    }
    if current_tick >= loop_end_ticks {
        return loop_start_ticks + ((current_tick - loop_start_ticks) % len);
    }
    current_tick
}

fn samples_to_ticks_precise(sample_count: u64, ppqn: u64, bpm: f64, sample_rate: u32) -> u64 {
    let ppqn = ppqn.max(1);
    if bpm <= 0.0 || sample_rate == 0 {
        return 0;
    }
    let seconds = sample_count as f64 / sample_rate as f64;
    let seconds_per_tick = (60.0 / bpm) / ppqn as f64;
    (seconds / seconds_per_tick).round() as u64
}

fn ticks_to_samples_precise(ticks: u64, ppqn: u64, bpm: f64, sample_rate: u32) -> u64 {
    let ppqn = ppqn.max(1);
    if bpm <= 0.0 || sample_rate == 0 {
        return 0;
    }
    let seconds_per_tick = (60.0 / bpm) / ppqn as f64;
    (ticks as f64 * seconds_per_tick * sample_rate as f64).round() as u64
}

#[inline]
fn ticks_to_secs_precise(ticks: u64, ppqn: u64, bpm: f64) -> f32 {
    if ppqn == 0 || bpm <= 0.0 {
        return 0.0;
    }
    let seconds_per_tick = (60.0 / bpm) / (ppqn as f64);
    (ticks as f64 * seconds_per_tick) as f32
}

fn load_sample_info(path: &PathBuf, ppqn: u64, bpm: f64) -> (u64, Vec<f32>) {
    let mut peaks = Vec::new();

    match hound::WavReader::open(path) {
        Ok(mut reader) => {
            let spec = reader.spec();
            let total_frames = reader.duration() as u64;
            let channels = spec.channels.max(1) as u64;

            if spec.sample_rate > 0 && total_frames > 0 && bpm > 0.0 {
                let duration_sec = total_frames as f64 / spec.sample_rate as f64;
                let seconds_per_tick = (60.0 / bpm) / ppqn.max(1) as f64;
                let calculated_ticks = (duration_sec / seconds_per_tick).round() as u64;

                let target_peaks = 512;
                let step_frames = ((total_frames as usize) / target_peaks).max(1);
                let step_samples = step_frames * channels as usize;

                let samples: Vec<f32> = match spec.sample_format {
                    hound::SampleFormat::Int => {
                        let max_val = (1i64 << (spec.bits_per_sample - 1)) as f32;
                        reader.samples::<i32>()
                            .filter_map(|s| s.ok())
                            .map(|s| (s as f32 / max_val).abs())
                            .collect()
                    }
                    hound::SampleFormat::Float => {
                        reader.samples::<f32>()
                            .filter_map(|s| s.ok())
                            .map(|s| s.abs())
                            .collect()
                    }
                };

                for chunk in samples.chunks(step_samples) {
                    let max_peak = chunk.iter().cloned().fold(0.0f32, f32::max);
                    peaks.push(max_peak.clamp(0.0, 1.0));
                }

                return (calculated_ticks.max(1), peaks);
            }
        }
        Err(err) => {
            eprintln!("[PLAYLIST ERROR] Hound no pudo abrir {:?}: {}", path, err);
        }
    }

    (0, peaks)
}

pub fn build_audio_clip(
    id: usize,
    name: String,
    path: &PathBuf,
    start_tick: u64,
    ppqn: u64,
    bpm: f64,
    color: Hsla,
) -> PlaylistClip {
    let (duration_ticks, peaks) = load_sample_info(path, ppqn, bpm);
    let duration_ticks = duration_ticks.max(1);

    PlaylistClip {
        id,
        name,
        start_tick,
        duration_ticks,
        clip_type: ClipType::Audio {
            sample_path: path.to_string_lossy().to_string(),
            peaks,
            sample_offset_ticks: 0,
            total_sample_ticks: duration_ticks,
        },
        color,
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
    format!("{:.1}dB", db_val)
}

fn pan_text(pan: f32) -> String {
    let v = (pan * 100.0).round() as i32;
    if v == 0 {
        "C".to_string()
    } else if v < 0 {
        format!("L{}", v.abs())
    } else {
        format!("R{}", v)
    }
}

pub fn render(cx: &mut Context<HikaruApp>) -> AnyElement {
    let app = state(cx).read(cx);
    let pl = &app.playlist_state;
    let ppqn = pl.ppqn.max(1);
    let zoom_x = pl.zoom_x;
    let header_width = pl.header_width;
    let grid_num = pl.grid_numerator;
    let grid_den = pl.grid_denominator;
    let playhead_tick = pl.playhead_tick;
    let loop_start = pl.loop_start_ticks;
    let loop_end = pl.loop_end_ticks;
    let loop_active = pl.loop_region_active;
    let selected = pl.selected_clips.clone();
    let is_looping = app.is_looping;
    let audio_proxy = app.audio_proxy.clone();
    let bpm = app.transport.bpm;
    let sample_rate = app.transport.sample_rate.get() as u32;
    let transport_sc = app.transport.sample_count;
    let beats_per_bar = app.transport.beats_per_bar;
    let tracks = match app.mode {
        crate::app::AppMode::OpenLive => &app.live_tracks,
        crate::app::AppMode::OpenStudio => &app.studio_tracks,
    };
    let dragged_sample = app.dragged_sample.clone();
    drop(app);

    let ticks_per_bar = ppqn * beats_per_bar as u64;
    let display_tick = loop_display_tick(playhead_tick, loop_start, loop_end, is_looping && loop_active);

    let non_master: Vec<(usize, &Track)> = tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.is_master)
        .collect();

    let track_height = 54.0_f32;
    let ruler_h = 24.0_f32;
    let canvas_h = ruler_h + non_master.len() as f32 * track_height;
    let total_ticks = (ppqn * 4 * 128).max(playhead_tick + ticks_per_bar * 16);
    let canvas_w = (header_width + total_ticks as f32 * zoom_x).max(800.0);

    let mut track_headers: Vec<AnyElement> = Vec::new();
    for (idx, track) in &non_master {
        let tid = track.id;
        let tname = track.name.clone();
        let tvol = track.volume;
        let tpan = track.pan;
        let tmute = track.mute;
        let tsolo = track.solo;
        track_headers.push(
            v_flex()
                .w(px(header_width))
                .h(px(track_height))
                .bg(rgb(0x1C1C20))
                .border_1()
                .border_color(rgb(0x2D2D37))
                .p(px(4.0))
                .gap(px(2.0))
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .child(Label::new(tname.clone()).text_xs())
                        .child(
                            h_flex()
                                .gap(px(2.0))
                                .child(
                                    Button::new(format!("pl_track_solo_{}", tid))
                                        .label("S")
                                        .compact()
                                        .when(tsolo, |b| b.text_color(rgb(0xC8A000)))
                                        .on_click(move |_, _, cx| {
                                            let st = state(cx);
                                            st.update(cx, |state, cx| {
                                                if let Some(t) = state.live_tracks.iter_mut().find(|t| t.id == tid) {
                                                    t.solo = !t.solo;
                                                }
                                                if let Some(t) = state.studio_tracks.iter_mut().find(|t| t.id == tid) {
                                                    t.solo = !t.solo;
                                                }
                                                cx.notify();
                                            });
                                        }),
                                )
                                .child(
                                    Button::new(format!("pl_track_mute_{}", tid))
                                        .label("M")
                                        .compact()
                                        .when(tmute, |b| b.text_color(rgb(0xC80000)))
                                        .on_click(move |_, _, cx| {
                                            let st = state(cx);
                                            st.update(cx, |state, cx| {
                                                if let Some(t) = state.live_tracks.iter_mut().find(|t| t.id == tid) {
                                                    t.mute = !t.mute;
                                                }
                                                if let Some(t) = state.studio_tracks.iter_mut().find(|t| t.id == tid) {
                                                    t.mute = !t.mute;
                                                }
                                                cx.notify();
                                            });
                                        }),
                                ),
                        ),
                )
                .child(Label::new(db_text(tvol)).text_xs())
                .into_any_element(),
        );
    }

    let mut clip_elems: Vec<AnyElement> = Vec::new();
    for (track_id, clip) in &pl.clips {
        let is_sel = selected.contains(&clip.id);
        let clip_name = clip.name.clone();
        let clip_start = clip.start_tick;
        let clip_dur = clip.duration_ticks;
        let clip_x = ticks_to_px(clip_start, zoom_x);
        let clip_w = ticks_to_px(clip_dur, zoom_x).max(12.0);
        let track_row = non_master.iter().position(|(i, _)| *i == *track_id).unwrap_or(0);
        let clip_y = ruler_h + track_row as f32 * track_height + 1.0;
        let clip_h = track_height - 2.0;
        let clip_id = clip.id;

        if let ClipType::Audio { peaks, sample_offset_ticks, total_sample_ticks, .. } = &clip.clip_type {
            let peaks = peaks.clone();
            let soff = *sample_offset_ticks;
            let ttot = *sample_offset_ticks + *total_sample_ticks;
            let clip_el = div()
                .w(px(clip_w))
                .h(px(clip_h))
                .absolute()
                .left(px(clip_x))
                .top(px(clip_y))
                .bg(if is_sel {
                    rgb(0x325078)
                } else {
                    rgb(0x205F91)
                })
                .border_1()
                .border_color(if is_sel {
                    rgb(0xFFC800)
                } else {
                    rgba(0xFFFFFF66)
                })
                .id(format!("pl_clip_{}", clip_id))
                .on_click(move |event, _, cx| {
                    let shift = event.modifiers().shift;
                    let st = state(cx);
                    st.update(cx, |state, cx| {
                        if shift {
                            if state.playlist_state.selected_clips.contains(&clip_id) {
                                state.playlist_state.selected_clips.retain(|&id| id != clip_id);
                            } else {
                                state.playlist_state.selected_clips.push(clip_id);
                            }
                        } else {
                            state.playlist_state.selected_clips = vec![clip_id];
                        }
                        cx.notify();
                    });
                })
                .child(canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let cx0 = bounds.origin.x;
                    let cy = bounds.origin.y + px(bounds.size.height.as_f32() / 2.0);
                    let max_h = bounds.size.height.as_f32() * 0.4;
                    let n = peaks.len();
                    if n == 0 || ttot <= 0 {
                        return;
                    }
                    let start_ratio = soff as f32 / ttot as f32;
                    let dur_ratio = clip_dur as f32 / ttot as f32;
                    let step = 2.0;
                    let steps = (bounds.size.width.as_f32() / step) as usize;
                    for i in 0..steps {
                        let local = i as f32 / steps as f32;
                        let sample_norm = start_ratio + local * dur_ratio;
                        let peak_idx = (sample_norm * n as f32) as usize;
                        if let Some(&pv) = peaks.get(peak_idx) {
                            let bh = bounds.size.height.as_f32() * 0.8 * pv;
                            if bh > 0.5 {
                                let x = cx0 + px(i as f32 * step);
                                let mut path = PathBuilder::fill();
                                path.move_to(point(x, cy - px(bh * 0.5)));
                                path.line_to(point(x + px(1.0), cy - px(bh * 0.5)));
                                path.line_to(point(x + px(1.0), cy + px(bh * 0.5)));
                                path.line_to(point(x, cy + px(bh * 0.5)));
                                path.close();
                                window.paint_path(path.build().unwrap(), rgba(0xFFFFFFB3));
                            }
                        }
                    }
                },
            ));
            clip_elems.push(clip_el.into_any_element());
        } else {
            let clip_el = div()
                .w(px(clip_w))
                .h(px(clip_h))
                .absolute()
                .left(px(clip_x))
                .top(px(clip_y))
                .bg(rgb(0x205F91))
                .border_1()
                .border_color(rgba(0xFFFFFF66))
                .id(format!("pl_clip_{}", clip_id))
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, cx| {
                        state.playlist_state.selected_clips = vec![clip_id];
                        cx.notify();
                    });
                });
            clip_elems.push(clip_el.into_any_element());
        }
    }

    let playhead_x = ticks_to_px(display_tick, zoom_x);
    let snap_step_ticks = (ppqn * 4 * grid_num as u64) / grid_den as u64;
    let step_w = ticks_to_px(snap_step_ticks, zoom_x);
    let mut grid_lines: Vec<AnyElement> = Vec::new();
    let mut step = 0u64;
    let mut bar_num = 1u32;
    let steps_per_bar = (grid_den / grid_num).max(1) as u32;
    while step <= total_ticks {
        let x = ticks_to_px(step, zoom_x);
        if x > canvas_w {
            break;
        }
        let is_main = (step / snap_step_ticks) % steps_per_bar as u64 == 0;
        grid_lines.push(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let y0 = bounds.origin.y + px(ruler_h);
                    let col = if is_main { rgb(0x323232) } else { rgb(0x1C1C1C) };
                    let mut path = PathBuilder::stroke(px(1.0));
                    path.move_to(point(px(x), y0));
                    path.line_to(point(px(x), y0 + bounds.size.height - px(ruler_h)));
                    window.paint_path(path.build().unwrap(), col);
                },
            )
            .absolute()
            .left(px(x))
            .top(px(ruler_h))
            .w(px(1.0))
            .h(px(canvas_h - ruler_h))
            .into_any_element(),
        );
        if is_main {
            grid_lines.push(
                Label::new(bar_num.to_string())
                    .text_xs()
                    .absolute()
                    .left(px(x + 4.0))
                    .top(px(2.0))
                    .into_any_element(),
            );
            bar_num += 1;
        }
        step += snap_step_ticks;
    }

    let loop_render = if loop_active && loop_end > loop_start {
        let lx0 = ticks_to_px(loop_start, zoom_x);
        let lx1 = ticks_to_px(loop_end, zoom_x);
        Some((lx0, lx1))
    } else {
        None
    };

    let mut drop_zones: Vec<AnyElement> = Vec::new();
    if let Some(ref path) = dragged_sample {
        let file_name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        drop_zones.push(
            div()
                .absolute()
                .left(px(header_width + 10.0))
                .top(px(ruler_h + 10.0))
                .bg(rgba(0x0096BE80))
                .rounded(px(4.0))
                .p(px(4.0))
                .child(Label::new(format!("🎵 {}", file_name)).text_xs())
                .into_any_element(),
        );
    }

    let drop_handler = if dragged_sample.is_some() {
        let audio_proxy = audio_proxy.clone();
        let track_ids: Vec<usize> = non_master.iter().map(|(tid, _)| *tid).collect();
        Some(
            div()
                .absolute()
                .left(px(0.0))
                .top(px(ruler_h))
                .w(px(canvas_w))
                .h(px(canvas_h - ruler_h))
                .id("pl_drop_handler")
                .on_click(move |event, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, cx| {
                        if let Some(ref path) = state.dragged_sample {
                            let Some(pos) = event.mouse_position() else {
                                return;
                            };
                            let rel_x = pos.x.as_f32() - header_width;
                            let raw = px_to_ticks(rel_x.max(0.0), zoom_x);
                            let drop_tick = snap_ticks(raw, snap_step_ticks);
                            let rel_y = pos.y.as_f32() - ruler_h;
                            let track_idx = (rel_y / track_height).floor() as usize;
                            if track_idx < track_ids.len() {
                                let target_tid = track_ids[track_idx];
                                let name = path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string();
                                let pstr = path.to_string_lossy().to_string();
                                let (dur, peaks) = load_sample_info(path, ppqn, bpm);
                                let clip = build_audio_clip(
                                    state.playlist_state.next_clip_id,
                                    name,
                                    path,
                                    drop_tick,
                                    ppqn,
                                    bpm,
                                    rgb(0x205F91).into(),
                                );
                                let cid = state.playlist_state.next_clip_id;
                                state.playlist_state.next_clip_id += 1;
                                state.playlist_state.clips.push((target_tid, clip));
                                state.audio_proxy.send(GuiCommand::LoadClip {
                                    clip_id: cid,
                                    path: pstr,
                                    position_secs: ticks_to_secs_precise(drop_tick, ppqn, bpm),
                                    duration_secs: ticks_to_secs_precise(dur, ppqn, bpm),
                                    offset_secs: 0.0,
                                    track_index: target_tid,
                                    scene_index: 0,
                                });
                            }
                        }
                        state.dragged_sample = None;
                        cx.notify();
                    });
                })
                .into_any_element(),
        )
    } else {
        None
    };

    let seek_zone = div()
        .absolute()
        .left(px(0.0))
        .top(px(ruler_h))
        .w(px(canvas_w))
        .h(px(canvas_h - ruler_h))
        .id("pl_seek_zone")
        .on_click(move |event, _, cx| {
            let Some(pos) = event.mouse_position() else {
                return;
            };
            let rel_x = pos.x.as_f32() - header_width;
            let clicked = px_to_ticks(rel_x.max(0.0), zoom_x);
            let st = state(cx);
            st.update(cx, |state, _| {
                let target = ticks_to_samples_precise(clicked, ppqn, bpm, sample_rate);
                state.audio_proxy.send(GuiCommand::Seek { sample_count: target });
            });
        });

    let children = vec![
        div()
            .h(px(ruler_h))
            .w(px(canvas_w))
            .bg(rgb(0x18181E))
            .into_any_element(),
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let y0 = bounds.origin.y + px(ruler_h);
                let col = rgb(0x1C1C1C);
                let mut path = PathBuilder::stroke(px(1.0));
                path.move_to(point(bounds.origin.x, y0));
                path.line_to(point(bounds.origin.x, y0 + bounds.size.height - px(ruler_h)));
                window.paint_path(path.build().unwrap(), col);
            },
        )
        .absolute()
        .left(px(0.0))
        .top(px(ruler_h))
        .w(px(canvas_w))
        .h(px(canvas_h - ruler_h))
        .into_any_element(),
    ];

    let playhead_canvas = canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let mut playhead_path = PathBuilder::stroke(px(2.0));
            playhead_path.move_to(point(px(playhead_x), bounds.origin.y));
            playhead_path.line_to(point(
                px(playhead_x),
                bounds.origin.y + bounds.size.height,
            ));
            window.paint_path(playhead_path.build().unwrap(), rgb(0x00FFFF));
            if let Some((lx0, lx1)) = loop_render {
                let top = bounds.origin.y;
                let bot = bounds.origin.y + bounds.size.height;
                let mut loop_path = PathBuilder::stroke(px(2.0));
                loop_path.move_to(point(px(lx0), top));
                loop_path.line_to(point(px(lx0), bot));
                window.paint_path(loop_path.build().unwrap(), rgb(0x00C8FF));
                let mut loop_path = PathBuilder::stroke(px(2.0));
                loop_path.move_to(point(px(lx1), top));
                loop_path.line_to(point(px(lx1), bot));
                window.paint_path(loop_path.build().unwrap(), rgb(0x00C8FF));
            }
        },
    )
    .absolute()
    .left(px(0.0))
    .top(px(0.0))
    .w(px(canvas_w))
    .h(px(canvas_h));

    let mut all: Vec<AnyElement> = Vec::new();
    all.push(
        h_flex()
            .items_center()
            .gap(px(6.0))
            .child(Label::new("PLAYLIST / TIMELINE").text_sm().font_weight(FontWeight::BOLD))
            .child(Label::new(format!("Grid {}/{}", grid_num, grid_den)).text_xs())
            .id("pl_grid_header")
            .on_click(move |_, _, cx| {
                let st = state(cx);
                st.update(cx, |state, cx| {
                    state.playlist_state.grid_denominator = match state.playlist_state.grid_denominator {
                        2 => 4,
                        4 => 8,
                        8 => 16,
                        _ => 2,
                    };
                    cx.notify();
                });
            })
            .into_any_element(),
    );
    all.push(
        div()
            .flex_1()
            .overflow_x_scrollbar()
            .child(
                div()
                    .w(px(canvas_w + header_width))
                    .h(px(canvas_h))
                    .relative()
                    .child(
                        h_flex()
                            .absolute()
                            .left(px(0.0))
                            .top(px(0.0))
                            .children(track_headers),
                    )
                    .children(children)
                    .children(clip_elems)
                    .children(grid_lines)
                    .children(drop_zones)
                    .when_some(drop_handler, |v, h| v.child(h))
                    .child(seek_zone.into_any_element())
                    .child(playhead_canvas.into_any_element()),
            )
            .into_any_element(),
    );
    all.push(
        h_flex()
            .gap(px(4.0))
            .child(
                Button::new("pl_add_track")
                    .label("[ + ]")
                    .compact()
                    .on_click(move |_, _, cx| {
                        let st = state(cx);
                        st.update(cx, |state, cx| {
                            let next_id = state
                                .live_tracks
                                .iter()
                                .map(|t| t.id)
                                .max()
                                .unwrap_or(0)
                                + 1;
                            let n = state.live_tracks.iter().filter(|t| !t.is_master).count();
                            state.live_tracks.push(Track::new(
                                next_id,
                                format!("TRACK {:02}", n + 1),
                                false,
                            ));
                            cx.notify();
                        });
                    }),
            )
            .child(
                Button::new("pl_remove_track")
                    .label("[ - ]")
                    .compact()
                    .on_click(move |_, _, cx| {
                        let st = state(cx);
                        st.update(cx, |state, cx| {
                            if let Some(pos) = state
                                .live_tracks
                                .iter()
                                .rposition(|t| !t.is_master)
                            {
                                state.live_tracks.remove(pos);
                            }
                            cx.notify();
                        });
                    }),
            )
            .child(Label::new(format!("PPQN {}", ppqn)).text_xs())
            .child(Label::new(format!("{:.2} bars", playhead_tick as f64 / ticks_per_bar as f64)).text_xs())
            .into_any_element(),
    );

    v_flex()
        .id("playlist")
        .size_full()
        .gap(px(4.0))
        .children(all)
        .into_any_element()
}
