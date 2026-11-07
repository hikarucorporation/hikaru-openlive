// Piano Roll / Editor MIDI
use std::collections::{HashMap, HashSet};

use crate::app::AppState;
use crate::audio_proxy::{AudioProxy, GuiCommand};
use crate::theme;
use super::hikaru_native_plugins::hikaru_opendms::OpenDms;
use super::mixer::Track;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionDragHandle {
    None,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PianoRollMode {
    Keys,
    Drums,
}

#[derive(Debug, Clone)]
pub struct MidiNote {
    pub pitch: u8,
    pub start_tick: u64,
    pub duration_ticks: u64,
    pub velocity: u8,
}

pub struct PianoRollState {
    pub mode: PianoRollMode,
    pub zoom_x: f32,
    pub key_height: f32,
    pub scroll_offset: egui::Vec2,
    pub layout_initialized: bool,
    pub layout_wait_frames: u8,
    pub notes: Vec<MidiNote>,
    pub drum_map: HashMap<u8, String>,
    pub playhead_tick: u64,
    pub prev_playhead_tick: u64,
    pub selection_start_tick: u64,
    pub selection_end_tick: u64,
    pub selection_active: bool,
    pub selection_dragging: bool,
    pub selection_preview_start_tick: u64,
    pub selection_preview_end_tick: u64,
    pub selection_preview_active: bool,
    pub selection_drag_handle: SelectionDragHandle,
    pub loop_enabled: bool,
    pub loop_transport_start_tick: u64,
    pub loop_start_instant: Option<std::time::Instant>,
    pub note_resize_active: bool,
    pub note_resize_handle: SelectionDragHandle,
    pub note_resize_index: Option<usize>,
    pub note_resize_targets: Vec<(usize, u64, u64)>,
    pub note_resize_delta_start: i64,
    pub note_resize_delta_duration: i64,
    pub selected_notes: HashSet<usize>,
    pub note_marquee_active: bool,
    pub note_marquee_start: Option<egui::Pos2>,
    pub note_marquee_current: Option<egui::Pos2>,
    pub note_marquee_additive: bool,
    pub notes_source_slot: Option<(usize, usize)>,
    pub note_move_active: bool,
    pub note_move_grab_index: Option<usize>,
    pub note_move_orig: Vec<(usize, u64, u8)>,
    pub note_move_preview_delta_ticks: i64,
    pub note_move_preview_delta_rows: i32,
    pub note_move_origin_pointer: Option<egui::Pos2>,
}

impl Default for PianoRollState {
    fn default() -> Self {
        Self {
            mode: PianoRollMode::Keys,
            zoom_x: DEFAULT_ZOOM_X,
            key_height: DEFAULT_KEY_HEIGHT,
            scroll_offset: egui::Vec2::ZERO,
            layout_initialized: false,
            layout_wait_frames: 0,
            notes: Vec::new(),
            drum_map: HashMap::new(),
            playhead_tick: 0,
            prev_playhead_tick: 0,
            selection_start_tick: 0,
            selection_end_tick: 0,
            selection_active: false,
            selection_dragging: false,
            selection_preview_start_tick: 0,
            selection_preview_end_tick: 0,
            selection_preview_active: false,
            selection_drag_handle: SelectionDragHandle::None,
            loop_enabled: false,
            loop_transport_start_tick: 0,
            loop_start_instant: None,
            note_resize_active: false,
            note_resize_handle: SelectionDragHandle::None,
            note_resize_index: None,
            note_resize_targets: Vec::new(),
            note_resize_delta_start: 0,
            note_resize_delta_duration: 0,
            selected_notes: HashSet::new(),
            note_marquee_active: false,
            note_marquee_start: None,
            note_marquee_current: None,
            note_marquee_additive: false,
            notes_source_slot: None,
            note_move_active: false,
            note_move_grab_index: None,
            note_move_orig: Vec::new(),
            note_move_preview_delta_ticks: 0,
            note_move_preview_delta_rows: 0,
            note_move_origin_pointer: None,
        }
    }
}

impl PianoRollState {
    pub fn clear_selection(&mut self) {
        self.selection_start_tick = 0;
        self.selection_end_tick = 0;
        self.selection_active = false;
        self.selection_dragging = false;
        self.selection_preview_start_tick = 0;
        self.selection_preview_end_tick = 0;
        self.selection_preview_active = false;
        self.selection_drag_handle = SelectionDragHandle::None;
        self.loop_enabled = false;
        self.loop_transport_start_tick = 0;
        self.loop_start_instant = None;
        self.note_resize_active = false;
        self.note_resize_handle = SelectionDragHandle::None;
        self.note_resize_index = None;
        self.note_resize_targets.clear();
        self.note_resize_delta_start = 0;
        self.note_resize_delta_duration = 0;
        self.note_move_active = false;
        self.note_move_grab_index = None;
        self.note_move_orig.clear();
        self.note_move_preview_delta_ticks = 0;
        self.note_move_preview_delta_rows = 0;
        self.note_move_origin_pointer = None;
    }

    pub fn clear_note_selection(&mut self) {
        self.selected_notes.clear();
        self.note_marquee_active = false;
        self.note_marquee_start = None;
        self.note_marquee_current = None;
        self.note_marquee_additive = false;
        self.note_move_active = false;
        self.note_move_grab_index = None;
        self.note_move_orig.clear();
        self.note_move_preview_delta_ticks = 0;
        self.note_move_preview_delta_rows = 0;
        self.note_move_origin_pointer = None;
    }
}

pub fn sync_drum_map_from_opendms(state: &mut PianoRollState, opendms: &OpenDms) {
    state.drum_map.clear();
    for pad in &opendms.pads {
        if let Some(ref path) = pad.sample_path {
            let name = std::path::Path::new(path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Sample")
                .to_string();
            state.drum_map.insert(pad.midi_note, name);
        }
    }
    state.mode = if state.drum_map.is_empty() {
        PianoRollMode::Keys
    } else {
        PianoRollMode::Drums
    };
}

fn find_opendms_in_track(track: &Track) -> Option<&OpenDms> {
    track.effects.iter().find_map(|slot| slot.dms_state.as_ref())
}

pub const TICKS_PER_BEAT: u64 = 960;
pub const TICKS_PER_BAR: u64 = 3840;
pub const RULER_HEIGHT: f32 = 24.0;
pub const MIDI_KEY_COUNT: u32 = 128;
pub const SIDEBAR_WIDTH: f32 = 120.0;
pub const DEFAULT_KEY_HEIGHT: f32 = 16.0;
pub const DEFAULT_ZOOM_X: f32 = 0.06;
pub const MIN_ZOOM_X: f32 = 0.008;
pub const MAX_ZOOM_X: f32 = 0.5;
pub const DEFAULT_BARS: u64 = 32;
pub const FOCUS_PITCH: u8 = 60;
pub const BARS_FIT_TO_PANEL: f32 = 4.0;
const ROW_OVERSCAN: u32 = 4;
const TICK_OVERSCAN: u64 = TICKS_PER_BAR;
const MIN_VIEWPORT_W: f32 = 480.0;
const MIN_VIEWPORT_H: f32 = 180.0;

const PITCH_CLASS_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

pub fn pitch_name(pitch: u8) -> String {
    let octave = (pitch as i32 / 12) - 1;
    let name = PITCH_CLASS_NAMES[(pitch % 12) as usize];
    format!("{}{}", name, octave)
}

pub fn is_black_key(pitch: u8) -> bool {
    matches!(pitch % 12, 1 | 3 | 6 | 8 | 10)
}

pub fn quantize_tick(tick: u64, step: u64) -> u64 {
    if step <= 1 {
        return tick;
    }
    let half = step / 2;
    ((tick + half) / step) * step
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridViewport {
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub width: f32,
    pub height: f32,
    pub row_start: u32,
    pub row_end: u32,
    pub tick_start: u64,
    pub tick_end: u64,
}

pub fn compute_grid_viewport(
    scroll_x: f32,
    scroll_y: f32,
    viewport_w: f32,
    viewport_h: f32,
    key_height: f32,
    zoom_x: f32,
) -> GridViewport {
    let kh = if key_height > 0.5 { key_height } else { DEFAULT_KEY_HEIGHT };
    let zx = if zoom_x > f32::EPSILON { zoom_x } else { MIN_ZOOM_X };
    let width = viewport_w.max(MIN_VIEWPORT_W);
    let height = viewport_h.max(MIN_VIEWPORT_H);

    let rows_top = scroll_y - RULER_HEIGHT;
    let rows_bottom = scroll_y + height - RULER_HEIGHT;
    let row_start = ((rows_top / kh).floor() as i64 - i64::from(ROW_OVERSCAN))
        .clamp(0, i64::from(MIDI_KEY_COUNT) - 1) as u32;
    let row_end = (((rows_bottom / kh).ceil() as i64 + i64::from(ROW_OVERSCAN))
        .clamp(0, i64::from(MIDI_KEY_COUNT)) as u32)
        .max(row_start + 1)
        .min(MIDI_KEY_COUNT);

    let ticks_left = scroll_x - SIDEBAR_WIDTH;
    let ticks_right = scroll_x + width - SIDEBAR_WIDTH;
    let tick_start = ((ticks_left / zx).floor() as i64 - TICK_OVERSCAN as i64).max(0) as u64;
    let tick_end = (((ticks_right / zx).ceil() as i64 + TICK_OVERSCAN as i64).max(0) as u64)
        .max(tick_start);

    GridViewport {
        scroll_x,
        scroll_y,
        width,
        height,
        row_start,
        row_end,
        tick_start,
        tick_end,
    }
}

pub fn note_visible_in(start_tick: u64, duration_ticks: u64, pitch: u8, vp: &GridViewport) -> bool {
    let row = i64::from(MIDI_KEY_COUNT) - 1 - i64::from(pitch);
    if row < i64::from(vp.row_start) || row >= i64::from(vp.row_end) {
        return false;
    }
    let end_tick = start_tick.saturating_add(duration_ticks);
    end_tick >= vp.tick_start && start_tick <= vp.tick_end
}

pub fn fit_zoom_x(viewport_width: f32, bars: f32) -> f32 {
    let usable_w = (viewport_width - SIDEBAR_WIDTH).max(120.0);
    let total_ticks = bars.max(1.0) * TICKS_PER_BAR as f32;
    (usable_w / total_ticks).clamp(MIN_ZOOM_X, MAX_ZOOM_X)
}

pub fn focus_scroll_y(viewport_height: f32, key_height: f32, pitch: u8) -> f32 {
    let row = (MIDI_KEY_COUNT.saturating_sub(1) - u32::from(pitch)) as f32;
    let target_y = RULER_HEIGHT + row * key_height;
    (target_y - viewport_height * 0.5).max(0.0)
}

pub fn total_ticks_for(max_note_tick: u64, playhead_tick: u64) -> u64 {
    let default_ticks = DEFAULT_BARS * TICKS_PER_BAR;
    max_note_tick.max(playhead_tick).max(default_ticks)
}

pub fn trigger_playhead_notes(
    state: &mut PianoRollState,
    tracks: &[Track],
    selected_track_index: usize,
    audio_proxy: &AudioProxy,
) {
    let current_tick = state.playhead_tick;
    let prev_tick = state.prev_playhead_tick;

    if current_tick != prev_tick {
        for note in &state.notes {
            if (prev_tick < note.start_tick || prev_tick > current_tick)
                && current_tick >= note.start_tick
                && current_tick < note.start_tick + 240
            {
                let velocity_scale = note.velocity as f32 / 127.0;
                preview_drum_pad(
                    tracks,
                    selected_track_index,
                    note.pitch,
                    velocity_scale,
                    audio_proxy,
                );
            }
        }
    }

    state.prev_playhead_tick = state.playhead_tick;
}

fn preview_drum_pad(
    tracks: &[Track],
    selected_track_index: usize,
    pitch: u8,
    _velocity: f32,
    audio_proxy: &AudioProxy,
) {
    if let Some(track) = tracks.get(selected_track_index) {
        if let Some(opendms) = find_opendms_in_track(track) {
            if let Some(pad) = opendms.pads.iter().find(|p| p.midi_note == pitch) {
                if let Some(ref path) = pad.sample_path {
                    audio_proxy.send(GuiCommand::PreviewSample {
                        path: path.clone(),
                        volume: 1.0,
                        speed: 1.0,
                    });
                }
            }
        }
    }
}

pub fn sync_frame(prs: &mut PianoRollState) -> bool {
    if !prs.layout_initialized {
        prs.scroll_offset.y = focus_scroll_y(360.0, prs.key_height, FOCUS_PITCH);
        prs.layout_initialized = true;
        return true;
    }
    false
}

/// Render the Piano Roll editor panel using egui.
pub fn render(ui: &mut egui::Ui, app: &mut AppState) {
    let prs = &mut app.piano_roll_state;
    let total_width = ui.available_width();
    let total_height = ui.available_height();

    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(total_width, total_height),
        egui::Sense::click_and_drag(),
    );

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme::DAW_BG_LAYER_1);

    let sidebar_w = SIDEBAR_WIDTH;
    let ruler_h = RULER_HEIGHT;
    let key_h = prs.key_height;
    let zoom_x = prs.zoom_x;

    let content_x = rect.min.x + sidebar_w;
    let content_y = rect.min.y + ruler_h;
    let grid_rect = egui::Rect::from_min_max(
        egui::pos2(content_x, content_y),
        rect.max,
    );

    // Scroll & zoom
    if response.hovered() {
        let delta = ui.input(|i| i.smooth_scroll_delta);
        if delta.x != 0.0 || delta.y != 0.0 {
            if ui.input(|i| i.modifiers.ctrl) {
                prs.zoom_x = (prs.zoom_x * (1.0 + delta.y * 0.01)).clamp(MIN_ZOOM_X, MAX_ZOOM_X);
            } else {
                prs.scroll_offset.x = (prs.scroll_offset.x + delta.x).min(0.0);
                let max_y = (MIDI_KEY_COUNT as f32 * key_h - grid_rect.height()).max(0.0);
                prs.scroll_offset.y = (prs.scroll_offset.y - delta.y).clamp(0.0, max_y);
            }
        }
    }

    let scroll_x = prs.scroll_offset.x;
    let scroll_y = prs.scroll_offset.y;

    // --- 1. RULER ---
    let ruler_rect = egui::Rect::from_min_max(
        egui::pos2(content_x, rect.min.y),
        egui::pos2(rect.max.x, content_y),
    );
    painter.rect_filled(ruler_rect, 0.0, theme::DAW_RULER_BG);
    painter.line_segment([ruler_rect.left_bottom(), ruler_rect.right_bottom()], egui::Stroke::new(1.0_f32, theme::DAW_BORDER_SUBTLE));

    let first_bar = ((-scroll_x) / (TICKS_PER_BAR as f32 * zoom_x)).floor() as u64;
    let last_bar = first_bar + (ruler_rect.width() / (TICKS_PER_BAR as f32 * zoom_x)).ceil() as u64 + 2;

    for b in first_bar..=last_bar {
        let px = content_x + scroll_x + (b * TICKS_PER_BAR) as f32 * zoom_x;
        if px >= content_x && px <= rect.max.x {
            painter.line_segment(
                [egui::pos2(px, ruler_rect.min.y + 10.0), egui::pos2(px, ruler_rect.max.y)],
                egui::Stroke::new(1.0_f32, theme::DAW_GRID_MAJOR),
            );
            painter.text(
                egui::pos2(px + 4.0, ruler_rect.min.y + 2.0),
                egui::Align2::LEFT_TOP,
                format!("{}", b + 1),
                egui::FontId::monospace(10.0),
                theme::TEXT_MUTED,
            );
        }
    }

    // --- 2. PIANO KEYS SIDEBAR ---
    let sidebar_rect = egui::Rect::from_min_max(
        egui::pos2(rect.min.x, content_y),
        egui::pos2(content_x, rect.max.y),
    );
    painter.rect_filled(sidebar_rect, 0.0, theme::DAW_PANEL_BG);
    painter.line_segment([sidebar_rect.right_top(), sidebar_rect.right_bottom()], egui::Stroke::new(1.0_f32, theme::DAW_BORDER_SUBTLE));

    // Top-left header
    let tl_rect = egui::Rect::from_min_max(rect.min, egui::pos2(content_x, content_y));
    painter.rect_filled(tl_rect, 0.0, theme::DAW_BG_LAYER_2);
    painter.text(
        tl_rect.center(),
        egui::Align2::CENTER_CENTER,
        match prs.mode {
            PianoRollMode::Keys => "PIANO ROLL",
            PianoRollMode::Drums => "DRUM PADS",
        },
        egui::FontId::proportional(11.0),
        theme::TEXT_MUTED,
    );

    let first_row = (scroll_y / key_h).floor() as u32;
    let visible_rows = (sidebar_rect.height() / key_h).ceil() as u32 + 1;
    let last_row = (first_row + visible_rows).min(MIDI_KEY_COUNT);

    let sidebar_painter = painter.with_clip_rect(sidebar_rect);
    for row in first_row..last_row {
        let pitch = (MIDI_KEY_COUNT - 1 - row) as u8;
        let y0 = content_y + row as f32 * key_h - scroll_y;
        let y1 = y0 + key_h;
        let key_rect = egui::Rect::from_min_max(
            egui::pos2(rect.min.x, y0),
            egui::pos2(content_x, y1),
        );

        let is_black = is_black_key(pitch);
        let key_bg = if is_black {
            egui::Color32::from_rgb(26, 26, 28)
        } else {
            egui::Color32::from_rgb(45, 45, 48)
        };
        sidebar_painter.rect_filled(key_rect, 0.0, key_bg);
        sidebar_painter.line_segment([key_rect.left_bottom(), key_rect.right_bottom()], egui::Stroke::new(0.5_f32, theme::DAW_BORDER_SUBTLE));

        let label = match prs.mode {
            PianoRollMode::Keys => pitch_name(pitch),
            PianoRollMode::Drums => {
                if let Some(name) = prs.drum_map.get(&pitch) {
                    name.clone()
                } else {
                    format!("Pad {}", pitch)
                }
            }
        };
        sidebar_painter.text(
            egui::pos2(key_rect.max.x - 6.0, key_rect.center().y),
            egui::Align2::RIGHT_CENTER,
            label,
            egui::FontId::monospace(9.0),
            if is_black { theme::TEXT_MUTED } else { theme::TEXT_PRIMARY },
        );
    }

    // --- 3. GRID & NOTES ---
    let grid_painter = painter.with_clip_rect(grid_rect);

    // Grid row backgrounds
    for row in first_row..last_row {
        let pitch = (MIDI_KEY_COUNT - 1 - row) as u8;
        let y0 = content_y + row as f32 * key_h - scroll_y;
        let y1 = y0 + key_h;
        let is_black = is_black_key(pitch);
        if is_black {
            grid_painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(content_x, y0), egui::pos2(rect.max.x, y1)),
                0.0,
                egui::Color32::from_rgba_unmultiplied(0, 0, 0, 40),
            );
        }
        grid_painter.line_segment(
            [egui::pos2(content_x, y1), egui::pos2(rect.max.x, y1)],
            egui::Stroke::new(0.5_f32, theme::DAW_BORDER_SUBTLE),
        );
    }

    // Grid vertical lines
    for b in first_bar..=last_bar {
        let px = content_x + scroll_x + (b * TICKS_PER_BAR) as f32 * zoom_x;
        if px >= content_x && px <= rect.max.x {
            grid_painter.line_segment(
                [egui::pos2(px, content_y), egui::pos2(px, rect.max.y)],
                egui::Stroke::new(1.0_f32, theme::DAW_GRID_MAJOR),
            );
        }
    }

    // Draw MIDI Notes
    let mut clicked_note_index = None;
    for (idx, note) in prs.notes.iter().enumerate() {
        let row = (MIDI_KEY_COUNT - 1 - note.pitch as u32) as f32;
        let nx = content_x + scroll_x + note.start_tick as f32 * zoom_x;
        let nw = (note.duration_ticks as f32 * zoom_x).max(4.0);
        let ny = content_y + row * key_h - scroll_y + 1.0;
        let nh = key_h - 2.0;

        let note_rect = egui::Rect::from_min_size(
            egui::pos2(nx, ny),
            egui::vec2(nw, nh),
        );

        if note_rect.max.x < content_x || note_rect.min.x > rect.max.x ||
           note_rect.max.y < content_y || note_rect.min.y > rect.max.y {
            continue;
        }

        let is_selected = prs.selected_notes.contains(&idx);
        let note_color = if is_selected {
            theme::ACCENT_HOVER
        } else {
            theme::ACCENT_PRIMARY
        };

        grid_painter.rect_filled(note_rect, 2.0, note_color);
        grid_painter.rect_stroke(
            note_rect,
            2.0,
            egui::Stroke::new(1.0_f32, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 120)),
            egui::StrokeKind::Inside,
        );

        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                if note_rect.contains(pos) {
                    clicked_note_index = Some(idx);
                }
            }
        }
    }

    if let Some(idx) = clicked_note_index {
        if ui.input(|i| i.modifiers.shift) {
            prs.selected_notes.insert(idx);
        } else {
            prs.selected_notes.clear();
            prs.selected_notes.insert(idx);
        }
    }

    // Playhead line
    let playhead_px = content_x + scroll_x + prs.playhead_tick as f32 * zoom_x;
    if playhead_px >= content_x && playhead_px <= rect.max.x {
        grid_painter.line_segment(
            [egui::pos2(playhead_px, rect.min.y), egui::pos2(playhead_px, rect.max.y)],
            egui::Stroke::new(1.5_f32, theme::PLAYHEAD_COLOR),
        );
    }
}
