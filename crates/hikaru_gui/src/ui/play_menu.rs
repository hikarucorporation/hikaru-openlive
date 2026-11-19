// Hikaru OpenLive - Bitwig Studio 5.x style PLAY dropdown menu
// GNU AGPLv3
// crates/hikaru_gui/src/ui/play_menu.rs
//
// Menú desplegable de [PLAY] pintado a mano para reproducir 1:1 el layout de
// Bitwig Studio 5.x:
//
//   ARRANGER
//     [W]  Automation Write              [Latch | Touch | Write]  📌
//     [+]  Overdub                                                📌
//   ───────────────────────────────────────────────────────────────
//   CLIP LAUNCHER
//     [W]  Automation Write                                       📌
//     [+]  Overdub                                                📌
//     [▪]  Record as Comping Takes       Take Length [ 4.0.0.00 ] 📌
//     [Scn] Record on scene launch
//   ───────────────────────────────────────────────────────────────
//   GROOVE
//     [≋]  Enable Groove                                          📌
//   ───────────────────────────────────────────────────────────────
//   PLAYBACK
//     [▲]  Metronome                     [ -12.0 dB ] [Play ticks]
//     [⊞]  Fill (used by Operators)                               📌
//   ───────────────────────────────────────────────────────────────
//   RECORDING SETTINGS
//     Pre-roll                           [None | 1 | 2 | 4]  [▲]
//     Record quantization           [Off|1/32|1/16|1/8|1/4] [LEN]

use std::collections::HashSet;

use egui::{
    pos2, vec2, Align2, Color32, Context, CursorIcon, Id, Order, Painter, Pos2, Rect, Sense,
    Stroke, Ui,
};

use crate::app::AppState;
use crate::ui::bitwig_kit::BW_ACCENT_ORANGE;
use crate::ui::fonts;
use crate::ui::icons::{self, VectorIcon};

const MENU_W: f32 = 344.0;
const PAD_X: f32 = 10.0;
const PAD_Y: f32 = 8.0;
const SEC_HDR_H: f32 = 17.0;
const ROW_H: f32 = 27.0;
const SEP_H: f32 = 13.0;
const ICON_BOX_SZ: f32 = 22.0;
const CTRL_H: f32 = 21.0;

const MENU_BG: Color32 = Color32::from_rgb(0x2D, 0x2D, 0x2F);
const MENU_BORDER: Color32 = Color32::from_rgb(0x18, 0x18, 0x1A);
const SEP_COL: Color32 = Color32::from_rgb(0x46, 0x46, 0x49);
const SEC_TXT: Color32 = Color32::from_rgb(0x9A, 0x9B, 0xA0);
const TXT: Color32 = Color32::from_rgb(0xEA, 0xEA, 0xEC);
const TXT_MUTED: Color32 = Color32::from_rgb(0x96, 0x97, 0x9C);
const TXT_DARK: Color32 = Color32::from_rgb(0x1A, 0x1A, 0x1C);
const BTN_BG: Color32 = Color32::from_rgb(0x4B, 0x4C, 0x51);
const BTN_BG_HOVER: Color32 = Color32::from_rgb(0x5A, 0x5B, 0x62);
const BTN_BORDER: Color32 = Color32::from_rgb(0x1E, 0x1E, 0x21);
const SEG_DIVIDER: Color32 = Color32::from_rgb(0x2A, 0x2B, 0x2E);
const VAL_BG: Color32 = Color32::from_rgb(0x9C, 0x9E, 0xA4);
const VAL_BG_HOVER: Color32 = Color32::from_rgb(0xAC, 0xAE, 0xB4);
const PIN_IDLE: Color32 = Color32::from_rgb(0x9A, 0x9A, 0xA0);
const ORANGE_DIM: Color32 = Color32::from_rgb(0x9E, 0x5A, 0x22);

