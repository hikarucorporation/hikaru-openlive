// crates/hikaru_dsp/src/effects/open_harmonic.rs

use std::sync::Arc;
use realfft::{RealFftPlanner, RealToComplex, ComplexToReal};
use rustfft::num_complex::Complex;
use std::f32::consts::PI;

const N: usize = 2048;
const HOP: usize = 512;

pub struct OpenHarmonicProcessor {
    r2c: Arc<dyn RealToComplex<f32>>,
    c2r: Arc<dyn ComplexToReal<f32>>,
    
    input_buffer: [f32; N],
    output_buffer: [f32; N * 2],
    window: [f32; N],
    
    // Buffers para evitar alocaciones en runtime
    complex_scratch: Vec<Complex<f32>>, 
    last_phase: [f32; N / 2 + 1],
    output_phase: [f32; N / 2 + 1],
    
    pub wet_dry: f32,
    pub pitch_shift: f32,
    write_ptr: usize,
    read_ptr: usize,
}

impl OpenHarmonicProcessor {
    pub fn new() -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let r2c = planner.plan_fft_forward(N); // <-- NOMBRE CORRECTO
        let c2r = planner.plan_fft_inverse(N); // <-- NOMBRE CORRECTO

        let mut window = [0.0; N];
        for i in 0..N {
            window[i] = 0.5 * (1.0 - (2.0 * PI * i as f32 / N as f32).cos());
        }

        Self {
            r2c,
            c2r,
            input_buffer: [0.0; N],
            output_buffer: [0.0; N * 2],
            window,
            complex_scratch: vec![Complex::new(0.0, 0.0); N / 2 + 1],
            last_phase: [0.0; N / 2 + 1],
            output_phase: [0.0; N / 2 + 1],
            wet_dry: 0.5,
            pitch_shift: 0.0,
            write_ptr: 0,
            read_ptr: 0,
        }
    }

    pub fn process_sample(&mut self, input: f32) -> f32 {
        self.input_buffer[self.write_ptr] = input;
        
        let out_sample = self.output_buffer[self.read_ptr];
        self.output_buffer[self.read_ptr] = 0.0;
        
        self.write_ptr += 1;
        self.read_ptr = (self.read_ptr + 1) % (N * 2);

        if self.write_ptr >= N {
            self.process_fft_frame();
            self.input_buffer.copy_within(HOP..N, 0);
            self.write_ptr = N - HOP;
        }

        (input * (1.0 - self.wet_dry)) + (out_sample * self.wet_dry)
    }

    fn process_fft_frame(&mut self) {
        let mut fft_in = self.r2c.make_input_vec();
        let mut fft_out = self.r2c.make_output_vec();

        for i in 0..N {
            fft_in[i] = self.input_buffer[i] * self.window[i];
        }

        self.r2c.process(&mut fft_in, &mut fft_out).unwrap();

        let shift_factor = 2.0f32.powf(self.pitch_shift / 12.0);
        
        // Limpiamos el scratch buffer pre-alocado (CERO ALLOCATIONS!)
        for bin in &mut self.complex_scratch { *bin = Complex::new(0.0, 0.0); }

        for i in 0..N / 2 + 1 {
            let mag = fft_out[i].norm();
            let phase = fft_out[i].arg();

            let mut delta_phase = phase - self.last_phase[i];
            self.last_phase[i] = phase;

            let expected_delta = 2.0 * PI * (i as f32) * (HOP as f32) / (N as f32);
            delta_phase -= expected_delta;

            while delta_phase > PI { delta_phase -= 2.0 * PI; }
            while delta_phase < -PI { delta_phase += 2.0 * PI; }

            let new_bin_idx = (i as f32 * shift_factor) as usize;
            if new_bin_idx < (N / 2 + 1) {
                let phase_inc = expected_delta + delta_phase * shift_factor;
                self.output_phase[new_bin_idx] += phase_inc;
                
                self.complex_scratch[new_bin_idx] += Complex::from_polar(mag, self.output_phase[new_bin_idx]);
            }
        }

        let mut ifft_out = self.c2r.make_output_vec();
        self.c2r.process(&mut self.complex_scratch, &mut ifft_out).unwrap();

        for i in 0..N {
            let out_idx = (self.read_ptr + i) % (N * 2);
            self.output_buffer[out_idx] += (ifft_out[i] * self.window[i]) / (N as f32 * 0.5);
        }
    }
}