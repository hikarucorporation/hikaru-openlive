// Hikaru OpenLive / OpenStudio - GPU Vector Waveform Renderer
// GNU AGPLv3
// crates/hikaru_gui/src/views/waveform.rs

use gpui_kit::*;

/// Renderiza una forma de onda vectorial en GPU a partir de muestras PCM crudas,
/// agrupando la envolvente completa de picos, el núcleo energético RMS y los filos
/// de transientes en polígonos/trazos vectoriales continuos (4 draw calls totales).
pub fn draw_waveform_bounds(
    window: &mut Window,
    rect: Bounds<Pixels>,
    pcm_data: &[f32],
    color: Hsla,
) {
    let width: f32 = rect.size.width.into();
    let height: f32 = rect.size.height.into();
    if pcm_data.is_empty() || width <= 0.5 || height <= 1.0 {
        return;
    }

    let origin_x: f32 = rect.origin.x.into();
    let origin_y: f32 = rect.origin.y.into();
    let center_y = origin_y + height * 0.5;
    let half_height = (height * 0.5) * 0.90_f32;

    // Línea base central (referencia de cruce por cero)
    let zero_color = Hsla {
        a: (color.a * 0.26).clamp(0.08, 0.42),
        ..color
    };
    let mut zero_pb = PathBuilder::stroke(px(1.0));
    zero_pb.move_to(point(px(origin_x), px(center_y)));
    zero_pb.line_to(point(px(origin_x + width), px(center_y)));
    if let Ok(path) = zero_pb.build() {
        window.paint_path(path, zero_color);
    }

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

    paint_batched_envelope(
        window,
        &columns,
        center_y,
        Hsla {
            a: (color.a * 0.56).clamp(0.18, 0.82),
            ..color
        },
        color,
        Hsla {
            l: (color.l + 0.16).clamp(0.0, 0.96),
            a: (color.a * 0.82).clamp(0.25, 0.95),
            ..color
        },
    );
}

/// Renderiza una forma de onda vectorial en GPU a partir de picos normalizados `[0.0..1.0]`
/// (usada en `ClipCell` de Session Matrix y clips de Playlist/Arranger).
/// Soporta un recorte `trim_start_norm..trim_end_norm` en `[0.0..1.0]`.
pub fn draw_waveform_peaks_bounds(
    window: &mut Window,
    rect: Bounds<Pixels>,
    peaks: &[f32],
    trim_start_norm: f32,
    trim_end_norm: f32,
    peak_color: Hsla,
    rms_color: Hsla,
) {
    let width: f32 = rect.size.width.into();
    let height: f32 = rect.size.height.into();
    if peaks.is_empty() || width <= 1.0 || height <= 1.0 {
        return;
    }

    let origin_x: f32 = rect.origin.x.into();
    let origin_y: f32 = rect.origin.y.into();
    let center_y = origin_y + height * 0.5;
    let half_height = (height * 0.5) * 0.90;

    // Línea central de cruce por cero
    let zero_line_color = Hsla {
        a: (peak_color.a * 0.24).clamp(0.08, 0.34),
        ..peak_color
    };
    let mut zero_pb = PathBuilder::stroke(px(1.0));
    zero_pb.move_to(point(px(origin_x), px(center_y)));
    zero_pb.line_to(point(px(origin_x + width), px(center_y)));
    if let Ok(path) = zero_pb.build() {
        window.paint_path(path, zero_line_color);
    }

    let start_n = trim_start_norm.clamp(0.0, 1.0);
    let end_n = trim_end_norm.clamp(start_n + 0.0005, 1.0);
    let start_idx = ((start_n * peaks.len() as f32).floor() as usize)
        .min(peaks.len().saturating_sub(1));
    let end_idx = ((end_n * peaks.len() as f32).ceil() as usize)
        .clamp(start_idx + 1, peaks.len());
    let visible = &peaks[start_idx..end_idx];
    if visible.is_empty() {
        return;
    }

    let cols = (width.floor() as usize).clamp(1, 768);
    let step = width / cols as f32;
    let mut columns: Vec<(f32, f32, f32, f32)> = Vec::with_capacity(cols);

    for col in 0..cols {
        let p0 = (col * visible.len()) / cols;
        let p1 = (((col + 1) * visible.len()) / cols)
            .max(p0 + 1)
            .min(visible.len());
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

    let body_fill = Hsla {
        a: (peak_color.a * 0.58).clamp(0.20, 0.84),
        ..peak_color
    };
    let core_fill = Hsla {
        a: (rms_color.a * 0.85).clamp(0.30, 0.95),
        ..rms_color
    };
    paint_batched_envelope(window, &columns, center_y, body_fill, peak_color, core_fill);
}

/// Pinta en lote la silueta completa de picos, el núcleo RMS interior y los
/// contornos superior/inferior de transientes en solo 4 llamadas a GPU.
fn paint_batched_envelope(
    window: &mut Window,
    columns: &[(f32, f32, f32, f32)],
    center_y: f32,
    peak_fill: Hsla,
    peak_stroke: Hsla,
    rms_fill: Hsla,
) {
    if columns.is_empty() {
        return;
    }
    if columns.len() == 1 {
        let (x, y_top, y_bot, _) = columns[0];
        let mut pb = PathBuilder::stroke(px(1.5));
        pb.move_to(point(px(x), px(y_top)));
        pb.line_to(point(px(x), px(y_bot)));
        if let Ok(path) = pb.build() {
            window.paint_path(path, peak_stroke);
        }
        return;
    }

    // 1. Polígono relleno de envolvente exterior de picos
    let mut env = PathBuilder::fill();
    env.move_to(point(px(columns[0].0), px(columns[0].1)));
    for &(x, y_top, _, _) in &columns[1..] {
        env.line_to(point(px(x), px(y_top)));
    }
    for &(x, _, y_bot, _) in columns.iter().rev() {
        env.line_to(point(px(x), px(y_bot)));
    }
    env.close();
    if let Ok(path) = env.build() {
        window.paint_path(path, peak_fill);
    }

    // 2. Polígono relleno del núcleo energético RMS interior
    let mut core = PathBuilder::fill();
    core.move_to(point(px(columns[0].0), px(center_y - columns[0].3)));
    for &(x, _, _, rms_h) in &columns[1..] {
        core.line_to(point(px(x), px(center_y - rms_h)));
    }
    for &(x, _, _, rms_h) in columns.iter().rev() {
        core.line_to(point(px(x), px(center_y + rms_h)));
    }
    core.close();
    if let Ok(path) = core.build() {
        window.paint_path(path, rms_fill);
    }

    // 3. Contornos nítidos de 1px en crestas superiores e inferiores
    let mut upper = PathBuilder::stroke(px(1.0));
    upper.move_to(point(px(columns[0].0), px(columns[0].1)));
    for &(x, y_top, _, _) in &columns[1..] {
        upper.line_to(point(px(x), px(y_top)));
    }
    if let Ok(path) = upper.build() {
        window.paint_path(path, peak_stroke);
    }

    let mut lower = PathBuilder::stroke(px(1.0));
    lower.move_to(point(px(columns[0].0), px(columns[0].2)));
    for &(x, _, y_bot, _) in &columns[1..] {
        lower.line_to(point(px(x), px(y_bot)));
    }
    if let Ok(path) = lower.build() {
        window.paint_path(path, peak_stroke);
    }
}
