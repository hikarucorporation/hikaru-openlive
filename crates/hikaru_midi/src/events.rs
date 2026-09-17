#[derive(Debug, Clone, PartialEq)]
pub enum MidiMessage {
    NoteOn { channel: u8, pitch: u8, velocity: u8 },
    NoteOff { channel: u8, pitch: u8, velocity: u8 },
    ControlChange { channel: u8, controller: u8, value: u8 },
    PitchBend { channel: u8, value: u16 },
    SysEx(Vec<u8>),
}
