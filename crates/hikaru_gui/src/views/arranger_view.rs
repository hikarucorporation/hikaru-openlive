// Copyright (C) Hikaru Corporation - 2026
// GNU Affero General Public License v3
// Bitwig-style Session / Arranger Matrix Launcher (OpenLive Dynamic Grid)
// crates/hikaru_gui/src/views/arranger_view.rs

use egui::*;
use crate::views::mixer;
use hikaru_transport::TransportPosition;
use crate::audio_proxy::{AudioProxy, GuiCommand};
use crate::views::matrix::{self, SessionMatrixState, SlotState, MatrixClipboard};

pub fn show(
    ui: &mut Ui,
    tracks: &mut [mixer::Track],
    matrix_state: &mut SessionMatrixState,
    clipboard: &mut MatrixClipboard,
    _transport: &TransportPosition,
    audio_proxy: &AudioProxy,
    dragged_sample: &mut Option<std::path::PathBuf>,
    bpm: f64,
) {
    if tracks.is_empty() {
        return;
    }

    let track_width = 110.0;
    let scene_label_width = 100.0;
    let clip_slot_height = 28.0;
    let scenes_count = matrix_state.scenes.len();

    let (master_track, audio_tracks) = tracks.split_first_mut().unwrap();

    ScrollArea::both()
        .id_source("arranger_view_scroll")
        .show(ui, |ui| {
            // -------------------------------------------------------------
            // 1. ENCABEZADOS DE PISTAS (LAYOUT HORIZONTAL SUPERIOR)
            // -------------------------------------------------------------
            ui.horizontal(|ui| {
                ui.allocate_ui(vec2(scene_label_width, 40.0), |ui| {
                    Frame::group(ui.style()).show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new("SCENES").strong().color(Color32::LIGHT_BLUE));
                            ui.label(RichText::new("Master Launch").size(9.0));
                        });
                    });
                });

                ui.separator();

                for (i, _track) in audio_tracks.iter().enumerate() {
                    let track_name = matrix_state
                        .tracks
                        .get(i)
                        .map(|t| t.name.as_str())
                        .unwrap_or("Audio");

                    ui.allocate_ui(vec2(track_width, 40.0), |ui| {
                        Frame::group(ui.style()).show(ui, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.label(RichText::new(format!("TRK {:02}", i + 1)).strong());
                                ui.label(RichText::new(track_name).size(9.0));
                            });
                        });
                    });
                    ui.add_space(2.0);
                }
            });

            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);

            // -------------------------------------------------------------
            // 2. GRILLA DE ESCENAS (FILAS) Y PADS DE PISTAS (COLUMNAS)
            // -------------------------------------------------------------
            for scene_idx in 0..scenes_count {
                ui.horizontal(|ui| {
                    let (scene_rect, scene_response) = ui.allocate_exact_size(
                        Vec2::new(scene_label_width, clip_slot_height),
                        Sense::click(),
                    );

                    if scene_response.clicked() {
                        matrix::trigger_scene(matrix_state, audio_proxy, scene_idx);
                    }

                    if ui.is_rect_visible(scene_rect) {
                        let painter = ui.painter();
                        let fill_color = if scene_response.hovered() {
                            Color32::from_rgb(50, 50, 60)
                        } else {
                            Color32::from_rgb(30, 30, 40)
                        };

                        painter.rect_filled(scene_rect, 2.0, fill_color);
                        painter.rect_stroke(
                            scene_rect,
                            2.0,
                            Stroke::new(1.0_f32, Color32::from_rgb(0, 150, 190)),
                        );

                        let scene_name = matrix_state.scenes.get(scene_idx)
                            .map(|s| s.name.as_str())
                            .unwrap_or("Scene");

                        painter.text(
                            scene_rect.center(),
                            Align2::CENTER_CENTER,
                            format!("▶ {}", scene_name),
                            FontId::proportional(10.0),
                            Color32::WHITE,
                        );
                    }

                    ui.separator();

                    for (track_idx, _track) in audio_tracks.iter().enumerate() {
                        let (slot_rect, response) = ui.allocate_exact_size(
                            Vec2::new(track_width, clip_slot_height),
                            Sense::click(),
                        );

                        let is_selected = matrix_state.selected_slot == Some((track_idx, scene_idx));
                        let has_clip = matrix_state
                            .grid
                            .get(track_idx)
                            .and_then(|row| row.get(scene_idx))
                            .map_or(false, |slot| slot.clip.is_some());

                        // --- MENÚ CONTEXTUAL PORTAPAPELES ---
                        response.context_menu(|ui| {
                            ui.style_mut().spacing.button_padding = Vec2::new(8.0, 4.0);

                            if ui.add_enabled(has_clip, Button::new("📋 Copiar")).clicked() {
                                matrix_state.copy_slot(track_idx, scene_idx, clipboard);
                                ui.close_menu();
                            }
                            if ui.add_enabled(has_clip, Button::new("✂ Cortar")).clicked() {
                                matrix_state.cut_slot(track_idx, scene_idx, clipboard, audio_proxy);
                                ui.close_menu();
                            }
                            if ui.add_enabled(clipboard.has_content(), Button::new("📥 Pegar")).clicked() {
                                matrix_state.paste_slot(track_idx, scene_idx, clipboard, audio_proxy);
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.add_enabled(has_clip, Button::new("📑 Duplicar")).clicked() {
                                matrix_state.duplicate_slot(track_idx, scene_idx, audio_proxy);
                                ui.close_menu();
                            }
                            if ui.add_enabled(has_clip, Button::new("🗑 Eliminar")).clicked() {
                                matrix_state.delete_slot(track_idx, scene_idx, audio_proxy);
                                ui.close_menu();
                            }
                        });

                        if response.clicked() && track_idx < matrix_state.grid.len() {
                            matrix_state.selected_slot = Some((track_idx, scene_idx));
                            matrix::trigger_pad(matrix_state, audio_proxy, track_idx, scene_idx);
                        }

                        // Arrastrar y soltar samples en los slots del Arranger
                        if ui.rect_contains_pointer(slot_rect) {
                            ui.output_mut(|o| o.cursor_icon = CursorIcon::PointingHand);

                            if ui.input(|i| i.pointer.any_released()) {
                                if let Some(sample_path) = dragged_sample.take() {
                                    if crate::views::explorer::is_audio_file(&sample_path) {
                                        matrix_state.selected_slot = Some((track_idx, scene_idx));
                                        load_clip_into_arranger_slot(
                                            matrix_state,
                                            audio_proxy,
                                            track_idx,
                                            scene_idx,
                                            sample_path,
                                            bpm,
                                        );
                                    }
                                }
                            }

                            let dropped_files = ui.input(|i| i.raw.dropped_files.clone());
                            if let Some(file) = dropped_files.first() {
                                if let Some(path) = &file.path {
                                    if crate::views::explorer::is_audio_file(path) {
                                        matrix_state.selected_slot = Some((track_idx, scene_idx));
                                        load_clip_into_arranger_slot(
                                            matrix_state,
                                            audio_proxy,
                                            track_idx,
                                            scene_idx,
                                            path.clone(),
                                            bpm,
                                        );
                                    }
                                }
                            }
                        }

                        if ui.is_rect_visible(slot_rect) {
                            let painter = ui.painter();
                            let slot_info = matrix_state.grid.get(track_idx).and_then(|row| row.get(scene_idx));

                            let (fill_color, mut border_color, text_label) = match slot_info {
                                Some(slot) => match &slot.state {
                                    SlotState::Playing => (
                                        Color32::from_rgb(35, 135, 60),
                                        Color32::GREEN,
                                        slot.clip.as_ref().map(|c| format!("▶ {}", c.name)).unwrap_or_else(|| "▶ Clip".into()),
                                    ),
                                    SlotState::QueuedToPlay => (
                                        Color32::from_rgb(120, 100, 30),
                                        Color32::YELLOW,
                                        slot.clip.as_ref().map(|c| format!("⌛ {}", c.name)).unwrap_or_else(|| "⌛ Clip".into()),
                                    ),
                                    SlotState::Stopped => (
                                        Color32::from_rgb(45, 55, 75),
                                        Color32::from_rgb(90, 130, 190),
                                        slot.clip.as_ref().map(|c| c.name.clone()).unwrap_or_else(|| "[ Empty ]".into()),
                                    ),
                                    SlotState::QueuedToStop => (
                                        Color32::from_rgb(130, 45, 45),
                                        Color32::RED,
                                        "⏹ Stop".into(),
                                    ),
                                    SlotState::Empty => (
                                        if response.hovered() { Color32::from_rgb(35, 35, 35) } else { Color32::from_rgb(25, 25, 25) },
                                        Color32::from_gray(40),
                                        "".into(),
                                    ),
                                },
                                None => (Color32::from_rgb(25, 25, 25), Color32::from_gray(40), "".into()),
                            };

                            if is_selected {
                                border_color = Color32::WHITE;
                            }

                            painter.rect_filled(slot_rect, 2.0, fill_color);
                            painter.rect_stroke(
                                slot_rect,
                                2.0,
                                Stroke::new(if is_selected { 2.0_f32 } else { 1.0_f32 }, border_color),
                            );

                            if !text_label.is_empty() {
                                let display_text = if text_label.len() > 14 {
                                    format!("{}...", &text_label[..11])
                                } else {
                                    text_label
                                };

                                painter.text(
                                    slot_rect.center(),
                                    Align2::CENTER_CENTER,
                                    display_text,
                                    FontId::proportional(9.0),
                                    Color32::WHITE,
                                );
                            }
                        }

                        ui.add_space(2.0);
                    }
                });

                ui.add_space(2.0);
            }

            ui.add_space(10.0);
            ui.separator();

            // -------------------------------------------------------------
            // 3. MEZCLADOR / FADERS AL PIE DE CADA COLUMNA
            // -------------------------------------------------------------
            ui.horizontal(|ui| {
                ui.allocate_ui(vec2(scene_label_width, 240.0), |ui| {
                    ui.vertical_centered(|ui| {
                        // ui.label(RichText::new("MASTER").small().strong().color(Color32::LIGHT_BLUE));
                        
                        let _old_pan = master_track.pan;
                        if custom_pan_slider(ui, &mut master_track.pan).changed() {
                            // Sincronización de Master Pan si tenés comando asignado
                        }

                        ui.add_space(4.0);

                        let _old_vol = master_track.volume;
                        if custom_volume_fader(ui, &mut master_track.volume).changed() {
                            // Sincronización de Master Volume si tenés comando asignado
                        }

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.toggle_value(&mut master_track.mute, "M");
                            ui.toggle_value(&mut master_track.solo, "S");
                        });
                    });
                });

                ui.separator();

                for (idx, track) in audio_tracks.iter_mut().enumerate() {
                    ui.allocate_ui(vec2(track_width, 240.0), |ui| {
                        ui.vertical_centered(|ui| {
                            let _old_pan = track.pan;
                            if custom_pan_slider(ui, &mut track.pan).dragged() {
                                if let Some(meta) = matrix_state.tracks.get_mut(idx) {
                                    meta.pan = track.pan;
                                }
                                audio_proxy.send(GuiCommand::SetTrackPan {
                                    track_idx: idx,
                                    pan: track.pan,
                                });
                            }

                            ui.add_space(4.0);

                            let _old_vol = track.volume;
                            if custom_volume_fader(ui, &mut track.volume).dragged() {
                                if let Some(meta) = matrix_state.tracks.get_mut(idx) {
                                    meta.volume = track.volume;
                                }
                                audio_proxy.send(GuiCommand::SetTrackVolume {
                                    track_idx: idx,
                                    volume_db: track.volume,
                                });
                            }

                            ui.add_space(4.0);

                            ui.horizontal(|ui| {
                                let mute_btn = ui.toggle_value(&mut track.mute, "M");
                                if mute_btn.clicked() {
                                    if let Some(meta) = matrix_state.tracks.get_mut(idx) {
                                        meta.muted = track.mute;
                                    }
                                    audio_proxy.send(GuiCommand::SetTrackMute {
                                        track_idx: idx,
                                        mute: track.mute,
                                    });
                                }

                                let solo_btn = ui.toggle_value(&mut track.solo, "S");
                                if solo_btn.clicked() {
                                    if let Some(meta) = matrix_state.tracks.get_mut(idx) {
                                        meta.soloed = track.solo;
                                    }
                                    audio_proxy.send(GuiCommand::SetTrackSolo {
                                        track_idx: idx,
                                        solo: track.solo,
                                    });
                                }
                            });
                        });
                    });
                    ui.add_space(2.0);
                }
            });
        });
}

