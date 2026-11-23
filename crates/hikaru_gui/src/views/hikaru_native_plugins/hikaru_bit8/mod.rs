// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_bit8/mod.rs

//! **Bit-8**: panel nativo estilo **Bitwig Studio** para el efecto `hikaru_dsp::effects::Bit8Effect`.
//!
//! Replica el diseño, paleta de colores, iconos vectoriales y controles del Bit-8 de Bitwig:
//! - Columna izquierda **Clock** (amarillo): `Clock` (`10 Hz .. 100.0 kHz`), `Jitter`, `Chance`.
//! - Display superior oscuro: grilla horizontal de niveles de cuantización (lineal o
//!   `Squared Domain`), línea de umbral `Gate`, barra base púrpura y señal/curva en vivo.
//! - Fila inferior central:
//!   - **Gate** (blanco): checkbox + knob (`-96.0 .. 0.0 dB`, default `-58.2 dB`).
//!   - **Shape** (azul): checkbox + 3 iconos de rectificación (`Normal`, `Half-wave`, `Full-wave`)
//!     + selector de modo (`Off`, `Clip`, `Soft`, `Fold`, `Wrap`, `Over`) + knob `Drive` al activar.
//!   - **Quantize** (púrpura): checkbox + selector de modo con icono de escalones (`Off-zero`,
//!     `At-zero`, `Truncate`, `Ceil`) + knob (`-96.0 .. 0.0 dB`, default `-39.1 dB`) + botón
//!     vectorial **Squared Domain**.
//! - Columna derecha (naranja): pestaña expandible **Wet FX** (`HP`, `LP`, `Gain`), botón
//!   **Anti-alias**, knobs **Width** y **Mix**.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use egui::{pos2, vec2, Align2, Color32, CornerRadius, CursorIcon, Painter, Pos2, Rect, Sense, Stroke};
use hikaru_dsp::effects::{
    Bit8Params, Bit8QuantizeMode, Bit8Rectify, Bit8ShapeMode, BIT8_PARAM_COUNT,
};
use hikaru_plugin_host::PluginTap;

use crate::app::AppState;
use crate::theme;
use crate::ui::bitwig_kit;
use crate::ui::fonts;

pub const BIT8_BASE_CARD_WIDTH: f32 = 398.0;
pub const BIT8_WET_FX_DRAWER_WIDTH: f32 = 84.0;

// Paleta calibrada 1:1 contra Bitwig Studio Bit-8
const COL_PANEL_BG: Color32 = Color32::from_rgb(0x23, 0x25, 0x29);
const COL_PANEL_BORDER: Color32 = Color32::from_rgb(0x18, 0x1A, 0x1E);
const COL_DISPLAY_BG: Color32 = Color32::from_rgb(0x12, 0x13, 0x16);
const COL_CTRL_BG: Color32 = Color32::from_rgb(0x3B, 0x3D, 0x43);
const COL_CTRL_HOVER: Color32 = Color32::from_rgb(0x49, 0x4C, 0x54);
const COL_CTRL_BORDER: Color32 = Color32::from_rgb(0x1B, 0x1D, 0x21);
const COL_CHECK_OFF: Color32 = Color32::from_rgb(0x1E, 0x20, 0x24);
const COL_CHECK_BORDER: Color32 = Color32::from_rgb(0x34, 0x37, 0x3F);

const COL_CLOCK_YELLOW: Color32 = Color32::from_rgb(0xEB, 0xCC, 0x34);
const COL_GATE_WHITE: Color32 = Color32::from_rgb(0xE6, 0xE8, 0xEC);
const COL_SHAPE_BLUE: Color32 = Color32::from_rgb(0x46, 0x9E, 0xF5);
const COL_QUANT_PURPLE: Color32 = Color32::from_rgb(0xB8, 0x5C, 0xFF);
const COL_OUT_ORANGE: Color32 = Color32::from_rgb(0xF5, 0x7A, 0x1A);
const COL_WET_CYAN: Color32 = Color32::from_rgb(0x3D, 0x8F, 0xB8);

pub fn is_bit8_name(name: &str) -> bool {
    matches!(name, "Bit-8" | "Bit 8" | "Hikaru Bit-8")
}

struct TapGuard {
    tap: Arc<PluginTap>,
}

impl TapGuard {
    fn attach(tap: Arc<PluginTap>) -> Self {
        tap.attach_consumer();
        Self { tap }
    }
}

impl Drop for TapGuard {
    fn drop(&mut self) {
        self.tap.detach_consumer();
    }
}

