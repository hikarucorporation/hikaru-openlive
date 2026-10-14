// crates/hikaru_dsp/src/synth/wavetable.rs
use hikaru_core::SampleRate;

pub struct WavetableOscillator<'a> {
    table: &'a [f32],
    phase: f32,
    phase_increment: f32,
    sample_rate: f32,
}

impl<'a> WavetableOscillator<'a> {
    pub fn new(table: &'a [f32], sample_rate: SampleRate) -> Self {
        Self {
            table,
            phase: 0.0,
            phase_increment: 0.0,
            sample_rate: sample_rate.get(),
        }
    }

    pub fn set_frequency(&mut self, frequency: f32) {
        let table_size = self.table.len() as f32;
        self.phase_increment = (frequency * table_size) / self.sample_rate;
    }

    #[inline]
    pub fn next_sample(&mut self) -> f32 {
        let table_size = self.table.len();
        let index0 = self.phase as usize;
        let index1 = (index0 + 1) % table_size;
        let frac = self.phase - index0 as f32;

        let y0 = self.table[index0];
        let y1 = self.table[index1];
        let sample = y0 + frac * (y1 - y0);

        self.phase = (self.phase + self.phase_increment) % (table_size as f32);
        sample
    }
}