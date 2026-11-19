/*
 * Hikaru OpenPoly - State Model, Wavetable I/O & Persistence (GNU AGPL v3)
 * Copyright (C) 2026 Hikaru Corporation
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 */

use std::path::Path;
use std::sync::Arc;

use hikaru_dsp::synth::open_poly::{
    OpenPolyDspParams, OpenPolyEnvType, OpenPolyFilterType, OpenPolyLfoWave, OpenPolyOscType,
    OpenPolySubWave, OpenPolyWavetableData,
};
use hikaru_project_file::{OpenPolyParams, OpenWavetableParams};

use crate::views::hikaru_native_plugins::hikaru_openwavetable::wavetable_io::{
    self, Wavetable, FRAME_SAMPLES,
};

const MAX_FRAMES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenPolyLfoTarget {
    Cutoff,
    Index,
    Pitch,
    Amp,
}

impl OpenPolyLfoTarget {
    pub const ALL: [OpenPolyLfoTarget; 4] = [
        OpenPolyLfoTarget::Cutoff,
        OpenPolyLfoTarget::Index,
        OpenPolyLfoTarget::Pitch,
        OpenPolyLfoTarget::Amp,
    ];

    pub fn label(self) -> &'static str {
        match self {
            OpenPolyLfoTarget::Cutoff => "Cutoff",
            OpenPolyLfoTarget::Index => "Index",
            OpenPolyLfoTarget::Pitch => "Pitch",
            OpenPolyLfoTarget::Amp => "Amp",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            OpenPolyLfoTarget::Cutoff => "CUT",
            OpenPolyLfoTarget::Index => "IDX",
            OpenPolyLfoTarget::Pitch => "PIT",
            OpenPolyLfoTarget::Amp => "AMP",
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            OpenPolyLfoTarget::Cutoff => OpenPolyLfoTarget::Index,
            OpenPolyLfoTarget::Index => OpenPolyLfoTarget::Pitch,
            OpenPolyLfoTarget::Pitch => OpenPolyLfoTarget::Amp,
            OpenPolyLfoTarget::Amp => OpenPolyLfoTarget::Cutoff,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OpenPolyFactoryPreset {
    pub name: &'static str,
    pub category: &'static str,
}

pub const FACTORY_PRESETS: [OpenPolyFactoryPreset; 6] = [
    OpenPolyFactoryPreset { name: "Standard Four", category: "Init" },
    OpenPolyFactoryPreset { name: "Analog Brass", category: "Brass" },
    OpenPolyFactoryPreset { name: "Deep Sub Bass", category: "Bass" },
    OpenPolyFactoryPreset { name: "Pluck Lead", category: "Pluck" },
    OpenPolyFactoryPreset { name: "Swarm Pad", category: "Pad" },
    OpenPolyFactoryPreset { name: "Acid Sallen-Key", category: "Acid" },
];

#[derive(Clone, Debug)]
pub struct HikaruOpenPoly {
    // Wavetable data
    pub table: Wavetable,
    pub wt_data: Arc<OpenPolyWavetableData>,

    // Oscillator module
    pub osc_type: OpenPolyOscType,
    pub osc_index: f32,
    pub base_index: Option<f32>,
    pub osc_fold: f32,
    pub osc_ratio_num: u8,
    pub osc_ratio_den: u8,
    pub osc_semitones: f32,
    pub osc_detune_hz: f32,
    pub osc_sync: bool,
    pub unison_voices: u8,
    pub unison_detune: f32,
    pub unison_spread: f32,

    // Sub & Noise
    pub sub_level: f32,
    pub sub_wave: OpenPolySubWave,
    pub sub_octave: i8,
    pub noise_level: f32,

    // Filter module
    pub filter_enabled: bool,
    pub filter_type: OpenPolyFilterType,
    pub filter_cutoff_hz: f32,
    pub filter_resonance: f32,
    pub filter_drive: f32,
    pub filter_keytrack: f32,
    pub filter_env_amount: f32,
    pub filter_vel_amount: f32,

    // Filter Envelope (FEG)
    pub feg_enabled: bool,
    pub feg_attack_ms: f32,
    pub feg_decay_ms: f32,
    pub feg_sustain: f32,
    pub feg_release_ms: f32,

    // Amp Envelope
    pub amp_env_type: OpenPolyEnvType,
    pub amp_attack_ms: f32,
    pub amp_decay_ms: f32,
    pub amp_sustain: f32,
    pub amp_release_ms: f32,

    // Voice / Pitch / Glide
    pub mono_mode: bool,
    pub legato: bool,
    pub glide_ms: f32,
    pub global_pitch_st: f32,
    pub velocity_sens: f32,

    // Built-in Modulator drawer (LFO + Expressions)
    pub modulators_open: bool,
    pub lfo_enabled: bool,
    pub lfo_wave: OpenPolyLfoWave,
    pub lfo_rate_hz: f32,
    pub lfo_selected_target: OpenPolyLfoTarget,
    pub lfo_amount_cutoff: f32,
    pub lfo_amount_index: f32,
    pub lfo_amount_pitch: f32,
    pub lfo_amount_amp: f32,

    // Output
    pub pan: f32,
    pub output_gain: f32,
    pub stereo_width: f32,

    // Visual options
    pub render_mode_3d: bool,
    pub preset_name: String,

    // Incremental audio sync revisions
    pub wavetable_rev: u64,
    pub params_rev: u64,
}

impl Default for HikaruOpenPoly {
    fn default() -> Self {
        let mut table = wavetable_io::bundled_wavetable(MAX_FRAMES)
            .unwrap_or_else(|_| Wavetable::default_table());
        if table.frames <= 1 {
            table = Wavetable::default_table();
            table.name = wavetable_io::BUNDLED_WAVETABLE_NAME.to_string();
        }
        let wt_data = Arc::new(OpenPolyWavetableData::new(
            table.samples.clone(),
            table.frames,
            FRAME_SAMPLES,
        ));
        let d = OpenPolyDspParams::default();
        Self {
            table,
            wt_data,
            osc_type: d.osc_type,
            osc_index: d.osc_index,
            base_index: None,
            osc_fold: d.osc_fold,
            osc_ratio_num: d.osc_ratio_num,
            osc_ratio_den: d.osc_ratio_den,
            osc_semitones: d.osc_semitones,
            osc_detune_hz: d.osc_detune_hz,
            osc_sync: d.osc_sync,
            unison_voices: d.unison_voices,
            unison_detune: d.unison_detune,
            unison_spread: d.unison_spread,

            sub_level: d.sub_level,
            sub_wave: d.sub_wave,
            sub_octave: d.sub_octave,
            noise_level: d.noise_level,

            filter_enabled: d.filter_enabled,
            filter_type: d.filter_type,
            filter_cutoff_hz: d.filter_cutoff_hz,
            filter_resonance: d.filter_resonance,
            filter_drive: d.filter_drive,
            filter_keytrack: d.filter_keytrack,
            filter_env_amount: d.filter_env_amount,
            filter_vel_amount: d.filter_vel_amount,

            feg_enabled: d.feg_enabled,
            feg_attack_ms: d.feg_attack_ms,
            feg_decay_ms: d.feg_decay_ms,
            feg_sustain: d.feg_sustain,
            feg_release_ms: d.feg_release_ms,

            amp_env_type: d.amp_env_type,
            amp_attack_ms: d.amp_attack_ms,
            amp_decay_ms: d.amp_decay_ms,
            amp_sustain: d.amp_sustain,
            amp_release_ms: d.amp_release_ms,

            mono_mode: d.mono_mode,
            legato: d.legato,
            glide_ms: d.glide_ms,
            global_pitch_st: d.global_pitch_st,
            velocity_sens: d.velocity_sens,

            modulators_open: true,
            lfo_enabled: d.lfo_enabled,
            lfo_wave: d.lfo_wave,
            lfo_rate_hz: d.lfo_rate_hz,
            lfo_selected_target: OpenPolyLfoTarget::Cutoff,
            lfo_amount_cutoff: d.lfo_amount_cutoff,
            lfo_amount_index: d.lfo_amount_index,
            lfo_amount_pitch: d.lfo_amount_pitch,
            lfo_amount_amp: d.lfo_amount_amp,

            pan: d.pan,
            output_gain: d.output_gain,
            stereo_width: d.stereo_width,

            render_mode_3d: true,
            preset_name: "Standard Four".to_string(),

            wavetable_rev: 1,
            params_rev: 1,
        }
    }
}

impl HikaruOpenPoly {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mark_params_dirty(&mut self) {
        self.params_rev = self.params_rev.wrapping_add(1);
    }

    pub fn load_wavetable(&mut self, table: Wavetable) {
        self.wt_data = Arc::new(OpenPolyWavetableData::new(
            table.samples.clone(),
            table.frames,
            FRAME_SAMPLES,
        ));
        self.table = table;
        self.osc_type = OpenPolyOscType::Wavetable;
        self.wavetable_rev = self.wavetable_rev.wrapping_add(1);
        self.mark_params_dirty();
    }

    pub fn lfo_target_amount(&self, target: OpenPolyLfoTarget) -> f32 {
        match target {
            OpenPolyLfoTarget::Cutoff => self.lfo_amount_cutoff,
            OpenPolyLfoTarget::Index => self.lfo_amount_index,
            OpenPolyLfoTarget::Pitch => self.lfo_amount_pitch,
            OpenPolyLfoTarget::Amp => self.lfo_amount_amp,
        }
    }

    pub fn lfo_target_amount_mut(&mut self, target: OpenPolyLfoTarget) -> &mut f32 {
        match target {
            OpenPolyLfoTarget::Cutoff => &mut self.lfo_amount_cutoff,
            OpenPolyLfoTarget::Index => &mut self.lfo_amount_index,
            OpenPolyLfoTarget::Pitch => &mut self.lfo_amount_pitch,
            OpenPolyLfoTarget::Amp => &mut self.lfo_amount_amp,
        }
    }

    pub fn apply_factory_preset(&mut self, name: &str) {
        let mut p = OpenPolyParams::default();
        p.table_name = self.table.name.clone();
        match name {
            "Analog Brass" => {
                p.osc_type = "Sawtooth".into();
                p.osc_index = 0.25;
                p.unison_voices = 3;
                p.unison_detune = 0.24;
                p.sub_level = 0.2;
                p.sub_wave = "Saw".into();
                p.filter_type = "Low-pass MG".into();
                p.filter_cutoff_hz = 420.0;
                p.filter_resonance = 0.28;
                p.filter_drive = 0.25;
                p.filter_env_amount = 0.62;
                p.feg_attack_ms = 55.0;
                p.feg_decay_ms = 420.0;
                p.feg_sustain = 0.38;
                p.amp_attack_ms = 38.0;
                p.amp_sustain = 0.85;
                p.amp_release_ms = 260.0;
            }
            "Deep Sub Bass" => {
                p.osc_type = "Sine".into();
                p.osc_index = 0.14;
                p.osc_semitones = -12.0;
                p.sub_level = 0.65;
                p.sub_wave = "Sine".into();
                p.sub_octave = -1;
                p.filter_type = "Low-pass LD".into();
                p.filter_cutoff_hz = 260.0;
                p.filter_resonance = 0.12;
                p.filter_drive = 0.35;
                p.filter_env_amount = 0.22;
                p.mono_mode = true;
                p.legato = true;
                p.glide_ms = 48.0;
                p.amp_attack_ms = 3.0;
                p.amp_sustain = 0.92;
                p.amp_release_ms = 90.0;
            }
            "Pluck Lead" => {
                p.osc_type = "Phase-1".into();
                p.osc_index = 0.68;
                p.sub_level = 0.15;
                p.noise_level = 0.08;
                p.filter_type = "SVF".into();
                p.filter_cutoff_hz = 380.0;
                p.filter_resonance = 0.32;
                p.filter_env_amount = 0.78;
                p.feg_attack_ms = 1.5;
                p.feg_decay_ms = 165.0;
                p.feg_sustain = 0.05;
                p.amp_env_type = "AD".into();
                p.amp_attack_ms = 1.5;
                p.amp_decay_ms = 340.0;
                p.amp_sustain = 0.0;
                p.amp_release_ms = 180.0;
            }
            "Swarm Pad" => {
                p.osc_type = "Swarm".into();
                p.osc_index = 0.58;
                p.unison_spread = 0.85;
                p.sub_level = 0.30;
                p.sub_wave = "Triangle".into();
                p.filter_type = "Low-pass MG".into();
                p.filter_cutoff_hz = 920.0;
                p.filter_resonance = 0.18;
                p.filter_env_amount = 0.35;
                p.feg_attack_ms = 650.0;
                p.feg_decay_ms = 1200.0;
                p.feg_sustain = 0.55;
                p.amp_attack_ms = 520.0;
                p.amp_decay_ms = 800.0;
                p.amp_sustain = 0.82;
                p.amp_release_ms = 1100.0;
                p.lfo_rate_hz = 0.45;
                p.lfo_amount_cutoff = 0.22;
                p.stereo_width = 0.9;
            }
            "Acid Sallen-Key" => {
                p.osc_type = "Pulse".into();
                p.osc_index = 0.32;
                p.sub_level = 0.20;
                p.sub_wave = "Square".into();
                p.filter_type = "Sallen-Key".into();
                p.filter_cutoff_hz = 290.0;
                p.filter_resonance = 0.72;
                p.filter_drive = 0.45;
                p.filter_env_amount = 0.72;
                p.feg_attack_ms = 2.0;
                p.feg_decay_ms = 190.0;
                p.feg_sustain = 0.12;
                p.mono_mode = true;
                p.legato = true;
                p.glide_ms = 65.0;
            }
            _ => {
                // Standard Four (default)
            }
        }
        self.apply_project_params(&p);
        self.preset_name = name.to_string();
    }

    pub fn to_dsp_params(&self) -> OpenPolyDspParams {
        OpenPolyDspParams {
            osc_type: self.osc_type,
            osc_index: self.osc_index.clamp(0.0, 1.0),
            osc_fold: self.osc_fold.clamp(0.0, 1.0),
            osc_ratio_num: self.osc_ratio_num.clamp(1, 8),
            osc_ratio_den: self.osc_ratio_den.clamp(1, 8),
            osc_semitones: self.osc_semitones.clamp(-36.0, 36.0),
            osc_detune_hz: self.osc_detune_hz.clamp(-50.0, 50.0),
            osc_sync: self.osc_sync,
            unison_voices: self.unison_voices.clamp(1, 7),
            unison_detune: self.unison_detune.clamp(0.0, 1.0),
            unison_spread: self.unison_spread.clamp(0.0, 1.0),

            sub_level: self.sub_level.clamp(0.0, 1.0),
            sub_wave: self.sub_wave,
            sub_octave: self.sub_octave.clamp(-2, 0),
            noise_level: self.noise_level.clamp(0.0, 1.0),

            filter_enabled: self.filter_enabled,
            filter_type: self.filter_type,
            filter_cutoff_hz: self.filter_cutoff_hz.clamp(20.0, 20000.0),
            filter_resonance: self.filter_resonance.clamp(0.0, 0.98),
            filter_drive: self.filter_drive.clamp(0.0, 1.0),
            filter_keytrack: self.filter_keytrack.clamp(0.0, 1.0),
            filter_env_amount: self.filter_env_amount.clamp(-1.0, 1.0),
            filter_vel_amount: self.filter_vel_amount.clamp(0.0, 1.0),

            feg_enabled: self.feg_enabled,
            feg_attack_ms: self.feg_attack_ms.clamp(0.5, 5000.0),
            feg_decay_ms: self.feg_decay_ms.clamp(1.0, 8000.0),
            feg_sustain: self.feg_sustain.clamp(0.0, 1.0),
            feg_release_ms: self.feg_release_ms.clamp(1.0, 8000.0),

            amp_env_type: self.amp_env_type,
            amp_attack_ms: self.amp_attack_ms.clamp(0.5, 5000.0),
            amp_decay_ms: self.amp_decay_ms.clamp(1.0, 8000.0),
            amp_sustain: self.amp_sustain.clamp(0.0, 1.0),
            amp_release_ms: self.amp_release_ms.clamp(1.0, 8000.0),

            mono_mode: self.mono_mode,
            legato: self.legato,
            glide_ms: self.glide_ms.clamp(0.0, 2000.0),
            global_pitch_st: self.global_pitch_st.clamp(-24.0, 24.0),
            velocity_sens: self.velocity_sens.clamp(0.0, 1.0),

            lfo_enabled: self.lfo_enabled,
            lfo_wave: self.lfo_wave,
            lfo_rate_hz: self.lfo_rate_hz.clamp(0.05, 25.0),
            lfo_amount_cutoff: self.lfo_amount_cutoff.clamp(-1.0, 1.0),
            lfo_amount_index: self.lfo_amount_index.clamp(-1.0, 1.0),
            lfo_amount_pitch: self.lfo_amount_pitch.clamp(-1.0, 1.0),
            lfo_amount_amp: self.lfo_amount_amp.clamp(-1.0, 1.0),

            pan: self.pan.clamp(-1.0, 1.0),
            output_gain: self.output_gain.clamp(0.0, 2.0),
            stereo_width: self.stereo_width.clamp(0.0, 1.0),
        }
    }

    pub fn to_project_params(&self) -> OpenPolyParams {
        OpenPolyParams {
            table_name: self.table.name.clone(),
            osc_type: self.osc_type.label().to_string(),
            osc_index: self.osc_index,
            osc_fold: self.osc_fold,
            osc_ratio_num: self.osc_ratio_num,
            osc_ratio_den: self.osc_ratio_den,
            osc_semitones: self.osc_semitones,
            osc_detune_hz: self.osc_detune_hz,
            osc_sync: self.osc_sync,
            unison_voices: self.unison_voices,
            unison_detune: self.unison_detune,
            unison_spread: self.unison_spread,

            sub_level: self.sub_level,
            sub_wave: self.sub_wave.label().to_string(),
            sub_octave: self.sub_octave,
            noise_level: self.noise_level,

            filter_enabled: self.filter_enabled,
            filter_type: self.filter_type.label().to_string(),
            filter_cutoff_hz: self.filter_cutoff_hz,
            filter_resonance: self.filter_resonance,
            filter_drive: self.filter_drive,
            filter_keytrack: self.filter_keytrack,
            filter_env_amount: self.filter_env_amount,
            filter_vel_amount: self.filter_vel_amount,

            feg_enabled: self.feg_enabled,
            feg_attack_ms: self.feg_attack_ms,
            feg_decay_ms: self.feg_decay_ms,
            feg_sustain: self.feg_sustain,
            feg_release_ms: self.feg_release_ms,

            amp_env_type: self.amp_env_type.label().to_string(),
            amp_attack_ms: self.amp_attack_ms,
            amp_decay_ms: self.amp_decay_ms,
            amp_sustain: self.amp_sustain,
            amp_release_ms: self.amp_release_ms,

            mono_mode: self.mono_mode,
            legato: self.legato,
            glide_ms: self.glide_ms,
            global_pitch_st: self.global_pitch_st,
            velocity_sens: self.velocity_sens,

            lfo_enabled: self.lfo_enabled,
            lfo_wave: self.lfo_wave.label().to_string(),
            lfo_rate_hz: self.lfo_rate_hz,
            lfo_amount_cutoff: self.lfo_amount_cutoff,
            lfo_amount_index: self.lfo_amount_index,
            lfo_amount_pitch: self.lfo_amount_pitch,
            lfo_amount_amp: self.lfo_amount_amp,

            pan: self.pan,
            output_gain: self.output_gain,
            stereo_width: self.stereo_width,
            modulators_open: self.modulators_open,
            render_mode_3d: self.render_mode_3d,
        }
    }

    pub fn apply_project_params(&mut self, p: &OpenPolyParams) {
        if !p.table_name.is_empty() {
            self.table.name = p.table_name.clone();
        }
        self.osc_type = OpenPolyOscType::from_name(&p.osc_type);
        self.osc_index = p.osc_index.clamp(0.0, 1.0);
        self.base_index = Some(self.osc_index);
        self.osc_fold = p.osc_fold.clamp(0.0, 1.0);
        self.osc_ratio_num = p.osc_ratio_num.clamp(1, 8);
        self.osc_ratio_den = p.osc_ratio_den.clamp(1, 8);
        self.osc_semitones = p.osc_semitones.clamp(-36.0, 36.0);
        self.osc_detune_hz = p.osc_detune_hz.clamp(-50.0, 50.0);
        self.osc_sync = p.osc_sync;
        self.unison_voices = p.unison_voices.clamp(1, 7);
        self.unison_detune = p.unison_detune.clamp(0.0, 1.0);
        self.unison_spread = p.unison_spread.clamp(0.0, 1.0);

        self.sub_level = p.sub_level.clamp(0.0, 1.0);
        self.sub_wave = OpenPolySubWave::from_name(&p.sub_wave);
        self.sub_octave = p.sub_octave.clamp(-2, 0);
        self.noise_level = p.noise_level.clamp(0.0, 1.0);

        self.filter_enabled = p.filter_enabled;
        self.filter_type = OpenPolyFilterType::from_name(&p.filter_type);
        self.filter_cutoff_hz = p.filter_cutoff_hz.clamp(20.0, 20000.0);
        self.filter_resonance = p.filter_resonance.clamp(0.0, 0.98);
        self.filter_drive = p.filter_drive.clamp(0.0, 1.0);
        self.filter_keytrack = p.filter_keytrack.clamp(0.0, 1.0);
        self.filter_env_amount = p.filter_env_amount.clamp(-1.0, 1.0);
        self.filter_vel_amount = p.filter_vel_amount.clamp(0.0, 1.0);

        self.feg_enabled = p.feg_enabled;
        self.feg_attack_ms = p.feg_attack_ms.clamp(0.5, 5000.0);
        self.feg_decay_ms = p.feg_decay_ms.clamp(1.0, 8000.0);
        self.feg_sustain = p.feg_sustain.clamp(0.0, 1.0);
        self.feg_release_ms = p.feg_release_ms.clamp(1.0, 8000.0);

        self.amp_env_type = OpenPolyEnvType::from_name(&p.amp_env_type);
        self.amp_attack_ms = p.amp_attack_ms.clamp(0.5, 5000.0);
        self.amp_decay_ms = p.amp_decay_ms.clamp(1.0, 8000.0);
        self.amp_sustain = p.amp_sustain.clamp(0.0, 1.0);
        self.amp_release_ms = p.amp_release_ms.clamp(1.0, 8000.0);

        self.mono_mode = p.mono_mode;
        self.legato = p.legato;
        self.glide_ms = p.glide_ms.clamp(0.0, 2000.0);
        self.global_pitch_st = p.global_pitch_st.clamp(-24.0, 24.0);
        self.velocity_sens = p.velocity_sens.clamp(0.0, 1.0);

        self.lfo_enabled = p.lfo_enabled;
        self.lfo_wave = OpenPolyLfoWave::from_name(&p.lfo_wave);
        self.lfo_rate_hz = p.lfo_rate_hz.clamp(0.05, 25.0);
        self.lfo_amount_cutoff = p.lfo_amount_cutoff.clamp(-1.0, 1.0);
        self.lfo_amount_index = p.lfo_amount_index.clamp(-1.0, 1.0);
        self.lfo_amount_pitch = p.lfo_amount_pitch.clamp(-1.0, 1.0);
        self.lfo_amount_amp = p.lfo_amount_amp.clamp(-1.0, 1.0);

        self.pan = p.pan.clamp(-1.0, 1.0);
        self.output_gain = p.output_gain.clamp(0.0, 2.0);
        self.stereo_width = p.stereo_width.clamp(0.0, 1.0);
        self.modulators_open = p.modulators_open;
        self.render_mode_3d = p.render_mode_3d;
        self.mark_params_dirty();
    }

    /// Migrates a legacy `OpenWavetableParams` preset/project seamlessly into `HikaruOpenPoly`.
    pub fn apply_legacy_wavetable_params(&mut self, p: &OpenWavetableParams) {
        if !p.table_name.is_empty() {
            self.table.name = p.table_name.clone();
        }
        self.osc_type = OpenPolyOscType::Wavetable;
        self.osc_index = p.position.clamp(0.0, 1.0);
        self.base_index = Some(self.osc_index);
        self.unison_voices = p.unison.clamp(1, 7);
        self.unison_detune = p.detune.clamp(0.0, 1.0);
        self.osc_semitones = (p.octave as f32 * 12.0 + p.pitch as f32).clamp(-36.0, 36.0);
        self.output_gain = p.volume.clamp(0.0, 2.0);
        self.mono_mode = matches!(p.voices.as_str(), "Mono" | "Legato");
        self.legato = p.voices == "Legato";
        self.render_mode_3d = p.render_mode_3d;
        self.mark_params_dirty();
    }
}

pub fn load_wavetable_from_path(poly: &mut HikaruOpenPoly, path: &Path) -> bool {
    match wavetable_io::load_wavetable(path, MAX_FRAMES) {
        Ok(table) => {
            poly.load_wavetable(table);
            true
        }
        Err(_) => false,
    }
}

pub fn save_openpoly_preset(poly: &HikaruOpenPoly) {
    let default_name = format!(
        "{}.hdp",
        poly.preset_name
            .to_lowercase()
            .replace(' ', "_")
            .replace(|c: char| !c.is_alphanumeric() && c != '_', "")
    );
    if let Some(path) = rfd::FileDialog::new()
        .set_file_name(&default_name)
        .add_filter(
            hikaru_project_file::HDP_FILE_FILTER,
            &[hikaru_project_file::HDP_EXTENSION],
        )
        .save_file()
    {
        let slot = hikaru_project_file::DspSlot {
            id: 0,
            name: "OpenPoly".to_string(),
            active: true,
            wavetable_path: poly
                .table
                .path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
            plugin_path: None,
            plugin_format: None,
            open_wavetable: None,
            open_modulation: None,
            sampler: None,
            open_poly: Some(poly.to_project_params()),
        };
        let preset = hikaru_project_file::HikaruDevicePreset {
            format_version: hikaru_project_file::FORMAT_VERSION,
            created_by: format!("Hikaru OpenLive {}", crate::version::VERSION),
            preset_type: hikaru_project_file::PresetType::SingleDevice,
            name: poly.preset_name.clone(),
            author: None,
            category: Some("Polysynth".to_string()),
            description: Some("Hikaru OpenPoly preset (AGPL-v3)".to_string()),
            slot: Some(slot),
            chain: None,
        };
        let _ = hikaru_project_file::save_hdp(&path, &preset);
    }
}

pub fn load_openpoly_preset(poly: &mut HikaruOpenPoly) {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter(
            hikaru_project_file::HDP_FILE_FILTER,
            &[hikaru_project_file::HDP_EXTENSION],
        )
        .pick_file()
    {
        if let Ok(preset) = hikaru_project_file::parse_hdp(&path) {
            if let Some(slot) = preset.slot {
                if let Some(ref wt_path) = slot.wavetable_path {
                    if let Some(resolved) = crate::project::common::resolve_wavetable_path(wt_path)
                    {
                        let _ = load_wavetable_from_path(poly, &resolved);
                    }
                }
                if let Some(ref p) = slot.open_poly {
                    poly.apply_project_params(p);
                } else if let Some(ref legacy) = slot.open_wavetable {
                    poly.apply_legacy_wavetable_params(legacy);
                }
                if !preset.name.is_empty() {
                    poly.preset_name = preset.name;
                }
            }
        }
    }
}
