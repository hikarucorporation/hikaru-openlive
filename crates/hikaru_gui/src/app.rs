// Copyright (c) Hikaru Corporation - 2026
// GNU Affero General Public License v3
// Código fuente del App
// crates/hikaru_gui/src/app.rs

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::path::{Path, PathBuf};

use raw_window_handle::HasWindowHandle;
use egui::{CentralPanel, Color32, RichText, ScrollArea, TopBottomPanel, ViewportBuilder, ViewportId};
use hikaru_audio_engine::AudioEngine;
use hikaru_core::SampleRate;
use hikaru_transport::{TransportPlaybackState, TransportPosition};

// Importamos la API del plugin host para ventanas flotantes
use hikaru_plugin_host::{spawn_floating_gui, PluginFormat, PluginInstance};

use crate::audio_proxy::{AudioProxy, GuiCommand};
use crate::views::{
    about, audio_settings, dsp_rack, explorer, external_plugins_settings, footer, header, matrix, menu_bar, mixer, open_wavetable, playlist,
};

/// Sincronización bidireccional Matrix ↔ Mixer.
fn sync_matrix_mixer_bidirectional(
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
        if mixer_idx >= live_tracks.len() { break; }

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

pub struct HikaruApp {
    pub mode: AppMode,
    pub transport: TransportPosition,
    pub position_clock: Arc<AtomicU64>,
    pub output_level_bits: Arc<AtomicU32>,
    pub is_looping: bool,
    pub cpu_usage: f32,
    bpm_synced_to_engine: f64,
    global_loop_synced_to_engine: Option<(bool, u64, u64)>,
    pub show_mixer: bool,
    pub show_dsp_rack: bool,
    pub show_about: bool,
    pub is_recording: bool,
    pub show_explorer: bool,
    pub show_clip_editor: bool,
    pub show_piano_roll: bool,
    pub piano_roll_state: crate::views::piano_roll::PianoRollState,
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
    fonts_configured: bool,

    pub audio_proxy: AudioProxy,
    pub _audio_stream: Option<cpal::Stream>,
    pub engine_handle: Option<Arc<Mutex<AudioEngine<'static>>>>,
    pub track_peak_bits: Vec<Arc<AtomicU32>>,
    pub smoothed_track_peaks: [f32; 16],
    pub smoothed_master_peak: f32,

    pub openlive_view: OpenLiveView,

    /// Almacena las instancias activas de los plugins con ventana flotante nativa abierta
    pub active_external_plugins: Vec<Box<dyn PluginInstance>>,
}

impl HikaruApp {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        audio_proxy: AudioProxy,
        audio_stream: Option<cpal::Stream>,
        position_clock: Arc<AtomicU64>,
        output_level_bits: Arc<AtomicU32>,
        engine_handle: Option<Arc<Mutex<AudioEngine<'static>>>>,
    ) -> Self {
        let sample_rate = SampleRate::new(48000.0);
        let transport = TransportPosition::new(sample_rate, 140.0);

        let mut live_tracks = vec![
            mixer::Track::new(0, "MASTER".to_string(), true),
        ];
        let mut matrix_state = matrix::SessionMatrixState::default();
        {
            let empty_old: Vec<matrix::TrackMeta> = Vec::new();
            let empty_mixer_old: Vec<(f32, f32, bool, bool)> = Vec::new();
            sync_matrix_mixer_bidirectional(
                &mut live_tracks, &mut matrix_state, &empty_old, &empty_mixer_old, &audio_proxy,
            );
        }
        for t in &mut live_tracks {
            if t.volume <= 0.0 { t.volume = 0.70; }
        }

        let mut studio_tracks = vec![
            mixer::Track::new(0, "MASTER".to_string(), true),
            mixer::Track::new(1, "TRACK 01".to_string(), false),
        ];
        for t in &mut studio_tracks {
            t.volume = 0.70;
        }

        let track_peak_bits_init: Vec<Arc<AtomicU32>> = if let Some(ref handle) = engine_handle {
            if let Ok(engine) = handle.try_lock() {
                engine.track_peak_bits.iter().map(|a| Arc::clone(a)).collect()
            } else {
                (0..16).map(|_| Arc::new(AtomicU32::new(0.0f32.to_bits()))).collect()
            }
        } else {
            (0..16).map(|_| Arc::new(AtomicU32::new(0.0f32.to_bits()))).collect()
        };

        Self {
            mode: AppMode::OpenLive,
            transport,
            position_clock,
            output_level_bits,
            is_looping: false,
            cpu_usage: 0.12,
            show_mixer: false,
            show_dsp_rack: false,
            show_about: false,
            show_clip_editor: true,
            show_piano_roll: false,
            piano_roll_state: crate::views::piano_roll::PianoRollState::default(),

            audio_settings_state: audio_settings::AudioSettingsState::default(),
            plugin_settings_state: external_plugins_settings::PluginSettingsState::default(),
            dragged_sample: None,

            show_explorer: false,
            explorer_state: explorer::FileExplorerState::default(),

            is_recording: false,

            playlist_state: playlist::PlaylistState::default(),
            matrix_state,
            matrix_clipboard: matrix::MatrixClipboard::default(),

            live_tracks,
            studio_tracks,

            selected_track_index: 1,
            selected_slot_index: 0,
            fonts_configured: false,
            bpm_synced_to_engine: -1.0,
            global_loop_synced_to_engine: None,
            audio_proxy,
            _audio_stream: audio_stream,
            engine_handle,
            track_peak_bits: track_peak_bits_init,
            smoothed_track_peaks: [0.0; 16],
            smoothed_master_peak: 0.0,
            openlive_view: OpenLiveView::SessionMatrix,
            active_external_plugins: Vec::new(),
        }
    }

    /// Método para abrir la GUI flotante de un plugin independiente (ej. Vital)
    pub fn open_plugin_floating_gui(&mut self, path: &Path, format: PluginFormat) {
        match spawn_floating_gui(path, format) {
            Ok(plugin_instance) => {
                println!("[HikaruApp] Plugin cargado y lanzado en ventana flotante: {}", plugin_instance.get_name());
                self.active_external_plugins.push(plugin_instance);
            }
            Err(err) => {
                eprintln!("[HikaruApp] Error al abrir la GUI flotante del plugin: {}", err);
            }
        }
    }

    pub fn sync_hardware_sample_rate(&mut self, hardware_sr: f32) {
        if hardware_sr > 0.0 {
            self.transport.sample_rate = SampleRate::new(hardware_sr);
        }
    }

    pub fn current_bar(&self) -> f32 {
        let samples_per_bar = self.transport.samples_per_bar();
        if samples_per_bar <= 0.0 {
            return 1.0;
        }
        (1.0 + self.transport.sample_count as f64 / samples_per_bar) as f32
    }

    pub fn current_tick(&self) -> u64 {
        self.transport
            .samples_to_ticks(self.transport.sample_count)
    }
}

