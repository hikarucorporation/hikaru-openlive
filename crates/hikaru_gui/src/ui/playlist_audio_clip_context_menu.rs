// Hikaru OpenLive - Playlist / Arranger Audio Clip context menu (Bitwig Studio 5.x look)
// crates/hikaru_gui/src/ui/playlist_audio_clip_context_menu.rs

use egui::{pos2, vec2, Align2, Color32, Context, Id, Order, Pos2, Rect, Sense, Stroke, Ui};

use crate::ui::fonts;
use crate::ui::icons::{self, VectorIcon};

const LEFT_COL_W: f32 = 246.0;
const RIGHT_COL_W: f32 = 216.0;
const COL_GAP: f32 = 6.0;
const PAD_X: f32 = 6.0;
const PAD_Y: f32 = 5.0;
const ROW_H: f32 = 22.0;
const SEP_H: f32 = 9.0;
const PALETTE_SWATCH_W: f32 = 18.0;
const PALETTE_SWATCH_H: f32 = 16.0;
const PALETTE_GAP: f32 = 1.0;
const ACTION_BAR_H: f32 = 38.0;

const MENU_BG: Color32 = Color32::from_rgb(0x32, 0x32, 0x32);
const MENU_BORDER: Color32 = Color32::from_rgb(0x1C, 0x1C, 0x1C);
const SEP: Color32 = Color32::from_rgb(0x4A, 0x4A, 0x4A);
const ROW_HOVER: Color32 = Color32::from_rgb(0xBE, 0xBE, 0xBE);
const ACTION_HOVER: Color32 = Color32::from_rgb(0x46, 0x46, 0x46);
const TXT: Color32 = Color32::from_rgb(0xEA, 0xEA, 0xEA);
const TXT_DISABLED: Color32 = Color32::from_rgb(0x74, 0x74, 0x74);
const TXT_HINT: Color32 = Color32::from_rgb(0x98, 0x98, 0x98);
const TXT_ON_HOVER: Color32 = Color32::from_rgb(0x1A, 0x1A, 0x1A);

/// 3×9 Bitwig Studio arranger clip color palette
pub const BITWIG_ARRANGER_PALETTE: [[Color32; 9]; 3] = [
    [
        Color32::from_rgb(0x4E, 0x50, 0x56),
        Color32::from_rgb(0x76, 0x79, 0x82),
        Color32::from_rgb(0xC8, 0xCA, 0xD0),
        Color32::from_rgb(0x88, 0x8C, 0xA8),
        Color32::from_rgb(0xA6, 0x8B, 0x74),
        Color32::from_rgb(0xC8, 0xA8, 0x82),
        Color32::from_rgb(0x52, 0x72, 0x92),
        Color32::from_rgb(0x76, 0x58, 0x96),
        Color32::from_rgb(0xA6, 0x52, 0xD4),
    ],
    [
        Color32::from_rgb(0xD8, 0x36, 0x48),
        Color32::from_rgb(0xE8, 0x4A, 0x24),
        Color32::from_rgb(0xF0, 0x7E, 0x18),
        Color32::from_rgb(0xE8, 0xBA, 0x1C),
        Color32::from_rgb(0x84, 0xB8, 0x24),
        Color32::from_rgb(0x28, 0xAA, 0x48),
        Color32::from_rgb(0x24, 0xA8, 0x9E),
        Color32::from_rgb(0x24, 0x88, 0xD8),
        Color32::from_rgb(0x76, 0x54, 0xD8),
    ],
    [
        Color32::from_rgb(0xE4, 0x72, 0x82),
        Color32::from_rgb(0xF0, 0x86, 0x62),
        Color32::from_rgb(0xF4, 0xA6, 0x54),
        Color32::from_rgb(0xF0, 0xD4, 0x5C),
        Color32::from_rgb(0xA8, 0xD2, 0x64),
        Color32::from_rgb(0x5C, 0xC8, 0x84),
        Color32::from_rgb(0x58, 0xC4, 0xBE),
        Color32::from_rgb(0x62, 0xB6, 0xF0),
        Color32::from_rgb(0xBA, 0x86, 0xF0),
    ],
];

