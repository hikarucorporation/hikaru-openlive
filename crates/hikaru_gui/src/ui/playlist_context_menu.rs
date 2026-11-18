// Hikaru OpenLive - Playlist / Arranger Timeline context menu (Bitwig Studio 5.x look)
// crates/hikaru_gui/src/ui/playlist_context_menu.rs

use egui::{pos2, vec2, Align2, Color32, Context, Id, Order, Pos2, Rect, Sense, Stroke, Ui};

use crate::ui::fonts;
use crate::ui::icons::{self, VectorIcon};
use crate::views::playlist::PlaylistTool;

const MENU_W: f32 = 250.0;
const PAD_X: f32 = 6.0;
const TOOL_STRIP_H: f32 = 44.0;
const ROW_H: f32 = 24.0;
const SEP_H: f32 = 9.0;

const MENU_BG: Color32 = Color32::from_rgb(0x32, 0x32, 0x32);
const MENU_BORDER: Color32 = Color32::from_rgb(0x1C, 0x1C, 0x1C);
const SEP: Color32 = Color32::from_rgb(0x4A, 0x4A, 0x4A);
const ROW_HOVER: Color32 = Color32::from_rgb(0xBE, 0xBE, 0xBE);
const TOOL_ACTIVE_BG: Color32 = Color32::from_rgb(0x4A, 0x4A, 0x4A);
const TOOL_HOVER_BG: Color32 = Color32::from_rgb(0x3F, 0x3F, 0x3F);
const TXT: Color32 = Color32::from_rgb(0xEA, 0xEA, 0xEA);
const TXT_DISABLED: Color32 = Color32::from_rgb(0x74, 0x74, 0x74);
const TXT_HINT: Color32 = Color32::from_rgb(0x98, 0x98, 0x98);
const TXT_ON_HOVER: Color32 = Color32::from_rgb(0x1A, 0x1A, 0x1A);

const fn vi(d: &'static str) -> VectorIcon {
    VectorIcon { filled: false, d }
}

const IC_TIME: VectorIcon = vi("M 9 5 L 15 5 M 12 5 L 12 19 M 9 19 L 15 19");
const IC_PEN: VectorIcon = vi("M 15 4 L 20 9 L 9 20 L 4 20 L 4 15 Z M 13 6 L 18 11");
const IC_ERASER: VectorIcon = vi("M 7 19 L 4 16 L 14 6 L 20 12 L 13 19 Z M 9 11 L 15 17");
const IC_KNIFE: VectorIcon = vi("M 18 4 L 10 12 L 13 15 L 20 8 Z M 10 12 L 4 18 L 6 20 L 12 14");

const IC_CONSOLIDATE: VectorIcon = vi("M 4 6 L 20 6 L 20 18 L 4 18 Z M 12 6 L 12 18");
const IC_SPLIT: VectorIcon = vi("M 4 7 L 10 7 L 10 17 L 4 17 Z M 14 7 L 20 7 L 20 17 L 14 17 Z M 12 4 L 12 20");
const IC_SET_CLIP_START: VectorIcon = vi("M 6 6 L 4 6 L 4 18 L 6 18 M 18 6 L 20 6 L 20 18 L 18 18 M 9 9 L 15 12 L 9 15 Z");
const IC_BOUNCE_IN_PLACE: VectorIcon = vi("M 5 13 L 9 8 L 14 8 L 17 11 L 17 16 M 14 13 L 17 16 L 20 13 M 4 19 L 20 19");
const IC_BOUNCE: VectorIcon = vi("M 6 14 L 10 9 L 15 9 L 18 12 L 18 18 M 15 15 L 18 18 L 21 15");
const IC_SLICE_DRUM: VectorIcon = vi("M 4 8 L 8 8 M 4 12 L 10 12 M 4 16 L 8 16 M 12 12 L 19 12 M 16 9 L 19 12 L 16 15");
const IC_SLICE_MULTI: VectorIcon = vi("M 5 17 L 5 9 M 8 17 L 8 6 M 11 17 L 11 11 M 14 13 L 20 13 M 17 10 L 20 13 L 17 16");
const IC_INSERT_SILENCE: VectorIcon = vi("M 4 11 L 6 7 L 10 5 L 14 7 L 16 11 L 14 15 L 10 17 L 6 15 Z M 10 8 L 10 11 L 12 13 M 17 13 L 17 19 M 14 16 L 20 16");
const IC_DUPLICATE_TIME: VectorIcon = vi("M 4 11 L 6 7 L 10 5 L 14 7 L 16 11 L 14 15 L 10 17 L 6 15 Z M 10 8 L 10 11 L 12 13 M 15 16 L 21 16 M 18 13 L 21 16 L 18 19");
const IC_REMOVE_TIME: VectorIcon = vi("M 4 11 L 6 7 L 10 5 L 14 7 L 16 11 L 14 15 L 10 17 L 6 15 Z M 10 8 L 10 11 L 12 13 M 15 14 L 20 19 M 20 14 L 15 19");

