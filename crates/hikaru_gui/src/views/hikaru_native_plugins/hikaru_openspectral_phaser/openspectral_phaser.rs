// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_openspectral_phaser/openspectral_phaser.rs
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::app::AppState;
use crate::theme;

const PARAM_NAMES: [&str; 11] = [
    "Width", "Offset", "Depth", "Speed", "Mix", "Blur", "Hz/Oct", "1x/5x",
    "Keep Bass", "Blocksize", "Bypass",
];

const DEFAULTS: [f32; 11] = [50.0, 0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 0.0, 0.0, 1024.0, 0.0];

pub const BLOCKSIZE_OPTIONS: [f32; 6] = [512.0, 1024.0, 2048.0, 4096.0, 8192.0, 16384.0];

type StateMap = Mutex<HashMap<(usize, usize), [f32; 11]>>;
fn state_map() -> &'static StateMap {
    static CELL: OnceLock<StateMap> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(HashMap::new()))
}

fn param_min(idx: usize) -> f32 {
    match idx {
        0 | 2 | 4 | 5 => 0.0,
        1 | 3 | 6 => -100.0,
        7 | 8 | 10 => 0.0,
        9 => 512.0,
        _ => 0.0,
    }
}

fn param_max(idx: usize) -> f32 {
    match idx {
        0 | 2 | 4 | 5 => 100.0,
        1 | 3 | 6 => 100.0,
        7 | 8 | 10 => 1.0,
        9 => 16384.0,
        _ => 100.0,
    }
}

fn clamp_param(idx: usize, value: f32) -> f32 {
    value.clamp(param_min(idx), param_max(idx))
}

pub fn get_params(track_idx: usize, slot_idx: usize) -> [f32; 11] {
    let mut map = state_map().lock().unwrap();
    *map.entry((track_idx, slot_idx)).or_insert(DEFAULTS)
}

pub fn set_param(track_idx: usize, slot_idx: usize, idx: usize, value: f32) {
    let mut map = state_map().lock().unwrap();
    let entry = map.entry((track_idx, slot_idx)).or_insert(DEFAULTS);
    if idx < 11 {
        entry[idx] = clamp_param(idx, value);
    }
}

const SCOPE_BINS: usize = 48;
const SCOPE_FPS: f32 = 15.0;

fn scope_time_secs() -> f32 {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f32()
}

fn scope_tick() -> u64 {
    (scope_time_secs() * SCOPE_FPS) as u64
}

fn compute_phaser_scope_magnitudes(params: [f32; 11]) -> Vec<f32> {
    use hikaru_dsp::effects::openspectral_phaser::{SpectralPhaser, SpectralPhaserParams};

    if params[10] >= 0.5 {
        return vec![1.0; SCOPE_BINS];
    }

    let sr = 48_000.0_f32;
    let nyq = sr * 0.49;
    let f_min = 20.0_f32;

    let p = SpectralPhaserParams {
        width: params[0],
        offset: params[1],
        depth: params[2],
        speed: params[3],
        mix: params[4],
        blur: params[5],
        hz_per_oct: params[6],
        times_five: params[7],
        keep_bass: params[8],
        blocksize: params[9],
        bypass: params[10],
    };

    let mut phaser = SpectralPhaser::new(sr);
    phaser.set_params(p);

    let rate = phaser.lfo_rate_hz();
    let t = scope_tick() as f32 / SCOPE_FPS;
    let phase = std::f32::consts::TAU * rate * t;
    phaser.set_analysis_cutoff(phaser.cutoff_at_phase(phase));

    let wet = (params[4] / 100.0).clamp(0.0, 1.0);
    let sections = phaser.num_sections().max(1) as f32;
    let mut db = Vec::with_capacity(SCOPE_BINS);
    for i in 0..SCOPE_BINS {
        let t = (i as f32 + 0.5) / SCOPE_BINS as f32;
        let f = (f_min * (nyq / f_min).powf(t)).clamp(20.0, nyq);
        let cascade = phaser.magnitude_response(f);
        let mag = ((1.0 - wet) + wet * cascade).max(1.0e-9);
        db.push(20.0 * mag.log10() / sections);
    }

    let min_db = db.iter().copied().fold(f32::INFINITY, f32::min);
    let max_db = db.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let span = (max_db - min_db).max(1.0);
    db.into_iter()
        .map(|value| ((value - min_db) / span).clamp(0.0, 1.0))
        .collect()
}

