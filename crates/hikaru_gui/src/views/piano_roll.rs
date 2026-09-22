// Copyright (C) Hikaru Corporation - 2026
// GNU Affero General Public License v3
// crates/hikaru_gui/src/views/piano_roll.rs

use egui::*;
use std::collections::{HashMap, HashSet};
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
    /// Inicio de la selección de tiempo en ticks.
    pub selection_start_tick: u64,
    /// Fin de la selección de tiempo en ticks.
    pub selection_end_tick: u64,
    /// Si hay una selección de tiempo activa.
    pub selection_active: bool,
    /// Estado temporal para dibujar la selección mientras se arrastra.
    pub selection_dragging: bool,
    /// Selección visual (translúcida) mientras dura el Shift+Drag.
    pub selection_preview_start_ticks: u64,
    pub selection_preview_end_ticks: u64,
    pub selection_preview_active: bool,
    /// Handle activo para redimensionar la selección.
    pub selection_drag_handle: SelectionDragHandle,
    /// Si el loop de la selección está activado.
    pub loop_enabled: bool,
    /// Tick del transporte cuando empezó el loop.
    pub loop_transport_start_tick: u64,
    /// Timestamp del sistema (Instant) cuando empezó el loop.
    pub loop_start_instant: Option<std::time::Instant>,
    /// Si hay un resize de nota en curso por uno de sus extremos.
    pub note_resize_active: bool,
    /// Extremo que se está arrastrando (Left o Right).
    pub note_resize_handle: SelectionDragHandle,
    /// Índice en `notes` de la nota que se está redimensionando.
    pub note_resize_index: Option<usize>,
    /// Valores originales de la nota al iniciar el resize.
    pub note_resize_orig_start: u64,
    pub note_resize_orig_duration: u64,
    /// Preview visual mientras dura el arrastre.
    pub note_resize_preview_start: u64,
    pub note_resize_preview_duration: u64,
    /// Índices de notas seleccionadas (marquee con click derecho).
    /// NO confundir con la Time Selection (que sirve para loopear).
    pub selected_notes: HashSet<usize>,
    /// Si el marquee de selección de notas está en curso.
    pub note_marquee_active: bool,
    /// Esquina de inicio del marquee (coords de contenido).
    pub note_marquee_start: Option<Pos2>,
    /// Esquina actual del marquee mientras se arrastra.
    pub note_marquee_current: Option<Pos2>,
    /// Si el marquee actual añade a la selección (Shift) o la reemplaza.
    pub note_marquee_additive: bool,
    /// Slot del que se cargaron las notas (para invalidar la selección al cambiar).
    pub notes_source_slot: Option<(usize, usize)>,
    /// Si hay un arrastre (mover) de notas seleccionadas en curso.
    pub note_move_active: bool,
    /// Índice de la nota agarrada para iniciar el move.
    pub note_move_grab_index: Option<usize>,
    /// Posiciones originales `(index, start_tick, pitch)` al iniciar el arrastre.
    pub note_move_orig: Vec<(usize, u64, u8)>,
    /// Delta preview en ticks (ya cuantizado al Snap to Grid).
    pub note_move_preview_delta_ticks: i64,
    /// Delta preview en filas (semitonos).
    pub note_move_preview_delta_rows: i32,
    /// Puntero donde se originó el press del move (para delta acumulado).
    pub note_move_origin_pointer: Option<Pos2>,
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
            selection_preview_start_ticks: 0,
            selection_preview_end_ticks: 0,
            selection_preview_active: false,
            selection_drag_handle: SelectionDragHandle::None,
            loop_enabled: false,
            loop_transport_start_tick: 0,
            loop_start_instant: None,
            note_resize_active: false,
            note_resize_handle: SelectionDragHandle::None,
            note_resize_index: None,
            note_resize_orig_start: 0,
            note_resize_orig_duration: 0,
            note_resize_preview_start: 0,
            note_resize_preview_duration: 0,
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
        self.selection_preview_start_ticks = 0;
        self.selection_preview_end_ticks = 0;
        self.selection_preview_active = false;
        self.selection_drag_handle = SelectionDragHandle::None;
        self.loop_enabled = false;
        self.loop_transport_start_tick = 0;
        self.loop_start_instant = None;
        self.note_resize_active = false;
        self.note_resize_handle = SelectionDragHandle::None;
        self.note_resize_index = None;
        self.note_resize_preview_start = 0;
        self.note_resize_preview_duration = 0;
        self.note_move_active = false;
        self.note_move_grab_index = None;
        self.note_move_orig.clear();
        self.note_move_preview_delta_ticks = 0;
        self.note_move_preview_delta_rows = 0;
        self.note_move_origin_pointer = None;
    }

    /// Limpia solo la selección de NOTAS (marquee), sin tocar la Time Selection.
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

const QUANTIZE_TICKS: u64 = 240; // 1/16 note a 960 PPQ
const TICKS_PER_BEAT: u64 = 960;
const TICKS_PER_BAR: u64 = 3840; // 4/4 a 960 PPQ
const RULER_HEIGHT: f32 = 24.0;   // Altura fija de la regla de compases
const NOTE_INSERT_VELOCITY: u8 = 100;
/// Ancho en px de las hitboxes de resize en los extremos de cada nota.
const NOTE_EDGE_HIT_W: f32 = 8.0;
/// Duración mínima de una nota al redimensionar (1/16).
const MIN_NOTE_DURATION_TICKS: u64 = QUANTIZE_TICKS;

/// Cuantiza `tick` a múltiplos de `step` (hacia abajo), sin bajar de 0.
fn quantize_tick(tick: u64, step: u64) -> u64 {
    if step == 0 {
        return tick;
    }
    (tick / step) * step
}

/// Redimensiona el extremo **izquierdo** de una nota.
/// El extremo derecho queda anclado; se devuelve `(nuevo_start, nueva_duration)`.
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

/// Redimensiona el extremo **derecho** de una nota.
/// El extremo izquierdo queda anclado; se devuelve `(start, nueva_duration)`.
fn resize_note_right(orig_start: u64, _orig_duration: u64, pointer_tick: u64) -> (u64, u64) {
    let min_dur = MIN_NOTE_DURATION_TICKS;
    let new_end = quantize_tick(pointer_tick, QUANTIZE_TICKS).max(orig_start + min_dur);
    let new_duration = new_end - orig_start;
    (orig_start, new_duration)
}

/// Aplica un delta de arrastre a una nota original `(start, pitch)`.
/// El pitch se desplaza por filas (semitonos) y se acota a `0..=127`.
fn apply_move_delta(start_tick: u64, pitch: u8, delta_ticks: i64, delta_rows: i32) -> (u64, u8) {
    let new_start = if delta_ticks >= 0 {
        start_tick.saturating_add(delta_ticks as u64)
    } else {
        start_tick.saturating_sub(delta_ticks.unsigned_abs())
    };
    let new_pitch = (i32::from(pitch) - delta_rows).clamp(0, 127) as u8;
    (new_start, new_pitch)
}

/// Calcula los deltas ya saneados de un arrastre de notas.
///
/// - El delta horizontal se obtiene cuantizando la posición destino de la nota
///   agarrada al Snap to Grid (`QUANTIZE_TICKS`), de modo que todas las notas
///   seleccionadas se muevan juntas preservando sus offsets relativos.
/// - Se acota para que ninguna nota salga de `start >= 0` ni de `pitch 0..=127`.
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
        // new_pitch = pitch - delta_rows; acotar a 0..=127 para todas.
        delta_rows = delta_rows.clamp(max_pitch - 127, min_pitch);
    }

    (delta_ticks, delta_rows)
}

/// Selección de una nota al hacer click/arrastre sobre su cuerpo.
/// `additive` (Shift) añade a la selección; si la nota ya está seleccionada
/// y no es additive, se conserva la selección múltiple actual.
fn select_note_set(selected: &mut HashSet<usize>, index: usize, additive: bool) {
    if additive {
        selected.insert(index);
    } else if !selected.contains(&index) {
        selected.clear();
        selected.insert(index);
    }
}

