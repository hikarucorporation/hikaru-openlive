// La grilla de pads del sampler Hikaru OpenDMS.
use std::path::PathBuf;

use crate::audio_proxy::AudioProxy;
use crate::theme;
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
    pub active_bank: usize,
    pub sampler: DmsSampler,
}

impl Default for OpenDms {
    fn default() -> Self {
        let pad_count = 16;
        let mut pads = Vec::with_capacity(144);
        for i in 0..144 {
            pads.push(DmsPad::new(i, (i.min(127)) as u8));
        }
        Self {
            pads,
            selected_pad: 0,
            pad_count,
            active_bank: 0,
            sampler: DmsSampler::default(),
        }
    }
}

pub fn render_dms_ui(
    ui: &mut egui::Ui,
    dms: &mut OpenDms,
    _project_bpm: f32,
    _dragged_sample: &mut Option<PathBuf>,
    _audio_proxy: &AudioProxy,
) {
    let frame_fill = egui::Color32::from_rgb(38, 38, 40);
    let pad_fill = egui::Color32::from_rgb(70, 70, 74);
    let pad_hover_fill = egui::Color32::from_rgb(84, 84, 88);
    let pad_border = egui::Color32::from_rgb(32, 32, 34);
    let text_note = egui::Color32::from_rgb(200, 200, 205);
    let text_plus = egui::Color32::from_rgb(160, 160, 165);

    let frame_resp = egui::Frame::NONE
        .fill(frame_fill)
        .corner_radius(4.0)
        .inner_margin(egui::Margin::same(4))
        .show(ui, |ui| {
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                // Izquierda: barra vertical de 9 mini-drumpads (scroll de bancos)
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 1.0);
                    let num_banks = 9usize;
                    for rev_b in 0..num_banks {
                        let bank = num_banks - 1 - rev_b;
                        let is_active = dms.active_bank == bank;
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::click());
                        if response.clicked() {
                            dms.active_bank = bank;
                        }
                        let painter = ui.painter_at(rect);
                        let bg = if is_active {
                            egui::Color32::from_rgb(90, 90, 96)
                        } else if response.hovered() {
                            egui::Color32::from_rgb(60, 60, 64)
                        } else {
                            egui::Color32::from_rgb(46, 46, 48)
                        };
                        painter.rect_filled(rect, 2.0, bg);
                        if is_active {
                            painter.rect_stroke(
                                rect,
                                2.0,
                                egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(180, 180, 185)),
                                egui::StrokeKind::Inside,
                            );
                        }

                        // Mini grilla 4x4 dentro de cada banco
                        let cell = 2.0;
                        let gap = 1.0;
                        let start_x = rect.min.x + 1.5;
                        let start_y = rect.min.y + 1.5;
                        for mr in 0..4 {
                            for mc in 0..4 {
                                let pad_idx = bank * 16 + (3 - mr) * 4 + mc;
                                let has_sample = dms
                                    .pads
                                    .get(pad_idx)
                                    .map(|p| p.sample_path.is_some())
                                    .unwrap_or(false);
                                let dot_color = if has_sample {
                                    theme::ACCENT_PRIMARY
                                } else if is_active {
                                    egui::Color32::from_rgb(150, 150, 156)
                                } else {
                                    egui::Color32::from_rgb(76, 76, 80)
                                };
                                let dot_rect = egui::Rect::from_min_size(
                                    egui::pos2(
                                        start_x + mc as f32 * (cell + gap),
                                        start_y + mr as f32 * (cell + gap),
                                    ),
                                    egui::vec2(cell, cell),
                                );
                                painter.rect_filled(dot_rect, 0.5, dot_color);
                            }
                        }
                    }
                });

                ui.add_space(6.0);

                // Centro: grilla de 16 pads (4x4)
                let cols = 4usize;
                let rows = 4usize;
                let base_idx = dms.active_bank * 16;
                egui::Grid::new("dms_pads_grid")
                    .spacing(egui::vec2(3.0, 3.0))
                    .show(ui, |ui| {
                        for r in 0..rows {
                            for c in 0..cols {
                                let pad_idx = base_idx + (rows - 1 - r) * cols + c;
                                let (rect, response) = ui.allocate_exact_size(
                                    egui::vec2(58.0, 26.0),
                                    egui::Sense::click(),
                                );
                                if response.clicked() {
                                    dms.selected_pad = pad_idx;
                                }

                                let is_selected = dms.selected_pad == pad_idx;
                                let painter = ui.painter_at(rect);
                                let fill = if response.hovered() {
                                    pad_hover_fill
                                } else {
                                    pad_fill
                                };
                                painter.rect_filled(rect, 3.0, fill);

                                let stroke = if is_selected {
                                    egui::Stroke::new(1.5_f32, theme::ACCENT_PRIMARY)
                                } else {
                                    egui::Stroke::new(1.0_f32, pad_border)
                                };
                                painter.rect_stroke(rect, 3.0, stroke, egui::StrokeKind::Inside);

                                if let Some(pad) = dms.pads.get(pad_idx) {
                                    // Nombre de nota arriba a la izquierda (ej. C-1, C-2)
                                    let note_str = midi_note_name(pad.midi_note);
                                    painter.text(
                                        rect.min + egui::vec2(5.0, 4.0),
                                        egui::Align2::LEFT_TOP,
                                        note_str,
                                        egui::FontId::proportional(11.0),
                                        text_note,
                                    );

                                    // Nombre del sample si está cargado
                                    if pad.sample_path.is_some() {
                                        painter.text(
                                            rect.min + egui::vec2(5.0, 18.0),
                                            egui::Align2::LEFT_TOP,
                                            pad.display_filename(),
                                            egui::FontId::proportional(9.5),
                                            theme::ACCENT_PRIMARY,
                                        );
                                    }

                                    // Símbolo '+' abajo a la derecha
                                    painter.text(
                                        rect.max - egui::vec2(6.0, 4.0),
                                        egui::Align2::RIGHT_BOTTOM,
                                        "+",
                                        egui::FontId::proportional(12.0),
                                        text_plus,
                                    );
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
        });

    if frame_resp.response.hovered() || ui.rect_contains_pointer(frame_resp.response.rect) {
        let scroll_y = ui.input(|i| i.raw_scroll_delta.y);
        if scroll_y > 0.0 {
            dms.active_bank = (dms.active_bank + 1).min(8);
        } else if scroll_y < 0.0 {
            dms.active_bank = dms.active_bank.saturating_sub(1);
        }
    }
}
