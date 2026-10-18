use std::collections::{HashMap, HashSet};

use gpui_kit::component::*;
use gpui_kit::component::button::Button;
use gpui_kit::component::label::Label;
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
    /// Scroll del área de grilla (ambos ejes). Vive acá y no en la vista
    /// porque los handlers de click necesitan restar el offset para traducir
    /// coordenadas de ventana a contenido, y `ScrollHandle` no depende de GPUI.
    pub grid_scroll: gpui_kit::ScrollHandle,
    /// `true` una vez que se aplicó el layout inicial (zoom + foco en C4).
    /// Se hace UNA vez: si no, cada frame recolocaría el scroll y el usuario
    /// no podría scrollear.
    pub layout_initialized: bool,
    /// Frames restantes esperando el primer prepaint (ver `LAYOUT_WAIT_FRAMES`).
    pub layout_wait_frames: u8,
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
            zoom_x: DEFAULT_ZOOM_X,
            key_height: DEFAULT_KEY_HEIGHT,
            grid_scroll: gpui_kit::ScrollHandle::default(),
            layout_initialized: false,
            layout_wait_frames: 0,
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
pub const TICKS_PER_BEAT: u64 = 960;
pub const TICKS_PER_BAR: u64 = 3840;
pub const RULER_HEIGHT: f32 = 24.0;
const NOTE_INSERT_VELOCITY: u8 = 100;
const NOTE_EDGE_HIT_W: f32 = 8.0;
const MIN_NOTE_DURATION_TICKS: u64 = QUANTIZE_TICKS;

// =========================================================================
// GEOMETRÍA DEL PANEL (virtualización + layout inicial)
// =========================================================================
//
// La grilla es de 128 filas MIDI de alto fijo, así que SIEMPRE desborda el
// panel: sin scroll vertical el usuario queda clavado en la fila 0 (G9) y ve
// un área vacía. Y el zoom por defecto tenía que hacer que un compás entre en
// el panel; con el valor anterior (0.15) un compás medía 576 px, o sea que
// sólo se veía un sliver de compás y el contenido crecía a ~74.000 px de ancho.

/// Filas de la grilla (una por nota MIDI, 0 = pitch 127 = G9).
pub const MIDI_KEY_COUNT: u32 = 128;
/// Columna de nombres de tecla, en px de contenido.
pub const SIDEBAR_WIDTH: f32 = 140.0;
/// Alto por defecto de una fila de tecla, en px.
pub const DEFAULT_KEY_HEIGHT: f32 = 16.0;
/// Zoom por defecto: 4 compases entran en el ancho útil del panel.
pub const DEFAULT_ZOOM_X: f32 = 0.06;
pub const MIN_ZOOM_X: f32 = 0.008;
pub const MAX_ZOOM_X: f32 = 0.5;
/// Compases que siempre se pueden scrollear aunque no haya notas.
pub const DEFAULT_BARS: u64 = 32;
/// Pitch al que apunta la vista al abrir el panel (C4, el centro del registro
/// useful). Antes el scroll arrancaba en 0 => G9.
pub const FOCUS_PITCH: u8 = 60;
/// Compases que el zoom inicial intenta que entren en el panel.
pub const BARS_FIT_TO_PANEL: f32 = 4.0;
/// Espacio reservado a la barra de scroll vertical al calculaar el zoom.
const SCROLLBAR_ALLOWANCE: f32 = 16.0;
/// Filas extra por arriba y por abajo del viewport, para que un scroll rápido
/// no muestre un borde vacío.
const ROW_OVERSCAN: u32 = 4;
/// Margen extra en ticks a ambos lados (un compás reach).
const TICK_OVERSCAN: u64 = TICKS_PER_BAR;
/// Viewport de Backup: antes del primer prepaint el handle todavía no conoce
/// el tamaño real. Es un piso, no un techo, así que la grilla siempre dibuja
/// algo acotado en vez de las 128 filas.
const MIN_VIEWPORT_W: f32 = 480.0;
const MIN_VIEWPORT_H: f32 = 180.0;
/// Frames que se piden para esperar el primer prepaint que publique el tamaño
/// real del viewport. Acotado: si el panel nunca se layoutea (0 px), el piano
/// roll no puede pedir frames para siempre.
const LAYOUT_WAIT_FRAMES: u8 = 8;