struct Bit8SlotUiState {
    params: Bit8Params,
    wet_fx_open: bool,
    live_peak: f32,
    live_wave: [f32; 48],
    scratch: Vec<f32>,
    tap_guard: Option<TapGuard>,
}

impl Default for Bit8SlotUiState {
    fn default() -> Self {
        Self {
            params: Bit8Params::default(),
            wet_fx_open: false,
            live_peak: 0.0,
            live_wave: [0.0; 48],
            scratch: Vec::with_capacity(2048),
            tap_guard: None,
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
            name: "Default (Bitwig)",
            params: Bit8Params::default(),
        },
        Bit8Preset {
            name: "8-Bit Console",
            params: Bit8Params {
                clock_hz: 22_050.0,
                jitter: 0.0,
                chance: 1.0,
                gate_db: -64.0,
                quantize_mode: Bit8QuantizeMode::AtZero,
                quantize_db: -48.0,
                ..Bit8Params::default()
            },
        },
        Bit8Preset {
            name: "Amiga 8-Bit",
            params: Bit8Params {
                clock_hz: 16_000.0,
                jitter: 0.15,
                chance: 1.0,
                gate_db: -60.0,
                quantize_mode: Bit8QuantizeMode::OffZero,
                quantize_db: -42.0,
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
                gate_db: -50.0,
                shape_mode: Bit8ShapeMode::Soft,
                shape_drive_db: 8.0,
                quantize_db: -30.0,
                wet_hp_hz: 320.0,
                wet_lp_hz: 3_400.0,
                ..Bit8Params::default()
            },
        },
        Bit8Preset {
            name: "Clock Glitch",
            params: Bit8Params {
                clock_hz: 6_500.0,
                jitter: 0.85,
                chance: 0.62,
                gate_db: -58.2,
                quantize_db: -24.0,
                ..Bit8Params::default()
            },
        },
        Bit8Preset {
            name: "Squared Grit",
            params: Bit8Params {
                clock_hz: 18_000.0,
                jitter: 0.35,
                shape_mode: Bit8ShapeMode::Soft,
                shape_drive_db: 12.0,
                quantize_db: -18.0,
                squared_domain: true,
                ..Bit8Params::default()
            },
        },
        Bit8Preset {
            name: "Wavefolder Crush",
            params: Bit8Params {
                clock_hz: 12_000.0,
                jitter: 0.2,
                shape_mode: Bit8ShapeMode::Fold,
                shape_drive_db: 18.0,
                quantize_db: -22.0,
                anti_alias: true,
                ..Bit8Params::default()
            },
        },
        Bit8Preset {
            name: "Half-Wave Fuzz",
            params: Bit8Params {
                clock_hz: 32_000.0,
                jitter: 0.1,
                rectify: Bit8Rectify::HalfWave,
                shape_mode: Bit8ShapeMode::Over,
                shape_drive_db: 16.0,
                quantize_db: -28.0,
                ..Bit8Params::default()
            },
        },
    ]
}

