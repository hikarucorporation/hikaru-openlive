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
    pub speed_ratio: f32,
}

pub fn compute_tempo_sync_ratio(path: &std::path::Path, project_bpm: f64) -> f32 {
    if project_bpm <= 0.0 {
        return 1.0;
    }
    let filename = path.file_name().unwrap_or_default().to_string_lossy();
    if let Some(source_bpm) = crate::views::explorer::parse_bpm_from_filename(&filename) {
        if source_bpm > 0.0 {
            return (project_bpm as f32 / source_bpm).clamp(0.25, 4.0);
        }
    }
    1.0
}

#[derive(Clone, Debug)]
pub struct EngineEventData {
    pub id: u64,
    pub samples: Vec<f32>,
    pub channels: usize,
    pub clip_start_secs: f32,
    pub sample_rate: u32,
    pub gain: f32,
    pub fade_in_secs: f32,
    pub fade_out_secs: f32,
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
    MoveMatrixClip {
        src_track: usize,
        src_scene: usize,
        dst_track: usize,
        dst_scene: usize,
    },
    ClearMatrixSlot {
        track_idx: usize,
        scene_idx: usize,
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
        speed_ratio: f32,
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
    SetMidiClipLoop {
        track_idx: usize,
        scene_idx: usize,
        loop_start_ticks: u64,
        loop_end_ticks: u64,
    },
    SetMidiClipLoopEnabled {
        track_idx: usize,
        scene_idx: usize,
        enabled: bool,
    },
    SetClipLegato {
        track_idx: usize,
        scene_idx: usize,
        legato: bool,
    },
    SetClipLaunchMode {
        track_idx: usize,
        scene_idx: usize,
        mode: hikaru_sequencer::ClipLaunchMode,
    },
    ReleaseClip {
        track_idx: usize,
        scene_idx: usize,
    },
    SetClipGain {
        track_idx: usize,
        scene_idx: usize,
        gain: f32,
    },
    SetGlobalLaunchQuantization {
        grid: hikaru_sequencer::QuantizationGrid,
    },
    SetSceneLaunchQuantization {
        scene_idx: usize,
        grid: Option<hikaru_sequencer::QuantizationGrid>,
    },

    SyncPlaylistClips { 
        clips: Vec<AudioClipData> 
    },
    ToggleRecord,
    SetBpm(f32),
    /// Lightweight live BPM update (for Tempo Automation playback) without
    /// spawning per-frame WSOLA background re-stretch threads.
    SetBpmLive(f32),
    SetClipStretchBpm {
        clip_id: usize,
        source_bpm: Option<f32>,
    },
    SetClipWarpMarkers {
        clip_id: usize,
        source_bpm: Option<f32>,
        warp_markers: Vec<(f64, f64)>,
    },
    SetClipExpressions {
        clip_id: usize,
        expressions: hikaru_core::AudioClipExpressions,
    },
    TriggerScene { 
        scene_idx: usize 
    },
    TriggerClip {
        track_idx: usize,
        scene_idx: usize
    },
    StartClip {
        track_idx: usize,
        scene_idx: usize,
    },
    /// Eventos editables del pad (cortar/pegar/split/multi-sample).
    /// El motor los mezcla por suma; el GUI es la fuente de verdad.
    SetClipEvents {
        track_idx: usize,
        scene_idx: usize,
        events: Vec<EngineEventData>,
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
    InsertScene {
        scene_idx: usize,
    },
    RemoveScene { 
        scene_idx: usize 
    },
    MoveScene {
        from_idx: usize,
        to_idx: usize,
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
    LoadDmsSample {
        pad_idx: usize,
        path: String,
    },
    DmsNoteOn {
        pad_idx: usize,
        gain: f32,
        pan: f32,
        velocity: f32,
        play_speed: f64,
        attack_ms: f32,
        decay_ms: f32,
        sustain: f32,
        release_ms: f32,
    },
    DmsNoteOff {
        pad_idx: usize,
    },
    /// Input MIDI virtual para sintetizadores (OpenWavetable / instrumentos externos)
    VirtualNoteOn {
        track_idx: usize,
        pitch: u8,
        velocity: f32,
    },
    VirtualNoteOff {
        track_idx: usize,
        pitch: u8,
    },
    SetTrackWavetable {
        track_idx: usize,
        samples: Vec<f32>,
        pitch_offset: i8,
        volume: f32,
    },
    SetTrackSamplerSample {
        track_idx: usize,
        samples: Vec<f32>,
        channels: usize,
        sample_rate: f32,
    },
    SetTrackSamplerParams {
        track_idx: usize,
        params: hikaru_dsp::synth::sampler::SamplerDspParams,
    },
    SetTrackOpenPolyParams {
        track_idx: usize,
        enabled: bool,
        params: hikaru_dsp::synth::open_poly::OpenPolyDspParams,
    },
    SetTrackOpenPolyWavetable {
        track_idx: usize,
        samples: Vec<f32>,
        frames: usize,
        frame_len: usize,
    },
    SetTrackOpenGridPatch {
        track_idx: usize,
        enabled: bool,
        patch: Option<hikaru_dsp::synth::open_grid::OpenGridPatch>,
    },
    /// Carga un plugin externo (VST3/CLAP) en un slot de pista.
    /// El motor instancia el binario y lo inserta en la cadena de efectos.
    LoadExternalPlugin {
        track_idx: usize,
        slot_idx: usize,
        descriptor: hikaru_plugin_host::PluginDescriptor,
    },
    /// Quita un plugin de un slot.
    RemoveExternalPlugin {
        track_idx: usize,
        slot_idx: usize,
    },
    /// Mueve un plugin externo entre pistas o slots en el motor de audio.
    MoveExternalPlugin {
        from_track: usize,
        from_slot: usize,
        to_track: usize,
        to_slot: usize,
    },
    /// Actualiza un parámetro de un plugin externo.
    SetExternalPluginParam {
        track_idx: usize,
        slot_idx: usize,
        param_id: u32,
        value: f32,
    },
    /// Activa o desactiva (bypass) un plugin externo sin descargarlo.
    SetExternalPluginActive {
        track_idx: usize,
        slot_idx: usize,
        active: bool,
    },
    /// Envía modulación no destructiva (CLAP `ParamModEvent`) a un parámetro de
    /// plugin externo.
    SetExternalPluginMod {
        track_idx: usize,
        slot_idx: usize,
        param_id: u32,
        delta: f32,
    },
    /// Varios parámetros de un plugin externo, de una sola vez.
    ///
    /// Es la vía que usan los knobs del rack y el feedback del editor nativo:
    /// la GUI los deja pendientes en el slot y los manda juntos, una vez por
    /// frame, en vez de un comando por parámetro.
    SyncExternalPluginParams {
        track_idx: usize,
        slot_idx: usize,
        params: Vec<(u32, f32)>,
    },
    /// Restaura el estado binario completo (preset + parámetros internos) de un
    /// plugin externo en el motor de audio (VST3 `setState` / CLAP `state.load`).
    LoadExternalPluginState {
        track_idx: usize,
        slot_idx: usize,
        state: Vec<u8>,
    },
    /// Carga un efecto nativo (ej. Hikaru Ultracomb) en un slot del rack.
    LoadNativeEffect {
        track_idx: usize,
        slot_idx: usize,
        name: String,
    },
    /// Carga un efecto nativo dentro de la cadena anidada `Wet FX` de `Hikaru Open8-Bit`.
    LoadBit8WetNativeEffect {
        track_idx: usize,
        slot_idx: usize,
        wet_idx: usize,
        name: String,
    },
    /// Carga un plugin externo VST3/CLAP dentro de la cadena anidada `Wet FX` de `Hikaru Open8-Bit`.
    LoadBit8WetExternalPlugin {
        track_idx: usize,
        slot_idx: usize,
        wet_idx: usize,
        descriptor: hikaru_plugin_host::PluginDescriptor,
    },
    /// Elimina un efecto de la cadena anidada `Wet FX` de `Hikaru Open8-Bit`.
    RemoveBit8WetFx {
        track_idx: usize,
        slot_idx: usize,
        wet_idx: usize,
    },
    /// Activa o desactiva (bypass) un efecto dentro de `Wet FX` de `Hikaru Open8-Bit`.
    SetBit8WetFxActive {
        track_idx: usize,
        slot_idx: usize,
        wet_idx: usize,
        active: bool,
    },
    /// Actualiza un parámetro normalizado de un efecto dentro de `Wet FX` de `Hikaru Open8-Bit`.
    SetBit8WetFxParam {
        track_idx: usize,
        slot_idx: usize,
        wet_idx: usize,
        param_id: u32,
        value: f32,
    },
    /// Sincroniza varios parámetros de un efecto dentro de `Wet FX` de `Hikaru Open8-Bit`.
    SyncBit8WetFxParams {
        track_idx: usize,
        slot_idx: usize,
        wet_idx: usize,
        params: Vec<(u32, f32)>,
    },
}