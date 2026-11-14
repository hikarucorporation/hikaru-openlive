// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_sampler/sampler_editor.rs
// Hikaru Sampler — Bitwig Studio-style Polyphonic Sampler UI & State

use std::path::{Path, PathBuf};

use egui::{pos2, vec2, Align2, Color32, CursorIcon, Rect, RichText, Sense, Stroke, Ui};
use hikaru_dsp::synth::sampler::{
    SamplerDspParams, SamplerFilterMode, SamplerLoopMode, SamplerPlaybackMode,
};
use hikaru_project_file::SamplerParams;

use crate::app::AppState;
use crate::theme;
use crate::ui::bitwig_kit::{capsule_text_button, rotary_knob, JoinSide};
use crate::ui::fonts;

pub const SAMPLER_CARD_WIDTH: f32 = 794.0;

const WAVE_BG: Color32 = Color32::from_rgb(0x0E, 0x0F, 0x12);
const PANEL_BG: Color32 = Color32::from_rgb(0x23, 0x25, 0x2A);
const PANEL_BORDER: Color32 = Color32::from_rgb(0x36, 0x39, 0x42);
const BW_YELLOW: Color32 = Color32::from_rgb(0xD8, 0xC8, 0x22);
const BW_CYAN: Color32 = Color32::from_rgb(0x2A, 0xB8, 0xB8);
const BW_GREEN: Color32 = Color32::from_rgb(0x2E, 0xCC, 0x71);
const BW_ORANGE: Color32 = Color32::from_rgb(0xFF, 0x7A, 0x10);
const WAVE_FG: Color32 = Color32::from_rgb(0xE8, 0xEA, 0xEE);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SamplerOffsetMode {
    #[default]
    Play,
    Loop,
    Len,
}

#[derive(Clone, Debug)]
pub struct HikaruSampler {
    pub sample_path: Option<PathBuf>,
    pub sample_name: String,
    pub sample_rate: f32,
    pub channels: usize,
    pub total_frames: usize,
    pub samples: Vec<f32>,
    /// Downsampled min/max peaks for waveform drawing (channel 0 & optional channel 1)
    pub peaks_l: Vec<(f32, f32)>,
    pub peaks_r: Vec<(f32, f32)>,

    pub playback_mode: SamplerPlaybackMode,
    pub speed: f32,
    pub reverse: bool,
    pub freeze: bool,
    pub ram_mode: bool,

    pub root_note: u8,
    pub fine_cents: f32,
    pub pitch_semitones: f32,
    pub glide_ms: f32,
    pub legato: bool,
    pub select_pos: f32,

    pub play_start: f32,
    pub play_end: f32,

    pub loop_mode: SamplerLoopMode,
    pub loop_start: f32,
    pub loop_len: f32,
    pub loop_crossfade: f32,
    pub offset_mode: SamplerOffsetMode,

    pub filter_mode: SamplerFilterMode,
    pub filter_cutoff_hz: f32,
    pub filter_resonance: f32,
    pub filter_keytrack: f32,

    pub attack_ms: f32,
    pub hold_ms: f32,
    pub decay_ms: f32,
    pub sustain: f32,
    pub release_ms: f32,

    pub gain_db: f32,
    pub pan: f32,
    pub output_db: f32,
    pub velocity_sens: f32,
    pub note_trigger: bool,
    pub release_trigger: bool,
    pub fx_expanded: bool,

    /// Version counters for incremental audio-engine sync
    pub sample_rev: u64,
    pub params_rev: u64,
}

impl Default for HikaruSampler {
    fn default() -> Self {
        Self {
            sample_path: None,
            sample_name: String::new(),
            sample_rate: 44100.0,
            channels: 1,
            total_frames: 0,
            samples: Vec::new(),
            peaks_l: Vec::new(),
            peaks_r: Vec::new(),

            playback_mode: SamplerPlaybackMode::Repitch,
            speed: 1.0,
            reverse: false,
            freeze: false,
            ram_mode: true,

            root_note: 60,
            fine_cents: 0.0,
            pitch_semitones: 0.0,
            glide_ms: 0.0,
            legato: false,
            select_pos: 0.0,

            play_start: 0.0,
            play_end: 1.0,

            loop_mode: SamplerLoopMode::Off,
            loop_start: 0.0,
            loop_len: 1.0,
            loop_crossfade: 0.0,
            offset_mode: SamplerOffsetMode::Play,

            filter_mode: SamplerFilterMode::LowPass12,
            filter_cutoff_hz: 2640.0,
            filter_resonance: 0.0,
            filter_keytrack: 0.0,

            attack_ms: 2.0,
            hold_ms: 0.0,
            decay_ms: 200.0,
            sustain: 1.0,
            release_ms: 50.0,

            gain_db: 0.0,
            pan: 0.0,
            output_db: 0.0,
            velocity_sens: 1.0,
            note_trigger: true,
            release_trigger: false,
            fx_expanded: false,

            sample_rev: 0,
            params_rev: 1,
        }
    }
}

impl HikaruSampler {
    pub fn to_dsp_params(&self) -> SamplerDspParams {
        SamplerDspParams {
            playback_mode: self.playback_mode,
            speed: self.speed,
            reverse: self.reverse,
            freeze: self.freeze,
            root_note: self.root_note,
            fine_cents: self.fine_cents,
            pitch_semitones: self.pitch_semitones,
            glide_ms: self.glide_ms,
            legato: self.legato,
            play_start: self.play_start,
            play_end: self.play_end,
            loop_mode: self.loop_mode,
            loop_start: self.loop_start,
            loop_len: self.loop_len,
            loop_crossfade: self.loop_crossfade,
            filter_mode: self.filter_mode,
            filter_cutoff_hz: self.filter_cutoff_hz,
            filter_resonance: self.filter_resonance,
            filter_keytrack: self.filter_keytrack,
            attack_ms: self.attack_ms,
            hold_ms: self.hold_ms,
            decay_ms: self.decay_ms,
            sustain: self.sustain,
            release_ms: self.release_ms,
            gain_db: self.gain_db,
            pan: self.pan,
            output_db: self.output_db,
            velocity_sens: self.velocity_sens,
            note_trigger: self.note_trigger,
            release_trigger: self.release_trigger,
        }
    }

