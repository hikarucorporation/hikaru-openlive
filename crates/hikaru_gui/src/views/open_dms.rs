use std::path::PathBuf;

use gpui_kit::component::*;
use gpui_kit::component::button::Button;
use gpui_kit::component::label::Label;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, HikaruApp};
use crate::audio_proxy::{AudioProxy, GuiCommand};
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

fn pad_color(pad: &DmsPad, is_selected: bool) -> (Hsla, Hsla) {
    let bg = if is_selected {
        rgb(0x00A0A0).into()
    } else if pad.sample_path.is_some() {
        rgb(0x2D323A).into()
    } else {
        rgb(0x1A1A1E).into()
    };
    let border = if is_selected {
        rgb(0x00FFFF).into()
    } else if pad.sample_path.is_some() {
        rgb(0x464650).into()
    } else {
        rgb(0x2D2D32).into()
    };
    (bg, border)
}

pub fn render_dms_compact(
    cx: &mut Context<HikaruApp>,
    dms: &OpenDms,
    dragged_sample: &Option<PathBuf>,
    audio_proxy: &AudioProxy,
) -> AnyElement {
    let pad_count = dms.pad_count;
    let selected = dms.selected_pad;
    let cols = if pad_count <= 16 { 4 } else { 8 };
    let rows = pad_count / cols;
    let pad_size: f32 = match pad_count {
        16 => 46.0,
        32 => 40.0,
        _ => 36.0,
    };

    let mut pad_elems: Vec<AnyElement> = Vec::new();
    for row in 0..rows {
        for col in 0..cols {
            let pad_idx = row * cols + col;
            if pad_idx >= pad_count {
                continue;
            }
            let pad = &dms.pads[pad_idx];
            let is_sel = pad_idx == selected;
            let (bg, border) = pad_color(pad, is_sel);
            let has_sample = pad.sample_path.is_some();
            let note = midi_note_name(pad.midi_note);
            let label = if has_sample {
                pad.name.clone()
            } else {
                note.clone()
            };
            let audio_proxy = audio_proxy.clone();
            let dragged = dragged_sample.clone();

            pad_elems.push(
                div()
                    .w(px(pad_size))
                    .h(px(pad_size))
                    .bg(bg)
                    .border_1()
                    .border_color(border)
                    .rounded(px(3.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Label::new(label).text_xs())
                    .id(format!("dms_pad_{}", pad_idx))
                    .on_click(move |_, _, cx| {
                        let st = state(cx);
                        st.update(cx, |state, cx| {
                            if let Some(track) = state.live_tracks.get_mut(state.selected_track_index) {
                                if let Some(slot) = track.effects.iter_mut().find(|s| s.name == "Hikaru OpenDMS") {
                                    if let Some(dms) = slot.dms_state.as_mut() {
                                        dms.selected_pad = pad_idx;
                                        if dms.pads[pad_idx].sample_path.is_some() {
                                            let p = &dms.pads[pad_idx];
                                            let adsr = &dms.sampler.adsr;
                                            let speed = 2.0_f32.powf(p.pitch / 12.0);
                                            state.audio_proxy.send(GuiCommand::DmsNoteOn {
                                                pad_idx,
                                                gain: p.volume,
                                                pan: p.pan,
                                                velocity: 1.0,
                                                play_speed: speed as f64,
                                                attack_ms: adsr.attack,
                                                decay_ms: adsr.decay,
                                                sustain: adsr.sustain,
                                                release_ms: adsr.release,
                                            });
                                        }
                                    }
                                }
                            }
                            cx.notify();
                        });
                    })
                    .into_any_element(),
            );
        }
    }

    v_flex()
        .gap(px(2.0))
        .child(
            h_flex()
                .gap(px(4.0))
                .child(Label::new("OpenDMS").text_xs().font_weight(FontWeight::BOLD).text_color(rgb(0x00FFC8)))
                .child(
                    Button::new("dms_16")
                        .label("16")
                        .compact()
                        .when(pad_count == 16, |b| b.text_color(rgb(0x00FFFF)))
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            st.update(cx, |state, cx| {
                                if let Some(track) = state.live_tracks.get_mut(state.selected_track_index) {
                                    if let Some(slot) = track.effects.iter_mut().find(|s| s.name == "Hikaru OpenDMS") {
                                        if let Some(dms) = slot.dms_state.as_mut() {
                                            dms.pad_count = 16;
                                        }
                                    }
                                }
                                cx.notify();
                            });
                        }),
                )
                .child(
                    Button::new("dms_32")
                        .label("32")
                        .compact()
                        .when(pad_count == 32, |b| b.text_color(rgb(0x00FFFF)))
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            st.update(cx, |state, cx| {
                                if let Some(track) = state.live_tracks.get_mut(state.selected_track_index) {
                                    if let Some(slot) = track.effects.iter_mut().find(|s| s.name == "Hikaru OpenDMS") {
                                        if let Some(dms) = slot.dms_state.as_mut() {
                                            dms.pad_count = 32;
                                        }
                                    }
                                }
                                cx.notify();
                            });
                        }),
                )
                .child(
                    Button::new("dms_64")
                        .label("64")
                        .compact()
                        .when(pad_count == 64, |b| b.text_color(rgb(0x00FFFF)))
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            st.update(cx, |state, cx| {
                                if let Some(track) = state.live_tracks.get_mut(state.selected_track_index) {
                                    if let Some(slot) = track.effects.iter_mut().find(|s| s.name == "Hikaru OpenDMS") {
                                        if let Some(dms) = slot.dms_state.as_mut() {
                                            dms.pad_count = 64;
                                        }
                                    }
                                }
                                cx.notify();
                            });
                        }),
                ),
        )
        .child(
            div()
                .w(px(pad_size * cols as f32 + (cols - 1) as f32 * 2.0))
                .h(px(pad_size * rows as f32 + (rows - 1) as f32 * 2.0))
                .grid()
                .grid_cols(cols as u16)
                .gap(px(2.0))
                .children(pad_elems),
        )
        .into_any_element()
}

pub fn render_dms_ui(
    cx: &mut Context<HikaruApp>,
    dms: &mut OpenDms,
    project_bpm: f32,
    dragged_sample: &mut Option<PathBuf>,
    audio_proxy: &AudioProxy,
) {
    let _ = (cx, dms, project_bpm, dragged_sample, audio_proxy);
}
