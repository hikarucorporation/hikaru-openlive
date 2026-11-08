// crates/hikaru_core/src/midi_note.rs

/// Instancia canónica de nota MIDI para el motor de audio y el secuenciador.
///
/// Ocupa exactamente 16 bytes:
/// - `start_tick`: u64 (8 bytes)
/// - `pitch`: u8 (1 byte)
/// - `velocity`: u8 (1 byte)
/// - padding: 2 bytes
/// - `duration_ticks`: u32 (4 bytes)
///
/// La ordenación derivada sitúa `start_tick` como primer criterio de comparación,
/// permitiendo ordenar secuencias cronológicamente con `Vec::sort()` o `sort_unstable()`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct MidiNoteInstance {
    pub start_tick: u64,
    pub pitch: u8,
    pub velocity: u8,
    pub duration_ticks: u32,
}

impl MidiNoteInstance {
    #[inline]
    pub const fn new(start_tick: u64, pitch: u8, velocity: u8, duration_ticks: u32) -> Self {
        Self {
            start_tick,
            pitch,
            velocity,
            duration_ticks,
        }
    }

    #[inline]
    pub fn end_tick(&self) -> u64 {
        self.start_tick.saturating_add(self.duration_ticks as u64)
    }

    #[inline]
    pub fn is_active_at(&self, tick: u64) -> bool {
        tick >= self.start_tick && tick < self.end_tick()
    }
}

impl From<(u64, u8, u8, u32)> for MidiNoteInstance {
    #[inline]
    fn from((start_tick, pitch, velocity, duration_ticks): (u64, u8, u8, u32)) -> Self {
        Self {
            start_tick,
            pitch,
            velocity,
            duration_ticks,
        }
    }
}

impl From<MidiNoteInstance> for (u64, u8, u8, u32) {
    #[inline]
    fn from(n: MidiNoteInstance) -> Self {
        (n.start_tick, n.pitch, n.velocity, n.duration_ticks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_midi_note_instance_size_and_alignment() {
        assert_eq!(std::mem::size_of::<MidiNoteInstance>(), 16);
        assert_eq!(std::mem::align_of::<MidiNoteInstance>(), 8);
    }

    #[test]
    fn test_midi_note_sorting_by_start_tick() {
        let mut notes = vec![
            MidiNoteInstance::new(1920, 60, 100, 480),
            MidiNoteInstance::new(0, 64, 90, 240),
            MidiNoteInstance::new(960, 67, 110, 480),
        ];
        notes.sort();
        assert_eq!(notes[0].start_tick, 0);
        assert_eq!(notes[1].start_tick, 960);
        assert_eq!(notes[2].start_tick, 1920);
    }

    #[test]
    fn test_tuple_conversion() {
        let tuple = (480u64, 60u8, 100u8, 240u32);
        let note = MidiNoteInstance::from(tuple);
        assert_eq!(note.start_tick, 480);
        assert_eq!(note.pitch, 60);
        assert_eq!(note.velocity, 100);
        assert_eq!(note.duration_ticks, 240);
        assert_eq!(note.end_tick(), 720);
        assert!(note.is_active_at(480));
        assert!(note.is_active_at(719));
        assert!(!note.is_active_at(720));

        let back: (u64, u8, u8, u32) = note.into();
        assert_eq!(back, tuple);
    }
}