/// Hit-testing manual del **cuerpo** de una nota (excluye los extremos de
/// resize). Devuelve el índice de la nota más arriba (última dibujada).
/// Sin `ui.interact` por nota: así no compite por el pointer capture con el
/// grid ni con los hitboxes de extremos (evita que el drag quede "trabado").
fn hit_test_note_body(
    notes: &[MidiNote],
    pos: Pos2,
    grid_rect: Rect,
    zoom_x: f32,
    key_height: f32,
) -> Option<usize> {
    let inset = NOTE_EDGE_HIT_W * 0.5;
    for (i, note) in notes.iter().enumerate().rev() {
        let rect = note_rect(grid_rect, note, zoom_x, key_height);
        if !rect.intersects(grid_rect) || !rect.contains(pos) {
            continue;
        }
        // Nota angosta: todo el rect es zona de extremos (resize).
        if rect.width() <= NOTE_EDGE_HIT_W {
            return None;
        }
        let body = Rect::from_min_max(
            pos2(rect.min.x + inset, rect.min.y),
            pos2(rect.max.x - inset, rect.max.y),
        );
        if body.contains(pos) {
            return Some(i);
        }
    }
    None
}

/// Posición efectiva `(start, duration, pitch)` de una nota para dibujo:
/// aplica el preview de move (si está activo) y el de resize.
fn effective_note_view(state: &PianoRollState, index: usize, note: &MidiNote) -> (u64, u64, u8) {
    let mut start = note.start_tick;
    let mut duration = note.duration_ticks;
    let mut pitch = note.pitch;

    if state.note_move_active {
        if let Some(&(_, orig_start, orig_pitch)) =
            state.note_move_orig.iter().find(|(i, _, _)| *i == index)
        {
            let (ns, np) = apply_move_delta(
                orig_start,
                orig_pitch,
                state.note_move_preview_delta_ticks,
                state.note_move_preview_delta_rows,
            );
            start = ns;
            pitch = np;
        }
    }
    if state.note_resize_active && state.note_resize_index == Some(index) {
        start = state.note_resize_preview_start;
        duration = state.note_resize_preview_duration;
    }

    (start, duration, pitch)
}

/// Hit-testing del rect completo de una nota (incluye extremos de resize).
/// Devuelve la nota más arriba (última dibujada) cuyo rect contiene `pos`.
fn hit_test_note_full(
    notes: &[MidiNote],
    pos: Pos2,
    grid_rect: Rect,
    zoom_x: f32,
    key_height: f32,
) -> Option<usize> {
    for (i, note) in notes.iter().enumerate().rev() {
        let rect = note_rect(grid_rect, note, zoom_x, key_height);
        if rect.intersects(grid_rect) && rect.contains(pos) {
            return Some(i);
        }
    }
    None
}

/// Índices de las notas cuyo rectángulo intersecta `marquee` (en coords de contenido).
fn notes_in_marquee(
    notes: &[MidiNote],
    marquee: Rect,
    grid_rect: Rect,
    zoom_x: f32,
    key_height: f32,
) -> HashSet<usize> {
    let mut out = HashSet::new();
    for (i, note) in notes.iter().enumerate() {
        let rect = note_rect(grid_rect, note, zoom_x, key_height);
        if rect.intersects(marquee) {
            out.insert(i);
        }
    }
    out
}

/// Duplica las notas seleccionadas, colocándolas justo después del bloque
/// que ocupan (mismo criterio que Ctrl+D en la Playlist).
/// Devuelve las notas nuevas y sus índices tras insertarlas.
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

