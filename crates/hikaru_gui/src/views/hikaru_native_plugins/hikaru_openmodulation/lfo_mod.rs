// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_openmodulation/lfo_mod.rs
// Hikaru OpenLFO MOD (Bitwig-style Dual LFO MOD Device)
// Dual hardware-modeled, tempo-synced LFO modulator with multi-target Bitwig-style
// drag-to-assign modulation, phase, depth, retrigger, polarity, dual oscilloscope,
// FX slot, and Mix control.

use std::collections::{HashMap, VecDeque};
use std::ops::{Deref, DerefMut};
use std::sync::{Mutex, OnceLock};

use egui::{pos2, vec2, Align2, Color32, CursorIcon, Painter, Pos2, Rect, RichText, Sense, Stroke};
use hikaru_dsp::effects::lfo::{
    evaluate_waveform, pseudo_random_step, LfoSyncMode, LfoWaveform, SyncRate,
};
use hikaru_project_file::{LfoChannelParams, OpenModulationParams};

use crate::app::AppState;
use crate::theme;
use crate::ui::bitwig_kit;
use crate::ui::bitwig_wgpu::paint_rotary_knob;
use crate::ui::fonts;

pub const MODULATION_CARD_WIDTH: f32 = 272.0;
const SCOPE_HISTORY_LEN: usize = 120; // ~2.0 seconds of 60Hz rolling oscilloscope history

const LFO1_WAVE_ORANGE: Color32 = Color32::from_rgb(0xE8, 0x7A, 0x24);
const LFO2_WAVE_BLUE: Color32 = Color32::from_rgb(0x38, 0xA8, 0xE8);
pub const MOD_ARROW_CYAN: Color32 = Color32::from_rgb(0x46, 0xC8, 0xF5);
const MOD_KNOB_CAP_CYAN: Color32 = Color32::from_rgb(0x78, 0xB8, 0xCC);
const PHASE_ARC_SILVER: Color32 = Color32::from_rgb(0xC8, 0xCC, 0xD4);
const PANEL_BG: Color32 = Color32::from_rgb(0x28, 0x2A, 0x30);
const PANEL_BORDER: Color32 = Color32::from_rgb(0x3A, 0x3D, 0x46);
const WELL_BG: Color32 = Color32::from_rgb(0x16, 0x18, 0x1D);
const WELL_BORDER: Color32 = Color32::from_rgb(0x34, 0x37, 0x40);
const RIGHT_COL_BG: Color32 = Color32::from_rgb(0x33, 0x36, 0x3E);
const BTN_IDLE_BG: Color32 = Color32::from_rgb(0x2E, 0x31, 0x38);
const BTN_ACTIVE_BG: Color32 = Color32::from_rgb(0x4A, 0x4E, 0x58);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ModulationTarget {
    OpenWavetableWtPos,
    OpenPolyFold,
    OpenPolySub,
    OpenPolyNoise,
    OpenPolyDrive,
    OpenPolyKtr,
    OpenPolyFilterEnv,
    OpenPolyCutoff,
    OpenPolyReso,
    OpenPolyAmpA,
    OpenPolyAmpD,
    OpenPolyAmpS,
    OpenPolyAmpR,
    OpenPolyFegA,
    OpenPolyFegD,
    OpenPolyFegS,
    OpenPolyFegR,
    OpenPolyVel,
    OpenPolyPitch,
    OpenPolyGlide,
    OpenPolyPan,
    OpenWavetableDetune,
    OpenWavetablePhase,
    OpenWavetableVolume,
    SpectralPhaserOffset,
    SpectralPhaserSpeed,
    SpectralPhaserDepth,
    OpenEq3Low,
    OpenEq3Mid,
    OpenEq3High,
    /// Parámetro público de un plugin externo VST3/CLAP en `(slot_idx, param_id)`.
    ExternalParam {
        slot_idx: usize,
        param_id: u32,
    },
    #[default]
    None,
}

impl ModulationTarget {
    pub const ALL: [ModulationTarget; 31] = [
        ModulationTarget::OpenWavetableWtPos,
        ModulationTarget::OpenPolyFold,
        ModulationTarget::OpenPolySub,
        ModulationTarget::OpenPolyNoise,
        ModulationTarget::OpenPolyDrive,
        ModulationTarget::OpenPolyKtr,
        ModulationTarget::OpenPolyFilterEnv,
        ModulationTarget::OpenPolyCutoff,
        ModulationTarget::OpenPolyReso,
        ModulationTarget::OpenPolyAmpA,
        ModulationTarget::OpenPolyAmpD,
        ModulationTarget::OpenPolyAmpS,
        ModulationTarget::OpenPolyAmpR,
        ModulationTarget::OpenPolyFegA,
        ModulationTarget::OpenPolyFegD,
        ModulationTarget::OpenPolyFegS,
        ModulationTarget::OpenPolyFegR,
        ModulationTarget::OpenPolyVel,
        ModulationTarget::OpenPolyPitch,
        ModulationTarget::OpenPolyGlide,
        ModulationTarget::OpenPolyPan,
        ModulationTarget::OpenWavetableDetune,
        ModulationTarget::OpenWavetablePhase,
        ModulationTarget::OpenWavetableVolume,
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
            ModulationTarget::OpenWavetableWtPos => "OpenPoly: INDEX",
            ModulationTarget::OpenPolyFold => "OpenPoly: FOLD",
            ModulationTarget::OpenPolySub => "OpenPoly: SUB",
            ModulationTarget::OpenPolyNoise => "OpenPoly: NOISE",
            ModulationTarget::OpenPolyDrive => "OpenPoly: DRV",
            ModulationTarget::OpenPolyKtr => "OpenPoly: KTR",
            ModulationTarget::OpenPolyFilterEnv => "OpenPoly: ENV",
            ModulationTarget::OpenPolyCutoff => "OpenPoly: CUTOFF",
            ModulationTarget::OpenPolyReso => "OpenPoly: RESO",
            ModulationTarget::OpenPolyAmpA => "OpenPoly: AMP A",
            ModulationTarget::OpenPolyAmpD => "OpenPoly: AMP D",
            ModulationTarget::OpenPolyAmpS => "OpenPoly: AMP S",
            ModulationTarget::OpenPolyAmpR => "OpenPoly: AMP R",
            ModulationTarget::OpenPolyFegA => "OpenPoly: FEG A",
            ModulationTarget::OpenPolyFegD => "OpenPoly: FEG D",
            ModulationTarget::OpenPolyFegS => "OpenPoly: FEG S",
            ModulationTarget::OpenPolyFegR => "OpenPoly: FEG R",
            ModulationTarget::OpenPolyVel => "OpenPoly: VEL",
            ModulationTarget::OpenPolyPitch => "OpenPoly: PITCH",
            ModulationTarget::OpenPolyGlide => "OpenPoly: GLIDE",
            ModulationTarget::OpenPolyPan => "OpenPoly: PAN",
            ModulationTarget::OpenWavetableDetune => "OpenPoly: DETUNE",
            ModulationTarget::OpenWavetablePhase => "OpenPoly: PHASE",
            ModulationTarget::OpenWavetableVolume => "OpenPoly: VOL",
            ModulationTarget::SpectralPhaserOffset => "Phaser: Offset",
            ModulationTarget::SpectralPhaserSpeed => "Phaser: Speed",
            ModulationTarget::SpectralPhaserDepth => "Phaser: Depth",
            ModulationTarget::OpenEq3Low => "OpenEQ3: Low Gain",
            ModulationTarget::OpenEq3Mid => "OpenEQ3: Mid Gain",
            ModulationTarget::OpenEq3High => "OpenEQ3: High Gain",
            ModulationTarget::ExternalParam { .. } => "Plugin Param",
            ModulationTarget::None => "Unassigned (None)",
        }
    }

    pub fn display_label(&self) -> String {
        match self {
            ModulationTarget::ExternalParam { slot_idx, param_id } => {
                format!("Plugin #{} · Param {}", slot_idx + 1, param_id)
            }
            _ => self.label().to_string(),
        }
    }

    pub fn save_key(&self) -> String {
        match self {
            ModulationTarget::ExternalParam { slot_idx, param_id } => {
                format!("ext:{slot_idx}:{param_id}")
            }
            _ => self.label().to_string(),
        }
    }

    pub fn device_name(&self) -> &'static str {
        match self {
            ModulationTarget::OpenWavetableWtPos
            | ModulationTarget::OpenPolyFold
            | ModulationTarget::OpenPolySub
            | ModulationTarget::OpenPolyNoise
            | ModulationTarget::OpenPolyDrive
            | ModulationTarget::OpenPolyKtr
            | ModulationTarget::OpenPolyFilterEnv
            | ModulationTarget::OpenPolyCutoff
            | ModulationTarget::OpenPolyReso
            | ModulationTarget::OpenPolyAmpA
            | ModulationTarget::OpenPolyAmpD
            | ModulationTarget::OpenPolyAmpS
            | ModulationTarget::OpenPolyAmpR
            | ModulationTarget::OpenPolyFegA
            | ModulationTarget::OpenPolyFegD
            | ModulationTarget::OpenPolyFegS
            | ModulationTarget::OpenPolyFegR
            | ModulationTarget::OpenPolyVel
            | ModulationTarget::OpenPolyPitch
            | ModulationTarget::OpenPolyGlide
            | ModulationTarget::OpenPolyPan
            | ModulationTarget::OpenWavetableDetune
            | ModulationTarget::OpenWavetablePhase
            | ModulationTarget::OpenWavetableVolume => "OpenPoly",
            ModulationTarget::SpectralPhaserOffset
            | ModulationTarget::SpectralPhaserSpeed
            | ModulationTarget::SpectralPhaserDepth => "OpenSpectral Phaser",
            ModulationTarget::OpenEq3Low
            | ModulationTarget::OpenEq3Mid
            | ModulationTarget::OpenEq3High => "OpenEQ3",
            ModulationTarget::ExternalParam { .. } => "ExternalPlugin",
            ModulationTarget::None => "",
        }
    }

    pub fn param_name(&self) -> &'static str {
        match self {
            ModulationTarget::OpenWavetableWtPos => "INDEX",
            ModulationTarget::OpenPolyFold => "FOLD",
            ModulationTarget::OpenPolySub => "SUB",
            ModulationTarget::OpenPolyNoise => "NOISE",
            ModulationTarget::OpenPolyDrive => "DRV",
            ModulationTarget::OpenPolyKtr => "KTR",
            ModulationTarget::OpenPolyFilterEnv => "ENV",
            ModulationTarget::OpenPolyCutoff => "CUTOFF",
            ModulationTarget::OpenPolyReso => "RESO",
            ModulationTarget::OpenPolyAmpA => "AMP_A",
            ModulationTarget::OpenPolyAmpD => "AMP_D",
            ModulationTarget::OpenPolyAmpS => "AMP_S",
            ModulationTarget::OpenPolyAmpR => "AMP_R",
            ModulationTarget::OpenPolyFegA => "FEG_A",
            ModulationTarget::OpenPolyFegD => "FEG_D",
            ModulationTarget::OpenPolyFegS => "FEG_S",
            ModulationTarget::OpenPolyFegR => "FEG_R",
            ModulationTarget::OpenPolyVel => "VEL",
            ModulationTarget::OpenPolyPitch => "PITCH",
            ModulationTarget::OpenPolyGlide => "GLIDE",
            ModulationTarget::OpenPolyPan => "PAN",
            ModulationTarget::OpenWavetableDetune => "DETUNE",
            ModulationTarget::OpenWavetablePhase => "PHASE",
            ModulationTarget::OpenWavetableVolume => "VOL",
            ModulationTarget::SpectralPhaserOffset => "Offset",
            ModulationTarget::SpectralPhaserSpeed => "Speed",
            ModulationTarget::SpectralPhaserDepth => "Depth",
            ModulationTarget::OpenEq3Low => "Low Gain",
            ModulationTarget::OpenEq3Mid => "Mid Gain",
            ModulationTarget::OpenEq3High => "High Gain",
            ModulationTarget::ExternalParam { .. } => "",
            ModulationTarget::None => "",
        }
    }

    pub fn matches(&self, device: &str, param: &str) -> bool {
        if *self == ModulationTarget::None {
            return false;
        }
        let dev_match = match device {
            "OpenPoly" | "Hikaru OpenPoly" | "OpenWavetable" | "Hikaru OpenWavetable" => {
                self.device_name() == "OpenPoly"
            }
            "OpenSpectral Phaser" | "OpenSpectralPhaser" => {
                self.device_name() == "OpenSpectral Phaser"
            }
            "OpenEQ3" => self.device_name() == "OpenEQ3",
            _ => device == self.device_name(),
        };
        let param_match = if *self == ModulationTarget::OpenWavetableWtPos {
            param.eq_ignore_ascii_case("INDEX") || param.eq_ignore_ascii_case("WT POS")
        } else {
            self.param_name().eq_ignore_ascii_case(param)
        };
        dev_match && param_match
    }

    pub fn from_device_and_param(device: &str, param: &str) -> Option<Self> {
        for target in &Self::ALL {
            if target.matches(device, param) {
                return Some(*target);
            }
        }
        None
    }

    pub(crate) fn parse_saved(label: &str, fallback: Self) -> Self {
        if let Some(rest) = label.strip_prefix("ext:") {
            let mut parts = rest.split(':');
            if let (Some(sl), Some(pid)) = (parts.next(), parts.next()) {
                if let (Ok(slot_idx), Ok(param_id)) = (sl.parse::<usize>(), pid.parse::<u32>()) {
                    return ModulationTarget::ExternalParam { slot_idx, param_id };
                }
            }
        }
        Self::ALL
            .iter()
            .find(|t| t.label().eq_ignore_ascii_case(label))
            .copied()
            .or_else(|| match label {
                "OpenWavetable: WT POS" | "OpenPoly: WT POS" => {
                    Some(ModulationTarget::OpenWavetableWtPos)
                }
                "OpenWavetable: DETUNE" => Some(ModulationTarget::OpenWavetableDetune),
                "OpenWavetable: PHASE" => Some(ModulationTarget::OpenWavetablePhase),
                "OpenWavetable: VOL" => Some(ModulationTarget::OpenWavetableVolume),
                _ => None,
            })
            .unwrap_or(fallback)
    }
}

