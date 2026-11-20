/*
 * Hikaru OpenGrid - Compact Minimap Card & Full Modular Patch Editor (GNU AGPL v3)
 * Copyright (C) 2026 Hikaru Corporation
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 */

use std::collections::HashMap;
use std::f32::consts::TAU;

use egui::{pos2, vec2, Align2, Color32, CursorIcon, Pos2, Rect, RichText, Sense, Stroke, Ui};
use hikaru_dsp::synth::open_grid::{
    GridCategory, GridModuleKind, GridNode, GridSignalKind,
};
use hikaru_dsp::synth::open_poly::{eval_osc_preview, OpenPolyOscType};

use crate::app::AppState;
use crate::theme;
use crate::ui::bitwig_kit::rotary_knob;
use crate::ui::fonts;

use super::open_grid::{load_opengrid_preset, save_opengrid_preset, GRID_FACTORY_PRESETS};

/// Width of compact OpenGrid card in bottom DSP Rack (with Modulators drawer open).
pub const OPEN_GRID_CARD_WIDTH: f32 = 310.0;
/// Width of compact OpenGrid card when Modulators drawer is collapsed.
pub const OPEN_GRID_COMPACT_CARD_WIDTH: f32 = 244.0;

const PANEL_BG: Color32 = Color32::from_rgb(0x24, 0x26, 0x2B);
const PANEL_BG_DARK: Color32 = Color32::from_rgb(0x11, 0x13, 0x17);
const PANEL_BORDER: Color32 = Color32::from_rgb(0x3B, 0x3E, 0x47);
const MOD_ORANGE: Color32 = Color32::from_rgb(0xFF, 0x82, 0x20);
const FEG_GREEN: Color32 = Color32::from_rgb(0x30, 0xC8, 0x74);
const VIEWPORT_BLUE: Color32 = Color32::from_rgb(0x46, 0xB8, 0xFF);
const VOICE_CORAL: Color32 = Color32::from_rgb(0xEB, 0x4E, 0x52);
const KNOB_WHITE: Color32 = Color32::from_rgb(0xE6, 0xE8, 0xEC);

fn cat_color(cat: GridCategory) -> Color32 {
    let [r, g, b] = cat.accent_rgb();
    Color32::from_rgb(r, g, b)
}

fn signal_color(kind: GridSignalKind) -> Color32 {
    match kind {
        GridSignalKind::Audio => Color32::from_rgb(0xE8, 0x54, 0x54),
        GridSignalKind::Mod => Color32::from_rgb(0x42, 0xB8, 0xF0),
        GridSignalKind::Pitch => Color32::from_rgb(0xF0, 0xA0, 0x28),
        GridSignalKind::Gate => Color32::from_rgb(0x36, 0xCC, 0x78),
        GridSignalKind::Phase => Color32::from_rgb(0xA4, 0x70, 0xFF),
    }
}

#[inline]
fn val_to_norm(val: f32, min: f32, max: f32, log_scale: bool) -> f32 {
    if log_scale && min > 0.0 {
        let lo = min.ln();
        let hi = max.max(min + 0.001).ln();
        ((val.clamp(min, max).ln() - lo) / (hi - lo)).clamp(0.0, 1.0)
    } else {
        let span = (max - min).max(1.0e-6);
        ((val - min) / span).clamp(0.0, 1.0)
    }
}

#[inline]
fn norm_to_val(norm: f32, min: f32, max: f32, log_scale: bool) -> f32 {
    let n = norm.clamp(0.0, 1.0);
    if log_scale && min > 0.0 {
        let lo = min.ln();
        let hi = max.max(min + 0.001).ln();
        (lo + n * (hi - lo)).exp()
    } else {
        min + n * (max - min)
    }
}

/// Renders the preset menu triggered from the sidebar folder icon.
pub fn render_preset_menu(ui: &mut Ui, app: &mut AppState, track_idx: usize, slot_idx: usize) {
    ui.set_min_width(185.0);
    ui.label(
        RichText::new("Hikaru OpenGrid")
            .font(fonts::semibold(11.0))
            .color(theme::ACCENT_ORANGE),
    );
    ui.label(
        RichText::new("AGPL-v3 Polyphonic Modular Grid")
            .font(fonts::regular(9.0))
            .color(theme::TEXT_MUTED),
    );
    ui.separator();
    let is_open = app.open_grid_editor == Some((track_idx, slot_idx));
    if ui
        .button(if is_open { "Hide OpenGrid Editor" } else { "Open OpenGrid Editor" })
        .clicked()
    {
        app.open_grid_editor = if is_open { None } else { Some((track_idx, slot_idx)) };
        ui.close_menu();
    }
    ui.separator();
    ui.label(
        RichText::new("Factory Patches")
            .font(fonts::semibold(9.5))
            .color(theme::TEXT_SECONDARY),
    );
    for preset in &GRID_FACTORY_PRESETS {
        let lbl = format!("{}  [{}]", preset.name, preset.category);
        if ui.button(lbl).clicked() {
            if let Some(track) = app.current_tracks_mut().get_mut(track_idx) {
                if let Some(slot) = track.effects.get_mut(slot_idx) {
                    slot.open_grid_mut().apply_factory_preset(preset.name);
                }
            }
            ui.close_menu();
        }
    }
    ui.separator();
    if ui.button("Save Patch (*.hdp)...").clicked() {
        if let Some(track) = app.current_tracks().get(track_idx) {
            if let Some(slot) = track.effects.get(slot_idx) {
                if let Some(ref g) = slot.open_grid {
                    save_opengrid_preset(g);
                }
            }
        }
        ui.close_menu();
    }
    if ui.button("Load Patch (*.hdp)...").clicked() {
        if let Some(track) = app.current_tracks_mut().get_mut(track_idx) {
            if let Some(slot) = track.effects.get_mut(slot_idx) {
                load_opengrid_preset(slot.open_grid_mut());
            }
        }
        ui.close_menu();
    }
}

// =============================================================================
// 1. COMPACT DSP RACK CARD (Minimap + Modulators + Pitch / Glide / NoteFX / Out)
// =============================================================================

