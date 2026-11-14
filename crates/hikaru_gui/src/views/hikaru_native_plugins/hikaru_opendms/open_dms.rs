// La grilla de pads del sampler Hikaru OpenDMS.
use std::path::PathBuf;

use crate::audio_proxy::{AudioProxy, GuiCommand};
use crate::theme;
use crate::ui::bitwig_kit::{
    capsule_text_button, fader_vertical_track_vol, rotary_knob, JoinSide, BW_ACCENT_ORANGE,
};
use crate::ui::fonts;
use crate::views::hikaru_native_plugins::hikaru_sampler::HikaruSampler;
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
    pub sampler: Option<HikaruSampler>,
}

impl DmsPad {
    pub fn new(id: usize, midi_note: u8) -> Self {
        Self {
            id,
            midi_note,
            name: format!("Pad {:02}", id + 1),
            sample_path: None,
            volume: 0.75,
            pan: 0.0,
            pitch: 0.0,
            mute: false,
            solo: false,
            waveform_peaks: Vec::new(),
            cached_peak_path: None,
            sampler: None,
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
        if let Some(ref smp) = self.sampler {
            if !smp.sample_name.is_empty() {
                return smp.sample_name.clone();
            }
            return "Sampler".to_string();
        }
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
    pub fx_open: bool,
    pub chains_expanded: bool,
    pub output_vol: f32,
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
            fx_open: false,
            chains_expanded: true,
            output_vol: 0.75,
        }
    }
}