/// Independent LFO channel state (used for LFO1 and LFO2 inside Hikaru OpenLFO MOD).
#[derive(Debug, Clone)]
pub struct LfoChannelState {
    pub waveform: LfoWaveform,
    pub sync_mode: LfoSyncMode,
    pub rate_hz: f32,
    pub sync_rate: SyncRate,
    pub sync_continuous: f32, // Smooth fractional accumulator for TempoSync mode (0.0 ..= 9.0)
    pub depth: f32,           // 0.0 .. 1.0
    pub phase_deg: f32,       // 0.0 .. 360.0
    pub bipolar: bool,        // true = +/-1.0, false = 0.0 .. 1.0
    pub retrigger: bool,      // true = reset phase on transport play/start
    pub amount: f32,          // Legacy single-target modulation intensity (-1.0 .. +1.0)
    pub target: ModulationTarget, // Legacy primary target (kept synced with routes[0])
    pub routes: Vec<(ModulationTarget, f32)>, // Bitwig-style multi-target modulation matrix
    pub is_mapping: bool,
    last_cycle_index: u64,
    current_sh_val: f32,
    scope_history: VecDeque<f32>,
    last_scope_tick: f32,
}

impl Default for LfoChannelState {
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
            retrigger: true,
            amount: 0.5,
            target: ModulationTarget::None,
            routes: Vec::new(),
            is_mapping: false,
            last_cycle_index: 0,
            current_sh_val: 0.0,
            scope_history: VecDeque::with_capacity(SCOPE_HISTORY_LEN),
            last_scope_tick: 0.0,
        }
    }
}

impl LfoChannelState {
    pub fn default_lfo2() -> Self {
        Self::default()
    }

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

    /// Returns the modulation amount (-1.0..+1.0) routed from this channel to `target`.
    pub fn route_amount(&self, target: ModulationTarget) -> f32 {
        if target == ModulationTarget::None {
            return 0.0;
        }
        if let Some((_, amt)) = self.routes.iter().find(|(t, _)| *t == target) {
            return *amt;
        }
        // Fallback for legacy single-target code/tests that set `state.target` directly
        if self.routes.is_empty() && self.target == target {
            return self.amount;
        }
        0.0
    }

    /// Returns the modulation amount matching `(device, param)`.
    pub fn route_amount_for_param(&self, device: &str, param: &str) -> Option<f32> {
        if !self.routes.is_empty() {
            for &(t, amt) in &self.routes {
                if t.matches(device, param) && amt.abs() > 1e-4 {
                    return Some(amt);
                }
            }
            return None;
        }
        if self.target.matches(device, param) {
            return Some(self.amount);
        }
        None
    }

    /// Sets (or clears if near zero) a modulation route to `target` in Bitwig style.
    pub fn set_route(&mut self, target: ModulationTarget, amount: f32) {
        if target == ModulationTarget::None {
            return;
        }
        let clamped = amount.clamp(-1.0, 1.0);
        // Migrate legacy single target into routes if needed
        if self.routes.is_empty() && self.target != ModulationTarget::None && self.target != target
        {
            self.routes.push((self.target, self.amount));
        }

        if clamped.abs() < 0.005 {
            self.routes.retain(|(t, _)| *t != target);
        } else if let Some(entry) = self.routes.iter_mut().find(|(t, _)| *t == target) {
            entry.1 = clamped;
        } else {
            self.routes.push((target, clamped));
        }

        // Keep legacy `target` & `amount` in sync with primary route
        if let Some(&(first_t, first_amt)) = self.routes.first() {
            self.target = first_t;
            self.amount = first_amt;
        } else {
            self.target = ModulationTarget::None;
            self.amount = 0.5;
        }
    }

    pub fn clear_all_routes(&mut self) {
        self.routes.clear();
        self.target = ModulationTarget::None;
    }

    pub fn has_any_route(&self) -> bool {
        !self.routes.is_empty() || self.target != ModulationTarget::None
    }

    pub fn active_routes_snapshot(&self) -> Vec<(ModulationTarget, f32)> {
        if !self.routes.is_empty() {
            self.routes.clone()
        } else if self.target != ModulationTarget::None {
            vec![(self.target, self.amount)]
        } else {
            Vec::new()
        }
    }

    pub fn to_channel_params(&self) -> LfoChannelParams {
        let saved_routes: Vec<(String, f32)> = self
            .active_routes_snapshot()
            .into_iter()
            .map(|(t, a)| (t.save_key(), a))
            .collect();
        LfoChannelParams {
            waveform: waveform_to_str(self.waveform).to_string(),
            sync_mode: sync_mode_to_str(self.sync_mode).to_string(),
            rate_hz: self.rate_hz,
            sync_rate: self.sync_rate.label().to_string(),
            depth: self.depth,
            phase_deg: self.phase_deg,
            bipolar: self.bipolar,
            retrigger: self.retrigger,
            amount: self.amount,
            target: self.target.save_key(),
            routes: saved_routes,
        }
    }

    pub fn apply_channel_params(&mut self, p: &LfoChannelParams, fallback_target: ModulationTarget) {
        self.waveform = parse_waveform(&p.waveform);
        self.sync_mode = parse_sync_mode(&p.sync_mode);
        self.rate_hz = p.rate_hz.clamp(0.02, 30.0);
        let matched_rate = SyncRate::ALL
            .iter()
            .find(|r| r.label().eq_ignore_ascii_case(&p.sync_rate))
            .copied()
            .unwrap_or(SyncRate::Quarter);
        self.set_sync_rate(matched_rate);
        self.depth = p.depth.clamp(0.0, 1.0);
        self.phase_deg = p.phase_deg.clamp(0.0, 360.0);
        self.bipolar = p.bipolar;
        self.retrigger = p.retrigger;
        self.amount = p.amount.clamp(-1.0, 1.0);
        self.target = ModulationTarget::parse_saved(&p.target, fallback_target);
        self.routes.clear();
        for (lbl, amt) in &p.routes {
            let t = ModulationTarget::parse_saved(lbl, ModulationTarget::None);
            if t != ModulationTarget::None && amt.abs() >= 0.005 {
                self.routes.push((t, amt.clamp(-1.0, 1.0)));
            }
        }
        if self.routes.is_empty() && self.target != ModulationTarget::None {
            self.routes.push((self.target, self.amount));
        }
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

    /// Evaluates live LFO sample phase-locked to host transport position and project BPM.
    pub fn compute_live_sample_from_transport(
        &mut self,
        transport: &LfoTransportState,
    ) -> (f32, f32, f32) {
        let phase_offset = (self.phase_deg / 360.0).rem_euclid(1.0);

        if !transport.is_playing {
            let frac_phase = phase_offset;
            let raw = evaluate_waveform(self.waveform, frac_phase, self.current_sh_val);
            let normalized = if self.bipolar {
                raw * self.depth
            } else {
                ((raw + 1.0) * 0.5) * self.depth
            };
            let delta = normalized * self.amount;
            return (frac_phase, normalized, delta);
        }

        let bpm = transport.bpm.clamp(20.0, 400.0);
        let sr = transport.sample_rate.max(1.0);
        let elapsed_samples = if self.retrigger {
            transport
                .sample_count
                .saturating_sub(transport.playback_start_sample)
        } else {
            transport.sample_count
        };

        let (cycle_idx, frac_phase) = match self.sync_mode {
            LfoSyncMode::TempoSync => {
                let samples_per_beat = (sr * 60.0) / bpm;
                let beats = elapsed_samples as f64 / samples_per_beat as f64;
                let beats_per_cycle = self.sync_rate.beats_per_cycle() as f64;
                let total_cycles = (beats / beats_per_cycle) + phase_offset as f64;
                (
                    total_cycles.floor() as u64,
                    total_cycles.rem_euclid(1.0) as f32,
                )
            }
            LfoSyncMode::FreeHz => {
                let seconds = elapsed_samples as f64 / sr as f64;
                let total_cycles = (seconds * self.rate_hz as f64) + phase_offset as f64;
                (
                    total_cycles.floor() as u64,
                    total_cycles.rem_euclid(1.0) as f32,
                )
            }
        };

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

    /// Evaluates continuous live LFO output like Bitwig's LFO MOD:
    /// - When DAW transport is playing, locks sample-accurately to host transport & BPM.
    /// - When DAW transport is stopped, continues oscillating smoothly via wall clock so the
    ///   rolling oscilloscope and target knob modulation rings stay alive at 60 FPS.
    pub fn compute_live_sample_continuous(
        &mut self,
        transport: &LfoTransportState,
    ) -> (f32, f32, f32) {
        if transport.is_playing {
            self.compute_live_sample_from_transport(transport)
        } else {
            self.compute_live_sample(clock_time_secs(), transport.bpm)
        }
    }

    /// Advances the real-time rolling oscilloscope history buffer (60Hz fixed-step scrolling
    /// right-to-left, just like Bitwig Studio's LFO MOD dual scope).
    pub fn push_rolling_scope_samples(&mut self, transport: &LfoTransportState) -> f32 {
        const STEP_SECS: f32 = 1.0 / 60.0;
        let now = clock_time_secs();

        // Pre-populate full history on first render so the scope never starts empty
        if self.scope_history.is_empty() {
            for i in 0..SCOPE_HISTORY_LEN {
                let back = (SCOPE_HISTORY_LEN - 1 - i) as f32 * STEP_SECS;
                let t = (now - back).max(0.0);
                let (_, val, _) = self.compute_live_sample(t, transport.bpm);
                self.scope_history.push_back(val);
            }
            self.last_scope_tick = now;
            return self.scope_history.back().copied().unwrap_or(0.0);
        }

        let elapsed = (now - self.last_scope_tick).max(0.0);
        let steps = ((elapsed / STEP_SECS).floor() as usize).min(6);
        if steps > 0 {
            let (_, live_val, _) = self.compute_live_sample_continuous(transport);
            let prev_val = self.scope_history.back().copied().unwrap_or(live_val);
            for s in 1..=steps {
                let alpha = s as f32 / steps as f32;
                let interp = prev_val + (live_val - prev_val) * alpha;
                if self.scope_history.len() >= SCOPE_HISTORY_LEN {
                    self.scope_history.pop_front();
                }
                self.scope_history.push_back(interp);
            }
            self.last_scope_tick = now;
            live_val
        } else {
            let (_, live_val, _) = self.compute_live_sample_continuous(transport);
            if let Some(last) = self.scope_history.back_mut() {
                *last = live_val;
            }
            live_val
        }
    }
}

/// Complete Hikaru OpenLFO MOD state (Dual LFO1 + LFO2 + FX slot + Mix).
/// Implements `Deref`/`DerefMut` to `lfo1` for seamless backwards compatibility.
#[derive(Debug, Clone)]
pub struct LfoModState {
    pub lfo1: LfoChannelState,
    pub lfo2: LfoChannelState,
    pub mix: f32,      // 0.0 .. 1.0 overall modulation mix
    pub fx_open: bool, // FX slot toggle
}

impl Default for LfoModState {
    fn default() -> Self {
        Self {
            lfo1: LfoChannelState::default(),
            lfo2: LfoChannelState::default_lfo2(),
            mix: 1.0,
            fx_open: false,
        }
    }
}

impl Deref for LfoModState {
    type Target = LfoChannelState;

    fn deref(&self) -> &Self::Target {
        &self.lfo1
    }
}

impl DerefMut for LfoModState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.lfo1
    }
}

