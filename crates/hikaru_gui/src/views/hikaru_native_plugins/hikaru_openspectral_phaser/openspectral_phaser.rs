// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_openspectral_phaser/openspectral_phaser.rs
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::app::AppState;
use crate::theme;

const PARAM_NAMES: [&str; 11] = [
    "Width", "Offset", "Depth", "Speed", "Mix", "Blur", "Hz/Oct", "1x/5x",
    "Keep Bass", "Blocksize", "Bypass",
];

const DEFAULTS: [f32; 11] = [50.0, 0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 0.0, 0.0, 1024.0, 0.0];

pub const BLOCKSIZE_OPTIONS: [f32; 6] = [512.0, 1024.0, 2048.0, 4096.0, 8192.0, 16384.0];

type StateMap = Mutex<HashMap<(usize, usize), [f32; 11]>>;
fn state_map() -> &'static StateMap {
    static CELL: OnceLock<StateMap> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(HashMap::new()))
}

fn param_min(idx: usize) -> f32 {
    match idx {
        0 | 2 | 4 | 5 => 0.0,
        1 | 3 | 6 => -100.0,
        7 | 8 | 10 => 0.0,
        9 => 512.0,
        _ => 0.0,
    }
}

fn param_max(idx: usize) -> f32 {
    match idx {
        0 | 2 | 4 | 5 => 100.0,
        1 | 3 | 6 => 100.0,
        7 | 8 | 10 => 1.0,
        9 => 16384.0,
        _ => 100.0,
    }
}

fn clamp_param(idx: usize, value: f32) -> f32 {
    value.clamp(param_min(idx), param_max(idx))
}

pub fn get_params(track_idx: usize, slot_idx: usize) -> [f32; 11] {
    let mut map = state_map().lock().unwrap();
    *map.entry((track_idx, slot_idx)).or_insert(DEFAULTS)
}

pub fn set_param(track_idx: usize, slot_idx: usize, idx: usize, value: f32) {
    let mut map = state_map().lock().unwrap();
    let entry = map.entry((track_idx, slot_idx)).or_insert(DEFAULTS);
    if idx < 11 {
        entry[idx] = clamp_param(idx, value);
    }
}

pub fn render_module(ui: &mut egui::Ui, _app: &mut AppState, track_idx: usize, slot_idx: usize) {
    let mut params = get_params(track_idx, slot_idx);
    let mut changed = false;

    ui.horizontal(|ui| {
        // Left Column: Scope
        ui.vertical(|ui| {
            ui.set_width(180.0);
            ui.label(egui::RichText::new("Spectral Scope").size(11.0).color(theme::TEXT_MUTED));
            let (rect, _) = ui.allocate_exact_size(egui::vec2(170.0, 100.0), egui::Sense::hover());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 4.0, theme::DAW_BG_LAYER_1);

            let bar_count = 32;
            let bar_w = (rect.width() - 4.0) / bar_count as f32;
            let depth = params[2] / 100.0;
            let width_f = params[0] / 100.0;

            for i in 0..bar_count {
                let frac = ((i as f32 * 0.4 + width_f * 3.0).sin().abs() * 0.7 * depth + 0.1).clamp(0.05, 1.0);
                let bh = frac * (rect.height() - 8.0);
                let bx = rect.min.x + 2.0 + i as f32 * bar_w;
                let by = rect.max.y - 4.0 - bh;
                painter.rect_filled(
                    egui::Rect::from_min_size(egui::pos2(bx, by), egui::vec2(bar_w - 1.0, bh)),
                    1.0,
                    theme::ACCENT_PRIMARY,
                );
            }
        });

        ui.separator();

        // Right Column: Controls
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("OpenSpectral Phaser").strong().color(theme::ACCENT_PRIMARY));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let bypass = params[10] >= 0.5;
                    if ui.selectable_label(bypass, "BYPASS").clicked() {
                        params[10] = if bypass { 0.0 } else { 1.0 };
                        changed = true;
                    }
                });
            });

            ui.add_space(4.0);

            // GPU Rotary Knobs Grid (6 primary parameters)
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                for (i, name) in PARAM_NAMES.iter().take(6).enumerate() {
                    ui.vertical_centered(|ui| {
                        ui.set_width(38.0);
                        let min_v = param_min(i);
                        let max_v = param_max(i);
                        let bipolar = min_v < 0.0;
                        let mut val = params[i];
                        let (k_resp, k_changed) = crate::ui::bitwig_kit::rotary_knob(
                            ui,
                            28.0,
                            &mut val,
                            min_v..=max_v,
                            DEFAULTS[i],
                            if bipolar {
                                theme::BW_LCD_CYAN
                            } else {
                                theme::ACCENT_ORANGE
                            },
                            bipolar,
                        );
                        k_resp.on_hover_text(format!("{name}: {val:.1}"));
                        if k_changed {
                            params[i] = val;
                            changed = true;
                        }
                        ui.label(
                            egui::RichText::new(*name)
                                .font(crate::ui::fonts::semibold(8.5))
                                .color(theme::TEXT_SECONDARY),
                        );
                    });
                }
            });
        });
    });

    if changed {
        for (i, &v) in params.iter().enumerate() {
            set_param(track_idx, slot_idx, i, v);
        }
    }
}

pub fn render_module_footer(_ui: &mut egui::Ui, _app: &mut AppState, _track_idx: usize, _slot_idx: usize) {}

pub fn render_module_simple(ui: &mut egui::Ui) {
    ui.label("OpenSpectral Phaser");
}
