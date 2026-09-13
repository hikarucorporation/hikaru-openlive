// crates/hikaru_gui/src/views/clip_editor.rs
use egui::{Color32, Frame, Pos2, Rect, Sense, Stroke, Ui};
use crate::views::matrix::MatrixClip;
use crate::views::waveform;

pub fn show(
    ui: &mut Ui,
    clip: &mut MatrixClip,
    elapsed_frames: Option<u64>,
    sample_rate: u32,
) {
    // 1. Decodificación limpia soportando 16-bit, 24/32-bit e i32/f32
    if clip.pcm_data.is_empty() && clip.path.exists() {
        if let Ok(mut reader) = hound::WavReader::open(&clip.path) {
            let spec = reader.spec();
            let channels = spec.channels as usize;
            let bits = spec.bits_per_sample;

            let samples: Vec<f32> = match spec.sample_format {
                hound::SampleFormat::Float => {
                    reader.into_samples::<f32>().filter_map(Result::ok).collect()
                }
                hound::SampleFormat::Int => {
                    if bits <= 16 {
                        let max_val = i16::MAX as f32;
                        reader
                            .into_samples::<i16>()
                            .filter_map(Result::ok)
                            .map(|s| s as f32 / max_val)
                            .collect()
                    } else {
                        let max_val = i32::MAX as f32;
                        reader
                            .into_samples::<i32>()
                            .filter_map(Result::ok)
                            .map(|s| s as f32 / max_val)
                            .collect()
                    }
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

                // LAYER 2: Time Selection / Loop Region Background
                if total_frames > 0 && clip.loop_end > clip.loop_start {
                    let start_norm = clip.loop_start as f32 / total_frames as f32;
                    let end_norm = clip.loop_end as f32 / total_frames as f32;

                    let sel_x_min = rect.min.x + (start_norm * rect.width());
                    let sel_x_max = rect.min.x + (end_norm * rect.width());

                    let sel_rect = Rect::from_min_max(
                        Pos2::new(sel_x_min, rect.min.y),
                        Pos2::new(sel_x_max, rect.max.y),
                    );

                    // Sombra traslúcida azul para el fondo seleccionado
                    ui.painter().rect_filled(sel_rect, 0.0_f32, Color32::from_rgba_unmultiplied(0, 120, 255, 50));
                }

                // LAYER 3: Waveform
                if !clip.pcm_data.is_empty() {
                    waveform::draw_waveform(ui, rect, &clip.pcm_data, Color32::from_rgb(0, 200, 255));
                } else {
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "No se pudo decodificar el archivo de audio",
                        egui::FontId::proportional(12.0_f32),
                        Color32::from_rgb(255, 100, 100),
                    );
                }

                // LAYER 4: Time Selection Borders & Handles
                if total_frames > 0 && clip.loop_end > clip.loop_start {
                    let start_norm = clip.loop_start as f32 / total_frames as f32;
                    let end_norm = clip.loop_end as f32 / total_frames as f32;

                    let sel_x_min = rect.min.x + (start_norm * rect.width());
                    let sel_x_max = rect.min.x + (end_norm * rect.width());

                    // Bordes de inicio (verde) y fin (rojo)
                    ui.painter().line_segment(
                        [Pos2::new(sel_x_min, rect.min.y), Pos2::new(sel_x_min, rect.max.y)],
                        Stroke::new(2.0_f32, Color32::from_rgb(0, 255, 200)),
                    );
                    ui.painter().line_segment(
                        [Pos2::new(sel_x_max, rect.min.y), Pos2::new(sel_x_max, rect.max.y)],
                        Stroke::new(2.0_f32, Color32::from_rgb(255, 80, 80)),
                    );
                }

                // Manejo de Drag para Time Selection interactivo
                if response.dragged() {
                    if let Some(pointer_pos) = response.interact_pointer_pos() {
                        let press_pos = ui.input(|i| i.pointer.press_origin()).unwrap_or(pointer_pos);
                        
                        let x_min = press_pos.x.min(pointer_pos.x).clamp(rect.min.x, rect.max.x);
                        let x_max = press_pos.x.max(pointer_pos.x).clamp(rect.min.x, rect.max.x);

                        let norm_start = ((x_min - rect.min.x) / rect.width()) as f64;
                        let norm_end = ((x_max - rect.min.x) / rect.width()) as f64;

                        clip.loop_start = (norm_start * total_frames as f64) as u64;
                        clip.loop_end = (norm_end * total_frames as f64) as u64;
                    }
                }

                // LAYER 5: Playhead Local
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

                // Outer Border
                ui.painter().rect_stroke(rect, 4.0_f32, Stroke::new(1.0_f32, Color32::from_gray(50)));
            }
        });
}