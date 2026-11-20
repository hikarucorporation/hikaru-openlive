/*
 * Hikaru OpenPoly - Polymer-Inspired Modular Synth UI (GNU AGPL v3)
 * Copyright (C) 2026 Hikaru Corporation
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 */

use egui::{pos2, vec2, Align2, Color32, CursorIcon, Pos2, Rect, RichText, Sense, Stroke, Ui};
use hikaru_dsp::synth::open_poly::{
    eval_osc_preview, OpenPolyEnvType, OpenPolyFilterType, OpenPolyOscType, OpenPolySubWave,
};

use crate::app::AppState;
use crate::theme;
use crate::ui::bitwig_kit::rotary_knob;
use crate::ui::fonts;
use crate::views::hikaru_native_plugins::hikaru_openmodulation;

use super::open_poly::{
    load_openpoly_preset, save_openpoly_preset, HikaruOpenPoly, FACTORY_PRESETS,
};

/// Full width of Hikaru OpenPoly card in DSP Rack (including 22px left sidebar & Modulators strip).
pub const OPEN_POLY_CARD_WIDTH: f32 = 678.0;
/// Width when the left Modulators drawer is collapsed.
pub const OPEN_POLY_COMPACT_CARD_WIDTH: f32 = 612.0;

// Section accent palette (color-coded modular blocks)
const PANEL_BG: Color32 = Color32::from_rgb(0x24, 0x26, 0x2B);
const PANEL_BG_DARK: Color32 = Color32::from_rgb(0x16, 0x18, 0x1C);
const PANEL_BORDER: Color32 = Color32::from_rgb(0x3B, 0x3E, 0x47);
const OSC_AMBER: Color32 = Color32::from_rgb(0xE8, 0xA8, 0x24);
const SUB_PURPLE: Color32 = Color32::from_rgb(0x9C, 0x6E, 0xFF);
const FILTER_CYAN: Color32 = Color32::from_rgb(0x2B, 0xB8, 0xC8);
const FEG_GREEN: Color32 = Color32::from_rgb(0x30, 0xC8, 0x74);
const AMP_BLUE: Color32 = Color32::from_rgb(0x50, 0xA8, 0xFF);
const VOICE_CORAL: Color32 = Color32::from_rgb(0xEB, 0x4E, 0x52);
const MOD_ORANGE: Color32 = Color32::from_rgb(0xFF, 0x82, 0x20);
const KNOB_WHITE: Color32 = Color32::from_rgb(0xE6, 0xE8, 0xEC);

#[inline]
fn hz_to_log_norm(hz: f32) -> f32 {
    let min_l = 20.0_f32.ln();
    let max_l = 20000.0_f32.ln();
    ((hz.clamp(20.0, 20000.0).ln() - min_l) / (max_l - min_l)).clamp(0.0, 1.0)
}

#[inline]
fn log_norm_to_hz(norm: f32) -> f32 {
    let min_l = 20.0_f32.ln();
    let max_l = 20000.0_f32.ln();
    (min_l + norm.clamp(0.0, 1.0) * (max_l - min_l)).exp()
}

#[inline]
fn ms_to_log_norm(ms: f32, min_ms: f32, max_ms: f32) -> f32 {
    let lo = min_ms.max(0.1).ln();
    let hi = max_ms.max(min_ms + 0.1).ln();
    ((ms.clamp(min_ms, max_ms).ln() - lo) / (hi - lo)).clamp(0.0, 1.0)
}

#[inline]
fn log_norm_to_ms(norm: f32, min_ms: f32, max_ms: f32) -> f32 {
    let lo = min_ms.max(0.1).ln();
    let hi = max_ms.max(min_ms + 0.1).ln();
    (lo + norm.clamp(0.0, 1.0) * (hi - lo)).exp()
}

fn format_hz(hz: f32) -> String {
    if hz >= 1000.0 {
        format!("{:.2} kHz", hz / 1000.0)
    } else {
        format!("{:.0} Hz", hz)
    }
}

fn format_ms(ms: f32) -> String {
    if ms >= 1000.0 {
        format!("{:.2} s", ms / 1000.0)
    } else if ms >= 10.0 {
        format!("{:.0} ms", ms)
    } else {
        format!("{:.1} ms", ms)
    }
}

fn paint_panel(ui: &Ui, rect: Rect, border: Color32) {
    let p = ui.painter().clone();
    p.rect_filled(rect, 3.0, PANEL_BG);
    p.rect_stroke(rect, 3.0, Stroke::new(1.0_f32, border), egui::StrokeKind::Inside);
}

/// Renders the preset & WAV file popup triggered from the device sidebar folder button.
pub fn render_preset_menu(ui: &mut Ui, app: &mut AppState, track_idx: usize, slot_idx: usize) {
    ui.set_min_width(185.0);
    ui.label(
        RichText::new("Hikaru OpenPoly")
            .font(fonts::semibold(11.0))
            .color(theme::ACCENT_ORANGE),
    );
    ui.label(
        RichText::new("AGPL-v3 Hybrid Modular Synth")
            .font(fonts::regular(9.0))
            .color(theme::TEXT_MUTED),
    );
    ui.separator();
    ui.label(
        RichText::new("Factory Presets")
            .font(fonts::semibold(9.5))
            .color(theme::TEXT_SECONDARY),
    );
    for preset in &FACTORY_PRESETS {
        let label = format!("{}  [{}]", preset.name, preset.category);
        if ui.button(label).clicked() {
            if let Some(track) = app.current_tracks_mut().get_mut(track_idx) {
                if let Some(slot) = track.effects.get_mut(slot_idx) {
                    slot.open_poly_mut().apply_factory_preset(preset.name);
                }
            }
            ui.close_menu();
        }
    }
    ui.separator();
    if ui.button("Load Wavetable (.wav) from Explorer").clicked() {
        crate::views::explorer::begin_wavetable_pick(app, track_idx, slot_idx);
        ui.close_menu();
    }
    if ui.button("Save Preset (*.hdp)...").clicked() {
        if let Some(track) = app.current_tracks().get(track_idx) {
            if let Some(slot) = track.effects.get(slot_idx) {
                if let Some(ref poly) = slot.open_poly {
                    save_openpoly_preset(poly);
                }
            }
        }
        ui.close_menu();
    }
    if ui.button("Load Preset (*.hdp)...").clicked() {
        if let Some(track) = app.current_tracks_mut().get_mut(track_idx) {
            if let Some(slot) = track.effects.get_mut(slot_idx) {
                load_openpoly_preset(slot.open_poly_mut());
            }
        }
        ui.close_menu();
    }
}

