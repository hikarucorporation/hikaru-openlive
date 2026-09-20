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
        
    pub fn set_clip_loop(
        &self,
        track_idx: usize,
        scene_idx: usize,
        start_secs: f32,
        end_secs: f32,
        enabled: bool,
    ) {
        let _ = self.cmd_sender.send(GuiCommand::SetClipLoop {
            track_idx,
            scene_idx,
            start_secs,
            end_secs,
            enabled,
        });
    }
}

pub enum AudioMessage {
    SetClipLoop {
        track_idx: usize,
        scene_idx: usize,
        start_secs: f32,
        end_secs: f32,
        enabled: bool,
    },
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
        start_secs: f32,
        end_secs: f32,
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