impl eframe::App for HikaruApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if !self.fonts_configured {
            setup_custom_fonts(ctx);
            self.fonts_configured = true;
        }

        self.transport.sample_count = self.position_clock.load(Ordering::Relaxed);
        self.playlist_state.ppqn = self.transport.ppqn();

        if (self.transport.bpm - self.bpm_synced_to_engine).abs() > f64::EPSILON {
            self.bpm_synced_to_engine = self.transport.bpm;
            self.audio_proxy
                .send(GuiCommand::SetBpm(self.transport.bpm as f32));
        }

        if self.transport.playback_state == TransportPlaybackState::Playing
            && self.explorer_state.is_playing_preview
        {
            self.explorer_state.is_playing_preview = false;
            self.explorer_state.preview_position = 0.0;
        }

        if self.transport.playback_state == TransportPlaybackState::Playing {
            let is_live = self.mode == AppMode::OpenLive;
            if self.is_looping && !is_live {
                let ppqn = self.playlist_state.ppqn.max(1);
                let mut start = self.playlist_state.loop_start_ticks;
                let mut end = self.playlist_state.loop_end_ticks;
                let len = end.saturating_sub(start);
                if !self.playlist_state.loop_region_active
                    || end <= start
                    || len < ppqn
                {
                    let total = self.playlist_state.total_project_ticks();
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
                    self.playlist_state.loop_start_ticks = start;
                    self.playlist_state.loop_end_ticks = end;
                    self.playlist_state.loop_region_active = true;
                    self.playlist_state.loop_preview_start_ticks = start;
                    self.playlist_state.loop_preview_end_ticks = end;
                }
            }
            ctx.request_repaint();
        }

        {
            let ppqn = self.playlist_state.ppqn.max(1);
            let (want_enabled, want_start, want_end) =
                if self.is_looping && self.playlist_state.is_loop_region_valid() {
                    let start = self.playlist_state.loop_start_ticks;
                    let mut end = self.playlist_state.loop_end_ticks;
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
            let want_start_samples = self.transport.ticks_to_samples(want_start);
            let want_end_samples = self.transport.ticks_to_samples(want_end);
            let want = (want_enabled, want_start_samples, want_end_samples);
            if self.global_loop_synced_to_engine != Some(want) {
                self.global_loop_synced_to_engine = Some(want);
                self.transport
                    .set_loop_region_samples(want_start_samples, want_end_samples);
                self.transport.set_loop_enabled(want_enabled);
                self.audio_proxy.send(GuiCommand::SetGlobalLoop {
                    start_samples: want_start_samples,
                    end_samples: want_end_samples,
                    enabled: want_enabled,
                });
            }
        }

        if self.mode == AppMode::OpenLive {
            let any_clip_playing = self.matrix_state.grid.iter().flatten()
                .any(|slot| slot.state == matrix::SlotState::Playing);
            if any_clip_playing {
                ctx.request_repaint();
            }
        }

        ctx.input(|i| {
            if i.key_pressed(egui::Key::Tab) {
                match self.mode {
                    AppMode::OpenLive => {
                        self.openlive_view = match self.openlive_view {
                            OpenLiveView::SessionMatrix => OpenLiveView::ArrangerView,
                            OpenLiveView::ArrangerView => OpenLiveView::SessionMatrix,
                        };
                    }
                    AppMode::OpenStudio => {
                        self.mode = AppMode::OpenLive;
                    }
                }
            }

            if i.key_pressed(egui::Key::F9) {
                self.show_mixer = !self.show_mixer;
            }
            if i.key_pressed(egui::Key::F10) {
                self.show_dsp_rack = !self.show_dsp_rack;
            }
            if i.key_pressed(egui::Key::F11) {
                self.show_explorer = !self.show_explorer;
            }
        });

        TopBottomPanel::top("menu_bar_panel").resizable(false).show(ctx, |ui| {
            menu_bar::show(ui, self);
        });

        TopBottomPanel::top("header_panel").resizable(false).show(ctx, |ui| {
            ScrollArea::horizontal()
                .id_source("header_scroll_area")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    header::show(
                        ui,
                        &mut self.transport,
                        &self.position_clock,
                        &mut self.is_looping,
                        &mut self.is_recording,
                        &mut self.mode,
                        &mut self.show_mixer,
                        &mut self.show_dsp_rack,
                        &mut self.show_explorer,
                        &self.playlist_state,
                        &self.audio_proxy,
                    );
                });
        });

        TopBottomPanel::bottom("footer_panel").resizable(false).show(ctx, |ui| {
            footer::show(
                ui, 
                self.cpu_usage, 
                &mut self.matrix_state.show_editor, 
                &mut self.show_piano_roll,
                &mut self.show_dsp_rack, // <-- Agregar esta variable
            );
        });

        // 1. DENTRO DE IMPL APPARC FOR HIKARUAPP (update):
        // Declaramos primero todos los TopBottomPanels inferiores para que recorten el espacio del CentralPanel.

        if self.show_piano_roll {
            if let Some((track_idx, scene_idx)) = self.matrix_state.selected_slot {
                if let Some(slot) = self.matrix_state.grid.get_mut(track_idx).and_then(|r| r.get_mut(scene_idx)) {
                    if let Some(clip) = &mut slot.clip {
                        if let matrix::ClipData::Midi { notes } = &mut clip.content {
                            self.piano_roll_state.notes = notes.iter().map(|&(start_tick, pitch, velocity, duration_ticks)| {
                                crate::views::piano_roll::MidiNote {
                                    pitch,
                                    start_tick,
                                    duration_ticks: duration_ticks as u64,
                                    velocity,
                                }
                            }).collect();
                        } else {
                            self.piano_roll_state.notes.clear();
                        }
                    } else {
                        self.piano_roll_state.notes.clear();
                    }
                }
            } else {
                self.piano_roll_state.notes.clear();
            }

            let piano_roll_tracks = match self.mode {
                AppMode::OpenLive => &self.live_tracks,
                AppMode::OpenStudio => &self.studio_tracks,
            };

            TopBottomPanel::bottom("piano_roll_panel")
                .resizable(true)
                .default_height(280.0)
                .show(ctx, |ui| {
                    crate::views::piano_roll::show(
                        ui,
                        &mut self.piano_roll_state,
                        piano_roll_tracks,
                        self.selected_track_index,
                        &self.audio_proxy,
                    );
                });

            if let Some((track_idx, scene_idx)) = self.matrix_state.selected_slot {
                if let Some(slot) = self.matrix_state.grid.get_mut(track_idx).and_then(|r| r.get_mut(scene_idx)) {
                    if let Some(clip) = &mut slot.clip {
                        if let matrix::ClipData::Midi { notes } = &mut clip.content {
                            *notes = self.piano_roll_state.notes.iter().map(|n| {
                                (n.start_tick, n.pitch, n.velocity, n.duration_ticks as u32)
                            }).collect();
                        }
                    }
                }
            }
        }

        if self.show_dsp_rack {
            let active_tracks = match self.mode {
                AppMode::OpenLive => &mut self.live_tracks,
                AppMode::OpenStudio => &mut self.studio_tracks,
            };

            egui::TopBottomPanel::bottom("dsp_rack_bottom_panel")
                .resizable(true)
                .default_height(280.0)
                .min_height(180.0)
                .max_height(500.0)
                .show(ctx, |ui| {
                    dsp_rack::show(
                        ui,
                        active_tracks,
                        self.selected_track_index,
                        &mut self.selected_slot_index,
                        self.matrix_state.selected_slot,
                        &mut self.dragged_sample,
                        &self.audio_proxy,
                    );
                });
        }

        CentralPanel::default().show(ctx, |ui| {
            let engine_handle = self.engine_handle.clone();
            match self.mode {
                AppMode::OpenLive => {
                    match self.openlive_view {
                        OpenLiveView::SessionMatrix => {
                            let ppqn = self.transport.ppqn();
                            let current_tick = self.current_tick();

                            matrix::show(
                                ui,
                                &mut self.matrix_state,
                                &mut self.matrix_clipboard,
                                &mut self.dragged_sample,
                                &self.audio_proxy,
                                ppqn,
                                current_tick,
                                self.transport.bpm,
                                self.transport.sample_rate.get() as u32,
                                self.transport.sample_count,
                                self.is_looping,
                                self.playlist_state.loop_end_ticks,
                                self.playlist_state.loop_start_ticks,
                                engine_handle.as_ref(),
                            );
                        }
                        OpenLiveView::ArrangerView => {
                            crate::views::arranger_view::show(
                                ui,
                                &mut self.live_tracks,
                                &mut self.matrix_state,
                                &mut self.matrix_clipboard,
                                &self.transport,
                                &self.audio_proxy,
                                &mut self.dragged_sample,
                                self.transport.bpm,
                            );
                        }
                    }
                }
                AppMode::OpenStudio => {
                    let mut current_bar = self.current_bar();
                    let transport_sample_count = self.transport.sample_count;
                    let beats_per_bar = self.transport.beats_per_bar;
                    playlist::show(
                        ui,
                        &mut self.playlist_state,
                        &mut self.studio_tracks,
                        &mut current_bar,
                        &mut self.dragged_sample,
                        &self.audio_proxy,
                        self.transport.bpm,
                        self.transport.sample_rate.get() as u32,
                        self.is_looping,
                        transport_sample_count,
                        beats_per_bar,
                    );
                }
            }

            if let Some(ref sample_path) = self.dragged_sample {
                if let Some(pointer_pos) = ctx.pointer_latest_pos() {
                    egui::Area::new(egui::Id::new("drag_sample_preview"))
                        .fixed_pos(pointer_pos + egui::vec2(14.0, 14.0))
                        .order(egui::Order::Tooltip)
                        .interactable(false)
                        .show(ctx, |ui| {
                            egui::Frame::popup(ui.style())
                                .fill(Color32::from_rgb(20, 22, 28))
                                .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(0, 255, 255)))
                                .rounding(4.0)
                                .inner_margin(6.0)
                                .show(ui, |ui| {
                                    let name = sample_path.file_name().unwrap_or_default().to_string_lossy();
                                    ui.label(RichText::new(format!("🎵 {}", name)).size(11.0).color(Color32::WHITE));
                                });
                        });
                }
            }

            if self.show_explorer {
                egui::Window::new("File Explorer")
                    .resizable(true)
                    .collapsible(true)
                    .default_size([300.0, 400.0])
                    .drag_to_scroll(false)
                    .show(ctx, |ui| {
                        crate::views::explorer::show(
                            ui,
                            &mut self.explorer_state,
                            &mut self.dragged_sample,
                            &self.audio_proxy,
                            self.transport.bpm as f32,
                        );
                    });
            }
        });

        if self.show_mixer {
            let raw_master =
                f32::from_bits(self.output_level_bits.load(Ordering::Relaxed));
            let raw_master = if raw_master.is_finite() { raw_master } else { 0.0 };
            self.smoothed_master_peak = self.smoothed_master_peak * 0.85 + raw_master * 0.15;
            let output_level = self.smoothed_master_peak;

            let mut track_peaks = [0.0f32; 16];
            for (i, arc) in self.track_peak_bits.iter().enumerate().take(16) {
                let raw = f32::from_bits(arc.load(Ordering::Relaxed));
                let raw = if raw.is_finite() { raw } else { 0.0 };
                self.smoothed_track_peaks[i] = self.smoothed_track_peaks[i] * 0.85 + raw * 0.15;
                track_peaks[i] = self.smoothed_track_peaks[i];
            }

            let old_matrix: Vec<matrix::TrackMeta> = if self.mode == AppMode::OpenLive {
                self.matrix_state.tracks.clone()
            } else {
                Vec::new()
            };

            let old_live: Vec<(f32, f32, bool, bool)> = {
                let active = match self.mode {
                    AppMode::OpenLive => &self.live_tracks,
                    AppMode::OpenStudio => &self.studio_tracks,
                };
                active.iter()
                    .map(|t| (t.volume, t.pan, t.mute, t.solo))
                    .collect()
            };

            ctx.show_viewport_immediate(
                ViewportId::from_hash_of("hikaru_mixer_viewport"),
                ViewportBuilder::default()
                    .with_title("Hikaru Mixer")
                    .with_inner_size([800.0, 450.0])
                    .with_min_inner_size([300.0, 250.0]),
                |vp_ctx, _class| {
                    CentralPanel::default().show(vp_ctx, |ui| {
                        let active_tracks = match self.mode {
                            AppMode::OpenLive => &mut self.live_tracks,
                            AppMode::OpenStudio => &mut self.studio_tracks,
                        };
                        mixer::show(ui, active_tracks, &mut self.selected_track_index, &mut self.mode, output_level, &track_peaks);
                    });
                },
            );

            {
                let active = match self.mode {
                    AppMode::OpenLive => &self.live_tracks,
                    AppMode::OpenStudio => &self.studio_tracks,
                };
                if let Some(old_master_vol) = old_live.first().map(|m| m.0) {
                    let master = &active[0];
                    if (master.volume - old_master_vol).abs() > f32::EPSILON {
                        self.audio_proxy.send(GuiCommand::SetMasterVolume {
                            volume_db: master.volume,
                        });
                    }
                }
            }

            if self.mode == AppMode::OpenLive {
                sync_matrix_mixer_bidirectional(
                    &mut self.live_tracks,
                    &mut self.matrix_state,
                    &old_matrix,
                    &old_live,
                    &self.audio_proxy,
                );
            }
        }

        // DSP RACK PANEL (Se declara antes del CentralPanel)
        let active_tracks = match self.mode {
            AppMode::OpenLive => &mut self.live_tracks,
            AppMode::OpenStudio => &mut self.studio_tracks,
        };

        for track in active_tracks.iter_mut() {
            for slot in track.effects.iter_mut() {
                if slot.name == "OpenWavetable" && slot.is_open {
                    let viewport_title = format!("OpenWavetable - TRK: {} (Slot {})", track.name, slot.id + 1);
                    let viewport_id = ViewportId::from_hash_of(&(track.id, slot.id, "open_wavetable_instance"));

                    ctx.show_viewport_immediate(
                        viewport_id,
                        ViewportBuilder::default()
                            .with_title(viewport_title)
                            .with_inner_size([750.0, 520.0])
                            .with_min_inner_size([350.0, 300.0]),
                        |vp_ctx, _class| {
                            if vp_ctx.input(|i| i.viewport().close_requested()) {
                                slot.is_open = false;
                            }

                            CentralPanel::default().show(vp_ctx, |ui| {
                                open_wavetable::show(
                                    ui,
                                    &mut slot.wavetable_oscillators,
                                    &mut slot.modulators,
                                    &mut slot.cam_x,
                                    &mut slot.cam_y,
                                    &mut slot.cam_z,
                                );
                            });
                        },
                    );
                }
            }
        }

        if self.show_about {
            let about_title = match self.mode {
                AppMode::OpenLive => "About Hikaru OpenLive",
                AppMode::OpenStudio => "About Hikaru OpenStudio",
            };

            ctx.show_viewport_immediate(
                ViewportId::from_hash_of("hikaru_about_viewport"),
                ViewportBuilder::default()
                    .with_title(about_title)
                    .with_inner_size([380.0, 300.0])
                    .with_resizable(false),
                |vp_ctx, _class| {
                    if vp_ctx.input(|i| i.viewport().close_requested()) {
                        self.show_about = false;
                    }

                    CentralPanel::default().show(vp_ctx, |ui| {
                        about::show(ui, self.mode);
                    });
                },
            );
        }

        if self.audio_settings_state.is_open {
            ctx.show_viewport_immediate(
                ViewportId::from_hash_of("hikaru_audio_settings_viewport"),
                ViewportBuilder::default()
                    .with_title("Audio Setup (JACK / ALSA / PipeWire)")
                    .with_inner_size([440.0, 320.0])
                    .with_resizable(false),
                |vp_ctx, _class| {
                    if vp_ctx.input(|i| i.viewport().close_requested()) {
                        self.audio_settings_state.is_open = false;
                    }

                    CentralPanel::default().show(vp_ctx, |ui| {
                        audio_settings::show(ui, &mut self.audio_settings_state);
                    });
                },
            );
        }

        if self.plugin_settings_state.is_open {
            let window_handle = frame.window_handle().ok().map(|h| h.as_raw());

            ctx.show_viewport_immediate(
                ViewportId::from_hash_of("hikaru_plugin_settings_viewport"),
                ViewportBuilder::default()
                    .with_title("VST3 / CLAP Plugin Settings")
                    .with_inner_size([520.0, 380.0])
                    .with_resizable(true),
                |vp_ctx, _class| {
                    if vp_ctx.input(|i| i.viewport().close_requested()) {
                        self.plugin_settings_state.is_open = false;
                    }

                    CentralPanel::default().show(vp_ctx, |_ui| {
                        external_plugins_settings::render(
                            vp_ctx,
                            &mut self.plugin_settings_state,
                            window_handle,
                        );
                    });
                },
            );
        }
    }
}

use egui::{FontData, FontDefinitions, FontFamily};

fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        "Arimo-Regular".to_owned(),
        FontData::from_static(include_bytes!("../assets/fonts/Arimo/static/Arimo-Regular.ttf")),
    );

    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "Arimo-Regular".to_owned());

    ctx.set_fonts(fonts);
}