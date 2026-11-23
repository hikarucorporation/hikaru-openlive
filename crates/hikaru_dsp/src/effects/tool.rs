// crates/hikaru_dsp/src/effects/tool.rs

//! **Tool**: utility stereo processor (Gain, Pan, Width, Phase Invert L/R, Swap L/R).

use hikaru_core::AudioBuffer;
use hikaru_plugin_host::{PluginInstance, PluginParamInfo};

#[derive(Debug, Clone, Copy)]
pub struct ToolParams {
    /// Gain in dB (-36.0 .. +36.0).
    pub gain_db: f32,
    /// Stereo pan (-1.0 = L .. +1.0 = R).
    pub pan: f32,
    /// Stereo width (0.0 = mono, 1.0 = normal, 2.0 = wide).
    pub width: f32,
    pub invert_left: bool,
    pub invert_right: bool,
    pub swap_lr: bool,
}

impl Default for ToolParams {
    fn default() -> Self {
        Self {
            gain_db: 0.0,
            pan: 0.0,
            width: 1.0,
            invert_left: false,
            invert_right: false,
            swap_lr: false,
        }
    }
}

pub struct ToolEffect {
    params: ToolParams,
    gain_lin: f32,
}

impl ToolEffect {
    pub fn new(_sample_rate: f32) -> Self {
        let params = ToolParams::default();
        Self {
            gain_lin: db_to_lin(params.gain_db),
            params,
        }
    }

    pub fn set_params(&mut self, params: ToolParams) {
        self.params = ToolParams {
            gain_db: params.gain_db.clamp(-36.0, 36.0),
            pan: params.pan.clamp(-1.0, 1.0),
            width: params.width.clamp(0.0, 2.0),
            ..params
        };
        self.gain_lin = db_to_lin(self.params.gain_db);
    }

    pub fn params(&self) -> ToolParams {
        self.params
    }

    #[inline]
    pub fn process_stereo(&self, left: f32, right: f32) -> (f32, f32) {
        let (mut l, mut r) = if self.params.swap_lr {
            (right, left)
        } else {
            (left, right)
        };

        if self.params.invert_left {
            l = -l;
        }
        if self.params.invert_right {
            r = -r;
        }

        // Mid/Side stereo width
        let mid = 0.5 * (l + r);
        let side = 0.5 * (l - r) * self.params.width;
        l = mid + side;
        r = mid - side;

        // Constant-power pan
        let pan_l = (1.0 - self.params.pan).min(1.0);
        let pan_r = (1.0 + self.params.pan).min(1.0);

        (l * pan_l * self.gain_lin, r * pan_r * self.gain_lin)
    }
}

#[inline]
fn db_to_lin(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

impl PluginInstance for ToolEffect {
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
        "Tool"
    }

    fn param_count(&mut self) -> usize {
        6
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
            0 => Some(mk(0, "Gain", "dB", 0.5, -36.0, 36.0, 0)),
            1 => Some(mk(1, "Pan", "%", 0.5, -100.0, 100.0, 0)),
            2 => Some(mk(2, "Width", "%", 0.5, 0.0, 200.0, 0)),
            3 => Some(mk(3, "Invert L", "", 0.0, 0.0, 1.0, 1)),
            4 => Some(mk(4, "Invert R", "", 0.0, 0.0, 1.0, 1)),
            5 => Some(mk(5, "Swap L/R", "", 0.0, 0.0, 1.0, 1)),
            _ => None,
        }
    }

    fn get_param_normalized(&mut self, id: u32) -> Option<f32> {
        match id {
            0 => Some(((self.params.gain_db + 36.0) / 72.0).clamp(0.0, 1.0)),
            1 => Some(((self.params.pan + 1.0) * 0.5).clamp(0.0, 1.0)),
            2 => Some((self.params.width * 0.5).clamp(0.0, 1.0)),
            3 => Some(if self.params.invert_left { 1.0 } else { 0.0 }),
            4 => Some(if self.params.invert_right { 1.0 } else { 0.0 }),
            5 => Some(if self.params.swap_lr { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    fn set_param_normalized(&mut self, id: u32, value: f32) -> bool {
        let v = value.clamp(0.0, 1.0);
        let mut p = self.params;
        match id {
            0 => p.gain_db = -36.0 + v * 72.0,
            1 => p.pan = -1.0 + v * 2.0,
            2 => p.width = v * 2.0,
            3 => p.invert_left = v >= 0.5,
            4 => p.invert_right = v >= 0.5,
            5 => p.swap_lr = v >= 0.5,
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
    fn tool_gain_invert_and_swap() {
        let mut t = ToolEffect::new(48_000.0);
        let (l, r) = t.process_stereo(0.5, -0.25);
        assert!((l - 0.5).abs() < 1e-5 && (r + 0.25).abs() < 1e-5);

        t.set_params(ToolParams {
            gain_db: 6.0206,
            swap_lr: true,
            invert_left: true,
            ..Default::default()
        });
        let (l2, r2) = t.process_stereo(0.25, 0.5);
        assert!((l2 + 1.0).abs() < 1e-2);
        assert!((r2 - 0.5).abs() < 1e-2);
    }
}
