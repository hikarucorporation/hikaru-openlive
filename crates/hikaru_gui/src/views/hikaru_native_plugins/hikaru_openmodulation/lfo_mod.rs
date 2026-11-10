// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_openmodulation/lfo_mod.rs
// Hikaru OpenModulation - LFO MOD
// Hardware-modeled, tempo-synced LFO modulator for any device knob in Hikaru OpenLive.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use egui::{pos2, vec2, Align2, Color32, Rect, RichText, Sense, Stroke};
use hikaru_dsp::effects::lfo::{
    evaluate_waveform, pseudo_random_step, LfoSyncMode, LfoWaveform, SyncRate,
};

use crate::app::AppState;
use crate::theme;
use crate::ui::bitwig_kit;
use crate::ui::fonts;

pub const MODULATION_CARD_WIDTH: f32 = 426.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModulationTarget {
    #[default]
    OpenWavetableWtPos,
    OpenWavetableDetune,
    OpenWavetablePhase,
    SpectralPhaserOffset,
    SpectralPhaserSpeed,
    SpectralPhaserDepth,
    OpenEq3Low,
    OpenEq3Mid,
    OpenEq3High,
    None,
}

impl ModulationTarget {
    pub const ALL: [ModulationTarget; 10] = [
        ModulationTarget::OpenWavetableWtPos,
        ModulationTarget::OpenWavetableDetune,
        ModulationTarget::OpenWavetablePhase,
        ModulationTarget::SpectralPhaserOffset,
        ModulationTarget::SpectralPhaserSpeed,
        ModulationTarget::SpectralPhaserDepth,
        ModulationTarget::OpenEq3Low,
        ModulationTarget::OpenEq3Mid,
        ModulationTarget::OpenEq3High,
        ModulationTarget::None,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            ModulationTarget::OpenWavetableWtPos => "OpenWavetable: WT POS",
            ModulationTarget::OpenWavetableDetune => "OpenWavetable: DETUNE",
            ModulationTarget::OpenWavetablePhase => "OpenWavetable: PHASE",
            ModulationTarget::SpectralPhaserOffset => "Phaser: Offset",
            ModulationTarget::SpectralPhaserSpeed => "Phaser: Speed",
            ModulationTarget::SpectralPhaserDepth => "Phaser: Depth",
            ModulationTarget::OpenEq3Low => "OpenEQ3: Low Gain",
            ModulationTarget::OpenEq3Mid => "OpenEQ3: Mid Gain",
            ModulationTarget::OpenEq3High => "OpenEQ3: High Gain",
            ModulationTarget::None => "Unassigned (None)",
        }
    }

    pub fn device_name(&self) -> &'static str {
        match self {
            ModulationTarget::OpenWavetableWtPos
            | ModulationTarget::OpenWavetableDetune
            | ModulationTarget::OpenWavetablePhase => "OpenWavetable",
            ModulationTarget::SpectralPhaserOffset
            | ModulationTarget::SpectralPhaserSpeed
            | ModulationTarget::SpectralPhaserDepth => "OpenSpectral Phaser",
            ModulationTarget::OpenEq3Low
            | ModulationTarget::OpenEq3Mid
            | ModulationTarget::OpenEq3High => "OpenEQ3",
            ModulationTarget::None => "",
        }
    }

    pub fn param_name(&self) -> &'static str {
        match self {
            ModulationTarget::OpenWavetableWtPos => "WT POS",
            ModulationTarget::OpenWavetableDetune => "DETUNE",
            ModulationTarget::OpenWavetablePhase => "PHASE",
            ModulationTarget::SpectralPhaserOffset => "Offset",
            ModulationTarget::SpectralPhaserSpeed => "Speed",
            ModulationTarget::SpectralPhaserDepth => "Depth",
            ModulationTarget::OpenEq3Low => "Low Gain",
            ModulationTarget::OpenEq3Mid => "Mid Gain",
            ModulationTarget::OpenEq3High => "High Gain",
            ModulationTarget::None => "",
        }
    }

    pub fn matches(&self, device: &str, param: &str) -> bool {
        if *self == ModulationTarget::None {
            return false;
        }
        let dev_match = match device {
            "OpenWavetable" | "Hikaru OpenWavetable" => self.device_name() == "OpenWavetable",
            "OpenSpectral Phaser" | "OpenSpectralPhaser" => self.device_name() == "OpenSpectral Phaser",
            "OpenEQ3" => self.device_name() == "OpenEQ3",
            _ => device == self.device_name(),
        };
        dev_match && self.param_name().eq_ignore_ascii_case(param)
    }

    pub fn from_device_and_param(device: &str, param: &str) -> Option<Self> {
        for target in &Self::ALL {
            if target.matches(device, param) {
                return Some(*target);
            }
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct LfoModState {
    pub waveform: LfoWaveform,
    pub sync_mode: LfoSyncMode,
    pub rate_hz: f32,
    pub sync_rate: SyncRate,
    pub sync_continuous: f32, // Smooth fractional accumulator for TempoSync mode (0.0 ..= 9.0)
    pub depth: f32,           // 0.0 .. 1.0
    pub phase_deg: f32,       // 0.0 .. 360.0
    pub bipolar: bool,        // true = +/-1.0, false = 0.0 .. 1.0
    pub amount: f32,          // -1.0 .. +1.0 modulation intensity
    pub target: ModulationTarget,
    pub is_mapping: bool,
    last_cycle_index: u64,
    current_sh_val: f32,
}

impl Default for LfoModState {
    fn default() -> Self {
        Self {
            waveform: LfoWaveform::Sine,
            sync_mode: LfoSyncMode::FreeHz,
            rate_hz: 1.0,
            sync_rate: SyncRate::Quarter,
            sync_continuous: 4.0, // Quarter is index 4
            depth: 1.0,
            phase_deg: 0.0,
            bipolar: true,
            amount: 0.5,
            target: ModulationTarget::OpenWavetableWtPos,
            is_mapping: false,
            last_cycle_index: 0,
            current_sh_val: 0.0,
        }
    }
}

impl LfoModState {
    pub fn sync_index(&self) -> usize {
        SyncRate::ALL
            .iter()
            .position(|&r| r == self.sync_rate)
            .unwrap_or(4)
    }

    pub fn set_sync_rate(&mut self, rate: SyncRate) {
        self.sync_rate = rate;
        self.sync_continuous = self.sync_index() as f32;
    }

    pub fn effective_hz(&self, bpm: f32) -> f32 {
        match self.sync_mode {
            LfoSyncMode::FreeHz => self.rate_hz.clamp(0.02, 30.0),
            LfoSyncMode::TempoSync => self.sync_rate.hz_at_bpm(bpm),
        }
    }

    pub fn compute_live_sample(&mut self, time_secs: f32, bpm: f32) -> (f32, f32, f32) {
        let freq = self.effective_hz(bpm);
        let phase_offset = (self.phase_deg / 360.0).rem_euclid(1.0);
        let total_phase = time_secs * freq + phase_offset;
        let cycle_idx = total_phase.floor() as u64;
        let frac_phase = total_phase.rem_euclid(1.0);

        if cycle_idx != self.last_cycle_index || self.current_sh_val == 0.0 {
            self.last_cycle_index = cycle_idx;
            self.current_sh_val = pseudo_random_step(cycle_idx);
        }

        let raw = evaluate_waveform(self.waveform, frac_phase, self.current_sh_val);
        let normalized = if self.bipolar {
            raw * self.depth
        } else {
            ((raw + 1.0) * 0.5) * self.depth
        };
        let delta = normalized * self.amount;
        (frac_phase, normalized, delta)
    }
}

type LfoMap = Mutex<HashMap<(usize, usize), LfoModState>>;
fn lfo_state_map() -> &'static LfoMap {
    static CELL: OnceLock<LfoMap> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(HashMap::new()))
}

