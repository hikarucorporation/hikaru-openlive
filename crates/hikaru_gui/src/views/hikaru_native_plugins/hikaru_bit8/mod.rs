// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_bit8/mod.rs

//! **Hikaru Open8-Bit**: panel nativo para el degradador digital `hikaru_dsp::effects::Bit8Effect`.
//!
//! Diseño e instrumentación:
//! - Columna izquierda **Clock** (amarillo): `Clock` (`200 Hz .. 100.0 kHz`, default `12.9 kHz`),
//!   `Jitter` (`0 .. 100%`, default `25.5%`), `Chance` (`0 .. 100%`, default `100%`).
//! - Display superior oscuro de 3 segundos con 3 trazas desplazables en vivo:
//!   **Gate** (blanco), **Shape/Clip** (azul) y **Quantize** (púrpura) sobre grilla horizontal.
//! - Fila inferior central con LEDs de actividad amarillos por etapa:
//!   - **Gate** (blanco): umbral post-cuantización (`-96.0 .. 0.0 dB`, default `-58.2 dB`).
//!   - **Shape** (azul): 3 modos de rectificación (`Off`, `Half`, `Full`) + selector de clipping
//!     (`Off`, `Soft Clip`, `Hard Clip`, `Fold`, `Wrap`) + knob condicional `Drive` (`0 .. +36 dB`).
//!   - **Quantize** (púrpura): selector de modo (`Off-zero`, `On-zero`, `Diffusion`, `Dither`) con
//!     controles dinámicos (`Diffusion Brightness` con icono solar / `Noise` + toggle estéreo),
//!     knob `Quantize` (`-96.0 .. 0.0 dB`, default `-39.1 dB`) y botón **Squared Domain** (`x²`).
//! - Columna derecha (naranja): pestaña expandible **Wet FX** (`HP`, `LP`, `Gain`), botón
//!   **Anti-alias** (ADAA1), knobs **Width** y **Mix**.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{
    pos2, vec2, Align2, Color32, CornerRadius, CursorIcon, Painter, Pos2, Rect, Sense, Stroke,
};
use hikaru_dsp::effects::{
    Bit8Params, Bit8QuantizeMode, Bit8Rectify, Bit8ShapeMode, Bit8TelemetrySnapshot,
    BIT8_DISPLAY_HISTORY, BIT8_MAX_DRIVE_DB, BIT8_PARAM_COUNT,
};

use crate::app::AppState;
use crate::theme;
use crate::ui::bitwig_kit;
use crate::ui::fonts;

pub const BIT8_BASE_CARD_WIDTH: f32 = 418.0;
pub const BIT8_WET_FX_DRAWER_WIDTH: f32 = 84.0;

// Paleta de alta precisión para Hikaru Open8-Bit
const COL_PANEL_BG: Color32 = Color32::from_rgb(0x23, 0x25, 0x29);
const COL_PANEL_BORDER: Color32 = Color32::from_rgb(0x18, 0x1A, 0x1E);
const COL_DISPLAY_BG: Color32 = Color32::from_rgb(0x12, 0x13, 0x16);
const COL_CTRL_BG: Color32 = Color32::from_rgb(0x3B, 0x3D, 0x43);
const COL_CTRL_HOVER: Color32 = Color32::from_rgb(0x49, 0x4C, 0x54);
const COL_CTRL_BORDER: Color32 = Color32::from_rgb(0x1B, 0x1D, 0x21);

const COL_CLOCK_YELLOW: Color32 = Color32::from_rgb(0xEB, 0xCC, 0x34);
const COL_LED_YELLOW: Color32 = Color32::from_rgb(0xF2, 0xCE, 0x35);
const COL_GATE_WHITE: Color32 = Color32::from_rgb(0xE6, 0xE8, 0xEC);
const COL_SHAPE_BLUE: Color32 = Color32::from_rgb(0x46, 0x9E, 0xF5);
const COL_QUANT_PURPLE: Color32 = Color32::from_rgb(0xB8, 0x5C, 0xFF);
const COL_OUT_ORANGE: Color32 = Color32::from_rgb(0xF5, 0x7A, 0x1A);
const COL_WET_CYAN: Color32 = Color32::from_rgb(0x3D, 0x8F, 0xB8);

pub fn is_bit8_name(name: &str) -> bool {
    matches!(
        name,
        "Hikaru Open8-Bit"
            | "Hikaru Open8-bit"
            | "Open8-Bit"
            | "Open8-bit"
            | "Bit-8"
            | "Bit 8"
            | "Hikaru Bit-8"
    )
}

struct Bit8SlotUiState {
    params: Bit8Params,
    wet_fx_open: bool,
    last_snapshot: Bit8TelemetrySnapshot,
}

impl Default for Bit8SlotUiState {
    fn default() -> Self {
        Self {
            params: Bit8Params::default(),
            wet_fx_open: false,
            last_snapshot: Bit8TelemetrySnapshot {
                gate_led: 0.0,
                clip_led: 0.0,
                quant_led: 0.0,
                write_seq: 0,
                gate_trace: [-1.0; BIT8_DISPLAY_HISTORY],
                clip_trace: [-1.0; BIT8_DISPLAY_HISTORY],
                quant_trace: [-1.0; BIT8_DISPLAY_HISTORY],
            },
        }
    }
}

type StateMap = Mutex<HashMap<(usize, usize), Bit8SlotUiState>>;
type LoadedSet = Mutex<HashSet<(usize, usize)>>;

fn state_map() -> &'static StateMap {
    static CELL: OnceLock<StateMap> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(HashMap::new()))
}

fn loaded_set() -> &'static LoadedSet {
    static CELL: OnceLock<LoadedSet> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(HashSet::new()))
}

pub fn card_width_for_slot(track_idx: usize, slot_idx: usize) -> f32 {
    let open = state_map()
        .lock()
        .unwrap()
        .get(&(track_idx, slot_idx))
        .map(|s| s.wet_fx_open)
        .unwrap_or(false);
    if open {
        BIT8_BASE_CARD_WIDTH + BIT8_WET_FX_DRAWER_WIDTH
    } else {
        BIT8_BASE_CARD_WIDTH
    }
}

pub fn clear_slot_state(track_idx: usize, slot_idx: usize) {
    state_map().lock().unwrap().remove(&(track_idx, slot_idx));
    loaded_set().lock().unwrap().remove(&(track_idx, slot_idx));
}

pub struct Bit8Preset {
    pub name: &'static str,
    pub params: Bit8Params,
}

