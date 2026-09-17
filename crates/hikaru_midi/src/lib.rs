pub mod events;
pub mod file;
pub mod device;
pub mod controllers;

pub use events::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiEvent {
    pub status: u8,
    pub data1: u8,
    pub data2: u8,
}