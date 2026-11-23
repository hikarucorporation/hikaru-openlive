// crates/hikaru_dsp/src/effects/time_shift.rs

//! **Time Shift**: precise sample / millisecond delay utility.

use hikaru_core::AudioBuffer;
use hikaru_plugin_host::{PluginInstance, PluginParamInfo};

const MAX_DELAY_MS: f32 = 100.0;

#[derive(Debug, Clone, Copy)]
pub struct TimeShiftParams {
    /// Delay in milliseconds (0.0 .. 100.0).
    pub delay_ms: f32,
    /// Fine offset in samples (0.0 .. 100.0).
    pub fine_samples: f32,
    /// Dry/wet mix (0.0 .. 1.0).
    pub mix: f32,
}

impl Default for TimeShiftParams {
    fn default() -> Self {
        Self {
            delay_ms: 0.0,
            fine_samples: 0.0,
            mix: 1.0,
        }
    }
}

pub struct TimeShiftEffect {
    params: TimeShiftParams,
    sample_rate: f32,
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    write_pos: usize,
}

impl TimeShiftEffect {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        let cap = ((sr * MAX_DELAY_MS / 1000.0) as usize) + 256;
        Self {
            params: TimeShiftParams::default(),
            sample_rate: sr,
            buf_l: vec![0.0; cap],
            buf_r: vec![0.0; cap],
            write_pos: 0,
        }
    }

    pub fn set_params(&mut self, params: TimeShiftParams) {
        self.params = TimeShiftParams {
            delay_ms: params.delay_ms.clamp(0.0, MAX_DELAY_MS),
            fine_samples: params.fine_samples.clamp(0.0, 100.0),
            mix: params.mix.clamp(0.0, 1.0),
        };
    }

    #[inline]
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        let len = self.buf_l.len();
        self.buf_l[self.write_pos] = left;
        self.buf_r[self.write_pos] = right;

        let delay_samps = ((self.params.delay_ms * 0.001 * self.sample_rate)
            + self.params.fine_samples)
            .clamp(0.0, (len - 2) as f32);

        let d_floor = delay_samps.floor() as usize;
        let frac = delay_samps - d_floor as f32;

        let idx0 = (self.write_pos + len - d_floor) % len;
        let idx1 = (self.write_pos + len - d_floor - 1) % len;

        let wet_l = self.buf_l[idx0] * (1.0 - frac) + self.buf_l[idx1] * frac;
        let wet_r = self.buf_r[idx0] * (1.0 - frac) + self.buf_r[idx1] * frac;

        self.write_pos = (self.write_pos + 1) % len;

        let m = self.params.mix;
        (left * (1.0 - m) + wet_l * m, right * (1.0 - m) + wet_r * m)
    }
}

impl PluginInstance for TimeShiftEffect {
    fn process(&mut self, buffer: &mut AudioBuffer) {
        for frame in buffer.samples.chunks_exact_mut(2) {
            let (l, r) = self.process_stereo(frame[0], frame[1]);
            frame[0] = l;
            frame[1] = r;
        }
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        let sr = sample_rate.max(8_000.0);
        if (sr - self.sample_rate).abs() > 1.0 {
            self.sample_rate = sr;
            let cap = ((sr * MAX_DELAY_MS / 1000.0) as usize) + 256;
            self.buf_l.resize(cap, 0.0);
            self.buf_r.resize(cap, 0.0);
            self.buf_l.fill(0.0);
            self.buf_r.fill(0.0);
            self.write_pos = 0;
        }
    }

    fn show_gui_embedded(&mut self, _: hikaru_plugin_host::RawWindowHandle) -> Result<(), String> {
        Err("Native plugin".to_string())
    }
    fn show_gui_floating(&mut self) -> Result<(), String> {
        Err("Native plugin".to_string())
    }
    fn hide_gui(&mut self) {}

    fn get_name(&self) -> &str {
        "Time Shift"
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
            0 => Some(mk(0, "Delay", "ms", 0.0, 0.0, MAX_DELAY_MS)),
            1 => Some(mk(1, "Fine", "smp", 0.0, 0.0, 100.0)),
            2 => Some(mk(2, "Mix", "%", 1.0, 0.0, 100.0)),
            _ => None,
        }
    }

    fn get_param_normalized(&mut self, id: u32) -> Option<f32> {
        match id {
            0 => Some((self.params.delay_ms / MAX_DELAY_MS).clamp(0.0, 1.0)),
            1 => Some((self.params.fine_samples / 100.0).clamp(0.0, 1.0)),
            2 => Some(self.params.mix),
            _ => None,
        }
    }

    fn set_param_normalized(&mut self, id: u32, value: f32) -> bool {
        let v = value.clamp(0.0, 1.0);
        let mut p = self.params;
        match id {
            0 => p.delay_ms = v * MAX_DELAY_MS,
            1 => p.fine_samples = v * 100.0,
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
    fn time_shift_delays_impulse() {
        let mut fx = TimeShiftEffect::new(48_000.0);
        fx.set_params(TimeShiftParams {
            delay_ms: 0.0,
            fine_samples: 4.0,
            mix: 1.0,
        });
        let mut out = Vec::new();
        for i in 0..8 {
            let inp = if i == 0 { 1.0 } else { 0.0 };
            let (l, _) = fx.process_stereo(inp, inp);
            out.push(l);
        }
        assert!((out[4] - 1.0).abs() < 1e-5, "Impulse should appear at sample 4: {out:?}");
    }
}
