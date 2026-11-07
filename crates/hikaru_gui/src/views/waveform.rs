// Hikaru OpenLive / OpenStudio - GPU Vector Waveform Renderer (egui)
// GNU AGPLv3
// crates/hikaru_gui/src/views/waveform.rs

use egui::{Color32, Pos2, Rect, Stroke};

/// Renderiza una forma de onda vectorial a partir de muestras PCM crudas,
/// dibujando la envolvente exterior de picos y el núcleo RMS en el painter de egui.
pub fn draw_waveform_bounds(
    painter: &egui::Painter,
    rect: Rect,
    pcm_data: &[f32],
    color: Color32,
) {
    let width = rect.width();
    let height = rect.height();
    if pcm_data.is_empty() || width <= 0.5 || height <= 1.0 {
        return;
    }

    let origin_x = rect.min.x;
    let origin_y = rect.min.y;
    let center_y = origin_y + height * 0.5;
    let half_height = (height * 0.5) * 0.90;

    // Línea base central
    let zero_color = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), (color.a() as f32 * 0.25) as u8);
    painter.line_segment(
        [Pos2::new(origin_x, center_y), Pos2::new(origin_x + width, center_y)],
        Stroke::new(1.0_f32, zero_color),
    );

    let mut max_peak = 0.0_f32;
    for &sample in pcm_data {
        let abs_s = if sample.is_finite() { sample.abs() } else { 0.0 };
        if abs_s > max_peak {
            max_peak = abs_s;
        }
    }
    if max_peak < 0.00001_f32 {
        return;
    }

    let norm_scale = 1.0_f32 / max_peak;
    let total_samples = pcm_data.len();
    let cols = (width.floor() as usize).clamp(1, 768);
    let step = width / cols as f32;

    let mut columns: Vec<(f32, f32, f32, f32)> = Vec::with_capacity(cols);
    for col in 0..cols {
        let s0 = (col * total_samples) / cols;
        let s1 = (((col + 1) * total_samples) / cols).max(s0 + 1).min(total_samples);
        if s0 >= s1 {
            continue;
        }
        let slice = &pcm_data[s0..s1];
        let mut s_min = 0.0_f32;
        let mut s_max = 0.0_f32;
        let mut sum_sq = 0.0_f32;
        for &s in slice {
            let v = if s.is_finite() { (s * norm_scale).clamp(-1.0, 1.0) } else { 0.0 };
            if v < s_min {
                s_min = v;
            }
            if v > s_max {
                s_max = v;
            }
            sum_sq += v * v;
        }
        let x = origin_x + (col as f32 + 0.5) * step;
        let y_top = (center_y - s_max * half_height).min(center_y - 0.5);
        let y_bot = (center_y - s_min * half_height).max(center_y + 0.5);
        let rms = (sum_sq / slice.len() as f32).sqrt();
        let rms_h = (rms * half_height).clamp(0.35, half_height);
        columns.push((x, y_top, y_bot, rms_h));
    }

    let peak_fill = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), (color.a() as f32 * 0.55) as u8);
    let core_fill = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), (color.a() as f32 * 0.85) as u8);
    paint_batched_envelope(painter, &columns, center_y, peak_fill, color, core_fill);
}

