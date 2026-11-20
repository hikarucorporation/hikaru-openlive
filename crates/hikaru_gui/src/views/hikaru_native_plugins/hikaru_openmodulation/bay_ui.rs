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
    get_bay_state, update_bay_state, ModulatorKind, ModulatorSlotState, BAY_COL_WIDTH, BAY_GAP,
    BAY_SLOTS_PER_COL,
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
        if m.enabled && m.kind == ModulatorKind::ClassicLfo {
            any_lfo_active = true;
        }
        let mod_id = m.id;
        let map_active = is_bay_mapping_active(track_idx, slot_idx, mod_id, 0);

        let border = if map_active {
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
            if map_active {
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
        let arrow_h = 15.0;
        let body_r = Rect::from_min_max(
            pos2(cell_rect.min.x + 4.0, hdr_r.max.y + 2.0),
            pos2(cell_rect.max.x - 4.0, cell_rect.max.y - arrow_h - 3.0),
        );
        render_slot_body(ui, body_r, m, outs[0], track_idx, slot_idx, &mut dirty);

        // --- Bottom Cyan Mapping Arrow (`●→`) ---
        let arrow_r = Rect::from_min_max(
            pos2(cell_rect.min.x + 4.0, cell_rect.max.y - arrow_h - 1.0),
            pos2(cell_rect.max.x - 4.0, cell_rect.max.y - 2.0),
        );
        let arrow_resp = ui.interact(
            arrow_r,
            ui.id().with(("bay_map", track_idx, slot_idx, mod_id)),
            Sense::click(),
        );
        if arrow_resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let is_assigned = m.has_routes_for_output(0);
        if map_active {
            p.rect_filled(arrow_r, 2.5, MOD_ARROW_CYAN);
        } else if is_assigned {
            let pulse = (outs[0].abs().clamp(0.0, 1.0) * 45.0) as u8;
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

        paint_bay_arrow(
            &p,
            arrow_r.center(),
            if map_active {
                Color32::from_rgb(0x10, 0x14, 0x1A)
            } else if is_assigned {
                MOD_ARROW_CYAN
            } else {
                Color32::from_rgb(0x52, 0x8A, 0xA4)
            },
            is_assigned || map_active,
        );

        let tip = if map_active {
            format!(
                "{}: Mapping active — Drag any cyan knob up/down • Click again or Esc to finish",
                m.kind.meta().name
            )
        } else if is_assigned {
            let summary: Vec<String> = m
                .routes
                .iter()
                .map(|r| format!("{} ({:+.0}%)", r.target.label(), r.amount * 100.0))
                .collect();
            format!(
                "{} → {}\nLeft-click: Map knobs • Right-click: Manage routes",
                m.kind.meta().name,
                summary.join(", ")
            )
        } else {
            format!(
                "{}: Click ●→ then drag any knob to modulate • Right-click: Target list",
                m.kind.meta().name
            )
        };
        let arrow_resp = arrow_resp.on_hover_text(tip);
        if arrow_resp.clicked() {
            toggle_bay_mapping_mode(track_idx, slot_idx, mod_id, 0);
        }
        arrow_resp.context_menu(|menu_ui| {
            menu_ui.label(
                RichText::new(format!("{} Routes", m.kind.meta().name))
                    .font(fonts::semibold(10.0))
                    .color(theme::ACCENT_ORANGE),
            );
            menu_ui.separator();
            if m.routes.is_empty() {
                menu_ui.label(
                    RichText::new("Not mapped (click ●→ then drag any cyan knob)")
                        .font(fonts::regular(9.0))
                        .color(theme::TEXT_MUTED),
                );
            } else {
                let snap = m.routes.clone();
                for r in snap {
                    let mut amt = r.amount;
                    menu_ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(r.target.label())
                                .font(fonts::regular(9.0))
                                .color(MOD_ARROW_CYAN),
                        );
                        if ui
                            .add(egui::Slider::new(&mut amt, -1.0..=1.0).fixed_decimals(2))
                            .changed()
                        {
                            m.set_route(r.out_idx, r.target, amt);
                            dirty = true;
                        }
                        if ui.small_button("×").clicked() {
                            m.set_route(r.out_idx, r.target, 0.0);
                            dirty = true;
                        }
                    });
                }
                menu_ui.separator();
                if menu_ui.button("Clear All Routes").clicked() {
                    m.clear_routes();
                    dirty = true;
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
                        .selectable_label(m.route_amount(0, target).abs() > 0.004, target.label())
                        .clicked()
                    {
                        m.set_route(0, target, 0.5);
                        dirty = true;
                        sub_ui.close_menu();
                    }
                }
            });
            menu_ui.separator();
            if menu_ui.button("Remove Modulator").clicked() {
                remove_id = Some(mod_id);
                menu_ui.close_menu();
            }
        });
    }

    // --- '+' Add Modulator Cell ---
    let plus_idx = bay.slots.len();
    let plus_col = plus_idx / BAY_SLOTS_PER_COL;
    let plus_row = plus_idx % BAY_SLOTS_PER_COL;
    let plus_rect = Rect::from_min_size(
        pos2(
            origin.x + plus_col as f32 * (BAY_COL_WIDTH + BAY_GAP),
            origin.y + plus_row as f32 * (cell_h + BAY_GAP),
        ),
        vec2(BAY_COL_WIDTH, cell_h),
    );
    let p = ui.painter().clone();
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
            ui.id().with(("bay_add_mod", track_idx, slot_idx)),
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

fn render_slot_body(
    ui: &mut Ui,
    rect: Rect,
    m: &mut ModulatorSlotState,
    live_out: f32,
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
    }
}
