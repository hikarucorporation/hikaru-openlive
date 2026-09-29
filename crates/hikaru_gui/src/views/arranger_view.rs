// Copyright (C) Hikaru Corporation - 2026
// Hikaru OpenLive - Arranger View (Bitwig-style Session / Arranger Matrix Launcher)
// GNU Affero General Public License v3
// crates/hikaru_gui/src/views/arranger_view.rs

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use hikaru_audio_engine::AudioEngine;

use gpui_kit::component::*;
use gpui_kit::component::button::{Button, ButtonRounded};
use gpui_kit::component::label::Label;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::{FluentBuilder as _, InteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, HikaruApp};
use crate::audio_proxy::GuiCommand;
use crate::theme;
use crate::views::matrix::{self, SessionMatrixState, SlotState};
use crate::views::mixer::Track;

// =========================================================================
// GEOMETRÍA (idéntica a la versión legacy en egui)
// =========================================================================

const SCENE_LABEL_WIDTH: f32 = 100.0;
const TRACK_WIDTH: f32 = 110.0;
const PAD_HEIGHT: f32 = 28.0;
const PAD_GAP: f32 = 2.0;
const HEADER_HEIGHT: f32 = 40.0;
const FADER_ROW_HEIGHT: f32 = 240.0;

const PAN_WIDTH: f32 = 90.0;
const PAN_HEIGHT: f32 = 20.0;
const PAN_THUMB_W: f32 = 12.0;

const FADER_WIDTH: f32 = 30.0;
const FADER_HEIGHT: f32 = 160.0;
const FADER_THUMB_H: f32 = 18.0;
const FADER_RAIL_W: f32 = 24.0;

// =========================================================================
// ESTADO DE ARRASTRE DE LOS FADERS
// =========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaderTarget {
    MasterPan,
    MasterVolume,
    TrackPan(usize),
    TrackVolume(usize),
}

fn start_fader_drag(cx: &mut App, target: FaderTarget) {
    let st = state(cx);
    st.update(cx, |s, _| s.arranger_fader_drag = Some(target));
}

fn stop_fader_drag(cx: &mut App, target: FaderTarget) {
    let st = state(cx);
    st.update(cx, |s, _| {
        if s.arranger_fader_drag == Some(target) {
            s.arranger_fader_drag = None;
        }
    });
}

fn is_fader_dragging(cx: &mut App, target: FaderTarget) -> bool {
    state(cx).read(cx).arranger_fader_drag == Some(target)
}

fn set_pan(cx: &mut App, target: FaderTarget, pan: f32) {
    let pan = pan.clamp(-1.0, 1.0);
    let st = state(cx);
    st.update(cx, |s, cx| {
        match target {
            FaderTarget::MasterPan => {
                if let Some(t) = s.live_tracks.first_mut() {
                    t.pan = pan;
                }
            }
            FaderTarget::TrackPan(idx) => {
                let Some(t) = s.matrix_state.tracks.get_mut(idx) else {
                    return;
                };
                t.pan = pan;
                if let Some(live) = s.live_tracks.get_mut(idx + 1) {
                    live.pan = pan;
                }
                s.audio_proxy.send(GuiCommand::SetTrackPan {
                    track_idx: idx,
                    pan,
                });
            }
            _ => return,
        }
        cx.notify();
    });
}

fn set_volume(cx: &mut App, target: FaderTarget, volume: f32) {
    let volume = volume.clamp(0.0, 1.0);
    let st = state(cx);
    st.update(cx, |s, cx| {
        match target {
            FaderTarget::MasterVolume => {
                if let Some(t) = s.live_tracks.first_mut() {
                    t.volume = volume;
                }
            }
            FaderTarget::TrackVolume(idx) => {
                let Some(t) = s.matrix_state.tracks.get_mut(idx) else {
                    return;
                };
                t.volume = volume;
                if let Some(live) = s.live_tracks.get_mut(idx + 1) {
                    live.volume = volume;
                }
                s.audio_proxy.send(GuiCommand::SetTrackVolume {
                    track_idx: idx,
                    volume_db: volume,
                });
            }
            _ => return,
        }
        cx.notify();
    });
}

