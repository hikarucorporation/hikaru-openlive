// Copyright (C) Hikaru Corporation - 2026
// Hikaru OpenLive - Audio Clip Editor
// GNU Affero General Public License v3
// crates/hikaru_gui/src/views/clip_editor.rs

use egui::{Align2, Color32, ComboBox, FontId, Frame, Pos2, Rect, Sense, Stroke, Ui};
use std::path::PathBuf;

// use crate::views::matrix::{ClipData, MatrixClip, MatrixSlot, SessionMatrixState}; //

// use crate::audio::AudioProxy; // <-- Asegurate de importar AudioProxy
// ✅ Usá la crate externa:
use crate::audio_proxy::AudioProxy;
use crate::views::matrix::{self, MatrixClip, SessionMatrixState};

// use crate::views::matrix::{self, ClipData, MatrixClip, MatrixSlot, SessionMatrixState};

use crate::views::waveform;
use hikaru_audio_engine::AudioEngine; // <-- Importamos AudioEngine para notificar cambios de DSP

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SnapValue {
    None,
    Bar1,
    Beat1_2,
    Beat1_4,
    Beat1_8,
    Beat1_16,
}

impl SnapValue {
    pub fn label(&self) -> &'static str {
        match self {
            SnapValue::None => "Off (Free)",
            SnapValue::Bar1 => "1 Bar",
            SnapValue::Beat1_2 => "1/2 Beat",
            SnapValue::Beat1_4 => "1/4 Beat",
            SnapValue::Beat1_8 => "1/8 Beat",
            SnapValue::Beat1_16 => "1/16 Beat",
        }
    }

    pub fn interval_secs(&self, bpm: f32) -> f64 {
        let current_bpm = if bpm > 0.0 { bpm as f64 } else { 120.0 };
        let sec_per_beat = 60.0 / current_bpm;

        match self {
            SnapValue::None => 0.0,
            SnapValue::Bar1 => sec_per_beat * 4.0,
            SnapValue::Beat1_2 => sec_per_beat * 2.0,
            SnapValue::Beat1_4 => sec_per_beat,
            SnapValue::Beat1_8 => sec_per_beat / 2.0,
            SnapValue::Beat1_16 => sec_per_beat / 4.0,
        }
    }
}

