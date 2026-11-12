// Hikaru OpenLive - Track header context menu (Bitwig Studio 5.x look)
// crates/hikaru_gui/src/ui/track_context_menu.rs
//
// Menú de click derecho sobre la cabecera de pista. Se pinta a mano (sin
// egui::Menu) para clavar las medidas del original: ancho 260px, filas de
// 25px, separadores de 1px con 6px de aire, paleta 8x3 y submenú Monitor.

use egui::{pos2, vec2, Align2, Color32, Context, Id, Order, Pos2, Rect, Sense, Stroke, Ui};

use crate::ui::fonts;
use crate::ui::icons::{self, VectorIcon};

const MENU_W: f32 = 260.0;
const PAD_X: f32 = 6.0;
const ROW_H: f32 = 25.0;
const SEP_H: f32 = 13.0;
const SWATCH: f32 = 19.5;
const SWATCH_GAP: f32 = 4.5;
const fn vi(d: &'static str) -> VectorIcon {
    VectorIcon { filled: false, d }
}
const IC_COPY: VectorIcon = vi("M 9 9 L 20 9 L 20 20 L 9 20 L 9 9 M 15 9 L 15 4 L 4 4 L 4 15 L 9 15");
const IC_PASTE: VectorIcon = vi("M 5 5 L 19 5 L 19 21 L 5 21 L 5 5 M 9 3 L 15 3 L 15 7 L 9 7 L 9 3");
const IC_RENAME: VectorIcon = vi("M 12 4 L 12 20 M 8 4 L 16 4 M 8 20 L 16 20");
const IC_ACTIVE: VectorIcon = vi("M 12 3 L 12 11 M 7 7 L 5 12 L 7 17 L 12 20 L 17 17 L 19 12 L 17 7");
const IC_GROUP: VectorIcon = vi("M 4 4 L 20 4 L 20 20 L 4 20 L 4 4 M 4 12 L 20 12 M 12 4 L 12 20");
const IC_UNGROUP: VectorIcon = vi("M 4 4 L 11 4 L 11 11 L 4 11 L 4 4 M 13 13 L 20 13 L 20 20 L 13 20 L 13 13");
const IC_ENTER: VectorIcon = vi("M 6 4 L 6 16 L 18 16 M 13 11 L 18 16 L 13 21");
const IC_EXIT: VectorIcon = vi("M 18 20 L 18 8 L 6 8 M 11 3 L 6 8 L 11 13");
const IC_INST: VectorIcon = vi("M 4 6 L 20 6 L 20 18 L 4 18 L 4 6 M 8 6 L 8 18 M 12 6 L 12 18 M 16 6 L 16 18");
const IC_AUDIO: VectorIcon = vi("M 3 12 L 6 12 L 8 6 L 11 18 L 14 4 L 16 20 L 18 12 L 21 12");
const IC_HYBRID: VectorIcon = vi("M 4 6 L 4 18 M 8 4 L 8 20 M 12 8 L 12 16 M 16 5 L 16 19 M 20 9 L 20 15");

const ICON_ROW_H: f32 = 44.0;

const MENU_BG: Color32 = Color32::from_rgb(0x32, 0x32, 0x32);
const MENU_BORDER: Color32 = Color32::from_rgb(0x1C, 0x1C, 0x1C);
const SEP: Color32 = Color32::from_rgb(0x4A, 0x4A, 0x4A);
const ROW_HOVER: Color32 = Color32::from_rgb(0xBE, 0xBE, 0xBE);
const TXT: Color32 = Color32::from_rgb(0xEA, 0xEA, 0xEA);
const TXT_DIM: Color32 = Color32::from_rgb(0x70, 0x70, 0x70);
const TXT_HINT: Color32 = Color32::from_rgb(0x8E, 0x8E, 0x8E);
const TXT_ON_HOVER: Color32 = Color32::from_rgb(0x1A, 0x1A, 0x1A);

