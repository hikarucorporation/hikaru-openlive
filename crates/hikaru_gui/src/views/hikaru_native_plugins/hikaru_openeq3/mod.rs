// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_openeq3/mod.rs
//! **Hikaru OpenEQ+**: ecualizador paramétrico de 8 bandas inspirado en **EQ+**
//! de Bitwig Studio para `hikaru_dsp::effects::EqPlusEffect`.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use egui::{
    pos2, vec2, Align2, Color32, CornerRadius, CursorIcon, PopupCloseBehavior, Rect, Sense,
    Stroke,
};
use hikaru_dsp::effects::{
    AnalyzerResolution, AnalyzerSpeed, EqPlusBandParams, EqPlusBandType, EqPlusParams, EqPlusRange,
    EQ_PLUS_BAND_COUNT, EQ_PLUS_MAX_GAIN_DB, EQ_PLUS_MAX_OUT_GAIN_DB, EQ_PLUS_MAX_Q,
    EQ_PLUS_MAX_SHIFT_ST, EQ_PLUS_MIN_GAIN_DB, EQ_PLUS_MIN_OUT_GAIN_DB, EQ_PLUS_MIN_Q,
    EQ_PLUS_MIN_SHIFT_ST, EQ_PLUS_PARAM_COUNT,
};

use crate::app::AppState;
use crate::ui::bitwig_kit;
use crate::ui::fonts;

pub const OPEN_EQ_PLUS_CARD_WIDTH: f32 = 380.0;
pub const OPEN_EQ_PLUS_WET_CARD_WIDTH: f32 = 340.0;

const VIEW_MIN_HZ: f32 = 20.0;
const VIEW_MAX_HZ: f32 = 20_000.0;
const SPECTRUM_BINS: usize = 96;

const BITWIG_ORANGE: Color32 = Color32::from_rgb(0xFF, 0x7A, 0x1A);
const COL_BG_WELL: Color32 = Color32::from_rgb(0x18, 0x19, 0x1D);
const COL_GRID: Color32 = Color32::from_rgb(0x28, 0x2A, 0x31);
const COL_GRID_ZERO: Color32 = Color32::from_rgb(0x3A, 0x3D, 0x46);
const COL_LABEL: Color32 = Color32::from_rgb(0xCA, 0xCD, 0xD4);
const COL_MUTED: Color32 = Color32::from_rgb(0x7E, 0x82, 0x8C);

pub fn is_openeq_plus_name(name: &str) -> bool {
    matches!(
        name,
        "Hikaru OpenEQ+"
            | "Hikaru OpenEQ3"
            | "OpenEQ+"
            | "OpenEQ3"
            | "EQ+"
            | "Hikaru EQ+"
            | "EQ-2"
            | "EQ-5"
            | "EQ-DJ"
    )
}

#[derive(Clone)]
struct UiSlotState {
    params: EqPlusParams,
    show_analyzer_bar: bool,
    smoothed_pre: Vec<f32>,
    smoothed_post: Vec<f32>,
}

impl Default for UiSlotState {
    fn default() -> Self {
        Self {
            params: EqPlusParams::default(),
            show_analyzer_bar: false,
            smoothed_pre: vec![-90.0; SPECTRUM_BINS],
            smoothed_post: vec![-90.0; SPECTRUM_BINS],
        }
    }
}

type SlotStateMap = Mutex<HashMap<(usize, usize), UiSlotState>>;
type LoadedSet = Mutex<HashSet<(usize, usize)>>;
type WetStateMap = Mutex<HashMap<(usize, usize, usize), UiSlotState>>;

fn state_map() -> &'static SlotStateMap {
    static CELL: OnceLock<SlotStateMap> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(HashMap::new()))
}

fn loaded_set() -> &'static LoadedSet {
    static CELL: OnceLock<LoadedSet> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(HashSet::new()))
}

fn wet_state_map() -> &'static WetStateMap {
    static CELL: OnceLock<WetStateMap> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn clear_slot_state(track_idx: usize, slot_idx: usize) {
    state_map().lock().unwrap().remove(&(track_idx, slot_idx));
    loaded_set().lock().unwrap().remove(&(track_idx, slot_idx));
}

pub fn clear_wet_fx_state(track_idx: usize, parent_slot_idx: usize) {
    wet_state_map()
        .lock()
        .unwrap()
        .retain(|&(t, p, _), _| !(t == track_idx && p == parent_slot_idx));
}

pub fn remove_wet_fx_slot_state(track_idx: usize, parent_slot_idx: usize, wet_idx: usize) {
    let mut map = wet_state_map().lock().unwrap();
    let existing: Vec<((usize, usize, usize), UiSlotState)> = map
        .iter()
        .filter(|(&(t, p, _), _)| t == track_idx && p == parent_slot_idx)
        .map(|(k, v)| (*k, v.clone()))
        .collect();
    map.retain(|&(t, p, _), _| !(t == track_idx && p == parent_slot_idx));
    for ((t, p, w), st) in existing {
        if w < wet_idx {
            map.insert((t, p, w), st);
        } else if w > wet_idx {
            map.insert((t, p, w - 1), st);
        }
    }
}