/// Main body renderer for Hikaru OpenPoly inside the DSP Rack card.
pub fn render_module(ui: &mut Ui, app: &mut AppState, track_idx: usize, slot_idx: usize) {
    // Query live stereo output meter from audio engine (non-blocking try_lock)
    let eng_track = track_idx.saturating_sub(1);
    let (meter_l, meter_r) = app
        .engine_handle
        .as_ref()
        .and_then(|arc| arc.try_lock().ok())
        .map(|eng| eng.open_poly_meter(eng_track))
        .unwrap_or((0.0, 0.0));

    // Read external OpenModulation deltas for visual rings
    let mod_index = hikaru_openmodulation::get_modulation(track_idx, "OpenPoly", "INDEX")
        .or_else(|| hikaru_openmodulation::get_modulation(track_idx, "OpenWavetable", "WT POS"));
    let mod_cutoff = hikaru_openmodulation::get_modulation(track_idx, "OpenPoly", "CUTOFF");
    let mod_vol = hikaru_openmodulation::get_modulation(track_idx, "OpenPoly", "VOL")
        .or_else(|| hikaru_openmodulation::get_modulation(track_idx, "OpenWavetable", "VOL"));

    let mut poly = {
        let Some(track) = app.current_tracks_mut().get_mut(track_idx) else {
            return;
        };
        let Some(slot) = track.effects.get_mut(slot_idx) else {
            return;
        };
        slot.open_poly_mut().clone()
    };

    let mut dirty = false;
    let mut request_wav_browser = false;
    let mut request_add_modulator = false;
    let mut request_insert_device = false;

    let avail = ui.available_rect_before_wrap();
    let total_h = avail.height().max(215.0);
    let top_h = (total_h * 0.645).round().clamp(132.0, 148.0);
    let gap = 4.0;
    let bot_h = (total_h - top_h - gap).max(66.0);

    let mut cursor_x = avail.min.x;
    let y0 = avail.min.y;

    // =========================================================================
    // COLUMN 0 (optional): Reusable Bitwig-style Device Modulator Bay
    // =========================================================================
    if poly.modulators_open {
        let bay_w = hikaru_openmodulation::render_device_modulator_bay(
            ui,
            pos2(cursor_x, y0),
            total_h,
            track_idx,
            slot_idx,
            &mut request_add_modulator,
        );
        cursor_x += bay_w;
    }

    // =========================================================================
    // COLUMN 1: Oscillator (top) + Sub & Noise (bottom)
    // =========================================================================
    let osc_w = 186.0;
    let osc_rect = Rect::from_min_size(pos2(cursor_x, y0), vec2(osc_w, top_h));
    let sub_w = 122.0;
    let sub_rect = Rect::from_min_size(pos2(cursor_x, y0 + top_h + gap), vec2(sub_w, bot_h));
    let noise_rect = Rect::from_min_size(
        pos2(cursor_x + sub_w + gap, y0 + top_h + gap),
        vec2(osc_w - sub_w - gap, bot_h),
    );

    render_oscillator_section(
        ui,
        osc_rect,
        &mut poly,
        mod_index.map(|m| m.delta),
        track_idx,
        slot_idx,
        &mut dirty,
        &mut request_wav_browser,
    );
    render_sub_section(ui, sub_rect, &mut poly, &mut dirty, track_idx, slot_idx);
    render_noise_section(ui, noise_rect, &mut poly, &mut dirty, track_idx, slot_idx);
    cursor_x += osc_w + gap;

    // =========================================================================
    // COLUMN 2: Filter (top) + FEG Filter Envelope (bottom)
    // =========================================================================
    let flt_w = 172.0;
    let flt_rect = Rect::from_min_size(pos2(cursor_x, y0), vec2(flt_w, top_h));
    let feg_rect = Rect::from_min_size(pos2(cursor_x, y0 + top_h + gap), vec2(flt_w, bot_h));

    render_filter_section(
        ui,
        flt_rect,
        &mut poly,
        mod_cutoff.map(|m| m.delta),
        track_idx,
        slot_idx,
        &mut dirty,
    );
    render_feg_section(ui, feg_rect, &mut poly, &mut dirty, track_idx, slot_idx);
    cursor_x += flt_w + gap;

    // =========================================================================
    // COLUMN 3: Amp Envelope (top) + Voice/Pitch/Glide (bottom)
    // =========================================================================
    let env_w = 150.0;
    let env_rect = Rect::from_min_size(pos2(cursor_x, y0), vec2(env_w, top_h));
    let voice_rect = Rect::from_min_size(pos2(cursor_x, y0 + top_h + gap), vec2(env_w, bot_h));

    render_amp_env_section(ui, env_rect, &mut poly, &mut dirty, track_idx, slot_idx);
    render_voice_section(ui, voice_rect, &mut poly, &mut dirty, track_idx, slot_idx);
    cursor_x += env_w + gap;

    // =========================================================================
    // COLUMN 4: Note FX / FX pills + Pan / Unison / Out + Stereo Meter
    // =========================================================================
    let out_w = (avail.max.x - cursor_x).max(58.0);
    let out_rect = Rect::from_min_size(pos2(cursor_x, y0), vec2(out_w, total_h));
    render_output_section(
        ui,
        out_rect,
        &mut poly,
        mod_vol.map(|m| m.delta),
        meter_l,
        meter_r,
        track_idx,
        slot_idx,
        &mut dirty,
        &mut request_insert_device,
    );

    // Reserve exact bounding area so parent layout stays bounded
    ui.allocate_rect(avail, Sense::hover());

    if dirty {
        poly.mark_params_dirty();
    }

    // Write back updated state
    if let Some(track) = app.current_tracks_mut().get_mut(track_idx) {
        if let Some(slot) = track.effects.get_mut(slot_idx) {
            slot.open_poly = Some(poly);
        }
    }

    if request_wav_browser {
        crate::views::explorer::begin_wavetable_pick(app, track_idx, slot_idx);
    }
    if request_add_modulator {
        app.popup_browser.open_insert_modulator(track_idx, slot_idx);
    }
    if request_insert_device {
        app.popup_browser.open_insert_device(track_idx);
    }
}