impl LfoModState {
    pub fn to_params(&self) -> OpenModulationParams {
        let routes1: Vec<(String, f32)> = self
            .lfo1
            .active_routes_snapshot()
            .into_iter()
            .map(|(t, a)| (t.save_key(), a))
            .collect();
        OpenModulationParams {
            waveform: waveform_to_str(self.lfo1.waveform).to_string(),
            sync_mode: sync_mode_to_str(self.lfo1.sync_mode).to_string(),
            rate_hz: self.lfo1.rate_hz,
            sync_rate: self.lfo1.sync_rate.label().to_string(),
            depth: self.lfo1.depth,
            phase_deg: self.lfo1.phase_deg,
            bipolar: self.lfo1.bipolar,
            amount: self.lfo1.amount,
            target: self.lfo1.target.save_key(),
            retrigger: self.lfo1.retrigger,
            routes: routes1,
            lfo2: Some(self.lfo2.to_channel_params()),
            mix: self.mix,
        }
    }

    pub fn apply_params(&mut self, p: &OpenModulationParams) {
        self.lfo1.waveform = parse_waveform(&p.waveform);
        self.lfo1.sync_mode = parse_sync_mode(&p.sync_mode);
        self.lfo1.rate_hz = p.rate_hz.clamp(0.02, 30.0);
        let matched_rate = SyncRate::ALL
            .iter()
            .find(|r| r.label().eq_ignore_ascii_case(&p.sync_rate))
            .copied()
            .unwrap_or(SyncRate::Quarter);
        self.lfo1.set_sync_rate(matched_rate);
        self.lfo1.depth = p.depth.clamp(0.0, 1.0);
        self.lfo1.phase_deg = p.phase_deg.clamp(0.0, 360.0);
        self.lfo1.bipolar = p.bipolar;
        self.lfo1.retrigger = p.retrigger;
        self.lfo1.amount = p.amount.clamp(-1.0, 1.0);
        self.lfo1.target = ModulationTarget::parse_saved(&p.target, ModulationTarget::None);
        self.lfo1.routes.clear();
        for (lbl, amt) in &p.routes {
            let t = ModulationTarget::parse_saved(lbl, ModulationTarget::None);
            if t != ModulationTarget::None && amt.abs() >= 0.005 {
                self.lfo1.routes.push((t, amt.clamp(-1.0, 1.0)));
            }
        }
        if self.lfo1.routes.is_empty() && self.lfo1.target != ModulationTarget::None {
            self.lfo1.routes.push((self.lfo1.target, self.lfo1.amount));
        }

        if let Some(ref ch2) = p.lfo2 {
            self.lfo2.apply_channel_params(ch2, ModulationTarget::None);
        } else {
            self.lfo2 = LfoChannelState::default_lfo2();
        }
        self.mix = p.mix.clamp(0.0, 1.0);
    }
}

fn waveform_to_str(wf: LfoWaveform) -> &'static str {
    match wf {
        LfoWaveform::Sine => "Sine",
        LfoWaveform::Triangle => "Triangle",
        LfoWaveform::SawUp => "SawUp",
        LfoWaveform::SawDown => "SawDown",
        LfoWaveform::Square => "Square",
        LfoWaveform::SampleAndHold => "SampleAndHold",
    }
}

fn parse_waveform(s: &str) -> LfoWaveform {
    match s {
        "Tri" | "Triangle" => LfoWaveform::Triangle,
        "Saw Up" | "SawUp" => LfoWaveform::SawUp,
        "Saw Dn" | "SawDown" => LfoWaveform::SawDown,
        "Square" => LfoWaveform::Square,
        "S&H" | "SampleAndHold" => LfoWaveform::SampleAndHold,
        _ => LfoWaveform::Sine,
    }
}

fn sync_mode_to_str(mode: LfoSyncMode) -> &'static str {
    match mode {
        LfoSyncMode::FreeHz => "FreeHz",
        LfoSyncMode::TempoSync => "TempoSync",
    }
}

fn parse_sync_mode(s: &str) -> LfoSyncMode {
    match s {
        "TempoSync" | "Sync" => LfoSyncMode::TempoSync,
        _ => LfoSyncMode::FreeHz,
    }
}

type LfoMap = Mutex<HashMap<(usize, usize), LfoModState>>;
fn lfo_state_map() -> &'static LfoMap {
    static CELL: OnceLock<LfoMap> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Identifica la fuente activa del modo mapping estilo Bitwig (flecha cyan `●→`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModSourceId {
    LfoMod {
        track_idx: usize,
        slot_idx: usize,
        channel_idx: u8,
    },
    DeviceBay {
        track_idx: usize,
        slot_idx: usize,
        mod_id: u32,
        out_idx: u8,
    },
}

impl ModSourceId {
    pub fn track_idx(self) -> usize {
        match self {
            Self::LfoMod { track_idx, .. } | Self::DeviceBay { track_idx, .. } => track_idx,
        }
    }
}

type MappingActive = Mutex<Option<ModSourceId>>;
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

/// Global transport state synchronized with the DAW audio engine.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LfoTransportState {
    pub bpm: f32,
    pub sample_count: u64,
    pub sample_rate: f32,
    pub is_playing: bool,
    pub playback_start_sample: u64,
}

impl Default for LfoTransportState {
    fn default() -> Self {
        Self {
            bpm: 120.0,
            sample_count: 0,
            sample_rate: 44100.0,
            is_playing: false,
            playback_start_sample: 0,
        }
    }
}

static GLOBAL_LFO_TRANSPORT: OnceLock<Mutex<LfoTransportState>> = OnceLock::new();

fn global_transport() -> &'static Mutex<LfoTransportState> {
    GLOBAL_LFO_TRANSPORT.get_or_init(|| Mutex::new(LfoTransportState::default()))
}

/// Updates the global modulation transport clock from the DAW state.
pub fn update_global_transport(bpm: f32, sample_count: u64, sample_rate: f32, is_playing: bool) {
    if let Ok(mut lock) = global_transport().lock() {
        let prev_playing = lock.is_playing;
        lock.bpm = bpm.clamp(20.0, 400.0);
        lock.sample_count = sample_count;
        lock.sample_rate = sample_rate.max(1.0);
        lock.is_playing = is_playing;

        if !prev_playing && is_playing {
            lock.playback_start_sample = sample_count;
        } else if !is_playing {
            lock.playback_start_sample = 0;
        } else if sample_count < lock.playback_start_sample {
            lock.playback_start_sample = sample_count;
        }
    }
}

/// Explicit notification that host transport has started playback.
pub fn on_transport_play(start_sample: u64) {
    if let Ok(mut lock) = global_transport().lock() {
        lock.is_playing = true;
        lock.playback_start_sample = start_sample;
    }
}

/// Explicit notification that host transport has stopped or paused playback.
pub fn on_transport_pause_or_stop() {
    if let Ok(mut lock) = global_transport().lock() {
        lock.is_playing = false;
        lock.playback_start_sample = 0;
    }
}

/// Returns the current global LFO transport snapshot.
pub fn get_global_transport() -> LfoTransportState {
    global_transport().lock().map(|l| *l).unwrap_or_default()
}

#[derive(Debug, Clone, Copy)]
pub struct ActiveModulation {
    pub raw_lfo: f32,
    pub delta: f32,
    pub amount: f32,
    pub phase: f32,
}

/// Queries whether a given device knob on track_idx is currently modulated by any active LFO1 or LFO2.
/// Sums contributions from all matching routes across both channels, scaled by `mix`.
pub fn get_modulation(track_idx: usize, device: &str, param: &str) -> Option<ActiveModulation> {
    let transport = get_global_transport();
    let mut map = lfo_state_map().lock().unwrap();

    let mut matched = false;
    let mut total_delta = 0.0_f32;
    let mut total_raw = 0.0_f32;
    let mut total_amount = 0.0_f32;
    let mut last_phase = 0.0_f32;

    for (&(tr, _slot), state) in map.iter_mut() {
        if tr != track_idx {
            continue;
        }
        let mix = state.mix.clamp(0.0, 1.0);

        if let Some(amt1) = state.lfo1.route_amount_for_param(device, param) {
            let (phase, raw, _) = state.lfo1.compute_live_sample_continuous(&transport);
            matched = true;
            total_raw += raw * mix;
            total_delta += raw * amt1 * mix;
            total_amount += amt1 * mix;
            last_phase = phase;
        }
        if let Some(amt2) = state.lfo2.route_amount_for_param(device, param) {
            let (phase, raw, _) = state.lfo2.compute_live_sample_continuous(&transport);
            matched = true;
            total_raw += raw * mix;
            total_delta += raw * amt2 * mix;
            total_amount += amt2 * mix;
            last_phase = phase;
        }
    }

    let (bay_matched, bay_raw, bay_delta, bay_amount, bay_phase) =
        super::bay::evaluate_bay_modulation_for_track(track_idx, device, param, &transport);
    if bay_matched {
        matched = true;
        total_raw += bay_raw;
        total_delta += bay_delta;
        total_amount += bay_amount;
        if last_phase == 0.0 {
            last_phase = bay_phase;
        }
    }

    if matched {
        Some(ActiveModulation {
            raw_lfo: total_raw.clamp(-1.0, 1.0),
            delta: total_delta.clamp(-1.0, 1.0),
            amount: total_amount.clamp(-1.0, 1.0),
            phase: last_phase,
        })
    } else {
        None
    }
}

