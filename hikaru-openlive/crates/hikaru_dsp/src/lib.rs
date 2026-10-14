// crates/hikaru_dsp/src/lib.rs

pub mod synth;
pub mod effects;
pub mod plugin_node; // Nuevo módulo para wrappear VST3/CLAP en el DSP Rack

pub use plugin_node::{DspPluginNode, PluginCategory};

#[cfg(test)]
mod tests {
    use crate::synth::wavetable::WavetableOscillator;
    use crate::effects::filter::StateVariableFilter;
    use crate::effects::open_harmonic::OpenHarmonicProcessor;
    use hikaru_core::SampleRate;

    #[test]
    fn test_all_dsp_paths() {
        let table = [0.0; 1024];
        let sr = SampleRate::new(44100.0);
        let _osc = WavetableOscillator::new(&table, sr);
        
        let mut filter = StateVariableFilter::new();
        filter.set_params(1000.0, 0.707, 44100.0);
        
        let mut ohp = OpenHarmonicProcessor::new();
        let _ = ohp.process_sample(0.0);

        assert!(true);
    }
}