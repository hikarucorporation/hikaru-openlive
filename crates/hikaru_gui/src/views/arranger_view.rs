// Copyright (C) Hikaru Corporation - 2026
// Hikaru OpenLive / OpenStudio - Arranger View (columnas unificadas)
// GNU Affero General Public License v3
// crates/hikaru_gui/src/views/arranger_view.rs
//
// El Arranger View reemplaza al dock inferior del Mixer clásico: cada
// canal/pista es UNA columna vertical unificada que contiene arriba su
// contenido y abajo el mismo Channel Strip en ambos modos:
//
// - Modo OpenLive   (docs/arranger_view_ligero_rediseño_modo-openlive.md):
//   arriba la matriz de Clips / Launchers por escena, abajo el strip.
// - Modo OpenStudio (docs/arranger_view_ligero_rediseño_modo-openstudio.md):
//   arriba el contenedor vertical unificado de Waveform, abajo el MISMO strip.
//
// Channel Strip (idéntico en ambos modos, de arriba a abajo):
//   DEVICES (rack) -> ROUTING/SENDS -> PAN/BALANCE -> [S][M][R] -> FADER & VU
//   (fader vertical de capuchón ancho + VU vertical a su izquierda).
//
// Rendimiento: los VU son `canvas` de tamaño fijo (`mixer::vertical_vu_meter`)
// que sólo repintan quads; el nivel nunca toca el layout. El suavizado
// ataque/liberación se calcula una vez por frame en `AppState::sync_frame`.

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
// Registro para el harness de tests headless (`tests/arranger_columns_fit`):
// sin la feature `test-support` es identidad y no cambia nada en producción.
use gpui_kit::TestSupportExt as _;
use gpui_kit::*;

use crate::app::{state, AppMode, HikaruApp};
use crate::audio_proxy::GuiCommand;
use crate::theme;
use crate::views::matrix::{self, SessionMatrixState, SlotState};
use crate::views::mixer::{self, Track};
use crate::views::playlist::{self, ClipType};

// =========================================================================
// GEOMETRÍA
// =========================================================================

const SCENE_LABEL_WIDTH: f32 = 124.0;
const TRACK_WIDTH: f32 = 140.0;
/// Alto de pads y launchers: 22px para que 8 escenas + strip quepan en 720p.
const PAD_HEIGHT: f32 = 22.0;
const PAD_GAP: f32 = 2.0;
const HEADER_HEIGHT: f32 = 32.0;

/// Alto del contenedor vertical de waveform (modo OpenStudio).
const WAVEFORM_HEIGHT: f32 = 192.0;
/// Alto fijo de la lista de devices: mantiene PAN / STATE / FADER alineados
/// entre columnas aunque las pistas tengan distinto número de plugins.
const DEVICES_HEIGHT: f32 = 28.0;

const PAN_WIDTH: f32 = 90.0;
const PAN_HEIGHT: f32 = 20.0;
const PAN_THUMB_W: f32 = 12.0;

const FADER_WIDTH: f32 = 30.0;
/// Mantener igual a `mixer::VU_HEIGHT` para lectura paralela VU <-> fader.
/// 68px para margen en viewports reales (ver `VU_HEIGHT`).
const FADER_HEIGHT: f32 = 68.0;
const FADER_THUMB_H: f32 = 12.0;
const FADER_RAIL_W: f32 = 24.0;
/// Volumen unity del reset con doble-clic: 0.75 lineal = 0.0 dB.
const VOLUME_RESET: f32 = 0.75;

// =========================================================================
// DESTINO DE PISTA DEL CHANNEL STRIP
// =========================================================================

/// A qué pista del estado apunta una columna del strip.
///
/// `Live` guarda el índice de matriz (los faders/pads hablan el idioma de la
/// matriz y el índice +1 es la posición en `live_tracks`). `Studio` guarda el
/// índice directo en `studio_tracks`. `Master` resuelve al track 0 del modo
/// activo vía `tracks_mut()`, así vale para ambos modos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StripTarget {
    Master,
    Live(usize),
    Studio(usize),
}

