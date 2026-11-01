use gpui_kit::*;

/// Renderiza una forma de onda vectorial en GPU a partir de muestras PCM crudas,
/// dibujando un cuerpo simétrico semitransparente, línea central de cruce por cero
/// y trazos verticales de pico de alta densidad.
pub fn draw_waveform_bounds(
    window: &mut Window,
    rect: Bounds<Pixels>,
    pcm_data: &[f32],
    color: Hsla,
) {
    let width: f32 = rect.size.width.into();
    let height: f32 = rect.size.height.into();
    if pcm_data.is_empty() || width <= 0.0 || height <= 0.0 {
        return;
    }

    let origin_x: f32 = rect.origin.x.into();
    let origin_y: f32 = rect.origin.y.into();
    let center_y = origin_y + height / 2.0;
    let half_height = (height / 2.0) * 0.92_f32;
    let width_px = width.floor() as usize;

    if width_px == 0 {
        return;
    }

    // Línea base central (zero-crossing reference)
    let zero_color = Hsla {
        a: (color.a * 0.28).clamp(0.08, 0.45),
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
        let abs_s = sample.abs();
        if abs_s > max_peak {
            max_peak = abs_s;
        }
    }

    if max_peak < 0.00001_f32 {
        return;
    }

    let norm_scale = 1.0_f32 / max_peak;
    let total_samples = pcm_data.len();

    let rms_color = Hsla {
        a: (color.a * 0.45).clamp(0.15, 0.65),
        ..color
    };

    for x_idx in 0..width_px {
        let start_sample = (x_idx * total_samples) / width_px;
        let end_sample = (((x_idx + 1) * total_samples) / width_px).min(total_samples);

        if start_sample >= end_sample {
            continue;
        }

        let slice = &pcm_data[start_sample..end_sample];

        let mut sample_min = 0.0_f32;
        let mut sample_max = 0.0_f32;
        let mut sum_sq = 0.0_f32;

        for &s in slice {
            let scaled = s * norm_scale;
            if scaled < sample_min {
                sample_min = scaled;
            }
            if scaled > sample_max {
                sample_max = scaled;
            }
            sum_sq += scaled * scaled;
        }

        if sample_min == 0.0_f32 && sample_max == 0.0_f32 {
            continue;
        }

        let x_pos = origin_x + x_idx as f32;

        let y_top = center_y - (sample_max * half_height);
        let y_bottom = center_y - (sample_min * half_height);

        let y_min_final = y_top.min(y_bottom);
        let mut y_max_final = y_top.max(y_bottom);

        if (y_max_final - y_min_final) < 1.5 {
            y_max_final = y_min_final + 1.5;
        }

        // Pico exterior (trazo fino)
        let mut pb = PathBuilder::stroke(px(1.0));
        pb.move_to(point(px(x_pos), px(y_min_final)));
        pb.line_to(point(px(x_pos), px(y_max_final)));
        if let Ok(path) = pb.build() {
            window.paint_path(path, color);
        }

        // Cuerpo RMS interior más brillante
        let rms = (sum_sq / slice.len() as f32).sqrt();
        if rms > 0.02 {
            let rms_h = (rms * half_height).max(1.0);
            let mut rms_pb = PathBuilder::stroke(px(1.0));
            rms_pb.move_to(point(px(x_pos), px(center_y - rms_h)));
            rms_pb.line_to(point(px(x_pos), px(center_y + rms_h)));
            if let Ok(path) = rms_pb.build() {
                window.paint_path(path, rms_color);
            }
        }
    }
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
        a: (peak_color.a * 0.25).clamp(0.08, 0.35),
        ..peak_color
    };
    let mut zero_pb = PathBuilder::stroke(px(1.0));
    zero_pb.move_to(point(px(origin_x), px(center_y)));
    zero_pb.line_to(point(px(origin_x + width), px(center_y)));
    if let Ok(path) = zero_pb.build() {
        window.paint_path(path, zero_line_color);
    }

    let start_n = trim_start_norm.clamp(0.0, 1.0);
    let end_n = trim_end_norm.clamp(start_n + 0.001, 1.0);
    let start_idx = ((start_n * peaks.len() as f32).floor() as usize).min(peaks.len().saturating_sub(1));
    let end_idx = ((end_n * peaks.len() as f32).ceil() as usize).clamp(start_idx + 1, peaks.len());
    let visible = &peaks[start_idx..end_idx];
    if visible.is_empty() {
        return;
    }

    let cols = (width.floor() as usize).clamp(1, 512);
    let step = width / cols as f32;
    let stroke_w = (step * 0.78).clamp(1.0, 2.5);

    for col in 0..cols {
        let p0 = (col * visible.len()) / cols;
        let p1 = (((col + 1) * visible.len()) / cols).max(p0 + 1).min(visible.len());
        let mut peak = 0.0_f32;
        let mut sum = 0.0_f32;
        for &v in &visible[p0..p1] {
            let av = v.abs().clamp(0.0, 1.0);
            if av > peak {
                peak = av;
            }
            sum += av;
        }
        let avg = sum / (p1 - p0) as f32;
        let x = origin_x + (col as f32 + 0.5) * step;

        let amp_h = (peak * half_height).max(1.0);
        let mut pb = PathBuilder::stroke(px(stroke_w));
        pb.move_to(point(px(x), px(center_y - amp_h)));
        pb.line_to(point(px(x), px(center_y + amp_h)));
        if let Ok(path) = pb.build() {
            window.paint_path(path, peak_color);
        }

        let rms_h = (avg * 0.68 * half_height).max(0.5);
        if rms_h >= 1.0 {
            let mut rpb = PathBuilder::stroke(px(stroke_w));
            rpb.move_to(point(px(x), px(center_y - rms_h)));
            rpb.line_to(point(px(x), px(center_y + rms_h)));
            if let Ok(path) = rpb.build() {
                window.paint_path(path, rms_color);
            }
        }
    }
}

