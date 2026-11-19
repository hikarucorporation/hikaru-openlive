// Hikaru OpenLive - Bitwig Studio 5.x style EDIT dropdown menu & EditHistory
// GNU AGPLv3
// crates/hikaru_gui/src/ui/edit_menu.rs
//
// Menú desplegable de [EDIT] pintado a mano para reproducir 1:1 el layout y
// comportamiento contextual de Bitwig Studio 5.x:
//
//   [↺] Undo [última acción]              Ctrl+Z        📌 (anclado por defecto)
//   [↻] Redo [siguiente acción]           Ctrl+Shift+Z  📌 (anclado por defecto)
//   ───────────────────────────────────────────────────────────────────────────
//   [✂] Cut Arranger clip / track region  Ctrl+X        📌
//   [❐] Copy Arranger clip / track region Ctrl+C        📌
//   [📋] Paste                            Ctrl+V        📌
//   [❐+] Duplicate                        Ctrl+D        📌 (anclado por defecto)
//   [⊗] Delete Arranger clip / track region Backspace   📌 (anclado por defecto)
//   ───────────────────────────────────────────────────────────────────────────
//   [☑] Select All                        Ctrl+A        📌
//   [❐] Deselect All                      Ctrl+Shift+A  📌
//   [◫] Convert to Time Selection                       📌

use std::collections::HashSet;

use egui::{
    pos2, vec2, Align2, Color32, Context, CursorIcon, Id, Order, Pos2, Rect, Sense, Stroke, Ui,
};

use crate::app::AppState;
use crate::ui::bitwig_kit::BW_ACCENT_ORANGE;
use crate::ui::fonts;
use crate::ui::icons::{self, VectorIcon};
use crate::views::playlist::{self, CueMarker, PlaylistClip, PlaylistState};

const MENU_W: f32 = 296.0;
const PAD_X: f32 = 8.0;
const PAD_Y: f32 = 4.0;
const ROW_H: f32 = 26.0;
const SEP_H: f32 = 11.0;

const MENU_BG: Color32 = Color32::from_rgb(0x2D, 0x2D, 0x2F);
const MENU_BORDER: Color32 = Color32::from_rgb(0x18, 0x18, 0x1A);
const SEP: Color32 = Color32::from_rgb(0x46, 0x46, 0x49);
const ROW_HOVER: Color32 = Color32::from_rgb(0xBE, 0xBE, 0xBE);
const TXT: Color32 = Color32::from_rgb(0xEA, 0xEA, 0xEA);
const TXT_DISABLED: Color32 = Color32::from_rgb(0x74, 0x74, 0x74);
const TXT_HINT: Color32 = Color32::from_rgb(0x98, 0x98, 0x98);
const TXT_ON_HOVER: Color32 = Color32::from_rgb(0x1A, 0x1A, 0x1A);
const PIN_IDLE: Color32 = Color32::from_rgb(0x9A, 0x9A, 0xA0);

const MAX_HISTORY: usize = 64;

const fn vi(d: &'static str) -> VectorIcon {
    VectorIcon { filled: false, d }
}

// Iconos 24x24 calibrados al menú EDIT de Bitwig Studio 5.x
const IC_UNDO: VectorIcon = vi(
    "M 8 8 L 4 12 L 8 16 M 4 12 C 6.5 8.5 11.5 7.5 15.5 9.5 C 18.2 10.8 19.8 13.4 20 16",
);
const IC_REDO: VectorIcon = vi(
    "M 16 8 L 20 12 L 16 16 M 20 12 C 17.5 8.5 12.5 7.5 8.5 9.5 C 5.8 10.8 4.2 13.4 4 16",
);
const IC_COPY: VectorIcon = vi(
    "M 9 9 L 19 9 L 19 20 L 9 20 Z M 15 9 L 15 5 L 5 5 L 5 16 L 9 16",
);
const IC_PASTE: VectorIcon = vi(
    "M 9 5 L 15 5 L 15 7.5 L 9 7.5 Z M 9 6 L 6 6 L 6 20 L 18 20 L 18 6 L 15 6",
);
const IC_SELECT_ALL: VectorIcon = vi(
    "M 5 5 L 19 5 L 19 19 L 5 19 Z M 8.5 12.5 L 11 15 L 15.5 9.5",
);
const IC_DESELECT_ALL: VectorIcon = vi(
    "M 4 4 L 15 4 L 15 15 L 4 15 Z M 9 15 L 9 19 L 19 19 L 19 9 L 15 9",
);
const IC_CONVERT_TIME_SEL: VectorIcon = vi(
    "M 4 5 L 20 5 L 20 19 L 4 19 Z M 9 5 L 9 19 M 4 9 L 20 9 M 12 13 L 17 13 M 12 16 L 16 16",
);
const IC_PIN: VectorIcon = vi(
    "M 15 4 L 20 9 M 16 5 L 11 10 L 8 10 L 14 16 L 14 13 L 19 8 M 11 13 L 5 19",
);

