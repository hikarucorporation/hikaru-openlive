/*
 * Hikaru OpenLive / OpenStudio - Audio DAW
 * Copyright (C) 2026 Hikaru OpenStudio Developers
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU Affero General Public License for more details.
 */

//! **Hikaru OpenComb**: filtro peine (*comb filter*) estéreo limpio e interpolado,
//! diseñado a partir del comportamiento público observable de **Comb** de Bitwig Studio.
//!
//! Parámetros públicos (`3`):
//! - `Frequency` (`20.0 Hz ..= 3470.0 Hz`, escala logarítmica/pitch, default `104.0 Hz`)
//! - `Feedback` (`-100.0 % ..= +100.0 %`, bipolar, default `0.0 %`):
//!   - `> 0%`: refuerza DC y armónicos enteros ($f_0, 2f_0, 3f_0, \dots$)
//!   - `< 0%`: invierte fase en el lazo y refuerza múltiplos impares de la mitad
//!     ($\frac{1}{2}f_0, \frac{3}{2}f_0, \frac{5}{2}f_0, \dots$)
//! - `Mix` (`0.0 % ..= 100.0 %`, default `100.0 %`)

use hikaru_core::AudioBuffer;
use hikaru_plugin_host::{PluginInstance, PluginParamInfo};

pub const OPENCOMB_MIN_FREQ_HZ: f32 = 20.0;
pub const OPENCOMB_MAX_FREQ_HZ: f32 = 3470.0;
pub const OPENCOMB_DEFAULT_FREQ_HZ: f32 = 104.0;

pub const OPENCOMB_PARAM_FREQ: u32 = 0;
pub const OPENCOMB_PARAM_FEEDBACK: u32 = 1;
pub const OPENCOMB_PARAM_MIX: u32 = 2;
pub const OPENCOMB_PARAM_COUNT: usize = 3;

/// Potencia de 2 suficiente para `20 Hz` incluso a `192 kHz` (`192000 / 20 = 9600` muestras).
const DELAY_BUF_LEN: usize = 16384;
const DELAY_BUF_MASK: usize = DELAY_BUF_LEN - 1;

/// Ganancia máxima interna del lazo cuando `Feedback = ±100%` para evitar explosión infinita
/// manteniendo resonancia metálica extrema.
const MAX_FEEDBACK_GAIN: f64 = 0.985;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpenCombParams {
    /// Frecuencia fundamental del peine en Hz (`20.0 ..= 3470.0`).
    pub freq_hz: f32,
    /// Realimentación bipolar en porcentaje (`-100.0 ..= 100.0`).
    pub feedback_pct: f32,
    /// Mezcla Dry/Wet en porcentaje (`0.0 ..= 100.0`).
    pub mix_pct: f32,
}

impl Default for OpenCombParams {
    fn default() -> Self {
        Self {
            freq_hz: OPENCOMB_DEFAULT_FREQ_HZ,
            feedback_pct: 0.0,
            mix_pct: 100.0,
        }
    }
}

impl OpenCombParams {
    #[inline]
    pub fn freq_hz_to_norm(freq_hz: f32) -> f32 {
        let f = freq_hz.clamp(OPENCOMB_MIN_FREQ_HZ, OPENCOMB_MAX_FREQ_HZ);
        let ratio = OPENCOMB_MAX_FREQ_HZ / OPENCOMB_MIN_FREQ_HZ;
        ((f / OPENCOMB_MIN_FREQ_HZ).ln() / ratio.ln()).clamp(0.0, 1.0)
    }

    #[inline]
    pub fn norm_to_freq_hz(norm: f32) -> f32 {
        let t = norm.clamp(0.0, 1.0);
        let ratio = OPENCOMB_MAX_FREQ_HZ / OPENCOMB_MIN_FREQ_HZ;
        OPENCOMB_MIN_FREQ_HZ * ratio.powf(t)
    }

    #[inline]
    pub fn feedback_pct_to_norm(fb_pct: f32) -> f32 {
        ((fb_pct.clamp(-100.0, 100.0) + 100.0) / 200.0).clamp(0.0, 1.0)
    }

