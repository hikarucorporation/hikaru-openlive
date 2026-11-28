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

//! **Hikaru OpenFlanger+**: panel nativo 1-a-1 inspirado en **Flanger+** de Bitwig Studio
//! para `hikaru_dsp::effects::FlangerPlusEffect`.
//!
//! Geometría y controles (paleta *Pith* `#2A2A2D` + Verde Esmeralda Flanger `#18D88E`):
//! - **Barra superior de carácter** (`[ DP | MX | TFX | WA ]`) sobre pozo oscuro `LEMON_PLUG_WELL`.
//! - **Fila 1 (perillas principales `42 px`)**: **`Color`** y **`FB`**.
//! - **Fila 2 (perillas de modulación `34 px`)**: **`Speed`** (con menú contextual `Clic derecho`
//!   para alternar entre `Hz` libre y sincronía rítmica `1/16 .. 8/1`) y **`Depth`**.
//! - **Fila 3 (salida e imagen)**: botón hundido **`[ ∅ ]`** (*Polarity Invert*), perilla central
//!   **`Mix`** (`42 px`) y botón hundido **`[ ⚭ ]`** (*Stereo Phase Offset*).

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use egui::{pos2, vec2, Align2, Color32, CornerRadius, CursorIcon, Rect, Sense, Stroke};
use hikaru_dsp::effects::{
    FlangerPlusMode, FlangerPlusParams, FLANGER_PLUS_PARAM_COUNT, FLANGER_SYNC_LABELS,
    FLP_PARAM_COLOR, FLP_PARAM_DEPTH, FLP_PARAM_FEEDBACK, FLP_PARAM_MIX, FLP_PARAM_SPEED,
    FLP_PARAM_SYNC_DIV,
};

use crate::app::AppState;
use crate::theme;
use crate::ui::bitwig_kit;
use crate::ui::fonts;
use crate::ui::icons;

pub const OPEN_FLANGER_PLUS_CARD_WIDTH: f32 = 178.0;
pub const OPEN_FLANGER_PLUS_WET_CARD_WIDTH: f32 = 166.0;

/// Verde esmeralda/turquesa calibrado contra la captura de referencia de Flanger+ (`#18D88E`).
pub const FLANGER_TEAL: Color32 = Color32::from_rgb(0x18, 0xD8, 0x8E);
const COL_LABEL: Color32 = Color32::from_rgb(0xCA, 0xCD, 0xD4);
const COL_MODE_IDLE: Color32 = Color32::from_rgb(0x9E, 0xA2, 0xAA);

pub fn is_openflanger_plus_name(name: &str) -> bool {
    matches!(
        name,
        "Hikaru OpenFlanger+"
            | "Hikaru OpenFlanger"
            | "OpenFlanger+"
            | "OpenFlanger"
            | "Flanger+"
            | "Hikaru Flanger+"
    )
}

type SlotStateMap = Mutex<HashMap<(usize, usize), FlangerPlusParams>>;
type LoadedSet = Mutex<HashSet<(usize, usize)>>;
type WetStateMap = Mutex<HashMap<(usize, usize, usize), FlangerPlusParams>>;

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
    let existing: Vec<((usize, usize, usize), FlangerPlusParams)> = map
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

pub struct OpenFlangerPreset {
    pub name: &'static str,
    pub params: FlangerPlusParams,
}

