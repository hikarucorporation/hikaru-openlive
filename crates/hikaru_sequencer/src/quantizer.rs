// crates/hikaru_sequencer/src/quantizer.rs

use hikaru_transport::TransportPosition;

/// Tipos de cuantización para el disparo de clips y escenas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum QuantizationGrid {
    Bar,        // 1 Compás
    HalfNote,   // 1/2
    #[default]
    Quarter,    // 1/4 (1 Beat)
    Eighth,     // 1/8
    Sixteenth,  // 1/16
    None,       // Disparo instantáneo (0 latencia de rejilla)
}

impl QuantizationGrid {
    pub const ALL: [QuantizationGrid; 6] = [
        QuantizationGrid::None,
        QuantizationGrid::Sixteenth,
        QuantizationGrid::Eighth,
        QuantizationGrid::Quarter,
        QuantizationGrid::HalfNote,
        QuantizationGrid::Bar,
    ];

    pub fn label(self) -> &'static str {
        match self {
            QuantizationGrid::None => "Off",
            QuantizationGrid::Sixteenth => "1/16",
            QuantizationGrid::Eighth => "1/8",
            QuantizationGrid::Quarter => "1/4",
            QuantizationGrid::HalfNote => "1/2",
            QuantizationGrid::Bar => "1 Bar",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            QuantizationGrid::None => "Off",
            QuantizationGrid::Sixteenth => "1/16",
            QuantizationGrid::Eighth => "1/8",
            QuantizationGrid::Quarter => "1/4",
            QuantizationGrid::HalfNote => "1/2",
            QuantizationGrid::Bar => "1B",
        }
    }

    pub fn to_notation(self) -> &'static str {
        match self {
            QuantizationGrid::None => "none",
            QuantizationGrid::Sixteenth => "1/16",
            QuantizationGrid::Eighth => "1/8",
            QuantizationGrid::Quarter => "1/4",
            QuantizationGrid::HalfNote => "1/2",
            QuantizationGrid::Bar => "1/1",
        }
    }

    pub fn from_notation(raw: &str) -> Option<Self> {
        let s = raw.trim().to_ascii_lowercase();
        match s.as_str() {
            "none" | "off" | "0" => Some(QuantizationGrid::None),
            "1/1" | "1" | "bar" | "1 bar" | "1b" => Some(QuantizationGrid::Bar),
            "1/2" | "2" => Some(QuantizationGrid::HalfNote),
            "1/4" | "4" => Some(QuantizationGrid::Quarter),
            "1/8" | "8" => Some(QuantizationGrid::Eighth),
            "1/16" | "16" => Some(QuantizationGrid::Sixteenth),
            _ => None,
        }
    }

    pub fn beats(self, beats_per_bar: u32) -> f64 {
        match self {
            QuantizationGrid::Bar => beats_per_bar.max(1) as f64,
            QuantizationGrid::HalfNote => 2.0,
            QuantizationGrid::Quarter => 1.0,
            QuantizationGrid::Eighth => 0.5,
            QuantizationGrid::Sixteenth => 0.25,
            QuantizationGrid::None => 0.0,
        }
    }
}

pub struct QuantizationEngine;

impl QuantizationEngine {
    /// Resuelve la rejilla efectiva de una escena: si la escena tiene un
    /// override explícito, prevalece sobre la cuantización global.
    #[inline]
    pub fn resolve_scene_grid(
        global: QuantizationGrid,
        scene_override: Option<QuantizationGrid>,
    ) -> QuantizationGrid {
        scene_override.unwrap_or(global)
    }

