use midir::{MidiInput, MidiInputConnection};
use crate::events::MidiMessage;

pub struct HardwareController {
    _connection: MidiInputConnection<()>,
}

impl HardwareController {
    pub fn connect_device<F>(port_name: &str, mut _callback: F) -> Result<Self, String>
    where
        F: FnMut(u64, MidiMessage) + Send + 'static,
    {
        let midi_in = MidiInput::new("Hikaru MIDI Input").map_err(|e| e.to_string())?;
        let ports = midi_in.ports();
        
        let port = ports.iter().find(|p| {
            midi_in.port_name(p).unwrap_or_default().contains(port_name)
        }).ok_or("Dispositivo no encontrado")?;

        let connection = midi_in.connect(port, "hikaru-input-read", move |_timestamp, _data, _| {
            // Callback parsing logic
        }, ()).map_err(|e| e.to_string())?;

        Ok(Self { _connection: connection })
    }
}
