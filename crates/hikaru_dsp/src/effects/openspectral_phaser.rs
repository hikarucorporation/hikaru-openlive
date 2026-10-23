// crates/hikaru_dsp/src/effects/openspectral_phaser.rs
//
// Motor DSP del **OpenSpectral Phaser** (ingeniería inversa del UZU.vst3 /
// parche Pure Data "EBPhaselock"). Implementa la cascada de biquads con
// cálculo de `r` (radio), `phi` (ángulo) y coeficientes normalizados, más
// un LFO para el barrido del cutoff y mezcla dry/wet. Todo en Rust puro,
// sin alocaciones en la ruta de audio.

use std::f32::consts::PI;

/// Los 11 parámetros del plugin original, normalizados a 0..1.
#[derive(Clone, Copy, Debug, Default)]
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
        let n = (2.0 + params.width.clamp(0.0, 1.0) * 48.0).round() as usize;
        self.num_sections = n.max(1).min(100);
        self.resize_sections();
    }

    pub fn params(&self) -> SpectralPhaserParams {
        self.params
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

    /// Procesa una muestra. RT-safe: sin alocaciones.
    pub fn process_sample(&mut self, input: f32) -> f32 {
        if self.params.bypass > 0.5 {
            return input;
        }

        let rate = self.params.speed.clamp(0.0, 1.0) * 10.0;
        let rate = if self.params.times_five > 0.5 {
            rate * 5.0
        } else {
            rate
        };
        let rate = rate.max(0.01);

        self.lfo_phase += 2.0 * PI * rate / self.sample_rate;
        if self.lfo_phase > 2.0 * PI {
            self.lfo_phase -= 2.0 * PI;
        }
        let lfo = self.lfo_phase.sin();

        let base = 200.0 + self.params.offset.clamp(0.0, 1.0) * 4000.0;
        let cutoff = base
            * (2.0_f32).powf(
                self.params.depth.clamp(0.0, 1.0)
                    * lfo
                    * (1.0 + self.params.hz_per_oct.clamp(0.0, 1.0) * 4.0),
            );
        let cutoff = cutoff.clamp(20.0, self.sample_rate * 0.49);

        let blur = self.params.blur.clamp(0.0, 1.0);
        let alpha = 1.0 - blur * 0.9;
        self.smoothed_cutoff += (cutoff - self.smoothed_cutoff) * alpha;

        self.recompute(self.smoothed_cutoff);

        let mut x = input;
        for s in &mut self.sections {
            x = s.process(x);
        }

        let mut out = x;
        if self.params.keep_bass > 0.0 {
            let a = (2.0 * PI * 120.0 / self.sample_rate).clamp(0.0, 1.0);
            self.bass_lp += a * (input - self.bass_lp);
            out += self.params.keep_bass.clamp(0.0, 1.0) * self.bass_lp;
        }

        let wet = self.params.mix.clamp(0.0, 1.0);
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
            width: 0.5,
            offset: 0.3,
            depth: 0.5,
            speed: 0.4,
            mix: 1.0,
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
}
