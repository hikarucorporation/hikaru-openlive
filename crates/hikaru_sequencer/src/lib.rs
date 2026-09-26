// crates/hikaru_sequencer/src/lib.rs

pub mod clip;
pub mod quantizer;
pub mod matrix;

// Re-exportamos para que sea fácil de usar
pub use clip::{Clip, ClipAudioEvent, ClipAudioTimeline, ClipState, TriggerMode, VoiceState, VoiceStatus};
pub use quantizer::{QuantizationEngine, QuantizationGrid};
pub use matrix::TrackMatrix;