pub struct OpenEqPreset {
    pub name: &'static str,
    pub params: EqPlusParams,
}

pub fn factory_presets() -> Vec<OpenEqPreset> {
    let mut vocal = EqPlusParams::default();
    vocal.bands[0] = EqPlusBandParams {
        enabled: true,
        band_type: EqPlusBandType::LowCut4,
        freq_hz: 85.0,
        gain_db: 0.0,
        q: 0.71,
    };
    vocal.bands[1] = EqPlusBandParams {
        enabled: true,
        band_type: EqPlusBandType::Bell,
        freq_hz: 280.0,
        gain_db: -2.4,
        q: 1.4,
    };
    vocal.bands[2] = EqPlusBandParams {
        enabled: true,
        band_type: EqPlusBandType::Bell,
        freq_hz: 3200.0,
        gain_db: 3.2,
        q: 1.2,
    };
    vocal.bands[3] = EqPlusBandParams {
        enabled: true,
        band_type: EqPlusBandType::HighShelf,
        freq_hz: 10_000.0,
        gain_db: 2.5,
        q: 0.71,
    };

    let mut master = EqPlusParams::default();
    master.bands[0] = EqPlusBandParams {
        enabled: true,
        band_type: EqPlusBandType::LowCut2,
        freq_hz: 28.0,
        gain_db: 0.0,
        q: 0.71,
    };
    master.bands[1] = EqPlusBandParams {
        enabled: true,
        band_type: EqPlusBandType::LowShelf,
        freq_hz: 110.0,
        gain_db: -1.2,
        q: 0.8,
    };
    master.bands[2] = EqPlusBandParams {
        enabled: true,
        band_type: EqPlusBandType::HighShelf,
        freq_hz: 8500.0,
        gain_db: 1.8,
        q: 0.75,
    };

    let mut rainbow = EqPlusParams::default();
    let demo_specs = [
        (45.3, -1.9),
        (78.0, 4.7),
        (246.0, -2.8),
        (696.0, 4.2),
        (1250.0, -2.2),
        (2780.0, 3.5),
        (6910.0, -2.8),
        (9980.0, 3.5),
    ];
    for (i, (f, g)) in demo_specs.into_iter().enumerate() {
        rainbow.bands[i] = EqPlusBandParams {
            enabled: true,
            band_type: EqPlusBandType::Bell,
            freq_hz: f,
            gain_db: g,
            q: 1.0,
        };
    }
    rainbow.selected_band = 1;

    let mut telephone = EqPlusParams::default();
    telephone.bands[0] = EqPlusBandParams {
        enabled: true,
        band_type: EqPlusBandType::LowCut4,
        freq_hz: 400.0,
        gain_db: 0.0,
        q: 0.71,
    };
    telephone.bands[1] = EqPlusBandParams {
        enabled: true,
        band_type: EqPlusBandType::Bell,
        freq_hz: 1200.0,
        gain_db: 4.0,
        q: 1.5,
    };
    telephone.bands[2] = EqPlusBandParams {
        enabled: true,
        band_type: EqPlusBandType::HighCut4,
        freq_hz: 3200.0,
        gain_db: 0.0,
        q: 0.71,
    };

    vec![
        OpenEqPreset {
            name: "Default (Flat)",
            params: EqPlusParams::default(),
        },
        OpenEqPreset {
            name: "Vocal Presence",
            params: vocal,
        },
        OpenEqPreset {
            name: "Master Clean Tilt",
            params: master,
        },
        OpenEqPreset {
            name: "8-Band Rainbow",
            params: rainbow,
        },
        OpenEqPreset {
            name: "Telephone Bandpass",
            params: telephone,
        },
    ]
}

fn push_all_params(app: &AppState, engine_track_idx: usize, slot_idx: usize, params: &EqPlusParams) {
    for id in 0..EQ_PLUS_PARAM_COUNT as u32 {
        if let Some(value) = params.get_normalized(id) {
            app.audio_proxy
                .send(crate::audio_proxy::GuiCommand::SetExternalPluginParam {
                    track_idx: engine_track_idx,
                    slot_idx,
                    param_id: id,
                    value,
                });
        }
    }
}

pub fn render_preset_menu(ui: &mut egui::Ui, app: &mut AppState, track_idx: usize, slot_idx: usize) {
    ui.set_min_width(185.0);
    ui.label(
        egui::RichText::new("Hikaru OpenEQ+ Presets")
            .font(fonts::semibold(11.0))
            .color(BITWIG_ORANGE),
    );
    ui.separator();
    for preset in factory_presets() {
        if ui.button(preset.name).clicked() {
            let mut st = state_map()
                .lock()
                .unwrap()
                .entry((track_idx, slot_idx))
                .or_default()
                .clone();
            st.params = preset.params;
            state_map()
                .lock()
                .unwrap()
                .insert((track_idx, slot_idx), st.clone());
            if let Some(et_idx) = track_idx.checked_sub(1) {
                push_all_params(app, et_idx, slot_idx, &st.params);
            }
            app.project_dirty = true;
            ui.close_menu();
        }
    }
}

