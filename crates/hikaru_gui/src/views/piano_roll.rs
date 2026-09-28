use std::collections::{HashMap, HashSet};

use gpui_kit::component::*;
use gpui_kit::component::button::Button;
use gpui_kit::component::label::Label;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, HikaruApp};
use crate::audio_proxy::{AudioProxy, GuiCommand};
use super::open_dms::OpenDms;
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
    pub notes: Vec<MidiNote>,
    pub drum_map: HashMap<u8, String>,
    pub playhead_tick: u64,
    pub prev_playhead_tick: u64,
    triggered_notes: HashSet<(u8, u64)>,
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
    pub note_marquee_start: Option<gpui_kit::Point<gpui_kit::Pixels>>,
    pub note_marquee_current: Option<gpui_kit::Point<gpui_kit::Pixels>>,
    pub note_marquee_additive: bool,
    pub notes_source_slot: Option<(usize, usize)>,
    pub note_move_active: bool,
    pub note_move_grab_index: Option<usize>,
    pub note_move_orig: Vec<(usize, u64, u8)>,
    pub note_move_preview_delta_ticks: i64,
    pub note_move_preview_delta_rows: i32,
    pub note_move_origin_pointer: Option<gpui_kit::Point<gpui_kit::Pixels>>,
}

impl Default for PianoRollState {
    fn default() -> Self {
        Self {
            mode: PianoRollMode::Keys,
            zoom_x: 0.15,
            key_height: 16.0,
            notes: Vec::new(),
            drum_map: HashMap::new(),
            playhead_tick: 0,
            prev_playhead_tick: 0,
            triggered_notes: HashSet::new(),
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
    let pad_entries: Vec<(u8, String)> = opendms.pads.iter().map(|pad| {
        let display_name = if pad.sample_path.is_some() {
            let filename = pad.display_filename();
            format!("{}: {}", pad.name, filename)
        } else {
            pad.name.clone()
        };
        (pad.midi_note, display_name)
    }).collect();

    state.drum_map.clear();
    for (pitch, name) in pad_entries {
        state.drum_map.insert(pitch, name);
    }
}

fn find_opendms_in_track(track: &Track) -> Option<&OpenDms> {
    track.effects.iter().find_map(|slot| {
        if slot.name == "Hikaru OpenDMS" {
            slot.dms_state.as_ref()
        } else {
            None
        }
    })
}

const QUANTIZE_TICKS: u64 = 240;
const TICKS_PER_BEAT: u64 = 960;
const TICKS_PER_BAR: u64 = 3840;
const RULER_HEIGHT: f32 = 24.0;
const NOTE_INSERT_VELOCITY: u8 = 100;
const NOTE_EDGE_HIT_W: f32 = 8.0;
const MIN_NOTE_DURATION_TICKS: u64 = QUANTIZE_TICKS;

fn quantize_tick(tick: u64, step: u64) -> u64 {
    if step == 0 {
        return tick;
    }
    (tick / step) * step
}

fn resize_note_left(orig_start: u64, orig_duration: u64, pointer_tick: u64) -> (u64, u64) {
    let end = orig_start.saturating_add(orig_duration);
    let min_dur = MIN_NOTE_DURATION_TICKS;
    if end <= min_dur {
        return (0, end.max(min_dur));
    }
    let new_start = quantize_tick(pointer_tick, QUANTIZE_TICKS).min(end - min_dur);
    let new_duration = end - new_start;
    (new_start, new_duration)
}

fn resize_note_right(orig_start: u64, _orig_duration: u64, pointer_tick: u64) -> (u64, u64) {
    let min_dur = MIN_NOTE_DURATION_TICKS;
    let new_end = quantize_tick(pointer_tick, QUANTIZE_TICKS).max(orig_start + min_dur);
    let new_duration = new_end - orig_start;
    (orig_start, new_duration)
}

fn apply_resize_left_delta(orig_start: u64, orig_duration: u64, delta_start: i64) -> (u64, u64) {
    let end = orig_start.saturating_add(orig_duration);
    let min_end = orig_start.saturating_add(MIN_NOTE_DURATION_TICKS);
    let new_end = end.max(min_end);
    let raw_start = (orig_start as i64).saturating_add(delta_start);
    let new_start = (raw_start.max(0) as u64).min(new_end.saturating_sub(MIN_NOTE_DURATION_TICKS));
    (new_start, new_end.saturating_sub(new_start))
}

fn apply_resize_right_delta(orig_start: u64, orig_duration: u64, delta_duration: i64) -> (u64, u64) {
    let raw_dur = (orig_duration as i64).saturating_add(delta_duration);
    let new_dur = (raw_dur.max(MIN_NOTE_DURATION_TICKS as i64) as u64)
        .max(MIN_NOTE_DURATION_TICKS);
    (orig_start, new_dur)
}

fn compute_resize_left_delta(
    targets: &[(usize, u64, u64)],
    grab_index: usize,
    pointer_tick: u64,
) -> i64 {
    let (_, orig_start, orig_duration) = match targets.iter().find(|(i, _, _)| *i == grab_index) {
        Some(t) => *t,
        None => return 0,
    };
    let (new_start, _) = resize_note_left(orig_start, orig_duration, pointer_tick);
    (new_start as i64) - (orig_start as i64)
}

fn compute_resize_right_delta(
    targets: &[(usize, u64, u64)],
    grab_index: usize,
    pointer_tick: u64,
) -> i64 {
    let (_, orig_start, orig_duration) = match targets.iter().find(|(i, _, _)| *i == grab_index) {
        Some(t) => *t,
        None => return 0,
    };
    let (_, new_dur) = resize_note_right(orig_start, orig_duration, pointer_tick);
    (new_dur as i64) - (orig_duration as i64)
}

fn build_resize_targets(
    notes: &[MidiNote],
    grab_index: usize,
    selected: &HashSet<usize>,
) -> Vec<(usize, u64, u64)> {
    let mut idxs: Vec<usize> = if selected.contains(&grab_index) && selected.len() > 1 {
        selected.iter().copied().filter(|&i| i < notes.len()).collect()
    } else {
        vec![grab_index]
    };
    if !idxs.contains(&grab_index) && grab_index < notes.len() {
        idxs.push(grab_index);
    }
    idxs.sort_unstable();
    idxs.dedup();
    idxs.into_iter()
        .filter_map(|i| notes.get(i).map(|n| (i, n.start_tick, n.duration_ticks)))
        .collect()
}

fn apply_move_delta(start_tick: u64, pitch: u8, delta_ticks: i64, delta_rows: i32) -> (u64, u8) {
    let new_start = if delta_ticks >= 0 {
        start_tick.saturating_add(delta_ticks as u64)
    } else {
        start_tick.saturating_sub(delta_ticks.unsigned_abs())
    };
    let new_pitch = (i32::from(pitch) - delta_rows).clamp(0, 127) as u8;
    (new_start, new_pitch)
}

fn compute_note_move_deltas(
    orig: &[(usize, u64, u8)],
    grab_index: usize,
    raw_delta_ticks: i64,
    raw_delta_rows: i32,
) -> (i64, i32) {
    let grab_start = match orig.iter().find(|(i, _, _)| *i == grab_index) {
        Some((_, start, _)) => *start,
        None => return (0, 0),
    };

    let target = (grab_start as i64)
        .saturating_add(raw_delta_ticks)
        .max(0);
    let snapped = quantize_tick(target as u64, QUANTIZE_TICKS) as i64;
    let mut delta_ticks = snapped - grab_start as i64;

    if let Some(min_start) = orig.iter().map(|(_, s, _)| *s).min() {
        if delta_ticks < 0 {
            delta_ticks = delta_ticks.max(-(min_start as i64));
        }
    }

    let mut delta_rows = raw_delta_rows;
    if let (Some(min_pitch), Some(max_pitch)) = (
        orig.iter().map(|(_, _, p)| i32::from(*p)).min(),
        orig.iter().map(|(_, _, p)| i32::from(*p)).max(),
    ) {
        delta_rows = delta_rows.clamp(max_pitch - 127, min_pitch);
    }

    (delta_ticks, delta_rows)
}

fn select_note_set(selected: &mut HashSet<usize>, index: usize, additive: bool) {
    if additive {
        selected.insert(index);
    } else if !selected.contains(&index) {
        selected.clear();
        selected.insert(index);
    }
}

fn hit_test_note_body(
    notes: &[MidiNote],
    pos: Point<Pixels>,
    grid_rect: Bounds<Pixels>,
    zoom_x: f32,
    key_height: f32,
) -> Option<usize> {
    let inset = NOTE_EDGE_HIT_W * 0.5;
    for (i, note) in notes.iter().enumerate().rev() {
        let rect = note_rect(grid_rect, note, zoom_x, key_height);
        if !rect.intersects(&grid_rect) || !rect.contains(&pos) {
            continue;
        }
        if rect.size.width.as_f32() <= NOTE_EDGE_HIT_W {
            return None;
        }
        let body = Bounds::new(
            point(rect.origin.x + px(inset), rect.origin.y),
            size(rect.size.width - px(inset * 2.0), rect.size.height),
        );
        if body.contains(&pos) {
            return Some(i);
        }
    }
    None
}

fn hit_test_note_full(
    notes: &[MidiNote],
    pos: Point<Pixels>,
    grid_rect: Bounds<Pixels>,
    zoom_x: f32,
    key_height: f32,
) -> Option<usize> {
    for (i, note) in notes.iter().enumerate().rev() {
        let rect = note_rect(grid_rect, note, zoom_x, key_height);
        if rect.intersects(&grid_rect) && rect.contains(&pos) {
            return Some(i);
        }
    }
    None
}

fn notes_in_marquee(
    notes: &[MidiNote],
    marquee: Bounds<Pixels>,
    grid_rect: Bounds<Pixels>,
    zoom_x: f32,
    key_height: f32,
) -> HashSet<usize> {
    let mut out = HashSet::new();
    for (i, note) in notes.iter().enumerate() {
        let rect = note_rect(grid_rect, note, zoom_x, key_height);
        if rect.intersects(&marquee) {
            out.insert(i);
        }
    }
    out
}

fn remove_selected_notes(notes: &mut Vec<MidiNote>, selected: &mut HashSet<usize>) -> usize {
    let sel = std::mem::take(selected);
    let before = notes.len();
    *notes = std::mem::take(notes)
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !sel.contains(i))
        .map(|(_, n)| n)
        .collect();
    before - notes.len()
}

fn duplicate_selected_notes(
    notes: &mut Vec<MidiNote>,
    selected: &HashSet<usize>,
) -> Vec<usize> {
    if selected.is_empty() {
        return Vec::new();
    }

    let mut sorted: Vec<usize> = selected.iter().copied().filter(|&i| i < notes.len()).collect();
    sorted.sort_unstable();

    let mut selected_notes: Vec<MidiNote> = Vec::with_capacity(sorted.len());
    for i in sorted {
        selected_notes.push(notes[i].clone());
    }
    if selected_notes.is_empty() {
        return Vec::new();
    }

    let min_start = selected_notes.iter().map(|n| n.start_tick).min().unwrap_or(0);
    let max_end = selected_notes
        .iter()
        .map(|n| n.start_tick + n.duration_ticks)
        .max()
        .unwrap_or(0);
    let duration_block = max_end.saturating_sub(min_start);

    let base = notes.len();
    let mut new_indices = Vec::with_capacity(selected_notes.len());
    for note in selected_notes {
        let mut clone = note.clone();
        clone.start_tick = note.start_tick.saturating_add(duration_block);
        notes.push(clone);
        new_indices.push(notes.len() - 1);
    }
    debug_assert_eq!(base + new_indices.len(), notes.len());
    new_indices
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
            if note_just_crossed(prev_tick, current_tick, note.start_tick) {
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

fn note_just_crossed(prev_tick: u64, current_tick: u64, start_tick: u64) -> bool {
    (prev_tick < start_tick || prev_tick > current_tick)
        && current_tick >= start_tick
        && current_tick < start_tick + 240
}

fn note_rect(grid_rect: Bounds<Pixels>, note: &MidiNote, zoom_x: f32, key_height: f32) -> Bounds<Pixels> {
    let x = grid_rect.origin.x + px(note.start_tick as f32 * zoom_x);
    let y = grid_rect.origin.y + px((127 - note.pitch) as f32 * key_height);
    Bounds::new(
        point(x, y),
        size(
            px((note.duration_ticks as f32 * zoom_x).max(4.0)),
            px(key_height - 1.0),
        ),
    )
}

fn preview_drum_pad(
    tracks: &[Track],
    selected_track_index: usize,
    pitch: u8,
    velocity_scale: f32,
    audio_proxy: &AudioProxy,
) {
    if let Some(track) = tracks.get(selected_track_index) {
        if let Some(opendms) = find_opendms_in_track(track) {
            if let Some(pad) = opendms.pads.iter().find(|p| p.midi_note == pitch) {
                if pad.sample_path.is_some() {
                    let adsr = &opendms.sampler.adsr;
                    let speed = 2.0_f32.powf(pad.pitch / 12.0);
                    audio_proxy.send(GuiCommand::DmsNoteOn {
                        pad_idx: pad.id,
                        gain: pad.volume * velocity_scale,
                        pan: pad.pan,
                        velocity: velocity_scale,
                        play_speed: speed as f64,
                        attack_ms: adsr.attack,
                        decay_ms: adsr.decay,
                        sustain: adsr.sustain,
                        release_ms: adsr.release,
                    });
                }
            }
        }
    }
}

pub fn render(cx: &mut Context<HikaruApp>) -> AnyElement {
    let app = state(cx).read(cx);
    let prs = &app.piano_roll_state;
    let mode = prs.mode;
    let zoom_x = prs.zoom_x;
    let key_height = prs.key_height;
    let notes_count = prs.notes.len();
    let playhead_tick = prs.playhead_tick;
    let sel_active = prs.selection_active;
    let sel_start = prs.selection_start_tick;
    let sel_end = prs.selection_end_tick;
    let loop_enabled = prs.loop_enabled;
    let sel_notes = prs.selected_notes.len();
    let selected_track = app.selected_track_index;
    let is_playing = app.transport.playback_state == hikaru_transport::transport_state::TransportPlaybackState::Playing;
    let tracks = match app.mode {
        crate::app::AppMode::OpenLive => &app.live_tracks,
        crate::app::AppMode::OpenStudio => &app.studio_tracks,
    };
    let has_opendms = tracks.get(selected_track).map(|t| t.effects.iter().any(|s| s.name == "Hikaru OpenDMS")).unwrap_or(false);
    let audio_proxy = app.audio_proxy.clone();
    drop(app);

    let sidebar_width = 140.0;
    let max_note_tick = prs
        .notes
        .iter()
        .map(|n| n.start_tick + n.duration_ticks)
        .max()
        .unwrap_or(0);
    let total_ticks = (TICKS_PER_BAR * 128).max(max_note_tick.max(playhead_tick) + TICKS_PER_BAR * 16);
    let grid_h = 128.0 * key_height;
    let canvas_w = sidebar_width + total_ticks as f32 * zoom_x;

    let mut note_rects: Vec<AnyElement> = Vec::new();
    for (i, note) in prs.notes.iter().enumerate() {
        let (st, dur, pitch) = {
            let n = &prs.notes[i];
            let mut s = n.start_tick;
            let mut d = n.duration_ticks;
            let mut p = n.pitch;
            if prs.note_move_active {
                if let Some(&(_, os, op)) = prs.note_move_orig.iter().find(|(j, _, _)| *j == i) {
                    let (ns, np) = apply_move_delta(os, op, prs.note_move_preview_delta_ticks, prs.note_move_preview_delta_rows);
                    s = ns;
                    p = np;
                }
            }
            if prs.note_resize_active {
                if let Some(&(_, os, od)) = prs.note_resize_targets.iter().find(|(j, _, _)| *j == i) {
                    match prs.note_resize_handle {
                        SelectionDragHandle::Left => {
                            let (ns, nd) = apply_resize_left_delta(os, od, prs.note_resize_delta_start);
                            s = ns;
                            d = nd;
                        }
                        SelectionDragHandle::Right => {
                            let (_, nd) = apply_resize_right_delta(os, od, prs.note_resize_delta_duration);
                            d = nd;
                        }
                        SelectionDragHandle::None => {}
                    }
                }
            }
            (s, d, p)
        };
        let is_sel = prs.selected_notes.contains(&i);
        let nx = sidebar_width + st as f32 * zoom_x;
        let ny = (127 - pitch) as f32 * key_height;
        let nw = (dur as f32 * zoom_x).max(4.0);
        let body_color = if prs.note_resize_active
            && prs.note_resize_targets.iter().any(|(j, _, _)| *j == i)
        {
            rgb(0xFFBE3C)
        } else if is_sel {
            rgb(0xFFD23C)
        } else {
            rgb(0xFF8C00)
        };
        note_rects.push(
            div()
                .absolute()
                .left(px(nx))
                .top(px(ny + RULER_HEIGHT))
                .w(px(nw))
                .h(px(key_height - 1.0))
                .bg(body_color)
                .border_1()
                .border_color(if is_sel { rgb(0xFFFFA0) } else { rgb(0xFFFFFF) })
                .id(format!("pr_note_{}", i))
                .on_click(move |event, _, cx| {
                    let shift = event.modifiers().shift;
                    let st2 = state(cx);
                    st2.update(cx, |state, cx| {
                        let prs = &mut state.piano_roll_state;
                        let grab = i;
                        select_note_set(&mut prs.selected_notes, grab, shift);
                        cx.notify();
                    });
                })
                .into_any_element(),
        );
    }

    let mut grid_bg: Vec<AnyElement> = Vec::new();
    for row in 0..128 {
        let pitch = 127 - row;
        let y = row as f32 * key_height;
        let is_black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
        if is_black {
            grid_bg.push(
                div()
                    .absolute()
                    .left(px(0.0))
                    .top(px(RULER_HEIGHT + y))
                    .w(px(canvas_w))
                    .h(px(key_height))
                    .bg(rgba(0x00000026))
                    .into_any_element(),
            );
        }
        grid_bg.push(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let yy = bounds.origin.y + px(y);
                    let mut path = PathBuilder::stroke(px(0.5));
                    path.move_to(point(bounds.origin.x, yy));
                    path.line_to(point(bounds.origin.x + bounds.size.width, yy));
                    window.paint_path(path.build().unwrap(), rgb(0x1E1E1E));
                },
            )
            .absolute()
            .left(px(0.0))
            .top(px(RULER_HEIGHT))
            .w(px(canvas_w))
            .h(px(grid_h))
            .into_any_element(),
        );
    }

    let mut sidebar_rows: Vec<AnyElement> = Vec::new();
    for row in 0..128 {
        let pitch = 127 - row;
        let y = row as f32 * key_height;
        let is_black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
        let (bg, tc) = match mode {
            PianoRollMode::Keys => {
                if is_black {
                    (rgb(0x28282D), rgb(0xFFFFFF))
                } else {
                    (rgb(0xDCDCE1), rgb(0x000000))
                }
            }
            PianoRollMode::Drums => (rgb(0x232328), rgb(0xC8C8C8)),
        };
        let label = match mode {
            PianoRollMode::Keys => {
                let names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
                let octave = (pitch / 12) as i32 - 1;
                format!("{}{}", names[(pitch % 12) as usize], octave)
            }
            PianoRollMode::Drums => {
                if let Some(name) = prs.drum_map.get(&pitch) {
                    format!("{} ({})", name, pitch)
                } else {
                    format!("Pad {}", pitch)
                }
            }
        };
        sidebar_rows.push(
            div()
                .absolute()
                .left(px(0.0))
                .top(px(RULER_HEIGHT + y))
                .w(px(sidebar_width))
                .h(px(key_height))
                .bg(bg)
                .border_r_1()
                .border_color(rgb(0x323232))
                .child(
                    Label::new(label)
                        .text_xs()
                        .text_color(tc)
                        .pl(px(6.0)),
                )
                .id(format!("pr_key_{}", pitch))
                .on_click(move |_, _, cx| {
                    let st2 = state(cx);
                    st2.update(cx, |state, _| {
                        preview_drum_pad(
                            &state.live_tracks,
                            state.selected_track_index,
                            pitch,
                            1.0,
                            &state.audio_proxy,
                        );
                    });
                })
                .into_any_element(),
        );
    }

    let sel_canvas = canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            if sel_active && sel_end > sel_start {
                let sx0 = bounds.origin.x + px(sel_start as f32 * zoom_x);
                let sx1 = bounds.origin.x + px(sel_end as f32 * zoom_x);
                let mx0 = sx0.min(sx1);
                let mx1 = sx0.max(sx1);
                let sel_rect = Bounds::new(
                    point(mx0, bounds.origin.y),
                    size(mx1 - mx0, bounds.size.height),
                );
                window.paint_quad(PaintQuad {
                    bounds: sel_rect,
                    background: rgba(0x64C8FF26).into(),
                    border_color: rgba(0x64C8FF00).into(),
                    corner_radii: gpui_kit::Corners::default(),
                    border_widths: gpui_kit::Edges::default(),
                    border_style: BorderStyle::default(),
                });
                let bracket = rgb(0x00C8FF);
                for bx in [mx0, mx1] {
                    let mut path = PathBuilder::stroke(px(2.0));
                    path.move_to(point(bx, bounds.origin.y));
                    path.line_to(point(bx, bounds.origin.y + bounds.size.height));
                    window.paint_path(path.build().unwrap(), bracket);
                }
            }
        },
    )
    .absolute()
    .left(px(sidebar_width))
    .top(px(RULER_HEIGHT))
    .w(px(canvas_w - sidebar_width))
    .h(px(grid_h));

    let playhead_canvas = canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let phx = bounds.origin.x + px(playhead_tick as f32 * zoom_x);
            let mut path = PathBuilder::stroke(px(2.0));
            path.move_to(point(phx, bounds.origin.y));
            path.line_to(point(phx, bounds.origin.y + bounds.size.height));
            window.paint_path(path.build().unwrap(), rgb(0x00C8FF));
        },
    )
    .absolute()
    .left(px(sidebar_width))
    .top(px(RULER_HEIGHT))
    .w(px(canvas_w - sidebar_width))
    .h(px(grid_h));

    let grid_interaction = div()
        .absolute()
        .left(px(sidebar_width))
        .top(px(RULER_HEIGHT))
        .w(px(canvas_w - sidebar_width))
        .h(px(grid_h))
        .id("pr_grid_interaction")
        .on_click(move |event, _, cx| {
            let Some(pos) = event.mouse_position() else {
                return;
            };
            let st2 = state(cx);
            st2.update(cx, |state, cx| {
                let prs = &mut state.piano_roll_state;
                let local_x = pos.x.as_f32() - sidebar_width;
                let local_y = pos.y.as_f32() - RULER_HEIGHT;
                let raw_tick = (local_x / zoom_x).max(0.0) as u64;
                let q_tick = quantize_tick(raw_tick, QUANTIZE_TICKS);
                let row = (local_y / key_height).max(0.0) as i32;
                let pitch = (127 - row).clamp(0, 127) as u8;
                let already = prs.notes.iter().any(|n| {
                    n.pitch == pitch
                        && q_tick >= n.start_tick
                        && q_tick < n.start_tick + n.duration_ticks
                });
                if !already {
                    prs.notes.push(MidiNote {
                        pitch,
                        start_tick: q_tick,
                        duration_ticks: QUANTIZE_TICKS,
                        velocity: NOTE_INSERT_VELOCITY,
                    });
                    if prs.mode == PianoRollMode::Drums {
                        preview_drum_pad(
                            &state.live_tracks,
                            state.selected_track_index,
                            pitch,
                            1.0,
                            &state.audio_proxy,
                        );
                    }
                }
                cx.notify();
            });
        });

    let bar = (playhead_tick / TICKS_PER_BAR) + 1;
    let beat = ((playhead_tick % TICKS_PER_BAR) / TICKS_PER_BEAT) + 1;

    v_flex()
        .id("piano_roll")
        .size_full()
        .child(
            h_flex()
                .items_center()
                .gap(px(6.0))
                .h(px(28.0))
                .bg(rgb(0x181A20))
                .px(px(8.0))
                .child(
                    Button::new("pr_mode_keys")
                        .label("🎹 Keys")
                        .compact()
                        .when(mode == PianoRollMode::Keys, |b| b.text_color(rgb(0x00B4D8)))
                        .on_click(move |_, _, cx| {
                            let st2 = state(cx);
                            st2.update(cx, |state, cx| {
                                state.piano_roll_state.mode = PianoRollMode::Keys;
                                cx.notify();
                            });
                        }),
                )
                .child(
                    Button::new("pr_mode_drums")
                        .label("🥁 Drums")
                        .compact()
                        .when(mode == PianoRollMode::Drums, |b| b.text_color(rgb(0x00B4D8)))
                        .on_click(move |_, _, cx| {
                            let st2 = state(cx);
                            st2.update(cx, |state, cx| {
                                state.piano_roll_state.mode = PianoRollMode::Drums;
                                cx.notify();
                            });
                        }),
                )
                .child(Label::new(format!("Zoom {:.2}", zoom_x)).text_xs())
                .child(Label::new(format!("Compás: {}.{} | Tick: {}", bar, beat, playhead_tick)).text_xs())
                .when(sel_active, |this| {
                    this.child(Label::new(format!("Sel: {} -> {}", sel_start, sel_end)).text_xs())
                        .child(
                            Button::new("pr_loop_toggle")
                                .label(if loop_enabled { "Loop ON" } else { "Loop OFF" })
                                .compact()
                                .when(loop_enabled, |b| b.text_color(rgb(0x39FF14)))
                                .on_click(move |_, _, cx| {
                                    let st2 = state(cx);
                                    st2.update(cx, |state, cx| {
                                        state.piano_roll_state.loop_enabled =
                                            !state.piano_roll_state.loop_enabled;
                                        cx.notify();
                                    });
                                }),
                        )
                        .child(
                            Button::new("pr_clear_sel")
                                .label("X Sel")
                                .compact()
                                .on_click(move |_, _, cx| {
                                    let st2 = state(cx);
                                    st2.update(cx, |state, cx| {
                                        state.piano_roll_state.clear_selection();
                                        state.piano_roll_state.loop_enabled = false;
                                        cx.notify();
                                    });
                                }),
                        )
                })
                .when(sel_notes > 0, |this| {
                    this.child(Label::new(format!("Notas: {}", sel_notes)).text_xs())
                        .child(
                            Button::new("pr_clear_notes")
                                .label("X Notas")
                                .compact()
                                .on_click(move |_, _, cx| {
                                    let st2 = state(cx);
                                    st2.update(cx, |state, cx| {
                                        state.piano_roll_state.clear_note_selection();
                                        cx.notify();
                                    });
                                }),
                        )
                }),
        )
        .child(
            div()
                .flex_1()
                .overflow_x_scrollbar()
                .child(
                    div()
                        .w(px(canvas_w))
                        .h(px(RULER_HEIGHT + grid_h))
                        .relative()
                        .child(
                            div()
                                .absolute()
                                .left(px(0.0))
                                .top(px(0.0))
                                .w(px(sidebar_width))
                                .h(px(RULER_HEIGHT))
                                .bg(rgb(0x141418))
                                .into_any_element(),
                        )
                        .child(
                            canvas(
                                |_, _, _| {},
                                move |bounds, _, window, _| {
                                    let rx = bounds.origin.x + px(sidebar_width);
                                    let rw = bounds.size.width - px(sidebar_width);
                                    let mut tick = 0u64;
                                    let mut bar_number = 1u32;
                                    while tick <= total_ticks {
                                        let x = rx + px(tick as f32 * zoom_x);
                                        if x > bounds.origin.x + bounds.size.width {
                                            break;
                                        }
                                        if x >= rx {
                                            let mut tick_path = PathBuilder::stroke(px(1.5));
                                            tick_path.move_to(point(x, bounds.origin.y + px(4.0)));
                                            tick_path.line_to(point(
                                                x,
                                                bounds.origin.y + bounds.size.height,
                                            ));
                                            window.paint_path(tick_path.build().unwrap(), rgb(0x8C8C8C));
                                            let mut bar_path = PathBuilder::fill();
                                            bar_path.move_to(point(x + px(5.0), bounds.origin.y + px(3.0)));
                                            bar_path.line_to(point(x + px(13.0), bounds.origin.y + px(3.0)));
                                            bar_path.line_to(point(x + px(13.0), bounds.origin.y + px(13.0)));
                                            bar_path.line_to(point(x + px(5.0), bounds.origin.y + px(13.0)));
                                            bar_path.close();
                                            window.paint_path(bar_path.build().unwrap(), rgb(0xD2D2D2));
                                            bar_number += 1;
                                        }
                                        tick += TICKS_PER_BAR;
                                    }
                                },
                            )
                            .absolute()
                            .left(px(0.0))
                            .top(px(0.0))
                            .w(px(canvas_w))
                            .h(px(RULER_HEIGHT))
                            .into_any_element(),
                        )
                        .children(sidebar_rows)
                        .children(grid_bg)
                        .child(sel_canvas.into_any_element())
                        .child(playhead_canvas.into_any_element())
                        .children(note_rects)
                        .child(grid_interaction.into_any_element()),
                ),
        )
        .into_any_element()
}
