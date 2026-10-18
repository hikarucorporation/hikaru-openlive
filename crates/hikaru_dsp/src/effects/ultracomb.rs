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

        // Escribir en el buffer con feedback
        self.buffer[self.write_ptr] = input + out * self.feedback;
        self.write_ptr = (self.write_ptr + 1) % MAX_DELAY;

        out
    }

    pub fn set_feedback(&mut self, fb: f32) { self.feedback = fb.clamp(-0.99, 0.99); }
    pub fn set_damp(&mut self, d: f32) { self.damp = d.clamp(0.0, 0.9); }
}