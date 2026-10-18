// crates/hikaru_dsp/src/effects/flanger.rs

pub struct Flanger {
    buffer: [f32; 1024], // Delay corto para flanging
    write_ptr: usize,
    lfo_phase: f32,
}

impl Flanger {
    pub fn new() -> Self {
        Self {
            buffer: [0.0; 1024],
            write_ptr: 0,
            lfo_phase: 0.0,
        }
    }

    #[inline]
    pub fn process(&mut self, input: f32, rate: f32, depth_ms: f32, feedback: f32, sample_rate: f32) -> f32 {
        self.lfo_phase += (2.0 * std::f32::consts::PI * rate) / sample_rate;
        let lfo_val = (self.lfo_phase.sin() + 1.0) * 0.5;
        
        // Delay de 1ms a 10ms aprox
        let delay_samples = (1.0 + lfo_val * depth_ms) * (sample_rate / 1000.0);
        
        let size = 1024.0;
        let read_ptr = (self.write_ptr as f32 - delay_samples + size) % size;
        let idx = read_ptr as usize;
        let delayed = self.buffer[idx]; // (Sin interpolación para ahorrar CPU, o podés reusar la del Comb)

        self.buffer[self.write_ptr] = input + delayed * feedback;
        self.write_ptr = (self.write_ptr + 1) % 1024;

        input + delayed // Mezcla Dry/Wet 50%
    }
}