    #[inline]
    pub fn norm_to_feedback_pct(norm: f32) -> f32 {
        (norm.clamp(0.0, 1.0) * 200.0 - 100.0).clamp(-100.0, 100.0)
    }

    #[inline]
    pub fn mix_pct_to_norm(mix_pct: f32) -> f32 {
        (mix_pct / 100.0).clamp(0.0, 1.0)
    }

    #[inline]
    pub fn norm_to_mix_pct(norm: f32) -> f32 {
        (norm.clamp(0.0, 1.0) * 100.0).clamp(0.0, 100.0)
    }

    pub fn get_normalized(&self, id: u32) -> Option<f32> {
        match id {
            OPENCOMB_PARAM_FREQ => Some(Self::freq_hz_to_norm(self.freq_hz)),
            OPENCOMB_PARAM_FEEDBACK => Some(Self::feedback_pct_to_norm(self.feedback_pct)),
            OPENCOMB_PARAM_MIX => Some(Self::mix_pct_to_norm(self.mix_pct)),
            _ => None,
        }
    }

    pub fn set_normalized(&mut self, id: u32, value: f32) -> bool {
        let v = value.clamp(0.0, 1.0);
        match id {
            OPENCOMB_PARAM_FREQ => self.freq_hz = Self::norm_to_freq_hz(v),
            OPENCOMB_PARAM_FEEDBACK => self.feedback_pct = Self::norm_to_feedback_pct(v),
            OPENCOMB_PARAM_MIX => self.mix_pct = Self::norm_to_mix_pct(v),
            _ => return false,
        }
        true
    }
}

pub fn format_freq_label(freq_hz: f32) -> String {
    let f = freq_hz.clamp(OPENCOMB_MIN_FREQ_HZ, OPENCOMB_MAX_FREQ_HZ);
    if f >= 1000.0 {
        format!("{:.2} kHz", f / 1000.0)
    } else if f >= 100.0 {
        format!("{f:.0} Hz")
    } else {
        format!("{f:.1} Hz")
    }
}

/// Convierte una frecuencia en Hz al nombre de nota MIDI (convención Bitwig: C3 = 60 / 261.63 Hz).
pub fn freq_hz_to_note_label(freq_hz: f32) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let f = freq_hz.max(1.0);
    let midi_float = 69.0 + 12.0 * (f / 440.0).log2();
    let midi = midi_float.round() as i32;
    let cents = ((midi_float - midi as f32) * 100.0).round() as i32;
    let note_idx = midi.rem_euclid(12) as usize;
    let octave = midi.div_euclid(12) - 2;
    if cents == 0 {
        format!("{} {}", NAMES[note_idx], octave)
    } else {
        format!("{} {} {:+}", NAMES[note_idx], octave, cents)
    }
}

struct HermiteCombLine {
    buffer: Vec<f64>,
    write_idx: usize,
}

impl HermiteCombLine {
    fn new() -> Self {
        Self {
            buffer: vec![0.0; DELAY_BUF_LEN],
            write_idx: 0,
        }
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write_idx = 0;
    }

    #[inline]
    fn read_hermite(&self, delay_samples: f64) -> f64 {
        let d = delay_samples.clamp(1.0, (DELAY_BUF_LEN - 4) as f64);
        let d_int = d.floor() as usize;
        let frac = d - d_int as f64;

        let idx0 = (self.write_idx + DELAY_BUF_LEN - d_int + 1) & DELAY_BUF_MASK;
        let idx1 = (self.write_idx + DELAY_BUF_LEN - d_int) & DELAY_BUF_MASK;
        let idx2 = (self.write_idx + DELAY_BUF_LEN - d_int - 1) & DELAY_BUF_MASK;
        let idx3 = (self.write_idx + DELAY_BUF_LEN - d_int - 2) & DELAY_BUF_MASK;

        let ym1 = self.buffer[idx0];
        let y0 = self.buffer[idx1];
        let y1 = self.buffer[idx2];
        let y2 = self.buffer[idx3];

        // Interpolación cúbica de Hermite de 4 puntos (Catmull-Rom)
        let c0 = y0;
        let c1 = 0.5 * (y1 - ym1);
        let c2 = ym1 - 2.5 * y0 + 2.0 * y1 - 0.5 * y2;
        let c3 = 0.5 * (y2 - ym1) + 1.5 * (y0 - y1);
        ((c3 * frac + c2) * frac + c1) * frac + c0
    }