// =========================================================================
// HISTORIAL UNDO / REDO PARA EL ARRANGER / PLAYLIST
// =========================================================================

#[derive(Clone, Debug)]
struct PlaylistSnapshot {
    clips: Vec<(usize, PlaylistClip)>,
    selected_clips: Vec<usize>,
    cue_markers: Vec<CueMarker>,
    loop_start_ticks: u64,
    loop_end_ticks: u64,
    loop_region_active: bool,
    next_clip_id: usize,
}

impl PlaylistSnapshot {
    fn capture(s: &PlaylistState) -> Self {
        Self {
            clips: s.clips.clone(),
            selected_clips: s.selected_clips.clone(),
            cue_markers: s.cue_markers.clone(),
            loop_start_ticks: s.loop_start_ticks,
            loop_end_ticks: s.loop_end_ticks,
            loop_region_active: s.loop_region_active,
            next_clip_id: s.next_clip_id,
        }
    }

    fn apply_to(&self, s: &mut PlaylistState) {
        s.clips = self.clips.clone();
        s.selected_clips = self.selected_clips.clone();
        s.cue_markers = self.cue_markers.clone();
        s.loop_start_ticks = self.loop_start_ticks;
        s.loop_end_ticks = self.loop_end_ticks;
        s.loop_region_active = self.loop_region_active;
        s.next_clip_id = self.next_clip_id;
        s.needs_full_sync = true;
    }
}

#[derive(Clone, Debug)]
struct HistoryEntry {
    label: String,
    snapshot: PlaylistSnapshot,
}

#[derive(Clone, Debug, Default)]
pub struct EditHistory {
    undo_stack: Vec<HistoryEntry>,
    redo_stack: Vec<HistoryEntry>,
}

impl EditHistory {
    pub fn push_playlist(&mut self, label: impl Into<String>, playlist: &PlaylistState) {
        let entry = HistoryEntry {
            label: label.into(),
            snapshot: PlaylistSnapshot::capture(playlist),
        };
        if self.undo_stack.len() >= MAX_HISTORY {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(entry);
        self.redo_stack.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo_stack.last().map(|e| e.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo_stack.last().map(|e| e.label.as_str())
    }

    pub fn undo(&mut self, playlist: &mut PlaylistState) -> bool {
        let Some(entry) = self.undo_stack.pop() else {
            return false;
        };
        let redo_entry = HistoryEntry {
            label: entry.label,
            snapshot: PlaylistSnapshot::capture(playlist),
        };
        self.redo_stack.push(redo_entry);
        entry.snapshot.apply_to(playlist);
        true
    }

    pub fn redo(&mut self, playlist: &mut PlaylistState) -> bool {
        let Some(entry) = self.redo_stack.pop() else {
            return false;
        };
        let undo_entry = HistoryEntry {
            label: entry.label,
            snapshot: PlaylistSnapshot::capture(playlist),
        };
        self.undo_stack.push(undo_entry);
        entry.snapshot.apply_to(playlist);
        true
    }

    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }
}

// =========================================================================
// ACCIONES Y CONTEXTO DEL MENÚ EDIT
// =========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EditMenuAction {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Duplicate,
    Delete,
    SelectAll,
    DeselectAll,
    ConvertToTimeSelection,
}

pub const ALL_ACTIONS: [EditMenuAction; 10] = [
    EditMenuAction::Undo,
    EditMenuAction::Redo,
    EditMenuAction::Cut,
    EditMenuAction::Copy,
    EditMenuAction::Paste,
    EditMenuAction::Duplicate,
    EditMenuAction::Delete,
    EditMenuAction::SelectAll,
    EditMenuAction::DeselectAll,
    EditMenuAction::ConvertToTimeSelection,
];