pub fn render_rack_card(ui: &mut Ui, app: &mut AppState, track_idx: usize, slot_idx: usize) {
    let eng_track = track_idx.saturating_sub(1);
    let (meter_l, meter_r) = app
        .engine_handle
        .as_ref()
        .and_then(|arc| arc.try_lock().ok())
        .map(|eng| eng.open_grid_meter(eng_track))
        .unwrap_or((0.0, 0.0));

    let editor_is_open = app.open_grid_editor == Some((track_idx, slot_idx));

    let mut grid = {
        let Some(track) = app.current_tracks_mut().get_mut(track_idx) else {
            return;
        };
        let Some(slot) = track.effects.get_mut(slot_idx) else {
            return;
        };
        slot.open_grid_mut().clone()
    };

    let mut dirty = false;
    let mut toggle_editor = false;
    let mut open_editor = false;
    let mut request_add_modulator = false;
    let mut request_insert_device = false;

    let avail = ui.available_rect_before_wrap();
    let total_h = avail.height().max(215.0);
    let gap = 4.0;
    let mut cursor_x = avail.min.x;
    let y0 = avail.min.y;
    let p = ui.painter().clone();

    // Bitwig-style Device Modulator Bay (columnas dinámicas de 3 slots + botón '+')
    if grid.modulators_open {
        let bay_w =
            crate::views::hikaru_native_plugins::hikaru_openmodulation::render_device_modulator_bay(
                ui,
                pos2(cursor_x, y0),
                total_h,
                track_idx,
                slot_idx,
                &mut request_add_modulator,
            );
        cursor_x += bay_w;
    }

    // Main column: Top Live Minimap + Bottom Global Controls
    let main_w = (avail.max.x - cursor_x).max(170.0);
    let footer_h = 66.0;
    let map_h = (total_h - footer_h - gap).max(120.0);
    let map_rect = Rect::from_min_size(pos2(cursor_x, y0), vec2(main_w, map_h));
    let footer_rect = Rect::from_min_size(pos2(cursor_x, map_rect.max.y + gap), vec2(main_w, footer_h));

    // ── LIVE INTERACTIVE MINIMAP ─────────────────────────────────────────
    p.rect_filled(map_rect, 3.0, PANEL_BG_DARK);
    p.rect_stroke(
        map_rect,
        3.0,
        Stroke::new(
            1.0_f32,
            if editor_is_open {
                VIEWPORT_BLUE
            } else {
                PANEL_BORDER
            },
        ),
        egui::StrokeKind::Inside,
    );

    // Subtle minimap grid lines
    let grid_step = 14.0;
    let grid_col = Color32::from_rgb(0x1B, 0x1E, 0x24);
    let mut gx = map_rect.min.x + grid_step;
    while gx < map_rect.max.x {
        p.line_segment([pos2(gx, map_rect.min.y + 1.0), pos2(gx, map_rect.max.y - 1.0)], Stroke::new(0.6_f32, grid_col));
        gx += grid_step;
    }
    let mut gy = map_rect.min.y + grid_step;
    while gy < map_rect.max.y {
        p.line_segment([pos2(map_rect.min.x + 1.0, gy), pos2(map_rect.max.x - 1.0, gy)], Stroke::new(0.6_f32, grid_col));
        gy += grid_step;
    }

    // Compute minimap world-to-screen mapping
    let (mut bmin_x, mut bmin_y, mut bmax_x, mut bmax_y) = grid.content_bounds();
    // Expand bounds gently to include current viewport if open
    let vw = grid.last_canvas_size[0].max(420.0) / grid.zoom.max(0.25);
    let vh = grid.last_canvas_size[1].max(220.0) / grid.zoom.max(0.25);
    if editor_is_open {
        bmin_x = bmin_x.min(grid.pan[0] - 40.0);
        bmin_y = bmin_y.min(grid.pan[1] - 40.0);
        bmax_x = bmax_x.max(grid.pan[0] + vw + 40.0);
        bmax_y = bmax_y.max(grid.pan[1] + vh + 40.0);
    }
    let world_w = (bmax_x - bmin_x).max(600.0);
    let world_h = (bmax_y - bmin_y).max(360.0);
    let inner_map = map_rect.shrink(6.0);
    let scale = (inner_map.width() / world_w).min(inner_map.height() / world_h);
    let ox = inner_map.min.x + (inner_map.width() - world_w * scale) * 0.5 - bmin_x * scale;
    let oy = inner_map.min.y + (inner_map.height() - world_h * scale) * 0.5 - bmin_y * scale;
    let w2m = |wx: f32, wy: f32| pos2(ox + wx * scale, oy + wy * scale);

    // Paint miniature nodes & cables on minimap
    let mut node_centers: HashMap<u32, Pos2> = HashMap::new();
    for nd in &grid.patch.nodes {
        let [nw, nh] = nd.kind.default_size();
        let tl = w2m(nd.pos[0], nd.pos[1]);
        let br = w2m(nd.pos[0] + nw, nd.pos[1] + nh);
        let nr = Rect::from_min_max(tl, br).intersect(map_rect.shrink(2.0));
        let col = cat_color(nd.kind.category());
        let fill = Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 155);
        p.rect_filled(nr, 1.5, fill);
        p.rect_stroke(nr, 1.5, Stroke::new(0.8_f32, col), egui::StrokeKind::Inside);
        node_centers.insert(nd.id, nr.center());
    }
    for c in &grid.patch.cables {
        if let (Some(&a), Some(&b)) = (node_centers.get(&c.from_node), node_centers.get(&c.to_node)) {
            p.line_segment([a, b], Stroke::new(0.9_f32, Color32::from_rgba_unmultiplied(230, 234, 240, 120)));
        }
    }

    // Glowing sky-blue viewport rectangle when OpenGrid editor is open!
    if editor_is_open {
        let vp_tl = w2m(grid.pan[0], grid.pan[1]);
        let vp_br = w2m(grid.pan[0] + vw, grid.pan[1] + vh);
        let vp_rect = Rect::from_min_max(vp_tl, vp_br).intersect(map_rect.shrink(1.0));
        p.rect_filled(
            vp_rect,
            2.0,
            Color32::from_rgba_unmultiplied(0x36, 0x9E, 0xE8, 42),
        );
        p.rect_stroke(vp_rect, 2.0, Stroke::new(1.5_f32, VIEWPORT_BLUE), egui::StrokeKind::Inside);
    }

    // Minimap interaction: click opens, drag pans when open, double-click toggles
    let map_resp = ui
        .interact(
            map_rect,
            ui.id().with(("opengrid_minimap", track_idx, slot_idx)),
            Sense::click_and_drag(),
        )
        .on_hover_text(if editor_is_open {
            "Drag to scroll OpenGrid canvas | Double-click to hide Grid"
        } else {
            "Click to open Hikaru OpenGrid modular patch editor"
        });
    if map_resp.hovered() {
        ui.ctx().set_cursor_icon(if editor_is_open {
            CursorIcon::Grab
        } else {
            CursorIcon::PointingHand
        });
    }
    if map_resp.double_clicked() {
        toggle_editor = true;
    } else if map_resp.clicked() && !editor_is_open {
        open_editor = true;
    } else if map_resp.dragged() && editor_is_open && scale > 1.0e-4 {
        let delta = map_resp.drag_delta();
        grid.pan[0] += delta.x / scale;
        grid.pan[1] += delta.y / scale;
    }

    // ── BOTTOM FOOTER: Pitch | Glide + L | Note FX / FX | Out + Meter ────
    p.rect_filled(footer_rect, 3.0, PANEL_BG);
    p.rect_stroke(footer_rect, 3.0, Stroke::new(1.0_f32, PANEL_BORDER), egui::StrokeKind::Inside);

    let ky = footer_rect.min.y + 24.0;
    // Pitch knob
    let pit_r = Rect::from_center_size(pos2(footer_rect.min.x + 22.0, ky), vec2(26.0, 26.0));
    let mut pit_ui = ui.new_child(egui::UiBuilder::new().max_rect(pit_r));
    let (pit_resp, pit_chg) = rotary_knob(
        &mut pit_ui,
        26.0,
        &mut grid.patch.global_pitch_st,
        -24.0..=24.0,
        0.0,
        KNOB_WHITE,
        true,
    );
    if pit_chg {
        grid.patch.global_pitch_st = (grid.patch.global_pitch_st * 2.0).round() * 0.5;
        dirty = true;
    }
    let _ = pit_resp.on_hover_text(format!("Global Pitch: {:+.1} st", grid.patch.global_pitch_st));
    p.text(pos2(pit_r.center().x, footer_rect.max.y - 10.0), Align2::CENTER_CENTER, "Pitch", fonts::regular(8.5), Color32::from_rgb(0xC6, 0xCA, 0xD2));

    // Glide knob + L button
    let gld_r = Rect::from_center_size(pos2(footer_rect.min.x + 56.0, ky), vec2(26.0, 26.0));
    let mut gld_ui = ui.new_child(egui::UiBuilder::new().max_rect(gld_r));
    let (gld_resp, gld_chg) = rotary_knob(
        &mut gld_ui,
        26.0,
        &mut grid.patch.glide_ms,
        0.0..=1500.0,
        0.0,
        KNOB_WHITE,
        false,
    );
    if gld_chg {
        dirty = true;
    }
    let _ = gld_resp.on_hover_text(format!("Glide: {:.0} ms", grid.patch.glide_ms));
    p.text(pos2(gld_r.center().x - 3.0, footer_rect.max.y - 10.0), Align2::CENTER_CENTER, "Glide", fonts::regular(8.5), Color32::from_rgb(0xC6, 0xCA, 0xD2));

    let leg_r = Rect::from_min_size(pos2(gld_r.max.x + 2.0, footer_rect.max.y - 17.0), vec2(12.0, 12.0));
    let leg_resp = ui
        .interact(leg_r, ui.id().with(("grid_legato", slot_idx)), Sense::click())
        .on_hover_text("Toggle Legato Glide");
    p.rect_filled(
        leg_r,
        2.0,
        if grid.patch.legato { MOD_ORANGE } else { Color32::from_rgb(0x34, 0x37, 0x40) },
    );
    p.text(
        leg_r.center(),
        Align2::CENTER_CENTER,
        "L",
        fonts::semibold(7.5),
        if grid.patch.legato { Color32::from_rgb(0x14, 0x10, 0x10) } else { theme::TEXT_SECONDARY },
    );
    if leg_resp.clicked() {
        grid.patch.legato = !grid.patch.legato;
        dirty = true;
    }

    // Note FX & FX stacked pills
    let pill_x = footer_rect.min.x + 88.0;
    let pill_w = (footer_rect.width() - 146.0).clamp(44.0, 62.0);
    for (i, lbl) in ["Note FX", "FX"].iter().enumerate() {
        let pr = Rect::from_min_size(
            pos2(pill_x, footer_rect.min.y + 8.0 + i as f32 * 26.0),
            vec2(pill_w, 21.0),
        );
        let resp = ui
            .interact(pr, ui.id().with(("grid_fx_pill", slot_idx, i)), Sense::click())
            .on_hover_text(format!("Insert {} device on track", lbl));
        p.rect_filled(
            pr,
            2.5,
            if resp.hovered() { Color32::from_rgb(0x3B, 0x3F, 0x48) } else { Color32::from_rgb(0x2E, 0x31, 0x39) },
        );
        p.rect_stroke(pr, 2.5, Stroke::new(1.0_f32, Color32::from_rgb(0x48, 0x4C, 0x56)), egui::StrokeKind::Inside);
        p.text(pr.center(), Align2::CENTER_CENTER, *lbl, fonts::semibold(8.5), KNOB_WHITE);
        if resp.clicked() {
            request_insert_device = true;
        }
    }

    // Out knob (red/coral ring) + vertical stereo meter
    let out_r = Rect::from_center_size(pos2(footer_rect.max.x - 32.0, ky), vec2(28.0, 28.0));
    let mut out_ui = ui.new_child(egui::UiBuilder::new().max_rect(out_r));
    let (out_resp, out_chg) = rotary_knob(
        &mut out_ui,
        28.0,
        &mut grid.patch.output_gain,
        0.0..=2.0,
        0.85,
        VOICE_CORAL,
        false,
    );
    if out_chg {
        dirty = true;
    }
    let _ = out_resp.on_hover_text(format!("Output Gain: {:.0}%", grid.patch.output_gain * 100.0));
    p.text(pos2(out_r.center().x, footer_rect.max.y - 10.0), Align2::CENTER_CENTER, "Out", fonts::semibold(8.5), Color32::from_rgb(0xD8, 0xDC, 0xE4));

    let m_rect = Rect::from_min_max(
        pos2(footer_rect.max.x - 10.0, footer_rect.min.y + 6.0),
        pos2(footer_rect.max.x - 3.0, footer_rect.max.y - 6.0),
    );
    paint_vertical_meter(&p, m_rect, meter_l, meter_r);

    ui.allocate_rect(avail, Sense::hover());

    if dirty {
        grid.mark_dirty();
    }

    if let Some(track) = app.current_tracks_mut().get_mut(track_idx) {
        if let Some(slot) = track.effects.get_mut(slot_idx) {
            slot.open_grid = Some(grid);
        }
    }

    if toggle_editor {
        app.open_grid_editor = if editor_is_open { None } else { Some((track_idx, slot_idx)) };
    } else if open_editor {
        app.open_grid_editor = Some((track_idx, slot_idx));
    }
    if request_add_modulator {
        app.popup_browser.open_insert_modulator(track_idx, slot_idx);
    }
    if request_insert_device {
        app.popup_browser.open_insert_device(track_idx);
    }
}

