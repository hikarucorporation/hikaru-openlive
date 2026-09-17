use crate::events::MidiMessage;

pub struct LaunchpadGrid {
    pub active_pads: [bool; 64],
}

impl LaunchpadGrid {
    pub fn new() -> Self {
        Self { active_pads: [false; 64] }
    }

    pub fn handle_message(&mut self, msg: &MidiMessage) {
        if let MidiMessage::NoteOn { pitch, velocity, .. } = msg {
            if *velocity > 0 && *pitch < 64 {
                self.active_pads[*pitch as usize] = true;
            }
        }
    }
}