/// Evalúa todas las modulaciones activas (LFO MOD + Device Modulator Bay +
/// Track Modulators + Project Modulators) dirigidas al parámetro público
/// `(slot_idx, param_id)` de un plugin externo VST3/CLAP en `track_idx`.
pub fn get_external_param_modulation(
    track_idx: usize,
    slot_idx: usize,
    param_id: u32,
) -> Option<ActiveModulation> {
    let target = ModulationTarget::ExternalParam { slot_idx, param_id };
    let transport = get_global_transport();

    let mut matched = false;
    let mut total_delta = 0.0_f32;
    let mut total_raw = 0.0_f32;
    let mut total_amount = 0.0_f32;
    let mut last_phase = 0.0_f32;

    {
        let mut map = lfo_state_map().lock().unwrap();
        for (&(tr, _slot), state) in map.iter_mut() {
            if tr != track_idx {
                continue;
            }
            let mix = state.mix.clamp(0.0, 1.0);
            let amt1 = state.lfo1.route_amount(target);
            if amt1.abs() > 1e-4 {
                let (phase, raw, _) = state.lfo1.compute_live_sample_continuous(&transport);
                matched = true;
                total_raw += raw * mix;
                total_delta += raw * amt1 * mix;
                total_amount += amt1 * mix;
                last_phase = phase;
            }
            let amt2 = state.lfo2.route_amount(target);
            if amt2.abs() > 1e-4 {
                let (phase, raw, _) = state.lfo2.compute_live_sample_continuous(&transport);
                matched = true;
                total_raw += raw * mix;
                total_delta += raw * amt2 * mix;
                total_amount += amt2 * mix;
                last_phase = phase;
            }
        }
    }

    let (bay_matched, bay_raw, bay_delta, bay_amount, bay_phase) =
        super::bay::evaluate_bay_modulation_for_target(track_idx, target, &transport);
    if bay_matched {
        matched = true;
        total_raw += bay_raw;
        total_delta += bay_delta;
        total_amount += bay_amount;
        if last_phase == 0.0 {
            last_phase = bay_phase;
        }
    }

    if matched {
        Some(ActiveModulation {
            raw_lfo: total_raw.clamp(-1.0, 1.0),
            delta: total_delta.clamp(-1.0, 1.0),
            amount: total_amount.clamp(-1.0, 1.0),
            phase: last_phase,
        })
    } else {
        None
    }
}

/// Fuente de mapping activa aplicable a un plugin externo en `(track_idx, slot_idx)`:
/// acepta el Device Modulator Bay del propio plugin, el Track Modulator Bay de la pista,
/// el Project Modulator Bay (`(0, PROJECT_MODULATOR_SLOT)`) y cualquier OpenLFO MOD de la pista.
pub fn active_mapping_source_for_external(track_idx: usize, slot_idx: usize) -> Option<ModSourceId> {
    let active = *mapping_mode_active().lock().unwrap();
    let src = active?;
    match src {
        ModSourceId::LfoMod { track_idx: tr, .. } if tr == track_idx => Some(src),
        ModSourceId::DeviceBay {
            track_idx: tr,
            slot_idx: sl,
            ..
        } if (tr == track_idx
            && (sl == slot_idx || sl == crate::views::remote_controls::TRACK_MODULATOR_SLOT))
            || (tr == 0 && sl == crate::views::remote_controls::PROJECT_MODULATOR_SLOT) =>
        {
            Some(src)
        }
        _ => None,
    }
}

/// Lee el monto de modulación (`-1.0..=1.0`) de `src` hacia un parámetro externo `(slot_idx, param_id)`.
pub fn get_external_route_amount(src: ModSourceId, slot_idx: usize, param_id: u32) -> f32 {
    let target = ModulationTarget::ExternalParam { slot_idx, param_id };
    match src {
        ModSourceId::LfoMod {
            track_idx,
            slot_idx: mod_slot,
            channel_idx,
        } => {
            let st = get_lfo_state(track_idx, mod_slot);
            if channel_idx == 0 {
                st.lfo1.route_amount(target)
            } else {
                st.lfo2.route_amount(target)
            }
        }
        ModSourceId::DeviceBay {
            track_idx,
            slot_idx: bay_slot,
            mod_id,
            out_idx,
        } => super::bay::get_bay_route_amount_target(track_idx, bay_slot, mod_id, out_idx, target),
    }
}

/// Fija (o borra si `|amount| < 0.005`) la ruta de modulación de `src` hacia `(slot_idx, param_id)`.
pub fn set_external_route_amount(
    src: ModSourceId,
    slot_idx: usize,
    param_id: u32,
    amount: f32,
) {
    let target = ModulationTarget::ExternalParam { slot_idx, param_id };
    match src {
        ModSourceId::LfoMod {
            track_idx,
            slot_idx: mod_slot,
            channel_idx,
        } => {
            update_lfo_state(track_idx, mod_slot, |st| {
                if channel_idx == 0 {
                    st.lfo1.set_route(target, amount);
                } else {
                    st.lfo2.set_route(target, amount);
                }
            });
        }
        ModSourceId::DeviceBay {
            track_idx,
            slot_idx: bay_slot,
            mod_id,
            out_idx,
        } => super::bay::set_bay_route_amount_target(
            track_idx, bay_slot, mod_id, out_idx, target, amount,
        ),
    }
}

/// Limpia todas las modulaciones dirigidas al parámetro externo `(slot_idx, param_id)` en `track_idx`.
pub fn clear_external_param_routes(track_idx: usize, slot_idx: usize, param_id: u32) {
    let target = ModulationTarget::ExternalParam { slot_idx, param_id };
    {
        let mut map = lfo_state_map().lock().unwrap();
        for (&(tr, _), st) in map.iter_mut() {
            if tr == track_idx {
                st.lfo1.set_route(target, 0.0);
                st.lfo2.set_route(target, 0.0);
            }
        }
    }
    super::bay::clear_bay_routes_for_target(track_idx, target);
}

/// Returns the active modulation mapping source on `track_idx` (either LFO MOD or Device Modulator Bay).
pub fn active_mapping_source_for_track(track_idx: usize) -> Option<ModSourceId> {
    let active = mapping_mode_active().lock().unwrap();
    active.filter(|src| src.track_idx() == track_idx)
}

/// Returns `true` if any modulator (LFO MOD or Device Modulator Bay) on `track_idx` is in mapping mode.
pub fn is_any_mapping_active_for_track(track_idx: usize) -> bool {
    active_mapping_source_for_track(track_idx).is_some()
}

/// Toggles Bitwig mapping mode for a Device Modulator Bay output.
pub fn toggle_bay_mapping_mode(track_idx: usize, slot_idx: usize, mod_id: u32, out_idx: u8) {
    let target_src = ModSourceId::DeviceBay {
        track_idx,
        slot_idx,
        mod_id,
        out_idx,
    };
    let mut active = mapping_mode_active().lock().unwrap();
    if *active == Some(target_src) {
        *active = None;
    } else {
        *active = Some(target_src);
    }
}

/// Returns `true` if the specified Device Modulator Bay output is currently in mapping mode.
pub fn is_bay_mapping_active(track_idx: usize, slot_idx: usize, mod_id: u32, out_idx: u8) -> bool {
    let active = mapping_mode_active().lock().unwrap();
    *active
        == Some(ModSourceId::DeviceBay {
            track_idx,
            slot_idx,
            mod_id,
            out_idx,
        })
}

/// Returns `Some((track_idx, slot_idx, channel_idx))` if a modulator on `track_idx` is in Bitwig mapping mode.
pub fn active_mapping_for_track(track_idx: usize) -> Option<(usize, usize, u8)> {
    match active_mapping_source_for_track(track_idx)? {
        ModSourceId::LfoMod {
            track_idx,
            slot_idx,
            channel_idx,
        } => Some((track_idx, slot_idx, channel_idx)),
        ModSourceId::DeviceBay {
            track_idx,
            slot_idx,
            out_idx,
            ..
        } => Some((track_idx, slot_idx, out_idx)),
    }
}

/// Exits mapping mode globally.
pub fn clear_mapping_mode() {
    let mut active = mapping_mode_active().lock().unwrap();
    if let Some(ModSourceId::LfoMod {
        track_idx: tr,
        slot_idx: slot,
        ..
    }) = active.take()
    {
        update_lfo_state(tr, slot, |st| {
            st.lfo1.is_mapping = false;
            st.lfo2.is_mapping = false;
        });
    }
}

/// Reads the current modulation amount (-1.0..+1.0) for a specific channel -> (device, param).
pub fn get_channel_route_amount(
    track_idx: usize,
    slot_idx: usize,
    channel_idx: u8,
    device: &str,
    param: &str,
) -> f32 {
    let Some(target) = ModulationTarget::from_device_and_param(device, param) else {
        return 0.0;
    };
    let st = get_lfo_state(track_idx, slot_idx);
    if channel_idx == 0 {
        st.lfo1.route_amount(target)
    } else {
        st.lfo2.route_amount(target)
    }
}

/// Sets the modulation amount (-1.0..+1.0) for a specific channel -> (device, param).
pub fn set_channel_route_amount(
    track_idx: usize,
    slot_idx: usize,
    channel_idx: u8,
    device: &str,
    param: &str,
    amount: f32,
) {
    let Some(target) = ModulationTarget::from_device_and_param(device, param) else {
        return;
    };
    update_lfo_state(track_idx, slot_idx, |st| {
        if channel_idx == 0 {
            st.lfo1.set_route(target, amount);
        } else {
            st.lfo2.set_route(target, amount);
        }
    });
}

/// Queries whether mapping mode is active and listening for knob clicks on this track.
pub fn is_mapping_target(track_idx: usize, _device: &str, _param: &str) -> bool {
    active_mapping_for_track(track_idx).is_some()
}

/// Legacy single-click mapping binding (still used by OpenWavetable & tests).
pub fn handle_knob_mapping_click(track_idx: usize, device: &str, param: &str) -> bool {
    let mut active = mapping_mode_active().lock().unwrap();
    match *active {
        Some(ModSourceId::LfoMod {
            track_idx: map_tr,
            slot_idx: map_slot,
            channel_idx: channel,
        }) if map_tr == track_idx => {
            if let Some(target) = ModulationTarget::from_device_and_param(device, param) {
                update_lfo_state(map_tr, map_slot, |st| {
                    if channel == 0 {
                        st.lfo1.set_route(target, 0.5);
                        st.lfo1.is_mapping = false;
                    } else {
                        st.lfo2.set_route(target, 0.5);
                        st.lfo2.is_mapping = false;
                    }
                });
                *active = None;
                return true;
            }
        }
        Some(ModSourceId::DeviceBay {
            track_idx: map_tr,
            slot_idx: map_slot,
            mod_id,
            out_idx,
        }) if map_tr == track_idx => {
            if ModulationTarget::from_device_and_param(device, param).is_some() {
                super::bay::set_bay_route_amount(
                    map_tr, map_slot, mod_id, out_idx, device, param, 0.5,
                );
                *active = None;
                return true;
            }
        }
        _ => {}
    }
    false
}

/// Draws a Bitwig-style 270° modulation arc around a rotary knob from `from_norm` to `to_norm`.
fn paint_knob_mod_arc(
    p: &Painter,
    center: Pos2,
    radius: f32,
    from_norm: f32,
    to_norm: f32,
    color: Color32,
    stroke_width: f32,
) {
    let a = from_norm.clamp(0.0, 1.0);
    let b = to_norm.clamp(0.0, 1.0);
    if (b - a).abs() < 0.002 {
        return;
    }
    let steps = (((b - a).abs() * 36.0).ceil() as usize).clamp(4, 36);
    let mut pts = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let norm = a + (b - a) * t;
        let deg = -135.0_f32 + norm * 270.0_f32;
        let rad = deg.to_radians();
        pts.push(pos2(center.x + radius * rad.sin(), center.y - radius * rad.cos()));
    }
    p.add(egui::epaint::PathShape::line(pts, Stroke::new(stroke_width, color)));
}