/// Paleta 8 columnas x 3 filas (aprox. la de Bitwig).
pub const PALETTE: [[u32; 8]; 3] = [
    [0x7A7A7A, 0x8F8F8F, 0xD0D0D0, 0x8E8EBE, 0xA88B57, 0xD2A86B, 0x5662C6, 0x7B8CF0],
    [0xDB4B7E, 0xE0352F, 0xFF5500, 0xE5A21A, 0x78A01E, 0x1AA64A, 0x15A89B, 0x1E9BDC],
    [0xE8548B, 0xF0605A, 0xFF8A3D, 0xE8BA4F, 0xA0C23F, 0x3DBE6A, 0x3DD6B8, 0x45B8F5],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MonitorMode {
    Off,
    Auto,
    On,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    SetColor(Color32),
    ToggleFillHeader,
    Cut,
    Copy,
    Paste,
    Duplicate,
    Delete,
    Rename,
    ToggleActive,
    Group,
    Ungroup,
    EnterGroup,
    ExitGroup,
    ConvertToInstrument,
    ConvertToAudio,
    ConvertToHybrid,
    SetMonitor(MonitorMode),
}

/// Qué entradas están habilitadas / estado actual para esta pista.
#[derive(Clone, Copy, Debug)]
pub struct MenuContext {
    pub current_color: Color32,
    pub fill_header: bool,
    pub can_paste: bool,
    pub can_ungroup: bool,
    pub can_enter_group: bool,
    pub can_exit_group: bool,
    pub can_convert_instrument: bool,
    pub can_convert_audio: bool,
    pub can_convert_hybrid: bool,
    pub monitor: MonitorMode,
}

impl Default for MenuContext {
    fn default() -> Self {
        Self {
            current_color: Color32::from_rgb(0xFF, 0x55, 0x00),
            fill_header: true,
            can_paste: false,
            can_ungroup: false,
            can_enter_group: false,
            can_exit_group: false,
            can_convert_instrument: false,
            can_convert_audio: true,
            can_convert_hybrid: true,
            monitor: MonitorMode::Auto,
        }
    }
}

#[derive(Clone, Copy)]
struct OpenState {
    pos: Pos2,
    submenu: bool,
}

fn state_id(key: Id) -> Id {
    key.with("trk_ctx_menu")
}

/// Abre el menú en `pos`.
pub fn open(ctx: &Context, key: Id, pos: Pos2) {
    ctx.data_mut(|d| d.insert_temp(state_id(key), OpenState { pos, submenu: false }));
}

pub fn is_open(ctx: &Context, key: Id) -> bool {
    ctx.data(|d| d.get_temp::<OpenState>(state_id(key))).is_some()
}

fn close(ctx: &Context, key: Id) {
    ctx.data_mut(|d| d.remove::<OpenState>(state_id(key)));
}

struct Row<'a> {
    label: &'a str,
    shortcut: &'a str,
    enabled: bool,
    icon: Option<VectorIcon>,
}

fn paint_row(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    row: &Row,
    arrow: bool,
    force_hover: bool,
) -> egui::Response {
    let resp = ui.interact(rect, id, if row.enabled { Sense::click() } else { Sense::hover() });
    let hot = row.enabled && (resp.hovered() || force_hover);
    let p = ui.painter();
    if hot {
        p.rect_filled(rect, 0.0, ROW_HOVER);
    }
    let fg = if !row.enabled {
        TXT_DIM
    } else if hot {
        TXT_ON_HOVER
    } else {
        TXT
    };
    if let Some(ic) = row.icon {
        let ir = Rect::from_center_size(pos2(rect.min.x + 12.0, rect.center().y), vec2(14.0, 14.0));
        icons::paint_icon(p, ir, ic, fg, 1.4);
    }
    p.text(
        pos2(rect.min.x + 30.0, rect.center().y),
        Align2::LEFT_CENTER,
        row.label,
        fonts::regular(13.0),
        fg,
    );
    if !row.shortcut.is_empty() {
        p.text(
            pos2(rect.max.x - 8.0, rect.center().y),
            Align2::RIGHT_CENTER,
            row.shortcut,
            fonts::regular(12.5),
            if hot { TXT_ON_HOVER } else { TXT_HINT },
        );
    }
    if arrow {
        let c = pos2(rect.max.x - 10.0, rect.center().y);
        p.add(egui::Shape::convex_polygon(
            vec![c + vec2(-3.0, -4.5), c + vec2(-3.0, 4.5), c + vec2(3.5, 0.0)],
            fg,
            Stroke::NONE,
        ));
    }
    resp
}