#[derive(Clone, Copy, Debug)]
pub struct PlaylistContextMenuData {
    pub pos: Pos2,
    pub click_tick: u64,
    pub on_clip: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct PlaylistContextMenuCapabilities {
    pub active_tool: PlaylistTool,
    pub can_consolidate: bool,
    pub can_split: bool,
    pub can_set_clip_start: bool,
    pub can_bounce: bool,
    pub can_slice: bool,
    pub can_insert_silence: bool,
    pub can_duplicate_time: bool,
    pub can_remove_time: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaylistContextMenuAction {
    SelectTool(PlaylistTool),
    Consolidate,
    Split,
    SetClipStart,
    BounceInPlace,
    Bounce,
    SliceToDrumMachine,
    SliceToMultisample,
    InsertSilence,
    DuplicateTime,
    RemoveTime,
}

pub fn open(ctx: &Context, key: Id, data: PlaylistContextMenuData) {
    ctx.data_mut(|d| {
        d.insert_temp(key.with("open"), true);
        d.insert_temp(key.with("data"), data);
    });
}

pub fn close(ctx: &Context, key: Id) {
    ctx.data_mut(|d| d.insert_temp(key.with("open"), false));
}

pub fn get_data(ctx: &Context, key: Id) -> Option<PlaylistContextMenuData> {
    ctx.data(|d| d.get_temp::<PlaylistContextMenuData>(key.with("data")))
}

pub fn show(
    ctx: &Context,
    key: Id,
    caps: PlaylistContextMenuCapabilities,
) -> Option<PlaylistContextMenuAction> {
    let is_open = ctx.data(|d| d.get_temp::<bool>(key.with("open"))).unwrap_or(false);
    if !is_open {
        return None;
    }

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close(ctx, key);
        return None;
    }

    let raw_pos = ctx
        .data(|d| d.get_temp::<PlaylistContextMenuData>(key.with("data")))
        .map(|d| d.pos)
        .unwrap_or(pos2(120.0, 120.0));

    // 1 tool strip + 4 separators + 10 rows + vertical padding
    let total_h = TOOL_STRIP_H + 4.0 * SEP_H + 10.0 * ROW_H + 10.0;
    let scr = ctx.screen_rect();
    let pos = pos2(
        raw_pos.x.min(scr.max.x - MENU_W - 6.0).max(scr.min.x + 4.0),
        raw_pos.y.min(scr.max.y - total_h - 6.0).max(scr.min.y + 4.0),
    );

    let mut chosen: Option<PlaylistContextMenuAction> = None;

    let area_resp = egui::Area::new(key.with("area"))
        .fixed_pos(pos)
        .order(Order::Foreground)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(MENU_W, total_h), Sense::click());
            let p = ui.painter();
            p.rect_filled(
                rect.translate(vec2(2.0, 4.0)),
                3.0,
                Color32::from_black_alpha(130),
            );
            p.rect_filled(rect, 3.0, MENU_BG);
            p.rect_stroke(
                rect,
                3.0,
                Stroke::new(1.0_f32, MENU_BORDER),
                egui::StrokeKind::Inside,
            );