    #[inline]
    fn process(&mut self, input: f64, delay_samples: f64, fb_gain: f64) -> f64 {
        let delayed = self.read_hermite(delay_samples);
        let out = input + fb_gain * delayed;
        // Techo alto (1.5..4.0) para mantener Feedback=0% bit-a-bit transparente
        // y proteger contra blowup cuando Feedback=±100%.
        let safe_fb = if !out.is_finite() {
            0.0
        } else if out.abs() <= 1.5 {
            out
        } else {
            let excess = (out.abs() - 1.5) / 2.5;
            out.signum() * (1.5 + 2.5 * excess.tanh())
        };
        self.buffer[self.write_idx] = safe_fb;
        self.write_idx = (self.write_idx + 1) & DELAY_BUF_MASK;
        safe_fb
    }
}

/// Filtro peine estéreo (`true stereo`, mismo retardo L/R) con interpolación Hermite.
pub struct OpenCombFilter {
    sample_rate: f32,
    params: OpenCombParams,
    line_l: HermiteCombLine,
    line_r: HermiteCombLine,
    cur_delay: f64,
    cur_fb: f64,
    cur_mix: f64,
    smooth_coeff: f64,
}

impl OpenCombFilter {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8000.0);
        let params = OpenCombParams::default();
        let target_delay = Self::calc_delay_samples(sr, params.freq_hz);
        let mut instance = Self {
            sample_rate: sr,
            params,
            line_l: HermiteCombLine::new(),
            line_r: HermiteCombLine::new(),
            cur_delay: target_delay,
            cur_fb: 0.0,
            cur_mix: 1.0,
            smooth_coeff: Self::calc_smooth_coeff(sr),
        };
        instance.snap_smoothers();
        instance
    }

    #[inline]
    fn calc_smooth_coeff(sample_rate: f32) -> f64 {
        let tau_samples = 0.004_f64 * sample_rate.max(8000.0) as f64;
        1.0 - (-1.0 / tau_samples).exp()
    }

    #[inline]
    fn calc_delay_samples(sample_rate: f32, freq_hz: f32) -> f64 {
        let f = freq_hz.clamp(OPENCOMB_MIN_FREQ_HZ, OPENCOMB_MAX_FREQ_HZ) as f64;
        (sample_rate.max(8000.0) as f64 / f).clamp(1.0, (DELAY_BUF_LEN - 4) as f64)
    }

    fn snap_smoothers(&mut self) {
        self.cur_delay = Self::calc_delay_samples(self.sample_rate, self.params.freq_hz);
        self.cur_fb = (self.params.feedback_pct as f64 / 100.0) * MAX_FEEDBACK_GAIN;
        self.cur_mix = (self.params.mix_pct as f64 / 100.0).clamp(0.0, 1.0);
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(8000.0);
        self.smooth_coeff = Self::calc_smooth_coeff(self.sample_rate);
        self.line_l.reset();
        self.line_r.reset();
        self.snap_smoothers();
    }

    pub fn set_params(&mut self, params: OpenCombParams) {
        self.params = OpenCombParams {
            freq_hz: params
                .freq_hz
                .clamp(OPENCOMB_MIN_FREQ_HZ, OPENCOMB_MAX_FREQ_HZ),
            feedback_pct: params.feedback_pct.clamp(-100.0, 100.0),
            mix_pct: params.mix_pct.clamp(0.0, 100.0),
        };
    }

    pub fn params(&self) -> OpenCombParams {
        self.params
    }

    pub fn delay_samples(&self) -> f32 {
        Self::calc_delay_samples(self.sample_rate, self.params.freq_hz) as f32
    }

    /// Calcula la curva de respuesta en magnitud normalizada (`0.0 ..= 1.0`, `0 dB` en `0.5`)
    /// en `num_bins` espaciados logarítmicamente entre `20 Hz` y `4900 Hz` para el display XY.
    pub fn frequency_response_curve(&self, num_bins: usize) -> Vec<f32> {
        let n = num_bins.max(2);
        let delay = Self::calc_delay_samples(self.sample_rate, self.params.freq_hz) as f32;
        let fb = (self.params.feedback_pct / 100.0) * 0.92;
        let mix = (self.params.mix_pct / 100.0).clamp(0.0, 1.0);

        let f_min = 20.0_f32;
        let f_max = 4900.0_f32;
        let mut out = Vec::with_capacity(n);

        for i in 0..n {
            let frac = i as f32 / (n - 1) as f32;
            let freq = f_min * (f_max / f_min).powf(frac);
            let w = std::f32::consts::TAU * freq / self.sample_rate;
            let wd = w * delay;

            // H(e^jw) = (1 - mix) + mix / (1 - fb * e^-jwD)
            let den_r = 1.0 - fb * wd.cos();
            let den_i = fb * wd.sin();
            let den_sq = (den_r * den_r + den_i * den_i).max(1e-5);
            let wet_r = den_r / den_sq;
            let wet_i = -den_i / den_sq;

            let tot_r = (1.0 - mix) + mix * wet_r;
            let tot_i = mix * wet_i;
            let mag = (tot_r * tot_r + tot_i * tot_i).sqrt().max(1e-4);

            // Mapeo logarítmico en dB: 0 dB en 0.5, rango aprox -18 dB .. +18 dB
            let db = 20.0 * mag.log10();
            let norm = (0.5 + db / 36.0).clamp(0.04, 0.96);
            out.push(norm);
        }
        out
    }

    #[inline]
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        let target_delay = Self::calc_delay_samples(self.sample_rate, self.params.freq_hz);
        let target_fb = (self.params.feedback_pct as f64 / 100.0) * MAX_FEEDBACK_GAIN;
        let target_mix = (self.params.mix_pct as f64 / 100.0).clamp(0.0, 1.0);

        let c = self.smooth_coeff;
        self.cur_delay += (target_delay - self.cur_delay) * c;
        self.cur_fb += (target_fb - self.cur_fb) * c;
        self.cur_mix += (target_mix - self.cur_mix) * c;

        let in_l = left as f64;
        let in_r = right as f64;
        let wet_l = self.line_l.process(in_l, self.cur_delay, self.cur_fb);
        let wet_r = self.line_r.process(in_r, self.cur_delay, self.cur_fb);

        let dry = 1.0 - self.cur_mix;
        let out_l = dry * in_l + self.cur_mix * wet_l;
        let out_r = dry * in_r + self.cur_mix * wet_r;
        (out_l as f32, out_r as f32)
    }
}

