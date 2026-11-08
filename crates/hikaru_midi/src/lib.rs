pub mod events;
pub mod file;
pub mod device;
pub mod controllers;

pub use events::*;
pub use file::parse_midi_file_to_notes;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiEvent {
    pub status: u8,
    pub data1: u8,
    pub data2: u8,
}