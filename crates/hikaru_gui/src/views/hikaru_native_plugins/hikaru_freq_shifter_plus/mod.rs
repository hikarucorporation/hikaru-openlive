// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_freq_shifter_plus/mod.rs

//! **Hikaru OpenFreqShifter+**: panel nativo de precisión para `hikaru_dsp::effects::FreqShifterPlusEffect`.
//!
//! Arquitectura visual de 4 columnas (paleta Obsidian + Mint-Emerald `#25D98E`):
//! - **Tira izquierda** (`#17191C`): selector de orden de polos de Hilbert (`6P`, `12P`, `18P`, `24P`),
//!   toggle vectorial de cruce estéreo en realimentación (`↱↲`) y selector vertical rotado de calidad
//!   de sobremuestreo (`Excellent` / `High` / `Eco`).
//! - **Columna central-izquierda**:
//!   - Pozo superior oscuro (`#17191C`): icono vectorial de inversión estéreo (`↔`), perilla heroica
//!     bipolar de aluminio cepillado (`#ADAFB4`) con entrada numérica directa (`Ctrl+Clic` / clic en
//!     el visor verde `0.00`) y selector de unidad/escala (`Hz`, `kHz`, `Sub`, `Beat`).
//!   - Subpanel inferior **`FB Cuts`** (`#24262A`): dos perillas de corte para el lazo de realimentación
//!     con arcos de banda pasante conservada (HP ilumina `norm..1.0` + curva vectorial `╭─`; LP ilumina
//!     `0.0..norm` + curva vectorial `─╮`).
//! - **Columna central-derecha** (`#24262A`): perilla **`Delay`** + cápsula esmeralda **`D`** (`ms` vs
//!   sincronía rítmica `1/64..1/1`) + lectura en vivo (`0.00 ms`), perilla bipolar **`Feedback`** y
//!   perilla bipolar **`FB Shift`** con icono vectorial de independencia de etapas (`||┌||`).
//! - **Columna derecha**: pestaña superior **`Wet FX`** (despliega la cadena anidada de efectos en vivo
//!   sobre la rama wet) + subpanel inferior con **`Width`** (`0..200%`) y **`Mix`** (`0..100%`).

use std::collections::{HashMap, HashSet};
use std::f32::consts::FRAC_PI_2;
use std::sync::{Mutex, OnceLock};

use egui::{pos2, vec2, Align2, Color32, CornerRadius, CursorIcon, Painter, Pos2, Rect, Sense, Stroke};
use hikaru_dsp::effects::{
    FreqShiftPoles, FreqShiftQuality, FreqShiftUnit, FreqShifterPlusParams, DELAY_SYNC_LABELS,
    FREQ_SHIFTER_MAX_FEEDBACK, FREQ_SHIFTER_PLUS_PARAM_COUNT,
};

use crate::app::AppState;
use crate::theme;
use crate::ui::bitwig_kit;
use crate::ui::fonts;

pub const FREQ_SHIFTER_PLUS_CARD_WIDTH: f32 = 344.0;

// Tokens de paleta Obsidian + Mint-Emerald
const COL_WELL_DEEP: Color32 = Color32::from_rgb(0x16, 0x18, 0x1B);
const COL_WELL_BORDER: Color32 = Color32::from_rgb(0x10, 0x11, 0x14);
const COL_PANEL_BG: Color32 = Color32::from_rgb(0x23, 0x25, 0x29);
const COL_PANEL_BORDER: Color32 = Color32::from_rgb(0x18, 0x1A, 0x1E);
const COL_CTRL_BG: Color32 = Color32::from_rgb(0x2B, 0x2E, 0x33);
const COL_CTRL_HOVER: Color32 = Color32::from_rgb(0x36, 0x3A, 0x41);
const COL_CTRL_BORDER: Color32 = Color32::from_rgb(0x18, 0x1A, 0x1E);

const COL_EMERALD: Color32 = Color32::from_rgb(0x25, 0xD9, 0x8E);
const COL_EMERALD_DIM: Color32 = Color32::from_rgb(0x1D, 0x8C, 0x5E);
const COL_HERO_CAP: Color32 = Color32::from_rgb(0xAD, 0xAF, 0xB5);
const COL_HERO_CAP_HOVER: Color32 = Color32::from_rgb(0xBA, 0xBD, 0xC4);
const COL_TRACK_RING: Color32 = Color32::from_rgb(0x36, 0x39, 0x40);
const COL_TEXT_BRIGHT: Color32 = Color32::from_rgb(0xDF, 0xE2, 0xE8);
const COL_TEXT_SUBTLE: Color32 = Color32::from_rgb(0x8D, 0x92, 0x9B);

pub fn is_freq_shifter_plus_name(name: &str) -> bool {
    matches!(
        name,
        "Hikaru OpenFreqShifter+"
            | "Hikaru OpenFreqShifter"
            | "OpenFreqShifter+"
            | "OpenFreqShifter"
            | "Freq Shifter+"
            | "Freq Shifter"
            | "Hikaru Freq Shifter+"
    )
}

struct FreqShifterSlotUiState {
    params: FreqShifterPlusParams,
    wet_fx_open: bool,
    shift_text_buf: String,
    editing_shift_text: bool,
}

impl Default for FreqShifterSlotUiState {
    fn default() -> Self {
        Self {
            params: FreqShifterPlusParams::default(),
            wet_fx_open: false,
            shift_text_buf: String::new(),
            editing_shift_text: false,
        }
    }
}

type StateMap = Mutex<HashMap<(usize, usize), FreqShifterSlotUiState>>;
type LoadedSet = Mutex<HashSet<(usize, usize)>>;

fn state_map() -> &'static StateMap {
    static CELL: OnceLock<StateMap> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(HashMap::new()))
}

fn loaded_set() -> &'static LoadedSet {
    static CELL: OnceLock<LoadedSet> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(HashSet::new()))
}

pub fn is_wet_fx_open(track_idx: usize, slot_idx: usize) -> bool {
    state_map()
        .lock()
        .unwrap()
        .get(&(track_idx, slot_idx))
        .map(|s| s.wet_fx_open)
        .unwrap_or(false)
}

pub fn card_width_for_slot(_track_idx: usize, _slot_idx: usize) -> f32 {
    FREQ_SHIFTER_PLUS_CARD_WIDTH
}

pub fn clear_slot_state(track_idx: usize, slot_idx: usize) {
    state_map().lock().unwrap().remove(&(track_idx, slot_idx));
    loaded_set().lock().unwrap().remove(&(track_idx, slot_idx));
    crate::views::hikaru_native_plugins::hikaru_tier_s_fx::clear_wet_fx_state(track_idx, slot_idx);
    crate::views::hikaru_native_plugins::hikaru_openphaser_plus::clear_wet_fx_state(
        track_idx, slot_idx,
    );
    crate::views::hikaru_native_plugins::hikaru_ultracomb::ultracomb::clear_wet_fx_state(
        track_idx, slot_idx,
    );
}

