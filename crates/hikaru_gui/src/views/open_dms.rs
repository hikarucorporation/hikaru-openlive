/*
 * Copyright (C) Hikaru Corporation - 2026
 * Hikaru OpenLive - Hikaru OpenDMS (Drum Machine Sampler)
 * License: AGPL-3.0-or-later
 */

use egui::{Ui, RichText, Color32, Frame, Stroke, Vec2, Sense, Slider};
use super::open_dms_sampler::{self, DmsSampler};

const MIDI_NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

pub fn midi_note_name(note: u8) -> String {
    let octave = (note / 12) as i8 - 1;
    let name_idx = (note % 12) as usize;
    format!("{}{}", MIDI_NOTE_NAMES[name_idx], octave)
}

#[derive(Clone, Debug)]
pub struct DmsPad {
    pub id: usize,
    pub midi_note: u8,
    pub name: String,
    pub sample_path: Option<String>,
    pub volume: f32,
    pub pan: f32,
    pub pitch: f32,
    pub mute: bool,
    pub solo: bool,
}

impl DmsPad {
    pub fn new(id: usize, midi_note: u8) -> Self {
        Self {
            id,
            midi_note,
            name: format!("Pad {:02}", id + 1),
            sample_path: None,
            volume: 1.0,
            pan: 0.0,
            pitch: 0.0,
            mute: false,
            solo: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct OpenDms {
    pub pads: Vec<DmsPad>,
    pub selected_pad: usize,
    pub pad_count: usize,
    pub sampler: DmsSampler,
}

impl Default for OpenDms {
    fn default() -> Self {
        let pad_count = 16;
        let mut pads = Vec::with_capacity(64);
        for i in 0..64 {
            pads.push(DmsPad::new(i, 36 + i as u8));
        }
        Self {
            pads,
            selected_pad: 0,
            pad_count,
            sampler: DmsSampler::default(),
        }
    }
}

pub fn render_dms_ui(ui: &mut Ui, dms: &mut OpenDms, project_bpm: f32) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("OpenDMS").strong().color(Color32::from_rgb(0, 255, 200)));
                ui.separator();
                ui.label(RichText::new("Grid:").small());
                if ui.selectable_label(dms.pad_count == 16, "16").clicked() {
                    dms.pad_count = 16;
                }
                if ui.selectable_label(dms.pad_count == 32, "32").clicked() {
                    dms.pad_count = 32;
                }
                if ui.selectable_label(dms.pad_count == 64, "64").clicked() {
                    dms.pad_count = 64;
                }
            });

            ui.add_space(6.0);

            let cols = if dms.pad_count == 16 { 4 } else { 8 };
            let rows = dms.pad_count / cols;
            let pad_size = if dms.pad_count == 16 { 52.0 } else { 42.0 };

            egui::Grid::new("dms_pads_grid")
                .spacing([3.0, 3.0])
                .show(ui, |ui| {
                    for row in 0..rows {
                        for col in 0..cols {
                            let pad_idx = row * cols + col;
                            if pad_idx < dms.pad_count {
                                let is_selected = pad_idx == dms.selected_pad;
                                let pad = &mut dms.pads[pad_idx];

                                let bg_color = if is_selected {
                                    Color32::from_rgb(0, 180, 180)
                                } else if pad.sample_path.is_some() {
                                    Color32::from_rgb(50, 55, 65)
                                } else {
                                    Color32::from_rgb(28, 28, 32)
                                };

                                let border = if is_selected {
                                    Color32::from_rgb(0, 255, 255)
                                } else if pad.sample_path.is_some() {
                                    Color32::from_rgb(80, 80, 90)
                                } else {
                                    Color32::from_rgb(50, 50, 55)
                                };

                                let (rect, response) =
                                    ui.allocate_exact_size(Vec2::new(pad_size, pad_size), Sense::click());

                                if response.clicked() {
                                    dms.selected_pad = pad_idx;
                                    dms.sampler = DmsSampler {
                                        sample_path: pad.sample_path.clone(),
                                        ..dms.sampler.clone()
                                    };
                                }

                                ui.allocate_ui_at_rect(rect, |ui| {
                                    Frame::none()
                                        .fill(bg_color)
                                        .stroke(Stroke::new(1.0_f32, border))
                                        .inner_margin(2.0)
                                        .show(ui, |ui| {
                                            ui.set_max_size(Vec2::new(pad_size, pad_size));
                                            ui.vertical(|ui| {
                                                ui.horizontal(|ui| {
                                                    ui.label(
                                                        RichText::new(format!("{:02}", pad_idx + 1))
                                                            .size(9.0)
                                                            .color(Color32::GRAY),
                                                    );
                                                    ui.with_layout(
                                                        egui::Layout::right_to_left(egui::Align::TOP),
                                                        |ui| {
                                                            ui.label(
                                                                RichText::new(midi_note_name(pad.midi_note))
                                                                    .size(8.0)
                                                                    .color(Color32::DARK_GRAY),
                                                            );
                                                        },
                                                    );
                                                });
                                                ui.centered_and_justified(|ui| {
                                                    let display_name = if pad.sample_path.is_some() {
                                                        &pad.name
                                                    } else {
                                                        "Empty"
                                                    };
                                                    ui.label(
                                                        RichText::new(display_name)
                                                            .size(9.0)
                                                            .strong(),
                                                    );
                                                });
                                            });
                                        });
                                });
                            }
                        }
                        ui.end_row();
                    }
                });
        });

        ui.separator();

        if let Some(pad) = dms.pads.get_mut(dms.selected_pad) {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "PAD {:02} [{}]",
                            pad.id + 1,
                            midi_note_name(pad.midi_note)
                        ))
                        .strong()
                        .color(Color32::from_rgb(0, 255, 255)),
                    );
                    if ui.button(RichText::new("Load Sample").small()).clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Audio", &["wav", "mp3", "ogg", "flac"])
                            .pick_file()
                        {
                            let path_str = path.to_string_lossy().to_string();
                            pad.sample_path = Some(path_str.clone());
                            pad.name = path
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            dms.sampler.sample_path = Some(path_str);
                        }
                    }
                });

                ui.separator();

                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Gain").size(10.0));
                        ui.add(Slider::new(&mut pad.volume, 0.0..=2.0).show_value(false));
                    });
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Pan").size(10.0));
                        ui.add(Slider::new(&mut pad.pan, -1.0..=1.0).show_value(false));
                    });
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Pitch").size(10.0));
                        ui.add(Slider::new(&mut pad.pitch, -24.0..=24.0).show_value(false));
                    });
                });

                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.toggle_value(
                        &mut pad.mute,
                        RichText::new("M").small().strong(),
                    );
                    ui.toggle_value(
                        &mut pad.solo,
                        RichText::new("S").small().strong(),
                    );
                });

                ui.add_space(6.0);
                ui.separator();

                open_dms_sampler::render_sampler_ui(ui, &mut dms.sampler, project_bpm);
            });
        }
    });
}
