// Copyright (C) Hikaru Corporation - 2026
// Hikaru OpenLive - Audio Clip Editor
// GNU Affero General Public License v3
// crates/hikaru_gui/src/views/clip_editor.rs

use egui::{Align2, Color32, ComboBox, CursorIcon, FontId, Frame, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use std::path::PathBuf;

use crate::audio_proxy::AudioProxy;
use crate::views::matrix::{self, AudioEvent, MatrixClip, SessionMatrixState};
use crate::views::waveform;
use hikaru_audio_engine::AudioEngine;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SnapValue {
    None,
    Bar1,
    Beat1_2,
    Beat1_4,
    Beat1_8,
    Beat1_16,
}

impl SnapValue {
    pub fn label(&self) -> &'static str {
        match self {
            SnapValue::None => "Off (Free)",
            SnapValue::Bar1 => "1 Bar",
            SnapValue::Beat1_2 => "1/2 Beat",
            SnapValue::Beat1_4 => "1/4 Beat",
            SnapValue::Beat1_8 => "1/8 Beat",
            SnapValue::Beat1_16 => "1/16 Beat",
        }
    }

    pub fn interval_secs(&self, bpm: f32) -> f64 {
        let current_bpm = if bpm > 0.0 { bpm as f64 } else { 120.0 };
        let sec_per_beat = 60.0 / current_bpm;

        match self {
            SnapValue::None => 0.0,
            SnapValue::Bar1 => sec_per_beat * 4.0,
            SnapValue::Beat1_2 => sec_per_beat * 2.0,
            SnapValue::Beat1_4 => sec_per_beat,
            SnapValue::Beat1_8 => sec_per_beat / 2.0,
            SnapValue::Beat1_16 => sec_per_beat / 4.0,
        }
    }
}

// Un solo Enum mutuamente excluyente para la herramienta activa en el Inspector
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InspectorToolMode {
    AudioEvents,
    Comping,
    Stretch,
    Onsets,
    Gain,
    Pan,
    Pitch,
    Formant,
}

