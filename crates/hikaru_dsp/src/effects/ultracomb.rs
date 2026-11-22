// crates/hikaru_dsp/src/effects/ultracomb.rs

// use hikaru_core::SampleRate;

const MAX_DELAY: usize = 2048; // Suficiente para frecuencias bajas/medias

pub struct CombFilter {
    buffer: [f32; MAX_DELAY],
    write_ptr: usize,
    feedback: f32,
    damp: f32,
    last_out: f32,
}

impl CombFilter {
    pub fn new() -> Self {
        Self {
            buffer: [0.0; MAX_DELAY],
            write_ptr: 0,
            feedback: 0.5,
            damp: 0.2,
            last_out: 0.0,
        }
    }

    #[inline]
    pub fn process(&mut self, input: f32, delay_samples: f32) -> f32 {
        let size = MAX_DELAY as f32;
        
        // Calcular posición de lectura con interpolación lineal
        let read_ptr = (self.write_ptr as f32 - delay_samples + size) % size;
        let idx0 = read_ptr as usize;
        let idx1 = (idx0 + 1) % MAX_DELAY;
        let frac = read_ptr - idx0 as f32;

        let delayed = self.buffer[idx0] + frac * (self.buffer[idx1] - self.buffer[idx0]);

        // Filtro de damping (LP simple dentro del loop de feedback)
        let out = delayed * (1.0 - self.damp) + self.last_out * self.damp;
        self.last_out = out;

        // Saturación suave con techo alto (3.0) para no aplastar el Q resonante del feedback
        let raw = input + out * self.feedback;
        let fb_signal = 3.0 * (raw / 3.0).tanh();
        self.buffer[self.write_ptr] = fb_signal;
        self.write_ptr = (self.write_ptr + 1) % MAX_DELAY;

        out
    }

    pub fn set_feedback(&mut self, fb: f32) { self.feedback = fb.clamp(-0.99, 0.99); }
    pub fn set_damp(&mut self, d: f32) { self.damp = d.clamp(0.0, 0.9); }
}

/// Parameters for the Hikaru Ultracomb Filter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UltracombParams {
    /// Cutoff (0.0 .. 100.0).
    /// Closer to 0 gives higher-pitched, tighter comb resonances;
    /// closer to 100 gives larger, deeper resonant bodies.
    pub cutoff: f32,
    /// Resonance (0.0 .. 100.0).
    /// Controls feedback intensity and metallic sustain.
    pub resonance: f32,
}

impl Default for UltracombParams {
    fn default() -> Self {
        Self {
            cutoff: 50.0,
            resonance: 60.0,
        }
    }
}

/// Stereo resonant Ultracomb filter calibrated against `Au5 I DAW Nation_Ultracomb_Bass 15.wav`:
/// - `50%` Cutoff lands at ~3.16 ms (~316 Hz, matching the 3.21 ms / 311.7 Hz comb peak in the reference).
/// - Dual-comb + allpass dispersion with feedback soft-saturation and ~4.5% stereo spread
///   for the wide, metallic, liquid Au5 Ultracomb character.
pub struct UltracombFilter {
    sample_rate: f32,
    params: UltracombParams,
    comb_l: CombFilter,
    comb_r: CombFilter,
    /// Secondary octave-up comb per channel for dense Ultracomb harmonicity.
    sub_comb_l: CombFilter,
    sub_comb_r: CombFilter,
    /// Allpass dispersion state per channel for metallic ultracomb sheen.
    ap_l: f32,
    ap_r: f32,
}

impl UltracombFilter {
    pub fn new(sample_rate: f32) -> Self {
        let mut instance = Self {
            sample_rate: sample_rate.max(8000.0),
            params: UltracombParams::default(),
            comb_l: CombFilter::new(),
            comb_r: CombFilter::new(),
            sub_comb_l: CombFilter::new(),
            sub_comb_r: CombFilter::new(),
            ap_l: 0.0,
            ap_r: 0.0,
        };
        instance.sync_params();
        instance
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(8000.0);
    }

    pub fn set_params(&mut self, params: UltracombParams) {
        self.params = UltracombParams {
            cutoff: params.cutoff.clamp(0.0, 100.0),
            resonance: params.resonance.clamp(0.0, 100.0),
        };
        self.sync_params();
    }

    pub fn params(&self) -> UltracombParams {
        self.params
    }