#[allow(clippy::too_many_arguments)]
fn render_oscillator_section(
    ui: &mut Ui,
    rect: Rect,
    poly: &mut HikaruOpenPoly,
    mod_index_delta: Option<f32>,
    track_idx: usize,
    slot_idx: usize,
    dirty: &mut bool,
    request_wav_browser: &mut bool,
) {
    paint_panel(ui, rect, PANEL_BORDER);
    let p = ui.painter().clone();

    // Header bar: Oscillator dropdown + 3D/2D + WAV button
    let hdr_r = Rect::from_min_size(rect.min + vec2(5.0, 3.0), vec2(rect.width() - 10.0, 18.0));
    let osc_sel_r = Rect::from_min_size(hdr_r.min, vec2(102.0, 17.0));
    let osc_btn_resp = ui
        .interact(osc_sel_r, ui.id().with(("poly_osc_sel", track_idx, slot_idx)), Sense::click())
        .on_hover_text("Swap Oscillator Module");
    if osc_btn_resp.hovered() {
        p.rect_filled(osc_sel_r, 2.5, Color32::from_rgb(0x32, 0x35, 0x3D));
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    p.text(
        pos2(osc_sel_r.min.x + 4.0, osc_sel_r.center().y),
        Align2::LEFT_CENTER,
        format!("{} ▾", poly.osc_type.label()),
        fonts::semibold(10.0),
        OSC_AMBER,
    );
    let osc_popup_id = ui.id().with(("poly_osc_popup", track_idx, slot_idx));
    if osc_btn_resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(osc_popup_id));
    }
    egui::popup_below_widget(
        ui,
        osc_popup_id,
        &osc_btn_resp,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(120.0);
            for osc in OpenPolyOscType::ALL {
                let selected = poly.osc_type == osc;
                if ui
                    .selectable_label(selected, RichText::new(osc.label()).color(if selected { OSC_AMBER } else { KNOB_WHITE }))
                    .clicked()
                {
                    poly.osc_type = osc;
                    *dirty = true;
                    ui.close_menu();
                }
            }
        },
    );

    // 3D / 2D pill (for Wavetable) + WAV load button
    if poly.osc_type == OpenPolyOscType::Wavetable {
        let mode_r = Rect::from_min_size(pos2(hdr_r.max.x - 54.0, hdr_r.min.y + 1.0), vec2(24.0, 15.0));
        let mode_resp = ui
            .interact(mode_r, ui.id().with(("poly_3d_mode", slot_idx)), Sense::click())
            .on_hover_text("Toggle 3D Isometric / 2D Wavetable Display");
        p.rect_filled(
            mode_r,
            2.0,
            if poly.render_mode_3d {
                Color32::from_rgb(0x4B, 0x3D, 0x1E)
            } else {
                Color32::from_rgb(0x2F, 0x32, 0x3A)
            },
        );
        p.text(
            mode_r.center(),
            Align2::CENTER_CENTER,
            if poly.render_mode_3d { "3D" } else { "2D" },
            fonts::semibold(8.0),
            if poly.render_mode_3d { OSC_AMBER } else { theme::TEXT_SECONDARY },
        );
        if mode_resp.clicked() {
            poly.render_mode_3d = !poly.render_mode_3d;
            *dirty = true;
        }

        let wav_r = Rect::from_min_size(pos2(hdr_r.max.x - 28.0, hdr_r.min.y + 1.0), vec2(28.0, 15.0));
        let wav_resp = ui
            .interact(wav_r, ui.id().with(("poly_wav_btn", slot_idx)), Sense::click())
            .on_hover_text("Load .wav Wavetable from Explorer");
        p.rect_filled(
            wav_r,
            2.0,
            if wav_resp.hovered() {
                Color32::from_rgb(0x44, 0x48, 0x52)
            } else {
                Color32::from_rgb(0x2F, 0x32, 0x3A)
            },
        );
        p.text(
            wav_r.center(),
            Align2::CENTER_CENTER,
            "WAV",
            fonts::semibold(7.5),
            KNOB_WHITE,
        );
        if wav_resp.clicked() {
            *request_wav_browser = true;
        }
    }

    // Middle area: Waveform visual canvas (left) + Index & Fold knobs (right)
    let wave_r = Rect::from_min_size(pos2(rect.min.x + 6.0, hdr_r.max.y + 3.0), vec2(114.0, 68.0));
    paint_osc_display(ui, wave_r, poly, mod_index_delta);

    // Table / Osc subtitle directly under waveform
    let sub_lbl = if poly.osc_type == OpenPolyOscType::Wavetable {
        let name = poly.table.name.as_str();
        if name.len() > 16 {
            format!("{}…", &name[..15])
        } else {
            name.to_string()
        }
    } else {
        match poly.osc_type {
            OpenPolyOscType::Sawtooth => "PolyBLEP Saw".to_string(),
            OpenPolyOscType::Pulse => format!("PW {:.0}%", (0.06 + poly.osc_index * 0.88) * 100.0),
            OpenPolyOscType::Sine => "Pure / Self-FM".to_string(),
            OpenPolyOscType::Triangle => "Skew Triangle".to_string(),
            OpenPolyOscType::Phase1 => "Phase Distort".to_string(),
            OpenPolyOscType::Swarm => "7-Voice Swarm".to_string(),
            OpenPolyOscType::Union => "Dual Unison".to_string(),
            OpenPolyOscType::Wavetable => poly.table.name.clone(),
        }
    };
    p.text(
        pos2(wave_r.center().x, wave_r.max.y + 8.0),
        Align2::CENTER_CENTER,
        sub_lbl,
        fonts::regular(8.5),
        OSC_AMBER,
    );

    // Index knob (big, right of display) + Fold mini knob above/beside
    let knob_col_x = wave_r.max.x + 32.0;
    let index_rect = Rect::from_center_size(pos2(knob_col_x, wave_r.min.y + 24.0), vec2(36.0, 36.0));
    let mut index_ui = ui.new_child(egui::UiBuilder::new().max_rect(index_rect));
    let _ = mod_index_delta;
    let (idx_resp, idx_chg) = hikaru_openmodulation::modulatable_rotary_knob(
        &mut index_ui,
        36.0,
        &mut poly.osc_index,
        0.0..=1.0,
        0.0,
        KNOB_WHITE,
        false,
        track_idx,
        "OpenPoly",
        "INDEX",
    );
    if idx_chg {
        poly.base_index = Some(poly.osc_index);
        *dirty = true;
    }
    if hikaru_openmodulation::active_mapping_for_track(track_idx).is_none() {
        let _ = idx_resp.on_hover_text(format!(
            "{}: {:.0}% (Shift=fine, double-click=reset)",
            poly.osc_type.index_param_label(),
            poly.osc_index * 100.0
        ));
    }
    p.text(
        pos2(knob_col_x, index_rect.max.y + 6.0),
        Align2::CENTER_CENTER,
        poly.osc_type.index_param_label(),
        fonts::regular(8.5),
        Color32::from_rgb(0xC6, 0xCA, 0xD2),
    );

    // Fold / Morph mini knob below Index label
    let fold_rect = Rect::from_center_size(pos2(knob_col_x, wave_r.max.y + 5.0), vec2(20.0, 20.0));
    let mut fold_ui = ui.new_child(egui::UiBuilder::new().max_rect(fold_rect));
    let (fold_resp, fold_chg) = hikaru_openmodulation::modulatable_rotary_knob(
        &mut fold_ui,
        20.0,
        &mut poly.osc_fold,
        0.0..=1.0,
        0.0,
        OSC_AMBER,
        false,
        track_idx,
        "OpenPoly",
        "FOLD",
    );
    if fold_chg {
        *dirty = true;
    }
    if hikaru_openmodulation::active_mapping_for_track(track_idx).is_none() {
        let _ = fold_resp.on_hover_text(format!("Wavefold / Bend: {:.0}%", poly.osc_fold * 100.0));
    }

    // Bottom pitch bar: Ratio (1:1) | Semitones (0.0 st) | Detune (±0.0 Hz) | SYNC
    let bot_y = rect.max.y - 24.0;
    let ratio_r = Rect::from_min_size(pos2(rect.min.x + 6.0, bot_y), vec2(32.0, 17.0));
    let semi_r = Rect::from_min_size(pos2(ratio_r.max.x + 3.0, bot_y), vec2(48.0, 17.0));
    let det_r = Rect::from_min_size(pos2(semi_r.max.x + 3.0, bot_y), vec2(50.0, 17.0));
    let sync_r = Rect::from_min_size(pos2(det_r.max.x + 4.0, bot_y), vec2(34.0, 17.0));

    // Ratio pill (click cycles common musical ratios)
    let ratio_resp = ui
        .interact(ratio_r, ui.id().with(("poly_ratio", slot_idx)), Sense::click())
        .on_hover_text("Oscillator harmonic ratio (click to cycle 1:1, 2:1, 1:2, 3:2, 3:1, 4:1)");
    p.rect_filled(ratio_r, 2.0, PANEL_BG_DARK);
    p.text(
        ratio_r.center(),
        Align2::CENTER_CENTER,
        format!("{}:{}", poly.osc_ratio_num, poly.osc_ratio_den),
        fonts::semibold(8.5),
        OSC_AMBER,
    );
    if ratio_resp.clicked() {
        let next = match (poly.osc_ratio_num, poly.osc_ratio_den) {
            (1, 1) => (2, 1),
            (2, 1) => (1, 2),
            (1, 2) => (3, 2),
            (3, 2) => (3, 1),
            (3, 1) => (4, 1),
            _ => (1, 1),
        };
        poly.osc_ratio_num = next.0;
        poly.osc_ratio_den = next.1;
        *dirty = true;
    }

    // Semitones draggable readout
    let semi_resp = ui
        .interact(semi_r, ui.id().with(("poly_semi", slot_idx)), Sense::click_and_drag())
        .on_hover_text("Oscillator pitch in semitones (-36..+36). Drag vertically; double-click = 0");
    p.rect_filled(semi_r, 2.0, PANEL_BG_DARK);
    if semi_resp.double_clicked() {
        poly.osc_semitones = 0.0;
        *dirty = true;
    } else if semi_resp.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let step = if fine { 0.05 } else { 0.2 };
        poly.osc_semitones = (poly.osc_semitones - semi_resp.drag_delta().y * step).clamp(-36.0, 36.0);
        if !fine {
            poly.osc_semitones = poly.osc_semitones.round();
        }
        *dirty = true;
    }
    if semi_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
    }
    p.text(
        semi_r.center(),
        Align2::CENTER_CENTER,
        format!("{:.1} st", poly.osc_semitones),
        fonts::regular(8.5),
        OSC_AMBER,
    );

    // Fine Hz Detune draggable readout
    let det_resp = ui
        .interact(det_r, ui.id().with(("poly_det_hz", slot_idx)), Sense::click_and_drag())
        .on_hover_text("Oscillator fine detune (±50 Hz). Drag vertically; double-click = 0 Hz");
    p.rect_filled(det_r, 2.0, PANEL_BG_DARK);
    if det_resp.double_clicked() {
        poly.osc_detune_hz = 0.0;
        *dirty = true;
    } else if det_resp.dragged() {
        poly.osc_detune_hz = (poly.osc_detune_hz - det_resp.drag_delta().y * 0.25).clamp(-50.0, 50.0);
        *dirty = true;
    }
    if det_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
    }
    p.text(
        det_r.center(),
        Align2::CENTER_CENTER,
        format!("±{:.1}Hz", poly.osc_detune_hz.abs()),
        fonts::regular(8.0),
        Color32::from_rgb(0xC6, 0xB8, 0x9A),
    );

    // SYNC toggle pill
    let sync_resp = ui
        .interact(sync_r, ui.id().with(("poly_sync", slot_idx)), Sense::click())
        .on_hover_text("Hard-Sync Oscillator");
    p.rect_filled(
        sync_r,
        2.5,
        if poly.osc_sync {
            SUB_PURPLE
        } else {
            Color32::from_rgb(0x32, 0x34, 0x3C)
        },
    );
    p.text(
        sync_r.center(),
        Align2::CENTER_CENTER,
        "SYNC",
        fonts::semibold(7.5),
        if poly.osc_sync { Color32::from_rgb(0x14, 0x12, 0x1A) } else { theme::TEXT_SECONDARY },
    );
    if sync_resp.clicked() {
        poly.osc_sync = !poly.osc_sync;
        *dirty = true;
    }
}