pub fn show(
    ui: &mut Ui,
    clip: &mut MatrixClip,
    track_idx: usize,
    scene_idx: usize,
    mut audio_engine: Option<&mut AudioEngine>,
    playhead: Option<(u64, u64)>,
    sample_rate: u32,
    bpm: f32,
) {
    let mut loop_changed = false;
    let mut events_changed = false;

    // 1. Decodificación a eventos: si el pad aún no tiene eventos pero sí
    // archivo, crear el primer evento (región completa). Los pads ya no son
    // un buffer fijo: son una lista de eventos editables.
    if clip.audio_events().is_empty() && clip.path.exists() {
        if let Some((samples, channels, sr)) = matrix::decode_audio_file(&clip.path) {
            let id = clip.alloc_event_id();
            let name = clip
                .path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if let Some(events) = clip.audio_events_mut() {
                events.push(AudioEvent::new_full(
                    id, name, samples, channels, sr, 0.0,
                ));
            }
            clip.refresh_preview();
            events_changed = true;
        }
    }

    let snap_id = ui.make_persistent_id("clip_editor_snap_grid");
    let mut current_snap: SnapValue = ui.data_mut(|d| d.get_temp(snap_id).unwrap_or(SnapValue::Beat1_4));

    // Estado ÚNICO para la herramienta seleccionada
    let tool_id = ui.make_persistent_id("inspector_active_tool");
    let mut active_tool: InspectorToolMode = ui.data_mut(|d| d.get_temp(tool_id).unwrap_or(InspectorToolMode::Pitch));

    let auto_fades_id = ui.make_persistent_id("inspector_auto_fades");
    let mut auto_fades: bool = ui.data_mut(|d| d.get_temp(auto_fades_id).unwrap_or(true));

    // Zoom horizontal del canvas (1.0 = Fit al clip, <1.0 = ver más
    // contexto, >1.0 = ampliado con scroll). El canvas siempre muestra
    // una cola vacía más allá del fin del clip, estilo Ableton/Bitwig.
    let zoom_id = ui.make_persistent_id("clip_editor_zoom_factor");
    let mut zoom_factor: f32 = ui.data_mut(|d| d.get_temp(zoom_id).unwrap_or(1.0_f32));
    zoom_factor = zoom_factor.clamp(0.25, 32.0);

    Frame::none()
        .fill(Color32::from_rgb(18, 18, 22))
        .stroke(Stroke::new(1.0_f32, Color32::from_gray(45)))
        .inner_margin(6.0)
        .show(ui, |ui| {
            // Header
            ui.horizontal(|ui| {
                ui.label(format!("🎵 {}", clip.name));
                ui.weak(format!("({:.2}s)", clip.duration_secs));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.toggle_value(&mut clip.loop_enabled, "🔁 Loop").changed() {
                        loop_changed = true;
                    }

                    ui.add_space(8.0);

                    ComboBox::from_id_source("snap_combo_box")
                        .selected_text(format!("🧲 Snap: {}", current_snap.label()))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut current_snap, SnapValue::None, SnapValue::None.label());
                            ui.selectable_value(&mut current_snap, SnapValue::Bar1, SnapValue::Bar1.label());
                            ui.selectable_value(&mut current_snap, SnapValue::Beat1_2, SnapValue::Beat1_2.label());
                            ui.selectable_value(&mut current_snap, SnapValue::Beat1_4, SnapValue::Beat1_4.label());
                            ui.selectable_value(&mut current_snap, SnapValue::Beat1_8, SnapValue::Beat1_8.label());
                            ui.selectable_value(&mut current_snap, SnapValue::Beat1_16, SnapValue::Beat1_16.label());
                        });

                    ui.data_mut(|d| d.insert_temp(snap_id, current_snap));
                });
            });

            // --- Toolbar de eventos: el pad ya no es fijo ---
            let sel_id = ui.make_persistent_id(format!("clip_ev_sel_{}", clip.id));
            let mut selected_event: Option<u64> = ui.data_mut(|d| d.get_temp(sel_id).unwrap_or(None));
            let cb_id = ui.make_persistent_id("clip_editor_event_clipboard");
            let mut event_clipboard: Vec<AudioEvent> =
                ui.data_mut(|d| d.get_temp(cb_id).unwrap_or(Vec::new()));

            ui.horizontal(|ui| {
                let n = clip.audio_events().len();
                ui.label(format!("🧩 Events: {}", n));
                ui.add_space(6.0);
                let has_sel = selected_event
                    .and_then(|id| clip.audio_events().iter().find(|e| e.id == id))
                    .is_some();
                if ui
                    .small_button("✂ Split (S)")
                    .on_hover_text("S: dividir en el puntero (con Snap) · Click con S: cortar donde clickeás · Doble-click: dividir · Alt: sin Snap")
                    .clicked()
                {
                    if let Some(id) = selected_event {
                        if let Some(pos) = clip.audio_events().iter().position(|e| e.id == id) {
                            let (s, e) = {
                                let ev = &clip.audio_events()[pos];
                                (ev.start_secs, ev.end_secs())
                            };
                            let mid = (s + e) * 0.5;
                            let new_id = clip.alloc_event_id();
                            let new_name = format!("{}#{}", clip.name, new_id);
                            if let Some(events) = clip.audio_events_mut() {
                                if let Some(right) =
                                    events[pos].split_at(mid, new_id, new_name)
                                {
                                    let rid = right.id;
                                    events.push(right);
                                    events.sort_by(|a, b| {
                                        a.start_secs
                                            .partial_cmp(&b.start_secs)
                                            .unwrap_or(std::cmp::Ordering::Equal)
                                    });
                                    selected_event = Some(rid);
                                    clip.refresh_preview();
                                    events_changed = true;
                                }
                            }
                        }
                    }
                }
                if ui
                    .add_enabled(has_sel, egui::Button::new("📋 Copy").small())
                    .on_hover_text("Copiar evento seleccionado")
                    .clicked()
                {
                    if let Some(id) = selected_event {
                        if let Some(ev) =
                            clip.audio_events().iter().find(|e| e.id == id).cloned()
                        {
                            event_clipboard = vec![ev];
                        }
                    }
                }
                if ui
                    .add_enabled(!event_clipboard.is_empty(), egui::Button::new("📥 Paste").small())
                    .on_hover_text("Pegar evento(s) al final del pad")
                    .clicked()
                {
                    let at = clip.audio_total_secs();
                    for mut ev in event_clipboard.clone() {
                        ev.id = clip.alloc_event_id();
                        ev.start_secs += at;
                        if let Some(events) = clip.audio_events_mut() {
                            events.push(ev);
                        }
                    }
                    if let Some(events) = clip.audio_events_mut() {
                        events.sort_by(|a, b| {
                            a.start_secs
                                .partial_cmp(&b.start_secs)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });
                    }
                    clip.refresh_preview();
                    events_changed = true;
                }
                if ui
                    .add_enabled(has_sel, egui::Button::new("⧉ Dupl").small())
                    .on_hover_text("Duplicar evento al final")
                    .clicked()
                {
                    if let Some(id) = selected_event {
                        if let Some(ev) =
                            clip.audio_events().iter().find(|e| e.id == id).cloned()
                        {
                            let mut dup = ev;
                            dup.id = clip.alloc_event_id();
                            dup.start_secs = clip.audio_total_secs();
                            if let Some(events) = clip.audio_events_mut() {
                                events.push(dup);
                                events.sort_by(|a, b| {
                                    a.start_secs
                                        .partial_cmp(&b.start_secs)
                                        .unwrap_or(std::cmp::Ordering::Equal)
                                });
                            }
                            clip.refresh_preview();
                            events_changed = true;
                        }
                    }
                }
                if ui
                    .add_enabled(has_sel, egui::Button::new("🗑 Del").small())
                    .on_hover_text("Eliminar evento seleccionado")
                    .clicked()
                {
                    if let Some(id) = selected_event {
                        if let Some(events) = clip.audio_events_mut() {
                            events.retain(|e| e.id != id);
                        }
                        selected_event = None;
                        clip.refresh_preview();
                        events_changed = true;
                    }
                }
                if ui
                    .small_button("➕ Add")
                    .on_hover_text("Añadir otro sample al mismo pad (multi-sample)")
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Audio Files", &["wav", "mp3", "flac", "ogg"])
                        .pick_file()
                    {
                        if let Some((samples, channels, sr)) =
                            matrix::decode_audio_file(&path)
                        {
                            let at = clip.audio_total_secs();
                            let nid = clip.alloc_event_id();
                            let nm = path
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            if let Some(events) = clip.audio_events_mut() {
                                events.push(AudioEvent::new_full(
                                    nid, nm, samples, channels, sr, at,
                                ));
                            }
                            clip.refresh_preview();
                            events_changed = true;
                        }
                    }
                }
                // Gain del evento seleccionado.
                if let Some(id) = selected_event {
                    if let Some(pos) =
                        clip.audio_events().iter().position(|e| e.id == id)
                    {
                        ui.add_space(6.0);
                        ui.label("Gain");
                        let mut g = clip.audio_events()[pos].gain;
                        if ui.add(egui::Slider::new(&mut g, 0.0..=2.0)).changed() {
                            if let Some(events) = clip.audio_events_mut() {
                                if let Some(ev) =
                                    events.iter_mut().find(|e| e.id == id)
                                {
                                    ev.gain = g;
                                }
                            }
                            clip.refresh_preview();
                            events_changed = true;
                        }
                    }
                }
            });
            ui.weak("Click: seleccionar · S o Click+S: cortar en el puntero · Arrastrar: mover libre (Alt = sin Snap) · Bordes: trim re-estirable · Doble-click: dividir · Supr: borrar · Ctrl+A: Time Selection total · Ctrl+Alt+A: quitar Time Selection · Arrastrar un archivo al pad apila otro evento");
            ui.data_mut(|d| {
                d.insert_temp(sel_id, selected_event);
                d.insert_temp(cb_id, event_clipboard);
            });

            ui.separator();

            // =========================================================
            //  DISPOSICIÓN PRINCIPAL: INSPECTOR LATERAL + WAVEFORM
            // =========================================================
            ui.horizontal(|ui| {
                // --- SIDEBAR LEFT: BITWIG INSPECTOR MENU ---
                Frame::none()
                    .fill(Color32::from_rgb(25, 25, 30))
                    .stroke(Stroke::new(1.0, Color32::from_gray(40)))
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        ui.set_width(130.0);
                        ui.vertical(|ui| {
                            ui.add_space(2.0);

                            // Modos principales
                            ui.selectable_value(&mut active_tool, InspectorToolMode::AudioEvents, "Audio Events");
                            ui.selectable_value(&mut active_tool, InspectorToolMode::Comping, "Comping");

                            ui.add_space(6.0);
                            ui.separator();
                            ui.add_space(6.0);

                            // Herramientas de Time & Pitch
                            ui.horizontal(|ui| {
                                ui.selectable_value(&mut active_tool, InspectorToolMode::Stretch, "Stretch");
                                ui.selectable_value(&mut active_tool, InspectorToolMode::Onsets, "Onsets");
                            });

                            ui.add_space(4.0);

                            // Matriz de edición de parámetros (Mutuamente excluyentes)
                            egui::Grid::new("inspector_params_grid")
                                .num_columns(2)
                                .spacing([4.0, 4.0])
                                .show(ui, |ui| {
                                    ui.selectable_value(&mut active_tool, InspectorToolMode::Gain, "Gain");
                                    ui.selectable_value(&mut active_tool, InspectorToolMode::Pan, "Pan");
                                    ui.end_row();

                                    ui.selectable_value(&mut active_tool, InspectorToolMode::Pitch, "Pitch");
                                    ui.selectable_value(&mut active_tool, InspectorToolMode::Formant, "Formant");
                                    ui.end_row();
                                });

                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(8.0);

                            // Toggle independiente para Auto-fades
                            ui.checkbox(&mut auto_fades, "Create auto-fades");
                        });
                    });

                // Persistir estado único
                ui.data_mut(|d| d.insert_temp(tool_id, active_tool));
                ui.data_mut(|d| d.insert_temp(auto_fades_id, auto_fades));

                ui.add_space(4.0);

                // --- CANVAS PRINCIPAL: WAVEFORM & RULER (con zoom Ctrl+Ruedita) ---
                ui.vertical(|ui| {
                    // Toolbar de zoom
                    ui.horizontal(|ui| {
                        ui.label("🔍");
                        if ui.small_button("➖").clicked() {
                            zoom_factor = (zoom_factor / 1.25).clamp(0.25, 32.0);
                            ui.data_mut(|d| d.insert_temp(zoom_id, zoom_factor));
                        }
                        ui.label(format!("{:.0}%", zoom_factor * 100.0));
                        if ui.small_button("➕").clicked() {
                            zoom_factor = (zoom_factor * 1.25).clamp(0.25, 32.0);
                            ui.data_mut(|d| d.insert_temp(zoom_id, zoom_factor));
                        }
                        if ui.small_button("Fit").clicked() {
                            zoom_factor = 1.0;
                            ui.data_mut(|d| {
                                d.insert_temp(zoom_id, zoom_factor);
                                // Reencuadrar en el compás 1 (saltear el count-in)
                                // y recalibrar la referencia de Fit a la duración
                                // actual (ver fit_ref abajo).
                                d.insert_temp(
                                    egui::Id::new("clip_editor_fit_scroll"),
                                    true,
                                );
                                d.insert_temp(
                                    egui::Id::new(format!(
                                        "clip_editor_fit_ref_{}",
                                        clip.id
                                    )),
                                    clip.duration_secs.max(0.001),
                                );
                            });
                        }
                        ui.weak("Ctrl+Rueda: Zoom");
                    });

                    let viewport = ui.available_size();
                    // --- Escala DAW (px/seg) + regla infinita ---
                    // La regla ya no está limitada a 2 compases: el canvas es
                    // virtualmente infinito (64 compases a la izquierda, 512 a
                    // la derecha por defecto) y crece solo si los eventos se
                    // arrastran más lejos. El dibujado de la grilla se recorta
                    // a lo visible (culling) para que el canvas enorme no
                    // cueste rendimiento. Estilo Ableton/Bitwig.
                    let bpm_tmp = if bpm > 0.0 { bpm as f64 } else { 120.0 };
                    let bar_tmp = (60.0 / bpm_tmp) * 4.0;
                    let dur_tmp = clip.duration_secs.max(0.001);
                    // Referencia de Fit CONGELADA por clip: si usáramos la
                    // duración actual cada frame, al mover/trimear un evento
                    // cambiaría duration_secs → cambiaría px_per_sec → el
                    // mapeo x_of se movería bajo el cursor y el clip saltaría
                    // a un compás lejano (feedback positivo). Por eso la
                    // escala solo se recalibra al abrir o con Fit.
                    let fit_ref_id =
                        egui::Id::new(format!("clip_editor_fit_ref_{}", clip.id));
                    let fit_tmp: f64 = ui.data_mut(|d| {
                        if let Some(v) = d.get_temp::<f64>(fit_ref_id) {
                            // Si el clip quedó vacío, permitir reencuadre.
                            if dur_tmp <= 0.002 {
                                return dur_tmp;
                            }
                            v.max(0.001)
                        } else {
                            let init = if clip.duration_secs > 0.001 {
                                clip.duration_secs
                            } else {
                                bar_tmp.max(0.5)
                            };
                            d.insert_temp(fit_ref_id, init);
                            init
                        }
                    });
                    // Extensión máxima de los eventos: así la cola crece sola
                    // al arrastrar un evento lejos (infinito efectivo).
                    let mut ev_max_end = dur_tmp;
                    for e in clip.audio_events() {
                        ev_max_end = ev_max_end.max(e.end_secs());
                    }
                    // Izquierda fija y amplia (origen estable: si creciera con
                    // los eventos, el mapeo x_of se movería bajo el cursor y
                    // el drag pegaría saltos). 64 compases ≈ infinito práctico.
                    let lead_in_secs: f64 = bar_tmp * 64.0;
                    // Derecha: 512 compases por defecto + 32 de margen sobre el
                    // evento más lejano. Crecer a la derecha no mueve el origen.
                    let tail_default: f64 = (bar_tmp * 512.0).max(dur_tmp * 0.5).max(4.0);
                    let mut visible_secs: f64 =
                        (dur_tmp + tail_default).max(ev_max_end + bar_tmp * 32.0);
                    // Tope de seguridad (24h) para no desbordar el canvas.
                    visible_secs = visible_secs.min(86_400.0);
                    let px_per_sec: f32 =
                        (viewport.x.max(1.0) / fit_tmp as f32 * zoom_factor).max(1.0);
                    // Al abrir (y con Fit) encuadrar en el compás 1, no en el
                    // count-in: el offset se aplica una sola vez vía temp.
                    let lead_px = lead_in_secs as f32 * px_per_sec;
                    let need_reanchor: bool = ui.data_mut(|d| {
                        let init_done: bool =
                            d.get_temp(egui::Id::new("clip_editor_hscroll_init"))
                                .unwrap_or(false);
                        let fit_req: bool =
                            d.get_temp(egui::Id::new("clip_editor_fit_scroll"))
                                .unwrap_or(false);
                        let need = !init_done || fit_req;
                        if need {
                            d.insert_temp(egui::Id::new("clip_editor_hscroll_init"), true);
                            d.insert_temp(
                                egui::Id::new("clip_editor_fit_scroll"),
                                false,
                            );
                        }
                        need
                    });
                    let scroll_area = egui::ScrollArea::horizontal()
                        .id_source("clip_editor_hscroll")
                        // Sin drag_to_scroll: si el área scrolleara durante el
                        // arrastre, el contenido se deslizaría bajo el cursor y
                        // el move/trim se movería al doble / no-lineal.
                        // Scroll con ruedita/barra, zoom con Ctrl+rueda.
                        .drag_to_scroll(false);
                    let scroll_area = if need_reanchor {
                        scroll_area.horizontal_scroll_offset(lead_px)
                    } else {
                        scroll_area
                    };
                    scroll_area.show(ui, |ui| {
                            let canvas_w = ((lead_in_secs + visible_secs) as f32 * px_per_sec)
                                .max(viewport.x);
                            let canvas_h = viewport.y.max(50.0);
                            let (rect, response) = ui.allocate_exact_size(
                                Vec2::new(canvas_w, canvas_h),
                                Sense::click_and_drag(),
                            );

                            // Zoom con Ctrl + Ruedita (anclado al canvas, como en playlist.rs)
                            if response.hovered() {
                                let ctrl_pressed =
                                    ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
                                if ctrl_pressed {
                                    let mut zoom_delta = 0.0_f32;
                                    ui.input(|i| {
                                        for event in &i.events {
                                            if let egui::Event::MouseWheel { delta, .. } = event {
                                                zoom_delta += delta.y;
                                            }
                                        }
                                    });
                                    if zoom_delta != 0.0 {
                                        let factor =
                                            if zoom_delta > 0.0 { 1.15 } else { 0.85 };
                                        zoom_factor =
                                            (zoom_factor * factor).clamp(0.25, 32.0);
                                        ui.data_mut(|d| {
                                            d.insert_temp(zoom_id, zoom_factor)
                                        });
                                    }
                                }
                            }

                            if ui.is_rect_visible(rect) {
                    let total_frames = (clip.duration_secs * sample_rate as f64) as u64;

                    if clip.has_time_selection && total_frames > 0 {
                        if clip.loop_end == 0 || clip.loop_end > total_frames {
                            clip.loop_end = total_frames;
                        }
                        if clip.loop_start >= clip.loop_end {
                            clip.loop_start = clip.loop_end.saturating_sub(sample_rate as u64 / 4);
                        }
                    }

                    // --- Time Selection: Ctrl+A = restaurar (clip completo),
                    // Ctrl+Alt+A = quitar. Por defecto viene visible para
                    // loopear sin dramas. No robar el atajo si se edita texto.
                    let text_focused_here = ui.memory(|m| m.focused().is_some());
                    if !text_focused_here {
                        let (ctrl_a, alt_a) = ui.input(|i| {
                            let ctrl = i.modifiers.ctrl || i.modifiers.command;
                            (
                                ctrl && i.key_pressed(egui::Key::A) && !i.modifiers.alt,
                                ctrl && i.modifiers.alt && i.key_pressed(egui::Key::A),
                            )
                        });
                        if alt_a && clip.has_time_selection {
                            clip.has_time_selection = false;
                            loop_changed = true;
                        } else if ctrl_a
                            && (!clip.has_time_selection
                                || clip.loop_start != 0
                                || clip.loop_end != total_frames)
                        {
                            clip.has_time_selection = true;
                            clip.loop_start = 0;
                            clip.loop_end = total_frames;
                            loop_changed = true;
                        }
                    }

                    // LAYER 1: Canvas Background
                    ui.painter().rect_filled(rect, 4.0_f32, Color32::from_rgb(12, 12, 15));

                    // LAYER 2: Grid y Ruler (count-in negativo + cola infinita)
                    let current_bpm = if bpm > 0.0 { bpm as f64 } else { 120.0 };
                    let sec_per_beat = 60.0 / current_bpm;
                    let sec_per_bar = sec_per_beat * 4.0;
                    let total_bars = (visible_secs / sec_per_bar).ceil() as i32;
                    let neg_bars = (lead_in_secs / sec_per_bar).ceil() as i32;
                    // Mapeo DAW con origen desplazado: el compás 1 (t=0) queda
                    // a `lead_in_secs` del borde izquierdo; a la izquierda hay
                    // count-in en negativo estilo Ableton/Bitwig.
                    let x_of = |t: f64| -> f32 {
                        rect.min.x + ((t + lead_in_secs) as f32 * px_per_sec)
                    };

                    let ruler_height = 18.0_f32;
                    let ruler_rect = Rect::from_min_max(
                        rect.min,
                        Pos2::new(rect.max.x, rect.min.y + ruler_height),
                    );
                    let wave_rect = Rect::from_min_max(
                        Pos2::new(rect.min.x, rect.min.y + ruler_height),
                        rect.max,
                    );

                    ui.painter().rect_filled(ruler_rect, 0.0_f32, Color32::from_rgb(22, 22, 28));
                    ui.painter().line_segment(
                        [Pos2::new(rect.min.x, ruler_rect.max.y), Pos2::new(rect.max.x, ruler_rect.max.y)],
                        Stroke::new(1.0_f32, Color32::from_gray(50)),
                    );

                    // Fin real del clip + sombreado de la cola vacía (más allá).
                    let clip_end_x = x_of(clip.duration_secs.max(0.0));
                    if clip_end_x < rect.max.x {
                        ui.painter().rect_filled(
                            Rect::from_min_max(
                                Pos2::new(clip_end_x, wave_rect.min.y),
                                rect.max,
                            ),
                            0.0_f32,
                            Color32::from_rgb(9, 9, 12),
                        );
                        ui.painter().line_segment(
                            [Pos2::new(clip_end_x, rect.min.y), Pos2::new(clip_end_x, rect.max.y)],
                            Stroke::new(1.5_f32, Color32::from_rgb(255, 170, 60)),
                        );
                    }

                    if clip.duration_secs > 0.0 {
                        // Culling: solo dibujar los compases que caen en el
                        // viewport visible. Así el canvas puede ser enorme
                        // (regla infinita) sin costo de dibujado.
                        let clip_v = ui.clip_rect();
                        let t_of_x = |x: f32| -> f64 {
                            ((x - rect.min.x) / px_per_sec.max(0.001) - lead_in_secs as f32)
                                as f64
                        };
                        let t_min_v = t_of_x(clip_v.min.x).min(t_of_x(clip_v.max.x));
                        let t_max_v = t_of_x(clip_v.min.x).max(t_of_x(clip_v.max.x));
                        let mut bi_min =
                            (t_min_v / sec_per_bar).floor() as i32 - 1;
                        let mut bi_max =
                            (t_max_v / sec_per_bar).ceil() as i32 + 1;
                        bi_min = bi_min.max(-neg_bars).min(total_bars);
                        bi_max = bi_max.max(-neg_bars).min(total_bars);
                        for bi in bi_min..=bi_max {
                            let bar_time = bi as f64 * sec_per_bar;

                            let x_pos = x_of(bar_time);
                            let beyond = bar_time < -1e-9
                                || bar_time > clip.duration_secs + 1e-6;

                            ui.painter().line_segment(
                                [Pos2::new(x_pos, rect.min.y), Pos2::new(x_pos, rect.max.y)],
                                Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, if beyond { 18 } else { 40 })),
                            );

                            ui.painter().text(
                                Pos2::new(x_pos + 4.0, rect.min.y + 2.0),
                                Align2::LEFT_TOP,
                                format!("{}", bi + 1),
                                FontId::proportional(10.0),
                                if beyond { Color32::from_gray(110) } else { Color32::from_gray(180) },
                            );

                            for beat in 1..4 {
                                let beat_time = bar_time + (beat as f64 * sec_per_beat);
                                if beat_time < -lead_in_secs { continue; }
                                if beat_time > visible_secs { break; }

                                let beat_x_pos = x_of(beat_time);

                                ui.painter().line_segment(
                                    [Pos2::new(beat_x_pos, wave_rect.min.y), Pos2::new(beat_x_pos, wave_rect.max.y)],
                                    Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 12)),
                                );
                            }
                        }
                    }

                    // LAYER 3: Active Selection Region Background
                    // (oculto con Ctrl+Alt+A, se restaura con Ctrl+A)
                    let (sel_x_min, sel_x_max) = if total_frames > 0
                        && clip.has_time_selection
                        && clip.loop_end > clip.loop_start
                    {
                        let sr_f = sample_rate.max(1) as f64;
                        let start_t = clip.loop_start as f64 / sr_f;
                        let end_t = clip.loop_end as f64 / sr_f;

                        let min_x = x_of(start_t);
                        let max_x = x_of(end_t);

                        let sel_rect = Rect::from_min_max(
                            Pos2::new(min_x, wave_rect.min.y),
                            Pos2::new(max_x, wave_rect.max.y),
                        );

                        ui.painter().rect_filled(sel_rect, 0.0_f32, Color32::from_rgba_unmultiplied(0, 120, 255, 45));
                        (min_x, max_x)
                    } else {
                        (rect.min.x, clip_end_x)
                    };

                    // LAYER 4: Waveform por evento, en el rect de cada evento.
                    // A propósito NO se usa el mix global posicionado en 0: así
                    // la onda y su rectángulo salen de los mismos (es, ee) y es
                    // imposible que se desincronicen al mover/trimear.
                    {
                        let waves: Vec<(f64, f64, Vec<f32>)> = clip
                            .audio_events()
                            .iter()
                            .map(|e| (e.start_secs, e.end_secs(), e.mono_mixed()))
                            .collect();
                        for (es, ee, mono) in &waves {
                            if mono.is_empty() {
                                continue;
                            }
                            let r = Rect::from_min_max(
                                Pos2::new(x_of(*es), wave_rect.min.y),
                                Pos2::new(
                                    x_of(*ee).max(x_of(*es) + 1.0),
                                    wave_rect.max.y,
                                ),
                            );
                            waveform::draw_waveform(
                                ui,
                                r,
                                mono,
                                Color32::from_rgb(0, 200, 255),
                            );
                        }
                    }

                    // LAYER 4b: Regiones de eventos (selección / mover suelto / split con S)
                    {
                        let total = clip.duration_secs.max(0.001);
                        let snap_step_ev = current_snap.interval_secs(bpm);
                        let alt_held = ui.input(|i| i.modifiers.alt);
                        // Snap efectivo: Alt lo desactiva momentáneamente (movida libre).
                        let eff_snap = if alt_held { 0.0 } else { snap_step_ev };

                        // --- Puntero sobre el canvas: tiempo crudo + con snap ---
                        let hover_pos_opt: Option<Pos2> = ui
                            .input(|i| i.pointer.hover_pos())
                            .or_else(|| response.interact_pointer_pos());
                        let hover_in_canvas = hover_pos_opt
                            .map(|p| rect.contains(p))
                            .unwrap_or(false);
                        let hover_raw_t: Option<f64> = hover_pos_opt.and_then(|p| {
                            if px_per_sec <= 0.0 || !rect.contains(p) {
                                return None;
                            }
                            Some(
                                ((p.x - rect.min.x) / px_per_sec - lead_in_secs as f32)
                                    .clamp(-lead_in_secs as f32, visible_secs as f32)
                                    as f64,
                            )
                        });
                        let hover_split_t: Option<f64> = hover_raw_t.map(|t| {
                            if eff_snap > 0.0 {
                                ((t / eff_snap).round() * eff_snap)
                                    .clamp(-lead_in_secs, total)
                            } else {
                                t.clamp(-lead_in_secs, total)
                            }
                        });

                        // Tecla S: split directo en el puntero (solo si el puntero
                        // está sobre el canvas, para no romper atajos globales).
                        // Se ignora con Ctrl/Cmd para no chocar con Guardar, etc.
                        let s_pressed = ui.input(|i| {
                            i.key_pressed(egui::Key::S)
                                && !i.modifiers.ctrl
                                && !i.modifiers.command
                        });
                        let s_held = ui.input(|i| i.key_down(egui::Key::S));
                        // Pequeña ayuda para que el split caiga dentro aunque el
                        // snap lo empuje al borde: margen de un píxel mínimo.
                        let split_margin =
                            (1.0 / px_per_sec.max(0.001) as f64).max(0.0005);

                        // Helper local: intenta dividir `eid` en `t`.
                        // Retorna el id de la mitad derecha si tuvo éxito.
                        let mut try_split_at = |clip: &mut MatrixClip,
                                                eid: u64,
                                                t: f64|
                         -> Option<u64> {
                            let nid = clip.alloc_event_id();
                            let nm = format!("{}#{}", clip.name, nid);
                            let events = clip.audio_events_mut()?;
                            let pos = events.iter().position(|e| e.id == eid)?;
                            let right = events[pos].split_at(t, nid, nm)?;
                            let rid = right.id;
                            events.push(right);
                            events.sort_by(|a, b| {
                                a.start_secs
                                    .partial_cmp(&b.start_secs)
                                    .unwrap_or(std::cmp::Ordering::Equal)
                            });
                            Some(rid)
                        };

                        // Snapshot para no pelear con el borrow checker.
                        let ev_snapshot: Vec<(u64, String, f64, f64, f32)> = clip
                            .audio_events()
                            .iter()
                            .map(|e| (e.id, e.name.clone(), e.start_secs, e.end_secs(), e.gain))
                            .collect();

                        // ¿Qué evento está bajo el puntero (para preview + S)?
                        let hover_eid: Option<u64> = hover_split_t.and_then(|t| {
                            ev_snapshot
                                .iter()
                                .find(|(_, _, es, ee, _)| {
                                    t > *es + split_margin && t < *ee - split_margin
                                })
                                .map(|(id, _, _, _, _)| *id)
                        });

                        // --- SPLIT GLOBAL CON S (sin necesidad de clickear) ---
                        if s_pressed && hover_in_canvas {
                            if let (Some(t), Some(eid)) = (hover_split_t, hover_eid) {
                                if let Some(rid) = try_split_at(clip, eid, t) {
                                    selected_event = Some(rid);
                                    clip.refresh_preview();
                                    events_changed = true;
                                }
                            }
                        }

                        // --- Preview de corte: línea vertical + tijera ---
                        // Oculta mientras se arrastra (botón apretado): si no la
                        // línea de tijera queda pegada encima durante el move/trim.
                        let pointer_down = ui.input(|i| {
                            i.pointer.primary_down() || i.pointer.secondary_down()
                        });
                        if hover_in_canvas && !response.dragged() && !pointer_down {
                            if let (Some(t), Some(_)) = (hover_split_t, hover_eid) {
                                let x = x_of(t);
                                ui.painter().line_segment(
                                    [Pos2::new(x, wave_rect.min.y), Pos2::new(x, wave_rect.max.y)],
                                    Stroke::new(1.5_f32, Color32::from_rgb(255, 220, 90)),
                                );
                                // Marquitas arriba/abajo estilo tijera.
                                for y in [wave_rect.min.y + 3.0, wave_rect.max.y - 3.0] {
                                    ui.painter().text(
                                        Pos2::new(x + 3.0, y - 7.0),
                                        Align2::LEFT_TOP,
                                        "✂",
                                        FontId::proportional(10.0),
                                        Color32::from_rgb(255, 220, 90),
                                    );
                                }
                                if s_held {
                                    ui.output_mut(|o| o.cursor_icon = CursorIcon::Crosshair);
                                }
                            }
                        }

                        let mut clicked_on_event = false;
                        for (eid, ename, es, ee, _eg) in ev_snapshot {
                            let x0 = x_of(es);
                            let x1 = x_of(ee);
                            let ev_rect = Rect::from_min_max(
                                Pos2::new(x0, wave_rect.min.y),
                                Pos2::new(x1.max(x0 + 6.0), wave_rect.max.y),
                            );
                            let is_sel = selected_event == Some(eid);
                            let is_hovered_target = hover_eid == Some(eid);
                            let border = if is_sel {
                                Color32::from_rgb(0, 255, 200)
                            } else if is_hovered_target {
                                Color32::from_rgb(255, 220, 90)
                            } else {
                                Color32::from_rgba_unmultiplied(255, 255, 255, 90)
                            };
                            // Relleno sutil en el evento bajo el puntero: se siente suelto.
                            if is_hovered_target && !is_sel {
                                ui.painter().rect_filled(
                                    ev_rect,
                                    2.0,
                                    Color32::from_rgba_unmultiplied(255, 220, 90, 14),
                                );
                            }
                            ui.painter().rect_stroke(
                                ev_rect,
                                2.0,
                                Stroke::new(if is_sel { 2.0 } else { 1.0 }, border),
                            );
                            // Etiqueta del evento.
                            ui.painter().text(
                                Pos2::new(ev_rect.min.x + 4.0, ev_rect.min.y + 2.0),
                                Align2::LEFT_TOP,
                                format!("{} ", ename),
                                FontId::proportional(10.0),
                                if is_sel {
                                    Color32::from_rgb(0, 255, 200)
                                } else {
                                    Color32::from_gray(200)
                                },
                            );
                            let ev_id = ui.make_persistent_id(format!("clip_ev_{}_{}", clip.id, eid));
                            let ev_resp = ui.interact(ev_rect, ev_id, Sense::click_and_drag());
                            // --- Cursor <> en los bordes: zona de trim (izq/der) ---
                            // Clásico cursor de resize horizontal cuando el puntero
                            // está sobre los lados del clip, Grab en el centro.
                            let edge_px = 9.0_f32;
                            let hover_x = hover_pos_opt.map(|p| p.x);
                            let near_left = hover_x
                                .map(|hx| (hx - x0).abs() <= edge_px)
                                .unwrap_or(false)
                                && ev_rect.contains(hover_pos_opt.unwrap_or(Pos2::ZERO));
                            let near_right = hover_x
                                .map(|hx| (hx - x1).abs() <= edge_px)
                                .unwrap_or(false)
                                && ev_rect.contains(hover_pos_opt.unwrap_or(Pos2::ZERO));
                            let on_edge = (near_left || near_right) && ev_resp.hovered();
                            // Modo de arrastre por evento: 0 = mover, 1 = trim izq, 2 = trim der.
                            let mode_id = ui.make_persistent_id(format!(
                                "clip_ev_dragmode_{}_{}",
                                clip.id, eid
                            ));
                            if ev_resp.hovered() {
                                clicked_on_event = true;
                                if ev_resp.dragged() {
                                    let mode: u8 =
                                        ui.data_mut(|d| d.get_temp(mode_id).unwrap_or(0));
                                    if mode == 1 || mode == 2 {
                                        ui.output_mut(|o| {
                                            o.cursor_icon = CursorIcon::ResizeHorizontal
                                        });
                                    } else {
                                        ui.output_mut(|o| o.cursor_icon = CursorIcon::Grabbing);
                                    }
                                } else if on_edge {
                                    ui.output_mut(|o| {
                                        o.cursor_icon = CursorIcon::ResizeHorizontal
                                    });
                                } else {
                                    ui.output_mut(|o| o.cursor_icon = CursorIcon::Grab);
                                }
                            }
                            // --- Manijas de borde visibles (estilo DAW) ---
                            // Siempre se ven sutiles y brillan al apuntar o
                            // arrastrar: agarrar ahí hace trim, el centro mueve.
                            let drag_mode: u8 = if ev_resp.dragged() {
                                ui.data_mut(|d| d.get_temp(mode_id).unwrap_or(0))
                            } else {
                                0
                            };
                            for (hx, active) in [
                                (x0, near_left || drag_mode == 1),
                                (x1, near_right || drag_mode == 2),
                            ] {
                                ui.painter().line_segment(
                                    [
                                        Pos2::new(hx, ev_rect.min.y + 2.0),
                                        Pos2::new(hx, ev_rect.max.y - 2.0),
                                    ],
                                    Stroke::new(
                                        if active { 3.0_f32 } else { 1.5_f32 },
                                        if active {
                                            Color32::from_rgb(0, 255, 200)
                                        } else {
                                            Color32::from_rgba_unmultiplied(
                                                255, 255, 255, 70,
                                            )
                                        },
                                    ),
                                );
                            }
                            // Click con S apretada = corte directo donde clickeás.
                            if ev_resp.clicked() {
                                clicked_on_event = true;
                                if s_held {
                                    if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
                                        let raw = ((p.x - rect.min.x) / px_per_sec
                                            - lead_in_secs as f32)
                                            .clamp(-lead_in_secs as f32, total as f32)
                                            as f64;
                                        let t = if eff_snap > 0.0 {
                                            ((raw / eff_snap).round() * eff_snap)
                                                .clamp(0.0, total)
                                        } else {
                                            raw
                                        };
                                        if let Some(rid) = try_split_at(clip, eid, t) {
                                            selected_event = Some(rid);
                                            clip.refresh_preview();
                                            events_changed = true;
                                            continue;
                                        }
                                    }
                                }
                                selected_event = Some(eid);
                            }
                            if ev_resp.double_clicked() {
                                if let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) {
                                    let raw = ((pointer.x - rect.min.x) / px_per_sec
                                        - lead_in_secs as f32)
                                        .clamp(-lead_in_secs as f32, total as f32)
                                        as f64;
                                    let t = if eff_snap > 0.0 {
                                        ((raw / eff_snap).round() * eff_snap)
                                            .clamp(-lead_in_secs, total)
                                    } else {
                                        raw
                                    };
                                    if let Some(rid) = try_split_at(clip, eid, t) {
                                        selected_event = Some(rid);
                                        clip.refresh_preview();
                                        events_changed = true;
                                    }
                                }
                            }
                            // --- MOVER SUELTO / TRIM DESDE BORDES ---
                            // Centro = mover (con acumulador anti-sticky).
                            // Bordes (manijas <>) = trim no-destructivo: el audio
                            // original se conserva y se puede re-estirar.
                            let accum_id = ui.make_persistent_id(format!(
                                "clip_ev_dragacc_{}_{}",
                                clip.id, eid
                            ));
                            if ev_resp.drag_started() {
                                ui.data_mut(|d| d.insert_temp(accum_id, es));
                                // Decidir modo según dónde arrancó el agarre.
                                let start_x = ui
                                    .input(|i| i.pointer.interact_pos())
                                    .map(|p| p.x)
                                    .unwrap_or_else(|| {
                                        hover_x.unwrap_or((x0 + x1) * 0.5)
                                    });
                                let m: u8 = if (start_x - x0).abs() <= edge_px {
                                    1
                                } else if (start_x - x1).abs() <= edge_px {
                                    2
                                } else {
                                    0
                                };
                                ui.data_mut(|d| d.insert_temp(mode_id, m));
                            }
                            if ev_resp.dragged() {
                                let mode: u8 =
                                    ui.data_mut(|d| d.get_temp(mode_id).unwrap_or(0));
                                if mode == 1 || mode == 2 {
                                    // --- TRIM NO-DESTRUCTIVO (re-estirable) ---
                                    // El buffer original se conserva: solo se mueve
                                    // la ventana visible, así que estirar de vuelta
                                    // recupera el audio (como Ableton/Bitwig).
                                    if let Some(pointer) =
                                        ui.input(|i| i.pointer.interact_pos())
                                    {
                                        if px_per_sec > 0.0 && total > 0.0 {
                                            let raw = ((pointer.x - rect.min.x)
                                                / px_per_sec
                                                - lead_in_secs as f32)
                                            .clamp(-lead_in_secs as f32, total as f32)
                                                as f64;
                                            let t = if eff_snap > 0.0 {
                                                ((raw / eff_snap).round() * eff_snap)
                                                    .clamp(-lead_in_secs, total)
                                            } else {
                                                raw
                                            };
                                            if let Some(events) =
                                                clip.audio_events_mut()
                                            {
                                                if let Some(ev) = events
                                                    .iter_mut()
                                                    .find(|e| e.id == eid)
                                                {
                                                    let sr = ev.sample_rate.max(1)
                                                        as f64;
                                                    let c_start =
                                                        ev.content_start_secs();
                                                    let c_end =
                                                        ev.content_end_secs();
                                                    let total_av = ev.total_frames();
                                                    let min_dur = 0.05_f64;
                                                    if mode == 1 {
                                                        // Borde izquierdo: inicio + head.
                                                        let end = ev.end_secs();
                                                        let ns = t.clamp(
                                                            c_start,
                                                            (end - min_dur).max(c_start),
                                                        );
                                                        let new_left =
                                                            (((ns - c_start) * sr).round()
                                                                as usize)
                                                                .min(total_av);
                                                        let new_vis =
                                                            (((end - ns) * sr).round()
                                                                as usize)
                                                                .min(total_av
                                                                    .saturating_sub(new_left));
                                                        if (ns - ev.start_secs).abs()
                                                            > f64::EPSILON
                                                            || new_left
                                                                != ev.trim_left_frames
                                                            || new_vis != ev.visible_frames
                                                        {
                                                            ev.start_secs = ns;
                                                            ev.trim_left_frames = new_left;
                                                            ev.visible_frames = new_vis;
                                                            selected_event = Some(eid);
                                                            clip.refresh_preview();
                                                            events_changed = true;
                                                        }
                                                    } else {
                                                        // Borde derecho: tail.
                                                        let st = ev.start_secs;
                                                        let ne = t.clamp(
                                                            (st + min_dur).min(c_end),
                                                            c_end,
                                                        );
                                                        let new_vis =
                                                            (((ne - st) * sr).round()
                                                                as usize)
                                                                .min(total_av.saturating_sub(
                                                                    ev.trim_left_frames,
                                                                ));
                                                        if new_vis != ev.visible_frames {
                                                            ev.visible_frames = new_vis;
                                                            selected_event = Some(eid);
                                                            clip.refresh_preview();
                                                            events_changed = true;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    let dx = ev_resp.drag_delta().x;
                                    if dx != 0.0 && total > 0.0 && px_per_sec > 0.0 {
                                        let dt_raw = dx as f64 / px_per_sec as f64;
                                        let mut accum: f64 =
                                            ui.data_mut(|d| d.get_temp(accum_id).unwrap_or(es));
                                        accum += dt_raw;
                                        // Tope también en el acumulado: si solo se
                                        // topa `nt`, el residuo crudo sigue
                                        // bajando y al volver hay zona muerta +
                                        // salto (movimiento no-lineal). El tope
                                        // inferior es el count-in negativo.
                                        accum = accum.max(-lead_in_secs);
                                        let mut nt = if eff_snap > 0.0 {
                                            (accum / eff_snap).round() * eff_snap
                                        } else {
                                            accum
                                        };
                                        // El evento puede vivir en el count-in
                                        // negativo; el motor recorta lo previo al 0.
                                        nt = nt.max(-lead_in_secs);
                                        // Guardar el acumulado CRUDO para no perder
                                        // el residuo sub-snap (sensación suelta).
                                        ui.data_mut(|d| d.insert_temp(accum_id, accum));
                                        if let Some(events) = clip.audio_events_mut() {
                                            if let Some(ev) = events.iter_mut().find(|e| e.id == eid) {
                                                // Evitar refresh si el cambio es imperceptible.
                                                if (nt - ev.start_secs).abs() > f64::EPSILON {
                                                    ev.start_secs = nt;
                                                    selected_event = Some(eid);
                                                    clip.refresh_preview();
                                                    events_changed = true;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            if ev_resp.drag_stopped() {
                                ui.data_mut(|d| d.remove_temp::<f64>(accum_id));
                                ui.data_mut(|d| d.remove_temp::<u8>(mode_id));
                            }
                        }
                        // Click en zona vacía = deseleccionar (comportamiento DAW).
                        if response.clicked() && !clicked_on_event {
                            selected_event = None;
                        }
                        // Supr/Backspace borra el seleccionado (si no se está editando texto).
                        let text_focused = ui.memory(|m| m.focused().is_some());
                        if !text_focused
                            && ui.input(|i| {
                                i.key_pressed(egui::Key::Delete)
                                    || i.key_pressed(egui::Key::Backspace)
                            })
                        {
                            if let Some(id) = selected_event {
                                if let Some(events) = clip.audio_events_mut() {
                                    let before = events.len();
                                    events.retain(|e| e.id != id);
                                    if events.len() != before {
                                        selected_event = None;
                                        clip.refresh_preview();
                                        events_changed = true;
                                    }
                                }
                            }
                        }
                        ui.data_mut(|d| {
                            d.insert_temp(sel_id, selected_event);
                        });
                    }

                    // LAYER 5: Independent Interactive Bracket Handles
                    // (ocultos sin Time Selection)
                    if total_frames > 0 && clip.has_time_selection {
                        // Manijas SOLO en la regla superior (estilo Ableton): la
                        // línea vertical de cada bracket es solo visual. Antes los
                        // botones ocupaban toda la altura y le robaban el agarre
                        // a los bordes del evento (agarrabas el clip por el
                        // costado y en realidad movías el loop).
                        let handle_w = 14.0_f32;

                        let min_frame_gap = (sample_rate as u64 / 10).max(1024);
                        let snap_step = current_snap.interval_secs(bpm);

                        let calc_snapped_frame = |x_pos: f32| -> u64 {
                            if px_per_sec <= 0.0 || clip.duration_secs <= 0.0 {
                                return 0;
                            }
                            let mut time_secs =
                                ((x_pos - rect.min.x) / px_per_sec - lead_in_secs as f32)
                                    .clamp(0.0, clip.duration_secs as f32)
                                    as f64;

                            if snap_step > 0.0 {
                                time_secs = (time_secs / snap_step).round() * snap_step;
                            }

                            let norm_snapped = (time_secs / clip.duration_secs).clamp(0.0, 1.0);
                            (norm_snapped * total_frames as f64) as u64
                        };

                        // Bracket Izquierdo (`[`)
                        let start_rect = Rect::from_min_max(
                            Pos2::new(sel_x_min - handle_w * 0.5, rect.min.y),
                            Pos2::new(sel_x_min + handle_w * 0.5, rect.min.y + ruler_height),
                        );
                        let start_response = ui.put(
                            start_rect,
                            egui::Button::new("")
                                .fill(Color32::from_rgb(0, 255, 200))
                                .sense(Sense::drag()),
                        );

                        ui.painter().line_segment(
                            [Pos2::new(sel_x_min, rect.min.y), Pos2::new(sel_x_min, rect.max.y)],
                            Stroke::new(2.0_f32, Color32::from_rgb(0, 255, 200)),
                        );
                        ui.painter().rect_filled(
                            start_rect,
                            3.0_f32,
                            Color32::from_rgb(0, 255, 200),
                        );
                        ui.painter().text(
                            start_rect.center(),
                            Align2::CENTER_CENTER,
                            "[",
                            FontId::proportional(11.0),
                            Color32::BLACK,
                        );

                        if start_response.hovered() || start_response.dragged() {
                            ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeHorizontal);
                        }
                        if start_response.dragged() {
                            if let Some(pointer_pos) = ui.input(|i| i.pointer.interact_pos()) {
                                let new_start = calc_snapped_frame(pointer_pos.x);
                                let target = if new_start + min_frame_gap <= clip.loop_end {
                                    new_start
                                } else {
                                    clip.loop_end.saturating_sub(min_frame_gap)
                                };
                                if clip.loop_start != target {
                                    clip.loop_start = target;
                                    loop_changed = true;
                                }
                            }
                        }

                        // Bracket Derecho (`]`)
                        let end_rect = Rect::from_min_max(
                            Pos2::new(sel_x_max - handle_w * 0.5, rect.min.y),
                            Pos2::new(sel_x_max + handle_w * 0.5, rect.min.y + ruler_height),
                        );
                        let end_response = ui.put(
                            end_rect,
                            egui::Button::new("")
                                .fill(Color32::from_rgb(255, 80, 80))
                                .sense(Sense::drag()),
                        );

                        ui.painter().line_segment(
                            [Pos2::new(sel_x_max, rect.min.y), Pos2::new(sel_x_max, rect.max.y)],
                            Stroke::new(2.0_f32, Color32::from_rgb(255, 80, 80)),
                        );
                        ui.painter().rect_filled(
                            end_rect,
                            3.0_f32,
                            Color32::from_rgb(255, 80, 80),
                        );
                        ui.painter().text(
                            end_rect.center(),
                            Align2::CENTER_CENTER,
                            "]",
                            FontId::proportional(11.0),
                            Color32::BLACK,
                        );

                        if end_response.hovered() || end_response.dragged() {
                            ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeHorizontal);
                        }
                        if end_response.dragged() {
                            if let Some(pointer_pos) = ui.input(|i| i.pointer.interact_pos()) {
                                let new_end = calc_snapped_frame(pointer_pos.x);
                                let target = if new_end >= clip.loop_start + min_frame_gap {
                                    new_end
                                } else {
                                    (clip.loop_start + min_frame_gap).min(total_frames)
                                };
                                if clip.loop_end != target {
                                    clip.loop_end = target;
                                    loop_changed = true;
                                }
                            }
                        }
                    }

                    // LAYER 6: Playhead desde el motor (sobre el clip, no la cola)
                    if let Some((frame, engine_total)) = playhead {
                        let denom = if engine_total > 0 { engine_total } else { total_frames };
                        if denom > 0 {
                            let progress = (frame as f32 / denom as f32).clamp(0.0, 1.0);
                            let playhead_x =
                                x_of(progress as f64 * clip.duration_secs.max(0.0));

                            ui.painter().line_segment(
                                [Pos2::new(playhead_x, rect.min.y), Pos2::new(playhead_x, rect.max.y)],
                                Stroke::new(2.0_f32, Color32::WHITE),
                            );
                        }
                    }

                    ui.painter().rect_stroke(rect, 4.0_f32, Stroke::new(1.0_f32, Color32::from_gray(50)));
                            }
                        });
                    });
            });
        });

    // Sincronizar automáticamente con AudioEngine si hubo cambios.
    // Sin Time Selection no hay región que loopear: se avisa con enabled=false
    // (el toggle 🔁 Loop del clip no se toca, al restaurar vuelve solo).
    if loop_changed {
        if let Some(engine) = audio_engine.as_mut() {
            let start_secs = clip.loop_start as f32 / sample_rate as f32;
            let end_secs = clip.loop_end as f32 / sample_rate as f32;
            engine.set_clip_loop(
                track_idx,
                scene_idx,
                start_secs,
                end_secs,
                clip.loop_enabled && clip.has_time_selection,
            );
        }
    }
    if events_changed {
        if let Some(engine) = audio_engine.as_mut() {
            let evs: Vec<hikaru_audio_engine::EngineAudioEvent> = clip
                .audio_events()
                .iter()
                .map(|e| {
                    let sr = engine.sample_rate.max(1.0);
                    // Ventana visible (trim no-destructivo) + recorte de lo
                    // previo al 0 (count-in).
                    let window = e.window_samples();
                    let (samples, clip_start) = if e.start_secs < 0.0 {
                        let drop = ((-e.start_secs as f32) * sr).round() as usize;
                        let off = (drop * e.channels.max(1)).min(window.len());
                        (window[off..].to_vec(), 0)
                    } else {
                        (
                            window.to_vec(),
                            (e.start_secs as f32 * sr) as u64,
                        )
                    };
                    hikaru_audio_engine::EngineAudioEvent {
                        id: e.id,
                        samples,
                        channels: e.channels,
                        clip_start,
                        gain: e.gain,
                        fade_in: (e.fade_in_secs as f32 * sr) as u64,
                        fade_out: (e.fade_out_secs as f32 * sr) as u64,
                    }
                })
                .collect();
            engine.set_clip_events(track_idx, scene_idx, evs);
        }
    }
}

