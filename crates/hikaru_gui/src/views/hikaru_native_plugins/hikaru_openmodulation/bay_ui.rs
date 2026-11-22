// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_openmodulation/bay_ui.rs
// Bitwig-style Device Modulator Bay UI (GNU AGPL v3)

use egui::{
    pos2, vec2, Align2, Color32, CursorIcon, Key, Painter, Pos2, Rect, RichText, Sense, Stroke, Ui,
};
use hikaru_dsp::effects::lfo::{evaluate_waveform, LfoSyncMode, LfoWaveform, SyncRate};

use crate::theme;
use crate::ui::bitwig_kit;
use crate::ui::fonts;

use super::bay::{
    get_bay_state, update_bay_state, MathOp, ModulatorKind, ModulatorSlotState, BAY_COL_WIDTH,
    BAY_GAP, BAY_SLOTS_PER_COL,
};
use super::lfo_mod::{
    clear_mapping_mode, clock_time_secs, get_global_transport, is_bay_mapping_active,
    toggle_bay_mapping_mode, ModulationTarget, MOD_ARROW_CYAN,
};

const CELL_BG: Color32 = Color32::from_rgb(0x24, 0x26, 0x2B);
const CELL_WELL_BG: Color32 = Color32::from_rgb(0x16, 0x18, 0x1C);
const CELL_BORDER: Color32 = Color32::from_rgb(0x3B, 0x3E, 0x47);
const CELL_BORDER_ACTIVE: Color32 = Color32::from_rgb(0x52, 0x46, 0x38);
const MOD_ORANGE: Color32 = Color32::from_rgb(0xFF, 0x82, 0x20);
const KNOB_WHITE: Color32 = Color32::from_rgb(0xE6, 0xE8, 0xEC);

fn paint_bay_arrow(p: &Painter, center: Pos2, color: Color32, filled_dot: bool) {
    let dot_c = pos2(center.x - 5.5, center.y);
    let tip_x = center.x + 6.5;
    if filled_dot {
        p.circle_filled(dot_c, 2.3, color);
    } else {
        p.circle_stroke(dot_c, 2.2, Stroke::new(1.15_f32, color));
    }
    p.line_segment(
        [pos2(dot_c.x + 2.4, center.y), pos2(tip_x - 1.5, center.y)],
        Stroke::new(1.25_f32, color),
    );
    p.add(egui::Shape::convex_polygon(
        vec![
            pos2(tip_x, center.y),
            pos2(tip_x - 3.6, center.y - 2.6),
            pos2(tip_x - 3.6, center.y + 2.6),
        ],
        color,
        Stroke::NONE,
    ));
}