pub struct FreqShifterPreset {
    pub name: &'static str,
    pub params: FreqShifterPlusParams,
}

pub fn factory_presets() -> Vec<FreqShifterPreset> {
    let mk_hz = |hz: f32, unit: FreqShiftUnit| -> f32 {
        let mut p = FreqShifterPlusParams {
            shift_unit: unit,
            ..FreqShifterPlusParams::default()
        };
        p.set_shift_from_display_value(hz);
        p.shift_norm
    };
    let mk_fb_hz = |hz: f32| -> f32 {
        let ratio = (hz.clamp(-500.0, 500.0) / 500.0) as f64;
        ((ratio.signum() * ratio.abs().cbrt()) * 0.5 + 0.5) as f32
    };

    vec![
        FreqShifterPreset {
            name: "Default (OpenFreqShifter+)",
            params: FreqShifterPlusParams::default(),
        },
        FreqShifterPreset {
            name: "Stereo Barberpole Phaser",
            params: FreqShifterPlusParams {
                shift_unit: FreqShiftUnit::SubHz,
                shift_norm: mk_hz(0.38, FreqShiftUnit::SubHz),
                stereo_twist: true,
                pole_order: FreqShiftPoles::P24,
                fb_cross: true,
                feedback: 0.58,
                fb_hp_hz: 65.0,
                fb_lp_hz: 14_000.0,
                mix: 0.50,
                ..FreqShifterPlusParams::default()
            },
        },
        FreqShifterPreset {
            name: "Dub Spiral Echo",
            params: FreqShifterPlusParams {
                shift_unit: FreqShiftUnit::Hz,
                shift_norm: 0.5,
                delay_sync: true,
                delay_sync_div: 5, // 1/8
                fb_cross: true,
                feedback: 0.72,
                fb_hp_hz: 160.0,
                fb_lp_hz: 4_800.0,
                fb_shift_norm: mk_fb_hz(24.0),
                fb_shift_independent: true,
                width: 1.35,
                mix: 0.42,
                ..FreqShifterPlusParams::default()
            },
        },
        FreqShifterPreset {
            name: "Bode Metallic Clang",
            params: FreqShifterPlusParams {
                shift_unit: FreqShiftUnit::Khz,
                shift_norm: mk_hz(0.62, FreqShiftUnit::Khz),
                stereo_twist: false,
                pole_order: FreqShiftPoles::P12,
                quality: FreqShiftQuality::Excellent,
                delay_ms: 6.5,
                feedback: 0.48,
                fb_hp_hz: 110.0,
                fb_lp_hz: 9_500.0,
                mix: 0.75,
                ..FreqShifterPlusParams::default()
            },
        },
        FreqShifterPreset {
            name: "Lush Micro Stereo Widener",
            params: FreqShifterPlusParams {
                shift_unit: FreqShiftUnit::SubHz,
                shift_norm: mk_hz(0.12, FreqShiftUnit::SubHz),
                stereo_twist: true,
                delay_ms: 11.5,
                feedback: 0.22,
                fb_cross: true,
                width: 1.45,
                mix: 0.35,
                ..FreqShifterPlusParams::default()
            },
        },
        FreqShifterPreset {
            name: "Hollow Negative Comb",
            params: FreqShifterPlusParams {
                shift_unit: FreqShiftUnit::Hz,
                shift_norm: mk_hz(-4.5, FreqShiftUnit::Hz),
                stereo_twist: true,
                pole_order: FreqShiftPoles::P6,
                delay_ms: 3.4,
                feedback: -0.78,
                fb_hp_hz: 85.0,
                fb_lp_hz: 8_200.0,
                mix: 0.65,
                ..FreqShifterPlusParams::default()
            },
        },
        FreqShifterPreset {
            name: "Downward Sub Vortex",
            params: FreqShifterPlusParams {
                shift_unit: FreqShiftUnit::Hz,
                shift_norm: mk_hz(-48.0, FreqShiftUnit::Hz),
                delay_ms: 42.0,
                feedback: 0.68,
                fb_shift_norm: mk_fb_hz(-18.0),
                fb_hp_hz: 45.0,
                fb_lp_hz: 3_600.0,
                mix: 0.80,
                ..FreqShifterPlusParams::default()
            },
        },
        FreqShifterPreset {
            name: "Rhythmic Beat Shifter",
            params: FreqShifterPlusParams {
                shift_unit: FreqShiftUnit::Beat,
                shift_norm: mk_hz(0.50, FreqShiftUnit::Beat),
                stereo_twist: true,
                delay_sync: true,
                delay_sync_div: 3, // 1/16
                feedback: 0.54,
                fb_cross: true,
                mix: 0.55,
                ..FreqShifterPlusParams::default()
            },
        },
    ]
}

fn push_all_params(
    app: &mut AppState,
    engine_track_idx: usize,
    slot_idx: usize,
    params: &FreqShifterPlusParams,
) {
    for id in 0..(FREQ_SHIFTER_PLUS_PARAM_COUNT as u32) {
        if let Some(val) = params.get_normalized(id) {
            app.audio_proxy
                .send(crate::audio_proxy::GuiCommand::SetExternalPluginParam {
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
    ui.set_min_width(188.0);
    ui.label(
        egui::RichText::new("Hikaru OpenFreqShifter+ Presets")
            .font(fonts::semibold(10.0))
            .color(COL_EMERALD),
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
                st.editing_shift_text = false;
            }
            if let Some(et_idx) = track_idx.checked_sub(1) {
                push_all_params(app, et_idx, slot_idx, &preset.params);
            }
            ui.close_menu();
        }
    }
}

// =============================================================================
// Primitivas vectoriales y controles dedicados
// =============================================================================

fn paint_deep_well(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect, CornerRadius::same(3), COL_WELL_DEEP);
    painter.rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(1.0_f32, COL_WELL_BORDER),
        egui::StrokeKind::Inside,
    );
}

fn paint_sub_panel(painter: &Painter, rect: Rect) {
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
    painter.rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(1.0_f32, COL_CTRL_BORDER),
        egui::StrokeKind::Inside,
    );
}

fn paint_dropdown_arrow(painter: &Painter, center: Pos2, color: Color32) {
    painter.add(egui::Shape::convex_polygon(
        vec![
            pos2(center.x - 2.7, center.y - 1.4),
            pos2(center.x + 2.7, center.y - 1.4),
            pos2(center.x, center.y + 2.2),
        ],
        color,
        Stroke::NONE,
    ));
}