fn load_clip_into_arranger_slot(
    state: &mut SessionMatrixState,
    audio_proxy: &AudioProxy,
    track_idx: usize,
    scene_idx: usize,
    path: std::path::PathBuf,
    bpm: f64,
) {
    let name = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let new_id = state.next_clip_id;
    state.next_clip_id += 1;

    let path_str = path.to_string_lossy().to_string();

    let mut local_state = crate::views::playlist::PlaylistState::default();
    local_state.zoom_x = state.editor_zoom_x;

    let sub_clip_id = local_state.next_clip_id;
    local_state.next_clip_id += 1;
    let initial_sub_clip = crate::views::playlist::build_audio_clip(
        sub_clip_id,
        name.clone(),
        &path,
        0,
        local_state.ppqn,
        bpm,
        Color32::from_rgb(32, 95, 145),
    );
    local_state.clips.push((0, initial_sub_clip));

    for (_, sub) in local_state.clips.iter_mut() {
        if let crate::views::playlist::ClipType::Audio { sample_offset_ticks, .. } = &mut sub.clip_type {
            *sample_offset_ticks = 0;
        }
    }

    state.grid[track_idx][scene_idx] = matrix::MatrixSlot {
        state: SlotState::Stopped,
        clip: Some(matrix::MatrixClip {
            id: new_id,
            name: name.clone(),
            path,
            duration_secs: 0.0,
            pcm_data: Vec::new(),
            local_state,
            local_track: mixer::Track::new(0, name, false),
            local_bar: 1.0,
            loop_start: 0,
            loop_end: 0,
            loop_enabled: false,
        }),
    };

    audio_proxy.send(GuiCommand::LoadClip {
        clip_id: new_id,
        path: path_str,
        position_secs: 0.0,
        duration_secs: 0.0,
        offset_secs: 0.0,
        track_index: track_idx,
        scene_index: scene_idx,
    });
}