// El canvas de cada fader guarda sus bounds en pantalla para poder traducir
// la posición del puntero (coordenadas de ventana) a un valor 0..=1 / -1..=1.
fn record_bounds(
    slot: &Rc<Cell<[f32; 4]>>,
    bounds: Bounds<Pixels>,
) {
    slot.set([
        bounds.origin.x.as_f32(),
        bounds.origin.y.as_f32(),
        bounds.size.width.as_f32(),
        bounds.size.height.as_f32(),
    ]);
}

fn pan_from_pointer(b: [f32; 4], x: f32) -> f32 {
    let travel = (b[2] - PAN_THUMB_W).max(1.0);
    let norm = ((x - b[0] - PAN_THUMB_W / 2.0) / travel).clamp(0.0, 1.0);
    norm * 2.0 - 1.0
}

fn volume_from_pointer(b: [f32; 4], y: f32) -> f32 {
    let travel = (b[3] - FADER_THUMB_H).max(1.0);
    (1.0 - ((y - b[1] - FADER_THUMB_H / 2.0) / travel)).clamp(0.0, 1.0)
}

fn pan_text(pan: f32) -> String {
    let pan_int = (pan * 100.0).round() as i32;
    match pan_int {
        0 => "C".to_string(),
        v if v < 0 => format!("L{}", v.abs()),
        v => format!("R{}", v),
    }
}

// =========================================================================
// HELPERS DE ESTADO
// =========================================================================

fn slot_colors(slot_state: &SlotState, has_clip: bool) -> (Hsla, Hsla) {
    if !has_clip {
        return (rgb(0x191919).into(), rgb(0x282828).into());
    }
    match slot_state {
        SlotState::Playing => (rgb(0x1E6ED2).into(), rgb(0x5AB4FF).into()),
        SlotState::QueuedToPlay => (rgb(0x78641E).into(), rgb(0xE6C84C).into()),
        SlotState::Stopped => (rgb(0x2D374B).into(), rgb(0x5A82BE).into()),
        SlotState::QueuedToStop => (rgb(0x822D2D).into(), rgb(0xE06060).into()),
        SlotState::Empty => (rgb(0x191919).into(), rgb(0x282828).into()),
    }
}

fn slot_display_text(
    slot_state: &SlotState,
    has_clip: bool,
    clip_name: &str,
) -> String {
    if !has_clip {
        return String::new();
    }
    match slot_state {
        SlotState::Playing | SlotState::QueuedToPlay => {
            if clip_name.is_empty() {
                "Clip".to_string()
            } else {
                clip_name.to_string()
            }
        }
        SlotState::QueuedToStop => "Stop".to_string(),
        _ => clip_name.to_string(),
    }
}

fn set_mute(cx: &mut App, track_idx: Option<usize>, muted: bool) {
    let st = state(cx);
    st.update(cx, |s, cx| {
        match track_idx {
            None => {
                if let Some(t) = s.live_tracks.first_mut() {
                    t.mute = muted;
                }
            }
            Some(idx) => {
                if let Some(t) = s.matrix_state.tracks.get_mut(idx) {
                    t.muted = muted;
                }
                if let Some(live) = s.live_tracks.get_mut(idx + 1) {
                    live.mute = muted;
                }
                s.audio_proxy.send(GuiCommand::SetTrackMute {
                    track_idx: idx,
                    mute: muted,
                });
            }
        }
        cx.notify();
    });
}

fn set_solo(cx: &mut App, track_idx: Option<usize>, soloed: bool) {
    let st = state(cx);
    st.update(cx, |s, cx| {
        match track_idx {
            None => {
                if let Some(t) = s.live_tracks.first_mut() {
                    t.solo = soloed;
                }
            }
            Some(idx) => {
                if let Some(t) = s.matrix_state.tracks.get_mut(idx) {
                    t.soloed = soloed;
                }
                if let Some(live) = s.live_tracks.get_mut(idx + 1) {
                    live.solo = soloed;
                }
                s.audio_proxy.send(GuiCommand::SetTrackSolo {
                    track_idx: idx,
                    solo: soloed,
                });
            }
        }
        cx.notify();
    });
}

fn toggle_mute(cx: &mut App, track_idx: Option<usize>) {
    let muted = {
        let app = state(cx).read(cx);
        match track_idx {
            None => app.live_tracks.first().map(|t| t.mute).unwrap_or(false),
            Some(idx) => app
                .matrix_state
                .tracks
                .get(idx)
                .map(|t| t.muted)
                .unwrap_or(false),
        }
    };
    set_mute(cx, track_idx, !muted);
}

