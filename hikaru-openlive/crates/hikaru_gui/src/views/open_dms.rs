use std::path::PathBuf;

use gpui_kit::component::*;
use gpui_kit::component::button::Button;
use gpui_kit::component::label::Label;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, HikaruApp};
use crate::audio_proxy::{AudioProxy, GuiCommand};
use crate::views::controls::{envelope_display, knob_row, param_row, section};
use crate::views::util::normalized;
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
                    Button::new("dms_16").rounded(gpui_kit::component::button::ButtonRounded::None)
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
                    Button::new("dms_32").rounded(gpui_kit::component::button::ButtonRounded::None)
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
                    Button::new("dms_64").rounded(gpui_kit::component::button::ButtonRounded::None)
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

/// Editor extendido del sampler, el que se despliega desde el rack.
///
/// Es la versión grande de [`render_dms_compact`]: la misma grilla de pads más
/// los controles del pad seleccionado (volumen, pan, pitch, mute/solo) y los
/// del sampler (tempo, envolvente, slices). Recibe `track_idx`/`slot_idx` en
/// vez de leer la selección global para que el panel siga siendo correcto si el
/// rack cambia de pista mientras se dibuja.
pub fn render_editor(
    cx: &mut Context<HikaruApp>,
    track_idx: usize,
    slot_idx: usize,
    dragged_sample: &Option<PathBuf>,
    audio_proxy: &AudioProxy,
) -> AnyElement {
    let dms = {
        let app = state(cx).read(cx);
        match app.slot(track_idx, slot_idx).and_then(|slot| slot.dms_state.clone()) {
            Some(dms) => dms,
            None => return not_ready_panel(),
        }
    };

    let pad = dms.pads.get(dms.selected_pad).cloned();
    let sampler = dms.sampler.clone();

    v_flex()
        .id("open_dms_editor")
        .size_full()
        .bg(rgb(0x101116))
        .border_1()
        .border_color(rgb(0x2A2E3A))
        .rounded(px(4.0))
        .gap(px(4.0))
        .p(px(6.0))
        .child(
            h_flex()
                .items_center()
                .gap(px(4.0))
                .child(section("HIKARIU OPENDMS"))
                .child(Label::new(format!("{} pads", dms.pad_count)).text_xs().text_color(rgb(0x8A90A0)))
                .child(div().flex_1())
                .child(
                    Button::new("dms_editor_load")
                        .rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("Load WAV into Pad")
                        .compact()
                        .on_click(move |_, _, cx| {
                            let Some(path) = rfd::FileDialog::new()
                                .add_filter("Audio", &["wav", "mp3", "ogg", "flac"])
                                .pick_file()
                            else {
                                return;
                            };
                            let path = path.to_string_lossy().to_string();
                            let pad_idx = state(cx).read(cx).selected_slot_pad(track_idx, slot_idx);

                            state(cx).update(cx, |state, cx| {
                                // El sample se registra en el motor y también en
                                // el estado de la GUI: el motor no puede
                                // devolver los picos, así que los peaks se
                                // calculan acá para poder dibujar la forma.
                                state.audio_proxy.send(GuiCommand::LoadDmsSample { pad_idx, path: path.clone() });
                                if let Some(dms) = state.slot_mut(track_idx, slot_idx).and_then(|slot| slot.dms_state.as_mut()) {
                                    if let Some(pad) = dms.pads.get_mut(pad_idx) {
                                        pad.load_sample(path);
                                    }
                                }
                                cx.notify();
                            });
                        }),
                ),
        )
        .child(
            h_flex()
                .flex_1()
                .min_h(px(0.0))
                .gap(px(6.0))
                // Centro: la grilla de pads, que es la vista principal.
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(0.0))
                        .gap(px(4.0))
                        .child(render_dms_compact(cx, &dms, dragged_sample, audio_proxy)),
                )
                // Derecha: los controles del pad y del sampler.
                .child(render_pad_controls(cx, track_idx, slot_idx, pad.as_ref()))
                .child(render_sampler_controls(cx, track_idx, slot_idx, &sampler)),
        )
        .into_any_element()
}

