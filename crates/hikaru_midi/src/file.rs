use std::collections::HashMap;
use midly::{MidiMessage as MidlyMsg, Smf, TrackEventKind};
use hikaru_core::MidiNoteInstance;
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

/// Parsea un archivo MIDI (.mid / .midi) y extrae todas las notas normalizadas a 960 PPQN.
pub fn parse_midi_file_to_notes(bytes: &[u8]) -> Result<Vec<MidiNoteInstance>, String> {
    let smf = Smf::parse(bytes).map_err(|e| e.to_string())?;
    let tpb: f64 = match smf.header.timing {
        midly::Timing::Metrical(t) => t.as_int() as f64,
        midly::Timing::Timecode(fps, sub) => (fps.as_f32() * sub as f32) as f64,
    };
    let scale = 960.0 / tpb.max(1.0);

    let mut notes = Vec::new();

    for track in smf.tracks {
        let mut current_tick_raw: u64 = 0;
        let mut active_notes: HashMap<(u8, u8), (u64, u8)> = HashMap::new();

        for event in track {
            current_tick_raw += event.delta.as_int() as u64;
            let current_tick_960 = (current_tick_raw as f64 * scale).round() as u64;

            if let TrackEventKind::Midi { channel, message } = event.kind {
                let ch = channel.as_int();
                match message {
                    MidlyMsg::NoteOn { key, vel } => {
                        let pitch = key.as_int();
                        let velocity = vel.as_int();
                        if velocity > 0 {
                            if let Some((start_tick, start_vel)) = active_notes.remove(&(ch, pitch)) {
                                let dur = (current_tick_960.saturating_sub(start_tick)).max(1) as u32;
                                notes.push(MidiNoteInstance::new(start_tick, pitch, start_vel, dur));
                            }
                            active_notes.insert((ch, pitch), (current_tick_960, velocity));
                        } else if let Some((start_tick, start_vel)) = active_notes.remove(&(ch, pitch)) {
                            let dur = (current_tick_960.saturating_sub(start_tick)).max(1) as u32;
                            notes.push(MidiNoteInstance::new(start_tick, pitch, start_vel, dur));
                        }
                    }
                    MidlyMsg::NoteOff { key, vel: _ } => {
                        let pitch = key.as_int();
                        if let Some((start_tick, start_vel)) = active_notes.remove(&(ch, pitch)) {
                            let dur = (current_tick_960.saturating_sub(start_tick)).max(1) as u32;
                            notes.push(MidiNoteInstance::new(start_tick, pitch, start_vel, dur));
                        }
                    }
                    _ => {}
                }
            }
        }

        let end_tick = (current_tick_raw as f64 * scale).round() as u64;
        for ((_, pitch), (start_tick, start_vel)) in active_notes {
            let dur = (end_tick.saturating_sub(start_tick)).max(480) as u32;
            notes.push(MidiNoteInstance::new(start_tick, pitch, start_vel, dur));
        }
    }

    notes.sort_by_key(|n| (n.start_tick, n.pitch));
    Ok(notes)
}
