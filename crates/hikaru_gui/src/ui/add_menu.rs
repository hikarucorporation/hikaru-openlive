// Hikaru OpenLive - Bitwig Studio 5.x style ADD dropdown menu
// GNU AGPLv3
// crates/hikaru_gui/src/ui/add_menu.rs
//
// Menú desplegable de [ADD] pintado a mano para reproducir 1:1 el layout de
// Bitwig Studio 5.x con todas las acciones operativas en Hikaru OpenLive:
//
//   [🎹] Add Instrument Track              Ctrl+T        📌
//   [〰] Add Audio Track                   Ctrl+Shift+T  📌
//   [↴] Add FX Track                      Ctrl+Alt+T    📌
//   [📁] Add Group Track                   Ctrl+Alt+G    📌
//   ───────────────────────────────────────────────────────
//   [▷] Add Scene                         Ctrl+I        📌
//   [❐] Add Scene from Playing Clips      Ctrl+Shift+I  📌
//   ───────────────────────────────────────────────────────
//   [⚑] Insert Cue Marker Here                          📌

use std::collections::HashSet;

use egui::{
    pos2, vec2, Align2, Color32, Context, CursorIcon, Id, Order, Pos2, Rect, Sense, Stroke, Ui,
};

use crate::app::AppState;
use crate::ui::bitwig_kit::BW_ACCENT_ORANGE;
use crate::ui::fonts;
use crate::ui::icons::{self, VectorIcon};
use crate::views::matrix::SlotState;
use crate::views::mixer::Track;

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
const TXT_HINT: Color32 = Color32::from_rgb(0x98, 0x98, 0x98);
const TXT_ON_HOVER: Color32 = Color32::from_rgb(0x1A, 0x1A, 0x1A);
const PIN_IDLE: Color32 = Color32::from_rgb(0x9A, 0x9A, 0xA0);

const fn vi(d: &'static str) -> VectorIcon {
    VectorIcon { filled: false, d }
}