/// Bitwig-style modulatable rotary knob!
///
/// - When **LFO1/LFO2 mapping mode is active** on `track_idx`:
///   - The knob cap glows bright **Bitwig cyan** (`#78B8CC`) with a crisp cyan ring.
///   - Dragging UP/DOWN directly on the knob sets the bipolar modulation amount (`-100%..+100%`)
///     for the active LFO channel **without modifying the knob's base value**.
///   - Double-click clears the modulation route on this knob.
/// - When **mapping mode is NOT active**:
///   - Behaves as a normal `bitwig_kit::rotary_knob`.
///   - If modulated by LFO1/LFO2, paints the cyan configured modulation range arc AND a live
///     animated cyan modulation indicator arc moving in real time!
#[allow(clippy::too_many_arguments)]
pub fn modulatable_rotary_knob(
    ui: &mut egui::Ui,
    diameter: f32,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    default_value: f32,
    arc_color: Color32,
    bipolar: bool,
    track_idx: usize,
    device: &str,
    param: &str,
) -> (egui::Response, bool) {
    let lo = *range.start();
    let hi = *range.end();
    let span = (hi - lo).max(1.0e-6);
    let base_norm = ((*value - lo) / span).clamp(0.0, 1.0);

    // Check if Remote Control Mapping Mode is active
    if crate::views::remote_controls::is_global_remote_mapping_active() {
        let (rect, response) = ui.allocate_exact_size(vec2(diameter, diameter), Sense::click());
        let (_, _) = (base_norm, bipolar);
        paint_rotary_knob(
            ui.painter(),
            rect,
            base_norm,
            Color32::from_rgb(0x36, 0x39, 0x41),
            arc_color,
            response.hovered(),
            bipolar,
        );
        crate::views::remote_controls::paint_mappable_overlay(ui, rect);
        if response.clicked() {
            crate::views::remote_controls::request_global_knob_remote_map(track_idx, param);
        }
        return (response, false);
    }

    // Check if Bitwig modulation mapping mode is active on this track
    if let Some(mod_src) = active_mapping_source_for_track(track_idx) {
        let (rect, response) =
            ui.allocate_exact_size(vec2(diameter, diameter), Sense::click_and_drag());

        let mut route_amt = match mod_src {
            ModSourceId::LfoMod {
                track_idx: map_tr,
                slot_idx: map_slot,
                channel_idx: ch_idx,
            } => get_channel_route_amount(map_tr, map_slot, ch_idx, device, param),
            ModSourceId::DeviceBay {
                track_idx: map_tr,
                slot_idx: map_slot,
                mod_id,
                out_idx,
            } => super::bay::get_bay_route_amount(map_tr, map_slot, mod_id, out_idx, device, param),
        };
        let mut amt_changed = false;

        if response.double_clicked() {
            route_amt = 0.0;
            amt_changed = true;
        } else if response.dragged() {
            let fine = ui.input(|i| i.modifiers.shift);
            let sens = if fine { 0.0015 } else { 0.006 };
            let delta = -response.drag_delta().y * sens;
            if delta.abs() > 0.0 {
                route_amt = (route_amt + delta).clamp(-1.0, 1.0);
                amt_changed = true;
            }
        }
        let scroll_y = ui.input(|i| i.raw_scroll_delta.y);
        if response.hovered() && scroll_y.abs() > 0.0 {
            let step = if scroll_y > 0.0 { 0.05 } else { -0.05 };
            route_amt = (route_amt + step).clamp(-1.0, 1.0);
            amt_changed = true;
        }
        if amt_changed {
            match mod_src {
                ModSourceId::LfoMod {
                    track_idx: map_tr,
                    slot_idx: map_slot,
                    channel_idx: ch_idx,
                } => set_channel_route_amount(map_tr, map_slot, ch_idx, device, param, route_amt),
                ModSourceId::DeviceBay {
                    track_idx: map_tr,
                    slot_idx: map_slot,
                    mod_id,
                    out_idx,
                } => super::bay::set_bay_route_amount(
                    map_tr, map_slot, mod_id, out_idx, device, param, route_amt,
                ),
            }
        }

        if ui.is_rect_visible(rect) {
            // Paint the rotary knob with Bitwig's signature cyan-lit cap!
            paint_rotary_knob(
                ui.painter(),
                rect,
                base_norm,
                MOD_KNOB_CAP_CYAN,
                arc_color,
                response.hovered() || response.dragged(),
                bipolar,
            );
            let p = ui.painter();
            let c = rect.center();
            let r = diameter * 0.5;
            // Subtle cyan glow ring around the knob
            p.circle_stroke(
                c,
                r - 0.5,
                Stroke::new(
                    if response.hovered() || response.dragged() {
                        1.8_f32
                    } else {
                        1.1_f32
                    },
                    if response.hovered() || response.dragged() {
                        Color32::from_rgb(0x74, 0xE2, 0xFF)
                    } else {
                        Color32::from_rgba_unmultiplied(0x46, 0xC8, 0xF5, 190)
                    },
                ),
            );
            // Paint configured modulation depth arc in bright cyan
            if route_amt.abs() > 0.004 {
                paint_knob_mod_arc(
                    p,
                    c,
                    r - 1.5,
                    base_norm,
                    (base_norm + route_amt).clamp(0.0, 1.0),
                    MOD_ARROW_CYAN,
                    2.6_f32,
                );
            }
        }

        if response.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::None);
            paint_drag_value_pill(
                ui.ctx(),
                rect.center_top(),
                &format!("{:+.1} %", route_amt * 100.0),
            );
        } else if response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
        }

        let ch_lbl = match mod_src {
            ModSourceId::LfoMod { channel_idx: 0, .. } => "LFO1",
            ModSourceId::LfoMod { .. } => "LFO2",
            ModSourceId::DeviceBay { .. } => "MOD",
        };
        let tip = if route_amt.abs() > 0.004 {
            format!(
                "{ch_lbl} → {param}: {:+.0}% (Drag up/down to adjust • Double-click to clear • Esc to finish)",
                route_amt * 100.0
            )
        } else {
            format!(
                "{ch_lbl} → {param}: Drag up/down to modulate (Esc or click {ch_lbl} arrow to finish)"
            )
        };
        let response = response.on_hover_text(tip);
        return (response, false);
    }

    // Normal mode: standard rotary knob + live cyan modulation arc overlay
    let (response, changed) =
        bitwig_kit::rotary_knob(ui, diameter, value, range, default_value, arc_color, bipolar);

    if ui.is_rect_visible(response.rect) {
        if let Some(m) = get_modulation(track_idx, device, param) {
            let p = ui.painter();
            let c = response.rect.center();
            let r = diameter * 0.5 - 1.5;
            let cur_norm = ((*value - lo) / span).clamp(0.0, 1.0);
            // Configured range arc (subtle cyan)
            paint_knob_mod_arc(
                p,
                c,
                r,
                cur_norm,
                (cur_norm + m.amount).clamp(0.0, 1.0),
                Color32::from_rgba_unmultiplied(0x46, 0xC8, 0xF5, 90),
                2.0_f32,
            );
            // Live animated modulation arc (bright cyan)
            let live_norm = (cur_norm + m.delta).clamp(0.0, 1.0);
            paint_knob_mod_arc(p, c, r, cur_norm, live_norm, MOD_ARROW_CYAN, 2.5_f32);
            // Live tip dot
            let deg = -135.0_f32 + live_norm * 270.0_f32;
            let rad = deg.to_radians();
            let tip_pos = pos2(c.x + r * rad.sin(), c.y - r * rad.cos());
            p.circle_filled(tip_pos, 2.0, Color32::WHITE);
        }
    }

    (response, changed)
}

/// Renders the Bitwig-style Hikaru OpenLFO MOD body inside the DSP rack card
/// (to the right of the vertical `OPENLFO MOD` sidebar).
pub fn render_module(ui: &mut egui::Ui, app: &mut AppState, track_idx: usize, slot_idx: usize) {
    ui.ctx().request_repaint();

    // Press Escape to exit Bitwig mapping mode cleanly
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) && active_mapping_for_track(track_idx).is_some()
    {
        clear_mapping_mode();
    }

    let is_playing =
        app.transport.playback_state == hikaru_transport::TransportPlaybackState::Playing;
    update_global_transport(
        app.transport.bpm as f32,
        app.transport.sample_count,
        app.transport.sample_rate.get().max(1.0),
        is_playing,
    );
    let transport = get_global_transport();

    let mut state = get_lfo_state(track_idx, slot_idx);
    let val1 = state.lfo1.push_rolling_scope_samples(&transport);
    let val2 = state.lfo2.push_rolling_scope_samples(&transport);

    let full_rect = ui.available_rect_before_wrap();
    let body_h = full_rect.height().max(210.0);
    let right_col_w = 58.0;
    let gap = 4.0;
    let main_w = (full_rect.width() - right_col_w - gap).max(164.0);

    let main_rect = Rect::from_min_size(full_rect.min, vec2(main_w, body_h));
    let right_rect = Rect::from_min_size(
        pos2(main_rect.max.x + gap, full_rect.min.y),
        vec2(right_col_w, body_h),
    );

    // =========================================================================
    // 1. MAIN LEFT COLUMN: LFO1 Panel + Dual Oscilloscope + LFO2 Panel
    // =========================================================================
    let v_gap = 3.0;
    let lfo_h = ((body_h - v_gap * 2.0) * 0.355).floor().max(72.0);
    let scope_h = (body_h - lfo_h * 2.0 - v_gap * 2.0).max(54.0);

    let lfo1_rect = Rect::from_min_size(main_rect.min, vec2(main_w, lfo_h));
    let scope_rect = Rect::from_min_size(
        pos2(main_rect.min.x, lfo1_rect.max.y + v_gap),
        vec2(main_w, scope_h),
    );
    let lfo2_rect = Rect::from_min_size(
        pos2(main_rect.min.x, scope_rect.max.y + v_gap),
        vec2(main_w, lfo_h),
    );

    render_lfo_channel_panel(
        ui,
        lfo1_rect,
        track_idx,
        slot_idx,
        0,
        "LFO1",
        &mut state.lfo1,
        val1,
        transport.bpm,
    );

    render_dual_scope(ui, scope_rect, &state.lfo1, val1, &state.lfo2, val2);

    render_lfo_channel_panel(
        ui,
        lfo2_rect,
        track_idx,
        slot_idx,
        1,
        "LFO2",
        &mut state.lfo2,
        val2,
        transport.bpm,
    );

    // =========================================================================
    // 2. RIGHT COLUMN: FX Slot Button + Mix Rotary Knob (Bitwig LFO MOD style)
    // =========================================================================
    render_right_column(ui, right_rect, track_idx, slot_idx, &mut state);

    // Persist updated state
    update_lfo_state(track_idx, slot_idx, |st| {
        *st = state;
    });
}