pub fn factory_presets() -> Vec<OpenFlangerPreset> {
    vec![
        OpenFlangerPreset {
            name: "Default (TFX Sparkly)",
            params: FlangerPlusParams::default(),
        },
        OpenFlangerPreset {
            name: "Jet Plane Sweep (DP)",
            params: FlangerPlusParams {
                mode: FlangerPlusMode::Dp,
                color: 0.62,
                feedback: 0.84,
                speed_norm: FlangerPlusParams::speed_hz_to_norm(0.24),
                depth: 0.82,
                phase_invert: false,
                stereo: true,
                mix: 0.50,
                ..FlangerPlusParams::default()
            },
        },
        OpenFlangerPreset {
            name: "Classic BBD Pedal (MX)",
            params: FlangerPlusParams {
                mode: FlangerPlusMode::Mx,
                color: 0.45,
                feedback: 0.64,
                speed_norm: FlangerPlusParams::speed_hz_to_norm(0.42),
                depth: 0.60,
                phase_invert: false,
                stereo: false,
                mix: 0.50,
                ..FlangerPlusParams::default()
            },
        },
        OpenFlangerPreset {
            name: "Liquid Stereo Chorus (TFX)",
            params: FlangerPlusParams {
                mode: FlangerPlusMode::Tfx,
                color: 0.28,
                feedback: 0.22,
                speed_norm: FlangerPlusParams::speed_hz_to_norm(0.65),
                depth: 0.48,
                phase_invert: false,
                stereo: true,
                mix: 0.48,
                ..FlangerPlusParams::default()
            },
        },
        OpenFlangerPreset {
            name: "Wide Analog Tape (WA)",
            params: FlangerPlusParams {
                mode: FlangerPlusMode::Wa,
                color: 0.52,
                feedback: 0.58,
                speed_norm: FlangerPlusParams::speed_hz_to_norm(0.18),
                depth: 0.72,
                phase_invert: false,
                stereo: true,
                mix: 0.50,
                ..FlangerPlusParams::default()
            },
        },
        OpenFlangerPreset {
            name: "Hollow Metallic Comb (∅)",
            params: FlangerPlusParams {
                mode: FlangerPlusMode::Dp,
                color: 0.74,
                feedback: 0.78,
                speed_norm: FlangerPlusParams::speed_hz_to_norm(0.35),
                depth: 0.52,
                phase_invert: true,
                stereo: true,
                mix: 0.50,
                ..FlangerPlusParams::default()
            },
        },
        OpenFlangerPreset {
            name: "Fast Flutter Vibrato",
            params: FlangerPlusParams {
                mode: FlangerPlusMode::Tfx,
                color: 0.58,
                feedback: 0.30,
                speed_norm: FlangerPlusParams::speed_hz_to_norm(5.80),
                depth: 0.38,
                phase_invert: false,
                stereo: true,
                mix: 0.55,
                ..FlangerPlusParams::default()
            },
        },
        OpenFlangerPreset {
            name: "Tempo-Synced 2-Bar Jet",
            params: FlangerPlusParams {
                mode: FlangerPlusMode::Wa,
                color: 0.48,
                feedback: 0.80,
                speed_norm: 0.50,
                depth: 0.85,
                phase_invert: true,
                stereo: true,
                mix: 0.50,
                tempo_sync: true,
                sync_div: 7, // 2/1
            },
        },
    ]
}

