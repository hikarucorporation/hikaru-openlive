// Hikaru OpenLive - Bitwig 5.1.7 1:1 Top Tab Bar & GPU Transport Header
// GNU AGPLv3
// crates/hikaru_gui/src/views/header.rs

use egui::{pos2, vec2, Align2, Color32, CursorIcon, PopupCloseBehavior, Rect, Sense, Stroke, Ui};

use crate::app::{format_sig, set_footer_cues, step_sig, AppMode, AppState};
use crate::audio_proxy::{AudioClipData, GuiCommand};
use crate::project;
use crate::theme::*;
use crate::ui::bitwig_kit::{
    capsule_icon_button, capsule_text_button, flat_icon_toggle, JoinSide, BW_ACCENT_ORANGE,
    BW_LCD_CYAN,
};
use crate::ui::bitwig_wgpu::{
    paint_capsule, paint_lcd_bezel, FLAG_ACTIVE, FLAG_HOVERED, FLAG_JOIN_LEFT, FLAG_JOIN_RIGHT,
};
use crate::ui::fonts;
use crate::ui::icons;
use crate::views::dsp_rack::PLUGIN_CATALOG;
use crate::views::mixer::{DspSlot, Track};
use crate::views::playlist::ClipType;
use hikaru_transport::transport_state::TransportPlaybackState;

const CAPSULE_H: f32 = 30.0;

fn format_timecode(bars: f32, bpm: f64, beats_per_bar: u32) -> String {
    let bpb = beats_per_bar.max(1) as f32;
    let beats = (bars - 1.0).max(0.0) * bpb;
    let total_seconds = (beats / (bpm.max(1.0) as f32)) * 60.0;

    let minutes = (total_seconds / 60.0) as u32;
    let seconds = (total_seconds % 60.0) as u32;
    let millis = (total_seconds.fract() * 1000.0) as u32;

    format!("{}:{:02}.{:03}", minutes, seconds, millis)
}

fn format_musical_position(
    sample_count: u64,
    sample_rate: f64,
    bpm: f64,
    beats_per_bar: u32,
) -> String {
    let sr = sample_rate.max(1.0);
    let bpb = beats_per_bar.max(1) as u64;
    let samples_per_beat = (sr * 60.0) / bpm.max(1.0);
    let total_beats = sample_count as f64 / samples_per_beat.max(1.0);
    let whole_beats = total_beats.floor() as u64;
    let bar = 1 + (whole_beats / bpb);
    let beat = 1 + (whole_beats % bpb);
    let sub_beat = total_beats.fract() * 4.0;
    let sixteenth = 1 + (sub_beat.floor() as u64).min(3);
    let ticks_pct = ((sub_beat.fract() * 100.0).floor() as u64).min(99);
    format!("{}.{}.{}.{:02}", bar, beat, sixteenth, ticks_pct)
}

/// Control segmentado GPU estilo Bitwig Studio 5.1.7 en la esquina superior izquierda
/// de la columna de cabeceras de pista (Clip Launcher `3×2` | Arranger `2 barras`).
pub fn bitwig_layout_toggles_widget(
    ui: &mut Ui,
    is_launcher_active: bool,
    is_timeline_active: bool,
) -> (egui::Response, egui::Response) {
    let total_size = vec2(56.0, 24.0);
    let (rect, _) = ui.allocate_exact_size(total_size, Sense::hover());

    let mid_x = rect.center().x;
    let rect_launcher = Rect::from_min_max(rect.min, pos2(mid_x, rect.max.y));
    let rect_timeline = Rect::from_min_max(pos2(mid_x, rect.min.y), rect.max);

    let launcher_resp = ui.interact(
        rect_launcher,
        ui.id().with("bitwig_clip_launcher_toggle"),
        Sense::click(),
    );
    let timeline_resp = ui.interact(
        rect_timeline,
        ui.id().with("bitwig_arranger_timeline_toggle"),
        Sense::click(),
    );

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let idle_bg = Color32::from_rgb(56, 58, 63);
        let dark_active_bg = Color32::from_rgb(32, 34, 38);

        let mut flags_l = FLAG_JOIN_RIGHT;
        if launcher_resp.hovered() {
            flags_l |= FLAG_HOVERED;
        }
        paint_capsule(
            painter,
            rect_launcher,
            2.5,
            if is_launcher_active { dark_active_bg } else { idle_bg },
            dark_active_bg,
            flags_l,
        );
        let fg_l = if is_launcher_active {
            BW_ACCENT_ORANGE
        } else if launcher_resp.hovered() {
            Color32::WHITE
        } else {
            Color32::from_rgb(184, 188, 196)
        };
        icons::paint_clip_launcher_grid(painter, rect_launcher.shrink(3.5), fg_l);

        let mut flags_r = FLAG_JOIN_LEFT;
        if timeline_resp.hovered() {
            flags_r |= FLAG_HOVERED;
        }
        paint_capsule(
            painter,
            rect_timeline,
            2.5,
            if is_timeline_active { dark_active_bg } else { idle_bg },
            dark_active_bg,
            flags_r,
        );
        let fg_r = if is_timeline_active {
            BW_ACCENT_ORANGE
        } else if timeline_resp.hovered() {
            Color32::WHITE
        } else {
            Color32::from_rgb(184, 188, 196)
        };
        icons::paint_arranger_lanes(painter, rect_timeline.shrink(3.5), fg_r);
    }

    if launcher_resp.hovered() || timeline_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    (launcher_resp, timeline_resp)
}

pub fn clip_launcher_toggle_widget(ui: &mut Ui, active: bool) -> egui::Response {
    flat_icon_toggle(
        ui,
        vec2(26.0, 22.0),
        active,
        BW_ACCENT_ORANGE,
        Color32::from_rgb(184, 188, 196),
        |painter, rect, fg| icons::paint_clip_launcher_grid(painter, rect, fg),
    )
}

pub fn arranger_timeline_toggle_widget(ui: &mut Ui, active: bool) -> egui::Response {
    flat_icon_toggle(
        ui,
        vec2(26.0, 22.0),
        active,
        BW_ACCENT_ORANGE,
        Color32::from_rgb(184, 188, 196),
        |painter, rect, fg| icons::paint_arranger_lanes(painter, rect, fg),
    )
}

