// Hikaru OpenLive / OpenStudio - Bitwig Studio 5.1.7 Mix View (egui + wgpu SDF)
// GNU Affero General Public License v3
// crates/hikaru_gui/src/views/arranger_view.rs

use std::path::PathBuf;

use egui::{pos2, vec2, Align2, Color32, Rect, RichText, ScrollArea, Sense, Stroke, Ui};

use crate::app::AppState;
use crate::audio_proxy::GuiCommand;
use crate::theme::*;
use crate::ui::bitwig_kit::{
    capsule_icon_button, capsule_text_button, fader_horizontal_range, fader_vertical_track_vol,
    rotary_knob, stereo_meter_bar, JoinSide,
};
use crate::ui::fonts;
use crate::ui::icons;
use crate::views::dsp_rack::PLUGIN_CATALOG;
use crate::views::header::clip_launcher_toggle_widget;
use crate::views::matrix::{self, SlotState};
use crate::views::mixer::{self, DspSlot, SendConnection, Track};

pub const DEFAULT_TRACK_WIDTH: f32 = 88.0;
pub const MIN_TRACK_WIDTH: f32 = 82.0;
pub const MAX_TRACK_WIDTH: f32 = 340.0;

const LEGEND_COL_W: f32 = 82.0;
const HEADER_BLOCK_H: f32 = 42.0;
const CLIP_SLOT_H: f32 = 22.0;
const STOP_ROW_H: f32 = 20.0;
const DEVICES_WELL_H: f32 = 58.0;
const SENDS_BOX_H: f32 = 46.0;
const IO_BOX_H: f32 = 36.0;
// 126 en vez de 136: la suma de todas las franjas verticales daba 588px, o
// sea 8px más que el alto disponible del panel central a 768p. Eso encendía
// la scrollbar vertical de `ScrollArea::both` y dejaba la vista scrolleada
// al fondo, comiéndose 8px de las cabeceras de pista. La bahía del fader es
// la franja elástica, así que absorbe el sobrante y el strip queda casi
// pegado al footer (2px de aire).
const FADER_BAY_H: f32 = 126.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StripTarget {
    Master,
    Live(usize),
    Studio(usize),
    Fx(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaderTarget {
    MasterPan,
    MasterVolume,
    TrackPan(usize),
    TrackVolume(usize),
    StudioPan(usize),
    StudioVolume(usize),
    FxPan(usize),
    FxVolume(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MixerPanState {
    pub last: (f32, f32),
}

pub const MIXER_PAN_SENSITIVITY: f32 = 1.0;
pub const MIXER_BAR_HEIGHT: f32 = 10.0;
pub const MIXER_THUMB_MIN_W: f32 = 24.0;

pub struct MixerBarGeom {
    pub rail_x: f32,
    pub rail_w: f32,
    pub thumb_x: f32,
    pub thumb_w: f32,
}

pub fn mixer_bar_geom(offset_x: f32, max_x: f32, viewport_w: f32) -> MixerBarGeom {
    let rail_w = viewport_w.max(1.0);
    let total_w = rail_w + max_x;
    let thumb_w = (rail_w * rail_w / total_w).clamp(MIXER_THUMB_MIN_W, rail_w);
    let travel = (rail_w - thumb_w).max(0.0);
    let norm = if max_x > 0.0 {
        (-offset_x / max_x).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let thumb_x = norm * travel;
    MixerBarGeom {
        rail_x: 0.0,
        rail_w,
        thumb_x,
        thumb_w,
    }
}

pub const WAVEFORM_WINDOW_BARS: u64 = 2;

pub fn waveform_window_tick(
    playhead_tick: u64,
    window_ticks: u64,
    row: usize,
    rows: usize,
) -> u64 {
    if rows == 0 || window_ticks == 0 {
        return playhead_tick;
    }
    let span = window_ticks as i64;
    let tick = playhead_tick as i64 + span / 2 - (row as i64 * span) / rows.max(1) as i64;
    tick.max(0) as u64
}

pub fn waveform_peak_at_tick(
    peaks: &[f32],
    start_tick: u64,
    duration_ticks: u64,
    sample_offset_ticks: u64,
    total_sample_ticks: u64,
    tick: u64,
) -> f32 {
    if peaks.is_empty() || total_sample_ticks == 0 {
        return 0.0;
    }
    let end = start_tick.saturating_add(duration_ticks);
    if tick < start_tick || tick >= end {
        return 0.0;
    }
    let pos = sample_offset_ticks.saturating_add(tick - start_tick);
    let n = peaks.len();
    let idx = ((pos as f64 / total_sample_ticks as f64) * n as f64).floor() as usize;
    peaks.get(idx).copied().unwrap_or(0.0).clamp(0.0, 1.0)
}

fn format_strip_db(volume: f32) -> String {
    if volume <= 0.001 {
        "-inf".to_string()
    } else if volume <= 0.75 {
        let db = -60.0 + (volume / 0.75) * 60.0;
        format!("{db:.1}")
    } else {
        let db = ((volume - 0.75) / 0.25) * 6.0;
        format!("+{db:.1}")
    }
}

/// Alto total de la grilla de escenas (filas de `CLIP_SLOT_H` + 1px de separación).
fn scenes_content_h(scenes_count: usize) -> f32 {
    (scenes_count as f32 * (CLIP_SLOT_H + 1.0) - 1.0).max(0.0)
}

/// "Ventana" del Clip Launcher estilo Bitwig: la grilla de escenas/pads se
/// renderiza detrás de un viewport recortado de alto `view_h`, desplazada por
/// `scroll`. Cabeceras de pista arriba y faders abajo quedan fijos.
fn scene_viewport(
    ui: &mut Ui,
    w: f32,
    view_h: f32,
    scroll: f32,
    content_h: f32,
    salt: impl std::hash::Hash,
    add: impl FnOnce(&mut Ui),
) {
    let (rect, _) = ui.allocate_exact_size(vec2(w, view_h), Sense::hover());
    let inner = Rect::from_min_size(pos2(rect.min.x, rect.min.y - scroll), vec2(w, content_h.max(view_h)));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(salt)
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    add(&mut child);
    // Sombras internas sutiles cuando hay contenido oculto arriba/abajo.
    let painter = ui.painter().with_clip_rect(rect);
    let shade = Color32::from_black_alpha(110);
    if scroll > 0.5 {
        painter.rect_filled(Rect::from_min_size(rect.min, vec2(w, 3.0)), 0.0, shade);
    }
    if scroll + view_h < content_h - 0.5 {
        painter.rect_filled(Rect::from_min_size(pos2(rect.min.x, rect.max.y - 3.0), vec2(w, 3.0)), 0.0, shade);
    }
}

struct StripActions {
    selected_track_idx: usize,
    trigger_slot: Option<(usize, usize)>,
    stop_track: Option<usize>,
    drop_slot: Option<(usize, usize, PathBuf)>,
    /// Slot destino al soltar un clip arrastrado (estilo Bitwig: drag = mover, Ctrl/Alt+drag = copiar).
    clip_drop_target: Option<(usize, usize)>,
    /// Inicio de arrastre de un clip del launcher: (track, scene, clip, grab_offset).
    clip_drag_start: Option<(usize, usize, matrix::MatrixClip, egui::Vec2)>,
}

/// Renderiza la vista **Mix** de Bitwig Studio 5.1.7 (`bitwig-mix-panel.png`):
/// - Izquierda: Columna de escenas verticales + leyendas `Devices`, `Sends`, `I/O` + toggle de Clip Launcher inferior.
/// - Pistas de usuario (`1..N`) alineadas a la izquierda con bloque superior sólido del color de pista + botón `+`.
/// - Vacío negro profundo `#0A0A0C` en el centro.
/// - Canal `Master` anclado en el extremo derecho con cabecera sólida gris `#828488`.
/// - Controles renderizados con primitivas SDF `wgpu` (`rotary_knob`, `[● | S | M]`, `fader_horizontal_range`, `stereo_meter_bar`, `fader_vertical_track_vol`).
pub fn render(ui: &mut Ui, state: &mut AppState) {
    let tracks_count = state.tracks().len();
    let scenes_count = state.matrix_state.scenes.len();
    let show_launcher = state.panel_layout.show_clip_launcher;

    let mut actions = StripActions {
        selected_track_idx: state.selected_track_index,
        trigger_slot: None,
        stop_track: None,
        drop_slot: None,
        clip_drop_target: None,
        clip_drag_start: None,
    };
    let mut trigger_scene: Option<usize> = None;
    let mut add_scene_clicked = false;
    let mut add_track_clicked = false;

    let pointer_released = ui.input(|i| i.pointer.any_released());
    let is_dragging_sample = state.dragged_sample.is_some();

    let bg_rect = ui.available_rect_before_wrap();
    ui.painter().rect_filled(bg_rect, 0.0, BW_DARK_WELL);

    let viewport_w = bg_rect.width();

    // --- Ventana scrolleable de escenas (Bitwig) ---
    let content_h = scenes_content_h(scenes_count);
    // Alto de todo lo que NO es la grilla de escenas: cabecera, fila Stop,
    // Devices/Sends/I/O, bloque de controles (incluye el fader) y pie de 4px,
    // más los 7 separadores de 1px entre franjas. Antes se calculaba con el
    // bloque de la columna de leyendas y 8px de gaps, lo que se quedaba 1px
    // corto y volvía a encender la scrollbar vertical de `ScrollArea::both`.
    let fixed_h = HEADER_BLOCK_H
        + STOP_ROW_H
        + DEVICES_WELL_H
        + SENDS_BOX_H
        + IO_BOX_H
        + (FADER_BAY_H + 56.0) // margen 6 + cápsulas 18 + pan 13 + dB 14 + gaps 9 + fader (FADER_BAY_H - 4)
        + 4.0
        + 7.0;
    let min_view = (2.0 * (CLIP_SLOT_H + 1.0) - 1.0).min(content_h);
    // 1px de aire para no quedar exactamente contra el borde del viewport.
    let slots_view_h = content_h.min((bg_rect.height() - fixed_h - 1.0).max(min_view));
    let max_scroll = (content_h - slots_view_h).max(0.0);
    let scroll_id = ui.id().with("mix_scene_scroll");
    let mut scene_scroll: f32 = ui.ctx().data(|d| d.get_temp(scroll_id)).unwrap_or(0.0);
    if show_launcher {
        let band = Rect::from_min_size(
            pos2(bg_rect.min.x, bg_rect.min.y + HEADER_BLOCK_H + 1.0),
            vec2(bg_rect.width(), slots_view_h),
        );
        let hovering = ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| band.contains(p));
        if hovering {
            let dy = ui.input(|i| i.smooth_scroll_delta.y);
            if dy != 0.0 {
                scene_scroll -= dy;
                ui.input_mut(|i| i.smooth_scroll_delta.y = 0.0);
            }
        }
        // Middle-drag (ruedita apretada + arrastre) = pan vertical estilo Bitwig.
        let pan_id = ui.id().with("mix_scene_mid_pan");
        let (mid_pressed, mid_down, press_origin, delta) = ui.input(|i| {
            (
                i.pointer.button_pressed(egui::PointerButton::Middle),
                i.pointer.button_down(egui::PointerButton::Middle),
                i.pointer.press_origin(),
                i.pointer.delta(),
            )
        });
        let mut panning: bool = ui.ctx().data(|d| d.get_temp(pan_id)).unwrap_or(false);
        if mid_pressed && press_origin.is_some_and(|p| band.contains(p)) {
            panning = true;
        }
        if !mid_down {
            panning = false;
        }
        if panning {
            scene_scroll -= delta.y;
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        ui.ctx().data_mut(|d| d.insert_temp(pan_id, panning));
    }
    scene_scroll = scene_scroll.clamp(0.0, max_scroll);
    ui.ctx().data_mut(|d| d.insert_temp(scroll_id, scene_scroll));

    ScrollArea::both().auto_shrink([false; 2]).show(ui, |ui| {
        ui.spacing_mut().item_spacing = vec2(1.0, 1.0);

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;

            // =================================================================
            // 1. COLUMNA IZQUIERDA DE ESCENAS Y LEYENDAS DE SECCIÓN
            // =================================================================
            ui.vertical(|ui| {
                ui.set_min_width(LEGEND_COL_W);
                ui.set_max_width(LEGEND_COL_W);
                ui.spacing_mut().item_spacing.y = 1.0;

                // Cabecera neutra alineada a las cabeceras sólidas de 42px
                let (hdr_rect, _) = ui.allocate_exact_size(vec2(LEGEND_COL_W, HEADER_BLOCK_H), Sense::hover());
                ui.painter().rect_filled(hdr_rect, 0.0, Color32::from_rgb(0x3C, 0x3E, 0x43));
                ui.painter().line_segment(
                    [hdr_rect.left_bottom(), hdr_rect.right_bottom()],
                    Stroke::new(1.0_f32, BW_CAPSULE_BORDER),
                );

                if show_launcher {
                    let mut legend_scene_click = None;
                    scene_viewport(ui, LEGEND_COL_W, slots_view_h, scene_scroll, content_h, "mix_scene_legend", |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    for scene_idx in 0..scenes_count {
                        let s_name = state
                            .matrix_state
                            .scenes
                            .get(scene_idx)
                            .map(|s| s.name.clone())
                            .unwrap_or_else(|| format!("Scene {}", scene_idx + 1));

                        let (s_rect, s_resp) =
                            ui.allocate_exact_size(vec2(LEGEND_COL_W, CLIP_SLOT_H), Sense::click());
                        let s_bg = if s_resp.hovered() {
                            Color32::from_rgb(0x5E, 0x60, 0x66)
                        } else {
                            Color32::from_rgb(0x4C, 0x4E, 0x54)
                        };
                        ui.painter().rect_filled(s_rect, 1.5, s_bg);
                        let play_rect =
                            Rect::from_min_size(pos2(s_rect.min.x + 3.0, s_rect.min.y + 4.0), vec2(13.0, 13.0));
                        // Triángulo en gris muy claro (blanqueado) para que
                        // se distinga del texto blanco de la escena.
                        icons::paint_play_triangle(
                            ui.painter(),
                            play_rect,
                            Color32::from_rgb(0xBF, 0xC2, 0xC7),
                        );
                        // Texto en blanco para que se lea sobre el gris de la
                        // fila (antes era casi negro y no se distinguía).
                        ui.painter().text(
                            pos2(s_rect.min.x + 18.0, s_rect.center().y),
                            Align2::LEFT_CENTER,
                            &s_name,
                            fonts::semibold(10.5),
                            Color32::WHITE,
                        );
                        if s_resp.clicked() {
                            legend_scene_click = Some(scene_idx);
                        }
                    }
                    });
                    if legend_scene_click.is_some() {
                        trigger_scene = legend_scene_click;
                    }

                    // Fila de `+` (Add Scene) + `[■]` (Stop All Clips)
                    let (ctrl_rect, _) =
                        ui.allocate_exact_size(vec2(LEGEND_COL_W, STOP_ROW_H), Sense::hover());
                    ui.painter()
                        .rect_filled(ctrl_rect, 0.0, Color32::from_rgb(0x23, 0x24, 0x28));

                    let add_sc_rect =
                        Rect::from_min_size(pos2(ctrl_rect.min.x + 4.0, ctrl_rect.min.y + 2.0), vec2(18.0, 16.0));
                    let add_sc_resp = ui.interact(add_sc_rect, ui.id().with("mix_add_scene"), Sense::click());
                    icons::paint_icon(
                        ui.painter(),
                        add_sc_rect.shrink(3.0),
                        icons::PLUS,
                        if add_sc_resp.hovered() {
                            Color32::WHITE
                        } else {
                            TEXT_SECONDARY
                        },
                        1.7,
                    );
                    if add_sc_resp.on_hover_text("Add Scene").clicked() {
                        add_scene_clicked = true;
                    }

                    let stop_all_rect = Rect::from_min_size(
                        pos2(ctrl_rect.max.x - 22.0, ctrl_rect.min.y + 2.0),
                        vec2(18.0, 16.0),
                    );
                    let stop_all_resp =
                        ui.interact(stop_all_rect, ui.id().with("mix_stop_all"), Sense::click());
                    ui.painter().rect_filled(
                        stop_all_rect,
                        2.0,
                        if stop_all_resp.hovered() {
                            Color32::from_rgb(0x46, 0x48, 0x50)
                        } else {
                            Color32::from_rgb(0x34, 0x36, 0x3C)
                        },
                    );
                    icons::paint_stop_square(
                        ui.painter(),
                        stop_all_rect.shrink(3.5),
                        Color32::from_rgb(0xDA, 0xDC, 0xE2),
                    );
                    if stop_all_resp.on_hover_text("Stop All Clips").clicked() {
                        state.matrix_state.stop_all_clips(&state.audio_proxy);
                    }
                }

                // Bloques de leyenda lateral: Devices | Sends | I/O
                for (label, blk_h, fill) in [
                    ("Devices", DEVICES_WELL_H, Color32::from_rgb(0x2C, 0x2E, 0x33)),
                    ("Sends", SENDS_BOX_H, Color32::from_rgb(0x36, 0x38, 0x3E)),
                    ("I/O", IO_BOX_H, Color32::from_rgb(0x2C, 0x2E, 0x33)),
                ] {
                    let (r, _) = ui.allocate_exact_size(vec2(LEGEND_COL_W, blk_h), Sense::hover());
                    ui.painter().rect_filled(r, 0.0, fill);
                    ui.painter().text(
                        pos2(r.max.x - 7.0, r.min.y + 9.0),
                        Align2::RIGHT_TOP,
                        label,
                        fonts::regular(10.0),
                        Color32::from_rgb(0xB4, 0xB6, 0xBC),
                    );
                }

                // Bloque inferior con el toggle de Clip Launcher en la esquina inferior izquierda
                let bottom_legend_h = 20.0 + 14.0 + 16.0 + FADER_BAY_H + 8.0;
                let (bot_rect, _) =
                    ui.allocate_exact_size(vec2(LEGEND_COL_W, bottom_legend_h), Sense::hover());
                ui.painter()
                    .rect_filled(bot_rect, 0.0, Color32::from_rgb(0x19, 0x1A, 0x1E));

                let toggle_area = Rect::from_min_size(
                    pos2(bot_rect.min.x + 6.0, bot_rect.max.y - 24.0),
                    vec2(28.0, 20.0),
                );
                let mut toggle_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(toggle_area)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                if clip_launcher_toggle_widget(&mut toggle_ui, show_launcher)
                    .on_hover_text("Toggle Clip Launcher in Mix View")
                    .clicked()
                {
                    state.panel_layout.show_clip_launcher = !state.panel_layout.show_clip_launcher;
                }
            });

            // =================================================================
            // 2. PISTAS DE USUARIO (1..N) A LA IZQUIERDA
            // =================================================================
            let mut used_w = LEGEND_COL_W + 1.0;
            for idx in 1..tracks_count {
                let w = render_channel_strip(
                    ui,
                    state,
                    idx,
                    show_launcher,
                    scenes_count,
                    slots_view_h,
                    scene_scroll,
                    pointer_released,
                    is_dragging_sample,
                    &mut actions,
                );
                used_w += w + 1.0;
            }

            // Botón `+` (Add Track) inmediatamente a la derecha de la última pista de usuario
            ui.vertical(|ui| {
                ui.set_min_width(28.0);
                ui.set_max_width(28.0);
                let (plus_rect, plus_resp) =
                    ui.allocate_exact_size(vec2(26.0, HEADER_BLOCK_H), Sense::click());
                ui.painter().rect_filled(
                    plus_rect,
                    1.5,
                    if plus_resp.hovered() {
                        Color32::from_rgb(0x28, 0x2A, 0x30)
                    } else {
                        Color32::from_rgb(0x15, 0x16, 0x19)
                    },
                );
                icons::paint_icon(
                    ui.painter(),
                    Rect::from_center_size(plus_rect.center(), vec2(12.0, 12.0)),
                    icons::PLUS,
                    if plus_resp.hovered() {
                        Color32::WHITE
                    } else {
                        Color32::from_rgb(0x9A, 0x9E, 0xA6)
                    },
                    1.8,
                );
                if plus_resp.on_hover_text("Add Audio Track").clicked() {
                    add_track_clicked = true;
                }
            });
            used_w += 29.0;

            // =================================================================
            // 3. VACÍO NEGRO PROFUNDO + CANAL MASTER ANCLADO A LA DERECHA
            // =================================================================
            if tracks_count > 0 {
                let master_w = state.tracks()[0].width.clamp(MIN_TRACK_WIDTH, 94.0);
                let void_w = (viewport_w - used_w - master_w - 4.0).max(8.0);
                ui.add_space(void_w);

                render_channel_strip(
                    ui,
                    state,
                    0,
                    show_launcher,
                    scenes_count,
                    slots_view_h,
                    scene_scroll,
                    pointer_released,
                    is_dragging_sample,
                    &mut actions,
                );
            }
        });
    });

    state.selected_track_index = actions.selected_track_idx;

    if add_scene_clicked {
        state.matrix_state.add_scene();
    }

    if add_track_clicked {
        state.matrix_state.add_track();
        let new_idx = state.matrix_state.tracks.len();
        let mut tr = Track::new(new_idx, format!("Audio {new_idx}"), false);
        tr.matrix_idx = Some(new_idx - 1);
        state.live_tracks.push(tr.clone());
        state.studio_tracks.push(tr);
    }

    if let Some((t_idx, s_idx, path)) = actions.drop_slot {
        matrix::load_clip_into_slot(
            &mut state.matrix_state,
            &state.audio_proxy,
            t_idx,
            s_idx,
            path,
            state.transport.bpm,
        );
        state.dragged_sample = None;
    }

    // --- Drag & drop de clips del launcher (Bitwig: DRAG = mover, Ctrl/Alt+DRAG = copiar) ---
    if let Some((from_track, from_scene, clip, grab_offset)) = actions.clip_drag_start.take() {
        let is_copy = ui.input(|i| i.modifiers.ctrl || i.modifiers.alt);
        state.matrix_state.clip_drag = Some(matrix::MatrixClipDrag {
            from_track,
            from_scene,
            clip,
            grab_offset,
            is_copy,
            moved: true,
        });
        state.matrix_state.selected_slot = Some((from_track, from_scene));
    }
    if let Some(ref mut drag) = state.matrix_state.clip_drag {
        drag.is_copy = ui.input(|i| i.modifiers.ctrl || i.modifiers.alt);
    }
    if pointer_released {
        if let Some(drag) = state.matrix_state.clip_drag.take() {
            if let Some((dst_track, dst_scene)) = actions.clip_drop_target {
                if (dst_track, dst_scene) != (drag.from_track, drag.from_scene) {
                    if drag.is_copy {
                        state.matrix_state.copy_to_slot(
                            drag.from_track, drag.from_scene, dst_track, dst_scene, &state.audio_proxy,
                        );
                    } else {
                        state.matrix_state.move_slot(
                            drag.from_track, drag.from_scene, dst_track, dst_scene, &state.audio_proxy,
                        );
                    }
                    state.matrix_state.selected_slot = Some((dst_track, dst_scene));
                    state.selected_track_index = dst_track + 1;
                }
            }
            // Un drag no debe disparar el clip al soltar.
            actions.trigger_slot = None;
        }
    }
    // Ghost card flotante pegada al cursor (capa Tooltip, por encima del ScrollArea).
    if let Some(ref drag) = state.matrix_state.clip_drag {
        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
            let rect = Rect::from_min_size(pos - drag.grab_offset, vec2(78.0, CLIP_SLOT_H - 1.0));
            let p = ui.ctx().layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::new("mix_clip_drag_ghost"),
            ));
            let col = state
                .tracks()
                .get(drag.from_track + 1)
                .map(|t| t.color)
                .unwrap_or(ACCENT_ORANGE);
            p.rect_filled(rect.translate(vec2(2.0, 3.0)), 2.0, Color32::from_black_alpha(110));
            p.rect_filled(rect, 1.5, col.gamma_multiply(0.9));
            p.rect_stroke(rect, 1.5, Stroke::new(1.0_f32, Color32::WHITE), egui::StrokeKind::Inside);
            icons::paint_play_triangle(
                &p,
                Rect::from_min_size(pos2(rect.min.x + 3.0, rect.min.y + 4.0), vec2(12.0, 12.0)),
                Color32::from_rgb(0x10, 0x11, 0x13),
            );
            let name = if drag.clip.name.chars().count() > 9 {
                format!("{}…", drag.clip.name.chars().take(8).collect::<String>())
            } else {
                drag.clip.name.clone()
            };
            p.text(
                pos2(rect.min.x + 17.0, rect.center().y),
                Align2::LEFT_CENTER,
                if drag.is_copy { format!("+ {name}") } else { name },
                fonts::semibold(9.5),
                Color32::from_rgb(0x10, 0x11, 0x13),
            );
            ui.ctx().set_cursor_icon(if drag.is_copy {
                egui::CursorIcon::Copy
            } else {
                egui::CursorIcon::Grabbing
            });
        }
    }

    if let Some((t_idx, s_idx)) = actions.trigger_slot {
        matrix::trigger_pad(&mut state.matrix_state, &state.audio_proxy, t_idx, s_idx);
    }

    if let Some(t_idx) = actions.stop_track {
        state.matrix_state.stop_track(&state.audio_proxy, t_idx);
    }

    if let Some(s_idx) = trigger_scene {
        matrix::trigger_scene(&mut state.matrix_state, &state.audio_proxy, s_idx);
    }
}