fn paint_vertical_meter(p: &egui::Painter, m_rect: Rect, meter_l: f32, meter_r: f32) {
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
                MOD_ORANGE
            } else {
                FEG_GREEN
            };
            p.rect_filled(bar, 1.0, col);
        }
    }
}

// =============================================================================
// 2. FULL INTERACTIVE OPENGRID PATCH EDITOR PANEL (Central Workspace)
// =============================================================================

pub fn render_editor_panel(ui: &mut Ui, app: &mut AppState, track_idx: usize, slot_idx: usize) {
    let eng_track = track_idx.saturating_sub(1);
    let (meter_l, meter_r, scope_samples) = app
        .engine_handle
        .as_ref()
        .and_then(|arc| arc.try_lock().ok())
        .map(|eng| {
            let (ml, mr) = eng.open_grid_meter(eng_track);
            let sc = eng.open_grid_scope(eng_track);
            (ml, mr, sc)
        })
        .unwrap_or_else(|| (0.0, 0.0, Vec::new()));

    let track_name = app
        .current_tracks()
        .get(track_idx)
        .map(|t| t.name.clone())
        .unwrap_or_else(|| "Track".to_string());

    let Some(mut grid) = app
        .current_tracks_mut()
        .get_mut(track_idx)
        .and_then(|t| t.effects.get_mut(slot_idx))
        .filter(|s| s.is_open_grid())
        .map(|s| s.open_grid_mut().clone())
    else {
        app.open_grid_editor = None;
        return;
    };

    let mut dirty = false;
    let mut close_editor = false;

    let full_rect = ui.available_rect_before_wrap();
    let p = ui.painter().clone();
    p.rect_filled(full_rect, 0.0, Color32::from_rgb(0x16, 0x18, 0x1C));

    // ── A. TOP HEADER BAR (26px) ─────────────────────────────────────────
    let hdr_h = 26.0;
    let hdr_rect = Rect::from_min_size(full_rect.min, vec2(full_rect.width(), hdr_h));
    p.rect_filled(hdr_rect, 0.0, Color32::from_rgb(0x28, 0x2A, 0x30));
    p.line_segment(
        [pos2(hdr_rect.min.x, hdr_rect.max.y), pos2(hdr_rect.max.x, hdr_rect.max.y)],
        Stroke::new(1.0_f32, Color32::from_rgb(0x14, 0x16, 0x1A)),
    );

    // Left: Factory Preset selector pill + module count
    let preset_r = Rect::from_min_size(pos2(hdr_rect.min.x + 8.0, hdr_rect.min.y + 4.0), vec2(138.0, 18.0));
    let preset_resp = ui
        .interact(preset_r, ui.id().with(("og_hdr_preset", slot_idx)), Sense::click())
        .on_hover_text("Load OpenGrid factory patch");
    p.rect_filled(
        preset_r,
        2.5,
        if preset_resp.hovered() { Color32::from_rgb(0x3E, 0x42, 0x4B) } else { Color32::from_rgb(0x32, 0x35, 0x3D) },
    );
    p.text(
        pos2(preset_r.min.x + 6.0, preset_r.center().y),
        Align2::LEFT_CENTER,
        format!("{} ▾", grid.preset_name),
        fonts::semibold(9.5),
        theme::ACCENT_ORANGE,
    );
    let preset_popup_id = ui.id().with(("og_hdr_preset_pop", slot_idx));
    if preset_resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(preset_popup_id));
    }
    egui::popup_below_widget(
        ui,
        preset_popup_id,
        &preset_resp,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(155.0);
            for fp in &GRID_FACTORY_PRESETS {
                if ui.button(format!("{}  [{}]", fp.name, fp.category)).clicked() {
                    grid.apply_factory_preset(fp.name);
                    dirty = true;
                    ui.close_menu();
                }
            }
        },
    );

    p.text(
        pos2(preset_r.max.x + 10.0, hdr_rect.center().y),
        Align2::LEFT_CENTER,
        format!(
            "{} modules · {} cables",
            grid.patch.nodes.len(),
            grid.patch.cables.len()
        ),
        fonts::regular(9.0),
        theme::TEXT_MUTED,
    );

    // Center title
    p.text(
        hdr_rect.center(),
        Align2::CENTER_CENTER,
        format!("{track_name} / Hikaru OpenGrid"),
        fonts::semibold(11.0),
        KNOB_WHITE,
    );

    // Right controls: Zoom % | Fit | Close ×
    let close_r = Rect::from_min_size(pos2(hdr_rect.max.x - 24.0, hdr_rect.min.y + 4.0), vec2(18.0, 18.0));
    let fit_r = Rect::from_min_size(pos2(close_r.min.x - 40.0, hdr_rect.min.y + 4.0), vec2(34.0, 18.0));
    let zoom_r = Rect::from_min_size(pos2(fit_r.min.x - 46.0, hdr_rect.min.y + 4.0), vec2(42.0, 18.0));

    p.rect_filled(zoom_r, 2.0, Color32::from_rgb(0x1E, 0x20, 0x26));
    p.text(
        zoom_r.center(),
        Align2::CENTER_CENTER,
        format!("{:.0}%", grid.zoom * 100.0),
        fonts::regular(9.0),
        Color32::from_rgb(0xC6, 0xCA, 0xD2),
    );
    let zoom_resp = ui
        .interact(zoom_r, ui.id().with(("og_zoom_rst", slot_idx)), Sense::click())
        .on_hover_text("Reset zoom to 100%");
    if zoom_resp.clicked() {
        grid.zoom = 1.0;
    }

    let fit_resp = ui
        .interact(fit_r, ui.id().with(("og_fit_btn", slot_idx)), Sense::click())
        .on_hover_text("Fit entire patch in view");
    p.rect_filled(
        fit_r,
        2.0,
        if fit_resp.hovered() { Color32::from_rgb(0x42, 0x46, 0x50) } else { Color32::from_rgb(0x32, 0x35, 0x3D) },
    );
    p.text(fit_r.center(), Align2::CENTER_CENTER, "Fit", fonts::semibold(9.0), KNOB_WHITE);
    if fit_resp.clicked() {
        grid.fit_view();
    }

    let close_resp = ui
        .interact(close_r, ui.id().with(("og_close_btn", slot_idx)), Sense::click())
        .on_hover_text("Close OpenGrid Editor (Esc)");
    p.rect_filled(
        close_r,
        2.0,
        if close_resp.hovered() { Color32::from_rgb(0x6E, 0x2C, 0x2E) } else { Color32::from_rgb(0x34, 0x37, 0x40) },
    );
    p.text(close_r.center(), Align2::CENTER_CENTER, "×", fonts::semibold(12.0), KNOB_WHITE);
    if close_resp.clicked() {
        close_editor = true;
    }

    // ── B. TOP MODULE PALETTE STRIP (82px) ───────────────────────────────
    let pal_h = 82.0;
    let pal_rect = Rect::from_min_size(pos2(full_rect.min.x, hdr_rect.max.y), vec2(full_rect.width(), pal_h));
    p.rect_filled(pal_rect, 0.0, Color32::from_rgb(0x1D, 0x1F, 0x24));
    p.line_segment(
        [pos2(pal_rect.min.x, pal_rect.max.y), pos2(pal_rect.max.x, pal_rect.max.y)],
        Stroke::new(1.0_f32, Color32::from_rgb(0x2E, 0x31, 0x39)),
    );

    // Left 4x4 category grid (224px wide)
    let cat_area_w = 224.0;
    let cell_w = (cat_area_w - 14.0) / 4.0;
    let cell_h = (pal_h - 10.0) / 4.0;
    for (row_i, row) in GridCategory::GRID_4X4.iter().enumerate() {
        for (col_i, &cat) in row.iter().enumerate() {
            let cr = Rect::from_min_size(
                pos2(pal_rect.min.x + 6.0 + col_i as f32 * (cell_w + 1.0), pal_rect.min.y + 4.0 + row_i as f32 * (cell_h + 1.0)),
                vec2(cell_w, cell_h - 1.0),
            );
            let active = grid.selected_category == cat;
            let resp = ui.interact(cr, ui.id().with(("og_cat", row_i, col_i)), Sense::click());
            if active {
                p.rect_filled(cr, 2.0, Color32::from_rgb(0x3B, 0x3E, 0x48));
                p.rect_filled(Rect::from_min_size(cr.min, vec2(2.5, cr.height())), 1.0, cat_color(cat));
            } else if resp.hovered() {
                p.rect_filled(cr, 2.0, Color32::from_rgb(0x2A, 0x2D, 0x34));
            }
            p.text(
                pos2(cr.min.x + 5.0, cr.center().y),
                Align2::LEFT_CENTER,
                cat.label(),
                if active { fonts::semibold(8.5) } else { fonts::regular(8.5) },
                if active { Color32::WHITE } else { Color32::from_rgb(0xB4, 0xB8, 0xC2) },
            );
            if resp.clicked() {
                grid.selected_category = cat;
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
        }
    }

    p.line_segment(
        [pos2(pal_rect.min.x + cat_area_w + 2.0, pal_rect.min.y + 4.0), pos2(pal_rect.min.x + cat_area_w + 2.0, pal_rect.max.y - 4.0)],
        Stroke::new(1.0_f32, Color32::from_rgb(0x2F, 0x32, 0x3B)),
    );

    // Right horizontal scrollable Module Shelf for selected category
    let shelf_rect = Rect::from_min_max(
        pos2(pal_rect.min.x + cat_area_w + 8.0, pal_rect.min.y + 4.0),
        pos2(pal_rect.max.x - 6.0, pal_rect.max.y - 4.0),
    );
    let mut shelf_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(shelf_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    shelf_ui.set_clip_rect(shelf_rect);
    let cat_modules = GridModuleKind::in_category(grid.selected_category);
    let shelf_accent = cat_color(grid.selected_category);
    egui::ScrollArea::horizontal()
        .id_salt(("og_palette_shelf", track_idx, slot_idx))
        .auto_shrink([false; 2])
        .show(&mut shelf_ui, |ui| {
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 0.0);
                for kind in cat_modules {
                    let (tile_r, tile_resp) = ui.allocate_exact_size(vec2(68.0, 64.0), Sense::click());
                    let tp = ui.painter();
                    let bg = if tile_resp.hovered() {
                        Color32::from_rgb(0x2D, 0x31, 0x3A)
                    } else {
                        Color32::from_rgb(0x22, 0x25, 0x2B)
                    };
                    tp.rect_filled(tile_r, 3.0, bg);
                    tp.rect_stroke(
                        tile_r,
                        3.0,
                        Stroke::new(
                            1.0_f32,
                            if tile_resp.hovered() { shelf_accent } else { Color32::from_rgb(0x36, 0x3A, 0x44) },
                        ),
                        egui::StrokeKind::Inside,
                    );
                    // Mini visual icon box
                    let icon_r = Rect::from_min_size(pos2(tile_r.min.x + 8.0, tile_r.min.y + 6.0), vec2(52.0, 34.0));
                    tp.rect_filled(icon_r, 2.0, Color32::from_rgb(0x14, 0x16, 0x1B));
                    paint_palette_glyph(tp, icon_r.shrink(4.0), kind, shelf_accent);

                    // Module name
                    let name = kind.display_name();
                    let short = if name.len() > 11 {
                        format!("{}…", &name[..10])
                    } else {
                        name.to_string()
                    };
                    tp.text(
                        pos2(tile_r.center().x, tile_r.max.y - 11.0),
                        Align2::CENTER_CENTER,
                        short,
                        fonts::regular(8.5),
                        if tile_resp.hovered() { Color32::WHITE } else { Color32::from_rgb(0xC8, 0xCC, 0xD4) },
                    );
                    if tile_resp.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                    if tile_resp
                        .on_hover_text(format!("Click to add {} to OpenGrid canvas", kind.display_name()))
                        .clicked()
                    {
                        grid.add_module(kind);
                        dirty = true;
                    }
                }
            });
        });

    // ── C. INFINITE ZOOMABLE / PANNABLE GRID CANVAS ──────────────────────
    let canvas_rect = Rect::from_min_max(
        pos2(full_rect.min.x, pal_rect.max.y),
        full_rect.max,
    );
    grid.last_canvas_size = [canvas_rect.width().max(200.0), canvas_rect.height().max(120.0)];

    let mut canvas_ui = ui.new_child(egui::UiBuilder::new().max_rect(canvas_rect));
    canvas_ui.set_clip_rect(canvas_rect);
    let cp = canvas_ui.painter().clone();
    cp.rect_filled(canvas_rect, 0.0, Color32::from_rgb(0x10, 0x12, 0x16));

    // Background interaction: pan (drag) & wheel zoom
    let bg_resp = canvas_ui.interact(
        canvas_rect,
        canvas_ui.id().with(("og_canvas_bg", track_idx, slot_idx)),
        Sense::click_and_drag(),
    );

    // Zoom with scroll wheel when pointer is inside canvas
    if let Some(hover_pos) = canvas_ui.ctx().input(|i| i.pointer.hover_pos()) {
        if canvas_rect.contains(hover_pos) {
            let scroll_y = canvas_ui.ctx().input(|i| i.raw_scroll_delta.y);
            if scroll_y.abs() > 0.5 {
                let old_z = grid.zoom;
                let factor = (1.0 + scroll_y * 0.0018).clamp(0.82, 1.22);
                let new_z = (old_z * factor).clamp(0.45, 2.2);
                // Keep world coordinate under cursor fixed
                let world_x = grid.pan[0] + (hover_pos.x - canvas_rect.min.x) / old_z;
                let world_y = grid.pan[1] + (hover_pos.y - canvas_rect.min.y) / old_z;
                grid.zoom = new_z;
                grid.pan[0] = world_x - (hover_pos.x - canvas_rect.min.x) / new_z;
                grid.pan[1] = world_y - (hover_pos.y - canvas_rect.min.y) / new_z;
            }
        }
    }

    // Pan with middle mouse anywhere on canvas, or left-drag on empty canvas
    let mid_drag = canvas_ui.ctx().input(|i| i.pointer.middle_down());
    if (bg_resp.dragged() || (mid_drag && canvas_rect.contains(canvas_ui.ctx().input(|i| i.pointer.interact_pos().unwrap_or(Pos2::ZERO)))))
        && grid.dragging_cable.is_none()
    {
        let d = canvas_ui.ctx().input(|i| i.pointer.delta());
        if d.length_sq() > 0.0 {
            grid.pan[0] -= d.x / grid.zoom;
            grid.pan[1] -= d.y / grid.zoom;
        }
    }
    if bg_resp.clicked() {
        grid.selected_node = None;
        grid.dragging_cable = None;
    }

    let z = grid.zoom;
    let pan = grid.pan;
    let w2s = |wx: f32, wy: f32| {
        pos2(
            canvas_rect.min.x + (wx - pan[0]) * z,
            canvas_rect.min.y + (wy - pan[1]) * z,
        )
    };

    // Draw snap grid lines
    let minor_step = 24.0 * z;
    if minor_step >= 8.0 {
        let start_wx = (pan[0] / 24.0).floor() * 24.0;
        let end_wx = pan[0] + canvas_rect.width() / z + 24.0;
        let mut wx = start_wx;
        while wx <= end_wx {
            let sx = w2s(wx, 0.0).x;
            let major = ((wx / 24.0).round() as i32).rem_euclid(4) == 0;
            cp.line_segment(
                [pos2(sx, canvas_rect.min.y), pos2(sx, canvas_rect.max.y)],
                Stroke::new(
                    if major { 0.9_f32 } else { 0.5_f32 },
                    if major { Color32::from_rgb(0x20, 0x23, 0x2B) } else { Color32::from_rgb(0x17, 0x1A, 0x20) },
                ),
            );
            wx += 24.0;
        }
        let start_wy = (pan[1] / 24.0).floor() * 24.0;
        let end_wy = pan[1] + canvas_rect.height() / z + 24.0;
        let mut wy = start_wy;
        while wy <= end_wy {
            let sy = w2s(0.0, wy).y;
            let major = ((wy / 24.0).round() as i32).rem_euclid(4) == 0;
            cp.line_segment(
                [pos2(canvas_rect.min.x, sy), pos2(canvas_rect.max.x, sy)],
                Stroke::new(
                    if major { 0.9_f32 } else { 0.5_f32 },
                    if major { Color32::from_rgb(0x20, 0x23, 0x2B) } else { Color32::from_rgb(0x17, 0x1A, 0x20) },
                ),
            );
            wy += 24.0;
        }
    }

    // Precompute screen positions of all input & output jacks for Bezier cables
    let mut in_jacks: HashMap<(u32, usize), (Pos2, GridSignalKind)> = HashMap::new();
    let mut out_jacks: HashMap<(u32, usize), (Pos2, GridSignalKind)> = HashMap::new();
    for nd in &grid.patch.nodes {
        let [nw, nh] = nd.kind.default_size();
        let nr = Rect::from_min_max(w2s(nd.pos[0], nd.pos[1]), w2s(nd.pos[0] + nw, nd.pos[1] + nh));
        let hdr_h_s = 20.0 * z;
        let body_top = nr.min.y + hdr_h_s;
        let body_h = (nr.max.y - body_top).max(16.0);

        let ins = nd.kind.inputs();
        for (pi, spec) in ins.iter().enumerate() {
            let frac = (pi as f32 + 1.0) / (ins.len() as f32 + 1.0);
            in_jacks.insert((nd.id, pi), (pos2(nr.min.x, body_top + body_h * frac), spec.kind));
        }
        let outs = nd.kind.outputs();
        for (pi, spec) in outs.iter().enumerate() {
            let frac = (pi as f32 + 1.0) / (outs.len() as f32 + 1.0);
            out_jacks.insert((nd.id, pi), (pos2(nr.max.x, body_top + body_h * frac), spec.kind));
        }
    }

    // ── RENDER MODULE NODES ──────────────────────────────────────────────
    let mut node_to_delete: Option<u32> = None;
    let mut node_to_dup: Option<u32> = None;
    let mut pending_connect: Option<(u32, usize, u32, usize)> = None;
    let mut pending_disconn_in: Option<(u32, usize)> = None;
    let mut pending_disconn_out: Option<(u32, usize)> = None;

    let node_count = grid.patch.nodes.len();
    for ni in 0..node_count {
        let nd_id = grid.patch.nodes[ni].id;
        let nd_kind = grid.patch.nodes[ni].kind;
        let nd_pos = grid.patch.nodes[ni].pos;
        let [nw, nh] = nd_kind.default_size();
        let nr = Rect::from_min_max(w2s(nd_pos[0], nd_pos[1]), w2s(nd_pos[0] + nw, nd_pos[1] + nh));
        if !nr.intersects(canvas_rect.expand(40.0)) {
            continue;
        }

        let is_selected = grid.selected_node == Some(nd_id);
        let accent = cat_color(nd_kind.category());
        let bypassed = grid.patch.nodes[ni].bypassed;

        // Node shadow + body
        cp.rect_filled(
            nr.translate(vec2(2.0, 3.0)),
            4.0 * z,
            Color32::from_rgba_unmultiplied(0, 0, 0, 95),
        );
        cp.rect_filled(
            nr,
            4.0 * z,
            if bypassed { Color32::from_rgb(0x1B, 0x1D, 0x22) } else { Color32::from_rgb(0x26, 0x29, 0x30) },
        );
        cp.rect_stroke(
            nr,
            4.0 * z,
            Stroke::new(
                if is_selected { 1.8_f32 } else { 1.0_f32 },
                if is_selected { Color32::WHITE } else { Color32::from_rgb(0x3F, 0x43, 0x4E) },
            ),
            egui::StrokeKind::Inside,
        );

        // Top colored accent header bar (draggable to move node)
        let nhdr_h = (20.0 * z).clamp(12.0, 32.0);
        let nhdr_r = Rect::from_min_size(nr.min, vec2(nr.width(), nhdr_h));
        cp.rect_filled(
            nhdr_r,
            egui::CornerRadius {
                nw: (4.0 * z) as u8,
                ne: (4.0 * z) as u8,
                sw: 0,
                se: 0,
            },
            Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), if bypassed { 25 } else { 52 }),
        );
        cp.line_segment(
            [pos2(nhdr_r.min.x, nhdr_r.max.y), pos2(nhdr_r.max.x, nhdr_r.max.y)],
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 120)),
        );
        cp.text(
            nhdr_r.center(),
            Align2::CENTER_CENTER,
            nd_kind.display_name(),
            fonts::semibold((9.5 * z).clamp(7.0, 15.0)),
            if bypassed { theme::TEXT_MUTED } else { accent },
        );

        let hdr_resp = canvas_ui.interact(
            nhdr_r,
            canvas_ui.id().with(("og_node_hdr", track_idx, slot_idx, nd_id)),
            Sense::click_and_drag(),
        );
        if hdr_resp.clicked() || hdr_resp.drag_started() {
            grid.selected_node = Some(nd_id);
        }
        if hdr_resp.dragged() {
            let d = hdr_resp.drag_delta();
            grid.patch.nodes[ni].pos[0] += d.x / z;
            grid.patch.nodes[ni].pos[1] += d.y / z;
        }
        if hdr_resp.drag_stopped() {
            grid.patch.nodes[ni].pos[0] = (grid.patch.nodes[ni].pos[0] / 12.0).round() * 12.0;
            grid.patch.nodes[ni].pos[1] = (grid.patch.nodes[ni].pos[1] / 12.0).round() * 12.0;
            dirty = true;
        }
        hdr_resp.context_menu(|ui| {
            if ui.button(if bypassed { "Enable Module" } else { "Bypass Module" }).clicked() {
                grid.patch.nodes[ni].bypassed = !bypassed;
                dirty = true;
                ui.close_menu();
            }
            if ui.button("Duplicate Module").clicked() {
                node_to_dup = Some(nd_id);
                ui.close_menu();
            }
            if ui.button("Delete Module (Del)").clicked() {
                node_to_delete = Some(nd_id);
                ui.close_menu();
            }
        });

        // Node inner body (waveform / ADSR curve / meter / interactive knobs)
        let body_r = Rect::from_min_max(
            pos2(nr.min.x + 8.0 * z, nhdr_r.max.y + 3.0 * z),
            pos2(nr.max.x - 8.0 * z, nr.max.y - 4.0 * z),
        );
        if body_r.width() > 20.0 && body_r.height() > 14.0 {
            render_node_body(
                &mut canvas_ui,
                &cp,
                body_r,
                &mut grid.patch.nodes[ni],
                z,
                accent,
                meter_l,
                meter_r,
                &scope_samples,
                &mut dirty,
            );
        }

        // Input jacks (left edge)
        let jack_rad = (5.0 * z).clamp(3.2, 8.0);
        for (pi, spec) in nd_kind.inputs().iter().enumerate() {
            let Some(&(jpos, sk)) = in_jacks.get(&(nd_id, pi)) else { continue };
            let jr = Rect::from_center_size(jpos, vec2(jack_rad * 2.6, jack_rad * 2.6));
            let jresp = canvas_ui
                .interact(jr, canvas_ui.id().with(("og_in", nd_id, pi)), Sense::click())
                .on_hover_text(format!("In: {} (right-click to unplug)", spec.name));
            let col = signal_color(sk);
            let connected = grid.patch.cables.iter().any(|c| c.to_node == nd_id && c.to_port == pi);
            cp.circle_filled(jpos, jack_rad, Color32::from_rgb(0x14, 0x16, 0x1B));
            cp.circle_stroke(
                jpos,
                jack_rad,
                Stroke::new(if jresp.hovered() { 2.0_f32 } else { 1.4_f32 }, col),
            );
            if connected || jresp.hovered() {
                cp.circle_filled(jpos, jack_rad * 0.48, col);
            }
            if jresp.secondary_clicked() {
                pending_disconn_in = Some((nd_id, pi));
            }
            // Complete cable connection on click or pointer release over this input jack
            if let Some((from_n, from_p)) = grid.dragging_cable {
                let released_here = canvas_ui.ctx().input(|i| {
                    i.pointer.any_released()
                        && i.pointer.interact_pos().is_some_and(|pt| jr.expand(6.0).contains(pt))
                });
                if jresp.clicked() || released_here {
                    pending_connect = Some((from_n, from_p, nd_id, pi));
                }
            }
        }

        // Output jacks (right edge)
        for (pi, spec) in nd_kind.outputs().iter().enumerate() {
            let Some(&(jpos, sk)) = out_jacks.get(&(nd_id, pi)) else { continue };
            let jr = Rect::from_center_size(jpos, vec2(jack_rad * 2.6, jack_rad * 2.6));
            let jresp = canvas_ui
                .interact(jr, canvas_ui.id().with(("og_out", nd_id, pi)), Sense::click_and_drag())
                .on_hover_text(format!("Out: {} (drag to an Input jack; right-click to unplug)", spec.name));
            let col = signal_color(sk);
            let connected = grid.patch.cables.iter().any(|c| c.from_node == nd_id && c.from_port == pi);
            cp.circle_filled(jpos, jack_rad, Color32::from_rgb(0x14, 0x16, 0x1B));
            cp.circle_stroke(
                jpos,
                jack_rad,
                Stroke::new(if jresp.hovered() { 2.0_f32 } else { 1.4_f32 }, col),
            );
            if connected || jresp.hovered() {
                cp.circle_filled(jpos, jack_rad * 0.52, col);
            }
            if jresp.drag_started() || jresp.clicked() {
                grid.dragging_cable = Some((nd_id, pi));
            }
            if jresp.secondary_clicked() {
                pending_disconn_out = Some((nd_id, pi));
            }
        }
    }

    // ── RENDER BEZIER PATCH CABLES ON TOP ────────────────────────────────
    for cable in &grid.patch.cables {
        if let (Some(&(p0, sk)), Some(&(p1, _))) = (
            out_jacks.get(&(cable.from_node, cable.from_port)),
            in_jacks.get(&(cable.to_node, cable.to_port)),
        ) {
            paint_bezier_cable(&cp, p0, p1, signal_color(sk), (2.2 * z).clamp(1.3, 3.6));
        }
    }

    // Live dragging cable preview
    if let Some((from_n, from_p)) = grid.dragging_cable {
        if let Some(&(p0, sk)) = out_jacks.get(&(from_n, from_p)) {
            if let Some(cursor) = canvas_ui.ctx().input(|i| i.pointer.hover_pos()) {
                paint_bezier_cable(&cp, p0, cursor, signal_color(sk), (2.4 * z).clamp(1.5, 3.6));
            }
        }
        if pending_connect.is_none()
            && canvas_ui.ctx().input(|i| i.pointer.any_released())
            && !canvas_ui.ctx().input(|i| i.pointer.primary_clicked())
        {
            grid.dragging_cable = None;
        }
    }

    // Apply deferred graph mutations
    if let Some((fn_id, fp, tn_id, tp)) = pending_connect {
        grid.connect_cable(fn_id, fp, tn_id, tp);
        grid.dragging_cable = None;
        dirty = true;
    }
    if let Some((nid, pid)) = pending_disconn_in {
        grid.disconnect_input(nid, pid);
        dirty = true;
    }
    if let Some((nid, pid)) = pending_disconn_out {
        grid.disconnect_output(nid, pid);
        dirty = true;
    }
    if let Some(nid) = node_to_dup {
        let _ = grid.duplicate_node(nid);
        dirty = true;
    }
    if let Some(nid) = node_to_delete {
        grid.remove_node(nid);
        dirty = true;
    }

    // Keyboard shortcuts inside OpenGrid editor (Esc closes, Del deletes selected node)
    let esc_pressed = ui.ctx().input(|i| i.key_pressed(egui::Key::Escape));
    if esc_pressed {
        if grid.dragging_cable.is_some() {
            grid.dragging_cable = None;
        } else {
            close_editor = true;
        }
    }
    let del_pressed = ui
        .ctx()
        .input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace));
    if del_pressed {
        if let Some(sel_id) = grid.selected_node {
            grid.remove_node(sel_id);
            dirty = true;
        }
    }

    ui.allocate_rect(full_rect, Sense::hover());

    if dirty {
        grid.mark_dirty();
    }
    if let Some(track) = app.current_tracks_mut().get_mut(track_idx) {
        if let Some(slot) = track.effects.get_mut(slot_idx) {
            slot.open_grid = Some(grid);
        }
    }
    if close_editor {
        app.open_grid_editor = None;
    }
}