const fn vi(d: &'static str) -> VectorIcon {
    VectorIcon { filled: false, d }
}

const IC_CUT: VectorIcon = vi("M 7 7 L 19 19 M 19 7 L 7 19 M 6 6 L 8 8 M 6 18 L 8 16");
const IC_COPY: VectorIcon = vi("M 9 9 L 19 9 L 19 19 L 9 19 Z M 5 15 L 5 5 L 15 5");
const IC_PASTE: VectorIcon = vi("M 8 5 L 16 5 L 16 8 L 8 8 Z M 6 7 L 18 7 L 18 20 L 6 20 Z");
const IC_DUPLICATE: VectorIcon = vi("M 5 5 L 15 5 L 15 15 L 5 15 Z M 9 9 L 19 9 L 19 19 L 9 19 Z M 14 12 L 14 16 M 12 14 L 16 14");
const IC_LOOP_REGION: VectorIcon = vi("M 4 7 L 20 7 L 20 17 L 4 17 Z M 9 10 L 15 10 M 13 8 L 15 10 L 13 12 M 15 14 L 9 14 M 11 12 L 9 14 L 11 16");
const IC_CONSOLIDATE: VectorIcon = vi("M 4 6 L 20 6 L 20 18 L 4 18 Z M 12 6 L 12 18");
const IC_DOUBLE_CONTENT: VectorIcon = vi("M 4 6 L 20 6 L 20 18 L 4 18 Z M 7 10 L 11 14 M 11 10 L 7 14 M 14 10 L 17 10 L 17 12 L 14 12 L 14 14 L 17 14");
const IC_SCALE_EACH_50: VectorIcon = vi("M 4 8 L 4 16 M 7 8 L 7 16 M 10 8 L 10 16 M 19 12 L 14 12 M 16 9 L 14 12 L 16 15");
const IC_SCALE_EACH_200: VectorIcon = vi("M 4 8 L 4 16 M 7 8 L 7 16 M 10 8 L 10 16 M 14 12 L 19 12 M 17 9 L 19 12 L 17 15");
const IC_SCALE_50: VectorIcon = vi("M 4 7 L 4 17 M 7 7 L 7 17 M 10 7 L 10 17 M 13 7 L 13 17 M 20 12 L 15 12 M 17 9 L 15 12 L 17 15");
const IC_SCALE_200: VectorIcon = vi("M 4 7 L 4 17 M 7 7 L 7 17 M 10 7 L 10 17 M 13 7 L 13 17 M 15 12 L 20 12 M 18 9 L 20 12 L 18 15");
const IC_SCALE_CUSTOM: VectorIcon = vi("M 4 7 L 4 17 M 7 7 L 7 17 M 10 7 L 10 17 M 14 16 L 20 8 M 14 9 L 15 9 M 19 15 L 20 15");
const IC_NORMALIZE: VectorIcon = vi("M 4 5 L 20 5 M 4 19 L 20 19 M 12 7 L 12 17 M 9 10 L 12 7 L 15 10 M 9 14 L 12 17 L 15 14");
const IC_BOUNCE_IN_PLACE: VectorIcon = vi("M 5 13 L 9 8 L 14 8 L 17 11 L 17 16 M 14 13 L 17 16 L 20 13 M 4 19 L 20 19");
const IC_BOUNCE: VectorIcon = vi("M 6 14 L 10 9 L 15 9 L 18 12 L 18 18 M 15 15 L 18 18 L 21 15");
const IC_SLICE_IN_PLACE: VectorIcon = vi("M 4 12 L 20 12 M 8 6 L 8 18 M 14 6 L 14 18");
const IC_SLICE_DRUM: VectorIcon = vi("M 4 8 L 8 8 M 4 12 L 10 12 M 4 16 L 8 16 M 12 12 L 19 12 M 16 9 L 19 12 L 16 15");
const IC_SLICE_MULTI: VectorIcon = vi("M 5 17 L 5 9 M 8 17 L 8 6 M 11 17 L 11 11 M 14 13 L 20 13 M 17 10 L 20 13 L 17 16");
const IC_FOLD_TAKES: VectorIcon = vi("M 4 14 L 20 14 M 4 18 L 20 18 M 12 5 L 12 11 M 9 8 L 12 11 L 15 8");
const IC_RESET_FADES: VectorIcon = vi("M 4 17 L 10 7 L 14 7 L 20 17 M 15 13 L 20 18 M 20 13 L 15 18");
const IC_AUTO_FADE: VectorIcon = vi("M 4 18 L 18 18 L 18 6 Z");
const IC_AUTO_CROSSFADE: VectorIcon = vi("M 4 18 L 19 6 L 19 18 L 4 6 Z");
const IC_QUANTIZE: VectorIcon = vi("M 4 17 L 9 17 L 9 13 L 14 13 L 14 9 L 20 9");
const IC_QUANTIZE_OPTS: VectorIcon = vi("M 4 17 L 9 17 L 9 12 L 14 12 L 14 7 L 19 7 M 5 7 L 9 7 M 7 5 L 7 9");
const IC_QUANTIZE_AUDIO: VectorIcon = vi("M 4 6 L 4 18 M 8 9 L 8 15 M 12 5 L 12 19 M 16 8 L 16 16 M 20 6 L 20 18");
const IC_MAKE_LEGATO: VectorIcon = vi("M 6 15 L 6 7 L 15 5 L 15 13 M 4 15 L 6 15 M 13 13 L 15 13 M 6 18 L 18 18");
const IC_SAVE_LIB: VectorIcon = vi("M 4 12 L 15 12 M 12 9 L 15 12 L 12 15 M 16 6 L 20 6 L 20 18 L 16 18");