/// Dibuja el Device Modulator Bay reutilizable (columnas de 3 slots + botón `+`)
/// en la columna izquierda de cualquier dispositivo nativo.
///
/// Devuelve el ancho consumido en píxeles (incluyendo el gap derecho).
pub fn render_device_modulator_bay(
    ui: &mut Ui,
    origin: Pos2,
    total_h: f32,
    track_idx: usize,
    slot_idx: usize,
    request_open_browser: &mut bool,
) -> f32 {
    if ui.input(|i| i.key_pressed(Key::Escape)) {
        clear_mapping_mode();
    }

    let transport = get_global_transport();
    let time_secs = clock_time_secs();

    let mut bay = get_bay_state(track_idx, slot_idx);
    let mut dirty = false;
    let mut remove_id: Option<u32> = None;
    let mut any_lfo_active = false;

    let cell_h = ((total_h - BAY_GAP * (BAY_SLOTS_PER_COL as f32 - 1.0))
        / BAY_SLOTS_PER_COL as f32)
        .max(56.0);

    let slot_count = bay.slots.len();
    for idx in 0..slot_count {
        let col = idx / BAY_SLOTS_PER_COL;
        let row = idx % BAY_SLOTS_PER_COL;
        let cell_x = origin.x + col as f32 * (BAY_COL_WIDTH + BAY_GAP);
        let cell_y = origin.y + row as f32 * (cell_h + BAY_GAP);
        let cell_rect = Rect::from_min_size(pos2(cell_x, cell_y), vec2(BAY_COL_WIDTH, cell_h));

        let m = &mut bay.slots[idx];
        let outs = m.evaluate(&transport, time_secs);
        if m.enabled
            && matches!(
                m.kind,
                ModulatorKind::ClassicLfo
                    | ModulatorKind::BeatLfo
                    | ModulatorKind::Random
                    | ModulatorKind::SampleAndHold
                    | ModulatorKind::Vibrato
                    | ModulatorKind::Steps
                    | ModulatorKind::ParSeq8
                    | ModulatorKind::Curves
                    | ModulatorKind::Ramp
                    | ModulatorKind::Segments
                    | ModulatorKind::Adsr
                    | ModulatorKind::Ahdsr
                    | ModulatorKind::FourStage
                    | ModulatorKind::AhdOnRelease
                    | ModulatorKind::NoteSidechain
                    | ModulatorKind::WavetableLfo
                    | ModulatorKind::Globals
                    | ModulatorKind::EnvelopeFollower
                    | ModulatorKind::AudioSidechain
                    | ModulatorKind::AudioRate
            )
        {
            any_lfo_active = true;
        }
        let mod_id = m.id;
        let out_labels = m.kind.meta().outputs;
        let num_outs = out_labels.len().clamp(1, 16);
        let any_map_active = (0..num_outs as u8)
            .any(|o| is_bay_mapping_active(track_idx, slot_idx, mod_id, o));

        let border = if any_map_active {
            MOD_ARROW_CYAN
        } else if m.enabled {
            CELL_BORDER_ACTIVE
        } else {
            CELL_BORDER
        };
        let p = ui.painter().clone();
        p.rect_filled(cell_rect, 3.0, CELL_BG);
        p.rect_stroke(cell_rect, 3.0, Stroke::new(1.0_f32, border), egui::StrokeKind::Inside);

        // --- Header row: power dot + name (click opens inspector popover) + close × ---
        let hdr_r = Rect::from_min_size(
            cell_rect.min + vec2(3.0, 2.0),
            vec2(cell_rect.width() - 6.0, 13.0),
        );
        let pwr_r = Rect::from_min_size(hdr_r.min, vec2(10.0, 12.0));
        let pwr_resp = ui
            .interact(
                pwr_r,
                ui.id().with(("bay_pwr", track_idx, slot_idx, mod_id)),
                Sense::click(),
            )
            .on_hover_text("Enable / Bypass Modulator");
        p.circle_filled(
            pwr_r.center(),
            2.6,
            if m.enabled {
                MOD_ORANGE
            } else {
                Color32::from_rgb(0x52, 0x56, 0x60)
            },
        );
        if pwr_resp.clicked() {
            m.enabled = !m.enabled;
            dirty = true;
        }

        let del_r = Rect::from_min_size(pos2(hdr_r.max.x - 10.0, hdr_r.min.y), vec2(10.0, 12.0));
        let del_resp = ui
            .interact(
                del_r,
                ui.id().with(("bay_del", track_idx, slot_idx, mod_id)),
                Sense::click(),
            )
            .on_hover_text("Remove Modulator");
        let cell_hovered = ui.rect_contains_pointer(cell_rect);
        if cell_hovered || del_resp.hovered() {
            p.text(
                del_r.center(),
                Align2::CENTER_CENTER,
                "×",
                fonts::semibold(9.5),
                if del_resp.hovered() {
                    Color32::WHITE
                } else {
                    theme::TEXT_MUTED
                },
            );
        }
        if del_resp.clicked() {
            if any_map_active {
                clear_mapping_mode();
            }
            remove_id = Some(mod_id);
        }

        let title_r = Rect::from_min_max(pos2(pwr_r.max.x + 1.0, hdr_r.min.y), pos2(del_r.min.x, hdr_r.max.y));
        let title_resp = ui
            .interact(
                title_r,
                ui.id().with(("bay_title", track_idx, slot_idx, mod_id)),
                Sense::click(),
            )
            .on_hover_text(format!("{} — Click to edit parameters", m.kind.meta().name));
        if title_resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        p.text(
            pos2(title_r.min.x + 1.0, title_r.center().y),
            Align2::LEFT_CENTER,
            m.kind.meta().short_name,
            fonts::semibold(8.0),
            if m.enabled {
                KNOB_WHITE
            } else {
                theme::TEXT_MUTED
            },
        );

        let popup_id = ui.id().with(("bay_inspector", track_idx, slot_idx, mod_id));
        if title_resp.clicked() {
            ui.memory_mut(|mem| mem.toggle_popup(popup_id));
        }
        egui::popup_below_widget(
            ui,
            popup_id,
            &title_resp,
            egui::PopupCloseBehavior::CloseOnClickOutside,
            |pop_ui| {
                render_modulator_inspector(pop_ui, m, &mut dirty);
            },
        );

        // --- Body control (depends on ModulatorKind) ---
        let arrow_h = if num_outs > 4 { 21.0 } else { 15.0 };
        let body_r = Rect::from_min_max(
            pos2(cell_rect.min.x + 4.0, hdr_r.max.y + 2.0),
            pos2(cell_rect.max.x - 4.0, cell_rect.max.y - arrow_h - 3.0),
        );
        render_slot_body(ui, body_r, m, outs[0], time_secs, track_idx, slot_idx, &mut dirty);

        // --- Bottom Cyan Mapping Arrow Strip (`●→`, supports 1..8 outputs) ---
        let arrow_strip_r = Rect::from_min_max(
            pos2(cell_rect.min.x + 4.0, cell_rect.max.y - arrow_h - 1.0),
            pos2(cell_rect.max.x - 4.0, cell_rect.max.y - 2.0),
        );
        render_output_arrow_strip(
            ui,
            &p,
            arrow_strip_r,
            m,
            outs,
            out_labels,
            track_idx,
            slot_idx,
            &mut dirty,
            &mut remove_id,
        );
    }

    // --- '+' Add Modulator Cells (rellena las celdas vacías de la última columna como Bitwig) ---
    let plus_start = bay.slots.len();
    let total_grid_cells = bay.column_count() * BAY_SLOTS_PER_COL;
    let p = ui.painter().clone();
    for cell_idx in plus_start..total_grid_cells {
        let plus_col = cell_idx / BAY_SLOTS_PER_COL;
        let plus_row = cell_idx % BAY_SLOTS_PER_COL;
        let plus_rect = Rect::from_min_size(
            pos2(
                origin.x + plus_col as f32 * (BAY_COL_WIDTH + BAY_GAP),
                origin.y + plus_row as f32 * (cell_h + BAY_GAP),
            ),
            vec2(BAY_COL_WIDTH, cell_h),
        );
        p.rect_filled(plus_rect, 3.0, CELL_BG);
        p.rect_stroke(
            plus_rect,
            3.0,
            Stroke::new(1.0_f32, CELL_BORDER),
            egui::StrokeKind::Inside,
        );
        let plus_resp = ui
            .interact(
                plus_rect,
                ui.id().with(("bay_add_mod", track_idx, slot_idx, cell_idx)),
                Sense::click(),
            )
            .on_hover_text("Add Modulator (Pop-up Browser)");
        if plus_resp.hovered() {
            p.rect_filled(
                plus_rect.shrink(1.0),
                2.5,
                Color32::from_rgb(0x32, 0x35, 0x3D),
            );
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        p.text(
            plus_rect.center(),
            Align2::CENTER_CENTER,
            "+",
            fonts::semibold(16.0),
            if plus_resp.hovered() {
                Color32::WHITE
            } else {
                theme::TEXT_SECONDARY
            },
        );
        if plus_resp.clicked() {
            *request_open_browser = true;
        }
    }

    if let Some(id) = remove_id {
        bay.remove_modulator(id);
        dirty = true;
    }

    let consumed_w = bay.width_px();

    if dirty {
        let updated = bay;
        update_bay_state(track_idx, slot_idx, |st| {
            *st = updated;
        });
    }

    if any_lfo_active {
        ui.ctx().request_repaint();
    }

    consumed_w
}

#[allow(clippy::too_many_arguments)]
fn render_output_arrow_strip(
    ui: &mut Ui,
    p: &Painter,
    strip_r: Rect,
    m: &mut ModulatorSlotState,
    outs: [f32; 16],
    out_labels: &[&str],
    track_idx: usize,
    slot_idx: usize,
    dirty: &mut bool,
    remove_id: &mut Option<u32>,
) {
    let mod_id = m.id;
    let num_outs = out_labels.len().clamp(1, 16);
    let (rows, cols) = if num_outs <= 4 {
        (1, num_outs)
    } else if num_outs <= 8 {
        (2, 4)
    } else if num_outs <= 12 {
        (2, 6)
    } else {
        (2, 8)
    };
    let gap_x = if cols > 6 {
        0.8
    } else if cols > 1 {
        1.5
    } else {
        0.0
    };
    let gap_y = if rows > 1 { 1.5 } else { 0.0 };
    let cell_w = (strip_r.width() - gap_x * (cols as f32 - 1.0)) / cols as f32;
    let cell_h = (strip_r.height() - gap_y * (rows as f32 - 1.0)) / rows as f32;

    for out_i in 0..num_outs {
        let out_u8 = out_i as u8;
        let out_label = out_labels[out_i];
        let col = out_i % cols;
        let row = out_i / cols;
        let x0 = strip_r.min.x + col as f32 * (cell_w + gap_x);
        let y0 = strip_r.min.y + row as f32 * (cell_h + gap_y);
        let arrow_r = Rect::from_min_size(pos2(x0, y0), vec2(cell_w, cell_h));

        let arrow_resp = ui.interact(
            arrow_r,
            ui.id().with(("bay_map", track_idx, slot_idx, mod_id, out_u8)),
            Sense::click(),
        );
        if arrow_resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }

        let map_active = is_bay_mapping_active(track_idx, slot_idx, mod_id, out_u8);
        let is_assigned = m.has_routes_for_output(out_u8);
        let live_val = outs.get(out_i).copied().unwrap_or(0.0);

        if map_active {
            p.rect_filled(arrow_r, 2.5, MOD_ARROW_CYAN);
        } else if is_assigned {
            let pulse = (live_val.abs().clamp(0.0, 1.0) * 45.0) as u8;
            p.rect_filled(
                arrow_r,
                2.5,
                Color32::from_rgba_unmultiplied(0x46, 0xC8, 0xF5, 22 + pulse),
            );
        } else if arrow_resp.hovered() {
            p.rect_filled(arrow_r, 2.5, Color32::from_rgb(0x22, 0x38, 0x44));
        } else {
            p.rect_filled(arrow_r, 2.5, Color32::from_rgb(0x1C, 0x1E, 0x23));
        }

        let fg = if map_active {
            Color32::from_rgb(0x10, 0x14, 0x1A)
        } else if is_assigned {
            MOD_ARROW_CYAN
        } else {
            Color32::from_rgb(0x52, 0x8A, 0xA4)
        };

        if num_outs == 1 {
            paint_bay_arrow(p, arrow_r.center(), fg, is_assigned || map_active);
        } else if num_outs == 2 {
            // Compact label + mini arrow (e.g. X →, Y →)
            let c = arrow_r.center();
            p.text(
                pos2(c.x - 5.0, c.y),
                Align2::CENTER_CENTER,
                out_label,
                fonts::semibold(8.0),
                fg,
            );
            let tip_x = c.x + 8.0;
            p.line_segment(
                [pos2(c.x + 0.5, c.y), pos2(tip_x - 1.2, c.y)],
                Stroke::new(1.1_f32, fg),
            );
            p.add(egui::Shape::convex_polygon(
                vec![
                    pos2(tip_x, c.y),
                    pos2(tip_x - 3.0, c.y - 2.2),
                    pos2(tip_x - 3.0, c.y + 2.2),
                ],
                fg,
                Stroke::NONE,
            ));
        } else {
            // 4..16 outputs: compact labeled/numbered mapping cell with active dot
            let short_tag = if num_outs <= 4 && out_label.len() <= 3 {
                out_label.to_string()
            } else if out_label.len() <= 2 {
                out_label.to_string()
            } else {
                (out_i + 1).to_string()
            };
            let font_sz = if num_outs > 8 {
                6.0
            } else if num_outs > 4 {
                7.0
            } else if short_tag.len() >= 3 {
                6.2
            } else {
                8.0
            };
            p.text(
                arrow_r.center(),
                Align2::CENTER_CENTER,
                short_tag,
                fonts::semibold(font_sz),
                fg,
            );
            if is_assigned && !map_active {
                p.circle_filled(pos2(arrow_r.max.x - 1.8, arrow_r.min.y + 1.8), 1.2, MOD_ARROW_CYAN);
            }
        }

        let out_title = if num_outs == 1 {
            m.kind.meta().name.to_string()
        } else {
            format!("{} [{}]", m.kind.meta().name, out_label)
        };

        let tip = if map_active {
            format!(
                "{out_title}: Mapping active — Drag any cyan knob up/down • Click again or Esc to finish"
            )
        } else if is_assigned {
            let summary: Vec<String> = m
                .routes
                .iter()
                .filter(|r| r.out_idx == out_u8)
                .map(|r| format!("{} ({:+.0}%)", r.target.display_label(), r.amount * 100.0))
                .collect();
            format!(
                "{out_title} → {}\nLeft-click: Map knobs • Right-click: Manage routes",
                summary.join(", ")
            )
        } else {
            format!(
                "{out_title}: Click to map any knob • Right-click: Target list"
            )
        };
        let arrow_resp = arrow_resp.on_hover_text(tip);
        if arrow_resp.clicked() {
            toggle_bay_mapping_mode(track_idx, slot_idx, mod_id, out_u8);
        }
        arrow_resp.context_menu(|menu_ui| {
            menu_ui.label(
                RichText::new(format!("{out_title} Routes"))
                    .font(fonts::semibold(10.0))
                    .color(theme::ACCENT_ORANGE),
            );
            menu_ui.separator();
            let out_routes: Vec<_> = m
                .routes
                .iter()
                .filter(|r| r.out_idx == out_u8)
                .cloned()
                .collect();
            if out_routes.is_empty() {
                menu_ui.label(
                    RichText::new("Not mapped (click ●→ then drag any cyan knob)")
                        .font(fonts::regular(9.0))
                        .color(theme::TEXT_MUTED),
                );
            } else {
                for r in out_routes {
                    let mut amt = r.amount;
                    menu_ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(r.target.display_label())
                                .font(fonts::regular(9.0))
                                .color(MOD_ARROW_CYAN),
                        );
                        if ui
                            .add(egui::Slider::new(&mut amt, -1.0..=1.0).fixed_decimals(2))
                            .changed()
                        {
                            m.set_route(out_u8, r.target, amt);
                            *dirty = true;
                        }
                        if ui.small_button("×").clicked() {
                            m.set_route(out_u8, r.target, 0.0);
                            *dirty = true;
                        }
                    });
                }
                menu_ui.separator();
                if menu_ui.button("Clear Output Routes").clicked() {
                    m.clear_routes_for_output(out_u8);
                    *dirty = true;
                    menu_ui.close_menu();
                }
            }
            menu_ui.separator();
            menu_ui.menu_button("Add Target...", |sub_ui| {
                for &target in &ModulationTarget::ALL {
                    if target == ModulationTarget::None {
                        continue;
                    }
                    if sub_ui
                        .selectable_label(m.route_amount(out_u8, target).abs() > 0.004, target.label())
                        .clicked()
                    {
                        m.set_route(out_u8, target, 0.5);
                        *dirty = true;
                        sub_ui.close_menu();
                    }
                }
            });
            menu_ui.separator();
            if menu_ui.button("Remove Modulator").clicked() {
                *remove_id = Some(mod_id);
                menu_ui.close_menu();
            }
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn render_slot_body(
    ui: &mut Ui,
    rect: Rect,
    m: &mut ModulatorSlotState,
    live_out: f32,
    time_secs: f32,
    track_idx: usize,
    slot_idx: usize,
    dirty: &mut bool,
) {
    let p = ui.painter().clone();
    match m.kind {
        ModulatorKind::ClassicLfo => {
            // Mini waveform oscilloscope (click cycles waveform)
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_clfo_wave", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(format!(
                    "{} ({:.2} Hz) — Click: cycle waveform • Drag vertical: rate",
                    m.lfo_waveform.label(),
                    m.lfo_rate_hz
                ));
            if resp.clicked() {
                let idx = LfoWaveform::ALL
                    .iter()
                    .position(|&w| w == m.lfo_waveform)
                    .unwrap_or(0);
                m.lfo_waveform = LfoWaveform::ALL[(idx + 1) % LfoWaveform::ALL.len()];
                *dirty = true;
            } else if resp.dragged() {
                let delta = -resp.drag_delta().y * 0.015;
                m.lfo_rate_hz = (m.lfo_rate_hz * (1.0 + delta)).clamp(0.05, 25.0);
                *dirty = true;
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }

            let cy = rect.center().y;
            p.line_segment(
                [pos2(rect.min.x + 2.0, cy), pos2(rect.max.x - 2.0, cy)],
                Stroke::new(0.7_f32, Color32::from_rgb(0x36, 0x3A, 0x42)),
            );
            let steps = 24;
            let mut pts = Vec::with_capacity(steps + 1);
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let v = evaluate_waveform(m.lfo_waveform, t, if t < 0.5 { 0.65 } else { -0.55 });
                let x = rect.min.x + 2.0 + t * (rect.width() - 4.0);
                let y = cy - v * (rect.height() * 0.36);
                pts.push(pos2(x, y));
            }
            let wave_col = if m.enabled {
                MOD_ORANGE
            } else {
                Color32::from_rgb(0x62, 0x66, 0x70)
            };
            p.add(egui::Shape::line(pts, Stroke::new(1.25_f32, wave_col)));

            // Live phase dot
            let dot_x = rect.min.x + 2.0 + m.lfo_last_phase * (rect.width() - 4.0);
            let dot_y = cy - live_out * (rect.height() * 0.36);
            p.circle_filled(pos2(dot_x, dot_y), 2.1, Color32::WHITE);
        }
        ModulatorKind::BeatLfo => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_blfo_wave", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(format!(
                    "Beat LFO: {} @ {} — Click: cycle beat rate • Drag vertical: skew",
                    m.lfo_waveform.label(),
                    m.lfo_sync_rate.label()
                ));
            if resp.clicked() {
                let idx = SyncRate::ALL
                    .iter()
                    .position(|&r| r == m.lfo_sync_rate)
                    .unwrap_or(4);
                m.lfo_sync_rate = SyncRate::ALL[(idx + 1) % SyncRate::ALL.len()];
                *dirty = true;
            } else if resp.dragged() {
                let delta = -resp.drag_delta().y * 0.008;
                m.beat_lfo_skew = (m.beat_lfo_skew + delta).clamp(0.05, 0.95);
                *dirty = true;
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }

            let cy = rect.center().y;
            p.line_segment(
                [pos2(rect.min.x + 2.0, cy), pos2(rect.max.x - 2.0, cy)],
                Stroke::new(0.7_f32, Color32::from_rgb(0x36, 0x3A, 0x42)),
            );
            let steps = 24;
            let mut pts = Vec::with_capacity(steps + 1);
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let wt = ModulatorSlotState::warp_beat_phase(t, m.beat_lfo_skew);
                let v = evaluate_waveform(m.lfo_waveform, wt, if t < 0.5 { 0.65 } else { -0.55 });
                let x = rect.min.x + 2.0 + t * (rect.width() - 4.0);
                let y = cy - v * (rect.height() * 0.34);
                pts.push(pos2(x, y));
            }
            let wave_col = if m.enabled {
                MOD_ARROW_CYAN
            } else {
                Color32::from_rgb(0x62, 0x66, 0x70)
            };
            p.add(egui::Shape::line(pts, Stroke::new(1.25_f32, wave_col)));

            let dot_x = rect.min.x + 2.0 + m.lfo_last_phase * (rect.width() - 4.0);
            let dot_y = cy - live_out * (rect.height() * 0.34);
            p.circle_filled(pos2(dot_x, dot_y), 2.1, Color32::WHITE);

            // Musical rate badge in top-right corner
            p.text(
                pos2(rect.max.x - 2.0, rect.min.y + 1.5),
                Align2::RIGHT_TOP,
                m.lfo_sync_rate.label(),
                fonts::semibold(7.0),
                Color32::from_rgb(0xCC, 0xD2, 0xDC),
            );
        }
        ModulatorKind::Macro => {
            let knob_d = rect.height().min(rect.width()).clamp(22.0, 28.0);
            let knob_r = Rect::from_center_size(rect.center(), vec2(knob_d, knob_d));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(knob_r));
            let (_, changed) = bitwig_kit::rotary_knob(
                &mut child,
                knob_d,
                &mut m.macro_value,
                0.0..=1.0,
                0.5,
                if m.enabled {
                    MOD_ARROW_CYAN
                } else {
                    theme::TEXT_MUTED
                },
                m.macro_bipolar,
            );
            if changed {
                *dirty = true;
            }
        }
        ModulatorKind::Macro4 => {
            let gap = 2.0;
            let cw = (rect.width() - gap) * 0.5;
            let ch = (rect.height() - gap) * 0.5;
            let knob_d = cw.min(ch).clamp(11.0, 16.0);
            let col = if m.enabled {
                MOD_ARROW_CYAN
            } else {
                theme::TEXT_MUTED
            };
            for i in 0..4 {
                let gx = (i % 2) as f32;
                let gy = (i / 2) as f32;
                let sub = Rect::from_min_size(
                    pos2(rect.min.x + gx * (cw + gap), rect.min.y + gy * (ch + gap)),
                    vec2(cw, ch),
                );
                let knob_r = Rect::from_center_size(sub.center(), vec2(knob_d, knob_d));
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(knob_r));
                let (resp, changed) = bitwig_kit::rotary_knob(
                    &mut child,
                    knob_d,
                    &mut m.macro4_values[i],
                    0.0..=1.0,
                    0.5,
                    col,
                    m.macro4_bipolar[i],
                );
                resp.on_hover_text(format!("M{}: {:.0}%", i + 1, m.macro4_values[i] * 100.0));
                if changed {
                    *dirty = true;
                }
            }
        }
        ModulatorKind::Button => {
            let btn_r = rect.shrink2(vec2(4.0, 3.0));
            let resp = ui
                .interact(
                    btn_r,
                    ui.id().with(("bay_btn", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(if m.button_momentary {
                    "Momentary Button (hold to engage)"
                } else {
                    "Toggle Button (click to switch ON/OFF)"
                });
            if m.button_momentary {
                let next = resp.is_pointer_button_down_on();
                if next != m.button_on {
                    m.button_on = next;
                    *dirty = true;
                }
            } else if resp.clicked() {
                m.button_on = !m.button_on;
                *dirty = true;
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            let bg = if m.button_on && m.enabled {
                MOD_ARROW_CYAN
            } else {
                CELL_WELL_BG
            };
            p.rect_filled(btn_r, 3.0, bg);
            p.rect_stroke(
                btn_r,
                3.0,
                Stroke::new(
                    1.0_f32,
                    if m.button_on {
                        Color32::WHITE
                    } else {
                        CELL_BORDER
                    },
                ),
                egui::StrokeKind::Inside,
            );
            p.text(
                btn_r.center(),
                Align2::CENTER_CENTER,
                if m.button_on { "ON" } else { "OFF" },
                fonts::semibold(8.5),
                if m.button_on && m.enabled {
                    Color32::from_rgb(0x10, 0x14, 0x1A)
                } else {
                    theme::TEXT_SECONDARY
                },
            );
        }
        ModulatorKind::Buttons => {
            let gap = 2.0;
            let cw = (rect.width() - gap) * 0.5;
            let ch = (rect.height() - gap) * 0.5;
            for i in 0..4 {
                let gx = (i % 2) as f32;
                let gy = (i / 2) as f32;
                let btn_r = Rect::from_min_size(
                    pos2(rect.min.x + gx * (cw + gap), rect.min.y + gy * (ch + gap)),
                    vec2(cw, ch),
                );
                let resp = ui
                    .interact(
                        btn_r,
                        ui.id().with(("bay_btns", track_idx, slot_idx, m.id, i)),
                        Sense::click_and_drag(),
                    )
                    .on_hover_text(format!(
                        "Button {} ({})",
                        i + 1,
                        if m.buttons_momentary[i] { "Momentary" } else { "Toggle" }
                    ));
                if m.buttons_momentary[i] {
                    let next = resp.is_pointer_button_down_on();
                    if next != m.buttons_on[i] {
                        m.buttons_on[i] = next;
                        *dirty = true;
                    }
                } else if resp.clicked() {
                    m.buttons_on[i] = !m.buttons_on[i];
                    *dirty = true;
                }
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                let on = m.buttons_on[i];
                let bg = if on && m.enabled {
                    MOD_ARROW_CYAN
                } else {
                    CELL_WELL_BG
                };
                p.rect_filled(btn_r, 2.0, bg);
                p.rect_stroke(
                    btn_r,
                    2.0,
                    Stroke::new(1.0_f32, if on { Color32::WHITE } else { CELL_BORDER }),
                    egui::StrokeKind::Inside,
                );
                p.text(
                    btn_r.center(),
                    Align2::CENTER_CENTER,
                    format!("{}", i + 1),
                    fonts::semibold(7.5),
                    if on && m.enabled {
                        Color32::from_rgb(0x10, 0x14, 0x1A)
                    } else {
                        theme::TEXT_SECONDARY
                    },
                );
            }
        }
        ModulatorKind::Xy => {
            p.rect_filled(rect, 2.5, CELL_WELL_BG);
            p.rect_stroke(
                rect,
                2.5,
                Stroke::new(0.8_f32, CELL_BORDER),
                egui::StrokeKind::Inside,
            );
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_xy", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(format!(
                    "XY Pad (X: {:.0}%, Y: {:.0}%) — Drag to move • Double-click to center",
                    m.xy_x * 100.0,
                    m.xy_y * 100.0
                ));
            if resp.double_clicked() {
                m.xy_x = 0.5;
                m.xy_y = 0.5;
                *dirty = true;
            } else if resp.dragged() || resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let nx = ((pos.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 1.0);
                    let ny = (1.0 - (pos.y - rect.min.y) / rect.height().max(1.0)).clamp(0.0, 1.0);
                    m.xy_x = nx;
                    m.xy_y = ny;
                    *dirty = true;
                }
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
            }

            // Subtle center crosshairs
            let c = rect.center();
            let grid_col = Color32::from_rgb(0x2D, 0x31, 0x3A);
            p.line_segment(
                [pos2(rect.min.x + 2.0, c.y), pos2(rect.max.x - 2.0, c.y)],
                Stroke::new(0.7_f32, grid_col),
            );
            p.line_segment(
                [pos2(c.x, rect.min.y + 2.0), pos2(c.x, rect.max.y - 2.0)],
                Stroke::new(0.7_f32, grid_col),
            );

            // Draggable puck
            let pad = 3.0;
            let px = rect.min.x + pad + m.xy_x.clamp(0.0, 1.0) * (rect.width() - pad * 2.0).max(1.0);
            let py = rect.min.y + pad + (1.0 - m.xy_y.clamp(0.0, 1.0)) * (rect.height() - pad * 2.0).max(1.0);
            let puck = pos2(px, py);
            let puck_col = if m.enabled {
                MOD_ARROW_CYAN
            } else {
                theme::TEXT_MUTED
            };
            p.line_segment(
                [pos2(rect.min.x + 1.0, py), pos2(rect.max.x - 1.0, py)],
                Stroke::new(0.6_f32, Color32::from_rgba_unmultiplied(0x46, 0xC8, 0xF5, 60)),
            );
            p.line_segment(
                [pos2(px, rect.min.y + 1.0), pos2(px, rect.max.y - 1.0)],
                Stroke::new(0.6_f32, Color32::from_rgba_unmultiplied(0x46, 0xC8, 0xF5, 60)),
            );
            p.circle_filled(puck, 3.2, puck_col);
            p.circle_stroke(puck, 3.2, Stroke::new(0.9_f32, Color32::WHITE));
        }
        ModulatorKind::Math => {
            // Top badge: op selector (click cycles op); bottom: A & B mini knobs
            let op_h = 11.0;
            let op_r = Rect::from_min_size(rect.min, vec2(rect.width(), op_h));
            let op_resp = ui
                .interact(
                    op_r,
                    ui.id().with(("bay_math_op", track_idx, slot_idx, m.id)),
                    Sense::click(),
                )
                .on_hover_text(format!("Math: {} — Click to cycle operation", m.math_op.label()));
            if op_resp.clicked() {
                let idx = MathOp::ALL
                    .iter()
                    .position(|&o| o == m.math_op)
                    .unwrap_or(0);
                m.math_op = MathOp::ALL[(idx + 1) % MathOp::ALL.len()];
                *dirty = true;
            }
            if op_resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            p.rect_filled(op_r, 2.0, CELL_WELL_BG);
            p.text(
                op_r.center(),
                Align2::CENTER_CENTER,
                m.math_op.short_symbol(),
                fonts::semibold(7.5),
                if m.enabled { MOD_ORANGE } else { theme::TEXT_MUTED },
            );

            let knobs_r = Rect::from_min_max(pos2(rect.min.x, op_r.max.y + 2.0), rect.max);
            let half_w = knobs_r.width() * 0.5;
            let knob_d = half_w.min(knobs_r.height()).clamp(12.0, 16.0);
            let col = if m.enabled { MOD_ARROW_CYAN } else { theme::TEXT_MUTED };
            let r_a = Rect::from_center_size(
                pos2(knobs_r.min.x + half_w * 0.5, knobs_r.center().y),
                vec2(knob_d, knob_d),
            );
            let r_b = Rect::from_center_size(
                pos2(knobs_r.min.x + half_w * 1.5, knobs_r.center().y),
                vec2(knob_d, knob_d),
            );
            let mut ch_a = ui.new_child(egui::UiBuilder::new().max_rect(r_a));
            let (ra, ca) = bitwig_kit::rotary_knob(&mut ch_a, knob_d, &mut m.math_a, -1.0..=1.0, 0.5, col, true);
            ra.on_hover_text(format!("A: {:+.2}", m.math_a));
            let mut ch_b = ui.new_child(egui::UiBuilder::new().max_rect(r_b));
            let (rb, cb) = bitwig_kit::rotary_knob(&mut ch_b, knob_d, &mut m.math_b, -1.0..=1.0, 0.5, col, true);
            rb.on_hover_text(format!("B: {:+.2}", m.math_b));
            if ca || cb {
                *dirty = true;
            }
        }
        ModulatorKind::Mix => {
            let knob_d = (rect.height() - 6.0).min(rect.width()).clamp(18.0, 24.0);
            let knob_r = Rect::from_center_size(
                pos2(rect.center().x, rect.min.y + knob_d * 0.5 + 1.0),
                vec2(knob_d, knob_d),
            );
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(knob_r));
            let (resp, changed) = bitwig_kit::rotary_knob(
                &mut child,
                knob_d,
                &mut m.mix_amount,
                0.0..=1.0,
                0.5,
                if m.enabled { MOD_ORANGE } else { theme::TEXT_MUTED },
                false,
            );
            resp.on_hover_text(format!(
                "Mix: {:.0}% (A={:+.2}, B={:+.2} → Out={:+.2})",
                m.mix_amount * 100.0,
                m.mix_a,
                m.mix_b,
                live_out
            ));
            if changed {
                *dirty = true;
            }
            // Mini crossfade bar at bottom
            let bar_r = Rect::from_min_max(
                pos2(rect.min.x + 2.0, rect.max.y - 4.0),
                pos2(rect.max.x - 2.0, rect.max.y - 1.0),
            );
            p.rect_filled(bar_r, 1.0, CELL_WELL_BG);
            let fill_w = bar_r.width() * m.mix_amount.clamp(0.0, 1.0);
            p.rect_filled(
                Rect::from_min_size(bar_r.min, vec2(fill_w, bar_r.height())),
                1.0,
                if m.enabled { MOD_ARROW_CYAN } else { theme::TEXT_MUTED },
            );
        }
        ModulatorKind::Quantize => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_quant", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(format!(
                    "Quantize ({} steps): In={:.2} → Out={:.2}\nDrag horizontal: input • Click: cycle steps",
                    m.quant_steps, m.quant_in, live_out
                ));
            if resp.dragged() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let t = ((pos.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 1.0);
                    m.quant_in = if m.quant_bipolar { t * 2.0 - 1.0 } else { t };
                    *dirty = true;
                }
            } else if resp.clicked() {
                const PRESETS: [u8; 6] = [2, 3, 4, 6, 8, 12];
                let next = PRESETS
                    .iter()
                    .copied()
                    .find(|&s| s > m.quant_steps)
                    .unwrap_or(PRESETS[0]);
                m.quant_steps = next;
                *dirty = true;
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
            }

            // Staircase transfer plot
            let steps = m.quant_steps.clamp(2, 32) as usize;
            let samples = 24;
            let mut pts = Vec::with_capacity(samples + 1);
            for i in 0..=samples {
                let t = i as f32 / samples as f32;
                let q = ((t * (steps - 1) as f32).round() / (steps - 1) as f32).clamp(0.0, 1.0);
                let x = rect.min.x + 2.0 + t * (rect.width() - 4.0);
                let y = rect.max.y - 3.0 - q * (rect.height() - 6.0);
                pts.push(pos2(x, y));
            }
            let col = if m.enabled { MOD_ORANGE } else { theme::TEXT_MUTED };
            p.add(egui::Shape::line(pts, Stroke::new(1.2_f32, col)));

            let in_norm = if m.quant_bipolar {
                (m.quant_in.clamp(-1.0, 1.0) + 1.0) * 0.5
            } else {
                m.quant_in.clamp(0.0, 1.0)
            };
            let out_norm = if m.quant_bipolar {
                (live_out.clamp(-1.0, 1.0) + 1.0) * 0.5
            } else {
                live_out.clamp(0.0, 1.0)
            };
            let dx = rect.min.x + 2.0 + in_norm * (rect.width() - 4.0);
            let dy = rect.max.y - 3.0 - out_norm * (rect.height() - 6.0);
            p.circle_filled(pos2(dx, dy), 2.3, Color32::WHITE);
            p.text(
                pos2(rect.min.x + 2.5, rect.min.y + 1.5),
                Align2::LEFT_TOP,
                format!("{}st", m.quant_steps),
                fonts::semibold(7.0),
                Color32::from_rgb(0xCC, 0xD2, 0xDC),
            );
        }
        ModulatorKind::Select4 => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_sel4", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(format!(
                    "Select-4 ({}): Pos={:.0}% → Out={:+.2}\nDrag horizontal to sweep selector",
                    if m.sel4_smooth { "Morph" } else { "Step" },
                    m.sel4_index * 100.0,
                    live_out
                ));
            if resp.dragged() || resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    m.sel4_index = ((pos.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 1.0);
                    *dirty = true;
                }
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
            }

            // 4 vertical bars for the 4 values
            let pad = 2.5;
            let gap = 2.0;
            let inner_w = (rect.width() - pad * 2.0 - gap * 3.0).max(4.0);
            let bw = inner_w / 4.0;
            let active_pos = m.sel4_index.clamp(0.0, 1.0) * 3.0;
            for i in 0..4 {
                let x0 = rect.min.x + pad + i as f32 * (bw + gap);
                let norm = ((m.sel4_values[i].clamp(-1.0, 1.0) + 1.0) * 0.5).clamp(0.05, 1.0);
                let bh = norm * (rect.height() - pad * 2.0);
                let bar_r = Rect::from_min_max(
                    pos2(x0, rect.max.y - pad - bh),
                    pos2(x0 + bw, rect.max.y - pad),
                );
                let dist = (active_pos - i as f32).abs();
                let highlight = if m.sel4_smooth {
                    (1.0 - dist).clamp(0.0, 1.0)
                } else if dist < 0.5 {
                    1.0
                } else {
                    0.0
                };
                let bar_col = if !m.enabled {
                    theme::TEXT_MUTED
                } else if highlight > 0.35 {
                    MOD_ARROW_CYAN
                } else {
                    Color32::from_rgb(0x4A, 0x52, 0x60)
                };
                p.rect_filled(bar_r, 1.5, bar_col);
            }
            // Selector cursor line
            let sx = rect.min.x + pad + m.sel4_index.clamp(0.0, 1.0) * (rect.width() - pad * 2.0);
            p.line_segment(
                [pos2(sx, rect.min.y + 1.0), pos2(sx, rect.max.y - 1.0)],
                Stroke::new(1.1_f32, MOD_ORANGE),
            );
        }
        ModulatorKind::Polynom => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_poly", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(format!(
                    "Polynom: x={:+.2} → y={:+.2}\nDrag horizontal to sweep input x",
                    m.poly_in, live_out
                ));
            if resp.dragged() || resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let t = ((pos.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 1.0);
                    m.poly_in = t * 2.0 - 1.0;
                    *dirty = true;
                }
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
            }

            let cy = rect.center().y;
            p.line_segment(
                [pos2(rect.min.x + 2.0, cy), pos2(rect.max.x - 2.0, cy)],
                Stroke::new(0.6_f32, Color32::from_rgb(0x32, 0x36, 0x40)),
            );
            let steps = 24;
            let mut pts = Vec::with_capacity(steps + 1);
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let x_in = t * 2.0 - 1.0;
                let y_out = m.eval_polynom(x_in);
                let px = rect.min.x + 2.0 + t * (rect.width() - 4.0);
                let py = cy - y_out * (rect.height() * 0.42);
                pts.push(pos2(px, py));
            }
            let curve_col = if m.enabled { MOD_ORANGE } else { theme::TEXT_MUTED };
            p.add(egui::Shape::line(pts, Stroke::new(1.25_f32, curve_col)));

            let dot_t = (m.poly_in.clamp(-1.0, 1.0) + 1.0) * 0.5;
            let dx = rect.min.x + 2.0 + dot_t * (rect.width() - 4.0);
            let dy = cy - live_out * (rect.height() * 0.42);
            p.circle_filled(pos2(dx, dy), 2.3, Color32::WHITE);
        }
        ModulatorKind::Random => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_rand", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(format!(
                    "Random ({:.2} Hz, Smooth {:.0}%) — Click: toggle smooth • Drag vertical: rate",
                    m.lfo_rate_hz,
                    m.rand_smooth * 100.0
                ));
            if resp.clicked() {
                m.rand_smooth = if m.rand_smooth > 0.1 { 0.0 } else { 0.5 };
                *dirty = true;
            } else if resp.dragged() {
                let delta = -resp.drag_delta().y * 0.015;
                m.lfo_rate_hz = (m.lfo_rate_hz * (1.0 + delta)).clamp(0.05, 25.0);
                *dirty = true;
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            let cy = rect.center().y;
            let sample_vals = [0.2_f32, -0.7, 0.85, -0.35, 0.55, -0.8, 0.4, 0.1];
            let mut pts = Vec::with_capacity(sample_vals.len() * 2);
            for (i, &v) in sample_vals.iter().enumerate() {
                let x0 = rect.min.x + 2.0 + (i as f32 / sample_vals.len() as f32) * (rect.width() - 4.0);
                let x1 = rect.min.x + 2.0 + ((i + 1) as f32 / sample_vals.len() as f32) * (rect.width() - 4.0);
                let y = cy - v * (rect.height() * 0.36);
                if m.rand_smooth < 0.1 {
                    pts.push(pos2(x0, y));
                    pts.push(pos2(x1, y));
                } else {
                    pts.push(pos2((x0 + x1) * 0.5, y));
                }
            }
            let col = if m.enabled { MOD_ORANGE } else { theme::TEXT_MUTED };
            p.add(egui::Shape::line(pts, Stroke::new(1.2_f32, col)));
            let dy = cy - live_out * (rect.height() * 0.38);
            p.circle_filled(pos2(rect.max.x - 4.5, dy), 2.3, Color32::WHITE);
        }
        ModulatorKind::SampleAndHold => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let src_names = ["Noise", "Sine", "Tri", "Saw"];
            let src_tags = ["N", "S", "T", "W"];
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_sh", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(format!(
                    "Sample & Hold ({}, {:.2} Hz) — Click: cycle source • Drag vertical: rate",
                    src_names[(m.sh_source as usize).min(3)],
                    m.lfo_rate_hz
                ));
            if resp.clicked() {
                m.sh_source = (m.sh_source + 1) % 4;
                *dirty = true;
            } else if resp.dragged() {
                let delta = -resp.drag_delta().y * 0.015;
                m.lfo_rate_hz = (m.lfo_rate_hz * (1.0 + delta)).clamp(0.05, 25.0);
                *dirty = true;
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            let cy = rect.center().y;
            let steps = [0.6_f32, -0.45, 0.85, -0.75, 0.25, -0.15];
            let mut pts = Vec::with_capacity(steps.len() * 2);
            for (i, &v) in steps.iter().enumerate() {
                let x0 = rect.min.x + 2.0 + (i as f32 / steps.len() as f32) * (rect.width() - 4.0);
                let x1 = rect.min.x + 2.0 + ((i + 1) as f32 / steps.len() as f32) * (rect.width() - 4.0);
                let y = cy - v * (rect.height() * 0.36);
                pts.push(pos2(x0, y));
                pts.push(pos2(x1, y));
            }
            let col = if m.enabled { MOD_ARROW_CYAN } else { theme::TEXT_MUTED };
            p.add(egui::Shape::line(pts, Stroke::new(1.2_f32, col)));
            let dy = cy - live_out * (rect.height() * 0.38);
            p.circle_filled(pos2(rect.max.x - 4.5, dy), 2.3, Color32::WHITE);
            p.text(
                pos2(rect.min.x + 2.5, rect.min.y + 1.5),
                Align2::LEFT_TOP,
                src_tags[(m.sh_source as usize).min(3)],
                fonts::semibold(7.0),
                Color32::from_rgb(0xCC, 0xD2, 0xDC),
            );
        }
        ModulatorKind::Steps => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let len = (m.steps_len.clamp(2, 16)) as usize;
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_steps", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(format!(
                    "Steps ({} steps @ {}) — Drag to draw step values",
                    len,
                    m.lfo_sync_rate.label()
                ));
            if resp.dragged() || resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let tx = ((pos.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 0.999);
                    let ty = (1.0 - (pos.y - rect.min.y) / rect.height().max(1.0)).clamp(0.0, 1.0);
                    let idx = (tx * len as f32) as usize;
                    if idx < 16 {
                        m.steps_values[idx] = ty;
                        *dirty = true;
                    }
                }
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
            }
            let pad = 2.0;
            let gap = 1.0;
            let bw = ((rect.width() - pad * 2.0 - gap * (len as f32 - 1.0)) / len as f32).max(1.5);
            for i in 0..len {
                let x0 = rect.min.x + pad + i as f32 * (bw + gap);
                let bh = (m.steps_values[i].clamp(0.05, 1.0)) * (rect.height() - pad * 2.0);
                let bar = Rect::from_min_max(
                    pos2(x0, rect.max.y - pad - bh),
                    pos2(x0 + bw, rect.max.y - pad),
                );
                let is_cur = i == m.steps_last_idx;
                let col = if !m.enabled {
                    theme::TEXT_MUTED
                } else if is_cur {
                    MOD_ARROW_CYAN
                } else {
                    MOD_ORANGE
                };
                p.rect_filled(bar, 1.0, col);
            }
        }
        ModulatorKind::ParSeq8 => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let len = (m.parseq_len.clamp(2, 8)) as usize;
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_parseq8", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(format!(
                    "ParSeq-8 ({} steps @ {}) — Drag to edit per-step output level",
                    len,
                    m.lfo_sync_rate.label()
                ));
            if resp.dragged() || resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let tx = ((pos.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 0.999);
                    let ty = (1.0 - (pos.y - rect.min.y) / rect.height().max(1.0)).clamp(0.0, 1.0);
                    let idx = (tx * len as f32) as usize;
                    if idx < 8 {
                        m.parseq_steps[idx] = ty;
                        *dirty = true;
                    }
                }
            }
            let pad = 2.0;
            let gap = 1.0;
            let bw = ((rect.width() - pad * 2.0 - gap * (len as f32 - 1.0)) / len as f32).max(2.0);
            for i in 0..len {
                let x0 = rect.min.x + pad + i as f32 * (bw + gap);
                let bh = (m.parseq_steps[i].clamp(0.08, 1.0)) * (rect.height() - pad * 2.0);
                let bar = Rect::from_min_max(
                    pos2(x0, rect.max.y - pad - bh),
                    pos2(x0 + bw, rect.max.y - pad),
                );
                let is_cur = i == m.parseq_last_step;
                let col = if !m.enabled {
                    theme::TEXT_MUTED
                } else if is_cur {
                    MOD_ARROW_CYAN
                } else {
                    Color32::from_rgb(0x4E, 0x56, 0x64)
                };
                p.rect_filled(bar, 1.0, col);
            }
        }
        ModulatorKind::Curves => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_curves", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text("Curves — Drag to sculpt the 8-node looping curve");
            if resp.dragged() || resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let tx = ((pos.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 0.999);
                    let ty = (1.0 - (pos.y - rect.min.y) / rect.height().max(1.0)).clamp(0.0, 1.0);
                    let idx = ((tx * 8.0) as usize).min(7);
                    m.curves_nodes[idx] = ty;
                    *dirty = true;
                }
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
            }
            let steps = 24;
            let mut pts = Vec::with_capacity(steps + 1);
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let v = m.eval_curve_at(t);
                let x = rect.min.x + 2.0 + t * (rect.width() - 4.0);
                let y = rect.max.y - 2.5 - v * (rect.height() - 5.0);
                pts.push(pos2(x, y));
            }
            let col = if m.enabled { MOD_ORANGE } else { theme::TEXT_MUTED };
            p.add(egui::Shape::line(pts, Stroke::new(1.25_f32, col)));
            let dx = rect.min.x + 2.0 + m.lfo_last_phase * (rect.width() - 4.0);
            let dy = rect.max.y - 2.5 - m.eval_curve_at(m.lfo_last_phase) * (rect.height() - 5.0);
            p.circle_filled(pos2(dx, dy), 2.2, Color32::WHITE);
        }
        ModulatorKind::Ramp => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_ramp", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(format!(
                    "Ramp ({:.2}s, {}) — Click: retrigger • Drag vertical: curve",
                    m.ramp_time_secs,
                    if m.ramp_loop { "Loop" } else { "One-Shot" }
                ));
            if resp.clicked() {
                m.ramp_start_time = time_secs;
                *dirty = true;
            } else if resp.dragged() {
                m.ramp_curve = (m.ramp_curve - resp.drag_delta().y * 0.015).clamp(-1.0, 1.0);
                *dirty = true;
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            let steps = 20;
            let mut pts = Vec::with_capacity(steps + 1);
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let v = m.eval_ramp_at(t);
                let x = rect.min.x + 2.0 + t * (rect.width() - 4.0);
                let y = rect.max.y - 2.5 - v * (rect.height() - 5.0);
                pts.push(pos2(x, y));
            }
            let col = if m.enabled { MOD_ORANGE } else { theme::TEXT_MUTED };
            p.add(egui::Shape::line(pts, Stroke::new(1.25_f32, col)));
            let dx = rect.min.x + 2.0 + m.lfo_last_phase * (rect.width() - 4.0);
            let dy = rect.max.y - 2.5 - m.eval_ramp_at(m.lfo_last_phase) * (rect.height() - 5.0);
            p.circle_filled(pos2(dx, dy), 2.2, Color32::WHITE);
        }
        ModulatorKind::Segments => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_segs", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text("Segments (MSEG) — Click: retrigger • Drag to edit breakpoint");
            if resp.dragged() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let tx = ((pos.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 1.0);
                    let ty = (1.0 - (pos.y - rect.min.y) / rect.height().max(1.0)).clamp(0.0, 1.0);
                    let idx = (tx * 5.0).round() as usize;
                    if idx < 6 {
                        m.seg_levels[idx] = ty;
                        *dirty = true;
                    }
                }
            } else if resp.clicked() {
                m.seg_start_time = time_secs;
                *dirty = true;
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
            }
            let steps = 25;
            let mut pts = Vec::with_capacity(steps + 1);
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let v = m.eval_segments_at(t);
                let x = rect.min.x + 2.0 + t * (rect.width() - 4.0);
                let y = rect.max.y - 2.5 - v * (rect.height() - 5.0);
                pts.push(pos2(x, y));
            }
            let col = if m.enabled { MOD_ARROW_CYAN } else { theme::TEXT_MUTED };
            p.add(egui::Shape::line(pts, Stroke::new(1.25_f32, col)));
            let dx = rect.min.x + 2.0 + m.lfo_last_phase * (rect.width() - 4.0);
            let dy = rect.max.y - 2.5 - m.eval_segments_at(m.lfo_last_phase) * (rect.height() - 5.0);
            p.circle_filled(pos2(dx, dy), 2.2, Color32::WHITE);
        }
        ModulatorKind::Vector4 => {
            p.rect_filled(rect, 2.5, CELL_WELL_BG);
            p.rect_stroke(rect, 2.5, Stroke::new(0.8_f32, CELL_BORDER), egui::StrokeKind::Inside);
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_vec4", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text("Vector-4 (A/B/C/D corner blend) — Drag to morph • Double-click to center");
            if resp.double_clicked() {
                m.xy_x = 0.5;
                m.xy_y = 0.5;
                *dirty = true;
            } else if resp.dragged() || resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    m.xy_x = ((pos.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 1.0);
                    m.xy_y = (1.0 - (pos.y - rect.min.y) / rect.height().max(1.0)).clamp(0.0, 1.0);
                    *dirty = true;
                }
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
            }
            // Corner markers A, B, C, D
            let lbl_col = Color32::from_rgb(0x6C, 0x74, 0x82);
            p.text(pos2(rect.min.x + 3.0, rect.min.y + 1.5), Align2::LEFT_TOP, "A", fonts::semibold(6.5), lbl_col);
            p.text(pos2(rect.max.x - 3.0, rect.min.y + 1.5), Align2::RIGHT_TOP, "B", fonts::semibold(6.5), lbl_col);
            p.text(pos2(rect.min.x + 3.0, rect.max.y - 1.5), Align2::LEFT_BOTTOM, "C", fonts::semibold(6.5), lbl_col);
            p.text(pos2(rect.max.x - 3.0, rect.max.y - 1.5), Align2::RIGHT_BOTTOM, "D", fonts::semibold(6.5), lbl_col);
            let pad = 3.0;
            let px = rect.min.x + pad + m.xy_x * (rect.width() - pad * 2.0).max(1.0);
            let py = rect.min.y + pad + (1.0 - m.xy_y) * (rect.height() - pad * 2.0).max(1.0);
            let puck = pos2(px, py);
            p.circle_filled(puck, 3.0, if m.enabled { MOD_ORANGE } else { theme::TEXT_MUTED });
            p.circle_stroke(puck, 3.0, Stroke::new(0.9_f32, Color32::WHITE));
        }
        ModulatorKind::Vector8 => {
            let z_w = 8.0;
            let pad_r = Rect::from_min_max(rect.min, pos2(rect.max.x - z_w - 2.0, rect.max.y));
            let z_r = Rect::from_min_max(pos2(rect.max.x - z_w, rect.min.y), rect.max);

            p.rect_filled(pad_r, 2.0, CELL_WELL_BG);
            p.rect_stroke(pad_r, 2.0, Stroke::new(0.8_f32, CELL_BORDER), egui::StrokeKind::Inside);
            let xy_resp = ui
                .interact(
                    pad_r,
                    ui.id().with(("bay_vec8_xy", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(format!(
                    "Vector-8 XY (X:{:.0}% Y:{:.0}% Z:{:.0}%) — Double-click to center",
                    m.xy_x * 100.0,
                    m.xy_y * 100.0,
                    m.vec8_z * 100.0
                ));
            if xy_resp.double_clicked() {
                m.xy_x = 0.5;
                m.xy_y = 0.5;
                m.vec8_z = 0.5;
                *dirty = true;
            } else if xy_resp.dragged() || xy_resp.clicked() {
                if let Some(pos) = xy_resp.interact_pointer_pos() {
                    m.xy_x = ((pos.x - pad_r.min.x) / pad_r.width().max(1.0)).clamp(0.0, 1.0);
                    m.xy_y = (1.0 - (pos.y - pad_r.min.y) / pad_r.height().max(1.0)).clamp(0.0, 1.0);
                    *dirty = true;
                }
            }
            let p_pad = 2.5;
            let px = pad_r.min.x + p_pad + m.xy_x * (pad_r.width() - p_pad * 2.0).max(1.0);
            let py = pad_r.min.y + p_pad + (1.0 - m.xy_y) * (pad_r.height() - p_pad * 2.0).max(1.0);
            p.circle_filled(pos2(px, py), 2.8, if m.enabled { MOD_ORANGE } else { theme::TEXT_MUTED });
            p.circle_stroke(pos2(px, py), 2.8, Stroke::new(0.8_f32, Color32::WHITE));

            // Vertical Z depth bar
            p.rect_filled(z_r, 1.5, CELL_WELL_BG);
            let z_resp = ui
                .interact(
                    z_r,
                    ui.id().with(("bay_vec8_z", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(format!("Vector-8 Z Depth: {:.0}%", m.vec8_z * 100.0));
            if z_resp.dragged() || z_resp.clicked() {
                if let Some(pos) = z_resp.interact_pointer_pos() {
                    m.vec8_z = (1.0 - (pos.y - z_r.min.y) / z_r.height().max(1.0)).clamp(0.0, 1.0);
                    *dirty = true;
                }
            }
            let zh = m.vec8_z.clamp(0.0, 1.0) * z_r.height();
            p.rect_filled(
                Rect::from_min_max(pos2(z_r.min.x + 1.0, z_r.max.y - zh), pos2(z_r.max.x - 1.0, z_r.max.y)),
                1.0,
                if m.enabled { MOD_ARROW_CYAN } else { theme::TEXT_MUTED },
            );
        }
        ModulatorKind::Adsr | ModulatorKind::Ahdsr | ModulatorKind::NoteSidechain => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_env", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(format!(
                    "{} — Hold mouse to trigger Gate (Out: {:.0}%)",
                    m.kind.meta().name,
                    live_out * 100.0
                ));
            let held = resp.is_pointer_button_down_on();
            if held && !m.env_gate {
                m.trigger_gate(true, time_secs);
                *dirty = true;
            } else if !held && m.env_gate && resp.drag_stopped() {
                m.trigger_gate(false, time_secs);
                *dirty = true;
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            let x0 = rect.min.x + 2.0;
            let w = rect.width() - 4.0;
            let y_bot = rect.max.y - 2.5;
            let h = rect.height() - 5.0;
            let has_hold = m.kind == ModulatorKind::Ahdsr;
            let ax = x0 + w * 0.22;
            let hx = if has_hold { x0 + w * 0.34 } else { ax };
            let dx = x0 + w * 0.56;
            let sx = x0 + w * 0.78;
            let rx = x0 + w;
            let sy = y_bot - m.env_sustain.clamp(0.0, 1.0) * h;
            let mut pts = vec![pos2(x0, y_bot), pos2(ax, y_bot - h)];
            if has_hold {
                pts.push(pos2(hx, y_bot - h));
            }
            pts.push(pos2(dx, sy));
            pts.push(pos2(sx, sy));
            pts.push(pos2(rx, y_bot));
            let col = if !m.enabled {
                theme::TEXT_MUTED
            } else if m.env_gate {
                MOD_ARROW_CYAN
            } else {
                MOD_ORANGE
            };
            p.add(egui::Shape::line(pts, Stroke::new(1.25_f32, col)));
            // Live output level bar on right edge
            let lvl_h = live_out.clamp(0.0, 1.0) * h;
            p.rect_filled(
                Rect::from_min_max(pos2(rect.max.x - 4.0, y_bot - lvl_h), pos2(rect.max.x - 1.5, y_bot)),
                1.0,
                MOD_ARROW_CYAN,
            );
        }
        ModulatorKind::FourStage => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_4stage", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text("4-Stage Envelope — Hold to gate / trigger");
            let held = resp.is_pointer_button_down_on();
            if held && !m.env_gate {
                m.trigger_gate(true, time_secs);
                *dirty = true;
            } else if !held && m.env_gate && resp.drag_stopped() {
                m.trigger_gate(false, time_secs);
                *dirty = true;
            }
            let x0 = rect.min.x + 2.0;
            let w = rect.width() - 4.0;
            let y_bot = rect.max.y - 2.5;
            let h = rect.height() - 5.0;
            let pts = vec![
                pos2(x0, y_bot),
                pos2(x0 + w * 0.25, y_bot - m.env_4stage_levels[0] * h),
                pos2(x0 + w * 0.50, y_bot - m.env_4stage_levels[1] * h),
                pos2(x0 + w * 0.75, y_bot - m.env_4stage_levels[2] * h),
                pos2(x0 + w, y_bot - m.env_4stage_levels[3] * h),
            ];
            let col = if m.enabled { MOD_ORANGE } else { theme::TEXT_MUTED };
            p.add(egui::Shape::line(pts, Stroke::new(1.25_f32, col)));
            let lvl_h = live_out.clamp(0.0, 1.0) * h;
            p.rect_filled(
                Rect::from_min_max(pos2(rect.max.x - 4.0, y_bot - lvl_h), pos2(rect.max.x - 1.5, y_bot)),
                1.0,
                MOD_ARROW_CYAN,
            );
        }
        ModulatorKind::AhdOnRelease => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_ahd_rel", track_idx, slot_idx, m.id)),
                    Sense::click(),
                )
                .on_hover_text("AHD on Release — Click to fire release envelope");
            if resp.clicked() {
                m.env_gate = false;
                m.env_gate_off_time = time_secs;
                *dirty = true;
            }
            let x0 = rect.min.x + 2.0;
            let w = rect.width() - 4.0;
            let y_bot = rect.max.y - 2.5;
            let h = rect.height() - 5.0;
            let pts = vec![
                pos2(x0, y_bot),
                pos2(x0 + w * 0.25, y_bot - h),
                pos2(x0 + w * 0.50, y_bot - h),
                pos2(x0 + w, y_bot),
            ];
            p.add(egui::Shape::line(
                pts,
                Stroke::new(1.25_f32, if m.enabled { MOD_ARROW_CYAN } else { theme::TEXT_MUTED }),
            ));
            let lvl_h = live_out.clamp(0.0, 1.0) * h;
            p.rect_filled(
                Rect::from_min_max(pos2(rect.max.x - 4.0, y_bot - lvl_h), pos2(rect.max.x - 1.5, y_bot)),
                1.0,
                MOD_ORANGE,
            );
        }
        ModulatorKind::Expressions => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_expr", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text("Expressions (VEL / TMB / REL / PRS) — Drag bars to audition");
            if resp.dragged() || resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let tx = ((pos.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 0.999);
                    let ty = (1.0 - (pos.y - rect.min.y) / rect.height().max(1.0)).clamp(0.0, 1.0);
                    match (tx * 4.0) as usize {
                        0 => m.expr_vel = ty,
                        1 => m.expr_timbre = ty,
                        2 => m.expr_rel = ty,
                        _ => m.expr_pres = ty,
                    }
                    *dirty = true;
                }
            }
            let vals = [m.expr_vel, m.expr_timbre, m.expr_rel, m.expr_pres];
            let pad = 2.0;
            let gap = 2.0;
            let bw = (rect.width() - pad * 2.0 - gap * 3.0) / 4.0;
            for (i, &v) in vals.iter().enumerate() {
                let x0 = rect.min.x + pad + i as f32 * (bw + gap);
                let bh = v.clamp(0.06, 1.0) * (rect.height() - pad * 2.0);
                p.rect_filled(
                    Rect::from_min_max(pos2(x0, rect.max.y - pad - bh), pos2(x0 + bw, rect.max.y - pad)),
                    1.0,
                    if m.enabled { MOD_ARROW_CYAN } else { theme::TEXT_MUTED },
                );
            }
        }
        ModulatorKind::Midi => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_midi", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(format!(
                    "MIDI (CC{}, MW, PB, AT) — Drag bars to audition",
                    m.midi_cc_num
                ));
            if resp.dragged() || resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let tx = ((pos.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 0.999);
                    let ty = (1.0 - (pos.y - rect.min.y) / rect.height().max(1.0)).clamp(0.0, 1.0);
                    match (tx * 4.0) as usize {
                        0 => m.midi_cc_val = ty,
                        1 => m.midi_mw = ty,
                        2 => m.midi_pb = ty * 2.0 - 1.0,
                        _ => m.midi_at = ty,
                    }
                    *dirty = true;
                }
            }
            let vals = [m.midi_cc_val, m.midi_mw, (m.midi_pb + 1.0) * 0.5, m.midi_at];
            let pad = 2.0;
            let gap = 2.0;
            let bw = (rect.width() - pad * 2.0 - gap * 3.0) / 4.0;
            for (i, &v) in vals.iter().enumerate() {
                let x0 = rect.min.x + pad + i as f32 * (bw + gap);
                let bh = v.clamp(0.06, 1.0) * (rect.height() - pad * 2.0);
                p.rect_filled(
                    Rect::from_min_max(pos2(x0, rect.max.y - pad - bh), pos2(x0 + bw, rect.max.y - pad)),
                    1.0,
                    if m.enabled { MOD_ORANGE } else { theme::TEXT_MUTED },
                );
            }
        }
        ModulatorKind::KeytrackPlus => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_keytrk", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(format!(
                    "Keytrack+ (Note {}, Root {}) — Drag horizontal to audition note",
                    m.key_note, m.key_root
                ));
            if resp.dragged() || resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let t = ((pos.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 1.0);
                    m.key_note = (24.0 + t * 72.0).round() as u8;
                    *dirty = true;
                }
            }
            let cy = rect.center().y;
            let exp = 2.0_f32.powf(-m.key_curve.clamp(-1.0, 1.0) * 1.5);
            let mut pts = Vec::with_capacity(21);
            for i in 0..=20 {
                let t = i as f32 / 20.0;
                let raw = t * 2.0 - 1.0;
                let yv = raw.signum() * raw.abs().powf(exp);
                pts.push(pos2(
                    rect.min.x + 2.0 + t * (rect.width() - 4.0),
                    cy - yv * (rect.height() * 0.40),
                ));
            }
            p.add(egui::Shape::line(
                pts,
                Stroke::new(1.2_f32, if m.enabled { MOD_ORANGE } else { theme::TEXT_MUTED }),
            ));
            let dot_t = ((live_out + 1.0) * 0.5).clamp(0.0, 1.0);
            p.circle_filled(
                pos2(rect.min.x + 2.0 + dot_t * (rect.width() - 4.0), cy - live_out * (rect.height() * 0.40)),
                2.2,
                Color32::WHITE,
            );
        }
        ModulatorKind::RelativeKeytrack => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let diff = m.key_note as i32 - m.key_prev_note as i32;
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_relkey", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(format!("Relative Keytrack ({:+} st) — Drag horizontal to audition", diff));
            if resp.dragged() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let t = ((pos.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 1.0);
                    m.key_note = (m.key_prev_note as i32 + ((t * 2.0 - 1.0) * m.key_spread_semi).round() as i32).clamp(0, 127) as u8;
                    *dirty = true;
                }
            }
            let cx = rect.center().x;
            p.line_segment([pos2(cx, rect.min.y + 2.0), pos2(cx, rect.max.y - 2.0)], Stroke::new(0.8_f32, CELL_BORDER));
            let bar_x = cx + live_out.clamp(-1.0, 1.0) * (rect.width() * 0.44);
            let bar_r = Rect::from_min_max(
                pos2(cx.min(bar_x), rect.center().y - 4.0),
                pos2(cx.max(bar_x), rect.center().y + 4.0),
            );
            p.rect_filled(bar_r, 1.5, if m.enabled { MOD_ARROW_CYAN } else { theme::TEXT_MUTED });
            p.text(
                pos2(rect.center().x, rect.min.y + 1.5),
                Align2::CENTER_TOP,
                format!("{:+}st", diff),
                fonts::semibold(7.0),
                KNOB_WHITE,
            );
        }
        ModulatorKind::Pitch12 => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_pitch12", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text("Pitch-12 — Click/drag chromatic strip to select pitch class");
            if resp.dragged() || resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let t = ((pos.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 0.999);
                    let pc = (t * 12.0) as u8;
                    m.key_note = 60 + pc;
                    *dirty = true;
                }
            }
            let active_pc = (m.key_note % 12) as usize;
            let bw = (rect.width() - 4.0) / 12.0;
            for i in 0..12 {
                let is_black = matches!(i, 1 | 3 | 6 | 8 | 10);
                let x0 = rect.min.x + 2.0 + i as f32 * bw;
                let kr = Rect::from_min_max(
                    pos2(x0, rect.min.y + 2.0),
                    pos2(x0 + bw - 0.5, if is_black { rect.center().y + 2.0 } else { rect.max.y - 2.0 }),
                );
                let col = if i == active_pc && m.enabled {
                    MOD_ARROW_CYAN
                } else if is_black {
                    Color32::from_rgb(0x28, 0x2B, 0x32)
                } else {
                    Color32::from_rgb(0x52, 0x56, 0x62)
                };
                p.rect_filled(kr, 0.8, col);
            }
        }
        ModulatorKind::NoteCounter => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_nc", track_idx, slot_idx, m.id)),
                    Sense::click(),
                )
                .on_hover_text("Note Counter — Click to advance step manually");
            if resp.clicked() {
                m.nc_count = m.nc_count.wrapping_add(1);
                *dirty = true;
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            let steps = m.nc_steps.clamp(2, 16) as u32;
            let cur = (m.nc_count % steps) + 1;
            p.text(
                rect.center(),
                Align2::CENTER_CENTER,
                format!("{cur}/{steps}"),
                fonts::semibold(10.0),
                if m.enabled { MOD_ARROW_CYAN } else { theme::TEXT_MUTED },
            );
        }
        ModulatorKind::Vibrato => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_vib", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(format!(
                    "Vibrato ({:.1} Hz, Depth {:.0}%) — Drag vertical: depth • Click: retrigger fade",
                    m.vib_rate_hz,
                    m.vib_depth * 100.0
                ));
            if resp.clicked() {
                m.env_gate_on_time = time_secs;
                *dirty = true;
            } else if resp.dragged() {
                m.vib_depth = (m.vib_depth - resp.drag_delta().y * 0.01).clamp(0.0, 1.0);
                *dirty = true;
            }
            let cy = rect.center().y;
            let mut pts = Vec::with_capacity(25);
            for i in 0..=24 {
                let t = i as f32 / 24.0;
                let fade = (t * 2.5).min(1.0);
                let yv = (t * std::f32::consts::TAU * 3.0).sin() * fade * m.vib_depth;
                pts.push(pos2(
                    rect.min.x + 2.0 + t * (rect.width() - 4.0),
                    cy - yv * (rect.height() * 0.38),
                ));
            }
            p.add(egui::Shape::line(
                pts,
                Stroke::new(1.2_f32, if m.enabled { MOD_ORANGE } else { theme::TEXT_MUTED }),
            ));
            p.circle_filled(
                pos2(rect.max.x - 4.0, cy - live_out * (rect.height() * 0.38)),
                2.2,
                Color32::WHITE,
            );
        }
        ModulatorKind::VoiceControl => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let cnt = m.voice_count.clamp(1, 16) as f32;
            let idx = (m.voice_index as f32).min(cnt - 1.0);
            let vals = [
                if cnt > 1.0 { idx / (cnt - 1.0) } else { 0.0 },
                (cnt / 16.0).clamp(0.0, 1.0),
                (m.voice_age_secs / 4.0).clamp(0.0, 1.0),
                m.expr_vel,
            ];
            let pad = 2.0;
            let gap = 2.0;
            let bw = (rect.width() - pad * 2.0 - gap * 3.0) / 4.0;
            for (i, &v) in vals.iter().enumerate() {
                let x0 = rect.min.x + pad + i as f32 * (bw + gap);
                let bh = v.clamp(0.08, 1.0) * (rect.height() - pad * 2.0);
                p.rect_filled(
                    Rect::from_min_max(pos2(x0, rect.max.y - pad - bh), pos2(x0 + bw, rect.max.y - pad)),
                    1.0,
                    if m.enabled { MOD_ARROW_CYAN } else { theme::TEXT_MUTED },
                );
            }
        }
        ModulatorKind::StackSpread => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_spread", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(format!(
                    "Stack Spread ({:+.0}%) — Drag horizontal to adjust spread",
                    m.spread_amount * 100.0
                ));
            if resp.dragged() {
                m.spread_amount = (m.spread_amount + resp.drag_delta().x * 0.02).clamp(-1.0, 1.0);
                *dirty = true;
            }
            let cx = rect.center().x;
            let n = m.voice_count.clamp(2, 8) as usize;
            let cy = rect.center().y;
            for i in 0..n {
                let norm = i as f32 / (n - 1) as f32;
                let off = (norm * 2.0 - 1.0) * m.spread_amount * (rect.width() * 0.42);
                let is_sel = i == m.voice_index as usize;
                p.line_segment(
                    [pos2(cx + off, rect.min.y + 3.0), pos2(cx + off, rect.max.y - 3.0)],
                    Stroke::new(
                        if is_sel { 2.0_f32 } else { 1.1_f32 },
                        if !m.enabled {
                            theme::TEXT_MUTED
                        } else if is_sel {
                            MOD_ARROW_CYAN
                        } else {
                            MOD_ORANGE
                        },
                    ),
                );
            }
            p.circle_filled(pos2(cx, cy), 1.5, CELL_BORDER);
        }
        ModulatorKind::WavetableLfo => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_wtlfo", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(format!(
                    "Wavetable LFO (Index {:.0}%) — Drag vertical to morph wavetable",
                    m.wt_index * 100.0
                ));
            if resp.dragged() {
                m.wt_index = (m.wt_index - resp.drag_delta().y * 0.012).clamp(0.0, 1.0);
                *dirty = true;
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
            }
            let cy = rect.center().y;
            let mut pts = Vec::with_capacity(25);
            for i in 0..=24 {
                let t = i as f32 / 24.0;
                let yv = ModulatorSlotState::eval_wavetable_lfo(t, m.wt_index);
                pts.push(pos2(
                    rect.min.x + 2.0 + t * (rect.width() - 4.0),
                    cy - yv * (rect.height() * 0.38),
                ));
            }
            p.add(egui::Shape::line(
                pts,
                Stroke::new(1.25_f32, if m.enabled { MOD_ARROW_CYAN } else { theme::TEXT_MUTED }),
            ));
            let dx = rect.min.x + 2.0 + m.lfo_last_phase * (rect.width() - 4.0);
            let dy = cy - live_out * (rect.height() * 0.38);
            p.circle_filled(pos2(dx, dy), 2.1, Color32::WHITE);
        }
        ModulatorKind::Channel16 => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_ch16", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(format!(
                    "Channel-16 (Ch {}) — Click/drag to select channel & level",
                    m.ch16_active + 1
                ));
            if resp.dragged() || resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let tx = ((pos.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 0.999);
                    let ty = (1.0 - (pos.y - rect.min.y) / rect.height().max(1.0)).clamp(0.0, 1.0);
                    let ch = (tx * 16.0) as usize;
                    m.ch16_active = ch as u8;
                    m.ch16_levels[ch] = ty.max(0.1);
                    *dirty = true;
                }
            }
            let pad = 1.5;
            let bw = ((rect.width() - pad * 2.0) / 16.0).max(1.5);
            for i in 0..16 {
                let x0 = rect.min.x + pad + i as f32 * bw;
                let bh = m.ch16_levels[i].clamp(0.08, 1.0) * (rect.height() - pad * 2.0);
                let is_act = i == m.ch16_active as usize;
                let col = if !m.enabled {
                    theme::TEXT_MUTED
                } else if is_act {
                    MOD_ARROW_CYAN
                } else if !m.ch16_exclusive && m.ch16_levels[i] > 0.05 {
                    MOD_ORANGE
                } else {
                    Color32::from_rgb(0x3C, 0x42, 0x4E)
                };
                p.rect_filled(
                    Rect::from_min_max(pos2(x0, rect.max.y - pad - bh), pos2(x0 + bw - 0.5, rect.max.y - pad)),
                    0.5,
                    col,
                );
            }
        }
        ModulatorKind::Globals => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let vals = [
                m.last_outputs[0],
                m.last_outputs[1],
                m.last_outputs[2],
                m.last_outputs[3],
            ];
            let pad = 2.0;
            let gap = 2.0;
            let bw = (rect.width() - pad * 2.0 - gap * 3.0) / 4.0;
            for (i, &v) in vals.iter().enumerate() {
                let x0 = rect.min.x + pad + i as f32 * (bw + gap);
                let bh = v.clamp(0.06, 1.0) * (rect.height() - pad * 2.0);
                p.rect_filled(
                    Rect::from_min_max(pos2(x0, rect.max.y - pad - bh), pos2(x0 + bw, rect.max.y - pad)),
                    1.0,
                    if m.enabled {
                        if i == 3 && v > 0.5 { MOD_ORANGE } else { MOD_ARROW_CYAN }
                    } else {
                        theme::TEXT_MUTED
                    },
                );
            }
        }
        ModulatorKind::EnvelopeFollower | ModulatorKind::AudioSidechain => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_aud_env", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(format!(
                    "{} (Out: {:+.0}%) — Drag vertical to audition audio level",
                    m.kind.meta().name,
                    live_out * 100.0
                ));
            if resp.dragged() || resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    m.audio_input_level = (1.0 - (pos.y - rect.min.y) / rect.height().max(1.0)).clamp(0.0, 1.0);
                    *dirty = true;
                }
            }
            let pad = 2.5;
            let h = rect.height() - pad * 2.0;
            let in_h = m.audio_input_level.clamp(0.0, 1.0) * h;
            let env_h = live_out.abs().clamp(0.0, 1.0) * h;
            let mid_x = rect.center().x;
            p.rect_filled(
                Rect::from_min_max(pos2(rect.min.x + pad, rect.max.y - pad - in_h), pos2(mid_x - 1.0, rect.max.y - pad)),
                1.0,
                Color32::from_rgb(0x4E, 0x58, 0x68),
            );
            p.rect_filled(
                Rect::from_min_max(pos2(mid_x + 1.0, rect.max.y - pad - env_h), pos2(rect.max.x - pad, rect.max.y - pad)),
                1.0,
                if m.enabled {
                    if m.kind == ModulatorKind::AudioSidechain && m.audio_sc_invert {
                        MOD_ORANGE
                    } else {
                        MOD_ARROW_CYAN
                    }
                } else {
                    theme::TEXT_MUTED
                },
            );
            if m.kind == ModulatorKind::AudioSidechain && m.audio_threshold > 0.01 {
                let ty = rect.max.y - pad - m.audio_threshold.clamp(0.0, 1.0) * h;
                p.line_segment(
                    [pos2(rect.min.x + 2.0, ty), pos2(rect.max.x - 2.0, ty)],
                    Stroke::new(0.9_f32, MOD_ORANGE),
                );
            }
        }
        ModulatorKind::AudioRate => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_audrate", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(format!(
                    "Audio Rate ({:.0} Hz) — Click: cycle wave • Drag vertical: freq",
                    m.ar_freq_hz
                ));
            if resp.clicked() {
                m.ar_wave = (m.ar_wave + 1) % 4;
                *dirty = true;
            } else if resp.dragged() {
                let delta = -resp.drag_delta().y * 0.015;
                m.ar_freq_hz = (m.ar_freq_hz * (1.0 + delta)).clamp(20.0, 2000.0);
                *dirty = true;
            }
            let cy = rect.center().y;
            let mut pts = Vec::with_capacity(25);
            for i in 0..=24 {
                let t = i as f32 / 24.0;
                let cycles = 3.0;
                let yv = (t * std::f32::consts::TAU * cycles).sin();
                let v = if m.ar_rectify { yv.abs() } else { yv };
                pts.push(pos2(
                    rect.min.x + 2.0 + t * (rect.width() - 4.0),
                    cy - v * (rect.height() * 0.36),
                ));
            }
            p.add(egui::Shape::line(
                pts,
                Stroke::new(1.1_f32, if m.enabled { MOD_ORANGE } else { theme::TEXT_MUTED }),
            ));
            p.text(
                pos2(rect.max.x - 2.0, rect.min.y + 1.5),
                Align2::RIGHT_TOP,
                format!("{:.0}Hz", m.ar_freq_hz),
                fonts::semibold(6.5),
                Color32::from_rgb(0xCC, 0xD2, 0xDC),
            );
        }
        ModulatorKind::HwCvIn => {
            p.rect_filled(rect, 2.0, CELL_WELL_BG);
            let resp = ui
                .interact(
                    rect,
                    ui.id().with(("bay_hwcv", track_idx, slot_idx, m.id)),
                    Sense::click_and_drag(),
                )
                .on_hover_text(format!(
                    "HW CV In (Ch {}, {:+.2}) — Drag horizontal to audition CV",
                    m.cv_channel, live_out
                ));
            if resp.dragged() || resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let t = ((pos.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 1.0);
                    m.cv_level = if m.cv_bipolar { t * 2.0 - 1.0 } else { t };
                    *dirty = true;
                }
            }
            let cx = if m.cv_bipolar { rect.center().x } else { rect.min.x + 3.0 };
            let bar_x = if m.cv_bipolar {
                cx + live_out.clamp(-1.0, 1.0) * (rect.width() * 0.44)
            } else {
                cx + live_out.clamp(0.0, 1.0) * (rect.width() - 6.0)
            };
            p.rect_filled(
                Rect::from_min_max(
                    pos2(cx.min(bar_x), rect.center().y - 4.0),
                    pos2(cx.max(bar_x), rect.center().y + 4.0),
                ),
                1.5,
                if m.enabled { MOD_ARROW_CYAN } else { theme::TEXT_MUTED },
            );
            p.text(
                pos2(rect.min.x + 2.5, rect.min.y + 1.5),
                Align2::LEFT_TOP,
                format!("CV{}", m.cv_channel),
                fonts::semibold(7.0),
                KNOB_WHITE,
            );
        }
    }
}