// Helper: Custom Pan Slider
fn custom_pan_slider(ui: &mut Ui, pan_val: &mut f32) -> Response {
    let desired_size = vec2(90.0, 20.0);
    let (rect, response) = ui.allocate_at_least(desired_size, Sense::drag());

    let thumb_width = 12.0;
    let half_thumb = thumb_width / 2.0;

    let min_x = rect.min.x + half_thumb;
    let max_x = rect.max.x - half_thumb;

    if response.dragged() {
        if let Some(pointer_pos) = response.interact_pointer_pos() {
            let normalized = ((pointer_pos.x - min_x) / (max_x - min_x)).clamp(0.0, 1.0);
            *pan_val = (normalized * 2.0) - 1.0; // Normalizado de -1.0 a 1.0
        }
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let center_x = rect.center().x;
        let track_y = rect.center().y;

        let rail_rect = Rect::from_min_max(
            pos2(min_x - 2.0, track_y - 3.0),
            pos2(max_x + 2.0, track_y + 3.0),
        );
        painter.rect_filled(rail_rect, 1.0, Color32::from_rgb(45, 45, 45));

        let current_norm = (*pan_val + 1.0) / 2.0;
        let thumb_x = min_x + (current_norm * (max_x - min_x));

        if (thumb_x - center_x).abs() > 0.5 {
            let fill_min_x = center_x.min(thumb_x);
            let fill_max_x = center_x.max(thumb_x);
            let filled_rect = Rect::from_min_max(
                pos2(fill_min_x, track_y - 3.0),
                pos2(fill_max_x, track_y + 3.0),
            );
            painter.rect_filled(filled_rect, 0.0, Color32::from_rgb(0, 150, 190));
        }

        painter.line_segment(
            [pos2(center_x, track_y - 5.0), pos2(center_x, track_y + 5.0)],
            Stroke::new(1.0_f32, Color32::GRAY),
        );

        let thumb_rect = Rect::from_center_size(
            pos2(thumb_x, track_y),
            vec2(thumb_width, 18.0),
        );
        let thumb_color = if response.hovered() || response.dragged() {
            Color32::from_rgb(100, 200, 255)
        } else {
            Color32::from_rgb(0, 162, 232)
        };

        painter.rect_filled(thumb_rect, 0.0, thumb_color);
        painter.rect_stroke(thumb_rect, 0.0, Stroke::new(1.0_f32, Color32::BLACK));
    }

    response
}