    /// Maps `cutoff` (0..100) exponentially to delay in samples.
    /// 0.0 -> ~0.25 ms (high pitch), 100.0 -> ~40.0 ms (large/deep).
    pub fn delay_samples(&self) -> f32 {
        let t = (self.params.cutoff / 100.0).clamp(0.0, 1.0);
        let min_ms = 0.25_f32;
        let max_ms = 40.0_f32;
        let delay_ms = min_ms * (max_ms / min_ms).powf(t);
        let samples = (delay_ms * 0.001) * self.sample_rate;
        samples.clamp(1.0, (MAX_DELAY - 2) as f32)
    }

    /// Fundamental comb frequency in Hz (`sample_rate / delay_samples`).
    pub fn fundamental_hz(&self) -> f32 {
        self.sample_rate / self.delay_samples().max(1.0)
    }

    /// Computes normalized magnitude response (0.0..=1.0) across `num_bins`
    /// logarithmically spaced from 40 Hz to 18 kHz for UI scope visualization.
    pub fn frequency_response_curve(&self, num_bins: usize) -> Vec<f32> {
        let n = num_bins.max(2);
        let delay = self.delay_samples();
        let res_norm = (self.params.resonance / 100.0).clamp(0.0, 1.0);
        let fb = res_norm.powf(0.75) * 0.96;
        let damp = 0.25 - 0.12 * res_norm;

        let f_min = 40.0_f32;
        let f_max = (self.sample_rate * 0.45).min(18_000.0).max(f_min + 1.0);
        let mut out = Vec::with_capacity(n);

        for i in 0..n {
            let frac = i as f32 / (n - 1) as f32;
            let freq = f_min * (f_max / f_min).powf(frac);
            let w = std::f32::consts::TAU * freq / self.sample_rate;

            // H_damp(e^jw) = (1 - damp) / (1 - damp * e^-jw)
            let dr = 1.0 - damp * w.cos();
            let di = damp * w.sin();
            let d_den = (dr * dr + di * di).max(1e-6);
            let hd_r = (1.0 - damp) * dr / d_den;
            let hd_i = -(1.0 - damp) * di / d_den;

            // e^-jwD
            let wd = w * delay;
            let ed_r = wd.cos();
            let ed_i = -wd.sin();

            // fb * H_damp * e^-jwD
            let loop_r = fb * (hd_r * ed_r - hd_i * ed_i);
            let loop_i = fb * (hd_r * ed_i + hd_i * ed_r);

            // |1 / (1 - loop)|
            let den_r = 1.0 - loop_r;
            let den_i = -loop_i;
            let comb_mag = 1.0 / (den_r * den_r + den_i * den_i).sqrt().max(1e-4);

            // Compress peak height logarithmically for clean 0..1 display
            let db = 20.0 * comb_mag.log10();
            let norm = ((db + 12.0) / 40.0).clamp(0.03, 1.0);
            out.push(norm);
        }
        out
    }

    fn sync_params(&mut self) {
        let res_norm = (self.params.resonance / 100.0).clamp(0.0, 1.0);
        // Curve resonance so 0..100 reaches deep metallic Ultracomb Q without blowing up
        let fb = res_norm.powf(0.65) * 0.975;
        let sub_fb = fb * 0.60;
        // Less damping at higher resonance for bright metallic shimmer
        let damp = 0.20 - 0.12 * res_norm;
        self.comb_l.set_feedback(fb);
        self.comb_r.set_feedback(fb);
        self.comb_l.set_damp(damp);
        self.comb_r.set_damp(damp);

        self.sub_comb_l.set_feedback(sub_fb);
        self.sub_comb_r.set_feedback(sub_fb);
        self.sub_comb_l.set_damp(damp * 0.75);
        self.sub_comb_r.set_damp(damp * 0.75);
    }

    #[inline]
    pub fn process(&mut self, input: f32) -> f32 {
        let delay = self.delay_samples();
        let c1 = self.comb_l.process(input, delay);
        let c2 = self.sub_comb_l.process(c1 * 0.7 + input * 0.3, (delay * 0.5).max(1.0));
        let wet = c1 * 0.65 + c2 * 0.35;
        // 1-pole allpass dispersion for ultracomb metallic sheen
        let g = 0.42_f32;
        let ap_out = -g * wet + self.ap_l;
        self.ap_l = wet + g * ap_out;
        (0.15 * input + 0.85 * ap_out * 0.55).tanh()
    }

    #[inline]
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        let delay_l = self.delay_samples();
        // 4.5% stereo spread calibrated to match the wide stereo decorrelation in the Au5 reference
        let delay_r = (delay_l * 1.045).min((MAX_DELAY - 2) as f32);