#[derive(Clone, Copy, Debug)]
pub struct PlaylistAudioClipMenuData {
    pub pos: Pos2,
    pub clip_id: usize,
    pub click_tick: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct PlaylistAudioClipMenuState {
    pub current_color: Color32,
    pub muted: bool,
    pub looping: bool,
    pub has_clipboard: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaylistAudioClipMenuAction {
    Recolor(Color32),
    ToggleMute,
    ToggleLoop,
    Cut,
    Copy,
    Paste,
    Duplicate,
    Delete,
    LoopSelectedRegion,
    Consolidate,
    DoubleContent,
    ScaleEach50,
    ScaleEach200,
    Scale50,
    Scale200,
    ScaleCustom,
    Normalize,
    BounceInPlace,
    Bounce,
    SliceInPlace,
    SliceToDrumMachine,
    SliceToMultisample,
    FoldToTakes,
    ResetFades,
    AutoFade,
    AutoCrossfade,
    TransposeOctaveUp,
    TransposeOctaveDown,
    Quantize,
    QuantizeOptions,
    QuantizeAudio,
    QuantizeAudioOptions,
    MakeLegato,
    SaveArrangerClipToLibrary,
}

pub fn open(ctx: &Context, key: Id, data: PlaylistAudioClipMenuData) {
    ctx.data_mut(|d| {
        d.insert_temp(key.with("open"), true);
        d.insert_temp(key.with("data"), data);
    });
}

pub fn close(ctx: &Context, key: Id) {
    ctx.data_mut(|d| d.insert_temp(key.with("open"), false));
}

pub fn get_data(ctx: &Context, key: Id) -> Option<PlaylistAudioClipMenuData> {
    ctx.data(|d| d.get_temp::<PlaylistAudioClipMenuData>(key.with("data")))
}

pub fn is_open(ctx: &Context, key: Id) -> bool {
    ctx.data(|d| d.get_temp::<bool>(key.with("open")))
        .unwrap_or(false)
}

pub fn show(
    ctx: &Context,
    key: Id,
    menu_state: PlaylistAudioClipMenuState,
) -> Option<PlaylistAudioClipMenuAction> {
    if !is_open(ctx, key) {
        return None;
    }

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close(ctx, key);
        return None;
    }

    let raw_pos = get_data(ctx, key)
        .map(|d| d.pos)
        .unwrap_or(pos2(120.0, 120.0));

    let total_w = LEFT_COL_W + COL_GAP + RIGHT_COL_W + PAD_X * 2.0;
    // Palette (54) + 2 toggles (44) + action bar (38) + 22 rows (484) + 10 seps (90) + padding
    let total_h = 54.0 + 2.0 * ROW_H + ACTION_BAR_H + 22.0 * ROW_H + 10.0 * SEP_H + PAD_Y * 2.0;

    let scr = ctx.screen_rect();
    let pos = pos2(
        raw_pos.x.min((scr.max.x - total_w - 6.0).max(scr.min.x + 6.0)),
        raw_pos.y.min((scr.max.y - total_h - 6.0).max(scr.min.y + 6.0)),
    );

    let mut chosen: Option<PlaylistAudioClipMenuAction> = None;
    let mut keep_open = false;

    let area_resp = egui::Area::new(key.with("area"))
        .order(Order::Foreground)
        .fixed_pos(pos)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(MENU_BG)
                .stroke(Stroke::new(1.0_f32, MENU_BORDER))
                .corner_radius(egui::CornerRadius::same(2))
                .shadow(egui::Shadow {
                    offset: [0, 6],
                    blur: 18,
                    spread: 2,
                    color: Color32::from_black_alpha(175),
                })
                .inner_margin(egui::Margin::symmetric(PAD_X as i8, PAD_Y as i8))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);