pub fn show(
    ui: &mut Ui,
    clip: &mut MatrixClip,
    track_idx: usize,
    scene_idx: usize,
    audio_engine: Option<&mut AudioEngine>,
    elapsed_frames: Option<u64>,
    sample_rate: u32,
    bpm: f32,
) {
    let mut loop_changed = false;

    // 1. Decodificación PCM limpia
    if clip.pcm_data().is_empty() && clip.path.exists() {
        if let Ok(reader) = hound::WavReader::open(&clip.path) {
            let spec = reader.spec();
            let channels = spec.channels as usize;
            let bits = spec.bits_per_sample;

            let samples: Vec<f32> = match spec.sample_format {
                hound::SampleFormat::Float => {
                    reader.into_samples::<f32>().filter_map(Result::ok).collect()
                }
                hound::SampleFormat::Int => {
                    let max_val = if bits <= 16 {
                        i16::MAX as f32
                    } else {
                        (1 << (bits - 1)) as f32
                    };
                    reader
                        .into_samples::<i32>()
                        .filter_map(Result::ok)
                        .map(|s| s as f32 / max_val)
                        .collect()
                }
            };
            
            if !samples.is_empty() {
                let total_frames = samples.len() / channels.max(1);
                clip.duration_secs = total_frames as f64 / spec.sample_rate as f64;
                
                let processed_pcm = if channels > 1 {
                    samples
                        .chunks(channels)
                        .map(|chunk| chunk.iter().sum::<f32>() / chunk.len() as f32)
                        .collect()
                } else {
                    samples
                };

                if let Some(pcm) = clip.pcm_data_mut() {
                    *pcm = processed_pcm;
                }
            }
        }
    }

    let snap_id = ui.make_persistent_id("clip_editor_snap_grid");
    let mut current_snap: SnapValue = ui.data_mut(|d| d.get_temp(snap_id).unwrap_or(SnapValue::Beat1_4));

    Frame::none()
        .fill(Color32::from_rgb(18, 18, 22))
        .stroke(Stroke::new(1.0_f32, Color32::from_gray(45)))
        .inner_margin(6.0)
        .show(ui, |ui| {
            // Header
            ui.horizontal(|ui| {
                ui.label(format!("🎵 {}", clip.name));
                ui.weak(format!("({:.2}s)", clip.duration_secs));
                
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Sincronización del toggle de Loop con el AudioEngine
                    if ui.toggle_value(&mut clip.loop_enabled, "🔁 Loop").changed() {
                        loop_changed = true;
                    }

                    ui.add_space(8.0);

                    ComboBox::from_id_source("snap_combo_box")
                        .selected_text(format!("🧲 Snap: {}", current_snap.label()))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut current_snap, SnapValue::None, SnapValue::None.label());
                            ui.selectable_value(&mut current_snap, SnapValue::Bar1, SnapValue::Bar1.label());
                            ui.selectable_value(&mut current_snap, SnapValue::Beat1_2, SnapValue::Beat1_2.label());
                            ui.selectable_value(&mut current_snap, SnapValue::Beat1_4, SnapValue::Beat1_4.label());
                            ui.selectable_value(&mut current_snap, SnapValue::Beat1_8, SnapValue::Beat1_8.label());
                            ui.selectable_value(&mut current_snap, SnapValue::Beat1_16, SnapValue::Beat1_16.label());
                        });
                    
                    ui.data_mut(|d| d.insert_temp(snap_id, current_snap));
                });
            });

            ui.separator();

            let available_size = ui.available_size();
            let (rect, _response) = ui.allocate_exact_size(available_size, Sense::click_and_drag());

            if ui.is_rect_visible(rect) {
                let total_frames = (clip.duration_secs * sample_rate as f64) as u64;

                // Sanitización estricta de límites del loop anti-crash
                if total_frames > 0 {
                    if clip.loop_end == 0 || clip.loop_end > total_frames {
                        clip.loop_end = total_frames;
                    }
                    if clip.loop_start >= clip.loop_end {
                        clip.loop_start = clip.loop_end.saturating_sub(sample_rate as u64 / 4); // Mínimo 250ms
                    }
                }

                // LAYER 1: Canvas Background
                ui.painter().rect_filled(rect, 4.0_f32, Color32::from_rgb(12, 12, 15));

                // LAYER 2: Grid y Ruler
                let current_bpm = if bpm > 0.0 { bpm as f64 } else { 120.0 };
                let sec_per_beat = 60.0 / current_bpm;
                let sec_per_bar = sec_per_beat * 4.0;
                let total_bars = if clip.duration_secs > 0.0 {
                    (clip.duration_secs / sec_per_bar).ceil() as usize
                } else {
                    0
                };

                let ruler_height = 18.0_f32;
                let ruler_rect = Rect::from_min_max(
                    rect.min,
                    Pos2::new(rect.max.x, rect.min.y + ruler_height),
                );
                let wave_rect = Rect::from_min_max(
                    Pos2::new(rect.min.x, rect.min.y + ruler_height),
                    rect.max,
                );

                ui.painter().rect_filled(ruler_rect, 0.0_f32, Color32::from_rgb(22, 22, 28));
                ui.painter().line_segment(
                    [Pos2::new(rect.min.x, ruler_rect.max.y), Pos2::new(rect.max.x, ruler_rect.max.y)],
                    Stroke::new(1.0_f32, Color32::from_gray(50)),
                );

                if clip.duration_secs > 0.0 {
                    for bar in 0..=total_bars {
                        let bar_time = bar as f64 * sec_per_bar;
                        if bar_time > clip.duration_secs { break; }

                        let norm_x = (bar_time / clip.duration_secs) as f32;
                        let x_pos = rect.min.x + (norm_x * rect.width());

                        ui.painter().line_segment(
                            [Pos2::new(x_pos, rect.min.y), Pos2::new(x_pos, rect.max.y)],
                            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 40)),
                        );

                        ui.painter().text(
                            Pos2::new(x_pos + 4.0, rect.min.y + 2.0),
                            Align2::LEFT_TOP,
                            format!("{}", bar + 1),
                            FontId::proportional(10.0),
                            Color32::from_gray(180),
                        );

                        for beat in 1..4 {
                            let beat_time = bar_time + (beat as f64 * sec_per_beat);
                            if beat_time >= clip.duration_secs { break; }

                            let beat_norm_x = (beat_time / clip.duration_secs) as f32;
                            let beat_x_pos = rect.min.x + (beat_norm_x * rect.width());

                            ui.painter().line_segment(
                                [Pos2::new(beat_x_pos, wave_rect.min.y), Pos2::new(beat_x_pos, wave_rect.max.y)],
                                Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 12)),
                            );
                        }
                    }
                }

                // LAYER 3: Active Selection Region Background
                let (sel_x_min, sel_x_max) = if total_frames > 0 && clip.loop_end > clip.loop_start {
                    let start_norm = (clip.loop_start as f32 / total_frames as f32).clamp(0.0, 1.0);
                    let end_norm = (clip.loop_end as f32 / total_frames as f32).clamp(0.0, 1.0);

                    let min_x = rect.min.x + (start_norm * rect.width());
                    let max_x = rect.min.x + (end_norm * rect.width());

                    let sel_rect = Rect::from_min_max(
                        Pos2::new(min_x, wave_rect.min.y),
                        Pos2::new(max_x, wave_rect.max.y),
                    );

                    ui.painter().rect_filled(sel_rect, 0.0_f32, Color32::from_rgba_unmultiplied(0, 120, 255, 45));
                    (min_x, max_x)
                } else {
                    (rect.min.x, rect.max.x)
                };

                // LAYER 4: Waveform
                if !clip.pcm_data().is_empty() {
                    waveform::draw_waveform(ui, wave_rect, clip.pcm_data(), Color32::from_rgb(0, 200, 255));
                }

                // LAYER 5: Independent Interactive Bracket Handles
                if total_frames > 0 {
                    let handle_width = 14.0_f32;
                    let handle_height = wave_rect.height();

                    let min_frame_gap = (sample_rate as u64 / 10).max(1024);
                    let snap_step = current_snap.interval_secs(bpm);

                    let calc_snapped_frame = |x_pos: f32| -> u64 {
                        if rect.width() <= 0.0 || clip.duration_secs <= 0.0 {
                            return 0;
                        }
                        let norm_x = ((x_pos - rect.min.x) / rect.width()).clamp(0.0, 1.0) as f64;
                        let mut time_secs = norm_x * clip.duration_secs;

                        if snap_step > 0.0 {
                            time_secs = (time_secs / snap_step).round() * snap_step;
                        }

                        let norm_snapped = (time_secs / clip.duration_secs).clamp(0.0, 1.0);
                        (norm_snapped * total_frames as f64) as u64
                    };

                    // 1. Bracket Izquierdo (`[`)
                    let start_rect = Rect::from_center_size(
                        Pos2::new(sel_x_min, wave_rect.center().y),
                        egui::vec2(handle_width, handle_height),
                    );
                    let start_response = ui.put(
                        start_rect,
                        egui::Button::new("")
                            .fill(Color32::from_rgb(0, 255, 200))
                            .sense(Sense::drag()),
                    );

                    ui.painter().line_segment(
                        [Pos2::new(sel_x_min, rect.min.y), Pos2::new(sel_x_min, rect.max.y)],
                        Stroke::new(2.0_f32, Color32::from_rgb(0, 255, 200)),
                    );
                    ui.painter().text(
                        start_rect.center(),
                        Align2::CENTER_CENTER,
                        "[",
                        FontId::proportional(14.0),
                        Color32::BLACK,
                    );

                    if start_response.dragged() {
                        if let Some(pointer_pos) = ui.input(|i| i.pointer.interact_pos()) {
                            let new_start = calc_snapped_frame(pointer_pos.x);
                            let target = if new_start + min_frame_gap <= clip.loop_end {
                                new_start
                            } else {
                                clip.loop_end.saturating_sub(min_frame_gap)
                            };
                            if clip.loop_start != target {
                                clip.loop_start = target;
                                loop_changed = true;
                            }
                        }
                    }

                    // 2. Bracket Derecho (`]`)
                    let end_rect = Rect::from_center_size(
                        Pos2::new(sel_x_max, wave_rect.center().y),
                        egui::vec2(handle_width, handle_height),
                    );
                    let end_response = ui.put(
                        end_rect,
                        egui::Button::new("")
                            .fill(Color32::from_rgb(255, 80, 80))
                            .sense(Sense::drag()),
                    );

                    ui.painter().line_segment(
                        [Pos2::new(sel_x_max, rect.min.y), Pos2::new(sel_x_max, rect.max.y)],
                        Stroke::new(2.0_f32, Color32::from_rgb(255, 80, 80)),
                    );
                    ui.painter().text(
                        end_rect.center(),
                        Align2::CENTER_CENTER,
                        "]",
                        FontId::proportional(14.0),
                        Color32::BLACK,
                    );

                    if end_response.dragged() {
                        if let Some(pointer_pos) = ui.input(|i| i.pointer.interact_pos()) {
                            let new_end = calc_snapped_frame(pointer_pos.x);
                            let target = if new_end >= clip.loop_start + min_frame_gap {
                                new_end
                            } else {
                                (clip.loop_start + min_frame_gap).min(total_frames)
                            };
                            if clip.loop_end != target {
                                clip.loop_end = target;
                                loop_changed = true;
                            }
                        }
                    }
                }

                // LAYER 6: Playhead con Anti-Zero Division Guard
                if let Some(frames) = elapsed_frames {
                    if total_frames > 0 {
                        let active_start = clip.loop_start.min(total_frames);
                        let active_end = if clip.loop_end > active_start {
                            clip.loop_end.min(total_frames)
                        } else {
                            total_frames
                        };
                        
                        let loop_length = active_end.saturating_sub(active_start).max(1);

                        let current_frame = if clip.loop_enabled {
                            active_start + (frames % loop_length)
                        } else {
                            frames % total_frames
                        };

                        let progress = (current_frame as f32 / total_frames as f32).clamp(0.0, 1.0);
                        let playhead_x = rect.min.x + (progress * rect.width());

                        ui.painter().line_segment(
                            [Pos2::new(playhead_x, rect.min.y), Pos2::new(playhead_x, rect.max.y)],
                            Stroke::new(2.0_f32, Color32::WHITE),
                        );
                    }
                }

                ui.painter().rect_stroke(rect, 4.0_f32, Stroke::new(1.0_f32, Color32::from_gray(50)));
            }
        });

    // Sincronizar automáticamente con AudioEngine si hubo cambios
    if loop_changed {
        if let Some(engine) = audio_engine {
            let start_secs = clip.loop_start as f32 / sample_rate as f32;
            let end_secs = clip.loop_end as f32 / sample_rate as f32;
            engine.set_clip_loop(
                track_idx,
                scene_idx,
                start_secs,
                end_secs,
                clip.loop_enabled,
            );
        }
    }
}

