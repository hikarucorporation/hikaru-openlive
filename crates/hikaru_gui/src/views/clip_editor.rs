// crates/hikaru_gui/src/views/clip_editor.rs
use egui::{Align2, Color32, ComboBox, FontId, Frame, Pos2, Rect, Sense, Stroke, Ui};
use crate::views::matrix::MatrixClip;
use crate::views::waveform;

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

    /// Retorna la duración en segundos del intervalo de cuantización según el BPM actual
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
    elapsed_frames: Option<u64>,
    sample_rate: u32,
    bpm: f32, // Recibimos el BPM del proyecto
) {
    // 1. Decodificación PCM limpia
    if clip.pcm_data.is_empty() && clip.path.exists() {
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
                
                if channels > 1 {
                    clip.pcm_data = samples
                        .chunks(channels)
                        .map(|chunk| chunk.iter().sum::<f32>() / chunk.len() as f32)
                        .collect();
                } else {
                    clip.pcm_data = samples;
                }
            }
        }
    }

    // Usamos la memoria del ID de egui para retener el modo Snap seleccionado
    let snap_id = ui.make_persistent_id("clip_editor_snap_grid");
    let mut current_snap: SnapValue = ui.data_mut(|d| d.get_temp(snap_id).unwrap_or(SnapValue::Beat1_4));

    Frame::none()
        .fill(Color32::from_rgb(18, 18, 22))
        .stroke(Stroke::new(1.0_f32, Color32::from_gray(45)))
        .inner_margin(6.0)
        .show(ui, |ui| {
            // Header del Clip Editor
            ui.horizontal(|ui| {
                ui.label(format!("🎵 {}", clip.name));
                ui.weak(format!("({:.2}s)", clip.duration_secs));
                
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.toggle_value(&mut clip.loop_enabled, "🔁 Loop");

                    ui.add_space(8.0);

                    // Selector de Snap / Cuantización
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
            let (rect, response) = ui.allocate_exact_size(available_size, Sense::click_and_drag());

            if ui.is_rect_visible(rect) {
                let total_frames = (clip.duration_secs * sample_rate as f64) as u64;

                // Auto-inicializar el loop al total del audio si loop_end es 0
                if clip.loop_end == 0 && total_frames > 0 {
                    clip.loop_end = total_frames;
                }

                // LAYER 1: Background Canvas
                ui.painter().rect_filled(rect, 4.0_f32, Color32::from_rgb(12, 12, 15));

                // LAYER 2: Timeline Bar & Beat Grid
                let current_bpm = if bpm > 0.0 { bpm as f64 } else { 120.0 };
                let sec_per_beat = 60.0 / current_bpm;
                let sec_per_bar = sec_per_beat * 4.0;
                let total_bars = (clip.duration_secs / sec_per_bar).ceil() as usize;

                let ruler_height = 18.0_f32;
                let ruler_rect = Rect::from_min_max(
                    rect.min,
                    Pos2::new(rect.max.x, rect.min.y + ruler_height),
                );
                let wave_rect = Rect::from_min_max(
                    Pos2::new(rect.min.x, rect.min.y + ruler_height),
                    rect.max,
                );

                // Fondo de la regla de compases
                ui.painter().rect_filled(ruler_rect, 0.0_f32, Color32::from_rgb(22, 22, 28));
                ui.painter().line_segment(
                    [Pos2::new(rect.min.x, ruler_rect.max.y), Pos2::new(rect.max.x, ruler_rect.max.y)],
                    Stroke::new(1.0_f32, Color32::from_gray(50)),
                );

                if clip.duration_secs > 0.0 {
                    for bar in 0..=total_bars {
                        let bar_time = bar as f64 * sec_per_bar;
                        if bar_time > clip.duration_secs {
                            break;
                        }

                        let norm_x = (bar_time / clip.duration_secs) as f32;
                        let x_pos = rect.min.x + (norm_x * rect.width());

                        // Línea principal de Compás (Bar Line)
                        ui.painter().line_segment(
                            [Pos2::new(x_pos, rect.min.y), Pos2::new(x_pos, rect.max.y)],
                            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 40)),
                        );

                        // Número de compás
                        ui.painter().text(
                            Pos2::new(x_pos + 4.0, rect.min.y + 2.0),
                            Align2::LEFT_TOP,
                            format!("{}", bar + 1),
                            FontId::proportional(10.0),
                            Color32::from_gray(180),
                        );

                        // Subdivisiones por Tiempos (Beats 2, 3, 4)
                        for beat in 1..4 {
                            let beat_time = bar_time + (beat as f64 * sec_per_beat);
                            if beat_time >= clip.duration_secs {
                                break;
                            }

                            let beat_norm_x = (beat_time / clip.duration_secs) as f32;
                            let beat_x_pos = rect.min.x + (beat_norm_x * rect.width());

                            ui.painter().line_segment(
                                [Pos2::new(beat_x_pos, wave_rect.min.y), Pos2::new(beat_x_pos, wave_rect.max.y)],
                                Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 12)),
                            );
                        }
                    }
                }

                // LAYER 3: Time Selection / Loop Region Background
                if total_frames > 0 && clip.loop_end > clip.loop_start {
                    let start_norm = clip.loop_start as f32 / total_frames as f32;
                    let end_norm = clip.loop_end as f32 / total_frames as f32;

                    let sel_x_min = rect.min.x + (start_norm * rect.width());
                    let sel_x_max = rect.min.x + (end_norm * rect.width());

                    let sel_rect = Rect::from_min_max(
                        Pos2::new(sel_x_min, wave_rect.min.y),
                        Pos2::new(sel_x_max, wave_rect.max.y),
                    );

                    ui.painter().rect_filled(sel_rect, 0.0_f32, Color32::from_rgba_unmultiplied(0, 120, 255, 45));
                }

                // LAYER 4: Waveform
                if !clip.pcm_data.is_empty() {
                    waveform::draw_waveform(ui, wave_rect, &clip.pcm_data, Color32::from_rgb(0, 200, 255));
                }

                // LAYER 5: Time Selection Borders
                if total_frames > 0 && clip.loop_end > clip.loop_start {
                    let start_norm = clip.loop_start as f32 / total_frames as f32;
                    let end_norm = clip.loop_end as f32 / total_frames as f32;

                    let sel_x_min = rect.min.x + (start_norm * rect.width());
                    let sel_x_max = rect.min.x + (end_norm * rect.width());

                    ui.painter().line_segment(
                        [Pos2::new(sel_x_min, rect.min.y), Pos2::new(sel_x_min, rect.max.y)],
                        Stroke::new(2.0_f32, Color32::from_rgb(0, 255, 200)),
                    );
                    ui.painter().line_segment(
                        [Pos2::new(sel_x_max, rect.min.y), Pos2::new(sel_x_max, rect.max.y)],
                        Stroke::new(2.0_f32, Color32::from_rgb(255, 80, 80)),
                    );
                }

                // Drag con Snap to Grid
                if response.dragged() {
                    if let Some(pointer_pos) = response.interact_pointer_pos() {
                        let press_pos = ui.input(|i| i.pointer.press_origin()).unwrap_or(pointer_pos);
                        
                        let x_min = press_pos.x.min(pointer_pos.x).clamp(rect.min.x, rect.max.x);
                        let x_max = press_pos.x.max(pointer_pos.x).clamp(rect.min.x, rect.max.x);

                        let mut time_start = ((x_min - rect.min.x) / rect.width()) as f64 * clip.duration_secs;
                        let mut time_end = ((x_max - rect.min.x) / rect.width()) as f64 * clip.duration_secs;

                        // Aplicar Snap to Grid si está activo
                        let snap_step = current_snap.interval_secs(bpm);
                        if snap_step > 0.0 {
                            time_start = (time_start / snap_step).round() * snap_step;
                            time_end = (time_end / snap_step).round() * snap_step;
                        }

                        // Convertir tiempo a frames
                        let norm_start = (time_start / clip.duration_secs).clamp(0.0, 1.0);
                        let norm_end = (time_end / clip.duration_secs).clamp(0.0, 1.0);

                        clip.loop_start = (norm_start * total_frames as f64) as u64;
                        clip.loop_end = (norm_end * total_frames as f64) as u64;
                    }
                }

                // LAYER 6: Playhead Local
                if let Some(frames) = elapsed_frames {
                    if total_frames > 0 {
                        let local_frame = frames % total_frames;
                        let progress = local_frame as f32 / total_frames as f32;
                        let playhead_x = rect.min.x + (progress * rect.width());

                        ui.painter().line_segment(
                            [Pos2::new(playhead_x, rect.min.y), Pos2::new(playhead_x, rect.max.y)],
                            Stroke::new(2.0_f32, Color32::from_rgb(255, 255, 255)),
                        );
                    }
                }

                // Borde contenedor
                ui.painter().rect_stroke(rect, 4.0_f32, Stroke::new(1.0_f32, Color32::from_gray(50)));
            }
        });
}