// Iconos 24x24 calibrados al menú ADD de Bitwig Studio 5.x
const IC_INSTRUMENT: VectorIcon = vi(
    "M 4 5 L 20 5 L 20 19 L 4 19 Z M 9.3 5 L 9.3 19 M 14.7 5 L 14.7 19 M 7.8 5 L 7.8 13 L 10.8 13 L 10.8 5 M 13.2 5 L 13.2 13 L 16.2 13 L 16.2 5",
);
const IC_AUDIO: VectorIcon = vi(
    "M 3 12 L 6 12 L 8 6 L 10.5 18 L 13 4 L 15.5 20 L 18 12 L 21 12",
);
const IC_FX: VectorIcon = vi(
    "M 6 6 L 13 6 L 13 18 M 9.5 14.5 L 13 18 L 16.5 14.5",
);
const IC_GROUP: VectorIcon = vi(
    "M 3 6 L 9.5 6 L 11.5 8.5 L 21 8.5 L 21 18.5 L 3 18.5 Z",
);
const IC_SCENE: VectorIcon = vi(
    "M 3 6 L 21 6 L 21 18 L 3 18 Z M 9.5 9.5 L 15.5 12 L 9.5 14.5 Z",
);
const IC_SCENE_PLAYING: VectorIcon = vi(
    "M 3 8 L 19 8 L 19 19 L 3 19 Z M 8.5 11 L 14 13.5 L 8.5 16 Z M 6 5 L 21 5 L 21 16",
);
const IC_CUE_FLAG: VectorIcon = vi(
    "M 7 4 L 7 20 M 7 5 L 17 5 L 14.5 8.5 L 17 12 L 7 12",
);
const IC_PIN: VectorIcon = vi(
    "M 15 4 L 20 9 M 16 5 L 11 10 L 8 10 L 14 16 L 14 13 L 19 8 M 11 13 L 5 19",
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AddMenuAction {
    AddInstrumentTrack,
    AddAudioTrack,
    AddFxTrack,
    AddGroupTrack,
    AddScene,
    AddSceneFromPlayingClips,
    InsertCueMarkerHere,
}

struct MenuRow {
    label: &'static str,
    shortcut: &'static str,
    icon: VectorIcon,
    action: AddMenuAction,
}

const SECTION_TRACKS: [MenuRow; 4] = [
    MenuRow {
        label: "Add Instrument Track",
        shortcut: "Ctrl+T",
        icon: IC_INSTRUMENT,
        action: AddMenuAction::AddInstrumentTrack,
    },
    MenuRow {
        label: "Add Audio Track",
        shortcut: "Ctrl+Shift+T",
        icon: IC_AUDIO,
        action: AddMenuAction::AddAudioTrack,
    },
    MenuRow {
        label: "Add FX Track",
        shortcut: "Ctrl+Alt+T",
        icon: IC_FX,
        action: AddMenuAction::AddFxTrack,
    },
    MenuRow {
        label: "Add Group Track",
        shortcut: "Ctrl+Alt+G",
        icon: IC_GROUP,
        action: AddMenuAction::AddGroupTrack,
    },
];

const SECTION_SCENES: [MenuRow; 2] = [
    MenuRow {
        label: "Add Scene",
        shortcut: "Ctrl+I",
        icon: IC_SCENE,
        action: AddMenuAction::AddScene,
    },
    MenuRow {
        label: "Add Scene from Playing Clips",
        shortcut: "Ctrl+Shift+I",
        icon: IC_SCENE_PLAYING,
        action: AddMenuAction::AddSceneFromPlayingClips,
    },
];

const SECTION_MARKERS: [MenuRow; 1] = [MenuRow {
    label: "Insert Cue Marker Here",
    shortcut: "",
    icon: IC_CUE_FLAG,
    action: AddMenuAction::InsertCueMarkerHere,
}];

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

fn load_pinned(ctx: &Context, key: Id) -> HashSet<AddMenuAction> {
    ctx.data(|d| d.get_temp::<HashSet<AddMenuAction>>(key.with("pinned")))
        .unwrap_or_default()
}

fn save_pinned(ctx: &Context, key: Id, pinned: HashSet<AddMenuAction>) {
    ctx.data_mut(|d| d.insert_temp(key.with("pinned"), pinned));
}

pub fn show(ctx: &Context, key: Id, anchor_rect: Rect) -> Option<AddMenuAction> {
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

    let total_rows = SECTION_TRACKS.len() + SECTION_SCENES.len() + SECTION_MARKERS.len();
    let total_h = (total_rows as f32) * ROW_H + 2.0 * SEP_H + 2.0 * PAD_Y;
    let scr = ctx.screen_rect();
    let pos = pos2(
        raw_pos.x.min(scr.max.x - MENU_W - 4.0).max(scr.min.x + 2.0),
        raw_pos.y.min(scr.max.y - total_h - 4.0).max(scr.min.y + 2.0),
    );

    let mut pinned = load_pinned(ctx, key);
    let mut pinned_changed = false;
    let mut chosen: Option<AddMenuAction> = None;

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
            for row in &SECTION_TRACKS {
                match draw_row(ui, key, rect, y, row, pinned.contains(&row.action)) {
                    RowClick::Action => chosen = Some(row.action),
                    RowClick::TogglePin => {
                        if !pinned.remove(&row.action) {
                            pinned.insert(row.action);
                        }
                        pinned_changed = true;
                    }
                    RowClick::None => {}
                }
                y += ROW_H;
            }

            draw_separator(ui, rect, y);
            y += SEP_H;

            for row in &SECTION_SCENES {
                match draw_row(ui, key, rect, y, row, pinned.contains(&row.action)) {
                    RowClick::Action => chosen = Some(row.action),
                    RowClick::TogglePin => {
                        if !pinned.remove(&row.action) {
                            pinned.insert(row.action);
                        }
                        pinned_changed = true;
                    }
                    RowClick::None => {}
                }
                y += ROW_H;
            }

            draw_separator(ui, rect, y);
            y += SEP_H;

            for row in &SECTION_MARKERS {
                match draw_row(ui, key, rect, y, row, pinned.contains(&row.action)) {
                    RowClick::Action => chosen = Some(row.action),
                    RowClick::TogglePin => {
                        if !pinned.remove(&row.action) {
                            pinned.insert(row.action);
                        }
                        pinned_changed = true;
                    }
                    RowClick::None => {}
                }
                y += ROW_H;
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

fn draw_row(
    ui: &mut Ui,
    key: Id,
    menu_rect: Rect,
    y: f32,
    row: &MenuRow,
    is_pinned: bool,
) -> RowClick {
    let row_rect = Rect::from_min_size(pos2(menu_rect.min.x + 2.0, y), vec2(MENU_W - 4.0, ROW_H));
    let pin_rect = Rect::from_center_size(
        pos2(row_rect.max.x - 13.0, row_rect.center().y),
        vec2(18.0, ROW_H - 4.0),
    );
    let main_rect = Rect::from_min_max(row_rect.min, pos2(pin_rect.min.x - 2.0, row_rect.max.y));

    let pin_resp = ui.interact(pin_rect, key.with(("add_pin", row.label)), Sense::click());
    let main_resp = ui.interact(main_rect, key.with(("add_row", row.label)), Sense::click());

    let hovered_row = main_resp.hovered() || pin_resp.hovered();
    let p = ui.painter();

    if hovered_row {
        p.rect_filled(row_rect, 2.0, ROW_HOVER);
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    let fg = if hovered_row { TXT_ON_HOVER } else { TXT };
    let hint_fg = if hovered_row { TXT_ON_HOVER } else { TXT_HINT };

    // Icono izquierdo
    let ic_rect = Rect::from_center_size(
        pos2(row_rect.min.x + 16.0, row_rect.center().y),
        vec2(14.0, 14.0),
    );
    icons::paint_icon(p, ic_rect, row.icon, fg, 1.3_f32);

    // Etiqueta principal
    p.text(
        pos2(row_rect.min.x + 32.0, row_rect.center().y),
        Align2::LEFT_CENTER,
        row.label,
        fonts::regular(11.5),
        fg,
    );

    // Atajo de teclado (a la izquierda del pin)
    if !row.shortcut.is_empty() {
        p.text(
            pos2(pin_rect.min.x - 6.0, row_rect.center().y),
            Align2::RIGHT_CENTER,
            row.shortcut,
            fonts::regular(10.5),
            hint_fg,
        );
    }

    // Icono Pin derecho
    let pin_col = if is_pinned {
        BW_ACCENT_ORANGE
    } else if hovered_row {
        TXT_ON_HOVER
    } else {
        PIN_IDLE
    };
    let pin_ic_rect = Rect::from_center_size(pin_rect.center(), vec2(11.0, 11.0));
    icons::paint_icon(p, pin_ic_rect, IC_PIN, pin_col, 1.2_f32);

    if pin_resp.clicked() {
        RowClick::TogglePin
    } else if main_resp.clicked() {
        RowClick::Action
    } else {
        RowClick::None
    }
}

// =========================================================================
// EJECUCIÓN DE ACCIONES DEL MENÚ ADD
// =========================================================================

pub fn execute_action(state: &mut AppState, action: AddMenuAction) {
    match action {
        AddMenuAction::AddInstrumentTrack => {
            state.popup_browser.open_new_instrument();
        }
        AddMenuAction::AddAudioTrack => {
            state.matrix_state.add_track();
            let new_idx = state.matrix_state.tracks.len();
            let name = format!("Audio {new_idx}");
            if let Some(last) = state.matrix_state.tracks.last_mut() {
                last.name = name.clone();
            }
            let mut tr = Track::new(new_idx, name, false);
            tr.matrix_idx = Some(new_idx - 1);
            state.live_tracks.push(tr.clone());
            state.studio_tracks.push(tr);
            state.selected_track_index = state.live_tracks.len().saturating_sub(1);
            state.selected_fx_index = None;
            state.project_dirty = true;
        }
        AddMenuAction::AddFxTrack => {
            let fx_idx = state.add_fx_track();
            state.selected_fx_index = Some(fx_idx);
            state.project_dirty = true;
        }
        AddMenuAction::AddGroupTrack => {
            let next_id = state.live_tracks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
            let name = format!("Group {next_id}");
            state.matrix_state.add_track();
            if let Some(last) = state.matrix_state.tracks.last_mut() {
                last.name = name.clone();
            }
            let mut tr = Track::new_group(next_id, name);
            tr.matrix_idx = Some(state.matrix_state.tracks.len().saturating_sub(1));
            state.live_tracks.push(tr.clone());
            state.studio_tracks.push(tr);
            state.selected_track_index = state.live_tracks.len().saturating_sub(1);
            state.selected_fx_index = None;
            state.project_dirty = true;
        }
        AddMenuAction::AddScene => {
            state.matrix_state.add_scene_synced(&state.audio_proxy);
            state.project_dirty = true;
        }
        AddMenuAction::AddSceneFromPlayingClips => {
            state.matrix_state.add_scene_synced(&state.audio_proxy);
            let new_scene_idx = state.matrix_state.scenes.len().saturating_sub(1);
            for t_idx in 0..state.matrix_state.grid.len() {
                let playing_clip = state.matrix_state.grid[t_idx]
                    .iter()
                    .find(|s| s.state == SlotState::Playing && s.clip.is_some())
                    .and_then(|s| s.clip.clone());
                if let Some(clip) = playing_clip {
                    if let Some(dst) = state.matrix_state.slot_mut(t_idx, new_scene_idx) {
                        dst.clip = Some(clip);
                        dst.state = SlotState::Stopped;
                    }
                }
            }
            state.project_dirty = true;
        }
        AddMenuAction::InsertCueMarkerHere => {
            let tick = state.playlist_state.playhead_tick;
            state.playlist_state.insert_cue_marker_at(tick);
            state.project_dirty = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_menu_has_exact_bitwig_sections_and_items() {
        assert_eq!(SECTION_TRACKS.len(), 4);
        assert_eq!(SECTION_TRACKS[0].label, "Add Instrument Track");
        assert_eq!(SECTION_TRACKS[0].shortcut, "Ctrl+T");
        assert_eq!(SECTION_TRACKS[1].label, "Add Audio Track");
        assert_eq!(SECTION_TRACKS[1].shortcut, "Ctrl+Shift+T");
        assert_eq!(SECTION_TRACKS[2].label, "Add FX Track");
        assert_eq!(SECTION_TRACKS[2].shortcut, "Ctrl+Alt+T");
        assert_eq!(SECTION_TRACKS[3].label, "Add Group Track");
        assert_eq!(SECTION_TRACKS[3].shortcut, "Ctrl+Alt+G");

        assert_eq!(SECTION_SCENES.len(), 2);
        assert_eq!(SECTION_SCENES[0].label, "Add Scene");
        assert_eq!(SECTION_SCENES[0].shortcut, "Ctrl+I");
        assert_eq!(SECTION_SCENES[1].label, "Add Scene from Playing Clips");
        assert_eq!(SECTION_SCENES[1].shortcut, "Ctrl+Shift+I");

        assert_eq!(SECTION_MARKERS.len(), 1);
        assert_eq!(SECTION_MARKERS[0].label, "Insert Cue Marker Here");
        assert_eq!(SECTION_MARKERS[0].shortcut, "");
    }

    #[test]
    fn execute_add_actions_mutate_state_cleanly() {
        use std::sync::atomic::{AtomicU32, AtomicU64};
        use std::sync::mpsc::channel;
        use std::sync::Arc;
        use crate::audio_proxy::AudioProxy;

        let (tx, _rx) = channel();
        let mut state = AppState::new(
            AudioProxy::new(tx),
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU32::new(0)),
            None,
        );
        let tracks_before = state.live_tracks.len();
        let fx_before = state.fx_tracks.len();
        let scenes_before = state.matrix_state.scenes.len();

        execute_action(&mut state, AddMenuAction::AddInstrumentTrack);
        assert!(state.popup_browser.open);

        execute_action(&mut state, AddMenuAction::AddAudioTrack);
        assert_eq!(state.live_tracks.len(), tracks_before + 1);

        execute_action(&mut state, AddMenuAction::AddFxTrack);
        assert_eq!(state.fx_tracks.len(), fx_before + 1);

        execute_action(&mut state, AddMenuAction::AddGroupTrack);
        assert_eq!(state.live_tracks.len(), tracks_before + 2);
        assert!(state.live_tracks.last().unwrap().is_group);

        execute_action(&mut state, AddMenuAction::AddScene);
        assert_eq!(state.matrix_state.scenes.len(), scenes_before + 1);

        execute_action(&mut state, AddMenuAction::AddSceneFromPlayingClips);
        assert_eq!(state.matrix_state.scenes.len(), scenes_before + 2);

        state.playlist_state.playhead_tick = 1920;
        execute_action(&mut state, AddMenuAction::InsertCueMarkerHere);
        assert_eq!(state.playlist_state.cue_markers.len(), 1);
        assert_eq!(state.playlist_state.cue_markers[0].tick, 1920);
        assert_eq!(state.playlist_state.cue_markers[0].name, "Cue 1");
    }
}