pub fn render_clip_editor_track_view(
    ui: &mut Ui,
    state: &mut SessionMatrixState,
    audio_proxy: &AudioProxy, // <-- Pasamos el AudioProxy que usa tu sistema
    dragged_sample: &mut Option<PathBuf>,
    bpm: f64,
    sample_rate: u32,
    _transport_sample_count: u64,
    _ppqn: u64,
    _global_loop_enabled: bool,
    _global_loop_start_ticks: u64,
    _global_loop_end_ticks: u64,
) {
    if let Some((track_idx, scene_idx)) = state.selected_slot {
        if let Some(path) = dragged_sample.take() {
            // Llamada correcta con los 6 argumentos requeridos
            matrix::load_clip_into_slot(state, audio_proxy, track_idx, scene_idx, path, bpm);
            return;
        }

        let mut clip_to_show = None;

        if track_idx < state.grid.len() && scene_idx < state.grid[track_idx].len() {
            clip_to_show = state.grid[track_idx][scene_idx].clip.as_mut();
        }

        if let Some(clip) = clip_to_show {
            // Si necesitás enviar comandos al audio_proxy cuando cambie el loop:
            let old_loop = clip.loop_enabled;
            let old_start = clip.loop_start;
            let old_end = clip.loop_end;

            show(
                ui,
                clip,
                track_idx,
                scene_idx,
                None,
                None,
                sample_rate,
                bpm as f32,
            );

            // Si el estado del loop o límites cambiaron en la UI, notificamos al AudioProxy
            if clip.loop_enabled != old_loop || clip.loop_start != old_start || clip.loop_end != old_end {
                let start_secs = clip.loop_start as f32 / sample_rate as f32;
                let end_secs = clip.loop_end as f32 / sample_rate as f32;
                
                // Notificar al motor de audio vía proxy
                audio_proxy.set_clip_loop(
                    track_idx,
                    scene_idx,
                    start_secs,
                    end_secs,
                    clip.loop_enabled,
                );
            }
        } else {
            ui.centered_and_justified(|ui| {
                ui.heading("El slot seleccionado está vacío. Arrastrá un archivo de audio aquí.");
            });
        }
    } else {
        ui.centered_and_justified(|ui| {
            ui.label("Seleccioná un clip en la Session Matrix para editarlo.");
        });
    }
}