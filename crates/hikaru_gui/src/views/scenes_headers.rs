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
    let mut select_scene: Option<usize> = None;
    let mut trigger_scene_idx: Option<usize> = None;
    let mut move_scene_action: Option<(usize, usize)> = None;
    let mut commit_rename: Option<(usize, String)> = None;
    let mut cancel_rename = false;

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for i in 0..scenes_count {
            let (rect, resp) = ui.allocate_exact_size(vec2(92.0, 24.0), Sense::click_and_drag());
            let (name, q_override, custom_color) = state
                .matrix_state
                .scenes
                .get(i)
                .map(|s| (s.name.clone(), s.launch_quantization, s.color))
                .unwrap_or_else(|| (format!("Scene {}", i + 1), None, None));
            let is_queued = state.queued_scene_index == Some(i);
            let is_selected = state.matrix_state.selected_scene == Some(i);
            let is_renaming = state
                .matrix_state
                .renaming_scene
                .as_ref()
                .is_some_and(|(idx, _)| *idx == i);
            let play_hit_rect = Rect::from_min_max(
                rect.min,
                pos2((rect.min.x + 18.0).min(rect.max.x), rect.max.y),
            );
            let play_hovered = resp
                .hover_pos()
                .is_some_and(|p| play_hit_rect.contains(p));
            let (bg, txt_col, play_col) = crate::views::matrix::scene_header_colors(
                custom_color,
                is_queued,
                resp.hovered(),
                is_selected,
                play_hovered,
            );
            ui.painter().rect_filled(rect, 1.5, bg);
            if is_selected {
                ui.painter().rect_stroke(
                    rect,
                    1.5,
                    egui::Stroke::new(1.5_f32, egui::Color32::WHITE),
                    egui::StrokeKind::Inside,
                );
            } else if is_queued {
                ui.painter().rect_stroke(
                    rect,
                    1.5,
                    egui::Stroke::new(1.2_f32, crate::theme::ACCENT_ORANGE),
                    egui::StrokeKind::Inside,
                );
            }
            icons::paint_play_triangle(
                ui.painter(),
                Rect::from_min_size(pos2(rect.min.x + 4.0, rect.min.y + 6.0), vec2(12.0, 12.0)),
                play_col,
            );
            if is_renaming {
                if let Some((_, ref mut buf)) = state.matrix_state.renaming_scene {
                    let edit_rect = Rect::from_min_max(
                        pos2(rect.min.x + 18.0, rect.min.y + 2.0),
                        pos2(rect.max.x - 2.0, rect.max.y - 2.0),
                    );
                    let edit_resp = ui.put(
                        edit_rect,
                        egui::TextEdit::singleline(buf)
                            .font(fonts::semibold(10.5))
                            .frame(true),
                    );
                    if !edit_resp.has_focus() {
                        edit_resp.request_focus();
                    }
                    if ui.input(|inp| inp.key_pressed(egui::Key::Escape)) {
                        cancel_rename = true;
                    } else if ui.input(|inp| inp.key_pressed(egui::Key::Enter))
                        || edit_resp.lost_focus()
                    {
                        commit_rename = Some((i, buf.clone()));
                    }
                }
            } else {
                ui.painter().text(
                    pos2(rect.min.x + 19.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    &name,
                    fonts::semibold(10.5),
                    txt_col,
                );
            }
            if !is_renaming {
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

                // Drag-and-drop reorder de escenas
                if resp.drag_started() {
                    state.matrix_state.scene_drag = Some(i);
                }
                if let Some(drag_from) = state.matrix_state.scene_drag {
                    if drag_from != i && resp.hovered() {
                        let edge_x = if drag_from < i { rect.max.x } else { rect.min.x };
                        ui.painter().line_segment(
                            [pos2(edge_x, rect.min.y), pos2(edge_x, rect.max.y)],
                            egui::Stroke::new(2.5_f32, crate::theme::ACCENT_ORANGE),
                        );
                        if ui.input(|inp| inp.pointer.any_released()) {
                            move_scene_action = Some((drag_from, i));
                        }
                    }
                }

                if resp.double_clicked() {
                    select_scene = Some(i);
                    trigger_scene_idx = Some(i);
                } else if resp.clicked() {
                    select_scene = Some(i);
                    if resp
                        .interact_pointer_pos()
                        .is_some_and(|p| play_hit_rect.contains(p))
                    {
                        trigger_scene_idx = Some(i);
                    }
                } else if resp.secondary_clicked() {
                    select_scene = Some(i);
                    if let Some(pos) = resp.interact_pointer_pos() {
                        open_scene_menu = Some((i, pos));
                    }
                }
            }
        }
        if ui.small_button("+").on_hover_text("Add Scene").clicked() {
            add_scene_clicked = true;
        }
    });

    if ui.input(|inp| inp.pointer.any_released()) {
        state.matrix_state.scene_drag = None;
    }
    if cancel_rename {
        state.matrix_state.renaming_scene = None;
    } else if let Some((idx, new_name)) = commit_rename {
        state.rename_scene(idx, new_name);
        state.matrix_state.renaming_scene = None;
    }
    if let Some((from_idx, to_idx)) = move_scene_action {
        state.move_scene(from_idx, to_idx);
    }

    if add_scene_clicked {
        let new_idx = state.matrix_state.add_scene_synced(&state.audio_proxy);
        state.matrix_state.selected_scene = Some(new_idx);
        state.matrix_state.selected_slot = None;
    }
    if let Some(idx) = select_scene {
        state.matrix_state.selected_scene = Some(idx);
        state.matrix_state.selected_slot = None;
        state.playlist_state.selected_clips.clear();
        state.selected_fx_index = None;
    }
    if let Some(idx) = trigger_scene_idx {
        crate::views::matrix::trigger_scene(&mut state.matrix_state, &state.audio_proxy, idx);
    }
    if let Some((scene_idx, click_pos)) = open_scene_menu {
        state.matrix_state.selected_scene = Some(scene_idx);
        state.matrix_state.context_menu = None;
        state.matrix_state.scene_context_menu = Some(crate::views::matrix::SceneContextMenu {
            scene_idx,
            click_pos,
        });
    }
    crate::views::matrix::render_scene_context_menu(ui, state);
}