            let mut y = rect.min.y + 4.0;

            // Top tool strip: POINTER | TIME | PEN | ERASER | KNIFE
            let tool_w = (MENU_W - 8.0) / 5.0;
            let tools = [
                (PlaylistTool::Pointer, "POINTER", None),
                (PlaylistTool::Time, "TIME", Some(IC_TIME)),
                (PlaylistTool::Pen, "PEN", Some(IC_PEN)),
                (PlaylistTool::Eraser, "ERASER", Some(IC_ERASER)),
                (PlaylistTool::Knife, "KNIFE", Some(IC_KNIFE)),
            ];
            for (idx, (tool, label, icon_opt)) in tools.into_iter().enumerate() {
                let cell = Rect::from_min_size(
                    pos2(rect.min.x + 4.0 + idx as f32 * tool_w, y),
                    vec2(tool_w, TOOL_STRIP_H),
                );
                let resp = ui.interact(cell, key.with(("tool", idx)), Sense::click());
                let is_active = caps.active_tool == tool;
                let hovered = resp.hovered();
                let cp = ui.painter();

                if is_active {
                    cp.rect_filled(cell.shrink(2.0), 3.0, TOOL_ACTIVE_BG);
                } else if hovered {
                    cp.rect_filled(cell.shrink(2.0), 3.0, TOOL_HOVER_BG);
                }

                let fg = if is_active || hovered { TXT } else { Color32::from_rgb(0xC8, 0xC8, 0xC8) };
                let ic_rect = Rect::from_center_size(
                    pos2(cell.center().x, cell.min.y + 15.0),
                    vec2(15.0, 15.0),
                );
                if let Some(ic) = icon_opt {
                    icons::paint_icon(cp, ic_rect, ic, fg, 1.35_f32);
                } else {
                    icons::paint_pointer_cursor(cp, ic_rect, fg);
                }

                cp.text(
                    pos2(cell.center().x, cell.max.y - 9.0),
                    Align2::CENTER_CENTER,
                    label,
                    fonts::regular(8.5),
                    fg,
                );

                if resp.clicked() {
                    chosen = Some(PlaylistContextMenuAction::SelectTool(tool));
                }
            }
            y += TOOL_STRIP_H;

            draw_separator(ui, rect, &mut y);

            // Section 1: Consolidate / Split / Set Clip Start
            let sec1 = [
                ("Consolidate", "Ctrl+J", IC_CONSOLIDATE, caps.can_consolidate, PlaylistContextMenuAction::Consolidate),
                ("Split", "Ctrl+E", IC_SPLIT, caps.can_split, PlaylistContextMenuAction::Split),
                ("Set Clip Start", "", IC_SET_CLIP_START, caps.can_set_clip_start, PlaylistContextMenuAction::SetClipStart),
            ];
            for (label, hint, ic, enabled, act) in sec1 {
                if draw_row(ui, key, rect, y, label, hint, ic, enabled) {
                    chosen = Some(act);
                }
                y += ROW_H;
            }

            draw_separator(ui, rect, &mut y);

            // Section 2: Bounce In Place / Bounce
            let sec2 = [
                ("Bounce In Place", "Ctrl+B", IC_BOUNCE_IN_PLACE, caps.can_bounce, PlaylistContextMenuAction::BounceInPlace),
                ("Bounce", "", IC_BOUNCE, caps.can_bounce, PlaylistContextMenuAction::Bounce),
            ];
            for (label, hint, ic, enabled, act) in sec2 {
                if draw_row(ui, key, rect, y, label, hint, ic, enabled) {
                    chosen = Some(act);
                }
                y += ROW_H;
            }

            draw_separator(ui, rect, &mut y);

