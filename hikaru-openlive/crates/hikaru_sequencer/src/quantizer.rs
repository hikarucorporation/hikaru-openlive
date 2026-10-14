// crates/hikaru_sequencer/src/quantizer.rs

use hikaru_transport::TransportPosition;

/// Tipos de cuantización para el disparo de clips.
#[derive(Debug, Clone, Copy)]
pub enum QuantizationGrid {
    Bar,        // 1 Compás
    HalfNote,   // 1/2
    Quarter,    // 1/4 (1 Beat)
    Eighth,     // 1/8
    Sixteenth,  // 1/16
    None,       // Disparo instantáneo (0 latencia de rejilla)
}

pub struct QuantizationEngine;

impl QuantizationEngine {
    /// Calcula cuántos samples faltan para el próximo "golpe" de la rejilla.
    pub fn samples_until_next_sync(
        current_pos: &TransportPosition,
        grid: QuantizationGrid,
        sample_rate: f64,
    ) -> u64 {
        let samples_per_beat = (60.0 / current_pos.bpm) * sample_rate;
        
        let grid_size_in_beats = match grid {
            QuantizationGrid::Bar => 4.0, // Asumiendo 4/4 por ahora
            QuantizationGrid::HalfNote => 2.0,
            QuantizationGrid::Quarter => 1.0,
            QuantizationGrid::Eighth => 0.5,
            QuantizationGrid::Sixteenth => 0.25,
            QuantizationGrid::None => return 0,
        };

        let samples_per_grid_unit = samples_per_beat * grid_size_in_beats;
        let current_sample = current_pos.sample_count as f64;

        // El modulo nos dice cuánto avanzamos en la unidad de rejilla actual
        let remainder = current_sample % samples_per_grid_unit;
        
        if remainder < 0.001 {
            0 // Ya estamos justo en el beat
        } else {
            (samples_per_grid_unit - remainder) as u64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hikaru_transport::TransportPosition;
    use hikaru_core::SampleRate;

    #[test]
    fn test_quantization_math() {
        // Agregamos el .0 para que Rust sea feliz
        let sr = SampleRate::new(44100.0);
        
        // 120 BPM, posición inicial
        let mut pos = TransportPosition::new(sr, 120.0);
        
        // 120 BPM = 2 beats por segundo. 
        // 44100 / 2 = 22050 samples por beat.
        // Si estamos a la mitad del beat, llevamos 11025 samples.
        pos.sample_count = 11025; 

        let wait = QuantizationEngine::samples_until_next_sync(
            &pos, 
            QuantizationGrid::Quarter, 
            44100.0 // Acá también el .0
        );

        // Debería pedirnos esperar exactamente la otra mitad (11025 samples)
        assert_eq!(wait, 11025);
    }
}