const SECTION_HISTORY: [EditMenuAction; 2] = [EditMenuAction::Undo, EditMenuAction::Redo];
const SECTION_CLIPBOARD: [EditMenuAction; 5] = [
    EditMenuAction::Cut,
    EditMenuAction::Copy,
    EditMenuAction::Paste,
    EditMenuAction::Duplicate,
    EditMenuAction::Delete,
];
const SECTION_SELECTION: [EditMenuAction; 3] = [
    EditMenuAction::SelectAll,
    EditMenuAction::DeselectAll,
    EditMenuAction::ConvertToTimeSelection,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditTarget {
    ArrangerClip,
    Scene,
    TrackRegion,
}

#[derive(Clone, Copy, Debug)]
struct EditContext {
    target: EditTarget,
    can_undo: bool,
    can_redo: bool,
    has_time_region: bool,
    has_clipboard: bool,
}

impl EditContext {
    fn from_state(state: &AppState) -> Self {
        let has_selected_clips = !state.playlist_state.selected_clips.is_empty();
        let has_selected_scene = state.matrix_state.selected_scene.is_some();
        let has_time_region = state.playlist_state.loop_region_active
            && state.playlist_state.loop_end_ticks > state.playlist_state.loop_start_ticks;

        let target = if has_selected_clips {
            EditTarget::ArrangerClip
        } else if has_selected_scene {
            EditTarget::Scene
        } else {
            EditTarget::TrackRegion
        };

        let has_clipboard = match target {
            EditTarget::Scene => {
                state.matrix_clipboard.copied_scene.is_some()
                    || !state.playlist_state.clipboard.is_empty()
            }
            EditTarget::ArrangerClip | EditTarget::TrackRegion => {
                !state.playlist_state.clipboard.is_empty()
            }
        };

        Self {
            target,
            can_undo: state.edit_history.can_undo(),
            can_redo: state.edit_history.can_redo(),
            has_time_region,
            has_clipboard,
        }
    }
}

pub fn current_target(state: &AppState) -> EditTarget {
    EditContext::from_state(state).target
}

pub fn action_icon(action: EditMenuAction) -> VectorIcon {
    match action {
        EditMenuAction::Undo => IC_UNDO,
        EditMenuAction::Redo => IC_REDO,
        EditMenuAction::Cut => icons::CUT,
        EditMenuAction::Copy => IC_COPY,
        EditMenuAction::Paste => IC_PASTE,
        EditMenuAction::Duplicate => icons::DUPLICATE_PLUS,
        EditMenuAction::Delete => icons::CIRCLE_X,
        EditMenuAction::SelectAll => IC_SELECT_ALL,
        EditMenuAction::DeselectAll => IC_DESELECT_ALL,
        EditMenuAction::ConvertToTimeSelection => IC_CONVERT_TIME_SEL,
    }
}

pub fn action_shortcut(action: EditMenuAction) -> &'static str {
    match action {
        EditMenuAction::Undo => "Ctrl+Z",
        EditMenuAction::Redo => "Ctrl+Shift+Z",
        EditMenuAction::Cut => "Ctrl+X",
        EditMenuAction::Copy => "Ctrl+C",
        EditMenuAction::Paste => "Ctrl+V",
        EditMenuAction::Duplicate => "Ctrl+D",
        EditMenuAction::Delete => "Backspace",
        EditMenuAction::SelectAll => "Ctrl+A",
        EditMenuAction::DeselectAll => "Ctrl+Shift+A",
        EditMenuAction::ConvertToTimeSelection => "",
    }
}

pub fn action_label(state: &AppState, action: EditMenuAction) -> String {
    let ctx = EditContext::from_state(state);
    match action {
        EditMenuAction::Undo => match state.edit_history.undo_label() {
            Some(last) => format!("Undo {last}"),
            None => "Undo".to_string(),
        },
        EditMenuAction::Redo => match state.edit_history.redo_label() {
            Some(next) => format!("Redo {next}"),
            None => "Redo".to_string(),
        },
        EditMenuAction::Cut => match ctx.target {
            EditTarget::ArrangerClip => "Cut Arranger clip".to_string(),
            EditTarget::Scene => "Cut Scene".to_string(),
            EditTarget::TrackRegion => "Cut track region".to_string(),
        },
        EditMenuAction::Copy => match ctx.target {
            EditTarget::ArrangerClip => "Copy Arranger clip".to_string(),
            EditTarget::Scene => "Copy Scene".to_string(),
            EditTarget::TrackRegion => "Copy track region".to_string(),
        },
        EditMenuAction::Paste => "Paste".to_string(),
        EditMenuAction::Duplicate => "Duplicate".to_string(),
        EditMenuAction::Delete => match ctx.target {
            EditTarget::ArrangerClip => "Delete Arranger clip".to_string(),
            EditTarget::Scene => "Delete Scene".to_string(),
            EditTarget::TrackRegion => "Delete track region".to_string(),
        },
        EditMenuAction::SelectAll => "Select All".to_string(),
        EditMenuAction::DeselectAll => "Deselect All".to_string(),
        EditMenuAction::ConvertToTimeSelection => "Convert to Time Selection".to_string(),
    }
}