    pub fn to_project_params(&self) -> SamplerParams {
        SamplerParams {
            sample_path: self
                .sample_path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
            sample_name: self.sample_name.clone(),
            playback_mode: format!("{:?}", self.playback_mode),
            speed: self.speed,
            reverse: self.reverse,
            freeze: self.freeze,
            ram_mode: self.ram_mode,
            root_note: self.root_note,
            fine_cents: self.fine_cents,
            pitch_semitones: self.pitch_semitones,
            glide_ms: self.glide_ms,
            legato: self.legato,
            select_pos: self.select_pos,
            play_start: self.play_start,
            play_end: self.play_end,
            loop_mode: format!("{:?}", self.loop_mode),
            loop_start: self.loop_start,
            loop_len: self.loop_len,
            loop_crossfade: self.loop_crossfade,
            offset_mode: format!("{:?}", self.offset_mode),
            filter_mode: format!("{:?}", self.filter_mode),
            filter_cutoff_hz: self.filter_cutoff_hz,
            filter_resonance: self.filter_resonance,
            filter_keytrack: self.filter_keytrack,
            attack_ms: self.attack_ms,
            hold_ms: self.hold_ms,
            decay_ms: self.decay_ms,
            sustain: self.sustain,
            release_ms: self.release_ms,
            gain_db: self.gain_db,
            pan: self.pan,
            output_db: self.output_db,
            velocity_sens: self.velocity_sens,
            note_trigger: self.note_trigger,
            release_trigger: self.release_trigger,
        }
    }

    pub fn apply_project_params(&mut self, p: &SamplerParams) {
        self.playback_mode = match p.playback_mode.as_str() {
            "Cycles" => SamplerPlaybackMode::Cycles,
            "Textures" => SamplerPlaybackMode::Textures,
            _ => SamplerPlaybackMode::Repitch,
        };
        self.speed = p.speed.clamp(0.05, 4.0);
        self.reverse = p.reverse;
        self.freeze = p.freeze;
        self.ram_mode = p.ram_mode;
        self.root_note = p.root_note.min(127);
        self.fine_cents = p.fine_cents.clamp(-100.0, 100.0);
        self.pitch_semitones = p.pitch_semitones.clamp(-36.0, 36.0);
        self.glide_ms = p.glide_ms.clamp(0.0, 2000.0);
        self.legato = p.legato;
        self.select_pos = p.select_pos.clamp(0.0, 1.0);
        self.play_start = p.play_start.clamp(0.0, 0.99);
        self.play_end = p.play_end.clamp(self.play_start + 0.01, 1.0);
        self.loop_mode = match p.loop_mode.as_str() {
            "Forward" => SamplerLoopMode::Forward,
            "PingPong" => SamplerLoopMode::PingPong,
            _ => SamplerLoopMode::Off,
        };
        self.loop_start = p.loop_start.clamp(0.0, 0.99);
        self.loop_len = p.loop_len.clamp(0.01, 1.0);
        self.loop_crossfade = p.loop_crossfade.clamp(0.0, 0.5);
        self.offset_mode = match p.offset_mode.as_str() {
            "Loop" => SamplerOffsetMode::Loop,
            "Len" => SamplerOffsetMode::Len,
            _ => SamplerOffsetMode::Play,
        };
        self.filter_mode = match p.filter_mode.as_str() {
            "Off" => SamplerFilterMode::Off,
            "LowPass24" => SamplerFilterMode::LowPass24,
            "BandPass" => SamplerFilterMode::BandPass,
            "HighPass" => SamplerFilterMode::HighPass,
            "Notch" => SamplerFilterMode::Notch,
            "Peak" => SamplerFilterMode::Peak,
            _ => SamplerFilterMode::LowPass12,
        };
        self.filter_cutoff_hz = p.filter_cutoff_hz.clamp(20.0, 20000.0);
        self.filter_resonance = p.filter_resonance.clamp(0.0, 1.0);
        self.filter_keytrack = p.filter_keytrack.clamp(0.0, 1.0);
        self.attack_ms = p.attack_ms.clamp(0.0, 5000.0);
        self.hold_ms = p.hold_ms.clamp(0.0, 5000.0);
        self.decay_ms = p.decay_ms.clamp(1.0, 10000.0);
        self.sustain = p.sustain.clamp(0.0, 1.0);
        self.release_ms = p.release_ms.clamp(1.0, 10000.0);
        self.gain_db = p.gain_db.clamp(-24.0, 24.0);
        self.pan = p.pan.clamp(-1.0, 1.0);
        self.output_db = p.output_db.clamp(-60.0, 6.0);
        self.velocity_sens = p.velocity_sens.clamp(0.0, 1.0);
        self.note_trigger = p.note_trigger;
        self.release_trigger = p.release_trigger;

        if let Some(ref path_str) = p.sample_path {
            let path = PathBuf::from(path_str);
            if path.exists() {
                let _ = self.load_wav(&path);
            }
        }
        self.params_rev = self.params_rev.wrapping_add(1);
    }

    pub fn load_wav(&mut self, path: &Path) -> Result<(), String> {
        let mut reader = hound::WavReader::open(path).map_err(|e| e.to_string())?;
        let spec = reader.spec();
        let ch = (spec.channels as usize).max(1);
        let sr = spec.sample_rate as f32;

        let raw: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader.samples::<f32>().filter_map(Result::ok).collect(),
            hound::SampleFormat::Int => {
                let scale = (1i64 << (spec.bits_per_sample.saturating_sub(1))) as f32;
                reader
                    .samples::<i32>()
                    .filter_map(Result::ok)
                    .map(|s| s as f32 / scale.max(1.0))
                    .collect()
            }
        };

        let total_frames = raw.len() / ch;
        if total_frames == 0 {
            return Err("Empty WAV file".to_string());
        }

        const BINS: usize = 180;
        let mut peaks_l = vec![(0.0f32, 0.0f32); BINS];
        let mut peaks_r = vec![(0.0f32, 0.0f32); BINS];
        let frames_per_bin = (total_frames / BINS).max(1);

        for bin in 0..BINS {
            let start_f = (bin * total_frames) / BINS;
            let end_f = (((bin + 1) * total_frames) / BINS).max(start_f + 1).min(total_frames);
            let step = ((end_f - start_f) / frames_per_bin.min(64)).max(1);
            let mut min_l = 0.0f32;
            let mut max_l = 0.0f32;
            let mut min_r = 0.0f32;
            let mut max_r = 0.0f32;
            let mut f = start_f;
            while f < end_f {
                let sl = raw[f * ch];
                min_l = min_l.min(sl);
                max_l = max_l.max(sl);
                let sr_v = if ch >= 2 { raw[f * ch + 1] } else { sl };
                min_r = min_r.min(sr_v);
                max_r = max_r.max(sr_v);
                f += step;
            }
            peaks_l[bin] = (min_l, max_l);
            peaks_r[bin] = (min_r, max_r);
        }