pub fn factory_presets() -> Vec<Bit8Preset> {
    vec![
        Bit8Preset {
            name: "Default (Open8-Bit)",
            params: Bit8Params::default(),
        },
        Bit8Preset {
            name: "8-Bit Console",
            params: Bit8Params {
                clock_hz: 22_050.0,
                jitter: 0.0,
                chance: 1.0,
                gate_db: -64.0,
                quantize_mode: Bit8QuantizeMode::OnZero,
                quantize_db: -48.0,
                ..Bit8Params::default()
            },
        },
        Bit8Preset {
            name: "Error Diffusion Shimmer",
            params: Bit8Params {
                clock_hz: 28_000.0,
                jitter: 0.12,
                chance: 1.0,
                gate_db: -72.0,
                quantize_mode: Bit8QuantizeMode::Diffusion,
                quantize_db: -18.0,
                diffusion_brightness: 0.82,
                anti_alias: true,
                ..Bit8Params::default()
            },
        },
        Bit8Preset {
            name: "Stereo Dither Grit",
            params: Bit8Params {
                clock_hz: 16_000.0,
                jitter: 0.20,
                chance: 1.0,
                gate_db: -66.0,
                quantize_mode: Bit8QuantizeMode::Dither,
                quantize_db: -24.0,
                stereo_dither: true,
                squared_domain: true,
                anti_alias: true,
                ..Bit8Params::default()
            },
        },
        Bit8Preset {
            name: "ADAA Wavefolder Crush",
            params: Bit8Params {
                clock_hz: 24_000.0,
                jitter: 0.15,
                shape_mode: Bit8ShapeMode::Fold,
                shape_drive_db: 18.0,
                quantize_db: -32.0,
                anti_alias: true,
                ..Bit8Params::default()
            },
        },
        Bit8Preset {
            name: "Wrap Glitch",
            params: Bit8Params {
                clock_hz: 8_500.0,
                jitter: 0.65,
                chance: 0.72,
                gate_db: -58.2,
                shape_mode: Bit8ShapeMode::Wrap,
                shape_drive_db: 14.0,
                quantize_db: -28.0,
                ..Bit8Params::default()
            },
        },
        Bit8Preset {
            name: "Hard Clip Gate",
            params: Bit8Params {
                clock_hz: 32_000.0,
                jitter: 0.0,
                gate_db: -36.0,
                rectify: Bit8Rectify::Full,
                shape_mode: Bit8ShapeMode::HardClip,
                shape_drive_db: 22.0,
                quantize_db: -36.0,
                anti_alias: true,
                ..Bit8Params::default()
            },
        },
        Bit8Preset {
            name: "Lo-Fi Telephone",
            params: Bit8Params {
                clock_hz: 8_000.0,
                jitter: 0.25,
                chance: 1.0,
                gate_db: -52.0,
                shape_mode: Bit8ShapeMode::SoftClip,
                shape_drive_db: 10.0,
                quantize_mode: Bit8QuantizeMode::OnZero,
                quantize_db: -30.0,
                wet_hp_hz: 320.0,
                wet_lp_hz: 3_400.0,
                ..Bit8Params::default()
            },
        },
    ]
}

fn push_all_params(
    app: &mut AppState,
    engine_track_idx: usize,
    slot_idx: usize,
    params: &Bit8Params,
) {
    for id in 0..(BIT8_PARAM_COUNT as u32) {
        if let Some(val) = params.get_normalized(id) {
            app.audio_proxy.send(crate::audio_proxy::GuiCommand::SetExternalPluginParam {
                track_idx: engine_track_idx,
                slot_idx,
                param_id: id,
                value: val.clamp(0.0, 1.0),
            });
        }
    }
}

pub fn render_preset_menu(
    ui: &mut egui::Ui,
    app: &mut AppState,
    track_idx: usize,
    slot_idx: usize,
) {
    ui.set_min_width(172.0);
    ui.label(
        egui::RichText::new("Hikaru Open8-Bit Presets")
            .font(fonts::semibold(10.0))
            .color(COL_OUT_ORANGE),
    );
    ui.separator();
    for preset in factory_presets() {
        if ui
            .button(
                egui::RichText::new(preset.name)
                    .font(fonts::regular(10.0))
                    .color(theme::TEXT_PRIMARY),
            )
            .clicked()
        {
            {
                let mut map = state_map().lock().unwrap();
                let st = map.entry((track_idx, slot_idx)).or_default();
                st.params = preset.params;
            }
            if let Some(et_idx) = track_idx.checked_sub(1) {
                push_all_params(app, et_idx, slot_idx, &preset.params);
            }
            ui.close_menu();
        }
    }
}

fn format_clock_hz(hz: f32) -> String {
    if hz >= 1000.0 {
        let khz = hz / 1000.0;
        if khz >= 10.0 {
            format!("{khz:.1} kHz")
        } else {
            format!("{khz:.2} kHz")
        }
    } else if hz >= 100.0 {
        format!("{hz:.0} Hz")
    } else {
        format!("{hz:.1} Hz")
    }
}

fn format_db(db: f32) -> String {
    if db.abs() < 0.05 {
        "0.0 dB".to_string()
    } else {
        format!("{db:.1} dB")
    }
}

fn paint_recessed_panel(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect, CornerRadius::same(3), COL_PANEL_BG);
    painter.rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(1.0_f32, COL_PANEL_BORDER),
        egui::StrokeKind::Inside,
    );
}

fn paint_bevel_box(painter: &Painter, rect: Rect, active: bool, hovered: bool, accent: Color32) {
    let fill = if active {
        accent
    } else if hovered {
        COL_CTRL_HOVER
    } else {
        COL_CTRL_BG
    };
    painter.rect_filled(rect, CornerRadius::same(3), fill);
    let hi = if active {
        Color32::from_rgba_unmultiplied(255, 255, 255, 55)
    } else {
        Color32::from_rgba_unmultiplied(255, 255, 255, 26)
    };
    painter.line_segment(
        [
            pos2(rect.min.x + 2.0, rect.min.y + 1.0),
            pos2(rect.max.x - 2.0, rect.min.y + 1.0),
        ],
        Stroke::new(1.0_f32, hi),
    );
    painter.rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(1.0_f32, COL_CTRL_BORDER),
        egui::StrokeKind::Inside,
    );
}

