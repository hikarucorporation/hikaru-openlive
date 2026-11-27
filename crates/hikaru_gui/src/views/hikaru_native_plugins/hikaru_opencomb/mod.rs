/*
 * Hikaru OpenLive / OpenStudio - Audio DAW
 * Copyright (C) 2026 Hikaru OpenStudio Developers
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 *
 * Uses Lemon Studio's `os-ui` design tokens, Barlow typography, and icon
 * geometry (`crates/os-ui/src/kit/lemon.rs`, `crates/os-ui/src/tabler.rs`,
 * GPL-3.0-or-later, compatible with AGPL-3.0-or-later) combined with Hikaru's
 * WGPU SDF rotary knob pipeline (`ui::bitwig_kit`).
 */

//! **Hikaru OpenComb**: panel nativo inspirado en el dispositivo **Comb** de Bitwig Studio
//! para `hikaru_dsp::effects::OpenCombFilter`.
//!
//! Geometría y controles:
//! - **Pad XY + curva de respuesta en frecuencia** (`X = Frequency` logarítmica `20 Hz..4.9 kHz`,
//!   `Y = Feedback` bipolar `-100%..+100%`).
//! - **Fila inferior (3 perillas)**: **`Freq`** (`20.0 Hz .. 3.47 kHz`), **`FB`** (`-100% .. +100%`),
//!   y **`Mix`** (`0% .. 100%`).

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use egui::{pos2, vec2, Align2, Color32, CornerRadius, CursorIcon, Rect, Sense, Stroke};
use hikaru_dsp::effects::{
    format_freq_label, freq_hz_to_note_label, OpenCombFilter, OpenCombParams,
    OPENCOMB_DEFAULT_FREQ_HZ, OPENCOMB_MAX_FREQ_HZ, OPENCOMB_MIN_FREQ_HZ, OPENCOMB_PARAM_COUNT,
};

use crate::app::AppState;
use crate::theme;
use crate::ui::bitwig_kit;
use crate::ui::fonts;

pub const OPENCOMB_CARD_WIDTH: f32 = 186.0;
pub const OPENCOMB_WET_CARD_WIDTH: f32 = 170.0;

const COMB_ORANGE: Color32 = Color32::from_rgb(0xFF, 0x84, 0x19);
const COL_LABEL: Color32 = Color32::from_rgb(0xCA, 0xCD, 0xD4);
const COL_GRID_TEXT: Color32 = Color32::from_rgb(0x74, 0x79, 0x84);

pub fn is_opencomb_name(name: &str) -> bool {
    matches!(
        name,
        "Hikaru OpenComb"
            | "OpenComb"
            | "Comb"
            | "Hikaru Comb"
            | "Hikaru Ultracomb"
            | "Hikaru Ultracomb Filter"
            | "Ultracomb"
    )
}

type SlotStateMap = Mutex<HashMap<(usize, usize), OpenCombParams>>;
type LoadedSet = Mutex<HashSet<(usize, usize)>>;
type WetStateMap = Mutex<HashMap<(usize, usize, usize), OpenCombParams>>;

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
    let existing: Vec<((usize, usize, usize), OpenCombParams)> = map
        .iter()
        .filter(|(&(t, p, _), _)| t == track_idx && p == parent_slot_idx)
        .map(|(&k, &v)| (k, v))
        .collect();
    map.retain(|&(t, p, _), _| !(t == track_idx && p == parent_slot_idx));
    for ((t, p, w), params) in existing {
        if w < wet_idx {
            map.insert((t, p, w), params);
        } else if w > wet_idx {
            map.insert((t, p, w - 1), params);
        }
    }
}

pub struct OpenCombPreset {
    pub name: &'static str,
    pub params: OpenCombParams,
}

