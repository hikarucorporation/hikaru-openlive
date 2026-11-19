// Hikaru OpenLive - Scenes Headers (egui)
// GNU AGPLv3
// crates/hikaru_gui/src/views/scenes_headers.rs

use egui::{pos2, vec2, Align2, Rect, Sense, Ui};

use crate::app::AppState;
use crate::ui::fonts;
use crate::ui::icons;

pub fn render(ui: &mut Ui, state: &mut AppState) {
    let scenes_count = state.matrix_state.scenes.len();
    let mut open_scene_menu: Option<(usize, egui::Pos2)> = None;
    let mut add_scene_clicked = false;
    let mut remove_last_clicked = false;

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for i in 0..scenes_count {
            let (rect, resp) = ui.allocate_exact_size(vec2(92.0, 24.0), Sense::click());
            let (name, q_override) = state
                .matrix_state
                .scenes
                .get(i)
                .map(|s| (s.name.clone(), s.launch_quantization))
                .unwrap_or_else(|| (format!("Scene {}", i + 1), None));
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
                &name,
                fonts::semibold(10.5),
                egui::Color32::from_rgb(0x12, 0x13, 0x16),
            );
            if let Some(ov) = q_override {
                let badge = Rect::from_min_max(
                    pos2(rect.max.x - 26.0, rect.min.y + 4.0),
                    pos2(rect.max.x - 3.0, rect.max.y - 4.0),
                );
                ui.painter()
                    .rect_filled(badge, 2.0, egui::Color32::from_rgb(0x23, 0x26, 0x2C));
                ui.painter().text(
                    badge.center(),
                    Align2::CENTER_CENTER,
                    ov.short_label(),
                    fonts::semibold(8.5),
                    crate::theme::ACCENT_ORANGE,
                );
            }
            if resp.clicked() {
                crate::views::matrix::trigger_scene(
                    &mut state.matrix_state,
                    &state.audio_proxy,
                    i,
                );
            } else if resp.secondary_clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    open_scene_menu = Some((i, pos));
                }
            }
        }
        if ui.small_button("+").on_hover_text("Add Scene").clicked() {
            add_scene_clicked = true;
        }
        if scenes_count > 1 && ui.small_button("-").on_hover_text("Remove Last Scene").clicked() {
            remove_last_clicked = true;
        }
    });

    if add_scene_clicked {
        state.matrix_state.add_scene_synced(&state.audio_proxy);
    }
    if remove_last_clicked && scenes_count > 1 {
        state.remove_scene_at(scenes_count - 1);
    }
    if let Some((scene_idx, click_pos)) = open_scene_menu {
        state.matrix_state.context_menu = None;
        state.matrix_state.scene_context_menu = Some(crate::views::matrix::SceneContextMenu {
            scene_idx,
            click_pos,
        });
    }
    crate::views::matrix::render_scene_context_menu(ui, state);
}