fn paint_osc_display(ui: &Ui, rect: Rect, poly: &HikaruOpenPoly, mod_index_delta: Option<f32>) {
    let p = ui.painter().clone();
    p.rect_filled(rect, 2.5, PANEL_BG_DARK);
    p.rect_stroke(
        rect,
        2.5,
        Stroke::new(1.0_f32, Color32::from_rgb(0x2D, 0x30, 0x38)),
        egui::StrokeKind::Inside,
    );

    let eff_idx = (poly.osc_index + mod_index_delta.unwrap_or(0.0)).clamp(0.0, 1.0);

    if poly.osc_type == OpenPolyOscType::Wavetable && poly.render_mode_3d {
        // Isometric multi-slice 3D wavetable ribbon stack
        let slices = 6usize;
        let active_slice = (eff_idx * (slices - 1) as f32).round() as usize;
        let samples = 44usize;
        let pad_x = 12.0;
        let w = rect.width() - pad_x * 2.0 - 16.0;
        let amp = rect.height() * 0.22;

        for s in (0..slices).rev() {
            let depth = s as f32 / (slices - 1) as f32;
            let idx_slice = depth;
            let ox = rect.min.x + pad_x + depth * 16.0;
            let oy = rect.center().y + 10.0 - depth * 18.0;
            let is_active = s == active_slice;

            let mut pts = Vec::with_capacity(samples + 1);
            for i in 0..=samples {
                let t = i as f32 / samples as f32;
                let v = eval_osc_preview(
                    OpenPolyOscType::Wavetable,
                    t,
                    idx_slice,
                    poly.osc_fold,
                    Some(&poly.wt_data),
                );
                pts.push(pos2(ox + t * w, oy - v * amp));
            }
            let col = if is_active {
                Color32::from_rgb(0xFF, 0xCA, 0x48)
            } else {
                Color32::from_rgba_unmultiplied(0xC8, 0x8E, 0x22, 105)
            };
            p.add(egui::Shape::line(pts, Stroke::new(if is_active { 1.8_f32 } else { 1.0_f32 }, col)));
        }
    } else {
        // Clean 2D live waveform trace
        let cy = rect.center().y;
        p.line_segment(
            [pos2(rect.min.x + 4.0, cy), pos2(rect.max.x - 4.0, cy)],
            Stroke::new(0.7_f32, Color32::from_rgb(0x2E, 0x32, 0x3B)),
        );
        let samples = 64usize;
        let mut pts = Vec::with_capacity(samples + 1);
        for i in 0..=samples {
            let t = i as f32 / samples as f32;
            let v = eval_osc_preview(
                poly.osc_type,
                t,
                eff_idx,
                poly.osc_fold,
                Some(&poly.wt_data),
            );
            let x = rect.min.x + 5.0 + t * (rect.width() - 10.0);
            let y = cy - v * (rect.height() * 0.36);
            pts.push(pos2(x, y));
        }
        p.add(egui::Shape::line(pts, Stroke::new(1.7_f32, OSC_AMBER)));
    }
}

fn render_sub_section(
    ui: &mut Ui,
    rect: Rect,
    poly: &mut HikaruOpenPoly,
    dirty: &mut bool,
    track_idx: usize,
    slot_idx: usize,
) {
    paint_panel(ui, rect, PANEL_BORDER);
    let p = ui.painter().clone();

    // Sub knob (left)
    let knob_r = Rect::from_center_size(pos2(rect.min.x + 24.0, rect.min.y + 30.0), vec2(30.0, 30.0));
    let mut k_ui = ui.new_child(egui::UiBuilder::new().max_rect(knob_r));
    let (resp, chg) = hikaru_openmodulation::modulatable_rotary_knob(
        &mut k_ui,
        30.0,
        &mut poly.sub_level,
        0.0..=1.0,
        0.25,
        SUB_PURPLE,
        false,
        track_idx,
        "OpenPoly",
        "SUB",
    );
    if chg {
        *dirty = true;
    }
    if hikaru_openmodulation::active_mapping_for_track(track_idx).is_none() {
        let _ = resp.on_hover_text(format!("Sub Oscillator Level: {:.0}%", poly.sub_level * 100.0));
    }
    p.text(
        pos2(knob_r.center().x, rect.max.y - 12.0),
        Align2::CENTER_CENTER,
        "Sub",
        fonts::regular(9.0),
        Color32::from_rgb(0xC6, 0xCA, 0xD2),
    );

    // 4 sub waveform selector buttons (top-right)
    let wave_x0 = rect.min.x + 48.0;
    let btn_w = 16.5;
    for (i, w) in OpenPolySubWave::ALL.iter().enumerate() {
        let br = Rect::from_min_size(pos2(wave_x0 + i as f32 * (btn_w + 1.5), rect.min.y + 11.0), vec2(btn_w, 20.0));
        let active = poly.sub_wave == *w;
        let bresp = ui
            .interact(br, ui.id().with(("poly_sub_w", slot_idx, i)), Sense::click())
            .on_hover_text(format!("Sub wave: {}", w.label()));
        p.rect_filled(
            br,
            2.0,
            if active {
                Color32::from_rgb(0x3D, 0x31, 0x56)
            } else {
                Color32::from_rgb(0x2C, 0x2F, 0x36)
            },
        );
        paint_mini_wave_glyph(&p, br.shrink(3.5), *w, if active { SUB_PURPLE } else { theme::TEXT_MUTED });
        if bresp.clicked() {
            poly.sub_wave = *w;
            *dirty = true;
        }
    }

    // 3 octave selector buttons (0, -1, -2) below waveforms
    let octs: [i8; 3] = [0, -1, -2];
    let oct_w = 22.0;
    for (i, oct) in octs.iter().enumerate() {
        let or = Rect::from_min_size(pos2(wave_x0 + i as f32 * (oct_w + 2.0), rect.min.y + 42.0), vec2(oct_w, 20.0));
        let active = poly.sub_octave == *oct;
        let oresp = ui
            .interact(or, ui.id().with(("poly_sub_oct", slot_idx, i)), Sense::click())
            .on_hover_text(format!("Sub octave offset: {oct}"));
        p.text(
            or.center(),
            Align2::CENTER_CENTER,
            format!("{oct}"),
            fonts::semibold(9.5),
            if active { Color32::WHITE } else { theme::TEXT_MUTED },
        );
        if active {
            p.line_segment(
                [pos2(or.min.x + 3.0, or.max.y - 1.5), pos2(or.max.x - 3.0, or.max.y - 1.5)],
                Stroke::new(2.0_f32, SUB_PURPLE),
            );
        }
        if oresp.clicked() {
            poly.sub_octave = *oct;
            *dirty = true;
        }
    }
}

fn paint_mini_wave_glyph(p: &egui::Painter, r: Rect, wave: OpenPolySubWave, col: Color32) {
    let cy = r.center().y;
    let pts: Vec<Pos2> = match wave {
        OpenPolySubWave::Sine => (0..=8)
            .map(|i| {
                let t = i as f32 / 8.0;
                pos2(r.min.x + t * r.width(), cy - (t * std::f32::consts::TAU).sin() * r.height() * 0.45)
            })
            .collect(),
        OpenPolySubWave::Triangle => vec![
            pos2(r.min.x, cy),
            pos2(r.min.x + r.width() * 0.25, r.min.y),
            pos2(r.min.x + r.width() * 0.75, r.max.y),
            pos2(r.max.x, cy),
        ],
        OpenPolySubWave::Square => vec![
            pos2(r.min.x, r.max.y),
            pos2(r.min.x, r.min.y),
            pos2(r.center().x, r.min.y),
            pos2(r.center().x, r.max.y),
            pos2(r.max.x, r.max.y),
            pos2(r.max.x, r.min.y),
        ],
        OpenPolySubWave::Saw => vec![
            pos2(r.min.x, r.max.y),
            pos2(r.max.x, r.min.y),
            pos2(r.max.x, r.max.y),
        ],
    };
    p.add(egui::Shape::line(pts, Stroke::new(1.3_f32, col)));
}

fn render_noise_section(
    ui: &mut Ui,
    rect: Rect,
    poly: &mut HikaruOpenPoly,
    dirty: &mut bool,
    track_idx: usize,
    _slot_idx: usize,
) {
    paint_panel(ui, rect, PANEL_BORDER);
    let knob_r = Rect::from_center_size(pos2(rect.center().x, rect.min.y + 30.0), vec2(30.0, 30.0));
    let mut k_ui = ui.new_child(egui::UiBuilder::new().max_rect(knob_r));
    let (resp, chg) = hikaru_openmodulation::modulatable_rotary_knob(
        &mut k_ui,
        30.0,
        &mut poly.noise_level,
        0.0..=1.0,
        0.0,
        KNOB_WHITE,
        false,
        track_idx,
        "OpenPoly",
        "NOISE",
    );
    if chg {
        *dirty = true;
    }
    if hikaru_openmodulation::active_mapping_for_track(track_idx).is_none() {
        let _ = resp.on_hover_text(format!("White Noise Level: {:.0}%", poly.noise_level * 100.0));
    }
    ui.painter().text(
        pos2(rect.center().x, rect.max.y - 12.0),
        Align2::CENTER_CENTER,
        "Noise",
        fonts::regular(9.0),
        Color32::from_rgb(0xC6, 0xCA, 0xD2),
    );
}

