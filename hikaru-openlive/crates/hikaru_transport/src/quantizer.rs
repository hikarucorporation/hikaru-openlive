#[cfg(test)]
mod tests {
    use super::*;
    use hikaru_transport::TransportPosition;
    use hikaru_core::SampleRate;

    #[test]
    fn test_quantization_math() {
        let sr = SampleRate::new(44100);
        // Posición: 120 BPM, justo a la mitad del primer beat (0.5 beats transcurridos)
        let mut pos = TransportPosition::new(sr, 120.0);
        
        // 120 BPM = 2 beats por segundo. 
        // 44100 / 2 = 22050 samples por beat.
        // Si estamos a la mitad, llevamos 11025 samples.
        pos.sample_count = 11025; 

        let wait = QuantizationEngine::samples_until_next_sync(
            &pos, 
            QuantizationGrid::Quarter, 
            44100.0
        );

        // Debería pedirnos esperar exactamente la otra mitad (11025 samples)
        assert_eq!(wait, 11025);
        println!("¡La matemática de cuantización es perfecta!");
    }
}