            // Section 3: Slice to Drum Machine... / Slice to Multisample...
            let sec3 = [
                ("Slice to Drum Machine...", "Ctrl+Alt+D", IC_SLICE_DRUM, caps.can_slice, PlaylistContextMenuAction::SliceToDrumMachine),
                ("Slice to Multisample...", "", IC_SLICE_MULTI, caps.can_slice, PlaylistContextMenuAction::SliceToMultisample),
            ];
            for (label, hint, ic, enabled, act) in sec3 {
                if draw_row(ui, key, rect, y, label, hint, ic, enabled) {
                    chosen = Some(act);
                }
                y += ROW_H;
            }

            draw_separator(ui, rect, &mut y);

            // Section 4: Insert Silence / Duplicate Time / Remove Time
            let sec4 = [
                ("Insert Silence", "Ctrl+Shift+P", IC_INSERT_SILENCE, caps.can_insert_silence, PlaylistContextMenuAction::InsertSilence),
                ("Duplicate Time", "Ctrl+Shift+D", IC_DUPLICATE_TIME, caps.can_duplicate_time, PlaylistContextMenuAction::DuplicateTime),
                ("Remove Time", "Shift+Backspace", IC_REMOVE_TIME, caps.can_remove_time, PlaylistContextMenuAction::RemoveTime),
            ];
            for (label, hint, ic, enabled, act) in sec4 {
                if draw_row(ui, key, rect, y, label, hint, ic, enabled) {
                    chosen = Some(act);
                }
                y += ROW_H;
            }
        });

    if chosen.is_some() {
        close(ctx, key);
        return chosen;
    }

    let clicked_outside = ctx.input(|i| {
        i.pointer.any_pressed()
            && i.pointer
                .interact_pos()
                .is_some_and(|p| !area_resp.response.rect.contains(p))
    });
    if clicked_outside {
        close(ctx, key);
    }

    None
}

fn draw_separator(ui: &mut Ui, menu_rect: Rect, y: &mut f32) {
    let sep_y = *y + SEP_H * 0.5;
    ui.painter().line_segment(
        [pos2(menu_rect.min.x + PAD_X, sep_y), pos2(menu_rect.max.x - PAD_X, sep_y)],
        Stroke::new(1.0_f32, SEP),
    );
    *y += SEP_H;
}

#[allow(clippy::too_many_arguments)]
fn draw_row(
    ui: &mut Ui,
    key: Id,
    menu_rect: Rect,
    y: f32,
    label: &str,
    hint: &str,
    icon: VectorIcon,
    enabled: bool,
) -> bool {
    let row_rect = Rect::from_min_size(pos2(menu_rect.min.x + 2.0, y), vec2(MENU_W - 4.0, ROW_H));
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let resp = ui.interact(row_rect, key.with(("pl_row", label)), sense);
    let hovered = enabled && resp.hovered();
    let p = ui.painter();

    if hovered {
        p.rect_filled(row_rect, 2.0, ROW_HOVER);
    }

    let fg = if !enabled {
        TXT_DISABLED
    } else if hovered {
        TXT_ON_HOVER
    } else {
        TXT
    };
    let hint_fg = if !enabled {
        TXT_DISABLED
    } else if hovered {
        TXT_ON_HOVER
    } else {
        TXT_HINT
    };

    let ic_rect = Rect::from_center_size(pos2(row_rect.min.x + 14.0, row_rect.center().y), vec2(14.0, 14.0));
    icons::paint_icon(p, ic_rect, icon, fg, 1.25_f32);

    p.text(
        pos2(row_rect.min.x + 28.0, row_rect.center().y),
        Align2::LEFT_CENTER,
        label,
        fonts::regular(11.0),
        fg,
    );

    if !hint.is_empty() {
        p.text(
            pos2(row_rect.max.x - 8.0, row_rect.center().y),
            Align2::RIGHT_CENTER,
            hint,
            fonts::regular(10.0),
            hint_fg,
        );
    }

    enabled && resp.clicked()
}