fn trigger_play_or_pause(state: &mut AppState) {
    if state.transport.playback_state == TransportPlaybackState::Playing {
        state.transport.playback_state = TransportPlaybackState::Paused;
        crate::views::hikaru_native_plugins::hikaru_openmodulation::on_transport_pause_or_stop();
        state.audio_proxy.send(GuiCommand::Pause);
    } else {
        state.transport.playback_state = TransportPlaybackState::Playing;
        crate::views::hikaru_native_plugins::hikaru_openmodulation::on_transport_play(state.transport.sample_count);
        state.audio_proxy.send(GuiCommand::StopPreview);
        state
            .audio_proxy
            .send(GuiCommand::SetBpm(state.transport.bpm as f32));

        if !state.playlist_state.clips.is_empty() && state.panel_layout.show_arranger {
            state.audio_proxy.send(GuiCommand::SetAppMode(true));
            let sr = state.transport.sample_rate.get().max(1.0);
            let clips_to_sync: Vec<AudioClipData> = state
                .playlist_state
                .clips
                .iter()
                .filter_map(|(track_id, clip)| {
                    if let ClipType::Audio {
                        sample_path,
                        sample_offset_ticks,
                        ..
                    } = &clip.clip_type
                    {
                        let clip_length_frames =
                            state.transport.clip_length_frames(clip.duration_ticks);
                        let duration_secs = clip_length_frames as f32 / sr;
                        Some(AudioClipData {
                            clip_id: clip.id,
                            path: sample_path.clone(),
                            start_secs: state.transport.ticks_to_samples(clip.start_tick) as f32
                                / sr,
                            duration_secs,
                            offset_secs: state.transport.ticks_to_samples(*sample_offset_ticks)
                                as f32
                                / sr,
                            track_index: *track_id,
                        })
                    } else {
                        None
                    }
                })
                .collect();

            state
                .audio_proxy
                .send(GuiCommand::SyncPlaylistClips { clips: clips_to_sync });
        } else {
            state.audio_proxy.send(GuiCommand::SetAppMode(false));
        }

        state.audio_proxy.send(GuiCommand::Seek {
            sample_count: state.transport.sample_count,
        });
        state.audio_proxy.send(GuiCommand::Play);
    }
}

fn save_project_action(state: &mut AppState) {
    let cur = state.project_path.clone();
    let target = cur.or_else(|| project::ask_save_path(state.mode, None));
    if let Some(t) = target {
        if let Ok(s) = project::save(state, &t) {
            state.project_path = Some(s);
            state.project_dirty = false;
        }
    }
}

fn add_audio_track_action(state: &mut AppState) {
    state.matrix_state.add_track();
    let new_idx = state.matrix_state.tracks.len();
    let mut tr = Track::new(new_idx, format!("Audio {new_idx}"), false);
    tr.matrix_idx = Some(new_idx - 1);
    state.live_tracks.push(tr.clone());
    state.studio_tracks.push(tr);
}

