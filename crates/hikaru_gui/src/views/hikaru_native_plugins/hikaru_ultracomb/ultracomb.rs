// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_ultracomb/ultracomb.rs

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use crate::app::AppState;
use crate::theme;

pub const ULTRACOMB_CARD_WIDTH: f32 = 176.0;

const PARAM_NAMES: [&str; 2] = ["Cutoff", "Resonance"];
const DEFAULTS: [f32; 2] = [50.0, 60.0];

type StateMap = Mutex<HashMap<(usize, usize), [f32; 2]>>;
type LoadedSet = Mutex<HashSet<(usize, usize)>>;
type WetStateMap = Mutex<HashMap<(usize, usize, usize), [f32; 2]>>;

fn state_map() -> &'static StateMap {
    static CELL: OnceLock<StateMap> = OnceLock::new();
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

pub fn clear_wet_fx_state(track_idx: usize, parent_slot_idx: usize) {
    let mut map = wet_state_map().lock().unwrap();
    map.retain(|k, _| !(k.0 == track_idx && k.1 == parent_slot_idx));
}

pub fn remove_wet_fx_slot_state(track_idx: usize, parent_slot_idx: usize, wet_idx: usize) {
    let mut map = wet_state_map().lock().unwrap();
    let old_entries: Vec<((usize, usize, usize), [f32; 2])> = map
        .iter()
        .filter(|(k, _)| k.0 == track_idx && k.1 == parent_slot_idx)
        .map(|(k, v)| (*k, *v))
        .collect();
    map.retain(|k, _| !(k.0 == track_idx && k.1 == parent_slot_idx));
    for ((t, p, w), vals) in old_entries {
        if w < wet_idx {
            map.insert((t, p, w), vals);
        } else if w > wet_idx {
            map.insert((t, p, w - 1), vals);
        }
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
        *map.entry(key).or_insert(DEFAULTS)
    };
    let mut changed = false;

    ui.vertical_centered(|ui| {
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Hikaru Ultracomb")
                .size(11.0)
                .strong()
                .color(theme::ACCENT_PRIMARY),
        );
        ui.add_space(6.0);

        let avail_w = ui.available_width().max(120.0);
        let col_w = (avail_w / 2.0).floor();
        let row_h = 62.0;
        let (row_rect, _) = ui.allocate_exact_size(egui::vec2(avail_w, row_h), egui::Sense::hover());

        for (i, name) in PARAM_NAMES.iter().enumerate() {
            let cell_rect = egui::Rect::from_min_size(
                egui::pos2(row_rect.min.x + i as f32 * col_w, row_rect.min.y),
                egui::vec2(col_w, row_h),
            );
            let mut cell_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(cell_rect)
                    .layout(egui::Layout::top_down(egui::Align::Center)),
            );
            cell_ui.spacing_mut().item_spacing.y = 2.0;

            let mut val = params[i];
            let color = if i == 0 {
                theme::BW_LCD_CYAN
            } else {
                theme::ACCENT_ORANGE
            };
            let (k_resp, k_changed) = crate::ui::bitwig_kit::rotary_knob(
                &mut cell_ui,
                32.0,
                &mut val,
                0.0..=100.0,
                DEFAULTS[i],
                color,
                false,
            );
            k_resp.on_hover_text(format!("{name}: {val:.1}%"));
            if k_changed {
                params[i] = val;
                changed = true;
            }
            cell_ui.label(
                egui::RichText::new(*name)
                    .font(crate::ui::fonts::semibold(9.5))
                    .color(theme::TEXT_SECONDARY),
            );
            cell_ui.label(
                egui::RichText::new(format!("{val:.0}%"))
                    .font(crate::ui::fonts::regular(8.5))
                    .color(theme::TEXT_MUTED),
            );
        }
    });

    if changed {
        wet_state_map().lock().unwrap().insert(key, params);
        if let Some(et_idx) = track_idx.checked_sub(1) {
            for (i, &v) in params.iter().enumerate() {
                app.audio_proxy.send(crate::audio_proxy::GuiCommand::SetBit8WetFxParam {
                    track_idx: et_idx,
                    slot_idx: parent_slot_idx,
                    wet_idx,
                    param_id: i as u32,
                    value: (v / 100.0).clamp(0.0, 1.0),
                });
            }
        }
    }
}

