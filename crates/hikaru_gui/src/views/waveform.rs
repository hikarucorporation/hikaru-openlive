// crates/hikaru_gui/src/views/waveform.rs
use egui::{Color32, Pos2, Rect, Stroke, Ui};

pub fn draw_waveform(ui: &mut Ui, rect: Rect, pcm_data: &[f32], color: Color32) {
    if pcm_data.is_empty() || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }

    let painter = ui.painter();
    let center_y = rect.center().y;
    let half_height = (rect.height() / 2.0) * 0.95_f32; // 95% del canvas
    let width_px = rect.width().floor() as usize;

    if width_px == 0 {
        return;
    }

    // 1. Escaneo del pico absoluto para normalización visual completa
    let mut max_peak = 0.0_f32;
    for &sample in pcm_data {
        let abs_s = sample.abs();
        if abs_s > max_peak {
            max_peak = abs_s;
        }
    }

    // Si es silencio absoluto
    if max_peak < 0.00001_f32 {
        painter.line_segment(
            [Pos2::new(rect.min.x, center_y), Pos2::new(rect.max.x, center_y)],
            Stroke::new(1.0_f32, color.linear_multiply(0.3)),
        );
        return;
    }

    // Escala de ganancia visual: fuerza que el pico máximo toque los bordes superior/inferior
    let norm_scale = 1.0_f32 / max_peak;
    let total_samples = pcm_data.len();

    // 2. Trazado de min/max por cada píxel horizontal
    for x_idx in 0..width_px {
        let start_sample = (x_idx * total_samples) / width_px;
        let end_sample = (((x_idx + 1) * total_samples) / width_px).min(total_samples);

        if start_sample >= end_sample {
            continue;
        }

        let slice = &pcm_data[start_sample..end_sample];

        let mut sample_min = 0.0_f32;
        let mut sample_max = 0.0_f32;

        for &s in slice {
            let scaled = s * norm_scale;
            if scaled < sample_min { sample_min = scaled; }
            if scaled > sample_max { sample_max = scaled; }
        }

        let x_pos = rect.min.x + x_idx as f32;

        // Mapeo top-down en coordenadas de egui
        let y_top = center_y - (sample_max * half_height);
        let y_bottom = center_y - (sample_min * half_height);

        // Garantizar al menos 2px de altura para picos muy breves o transitorios
        let y_min_final = y_top.min(y_bottom);
        let mut y_max_final = y_top.max(y_bottom);

        if (y_max_final - y_min_final) < 2.0 {
            y_max_final = y_min_final + 2.0;
        }

        painter.line_segment(
            [Pos2::new(x_pos, y_min_final), Pos2::new(x_pos, y_max_final)],
            Stroke::new(1.0_f32, color),
        );
    }
}