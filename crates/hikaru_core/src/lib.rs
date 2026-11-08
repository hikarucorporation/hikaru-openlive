// crates/hikaru_core/src/lib.rs

pub mod audio_buffer;
pub mod audio_clip_event;
pub mod midi_note;

pub use audio_buffer::AudioBuffer;
pub use audio_clip_event::{AudioClipEvent, StretchMode};
pub use midi_note::MidiNoteInstance;

/// Alias para el tipo de dato de audio básico (f32 por estándar)
pub type AudioSample = f32;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SampleRate(f32);

impl SampleRate {
    pub fn new(value: f32) -> Self { Self(value) }
    pub fn get(&self) -> f32 { self.0 }
}