fn event_signature(clip: &MatrixClip) -> Vec<(u64, u64, u64, u32)> {
    clip.audio_events()
        .iter()
        .map(|e| {
            (
                e.id,
                (e.start_secs * 1_000_000.0).round() as u64,
                e.frames(),
                e.gain.to_bits(),
            )
        })
        .collect()
}

pub fn render_clip_editor_track_view(    ui: &mut Ui,
    state: &mut SessionMatrixState,
    audio_proxy: &AudioProxy,
    dragged_sample: &mut Option<PathBuf>,
    bpm: f64,
    sample_rate: u32,
    _transport_sample_count: u64,
    _ppqn: u64,
    _global_loop_enabled: bool,
    _global_loop_start_ticks: u64,
    _global_loop_end_ticks: u64,
) {
    if let Some((track_idx, scene_idx)) = state.selected_slot {
        if let Some(path) = dragged_sample.take() {
            matrix::load_clip_into_slot(state, audio_proxy, track_idx, scene_idx, path, bpm);
            return;
        }

        let mut clip_to_show = None;

        if track_idx < state.grid.len() && scene_idx < state.grid[track_idx].len() {
            clip_to_show = state.grid[track_idx][scene_idx].clip.as_mut();
        }

        if let Some(clip) = clip_to_show {
            let old_loop = clip.loop_enabled;
            let old_start = clip.loop_start;
            let old_end = clip.loop_end;
            let old_sel = clip.has_time_selection;
            let old_ev_sig = event_signature(clip);

            show(
                ui,
                clip,
                track_idx,
                scene_idx,
                None,
                None,
                sample_rate,
                bpm as f32,
            );

            if clip.loop_enabled != old_loop || clip.loop_start != old_start || clip.loop_end != old_end || clip.has_time_selection != old_sel {
                let start_secs = clip.loop_start as f32 / sample_rate as f32;
                let end_secs = clip.loop_end as f32 / sample_rate as f32;

                audio_proxy.set_clip_loop(
                    track_idx,
                    scene_idx,
                    start_secs,
                    end_secs,
                    clip.loop_enabled && clip.has_time_selection,
                );
            }
            if event_signature(clip) != old_ev_sig {
                matrix::sync_audio_events_to_engine(clip, audio_proxy, track_idx, scene_idx);
            }
        } else {
            ui.centered_and_justified(|ui| {
                ui.heading("El slot seleccionado está vacío. Arrastrá un archivo de audio aquí.");
            });
        }
    } else {
        ui.centered_and_justified(|ui| {
            ui.label("Seleccioná un clip en la Session Matrix para editarlo.");
        });
    }
}