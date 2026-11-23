// crates/hikaru_dsp/src/effects/test_tone.rs

//! **Test Tone**: diagnostic sine / white noise / pink noise generator.

use hikaru_core::AudioBuffer;
use hikaru_plugin_host::{PluginInstance, PluginParamInfo};
use std::f32::consts::TAU;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestToneWave {
    Sine = 0,
    WhiteNoise = 1,
    PinkNoise = 2,
}

#[derive(Debug, Clone, Copy)]
pub struct TestToneParams {
    pub wave: TestToneWave,
    /// Frequency in Hz (20 .. 20000).
    pub freq_hz: f32,
    /// Output level in dB (-60 .. 0).
    pub gain_db: f32,
    /// Dry/wet mix (0 .. 1).
    pub mix: f32,
}

impl Default for TestToneParams {
    fn default() -> Self {
        Self {
            wave: TestToneWave::Sine,
            freq_hz: 440.0,
            gain_db: -12.0,
            mix: 1.0,
        }
    }
}

pub struct TestToneEffect {
    params: TestToneParams,
    sample_rate: f32,
    phase: f32,
    gain_lin: f32,
    rng: u32,
    pink_b: [f32; 7],
}

impl TestToneEffect {
    pub fn new(sample_rate: f32) -> Self {
        let params = TestToneParams::default();
        Self {
            gain_lin: 10.0_f32.powf(params.gain_db / 20.0),
            params,
            sample_rate: sample_rate.max(8_000.0),
            phase: 0.0,
            rng: 0x1234_5678,
            pink_b: [0.0; 7],
        }
    }

    pub fn set_params(&mut self, params: TestToneParams) {
        self.params = TestToneParams {
            wave: params.wave,
            freq_hz: params.freq_hz.clamp(20.0, 20_000.0),
            gain_db: params.gain_db.clamp(-60.0, 0.0),
            mix: params.mix.clamp(0.0, 1.0),
        };
        self.gain_lin = 10.0_f32.powf(self.params.gain_db / 20.0);
    }

    #[inline]
    fn next_white(&mut self) -> f32 {
        // xorshift32
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    #[inline]
    fn next_sample(&mut self) -> f32 {
        let raw = match self.params.wave {
            TestToneWave::Sine => {
                let s = (self.phase * TAU).sin();
                self.phase = (self.phase + self.params.freq_hz / self.sample_rate).fract();
                s
            }
            TestToneWave::WhiteNoise => self.next_white(),
            TestToneWave::PinkNoise => {
                let w = self.next_white();
                let b = &mut self.pink_b;
                b[0] = 0.99886 * b[0] + w * 0.0555179;
                b[1] = 0.99332 * b[1] + w * 0.0750759;
                b[2] = 0.96900 * b[2] + w * 0.1538520;
                b[3] = 0.86650 * b[3] + w * 0.3104856;
                b[4] = 0.55000 * b[4] + w * 0.5329522;
                b[5] = -0.7616 * b[5] - w * 0.0168980;
                let pink = b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + w * 0.5362;
                b[6] = w * 0.115926;
                (pink * 0.11).clamp(-1.0, 1.0)
            }
        };
        raw * self.gain_lin
    }

    #[inline]
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        let tone = self.next_sample();
        let m = self.params.mix;
        (left * (1.0 - m) + tone * m, right * (1.0 - m) + tone * m)
    }
}

fn freq_to_norm(hz: f32) -> f32 {
    ((hz.clamp(20.0, 20_000.0) / 20.0).ln() / (1000.0_f32).ln()).clamp(0.0, 1.0)
}

fn norm_to_freq(norm: f32) -> f32 {
    20.0 * (1000.0_f32).powf(norm.clamp(0.0, 1.0))
}

impl PluginInstance for TestToneEffect {
    fn process(&mut self, buffer: &mut AudioBuffer) {
        for frame in buffer.samples.chunks_exact_mut(2) {
            let (l, r) = self.process_stereo(frame[0], frame[1]);
            frame[0] = l;
            frame[1] = r;
        }
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(8_000.0);
    }

    fn show_gui_embedded(&mut self, _: hikaru_plugin_host::RawWindowHandle) -> Result<(), String> {
        Err("Native plugin".to_string())
    }
    fn show_gui_floating(&mut self) -> Result<(), String> {
        Err("Native plugin".to_string())
    }
    fn hide_gui(&mut self) {}

    fn get_name(&self) -> &str {
        "Test Tone"
    }

    fn param_count(&mut self) -> usize {
        4
    }

    fn param_info(&mut self, index: usize) -> Option<PluginParamInfo> {
        let mk = |id, name: &str, unit: &str, default_value, min, max, step_count| PluginParamInfo {
            id,
            name: name.to_string(),
            unit: unit.to_string(),
            default_value,
            min,
            max,
            step_count,
            can_automate: true,
            is_modulatable: true,
        };
        match index {
            0 => Some(mk(0, "Frequency", "Hz", freq_to_norm(440.0), 20.0, 20_000.0, 0)),
            1 => Some(mk(1, "Gain", "dB", 0.8, -60.0, 0.0, 0)),
            2 => Some(mk(2, "Wave", "", 0.0, 0.0, 2.0, 2)),
            3 => Some(mk(3, "Mix", "%", 1.0, 0.0, 100.0, 0)),
            _ => None,
        }
    }

    fn get_param_normalized(&mut self, id: u32) -> Option<f32> {
        match id {
            0 => Some(freq_to_norm(self.params.freq_hz)),
            1 => Some(((self.params.gain_db + 60.0) / 60.0).clamp(0.0, 1.0)),
            2 => Some((self.params.wave as u32) as f32 / 2.0),
            3 => Some(self.params.mix),
            _ => None,
        }
    }

    fn set_param_normalized(&mut self, id: u32, value: f32) -> bool {
        let v = value.clamp(0.0, 1.0);
        let mut p = self.params;
        match id {
            0 => p.freq_hz = norm_to_freq(v),
            1 => p.gain_db = -60.0 + v * 60.0,
            2 => {
                p.wave = match (v * 2.0).round() as u32 {
                    1 => TestToneWave::WhiteNoise,
                    2 => TestToneWave::PinkNoise,
                    _ => TestToneWave::Sine,
                };
            }
            3 => p.mix = v,
            _ => return false,
        }
        self.set_params(p);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tone_generates_signal() {
        let mut fx = TestToneEffect::new(48_000.0);
        let mut energy = 0.0_f32;
        for _ in 0..256 {
            let (l, _) = fx.process_stereo(0.0, 0.0);
            energy += l * l;
        }
        assert!(energy > 1.0, "Sine test tone should produce energy");
    }
}