/// Color espectral arcoíris estilo Bitwig EQ+ según frecuencia (20 Hz rojo -> 20 kHz blanco/violeta).
fn freq_rainbow_color(freq_hz: f32) -> Color32 {
    let t = freq_to_t(freq_hz);
    let stops: [(f32, [u8; 3]); 7] = [
        (0.00, [0xE6, 0x3B, 0x2E]), // rojo (20 Hz)
        (0.20, [0xF0, 0x8A, 0x24]), // naranja (80 Hz)
        (0.38, [0xEC, 0xD2, 0x2E]), // amarillo (250 Hz)
        (0.55, [0x48, 0xD8, 0x58]), // verde (900 Hz)
        (0.72, [0x30, 0xD8, 0xE8]), // cian (3 kHz)
        (0.88, [0x78, 0x9C, 0xF8]), // azul-violeta (9 kHz)
        (1.00, [0xE8, 0xEC, 0xF4]), // blanco-hielo (20 kHz)
    ];
    for w in stops.windows(2) {
        let (t0, c0) = w[0];
        let (t1, c1) = w[1];
        if t <= t1 {
            let f = ((t - t0) / (t1 - t0).max(1e-6)).clamp(0.0, 1.0);
            let lerp = |a: u8, b: u8| -> u8 { ((a as f32) + ((b as f32) - (a as f32)) * f) as u8 };
            return Color32::from_rgb(lerp(c0[0], c1[0]), lerp(c0[1], c1[1]), lerp(c0[2], c1[2]));
        }
    }
    Color32::WHITE
}

#[inline]
fn freq_to_t(freq_hz: f32) -> f32 {
    let min_l = VIEW_MIN_HZ.ln();
    let max_l = VIEW_MAX_HZ.ln();
    ((freq_hz.clamp(VIEW_MIN_HZ, VIEW_MAX_HZ).ln() - min_l) / (max_l - min_l)).clamp(0.0, 1.0)
}

#[inline]
fn t_to_freq(t: f32) -> f32 {
    let min_l = VIEW_MIN_HZ.ln();
    let max_l = VIEW_MAX_HZ.ln();
    (min_l + t.clamp(0.0, 1.0) * (max_l - min_l)).exp()
}

#[inline]
fn db_to_y(db: f32, rect: Rect, max_db: f32) -> f32 {
    let norm = (db / max_db).clamp(-1.15, 1.15);
    rect.center().y - norm * (rect.height() * 0.45)
}

#[inline]
fn y_to_db(y: f32, rect: Rect, max_db: f32) -> f32 {
    let norm = (rect.center().y - y) / (rect.height() * 0.45).max(1.0);
    (norm * max_db).clamp(EQ_PLUS_MIN_GAIN_DB, EQ_PLUS_MAX_GAIN_DB)
}

fn format_freq(hz: f32) -> String {
    if hz >= 1000.0 {
        format!("{:.2} kHz", hz / 1000.0)
    } else if hz >= 100.0 {
        format!("{:.0} Hz", hz)
    } else {
        format!("{:.1} Hz", hz)
    }
}

fn render_draggable_value(
    ui: &mut egui::Ui,
    val: &mut f32,
    text: String,
    color: Color32,
    width: f32,
    delta_fn: impl FnOnce(&mut f32, f32),
    reset_fn: impl FnOnce(&mut f32),
) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 16.0), Sense::click_and_drag());
    let hovered = resp.hovered() || resp.dragged();
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        fonts::regular(10.5),
        if hovered { Color32::WHITE } else { color },
    );
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
    }
    let mut changed = false;
    if resp.dragged() {
        let dy = resp.drag_delta().y;
        if dy.abs() > 0.01 {
            delta_fn(val, -dy);
            changed = true;
        }
    }
    if resp.double_clicked() {
        reset_fn(val);
        changed = true;
    }
    changed
}

