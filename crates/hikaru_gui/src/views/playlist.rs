// Playlist / Timeline (OpenStudio) — lienzo de composición lineal.
//
// LÍMITE DEL MÓDULO: esta vista es SÓLO tiempo. Contiene la regla de
// compases (ruler), filas horizontales limpias por pista, grilla con snap,
// clips de audio/MIDI y playhead.
use std::path::PathBuf;

use crate::app::AppState;
use crate::audio_proxy::GuiCommand;
use crate::theme;
use hikaru_transport::DEFAULT_PPQN;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewMode<'a> {
    Arranger,
    ClipEditor { title: &'a str },
}

#[allow(dead_code)]
impl<'a> ViewMode<'a> {
    #[inline]
    pub fn drives_engine(&self) -> bool {
        matches!(self, ViewMode::Arranger)
    }

    #[inline]
    pub fn allows_track_add_remove(&self) -> bool {
        matches!(self, ViewMode::Arranger)
    }

    pub fn header_label(&self) -> String {
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

pub type Color32 = egui::Color32;

#[derive(Clone, Debug)]
pub struct PlaylistClip {
    pub id: usize,
    pub name: String,
    pub start_tick: u64,
    pub duration_ticks: u64,
    pub clip_type: ClipType,
    pub color: egui::Color32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopDragHandle {
    None,
    Left,
    Right,
    Body,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaylistClipDrag {
    pub clip_id: usize,
    pub orig_tick: u64,
    pub orig_track: usize,
    pub grab_dx_px: f32,
    pub grab_dy_px: f32,
    pub moved: bool,
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
    pub row_h: f32,
    pub scroll_offset_x: f32,
    pub scroll_offset_y: f32,
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
    pub clip_drag: Option<PlaylistClipDrag>,
    pub clip_click_suppress: bool,
    pub marquee: Option<MarqueeState>,
    pub trim_drag: Option<TrimDrag>,
    pub trim_hover: Option<(usize, TrimEdge)>,
    pub hover_tick: Option<u64>,
}

impl Default for PlaylistState {
    fn default() -> Self {
        Self {
            clips: Vec::new(),
            playhead_tick: 0,
            ppqn: DEFAULT_PPQN,
            next_clip_id: 1,
            header_width: 196.0,
            grid_numerator: 1,
            grid_denominator: 4,
            zoom_x: 0.04,
            row_h: TRACK_ROW_H,
            scroll_offset_x: 0.0,
            scroll_offset_y: 0.0,
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
            clip_drag: None,
            clip_click_suppress: false,
            marquee: None,
            trim_drag: None,
            trim_hover: None,
            hover_tick: None,
        }
    }
}

impl PlaylistState {
    pub fn track_height(&self) -> f32 {
        clamped_row_h(self.row_h)
    }

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

    pub fn paste_tick(&self) -> u64 {
        loop_display_tick(
            self.playhead_tick,
            self.loop_start_ticks,
            self.loop_end_ticks,
            self.loop_region_active,
        )
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

    pub fn sync_all_clips_to_engine(&mut self, audio_proxy: &crate::audio_proxy::AudioProxy, bpm: f64) {
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

pub const TRACK_ROW_H: f32 = 68.0;
pub const RULER_H: f32 = 26.0;
pub const MIN_ZOOM_X: f32 = 0.005;
pub const MAX_ZOOM_X: f32 = 2.0;
pub const MIN_ROW_H: f32 = 60.0;
pub const MAX_ROW_H: f32 = 180.0;
pub const ZOOM_STEP_FACTOR: f32 = 1.25;
pub const LINES_PER_NOTCH: f32 = 3.0;
pub const PX_PER_NOTCH: f32 = 53.0;

#[inline]
pub fn px_to_ticks(px: f32, zoom_x: f32) -> u64 {
    (px.max(0.0) / zoom_x.max(0.0001)) as u64
}

#[inline]
pub fn ticks_to_px(ticks: u64, zoom_x: f32) -> f32 {
    ticks as f32 * zoom_x
}

pub fn ticks_per_bar(ppqn: u64, beats_per_bar: u32) -> u64 {
    ppqn.max(1) * beats_per_bar.max(1) as u64
}

pub fn bar_number_at_tick(tick: u64, ticks_per_bar: u64) -> u32 {
    (tick / ticks_per_bar.max(1)) as u32 + 1
}

pub fn bar_to_px(bar: u32, ticks_per_bar: u64, zoom_x: f32) -> f32 {
    (bar.saturating_sub(1) as u64 * ticks_per_bar.max(1)) as f32 * zoom_x
}

pub fn snap_step_ticks(ppqn: u64, grid_denominator: u32) -> u64 {
    let ppqn = ppqn.max(1);
    match grid_denominator.max(1) {
        2 => ppqn * 2,
        4 => ppqn,
        8 => ppqn / 2,
        16 => ppqn / 4,
        d => ppqn * 4 / d as u64,
    }
    .max(1)
}

pub fn clamped_zoom(zoom: f32) -> f32 {
    if !zoom.is_finite() {
        return 0.04;
    }
    zoom.clamp(MIN_ZOOM_X, MAX_ZOOM_X)
}

pub fn step_zoom(zoom: f32, zoom_in: bool) -> f32 {
    clamped_zoom(if zoom_in { zoom * 1.25 } else { zoom / 1.25 })
}

pub fn clamped_row_h(h: f32) -> f32 {
    if !h.is_finite() {
        return TRACK_ROW_H;
    }
    h.clamp(MIN_ROW_H, MAX_ROW_H)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaylistPanState {
    pub last: (f32, f32),
    pub geometry: PlaylistNavGeometry,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct PlaylistNavGeometry {
    pub header_width: f32,
    pub ruler_h: f32,
    pub total_ticks: u64,
    pub row_count: usize,
}

pub const PAN_SENSITIVITY: f32 = 1.0;
pub const MMB_DRAG_PX_PER_NOTCH: f32 = 120.0;

pub fn mmb_drag_notches(dy: f32) -> f32 {
    if !dy.is_finite() {
        return 0.0;
    }
    -dy / MMB_DRAG_PX_PER_NOTCH
}

pub fn clamp_scroll_offset(current: f32, max: f32) -> f32 {
    (current).clamp(-max.max(0.0), 0.0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrimEdge {
    Left,
    Right,
}

impl TrimEdge {
    pub fn cursor(self) -> egui::CursorIcon {
        match self {
            TrimEdge::Left => egui::CursorIcon::ResizeWest,
            TrimEdge::Right => egui::CursorIcon::ResizeEast,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrimDrag {
    pub clip_id: usize,
    pub edge: TrimEdge,
    pub orig_edge_tick: u64,
    pub orig_start: u64,
    pub orig_duration: u64,
    pub orig_offset: u64,
    pub orig_total: u64,
    pub grab_dx_px: f32,
    pub moved: bool,
}

pub fn trim_edge_at(x: f32, clip_x: f32, clip_w: f32, edge_px: f32) -> Option<TrimEdge> {
    if clip_w <= 0.0 {
        return None;
    }
    let left = clip_x;
    let right = clip_x + clip_w;
    if x >= left && x <= left + edge_px {
        if x - left <= right - x {
            return Some(TrimEdge::Left);
        }
    }
    if x >= right - edge_px && x <= right {
        if right - x <= x - left {
            return Some(TrimEdge::Right);
        }
    }
    None
}

pub fn trim_apply(
    orig_start: u64,
    orig_dur: u64,
    orig_offset: u64,
    orig_total: u64,
    edge: TrimEdge,
    edge_tick: u64,
    grid_ticks: u64,
    min_ticks: u64,
) -> (u64, u64, u64) {
    match edge {
        TrimEdge::Right => {
            let audio_left = orig_total.saturating_sub(orig_offset);
            let audio_end = orig_start
                .saturating_add(audio_left)
                .max(orig_start.saturating_add(min_ticks));
            let new_end = edge_tick
                .max(orig_start.saturating_add(min_ticks))
                .min(audio_end);
            let dur = new_end.saturating_sub(orig_start).max(min_ticks);
            (orig_start, dur, orig_offset)
        }
        TrimEdge::Left => {
            let max_tick = orig_start
                .saturating_add(orig_dur)
                .saturating_sub(min_ticks);
            let new_start = edge_tick.clamp(orig_offset, max_tick);
            let offset = orig_offset.saturating_add(new_start.saturating_sub(orig_start));
            let dur = orig_start
                .saturating_add(orig_dur)
                .saturating_sub(new_start)
                .max(min_ticks);
            let _ = (grid_ticks, orig_total);
            (new_start, dur, offset)
        }
    }
}

pub fn audio_span(offset: u64, total: u64) -> (u64, u64) {
    (offset, offset.saturating_add(total))
}

pub const MARQUEE_DRAG_THRESHOLD: f32 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarqueeState {
    pub start_x: f32,
    pub start_y: f32,
    pub cur_x: f32,
    pub cur_y: f32,
}

impl MarqueeState {
    pub fn drag_distance(&self) -> f32 {
        (self.cur_x - self.start_x).abs() + (self.cur_y - self.start_y).abs()
    }

    pub fn is_drag(&self) -> bool {
        self.drag_distance() >= MARQUEE_DRAG_THRESHOLD
    }

    pub fn normalized(&self) -> (f32, f32, f32, f32) {
        let x0 = self.start_x.min(self.cur_x);
        let y0 = self.start_y.min(self.cur_y);
        let x1 = self.start_x.max(self.cur_x);
        let y1 = self.start_y.max(self.cur_y);
        (x0, y0, x1, y1)
    }
}

pub fn normalize_rect(ax: f32, ay: f32, bx: f32, by: f32) -> (f32, f32, f32, f32) {
    (ax.min(bx), ay.min(by), ax.max(bx), ay.max(by))
}

pub fn rects_intersect(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> bool {
    let (ax0, ay0, ax1, ay1) = a;
    let (bx0, by0, bx1, by1) = b;
    ax0 < bx1 && ax1 > bx0 && ay0 < by1 && ay1 > by0
}

pub fn clips_in_marquee(
    clips: &[(usize, PlaylistClip)],
    marquee_rect: (f32, f32, f32, f32),
    row_keys: &[usize],
    zoom_x: f32,
    row_h: f32,
    grid_ox: f32,
    grid_oy: f32,
) -> Vec<usize> {
    clips
        .iter()
        .filter_map(|(track_id, clip)| {
            let row_idx = row_keys.iter().position(|k| k == track_id)?;
            let cx0 = grid_ox + ticks_to_px(clip.start_tick, zoom_x);
            let cx1 = cx0 + ticks_to_px(clip.duration_ticks, zoom_x);
            let cy0 = grid_oy + row_idx as f32 * row_h;
            let cy1 = cy0 + row_h;
            if rects_intersect(marquee_rect, (cx0, cy0, cx1, cy1)) {
                Some(clip.id)
            } else {
                None
            }
        })
        .collect()
}

pub fn copy_selected_clips(s: &mut PlaylistState) -> usize {
    s.clipboard.clear();
    for (t, c) in &s.clips {
        if s.selected_clips.contains(&c.id) {
            s.clipboard.push((*t, c.clone()));
        }
    }
    s.clipboard.len()
}

pub fn cut_selected_clips(s: &mut PlaylistState) -> usize {
    let count = copy_selected_clips(s);
    s.clips.retain(|(_, c)| !s.selected_clips.contains(&c.id));
    s.selected_clips.clear();
    s.needs_full_sync = true;
    count
}

pub fn next_free_clip_id(s: &PlaylistState) -> usize {
    let max_id = s.clips.iter().map(|(_, c)| c.id).max().unwrap_or(0);
    max_id.saturating_add(1)
}

pub fn paste_clips(
    s: &mut PlaylistState,
    target_tick: u64,
    target_track_offset: usize,
) -> Vec<usize> {
    if s.clipboard.is_empty() {
        return Vec::new();
    }
    let min_tick = s.clipboard.iter().map(|(_, c)| c.start_tick).min().unwrap_or(0);
    let min_track = s.clipboard.iter().map(|(t, _)| *t).min().unwrap_or(0);

    let mut new_ids = Vec::new();
    for (orig_t, orig_c) in &s.clipboard {
        let tick_offset = orig_c.start_tick.saturating_sub(min_tick);
        let track_offset = orig_t.saturating_sub(min_track);
        let new_track = target_track_offset + track_offset;
        let new_tick = target_tick.saturating_add(tick_offset);

        let new_id = next_free_clip_id(s);
        let mut new_clip = orig_c.clone();
        new_clip.id = new_id;
        new_clip.start_tick = new_tick;

        s.clips.push((new_track, new_clip));
        new_ids.push(new_id);
    }
    s.selected_clips = new_ids.clone();
    s.needs_full_sync = true;
    new_ids
}

pub fn duplicate_selected_clips(s: &mut PlaylistState, grid_ticks: u64) -> Vec<usize> {
    if s.selected_clips.is_empty() {
        return Vec::new();
    }
    let mut selected: Vec<(usize, PlaylistClip)> = s
        .clips
        .iter()
        .filter(|(_, c)| s.selected_clips.contains(&c.id))
        .cloned()
        .collect();

    let max_end = selected
        .iter()
        .map(|(_, c)| c.start_tick.saturating_add(c.duration_ticks))
        .max()
        .unwrap_or(0);
    let shift_ticks = snap_ticks(max_end, grid_ticks.max(1));

    let mut new_ids = Vec::new();
    for (t, mut c) in selected.drain(..) {
        let new_id = next_free_clip_id(s);
        c.id = new_id;
        c.start_tick = c.start_tick.saturating_add(shift_ticks);
        s.clips.push((t, c));
        new_ids.push(new_id);
    }
    s.selected_clips = new_ids.clone();
    s.needs_full_sync = true;
    new_ids
}

pub fn step_row_h(h: f32, zoom_in: bool) -> f32 {
    clamped_row_h(if zoom_in { h * 1.25 } else { h / 1.25 })
}

pub fn smooth_zoom(zoom: f32, notches: f32) -> f32 {
    let factor = ZOOM_STEP_FACTOR.powf(notches);
    clamped_zoom(zoom * factor)
}

pub fn smooth_row_h(h: f32, notches: f32) -> f32 {
    let factor = ZOOM_STEP_FACTOR.powf(notches);
    clamped_row_h(h * factor)
}

pub fn anchor_h_offset(
    cursor_vx: f32,
    header_w: f32,
    old_zoom: f32,
    new_zoom: f32,
    old_offset_x: f32,
    viewport_w: f32,
    content_w: f32,
) -> f32 {
    let track_x = (cursor_vx - header_w - old_offset_x).max(0.0);
    let tick = (track_x / old_zoom) as u64;
    let new_track_x = tick as f32 * new_zoom;
    let new_offset_x = cursor_vx - header_w - new_track_x;
    let max_scroll = (content_w - viewport_w).max(0.0);
    new_offset_x.clamp(-max_scroll, 0.0)
}

pub fn anchor_v_offset(
    cursor_vy: f32,
    ruler_h: f32,
    old_row_h: f32,
    new_row_h: f32,
    old_offset_y: f32,
    viewport_h: f32,
    content_h: f32,
) -> f32 {
    let track_y = (cursor_vy - ruler_h - old_offset_y).max(0.0);
    let row_idx = track_y / old_row_h;
    let new_track_y = row_idx * new_row_h;
    let new_offset_y = cursor_vy - ruler_h - new_track_y;
    let max_scroll = (content_h - viewport_h).max(0.0);
    new_offset_y.clamp(-max_scroll, 0.0)
}

pub fn snap_ticks(ticks: u64, grid_ticks: u64) -> u64 {
    if grid_ticks <= 1 {
        return ticks;
    }
    let half = grid_ticks / 2;
    ((ticks + half) / grid_ticks) * grid_ticks
}

pub fn min_clip_ticks(ppqn: u64) -> u64 {
    (ppqn / 16).max(1)
}

pub fn loop_display_tick(
    playhead_tick: u64,
    loop_start: u64,
    loop_end: u64,
    loop_active: bool,
) -> u64 {
    if loop_active && loop_end > loop_start {
        let len = loop_end - loop_start;
        if playhead_tick >= loop_start {
            loop_start + ((playhead_tick - loop_start) % len)
        } else {
            playhead_tick
        }
    } else {
        playhead_tick
    }
}

pub fn ticks_to_secs_precise(ticks: u64, ppqn: u64, bpm: f64) -> f32 {
    let ppqn_f = ppqn.max(1) as f64;
    let bpm_f = bpm.max(1.0);
    let beats = ticks as f64 / ppqn_f;
    let secs = beats * (60.0 / bpm_f);
    secs as f32
}

pub fn load_sample_info(path: &PathBuf, ppqn: u64, bpm: f64) -> (u64, Vec<f32>) {
    let mut peaks = Vec::new();
    let mut total_samples = 44100;
    let sample_rate = 44100;

    if let Ok(mut reader) = hound::WavReader::open(path) {
        let _spec = reader.spec();
        let samples_count = reader.duration() as usize;
        total_samples = samples_count;
        let num_peaks = 256;
        let step = (samples_count / num_peaks).max(1);

        let mut current_max: f32 = 0.0;
        let mut count = 0;

        for sample in reader.samples::<i32>() {
            if let Ok(s) = sample {
                let norm = (s as f32 / i32::MAX as f32).abs();
                if norm > current_max {
                    current_max = norm;
                }
                count += 1;
                if count >= step {
                    peaks.push(current_max);
                    current_max = 0.0;
                    count = 0;
                }
            }
        }
        if peaks.is_empty() {
            peaks.push(0.5);
        }
    }

    let secs = total_samples as f64 / sample_rate as f64;
    let beats = secs / (60.0 / bpm.max(1.0));
    let total_ticks = (beats * ppqn as f64).round() as u64;

    (total_ticks, peaks)
}

pub fn build_audio_clip(
    id: usize,
    name: String,
    start_tick: u64,
    path: PathBuf,
    ppqn: u64,
    bpm: f64,
) -> PlaylistClip {
    let (total_ticks, peaks) = load_sample_info(&path, ppqn, bpm);
    PlaylistClip {
        id,
        name,
        start_tick,
        duration_ticks: total_ticks.max(ppqn),
        clip_type: ClipType::Audio {
            sample_path: path.to_string_lossy().to_string(),
            peaks,
            sample_offset_ticks: 0,
            total_sample_ticks: total_ticks.max(ppqn),
        },
        color: theme::ACCENT_PRIMARY,
    }
}

pub fn grid_origin_from_zone(zone: [f32; 4], ruler_h: f32) -> (f32, f32) {
    (zone[0], zone[1] + ruler_h)
}

pub fn handle_edit_shortcut(key: &str, _shift: bool, app: &mut AppState) -> bool {
    match key {
        "c" | "C" => {
            copy_selected_clips(&mut app.playlist_state);
            true
        }
        "x" | "X" => {
            cut_selected_clips(&mut app.playlist_state);
            true
        }
        "v" | "V" => {
            let target_tick = app.playlist_state.paste_tick();
            paste_clips(&mut app.playlist_state, target_tick, app.selected_track_index);
            true
        }
        "d" | "D" => {
            let ppqn = app.playlist_state.ppqn;
            let step = snap_step_ticks(ppqn, app.playlist_state.grid_denominator);
            duplicate_selected_clips(&mut app.playlist_state, step);
            true
        }
        "Delete" | "Backspace" => {
            let selected = app.playlist_state.selected_clips.clone();
            app.playlist_state.clips.retain(|(_, c)| !selected.contains(&c.id));
            app.playlist_state.selected_clips.clear();
            app.playlist_state.needs_full_sync = true;
            true
        }
        _ => false,
    }
}

/// Render the Playlist / Arranger view in immediate mode using the unified Bitwig 5.1.7 Arrange Workspace.
pub fn render(ui: &mut egui::Ui, app: &mut AppState) {
    crate::views::matrix::render(ui, app);
}