/// Panel de transición mientras el estado del sampler todavía no existe.
fn not_ready_panel() -> AnyElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .child(Label::new("Sampler sin estado").text_xs().text_color(rgb(0x6A7080)))
        .into_any_element()
}

/// Controles del pad seleccionado.
fn render_pad_controls(
    cx: &mut Context<HikaruApp>,
    track_idx: usize,
    slot_idx: usize,
    pad: Option<&DmsPad>,
) -> AnyElement {
    let _ = cx;
    let Some(pad) = pad else {
        return v_flex().w(px(180.0)).into_any_element();
    };

    let pad_idx = pad.id;
    let name = pad.display_filename();
    let volume = pad.volume;
    let pan = pad.pan;
    let pitch = pad.pitch;
    let (mute, solo) = (pad.mute, pad.solo);

    v_flex()
        .w(px(180.0))
        .gap(px(3.0))
        .p(px(6.0))
        .bg(rgb(0x16181F))
        .border_1()
        .border_color(rgb(0x242833))
        .rounded(px(3.0))
        .child(section(&format!("PAD {:02}", pad_idx + 1)))
        .child(Label::new(name).text_xs().text_color(rgb(0x8A90A0)))
        // La forma de onda del pad: con 512 picos leídos del WAV, es la única
        // forma de saber qué sample está cargado sin abrir el archivo.
        .child(pad_waveform(&pad.waveform_peaks, rgb(0x00FFC8).into()))
        .child(knob_row(
            &format!("dms_pad_volume_{pad_idx}"),
            "VOL",
            &format!("{volume:.2}"),
            normalized(volume, 0.0, 1.5),
            move |_, _, cx| with_pad(cx, track_idx, slot_idx, pad_idx, |pad| pad.volume = (pad.volume - 0.05).max(0.0)),
            move |_, _, cx| with_pad(cx, track_idx, slot_idx, pad_idx, |pad| pad.volume = (pad.volume + 0.05).min(1.5)),
        ))
        .child(knob_row(
            &format!("dms_pad_pan_{pad_idx}"),
            "PAN",
            &format!("{pan:+.2}"),
            normalized(pan, -1.0, 1.0),
            move |_, _, cx| with_pad(cx, track_idx, slot_idx, pad_idx, |pad| pad.pan = (pad.pan - 0.05).max(-1.0)),
            move |_, _, cx| with_pad(cx, track_idx, slot_idx, pad_idx, |pad| pad.pan = (pad.pan + 0.05).min(1.0)),
        ))
        .child(knob_row(
            &format!("dms_pad_pitch_{pad_idx}"),
            "PITCH",
            &format!("{:+.0} st", pitch),
            normalized(pitch, -24.0, 24.0),
            move |_, _, cx| with_pad(cx, track_idx, slot_idx, pad_idx, |pad| pad.pitch = (pad.pitch - 1.0).max(-24.0)),
            move |_, _, cx| with_pad(cx, track_idx, slot_idx, pad_idx, |pad| pad.pitch = (pad.pitch + 1.0).min(24.0)),
        ))
        .child(
            h_flex()
                .gap(px(2.0))
                .child(
                    Button::new(format!("dms_pad_mute_{pad_idx}"))
                        .rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("MUTE")
                        .compact()
                        .when(mute, |b| b.text_color(rgb(0xFF6464)))
                        .on_click(move |_, _, cx| with_pad(cx, track_idx, slot_idx, pad_idx, |pad| pad.mute = !pad.mute)),
                )
                .child(
                    Button::new(format!("dms_pad_solo_{pad_idx}"))
                        .rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("SOLO")
                        .compact()
                        .when(solo, |b| b.text_color(rgb(0x00FFFF)))
                        .on_click(move |_, _, cx| with_pad(cx, track_idx, slot_idx, pad_idx, |pad| pad.solo = !pad.solo)),
                ),
        )
        .into_any_element()
}

