// crates/hikaru_dsp/src/effects/phaser.rs

#[derive(Debug, Clone, Copy)]
pub struct AllPassStage {
    z1: f32,
}

impl AllPassStage {
    pub const fn new() -> Self { 
        Self { z1: 0.0 } 
    }
    
    #[inline]
    pub fn process(&mut self, input: f32, a: f32) -> f32 {
        let out = a * input + self.z1;
        self.z1 = input - a * out;
        out
    }
}

pub struct Phaser {
    stages: [AllPassStage; 6],
    lfo_phase: f32,
    feedback: f32,
    last_out: f32,
}

impl Phaser {
    pub fn new() -> Self {
        Self {
            stages: [AllPassStage::new(); 6], 
            lfo_phase: 0.0,
            feedback: 0.4,
            last_out: 0.0,
        }
    }

    pub fn process(&mut self, input: f32, rate: f32, depth: f32, sample_rate: f32) -> f32 {
        // LFO simple
        self.lfo_phase += (2.0 * std::f32::consts::PI * rate) / sample_rate;
        if self.lfo_phase > 2.0 * std::f32::consts::PI { 
            self.lfo_phase -= 2.0 * std::f32::consts::PI; 
        }
        
        let lfo_val = (self.lfo_phase.sin() + 1.0) * 0.5;
        let a = 0.2 + lfo_val * depth * 0.6;

        let mut out = input + self.last_out * self.feedback;
        
        for stage in &mut self.stages {
            out = stage.process(out, a);
        }
        
        self.last_out = out;
        out
    }
}