fn paint_rotated_vertical_text(
    painter: &Painter,
    center_x: f32,
    center_y: f32,
    text: &str,
    font_size: f32,
    color: Color32,
) {
    let galley = painter.layout_no_wrap(text.to_string(), fonts::semibold(font_size), color);
    let text_w = galley.size().x;
    let text_h = galley.size().y;
    let mut ts = egui::epaint::TextShape::new(
        pos2(center_x - text_h * 0.5, center_y + text_w * 0.5),
        galley,
        color,
    );
    ts.angle = -FRAC_PI_2;
    painter.add(egui::Shape::Text(ts));
}

/// Icono vectorial `↔` de inversión estéreo (Stereo Twist) sobre la perilla principal.
fn paint_stereo_twist_arrows(painter: &Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let half_w = (rect.width() * 0.38).clamp(4.5, 7.5);
    let head = 2.6_f32;
    let st = Stroke::new(1.35_f32, color);
    painter.line_segment([pos2(c.x - half_w, c.y), pos2(c.x + half_w, c.y)], st);
    painter.add(egui::Shape::convex_polygon(
        vec![
            pos2(c.x - half_w - 0.8, c.y),
            pos2(c.x - half_w + head, c.y - head),
            pos2(c.x - half_w + head, c.y + head),
        ],
        color,
        Stroke::NONE,
    ));
    painter.add(egui::Shape::convex_polygon(
        vec![
            pos2(c.x + half_w + 0.8, c.y),
            pos2(c.x + half_w - head, c.y - head),
            pos2(c.x + half_w - head, c.y + head),
        ],
        color,
        Stroke::NONE,
    ));
}

/// Icono vectorial `↱↲` de cruce estéreo en realimentación (dos flechas acodadas opuestas).
fn paint_fb_cross_icon(painter: &Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let st = Stroke::new(1.35_f32, color);
    // Flecha superior: sube por la izquierda y gira a la derecha
    let top_y = c.y - 2.6;
    painter.line_segment([pos2(c.x - 4.2, c.y + 0.5), pos2(c.x - 4.2, top_y)], st);
    painter.line_segment([pos2(c.x - 4.2, top_y), pos2(c.x + 2.4, top_y)], st);
    painter.line_segment([pos2(c.x + 0.2, top_y - 2.1), pos2(c.x + 2.7, top_y)], st);
    painter.line_segment([pos2(c.x + 0.2, top_y + 2.1), pos2(c.x + 2.7, top_y)], st);

    // Flecha inferior: baja por la derecha y gira a la izquierda
    let bot_y = c.y + 3.2;
    painter.line_segment([pos2(c.x + 4.2, c.y - 0.1), pos2(c.x + 4.2, bot_y)], st);
    painter.line_segment([pos2(c.x + 4.2, bot_y), pos2(c.x - 2.4, bot_y)], st);
    painter.line_segment([pos2(c.x - 0.2, bot_y - 2.1), pos2(c.x - 2.7, bot_y)], st);
    painter.line_segment([pos2(c.x - 0.2, bot_y + 2.1), pos2(c.x - 2.7, bot_y)], st);
}

/// Curva vectorial paso-alto (`╭─`) o paso-bajo (`─╮`) bajo las perillas de `FB Cuts`.
fn paint_filter_cut_icon(painter: &Painter, center: Pos2, is_highpass: bool, color: Color32) {
    let st = Stroke::new(1.25_f32, color);
    let mut pts = Vec::with_capacity(9);
    if is_highpass {
        pts.push(pos2(center.x - 5.5, center.y + 3.2));
        pts.push(pos2(center.x - 4.8, center.y + 0.4));
        pts.push(pos2(center.x - 3.3, center.y - 1.8));
        pts.push(pos2(center.x - 1.2, center.y - 2.6));
        pts.push(pos2(center.x + 5.5, center.y - 2.6));
    } else {
        pts.push(pos2(center.x - 5.5, center.y - 2.6));
        pts.push(pos2(center.x + 1.2, center.y - 2.6));
        pts.push(pos2(center.x + 3.3, center.y - 1.8));
        pts.push(pos2(center.x + 4.8, center.y + 0.4));
        pts.push(pos2(center.x + 5.5, center.y + 3.2));
    }
    painter.add(egui::Shape::line(pts, st));
}

/// Icono vectorial `||┌||` junto a `FB Shift` (espectro desplazado independiente en el lazo).
fn paint_fb_shift_step_icon(painter: &Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let faint = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 155);
    let st_faint = Stroke::new(1.0_f32, faint);
    let st_main = Stroke::new(1.2_f32, color);

    // Barras espectrales laterales
    painter.line_segment([pos2(c.x - 5.2, c.y - 2.8), pos2(c.x - 5.2, c.y + 3.0)], st_faint);
    painter.line_segment([pos2(c.x - 3.0, c.y - 2.8), pos2(c.x - 3.0, c.y + 3.0)], st_faint);
    painter.line_segment([pos2(c.x + 3.0, c.y - 3.0), pos2(c.x + 3.0, c.y + 2.8)], st_faint);
    painter.line_segment([pos2(c.x + 5.2, c.y - 3.0), pos2(c.x + 5.2, c.y + 2.8)], st_faint);

    // Escalón central en Z vertical `_|¯`
    painter.add(egui::Shape::line(
        vec![
            pos2(c.x - 4.4, c.y + 4.6),
            pos2(c.x, c.y + 4.6),
            pos2(c.x, c.y - 4.6),
            pos2(c.x + 4.4, c.y - 4.6),
        ],
        st_main,
    ));
}

fn paint_knob_arc_segment(
    painter: &Painter,
    center: Pos2,
    radius: f32,
    norm_start: f32,
    norm_end: f32,
    stroke: Stroke,
) {
    let a0 = (-225.0 + norm_start.clamp(0.0, 1.0) * 270.0).to_radians();
    let a1 = (-225.0 + norm_end.clamp(0.0, 1.0) * 270.0).to_radians();
    let span = (a1 - a0).abs();
    if span < 0.01 {
        return;
    }
    let steps = ((span * radius * 0.9).ceil() as usize).clamp(6, 42);
    let mut pts = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let ang = a0 + (a1 - a0) * t;
        pts.push(pos2(center.x + radius * ang.cos(), center.y + radius * ang.sin()));
    }
    painter.add(egui::Shape::line(pts, stroke));
}