/// Forma de onda del pad, dibujada con `canvas`.
///
/// `peaks` viene del WAV con [`open_dms_sampler::load_peaks_from_wav`]: son
/// máximos absolutos por bin, así que se dibuja como una silueta simétrica
/// alrededor del eje. Un pad sin sample pinta la línea de base: es la
/// diferencia entre "vacío" y "no se está dibujando".
fn pad_waveform(peaks: &[f32], color: Hsla) -> AnyElement {
    let peaks = peaks.to_vec();

    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            // Todo en `f32` y conversión al construir los puntos: `Pixels` no
            // tiene constructor público y mezclarlo con `f32` obliga a
            // convertir en cada operación.
            let left = bounds.origin.x.as_f32();
            let width = bounds.size.width.as_f32().max(1.0);
            let height = bounds.size.height.as_f32().max(1.0);
            let middle = bounds.origin.y.as_f32() + height / 2.0;

            let mut axis = PathBuilder::stroke(px(1.0));
            axis.move_to(point(px(left), px(middle)));
            axis.line_to(point(px(left + width), px(middle)));
            if let Ok(axis) = axis.build() {
                window.paint_path(axis, rgb(0x2A2E3A));
            }

            if peaks.is_empty() {
                return;
            }

            // Un path por arriba y otro por abajo: la silueta se lee mejor que
            // una línea de 512 segmentos que cruza el eje 512 veces.
            for mirror in [false, true] {
                let mut path = PathBuilder::stroke(px(1.0));
                for (index, peak) in peaks.iter().enumerate() {
                    let x = left + width * index as f32 / peaks.len() as f32;
                    let amplitude = height * 0.45 * peak.clamp(0.0, 1.0);
                    let y = if mirror { middle + amplitude } else { middle - amplitude };
                    if index == 0 {
                        path.move_to(point(px(x), px(y)));
                    } else {
                        path.line_to(point(px(x), px(y)));
                    }
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            }
        },
    )
    .w_full()
    .h(px(34.0))
    .into_any_element()
}