#[allow(clippy::too_many_arguments)]
fn render_node_body(
    ui: &mut Ui,
    cp: &egui::Painter,
    rect: Rect,
    node: &mut GridNode,
    z: f32,
    accent: Color32,
    meter_l: f32,
    meter_r: f32,
    scope: &[f32],
    dirty: &mut bool,
) {
    match node.kind {
        GridModuleKind::AudioOut => {
            // Coral speaker icon + live stereo output meter
            let spk_r = Rect::from_min_size(rect.min + vec2(4.0 * z, 4.0 * z), vec2(rect.width() * 0.55, rect.height() - 8.0 * z));
            let c = spk_r.center();
            let s = (10.0 * z).clamp(5.0, 16.0);
            cp.add(egui::Shape::convex_polygon(
                vec![
                    pos2(c.x - s * 0.7, c.y - s * 0.4),
                    pos2(c.x - s * 0.1, c.y - s * 0.4),
                    pos2(c.x + s * 0.65, c.y - s * 0.9),
                    pos2(c.x + s * 0.65, c.y + s * 0.9),
                    pos2(c.x - s * 0.1, c.y + s * 0.4),
                    pos2(c.x - s * 0.7, c.y + s * 0.4),
                ],
                VOICE_CORAL,
                Stroke::NONE,
            ));
            let mr = Rect::from_min_max(
                pos2(rect.max.x - 10.0 * z, rect.min.y + 3.0 * z),
                pos2(rect.max.x - 2.0 * z, rect.max.y - 3.0 * z),
            );
            paint_vertical_meter(cp, mr, meter_l, meter_r);
        }
        GridModuleKind::Oscilloscope => {
            cp.rect_filled(rect, 2.5, PANEL_BG_DARK);
            let cy = rect.center().y;
            cp.line_segment([pos2(rect.min.x + 2.0, cy), pos2(rect.max.x - 2.0, cy)], Stroke::new(0.7_f32, Color32::from_rgb(0x2B, 0x30, 0x3A)));
            if scope.len() >= 4 {
                let pts: Vec<Pos2> = scope
                    .iter()
                    .enumerate()
                    .map(|(i, &v)| {
                        let t = i as f32 / (scope.len() - 1) as f32;
                        pos2(
                            rect.min.x + 2.0 + t * (rect.width() - 4.0),
                            cy - v.clamp(-1.0, 1.0) * (rect.height() * 0.42),
                        )
                    })
                    .collect();
                cp.add(egui::Shape::line(pts, Stroke::new(1.4_f32, VIEWPORT_BLUE)));
            }
        }
        GridModuleKind::VuMeter => {
            let mr = rect.shrink2(vec2(rect.width() * 0.3, 4.0 * z));
            paint_vertical_meter(cp, mr, meter_l, meter_r);
        }
        GridModuleKind::Wavetable
        | GridModuleKind::Sawtooth
        | GridModuleKind::Pulse
        | GridModuleKind::Sine
        | GridModuleKind::Triangle
        | GridModuleKind::Phase1
        | GridModuleKind::Swarm => {
            // Live oscillator waveform preview (left) + Index knob (right) + bottom Semi/Detune
            let wave_w = (rect.width() * 0.58).max(40.0);
            let wave_h = (rect.height() * 0.62).max(24.0);
            let wave_r = Rect::from_min_size(rect.min, vec2(wave_w, wave_h));
            cp.rect_filled(wave_r, 2.5, PANEL_BG_DARK);
            let osc_t = match node.kind {
                GridModuleKind::Wavetable => OpenPolyOscType::Wavetable,
                GridModuleKind::Sawtooth => OpenPolyOscType::Sawtooth,
                GridModuleKind::Pulse => OpenPolyOscType::Pulse,
                GridModuleKind::Sine => OpenPolyOscType::Sine,
                GridModuleKind::Triangle => OpenPolyOscType::Triangle,
                GridModuleKind::Phase1 => OpenPolyOscType::Phase1,
                _ => OpenPolyOscType::Swarm,
            };
            let pts: Vec<Pos2> = (0..=40)
                .map(|i| {
                    let t = i as f32 / 40.0;
                    let v = eval_osc_preview(osc_t, t, node.params[0], node.params[1], None);
                    pos2(
                        wave_r.min.x + 3.0 + t * (wave_r.width() - 6.0),
                        wave_r.center().y - v * (wave_r.height() * 0.36),
                    )
                })
                .collect();
            cp.add(egui::Shape::line(pts, Stroke::new(1.5_f32, accent)));

            // Big Index knob on right
            let kd = (32.0 * z).clamp(16.0, 46.0);
            let kcx = wave_r.max.x + (rect.max.x - wave_r.max.x) * 0.5;
            let kcy = wave_r.center().y - 3.0 * z;
            let kr = Rect::from_center_size(pos2(kcx, kcy), vec2(kd, kd));
            let mut kui = ui.new_child(egui::UiBuilder::new().max_rect(kr));
            let (resp, chg) = rotary_knob(&mut kui, kd, &mut node.params[0], 0.0..=1.0, 0.0, KNOB_WHITE, false);
            if chg {
                *dirty = true;
            }
            let _ = resp.on_hover_text(format!("Index: {:.0}%", node.params[0] * 100.0));
            cp.text(
                pos2(kcx, kr.max.y + 6.0 * z),
                Align2::CENTER_CENTER,
                "Index",
                fonts::regular((8.0 * z).clamp(6.5, 11.0)),
                Color32::from_rgb(0xC0, 0xC4, 0xCE),
            );

            // Bottom readout bar: 1:1 | Semi | Detune
            let bot_y = rect.max.y - 10.0 * z;
            cp.text(
                pos2(rect.min.x + 4.0, bot_y),
                Align2::LEFT_CENTER,
                format!("1:1  {:+.0} st  ±{:.1}Hz", node.params[2], node.params[3].abs()),
                fonts::regular((8.0 * z).clamp(6.5, 11.0)),
                Color32::from_rgb(0xE8, 0xA8, 0x42),
            );
        }
        GridModuleKind::Adsr => {
            // Live ADSR curve (top) + A D S R knobs (bottom)
            let graph_h = (rect.height() * 0.44).max(20.0);
            let gr = Rect::from_min_size(rect.min, vec2(rect.width(), graph_h));
            cp.rect_filled(gr, 2.5, PANEL_BG_DARK);
            let a_n = val_to_norm(node.params[0], 0.5, 5000.0, true);
            let d_n = val_to_norm(node.params[1], 1.0, 8000.0, true);
            let s_v = node.params[2].clamp(0.0, 1.0);
            let r_n = val_to_norm(node.params[3], 1.0, 8000.0, true);
            let pad = 4.0 * z;
            let gw = (gr.width() - pad * 2.0).max(10.0);
            let gh = (gr.height() - pad * 2.0).max(8.0);
            let x0 = gr.min.x + pad;
            let yb = gr.max.y - pad;
            let yt = gr.min.y + pad;
            let xa = x0 + (0.05 + a_n * 0.24) * gw;
            let xd = xa + (0.06 + d_n * 0.24) * gw;
            let xs = xd + 0.18 * gw;
            let xr = (xs + (0.05 + r_n * 0.22) * gw).min(x0 + gw);
            let ys = yb - s_v * gh;
            let pts = vec![pos2(x0, yb), pos2(xa, yt), pos2(xd, ys), pos2(xs, ys), pos2(xr, yb)];
            cp.add(egui::Shape::line(pts, Stroke::new(1.5_f32, accent)));

            let knob_area = Rect::from_min_max(pos2(rect.min.x, gr.max.y + 4.0 * z), rect.max);
            render_generic_param_knobs(ui, cp, knob_area, node, z, accent, dirty);
        }
        _ => {
            if node.kind.param_specs().is_empty() {
                cp.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    node.kind.category().label(),
                    fonts::regular((8.5 * z).clamp(6.5, 11.5)),
                    theme::TEXT_MUTED,
                );
            } else {
                render_generic_param_knobs(ui, cp, rect, node, z, accent, dirty);
            }
        }
    }
}