/// Renders one LFO channel panel (LFO1 or LFO2) faithfully matching Bitwig's LFO MOD layout.
#[allow(clippy::too_many_arguments)]
fn render_lfo_channel_panel(
    ui: &mut egui::Ui,
    rect: Rect,
    track_idx: usize,
    slot_idx: usize,
    channel_idx: u8,
    label: &str,
    ch: &mut LfoChannelState,
    live_val: f32,
    bpm: f32,
) {
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 3.0, PANEL_BG);
    p.rect_stroke(
        rect,
        3.0,
        Stroke::new(1.0_f32, PANEL_BORDER),
        egui::StrokeKind::Inside,
    );

    let pad_x = 4.0;
    let pad_y = 3.0;
    let inner = Rect::from_min_max(
        pos2(rect.min.x + pad_x, rect.min.y + pad_y),
        pos2(rect.max.x - pad_x, rect.max.y - pad_y),
    );

    let bottom_h = 17.0;
    let top_h = (inner.height() - bottom_h - 3.0).max(46.0);
    let top_rect = Rect::from_min_size(inner.min, vec2(inner.width(), top_h));
    let bottom_rect = Rect::from_min_size(
        pos2(inner.min.x, inner.max.y - bottom_h),
        vec2(inner.width(), bottom_h),
    );

    // --- ZONE A (Left): Label ("LFO1"/"LFO2") + Cyan Modulation Routing Icon ---
    let zone_a_w = 34.0;
    let zone_a = Rect::from_min_size(top_rect.min, vec2(zone_a_w, top_h));

    p.text(
        pos2(zone_a.center().x, zone_a.min.y + 10.0),
        Align2::CENTER_CENTER,
        label,
        fonts::semibold(9.0),
        Color32::from_rgb(0xC8, 0xCC, 0xD4),
    );

    let is_global_mapping = {
        let active = mapping_mode_active().lock().unwrap();
        *active
            == Some(ModSourceId::LfoMod {
                track_idx,
                slot_idx,
                channel_idx,
            })
    };
    let map_active = is_global_mapping;
    ch.is_mapping = map_active;
    let active_routes = ch.active_routes_snapshot();
    let is_assigned = !active_routes.is_empty();

    let arrow_rect = Rect::from_center_size(
        pos2(zone_a.center().x, zone_a.min.y + 28.0),
        vec2(26.0, 16.0),
    );
    let arrow_resp = ui.interact(
        arrow_rect,
        ui.id().with(("lfo_mod_route", track_idx, slot_idx, channel_idx)),
        Sense::click(),
    );
    if arrow_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    if map_active {
        p.rect_filled(arrow_rect, 3.0, MOD_ARROW_CYAN);
    } else if is_assigned {
        // Subtle live breathing glow on the routing arrow in rhythm with the LFO output
        let pulse = (live_val.abs().clamp(0.0, 1.0) * 55.0) as u8;
        p.rect_filled(
            arrow_rect,
            3.0,
            Color32::from_rgba_unmultiplied(0x46, 0xC8, 0xF5, 18 + pulse),
        );
    } else if arrow_resp.hovered() {
        p.rect_filled(arrow_rect, 3.0, Color32::from_rgb(0x22, 0x38, 0x44));
    }
    paint_mod_routing_arrow(
        &p,
        arrow_rect.center(),
        if map_active {
            Color32::from_rgb(0x10, 0x14, 0x1A)
        } else if is_assigned {
            MOD_ARROW_CYAN
        } else {
            Color32::from_rgb(0x52, 0x8A, 0xA4)
        },
        is_assigned || map_active,
    );

    let route_tip = if map_active {
        format!("{label}: Mapping active — Drag any highlighted cyan knob up/down • Click again or Esc to stop")
    } else if is_assigned {
        let summary: Vec<String> = active_routes
            .iter()
            .map(|(t, a)| format!("{} ({:+.0}%)", t.label(), a * 100.0))
            .collect();
        format!(
            "{label} → {}\nLeft-click: Map knobs • Right-click: Manage routes",
            summary.join(", ")
        )
    } else {
        format!("{label}: Not mapped\nLeft-click: Map knobs (drag any cyan knob) • Right-click: Target list")
    };

    let arrow_resp = arrow_resp.on_hover_text(route_tip);
    if arrow_resp.clicked() {
        let next = !map_active;
        ch.is_mapping = next;
        let mut active = mapping_mode_active().lock().unwrap();
        if next {
            *active = Some(ModSourceId::LfoMod {
                track_idx,
                slot_idx,
                channel_idx,
            });
        } else {
            *active = None;
        }
    }
    arrow_resp.context_menu(|menu_ui| {
        menu_ui.label(
            RichText::new(format!("{label} Modulation Routes"))
                .font(fonts::semibold(10.0))
                .color(theme::ACCENT_ORANGE),
        );
        menu_ui.separator();
        let routes_snap = ch.active_routes_snapshot();
        if routes_snap.is_empty() {
            menu_ui.label(
                RichText::new("Not mapped (click o-> then drag any cyan knob)")
                    .font(fonts::regular(9.5))
                    .color(theme::TEXT_MUTED),
            );
        } else {
            for (t, mut amt) in routes_snap {
                menu_ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(t.label())
                            .font(fonts::regular(9.5))
                            .color(MOD_ARROW_CYAN),
                    );
                    if ui
                        .add(
                            egui::Slider::new(&mut amt, -1.0..=1.0)
                                .fixed_decimals(2)
                                .show_value(true),
                        )
                        .changed()
                    {
                        ch.set_route(t, amt);
                    }
                    if ui.small_button("×").clicked() {
                        ch.set_route(t, 0.0);
                    }
                });
            }
            menu_ui.separator();
            if menu_ui.button("Clear All Routes").clicked() {
                ch.clear_all_routes();
                menu_ui.close_menu();
            }
        }
        menu_ui.separator();
        menu_ui.menu_button("Add Target...", |sub_ui| {
            for &target in &ModulationTarget::ALL {
                if target == ModulationTarget::None {
                    continue;
                }
                if sub_ui
                    .selectable_label(ch.route_amount(target).abs() > 0.004, target.label())
                    .clicked()
                {
                    ch.set_route(target, 0.5);
                    sub_ui.close_menu();
                }
            }
        });
    });

    // --- ZONE B (Waveform Preview Box + `±` and `R` Toggles) ---
    let zone_b_x = zone_a.max.x + 3.0;
    let zone_b_w = 40.0;

    let wf_rect = Rect::from_min_size(pos2(zone_b_x, top_rect.min.y + 1.0), vec2(zone_b_w, 22.0));
    let wf_resp = ui
        .interact(
            wf_rect,
            ui.id().with(("lfo_wf_box", track_idx, slot_idx, channel_idx)),
            Sense::click(),
        )
        .on_hover_text(format!(
            "Waveform: {} (Click to cycle, Right-click for list)",
            ch.waveform.label()
        ));
    if wf_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    p.rect_filled(wf_rect, 2.5, WELL_BG);
    p.rect_stroke(
        wf_rect,
        2.5,
        Stroke::new(
            1.0_f32,
            if wf_resp.hovered() {
                LFO1_WAVE_ORANGE
            } else {
                WELL_BORDER
            },
        ),
        egui::StrokeKind::Inside,
    );
    paint_mini_waveform(&p, wf_rect.shrink2(vec2(4.0, 4.0)), ch.waveform, LFO1_WAVE_ORANGE);

    if wf_resp.clicked() {
        let cur_idx = LfoWaveform::ALL
            .iter()
            .position(|&w| w == ch.waveform)
            .unwrap_or(0);
        ch.waveform = LfoWaveform::ALL[(cur_idx + 1) % LfoWaveform::ALL.len()];
    }
    wf_resp.context_menu(|menu_ui| {
        menu_ui.label(
            RichText::new(format!("{label} Waveform"))
                .font(fonts::semibold(9.5))
                .color(theme::ACCENT_ORANGE),
        );
        menu_ui.separator();
        for &wf in &LfoWaveform::ALL {
            if menu_ui
                .selectable_label(ch.waveform == wf, wf.label())
                .clicked()
            {
                ch.waveform = wf;
                menu_ui.close_menu();
            }
        }
    });

    // `±` (Bipolar) and `R` (Retrigger) toggles right below waveform box
    let toggle_y = wf_rect.max.y + 3.0;
    let toggle_h = 17.0;
    let toggle_w = (zone_b_w - 2.0) * 0.5;

    let bip_rect = Rect::from_min_size(pos2(zone_b_x, toggle_y), vec2(toggle_w, toggle_h));
    let bip_resp = ui
        .interact(
            bip_rect,
            ui.id().with(("lfo_bip", track_idx, slot_idx, channel_idx)),
            Sense::click(),
        )
        .on_hover_text(if ch.bipolar {
            "Polarity: Bipolar (±) — Click for Unipolar (0..+1)"
        } else {
            "Polarity: Unipolar (+) — Click for Bipolar (±)"
        });
    if bip_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    p.rect_filled(
        bip_rect,
        2.0,
        if ch.bipolar {
            BTN_ACTIVE_BG
        } else {
            BTN_IDLE_BG
        },
    );
    p.rect_stroke(
        bip_rect,
        2.0,
        Stroke::new(
            0.8_f32,
            if bip_resp.hovered() {
                Color32::from_rgb(0x70, 0x74, 0x80)
            } else {
                PANEL_BORDER
            },
        ),
        egui::StrokeKind::Inside,
    );
    p.text(
        bip_rect.center(),
        Align2::CENTER_CENTER,
        if ch.bipolar { "±" } else { "+" },
        fonts::semibold(10.0),
        if ch.bipolar {
            Color32::WHITE
        } else {
            theme::TEXT_MUTED
        },
    );
    if bip_resp.clicked() {
        ch.bipolar = !ch.bipolar;
    }

    let ret_rect = Rect::from_min_size(
        pos2(bip_rect.max.x + 2.0, toggle_y),
        vec2(toggle_w, toggle_h),
    );
    let ret_resp = ui
        .interact(
            ret_rect,
            ui.id().with(("lfo_ret", track_idx, slot_idx, channel_idx)),
            Sense::click(),
        )
        .on_hover_text(if ch.retrigger {
            "Retrigger: ON (Resets phase on play)"
        } else {
            "Retrigger: OFF (Free-running phase)"
        });
    if ret_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    p.rect_filled(
        ret_rect,
        2.0,
        if ch.retrigger {
            BTN_ACTIVE_BG
        } else {
            BTN_IDLE_BG
        },
    );
    p.rect_stroke(
        ret_rect,
        2.0,
        Stroke::new(
            0.8_f32,
            if ret_resp.hovered() {
                Color32::from_rgb(0x70, 0x74, 0x80)
            } else {
                PANEL_BORDER
            },
        ),
        egui::StrokeKind::Inside,
    );
    p.text(
        ret_rect.center(),
        Align2::CENTER_CENTER,
        "R",
        fonts::semibold(9.0),
        if ch.retrigger {
            Color32::WHITE
        } else {
            theme::TEXT_MUTED
        },
    );
    if ret_resp.clicked() {
        ch.retrigger = !ch.retrigger;
    }

    // --- ZONE C & D (Right): `Phase` Knob and `Depth` Knob ---
    let knobs_x = zone_b_x + zone_b_w + 4.0;
    let knobs_w = (top_rect.max.x - knobs_x).max(68.0);
    let half_k = (knobs_w * 0.5).floor();
    let knob_dia = 25.0;

    // Phase Knob
    let phase_cell = Rect::from_min_size(pos2(knobs_x, top_rect.min.y), vec2(half_k, top_h));
    {
        let mut cell_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(phase_cell)
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );
        cell_ui.spacing_mut().item_spacing.y = 1.0;
        let (r, _) = bitwig_kit::rotary_knob(
            &mut cell_ui,
            knob_dia,
            &mut ch.phase_deg,
            0.0..=360.0,
            0.0,
            PHASE_ARC_SILVER,
            false,
        );
        let scroll_y = cell_ui.input(|i| i.raw_scroll_delta.y);
        if r.hovered() && scroll_y.abs() > 0.0 {
            let step = if scroll_y > 0.0 { 15.0 } else { -15.0 };
            ch.phase_deg = (ch.phase_deg + step).clamp(0.0, 360.0);
        }
        if r.dragged() {
            paint_drag_value_pill(
                ui.ctx(),
                r.rect.center_top(),
                &format!("{:.1}°", ch.phase_deg),
            );
        }
        r.on_hover_text(format!("Phase: {:.0}°", ch.phase_deg));
        cell_ui.label(
            RichText::new("Phase")
                .font(fonts::regular(8.5))
                .color(Color32::from_rgb(0xB4, 0xB8, 0xC0)),
        );
    }

    // Depth Knob
    let depth_cell =
        Rect::from_min_size(pos2(knobs_x + half_k, top_rect.min.y), vec2(half_k, top_h));
    {
        let mut cell_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(depth_cell)
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );
        cell_ui.spacing_mut().item_spacing.y = 1.0;
        let (r, _) = bitwig_kit::rotary_knob(
            &mut cell_ui,
            knob_dia,
            &mut ch.depth,
            0.0..=1.0,
            1.0,
            theme::ACCENT_ORANGE,
            false,
        );
        let scroll_y = cell_ui.input(|i| i.raw_scroll_delta.y);
        if r.hovered() && scroll_y.abs() > 0.0 {
            let step = if scroll_y > 0.0 { 0.05 } else { -0.05 };
            ch.depth = (ch.depth + step).clamp(0.0, 1.0);
        }
        if r.dragged() {
            paint_drag_value_pill(
                ui.ctx(),
                r.rect.center_top(),
                &format!("{:.1} %", ch.depth * 100.0),
            );
        }
        r.on_hover_text(format!("Depth: {:.0}%", ch.depth * 100.0));
        cell_ui.label(
            RichText::new("Depth")
                .font(fonts::regular(8.5))
                .color(Color32::from_rgb(0xB4, 0xB8, 0xC0)),
        );
    }

    // --- BOTTOM ROW: Sync Mode Dropdown Pill + Draggable Orange Rate Value Bar ---
    let sync_btn_w = 38.0;
    let sync_rect = Rect::from_min_size(bottom_rect.min, vec2(sync_btn_w, bottom_h));
    let is_sync = ch.sync_mode == LfoSyncMode::TempoSync;

    let sync_resp = ui
        .interact(
            sync_rect,
            ui.id().with(("lfo_sync_mode", track_idx, slot_idx, channel_idx)),
            Sense::click(),
        )
        .on_hover_text(if is_sync {
            "Rate Mode: Tempo Sync (Click to switch to Free Hz)"
        } else {
            "Rate Mode: Free Hz (Click to switch to Tempo Sync)"
        });
    if sync_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    p.rect_filled(
        sync_rect,
        2.5,
        if is_sync {
            BTN_ACTIVE_BG
        } else {
            BTN_IDLE_BG
        },
    );
    p.rect_stroke(
        sync_rect,
        2.5,
        Stroke::new(
            0.8_f32,
            if sync_resp.hovered() {
                Color32::from_rgb(0x70, 0x74, 0x80)
            } else {
                PANEL_BORDER
            },
        ),
        egui::StrokeKind::Inside,
    );

    let icon_cx = sync_rect.min.x + 13.0;
    let icon_cy = sync_rect.center().y;
    let icon_col = if is_sync {
        theme::ACCENT_ORANGE
    } else {
        Color32::from_rgb(0xCC, 0xD0, 0xD8)
    };
    if is_sync {
        p.circle_filled(pos2(icon_cx - 1.5, icon_cy + 2.5), 2.3, icon_col);
        p.line_segment(
            [pos2(icon_cx + 0.5, icon_cy + 2.5), pos2(icon_cx + 0.5, icon_cy - 4.0)],
            Stroke::new(1.3_f32, icon_col),
        );
        p.line_segment(
            [pos2(icon_cx + 0.5, icon_cy - 4.0), pos2(icon_cx + 4.0, icon_cy - 2.5)],
            Stroke::new(1.3_f32, icon_col),
        );
    } else {
        p.circle_stroke(pos2(icon_cx, icon_cy), 4.5, Stroke::new(1.1_f32, icon_col));
        p.line_segment(
            [pos2(icon_cx, icon_cy), pos2(icon_cx, icon_cy - 2.6)],
            Stroke::new(1.1_f32, icon_col),
        );
        p.line_segment(
            [pos2(icon_cx, icon_cy), pos2(icon_cx + 2.2, icon_cy + 0.8)],
            Stroke::new(1.1_f32, icon_col),
        );
    }
    let tri_cx = sync_rect.max.x - 8.0;
    p.add(egui::Shape::convex_polygon(
        vec![
            pos2(tri_cx - 2.5, icon_cy - 1.2),
            pos2(tri_cx + 2.5, icon_cy - 1.2),
            pos2(tri_cx, icon_cy + 2.0),
        ],
        Color32::from_rgb(0x98, 0x9C, 0xA6),
        Stroke::NONE,
    ));

    if sync_resp.clicked() {
        if is_sync {
            ch.sync_mode = LfoSyncMode::FreeHz;
        } else {
            ch.sync_mode = LfoSyncMode::TempoSync;
            ch.sync_continuous = ch.sync_index() as f32;
        }
    }
    sync_resp.context_menu(|menu_ui| {
        if menu_ui
            .selectable_label(ch.sync_mode == LfoSyncMode::FreeHz, "Free (Hz)")
            .clicked()
        {
            ch.sync_mode = LfoSyncMode::FreeHz;
            menu_ui.close_menu();
        }
        if menu_ui
            .selectable_label(ch.sync_mode == LfoSyncMode::TempoSync, "Tempo Sync (Beats)")
            .clicked()
        {
            ch.sync_mode = LfoSyncMode::TempoSync;
            ch.sync_continuous = ch.sync_index() as f32;
            menu_ui.close_menu();
        }
    });

    // Rate Value Bar (Bitwig dark pill with orange tick + orange text `1.00 Hz`)
    let rate_rect = Rect::from_min_max(
        pos2(sync_rect.max.x + 4.0, bottom_rect.min.y),
        bottom_rect.max,
    );
    let rate_resp = ui.interact(
        rate_rect,
        ui.id().with(("lfo_rate_bar", track_idx, slot_idx, channel_idx)),
        Sense::click_and_drag(),
    );
    if rate_resp.hovered() || rate_resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
    }

    const MIN_HZ: f32 = 0.05;
    const MAX_HZ: f32 = 20.0;
    let max_sync_idx = (SyncRate::ALL.len() - 1) as f32;

    let hz_to_norm = |hz: f32| -> f32 {
        let ratio = (hz / MIN_HZ).clamp(1.0, MAX_HZ / MIN_HZ);
        ratio.ln() / (MAX_HZ / MIN_HZ).ln()
    };
    let norm_to_hz = |norm: f32| -> f32 { MIN_HZ * (MAX_HZ / MIN_HZ).powf(norm.clamp(0.0, 1.0)) };

    let drag_delta = rate_resp.drag_delta();
    let scroll_y = ui.input(|i| i.raw_scroll_delta.y);
    let fine = ui.input(|i| i.modifiers.shift);
    let sens = if fine { 0.0015 } else { 0.006 };

    match ch.sync_mode {
        LfoSyncMode::FreeHz => {
            let mut norm = hz_to_norm(ch.rate_hz);
            let mut changed = false;
            if rate_resp.dragged() {
                let d = drag_delta.x - drag_delta.y;
                if d.abs() > 0.0 {
                    norm = (norm + d * sens).clamp(0.0, 1.0);
                    changed = true;
                }
            }
            if rate_resp.hovered() && scroll_y.abs() > 0.0 {
                norm = (norm + if scroll_y > 0.0 { 0.025 } else { -0.025 }).clamp(0.0, 1.0);
                changed = true;
            }
            if rate_resp.double_clicked() {
                ch.rate_hz = 1.0;
                changed = false;
            }
            if changed {
                ch.rate_hz = norm_to_hz(norm);
            }
        }
        LfoSyncMode::TempoSync => {
            let cur_idx = ch.sync_index() as f32;
            if (ch.sync_continuous - cur_idx).abs() > 1.0 {
                ch.sync_continuous = cur_idx;
            }
            let mut changed = false;
            if rate_resp.dragged() {
                let d = drag_delta.x - drag_delta.y;
                if d.abs() > 0.0 {
                    ch.sync_continuous =
                        (ch.sync_continuous + d * 0.05).clamp(0.0, max_sync_idx);
                    changed = true;
                }
            }
            if rate_resp.hovered() && scroll_y.abs() > 0.0 {
                let step = if scroll_y > 0.0 { 1.0 } else { -1.0 };
                ch.sync_continuous = (ch.sync_continuous + step).clamp(0.0, max_sync_idx);
                changed = true;
            }
            if rate_resp.double_clicked() {
                ch.set_sync_rate(SyncRate::Quarter);
                changed = false;
            }
            if changed {
                let idx = ch.sync_continuous.round().clamp(0.0, max_sync_idx) as usize;
                ch.sync_rate = SyncRate::ALL[idx];
            }
        }
    }

    p.rect_filled(rate_rect, 2.5, WELL_BG);
    p.rect_stroke(
        rate_rect,
        2.5,
        Stroke::new(
            1.0_f32,
            if rate_resp.hovered() || rate_resp.dragged() {
                LFO1_WAVE_ORANGE
            } else {
                WELL_BORDER
            },
        ),
        egui::StrokeKind::Inside,
    );

    let tick_rect = Rect::from_min_size(
        pos2(rate_rect.min.x + 3.0, rate_rect.min.y + 3.0),
        vec2(2.5, rate_rect.height() - 6.0),
    );
    p.rect_filled(tick_rect, 1.0, LFO1_WAVE_ORANGE);

    let rate_text = match ch.sync_mode {
        LfoSyncMode::FreeHz => format!("{:.2} Hz", ch.rate_hz),
        LfoSyncMode::TempoSync => format!("{} ({:.1} Hz)", ch.sync_rate.label(), ch.effective_hz(bpm)),
    };
    let display_text = match ch.sync_mode {
        LfoSyncMode::FreeHz => format!("{:.2} Hz", ch.rate_hz),
        LfoSyncMode::TempoSync => ch.sync_rate.label().to_string(),
    };
    p.text(
        pos2(rate_rect.max.x - 6.0, rate_rect.center().y),
        Align2::RIGHT_CENTER,
        display_text,
        fonts::lcd(9.5),
        LFO1_WAVE_ORANGE,
    );

    let rate_resp = rate_resp.on_hover_text(format!(
        "{label} Rate: {rate_text}\nDrag or scroll to adjust • Double-click to reset • Right-click for presets"
    ));
    rate_resp.context_menu(|menu_ui| match ch.sync_mode {
        LfoSyncMode::TempoSync => {
            menu_ui.label(
                RichText::new("Tempo-Sync Division")
                    .font(fonts::semibold(9.5))
                    .color(theme::ACCENT_ORANGE),
            );
            menu_ui.separator();
            for &rate in &SyncRate::ALL {
                if menu_ui
                    .selectable_label(ch.sync_rate == rate, rate.label())
                    .clicked()
                {
                    ch.set_sync_rate(rate);
                    menu_ui.close_menu();
                }
            }
        }
        LfoSyncMode::FreeHz => {
            menu_ui.label(
                RichText::new("Rate Presets (Hz)")
                    .font(fonts::semibold(9.5))
                    .color(theme::ACCENT_ORANGE),
            );
            menu_ui.separator();
            for preset_hz in [0.1, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0] {
                if menu_ui
                    .selectable_label(
                        (ch.rate_hz - preset_hz).abs() < 0.05,
                        format!("{preset_hz:.2} Hz"),
                    )
                    .clicked()
                {
                    ch.rate_hz = preset_hz;
                    menu_ui.close_menu();
                }
            }
        }
    });
}