#[allow(clippy::too_many_arguments)]
fn render_filter_section(
    ui: &mut Ui,
    rect: Rect,
    poly: &mut HikaruOpenPoly,
    mod_cutoff_delta: Option<f32>,
    track_idx: usize,
    slot_idx: usize,
    dirty: &mut bool,
) {
    paint_panel(ui, rect, PANEL_BORDER);
    let p = ui.painter().clone();

    // Header: Filter power toggle + Filter Type selector
    let hdr_r = Rect::from_min_size(rect.min + vec2(5.0, 3.0), vec2(rect.width() - 10.0, 18.0));
    let pwr_r = Rect::from_min_size(pos2(hdr_r.min.x, hdr_r.center().y - 6.0), vec2(12.0, 12.0));
    let pwr_resp = ui
        .interact(pwr_r, ui.id().with(("poly_flt_pwr", slot_idx)), Sense::click())
        .on_hover_text("Enable / Bypass Filter");
    p.circle_stroke(
        pwr_r.center(),
        4.5,
        Stroke::new(1.5_f32, if poly.filter_enabled { FILTER_CYAN } else { theme::TEXT_MUTED }),
    );
    if poly.filter_enabled {
        p.circle_filled(pwr_r.center(), 2.0, FILTER_CYAN);
    }
    if pwr_resp.clicked() {
        poly.filter_enabled = !poly.filter_enabled;
        *dirty = true;
    }

    let sel_r = Rect::from_min_max(pos2(pwr_r.max.x + 6.0, hdr_r.min.y), hdr_r.max);
    let sel_resp = ui
        .interact(sel_r, ui.id().with(("poly_flt_sel", track_idx, slot_idx)), Sense::click())
        .on_hover_text("Swap Filter Module");
    if sel_resp.hovered() {
        p.rect_filled(sel_r, 2.5, Color32::from_rgb(0x30, 0x34, 0x3D));
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    p.text(
        sel_r.center(),
        Align2::CENTER_CENTER,
        format!("{} ▾", poly.filter_type.label()),
        fonts::semibold(10.0),
        if poly.filter_enabled { FILTER_CYAN } else { theme::TEXT_MUTED },
    );
    let flt_popup_id = ui.id().with(("poly_flt_popup", track_idx, slot_idx));
    if sel_resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(flt_popup_id));
    }
    egui::popup_below_widget(
        ui,
        flt_popup_id,
        &sel_resp,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(126.0);
            for ft in OpenPolyFilterType::ALL {
                let selected = poly.filter_type == ft;
                if ui
                    .selectable_label(selected, RichText::new(ft.label()).color(if selected { FILTER_CYAN } else { KNOB_WHITE }))
                    .clicked()
                {
                    poly.filter_type = ft;
                    *dirty = true;
                    ui.close_menu();
                }
            }
        },
    );

    // Left vertical mini-knobs: Drive (cyan), Keytrack (amber), FEG -> Cutoff (green bipolar)
    let mini_x = rect.min.x + 28.0;
    let body_top = hdr_r.max.y + 8.0;

    let mini_specs: [(&str, f32, Color32, bool); 3] = [
        ("DRV", body_top + 11.0, FILTER_CYAN, false),
        ("KTR", body_top + 46.0, OSC_AMBER, false),
        ("ENV", body_top + 81.0, FEG_GREEN, true),
    ];
    for (i, (tag, cy, col, bipolar)) in mini_specs.iter().enumerate() {
        p.text(
            pos2(rect.min.x + 5.0, *cy),
            Align2::LEFT_CENTER,
            *tag,
            fonts::semibold(6.8),
            *col,
        );
        let kr = Rect::from_center_size(pos2(mini_x + 8.0, *cy), vec2(22.0, 22.0));
        let mut kui = ui.new_child(egui::UiBuilder::new().max_rect(kr));
        let (resp, chg) = match i {
            0 => hikaru_openmodulation::modulatable_rotary_knob(
                &mut kui, 22.0, &mut poly.filter_drive, 0.0..=1.0, 0.08, *col, *bipolar, track_idx, "OpenPoly", "DRV",
            ),
            1 => hikaru_openmodulation::modulatable_rotary_knob(
                &mut kui, 22.0, &mut poly.filter_keytrack, 0.0..=1.0, 0.5, *col, *bipolar, track_idx, "OpenPoly", "KTR",
            ),
            _ => hikaru_openmodulation::modulatable_rotary_knob(
                &mut kui, 22.0, &mut poly.filter_env_amount, -1.0..=1.0, 0.42, *col, *bipolar, track_idx, "OpenPoly", "ENV",
            ),
        };
        if chg {
            *dirty = true;
        }
        if hikaru_openmodulation::active_mapping_for_track(track_idx).is_none() {
            let tip = match i {
                0 => format!("Filter Drive / Saturation: {:.0}%", poly.filter_drive * 100.0),
                1 => format!("Filter Keytracking: {:.0}%", poly.filter_keytrack * 100.0),
                _ => format!("FEG → Cutoff Depth: {:+.0}%", poly.filter_env_amount * 100.0),
            };
            let _ = resp.on_hover_text(tip);
        }
    }

    // Center hero Cutoff knob (48px log-mapped 20Hz..20kHz)
    let cut_cx = rect.min.x + 92.0;
    let cut_cy = body_top + 42.0;
    let cut_r = Rect::from_center_size(pos2(cut_cx, cut_cy), vec2(48.0, 48.0));
    let mut cut_norm = hz_to_log_norm(poly.filter_cutoff_hz);
    let mut cut_ui = ui.new_child(egui::UiBuilder::new().max_rect(cut_r));
    let _ = mod_cutoff_delta;
    let (cut_resp, cut_chg) = hikaru_openmodulation::modulatable_rotary_knob(
        &mut cut_ui,
        48.0,
        &mut cut_norm,
        0.0..=1.0,
        hz_to_log_norm(1650.0),
        KNOB_WHITE,
        false,
        track_idx,
        "OpenPoly",
        "CUTOFF",
    );
    if cut_chg {
        poly.filter_cutoff_hz = log_norm_to_hz(cut_norm);
        *dirty = true;
    }
    if hikaru_openmodulation::active_mapping_for_track(track_idx).is_none() {
        let _ = cut_resp.on_hover_text(format!("Filter Cutoff: {}", format_hz(poly.filter_cutoff_hz)));
    }
    p.text(
        pos2(cut_cx, cut_r.max.y + 11.0),
        Align2::CENTER_CENTER,
        format_hz(poly.filter_cutoff_hz),
        fonts::regular(9.5),
        Color32::from_rgb(0xD2, 0xD6, 0xDE),
    );

    // Right Resonance knob (32px) + mini resonance peak glyph
    let res_cx = rect.max.x - 26.0;
    let res_cy = body_top + 42.0;
    let res_r = Rect::from_center_size(pos2(res_cx, res_cy), vec2(32.0, 32.0));
    let mut res_ui = ui.new_child(egui::UiBuilder::new().max_rect(res_r));
    let (res_resp, res_chg) = hikaru_openmodulation::modulatable_rotary_knob(
        &mut res_ui,
        32.0,
        &mut poly.filter_resonance,
        0.0..=0.98,
        0.22,
        FILTER_CYAN,
        false,
        track_idx,
        "OpenPoly",
        "RESO",
    );
    if res_chg {
        *dirty = true;
    }
    if hikaru_openmodulation::active_mapping_for_track(track_idx).is_none() {
        let _ = res_resp.on_hover_text(format!("Filter Resonance: {:.0}%", poly.filter_resonance * 100.0));
    }
    p.text(
        pos2(res_cx, res_r.max.y + 11.0),
        Align2::CENTER_CENTER,
        "Reso",
        fonts::regular(8.5),
        Color32::from_rgb(0xB8, 0xBC, 0xC6),
    );
}