fn toggle_solo(cx: &mut App, track_idx: Option<usize>) {
    let soloed = {
        let app = state(cx).read(cx);
        match track_idx {
            None => app.live_tracks.first().map(|t| t.solo).unwrap_or(false),
            Some(idx) => app
                .matrix_state
                .tracks
                .get(idx)
                .map(|t| t.soloed)
                .unwrap_or(false),
        }
    };
    set_solo(cx, track_idx, !soloed);
}

// =========================================================================
// CONTROLES
// =========================================================================

fn pan_slider(target: FaderTarget, pan: f32) -> AnyElement {
    let slot: Rc<Cell<[f32; 4]>> = Rc::new(Cell::new([0.0; 4]));
    let b_down = slot.clone();
    let b_move = slot.clone();
    let b_paint = slot.clone();

    let norm = (pan.clamp(-1.0, 1.0) + 1.0) / 2.0;
    let thumb_x = PAN_THUMB_W / 2.0 + norm * (PAN_WIDTH - PAN_THUMB_W);
    let center_x = PAN_WIDTH / 2.0;
    let fill_w = (thumb_x - center_x).abs();
    let fill_left = thumb_x.min(center_x);
    let label = pan_text(pan);

    div()
        .id(SharedString::from(format!("arr_pan_{:?}", target)))
        .relative()
        .w(px(PAN_WIDTH))
        .h(px(PAN_HEIGHT))
        .flex_shrink_0()
        .on_mouse_down(gpui_kit::MouseButton::Left, move |event, _, cx| {
            let b = b_down.get();
            start_fader_drag(cx, target);
            set_pan(cx, target, pan_from_pointer(b, event.position.x.as_f32()));
        })
        .on_mouse_move(move |event, _, cx| {
            if !is_fader_dragging(cx, target) {
                return;
            }
            let b = b_move.get();
            set_pan(cx, target, pan_from_pointer(b, event.position.x.as_f32()));
        })
        .on_mouse_up(gpui_kit::MouseButton::Left, move |_, _, cx| {
            stop_fader_drag(cx, target)
        })
        .child(
            canvas(
                move |bounds, _, _| record_bounds(&b_paint, bounds),
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        )
        // Riel
        .child(
            div()
                .absolute()
                .left(px(4.0))
                .top(px(7.0))
                .w(px(PAN_WIDTH - 8.0))
                .h(px(6.0))
                .bg(rgb(0x2D2D2D))
                .rounded(px(1.0)),
        )
        // Relleno centro -> thumb
        .child(
            div()
                .absolute()
                .left(px(fill_left))
                .top(px(7.0))
                .w(px(fill_w))
                .h(px(6.0))
                .bg(rgb(0x0096BE)),
        )
        // Tick central
        .child(
            div()
                .absolute()
                .left(px(center_x - 0.5))
                .top(px(5.0))
                .w(px(1.0))
                .h(px(10.0))
                .bg(rgb(0x808080)),
        )
        // Thumb
        .child(
            div()
                .absolute()
                .left(px(thumb_x - PAN_THUMB_W / 2.0))
                .top(px(1.0))
                .w(px(PAN_THUMB_W))
                .h(px(18.0))
                .bg(rgb(0x00A2E8))
                .border_1()
                .border_color(rgb(0x000000)),
        )
        // Texto C / Lxx / Rxx
        .child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Label::new(label)
                        .text_size(px(9.0))
                        .text_color(rgb(0xFFFFFF)),
                ),
        )
        .into_any_element()
}