                    ui.horizontal_top(|ui| {
                        // =================================================
                        // LEFT COLUMN
                        // =================================================
                        ui.vertical(|ui| {
                            ui.set_width(LEFT_COL_W);

                            if let Some(col) =
                                draw_color_palette(ui, menu_state.current_color)
                            {
                                chosen = Some(PlaylistAudioClipMenuAction::Recolor(col));
                            }

                            draw_sep(ui, LEFT_COL_W);

                            if draw_toggle_row(ui, LEFT_COL_W, "Muted", menu_state.muted, false) {
                                chosen = Some(PlaylistAudioClipMenuAction::ToggleMute);
                                keep_open = true;
                            }
                            if draw_toggle_row(ui, LEFT_COL_W, "Looping", menu_state.looping, true)
                            {
                                chosen = Some(PlaylistAudioClipMenuAction::ToggleLoop);
                                keep_open = true;
                            }

                            draw_sep(ui, LEFT_COL_W);

                            if let Some(act) =
                                draw_action_bar(ui, LEFT_COL_W, menu_state.has_clipboard)
                            {
                                chosen = Some(act);
                            }

                            draw_sep(ui, LEFT_COL_W);

                            if draw_row(
                                ui,
                                LEFT_COL_W,
                                Some(&IC_LOOP_REGION),
                                None,
                                "Loop Selected Region",
                                Some("Ctrl+L"),
                                true,
                            ) {
                                chosen = Some(PlaylistAudioClipMenuAction::LoopSelectedRegion);
                            }

                            draw_sep(ui, LEFT_COL_W);

                            if draw_row(
                                ui,
                                LEFT_COL_W,
                                Some(&IC_CONSOLIDATE),
                                None,
                                "Consolidate",
                                Some("Ctrl+J"),
                                true,
                            ) {
                                chosen = Some(PlaylistAudioClipMenuAction::Consolidate);
                            }

                            draw_sep(ui, LEFT_COL_W);

                            if draw_row(
                                ui,
                                LEFT_COL_W,
                                Some(&IC_DOUBLE_CONTENT),
                                None,
                                "Double Content",
                                None,
                                true,
                            ) {
                                chosen = Some(PlaylistAudioClipMenuAction::DoubleContent);
                            }
                            if draw_row(
                                ui,
                                LEFT_COL_W,
                                Some(&IC_SCALE_EACH_50),
                                None,
                                "Scale Each 50%",
                                None,
                                true,
                            ) {
                                chosen = Some(PlaylistAudioClipMenuAction::ScaleEach50);
                            }
                            if draw_row(
                                ui,
                                LEFT_COL_W,
                                Some(&IC_SCALE_EACH_200),
                                None,
                                "Scale Each 200%",
                                None,
                                true,
                            ) {
                                chosen = Some(PlaylistAudioClipMenuAction::ScaleEach200);
                            }
                            if draw_row(
                                ui,
                                LEFT_COL_W,
                                Some(&IC_SCALE_50),
                                None,
                                "Scale 50%",
                                None,
                                true,
                            ) {
                                chosen = Some(PlaylistAudioClipMenuAction::Scale50);
                            }
                            if draw_row(
                                ui,
                                LEFT_COL_W,
                                Some(&IC_SCALE_200),
                                None,
                                "Scale 200%",
                                None,
                                true,
                            ) {
                                chosen = Some(PlaylistAudioClipMenuAction::Scale200);
                            }
                            if draw_row(
                                ui,
                                LEFT_COL_W,
                                Some(&IC_SCALE_CUSTOM),
                                None,
                                "Scale...",
                                None,
                                true,
                            ) {
                                chosen = Some(PlaylistAudioClipMenuAction::ScaleCustom);
                            }

                            draw_sep(ui, LEFT_COL_W);

                            if draw_row(
                                ui,
                                LEFT_COL_W,
                                Some(&IC_NORMALIZE),
                                None,
                                "Normalize",
                                None,
                                true,
                            ) {
                                chosen = Some(PlaylistAudioClipMenuAction::Normalize);
                            }

                            draw_sep(ui, LEFT_COL_W);

                            if draw_row(
                                ui,
                                LEFT_COL_W,
                                Some(&IC_BOUNCE_IN_PLACE),
                                None,
                                "Bounce In Place",
                                Some("Ctrl+B"),
                                true,
                            ) {
                                chosen = Some(PlaylistAudioClipMenuAction::BounceInPlace);
                            }
                            if draw_row(
                                ui,
                                LEFT_COL_W,
                                Some(&IC_BOUNCE),
                                None,
                                "Bounce",
                                None,
                                true,
                            ) {
                                chosen = Some(PlaylistAudioClipMenuAction::Bounce);
                            }

                            draw_sep(ui, LEFT_COL_W);

                            if draw_row(
                                ui,
                                LEFT_COL_W,
                                Some(&IC_SLICE_IN_PLACE),
                                None,
                                "Slice In Place...",
                                None,
                                true,
                            ) {
                                chosen = Some(PlaylistAudioClipMenuAction::SliceInPlace);
                            }
                            if draw_row(
                                ui,
                                LEFT_COL_W,
                                Some(&IC_SLICE_DRUM),
                                None,
                                "Slice to Drum Machine...",
                                Some("Ctrl+Alt+D"),
                                true,
                            ) {
                                chosen = Some(PlaylistAudioClipMenuAction::SliceToDrumMachine);
                            }
                            if draw_row(
                                ui,
                                LEFT_COL_W,
                                Some(&IC_SLICE_MULTI),
                                None,
                                "Slice to Multisample...",
                                None,
                                true,
                            ) {
                                chosen = Some(PlaylistAudioClipMenuAction::SliceToMultisample);
                            }
                            if draw_row(
                                ui,
                                LEFT_COL_W,
                                Some(&IC_FOLD_TAKES),
                                None,
                                "Fold to Takes...",
                                None,
                                true,
                            ) {
                                chosen = Some(PlaylistAudioClipMenuAction::FoldToTakes);
                            }

                            draw_sep(ui, LEFT_COL_W);

                            if draw_row(
                                ui,
                                LEFT_COL_W,
                                Some(&IC_RESET_FADES),
                                None,
                                "Reset Fades",
                                Some("Ctrl+Alt+F"),
                                true,
                            ) {
                                chosen = Some(PlaylistAudioClipMenuAction::ResetFades);
                            }
                            if draw_row(
                                ui,
                                LEFT_COL_W,
                                Some(&IC_AUTO_FADE),
                                None,
                                "Auto-Fade",
                                Some("Ctrl+F"),
                                true,
                            ) {
                                chosen = Some(PlaylistAudioClipMenuAction::AutoFade);
                            }
                            if draw_row(
                                ui,
                                LEFT_COL_W,
                                Some(&IC_AUTO_CROSSFADE),
                                None,
                                "Auto-Crossfade",
                                Some("Ctrl+Shift+F"),
                                true,
                            ) {
                                chosen = Some(PlaylistAudioClipMenuAction::AutoCrossfade);
                            }

                            draw_sep(ui, LEFT_COL_W);

                            if draw_row(
                                ui,
                                LEFT_COL_W,
                                None,
                                Some("8va"),
                                "Transpose an Octave Up",
                                Some("Shift+Alt++"),
                                true,
                            ) {
                                chosen = Some(PlaylistAudioClipMenuAction::TransposeOctaveUp);
                            }
                            if draw_row(
                                ui,
                                LEFT_COL_W,
                                None,
                                Some("8vb"),
                                "Transpose an Octave Down",
                                Some("Shift+Alt+-"),
                                true,
                            ) {
                                chosen = Some(PlaylistAudioClipMenuAction::TransposeOctaveDown);
                            }
                        });

                        ui.add_space(COL_GAP);

                        // =================================================
                        // RIGHT COLUMN
                        // =================================================
                        ui.vertical(|ui| {
                            ui.set_width(RIGHT_COL_W);

                            if draw_row(
                                ui,
                                RIGHT_COL_W,
                                Some(&IC_QUANTIZE),
                                None,
                                "Quantize",
                                Some("Q"),
                                true,
                            ) {
                                chosen = Some(PlaylistAudioClipMenuAction::Quantize);
                            }
                            if draw_row(
                                ui,
                                RIGHT_COL_W,
                                Some(&IC_QUANTIZE_OPTS),
                                None,
                                "Quantize...",
                                Some("Alt+Q"),
                                true,
                            ) {
                                chosen = Some(PlaylistAudioClipMenuAction::QuantizeOptions);
                            }
                            if draw_row(
                                ui,
                                RIGHT_COL_W,
                                Some(&IC_QUANTIZE_AUDIO),
                                None,
                                "Quantize Audio",
                                None,
                                true,
                            ) {
                                chosen = Some(PlaylistAudioClipMenuAction::QuantizeAudio);
                            }
                            if draw_row(
                                ui,
                                RIGHT_COL_W,
                                Some(&IC_QUANTIZE_AUDIO),
                                None,
                                "Quantize Audio...",
                                None,
                                true,
                            ) {
                                chosen = Some(PlaylistAudioClipMenuAction::QuantizeAudioOptions);
                            }
                            if draw_row(
                                ui,
                                RIGHT_COL_W,
                                Some(&IC_MAKE_LEGATO),
                                None,
                                "Make Legato",
                                Some("Ctrl+Shift+Alt+L"),
                                false,
                            ) {
                                chosen = Some(PlaylistAudioClipMenuAction::MakeLegato);
                            }

                            draw_sep(ui, RIGHT_COL_W);

                            if draw_row(
                                ui,
                                RIGHT_COL_W,
                                Some(&IC_SAVE_LIB),
                                None,
                                "Save Arranger clip To Library",
                                None,
                                false,
                            ) {
                                chosen =
                                    Some(PlaylistAudioClipMenuAction::SaveArrangerClipToLibrary);
                            }
                        });
                    });
                });
        });

    if chosen.is_some() && !keep_open {
        close(ctx, key);
        return chosen;
    }

    let menu_rect = area_resp.response.rect;
    let clicked_outside = ctx.input(|i| {
        if i.pointer.any_pressed() {
            if let Some(p) = i.pointer.interact_pos() {
                return !menu_rect.contains(p);
            }
        }
        false
    });
    if clicked_outside {
        close(ctx, key);
    }

    chosen
}