const PITCH_CLASS_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// Nombres de las 128 teclas ("C-1".."G9"), indexados por fila.
///
/// Antes se armaban con `format!` las 128 etiquetas en CADA frame. La tabla se
/// calcula una única vez: en el loop de render sólo hay un clon de
/// `SharedString` (que es un `SmolStr`, o sea un refcount).
fn pitch_name_table() -> &'static [gpui_kit::SharedString; MIDI_KEY_COUNT as usize] {
    static TABLE: std::sync::OnceLock<[gpui_kit::SharedString; MIDI_KEY_COUNT as usize]> =
        std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        std::array::from_fn(|row| {
            let pitch = (MIDI_KEY_COUNT as usize - 1 - row) as u8;
            gpui_kit::SharedString::from(format!(
                "{}{}",
                PITCH_CLASS_NAMES[(pitch % 12) as usize],
                (pitch / 12) as i32 - 1
            ))
        })
    })
}

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

/// Rango del contenido que realmente se ve, en coordenadas de contenido.
///
/// Es la clave de la virtualización: todo lo que se dibuja (teclas del sidebar,
/// líneas de la grilla, notas, regla, playhead) se recorta contra este rango.
/// Antes se iteraban las 128 filas y todas las notas en cada frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridViewport {
    /// Offset de scroll en px de contenido, normalizado a positivo (GPUI lo
    /// devuelve negativo cuando se scrollea hacia abajo/hacia la derecha).
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub width: f32,
    pub height: f32,
    /// Filas MIDI visibles, `[row_start, row_end)`. Fila 0 = pitch 127 = G9.
    pub row_start: u32,
    pub row_end: u32,
    /// Ticks visibles, `[tick_start, tick_end]`.
    pub tick_start: u64,
    pub tick_end: u64,
}

impl GridViewport {
    /// Alto en px que ocupa el rango de filas visible.
    pub fn visible_rows_height(&self, key_height: f32) -> f32 {
        (self.row_end.saturating_sub(self.row_start)) as f32 * key_height
    }
}