fn volume_fader(target: FaderTarget, volume: f32) -> AnyElement {
    let slot: Rc<Cell<[f32; 4]>> = Rc::new(Cell::new([0.0; 4]));
    let b_down = slot.clone();
    let b_move = slot.clone();
    let b_paint = slot.clone();

    let half_thumb = FADER_THUMB_H / 2.0;
    let travel = FADER_HEIGHT - FADER_THUMB_H;
    let thumb_y = half_thumb + (1.0 - volume.clamp(0.0, 1.0)) * travel;
    let rail_left = (FADER_WIDTH - FADER_RAIL_W) / 2.0;

    div()
        .id(SharedString::from(format!("arr_vol_{:?}", target)))
        .relative()
        .w(px(FADER_WIDTH))
        .h(px(FADER_HEIGHT))
        .flex_shrink_0()
        .on_mouse_down(gpui_kit::MouseButton::Left, move |event, _, cx| {
            let b = b_down.get();
            start_fader_drag(cx, target);
            set_volume(cx, target, volume_from_pointer(b, event.position.y.as_f32()));
        })
        .on_mouse_move(move |event, _, cx| {
            if !is_fader_dragging(cx, target) {
                return;
            }
            let b = b_move.get();
            set_volume(cx, target, volume_from_pointer(b, event.position.y.as_f32()));
        })
        .on_mouse_up(gpui_kit::MouseButton::Left, move |_, _, cx| {
            stop_fader_drag(cx, target)
        })
        .child(
            canvas(
                move |bounds, _, _| record_bounds(&b_paint, bounds),
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        )
        // Riel
        .child(
            div()
                .absolute()
                .left(px(rail_left))
                .top(px(-2.0))
                .w(px(FADER_RAIL_W))
                .h(px(FADER_HEIGHT + 4.0))
                .bg(rgb(0x2D2D2D))
                .rounded(px(1.0)),
        )
        // Relleno thumb -> base
        .child(
            div()
                .absolute()
                .left(px(rail_left))
                .top(px(thumb_y))
                .w(px(FADER_RAIL_W))
                .h(px((FADER_HEIGHT + 2.0 - thumb_y).max(0.0)))
                .bg(rgb(0x0096BE)),
        )
        // Thumb
        .child(
            div()
                .absolute()
                .left(px(-7.5))
                .top(px(thumb_y - half_thumb))
                .w(px(FADER_THUMB_H * 2.5))
                .h(px(FADER_THUMB_H))
                .bg(rgb(0x00A2E8))
                .border_1()
                .border_color(rgb(0x000000)),
        )
        .into_any_element()
}

fn mute_solo_buttons(id_prefix: &str, track_idx: Option<usize>, muted: bool, soloed: bool) -> AnyElement {
    h_flex()
        .gap(px(2.0))
        .flex_shrink_0()
        .child(
            Button::new(SharedString::from(format!("{}_mute", id_prefix)))
                .label("M")
                .compact()
                .rounded(ButtonRounded::None)
                .when(muted, |b| b.text_color(rgb(0xE06060)))
                .on_click(move |_, _, cx| toggle_mute(cx, track_idx)),
        )
        .child(
            Button::new(SharedString::from(format!("{}_solo", id_prefix)))
                .label("S")
                .compact()
                .rounded(ButtonRounded::None)
                .when(soloed, |b| b.text_color(rgb(0xE6C84C)))
                .on_click(move |_, _, cx| toggle_solo(cx, track_idx)),
        )
        .into_any_element()
}

// =========================================================================
// PADS DE LA GRILLA
// =========================================================================

#[allow(clippy::too_many_arguments)]
fn render_pad(
    track_idx: usize,
    scene_idx: usize,
    slot_state: SlotState,
    has_clip: bool,
    clip_name: String,
    is_selected: bool,
    clipboard_ready: bool,
    drop_sample_ready: bool,
    engine_handle: Option<Arc<Mutex<AudioEngine<'static>>>>,
) -> AnyElement {
    let (bg, border) = slot_colors(&slot_state, has_clip);
    let display_text = slot_display_text(&slot_state, has_clip, &clip_name);
    let is_playing = slot_state == SlotState::Playing;

    let pad = div()
        .id(SharedString::from(format!("arr_pad_{}_{}", track_idx, scene_idx)))
        .relative()
        .w(px(TRACK_WIDTH))
        .h(px(PAD_HEIGHT))
        .bg(bg)
        .border_1()
        .border_color(border)
        .when(is_selected, |d| d.border_2().border_color(rgb(0xE0E0E0)))
        .when(drop_sample_ready, |d| d.border_color(rgb(0x0096BE)))
        .rounded(px(2.0))
        .overflow_hidden()
        .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| {
            let st = state(cx);
            st.update(cx, |s, cx| {
                if let Some(path) = s.dragged_sample.take() {
                    if crate::views::explorer::is_audio_file(&path) {
                        s.matrix_state.selected_slot = Some((track_idx, scene_idx));
                        let bpm = s.transport.bpm;
                        let proxy = s.audio_proxy.clone();
                        load_clip_into_slot(
                            &mut s.matrix_state,
                            &proxy,
                            track_idx,
                            scene_idx,
                            path,
                            bpm,
                        );
                    }
                } else {
                    s.matrix_state.selected_slot = Some((track_idx, scene_idx));
                }
                cx.notify();
            });
        })
        // Mini playhead del clip en reproducción
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    if !is_playing {
                        return;
                    }
                    let Some(ref handle) = engine_handle else {
                        return;
                    };
                    let Ok(engine) = handle.try_lock() else {
                        return;
                    };
                    let Some((frame, total)) = engine.voice_playhead_frame(track_idx, scene_idx)
                    else {
                        return;
                    };
                    if total == 0 {
                        return;
                    }
                    let progress = (frame as f32 / total as f32).clamp(0.0, 1.0);
                    let x = bounds.origin.x + bounds.size.width * progress;
                    let mut path = PathBuilder::stroke(px(1.5));
                    path.move_to(point(x, bounds.origin.y + px(2.0)));
                    path.line_to(point(x, bounds.origin.y + bounds.size.height - px(2.0)));
                    if let Ok(path) = path.build() {
                        window.paint_path(path, rgb(0xFFFFFF));
                    }
                },
            )
            .absolute()
            .inset_0(),
        )
        // Botón central Play / Stop
        .child(
            div()
                .absolute()
                .left(px(5.0))
                .top(px(5.0))
                .w(px(18.0))
                .h(px(18.0))
                .rounded(px(2.0))
                .bg(if has_clip { rgb(0x1B6FB5) } else { rgb(0x32323C) })
                .border_1()
                .border_color(rgb(0x5A5A5A))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |s, cx| {
                        if has_clip {
                            s.matrix_state.selected_slot = Some((track_idx, scene_idx));
                            matrix::trigger_pad(
                                &mut s.matrix_state,
                                &s.audio_proxy.clone(),
                                track_idx,
                                scene_idx,
                            );
                        } else {
                            s.audio_proxy.send(GuiCommand::StopTrack { track_idx });
                            if let Some(slot) = s
                                .matrix_state
                                .grid
                                .get_mut(track_idx)
                                .and_then(|row| row.get_mut(scene_idx))
                            {
                                slot.state = SlotState::Stopped;
                            }
                        }
                        cx.notify();
                    });
                })
                .child(if has_clip {
                    Label::new("▶")
                        .text_size(px(9.0))
                        .text_color(rgb(0xFFFFFF))
                        .into_any_element()
                } else {
                    div().w(px(6.0)).h(px(6.0)).bg(rgb(0xFFFFFF)).into_any_element()
                }),
        )
        // Nombre del clip (recortado al pad)
        .child(
            div()
                .absolute()
                .left(px(28.0))
                .right(px(4.0))
                .top_0()
                .bottom_0()
                .flex()
                .items_center()
                .overflow_hidden()
                .child(
                    Label::new(display_text)
                        .text_size(px(9.0))
                        .text_color(rgb(0xFFFFFF)),
                ),
        );

    pad.context_menu(move |menu: PopupMenu, _window, _cx| {
        menu.item(
            PopupMenuItem::new("Copiar")
                .disabled(!has_clip)
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |s, cx| {
                        let proxy = s.audio_proxy.clone();
                        let clipboard = &mut s.matrix_clipboard;
                        s.matrix_state.copy_slot(track_idx, scene_idx, clipboard);
                        let _ = &proxy;
                        cx.notify();
                    });
                }),
        )
        .item(
            PopupMenuItem::new("Cortar")
                .disabled(!has_clip)
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |s, cx| {
                        let proxy = s.audio_proxy.clone();
                        let clipboard = &mut s.matrix_clipboard;
                        s.matrix_state
                            .cut_slot(track_idx, scene_idx, clipboard, &proxy);
                        cx.notify();
                    });
                }),
        )
        .item(
            PopupMenuItem::new("Pegar")
                .disabled(!clipboard_ready)
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |s, cx| {
                        let proxy = s.audio_proxy.clone();
                        let clipboard = &s.matrix_clipboard;
                        s.matrix_state
                            .paste_slot(track_idx, scene_idx, clipboard, &proxy);
                        cx.notify();
                    });
                }),
        )
        .item(PopupMenuItem::separator())
        .item(
            PopupMenuItem::new("Duplicar")
                .disabled(!has_clip)
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |s, cx| {
                        let proxy = s.audio_proxy.clone();
                        s.matrix_state
                            .duplicate_slot(track_idx, scene_idx, &proxy);
                        cx.notify();
                    });
                }),
        )
        .item(
            PopupMenuItem::new("Eliminar")
                .disabled(!has_clip)
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |s, cx| {
                        let proxy = s.audio_proxy.clone();
                        s.matrix_state
                            .delete_slot(track_idx, scene_idx, &proxy);
                        cx.notify();
                    });
                }),
        )
    })
    .into_any_element()
}