fn draw_color_palette(ui: &mut Ui, current_color: Color32) -> Option<Color32> {
    let grid_w = 9.0 * PALETTE_SWATCH_W + 8.0 * PALETTE_GAP;
    let grid_h = 3.0 * PALETTE_SWATCH_H + 2.0 * PALETTE_GAP;
    let (row_rect, _) = ui.allocate_exact_size(vec2(LEFT_COL_W, grid_h + 4.0), Sense::hover());

    let start_x = row_rect.center().x - (grid_w + 10.0) * 0.5;
    let start_y = row_rect.min.y + 2.0;

    let mut picked = None;
    let painter = ui.painter();

    // Subtle dark backing behind swatches
    let grid_bg = Rect::from_min_size(pos2(start_x - 1.0, start_y - 1.0), vec2(grid_w + 2.0, grid_h + 2.0));
    painter.rect_filled(grid_bg, 1.0, Color32::from_rgb(0x1E, 0x1E, 0x1E));

    for (r, row) in BITWIG_ARRANGER_PALETTE.iter().enumerate() {
        for (c, &col) in row.iter().enumerate() {
            let x = start_x + c as f32 * (PALETTE_SWATCH_W + PALETTE_GAP);
            let y = start_y + r as f32 * (PALETTE_SWATCH_H + PALETTE_GAP);
            let sw_rect = Rect::from_min_size(pos2(x, y), vec2(PALETTE_SWATCH_W, PALETTE_SWATCH_H));
            let resp = ui.interact(
                sw_rect,
                ui.id().with(("pl_clip_pal", r, c)),
                Sense::click(),
            );
            painter.rect_filled(sw_rect, 0.0, col);

            let is_selected = col == current_color;
            if resp.hovered() || is_selected {
                painter.rect_stroke(
                    sw_rect.shrink(0.5),
                    0.0,
                    Stroke::new(if is_selected { 1.5_f32 } else { 1.0_f32 }, Color32::WHITE),
                    egui::StrokeKind::Inside,
                );
            }
            if resp.clicked() {
                picked = Some(col);
            }
        }
    }

    // Small Bitwig right-arrow indicator next to bottom-right swatch
    let tri_x = start_x + grid_w + 4.0;
    let tri_y = start_y + grid_h - 6.0;
    painter.add(egui::Shape::convex_polygon(
        vec![
            pos2(tri_x, tri_y - 3.5),
            pos2(tri_x + 4.5, tri_y),
            pos2(tri_x, tri_y + 3.5),
        ],
        Color32::from_rgb(0xC0, 0xC0, 0xC0),
        Stroke::NONE,
    ));

    picked
}

