// Hikaru OpenLive - Scene header context menu (Bitwig Studio 5.x look)
// crates/hikaru_gui/src/ui/scene_context_menu.rs
//
// Menú contextual de click derecho sobre la cabecera de una escena:
// paleta de colores 9x3 en bandeja oscura + separador + barra horizontal
// de acciones (CUT, COPY, PASTE, DUPL., DELETE).

use egui::{pos2, vec2, Align2, Color32, Context, Id, Order, Pos2, Rect, Sense, Shape, Stroke};

use crate::ui::fonts;
use crate::ui::icons::{self, VectorIcon};

const MENU_W: f32 = 220.0;
const PAD_X: f32 = 8.0;
const PAD_TOP: f32 = 8.0;
const PAD_BOTTOM: f32 = 6.0;
const SWATCH_W: f32 = 17.0;
const SWATCH_H: f32 = 15.0;
const SWATCH_GAP: f32 = 1.0;
const TRAY_PAD: f32 = 4.0;
const TRAY_ARROW_W: f32 = 12.0;
const SEP_H: f32 = 13.0;
const ICON_ROW_H: f32 = 42.0;

const MENU_BG: Color32 = Color32::from_rgb(0x2E, 0x2F, 0x33);
const MENU_BORDER: Color32 = Color32::from_rgb(0x18, 0x19, 0x1C);
const TRAY_BG: Color32 = Color32::from_rgb(0x1C, 0x1D, 0x20);
const SEP: Color32 = Color32::from_rgb(0x4A, 0x4C, 0x52);
const ROW_HOVER: Color32 = Color32::from_rgb(0xBE, 0xBE, 0xBE);
const TXT: Color32 = Color32::from_rgb(0xEA, 0xEA, 0xEA);
const TXT_DIM: Color32 = Color32::from_rgb(0x6C, 0x6E, 0x74);
const TXT_LABEL: Color32 = Color32::from_rgb(0xD2, 0xD4, 0xDA);
const TXT_ON_HOVER: Color32 = Color32::from_rgb(0x1A, 0x1A, 0x1A);

const fn vi(d: &'static str) -> VectorIcon {
    VectorIcon { filled: false, d }
}

const IC_COPY: VectorIcon =
    vi("M 9 9 L 20 9 L 20 20 L 9 20 L 9 9 M 15 9 L 15 4 L 4 4 L 4 15 L 9 15");
const IC_PASTE: VectorIcon =
    vi("M 5 5 L 19 5 L 19 21 L 5 21 L 5 5 M 9 3 L 15 3 L 15 7 L 9 7 L 9 3");

