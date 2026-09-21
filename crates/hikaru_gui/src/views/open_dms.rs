/*
 * Copyright (C) Hikaru Corporation - 2026
 * Hikaru OpenLive - Hikaru OpenDMS (Drum Machine Sampler)
 * License: AGPL-3.0-or-later
 */

use std::path::PathBuf;
use egui::{Align, CursorIcon, RichText, Rounding, Sense, Stroke, Ui, Vec2, Color32};
use super::open_dms_sampler::{self, DmsSampler, ui_knob};
use crate::audio_proxy::{AudioProxy, GuiCommand};

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
    pub waveform_peaks: Vec<f32>,
    cached_peak_path: Option<String>,
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
            waveform_peaks: Vec::new(),
            cached_peak_path: None,
        }
    }

    pub fn load_sample(&mut self, path: String) {
        if self.cached_peak_path.as_deref() == Some(&path) {
            return;
        }
        self.waveform_peaks = open_dms_sampler::load_peaks_from_wav(&path, 512);
        self.cached_peak_path = Some(path.clone());
        self.sample_path = Some(path);
    }

    pub fn display_filename(&self) -> String {
        match &self.sample_path {
            Some(p) => std::path::Path::new(p)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Unknown".to_string()),
            None => "No Sample Loaded".to_string(),
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

fn pad_color(pad: &DmsPad, is_selected: bool) -> (Color32, Color32) {
    let bg = if is_selected {
        Color32::from_rgb(0, 160, 160)
    } else if pad.sample_path.is_some() {
        Color32::from_rgb(45, 50, 58)
    } else {
        Color32::from_rgb(26, 26, 30)
    };
    let border = if is_selected {
        Color32::from_rgb(0, 255, 255)
    } else if pad.sample_path.is_some() {
        Color32::from_rgb(70, 70, 80)
    } else {
        Color32::from_rgb(45, 45, 50)
    };
    (bg, border)
}

pub fn render_dms_ui(
    ui: &mut Ui,
    dms: &mut OpenDms,
    project_bpm: f32,
    dragged_sample: &mut Option<PathBuf>,
    audio_proxy: &AudioProxy,
) {
    ui.horizontal(|ui| {
        // --- COLUMNA IZQUIERDA: GRILLA DE PADS ---
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(2.0);

            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("OpenDMS")
                        .strong()
                        .size(11.0)
                        .color(Color32::from_rgb(0, 255, 200)),
                );
                ui.separator();
                for n in [16, 32, 64] {
                    if ui
                        .selectable_label(
                            dms.pad_count == n,
                            RichText::new(format!("{}", n)).small(),
                        )
                        .clicked()
                    {
                        dms.pad_count = n;
                    }
                }
            });

            ui.add_space(2.0);

            let cols = if dms.pad_count <= 16 { 4 } else { 8 };
            let rows = dms.pad_count / cols;
            let pad_size: f32 = match dms.pad_count {
                16 => 46.0,
                32 => 40.0,
                _ => 36.0,
            };
            let pad_gap = 2.0_f32;

            egui::Grid::new("dms_pads_grid")
                .spacing([pad_gap, pad_gap])
                .show(ui, |ui| {
                    for row in 0..rows {
                        for col in 0..cols {
                            let pad_idx = row * cols + col;
                            if pad_idx >= dms.pad_count {
                                ui.add_space(pad_size);
                                continue;
                            }

                            // Copy rendering data from pad to avoid borrow conflicts
                            let is_selected = pad_idx == dms.selected_pad;
                            let (bg, border) = pad_color(&dms.pads[pad_idx], is_selected);
                            let midi_note = dms.pads[pad_idx].midi_note;
                            let has_sample = dms.pads[pad_idx].sample_path.is_some();
                            let pad_name = dms.pads[pad_idx].name.clone();

                            let desired = Vec2::splat(pad_size);
                            let (rect, response) =
                                ui.allocate_exact_size(desired, Sense::click());

                            // Cursor de mano al hacer hover
                            if response.hovered() {
                                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                            }

                            if response.clicked() {
                                dms.selected_pad = pad_idx;

                                // Trigger polyphonic DMS voice
                                if dms.pads[pad_idx].sample_path.is_some() {
                                    let vol = dms.pads[pad_idx].volume;
                                    let pan = dms.pads[pad_idx].pan;
                                    let pitch_shift = dms.pads[pad_idx].pitch;
                                    let speed = 2.0_f32.powf(pitch_shift / 12.0);
                                    let adsr = &dms.sampler.adsr;
                                    audio_proxy.send(GuiCommand::DmsNoteOn {
                                        pad_idx,
                                        gain: vol,
                                        pan,
                                        velocity: 1.0,
                                        play_speed: speed as f64,
                                        attack_ms: adsr.attack,
                                        decay_ms: adsr.decay,
                                        sustain: adsr.sustain,
                                        release_ms: adsr.release,
                                    });
                                }
                            }

                            // Handle drag-over drop on this pad
                            if dragged_sample.is_some() && response.hovered() && ui.input(|i| i.pointer.any_released()) {
                                if let Some(path) = dragged_sample.take() {
                                    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                                    if matches!(ext.as_str(), "wav" | "mp3" | "flac" | "ogg") {
                                        let stem = path.file_stem()
                                            .map(|s| s.to_string_lossy().to_string())
                                            .unwrap_or_else(|| "Sample".to_string());
                                        dms.pads[pad_idx].name = stem;
                                        let path_str = path.to_string_lossy().to_string();
                                        dms.pads[pad_idx].load_sample(path_str.clone());
                                        audio_proxy.send(GuiCommand::LoadDmsSample {
                                            pad_idx,
                                            path: path_str,
                                        });
                                    }
                                }
                            }

                            // Pintar el pad completo sobre el rect asignado
                            let painter = ui.painter();
                            painter.rect(
                                rect,
                                Rounding::same(3.0),
                                bg,
                                Stroke::new(1.0_f32, border),
                            );

                            // Texto del número (arriba-izquierda)
                            let num_text = format!("{:02}", pad_idx + 1);
                            let num_pos = rect.left_top() + Vec2::new(3.0, 2.0);
                            painter.text(
                                num_pos,
                                egui::Align2::LEFT_TOP,
                                &num_text,
                                egui::FontId::proportional(8.0),
                                Color32::GRAY,
                            );

                            // Texto de la nota MIDI (arriba-derecha)
                            let note_text = midi_note_name(midi_note);
                            let note_pos = rect.right_top() + Vec2::new(-3.0, 2.0);
                            painter.text(
                                note_pos,
                                egui::Align2::RIGHT_TOP,
                                &note_text,
                                egui::FontId::proportional(7.0),
                                Color32::DARK_GRAY,
                            );

                            // Nombre del sample o nada (centro)
                            if has_sample {
                                painter.text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    &pad_name,
                                    egui::FontId::proportional(8.0),
                                    Color32::WHITE,
                                );
                            }
                        }
                        ui.end_row();
                    }
                });
        });

        ui.separator();

        // --- COLUMNA DERECHA: EDITOR DEL PAD SELECCIONADO ---
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(2.0);

            let sel = dms.selected_pad;

            // Header: pad identifier + filename display + Load Sample button
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "PAD {:02} [{}]",
                        sel + 1,
                        midi_note_name(dms.pads[sel].midi_note)
                    ))
                    .strong()
                    .size(11.0)
                    .color(Color32::from_rgb(0, 255, 255)),
                );

                ui.separator();

                // Show loaded filename
                let filename = dms.pads[sel].display_filename();
                let file_color = if dms.pads[sel].sample_path.is_some() {
                    Color32::from_rgb(180, 180, 190)
                } else {
                    Color32::from_rgb(80, 80, 85)
                };
                ui.label(
                    RichText::new(&filename).small().color(file_color),
                );

                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(RichText::new("Load Sample").small()).clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Audio", &["wav", "mp3", "ogg", "flac"])
                            .pick_file()
                        {
                            let stem = path.file_stem()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_else(|| "Sample".to_string());
                            dms.pads[sel].name = stem;
                            let path_str = path.to_string_lossy().to_string();
                            dms.pads[sel].load_sample(path_str.clone());
                            audio_proxy.send(GuiCommand::LoadDmsSample {
                                pad_idx: sel,
                                path: path_str,
                            });
                        }
                    }
                });
            });

            ui.add_space(2.0);

            let pad = &mut dms.pads[sel];
            ui.horizontal(|ui| {
                ui_knob(ui, &mut pad.volume, 0.0..=2.0, "Gain");
                ui_knob(ui, &mut pad.pan, -1.0..=1.0, "Pan");
                ui_knob(ui, &mut pad.pitch, -24.0..=24.0, "Pitch");

                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new("").size(9.0));
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
                });
            });

            ui.add_space(2.0);
            ui.separator();

            // Sampler editor — pass pad's peaks, receive new path if loaded
            let peaks = dms.pads[sel].waveform_peaks.clone();
            let new_path = open_dms_sampler::render_sampler_ui(
                ui,
                &mut dms.sampler,
                project_bpm,
                dragged_sample,
                &peaks,
            );

            if let Some(path) = new_path {
                let stem = std::path::Path::new(&path)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Sample".to_string());
                dms.pads[sel].name = stem;
                dms.pads[sel].load_sample(path.clone());
                audio_proxy.send(GuiCommand::LoadDmsSample {
                    pad_idx: sel,
                    path,
                });
            }
        });
    });
}