pub fn get_params(track_idx: usize, slot_idx: usize) -> [f32; 2] {
    let mut map = state_map().lock().unwrap();
    *map.entry((track_idx, slot_idx)).or_insert(DEFAULTS)
}

pub fn set_param(track_idx: usize, slot_idx: usize, idx: usize, value: f32) {
    let mut map = state_map().lock().unwrap();
    let entry = map.entry((track_idx, slot_idx)).or_insert(DEFAULTS);
    if idx < 2 {
        entry[idx] = value.clamp(0.0, 100.0);
    }
}

pub fn render_module(ui: &mut egui::Ui, app: &mut AppState, track_idx: usize, slot_idx: usize) {
    let mut params = get_params(track_idx, slot_idx);
    let engine_track_idx = track_idx.checked_sub(1);
    if let Some(et_idx) = engine_track_idx {
        if loaded_set().lock().unwrap().insert((track_idx, slot_idx)) {
            app.audio_proxy.send(crate::audio_proxy::GuiCommand::LoadNativeEffect {
                track_idx: et_idx,
                slot_idx,
                name: "Hikaru Ultracomb".to_string(),
            });
            for (i, &v) in params.iter().enumerate() {
                app.audio_proxy.send(crate::audio_proxy::GuiCommand::SetExternalPluginParam {
                    track_idx: et_idx,
                    slot_idx,
                    param_id: i as u32,
                    value: (v / 100.0).clamp(0.0, 1.0),
                });
            }
        }
    }
    let mut changed = false;

    ui.vertical_centered(|ui| {
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Hikaru Ultracomb")
                .size(11.0)
                .strong()
                .color(theme::ACCENT_PRIMARY),
        );
        ui.add_space(6.0);

        let avail_w = ui.available_width().max(120.0);
        let col_w = (avail_w / 2.0).floor();
        let row_h = 62.0;
        let (row_rect, _) = ui.allocate_exact_size(egui::vec2(avail_w, row_h), egui::Sense::hover());

        for (i, name) in PARAM_NAMES.iter().enumerate() {
            let cell_rect = egui::Rect::from_min_size(
                egui::pos2(row_rect.min.x + i as f32 * col_w, row_rect.min.y),
                egui::vec2(col_w, row_h),
            );
            let mut cell_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(cell_rect)
                    .layout(egui::Layout::top_down(egui::Align::Center)),
            );
            cell_ui.spacing_mut().item_spacing.y = 2.0;

            let mut val = params[i];
            let color = if i == 0 {
                theme::BW_LCD_CYAN
            } else {
                theme::ACCENT_ORANGE
            };
            let (k_resp, k_changed) = crate::ui::bitwig_kit::rotary_knob(
                &mut cell_ui,
                32.0,
                &mut val,
                0.0..=100.0,
                DEFAULTS[i],
                color,
                false,
            );
            k_resp.on_hover_text(format!("{name}: {val:.1}%"));
            if k_changed {
                params[i] = val;
                changed = true;
            }
            cell_ui.label(
                egui::RichText::new(*name)
                    .font(crate::ui::fonts::semibold(9.5))
                    .color(theme::TEXT_SECONDARY),
            );
            cell_ui.label(
                egui::RichText::new(format!("{val:.0}%"))
                    .font(crate::ui::fonts::regular(8.5))
                    .color(theme::TEXT_MUTED),
            );
        }

        ui.add_space(6.0);

        // Visual Comb Frequency Response Scope
        let scope_w = (avail_w - 8.0).max(110.0);
        let scope_h = ui.available_height().clamp(44.0, 68.0);
        let (scope_rect, _) =
            ui.allocate_exact_size(egui::vec2(scope_w, scope_h), egui::Sense::hover());
        let painter = ui.painter_at(scope_rect);

        painter.rect_filled(scope_rect, 3.0, egui::Color32::from_rgb(0x0B, 0x0C, 0x0F));

        // Subtle horizontal 0 dB reference line
        let zero_db_y = scope_rect.max.y - (12.0 / 40.0) * (scope_rect.height() - 6.0) - 3.0;
        painter.line_segment(
            [
                egui::pos2(scope_rect.min.x + 2.0, zero_db_y),
                egui::pos2(scope_rect.max.x - 2.0, zero_db_y),
            ],
            egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0x1E, 0x22, 0x2B)),
        );

        let mut uc = hikaru_dsp::effects::UltracombFilter::new(48_000.0);
        uc.set_params(hikaru_dsp::effects::UltracombParams {
            cutoff: params[0],
            resonance: params[1],
        });
        let bins = 96;
        let curve = uc.frequency_response_curve(bins);
        let f0 = uc.fundamental_hz();
        let delay_ms = (uc.delay_samples() / 48_000.0) * 1000.0;

        let inner_w = (scope_rect.width() - 6.0).max(1.0);
        let inner_h = (scope_rect.height() - 6.0).max(1.0);
        let points: Vec<egui::Pos2> = curve
            .iter()
            .enumerate()
            .map(|(idx, &mag)| {
                let x = scope_rect.min.x + 3.0 + (idx as f32 / (bins - 1) as f32) * inner_w;
                let y = scope_rect.max.y - 3.0 - mag * inner_h;
                egui::pos2(x, y)
            })
            .collect();

        // Subtle filled area under curve
        let fill_color = egui::Color32::from_rgba_unmultiplied(0x33, 0xC4, 0xE8, 35);
        for w in points.windows(2) {
            let p0 = w[0];
            let p1 = w[1];
            let base0 = egui::pos2(p0.x, scope_rect.max.y - 3.0);
            let base1 = egui::pos2(p1.x, scope_rect.max.y - 3.0);
            painter.add(egui::Shape::convex_polygon(
                vec![p0, p1, base1, base0],
                fill_color,
                egui::Stroke::NONE,
            ));
        }

        // Glowing comb response line
        painter.add(egui::Shape::line(
            points,
            egui::Stroke::new(1.4_f32, theme::BW_LCD_CYAN),
        ));

        // Readout overlay: f0 & delay ms
        let f0_label = if f0 >= 1000.0 {
            format!("{:.1} kHz", f0 / 1000.0)
        } else {
            format!("{f0:.0} Hz")
        };
        painter.text(
            egui::pos2(scope_rect.min.x + 5.0, scope_rect.min.y + 4.0),
            egui::Align2::LEFT_TOP,
            format!("f0: {f0_label}"),
            crate::ui::fonts::regular(8.0),
            theme::TEXT_MUTED,
        );
        painter.text(
            egui::pos2(scope_rect.max.x - 5.0, scope_rect.min.y + 4.0),
            egui::Align2::RIGHT_TOP,
            format!("{delay_ms:.2} ms"),
            crate::ui::fonts::regular(8.0),
            theme::TEXT_MUTED,
        );

        painter.rect_stroke(
            scope_rect,
            3.0,
            egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0x2D, 0x30, 0x38)),
            egui::StrokeKind::Inside,
        );
    });

    if changed {
        for (i, &v) in params.iter().enumerate() {
            set_param(track_idx, slot_idx, i, v);
            if let Some(et_idx) = engine_track_idx {
                app.audio_proxy.send(crate::audio_proxy::GuiCommand::SetExternalPluginParam {
                    track_idx: et_idx,
                    slot_idx,
                    param_id: i as u32,
                    value: (v / 100.0).clamp(0.0, 1.0),
                });
            }
        }
    }
}