fn load_clip_into_slot(
    matrix_state: &mut SessionMatrixState,
    audio_proxy: &crate::audio_proxy::AudioProxy,
    track_idx: usize,
    scene_idx: usize,
    path: std::path::PathBuf,
    bpm: f64,
) {
    matrix::load_clip_into_slot(matrix_state, audio_proxy, track_idx, scene_idx, path, bpm);
}

// =========================================================================
// RENDER PRINCIPAL
// =========================================================================

fn scene_header_cell() -> AnyElement {
    div()
        .w(px(SCENE_LABEL_WIDTH))
        .h(px(HEADER_HEIGHT))
        .flex_shrink_0()
        .bg(theme::PANEL_BG)
        .border_1()
        .border_color(theme::BORDER_COLOR)
        .rounded(px(2.0))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .child(
            Label::new("SCENES")
                .text_size(px(10.0))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0x5AB4FF)),
        )
        .child(
            Label::new("Master Launch")
                .text_size(px(9.0))
                .text_color(theme::TEXT_MUTED),
        )
        .into_any_element()
}

fn track_header_cell(name: &str, idx: usize) -> AnyElement {
    div()
        .w(px(TRACK_WIDTH))
        .h(px(HEADER_HEIGHT))
        .flex_shrink_0()
        .bg(theme::PANEL_BG)
        .border_1()
        .border_color(theme::BORDER_COLOR)
        .rounded(px(2.0))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .child(
            Label::new(format!("TRK {:02}", idx + 1))
                .text_size(px(10.0))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0xE0E0E0)),
        )
        .child(
            Label::new(name.to_string())
                .text_size(px(9.0))
                .text_color(theme::TEXT_MUTED),
        )
        .into_any_element()
}