fn render_feg_section(
    ui: &mut Ui,
    rect: Rect,
    poly: &mut HikaruOpenPoly,
    dirty: &mut bool,
    track_idx: usize,
    slot_idx: usize,
) {
    paint_panel(ui, rect, PANEL_BORDER);
    let p = ui.painter().clone();

    // Left FEG badge toggle
    let badge_r = Rect::from_min_size(pos2(rect.min.x + 5.0, rect.min.y + 12.0), vec2(26.0, 50.0));
    let badge_resp = ui
        .interact(badge_r, ui.id().with(("poly_feg_en", slot_idx)), Sense::click())
        .on_hover_text("Toggle Filter Envelope Generator (FEG)");
    p.text(
        pos2(badge_r.center().x, badge_r.min.y + 10.0),
        Align2::CENTER_CENTER,
        "FEG",
        fonts::semibold(8.5),
        if poly.feg_enabled { FEG_GREEN } else { theme::TEXT_MUTED },
    );
    // Tiny envelope curve glyph
    let gx = badge_r.min.x + 3.0;
    let gy = badge_r.max.y - 10.0;
    p.add(egui::Shape::line(
        vec![
            pos2(gx, gy),
            pos2(gx + 5.0, gy - 12.0),
            pos2(gx + 10.0, gy - 6.0),
            pos2(gx + 15.0, gy - 6.0),
            pos2(gx + 20.0, gy),
        ],
        Stroke::new(1.3_f32, if poly.feg_enabled { FEG_GREEN } else { theme::TEXT_MUTED }),
    ));
    if badge_resp.clicked() {
        poly.feg_enabled = !poly.feg_enabled;
        *dirty = true;
    }

    // 4 FEG knobs: A, D, S, R
    let start_x = rect.min.x + 48.0;
    let step_x = 34.0;
    let ky = rect.min.y + 30.0;
    let labels = ["A", "D", "S", "R"];

    let mut a_n = ms_to_log_norm(poly.feg_attack_ms, 0.5, 5000.0);
    let mut d_n = ms_to_log_norm(poly.feg_decay_ms, 1.0, 8000.0);
    let mut s_v = poly.feg_sustain;
    let mut r_n = ms_to_log_norm(poly.feg_release_ms, 1.0, 8000.0);

    for (i, lbl) in labels.iter().enumerate() {
        let cx = start_x + i as f32 * step_x;
        let kr = Rect::from_center_size(pos2(cx, ky), vec2(26.0, 26.0));
        let mut kui = ui.new_child(egui::UiBuilder::new().max_rect(kr));
        let col = if poly.feg_enabled { FEG_GREEN } else { theme::TEXT_MUTED };
        let (resp, chg) = match i {
            0 => hikaru_openmodulation::modulatable_rotary_knob(
                &mut kui, 26.0, &mut a_n, 0.0..=1.0, ms_to_log_norm(4.0, 0.5, 5000.0), col, false, track_idx, "OpenPoly", "FEG_A",
            ),
            1 => hikaru_openmodulation::modulatable_rotary_knob(
                &mut kui, 26.0, &mut d_n, 0.0..=1.0, ms_to_log_norm(280.0, 1.0, 8000.0), col, false, track_idx, "OpenPoly", "FEG_D",
            ),
            2 => hikaru_openmodulation::modulatable_rotary_knob(
                &mut kui, 26.0, &mut s_v, 0.0..=1.0, 0.25, col, false, track_idx, "OpenPoly", "FEG_S",
            ),
            _ => hikaru_openmodulation::modulatable_rotary_knob(
                &mut kui, 26.0, &mut r_n, 0.0..=1.0, ms_to_log_norm(220.0, 1.0, 8000.0), col, false, track_idx, "OpenPoly", "FEG_R",
            ),
        };
        if chg {
            match i {
                0 => poly.feg_attack_ms = log_norm_to_ms(a_n, 0.5, 5000.0),
                1 => poly.feg_decay_ms = log_norm_to_ms(d_n, 1.0, 8000.0),
                2 => poly.feg_sustain = s_v,
                _ => poly.feg_release_ms = log_norm_to_ms(r_n, 1.0, 8000.0),
            }
            *dirty = true;
        }
        if hikaru_openmodulation::active_mapping_for_track(track_idx).is_none() {
            let tip = match i {
                0 => format!("FEG Attack: {}", format_ms(poly.feg_attack_ms)),
                1 => format!("FEG Decay: {}", format_ms(poly.feg_decay_ms)),
                2 => format!("FEG Sustain: {:.0}%", poly.feg_sustain * 100.0),
                _ => format!("FEG Release: {}", format_ms(poly.feg_release_ms)),
            };
            let _ = resp.on_hover_text(tip);
        }
        p.text(
            pos2(cx, rect.max.y - 12.0),
            Align2::CENTER_CENTER,
            *lbl,
            fonts::regular(9.0),
            Color32::from_rgb(0xC6, 0xCA, 0xD2),
        );
    }
}

fn render_amp_env_section(
    ui: &mut Ui,
    rect: Rect,
    poly: &mut HikaruOpenPoly,
    dirty: &mut bool,
    track_idx: usize,
    slot_idx: usize,
) {
    paint_panel(ui, rect, PANEL_BORDER);
    let p = ui.painter().clone();

    // Header: [A] badge + Env Type selector (ADSR / AR / AD)
    let hdr_r = Rect::from_min_size(rect.min + vec2(5.0, 3.0), vec2(rect.width() - 10.0, 18.0));
    let badge_r = Rect::from_min_size(pos2(hdr_r.min.x, hdr_r.min.y + 2.0), vec2(14.0, 14.0));
    p.rect_filled(badge_r, 2.0, AMP_BLUE);
    p.text(
        badge_r.center(),
        Align2::CENTER_CENTER,
        "A",
        fonts::semibold(8.5),
        Color32::from_rgb(0x10, 0x16, 0x20),
    );

    let sel_r = Rect::from_min_max(pos2(badge_r.max.x + 6.0, hdr_r.min.y), hdr_r.max);
    let sel_resp = ui
        .interact(sel_r, ui.id().with(("poly_env_sel", slot_idx)), Sense::click())
        .on_hover_text("Swap Amplifier Envelope Module (ADSR / AR / AD)");
    if sel_resp.hovered() {
        p.rect_filled(sel_r, 2.5, Color32::from_rgb(0x30, 0x34, 0x3D));
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    p.text(
        sel_r.center(),
        Align2::CENTER_CENTER,
        format!("{} ▾", poly.amp_env_type.label()),
        fonts::semibold(10.0),
        AMP_BLUE,
    );
    let env_popup_id = ui.id().with(("poly_env_popup", slot_idx));
    if sel_resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(env_popup_id));
    }
    egui::popup_below_widget(
        ui,
        env_popup_id,
        &sel_resp,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(96.0);
            for et in OpenPolyEnvType::ALL {
                let selected = poly.amp_env_type == et;
                if ui
                    .selectable_label(selected, RichText::new(et.label()).color(if selected { AMP_BLUE } else { KNOB_WHITE }))
                    .clicked()
                {
                    poly.amp_env_type = et;
                    *dirty = true;
                    ui.close_menu();
                }
            }
        },
    );

    // Interactive ADSR Curve Graph
    let graph_r = Rect::from_min_size(pos2(rect.min.x + 6.0, hdr_r.max.y + 3.0), vec2(rect.width() - 12.0, 58.0));
    p.rect_filled(graph_r, 2.5, PANEL_BG_DARK);
    p.rect_stroke(
        graph_r,
        2.5,
        Stroke::new(1.0_f32, Color32::from_rgb(0x2D, 0x30, 0x38)),
        egui::StrokeKind::Inside,
    );

    let a_n = ms_to_log_norm(poly.amp_attack_ms, 0.5, 5000.0);
    let d_n = ms_to_log_norm(poly.amp_decay_ms, 1.0, 8000.0);
    let s_v = match poly.amp_env_type {
        OpenPolyEnvType::Ar => 1.0,
        OpenPolyEnvType::Ad => 0.0,
        OpenPolyEnvType::Adsr => poly.amp_sustain.clamp(0.0, 1.0),
    };
    let r_n = ms_to_log_norm(poly.amp_release_ms, 1.0, 8000.0);

    let pad = 6.0;
    let gw = graph_r.width() - pad * 2.0;
    let gh = graph_r.height() - pad * 2.0;
    let gx0 = graph_r.min.x + pad;
    let gy_bot = graph_r.max.y - pad;
    let gy_top = graph_r.min.y + pad;

    let x_atk = gx0 + (0.04 + a_n * 0.24) * gw;
    let x_dec = x_atk + (0.06 + d_n * 0.24) * gw;
    let x_sus = x_dec + 0.20 * gw;
    let x_rel = (x_sus + (0.05 + r_n * 0.22) * gw).min(gx0 + gw);
    let y_sus = gy_bot - s_v * gh;

    let env_pts = match poly.amp_env_type {
        OpenPolyEnvType::Ad => vec![
            pos2(gx0, gy_bot),
            pos2(x_atk, gy_top),
            pos2(x_dec, gy_bot),
        ],
        OpenPolyEnvType::Ar => vec![
            pos2(gx0, gy_bot),
            pos2(x_atk, gy_top),
            pos2(x_sus, gy_top),
            pos2(x_rel, gy_bot),
        ],
        OpenPolyEnvType::Adsr => vec![
            pos2(gx0, gy_bot),
            pos2(x_atk, gy_top),
            pos2(x_dec, y_sus),
            pos2(x_sus, y_sus),
            pos2(x_rel, gy_bot),
        ],
    };

    // Soft translucent blue fill under curve
    let mut fill_pts = env_pts.clone();
    fill_pts.push(pos2(env_pts.last().unwrap().x, gy_bot));
    fill_pts.push(pos2(gx0, gy_bot));
    p.add(egui::Shape::convex_polygon(
        fill_pts,
        Color32::from_rgba_unmultiplied(0x38, 0x88, 0xD8, 36),
        Stroke::NONE,
    ));
    p.add(egui::Shape::line(env_pts.clone(), Stroke::new(1.7_f32, AMP_BLUE)));
    for pt in env_pts.iter().skip(1) {
        p.circle_filled(*pt, 2.6, Color32::from_rgb(0x88, 0xC8, 0xFF));
    }

    // 4 Amp Envelope knobs: A, D, S, R
    let mut a_k = a_n;
    let mut d_k = d_n;
    let mut s_k = poly.amp_sustain;
    let mut r_k = r_n;
    let start_x = rect.min.x + 24.0;
    let step_x = 34.0;
    let ky = graph_r.max.y + 24.0;
    let labels = ["A", "D", "S", "R"];

    for (i, lbl) in labels.iter().enumerate() {
        let cx = start_x + i as f32 * step_x;
        let kr = Rect::from_center_size(pos2(cx, ky), vec2(26.0, 26.0));
        let mut kui = ui.new_child(egui::UiBuilder::new().max_rect(kr));
        let active_knob = match (poly.amp_env_type, i) {
            (OpenPolyEnvType::Ar, 1 | 2) => false,
            (OpenPolyEnvType::Ad, 2 | 3) => false,
            _ => true,
        };
        let col = if active_knob { AMP_BLUE } else { Color32::from_rgb(0x52, 0x56, 0x60) };
        let (resp, chg) = match i {
            0 => hikaru_openmodulation::modulatable_rotary_knob(
                &mut kui, 26.0, &mut a_k, 0.0..=1.0, ms_to_log_norm(5.0, 0.5, 5000.0), col, false, track_idx, "OpenPoly", "AMP_A",
            ),
            1 => hikaru_openmodulation::modulatable_rotary_knob(
                &mut kui, 26.0, &mut d_k, 0.0..=1.0, ms_to_log_norm(240.0, 1.0, 8000.0), col, false, track_idx, "OpenPoly", "AMP_D",
            ),
            2 => hikaru_openmodulation::modulatable_rotary_knob(
                &mut kui, 26.0, &mut s_k, 0.0..=1.0, 0.78, col, false, track_idx, "OpenPoly", "AMP_S",
            ),
            _ => hikaru_openmodulation::modulatable_rotary_knob(
                &mut kui, 26.0, &mut r_k, 0.0..=1.0, ms_to_log_norm(180.0, 1.0, 8000.0), col, false, track_idx, "OpenPoly", "AMP_R",
            ),
        };
        if chg {
            match i {
                0 => poly.amp_attack_ms = log_norm_to_ms(a_k, 0.5, 5000.0),
                1 => poly.amp_decay_ms = log_norm_to_ms(d_k, 1.0, 8000.0),
                2 => poly.amp_sustain = s_k,
                _ => poly.amp_release_ms = log_norm_to_ms(r_k, 1.0, 8000.0),
            }
            *dirty = true;
        }
        if hikaru_openmodulation::active_mapping_for_track(track_idx).is_none() {
            let tip = match i {
                0 => format!("Amp Attack: {}", format_ms(poly.amp_attack_ms)),
                1 => format!("Amp Decay: {}", format_ms(poly.amp_decay_ms)),
                2 => format!("Amp Sustain: {:.0}%", poly.amp_sustain * 100.0),
                _ => format!("Amp Release: {}", format_ms(poly.amp_release_ms)),
            };
            let _ = resp.on_hover_text(tip);
        }
        p.text(
            pos2(cx, rect.max.y - 10.0),
            Align2::CENTER_CENTER,
            *lbl,
            fonts::regular(9.0),
            if active_knob { Color32::from_rgb(0xC6, 0xCA, 0xD2) } else { theme::TEXT_MUTED },
        );
    }
}

