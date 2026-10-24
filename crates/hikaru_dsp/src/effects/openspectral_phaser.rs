// crates/hikaru_dsp/src/effects/openspectral_phaser.rs
//
// Motor DSP del **OpenSpectral Phaser** (ingeniería inversa del UZU.vst3 /
// parche Pure Data "EBPhaselock"). Implementa la cascada de biquads con
// cálculo de `r` (radio), `phi` (ángulo) y coeficientes normalizados, más
// un LFO para el barrido del cutoff y mezcla dry/wet. Todo en Rust puro,
// sin alocaciones en la ruta de audio.

use std::f32::consts::PI;

/// Los 11 parámetros del plugin, en las unidades oficiales de la interfaz:
/// knobs normales 0..100, knobs bipolares -100..+100, toggles 0/1 y blocksize
/// discreto (512..16384).
#[derive(Clone, Copy, Debug)]
pub struct SpectralPhaserParams {
    pub width: f32,
    pub offset: f32,
    pub depth: f32,
    pub speed: f32,
    pub mix: f32,
    pub blur: f32,
    pub hz_per_oct: f32,
    pub times_five: f32,
    pub keep_bass: f32,
    pub blocksize: f32,
    pub bypass: f32,
}

impl Default for SpectralPhaserParams {
    fn default() -> Self {
        Self {
            width: 50.0,
            offset: 0.0,
            depth: 0.0,
            speed: 0.0,
            mix: 100.0,
            blur: 0.0,
            hz_per_oct: 0.0,
            times_five: 0.0,
            keep_bass: 0.0,
            blocksize: 1024.0,
            bypass: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Biquad {
    z1: f32,
    z2: f32,
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Biquad {
    #[inline]
    fn set_coeffs(&mut self, b0: f32, b1: f32, b2: f32, a1: f32, a2: f32) {
        self.b0 = b0;
        self.b1 = b1;
        self.b2 = b2;
        self.a1 = a1;
        self.a2 = a2;
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x + self.z2 - self.a1 * y;
        self.z2 = self.b2 * x - self.a2 * y;
        if y.is_finite() { y } else { 0.0 }
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// Phaser espectral de N secciones biquad.
#[derive(Clone, Debug)]
pub struct SpectralPhaser {
    sample_rate: f32,
    params: SpectralPhaserParams,
    sections: Vec<Biquad>,
    num_sections: usize,
    lfo_phase: f32,
    smoothed_cutoff: f32,
    bass_lp: f32,
}

impl SpectralPhaser {
    pub fn new(sample_rate: f32) -> Self {
        let mut s = Self {
            sample_rate: if sample_rate > 0.0 { sample_rate } else { 44100.0 },
            params: SpectralPhaserParams::default(),
            sections: Vec::new(),
            num_sections: 4,
            lfo_phase: 0.0,
            smoothed_cutoff: 1000.0,
            bass_lp: 0.0,
        };
        s.resize_sections();
        s
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        if sample_rate > 0.0 {
            self.sample_rate = sample_rate;
        }
    }

    pub fn set_params(&mut self, params: SpectralPhaserParams) {
        self.params = params;
        let width_norm = (params.width / 100.0).clamp(0.0, 1.0);
        let n = (2.0 + width_norm * 48.0).round() as usize;
        self.num_sections = n.max(1).min(100);
        self.resize_sections();
    }

    pub fn params(&self) -> SpectralPhaserParams {
        self.params
    }

    /// Cantidad de secciones biquad de la cascada (la fija `width`).
    pub fn num_sections(&self) -> usize {
        self.num_sections
    }

    pub fn reset(&mut self) {
        self.lfo_phase = 0.0;
        self.smoothed_cutoff = 1000.0;
        self.bass_lp = 0.0;
        for s in &mut self.sections {
            s.reset();
        }
    }

    fn resize_sections(&mut self) {
        self.sections.resize_with(self.num_sections, Biquad::default);
    }

    /// Recalcula los coeficientes para un cutoff dado (Hz).
    fn recompute(&mut self, cutoff: f32) {
        let nyq = self.sample_rate * 0.5;
        // r = tan(clip(cutoff/nyquist, 0, 1) * pi/2)
        let norm = (cutoff / nyq).clamp(0.0, 0.999);
        let r = (norm * (PI * 0.5)).tan().clamp(0.0, 0.9999);

        let mut prev_re = 0.0_f32;
        let mut prev_im = 0.0_f32;

        for k in 0..self.num_sections {
            // phi = (2k+1) * pi / (2 * num_sections)
            let phi = (2 * k + 1) as f32 * PI / (2.0 * self.num_sections as f32);
            let denom = 1.0 + r * r + 2.0 * r * phi.cos();
            let denom = if denom.abs() < 1e-9 { 1.0 } else { denom };
            let re = (1.0 - r * r) / denom;
            let im = (2.0 * r * phi.sin()) / denom;
            // g = ((prev_re + 1)^2 + prev_im^2) / 4
            let g = ((prev_re + 1.0).powi(2) + prev_im * prev_im) / 4.0;

            prev_re = re;
            prev_im = im;

            let b0 = 2.0 * re;
            let b1 = -(re * re + im * im);
            let b2 = g;
            let a1 = -2.0 * g;
            let a2 = g;

            if k < self.sections.len() {
                self.sections[k].set_coeffs(b0, b1, b2, a1, a2);
            }
        }
    }

    /// Frecuencia del LFO en Hz, con el multiplicador `1x/5x` y el piso de
    /// velocidad que usa el DSP. Es la misma cuenta que hace `process_sample`.
    pub fn lfo_rate_hz(&self) -> f32 {
        let mut rate = self.params.speed / 10.0;
        if self.params.times_five >= 0.5 {
            rate *= 5.0;
        }
        if rate != 0.0 && rate.abs() < 0.01 {
            rate = 0.01 * rate.signum();
        }
        rate
    }

    /// Cutoff (Hz) que produce el LFO en la fase dada, para los params
    /// actuales. Se usa tanto en la ruta de audio como en el análisis.
    pub fn cutoff_at_phase(&self, phase: f32) -> f32 {
        let lfo = if phase.is_finite() { phase.sin() } else { 0.0 };
        let base = 200.0 + (self.params.offset / 100.0) * 4000.0;
        let octaves = (self.params.depth / 100.0).clamp(0.0, 1.0)
            * lfo
            * (1.0 + (self.params.hz_per_oct / 100.0) * 4.0);
        let cutoff = base * (2.0_f32).powf(octaves);
        if cutoff.is_finite() {
            cutoff.clamp(20.0, self.sample_rate * 0.49)
        } else {
            20.0
        }
    }

    /// Fija el cutoff de análisis (Hz) y recalcula los coeficientes, sin
    /// procesar audio. Pensado para dibujar la curva de respuesta en la GUI.
    pub fn set_analysis_cutoff(&mut self, cutoff: f32) {
        self.smoothed_cutoff = if cutoff.is_finite() {
            cutoff.clamp(20.0, self.sample_rate * 0.49)
        } else {
            20.0
        };
        self.recompute(self.smoothed_cutoff);
    }

    /// Magnitud de la respuesta en frecuencia de la cascada completa
    /// (`|H(f)|`) para los coeficientes actuales.
    ///
    /// Es analítica: evalúa `H(z)` sobre el círculo unitario en vez de procesar
    /// audio, así que sirve para un scope en tiempo real sin costo de DSP.
    pub fn magnitude_response(&self, freq: f32) -> f32 {
        let freq = freq.clamp(1.0, self.sample_rate * 0.49);
        let w = 2.0 * PI * freq / self.sample_rate;
        let (sin1, cos1) = w.sin_cos();
        let (sin2, cos2) = (2.0 * w).sin_cos();

        let mut magnitude = 1.0_f32;
        for s in &self.sections {
            // H(z) = (b0 + b1 z^-1 + b2 z^-2) / (1 + a1 z^-1 + a2 z^-2)
            // con z^-1 = cos(w) - j·sin(w).
            let num_re = s.b0 + s.b1 * cos1 + s.b2 * cos2;
            let num_im = -(s.b1 * sin1 + s.b2 * sin2);
            let den_re = 1.0 + s.a1 * cos1 + s.a2 * cos2;
            let den_im = -(s.a1 * sin1 + s.a2 * sin2);
            let num = (num_re * num_re + num_im * num_im).sqrt();
            let den = (den_re * den_re + den_im * den_im).sqrt();
            if den > 1.0e-12 {
                magnitude *= num / den;
            }
        }
        if magnitude.is_finite() { magnitude } else { 0.0 }
    }

    /// Procesa una muestra. RT-safe: sin alocaciones.
    pub fn process_sample(&mut self, input: f32) -> f32 {
        if self.params.bypass >= 0.5 {
            return input;
        }

        self.lfo_phase += 2.0 * PI * self.lfo_rate_hz() / self.sample_rate;
        if self.lfo_phase > 2.0 * PI {
            self.lfo_phase -= 2.0 * PI;
        }

        let cutoff = self.cutoff_at_phase(self.lfo_phase);

        let blur = (self.params.blur / 100.0).clamp(0.0, 1.0);
        let alpha = 1.0 - blur * 0.9;
        self.smoothed_cutoff += (cutoff - self.smoothed_cutoff) * alpha;

        self.recompute(self.smoothed_cutoff);

        let mut x = input;
        for s in &mut self.sections {
            x = s.process(x);
        }

        let mut out = x;
        if self.params.keep_bass >= 0.5 {
            let a = (2.0 * PI * 120.0 / self.sample_rate).clamp(0.0, 1.0);
            self.bass_lp += a * (input - self.bass_lp);
            out += self.bass_lp;
        }

        let wet = (self.params.mix / 100.0).clamp(0.0, 1.0);
        let y = input * (1.0 - wet) + out * wet;
        if y.is_finite() { y } else { input }
    }

    /// Procesa un bloque interleaved (mono). RT-safe.
    pub fn process_block(&mut self, buffer: &mut [f32]) {
        for x in buffer.iter_mut() {
            *x = self.process_sample(*x);
        }
    }

    /// Procesa un bloque estéreo interleaved (L,R,...).
    pub fn process_block_stereo(&mut self, buffer: &mut [f32]) {
        // Secciones compartidas; LFO común.
        for frame in buffer.chunks_mut(2) {
            let l = self.process_sample(frame[0]);
            let r = self.process_sample(frame[1]);
            frame[0] = l;
            frame[1] = r;
        }
    }
}

// Aliases para compatibilidad con nombres usados en la ingeniería inversa.
pub type OpenSpectralPhaser = SpectralPhaser;
pub type Phaselock = SpectralPhaser;
pub type EBPhaselock = SpectralPhaser;
pub type OpenSpectralPhaserParams = SpectralPhaserParams;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coeffs_finite() {
        let mut p = SpectralPhaser::new(44100.0);
        p.set_params(SpectralPhaserParams {
            width: 50.0,
            offset: 30.0,
            depth: 50.0,
            speed: 40.0,
            mix: 100.0,
            ..Default::default()
        });
        let mut buf = vec![0.0f32; 256];
        for (i, x) in buf.iter_mut().enumerate() {
            *x = (i as f32 * 0.1).sin() * 0.5;
        }
        p.process_block(&mut buf);
        assert!(buf.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn bypass_passthrough() {
        let mut p = SpectralPhaser::new(44100.0);
        p.set_params(SpectralPhaserParams { bypass: 1.0, ..Default::default() });
        let input = vec![0.1, -0.2, 0.3, -0.4];
        let mut buf = input.clone();
        p.process_block(&mut buf);
        assert_eq!(buf, input);
    }

    #[test]
    fn analytic_response_is_finite() {
        let mut p = SpectralPhaser::new(44100.0);
        p.set_params(SpectralPhaserParams {
            width: 50.0,
            depth: 60.0,
            speed: 20.0,
            ..Default::default()
        });
        let cutoff = p.cutoff_at_phase(1.234);
        assert!(cutoff.is_finite() && cutoff > 0.0);
        p.set_analysis_cutoff(cutoff);
        for freq in [40.0, 120.0, 800.0, 5_000.0, 15_000.0] {
            let magnitude = p.magnitude_response(freq);
            assert!(magnitude.is_finite(), "|H({freq})| no es finito: {magnitude}");
            assert!(magnitude >= 0.0);
        }
    }
}