/// Renders the central Dual Oscilloscope (LFO1 orange on top, LFO2 blue on bottom)
/// with live right-to-left scrolling history just like Bitwig Studio's LFO MOD.
fn render_dual_scope(
    ui: &mut egui::Ui,
    rect: Rect,
    lfo1: &LfoChannelState,
    val1: f32,
    lfo2: &LfoChannelState,
    val2: f32,
) {
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 2.5, WELL_BG);
    p.rect_stroke(
        rect,
        2.5,
        Stroke::new(1.0_f32, WELL_BORDER),
        egui::StrokeKind::Inside,
    );

    let mid_y = rect.center().y;
    p.line_segment(
        [pos2(rect.min.x + 1.0, mid_y), pos2(rect.max.x - 1.0, mid_y)],
        Stroke::new(1.0_f32, Color32::from_rgb(0x28, 0x2B, 0x34)),
    );

    let lane1 = Rect::from_min_max(
        pos2(rect.min.x + 2.0, rect.min.y + 2.0),
        pos2(rect.max.x - 2.0, mid_y - 1.0),
    );
    let lane2 = Rect::from_min_max(
        pos2(rect.min.x + 2.0, mid_y + 1.0),
        pos2(rect.max.x - 2.0, rect.max.y - 2.0),
    );

    paint_scope_lane(&p, lane1, lfo1, val1, LFO1_WAVE_ORANGE);
    paint_scope_lane(&p, lane2, lfo2, val2, LFO2_WAVE_BLUE);
}

