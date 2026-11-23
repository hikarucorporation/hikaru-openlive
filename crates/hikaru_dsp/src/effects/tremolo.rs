// crates/hikaru_dsp/src/effects/tremolo.rs

//! **Tremolo**: stereo LFO amplitude modulation with stereo phase offset.

use hikaru_core::AudioBuffer;
use hikaru_plugin_host::{PluginInstance, PluginParamInfo};
use std::f32::consts::TAU;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TremoloShape {
    Sine = 0,
    Triangle = 1,
    Square = 2,
}

#[derive(Debug, Clone, Copy)]
pub struct TremoloParams {
    /// LFO rate in Hz (0.1 .. 20.0).
    pub rate_hz: f32,
    /// Modulation depth (0.0 .. 1.0).
    pub depth: f32,
    /// Stereo phase offset in degrees (0.0 .. 180.0).
    pub stereo_phase_deg: f32,
    pub shape: TremoloShape,
}

impl Default for TremoloParams {
    fn default() -> Self {
        Self {
            rate_hz: 4.0,
            depth: 0.5,
            stereo_phase_deg: 0.0,
            shape: TremoloShape::Sine,
        }
    }
}

pub struct TremoloEffect {
    params: TremoloParams,
    sample_rate: f32,
    phase: f32,
}

impl TremoloEffect {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            params: TremoloParams::default(),
            sample_rate: sample_rate.max(8_000.0),
            phase: 0.0,
        }
    }

    pub fn set_params(&mut self, params: TremoloParams) {
        self.params = TremoloParams {
            rate_hz: params.rate_hz.clamp(0.1, 20.0),
            depth: params.depth.clamp(0.0, 1.0),
            stereo_phase_deg: params.stereo_phase_deg.clamp(0.0, 180.0),
            shape: params.shape,
        };
    }

    #[inline]
    fn eval_unipolar(&self, p: f32) -> f32 {
        let p = p.rem_euclid(1.0);
        match self.params.shape {
            TremoloShape::Sine => 0.5 + 0.5 * (p * TAU).sin(),
            TremoloShape::Triangle => 1.0 - (2.0 * p - 1.0).abs(),
            TremoloShape::Square => {
                if p < 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }

    #[inline]
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        let lfo_l = self.eval_unipolar(self.phase);
        let lfo_r = self.eval_unipolar(self.phase + self.params.stereo_phase_deg / 360.0);
        self.phase = (self.phase + self.params.rate_hz / self.sample_rate).fract();

        let d = self.params.depth;
        let gain_l = 1.0 - d * (1.0 - lfo_l);
        let gain_r = 1.0 - d * (1.0 - lfo_r);
        (left * gain_l, right * gain_r)
    }
}

impl PluginInstance for TremoloEffect {
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
        "Tremolo"
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
            0 => Some(mk(0, "Rate", "Hz", (4.0 - 0.1) / 19.9, 0.1, 20.0, 0)),
            1 => Some(mk(1, "Depth", "%", 0.5, 0.0, 100.0, 0)),
            2 => Some(mk(2, "Stereo", "deg", 0.0, 0.0, 180.0, 0)),
            3 => Some(mk(3, "Shape", "", 0.0, 0.0, 2.0, 2)),
            _ => None,
        }
    }

    fn get_param_normalized(&mut self, id: u32) -> Option<f32> {
        match id {
            0 => Some(((self.params.rate_hz - 0.1) / 19.9).clamp(0.0, 1.0)),
            1 => Some(self.params.depth),
            2 => Some((self.params.stereo_phase_deg / 180.0).clamp(0.0, 1.0)),
            3 => Some((self.params.shape as u32) as f32 / 2.0),
            _ => None,
        }
    }

    fn set_param_normalized(&mut self, id: u32, value: f32) -> bool {
        let v = value.clamp(0.0, 1.0);
        let mut p = self.params;
        match id {
            0 => p.rate_hz = 0.1 + v * 19.9,
            1 => p.depth = v,
            2 => p.stereo_phase_deg = v * 180.0,
            3 => {
                p.shape = match (v * 2.0).round() as u32 {
                    1 => TremoloShape::Triangle,
                    2 => TremoloShape::Square,
                    _ => TremoloShape::Sine,
                };
            }
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
    fn tremolo_modulates_amplitude() {
        let mut fx = TremoloEffect::new(48_000.0);
        fx.set_params(TremoloParams {
            rate_hz: 10.0,
            depth: 1.0,
            stereo_phase_deg: 0.0,
            shape: TremoloShape::Sine,
        });
        let mut min_v = 1.0_f32;
        let mut max_v = 0.0_f32;
        for _ in 0..4800 {
            let (l, _) = fx.process_stereo(1.0, 1.0);
            min_v = min_v.min(l);
            max_v = max_v.max(l);
        }
        assert!(min_v < 0.05 && max_v > 0.95);
    }
}
