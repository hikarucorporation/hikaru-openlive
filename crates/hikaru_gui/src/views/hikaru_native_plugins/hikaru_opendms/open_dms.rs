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

pub fn render_dms_ui(
    ui: &mut egui::Ui,
    dms: &mut OpenDms,
    _project_bpm: f32,
    _dragged_sample: &mut Option<PathBuf>,
    _audio_proxy: &AudioProxy,
) {
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Hikaru OpenDMS").heading().color(theme::ACCENT_PRIMARY));
            ui.separator();
            for count in [16, 32, 64] {
                if ui.selectable_label(dms.pad_count == count, format!("{} Pads", count)).clicked() {
                    dms.pad_count = count;
                }
            }
        });

        ui.add_space(8.0);

        // Pads Grid
        let cols = 4;
        let rows = (dms.pad_count / cols).max(1);
        egui::Grid::new("dms_pads_grid")
            .spacing(egui::vec2(6.0, 6.0))
            .show(ui, |ui| {
                for r in 0..rows {
                    for c in 0..cols {
                        let pad_idx = (rows - 1 - r) * cols + c;
                        if let Some(pad) = dms.pads.get_mut(pad_idx) {
                            let _is_selected = dms.selected_pad == pad_idx;
                            let text = format!("{}\n{}", pad.name, midi_note_name(pad.midi_note));
                            let btn = egui::Button::new(text)
                                .min_size(egui::vec2(60.0, 48.0));
                            if ui.add(btn).clicked() {
                                dms.selected_pad = pad_idx;
                            }
                        }
                    }
                    ui.end_row();
                }
            });
    });
}