pub fn factory_presets() -> Vec<OpenCombPreset> {
    vec![
        OpenCombPreset {
            name: "Default (104 Hz, 0% FB)",
            params: OpenCombParams::default(),
        },
        OpenCombPreset {
            name: "Metallic Resonator (+85%)",
            params: OpenCombParams {
                freq_hz: 220.0,
                feedback_pct: 85.0,
                mix_pct: 100.0,
            },
        },
        OpenCombPreset {
            name: "Hollow Negative (-85%)",
            params: OpenCombParams {
                freq_hz: 446.0,
                feedback_pct: -85.0,
                mix_pct: 100.0,
            },
        },
        OpenCombPreset {
            name: "Sub Bass Comb (57 Hz)",
            params: OpenCombParams {
                freq_hz: 56.9,
                feedback_pct: 76.0,
                mix_pct: 100.0,
            },
        },
        OpenCombPreset {
            name: "Crisp High Comb (1.74 kHz)",
            params: OpenCombParams {
                freq_hz: 1735.0,
                feedback_pct: -72.0,
                mix_pct: 100.0,
            },
        },
        OpenCombPreset {
            name: "Extreme Ring (-100%)",
            params: OpenCombParams {
                freq_hz: 99.4,
                feedback_pct: -100.0,
                mix_pct: 100.0,
            },
        },
    ]
}