fn scene_launcher_cell(scene_idx: usize, name: &str) -> AnyElement {
    div()
        .id(SharedString::from(format!("arr_scene_{}", scene_idx)))
        .w(px(SCENE_LABEL_WIDTH))
        .h(px(PAD_HEIGHT))
        .flex_shrink_0()
        .bg(rgb(0x1E1E28))
        .border_1()
        .border_color(rgb(0x0096BE))
        .rounded(px(2.0))
        .flex()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| {
            let st = state(cx);
            st.update(cx, |s, cx| {
                let proxy = s.audio_proxy.clone();
                matrix::trigger_scene(&mut s.matrix_state, &proxy, scene_idx);
                cx.notify();
            });
        })
        .child(
            Label::new(format!("▶ {}", name))
                .text_size(px(10.0))
                .text_color(rgb(0xFFFFFF)),
        )
        .into_any_element()
}

fn h_separator() -> AnyElement {
    div()
        .w_full()
        .h(px(1.0))
        .flex_shrink_0()
        .bg(theme::BORDER_COLOR)
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn fader_column(
    id_prefix: &str,
    width: f32,
    pan: f32,
    volume: f32,
    muted: bool,
    soloed: bool,
    track_idx: Option<usize>,
) -> AnyElement {
    let (pan_target, vol_target) = match track_idx {
        None => (FaderTarget::MasterPan, FaderTarget::MasterVolume),
        Some(idx) => (FaderTarget::TrackPan(idx), FaderTarget::TrackVolume(idx)),
    };

    div()
        .w(px(width))
        .h(px(FADER_ROW_HEIGHT))
        .flex_shrink_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(4.0))
        .child(pan_slider(pan_target, pan))
        .child(volume_fader(vol_target, volume))
        .child(mute_solo_buttons(id_prefix, track_idx, muted, soloed))
        .into_any_element()
}

