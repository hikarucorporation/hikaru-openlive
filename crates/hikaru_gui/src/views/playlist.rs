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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PlaylistTool {
    #[default]
    Pointer,
    Time,
    Pen,
    Eraser,
    Knife,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopDragHandle {
    None,
    Left,
    Right,
    Body,
    Create,
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
    pub loop_drag_origin_tick: u64,
    pub loop_drag_offset_ticks: u64,
    pub loop_drag_moved: bool,
    pub clip_drag: Option<PlaylistClipDrag>,
    pub clip_drag_peers: Vec<(usize, u64, usize)>,
    pub clip_click_suppress: bool,
    pub marquee: Option<MarqueeState>,
    pub trim_drag: Option<TrimDrag>,
    pub trim_hover: Option<(usize, TrimEdge)>,
    pub hover_tick: Option<u64>,
    pub active_tool: PlaylistTool,
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
            loop_drag_origin_tick: 0,
            loop_drag_offset_ticks: 0,
            loop_drag_moved: false,
            clip_drag: None,
            clip_drag_peers: Vec::new(),
            clip_click_suppress: false,
            marquee: None,
            trim_drag: None,
            trim_hover: None,
            hover_tick: None,
            active_tool: PlaylistTool::Pointer,
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
                    track_index: track_id.saturating_sub(1),
                    scene_index: 0,
                    speed_ratio: 1.0,
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
    pub start: (f32, f32),
    pub last: (f32, f32),
    pub moved: bool,
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
    let mut sample_rate = 44100u32;

    if let Ok((samples, channels, file_sr)) = crate::wav_io::read_file(path) {
        let samples_count = samples.len() / channels.max(1);
        total_samples = samples_count;
        sample_rate = file_sr;
        let num_peaks = 256;
        let step = (samples_count / num_peaks).max(1);

        let mut current_max: f32 = 0.0;
        let mut count = 0;

        for s in samples {
            let norm = s.abs();
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
    let effective_bpm = path
        .file_name()
        .and_then(|n| crate::views::explorer::parse_bpm_from_filename(&n.to_string_lossy()))
        .map(|sbpm| sbpm as f64)
        .unwrap_or(bpm);
    let (total_ticks, peaks) = load_sample_info(&path, ppqn, effective_bpm);
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

/// Bitwig-style adaptive grid resolution based on current horizontal zoom.
/// Returns `(step_ticks, label, denominator)`.
pub fn adaptive_grid_step(ppqn: u64, beats_per_bar: u32, zoom_x: f32) -> (u64, &'static str, u32) {
    let beat = ppqn.max(1);
    let bar = ticks_per_bar(beat, beats_per_bar.max(1));
    let candidates: [(u64, &'static str, u32); 8] = [
        ((beat / 16).max(1), "1/64", 64),
        ((beat / 8).max(1), "1/32", 32),
        ((beat / 4).max(1), "1/16", 16),
        ((beat / 2).max(1), "1/8", 8),
        (beat, "1/4", 4),
        (bar, "1/1", 1),
        (bar * 2, "2 Bars", 1),
        (bar * 4, "4 Bars", 1),
    ];
    for &(step, label, den) in &candidates {
        if ticks_to_px(step, zoom_x) >= 14.0 {
            return (step, label, den);
        }
    }
    (bar * 4, "4 Bars", 1)
}

/// Adaptive bar numbering step for the Bitwig timeline ruler so bar labels never overlap.
pub fn ruler_bar_step(bar_px_w: f32) -> u64 {
    if bar_px_w >= 26.0 {
        1
    } else if bar_px_w >= 13.0 {
        2
    } else if bar_px_w >= 6.5 {
        4
    } else if bar_px_w >= 3.2 {
        8
    } else {
        16
    }
}

/// Split all selected clips that span across `split_tick` into left and right halves.
pub fn split_selected_clips_at(s: &mut PlaylistState, split_tick: u64) -> usize {
    let min_ticks = min_clip_ticks(s.ppqn);
    let selected = s.selected_clips.clone();
    let mut new_clips: Vec<(usize, PlaylistClip)> = Vec::new();
    let mut new_selected: Vec<usize> = Vec::new();
    let mut split_count = 0usize;

    for (track_id, clip) in &mut s.clips {
        if !selected.contains(&clip.id) {
            continue;
        }
        let end_tick = clip.start_tick.saturating_add(clip.duration_ticks);
        if split_tick <= clip.start_tick.saturating_add(min_ticks)
            || split_tick >= end_tick.saturating_sub(min_ticks)
        {
            continue;
        }
        let left_dur = split_tick - clip.start_tick;
        let right_dur = end_tick - split_tick;
        let mut right_clip = clip.clone();
        let new_id = s.next_clip_id;
        s.next_clip_id += 1;
        right_clip.id = new_id;
        right_clip.start_tick = split_tick;
        right_clip.duration_ticks = right_dur;
        if let ClipType::Audio {
            ref mut sample_offset_ticks,
            ..
        } = right_clip.clip_type
        {
            *sample_offset_ticks = sample_offset_ticks.saturating_add(left_dur);
        }
        clip.duration_ticks = left_dur;
        new_selected.push(new_id);
        new_clips.push((*track_id, right_clip));
        split_count += 1;
    }

    if split_count > 0 {
        s.clips.extend(new_clips);
        s.selected_clips.extend(new_selected);
        s.needs_full_sync = true;
    }
    split_count
}

pub fn select_all_clips(s: &mut PlaylistState) {
    s.selected_clips = s.clips.iter().map(|(_, c)| c.id).collect();
}

/// Consolidate selected clips per track into a single merged clip spanning `[min_start, max_end)`.
/// Returns the number of consolidated clips produced.
pub fn consolidate_selected_clips(s: &mut PlaylistState) -> usize {
    if s.selected_clips.is_empty() {
        return 0;
    }
    let selected = s.selected_clips.clone();
    let mut track_ids: Vec<usize> = Vec::new();
    for (tid, c) in &s.clips {
        if selected.contains(&c.id) && !track_ids.contains(tid) {
            track_ids.push(*tid);
        }
    }

    let mut consolidated_ids: Vec<usize> = Vec::new();
    let mut to_remove: Vec<usize> = Vec::new();
    let mut to_add: Vec<(usize, PlaylistClip)> = Vec::new();

    for tid in track_ids {
        let mut group: Vec<&PlaylistClip> = s
            .clips
            .iter()
            .filter(|(t, c)| *t == tid && selected.contains(&c.id))
            .map(|(_, c)| c)
            .collect();
        if group.is_empty() {
            continue;
        }
        group.sort_by_key(|c| c.start_tick);
        let min_start = group.iter().map(|c| c.start_tick).min().unwrap_or(0);
        let max_end = group
            .iter()
            .map(|c| c.start_tick.saturating_add(c.duration_ticks))
            .max()
            .unwrap_or(min_start);
        let total_dur = max_end.saturating_sub(min_start).max(min_clip_ticks(s.ppqn));

        let first = group[0];
        let new_id = s.next_clip_id;
        s.next_clip_id += 1;
        let new_clip = PlaylistClip {
            id: new_id,
            name: first.name.clone(),
            start_tick: min_start,
            duration_ticks: total_dur,
            clip_type: first.clip_type.clone(),
            color: first.color,
        };
        for c in &group {
            to_remove.push(c.id);
        }
        consolidated_ids.push(new_id);
        to_add.push((tid, new_clip));
    }

    if to_add.is_empty() {
        return 0;
    }
    s.clips.retain(|(_, c)| !to_remove.contains(&c.id));
    s.clips.extend(to_add);
    s.selected_clips = consolidated_ids.clone();
    s.needs_full_sync = true;
    consolidated_ids.len()
}

/// Set the start of selected clips that span across `tick` to `tick` (trimming the left edge).
pub fn set_clip_start_at(s: &mut PlaylistState, tick: u64) -> usize {
    let min_ticks = min_clip_ticks(s.ppqn);
    let selected = s.selected_clips.clone();
    let mut count = 0usize;
    for (_, clip) in &mut s.clips {
        if !selected.contains(&clip.id) {
            continue;
        }
        let end_tick = clip.start_tick.saturating_add(clip.duration_ticks);
        if tick <= clip.start_tick || tick >= end_tick.saturating_sub(min_ticks) {
            continue;
        }
        let delta = tick - clip.start_tick;
        clip.start_tick = tick;
        clip.duration_ticks = end_tick - tick;
        if let ClipType::Audio {
            ref mut sample_offset_ticks,
            ..
        } = clip.clip_type
        {
            *sample_offset_ticks = sample_offset_ticks.saturating_add(delta);
        }
        count += 1;
    }
    if count > 0 {
        s.needs_full_sync = true;
    }
    count
}

/// Insert `duration_ticks` of silence at `at_tick`, splitting any clip that crosses `at_tick`
/// and shifting everything at or after `at_tick` to the right.
pub fn insert_silence_at(s: &mut PlaylistState, at_tick: u64, duration_ticks: u64) {
    if duration_ticks == 0 {
        return;
    }
    let min_ticks = min_clip_ticks(s.ppqn);
    let mut right_parts: Vec<(usize, PlaylistClip)> = Vec::new();
    for (tid, clip) in &mut s.clips {
        let end_tick = clip.start_tick.saturating_add(clip.duration_ticks);
        if clip.start_tick >= at_tick {
            clip.start_tick = clip.start_tick.saturating_add(duration_ticks);
        } else if end_tick > at_tick.saturating_add(min_ticks)
            && at_tick > clip.start_tick.saturating_add(min_ticks)
        {
            let left_dur = at_tick - clip.start_tick;
            let right_dur = end_tick - at_tick;
            let mut right_clip = clip.clone();
            let new_id = s.next_clip_id;
            s.next_clip_id += 1;
            right_clip.id = new_id;
            right_clip.start_tick = at_tick.saturating_add(duration_ticks);
            right_clip.duration_ticks = right_dur;
            if let ClipType::Audio {
                ref mut sample_offset_ticks,
                ..
            } = right_clip.clip_type
            {
                *sample_offset_ticks = sample_offset_ticks.saturating_add(left_dur);
            }
            clip.duration_ticks = left_dur;
            right_parts.push((*tid, right_clip));
        }
    }
    s.clips.extend(right_parts);
    s.needs_full_sync = true;
}

/// Duplicate the time range `[start_tick, end_tick)` immediately after `end_tick`,
/// shifting existing clips at or after `end_tick` to the right by `end_tick - start_tick`.
pub fn duplicate_time_range(s: &mut PlaylistState, start_tick: u64, end_tick: u64) {
    if end_tick <= start_tick {
        return;
    }
    let len = end_tick - start_tick;
    let min_ticks = min_clip_ticks(s.ppqn);
    let mut duplicated: Vec<(usize, PlaylistClip)> = Vec::new();

    for (tid, clip) in &mut s.clips {
        let clip_end = clip.start_tick.saturating_add(clip.duration_ticks);
        if clip.start_tick >= end_tick {
            clip.start_tick = clip.start_tick.saturating_add(len);
        } else if clip_end > start_tick && clip.start_tick < end_tick {
            let ov_start = clip.start_tick.max(start_tick);
            let ov_end = clip_end.min(end_tick);
            let ov_dur = ov_end.saturating_sub(ov_start);
            if ov_dur >= min_ticks {
                let mut copy = clip.clone();
                let new_id = s.next_clip_id;
                s.next_clip_id += 1;
                copy.id = new_id;
                copy.start_tick = end_tick.saturating_add(ov_start - start_tick);
                copy.duration_ticks = ov_dur;
                if let ClipType::Audio {
                    ref mut sample_offset_ticks,
                    ..
                } = copy.clip_type
                {
                    *sample_offset_ticks =
                        sample_offset_ticks.saturating_add(ov_start - clip.start_tick);
                }
                duplicated.push((*tid, copy));
            }
            // If clip extended past end_tick, shift its tail right
            if clip_end > end_tick {
                let tail_dur = clip_end - end_tick;
                let mut tail = clip.clone();
                let tail_id = s.next_clip_id;
                s.next_clip_id += 1;
                tail.id = tail_id;
                tail.start_tick = end_tick.saturating_add(len);
                tail.duration_ticks = tail_dur;
                if let ClipType::Audio {
                    ref mut sample_offset_ticks,
                    ..
                } = tail.clip_type
                {
                    *sample_offset_ticks =
                        sample_offset_ticks.saturating_add(end_tick - clip.start_tick);
                }
                clip.duration_ticks = end_tick - clip.start_tick;
                duplicated.push((*tid, tail));
            }
        }
    }
    s.clips.extend(duplicated);
    s.needs_full_sync = true;
}

/// Remove the time range `[start_tick, end_tick)` and shift all subsequent clips left by `end_tick - start_tick`.
pub fn remove_time_range(s: &mut PlaylistState, start_tick: u64, end_tick: u64) {
    if end_tick <= start_tick {
        return;
    }
    let len = end_tick - start_tick;
    let min_ticks = min_clip_ticks(s.ppqn);
    let mut updated: Vec<(usize, PlaylistClip)> = Vec::with_capacity(s.clips.len());

    for (tid, mut clip) in s.clips.drain(..) {
        let clip_end = clip.start_tick.saturating_add(clip.duration_ticks);
        if clip_end <= start_tick {
            updated.push((tid, clip));
        } else if clip.start_tick >= end_tick {
            clip.start_tick = clip.start_tick.saturating_sub(len);
            updated.push((tid, clip));
        } else if clip.start_tick < start_tick && clip_end > end_tick {
            // Spans across the whole removed range: keep left + right
            let left_dur = start_tick - clip.start_tick;
            let right_dur = clip_end - end_tick;
            if right_dur >= min_ticks {
                let mut right = clip.clone();
                let new_id = s.next_clip_id;
                s.next_clip_id += 1;
                right.id = new_id;
                right.start_tick = start_tick;
                right.duration_ticks = right_dur;
                if let ClipType::Audio {
                    ref mut sample_offset_ticks,
                    ..
                } = right.clip_type
                {
                    *sample_offset_ticks =
                        sample_offset_ticks.saturating_add(end_tick - clip.start_tick);
                }
                updated.push((tid, right));
            }
            if left_dur >= min_ticks {
                clip.duration_ticks = left_dur;
                updated.push((tid, clip));
            }
        } else if clip.start_tick < start_tick {
            // Overlaps left boundary
            let left_dur = start_tick - clip.start_tick;
            if left_dur >= min_ticks {
                clip.duration_ticks = left_dur;
                updated.push((tid, clip));
            }
        } else if clip_end > end_tick {
            // Overlaps right boundary
            let delta = end_tick - clip.start_tick;
            let right_dur = clip_end - end_tick;
            if right_dur >= min_ticks {
                clip.start_tick = start_tick;
                clip.duration_ticks = right_dur;
                if let ClipType::Audio {
                    ref mut sample_offset_ticks,
                    ..
                } = clip.clip_type
                {
                    *sample_offset_ticks = sample_offset_ticks.saturating_add(delta);
                }
                updated.push((tid, clip));
            }
        }
        // Else: completely inside [start_tick, end_tick), removed.
    }

    s.clips = updated;
    let alive_ids: Vec<usize> = s.clips.iter().map(|(_, c)| c.id).collect();
    s.selected_clips.retain(|id| alive_ids.contains(id));
    s.needs_full_sync = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adaptive_grid_scales_with_zoom() {
        let ppqn = DEFAULT_PPQN; // 960
        let (s_out, lbl_out, _) = adaptive_grid_step(ppqn, 4, 0.005);
        assert_eq!(lbl_out, "1/1");
        assert_eq!(s_out, ppqn * 4);

        let (s_mid, lbl_mid, _) = adaptive_grid_step(ppqn, 4, 0.06);
        assert_eq!(lbl_mid, "1/16");
        assert_eq!(s_mid, ppqn / 4);

        let (s_in, lbl_in, _) = adaptive_grid_step(ppqn, 4, 0.5);
        assert_eq!(lbl_in, "1/64");
        assert_eq!(s_in, ppqn / 16);
    }

    #[test]
    fn ruler_bar_step_prevents_overlap() {
        assert_eq!(ruler_bar_step(40.0), 1);
        assert_eq!(ruler_bar_step(16.0), 2);
        assert_eq!(ruler_bar_step(8.0), 4);
        assert_eq!(ruler_bar_step(4.0), 8);
        assert_eq!(ruler_bar_step(2.0), 16);
    }

    #[test]
    fn split_selected_clips_preserves_audio_offset() {
        let mut s = PlaylistState::default();
        s.clips.push((
            1,
            PlaylistClip {
                id: 1,
                name: "Bass".into(),
                start_tick: 960,
                duration_ticks: 3840,
                clip_type: ClipType::Audio {
                    sample_path: "bass.wav".into(),
                    peaks: vec![0.5],
                    sample_offset_ticks: 480,
                    total_sample_ticks: 7680,
                },
                color: Color32::WHITE,
            },
        ));
        s.selected_clips = vec![1];
        s.next_clip_id = 2;

        let count = split_selected_clips_at(&mut s, 2880);
        assert_eq!(count, 1);
        assert_eq!(s.clips.len(), 2);
        assert_eq!(s.clips[0].1.duration_ticks, 1920);
        assert_eq!(s.clips[1].1.start_tick, 2880);
        assert_eq!(s.clips[1].1.duration_ticks, 1920);
        if let ClipType::Audio { sample_offset_ticks, .. } = s.clips[1].1.clip_type {
            assert_eq!(sample_offset_ticks, 480 + 1920);
        } else {
            panic!("expected audio clip");
        }
    }

    #[test]
    fn trim_apply_clamps_audio_bounds() {
        let (st, dur, off) = trim_apply(960, 1920, 0, 3840, TrimEdge::Right, 5000, 240, 60);
        assert_eq!(st, 960);
        assert_eq!(dur, 3840);
        assert_eq!(off, 0);

        let (st2, dur2, off2) = trim_apply(960, 1920, 0, 3840, TrimEdge::Left, 1440, 240, 60);
        assert_eq!(st2, 1440);
        assert_eq!(dur2, 1440);
        assert_eq!(off2, 480);
    }

    #[test]
    fn mmb_drag_up_zooms_in() {
        assert!(mmb_drag_notches(-120.0) > 0.0);
        assert!(mmb_drag_notches(120.0) < 0.0);
    }

    #[test]
    fn consolidate_and_time_ops_work() {
        let mut s = PlaylistState::default();
        s.clips.push((
            1,
            PlaylistClip {
                id: 1,
                name: "A".into(),
                start_tick: 0,
                duration_ticks: 960,
                clip_type: ClipType::Pattern { pattern_id: 1 },
                color: Color32::WHITE,
            },
        ));
        s.clips.push((
            1,
            PlaylistClip {
                id: 2,
                name: "B".into(),
                start_tick: 1920,
                duration_ticks: 960,
                clip_type: ClipType::Pattern { pattern_id: 2 },
                color: Color32::WHITE,
            },
        ));
        s.next_clip_id = 3;
        s.selected_clips = vec![1, 2];

        assert_eq!(consolidate_selected_clips(&mut s), 1);
        assert_eq!(s.clips.len(), 1);
        assert_eq!(s.clips[0].1.start_tick, 0);
        assert_eq!(s.clips[0].1.duration_ticks, 2880);

        insert_silence_at(&mut s, 960, 960);
        assert_eq!(s.clips.len(), 2);
        assert_eq!(s.clips[0].1.duration_ticks, 960);
        assert_eq!(s.clips[1].1.start_tick, 1920);
        assert_eq!(s.clips[1].1.duration_ticks, 1920);

        remove_time_range(&mut s, 960, 1920);
        assert_eq!(s.clips.len(), 2);
        assert_eq!(s.clips[1].1.start_tick, 960);
    }
}