type MappingActive = Mutex<Option<(usize, usize)>>;
fn mapping_mode_active() -> &'static MappingActive {
    static CELL: OnceLock<MappingActive> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(None))
}

pub fn get_lfo_state(track_idx: usize, slot_idx: usize) -> LfoModState {
    let mut map = lfo_state_map().lock().unwrap();
    map.entry((track_idx, slot_idx))
        .or_insert_with(LfoModState::default)
        .clone()
}

pub fn update_lfo_state<F: FnOnce(&mut LfoModState)>(track_idx: usize, slot_idx: usize, f: F) {
    let mut map = lfo_state_map().lock().unwrap();
    let entry = map
        .entry((track_idx, slot_idx))
        .or_insert_with(LfoModState::default);
    f(entry);
}

pub fn clock_time_secs() -> f32 {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f32()
}

#[derive(Debug, Clone, Copy)]
pub struct ActiveModulation {
    pub raw_lfo: f32,
    pub delta: f32,
    pub amount: f32,
    pub phase: f32,
}

/// Queries whether a given device knob on track_idx is currently modulated by any active LFO.
pub fn get_modulation(track_idx: usize, device: &str, param: &str) -> Option<ActiveModulation> {
    let time_secs = clock_time_secs();
    let bpm = 120.0_f32; // Default if not in app context
    let mut map = lfo_state_map().lock().unwrap();

    for (&(tr, _slot), lfo) in map.iter_mut() {
        if tr == track_idx && lfo.target.matches(device, param) {
            let (phase, raw, delta) = lfo.compute_live_sample(time_secs, bpm);
            return Some(ActiveModulation {
                raw_lfo: raw,
                delta,
                amount: lfo.amount,
                phase,
            });
        }
    }
    None
}

