/*
 * Hikaru OpenLive - Hikaru OpenDMS Sampler (Slicex / Stretch Engine)
 * License: AGPL-3.0-or-later
 */

use egui::{Ui, RichText, Color32, Stroke, Vec2, Slider, Sense, Pos2};

#[derive(Clone, Debug)]
pub struct AdsrEnvelope {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

impl Default for AdsrEnvelope {
    fn default() -> Self {
        Self {
            attack: 0.0,
            decay: 100.0,
            sustain: 1.0,
            release: 50.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DmsSampler {
    pub sample_path: Option<String>,
    pub sample_bpm: f32,
    pub sync_tempo: bool,
    pub pitch_cents: f32,
    pub start_pos: f32,
    pub end_pos: f32,
    pub adsr: AdsrEnvelope,
    pub slices: Vec<f32>,
    pub selected_slice: usize,
}

impl Default for DmsSampler {
    fn default() -> Self {
        Self {
            sample_path: None,
            sample_bpm: 120.0,
            sync_tempo: true,
            pitch_cents: 0.0,
            start_pos: 0.0,
            end_pos: 1.0,
            adsr: AdsrEnvelope::default(),
            slices: vec![0.0, 0.25, 0.5, 0.75],
            selected_slice: 0,
        }
    }
}

pub fn render_sampler_ui(ui: &mut Ui, sampler: &mut DmsSampler, project_bpm: f32) {
    ui.spacing_mut().item_spacing = Vec2::splat(2.0);

    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            if ui
                .button(RichText::new("Load WAV").small().strong())
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Audio", &["wav", "mp3", "ogg", "flac"])
                    .pick_file()
                {
                    sampler.sample_path = Some(path.to_string_lossy().to_string());
                }
            }

            ui.separator();

            let sync_color = if sampler.sync_tempo {
                Color32::from_rgb(0, 255, 200)
            } else {
                Color32::GRAY
            };
            if ui
                .selectable_label(
                    sampler.sync_tempo,
                    RichText::new("Sync Tempo").small().color(sync_color),
                )
                .clicked()
            {
                sampler.sync_tempo = !sampler.sync_tempo;
            }

            ui.label(RichText::new("BPM:").size(9.0));
            ui.add(
                egui::DragValue::new(&mut sampler.sample_bpm)
                    .speed(0.1)
                    .clamp_range(40.0..=300.0),
            );

            if sampler.sync_tempo && sampler.sample_bpm > 0.0 {
                let ratio = project_bpm / sampler.sample_bpm;
                ui.label(
                    RichText::new(format!("Ratio: {:.2}x", ratio))
                        .size(9.0)
                        .color(Color32::YELLOW),
                );
            }
        });

        let canvas_size = Vec2::new(ui.available_width(), 48.0);
        let (response, painter) = ui.allocate_painter(canvas_size, Sense::click_and_drag());
        let rect = response.rect;

        painter.rect_filled(rect, 2.0, Color32::from_rgb(18, 18, 22));
        painter.rect_stroke(rect, 2.0, Stroke::new(1.0_f32, Color32::from_gray(50)));

        let center_y = rect.center().y;
        let points_count = 120;
        for i in 0..points_count {
            let x = rect.left() + (i as f32 / points_count as f32) * rect.width();
            let amp = ((i as f32 * 0.3).sin() * 16.0).abs();
            painter.line_segment(
                [
                    Pos2::new(x, center_y - amp),
                    Pos2::new(x, center_y + amp),
                ],
                Stroke::new(1.0_f32, Color32::from_rgb(0, 200, 255)),
            );
        }

        for (idx, &slice_pos) in sampler.slices.iter().enumerate() {
            let slice_x = rect.left() + slice_pos * rect.width();
            let is_sel = idx == sampler.selected_slice;
            let c = if is_sel {
                Color32::YELLOW
            } else {
                Color32::LIGHT_RED
            };

            painter.line_segment(
                [
                    Pos2::new(slice_x, rect.top()),
                    Pos2::new(slice_x, rect.bottom()),
                ],
                Stroke::new(if is_sel { 2.0_f32 } else { 1.0_f32 }, c),
            );

            painter.text(
                Pos2::new(slice_x + 2.0, rect.top() + 1.0),
                egui::Align2::LEFT_TOP,
                format!("S{}", idx + 1),
                egui::FontId::proportional(7.0),
                c,
            );
        }

        if response.clicked() {
            let mouse_x = response.interact_pointer_pos.unwrap_or(rect.left_top()).x;
            let rel = ((mouse_x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            let mut closest = 0;
            let mut min_dist = f32::MAX;
            for (i, &s) in sampler.slices.iter().enumerate() {
                let d = (s - rel).abs();
                if d < min_dist {
                    min_dist = d;
                    closest = i;
                }
            }
            sampler.selected_slice = closest;
        }

        ui.add_space(2.0);

        ui.horizontal(|ui| {
            ui.group(|ui| {
                ui.label(
                    RichText::new("Slices")
                        .small()
                        .strong()
                        .color(Color32::from_rgb(0, 255, 200)),
                );
                if ui.button(RichText::new("Auto").small()).clicked() {}
                if ui.button(RichText::new("Clear").small()).clicked() {
                    sampler.slices.clear();
                }
            });

            ui.separator();

            ui.group(|ui| {
                ui.label(
                    RichText::new("ADSR")
                        .small()
                        .strong()
                        .color(Color32::from_rgb(0, 255, 255)),
                );
                ui.horizontal(|ui| {
                    for (label, val, range) in [
                        ("A", &mut sampler.adsr.attack, 0.0_f32..=500.0),
                        ("D", &mut sampler.adsr.decay, 0.0..=1000.0),
                        ("S", &mut sampler.adsr.sustain, 0.0..=1.0),
                        ("R", &mut sampler.adsr.release, 0.0..=1000.0),
                    ] {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(label).size(8.0));
                            ui.add(Slider::new(val, range).show_value(false));
                        });
                    }
                });
            });

            ui.separator();

            ui.group(|ui| {
                ui.label(
                    RichText::new("Pitch")
                        .small()
                        .strong()
                        .color(Color32::LIGHT_GREEN),
                );
                ui.add(
                    Slider::new(&mut sampler.pitch_cents, -1200.0..=1200.0)
                        .show_value(false),
                );
            });
        });
    });
}