pub fn action_enabled(state: &AppState, action: EditMenuAction) -> bool {
    let ctx = EditContext::from_state(state);
    match action {
        EditMenuAction::Undo => ctx.can_undo,
        EditMenuAction::Redo => ctx.can_redo,
        EditMenuAction::Cut | EditMenuAction::Copy | EditMenuAction::Duplicate => {
            matches!(ctx.target, EditTarget::ArrangerClip | EditTarget::Scene)
                || ctx.has_time_region
        }
        EditMenuAction::Paste => ctx.has_clipboard,
        EditMenuAction::Delete => {
            matches!(ctx.target, EditTarget::ArrangerClip | EditTarget::Scene)
                || ctx.has_time_region
        }
        EditMenuAction::SelectAll | EditMenuAction::DeselectAll => true,
        EditMenuAction::ConvertToTimeSelection => ctx.target == EditTarget::ArrangerClip,
    }
}

// =========================================================================
// ESTADO DEL POPUP Y BOTONES ANCLADOS (DEFAULT BITWIG: UNDO, REDO, DUP, DEL)
// =========================================================================

pub fn default_pinned() -> HashSet<EditMenuAction> {
    HashSet::from([
        EditMenuAction::Undo,
        EditMenuAction::Redo,
        EditMenuAction::Duplicate,
        EditMenuAction::Delete,
    ])
}

pub fn is_open(ctx: &Context, key: Id) -> bool {
    ctx.data(|d| d.get_temp::<bool>(key.with("open")))
        .unwrap_or(false)
}

pub fn toggle(ctx: &Context, key: Id, pos: Pos2) {
    let currently_open = is_open(ctx, key);
    ctx.data_mut(|d| {
        d.insert_temp(key.with("open"), !currently_open);
        d.insert_temp(key.with("pos"), pos);
    });
}

pub fn close(ctx: &Context, key: Id) {
    ctx.data_mut(|d| d.insert_temp(key.with("open"), false));
}

fn load_pinned(ctx: &Context, key: Id) -> HashSet<EditMenuAction> {
    ctx.data(|d| d.get_temp::<HashSet<EditMenuAction>>(key.with("pinned")))
        .unwrap_or_else(default_pinned)
}

fn save_pinned(ctx: &Context, key: Id, pinned: HashSet<EditMenuAction>) {
    ctx.data_mut(|d| d.insert_temp(key.with("pinned"), pinned));
}

/// Devuelve los botones anclados a la barra de herramientas a la derecha de `EDIT`,
/// preservando el orden canónico del menú (por defecto: Undo, Redo, Duplicate, Delete).
pub fn pinned_actions(ctx: &Context, key: Id) -> Vec<EditMenuAction> {
    let pinned = load_pinned(ctx, key);
    ALL_ACTIONS
        .iter()
        .copied()
        .filter(|a| pinned.contains(a))
        .collect()
}

pub fn show(
    ctx: &Context,
    key: Id,
    anchor_rect: Rect,
    state: &AppState,
) -> Option<EditMenuAction> {
    if !is_open(ctx, key) {
        return None;
    }

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close(ctx, key);
        return None;
    }

    let raw_pos = ctx
        .data(|d| d.get_temp::<Pos2>(key.with("pos")))
        .unwrap_or_else(|| pos2(anchor_rect.min.x, anchor_rect.max.y + 2.0));

    let total_rows = SECTION_HISTORY.len() + SECTION_CLIPBOARD.len() + SECTION_SELECTION.len();
    let total_h = (total_rows as f32) * ROW_H + 2.0 * SEP_H + 2.0 * PAD_Y;
    let scr = ctx.screen_rect();
    let pos = pos2(
        raw_pos.x.min(scr.max.x - MENU_W - 4.0).max(scr.min.x + 2.0),
        raw_pos.y.min(scr.max.y - total_h - 4.0).max(scr.min.y + 2.0),
    );

    let mut pinned = load_pinned(ctx, key);
    let mut pinned_changed = false;
    let mut chosen: Option<EditMenuAction> = None;

    let area_resp = egui::Area::new(key.with("area"))
        .fixed_pos(pos)
        .order(Order::Foreground)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(MENU_W, total_h), Sense::click());
            let p = ui.painter();
            p.rect_filled(
                rect.translate(vec2(2.0, 4.0)),
                2.5,
                Color32::from_black_alpha(135),
            );
            p.rect_filled(rect, 2.5, MENU_BG);
            p.rect_stroke(
                rect,
                2.5,
                Stroke::new(1.0_f32, MENU_BORDER),
                egui::StrokeKind::Inside,
            );

            let mut y = rect.min.y + PAD_Y;
            let sections: [&[EditMenuAction]; 3] =
                [&SECTION_HISTORY, &SECTION_CLIPBOARD, &SECTION_SELECTION];

            for (sec_idx, section) in sections.iter().enumerate() {
                if sec_idx > 0 {
                    draw_separator(ui, rect, y);
                    y += SEP_H;
                }
                for &action in *section {
                    let label = action_label(state, action);
                    let shortcut = action_shortcut(action);
                    let icon = action_icon(action);
                    let enabled = action_enabled(state, action);
                    let is_pinned = pinned.contains(&action);

                    match draw_row(ui, key, rect, y, action, &label, shortcut, icon, enabled, is_pinned) {
                        RowClick::Action if enabled => chosen = Some(action),
                        RowClick::TogglePin => {
                            if !pinned.remove(&action) {
                                pinned.insert(action);
                            }
                            pinned_changed = true;
                        }
                        RowClick::Action | RowClick::None => {}
                    }
                    y += ROW_H;
                }
            }
        });

    if pinned_changed {
        save_pinned(ctx, key, pinned);
    }

    if chosen.is_some() {
        close(ctx, key);
        return chosen;
    }

    let clicked_outside = ctx.input(|i| {
        i.pointer.any_pressed()
            && i.pointer.interact_pos().is_some_and(|p| {
                !area_resp.response.rect.contains(p) && !anchor_rect.contains(p)
            })
    });
    if clicked_outside {
        close(ctx, key);
    }

    None
}

