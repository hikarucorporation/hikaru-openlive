// crates/hikaru_dsp/src/effects/dc_offset.rs

//! **DC Offset**: adds a constant DC offset (-1..+1) and/or removes DC via 1-pole HPF.

use hikaru_core::AudioBuffer;
use hikaru_plugin_host::{PluginInstance, PluginParamInfo};

#[derive(Debug, Clone, Copy)]
pub struct DcOffsetParams {
    /// Constant DC offset (-1.0 .. +1.0).
    pub offset: f32,
    /// When enabled, applies a ~5 Hz 1-pole high-pass filter before adding offset.
    pub remove_dc: bool,
}

impl Default for DcOffsetParams {
    fn default() -> Self {
        Self {
            offset: 0.0,
            remove_dc: false,
        }
    }
}

pub struct DcOffsetEffect {
    params: DcOffsetParams,
    sample_rate: f32,
    r_coeff: f32,
    prev_in_l: f32,
    prev_out_l: f32,
    prev_in_r: f32,
    prev_out_r: f32,
}

impl DcOffsetEffect {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        Self {
            params: DcOffsetParams::default(),
            sample_rate: sr,
            r_coeff: compute_r(sr),
            prev_in_l: 0.0,
            prev_out_l: 0.0,
            prev_in_r: 0.0,
            prev_out_r: 0.0,
        }
    }

    pub fn set_params(&mut self, params: DcOffsetParams) {
        self.params = DcOffsetParams {
            offset: params.offset.clamp(-1.0, 1.0),
            remove_dc: params.remove_dc,
        };
    }

    #[inline]
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        let (l, r) = if self.params.remove_dc {
            let yl = left - self.prev_in_l + self.r_coeff * self.prev_out_l;
            self.prev_in_l = left;
            self.prev_out_l = yl;
            let yr = right - self.prev_in_r + self.r_coeff * self.prev_out_r;
            self.prev_in_r = right;
            self.prev_out_r = yr;
            (yl, yr)
        } else {
            (left, right)
        };
        (l + self.params.offset, r + self.params.offset)
    }
}

fn compute_r(sr: f32) -> f32 {
    (1.0 - (2.0 * std::f32::consts::PI * 5.0 / sr)).clamp(0.9, 0.9999)
}

impl PluginInstance for DcOffsetEffect {
    fn process(&mut self, buffer: &mut AudioBuffer) {
        for frame in buffer.samples.chunks_exact_mut(2) {
            let (l, r) = self.process_stereo(frame[0], frame[1]);
            frame[0] = l;
            frame[1] = r;
        }
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(8_000.0);
        self.r_coeff = compute_r(self.sample_rate);
    }

    fn show_gui_embedded(&mut self, _: hikaru_plugin_host::RawWindowHandle) -> Result<(), String> {
        Err("Native plugin".to_string())
    }
    fn show_gui_floating(&mut self) -> Result<(), String> {
        Err("Native plugin".to_string())
    }
    fn hide_gui(&mut self) {}

    fn get_name(&self) -> &str {
        "DC Offset"
    }

    fn param_count(&mut self) -> usize {
        2
    }

    fn param_info(&mut self, index: usize) -> Option<PluginParamInfo> {
        match index {
            0 => Some(PluginParamInfo {
                id: 0,
                name: "Offset".to_string(),
                unit: "%".to_string(),
                default_value: 0.5,
                min: -100.0,
                max: 100.0,
                step_count: 0,
                can_automate: true,
                is_modulatable: true,
            }),
            1 => Some(PluginParamInfo {
                id: 1,
                name: "Remove DC".to_string(),
                unit: "".to_string(),
                default_value: 0.0,
                min: 0.0,
                max: 1.0,
                step_count: 1,
                can_automate: true,
                is_modulatable: false,
            }),
            _ => None,
        }
    }

    fn get_param_normalized(&mut self, id: u32) -> Option<f32> {
        match id {
            0 => Some(((self.params.offset + 1.0) * 0.5).clamp(0.0, 1.0)),
            1 => Some(if self.params.remove_dc { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    fn set_param_normalized(&mut self, id: u32, value: f32) -> bool {
        let v = value.clamp(0.0, 1.0);
        match id {
            0 => self.params.offset = -1.0 + v * 2.0,
            1 => self.params.remove_dc = v >= 0.5,
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dc_offset_adds_and_removes() {
        let mut fx = DcOffsetEffect::new(48_000.0);
        fx.set_params(DcOffsetParams {
            offset: 0.25,
            remove_dc: false,
        });
        let (l, r) = fx.process_stereo(0.1, -0.1);
        assert!((l - 0.35).abs() < 1e-5 && (r - 0.15).abs() < 1e-5);

        fx.set_params(DcOffsetParams {
            offset: 0.0,
            remove_dc: true,
        });
        let mut last = 1.0;
        for _ in 0..48_000 {
            let (out, _) = fx.process_stereo(0.5, 0.5);
            last = out;
        }
        assert!(last.abs() < 0.01, "DC should be removed, got {last}");
    }
}