const IC_PIN: VectorIcon = VectorIcon {
    filled: false,
    d: "M 15 4 L 20 9 M 16 5 L 11 10 L 8 10 L 14 16 L 14 13 L 19 8 M 11 13 L 5 19",
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AutomationWriteMode {
    Latch,
    Touch,
    Write,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PreRollBars {
    None,
    Bars1,
    Bars2,
    Bars4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RecordQuantization {
    Off,
    Q1_32,
    Q1_16,
    Q1_8,
    Q1_4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlayPinItem {
    ArrangerAutomationWrite,
    ArrangerOverdub,
    LauncherAutomationWrite,
    LauncherOverdub,
    CompingTakes,
    EnableGroove,
    FillMode,
}

#[derive(Clone, Debug)]
pub struct PlayMenuState {
    pub automation_write_mode: AutomationWriteMode,
    pub arranger_overdub: bool,
    pub launcher_automation_write: bool,
    pub launcher_overdub: bool,
    pub record_as_comping_takes: bool,
    pub comping_take_length_bars: f32,
    pub record_on_scene_launch: bool,
    pub groove_enabled: bool,
    pub metronome_volume_db: f32,
    pub metronome_play_ticks: bool,
    pub fill_mode: bool,
    pub pre_roll: PreRollBars,
    pub pre_roll_metronome: bool,
    pub record_quantization: RecordQuantization,
    pub record_quantize_length: bool,
    pub pinned: HashSet<PlayPinItem>,
}

impl Default for PlayMenuState {
    fn default() -> Self {
        let mut pinned = HashSet::new();
        pinned.insert(PlayPinItem::ArrangerAutomationWrite);
        pinned.insert(PlayPinItem::CompingTakes);
        pinned.insert(PlayPinItem::FillMode);
        Self {
            automation_write_mode: AutomationWriteMode::Latch,
            arranger_overdub: true,
            launcher_automation_write: false,
            launcher_overdub: false,
            record_as_comping_takes: false,
            comping_take_length_bars: 4.0,
            record_on_scene_launch: false,
            groove_enabled: false,
            metronome_volume_db: -12.0,
            metronome_play_ticks: false,
            fill_mode: false,
            pre_roll: PreRollBars::None,
            pre_roll_metronome: true,
            record_quantization: RecordQuantization::Off,
            record_quantize_length: true,
            pinned,
        }
    }
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

pub fn show(ctx: &Context, key: Id, anchor_rect: Rect, state: &mut AppState) {
    if !is_open(ctx, key) {
        return;
    }

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close(ctx, key);
        return;
    }

    let raw_pos = ctx
        .data(|d| d.get_temp::<Pos2>(key.with("pos")))
        .unwrap_or_else(|| pos2(anchor_rect.min.x, anchor_rect.max.y + 2.0));

    // 5 secciones, 11 filas, 4 separadores
    let total_h = 2.0 * PAD_Y + 5.0 * SEC_HDR_H + 11.0 * ROW_H + 4.0 * SEP_H + 6.0;
    let scr = ctx.screen_rect();
    let pos = pos2(
        raw_pos.x.min(scr.max.x - MENU_W - 4.0).max(scr.min.x + 2.0),
        raw_pos.y.min(scr.max.y - total_h - 4.0).max(scr.min.y + 2.0),
    );

    let area_resp = egui::Area::new(key.with("area"))
        .fixed_pos(pos)
        .order(Order::Foreground)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(MENU_W, total_h), Sense::click());
            {
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
            }

            let left_x = rect.min.x + PAD_X;
            let right_x = rect.max.x - PAD_X;
            let pin_cx = right_x - 8.0;
            let mut y = rect.min.y + PAD_Y;

            // ── ARRANGER ──────────────────────────────────────────────────────
            draw_section_header(ui.painter(), left_x, y, "ARRANGER");
            y += SEC_HDR_H;

            // Row 1: Automation Write + [Latch | Touch | Write] + pin
            {
                let cy = y + ROW_H * 0.5;
                let auto_active = state.panel_layout.show_automation;
                let seg_w = 156.0;
                let seg_right = pin_cx - 14.0;
                let seg_rect = Rect::from_min_size(
                    pos2(seg_right - seg_w, cy - CTRL_H * 0.5),
                    vec2(seg_w, CTRL_H),
                );
                if draw_icon_toggle_row(
                    ui,
                    key.with("arr_auto"),
                    left_x,
                    seg_rect.min.x - 6.0,
                    cy,
                    auto_active,
                    "Automation Write",
                    |painter, r, fg| icons::paint_automation_nodes(painter, r, fg, true),
                ) {
                    state.panel_layout.show_automation = !state.panel_layout.show_automation;
                }
                let modes = [
                    (AutomationWriteMode::Latch, "Latch"),
                    (AutomationWriteMode::Touch, "Touch"),
                    (AutomationWriteMode::Write, "Write"),
                ];
                if let Some(m) = draw_segmented_group(
                    ui,
                    key.with("auto_mode"),
                    seg_rect,
                    &modes,
                    state.play_menu.automation_write_mode,
                ) {
                    state.play_menu.automation_write_mode = m;
                }
                draw_pin(
                    ui,
                    key.with("pin_arr_auto"),
                    pos2(pin_cx, cy),
                    &mut state.play_menu.pinned,
                    PlayPinItem::ArrangerAutomationWrite,
                );
                y += ROW_H;
            }

            // Row 2: Overdub + pin
            {
                let cy = y + ROW_H * 0.5;
                if draw_icon_toggle_row(
                    ui,
                    key.with("arr_overdub"),
                    left_x,
                    pin_cx - 16.0,
                    cy,
                    state.play_menu.arranger_overdub,
                    "Overdub",
                    paint_overdub_plus,
                ) {
                    state.play_menu.arranger_overdub = !state.play_menu.arranger_overdub;
                }
                draw_pin(
                    ui,
                    key.with("pin_arr_overdub"),
                    pos2(pin_cx, cy),
                    &mut state.play_menu.pinned,
                    PlayPinItem::ArrangerOverdub,
                );
                y += ROW_H;
            }

            draw_separator(ui.painter(), rect, y);
            y += SEP_H;

            // ── CLIP LAUNCHER ─────────────────────────────────────────────────
            draw_section_header(ui.painter(), left_x, y, "CLIP LAUNCHER");
            y += SEC_HDR_H;

            // Row 3: Clip Launcher Automation Write + pin
            {
                let cy = y + ROW_H * 0.5;
                if draw_icon_toggle_row(
                    ui,
                    key.with("cl_auto"),
                    left_x,
                    pin_cx - 16.0,
                    cy,
                    state.play_menu.launcher_automation_write,
                    "Automation Write",
                    paint_clip_automation_write,
                ) {
                    state.play_menu.launcher_automation_write =
                        !state.play_menu.launcher_automation_write;
                }
                draw_pin(
                    ui,
                    key.with("pin_cl_auto"),
                    pos2(pin_cx, cy),
                    &mut state.play_menu.pinned,
                    PlayPinItem::LauncherAutomationWrite,
                );
                y += ROW_H;
            }

            // Row 4: Clip Launcher Overdub + pin
            {
                let cy = y + ROW_H * 0.5;
                if draw_icon_toggle_row(
                    ui,
                    key.with("cl_overdub"),
                    left_x,
                    pin_cx - 16.0,
                    cy,
                    state.play_menu.launcher_overdub,
                    "Overdub",
                    paint_clip_overdub,
                ) {
                    state.play_menu.launcher_overdub = !state.play_menu.launcher_overdub;
                }
                draw_pin(
                    ui,
                    key.with("pin_cl_overdub"),
                    pos2(pin_cx, cy),
                    &mut state.play_menu.pinned,
                    PlayPinItem::LauncherOverdub,
                );
                y += ROW_H;
            }

            // Row 5: Record as Comping Takes + Take Length [ 4.0.0.00 ] + pin
            {
                let cy = y + ROW_H * 0.5;
                let val_w = 58.0;
                let val_right = pin_cx - 14.0;
                let val_rect = Rect::from_min_size(
                    pos2(val_right - val_w, cy - CTRL_H * 0.5),
                    vec2(val_w, CTRL_H),
                );
                let take_lbl_right = val_rect.min.x - 6.0;

                if draw_icon_toggle_row(
                    ui,
                    key.with("cl_comping"),
                    left_x,
                    take_lbl_right - 58.0,
                    cy,
                    state.play_menu.record_as_comping_takes,
                    "Record as Comping Takes",
                    paint_comping_icon,
                ) {
                    state.play_menu.record_as_comping_takes =
                        !state.play_menu.record_as_comping_takes;
                }

                ui.painter().text(
                    pos2(take_lbl_right, cy),
                    Align2::RIGHT_CENTER,
                    "Take Length",
                    fonts::regular(10.5),
                    TXT_MUTED,
                );

                let take_bars = state.play_menu.comping_take_length_bars;
                let bars_whole = take_bars.floor() as u32;
                let beats_rem = ((take_bars.fract() * 4.0).round() as u32).min(3);
                let take_str = format!("{}.{}.0.00", bars_whole, beats_rem);
                let val_resp = draw_value_box(ui, key.with("take_len_val"), val_rect, &take_str);
                if val_resp.dragged() {
                    let delta = -val_resp.drag_delta().y + val_resp.drag_delta().x * 0.5;
                    state.play_menu.comping_take_length_bars =
                        (state.play_menu.comping_take_length_bars + delta * 0.05)
                            .clamp(0.25, 64.0);
                }
                if val_resp.double_clicked() {
                    state.play_menu.comping_take_length_bars = 4.0;
                }

                draw_pin(
                    ui,
                    key.with("pin_cl_comping"),
                    pos2(pin_cx, cy),
                    &mut state.play_menu.pinned,
                    PlayPinItem::CompingTakes,
                );
                y += ROW_H;
            }

            // Row 6: Record on scene launch (no pin)
            {
                let cy = y + ROW_H * 0.5;
                if draw_icon_toggle_row(
                    ui,
                    key.with("cl_rec_scene"),
                    left_x,
                    right_x,
                    cy,
                    state.play_menu.record_on_scene_launch,
                    "Record on scene launch",
                    paint_scn_icon,
                ) {
                    state.play_menu.record_on_scene_launch =
                        !state.play_menu.record_on_scene_launch;
                }
                y += ROW_H;
            }

            draw_separator(ui.painter(), rect, y);
            y += SEP_H;

            // ── GROOVE ────────────────────────────────────────────────────────
            draw_section_header(ui.painter(), left_x, y, "GROOVE");
            y += SEC_HDR_H;

            // Row 7: Enable Groove + pin
            {
                let cy = y + ROW_H * 0.5;
                if draw_icon_toggle_row(
                    ui,
                    key.with("groove_enable"),
                    left_x,
                    pin_cx - 16.0,
                    cy,
                    state.play_menu.groove_enabled,
                    "Enable Groove",
                    paint_groove_icon,
                ) {
                    state.play_menu.groove_enabled = !state.play_menu.groove_enabled;
                }
                draw_pin(
                    ui,
                    key.with("pin_groove"),
                    pos2(pin_cx, cy),
                    &mut state.play_menu.pinned,
                    PlayPinItem::EnableGroove,
                );
                y += ROW_H;
            }

            draw_separator(ui.painter(), rect, y);
            y += SEP_H;

            // ── PLAYBACK ──────────────────────────────────────────────────────
            draw_section_header(ui.painter(), left_x, y, "PLAYBACK");
            y += SEC_HDR_H;

            // Row 8: Metronome + [ -12.0 dB ] [ Play ticks ]
            {
                let cy = y + ROW_H * 0.5;
                let ticks_w = 56.0;
                let ticks_rect = Rect::from_min_size(
                    pos2(right_x - ticks_w, cy - CTRL_H * 0.5),
                    vec2(ticks_w, CTRL_H),
                );
                let db_w = 56.0;
                let db_rect = Rect::from_min_size(
                    pos2(ticks_rect.min.x - 4.0 - db_w, cy - CTRL_H * 0.5),
                    vec2(db_w, CTRL_H),
                );

                if draw_icon_toggle_row(
                    ui,
                    key.with("pb_metro"),
                    left_x,
                    db_rect.min.x - 6.0,
                    cy,
                    state.metronome_enabled,
                    "Metronome",
                    |painter, r, fg| icons::paint_icon(painter, r, icons::METRONOME, fg, 1.35),
                ) {
                    state.metronome_enabled = !state.metronome_enabled;
                }

                let db_str = format!("{:.1} dB", state.play_menu.metronome_volume_db);
                let db_resp = draw_value_box(ui, key.with("metro_db"), db_rect, &db_str);
                if db_resp.dragged() {
                    let delta = -db_resp.drag_delta().y + db_resp.drag_delta().x * 0.4;
                    state.play_menu.metronome_volume_db =
                        (state.play_menu.metronome_volume_db + delta * 0.25).clamp(-36.0, 6.0);
                }
                if db_resp.double_clicked() {
                    state.play_menu.metronome_volume_db = -12.0;
                }

                if draw_text_toggle_button(
                    ui,
                    key.with("metro_ticks"),
                    ticks_rect,
                    "Play ticks",
                    state.play_menu.metronome_play_ticks,
                ) {
                    state.play_menu.metronome_play_ticks = !state.play_menu.metronome_play_ticks;
                }
                y += ROW_H;
            }

            // Row 9: Fill (used by Operators) + pin
            {
                let cy = y + ROW_H * 0.5;
                if draw_icon_toggle_row(
                    ui,
                    key.with("pb_fill"),
                    left_x,
                    pin_cx - 16.0,
                    cy,
                    state.play_menu.fill_mode,
                    "Fill (used by Operators)",
                    paint_fill_icon,
                ) {
                    state.play_menu.fill_mode = !state.play_menu.fill_mode;
                }
                draw_pin(
                    ui,
                    key.with("pin_fill"),
                    pos2(pin_cx, cy),
                    &mut state.play_menu.pinned,
                    PlayPinItem::FillMode,
                );
                y += ROW_H;
            }

            draw_separator(ui.painter(), rect, y);
            y += SEP_H;

            // ── RECORDING SETTINGS ────────────────────────────────────────────
            draw_section_header(ui.painter(), left_x, y, "RECORDING SETTINGS");
            y += SEC_HDR_H;

            let sq_btn_w = 22.0;
            let rec_seg_w = 148.0;
            let sq_btn_x = right_x - sq_btn_w;
            let rec_seg_x = sq_btn_x - 4.0 - rec_seg_w;

            // Row 10: Pre-roll + [ None | 1 | 2 | 4 ] + [Metronome]
            {
                let cy = y + ROW_H * 0.5;
                ui.painter().text(
                    pos2(left_x, cy),
                    Align2::LEFT_CENTER,
                    "Pre-roll",
                    fonts::regular(11.0),
                    TXT,
                );

                let seg_rect = Rect::from_min_size(
                    pos2(rec_seg_x, cy - CTRL_H * 0.5),
                    vec2(rec_seg_w, CTRL_H),
                );
                let opts = [
                    (PreRollBars::None, "None"),
                    (PreRollBars::Bars1, "1"),
                    (PreRollBars::Bars2, "2"),
                    (PreRollBars::Bars4, "4"),
                ];
                if let Some(v) = draw_segmented_group(
                    ui,
                    key.with("pre_roll_seg"),
                    seg_rect,
                    &opts,
                    state.play_menu.pre_roll,
                ) {
                    state.play_menu.pre_roll = v;
                }

                let sq_rect = Rect::from_min_size(
                    pos2(sq_btn_x, cy - CTRL_H * 0.5),
                    vec2(sq_btn_w, CTRL_H),
                );
                let parent_active = state.play_menu.pre_roll != PreRollBars::None;
                if draw_accent_icon_toggle(
                    ui,
                    key.with("pre_roll_metro"),
                    sq_rect,
                    state.play_menu.pre_roll_metronome,
                    parent_active,
                    |painter, r, fg| icons::paint_icon(painter, r, icons::METRONOME, fg, 1.3),
                ) {
                    state.play_menu.pre_roll_metronome = !state.play_menu.pre_roll_metronome;
                }
                y += ROW_H;
            }

            // Row 11: Record quantization + [ Off | 1/32 | 1/16 | 1/8 | 1/4 ] + [LEN]
            {
                let cy = y + ROW_H * 0.5;
                ui.painter().text(
                    pos2(left_x, cy),
                    Align2::LEFT_CENTER,
                    "Record quantization",
                    fonts::regular(11.0),
                    TXT,
                );

                let seg_rect = Rect::from_min_size(
                    pos2(rec_seg_x, cy - CTRL_H * 0.5),
                    vec2(rec_seg_w, CTRL_H),
                );
                let opts = [
                    (RecordQuantization::Off, "Off"),
                    (RecordQuantization::Q1_32, "1/32"),
                    (RecordQuantization::Q1_16, "1/16"),
                    (RecordQuantization::Q1_8, "1/8"),
                    (RecordQuantization::Q1_4, "1/4"),
                ];
                if let Some(v) = draw_segmented_group(
                    ui,
                    key.with("rec_quant_seg"),
                    seg_rect,
                    &opts,
                    state.play_menu.record_quantization,
                ) {
                    state.play_menu.record_quantization = v;
                }

                let sq_rect = Rect::from_min_size(
                    pos2(sq_btn_x, cy - CTRL_H * 0.5),
                    vec2(sq_btn_w, CTRL_H),
                );
                let parent_active = state.play_menu.record_quantization != RecordQuantization::Off;
                if draw_accent_icon_toggle(
                    ui,
                    key.with("rec_quant_len"),
                    sq_rect,
                    state.play_menu.record_quantize_length,
                    parent_active,
                    paint_quantize_len_icon,
                ) {
                    state.play_menu.record_quantize_length =
                        !state.play_menu.record_quantize_length;
                }
            }
        });

    let clicked_outside = ctx.input(|i| {
        i.pointer.any_pressed()
            && i.pointer.interact_pos().is_some_and(|p| {
                !area_resp.response.rect.contains(p) && !anchor_rect.contains(p)
            })
    });
    if clicked_outside {
        close(ctx, key);
    }
}

fn draw_section_header(painter: &Painter, x: f32, y: f32, title: &str) {
    painter.text(
        pos2(x, y + SEC_HDR_H * 0.5),
        Align2::LEFT_CENTER,
        title,
        fonts::semibold(9.5),
        SEC_TXT,
    );
}

fn draw_separator(painter: &Painter, menu_rect: Rect, y: f32) {
    let sep_y = y + SEP_H * 0.5;
    painter.line_segment(
        [
            pos2(menu_rect.min.x + PAD_X, sep_y),
            pos2(menu_rect.max.x - PAD_X, sep_y),
        ],
        Stroke::new(1.0_f32, SEP_COL),
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_icon_toggle_row(
    ui: &mut Ui,
    id: Id,
    left_x: f32,
    max_x: f32,
    cy: f32,
    active: bool,
    label: &str,
    paint_icon: impl FnOnce(&Painter, Rect, Color32),
) -> bool {
    let icon_rect = Rect::from_min_size(
        pos2(left_x, cy - ICON_BOX_SZ * 0.5),
        vec2(ICON_BOX_SZ, ICON_BOX_SZ),
    );
    let hit_rect = Rect::from_min_max(
        pos2(left_x, cy - ROW_H * 0.5 + 1.0),
        pos2(max_x.max(icon_rect.max.x + 12.0), cy + ROW_H * 0.5 - 1.0),
    );
    let resp = ui.interact(hit_rect, id, Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    let p = ui.painter();
    let box_fill = if active {
        BW_ACCENT_ORANGE
    } else if resp.hovered() {
        BTN_BG_HOVER
    } else {
        BTN_BG
    };
    p.rect_filled(icon_rect, 2.5, box_fill);
    p.rect_stroke(
        icon_rect,
        2.5,
        Stroke::new(1.0_f32, BTN_BORDER),
        egui::StrokeKind::Inside,
    );

    let ic_fg = if active { TXT_DARK } else { TXT };
    paint_icon(p, icon_rect.shrink(3.5), ic_fg);

    let lbl_col = if resp.hovered() { Color32::WHITE } else { TXT };
    p.text(
        pos2(icon_rect.max.x + 8.0, cy),
        Align2::LEFT_CENTER,
        label,
        fonts::regular(11.0),
        lbl_col,
    );

    resp.clicked()
}

fn draw_segmented_group<T: Copy + PartialEq>(
    ui: &mut Ui,
    id: Id,
    rect: Rect,
    options: &[(T, &str)],
    current: T,
) -> Option<T> {
    let n = options.len().max(1);
    let seg_w = rect.width() / (n as f32);
    let mut selected = None;

    // Fondo base
    ui.painter().rect_filled(rect, 2.5, BTN_BG);

    for (idx, (val, label)) in options.iter().enumerate() {
        let x0 = rect.min.x + (idx as f32) * seg_w;
        let x1 = if idx + 1 == n {
            rect.max.x
        } else {
            rect.min.x + ((idx + 1) as f32) * seg_w
        };
        let seg_rect = Rect::from_min_max(pos2(x0, rect.min.y), pos2(x1, rect.max.y));
        let resp = ui.interact(seg_rect, id.with(idx), Sense::click());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }

        let is_active = *val == current;
        let fill = if is_active {
            BW_ACCENT_ORANGE
        } else if resp.hovered() {
            BTN_BG_HOVER
        } else {
            BTN_BG
        };
        let rounding = if idx == 0 {
            egui::CornerRadius {
                nw: 2,
                sw: 2,
                ne: 0,
                se: 0,
            }
        } else if idx + 1 == n {
            egui::CornerRadius {
                nw: 0,
                sw: 0,
                ne: 2,
                se: 2,
            }
        } else {
            egui::CornerRadius::ZERO
        };
        let p = ui.painter();
        p.rect_filled(seg_rect, rounding, fill);

        let fg = if is_active { TXT_DARK } else { TXT };
        let font = if is_active {
            fonts::semibold(10.0)
        } else {
            fonts::regular(10.0)
        };
        p.text(seg_rect.center(), Align2::CENTER_CENTER, *label, font, fg);

        if idx > 0 {
            p.line_segment(
                [pos2(x0, rect.min.y + 1.0), pos2(x0, rect.max.y - 1.0)],
                Stroke::new(1.0_f32, SEG_DIVIDER),
            );
        }

        if resp.clicked() {
            selected = Some(*val);
        }
    }

    ui.painter().rect_stroke(
        rect,
        2.5,
        Stroke::new(1.0_f32, BTN_BORDER),
        egui::StrokeKind::Inside,
    );
    selected
}

fn draw_value_box(ui: &mut Ui, id: Id, rect: Rect, text: &str) -> egui::Response {
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
    }
    let p = ui.painter();
    let fill = if resp.hovered() || resp.dragged() {
        VAL_BG_HOVER
    } else {
        VAL_BG
    };
    p.rect_filled(rect, 2.5, fill);
    p.rect_stroke(
        rect,
        2.5,
        Stroke::new(1.0_f32, BTN_BORDER),
        egui::StrokeKind::Inside,
    );
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        fonts::semibold(10.5),
        TXT_DARK,
    );
    resp
}

fn draw_text_toggle_button(
    ui: &mut Ui,
    id: Id,
    rect: Rect,
    label: &str,
    active: bool,
) -> bool {
    let resp = ui.interact(rect, id, Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let p = ui.painter();
    let fill = if active {
        BW_ACCENT_ORANGE
    } else if resp.hovered() {
        BTN_BG_HOVER
    } else {
        BTN_BG
    };
    p.rect_filled(rect, 2.5, fill);
    p.rect_stroke(
        rect,
        2.5,
        Stroke::new(1.0_f32, BTN_BORDER),
        egui::StrokeKind::Inside,
    );
    let fg = if active {
        TXT_DARK
    } else if resp.hovered() {
        TXT
    } else {
        TXT_MUTED
    };
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        fonts::regular(10.0),
        fg,
    );
    resp.clicked()
}

fn draw_accent_icon_toggle(
    ui: &mut Ui,
    id: Id,
    rect: Rect,
    active: bool,
    parent_active: bool,
    paint_icon: impl FnOnce(&Painter, Rect, Color32),
) -> bool {
    let resp = ui.interact(rect, id, Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let p = ui.painter();
    let fill = if active {
        if parent_active {
            BW_ACCENT_ORANGE
        } else {
            ORANGE_DIM
        }
    } else if resp.hovered() {
        BTN_BG_HOVER
    } else {
        BTN_BG
    };
    p.rect_filled(rect, 2.5, fill);
    p.rect_stroke(
        rect,
        2.5,
        Stroke::new(1.0_f32, BTN_BORDER),
        egui::StrokeKind::Inside,
    );
    let fg = if active { TXT_DARK } else { TXT };
    paint_icon(p, rect.shrink(3.0), fg);
    resp.clicked()
}

fn draw_pin(
    ui: &mut Ui,
    id: Id,
    center: Pos2,
    pinned: &mut HashSet<PlayPinItem>,
    item: PlayPinItem,
) {
    let pin_rect = Rect::from_center_size(center, vec2(18.0, 20.0));
    let resp = ui.interact(pin_rect, id, Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let is_pinned = pinned.contains(&item);
    let col = if is_pinned {
        BW_ACCENT_ORANGE
    } else if resp.hovered() {
        Color32::WHITE
    } else {
        PIN_IDLE
    };
    icons::paint_icon(ui.painter(), Rect::from_center_size(center, vec2(12.0, 12.0)), IC_PIN, col, 1.3);
    if resp.clicked() {
        if !pinned.remove(&item) {
            pinned.insert(item);
        }
    }
}

// ── Iconos vectoriales Bitwig calibrados para el menú PLAY ────────────────────

fn paint_overdub_plus(painter: &Painter, rect: Rect, fg: Color32) {
    let c = rect.center();
    let arm = rect.width().min(rect.height()) * 0.36;
    let stroke = Stroke::new(2.2_f32, fg);
    painter.line_segment([pos2(c.x - arm, c.y), pos2(c.x + arm, c.y)], stroke);
    painter.line_segment([pos2(c.x, c.y - arm), pos2(c.x, c.y + arm)], stroke);
}

fn paint_clip_automation_write(painter: &Painter, rect: Rect, fg: Color32) {
    let box_r = rect.shrink(0.5);
    painter.rect_stroke(
        box_r,
        1.5,
        Stroke::new(1.2_f32, fg),
        egui::StrokeKind::Inside,
    );
    icons::paint_automation_nodes(painter, box_r.shrink(1.5), fg, true);
}

fn paint_clip_overdub(painter: &Painter, rect: Rect, fg: Color32) {
    let box_r = rect.shrink(0.5);
    painter.rect_stroke(
        box_r,
        1.5,
        Stroke::new(1.2_f32, fg),
        egui::StrokeKind::Inside,
    );
    let c = box_r.center();
    let arm = box_r.width().min(box_r.height()) * 0.28;
    let stroke = Stroke::new(1.6_f32, fg);
    painter.line_segment([pos2(c.x - arm, c.y), pos2(c.x + arm, c.y)], stroke);
    painter.line_segment([pos2(c.x, c.y - arm), pos2(c.x, c.y + arm)], stroke);
}

fn paint_comping_icon(painter: &Painter, rect: Rect, fg: Color32) {
    let box_r = rect.shrink(0.5);
    painter.rect_stroke(
        box_r,
        1.5,
        Stroke::new(1.2_f32, fg),
        egui::StrokeKind::Inside,
    );
    let top_y = box_r.min.y + box_r.height() * 0.34;
    let bot_y = box_r.min.y + box_r.height() * 0.68;
    painter.rect_filled(
        Rect::from_min_size(pos2(box_r.min.x + 2.5, top_y - 1.5), vec2(3.5, 3.0)),
        0.5,
        fg,
    );
    painter.rect_filled(
        Rect::from_min_size(pos2(box_r.max.x - 6.0, top_y - 1.5), vec2(3.5, 3.0)),
        0.5,
        fg,
    );
    painter.line_segment(
        [pos2(box_r.min.x + 2.5, bot_y), pos2(box_r.max.x - 2.5, bot_y)],
        Stroke::new(1.3_f32, fg),
    );
}

fn paint_scn_icon(painter: &Painter, rect: Rect, fg: Color32) {
    let box_r = rect.shrink(0.5);
    painter.rect_stroke(
        box_r,
        1.5,
        Stroke::new(1.2_f32, fg),
        egui::StrokeKind::Inside,
    );
    painter.text(
        box_r.center(),
        Align2::CENTER_CENTER,
        "Scn",
        fonts::semibold(7.2),
        fg,
    );
}

fn paint_groove_icon(painter: &Painter, rect: Rect, fg: Color32) {
    let stroke = Stroke::new(1.3_f32, fg);
    let x0 = rect.min.x + 1.5;
    let x1 = rect.max.x - 1.5;
    let xm = rect.center().x;
    for row in 0..3 {
        let y = rect.min.y + rect.height() * (0.28 + (row as f32) * 0.22);
        let bump = if row == 0 { -2.2 } else if row == 1 { -1.2 } else { 0.0 };
        painter.add(egui::Shape::line(
            vec![
                pos2(x0, y),
                pos2(x0 + (xm - x0) * 0.5, y + bump),
                pos2(xm, y + bump * 1.15),
                pos2(xm + (x1 - xm) * 0.5, y + bump),
                pos2(x1, y),
            ],
            stroke,
        ));
    }
}

fn paint_fill_icon(painter: &Painter, rect: Rect, fg: Color32) {
    // Estilo Bitwig: caja superior con pequeño notch + bloque inferior con signo '+'
    let r = rect.shrink(0.5);
    let top_h = r.height() * 0.35;
    painter.rect_stroke(
        Rect::from_min_size(pos2(r.min.x + 2.0, r.min.y), vec2(r.width() - 4.0, top_h)),
        1.0,
        Stroke::new(1.1_f32, fg),
        egui::StrokeKind::Inside,
    );
    let bot_r = Rect::from_min_max(pos2(r.min.x, r.min.y + top_h + 1.5), r.max);
    painter.rect_filled(bot_r, 1.5, fg);
    let c = bot_r.center();
    let bg_cut = Color32::from_rgb(0x2D, 0x2D, 0x2F);
    painter.line_segment(
        [pos2(c.x - 2.5, c.y), pos2(c.x + 2.5, c.y)],
        Stroke::new(1.3_f32, bg_cut),
    );
    painter.line_segment(
        [pos2(c.x, c.y - 2.5), pos2(c.x, c.y + 2.5)],
        Stroke::new(1.3_f32, bg_cut),
    );
}

fn paint_quantize_len_icon(painter: &Painter, rect: Rect, fg: Color32) {
    // Escalera de cuantización arriba + "LEN" abajo
    let stroke = Stroke::new(1.2_f32, fg);
    let top = rect.min.y + 1.0;
    let mid = rect.center().y - 0.5;
    let x0 = rect.min.x + 1.0;
    let x1 = rect.min.x + rect.width() * 0.35;
    let x2 = rect.min.x + rect.width() * 0.65;
    let x3 = rect.max.x - 1.0;
    painter.add(egui::Shape::line(
        vec![
            pos2(x0, mid),
            pos2(x1, mid),
            pos2(x1, (top + mid) * 0.5),
            pos2(x2, (top + mid) * 0.5),
            pos2(x2, top),
            pos2(x3, top),
        ],
        stroke,
    ));
    painter.text(
        pos2(rect.center().x, rect.max.y - 3.5),
        Align2::CENTER_CENTER,
        "LEN",
        fonts::semibold(6.8),
        fg,
    );
}