pub fn render_module(ui: &mut egui::Ui, app: &mut AppState, track_idx: usize, slot_idx: usize) {
    app.wavetable_viewport.spawn_connect(Some(ui.ctx().clone()));
    let mut params = get_params(track_idx, slot_idx);
    let mut changed = false;

    let body_rect = ui.available_rect_before_wrap();
    let body_h = body_rect.height().max(96.0);
    let left_w = 176.0;
    let sep_x = body_rect.min.x + left_w + 4.0;
    let right_x0 = sep_x + 5.0;
    let right_w = (body_rect.max.x - right_x0).max(200.0);

    let left_rect = egui::Rect::from_min_size(body_rect.min, egui::vec2(left_w, body_h));
    let right_rect = egui::Rect::from_min_size(
        egui::pos2(right_x0, body_rect.min.y),
        egui::vec2(right_w, body_h),
    );

    // Left Column: WGPU Biquad Cascade Scope (`render_phaser_scope`)
    let mut left_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(left_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    left_ui.spacing_mut().item_spacing.y = 2.0;
    left_ui.label(egui::RichText::new("Spectral Scope (WGPU)").size(9.5).color(theme::TEXT_MUTED));
    let scope_h = (left_ui.available_height() - 2.0).clamp(76.0, 100.0);
    let (rect, _) = left_ui.allocate_exact_size(egui::vec2(170.0, scope_h), egui::Sense::hover());
    let painter = left_ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, egui::Color32::from_rgb(0x0B, 0x0C, 0x0F));

    let magnitudes = compute_phaser_scope_magnitudes(params);
    if let Some(scope_img) = app.wavetable_viewport.render_phaser_scope(&magnitudes) {
        let tex = left_ui.ctx().load_texture(
            format!("osp_scope_{track_idx}_{slot_idx}"),
            (*scope_img).clone(),
            egui::TextureOptions::LINEAR,
        );
        painter.image(
            tex.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    } else {
        let bar_count = magnitudes.len().max(1);
        let bar_w = (rect.width() - 4.0) / bar_count as f32;
        for (i, &frac) in magnitudes.iter().enumerate() {
            let bh = frac.clamp(0.05, 1.0) * (rect.height() - 8.0);
            let bx = rect.min.x + 2.0 + i as f32 * bar_w;
            let by = rect.max.y - 4.0 - bh;
            painter.rect_filled(
                egui::Rect::from_min_size(egui::pos2(bx, by), egui::vec2((bar_w - 0.8).max(1.0), bh)),
                1.0,
                theme::ACCENT_PRIMARY,
            );
        }
    }
    painter.rect_stroke(
        rect,
        3.0,
        egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0x2D, 0x30, 0x38)),
        egui::StrokeKind::Inside,
    );

    // Divider
    ui.painter().line_segment(
        [
            egui::pos2(sep_x, body_rect.min.y + 2.0),
            egui::pos2(sep_x, body_rect.max.y - 2.0),
        ],
        egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0x1E, 0x20, 0x25)),
    );

    // Right Column: Controls
    let mut right_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(right_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    right_ui.spacing_mut().item_spacing.y = 4.0;

    right_ui.horizontal(|ui| {
        ui.label(egui::RichText::new("OpenSpectral Phaser").size(10.5).strong().color(theme::ACCENT_PRIMARY));
        let bypass = params[10] >= 0.5;
        if ui.selectable_label(bypass, "BYPASS").clicked() {
            params[10] = if bypass { 0.0 } else { 1.0 };
            changed = true;
        }
    });

    // GPU Rotary Knobs Grid (6 primary parameters) with fixed cells
    let knobs_row_h = 42.0;
    let (knobs_row_rect, _) =
        right_ui.allocate_exact_size(egui::vec2(right_w, knobs_row_h), egui::Sense::hover());
    let col_w = (right_w / 6.0).floor();

    for (i, name) in PARAM_NAMES.iter().take(6).enumerate() {
        let cell_rect = egui::Rect::from_min_size(
            egui::pos2(knobs_row_rect.min.x + i as f32 * col_w, knobs_row_rect.min.y),
            egui::vec2(col_w, knobs_row_h),
        );
        let mut cell_ui = right_ui.new_child(
            egui::UiBuilder::new()
                .max_rect(cell_rect)
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );
        cell_ui.spacing_mut().item_spacing.y = 1.0;

        let min_v = param_min(i);
        let max_v = param_max(i);
        let bipolar = min_v < 0.0;
        let mut val = params[i];
        let (k_resp, k_changed) = crate::ui::bitwig_kit::rotary_knob(
            &mut cell_ui,
            26.0,
            &mut val,
            min_v..=max_v,
            DEFAULTS[i],
            if bipolar {
                theme::BW_LCD_CYAN
            } else {
                theme::ACCENT_ORANGE
            },
            bipolar,
        );
        k_resp.on_hover_text(format!("{name}: {val:.1}"));
        if k_changed {
            params[i] = val;
            changed = true;
        }
        cell_ui.label(
            egui::RichText::new(*name)
                .font(crate::ui::fonts::semibold(8.5))
                .color(theme::TEXT_SECONDARY),
        );
    }

    ui.allocate_rect(body_rect, egui::Sense::hover());

    if changed {
        for (i, &v) in params.iter().enumerate() {
            set_param(track_idx, slot_idx, i, v);
        }
    }
}

pub fn render_module_footer(_ui: &mut egui::Ui, _app: &mut AppState, _track_idx: usize, _slot_idx: usize) {}

pub fn render_module_simple(ui: &mut egui::Ui) {
    ui.label("OpenSpectral Phaser");
}
