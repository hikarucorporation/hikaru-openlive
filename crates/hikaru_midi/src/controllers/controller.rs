use crate::events::MidiMessage;

pub struct GeneralKeyboard {
    pub octaves: u8,
}

impl GeneralKeyboard {
    pub fn new(octaves: u8) -> Self {
        Self { octaves }
    }

    pub fn is_playable(&self) -> bool {
        self.octaves >= 4
    }
}