fn paint_scope_lane(
    p: &Painter,
    lane: Rect,
    ch: &LfoChannelState,
    live_val: f32,
    color: Color32,
) {
    let cy = lane.center().y;
    p.line_segment(
        [pos2(lane.min.x, cy), pos2(lane.max.x, cy)],
        Stroke::new(0.6_f32, Color32::from_rgb(0x20, 0x23, 0x2B)),
    );

    let n = ch.scope_history.len();
    if n < 2 {
        return;
    }

    let amp_y = lane.height() * 0.43;
    let base_y = if ch.bipolar { cy } else { lane.max.y - 2.0 };
    let val_to_y = |v: f32| -> f32 {
        let y = if ch.bipolar {
            cy - v.clamp(-1.0, 1.0) * amp_y
        } else {
            (lane.max.y - 2.0) - v.clamp(0.0, 1.0) * (amp_y * 2.0)
        };
        y.clamp(lane.min.y + 1.5, lane.max.y - 1.5)
    };

    let mut pts = Vec::with_capacity(n);
    for (i, &sample) in ch.scope_history.iter().enumerate() {
        let norm_x = i as f32 / (n - 1) as f32;
        let x = lane.min.x + norm_x * (lane.width() - 2.0);
        pts.push(pos2(x, val_to_y(sample)));
    }

    // Subtle translucent area fill under the rolling curve
    let fill_col = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 22);
    for pair in pts.windows(2) {
        let a = pair[0];
        let b = pair[1];
        p.add(egui::Shape::convex_polygon(
            vec![a, b, pos2(b.x, base_y), pos2(a.x, base_y)],
            fill_col,
            Stroke::NONE,
        ));
    }

    // Soft outer glow + crisp Bitwig trace
    let glow = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 52);
    p.add(egui::epaint::PathShape::line(
        pts.clone(),
        Stroke::new(2.8_f32, glow),
    ));
    p.add(egui::epaint::PathShape::line(
        pts,
        Stroke::new(1.35_f32, color),
    ));

    // Bright leading live head dot on the right edge
    let head_pos = pos2(lane.max.x - 2.0, val_to_y(live_val));
    p.circle_filled(head_pos, 3.2, glow);
    p.circle_filled(head_pos, 1.6, Color32::WHITE);
}

/// Paints a Bitwig-style floating numeric value pill above a knob while dragging.
fn paint_drag_value_pill(ctx: &egui::Context, anchor_top: Pos2, text: &str) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("openlfo_drag_pill"),
    ));
    let pill_size = vec2((text.len() as f32 * 5.8 + 12.0).max(40.0), 17.0);
    let pill_rect = Rect::from_center_size(pos2(anchor_top.x, anchor_top.y - 12.0), pill_size);
    painter.rect_filled(pill_rect, 3.0, Color32::from_rgb(0x16, 0x18, 0x1E));
    painter.rect_stroke(
        pill_rect,
        3.0,
        Stroke::new(1.0_f32, Color32::from_rgb(0x4E, 0x52, 0x5E)),
        egui::StrokeKind::Inside,
    );
    painter.text(
        pill_rect.center(),
        Align2::CENTER_CENTER,
        text,
        fonts::lcd(9.5),
        Color32::WHITE,
    );
}

/// Renders the right column containing the `FX` slot button and the `Mix` rotary knob.
fn render_right_column(
    ui: &mut egui::Ui,
    rect: Rect,
    track_idx: usize,
    slot_idx: usize,
    state: &mut LfoModState,
) {
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 3.0, RIGHT_COL_BG);
    p.rect_stroke(
        rect,
        3.0,
        Stroke::new(1.0_f32, Color32::from_rgb(0x46, 0x49, 0x52)),
        egui::StrokeKind::Inside,
    );

    // Top: FX Button
    let fx_rect = Rect::from_min_size(
        pos2(rect.min.x + 5.0, rect.min.y + 5.0),
        vec2(rect.width() - 10.0, 24.0),
    );
    let fx_resp = ui
        .interact(
            fx_rect,
            ui.id().with(("lfo_mod_fx_btn", track_idx, slot_idx)),
            Sense::click(),
        )
        .on_hover_text("Toggle FX Slot");
    if fx_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    p.rect_filled(
        fx_rect,
        2.5,
        if state.fx_open {
            Color32::from_rgb(0x3A, 0x2C, 0x22)
        } else if fx_resp.hovered() {
            Color32::from_rgb(0x30, 0x33, 0x3B)
        } else {
            Color32::from_rgb(0x25, 0x27, 0x2D)
        },
    );
    p.rect_stroke(
        fx_rect,
        2.5,
        Stroke::new(
            1.0_f32,
            if state.fx_open {
                theme::ACCENT_ORANGE
            } else {
                Color32::from_rgb(0x4A, 0x4E, 0x58)
            },
        ),
        egui::StrokeKind::Inside,
    );
    p.text(
        fx_rect.center(),
        Align2::CENTER_CENTER,
        "FX",
        fonts::semibold(9.5),
        if state.fx_open {
            theme::ACCENT_ORANGE
        } else {
            Color32::from_rgb(0xD8, 0xDC, 0xE2)
        },
    );
    if fx_resp.clicked() {
        state.fx_open = !state.fx_open;
    }

    // Bottom: Mix Rotary Knob
    let mix_area = Rect::from_min_max(
        pos2(rect.min.x + 2.0, rect.max.y - 54.0),
        pos2(rect.max.x - 2.0, rect.max.y - 4.0),
    );
    let mut mix_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(mix_area)
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    mix_ui.spacing_mut().item_spacing.y = 2.0;

    let (mix_resp, _) = bitwig_kit::rotary_knob(
        &mut mix_ui,
        34.0,
        &mut state.mix,
        0.0..=1.0,
        1.0,
        theme::ACCENT_ORANGE,
        false,
    );
    let scroll_y = mix_ui.input(|i| i.raw_scroll_delta.y);
    if mix_resp.hovered() && scroll_y.abs() > 0.0 {
        let step = if scroll_y > 0.0 { 0.05 } else { -0.05 };
        state.mix = (state.mix + step).clamp(0.0, 1.0);
    }
    if mix_resp.dragged() {
        paint_drag_value_pill(
            ui.ctx(),
            mix_resp.rect.center_top(),
            &format!("{:.1} %", state.mix * 100.0),
        );
    }
    mix_resp.on_hover_text(format!("Mix: {:.0}%", state.mix * 100.0));
    mix_ui.label(
        RichText::new("Mix")
            .font(fonts::regular(9.0))
            .color(Color32::from_rgb(0xC6, 0xCA, 0xD2)),
    );
}

/// Paints the small Bitwig cyan modulator routing icon (circle + arrow `o->`).
fn paint_mod_routing_arrow(p: &Painter, center: Pos2, color: Color32, filled: bool) {
    let left_x = center.x - 6.0;
    let right_x = center.x + 6.5;
    let y = center.y;

    if filled {
        p.circle_filled(pos2(left_x, y), 2.5, color);
    } else {
        p.circle_stroke(pos2(left_x, y), 2.5, Stroke::new(1.2_f32, color));
    }
    p.line_segment(
        [pos2(left_x + 2.5, y), pos2(right_x - 2.5, y)],
        Stroke::new(1.4_f32, color),
    );
    p.add(egui::Shape::convex_polygon(
        vec![
            pos2(right_x, y),
            pos2(right_x - 4.0, y - 3.0),
            pos2(right_x - 4.0, y + 3.0),
        ],
        color,
        Stroke::NONE,
    ));
}

/// Paints a crisp miniature vector waveform inside the waveform selector box.
fn paint_mini_waveform(p: &Painter, rect: Rect, waveform: LfoWaveform, color: Color32) {
    let cy = rect.center().y;
    let amp = rect.height() * 0.44;
    let steps = 28;
    let mut pts = Vec::with_capacity(steps);
    for i in 0..steps {
        let t = i as f32 / (steps - 1) as f32;
        let x = rect.min.x + t * rect.width();
        let sh = pseudo_random_step((t * 4.0) as u64);
        let v = evaluate_waveform(waveform, t * 1.25, sh);
        pts.push(pos2(x, cy - v * amp));
    }
    p.add(egui::epaint::PathShape::line(pts, Stroke::new(1.3_f32, color)));
}

/// Renders the HDP Preset menu triggered from the left vertical sidebar folder icon.
pub fn render_preset_menu(ui: &mut egui::Ui, track_idx: usize, slot_idx: usize) {
    ui.set_min_width(170.0);
    ui.label(
        RichText::new("Hikaru OpenLFO MOD")
            .font(fonts::semibold(10.0))
            .color(theme::ACCENT_ORANGE),
    );
    ui.separator();
    if ui.button("💾 Guardar Preset (*.hdp)").clicked() {
        let state = get_lfo_state(track_idx, slot_idx);
        save_openmodulation_preset(&state);
        ui.close_menu();
    }
    if ui.button("📂 Cargar Preset (*.hdp)").clicked() {
        if let Some(loaded) = load_openmodulation_preset() {
            update_lfo_state(track_idx, slot_idx, |st| {
                *st = loaded;
            });
        }
        ui.close_menu();
    }
}

pub fn save_openmodulation_preset(state: &LfoModState) {
    if let Some(path) = rfd::FileDialog::new()
        .set_file_name("openlfo_mod.hdp")
        .add_filter(
            hikaru_project_file::HDP_FILE_FILTER,
            &[hikaru_project_file::HDP_EXTENSION],
        )
        .save_file()
    {
        let slot = hikaru_project_file::DspSlot {
            id: 0,
            name: "OpenLFO MOD".to_string(),
            active: true,
            wavetable_path: None,
            plugin_path: None,
            plugin_format: None,
            open_wavetable: None,
            open_modulation: Some(state.to_params()),
            sampler: None,
            open_poly: None,
            open_grid: None,
            modulator_bay: None,
        };
        let preset = hikaru_project_file::HikaruDevicePreset {
            format_version: hikaru_project_file::FORMAT_VERSION,
            created_by: format!("Hikaru OpenLive {}", crate::version::VERSION),
            preset_type: hikaru_project_file::PresetType::SingleDevice,
            name: "OpenLFO MOD Preset".to_string(),
            author: None,
            category: Some("Modulation / LFO".to_string()),
            description: Some("Hikaru OpenLFO MOD preset".to_string()),
            slot: Some(slot),
            chain: None,
        };
        let _ = hikaru_project_file::save_hdp(&path, &preset);
    }
}

pub fn load_openmodulation_preset() -> Option<LfoModState> {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter(
            hikaru_project_file::HDP_FILE_FILTER,
            &[hikaru_project_file::HDP_EXTENSION],
        )
        .pick_file()
    {
        if let Ok(preset) = hikaru_project_file::parse_hdp(&path) {
            if let Some(slot) = preset.slot {
                if let Some(ref p) = slot.open_modulation {
                    let mut st = LfoModState::default();
                    st.apply_params(p);
                    return Some(st);
                }
            }
        }
    }
    None
}