/// Encabezado de sección con LED de actividad amarillo (sin checkbox de bypass).
fn paint_section_led_header(
    ui: &mut egui::Ui,
    panel_rect: Rect,
    id: egui::Id,
    label: &str,
    accent: Color32,
    led_level: f32,
    tooltip: &str,
) {
    let hdr_rect = Rect::from_min_size(
        pos2(panel_rect.min.x + 4.0, panel_rect.min.y + 4.0),
        vec2(panel_rect.width() - 8.0, 15.0),
    );
    let _ = ui.interact(hdr_rect, id, Sense::hover()).on_hover_text(tooltip);

    let p = ui.painter();
    let led_rect = Rect::from_min_size(
        pos2(hdr_rect.min.x + 1.0, hdr_rect.center().y - 5.0),
        vec2(10.0, 10.0),
    );
    let lvl = led_level.clamp(0.0, 1.0);
    if lvl > 0.008 {
        let vis = lvl.powf(0.62);
        let fill_alpha = (vis * 255.0).round() as u8;
        let fill = Color32::from_rgba_unmultiplied(
            COL_LED_YELLOW.r(),
            COL_LED_YELLOW.g(),
            COL_LED_YELLOW.b(),
            fill_alpha,
        );
        if vis > 0.35 {
            let glow_alpha = ((vis - 0.35) * 70.0).round().clamp(0.0, 52.0) as u8;
            p.rect_filled(
                led_rect.expand(1.5),
                CornerRadius::same(2),
                Color32::from_rgba_unmultiplied(
                    COL_LED_YELLOW.r(),
                    COL_LED_YELLOW.g(),
                    COL_LED_YELLOW.b(),
                    glow_alpha,
                ),
            );
        }
        p.rect_filled(led_rect, CornerRadius::same(1), fill);
        let border_alpha = (vis * 140.0).round().clamp(24.0, 140.0) as u8;
        p.rect_stroke(
            led_rect,
            CornerRadius::same(1),
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(0x16, 0x15, 0x10, border_alpha)),
            egui::StrokeKind::Inside,
        );
    }

    let text_cx = ((led_rect.max.x + 3.0) + hdr_rect.max.x) * 0.5;
    p.text(
        pos2(text_cx.max(panel_rect.center().x + 2.0), hdr_rect.center().y),
        Align2::CENTER_CENTER,
        label,
        fonts::semibold(10.5),
        accent,
    );
}

fn paint_rectify_icon(painter: &Painter, rect: Rect, rectify: Bit8Rectify, color: Color32) {
    let mut pts = Vec::with_capacity(17);
    let w = rect.width();
    let h = rect.height();
    let cy = rect.center().y;
    let amp = h * 0.42;
    for i in 0..=16 {
        let t = i as f32 / 16.0;
        let s = (t * std::f32::consts::TAU).sin();
        let x = rect.min.x + t * w;
        let y = match rectify {
            Bit8Rectify::Off => cy - s * amp,
            Bit8Rectify::Half => cy + amp * 0.25 - s.max(0.0) * amp * 1.15,
            Bit8Rectify::Full => cy + amp * 0.45 - s.abs() * amp * 1.1,
        };
        pts.push(pos2(x, y));
    }
    painter.add(egui::Shape::line(pts, Stroke::new(1.35_f32, color)));
}

fn paint_quantize_mode_icon(
    painter: &Painter,
    rect: Rect,
    mode: Bit8QuantizeMode,
    color: Color32,
) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    let axis_col = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 100);
    let st = Stroke::new(1.25_f32, color);
    let w = rect.width();
    let h = rect.height();

    match mode {
        Bit8QuantizeMode::OffZero => {
            painter.line_segment(
                [pos2(cx, rect.min.y), pos2(cx, rect.max.y)],
                Stroke::new(0.8_f32, axis_col),
            );
            painter.line_segment(
                [pos2(rect.min.x, cy), pos2(rect.max.x, cy)],
                Stroke::new(0.8_f32, axis_col),
            );
            painter.line_segment(
                [pos2(rect.min.x + 1.0, rect.max.y - 1.5), pos2(cx - 2.2, rect.max.y - 1.5)],
                st,
            );
            painter.line_segment(
                [pos2(cx - 2.2, cy + h * 0.18), pos2(cx, cy + h * 0.18)],
                st,
            );
            painter.line_segment(
                [pos2(cx, cy - h * 0.18), pos2(cx + 2.4, cy - h * 0.18)],
                st,
            );
            painter.line_segment(
                [pos2(cx + 2.4, rect.min.y + 1.5), pos2(rect.max.x - 1.0, rect.min.y + 1.5)],
                st,
            );
        }
        Bit8QuantizeMode::OnZero => {
            painter.line_segment(
                [pos2(cx, rect.min.y), pos2(cx, rect.max.y)],
                Stroke::new(0.8_f32, axis_col),
            );
            painter.line_segment(
                [pos2(rect.min.x, cy), pos2(rect.max.x, cy)],
                Stroke::new(0.8_f32, axis_col),
            );
            painter.line_segment(
                [pos2(rect.min.x + 1.0, rect.max.y - 1.8), pos2(cx - w * 0.19, rect.max.y - 1.8)],
                st,
            );
            painter.line_segment(
                [pos2(cx - w * 0.19, cy), pos2(cx + w * 0.19, cy)],
                st,
            );
            painter.line_segment(
                [pos2(cx + w * 0.19, rect.min.y + 1.8), pos2(rect.max.x - 1.0, rect.min.y + 1.8)],
                st,
            );
        }
        Bit8QuantizeMode::Diffusion => {
            // Haz diagonal difuso (error diffusion)
            let p0 = pos2(rect.min.x + 1.2, rect.max.y - 1.2);
            let p1 = pos2(rect.max.x - 1.2, rect.min.y + 1.2);
            let soft_wide = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 60);
            let soft_mid = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 125);
            painter.line_segment([p0, p1], Stroke::new(4.2_f32, soft_wide));
            painter.line_segment([p0, p1], Stroke::new(2.2_f32, soft_mid));
            painter.line_segment([p0, p1], Stroke::new(1.1_f32, color));
        }
        Bit8QuantizeMode::Dither => {
            // Puntos/píxeles dispersos sobre la diagonal (TPDF dither)
            let dots: [(f32, f32, u8); 9] = [
                (0.14, 0.84, 220),
                (0.28, 0.72, 165),
                (0.22, 0.56, 120),
                (0.45, 0.58, 230),
                (0.54, 0.44, 190),
                (0.64, 0.60, 125),
                (0.62, 0.28, 175),
                (0.76, 0.34, 210),
                (0.86, 0.16, 235),
            ];
            for (fx, fy, alpha) in dots {
                let c = pos2(rect.min.x + fx * w, rect.min.y + fy * h);
                painter.rect_filled(
                    Rect::from_center_size(c, vec2(2.0, 2.0)),
                    0.0,
                    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha),
                );
            }
        }
    }
}

fn paint_sunburst_icon(painter: &Painter, center: Pos2, brightness: f32) {
    let alpha = ((0.45 + 0.55 * brightness.clamp(0.0, 1.0)) * 255.0).round() as u8;
    let col = Color32::from_rgba_unmultiplied(0xE8, 0xEC, 0xF4, alpha);
    painter.circle_filled(center, 2.2, col);
    for i in 0..8 {
        let ang = i as f32 * (std::f32::consts::TAU / 8.0);
        let dir = vec2(ang.cos(), ang.sin());
        painter.line_segment(
            [center + dir * 3.6, center + dir * 5.8],
            Stroke::new(1.1_f32, col),
        );
    }
}

fn paint_stereo_venn_icon(painter: &Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let r = (rect.height() * 0.26).clamp(3.6, 5.0);
    let dx = r * 0.58;
    let st = Stroke::new(1.25_f32, color);
    painter.circle_stroke(pos2(c.x - dx, c.y), r, st);
    painter.circle_stroke(pos2(c.x + dx, c.y), r, st);
}