fn push_all_params(
    app: &mut AppState,
    engine_track_idx: usize,
    slot_idx: usize,
    params: &FlangerPlusParams,
) {
    for id in 0..(FLANGER_PLUS_PARAM_COUNT as u32) {
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
    params: &FlangerPlusParams,
) {
    for id in 0..(FLANGER_PLUS_PARAM_COUNT as u32) {
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
        egui::RichText::new("Hikaru OpenFlanger+ Presets")
            .font(fonts::semibold(10.0))
            .color(FLANGER_TEAL),
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
fn draw_flanger_knob(
    ui: &mut egui::Ui,
    center_x: f32,
    top_y: f32,
    diameter: f32,
    norm: &mut f32,
    default_norm: f32,
    label: &str,
    label_font_sz: f32,
    tooltip: &str,
    mod_slot: Option<(usize, usize)>,
    param_id: u32,
) -> (egui::Response, bool) {
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
    let (resp, changed) = if let Some((track_idx, slot_idx)) = mod_slot {
        crate::views::hikaru_native_plugins::hikaru_openmodulation::modulatable_slot_knob(
            &mut knob_ui,
            diameter,
            &mut pct,
            0.0..=100.0,
            default_norm.clamp(0.0, 1.0) * 100.0,
            FLANGER_TEAL,
            false,
            track_idx,
            slot_idx,
            param_id,
            label,
        )
    } else {
        bitwig_kit::rotary_knob(
            &mut knob_ui,
            diameter,
            &mut pct,
            0.0..=100.0,
            default_norm.clamp(0.0, 1.0) * 100.0,
            FLANGER_TEAL,
            false,
        )
    };
    if changed {
        *norm = (pct / 100.0).clamp(0.0, 1.0);
    }
    ui.painter().text(
        pos2(center_x, knob_rect.max.y + 7.5),
        Align2::CENTER_CENTER,
        label,
        fonts::regular(label_font_sz),
        COL_LABEL,
    );
    let resp = resp.on_hover_text(tooltip);
    (resp, changed)
}

fn render_flanger_plus_surface(
    ui: &mut egui::Ui,
    params: &mut FlangerPlusParams,
    tempo_bpm: f64,
    mod_slot: Option<(usize, usize)>,
    id_salt: impl std::hash::Hash + Copy,
) -> bool {
    let full_rect = ui.available_rect_before_wrap();
    let (_, _) = ui.allocate_exact_size(full_rect.size(), Sense::hover());

    let mut changed = false;
    let def = FlangerPlusParams::default();

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
    let v_scale = (ph / 224.0).clamp(0.74, 1.06);

    // =========================================================================
    // 1. Tira superior segmentada `[ DP | MX | TFX | WA ]`
    // =========================================================================
    let mode_h = (21.0 * v_scale).clamp(17.0, 22.0);
    let mode_pad_x = 8.0_f32;
    let mode_top = panel_rect.min.y + (7.0 * v_scale).max(4.5);
    let mode_rect = Rect::from_min_size(
        pos2(panel_rect.min.x + mode_pad_x, mode_top),
        vec2((pw - mode_pad_x * 2.0).max(96.0), mode_h),
    );
    bitwig_kit::paint_lemon_plug_well(ui.painter(), mode_rect, 4);

    let seg_w = mode_rect.width() / 4.0;
    for (idx, &mode) in FlangerPlusMode::ALL.iter().enumerate() {
        let seg_rect = Rect::from_min_size(
            pos2(mode_rect.min.x + idx as f32 * seg_w, mode_rect.min.y),
            vec2(seg_w, mode_h),
        );
        let seg_resp = ui
            .interact(
                seg_rect,
                ui.id().with((id_salt, "flp_mode", idx)),
                Sense::click(),
            )
            .on_hover_text(mode.description());
        if seg_resp.clicked() && params.mode != mode {
            params.mode = mode;
            changed = true;
        }
        if seg_resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }

        let active = params.mode == mode;
        if active {
            ui.painter().rect_filled(
                seg_rect.shrink(1.5),
                CornerRadius::same(3),
                Color32::from_rgba_unmultiplied(0x18, 0xD8, 0x8E, 18),
            );
        } else if seg_resp.hovered() {
            ui.painter().rect_filled(
                seg_rect.shrink(1.5),
                CornerRadius::same(3),
                Color32::from_rgba_unmultiplied(255, 255, 255, 10),
            );
        }

        let text_col = if active {
            FLANGER_TEAL
        } else if seg_resp.hovered() {
            Color32::WHITE
        } else {
            COL_MODE_IDLE
        };
        ui.painter().text(
            seg_rect.center(),
            Align2::CENTER_CENTER,
            mode.label(),
            fonts::semibold((10.5 * v_scale).clamp(9.2, 11.0)),
            text_col,
        );
    }

    // =========================================================================
    // 2. Geometría de 3 filas: (Color, FB) -> (Speed, Depth) -> ([∅], Mix, [⚭])
    // =========================================================================
    let hero_diam = (42.0 * v_scale).clamp(32.0, 44.0);
    let mid_diam = (34.0 * v_scale).clamp(26.0, 36.0);
    let lbl_font = (10.0 * v_scale).clamp(8.8, 10.4);

    let left_cx = panel_rect.min.x + pw * 0.285;
    let right_cx = panel_rect.min.x + pw * 0.715;
    let center_cx = panel_rect.center().x;

    let body_top = mode_rect.max.y + (7.0 * v_scale).max(4.0);
    let body_avail = (panel_rect.max.y - body_top - 4.0).max(115.0);

    let row1_top = body_top;
    let row2_top = body_top + body_avail * 0.345;
    let row3_top = body_top + body_avail * 0.655;

    // Fila 1: Color (izq) + FB (der)
    let color_tip = format!(
        "Color: {:.1}% ({} base delay — Shift+Drag fine, Double-click reset)",
        params.color * 100.0,
        params.format_color_readout()
    );
    let (_, c_chg) = draw_flanger_knob(
        ui,
        left_cx,
        row1_top,
        hero_diam,
        &mut params.color,
        def.color,
        "Color",
        lbl_font,
        &color_tip,
        mod_slot,
        FLP_PARAM_COLOR,
    );
    changed |= c_chg;

    let fb_tip = format!("Feedback (FB): {:.1}%", params.feedback * 100.0);
    let (_, fb_chg) = draw_flanger_knob(
        ui,
        right_cx,
        row1_top,
        hero_diam,
        &mut params.feedback,
        def.feedback,
        "FB",
        lbl_font,
        &fb_tip,
        mod_slot,
        FLP_PARAM_FEEDBACK,
    );
    changed |= fb_chg;

    // Fila 2: Speed (izq) + Depth (der)
    let speed_lbl = if params.tempo_sync { "Speed ♩" } else { "Speed" };
    let speed_tip = format!(
        "LFO Speed: {} (Right-click for Hz / Beat Sync)",
        params.format_speed_readout(tempo_bpm)
    );
    let speed_resp = if params.tempo_sync {
        let max_idx = (FLANGER_SYNC_LABELS.len() - 1) as f32;
        let mut div_norm = (params.sync_div as f32 / max_idx).clamp(0.0, 1.0);
        let (resp, s_chg) = draw_flanger_knob(
            ui,
            left_cx,
            row2_top,
            mid_diam,
            &mut div_norm,
            def.sync_div as f32 / max_idx,
            speed_lbl,
            lbl_font,
            &speed_tip,
            mod_slot,
            FLP_PARAM_SYNC_DIV,
        );
        if s_chg {
            params.sync_div = (div_norm * max_idx).round() as u8;
            changed = true;
        }
        resp
    } else {
        let (resp, s_chg) = draw_flanger_knob(
            ui,
            left_cx,
            row2_top,
            mid_diam,
            &mut params.speed_norm,
            def.speed_norm,
            speed_lbl,
            lbl_font,
            &speed_tip,
            mod_slot,
            FLP_PARAM_SPEED,
        );
        changed |= s_chg;
        resp
    };

    speed_resp.context_menu(|ui| {
        ui.set_min_width(136.0);
        ui.label(
            egui::RichText::new("LFO Speed Mode")
                .font(fonts::semibold(9.8))
                .color(FLANGER_TEAL),
        );
        ui.separator();
        if ui
            .selectable_label(!params.tempo_sync, "Free Rate (Hz)")
            .clicked()
        {
            params.tempo_sync = false;
            changed = true;
            ui.close_menu();
        }
        if ui
            .selectable_label(params.tempo_sync, "Tempo Sync (Beats)")
            .clicked()
        {
            params.tempo_sync = true;
            changed = true;
            ui.close_menu();
        }
        if params.tempo_sync {
            ui.separator();
            for (idx, &lbl) in FLANGER_SYNC_LABELS.iter().enumerate() {
                let sel = params.sync_div as usize == idx;
                if ui.selectable_label(sel, lbl).clicked() {
                    params.sync_div = idx as u8;
                    changed = true;
                    ui.close_menu();
                }
            }
        }
    });

    let depth_tip = format!("Modulation Depth: {:.1}%", params.depth * 100.0);
    let (_, d_chg) = draw_flanger_knob(
        ui,
        right_cx,
        row2_top,
        mid_diam,
        &mut params.depth,
        def.depth,
        "Depth",
        lbl_font,
        &depth_tip,
        mod_slot,
        FLP_PARAM_DEPTH,
    );
    changed |= d_chg;

    // Fila 3: [ ∅ ] (izq) + Mix (centro) + [ ⚭ ] (der)
    let mix_tip = format!("Dry/Wet Mix: {:.1}%", params.mix * 100.0);
    let (_, m_chg) = draw_flanger_knob(
        ui,
        center_cx,
        row3_top,
        hero_diam,
        &mut params.mix,
        def.mix,
        "Mix",
        lbl_font,
        &mix_tip,
        mod_slot,
        FLP_PARAM_MIX,
    );
    changed |= m_chg;

    let btn_w = (27.0 * v_scale).clamp(22.0, 29.0);
    let btn_h = (20.0 * v_scale).clamp(16.0, 21.0);
    let btn_cy = row3_top + hero_diam * 0.80;
    let side_margin = (pw * 0.165).clamp(19.0, 26.0);

    // Botón izquierdo `[ ∅ ]`: Polarity / Phase Invert
    let inv_rect = Rect::from_center_size(
        pos2(panel_rect.min.x + side_margin, btn_cy),
        vec2(btn_w, btn_h),
    );
    let inv_resp = bitwig_kit::lemon_sunken_icon_toggle(
        ui,
        inv_rect,
        ui.id().with((id_salt, "flp_phase_inv")),
        params.phase_invert,
        FLANGER_TEAL,
        icons::paint_phase_invert_symbol,
    )
    .on_hover_text(if params.phase_invert {
        "Polarity Invert (∅): ON (inverted comb notches & negative feedback)"
    } else {
        "Polarity Invert (∅): OFF (normal additive flanger comb)"
    });
    if inv_resp.clicked() {
        params.phase_invert = !params.phase_invert;
        changed = true;
    }

    // Botón derecho `[ ⚭ ]`: Stereo Phase Offset
    let st_rect = Rect::from_center_size(
        pos2(panel_rect.max.x - side_margin, btn_cy),
        vec2(btn_w, btn_h),
    );
    let st_resp = bitwig_kit::lemon_sunken_icon_toggle(
        ui,
        st_rect,
        ui.id().with((id_salt, "flp_stereo")),
        params.stereo,
        FLANGER_TEAL,
        icons::paint_stereo_rings_symbol,
    )
    .on_hover_text(if params.stereo {
        "Stereo Phase Offset (⚭): ON (wide L/R offset sweep)"
    } else {
        "Stereo Phase Offset (⚭): OFF (mono-coherent L/R sweep)"
    });
    if st_resp.clicked() {
        params.stereo = !params.stereo;
        changed = true;
    }

    changed
}

pub fn render_module(ui: &mut egui::Ui, app: &mut AppState, track_idx: usize, slot_idx: usize) {
    let key = (track_idx, slot_idx);
    let engine_track_idx = track_idx.checked_sub(1);
    let tempo_bpm = app.transport.bpm as f64;

    let mut params = {
        let mut map = state_map().lock().unwrap();
        *map.entry(key).or_default()
    };

    if let Some(et_idx) = engine_track_idx {
        if loaded_set().lock().unwrap().insert(key) {
            app.audio_proxy
                .send(crate::audio_proxy::GuiCommand::LoadNativeEffect {
                    track_idx: et_idx,
                    slot_idx,
                    name: "Hikaru OpenFlanger+".to_string(),
                });
            push_all_params(app, et_idx, slot_idx, &params);
        }
    }

    if render_flanger_plus_surface(
        ui,
        &mut params,
        tempo_bpm,
        Some((track_idx, slot_idx)),
        ("flp_slot", track_idx, slot_idx),
    ) {
        state_map().lock().unwrap().insert(key, params);
        app.project_dirty = true;
    }
    crate::views::hikaru_native_plugins::hikaru_openmodulation::sync_slot_params_to_engine(
        ui,
        app,
        track_idx,
        slot_idx,
        FLANGER_PLUS_PARAM_COUNT as u32,
        |id| params.get_normalized(id),
    );
}

pub fn render_wet_fx_module(
    ui: &mut egui::Ui,
    app: &mut AppState,
    track_idx: usize,
    parent_slot_idx: usize,
    wet_idx: usize,
) {
    let key = (track_idx, parent_slot_idx, wet_idx);
    let tempo_bpm = app.transport.bpm as f64;
    let mut params = {
        let mut map = wet_state_map().lock().unwrap();
        *map.entry(key).or_default()
    };

    if render_flanger_plus_surface(
        ui,
        &mut params,
        tempo_bpm,
        None,
        ("flp_wet", track_idx, parent_slot_idx, wet_idx),
    ) {
        wet_state_map().lock().unwrap().insert(key, params);
        if let Some(et_idx) = track_idx.checked_sub(1) {
            push_all_wet_params(app, et_idx, parent_slot_idx, wet_idx, &params);
        }
        app.project_dirty = true;
    }
}
