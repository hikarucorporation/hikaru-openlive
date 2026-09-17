use midly::Smf;
use crate::events::MidiMessage;

pub struct MidiSequence {
    pub ticks_per_beat: u16,
    pub messages: Vec<(u64, MidiMessage)>,
}

impl MidiSequence {
    pub fn from_file_bytes(bytes: &[u8]) -> Result<Self, String> {
        let _smf = Smf::parse(bytes).map_err(|e| e.to_string())?;
        Ok(Self {
            ticks_per_beat: 960,
            messages: vec![],
        })
    }
}
