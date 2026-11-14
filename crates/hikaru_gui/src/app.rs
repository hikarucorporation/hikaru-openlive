// Hikaru OpenLive - App (Immediate-mode egui + eframe)
// GNU AGPLv3
// crates/hikaru_gui/src/app.rs

use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::path::PathBuf;

use hikaru_audio_engine::AudioEngine;
use hikaru_core::SampleRate;
use hikaru_transport::{TransportPlaybackState, TransportPosition};
use hikaru_plugin_host::PluginInstance;

use crate::audio_proxy::{AudioProxy, GuiCommand};
use crate::render::WavetableViewportHandle;
use crate::theme;
use crate::views::{
    about, arranger_view, audio_detail_editor, audio_settings, clip_editor, detail_editor_panel, control_panel, dsp_rack, explorer,
    external_plugins_settings, footer,
    header, matrix, mcp_port_settings, mixer, modal, piano_roll, playlist,
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
    pub punch_in: bool,
    pub punch_out: bool,
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
    pub selected_slot_index: usize,
    pub footer_context_help: Option<(String, String)>,
    pub footer_context_cues: Vec<(String, String)>,
    pub active_modifiers: egui::Modifiers,
    pub add_slot_menu_open: bool,
    pub plugin_browser_query: String,
    pub plugin_browser_filter: dsp_rack::PluginBrowserFilter,
    pub plugin_search_input: String,
    pub param_drag: Option<ParamDragState>,
    pub param_probe_tx: Sender<ParamProbeResult>,
    pub param_probe_rx: Receiver<ParamProbeResult>,
    pub param_probing: HashSet<String>,

    pub track_peak_bits: Vec<Arc<AtomicU32>>,
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
        let fx_tracks = vec![mixer::Track::new(0, "FX 1".to_string(), false)];

        let track_peak_bits: Vec<Arc<AtomicU32>> = if let Some(ref handle) = engine_handle {
            if let Ok(engine) = handle.try_lock() {
                engine.track_peak_bits.iter().map(|a| Arc::clone(a)).collect()
            } else {
                (0..16)
                    .map(|_| Arc::new(AtomicU32::new(0.0f32.to_bits())))
                    .collect()
            }
        } else {
            (0..16)
                .map(|_| Arc::new(AtomicU32::new(0.0f32.to_bits())))
                .collect()
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
            punch_in: false,
            punch_out: false,
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
            clip_editor: clip_editor::ClipEditorState::default(),
            matrix_state,
            matrix_clipboard: matrix::MatrixClipboard::default(),
            dragged_sample: None,
            pending_wavetable_slot: None,
            live_tracks,
            studio_tracks,
            fx_tracks,
            selected_track_index: 1,
            selected_slot_index: 0,
            footer_context_help: None,
            footer_context_cues: Vec::new(),
            active_modifiers: egui::Modifiers::default(),
            add_slot_menu_open: false,
            plugin_browser_query: String::new(),
            plugin_browser_filter: dsp_rack::PluginBrowserFilter::default(),
            plugin_search_input: String::new(),
            param_drag: None,
            param_probe_tx,
            param_probe_rx,
            param_probing: HashSet::new(),
            track_peak_bits,
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
        };
        state.audio_proxy.send(GuiCommand::SetAppMode(false));
        state
    }

    pub fn sync_wavetables_to_engine(&mut self) {
        let is_playing = self.transport.playback_state == hikaru_transport::TransportPlaybackState::Playing;
        crate::views::hikaru_native_plugins::hikaru_openmodulation::update_global_transport(
            self.transport.bpm as f32,
            self.transport.sample_count,
            self.transport.sample_rate.get().max(1.0),
            is_playing,
        );

        let is_openstudio = self.mode == AppMode::OpenStudio;
        let tracks = if is_openstudio {
            &mut self.studio_tracks
        } else {
            &mut self.live_tracks
        };

        for (tr_idx, track) in tracks.iter_mut().enumerate() {
            let Some(eng_track) = tr_idx.checked_sub(1) else { continue };
            if eng_track >= 16 { continue; }

            let open_wt_slot = track.effects.iter_mut().find(|slot| slot.active && slot.name == "OpenWavetable");
            if let Some(slot) = open_wt_slot {
                // If WT POS is modulated by OpenModulation, apply live modulation
                if let Some(m) = crate::views::hikaru_native_plugins::hikaru_openmodulation::get_modulation(
                    tr_idx,
                    "OpenWavetable",
                    "WT POS",
                ) {
                    let base = slot.wavetable.base_position.unwrap_or_else(|| slot.wavetable.position());
                    let effective = (base + m.delta).clamp(0.0, 1.0);
                    slot.wavetable.set_position(effective);
                }

                let pitch_offset = slot.wavetable.octave * 12 + slot.wavetable.pitch;
                let vol_mod = crate::views::hikaru_native_plugins::hikaru_openmodulation::get_modulation(
                    tr_idx,
                    "OpenWavetable",
                    "VOL",
                );
                let effective_vol = if let Some(m) = vol_mod {
                    (slot.wavetable.volume + m.delta).clamp(0.0, 2.0)
                } else {
                    slot.wavetable.volume
                };
                let vol_bits = effective_vol.to_bits();

                let fp: u64 = (slot.wavetable.frame as u64)
                    ^ (((slot.wavetable.morph * 10000.0) as u64) << 16)
                    ^ ((slot.wavetable.table.samples.len() as u64) << 32)
                    ^ ((slot.wavetable.smooth as u64) << 48)
                    ^ ((slot.wavetable.table.frames as u64) << 52);

                if self.synced_wavetable_fingerprints[eng_track] != Some((fp, pitch_offset, vol_bits)) {
                    let samples = slot.wavetable.current_samples();
                    self.audio_proxy.send(GuiCommand::SetTrackWavetable {
                        track_idx: eng_track,
                        samples,
                        pitch_offset,
                        volume: effective_vol,
                    });
                    self.synced_wavetable_fingerprints[eng_track] = Some((fp, pitch_offset, vol_bits));
                }
            } else if self.synced_wavetable_fingerprints[eng_track].is_some() {
                self.audio_proxy.send(GuiCommand::SetTrackWavetable {
                    track_idx: eng_track,
                    samples: Vec::new(),
                    pitch_offset: 0,
                    volume: 1.0,
                });
                self.synced_wavetable_fingerprints[eng_track] = None;
            }
        }
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

    pub fn current_tracks(&self) -> &Vec<mixer::Track> {
        self.tracks()
    }

    pub fn current_tracks_mut(&mut self) -> &mut Vec<mixer::Track> {
        self.tracks_mut()
    }

    pub fn safe_track_index(&self) -> usize {
        self.selected_track_index.min(self.tracks().len().saturating_sub(1))
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
                slot.active && matches!(slot.name.as_str(), "OpenModulation" | "Hikaru OpenModulation" | "LFO MOD")
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
            if i.key_pressed(egui::Key::Delete) {
                if self.state.show_dsp_rack {
                    dsp_rack::handle_delete_key(&mut self.state);
                } else if !self.state.playlist_state.selected_clips.is_empty() {
                    let selected = self.state.playlist_state.selected_clips.clone();
                    self.state.playlist_state.clips.retain(|(_, c)| !selected.contains(&c.id));
                    self.state.playlist_state.selected_clips.clear();
                } else if self.state.selected_track_index > 0 {
                    self.state.delete_track(self.state.selected_track_index);
                }
            }
        });

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
                    } else if self.state.show_piano_roll {
                        render_edit_or_audio(ui, &mut self.state);
                    } else if self.state.clip_editor.show_editor {
                        detail_editor_panel::render(ui, &mut self.state);
                    }
                });
        }

        // 5. CENTRAL WORKSPACE PANEL
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(theme::DAW_CANVAS_VOID))
            .show(ctx, |ui| {
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
    }
}

/// Editor de la pestaña ARRANGE/EDIT: Detail Editor de audio si el clip seleccionado
/// es audio; Piano Roll en cualquier otro caso.
fn render_edit_or_audio(ui: &mut egui::Ui, state: &mut AppState) {
    if let Some(slot) = audio_detail_editor::selected_audio_slot(state) {
        audio_detail_editor::render(ui, state, slot);
    } else {
        piano_roll::render(ui, state);
    }
}