// Helper: Custom Volume Vertical Fader
fn custom_volume_fader(ui: &mut Ui, volume_val: &mut f32) -> Response {
    let desired_size = vec2(30.0, 160.0);
    let (rect, response) = ui.allocate_at_least(desired_size, Sense::drag());

    let thumb_width = 45.0;
    let thumb_height = 18.0;
    let half_thumb = thumb_height / 2.0;

    let min_y = rect.min.y + half_thumb;
    let max_y = rect.max.y - half_thumb;

    if response.dragged() {
        if let Some(pointer_pos) = response.interact_pointer_pos() {
            let normalized = (1.0 - ((pointer_pos.y - min_y) / (max_y - min_y))).clamp(0.0, 1.0);
            *volume_val = normalized;
        }
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let track_x = rect.center().x;
        let rail_width = 12.0;

        let rail_rect = Rect::from_min_max(
            pos2(track_x - rail_width, min_y - 2.0),
            pos2(track_x + rail_width, max_y + 2.0),
        );
        painter.rect_filled(rail_rect, 1.0, Color32::from_rgb(45, 45, 45));

        let current_norm = volume_val.clamp(0.0, 1.0);
        let thumb_y = max_y - (current_norm * (max_y - min_y));

        if (max_y - thumb_y).abs() > 0.5 {
            let filled_rect = Rect::from_min_max(
                pos2(track_x - rail_width, thumb_y),
                pos2(track_x + rail_width, max_y + 2.0),
            );
            painter.rect_filled(filled_rect, 0.0, Color32::from_rgb(0, 150, 190));
        }

        let thumb_rect = Rect::from_center_size(
            pos2(track_x, thumb_y),
            vec2(thumb_width, thumb_height),
        );
        let thumb_color = if response.hovered() || response.dragged() {
            Color32::from_rgb(100, 200, 255)
        } else {
            Color32::from_rgb(0, 162, 232)
        };

        painter.rect_filled(thumb_rect, 0.0, thumb_color);
        painter.rect_stroke(thumb_rect, 0.0, Stroke::new(1.0_f32, Color32::BLACK));
    }

    response
}