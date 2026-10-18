// crates/hikaru_dsp/src/effects/filter.rs
use std::f32::consts::PI;

pub struct StateVariableFilter {
    ic1eq: f32,
    ic2eq: f32,
    g: f32,
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
}

impl StateVariableFilter {
    pub fn new() -> Self {
        Self {
            ic1eq: 0.0, ic2eq: 0.0,
            g: 0.0, k: 0.0, a1: 0.0, a2: 0.0, a3: 0.0,
        }
    }

    pub fn set_params(&mut self, cutoff: f32, resonance: f32, sample_rate: f32) {
        let q = 1.0 / resonance.max(0.01);
        let g = (PI * cutoff / sample_rate).tan();
        let k = q;
        
        self.g = g;
        self.k = k;
        self.a1 = 1.0 / (1.0 + g * (g + k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    #[inline]
    pub fn process(&mut self, v0: f32) -> (f32, f32, f32) {
        let v3 = v0 - self.ic2eq;
        let v1 = self.a1 * self.ic1eq + self.a2 * v3;
        let v2 = self.ic2eq + self.a2 * self.ic1eq + self.a3 * v3;

        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;

        (v2, v0 - self.k * v1 - v2, v1) // LP, HP, BP
    }
}