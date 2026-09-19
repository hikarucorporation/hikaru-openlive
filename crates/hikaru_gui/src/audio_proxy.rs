// Copyright (c) Hikaru Corporation - 2026
// Hikaru OpenLive - Audio Proxy
// GNU Affero General Public License v3
// crates/hikaru_gui/src/audio_proxy.rs

use std::sync::mpsc::Sender;

#[derive(Clone, Debug)]
pub struct MidiNoteInstance {
    pub start_tick: u64,
    pub pitch: u8,
    pub velocity: u8,
    pub duration_ticks: u32,
}

pub struct MidiClipInstance {
    pub track_index: usize,
    pub scene_index: usize,
    pub notes: Vec<MidiNoteInstance>,
    pub is_playing: bool,
    pub start_frame: u64,
    pub clip_loop_ticks: u64,
}

#[derive(Clone, Debug)] // HOTFIX CLAUDE #1
pub struct AudioClipData {
    pub clip_id: usize,
    pub path: String,
    pub start_secs: f32,
    pub duration_secs: f32,
    pub offset_secs: f32,
    pub track_index: usize,
}

#[derive(Clone)]
pub struct AudioProxy {
    pub cmd_sender: Sender<GuiCommand>,
}

impl AudioProxy {
    pub fn new(cmd_sender: Sender<GuiCommand>) -> Self {
        Self { cmd_sender }
    }

    pub fn send(&self, cmd: GuiCommand) {
        let _ = self.cmd_sender.send(cmd);
    }
}

pub enum GuiCommand {
    SetAppMode(bool), // true = OpenStudio, false = OpenLive
    Play,
    Pause,
    Stop,
    Seek { 
        sample_count: u64 
    },
    // ACTUALIZADO: Pasamos el clip_id, su slot (track/scene) y sus límites al cargar
    LoadClip { 
        clip_id: usize,
        path: String, 
        position_secs: f32, 
        duration_secs: f32,
        offset_secs: f32,
        track_index: usize,
        scene_index: usize,
    },
    UpdateClipBounds { 
        clip_id: usize, 
        track_index: usize,
        scene_index: usize,
        position_secs: f32, 
        duration_secs: f32, 
        offset_secs: f32 
    },

    LoadMidiClip {
        clip_id: usize,
        track_index: usize,
        scene_index: usize,
        notes: Vec<(u64, u8, u8, u32)>,
    },
    UpdateMidiClipNotes {
        track_idx: usize,
        scene_idx: usize,
        notes: Vec<(u64, u8, u8, u32)>,
    },

    SyncPlaylistClips { 
        clips: Vec<AudioClipData> 
    },
    ToggleRecord,
    SetBpm(f32),
    TriggerScene { 
        scene_idx: usize 
    },
    TriggerClip {
        track_idx: usize,
        scene_idx: usize
    },
    StopTrack {
        track_idx: usize,
    },
    /// Puntos de loop del clip individual (Session Matrix / OPENLIVE),
    /// independientes del transporte global.
    SetClipLoop {
        track_idx: usize,
        scene_idx: usize,
        loop_start_secs: f32,
        loop_end_secs: f32,
        enabled: bool,
    },
    /// Región de loop global del transporte (en SAMPLES, convertidos en la
    /// GUI con los mismos ticks de la barra vía `ticks_to_samples`).
    /// El engine es el dueño único del wrap (en `process()`); la GUI no
    /// reescribe `sample_count` para loopear.
    SetGlobalLoop {
        start_samples: u64,
        end_samples: u64,
        enabled: bool,
    },
    AddTrack,
    AddScene,
    RemoveScene { 
        scene_idx: usize 
    },
    SetTrackPan { 
        track_idx: usize, 
        pan: f32 
    },
    SetTrackVolume { 
        track_idx: usize, 
        volume_db: f32 
    },
    SetMasterVolume { 
        volume_db: f32 
    },
    SetTrackMute { 
        track_idx: usize, 
        mute: bool 
    },
    SetTrackSolo { 
        track_idx: usize, 
        solo: bool 
    },
    RemoveTrack(usize),
    PreviewSample {
        path: String,
        volume: f32,
        speed: f32,
    },
    StopPreview,
    SetPreviewVolume(f32),
}

/*
GuiCommand::UpdateMidiClipNotes { track_idx, scene_idx, notes } => {
    let converted_notes = notes
        .into_iter()
        .map(|(start_tick, pitch, velocity, duration_ticks)| MidiNoteInstance {
            start_tick,
            pitch,
            velocity,
            duration_ticks,
        })
        .collect();

    engine.update_midi_clip(track_idx, scene_idx, converted_notes);
}
*/

/*
// Dentro del bloque de iteración de frames de AudioEngine::process
let current_tick = self.transport.samples_to_ticks(self.transport.sample_count);

for midi_clip in self.midi_clips.iter_mut().filter(|c| c.is_playing) {
    let clip_elapsed_ticks = current_tick.saturating_sub(self.transport.samples_to_ticks(midi_clip.start_frame));
    let loop_ticks = if midi_clip.clip_loop_ticks > 0 { midi_clip.clip_loop_ticks } else { 3840 }; // 1 Bar = 3840 ticks
    let local_tick = clip_elapsed_ticks % loop_ticks;

    for note in &midi_clip.notes {
        // Generar NoteOn
        if note.start_tick == local_tick {
            self.send_midi_event_to_instrument(midi_clip.track_index, MidiEvent::NoteOn {
                key: note.pitch,
                velocity: note.velocity,
            });
        }
        
        // Generar NoteOff
        let note_end_tick = note.start_tick + note.duration_ticks as u64;
        if note_end_tick == local_tick {
            self.send_midi_event_to_instrument(midi_clip.track_index, MidiEvent::NoteOff {
                key: note.pitch,
            });
        }
    }
}
*/