/// Dibuja el menú si está abierto. Devuelve la acción elegida (y lo cierra).
pub fn show(ctx: &Context, key: Id, mc: &MenuContext) -> Option<MenuAction> {
    let st = ctx.data(|d| d.get_temp::<OpenState>(state_id(key)))?;
    let mut action: Option<MenuAction> = None;
    let mut submenu = st.submenu;

    let screen = ctx.screen_rect();
    let inner_w = MENU_W - PAD_X * 2.0;
    let palette_h = 3.0 * SWATCH + 2.0 * SWATCH_GAP + 12.0;
    let total_h = 4.0
        + palette_h
        + ROW_H // Fill header backgrounds
        + SEP_H
        + ICON_ROW_H
        + SEP_H
        + ROW_H * 2.0 // Rename, Toggle Active (con sep. extra abajo)
        + SEP_H
        + ROW_H * 4.0 // Group..Exit Group
        + SEP_H
        + ROW_H * 3.0 // Convert
        + SEP_H
        + ROW_H; // Monitor
    let mut origin = st.pos;
    origin.x = origin.x.min(screen.max.x - MENU_W - 2.0).max(screen.min.x);
    origin.y = origin.y.min(screen.max.y - total_h - 2.0).max(screen.min.y);
    let menu_rect = Rect::from_min_size(origin, vec2(MENU_W, total_h));

    let mut monitor_rect = menu_rect;
    let mut sub_rect = menu_rect;

    egui::Area::new(key.with("trk_ctx_area"))
        .order(Order::Foreground)
        .fixed_pos(origin)
        .sense(Sense::hover())
        .show(ctx, |ui| {
            ui.set_clip_rect(screen);
            let (rect, _) = ui.allocate_exact_size(vec2(MENU_W, total_h), Sense::hover());
            let p = ui.painter().clone();
            p.rect_filled(rect, 1.0, MENU_BG);
            p.rect_stroke(rect, 1.0, Stroke::new(1.0_f32, MENU_BORDER), egui::StrokeKind::Inside);

            let mut y = rect.min.y + 4.0;
            let x0 = rect.min.x + PAD_X;

            // --- Paleta ---
            let pal_rect = Rect::from_min_size(
                pos2(x0 + 26.0, y + 2.0),
                vec2(8.0 * SWATCH + 7.0 * SWATCH_GAP, 3.0 * SWATCH + 2.0 * SWATCH_GAP),
            );
            p.rect_filled(pal_rect.expand(4.0), 0.0, Color32::from_rgb(0x20, 0x20, 0x20));
            for (r, row) in PALETTE.iter().enumerate() {
                for (c, hex) in row.iter().enumerate() {
                    let col = Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, *hex as u8);
                    let sr = Rect::from_min_size(
                        pos2(
                            pal_rect.min.x + c as f32 * (SWATCH + SWATCH_GAP),
                            pal_rect.min.y + r as f32 * (SWATCH + SWATCH_GAP),
                        ),
                        vec2(SWATCH, SWATCH),
                    );
                    p.rect_filled(sr, 0.0, col);
                    if col == mc.current_color {
                        p.rect_stroke(
                            sr.expand(1.5),
                            0.0,
                            Stroke::new(1.5_f32, Color32::BLACK),
                            egui::StrokeKind::Inside,
                        );
                    }
                    let resp = ui.interact(sr, key.with(("sw", r, c)), Sense::click());
                    if resp.hovered() {
                        p.rect_stroke(sr, 0.0, Stroke::new(1.0_f32, Color32::WHITE), egui::StrokeKind::Inside);
                    }
                    if resp.clicked() {
                        action = Some(MenuAction::SetColor(col));
                    }
                }
            }
            y += palette_h;

            // --- Fill header backgrounds ---
            let r = Rect::from_min_size(pos2(x0, y), vec2(inner_w, ROW_H));
            let resp = paint_row(
                ui,
                r,
                key.with("fill"),
                &Row { label: "Fill header backgrounds", shortcut: "", enabled: true, icon: None },
                false,
                false,
            );
            let cc = pos2(r.min.x + 12.0, r.center().y);
            p.circle_filled(cc, 7.0, if mc.fill_header { Color32::from_rgb(0xB4, 0xB4, 0xB4) } else { Color32::from_rgb(0x55, 0x55, 0x55) });
            p.line_segment([cc + vec2(-3.0, 0.5), cc + vec2(-1.0, 3.0)], Stroke::new(1.8_f32, MENU_BG));
            p.line_segment([cc + vec2(-1.0, 3.0), cc + vec2(3.5, -2.5)], Stroke::new(1.8_f32, MENU_BG));
            if resp.clicked() {
                action = Some(MenuAction::ToggleFillHeader);
            }
            y += ROW_H;

            let sep = |y: &mut f32| {
                let ly = (*y + SEP_H * 0.5).round() - 0.5;
                p.line_segment([pos2(x0, ly), pos2(x0 + inner_w, ly)], Stroke::new(1.0_f32, SEP));
                *y += SEP_H;
            };
            sep(&mut y);

            // --- Fila de iconos: Cut / Copy / Paste / Dupl. / Delete ---
            let cell_w = inner_w / 5.0;
            let icon_items: [(VectorIcon, &str, bool, MenuAction); 5] = [
                (icons::CUT, "CUT", true, MenuAction::Cut),
                (IC_COPY, "COPY", true, MenuAction::Copy),
                (IC_PASTE, "PASTE", mc.can_paste, MenuAction::Paste),
                (icons::DUPLICATE_PLUS, "DUPL.", true, MenuAction::Duplicate),
                (icons::CIRCLE_X, "DELETE", true, MenuAction::Delete),
            ];
            for (i, (glyph, label, en, act)) in icon_items.iter().enumerate() {
                let cr = Rect::from_min_size(
                    pos2(x0 + i as f32 * cell_w, y),
                    vec2(cell_w, ICON_ROW_H),
                );
                let resp = ui.interact(cr, key.with(("ic", i)), if *en { Sense::click() } else { Sense::hover() });
                let hot = *en && resp.hovered();
                if hot {
                    p.rect_filled(cr, 0.0, ROW_HOVER);
                }
                let fg = if !*en { TXT_DIM } else if hot { TXT_ON_HOVER } else { TXT };
                icons::paint_icon(&p, Rect::from_center_size(pos2(cr.center().x, cr.min.y + 15.0), vec2(16.0, 16.0)), *glyph, fg, 1.5);
                p.text(
                    pos2(cr.center().x, cr.min.y + 33.0),
                    Align2::CENTER_CENTER,
                    *label,
                    fonts::regular(9.5),
                    if !*en { TXT_DIM } else if hot { TXT_ON_HOVER } else { TXT_HINT },
                );
                if resp.clicked() {
                    action = Some(*act);
                }
            }
            y += ICON_ROW_H;
            sep(&mut y);

            let simple = |ui: &mut Ui, y: &mut f32, id: &str, row: Row, act: MenuAction, action: &mut Option<MenuAction>| {
                let r = Rect::from_min_size(pos2(x0, *y), vec2(inner_w, ROW_H));
                let resp = paint_row(ui, r, key.with(id), &row, false, false);
                if resp.clicked() {
                    *action = Some(act);
                }
                *y += ROW_H;
            };

            simple(ui, &mut y, "rename", Row { label: "Rename", shortcut: "Ctrl+R", enabled: true, icon: Some(IC_RENAME) }, MenuAction::Rename, &mut action);
            y += 9.0; // aire extra, igual que el original
            simple(ui, &mut y, "active", Row { label: "Toggle Active", shortcut: "Alt+A", enabled: true, icon: Some(IC_ACTIVE) }, MenuAction::ToggleActive, &mut action);
            y -= 9.0;
            sep(&mut y);

            simple(ui, &mut y, "group", Row { label: "Group", shortcut: "Ctrl+G", enabled: true, icon: Some(IC_GROUP) }, MenuAction::Group, &mut action);
            simple(ui, &mut y, "ungroup", Row { label: "Ungroup", shortcut: "Ctrl+Shift+G", enabled: mc.can_ungroup, icon: Some(IC_UNGROUP) }, MenuAction::Ungroup, &mut action);
            simple(ui, &mut y, "enter", Row { label: "Enter Group", shortcut: "", enabled: mc.can_enter_group, icon: Some(IC_ENTER) }, MenuAction::EnterGroup, &mut action);
            simple(ui, &mut y, "exit", Row { label: "Exit Group", shortcut: "", enabled: mc.can_exit_group, icon: Some(IC_EXIT) }, MenuAction::ExitGroup, &mut action);
            sep(&mut y);

            simple(ui, &mut y, "c_inst", Row { label: "Convert to Instrument Track", shortcut: "", enabled: mc.can_convert_instrument, icon: Some(IC_INST) }, MenuAction::ConvertToInstrument, &mut action);
            simple(ui, &mut y, "c_audio", Row { label: "Convert to Audio Track", shortcut: "", enabled: mc.can_convert_audio, icon: Some(IC_AUDIO) }, MenuAction::ConvertToAudio, &mut action);
            simple(ui, &mut y, "c_hyb", Row { label: "Convert to Hybrid Track", shortcut: "", enabled: mc.can_convert_hybrid, icon: Some(IC_HYBRID) }, MenuAction::ConvertToHybrid, &mut action);
            sep(&mut y);

            // --- Monitor ▶ ---
            let mr = Rect::from_min_size(pos2(rect.min.x, y), vec2(MENU_W, ROW_H));
            monitor_rect = mr;
            let resp = paint_row(
                ui,
                Rect::from_min_size(pos2(rect.min.x, y), vec2(MENU_W, ROW_H)).shrink2(vec2(0.0, 0.0)),
                key.with("monitor"),
                &Row { label: "Monitor", shortcut: "", enabled: true, icon: None },
                true,
                submenu,
            );
            if resp.hovered() {
                submenu = true;
            }
        });

    // --- Submenú Monitor (a la derecha, alineado con la fila) ---
    if submenu {
        let items: [(&str, &str, MonitorMode); 3] = [
            ("Off", "Never monitor input", MonitorMode::Off),
            ("Auto", "Monitor armed tracks (when no clip is playing)", MonitorMode::Auto),
            ("On", "Always monitor input", MonitorMode::On),
        ];
        let sub_w = 330.0;
        let sub_h = ROW_H * 3.0 + 6.0;
        let mut sp = pos2(menu_rect.max.x - 1.0, monitor_rect.min.y);
        if sp.x + sub_w > screen.max.x {
            sp.x = menu_rect.min.x - sub_w + 1.0;
        }
        sp.y = sp.y.min(screen.max.y - sub_h - 2.0);
        sub_rect = Rect::from_min_size(sp, vec2(sub_w, sub_h));
        egui::Area::new(key.with("trk_ctx_sub"))
            .order(Order::Tooltip)
            .fixed_pos(sp)
            .sense(Sense::hover())
            .show(ctx, |ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(sub_w, sub_h), Sense::hover());
                let p = ui.painter().clone();
                p.rect_filled(rect, 1.0, MENU_BG);
                p.rect_stroke(rect, 1.0, Stroke::new(1.0_f32, MENU_BORDER), egui::StrokeKind::Inside);
                for (i, (name, hint, mode)) in items.iter().enumerate() {
                    let r = Rect::from_min_size(
                        pos2(rect.min.x, rect.min.y + 3.0 + i as f32 * ROW_H),
                        vec2(sub_w, ROW_H),
                    );
                    let resp = ui.interact(r, key.with(("mon", i)), Sense::click());
                    let hot = resp.hovered();
                    if hot {
                        p.rect_filled(r, 0.0, ROW_HOVER);
                    }
                    let fg = if hot { TXT_ON_HOVER } else { TXT };
                    // Radio
                    let c = pos2(r.min.x + 14.0, r.center().y);
                    p.circle_stroke(c, 4.0, Stroke::new(1.2_f32, fg));
                    if mc.monitor == *mode {
                        p.circle_filled(c, 2.6, fg);
                    }
                    let galley = p.layout_no_wrap((*name).to_string(), fonts::semibold(13.0), fg);
                    let nw = galley.size().x;
                    p.galley(pos2(r.min.x + 30.0, r.center().y - galley.size().y * 0.5), galley, fg);
                    p.text(
                        pos2(r.min.x + 30.0 + nw + 8.0, r.center().y),
                        Align2::LEFT_CENTER,
                        *hint,
                        fonts::regular(13.0),
                        if hot { TXT_ON_HOVER } else { TXT_HINT },
                    );
                    if resp.clicked() {
                        action = Some(MenuAction::SetMonitor(*mode));
                    }
                }
            });
    }

    // Mantener/cerrar submenú según el puntero.
    let ptr = ctx.input(|i| i.pointer.hover_pos());
    if let Some(pp) = ptr {
        if submenu && !monitor_rect.contains(pp) && !sub_rect.contains(pp) && menu_rect.contains(pp) {
            submenu = false;
        }
    }
    ctx.data_mut(|d| d.insert_temp(state_id(key), OpenState { pos: st.pos, submenu }));

    // Cerrar: elección, Escape, o click fuera.
    let clicked_outside = ctx.input(|i| i.pointer.any_pressed())
        && ptr.is_some_and(|pp| !menu_rect.contains(pp) && !(submenu && sub_rect.contains(pp)));
    let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    // El submenú (Area::Tooltip) puede quedar fuera de menu_rect: ya contemplado arriba.
    if action.is_some() || clicked_outside || esc {
        close(ctx, key);
    }
    action
}