        self.sample_path = Some(path.to_path_buf());
        self.sample_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Sample.wav".to_string());
        self.sample_rate = sr;
        self.channels = ch;
        self.total_frames = total_frames;
        self.samples = raw;
        self.peaks_l = peaks_l;
        self.peaks_r = peaks_r;
        self.play_start = 0.0;
        self.play_end = 1.0;
        self.loop_start = 0.0;
        self.loop_len = 1.0;
        self.sample_rev = self.sample_rev.wrapping_add(1);
        self.params_rev = self.params_rev.wrapping_add(1);
        Ok(())
    }

    pub fn duration_ms(&self) -> f32 {
        if self.sample_rate > 0.0 && self.total_frames > 0 {
            (self.total_frames as f32 / self.sample_rate) * 1000.0
        } else {
            0.0
        }
    }

    pub fn mark_params_dirty(&mut self) {
        self.params_rev = self.params_rev.wrapping_add(1);
    }
}

fn midi_note_label(note: u8) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let name = NAMES[(note % 12) as usize];
    let oct = (note as i32 / 12) - 2;
    format!("{name}{oct}")
}

fn format_ms(ms: f32) -> String {
    if ms >= 1000.0 {
        format!("{:.2} s", ms / 1000.0)
    } else if ms >= 10.0 {
        format!("{ms:.0} ms")
    } else {
        format!("{ms:.2} ms")
    }
}

fn format_hz(hz: f32) -> String {
    if hz >= 1000.0 {
        format!("{:.2} kHz", hz / 1000.0)
    } else {
        format!("{hz:.0} Hz")
    }
}

fn cutoff_to_norm(hz: f32) -> f32 {
    let min_l = 20.0f32.ln();
    let max_l = 20000.0f32.ln();
    ((hz.clamp(20.0, 20000.0).ln() - min_l) / (max_l - min_l)).clamp(0.0, 1.0)
}

fn norm_to_cutoff(norm: f32) -> f32 {
    let min_l = 20.0f32.ln();
    let max_l = 20000.0f32.ln();
    (min_l + norm.clamp(0.0, 1.0) * (max_l - min_l)).exp()
}

fn paint_filter_icon(painter: &egui::Painter, rect: Rect, mode: SamplerFilterMode, color: Color32) {
    let l = rect.left() + 2.0;
    let r = rect.right() - 2.0;
    let t = rect.top() + 3.0;
    let b = rect.bottom() - 3.0;
    let m_x = rect.center().x;
    let m_y = rect.center().y;
    let stroke = Stroke::new(1.3_f32, color);

    match mode {
        SamplerFilterMode::Off => {
            painter.line_segment([pos2(l, m_y), pos2(r, m_y)], stroke);
        }
        SamplerFilterMode::LowPass12 => {
            painter.line_segment([pos2(l, m_y), pos2(m_x, m_y)], stroke);
            painter.line_segment([pos2(m_x, m_y), pos2(r, b)], stroke);
        }
        SamplerFilterMode::LowPass24 => {
            painter.line_segment([pos2(l, m_y), pos2(m_x - 1.0, m_y)], stroke);
            painter.line_segment([pos2(m_x - 1.0, m_y), pos2(m_x + 2.0, t)], stroke);
            painter.line_segment([pos2(m_x + 2.0, t), pos2(r, b)], stroke);
        }
        SamplerFilterMode::BandPass => {
            painter.line_segment([pos2(l, b), pos2(m_x, t)], stroke);
            painter.line_segment([pos2(m_x, t), pos2(r, b)], stroke);
        }
        SamplerFilterMode::HighPass => {
            painter.line_segment([pos2(l, b), pos2(m_x, m_y)], stroke);
            painter.line_segment([pos2(m_x, m_y), pos2(r, m_y)], stroke);
        }
        SamplerFilterMode::Notch => {
            painter.line_segment([pos2(l, m_y), pos2(m_x - 3.0, m_y)], stroke);
            painter.line_segment([pos2(m_x - 3.0, m_y), pos2(m_x, b)], stroke);
            painter.line_segment([pos2(m_x, b), pos2(m_x + 3.0, m_y)], stroke);
            painter.line_segment([pos2(m_x + 3.0, m_y), pos2(r, m_y)], stroke);
        }
        SamplerFilterMode::Peak => {
            painter.line_segment([pos2(l, m_y), pos2(m_x, t)], stroke);
            painter.line_segment([pos2(m_x, t), pos2(r, m_y)], stroke);
        }
    }
}

pub fn render_module(ui: &mut Ui, app: &mut AppState, track_idx: usize, slot_idx: usize) {
    let mut sampler = app
        .current_tracks()
        .get(track_idx)
        .and_then(|t| t.effects.get(slot_idx))
        .and_then(|s| s.sampler_state.clone())
        .unwrap_or_default();

    render_sampler_state(ui, &mut sampler, slot_idx);

    if let Some(track) = app.current_tracks_mut().get_mut(track_idx) {
        if let Some(slot) = track.effects.get_mut(slot_idx) {
            slot.sampler_state = Some(sampler);
        }
    }
}