fn draw_toggle_row(ui: &mut Ui, width: f32, label: &str, active: bool, with_check: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::click());
    let hovered = resp.hovered();
    let painter = ui.painter();

    if hovered {
        painter.rect_filled(rect, 2.0, ROW_HOVER);
    }

    let fg = if hovered { TXT_ON_HOVER } else { TXT };
    let circle_center = pos2(rect.min.x + 12.0, rect.center().y);
    let r = 5.5;

    if active {
        let circle_fill = if hovered {
            Color32::from_rgb(0x3A, 0x3A, 0x3A)
        } else {
            Color32::from_rgb(0xC8, 0xC8, 0xC8)
        };
        painter.circle_filled(circle_center, r, circle_fill);
        if with_check {
            let chk_col = if hovered { Color32::WHITE } else { Color32::from_rgb(0x22, 0x22, 0x22) };
            painter.line_segment(
                [
                    pos2(circle_center.x - 2.6, circle_center.y + 0.2),
                    pos2(circle_center.x - 0.6, circle_center.y + 2.2),
                ],
                Stroke::new(1.5_f32, chk_col),
            );
            painter.line_segment(
                [
                    pos2(circle_center.x - 0.6, circle_center.y + 2.2),
                    pos2(circle_center.x + 2.8, circle_center.y - 2.0),
                ],
                Stroke::new(1.5_f32, chk_col),
            );
        }
    } else {
        let dot_col = if hovered {
            Color32::from_rgb(0x55, 0x55, 0x55)
        } else {
            Color32::from_rgb(0x58, 0x58, 0x58)
        };
        painter.circle_filled(circle_center, r, dot_col);
    }

    painter.text(
        pos2(rect.min.x + 24.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        fonts::regular(12.0),
        fg,
    );

    resp.clicked()
}