/// Controles del sampler: tempo, envolvente y slices.
fn render_sampler_controls(
    cx: &mut Context<HikaruApp>,
    track_idx: usize,
    slot_idx: usize,
    sampler: &DmsSampler,
) -> AnyElement {
    let _ = cx;
    let adsr = sampler.adsr;
    let slice = sampler.selected_slice.min(sampler.slices.len().saturating_sub(1));
    let slice_count = sampler.slices.len();

    v_flex()
        .w(px(196.0))
        .gap(px(3.0))
        .p(px(6.0))
        .bg(rgb(0x16181F))
        .border_1()
        .border_color(rgb(0x242833))
        .rounded(px(3.0))
        .child(section("SAMPLER"))
        .child(param_row(
            "dms_sample_bpm",
            "BPM",
            &format!("{:.0}", sampler.sample_bpm),
            move |_, _, cx| with_sampler(cx, track_idx, slot_idx, |s| s.sample_bpm = (s.sample_bpm - 1.0).max(20.0)),
            move |_, _, cx| with_sampler(cx, track_idx, slot_idx, |s| s.sample_bpm = (s.sample_bpm + 1.0).min(300.0)),
        ))
        .child(
            Button::new("dms_sync_tempo")
                .rounded(gpui_kit::component::button::ButtonRounded::None)
                .label("SYNC TEMPO")
                .compact()
                .when(sampler.sync_tempo, |b| b.text_color(rgb(0x00FFC8)))
                .on_click(move |_, _, cx| with_sampler(cx, track_idx, slot_idx, |s| s.sync_tempo = !s.sync_tempo)),
        )
        .child(section("ENVELOPE"))
        .child(envelope_display(adsr.attack, adsr.decay, adsr.sustain, adsr.release))
        .child(knob_row(
            "dms_adsr_attack",
            "ATK",
            &format!("{:.0} ms", adsr.attack),
            normalized(adsr.attack, 0.0, 2000.0),
            move |_, _, cx| with_sampler(cx, track_idx, slot_idx, |s| s.adsr.attack = (s.adsr.attack - 5.0).max(0.0)),
            move |_, _, cx| with_sampler(cx, track_idx, slot_idx, |s| s.adsr.attack = (s.adsr.attack + 5.0).min(2000.0)),
        ))
        .child(knob_row(
            "dms_adsr_decay",
            "DEC",
            &format!("{:.0} ms", adsr.decay),
            normalized(adsr.decay, 0.0, 2000.0),
            move |_, _, cx| with_sampler(cx, track_idx, slot_idx, |s| s.adsr.decay = (s.adsr.decay - 5.0).max(0.0)),
            move |_, _, cx| with_sampler(cx, track_idx, slot_idx, |s| s.adsr.decay = (s.adsr.decay + 5.0).min(2000.0)),
        ))
        .child(knob_row(
            "dms_adsr_sustain",
            "SUS",
            &format!("{:.2}", adsr.sustain),
            normalized(adsr.sustain, 0.0, 1.0),
            move |_, _, cx| with_sampler(cx, track_idx, slot_idx, |s| s.adsr.sustain = (s.adsr.sustain - 0.05).max(0.0)),
            move |_, _, cx| with_sampler(cx, track_idx, slot_idx, |s| s.adsr.sustain = (s.adsr.sustain + 0.05).min(1.0)),
        ))
        .child(knob_row(
            "dms_adsr_release",
            "REL",
            &format!("{:.0} ms", adsr.release),
            normalized(adsr.release, 0.0, 4000.0),
            move |_, _, cx| with_sampler(cx, track_idx, slot_idx, |s| s.adsr.release = (s.adsr.release - 5.0).max(0.0)),
            move |_, _, cx| with_sampler(cx, track_idx, slot_idx, |s| s.adsr.release = (s.adsr.release + 5.0).min(4000.0)),
        ))
        .child(section("SLICES"))
        .child(
            h_flex()
                .items_center()
                .gap(px(4.0))
                .child(
                    Button::new("dms_slice_prev")
                        .rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("<")
                        .compact()
                        .on_click(move |_, _, cx| {
                            with_sampler(cx, track_idx, slot_idx, |s| {
                                s.selected_slice = s.selected_slice.saturating_sub(1);
                            });
                        }),
                )
                .child(
                    Label::new(format!("{} / {}", slice + 1, slice_count)).text_xs(),
                )
                .child(
                    Button::new("dms_slice_next")
                        .rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label(">")
                        .compact()
                        .on_click(move |_, _, cx| {
                            with_sampler(cx, track_idx, slot_idx, |s| {
                                s.selected_slice = (s.selected_slice + 1).min(s.slices.len().saturating_sub(1));
                            });
                        }),
                )
                .child(div().flex_1())
                .child(
                    Button::new("dms_slice_add")
                        .rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("+")
                        .compact()
                        .on_click(move |_, _, cx| {
                            with_sampler(cx, track_idx, slot_idx, |s| {
                                s.slices.push(0.0);
                                s.selected_slice = s.slices.len() - 1;
                            });
                        }),
                ),
        )
        .into_any_element()
}

/// Aplica un cambio al sampler del slot y pide redibujar.
fn with_sampler(
    cx: &mut gpui_kit::App,
    track_idx: usize,
    slot_idx: usize,
    change: impl FnOnce(&mut DmsSampler),
) {
    state(cx).update(cx, |state, cx| {
        if let Some(sampler) = state
            .slot_mut(track_idx, slot_idx)
            .and_then(|slot| slot.dms_state.as_mut())
            .map(|dms| &mut dms.sampler)
        {
            change(sampler);
        }
        cx.notify();
    });
}

/// Aplica un cambio a un pad del sampler, buscándolo por id.
fn with_pad(
    cx: &mut gpui_kit::App,
    track_idx: usize,
    slot_idx: usize,
    pad_idx: usize,
    change: impl FnOnce(&mut DmsPad),
) {
    state(cx).update(cx, |state, cx| {
        if let Some(pad) = state
            .slot_mut(track_idx, slot_idx)
            .and_then(|slot| slot.dms_state.as_mut())
            .and_then(|dms| dms.pads.get_mut(pad_idx))
        {
            change(pad);
        }
        cx.notify();
    });
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