impl PluginInstance for OpenCombFilter {
    fn process(&mut self, buffer: &mut AudioBuffer) {
        for frame in buffer.samples.chunks_exact_mut(2) {
            let (l, r) = self.process_stereo(frame[0], frame[1]);
            frame[0] = l;
            frame[1] = r;
        }
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.set_sample_rate(sample_rate);
    }

    fn show_gui_embedded(
        &mut self,
        _window_handle: hikaru_plugin_host::RawWindowHandle,
    ) -> Result<(), String> {
        Err("Native plugin".to_string())
    }

    fn show_gui_floating(&mut self) -> Result<(), String> {
        Err("Native plugin".to_string())
    }

    fn hide_gui(&mut self) {}

    fn get_name(&self) -> &str {
        "Hikaru OpenComb"
    }

    fn param_count(&mut self) -> usize {
        OPENCOMB_PARAM_COUNT
    }

    fn param_info(&mut self, index: usize) -> Option<PluginParamInfo> {
        match index {
            0 => Some(PluginParamInfo {
                id: OPENCOMB_PARAM_FREQ,
                name: "Frequency".to_string(),
                unit: "Hz".to_string(),
                default_value: OpenCombParams::freq_hz_to_norm(OPENCOMB_DEFAULT_FREQ_HZ),
                min: OPENCOMB_MIN_FREQ_HZ,
                max: OPENCOMB_MAX_FREQ_HZ,
                step_count: 0,
                can_automate: true,
                is_modulatable: true,
            }),
            1 => Some(PluginParamInfo {
                id: OPENCOMB_PARAM_FEEDBACK,
                name: "Feedback".to_string(),
                unit: "%".to_string(),
                default_value: 0.5,
                min: -100.0,
                max: 100.0,
                step_count: 0,
                can_automate: true,
                is_modulatable: true,
            }),
            2 => Some(PluginParamInfo {
                id: OPENCOMB_PARAM_MIX,
                name: "Mix".to_string(),
                unit: "%".to_string(),
                default_value: 1.0,
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
        self.params.get_normalized(id)
    }

    fn set_param_normalized(&mut self, id: u32, value: f32) -> bool {
        let mut p = self.params;
        if !p.set_normalized(id, value) {
            return false;
        }
        self.set_params(p);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measure_sine_rms(comb: &mut OpenCombFilter, freq_hz: f32, sr: f32) -> f32 {
        let total = 4800;
        let skip = 2400;
        let mut sum_sq = 0.0_f32;
        for i in 0..total {
            let x = (std::f32::consts::TAU * freq_hz * i as f32 / sr).sin() * 0.25;
            let (y, _) = comb.process_stereo(x, x);
            if i >= skip {
                sum_sq += y * y;
            }
        }
        (sum_sq / (total - skip) as f32).sqrt()
    }

    #[test]
    fn feedback_zero_is_transparent_at_full_mix() {
        let mut comb = OpenCombFilter::new(48_000.0);
        comb.set_params(OpenCombParams {
            freq_hz: 220.0,
            feedback_pct: 0.0,
            mix_pct: 100.0,
        });
        for i in 0..512 {
            let x = (i as f32 * 0.13).sin() * 0.5;
            let (l, r) = comb.process_stereo(x, x);
            assert!((l - x).abs() < 1e-6);
            assert!((r - x).abs() < 1e-6);
        }
    }

    #[test]
    fn bipolar_feedback_resonates_at_f0_for_positive_and_half_f0_for_negative() {
        let sr = 48_000.0;
        let mut pos = OpenCombFilter::new(sr);
        pos.set_params(OpenCombParams {
            freq_hz: 100.0,
            feedback_pct: 85.0,
            mix_pct: 100.0,
        });
        let pos_100 = measure_sine_rms(&mut pos, 100.0, sr);
        let pos_50 = measure_sine_rms(&mut pos, 50.0, sr);
        assert!(
            pos_100 > pos_50 * 3.0,
            "Positive feedback should boost f0=100Hz ({pos_100}) over f0/2=50Hz ({pos_50})"
        );

        let mut neg = OpenCombFilter::new(sr);
        neg.set_params(OpenCombParams {
            freq_hz: 100.0,
            feedback_pct: -85.0,
            mix_pct: 100.0,
        });
        let neg_50 = measure_sine_rms(&mut neg, 50.0, sr);
        let neg_100 = measure_sine_rms(&mut neg, 100.0, sr);
        assert!(
            neg_50 > neg_100 * 3.0,
            "Negative feedback should boost f0/2=50Hz ({neg_50}) over f0=100Hz ({neg_100})"
        );
    }

    #[test]
    fn dual_mono_channels_stay_identical_and_bounded_at_extreme_feedback() {
        for fb in [-100.0_f32, 100.0_f32] {
            for freq in [20.0_f32, 3470.0_f32] {
                let mut comb = OpenCombFilter::new(48_000.0);
                comb.set_params(OpenCombParams {
                    freq_hz: freq,
                    feedback_pct: fb,
                    mix_pct: 100.0,
                });
                for i in 0..4096 {
                    let inp = if i == 0 { 1.0 } else { 0.0 };
                    let (l, r) = comb.process_stereo(inp, inp);
                    assert!(l.is_finite() && r.is_finite());
                    assert!((l - r).abs() < 1e-6, "True stereo L/R should match on mono input");
                    assert!(l.abs() <= 4.1);
                }
            }
        }
    }
}
