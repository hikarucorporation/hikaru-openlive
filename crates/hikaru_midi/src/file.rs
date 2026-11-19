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

/// Serializa una o más pistas de notas `(start_tick, pitch, velocity, duration_ticks)`
/// normalizadas a 960 PPQN en un archivo Standard MIDI File (SMF Formato 1).
pub fn write_midi_tracks_to_bytes(
    tracks: &[(String, Vec<(u64, u8, u8, u32)>)],
    bpm: f64,
    beats_per_bar: u32,
    beat_division: u32,
) -> Result<Vec<u8>, String> {
    use midly::num::{u15, u24, u28, u4, u7};
    use midly::{Format, Header, MetaMessage, Timing, TrackEvent};

    let mut smf = Smf::new(Header::new(
        Format::Parallel,
        Timing::Metrical(u15::new(960)),
    ));

    // Pista 0: tempo y compás
    let micros_per_beat = ((60_000_000.0 / bpm.max(1.0)).round() as u32).clamp(1, 0x00FF_FFFF);
    let denom_pow = match beat_division {
        1 => 0u8,
        2 => 1,
        4 => 2,
        8 => 3,
        16 => 4,
        32 => 5,
        _ => 2,
    };
    let tempo_track = vec![
        TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::new(micros_per_beat))),
        },
        TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::TimeSignature(
                beats_per_bar.clamp(1, 32) as u8,
                denom_pow,
                24,
                8,
            )),
        },
        TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
        },
    ];
    smf.tracks.push(tempo_track);

    // Pistas de notas
    let leaked_names: Vec<Vec<u8>> = tracks.iter().map(|(n, _)| n.as_bytes().to_vec()).collect();
    for (idx, (_, notes)) in tracks.iter().enumerate() {
        let ch = u4::new((idx % 16) as u8);
        let mut raw_events: Vec<(u64, u8, u8, u8)> = Vec::with_capacity(notes.len() * 2);
        for &(start, pitch, vel, dur) in notes {
            let p = pitch.min(127);
            let v = vel.clamp(1, 127);
            let end = start.saturating_add(dur.max(1) as u64);
            // order: NoteOff (0) before NoteOn (1) at same tick
            raw_events.push((start, 1, p, v));
            raw_events.push((end, 0, p, 0));
        }
        raw_events.sort_by_key(|&(tick, order, pitch, _)| (tick, order, pitch));

        let mut track_events: Vec<TrackEvent<'_>> = Vec::with_capacity(raw_events.len() + 2);
        track_events.push(TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::TrackName(&leaked_names[idx])),
        });

        let mut prev_tick = 0u64;
        for (tick, is_on, pitch, vel) in raw_events {
            let delta = tick.saturating_sub(prev_tick).min(0x0FFF_FFFF) as u32;
            prev_tick = tick;
            let message = if is_on == 1 {
                MidlyMsg::NoteOn {
                    key: u7::new(pitch),
                    vel: u7::new(vel),
                }
            } else {
                MidlyMsg::NoteOff {
                    key: u7::new(pitch),
                    vel: u7::new(0),
                }
            };
            track_events.push(TrackEvent {
                delta: u28::new(delta),
                kind: TrackEventKind::Midi {
                    channel: ch,
                    message,
                },
            });
        }

        track_events.push(TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
        });
        smf.tracks.push(track_events);
    }

    let mut out = Vec::new();
    smf.write_std(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midi_export_round_trip() {
        let tracks = vec![(
            "Lead".to_string(),
            vec![(0u64, 60u8, 100u8, 480u32), (480, 64, 90, 480)],
        )];
        let bytes = write_midi_tracks_to_bytes(&tracks, 128.0, 4, 4).expect("write midi");
        let parsed = parse_midi_file_to_notes(&bytes).expect("parse midi");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].pitch, 60);
        assert_eq!(parsed[0].duration_ticks, 480);
        assert_eq!(parsed[1].pitch, 64);
        assert_eq!(parsed[1].start_tick, 480);
    }
}
