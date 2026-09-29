// Hikaru OpenLive - App
// GNU AGPLv3
// crates/hikaru_gui/src/app.rs

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::path::{Path, PathBuf};

use gpui_kit::component::*;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::label::Label;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use hikaru_audio_engine::AudioEngine;
use hikaru_core::SampleRate;
use hikaru_transport::{TransportPlaybackState, TransportPosition};
use hikaru_plugin_host::{spawn_floating_gui, PluginFormat, PluginInstance};

use crate::audio_proxy::{AudioProxy, GuiCommand};
use crate::views::{
    about, arranger_view, audio_settings, dsp_rack, explorer, external_plugins_settings, footer,
    header, matrix, menu_bar, mixer, piano_roll, playlist,
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
pub enum OpenLiveView {
    SessionMatrix,
    ArrangerView,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanMode {
    Stereo,
    MidSide,
}

/// Rango del BPM del transport. El motor descarta valores `<= 0.0`
/// (`TransportPosition::set_bpm`) y toda la matemática de ticks asume un
/// tempo positivo, así que la caja numérica se acota aquí.
pub const BPM_MIN: f64 = 40.0;
pub const BPM_MAX: f64 = 300.0;

/// Texto de la caja numérica de BPM: enteros sin decimales, fracciones con
/// los ceros sobrantes recortados (`140`, `87.5`).
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

pub struct AppState {
    pub mode: AppMode,
    pub transport: TransportPosition,
    pub position_clock: Arc<AtomicU64>,
    pub output_level_bits: Arc<AtomicU32>,
    pub audio_proxy: AudioProxy,
    pub is_looping: bool,
    pub cpu_usage: f32,
    pub bpm_synced_to_engine: f64,
    /// Caja numérica de BPM del transport. Es la fuente de la edición del
    /// usuario; `transport.bpm` sigue siendo el valor con el que trabaja el
    /// resto de la app.
    pub bpm_input: Entity<InputState>,
    pub global_loop_synced_to_engine: Option<(bool, u64, u64)>,
    pub show_mixer: bool,
    pub show_dsp_rack: bool,
    pub show_about: bool,
    pub is_recording: bool,
    pub show_explorer: bool,
    pub show_piano_roll: bool,
    pub piano_roll_state: piano_roll::PianoRollState,
    pub explorer_state: explorer::FileExplorerState,

    pub audio_settings_state: audio_settings::AudioSettingsState,
    pub plugin_settings_state: external_plugins_settings::PluginSettingsState,
    pub playlist_state: playlist::PlaylistState,
    pub matrix_state: matrix::SessionMatrixState,
    pub matrix_clipboard: matrix::MatrixClipboard,
    pub dragged_sample: Option<PathBuf>,

    pub live_tracks: Vec<mixer::Track>,
    pub studio_tracks: Vec<mixer::Track>,

    pub selected_track_index: usize,
    pub selected_slot_index: usize,

    pub track_peak_bits: Vec<Arc<AtomicU32>>,
    pub smoothed_track_peaks: [f32; 16],
    pub smoothed_master_peak: f32,

    pub openlive_view: OpenLiveView,
    pub arranger_fader_drag: Option<arranger_view::FaderTarget>,

    pub active_external_plugins: Vec<Box<dyn PluginInstance>>,
    pub engine_handle: Option<Arc<Mutex<AudioEngine<'static>>>>,
}

#[derive(Clone)]
pub struct AppStateHandle(pub Entity<AppState>);

impl Global for AppStateHandle {}

pub fn state(cx: &App) -> Entity<AppState> {
    cx.global::<AppStateHandle>().0.clone()
}

pub fn update_state(cx: &mut App, f: impl FnOnce(&mut AppState)) {
    let state = state(cx);
    let id = state.entity_id();
    state.update(cx, |s, _| f(s));
    cx.notify(id);
}

pub struct HikaruApp {
    pub state: Entity<AppState>,
    pub focus_handle: FocusHandle,
    pub _audio_stream: Option<cpal::Stream>,
    /// Mantiene viva la suscripción a `InputEvent` de la caja de BPM.
    _bpm_input_sub: Subscription,
}

pub fn handle_global_key(key: &str, cx: &mut App) {
    match key {
        "tab" => {
            update_state(cx, |state| match state.mode {
                AppMode::OpenLive => {
                    state.openlive_view = match state.openlive_view {
                        OpenLiveView::SessionMatrix => OpenLiveView::ArrangerView,
                        OpenLiveView::ArrangerView => OpenLiveView::SessionMatrix,
                    };
                }
                AppMode::OpenStudio => {
                    state.mode = AppMode::OpenLive;
                }
            });
        }
        "f9" => {
            update_state(cx, |state| {
                state.show_mixer = !state.show_mixer;
            });
        }
        "f10" => {
            update_state(cx, |state| {
                state.show_dsp_rack = !state.show_dsp_rack;
            });
        }
        "f11" => {
            update_state(cx, |state| {
                state.show_explorer = !state.show_explorer;
            });
        }
        "delete" | "backspace" => {
            update_state(cx, |state| {
                if !state.playlist_state.selected_clips.is_empty() {
                    state.playlist_state.clips.retain(|(_, c)| {
                        !state.playlist_state.selected_clips.contains(&c.id)
                    });
                    state.playlist_state.selected_clips.clear();
                }
            });
        }
        _ => {}
    }
}

impl HikaruApp {
    pub fn build(
        window: &mut Window,
        cx: &mut Context<HikaruApp>,
        audio_proxy: AudioProxy,
        audio_stream: Option<cpal::Stream>,
        position_clock: Arc<AtomicU64>,
        output_level_bits: Arc<AtomicU32>,
        engine_handle: Option<Arc<Mutex<AudioEngine<'static>>>>,
    ) -> HikaruApp {
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

        // Caja numérica de BPM (el sustituto moderno del `egui::DragValue` que
        // usaba la UI legacy): se escribe con el teclado, con las flechas
        // arriba/abajo o con los botones +/-, y se acota al perder el foco.
        let initial_bpm = transport.bpm;
        let bpm_input = cx.new(|cx| {
            InputState::new(window, cx)
                .step(1.0)
                .min(BPM_MIN)
                .max(BPM_MAX)
        });
        bpm_input.update(cx, |input, cx| {
            input.set_value(format_bpm(initial_bpm), window, cx)
        });

        let state = cx.new(|_| AppState {
            mode: AppMode::OpenLive,
            transport,
            position_clock,
            output_level_bits,
            audio_proxy,
            is_looping: false,
            cpu_usage: 0.12,
            bpm_synced_to_engine: -1.0,
            bpm_input: bpm_input.clone(),
            global_loop_synced_to_engine: None,
            show_mixer: false,
            show_dsp_rack: false,
            show_about: false,
            is_recording: false,
            show_explorer: false,
            show_piano_roll: false,
            piano_roll_state: piano_roll::PianoRollState::default(),
            explorer_state: explorer::FileExplorerState::default(),
            audio_settings_state: audio_settings::AudioSettingsState::default(),
            plugin_settings_state: external_plugins_settings::PluginSettingsState::default(),
            playlist_state: playlist::PlaylistState::default(),
            matrix_state,
            matrix_clipboard: matrix::MatrixClipboard::default(),
            dragged_sample: None,
            live_tracks,
            studio_tracks,
            selected_track_index: 1,
            selected_slot_index: 0,
            track_peak_bits,
            smoothed_track_peaks: [0.0; 16],
            smoothed_master_peak: 0.0,
            openlive_view: OpenLiveView::SessionMatrix,
            arranger_fader_drag: None,
            active_external_plugins: Vec::new(),
            engine_handle,
        });

        cx.set_global(AppStateHandle(state.clone()));
        cx.observe(&state, |_, _, cx| {
            cx.notify();
        });

        // La caja escribe el BPM; el transport lo propaga al motor. `Blur`
        // hace falta porque al perder el foco el input reescribe el texto con
        // el valor acotado, y ese `Change` no lo emite el setter.
        let bpm_input_sub = cx.subscribe(&bpm_input, |_, input, event, cx| {
            if !matches!(event, InputEvent::Change | InputEvent::Blur) {
                return;
            }
            let text = input.read(cx).value();
            let Ok(parsed) = text.trim().parse::<f64>() else {
                return;
            };
            if !parsed.is_finite() {
                return;
            }
            let bpm = parsed.clamp(BPM_MIN, BPM_MAX);
            update_state(cx, |state| {
                if (state.transport.bpm - bpm).abs() > f64::EPSILON {
                    state.transport.bpm = bpm;
                    state.audio_proxy.send(GuiCommand::SetBpm(bpm as f32));
                }
            });
        });

        HikaruApp {
            state,
            focus_handle: cx.focus_handle(),
            _audio_stream: audio_stream,
            _bpm_input_sub: bpm_input_sub,
        }
    }

    pub fn open_plugin_floating_gui(
        &mut self,
        cx: &mut Context<HikaruApp>,
        path: &Path,
        format: PluginFormat,
    ) {
        match spawn_floating_gui(path, format) {
            Ok(plugin_instance) => {
                println!(
                    "[HikaruApp] Plugin cargado y lanzado en ventana flotante: {}",
                    plugin_instance.get_name()
                );
                self.state.update(cx, |s, _| {
                    s.active_external_plugins.push(plugin_instance);
                });
            }
            Err(err) => {
                eprintln!("[HikaruApp] Error al abrir la GUI flotante del plugin: {}", err);
            }
        }
    }

    pub fn sync_hardware_sample_rate(&mut self, cx: &mut Context<HikaruApp>, hardware_sr: f32) {
        if hardware_sr > 0.0 {
            self.state.update(cx, |s, _| {
                s.transport.sample_rate = SampleRate::new(hardware_sr);
            });
        }
    }

    pub fn current_bar(&self, cx: &mut Context<HikaruApp>) -> f32 {
        let s = self.state.read(cx);
        let samples_per_bar = s.transport.samples_per_bar();
        if samples_per_bar <= 0.0 {
            return 1.0;
        }
        (1.0 + s.transport.sample_count as f64 / samples_per_bar) as f32
    }

    pub fn current_tick(&self, cx: &mut Context<HikaruApp>) -> u64 {
        let s = self.state.read(cx);
        s.transport.samples_to_ticks(s.transport.sample_count)
    }

    fn sync_frame(&mut self, cx: &mut Context<HikaruApp>) {
        let (position_clock, audio_proxy) = {
            let s = self.state.read(cx);
            (s.position_clock.clone(), s.audio_proxy.clone())
        };

        self.state.update(cx, |state, cx| {
            state.transport.sample_count = position_clock.load(Ordering::Relaxed);
            state.playlist_state.ppqn = state.transport.ppqn();

            if (state.transport.bpm - state.bpm_synced_to_engine).abs() > f64::EPSILON {
                state.bpm_synced_to_engine = state.transport.bpm;
                audio_proxy.send(GuiCommand::SetBpm(state.transport.bpm as f32));
            }

            if state.transport.playback_state == TransportPlaybackState::Playing
                && state.explorer_state.is_playing_preview
            {
                state.explorer_state.is_playing_preview = false;
                state.explorer_state.preview_position = 0.0;
            }

            if state.transport.playback_state == TransportPlaybackState::Playing {
                let is_live = state.mode == AppMode::OpenLive;
                if state.is_looping && !is_live {
                    let ppqn = state.playlist_state.ppqn.max(1);
                    let mut start = state.playlist_state.loop_start_ticks;
                    let mut end = state.playlist_state.loop_end_ticks;
                    let len = end.saturating_sub(start);
                    if !state.playlist_state.loop_region_active || end <= start || len < ppqn {
                        let total = state.playlist_state.total_project_ticks();
                        start = 0u64;
                        end = total.max(4u64.saturating_mul(ppqn));
                    }
                    if end <= start {
                        end = start.saturating_add(4u64.saturating_mul(ppqn));
                    }
                    if end.saturating_sub(start) < ppqn {
                        end = start.saturating_add(ppqn);
                    }
                    if end > start {
                        state.playlist_state.loop_start_ticks = start;
                        state.playlist_state.loop_end_ticks = end;
                        state.playlist_state.loop_region_active = true;
                        state.playlist_state.loop_preview_start_ticks = start;
                        state.playlist_state.loop_preview_end_ticks = end;
                    }
                }
            }

            {
                let ppqn = state.playlist_state.ppqn.max(1);
                let (want_enabled, want_start, want_end) =
                    if state.is_looping && state.playlist_state.is_loop_region_valid() {
                        let start = state.playlist_state.loop_start_ticks;
                        let mut end = state.playlist_state.loop_end_ticks;
                        if end <= start {
                            end = start.saturating_add(4u64.saturating_mul(ppqn));
                        }
                        if end.saturating_sub(start) < ppqn {
                            end = start.saturating_add(ppqn);
                        }
                        (end > start, start, end)
                    } else {
                        (false, 0, 0)
                    };
                let want_start_samples = state.transport.ticks_to_samples(want_start);
                let want_end_samples = state.transport.ticks_to_samples(want_end);
                let want = (want_enabled, want_start_samples, want_end_samples);
                if state.global_loop_synced_to_engine != Some(want) {
                    state.global_loop_synced_to_engine = Some(want);
                    state
                        .transport
                        .set_loop_region_samples(want_start_samples, want_end_samples);
                    state.transport.set_loop_enabled(want_enabled);
                    audio_proxy.send(GuiCommand::SetGlobalLoop {
                        start_samples: want_start_samples,
                        end_samples: want_end_samples,
                        enabled: want_enabled,
                    });
                }
            }

            if state.mode == AppMode::OpenLive {
                let any_clip_playing = state
                    .matrix_state
                    .grid
                    .iter()
                    .flatten()
                    .any(|slot| slot.state == matrix::SlotState::Playing);
                if any_clip_playing {
                    cx.notify();
                }
            }

            if let Some((track_idx, scene_idx)) = state.matrix_state.selected_slot {
                if let Some(slot) = state
                    .matrix_state
                    .grid
                    .get_mut(track_idx)
                    .and_then(|r| r.get_mut(scene_idx))
                {
                    if let Some(clip) = &mut slot.clip {
                        if let matrix::ClipData::Midi { notes } = &mut clip.content {
                            if state.piano_roll_state.notes_source_slot
                                != Some((track_idx, scene_idx))
                            {
                                state.piano_roll_state.clear_note_selection();
                                state.piano_roll_state.notes_source_slot =
                                    Some((track_idx, scene_idx));
                            }
                            state.piano_roll_state.notes = notes
                                .iter()
                                .map(|&(start_tick, pitch, velocity, duration_ticks)| {
                                    piano_roll::MidiNote {
                                        pitch,
                                        start_tick,
                                        duration_ticks: duration_ticks as u64,
                                        velocity,
                                    }
                                })
                                .collect();
                        } else {
                            state.piano_roll_state.notes.clear();
                            state.piano_roll_state.clear_note_selection();
                            state.piano_roll_state.notes_source_slot = None;
                        }
                    } else {
                        state.piano_roll_state.notes.clear();
                        state.piano_roll_state.clear_note_selection();
                        state.piano_roll_state.notes_source_slot = None;
                    }
                } else {
                    state.piano_roll_state.notes.clear();
                    state.piano_roll_state.clear_note_selection();
                    state.piano_roll_state.notes_source_slot = None;
                }
            } else {
                state.piano_roll_state.notes.clear();
                state.piano_roll_state.clear_note_selection();
                state.piano_roll_state.notes_source_slot = None;
            }

            if state.transport.playback_state == TransportPlaybackState::Playing {
                let transport_tick =
                    state.transport.samples_to_ticks(state.transport.sample_count);

                if state.piano_roll_state.loop_enabled
                    && state.piano_roll_state.selection_active
                    && state.piano_roll_state.selection_end_tick
                        > state.piano_roll_state.selection_start_tick
                {
                    let loop_len = state.piano_roll_state.selection_end_tick
                        - state.piano_roll_state.selection_start_tick;

                    if state.piano_roll_state.loop_start_instant.is_none() {
                        state.piano_roll_state.loop_transport_start_tick = transport_tick;
                        state.piano_roll_state.loop_start_instant = Some(std::time::Instant::now());
                    }

                    let elapsed_secs = state
                        .piano_roll_state
                        .loop_start_instant
                        .map(|t| t.elapsed().as_secs_f64())
                        .unwrap_or(0.0);

                    let ppqn = state.transport.ppqn().max(1) as f64;
                    let ticks_per_second = (state.transport.bpm * ppqn) / 60.0;
                    let ticks_elapsed = (elapsed_secs * ticks_per_second) as u64;

                    let offset_in_loop = ticks_elapsed % loop_len;
                    state.piano_roll_state.playhead_tick =
                        state.piano_roll_state.selection_start_tick + offset_in_loop;
                } else {
                    state.piano_roll_state.playhead_tick = transport_tick;
                    state.piano_roll_state.loop_start_instant = None;
                }
            } else {
                state.piano_roll_state.loop_start_instant = None;
            }

            let piano_roll_tracks = match state.mode {
                AppMode::OpenLive => &state.live_tracks,
                AppMode::OpenStudio => &state.studio_tracks,
            };

            if state.show_piano_roll {
                if let Some((track_idx, scene_idx)) = state.matrix_state.selected_slot {
                    if let Some(slot) = state
                        .matrix_state
                        .grid
                        .get_mut(track_idx)
                        .and_then(|r| r.get_mut(scene_idx))
                    {
                        if let Some(clip) = &mut slot.clip {
                            if let matrix::ClipData::Midi { notes } = &mut clip.content {
                                *notes = state
                                    .piano_roll_state
                                    .notes
                                    .iter()
                                    .map(|n| {
                                        (
                                            n.start_tick,
                                            n.pitch,
                                            n.velocity,
                                            n.duration_ticks as u32,
                                        )
                                    })
                                    .collect();
                            }
                        }
                    }
                }
            }

            piano_roll::trigger_playhead_notes(
                &mut state.piano_roll_state,
                piano_roll_tracks,
                state.selected_track_index,
                &audio_proxy,
            );
        });
    }
}

// =========================================================================
// RENDER PRINCIPAL DE LA APLICACIÓN (LAYOUT DAW INTEGRADO)
// GNU AGPLv3 - crates/hikaru_gui/src/app.rs
// =========================================================================

impl Render for HikaruApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<HikaruApp>) -> impl IntoElement {
        self.sync_frame(cx);

        // Foco en la vista raíz: garantiza que las teclas (TAB, F9-F11)
        // lleguen al div raíz aunque el usuario no haya clickeado nada.
        if window.focused(cx).is_none() {
            window.focus(&self.focus_handle, cx);
        }

        let app_state = self.state.read(cx);
        let mode = app_state.mode;
        let openlive_view = app_state.openlive_view;
        let show_piano_roll = app_state.show_piano_roll;
        let show_dsp_rack = app_state.show_dsp_rack;
        let show_explorer = app_state.show_explorer;
        let show_about = app_state.show_about;
        let audio_settings_open = app_state.audio_settings_state.is_open;
        let plugin_settings_open = app_state.plugin_settings_state.is_open;
        let show_mixer = app_state.show_mixer;
        let dragged_sample = app_state.dragged_sample.clone();
        drop(app_state);

        div()
            .id("app_root")
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .bg(crate::theme::WINDOW_BG)
            
            // 1. BARRA SUPERIOR (MENÚ + TRANSPORT)
            .child(
                v_flex()
                    .w_full()
                    .child(menu_bar::render(cx))
                    .child(header::render(window, cx)),
            )
            
            // 2. ÁREA CENTRAL WORKSPACE (LAYOUT HORIZONTAL + VERTICAL)
            .child(
                h_flex()
                    .flex_1()
                    .w_full()
                    .overflow_hidden()
                    
                    // PANEL LATERAL IZQUIERDO: EXPLORADOR DE ARCHIVOS (Slo si F11 está activo)
                    .when(show_explorer, |this| {
                        this.child(
                            div()
                                .w(px(260.0))
                                .h_full()
                                .border_r_1()
                                .border_color(crate::theme::BORDER_COLOR)
                                .bg(crate::theme::PANEL_BG)
                                .child(explorer::render(cx)),
                        )
                    })
                    
                    // CONTENEDOR PRINCIPAL FLEX (Vistas + Paneles Inferiores)
                    .child(
                        v_flex()
                            .flex_1()
                            .h_full()
                            .overflow_hidden()
                            
                            // VISTA PRINCIPAL (Session Matrix / Arranger / Studio Playlist)
                            .child(
                                div()
                                    .flex_1()
                                    .w_full()
                                    .overflow_hidden()
                                    .child(render_central(mode, openlive_view, cx)),
                            )
                            
                            // PANEL INFERIOR PLEGABLE: PIANO ROLL (F9/F10)
                            .when(show_piano_roll, |this| {
                                this.child(
                                    div()
                                        .h(px(220.0))
                                        .w_full()
                                        .border_t_1()
                                        .border_color(crate::theme::BORDER_COLOR)
                                        .child(piano_roll::render(cx)),
                                )
                            })
                            
                            // PANEL INFERIOR PLEGABLE: DSP RACK
                            .when(show_dsp_rack, |this| {
                                this.child(
                                    div()
                                        .h(px(200.0))
                                        .w_full()
                                        .border_t_1()
                                        .border_color(crate::theme::BORDER_COLOR)
                                        .child(dsp_rack::render(cx)),
                                )
                            })
                            
                            // PANEL INFERIOR PLEGABLE: MIXER DE PISTAS (F9)
                            .when(show_mixer, |this| {
                                this.child(
                                    div()
                                        .h(px(240.0))
                                        .w_full()
                                        .border_t_1()
                                        .border_color(crate::theme::BORDER_COLOR)
                                        .child(mixer::render(cx)),
                                )
                            }),
                    ),
            )
            
            // 3. BARRA DE ESTADO / FOOTER
            .child(footer::render(cx))
            
            // 4. OVERLAYS Y VENTANAS MODALES
            .when(show_about, |this| this.child(about::render(cx)))
            .when(audio_settings_open, |this| this.child(audio_settings::render(cx)))
            .when(plugin_settings_open, |this| {
                this.child(external_plugins_settings::render(cx))
            })
            .when_some(dragged_sample, |this, sample_path| {
                this.child(render_drag_preview(&sample_path))
            })
            
            // SHORTCUTS: el foco vive en el div raíz, por eso capture_key_down
            // los recibe sin importar qué control interno esté enfocado.
            .capture_key_down(move |event, window, cx| {
                // Salvo la caja de BPM: es el único campo de texto de la app,
                // y mientras edita el tempo las teclas le pertenecen a ella
                // (si no, `backspace` borraría clips del playlist).
                let app = state(cx).read(cx);
                let bpm_input = app.bpm_input.clone();
                drop(app);
                if bpm_input.read(cx).focus_handle(cx).is_focused(window) {
                    return;
                }
                let key = event.keystroke.key.as_str().to_lowercase();
                handle_global_key(&key, cx);
            })
    }
}

fn render_central(mode: AppMode, openlive_view: OpenLiveView, cx: &mut Context<HikaruApp>) -> AnyElement {
    match mode {
        AppMode::OpenLive => match openlive_view {
            OpenLiveView::SessionMatrix => matrix::render(cx),
            OpenLiveView::ArrangerView => arranger_view::render(cx),
        },
        AppMode::OpenStudio => playlist::render(cx),
    }
}

fn render_drag_preview(sample_path: &Path) -> AnyElement {
    let name = sample_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    div()
        .absolute()
        .left(px(20.0))
        .top(px(20.0))
        .bg(rgb(0x14161C))
        .border_1()
        .border_color(rgb(0x00FFFF))
        .rounded(px(4.0))
        .p(px(6.0))
        .child(Label::new(format!("🎵 {}", name)).text_sm())
        .into_any_element()
}
