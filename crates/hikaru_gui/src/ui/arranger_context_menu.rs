// Hikaru OpenLive - Arranger / Mix void context menu (Bitwig Studio 5.x look)
// crates/hikaru_gui/src/ui/arranger_context_menu.rs

use egui::{pos2, vec2, Align2, Color32, Context, Id, Order, Pos2, Rect, Sense, Stroke, Ui};

use crate::ui::fonts;
use crate::ui::icons::{self, VectorIcon};

const MENU_W: f32 = 252.0;
const PAD_X: f32 = 6.0;
const ROW_H: f32 = 24.0;
const SEP_H: f32 = 9.0;

const MENU_BG: Color32 = Color32::from_rgb(0x32, 0x32, 0x32);
const MENU_BORDER: Color32 = Color32::from_rgb(0x1C, 0x1C, 0x1C);
const SEP: Color32 = Color32::from_rgb(0x4A, 0x4A, 0x4A);
const ROW_HOVER: Color32 = Color32::from_rgb(0xBE, 0xBE, 0xBE);
const TXT: Color32 = Color32::from_rgb(0xEA, 0xEA, 0xEA);
const TXT_HINT: Color32 = Color32::from_rgb(0x98, 0x98, 0x98);
const TXT_ON_HOVER: Color32 = Color32::from_rgb(0x1A, 0x1A, 0x1A);

const fn vi(d: &'static str) -> VectorIcon {
    VectorIcon { filled: false, d }
}

const IC_INST: VectorIcon = vi("M 4 6 L 20 6 L 20 18 L 4 18 L 4 6 M 8 6 L 8 18 M 12 6 L 12 18 M 16 6 L 16 18");
const IC_AUDIO: VectorIcon = vi("M 3 12 L 6 12 L 8 6 L 11 18 L 14 4 L 16 20 L 18 12 L 21 12");
const IC_FX: VectorIcon = vi("M 6 5 L 6 14 L 18 14 M 14 10 L 18 14 L 14 18");
const IC_GROUP: VectorIcon = vi("M 3 6 L 9 6 L 11 8 L 21 8 L 21 18 L 3 18 Z");
const IC_SCENE: VectorIcon = vi("M 3 6 L 21 6 L 21 18 L 3 18 Z M 9 9 L 16 12 L 9 15 Z");
const IC_SCENE_PLAYING: VectorIcon = vi("M 3 7 L 19 7 L 19 19 L 3 19 Z M 8 10 L 14 13 L 8 16 Z M 7 4 L 21 4 L 21 15");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoidMenuAction {
    AddInstrumentTrack,
    AddAudioTrack,
    AddFxTrack,
    AddGroupTrack,
    AddScene,
    AddSceneFromPlayingClips,
}

pub fn open(ctx: &Context, key: Id, pos: Pos2) {
    ctx.data_mut(|d| {
        d.insert_temp(key.with("open"), true);
        d.insert_temp(key.with("pos"), pos);
    });
}

pub fn close(ctx: &Context, key: Id) {
    ctx.data_mut(|d| d.insert_temp(key.with("open"), false));
}

pub fn show(ctx: &Context, key: Id) -> Option<VoidMenuAction> {
    let is_open = ctx.data(|d| d.get_temp::<bool>(key.with("open"))).unwrap_or(false);
    if !is_open {
        return None;
    }

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close(ctx, key);
        return None;
    }

    let raw_pos = ctx
        .data(|d| d.get_temp::<Pos2>(key.with("pos")))
        .unwrap_or(pos2(120.0, 120.0));

    let total_h = 6.0 * ROW_H + SEP_H + 8.0;
    let scr = ctx.screen_rect();
    let pos = pos2(
        raw_pos.x.min(scr.max.x - MENU_W - 6.0).max(scr.min.x + 4.0),
        raw_pos.y.min(scr.max.y - total_h - 6.0).max(scr.min.y + 4.0),
    );

    let mut chosen: Option<VoidMenuAction> = None;

    let area_resp = egui::Area::new(key.with("area"))
        .fixed_pos(pos)
        .order(Order::Foreground)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(MENU_W, total_h), Sense::click());
            let p = ui.painter();
            p.rect_filled(
                rect.translate(vec2(2.0, 4.0)),
                3.0,
                Color32::from_black_alpha(120),
            );
            p.rect_filled(rect, 3.0, MENU_BG);
            p.rect_stroke(
                rect,
                3.0,
                Stroke::new(1.0_f32, MENU_BORDER),
                egui::StrokeKind::Inside,
            );

            let mut y = rect.min.y + 4.0;
            let track_items = [
                ("Add Instrument Track", "Ctrl+T", IC_INST, VoidMenuAction::AddInstrumentTrack),
                ("Add Audio Track", "Ctrl+Shift+T", IC_AUDIO, VoidMenuAction::AddAudioTrack),
                ("Add FX Track", "Ctrl+Alt+T", IC_FX, VoidMenuAction::AddFxTrack),
                ("Add Group Track", "Ctrl+Alt+G", IC_GROUP, VoidMenuAction::AddGroupTrack),
            ];
            for (label, hint, ic, act) in track_items {
                if draw_row(ui, key, rect, y, label, hint, ic) {
                    chosen = Some(act);
                }
                y += ROW_H;
            }

            let sep_y = y + SEP_H * 0.5;
            ui.painter().line_segment(
                [pos2(rect.min.x + PAD_X, sep_y), pos2(rect.max.x - PAD_X, sep_y)],
                Stroke::new(1.0_f32, SEP),
            );
            y += SEP_H;

            let scene_items = [
                ("Add Scene", "Ctrl+I", IC_SCENE, VoidMenuAction::AddScene),
                (
                    "Add Scene from Playing Clips",
                    "Ctrl+Shift+I",
                    IC_SCENE_PLAYING,
                    VoidMenuAction::AddSceneFromPlayingClips,
                ),
            ];
            for (label, hint, ic, act) in scene_items {
                if draw_row(ui, key, rect, y, label, hint, ic) {
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

fn draw_row(
    ui: &mut Ui,
    key: Id,
    menu_rect: Rect,
    y: f32,
    label: &str,
    hint: &str,
    icon: VectorIcon,
) -> bool {
    let row_rect = Rect::from_min_size(pos2(menu_rect.min.x + 2.0, y), vec2(MENU_W - 4.0, ROW_H));
    let resp = ui.interact(row_rect, key.with(("void_row", label)), Sense::click());
    let hovered = resp.hovered();
    let p = ui.painter();

    if hovered {
        p.rect_filled(row_rect, 2.0, ROW_HOVER);
    }

    let fg = if hovered { TXT_ON_HOVER } else { TXT };
    let hint_fg = if hovered { TXT_ON_HOVER } else { TXT_HINT };

    let ic_rect = Rect::from_center_size(pos2(row_rect.min.x + 14.0, row_rect.center().y), vec2(14.0, 14.0));
    icons::paint_icon(p, ic_rect, icon, fg, 1.3_f32);

    p.text(
        pos2(row_rect.min.x + 28.0, row_rect.center().y),
        Align2::LEFT_CENTER,
        label,
        fonts::regular(11.0),
        fg,
    );

    p.text(
        pos2(row_rect.max.x - 8.0, row_rect.center().y),
        Align2::RIGHT_CENTER,
        hint,
        fonts::regular(10.0),
        hint_fg,
    );

    resp.clicked()
}