#[allow(clippy::too_many_arguments)]
fn render_eq_plus_surface(
    ui: &mut egui::Ui,
    state: &mut UiSlotState,
    engine_track_idx: Option<usize>,
    slot_idx: usize,
    id_salt: impl std::hash::Hash + Copy,
) -> bool {
    let full_rect = ui.available_rect_before_wrap();
    let (_, _) = ui.allocate_exact_size(full_rect.size(), Sense::hover());

    let mut changed = false;
    let params = &mut state.params;

    let panel_rect = full_rect.shrink(1.5);
    ui.painter().rect_filled(
        panel_rect,
        CornerRadius::same(4),
        bitwig_kit::LEMON_PLUG_BG,
    );
    ui.painter().rect_stroke(
        panel_rect,
        CornerRadius::same(4),
        Stroke::new(1.0_f32, bitwig_kit::LEMON_PLUG_PANEL_LINE),
        egui::StrokeKind::Inside,
    );

    // =========================================================================
    // 1. Barra superior: Shift | Gain | Range (±10 ±20 ±30) | Adaptive Q
    // =========================================================================
    let top_h = 20.0;
    let top_rect = Rect::from_min_size(
        pos2(panel_rect.min.x + 6.0, panel_rect.min.y + 4.0),
        vec2((panel_rect.width() - 12.0).max(120.0), top_h),
    );
    let mut top_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(top_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    top_ui.spacing_mut().item_spacing.x = 4.0;

    top_ui.label(egui::RichText::new("Shift").font(fonts::regular(10.0)).color(COL_LABEL));
    let mut shift_val = params.shift_st;
    let shift_txt = format!("{:.2} st", shift_val);
    if render_draggable_value(
        &mut top_ui,
        &mut shift_val,
        shift_txt,
        BITWIG_ORANGE,
        50.0,
        |v, d| {
            *v = (*v + d * 0.15).clamp(EQ_PLUS_MIN_SHIFT_ST, EQ_PLUS_MAX_SHIFT_ST);
        },
        |v| {
            *v = 0.0;
        },
    ) {
        params.shift_st = shift_val;
        changed = true;
    }

    top_ui.add_space(4.0);
    top_ui.label(egui::RichText::new("Gain").font(fonts::regular(10.0)).color(COL_LABEL));
    let mut out_val = params.output_gain_db;
    let out_txt = format!("{:.1} dB", out_val);
    if render_draggable_value(
        &mut top_ui,
        &mut out_val,
        out_txt,
        BITWIG_ORANGE,
        46.0,
        |v, d| {
            *v = (*v + d * 0.15).clamp(EQ_PLUS_MIN_OUT_GAIN_DB, EQ_PLUS_MAX_OUT_GAIN_DB);
        },
        |v| {
            *v = 0.0;
        },
    ) {
        params.output_gain_db = out_val;
        changed = true;
    }

    top_ui.add_space(4.0);
    top_ui.label(egui::RichText::new("Range").font(fonts::regular(10.0)).color(COL_LABEL));
    for r in EqPlusRange::ALL {
        let active = params.range == r;
        let (rr, r_resp) = top_ui.allocate_exact_size(vec2(24.0, 15.0), Sense::click());
        if active {
            top_ui.painter().line_segment(
                [pos2(rr.min.x + 2.0, rr.max.y - 1.0), pos2(rr.max.x - 2.0, rr.max.y - 1.0)],
                Stroke::new(1.5, BITWIG_ORANGE),
            );
        }
        top_ui.painter().text(
            rr.center(),
            Align2::CENTER_CENTER,
            r.label(),
            fonts::regular(10.0),
            if active { BITWIG_ORANGE } else { COL_MUTED },
        );
        if r_resp.clicked() {
            params.range = r;
            changed = true;
        }
    }

    // Botón Adaptive Q a la derecha
    let aq_rect = Rect::from_min_size(pos2(top_rect.max.x - 22.0, top_rect.min.y + 1.0), vec2(20.0, 18.0));
    let aq_resp = ui
        .interact(aq_rect, ui.id().with((id_salt, "adaptive_q")), Sense::click())
        .on_hover_text("Adaptive Q (Proportional Q for Bell bands)");
    ui.painter().text(
        aq_rect.center(),
        Align2::CENTER_CENTER,
        "AQ",
        fonts::semibold(9.5),
        if params.adaptive_q { BITWIG_ORANGE } else { COL_MUTED },
    );
    if aq_resp.clicked() {
        params.adaptive_q = !params.adaptive_q;
        changed = true;
    }

    // =========================================================================
    // 2. Canvas principal de respuesta en frecuencia + analizador Pre/Post
    // =========================================================================
    let bot_h = 24.0;
    let canvas_rect = Rect::from_min_max(
        pos2(panel_rect.min.x + 6.0, top_rect.max.y + 3.0),
        pos2(panel_rect.max.x - 6.0, (panel_rect.max.y - bot_h - 6.0).max(top_rect.max.y + 60.0)),
    );
    ui.painter().rect_filled(canvas_rect, CornerRadius::same(3), COL_BG_WELL);
    ui.painter().rect_stroke(
        canvas_rect,
        CornerRadius::same(3),
        Stroke::new(1.0, Color32::from_rgb(0x2B, 0x2D, 0x34)),
        egui::StrokeKind::Inside,
    );

    let max_db = params.range.max_db();
    let p = ui.painter().with_clip_rect(canvas_rect);

    // Rejilla de frecuencias
    for &f in &[20.0_f32, 30.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0] {
        let x = canvas_rect.min.x + freq_to_t(f) * canvas_rect.width();
        let major = matches!(f as i32, 20 | 100 | 1000 | 10_000);
        p.line_segment(
            [pos2(x, canvas_rect.min.y), pos2(x, canvas_rect.max.y)],
            Stroke::new(1.0, if major { COL_GRID_ZERO } else { COL_GRID }),
        );
        if major {
            let lbl = match f as i32 {
                20 => "20",
                100 => "100",
                1000 => "1k",
                10_000 => "10k",
                _ => "",
            };
            p.text(
                pos2(x + 3.0, canvas_rect.min.y + 8.0),
                Align2::LEFT_CENTER,
                lbl,
                fonts::regular(9.0),
                COL_MUTED,
            );
        }
    }

    // Línea 0 dB y ±max_db
    let y_zero = db_to_y(0.0, canvas_rect, max_db);
    p.line_segment(
        [pos2(canvas_rect.min.x, y_zero), pos2(canvas_rect.max.x, y_zero)],
        Stroke::new(1.0, COL_GRID_ZERO),
    );
    let y_top = db_to_y(max_db, canvas_rect, max_db);
    let y_bot = db_to_y(-max_db, canvas_rect, max_db);
    p.line_segment(
        [pos2(canvas_rect.min.x, y_top), pos2(canvas_rect.max.x, y_top)],
        Stroke::new(0.7, COL_GRID),
    );
    p.line_segment(
        [pos2(canvas_rect.min.x, y_bot), pos2(canvas_rect.max.x, y_bot)],
        Stroke::new(0.7, COL_GRID),
    );
    p.text(
        pos2(canvas_rect.max.x - 3.0, y_top + 6.0),
        Align2::RIGHT_CENTER,
        format!("+{}", max_db as i32),
        fonts::regular(8.5),
        COL_MUTED,
    );
    p.text(
        pos2(canvas_rect.max.x - 3.0, y_bot - 6.0),
        Align2::RIGHT_CENTER,
        format!("-{}", max_db as i32),
        fonts::regular(8.5),
        COL_MUTED,
    );

    // Analizador de espectro Pre/Post en tiempo real
    if params.show_pre || params.show_post {
        if let Some(et_idx) = engine_track_idx {
            if let Some(telem) = hikaru_dsp::effects::eq_plus::telemetry::get((et_idx, slot_idx)) {
                let speed = params.analyzer_speed.smoothing_coeff();
                if speed > 0.0 {
                    let (raw_pre, raw_post) = telem.compute_spectrum_db(
                        params.analyzer_resolution.fft_size(),
                        SPECTRUM_BINS,
                        VIEW_MIN_HZ,
                        VIEW_MAX_HZ,
                    );
                    for i in 0..SPECTRUM_BINS {
                        state.smoothed_pre[i] += (raw_pre[i] - state.smoothed_pre[i]) * speed;
                        state.smoothed_post[i] += (raw_post[i] - state.smoothed_post[i]) * speed;
                    }
                }
                let spec_to_y = |db: f32| -> f32 {
                    let norm = ((db + 84.0) / 84.0).clamp(0.0, 1.0);
                    canvas_rect.max.y - norm * canvas_rect.height()
                };
                if params.show_pre {
                    let pts: Vec<egui::Pos2> = (0..SPECTRUM_BINS)
                        .map(|i| {
                            let x = canvas_rect.min.x
                                + (i as f32 / (SPECTRUM_BINS - 1) as f32) * canvas_rect.width();
                            pos2(x, spec_to_y(state.smoothed_pre[i]))
                        })
                        .collect();
                    p.add(egui::Shape::line(
                        pts,
                        Stroke::new(1.0, Color32::from_rgba_unmultiplied(0xE8, 0x88, 0x30, 110)),
                    ));
                }
                if params.show_post {
                    let pts: Vec<egui::Pos2> = (0..SPECTRUM_BINS)
                        .map(|i| {
                            let x = canvas_rect.min.x
                                + (i as f32 / (SPECTRUM_BINS - 1) as f32) * canvas_rect.width();
                            pos2(x, spec_to_y(state.smoothed_post[i]))
                        })
                        .collect();
                    p.add(egui::Shape::line(
                        pts,
                        Stroke::new(1.1, Color32::from_rgba_unmultiplied(0x50, 0xC8, 0xF0, 145)),
                    ));
                }
            }
        }
    }

    // Relleno sutil de la banda seleccionada
    let sel_idx = params.selected_band.min(EQ_PLUS_BAND_COUNT - 1);
    let sr_hz = 48_000.0_f64;
    let curve_steps = 110;
    if params.bands[sel_idx].enabled && params.bands[sel_idx].band_type.is_active() {
        let sel_col = freq_rainbow_color(params.bands[sel_idx].freq_hz);
        let fill_col = Color32::from_rgba_unmultiplied(sel_col.r(), sel_col.g(), sel_col.b(), 26);
        for i in 0..curve_steps {
            let t0 = i as f32 / curve_steps as f32;
            let t1 = (i + 1) as f32 / curve_steps as f32;
            let f0 = t_to_freq(t0) as f64;
            let f1 = t_to_freq(t1) as f64;
            let d0 = params.band_magnitude_db(sel_idx, f0, sr_hz) as f32;
            let d1 = params.band_magnitude_db(sel_idx, f1, sr_hz) as f32;
            let x0 = canvas_rect.min.x + t0 * canvas_rect.width();
            let x1 = canvas_rect.min.x + t1 * canvas_rect.width();
            let y0 = db_to_y(d0, canvas_rect, max_db);
            let y1 = db_to_y(d1, canvas_rect, max_db);
            p.add(egui::Shape::convex_polygon(
                vec![pos2(x0, y_zero), pos2(x0, y0), pos2(x1, y1), pos2(x1, y_zero)],
                fill_col,
                Stroke::NONE,
            ));
        }
    }

    // Curva total arcoíris
    for i in 0..curve_steps {
        let t0 = i as f32 / curve_steps as f32;
        let t1 = (i + 1) as f32 / curve_steps as f32;
        let f0 = t_to_freq(t0);
        let f1 = t_to_freq(t1);
        let d0 = params.total_magnitude_db(f0 as f64, sr_hz) as f32;
        let d1 = params.total_magnitude_db(f1 as f64, sr_hz) as f32;
        let x0 = canvas_rect.min.x + t0 * canvas_rect.width();
        let x1 = canvas_rect.min.x + t1 * canvas_rect.width();
        let y0 = db_to_y(d0, canvas_rect, max_db);
        let y1 = db_to_y(d1, canvas_rect, max_db);
        p.line_segment([pos2(x0, y0), pos2(x1, y1)], Stroke::new(2.0, freq_rainbow_color(f0)));
    }

    // Interacción del canvas (doble clic para añadir nodo Bell)
    let canvas_resp = ui.interact(
        canvas_rect,
        ui.id().with((id_salt, "eq_canvas")),
        Sense::click(),
    );
    if canvas_resp.double_clicked() {
        if let Some(pos) = canvas_resp.interact_pointer_pos() {
            if let Some(free_idx) = params.bands.iter().position(|b| !b.band_type.is_active()) {
                let t = ((pos.x - canvas_rect.min.x) / canvas_rect.width()).clamp(0.0, 1.0);
                params.bands[free_idx].enabled = true;
                params.bands[free_idx].band_type = EqPlusBandType::Bell;
                params.bands[free_idx].freq_hz = t_to_freq(t);
                params.bands[free_idx].gain_db = y_to_db(pos.y, canvas_rect, max_db);
                params.bands[free_idx].q = 1.0;
                params.selected_band = free_idx;
                changed = true;
            }
        }
    }

    // Nodos interactivos (1..=8)
    for b_idx in 0..EQ_PLUS_BAND_COUNT {
        let band = params.bands[b_idx];
        if !band.band_type.is_active() {
            continue;
        }
        let bx = canvas_rect.min.x + freq_to_t(band.freq_hz) * canvas_rect.width();
        let display_db = if band.band_type.uses_gain() {
            band.gain_db
        } else {
            0.0
        };
        let by = db_to_y(display_db, canvas_rect, max_db);
        let node_pos = pos2(bx, by);
        let is_sel = params.selected_band == b_idx;

        if is_sel {
            p.line_segment(
                [pos2(bx, canvas_rect.min.y), pos2(bx, canvas_rect.max.y)],
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(0xFF, 0xFF, 0xFF, 120)),
            );
        }

        let node_r = Rect::from_center_size(node_pos, vec2(18.0, 18.0));
        let node_resp = ui.interact(
            node_r,
            ui.id().with((id_salt, "band_node", b_idx)),
            Sense::click_and_drag(),
        );
        if node_resp.hovered() || node_resp.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        }
        if node_resp.clicked() || node_resp.drag_started() {
            params.selected_band = b_idx;
            changed = true;
        }
        if node_resp.dragged() {
            if let Some(pointer) = ui.ctx().pointer_latest_pos() {
                let t = ((pointer.x - canvas_rect.min.x) / canvas_rect.width()).clamp(0.0, 1.0);
                params.bands[b_idx].freq_hz = t_to_freq(t);
                if params.bands[b_idx].band_type.uses_gain() {
                    params.bands[b_idx].gain_db = y_to_db(pointer.y, canvas_rect, max_db);
                } else {
                    let q_delta = -node_resp.drag_delta().y * 0.04;
                    params.bands[b_idx].q = (params.bands[b_idx].q + q_delta).clamp(EQ_PLUS_MIN_Q, EQ_PLUS_MAX_Q);
                }
                changed = true;
            }
        }
        if node_resp.hovered() {
            let scroll_y = ui.input(|i| i.raw_scroll_delta.y);
            if scroll_y.abs() > 0.1 {
                let factor = if scroll_y > 0.0 { 1.1 } else { 1.0 / 1.1 };
                params.bands[b_idx].q = (params.bands[b_idx].q * factor).clamp(EQ_PLUS_MIN_Q, EQ_PLUS_MAX_Q);
                changed = true;
            }
        }
        node_resp.context_menu(|ui| {
            let mut en = params.bands[b_idx].enabled;
            if ui.checkbox(&mut en, "Enabled").clicked() {
                params.bands[b_idx].enabled = en;
                changed = true;
                ui.close_menu();
            }
            if ui.button("Remove Band (Off)").clicked() {
                params.bands[b_idx].band_type = EqPlusBandType::Off;
                changed = true;
                ui.close_menu();
            }
            ui.separator();
            for t in EqPlusBandType::ALL {
                if ui.selectable_label(params.bands[b_idx].band_type == t, t.label()).clicked() {
                    params.bands[b_idx].band_type = t;
                    params.bands[b_idx].enabled = true;
                    changed = true;
                    ui.close_menu();
                }
            }
        });

        let ring_col = if !band.enabled {
            COL_MUTED
        } else if is_sel {
            Color32::WHITE
        } else {
            freq_rainbow_color(band.freq_hz)
        };
        p.circle_filled(node_pos, 7.5, if is_sel { Color32::WHITE } else { Color32::from_rgb(0xE8, 0xEC, 0xF2) });
        p.circle_stroke(node_pos, 7.5, Stroke::new(1.5, ring_col));
        p.text(
            node_pos,
            Align2::CENTER_CENTER,
            format!("{}", b_idx + 1),
            fonts::semibold(9.5),
            Color32::from_rgb(0x18, 0x1A, 0x20),
        );
    }

    // =========================================================================
    // 3. Barra inferior: Band Controls <-> Analyzer Settings
    // =========================================================================
    let bot_rect = Rect::from_min_size(
        pos2(panel_rect.min.x + 6.0, canvas_rect.max.y + 4.0),
        vec2((panel_rect.width() - 12.0).max(120.0), bot_h),
    );
    let mut bot_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(bot_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    bot_ui.spacing_mut().item_spacing.x = 4.0;

    // Botón alternar barra Analyzer
    let an_btn = bot_ui
        .selectable_label(state.show_analyzer_bar, "📊")
        .on_hover_text("Toggle Spectrum Analyzer Settings");
    if an_btn.clicked() {
        state.show_analyzer_bar = !state.show_analyzer_bar;
    }

    if state.show_analyzer_bar {
        bot_ui.label(egui::RichText::new("Analyzer").font(fonts::regular(10.0)).color(COL_LABEL));
        if bot_ui.selectable_label(params.show_pre, "Pre").clicked() {
            params.show_pre = !params.show_pre;
        }
        if bot_ui.selectable_label(params.show_post, "Post").clicked() {
            params.show_post = !params.show_post;
        }
        bot_ui.label(egui::RichText::new("Res").font(fonts::regular(10.0)).color(COL_MUTED));
        egui::ComboBox::from_id_salt((id_salt, "an_res"))
            .width(62.0)
            .selected_text(params.analyzer_resolution.label())
            .show_ui(&mut bot_ui, |ui| {
                for r in AnalyzerResolution::ALL {
                    if ui.selectable_value(&mut params.analyzer_resolution, r, r.label()).clicked() {
                        // visual only
                    }
                }
            });
        bot_ui.label(egui::RichText::new("Speed").font(fonts::regular(10.0)).color(COL_MUTED));
        egui::ComboBox::from_id_salt((id_salt, "an_spd"))
            .width(68.0)
            .selected_text(params.analyzer_speed.label())
            .show_ui(&mut bot_ui, |ui| {
                for s in AnalyzerSpeed::ALL {
                    if ui.selectable_value(&mut params.analyzer_speed, s, s.label()).clicked() {
                        // visual only
                    }
                }
            });
    } else {
        let sel = params.selected_band.min(EQ_PLUS_BAND_COUNT - 1);
        // Selector de banda 1..8
        let band_btn_id = bot_ui.id().with((id_salt, "band_sel_pop"));
        let band_lbl = format!("{} ▾", sel + 1);
        let b_resp = bot_ui.button(egui::RichText::new(band_lbl).font(fonts::semibold(10.0)));
        if b_resp.clicked() {
            bot_ui.memory_mut(|m| m.toggle_popup(band_btn_id));
        }
        egui::popup_below_widget(
            &bot_ui,
            band_btn_id,
            &b_resp,
            PopupCloseBehavior::CloseOnClickOutside,
            |ui| {
                for i in 0..EQ_PLUS_BAND_COUNT {
                    let b = params.bands[i];
                    let txt = format!("{}  {}", i + 1, b.band_type.label());
                    if ui.selectable_label(params.selected_band == i, txt).clicked() {
                        params.selected_band = i;
                        changed = true;
                        ui.close_menu();
                    }
                }
            },
        );

        // Solo & Enable
        if bot_ui
            .selectable_label(params.solo, "S")
            .on_hover_text("Solo selected band")
            .clicked()
        {
            params.solo = !params.solo;
            changed = true;
        }
        let mut en = params.bands[sel].enabled;
        if bot_ui
            .selectable_label(en, "⏻")
            .on_hover_text("Enable/Disable selected band")
            .clicked()
        {
            en = !en;
            params.bands[sel].enabled = en;
            changed = true;
        }

        // Selector de tipo (15 tipos)
        let mut btype = params.bands[sel].band_type;
        egui::ComboBox::from_id_salt((id_salt, "band_type_cb"))
            .width(86.0)
            .selected_text(btype.label())
            .show_ui(&mut bot_ui, |ui| {
                for t in EqPlusBandType::ALL {
                    let row_txt = format!("{:<12} {}", t.label(), t.description());
                    if ui.selectable_value(&mut btype, t, row_txt).clicked() {
                        params.bands[sel].band_type = btype;
                        if btype.is_active() {
                            params.bands[sel].enabled = true;
                        }
                        changed = true;
                    }
                }
            });

        // Freq / Gain / Q readouts
        let bcol = freq_rainbow_color(params.bands[sel].freq_hz);
        let mut f_val = params.bands[sel].freq_hz;
        let f_txt = format_freq(f_val);
        if render_draggable_value(
            &mut bot_ui,
            &mut f_val,
            f_txt,
            bcol,
            54.0,
            |v, d| {
                let norm = (EqPlusParams::freq_hz_to_norm(*v) + d * 0.004).clamp(0.0, 1.0);
                *v = EqPlusParams::norm_to_freq_hz(norm);
            },
            |v| {
                *v = hikaru_dsp::effects::EQ_PLUS_DEFAULT_BAND_FREQS_HZ[sel];
            },
        ) {
            params.bands[sel].freq_hz = f_val;
            changed = true;
        }

        let mut g_val = params.bands[sel].gain_db;
        let g_txt = format!("{:+.1} dB", g_val);
        if render_draggable_value(
            &mut bot_ui,
            &mut g_val,
            g_txt,
            bcol,
            46.0,
            |v, d| {
                *v = (*v + d * 0.15).clamp(EQ_PLUS_MIN_GAIN_DB, EQ_PLUS_MAX_GAIN_DB);
            },
            |v| {
                *v = 0.0;
            },
        ) {
            params.bands[sel].gain_db = g_val;
            changed = true;
        }

        let mut q_val = params.bands[sel].q;
        let q_txt = format!("{:.2}", q_val);
        if render_draggable_value(
            &mut bot_ui,
            &mut q_val,
            q_txt,
            bcol,
            34.0,
            |v, d| {
                let norm = (EqPlusParams::q_to_norm(*v) + d * 0.005).clamp(0.0, 1.0);
                *v = EqPlusParams::norm_to_q(norm);
            },
            |v| {
                *v = 1.0;
            },
        ) {
            params.bands[sel].q = q_val;
            changed = true;
        }
    }

    changed
}