/// Perilla heroica central de aluminio claro (`#ADAFB4`) con ranura oscura, punto central
/// esmeralda a las 12 en punto y arco bipolar desde `0.5`.
fn draw_hero_shift_knob(
    ui: &mut egui::Ui,
    center: Pos2,
    diameter: f32,
    norm: &mut f32,
    tooltip: &str,
) -> (egui::Response, bool) {
    let rect = Rect::from_center_size(center, vec2(diameter, diameter));
    let mut knob_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    let (_, resp) = knob_ui.allocate_exact_size(rect.size(), Sense::click_and_drag());
    let mut changed = false;

    if resp.double_clicked() {
        *norm = 0.5;
        changed = true;
    } else if resp.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let sens = if fine { 0.0008 } else { 0.0042 };
        let delta = -resp.drag_delta().y * sens;
        if delta.abs() > 0.0 {
            *norm = (*norm + delta).clamp(0.0, 1.0);
            changed = true;
        }
    }

    if resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::None);
    } else if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
    }

    let p = ui.painter();
    let ring_r = diameter * 0.47;
    let cap_r = diameter * 0.375;

    // Sombra suave bajo el capuchón
    p.circle_filled(
        pos2(center.x, center.y + 2.2),
        cap_r + 1.5,
        Color32::from_rgba_unmultiplied(0, 0, 0, 125),
    );

    // Pista circular 270° + arco activo bipolar desde 0.5
    paint_knob_arc_segment(
        p,
        center,
        ring_r,
        0.0,
        1.0,
        Stroke::new(2.1_f32, COL_TRACK_RING),
    );
    let n = norm.clamp(0.0, 1.0);
    if (n - 0.5).abs() > 0.004 {
        let (s, e) = if n >= 0.5 { (0.5, n) } else { (n, 0.5) };
        paint_knob_arc_segment(p, center, ring_r, s, e, Stroke::new(2.3_f32, COL_EMERALD));
    }

    // Punto indicador esmeralda en el centro superior (12 en punto)
    p.circle_filled(pos2(center.x, center.y - ring_r), 1.65, COL_EMERALD);

    // Capuchón metálico claro + bisel sutil
    let cap_col = if resp.hovered() || resp.dragged() {
        COL_HERO_CAP_HOVER
    } else {
        COL_HERO_CAP
    };
    p.circle_filled(center, cap_r, cap_col);
    p.circle_stroke(
        center,
        cap_r,
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 55)),
    );

    // Ranura oscura indicadora de ángulo
    let ang = (-225.0 + n * 270.0).to_radians();
    let dir = vec2(ang.cos(), ang.sin());
    p.line_segment(
        [center + dir * (cap_r * 0.18), center + dir * (cap_r - 1.2)],
        Stroke::new(2.6_f32, Color32::from_rgb(0x1B, 0x1D, 0x21)),
    );

    let resp = resp.on_hover_text(tooltip);
    (resp, changed)
}

/// Perilla de filtro para `FB Cuts` cuyo arco muestra la **banda pasante conservada**:
/// - `is_highpass == true`: arco verde desde `norm` hasta `1.0` (altas frecuencias conservadas).
/// - `is_highpass == false`: arco verde desde `0.0` hasta `norm` (bajas frecuencias conservadas).
fn draw_passband_cut_knob(
    ui: &mut egui::Ui,
    center: Pos2,
    diameter: f32,
    norm: &mut f32,
    default_norm: f32,
    is_highpass: bool,
    tooltip: &str,
) -> bool {
    let rect = Rect::from_center_size(center, vec2(diameter, diameter));
    let mut knob_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    let (_, resp) = knob_ui.allocate_exact_size(rect.size(), Sense::click_and_drag());
    let mut changed = false;

    if resp.double_clicked() {
        *norm = default_norm.clamp(0.0, 1.0);
        changed = true;
    } else if resp.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let sens = if fine { 0.0012 } else { 0.0055 };
        let delta = -resp.drag_delta().y * sens;
        if delta.abs() > 0.0 {
            *norm = (*norm + delta).clamp(0.0, 1.0);
            changed = true;
        }
    }

    if resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::None);
    } else if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
    }

    let p = ui.painter();
    let ring_r = diameter * 0.46;
    let cap_r = diameter * 0.35;
    let n = norm.clamp(0.0, 1.0);

    paint_knob_arc_segment(
        p,
        center,
        ring_r,
        0.0,
        1.0,
        Stroke::new(2.0_f32, COL_TRACK_RING),
    );
    let (arc_s, arc_e) = if is_highpass { (n, 1.0) } else { (0.0, n) };
    if arc_e - arc_s > 0.005 {
        paint_knob_arc_segment(p, center, ring_r, arc_s, arc_e, Stroke::new(2.2_f32, COL_EMERALD));
    }

    p.circle_filled(
        pos2(center.x, center.y + 1.5),
        cap_r + 0.8,
        Color32::from_rgba_unmultiplied(0, 0, 0, 110),
    );
    let cap_col = if resp.hovered() || resp.dragged() {
        Color32::from_rgb(0x68, 0x6C, 0x74)
    } else {
        bitwig_kit::BW_KNOB_CAP
    };
    p.circle_filled(center, cap_r, cap_col);
    p.circle_stroke(
        center,
        cap_r,
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 28)),
    );

    let ang = (-225.0 + n * 270.0).to_radians();
    let dir = vec2(ang.cos(), ang.sin());
    p.line_segment(
        [center + dir * (cap_r * 0.28), center + dir * (cap_r - 1.0)],
        Stroke::new(1.9_f32, Color32::WHITE),
    );

    resp.on_hover_text(tooltip);
    changed
}

#[allow(clippy::too_many_arguments)]
fn draw_std_knob(
    ui: &mut egui::Ui,
    center_x: f32,
    top_y: f32,
    diameter: f32,
    norm: &mut f32,
    default_norm: f32,
    bipolar: bool,
    bottom_label: Option<&str>,
    tooltip: &str,
) -> bool {
    let knob_rect = Rect::from_min_size(
        pos2(center_x - diameter * 0.5, top_y),
        vec2(diameter, diameter),
    );
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
        COL_EMERALD,
        bipolar,
    );
    if changed {
        *norm = (pct / 100.0).clamp(0.0, 1.0);
    }
    // Punto indicador superior en 12 en punto para controles centrados (Width / Feedback / FB Shift)
    if bipolar {
        ui.painter().circle_filled(
            pos2(center_x, knob_rect.min.y + 1.2),
            1.35,
            if (norm.clamp(0.0, 1.0) - 0.5).abs() < 0.02 {
                COL_EMERALD
            } else {
                COL_EMERALD_DIM
            },
        );
    }
    resp.on_hover_text(tooltip);

    if let Some(lbl) = bottom_label {
        ui.painter().text(
            pos2(center_x, knob_rect.max.y + 8.0),
            Align2::CENTER_CENTER,
            lbl,
            fonts::regular(9.5),
            COL_TEXT_BRIGHT,
        );
    }
    changed
}

