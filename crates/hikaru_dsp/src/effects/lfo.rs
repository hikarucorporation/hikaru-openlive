// crates/hikaru_dsp/src/effects/lfo.rs
// Hikaru OpenModulation - LFO MOD DSP Oscillator

use std::f32::consts::TAU;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LfoWaveform {
    #[default]
    Sine,
    Triangle,
    SawUp,
    SawDown,
    Square,
    SampleAndHold,
}

impl LfoWaveform {
    pub const ALL: [LfoWaveform; 6] = [
        LfoWaveform::Sine,
        LfoWaveform::Triangle,
        LfoWaveform::SawUp,
        LfoWaveform::SawDown,
        LfoWaveform::Square,
        LfoWaveform::SampleAndHold,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            LfoWaveform::Sine => "Sine",
            LfoWaveform::Triangle => "Tri",
            LfoWaveform::SawUp => "Saw Up",
            LfoWaveform::SawDown => "Saw Dn",
            LfoWaveform::Square => "Square",
            LfoWaveform::SampleAndHold => "S&H",
        }
    }

    pub fn short_symbol(&self) -> &'static str {
        match self {
            LfoWaveform::Sine => "~",
            LfoWaveform::Triangle => "/\\",
            LfoWaveform::SawUp => "/|",
            LfoWaveform::SawDown => "|\\",
            LfoWaveform::Square => "[]",
            LfoWaveform::SampleAndHold => "Rnd",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LfoSyncMode {
    #[default]
    FreeHz,
    TempoSync,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyncRate {
    Whole4,
    Whole2,
    Whole1,
    Half,
    #[default]
    Quarter,
    Eighth,
    Sixteenth,
    ThirtySecond,
    EighthTriplet,
    SixteenthTriplet,
}

impl SyncRate {
    pub const ALL: [SyncRate; 10] = [
        SyncRate::Whole4,
        SyncRate::Whole2,
        SyncRate::Whole1,
        SyncRate::Half,
        SyncRate::Quarter,
        SyncRate::Eighth,
        SyncRate::Sixteenth,
        SyncRate::ThirtySecond,
        SyncRate::EighthTriplet,
        SyncRate::SixteenthTriplet,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            SyncRate::Whole4 => "4/1",
            SyncRate::Whole2 => "2/1",
            SyncRate::Whole1 => "1/1",
            SyncRate::Half => "1/2",
            SyncRate::Quarter => "1/4",
            SyncRate::Eighth => "1/8",
            SyncRate::Sixteenth => "1/16",
            SyncRate::ThirtySecond => "1/32",
            SyncRate::EighthTriplet => "1/8T",
            SyncRate::SixteenthTriplet => "1/16T",
        }
    }

    pub fn beats_per_cycle(&self) -> f32 {
        match self {
            SyncRate::Whole4 => 16.0,
            SyncRate::Whole2 => 8.0,
            SyncRate::Whole1 => 4.0,
            SyncRate::Half => 2.0,
            SyncRate::Quarter => 1.0,
            SyncRate::Eighth => 0.5,
            SyncRate::Sixteenth => 0.25,
            SyncRate::ThirtySecond => 0.125,
            SyncRate::EighthTriplet => 1.0 / 3.0,
            SyncRate::SixteenthTriplet => 1.0 / 6.0,
        }
    }

    pub fn hz_at_bpm(&self, bpm: f32) -> f32 {
        let beats_per_sec = (bpm / 60.0).max(0.1);
        beats_per_sec / self.beats_per_cycle()
    }
}

/// Evaluates a waveform at phase in [0.0, 1.0), returning raw value in [-1.0, +1.0].
pub fn evaluate_waveform(waveform: LfoWaveform, phase: f32, sh_value: f32) -> f32 {
    let p = phase.rem_euclid(1.0);
    match waveform {
        LfoWaveform::Sine => (p * TAU).sin(),
        LfoWaveform::Triangle => {
            if p < 0.25 {
                p * 4.0
            } else if p < 0.75 {
                1.0 - (p - 0.25) * 4.0
            } else {
                -1.0 + (p - 0.75) * 4.0
            }
        }
        LfoWaveform::SawUp => -1.0 + 2.0 * p,
        LfoWaveform::SawDown => 1.0 - 2.0 * p,
        LfoWaveform::Square => {
            if p < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        LfoWaveform::SampleAndHold => sh_value.clamp(-1.0, 1.0),
    }
}

/// Simple deterministic pseudo-random generator for Sample & Hold LFO steps
pub fn pseudo_random_step(seed_index: u64) -> f32 {
    let mut x = seed_index.wrapping_mul(0xd1342543de82ef95).wrapping_add(1);
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51afd7ed558ccd);
    x ^= x >> 33;
    let norm = (x & 0x00ffffff) as f32 / 16777215.0; // 0.0 .. 1.0
    norm * 2.0 - 1.0 // -1.0 .. 1.0
}

#[derive(Debug, Clone)]
pub struct LfoOscillator {
    pub waveform: LfoWaveform,
    pub sync_mode: LfoSyncMode,
    pub rate_hz: f32,
    pub sync_rate: SyncRate,
    pub depth: f32,      // 0.0 .. 1.0
    pub phase_deg: f32,  // 0.0 .. 360.0
    pub bipolar: bool,   // true = -1..+1, false = 0..1
    pub amount: f32,     // -1.0 .. +1.0 target modulation amount
    last_cycle_index: u64,
    current_sh_val: f32,
}

impl Default for LfoOscillator {
    fn default() -> Self {
        Self {
            waveform: LfoWaveform::Sine,
            sync_mode: LfoSyncMode::FreeHz,
            rate_hz: 1.0,
            sync_rate: SyncRate::Quarter,
            depth: 1.0,
            phase_deg: 0.0,
            bipolar: true,
            amount: 0.5,
            last_cycle_index: 0,
            current_sh_val: 0.0,
        }
    }
}

impl LfoOscillator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn effective_frequency_hz(&self, bpm: f32) -> f32 {
        match self.sync_mode {
            LfoSyncMode::FreeHz => self.rate_hz.clamp(0.02, 40.0),
            LfoSyncMode::TempoSync => self.sync_rate.hz_at_bpm(bpm),
        }
    }

    /// Evaluates current normalized LFO output value at a given total elapsed time in seconds
    /// and current tempo (BPM).
    pub fn output_at_time(&mut self, time_seconds: f32, bpm: f32) -> f32 {
        let freq = self.effective_frequency_hz(bpm);
        let phase_offset = (self.phase_deg / 360.0).rem_euclid(1.0);
        let total_phase = time_seconds * freq + phase_offset;
        let cycle_idx = total_phase.floor() as u64;
        let frac_phase = total_phase.rem_euclid(1.0);

        if cycle_idx != self.last_cycle_index || self.current_sh_val == 0.0 {
            self.last_cycle_index = cycle_idx;
            self.current_sh_val = pseudo_random_step(cycle_idx);
        }

        let raw = evaluate_waveform(self.waveform, frac_phase, self.current_sh_val);
        if self.bipolar {
            raw * self.depth
        } else {
            ((raw + 1.0) * 0.5) * self.depth
        }
    }

    /// Computes target modulation delta to be added to a normalized target knob [0.0 .. 1.0].
    pub fn modulation_delta_at_time(&mut self, time_seconds: f32, bpm: f32) -> f32 {
        let lfo_val = self.output_at_time(time_seconds, bpm);
        lfo_val * self.amount
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_waveform_evaluations_bounded() {
        for &wf in &LfoWaveform::ALL {
            for step in 0..100 {
                let phase = step as f32 / 100.0;
                let val = evaluate_waveform(wf, phase, 0.42);
                assert!(
                    val >= -1.001 && val <= 1.001,
                    "Waveform {:?} at phase {} produced out of bounds {}",
                    wf,
                    phase,
                    val
                );
            }
        }
    }

    #[test]
    fn test_lfo_bipolar_vs_unipolar() {
        let mut lfo = LfoOscillator::default();
        lfo.rate_hz = 1.0;
        lfo.depth = 1.0;
        lfo.bipolar = true;

        // Sine at phase 0.25 is +1.0
        let val_peak = lfo.output_at_time(0.25, 120.0);
        assert!((val_peak - 1.0).abs() < 1e-4);

        // Sine at phase 0.75 is -1.0
        let val_trough = lfo.output_at_time(0.75, 120.0);
        assert!((val_trough - (-1.0)).abs() < 1e-4);

        // Unipolar
        lfo.bipolar = false;
        let uni_peak = lfo.output_at_time(0.25, 120.0);
        assert!((uni_peak - 1.0).abs() < 1e-4);
        let uni_trough = lfo.output_at_time(0.75, 120.0);
        assert!(uni_trough.abs() < 1e-4);
    }

    #[test]
    fn test_sync_rate_tempo_hz() {
        let rate = SyncRate::Quarter; // 1 beat = 0.5s at 120 BPM => 2.0 Hz
        let hz = rate.hz_at_bpm(120.0);
        assert!((hz - 2.0).abs() < 1e-4);

        let half = SyncRate::Half; // 2 beats => 1.0 Hz at 120 BPM
        assert!((half.hz_at_bpm(120.0) - 1.0).abs() < 1e-4);
    }
}