pub fn render_module(ui: &mut egui::Ui, app: &mut AppState, track_idx: usize, slot_idx: usize) {
    let key = (track_idx, slot_idx);
    let engine_track_idx = track_idx.checked_sub(1);

    let mut st = {
        let mut map = state_map().lock().unwrap();
        map.entry(key).or_default().clone()
    };

    if let Some(et_idx) = engine_track_idx {
        if loaded_set().lock().unwrap().insert(key) {
            app.audio_proxy
                .send(crate::audio_proxy::GuiCommand::LoadNativeEffect {
                    track_idx: et_idx,
                    slot_idx,
                    name: "Hikaru OpenEQ+".to_string(),
                });
            push_all_params(app, et_idx, slot_idx, &st.params);
        }
    }

    let changed = render_eq_plus_surface(
        ui,
        &mut st,
        engine_track_idx,
        slot_idx,
        ("eqp_slot", track_idx, slot_idx),
    );
    state_map().lock().unwrap().insert(key, st.clone());
    if changed {
        if let Some(et_idx) = engine_track_idx {
            push_all_params(app, et_idx, slot_idx, &st.params);
        }
        app.project_dirty = true;
    }
}

pub fn render_wet_fx_module(
    ui: &mut egui::Ui,
    app: &mut AppState,
    track_idx: usize,
    parent_slot_idx: usize,
    wet_idx: usize,
) {
    let key = (track_idx, parent_slot_idx, wet_idx);
    let mut st = {
        let mut map = wet_state_map().lock().unwrap();
        map.entry(key).or_default().clone()
    };
    let changed = render_eq_plus_surface(
        ui,
        &mut st,
        None,
        wet_idx,
        ("eqp_wet", track_idx, parent_slot_idx, wet_idx),
    );
    wet_state_map().lock().unwrap().insert(key, st.clone());
    if changed {
        if let Some(et_idx) = track_idx.checked_sub(1) {
            app.audio_proxy
                .send(crate::audio_proxy::GuiCommand::SyncBit8WetFxParams {
                    track_idx: et_idx,
                    slot_idx: parent_slot_idx,
                    wet_idx,
                    params: st.params.to_param_pairs(),
                });
        }
        app.project_dirty = true;
    }
}