/// Renderiza una forma de onda a partir de picos normalizados `[0.0..1.0]`.
pub fn draw_waveform_peaks_bounds(
    painter: &egui::Painter,
    rect: Rect,
    peaks: &[f32],
    trim_start_norm: f32,
    trim_end_norm: f32,
    peak_color: Color32,
    rms_color: Color32,
) {
    let width = rect.width();
    let height = rect.height();
    if peaks.is_empty() || width <= 1.0 || height <= 1.0 {
        return;
    }

    let origin_x = rect.min.x;
    let origin_y = rect.min.y;
    let center_y = origin_y + height * 0.5;
    let half_height = (height * 0.5) * 0.90;

    // Línea central de cruce por cero
    let zero_line_color = Color32::from_rgba_unmultiplied(peak_color.r(), peak_color.g(), peak_color.b(), (peak_color.a() as f32 * 0.24) as u8);
    painter.line_segment(
        [Pos2::new(origin_x, center_y), Pos2::new(origin_x + width, center_y)],
        Stroke::new(1.0_f32, zero_line_color),
    );

    let start_n = trim_start_norm.clamp(0.0, 1.0);
    let end_n = trim_end_norm.clamp(start_n + 0.0005, 1.0);
    let start_idx = ((start_n * peaks.len() as f32).floor() as usize).min(peaks.len().saturating_sub(1));
    let end_idx = ((end_n * peaks.len() as f32).ceil() as usize).clamp(start_idx + 1, peaks.len());
    let visible = &peaks[start_idx..end_idx];
    if visible.is_empty() {
        return;
    }

    let cols = (width.floor() as usize).clamp(1, 768);
    let step = width / cols as f32;
    let mut columns: Vec<(f32, f32, f32, f32)> = Vec::with_capacity(cols);

    for col in 0..cols {
        let p0 = (col * visible.len()) / cols;
        let p1 = (((col + 1) * visible.len()) / cols).max(p0 + 1).min(visible.len());
        let mut peak = 0.0_f32;
        let mut sum_sq = 0.0_f32;
        for &v in &visible[p0..p1] {
            let av = if v.is_finite() { v.abs().clamp(0.0, 1.0) } else { 0.0 };
            if av > peak {
                peak = av;
            }
            sum_sq += av * av;
        }
        let rms = (sum_sq / (p1 - p0) as f32).sqrt();
        let x = origin_x + (col as f32 + 0.5) * step;
        let amp_h = (peak * half_height).max(0.5);
        let rms_h = (rms * 0.66 * half_height).clamp(0.35, amp_h);
        columns.push((x, center_y - amp_h, center_y + amp_h, rms_h));
    }

    let body_fill = Color32::from_rgba_unmultiplied(peak_color.r(), peak_color.g(), peak_color.b(), (peak_color.a() as f32 * 0.58) as u8);
    let core_fill = Color32::from_rgba_unmultiplied(rms_color.r(), rms_color.g(), rms_color.b(), (rms_color.a() as f32 * 0.85) as u8);
    paint_batched_envelope(painter, &columns, center_y, body_fill, peak_color, core_fill);
}

fn paint_batched_envelope(
    painter: &egui::Painter,
    columns: &[(f32, f32, f32, f32)],
    center_y: f32,
    peak_fill: Color32,
    peak_stroke: Color32,
    rms_fill: Color32,
) {
    if columns.is_empty() {
        return;
    }
    if columns.len() == 1 {
        let (x, y_top, y_bot, _) = columns[0];
        painter.line_segment([Pos2::new(x, y_top), Pos2::new(x, y_bot)], Stroke::new(1.5_f32, peak_stroke));
        return;
    }

    // 1. Polígono exterior de picos
    let mut env_points = Vec::with_capacity(columns.len() * 2);
    for &(x, y_top, _, _) in columns {
        env_points.push(Pos2::new(x, y_top));
    }
    for &(x, _, y_bot, _) in columns.iter().rev() {
        env_points.push(Pos2::new(x, y_bot));
    }
    painter.add(egui::Shape::convex_polygon(env_points, peak_fill, Stroke::NONE));

    // 2. Polígono interior RMS
    let mut core_points = Vec::with_capacity(columns.len() * 2);
    for &(x, _, _, rms_h) in columns {
        core_points.push(Pos2::new(x, center_y - rms_h));
    }
    for &(x, _, _, rms_h) in columns.iter().rev() {
        core_points.push(Pos2::new(x, center_y + rms_h));
    }
    painter.add(egui::Shape::convex_polygon(core_points, rms_fill, Stroke::NONE));

    // 3. Trazos superior e inferior de transientes
    let mut upper_pts = Vec::with_capacity(columns.len());
    let mut lower_pts = Vec::with_capacity(columns.len());
    for &(x, y_top, y_bot, _) in columns {
        upper_pts.push(Pos2::new(x, y_top));
        lower_pts.push(Pos2::new(x, y_bot));
    }
    painter.add(egui::Shape::line(upper_pts, Stroke::new(1.0_f32, peak_stroke)));
    painter.add(egui::Shape::line(lower_pts, Stroke::new(1.0_f32, peak_stroke)));
}