// =========================================================================
// ESTADO DE ARRASTRE DE LOS FADERS
// =========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaderTarget {
    MasterPan,
    MasterVolume,
    TrackPan(usize),
    TrackVolume(usize),
    StudioPan(usize),
    StudioVolume(usize),
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
                if let Some(t) = s.tracks_mut().first_mut() {
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
            FaderTarget::StudioPan(idx) => {
                let Some(t) = s.studio_tracks.get_mut(idx) else {
                    return;
                };
                t.pan = pan;
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
                if let Some(t) = s.tracks_mut().first_mut() {
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
            FaderTarget::StudioVolume(idx) => {
                let Some(t) = s.studio_tracks.get_mut(idx) else {
                    return;
                };
                t.volume = volume;
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
fn record_bounds(slot: &Rc<Cell<[f32; 4]>>, bounds: Bounds<Pixels>) {
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
// SNAPSHOT DEL STRIP (una lectura de estado por frame)
// =========================================================================

#[derive(Clone)]
struct StripSnapshot {
    name: String,
    header: String,
    volume: f32,
    pan: f32,
    muted: bool,
    soloed: bool,
    armed: bool,
    is_master: bool,
    devices: Vec<(usize, String, bool)>,
    route_dest: String,
    sends: Vec<(String, f32)>,
    vu: f32,
    selected_slot: usize,
    is_selected_track: bool,
    target: StripTarget,
}

fn snapshot_master(
    master: Option<&Track>,
    vu: f32,
    selected_track: usize,
    selected_slot: usize,
) -> StripSnapshot {
    let (name, volume, pan, muted, soloed, armed, devices, route_dest, sends) = match master {
        Some(t) => (
            t.name.clone(),
            t.volume,
            t.pan,
            t.mute,
            t.solo,
            t.arm,
            t.effects
                .iter()
                .map(|s| (s.id, s.name.clone(), s.active))
                .collect(),
            format!("Bus {}", t.route_destination_id),
            t.sends
                .iter()
                .map(|s| (format!("Bus {}", s.target_id), s.amount))
                .collect(),
        ),
        None => (
            "MASTER".to_string(),
            0.75,
            0.0,
            false,
            false,
            false,
            Vec::new(),
            "Bus 0".to_string(),
            Vec::new(),
        ),
    };
    StripSnapshot {
        header: "MASTER".to_string(),
        name,
        volume,
        pan,
        muted,
        soloed,
        armed,
        is_master: true,
        devices,
        route_dest,
        sends,
        vu,
        selected_slot,
        is_selected_track: selected_track == 0,
        target: StripTarget::Master,
    }
}

// =========================================================================
// HELPERS DE ESTADO (mute / solo / arm por destino)
// =========================================================================

fn strip_mute_state(cx: &mut App, target: StripTarget) -> bool {
    let app = state(cx).read(cx);
    match target {
        StripTarget::Master => app.tracks().first().map(|t| t.mute).unwrap_or(false),
        StripTarget::Live(idx) => app
            .matrix_state
            .tracks
            .get(idx)
            .map(|t| t.muted)
            .unwrap_or(false),
        StripTarget::Studio(idx) => app
            .studio_tracks
            .get(idx)
            .map(|t| t.mute)
            .unwrap_or(false),
    }
}

fn strip_solo_state(cx: &mut App, target: StripTarget) -> bool {
    let app = state(cx).read(cx);
    match target {
        StripTarget::Master => app.tracks().first().map(|t| t.solo).unwrap_or(false),
        StripTarget::Live(idx) => app
            .matrix_state
            .tracks
            .get(idx)
            .map(|t| t.soloed)
            .unwrap_or(false),
        StripTarget::Studio(idx) => app
            .studio_tracks
            .get(idx)
            .map(|t| t.solo)
            .unwrap_or(false),
    }
}

fn strip_arm_state(cx: &mut App, target: StripTarget) -> bool {
    let app = state(cx).read(cx);
    match target {
        StripTarget::Master => false,
        StripTarget::Live(idx) => app.live_tracks.get(idx + 1).map(|t| t.arm).unwrap_or(false),
        StripTarget::Studio(idx) => app.studio_tracks.get(idx).map(|t| t.arm).unwrap_or(false),
    }
}

fn toggle_mute_target(cx: &mut App, target: StripTarget) {
    let muted = strip_mute_state(cx, target);
    let st = state(cx);
    st.update(cx, |s, cx| {
        match target {
            StripTarget::Master => {
                if let Some(t) = s.tracks_mut().first_mut() {
                    t.mute = !muted;
                }
            }
            StripTarget::Live(idx) => {
                if let Some(t) = s.matrix_state.tracks.get_mut(idx) {
                    t.muted = !muted;
                }
                if let Some(live) = s.live_tracks.get_mut(idx + 1) {
                    live.mute = !muted;
                }
                s.audio_proxy.send(GuiCommand::SetTrackMute {
                    track_idx: idx,
                    mute: !muted,
                });
            }
            StripTarget::Studio(idx) => {
                if let Some(t) = s.studio_tracks.get_mut(idx) {
                    t.mute = !t.mute;
                    let mute = t.mute;
                    s.audio_proxy.send(GuiCommand::SetTrackMute {
                        track_idx: idx,
                        mute,
                    });
                }
            }
        }
        cx.notify();
    });
}

fn toggle_solo_target(cx: &mut App, target: StripTarget) {
    let soloed = strip_solo_state(cx, target);
    let st = state(cx);
    st.update(cx, |s, cx| {
        match target {
            StripTarget::Master => {
                if let Some(t) = s.tracks_mut().first_mut() {
                    t.solo = !soloed;
                }
            }
            StripTarget::Live(idx) => {
                if let Some(t) = s.matrix_state.tracks.get_mut(idx) {
                    t.soloed = !soloed;
                }
                if let Some(live) = s.live_tracks.get_mut(idx + 1) {
                    live.solo = !soloed;
                }
                s.audio_proxy.send(GuiCommand::SetTrackSolo {
                    track_idx: idx,
                    solo: !soloed,
                });
            }
            StripTarget::Studio(idx) => {
                if let Some(t) = s.studio_tracks.get_mut(idx) {
                    t.solo = !t.solo;
                    let solo = t.solo;
                    s.audio_proxy.send(GuiCommand::SetTrackSolo {
                        track_idx: idx,
                        solo,
                    });
                }
            }
        }
        cx.notify();
    });
}

fn toggle_arm_target(cx: &mut App, target: StripTarget) {
    // El armado es estado puro de GUI (no hay GuiCommand de arm en el motor):
    // decide qué pista recibe la próxima grabación.
    let armed = strip_arm_state(cx, target);
    let st = state(cx);
    st.update(cx, |s, cx| {
        match target {
            StripTarget::Master => {}
            StripTarget::Live(idx) => {
                if let Some(live) = s.live_tracks.get_mut(idx + 1) {
                    live.arm = !armed;
                }
            }
            StripTarget::Studio(idx) => {
                if let Some(t) = s.studio_tracks.get_mut(idx) {
                    t.arm = !armed;
                }
            }
        }
        cx.notify();
    });
}

fn select_device_slot(cx: &mut App, target: StripTarget, slot: usize) {
    let st = state(cx);
    st.update(cx, |s, cx| {
        let vec_idx = match target {
            StripTarget::Master => 0,
            StripTarget::Live(idx) => idx + 1,
            StripTarget::Studio(idx) => idx,
        };
        s.selected_track_index = vec_idx;
        s.selected_slot_index = slot;
        cx.notify();
    });
}

fn toggle_device_active(cx: &mut App, target: StripTarget, slot: usize) {
    let st = state(cx);
    st.update(cx, |s, cx| {
        let fx = match target {
            StripTarget::Master => s.tracks_mut().first_mut().map(|t| &mut t.effects),
            StripTarget::Live(idx) => s.live_tracks.get_mut(idx + 1).map(|t| &mut t.effects),
            StripTarget::Studio(idx) => s.studio_tracks.get_mut(idx).map(|t| &mut t.effects),
        };
        if let Some(fx) = fx {
            if let Some(dev) = fx.get_mut(slot) {
                dev.active = !dev.active;
            }
        }
        cx.notify();
    });
}

fn cycle_route_destination(cx: &mut App, target: StripTarget) {
    let st = state(cx);
    st.update(cx, |s, cx| {
        let (len, slot) = match target {
            StripTarget::Master => {
                let len = s.tracks().len().max(1);
                (len, s.tracks_mut().first_mut().map(|t| &mut t.route_destination_id))
            }
            StripTarget::Live(idx) => {
                let len = s.live_tracks.len().max(1);
                (
                    len,
                    s.live_tracks
                        .get_mut(idx + 1)
                        .map(|t| &mut t.route_destination_id),
                )
            }
            StripTarget::Studio(idx) => {
                let len = s.studio_tracks.len().max(1);
                (
                    len,
                    s.studio_tracks
                        .get_mut(idx)
                        .map(|t| &mut t.route_destination_id),
                )
            }
        };
        if let Some(slot) = slot {
            *slot = (*slot + 1) % len;
        }
        cx.notify();
    });
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
            if event.click_count >= 2 {
                // Doble-clic: pan de vuelta al centro (C).
                stop_fader_drag(cx, target);
                set_pan(cx, target, 0.0);
                return;
            }
            let b = b_down.get();
            start_fader_drag(cx, target);
            set_pan(cx, target, pan_from_pointer(b, event.position.x.as_f32()));
        })
        .on_mouse_move(move |event, _, cx| {
            // Si el botón se soltó fuera del slider, el `mouse_up` nunca
            // llega y el drag quedaría colgado siguiendo al cursor: ante un
            // move sin botón presionado se cierra el gesto.
            if event.pressed_button != Some(gpui_kit::MouseButton::Left) {
                stop_fader_drag(cx, target);
                return;
            }
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

/// Fader vertical de capuchón ANCHO (2.5x el alto del thumb, según blueprint).
fn volume_fader(target: FaderTarget, volume: f32) -> AnyElement {
    let slot: Rc<Cell<[f32; 4]>> = Rc::new(Cell::new([0.0; 4]));
    let b_down = slot.clone();
    let b_move = slot.clone();
    let b_paint = slot.clone();

    let half_thumb = FADER_THUMB_H / 2.0;
    let travel = FADER_HEIGHT - FADER_THUMB_H;
    let thumb_y = half_thumb + (1.0 - volume.clamp(0.0, 1.0)) * travel;
    let rail_left = (FADER_WIDTH - FADER_RAIL_W) / 2.0;
    // Capuchón ancho del blueprint: ocupa casi todo el ancho de la columna.
    let cap_w = FADER_THUMB_H * 2.5;

    div()
        .id(SharedString::from(format!("arr_vol_{:?}", target)))
        .relative()
        .w(px(FADER_WIDTH))
        .h(px(FADER_HEIGHT))
        .flex_shrink_0()
        .on_mouse_down(gpui_kit::MouseButton::Left, move |event, _, cx| {
            if event.click_count >= 2 {
                // Doble-clic: volumen de vuelta a unity (0.0 dB).
                stop_fader_drag(cx, target);
                set_volume(cx, target, VOLUME_RESET);
                return;
            }
            let b = b_down.get();
            start_fader_drag(cx, target);
            set_volume(cx, target, volume_from_pointer(b, event.position.y.as_f32()));
        })
        .on_mouse_move(move |event, _, cx| {
            // Igual que en el pan: move sin botón = el `mouse_up` se perdió
            // fuera del fader, se cierra el gesto colgado.
            if event.pressed_button != Some(gpui_kit::MouseButton::Left) {
                stop_fader_drag(cx, target);
                return;
            }
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
        // Capuchón ancho
        .child(
            div()
                .absolute()
                .left(px((FADER_WIDTH - cap_w) / 2.0))
                .top(px(thumb_y - half_thumb))
                .w(px(cap_w))
                .h(px(FADER_THUMB_H))
                .bg(rgb(0x00A2E8))
                .border_1()
                .border_color(rgb(0x000000)),
        )
        .into_any_element()
}

/// Botones [S] Solo, [M] Mute, [R] Record-arm del blueprint.
fn smr_buttons(id_prefix: &str, target: StripTarget, soloed: bool, muted: bool, armed: bool) -> AnyElement {
    // Mini-toggles 22×18 como los M/S de los headers (el `Button` del kit no
    // baja de ~24px y empujaba el pie del canal fuera de pantalla).
    h_flex()
        .gap(px(4.0))
        .flex_shrink_0()
        .justify_center()
        .child(matrix::ms_button(
            format!("{}_solo", id_prefix),
            "S",
            soloed,
            rgb(0xE6C84C),
            move |cx| toggle_solo_target(cx, target),
        ))
        .child(matrix::ms_button(
            format!("{}_mute", id_prefix),
            "M",
            muted,
            rgb(0xE06060),
            move |cx| toggle_mute_target(cx, target),
        ))
        .child(matrix::ms_button(
            format!("{}_rec", id_prefix),
            "R",
            armed,
            rgb(0xFF3C3C),
            move |cx| toggle_arm_target(cx, target),
        ))
        .into_any_element()
}

// =========================================================================
// SECCIONES DEL CHANNEL STRIP (idénticas en OpenLive y OpenStudio)
// =========================================================================

fn section_label(text: &str) -> AnyElement {
    Label::new(text.to_string())
        .text_size(px(8.0))
        .font_weight(FontWeight::BOLD)
        .text_color(theme::TEXT_MUTED)
        .into_any_element()
}

fn strip_separator() -> AnyElement {
    div()
        .w_full()
        .h(px(1.0))
        .flex_shrink_0()
        .bg(theme::BORDER_COLOR)
        .into_any_element()
}

/// DEVICES: rack compacto de la pista. Click = seleccionar slot (lo muestra el
/// DSP rack); el punto conmuta el bypass del slot.
fn devices_section(snap: &StripSnapshot, id_prefix: &str) -> AnyElement {
    let rows: Vec<AnyElement> = if snap.devices.is_empty() {
        vec![
            Label::new("— vacío —")
                .text_size(px(9.0))
                .text_color(theme::TEXT_DISABLED)
                .into_any_element(),
        ]
    } else {
        snap.devices
            .iter()
            .map(|(slot, name, active)| {
                let target = snap.target;
                let slot = *slot;
                let active = *active;
                let name = name.clone();
                let is_sel = snap.is_selected_track && snap.selected_slot == slot;
                h_flex()
                    .w_full()
                    .h(px(14.0))
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(4.0))
                    .px(px(4.0))
                    .rounded(px(2.0))
                    .bg(if is_sel { rgb(0x2D3A4A) } else { rgb(0x1A1A1E) })
                    .border_1()
                    .border_color(if is_sel { rgb(0x5AB4FF) } else { rgb(0x282828) })
                    .on_mouse_down(
                        gpui_kit::MouseButton::Left,
                        move |_, _, cx| select_device_slot(cx, target, slot),
                    )
                    .child(
                        div()
                            .w(px(8.0))
                            .h(px(8.0))
                            .flex_shrink_0()
                            .rounded(px(4.0))
                            .bg(if active { rgb(0x00FF64) } else { rgb(0x505055) })
                            .id(SharedString::from(format!("{}_devpw_{}", id_prefix, slot)))
                            .on_mouse_down(
                                gpui_kit::MouseButton::Left,
                                move |_, _, cx| toggle_device_active(cx, target, slot),
                            ),
                    )
                    .child(
                        Label::new(if name.is_empty() {
                            "Empty Slot".to_string()
                        } else {
                            name
                        })
                        .text_size(px(8.0))
                        .text_color(if active {
                            rgb(0xE0E0E0)
                        } else {
                            rgb(0x808080)
                        }),
                    )
                    .into_any_element()
            })
            .collect()
    };

    v_flex()
        .w_full()
        .flex_shrink_0()
        .gap(px(1.0))
        .child(section_label("DEVICES:"))
        .child(
            div()
                .w_full()
                .h(px(DEVICES_HEIGHT))
                .overflow_y_scrollbar()
                .child(v_flex().w_full().gap(px(1.0)).children(rows)),
        )
        .into_any_element()
}

/// ROUTING / SENDS: destino R/S + lista de envíos con cantidad.
fn routing_section(snap: &StripSnapshot, id_prefix: &str) -> AnyElement {
    let target = snap.target;
    let dest = snap.route_dest.clone();
    let mut rows: Vec<AnyElement> = vec![
        h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .child(
                Label::new(format!("R/S: {}", dest))
                    .text_size(px(9.0))
                    .text_color(rgb(0xE0E0E0)),
            )
            .child(
                Button::new(SharedString::from(format!("{}_route", id_prefix)))
                    .label(">")
                    .compact()
                    .rounded(ButtonRounded::None)
                    .on_click(move |_, _, cx| cycle_route_destination(cx, target)),
            )
            .into_any_element(),
    ];
    for (name, amount) in &snap.sends {
        rows.push(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .child(
                    Label::new(format!("Snd>{}", name))
                        .text_size(px(9.0))
                        .text_color(theme::TEXT_MUTED),
                )
                .child(
                    Label::new(format!("{:.0}%", amount.clamp(0.0, 1.0) * 100.0))
                        .text_size(px(9.0))
                        .text_color(theme::TEXT_MUTED),
                )
                .into_any_element(),
        );
    }

    v_flex()
        .w_full()
        .flex_shrink_0()
        .gap(px(1.0))
        .child(section_label("ROUTING / SENDS:"))
        .children(rows)
        .into_any_element()
}

/// FADER & VU: VU vertical a la izquierda (canvas fijo, sin relayout) + fader
/// de capuchón ancho a la derecha, cada uno con su lectura en dB debajo.
fn fader_vu_section(snap: &StripSnapshot, id_prefix: &str) -> AnyElement {
    let vol_target = match snap.target {
        StripTarget::Master => FaderTarget::MasterVolume,
        StripTarget::Live(idx) => FaderTarget::TrackVolume(idx),
        StripTarget::Studio(idx) => FaderTarget::StudioVolume(idx),
    };
    let vu = snap.vu;
    let vu_db = mixer::level_db_text(vu);
    let vol_db = mixer::db_text(snap.volume);

    v_flex()
        .w_full()
        .flex_shrink_0()
        .items_center()
        .gap(px(1.0))
        .child(section_label("FADER & VU METER"))
        .child(
            h_flex()
                .w_full()
                .justify_center()
                .items_start()
                .gap(px(6.0))
                .child(
                    v_flex()
                        .items_center()
                        .gap(px(1.0))
                        .child(
                            Label::new("VU")
                                .text_size(px(8.0))
                                .text_color(theme::TEXT_MUTED),
                        )
                        .child(mixer::vertical_vu_meter(vu))
                        .child(
                            Label::new(vu_db)
                                .text_size(px(8.0))
                                .text_color(theme::TEXT_MUTED),
                        ),
                )
                .child(
                    v_flex()
                        .items_center()
                        .gap(px(1.0))
                        .child(
                            Label::new("Vol")
                                .text_size(px(8.0))
                                .text_color(theme::TEXT_MUTED),
                        )
                        .child(volume_fader(vol_target, snap.volume))
                        .child(
                            Label::new(vol_db)
                                .text_size(px(8.0))
                                .text_color(theme::TEXT_MUTED),
                        ),
                ),
        )
        .child(smr_buttons(id_prefix, snap.target, snap.soloed, snap.muted, snap.armed))
        .into_any_element()
}

/// Channel Strip completo: el MISMO en OpenLive y OpenStudio.
fn channel_strip(snap: &StripSnapshot, id_prefix: &str) -> AnyElement {
    let (pan_target, _) = match snap.target {
        StripTarget::Master => (FaderTarget::MasterPan, FaderTarget::MasterVolume),
        StripTarget::Live(idx) => (FaderTarget::TrackPan(idx), FaderTarget::TrackVolume(idx)),
        StripTarget::Studio(idx) => (FaderTarget::StudioPan(idx), FaderTarget::StudioVolume(idx)),
    };
    v_flex()
        .w_full()
        .flex_shrink_0()
        .gap(px(1.0))
        .child(strip_separator())
        .child(devices_section(snap, id_prefix))
        .child(strip_separator())
        .child(routing_section(snap, id_prefix))
        .child(strip_separator())
        .child(
            v_flex()
                .w_full()
                .flex_shrink_0()
                .items_center()
                .gap(px(1.0))
                .child(section_label("PAN / BALANCE"))
                .child(pan_slider(pan_target, snap.pan)),
        )
        .child(strip_separator())
        .child(fader_vu_section(snap, id_prefix))
        .into_any_element()
}

// =========================================================================
// PADS DE LA GRILLA (Modo OpenLive)
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
        .w_full()
        .h(px(PAD_HEIGHT))
        .flex_shrink_0()
        .bg(bg)
        .border_1()
        .border_color(border)
        .when(is_selected, |d| d.border_2().border_color(rgb(0xE0E0E0)))
        .when(drop_sample_ready, |d| d.border_color(rgb(0x0096BE)))
        .rounded(px(2.0))
        .overflow_hidden()
        // Drop target del Drag & Drop desde el Explorer (línea de tiempo).
        .can_drop(|payload: &dyn std::any::Any, _, _| {
            payload.is::<crate::views::explorer::ExplorerAudioDrag>()
        })
        .on_drop(
            move |payload: &crate::views::explorer::ExplorerAudioDrag, _, cx| {
                let path = payload.0.clone();
                let st = state(cx);
                st.update(cx, |s, cx| {
                    s.dragged_sample = None;
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
                    cx.notify();
                });
            },
        )
        .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| {
            let st = state(cx);
            st.update(cx, |s, cx| {
                if let Some(path) = s.dragged_sample.take() {
                    if crate::views::explorer::is_audio_file(&path)
                        || crate::views::explorer::is_midi_file(&path)
                    {
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
                .left(px(4.0))
                .top(px(4.0))
                .w(px(14.0))
                .h(px(14.0))
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
                        .text_size(px(8.0))
                        .text_color(rgb(0xFFFFFF))
                        .into_any_element()
                } else {
                    div().w(px(5.0)).h(px(5.0)).bg(rgb(0xFFFFFF)).into_any_element()
                }),
        )
        // Nombre del clip (recortado al pad)
        .child(
            div()
                .absolute()
                .left(px(22.0))
                .right(px(4.0))
                .top_0()
                .bottom_0()
                .flex()
                .items_center()
                .overflow_hidden()
                .child(
                    Label::new(display_text)
                        .text_size(px(8.0))
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

fn slot_display_text(slot_state: &SlotState, has_clip: bool, clip_name: &str) -> String {
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
// CONTENEDOR VERTICAL DE WAVEFORM (Modo OpenStudio)
// =========================================================================

/// Caja vertical unificada de la pista: pinta los picos del primer clip de
/// audio en orientación vertical (el tiempo corre de arriba a abajo). Sin clip
/// muestra el placeholder del blueprint. Tamaño fijo: no hay relayout.
fn waveform_container(track_name: &str, peaks: Option<Vec<f32>>) -> AnyElement {
    let has_wave = peaks.as_ref().map(|p| !p.is_empty()).unwrap_or(false);
    let label = if has_wave {
        track_name.to_string()
    } else {
        "INSERTAR WAVEFORM VERTICAL".to_string()
    };
    div()
        .w_full()
        .h(px(WAVEFORM_HEIGHT))
        .flex_shrink_0()
        .bg(rgb(0x101014))
        .border_1()
        .border_color(rgb(0x2D2D37))
        .rounded(px(2.0))
        .relative()
        .overflow_hidden()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let Some(ref peaks) = peaks else { return };
                    if peaks.is_empty() {
                        return;
                    }
                    let n = peaks.len();
                    let w = bounds.size.width.as_f32();
                    let h = bounds.size.height.as_f32();
                    if w <= 0.0 || h <= 0.0 {
                        return;
                    }
                    let cx0 = bounds.origin.x + px(w / 2.0);
                    let step = 2.0_f32;
                    let steps = (h / step) as usize;
                    for i in 0..steps {
                        let norm = i as f32 / steps as f32;
                        let peak_idx = (norm * n as f32) as usize;
                        let Some(&pv) = peaks.get(peak_idx) else {
                            continue;
                        };
                        let bw = (w * 0.86 * pv.clamp(0.0, 1.0)).max(1.0);
                        if bw < 0.75 {
                            continue;
                        }
                        let y = bounds.origin.y + px(i as f32 * step);
                        let mut path = PathBuilder::fill();
                        path.move_to(point(cx0 - px(bw * 0.5), y));
                        path.line_to(point(cx0 + px(bw * 0.5), y));
                        path.line_to(point(cx0 + px(bw * 0.5), y + px(1.0)));
                        path.line_to(point(cx0 - px(bw * 0.5), y + px(1.0)));
                        path.close();
                        if let Ok(path) = path.build() {
                            window.paint_path(path, rgba(0x5AB4FFCC));
                        }
                    }
                    // Línea central de referencia.
                    let mut axis = PathBuilder::stroke(px(1.0));
                    axis.move_to(point(cx0, bounds.origin.y));
                    axis.line_to(point(cx0, bounds.origin.y + bounds.size.height));
                    if let Ok(axis) = axis.build() {
                        window.paint_path(axis, rgb(0x32323C));
                    }
                },
            )
            .absolute()
            .inset_0(),
        )
        .child(
            div()
                .absolute()
                .left(px(4.0))
                .top(px(4.0))
                .right(px(4.0))
                .child(
                    Label::new(label)
                        .text_size(px(9.0))
                        .text_color(if has_wave {
                            rgb(0x5AB4FF)
                        } else {
                            rgb(0x808080)
                        }),
                ),
        )
        .into_any_element()
}

// =========================================================================
// COLUMNAS
// =========================================================================

fn track_header_cell(width: f32, header: &str, name: &str, accent: Rgba) -> AnyElement {
    div()
        .w(px(width))
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
            Label::new(header.to_string())
                .text_size(px(10.0))
                .font_weight(FontWeight::BOLD)
                .text_color(accent),
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
        .w_full()
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
                .text_size(px(9.0))
                .text_color(rgb(0xFFFFFF)),
        )
        .into_any_element()
}

/// Columna unificada: cabecera + contenido superior + channel strip.
fn track_column(width: f32, header: AnyElement, top: AnyElement, strip: AnyElement) -> AnyElement {
    v_flex()
        .w(px(width))
        .flex_shrink_0()
        .bg(theme::PANEL_BG)
        .border_1()
        .border_color(theme::BORDER_COLOR)
        .rounded(px(4.0))
        .p(px(3.0))
        .gap(px(2.0))
        .child(header)
        .child(top)
        .child(strip)
        .into_any_element()
}

// =========================================================================
// RENDER PRINCIPAL
// =========================================================================

pub fn render(cx: &mut Context<HikaruApp>) -> AnyElement {
    let (
        mode,
        track_names,
        scene_names,
        slots,
        track_mix,
        selected,
        clipboard_ready,
        drop_sample_ready,
        engine_handle,
        master_live,
        _master_studio,
        live_devices,
        studio_strips,
        studio_peaks,
        vu_live,
        vu_master_live,
        selected_track,
        selected_slot,
        track_count,
        show_studio_mixer,
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
        let master_live = app.live_tracks.first().cloned();
        let master_studio = app.studio_tracks.first().cloned();
        // Devices / routing / arm viven en `live_tracks[idx+1]` (matriz) o en
        // `studio_tracks[idx]` (estudio): se congelan acá para no leer el
        // estado desde los closures del layout.
        let live_devices: Vec<(bool, Vec<(usize, String, bool)>, String, Vec<(String, f32)>)> =
            app.matrix_state
                .tracks
                .iter()
                .enumerate()
                .map(|(idx, _)| {
                    app.live_tracks.get(idx + 1).map(|t| {
                        (
                            t.arm,
                            t.effects
                                .iter()
                                .map(|s| (s.id, s.name.clone(), s.active))
                                .collect(),
                            format!("Bus {}", t.route_destination_id),
                            t.sends
                                .iter()
                                .map(|s| (format!("Bus {}", s.target_id), s.amount))
                                .collect(),
                        )
                    }).unwrap_or((false, Vec::new(), "Bus 0".to_string(), Vec::new()))
                })
                .collect();
        let studio_strips: Vec<StripSnapshot> = app
            .studio_tracks
            .iter()
            .enumerate()
            .map(|(idx, t)| {
                let vu = if idx == 0 {
                    app.smoothed_master_peak
                } else {
                    app.smoothed_track_peaks
                        .get((idx - 1) % 16)
                        .copied()
                        .unwrap_or(0.0)
                };
                StripSnapshot {
                    name: t.name.clone(),
                    header: if t.is_master {
                        "MASTER".to_string()
                    } else {
                        format!("TRK {:02}", t.id)
                    },
                    volume: t.volume,
                    pan: t.pan,
                    muted: t.mute,
                    soloed: t.solo,
                    armed: t.arm,
                    is_master: t.is_master,
                    devices: t
                        .effects
                        .iter()
                        .map(|s| (s.id, s.name.clone(), s.active))
                        .collect(),
                    route_dest: format!("Bus {}", t.route_destination_id),
                    sends: t
                        .sends
                        .iter()
                        .map(|s| (format!("Bus {}", s.target_id), s.amount))
                        .collect(),
                    vu,
                    selected_slot: app.selected_slot_index,
                    is_selected_track: app.selected_track_index == idx,
                    target: if idx == 0 {
                        StripTarget::Master
                    } else {
                        StripTarget::Studio(idx)
                    },
                }
            })
            .collect();
        // Picos del primer clip de audio por pista de estudio (para la caja de
        // waveform). Se clonan una vez por frame; son ~512 f32 por pista.
        let studio_peaks: Vec<Option<Vec<f32>>> = {
            (0..app.studio_tracks.len())
                .map(|idx| {
                    app.playlist_state
                        .clips
                        .iter()
                        .find(|(tid, c)| {
                            *tid == idx
                                && matches!(
                                    c.clip_type,
                                    ClipType::Audio { .. }
                                )
                        })
                        .and_then(|(_, c)| match &c.clip_type {
                            ClipType::Audio { peaks, .. } => Some(peaks.clone()),
                            _ => None,
                        })
                })
                .collect()
        };
        let vu_live: Vec<f32> = (0..track_names.len())
            .map(|i| app.smoothed_track_peaks.get(i).copied().unwrap_or(0.0))
            .collect();
        (
            app.mode,
            track_names,
            scene_names,
            slots,
            track_mix,
            app.matrix_state.selected_slot,
            app.matrix_clipboard.has_content(),
            app.dragged_sample.is_some(),
            app.engine_handle.clone(),
            master_live,
            master_studio,
            live_devices,
            studio_strips,
            studio_peaks,
            vu_live,
            app.smoothed_master_peak,
            app.selected_track_index,
            app.selected_slot_index,
            app.matrix_state.tracks.len() + app.studio_tracks.len(),
            app.show_studio_mixer,
        )
    };

    // -------------------------------------------------------------
    // Barra superior: modo + pistas (reemplaza la cabecera del dock mixer)
    // -------------------------------------------------------------
    let is_live = mode == AppMode::OpenLive;
    let toolbar = h_flex()
        .w_full()
        .flex_shrink_0()
        .items_center()
        .gap(px(6.0))
        .px(px(4.0))
        .py(px(2.0))
        .child(
            Button::new("arr_openlive")
                .rounded(ButtonRounded::None)
                .label("OPENLIVE")
                .compact()
                .when(is_live, |b| b.text_color(rgb(0x00B4D8)))
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, _| {
                        state.mode = AppMode::OpenLive;
                    });
                }),
        )
        .child(
            Button::new("arr_openstudio")
                .rounded(ButtonRounded::None)
                .label("OPENSTUDIO")
                .compact()
                .when(!is_live, |b| b.text_color(rgb(0xFF6E00)))
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, _| {
                        state.mode = AppMode::OpenStudio;
                    });
                }),
        )
        .child(
            Button::new("arr_add_track")
                .rounded(ButtonRounded::None)
                .label(" [ + ] ")
                .compact()
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, cx| {
                        if state.mode == AppMode::OpenLive {
                            state.matrix_state.add_track();
                            let n = state.matrix_state.tracks.len();
                            let name = state
                                .matrix_state
                                .tracks
                                .last()
                                .map(|t| t.name.clone())
                                .unwrap_or(format!("Track {}", n));
                            let mut t = Track::new(n, name, false);
                            if let Some(mx) = state.matrix_state.tracks.last() {
                                t.volume = mx.volume;
                                t.pan = mx.pan;
                            }
                            t.matrix_idx = Some(n.saturating_sub(1));
                            state.live_tracks.push(t);
                        } else {
                            let next_id = state
                                .studio_tracks
                                .iter()
                                .map(|t| t.id)
                                .max()
                                .unwrap_or(0)
                                + 1;
                            let n = state.studio_tracks.iter().filter(|t| !t.is_master).count();
                            state.studio_tracks.push(Track::new(
                                next_id,
                                format!("TRACK {:02}", n + 1),
                                false,
                            ));
                        }
                        cx.notify();
                    });
                }),
        )
        .child(
            Button::new("arr_remove_track")
                .rounded(ButtonRounded::None)
                .label(" [ - ] ")
                .compact()
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, cx| {
                        if state.mode == AppMode::OpenLive {
                            state.matrix_state.remove_track();
                            if state.live_tracks.len() > 2 {
                                state.live_tracks.pop();
                            }
                        } else if let Some(pos) =
                            state.studio_tracks.iter().rposition(|t| !t.is_master)
                        {
                            state.studio_tracks.remove(pos);
                        }
                        cx.notify();
                    });
                }),
        )
        .child(Label::new(format!("TOTAL: {}", track_count)).text_xs())
        // OpenStudio: conmuta la consola de mezcla. Las vistas son
        // independientes — ocultar el mixer deja la Playlist / Timeline a
        // todo el ancho sin que nada de un módulo sangren adentro del otro.
        .when(!is_live, |t| {
            t.child(
                Button::new("arr_toggle_mixer")
                    .rounded(ButtonRounded::None)
                    .label("MIXER")
                    .compact()
                    .when(show_studio_mixer, |b| b.text_color(rgb(0xFF6E00)))
                    .on_click(move |_, _, cx| {
                        let st = state(cx);
                        st.update(cx, |state, cx| {
                            state.show_studio_mixer = !state.show_studio_mixer;
                            cx.notify();
                        });
                    }),
            )
        })
        .into_any_element();

    // -------------------------------------------------------------
    // Columnas según modo (misma strip abajo en ambos)
    // -------------------------------------------------------------
    let columns: Vec<AnyElement> = if is_live {
        let mut cols: Vec<AnyElement> = Vec::new();
        // Columna SCENES + strip del master.
        let master_snap =
            snapshot_master(master_live.as_ref(), vu_master_live, selected_track, selected_slot);
        let scene_cells: Vec<AnyElement> = scene_names
            .iter()
            .enumerate()
            .map(|(i, name)| scene_launcher_cell(i, name))
            .collect();
        cols.push(track_column(
            SCENE_LABEL_WIDTH,
            track_header_cell(
                SCENE_LABEL_WIDTH,
                "SCENES",
                "Master Launch",
                rgb(0x5AB4FF),
            ),
            v_flex().w_full().gap(px(PAD_GAP)).children(scene_cells).into_any_element(),
            channel_strip(&master_snap, "arr_master"),
        ));
        // Una columna por pista: launchers arriba + mismo strip abajo.
        let tracks_len = track_names.len();
        for (idx, name) in track_names.iter().enumerate() {
            let mut pads: Vec<AnyElement> = Vec::new();
            for scene_idx in 0..scene_names.len() {
                let (slot_state, clip_name) = slots
                    .get(idx)
                    .and_then(|row| row.get(scene_idx))
                    .cloned()
                    .unwrap_or((SlotState::Empty, None));
                let clip_name = clip_name.unwrap_or_default();
                let has_clip = !clip_name.is_empty();
                pads.push(render_pad(
                    idx,
                    scene_idx,
                    slot_state,
                    has_clip,
                    clip_name,
                    selected == Some((idx, scene_idx)),
                    clipboard_ready,
                    drop_sample_ready,
                    engine_handle.clone(),
                ));
            }
            let (volume, pan, muted, soloed) =
                track_mix.get(idx).copied().unwrap_or((0.75, 0.0, false, false));
            let (armed, devices, route_dest, sends) = live_devices
                .get(idx)
                .cloned()
                .unwrap_or((false, Vec::new(), "Bus 0".to_string(), Vec::new()));
            let snap = StripSnapshot {
                name: name.clone(),
                header: format!("TRK {:02}", idx + 1),
                volume,
                pan,
                muted,
                soloed,
                armed,
                is_master: false,
                devices,
                route_dest,
                sends,
                vu: vu_live.get(idx).copied().unwrap_or(0.0),
                selected_slot,
                is_selected_track: selected_track == idx + 1,
                target: StripTarget::Live(idx),
            };
            let _ = tracks_len;
            cols.push(track_column(
                TRACK_WIDTH,
                track_header_cell(TRACK_WIDTH, &format!("TRK {:02}", idx + 1), name, rgb(0xE0E0E0)),
                v_flex().w_full().gap(px(PAD_GAP)).children(pads).into_any_element(),
                channel_strip(&snap, &format!("arr_track_{}", idx)),
            ));
        }
        cols
    } else {
        // Modo OpenStudio: mixer lateral (columnas con waveform + strip).
        // El Timeline horizontal vive en `playlist::render` y se coloca a la
        // derecha ocupando todo el espacio flexible (ver layout de abajo).
        studio_strips
            .iter()
            .enumerate()
            .map(|(idx, snap)| {
                let peaks = studio_peaks.get(idx).cloned().unwrap_or(None);
                let header = track_header_cell(
                    TRACK_WIDTH,
                    &snap.header,
                    &snap.name,
                    if snap.is_master {
                        rgb(0x5AB4FF)
                    } else {
                        rgb(0xE0E0E0)
                    },
                );
                track_column(
                    TRACK_WIDTH,
                    header,
                    waveform_container(&snap.name, peaks),
                    channel_strip(snap, &format!("arr_studio_{}", idx)),
                )
            })
            .collect()
    };

    // -------------------------------------------------------------
    // Layout OpenStudio: Vista de Mezcla + Vista de Arreglo (independientes)
    // -------------------------------------------------------------
    // Jerarquía:
    //   - Vista de Mezcla (`studio_mixer_panel`): consola de channel strips
    //     verticales — faders, pan, S/M/R, devices, routing. Vive acá y sólo
    //     acá.
    //   - Vista de Arreglo (`studio_playlist_panel` → `playlist::render`):
    //     lienzo temporal — ruler, filas limpias, grilla, clips, playhead.
    //     No contiene ningún control de mezcla.
    // Ambas leen el mismo `studio_tracks`: añadir/quitar pistas actualiza
    // filas del grid y canales a la vez. El botón MIXER de la toolbar
    // (`show_studio_mixer`) conmuta la consola: oculta, la Playlist ocupa
    // todo el ancho para composición.
    if !is_live {
        let mixer_columns = columns;
        return v_flex()
            .id("arranger_view")
            .size_full()
            .bg(theme::WINDOW_BG)
            .child(toolbar)
            // Wrapper observable del área (mismo `id` que en OpenLive para el
            // harness `arranger_columns_fit`): la medición vive acá porque el
            // `Scrollable` interno sobrescribe el id de su contenido.
            .child(
                div()
                    .id("arranger_columns")
                    .test_support()
                    .flex_1()
                    .w_full()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(
                        h_flex()
                            .id("studio_playlist_layout")
                            .test_support()
                            .flex_1()
                            .w_full()
                            .min_h_0()
                            .overflow_hidden()
                            // Vista de Mezcla: ancho fijo según nº de pistas,
                            // scroll vertical propio. El flex-item es un `div`
                            // plano (el `Scrollable` consume el `id` de su
                            // contenido, así que el scroll vive en el hijo).
                            .when(show_studio_mixer, |l| {
                                l.child(
                                    div()
                                        .id("studio_mixer_panel")
                                        .test_support()
                                        .flex_shrink_0()
                                        .h_full()
                                        .child(
                                            div()
                                                .h_full()
                                                .w_full()
                                                .overflow_y_scrollbar()
                                                .p(px(6.0))
                                                .child(
                                                    h_flex()
                                                        .gap(px(PAD_GAP))
                                                        .items_start()
                                                        .flex_shrink_0()
                                                        .children(mixer_columns),
                                                ),
                                        ),
                                )
                            })
                            // Vista de Arreglo: panel flexible (ruler + grid +
                            // clips + scrollbar horizontal y zoom propios).
                            .child(
                                div()
                                    .id("studio_playlist_panel")
                                    .test_support()
                                    .flex_1()
                                    .h_full()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .p(px(6.0))
                                    .child(playlist::render(cx)),
                            ),
                    ),
            )
            .into_any_element();
    }

    v_flex()
        .id("arranger_view")
        .size_full()
        .bg(theme::WINDOW_BG)
        .child(toolbar)
        // Wrapper observable del área de columnas (para el test de encaje):
        // el `Scrollable` interno sobrescribe el id de su contenido, así que
        // la medición vive en este `div` externo.
        .child(
            div()
                .id("arranger_columns")
                .test_support()
                .flex_1()
                .w_full()
                .min_h_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex_1()
                        .w_full()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .child(
                            div()
                                .w_full()
                                .overflow_x_scrollbar()
                                .p(px(6.0))
                                .child(
                                    h_flex()
                                        .gap(px(PAD_GAP))
                                        .items_start()
                                        .flex_shrink_0()
                                        .children(columns),
                                ),
                        ),
                ),
        )
        .into_any_element()
}