fn paint_squared_domain_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = rect.shrink(3.5);
    let mut upper = Vec::with_capacity(11);
    let mut lower = Vec::with_capacity(11);
    for i in 0..=10 {
        let t = i as f32 / 10.0;
        let x = r.min.x + t * r.width();
        let y_sq = r.max.y - (t * t) * r.height();
        let y_rt = r.max.y - t.sqrt() * r.height();
        lower.push(pos2(x, y_sq));
        upper.push(pos2(x, y_rt));
    }
    let faint = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 175);
    painter.add(egui::Shape::line(upper, Stroke::new(1.15_f32, faint)));
    painter.add(egui::Shape::line(lower, Stroke::new(1.15_f32, faint)));
    painter.line_segment(
        [pos2(r.min.x, r.max.y), pos2(r.max.x, r.min.y)],
        Stroke::new(1.3_f32, color),
    );
}

fn paint_dropdown_arrow(painter: &Painter, center: Pos2, color: Color32) {
    painter.add(egui::Shape::convex_polygon(
        vec![
            pos2(center.x - 3.0, center.y - 1.5),
            pos2(center.x + 3.0, center.y - 1.5),
            pos2(center.x, center.y + 2.5),
        ],
        color,
        Stroke::NONE,
    ));
}

fn paint_ribbon_between(
    painter: &Painter,
    top_pts: &[Pos2],
    bot_pts: &[Pos2],
    color: Color32,
) {
    let n = top_pts.len().min(bot_pts.len());
    if n < 2 {
        return;
    }
    let mut mesh = Mesh::default();
    mesh.vertices.reserve(n * 2);
    mesh.indices.reserve((n - 1) * 6);
    for i in 0..n {
        mesh.vertices.push(Vertex {
            pos: top_pts[i],
            uv: WHITE_UV,
            color,
        });
        mesh.vertices.push(Vertex {
            pos: bot_pts[i],
            uv: WHITE_UV,
            color,
        });
    }
    for i in 0..(n - 1) {
        let base = (i * 2) as u32;
        mesh.indices.extend_from_slice(&[
            base,
            base + 1,
            base + 2,
            base + 1,
            base + 3,
            base + 2,
        ]);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// Display superior desplazable de 3 segundos con 3 trazas:
/// - Blanco: estado del Gate (`0.0` al pasar audio, `-1.0` cuando el Gate silencia).
/// - Azul: contribución de Rectify + Clip apilada sobre la traza Gate.
/// - Púrpura: contribución de Quantize apilada sobre la traza Shape.
fn paint_top_display(painter: &Painter, rect: Rect, snap: &Bit8TelemetrySnapshot) {
    painter.rect_filled(rect, CornerRadius::same(3), COL_DISPLAY_BG);

    let inner = rect.shrink2(vec2(1.5, 3.0));
    let w = inner.width().max(8.0);
    let h = inner.height().max(8.0);

    // 1. Grilla horizontal de 7 líneas sobre [-1.0, +1.0] (centro Y=0 más nítido).
    for i in 0..=6 {
        let frac = i as f32 / 6.0;
        let y = inner.min.y + frac * h;
        let col = if i == 3 {
            Color32::from_rgba_unmultiplied(0x9C, 0xA0, 0xAC, 64)
        } else {
            Color32::from_rgba_unmultiplied(0x78, 0x7C, 0x86, 30)
        };
        painter.line_segment([pos2(inner.min.x, y), pos2(inner.max.x, y)], Stroke::new(1.0_f32, col));
    }

    // 2. Convertir las 3 trazas desplazables (180 muestras ≈ 3.0 s) a coordenadas de pantalla.
    let map_y = |v: f32| -> f32 {
        let norm = (v.clamp(-1.0, 1.0) + 1.0) * 0.5;
        inner.max.y - norm * h
    };

    let n = BIT8_DISPLAY_HISTORY;
    let inv_n = 1.0 / (n - 1) as f32;
    let mut gate_pts = Vec::with_capacity(n);
    let mut clip_pts = Vec::with_capacity(n);
    let mut quant_pts = Vec::with_capacity(n);

    for i in 0..n {
        let x = inner.min.x + (i as f32 * inv_n) * w;
        gate_pts.push(pos2(x, map_y(snap.gate_trace[i])));
        clip_pts.push(pos2(x, map_y(snap.clip_trace[i])));
        quant_pts.push(pos2(x, map_y(snap.quant_trace[i])));
    }

    // 3. Bandas translúcidas entre capas (púrpura sobre azul, azul sobre blanco).
    paint_ribbon_between(
        painter,
        &quant_pts,
        &clip_pts,
        Color32::from_rgba_unmultiplied(0xA8, 0x56, 0xF5, 34),
    );
    paint_ribbon_between(
        painter,
        &clip_pts,
        &gate_pts,
        Color32::from_rgba_unmultiplied(0x46, 0x9E, 0xF5, 40),
    );

    // 4. Trazas en orden: Púrpura -> Azul -> Blanco encima (halo lavanda/blanco en reposo).
    painter.add(egui::Shape::line(
        quant_pts,
        Stroke::new(2.0_f32, Color32::from_rgba_unmultiplied(0xB8, 0x62, 0xFF, 225)),
    ));
    painter.add(egui::Shape::line(
        clip_pts,
        Stroke::new(1.6_f32, Color32::from_rgba_unmultiplied(0x4A, 0xA2, 0xF8, 235)),
    ));
    painter.add(egui::Shape::line(
        gate_pts,
        Stroke::new(1.25_f32, Color32::from_rgba_unmultiplied(0xDC, 0xE0, 0xE8, 240)),
    ));

    painter.rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(1.0_f32, Color32::from_rgb(0x1E, 0x20, 0x26)),
        egui::StrokeKind::Inside,
    );
}

fn draw_knob_cell(
    ui: &mut egui::Ui,
    center_x: f32,
    top_y: f32,
    diameter: f32,
    norm: &mut f32,
    default_norm: f32,
    arc_color: Color32,
    bottom_label: Option<&str>,
    tooltip: &str,
) -> bool {
    let knob_rect =
        Rect::from_min_size(pos2(center_x - diameter * 0.5, top_y), vec2(diameter, diameter));
    let mut knob_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(knob_rect)
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    let mut pct = norm.clamp(0.0, 1.0) * 100.0;
    let (resp, changed) = bitwig_kit::rotary_knob(
        &mut knob_ui,
        diameter,
        &mut pct,
        0.0..=100.0,
        default_norm * 100.0,
        arc_color,
        false,
    );
    if changed {
        *norm = (pct / 100.0).clamp(0.0, 1.0);
    }
    resp.on_hover_text(tooltip);

    if let Some(lbl) = bottom_label {
        ui.painter().text(
            pos2(center_x, knob_rect.max.y + 8.5),
            Align2::CENTER_CENTER,
            lbl,
            fonts::regular(9.5),
            Color32::from_rgb(0xCE, 0xD1, 0xD8),
        );
    }
    changed
}

pub fn render_module(ui: &mut egui::Ui, app: &mut AppState, track_idx: usize, slot_idx: usize) {
    let key = (track_idx, slot_idx);
    let engine_track_idx = track_idx.checked_sub(1);

    // Asegurar carga y sincronización inicial con el motor de audio antes de leer telemetría.
    let initial_params = {
        let mut map = state_map().lock().unwrap();
        map.entry(key).or_default().params
    };
    if let Some(et_idx) = engine_track_idx {
        if loaded_set().lock().unwrap().insert(key) {
            app.audio_proxy.send(crate::audio_proxy::GuiCommand::LoadNativeEffect {
                track_idx: et_idx,
                slot_idx,
                name: "Hikaru Open8-Bit".to_string(),
            });
            push_all_params(app, et_idx, slot_idx, &initial_params);
        }
    }

    // Leer estado + snapshot lock-free de telemetría en vivo.
    let (mut params, mut wet_fx_open, snap) = {
        let mut map = state_map().lock().unwrap();
        let st = map.entry(key).or_default();
        if let Some(et_idx) = engine_track_idx {
            if let Some(telem) = hikaru_dsp::effects::bit8::telemetry::lookup((et_idx, slot_idx)) {
                st.last_snapshot = telem.snapshot();
            }
        }
        (st.params, st.wet_fx_open, st.last_snapshot)
    };

    // Repintar a 60 FPS mientras haya actividad visible en LEDs o trazas;
    // en reposo total hacer un chequeo ligero para despertar al instante al reproducir audio.
    let display_active = snap.gate_led > 0.002
        || snap.clip_led > 0.002
        || snap.quant_led > 0.002
        || snap.gate_trace.iter().any(|&v| v > -0.995)
        || snap.clip_trace.iter().any(|&v| v > -0.995)
        || snap.quant_trace.iter().any(|&v| v > -0.995);
    if display_active {
        ui.ctx().request_repaint();
    } else if engine_track_idx.is_some() {
        ui.ctx().request_repaint_after(Duration::from_millis(120));
    }

    let full_rect = ui.available_rect_before_wrap();
    let (_, _) = ui.allocate_exact_size(full_rect.size(), Sense::hover());

    let mut changed = false;
    let mut wet_toggle_changed = false;
    let def = Bit8Params::default();

    let pad = 3.0;
    let gap = 4.0;
    let top_y = full_rect.min.y + pad;
    let bot_y = full_rect.max.y - pad;
    let total_h = (bot_y - top_y).max(160.0);

    let clock_w = 62.0;
    let right_w = 64.0;
    let drawer_w = if wet_fx_open {
        BIT8_WET_FX_DRAWER_WIDTH - 4.0
    } else {
        0.0
    };
    let drawer_gap = if wet_fx_open { gap } else { 0.0 };
    let center_w = (full_rect.width()
        - pad * 2.0
        - clock_w
        - right_w
        - gap * 2.0
        - drawer_w
        - drawer_gap)
        .max(210.0);

    // =========================================================================
    // 1. COLUMNA IZQUIERDA: CLOCK (Amarillo)
    // =========================================================================
    let clock_rect = Rect::from_min_size(pos2(full_rect.min.x + pad, top_y), vec2(clock_w, total_h));
    paint_recessed_panel(ui.painter(), clock_rect);
    ui.painter().text(
        pos2(clock_rect.center().x, clock_rect.min.y + 10.0),
        Align2::CENTER_CENTER,
        "Clock",
        fonts::semibold(10.5),
        COL_CLOCK_YELLOW,
    );

    let k_diam = 34.0;
    let c_slot_h = (total_h - 22.0) / 3.0;

    let mut clock_norm = Bit8Params::clock_hz_to_norm(params.clock_hz);
    let clock_str = format_clock_hz(params.clock_hz);
    let clock_note = Bit8Params::clock_hz_note_label(params.clock_hz);
    if draw_knob_cell(
        ui,
        clock_rect.center().x,
        clock_rect.min.y + 20.0,
        k_diam,
        &mut clock_norm,
        Bit8Params::clock_hz_to_norm(def.clock_hz),
        COL_CLOCK_YELLOW,
        Some(&clock_str),
        &format!("Clock Frequency: {clock_str} ({clock_note})"),
    ) {
        params.clock_hz = Bit8Params::clock_norm_to_hz(clock_norm);
        changed = true;
    }

    let mut jitter_norm = params.jitter;
    if draw_knob_cell(
        ui,
        clock_rect.center().x,
        clock_rect.min.y + 20.0 + c_slot_h,
        k_diam,
        &mut jitter_norm,
        def.jitter,
        COL_CLOCK_YELLOW,
        Some("Jitter"),
        &format!("Clock Jitter: {:.1}%", params.jitter * 100.0),
    ) {
        params.jitter = jitter_norm;
        changed = true;
    }

    let mut chance_norm = params.chance;
    if draw_knob_cell(
        ui,
        clock_rect.center().x,
        clock_rect.min.y + 20.0 + c_slot_h * 2.0,
        k_diam,
        &mut chance_norm,
        def.chance,
        COL_CLOCK_YELLOW,
        Some("Chance"),
        &format!("Clock Chance: {:.1}%", params.chance * 100.0),
    ) {
        params.chance = chance_norm;
        changed = true;
    }

    // =========================================================================
    // 2. BLOQUE CENTRAL: DISPLAY 3-TRAZAS + GATE / SHAPE / QUANTIZE
    // =========================================================================
    let center_x = clock_rect.max.x + gap;
    let disp_h = (total_h * 0.27).clamp(48.0, 60.0);
    let disp_rect = Rect::from_min_size(pos2(center_x, top_y), vec2(center_w, disp_h));
    paint_top_display(ui.painter(), disp_rect, &snap);

    let sub_y = disp_rect.max.y + gap;
    let sub_h = (bot_y - sub_y).max(116.0);

    let usable_sub_w = (center_w - gap * 2.0).max(180.0);
    let gate_w = (usable_sub_w * 0.255).floor();
    let shape_w = (usable_sub_w * 0.33).floor();
    let quant_w = (usable_sub_w - gate_w - shape_w).max(88.0);
    let bottom_knob_y = sub_y + sub_h - 52.0;

    // --- 2a. GATE (Blanco) ---
    let gate_rect = Rect::from_min_size(pos2(center_x, sub_y), vec2(gate_w, sub_h));
    paint_recessed_panel(ui.painter(), gate_rect);
    paint_section_led_header(
        ui,
        gate_rect,
        ui.id().with(("open8_gate_hdr", track_idx, slot_idx)),
        "Gate",
        COL_GATE_WHITE,
        snap.gate_led,
        "Gate Activity LED — lights up when input amplitude exceeds Gate threshold",
    );

    let mut gate_norm = Bit8Params::db_val_to_norm(params.gate_db);
    let gate_str = format_db(params.gate_db);
    if draw_knob_cell(
        ui,
        gate_rect.center().x,
        bottom_knob_y,
        k_diam,
        &mut gate_norm,
        Bit8Params::db_val_to_norm(def.gate_db),
        COL_GATE_WHITE,
        Some(&gate_str),
        &format!("Gate Threshold: {gate_str} (post-quantize, keyed by input)"),
    ) {
        params.gate_db = Bit8Params::db_norm_to_val(gate_norm);
        changed = true;
    }

    // --- 2b. SHAPE (Azul) ---
    let shape_rect = Rect::from_min_size(pos2(gate_rect.max.x + gap, sub_y), vec2(shape_w, sub_h));
    paint_recessed_panel(ui.painter(), shape_rect);
    paint_section_led_header(
        ui,
        shape_rect,
        ui.id().with(("open8_shape_hdr", track_idx, slot_idx)),
        "Shape",
        COL_SHAPE_BLUE,
        snap.clip_led,
        "Shape Activity LED — lights up when Rectify or Clip is actively shaping",
    );

    // 3 botones vectoriales de rectificación (Off / Half / Full)
    let wave_row_y = shape_rect.min.y + 23.0;
    let wave_row_w = shape_w - 8.0;
    let wave_btn_w = (wave_row_w / 3.0).floor();
    for (idx, &mode) in Bit8Rectify::ALL.iter().enumerate() {
        let btn_r = Rect::from_min_size(
            pos2(shape_rect.min.x + 4.0 + idx as f32 * wave_btn_w, wave_row_y),
            vec2(wave_btn_w, 18.0),
        );
        let resp = ui
            .interact(
                btn_r,
                ui.id().with(("open8_rect", track_idx, slot_idx, idx)),
                Sense::click(),
            )
            .on_hover_text(format!("Rectify {}: {}", mode.label(), mode.description()));
        if resp.clicked() {
            params.rectify = mode;
            changed = true;
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let selected = params.rectify == mode;
        let icon_col = if selected {
            COL_SHAPE_BLUE
        } else if resp.hovered() {
            Color32::from_rgb(0xD4, 0xD8, 0xE0)
        } else {
            Color32::from_rgb(0x90, 0x94, 0x9D)
        };
        paint_rectify_icon(ui.painter(), btn_r.shrink2(vec2(3.0, 3.0)), mode, icon_col);
        if selected {
            ui.painter().line_segment(
                [
                    pos2(btn_r.min.x + 1.0, btn_r.max.y + 1.0),
                    pos2(btn_r.max.x - 1.0, btn_r.max.y + 1.0),
                ],
                Stroke::new(2.0_f32, COL_SHAPE_BLUE),
            );
        }
    }

    // Selector desplegable de Clip Mode (`Off`, `Soft Clip`, `Hard Clip`, `Fold`, `Wrap`)
    let shape_dd_rect = Rect::from_min_size(
        pos2(shape_rect.min.x + 5.0, wave_row_y + 24.0),
        vec2(shape_w - 10.0, 21.0),
    );
    let shape_dd_id = ui.id().with(("open8_shape_dd", track_idx, slot_idx));
    let shape_dd_resp = ui
        .interact(shape_dd_rect, shape_dd_id, Sense::click())
        .on_hover_text(format!(
            "Clip Mode: {} — {}",
            params.shape_mode.label(),
            params.shape_mode.description()
        ));
    if shape_dd_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    paint_bevel_box(ui.painter(), shape_dd_rect, false, shape_dd_resp.hovered(), COL_SHAPE_BLUE);
    ui.painter().text(
        pos2(shape_dd_rect.min.x + 6.0, shape_dd_rect.center().y),
        Align2::LEFT_CENTER,
        params.shape_mode.label(),
        fonts::semibold(9.5),
        theme::TEXT_PRIMARY,
    );
    paint_dropdown_arrow(
        ui.painter(),
        pos2(shape_dd_rect.max.x - 7.5, shape_dd_rect.center().y),
        Color32::from_rgb(0xB4, 0xB8, 0xC2),
    );
    let shape_popup_id = ui.id().with(("open8_shape_popup", track_idx, slot_idx));
    if shape_dd_resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(shape_popup_id));
    }
    egui::popup_below_widget(
        ui,
        shape_popup_id,
        &shape_dd_resp,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(96.0);
            for &m in &Bit8ShapeMode::ALL {
                let sel = params.shape_mode == m;
                let item = ui
                    .selectable_label(
                        sel,
                        egui::RichText::new(m.label())
                            .font(fonts::semibold(10.0))
                            .color(if sel { COL_SHAPE_BLUE } else { theme::TEXT_PRIMARY }),
                    )
                    .on_hover_text(m.description());
                if item.clicked() {
                    params.shape_mode = m;
                    changed = true;
                    ui.close_menu();
                }
            }
        },
    );

    // Knob azul Drive (0 .. +36 dB): aparece SOLO cuando Clip Mode != Off.
    if params.shape_mode != Bit8ShapeMode::Off {
        let mut drive_norm = (params.shape_drive_db / BIT8_MAX_DRIVE_DB).clamp(0.0, 1.0);
        let drive_str = format!("+{:.1} dB", params.shape_drive_db);
        if draw_knob_cell(
            ui,
            shape_rect.center().x,
            bottom_knob_y,
            k_diam,
            &mut drive_norm,
            def.shape_drive_db / BIT8_MAX_DRIVE_DB,
            COL_SHAPE_BLUE,
            Some(&drive_str),
            &format!("Clip Drive: {drive_str} (threshold ±0.5)"),
        ) {
            params.shape_drive_db = drive_norm * BIT8_MAX_DRIVE_DB;
            changed = true;
        }
    }

    // --- 2c. QUANTIZE (Púrpura) ---
    let quant_rect = Rect::from_min_size(pos2(shape_rect.max.x + gap, sub_y), vec2(quant_w, sub_h));
    paint_recessed_panel(ui.painter(), quant_rect);
    paint_section_led_header(
        ui,
        quant_rect,
        ui.id().with(("open8_quant_hdr", track_idx, slot_idx)),
        "Quantize",
        COL_QUANT_PURPLE,
        snap.quant_led,
        "Quantize Activity LED — lights up when quantization is altering the signal",
    );

    // Dropdown de modo Quantize (`Off-zero`, `On-zero`, `Diffusion`, `Dither`)
    let q_dd_rect = Rect::from_min_size(
        pos2(quant_rect.min.x + 5.0, quant_rect.min.y + 22.0),
        vec2(quant_w - 10.0, 21.0),
    );
    let q_dd_resp = ui
        .interact(q_dd_rect, ui.id().with(("open8_q_dd", track_idx, slot_idx)), Sense::click())
        .on_hover_text(format!(
            "Quantization Mode: {} — {}",
            params.quantize_mode.label(),
            params.quantize_mode.description()
        ));
    if q_dd_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    paint_bevel_box(ui.painter(), q_dd_rect, false, q_dd_resp.hovered(), COL_QUANT_PURPLE);
    let icon_rect = Rect::from_min_size(
        pos2(q_dd_rect.min.x + 5.0, q_dd_rect.center().y - 6.0),
        vec2(12.0, 12.0),
    );
    paint_quantize_mode_icon(
        ui.painter(),
        icon_rect,
        params.quantize_mode,
        Color32::from_rgb(0xE0, 0xE2, 0xE8),
    );
    ui.painter().text(
        pos2(icon_rect.max.x + 5.0, q_dd_rect.center().y),
        Align2::LEFT_CENTER,
        params.quantize_mode.label(),
        fonts::semibold(9.5),
        theme::TEXT_PRIMARY,
    );
    paint_dropdown_arrow(
        ui.painter(),
        pos2(q_dd_rect.max.x - 7.5, q_dd_rect.center().y),
        Color32::from_rgb(0xB4, 0xB8, 0xC2),
    );
    let q_popup_id = ui.id().with(("open8_q_popup", track_idx, slot_idx));
    if q_dd_resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(q_popup_id));
    }
    egui::popup_below_widget(
        ui,
        q_popup_id,
        &q_dd_resp,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(104.0);
            for &m in &Bit8QuantizeMode::ALL {
                let sel = params.quantize_mode == m;
                let item = ui
                    .selectable_label(
                        sel,
                        egui::RichText::new(m.label())
                            .font(fonts::semibold(10.0))
                            .color(if sel { COL_QUANT_PURPLE } else { theme::TEXT_PRIMARY }),
                    )
                    .on_hover_text(m.description());
                if item.clicked() {
                    params.quantize_mode = m;
                    changed = true;
                    ui.close_menu();
                }
            }
        },
    );

    // Controles dinámicos según Quantization Mode (fila intermedia)
    let mid_row_cy = ((q_dd_rect.max.y + bottom_knob_y) * 0.5).max(q_dd_rect.max.y + 13.0);
    let q_knob_cx = quant_rect.min.x + 29.0;
    let right_ctrl_cx = quant_rect.max.x - 21.0;

    match params.quantize_mode {
        Bit8QuantizeMode::Diffusion => {
            paint_sunburst_icon(
                ui.painter(),
                pos2(q_knob_cx + 4.0, mid_row_cy),
                params.diffusion_brightness,
            );
            let mut diff_norm = params.diffusion_brightness;
            let diff_d = 24.0;
            if draw_knob_cell(
                ui,
                right_ctrl_cx,
                mid_row_cy - diff_d * 0.5,
                diff_d,
                &mut diff_norm,
                def.diffusion_brightness,
                Color32::from_rgb(0xC8, 0xCC, 0xD4),
                None,
                &format!("Diffusion Brightness: {:.1}%", params.diffusion_brightness * 100.0),
            ) {
                params.diffusion_brightness = diff_norm;
                changed = true;
            }
        }
        Bit8QuantizeMode::Dither => {
            ui.painter().text(
                pos2(q_knob_cx, mid_row_cy),
                Align2::CENTER_CENTER,
                "Noise",
                fonts::regular(9.8),
                Color32::from_rgb(0xD2, 0xD5, 0xDC),
            );
            let st_rect = Rect::from_center_size(pos2(right_ctrl_cx, mid_row_cy), vec2(26.0, 20.0));
            let st_resp = ui
                .interact(
                    st_rect,
                    ui.id().with(("open8_stereo_dither", track_idx, slot_idx)),
                    Sense::click(),
                )
                .on_hover_text("Stereo Dither — independent L/R triangular dither noise");
            if st_resp.clicked() {
                params.stereo_dither = !params.stereo_dither;
                changed = true;
            }
            if st_resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            paint_bevel_box(
                ui.painter(),
                st_rect,
                params.stereo_dither,
                st_resp.hovered(),
                COL_OUT_ORANGE,
            );
            paint_stereo_venn_icon(
                ui.painter(),
                st_rect,
                if params.stereo_dither {
                    Color32::from_rgb(0x14, 0x12, 0x10)
                } else {
                    Color32::from_rgb(0xD8, 0xDC, 0xE4)
                },
            );
        }
        Bit8QuantizeMode::OffZero | Bit8QuantizeMode::OnZero => {}
    }

    // Fila inferior: Knob púrpura Quantization Level + botón naranja Squared Domain (x²)
    let mut quant_norm = Bit8Params::db_val_to_norm(params.quantize_db);
    let quant_str = format_db(params.quantize_db);
    if draw_knob_cell(
        ui,
        q_knob_cx,
        bottom_knob_y,
        k_diam,
        &mut quant_norm,
        Bit8Params::db_val_to_norm(def.quantize_db),
        COL_QUANT_PURPLE,
        Some(&quant_str),
        &format!("Quantization Level: {quant_str}"),
    ) {
        params.quantize_db = Bit8Params::db_norm_to_val(quant_norm);
        changed = true;
    }

    let sq_rect = Rect::from_center_size(
        pos2(right_ctrl_cx, bottom_knob_y + k_diam * 0.52),
        vec2(26.0, 22.0),
    );
    let sq_resp = ui
        .interact(sq_rect, ui.id().with(("open8_sq_dom", track_idx, slot_idx)), Sense::click())
        .on_hover_text("Quantization in Squared Domain (x² — finer low-level steps, coarser peaks)");
    if sq_resp.clicked() {
        params.squared_domain = !params.squared_domain;
        changed = true;
    }
    if sq_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    paint_bevel_box(
        ui.painter(),
        sq_rect,
        params.squared_domain,
        sq_resp.hovered(),
        COL_OUT_ORANGE,
    );
    paint_squared_domain_icon(
        ui.painter(),
        sq_rect,
        if params.squared_domain {
            Color32::from_rgb(0x14, 0x12, 0x10)
        } else {
            Color32::from_rgb(0xD8, 0xDC, 0xE4)
        },
    );

    // =========================================================================
    // 3. COLUMNA DERECHA: WET FX / ANTI-ALIAS / WIDTH / MIX (Naranja)
    // =========================================================================
    let right_x = disp_rect.max.x + gap;

    // Pestaña superior Wet FX
    let wet_tab_h = 30.0;
    let wet_tab_rect = Rect::from_min_size(pos2(right_x, top_y), vec2(right_w, wet_tab_h));
    let wet_resp = ui
        .interact(wet_tab_rect, ui.id().with(("open8_wet_tab", track_idx, slot_idx)), Sense::click())
        .on_hover_text("Toggle Wet FX Post-Filter (HP/LP) & Gain");
    if wet_resp.clicked() {
        wet_fx_open = !wet_fx_open;
        wet_toggle_changed = true;
    }
    if wet_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    paint_recessed_panel(ui.painter(), wet_tab_rect);
    let top_bar_col = if wet_fx_open {
        theme::BW_LCD_CYAN
    } else if wet_resp.hovered() {
        COL_WET_CYAN
    } else {
        Color32::from_rgb(0x2D, 0x5A, 0x72)
    };
    ui.painter().line_segment(
        [
            pos2(wet_tab_rect.min.x + 3.0, wet_tab_rect.min.y + 3.0),
            pos2(wet_tab_rect.max.x - 3.0, wet_tab_rect.min.y + 3.0),
        ],
        Stroke::new(2.0_f32, top_bar_col),
    );
    ui.painter().text(
        pos2(wet_tab_rect.center().x, wet_tab_rect.center().y + 2.0),
        Align2::CENTER_CENTER,
        if wet_fx_open { "Wet FX ›" } else { "Wet FX" },
        fonts::semibold(10.0),
        if wet_fx_open || wet_resp.hovered() {
            Color32::WHITE
        } else {
            Color32::from_rgb(0xE2, 0xE5, 0xEC)
        },
    );

    // Panel inferior derecho: Anti-alias, Width, Mix
    let out_rect = Rect::from_min_max(
        pos2(right_x, wet_tab_rect.max.y + gap),
        pos2(right_x + right_w, bot_y),
    );
    paint_recessed_panel(ui.painter(), out_rect);

    let aa_rect = Rect::from_min_size(
        pos2(out_rect.min.x + 4.0, out_rect.min.y + 6.0),
        vec2(right_w - 8.0, 21.0),
    );
    let aa_resp = ui
        .interact(aa_rect, ui.id().with(("open8_aa", track_idx, slot_idx)), Sense::click())
        .on_hover_text("Anti-alias — 1st-order ADAA over Rectify, Clip, Quantize & Gate");
    if aa_resp.clicked() {
        params.anti_alias = !params.anti_alias;
        changed = true;
    }
    if aa_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    paint_bevel_box(ui.painter(), aa_rect, params.anti_alias, aa_resp.hovered(), COL_OUT_ORANGE);
    ui.painter().text(
        aa_rect.center(),
        Align2::CENTER_CENTER,
        "Anti-alias",
        fonts::semibold(9.5),
        if params.anti_alias {
            Color32::from_rgb(0x14, 0x12, 0x10)
        } else {
            Color32::from_rgb(0xE4, 0xE6, 0xEC)
        },
    );

    let out_knobs_top = aa_rect.max.y + 12.0;
    let out_slot_h = ((out_rect.max.y - out_knobs_top) * 0.5).max(52.0);

    let mut width_norm = params.width;
    if draw_knob_cell(
        ui,
        out_rect.center().x,
        out_knobs_top,
        k_diam,
        &mut width_norm,
        def.width,
        COL_OUT_ORANGE,
        Some("Width"),
        &format!("Stereo Width: {:.1}%", params.width * 100.0),
    ) {
        params.width = width_norm;
        changed = true;
    }

    let mut mix_norm = params.mix;
    if draw_knob_cell(
        ui,
        out_rect.center().x,
        out_knobs_top + out_slot_h,
        k_diam,
        &mut mix_norm,
        def.mix,
        COL_OUT_ORANGE,
        Some("Mix"),
        &format!("Dry/Wet Mix: {:.1}%", params.mix * 100.0),
    ) {
        params.mix = mix_norm;
        changed = true;
    }

    // =========================================================================
    // 4. PANEL EXPANDIBLE WET FX (opcional al hacer clic en `Wet FX`)
    // =========================================================================
    if wet_fx_open {
        let drawer_rect = Rect::from_min_size(pos2(out_rect.max.x + gap, top_y), vec2(drawer_w, total_h));
        paint_recessed_panel(ui.painter(), drawer_rect);
        ui.painter().text(
            pos2(drawer_rect.center().x, drawer_rect.min.y + 10.0),
            Align2::CENTER_CENTER,
            "WET FX",
            fonts::semibold(9.5),
            theme::BW_LCD_CYAN,
        );
        let d_slot_h = (total_h - 22.0) / 3.0;
        let dk = 30.0;

        let mut hp_norm = Bit8Params::wet_hp_hz_to_norm(params.wet_hp_hz);
        let hp_lbl = if params.wet_hp_hz <= 20.5 {
            "HP Off".to_string()
        } else {
            format!("HP {:.0}", params.wet_hp_hz)
        };
        if draw_knob_cell(
            ui,
            drawer_rect.center().x,
            drawer_rect.min.y + 20.0,
            dk,
            &mut hp_norm,
            Bit8Params::wet_hp_hz_to_norm(def.wet_hp_hz),
            theme::BW_LCD_CYAN,
            Some(&hp_lbl),
            &format!("Wet High-Pass: {:.0} Hz", params.wet_hp_hz),
        ) {
            params.wet_hp_hz = Bit8Params::wet_hp_norm_to_hz(hp_norm);
            changed = true;
        }

        let mut lp_norm = Bit8Params::wet_lp_hz_to_norm(params.wet_lp_hz);
        let lp_lbl = if params.wet_lp_hz >= 19_900.0 {
            "LP Off".to_string()
        } else {
            format!("LP {:.1}k", params.wet_lp_hz / 1000.0)
        };
        if draw_knob_cell(
            ui,
            drawer_rect.center().x,
            drawer_rect.min.y + 20.0 + d_slot_h,
            dk,
            &mut lp_norm,
            Bit8Params::wet_lp_hz_to_norm(def.wet_lp_hz),
            theme::BW_LCD_CYAN,
            Some(&lp_lbl),
            &format!("Wet Low-Pass: {:.0} Hz", params.wet_lp_hz),
        ) {
            params.wet_lp_hz = Bit8Params::wet_lp_norm_to_hz(lp_norm);
            changed = true;
        }

        let mut gain_norm = ((params.wet_gain_db + 12.0) / 24.0).clamp(0.0, 1.0);
        let gain_lbl = format!("{:+.1} dB", params.wet_gain_db);
        if draw_knob_cell(
            ui,
            drawer_rect.center().x,
            drawer_rect.min.y + 20.0 + d_slot_h * 2.0,
            dk,
            &mut gain_norm,
            0.5,
            COL_OUT_ORANGE,
            Some(&gain_lbl),
            &format!("Wet Gain: {gain_lbl}"),
        ) {
            params.wet_gain_db = -12.0 + gain_norm * 24.0;
            changed = true;
        }
    }

    if changed || wet_toggle_changed {
        {
            let mut map = state_map().lock().unwrap();
            let st = map.entry(key).or_default();
            st.params = params;
            st.wet_fx_open = wet_fx_open;
        }
        if changed {
            if let Some(et_idx) = engine_track_idx {
                push_all_params(app, et_idx, slot_idx, &params);
            }
        }
    }
}