/// Queries whether mapping mode is active and listening for knob clicks on this track.
pub fn is_mapping_target(_track_idx: usize, _device: &str, _param: &str) -> bool {
    mapping_mode_active().lock().unwrap().is_some()
}

/// Binds a knob as target if mapping mode is active. Returns true if bound.
pub fn handle_knob_mapping_click(track_idx: usize, device: &str, param: &str) -> bool {
    let mut active = mapping_mode_active().lock().unwrap();
    if let Some((map_tr, map_slot)) = *active {
        if map_tr == track_idx {
            if let Some(target) = ModulationTarget::from_device_and_param(device, param) {
                update_lfo_state(map_tr, map_slot, |st| {
                    st.target = target;
                    st.is_mapping = false;
                });
                *active = None;
                return true;
            }
        }
    }
    false
}

/// Renders the complete Hikaru OpenModulation (LFO MOD) device module inside the DSP rack card.
pub fn render_module(ui: &mut egui::Ui, app: &mut AppState, track_idx: usize, slot_idx: usize) {
    // Request continuous repaint for fluid 60fps LFO animation
    ui.ctx().request_repaint();

    let bpm = app.transport.bpm as f32;
    let time_secs = clock_time_secs();

    let mut state = get_lfo_state(track_idx, slot_idx);
    let (current_phase, current_val, current_delta) =
        state.compute_live_sample(time_secs, bpm);

    // Keep global mapping mode synchronized
    let mut is_global_mapping = {
        let active = mapping_mode_active().lock().unwrap();
        *active == Some((track_idx, slot_idx))
    };

    let full_rect = ui.available_rect_before_wrap();
    let body_h = full_rect.height().max(104.0);

    let left_w = 178.0;
    let right_w = (full_rect.width() - left_w - 6.0).max(220.0);

    let left_rect = Rect::from_min_size(full_rect.min, vec2(left_w, body_h));
    let right_rect = Rect::from_min_size(
        pos2(left_rect.max.x + 6.0, full_rect.min.y),
        vec2(right_w, body_h),
    );

    // =========================================================================
    // 1. LEFT COLUMN: Oscilloscope Waveform Display + Shape Selector
    // =========================================================================
    let mut left_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(left_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    left_ui.set_clip_rect(left_rect);
    left_ui.spacing_mut().item_spacing = vec2(0.0, 3.0);

    // Oscilloscope Viewport (176 x 78 px)
    let scope_h = 76.0;
    let (scope_rect, _) =
        left_ui.allocate_exact_size(vec2(left_w, scope_h), Sense::hover());

    let p = left_ui.painter_at(scope_rect);
    // Dark LCD Well Background
    p.rect_filled(scope_rect, 3.5, Color32::from_rgb(0x0A, 0x0C, 0x10));
    p.rect_stroke(
        scope_rect,
        3.5,
        Stroke::new(1.0_f32, Color32::from_rgb(0x22, 0x25, 0x2D)),
        egui::StrokeKind::Inside,
    );

    let cy = scope_rect.center().y;
    // Zero baseline
    p.line_segment(
        [
            pos2(scope_rect.min.x + 2.0, cy),
            pos2(scope_rect.max.x - 2.0, cy),
        ],
        Stroke::new(1.0_f32, Color32::from_rgb(0x18, 0x1C, 0x24)),
    );

    // Subtle Quarter-Phase Grid Lines
    for q in 1..4 {
        let gx = scope_rect.min.x + (q as f32 / 4.0) * scope_rect.width();
        p.line_segment(
            [pos2(gx, scope_rect.min.y + 2.0), pos2(gx, scope_rect.max.y - 2.0)],
            Stroke::new(0.5_f32, Color32::from_rgb(0x16, 0x1A, 0x22)),
        );
    }

    // Render Full-Cycle Waveform Polyline
    let pad_x = 4.0;
    let wave_w = scope_rect.width() - pad_x * 2.0;
    let amp_y = (scope_rect.height() * 0.40) * state.depth;

    let points_count = 64;
    let mut pts = Vec::with_capacity(points_count);
    for i in 0..points_count {
        let norm_x = i as f32 / (points_count - 1) as f32;
        let x = scope_rect.min.x + pad_x + norm_x * wave_w;
        let raw = evaluate_waveform(state.waveform, norm_x, state.current_sh_val);
        let val = if state.bipolar {
            raw
        } else {
            (raw + 1.0) * 0.5 * 2.0 - 1.0
        };
        let y = cy - val * amp_y;
        pts.push(pos2(x, y));
    }

    // Faint outer glow
    p.add(egui::epaint::PathShape::line(
        pts.clone(),
        Stroke::new(2.8_f32, Color32::from_rgba_unmultiplied(0x00, 0xE5, 0xFF, 50)),
    ));
    // Crisp inner trace
    p.add(egui::epaint::PathShape::line(
        pts,
        Stroke::new(1.5_f32, theme::BW_LCD_CYAN),
    ));

    // Animated Phase Scanner Cursor & Glowing Puck
    let puck_x = scope_rect.min.x + pad_x + current_phase * wave_w;
    let puck_val = if state.bipolar {
        current_val / state.depth.max(1e-4)
    } else {
        (current_val / state.depth.max(1e-4)) * 2.0 - 1.0
    };
    let puck_y = cy - puck_val * amp_y;

    // Vertical playhead hair-line
    p.line_segment(
        [
            pos2(puck_x, scope_rect.min.y + 2.0),
            pos2(puck_x, scope_rect.max.y - 2.0),
        ],
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(0x00, 0xE5, 0xFF, 80)),
    );

    // Glowing Dot / Puck
    p.circle_filled(
        pos2(puck_x, puck_y),
        4.5,
        Color32::from_rgba_unmultiplied(0x00, 0xE5, 0xFF, 120),
    );
    p.circle_filled(pos2(puck_x, puck_y), 2.2, Color32::WHITE);

    // Digital Readouts inside Scope
    p.text(
        pos2(scope_rect.max.x - 4.0, scope_rect.min.y + 4.0),
        Align2::RIGHT_TOP,
        format!("{:+0.2}", current_val),
        fonts::lcd(9.5),
        theme::BW_LCD_CYAN,
    );

    let freq_txt = match state.sync_mode {
        LfoSyncMode::FreeHz => format!("{:.2} Hz", state.effective_hz(bpm)),
        LfoSyncMode::TempoSync => format!("{} ({} BPM)", state.sync_rate.label(), bpm.round()),
    };
    p.text(
        pos2(scope_rect.min.x + 4.0, scope_rect.max.y - 4.0),
        Align2::LEFT_BOTTOM,
        freq_txt,
        fonts::regular(8.5),
        Color32::from_rgb(0x8A, 0x90, 0x9C),
    );

    // Waveform Shape Selector Row
    left_ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let btn_w = (left_w - 10.0) / 6.0;

        for wf in &LfoWaveform::ALL {
            let is_sel = state.waveform == *wf;
            let (rect, resp) = ui.allocate_exact_size(vec2(btn_w, 18.0), Sense::click());
            let painter = ui.painter();

            painter.rect_filled(
                rect,
                2.0,
                if is_sel {
                    Color32::from_rgb(0x1F, 0x36, 0x44)
                } else if resp.hovered() {
                    Color32::from_rgb(0x36, 0x3A, 0x44)
                } else {
                    Color32::from_rgb(0x24, 0x27, 0x2E)
                },
            );

            if is_sel {
                painter.rect_stroke(
                    rect,
                    2.0,
                    Stroke::new(1.0_f32, theme::BW_LCD_CYAN),
                    egui::StrokeKind::Inside,
                );
            }

            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                wf.short_symbol(),
                fonts::semibold(9.0),
                if is_sel {
                    theme::BW_LCD_CYAN
                } else {
                    theme::TEXT_SECONDARY
                },
            );

            if resp.on_hover_text(format!("Waveform: {}", wf.label())).clicked() {
                state.waveform = *wf;
            }
        }
    });

    // =========================================================================
    // 2. RIGHT COLUMN: Rotary Knobs + Mod Target Router
    // =========================================================================
    let mut right_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(right_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    right_ui.set_clip_rect(right_rect);
    right_ui.spacing_mut().item_spacing = vec2(0.0, 3.0);

    // Row 1: Primary Knobs (RATE, DEPTH, AMOUNT)
    let knobs_row_h = 50.0;
    let (knobs_row_rect, _) =
        right_ui.allocate_exact_size(vec2(right_w, knobs_row_h), Sense::hover());
    let col_w = (right_w / 3.0).floor();

    for col in 0..3 {
        let cell_rect = Rect::from_min_size(
            pos2(knobs_row_rect.min.x + col as f32 * col_w, knobs_row_rect.min.y),
            vec2(col_w, knobs_row_h),
        );
        let mut cell_ui = right_ui.new_child(
            egui::UiBuilder::new()
                .max_rect(cell_rect)
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );
        cell_ui.spacing_mut().item_spacing.y = 1.0;

        match col {
            0 => {
                // Rate knob (Dual-Mode: TempoSync with persistent fractional accumulator & mouse wheel, or FreeHz with perceptual exponential taper)
                let max_sync_idx = (SyncRate::ALL.len() - 1) as f32;
                const MIN_HZ: f32 = 0.05;
                const MAX_HZ: f32 = 20.0;

                let (resp, display_val) = match state.sync_mode {
                    LfoSyncMode::TempoSync => {
                        let cur_idx = state.sync_index() as f32;
                        if (state.sync_continuous - cur_idx).abs() > 1.0 {
                            state.sync_continuous = cur_idx;
                        }

                        let (r, mut ch) = bitwig_kit::rotary_knob(
                            &mut cell_ui,
                            26.0,
                            &mut state.sync_continuous,
                            0.0..=max_sync_idx,
                            4.0, // Default 1/4 (index 4)
                            theme::BW_LCD_CYAN,
                            false,
                        );

                        // Mouse wheel scroll to easily step through musical divisions
                        let scroll_y = cell_ui.input(|i| i.raw_scroll_delta.y);
                        if r.hovered() && scroll_y.abs() > 0.0 {
                            let step = if scroll_y > 0.0 { 1.0 } else { -1.0 };
                            state.sync_continuous =
                                (state.sync_continuous + step).clamp(0.0, max_sync_idx);
                            ch = true;
                        }

                        if ch {
                            let idx = state
                                .sync_continuous
                                .round()
                                .clamp(0.0, max_sync_idx) as usize;
                            state.sync_rate = SyncRate::ALL[idx];
                        }

                        let r = r.on_hover_text(format!(
                            "Sync Rate: {} (Drag, scroll wheel, or click badge to change)",
                            state.sync_rate.label()
                        ));
                        (r, state.sync_rate.label().to_string())
                    }
                    LfoSyncMode::FreeHz => {
                        // Perceptual exponential taper: 0.05 Hz to 20.0 Hz with 1.00 Hz at center (norm = 0.5)
                        let hz_to_norm = |hz: f32| -> f32 {
                            let ratio = (hz / MIN_HZ).clamp(1.0, MAX_HZ / MIN_HZ);
                            ratio.ln() / (MAX_HZ / MIN_HZ).ln()
                        };
                        let norm_to_hz = |norm: f32| -> f32 {
                            MIN_HZ * (MAX_HZ / MIN_HZ).powf(norm.clamp(0.0, 1.0))
                        };

                        let mut norm = hz_to_norm(state.rate_hz);
                        let (r, mut ch) = bitwig_kit::rotary_knob(
                            &mut cell_ui,
                            26.0,
                            &mut norm,
                            0.0..=1.0,
                            0.5, // 1.00 Hz default
                            theme::BW_LCD_CYAN,
                            false,
                        );

                        // Mouse wheel scroll
                        let scroll_y = cell_ui.input(|i| i.raw_scroll_delta.y);
                        if r.hovered() && scroll_y.abs() > 0.0 {
                            let step = if scroll_y > 0.0 { 0.02 } else { -0.02 };
                            norm = (norm + step).clamp(0.0, 1.0);
                            ch = true;
                        }

                        if ch {
                            state.rate_hz = norm_to_hz(norm);
                        }

                        let r = r.on_hover_text(format!(
                            "Rate: {:.2} Hz (Drag, scroll wheel, or click badge to tune)",
                            state.rate_hz
                        ));
                        (r, format!("{:.2} Hz", state.rate_hz))
                    }
                };
                let _ = resp;

                // Live digital readout badge (clickable to cycle or open menu)
                let badge_btn = egui::Button::new(
                    RichText::new(&display_val)
                        .font(fonts::lcd(9.0))
                        .color(theme::BW_LCD_CYAN),
                )
                .fill(Color32::from_rgb(0x10, 0x14, 0x1C))
                .stroke(Stroke::new(0.5_f32, Color32::from_rgb(0x1F, 0x36, 0x44)))
                .corner_radius(2.0);

                let badge_resp = cell_ui.add(badge_btn);
                if state.sync_mode == LfoSyncMode::TempoSync {
                    if badge_resp.clicked() {
                        let next_idx = (state.sync_index() + 1) % SyncRate::ALL.len();
                        state.set_sync_rate(SyncRate::ALL[next_idx]);
                    }
                    badge_resp.context_menu(|menu_ui| {
                        menu_ui.label(
                            RichText::new("Tempo-Sync Division")
                                .font(fonts::semibold(9.5))
                                .color(theme::ACCENT_ORANGE),
                        );
                        menu_ui.separator();
                        for &rate in &SyncRate::ALL {
                            if menu_ui
                                .selectable_label(state.sync_rate == rate, rate.label())
                                .clicked()
                            {
                                state.set_sync_rate(rate);
                                menu_ui.close_menu();
                            }
                        }
                    });
                } else {
                    badge_resp.context_menu(|menu_ui| {
                        menu_ui.label(
                            RichText::new("Rate Presets")
                                .font(fonts::semibold(9.5))
                                .color(theme::ACCENT_ORANGE),
                        );
                        menu_ui.separator();
                        for preset_hz in [0.1, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0] {
                            if menu_ui
                                .selectable_label(
                                    (state.rate_hz - preset_hz).abs() < 0.05,
                                    format!("{preset_hz:.2} Hz"),
                                )
                                .clicked()
                            {
                                state.rate_hz = preset_hz;
                                menu_ui.close_menu();
                            }
                        }
                    });
                }

                cell_ui.label(
                    RichText::new("RATE")
                        .font(fonts::semibold(8.0))
                        .color(theme::TEXT_SECONDARY),
                );
            }
            1 => {
                // Depth knob
                let (r, mut ch) = bitwig_kit::rotary_knob(
                    &mut cell_ui,
                    26.0,
                    &mut state.depth,
                    0.0..=1.0,
                    1.0,
                    theme::ACCENT_ORANGE,
                    false,
                );
                let scroll_y = cell_ui.input(|i| i.raw_scroll_delta.y);
                if r.hovered() && scroll_y.abs() > 0.0 {
                    let step = if scroll_y > 0.0 { 0.05 } else { -0.05 };
                    state.depth = (state.depth + step).clamp(0.0, 1.0);
                    ch = true;
                }
                let _ = ch;
                r.on_hover_text(format!("Depth: {:.0}%", state.depth * 100.0));

                let badge_btn = egui::Button::new(
                    RichText::new(format!("{:.0}%", state.depth * 100.0))
                        .font(fonts::lcd(9.0))
                        .color(theme::ACCENT_ORANGE),
                )
                .fill(Color32::from_rgb(0x16, 0x12, 0x10))
                .stroke(Stroke::new(0.5_f32, Color32::from_rgb(0x44, 0x24, 0x1B)))
                .corner_radius(2.0);
                cell_ui.add(badge_btn);

                cell_ui.label(
                    RichText::new("DEPTH")
                        .font(fonts::semibold(8.0))
                        .color(theme::TEXT_SECONDARY),
                );
            }
            _ => {
                // Amount knob (Target modulation intensity, bipolar -1.0 .. +1.0)
                let (r, mut ch) = bitwig_kit::rotary_knob(
                    &mut cell_ui,
                    26.0,
                    &mut state.amount,
                    -1.0..=1.0,
                    0.5,
                    theme::BW_FADER_AMBER,
                    true,
                );
                let scroll_y = cell_ui.input(|i| i.raw_scroll_delta.y);
                if r.hovered() && scroll_y.abs() > 0.0 {
                    let step = if scroll_y > 0.0 { 0.05 } else { -0.05 };
                    state.amount = (state.amount + step).clamp(-1.0, 1.0);
                    ch = true;
                }
                let _ = ch;
                r.on_hover_text(format!(
                    "Modulation Amount: {:+.0}% (Live Delta: {:+.2})",
                    state.amount * 100.0,
                    current_delta
                ));

                let badge_btn = egui::Button::new(
                    RichText::new(format!("{:+.0}%", state.amount * 100.0))
                        .font(fonts::lcd(9.0))
                        .color(theme::BW_FADER_AMBER),
                )
                .fill(Color32::from_rgb(0x16, 0x14, 0x10))
                .stroke(Stroke::new(0.5_f32, Color32::from_rgb(0x44, 0x36, 0x1A)))
                .corner_radius(2.0);
                cell_ui.add(badge_btn);

                cell_ui.label(
                    RichText::new("AMOUNT")
                        .font(fonts::semibold(8.0))
                        .color(theme::TEXT_SECONDARY),
                );
            }
        }
    }

    // Row 2: Mode Toggles (Sync Mode, Bipolar, Phase)
    right_ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;

        // Hz vs Sync toggle
        let is_sync = state.sync_mode == LfoSyncMode::TempoSync;
        if bitwig_kit::capsule_text_button(
            ui,
            vec2(44.0, 16.0),
            if is_sync { "SYNC" } else { "Hz" },
            8.5,
            is_sync,
            theme::BW_LCD_CYAN,
            bitwig_kit::JoinSide::Standalone,
            false,
        )
        .on_hover_text("Toggle Tempo-Sync / Free Hz")
        .clicked()
        {
            if is_sync {
                state.sync_mode = LfoSyncMode::FreeHz;
            } else {
                state.sync_mode = LfoSyncMode::TempoSync;
                state.sync_continuous = state.sync_index() as f32;
            }
        }

        // Bipolar vs Unipolar toggle
        if bitwig_kit::capsule_text_button(
            ui,
            vec2(46.0, 16.0),
            if state.bipolar { "+ / -" } else { "+ 0..1" },
            8.0,
            state.bipolar,
            theme::BW_FADER_AMBER,
            bitwig_kit::JoinSide::Standalone,
            false,
        )
        .on_hover_text("Toggle Bipolar (+/-) or Unipolar (0..1) Modulation")
        .clicked()
        {
            state.bipolar = !state.bipolar;
        }

        // Phase Slider / Readout
        ui.label(
            RichText::new(format!("Ø {:3.0}°", state.phase_deg))
                .font(fonts::lcd(9.0))
                .color(theme::TEXT_MUTED),
        );
        let mut phase_norm = state.phase_deg / 360.0;
        if bitwig_kit::fader_horizontal_range(
            ui,
            vec2(58.0, 11.0),
            &mut phase_norm,
            0.0..=1.0,
            0.0,
            theme::BW_FADER_AMBER,
            theme::ACCENT_ORANGE,
            false,
        )
        .1
        {
            state.phase_deg = phase_norm * 360.0;
        }
    });

    // Row 3: Modulation Target Routing & Interactive Mapping (Bitwig Style)
    right_ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;

        // Interactive "MOD" Mapping Button
        let mod_btn_rect = ui.allocate_exact_size(vec2(50.0, 19.0), Sense::click());
        let p_btn = ui.painter_at(mod_btn_rect.0);

        let map_active = is_global_mapping || state.is_mapping;
        let btn_bg = if map_active {
            Color32::from_rgb(0x00, 0xE5, 0xFF)
        } else if mod_btn_rect.1.hovered() {
            Color32::from_rgb(0x28, 0x48, 0x58)
        } else {
            Color32::from_rgb(0x1B, 0x2A, 0x34)
        };
        p_btn.rect_filled(mod_btn_rect.0, 3.0, btn_bg);
        p_btn.rect_stroke(
            mod_btn_rect.0,
            3.0,
            Stroke::new(
                1.0_f32,
                if map_active {
                    Color32::WHITE
                } else {
                    theme::BW_LCD_CYAN
                },
            ),
            egui::StrokeKind::Inside,
        );
        p_btn.text(
            mod_btn_rect.0.center(),
            Align2::CENTER_CENTER,
            if map_active { "MAP..." } else { "⇄ MOD" },
            fonts::semibold(9.5),
            if map_active {
                Color32::from_rgb(0x08, 0x0A, 0x0E)
            } else {
                theme::BW_LCD_CYAN
            },
        );

        if mod_btn_rect.1
            .on_hover_text("Interactive Modulation Assignment: Click then click any device knob (e.g. WT POS) to map")
            .clicked()
        {
            is_global_mapping = !is_global_mapping;
            state.is_mapping = is_global_mapping;
            let mut active = mapping_mode_active().lock().unwrap();
            if is_global_mapping {
                *active = Some((track_idx, slot_idx));
            } else {
                *active = None;
            }
        }

        // Target Selector Dropdown Menu
        let target_label = state.target.label();
        ui.menu_button(
            RichText::new(format!("🎯 {target_label}"))
                .font(fonts::semibold(9.5))
                .color(if state.target != ModulationTarget::None {
                    theme::BW_LCD_CYAN
                } else {
                    theme::TEXT_MUTED
                }),
            |menu_ui| {
                menu_ui.label(
                    RichText::new("Select Modulation Target Knob")
                        .font(fonts::semibold(10.0))
                        .color(theme::ACCENT_ORANGE),
                );
                menu_ui.separator();
                for target in &ModulationTarget::ALL {
                    let sel = state.target == *target;
                    let txt = target.label();
                    if menu_ui
                        .selectable_label(sel, RichText::new(txt).font(fonts::regular(10.0)))
                        .clicked()
                    {
                        state.target = *target;
                        menu_ui.close_menu();
                    }
                }
            },
        );
    });

    // Save state back to map
    update_lfo_state(track_idx, slot_idx, |st| {
        *st = state;
    });
}