pub fn show(
    ui: &mut Ui,
    state: &mut PianoRollState,
    tracks: &[Track],
    selected_track_index: usize,
    audio_proxy: &AudioProxy,
    bpm: f64,
    ppqn: u64,
    is_playing: bool,
) {
    if let Some(track) = tracks.get(selected_track_index) {
        let has_opendms = track.effects.iter().any(|s| s.name == "Hikaru OpenDMS");
        if has_opendms && state.mode == PianoRollMode::Keys && state.drum_map.is_empty() {
            state.mode = PianoRollMode::Drums;
        }
    }

    if let Some(track) = tracks.get(selected_track_index) {
        if let Some(opendms) = find_opendms_in_track(track) {
            sync_drum_map_from_opendms(state, opendms);
        }
    }

    // Sanear selección de notas por si el clip cambió de tamaño.
    state.selected_notes.retain(|&i| i < state.notes.len());

    // Ctrl+D: duplicar notas seleccionadas por el marquee (click derecho+drag).
    // Distinto del Ctrl+D de la Playlist (que duplica clips).
    // consume_key evita que el atajo llegue también a la Playlist si está visible.
    if !state.selected_notes.is_empty() {
        let ctrl = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
        if ctrl && ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::D)) {
            let new_indices = duplicate_selected_notes(&mut state.notes, &state.selected_notes);
            state.selected_notes = new_indices.into_iter().collect();
            ui.ctx().request_repaint();
        }
    }

    // Escape: limpiar solo la selección de notas (no la Time Selection).
    if !state.selected_notes.is_empty() && ui.input(|i| i.key_pressed(Key::Escape)) {
        state.clear_note_selection();
        ui.ctx().request_repaint();
    }

    ui.vertical(|ui| {
        // --- 1. TOOLBAR ---
        ui.horizontal(|ui| {
            ui.selectable_value(&mut state.mode, PianoRollMode::Keys, "\u{1F3B9} Keys");
            ui.selectable_value(&mut state.mode, PianoRollMode::Drums, "\u{1F941} Drums");
            ui.separator();
            ui.label(RichText::new("Zoom H:").small());
            ui.add(Slider::new(&mut state.zoom_x, 0.02..=0.8).text("X"));
            ui.add(Slider::new(&mut state.key_height, 10.0..=28.0).text("Y"));
            ui.separator();

            let bar = (state.playhead_tick / TICKS_PER_BAR) + 1;
            let beat = ((state.playhead_tick % TICKS_PER_BAR) / TICKS_PER_BEAT) + 1;
            ui.label(RichText::new(format!("Compás: {}.{} | Tick: {}", bar, beat, state.playhead_tick)).small());

            if state.selection_active {
                ui.separator();
                let sel_start_bar = (state.selection_start_tick / TICKS_PER_BAR) + 1;
                let sel_start_beat = ((state.selection_start_tick % TICKS_PER_BAR) / TICKS_PER_BEAT) + 1;
                let sel_end_bar = (state.selection_end_tick / TICKS_PER_BAR) + 1;
                let sel_end_beat = ((state.selection_end_tick % TICKS_PER_BAR) / TICKS_PER_BEAT) + 1;
                ui.label(RichText::new(format!(
                    "Sel: {}.{} -> {}.{}",
                    sel_start_bar, sel_start_beat, sel_end_bar, sel_end_beat
                )).small().color(Color32::LIGHT_BLUE));

                let loop_text = if state.loop_enabled { "Loop ON" } else { "Loop OFF" };
                let loop_color = if state.loop_enabled { Color32::LIGHT_GREEN } else { Color32::GRAY };
                if ui.selectable_label(state.loop_enabled, RichText::new(loop_text).color(loop_color))
                    .on_hover_text("Activar/desactivar loop de la selección")
                    .clicked()
                {
                    state.loop_enabled = !state.loop_enabled;
                }

                if ui.small_button("X Sel").on_hover_text("Limpiar selección (Shift+Drag en regla para crear)").clicked() {
                    state.clear_selection();
                    state.loop_enabled = false;
                }
            }

            if !state.selected_notes.is_empty() {
                ui.separator();
                ui.label(RichText::new(format!("Notas: {}", state.selected_notes.len()))
                    .small()
                    .color(Color32::YELLOW))
                    .on_hover_text("Selección de notas (click derecho+drag). Arrastrá el cuerpo para moverlas (Snap to Grid). Ctrl+D duplica. Esc limpia.");
                if ui.small_button("X Notas")
                    .on_hover_text("Limpiar selección de notas (no toca la Time Selection)")
                    .clicked()
                {
                    state.clear_note_selection();
                    ui.ctx().request_repaint();
                }
            }
        });

        ui.separator();

        let sidebar_width = 140.0;
        let grid_height = 128.0 * state.key_height;

        // Longitud dinámica de grilla
        let max_note_tick = state
            .notes
            .iter()
            .map(|n| n.start_tick + n.duration_ticks)
            .max()
            .unwrap_or(0);
        
        let min_default_ticks = TICKS_PER_BAR * 128;
        let active_boundary = max_note_tick.max(state.playhead_tick);
        let total_ticks = min_default_ticks.max(active_boundary + (TICKS_PER_BAR * 16));
        let total_grid_width = total_ticks as f32 * state.zoom_x;

        // --- 2. ÁREA DE CONTENIDO INTEGRADA (REGLA + GRILLA CON UN SOLO SCROLL H/V) ---
        ScrollArea::both()
            .id_source("piano_roll_body_scroll")
            .show_viewport(ui, |ui, viewport| {
                let content_width = (sidebar_width + total_grid_width).max(ui.available_width());
                let total_content_height = RULER_HEIGHT + grid_height;

                let (content_rect, _) = ui.allocate_exact_size(
                    vec2(content_width, total_content_height),
                    Sense::hover(),
                );

                // --- REGLA Y ESQUINA (Sticky Top con offset según el viewport de scroll) ---
                // Ajuste para pegar la regla exactamente al borde superior sin flotado:
                let ruler_y = (content_rect.min.y + viewport.min.y - 3.0).max(content_rect.min.y);
                let corner_rect = Rect::from_min_size(
                    pos2(content_rect.min.x + viewport.min.x, ruler_y),
                    vec2(sidebar_width, RULER_HEIGHT),
                );
                
                let ruler_rect = Rect::from_min_size(
                    pos2(content_rect.min.x + sidebar_width, ruler_y),
                    vec2(total_grid_width, RULER_HEIGHT),
                );

                // Dibujar el contenido de la grilla principal
                let sidebar_rect = Rect::from_min_size(
                    pos2(content_rect.min.x + viewport.min.x, content_rect.min.y + RULER_HEIGHT),
                    vec2(sidebar_width, grid_height),
                );

                let grid_rect = Rect::from_min_size(
                    pos2(content_rect.min.x + sidebar_width, content_rect.min.y + RULER_HEIGHT),
                    vec2(total_grid_width, grid_height),
                );

                // Fondo de grilla y notas
                if ui.is_rect_visible(grid_rect) {
                    let painter = ui.painter_at(grid_rect);
                    draw_grid_background(&painter, grid_rect, state.key_height, state.zoom_x);

                    // Dibujar Time Selection sobre la grilla
                    let (render_active, render_start, render_end) = if state.selection_dragging && state.selection_preview_active {
                        (true, state.selection_preview_start_ticks, state.selection_preview_end_ticks)
                    } else {
                        (state.selection_active, state.selection_start_tick, state.selection_end_tick)
                    };

                    if render_active && render_end > render_start {
                        let start_x = render_start as f32 * state.zoom_x;
                        let end_x = render_end as f32 * state.zoom_x;
                        let (min_x, max_x) = if start_x <= end_x { (start_x, end_x) } else { (end_x, start_x) };

                        let selection_rect = Rect::from_min_max(
                            pos2(grid_rect.min.x + min_x, grid_rect.min.y),
                            pos2(grid_rect.min.x + max_x, grid_rect.max.y),
                        );

                        // Fondo translúcido de la selección
                        painter.rect_filled(
                            selection_rect,
                            0.0,
                            Color32::from_rgba_unmultiplied(100, 200, 255, 40),
                        );

                        // Corchetes de selección estilo REAPER
                        let bracket_color = Color32::LIGHT_BLUE;
                        let bracket_stroke = Stroke::new(2.0_f32, bracket_color);
                        let tick_len = 6.0_f32;
                        let top_y = grid_rect.min.y;
                        let bottom_y = grid_rect.max.y;

                        // Corchete izquierdo `[`
                        painter.line_segment(
                            [pos2(grid_rect.min.x + min_x, top_y), pos2(grid_rect.min.x + min_x, bottom_y)],
                            bracket_stroke,
                        );
                        painter.line_segment(
                            [pos2(grid_rect.min.x + min_x, top_y), pos2(grid_rect.min.x + min_x + tick_len, top_y)],
                            bracket_stroke,
                        );
                        painter.line_segment(
                            [pos2(grid_rect.min.x + min_x, bottom_y), pos2(grid_rect.min.x + min_x + tick_len, bottom_y)],
                            bracket_stroke,
                        );

                        // Corchete derecho `]`
                        painter.line_segment(
                            [pos2(grid_rect.min.x + max_x, top_y), pos2(grid_rect.min.x + max_x, bottom_y)],
                            bracket_stroke,
                        );
                        painter.line_segment(
                            [pos2(grid_rect.min.x + max_x, top_y), pos2(grid_rect.min.x + max_x - tick_len, top_y)],
                            bracket_stroke,
                        );
                        painter.line_segment(
                            [pos2(grid_rect.min.x + max_x, bottom_y), pos2(grid_rect.min.x + max_x - tick_len, bottom_y)],
                            bracket_stroke,
                        );
                    }

                    for (i, note) in state.notes.iter().enumerate() {
                        // Durante un resize/move se dibuja el preview en lugar
                        // de los valores reales de la nota.
                        let (start_tick, duration_ticks, pitch) =
                            effective_note_view(state, i, note);
                        let preview_note = MidiNote {
                            pitch,
                            start_tick,
                            duration_ticks,
                            velocity: note.velocity,
                        };
                        let rect = note_rect(grid_rect, &preview_note, state.zoom_x, state.key_height);
                        if rect.intersects(grid_rect) {
                            let is_selected = state.selected_notes.contains(&i);
                            let body_color = if state.note_resize_active
                                && state.note_resize_index == Some(i)
                            {
                                Color32::from_rgb(255, 190, 60)
                            } else if is_selected {
                                Color32::from_rgb(255, 210, 60)
                            } else {
                                Color32::from_rgb(255, 140, 0)
                            };
                            painter.rect_filled(rect, 2.0, body_color);
                            let (stroke_w, stroke_c) = if is_selected {
                                (2.0_f32, Color32::from_rgb(255, 255, 160))
                            } else {
                                (1.0_f32, Color32::WHITE)
                            };
                            painter.rect_stroke(rect, 1.0, Stroke::new(stroke_w, stroke_c));
                        }
                    }
                }

                // Edición en la grilla
                let grid_response = ui.interact(
                    grid_rect,
                    ui.id().with("piano_roll_grid_interaction"),
                    Sense::click_and_drag(),
                );

                // --- RESIZE DE NOTAS POR LOS EXTREMOS (estilo DAW) ---
                // Hitboxes en el borde izquierdo y derecho de cada nota
                // visible. Prioridad sobre el grid: si el puntero está sobre
                // un extremo no se crea/borra nota ni se muestra el ghost.
                // PATRÓN PLAYLIST: durante `.dragged()` solo se toca el
                // preview; la confirmación es SOLO en `drag_stopped()`.
                let mut note_edge_active = false;
                let mut commit_note_resize = false;
                if ui.is_rect_visible(grid_rect) {
                    for (i, note) in state.notes.iter().enumerate() {
                        let (start_tick, duration_ticks, pitch) =
                            effective_note_view(state, i, note);
                        let preview_note = MidiNote {
                            pitch,
                            start_tick,
                            duration_ticks,
                            velocity: note.velocity,
                        };
                        let rect =
                            note_rect(grid_rect, &preview_note, state.zoom_x, state.key_height);
                        if !rect.intersects(grid_rect) {
                            continue;
                        }

                        let edge_h = rect.height().max(state.key_height - 1.0);
                        let left_hit = Rect::from_center_size(
                            pos2(rect.min.x, rect.center().y),
                            vec2(NOTE_EDGE_HIT_W, edge_h),
                        );
                        let right_hit = Rect::from_center_size(
                            pos2(rect.max.x, rect.center().y),
                            vec2(NOTE_EDGE_HIT_W, edge_h),
                        );
                        let left_id = ui.id().with(("note_resize_left", i));
                        let right_id = ui.id().with(("note_resize_right", i));
                        let l_resp = ui.interact(left_hit, left_id, Sense::drag());
                        let r_resp = ui.interact(right_hit, right_id, Sense::drag());

                        // Resize SOLO con botón izquierdo: el botón derecho queda
                        // libre para el marquee de selección de notas.
                        if l_resp.hovered() || l_resp.dragged_by(PointerButton::Primary) {
                            note_edge_active = true;
                            ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeHorizontal);
                        }
                        if r_resp.hovered() || r_resp.dragged_by(PointerButton::Primary) {
                            note_edge_active = true;
                            ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeHorizontal);
                        }

                        // Iniciar resize: sembrar preview desde la nota real.
                        // Solo si no hay ya otro resize ni un move en curso.
                        if !state.note_resize_active && !state.note_move_active {
                            if l_resp.drag_started_by(PointerButton::Primary) {
                                state.note_resize_active = true;
                                state.note_resize_handle = SelectionDragHandle::Left;
                                state.note_resize_index = Some(i);
                                state.note_resize_orig_start = note.start_tick;
                                state.note_resize_orig_duration = note.duration_ticks;
                                state.note_resize_preview_start = note.start_tick;
                                state.note_resize_preview_duration = note.duration_ticks;
                                note_edge_active = true;
                                ui.ctx().request_repaint();
                            } else if r_resp.drag_started_by(PointerButton::Primary) {
                                state.note_resize_active = true;
                                state.note_resize_handle = SelectionDragHandle::Right;
                                state.note_resize_index = Some(i);
                                state.note_resize_orig_start = note.start_tick;
                                state.note_resize_orig_duration = note.duration_ticks;
                                state.note_resize_preview_start = note.start_tick;
                                state.note_resize_preview_duration = note.duration_ticks;
                                note_edge_active = true;
                                ui.ctx().request_repaint();
                            }
                        }

                        // Arrastrar el extremo activo de ESTA nota.
                        if state.note_resize_active && state.note_resize_index == Some(i) {
                            let resp = match state.note_resize_handle {
                                SelectionDragHandle::Left => &l_resp,
                                SelectionDragHandle::Right => &r_resp,
                                SelectionDragHandle::None => continue,
                            };
                            note_edge_active = true;

                            if resp.dragged_by(PointerButton::Primary) {
                                if let Some(pointer_pos) = resp.interact_pointer_pos() {
                                    let rel_x = (pointer_pos.x - grid_rect.min.x).max(0.0);
                                    let raw_tick = (rel_x / state.zoom_x) as u64;
                                    let (new_start, new_dur) = match state.note_resize_handle {
                                        SelectionDragHandle::Left => resize_note_left(
                                            state.note_resize_orig_start,
                                            state.note_resize_orig_duration,
                                            raw_tick,
                                        ),
                                        SelectionDragHandle::Right => resize_note_right(
                                            state.note_resize_orig_start,
                                            state.note_resize_orig_duration,
                                            raw_tick,
                                        ),
                                        SelectionDragHandle::None => (0, 0),
                                    };
                                    state.note_resize_preview_start = new_start;
                                    state.note_resize_preview_duration = new_dur;
                                    ui.ctx().request_repaint();
                                }
                            }

                            // Confirmar al soltar (fuera del préstamo inmutable
                            // de `note`: se marca y se aplica tras el bucle).
                            if resp.drag_stopped_by(PointerButton::Primary) {
                                commit_note_resize = true;
                            }
                        }
                    }
                }

                // Aplicar el resize confirmado al soltar el puntero.
                if commit_note_resize {
                    if let Some(idx) = state.note_resize_index {
                        if idx < state.notes.len() {
                            state.notes[idx].start_tick = state.note_resize_preview_start;
                            state.notes[idx].duration_ticks =
                                state.note_resize_preview_duration.max(MIN_NOTE_DURATION_TICKS);
                        }
                    }
                    state.note_resize_active = false;
                    state.note_resize_handle = SelectionDragHandle::None;
                    state.note_resize_index = None;
                    note_edge_active = true;
                    ui.ctx().request_repaint();
                }

                // Si el resize sigue activo, no procesar el grid.
                if state.note_resize_active {
                    note_edge_active = true;
                }

                // --- MOVE DE NOTAS: drag con botón izquierdo sobre el cuerpo ---
                // Se resuelve SOLO con grid_response + hit-testing manual.
                // NO se crean hitboxes por nota para el cuerpo: competirían por
                // el pointer capture con el grid y con los extremos de resize,
                // y el drag quedaba "trabado" o se perdía el commit.
                //
                // El delta es ACUMULADO desde `note_move_origin_pointer`
                // (position del press). `Response::drag_delta()` solo da el
                // movimiento del frame actual, con lo cual la nota "rebotaba"
                // en lugar de seguir al puntero.
                let primary = PointerButton::Primary;
                let mut note_body_active = false;
                let mut commit_note_move = false;
                let mut just_started_move = false;

                /// Delta acumulado del pointer desde el origen del press.
                fn move_drag_delta(
                    origin: Option<Pos2>,
                    current: Option<Pos2>,
                ) -> Vec2 {
                    match (origin, current) {
                        (Some(o), Some(c)) => c - o,
                        _ => Vec2::ZERO,
                    }
                }

                if state.note_resize_active {
                    note_edge_active = true;
                } else if state.note_move_active {
                    note_body_active = true;
                    if grid_response.dragged_by(primary) {
                        let drag = move_drag_delta(
                            state.note_move_origin_pointer,
                            grid_response.interact_pointer_pos(),
                        );
                        let raw_ticks = (drag.x / state.zoom_x).round() as i64;
                        let raw_rows = (drag.y / state.key_height).round() as i32;
                        if let Some(grab) = state.note_move_grab_index {
                            let (dt, dr) = compute_note_move_deltas(
                                &state.note_move_orig,
                                grab,
                                raw_ticks,
                                raw_rows,
                            );
                            state.note_move_preview_delta_ticks = dt;
                            state.note_move_preview_delta_rows = dr;
                            ui.ctx().request_repaint();
                        }
                    }
                    // Commit en drag_stopped; fallback si se soltó fuera del área.
                    if grid_response.drag_stopped_by(primary)
                        || !ui.input(|i| i.pointer.primary_down())
                    {
                        commit_note_move = true;
                    }
                } else if grid_response.drag_started_by(primary) && !note_edge_active {
                    if let Some(pos) = grid_response.interact_pointer_pos() {
                        if let Some(grab) = hit_test_note_body(
                            &state.notes,
                            pos,
                            grid_rect,
                            state.zoom_x,
                            state.key_height,
                        ) {
                            let shift = ui.input(|i| i.modifiers.shift);
                            select_note_set(&mut state.selected_notes, grab, shift);
                            let orig: Vec<(usize, u64, u8)> = state
                                .selected_notes
                                .iter()
                                .filter_map(|&j| {
                                    state.notes.get(j).map(|n| (j, n.start_tick, n.pitch))
                                })
                                .collect();
                            if !orig.is_empty() {
                                state.note_move_active = true;
                                state.note_move_grab_index = Some(grab);
                                state.note_move_orig = orig;
                                state.note_move_preview_delta_ticks = 0;
                                state.note_move_preview_delta_rows = 0;
                                // Origen del delta acumulado: donde empezó el press
                                // (no el frame actual, que ya puede haberse movido).
                                state.note_move_origin_pointer = ui
                                    .input(|i| i.pointer.press_origin())
                                    .or(Some(pos));
                                note_body_active = true;
                                just_started_move = true;
                                ui.ctx().request_repaint();
                            }
                        }
                    }
                }

                // Sembrar el preview en el mismo frame del drag_started
                // (el press ya puede haberse movido un poco hasta superar el threshold).
                if just_started_move && state.note_move_active {
                    let drag = move_drag_delta(
                        state.note_move_origin_pointer,
                        grid_response.interact_pointer_pos(),
                    );
                    let raw_ticks = (drag.x / state.zoom_x).round() as i64;
                    let raw_rows = (drag.y / state.key_height).round() as i32;
                    if let Some(grab) = state.note_move_grab_index {
                        let (dt, dr) = compute_note_move_deltas(
                            &state.note_move_orig,
                            grab,
                            raw_ticks,
                            raw_rows,
                        );
                        state.note_move_preview_delta_ticks = dt;
                        state.note_move_preview_delta_rows = dr;
                    }
                }

                // Aplicar el move confirmado al soltar el puntero.
                if commit_note_move && state.note_move_active {
                    let delta_ticks = state.note_move_preview_delta_ticks;
                    let delta_rows = state.note_move_preview_delta_rows;
                    let orig = std::mem::take(&mut state.note_move_orig);
                    for (idx, orig_start, orig_pitch) in orig {
                        if let Some(n) = state.notes.get_mut(idx) {
                            let (ns, np) =
                                apply_move_delta(orig_start, orig_pitch, delta_ticks, delta_rows);
                            n.start_tick = ns;
                            n.pitch = np;
                        }
                    }
                    state.note_move_active = false;
                    state.note_move_grab_index = None;
                    state.note_move_preview_delta_ticks = 0;
                    state.note_move_preview_delta_rows = 0;
                    state.note_move_origin_pointer = None;
                    note_body_active = true;
                    ui.ctx().request_repaint();
                }

                if state.note_move_active {
                    note_body_active = true;
                    ui.output_mut(|o| o.cursor_icon = CursorIcon::Move);
                }

                // --- MARQUEE: selección de notas con click derecho + drag ---
                // Distinto de la Time Selection (Shift+drag en la regla), que
                // sirve para loopear. Este sirve para copiar/duplicar notas.
                {
                    let secondary = PointerButton::Secondary;
                    if grid_response.drag_started_by(secondary)
                        && !note_edge_active
                        && !note_body_active
                        && !state.note_move_active
                    {
                        if let Some(pos) = grid_response.interact_pointer_pos() {
                            state.note_marquee_active = true;
                            state.note_marquee_start = Some(pos);
                            state.note_marquee_current = Some(pos);
                            state.note_marquee_additive =
                                ui.input(|i| i.modifiers.shift);
                            if !state.note_marquee_additive {
                                state.selected_notes.clear();
                            }
                            ui.ctx().request_repaint();
                        }
                    } else if state.note_marquee_active
                        && grid_response.dragged_by(secondary)
                    {
                        state.note_marquee_current =
                            grid_response.interact_pointer_pos();
                        ui.ctx().request_repaint();
                    } else if state.note_marquee_active
                        && (grid_response.drag_stopped_by(secondary)
                            || !ui.input(|i| i.pointer.secondary_down()))
                    {
                        // Confirmar al soltar el botón derecho.
                        if let (Some(start), Some(current)) =
                            (state.note_marquee_start, state.note_marquee_current)
                        {
                            let marquee = Rect::from_two_pos(start, current);
                            if marquee.width() > 2.0 || marquee.height() > 2.0 {
                                let found = notes_in_marquee(
                                    &state.notes,
                                    marquee,
                                    grid_rect,
                                    state.zoom_x,
                                    state.key_height,
                                );
                                if state.note_marquee_additive {
                                    state.selected_notes.extend(found);
                                } else {
                                    state.selected_notes = found;
                                }
                            }
                        }
                        state.note_marquee_active = false;
                        state.note_marquee_start = None;
                        state.note_marquee_current = None;
                        state.note_marquee_additive = false;
                        ui.ctx().request_repaint();
                    }
                }

                // Dibujar el marquee encima de las notas (mismo frame del drag).
                if state.note_marquee_active && ui.is_rect_visible(grid_rect) {
                    if let (Some(start), Some(current)) =
                        (state.note_marquee_start, state.note_marquee_current)
                    {
                        let marquee = Rect::from_two_pos(start, current);
                        let p = ui.painter_at(grid_rect);
                        p.rect_filled(
                            marquee,
                            0.0,
                            Color32::from_rgba_unmultiplied(255, 200, 0, 35),
                        );
                        p.rect_stroke(
                            marquee,
                            0.0,
                            Stroke::new(1.5_f32, Color32::from_rgb(255, 200, 0)),
                        );
                    }
                }

                if let Some(hover_pos) = grid_response.hover_pos() {
                    let local_x = hover_pos.x - grid_rect.min.x;
                    let local_y = hover_pos.y - grid_rect.min.y;
                    let over_note_body = hit_test_note_body(
                        &state.notes,
                        hover_pos,
                        grid_rect,
                        state.zoom_x,
                        state.key_height,
                    )
                    .is_some();
                    let over_note_any = hit_test_note_full(
                        &state.notes,
                        hover_pos,
                        grid_rect,
                        state.zoom_x,
                        state.key_height,
                    )
                    .is_some();

                    let in_grid = local_x >= 0.0 && local_y >= 0.0;
                    let idle = in_grid
                        && !note_edge_active
                        && !note_body_active
                        && !state.note_move_active
                        && !state.note_marquee_active;

                    // --- Ghost + CREATE: solo en celda vacía (sin nota debajo) ---
                    if idle && !over_note_any {
                        let raw_tick = (local_x / state.zoom_x).max(0.0) as u64;
                        let quantized_tick = (raw_tick / QUANTIZE_TICKS) * QUANTIZE_TICKS;
                        let row = (local_y / state.key_height).max(0.0) as i32;
                        let pitch = (127 - row).clamp(0, 127) as u8;

                        if ui.is_rect_visible(grid_rect) {
                            let ghost_rect = Rect::from_min_size(
                                pos2(
                                    grid_rect.min.x + (quantized_tick as f32 * state.zoom_x),
                                    grid_rect.min.y + (row.clamp(0, 127) as f32 * state.key_height),
                                ),
                                vec2(QUANTIZE_TICKS as f32 * state.zoom_x, state.key_height - 1.0),
                            );
                            ui.painter_at(grid_rect).rect_filled(
                                ghost_rect,
                                2.0,
                                Color32::from_rgba_premultiplied(255, 140, 0, 60),
                            );
                        }

                        if grid_response.clicked() {
                            let already_exists = state.notes.iter().any(|n| {
                                n.pitch == pitch
                                    && quantized_tick >= n.start_tick
                                    && quantized_tick < n.start_tick + n.duration_ticks
                            });

                            if !already_exists {
                                state.notes.push(MidiNote {
                                    pitch,
                                    start_tick: quantized_tick,
                                    duration_ticks: QUANTIZE_TICKS,
                                    velocity: NOTE_INSERT_VELOCITY,
                                });
                                ui.ctx().request_repaint();

                                if state.mode == PianoRollMode::Drums {
                                    preview_drum_pad(tracks, selected_track_index, pitch, 1.0, audio_proxy);
                                }
                            }
                        }
                    }

                    // --- DELETE: click derecho SIN drag ---
                    // Funciona SOBRE la nota (cuerpo o extremo) y también en
                    // celda vacía (limpia la selección). Por eso vive FUERA del
                    // guard `!over_note_body`: antes, right-click sobre una nota
                    // nunca entraba al bloque y el borrado no ejecutaba.
                    // Tampoco depende de `!note_edge_active`: los extremos usan
                    // Sense::drag con Primary; Secondary debe poder borrar.
                    // (Con drag se crea el marquee; egui no dispara
                    // secondary_clicked si hubo un drag decidedly.)
                    if in_grid
                        && !note_body_active
                        && !state.note_move_active
                        && !state.note_marquee_active
                        && grid_response.secondary_clicked()
                    {
                        let hit_pos = grid_response
                            .interact_pointer_pos()
                            .or(grid_response.hover_pos());
                        if let Some(pos) = hit_pos {
                            if let Some(idx) = hit_test_note_full(
                                &state.notes,
                                pos,
                                grid_rect,
                                state.zoom_x,
                                state.key_height,
                            ) {
                                state.notes.remove(idx);
                                // Reajustar índices de la selección de notas.
                                state.selected_notes = state
                                    .selected_notes
                                    .iter()
                                    .filter_map(|&i| {
                                        if i == idx {
                                            None
                                        } else if i > idx {
                                            Some(i - 1)
                                        } else {
                                            Some(i)
                                        }
                                    })
                                    .collect();
                                ui.ctx().request_repaint();
                            } else {
                                // Click derecho en zona vacía: limpiar solo
                                // la selección de notas (no la Time Selection).
                                if !state.selected_notes.is_empty() {
                                    state.clear_note_selection();
                                    ui.ctx().request_repaint();
                                }
                            }
                        }
                    }

                    // --- SELECT: click izquierdo SOBRE el cuerpo de una nota ---
                    // El create solo corre en celda vacía (`!over_note_any`), así
                    // que acá no compite: si hay cuerpo, seleccionamos.
                    if idle && grid_response.clicked() && over_note_body {
                        let hit_pos = grid_response
                            .interact_pointer_pos()
                            .or(grid_response.hover_pos());
                        if let Some(pos) = hit_pos {
                            if let Some(idx) = hit_test_note_body(
                                &state.notes,
                                pos,
                                grid_rect,
                                state.zoom_x,
                                state.key_height,
                            ) {
                                let shift = ui.input(|i| i.modifiers.shift);
                                select_note_set(&mut state.selected_notes, idx, shift);
                                ui.ctx().request_repaint();
                            }
                        }
                    }
                }

                // Línea de Playhead
                let playhead_x = grid_rect.min.x + (state.playhead_tick as f32 * state.zoom_x);
                if ui.is_rect_visible(grid_rect) {
                    let grid_p = ui.painter_at(grid_rect);
                    grid_p.line_segment(
                        [pos2(playhead_x, grid_rect.min.y), pos2(playhead_x, grid_rect.max.y)],
                        Stroke::new(2.0_f32, Color32::from_rgb(0, 200, 255)),
                    );
                }

                // Teclado lateral
                let sidebar_clicked_pitch = draw_sidebar(ui, sidebar_rect, state);
                if let Some(clicked_pitch) = sidebar_clicked_pitch {
                    preview_drum_pad(tracks, selected_track_index, clicked_pitch, 1.0, audio_proxy);
                }

                // --- DIBUJAR LA REGLA Y LA ESQUINA POR ENCIMA (CAPA SUPERIOR) ---
                if ui.is_rect_visible(corner_rect) {
                    let p = ui.painter_at(corner_rect);
                    p.rect_filled(corner_rect, 0.0, Color32::from_rgb(20, 20, 24));
                    p.line_segment([corner_rect.left_bottom(), corner_rect.right_bottom()], Stroke::new(1.0_f32, Color32::from_gray(50)));
                    p.line_segment([corner_rect.right_top(), corner_rect.right_bottom()], Stroke::new(1.0_f32, Color32::from_gray(50)));
                }

                if ui.is_rect_visible(ruler_rect) {
                    let ruler_painter = ui.painter_at(ruler_rect);
                    draw_bar_ruler(&ruler_painter, ruler_rect, state.zoom_x, total_ticks);

                    // Dibujar Time Selection en la regla
                    let (render_active, render_start, render_end) = if state.selection_dragging && state.selection_preview_active {
                        (true, state.selection_preview_start_ticks, state.selection_preview_end_ticks)
                    } else {
                        (state.selection_active, state.selection_start_tick, state.selection_end_tick)
                    };

                    if render_active && render_end > render_start {
                        let start_x = ruler_rect.min.x + (render_start as f32 * state.zoom_x);
                        let end_x = ruler_rect.min.x + (render_end as f32 * state.zoom_x);
                        let (min_x, max_x) = if start_x <= end_x { (start_x, end_x) } else { (end_x, start_x) };

                        let selection_rect = Rect::from_min_max(
                            pos2(min_x, ruler_rect.min.y),
                            pos2(max_x, ruler_rect.max.y),
                        );

                        // Fondo translúcido en la regla
                        ruler_painter.rect_filled(
                            selection_rect,
                            0.0,
                            Color32::from_rgba_unmultiplied(100, 200, 255, 60),
                        );

                        // Corchetes de selección en la regla
                        let bracket_color = Color32::LIGHT_BLUE;
                        let bracket_stroke = Stroke::new(2.0_f32, bracket_color);
                        let tick_len = 5.0_f32;
                        let top_y = ruler_rect.min.y + 1.0;
                        let bottom_y = ruler_rect.max.y - 1.0;

                        // `[` izquierdo
                        ruler_painter.line_segment(
                            [pos2(min_x, top_y), pos2(min_x, bottom_y)],
                            bracket_stroke,
                        );
                        ruler_painter.line_segment(
                            [pos2(min_x, top_y), pos2(min_x + tick_len, top_y)],
                            bracket_stroke,
                        );
                        ruler_painter.line_segment(
                            [pos2(min_x, bottom_y), pos2(min_x + tick_len, bottom_y)],
                            bracket_stroke,
                        );

                        // `]` derecho
                        ruler_painter.line_segment(
                            [pos2(max_x, top_y), pos2(max_x, bottom_y)],
                            bracket_stroke,
                        );
                        ruler_painter.line_segment(
                            [pos2(max_x, top_y), pos2(max_x - tick_len, top_y)],
                            bracket_stroke,
                        );
                        ruler_painter.line_segment(
                            [pos2(max_x, bottom_y), pos2(max_x - tick_len, bottom_y)],
                            bracket_stroke,
                        );
                    }

                    // Indicador Playhead en la regla
                    let ruler_playhead_x = ruler_rect.min.x + (state.playhead_tick as f32 * state.zoom_x);
                    let head_size = 6.0_f32;
                    let head_triangle = vec![
                        pos2(ruler_playhead_x - head_size, ruler_rect.min.y),
                        pos2(ruler_playhead_x + head_size, ruler_rect.min.y),
                        pos2(ruler_playhead_x, ruler_rect.max.y),
                    ];
                    ruler_painter.add(Shape::convex_polygon(
                        head_triangle,
                        Color32::from_rgb(0, 200, 255),
                        Stroke::NONE,
                    ));
                }

                let ruler_response = ui.interact(
                    ruler_rect,
                    ui.id().with("piano_roll_ruler_interaction"),
                    Sense::click_and_drag(),
                );

                let shift = ui.input(|i| i.modifiers.shift);
                let pointer_pos = ruler_response.interact_pointer_pos();

                if let Some(pos) = pointer_pos {
                    let local_x = pos.x - ruler_rect.min.x;
                    let raw_tick = (local_x / state.zoom_x).max(0.0) as u64;
                    let tick = (raw_tick / QUANTIZE_TICKS) * QUANTIZE_TICKS;

                    if ruler_response.drag_started() && shift {
                        state.selection_dragging = true;
                        state.selection_preview_active = true;
                        state.selection_preview_start_ticks = tick;
                        state.selection_preview_end_ticks = tick;
                        ui.ctx().request_repaint();
                    } else if ruler_response.dragged() && state.selection_dragging && shift {
                        state.selection_preview_end_ticks = tick;
                        ui.ctx().request_repaint();
                    } else if ruler_response.drag_stopped() && state.selection_dragging {
                        let start = state.selection_preview_start_ticks.min(state.selection_preview_end_ticks);
                        let end = state.selection_preview_start_ticks.max(state.selection_preview_end_ticks);
                        if end > start {
                            state.selection_start_tick = start;
                            state.selection_end_tick = end;
                            state.selection_active = true;
                        } else {
                            state.selection_active = false;
                        }
                        state.selection_dragging = false;
                        state.selection_preview_active = false;
                        state.selection_preview_start_ticks = 0;
                        state.selection_preview_end_ticks = 0;
                        ui.ctx().request_repaint();
                    } else if ruler_response.clicked() && !shift {
                        state.playhead_tick = tick;
                        state.loop_start_instant = None;
                        ui.ctx().request_repaint();
                    }
                }

            });
    });
}