fn render_modulator_inspector(ui: &mut Ui, m: &mut ModulatorSlotState, dirty: &mut bool) {
    ui.set_min_width(196.0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(m.kind.meta().name)
                .font(fonts::semibold(11.0))
                .color(MOD_ARROW_CYAN),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.checkbox(&mut m.enabled, "Active").changed() {
                *dirty = true;
            }
        });
    });
    ui.separator();

    match m.kind {
        ModulatorKind::ClassicLfo => {
            ui.label(
                RichText::new("Waveform")
                    .font(fonts::semibold(9.5))
                    .color(theme::TEXT_SECONDARY),
            );
            ui.horizontal_wrapped(|ui| {
                for &wf in &LfoWaveform::ALL {
                    if ui.selectable_label(m.lfo_waveform == wf, wf.label()).clicked() {
                        m.lfo_waveform = wf;
                        *dirty = true;
                    }
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(m.lfo_sync_mode == LfoSyncMode::FreeHz, "Hz")
                    .clicked()
                {
                    m.lfo_sync_mode = LfoSyncMode::FreeHz;
                    *dirty = true;
                }
                if ui
                    .selectable_label(m.lfo_sync_mode == LfoSyncMode::TempoSync, "Sync")
                    .clicked()
                {
                    m.lfo_sync_mode = LfoSyncMode::TempoSync;
                    *dirty = true;
                }
                if ui
                    .selectable_label(m.lfo_bipolar, if m.lfo_bipolar { "± Bipolar" } else { "+ Unipolar" })
                    .clicked()
                {
                    m.lfo_bipolar = !m.lfo_bipolar;
                    *dirty = true;
                }
            });
            match m.lfo_sync_mode {
                LfoSyncMode::FreeHz => {
                    if ui
                        .add(
                            egui::Slider::new(&mut m.lfo_rate_hz, 0.05..=25.0)
                                .logarithmic(true)
                                .suffix(" Hz")
                                .text("Rate"),
                        )
                        .changed()
                    {
                        *dirty = true;
                    }
                }
                LfoSyncMode::TempoSync => {
                    let mut idx = SyncRate::ALL
                        .iter()
                        .position(|&r| r == m.lfo_sync_rate)
                        .unwrap_or(4);
                    if ui
                        .add(
                            egui::Slider::new(&mut idx, 0..=SyncRate::ALL.len() - 1)
                                .custom_formatter(|v, _| {
                                    SyncRate::ALL
                                        .get(v.round() as usize)
                                        .map(|r| r.label())
                                        .unwrap_or("1/4")
                                        .to_string()
                                })
                                .text("Rate"),
                        )
                        .changed()
                    {
                        m.lfo_sync_rate = SyncRate::ALL[idx];
                        *dirty = true;
                    }
                }
            }
            if ui
                .add(egui::Slider::new(&mut m.lfo_depth, 0.0..=1.0).text("Depth"))
                .changed()
            {
                *dirty = true;
            }
            if ui
                .add(
                    egui::Slider::new(&mut m.lfo_phase_deg, 0.0..=360.0)
                        .suffix("°")
                        .text("Phase"),
                )
                .changed()
            {
                *dirty = true;
            }
            ui.horizontal(|ui| {
                if ui.checkbox(&mut m.lfo_retrigger, "Retrigger").changed() {
                    *dirty = true;
                }
                if ui.checkbox(&mut m.lfo_per_voice, "Per-Voice").changed() {
                    *dirty = true;
                }
            });
        }
        ModulatorKind::BeatLfo => {
            ui.label(
                RichText::new("Waveform")
                    .font(fonts::semibold(9.5))
                    .color(theme::TEXT_SECONDARY),
            );
            ui.horizontal_wrapped(|ui| {
                for &wf in &LfoWaveform::ALL {
                    if ui.selectable_label(m.lfo_waveform == wf, wf.label()).clicked() {
                        m.lfo_waveform = wf;
                        *dirty = true;
                    }
                }
            });
            ui.separator();
            let mut idx = SyncRate::ALL
                .iter()
                .position(|&r| r == m.lfo_sync_rate)
                .unwrap_or(4);
            if ui
                .add(
                    egui::Slider::new(&mut idx, 0..=SyncRate::ALL.len() - 1)
                        .custom_formatter(|v, _| {
                            SyncRate::ALL
                                .get(v.round() as usize)
                                .map(|r| r.label())
                                .unwrap_or("1/4")
                                .to_string()
                        })
                        .text("Beat Rate"),
                )
                .changed()
            {
                m.lfo_sync_rate = SyncRate::ALL[idx];
                *dirty = true;
            }
            if ui
                .add(egui::Slider::new(&mut m.beat_lfo_skew, 0.05..=0.95).text("Skew"))
                .changed()
            {
                *dirty = true;
            }
            if ui
                .add(egui::Slider::new(&mut m.lfo_depth, 0.0..=1.0).text("Depth"))
                .changed()
            {
                *dirty = true;
            }
            if ui
                .add(
                    egui::Slider::new(&mut m.lfo_phase_deg, 0.0..=360.0)
                        .suffix("°")
                        .text("Phase"),
                )
                .changed()
            {
                *dirty = true;
            }
            if ui.checkbox(&mut m.lfo_bipolar, "Bipolar (±)").changed() {
                *dirty = true;
            }
        }
        ModulatorKind::Macro => {
            if ui
                .add(egui::Slider::new(&mut m.macro_value, 0.0..=1.0).text("Value"))
                .changed()
            {
                *dirty = true;
            }
            if ui.checkbox(&mut m.macro_bipolar, "Bipolar (±)").changed() {
                *dirty = true;
            }
        }
        ModulatorKind::Macro4 => {
            for i in 0..4 {
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Slider::new(&mut m.macro4_values[i], 0.0..=1.0)
                                .text(format!("M{}", i + 1)),
                        )
                        .changed()
                    {
                        *dirty = true;
                    }
                    if ui
                        .selectable_label(m.macro4_bipolar[i], "±")
                        .on_hover_text("Toggle Bipolar (±)")
                        .clicked()
                    {
                        m.macro4_bipolar[i] = !m.macro4_bipolar[i];
                        *dirty = true;
                    }
                });
            }
        }
        ModulatorKind::Button => {
            if ui.checkbox(&mut m.button_on, "Engaged (ON)").changed() {
                *dirty = true;
            }
            if ui
                .checkbox(&mut m.button_momentary, "Momentary Mode")
                .changed()
            {
                *dirty = true;
            }
        }
        ModulatorKind::Buttons => {
            for i in 0..4 {
                ui.horizontal(|ui| {
                    if ui
                        .checkbox(&mut m.buttons_on[i], format!("B{} ON", i + 1))
                        .changed()
                    {
                        *dirty = true;
                    }
                    if ui
                        .checkbox(&mut m.buttons_momentary[i], "Momentary")
                        .changed()
                    {
                        *dirty = true;
                    }
                });
            }
        }
        ModulatorKind::Xy => {
            if ui
                .add(egui::Slider::new(&mut m.xy_x, 0.0..=1.0).text("X"))
                .changed()
            {
                *dirty = true;
            }
            if ui
                .add(egui::Slider::new(&mut m.xy_y, 0.0..=1.0).text("Y"))
                .changed()
            {
                *dirty = true;
            }
            ui.horizontal(|ui| {
                if ui.checkbox(&mut m.xy_bipolar, "Bipolar (±)").changed() {
                    *dirty = true;
                }
                if ui.button("Center").clicked() {
                    m.xy_x = 0.5;
                    m.xy_y = 0.5;
                    *dirty = true;
                }
            });
        }
        ModulatorKind::Math => {
            ui.label(
                RichText::new("Operation")
                    .font(fonts::semibold(9.5))
                    .color(theme::TEXT_SECONDARY),
            );
            ui.horizontal_wrapped(|ui| {
                for &op in &MathOp::ALL {
                    if ui.selectable_label(m.math_op == op, op.short_symbol()).clicked() {
                        m.math_op = op;
                        *dirty = true;
                    }
                }
            });
            ui.separator();
            if ui
                .add(egui::Slider::new(&mut m.math_a, -1.0..=1.0).text("Input A"))
                .changed()
            {
                *dirty = true;
            }
            if ui
                .add(egui::Slider::new(&mut m.math_b, -1.0..=1.0).text("Input B"))
                .changed()
            {
                *dirty = true;
            }
            ui.label(
                RichText::new(format!("Output: {:+.3}", m.math_op.apply(m.math_a, m.math_b)))
                    .font(fonts::regular(9.0))
                    .color(MOD_ARROW_CYAN),
            );
        }
        ModulatorKind::Mix => {
            if ui
                .add(egui::Slider::new(&mut m.mix_a, -1.0..=1.0).text("Input A"))
                .changed()
            {
                *dirty = true;
            }
            if ui
                .add(egui::Slider::new(&mut m.mix_b, -1.0..=1.0).text("Input B"))
                .changed()
            {
                *dirty = true;
            }
            if ui
                .add(egui::Slider::new(&mut m.mix_amount, 0.0..=1.0).text("Mix"))
                .changed()
            {
                *dirty = true;
            }
            let out = (m.mix_a * (1.0 - m.mix_amount) + m.mix_b * m.mix_amount).clamp(-1.0, 1.0);
            ui.label(
                RichText::new(format!("Output: {:+.3}", out))
                    .font(fonts::regular(9.0))
                    .color(MOD_ARROW_CYAN),
            );
        }
        ModulatorKind::Quantize => {
            let range = if m.quant_bipolar { -1.0..=1.0 } else { 0.0..=1.0 };
            if ui
                .add(egui::Slider::new(&mut m.quant_in, range).text("Input"))
                .changed()
            {
                *dirty = true;
            }
            if ui
                .add(egui::Slider::new(&mut m.quant_steps, 2..=32).text("Steps"))
                .changed()
            {
                *dirty = true;
            }
            if ui.checkbox(&mut m.quant_bipolar, "Bipolar (±)").changed() {
                *dirty = true;
            }
            ui.label(
                RichText::new(format!("Output: {:+.3}", m.eval_quantize(m.quant_in)))
                    .font(fonts::regular(9.0))
                    .color(MOD_ARROW_CYAN),
            );
        }
        ModulatorKind::Select4 => {
            if ui
                .add(egui::Slider::new(&mut m.sel4_index, 0.0..=1.0).text("Select"))
                .changed()
            {
                *dirty = true;
            }
            if ui.checkbox(&mut m.sel4_smooth, "Smooth Morph").changed() {
                *dirty = true;
            }
            ui.separator();
            for i in 0..4 {
                if ui
                    .add(
                        egui::Slider::new(&mut m.sel4_values[i], -1.0..=1.0)
                            .text(format!("In {}", i + 1)),
                    )
                    .changed()
                {
                    *dirty = true;
                }
            }
        }
        ModulatorKind::Polynom => {
            if ui
                .add(egui::Slider::new(&mut m.poly_in, -1.0..=1.0).text("Input (x)"))
                .changed()
            {
                *dirty = true;
            }
            ui.separator();
            if ui
                .add(egui::Slider::new(&mut m.poly_c0, -2.0..=2.0).text("x⁰ (Offset)"))
                .changed()
            {
                *dirty = true;
            }
            if ui
                .add(egui::Slider::new(&mut m.poly_c1, -2.0..=2.0).text("x¹ (Linear)"))
                .changed()
            {
                *dirty = true;
            }
            if ui
                .add(egui::Slider::new(&mut m.poly_c2, -2.0..=2.0).text("x² (Quad)"))
                .changed()
            {
                *dirty = true;
            }
            if ui
                .add(egui::Slider::new(&mut m.poly_c3, -2.0..=2.0).text("x³ (Cubic)"))
                .changed()
            {
                *dirty = true;
            }
            ui.horizontal(|ui| {
                if ui.small_button("Linear").clicked() {
                    m.poly_c0 = 0.0;
                    m.poly_c1 = 1.0;
                    m.poly_c2 = 0.0;
                    m.poly_c3 = 0.0;
                    *dirty = true;
                }
                if ui.small_button("S-Curve").clicked() {
                    m.poly_c0 = 0.0;
                    m.poly_c1 = 1.5;
                    m.poly_c2 = 0.0;
                    m.poly_c3 = -0.5;
                    *dirty = true;
                }
                if ui.small_button("Parabola").clicked() {
                    m.poly_c0 = 0.0;
                    m.poly_c1 = 0.0;
                    m.poly_c2 = 1.0;
                    m.poly_c3 = 0.0;
                    *dirty = true;
                }
            });
            ui.label(
                RichText::new(format!("Output: {:+.3}", m.eval_polynom(m.poly_in)))
                    .font(fonts::regular(9.0))
                    .color(MOD_ARROW_CYAN),
            );
        }
        ModulatorKind::Random => {
            render_clock_mode_controls(ui, m, dirty);
            if ui
                .add(egui::Slider::new(&mut m.rand_smooth, 0.0..=1.0).text("Smooth"))
                .changed()
            {
                *dirty = true;
            }
            if ui
                .add(egui::Slider::new(&mut m.lfo_depth, 0.0..=1.0).text("Depth"))
                .changed()
            {
                *dirty = true;
            }
            if ui.checkbox(&mut m.lfo_bipolar, "Bipolar (±)").changed() {
                *dirty = true;
            }
        }
        ModulatorKind::SampleAndHold => {
            ui.horizontal(|ui| {
                for (i, label) in ["Noise", "Sine", "Tri", "Saw"].iter().enumerate() {
                    if ui.selectable_label(m.sh_source == i as u8, *label).clicked() {
                        m.sh_source = i as u8;
                        *dirty = true;
                    }
                }
            });
            ui.separator();
            render_clock_mode_controls(ui, m, dirty);
            if ui
                .add(egui::Slider::new(&mut m.sh_slew, 0.0..=1.0).text("Slew"))
                .changed()
            {
                *dirty = true;
            }
            if ui
                .add(egui::Slider::new(&mut m.lfo_depth, 0.0..=1.0).text("Depth"))
                .changed()
            {
                *dirty = true;
            }
            if ui.checkbox(&mut m.lfo_bipolar, "Bipolar (±)").changed() {
                *dirty = true;
            }
        }
        ModulatorKind::Steps => {
            render_clock_mode_controls(ui, m, dirty);
            if ui
                .add(egui::Slider::new(&mut m.steps_len, 2..=16).text("Steps"))
                .changed()
            {
                *dirty = true;
            }
            if ui
                .add(egui::Slider::new(&mut m.steps_smooth, 0.0..=1.0).text("Smooth"))
                .changed()
            {
                *dirty = true;
            }
            if ui.checkbox(&mut m.steps_bipolar, "Bipolar (±)").changed() {
                *dirty = true;
            }
        }
        ModulatorKind::ParSeq8 => {
            render_clock_mode_controls(ui, m, dirty);
            if ui
                .add(egui::Slider::new(&mut m.parseq_len, 2..=8).text("Length"))
                .changed()
            {
                *dirty = true;
            }
            if ui
                .add(egui::Slider::new(&mut m.parseq_smooth, 0.0..=1.0).text("Crossfade"))
                .changed()
            {
                *dirty = true;
            }
            ui.separator();
            for i in 0..8 {
                if ui
                    .add(
                        egui::Slider::new(&mut m.parseq_steps[i], 0.0..=1.0)
                            .text(format!("Out {}", i + 1)),
                    )
                    .changed()
                {
                    *dirty = true;
                }
            }
        }
        ModulatorKind::Curves => {
            render_clock_mode_controls(ui, m, dirty);
            if ui
                .add(egui::Slider::new(&mut m.lfo_depth, 0.0..=1.0).text("Depth"))
                .changed()
            {
                *dirty = true;
            }
            ui.horizontal(|ui| {
                if ui.checkbox(&mut m.curves_smooth, "Smooth").changed() {
                    *dirty = true;
                }
                if ui.checkbox(&mut m.curves_bipolar, "Bipolar (±)").changed() {
                    *dirty = true;
                }
            });
        }
        ModulatorKind::Ramp => {
            if ui
                .add(
                    egui::Slider::new(&mut m.ramp_time_secs, 0.05..=16.0)
                        .logarithmic(true)
                        .suffix(" s")
                        .text("Time"),
                )
                .changed()
            {
                *dirty = true;
            }
            if ui
                .add(egui::Slider::new(&mut m.ramp_curve, -1.0..=1.0).text("Curve"))
                .changed()
            {
                *dirty = true;
            }
            ui.horizontal(|ui| {
                if ui.checkbox(&mut m.ramp_loop, "Loop").changed() {
                    *dirty = true;
                }
                if ui.checkbox(&mut m.ramp_down, "Invert").changed() {
                    *dirty = true;
                }
                if ui.checkbox(&mut m.ramp_bipolar, "±").changed() {
                    *dirty = true;
                }
            });
            if ui.button("Retrigger Ramp").clicked() {
                m.ramp_start_time = clock_time_secs();
                *dirty = true;
            }
        }
        ModulatorKind::Segments => {
            if ui
                .add(
                    egui::Slider::new(&mut m.seg_time_secs, 0.05..=16.0)
                        .logarithmic(true)
                        .suffix(" s")
                        .text("Duration"),
                )
                .changed()
            {
                *dirty = true;
            }
            if ui
                .add(egui::Slider::new(&mut m.seg_curve, -1.0..=1.0).text("Curve"))
                .changed()
            {
                *dirty = true;
            }
            ui.horizontal(|ui| {
                if ui.checkbox(&mut m.seg_loop, "Loop").changed() {
                    *dirty = true;
                }
                if ui.checkbox(&mut m.seg_bipolar, "Bipolar (±)").changed() {
                    *dirty = true;
                }
                if ui.button("Trigger").clicked() {
                    m.seg_start_time = clock_time_secs();
                    *dirty = true;
                }
            });
            ui.separator();
            for i in 0..6 {
                if ui
                    .add(
                        egui::Slider::new(&mut m.seg_levels[i], 0.0..=1.0)
                            .text(format!("Pt {}", i + 1)),
                    )
                    .changed()
                {
                    *dirty = true;
                }
            }
        }
        ModulatorKind::Vector4 => {
            if ui.add(egui::Slider::new(&mut m.xy_x, 0.0..=1.0).text("X")).changed() {
                *dirty = true;
            }
            if ui.add(egui::Slider::new(&mut m.xy_y, 0.0..=1.0).text("Y")).changed() {
                *dirty = true;
            }
            if ui.button("Center").clicked() {
                m.xy_x = 0.5;
                m.xy_y = 0.5;
                *dirty = true;
            }
        }
        ModulatorKind::Vector8 => {
            if ui.add(egui::Slider::new(&mut m.xy_x, 0.0..=1.0).text("X")).changed() {
                *dirty = true;
            }
            if ui.add(egui::Slider::new(&mut m.xy_y, 0.0..=1.0).text("Y")).changed() {
                *dirty = true;
            }
            if ui.add(egui::Slider::new(&mut m.vec8_z, 0.0..=1.0).text("Z")).changed() {
                *dirty = true;
            }
            if ui.button("Center Cube").clicked() {
                m.xy_x = 0.5;
                m.xy_y = 0.5;
                m.vec8_z = 0.5;
                *dirty = true;
            }
        }
        ModulatorKind::Adsr => {
            if ui.add(egui::Slider::new(&mut m.env_attack, 0.002..=5.0).logarithmic(true).suffix(" s").text("Attack")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.env_decay, 0.002..=5.0).logarithmic(true).suffix(" s").text("Decay")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.env_sustain, 0.0..=1.0).text("Sustain")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.env_release, 0.002..=5.0).logarithmic(true).suffix(" s").text("Release")).changed() { *dirty = true; }
            if ui.button(if m.env_gate { "Release Gate" } else { "Trigger Gate" }).clicked() {
                m.trigger_gate(!m.env_gate, clock_time_secs());
                *dirty = true;
            }
        }
        ModulatorKind::Ahdsr => {
            if ui.add(egui::Slider::new(&mut m.env_attack, 0.002..=5.0).logarithmic(true).suffix(" s").text("Attack")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.env_hold, 0.0..=2.0).suffix(" s").text("Hold")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.env_decay, 0.002..=5.0).logarithmic(true).suffix(" s").text("Decay")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.env_sustain, 0.0..=1.0).text("Sustain")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.env_release, 0.002..=5.0).logarithmic(true).suffix(" s").text("Release")).changed() { *dirty = true; }
            if ui.button(if m.env_gate { "Release Gate" } else { "Trigger Gate" }).clicked() {
                m.trigger_gate(!m.env_gate, clock_time_secs());
                *dirty = true;
            }
        }
        ModulatorKind::FourStage => {
            for i in 0..4 {
                ui.horizontal(|ui| {
                    if ui.add(egui::Slider::new(&mut m.env_4stage_times[i], 0.01..=4.0).suffix("s").text(format!("T{}", i + 1))).changed() { *dirty = true; }
                    if ui.add(egui::Slider::new(&mut m.env_4stage_levels[i], 0.0..=1.0).text(format!("L{}", i + 1))).changed() { *dirty = true; }
                });
            }
            ui.horizontal(|ui| {
                if ui.checkbox(&mut m.env_loop, "Loop").changed() { *dirty = true; }
                if ui.button(if m.env_gate { "Release" } else { "Trigger" }).clicked() {
                    m.trigger_gate(!m.env_gate, clock_time_secs());
                    *dirty = true;
                }
            });
        }
        ModulatorKind::AhdOnRelease => {
            if ui.add(egui::Slider::new(&mut m.env_attack, 0.002..=5.0).logarithmic(true).suffix(" s").text("Attack")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.env_hold, 0.0..=2.0).suffix(" s").text("Hold")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.env_decay, 0.002..=5.0).logarithmic(true).suffix(" s").text("Decay")).changed() { *dirty = true; }
            if ui.button("Fire Release").clicked() {
                m.env_gate = false;
                m.env_gate_off_time = clock_time_secs();
                *dirty = true;
            }
        }
        ModulatorKind::Expressions => {
            if ui.add(egui::Slider::new(&mut m.expr_vel, 0.0..=1.0).text("Velocity")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.expr_timbre, 0.0..=1.0).text("Timbre")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.expr_rel, 0.0..=1.0).text("Release Vel")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.expr_pres, 0.0..=1.0).text("Pressure")).changed() { *dirty = true; }
        }
        ModulatorKind::Midi => {
            if ui.add(egui::Slider::new(&mut m.midi_cc_num, 0..=127).text("CC #")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.midi_cc_val, 0.0..=1.0).text("CC Value")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.midi_mw, 0.0..=1.0).text("Mod Wheel")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.midi_pb, -1.0..=1.0).text("Pitch Bend")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.midi_at, 0.0..=1.0).text("Aftertouch")).changed() { *dirty = true; }
        }
        ModulatorKind::KeytrackPlus => {
            if ui.add(egui::Slider::new(&mut m.key_note, 0..=127).text("Note")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.key_root, 0..=127).text("Root Note")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.key_spread_semi, 1.0..=60.0).suffix(" st").text("Spread")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.key_curve, -1.0..=1.0).text("Curve")).changed() { *dirty = true; }
        }
        ModulatorKind::RelativeKeytrack => {
            if ui.add(egui::Slider::new(&mut m.key_note, 0..=127).text("Current Note")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.key_prev_note, 0..=127).text("Prev Note")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.key_spread_semi, 1.0..=48.0).suffix(" st").text("Range")).changed() { *dirty = true; }
        }
        ModulatorKind::Pitch12 => {
            if ui.add(egui::Slider::new(&mut m.key_note, 0..=127).text("Note")).changed() { *dirty = true; }
            let names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
            ui.label(
                RichText::new(format!("Active Pitch Class: {}", names[(m.key_note % 12) as usize]))
                    .font(fonts::semibold(9.5))
                    .color(MOD_ARROW_CYAN),
            );
        }
        ModulatorKind::NoteCounter => {
            if ui.add(egui::Slider::new(&mut m.nc_steps, 2..=16).text("Steps")).changed() { *dirty = true; }
            ui.horizontal(|ui| {
                for (i, lbl) in ["Up", "Down", "Ping-Pong"].iter().enumerate() {
                    if ui.selectable_label(m.nc_mode == i as u8, *lbl).clicked() {
                        m.nc_mode = i as u8;
                        *dirty = true;
                    }
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Step +1").clicked() {
                    m.nc_count = m.nc_count.wrapping_add(1);
                    *dirty = true;
                }
                if ui.button("Reset").clicked() {
                    m.nc_count = 0;
                    *dirty = true;
                }
            });
        }
        ModulatorKind::NoteSidechain => {
            if ui.add(egui::Slider::new(&mut m.env_attack, 0.002..=2.0).logarithmic(true).suffix(" s").text("Attack")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.env_release, 0.002..=3.0).logarithmic(true).suffix(" s").text("Release")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.nsc_min_note, 0..=127).text("Min Note")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.nsc_max_note, 0..=127).text("Max Note")).changed() { *dirty = true; }
            if ui.button(if m.env_gate { "Release" } else { "Trigger" }).clicked() {
                m.trigger_gate(!m.env_gate, clock_time_secs());
                *dirty = true;
            }
        }
        ModulatorKind::Vibrato => {
            if ui.add(egui::Slider::new(&mut m.vib_rate_hz, 0.2..=16.0).suffix(" Hz").text("Rate")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.vib_depth, 0.0..=1.0).text("Depth")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.vib_fade_secs, 0.0..=3.0).suffix(" s").text("Fade-In")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.vib_mw_scale, 0.0..=1.0).text("MW Scale")).changed() { *dirty = true; }
        }
        ModulatorKind::VoiceControl => {
            if ui.add(egui::Slider::new(&mut m.voice_index, 0..=15).text("Voice Index")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.voice_count, 1..=16).text("Voice Count")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.voice_age_secs, 0.0..=4.0).suffix(" s").text("Voice Age")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.expr_vel, 0.0..=1.0).text("Velocity")).changed() { *dirty = true; }
        }
        ModulatorKind::StackSpread => {
            if ui.add(egui::Slider::new(&mut m.spread_amount, -1.0..=1.0).text("Spread")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.voice_index, 0..=15).text("Voice Idx")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.voice_count, 1..=16).text("Stack Size")).changed() { *dirty = true; }
            ui.horizontal(|ui| {
                for (i, lbl) in ["Linear", "Center", "Harmonic"].iter().enumerate() {
                    if ui.selectable_label(m.spread_mode == i as u8, *lbl).clicked() {
                        m.spread_mode = i as u8;
                        *dirty = true;
                    }
                }
            });
        }
        ModulatorKind::WavetableLfo => {
            render_clock_mode_controls(ui, m, dirty);
            if ui.add(egui::Slider::new(&mut m.wt_index, 0.0..=1.0).text("WT Index")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.lfo_depth, 0.0..=1.0).text("Depth")).changed() { *dirty = true; }
            if ui.checkbox(&mut m.lfo_bipolar, "Bipolar (±)").changed() { *dirty = true; }
        }
        ModulatorKind::Channel16 => {
            let mut ch_display = m.ch16_active + 1;
            if ui.add(egui::Slider::new(&mut ch_display, 1..=16).text("Active Ch")).changed() {
                m.ch16_active = ch_display.saturating_sub(1).min(15);
                *dirty = true;
            }
            if ui.checkbox(&mut m.ch16_exclusive, "Exclusive Active Ch").changed() { *dirty = true; }
            let idx = (m.ch16_active as usize).min(15);
            if ui.add(egui::Slider::new(&mut m.ch16_levels[idx], 0.0..=1.0).text(format!("Ch {} Level", idx + 1))).changed() {
                *dirty = true;
            }
        }
        ModulatorKind::Globals => {
            ui.label(
                RichText::new("Outputs: BPM (norm), Bar phase, Beat phase, Play gate")
                    .font(fonts::regular(9.0))
                    .color(theme::TEXT_SECONDARY),
            );
            ui.label(
                RichText::new(format!(
                    "BPM:{:.2}  BAR:{:.2}  BEAT:{:.2}  PLAY:{:.0}",
                    m.last_outputs[0], m.last_outputs[1], m.last_outputs[2], m.last_outputs[3]
                ))
                .font(fonts::regular(9.0))
                .color(MOD_ARROW_CYAN),
            );
        }
        ModulatorKind::EnvelopeFollower => {
            if ui.add(egui::Slider::new(&mut m.audio_gain, 0.0..=4.0).text("Gain")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.env_attack, 0.002..=1.0).logarithmic(true).suffix(" s").text("Attack")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.env_release, 0.002..=2.0).logarithmic(true).suffix(" s").text("Release")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.audio_input_level, 0.0..=1.0).text("Audition In")).changed() { *dirty = true; }
        }
        ModulatorKind::AudioSidechain => {
            let mut tr_disp = m.audio_sc_track + 1;
            if ui.add(egui::Slider::new(&mut tr_disp, 1..=16).text("Source Track")).changed() {
                m.audio_sc_track = tr_disp.saturating_sub(1);
                *dirty = true;
            }
            if ui.add(egui::Slider::new(&mut m.audio_gain, 0.0..=4.0).text("Gain")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.audio_threshold, 0.0..=1.0).text("Threshold")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.env_attack, 0.002..=1.0).logarithmic(true).suffix(" s").text("Attack")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.env_release, 0.002..=2.0).logarithmic(true).suffix(" s").text("Release")).changed() { *dirty = true; }
            if ui.checkbox(&mut m.audio_sc_invert, "Invert (Ducking)").changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.audio_input_level, 0.0..=1.0).text("Audition In")).changed() { *dirty = true; }
        }
        ModulatorKind::AudioRate => {
            ui.horizontal(|ui| {
                for (i, lbl) in ["Sine", "Tri", "Saw", "Live"].iter().enumerate() {
                    if ui.selectable_label(m.ar_wave == i as u8, *lbl).clicked() {
                        m.ar_wave = i as u8;
                        *dirty = true;
                    }
                }
            });
            if ui.add(egui::Slider::new(&mut m.ar_freq_hz, 20.0..=2000.0).logarithmic(true).suffix(" Hz").text("Freq")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.audio_gain, 0.0..=2.0).text("Gain")).changed() { *dirty = true; }
            if ui.checkbox(&mut m.ar_rectify, "Rectify (Unipolar)").changed() { *dirty = true; }
        }
        ModulatorKind::HwCvIn => {
            if ui.add(egui::Slider::new(&mut m.cv_channel, 1..=8).text("CV Channel")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.audio_gain, 0.0..=4.0).text("Gain")).changed() { *dirty = true; }
            if ui.add(egui::Slider::new(&mut m.cv_offset, -1.0..=1.0).text("Offset")).changed() { *dirty = true; }
            if ui.checkbox(&mut m.cv_bipolar, "Bipolar (±)").changed() { *dirty = true; }
            let range = if m.cv_bipolar { -1.0..=1.0 } else { 0.0..=1.0 };
            if ui.add(egui::Slider::new(&mut m.cv_level, range).text("CV Input")).changed() { *dirty = true; }
        }
    }
}