        let c1_l = self.comb_l.process(left, delay_l);
        let c1_r = self.comb_r.process(right, delay_r);

        let c2_l = self.sub_comb_l.process(c1_l * 0.7 + left * 0.3, (delay_l * 0.5).max(1.0));
        let c2_r = self.sub_comb_r.process(c1_r * 0.7 + right * 0.3, (delay_r * 0.5).max(1.0));

        let wet_l = c1_l * 0.65 + c2_l * 0.35;
        let wet_r = c1_r * 0.65 + c2_r * 0.35;

        let g = 0.42_f32;
        let ap_out_l = -g * wet_l + self.ap_l;
        self.ap_l = wet_l + g * ap_out_l;

        let ap_out_r = -g * wet_r + self.ap_r;
        self.ap_r = wet_r + g * ap_out_r;

        (
            (0.15 * left + 0.85 * ap_out_l * 0.55).tanh(),
            (0.15 * right + 0.85 * ap_out_r * 0.55).tanh(),
        )
    }
}

impl hikaru_plugin_host::PluginInstance for UltracombFilter {
    fn process(&mut self, buffer: &mut hikaru_core::AudioBuffer) {
        for frame in buffer.samples.chunks_exact_mut(2) {
            let (l, r) = self.process_stereo(frame[0], frame[1]);
            frame[0] = l;
            frame[1] = r;
        }
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.set_sample_rate(sample_rate);
    }

    fn show_gui_embedded(&mut self, _window_handle: hikaru_plugin_host::RawWindowHandle) -> Result<(), String> {
        Err("Native plugin".to_string())
    }

    fn show_gui_floating(&mut self) -> Result<(), String> {
        Err("Native plugin".to_string())
    }

    fn hide_gui(&mut self) {}

    fn get_name(&self) -> &str {
        "Hikaru Ultracomb"
    }

    fn param_count(&mut self) -> usize {
        2
    }

    fn param_info(&mut self, index: usize) -> Option<hikaru_plugin_host::PluginParamInfo> {
        match index {
            0 => Some(hikaru_plugin_host::PluginParamInfo {
                id: 0,
                name: "Cutoff".to_string(),
                unit: "%".to_string(),
                default_value: 0.5,
                min: 0.0,
                max: 100.0,
                step_count: 0,
                can_automate: true,
                is_modulatable: true,
            }),
            1 => Some(hikaru_plugin_host::PluginParamInfo {
                id: 1,
                name: "Resonance".to_string(),
                unit: "%".to_string(),
                default_value: 0.6,
                min: 0.0,
                max: 100.0,
                step_count: 0,
                can_automate: true,
                is_modulatable: true,
            }),
            _ => None,
        }
    }

    fn get_param_normalized(&mut self, id: u32) -> Option<f32> {
        match id {
            0 => Some((self.params.cutoff / 100.0).clamp(0.0, 1.0)),
            1 => Some((self.params.resonance / 100.0).clamp(0.0, 1.0)),
            _ => None,
        }
    }

    fn set_param_normalized(&mut self, id: u32, value: f32) -> bool {
        let v = value.clamp(0.0, 1.0) * 100.0;
        let mut p = self.params;
        match id {
            0 => p.cutoff = v,
            1 => p.resonance = v,
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
    fn cutoff_zero_is_shorter_delay_than_cutoff_hundred() {
        let mut uc = UltracombFilter::new(48_000.0);
        uc.set_params(UltracombParams { cutoff: 0.0, resonance: 50.0 });
        let d_low = uc.delay_samples();

        uc.set_params(UltracombParams { cutoff: 100.0, resonance: 50.0 });
        let d_high = uc.delay_samples();

        assert!(d_low < d_high, "cutoff=0 ({d_low}) should have shorter delay than cutoff=100 ({d_high})");
        assert!(d_low >= 1.0);
        assert!(d_high <= (MAX_DELAY - 2) as f32);
    }

    #[test]
    fn high_resonance_stays_finite_and_bounded() {
        let mut uc = UltracombFilter::new(48_000.0);
        uc.set_params(UltracombParams { cutoff: 35.0, resonance: 100.0 });
        for i in 0..4096 {
            let inp = if i == 0 { 1.0 } else { 0.0 };
            let (l, r) = uc.process_stereo(inp, inp);
            assert!(l.is_finite() && r.is_finite());
            assert!(l.abs() <= 1.5 && r.abs() <= 1.5);
        }
    }
}