fn push_all_params(app: &mut AppState, engine_track_idx: usize, slot_idx: usize, params: &Bit8Params) {
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

pub fn render_preset_menu(ui: &mut egui::Ui, app: &mut AppState, track_idx: usize, slot_idx: usize) {
    ui.set_min_width(156.0);
    ui.label(
        egui::RichText::new("Bit-8 Presets")
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
        if khz >= 100.0 {
            format!("{khz:.1} kHz")
        } else if khz >= 10.0 {
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
    if db <= -95.5 {
        "-∞ dB".to_string()
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
    // Sutil brillo superior estilo Bitwig 5.1
    let hi = if active {
        Color32::from_rgba_unmultiplied(255, 255, 255, 55)
    } else {
        Color32::from_rgba_unmultiplied(255, 255, 255, 26)
    };
    painter.line_segment(
        [pos2(rect.min.x + 2.0, rect.min.y + 1.0), pos2(rect.max.x - 2.0, rect.min.y + 1.0)],
        Stroke::new(1.0_f32, hi),
    );
    painter.rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(1.0_f32, COL_CTRL_BORDER),
        egui::StrokeKind::Inside,
    );
}

fn section_checkbox_header(
    ui: &mut egui::Ui,
    rect: Rect,
    id: egui::Id,
    label: &str,
    accent: Color32,
    enabled: &mut bool,
) -> bool {
    let resp = ui.interact(rect, id, Sense::click());
    let mut changed = false;
    if resp.clicked() {
        *enabled = !*enabled;
        changed = true;
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    let p = ui.painter();
    let box_rect = Rect::from_min_size(pos2(rect.min.x + 1.0, rect.center().y - 5.0), vec2(10.0, 10.0));
    p.rect_filled(box_rect, CornerRadius::same(2), COL_CHECK_OFF);
    p.rect_stroke(
        box_rect,
        CornerRadius::same(2),
        Stroke::new(
            1.0_f32,
            if resp.hovered() {
                accent
            } else {
                COL_CHECK_BORDER
            },
        ),
        egui::StrokeKind::Inside,
    );
    if !*enabled {
        // Indicador sutil de sección en bypass (línea diagonal)
        p.line_segment(
            [box_rect.min + vec2(2.0, 2.0), box_rect.max - vec2(2.0, 2.0)],
            Stroke::new(1.1_f32, Color32::from_rgb(0x6E, 0x72, 0x7B)),
        );
    } else if resp.hovered() {
        p.rect_filled(box_rect.shrink(3.0), CornerRadius::same(1), accent);
    }

    let text_col = if *enabled {
        accent
    } else {
        Color32::from_rgb(0x6B, 0x6F, 0x78)
    };
    p.text(
        pos2(box_rect.max.x + 5.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        fonts::semibold(10.5),
        text_col,
    );
    changed
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
            Bit8Rectify::Normal => cy - s * amp,
            Bit8Rectify::HalfWave => cy + amp * 0.25 - s.max(0.0) * amp * 1.15,
            Bit8Rectify::FullWave => cy + amp * 0.45 - s.abs() * amp * 1.1,
        };
        pts.push(pos2(x, y));
    }
    painter.add(egui::Shape::line(pts, Stroke::new(1.35_f32, color)));
}

fn paint_quantize_mode_icon(painter: &Painter, rect: Rect, mode: Bit8QuantizeMode, color: Color32) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    let axis_col = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 110);
    // Eje central vertical y horizontal sutil
    painter.line_segment([pos2(cx, rect.min.y), pos2(cx, rect.max.y)], Stroke::new(0.8_f32, axis_col));
    painter.line_segment([pos2(rect.min.x, cy), pos2(rect.max.x, cy)], Stroke::new(0.8_f32, axis_col));

    let st = Stroke::new(1.3_f32, color);
    let w = rect.width();
    let h = rect.height();
    match mode {
        Bit8QuantizeMode::OffZero => {
            // Escalones desplazados del cero (mid-riser)
            painter.line_segment([pos2(rect.min.x + 1.0, rect.max.y - 1.5), pos2(cx - 2.0, rect.max.y - 1.5)], st);
            painter.line_segment([pos2(cx - 2.0, cy + h * 0.18), pos2(cx, cy + h * 0.18)], st);
            painter.line_segment([pos2(cx, cy - h * 0.18), pos2(cx + 2.5, cy - h * 0.18)], st);
            painter.line_segment([pos2(cx + 2.5, rect.min.y + 1.5), pos2(rect.max.x - 1.0, rect.min.y + 1.5)], st);
        }
        Bit8QuantizeMode::AtZero => {
            // Escalón centrado sobre cero (mid-tread)
            painter.line_segment([pos2(rect.min.x + 1.0, rect.max.y - 2.0), pos2(cx - w * 0.18, rect.max.y - 2.0)], st);
            painter.line_segment([pos2(cx - w * 0.18, cy), pos2(cx + w * 0.18, cy)], st);
            painter.line_segment([pos2(cx + w * 0.18, rect.min.y + 2.0), pos2(rect.max.x - 1.0, rect.min.y + 2.0)], st);
        }
        Bit8QuantizeMode::Truncate => {
            painter.line_segment([pos2(rect.min.x + 1.0, cy + h * 0.25), pos2(cx - 1.0, cy + h * 0.25)], st);
            painter.line_segment([pos2(cx - 1.0, cy), pos2(cx + 1.0, cy)], st);
            painter.line_segment([pos2(cx + 1.0, cy - h * 0.25), pos2(rect.max.x - 1.0, cy - h * 0.25)], st);
        }
        Bit8QuantizeMode::Ceil => {
            painter.line_segment([pos2(rect.min.x + 1.0, rect.max.y - 1.5), pos2(cx - 1.0, rect.max.y - 1.5)], st);
            painter.line_segment([pos2(cx + 1.0, rect.min.y + 1.5), pos2(rect.max.x - 1.0, rect.min.y + 1.5)], st);
        }
    }
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
    let faint = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 150);
    painter.add(egui::Shape::line(upper, Stroke::new(1.15_f32, faint)));
    painter.add(egui::Shape::line(lower, Stroke::new(1.15_f32, faint)));
    painter.line_segment([pos2(r.min.x, r.max.y), pos2(r.max.x, r.min.y)], Stroke::new(1.3_f32, color));
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

fn paint_top_display(
    painter: &Painter,
    rect: Rect,
    params: &Bit8Params,
    live_peak: f32,
    live_wave: &[f32; 48],
) {
    painter.rect_filled(rect, CornerRadius::same(3), COL_DISPLAY_BG);

    let inner = rect.shrink2(vec2(2.0, 3.0));
    let bottom_y = inner.max.y - 3.0;
    let top_y = inner.min.y + 2.0;
    let usable_h = (bottom_y - top_y).max(8.0);

    // 1. Grilla horizontal de pasos de cuantización (estilo Bitwig Bit-8).
    let step = params.quantize_step();
    let quant_active = params.quantize_enabled && params.quantize_db > -95.5;
    let grid_lines = if quant_active {
        let raw_steps = (1.0 / step).clamp(2.0, 18.0).round() as usize;
        raw_steps.clamp(2, 16)
    } else {
        8
    };

    let line_col = if quant_active {
        Color32::from_rgba_unmultiplied(0x98, 0x9B, 0xA6, 48)
    } else {
        Color32::from_rgba_unmultiplied(0x60, 0x64, 0x6E, 24)
    };

    for i in 1..=grid_lines {
        let frac = i as f32 / grid_lines as f32;
        let mapped = if params.squared_domain && quant_active {
            frac * frac
        } else {
            frac
        };
        let y = bottom_y - mapped * usable_h;
        painter.line_segment(
            [pos2(inner.min.x, y), pos2(inner.max.x, y)],
            Stroke::new(1.0_f32, line_col),
        );
    }

    // 2. Línea de umbral Gate (blanco sutil).
    if params.gate_enabled && params.gate_db > -95.5 {
        let gate_norm = Bit8Params::db_val_to_norm(params.gate_db);
        let gate_y = bottom_y - (gate_norm * gate_norm) * usable_h * 0.45;
        painter.line_segment(
            [pos2(inner.min.x, gate_y), pos2(inner.max.x, gate_y)],
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(0xE6, 0xE8, 0xEC, 110)),
        );
    }

    // 3. Vista previa sutil de Shape/Quantize cuando Shape está activo o señal en vivo cuando suena audio.
    if live_peak > 0.002 {
        // Señal de audio real del tap cuantizada sobre la grilla
        let fill_h = (live_peak.clamp(0.0, 1.0) * usable_h).max(2.0);
        let meter_rect = Rect::from_min_max(
            pos2(inner.min.x, bottom_y - fill_h),
            pos2(inner.max.x, bottom_y),
        );
        painter.rect_filled(
            meter_rect,
            0.0,
            Color32::from_rgba_unmultiplied(0xB8, 0x5C, 0xFF, 38),
        );

        let mut pts = Vec::with_capacity(live_wave.len());
        let mid_y = top_y + usable_h * 0.5;
        for (i, &s) in live_wave.iter().enumerate() {
            let x = inner.min.x + (i as f32 / (live_wave.len() - 1) as f32) * inner.width();
            let y = (mid_y - s.clamp(-1.0, 1.0) * (usable_h * 0.46)).clamp(top_y, bottom_y);
            pts.push(pos2(x, y));
        }
        painter.add(egui::Shape::line(
            pts,
            Stroke::new(1.3_f32, Color32::from_rgba_unmultiplied(0xD2, 0x9B, 0xFF, 220)),
        ));
    } else if params.shape_enabled
        && (params.shape_mode != Bit8ShapeMode::Off || params.rectify != Bit8Rectify::Normal)
    {
        // Cuando Shape está activo e inactivo el audio, muestra la forma de onda conformada
        let mut pts = Vec::with_capacity(65);
        let mid_y = top_y + usable_h * 0.5;
        for i in 0..=64 {
            let t = i as f32 / 64.0;
            let raw = (t * std::f32::consts::TAU).sin() * 0.85;
            let y_val = params.eval_transfer(raw);
            let x = inner.min.x + t * inner.width();
            let y = (mid_y - y_val * (usable_h * 0.44)).clamp(top_y, bottom_y);
            pts.push(pos2(x, y));
        }
        painter.add(egui::Shape::line(
            pts,
            Stroke::new(1.25_f32, Color32::from_rgba_unmultiplied(0x46, 0x9E, 0xF5, 190)),
        ));
    }

    // 4. Barra horizontal inferior púrpura/blanca característica del Bit-8 de Bitwig.
    let bar_col = if quant_active {
        Color32::from_rgb(0xA6, 0x82, 0xD8)
    } else {
        Color32::from_rgb(0x62, 0x66, 0x72)
    };
    painter.line_segment(
        [pos2(inner.min.x, bottom_y + 1.0), pos2(inner.max.x, bottom_y + 1.0)],
        Stroke::new(2.0_f32, bar_col),
    );

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
    bottom_label: &str,
    tooltip: &str,
) -> bool {
    let knob_rect = Rect::from_min_size(pos2(center_x - diameter * 0.5, top_y), vec2(diameter, diameter));
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

    ui.painter().text(
        pos2(center_x, knob_rect.max.y + 8.5),
        Align2::CENTER_CENTER,
        bottom_label,
        fonts::regular(9.5),
        Color32::from_rgb(0xCE, 0xD1, 0xD8),
    );
    changed
}