fn render_clock_mode_controls(ui: &mut Ui, m: &mut ModulatorSlotState, dirty: &mut bool) {
    ui.horizontal(|ui| {
        if ui
            .selectable_label(m.lfo_sync_mode == LfoSyncMode::FreeHz, "Hz")
            .clicked()
        {
            m.lfo_sync_mode = LfoSyncMode::FreeHz;
            *dirty = true;
        }
        if ui
            .selectable_label(m.lfo_sync_mode == LfoSyncMode::TempoSync, "Sync")
            .clicked()
        {
            m.lfo_sync_mode = LfoSyncMode::TempoSync;
            *dirty = true;
        }
    });
    match m.lfo_sync_mode {
        LfoSyncMode::FreeHz => {
            if ui
                .add(
                    egui::Slider::new(&mut m.lfo_rate_hz, 0.05..=25.0)
                        .logarithmic(true)
                        .suffix(" Hz")
                        .text("Rate"),
                )
                .changed()
            {
                *dirty = true;
            }
        }
        LfoSyncMode::TempoSync => {
            let mut idx = SyncRate::ALL
                .iter()
                .position(|&r| r == m.lfo_sync_rate)
                .unwrap_or(4);
            if ui
                .add(
                    egui::Slider::new(&mut idx, 0..=SyncRate::ALL.len() - 1)
                        .custom_formatter(|v, _| {
                            SyncRate::ALL
                                .get(v.round() as usize)
                                .map(|r| r.label())
                                .unwrap_or("1/4")
                                .to_string()
                        })
                        .text("Rate"),
                )
                .changed()
            {
                m.lfo_sync_rate = SyncRate::ALL[idx];
                *dirty = true;
            }
        }
    }
}