fn render_voice_section(
    ui: &mut Ui,
    rect: Rect,
    poly: &mut HikaruOpenPoly,
    dirty: &mut bool,
    track_idx: usize,
    slot_idx: usize,
) {
    paint_panel(ui, rect, PANEL_BORDER);
    let p = ui.painter().clone();
    let ky = rect.min.y + 30.0;

    // 1. Velocity Sensitivity knob (left)
    let vel_cx = rect.min.x + 26.0;
    let vel_r = Rect::from_center_size(pos2(vel_cx, ky), vec2(26.0, 26.0));
    let mut v_ui = ui.new_child(egui::UiBuilder::new().max_rect(vel_r));
    let (v_resp, v_chg) = hikaru_openmodulation::modulatable_rotary_knob(
        &mut v_ui, 26.0, &mut poly.velocity_sens, 0.0..=1.0, 0.65, VOICE_CORAL, false, track_idx, "OpenPoly", "VEL",
    );
    if v_chg {
        *dirty = true;
    }
    if hikaru_openmodulation::active_mapping_for_track(track_idx).is_none() {
        let _ = v_resp.on_hover_text(format!("Velocity → Amp Sensitivity: {:.0}%", poly.velocity_sens * 100.0));
    }
    p.text(
        pos2(vel_cx, rect.max.y - 12.0),
        Align2::CENTER_CENTER,
        "Vel",
        fonts::regular(9.0),
        Color32::from_rgb(0xC6, 0xCA, 0xD2),
    );

    // 2. Global Pitch knob (center, ±24 st)
    let pit_cx = rect.min.x + 70.0;
    let pit_r = Rect::from_center_size(pos2(pit_cx, ky), vec2(26.0, 26.0));
    let mut p_ui = ui.new_child(egui::UiBuilder::new().max_rect(pit_r));
    let (p_resp, p_chg) = hikaru_openmodulation::modulatable_rotary_knob(
        &mut p_ui, 26.0, &mut poly.global_pitch_st, -24.0..=24.0, 0.0, VOICE_CORAL, true, track_idx, "OpenPoly", "PITCH",
    );
    if p_chg {
        poly.global_pitch_st = (poly.global_pitch_st * 2.0).round() * 0.5;
        *dirty = true;
    }
    if hikaru_openmodulation::active_mapping_for_track(track_idx).is_none() {
        let _ = p_resp.on_hover_text(format!("Global Pitch: {:+.1} st", poly.global_pitch_st));
    }
    p.text(
        pos2(pit_cx, rect.max.y - 12.0),
        Align2::CENTER_CENTER,
        "Pitch",
        fonts::regular(9.0),
        Color32::from_rgb(0xC6, 0xCA, 0xD2),
    );

    // 3. Glide knob (right) + L (Legato) & M/P (Mono/Poly) micro toggles
    let gld_cx = rect.min.x + 112.0;
    let gld_r = Rect::from_center_size(pos2(gld_cx, ky), vec2(26.0, 26.0));
    let mut g_ui = ui.new_child(egui::UiBuilder::new().max_rect(gld_r));
    let (g_resp, g_chg) = hikaru_openmodulation::modulatable_rotary_knob(
        &mut g_ui, 26.0, &mut poly.glide_ms, 0.0..=1500.0, 0.0, KNOB_WHITE, false, track_idx, "OpenPoly", "GLIDE",
    );
    if g_chg {
        *dirty = true;
    }
    if hikaru_openmodulation::active_mapping_for_track(track_idx).is_none() {
        let _ = g_resp.on_hover_text(format!("Portamento Glide: {}", format_ms(poly.glide_ms)));
    }
    p.text(
        pos2(gld_cx, rect.max.y - 12.0),
        Align2::CENTER_CENTER,
        "Glide",
        fonts::regular(9.0),
        Color32::from_rgb(0xC6, 0xCA, 0xD2),
    );

    // Mini 'M' (Mono) and 'L' (Legato) pills on far right of Voice box
    let mono_r = Rect::from_min_size(pos2(rect.max.x - 19.0, rect.min.y + 12.0), vec2(15.0, 17.0));
    let leg_r = Rect::from_min_size(pos2(rect.max.x - 19.0, rect.min.y + 34.0), vec2(15.0, 17.0));

    let mono_resp = ui
        .interact(mono_r, ui.id().with(("poly_mono_btn", slot_idx)), Sense::click())
        .on_hover_text(if poly.mono_mode { "Mode: Mono (click for Poly)" } else { "Mode: Poly (click for Mono)" });
    p.rect_filled(
        mono_r,
        2.0,
        if poly.mono_mode { VOICE_CORAL } else { Color32::from_rgb(0x32, 0x35, 0x3D) },
    );
    p.text(
        mono_r.center(),
        Align2::CENTER_CENTER,
        if poly.mono_mode { "M" } else { "P" },
        fonts::semibold(8.5),
        if poly.mono_mode { Color32::from_rgb(0x14, 0x10, 0x10) } else { KNOB_WHITE },
    );
    if mono_resp.clicked() {
        poly.mono_mode = !poly.mono_mode;
        *dirty = true;
    }

    let leg_resp = ui
        .interact(leg_r, ui.id().with(("poly_legato_btn", slot_idx)), Sense::click())
        .on_hover_text("Legato Glide (no envelope retrigger while holding notes in Mono mode)");
    p.rect_filled(
        leg_r,
        2.0,
        if poly.legato { MOD_ORANGE } else { Color32::from_rgb(0x32, 0x35, 0x3D) },
    );
    p.text(
        leg_r.center(),
        Align2::CENTER_CENTER,
        "L",
        fonts::semibold(8.5),
        if poly.legato { Color32::from_rgb(0x14, 0x10, 0x10) } else { theme::TEXT_SECONDARY },
    );
    if leg_resp.clicked() {
        poly.legato = !poly.legato;
        *dirty = true;
    }
}