pub fn render_dms_ui(
    ui: &mut egui::Ui,
    dms: &mut OpenDms,
    _project_bpm: f32,
    dragged_sample: &mut Option<PathBuf>,
    audio_proxy: &AudioProxy,
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
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 1.5);
                    let num_banks = 9usize;
                    for rev_b in 0..num_banks {
                        let bank = num_banks - 1 - rev_b;
                        let is_active = dms.active_bank == bank;
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(15.0, 19.0), egui::Sense::click());
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
                        let cell = 2.5;
                        let gap = 1.0;
                        let start_x = rect.min.x + 1.0;
                        let start_y = rect.min.y + 3.0;
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
                let output_vol = dms.output_vol;
                let default_adsr = dms.sampler.adsr;
                egui::Grid::new("dms_pads_grid")
                    .spacing(egui::vec2(3.0, 3.0))
                    .show(ui, |ui| {
                        for r in 0..rows {
                            for c in 0..cols {
                                let pad_idx = base_idx + (rows - 1 - r) * cols + c;
                                let (rect, response) = ui.allocate_exact_size(
                                    egui::vec2(64.0, 44.0),
                                    egui::Sense::click(),
                                );
                                if response.clicked() {
                                    dms.selected_pad = pad_idx;
                                    if let Some(pad) = dms.pads.get_mut(pad_idx) {
                                        if pad.sampler.is_none() {
                                            if let Some(ref p) = pad.sample_path {
                                                let mut smp = HikaruSampler::default();
                                                let _ = smp.load_wav(std::path::Path::new(p));
                                                pad.sampler = Some(smp);
                                            }
                                        }
                                    }
                                }

                                let pointer_over_pad = rect.contains(
                                    ui.input(|i| i.pointer.hover_pos()).unwrap_or(egui::Pos2::new(-1.0, -1.0)),
                                );
                                let is_drop_target = dragged_sample.is_some() && pointer_over_pad;
                                if is_drop_target && ui.input(|i| i.pointer.any_released()) {
                                    if let Some(path) = dragged_sample.take() {
                                        if let Some(pad) = dms.pads.get_mut(pad_idx) {
                                            let mut smp = pad.sampler.take().unwrap_or_default();
                                            let _ = smp.load_wav(&path);
                                            pad.sampler = Some(smp);
                                            let path_str = path.to_string_lossy().to_string();
                                            pad.load_sample(path_str.clone());
                                            audio_proxy.send(GuiCommand::LoadDmsSample {
                                                pad_idx,
                                                path: path_str,
                                            });
                                        }
                                        dms.selected_pad = pad_idx;
                                        dms.fx_open = true;
                                    }
                                }

                                let is_selected = dms.selected_pad == pad_idx;
                                let painter = ui.painter_at(rect);
                                let fill = if is_drop_target || response.hovered() {
                                    pad_hover_fill
                                } else {
                                    pad_fill
                                };
                                painter.rect_filled(rect, 3.0, fill);

                                if let Some(pad) = dms.pads.get_mut(pad_idx) {
                                    let is_loaded = pad.sampler.is_some() || pad.sample_path.is_some();
                                    if is_loaded {
                                        // Nombre del sample o Sampler arriba
                                        let name_color = if pad.mute {
                                            egui::Color32::from_rgb(150, 150, 155)
                                        } else {
                                            theme::ACCENT_PRIMARY
                                        };
                                        painter.text(
                                            rect.min + egui::vec2(4.0, 2.0),
                                            egui::Align2::LEFT_TOP,
                                            pad.display_filename(),
                                            egui::FontId::proportional(8.5),
                                            name_color,
                                        );

                                        // Barra inferior estilo Bitwig con Play (▶), Solo (S) y Mute (M)
                                        let bar_rect = egui::Rect::from_min_max(
                                            egui::pos2(rect.min.x + 1.0, rect.max.y - 12.0),
                                            egui::pos2(rect.max.x - 1.0, rect.max.y - 1.0),
                                        );
                                        painter.rect_filled(
                                            bar_rect,
                                            egui::CornerRadius {
                                                nw: 0,
                                                ne: 0,
                                                sw: 2,
                                                se: 2,
                                            },
                                            egui::Color32::from_rgb(24, 24, 26),
                                        );

                                        // Botón Play (▶) a la izquierda
                                        let play_rect = egui::Rect::from_min_size(
                                            egui::pos2(bar_rect.min.x + 1.0, bar_rect.min.y),
                                            egui::vec2(14.0, bar_rect.height()),
                                        );
                                        let play_resp = ui.interact(
                                            play_rect,
                                            ui.id().with(("dms_pad_play", pad_idx)),
                                            egui::Sense::click(),
                                        );
                                        if play_resp.hovered() {
                                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                        }
                                        let tri_center = play_rect.center();
                                        let tri_color = if play_resp.is_pointer_button_down_on() {
                                            BW_ACCENT_ORANGE
                                        } else if play_resp.hovered() {
                                            egui::Color32::WHITE
                                        } else {
                                            egui::Color32::from_rgb(200, 200, 205)
                                        };
                                        painter.add(egui::Shape::convex_polygon(
                                            vec![
                                                tri_center + egui::vec2(-2.5, -3.2),
                                                tri_center + egui::vec2(3.2, 0.0),
                                                tri_center + egui::vec2(-2.5, 3.2),
                                            ],
                                            tri_color,
                                            egui::Stroke::NONE,
                                        ));
                                        if play_resp.on_hover_text("Reproducir pad").clicked() {
                                            dms.selected_pad = pad_idx;
                                            let (play_speed, attack_ms, decay_ms, sustain, release_ms) =
                                                if let Some(ref smp) = pad.sampler {
                                                    let speed = smp.speed as f64
                                                        * 2.0_f64.powf(
                                                            (pad.pitch
                                                                + smp.pitch_semitones
                                                                + smp.fine_cents / 100.0)
                                                                as f64
                                                                / 12.0,
                                                        );
                                                    (
                                                        speed,
                                                        smp.attack_ms,
                                                        smp.decay_ms,
                                                        smp.sustain,
                                                        smp.release_ms,
                                                    )
                                                } else {
                                                    (
                                                        2.0_f64.powf(pad.pitch as f64 / 12.0),
                                                        default_adsr.attack,
                                                        default_adsr.decay,
                                                        default_adsr.sustain,
                                                        default_adsr.release,
                                                    )
                                                };
                                            audio_proxy.send(GuiCommand::DmsNoteOn {
                                                pad_idx,
                                                gain: pad.volume * output_vol,
                                                pan: pad.pan,
                                                velocity: 1.0,
                                                play_speed,
                                                attack_ms,
                                                decay_ms,
                                                sustain,
                                                release_ms,
                                            });
                                        }

                                        // Botón Solo (S)
                                        let solo_rect = egui::Rect::from_min_size(
                                            egui::pos2(bar_rect.max.x - 27.0, bar_rect.min.y + 1.0),
                                            egui::vec2(12.0, bar_rect.height() - 2.0),
                                        );
                                        let solo_resp = ui.interact(
                                            solo_rect,
                                            ui.id().with(("dms_pad_solo", pad_idx)),
                                            egui::Sense::click(),
                                        );
                                        if solo_resp.hovered() {
                                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                        }
                                        let solo_bg = if pad.solo {
                                            egui::Color32::from_rgb(0x3B, 0x82, 0xF6)
                                        } else if solo_resp.hovered() {
                                            egui::Color32::from_rgb(58, 58, 64)
                                        } else {
                                            egui::Color32::TRANSPARENT
                                        };
                                        if solo_bg != egui::Color32::TRANSPARENT {
                                            painter.rect_filled(solo_rect, 2.0, solo_bg);
                                        }
                                        let solo_fg = if pad.solo || solo_resp.hovered() {
                                            egui::Color32::WHITE
                                        } else {
                                            egui::Color32::from_rgb(175, 175, 182)
                                        };
                                        painter.text(
                                            solo_rect.center(),
                                            egui::Align2::CENTER_CENTER,
                                            "S",
                                            fonts::semibold(8.0),
                                            solo_fg,
                                        );
                                        if solo_resp.on_hover_text("Solo pad").clicked() {
                                            pad.solo = !pad.solo;
                                        }

                                        // Botón Mute (M)
                                        let mute_rect = egui::Rect::from_min_size(
                                            egui::pos2(bar_rect.max.x - 13.0, bar_rect.min.y + 1.0),
                                            egui::vec2(12.0, bar_rect.height() - 2.0),
                                        );
                                        let mute_resp = ui.interact(
                                            mute_rect,
                                            ui.id().with(("dms_pad_mute", pad_idx)),
                                            egui::Sense::click(),
                                        );
                                        if mute_resp.hovered() {
                                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                        }
                                        let mute_bg = if pad.mute {
                                            BW_ACCENT_ORANGE
                                        } else if mute_resp.hovered() {
                                            egui::Color32::from_rgb(58, 58, 64)
                                        } else {
                                            egui::Color32::TRANSPARENT
                                        };
                                        if mute_bg != egui::Color32::TRANSPARENT {
                                            painter.rect_filled(mute_rect, 2.0, mute_bg);
                                        }
                                        let mute_fg = if pad.mute {
                                            egui::Color32::from_rgb(18, 18, 20)
                                        } else if mute_resp.hovered() {
                                            egui::Color32::WHITE
                                        } else {
                                            egui::Color32::from_rgb(175, 175, 182)
                                        };
                                        painter.text(
                                            mute_rect.center(),
                                            egui::Align2::CENTER_CENTER,
                                            "M",
                                            fonts::semibold(8.0),
                                            mute_fg,
                                        );
                                        if mute_resp.on_hover_text("Mutear pad").clicked() {
                                            pad.mute = !pad.mute;
                                        }
                                    } else {
                                        // Pad vacío: nota arriba a la izquierda y '+' abajo a la derecha
                                        let note_str = midi_note_name(pad.midi_note);
                                        painter.text(
                                            rect.min + egui::vec2(5.0, 3.0),
                                            egui::Align2::LEFT_TOP,
                                            note_str,
                                            egui::FontId::proportional(10.0),
                                            text_note,
                                        );

                                        let plus_rect = egui::Rect::from_min_max(
                                            rect.max - egui::vec2(16.0, 14.0),
                                            rect.max,
                                        );
                                        let plus_resp = ui.interact(
                                            plus_rect,
                                            ui.id().with(("dms_pad_plus", pad_idx)),
                                            egui::Sense::click(),
                                        );
                                        painter.text(
                                            rect.max - egui::vec2(5.0, 3.0),
                                            egui::Align2::RIGHT_BOTTOM,
                                            "+",
                                            egui::FontId::proportional(11.5),
                                            if plus_resp.hovered() {
                                                egui::Color32::WHITE
                                            } else {
                                                text_plus
                                            },
                                        );
                                        if plus_resp
                                            .on_hover_text("Insertar Hikaru Sampler en este pad")
                                            .clicked()
                                        {
                                            dms.selected_pad = pad_idx;
                                            if pad.sampler.is_none() {
                                                pad.sampler = Some(HikaruSampler::default());
                                            }
                                            dms.fx_open = true;
                                        }
                                    }
                                }

                                let stroke = if is_drop_target {
                                    egui::Stroke::new(2.0_f32, BW_ACCENT_ORANGE)
                                } else if is_selected {
                                    egui::Stroke::new(1.5_f32, theme::ACCENT_PRIMARY)
                                } else {
                                    egui::Stroke::new(1.0_f32, pad_border)
                                };
                                painter.rect_stroke(rect, 3.0, stroke, egui::StrokeKind::Inside);
                            }
                            ui.end_row();
                        }
                    });

                ui.add_space(4.0);

                // Derecha: tira de canal del pad seleccionado (Pan + Fader + S/M) y FX + Output
                let sel_idx = dms.selected_pad.min(dms.pads.len().saturating_sub(1));
                ui.vertical(|ui| {
                    ui.set_width(28.0);
                    ui.spacing_mut().item_spacing.y = 3.0;
                    if let Some(pad) = dms.pads.get_mut(sel_idx) {
                        let (r_pan, _) = rotary_knob(
                            ui,
                            20.0,
                            &mut pad.pan,
                            -1.0..=1.0,
                            0.0,
                            egui::Color32::from_rgb(0xB8, 0xBC, 0xC6),
                            true,
                        );
                        r_pan.on_hover_text(format!("Pad Pan: {:+.0}%", pad.pan * 100.0));

                        let (r_vol, _) = fader_vertical_track_vol(
                            ui,
                            egui::vec2(16.0, 110.0),
                            &mut pad.volume,
                            1.0,
                        );
                        r_vol.on_hover_text(format!("Pad Volume: {:.0}%", pad.volume * 100.0));

                        if capsule_text_button(
                            ui,
                            egui::vec2(24.0, 16.0),
                            "S",
                            8.5,
                            pad.solo,
                            egui::Color32::from_rgb(0x3B, 0x82, 0xF6),
                            JoinSide::Standalone,
                            false,
                        )
                        .clicked()
                        {
                            pad.solo = !pad.solo;
                        }
                        if capsule_text_button(
                            ui,
                            egui::vec2(24.0, 16.0),
                            "M",
                            8.5,
                            pad.mute,
                            BW_ACCENT_ORANGE,
                            JoinSide::Standalone,
                            false,
                        )
                        .clicked()
                        {
                            pad.mute = !pad.mute;
                        }
                    }
                });

                ui.add_space(3.0);

                // Columna FX + Output estilo Bitwig Drum Machine
                let fx_col_stroke =
                    egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0x42, 0x45, 0x4C));
                egui::Frame::NONE
                    .fill(egui::Color32::from_rgb(0x28, 0x2A, 0x2E))
                    .stroke(fx_col_stroke)
                    .corner_radius(3.5)
                    .inner_margin(egui::Margin::symmetric(3, 4))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(48.0);
                            ui.spacing_mut().item_spacing.y = 6.0;

                            let (fx_rect, fx_resp) = ui.allocate_exact_size(
                                egui::vec2(48.0, 26.0),
                                egui::Sense::click(),
                            );
                            if ui.is_rect_visible(fx_rect) {
                                let fx_bg = if dms.fx_open {
                                    egui::Color32::from_rgb(0x22, 0x24, 0x28)
                                } else if fx_resp.hovered() {
                                    egui::Color32::from_rgb(0x3D, 0x40, 0x47)
                                } else {
                                    egui::Color32::from_rgb(0x32, 0x34, 0x3A)
                                };
                                let fx_border = if fx_resp.hovered() {
                                    egui::Color32::from_rgb(0x68, 0x6D, 0x78)
                                } else {
                                    egui::Color32::from_rgb(0x42, 0x45, 0x4C)
                                };
                                let fx_bracket_col = if dms.fx_open {
                                    egui::Color32::from_rgb(0x40, 0x6A, 0x80)
                                } else {
                                    egui::Color32::from_rgb(0x2E, 0x43, 0x50)
                                };
                                let fx_fg = if dms.fx_open || fx_resp.hovered() {
                                    egui::Color32::WHITE
                                } else {
                                    egui::Color32::from_rgb(0xD0, 0xD4, 0xDC)
                                };
                                let p = ui.painter();
                                p.rect_filled(fx_rect, 2.5, fx_bg);
                                p.rect_stroke(
                                    fx_rect,
                                    2.5,
                                    egui::Stroke::new(1.0_f32, fx_border),
                                    egui::StrokeKind::Inside,
                                );
                                // Corchete superior estilo Bitwig (`┌───┐`) dentro del botón FX
                                let bx0 = fx_rect.min.x + 3.0;
                                let bx1 = fx_rect.max.x - 3.0;
                                let by0 = fx_rect.min.y + 3.0;
                                p.rect_filled(
                                    egui::Rect::from_min_max(
                                        egui::pos2(bx0, by0),
                                        egui::pos2(bx1, by0 + 2.0),
                                    ),
                                    0.0,
                                    fx_bracket_col,
                                );
                                p.rect_filled(
                                    egui::Rect::from_min_size(
                                        egui::pos2(bx0, by0),
                                        egui::vec2(2.0, 4.0),
                                    ),
                                    0.0,
                                    fx_bracket_col,
                                );
                                p.rect_filled(
                                    egui::Rect::from_min_size(
                                        egui::pos2(bx1 - 2.0, by0),
                                        egui::vec2(2.0, 4.0),
                                    ),
                                    0.0,
                                    fx_bracket_col,
                                );
                                p.text(
                                    fx_rect.center() + egui::vec2(0.0, 1.5),
                                    egui::Align2::CENTER_CENTER,
                                    "FX",
                                    fonts::semibold(10.0),
                                    fx_fg,
                                );
                            }
                            if fx_resp.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            if fx_resp
                                .on_hover_text("Mostrar/ocultar cadena FX / Sampler del pad")
                                .clicked()
                            {
                                dms.fx_open = !dms.fx_open;
                            }

                            ui.add_space(74.0);
                            ui.vertical_centered(|ui| {
                                let (r_out, _) = rotary_knob(
                                    ui,
                                    30.0,
                                    &mut dms.output_vol,
                                    0.0..=1.0,
                                    0.75,
                                    BW_ACCENT_ORANGE,
                                    false,
                                );
                                r_out.on_hover_text(format!(
                                    "Output: {:.0}%",
                                    dms.output_vol * 100.0
                                ));
                                ui.label(
                                    egui::RichText::new("Output")
                                        .font(fonts::regular(9.0))
                                        .color(theme::TEXT_SECONDARY),
                                );
                            });
                        });
                    });
            });
        });

    // Sincronizar sample del Sampler del pad al motor DMS si cargó un WAV nuevo
    let sel_idx = dms.selected_pad.min(dms.pads.len().saturating_sub(1));
    if let Some(pad) = dms.pads.get_mut(sel_idx) {
        if let Some(ref smp) = pad.sampler {
            if let Some(ref path) = smp.sample_path {
                let path_str = path.to_string_lossy().to_string();
                if pad.sample_path.as_deref() != Some(&path_str) {
                    pad.load_sample(path_str.clone());
                    audio_proxy.send(GuiCommand::LoadDmsSample {
                        pad_idx: sel_idx,
                        path: path_str,
                    });
                }
            }
        }
    }

    if frame_resp.response.hovered() || ui.rect_contains_pointer(frame_resp.response.rect) {
        let scroll_y = ui.input(|i| i.raw_scroll_delta.y);
        if scroll_y > 0.0 {
            dms.active_bank = (dms.active_bank + 1).min(8);
        } else if scroll_y < 0.0 {
            dms.active_bank = dms.active_bank.saturating_sub(1);
        }
    }
}
