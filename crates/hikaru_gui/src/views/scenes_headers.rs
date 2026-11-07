// Hikaru OpenLive - Scenes Headers (egui)
// GNU AGPLv3
// crates/hikaru_gui/src/views/scenes_headers.rs

use egui::{pos2, vec2, Align2, Rect, Sense, Ui};

use crate::app::AppState;
use crate::ui::fonts;
use crate::ui::icons;

pub fn render(ui: &mut Ui, state: &mut AppState) {
    let scenes_count = state.matrix_state.scenes.len();

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for i in 0..scenes_count {
            let (rect, resp) = ui.allocate_exact_size(vec2(92.0, 24.0), Sense::click());
            ui.painter().rect_filled(
                rect,
                1.5,
                if resp.hovered() {
                    egui::Color32::from_rgb(0x5C, 0x5E, 0x64)
                } else {
                    egui::Color32::from_rgb(0x4A, 0x4C, 0x52)
                },
            );
            icons::paint_play_triangle(
                ui.painter(),
                Rect::from_min_size(pos2(rect.min.x + 4.0, rect.min.y + 6.0), vec2(12.0, 12.0)),
                egui::Color32::from_rgb(0x12, 0x13, 0x16),
            );
            ui.painter().text(
                pos2(rect.min.x + 19.0, rect.center().y),
                Align2::LEFT_CENTER,
                format!("Scene {}", i + 1),
                fonts::semibold(10.5),
                egui::Color32::from_rgb(0x12, 0x13, 0x16),
            );
            if resp.clicked() {
                crate::views::matrix::trigger_scene(
                    &mut state.matrix_state,
                    &state.audio_proxy,
                    i,
                );
            }
        }
    });
}
