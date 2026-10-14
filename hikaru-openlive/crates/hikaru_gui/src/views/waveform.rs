use gpui_kit::*;

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
    let half_height = (height / 2.0) * 0.95_f32;
    let width_px = width.floor() as usize;

    if width_px == 0 {
        return;
    }

    let mut max_peak = 0.0_f32;
    for &sample in pcm_data {
        let abs_s = sample.abs();
        if abs_s > max_peak {
            max_peak = abs_s;
        }
    }

    if max_peak < 0.00001_f32 {
        let mut pb = PathBuilder::stroke(px(1.0));
        pb.move_to(point(px(origin_x), px(center_y)));
        pb.line_to(point(px(origin_x + width), px(center_y)));
        window.paint_path(pb.build().unwrap(), color);
        return;
    }

    let norm_scale = 1.0_f32 / max_peak;
    let total_samples = pcm_data.len();

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
            if scaled < sample_min {
                sample_min = scaled;
            }
            if scaled > sample_max {
                sample_max = scaled;
            }
        }

        if sample_min == 0.0_f32 && sample_max == 0.0_f32 {
            continue;
        }

        let x_pos = origin_x + x_idx as f32;

        let y_top = center_y - (sample_max * half_height);
        let y_bottom = center_y - (sample_min * half_height);

        let y_min_final = y_top.min(y_bottom);
        let mut y_max_final = y_top.max(y_bottom);

        if (y_max_final - y_min_final) < 2.0 {
            y_max_final = y_min_final + 2.0;
        }

        let mut pb = PathBuilder::stroke(px(1.0));
        pb.move_to(point(px(x_pos), px(y_min_final)));
        pb.line_to(point(px(x_pos), px(y_max_final)));
        window.paint_path(pb.build().unwrap(), color);
    }
}
