// crates/hikaru_dsp/src/effects/bit8.rs

//! **Bit-8**: bitcrusher (bit-depth quantization) + sample-rate reducer (clock downsample).

use hikaru_core::AudioBuffer;
use hikaru_plugin_host::{PluginInstance, PluginParamInfo};

#[derive(Debug, Clone, Copy)]
pub struct Bit8Params {
    /// Bit depth (1.0 .. 16.0).
    pub bits: f32,
    /// Sample-rate reduction factor (1.0 .. 64.0).
    pub clock_div: f32,
    /// Dry/wet mix (0.0 .. 1.0).
    pub mix: f32,
}

impl Default for Bit8Params {
    fn default() -> Self {
        Self {
            bits: 8.0,
            clock_div: 1.0,
            mix: 1.0,
        }
    }
}

pub struct Bit8Effect {
    params: Bit8Params,
    hold_l: f32,
    hold_r: f32,
    counter: f32,
}

impl Bit8Effect {
    pub fn new(_sample_rate: f32) -> Self {
        Self {
            params: Bit8Params::default(),
            hold_l: 0.0,
            hold_r: 0.0,
            counter: 0.0,
        }
    }

    pub fn set_params(&mut self, params: Bit8Params) {
        self.params = Bit8Params {
            bits: params.bits.clamp(1.0, 16.0),
            clock_div: params.clock_div.clamp(1.0, 64.0),
            mix: params.mix.clamp(0.0, 1.0),
        };
    }

    #[inline]
    fn quantize(&self, x: f32) -> f32 {
        let levels = (2.0_f32.powf(self.params.bits) - 1.0).max(1.0);
        (x.clamp(-1.0, 1.0) * levels).round() / levels
    }

    #[inline]
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        self.counter += 1.0;
        if self.counter >= self.params.clock_div {
            self.counter -= self.params.clock_div;
            self.hold_l = self.quantize(left);
            self.hold_r = self.quantize(right);
        }
        let m = self.params.mix;
        (
            left * (1.0 - m) + self.hold_l * m,
            right * (1.0 - m) + self.hold_r * m,
        )
    }
}

impl PluginInstance for Bit8Effect {
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
        "Bit-8"
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
            0 => Some(mk(0, "Bits", "bit", (8.0 - 1.0) / 15.0, 1.0, 16.0)),
            1 => Some(mk(1, "Clock Div", "x", 0.0, 1.0, 64.0)),
            2 => Some(mk(2, "Mix", "%", 1.0, 0.0, 100.0)),
            _ => None,
        }
    }

    fn get_param_normalized(&mut self, id: u32) -> Option<f32> {
        match id {
            0 => Some(((self.params.bits - 1.0) / 15.0).clamp(0.0, 1.0)),
            1 => Some(((self.params.clock_div - 1.0) / 63.0).clamp(0.0, 1.0)),
            2 => Some(self.params.mix),
            _ => None,
        }
    }

    fn set_param_normalized(&mut self, id: u32, value: f32) -> bool {
        let v = value.clamp(0.0, 1.0);
        let mut p = self.params;
        match id {
            0 => p.bits = 1.0 + v * 15.0,
            1 => p.clock_div = 1.0 + v * 63.0,
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
    fn bit8_quantizes_signal() {
        let mut fx = Bit8Effect::new(48_000.0);
        fx.set_params(Bit8Params {
            bits: 2.0,
            clock_div: 1.0,
            mix: 1.0,
        });
        let (l, _) = fx.process_stereo(0.37, 0.37);
        // 2 bits -> 3 levels -> round(0.37 * 3) / 3 = 1/3
        assert!((l - (1.0 / 3.0)).abs() < 1e-4, "got {l}");
    }
}
