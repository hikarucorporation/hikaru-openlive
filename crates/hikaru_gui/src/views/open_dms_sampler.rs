use gpui_kit::component::*;
use gpui_kit::component::button::Button;
use gpui_kit::component::label::Label;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, HikaruApp};
use crate::audio_proxy::{AudioProxy, GuiCommand};

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
    cx: &mut Context<HikaruApp>,
    sampler: &mut DmsSampler,
    project_bpm: f32,
    dragged_sample: &mut Option<std::path::PathBuf>,
    peaks: &[f32],
) -> AnyElement {
    h_flex()
        .gap(px(4.0))
        .items_center()
        .child(
            Button::new("dms_load_wav")
                .label("Load WAV")
                .compact()
                .on_click(move |_, _, cx| {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Audio", &["wav", "mp3", "ogg", "flac"])
                        .pick_file()
                    {
                        let st = state(cx);
                        st.update(cx, |state, cx| {
                            state.audio_proxy.send(GuiCommand::LoadDmsSample {
                                pad_idx: 0,
                                path: path.to_string_lossy().to_string(),
                            });
                            cx.notify();
                        });
                    }
                }),
        )
        .child(
            Button::new("dms_sync_tempo")
                .label("Sync Tempo")
                .compact()
                .when(sampler.sync_tempo, |b| b.text_color(rgb(0x00FFC8)))
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, cx| {
                        if let Some(track) = state.live_tracks.get_mut(state.selected_track_index) {
                            if let Some(slot) = track.effects.iter_mut().find(|s| s.name == "Hikaru OpenDMS") {
                                if let Some(dms) = slot.dms_state.as_mut() {
                                    dms.sampler.sync_tempo = !dms.sampler.sync_tempo;
                                }
                            }
                        }
                        cx.notify();
                    });
                }),
        )
        .child(Label::new("BPM:").text_xs())
        .child(
            Button::new("dpm_bpm_btn")
                .label(format!("{:.0}", sampler.sample_bpm))
                .compact()
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, cx| {
                        if let Some(track) = state.live_tracks.get_mut(state.selected_track_index) {
                            if let Some(slot) = track.effects.iter_mut().find(|s| s.name == "Hikaru OpenDMS") {
                                if let Some(dms) = slot.dms_state.as_mut() {
                                    dms.sampler.sample_bpm = (dms.sampler.sample_bpm + 1.0).min(300.0);
                                }
                            }
                        }
                        cx.notify();
                    });
                }),
        )
        .into_any_element()
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