#[allow(clippy::too_many_arguments)]
fn render_channel_strip(
    ui: &mut Ui,
    state: &mut AppState,
    idx: usize,
    show_launcher: bool,
    scenes_count: usize,
    slots_view_h: f32,
    scene_scroll: f32,
    pointer_released: bool,
    is_dragging_sample: bool,
    actions: &mut StripActions,
) -> f32 {
    let is_master = idx == 0;
    let is_sel = actions.selected_track_idx == idx;
    let pointer_pos = ui.input(|i| i.pointer.interact_pos().or_else(|| i.pointer.hover_pos()));
    let strip_w = state.tracks()[idx].width.clamp(MIN_TRACK_WIDTH, 94.0);

    let (
        name,
        track_color,
        mut vol,
        mut pan,
        mut mute,
        mut solo,
        mut arm,
        mut send_amt,
        route_dest_id,
        peak,
        effects_list,
        track_names,
    ) = {
        let tracks = state.tracks();
        let t = &tracks[idx];
        let peak = if is_master {
            state.smoothed_master_peak
        } else {
            state
                .smoothed_track_peaks
                .get(idx.saturating_sub(1))
                .copied()
                .unwrap_or(0.0)
        };
        let fx: Vec<(usize, String, bool)> = t
            .effects
            .iter()
            .enumerate()
            .map(|(i, s)| (i, s.name.clone(), s.active))
            .collect();
        let send0 = t.sends.first().map(|s| s.amount).unwrap_or(0.0);
        let names: Vec<String> = tracks.iter().map(|tr| tr.name.clone()).collect();
        let col = if is_master {
            Color32::from_rgb(0x82, 0x84, 0x88)
        } else {
            t.color
        };
        (
            t.name.clone(),
            col,
            t.volume,
            t.pan,
            t.mute,
            t.solo,
            t.arm,
            send0,
            t.route_destination_id,
            peak,
            fx,
            names,
        )
    };

    let strip_bg = if is_sel {
        Color32::from_rgb(0x34, 0x36, 0x3B)
    } else {
        Color32::from_rgb(0x28, 0x2A, 0x2E)
    };

    egui::Frame::NONE.fill(strip_bg).show(ui, |ui| {
        ui.set_min_width(strip_w);
        ui.set_max_width(strip_w);
        ui.spacing_mut().item_spacing = vec2(0.0, 1.0);

        ui.vertical(|ui| {
            // 1. Bloque superior SÓLIDO del color de la pista (exacto a Bitwig Mix View)
            let (hdr_rect, hdr_resp) =
                ui.allocate_exact_size(vec2(strip_w, HEADER_BLOCK_H), Sense::click());
            ui.painter().rect_filled(hdr_rect, 1.0, track_color);
            if is_sel {
                ui.painter().rect_stroke(
                    hdr_rect.shrink(1.0),
                    1.0,
                    Stroke::new(1.5_f32, Color32::WHITE),
                    egui::StrokeKind::Inside,
                );
            }
            ui.painter().text(
                hdr_rect.center(),
                Align2::CENTER_CENTER,
                &name,
                fonts::semibold(11.5),
                Color32::from_rgb(0x11, 0x12, 0x14),
            );
            if hdr_resp.clicked() {
                actions.selected_track_idx = idx;
            }
            if hdr_resp.double_clicked() {
                actions.selected_track_idx = idx;
                state.show_dsp_rack = !state.show_dsp_rack;
            }

            // 2. Ranuras verticales del Clip Launcher + fila de Stop por pista
            if show_launcher {
                if !is_master {
                    let matrix_idx = idx.saturating_sub(1);
                    scene_viewport(ui, strip_w, slots_view_h, scene_scroll, scenes_content_h(scenes_count), ("mix_slots", idx), |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 1.0);
                    for scene_idx in 0..scenes_count {
                        let slot = state.matrix_state.slot(matrix_idx, scene_idx);
                        let slot_state = slot.map(|s| s.state.clone()).unwrap_or(SlotState::Empty);
                        let clip_opt = slot.and_then(|s| s.clip.as_ref());

                        let (slot_rect, slot_resp) = ui.allocate_exact_size(
                            vec2(strip_w, CLIP_SLOT_H),
                            if clip_opt.is_some() { Sense::click_and_drag() } else { Sense::click() },
                        );
                        let cell = slot_rect.shrink2(vec2(1.0, 0.5));
                        let clip_drag_active = state.matrix_state.clip_drag.as_ref();
                        let is_drag_source = clip_drag_active
                            .is_some_and(|d| d.from_track == matrix_idx && d.from_scene == scene_idx && !d.is_copy);
                        let ptr_in_slot = pointer_pos.is_some_and(|p| slot_rect.contains(p) && ui.clip_rect().contains(p));
                        let is_drag_target = clip_drag_active.is_some() && ptr_in_slot;

                        if let Some(clip) = clip_opt {
                            let is_playing = slot_state == SlotState::Playing;
                            let base = if is_playing {
                                track_color
                            } else {
                                track_color.gamma_multiply(0.75)
                            };
                            // Bitwig: el slot origen queda atenuado mientras el clip "vuela".
                            ui.painter().rect_filled(
                                cell,
                                1.5,
                                if is_drag_source { base.gamma_multiply(0.35) } else { base },
                            );
                            if slot_resp.drag_started_by(egui::PointerButton::Primary) {
                                let origin = ui
                                    .input(|i| i.pointer.press_origin())
                                    .unwrap_or(cell.min);
                                actions.clip_drag_start =
                                    Some((matrix_idx, scene_idx, clip.clone(), origin - cell.min));
                            }
                            let icon_r = Rect::from_min_size(
                                pos2(cell.min.x + 3.0, cell.min.y + 4.0),
                                vec2(12.0, 12.0),
                            );
                            if is_playing {
                                icons::paint_stop_square(
                                    ui.painter(),
                                    icon_r,
                                    Color32::from_rgb(0x10, 0x11, 0x13),
                                );
                            } else {
                                icons::paint_play_triangle(
                                    ui.painter(),
                                    icon_r,
                                    Color32::from_rgb(0x10, 0x11, 0x13),
                                );
                            }
                            let short_name = if clip.name.len() > 9 {
                                format!("{}…", &clip.name[..8])
                            } else {
                                clip.name.clone()
                            };
                            ui.painter().text(
                                pos2(cell.min.x + 17.0, cell.center().y),
                                Align2::LEFT_CENTER,
                                short_name,
                                fonts::semibold(9.5),
                                Color32::from_rgb(0x10, 0x11, 0x13),
                            );
                        } else {
                            // Gris neutro de Bitwig para los pads vacíos
                            // (#2E2E2E), sin el tinte azulado anterior.
                            ui.painter().rect_filled(
                                cell,
                                1.5,
                                if slot_resp.hovered() {
                                    Color32::from_rgb(0x3A, 0x3A, 0x3A)
                                } else {
                                    Color32::from_rgb(0x2E, 0x2E, 0x2E)
                                },
                            );
                            let icon_r = Rect::from_min_size(
                                pos2(cell.min.x + 3.0, cell.min.y + 4.0),
                                vec2(12.0, 12.0),
                            );
                            if arm {
                                icons::paint_record_circle(ui.painter(), icon_r, REC_RED, false);
                            } else {
                                // El cuadradito del pad vacío en Bitwig es gris
                                // medio (#535353), no casi negro.
                                icons::paint_stop_square(
                                    ui.painter(),
                                    icon_r,
                                    Color32::from_rgb(0x53, 0x53, 0x53),
                                );
                            }
                        }

                        if is_dragging_sample && slot_resp.hovered() {
                            ui.painter().rect_stroke(
                                cell,
                                1.5,
                                Stroke::new(2.0_f32, ACCENT_ORANGE),
                                egui::StrokeKind::Inside,
                            );
                            if pointer_released {
                                if let Some(ref p) = state.dragged_sample {
                                    actions.drop_slot = Some((matrix_idx, scene_idx, p.clone()));
                                }
                            }
                        }
                        if is_drag_target {
                            ui.painter().rect_stroke(
                                cell,
                                1.5,
                                Stroke::new(2.0_f32, Color32::WHITE),
                                egui::StrokeKind::Inside,
                            );
                            if pointer_released {
                                actions.clip_drop_target = Some((matrix_idx, scene_idx));
                            }
                        }
                        if slot_resp.clicked() {
                            actions.trigger_slot = Some((matrix_idx, scene_idx));
                            actions.selected_track_idx = idx;
                        }
                    }
                    });

                    // Fila Track Stop `[■ | ≡]`
                    let (stop_bar_rect, _) =
                        ui.allocate_exact_size(vec2(strip_w, STOP_ROW_H), Sense::hover());
                    ui.painter()
                        .rect_filled(stop_bar_rect, 0.0, Color32::from_rgb(0x23, 0x25, 0x29));
                    let stop_btn_r = Rect::from_min_size(
                        pos2(stop_bar_rect.min.x + 3.0, stop_bar_rect.min.y + 2.0),
                        vec2(16.0, 16.0),
                    );
                    let stop_resp = ui.interact(
                        stop_btn_r,
                        ui.id().with(("mix_stop_trk", idx)),
                        Sense::click(),
                    );
                    icons::paint_stop_square(
                        ui.painter(),
                        stop_btn_r.shrink(3.0),
                        if stop_resp.hovered() {
                            Color32::WHITE
                        } else {
                            Color32::from_rgb(0xC6, 0xC8, 0xCE)
                        },
                    );
                    if stop_resp.on_hover_text("Stop Track Clips").clicked() {
                        actions.stop_track = Some(matrix_idx);
                    }
                    let menu_r = Rect::from_min_size(
                        pos2(stop_bar_rect.max.x - 18.0, stop_bar_rect.min.y + 2.0),
                        vec2(14.0, 16.0),
                    );
                    icons::paint_menu_bars(
                        ui.painter(),
                        menu_r,
                        Color32::from_rgb(0x8E, 0x92, 0x9A),
                    );
                } else {
                    let spacer_h = slots_view_h + 1.0 + STOP_ROW_H;
                    let (sp_rect, _) = ui.allocate_exact_size(vec2(strip_w, spacer_h), Sense::hover());
                    ui.painter().rect_filled(sp_rect, 0.0, BW_DARK_WELL);
                }
            }

            // 3. Pozo oscuro de DEVICES (`58px` alto)
            let (dev_rect, _) =
                ui.allocate_exact_size(vec2(strip_w, DEVICES_WELL_H), Sense::hover());
            ui.painter()
                .rect_filled(dev_rect, 0.0, Color32::from_rgb(0x1B, 0x1C, 0x1F));
            ui.painter().rect_stroke(
                dev_rect,
                0.0,
                Stroke::new(1.0_f32, Color32::from_rgb(0x10, 0x11, 0x13)),
                egui::StrokeKind::Inside,
            );
            let mut dev_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(dev_rect.shrink2(vec2(3.0, 3.0)))
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            dev_ui.spacing_mut().item_spacing.y = 2.0;
            dev_ui.horizontal(|ui| {
                ui.menu_button(RichText::new("+").size(10.0).strong(), |ui| {
                    for plugin in &PLUGIN_CATALOG {
                        if ui.button(plugin.label).clicked() {
                            let mut slot = DspSlot::default();
                            slot.name = plugin.name.to_string();
                            state.tracks_mut()[idx].effects.push(slot);
                            actions.selected_track_idx = idx;
                            state.show_dsp_rack = true;
                            ui.close_menu();
                        }
                    }
                });
            });
            for (fx_idx, fx_name, fx_active) in effects_list.iter().take(2) {
                let short_fx = if fx_name.len() > 10 {
                    format!("{}…", &fx_name[..9])
                } else {
                    fx_name.clone()
                };
                let (chip_r, chip_resp) = dev_ui.allocate_exact_size(
                    vec2(strip_w - 8.0, 15.0),
                    Sense::click(),
                );
                dev_ui.painter().rect_filled(
                    chip_r,
                    2.0,
                    if *fx_active {
                        Color32::from_rgb(0x36, 0x38, 0x3E)
                    } else {
                        Color32::from_rgb(0x23, 0x24, 0x28)
                    },
                );
                dev_ui.painter().circle_filled(
                    pos2(chip_r.min.x + 5.0, chip_r.center().y),
                    2.2,
                    if *fx_active { ACCENT_ORANGE } else { TEXT_MUTED },
                );
                dev_ui.painter().text(
                    pos2(chip_r.min.x + 10.5, chip_r.center().y),
                    Align2::LEFT_CENTER,
                    short_fx,
                    fonts::semibold(9.0),
                    TEXT_PRIMARY,
                );
                if chip_resp.clicked() {
                    actions.selected_track_idx = idx;
                    state.selected_slot_index = *fx_idx;
                    state.show_dsp_rack = true;
                }
            }

            // 4. Caja de SENDS con perilla rotatoria GPU (`wgpu` SDF!)
            let (sends_rect, _) =
                ui.allocate_exact_size(vec2(strip_w, SENDS_BOX_H), Sense::hover());
            ui.painter()
                .rect_filled(sends_rect, 0.0, Color32::from_rgb(0x46, 0x48, 0x4E));
            if !is_master {
                let mut s_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(sends_rect.shrink2(vec2(4.0, 3.0)))
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                s_ui.spacing_mut().item_spacing.x = 4.0;
                s_ui.label(
                    RichText::new("FX 1")
                        .font(fonts::regular(9.5))
                        .color(Color32::from_rgb(0x14, 0x15, 0x18)),
                );
                let (s_resp, s_changed) = rotary_knob(
                    &mut s_ui,
                    24.0,
                    &mut send_amt,
                    0.0..=1.0,
                    0.0,
                    BW_FADER_AMBER,
                    false,
                );
                s_resp.on_hover_text("FX 1 Send");
                if s_changed {
                    let tr = &mut state.tracks_mut()[idx];
                    if let Some(first_send) = tr.sends.first_mut() {
                        first_send.amount = send_amt;
                    } else {
                        tr.sends.push(SendConnection {
                            target_id: mixer::ROUTE_FX_BASE,
                            amount: send_amt,
                        });
                    }
                }
            }

            // 5. Sección I/O (`All Ins` / `Master` o `Out 1/2`)
            let (io_rect, _) = ui.allocate_exact_size(vec2(strip_w, IO_BOX_H), Sense::hover());
            ui.painter()
                .rect_filled(io_rect, 0.0, Color32::from_rgb(0x32, 0x34, 0x39));
            let in_label = if is_master { "Audio In" } else { "All Ins" };
            let out_label = mixer::route_dest_label(route_dest_id, is_master, &track_names);
            for (row_i, text) in [in_label, out_label.as_str()].into_iter().enumerate() {
                let pill_r = Rect::from_min_size(
                    pos2(io_rect.min.x + 3.0, io_rect.min.y + 2.0 + row_i as f32 * 17.0),
                    vec2(strip_w - 6.0, 15.0),
                );
                ui.painter()
                    .rect_filled(pill_r, 1.5, Color32::from_rgb(0x46, 0x48, 0x4F));
                ui.painter().text(
                    pos2(pill_r.min.x + 4.0, pill_r.center().y),
                    Align2::LEFT_CENTER,
                    text,
                    fonts::regular(9.5),
                    Color32::from_rgb(0xE8, 0xEA, 0xEE),
                );
                let caret_r = Rect::from_min_size(
                    pos2(pill_r.max.x - 12.0, pill_r.min.y + 3.0),
                    vec2(9.0, 9.0),
                );
                icons::paint_caret_down(
                    ui.painter(),
                    caret_r,
                    Color32::from_rgb(0x18, 0x19, 0x1C),
                );
            }

            // 6. Cápsulas unidas GPU `[● | S | M]` + Pan bipolar GPU + dB + Fader/Meter GPU
            egui::Frame::NONE
                .fill(strip_bg)
                .inner_margin(egui::Margin::symmetric(4, 3))
                .show(ui, |ui| {
                    let inner_w = strip_w - 8.0;
                    ui.set_min_width(inner_w);
                    ui.set_max_width(inner_w);
                    ui.spacing_mut().item_spacing.y = 3.0;

                    // Grupo unido `[● | S | M]` renderizado en wgpu
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        if !is_master {
                            let seg_w = (inner_w / 3.0).floor();
                            if capsule_icon_button(
                                ui,
                                vec2(seg_w, 18.0),
                                arm,
                                REC_RED,
                                JoinSide::Left,
                                |p, r, fg| icons::paint_record_circle(p, r, fg, true),
                            )
                            .on_hover_text("Record Arm")
                            .clicked()
                            {
                                arm = !arm;
                                state.tracks_mut()[idx].arm = arm;
                            }

                            if capsule_text_button(
                                ui,
                                vec2(seg_w, 18.0),
                                "S",
                                9.5,
                                solo,
                                SOLO_YELLOW,
                                JoinSide::Middle,
                                false,
                            )
                            .on_hover_text("Solo")
                            .clicked()
                            {
                                solo = !solo;
                                state.tracks_mut()[idx].solo = solo;
                                let m_idx = idx.saturating_sub(1);
                                if let Some(mt) = state.matrix_state.tracks.get_mut(m_idx) {
                                    mt.soloed = solo;
                                }
                                state.audio_proxy.send(GuiCommand::SetTrackSolo {
                                    track_idx: m_idx,
                                    solo,
                                });
                            }

                            if capsule_text_button(
                                ui,
                                vec2(inner_w - seg_w * 2.0, 18.0),
                                "M",
                                9.5,
                                mute,
                                MUTE_BLUE,
                                JoinSide::Right,
                                false,
                            )
                            .on_hover_text("Mute")
                            .clicked()
                            {
                                mute = !mute;
                                state.tracks_mut()[idx].mute = mute;
                                let m_idx = idx.saturating_sub(1);
                                if let Some(mt) = state.matrix_state.tracks.get_mut(m_idx) {
                                    mt.muted = mute;
                                }
                                state.audio_proxy.send(GuiCommand::SetTrackMute {
                                    track_idx: m_idx,
                                    mute,
                                });
                            }
                        } else {
                            let half_w = (inner_w * 0.5).floor();
                            if capsule_text_button(
                                ui,
                                vec2(half_w, 18.0),
                                "S",
                                9.5,
                                solo,
                                SOLO_YELLOW,
                                JoinSide::Left,
                                false,
                            )
                            .clicked()
                            {
                                solo = !solo;
                                state.tracks_mut()[idx].solo = solo;
                            }
                            if capsule_text_button(
                                ui,
                                vec2(inner_w - half_w, 18.0),
                                "M",
                                9.5,
                                mute,
                                MUTE_BLUE,
                                JoinSide::Right,
                                false,
                            )
                            .clicked()
                            {
                                mute = !mute;
                                state.tracks_mut()[idx].mute = mute;
                            }
                        }
                    });

                    // Barra horizontal de Pan bipolar GPU (`wgpu` SDF)
                    let (pan_resp, pan_changed) = fader_horizontal_range(
                        ui,
                        vec2(inner_w, 13.0),
                        &mut pan,
                        -1.0..=1.0,
                        0.0,
                        BW_FADER_AMBER,
                        BW_FADER_ORANGE,
                        true,
                    );
                    pan_resp.on_hover_text("Pan (double-click to center)");
                    if pan_changed {
                        state.tracks_mut()[idx].pan = pan;
                        if !is_master {
                            let m_idx = idx.saturating_sub(1);
                            if let Some(mt) = state.matrix_state.tracks.get_mut(m_idx) {
                                mt.pan = pan;
                            }
                            state.audio_proxy.send(GuiCommand::SetTrackPan {
                                track_idx: m_idx,
                                pan,
                            });
                        }
                    }

                    // Fila de lectura numérica de pico + ganancia en dB naranja Bitwig
                    let (db_row, _) = ui.allocate_exact_size(vec2(inner_w, 14.0), Sense::hover());
                    let (pk_txt, pk_clip) = mixer::peak_db_readout(peak);
                    ui.painter().text(
                        pos2(db_row.min.x + 1.0, db_row.center().y),
                        Align2::LEFT_CENTER,
                        pk_txt,
                        fonts::lcd(9.5),
                        if pk_clip {
                            REC_RED
                        } else {
                            Color32::from_rgb(0x88, 0x8C, 0x94)
                        },
                    );
                    ui.painter().text(
                        pos2(db_row.max.x - 1.0, db_row.center().y),
                        Align2::RIGHT_CENTER,
                        format_strip_db(vol),
                        fonts::lcd(10.5),
                        BW_FADER_ORANGE,
                    );

                    // Bahía hundida de Escala dB + Stereo Meter GPU + Fader Vertical GPU
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;

                        // Escala dB estilo Bitwig (`0, 6, 12, 18, 24, 36, ∞`)
                        let (scale_r, _) =
                            ui.allocate_exact_size(vec2(14.0, FADER_BAY_H - 4.0), Sense::hover());
                        for (t_norm, lbl) in [
                            (0.75_f32, "0"),
                            (0.675, "6"),
                            (0.58, "12"),
                            (0.47, "18"),
                            (0.35, "24"),
                            (0.20, "36"),
                            (0.03, "inf"),
                        ] {
                            let y = scale_r.max.y - scale_r.height() * t_norm;
                            ui.painter().text(
                                pos2(scale_r.max.x - 1.0, y),
                                Align2::RIGHT_CENTER,
                                lbl,
                                fonts::regular(8.0),
                                Color32::from_rgb(0x8C, 0x90, 0x98),
                            );
                        }

                        // Medidor estéreo GPU (`wgpu` SDF)
                        let peak_r = (peak * 0.96).clamp(0.0, 1.0);
                        stereo_meter_bar(
                            ui,
                            vec2(14.0, FADER_BAY_H - 4.0),
                            [
                                peak,
                                peak_r,
                                (peak * 1.04).min(1.0),
                                (peak_r * 1.04).min(1.0),
                            ],
                        );

                        // Fader vertical ancho cobre-ámbar GPU (`wgpu` SDF)
                        let fader_w = (inner_w - 14.0 - 14.0 - 6.0).max(28.0);
                        let (vol_resp, vol_changed) = fader_vertical_track_vol(
                            ui,
                            vec2(fader_w, FADER_BAY_H - 4.0),
                            &mut vol,
                            0.75,
                        );
                        vol_resp.on_hover_text("Volume Fader (double-click for 0.0 dB)");
                        if vol_changed {
                            state.tracks_mut()[idx].volume = vol;
                            let eng_idx = if is_master { 0 } else { idx.saturating_sub(1) };
                            if !is_master {
                                if let Some(mt) = state.matrix_state.tracks.get_mut(eng_idx) {
                                    mt.volume = vol;
                                }
                            }
                            state.audio_proxy.send(GuiCommand::SetTrackVolume {
                                track_idx: eng_idx,
                                volume_db: vol,
                            });
                        }
                    });
                });

            // 7. Pie inferior sólido de 4px del color de la pista
            let (foot_r, _) = ui.allocate_exact_size(vec2(strip_w, 4.0), Sense::hover());
            ui.painter().rect_filled(foot_r, 0.5, track_color);
        });
    });

    strip_w
}
