// crates/hikaru_dsp/src/effects/dual_pan.rs

//! **Dual Pan**: independent stereo panning for Left and Right input channels.

use hikaru_core::AudioBuffer;
use hikaru_plugin_host::{PluginInstance, PluginParamInfo};

#[derive(Debug, Clone, Copy)]
pub struct DualPanParams {
    /// Pan position of the Left input (-1.0 = L .. +1.0 = R).
    pub left_pan: f32,
    /// Pan position of the Right input (-1.0 = L .. +1.0 = R).
    pub right_pan: f32,
    /// Output gain in dB (-24.0 .. +12.0).
    pub gain_db: f32,
}

impl Default for DualPanParams {
    fn default() -> Self {
        Self {
            left_pan: -1.0,
            right_pan: 1.0,
            gain_db: 0.0,
        }
    }
}

pub struct DualPanEffect {
    params: DualPanParams,
    gain_lin: f32,
}

impl DualPanEffect {
    pub fn new(_sample_rate: f32) -> Self {
        Self {
            params: DualPanParams::default(),
            gain_lin: 1.0,
        }
    }

    pub fn set_params(&mut self, params: DualPanParams) {
        self.params = DualPanParams {
            left_pan: params.left_pan.clamp(-1.0, 1.0),
            right_pan: params.right_pan.clamp(-1.0, 1.0),
            gain_db: params.gain_db.clamp(-24.0, 12.0),
        };
        self.gain_lin = 10.0_f32.powf(self.params.gain_db / 20.0);
    }

    #[inline]
    pub fn process_stereo(&self, left: f32, right: f32) -> (f32, f32) {
        let l_to_l = 0.5 * (1.0 - self.params.left_pan);
        let l_to_r = 0.5 * (1.0 + self.params.left_pan);
        let r_to_l = 0.5 * (1.0 - self.params.right_pan);
        let r_to_r = 0.5 * (1.0 + self.params.right_pan);

        let out_l = (left * l_to_l + right * r_to_l) * self.gain_lin;
        let out_r = (left * l_to_r + right * r_to_r) * self.gain_lin;
        (out_l, out_r)
    }
}

impl PluginInstance for DualPanEffect {
    fn process(&mut self, buffer: &mut AudioBuffer) {
        for frame in buffer.samples.chunks_exact_mut(2) {
            let (l, r) = self.process_stereo(frame[0], frame[1]);
            frame[0] = l;
            frame[1] = r;
        }
    }

    fn set_sample_rate(&mut self, _sample_rate: f32) {}
    fn show_gui_embedded(&mut self, _: hikaru_plugin_host::RawWindowHandle) -> Result<(), String> {
        Err("Native plugin".to_string())
    }
    fn show_gui_floating(&mut self) -> Result<(), String> {
        Err("Native plugin".to_string())
    }
    fn hide_gui(&mut self) {}

    fn get_name(&self) -> &str {
        "Dual Pan"
    }

    fn param_count(&mut self) -> usize {
        3
    }

    fn param_info(&mut self, index: usize) -> Option<PluginParamInfo> {
        let mk = |id, name: &str, unit: &str, default_value, min, max| PluginParamInfo {
            id,
            name: name.to_string(),
            unit: unit.to_string(),
            default_value,
            min,
            max,
            step_count: 0,
            can_automate: true,
            is_modulatable: true,
        };
        match index {
            0 => Some(mk(0, "Left Pan", "%", 0.0, -100.0, 100.0)),
            1 => Some(mk(1, "Right Pan", "%", 1.0, -100.0, 100.0)),
            2 => Some(mk(2, "Gain", "dB", 24.0 / 36.0, -24.0, 12.0)),
            _ => None,
        }
    }

    fn get_param_normalized(&mut self, id: u32) -> Option<f32> {
        match id {
            0 => Some(((self.params.left_pan + 1.0) * 0.5).clamp(0.0, 1.0)),
            1 => Some(((self.params.right_pan + 1.0) * 0.5).clamp(0.0, 1.0)),
            2 => Some(((self.params.gain_db + 24.0) / 36.0).clamp(0.0, 1.0)),
            _ => None,
        }
    }

    fn set_param_normalized(&mut self, id: u32, value: f32) -> bool {
        let v = value.clamp(0.0, 1.0);
        let mut p = self.params;
        match id {
            0 => p.left_pan = -1.0 + v * 2.0,
            1 => p.right_pan = -1.0 + v * 2.0,
            2 => p.gain_db = -24.0 + v * 36.0,
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
    fn dual_pan_routes_channels() {
        let mut fx = DualPanEffect::new(48_000.0);
        // Default: L->L, R->R
        let (l, r) = fx.process_stereo(0.8, 0.2);
        assert!((l - 0.8).abs() < 1e-5 && (r - 0.2).abs() < 1e-5);

        // Swap via dual pan: L->R, R->L
        fx.set_params(DualPanParams {
            left_pan: 1.0,
            right_pan: -1.0,
            gain_db: 0.0,
        });
        let (l2, r2) = fx.process_stereo(0.8, 0.2);
        assert!((l2 - 0.2).abs() < 1e-5 && (r2 - 0.8).abs() < 1e-5);
    }
}
