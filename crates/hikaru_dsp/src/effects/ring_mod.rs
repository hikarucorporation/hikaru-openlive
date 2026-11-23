// crates/hikaru_dsp/src/effects/ring_mod.rs

//! **Ring-Mod**: carrier ring modulator (Sine / Triangle / Saw / Square) with dry/wet mix.

use hikaru_core::AudioBuffer;
use hikaru_plugin_host::{PluginInstance, PluginParamInfo};
use std::f32::consts::TAU;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingModWave {
    Sine = 0,
    Triangle = 1,
    Saw = 2,
    Square = 3,
}

#[derive(Debug, Clone, Copy)]
pub struct RingModParams {
    /// Carrier frequency in Hz (20 .. 5000).
    pub freq_hz: f32,
    pub wave: RingModWave,
    /// Dry/wet mix (0.0 .. 1.0).
    pub mix: f32,
}

impl Default for RingModParams {
    fn default() -> Self {
        Self {
            freq_hz: 440.0,
            wave: RingModWave::Sine,
            mix: 0.5,
        }
    }
}

pub struct RingModEffect {
    params: RingModParams,
    sample_rate: f32,
    phase: f32,
}

impl RingModEffect {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            params: RingModParams::default(),
            sample_rate: sample_rate.max(8_000.0),
            phase: 0.0,
        }
    }

    pub fn set_params(&mut self, params: RingModParams) {
        self.params = RingModParams {
            freq_hz: params.freq_hz.clamp(20.0, 5_000.0),
            wave: params.wave,
            mix: params.mix.clamp(0.0, 1.0),
        };
    }

    #[inline]
    fn carrier(&mut self) -> f32 {
        let p = self.phase;
        self.phase = (self.phase + self.params.freq_hz / self.sample_rate).fract();
        match self.params.wave {
            RingModWave::Sine => (p * TAU).sin(),
            RingModWave::Triangle => 1.0 - 4.0 * (p - 0.5).abs(),
            RingModWave::Saw => 2.0 * p - 1.0,
            RingModWave::Square => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
        }
    }

    #[inline]
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        let c = self.carrier();
        let m = self.params.mix;
        (
            left * (1.0 - m) + (left * c) * m,
            right * (1.0 - m) + (right * c) * m,
        )
    }
}

fn freq_to_norm(hz: f32) -> f32 {
    ((hz.clamp(20.0, 5_000.0) / 20.0).ln() / (250.0_f32).ln()).clamp(0.0, 1.0)
}

fn norm_to_freq(norm: f32) -> f32 {
    20.0 * (250.0_f32).powf(norm.clamp(0.0, 1.0))
}

impl PluginInstance for RingModEffect {
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
        "Ring-Mod"
    }

    fn param_count(&mut self) -> usize {
        3
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
            0 => Some(mk(0, "Frequency", "Hz", freq_to_norm(440.0), 20.0, 5_000.0, 0)),
            1 => Some(mk(1, "Wave", "", 0.0, 0.0, 3.0, 3)),
            2 => Some(mk(2, "Mix", "%", 0.5, 0.0, 100.0, 0)),
            _ => None,
        }
    }

    fn get_param_normalized(&mut self, id: u32) -> Option<f32> {
        match id {
            0 => Some(freq_to_norm(self.params.freq_hz)),
            1 => Some((self.params.wave as u32) as f32 / 3.0),
            2 => Some(self.params.mix),
            _ => None,
        }
    }

    fn set_param_normalized(&mut self, id: u32, value: f32) -> bool {
        let v = value.clamp(0.0, 1.0);
        let mut p = self.params;
        match id {
            0 => p.freq_hz = norm_to_freq(v),
            1 => {
                p.wave = match (v * 3.0).round() as u32 {
                    1 => RingModWave::Triangle,
                    2 => RingModWave::Saw,
                    3 => RingModWave::Square,
                    _ => RingModWave::Sine,
                };
            }
            2 => p.mix = v,
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
    fn ring_mod_multiplies_carrier() {
        let mut fx = RingModEffect::new(48_000.0);
        fx.set_params(RingModParams {
            freq_hz: 1000.0,
            wave: RingModWave::Sine,
            mix: 1.0,
        });
        let mut diff = 0.0_f32;
        for _ in 0..128 {
            let (l, _) = fx.process_stereo(0.5, 0.5);
            diff += (l - 0.5).abs();
        }
        assert!(diff > 10.0);
    }
}