fn format_cut_hz(hz: f32) -> String {
    if hz >= 1000.0 {
        format!("{:.2} kHz", hz / 1000.0)
    } else {
        format!("{hz:.0} Hz")
    }
}

// =============================================================================
// Renderizado principal del módulo en el DSP Rack
// =============================================================================

pub fn render_module(ui: &mut egui::Ui, app: &mut AppState, track_idx: usize, slot_idx: usize) {
    let key = (track_idx, slot_idx);
    let engine_track_idx = track_idx.checked_sub(1);
    let tempo_bpm = app.transport.bpm as f64;

    let initial_params = {
        let mut map = state_map().lock().unwrap();
        map.entry(key).or_default().params
    };
    if let Some(et_idx) = engine_track_idx {
        if loaded_set().lock().unwrap().insert(key) {
            app.audio_proxy
                .send(crate::audio_proxy::GuiCommand::LoadNativeEffect {
                    track_idx: et_idx,
                    slot_idx,
                    name: "Hikaru OpenFreqShifter+".to_string(),
                });
            push_all_params(app, et_idx, slot_idx, &initial_params);
        }
    }

    let (mut params, mut wet_fx_open, mut shift_text_buf, mut editing_shift_text) = {
        let mut map = state_map().lock().unwrap();
        let st = map.entry(key).or_default();
        (
            st.params,
            st.wet_fx_open,
            st.shift_text_buf.clone(),
            st.editing_shift_text,
        )
    };

    let full_rect = ui.available_rect_before_wrap();
    let (_, _) = ui.allocate_exact_size(full_rect.size(), Sense::hover());

    let mut changed = false;
    let mut wet_toggle_changed = false;
    let def = FreqShifterPlusParams::default();

    let pad = 3.0_f32;
    let gap = 4.0_f32;
    let top_y = full_rect.min.y + pad;
    let bot_y = full_rect.max.y - pad;
    let total_h = (bot_y - top_y).max(170.0);

    let strip_w = 29.0_f32;
    let fb_col_w = 78.0_f32;
    let right_col_w = 66.0_f32;
    let main_col_w =
        (full_rect.width() - pad * 2.0 - strip_w - fb_col_w - right_col_w - gap * 3.0).max(116.0);

    // =========================================================================
    // 1. TIRA IZQUIERDA: 6P / 12P / 18P / 24P + FB CROSS + EXCELLENT
    // =========================================================================
    let strip_rect = Rect::from_min_size(pos2(full_rect.min.x + pad, top_y), vec2(strip_w, total_h));
    paint_deep_well(ui.painter(), strip_rect);

    let pole_h = 24.5_f32;
    for (idx, &pole) in FreqShiftPoles::ALL.iter().enumerate() {
        let pr = Rect::from_min_size(
            pos2(strip_rect.min.x + 1.0, strip_rect.min.y + 4.0 + idx as f32 * pole_h),
            vec2(strip_w - 2.0, pole_h - 1.0),
        );
        let resp = ui
            .interact(
                pr,
                ui.id().with(("fsp_pole", track_idx, slot_idx, idx)),
                Sense::click(),
            )
            .on_hover_text(pole.description());
        if resp.clicked() {
            params.pole_order = pole;
            changed = true;
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let active = params.pole_order == pole;
        let col = if active {
            COL_EMERALD
        } else if resp.hovered() {
            COL_TEXT_BRIGHT
        } else {
            COL_TEXT_SUBTLE
        };
        let num_str = pole.base_label();
        let num_x = if num_str.len() == 1 {
            pr.center().x - 2.2
        } else {
            pr.center().x - 3.6
        };
        ui.painter().text(
            pos2(num_x, pr.center().y + 1.2),
            Align2::CENTER_CENTER,
            num_str,
            fonts::semibold(9.5),
            col,
        );
        ui.painter().text(
            pos2(pr.max.x - 5.2, pr.center().y - 3.2),
            Align2::CENTER_CENTER,
            "P",
            fonts::semibold(6.8),
            col,
        );
    }

    // Botón de cruce estéreo en realimentación (`↱↲`)
    let cross_y = strip_rect.min.y + 4.0 + 4.0 * pole_h + 6.0;
    let cross_rect = Rect::from_center_size(pos2(strip_rect.center().x, cross_y + 10.0), vec2(22.0, 22.0));
    let cross_resp = ui
        .interact(
            cross_rect,
            ui.id().with(("fsp_fb_cross", track_idx, slot_idx)),
            Sense::click(),
        )
        .on_hover_text(if params.fb_cross {
            "Feedback Stereo Crossover: ON (L↔R ping-pong feedback)"
        } else {
            "Feedback Stereo Crossover: OFF (straight L→L, R→R feedback)"
        });
    if cross_resp.clicked() {
        params.fb_cross = !params.fb_cross;
        changed = true;
    }
    if cross_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let cross_col = if params.fb_cross {
        COL_EMERALD
    } else if cross_resp.hovered() {
        COL_TEXT_BRIGHT
    } else {
        COL_TEXT_SUBTLE
    };
    paint_fb_cross_icon(ui.painter(), cross_rect, cross_col);

    // Selector vertical rotado de calidad (`Excellent` / `High` / `Eco`)
    let qual_rect = Rect::from_min_max(
        pos2(strip_rect.min.x + 1.0, cross_rect.max.y + 6.0),
        pos2(strip_rect.max.x - 1.0, strip_rect.max.y - 3.0),
    );
    let qual_resp = ui
        .interact(
            qual_rect,
            ui.id().with(("fsp_quality_btn", track_idx, slot_idx)),
            Sense::click(),
        )
        .on_hover_text(format!(
            "Oversampling Quality: {} — {}",
            params.quality.label(),
            params.quality.description()
        ));
    if qual_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        ui.painter().rect_filled(
            qual_rect.shrink(1.5),
            CornerRadius::same(2),
            Color32::from_rgba_unmultiplied(255, 255, 255, 12),
        );
    }
    paint_rotated_vertical_text(
        ui.painter(),
        qual_rect.center().x,
        qual_rect.center().y,
        params.quality.label(),
        9.6,
        if qual_resp.hovered() {
            Color32::WHITE
        } else {
            COL_TEXT_BRIGHT
        },
    );
    let qual_popup_id = ui.id().with(("fsp_quality_popup", track_idx, slot_idx));
    if qual_resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(qual_popup_id));
    }
    egui::popup_below_widget(
        ui,
        qual_popup_id,
        &qual_resp,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(110.0);
            for &q in &FreqShiftQuality::ALL {
                let sel = params.quality == q;
                if ui
                    .selectable_label(
                        sel,
                        egui::RichText::new(q.label())
                            .font(fonts::semibold(10.0))
                            .color(if sel { COL_EMERALD } else { theme::TEXT_PRIMARY }),
                    )
                    .on_hover_text(q.description())
                    .clicked()
                {
                    params.quality = q;
                    changed = true;
                    ui.close_menu();
                }
            }
        },
    );

    // =========================================================================
    // 2. COLUMNA CENTRAL-IZQUIERDA: MAIN SHIFT + FB CUTS
    // =========================================================================
    let main_x = strip_rect.max.x + gap;
    let fb_cuts_h = (total_h * 0.34).clamp(66.0, 76.0);
    let main_well_h = (total_h - fb_cuts_h - gap).max(98.0);

    let main_well_rect = Rect::from_min_size(pos2(main_x, top_y), vec2(main_col_w, main_well_h));
    paint_deep_well(ui.painter(), main_well_rect);

    // 2a. Icono superior `↔` (Stereo Twist)
    let twist_rect = Rect::from_center_size(
        pos2(main_well_rect.center().x, main_well_rect.min.y + 13.0),
        vec2(26.0, 16.0),
    );
    let twist_resp = ui
        .interact(
            twist_rect,
            ui.id().with(("fsp_twist", track_idx, slot_idx)),
            Sense::click(),
        )
        .on_hover_text(if params.stereo_twist {
            "Stereo Twist: ON (Left shifts +Δf, Right shifts -Δf)"
        } else {
            "Stereo Twist: OFF (Left & Right shift in the same direction)"
        });
    if twist_resp.clicked() {
        params.stereo_twist = !params.stereo_twist;
        changed = true;
    }
    if twist_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    paint_stereo_twist_arrows(
        ui.painter(),
        twist_rect,
        if params.stereo_twist {
            COL_EMERALD
        } else if twist_resp.hovered() {
            COL_TEXT_BRIGHT
        } else {
            Color32::from_rgb(0xA4, 0xA8, 0xB2)
        },
    );

    // 2b. Perilla heroica central `Shift`
    let hero_diam = 56.0_f32.min(main_well_h - 50.0).max(42.0);
    let hero_center = pos2(
        main_well_rect.center().x,
        main_well_rect.min.y + 23.0 + hero_diam * 0.5,
    );
    let actual_hz = params.shift_hz(tempo_bpm);
    let shift_tooltip = format!(
        "Frequency Shift Amount: {:+.2} Hz (Drag to adjust, Shift for fine, Double-click reset, Ctrl+Click to type)",
        actual_hz
    );
    let (hero_resp, hero_changed) = draw_hero_shift_knob(
        ui,
        hero_center,
        hero_diam,
        &mut params.shift_norm,
        &shift_tooltip,
    );
    if hero_changed {
        changed = true;
        editing_shift_text = false;
    }
    if hero_resp.clicked() && ui.input(|i| i.modifiers.ctrl || i.modifiers.command) {
        shift_text_buf = params.format_shift_readout(tempo_bpm);
        editing_shift_text = true;
    }

    // 2c. Fila inferior del pozo principal: Lectura verde (`0.00`) + Selector `[ Hz ▾ ]`
    let unit_row_y = main_well_rect.max.y - 24.0;
    let unit_dd_w = 42.0_f32;
    let unit_dd_rect = Rect::from_min_size(
        pos2(main_well_rect.max.x - unit_dd_w - 8.0, unit_row_y),
        vec2(unit_dd_w, 19.0),
    );
    let readout_rect = Rect::from_min_max(
        pos2(main_well_rect.min.x + 8.0, unit_row_y),
        pos2(unit_dd_rect.min.x - 4.0, unit_row_y + 19.0),
    );

    if editing_shift_text {
        let mut edit_ui = ui.new_child(egui::UiBuilder::new().max_rect(readout_rect));
        let te = egui::TextEdit::singleline(&mut shift_text_buf)
            .font(fonts::semibold(10.5))
            .text_color(COL_EMERALD)
            .desired_width(readout_rect.width());
        let te_resp = edit_ui.add(te);
        te_resp.request_focus();
        let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
        let esc_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if enter_pressed || te_resp.lost_focus() {
            if !esc_pressed {
                if let Ok(parsed) = shift_text_buf.trim().replace(',', ".").parse::<f32>() {
                    params.set_shift_from_display_value(parsed);
                    changed = true;
                }
            }
            editing_shift_text = false;
        }
    } else {
        let readout_str = params.format_shift_readout(tempo_bpm);
        let read_resp = ui
            .interact(
                readout_rect,
                ui.id().with(("fsp_readout", track_idx, slot_idx)),
                Sense::click(),
            )
            .on_hover_text("Click to type exact Frequency Shift value");
        if read_resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Text);
        }
        if read_resp.clicked() {
            shift_text_buf = readout_str.clone();
            editing_shift_text = true;
        }
        ui.painter().text(
            pos2(readout_rect.max.x - 4.0, readout_rect.center().y),
            Align2::RIGHT_CENTER,
            &readout_str,
            fonts::semibold(11.0),
            COL_EMERALD,
        );
    }

    let unit_resp = ui
        .interact(
            unit_dd_rect,
            ui.id().with(("fsp_unit_dd", track_idx, slot_idx)),
            Sense::click(),
        )
        .on_hover_text(format!(
            "Shift Range/Unit: {} — {}",
            params.shift_unit.label(),
            params.shift_unit.description()
        ));
    if unit_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    paint_bevel_box(ui.painter(), unit_dd_rect, false, unit_resp.hovered(), COL_EMERALD);
    ui.painter().text(
        pos2(unit_dd_rect.min.x + 7.0, unit_dd_rect.center().y),
        Align2::LEFT_CENTER,
        params.shift_unit.label(),
        fonts::semibold(10.0),
        COL_EMERALD,
    );
    paint_dropdown_arrow(
        ui.painter(),
        pos2(unit_dd_rect.max.x - 7.5, unit_dd_rect.center().y + 0.5),
        COL_EMERALD_DIM,
    );
    let unit_popup_id = ui.id().with(("fsp_unit_popup", track_idx, slot_idx));
    if unit_resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(unit_popup_id));
    }
    egui::popup_below_widget(
        ui,
        unit_popup_id,
        &unit_resp,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(88.0);
            for &u in &FreqShiftUnit::ALL {
                let sel = params.shift_unit == u;
                if ui
                    .selectable_label(
                        sel,
                        egui::RichText::new(u.label())
                            .font(fonts::semibold(10.0))
                            .color(if sel { COL_EMERALD } else { theme::TEXT_PRIMARY }),
                    )
                    .on_hover_text(u.description())
                    .clicked()
                {
                    params.shift_unit = u;
                    changed = true;
                    ui.close_menu();
                }
            }
        },
    );

    // 2d. Subpanel inferior `FB Cuts` (HP y LP con arcos de banda pasante)
    let fb_cuts_rect = Rect::from_min_size(
        pos2(main_x, main_well_rect.max.y + gap),
        vec2(main_col_w, fb_cuts_h),
    );
    paint_sub_panel(ui.painter(), fb_cuts_rect);
    ui.painter().text(
        pos2(fb_cuts_rect.center().x, fb_cuts_rect.min.y + 9.5),
        Align2::CENTER_CENTER,
        "FB Cuts",
        fonts::semibold(9.6),
        COL_TEXT_SUBTLE,
    );

    let cut_diam = 31.0_f32;
    let cut_cy = fb_cuts_rect.min.y + 18.0 + cut_diam * 0.5;
    let hp_cx = fb_cuts_rect.min.x + main_col_w * 0.28;
    let lp_cx = fb_cuts_rect.min.x + main_col_w * 0.72;

    let mut hp_norm = FreqShifterPlusParams::cut_hz_to_norm(params.fb_hp_hz);
    let hp_tooltip = format!("Feedback Low Cut (HP): {}", format_cut_hz(params.fb_hp_hz));
    if draw_passband_cut_knob(
        ui,
        pos2(hp_cx, cut_cy),
        cut_diam,
        &mut hp_norm,
        FreqShifterPlusParams::cut_hz_to_norm(def.fb_hp_hz),
        true,
        &hp_tooltip,
    ) {
        params.fb_hp_hz = FreqShifterPlusParams::cut_norm_to_hz(hp_norm);
        changed = true;
    }
    paint_filter_cut_icon(
        ui.painter(),
        pos2(hp_cx, fb_cuts_rect.max.y - 7.5),
        true,
        COL_TEXT_SUBTLE,
    );

    let mut lp_norm = FreqShifterPlusParams::cut_hz_to_norm(params.fb_lp_hz);
    let lp_tooltip = format!("Feedback High Cut (LP): {}", format_cut_hz(params.fb_lp_hz));
    if draw_passband_cut_knob(
        ui,
        pos2(lp_cx, cut_cy),
        cut_diam,
        &mut lp_norm,
        FreqShifterPlusParams::cut_hz_to_norm(def.fb_lp_hz),
        false,
        &lp_tooltip,
    ) {
        params.fb_lp_hz = FreqShifterPlusParams::cut_norm_to_hz(lp_norm);
        changed = true;
    }
    paint_filter_cut_icon(
        ui.painter(),
        pos2(lp_cx, fb_cuts_rect.max.y - 7.5),
        false,
        COL_TEXT_SUBTLE,
    );

    // =========================================================================
    // 3. COLUMNA CENTRAL-DERECHA: DELAY + FEEDBACK + FB SHIFT
    // =========================================================================
    let fb_col_x = main_well_rect.max.x + gap;
    let fb_col_rect = Rect::from_min_size(pos2(fb_col_x, top_y), vec2(fb_col_w, total_h));
    paint_sub_panel(ui.painter(), fb_col_rect);

    let k_diam = 34.0_f32;
    let row_step = (total_h - 10.0) / 3.0;

    // 3a. Perilla Delay + badge esmeralda `D` (`ms` vs tempo-sync) + lectura `0.00 ms`
    let dly_top = fb_col_rect.min.y + 7.0;
    if params.delay_sync {
        let max_idx = (DELAY_SYNC_LABELS.len() - 1) as f32;
        let mut div_norm = (params.delay_sync_div as f32 / max_idx).clamp(0.0, 1.0);
        let dly_tip = format!(
            "Feedback Delay (Synced): {} ({:.1} ms @ {:.0} BPM)",
            params.format_delay_readout(),
            params.delay_seconds(tempo_bpm) * 1000.0,
            tempo_bpm
        );
        if draw_std_knob(
            ui,
            fb_col_rect.center().x,
            dly_top,
            k_diam,
            &mut div_norm,
            def.delay_sync_div as f32 / max_idx,
            false,
            None,
            &dly_tip,
        ) {
            params.delay_sync_div = (div_norm * max_idx).round() as u8;
            changed = true;
        }
    } else {
        let mut dly_norm = FreqShifterPlusParams::delay_ms_to_norm(params.delay_ms);
        let dly_tip = format!("Feedback Delay: {}", params.format_delay_readout());
        if draw_std_knob(
            ui,
            fb_col_rect.center().x,
            dly_top,
            k_diam,
            &mut dly_norm,
            FreqShifterPlusParams::delay_ms_to_norm(def.delay_ms),
            false,
            None,
            &dly_tip,
        ) {
            params.delay_ms = FreqShifterPlusParams::delay_norm_to_ms(dly_norm);
            changed = true;
        }
    }

    let badge_y = dly_top + k_diam + 8.5;
    let d_badge_rect = Rect::from_center_size(pos2(fb_col_rect.min.x + 12.5, badge_y), vec2(14.0, 13.0));
    let d_badge_resp = ui
        .interact(
            d_badge_rect,
            ui.id().with(("fsp_dly_mode", track_idx, slot_idx)),
            Sense::click(),
        )
        .on_hover_text(if params.delay_sync {
            "Delay Mode: Beat Sync — click to switch to Time (ms)"
        } else {
            "Delay Mode: Time (ms) — click to switch to Beat Sync"
        });
    if d_badge_resp.clicked() {
        params.delay_sync = !params.delay_sync;
        changed = true;
    }
    if d_badge_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let time_mode_active = !params.delay_sync;
    ui.painter().rect_filled(
        d_badge_rect,
        CornerRadius::same(2),
        if time_mode_active {
            COL_EMERALD
        } else {
            Color32::from_rgb(0x38, 0x3C, 0x44)
        },
    );
    ui.painter().text(
        d_badge_rect.center(),
        Align2::CENTER_CENTER,
        if time_mode_active { "D" } else { "S" },
        fonts::semibold(8.6),
        if time_mode_active {
            Color32::from_rgb(0x10, 0x16, 0x13)
        } else {
            COL_EMERALD
        },
    );
    ui.painter().text(
        pos2(d_badge_rect.max.x + 4.0, badge_y),
        Align2::LEFT_CENTER,
        &params.format_delay_readout(),
        fonts::regular(9.3),
        COL_TEXT_BRIGHT,
    );

    // 3b. Perilla bipolar `Feedback`
    let fb_top = fb_col_rect.min.y + 7.0 + row_step;
    let mut fb_norm = ((params.feedback / FREQ_SHIFTER_MAX_FEEDBACK) * 0.5 + 0.5).clamp(0.0, 1.0);
    let fb_tip = format!("Feedback: {:+.1}%", params.feedback * 100.0);
    if draw_std_knob(
        ui,
        fb_col_rect.center().x,
        fb_top,
        k_diam,
        &mut fb_norm,
        0.5,
        true,
        Some("Feedback"),
        &fb_tip,
    ) {
        params.feedback = (fb_norm * 2.0 - 1.0) * FREQ_SHIFTER_MAX_FEEDBACK;
        changed = true;
    }

    // 3c. Perilla bipolar `FB Shift` + icono vectorial `||┌||`
    let fbs_top = fb_col_rect.min.y + 7.0 + row_step * 2.0;
    let fbs_tip = format!(
        "Feedback Frequency Shift: {:+.2} Hz",
        params.fb_shift_hz()
    );
    if draw_std_knob(
        ui,
        fb_col_rect.center().x + 2.0,
        fbs_top,
        k_diam,
        &mut params.fb_shift_norm,
        0.5,
        true,
        None,
        &fbs_tip,
    ) {
        changed = true;
    }

    let fbs_lbl_y = fbs_top + k_diam + 8.5;
    let indep_rect = Rect::from_center_size(pos2(fb_col_rect.min.x + 12.5, fbs_lbl_y), vec2(15.0, 14.0));
    let indep_resp = ui
        .interact(
            indep_rect,
            ui.id().with(("fsp_fb_indep", track_idx, slot_idx)),
            Sense::click(),
        )
        .on_hover_text(if params.fb_shift_independent {
            "Independent FB Shift: ON (echoes shift purely by FB Shift)"
        } else {
            "Independent FB Shift: OFF (echoes compound Main Shift + FB Shift)"
        });
    if indep_resp.clicked() {
        params.fb_shift_independent = !params.fb_shift_independent;
        changed = true;
    }
    if indep_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    paint_fb_shift_step_icon(
        ui.painter(),
        indep_rect,
        if params.fb_shift_independent {
            COL_EMERALD
        } else if indep_resp.hovered() {
            COL_TEXT_BRIGHT
        } else {
            COL_TEXT_SUBTLE
        },
    );
    ui.painter().text(
        pos2(indep_rect.max.x + 4.0, fbs_lbl_y),
        Align2::LEFT_CENTER,
        "FB Shift",
        fonts::regular(9.5),
        COL_TEXT_BRIGHT,
    );

    // =========================================================================
    // 4. COLUMNA DERECHA: WET FX + WIDTH + MIX
    // =========================================================================
    let right_x = fb_col_rect.max.x + gap;
    let wet_fx_count = app
        .current_tracks()
        .get(track_idx)
        .and_then(|t| t.effects.get(slot_idx))
        .map(|s| s.wet_fx_chain.len())
        .unwrap_or(0);

    let wet_tab_h = 30.0_f32;
    let wet_tab_rect = Rect::from_min_size(pos2(right_x, top_y), vec2(right_col_w, wet_tab_h));
    let wet_resp = ui
        .interact(
            wet_tab_rect,
            ui.id().with(("fsp_wet_tab", track_idx, slot_idx)),
            Sense::click(),
        )
        .on_hover_text("Show/Hide Wet FX Nested Device Chain");
    if wet_resp.clicked() {
        wet_fx_open = !wet_fx_open;
        wet_toggle_changed = true;
    }
    if wet_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    paint_sub_panel(ui.painter(), wet_tab_rect);

    let bracket_col = if wet_fx_open || wet_fx_count > 0 {
        COL_EMERALD
    } else if wet_resp.hovered() {
        COL_EMERALD_DIM
    } else {
        Color32::from_rgb(0x23, 0x9C, 0x6A)
    };
    // Barra esmeralda superior con patillas laterales tipo corchete (como en la captura)
    let bx0 = wet_tab_rect.min.x + 3.0;
    let bx1 = wet_tab_rect.max.x - 3.0;
    let by0 = wet_tab_rect.min.y + 3.0;
    ui.painter().line_segment([pos2(bx0, by0), pos2(bx1, by0)], Stroke::new(1.8_f32, bracket_col));
    ui.painter().line_segment(
        [pos2(bx0, by0), pos2(bx0, by0 + 3.5)],
        Stroke::new(1.5_f32, bracket_col),
    );
    ui.painter().line_segment(
        [pos2(bx1, by0), pos2(bx1, by0 + 3.5)],
        Stroke::new(1.5_f32, bracket_col),
    );

    let wet_label = if !wet_fx_open && wet_fx_count > 0 {
        format!("Wet FX ({wet_fx_count})")
    } else {
        "Wet FX".to_string()
    };
    ui.painter().text(
        pos2(wet_tab_rect.center().x, wet_tab_rect.center().y + 2.0),
        Align2::CENTER_CENTER,
        &wet_label,
        fonts::semibold(9.8),
        if wet_fx_open || wet_resp.hovered() {
            Color32::WHITE
        } else {
            COL_TEXT_BRIGHT
        },
    );

    let out_rect = Rect::from_min_max(
        pos2(right_x, wet_tab_rect.max.y + gap),
        pos2(right_x + right_col_w, bot_y),
    );
    paint_sub_panel(ui.painter(), out_rect);

    let out_avail_h = out_rect.height();
    let width_top = out_rect.min.y + (out_avail_h * 0.27 - k_diam * 0.5).max(16.0);
    let mix_top = out_rect.min.y + (out_avail_h * 0.68 - k_diam * 0.5).max(width_top + 56.0);

    let mut width_norm = (params.width * 0.5).clamp(0.0, 1.0);
    if draw_std_knob(
        ui,
        out_rect.center().x,
        width_top,
        k_diam,
        &mut width_norm,
        def.width * 0.5,
        true,
        Some("Width"),
        &format!("Stereo Width: {:.1}%", params.width * 100.0),
    ) {
        params.width = width_norm * 2.0;
        changed = true;
    }

    let mut mix_norm = params.mix.clamp(0.0, 1.0);
    if draw_std_knob(
        ui,
        out_rect.center().x,
        mix_top,
        k_diam,
        &mut mix_norm,
        def.mix,
        false,
        Some("Mix"),
        &format!("Dry/Wet Mix: {:.1}%", params.mix * 100.0),
    ) {
        params.mix = mix_norm;
        changed = true;
    }

    // Persistir cambios de estado y sincronizar con el motor RT
    {
        let mut map = state_map().lock().unwrap();
        let st = map.entry(key).or_default();
        st.params = params;
        st.wet_fx_open = wet_fx_open;
        st.shift_text_buf = shift_text_buf;
        st.editing_shift_text = editing_shift_text;
    }
    if changed {
        if let Some(et_idx) = engine_track_idx {
            push_all_params(app, et_idx, slot_idx, &params);
        }
    }
    let _ = wet_toggle_changed;
}