#[allow(clippy::too_many_arguments)]
fn render_output_section(
    ui: &mut Ui,
    rect: Rect,
    poly: &mut HikaruOpenPoly,
    mod_vol_delta: Option<f32>,
    meter_l: f32,
    meter_r: f32,
    track_idx: usize,
    slot_idx: usize,
    dirty: &mut bool,
    request_insert_device: &mut bool,
) {
    paint_panel(ui, rect, PANEL_BORDER);
    let p = ui.painter().clone();

    let meter_w = 8.0;
    let body_w = (rect.width() - meter_w - 6.0).max(40.0);
    let body_cx = rect.min.x + 3.0 + body_w * 0.5;

    // Top chain pills: Note FX & FX
    let nfx_r = Rect::from_min_size(pos2(rect.min.x + 4.0, rect.min.y + 4.0), vec2(body_w - 2.0, 18.0));
    let fx_r = Rect::from_min_size(pos2(rect.min.x + 4.0, nfx_r.max.y + 3.0), vec2(body_w - 2.0, 18.0));
    for (i, (r, lbl, tip)) in [
        (nfx_r, "Note FX", "Insert Note FX / Modulator on this track"),
        (fx_r, "FX", "Insert Audio FX after Hikaru OpenPoly"),
    ]
    .iter()
    .enumerate()
    {
        let resp = ui
            .interact(*r, ui.id().with(("poly_fx_pill", slot_idx, i)), Sense::click())
            .on_hover_text(*tip);
        p.rect_filled(
            *r,
            2.5,
            if resp.hovered() {
                Color32::from_rgb(0x3B, 0x3E, 0x48)
            } else {
                Color32::from_rgb(0x2D, 0x30, 0x38)
            },
        );
        p.rect_stroke(*r, 2.5, Stroke::new(1.0_f32, Color32::from_rgb(0x46, 0x4A, 0x54)), egui::StrokeKind::Inside);
        p.text(
            r.center(),
            Align2::CENTER_CENTER,
            *lbl,
            fonts::semibold(8.5),
            KNOB_WHITE,
        );
        if resp.clicked() {
            *request_insert_device = true;
        }
    }

    // Middle mini knobs: Unison + Pan (row 1), Spread (row 2)
    let row1_y = fx_r.max.y + 22.0;
    let uni_r = Rect::from_center_size(pos2(body_cx - 12.0, row1_y), vec2(21.0, 21.0));
    let mut uni_f = poly.unison_voices as f32;
    let mut u_ui = ui.new_child(egui::UiBuilder::new().max_rect(uni_r));
    let (u_resp, u_chg) = rotary_knob(&mut u_ui, 21.0, &mut uni_f, 1.0..=7.0, 1.0, VOICE_CORAL, false);
    if u_chg {
        poly.unison_voices = uni_f.round().clamp(1.0, 7.0) as u8;
        *dirty = true;
    }
    let _ = u_resp.on_hover_text(format!("Unison Voices: {}", poly.unison_voices));
    p.text(
        pos2(uni_r.center().x, uni_r.max.y + 6.0),
        Align2::CENTER_CENTER,
        format!("{}v", poly.unison_voices),
        fonts::regular(7.5),
        theme::TEXT_MUTED,
    );

    let pan_r = Rect::from_center_size(pos2(body_cx + 12.0, row1_y), vec2(21.0, 21.0));
    let mut pan_ui = ui.new_child(egui::UiBuilder::new().max_rect(pan_r));
    let (pan_resp, pan_chg) = hikaru_openmodulation::modulatable_rotary_knob(
        &mut pan_ui, 21.0, &mut poly.pan, -1.0..=1.0, 0.0, KNOB_WHITE, true, track_idx, "OpenPoly", "PAN",
    );
    if pan_chg {
        *dirty = true;
    }
    if hikaru_openmodulation::active_mapping_for_track(track_idx).is_none() {
        let _ = pan_resp.on_hover_text(format!("Pan: {:+.0}%", poly.pan * 100.0));
    }
    p.text(
        pos2(pan_r.center().x, pan_r.max.y + 6.0),
        Align2::CENTER_CENTER,
        "Pan",
        fonts::regular(7.5),
        theme::TEXT_MUTED,
    );

    // Stereo Width / Detune knob
    let spr_y = row1_y + 34.0;
    let spr_r = Rect::from_center_size(pos2(body_cx, spr_y), vec2(22.0, 22.0));
    let mut s_ui = ui.new_child(egui::UiBuilder::new().max_rect(spr_r));
    let (s_resp, s_chg) = hikaru_openmodulation::modulatable_rotary_knob(
        &mut s_ui, 22.0, &mut poly.unison_detune, 0.0..=1.0, 0.18, KNOB_WHITE, false, track_idx, "OpenPoly", "DETUNE",
    );
    if s_chg {
        *dirty = true;
    }
    if hikaru_openmodulation::active_mapping_for_track(track_idx).is_none() {
        let _ = s_resp.on_hover_text(format!("Unison Detune: {:.0}%", poly.unison_detune * 100.0));
    }
    p.text(
        pos2(body_cx, spr_r.max.y + 6.0),
        Align2::CENTER_CENTER,
        "Det",
        fonts::regular(7.5),
        theme::TEXT_MUTED,
    );

    // Bottom hero Out knob (red/orange ring)
    let out_y = rect.max.y - 36.0;
    let out_r = Rect::from_center_size(pos2(body_cx, out_y), vec2(32.0, 32.0));
    let mut o_ui = ui.new_child(egui::UiBuilder::new().max_rect(out_r));
    let _ = mod_vol_delta;
    let (o_resp, o_chg) = hikaru_openmodulation::modulatable_rotary_knob(
        &mut o_ui,
        32.0,
        &mut poly.output_gain,
        0.0..=2.0,
        0.85,
        VOICE_CORAL,
        false,
        track_idx,
        "OpenPoly",
        "VOL",
    );
    if o_chg {
        *dirty = true;
    }
    if hikaru_openmodulation::active_mapping_for_track(track_idx).is_none() {
        let _ = o_resp.on_hover_text(format!("Output Gain: {:.0}%", poly.output_gain * 100.0));
    }
    p.text(
        pos2(body_cx, rect.max.y - 10.0),
        Align2::CENTER_CENTER,
        "Out",
        fonts::semibold(9.0),
        Color32::from_rgb(0xD8, 0xDC, 0xE4),
    );

    // Vertical stereo level meter on far right
    let m_rect = Rect::from_min_max(
        pos2(rect.max.x - meter_w - 2.0, rect.min.y + 6.0),
        pos2(rect.max.x - 2.0, rect.max.y - 6.0),
    );
    p.rect_filled(m_rect, 1.5, PANEL_BG_DARK);
    let bar_w = (m_rect.width() - 2.0) * 0.5;
    for (ch, lvl) in [meter_l, meter_r].iter().enumerate() {
        let n = lvl.clamp(0.0, 1.0).sqrt();
        let h = n * (m_rect.height() - 2.0);
        if h > 0.5 {
            let bx = m_rect.min.x + 1.0 + ch as f32 * (bar_w + 0.5);
            let bar = Rect::from_min_max(
                pos2(bx, m_rect.max.y - 1.0 - h),
                pos2(bx + bar_w - 0.5, m_rect.max.y - 1.0),
            );
            let col = if n > 0.92 {
                VOICE_CORAL
            } else if n > 0.72 {
                OSC_AMBER
            } else {
                FEG_GREEN
            };
            p.rect_filled(bar, 1.0, col);
        }
    }
}