fn draw_action_bar(
    ui: &mut Ui,
    width: f32,
    has_clipboard: bool,
) -> Option<PlaylistAudioClipMenuAction> {
    let (bar_rect, _) = ui.allocate_exact_size(vec2(width, ACTION_BAR_H), Sense::hover());
    let items: [(&VectorIcon, &str, PlaylistAudioClipMenuAction, bool); 5] = [
        (&IC_CUT, "CUT", PlaylistAudioClipMenuAction::Cut, true),
        (&IC_COPY, "COPY", PlaylistAudioClipMenuAction::Copy, true),
        (
            &IC_PASTE,
            "PASTE",
            PlaylistAudioClipMenuAction::Paste,
            has_clipboard,
        ),
        (
            &IC_DUPLICATE,
            "DUPL.",
            PlaylistAudioClipMenuAction::Duplicate,
            true,
        ),
        (
            &icons::CIRCLE_X,
            "DELETE",
            PlaylistAudioClipMenuAction::Delete,
            true,
        ),
    ];

    let btn_w = (width - 8.0) / 5.0;
    let mut clicked = None;

    for (i, (icon, label, action, enabled)) in items.iter().enumerate() {
        let bx = bar_rect.min.x + 4.0 + i as f32 * btn_w;
        let btn_rect = Rect::from_min_size(pos2(bx, bar_rect.min.y + 2.0), vec2(btn_w, ACTION_BAR_H - 4.0));
        let sense = if *enabled { Sense::click() } else { Sense::hover() };
        let resp = ui.interact(btn_rect, ui.id().with(("pl_act_btn", i)), sense);

        if *enabled && resp.hovered() {
            ui.painter().rect_filled(btn_rect, 2.0, ACTION_HOVER);
        }

        let fg = if *enabled { TXT } else { TXT_DISABLED };
        let icon_rect = Rect::from_center_size(pos2(btn_rect.center().x, btn_rect.min.y + 11.0), vec2(14.0, 14.0));
        icons::paint_icon(ui.painter(), icon_rect, **icon, fg, 1.3_f32);

        ui.painter().text(
            pos2(btn_rect.center().x, btn_rect.max.y - 6.0),
            Align2::CENTER_CENTER,
            *label,
            fonts::regular(8.5),
            fg,
        );

        if *enabled && resp.clicked() {
            clicked = Some(*action);
        }
    }

    clicked
}

