// Hikaru OpenLive - App (Immediate-mode egui + eframe)
// GNU AGPLv3
// crates/hikaru_gui/src/app.rs

use std::collections::HashSet;
use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::path::PathBuf;

use hikaru_audio_engine::AudioEngine;
use hikaru_core::SampleRate;
use hikaru_transport::{TapTempoDetector, TransportPlaybackState, TransportPosition};
use hikaru_plugin_host::PluginInstance;

use crate::audio_proxy::{AudioProxy, GuiCommand};
use crate::render::WavetableViewportHandle;
use crate::theme;
use crate::views::{
    about, arranger_view, audio_detail_editor, audio_settings, clip_editor, detail_editor_panel, control_panel, dsp_rack, explorer,
    external_plugins_settings, footer,
    header, matrix, mcp_port_settings, mixer, modal, piano_roll, playlist, popup_browser,
};

pub fn sync_matrix_mixer_bidirectional(
    live_tracks: &mut Vec<mixer::Track>,
    matrix_state: &mut matrix::SessionMatrixState,
    old_matrix: &[matrix::TrackMeta],
    old_mixer: &[(f32, f32, bool, bool)],
    audio_proxy: &AudioProxy,
) {
    if live_tracks.is_empty() || !live_tracks[0].is_master {
        live_tracks.insert(0, mixer::Track::new(0, "MASTER".to_string(), true));
    }
    live_tracks[0].matrix_idx = None;

    let matrix_len = matrix_state.tracks.len();

    while live_tracks.len() > matrix_len + 1 {
        live_tracks.pop();
    }
    while live_tracks.len() < matrix_len + 1 {
        let idx = live_tracks.len() - 1;
        let mx = &matrix_state.tracks[idx];
        let mut t = mixer::Track::new(idx + 1, mx.name.clone(), false);
        t.volume = mx.volume;
        t.pan = mx.pan;
        t.mute = mx.muted;
        t.solo = mx.soloed;
        t.matrix_idx = Some(idx);
        live_tracks.push(t);
    }

    for i in 0..matrix_len {
        let mixer_idx = i + 1;
        if mixer_idx >= live_tracks.len() {
            break;
        }

        let (mx_name, mx_vol, mx_pan, mx_muted, mx_soloed) = {
            let mx = &matrix_state.tracks[i];
            (mx.name.clone(), mx.volume, mx.pan, mx.muted, mx.soloed)
        };
        let old_mx = old_matrix.get(i);
        let old_mx_vol = old_mx.map(|m| m.volume).unwrap_or(0.75);
        let old_mx_pan = old_mx.map(|m| m.pan).unwrap_or(0.0);
        let old_mx_mute = old_mx.map(|m| m.muted).unwrap_or(false);
        let old_mx_solo = old_mx.map(|m| m.soloed).unwrap_or(false);
        let old_mix = old_mixer.get(mixer_idx);
        let old_mix_vol = old_mix.map(|m| m.0).unwrap_or(0.75);
        let old_mix_pan = old_mix.map(|m| m.1).unwrap_or(0.0);
        let old_mix_mute = old_mix.map(|m| m.2).unwrap_or(false);
        let old_mix_solo = old_mix.map(|m| m.3).unwrap_or(false);

        let t = &mut live_tracks[mixer_idx];
        t.name = mx_name;
        t.matrix_idx = Some(i);

        let matrix_changed = (mx_vol - old_mx_vol).abs() > f32::EPSILON;
        let mixer_changed = (t.volume - old_mix_vol).abs() > f32::EPSILON;
        if matrix_changed && !mixer_changed {
            t.volume = mx_vol;
        } else if mixer_changed {
            matrix_state.tracks[i].volume = t.volume;
            audio_proxy.send(GuiCommand::SetTrackVolume {
                track_idx: i,
                volume_db: t.volume,
            });
        }

        let matrix_changed = (mx_pan - old_mx_pan).abs() > f32::EPSILON;
        let mixer_changed = (t.pan - old_mix_pan).abs() > f32::EPSILON;
        if matrix_changed && !mixer_changed {
            t.pan = mx_pan;
        } else if mixer_changed {
            matrix_state.tracks[i].pan = t.pan;
            audio_proxy.send(GuiCommand::SetTrackPan {
                track_idx: i,
                pan: t.pan,
            });
        }

        let matrix_changed = mx_muted != old_mx_mute;
        let mixer_changed = t.mute != old_mix_mute;
        if matrix_changed && !mixer_changed {
            t.mute = mx_muted;
            audio_proxy.send(GuiCommand::SetTrackMute {
                track_idx: i,
                mute: t.mute,
            });
        } else if mixer_changed {
            matrix_state.tracks[i].muted = t.mute;
            audio_proxy.send(GuiCommand::SetTrackMute {
                track_idx: i,
                mute: t.mute,
            });
        }

        let matrix_changed = mx_soloed != old_mx_solo;
        let mixer_changed = t.solo != old_mix_solo;
        if matrix_changed && !mixer_changed {
            t.solo = mx_soloed;
            audio_proxy.send(GuiCommand::SetTrackSolo {
                track_idx: i,
                solo: t.solo,
            });
        } else if mixer_changed {
            matrix_state.tracks[i].soloed = t.solo;
            audio_proxy.send(GuiCommand::SetTrackSolo {
                track_idx: i,
                solo: t.solo,
            });
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    OpenLive,
    OpenStudio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OpenLiveView {
    SessionMatrix = 0,
    ArrangerView = 1,
    EditView = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OpenStudioView {
    Playlist = 0,
    ArrangerMixer = 1,
    EditView = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanMode {
    Stereo,
    MidSide,
}

pub const BPM_MIN: f64 = 40.0;
pub const BPM_MAX: f64 = 300.0;

pub fn format_bpm(bpm: f64) -> String {
    if !bpm.is_finite() {
        return format!("{BPM_MIN:.0}");
    }
    let rounded = (bpm * 100.0).round() / 100.0;
    if (rounded - rounded.trunc()).abs() < f64::EPSILON {
        format!("{rounded:.0}")
    } else {
        let text = format!("{rounded:.2}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

pub const STANDARD_SIGS: [(u32, u32); 7] =
    [(2, 4), (3, 4), (4, 4), (5, 4), (6, 8), (7, 8), (12, 8)];

pub fn format_sig(num: u32, den: u32) -> String {
    format!("{}/{}", num.max(1), den.max(1))
}

pub fn parse_sig(text: &str) -> Option<(u32, u32)> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let (num_txt, den_txt) = match text.split_once('/') {
        Some((n, d)) => (n.trim(), Some(d.trim())),
        None => (text, None),
    };
    let Ok(num) = num_txt.parse::<u32>() else {
        return None;
    };
    if !(1..=16).contains(&num) {
        return None;
    }
    let den = match den_txt {
        None => 4,
        Some(d) => {
            let Ok(v) = d.parse::<u32>() else {
                return None;
            };
            if ![2, 4, 8, 16].contains(&v) {
                return None;
            }
            v
        }
    };
    Some((num, den))
}

pub fn step_sig(num: u32, den: u32, delta: i32) -> (u32, u32) {
    let idx = STANDARD_SIGS
        .iter()
        .position(|&(n, d)| n == num && d == den)
        .unwrap_or(2) as i32;
    let next = (idx + delta).clamp(0, STANDARD_SIGS.len() as i32 - 1) as usize;
    STANDARD_SIGS[next]
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelLayout {
    pub keyboard_main_splitter: f32,
    pub inspector_main_splitter: f32,
    pub main_browser_splitter: f32,
    pub arranger_detail_splitter: f32,
    pub mixer_detail_editor_splitter: f32,
    pub editor_other_splitter: f32,
    pub show_keyboard: bool,
    pub show_inspector: bool,
    pub show_arranger: bool,
    pub show_clip_launcher: bool,
    pub show_mixer: bool,
    pub show_note_editor: bool,
    pub show_automation: bool,
}

impl Default for PanelLayout {
    fn default() -> Self {
        Self {
            keyboard_main_splitter: 0.0,
            inspector_main_splitter: 0.0,
            main_browser_splitter: 0.0,
            arranger_detail_splitter: 0.6,
            mixer_detail_editor_splitter: 0.5,
            editor_other_splitter: 0.5,
            show_keyboard: false,
            show_inspector: false,
            show_arranger: true,
            show_clip_launcher: true,
            show_mixer: false,
            show_note_editor: false,
            show_automation: false,
        }
    }
}

pub const PARAM_DRAG_RANGE_PX: f32 = 160.0;
const PARAM_DRAG_FINE: f32 = 0.25;
const PARAM_DRAG_AXIS_LOCK_PX: f32 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamDragAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug)]
pub struct ParamDragState {
    pub track: usize,
    pub slot: usize,
    pub param_id: u32,
    pub origin_x: f32,
    pub origin_y: f32,
    pub start_value: f32,
    axis: Option<ParamDragAxis>,
}

impl ParamDragState {
    pub fn new(
        track: usize,
        slot: usize,
        param_id: u32,
        x: f32,
        y: f32,
        start_value: f32,
    ) -> Self {
        Self { track, slot, param_id, origin_x: x, origin_y: y, start_value, axis: None }
    }

    fn axis_for(&self, x: f32, y: f32) -> ParamDragAxis {
        let dx = (x - self.origin_x).abs();
        let dy = (y - self.origin_y).abs();
        if dx >= dy {
            ParamDragAxis::Horizontal
        } else {
            ParamDragAxis::Vertical
        }
    }

    pub fn advance(&mut self, x: f32, y: f32, fine: bool) -> Option<f32> {
        if !x.is_finite() || !y.is_finite() || !self.start_value.is_finite() {
            return None;
        }
        let axis = match self.axis {
            Some(axis) => axis,
            None => {
                if (x - self.origin_x).abs().max((y - self.origin_y).abs()) < PARAM_DRAG_AXIS_LOCK_PX {
                    return None;
                }
                let locked = self.axis_for(x, y);
                self.axis = Some(locked);
                locked
            }
        };
        let delta = match axis {
            ParamDragAxis::Horizontal => x - self.origin_x,
            ParamDragAxis::Vertical => self.origin_y - y,
        };
        let gain = if fine { PARAM_DRAG_FINE } else { 1.0 };
        Some((self.start_value + delta / PARAM_DRAG_RANGE_PX * gain).clamp(0.0, 1.0))
    }
}

pub struct ParamProbeResult {
    pub descriptor_id: String,
    pub params: Vec<(hikaru_plugin_host::PluginParamInfo, f32)>,
    pub error: Option<String>,
    pub has_gui: bool,
}

fn probe_external_params(descriptor: &hikaru_plugin_host::PluginDescriptor) -> ParamProbeResult {
    use hikaru_plugin_host::{ClapInstance, PluginInstance, Vst3Instance};

    let load_result = match descriptor.format {
        hikaru_plugin_host::PluginFormat::VST3 => {
            Vst3Instance::load(&descriptor.path).map(|i| Box::new(i) as Box<dyn PluginInstance>)
        }
        hikaru_plugin_host::PluginFormat::CLAP => {
            ClapInstance::load(&descriptor.path).map(|i| Box::new(i) as Box<dyn PluginInstance>)
        }
    };

    let mut instance = match load_result {
        Ok(instance) => instance,
        Err(message) => {
            return ParamProbeResult {
                descriptor_id: descriptor.id.clone(),
                params: Vec::new(),
                error: Some(format!("No se pudo instanciar: {message}")),
                has_gui: false,
            };
        }
    };

    if !instance.params_supported() {
        return ParamProbeResult {
            descriptor_id: descriptor.id.clone(),
            params: Vec::new(),
            error: Some("El plugin no expone parámetros.".to_string()),
            has_gui: instance.has_gui(),
        };
    }

    let params = instance.snapshot_params();
    ParamProbeResult {
        descriptor_id: descriptor.id.clone(),
        params,
        error: None,
        has_gui: instance.has_gui(),
    }
}

pub struct AppState {
    pub mode: AppMode,
    pub transport: TransportPosition,
    pub position_clock: Arc<AtomicU64>,
    pub output_level_bits: Arc<AtomicU32>,
    pub audio_proxy: AudioProxy,
    pub is_looping: bool,
    pub metronome_enabled: bool,
    pub play_menu: crate::ui::play_menu::PlayMenuState,
    pub edit_history: crate::ui::edit_menu::EditHistory,
    pub punch_in: bool,
    pub punch_out: bool,
    pub tap_tempo: TapTempoDetector,
    pub tap_tempo_enabled: bool,
    pub pending_scene_index_atomic: Arc<AtomicI32>,
    pub queued_scene_index: Option<usize>,
    pub editing_bpm: bool,
    pub editing_sig: bool,
    pub bpm_input_first_key: bool,
    pub sig_input_first_key: bool,
    pub cpu_usage: f32,
    pub bpm_synced_to_engine: f64,
    pub bpm_input: String,
    pub sig_input: String,
    pub global_loop_synced_to_engine: Option<(bool, u64, u64)>,
    pub show_dsp_rack: bool,
    pub openstudio_view: OpenStudioView,
    pub show_about: bool,
    pub is_recording: bool,
    pub show_explorer: bool,
    pub show_piano_roll: bool,
    pub panel_layout: PanelLayout,
    pub piano_roll_state: piano_roll::PianoRollState,
    pub explorer_state: explorer::FileExplorerState,

    pub audio_settings_state: audio_settings::AudioSettingsState,
    pub plugin_settings_state: external_plugins_settings::PluginSettingsState,
    pub mcp_port_settings_state: mcp_port_settings::McpPortSettingsState,
    pub control_panel_state: control_panel::ControlPanelState,
    pub modal_drags: modal::ModalDrags,
    pub playlist_state: playlist::PlaylistState,
    pub matrix_state: matrix::SessionMatrixState,
    pub clip_editor: clip_editor::ClipEditorState,
    pub matrix_clipboard: matrix::MatrixClipboard,
    pub dragged_sample: Option<PathBuf>,
    pub pending_wavetable_slot: Option<(usize, usize)>,

    pub live_tracks: Vec<mixer::Track>,
    pub studio_tracks: Vec<mixer::Track>,
    pub fx_tracks: Vec<mixer::Track>,

    pub selected_track_index: usize,
    pub selected_fx_index: Option<usize>,
    pub selected_slot_index: usize,
    pub footer_context_help: Option<(String, String)>,
    pub footer_context_cues: Vec<(String, String)>,
    pub active_modifiers: egui::Modifiers,
    pub add_slot_menu_open: bool,
    pub plugin_browser_query: String,
    pub plugin_browser_filter: dsp_rack::PluginBrowserFilter,
    pub plugin_search_input: String,
    pub popup_browser: popup_browser::PopupBrowserState,
    pub param_drag: Option<ParamDragState>,
    pub param_probe_tx: Sender<ParamProbeResult>,
    pub param_probe_rx: Receiver<ParamProbeResult>,
    pub param_probing: HashSet<String>,

    pub track_peak_bits: Vec<Arc<AtomicU32>>,
    pub track_clip_playhead: Vec<Arc<AtomicU64>>,
    pub smoothed_track_peaks: [f32; 16],
    pub smoothed_master_peak: f32,

    pub openlive_view: OpenLiveView,
    pub arranger_fader_drag: Option<arranger_view::FaderTarget>,
    pub mixer_scroll_offset_x: f32,
    pub mixer_scroll_offset_y: f32,
    pub playlist_pan: Option<playlist::PlaylistPanState>,
    pub mixer_pan: Option<arranger_view::MixerPanState>,
    pub mixer_bar_drag: Option<f32>,
    pub matrix_mix_drag: Option<matrix::MatrixMixTarget>,
    pub matrix_pan_gesture: Option<(f32, f32)>,
    pub matrix_mix_bounds: [f32; 4],

    pub bottom_mixer_height: f32,
    pub bottom_mixer_splitter_drag: Option<(f32, f32)>,
    pub bottom_mixer_show_devices: bool,
    pub bottom_mixer_show_sends: bool,
    pub bottom_mixer_show_io: bool,
    pub bottom_mixer_scroll_offset_x: f32,

    pub arranger_track_width: f32,
    pub arranger_track_width_drag: Option<(arranger_view::StripTarget, f32, f32)>,
    pub dragging_device: Option<crate::views::mixer::MixerDeviceDrag>,

    pub active_external_plugins: Vec<Box<dyn PluginInstance>>,
    pub engine_handle: Option<Arc<Mutex<AudioEngine<'static>>>>,
    pub wavetable_viewport: WavetableViewportHandle,

    pub project_path: Option<PathBuf>,
    pub project_dirty: bool,
    pub open_menu: Option<usize>,
    pub synced_wavetable_fingerprints: [Option<(u64, i8, u32)>; 16],
    pub synced_sampler_revs: [Option<(u64, u64)>; 16],
    pub synced_open_poly_revs: [Option<(u64, u64, u32, u32, u32)>; 16],
    pub synced_open_grid_revs: [Option<u64>; 16],
    /// When `Some((track_idx, slot_idx))`, displays the full interactive Hikaru OpenGrid patch editor
    /// in the central workspace panel above the bottom DSP Rack.
    pub open_grid_editor: Option<(usize, usize)>,
}

impl AppState {
    pub fn new(
        audio_proxy: AudioProxy,
        position_clock: Arc<AtomicU64>,
        output_level_bits: Arc<AtomicU32>,
        engine_handle: Option<Arc<Mutex<AudioEngine<'static>>>>,
    ) -> Self {
        let sample_rate = SampleRate::new(48000.0);
        let transport = TransportPosition::new(sample_rate, 140.0);

        let mut live_tracks = vec![mixer::Track::new(0, "MASTER".to_string(), true)];
        let mut matrix_state = matrix::SessionMatrixState::default();
        {
            let empty_old: Vec<matrix::TrackMeta> = Vec::new();
            let empty_mixer_old: Vec<(f32, f32, bool, bool)> = Vec::new();
            sync_matrix_mixer_bidirectional(
                &mut live_tracks,
                &mut matrix_state,
                &empty_old,
                &empty_mixer_old,
                &audio_proxy,
            );
        }
        for t in &mut live_tracks {
            if t.volume <= 0.0 {
                t.volume = 0.70;
            }
        }

        let mut studio_tracks = vec![
            mixer::Track::new(0, "MASTER".to_string(), true),
            mixer::Track::new(1, "TRACK 01".to_string(), false),
        ];
        for t in &mut studio_tracks {
            t.volume = 0.70;
        }
        let fx_tracks = vec![mixer::Track::new_fx(0, "FX 1".to_string())];

        let (track_peak_bits, track_clip_playhead, pending_scene_index_atomic): (
            Vec<Arc<AtomicU32>>,
            Vec<Arc<AtomicU64>>,
            Arc<AtomicI32>,
        ) = if let Some(ref handle) = engine_handle {
            if let Ok(engine) = handle.try_lock() {
                (
                    engine.track_peak_bits.iter().map(|a| Arc::clone(a)).collect(),
                    engine.track_clip_playhead.iter().map(|a| Arc::clone(a)).collect(),
                    Arc::clone(&engine.pending_scene_index_atomic),
                )
            } else {
                (
                    (0..16).map(|_| Arc::new(AtomicU32::new(0.0f32.to_bits()))).collect(),
                    (0..16).map(|_| Arc::new(AtomicU64::new(0))).collect(),
                    Arc::new(AtomicI32::new(-1)),
                )
            }
        } else {
            (
                (0..16).map(|_| Arc::new(AtomicU32::new(0.0f32.to_bits()))).collect(),
                (0..16).map(|_| Arc::new(AtomicU64::new(0))).collect(),
                Arc::new(AtomicI32::new(-1)),
            )
        };

        let (param_probe_tx, param_probe_rx) = std::sync::mpsc::channel();
        let initial_bpm = transport.bpm;

        let state = Self {
            mode: AppMode::OpenLive,
            transport,
            position_clock,
            output_level_bits,
            audio_proxy,
            is_looping: false,
            metronome_enabled: false,
            play_menu: crate::ui::play_menu::PlayMenuState::default(),
            punch_in: false,
            punch_out: false,
            tap_tempo: TapTempoDetector::with_bpm_range(BPM_MIN, BPM_MAX),
            tap_tempo_enabled: false,
            pending_scene_index_atomic,
            queued_scene_index: None,
            editing_bpm: false,
            editing_sig: false,
            bpm_input_first_key: false,
            sig_input_first_key: false,
            cpu_usage: 0.12,
            bpm_synced_to_engine: -1.0,
            bpm_input: format_bpm(initial_bpm),
            sig_input: "4/4".to_string(),
            global_loop_synced_to_engine: None,
            show_dsp_rack: true,
            openstudio_view: OpenStudioView::Playlist,
            show_about: false,
            is_recording: false,
            show_explorer: true,
            show_piano_roll: false,
            panel_layout: PanelLayout::default(),
            piano_roll_state: piano_roll::PianoRollState::default(),
            explorer_state: explorer::FileExplorerState::default(),
            audio_settings_state: audio_settings::AudioSettingsState::default(),
            plugin_settings_state: external_plugins_settings::PluginSettingsState::default(),
            mcp_port_settings_state: mcp_port_settings::McpPortSettingsState::default(),
            control_panel_state: control_panel::ControlPanelState::default(),
            modal_drags: modal::ModalDrags::default(),
            playlist_state: playlist::PlaylistState::default(),
            edit_history: crate::ui::edit_menu::EditHistory::default(),
            clip_editor: clip_editor::ClipEditorState::default(),
            matrix_state,
            matrix_clipboard: matrix::MatrixClipboard::default(),
            dragged_sample: None,
            pending_wavetable_slot: None,
            live_tracks,
            studio_tracks,
            fx_tracks,
            selected_track_index: 1,
            selected_fx_index: None,
            selected_slot_index: 0,
            footer_context_help: None,
            footer_context_cues: Vec::new(),
            active_modifiers: egui::Modifiers::default(),
            add_slot_menu_open: false,
            plugin_browser_query: String::new(),
            plugin_browser_filter: dsp_rack::PluginBrowserFilter::default(),
            plugin_search_input: String::new(),
            popup_browser: popup_browser::PopupBrowserState::default(),
            param_drag: None,
            param_probe_tx,
            param_probe_rx,
            param_probing: HashSet::new(),
            track_peak_bits,
            track_clip_playhead,
            smoothed_track_peaks: [0.0; 16],
            smoothed_master_peak: 0.0,
            openlive_view: OpenLiveView::SessionMatrix,
            arranger_fader_drag: None,
            mixer_scroll_offset_x: 0.0,
            mixer_scroll_offset_y: 0.0,
            playlist_pan: None,
            mixer_pan: None,
            mixer_bar_drag: None,
            matrix_mix_drag: None,
            matrix_pan_gesture: None,
            matrix_mix_bounds: [0.0; 4],
            bottom_mixer_height: 240.0,
            bottom_mixer_splitter_drag: None,
            bottom_mixer_show_devices: true,
            bottom_mixer_show_sends: true,
            bottom_mixer_show_io: true,
            bottom_mixer_scroll_offset_x: 0.0,
            arranger_track_width: 132.0,
            arranger_track_width_drag: None,
            dragging_device: None,
            active_external_plugins: Vec::new(),
            engine_handle,
            wavetable_viewport: WavetableViewportHandle::new(),
            project_path: None,
            project_dirty: false,
            open_menu: None,
            synced_wavetable_fingerprints: [None; 16],
            synced_sampler_revs: [None; 16],
            synced_open_poly_revs: [None; 16],
            synced_open_grid_revs: [None; 16],
            open_grid_editor: None,
        };
        state.audio_proxy.send(GuiCommand::SetAppMode(false));
        state
    }

    pub fn sync_open_grid_to_engine(&mut self) {
        let is_openstudio = self.mode == AppMode::OpenStudio;
        let tracks = if is_openstudio {
            &mut self.studio_tracks
        } else {
            &mut self.live_tracks
        };

        for (tr_idx, track) in tracks.iter_mut().enumerate() {
            let Some(eng_track) = tr_idx.checked_sub(1) else { continue };
            if eng_track >= 16 {
                continue;
            }
            let grid_slot = track
                .effects
                .iter_mut()
                .find(|slot| slot.active && slot.is_open_grid());
            if let Some(slot) = grid_slot {
                let grid = slot.open_grid_mut();
                if self.synced_open_grid_revs[eng_track] != Some(grid.patch_rev) {
                    self.audio_proxy.send(GuiCommand::SetTrackOpenGridPatch {
                        track_idx: eng_track,
                        enabled: true,
                        patch: Some(grid.patch.clone()),
                    });
                    self.synced_open_grid_revs[eng_track] = Some(grid.patch_rev);
                }
            } else if self.synced_open_grid_revs[eng_track].is_some() {
                self.audio_proxy.send(GuiCommand::SetTrackOpenGridPatch {
                    track_idx: eng_track,
                    enabled: false,
                    patch: None,
                });
                self.synced_open_grid_revs[eng_track] = None;
            }
        }
    }

    pub fn sync_open_poly_to_engine(&mut self) {
        let is_openstudio = self.mode == AppMode::OpenStudio;
        let tracks = if is_openstudio {
            &mut self.studio_tracks
        } else {
            &mut self.live_tracks
        };

        for (tr_idx, track) in tracks.iter_mut().enumerate() {
            let Some(eng_track) = tr_idx.checked_sub(1) else { continue };
            if eng_track >= 16 {
                continue;
            }

            let poly_slot = track
                .effects
                .iter_mut()
                .find(|slot| slot.active && slot.is_open_poly());

            if let Some(slot) = poly_slot {
                let poly = slot.open_poly_mut();
                let mut dsp_params = poly.to_dsp_params();

                // Apply live external Hikaru OpenLFO MOD deltas to DSP params (never mutating base UI values!)
                let get_mod = |p: &str| {
                    crate::views::hikaru_native_plugins::hikaru_openmodulation::get_modulation(
                        tr_idx, "OpenPoly", p,
                    )
                };
                if let Some(m) = get_mod("INDEX").or_else(|| {
                    crate::views::hikaru_native_plugins::hikaru_openmodulation::get_modulation(
                        tr_idx,
                        "OpenWavetable",
                        "WT POS",
                    )
                }) {
                    dsp_params.osc_index = (dsp_params.osc_index + m.delta).clamp(0.0, 1.0);
                }
                if let Some(m) = get_mod("FOLD") {
                    dsp_params.osc_fold = (dsp_params.osc_fold + m.delta).clamp(0.0, 1.0);
                }
                if let Some(m) = get_mod("SUB") {
                    dsp_params.sub_level = (dsp_params.sub_level + m.delta).clamp(0.0, 1.0);
                }
                if let Some(m) = get_mod("NOISE") {
                    dsp_params.noise_level = (dsp_params.noise_level + m.delta).clamp(0.0, 1.0);
                }
                if let Some(m) = get_mod("DRV") {
                    dsp_params.filter_drive = (dsp_params.filter_drive + m.delta).clamp(0.0, 1.0);
                }
                if let Some(m) = get_mod("KTR") {
                    dsp_params.filter_keytrack =
                        (dsp_params.filter_keytrack + m.delta).clamp(0.0, 1.0);
                }
                if let Some(m) = get_mod("ENV") {
                    dsp_params.filter_env_amount =
                        (dsp_params.filter_env_amount + m.delta * 2.0).clamp(-1.0, 1.0);
                }
                if let Some(m) = get_mod("CUTOFF") {
                    let oct = m.delta * 4.0;
                    dsp_params.filter_cutoff_hz =
                        (dsp_params.filter_cutoff_hz * 2.0_f32.powf(oct)).clamp(20.0, 20000.0);
                }
                if let Some(m) = get_mod("RESO") {
                    dsp_params.filter_resonance =
                        (dsp_params.filter_resonance + m.delta * 0.98).clamp(0.0, 0.98);
                }
                if let Some(m) = get_mod("AMP_A") {
                    dsp_params.amp_attack_ms =
                        (dsp_params.amp_attack_ms * 2.0_f32.powf(m.delta * 4.0)).clamp(0.5, 5000.0);
                }
                if let Some(m) = get_mod("AMP_D") {
                    dsp_params.amp_decay_ms =
                        (dsp_params.amp_decay_ms * 2.0_f32.powf(m.delta * 4.0)).clamp(1.0, 8000.0);
                }
                if let Some(m) = get_mod("AMP_S") {
                    dsp_params.amp_sustain = (dsp_params.amp_sustain + m.delta).clamp(0.0, 1.0);
                }
                if let Some(m) = get_mod("AMP_R") {
                    dsp_params.amp_release_ms =
                        (dsp_params.amp_release_ms * 2.0_f32.powf(m.delta * 4.0)).clamp(1.0, 8000.0);
                }
                if let Some(m) = get_mod("FEG_A") {
                    dsp_params.feg_attack_ms =
                        (dsp_params.feg_attack_ms * 2.0_f32.powf(m.delta * 4.0)).clamp(0.5, 5000.0);
                }
                if let Some(m) = get_mod("FEG_D") {
                    dsp_params.feg_decay_ms =
                        (dsp_params.feg_decay_ms * 2.0_f32.powf(m.delta * 4.0)).clamp(1.0, 8000.0);
                }
                if let Some(m) = get_mod("FEG_S") {
                    dsp_params.feg_sustain = (dsp_params.feg_sustain + m.delta).clamp(0.0, 1.0);
                }
                if let Some(m) = get_mod("FEG_R") {
                    dsp_params.feg_release_ms =
                        (dsp_params.feg_release_ms * 2.0_f32.powf(m.delta * 4.0)).clamp(1.0, 8000.0);
                }
                if let Some(m) = get_mod("VEL") {
                    dsp_params.velocity_sens = (dsp_params.velocity_sens + m.delta).clamp(0.0, 1.0);
                }
                if let Some(m) = get_mod("PITCH") {
                    dsp_params.global_pitch_st =
                        (dsp_params.global_pitch_st + m.delta * 24.0).clamp(-24.0, 24.0);
                }
                if let Some(m) = get_mod("GLIDE") {
                    dsp_params.glide_ms =
                        (dsp_params.glide_ms + m.delta * 1500.0).clamp(0.0, 2000.0);
                }
                if let Some(m) = get_mod("PAN") {
                    dsp_params.pan = (dsp_params.pan + m.delta * 2.0).clamp(-1.0, 1.0);
                }
                if let Some(m) = get_mod("DETUNE") {
                    dsp_params.unison_detune = (dsp_params.unison_detune + m.delta).clamp(0.0, 1.0);
                }
                if let Some(m) = get_mod("VOL").or_else(|| {
                    crate::views::hikaru_native_plugins::hikaru_openmodulation::get_modulation(
                        tr_idx,
                        "OpenWavetable",
                        "VOL",
                    )
                }) {
                    dsp_params.output_gain =
                        (dsp_params.output_gain + m.delta * 2.0).clamp(0.0, 2.0);
                }

                let osc_hash = dsp_params.osc_index.to_bits()
                    ^ dsp_params.osc_fold.to_bits().rotate_left(7)
                    ^ dsp_params.sub_level.to_bits().rotate_left(13)
                    ^ dsp_params.noise_level.to_bits().rotate_left(19);
                let flt_hash = dsp_params.filter_cutoff_hz.to_bits()
                    ^ dsp_params.filter_resonance.to_bits().rotate_left(5)
                    ^ dsp_params.filter_drive.to_bits().rotate_left(11)
                    ^ dsp_params.filter_keytrack.to_bits().rotate_left(17)
                    ^ dsp_params.filter_env_amount.to_bits().rotate_left(23);
                let env_out_hash = dsp_params.output_gain.to_bits()
                    ^ dsp_params.pan.to_bits().rotate_left(3)
                    ^ dsp_params.unison_detune.to_bits().rotate_left(9)
                    ^ dsp_params.global_pitch_st.to_bits().rotate_left(15)
                    ^ dsp_params.glide_ms.to_bits().rotate_left(21)
                    ^ dsp_params.amp_attack_ms.to_bits().rotate_left(2)
                    ^ dsp_params.amp_sustain.to_bits().rotate_left(8)
                    ^ dsp_params.feg_attack_ms.to_bits().rotate_left(14);
                let prev = self.synced_open_poly_revs[eng_track];
                let prev_wt = prev.map(|p| p.0);
                let cur_key = (
                    poly.wavetable_rev,
                    poly.params_rev,
                    osc_hash,
                    flt_hash,
                    env_out_hash,
                );

                if prev_wt != Some(poly.wavetable_rev) {
                    self.audio_proxy.send(GuiCommand::SetTrackOpenPolyWavetable {
                        track_idx: eng_track,
                        samples: poly.wt_data.samples.clone(),
                        frames: poly.wt_data.frames,
                        frame_len: poly.wt_data.frame_len,
                    });
                }
                if prev != Some(cur_key) {
                    self.audio_proxy.send(GuiCommand::SetTrackOpenPolyParams {
                        track_idx: eng_track,
                        enabled: true,
                        params: dsp_params,
                    });
                    self.synced_open_poly_revs[eng_track] = Some(cur_key);
                }
            } else if self.synced_open_poly_revs[eng_track].is_some() {
                self.audio_proxy.send(GuiCommand::SetTrackOpenPolyParams {
                    track_idx: eng_track,
                    enabled: false,
                    params: hikaru_dsp::synth::open_poly::OpenPolyDspParams::default(),
                });
                self.synced_open_poly_revs[eng_track] = None;
            }
        }
    }

    pub fn sync_wavetables_to_engine(&mut self) {
        let is_playing = self.transport.playback_state == hikaru_transport::TransportPlaybackState::Playing;
        crate::views::hikaru_native_plugins::hikaru_openmodulation::update_global_transport(
            self.transport.bpm as f32,
            self.transport.sample_count,
            self.transport.sample_rate.get().max(1.0),
            is_playing,
        );
        self.sync_open_poly_to_engine();
        self.sync_open_grid_to_engine();
    }

    pub fn sync_samplers_to_engine(&mut self) {
        let tracks_len = self.current_tracks().len();
        for mixer_idx in 1..tracks_len {
            let eng_track = mixer_idx - 1;
            if eng_track >= 16 {
                break;
            }
            let smp_slot = self.current_tracks()[mixer_idx]
                .effects
                .iter()
                .find(|s| matches!(s.name.as_str(), "Sampler" | "Hikaru Sampler") && s.active);

            if let Some(slot) = smp_slot {
                if let Some(ref smp) = slot.sampler_state {
                    let prev = self.synced_sampler_revs[eng_track];
                    let (prev_s, prev_p) = prev.unwrap_or((u64::MAX, u64::MAX));
                    if prev.is_none() || prev_s != smp.sample_rev {
                        self.audio_proxy.send(GuiCommand::SetTrackSamplerSample {
                            track_idx: eng_track,
                            samples: smp.samples.clone(),
                            channels: smp.channels,
                            sample_rate: smp.sample_rate,
                        });
                    }
                    if prev.is_none() || prev_p != smp.params_rev {
                        self.audio_proxy.send(GuiCommand::SetTrackSamplerParams {
                            track_idx: eng_track,
                            params: smp.to_dsp_params(),
                        });
                    }
                    self.synced_sampler_revs[eng_track] = Some((smp.sample_rev, smp.params_rev));
                }
            } else if self.synced_sampler_revs[eng_track].is_some() {
                self.audio_proxy.send(GuiCommand::SetTrackSamplerSample {
                    track_idx: eng_track,
                    samples: Vec::new(),
                    channels: 1,
                    sample_rate: 44100.0,
                });
                self.synced_sampler_revs[eng_track] = None;
            }
        }
    }

    pub fn tracks(&self) -> &Vec<mixer::Track> {
        match self.mode {
            AppMode::OpenLive => &self.live_tracks,
            AppMode::OpenStudio => &self.studio_tracks,
        }
    }

    pub fn tracks_mut(&mut self) -> &mut Vec<mixer::Track> {
        match self.mode {
            AppMode::OpenLive => &mut self.live_tracks,
            AppMode::OpenStudio => &mut self.studio_tracks,
        }
    }

    /// Deletes a non-master track by its mixer index (`>= 1`), keeping
    /// `matrix_state`, `live_tracks`, `studio_tracks`, and selections in sync.
    pub fn delete_track(&mut self, mixer_idx: usize) {
        if mixer_idx == 0 || self.matrix_state.tracks.len() <= 1 {
            return;
        }
        let matrix_idx = mixer_idx - 1;
        if matrix_idx < self.matrix_state.tracks.len() {
            self.matrix_state.tracks.remove(matrix_idx);
            self.matrix_state.grid.remove(matrix_idx);
            if let Some((t, s)) = self.matrix_state.selected_slot {
                if t == matrix_idx {
                    self.matrix_state.selected_slot = None;
                } else if t > matrix_idx {
                    self.matrix_state.selected_slot = Some((t - 1, s));
                }
            }
        }
        if mixer_idx < self.live_tracks.len() {
            self.live_tracks.remove(mixer_idx);
        }
        if mixer_idx < self.studio_tracks.len() {
            self.studio_tracks.remove(mixer_idx);
        }
        for (i, tr) in self.live_tracks.iter_mut().enumerate().skip(1) {
            tr.matrix_idx = Some(i - 1);
        }
        for (i, tr) in self.studio_tracks.iter_mut().enumerate().skip(1) {
            tr.matrix_idx = Some(i - 1);
        }
        if self.selected_track_index == mixer_idx {
            self.selected_track_index = self.selected_track_index.saturating_sub(1);
        } else if self.selected_track_index > mixer_idx {
            self.selected_track_index -= 1;
        }
    }

    /// Deletes an FX track by its index in `fx_tracks`, updating sends and selection.
    pub fn delete_fx_track(&mut self, fx_idx: usize) {
        if fx_idx >= self.fx_tracks.len() {
            return;
        }
        self.fx_tracks.remove(fx_idx);
        let removed_target = crate::views::mixer::ROUTE_FX_BASE + fx_idx;
        let adjust_sends = |tracks: &mut Vec<mixer::Track>| {
            for tr in tracks.iter_mut() {
                tr.sends.retain(|s| s.target_id != removed_target);
                for s in &mut tr.sends {
                    if s.target_id > removed_target {
                        s.target_id -= 1;
                    }
                }
            }
        };
        adjust_sends(&mut self.live_tracks);
        adjust_sends(&mut self.studio_tracks);
        adjust_sends(&mut self.fx_tracks);

        if self.fx_tracks.is_empty() {
            self.selected_fx_index = None;
        } else {
            self.selected_fx_index = Some(fx_idx.min(self.fx_tracks.len() - 1));
        }
    }

    pub fn current_tracks(&self) -> &Vec<mixer::Track> {
        self.tracks()
    }

    pub fn current_tracks_mut(&mut self) -> &mut Vec<mixer::Track> {
        self.tracks_mut()
    }

    pub fn add_fx_track(&mut self) -> usize {
        let fx_num = self.fx_tracks.len() + 1;
        let fx = mixer::Track::new_fx(fx_num.saturating_sub(1), format!("FX {fx_num}"));
        self.fx_tracks.push(fx);
        self.fx_tracks.len() - 1
    }

    pub fn safe_track_index(&self) -> usize {
        self.selected_track_index.min(self.tracks().len().saturating_sub(1))
    }

    pub fn selected_track(&self) -> Option<&mixer::Track> {
        if let Some(fx_idx) = self.selected_fx_index {
            if let Some(fx) = self.fx_tracks.get(fx_idx) {
                return Some(fx);
            }
        }
        self.tracks().get(self.safe_track_index())
    }

    pub fn selected_track_mut(&mut self) -> Option<&mut mixer::Track> {
        if let Some(fx_idx) = self.selected_fx_index {
            if fx_idx < self.fx_tracks.len() {
                return self.fx_tracks.get_mut(fx_idx);
            }
        }
        let idx = self.safe_track_index();
        self.tracks_mut().get_mut(idx)
    }

    pub fn engine_track_index(&self) -> Option<usize> {
        self.safe_track_index().checked_sub(1)
    }

    pub fn selected_slot(&self) -> Option<&mixer::DspSlot> {
        self.tracks().get(self.safe_track_index())?.effects.get(self.selected_slot_index)
    }

    pub fn selected_slot_mut(&mut self) -> Option<&mut mixer::DspSlot> {
        let track = self.selected_track_index.min(self.tracks().len().saturating_sub(1));
        let slot = self.selected_slot_index;
        self.tracks_mut().get_mut(track)?.effects.get_mut(slot)
    }

    pub fn slot(&self, track_idx: usize, slot_idx: usize) -> Option<&mixer::DspSlot> {
        self.tracks().get(track_idx)?.effects.get(slot_idx)
    }

    pub fn slot_mut(&mut self, track_idx: usize, slot_idx: usize) -> Option<&mut mixer::DspSlot> {
        self.tracks_mut().get_mut(track_idx)?.effects.get_mut(slot_idx)
    }

    pub fn target_effects_mut(
        &mut self,
        target: arranger_view::StripTarget,
    ) -> Option<&mut Vec<mixer::DspSlot>> {
        use arranger_view::StripTarget;
        match target {
            StripTarget::Master => self.tracks_mut().first_mut().map(|t| &mut t.effects),
            StripTarget::Live(idx) => self.live_tracks.get_mut(idx + 1).map(|t| &mut t.effects),
            StripTarget::Studio(idx) => self.studio_tracks.get_mut(idx).map(|t| &mut t.effects),
            StripTarget::Fx(idx) => self.fx_tracks.get_mut(idx).map(|t| &mut t.effects),
        }
    }

    pub fn track_width(&self, target: arranger_view::StripTarget) -> f32 {
        use arranger_view::StripTarget;
        match target {
            StripTarget::Master => match self.mode {
                AppMode::OpenLive => self.live_tracks.first().map(|t| t.width).unwrap_or(self.arranger_track_width),
                AppMode::OpenStudio => self.studio_tracks.first().map(|t| t.width).unwrap_or(self.arranger_track_width),
            },
            StripTarget::Live(idx) => self.live_tracks.get(idx + 1).map(|t| t.width).unwrap_or(self.arranger_track_width),
            StripTarget::Studio(idx) => self.studio_tracks.get(idx).map(|t| t.width).unwrap_or(self.arranger_track_width),
            StripTarget::Fx(idx) => self.fx_tracks.get(idx).map(|t| t.width).unwrap_or(self.arranger_track_width),
        }
    }

    pub fn set_track_width(&mut self, target: arranger_view::StripTarget, width: f32) {
        use arranger_view::StripTarget;
        let clamped = width.clamp(
            arranger_view::MIN_TRACK_WIDTH,
            arranger_view::MAX_TRACK_WIDTH,
        );
        match target {
            StripTarget::Master => match self.mode {
                AppMode::OpenLive => {
                    if let Some(t) = self.live_tracks.first_mut() {
                        t.width = clamped;
                    }
                }
                AppMode::OpenStudio => {
                    if let Some(t) = self.studio_tracks.first_mut() {
                        t.width = clamped;
                    }
                }
            },
            StripTarget::Live(idx) => {
                if let Some(t) = self.live_tracks.get_mut(idx + 1) {
                    t.width = clamped;
                }
            }
            StripTarget::Studio(idx) => {
                if let Some(t) = self.studio_tracks.get_mut(idx) {
                    t.width = clamped;
                }
            }
            StripTarget::Fx(idx) => {
                if let Some(t) = self.fx_tracks.get_mut(idx) {
                    t.width = clamped;
                }
            }
        }
    }

    pub fn move_device(
        &mut self,
        from_target: arranger_view::StripTarget,
        from_slot: usize,
        to_target: arranger_view::StripTarget,
        to_slot: usize,
    ) {
        use arranger_view::StripTarget;

        self.dragging_device = None;

        let current_len = self.target_effects_mut(from_target).map(|e| e.len()).unwrap_or(0);
        if from_target == to_target && (from_slot == to_slot || (to_slot == usize::MAX && from_slot + 1 >= current_len)) {
            return;
        }

        let slot = {
            let Some(effects) = self.target_effects_mut(from_target) else { return; };
            if from_slot >= effects.len() { return; }
            effects.remove(from_slot)
        };

        let has_external = slot.external.is_some();

        let dest_inserted = {
            let Some(effects) = self.target_effects_mut(to_target) else {
                if let Some(orig) = self.target_effects_mut(from_target) {
                    orig.insert(from_slot.min(orig.len()), slot);
                }
                return;
            };
            let dest = to_slot.min(effects.len());
            effects.insert(dest, slot);
            dest
        };

        let from_mixer = match from_target {
            StripTarget::Master | StripTarget::Fx(_) => 0,
            StripTarget::Live(idx) => idx + 1,
            StripTarget::Studio(idx) => idx,
        };
        let to_mixer = match to_target {
            StripTarget::Master | StripTarget::Fx(_) => 0,
            StripTarget::Live(idx) => idx + 1,
            StripTarget::Studio(idx) => idx,
        };
        if self.selected_track_index == from_mixer && self.selected_slot_index == from_slot {
            self.selected_track_index = to_mixer;
            self.selected_slot_index = dest_inserted;
        }

        if has_external {
            let to_engine_track = |target: StripTarget| match target {
                StripTarget::Live(idx) => Some(idx),
                StripTarget::Studio(idx) => idx.checked_sub(1),
                StripTarget::Fx(_) => Some(15),
                _ => None,
            };
            if let (Some(f_tr), Some(t_tr)) = (to_engine_track(from_target), to_engine_track(to_target)) {
                self.audio_proxy.send(GuiCommand::MoveExternalPlugin {
                    from_track: f_tr,
                    from_slot,
                    to_track: t_tr,
                    to_slot: dest_inserted,
                });
            }
        }
    }

    pub fn request_external_params(
        &mut self,
        descriptor: &hikaru_plugin_host::PluginDescriptor,
    ) {
        if !self.param_probing.insert(descriptor.id.clone()) {
            return;
        }
        if let Some(slot) = self
            .tracks_mut()
            .iter_mut()
            .flat_map(|track| track.effects.iter_mut())
            .find(|slot| {
                slot.external
                    .as_ref()
                    .is_some_and(|ext| ext.id == descriptor.id)
            }) {
            slot.params_status = mixer::ExternalParamsStatus::Probing;
        }

        let desc = descriptor.clone();
        let tx = self.param_probe_tx.clone();
        std::thread::spawn(move || {
            let res = probe_external_params(&desc);
            let _ = tx.send(res);
        });
    }

    pub fn drain_param_probes(&mut self) {
        while let Ok(res) = self.param_probe_rx.try_recv() {
            self.param_probing.remove(&res.descriptor_id);
            for track in self.tracks_mut() {
                for slot in &mut track.effects {
                    if let Some(ref ext) = slot.external {
                        if ext.id == res.descriptor_id {
                            if let Some(ref err) = res.error {
                                slot.params_status = mixer::ExternalParamsStatus::Error(err.clone());
                            } else {
                                slot.cached_params = res
                                    .params
                                    .iter()
                                    .cloned()
                                    .map(|(info, val)| mixer::ExternalParamState {
                                        info,
                                        value: val,
                                        pending_to_engine: false,
                                    })
                                    .collect();
                                slot.params_status = mixer::ExternalParamsStatus::Ready;
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn apply_param_feedback(
        &mut self,
        feedback: Vec<crate::views::external_plugins_settings::ParamFeedbackEvent>,
    ) -> bool {
        let mut changed = false;
        for ev in feedback {
            for track in self.tracks_mut() {
                for slot in &mut track.effects {
                    if let Some(ref ext) = slot.external {
                        if ext.id == ev.descriptor_id {
                            for &(param_id, val) in &ev.changes {
                                if let Some(p) = slot.cached_params.iter_mut().find(|p| p.info.id == param_id) {
                                    p.value = val;
                                    changed = true;
                                }
                            }
                        }
                    }
                }
            }
        }
        changed
    }

    pub fn flush_pending_external_params(&mut self) {
        for (tr_idx, track) in self.tracks().iter().enumerate() {
            let Some(eng_track) = tr_idx.checked_sub(1) else { continue };
            for (sl_idx, slot) in track.effects.iter().enumerate() {
                for p in &slot.cached_params {
                    if p.pending_to_engine {
                        self.audio_proxy.send(GuiCommand::SetExternalPluginParam {
                            track_idx: eng_track,
                            slot_idx: sl_idx,
                            param_id: p.info.id,
                            value: p.value,
                        });
                    }
                }
            }
        }
        for track in self.tracks_mut() {
            for slot in &mut track.effects {
                for p in &mut slot.cached_params {
                    p.pending_to_engine = false;
                }
            }
        }
    }

    pub fn is_playing(&self) -> bool {
        self.transport.playback_state == TransportPlaybackState::Playing
    }

    /// Devuelve el frame actual del playhead interno del clip si `(track_idx, scene_idx)`
    /// está reproduciéndose activamente en el motor de audio.
    pub fn clip_playhead_frame(&self, track_idx: usize, scene_idx: usize) -> Option<u64> {
        let packed = self.track_clip_playhead.get(track_idx)?.load(Ordering::Relaxed);
        let (active_scene, frame) = hikaru_audio_engine::unpack_clip_playhead(packed)?;
        (active_scene == scene_idx).then_some(frame)
    }

    pub fn play(&mut self) {
        let is_studio = self.mode == AppMode::OpenStudio;
        self.audio_proxy.send(GuiCommand::SetAppMode(is_studio));
        self.sync_wavetables_to_engine();
        self.sync_samplers_to_engine();
        self.transport.playback_state = TransportPlaybackState::Playing;
        crate::views::hikaru_native_plugins::hikaru_openmodulation::on_transport_play(self.transport.sample_count);
        self.audio_proxy.send(GuiCommand::Play);

        if self.mode == AppMode::OpenLive {
            let any_slot_playing = self.matrix_state.grid.iter().any(|row| {
                row.iter().any(|s| s.state == matrix::SlotState::Playing)
            });

            // Si estamos en EditView o no hay ningún slot reproduciéndose,
            // aseguramos que el clip bajo edición se dispare de inmediato en el motor.
            if self.openlive_view == OpenLiveView::EditView || !any_slot_playing {
                let target_slot = self.matrix_state.selected_slot.or_else(|| {
                    let eng_idx = self.selected_track_index.saturating_sub(1);
                    if !self.matrix_state.grid.is_empty() {
                        let t = eng_idx.min(self.matrix_state.grid.len().saturating_sub(1));
                        self.matrix_state.selected_slot = Some((t, 0));
                        Some((t, 0))
                    } else {
                        None
                    }
                });

                if let Some((t_idx, s_idx)) = target_slot {
                    if self.matrix_state.grid.get(t_idx).and_then(|row| row.get(s_idx)).map_or(false, |slot| slot.clip.is_none()) {
                        matrix::create_empty_midi_clip(&mut self.matrix_state, &self.audio_proxy, t_idx, s_idx);
                    }
                    if !self.piano_roll_state.notes.is_empty() {
                        if let Some(slot) = self.matrix_state.slot_mut(t_idx, s_idx) {
                            if let Some(clip) = slot.clip.as_mut() {
                                clip.loop_start = self.piano_roll_state.loop_start_tick;
                                clip.loop_end = self.piano_roll_state.loop_end_tick;
                                let updated_notes: Vec<(u64, u8, u8, u32)> = self.piano_roll_state
                                    .notes
                                    .iter()
                                    .map(|n| (n.start_tick, n.pitch, n.velocity, n.duration_ticks as u32))
                                    .collect();
                                clip.content = matrix::ClipData::Midi { notes: updated_notes.clone() };
                                self.audio_proxy.send(GuiCommand::UpdateMidiClipNotes {
                                    track_idx: t_idx,
                                    scene_idx: s_idx,
                                    notes: updated_notes,
                                });
                                self.audio_proxy.send(GuiCommand::SetMidiClipLoop {
                                    track_idx: t_idx,
                                    scene_idx: s_idx,
                                    loop_start_ticks: self.piano_roll_state.loop_start_tick,
                                    loop_end_ticks: self.piano_roll_state.loop_end_tick,
                                });
                            }
                        }
                    }
                    if let Some(slot) = self.matrix_state.slot_mut(t_idx, s_idx) {
                        slot.state = matrix::SlotState::Playing;
                    }
                    self.audio_proxy.send(GuiCommand::StartClip {
                        track_idx: t_idx,
                        scene_idx: s_idx,
                    });
                }
            }
        }
    }

    pub fn pause(&mut self) {
        self.transport.playback_state = TransportPlaybackState::Paused;
        crate::views::hikaru_native_plugins::hikaru_openmodulation::on_transport_pause_or_stop();
        self.audio_proxy.send(GuiCommand::Pause);
    }

    pub fn stop(&mut self) {
        self.transport.playback_state = TransportPlaybackState::Stopped;
        self.transport.sample_count = 0;
        crate::views::hikaru_native_plugins::hikaru_openmodulation::on_transport_pause_or_stop();
        self.playlist_state.playhead_tick = 0;
        self.piano_roll_state.playhead_tick = self.piano_roll_state.loop_start_tick;
        self.piano_roll_state.prev_playhead_tick = self.piano_roll_state.playhead_tick;
        for row in self.matrix_state.grid.iter_mut() {
            for slot in row.iter_mut() {
                if slot.state == matrix::SlotState::Playing {
                    slot.state = matrix::SlotState::Stopped;
                }
            }
        }
        self.audio_proxy.send(GuiCommand::Stop);
    }

    pub fn toggle_play(&mut self) {
        if self.is_playing() {
            self.stop();
        } else {
            self.play();
        }
    }

    pub fn set_bpm(&mut self, bpm: f64) {
        let clamped = bpm.clamp(BPM_MIN, BPM_MAX);
        self.transport.bpm = clamped;
        self.bpm_input = format_bpm(clamped);
        self.audio_proxy.send(GuiCommand::SetBpm(clamped as f32));
    }

    /// Registra un tap de tiempo (en segundos monotónicos) y, a partir del
    /// segundo tap dentro de la ventana, actualiza el BPM del transporte y
    /// del motor de audio.
    pub fn tap_tempo(&mut self, now_secs: f64) -> Option<f64> {
        self.tap_tempo_enabled = true;
        if let Some(bpm) = self.tap_tempo.tap_at(now_secs) {
            self.set_bpm(bpm);
            Some(bpm)
        } else {
            None
        }
    }

    /// Inserta una escena en `scene_idx` manteniendo matriz, motor y
    /// `clip_editor` sincronizados.
    pub fn insert_scene_at(&mut self, scene_idx: usize) -> usize {
        let idx = self
            .matrix_state
            .insert_scene_synced(scene_idx, &self.audio_proxy);
        let shift_target = |t: &mut Option<detail_editor_panel::ClipEditorTarget>| {
            if let Some(detail_editor_panel::ClipEditorTarget::Matrix { slot_idx, .. }) = t {
                if *slot_idx >= idx {
                    *slot_idx += 1;
                }
            }
        };
        shift_target(&mut self.clip_editor.target);
        shift_target(&mut self.clip_editor.active_target);
        idx
    }

    /// Elimina la escena `scene_idx` manteniendo matriz, motor y `clip_editor`
    /// sincronizados (conserva siempre al menos 1 escena).
    pub fn remove_scene_at(&mut self, scene_idx: usize) -> bool {
        if !self
            .matrix_state
            .remove_scene_synced(scene_idx, &self.audio_proxy)
        {
            return false;
        }
        let adjust_target = |t: &mut Option<detail_editor_panel::ClipEditorTarget>| {
            if let Some(detail_editor_panel::ClipEditorTarget::Matrix { slot_idx, .. }) = t {
                if *slot_idx == scene_idx {
                    *t = None;
                } else if *slot_idx > scene_idx {
                    *slot_idx -= 1;
                }
            }
        };
        adjust_target(&mut self.clip_editor.target);
        adjust_target(&mut self.clip_editor.active_target);
        true
    }

    /// Renombra la escena `scene_idx`.
    pub fn rename_scene(&mut self, scene_idx: usize, new_name: impl Into<String>) -> bool {
        self.matrix_state.rename_scene(scene_idx, new_name)
    }

    /// Reordena la escena `from_idx` a `to_idx` manteniendo matriz, motor,
    /// `queued_scene_index` y `clip_editor` sincronizados.
    pub fn move_scene(&mut self, from_idx: usize, to_idx: usize) -> bool {
        if !self
            .matrix_state
            .move_scene_synced(from_idx, to_idx, &self.audio_proxy)
        {
            return false;
        }
        let remap = |idx: usize| -> usize {
            if idx == from_idx {
                to_idx
            } else if from_idx < to_idx && idx > from_idx && idx <= to_idx {
                idx - 1
            } else if to_idx < from_idx && idx >= to_idx && idx < from_idx {
                idx + 1
            } else {
                idx
            }
        };
        if let Some(q) = self.queued_scene_index {
            self.queued_scene_index = Some(remap(q));
        }
        let remap_target = |t: &mut Option<detail_editor_panel::ClipEditorTarget>| {
            if let Some(detail_editor_panel::ClipEditorTarget::Matrix { slot_idx, .. }) = t {
                *slot_idx = remap(*slot_idx);
            }
        };
        remap_target(&mut self.clip_editor.target);
        remap_target(&mut self.clip_editor.active_target);
        true
    }

    pub fn copy_scene_at(&mut self, scene_idx: usize) -> bool {
        if let Some(copied) = self.matrix_state.copy_scene(scene_idx) {
            self.matrix_clipboard.copy_scene(copied);
            true
        } else {
            false
        }
    }

    pub fn cut_scene_at(&mut self, scene_idx: usize) -> bool {
        if !self.copy_scene_at(scene_idx) {
            return false;
        }
        if self.matrix_state.scenes.len() > 1 {
            self.remove_scene_at(scene_idx)
        } else {
            self.matrix_state.clear_scene_clips(scene_idx);
            true
        }
    }

    pub fn paste_scene_at(&mut self, scene_idx: usize) -> bool {
        let Some(copied) = self.matrix_clipboard.copied_scene.clone() else {
            return false;
        };
        self.matrix_state
            .paste_scene(scene_idx, &copied, &self.audio_proxy)
    }

    pub fn duplicate_scene_at(&mut self, scene_idx: usize) -> Option<usize> {
        let copied = self.matrix_state.copy_scene(scene_idx)?;
        let new_idx = self.insert_scene_at(scene_idx + 1);
        self.matrix_state
            .paste_scene(new_idx, &copied, &self.audio_proxy);
        self.matrix_state.selected_scene = Some(new_idx);
        self.matrix_state.selected_slot = None;
        Some(new_idx)
    }

    pub fn set_sig(&mut self, num: u32, den: u32) {
        self.transport.beats_per_bar = num.max(1);
        self.transport.beat_division = den.max(1);
        self.sig_input = format_sig(num, den);
    }
}

pub fn set_footer_cues(
    state: &mut AppState,
    single_help: Option<(&str, &str)>,
    cues: &[(&str, &str)],
) {
    state.footer_context_help = single_help.map(|(a, d)| (a.to_string(), d.to_string()));
    state.footer_context_cues = cues
        .iter()
        .map(|&(a, d)| (a.to_string(), d.to_string()))
        .collect();
}

pub fn clear_footer_help(state: &mut AppState) {
    state.footer_context_help = None;
    state.footer_context_cues.clear();
}

pub fn toggle_central_view(state: &mut AppState) {
    match state.mode {
        AppMode::OpenLive => {
            state.openlive_view = match state.openlive_view {
                OpenLiveView::SessionMatrix => OpenLiveView::ArrangerView,
                OpenLiveView::ArrangerView => OpenLiveView::EditView,
                OpenLiveView::EditView => OpenLiveView::SessionMatrix,
            };
        }
        AppMode::OpenStudio => {
            state.openstudio_view = match state.openstudio_view {
                OpenStudioView::Playlist => OpenStudioView::ArrangerMixer,
                OpenStudioView::ArrangerMixer => OpenStudioView::EditView,
                OpenStudioView::EditView => OpenStudioView::Playlist,
            };
        }
    }
}

pub struct HikaruApp {
    pub state: AppState,
    pub _audio_stream: Option<cpal::Stream>,
    pub wavetable_viewport: WavetableViewportHandle,
}

impl HikaruApp {
    pub fn build(
        audio_proxy: AudioProxy,
        audio_stream: Option<cpal::Stream>,
        position_clock: Arc<AtomicU64>,
        output_level_bits: Arc<AtomicU32>,
        engine_handle: Option<Arc<Mutex<AudioEngine<'static>>>>,
    ) -> Self {
        let state = AppState::new(
            audio_proxy,
            position_clock,
            output_level_bits,
            engine_handle,
        );
        let wavetable_viewport = state.wavetable_viewport.clone();

        Self {
            state,
            _audio_stream: audio_stream,
            wavetable_viewport,
        }
    }

    pub fn sync_hardware_sample_rate(&mut self, hardware_sr: f32) {
        if hardware_sr > 0.0 {
            self.state.transport.sample_rate = SampleRate::new(hardware_sr);
        }
    }

    pub fn sync_frame(&mut self, ctx: &egui::Context) {
        self.state.plugin_settings_state.poll_updates();
        self.state.drain_param_probes();
        let param_feedback = self.state.plugin_settings_state.take_param_feedback();
        let _feedback_changed = self.state.apply_param_feedback(param_feedback);
        self.state.flush_pending_external_params();

        for plugin in self.state.active_external_plugins.iter_mut() {
            plugin.poll_editor_resize();
        }

        self.state.transport.sample_count = self.state.position_clock.load(Ordering::Relaxed);
        let pending_raw = self.state.pending_scene_index_atomic.load(Ordering::Relaxed);
        self.state.queued_scene_index = if pending_raw >= 0 {
            Some(pending_raw as usize)
        } else {
            None
        };
        self.state.playlist_state.ppqn = self.state.transport.ppqn();
        let raw_tick = self.state.transport.samples_to_ticks(self.state.transport.sample_count);
        self.state.playlist_state.playhead_tick = raw_tick;

        if self.state.is_playing() {
            let prs = &mut self.state.piano_roll_state;
            if prs.loop_enabled && prs.loop_end_tick > prs.loop_start_tick {
                let loop_len = (prs.loop_end_tick - prs.loop_start_tick).max(240);
                prs.playhead_tick = if raw_tick >= prs.loop_start_tick {
                    prs.loop_start_tick + ((raw_tick - prs.loop_start_tick) % loop_len)
                } else {
                    raw_tick
                };
            } else {
                prs.playhead_tick = raw_tick;
            }
        }

        // Smooth peaks
        let raw_master = f32::from_bits(self.state.output_level_bits.load(Ordering::Relaxed));
        self.state.smoothed_master_peak = mixer::smooth_peak(self.state.smoothed_master_peak, raw_master);
        for (i, slot) in self.state.smoothed_track_peaks.iter_mut().enumerate() {
            if let Some(bits) = self.state.track_peak_bits.get(i) {
                let raw = f32::from_bits(bits.load(Ordering::Relaxed));
                *slot = mixer::smooth_peak(*slot, raw);
            }
        }

        if (self.state.transport.bpm - self.state.bpm_synced_to_engine).abs() > f64::EPSILON {
            self.state.bpm_synced_to_engine = self.state.transport.bpm;
            self.state.audio_proxy.send(GuiCommand::SetBpm(self.state.transport.bpm as f32));
        }

        let has_active_lfo = self.state.current_tracks().iter().any(|tr| {
            tr.effects.iter().any(|slot| {
                slot.active
                    && matches!(
                        slot.name.as_str(),
                        "OpenLFO MOD"
                            | "Hikaru OpenLFO MOD"
                            | "LFO MOD"
                            | "OpenModulation"
                            | "Hikaru OpenModulation"
                    )
            })
        });

        if self.state.is_playing() || has_active_lfo {
            ctx.request_repaint();
        }

        self.state.sync_wavetables_to_engine();
        self.state.sync_samplers_to_engine();

        if !self.state.is_playing() {
            let piano_tracks = self.state.tracks().clone();
            let engine_tr = self.state.selected_track_index.saturating_sub(1);
            piano_roll::trigger_playhead_notes(
                &mut self.state.piano_roll_state,
                &piano_tracks,
                engine_tr,
                &self.state.audio_proxy,
            );
        } else {
            self.state.piano_roll_state.prev_playhead_tick = self.state.piano_roll_state.playhead_tick;
        }

        if self.state.show_piano_roll {
            let needs_frame = piano_roll::sync_frame(&mut self.state.piano_roll_state);
            if needs_frame {
                ctx.request_repaint();
            }
        }
    }
}

impl eframe::App for HikaruApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        theme::apply_theme(ctx);
        self.state.wavetable_viewport.spawn_connect(Some(ctx.clone()));
        self.sync_frame(ctx);

        // Global Keyboard shortcuts
        // IMPORTANT: query `ctx.wants_keyboard_input()` OUTSIDE `ctx.input(...)`
        // to avoid re-entrant egui Context lock deadlock!
        let wants_keyboard_input = ctx.wants_keyboard_input();
        let mut pending_file_action: Option<crate::ui::file_menu::FileMenuAction> = None;
        ctx.input(|i| {
            if i.key_pressed(egui::Key::Space) {
                self.state.toggle_play();
            }
            if i.key_pressed(egui::Key::Tab) || i.key_pressed(egui::Key::F9) {
                toggle_central_view(&mut self.state);
            }
            if i.key_pressed(egui::Key::F10) {
                self.state.show_dsp_rack = !self.state.show_dsp_rack;
            }
            if i.key_pressed(egui::Key::F11) {
                self.state.show_explorer = !self.state.show_explorer;
            }
            if (i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
                && !wants_keyboard_input
                && !self.state.editing_bpm
                && !self.state.editing_sig
                && self.state.open_grid_editor.is_none()
            {
                if let Some(scene_idx) = self.state.matrix_state.selected_scene {
                    self.state.remove_scene_at(scene_idx);
                } else if self.state.show_dsp_rack {
                    dsp_rack::handle_delete_key(&mut self.state);
                } else if !self.state.playlist_state.selected_clips.is_empty() {
                    crate::ui::edit_menu::execute_action(
                        &mut self.state,
                        crate::ui::edit_menu::EditMenuAction::Delete,
                    );
                } else if let Some(fx_idx) = self.state.selected_fx_index {
                    self.state.delete_fx_track(fx_idx);
                } else if self.state.selected_track_index > 0 {
                    self.state.delete_track(self.state.selected_track_index);
                }
            }
            let cmd = i.modifiers.ctrl || i.modifiers.command;
            if cmd && !wants_keyboard_input {
                use crate::ui::file_menu::FileMenuAction;
                if i.modifiers.shift && i.key_pressed(egui::Key::S) {
                    pending_file_action = Some(FileMenuAction::SaveAs);
                } else if i.modifiers.shift && i.key_pressed(egui::Key::B) {
                    pending_file_action = Some(FileMenuAction::ExportAudio);
                } else if !i.modifiers.alt && !i.modifiers.shift && i.key_pressed(egui::Key::N) {
                    pending_file_action = Some(FileMenuAction::NewProject);
                } else if !i.modifiers.alt && !i.modifiers.shift && i.key_pressed(egui::Key::O) {
                    pending_file_action = Some(FileMenuAction::Open);
                } else if !i.modifiers.alt && !i.modifiers.shift && i.key_pressed(egui::Key::S) {
                    pending_file_action = Some(FileMenuAction::Save);
                } else if !i.modifiers.alt && !i.modifiers.shift && i.key_pressed(egui::Key::W) {
                    pending_file_action = Some(FileMenuAction::Close);
                } else if !i.modifiers.alt && !i.modifiers.shift && i.key_pressed(egui::Key::Comma) {
                    pending_file_action = Some(FileMenuAction::Settings);
                } else if !i.modifiers.alt && !i.modifiers.shift && i.key_pressed(egui::Key::Q) {
                    pending_file_action = Some(FileMenuAction::Quit);
                }
            }
            if cmd && !wants_keyboard_input {
                use crate::ui::add_menu::AddMenuAction;
                if i.modifiers.alt && !i.modifiers.shift && i.key_pressed(egui::Key::T) {
                    crate::ui::add_menu::execute_action(&mut self.state, AddMenuAction::AddFxTrack);
                } else if i.modifiers.shift && !i.modifiers.alt && i.key_pressed(egui::Key::T) {
                    crate::ui::add_menu::execute_action(&mut self.state, AddMenuAction::AddAudioTrack);
                } else if !i.modifiers.shift && !i.modifiers.alt && i.key_pressed(egui::Key::T) {
                    crate::ui::add_menu::execute_action(
                        &mut self.state,
                        AddMenuAction::AddInstrumentTrack,
                    );
                } else if i.modifiers.alt && !i.modifiers.shift && i.key_pressed(egui::Key::G) {
                    crate::ui::add_menu::execute_action(&mut self.state, AddMenuAction::AddGroupTrack);
                } else if i.modifiers.shift && !i.modifiers.alt && i.key_pressed(egui::Key::I) {
                    crate::ui::add_menu::execute_action(
                        &mut self.state,
                        AddMenuAction::AddSceneFromPlayingClips,
                    );
                } else if !i.modifiers.shift && !i.modifiers.alt && i.key_pressed(egui::Key::I) {
                    crate::ui::add_menu::execute_action(&mut self.state, AddMenuAction::AddScene);
                }
            }
            if cmd && !wants_keyboard_input {
                if let Some(scene_idx) = self.state.matrix_state.selected_scene {
                    if !i.modifiers.shift && !i.modifiers.alt && i.key_pressed(egui::Key::C) {
                        self.state.copy_scene_at(scene_idx);
                    } else if !i.modifiers.shift && !i.modifiers.alt && i.key_pressed(egui::Key::X) {
                        self.state.cut_scene_at(scene_idx);
                    } else if !i.modifiers.shift && !i.modifiers.alt && i.key_pressed(egui::Key::V) {
                        self.state.paste_scene_at(scene_idx);
                    } else if !i.modifiers.shift && !i.modifiers.alt && i.key_pressed(egui::Key::D) {
                        self.state.duplicate_scene_at(scene_idx);
                    }
                }
            }
            if cmd && !wants_keyboard_input {
                use crate::ui::edit_menu::EditMenuAction;
                if i.modifiers.shift && !i.modifiers.alt && i.key_pressed(egui::Key::Z) {
                    crate::ui::edit_menu::execute_action(&mut self.state, EditMenuAction::Redo);
                } else if !i.modifiers.shift && !i.modifiers.alt && i.key_pressed(egui::Key::Z) {
                    crate::ui::edit_menu::execute_action(&mut self.state, EditMenuAction::Undo);
                } else if i.modifiers.shift && !i.modifiers.alt && i.key_pressed(egui::Key::A) {
                    crate::ui::edit_menu::execute_action(
                        &mut self.state,
                        EditMenuAction::DeselectAll,
                    );
                }
            }
            if cmd && !wants_keyboard_input && self.state.panel_layout.show_arranger && self.state.matrix_state.selected_scene.is_none() {
                use crate::ui::edit_menu::EditMenuAction;
                if !i.modifiers.shift && !i.modifiers.alt && i.key_pressed(egui::Key::C) && !self.state.playlist_state.selected_clips.is_empty() {
                    crate::ui::edit_menu::execute_action(&mut self.state, EditMenuAction::Copy);
                } else if !i.modifiers.shift && !i.modifiers.alt && i.key_pressed(egui::Key::X) && !self.state.playlist_state.selected_clips.is_empty() {
                    crate::ui::edit_menu::execute_action(&mut self.state, EditMenuAction::Cut);
                } else if !i.modifiers.shift && !i.modifiers.alt && i.key_pressed(egui::Key::V) && !self.state.playlist_state.clipboard.is_empty() {
                    crate::ui::edit_menu::execute_action(&mut self.state, EditMenuAction::Paste);
                } else if !i.modifiers.shift && !i.modifiers.alt && i.key_pressed(egui::Key::D) && !self.state.playlist_state.selected_clips.is_empty() {
                    crate::ui::edit_menu::execute_action(&mut self.state, EditMenuAction::Duplicate);
                } else if !i.modifiers.shift && !i.modifiers.alt && i.key_pressed(egui::Key::A) {
                    crate::ui::edit_menu::execute_action(&mut self.state, EditMenuAction::SelectAll);
                } else if !i.modifiers.shift && !i.modifiers.alt && i.key_pressed(egui::Key::E) && !self.state.playlist_state.selected_clips.is_empty() {
                    let split_tick = self.state.playlist_state.playhead_tick;
                    playlist::split_selected_clips_at(&mut self.state.playlist_state, split_tick);
                }
            }
            if !wants_keyboard_input {
                use crate::ui::clip_menu::ClipMenuAction;
                if cmd && i.modifiers.shift && i.modifiers.alt && i.key_pressed(egui::Key::L) {
                    crate::ui::clip_menu::execute_action(
                        &mut self.state,
                        ClipMenuAction::MakeLegato,
                    );
                } else if cmd && !i.modifiers.shift && !i.modifiers.alt && i.key_pressed(egui::Key::L)
                {
                    crate::ui::clip_menu::execute_action(
                        &mut self.state,
                        ClipMenuAction::LoopSelectedRegion,
                    );
                } else if cmd && !i.modifiers.shift && !i.modifiers.alt && i.key_pressed(egui::Key::J)
                {
                    crate::ui::clip_menu::execute_action(
                        &mut self.state,
                        ClipMenuAction::Consolidate,
                    );
                } else if !cmd && !i.modifiers.shift && !i.modifiers.alt && i.key_pressed(egui::Key::Q)
                {
                    crate::ui::clip_menu::execute_action(&mut self.state, ClipMenuAction::Quantize);
                } else if !cmd && i.modifiers.alt && i.modifiers.shift
                    && (i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals))
                {
                    crate::ui::clip_menu::execute_action(
                        &mut self.state,
                        ClipMenuAction::TransposeOctaveUp,
                    );
                } else if !cmd && i.modifiers.alt && i.modifiers.shift
                    && i.key_pressed(egui::Key::Minus)
                {
                    crate::ui::clip_menu::execute_action(
                        &mut self.state,
                        ClipMenuAction::TransposeOctaveDown,
                    );
                } else if !cmd && i.modifiers.alt && !i.modifiers.shift
                    && (i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals))
                {
                    crate::ui::clip_menu::execute_action(
                        &mut self.state,
                        ClipMenuAction::TransposeSemitoneUp,
                    );
                } else if !cmd && i.modifiers.alt && !i.modifiers.shift
                    && i.key_pressed(egui::Key::Minus)
                {
                    crate::ui::clip_menu::execute_action(
                        &mut self.state,
                        ClipMenuAction::TransposeSemitoneDown,
                    );
                }
            }
        });
        if let Some(action) = pending_file_action {
            crate::ui::file_menu::execute_action(ctx, &mut self.state, action);
        }

        // 1. TOP PANEL: Unified Bitwig 5.1.7 Transport & Application Header
        egui::TopBottomPanel::top("top_panel")
            .frame(egui::Frame::NONE.fill(theme::DAW_BG_LAYER_2))
            .show(ctx, |ui| {
                header::render(ui, &mut self.state);
            });

        // 2. BOTTOM PANEL: Footer status & panel bar
        egui::TopBottomPanel::bottom("bottom_footer")
            .frame(egui::Frame::NONE.fill(theme::DAW_BG_LAYER_2))
            .show(ctx, |ui| {
                footer::render(ui, &mut self.state);
            });

        // 3. RIGHT PANEL: Bitwig-style Browser (Samples, Files & Presets)
        if self.state.show_explorer {
            egui::SidePanel::right("right_explorer")
                .default_width(270.0)
                .min_width(210.0)
                .max_width(420.0)
                .frame(
                    egui::Frame::NONE
                        .fill(theme::DAW_PANEL_BG)
                        .inner_margin(egui::Margin::symmetric(6, 4))
                        .stroke(egui::Stroke::new(1.0_f32, theme::DAW_BORDER_SUBTLE)),
                )
                .show(ctx, |ui| {
                    explorer::render(ui, &mut self.state);
                });
        }

        // 4. BOTTOM DOCK PANELS: Clip Editor / Piano Roll / DSP Rack / Mix Panel
        let is_edit_view = match self.state.mode {
            AppMode::OpenLive => self.state.openlive_view == OpenLiveView::EditView,
            AppMode::OpenStudio => self.state.openstudio_view == OpenStudioView::EditView,
        };

        let has_dock = if is_edit_view {
            // En vista EDIT (1:1 Bitwig Studio), el dock inferior se activa exclusivamente
            // al seleccionar Show Device Panel o Show Mixer Panel en el footer.
            self.state.show_dsp_rack || self.state.panel_layout.show_mixer
        } else {
            self.state.clip_editor.show_editor
                || self.state.show_piano_roll
                || self.state.show_dsp_rack
                || self.state.panel_layout.show_mixer
        };

        if has_dock {
            let dock_h = if self.state.show_dsp_rack {
                dsp_rack::RACK_HEIGHT + 6.0
            } else if self.state.panel_layout.show_mixer {
                self.state.bottom_mixer_height.max(260.0)
            } else if self.state.show_piano_roll {
                self.state.clip_editor.editor_height.max(260.0)
            } else if self.state.clip_editor.show_editor {
                self.state.clip_editor.editor_height.max(200.0)
            } else {
                self.state.bottom_mixer_height
            };

            egui::TopBottomPanel::bottom("bottom_dock")
                .exact_height(dock_h)
                .frame(egui::Frame::NONE.fill(theme::DAW_BG_LAYER_1).stroke(egui::Stroke::new(1.0_f32, theme::DAW_BORDER_SUBTLE)))
                .show(ctx, |ui| {
                    if self.state.show_dsp_rack {
                        dsp_rack::render(ui, &mut self.state);
                    } else if self.state.panel_layout.show_mixer {
                        mixer::render_bottom_panel(ui, &mut self.state);
                    } else if self.state.show_piano_roll || self.state.clip_editor.show_editor {
                        render_edit_or_audio(ui, &mut self.state);
                    }
                });
        }

        // 5. CENTRAL WORKSPACE PANEL
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(theme::DAW_CANVAS_VOID))
            .show(ctx, |ui| {
                if let Some((grid_tr, grid_sl)) = self.state.open_grid_editor {
                    crate::views::hikaru_native_plugins::hikaru_opengrid::render_editor_panel(
                        ui,
                        &mut self.state,
                        grid_tr,
                        grid_sl,
                    );
                } else {
                    match self.state.mode {
                        AppMode::OpenLive => match self.state.openlive_view {
                            OpenLiveView::SessionMatrix => matrix::render(ui, &mut self.state),
                            OpenLiveView::ArrangerView => arranger_view::render(ui, &mut self.state),
                            OpenLiveView::EditView => {
                                render_edit_or_audio(ui, &mut self.state);
                            }
                        },
                        AppMode::OpenStudio => match self.state.openstudio_view {
                            OpenStudioView::Playlist => playlist::render(ui, &mut self.state),
                            OpenStudioView::ArrangerMixer => arranger_view::render(ui, &mut self.state),
                            OpenStudioView::EditView => {
                                render_edit_or_audio(ui, &mut self.state);
                            }
                        },
                    }
                }
            });

        // 6. Floating Drag-and-Drop Sample Preview Badge & Cleanup on Release
        if let Some(ref sample_path) = self.state.dragged_sample.clone() {
            if let Some(pos) = ctx.input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos())) {
                let file_label = sample_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let layer_id = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("wav_drag_badge"));
                let painter = ctx.layer_painter(layer_id);
                let badge_rect = egui::Rect::from_min_size(
                    egui::pos2(pos.x + 14.0, pos.y + 10.0),
                    egui::vec2(198.0, 22.0),
                );
                painter.rect_filled(badge_rect, 3.0, egui::Color32::from_rgba_unmultiplied(18, 20, 24, 240));
                painter.rect_stroke(
                    badge_rect,
                    3.0,
                    egui::Stroke::new(1.5_f32, theme::ACCENT_ORANGE),
                    egui::StrokeKind::Inside,
                );
                crate::ui::icons::paint_browser_badge(
                    &painter,
                    egui::Rect::from_min_size(
                        egui::pos2(badge_rect.min.x + 4.0, badge_rect.min.y + 3.0),
                        egui::vec2(16.0, 16.0),
                    ),
                    crate::ui::icons::BrowserBadgeKind::Audio,
                );
                painter.text(
                    egui::pos2(badge_rect.min.x + 24.0, badge_rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    file_label,
                    crate::ui::fonts::semibold(10.5),
                    egui::Color32::WHITE,
                );
            }
            if ctx.input(|i| i.pointer.any_released() || !i.pointer.any_down()) {
                self.state.dragged_sample = None;
            }
        }

        // 6. POPUPS & MODALS
        about::render(ctx, &mut self.state);
        audio_settings::render(ctx, &mut self.state);
        external_plugins_settings::render(ctx, &mut self.state);
        mcp_port_settings::render(ctx, &mut self.state);
        control_panel::render(ctx, &mut self.state);
        popup_browser::render(ctx, &mut self.state);
    }
}

/// Editor de la pestaña ARRANGE/EDIT: Detail Editor de audio si el clip seleccionado
/// es audio; Piano Roll en cualquier otro caso.
fn render_edit_or_audio(ui: &mut egui::Ui, state: &mut AppState) {
    if let Some(target) = audio_detail_editor::selected_audio_target(state) {
        audio_detail_editor::render(ui, state, target);
    } else {
        piano_roll::render(ui, state);
    }
}