/// Calcula el viewport visible de la grilla a partir del offset de scroll y del
/// tamaño del área.
///
/// `viewport` es el tamaño que publica `ScrollHandle::bounds()`, válido recién
/// después del prepaint del frame anterior; hasta entonces cae al piso
/// `MIN_VIEWPORT_*`, para que el primer frame sea acotado en vez de pintar las
/// 128 filas.
pub fn compute_grid_viewport(
    offset: Point<Pixels>,
    viewport: gpui_kit::Size<Pixels>,
    key_height: f32,
    zoom_x: f32,
) -> GridViewport {
    let kh = if key_height > 0.5 { key_height } else { DEFAULT_KEY_HEIGHT };
    let zx = if zoom_x > f32::EPSILON { zoom_x } else { MIN_ZOOM_X };
    let width = viewport.width.as_f32().max(MIN_VIEWPORT_W);
    let height = viewport.height.as_f32().max(MIN_VIEWPORT_H);
    let scroll_x = (-offset.x.as_f32()).max(0.0);
    let scroll_y = (-offset.y.as_f32()).max(0.0);

    // Las filas arrancan RULER_HEIGHT más abajo del contenido.
    let rows_top = scroll_y - RULER_HEIGHT;
    let rows_bottom = scroll_y + height - RULER_HEIGHT;
    let row_start = ((rows_top / kh).floor() as i64 - i64::from(ROW_OVERSCAN))
        .clamp(0, i64::from(MIDI_KEY_COUNT) - 1) as u32;
    let row_end = (((rows_bottom / kh).ceil() as i64 + i64::from(ROW_OVERSCAN))
        .clamp(0, i64::from(MIDI_KEY_COUNT)) as u32)
        .max(row_start + 1)
        .min(MIDI_KEY_COUNT);

    // El área de notas arranca SIDEBAR_WIDTH a la derecha del contenido.
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

/// ¿La nota toca el viewport? Comparación entera: no construye `Bounds`, así que
/// recorrer todas las notas cuesta nanosegundos por nota en vez de un `div()`.
pub fn note_visible_in(start_tick: u64, duration_ticks: u64, pitch: u8, vp: &GridViewport) -> bool {
    let row = i64::from(MIDI_KEY_COUNT) - 1 - i64::from(pitch);
    if row < i64::from(vp.row_start) || row >= i64::from(vp.row_end) {
        return false;
    }
    // Las notas tienen ancho mínimo de 4 px, así que una nota que empieza justo
    // afuera del viewport puede igual asomar: por eso `tick_start` ya trae un
    // compás de overscan.
    let end = start_tick.saturating_add(duration_ticks);
    end >= vp.tick_start && start_tick <= vp.tick_end
}

/// Zoom que hace entrar `bars` compases en el ancho útil del panel.
pub fn fit_zoom_x(viewport_width: f32, bars: f32) -> f32 {
    let usable = (viewport_width - SIDEBAR_WIDTH - SCROLLBAR_ALLOWANCE).max(120.0);
    (usable / (bars.max(1.0) * TICKS_PER_BAR as f32)).clamp(MIN_ZOOM_X, MAX_ZOOM_X)
}

/// Y del contenido que debería quedar en el borde superior del viewport para
/// dejar `pitch` en el medio del panel.
pub fn focus_scroll_y(viewport_height: f32, key_height: f32, pitch: u8) -> f32 {
    let kh = if key_height > 0.5 { key_height } else { DEFAULT_KEY_HEIGHT };
    let row = MIDI_KEY_COUNT as f32 - 1.0 - f32::from(pitch);
    let center = RULER_HEIGHT + (row + 0.5) * kh;
    (center - viewport_height * 0.5).max(0.0)
}

/// Ancho del contenido, en ticks.
///
/// Antes el piso eran `TICKS_PER_BAR * 128` compases, lo que al zoom anterior
/// daba un canvas de ~74.000 px de ancho. Ahora el piso son `DEFAULT_BARS`
/// compases y el contenido crece sólo hasta donde llegan las notas.
pub fn total_ticks_for(max_note_tick: u64, playhead_tick: u64) -> u64 {
    (TICKS_PER_BAR * DEFAULT_BARS)
        .max(max_note_tick.max(playhead_tick).saturating_add(TICKS_PER_BAR * 2))
}

/// ¿Es una tecla negra? (C#, D#, F#, G#, A#)
fn is_black_key(pitch: u8) -> bool {
    matches!(pitch % 12, 1 | 3 | 6 | 8 | 10)
}

/// Tablas de arrastre indexadas por índice de nota.
///
/// Antes cada nota hacía un `iter().find()` sobre `note_move_orig` y otro sobre
/// `note_resize_targets`: dos O(n·m) por frame mientras se arrastraba. Ahora se
/// indexa una vez por frame, y sólo si el arrastre está activo.
fn drag_lookup_maps(
    prs: &PianoRollState,
) -> (HashMap<usize, (u64, u8)>, HashMap<usize, (u64, u64)>) {
    let mut move_map = HashMap::new();
    let mut resize_map = HashMap::new();
    if prs.note_move_active {
        move_map.reserve(prs.note_move_orig.len());
        for &(i, start, pitch) in &prs.note_move_orig {
            move_map.insert(i, (start, pitch));
        }
    }
    if prs.note_resize_active {
        resize_map.reserve(prs.note_resize_targets.len());
        for &(i, start, dur) in &prs.note_resize_targets {
            resize_map.insert(i, (start, dur));
        }
    }
    (move_map, resize_map)
}

/// Geometría de la nota `i` con los previews de arrastre/resize ya aplicados.
fn effective_note_geometry(
    prs: &PianoRollState,
    i: usize,
    move_map: &HashMap<usize, (u64, u8)>,
    resize_map: &HashMap<usize, (u64, u64)>,
) -> (u64, u64, u8) {
    let note = &prs.notes[i];
    let (mut start, mut dur, mut pitch) = (note.start_tick, note.duration_ticks, note.pitch);
    if let Some(&(orig_start, orig_pitch)) = move_map.get(&i) {
        let (new_start, new_pitch) = apply_move_delta(
            orig_start,
            orig_pitch,
            prs.note_move_preview_delta_ticks,
            prs.note_move_preview_delta_rows,
        );
        start = new_start;
        pitch = new_pitch;
    }
    if let Some(&(orig_start, orig_dur)) = resize_map.get(&i) {
        match prs.note_resize_handle {
            SelectionDragHandle::Left => {
                let (new_start, new_dur) =
                    apply_resize_left_delta(orig_start, orig_dur, prs.note_resize_delta_start);
                start = new_start;
                dur = new_dur;
            }
            SelectionDragHandle::Right => {
                let (_, new_dur) =
                    apply_resize_right_delta(orig_start, orig_dur, prs.note_resize_delta_duration);
                dur = new_dur;
            }
            SelectionDragHandle::None => {}
        }
    }
    (start, dur, pitch)
}

/// Nombre de la tecla de la fila `row` (fila 0 = G9).
fn key_label(mode: PianoRollMode, prs: &PianoRollState, row: u32) -> gpui_kit::SharedString {
    let pitch = (MIDI_KEY_COUNT - 1 - row.min(MIDI_KEY_COUNT - 1)) as u8;
    match mode {
        PianoRollMode::Keys => pitch_name_table()[row as usize].clone(),
        PianoRollMode::Drums => match prs.drum_map.get(&pitch) {
            Some(name) => gpui_kit::SharedString::from(format!("{} ({})", name, pitch)),
            None => gpui_kit::SharedString::from(format!("Pad {}", pitch)),
        },
    }
}

/// Layout inicial del piano roll: zoom que llena el panel + foco en el registro
/// medio (C4) en vez de arrancar arriba del todo (G9).
///
/// Devuelve `true` si hay que pedir otro frame (el viewport aún no se conoce, o
/// recién se acaba de aplicar el layout y hace falta repintar).
pub fn sync_frame(prs: &mut PianoRollState) -> bool {
    let viewport = prs.grid_scroll.bounds().size;
    if viewport.width.as_f32() <= 1.0 || viewport.height.as_f32() <= 1.0 {
        if prs.layout_initialized {
            return false;
        }
        // Todavía sin prepaint: pedimos otro frame con un presupuesto acotado.
        // El presupuesto se rearma sólo cuando vuelve a hacer falta (panel
        // recién abierto), así que no hay forma de pedir frames para siempre.
        if prs.layout_wait_frames == 0 {
            prs.layout_wait_frames = LAYOUT_WAIT_FRAMES;
        }
        prs.layout_wait_frames -= 1;
        return true;
    }

    if !prs.layout_initialized {
        prs.zoom_x = fit_zoom_x(viewport.width.as_f32(), BARS_FIT_TO_PANEL);
        let viewport_h = viewport.height.as_f32();
        let content_h = RULER_HEIGHT + MIDI_KEY_COUNT as f32 * prs.key_height;
        let max_scroll = (content_h - viewport_h).max(0.0);
        let top = focus_scroll_y(viewport_h, prs.key_height, FOCUS_PITCH).clamp(0.0, max_scroll);
        prs.grid_scroll.set_offset(point(px(0.0), px(-top)));
        prs.layout_initialized = true;
        return true;
    }

    false
}

pub fn render(cx: &mut Context<HikaruApp>) -> AnyElement {
    let app = state(cx).read(cx);
    let prs = &app.piano_roll_state;
    let mode = prs.mode;
    let zoom_x = prs.zoom_x;
    let key_height = prs.key_height;
    let playhead_tick = prs.playhead_tick;
    let sel_active = prs.selection_active;
    let sel_start = prs.selection_start_tick;
    let sel_end = prs.selection_end_tick;
    let loop_enabled = prs.loop_enabled;
    let sel_notes = prs.selected_notes.len();

    let sidebar_width = SIDEBAR_WIDTH;
    let max_note_tick = prs
        .notes
        .iter()
        .map(|n| n.start_tick.saturating_add(n.duration_ticks))
        .max()
        .unwrap_or(0);
    let total_ticks = total_ticks_for(max_note_tick, playhead_tick);
    let grid_h = MIDI_KEY_COUNT as f32 * key_height;
    let content_w = sidebar_width + total_ticks as f32 * zoom_x;
    let content_h = RULER_HEIGHT + grid_h;

    let scroll = prs.grid_scroll.clone();
    let vp = compute_grid_viewport(scroll.offset(), scroll.bounds().size, key_height, zoom_x);

    // Ventana visible en coordenadas de CONTENIDO, recortada contra el
    // contenido. Todos los hijos absolutos se posicionan con esta ventana y
    // miden como máximo el viewport: así ningún nodo de layout crece con los
    // 32 compases ni con las 128 filas.
    let win_x0 = vp.scroll_x.clamp(0.0, content_w);
    let win_y0 = vp.scroll_y.clamp(RULER_HEIGHT, content_h);
    let win_x1 = (win_x0 + vp.width).min(content_w);
    let win_y1 = (vp.scroll_y + vp.height).min(content_h);
    let win_w = (win_x1 - win_x0).max(1.0);
    let win_h = (win_y1 - win_y0).max(1.0);

    // ---------------------------------------------------------------------
    // NOTAS: sólo las que tocan el viewport.
    // ---------------------------------------------------------------------
    let (move_map, resize_map) = drag_lookup_maps(prs);
    let mut note_rects: Vec<AnyElement> = Vec::new();
    for i in 0..prs.notes.len() {
        let (st, dur, pitch) = effective_note_geometry(prs, i, &move_map, &resize_map);
        if !note_visible_in(st, dur, pitch, &vp) {
            continue;
        }
        let is_sel = prs.selected_notes.contains(&i);
        let nx = sidebar_width + st as f32 * zoom_x;
        let ny = (MIDI_KEY_COUNT - 1 - u32::from(pitch)) as f32 * key_height;
        let nw = (dur as f32 * zoom_x).max(4.0);
        let body_color = if resize_map.contains_key(&i) {
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
                // `NamedInteger` en vez de `format!("pr_note_{i}")`: el id no
                // aloca por frame.
                .id(("pr_note", i as u64))
                .on_click(move |event, _, cx| {
                    let shift = event.modifiers().shift;
                    let st2 = state(cx);
                    st2.update(cx, |state, cx| {
                        let prs = &mut state.piano_roll_state;
                        select_note_set(&mut prs.selected_notes, i, shift);
                        cx.notify();
                    });
                })
                .into_any_element(),
        );
    }

    // ---------------------------------------------------------------------
    // GRILLA: bandas negras + líneas de fila, en UN SOLO canvas.
    //
    // Este era EL congelamiento. El loop creaba un canvas por fila (128) y
    // cada uno llevaba `.w(canvas_w).h(grid_h)`: 128 capas de 73.000 x 2.048 px
    // superpuestas, cada una con su `request_layout` y su `paint_path`, en cada
    // frame (y durante la reproducción hay un `cx.notify()` por frame). Ahora
    // hay un canvas del tamaño del viewport con un único path.
    // ---------------------------------------------------------------------
    let rows_top = RULER_HEIGHT + vp.row_start as f32 * key_height;
    let grid_canvas = canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let rows_h = bounds.size.height;
            let mut path = PathBuilder::stroke(px(0.5));
            for (i, row) in (vp.row_start..vp.row_end).enumerate() {
                let pitch = (MIDI_KEY_COUNT - 1 - row) as u8;
                let y = bounds.origin.y + px(i as f32 * key_height);
                if is_black_key(pitch) {
                    window.paint_quad(PaintQuad {
                        bounds: Bounds::new(
                            point(bounds.origin.x, y),
                            size(bounds.size.width, px(key_height)),
                        ),
                        background: rgba(0x00000026).into(),
                        border_color: rgba(0x00000000).into(),
                        corner_radii: gpui_kit::Corners::default(),
                        border_widths: gpui_kit::Edges::default(),
                        border_style: BorderStyle::default(),
                    });
                }
                path.move_to(point(bounds.origin.x, y));
                path.line_to(point(bounds.origin.x + bounds.size.width, y));
            }
            let _ = rows_h;
            if let Ok(built) = path.build() {
                window.paint_path(built, rgb(0x1E1E1E));
            }
        },
    )
    .absolute()
    .left(px(win_x0))
    .top(px(rows_top))
    .w(px(win_w))
    .h(px(vp.visible_rows_height(key_height)));

    let mut sidebar_rows: Vec<AnyElement> = Vec::new();
    for row in vp.row_start..vp.row_end {
        let pitch = (MIDI_KEY_COUNT - 1 - row) as u8;
        let y = row as f32 * key_height;
        let is_black = is_black_key(pitch);
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
        // La etiqueta sale de una tabla precalculada: antes eran 128 `format!`
        // por frame (y 128 `format!("pr_key_{}")` para los ids).
        let label = key_label(mode, prs, row);
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
                .id(("pr_key", u64::from(pitch)))
                .test_support()
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

    // La selección y el playhead también se acotan a la ventana visible: eran
    // canvas de `content_w x grid_h` completos, los tres más grandes del árbol.
    // Además el playhead no se crea si está fuera de la vista.
    let sel_visible = sel_active
        && sel_end > sel_start
        && sel_end >= vp.tick_start
        && sel_start <= vp.tick_end;
    let sel_canvas = canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            if sel_visible {
                let sx0 = bounds.origin.x + px(sel_start as f32 * zoom_x - win_x0);
                let sx1 = bounds.origin.x + px(sel_end as f32 * zoom_x - win_x0);
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
                    if let Ok(built) = path.build() {
                        window.paint_path(built, bracket);
                    }
                }
            }
        },
    )
    .absolute()
    .left(px(win_x0))
    .top(px(win_y0))
    .w(px(win_w))
    .h(px(win_h));

    let playhead_visible = playhead_tick >= vp.tick_start && playhead_tick <= vp.tick_end;
    let playhead_canvas = playhead_visible.then(|| {
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let phx = bounds.origin.x + px(playhead_tick as f32 * zoom_x - win_x0);
                let mut path = PathBuilder::stroke(px(2.0));
                path.move_to(point(phx, bounds.origin.y));
                path.line_to(point(phx, bounds.origin.y + bounds.size.height));
                if let Ok(built) = path.build() {
                    window.paint_path(built, rgb(0x00C8FF));
                }
            },
        )
        .absolute()
        .left(px(win_x0))
        .top(px(win_y0))
        .w(px(win_w))
        .h(px(win_h))
        .into_any_element()
    });

    // Capa de interacción: cubre sólo la ventana visible de la grilla. Va
    // ANTES que las notas para que el click caiga en la nota y no en el fondo
    // (antes las notas eran inalcanzables: quedaban tapadas por esta capa).
    let grid_interaction = div()
        .absolute()
        .left(px(win_x0))
        .top(px(win_y0))
        .w(px(win_w))
        .h(px(win_h))
        .id("pr_grid_interaction")
        .test_support()
        .on_click(move |event, _, cx| {
            let Some(pos) = event.mouse_position() else {
                return;
            };
            let st2 = state(cx);
            st2.update(cx, |state, cx| {
                let prs = &mut state.piano_roll_state;
                // `pos` viene en coordenadas de ventana; el contenido está
                // corrido por el scroll en AMBOS ejes, así que hay que
                // deshacerlo (antes sólo se restaban sidebar y ruler, así que
                // con scroll el click caía en la fila/tick equivocados).
                let off = prs.grid_scroll.offset();
                let content_x = pos.x.as_f32() - off.x.as_f32();
                let content_y = pos.y.as_f32() - off.y.as_f32();
                let local_x = content_x - SIDEBAR_WIDTH;
                let local_y = content_y - RULER_HEIGHT;
                if local_x < 0.0 || local_y < 0.0 {
                    return;
                }
                let raw_tick = (local_x / zoom_x).max(0.0) as u64;
                let q_tick = quantize_tick(raw_tick, QUANTIZE_TICKS);
                let row = (local_y / key_height).max(0.0) as i32;
                let pitch = (MIDI_KEY_COUNT as i32 - 1 - row).clamp(0, 127) as u8;
                let already = prs.notes.iter().any(|n| {
                    n.pitch == pitch
                        && q_tick >= n.start_tick
                        && q_tick < n.start_tick.saturating_add(n.duration_ticks)
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

    // REGLA: un canvas del ancho de la ventana visible, y el loop arranca en el
    // primer compás dentro de la vista en vez de recorrer los 32 desde el 0.
    // Con el zoom anterior eran ~480 barras dibujadas por frame.
    let bar_w = TICKS_PER_BAR as f32 * zoom_x;
    let beat_w = TICKS_PER_BEAT as f32 * zoom_x;
    let ruler_canvas = canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let base = bounds.origin.x;
            let y_top = bounds.origin.y + px(4.0);
            let y_bottom = bounds.origin.y + bounds.size.height;
            // Subdivisión de compás sólo si entra sin quedar ilegible.
            let mut step = TICKS_PER_BAR;
            if bar_w >= 4.0 * 56.0 {
                step = TICKS_PER_BEAT;
            }
            let first = vp.tick_start / step * step;
            let mut bars = PathBuilder::stroke(px(1.5));
            let mut minor = PathBuilder::stroke(px(0.5));
            let mut tick = first;
            while tick <= vp.tick_end + step {
                let x = base + px(tick as f32 * zoom_x - win_x0);
                if x > bounds.origin.x + bounds.size.width {
                    break;
                }
                if tick % TICKS_PER_BAR == 0 {
                    bars.move_to(point(x, y_top));
                    bars.line_to(point(x, y_bottom));
                    // Bandera del compás: se mantiene como rectángulo pintado a
                    // mano (igual que antes) en vez de `paint_text`, que
                    // necesita una fuente cargada en la ventana.
                    let mut flag = PathBuilder::fill();
                    flag.move_to(point(x + px(5.0), bounds.origin.y + px(3.0)));
                    flag.line_to(point(x + px(13.0), bounds.origin.y + px(3.0)));
                    flag.line_to(point(x + px(13.0), bounds.origin.y + px(13.0)));
                    flag.line_to(point(x + px(5.0), bounds.origin.y + px(13.0)));
                    flag.close();
                    if let Ok(built) = flag.build() {
                        window.paint_path(built, rgb(0xD2D2D2));
                    }
                } else if beat_w >= 12.0 {
                    minor.move_to(point(x, y_top));
                    minor.line_to(point(x, y_bottom));
                }
                tick += step;
            }
            if let Ok(built) = bars.build() {
                window.paint_path(built, rgb(0x8C8C8C));
            }
            if let Ok(built) = minor.build() {
                window.paint_path(built, rgb(0x4A4A4A));
            }
        },
    )
    .absolute()
    .left(px(win_x0))
    .top(px(0.0))
    .w(px(win_w))
    .h(px(RULER_HEIGHT))
    .into_any_element();

    let bar = (playhead_tick / TICKS_PER_BAR) + 1;
    let beat = ((playhead_tick % TICKS_PER_BAR) / TICKS_PER_BEAT) + 1;

    v_flex()
        .id("piano_roll")
        .test_support()
        .size_full()
        .child(
            h_flex()
                .items_center()
                .gap(px(6.0))
                .h(px(28.0))
                .bg(rgb(0x181A20))
                .px(px(8.0))
                .child(
                    Button::new("pr_mode_keys").rounded(gpui_kit::component::button::ButtonRounded::None)
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
                    Button::new("pr_mode_drums").rounded(gpui_kit::component::button::ButtonRounded::None)
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
                            Button::new("pr_loop_toggle").rounded(gpui_kit::component::button::ButtonRounded::None)
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
                            Button::new("pr_clear_sel").rounded(gpui_kit::component::button::ButtonRounded::None)
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
                            Button::new("pr_clear_notes").rounded(gpui_kit::component::button::ButtonRounded::None)
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
            // Raíz del scroller. La barra va como HERMANA del área scrolleable
            // (no como hija): GPUI le aplica el offset de scroll a los hijos, así
            // que como hija se iría con el scroll. Es la misma estructura que
            // arma `gpui_component::scroll::Scrollable`.
            div()
                .id("pr_scroll_root")
                .test_support()
                .flex_1()
                .relative()
                .overflow_hidden()
                .child(
                    div()
                        .id("pr_scroll_area")
                        .test_support()
                        .size_full()
                        // Scroll en AMBOS ejes: con sólo horizontal (como
                        // estaba) las 128 filas no se podían scrollear y la
                        // vista quedaba clavada en G9 con el resto vacío.
                        .overflow_scroll()
                        .track_scroll(&scroll)
                        .child(
                            div()
                                .id("pr_content")
                                .test_support()
                                .w(px(content_w))
                                .h(px(content_h))
                                .flex_none()
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
                                .child(ruler_canvas)
                                .children(sidebar_rows)
                                .child(grid_canvas.into_any_element())
                                .child(sel_canvas.into_any_element())
                                .child(playhead_canvas.unwrap_or_else(|| div().into_any_element()))
                                // La capa de interacción va antes que las notas
                                // para que el click llegue a la nota.
                                .child(grid_interaction.into_any_element())
                                .children(note_rects),
                        ),
                )
                .child(
                    gpui_kit::component::scroll::Scrollbar::new(&scroll)
                        .id("pr_scrollbar")
                        .axis(gpui_kit::component::scroll::ScrollbarAxis::Both)
                        .viewport_from_layout(),
                ),
        )
        .into_any_element()
}