enum RowClick {
    None,
    Action,
    TogglePin,
}

fn draw_separator(ui: &mut Ui, menu_rect: Rect, y: f32) {
    let sep_y = y + SEP_H * 0.5;
    ui.painter().line_segment(
        [
            pos2(menu_rect.min.x + PAD_X, sep_y),
            pos2(menu_rect.max.x - PAD_X, sep_y),
        ],
        Stroke::new(1.0_f32, SEP),
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_row(
    ui: &mut Ui,
    key: Id,
    menu_rect: Rect,
    y: f32,
    action: EditMenuAction,
    label: &str,
    shortcut: &str,
    icon: VectorIcon,
    enabled: bool,
    is_pinned: bool,
) -> RowClick {
    let row_rect = Rect::from_min_size(pos2(menu_rect.min.x + 2.0, y), vec2(MENU_W - 4.0, ROW_H));
    let pin_rect = Rect::from_center_size(
        pos2(row_rect.max.x - 13.0, row_rect.center().y),
        vec2(18.0, ROW_H - 4.0),
    );
    let main_rect = Rect::from_min_max(row_rect.min, pos2(pin_rect.min.x - 2.0, row_rect.max.y));

    let pin_resp = ui.interact(pin_rect, key.with(("edit_pin", action)), Sense::click());
    let main_resp = ui.interact(main_rect, key.with(("edit_row", action)), Sense::click());

    let hovered_action = enabled && main_resp.hovered();
    let hovered_pin = pin_resp.hovered();
    let hovered_row = hovered_action || (enabled && hovered_pin);
    let p = ui.painter();

    if hovered_row {
        p.rect_filled(row_rect, 2.0, ROW_HOVER);
    }
    if hovered_action || hovered_pin {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    let fg = if !enabled {
        TXT_DISABLED
    } else if hovered_row {
        TXT_ON_HOVER
    } else {
        TXT
    };
    let hint_fg = if !enabled {
        TXT_DISABLED
    } else if hovered_row {
        TXT_ON_HOVER
    } else {
        TXT_HINT
    };

    // Icono izquierdo
    let ic_rect = Rect::from_center_size(
        pos2(row_rect.min.x + 16.0, row_rect.center().y),
        vec2(14.0, 14.0),
    );
    icons::paint_icon(p, ic_rect, icon, fg, 1.3_f32);

    // Etiqueta principal
    p.text(
        pos2(row_rect.min.x + 32.0, row_rect.center().y),
        Align2::LEFT_CENTER,
        label,
        fonts::regular(11.5),
        fg,
    );

    // Atajo de teclado (a la izquierda del pin)
    if !shortcut.is_empty() {
        p.text(
            pos2(pin_rect.min.x - 6.0, row_rect.center().y),
            Align2::RIGHT_CENTER,
            shortcut,
            fonts::regular(10.5),
            hint_fg,
        );
    }

    // Icono Pin derecho (siempre clickeable, igual que en Bitwig Studio)
    let pin_col = if is_pinned {
        BW_ACCENT_ORANGE
    } else if hovered_row {
        TXT_ON_HOVER
    } else if hovered_pin {
        TXT
    } else {
        PIN_IDLE
    };
    let pin_ic_rect = Rect::from_center_size(pin_rect.center(), vec2(11.0, 11.0));
    icons::paint_icon(p, pin_ic_rect, IC_PIN, pin_col, 1.2_f32);

    if pin_resp.clicked() {
        RowClick::TogglePin
    } else if main_resp.clicked() && enabled {
        RowClick::Action
    } else {
        RowClick::None
    }
}

// =========================================================================
// EJECUCIÓN DE ACCIONES DEL MENÚ EDIT
// =========================================================================

fn clips_in_time_region(s: &PlaylistState) -> Vec<usize> {
    if !s.loop_region_active || s.loop_end_ticks <= s.loop_start_ticks {
        return Vec::new();
    }
    let start = s.loop_start_ticks;
    let end = s.loop_end_ticks;
    s.clips
        .iter()
        .filter(|(_, c)| {
            let c_end = c.start_tick.saturating_add(c.duration_ticks);
            c.start_tick < end && c_end > start
        })
        .map(|(_, c)| c.id)
        .collect()
}

pub fn execute_action(state: &mut AppState, action: EditMenuAction) -> bool {
    if !action_enabled(state, action) {
        return false;
    }

    let target = current_target(state);
    match action {
        EditMenuAction::Undo => {
            let ok = state.edit_history.undo(&mut state.playlist_state);
            if ok {
                state.project_dirty = true;
            }
            ok
        }
        EditMenuAction::Redo => {
            let ok = state.edit_history.redo(&mut state.playlist_state);
            if ok {
                state.project_dirty = true;
            }
            ok
        }
        EditMenuAction::Cut => match target {
            EditTarget::ArrangerClip => {
                state
                    .edit_history
                    .push_playlist("Cut Arranger clip", &state.playlist_state);
                let count = playlist::cut_selected_clips(&mut state.playlist_state);
                if count > 0 {
                    state.project_dirty = true;
                }
                count > 0
            }
            EditTarget::Scene => {
                if let Some(idx) = state.matrix_state.selected_scene {
                    let ok = state.cut_scene_at(idx);
                    if ok {
                        state.project_dirty = true;
                    }
                    ok
                } else {
                    false
                }
            }
            EditTarget::TrackRegion => {
                let ids = clips_in_time_region(&state.playlist_state);
                if ids.is_empty() {
                    return false;
                }
                state
                    .edit_history
                    .push_playlist("Cut track region", &state.playlist_state);
                state.playlist_state.selected_clips = ids;
                let count = playlist::cut_selected_clips(&mut state.playlist_state);
                if count > 0 {
                    state.project_dirty = true;
                }
                count > 0
            }
        },
        EditMenuAction::Copy => match target {
            EditTarget::ArrangerClip => {
                playlist::copy_selected_clips(&mut state.playlist_state) > 0
            }
            EditTarget::Scene => state
                .matrix_state
                .selected_scene
                .is_some_and(|idx| state.copy_scene_at(idx)),
            EditTarget::TrackRegion => {
                let ids = clips_in_time_region(&state.playlist_state);
                if ids.is_empty() {
                    return false;
                }
                state.playlist_state.clipboard.clear();
                for (t, c) in &state.playlist_state.clips {
                    if ids.contains(&c.id) {
                        state.playlist_state.clipboard.push((*t, c.clone()));
                    }
                }
                !state.playlist_state.clipboard.is_empty()
            }
        },
        EditMenuAction::Paste => {
            if target == EditTarget::Scene && state.matrix_clipboard.copied_scene.is_some() {
                if let Some(idx) = state.matrix_state.selected_scene {
                    let ok = state.paste_scene_at(idx);
                    if ok {
                        state.project_dirty = true;
                    }
                    return ok;
                }
            }
            if state.playlist_state.clipboard.is_empty() {
                return false;
            }
            state
                .edit_history
                .push_playlist("Paste", &state.playlist_state);
            let target_tick = state.playlist_state.paste_tick();
            let pasted = playlist::paste_clips(
                &mut state.playlist_state,
                target_tick,
                state.selected_track_index.max(1),
            );
            if !pasted.is_empty() {
                state.project_dirty = true;
                true
            } else {
                false
            }
        }
        EditMenuAction::Duplicate => match target {
            EditTarget::ArrangerClip => {
                state
                    .edit_history
                    .push_playlist("Duplicate", &state.playlist_state);
                let (step, _, _) = playlist::adaptive_grid_step(
                    state.playlist_state.ppqn,
                    state.transport.beats_per_bar,
                    state.playlist_state.zoom_x,
                );
                let created =
                    playlist::duplicate_selected_clips(&mut state.playlist_state, step);
                if !created.is_empty() {
                    state.project_dirty = true;
                    true
                } else {
                    false
                }
            }
            EditTarget::Scene => {
                if let Some(idx) = state.matrix_state.selected_scene {
                    let ok = state.duplicate_scene_at(idx).is_some();
                    if ok {
                        state.project_dirty = true;
                    }
                    ok
                } else {
                    false
                }
            }
            EditTarget::TrackRegion => {
                let ids = clips_in_time_region(&state.playlist_state);
                if ids.is_empty() {
                    return false;
                }
                state
                    .edit_history
                    .push_playlist("Duplicate", &state.playlist_state);
                state.playlist_state.selected_clips = ids;
                let (step, _, _) = playlist::adaptive_grid_step(
                    state.playlist_state.ppqn,
                    state.transport.beats_per_bar,
                    state.playlist_state.zoom_x,
                );
                let created =
                    playlist::duplicate_selected_clips(&mut state.playlist_state, step);
                if !created.is_empty() {
                    state.project_dirty = true;
                    true
                } else {
                    false
                }
            }
        },
        EditMenuAction::Delete => match target {
            EditTarget::ArrangerClip => {
                state
                    .edit_history
                    .push_playlist("Delete Arranger clip", &state.playlist_state);
                let selected = state.playlist_state.selected_clips.clone();
                let before = state.playlist_state.clips.len();
                state
                    .playlist_state
                    .clips
                    .retain(|(_, c)| !selected.contains(&c.id));
                state.playlist_state.selected_clips.clear();
                state.playlist_state.needs_full_sync = true;
                if state.playlist_state.clips.len() < before {
                    state.project_dirty = true;
                    true
                } else {
                    false
                }
            }
            EditTarget::Scene => {
                if let Some(idx) = state.matrix_state.selected_scene {
                    let ok = state.remove_scene_at(idx);
                    if ok {
                        state.project_dirty = true;
                    }
                    ok
                } else {
                    false
                }
            }
            EditTarget::TrackRegion => {
                let ids = clips_in_time_region(&state.playlist_state);
                if ids.is_empty() {
                    return false;
                }
                state
                    .edit_history
                    .push_playlist("Delete track region", &state.playlist_state);
                state
                    .playlist_state
                    .clips
                    .retain(|(_, c)| !ids.contains(&c.id));
                state
                    .playlist_state
                    .selected_clips
                    .retain(|id| !ids.contains(id));
                state.playlist_state.needs_full_sync = true;
                state.project_dirty = true;
                true
            }
        },
        EditMenuAction::SelectAll => {
            playlist::select_all_clips(&mut state.playlist_state);
            true
        }
        EditMenuAction::DeselectAll => {
            state.playlist_state.selected_clips.clear();
            state.matrix_state.selected_scene = None;
            true
        }
        EditMenuAction::ConvertToTimeSelection => {
            let selected = &state.playlist_state.selected_clips;
            let mut min_start = u64::MAX;
            let mut max_end = 0u64;
            for (_, clip) in &state.playlist_state.clips {
                if selected.contains(&clip.id) {
                    min_start = min_start.min(clip.start_tick);
                    max_end = max_end.max(clip.start_tick.saturating_add(clip.duration_ticks));
                }
            }
            if min_start < max_end {
                state
                    .edit_history
                    .push_playlist("Convert to Time Selection", &state.playlist_state);
                state.playlist_state.loop_start_ticks = min_start;
                state.playlist_state.loop_end_ticks = max_end;
                state.playlist_state.loop_region_active = true;
                state.project_dirty = true;
                true
            } else {
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_proxy::AudioProxy;
    use crate::views::playlist::{ClipType, PlaylistClip};
    use std::sync::atomic::{AtomicU32, AtomicU64};
    use std::sync::mpsc::channel;
    use std::sync::Arc;

    fn make_state() -> AppState {
        let (tx, _rx) = channel();
        AppState::new(
            AudioProxy::new(tx),
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU32::new(0)),
            None,
        )
    }

    fn make_audio_clip(id: usize, start_tick: u64, duration_ticks: u64) -> PlaylistClip {
        PlaylistClip {
            id,
            name: format!("Clip {id}"),
            start_tick,
            duration_ticks,
            clip_type: ClipType::Audio {
                sample_path: "test.wav".to_string(),
                sample_offset_ticks: 0,
                peaks: Vec::new(),
                total_sample_ticks: duration_ticks,
            },
            color: Color32::from_rgb(0xFF, 0x72, 0x00),
            muted: false,
            loop_enabled: false,
        }
    }

    #[test]
    fn default_pinned_matches_bitwig() {
        let pinned = default_pinned();
        assert_eq!(pinned.len(), 4);
        assert!(pinned.contains(&EditMenuAction::Undo));
        assert!(pinned.contains(&EditMenuAction::Redo));
        assert!(pinned.contains(&EditMenuAction::Duplicate));
        assert!(pinned.contains(&EditMenuAction::Delete));
    }

    #[test]
    fn contextual_labels_and_enablement_match_bitwig_screenshots() {
        let mut state = make_state();
        state.playlist_state.clips.push((1, make_audio_clip(1, 0, 3840)));

        // Sin clip seleccionado (Captura 1: track region)
        assert_eq!(current_target(&state), EditTarget::TrackRegion);
        assert_eq!(action_label(&state, EditMenuAction::Cut), "Cut track region");
        assert_eq!(action_label(&state, EditMenuAction::Copy), "Copy track region");
        assert_eq!(action_label(&state, EditMenuAction::Delete), "Delete track region");
        assert!(!action_enabled(&state, EditMenuAction::Cut));
        assert!(!action_enabled(&state, EditMenuAction::Copy));
        assert!(!action_enabled(&state, EditMenuAction::Paste));
        assert!(!action_enabled(&state, EditMenuAction::Duplicate));
        assert!(!action_enabled(&state, EditMenuAction::Delete));
        assert!(action_enabled(&state, EditMenuAction::SelectAll));
        assert!(action_enabled(&state, EditMenuAction::DeselectAll));
        assert!(!action_enabled(&state, EditMenuAction::ConvertToTimeSelection));

        // Con Audio Clip seleccionado (Captura 2: Arranger clip)
        state.playlist_state.selected_clips = vec![1];
        assert_eq!(current_target(&state), EditTarget::ArrangerClip);
        assert_eq!(action_label(&state, EditMenuAction::Cut), "Cut Arranger clip");
        assert_eq!(action_label(&state, EditMenuAction::Copy), "Copy Arranger clip");
        assert_eq!(action_label(&state, EditMenuAction::Delete), "Delete Arranger clip");
        assert!(action_enabled(&state, EditMenuAction::Cut));
        assert!(action_enabled(&state, EditMenuAction::Copy));
        assert!(!action_enabled(&state, EditMenuAction::Paste));
        assert!(action_enabled(&state, EditMenuAction::Duplicate));
        assert!(action_enabled(&state, EditMenuAction::Delete));
        assert!(action_enabled(&state, EditMenuAction::ConvertToTimeSelection));
    }

    #[test]
    fn execute_actions_and_undo_redo_work_end_to_end() {
        let mut state = make_state();
        state.playlist_state.clips.push((1, make_audio_clip(1, 960, 1920)));
        state.playlist_state.selected_clips = vec![1];

        // Convert to Time Selection
        assert!(execute_action(&mut state, EditMenuAction::ConvertToTimeSelection));
        assert!(state.playlist_state.loop_region_active);
        assert_eq!(state.playlist_state.loop_start_ticks, 960);
        assert_eq!(state.playlist_state.loop_end_ticks, 2880);
        assert_eq!(
            action_label(&state, EditMenuAction::Undo),
            "Undo Convert to Time Selection"
        );

        // Duplicate
        assert!(execute_action(&mut state, EditMenuAction::Duplicate));
        assert_eq!(state.playlist_state.clips.len(), 2);
        assert_eq!(action_label(&state, EditMenuAction::Undo), "Undo Duplicate");

        // Delete
        assert!(execute_action(&mut state, EditMenuAction::Delete));
        assert_eq!(state.playlist_state.clips.len(), 1);
        assert_eq!(
            action_label(&state, EditMenuAction::Undo),
            "Undo Delete Arranger clip"
        );

        // Undo restores deleted clip
        assert!(execute_action(&mut state, EditMenuAction::Undo));
        assert_eq!(state.playlist_state.clips.len(), 2);
        assert_eq!(
            action_label(&state, EditMenuAction::Redo),
            "Redo Delete Arranger clip"
        );

        // Redo re-deletes clip
        assert!(execute_action(&mut state, EditMenuAction::Redo));
        assert_eq!(state.playlist_state.clips.len(), 1);

        // Select All & Deselect All
        assert!(execute_action(&mut state, EditMenuAction::SelectAll));
        assert_eq!(state.playlist_state.selected_clips.len(), 1);
        assert!(execute_action(&mut state, EditMenuAction::DeselectAll));
        assert!(state.playlist_state.selected_clips.is_empty());
    }
}