fn render_generic_param_knobs(
    ui: &mut Ui,
    cp: &egui::Painter,
    rect: Rect,
    node: &mut GridNode,
    z: f32,
    accent: Color32,
    dirty: &mut bool,
) {
    let specs = node.kind.param_specs();
    let count = specs.len().min(4);
    if count == 0 {
        return;
    }
    let col_w = rect.width() / count as f32;
    let kd = ((col_w * 0.72).min(rect.height() * 0.62)).clamp(14.0, 32.0 * z.max(0.7));
    let ky = rect.center().y - 4.0 * z;

    for (i, sp) in specs.iter().enumerate().take(count) {
        let cx = rect.min.x + col_w * (i as f32 + 0.5);
        let kr = Rect::from_center_size(pos2(cx, ky), vec2(kd, kd));
        let mut norm = val_to_norm(node.params[i], sp.min, sp.max, sp.log_scale);
        let def_n = val_to_norm(sp.default, sp.min, sp.max, sp.log_scale);
        let mut kui = ui.new_child(egui::UiBuilder::new().max_rect(kr));
        let (resp, chg) = rotary_knob(&mut kui, kd, &mut norm, 0.0..=1.0, def_n, accent, sp.bipolar);
        if chg {
            node.params[i] = norm_to_val(norm, sp.min, sp.max, sp.log_scale);
            *dirty = true;
        }
        let _ = resp.on_hover_text(format!("{}: {:.2} {}", sp.name, node.params[i], sp.unit));
        cp.text(
            pos2(cx, (kr.max.y + 6.0 * z).min(rect.max.y - 3.0)),
            Align2::CENTER_CENTER,
            sp.name,
            fonts::regular((7.8 * z).clamp(6.0, 10.5)),
            Color32::from_rgb(0xC2, 0xC6, 0xD0),
        );
    }
}