/// Dispara las notas cruzadas por el playhead.
///
/// Se llama cada frame desde el app **independientemente de si el panel del
/// Piano Roll está visible**. Antes este código vivía dentro de `show()`, por
/// lo que minimizar el panel cortaba el único productor de eventos de nota
/// y los clips MIDI dejaban de sonar.
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

/// `true` si el playhead cruzó `start_tick` en este frame.
///
/// - Avance normal: `prev < start <= current` dentro de la ventana de 240 ticks.
/// - Wrap de loop o seek hacia atrás: `prev > current` y el destino cae en la ventana.
///
/// Usa `<` (no `<=`): si `prev == start`, el frame anterior ya disparó la nota;
/// con `<=` se re-dispararía dentro de la ventana y sonaría duplicada.
fn note_just_crossed(prev_tick: u64, current_tick: u64, start_tick: u64) -> bool {
    (prev_tick < start_tick || prev_tick > current_tick)
        && current_tick >= start_tick
        && current_tick < start_tick + 240
}

fn draw_bar_ruler(painter: &Painter, ruler_rect: Rect, zoom_x: f32, total_ticks: u64) {
    painter.rect_filled(ruler_rect, 0.0, Color32::from_rgb(28, 28, 32));
    painter.line_segment(
        [ruler_rect.left_bottom(), ruler_rect.right_bottom()],
        Stroke::new(1.0_f32, Color32::from_gray(60)),
    );

    let bar_step_px = TICKS_PER_BAR as f32 * zoom_x;
    let mut tick = 0u64;
    let mut bar_number = 1;

    while tick <= total_ticks {
        let x = ruler_rect.min.x + (tick as f32 * zoom_x);

        if x > ruler_rect.max.x {
            break;
        }

        if x >= ruler_rect.min.x {
            painter.line_segment(
                [pos2(x, ruler_rect.min.y + 4.0), pos2(x, ruler_rect.max.y)],
                Stroke::new(1.5_f32, Color32::from_gray(140)),
            );

            painter.text(
                pos2(x + 5.0, ruler_rect.min.y + 3.0),
                Align2::LEFT_TOP,
                bar_number.to_string(),
                FontId::proportional(11.0),
                Color32::from_gray(210),
            );

            if bar_step_px > 30.0 {
                for b in 1..4 {
                    let beat_tick = tick + (b * TICKS_PER_BEAT);
                    let beat_x = ruler_rect.min.x + (beat_tick as f32 * zoom_x);

                    if beat_x >= ruler_rect.min.x && beat_x <= ruler_rect.max.x {
                        painter.line_segment(
                            [pos2(beat_x, ruler_rect.min.y + 14.0), pos2(beat_x, ruler_rect.max.y)],
                            Stroke::new(0.8_f32, Color32::from_gray(80)),
                        );
                    }
                }
            }
        }

        tick += TICKS_PER_BAR;
        bar_number += 1;
    }
}