/// Paleta de 27 colores (9 columnas x 3 filas) estilo Bitwig Studio para escenas.
pub const SCENE_PALETTE: [[u32; 9]; 3] = [
    // Fila 1: neutros, tierra y tonos fríos suaves
    [
        0x565656, 0x7E7E7E, 0xC6C6C6, 0x868C9E, 0xA89260, 0xCCA06B, 0x6478B8, 0x8272B8, 0xA85ED8,
    ],
    // Fila 2: espectro vivo
    [
        0xE03C78, 0xE0322E, 0xFF5A00, 0xE6B018, 0x82B81C, 0x1EA648, 0x16A6A6, 0x248CE0, 0xC65CE8,
    ],
    // Fila 3: variaciones suaves / pastel
    [
        0xE87698, 0xEC7668, 0xF49A52, 0xE8CE66, 0xA4D062, 0x56C884, 0x54C8D4, 0x78B4F2, 0xD6A2E8,
    ],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneMenuAction {
    SetColor(Color32),
    Cut,
    Copy,
    Paste,
    Duplicate,
    Delete,
}

#[derive(Clone, Copy, Debug)]
pub struct SceneMenuContext {
    pub current_color: Option<Color32>,
    pub can_paste: bool,
    pub can_delete: bool,
}

/// Dibuja el menú contextual de escena en `click_pos`.
/// Devuelve `(Option<SceneMenuAction>, should_close)`.
pub fn show(
    ctx: &Context,
    key: Id,
    click_pos: Pos2,
    mc: &SceneMenuContext,
) -> (Option<SceneMenuAction>, bool) {
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        return (None, true);
    }

    let inner_w = MENU_W - PAD_X * 2.0;
    let grid_w = 9.0 * SWATCH_W + 8.0 * SWATCH_GAP;
    let grid_h = 3.0 * SWATCH_H + 2.0 * SWATCH_GAP;
    let tray_w = grid_w + TRAY_PAD * 2.0 + TRAY_ARROW_W;
    let tray_h = grid_h + TRAY_PAD * 2.0;
    let total_h = PAD_TOP + tray_h + SEP_H + ICON_ROW_H + PAD_BOTTOM;

    let screen = ctx.screen_rect();
    let origin = pos2(
        click_pos.x.min(screen.max.x - MENU_W - 6.0).max(screen.min.x + 4.0),
        click_pos.y.min(screen.max.y - total_h - 6.0).max(screen.min.y + 4.0),
    );

    let mut action: Option<SceneMenuAction> = None;
    let mut should_close = false;

    egui::Area::new(key.with("scene_ctx_area"))
        .order(Order::Foreground)
        .fixed_pos(origin)
        .sense(Sense::hover())
        .show(ctx, |ui| {
            ui.set_clip_rect(screen);
            let (rect, _) = ui.allocate_exact_size(vec2(MENU_W, total_h), Sense::hover());
            let p = ui.painter().clone();

            // Sombra sutil y cuerpo del menú
            p.rect_filled(
                rect.translate(vec2(2.0, 3.0)),
                2.0,
                Color32::from_black_alpha(95),
            );
            p.rect_filled(rect, 1.5, MENU_BG);
            p.rect_stroke(
                rect,
                1.5,
                Stroke::new(1.0_f32, MENU_BORDER),
                egui::StrokeKind::Inside,
            );

            let x0 = rect.min.x + PAD_X;
            let mut y = rect.min.y + PAD_TOP;

            // --- Bandeja de paleta 9x3 ---
            let tray_x = rect.center().x - tray_w * 0.5;
            let tray_rect = Rect::from_min_size(pos2(tray_x, y), vec2(tray_w, tray_h));
            p.rect_filled(tray_rect, 1.5, TRAY_BG);

            let grid_origin = pos2(tray_rect.min.x + TRAY_PAD, tray_rect.min.y + TRAY_PAD);
            for (r, row) in SCENE_PALETTE.iter().enumerate() {
                for (c, hex) in row.iter().enumerate() {
                    let col = Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, *hex as u8);
                    let sr = Rect::from_min_size(
                        pos2(
                            grid_origin.x + c as f32 * (SWATCH_W + SWATCH_GAP),
                            grid_origin.y + r as f32 * (SWATCH_H + SWATCH_GAP),
                        ),
                        vec2(SWATCH_W, SWATCH_H),
                    );
                    p.rect_filled(sr, 0.0, col);

                    if mc.current_color == Some(col) {
                        p.rect_stroke(
                            sr,
                            0.0,
                            Stroke::new(1.5_f32, Color32::WHITE),
                            egui::StrokeKind::Inside,
                        );
                    }

                    let resp = ui.interact(sr, key.with(("scene_sw", r, c)), Sense::click());
                    if resp.hovered() {
                        p.rect_stroke(
                            sr,
                            0.0,
                            Stroke::new(1.2_f32, Color32::WHITE),
                            egui::StrokeKind::Inside,
                        );
                    }
                    if resp.clicked() {
                        action = Some(SceneMenuAction::SetColor(col));
                        should_close = true;
                    }
                }
            }

            // Pequeño triángulo indicador en la esquina inferior derecha de la bandeja (estilo Bitwig)
            let tri_c = pos2(tray_rect.max.x - 6.0, tray_rect.max.y - 7.5);
            p.add(Shape::convex_polygon(
                vec![
                    tri_c + vec2(-2.2, -3.5),
                    tri_c + vec2(-2.2, 3.5),
                    tri_c + vec2(2.8, 0.0),
                ],
                Color32::from_rgb(0xD6, 0xD8, 0xDE),
                Stroke::NONE,
            ));

            y += tray_h;

            // --- Separador horizontal ---
            let ly = (y + SEP_H * 0.5).round() - 0.5;
            p.line_segment(
                [pos2(x0, ly), pos2(x0 + inner_w, ly)],
                Stroke::new(1.0_f32, SEP),
            );
            y += SEP_H;

            // --- Fila de 5 acciones: CUT / COPY / PASTE / DUPL. / DELETE ---
            let cell_w = inner_w / 5.0;
            let icon_items: [(VectorIcon, &str, bool, SceneMenuAction); 5] = [
                (icons::CUT, "CUT", true, SceneMenuAction::Cut),
                (IC_COPY, "COPY", true, SceneMenuAction::Copy),
                (IC_PASTE, "PASTE", mc.can_paste, SceneMenuAction::Paste),
                (icons::DUPLICATE_PLUS, "DUPL.", true, SceneMenuAction::Duplicate),
                (icons::CIRCLE_X, "DELETE", mc.can_delete, SceneMenuAction::Delete),
            ];

            for (i, (glyph, label, enabled, act)) in icon_items.iter().enumerate() {
                let cr = Rect::from_min_size(
                    pos2(x0 + i as f32 * cell_w, y),
                    vec2(cell_w, ICON_ROW_H),
                );
                let resp = ui.interact(
                    cr,
                    key.with(("scene_act", i)),
                    if *enabled { Sense::click() } else { Sense::hover() },
                );
                let hot = *enabled && resp.hovered();
                if hot {
                    p.rect_filled(cr, 2.0, ROW_HOVER);
                }
                let icon_fg = if !*enabled {
                    TXT_DIM
                } else if hot {
                    TXT_ON_HOVER
                } else {
                    TXT
                };
                let label_fg = if !*enabled {
                    TXT_DIM
                } else if hot {
                    TXT_ON_HOVER
                } else {
                    TXT_LABEL
                };
                icons::paint_icon(
                    &p,
                    Rect::from_center_size(pos2(cr.center().x, cr.min.y + 14.0), vec2(15.0, 15.0)),
                    *glyph,
                    icon_fg,
                    1.5,
                );
                p.text(
                    pos2(cr.center().x, cr.min.y + 32.0),
                    Align2::CENTER_CENTER,
                    *label,
                    fonts::semibold(9.0),
                    label_fg,
                );
                if resp.clicked() {
                    action = Some(*act);
                    should_close = true;
                }
            }

            if ctx.input(|i| i.pointer.any_pressed()) {
                if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                    if !rect.contains(pos) {
                        should_close = true;
                    }
                }
            }
        });

    (action, should_close)
}