pub fn render_module(ui: &mut egui::Ui, app: &mut AppState, track_idx: usize, slot_idx: usize) {
    let key = (track_idx, slot_idx);
    let engine_track_idx = track_idx.checked_sub(1);

    // Leer estado + drenar tap de audio en vivo sin retener el mutex mientras se dibuja UI.
    let (mut params, mut wet_fx_open, live_peak, live_wave) = {
        let mut map = state_map().lock().unwrap();
        let st = map.entry(key).or_default();

        if let Some(et_idx) = engine_track_idx {
            if st.tap_guard.is_none() {
                if let Some(tap) = hikaru_plugin_host::tap::lookup((et_idx, slot_idx)) {
                    st.tap_guard = Some(TapGuard::attach(tap));
                }
            }
            let mut got_audio = false;
            if let Some(guard) = st.tap_guard.as_ref() {
                if guard.tap.take(&mut st.scratch) && st.scratch.len() >= 2 {
                    got_audio = true;
                    let frames = st.scratch.len() / 2;
                    let mut peak = 0.0_f32;
                    for ch in st.scratch.chunks_exact(2) {
                        let m = ch[0].abs().max(ch[1].abs());
                        if m > peak {
                            peak = m;
                        }
                    }
                    st.live_peak = peak.max(st.live_peak * 0.72);
                    let bins = st.live_wave.len();
                    for (i, out_s) in st.live_wave.iter_mut().enumerate() {
                        let f_idx = ((i * frames) / bins).min(frames - 1);
                        *out_s = 0.5 * (st.scratch[f_idx * 2] + st.scratch[f_idx * 2 + 1]);
                    }
                }
            }
            if !got_audio {
                st.live_peak *= 0.80;
                if st.live_peak < 0.001 {
                    st.live_peak = 0.0;
                }
            }
        }
        (st.params, st.wet_fx_open, st.live_peak, st.live_wave)
    };

    if live_peak > 0.002 {
        ui.ctx().request_repaint();
    }

    // Asegurar carga y sincronización inicial con el motor de audio.
    if let Some(et_idx) = engine_track_idx {
        if loaded_set().lock().unwrap().insert(key) {
            app.audio_proxy.send(crate::audio_proxy::GuiCommand::LoadNativeEffect {
                track_idx: et_idx,
                slot_idx,
                name: "Bit-8".to_string(),
            });
            push_all_params(app, et_idx, slot_idx, &params);
        }
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
    let drawer_w = if wet_fx_open { BIT8_WET_FX_DRAWER_WIDTH - 4.0 } else { 0.0 };
    let drawer_gap = if wet_fx_open { gap } else { 0.0 };
    let center_w = (full_rect.width() - pad * 2.0 - clock_w - right_w - gap * 2.0 - drawer_w - drawer_gap).max(196.0);

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
    if draw_knob_cell(
        ui,
        clock_rect.center().x,
        clock_rect.min.y + 20.0,
        k_diam,
        &mut clock_norm,
        Bit8Params::clock_hz_to_norm(def.clock_hz),
        COL_CLOCK_YELLOW,
        &clock_str,
        &format!("Clock: {clock_str}"),
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
        "Jitter",
        &format!("Jitter: {:.0}%", params.jitter * 100.0),
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
        "Chance",
        &format!("Chance: {:.0}%", params.chance * 100.0),
    ) {
        params.chance = chance_norm;
        changed = true;
    }

    // =========================================================================
    // 2. BLOQUE CENTRAL: DISPLAY SUPERIOR + GATE / SHAPE / QUANTIZE
    // =========================================================================
    let center_x = clock_rect.max.x + gap;
    let disp_h = (total_h * 0.28).clamp(50.0, 62.0);
    let disp_rect = Rect::from_min_size(pos2(center_x, top_y), vec2(center_w, disp_h));
    paint_top_display(ui.painter(), disp_rect, &params, live_peak, &live_wave);

    let sub_y = disp_rect.max.y + gap;
    let sub_h = (bot_y - sub_y).max(110.0);

    let gate_w = (center_w * 0.255).floor();
    let shape_w = (center_w * 0.305).floor();
    let quant_w = (center_w - gate_w - shape_w - gap * 2.0).max(80.0);

    // --- 2a. GATE (Blanco) ---
    let gate_rect = Rect::from_min_size(pos2(center_x, sub_y), vec2(gate_w, sub_h));
    paint_recessed_panel(ui.painter(), gate_rect);
    let gate_hdr = Rect::from_min_size(pos2(gate_rect.min.x + 5.0, gate_rect.min.y + 5.0), vec2(gate_w - 10.0, 16.0));
    if section_checkbox_header(
        ui,
        gate_hdr,
        ui.id().with(("bit8_gate_en", track_idx, slot_idx)),
        "Gate",
        COL_GATE_WHITE,
        &mut params.gate_enabled,
    ) {
        changed = true;
    }

    let gate_knob_y = gate_rect.max.y - 54.0;
    let mut gate_norm = Bit8Params::db_val_to_norm(params.gate_db);
    let gate_str = format_db(params.gate_db);
    let gate_arc = if params.gate_enabled {
        COL_GATE_WHITE
    } else {
        Color32::from_rgb(0x64, 0x68, 0x72)
    };
    if draw_knob_cell(
        ui,
        gate_rect.center().x,
        gate_knob_y,
        k_diam,
        &mut gate_norm,
        Bit8Params::db_val_to_norm(def.gate_db),
        gate_arc,
        &gate_str,
        &format!("Gate Threshold: {gate_str}"),
    ) {
        params.gate_db = Bit8Params::db_norm_to_val(gate_norm);
        changed = true;
    }

    // --- 2b. SHAPE (Azul) ---
    let shape_rect = Rect::from_min_size(pos2(gate_rect.max.x + gap, sub_y), vec2(shape_w, sub_h));
    paint_recessed_panel(ui.painter(), shape_rect);
    let shape_hdr = Rect::from_min_size(pos2(shape_rect.min.x + 5.0, shape_rect.min.y + 5.0), vec2(shape_w - 10.0, 16.0));
    if section_checkbox_header(
        ui,
        shape_hdr,
        ui.id().with(("bit8_shape_en", track_idx, slot_idx)),
        "Shape",
        COL_SHAPE_BLUE,
        &mut params.shape_enabled,
    ) {
        changed = true;
    }

    // 3 botones vectoriales de rectificación (Normal / Half-wave / Full-wave)
    let wave_row_y = shape_rect.min.y + 25.0;
    let wave_row_w = shape_w - 8.0;
    let wave_btn_w = (wave_row_w / 3.0).floor();
    for (idx, &mode) in Bit8Rectify::ALL.iter().enumerate() {
        let btn_r = Rect::from_min_size(
            pos2(shape_rect.min.x + 4.0 + idx as f32 * wave_btn_w, wave_row_y),
            vec2(wave_btn_w, 18.0),
        );
        let resp = ui
            .interact(btn_r, ui.id().with(("bit8_rect", track_idx, slot_idx, idx)), Sense::click())
            .on_hover_text(mode.label());
        if resp.clicked() {
            params.rectify = mode;
            changed = true;
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let selected = params.rectify == mode;
        let icon_col = if selected && params.shape_enabled {
            COL_SHAPE_BLUE
        } else if selected {
            Color32::from_rgb(0x88, 0x90, 0x9E)
        } else if resp.hovered() {
            Color32::from_rgb(0xD4, 0xD8, 0xE0)
        } else {
            Color32::from_rgb(0x90, 0x94, 0x9D)
        };
        paint_rectify_icon(ui.painter(), btn_r.shrink2(vec2(3.0, 3.0)), mode, icon_col);
        if selected {
            let line_col = if params.shape_enabled {
                COL_SHAPE_BLUE
            } else {
                Color32::from_rgb(0x64, 0x6A, 0x76)
            };
            ui.painter().line_segment(
                [pos2(btn_r.min.x + 1.0, btn_r.max.y + 1.0), pos2(btn_r.max.x - 1.0, btn_r.max.y + 1.0)],
                Stroke::new(2.0_f32, line_col),
            );
        }
    }

    // Selector desplegable de modo Shape (`Off`, `Clip`, `Soft`, `Fold`, `Wrap`, `Over`)
    let shape_dd_rect = Rect::from_min_size(
        pos2(shape_rect.min.x + 5.0, wave_row_y + 24.0),
        vec2(shape_w - 10.0, 21.0),
    );
    let shape_dd_id = ui.id().with(("bit8_shape_dd", track_idx, slot_idx));
    let shape_dd_resp = ui
        .interact(shape_dd_rect, shape_dd_id, Sense::click())
        .on_hover_text("Waveshaper Mode");
    if shape_dd_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    paint_bevel_box(ui.painter(), shape_dd_rect, false, shape_dd_resp.hovered(), COL_SHAPE_BLUE);
    ui.painter().text(
        pos2(shape_dd_rect.min.x + 6.0, shape_dd_rect.center().y),
        Align2::LEFT_CENTER,
        params.shape_mode.label(),
        fonts::semibold(10.0),
        if params.shape_enabled {
            theme::TEXT_PRIMARY
        } else {
            theme::TEXT_MUTED
        },
    );
    paint_dropdown_arrow(
        ui.painter(),
        pos2(shape_dd_rect.max.x - 8.0, shape_dd_rect.center().y),
        Color32::from_rgb(0xB4, 0xB8, 0xC2),
    );
    let shape_popup_id = ui.id().with(("bit8_shape_popup", track_idx, slot_idx));
    if shape_dd_resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(shape_popup_id));
    }
    egui::popup_below_widget(
        ui,
        shape_popup_id,
        &shape_dd_resp,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(82.0);
            for &m in &Bit8ShapeMode::ALL {
                let sel = params.shape_mode == m;
                if ui
                    .selectable_label(
                        sel,
                        egui::RichText::new(m.label())
                            .font(fonts::semibold(10.0))
                            .color(if sel { COL_SHAPE_BLUE } else { theme::TEXT_PRIMARY }),
                    )
                    .clicked()
                {
                    params.shape_mode = m;
                    changed = true;
                    ui.close_menu();
                }
            }
        },
    );

    // Cuando Shape != Off, aparece el knob azul de Drive debajo del selector (como en Bitwig)
    if params.shape_mode != Bit8ShapeMode::Off {
        let mut drive_norm = (params.shape_drive_db / 36.0).clamp(0.0, 1.0);
        let drive_str = format!("+{:.1} dB", params.shape_drive_db);
        let drive_col = if params.shape_enabled {
            COL_SHAPE_BLUE
        } else {
            Color32::from_rgb(0x5E, 0x66, 0x74)
        };
        if draw_knob_cell(
            ui,
            shape_rect.center().x,
            shape_rect.max.y - 52.0,
            30.0,
            &mut drive_norm,
            def.shape_drive_db / 36.0,
            drive_col,
            &drive_str,
            &format!("Shape Drive: {drive_str}"),
        ) {
            params.shape_drive_db = drive_norm * 36.0;
            changed = true;
        }
    }

    // --- 2c. QUANTIZE (Púrpura) ---
    let quant_rect = Rect::from_min_size(pos2(shape_rect.max.x + gap, sub_y), vec2(quant_w, sub_h));
    paint_recessed_panel(ui.painter(), quant_rect);
    let quant_hdr = Rect::from_min_size(pos2(quant_rect.min.x + 5.0, quant_rect.min.y + 5.0), vec2(quant_w - 10.0, 16.0));
    if section_checkbox_header(
        ui,
        quant_hdr,
        ui.id().with(("bit8_quant_en", track_idx, slot_idx)),
        "Quantize",
        COL_QUANT_PURPLE,
        &mut params.quantize_enabled,
    ) {
        changed = true;
    }

    // Dropdown de modo Quantize con icono de pasos (`Off-zero`, `At-zero`, `Truncate`, `Ceil`)
    let q_dd_rect = Rect::from_min_size(
        pos2(quant_rect.min.x + 5.0, quant_rect.min.y + 24.0),
        vec2(quant_w - 10.0, 22.0),
    );
    let q_dd_resp = ui
        .interact(q_dd_rect, ui.id().with(("bit8_q_dd", track_idx, slot_idx)), Sense::click())
        .on_hover_text("Quantize Rounding Mode");
    if q_dd_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    paint_bevel_box(ui.painter(), q_dd_rect, false, q_dd_resp.hovered(), COL_QUANT_PURPLE);
    let icon_rect = Rect::from_min_size(pos2(q_dd_rect.min.x + 5.0, q_dd_rect.center().y - 6.0), vec2(12.0, 12.0));
    paint_quantize_mode_icon(
        ui.painter(),
        icon_rect,
        params.quantize_mode,
        if params.quantize_enabled {
            Color32::from_rgb(0xE0, 0xE2, 0xE8)
        } else {
            theme::TEXT_MUTED
        },
    );
    ui.painter().text(
        pos2(icon_rect.max.x + 5.0, q_dd_rect.center().y),
        Align2::LEFT_CENTER,
        params.quantize_mode.label(),
        fonts::semibold(10.0),
        if params.quantize_enabled {
            theme::TEXT_PRIMARY
        } else {
            theme::TEXT_MUTED
        },
    );
    paint_dropdown_arrow(
        ui.painter(),
        pos2(q_dd_rect.max.x - 8.0, q_dd_rect.center().y),
        Color32::from_rgb(0xB4, 0xB8, 0xC2),
    );
    let q_popup_id = ui.id().with(("bit8_q_popup", track_idx, slot_idx));
    if q_dd_resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(q_popup_id));
    }
    egui::popup_below_widget(
        ui,
        q_popup_id,
        &q_dd_resp,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(94.0);
            for &m in &Bit8QuantizeMode::ALL {
                let sel = params.quantize_mode == m;
                if ui
                    .selectable_label(
                        sel,
                        egui::RichText::new(m.label())
                            .font(fonts::semibold(10.0))
                            .color(if sel { COL_QUANT_PURPLE } else { theme::TEXT_PRIMARY }),
                    )
                    .clicked()
                {
                    params.quantize_mode = m;
                    changed = true;
                    ui.close_menu();
                }
            }
        },
    );

    // Knob púrpura Quantize (izquierda) + botón Squared Domain (derecha)
    let q_knob_cx = quant_rect.min.x + 28.0;
    let q_knob_y = quant_rect.max.y - 54.0;
    let mut quant_norm = Bit8Params::db_val_to_norm(params.quantize_db);
    let quant_str = format_db(params.quantize_db);
    let quant_arc = if params.quantize_enabled {
        COL_QUANT_PURPLE
    } else {
        Color32::from_rgb(0x64, 0x60, 0x74)
    };
    if draw_knob_cell(
        ui,
        q_knob_cx,
        q_knob_y,
        k_diam,
        &mut quant_norm,
        Bit8Params::db_val_to_norm(def.quantize_db),
        quant_arc,
        &quant_str,
        &format!("Quantize Step: {quant_str}"),
    ) {
        params.quantize_db = Bit8Params::db_norm_to_val(quant_norm);
        changed = true;
    }

    // Botón Squared Domain (icono curvo de lente + diagonal)
    let sq_rect = Rect::from_center_size(pos2(quant_rect.max.x - 20.0, q_knob_y + k_diam * 0.52), vec2(24.0, 22.0));
    let sq_resp = ui
        .interact(sq_rect, ui.id().with(("bit8_sq_dom", track_idx, slot_idx)), Sense::click())
        .on_hover_text("Squared Domain (non-linear quantization curve)");
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
        COL_QUANT_PURPLE,
    );
    paint_squared_domain_icon(
        ui.painter(),
        sq_rect,
        if params.squared_domain {
            Color32::from_rgb(0x16, 0x14, 0x1C)
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
        .interact(wet_tab_rect, ui.id().with(("bit8_wet_tab", track_idx, slot_idx)), Sense::click())
        .on_hover_text("Toggle Wet FX Post-Filter & Gain");
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
        "Wet FX",
        fonts::semibold(10.0),
        if wet_fx_open || wet_resp.hovered() {
            Color32::WHITE
        } else {
            Color32::from_rgb(0xE2, 0xE5, 0xEC)
        },
    );

    // Panel inferior derecho: Anti-alias, Width, Mix
    let out_rect = Rect::from_min_max(pos2(right_x, wet_tab_rect.max.y + gap), pos2(right_x + right_w, bot_y));
    paint_recessed_panel(ui.painter(), out_rect);

    let aa_rect = Rect::from_min_size(pos2(out_rect.min.x + 4.0, out_rect.min.y + 6.0), vec2(right_w - 8.0, 21.0));
    let aa_resp = ui
        .interact(aa_rect, ui.id().with(("bit8_aa", track_idx, slot_idx)), Sense::click())
        .on_hover_text("Anti-alias (oversampled shaping & smooth clock interpolation)");
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
        "Width",
        &format!("Stereo Width: {:.0}%", params.width * 100.0),
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
        "Mix",
        &format!("Dry/Wet Mix: {:.0}%", params.mix * 100.0),
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
            &hp_lbl,
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
            &lp_lbl,
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
            &gain_lbl,
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