/// Renderiza la cabecera doble estilo **Bitwig Studio 5.1.7**:
/// 1. Barra superior de pestañas de proyecto + emblema central de 10 puntos (`24px`).
/// 2. Barra principal de transporte con grupos de cápsulas GPU y pantalla LCD (`40px`).
pub fn render(ui: &mut Ui, state: &mut AppState) {
    // =====================================================================
    // FILA 1: BARRA SUPERIOR DE PESTAÑAS DE PROYECTO Y EMBLEMA DE 10 PUNTOS
    // =====================================================================
    let top_bar_bg = Color32::from_rgb(40, 41, 45);
    let (top_rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    if ui.is_rect_visible(top_rect) {
        ui.painter().rect_filled(top_rect, 0.0, top_bar_bg);
        ui.painter().line_segment(
            [
                pos2(top_rect.min.x, top_rect.max.y - 0.5),
                pos2(top_rect.max.x, top_rect.max.y - 0.5),
            ],
            Stroke::new(1.0_f32, Color32::from_rgb(20, 21, 24)),
        );
    }

    // Pastilla izquierda de marca (`HIKARU`) + Pestaña activa de proyecto (`New 1 * ×`)
    let badge_rect = Rect::from_min_size(pos2(top_rect.min.x + 4.0, top_rect.min.y + 3.0), vec2(44.0, 18.0));
    let badge_resp = ui.interact(badge_rect, ui.id().with("hikaru_brand_pill"), Sense::click());
    if ui.is_rect_visible(badge_rect) {
        ui.painter()
            .rect_filled(badge_rect, 2.5, Color32::from_rgb(218, 46, 38));
        ui.painter().text(
            badge_rect.center(),
            Align2::CENTER_CENTER,
            "HIKARU",
            fonts::semibold(10.0),
            Color32::WHITE,
        );
    }
    if badge_resp.clicked() {
        state.show_about = true;
    }

    let proj_name = state
        .project_path
        .as_ref()
        .and_then(|p| p.file_stem())
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "New 1".to_string());
    let tab_title = if state.project_dirty {
        format!("{proj_name} *")
    } else {
        proj_name
    };

    let tab_w = (tab_title.len() as f32 * 6.0 + 38.0).clamp(76.0, 170.0);
    let tab_rect = Rect::from_min_size(pos2(badge_rect.max.x + 6.0, top_rect.min.y + 3.0), vec2(tab_w, 20.0));
    let tab_close_rect = Rect::from_center_size(pos2(tab_rect.max.x - 10.0, tab_rect.center().y), vec2(12.0, 12.0));
    let close_resp = ui.interact(tab_close_rect, ui.id().with("proj_tab_close"), Sense::click());

    if ui.is_rect_visible(tab_rect) {
        let p = ui.painter();
        p.rect_filled(tab_rect, 2.5, Color32::from_rgb(59, 60, 65));
        p.rect_stroke(
            tab_rect,
            2.5,
            Stroke::new(1.0_f32, Color32::from_rgb(108, 110, 116)),
            egui::StrokeKind::Inside,
        );
        // Indicador cuadrado de motor activo dentro de la pestaña
        let dot_sq = Rect::from_center_size(pos2(tab_rect.min.x + 9.0, tab_rect.center().y), vec2(4.5, 4.5));
        p.rect_filled(dot_sq, 0.5, Color32::from_rgb(220, 222, 226));

        p.text(
            pos2(tab_rect.min.x + 17.0, tab_rect.center().y),
            Align2::LEFT_CENTER,
            &tab_title,
            fonts::regular(11.0),
            Color32::from_rgb(236, 238, 242),
        );

        let x_col = if close_resp.hovered() {
            Color32::WHITE
        } else {
            Color32::from_rgb(185, 188, 194)
        };
        icons::paint_icon(p, tab_close_rect.shrink(2.5), icons::X_CLOSE, x_col, 1.3);
    }
    if close_resp.clicked() {
        project::reset(state);
        state.project_path = None;
        state.project_dirty = false;
    }

    // Emblema central de 10 puntos estilo Bitwig (abre el Studio Dashboard)
    let emblem_rect = Rect::from_center_size(top_rect.center(), vec2(36.0, 20.0));
    let emblem_resp = ui.interact(emblem_rect, ui.id().with("bitwig_10dot_emblem"), Sense::click());
    if ui.is_rect_visible(emblem_rect) {
        if emblem_resp.hovered() || state.control_panel_state.is_open {
            ui.painter().rect_filled(
                emblem_rect,
                3.0,
                Color32::from_rgba_unmultiplied(255, 255, 255, 22),
            );
        }
        let emb_col = if state.control_panel_state.is_open {
            BW_ACCENT_ORANGE
        } else if emblem_resp.hovered() {
            Color32::WHITE
        } else {
            Color32::from_rgb(220, 222, 228)
        };
        icons::paint_bitwig_emblem(ui.painter(), emblem_rect, emb_col);
    }
    if emblem_resp.clicked() {
        state.control_panel_state.is_open = !state.control_panel_state.is_open;
        if !state.control_panel_state.is_open {
            state
                .modal_drags
                .reset(crate::views::modal::ModalId::ControlPanel);
        }
    }
    if emblem_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        set_footer_cues(
            state,
            None,
            &[("CLICK", "Toggle Studio Dashboard & Audio Engine")],
        );
    }

    // Indicador derecho de estado del motor de audio
    let eng_dot_rect = Rect::from_center_size(pos2(top_rect.max.x - 18.0, top_rect.center().y), vec2(16.0, 16.0));
    let eng_resp = ui.interact(eng_dot_rect, ui.id().with("top_audio_status_dot"), Sense::click());
    if ui.is_rect_visible(eng_dot_rect) {
        let active_col = if state.audio_settings_state.is_open {
            BW_ACCENT_ORANGE
        } else {
            Color32::from_rgb(104, 108, 116)
        };
        ui.painter().circle_filled(eng_dot_rect.center(), 3.6, active_col);
    }
    if eng_resp.clicked() {
        state.audio_settings_state.is_open = !state.audio_settings_state.is_open;
    }

    // =====================================================================
    // FILA 2: BARRA PRINCIPAL DE TRANSPORTE CON CÁPSULAS GPU Y PANTALLA LCD
    // =====================================================================
    let is_playing = state.transport.playback_state == TransportPlaybackState::Playing;
    let is_recording = state.is_recording;
    let is_looping = state.is_looping;

    let bpm = state.transport.bpm;
    let sample_rate = state.transport.sample_rate.get() as f64;
    let sample_count = state.transport.sample_count;
    let beats_per_bar = state.transport.beats_per_bar;
    let beat_division = state.transport.beat_division;

    let samples_per_beat = (sample_rate * 60.0) / bpm.max(1.0);
    let samples_per_bar = samples_per_beat * beats_per_bar.max(1) as f64;
    let current_bar = 1.0 + (sample_count as f64 / samples_per_bar.max(1.0)) as f32;

    let musical_pos = format_musical_position(sample_count, sample_rate, bpm, beats_per_bar);
    let timecode_str = format_timecode(current_bar, bpm, beats_per_bar);
    let sig_text = format_sig(beats_per_bar, beat_division);

    egui::Frame::NONE
        .fill(Color32::from_rgb(58, 59, 64))
        .inner_margin(egui::Margin::symmetric(6, 4))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(20, 21, 24)))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;

                // 1. Cápsula independiente [FILE ◢]
                let file_popup_id = ui.make_persistent_id("bw_file_popup");
                let file_resp = capsule_text_button(
                    ui,
                    vec2(38.0, CAPSULE_H),
                    "FILE",
                    11.0,
                    false,
                    BW_ACCENT_ORANGE,
                    JoinSide::Standalone,
                    true,
                );
                if file_resp.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(file_popup_id));
                }
                egui::popup_below_widget(
                    ui,
                    file_popup_id,
                    &file_resp,
                    PopupCloseBehavior::CloseOnClickOutside,
                    |ui| {
                        ui.set_min_width(170.0);
                        if ui.button("New Project").clicked() {
                            project::reset(state);
                            state.project_path = None;
                            state.project_dirty = false;
                            ui.close_menu();
                        }
                        if ui.button("Open Project...").clicked() {
                            let cur = state.project_path.clone();
                            if let Some(p) = project::ask_open_path(cur.as_deref()) {
                                if project::load(state, &p).is_ok() {
                                    state.project_path = Some(p);
                                    state.project_dirty = false;
                                }
                            }
                            ui.close_menu();
                        }
                        if ui.button("Save Project").clicked() {
                            save_project_action(state);
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Audio Engine Settings...").clicked() {
                            state.audio_settings_state.is_open = true;
                            ui.close_menu();
                        }
                        if ui.button("VST3 / CLAP Plugins...").clicked() {
                            state.plugin_settings_state.is_open = true;
                            ui.close_menu();
                        }
                        if ui.button("MCP Server Settings...").clicked() {
                            state.mcp_port_settings_state.open();
                            ui.close_menu();
                        }
                        if ui.button("About Hikaru OpenLive").clicked() {
                            state.show_about = true;
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Exit").clicked() {
                            std::process::exit(0);
                        }
                    },
                );

                ui.add_space(5.0);

                // 2. Grupo unido de 5 segmentos: [PLAY ◢ | ▶ | ■ | ● | Auto]
                let play_popup_id = ui.make_persistent_id("bw_play_popup");
                let play_menu_resp = capsule_text_button(
                    ui,
                    vec2(40.0, CAPSULE_H),
                    "PLAY",
                    11.0,
                    false,
                    BW_ACCENT_ORANGE,
                    JoinSide::Left,
                    true,
                );
                if play_menu_resp.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(play_popup_id));
                }
                egui::popup_below_widget(
                    ui,
                    play_popup_id,
                    &play_menu_resp,
                    PopupCloseBehavior::CloseOnClickOutside,
                    |ui| {
                        ui.set_min_width(165.0);
                        if ui.button("Start / Pause Playback (Space)").clicked() {
                            trigger_play_or_pause(state);
                            ui.close_menu();
                        }
                        if ui.button("Stop & Return to Zero").clicked() {
                            state.stop();
                            ui.close_menu();
                        }
                        if ui.button("Stop All Launcher Clips").clicked() {
                            state.matrix_state.stop_all_clips(&state.audio_proxy);
                            ui.close_menu();
                        }
                    },
                );

                if capsule_icon_button(
                    ui,
                    vec2(34.0, CAPSULE_H),
                    is_playing,
                    BW_ACCENT_ORANGE,
                    JoinSide::Middle,
                    |p, r, fg| icons::paint_play_triangle(p, r, fg),
                )
                .on_hover_text("Play / Pause (Space)")
                .clicked()
                {
                    trigger_play_or_pause(state);
                }

                if capsule_icon_button(
                    ui,
                    vec2(34.0, CAPSULE_H),
                    false,
                    BW_ACCENT_ORANGE,
                    JoinSide::Middle,
                    |p, r, fg| icons::paint_stop_square(p, r, fg),
                )
                .on_hover_text("Stop")
                .clicked()
                {
                    state.transport.playback_state = TransportPlaybackState::Stopped;
                    state.transport.sample_count = 0;
                    crate::views::hikaru_native_plugins::hikaru_openmodulation::on_transport_pause_or_stop();
                    state.audio_proxy.send(GuiCommand::Stop);
                }

                if capsule_icon_button(
                    ui,
                    vec2(34.0, CAPSULE_H),
                    is_recording,
                    REC_RED,
                    JoinSide::Middle,
                    |p, r, fg| icons::paint_record_circle(p, r, fg, true),
                )
                .on_hover_text("Arranger Record")
                .clicked()
                {
                    state.is_recording = !state.is_recording;
                }

                if capsule_icon_button(
                    ui,
                    vec2(34.0, CAPSULE_H),
                    state.panel_layout.show_automation,
                    BW_ACCENT_ORANGE,
                    JoinSide::Right,
                    |p, r, fg| icons::paint_automation_nodes(p, r, fg, true),
                )
                .on_hover_text("Automation Write")
                .clicked()
                {
                    state.panel_layout.show_automation = !state.panel_layout.show_automation;
                }

                ui.add_space(5.0);

                // 3. Grupo unido de 2 segmentos: [Save | Resume Arranger]
                if capsule_icon_button(
                    ui,
                    vec2(32.0, CAPSULE_H),
                    state.project_dirty,
                    BW_ACCENT_ORANGE,
                    JoinSide::Left,
                    |p, r, fg| icons::paint_icon(p, r, icons::DEVICE_FLOPPY, fg, 1.4),
                )
                .on_hover_text("Save Project")
                .clicked()
                {
                    save_project_action(state);
                }

                if capsule_icon_button(
                    ui,
                    vec2(32.0, CAPSULE_H),
                    false,
                    BW_ACCENT_ORANGE,
                    JoinSide::Right,
                    |p, r, fg| icons::paint_clip_launcher_grid(p, r, fg),
                )
                .on_hover_text("Stop All Clip Launcher Tracks")
                .clicked()
                {
                    state.matrix_state.stop_all_clips(&state.audio_proxy);
                }

                ui.add_space(6.0);

                // 4. Pantalla LCD hundida en GPU (`paint_lcd_bezel`)
                let lcd_size = vec2(266.0, 32.0);
                let (lcd_rect, _) = ui.allocate_exact_size(lcd_size, Sense::hover());
                if ui.is_rect_visible(lcd_rect) {
                    paint_lcd_bezel(ui.painter(), lcd_rect, 3.0);
                }

                // Sub-sección 4a: Chip DSP + Flechas I/O + mini medidor maestro (ancho 38)
                let col_dsp = Rect::from_min_size(lcd_rect.min, vec2(38.0, lcd_rect.height()));
                let dsp_resp = ui.interact(col_dsp, ui.id().with("lcd_dsp_io"), Sense::click());
                if ui.is_rect_visible(col_dsp) {
                    let p = ui.painter();
                    let chip_r = Rect::from_min_size(pos2(col_dsp.min.x + 4.0, col_dsp.min.y + 3.0), vec2(13.0, 13.0));
                    let io_r = Rect::from_min_size(pos2(col_dsp.min.x + 4.0, col_dsp.min.y + 16.0), vec2(13.0, 13.0));
                    let icon_col = if dsp_resp.hovered() { Color32::WHITE } else { BW_LCD_CYAN };
                    icons::paint_cpu_chip(p, chip_r, icon_col);
                    icons::paint_io_arrows(p, io_r, icon_col);

                    // Barras horizontales gemelas de carga DSP / pico maestro
                    let meter_x = col_dsp.min.x + 20.0;
                    let cpu_w = (state.cpu_usage.clamp(0.05, 1.0) * 14.0).round();
                    let pk_w = (state.smoothed_master_peak.clamp(0.0, 1.0) * 14.0).round();
                    p.rect_filled(
                        Rect::from_min_size(pos2(meter_x, col_dsp.min.y + 7.0), vec2(14.0, 3.0)),
                        0.5,
                        Color32::from_rgb(38, 42, 48),
                    );
                    p.rect_filled(
                        Rect::from_min_size(pos2(meter_x, col_dsp.min.y + 7.0), vec2(cpu_w, 3.0)),
                        0.5,
                        BW_LCD_CYAN,
                    );
                    p.rect_filled(
                        Rect::from_min_size(pos2(meter_x, col_dsp.min.y + 20.0), vec2(14.0, 3.0)),
                        0.5,
                        Color32::from_rgb(38, 42, 48),
                    );
                    p.rect_filled(
                        Rect::from_min_size(pos2(meter_x, col_dsp.min.y + 20.0), vec2(pk_w, 3.0)),
                        0.5,
                        PLAY_GREEN,
                    );

                    p.line_segment(
                        [pos2(col_dsp.max.x, lcd_rect.min.y + 2.0), pos2(col_dsp.max.x, lcd_rect.max.y - 2.0)],
                        Stroke::new(1.0_f32, Color32::from_rgb(36, 38, 44)),
                    );
                }
                if dsp_resp.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    set_footer_cues(
                        state,
                        Some(("AUDIO ENGINE", "DSP & I/O Monitor")),
                        &[("CLICK", "Open Audio Engine Settings")],
                    );
                }
                if dsp_resp.clicked() {
                    state.audio_settings_state.is_open = !state.audio_settings_state.is_open;
                }

                // Sub-sección 4b: BPM (arriba) + Compás 4/4 (abajo) (ancho 58)
                let col_bpm = Rect::from_min_size(pos2(col_dsp.max.x, lcd_rect.min.y), vec2(58.0, lcd_rect.height()));
                let bpm_top = Rect::from_min_max(col_bpm.min, pos2(col_bpm.max.x, col_bpm.center().y));
                let sig_bot = Rect::from_min_max(pos2(col_bpm.min.x, col_bpm.center().y), col_bpm.max);

                if state.editing_bpm {
                    // Procesamiento de entrada de teclado para BPM estilo Bitwig
                    ui.input(|i| {
                        for event in &i.events {
                            if let egui::Event::Text(t) = event {
                                if state.bpm_input_first_key {
                                    state.bpm_input.clear();
                                    state.bpm_input_first_key = false;
                                }
                                for ch in t.chars() {
                                    if ch.is_ascii_digit() || (ch == '.' && !state.bpm_input.contains('.')) {
                                        if state.bpm_input.len() < 7 {
                                            state.bpm_input.push(ch);
                                        }
                                    }
                                }
                            }
                        }
                        if i.key_pressed(egui::Key::Backspace) {
                            if state.bpm_input_first_key {
                                state.bpm_input.clear();
                                state.bpm_input_first_key = false;
                            } else {
                                state.bpm_input.pop();
                            }
                        }
                        if i.key_pressed(egui::Key::Escape) {
                            state.editing_bpm = false;
                        }
                        if i.key_pressed(egui::Key::Enter) {
                            if let Ok(v) = state.bpm_input.trim().parse::<f64>() {
                                state.set_bpm(v);
                            }
                            state.editing_bpm = false;
                        }
                    });

                    let display_str = if state.bpm_input.is_empty() {
                        " ".to_string()
                    } else {
                        state.bpm_input.clone()
                    };

                    let font = fonts::lcd(13.0);
                    let galley = ui.painter().layout_no_wrap(
                        display_str,
                        font,
                        Color32::from_rgb(18, 20, 24),
                    );
                    let text_w = galley.size().x.max(8.0);
                    let text_right = col_bpm.max.x - 6.0;
                    let text_left = text_right - text_w;
                    let text_top = bpm_top.center().y - galley.size().y * 0.5;
                    let pill_rect = Rect::from_min_max(
                        pos2(text_left - 3.0, bpm_top.min.y + 1.0),
                        pos2(text_right + 2.0, bpm_top.max.y - 1.0),
                    );

                    // Clic fuera de la pastilla confirma y sale del modo edición
                    ui.input(|i| {
                        if i.pointer.any_pressed() {
                            if let Some(pos) = i.pointer.interact_pos() {
                                if !pill_rect.expand(2.0).contains(pos) {
                                    if let Ok(v) = state.bpm_input.trim().parse::<f64>() {
                                        state.set_bpm(v);
                                    }
                                    state.editing_bpm = false;
                                }
                            }
                        }
                    });

                    if ui.is_rect_visible(pill_rect) {
                        let p = ui.painter();
                        // Pastilla de selección invertida idéntica a Bitwig Studio (fondo blanco, texto negro)
                        p.rect_filled(pill_rect, 1.5, Color32::WHITE);
                        p.galley(pos2(text_left, text_top), galley, Color32::from_rgb(18, 20, 24));

                        // Cursor parpadeante si ya comenzó a escribir
                        if !state.bpm_input_first_key {
                            let time = ui.input(|i| i.time);
                            if (time * 2.0).fract() < 0.5 {
                                let cursor_x = text_right + 0.5;
                                p.line_segment(
                                    [pos2(cursor_x, pill_rect.min.y + 2.0), pos2(cursor_x, pill_rect.max.y - 2.0)],
                                    Stroke::new(1.2_f32, Color32::from_rgb(18, 20, 24)),
                                );
                            }
                        }
                    }

                    ui.ctx().set_cursor_icon(CursorIcon::Text);
                    set_footer_cues(
                        state,
                        Some(("TEMPO", "Type Exact BPM")),
                        &[("ENTER", "Confirm BPM"), ("ESC", "Cancel")],
                    );
                } else {
                    let bpm_resp = ui.interact(bpm_top, ui.id().with("lcd_bpm_drag"), Sense::click_and_drag());
                    if bpm_resp.double_clicked() {
                        state.editing_bpm = true;
                        state.bpm_input = format!("{:.2}", state.transport.bpm);
                        state.bpm_input_first_key = true;
                    } else if bpm_resp.dragged() {
                        let fine = ui.input(|i| i.modifiers.shift);
                        let step = if fine { 0.01 } else { 0.10 };
                        let delta_y = bpm_resp.drag_delta().y as f64;
                        if delta_y.abs() > 0.0 {
                            let next = (state.transport.bpm - delta_y * step).clamp(crate::app::BPM_MIN, crate::app::BPM_MAX);
                            state.set_bpm(next);
                        }
                    }

                    if bpm_resp.dragged() {
                        ui.ctx().set_cursor_icon(CursorIcon::None);
                    } else if bpm_resp.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
                        set_footer_cues(
                            state,
                            Some(("TEMPO", &format!("{:.2} BPM", state.transport.bpm))),
                            &[
                                ("DRAG", "Adjust BPM (Up/Down)"),
                                ("SHIFT+DRAG", "Fine Adjust (0.01)"),
                                ("DBL-CLICK", "Type Exact BPM"),
                            ],
                        );
                    }

                    if ui.is_rect_visible(bpm_top) {
                        let bpm_col = if bpm_resp.hovered() || bpm_resp.dragged() {
                            Color32::WHITE
                        } else {
                            BW_LCD_CYAN
                        };
                        ui.painter().text(
                            pos2(col_bpm.max.x - 6.0, bpm_top.center().y + 0.5),
                            Align2::RIGHT_CENTER,
                            format!("{:.2}", state.transport.bpm),
                            fonts::lcd(13.0),
                            bpm_col,
                        );
                    }
                }

                if state.editing_sig {
                    // Procesamiento de entrada de teclado para Compás estilo Bitwig
                    ui.input(|i| {
                        for event in &i.events {
                            if let egui::Event::Text(t) = event {
                                if state.sig_input_first_key {
                                    state.sig_input.clear();
                                    state.sig_input_first_key = false;
                                }
                                for ch in t.chars() {
                                    if ch.is_ascii_digit() || (ch == '/' && !state.sig_input.contains('/')) {
                                        if state.sig_input.len() < 6 {
                                            state.sig_input.push(ch);
                                        }
                                    }
                                }
                            }
                        }
                        if i.key_pressed(egui::Key::Backspace) {
                            if state.sig_input_first_key {
                                state.sig_input.clear();
                                state.sig_input_first_key = false;
                            } else {
                                state.sig_input.pop();
                            }
                        }
                        if i.key_pressed(egui::Key::Escape) {
                            state.editing_sig = false;
                        }
                        if i.key_pressed(egui::Key::Enter) {
                            if let Some((n, d)) = crate::app::parse_sig(&state.sig_input) {
                                state.set_sig(n, d);
                            }
                            state.editing_sig = false;
                        }
                    });

                    let display_str = if state.sig_input.is_empty() {
                        " ".to_string()
                    } else {
                        state.sig_input.clone()
                    };

                    let font = fonts::lcd(11.5);
                    let galley = ui.painter().layout_no_wrap(
                        display_str,
                        font,
                        Color32::from_rgb(18, 20, 24),
                    );
                    let text_w = galley.size().x.max(8.0);
                    let text_right = col_bpm.max.x - 6.0;
                    let text_left = text_right - text_w;
                    let text_top = sig_bot.center().y - galley.size().y * 0.5;
                    let pill_rect = Rect::from_min_max(
                        pos2(text_left - 3.0, sig_bot.min.y + 1.0),
                        pos2(text_right + 2.0, sig_bot.max.y - 1.0),
                    );

                    // Clic fuera de la pastilla confirma y sale del modo edición
                    ui.input(|i| {
                        if i.pointer.any_pressed() {
                            if let Some(pos) = i.pointer.interact_pos() {
                                if !pill_rect.expand(2.0).contains(pos) {
                                    if let Some((n, d)) = crate::app::parse_sig(&state.sig_input) {
                                        state.set_sig(n, d);
                                    }
                                    state.editing_sig = false;
                                }
                            }
                        }
                    });

                    if ui.is_rect_visible(pill_rect) {
                        let p = ui.painter();
                        p.rect_filled(pill_rect, 1.5, Color32::WHITE);
                        p.galley(pos2(text_left, text_top), galley, Color32::from_rgb(18, 20, 24));

                        if !state.sig_input_first_key {
                            let time = ui.input(|i| i.time);
                            if (time * 2.0).fract() < 0.5 {
                                let cursor_x = text_right + 0.5;
                                p.line_segment(
                                    [pos2(cursor_x, pill_rect.min.y + 2.0), pos2(cursor_x, pill_rect.max.y - 2.0)],
                                    Stroke::new(1.2_f32, Color32::from_rgb(18, 20, 24)),
                                );
                            }
                        }
                    }

                    ui.ctx().set_cursor_icon(CursorIcon::Text);
                    set_footer_cues(
                        state,
                        Some(("TIME SIGNATURE", "Type Signature (e.g. 3/4)")),
                        &[("ENTER", "Confirm Signature"), ("ESC", "Cancel")],
                    );
                } else {
                    let sig_resp = ui.interact(sig_bot, ui.id().with("lcd_sig_btn"), Sense::click_and_drag());
                    if sig_resp.double_clicked() {
                        state.editing_sig = true;
                        state.sig_input = format_sig(state.transport.beats_per_bar, state.transport.beat_division);
                        state.sig_input_first_key = true;
                    } else if sig_resp.dragged() {
                        let delta_y = sig_resp.drag_delta().y;
                        if delta_y.abs() >= 3.0 {
                            let step = if delta_y < 0.0 { 1 } else { -1 };
                            if ui.input(|i| i.modifiers.shift) {
                                let divs = [2, 4, 8, 16];
                                let cur_idx = divs.iter().position(|&d| d == state.transport.beat_division).unwrap_or(1);
                                let next_idx = (cur_idx as isize + step as isize).clamp(0, divs.len() as isize - 1) as usize;
                                state.set_sig(state.transport.beats_per_bar, divs[next_idx]);
                            } else {
                                let next_num = (state.transport.beats_per_bar as i32 + step).clamp(1, 32) as u32;
                                state.set_sig(next_num, state.transport.beat_division);
                            }
                        }
                    } else if sig_resp.clicked() {
                        let (num, den) = step_sig(state.transport.beats_per_bar, state.transport.beat_division, 1);
                        state.set_sig(num, den);
                    }

                    if sig_resp.dragged() {
                        ui.ctx().set_cursor_icon(CursorIcon::None);
                    } else if sig_resp.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
                        set_footer_cues(
                            state,
                            Some(("TIME SIGNATURE", &sig_text)),
                            &[
                                ("CLICK", "Cycle Common Signatures"),
                                ("DRAG", "Change Numerator"),
                                ("SHIFT+DRAG", "Change Denominator"),
                                ("DBL-CLICK", "Type Signature"),
                            ],
                        );
                    }

                    if ui.is_rect_visible(sig_bot) {
                        let sig_col = if sig_resp.hovered() || sig_resp.dragged() {
                            Color32::WHITE
                        } else {
                            BW_LCD_CYAN
                        };
                        ui.painter().text(
                            pos2(col_bpm.max.x - 6.0, sig_bot.center().y - 0.5),
                            Align2::RIGHT_CENTER,
                            &sig_text,
                            fonts::lcd(11.5),
                            sig_col,
                        );
                    }
                }

                if ui.is_rect_visible(col_bpm) {
                    ui.painter().line_segment(
                        [pos2(col_bpm.max.x, lcd_rect.min.y + 2.0), pos2(col_bpm.max.x, lcd_rect.max.y - 2.0)],
                        Stroke::new(1.0_f32, Color32::from_rgb(36, 38, 44)),
                    );
                }

                // Sub-sección 4c: Posición musical 1.1.1.00 + Timecode 0:00.000 (ancho 84)
                let col_pos = Rect::from_min_size(pos2(col_bpm.max.x, lcd_rect.min.y), vec2(84.0, lcd_rect.height()));
                let pos_top = Rect::from_min_max(col_pos.min, pos2(col_pos.max.x, col_pos.center().y));
                let time_bot = Rect::from_min_max(pos2(col_pos.min.x, col_pos.center().y), col_pos.max);

                let pos_resp = ui.interact(pos_top, ui.id().with("lcd_pos_scrub"), Sense::click_and_drag());
                if pos_resp.double_clicked() {
                    state.transport.sample_count = 0;
                    state.audio_proxy.send(GuiCommand::Seek { sample_count: 0 });
                } else if pos_resp.dragged() {
                    let delta_y = pos_resp.drag_delta().y as f64;
                    if delta_y.abs() > 0.0 {
                        let fine = ui.input(|i| i.modifiers.shift);
                        let samples_per_step = if fine {
                            samples_per_beat * 0.25
                        } else {
                            samples_per_beat
                        };
                        let delta_samples = (-delta_y * (samples_per_step / 10.0)).round() as i64;
                        let next_sc = (state.transport.sample_count as i64 + delta_samples).max(0) as u64;
                        state.transport.sample_count = next_sc;
                        state.audio_proxy.send(GuiCommand::Seek { sample_count: next_sc });
                    }
                }

                if pos_resp.dragged() {
                    ui.ctx().set_cursor_icon(CursorIcon::None);
                } else if pos_resp.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
                    set_footer_cues(
                        state,
                        Some(("PLAYHEAD POSITION", &musical_pos)),
                        &[
                            ("DRAG", "Scrub Position (Bars/Beats)"),
                            ("SHIFT+DRAG", "Fine Scrub (16ths)"),
                            ("DBL-CLICK", "Rewind to Start"),
                        ],
                    );
                }

                let time_resp = ui.interact(time_bot, ui.id().with("lcd_time_scrub"), Sense::click_and_drag());
                if time_resp.double_clicked() {
                    state.transport.sample_count = 0;
                    state.audio_proxy.send(GuiCommand::Seek { sample_count: 0 });
                } else if time_resp.dragged() {
                    let delta_y = time_resp.drag_delta().y as f64;
                    if delta_y.abs() > 0.0 {
                        let fine = ui.input(|i| i.modifiers.shift);
                        let secs_per_px = if fine { 0.02 } else { 0.20 };
                        let delta_samples = (-delta_y * secs_per_px * sample_rate).round() as i64;
                        let next_sc = (state.transport.sample_count as i64 + delta_samples).max(0) as u64;
                        state.transport.sample_count = next_sc;
                        state.audio_proxy.send(GuiCommand::Seek { sample_count: next_sc });
                    }
                }

                if time_resp.dragged() {
                    ui.ctx().set_cursor_icon(CursorIcon::None);
                } else if time_resp.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
                    set_footer_cues(
                        state,
                        Some(("TIMECODE", &timecode_str)),
                        &[
                            ("DRAG", "Scrub Timecode (Seconds)"),
                            ("SHIFT+DRAG", "Fine Scrub (0.02s)"),
                            ("DBL-CLICK", "Rewind to Start"),
                        ],
                    );
                }

                if ui.is_rect_visible(col_pos) {
                    let p = ui.painter();
                    let pos_col = if pos_resp.hovered() || pos_resp.dragged() {
                        Color32::WHITE
                    } else {
                        BW_LCD_CYAN
                    };
                    p.text(
                        pos2(col_pos.max.x - 6.0, col_pos.min.y + 8.5),
                        Align2::RIGHT_CENTER,
                        &musical_pos,
                        fonts::lcd(13.0),
                        pos_col,
                    );
                    let time_col = if time_resp.hovered() || time_resp.dragged() {
                        Color32::WHITE
                    } else {
                        BW_LCD_CYAN
                    };
                    p.text(
                        pos2(col_pos.max.x - 6.0, col_pos.max.y - 8.5),
                        Align2::RIGHT_CENTER,
                        &timecode_str,
                        fonts::lcd(11.5),
                        time_col,
                    );
                    p.line_segment(
                        [pos2(col_pos.max.x, lcd_rect.min.y + 2.0), pos2(col_pos.max.x, lcd_rect.max.y - 2.0)],
                        Stroke::new(1.0_f32, Color32::from_rgb(36, 38, 44)),
                    );
                }

                // Sub-sección 4d: Metrónomo dentro del LCD (ancho 26)
                let col_metro = Rect::from_min_size(pos2(col_pos.max.x, lcd_rect.min.y), vec2(26.0, lcd_rect.height()));
                let metro_resp = ui.interact(col_metro, ui.id().with("lcd_metro"), Sense::click());
                if metro_resp.clicked() {
                    state.metronome_enabled = !state.metronome_enabled;
                }
                if metro_resp.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    set_footer_cues(
                        state,
                        Some(("METRONOME", if state.metronome_enabled { "ON" } else { "OFF" })),
                        &[("CLICK", "Toggle Metronome Click")],
                    );
                }
                if ui.is_rect_visible(col_metro) {
                    let p = ui.painter();
                    let col = if state.metronome_enabled {
                        BW_LCD_CYAN
                    } else if metro_resp.hovered() {
                        Color32::WHITE
                    } else {
                        Color32::from_rgb(112, 116, 124)
                    };
                    icons::paint_icon(p, col_metro.shrink(5.5), icons::METRONOME, col, 1.3);
                    p.line_segment(
                        [pos2(col_metro.max.x, lcd_rect.min.y + 2.0), pos2(col_metro.max.x, lcd_rect.max.y - 2.0)],
                        Stroke::new(1.0_f32, Color32::from_rgb(36, 38, 44)),
                    );
                }

                // Sub-sección 4e: Arranger Loop + Punch In/Out dentro del LCD (ancho 60)
                let col_loop = Rect::from_min_max(pos2(col_metro.max.x, lcd_rect.min.y), lcd_rect.max);
                let loop_box = Rect::from_min_size(pos2(col_loop.min.x + 3.0, col_loop.min.y), vec2(28.0, col_loop.height()));
                let punch_in_box = Rect::from_min_size(pos2(loop_box.max.x + 3.0, col_loop.min.y + 2.0), vec2(21.0, 13.0));
                let punch_out_box = Rect::from_min_size(pos2(loop_box.max.x + 3.0, col_loop.center().y + 1.0), vec2(21.0, 13.0));

                let loop_resp = ui.interact(loop_box, ui.id().with("lcd_loop_toggle"), Sense::click());
                if loop_resp.clicked() {
                    state.is_looping = !state.is_looping;
                    state.playlist_state.loop_region_active = state.is_looping;
                    state.playlist_state.ensure_minimum_global_loop(state.is_looping);
                }
                if loop_resp.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    set_footer_cues(
                        state,
                        Some(("LOOP", if is_looping { "ON" } else { "OFF" })),
                        &[("CLICK", "Toggle Arranger Loop Region")],
                    );
                }

                let punch_in_resp = ui.interact(punch_in_box, ui.id().with("lcd_punch_in"), Sense::click());
                if punch_in_resp.clicked() {
                    state.punch_in = !state.punch_in;
                }
                if punch_in_resp.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    set_footer_cues(
                        state,
                        Some(("PUNCH IN", if state.punch_in { "ARMED" } else { "OFF" })),
                        &[("CLICK", "Toggle Punch In Recording")],
                    );
                }

                let punch_out_resp = ui.interact(punch_out_box, ui.id().with("lcd_punch_out"), Sense::click());
                if punch_out_resp.clicked() {
                    state.punch_out = !state.punch_out;
                }
                if punch_out_resp.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    set_footer_cues(
                        state,
                        Some(("PUNCH OUT", if state.punch_out { "ARMED" } else { "OFF" })),
                        &[("CLICK", "Toggle Punch Out Recording")],
                    );
                }

                if ui.is_rect_visible(col_loop) {
                    let p = ui.painter();
                    let loop_col = if is_looping {
                        BW_LCD_CYAN
                    } else if loop_resp.hovered() {
                        Color32::WHITE
                    } else {
                        Color32::from_rgb(112, 116, 124)
                    };
                    icons::paint_loop_pill(p, loop_box.shrink(4.0), loop_col);

                    let punch_in_col = if state.punch_in {
                        BW_LCD_CYAN
                    } else if punch_in_resp.hovered() {
                        Color32::WHITE
                    } else {
                        Color32::from_rgb(112, 116, 124)
                    };
                    icons::paint_punch_step(p, punch_in_box.shrink(2.0), punch_in_col, true);

                    let punch_out_col = if state.punch_out {
                        BW_LCD_CYAN
                    } else if punch_out_resp.hovered() {
                        Color32::WHITE
                    } else {
                        Color32::from_rgb(112, 116, 124)
                    };
                    icons::paint_punch_step(p, punch_out_box.shrink(2.0), punch_out_col, false);
                }

                ui.add_space(6.0);

                // 5. Cápsula independiente [ADD ◢]
                let add_popup_id = ui.make_persistent_id("bw_add_popup");
                let add_resp = capsule_text_button(
                    ui,
                    vec2(38.0, CAPSULE_H),
                    "ADD",
                    11.0,
                    false,
                    BW_ACCENT_ORANGE,
                    JoinSide::Standalone,
                    true,
                );
                if add_resp.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(add_popup_id));
                }
                egui::popup_below_widget(
                    ui,
                    add_popup_id,
                    &add_resp,
                    PopupCloseBehavior::CloseOnClickOutside,
                    |ui| {
                        ui.set_min_width(175.0);
                        if ui.button("Add Audio / Instrument Track").clicked() {
                            add_audio_track_action(state);
                            ui.close_menu();
                        }
                        if ui.button("Add Launcher Scene").clicked() {
                            state.matrix_state.add_scene();
                            ui.close_menu();
                        }
                        ui.separator();
                        let track_idx = state.safe_track_index();
                        for plugin in &PLUGIN_CATALOG {
                            if ui.button(format!("Insert {}", plugin.label)).clicked() {
                                if let Some(t) = state.current_tracks_mut().get_mut(track_idx) {
                                    let mut s = DspSlot::default();
                                    s.name = plugin.name.to_string();
                                    t.effects.push(s);
                                    state.selected_slot_index = t.effects.len() - 1;
                                    state.show_dsp_rack = true;
                                }
                                ui.close_menu();
                            }
                        }
                    },
                );

                ui.add_space(5.0);

                // 6. Grupo unido de 5 segmentos: [EDIT ◢ | Undo | Redo | Dup | Delete]
                let edit_popup_id = ui.make_persistent_id("bw_edit_popup");
                let edit_resp = capsule_text_button(
                    ui,
                    vec2(38.0, CAPSULE_H),
                    "EDIT",
                    11.0,
                    false,
                    BW_ACCENT_ORANGE,
                    JoinSide::Left,
                    true,
                );
                if edit_resp.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(edit_popup_id));
                }
                egui::popup_below_widget(
                    ui,
                    edit_popup_id,
                    &edit_resp,
                    PopupCloseBehavior::CloseOnClickOutside,
                    |ui| {
                        ui.set_min_width(155.0);
                        if ui.button("Cut Selected Clip").clicked() {
                            crate::views::playlist::cut_selected_clips(&mut state.playlist_state);
                            ui.close_menu();
                        }
                        if ui.button("Copy Selected Clip").clicked() {
                            crate::views::playlist::copy_selected_clips(&mut state.playlist_state);
                            ui.close_menu();
                        }
                        if ui.button("Paste Clip").clicked() {
                            let target_tick = state.playlist_state.paste_tick();
                            crate::views::playlist::paste_clips(
                                &mut state.playlist_state,
                                target_tick,
                                state.selected_track_index,
                            );
                            ui.close_menu();
                        }
                    },
                );

                let _ = capsule_icon_button(
                    ui,
                    vec2(32.0, CAPSULE_H),
                    false,
                    BW_ACCENT_ORANGE,
                    JoinSide::Middle,
                    |p, r, fg| icons::paint_icon(p, r, icons::UNDO, fg, 1.4),
                )
                .on_hover_text("Undo");

                let _ = capsule_icon_button(
                    ui,
                    vec2(32.0, CAPSULE_H),
                    false,
                    BW_ACCENT_ORANGE,
                    JoinSide::Middle,
                    |p, r, fg| icons::paint_icon(p, r, icons::REDO, fg, 1.4),
                )
                .on_hover_text("Redo");

                if capsule_icon_button(
                    ui,
                    vec2(32.0, CAPSULE_H),
                    false,
                    BW_ACCENT_ORANGE,
                    JoinSide::Middle,
                    |p, r, fg| icons::paint_icon(p, r, icons::DUPLICATE_PLUS, fg, 1.35),
                )
                .on_hover_text("Duplicate Selected Clip")
                .clicked()
                {
                    crate::views::playlist::copy_selected_clips(&mut state.playlist_state);
                    let target_tick = state.playlist_state.paste_tick();
                    crate::views::playlist::paste_clips(
                        &mut state.playlist_state,
                        target_tick,
                        state.selected_track_index,
                    );
                }

                if capsule_icon_button(
                    ui,
                    vec2(32.0, CAPSULE_H),
                    false,
                    REC_RED,
                    JoinSide::Right,
                    |p, r, fg| icons::paint_icon(p, r, icons::CIRCLE_X, fg, 1.35),
                )
                .on_hover_text("Delete Selected Clip")
                .clicked()
                {
                    let selected = state.playlist_state.selected_clips.clone();
                    state
                        .playlist_state
                        .clips
                        .retain(|(_, c)| !selected.contains(&c.id));
                    state.playlist_state.selected_clips.clear();
                }

                ui.add_space(5.0);

                // 7. Cápsula independiente [TRACK ◢]
                let track_popup_id = ui.make_persistent_id("bw_track_popup");
                let track_resp = capsule_text_button(
                    ui,
                    vec2(46.0, CAPSULE_H),
                    "TRACK",
                    11.0,
                    false,
                    BW_ACCENT_ORANGE,
                    JoinSide::Standalone,
                    true,
                );
                if track_resp.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(track_popup_id));
                }
                egui::popup_below_widget(
                    ui,
                    track_popup_id,
                    &track_resp,
                    PopupCloseBehavior::CloseOnClickOutside,
                    |ui| {
                        ui.set_min_width(165.0);
                        if ui.button("Add Track").clicked() {
                            add_audio_track_action(state);
                            ui.close_menu();
                        }
                        if ui.button("Open Track Devices (F10)").clicked() {
                            state.show_dsp_rack = true;
                            ui.close_menu();
                        }
                        if ui.button("Clear All Mute / Solo").clicked() {
                            for tr in &mut state.matrix_state.tracks {
                                tr.muted = false;
                                tr.soloed = false;
                            }
                            for tr in &mut state.live_tracks {
                                tr.mute = false;
                                tr.solo = false;
                            }
                            ui.close_menu();
                        }
                    },
                );
            });
        });

    let _ = (FLAG_ACTIVE, AppMode::OpenLive);
}