fn push_all_params(
    app: &mut AppState,
    engine_track_idx: usize,
    slot_idx: usize,
    params: &OpenCombParams,
) {
    for id in 0..(OPENCOMB_PARAM_COUNT as u32) {
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

fn push_all_wet_params(
    app: &mut AppState,
    engine_track_idx: usize,
    parent_slot_idx: usize,
    wet_idx: usize,
    params: &OpenCombParams,
) {
    for id in 0..(OPENCOMB_PARAM_COUNT as u32) {
        if let Some(val) = params.get_normalized(id) {
            app.audio_proxy
                .send(crate::audio_proxy::GuiCommand::SetBit8WetFxParam {
                    track_idx: engine_track_idx,
                    slot_idx: parent_slot_idx,
                    wet_idx,
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
    ui.set_min_width(184.0);
    ui.label(
        egui::RichText::new("Hikaru OpenComb Presets")
            .font(fonts::semibold(10.0))
            .color(COMB_ORANGE),
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
            state_map()
                .lock()
                .unwrap()
                .insert((track_idx, slot_idx), preset.params);
            if let Some(et_idx) = track_idx.checked_sub(1) {
                push_all_params(app, et_idx, slot_idx, &preset.params);
            }
            app.project_dirty = true;
            ui.close_menu();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_comb_knob(
    ui: &mut egui::Ui,
    center_x: f32,
    top_y: f32,
    diameter: f32,
    norm: &mut f32,
    default_norm: f32,
    bipolar: bool,
    label: &str,
    label_font_sz: f32,
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
    let mut val = if bipolar {
        (*norm * 200.0 - 100.0).clamp(-100.0, 100.0)
    } else {
        (*norm * 100.0).clamp(0.0, 100.0)
    };
    let range = if bipolar { -100.0..=100.0 } else { 0.0..=100.0 };
    let def_val = if bipolar {
        default_norm * 200.0 - 100.0
    } else {
        default_norm * 100.0
    };
    let (resp, changed) = bitwig_kit::rotary_knob(
        &mut knob_ui,
        diameter,
        &mut val,
        range,
        def_val,
        COMB_ORANGE,
        bipolar,
    );
    if changed {
        *norm = if bipolar {
            ((val + 100.0) / 200.0).clamp(0.0, 1.0)
        } else {
            (val / 100.0).clamp(0.0, 1.0)
        };
    }
    ui.painter().text(
        pos2(center_x, knob_rect.max.y + 7.5),
        Align2::CENTER_CENTER,
        label,
        fonts::regular(label_font_sz),
        COL_LABEL,
    );
    let _ = resp.on_hover_text(tooltip);
    changed
}

fn freq_to_xy_x(freq_hz: f32, pad_rect: Rect) -> f32 {
    let f_min = 16.0_f32;
    let f_max = 4900.0_f32;
    let t = ((freq_hz.clamp(f_min, f_max) / f_min).ln() / (f_max / f_min).ln()).clamp(0.0, 1.0);
    pad_rect.min.x + 4.0 + t * (pad_rect.width() - 8.0).max(1.0)
}

fn xy_x_to_freq(x: f32, pad_rect: Rect) -> f32 {
    let f_min = 16.0_f32;
    let f_max = 4900.0_f32;
    let t = ((x - (pad_rect.min.x + 4.0)) / (pad_rect.width() - 8.0).max(1.0)).clamp(0.0, 1.0);
    (f_min * (f_max / f_min).powf(t)).clamp(OPENCOMB_MIN_FREQ_HZ, OPENCOMB_MAX_FREQ_HZ)
}

fn render_opencomb_surface(
    ui: &mut egui::Ui,
    params: &mut OpenCombParams,
    id_salt: impl std::hash::Hash + Copy,
) -> bool {
    let full_rect = ui.available_rect_before_wrap();
    let (_, _) = ui.allocate_exact_size(full_rect.size(), Sense::hover());

    let mut changed = false;
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

    let pw = panel_rect.width().max(120.0);
    let ph = panel_rect.height().max(150.0);
    let pad_x = 6.0_f32;
    let knob_area_h = (ph * 0.36).clamp(52.0, 68.0);
    let xy_rect = Rect::from_min_max(
        pos2(panel_rect.min.x + pad_x, panel_rect.min.y + 6.0),
        pos2(panel_rect.max.x - pad_x, panel_rect.max.y - knob_area_h - 6.0),
    );

    // =========================================================================
    // 1. Pad XY + Respuesta en Frecuencia
    // =========================================================================
    bitwig_kit::paint_lemon_plug_well(ui.painter(), xy_rect, 3);
    let xy_painter = ui.painter_at(xy_rect);

    // Líneas verticales logarítmicas (20 Hz, 100 Hz, 1 kHz + subdivisiones)
    let grid_freqs = [
        20.0_f32, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0, 200.0, 300.0, 400.0, 500.0,
        600.0, 700.0, 800.0, 900.0, 1000.0, 2000.0, 3000.0, 4000.0,
    ];
    for gf in grid_freqs {
        let gx = freq_to_xy_x(gf, xy_rect);
        let major = (gf - 20.0).abs() < 0.1
            || (gf - 100.0).abs() < 0.1
            || (gf - 1000.0).abs() < 0.1;
        let col = if major {
            Color32::from_rgb(0x32, 0x36, 0x40)
        } else {
            Color32::from_rgb(0x21, 0x24, 0x2B)
        };
        xy_painter.line_segment(
            [pos2(gx, xy_rect.min.y + 2.0), pos2(gx, xy_rect.max.y - 2.0)],
            Stroke::new(1.0_f32, col),
        );
    }

    // Líneas horizontales (0 dB al centro y ±50% FB)
    for frac in [0.25_f32, 0.5, 0.75] {
        let gy = xy_rect.min.y + frac * xy_rect.height();
        let col = if (frac - 0.5).abs() < 0.01 {
            Color32::from_rgb(0x34, 0x38, 0x42)
        } else {
            Color32::from_rgb(0x22, 0x25, 0x2C)
        };
        xy_painter.line_segment(
            [pos2(xy_rect.min.x + 2.0, gy), pos2(xy_rect.max.x - 2.0, gy)],
            Stroke::new(1.0_f32, col),
        );
    }

    // Etiquetas superiores de frecuencia
    for (gf, lbl) in [(20.0_f32, "20 Hz"), (100.0, "100 Hz"), (1000.0, "1 kHz")] {
        let gx = freq_to_xy_x(gf, xy_rect);
        xy_painter.text(
            pos2(gx + 3.0, xy_rect.min.y + 3.0),
            Align2::LEFT_TOP,
            lbl,
            fonts::regular(8.2),
            COL_GRID_TEXT,
        );
    }

    // Curva analítica del peine
    let mut dsp = OpenCombFilter::new(48_000.0);
    dsp.set_params(*params);
    let bins = 112;
    let curve = dsp.frequency_response_curve(bins);
    let inner_h = (xy_rect.height() - 8.0).max(1.0);
    let points: Vec<egui::Pos2> = curve
        .iter()
        .enumerate()
        .map(|(i, &mag)| {
            let t = i as f32 / (bins - 1) as f32;
            let f = 20.0_f32 * (4900.0_f32 / 20.0_f32).powf(t);
            let x = freq_to_xy_x(f, xy_rect);
            let y = xy_rect.max.y - 4.0 - mag * inner_h;
            pos2(x, y)
        })
        .collect();

    let fill_col = Color32::from_rgba_unmultiplied(0xFF, 0xFF, 0xFF, 20);
    for w in points.windows(2) {
        let p0 = w[0];
        let p1 = w[1];
        xy_painter.add(egui::Shape::convex_polygon(
            vec![
                p0,
                p1,
                pos2(p1.x, xy_rect.max.y - 3.0),
                pos2(p0.x, xy_rect.max.y - 3.0),
            ],
            fill_col,
            Stroke::NONE,
        ));
    }
    xy_painter.add(egui::Shape::line(
        points,
        Stroke::new(1.3_f32, Color32::from_rgb(0xE6, 0xE8, 0xEC)),
    ));

    // Interacción del Pad XY
    let xy_resp = ui.interact(
        xy_rect,
        ui.id().with((id_salt, "opencomb_xy")),
        Sense::click_and_drag(),
    );
    if xy_resp.hovered() || xy_resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
    }
    if xy_resp.double_clicked() {
        params.freq_hz = OPENCOMB_DEFAULT_FREQ_HZ;
        params.feedback_pct = 0.0;
        changed = true;
    } else if xy_resp.dragged() || xy_resp.clicked() {
        if let Some(pos) = xy_resp.interact_pointer_pos() {
            params.freq_hz = xy_x_to_freq(pos.x, xy_rect);
            let fb_t =
                1.0 - ((pos.y - (xy_rect.min.y + 6.0)) / (xy_rect.height() - 12.0).max(1.0)).clamp(0.0, 1.0);
            params.feedback_pct = (fb_t * 200.0 - 100.0).clamp(-100.0, 100.0);
            changed = true;
        }
    }

    // Punto naranja XY
    let dot_x = freq_to_xy_x(params.freq_hz, xy_rect);
    let fb_norm = OpenCombParams::feedback_pct_to_norm(params.feedback_pct);
    let dot_y = xy_rect.max.y - 6.0 - fb_norm * (xy_rect.height() - 12.0).max(1.0);
    xy_painter.circle_filled(pos2(dot_x, dot_y), 4.2, COMB_ORANGE);
    xy_painter.circle_stroke(
        pos2(dot_x, dot_y),
        4.2,
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(0, 0, 0, 160)),
    );

    if xy_resp.hovered() || xy_resp.dragged() {
        let badge_rect = Rect::from_min_size(
            pos2(xy_rect.max.x - 58.0, xy_rect.min.y + 3.0),
            vec2(55.0, 24.0),
        );
        xy_painter.rect_filled(
            badge_rect,
            CornerRadius::same(2),
            Color32::from_rgba_unmultiplied(0x0D, 0x0E, 0x12, 215),
        );
        xy_painter.text(
            pos2(badge_rect.max.x - 3.0, badge_rect.min.y + 2.0),
            Align2::RIGHT_TOP,
            format_freq_label(params.freq_hz),
            fonts::regular(8.5),
            Color32::WHITE,
        );
        xy_painter.text(
            pos2(badge_rect.max.x - 3.0, badge_rect.min.y + 12.0),
            Align2::RIGHT_TOP,
            format!("{:.0}%", params.feedback_pct),
            fonts::regular(8.5),
            COL_LABEL,
        );
    }

    // =========================================================================
    // 2. Caja inferior con 3 perillas: Freq | FB | Mix
    // =========================================================================
    let well_rect = Rect::from_min_max(
        pos2(panel_rect.min.x + pad_x, xy_rect.max.y + 4.0),
        pos2(panel_rect.max.x - pad_x, panel_rect.max.y - 5.0),
    );
    bitwig_kit::paint_lemon_plug_well(ui.painter(), well_rect, 4);

    let knob_diam = (well_rect.height() * 0.56).clamp(26.0, 36.0);
    let knob_top = well_rect.min.y + ((well_rect.height() - knob_diam - 14.0) * 0.5).max(2.0);
    let lbl_font = (pw * 0.052).clamp(8.4, 9.6);
    let col_w = well_rect.width() / 3.0;

    let freq_cx = well_rect.min.x + col_w * 0.5;
    let fb_cx = well_rect.min.x + col_w * 1.5;
    let mix_cx = well_rect.min.x + col_w * 2.5;

    let mut freq_norm = OpenCombParams::freq_hz_to_norm(params.freq_hz);
    let freq_lbl = format_freq_label(params.freq_hz);
    let freq_tip = format!(
        "Frequency: {} ({})",
        freq_lbl,
        freq_hz_to_note_label(params.freq_hz)
    );
    if draw_comb_knob(
        ui,
        freq_cx,
        knob_top,
        knob_diam,
        &mut freq_norm,
        OpenCombParams::freq_hz_to_norm(OPENCOMB_DEFAULT_FREQ_HZ),
        false,
        &freq_lbl,
        lbl_font,
        &freq_tip,
    ) {
        params.freq_hz = OpenCombParams::norm_to_freq_hz(freq_norm);
        changed = true;
    }

    let mut fb_norm = OpenCombParams::feedback_pct_to_norm(params.feedback_pct);
    let fb_tip = format!("Feedback: {:.1}%", params.feedback_pct);
    if draw_comb_knob(
        ui,
        fb_cx,
        knob_top,
        knob_diam,
        &mut fb_norm,
        0.5,
        true,
        "FB",
        lbl_font,
        &fb_tip,
    ) {
        params.feedback_pct = OpenCombParams::norm_to_feedback_pct(fb_norm);
        changed = true;
    }

    let mut mix_norm = OpenCombParams::mix_pct_to_norm(params.mix_pct);
    let mix_tip = format!("Mix: {:.1}%", params.mix_pct);
    if draw_comb_knob(
        ui,
        mix_cx,
        knob_top,
        knob_diam,
        &mut mix_norm,
        1.0,
        false,
        "Mix",
        lbl_font,
        &mix_tip,
    ) {
        params.mix_pct = OpenCombParams::norm_to_mix_pct(mix_norm);
        changed = true;
    }

    changed
}

pub fn render_module(ui: &mut egui::Ui, app: &mut AppState, track_idx: usize, slot_idx: usize) {
    let mut params = {
        let mut map = state_map().lock().unwrap();
        *map.entry((track_idx, slot_idx))
            .or_insert_with(OpenCombParams::default)
    };
    let engine_track_idx = track_idx.checked_sub(1);
    if let Some(et_idx) = engine_track_idx {
        if loaded_set().lock().unwrap().insert((track_idx, slot_idx)) {
            app.audio_proxy
                .send(crate::audio_proxy::GuiCommand::LoadNativeEffect {
                    track_idx: et_idx,
                    slot_idx,
                    name: "Hikaru OpenComb".to_string(),
                });
            push_all_params(app, et_idx, slot_idx, &params);
        }
    }

    if render_opencomb_surface(ui, &mut params, ("oc", track_idx, slot_idx)) {
        state_map()
            .lock()
            .unwrap()
            .insert((track_idx, slot_idx), params);
        if let Some(et_idx) = engine_track_idx {
            push_all_params(app, et_idx, slot_idx, &params);
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
    let mut params = {
        let mut map = wet_state_map().lock().unwrap();
        *map.entry(key).or_insert_with(OpenCombParams::default)
    };

    if render_opencomb_surface(ui, &mut params, ("oc_wet", track_idx, parent_slot_idx, wet_idx)) {
        wet_state_map().lock().unwrap().insert(key, params);
        if let Some(et_idx) = track_idx.checked_sub(1) {
            push_all_wet_params(app, et_idx, parent_slot_idx, wet_idx, &params);
        }
        app.project_dirty = true;
    }
}