fn paint_bezier_cable(p: &egui::Painter, p0: Pos2, p1: Pos2, color: Color32, width: f32) {
    let dx = ((p1.x - p0.x).abs() * 0.5).clamp(32.0, 160.0);
    let c1 = pos2(p0.x + dx, p0.y);
    let c2 = pos2(p1.x - dx, p1.y);
    let steps = 22usize;
    let mut pts = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let u = 1.0 - t;
        let x = u * u * u * p0.x + 3.0 * u * u * t * c1.x + 3.0 * u * t * t * c2.x + t * t * t * p1.x;
        let y = u * u * u * p0.y + 3.0 * u * u * t * c1.y + 3.0 * u * t * t * c2.y + t * t * t * p1.y;
        pts.push(pos2(x, y));
    }
    // Subtle outer glow
    p.add(egui::Shape::line(
        pts.clone(),
        Stroke::new(width + 2.4, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 46)),
    ));
    p.add(egui::Shape::line(pts, Stroke::new(width, color)));
}

fn paint_palette_glyph(p: &egui::Painter, r: Rect, kind: GridModuleKind, accent: Color32) {
    let cy = r.center().y;
    match kind.category() {
        GridCategory::Oscillator | GridCategory::Lfo | GridCategory::Phase => {
            let pts: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let t = i as f32 / 16.0;
                    let v = match kind {
                        GridModuleKind::Sawtooth | GridModuleKind::Swarm | GridModuleKind::Phasor => 2.0 * t - 1.0,
                        GridModuleKind::Pulse => if t < 0.5 { 0.85 } else { -0.85 },
                        GridModuleKind::Triangle => 1.0 - 4.0 * (t - 0.5).abs(),
                        GridModuleKind::Noise => ((i * 7 % 11) as f32 / 5.5) - 1.0,
                        _ => (t * TAU).sin(),
                    };
                    pos2(r.min.x + t * r.width(), cy - v * r.height() * 0.4)
                })
                .collect();
            p.add(egui::Shape::line(pts, Stroke::new(1.4_f32, accent)));
        }
        GridCategory::Envelope => {
            let pts = vec![
                pos2(r.min.x, r.max.y),
                pos2(r.min.x + r.width() * 0.22, r.min.y),
                pos2(r.min.x + r.width() * 0.5, r.center().y),
                pos2(r.min.x + r.width() * 0.75, r.center().y),
                pos2(r.max.x, r.max.y),
            ];
            p.add(egui::Shape::line(pts, Stroke::new(1.4_f32, accent)));
        }
        GridCategory::Filter => {
            let pts = vec![
                pos2(r.min.x, r.center().y),
                pos2(r.center().x, r.center().y),
                pos2(r.center().x + 4.0, r.min.y + 2.0),
                pos2(r.max.x, r.max.y),
            ];
            p.add(egui::Shape::line(pts, Stroke::new(1.4_f32, accent)));
        }
        GridCategory::Io => {
            p.circle_stroke(r.center(), (r.height() * 0.34).min(9.0), Stroke::new(1.5_f32, accent));
            p.circle_filled(r.center(), 2.6, accent);
        }
        _ => {
            p.rect_stroke(r.shrink(3.0), 2.0, Stroke::new(1.2_f32, accent), egui::StrokeKind::Inside);
            p.circle_filled(pos2(r.min.x + 3.0, cy), 2.0, accent);
            p.circle_filled(pos2(r.max.x - 3.0, cy), 2.0, accent);
        }
    }
}