fn note_rect(grid_rect: Rect, note: &MidiNote, zoom_x: f32, key_height: f32) -> Rect {
    let x = grid_rect.min.x + (note.start_tick as f32 * zoom_x);
    let y = grid_rect.min.y + ((127 - note.pitch) as f32 * key_height);
    Rect::from_min_size(
        pos2(x, y),
        vec2(
            (note.duration_ticks as f32 * zoom_x).max(4.0),
            key_height - 1.0,
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

fn draw_sidebar(
    ui: &mut Ui,
    sidebar_rect: Rect,
    state: &PianoRollState,
) -> Option<u8> {
    let mut clicked_pitch = None;

    if ui.is_rect_visible(sidebar_rect) {
        let painter = ui.painter_at(sidebar_rect);

        painter.rect_filled(sidebar_rect, 0.0, Color32::from_rgb(25, 25, 28));
        
        painter.line_segment(
            [sidebar_rect.right_top(), sidebar_rect.right_bottom()],
            Stroke::new(1.0_f32, Color32::from_gray(50)),
        );

        for row in 0..128 {
            let pitch = (127 - row) as u8;
            let row_y = sidebar_rect.min.y + (row as f32 * state.key_height);
            let row_rect = Rect::from_min_size(
                pos2(sidebar_rect.min.x, row_y),
                vec2(sidebar_rect.width(), state.key_height),
            );

            if !row_rect.intersects(sidebar_rect) {
                continue;
            }

            let is_black_key = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
            let bg_color = match state.mode {
                PianoRollMode::Keys => {
                    if is_black_key {
                        Color32::from_rgb(40, 40, 45)
                    } else {
                        Color32::from_rgb(220, 220, 225)
                    }
                }
                PianoRollMode::Drums => Color32::from_rgb(35, 35, 40),
            };

            let text_color = match state.mode {
                PianoRollMode::Keys => {
                    if is_black_key {
                        Color32::WHITE
                    } else {
                        Color32::BLACK
                    }
                }
                PianoRollMode::Drums => Color32::from_rgb(200, 200, 200),
            };

            painter.rect_filled(row_rect, 0.0, bg_color);
            painter.rect_stroke(
                row_rect,
                0.0,
                Stroke::new(0.5_f32, Color32::from_gray(60)),
            );

            let label = match state.mode {
                PianoRollMode::Keys => {
                    let note_names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
                    let octave = (pitch / 12) as i32 - 1;
                    format!("{}{}", note_names[(pitch % 12) as usize], octave)
                }
                PianoRollMode::Drums => {
                    if let Some(name) = state.drum_map.get(&pitch) {
                        format!("{} ({})", name, pitch)
                    } else {
                        format!("Pad {}", pitch)
                    }
                }
            };

            painter.text(
                pos2(row_rect.min.x + 6.0, row_rect.center().y),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional((state.key_height * 0.65).clamp(8.0, 12.0)),
                text_color,
            );

            let row_response = ui.interact(
                row_rect,
                ui.id().with(("sidebar_row", pitch)),
                Sense::click(),
            );

            if row_response.clicked() {
                clicked_pitch = Some(pitch);
            }
        }
    }

    clicked_pitch
}

fn draw_grid_background(
    painter: &Painter,
    grid_rect: Rect,
    key_height: f32,
    zoom_x: f32,
) {
    painter.rect_filled(grid_rect, 0.0, Color32::from_rgb(18, 18, 20));

    for row in 0..128 {
        let pitch = (127 - row) as u8;
        let y = grid_rect.min.y + (row as f32 * key_height);
        let is_black_key = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);

        if is_black_key {
            let row_rect = Rect::from_min_size(
                pos2(grid_rect.min.x, y),
                vec2(grid_rect.width(), key_height),
            );
            painter.rect_filled(row_rect, 0.0, Color32::from_rgba_premultiplied(0, 0, 0, 40));
        }

        painter.line_segment(
            [pos2(grid_rect.min.x, y), pos2(grid_rect.max.x, y)],
            Stroke::new(0.5_f32, Color32::from_gray(30)),
        );
    }

    let subdivision_ticks = QUANTIZE_TICKS; // 240 ticks (1/16)
    let step_px = subdivision_ticks as f32 * zoom_x;

    if step_px > 3.0 {
        let mut x = grid_rect.min.x;
        let mut tick = 0u64;

        while x < grid_rect.max.x {
            let is_bar = tick % TICKS_PER_BAR == 0;
            let is_beat = tick % TICKS_PER_BEAT == 0;

            let (stroke_width, color) = if is_bar {
                (1.5_f32, Color32::from_gray(80))
            } else if is_beat {
                (1.0_f32, Color32::from_gray(50))
            } else {
                (0.5_f32, Color32::from_gray(32))
            };

            painter.line_segment(
                [pos2(x, grid_rect.min.y), pos2(x, grid_rect.max.y)],
                Stroke::new(stroke_width, color),
            );

            tick += subdivision_ticks;
            x += step_px;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_move_delta, compute_note_move_deltas, duplicate_selected_notes,
        hit_test_note_body, hit_test_note_full, note_just_crossed, notes_in_marquee,
        quantize_tick, resize_note_left, resize_note_right, select_note_set, MidiNote,
        MIN_NOTE_DURATION_TICKS, QUANTIZE_TICKS,
    };
    use egui::{pos2, Rect};
    use std::collections::HashSet;

    #[test]
    fn crosses_note_start_from_below() {
        assert!(note_just_crossed(239, 240, 240));
        assert!(note_just_crossed(200, 280, 240));
    }

    #[test]
    fn does_not_refire_when_prev_lands_exactly_on_start() {
        // Frame anterior ya disparó al llegar a start; este frame no debe re-disparar.
        assert!(!note_just_crossed(240, 274, 240));
        assert!(!note_just_crossed(0, 34, 0));
    }

    #[test]
    fn fires_once_on_loop_wrap_to_note_at_start() {
        // Wrap: playhead salta del final del loop al inicio.
        assert!(note_just_crossed(3839, 0, 0));
        // Frame siguiente con prev exacto en start: no duplica.
        assert!(!note_just_crossed(0, 34, 0));
    }

    #[test]
    fn misses_note_further_than_window() {
        assert!(!note_just_crossed(0, 480, 240));
    }

    #[test]
    fn does_not_fire_when_stationary() {
        assert!(!note_just_crossed(240, 240, 240));
        assert!(!note_just_crossed(100, 100, 240));
    }

    #[test]
    fn resize_right_extends_note() {
        // Nota en start=0, dur=240; arrastrar el extremo derecho a tick 960.
        let (start, dur) = resize_note_right(0, 240, 960);
        assert_eq!(start, 0);
        assert_eq!(dur, 960);
    }

    #[test]
    fn resize_right_snaps_to_quantize() {
        // Pointer en 1000 se cuantiza hacia abajo a 960.
        let (start, dur) = resize_note_right(0, 240, 1000);
        assert_eq!(start, 0);
        assert_eq!(dur, 960);
        // Pointer en 959 → 720.
        let (_, dur) = resize_note_right(0, 240, 959);
        assert_eq!(dur, 720);
    }

    #[test]
    fn resize_right_enforces_min_duration() {
        // No se puede achicar por debajo de 1/16.
        let (start, dur) = resize_note_right(480, 960, 100);
        assert_eq!(start, 480);
        assert_eq!(dur, MIN_NOTE_DURATION_TICKS);
    }

    #[test]
    fn resize_left_extends_backwards() {
        // Nota start=480, dur=240 (end=720); arrastrar izq a tick 0.
        let (start, dur) = resize_note_left(480, 240, 0);
        assert_eq!(start, 0);
        assert_eq!(dur, 720);
    }

    #[test]
    fn resize_left_keeps_end_anchored() {
        // end = 480+960 = 1440; nuevo start cuantizado 720 → dur 720.
        let (start, dur) = resize_note_left(480, 960, 730);
        assert_eq!(start, 720);
        assert_eq!(dur, 1440 - 720);
    }

    #[test]
    fn resize_left_enforces_min_duration() {
        // end = 480+960 = 1440; min start = 1440-240 = 1200.
        // Pointer más allá del extremo derecho: se acota a min duration.
        let (start, dur) = resize_note_left(480, 960, 2000);
        assert_eq!(start, 1200);
        assert_eq!(dur, MIN_NOTE_DURATION_TICKS);
        // Pointer en 0 con nota start=240, dur=240 (end=480): extiende a 0.
        let (start, dur) = resize_note_left(240, 240, 0);
        assert_eq!(start, 0);
        assert_eq!(dur, 480);
    }

    #[test]
    fn resize_left_snaps_to_quantize() {
        // Pointer en 500 → 480.
        // end = 480+720 = 1200; start=480 → dur=720.
        let (start, dur) = resize_note_left(480, 720, 500);
        assert_eq!(start, 480);
        assert_eq!(dur, 720);
    }

    #[test]
    fn quantize_rounds_down() {
        assert_eq!(quantize_tick(0, QUANTIZE_TICKS), 0);
        assert_eq!(quantize_tick(239, QUANTIZE_TICKS), 0);
        assert_eq!(quantize_tick(240, QUANTIZE_TICKS), 240);
        assert_eq!(quantize_tick(479, QUANTIZE_TICKS), 240);
    }

    #[test]
    fn marquee_selects_intersecting_notes() {
        // grid en (0,0), key_height=16, zoom=1.0
        let grid = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(1000.0, 128.0 * 16.0));
        let notes = vec![
            // pitch 60 (C4) -> row = 127-60 = 67 -> y = 67*16 = 1072
            MidiNote { pitch: 60, start_tick: 0, duration_ticks: 240, velocity: 100 },
            // pitch 60, lejos en el tiempo
            MidiNote { pitch: 60, start_tick: 9600, duration_ticks: 240, velocity: 100 },
        ];
        // Marquee que solo cubre la primera nota (x 0..100, y toda la grilla)
        let marquee = Rect::from_min_size(pos2(0.0, 1000.0), egui::vec2(100.0, 100.0));
        let sel = notes_in_marquee(&notes, marquee, grid, 1.0, 16.0);
        assert!(sel.contains(&0));
        assert!(!sel.contains(&1));
    }

    #[test]
    fn marquee_empty_when_no_intersection() {
        let grid = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(1000.0, 128.0 * 16.0));
        let notes = vec![MidiNote { pitch: 60, start_tick: 0, duration_ticks: 240, velocity: 100 }];
        // Marquee arriba del todo (pitch alto, fuera de la nota)
        let marquee = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(50.0, 50.0));
        let sel = notes_in_marquee(&notes, marquee, grid, 1.0, 16.0);
        assert!(sel.is_empty());
    }

    #[test]
    fn duplicate_places_block_after_selection() {
        let mut notes = vec![
            MidiNote { pitch: 60, start_tick: 0, duration_ticks: 240, velocity: 100 },
            MidiNote { pitch: 62, start_tick: 240, duration_ticks: 240, velocity: 100 },
            MidiNote { pitch: 64, start_tick: 960, duration_ticks: 240, velocity: 100 }, // no seleccionada
        ];
        let mut selected = HashSet::new();
        selected.insert(0);
        selected.insert(1);

        let new_idx = duplicate_selected_notes(&mut notes, &selected);
        assert_eq!(notes.len(), 5);
        assert_eq!(new_idx.len(), 2);

        // Bloque original: 0..480 -> duration_block = 480
        // Duplicados en 480 y 720
        let d0 = &notes[new_idx[0]];
        let d1 = &notes[new_idx[1]];
        assert_eq!(d0.start_tick, 480);
        assert_eq!(d1.start_tick, 720);
        assert_eq!(d0.pitch, 60);
        assert_eq!(d1.pitch, 62);
        // La nota no seleccionada no se duplica
        assert_eq!(notes[2].start_tick, 960);
    }

    #[test]
    fn duplicate_empty_selection_is_noop() {
        let mut notes = vec![MidiNote { pitch: 60, start_tick: 0, duration_ticks: 240, velocity: 100 }];
        let selected = HashSet::new();
        let new_idx = duplicate_selected_notes(&mut notes, &selected);
        assert!(new_idx.is_empty());
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn apply_move_delta_horizontal_and_vertical() {
        let (start, pitch) = apply_move_delta(240, 60, 480, 2);
        assert_eq!(start, 720);
        assert_eq!(pitch, 58);
        let (start, pitch) = apply_move_delta(100, 60, -500, -2);
        assert_eq!(start, 0);
        assert_eq!(pitch, 62);
        let (_, pitch) = apply_move_delta(0, 120, 0, -50);
        assert_eq!(pitch, 127);
    }

    #[test]
    fn move_delta_snaps_grabbed_note_to_grid() {
        let orig = vec![(0, 480, 60), (1, 720, 62)];
        let (dt, dr) = compute_note_move_deltas(&orig, 0, 300, 0);
        assert_eq!(dt, 240);
        assert_eq!(dr, 0);
        let (s1, p1) = apply_move_delta(720, 62, dt, dr);
        assert_eq!(s1, 960);
        assert_eq!(p1, 62);
    }

    #[test]
    fn move_delta_does_not_push_notes_before_zero() {
        let orig = vec![(0, 240, 60)];
        let (dt, _) = compute_note_move_deltas(&orig, 0, -10_000, 0);
        assert_eq!(dt, -240);
        let (start, _) = apply_move_delta(240, 60, dt, 0);
        assert_eq!(start, 0);
    }

    #[test]
    fn move_delta_clamps_pitch_rows() {
        let orig = vec![(0, 0, 120)];
        let (_, dr) = compute_note_move_deltas(&orig, 0, 0, -50);
        assert_eq!(dr, -7);
        let (_, pitch) = apply_move_delta(0, 120, 0, dr);
        assert_eq!(pitch, 127);

        let orig = vec![(0, 0, 10)];
        let (_, dr) = compute_note_move_deltas(&orig, 0, 0, 99);
        assert_eq!(dr, 10);
        let (_, pitch) = apply_move_delta(0, 10, 0, dr);
        assert_eq!(pitch, 0);
    }

    #[test]
    fn select_note_set_replaces_or_adds() {
        let mut sel = HashSet::new();
        sel.insert(0);
        select_note_set(&mut sel, 2, false);
        assert_eq!(sel.len(), 1);
        assert!(sel.contains(&2));
        select_note_set(&mut sel, 5, true);
        assert_eq!(sel.len(), 2);
        assert!(sel.contains(&5));
        select_note_set(&mut sel, 2, false);
        assert_eq!(sel.len(), 2);
    }

    #[test]
    fn hit_test_body_finds_note_and_skips_edges() {
        let grid = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(1000.0, 128.0 * 16.0));
        let notes = vec![MidiNote {
            pitch: 60,
            start_tick: 0,
            duration_ticks: 960,
            velocity: 100,
        }];
        // pitch 60 → row = 67 → y = 67*16 = 1072; rect x=0..960 (zoom=1)
        // Centro del cuerpo: lejos de los extremos de 8px.
        let center = pos2(480.0, 1072.0 + 8.0);
        assert_eq!(
            hit_test_note_body(&notes, center, grid, 1.0, 16.0),
            Some(0)
        );
        // Sobre el extremo izquierdo (zona de resize): no es body.
        let edge = pos2(2.0, 1072.0 + 8.0);
        assert_eq!(hit_test_note_body(&notes, edge, grid, 1.0, 16.0), None);
        // Fuera de la nota.
        let outside = pos2(480.0, 0.0);
        assert_eq!(hit_test_note_body(&notes, outside, grid, 1.0, 16.0), None);
    }

    #[test]
    fn hit_test_full_includes_edges() {
        let grid = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(1000.0, 128.0 * 16.0));
        let notes = vec![MidiNote {
            pitch: 60,
            start_tick: 0,
            duration_ticks: 960,
            velocity: 100,
        }];
        // Centro del cuerpo.
        let center = pos2(480.0, 1072.0 + 8.0);
        assert_eq!(
            hit_test_note_full(&notes, center, grid, 1.0, 16.0),
            Some(0)
        );
        // Extremo izquierdo: body lo rechaza, full lo acepta (delete debe poder borrar ahí).
        let edge = pos2(2.0, 1072.0 + 8.0);
        assert_eq!(hit_test_note_body(&notes, edge, grid, 1.0, 16.0), None);
        assert_eq!(hit_test_note_full(&notes, edge, grid, 1.0, 16.0), Some(0));
        // Extremo derecho.
        let right_edge = pos2(958.0, 1072.0 + 8.0);
        assert_eq!(
            hit_test_note_full(&notes, right_edge, grid, 1.0, 16.0),
            Some(0)
        );
        // Fuera de la nota.
        let outside = pos2(480.0, 0.0);
        assert_eq!(hit_test_note_full(&notes, outside, grid, 1.0, 16.0), None);
    }
}