    /// Calcula cuántos samples faltan para el próximo "golpe" de la rejilla.
    pub fn samples_until_next_sync(
        current_pos: &TransportPosition,
        grid: QuantizationGrid,
        sample_rate: f64,
    ) -> u64 {
        if grid == QuantizationGrid::None || current_pos.bpm <= 0.0 || sample_rate <= 0.0 {
            return 0;
        }
        let samples_per_beat = (60.0 / current_pos.bpm) * sample_rate;
        let grid_size_in_beats = grid.beats(current_pos.beats_per_bar);
        let samples_per_grid_unit = samples_per_beat * grid_size_in_beats;
        if samples_per_grid_unit <= 0.0 {
            return 0;
        }
        let current_sample = current_pos.sample_count as f64;

        // El módulo nos dice cuánto avanzamos en la unidad de rejilla actual
        let remainder = current_sample % samples_per_grid_unit;

        if remainder < 0.001 {
            0 // Ya estamos justo en el beat
        } else {
            (samples_per_grid_unit - remainder).round() as u64
        }
    }

    /// Posición absoluta en samples del próximo límite de cuantización.
    #[inline]
    pub fn next_sync_sample(
        current_pos: &TransportPosition,
        grid: QuantizationGrid,
        sample_rate: f64,
    ) -> u64 {
        current_pos
            .sample_count
            .saturating_add(Self::samples_until_next_sync(current_pos, grid, sample_rate))
    }

    /// Samples hasta el lanzamiento de una escena aplicando su override si existe.
    #[inline]
    pub fn samples_until_scene_sync(
        current_pos: &TransportPosition,
        global: QuantizationGrid,
        scene_override: Option<QuantizationGrid>,
        sample_rate: f64,
    ) -> u64 {
        let effective = Self::resolve_scene_grid(global, scene_override);
        Self::samples_until_next_sync(current_pos, effective, sample_rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hikaru_transport::TransportPosition;
    use hikaru_core::SampleRate;

    #[test]
    fn test_quantization_math() {
        let sr = SampleRate::new(44100.0);
        let mut pos = TransportPosition::new(sr, 120.0);

        // 120 BPM = 2 beats por segundo -> 22050 samples por beat.
        // A mitad del beat (11025 samples), faltan exactamente 11025 samples.
        pos.sample_count = 11025;

        let wait = QuantizationEngine::samples_until_next_sync(
            &pos,
            QuantizationGrid::Quarter,
            44100.0,
        );

        assert_eq!(wait, 11025);
    }

    #[test]
    fn scene_override_takes_precedence_over_global() {
        let sr = SampleRate::new(44100.0);
        let mut pos = TransportPosition::new(sr, 120.0);
        // 120 BPM: 1 beat = 22050 samples, 1 bar (4/4) = 88200 samples.
        pos.sample_count = 11025; // 0.5 beat

        // Sin override: usa global Bar -> faltan 88200 - 11025 = 77175
        assert_eq!(
            QuantizationEngine::samples_until_scene_sync(&pos, QuantizationGrid::Bar, None, 44100.0),
            77175
        );
        // Override a None (instantáneo): 0 samples
        assert_eq!(
            QuantizationEngine::samples_until_scene_sync(
                &pos,
                QuantizationGrid::Bar,
                Some(QuantizationGrid::None),
                44100.0
            ),
            0
        );
        // Override a Eighth (0.5 beat = 11025 samples): ya está alineado -> 0
        assert_eq!(
            QuantizationEngine::samples_until_scene_sync(
                &pos,
                QuantizationGrid::Bar,
                Some(QuantizationGrid::Eighth),
                44100.0
            ),
            0
        );
        // Compás 3/4: 1 bar = 66150 samples -> faltan 66150 - 11025 = 55125
        pos.beats_per_bar = 3;
        assert_eq!(
            QuantizationEngine::samples_until_scene_sync(&pos, QuantizationGrid::Bar, None, 44100.0),
            55125
        );
    }

    #[test]
    fn quantization_notation_roundtrip() {
        for grid in QuantizationGrid::ALL {
            let notation = grid.to_notation();
            assert_eq!(QuantizationGrid::from_notation(notation), Some(grid));
        }
        assert_eq!(QuantizationGrid::from_notation("Off"), Some(QuantizationGrid::None));
        assert_eq!(QuantizationGrid::from_notation("1 Bar"), Some(QuantizationGrid::Bar));
        assert_eq!(QuantizationGrid::from_notation("invalid"), None);
    }
}