fn draw_sep(ui: &mut Ui, width: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, SEP_H), Sense::hover());
    let y = rect.center().y.round() + 0.5;
    ui.painter().line_segment(
        [pos2(rect.min.x + 4.0, y), pos2(rect.max.x - 4.0, y)],
        Stroke::new(1.0_f32, SEP),
    );
}

fn draw_row(
    ui: &mut Ui,
    width: f32,
    icon: Option<&VectorIcon>,
    text_badge: Option<&str>,
    label: &str,
    shortcut: Option<&str>,
    enabled: bool,
) -> bool {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW_H), sense);
    let hovered = enabled && resp.hovered();
    let painter = ui.painter();

    if hovered {
        painter.rect_filled(rect, 2.0, ROW_HOVER);
    }

    let fg = if !enabled {
        TXT_DISABLED
    } else if hovered {
        TXT_ON_HOVER
    } else {
        TXT
    };

    if let Some(ic) = icon {
        let icon_rect =
            Rect::from_center_size(pos2(rect.min.x + 12.0, rect.center().y), vec2(13.0, 13.0));
        icons::paint_icon(painter, icon_rect, *ic, fg, 1.25_f32);
    } else if let Some(badge) = text_badge {
        painter.text(
            pos2(rect.min.x + 12.0, rect.center().y),
            Align2::CENTER_CENTER,
            badge,
            fonts::regular(9.5),
            fg,
        );
    }

    painter.text(
        pos2(rect.min.x + 24.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        fonts::regular(12.0),
        fg,
    );

    if let Some(sc) = shortcut {
        let sc_col = if !enabled {
            TXT_DISABLED
        } else if hovered {
            TXT_ON_HOVER
        } else {
            TXT_HINT
        };
        painter.text(
            pos2(rect.max.x - 6.0, rect.center().y),
            Align2::RIGHT_CENTER,
            sc,
            fonts::regular(11.0),
            sc_col,
        );
    }

    enabled && resp.clicked()
}
