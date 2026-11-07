// El sampler de Hikaru OpenDMS

#[derive(Clone, Copy, Debug, PartialEq)]
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

pub fn render_sampler_ui(
    ui: &mut egui::Ui,
    sampler: &mut DmsSampler,
    _project_bpm: f32,
    _dragged_sample: &mut Option<std::path::PathBuf>,
    _peaks: &[f32],
) {
    ui.horizontal(|ui| {
        if ui.button("Load WAV").clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Audio", &["wav", "mp3", "ogg", "flac"])
                .pick_file()
            {
                // File picked
                let _ = path;
            }
        }

        let sync_text = if sampler.sync_tempo { "Sync: ON" } else { "Sync: OFF" };
        if ui.selectable_label(sampler.sync_tempo, sync_text).clicked() {
            sampler.sync_tempo = !sampler.sync_tempo;
        }

        ui.label("BPM:");
        ui.add(egui::DragValue::new(&mut sampler.sample_bpm).speed(0.5).range(20.0..=300.0));
    });
}

pub fn load_peaks_from_wav(path: &str, target_bins: usize) -> Vec<f32> {
    let mut peaks = vec![0.0f32; target_bins];
    if let Ok(mut reader) = hound::WavReader::open(path) {
        let spec = reader.spec();
        let total_samples = reader.len() as usize;
        if total_samples == 0 {
            return peaks;
        }
        let samples_per_bin = (total_samples / target_bins).max(1);
        match spec.sample_format {
            hound::SampleFormat::Int => {
                let max_val = (1 << (spec.bits_per_sample - 1)) as f32;
                let mut iter = reader.samples::<i32>().filter_map(Result::ok);
                for bin in 0..target_bins {
                    let mut max_peak = 0.0f32;
                    for _ in 0..samples_per_bin {
                        if let Some(s) = iter.next() {
                            let v = (s as f32 / max_val).abs();
                            if v > max_peak {
                                max_peak = v;
                            }
                        } else {
                            break;
                        }
                    }
                    peaks[bin] = max_peak;
                }
            }
            hound::SampleFormat::Float => {
                let mut iter = reader.samples::<f32>().filter_map(Result::ok);
                for bin in 0..target_bins {
                    let mut max_peak = 0.0f32;
                    for _ in 0..samples_per_bin {
                        if let Some(s) = iter.next() {
                            let v = s.abs();
                            if v > max_peak {
                                max_peak = v;
                            }
                        } else {
                            break;
                        }
                    }
                    peaks[bin] = max_peak;
                }
            }
        }
    }
    peaks
}