pub fn render_sampler_state(ui: &mut Ui, sampler: &mut HikaruSampler, slot_idx: usize) {
    let mut changed = false;
    let mut pick_file_requested = false;

    let body_rect = ui.available_rect_before_wrap();
    let h = body_rect.height().max(185.0);

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 2.0);

        // ── COL 1: Modulator / Expressions (Bitwig left strip) ──────────────
        ui.vertical(|ui| {
            ui.set_width(80.0);
            ui.spacing_mut().item_spacing.y = 3.0;

            // MW Sine card
            let (mw_rect, _) = ui.allocate_exact_size(vec2(80.0, 46.0), Sense::hover());
            let p = ui.painter();
            p.rect_filled(mw_rect, 2.5, PANEL_BG);
            p.rect_stroke(mw_rect, 2.5, Stroke::new(1.0_f32, PANEL_BORDER), egui::StrokeKind::Inside);
            p.text(
                pos2(mw_rect.right() - 4.0, mw_rect.top() + 3.0),
                Align2::RIGHT_TOP,
                "MW",
                fonts::semibold(8.5),
                theme::TEXT_SECONDARY,
            );
            // Draw orange sine wave
            let wave_pts: Vec<egui::Pos2> = (0..=24)
                .map(|i| {
                    let t = i as f32 / 24.0;
                    let x = mw_rect.left() + 6.0 + t * (mw_rect.width() - 12.0);
                    let y = mw_rect.center().y - (t * std::f32::consts::TAU).sin() * 11.0;
                    pos2(x, y)
                })
                .collect();
            p.add(egui::Shape::line(wave_pts, Stroke::new(1.4_f32, BW_ORANGE)));

            // Expressions box
            let (expr_rect, _) = ui.allocate_exact_size(vec2(80.0, 68.0), Sense::hover());
            let p = ui.painter();
            p.rect_filled(expr_rect, 2.5, PANEL_BG);
            p.rect_stroke(expr_rect, 2.5, Stroke::new(1.0_f32, PANEL_BORDER), egui::StrokeKind::Inside);
            p.text(
                pos2(expr_rect.center().x, expr_rect.top() + 5.0),
                Align2::CENTER_TOP,
                "Expressions",
                fonts::semibold(8.5),
                theme::TEXT_SECONDARY,
            );

            let expr_items = [
                ("VEL", sampler.velocity_sens > 0.01, pos2(expr_rect.left() + 4.0, expr_rect.top() + 20.0)),
                ("TIMB", false, pos2(expr_rect.center().x + 2.0, expr_rect.top() + 20.0)),
                ("REL", sampler.release_trigger, pos2(expr_rect.left() + 4.0, expr_rect.top() + 40.0)),
                ("PRES", false, pos2(expr_rect.center().x + 2.0, expr_rect.top() + 40.0)),
            ];
            for (idx, (lbl, active, pos)) in expr_items.iter().enumerate() {
                let r = Rect::from_min_size(*pos, vec2(34.0, 16.0));
                let resp = ui.interact(r, ui.id().with(("smp_expr", slot_idx, idx)), Sense::click());
                if resp.clicked() {
                    if idx == 0 {
                        sampler.velocity_sens = if sampler.velocity_sens > 0.01 { 0.0 } else { 1.0 };
                        changed = true;
                    } else if idx == 2 {
                        sampler.release_trigger = !sampler.release_trigger;
                        changed = true;
                    }
                }
                let col = if *active { BW_GREEN } else { theme::TEXT_MUTED };
                ui.painter().text(
                    pos2(r.center().x, r.top() + 1.0),
                    Align2::CENTER_TOP,
                    *lbl,
                    fonts::semibold(7.5),
                    if *active { theme::TEXT_PRIMARY } else { theme::TEXT_SECONDARY },
                );
                ui.painter().line_segment(
                    [pos2(r.left() + 6.0, r.bottom() - 3.0), pos2(r.right() - 6.0, r.bottom() - 3.0)],
                    Stroke::new(1.5_f32, col),
                );
            }

            // `+` Modulator button
            let (plus_rect, plus_resp) = ui.allocate_exact_size(vec2(80.0, (h - 120.0).max(24.0)), Sense::click());
            ui.painter().rect_filled(plus_rect, 2.5, PANEL_BG);
            ui.painter().rect_stroke(plus_rect, 2.5, Stroke::new(1.0_f32, PANEL_BORDER), egui::StrokeKind::Inside);
            ui.painter().text(
                plus_rect.center(),
                Align2::CENTER_CENTER,
                "+",
                fonts::semibold(14.0),
                if plus_resp.hovered() { Color32::WHITE } else { theme::TEXT_SECONDARY },
            );
        });

        // ── COL 2: Select, Pitch, Glide+L ───────────────────────────────────
        ui.vertical(|ui| {
            ui.set_width(50.0);
            ui.spacing_mut().item_spacing.y = 1.0;

            let (col_rect, _) = ui.allocate_exact_size(vec2(50.0, h), Sense::hover());
            ui.painter().rect_filled(col_rect, 2.5, PANEL_BG);
            ui.painter().rect_stroke(col_rect, 2.5, Stroke::new(1.0_f32, PANEL_BORDER), egui::StrokeKind::Inside);

            let mut col_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(col_rect.shrink2(vec2(3.0, 6.0)))
                    .layout(egui::Layout::top_down(egui::Align::Center)),
            );
            col_ui.spacing_mut().item_spacing.y = 6.0;

            let (r_sel, c_sel) = rotary_knob(
                &mut col_ui,
                25.0,
                &mut sampler.select_pos,
                0.0..=1.0,
                0.0,
                Color32::from_rgb(0xB8, 0xBC, 0xC6),
                false,
            );
            r_sel.on_hover_text(format!("Select: {:.0}%", sampler.select_pos * 100.0));
            if c_sel {
                changed = true;
            }
            col_ui.label(RichText::new("Select").font(fonts::regular(8.5)).color(theme::TEXT_SECONDARY));

            let (r_pit, c_pit) = rotary_knob(
                &mut col_ui,
                25.0,
                &mut sampler.pitch_semitones,
                -36.0..=36.0,
                0.0,
                Color32::from_rgb(0xB8, 0xBC, 0xC6),
                true,
            );
            r_pit.on_hover_text(format!("Pitch: {:+.1} st", sampler.pitch_semitones));
            if c_pit {
                changed = true;
            }
            col_ui.label(RichText::new("Pitch").font(fonts::regular(8.5)).color(theme::TEXT_SECONDARY));

            let (r_gld, c_gld) = rotary_knob(
                &mut col_ui,
                25.0,
                &mut sampler.glide_ms,
                0.0..=1000.0,
                0.0,
                Color32::from_rgb(0xB8, 0xBC, 0xC6),
                false,
            );
            r_gld.on_hover_text(format!("Glide: {:.0} ms", sampler.glide_ms));
            if c_gld {
                changed = true;
            }
            col_ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.label(RichText::new("Glide").font(fonts::regular(8.5)).color(theme::TEXT_SECONDARY));
                let l_resp = capsule_text_button(
                    ui,
                    vec2(13.0, 12.0),
                    "L",
                    8.0,
                    sampler.legato,
                    BW_ORANGE,
                    JoinSide::Standalone,
                    false,
                );
                if l_resp.on_hover_text("Legato Glide").clicked() {
                    sampler.legato = !sampler.legato;
                    changed = true;
                }
            });
        });

        // ── COL 3: Center Waveform + Repitch / Offsets / Filter / AHDSR ─────
        let center_w = 550.0;
        let wf_h = (h * 0.56).round().clamp(76.0, 126.0);
        ui.vertical(|ui| {
            ui.set_width(center_w);
            ui.spacing_mut().item_spacing.y = 3.0;

            // Top Half: Waveform Display (scales with panel height, ~112px at 200px)
            let (wf_box, _) = ui.allocate_exact_size(vec2(center_w, wf_h), Sense::hover());
            ui.painter().rect_filled(wf_box, 2.5, Color32::from_rgb(0x18, 0x1A, 0x1E));
            ui.painter().rect_stroke(wf_box, 2.5, Stroke::new(1.0_f32, PANEL_BORDER), egui::StrokeKind::Inside);

            if sampler.total_frames == 0 {
                // Empty State (Bitwig style)
                let btn_r = Rect::from_center_size(pos2(wf_box.center().x, wf_box.center().y - 10.0), vec2(210.0, 18.0));
                let resp1 = ui.interact(btn_r, ui.id().with(("smp_browse", slot_idx)), Sense::click());
                if resp1.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                ui.painter().text(
                    btn_r.center(),
                    Align2::CENTER_CENTER,
                    "📁  Browse samples / multisamples",
                    fonts::semibold(10.0),
                    if resp1.hovered() { Color32::WHITE } else { theme::TEXT_SECONDARY },
                );
                if resp1.clicked() {
                    pick_file_requested = true;
                }

                let btn2_r = Rect::from_center_size(pos2(wf_box.center().x, wf_box.center().y + 12.0), vec2(210.0, 18.0));
                let resp2 = ui.interact(btn2_r, ui.id().with(("smp_create", slot_idx)), Sense::click());
                if resp2.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                ui.painter().text(
                    btn2_r.center(),
                    Align2::CENTER_CENTER,
                    "🎹  Create new multisample",
                    fonts::regular(9.5),
                    if resp2.hovered() { Color32::WHITE } else { theme::TEXT_MUTED },
                );
                if resp2.clicked() {
                    pick_file_requested = true;
                }
            } else {
                // Loaded State (Bitwig style)
                let top_bar = Rect::from_min_size(wf_box.min, vec2(center_w, 16.0));
                let wave_rect = Rect::from_min_size(pos2(wf_box.min.x + 2.0, wf_box.min.y + 16.0), vec2(center_w - 4.0, (wf_h - 33.0).max(40.0)));
                let bot_bar = Rect::from_min_size(pos2(wf_box.min.x, wf_box.max.y - 17.0), vec2(center_w, 17.0));

                // Top info bar: Sample name | ROOT C3 | GAIN 0.0 dB
                let name_r = Rect::from_min_size(pos2(top_bar.min.x + 4.0, top_bar.min.y), vec2(170.0, 16.0));
                let name_resp = ui.interact(name_r, ui.id().with(("smp_name", slot_idx)), Sense::click());
                if name_resp.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                ui.painter().text(
                    pos2(name_r.min.x, name_r.center().y),
                    Align2::LEFT_CENTER,
                    format!("📄 {}", sampler.sample_name),
                    fonts::semibold(9.0),
                    if name_resp.hovered() { Color32::WHITE } else { Color32::from_rgb(0x6E, 0xB8, 0xFF) },
                );
                if name_resp.on_hover_text("Click to load another WAV sample").clicked() {
                    pick_file_requested = true;
                }

                // ROOT note draggable
                let root_r = Rect::from_min_size(pos2(top_bar.min.x + 215.0, top_bar.min.y), vec2(96.0, 16.0));
                let root_resp = ui.interact(root_r, ui.id().with(("smp_root", slot_idx)), Sense::drag());
                if root_resp.dragged() {
                    let dy = root_resp.drag_delta().y;
                    if dy.abs() > 0.5 {
                        let next = (sampler.root_note as f32 - dy * 0.25).round().clamp(0.0, 127.0) as u8;
                        if next != sampler.root_note {
                            sampler.root_note = next;
                            changed = true;
                        }
                    }
                }
                ui.painter().text(
                    pos2(root_r.min.x, root_r.center().y),
                    Align2::LEFT_CENTER,
                    format!("ROOT  {}  {:+.0} ct", midi_note_label(sampler.root_note), sampler.fine_cents),
                    fonts::regular(9.0),
                    Color32::from_rgb(0x7C, 0xB8, 0xFF),
                );

                // GAIN draggable
                let gain_r = Rect::from_min_size(pos2(top_bar.max.x - 115.0, top_bar.min.y), vec2(95.0, 16.0));
                let gain_resp = ui.interact(gain_r, ui.id().with(("smp_gain", slot_idx)), Sense::click_and_drag());
                if gain_resp.double_clicked() {
                    sampler.gain_db = 0.0;
                    changed = true;
                } else if gain_resp.dragged() {
                    sampler.gain_db = (sampler.gain_db - gain_resp.drag_delta().y * 0.2).clamp(-24.0, 24.0);
                    changed = true;
                }
                ui.painter().text(
                    pos2(gain_r.right(), gain_r.center().y),
                    Align2::RIGHT_CENTER,
                    format!("GAIN  {:+.1} dB", sampler.gain_db),
                    fonts::regular(9.0),
                    Color32::from_rgb(0x7C, 0xB8, 0xFF),
                );

                // Waveform canvas
                let p = ui.painter();
                p.rect_filled(wave_rect, 1.5, WAVE_BG);
                let cy = wave_rect.center().y;
                p.line_segment(
                    [pos2(wave_rect.left(), cy), pos2(wave_rect.right(), cy)],
                    Stroke::new(1.0_f32, Color32::from_rgb(0x28, 0x2B, 0x33)),
                );

                let bins = sampler.peaks_l.len();
                if bins > 0 {
                    let half_h = (wave_rect.height() * 0.45).max(2.0);
                    let dx = wave_rect.width() / bins as f32;
                    for (i, &(min_v, max_v)) in sampler.peaks_l.iter().enumerate() {
                        let x = wave_rect.left() + (i as f32 + 0.5) * dx;
                        let y_top = cy - max_v.clamp(0.0, 1.0) * half_h;
                        let y_bot = cy - min_v.clamp(-1.0, 0.0) * half_h;
                        p.line_segment(
                            [pos2(x, y_top), pos2(x, y_bot.max(y_top + 1.0))],
                            Stroke::new(dx.max(1.2), WAVE_FG),
                        );
                    }
                }

                // Loop region shading
                if sampler.loop_mode != SamplerLoopMode::Off {
                    let ls_x = wave_rect.left() + sampler.loop_start * wave_rect.width();
                    let le_x = wave_rect.left()
                        + (sampler.loop_start + sampler.loop_len).clamp(0.0, 1.0) * wave_rect.width();
                    p.rect_filled(
                        Rect::from_min_max(pos2(ls_x, wave_rect.top()), pos2(le_x, wave_rect.bottom())),
                        0.0,
                        Color32::from_rgba_unmultiplied(0x2A, 0xB8, 0xB8, 28),
                    );
                    p.line_segment([pos2(ls_x, wave_rect.top()), pos2(ls_x, wave_rect.bottom())], Stroke::new(1.2_f32, BW_CYAN));
                    p.line_segment([pos2(le_x, wave_rect.top()), pos2(le_x, wave_rect.bottom())], Stroke::new(1.2_f32, BW_CYAN));
                }

                // Play start & end markers (interactive!)
                let ps_x = wave_rect.left() + sampler.play_start * wave_rect.width();
                let pe_x = wave_rect.left() + sampler.play_end * wave_rect.width();
                p.line_segment([pos2(ps_x, wave_rect.top()), pos2(ps_x, wave_rect.bottom())], Stroke::new(1.5_f32, BW_YELLOW));
                p.line_segment([pos2(pe_x, wave_rect.top()), pos2(pe_x, wave_rect.bottom())], Stroke::new(1.5_f32, BW_YELLOW));
                p.rect_filled(Rect::from_min_size(pos2(ps_x, wave_rect.top()), vec2(6.0, 6.0)), 0.0, BW_YELLOW);
                p.rect_filled(Rect::from_min_size(pos2(pe_x - 6.0, wave_rect.top()), vec2(6.0, 6.0)), 0.0, BW_YELLOW);

                let wf_resp = ui.interact(wave_rect, ui.id().with(("smp_wf_drag", slot_idx)), Sense::click_and_drag());
                if wf_resp.dragged() {
                    if let Some(pos) = wf_resp.interact_pointer_pos() {
                        let frac = ((pos.x - wave_rect.left()) / wave_rect.width()).clamp(0.0, 1.0);
                        if (frac - sampler.play_start).abs() < (frac - sampler.play_end).abs() {
                            sampler.play_start = frac.min(sampler.play_end - 0.01);
                        } else {
                            sampler.play_end = frac.max(sampler.play_start + 0.01);
                        }
                        changed = true;
                    }
                }

                // Bottom info bar: PLAY | R | start ms | end ms | LOOP | modes
                let dur = sampler.duration_ms();
                let start_ms = dur * sampler.play_start;
                let end_ms = dur * sampler.play_end;

                let mut b_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(bot_bar.shrink2(vec2(4.0, 1.0)))
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                b_ui.spacing_mut().item_spacing.x = 5.0;
                b_ui.label(RichText::new("PLAY").font(fonts::semibold(8.5)).color(theme::TEXT_SECONDARY));
                let rev_resp = capsule_text_button(
                    &mut b_ui,
                    vec2(14.0, 13.0),
                    "R",
                    8.5,
                    sampler.reverse,
                    BW_YELLOW,
                    JoinSide::Standalone,
                    false,
                );
                if rev_resp.on_hover_text("Reverse Sample").clicked() {
                    sampler.reverse = !sampler.reverse;
                    changed = true;
                }
                b_ui.label(RichText::new(format_ms(start_ms)).font(fonts::regular(8.5)).color(BW_YELLOW));
                b_ui.label(RichText::new(format_ms(end_ms)).font(fonts::regular(8.5)).color(BW_YELLOW));

                b_ui.add_space(8.0);
                b_ui.label(RichText::new("LOOP").font(fonts::semibold(8.5)).color(theme::TEXT_SECONDARY));
                for (lm, lbl, tip) in [
                    (SamplerLoopMode::Off, "→|", "Loop Off"),
                    (SamplerLoopMode::Forward, "→", "Forward Loop"),
                    (SamplerLoopMode::PingPong, "↔", "Ping-Pong Loop"),
                ] {
                    let active = sampler.loop_mode == lm;
                    if capsule_text_button(
                        &mut b_ui,
                        vec2(18.0, 13.0),
                        lbl,
                        8.5,
                        active,
                        BW_CYAN,
                        JoinSide::Standalone,
                        false,
                    )
                    .on_hover_text(tip)
                    .clicked()
                    {
                        sampler.loop_mode = lm;
                        changed = true;
                    }
                }
                b_ui.label(
                    RichText::new(format_ms(dur * sampler.loop_start))
                        .font(fonts::regular(8.5))
                        .color(BW_CYAN),
                );
                b_ui.label(
                    RichText::new(format_ms(dur * sampler.loop_len))
                        .font(fonts::regular(8.5))
                        .color(BW_CYAN),
                );
            }

            // Bottom Half: Repitch | Offsets | Filter | AHDSR
            let bot_h = (h - wf_h - 3.0).max(54.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;

                // 1. Repitch / Speed Box (104px)
                let (rep_rect, _) = ui.allocate_exact_size(vec2(104.0, bot_h), Sense::hover());
                ui.painter().rect_filled(rep_rect, 2.5, PANEL_BG);
                ui.painter().rect_stroke(rep_rect, 2.5, Stroke::new(1.0_f32, PANEL_BORDER), egui::StrokeKind::Inside);
                let mut rep_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(rep_rect.shrink(3.0))
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                rep_ui.spacing_mut().item_spacing.y = 2.0;
                let mode_lbl = match sampler.playback_mode {
                    SamplerPlaybackMode::Repitch => "Repitch ▾",
                    SamplerPlaybackMode::Cycles => "Cycles ▾",
                    SamplerPlaybackMode::Textures => "Textures ▾",
                };
                let mode_resp = rep_ui.add(
                    egui::Button::new(RichText::new(mode_lbl).font(fonts::semibold(9.0)).color(theme::TEXT_PRIMARY))
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::NONE),
                );
                if mode_resp.clicked() {
                    sampler.playback_mode = match sampler.playback_mode {
                        SamplerPlaybackMode::Repitch => SamplerPlaybackMode::Cycles,
                        SamplerPlaybackMode::Cycles => SamplerPlaybackMode::Textures,
                        SamplerPlaybackMode::Textures => SamplerPlaybackMode::Repitch,
                    };
                    changed = true;
                }
                rep_ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.vertical(|ui| {
                        let (r_spd, c_spd) = rotary_knob(
                            ui,
                            20.0,
                            &mut sampler.speed,
                            0.25..=2.0,
                            1.0,
                            Color32::from_rgb(0xB8, 0xBC, 0xC6),
                            false,
                        );
                        r_spd.on_hover_text(format!("Speed: {:.0}%", sampler.speed * 100.0));
                        if c_spd {
                            changed = true;
                        }
                        ui.label(RichText::new("Speed").font(fonts::regular(8.0)).color(theme::TEXT_SECONDARY));
                    });
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        if capsule_text_button(
                            ui,
                            vec2(26.0, 13.0),
                            "❄",
                            8.5,
                            sampler.freeze,
                            BW_CYAN,
                            JoinSide::Standalone,
                            false,
                        )
                        .on_hover_text("Freeze Playhead")
                        .clicked()
                        {
                            sampler.freeze = !sampler.freeze;
                            changed = true;
                        }
                        if capsule_text_button(
                            ui,
                            vec2(26.0, 13.0),
                            "RAM",
                            7.5,
                            sampler.ram_mode,
                            BW_GREEN,
                            JoinSide::Standalone,
                            false,
                        )
                        .on_hover_text("RAM Preload")
                        .clicked()
                        {
                            sampler.ram_mode = !sampler.ram_mode;
                            changed = true;
                        }
                    });
                });

                // 2. Offsets Box (48px)
                let (off_rect, _) = ui.allocate_exact_size(vec2(48.0, bot_h), Sense::hover());
                ui.painter().rect_filled(off_rect, 2.5, PANEL_BG);
                ui.painter().rect_stroke(off_rect, 2.5, Stroke::new(1.0_f32, PANEL_BORDER), egui::StrokeKind::Inside);
                let mut off_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(off_rect.shrink(2.0))
                        .layout(egui::Layout::top_down(egui::Align::Center)),
                );
                off_ui.spacing_mut().item_spacing.y = 1.5;
                off_ui.label(RichText::new("Offsets").font(fonts::regular(7.5)).color(theme::TEXT_SECONDARY));
                for (om, lbl, col) in [
                    (SamplerOffsetMode::Play, "PLAY", BW_YELLOW),
                    (SamplerOffsetMode::Loop, "LOOP", BW_CYAN),
                    (SamplerOffsetMode::Len, "LEN", BW_GREEN),
                ] {
                    if capsule_text_button(
                        &mut off_ui,
                        vec2(38.0, 12.0),
                        lbl,
                        7.5,
                        sampler.offset_mode == om,
                        col,
                        JoinSide::Standalone,
                        false,
                    )
                    .clicked()
                    {
                        sampler.offset_mode = om;
                        changed = true;
                    }
                }

                // 3. Filter Box (150px)
                let (flt_rect, _) = ui.allocate_exact_size(vec2(150.0, bot_h), Sense::hover());
                ui.painter().rect_filled(flt_rect, 2.5, PANEL_BG);
                ui.painter().rect_stroke(flt_rect, 2.5, Stroke::new(1.0_f32, PANEL_BORDER), egui::StrokeKind::Inside);
                let mut flt_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(flt_rect.shrink(4.0))
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                flt_ui.spacing_mut().item_spacing.y = 4.0;

                // 6 Filter mode icons
                flt_ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    for (fm, tip) in [
                        (SamplerFilterMode::Off, "Filter Off"),
                        (SamplerFilterMode::LowPass12, "LP 12dB"),
                        (SamplerFilterMode::LowPass24, "LP 24dB"),
                        (SamplerFilterMode::BandPass, "BandPass"),
                        (SamplerFilterMode::HighPass, "HighPass"),
                        (SamplerFilterMode::Notch, "Notch"),
                    ] {
                        let active = sampler.filter_mode == fm;
                        let (ir, iresp) = ui.allocate_exact_size(vec2(20.0, 15.0), Sense::click());
                        if active {
                            ui.painter().rect_filled(ir, 2.0, Color32::from_rgb(0x3E, 0x42, 0x4C));
                        }
                        paint_filter_icon(
                            ui.painter(),
                            ir,
                            fm,
                            if active { BW_ORANGE } else { theme::TEXT_SECONDARY },
                        );
                        if iresp.on_hover_text(tip).clicked() {
                            sampler.filter_mode = fm;
                            changed = true;
                        }
                    }
                });

                // 3 Filter knobs: Cutoff, Res, Keytrack
                flt_ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    ui.vertical(|ui| {
                        let mut cut_norm = cutoff_to_norm(sampler.filter_cutoff_hz);
                        let (r, c) = rotary_knob(ui, 24.0, &mut cut_norm, 0.0..=1.0, cutoff_to_norm(2640.0), BW_ORANGE, false);
                        if c {
                            sampler.filter_cutoff_hz = norm_to_cutoff(cut_norm);
                            changed = true;
                        }
                        r.on_hover_text(format!("Cutoff: {}", format_hz(sampler.filter_cutoff_hz)));
                        ui.label(RichText::new(format_hz(sampler.filter_cutoff_hz)).font(fonts::regular(7.5)).color(theme::TEXT_SECONDARY));
                    });
                    ui.vertical(|ui| {
                        let (r, c) = rotary_knob(
                            ui,
                            24.0,
                            &mut sampler.filter_resonance,
                            0.0..=1.0,
                            0.0,
                            Color32::from_rgb(0xB8, 0xBC, 0xC6),
                            false,
                        );
                        if c {
                            changed = true;
                        }
                        r.on_hover_text(format!("Resonance: {:.0}%", sampler.filter_resonance * 100.0));
                        ui.label(RichText::new("Res").font(fonts::regular(7.5)).color(theme::TEXT_SECONDARY));
                    });
                    ui.vertical(|ui| {
                        let (r, c) = rotary_knob(
                            ui,
                            24.0,
                            &mut sampler.filter_keytrack,
                            0.0..=1.0,
                            0.0,
                            Color32::from_rgb(0xB8, 0xBC, 0xC6),
                            false,
                        );
                        if c {
                            changed = true;
                        }
                        r.on_hover_text(format!("Keytrack: {:.0}%", sampler.filter_keytrack * 100.0));
                        ui.label(RichText::new("Key").font(fonts::regular(7.5)).color(theme::TEXT_SECONDARY));
                    });
                });

                // 4. AHDSR Box (239px)
                let (env_rect, _) = ui.allocate_exact_size(vec2(239.0, bot_h), Sense::hover());
                ui.painter().rect_filled(env_rect, 2.5, PANEL_BG);
                ui.painter().rect_stroke(env_rect, 2.5, Stroke::new(1.0_f32, PANEL_BORDER), egui::StrokeKind::Inside);
                let mut env_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(env_rect.shrink(4.0))
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                env_ui.spacing_mut().item_spacing.y = 3.0;
                env_ui.horizontal(|ui| {
                    ui.label(RichText::new("AHDSR").font(fonts::semibold(8.5)).color(theme::TEXT_PRIMARY));
                    // Mini AHDSR orange curve preview
                    let (cr, _) = ui.allocate_exact_size(vec2(162.0, 20.0), Sense::hover());
                    let a = (sampler.attack_ms / 1500.0).clamp(0.02, 0.22);
                    let h_w = (sampler.hold_ms / 1500.0).clamp(0.0, 0.15);
                    let d = (sampler.decay_ms / 2500.0).clamp(0.05, 0.25);
                    let r = (sampler.release_ms / 2500.0).clamp(0.05, 0.25);
                    let sus_y = cr.bottom() - sampler.sustain.clamp(0.0, 1.0) * (cr.height() - 2.0) - 1.0;
                    let w = cr.width();
                    let pts = vec![
                        pos2(cr.left(), cr.bottom() - 1.0),
                        pos2(cr.left() + a * w, cr.top() + 1.0),
                        pos2(cr.left() + (a + h_w) * w, cr.top() + 1.0),
                        pos2(cr.left() + (a + h_w + d) * w, sus_y),
                        pos2(cr.right() - r * w, sus_y),
                        pos2(cr.right(), cr.bottom() - 1.0),
                    ];
                    ui.painter().add(egui::Shape::line(pts, Stroke::new(1.3_f32, BW_ORANGE)));
                });

                // 5 AHDSR mini knobs: A, H, D, S, R
                env_ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    for (lbl, val, range, def, unit) in [
                        ("A", &mut sampler.attack_ms, 0.0..=1500.0, 2.0, "ms"),
                        ("H", &mut sampler.hold_ms, 0.0..=1500.0, 0.0, "ms"),
                        ("D", &mut sampler.decay_ms, 1.0..=3000.0, 200.0, "ms"),
                        ("S", &mut sampler.sustain, 0.0..=1.0, 1.0, "%"),
                        ("R", &mut sampler.release_ms, 1.0..=3000.0, 50.0, "ms"),
                    ] {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            let (resp, c) = rotary_knob(ui, 22.0, val, range, def, BW_ORANGE, false);
                            if c {
                                changed = true;
                            }
                            let tip = if unit == "%" {
                                format!("{lbl}: {:.0}%", *val * 100.0)
                            } else {
                                format!("{lbl}: {:.0} ms", *val)
                            };
                            resp.on_hover_text(tip);
                            ui.label(RichText::new(lbl).font(fonts::regular(7.5)).color(theme::TEXT_SECONDARY));
                        });
                    }
                });
            });
        });

        // ── COL 4: Right Output & Trigger Panel ─────────────────────────────
        ui.vertical(|ui| {
            ui.set_width(88.0);
            ui.spacing_mut().item_spacing.y = 4.0;

            // Note | Release joined capsule
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if capsule_text_button(
                    ui,
                    vec2(42.0, 19.0),
                    "Note",
                    8.5,
                    sampler.note_trigger,
                    BW_ORANGE,
                    JoinSide::Left,
                    false,
                )
                .on_hover_text("Trigger on Note On")
                .clicked()
                {
                    sampler.note_trigger = !sampler.note_trigger;
                    changed = true;
                }
                if capsule_text_button(
                    ui,
                    vec2(46.0, 19.0),
                    "Release",
                    8.5,
                    sampler.release_trigger,
                    BW_ORANGE,
                    JoinSide::Right,
                    false,
                )
                .on_hover_text("Trigger on Note Off (Release)")
                .clicked()
                {
                    sampler.release_trigger = !sampler.release_trigger;
                    changed = true;
                }
            });

            if capsule_text_button(
                ui,
                vec2(88.0, 19.0),
                "FX",
                8.5,
                sampler.fx_expanded,
                BW_ORANGE,
                JoinSide::Standalone,
                false,
            )
            .clicked()
            {
                sampler.fx_expanded = !sampler.fx_expanded;
            }

            let (out_rect, _) = ui.allocate_exact_size(vec2(88.0, (h - 46.0).max(86.0)), Sense::hover());
            ui.painter().rect_filled(out_rect, 2.5, PANEL_BG);
            ui.painter().rect_stroke(out_rect, 2.5, Stroke::new(1.0_f32, PANEL_BORDER), egui::StrokeKind::Inside);

            let mut out_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(out_rect.shrink2(vec2(4.0, 8.0)))
                    .layout(egui::Layout::top_down(egui::Align::Center)),
            );
            out_ui.spacing_mut().item_spacing.y = 6.0;

            out_ui.horizontal(|ui| {
                ui.label(RichText::new("L").font(fonts::regular(7.5)).color(theme::TEXT_MUTED));
                let (r_pan, c_pan) = rotary_knob(
                    ui,
                    22.0,
                    &mut sampler.pan,
                    -1.0..=1.0,
                    0.0,
                    Color32::from_rgb(0xB8, 0xBC, 0xC6),
                    true,
                );
                r_pan.on_hover_text(format!("Pan: {:+.0}%", sampler.pan * 100.0));
                if c_pan {
                    changed = true;
                }
                ui.label(RichText::new("R").font(fonts::regular(7.5)).color(theme::TEXT_MUTED));
            });

            let (r_out, c_out) = rotary_knob(
                &mut out_ui,
                28.0,
                &mut sampler.output_db,
                -60.0..=6.0,
                0.0,
                BW_ORANGE,
                false,
            );
            r_out.on_hover_text(format!("Output: {:+.1} dB", sampler.output_db));
            if c_out {
                changed = true;
            }
            out_ui.label(RichText::new("Out").font(fonts::regular(8.5)).color(theme::TEXT_SECONDARY));
        });
    });

    if pick_file_requested {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Audio WAV", &["wav", "WAV"])
            .pick_file()
        {
            if let Err(e) = sampler.load_wav(&path) {
                eprintln!("[Hikaru Sampler] Error cargando WAV: {e}");
            } else {
                changed = true;
            }
        }
    }

    if changed {
        sampler.mark_params_dirty();
    }
}