pub fn render(cx: &mut Context<HikaruApp>) -> AnyElement {
    let (
        track_names,
        scene_names,
        slots,
        track_mix,
        selected,
        clipboard_ready,
        drop_sample_ready,
        engine_handle,
        master,
    ) = {
        let app = state(cx).read(cx);
        let track_names: Vec<String> = app
            .matrix_state
            .tracks
            .iter()
            .map(|t| t.name.clone())
            .collect();
        let scene_names: Vec<String> = app
            .matrix_state
            .scenes
            .iter()
            .map(|s| s.name.clone())
            .collect();
        let slots: Vec<Vec<(SlotState, Option<String>)>> = app
            .matrix_state
            .grid
            .iter()
            .map(|row| {
                row.iter()
                    .map(|slot| {
                        (
                            slot.state.clone(),
                            slot.clip.as_ref().map(|c| c.name.clone()),
                        )
                    })
                    .collect()
            })
            .collect();
        let track_mix: Vec<(f32, f32, bool, bool)> = app
            .matrix_state
            .tracks
            .iter()
            .map(|t| (t.volume, t.pan, t.muted, t.soloed))
            .collect();
        let master = app
            .live_tracks
            .first()
            .map(|t: &Track| (t.volume, t.pan, t.mute, t.solo))
            .unwrap_or((0.75, 0.0, false, false));
        (
            track_names,
            scene_names,
            slots,
            track_mix,
            app.matrix_state.selected_slot,
            app.matrix_clipboard.has_content(),
            app.dragged_sample.is_some(),
            app.engine_handle.clone(),
            master,
        )
    };

    let tracks_len = track_names.len();
    let scenes_len = scene_names.len();
    let content_width =
        SCENE_LABEL_WIDTH + PAD_GAP + tracks_len as f32 * (TRACK_WIDTH + PAD_GAP);

    // -------------------------------------------------------------
    // 1. ENCABEZADOS DE PISTAS
    // -------------------------------------------------------------
    let mut header_cells: Vec<AnyElement> = vec![scene_header_cell()];
    for (idx, name) in track_names.iter().enumerate() {
        header_cells.push(track_header_cell(name, idx));
    }

    // -------------------------------------------------------------
    // 2. GRILLA DE ESCENAS
    // -------------------------------------------------------------
    let mut scene_rows: Vec<AnyElement> = Vec::new();
    for scene_idx in 0..scenes_len {
        let mut row_cells: Vec<AnyElement> =
            vec![scene_launcher_cell(scene_idx, &scene_names[scene_idx])];

        for track_idx in 0..tracks_len {
            let Some(row) = slots.get(track_idx) else {
                continue;
            };
            let Some((slot_state, clip_name)) = row.get(scene_idx) else {
                continue;
            };
            let clip_name = clip_name.clone().unwrap_or_default();
            let has_clip = !clip_name.is_empty();
            row_cells.push(render_pad(
                track_idx,
                scene_idx,
                slot_state.clone(),
                has_clip,
                clip_name,
                selected == Some((track_idx, scene_idx)),
                clipboard_ready,
                drop_sample_ready,
                engine_handle.clone(),
            ));
        }

        scene_rows.push(
            h_flex()
                .gap(px(PAD_GAP))
                .flex_shrink_0()
                .children(row_cells)
                .into_any_element(),
        );
    }

    // -------------------------------------------------------------
    // 3. MEZCLADOR / FADERS AL PIE DE CADA COLUMNA
    // -------------------------------------------------------------
    let mut fader_cells: Vec<AnyElement> = vec![fader_column(
        "arr_master",
        SCENE_LABEL_WIDTH,
        master.1,
        master.0,
        master.2,
        master.3,
        None,
    )];

    for (idx, (volume, pan, muted, soloed)) in track_mix.iter().enumerate() {
        fader_cells.push(fader_column(
            &format!("arr_track_{}", idx),
            TRACK_WIDTH,
            *pan,
            *volume,
            *muted,
            *soloed,
            Some(idx),
        ));
    }

    v_flex()
        .id("arranger_view")
        .size_full()
        .bg(theme::WINDOW_BG)
        .child(
            div()
                .flex_1()
                .w_full()
                .min_h_0()
                .overflow_x_scrollbar()
                .child(
                    v_flex()
                        .w(px(content_width))
                        .h_full()
                        .gap(px(4.0))
                        .child(h_flex().gap(px(PAD_GAP)).flex_shrink_0().children(header_cells))
                        .child(h_separator())
                        .child(
                            v_flex()
                                .flex_1()
                                .min_h_0()
                                .gap(px(PAD_GAP))
                                .overflow_y_scrollbar()
                                .children(scene_rows),
                        )
                        .child(h_separator())
                        .child(
                            h_flex()
                                .gap(px(PAD_GAP))
                                .flex_shrink_0()
                                .items_stretch()
                                .children(fader_cells),
                        ),
                ),
        )
        .into_any_element()
}
