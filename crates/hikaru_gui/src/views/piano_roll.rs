// crates/hikaru_gui/src/views/piano_roll.rs
// Editor MIDI / Piano Roll 1:1 estilo Bitwig Studio 5.1.7 con assets de Lemon Studio

use std::collections::{HashMap, HashSet};

use egui::{
    pos2, vec2, Align2, Color32, CursorIcon, Pos2, Rect, Sense, Stroke, Ui,
};

use crate::app::AppState;
use crate::audio_proxy::{AudioProxy, GuiCommand};
use crate::theme;
use crate::ui::fonts;
use super::hikaru_native_plugins::hikaru_opendms::OpenDms;
use super::matrix::ClipData;
use super::mixer::Track;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionDragHandle {
    None,
    Left,
    Right,
    Body,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PianoRollMode {
    Keys,
    Drums,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiNote {
    pub pitch: u8,
    pub start_tick: u64,
    pub duration_ticks: u64,
    pub velocity: u8,
}

impl MidiNote {
    #[inline]
    pub fn new(pitch: u8, start_tick: u64, duration_ticks: u64, velocity: u8) -> Self {
        Self {
            pitch,
            start_tick,
            duration_ticks,
            velocity,
        }
    }

    #[inline]
    pub fn end_tick(&self) -> u64 {
        self.start_tick.saturating_add(self.duration_ticks)
    }
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
    pub loop_start_tick: u64,
    pub loop_end_tick: u64,
    pub loop_drag_handle: SelectionDragHandle,
    pub loop_drag_orig_ticks: (u64, u64),
    pub note_resize_active: bool,
    pub note_resize_handle: SelectionDragHandle,
    pub note_resize_index: Option<usize>,
    pub note_resize_targets: Vec<(usize, u64, u64)>,
    pub note_resize_delta_start: i64,
    pub note_resize_delta_duration: i64,
    pub selected_notes: HashSet<usize>,
    pub note_marquee_active: bool,
    pub note_marquee_start: Option<Pos2>,
    pub note_marquee_current: Option<Pos2>,
    pub note_marquee_additive: bool,
    pub notes_source_slot: Option<(usize, usize)>,
    pub note_move_active: bool,
    pub note_move_grab_index: Option<usize>,
    pub note_move_orig: Vec<(usize, u64, u8)>,
    pub note_move_preview_delta_ticks: i64,
    pub note_move_preview_delta_rows: i32,
    pub note_move_origin_pointer: Option<Pos2>,
    pub show_velocity_lane: bool,
    pub velocity_lane_height: f32,
    pub grid_snap_ticks: u64,
    pub audition_on_click: bool,
    pub velocity_drag_idx: Option<usize>,
    pub active_audition_pitch: Option<u8>,
    pub active_virtual_pitches: HashSet<u8>,
    pub musical_typing_octave: i8,
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
            loop_enabled: true,
            loop_transport_start_tick: 0,
            loop_start_instant: None,
            loop_start_tick: 0,
            loop_end_tick: TICKS_PER_BAR,
            loop_drag_handle: SelectionDragHandle::None,
            loop_drag_orig_ticks: (0, TICKS_PER_BAR),
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
            show_velocity_lane: true,
            velocity_lane_height: 70.0,
            grid_snap_ticks: 240, // 1/16th beat (960 / 4)
            audition_on_click: true,
            velocity_drag_idx: None,
            active_audition_pitch: None,
            active_virtual_pitches: HashSet::new(),
            musical_typing_octave: 4, // C4
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
        self.loop_transport_start_tick = 0;
        self.loop_start_instant = None;
        self.loop_drag_handle = SelectionDragHandle::None;
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
        self.velocity_drag_idx = None;
        self.active_audition_pitch = None;
        self.active_virtual_pitches.clear();
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
        self.velocity_drag_idx = None;
        self.active_audition_pitch = None;
        self.active_virtual_pitches.clear();
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

#[allow(dead_code)]
pub fn find_opendms_in_track(track: &Track) -> Option<&OpenDms> {
    track.effects.iter().find_map(|slot| slot.dms_state.as_ref())
}

pub const TICKS_PER_BEAT: u64 = 960;
pub const TICKS_PER_BAR: u64 = 3840;
pub const RULER_HEIGHT: f32 = 24.0;
pub const MIDI_KEY_COUNT: u32 = 128;
pub const SIDEBAR_WIDTH: f32 = 84.0; // 32px octave gutter + 52px piano keys
pub const OCTAVE_GUTTER_WIDTH: f32 = 32.0;
pub const DEFAULT_KEY_HEIGHT: f32 = 14.0;
pub const DEFAULT_ZOOM_X: f32 = 0.08;
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

pub fn track_has_instrument(track: &Track) -> bool {
    track.effects.iter().any(|slot| {
        slot.active && (slot.name == "OpenWavetable" || slot.name == "Hikaru OpenDMS" || slot.external.is_some())
    })
}

pub fn trigger_playhead_notes(
    state: &mut PianoRollState,
    tracks: &[Track],
    selected_track_index: usize,
    audio_proxy: &AudioProxy,
) {
    let track = tracks.get(selected_track_index).or_else(|| {
        if selected_track_index > 0 {
            tracks.get(selected_track_index - 1)
        } else {
            None
        }
    });
    let Some(track) = track else {
        state.prev_playhead_tick = state.playhead_tick;
        return;
    };
    if !track_has_instrument(track) {
        state.prev_playhead_tick = state.playhead_tick;
        return;
    }

    let current_tick = state.playhead_tick;
    let prev_tick = state.prev_playhead_tick;

    if current_tick != prev_tick {
        for note in &state.notes {
            if (prev_tick < note.start_tick || prev_tick > current_tick)
                && current_tick >= note.start_tick
                && current_tick < note.start_tick + 240
            {
                let velocity_scale = note.velocity as f32 / 127.0;
                let eng_tr = if tracks.first().map_or(false, |t| t.is_master) {
                    selected_track_index.saturating_sub(1)
                } else {
                    selected_track_index
                };
                trigger_audition_note(
                    eng_tr,
                    note.pitch,
                    velocity_scale,
                    audio_proxy,
                );
            }
        }
    }

    state.prev_playhead_tick = state.playhead_tick;
}

pub fn trigger_audition_note(
    selected_track_index: usize,
    pitch: u8,
    velocity: f32,
    audio_proxy: &AudioProxy,
) {
    audio_proxy.send(GuiCommand::VirtualNoteOn {
        track_idx: selected_track_index,
        pitch,
        velocity,
    });
}

pub fn release_audition_note(
    selected_track_index: usize,
    pitch: u8,
    audio_proxy: &AudioProxy,
) {
    audio_proxy.send(GuiCommand::VirtualNoteOff {
        track_idx: selected_track_index,
        pitch,
    });
}

pub fn preview_drum_pad(
    _tracks: &[Track],
    selected_track_index: usize,
    pitch: u8,
    velocity: f32,
    audio_proxy: &AudioProxy,
) {
    trigger_audition_note(selected_track_index, pitch, velocity, audio_proxy);
}

pub fn sync_frame(prs: &mut PianoRollState) -> bool {
    if !prs.layout_initialized {
        prs.scroll_offset.y = focus_scroll_y(360.0, prs.key_height, FOCUS_PITCH);
        prs.layout_initialized = true;
        return true;
    }
    false
}

// Colores 1:1 Bitwig Studio 5.1.7 extraídos del video y spec.md
const BW_CANVAS_BG: Color32 = Color32::from_rgb(0x1F, 0x20, 0x23);
const BW_ROW_WHITE: Color32 = Color32::from_rgb(0x24, 0x25, 0x29);
const BW_ROW_BLACK: Color32 = Color32::from_rgb(0x1B, 0x1C, 0x1E);
const BW_GRID_MAJOR: Color32 = Color32::from_rgb(0x35, 0x36, 0x3D);
const BW_GRID_BEAT: Color32 = Color32::from_rgb(0x2A, 0x2B, 0x30);
const BW_GRID_MINOR: Color32 = Color32::from_rgb(0x21, 0x22, 0x26);
const BW_RULER_BG: Color32 = Color32::from_rgb(0x23, 0x24, 0x28);
const BW_SIDEBAR_BG: Color32 = Color32::from_rgb(0x18, 0x19, 0x1C);
const BW_GUTTER_BG: Color32 = Color32::from_rgb(0x14, 0x15, 0x17);
const BW_KEY_IVORY_TOP: Color32 = Color32::from_rgb(0xD2, 0xD6, 0xE2);
const BW_KEY_IVORY_BOT: Color32 = Color32::from_rgb(0xB8, 0xBD, 0xC9);
const BW_KEY_EBONY_TOP: Color32 = Color32::from_rgb(0x2A, 0x2C, 0x31);
const BW_KEY_EBONY_BOT: Color32 = Color32::from_rgb(0x16, 0x17, 0x19);
pub const BW_KEY_HOVER_GLOW: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
const BW_KEY_BORDER: Color32 = Color32::from_rgb(0x4C, 0x4E, 0x56);
const BW_VEL_BG: Color32 = Color32::from_rgb(0x15, 0x16, 0x18);
const BW_VEL_BORDER: Color32 = Color32::from_rgb(0x2A, 0x2B, 0x30);

/// Render the Piano Roll editor panel using egui with 1:1 Bitwig Studio fidelity.
pub fn render(ui: &mut Ui, app: &mut AppState) {
    let (selected_track_idx, selected_slot_coord) = {
        let t_idx = app.selected_track_index;
        let slot_coord = app.matrix_state.selected_slot;
        (t_idx, slot_coord)
    };
    let engine_track_idx = selected_track_idx.saturating_sub(1);
    let has_instrument = app.tracks().get(selected_track_idx).map(track_has_instrument).unwrap_or(false);

    let track_color = app
        .tracks()
        .get(selected_track_idx)
        .map(|t| t.color)
        .unwrap_or(theme::ACCENT_ORANGE);

    // 1. SINCRONIZACIÓN DE NOTAS DEL SLOT ACTIVO (Session Matrix <-> Piano Roll)
    if let Some((t_idx, s_idx)) = selected_slot_coord {
        if app.piano_roll_state.notes_source_slot != Some((t_idx, s_idx)) {
            if let Some(slot) = app.matrix_state.slot(t_idx, s_idx) {
                if let Some(clip) = &slot.clip {
                    if let ClipData::Midi { notes } = &clip.content {
                        app.piano_roll_state.notes = notes
                            .iter()
                            .map(|&(st, p, v, dur)| MidiNote {
                                pitch: p,
                                start_tick: st,
                                duration_ticks: dur as u64,
                                velocity: v,
                            })
                            .collect();
                        app.piano_roll_state.loop_start_tick = clip.loop_start;
                        app.piano_roll_state.loop_end_tick =
                            clip.loop_end.max(clip.loop_start + 960);
                        app.piano_roll_state.notes_source_slot = Some((t_idx, s_idx));
                        app.piano_roll_state.clear_note_selection();
                    }
                }
            }
        }
    }

    let mut audition_event: Option<(u8, f32)> = None;
    let prs = &mut app.piano_roll_state;
    let total_width = ui.available_width();
    let total_height = ui.available_height();

    let (rect, response) = ui.allocate_exact_size(
        vec2(total_width, total_height),
        Sense::click_and_drag(),
    );

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, BW_CANVAS_BG);

    let sidebar_w = SIDEBAR_WIDTH;
    let ruler_h = RULER_HEIGHT;
    let vel_h = if prs.show_velocity_lane {
        prs.velocity_lane_height
    } else {
        0.0
    };
    let content_x = rect.min.x + sidebar_w;
    let content_y = rect.min.y + ruler_h;
    let grid_h = (rect.height() - ruler_h - vel_h).max(60.0);

    let grid_rect = Rect::from_min_size(
        pos2(content_x, content_y),
        vec2(rect.width() - sidebar_w, grid_h),
    );

    let vel_rect = Rect::from_min_size(
        pos2(content_x, content_y + grid_h),
        vec2(rect.width() - sidebar_w, vel_h),
    );

    let vel_sidebar_rect = Rect::from_min_size(
        pos2(rect.min.x, content_y + grid_h),
        vec2(sidebar_w, vel_h),
    );

    let key_h = prs.key_height;
    let zoom_x = prs.zoom_x;

    let pointer_pos = ui.input(|i| i.pointer.interact_pos().or_else(|| i.pointer.hover_pos()));
    let shift_held = ui.input(|i| i.modifiers.shift);
    let ctrl_held = ui.input(|i| i.modifiers.command);
    let alt_held = ui.input(|i| i.modifiers.alt);
    let primary_clicked = response.clicked();
    let double_clicked = response.double_clicked();
    let pointer_released = ui.input(|i| i.pointer.any_released());

    // SCROLL & ZOOM
    if response.hovered() {
        let delta = ui.input(|i| i.smooth_scroll_delta);
        if delta.x != 0.0 || delta.y != 0.0 {
            if ctrl_held && !shift_held {
                let zoom_factor = 1.0 + delta.y * 0.01;
                prs.zoom_x = (prs.zoom_x * zoom_factor).clamp(MIN_ZOOM_X, MAX_ZOOM_X);
            } else if alt_held || (ctrl_held && shift_held) {
                let kh_factor = 1.0 + delta.y * 0.02;
                prs.key_height = (prs.key_height * kh_factor).clamp(8.0, 36.0);
            } else if shift_held {
                prs.scroll_offset.x = (prs.scroll_offset.x + delta.y * 2.0).min(0.0);
            } else {
                prs.scroll_offset.x = (prs.scroll_offset.x + delta.x).min(0.0);
                let max_y = (MIDI_KEY_COUNT as f32 * key_h - grid_rect.height()).max(0.0);
                prs.scroll_offset.y = (prs.scroll_offset.y - delta.y).clamp(0.0, max_y);
            }
        }

        // Paneo con botón del medio o Ctrl+Alt Drag
        if ui.input(|i| i.pointer.middle_down()) || (ctrl_held && alt_held && response.dragged()) {
            let drag_delta = ui.input(|i| i.pointer.delta());
            prs.scroll_offset.x = (prs.scroll_offset.x + drag_delta.x).min(0.0);
            let max_y = (MIDI_KEY_COUNT as f32 * key_h - grid_rect.height()).max(0.0);
            prs.scroll_offset.y = (prs.scroll_offset.y - drag_delta.y).clamp(0.0, max_y);
        }
    }

    let scroll_x = prs.scroll_offset.x;
    let scroll_y = prs.scroll_offset.y;

    // --- TECLADO MUSICAL VIRTUAL (QWERTY Musical Typing estilo Bitwig Studio) ---
    // Filas centrales: A=C, W=C#, S=D, E=D#, D=E, F=F, T=F#, G=G, Y=G#, H=A, U=A#, J=B, K=C+1, O=C#+1, L=D+1, P=D#+1
    // Cambio de octava: Z (abajo), X (arriba)
    if (response.hovered() || app.panel_layout.show_keyboard) && !ctrl_held && !alt_held {
        if ui.input(|i| i.key_pressed(egui::Key::Z)) {
            prs.musical_typing_octave = (prs.musical_typing_octave - 1).clamp(-1, 8);
        }
        if ui.input(|i| i.key_pressed(egui::Key::X)) {
            prs.musical_typing_octave = (prs.musical_typing_octave + 1).clamp(-1, 8);
        }

        let base_pitch = ((prs.musical_typing_octave + 1) as i32 * 12).clamp(0, 112) as u8;
        let typing_map: [(egui::Key, u8); 16] = [
            (egui::Key::A, 0),
            (egui::Key::W, 1),
            (egui::Key::S, 2),
            (egui::Key::E, 3),
            (egui::Key::D, 4),
            (egui::Key::F, 5),
            (egui::Key::T, 6),
            (egui::Key::G, 7),
            (egui::Key::Y, 8),
            (egui::Key::H, 9),
            (egui::Key::U, 10),
            (egui::Key::J, 11),
            (egui::Key::K, 12),
            (egui::Key::O, 13),
            (egui::Key::L, 14),
            (egui::Key::P, 15),
        ];

        for (k, semi) in typing_map {
            let note_pitch = base_pitch + semi;
            if note_pitch <= 127 {
                if ui.input(|i| i.key_pressed(k)) {
                    if prs.active_virtual_pitches.insert(note_pitch) {
                        if has_instrument {
                            trigger_audition_note(
                                engine_track_idx,
                                note_pitch,
                                0.85,
                                &app.audio_proxy,
                            );
                        }
                    }
                }
                if ui.input(|i| i.key_released(k)) {
                    if prs.active_virtual_pitches.remove(&note_pitch) {
                        if has_instrument {
                            release_audition_note(
                                engine_track_idx,
                                note_pitch,
                                &app.audio_proxy,
                            );
                        }
                    }
                }
            }
        }
    }

    // --- TECLADO SHORTCUTS: DELETE / BACKSPACE / CTRL+A / FLECHAS ---
    let mut notes_modified = false;
    let delete_pressed =
        ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace));
    if delete_pressed && !prs.selected_notes.is_empty() {
        let mut to_remove: Vec<usize> = prs.selected_notes.iter().copied().collect();
        to_remove.sort_unstable_by(|a, b| b.cmp(a));
        for idx in to_remove {
            if idx < prs.notes.len() {
                prs.notes.remove(idx);
            }
        }
        prs.selected_notes.clear();
        notes_modified = true;
    }

    if ctrl_held && ui.input(|i| i.key_pressed(egui::Key::A)) {
        prs.selected_notes.clear();
        for i in 0..prs.notes.len() {
            prs.selected_notes.insert(i);
        }
    }

    if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) && !prs.selected_notes.is_empty() {
        let delta_p = if shift_held { 12 } else { 1 };
        for &idx in &prs.selected_notes {
            if let Some(n) = prs.notes.get_mut(idx) {
                n.pitch = (n.pitch + delta_p).min(127);
            }
        }
        notes_modified = true;
    }

    if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) && !prs.selected_notes.is_empty() {
        let delta_p = if shift_held { 12 } else { 1 };
        for &idx in &prs.selected_notes {
            if let Some(n) = prs.notes.get_mut(idx) {
                n.pitch = n.pitch.saturating_sub(delta_p);
            }
        }
        notes_modified = true;
    }

    // --- 1. RULER & LOOP HEADER ---
    let ruler_rect = Rect::from_min_max(
        pos2(content_x, rect.min.y),
        pos2(rect.max.x, content_y),
    );
    let ruler_painter = painter.with_clip_rect(ruler_rect);
    ruler_painter.rect_filled(ruler_rect, 0.0, BW_RULER_BG);
    ruler_painter.line_segment(
        [ruler_rect.left_bottom(), ruler_rect.right_bottom()],
        Stroke::new(1.0_f32, BW_GRID_MAJOR),
    );

    let first_bar = ((-scroll_x) / (TICKS_PER_BAR as f32 * zoom_x)).floor().max(0.0) as u64;
    let last_bar = first_bar + (ruler_rect.width() / (TICKS_PER_BAR as f32 * zoom_x)).ceil() as u64 + 2;

    for b in first_bar..=last_bar {
        let bar_tick = b * TICKS_PER_BAR;
        let px = content_x + scroll_x + bar_tick as f32 * zoom_x;
        if px >= content_x && px <= rect.max.x {
            ruler_painter.line_segment(
                [pos2(px, ruler_rect.min.y + 6.0), pos2(px, ruler_rect.max.y)],
                Stroke::new(1.0_f32, BW_GRID_MAJOR),
            );
            ruler_painter.text(
                pos2(px + 4.0, ruler_rect.min.y + 2.0),
                Align2::LEFT_TOP,
                format!("{}.1", b + 1),
                fonts::semibold(9.5),
                theme::TEXT_MUTED,
            );
        }

        // Subdivisión de beats (1/4)
        for beat in 1..4 {
            let beat_tick = bar_tick + beat * TICKS_PER_BEAT;
            let bx = content_x + scroll_x + beat_tick as f32 * zoom_x;
            if bx >= content_x && bx <= rect.max.x {
                ruler_painter.line_segment(
                    [pos2(bx, ruler_rect.min.y + 14.0), pos2(bx, ruler_rect.max.y)],
                    Stroke::new(0.5_f32, BW_GRID_BEAT),
                );
            }
        }
    }

    // Región de bucle (Loop Region en Ruler)
    let loop_s_px = content_x + scroll_x + prs.loop_start_tick as f32 * zoom_x;
    let loop_e_px = content_x + scroll_x + prs.loop_end_tick as f32 * zoom_x;

    if loop_e_px > content_x && loop_s_px < rect.max.x {
        let loop_bar_rect = Rect::from_min_max(
            pos2(loop_s_px.max(content_x), ruler_rect.max.y - 4.0),
            pos2(loop_e_px.min(rect.max.x), ruler_rect.max.y),
        );
        ruler_painter.rect_filled(loop_bar_rect, 0.0, track_color);

        // Banderas triangulares de inicio (▶) y fin (◀)
        if loop_s_px >= content_x - 10.0 && loop_s_px <= rect.max.x + 10.0 {
            let tri_s = [
                pos2(loop_s_px, ruler_rect.max.y - 12.0),
                pos2(loop_s_px + 8.0, ruler_rect.max.y - 6.0),
                pos2(loop_s_px, ruler_rect.max.y),
            ];
            ruler_painter.add(egui::epaint::PathShape::convex_polygon(
                tri_s.to_vec(),
                track_color,
                Stroke::NONE,
            ));
        }

        if loop_e_px >= content_x - 10.0 && loop_e_px <= rect.max.x + 10.0 {
            let tri_e = [
                pos2(loop_e_px, ruler_rect.max.y - 12.0),
                pos2(loop_e_px - 8.0, ruler_rect.max.y - 6.0),
                pos2(loop_e_px, ruler_rect.max.y),
            ];
            ruler_painter.add(egui::epaint::PathShape::convex_polygon(
                tri_e.to_vec(),
                track_color,
                Stroke::NONE,
            ));
        }
    }

    // --- 2. TECLADO 3D DE PIANO (Sidebar Izquierda estilo Lemon Studio & Bitwig) ---
    let sidebar_rect = Rect::from_min_max(
        pos2(rect.min.x, content_y),
        pos2(content_x, content_y + grid_h),
    );
    let sidebar_painter = painter.with_clip_rect(sidebar_rect);
    sidebar_painter.rect_filled(sidebar_rect, 0.0, BW_SIDEBAR_BG);
    sidebar_painter.line_segment(
        [sidebar_rect.right_top(), sidebar_rect.right_bottom()],
        Stroke::new(1.0_f32, BW_GRID_MAJOR),
    );

    // Gutter de octavas / strip lateral (primeros 22px con nombre de pista y color de track)
    let track_strip_w = 22.0;
    let track_strip_rect = Rect::from_min_max(
        pos2(rect.min.x, content_y),
        pos2(rect.min.x + track_strip_w, content_y + grid_h),
    );
    sidebar_painter.rect_filled(track_strip_rect, 0.0, BW_GUTTER_BG);
    // Barra de color de la pista
    sidebar_painter.rect_filled(
        Rect::from_min_max(pos2(rect.min.x, content_y), pos2(rect.min.x + 3.0, content_y + grid_h)),
        0.0,
        track_color,
    );
    sidebar_painter.line_segment(
        [track_strip_rect.right_top(), track_strip_rect.right_bottom()],
        Stroke::new(1.0_f32, Color32::from_rgb(0x1E, 0x20, 0x24)),
    );

    // Compatibilidad para tests existentes que miden gutter_rect
    let _gutter_rect = Rect::from_min_max(
        pos2(rect.min.x, content_y),
        pos2(rect.min.x + OCTAVE_GUTTER_WIDTH, content_y + grid_h),
    );

    // Esquina superior izquierda (Header del panel estilo Bitwig Studio)
    let tl_rect = Rect::from_min_max(rect.min, pos2(content_x, content_y));
    painter.rect_filled(tl_rect, 0.0, Color32::from_rgb(0x18, 0x19, 0x1D));
    painter.line_segment(
        [tl_rect.right_top(), tl_rect.right_bottom()],
        Stroke::new(1.0_f32, BW_GRID_MAJOR),
    );
    painter.line_segment(
        [tl_rect.left_bottom(), tl_rect.right_bottom()],
        Stroke::new(1.0_f32, BW_GRID_MAJOR),
    );

    // Botones de modo Keys / Drums / Audition / Octava en el header
    let btn_keys_rect = Rect::from_min_size(pos2(tl_rect.min.x + 4.0, tl_rect.min.y + 3.0), vec2(20.0, 18.0));
    let btn_drums_rect = Rect::from_min_size(pos2(tl_rect.min.x + 26.0, tl_rect.min.y + 3.0), vec2(20.0, 18.0));
    let btn_audition_rect = Rect::from_min_size(pos2(tl_rect.min.x + 48.0, tl_rect.min.y + 3.0), vec2(18.0, 18.0));
    let oct_badge_rect = Rect::from_min_size(pos2(tl_rect.min.x + 68.0, tl_rect.min.y + 4.0), vec2(14.0, 16.0));

    if let Some(pos) = pointer_pos {
        if primary_clicked {
            if btn_keys_rect.contains(pos) {
                prs.mode = PianoRollMode::Keys;
            } else if btn_drums_rect.contains(pos) {
                prs.mode = PianoRollMode::Drums;
            } else if btn_audition_rect.contains(pos) {
                prs.audition_on_click = !prs.audition_on_click;
            }
        }
    }

    let is_keys = matches!(prs.mode, PianoRollMode::Keys);
    painter.rect_filled(
        btn_keys_rect,
        2.0,
        if is_keys { Color32::from_rgb(0x2E, 0x30, 0x36) } else { Color32::TRANSPARENT },
    );
    painter.text(
        btn_keys_rect.center(),
        Align2::CENTER_CENTER,
        "🎹",
        fonts::regular(10.0),
        if is_keys { theme::ACCENT_ORANGE } else { theme::TEXT_MUTED },
    );
    if is_keys {
        painter.line_segment(
            [pos2(btn_keys_rect.min.x + 2.0, btn_keys_rect.max.y), pos2(btn_keys_rect.max.x - 2.0, btn_keys_rect.max.y)],
            Stroke::new(2.0_f32, theme::ACCENT_ORANGE),
        );
    }

    let is_drums = matches!(prs.mode, PianoRollMode::Drums);
    painter.rect_filled(
        btn_drums_rect,
        2.0,
        if is_drums { Color32::from_rgb(0x2E, 0x30, 0x36) } else { Color32::TRANSPARENT },
    );
    painter.text(
        btn_drums_rect.center(),
        Align2::CENTER_CENTER,
        "⊞",
        fonts::semibold(11.0),
        if is_drums { theme::ACCENT_ORANGE } else { theme::TEXT_MUTED },
    );
    if is_drums {
        painter.line_segment(
            [pos2(btn_drums_rect.min.x + 2.0, btn_drums_rect.max.y), pos2(btn_drums_rect.max.x - 2.0, btn_drums_rect.max.y)],
            Stroke::new(2.0_f32, theme::ACCENT_ORANGE),
        );
    }

    painter.text(
        btn_audition_rect.center(),
        Align2::CENTER_CENTER,
        if prs.audition_on_click { "🔊" } else { "🔈" },
        fonts::regular(10.0),
        if prs.audition_on_click { Color32::from_rgb(0xDF, 0xE2, 0xEC) } else { theme::TEXT_MUTED },
    );

    // Octava activa de Musical Typing
    painter.text(
        oct_badge_rect.center(),
        Align2::CENTER_CENTER,
        format!("C{}", prs.musical_typing_octave),
        fonts::semibold(8.0),
        theme::ACCENT_ORANGE,
    );

    let first_row = (scroll_y / key_h).floor() as u32;
    let visible_rows = (sidebar_rect.height() / key_h).ceil() as u32 + 1;
    let last_row = (first_row + visible_rows).min(MIDI_KEY_COUNT);

    let keys_start_x = rect.min.x + track_strip_w;
    let black_key_w = 38.0;

    // Recolectar notas activas bajo el cabezal de reproducción
    let mut playhead_active_pitches = HashSet::new();
    for note in &prs.notes {
        if prs.playhead_tick >= note.start_tick && prs.playhead_tick < note.end_tick() {
            playhead_active_pitches.insert(note.pitch);
        }
    }

    // Hit test prioritario de teclas para mouse virtual input y auditioning
    let mut hovered_key_pitch: Option<u8> = None;
    let pointer_primary_down = ui.input(|i| i.pointer.primary_down());

    if let Some(pos) = pointer_pos {
        if sidebar_rect.contains(pos) && pos.x >= keys_start_x {
            // Teclas negras primero (capa superior)
            for row in first_row..last_row {
                let pitch = (MIDI_KEY_COUNT - 1 - row) as u8;
                if is_black_key(pitch) {
                    let y0 = content_y + row as f32 * key_h - scroll_y;
                    let y1 = y0 + key_h;
                    let bk_rect = Rect::from_min_max(
                        pos2(keys_start_x, y0 + 0.5),
                        pos2(keys_start_x + black_key_w, y1 - 0.5),
                    );
                    if bk_rect.contains(pos) {
                        hovered_key_pitch = Some(pitch);
                        break;
                    }
                }
            }

            // Teclas blancas si no tocó ninguna negra
            if hovered_key_pitch.is_none() {
                for row in first_row..last_row {
                    let pitch = (MIDI_KEY_COUNT - 1 - row) as u8;
                    if !is_black_key(pitch) {
                        let y0 = content_y + row as f32 * key_h - scroll_y;
                        let y1 = y0 + key_h;
                        let wk_rect = Rect::from_min_max(
                            pos2(keys_start_x, y0),
                            pos2(content_x, y1),
                        );
                        if wk_rect.contains(pos) {
                            hovered_key_pitch = Some(pitch);
                            break;
                        }
                    }
                }
            }
        }
    }

    // Interacción continua de teclado virtual con el puntero del mouse
    if pointer_primary_down && sidebar_rect.contains(pointer_pos.unwrap_or(Pos2::ZERO)) {
        if let Some(new_p) = hovered_key_pitch {
            if prs.active_audition_pitch != Some(new_p) {
                if let Some(old_p) = prs.active_audition_pitch {
                    if has_instrument {
                        release_audition_note(engine_track_idx, old_p, &app.audio_proxy);
                    }
                }
                prs.active_audition_pitch = Some(new_p);
                if has_instrument {
                    trigger_audition_note(engine_track_idx, new_p, 0.85, &app.audio_proxy);
                }
            }
        }
    } else if pointer_released || !pointer_primary_down {
        if let Some(p) = prs.active_audition_pitch.take() {
            if has_instrument {
                release_audition_note(engine_track_idx, p, &app.audio_proxy);
            }
        }
    }

    // A. Dibujar teclas blancas primero (fondo completo de la fila)
    for row in first_row..last_row {
        let pitch = (MIDI_KEY_COUNT - 1 - row) as u8;
        let is_black = is_black_key(pitch);
        let y0 = content_y + row as f32 * key_h - scroll_y;
        let y1 = y0 + key_h;

        let is_active = prs.active_audition_pitch == Some(pitch)
            || prs.active_virtual_pitches.contains(&pitch)
            || playhead_active_pitches.contains(&pitch);

        let key_rect = Rect::from_min_max(
            pos2(keys_start_x, y0),
            pos2(content_x, y1),
        );
        let hovered = hovered_key_pitch == Some(pitch);

        if !is_black {
            let fill = if is_active {
                Color32::from_rgb(0xFF, 0xFF, 0xFF) // Iluminación blanca 1:1 Bitwig al reproducir
            } else if hovered {
                Color32::from_rgb(0xE6, 0xEA, 0xF5)
            } else {
                BW_KEY_IVORY_TOP
            };
            sidebar_painter.rect_filled(key_rect, 0.0, fill);
            sidebar_painter.line_segment(
                [pos2(key_rect.min.x, key_rect.max.y - 1.0), key_rect.right_bottom()],
                Stroke::new(1.0_f32, BW_KEY_IVORY_BOT),
            );
            sidebar_painter.line_segment(
                [key_rect.left_bottom(), key_rect.right_bottom()],
                Stroke::new(0.5_f32, BW_KEY_BORDER),
            );

            // Si es nota C, dibujar línea divisoria de octava y label nítido
            if pitch % 12 == 0 {
                let oct_label = pitch_name(pitch);
                sidebar_painter.line_segment(
                    [pos2(keys_start_x, y1), pos2(content_x, y1)],
                    Stroke::new(1.0_f32, Color32::from_rgb(0x38, 0x3A, 0x42)),
                );
                sidebar_painter.text(
                    pos2(keys_start_x + 4.0, (y0 + y1) * 0.5),
                    Align2::LEFT_CENTER,
                    oct_label,
                    fonts::semibold(9.5),
                    if is_active { Color32::from_rgb(0x10, 0x12, 0x16) } else { Color32::from_rgb(0x30, 0x32, 0x3A) },
                );
            } else if key_h >= 13.0 {
                // Pitch class sutil en tecla blanca
                sidebar_painter.text(
                    pos2(key_rect.max.x - 4.0, key_rect.center().y),
                    Align2::RIGHT_CENTER,
                    PITCH_CLASS_NAMES[(pitch % 12) as usize],
                    fonts::regular(8.5),
                    Color32::from_rgb(0x6A, 0x6E, 0x7C),
                );
            }
        }
    }

    // B. Dibujar teclas negras encima (relieve ébano 3D con iluminación activa)
    for row in first_row..last_row {
        let pitch = (MIDI_KEY_COUNT - 1 - row) as u8;
        if is_black_key(pitch) {
            let y0 = content_y + row as f32 * key_h - scroll_y;
            let y1 = y0 + key_h;
            let key_rect = Rect::from_min_max(
                pos2(keys_start_x, y0 + 0.5),
                pos2(keys_start_x + black_key_w, y1 - 0.5),
            );
            let hovered = hovered_key_pitch == Some(pitch);
            let is_active = prs.active_audition_pitch == Some(pitch)
                || prs.active_virtual_pitches.contains(&pitch)
                || playhead_active_pitches.contains(&pitch);

            let fill = if is_active {
                Color32::from_rgb(0xEE, 0xF2, 0xFC) // Brillo 1:1 Bitwig cuando suena
            } else if hovered {
                Color32::from_rgb(0x3E, 0x41, 0x4A)
            } else {
                BW_KEY_EBONY_BOT
            };
            sidebar_painter.rect_filled(key_rect, 2.0, fill);
            // Bisel superior iluminado
            sidebar_painter.line_segment(
                [key_rect.left_top(), key_rect.right_top()],
                Stroke::new(0.8_f32, if is_active { Color32::WHITE } else { BW_KEY_EBONY_TOP }),
            );

            if key_h >= 13.0 {
                sidebar_painter.text(
                    pos2(key_rect.max.x - 3.0, key_rect.center().y),
                    Align2::RIGHT_CENTER,
                    pitch_name(pitch),
                    fonts::regular(8.0),
                    if is_active { Color32::from_rgb(0x18, 0x1A, 0x20) } else { Color32::from_rgb(0x82, 0x86, 0x94) },
                );
            }
        }
    }

    // --- 3. GRILLA DE NOTAS Y LIENZO ---
    let grid_painter = painter.with_clip_rect(grid_rect);

    // Filas alternadas de fondo
    for row in first_row..last_row {
        let pitch = (MIDI_KEY_COUNT - 1 - row) as u8;
        let is_black = is_black_key(pitch);
        let y0 = content_y + row as f32 * key_h - scroll_y;
        let y1 = y0 + key_h;

        let row_bg = if is_black { BW_ROW_BLACK } else { BW_ROW_WHITE };
        grid_painter.rect_filled(
            Rect::from_min_max(pos2(content_x, y0), pos2(rect.max.x, y1)),
            0.0,
            row_bg,
        );

        let stroke_col = if pitch % 12 == 0 {
            BW_GRID_MAJOR
        } else {
            BW_GRID_MINOR
        };
        grid_painter.line_segment(
            [pos2(content_x, y1), pos2(rect.max.x, y1)],
            Stroke::new(0.5_f32, stroke_col),
        );
    }

    // Líneas verticales de compás y beat
    for b in first_bar..=last_bar {
        let bar_tick = b * TICKS_PER_BAR;
        let px = content_x + scroll_x + bar_tick as f32 * zoom_x;
        if px >= content_x && px <= rect.max.x {
            grid_painter.line_segment(
                [pos2(px, content_y), pos2(px, content_y + grid_h)],
                Stroke::new(1.0_f32, BW_GRID_MAJOR),
            );
        }

        for beat in 1..4 {
            let beat_tick = bar_tick + beat * TICKS_PER_BEAT;
            let bx = content_x + scroll_x + beat_tick as f32 * zoom_x;
            if bx >= content_x && bx <= rect.max.x {
                grid_painter.line_segment(
                    [pos2(bx, content_y), pos2(bx, content_y + grid_h)],
                    Stroke::new(0.5_f32, BW_GRID_BEAT),
                );
            }
        }
    }

    // Sombreado fuera del bucle
    if loop_s_px > content_x {
        grid_painter.rect_filled(
            Rect::from_min_max(pos2(content_x, content_y), pos2(loop_s_px.min(rect.max.x), content_y + grid_h)),
            0.0,
            Color32::from_rgba_unmultiplied(0, 0, 0, 75),
        );
    }
    if loop_e_px < rect.max.x {
        grid_painter.rect_filled(
            Rect::from_min_max(pos2(loop_e_px.max(content_x), content_y), pos2(rect.max.x, content_y + grid_h)),
            0.0,
            Color32::from_rgba_unmultiplied(0, 0, 0, 75),
        );
    }

    // --- 4. GESTIÓN E INTERACCIÓN DE NOTAS ---
    let mut clicked_note_index: Option<usize> = None;
    let mut double_clicked_note_index: Option<usize> = None;
    let mut resize_drag_detected: Option<(usize, SelectionDragHandle)> = None;

    let snap_step = prs.grid_snap_ticks.max(1);

    // Dibujar cada nota MIDI
    for (idx, note) in prs.notes.iter().enumerate() {
        let row = (MIDI_KEY_COUNT - 1 - note.pitch as u32) as f32;
        let nx = content_x + scroll_x + note.start_tick as f32 * zoom_x;
        let nw = (note.duration_ticks as f32 * zoom_x).max(4.0);
        let ny = content_y + row * key_h - scroll_y + 1.0;
        let nh = key_h - 2.0;

        let note_rect = Rect::from_min_size(pos2(nx, ny), vec2(nw, nh));

        if note_rect.max.x < content_x
            || note_rect.min.x > rect.max.x
            || note_rect.max.y < content_y
            || note_rect.min.y > content_y + grid_h
        {
            continue;
        }

        let is_selected = prs.selected_notes.contains(&idx);
        let note_color = if is_selected {
            theme::ACCENT_HOVER
        } else {
            track_color
        };

        grid_painter.rect_filled(note_rect, 2.5, note_color);

        let stroke_alpha = if is_selected { 240 } else { 90 };
        grid_painter.rect_stroke(
            note_rect,
            2.5,
            Stroke::new(
                if is_selected { 1.5_f32 } else { 1.0_f32 },
                Color32::from_rgba_unmultiplied(255, 255, 255, stroke_alpha),
            ),
            egui::StrokeKind::Inside,
        );

        // Nombre del pitch en la nota si cabe
        if nw >= 20.0 {
            grid_painter.text(
                pos2(note_rect.min.x + 3.0, note_rect.center().y),
                Align2::LEFT_CENTER,
                pitch_name(note.pitch),
                fonts::semibold(9.0),
                Color32::WHITE,
            );
        }

        // Hit-test interactivo
        if let Some(pos) = pointer_pos {
            if note_rect.contains(pos) {
                let is_left_edge = pos.x <= note_rect.min.x + 5.0;
                let is_right_edge = pos.x >= note_rect.max.x - 5.0;

                if is_left_edge || is_right_edge {
                    ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeHorizontal);
                    if response.drag_started() {
                        let handle = if is_left_edge {
                            SelectionDragHandle::Left
                        } else {
                            SelectionDragHandle::Right
                        };
                        resize_drag_detected = Some((idx, handle));
                    }
                } else {
                    ui.output_mut(|o| o.cursor_icon = CursorIcon::Grab);
                }

                if primary_clicked {
                    clicked_note_index = Some(idx);
                }
                if double_clicked {
                    double_clicked_note_index = Some(idx);
                }
            }
        }
    }

    // Iniciar Redimensionamiento de Nota
    if let Some((idx, handle)) = resize_drag_detected {
        prs.note_resize_active = true;
        prs.note_resize_handle = handle;
        prs.note_resize_index = Some(idx);
        if !prs.selected_notes.contains(&idx) {
            prs.selected_notes.clear();
            prs.selected_notes.insert(idx);
        }
        prs.note_resize_targets = prs
            .selected_notes
            .iter()
            .filter_map(|&i| {
                prs.notes
                    .get(i)
                    .map(|n| (i, n.start_tick, n.duration_ticks))
            })
            .collect();
    }

    // Procesar Clic / Doble Clic en Notas
    if let Some(idx) = double_clicked_note_index {
        // Doble clic en nota: ELIMINA LA NOTA (1:1 Bitwig)
        if idx < prs.notes.len() {
            prs.notes.remove(idx);
            prs.selected_notes.remove(&idx);
            notes_modified = true;
        }
    } else if let Some(idx) = clicked_note_index {
        if shift_held || ctrl_held {
            if prs.selected_notes.contains(&idx) {
                prs.selected_notes.remove(&idx);
            } else {
                prs.selected_notes.insert(idx);
            }
        } else if !prs.selected_notes.contains(&idx) {
            prs.selected_notes.clear();
            prs.selected_notes.insert(idx);
        }

        // Audition al seleccionar nota
        if let Some(note) = prs.notes.get(idx) {
            if prs.audition_on_click {
                audition_event = Some((note.pitch, note.velocity as f32 / 127.0));
            }
        }
    } else if double_clicked && grid_rect.contains(pointer_pos.unwrap_or_default()) {
        // Doble clic en espacio vacío: CREA UNA NUEVA NOTA (1:1 Bitwig)
        let pos = pointer_pos.unwrap_or_default();
        let clicked_tick_raw = ((pos.x - content_x - scroll_x) / zoom_x).max(0.0) as u64;
        let start_tick = quantize_tick(clicked_tick_raw, snap_step);
        let row = ((pos.y - content_y + scroll_y) / key_h).floor() as i64;
        let pitch = (i64::from(MIDI_KEY_COUNT) - 1 - row).clamp(0, 127) as u8;

        let new_note = MidiNote::new(pitch, start_tick, snap_step, 100);
        prs.notes.push(new_note);
        prs.notes.sort_by_key(|n| (n.start_tick, n.pitch));

        let new_idx = prs
            .notes
            .iter()
            .position(|n| n.start_tick == start_tick && n.pitch == pitch)
            .unwrap_or(0);
        prs.selected_notes.clear();
        prs.selected_notes.insert(new_idx);
        notes_modified = true;

        if prs.audition_on_click {
            audition_event = Some((pitch, 100.0 / 127.0));
        }
    } else if primary_clicked && grid_rect.contains(pointer_pos.unwrap_or_default()) {
        // Clic en vacío: deselecciona notas si no se sostiene Shift/Ctrl
        if !shift_held && !ctrl_held {
            prs.clear_note_selection();
        }
    }

    // ARRASTRE DE CUERPO DE NOTAS (Traslado Pitch y Tiempo)
    if response.drag_started()
        && !prs.note_resize_active
        && clicked_note_index.is_some()
    {
        prs.note_move_active = true;
        prs.note_move_grab_index = clicked_note_index;
        prs.note_move_origin_pointer = pointer_pos;
        prs.note_move_orig = prs
            .selected_notes
            .iter()
            .filter_map(|&i| prs.notes.get(i).map(|n| (i, n.start_tick, n.pitch)))
            .collect();
    }

    if prs.note_move_active && response.dragged() {
        if let (Some(orig_pos), Some(curr_pos)) = (prs.note_move_origin_pointer, pointer_pos) {
            let delta_px_x = curr_pos.x - orig_pos.x;
            let delta_px_y = curr_pos.y - orig_pos.y;

            let delta_ticks_raw = (delta_px_x / zoom_x) as i64;
            let delta_ticks = if shift_held {
                delta_ticks_raw
            } else {
                (delta_ticks_raw / snap_step as i64) * snap_step as i64
            };

            let delta_semitones = -(delta_px_y / key_h).round() as i32;

            for &(idx, orig_tick, orig_pitch) in &prs.note_move_orig {
                if let Some(n) = prs.notes.get_mut(idx) {
                    let new_tick = (orig_tick as i64 + delta_ticks).max(0) as u64;
                    let new_pitch = (orig_pitch as i32 + delta_semitones).clamp(0, 127) as u8;
                    n.start_tick = new_tick;
                    n.pitch = new_pitch;
                }
            }
            notes_modified = true;
        }
    }

    // ARRASTRE DE REDIMENSIONAMIENTO (Resize Duración)
    if prs.note_resize_active && response.dragged() {
        let delta_x = ui.input(|i| i.pointer.delta().x);
        let delta_ticks = (delta_x / zoom_x) as i64;

        for &(idx, orig_start, orig_dur) in &prs.note_resize_targets {
            if let Some(n) = prs.notes.get_mut(idx) {
                if prs.note_resize_handle == SelectionDragHandle::Right {
                    let new_dur = (orig_dur as i64 + delta_ticks).max(snap_step as i64) as u64;
                    n.duration_ticks = if shift_held {
                        new_dur
                    } else {
                        quantize_tick(new_dur, snap_step).max(snap_step)
                    };
                } else if prs.note_resize_handle == SelectionDragHandle::Left {
                    let end_tick = orig_start + orig_dur;
                    let new_start = (orig_start as i64 + delta_ticks).clamp(0, end_tick as i64 - 1) as u64;
                    n.start_tick = if shift_held {
                        new_start
                    } else {
                        quantize_tick(new_start, snap_step).min(end_tick.saturating_sub(snap_step))
                    };
                    n.duration_ticks = end_tick.saturating_sub(n.start_tick).max(1);
                }
            }
        }
        notes_modified = true;
    }

    // MARQUEE SELECTION (Selección Rectangular)
    if response.drag_started()
        && !prs.note_resize_active
        && !prs.note_move_active
        && clicked_note_index.is_none()
        && grid_rect.contains(pointer_pos.unwrap_or_default())
    {
        prs.note_marquee_active = true;
        prs.note_marquee_start = pointer_pos;
        prs.note_marquee_current = pointer_pos;
        if !shift_held {
            prs.clear_note_selection();
        }
    }

    if prs.note_marquee_active && response.dragged() {
        prs.note_marquee_current = pointer_pos;
        if let (Some(p0), Some(p1)) = (prs.note_marquee_start, prs.note_marquee_current) {
            let m_rect = Rect::from_two_pos(p0, p1);
            grid_painter.rect_stroke(
                m_rect,
                1.0,
                Stroke::new(1.0_f32, theme::ACCENT_ORANGE),
                egui::StrokeKind::Inside,
            );
            grid_painter.rect_filled(m_rect, 1.0, Color32::from_rgba_unmultiplied(255, 122, 0, 30));

            for (idx, note) in prs.notes.iter().enumerate() {
                let row = (MIDI_KEY_COUNT - 1 - note.pitch as u32) as f32;
                let nx = content_x + scroll_x + note.start_tick as f32 * zoom_x;
                let nw = (note.duration_ticks as f32 * zoom_x).max(4.0);
                let ny = content_y + row * key_h - scroll_y + 1.0;
                let note_rect = Rect::from_min_size(pos2(nx, ny), vec2(nw, key_h - 2.0));

                if m_rect.intersects(note_rect) {
                    prs.selected_notes.insert(idx);
                }
            }
        }
    }

    if pointer_released {
        if prs.note_move_active || prs.note_resize_active {
            prs.notes.sort_by_key(|n| (n.start_tick, n.pitch));
        }
        prs.note_move_active = false;
        prs.note_resize_active = false;
        prs.note_marquee_active = false;
    }

    // --- 5. CARRIL DE VELOCIDAD (Velocity Lane estilo Bitwig) ---
    if prs.show_velocity_lane && vel_h > 10.0 {
        let vel_painter = painter.with_clip_rect(vel_rect);
        vel_painter.rect_filled(vel_rect, 0.0, BW_VEL_BG);
        vel_painter.line_segment(
            [vel_rect.left_top(), vel_rect.right_top()],
            Stroke::new(1.0_f32, BW_VEL_BORDER),
        );

        // Sidebar del carril de velocidad
        let vs_painter = painter.with_clip_rect(vel_sidebar_rect);
        vs_painter.rect_filled(vel_sidebar_rect, 0.0, Color32::from_rgb(0x13, 0x14, 0x16));
        vs_painter.line_segment(
            [vel_sidebar_rect.right_top(), vel_sidebar_rect.right_bottom()],
            Stroke::new(1.0_f32, BW_GRID_MAJOR),
        );
        vs_painter.line_segment(
            [vel_sidebar_rect.left_top(), vel_sidebar_rect.right_top()],
            Stroke::new(1.0_f32, BW_VEL_BORDER),
        );
        vs_painter.text(
            pos2(vel_sidebar_rect.min.x + 8.0, vel_sidebar_rect.center().y),
            Align2::LEFT_CENTER,
            "VELOCITY",
            fonts::semibold(9.0),
            Color32::from_rgb(0x75, 0x78, 0x82),
        );

        let v_base_y = vel_rect.max.y - 4.0;
        let v_max_h = vel_h - 12.0;
        let mut dragged_vel: Option<(usize, u8)> = None;

        for (idx, note) in prs.notes.iter().enumerate() {
            let nx = content_x + scroll_x + note.start_tick as f32 * zoom_x;
            if nx < content_x || nx > rect.max.x {
                continue;
            }

            let is_sel = prs.selected_notes.contains(&idx);
            let stem_col = if is_sel {
                track_color
            } else {
                Color32::from_rgb(0x52, 0x54, 0x5C)
            };

            let norm_v = (note.velocity as f32 / 127.0).clamp(0.05, 1.0);
            let tip_y = v_base_y - norm_v * v_max_h;

            // Tallo vertical
            vel_painter.line_segment(
                [pos2(nx, v_base_y), pos2(nx, tip_y)],
                Stroke::new(1.5_f32, stem_col),
            );

            // Cabezal circular interactivo (radius 3.5px)
            let head_center = pos2(nx, tip_y);
            let head_rect = Rect::from_center_size(head_center, vec2(9.0, 9.0));

            if let Some(pos) = pointer_pos {
                if head_rect.contains(pos) {
                    ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeVertical);
                    if response.dragged() {
                        let new_vel = (((v_base_y - pos.y) / v_max_h) * 127.0).clamp(1.0, 127.0) as u8;
                        dragged_vel = Some((idx, new_vel));
                    }
                }
            }

            vel_painter.circle_filled(head_center, 3.5, if is_sel { Color32::WHITE } else { stem_col });
        }

        if let Some((idx, new_vel)) = dragged_vel {
            if let Some(n) = prs.notes.get_mut(idx) {
                n.velocity = new_vel;
                notes_modified = true;
            }
        }
    }

    // Playhead vertical (Línea de reproducción celeste)
    let playhead_px = content_x + scroll_x + prs.playhead_tick as f32 * zoom_x;
    if playhead_px >= content_x && playhead_px <= rect.max.x {
        painter.line_segment(
            [pos2(playhead_px, rect.min.y), pos2(playhead_px, rect.max.y)],
            Stroke::new(1.5_f32, theme::PLAYHEAD_COLOR),
        );
    }

    // --- 6. GUARDAR CAMBIOS AL CLIP Y SINCRONIZAR AL AUDIO ENGINE ---
    if notes_modified {
        if let Some((t_idx, s_idx)) = selected_slot_coord {
            let updated_notes: Vec<(u64, u8, u8, u32)> = prs
                .notes
                .iter()
                .map(|n| (n.start_tick, n.pitch, n.velocity, n.duration_ticks as u32))
                .collect();

            if let Some(slot) = app.matrix_state.slot_mut(t_idx, s_idx) {
                if let Some(clip) = slot.clip.as_mut() {
                    clip.content = ClipData::Midi {
                        notes: updated_notes,
                    };
                }
            }

            super::matrix::sync_midi_clip(
                &mut app.matrix_state,
                &app.audio_proxy,
                t_idx,
                s_idx,
            );
        }
    }

    // --- 7. AUDITION AUDIO (Preview de nota al final) ---
    if let Some((p, v)) = audition_event {
        if has_instrument {
            trigger_audition_note(engine_track_idx, p, v, &app.audio_proxy);
        }
    }
}
