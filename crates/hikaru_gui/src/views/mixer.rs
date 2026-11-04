// Hikaru OpenLive / OpenStudio - Mixer model + Channel Strip kit
// GNU AGPLv3
// crates/hikaru_gui/src/views/mixer.rs
//
// Este módulo YA NO es una vista independiente: el dock horizontal del Mixer
// inferior (que duplicaba faders y picos) se eliminó. Ahora contiene:
//
// - El modelo de mezcla (`Track`, `DspSlot`, `SendConnection`).
// - El kit de widgets del Channel Strip que `arranger_view.rs` reutiliza en
//   cada columna de pista (OpenLive y OpenStudio comparten el mismo strip):
//   `vertical_vu_meter`, curva de dB y suavizado de picos.
//
// La vista vive en `crate::views::arranger_view`.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonRounded};
use gpui_kit::component::label::Label;
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::*;
use gpui_kit::prelude::{FluentBuilder, InteractiveElement, StatefulInteractiveElement, Styled};
use gpui_kit::*;

use crate::app::{set_footer_cues, state, update_state, AppMode, HikaruApp};
use crate::theme;
use crate::views::arranger_view::{self, FaderTarget, StripTarget};
use crate::views::matrix;

// =========================================================================
// PANEL DE MEZCLA INFERIOR (BITWIG STUDIO PARITY)
// =========================================================================

pub const BOTTOM_MIXER_TRACK_WIDTH: f32 = 85.0;
pub const BOTTOM_MIXER_SIDEBAR_WIDTH: f32 = 40.0;
pub const BOTTOM_MIXER_PINNED_WIDTH: f32 = 172.0;

/// Payload de arrastre de dispositivo DSP entre pistas del Mix Panel (o DSP Rack).
#[derive(Clone, Debug)]
pub struct MixerDeviceDrag {
    pub source_target: StripTarget,
    pub source_slot: usize,
    pub device_name: String,
}

/// Fantasma visual flotante que sigue al cursor al arrastrar un dispositivo.
pub struct MixerDeviceDragGhost {
    pub device_name: String,
}

impl Render for MixerDeviceDragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .bg(rgb(0x1E1F24))
            .border_1()
            .border_color(rgb(theme::DAW_ACCENT_ORANGE))
            .rounded(px(3.0))
            .px(px(8.0))
            .py(px(4.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .shadow_lg()
            .child(
                div()
                    .w(px(6.0))
                    .h(px(6.0))
                    .rounded_full()
                    .bg(rgb(theme::DAW_ACCENT_ORANGE)),
            )
            .child(
                Label::new(self.device_name.clone())
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(0xFFFFFF)),
            )
    }
}

/// Metadata del chip de un dispositivo para el Channel Strip del mixer.
#[derive(Clone, Debug)]
pub struct MixerDeviceChipData {
    pub slot: usize,
    pub name: String,
    pub active: bool,
    pub has_gui: bool,
}

/// Renderiza el Mix Panel inferior de Hikaru OpenLive con paridad visual y funcional absoluta a Bitwig Studio.
pub fn render_bottom_panel(cx: &mut Context<HikaruApp>) -> AnyElement {
    let app = state(cx).read(cx);
    let height = app.bottom_mixer_height.clamp(160.0, 500.0);
    let show_devices = app.bottom_mixer_show_devices;
    let show_sends = app.bottom_mixer_show_sends;
    let show_io = app.bottom_mixer_show_io;
    let mode = app.mode;
    let is_tall = height >= 210.0;
    let scroll_handle = app.bottom_mixer_scroll_h.clone();

    // Snapshot de las pistas de la sesión
    let live_names: Vec<String> = app.live_tracks.iter().map(|t| t.name.clone()).collect();
    let studio_names: Vec<String> = app.studio_tracks.iter().map(|t| t.name.clone()).collect();

    let tracks_snapshot: Vec<(usize, String, f32, f32, bool, bool, bool, u32, f32, Vec<MixerDeviceChipData>, f32, String)> = match mode {
        AppMode::OpenLive => {
            app.matrix_state
                .tracks
                .iter()
                .enumerate()
                .map(|(idx, t)| {
                    let live_tr = app.live_tracks.get(idx + 1);
                    let armed = live_tr.map(|l| l.arm).unwrap_or(false);
                    let peak = app.smoothed_track_peaks.get(idx).copied().unwrap_or(0.0);
                    let stripe = theme::track_stripe_hex(idx);
                    let dev_chips = live_tr
                        .map(|l| {
                            l.effects
                                .iter()
                                .enumerate()
                                .map(|(s, e)| MixerDeviceChipData {
                                    slot: s,
                                    name: e.name.clone(),
                                    active: e.active,
                                    has_gui: e.has_editor(),
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let send_amt = live_tr
                        .and_then(|l| l.sends.first().map(|s| s.amount))
                        .unwrap_or(0.0);
                    let r_dest = live_tr
                        .map(|l| route_dest_label(l.route_destination_id, false, &live_names))
                        .unwrap_or_else(|| "Master".to_string());
                    (idx, t.name.clone(), t.volume, t.pan, t.muted, t.soloed, armed, stripe, peak, dev_chips, send_amt, r_dest)
                })
                .collect()
        }
        AppMode::OpenStudio => {
            app.studio_tracks
                .iter()
                .enumerate()
                .skip(1)
                .map(|(idx, t)| {
                    let peak = app.smoothed_track_peaks.get(idx.saturating_sub(1)).copied().unwrap_or(0.0);
                    let stripe = theme::track_stripe_hex(idx.saturating_sub(1));
                    let dev_chips = t
                        .effects
                        .iter()
                        .enumerate()
                        .map(|(s, e)| MixerDeviceChipData {
                            slot: s,
                            name: e.name.clone(),
                            active: e.active,
                            has_gui: e.has_editor(),
                        })
                        .collect();
                    let send_amt = t.sends.first().map(|s| s.amount).unwrap_or(0.0);
                    let r_dest = route_dest_label(t.route_destination_id, t.is_master, &studio_names);
                    (idx, t.name.clone(), t.volume, t.pan, t.mute, t.solo, t.arm, stripe, peak, dev_chips, send_amt, r_dest)
                })
                .collect()
        }
    };

    // Snapshot de FX y Master
    let fx_peak = app.smoothed_track_peaks.get(15).copied().unwrap_or(0.0);
    let fx_tr = app.fx_tracks.first();
    let fx_volume = fx_tr.map(|t| t.volume).unwrap_or(0.75);
    let fx_pan = fx_tr.map(|t| t.pan).unwrap_or(0.0);
    let fx_muted = fx_tr.map(|t| t.mute).unwrap_or(false);
    let fx_soloed = fx_tr.map(|t| t.solo).unwrap_or(false);
    let fx_route = fx_tr
        .map(|t| route_dest_label(t.route_destination_id, false, &live_names))
        .unwrap_or_else(|| "Master".to_string());
    let fx_devs = fx_tr
        .map(|t| {
            t.effects
                .iter()
                .enumerate()
                .map(|(s, e)| MixerDeviceChipData {
                    slot: s,
                    name: e.name.clone(),
                    active: e.active,
                    has_gui: e.has_editor(),
                })
                .collect()
        })
        .unwrap_or_default();

    let master_peak = app.smoothed_master_peak;
    let master_tr = app.tracks().first();
    let master_volume = master_tr.map(|t| t.volume).unwrap_or(0.75);
    let master_pan = master_tr.map(|t| t.pan).unwrap_or(0.0);
    let master_muted = master_tr.map(|t| t.mute).unwrap_or(false);
    let master_soloed = master_tr.map(|t| t.solo).unwrap_or(false);
    let master_route = "Out 1/2".to_string();
    let master_devs = master_tr
        .map(|t| {
            t.effects
                .iter()
                .enumerate()
                .map(|(s, e)| MixerDeviceChipData {
                    slot: s,
                    name: e.name.clone(),
                    active: e.active,
                    has_gui: e.has_editor(),
                })
                .collect()
        })
        .unwrap_or_default();

    let selected_track = app.selected_track_index;
    let _ = app;

    v_flex()
        .id("bottom_mix_panel_container")
        .w_full()
        .h(px(height))
        .flex_shrink_0()
        .bg(rgb(0x18181A))
        .border_t_1()
        .border_color(rgb(0x28292E))
        .overflow_hidden()
        // 1. SPLITTER SUPERIOR RESIZABLE
        .child(render_bottom_splitter(height))
        // 2. CUERPO DEL MIX PANEL
        .child(
            h_flex()
                .id("bottom_mix_panel_body")
                .w_full()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                // A. SIDEBAR IZQUIERDO CON LABELS Y TOGGLES
                .child(render_sidebar(is_tall, show_devices, show_sends, show_io))
                // B. CANALES DE PISTA CON SCROLL HORIZONTAL
                .child(
                    div()
                        .id("bottom_mixer_tracks_scroll")
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .overflow_x_scroll()
                        .track_scroll(&scroll_handle)
                        .child(
                            h_flex()
                                .min_w_full()
                                .h_full()
                                .gap(px(1.0))
                                .p(px(2.0))
                                .items_stretch()
                                .children(
                                    tracks_snapshot.into_iter().map(|(idx, name, vol, pan, muted, soloed, armed, stripe, peak, devs, send_amt, route_dest)| {
                                        let target = match mode {
                                            AppMode::OpenLive => StripTarget::Live(idx),
                                            AppMode::OpenStudio => StripTarget::Studio(idx),
                                        };
                                        let is_selected = match mode {
                                            AppMode::OpenLive => selected_track == idx + 1,
                                            AppMode::OpenStudio => selected_track == idx,
                                        };
                                        render_track_strip(
                                            idx,
                                            name,
                                            vol,
                                            pan,
                                            muted,
                                            soloed,
                                            armed,
                                            stripe,
                                            peak,
                                            devs,
                                            send_amt,
                                            route_dest,
                                            target,
                                            is_selected,
                                            is_tall,
                                            show_devices,
                                            show_sends,
                                            show_io,
                                        )
                                    })
                                )
                                // Botón [+] para añadir pista
                                .child(render_add_track_button())
                        )
                )
                // C. SECCIÓN FIJA A LA DERECHA (FX 1 Y MASTER)
                .child(
                    h_flex()
                        .w(px(BOTTOM_MIXER_PINNED_WIDTH))
                        .flex_shrink_0()
                        .h_full()
                        .bg(rgb(0x1F2023))
                        .border_l_1()
                        .border_color(rgb(0x2E3036))
                        .p(px(2.0))
                        .gap(px(1.0))
                        .items_stretch()
                        .child(render_fx_strip(
                            fx_volume,
                            fx_pan,
                            fx_muted,
                            fx_soloed,
                            fx_peak,
                            fx_devs,
                            fx_route,
                            is_tall,
                            show_devices,
                            show_sends,
                            show_io,
                        ))
                        .child(render_master_strip(
                            master_volume,
                            master_pan,
                            master_muted,
                            master_soloed,
                            master_peak,
                            master_devs,
                            master_route,
                            is_tall,
                            show_devices,
                            show_sends,
                            show_io,
                        ))
                )
        )
        .into_any_element()
}

/// Splitter superior horizontal para arrastrar la altura del Mix Panel.
fn render_bottom_splitter(current_height: f32) -> AnyElement {
    div()
        .id("bottom_mixer_splitter")
        .w_full()
        .h(px(4.0))
        .flex_shrink_0()
        .bg(rgb(0x202022))
        .border_t_1()
        .border_color(rgb(0x38393F))
        .cursor_ns_resize()
        .hover(|s| s.bg(rgb(theme::DAW_ACCENT_ORANGE)))
        .on_mouse_move(|e, _, cx| {
            set_footer_cues(cx, Some(e.modifiers), &[("DRAG", "Resize Mix Panel height")]);
        })
        .on_mouse_down(gpui_kit::MouseButton::Left, move |event, _, cx| {
            let start_y = event.position.y.as_f32();
            update_state(cx, |s| {
                s.bottom_mixer_splitter_drag = Some((start_y, current_height));
            });
        })
        .into_any_element()
}

/// Sidebar izquierdo que contiene las etiquetas de sección y los iconos de alternancia (Devices, Sends, I/O).
fn render_sidebar(is_tall: bool, show_dev: bool, show_sends: bool, show_io: bool) -> AnyElement {
    let accent = rgb(theme::DAW_ACCENT_ORANGE);
    let inactive = rgb(0x60646D);

    v_flex()
        .w(px(BOTTOM_MIXER_SIDEBAR_WIDTH))
        .flex_shrink_0()
        .h_full()
        .bg(rgb(0x222326))
        .border_r_1()
        .border_color(rgb(0x2A2B30))
        .justify_between()
        .overflow_hidden()
        // 1. Sección superior: etiquetas alineadas con las secciones de canal cuando is_tall
        .child(
            v_flex()
                .w_full()
                .overflow_hidden()
                // Espaciador alineado con track header (36px)
                .child(div().w_full().h(px(36.0)).flex_shrink_0())
                .when(is_tall && show_dev, |this| {
                    this.child(
                        h_flex()
                            .w_full()
                            .h(px(38.0))
                            .flex_shrink_0()
                            .items_center()
                            .justify_center()
                            .child(
                                Label::new("Devices")
                                    .text_size(px(8.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(0x868B95)),
                            ),
                    )
                })
                .when(is_tall && show_sends, |this| {
                    this.child(
                        h_flex()
                            .w_full()
                            .h(px(34.0))
                            .flex_shrink_0()
                            .items_center()
                            .justify_center()
                            .child(
                                Label::new("Sends")
                                    .text_size(px(8.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(0x868B95)),
                            ),
                    )
                })
                .when(is_tall && show_io, |this| {
                    this.child(
                        h_flex()
                            .w_full()
                            .h(px(34.0))
                            .flex_shrink_0()
                            .items_center()
                            .justify_center()
                            .child(
                                Label::new("I/O")
                                    .text_size(px(8.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(0x868B95)),
                            ),
                    )
                })
        )
        // 2. Sección inferior: Sx/Mx y conmutadores estilo Bitwig Studio
        .child(
            v_flex()
                .w_full()
                .flex_shrink_0()
                .pb(px(4.0))
                .gap(px(3.0))
                .items_center()
                // Separador horizontal
                .child(
                    div()
                        .w_full()
                        .h(px(1.0))
                        .bg(rgb(0x2F3035))
                        .flex_shrink_0(),
                )
                // Fila Sx y Mx (Clear Solos / Clear Mutes)
                .child(
                    h_flex()
                        .w_full()
                        .px(px(4.0))
                        .justify_between()
                        .child(
                            div()
                                .id("sidebar_clear_solo")
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(0x35373E)).rounded(px(2.0)))
                                .on_click(|_, _, cx| {
                                    update_state(cx, |s| {
                                        for t in &mut s.matrix_state.tracks {
                                            t.soloed = false;
                                        }
                                        for t in &mut s.live_tracks {
                                            t.solo = false;
                                        }
                                        for t in &mut s.studio_tracks {
                                            t.solo = false;
                                        }
                                        for t in &mut s.fx_tracks {
                                            t.solo = false;
                                        }
                                    });
                                })
                                .child(
                                    Label::new("Sx")
                                        .text_size(px(9.0))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(rgb(0x8E94A0)),
                                ),
                        )
                        .child(
                            div()
                                .id("sidebar_clear_mute")
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(0x35373E)).rounded(px(2.0)))
                                .on_click(|_, _, cx| {
                                    update_state(cx, |s| {
                                        for t in &mut s.matrix_state.tracks {
                                            t.muted = false;
                                        }
                                        for t in &mut s.live_tracks {
                                            t.mute = false;
                                        }
                                        for t in &mut s.studio_tracks {
                                            t.mute = false;
                                        }
                                        for t in &mut s.fx_tracks {
                                            t.mute = false;
                                        }
                                    });
                                })
                                .child(
                                    Label::new("Mx")
                                        .text_size(px(9.0))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(rgb(0x8E94A0)),
                                ),
                        ),
                )
                // Grid de conmutadores de sección
                // Fila 1: [|] Devices
                .child(
                    h_flex()
                        .w_full()
                        .px(px(4.0))
                        .justify_between()
                        .items_center()
                        .child(
                            div()
                                .id("sidebar_toggle_devices")
                                .cursor_pointer()
                                .p(px(2.0))
                                .hover(|s| s.bg(rgb(0x35373E)).rounded(px(2.0)))
                                .on_mouse_move(|e, _, cx| {
                                    set_footer_cues(cx, Some(e.modifiers), &[("CLICK", "Toggle Devices section")]);
                                })
                                .on_click(|_, _, cx| {
                                    update_state(cx, |s| {
                                        s.bottom_mixer_show_devices = !s.bottom_mixer_show_devices;
                                    });
                                })
                                .child(icon_sidebar_device(if show_dev { accent } else { inactive })),
                        )
                        .child(
                            div()
                                .p(px(2.0))
                                .child(icon_sidebar_knob(inactive)),
                        ),
                )
                // Fila 2: ↗ Sends
                .child(
                    h_flex()
                        .w_full()
                        .px(px(4.0))
                        .justify_between()
                        .items_center()
                        .child(
                            div()
                                .id("sidebar_toggle_sends")
                                .cursor_pointer()
                                .p(px(2.0))
                                .hover(|s| s.bg(rgb(0x35373E)).rounded(px(2.0)))
                                .on_mouse_move(|e, _, cx| {
                                    set_footer_cues(cx, Some(e.modifiers), &[("CLICK", "Toggle Sends section")]);
                                })
                                .on_click(|_, _, cx| {
                                    update_state(cx, |s| {
                                        s.bottom_mixer_show_sends = !s.bottom_mixer_show_sends;
                                    });
                                })
                                .child(icon_sidebar_sends(if show_sends { accent } else { inactive })),
                        )
                        .child(
                            div()
                                .p(px(2.0))
                                .child(icon_sidebar_routing(inactive)),
                        ),
                )
                // Fila 3: ⇄ I/O
                .child(
                    h_flex()
                        .w_full()
                        .px(px(4.0))
                        .justify_between()
                        .items_center()
                        .child(
                            div()
                                .id("sidebar_toggle_io")
                                .cursor_pointer()
                                .p(px(2.0))
                                .hover(|s| s.bg(rgb(0x35373E)).rounded(px(2.0)))
                                .on_mouse_move(|e, _, cx| {
                                    set_footer_cues(cx, Some(e.modifiers), &[("CLICK", "Toggle Input/Output section")]);
                                })
                                .on_click(|_, _, cx| {
                                    update_state(cx, |s| {
                                        s.bottom_mixer_show_io = !s.bottom_mixer_show_io;
                                    });
                                })
                                .child(icon_sidebar_io(if show_io { accent } else { inactive })),
                        )
                        .child(
                            div()
                                .p(px(2.0))
                                .child(
                                    Label::new("✕")
                                        .text_size(px(8.5))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(inactive),
                                ),
                        ),
                )
                // Separador sutil
                .child(
                    div()
                        .w(px(32.0))
                        .h(px(1.0))
                        .bg(rgb(0x2F3035))
                        .flex_shrink_0(),
                )
                // Fila 4: AB y Comentarios
                .child(
                    h_flex()
                        .w_full()
                        .px(px(4.0))
                        .justify_between()
                        .items_center()
                        .child(
                            Label::new("AB")
                                .text_size(px(8.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(inactive),
                        )
                        .child(
                            Label::new("💬")
                                .text_size(px(9.0))
                                .text_color(inactive),
                        ),
                )
        )
        .into_any_element()
}

// Iconografía milimétrica del sidebar del Mix Panel

fn icon_sidebar_device(col: Rgba) -> impl IntoElement {
    div()
        .w(px(12.0))
        .h(px(10.0))
        .rounded(px(1.0))
        .border_1()
        .border_color(col)
        .relative()
        .child(
            div()
                .absolute()
                .left(px(3.0))
                .top(px(1.0))
                .bottom(px(1.0))
                .w(px(1.0))
                .bg(col),
        )
}

fn icon_sidebar_knob(col: Rgba) -> impl IntoElement {
    div()
        .w(px(10.0))
        .h(px(10.0))
        .rounded_full()
        .border_1()
        .border_color(col)
        .relative()
        .child(
            div()
                .absolute()
                .left(px(4.0))
                .top(px(1.0))
                .w(px(1.0))
                .h(px(3.0))
                .bg(col),
        )
}

fn icon_sidebar_sends(col: Rgba) -> impl IntoElement {
    Label::new("↗")
        .text_size(px(9.5))
        .font_weight(FontWeight::BOLD)
        .text_color(col)
}

fn icon_sidebar_routing(col: Rgba) -> impl IntoElement {
    Label::new("⤵")
        .text_size(px(9.5))
        .font_weight(FontWeight::BOLD)
        .text_color(col)
}

fn icon_sidebar_io(col: Rgba) -> impl IntoElement {
    Label::new("⇄")
        .text_size(px(10.0))
        .font_weight(FontWeight::BOLD)
        .text_color(col)
}

/// Canal individual de pista en el Mix Panel inferior.
fn render_track_strip(
    idx: usize,
    name: String,
    volume: f32,
    pan: f32,
    muted: bool,
    soloed: bool,
    armed: bool,
    stripe_color: u32,
    peak_level: f32,
    devices: Vec<MixerDeviceChipData>,
    send_amount: f32,
    route_dest: String,
    target: StripTarget,
    is_selected: bool,
    is_tall: bool,
    show_devices: bool,
    show_sends: bool,
    show_io: bool,
) -> AnyElement {
    let vol_target = match target {
        StripTarget::Live(i) => FaderTarget::TrackVolume(i),
        StripTarget::Studio(i) => FaderTarget::StudioVolume(i),
        StripTarget::Fx(i) => FaderTarget::FxVolume(i),
        StripTarget::Master => FaderTarget::MasterVolume,
    };
    let pan_target = match target {
        StripTarget::Live(i) => FaderTarget::TrackPan(i),
        StripTarget::Studio(i) => FaderTarget::StudioPan(i),
        StripTarget::Fx(i) => FaderTarget::FxPan(i),
        StripTarget::Master => FaderTarget::MasterPan,
    };

    v_flex()
        .id(SharedString::from(format!("bottom_strip_{:?}", target)))
        .w(px(BOTTOM_MIXER_TRACK_WIDTH))
        .flex_shrink_0()
        .h_full()
        .bg(rgb(0x232427))
        .border_1()
        .border_color(if is_selected {
            rgb(0x5A606C)
        } else {
            rgb(0x1B1C1F)
        })
        .rounded(px(2.0))
        .overflow_hidden()
        // Drop target general para la pista entera: soltar un dispositivo aquí lo traslada a esta pista
        .can_drop(|payload: &dyn std::any::Any, _, _| payload.is::<MixerDeviceDrag>())
        .on_drop(move |payload: &MixerDeviceDrag, _, cx| {
            update_state(cx, |s| {
                s.move_device(payload.source_target, payload.source_slot, target, usize::MAX);
            });
        })
        .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| {
            arranger_view::select_track_column(cx, target);
        })
        // 1. Header de Pista
        .child(render_strip_header(name, idx + 1, stripe_color, is_selected, target))
        // 2. Sección Devices (cuando is_tall && show_devices)
        .when(is_tall && show_devices, |this| {
            this.child(render_strip_devices(devices, target))
        })
        // 3. Sección Sends (cuando is_tall && show_sends)
        .when(is_tall && show_sends, |this| {
            this.child(render_strip_sends(idx, send_amount))
        })
        // 4. Sección I/O (cuando is_tall && show_io)
        .when(is_tall && show_io, |this| {
            this.child(render_strip_io(target, route_dest))
        })
        // 5. Zona de Controles: M/S/Arm + Pan + Fader & VU
        .child(
            v_flex()
                .w_full()
                .flex_1()
                .min_h_0()
                .p(px(2.0))
                .gap(px(2.0))
                .overflow_hidden()
                .child(render_smr_buttons(target, armed, soloed, muted))
                .child(render_pan_slider(pan_target, pan))
                .child(render_fader_vu_well(vol_target, volume, peak_level, stripe_color))
        )
        // 6. Barra de Acento de Color Inferior
        .child(
            div()
                .w_full()
                .h(px(3.0))
                .flex_shrink_0()
                .bg(rgb(stripe_color)),
        )
        .into_any_element()
}

/// Header de pista con nombre y número de canal centrado.
fn render_strip_header(
    name: String,
    track_num: usize,
    stripe_color: u32,
    is_selected: bool,
    target: StripTarget,
) -> AnyElement {
    v_flex()
        .id(SharedString::from(format!("bstrip_hdr_{:?}", target)))
        .w_full()
        .h(px(36.0))
        .flex_shrink_0()
        .bg(rgb(stripe_color))
        .justify_center()
        .items_center()
        .px(px(2.0))
        .cursor_pointer()
        .when(is_selected, |s| s.border_t_2().border_color(rgb(0xFFFFFF)))
        .on_mouse_move(|e, _, cx| {
            set_footer_cues(
                cx,
                Some(e.modifiers),
                &[
                    ("CLICK", "Select Track"),
                    ("CTRL+CLICK", "Remove from selection"),
                    ("ALT+CLICK", "Rename"),
                    ("DOUBLE-CLICK", "Show devices"),
                ],
            );
        })
        .on_mouse_down(gpui_kit::MouseButton::Left, move |event, _, cx| {
            if event.click_count >= 2 {
                update_state(cx, |s| {
                    s.show_dsp_rack = true;
                });
            }
            arranger_view::select_track_column(cx, target);
        })
        .child(
            Label::new(name)
                .text_size(px(10.0))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0x141414))
                .text_ellipsis(),
        )
        .child(
            Label::new(format!("{}", track_num))
                .text_size(px(8.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(0x1A1A1A)),
        )
        .into_any_element()
}

/// Pozo de dispositivos (Devices) compacto con soporte total de Drag & Drop y reordenamiento.
fn render_strip_devices(devices: Vec<MixerDeviceChipData>, target: StripTarget) -> AnyElement {
    let dev_count = devices.len();
    h_flex()
        .id(SharedString::from(format!("bstrip_dev_row_{:?}", target)))
        .w_full()
        .h(px(38.0))
        .flex_shrink_0()
        .bg(rgb(0x191A1D))
        .border_b_1()
        .border_color(rgb(0x28292E))
        .p(px(2.0))
        .gap(px(2.0))
        .items_center()
        .overflow_x_scrollbar()
        // Drop target del contenedor: arrastrar y soltar un dispositivo aquí lo mueve al final de la cadena de esta pista
        .can_drop(|payload: &dyn std::any::Any, _, _| payload.is::<MixerDeviceDrag>())
        .on_drop(move |payload: &MixerDeviceDrag, _, cx| {
            update_state(cx, |s| {
                s.move_device(payload.source_target, payload.source_slot, target, dev_count);
            });
        })
        // Botón [+] para añadir device
        .child(
            div()
                .id(SharedString::from(format!("bstrip_add_dev_{:?}", target)))
                .w(px(14.0))
                .h(px(14.0))
                .flex_shrink_0()
                .rounded(px(2.0))
                .bg(rgb(0x2A2B30))
                .hover(|s| s.bg(rgb(0x35373E)))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_move(|e, _, cx| {
                    set_footer_cues(cx, Some(e.modifiers), &[("CLICK", "Add device to track")]);
                })
                .on_click(|_, _, cx| {
                    update_state(cx, |s| {
                        s.show_dsp_rack = true;
                    });
                })
                .child(
                    Label::new("+")
                        .text_size(px(9.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0x8E94A0)),
                ),
        )
        .children(
            devices.into_iter().map(|dev| {
                let slot = dev.slot;
                let dname = dev.name;
                let is_active = dev.active;
                let has_gui = dev.has_gui;
                let target_clone = target;
                let drag_payload = MixerDeviceDrag {
                    source_target: target,
                    source_slot: slot,
                    device_name: dname.clone(),
                };

                div()
                    .id(SharedString::from(format!("bstrip_dev_chip_{:?}_{}", target, slot)))
                    .h(px(28.0))
                    .px(px(4.0))
                    .flex_shrink_0()
                    .rounded(px(2.0))
                    .bg(rgb(0x24252A))
                    .hover(|s| s.bg(rgb(0x2E3036)))
                    .border_1()
                    .border_color(if is_active { rgb(0x3B3D44) } else { rgb(0x26272B) })
                    .flex()
                    .items_center()
                    .gap(px(3.0))
                    .cursor_grab()
                    // Drag and drop: habilita arrastrar este dispositivo a cualquier otra pista
                    .on_drag(drag_payload, |payload, _, _, cx| {
                        let name = payload.device_name.clone();
                        cx.new(|_| MixerDeviceDragGhost { device_name: name })
                    })
                    // Drop target: soltar sobre este chip inserta en este slot
                    .can_drop(|payload: &dyn std::any::Any, _, _| payload.is::<MixerDeviceDrag>())
                    .on_drop(move |payload: &MixerDeviceDrag, _, cx| {
                        update_state(cx, |s| {
                            s.move_device(payload.source_target, payload.source_slot, target_clone, slot);
                        });
                    })
                    .on_mouse_move(move |e, _, cx| {
                        set_footer_cues(
                            cx,
                            Some(e.modifiers),
                            &[
                                ("CLICK", "Select device"),
                                ("DRAG", "Move device to another track"),
                                ("CLICK ●", "Toggle device on/off"),
                            ],
                        );
                    })
                    .on_click(move |_, _, cx| {
                        arranger_view::select_device_slot(cx, target, slot);
                    })
                    // Botón LED indicador de activación (●)
                    .child(
                        div()
                            .w(px(5.0))
                            .h(px(5.0))
                            .rounded_full()
                            .bg(if is_active { rgb(theme::DAW_ACCENT_ORANGE) } else { rgb(0x555860) })
                            .cursor_pointer()
                            .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| {
                                arranger_view::toggle_device_active(cx, target, slot);
                            })
                    )
                    .child(
                        Label::new(if dname.is_empty() { "Empty".to_string() } else { dname })
                            .text_size(px(8.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if is_active { rgb(0xDCE0E8) } else { rgb(0x767A84) }),
                    )
                    .when(has_gui, |this| {
                        this.child(
                            div()
                                .w(px(10.0))
                                .h(px(10.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(1.0))
                                .bg(rgb(0x32343B))
                                .hover(|s| s.bg(rgb(0x42454F)))
                                .cursor_pointer()
                                .child(
                                    Label::new("⧉")
                                        .text_size(px(6.5))
                                        .text_color(rgb(0xA0A4B0)),
                                )
                        )
                    })
                    .into_any_element()
            })
        )
        .when(dev_count == 0, |this| {
            this.child(
                Label::new("No devices")
                    .text_size(px(7.5))
                    .text_color(rgb(0x525660)),
            )
        })
        .into_any_element()
}

/// Sección de envíos auxiliares (Sends) con rotary knob estilo Bitwig.
fn render_strip_sends(track_idx: usize, send_amount: f32) -> AnyElement {
    let s_val = send_amount.clamp(0.0, 1.0);

    h_flex()
        .w_full()
        .h(px(34.0))
        .flex_shrink_0()
        .bg(rgb(0x222326))
        .border_b_1()
        .border_color(rgb(0x28292E))
        .px(px(4.0))
        .justify_between()
        .items_center()
        .child(
            Label::new("FX 1")
                .text_size(px(8.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(0x9498A0)),
        )
        .child(
            div()
                .id(SharedString::from(format!("bottom_send_knob_{}", track_idx)))
                .w(px(18.0))
                .h(px(18.0))
                .cursor_ns_resize()
                .on_mouse_down(gpui_kit::MouseButton::Left, move |event, _, cx| {
                    if event.click_count >= 2 {
                        update_state(cx, |s| {
                            let mix_idx = track_idx + 1;
                            if let Some(tr) = s.live_tracks.get_mut(mix_idx) {
                                if let Some(first_send) = tr.sends.first_mut() {
                                    first_send.amount = 0.0;
                                }
                            }
                        });
                    }
                })
                .on_mouse_move(move |e, _, cx| {
                    let db_str = if s_val <= 0.001 {
                        "-inf dB".to_string()
                    } else {
                        format!("{:.1} dB", (20.0 * s_val.log10()).clamp(-60.0, 6.0))
                    };
                    set_footer_cues(
                        cx,
                        Some(e.modifiers),
                        &[
                            (format!("SEND {}", db_str).leak(), "DRAG Adjust value"),
                            ("SHIFT+DRAG", "Fine-adjust"),
                            ("DOUBLE-CLICK", "Reset to -inf dB"),
                        ],
                    );
                })
                .child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            let cx = bounds.origin.x + bounds.size.width / 2.0;
                            let cy = bounds.origin.y + bounds.size.height / 2.0;
                            let r = px(7.0);

                            // 1. Cuerpo circular oscuro
                            window.paint_quad(PaintQuad {
                                bounds: Bounds::new(point(cx - r, cy - r), size(r * 2.0, r * 2.0)),
                                background: rgb(0x383B42).into(),
                                border_color: rgb(0x4E525C).into(),
                                corner_radii: gpui_kit::Corners::all(r),
                                border_widths: gpui_kit::Edges::all(px(1.0)),
                                border_style: BorderStyle::default(),
                            });

                            // 2. Aguja indicadora (a las 7:30 en punto por defecto: -135 deg)
                            let s_norm = if s_val.is_finite() { s_val.clamp(0.0, 1.0) } else { 0.0 };
                            let deg = -135.0 + s_norm * 270.0;
                            let rad = deg.to_radians();
                            let p_len = 5.0;
                            let nx = cx.as_f32() + rad.sin() * p_len;
                            let ny = cy.as_f32() - rad.cos() * p_len;

                            let mut path = PathBuilder::stroke(px(1.5));
                            path.move_to(point(cx, cy));
                            path.line_to(point(px(nx), px(ny)));
                            if let Ok(path) = path.build() {
                                window.paint_path(path, rgb(0xEAECEF));
                            }
                        },
                    )
                    .size_full(),
                ),
        )
        .into_any_element()
}

/// Sección de selector I/O (Input / Output) de la pista en el Mix Panel inferior.
fn render_strip_io(target: StripTarget, route_dest: String) -> AnyElement {
    v_flex()
        .w_full()
        .h(px(34.0))
        .flex_shrink_0()
        .bg(rgb(0x1D1E21))
        .border_b_1()
        .border_color(rgb(0x28292E))
        .px(px(3.0))
        .py(px(1.0))
        .gap(px(1.0))
        .justify_center()
        // Input row
        .child(
            h_flex()
                .w_full()
                .h(px(13.0))
                .px(px(2.0))
                .bg(rgb(0x26272C))
                .rounded(px(1.5))
                .items_center()
                .justify_between()
                .cursor_pointer()
                .on_mouse_move(|e, _, cx| {
                    set_footer_cues(cx, Some(e.modifiers), &[("CLICK", "Select audio input")]);
                })
                .child(
                    Label::new("◀ All ins")
                        .text_size(px(7.5))
                        .text_color(rgb(0xC4C8D0)),
                )
                .child(
                    Label::new("▾")
                        .text_size(px(6.5))
                        .text_color(rgb(0x80848E)),
                ),
        )
        // Output row con dropdown interactivo estilo Bitwig Studio
        .child(
            h_flex()
                .w_full()
                .h(px(13.0))
                .px(px(2.0))
                .bg(rgb(0x26272C))
                .rounded(px(1.5))
                .items_center()
                .justify_between()
                .child(
                    div()
                        .id(SharedString::from(format!("bstrip_route_lbl_{:?}", target)))
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .cursor_pointer()
                        .on_mouse_move(|e, _, cx| {
                            set_footer_cues(
                                cx,
                                Some(e.modifiers),
                                &[
                                    ("CLICK", "Next output"),
                                    ("▾", "Choose output destination"),
                                ],
                            );
                        })
                        .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| {
                            arranger_view::cycle_route_destination(cx, target)
                        })
                        .child(
                            Label::new(format!("▶ {}", route_dest))
                                .text_size(px(7.5))
                                .text_color(rgb(0xC4C8D0))
                                .text_ellipsis(),
                        ),
                )
                .child(
                    Button::new(SharedString::from(format!("bstrip_route_{:?}", target)))
                        .label("▾")
                        .compact()
                        .bg(rgb(0x26272C))
                        .text_color(rgb(0x80848E))
                        .rounded(ButtonRounded::None)
                        .dropdown_menu(move |menu, _window, cx| {
                            arranger_view::build_route_menu(menu, target, cx)
                        }),
                ),
        )
        .into_any_element()
}

/// Botonera compacta [●] Arm, [S] Solo, [M] Mute con paridad visual a Bitwig.
fn render_smr_buttons(target: StripTarget, armed: bool, soloed: bool, muted: bool) -> AnyElement {
    let is_master = target == StripTarget::Master;
    let is_fx = matches!(target, StripTarget::Fx(_));

    if is_master || is_fx {
        h_flex()
            .w_full()
            .h(px(20.0))
            .gap(px(2.0))
            .flex_shrink_0()
            .justify_center()
            .child(matrix::ms_button_sized(
                format!("bsmr_solo_{:?}", target),
                "S",
                38.0,
                18.0,
                soloed,
                rgb(theme::DAW_SOLO_GOLD),
                move |cx| arranger_view::toggle_solo_target(cx, target),
            ))
            .child(matrix::ms_button_sized(
                format!("bsmr_mute_{:?}", target),
                "M",
                38.0,
                18.0,
                muted,
                rgb(theme::DAW_MUTE_AMBER),
                move |cx| arranger_view::toggle_mute_target(cx, target),
            ))
            .into_any_element()
    } else {
        h_flex()
            .w_full()
            .h(px(20.0))
            .gap(px(2.0))
            .flex_shrink_0()
            .justify_center()
            .child(matrix::ms_button_sized(
                format!("bsmr_rec_{:?}", target),
                "●",
                24.0,
                18.0,
                armed,
                rgb(theme::DAW_ARM_RED),
                move |cx| arranger_view::toggle_arm_target(cx, target),
            ))
            .child(matrix::ms_button_sized(
                format!("bsmr_solo_{:?}", target),
                "S",
                24.0,
                18.0,
                soloed,
                rgb(theme::DAW_SOLO_GOLD),
                move |cx| arranger_view::toggle_solo_target(cx, target),
            ))
            .child(matrix::ms_button_sized(
                format!("bsmr_mute_{:?}", target),
                "M",
                24.0,
                18.0,
                muted,
                rgb(theme::DAW_MUTE_AMBER),
                move |cx| arranger_view::toggle_mute_target(cx, target),
            ))
            .into_any_element()
    }
}

/// Slider horizontal de paneo bipolar centrado.
fn render_pan_slider(target: FaderTarget, pan: f32) -> AnyElement {
    let b_bounds: Rc<Cell<[f32; 4]>> = Rc::new(Cell::new([0.0; 4]));
    let b_down = b_bounds.clone();
    let b_move = b_bounds.clone();
    let b_paint = b_bounds.clone();

    let pan_label = if pan.abs() < 0.01 {
        "C".to_string()
    } else if pan < 0.0 {
        format!("L{:.0}", pan.abs() * 100.0)
    } else {
        format!("R{:.0}", pan * 100.0)
    };

    div()
        .id(SharedString::from(format!("bpan_{:?}", target)))
        .w_full()
        .h(px(16.0))
        .flex_shrink_0()
        .bg(rgb(0x141517))
        .border_1()
        .border_color(rgb(0x26282E))
        .rounded(px(2.0))
        .relative()
        .overflow_hidden()
        .cursor_ew_resize()
        .on_mouse_down(gpui_kit::MouseButton::Left, move |event, _, cx| {
            if event.click_count >= 2 {
                arranger_view::set_pan(cx, target, 0.0);
                return;
            }
            let b = b_down.get();
            let travel = (b[2] - 4.0).max(1.0);
            let norm = ((event.position.x.as_f32() - b[0] - 2.0) / travel).clamp(0.0, 1.0);
            let new_pan = norm * 2.0 - 1.0;
            arranger_view::set_pan(cx, target, new_pan);
        })
        .on_mouse_move(move |event, _, cx| {
            if event.pressed_button != Some(gpui_kit::MouseButton::Left) {
                crate::app::set_footer_cues(
                    cx,
                    Some(event.modifiers),
                    &[("DRAG", "Adjust panning"), ("DOUBLE-CLICK", "Center (C)")],
                );
                return;
            }
            let b = b_move.get();
            let travel = (b[2] - 4.0).max(1.0);
            let norm = ((event.position.x.as_f32() - b[0] - 2.0) / travel).clamp(0.0, 1.0);
            let new_pan = norm * 2.0 - 1.0;
            arranger_view::set_pan(cx, target, new_pan);
        })
        .child(
            canvas(
                move |bounds, _, _| {
                    b_paint.set([
                        bounds.origin.x.as_f32(),
                        bounds.origin.y.as_f32(),
                        bounds.size.width.as_f32(),
                        bounds.size.height.as_f32(),
                    ]);
                },
                move |bounds, _, window, _| {
                    let total_w = bounds.size.width.as_f32();
                    let total_h = bounds.size.height.as_f32();
                    let center_x = total_w / 2.0;
                    let norm = (pan.clamp(-1.0, 1.0) + 1.0) / 2.0;
                    let needle_x = 2.0 + norm * (total_w - 4.0).max(1.0);

                    // Bipolar fill
                    let (fill_x, fill_w) = if needle_x < center_x {
                        (needle_x, center_x - needle_x)
                    } else {
                        (center_x, needle_x - center_x)
                    };
                    if fill_w > 0.5 {
                        window.paint_quad(PaintQuad {
                            bounds: Bounds::new(
                                point(bounds.origin.x + px(fill_x), bounds.origin.y + px(2.0)),
                                size(px(fill_w), px(total_h - 4.0)),
                            ),
                            background: rgb(theme::DAW_FADER_FILL).into(),
                            border_color: Hsla::default(),
                            corner_radii: gpui_kit::Corners::default(),
                            border_widths: gpui_kit::Edges::default(),
                            border_style: BorderStyle::default(),
                        });
                    }

                    // Línea central de referencia
                    window.paint_quad(PaintQuad {
                        bounds: Bounds::new(
                            point(bounds.origin.x + px(center_x - 0.5), bounds.origin.y + px(2.0)),
                            size(px(1.0), px(total_h - 4.0)),
                        ),
                        background: rgb(0x4E525C).into(),
                        border_color: Hsla::default(),
                        corner_radii: gpui_kit::Corners::default(),
                        border_widths: gpui_kit::Edges::default(),
                        border_style: BorderStyle::default(),
                    });

                    // Aguja naranja brillante
                    window.paint_quad(PaintQuad {
                        bounds: Bounds::new(
                            point(bounds.origin.x + px(needle_x - 1.0), bounds.origin.y + px(1.0)),
                            size(px(2.0), px(total_h - 2.0)),
                        ),
                        background: rgb(theme::DAW_ACCENT_ORANGE).into(),
                        border_color: Hsla::default(),
                        corner_radii: gpui_kit::Corners::default(),
                        border_widths: gpui_kit::Edges::default(),
                        border_style: BorderStyle::default(),
                    });
                },
            )
            .size_full(),
        )
        // Texto de paneo alineado a la derecha
        .child(
            div()
                .absolute()
                .right(px(3.0))
                .top_0()
                .bottom_0()
                .flex()
                .items_center()
                .child(
                    Label::new(pan_label)
                        .text_size(px(7.5))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0xC8CCD4)),
                ),
        )
        .into_any_element()
}

/// Cámara empotrada con lectura superior de dB, escala calibrada, medidor VU estéreo y fader vertical ancho.
fn render_fader_vu_well(
    target: FaderTarget,
    volume: f32,
    peak_level: f32,
    accent_color: u32,
) -> AnyElement {
    let fader_bounds: Rc<Cell<[f32; 4]>> = Rc::new(Cell::new([0.0; 4]));
    let f_down = fader_bounds.clone();
    let f_move = fader_bounds.clone();
    let f_paint = fader_bounds.clone();

    let (vu_text, vu_clip) = peak_db_readout(peak_level);
    let vol_text = db_text(volume);

    v_flex()
        .w_full()
        .flex_1()
        .min_h_0()
        .bg(rgb(theme::DAW_FADER_WELL_BG))
        .border_1()
        .border_color(rgb(0x1B1C20))
        .rounded(px(2.0))
        .px(px(3.0))
        .py(px(2.0))
        .gap(px(2.0))
        .overflow_hidden()
        // Cabecera interna monoespaciada: pico (rojo en clip) y dB del fader en naranja
        .child(
            h_flex()
                .w_full()
                .h(px(13.0))
                .flex_shrink_0()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(rgb(0x23252B))
                .pb(px(1.0))
                .overflow_hidden()
                .child(
                    Label::new(vu_text)
                        .text_size(px(7.0))
                        .font_family("monospace")
                        .when(vu_clip, |l| l.font_weight(FontWeight::BOLD))
                        .text_color(if vu_clip {
                            rgb(VU_CLIP_RED)
                        } else {
                            rgb(0x72767F)
                        }),
                )
                .child(
                    Label::new(vol_text.trim_end_matches(" dB").to_string())
                        .text_size(px(8.0))
                        .font_family("monospace")
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(theme::DAW_ACCENT_ORANGE)),
                ),
        )
        // Cuerpo inferior estirado de arriba a abajo: escala dB + VU dual + Fader
        .child(
            h_flex()
                .w_full()
                .flex_1()
                .min_h_0()
                .items_stretch()
                .justify_between()
                .gap(px(2.0))
                .overflow_hidden()
                // Escala de dB (0, 6, 12, 24, 36, ∞)
                .child(render_calibrated_scale())
                // Medidor VU estéreo vertical
                .child(vertical_vu_meter(peak_level))
                // Fader vertical de volumen ancho
                .child(
                    div()
                        .id(SharedString::from(format!("bfader_{:?}", target)))
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .cursor_ns_resize()
                        .on_mouse_down(gpui_kit::MouseButton::Left, move |event, _, cx| {
                            if event.click_count >= 2 {
                                arranger_view::set_volume(cx, target, 0.75);
                                return;
                            }
                            let b = f_down.get();
                            let travel = (b[3] - 12.0).max(1.0);
                            let new_vol = (1.0 - ((event.position.y.as_f32() - b[1] - 6.0) / travel)).clamp(0.0, 1.0);
                            arranger_view::set_volume(cx, target, new_vol);
                        })
                        .on_mouse_move(move |event, _, cx| {
                            if event.pressed_button != Some(gpui_kit::MouseButton::Left) {
                                crate::app::set_footer_cues(
                                    cx,
                                    Some(event.modifiers),
                                    &[
                                        ("DRAG", "Adjust volume"),
                                        ("DOUBLE-CLICK", "Reset to 0.0 dB"),
                                    ],
                                );
                                return;
                            }
                            let b = f_move.get();
                            let travel = (b[3] - 12.0).max(1.0);
                            let new_vol = (1.0 - ((event.position.y.as_f32() - b[1] - 6.0) / travel)).clamp(0.0, 1.0);
                            arranger_view::set_volume(cx, target, new_vol);
                        })
                        .child(
                            canvas(
                                move |bounds, _, _| {
                                    f_paint.set([
                                        bounds.origin.x.as_f32(),
                                        bounds.origin.y.as_f32(),
                                        bounds.size.width.as_f32(),
                                        bounds.size.height.as_f32(),
                                    ]);
                                },
                                move |bounds, _, window, _| {
                                    let total_w = bounds.size.width.as_f32();
                                    let total_h = bounds.size.height.as_f32();
                                    if total_h <= 10.0 {
                                        return;
                                    }
                                    let thumb_h = 10.0;
                                    let travel = (total_h - thumb_h).max(1.0);
                                    let thumb_y = thumb_h / 2.0 + (1.0 - volume.clamp(0.0, 1.0)) * travel;
                                    let fill_h = (total_h - 2.0 - thumb_y).max(0.0);

                                    // 1. Riel oscuro
                                    window.paint_quad(PaintQuad {
                                        bounds,
                                        background: rgb(0x101113).into(),
                                        border_color: rgb(0x23252A).into(),
                                        corner_radii: gpui_kit::Corners::all(px(1.5)),
                                        border_widths: gpui_kit::Edges::all(px(1.0)),
                                        border_style: BorderStyle::default(),
                                    });

                                    // 2. Marca de 0.0 dB (unity = 0.75)
                                    let unity_y = bounds.origin.y + px(thumb_h / 2.0 + (1.0 - 0.75) * travel);
                                    window.paint_quad(PaintQuad {
                                        bounds: Bounds::new(
                                            point(bounds.origin.x + px(1.0), unity_y),
                                            size(px(total_w - 2.0), px(1.0)),
                                        ),
                                        background: rgb(0x353840).into(),
                                        border_color: Hsla::default(),
                                        corner_radii: gpui_kit::Corners::default(),
                                        border_widths: gpui_kit::Edges::default(),
                                        border_style: BorderStyle::default(),
                                    });

                                    // 3. Columna ámbar de ganancia
                                    if fill_h > 0.5 {
                                        window.paint_quad(PaintQuad {
                                            bounds: Bounds::new(
                                                point(bounds.origin.x + px(1.0), bounds.origin.y + px(thumb_y)),
                                                size(px(total_w - 2.0), px(fill_h)),
                                            ),
                                            background: rgb(theme::DAW_FADER_FILL).into(),
                                            border_color: Hsla::default(),
                                            corner_radii: gpui_kit::Corners::default(),
                                            border_widths: gpui_kit::Edges::default(),
                                            border_style: BorderStyle::default(),
                                        });
                                    }

                                    // 4. Capuchón del fader con acento de color
                                    let cap_y = bounds.origin.y + px((thumb_y - 2.0).clamp(0.0, total_h - 4.0));
                                    window.paint_quad(PaintQuad {
                                        bounds: Bounds::new(
                                            point(bounds.origin.x, cap_y),
                                            size(px(total_w), px(4.0)),
                                        ),
                                        background: rgb(accent_color).into(),
                                        border_color: rgb(0xFFFFFF).into(),
                                        corner_radii: gpui_kit::Corners::all(px(1.0)),
                                        border_widths: gpui_kit::Edges::all(px(0.5)),
                                        border_style: BorderStyle::default(),
                                    });
                                },
                            )
                            .size_full(),
                        ),
                )
        )
        .into_any_element()
}

/// Escala vertical calibrada de dB (0, 6, 12, 18, 24, 30, 36, ∞).
///
/// Recortada en alto y ancho (`min_h_0` + `overflow_hidden` en contenedor y
/// marcas): con el Mix Panel bajo, las marcas se comprimen y `36` / `∞`
/// nunca bajan del margen inferior de la tarjeta del canal.
fn render_calibrated_scale() -> AnyElement {
    v_flex()
        .h_full()
        .min_h_0()
        .w(px(13.0))
        .flex_shrink_0()
        .justify_between()
        .items_end()
        .py(px(1.0))
        .overflow_hidden()
        .children(
            ["0", "6", "12", "18", "24", "30", "36", "∞"]
                .into_iter()
                .map(|val| {
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .items_center()
                        .justify_end()
                        .overflow_hidden()
                        .child(
                            Label::new(val)
                                .text_size(px(6.5))
                                .line_height(px(7.0))
                                .font_family("monospace")
                                .text_color(rgb(0x72767F)),
                        )
                        .into_any_element()
                }),
        )
        .into_any_element()
}

/// Botón [+] para añadir una pista al Clip Launcher / Mix Panel.
fn render_add_track_button() -> AnyElement {
    div()
        .id("btn_bottom_add_track")
        .w(px(24.0))
        .h(px(36.0))
        .flex_shrink_0()
        .rounded(px(2.0))
        .bg(rgb(0x232427))
        .border_1()
        .border_color(rgb(0x2E3036))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(0x2E3036)))
        .flex()
        .items_center()
        .justify_center()
        .on_mouse_move(|e, _, cx| {
            set_footer_cues(cx, Some(e.modifiers), &[("CLICK", "Add new track")]);
        })
        .on_click(|_, _, cx| {
            update_state(cx, |s| {
                s.matrix_state.add_track();
            });
        })
        .child(
            Label::new("+")
                .text_size(px(12.0))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0x9498A0)),
        )
        .into_any_element()
}

/// Canal de retorno FX 1 fijado a la derecha en el Mix Panel.
fn render_fx_strip(
    volume: f32,
    pan: f32,
    muted: bool,
    soloed: bool,
    peak_level: f32,
    devices: Vec<MixerDeviceChipData>,
    route_dest: String,
    is_tall: bool,
    show_devices: bool,
    show_sends: bool,
    show_io: bool,
) -> AnyElement {
    let target = StripTarget::Fx(0);
    let vol_target = FaderTarget::FxVolume(0);
    let pan_target = FaderTarget::FxPan(0);
    let slate_hex = 0x88909A;

    v_flex()
        .id("bottom_strip_fx_0")
        .w(px(BOTTOM_MIXER_TRACK_WIDTH))
        .flex_shrink_0()
        .h_full()
        .bg(rgb(0x232427))
        .border_1()
        .border_color(rgb(0x1B1C1F))
        .rounded(px(2.0))
        .overflow_hidden()
        // Drop target general para el canal FX 1
        .can_drop(|payload: &dyn std::any::Any, _, _| payload.is::<MixerDeviceDrag>())
        .on_drop(move |payload: &MixerDeviceDrag, _, cx| {
            update_state(cx, |s| {
                s.move_device(payload.source_target, payload.source_slot, target, usize::MAX);
            });
        })
        // Header FX 1
        .child(
            v_flex()
                .w_full()
                .h(px(36.0))
                .flex_shrink_0()
                .bg(rgb(slate_hex))
                .justify_center()
                .items_center()
                .cursor_pointer()
                .on_mouse_move(|e, _, cx| {
                    set_footer_cues(
                        cx,
                        Some(e.modifiers),
                        &[("CLICK", "Select FX Return track"), ("DOUBLE-CLICK", "Show devices")],
                    );
                })
                .on_mouse_down(gpui_kit::MouseButton::Left, move |event, _, cx| {
                    if event.click_count >= 2 {
                        update_state(cx, |s| {
                            s.show_dsp_rack = true;
                        });
                    }
                    arranger_view::select_track_column(cx, target);
                })
                .child(
                    Label::new("FX 1")
                        .text_size(px(10.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0x141414)),
                ),
        )
        // Devices
        .when(is_tall && show_devices, |this| {
            this.child(render_strip_devices(devices, target))
        })
        // Sends (vacío en FX)
        .when(is_tall && show_sends, |this| {
            this.child(
                div()
                    .w_full()
                    .h(px(34.0))
                    .flex_shrink_0()
                    .bg(rgb(0x222326))
                    .border_b_1()
                    .border_color(rgb(0x28292E)),
            )
        })
        // I/O
        .when(is_tall && show_io, |this| {
            this.child(render_strip_io(target, route_dest))
        })
        // Controles S/M + Pan + Fader & VU
        .child(
            v_flex()
                .w_full()
                .flex_1()
                .min_h_0()
                .p(px(2.0))
                .gap(px(2.0))
                .overflow_hidden()
                .child(render_smr_buttons(target, false, soloed, muted))
                .child(render_pan_slider(pan_target, pan))
                .child(render_fader_vu_well(vol_target, volume, peak_level, slate_hex))
        )
        // Acento inferior
        .child(
            div()
                .w_full()
                .h(px(3.0))
                .flex_shrink_0()
                .bg(rgb(0x606570)),
        )
        .into_any_element()
}

/// Canal Master fijado a la derecha en el Mix Panel.
fn render_master_strip(
    volume: f32,
    pan: f32,
    muted: bool,
    soloed: bool,
    peak_level: f32,
    devices: Vec<MixerDeviceChipData>,
    route_dest: String,
    is_tall: bool,
    show_devices: bool,
    show_sends: bool,
    show_io: bool,
) -> AnyElement {
    let target = StripTarget::Master;
    let vol_target = FaderTarget::MasterVolume;
    let pan_target = FaderTarget::MasterPan;
    let master_cap = theme::DAW_FADER_CAP_ORANGE;

    v_flex()
        .id("bottom_strip_master")
        .w(px(BOTTOM_MIXER_TRACK_WIDTH))
        .flex_shrink_0()
        .h_full()
        .bg(rgb(0x232427))
        .border_1()
        .border_color(rgb(0x1B1C1F))
        .rounded(px(2.0))
        .overflow_hidden()
        // Drop target general para el canal Master
        .can_drop(|payload: &dyn std::any::Any, _, _| payload.is::<MixerDeviceDrag>())
        .on_drop(move |payload: &MixerDeviceDrag, _, cx| {
            update_state(cx, |s| {
                s.move_device(payload.source_target, payload.source_slot, target, usize::MAX);
            });
        })
        // Header Master
        .child(
            v_flex()
                .w_full()
                .h(px(36.0))
                .flex_shrink_0()
                .bg(rgb(0x555A64))
                .justify_center()
                .items_center()
                .cursor_pointer()
                .on_mouse_move(|e, _, cx| {
                    set_footer_cues(
                        cx,
                        Some(e.modifiers),
                        &[("CLICK", "Select Master track"), ("DOUBLE-CLICK", "Show devices")],
                    );
                })
                .on_mouse_down(gpui_kit::MouseButton::Left, move |event, _, cx| {
                    if event.click_count >= 2 {
                        update_state(cx, |s| {
                            s.show_dsp_rack = true;
                        });
                    }
                    arranger_view::select_track_column(cx, target);
                })
                .child(
                    Label::new("Master")
                        .text_size(px(10.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0x141414)),
                ),
        )
        // Devices
        .when(is_tall && show_devices, |this| {
            this.child(render_strip_devices(devices, target))
        })
        // Sends (vacío en Master)
        .when(is_tall && show_sends, |this| {
            this.child(
                div()
                    .w_full()
                    .h(px(34.0))
                    .flex_shrink_0()
                    .bg(rgb(0x222326))
                    .border_b_1()
                    .border_color(rgb(0x28292E)),
            )
        })
        // I/O
        .when(is_tall && show_io, |this| {
            this.child(render_strip_io(target, route_dest))
        })
        // Controles S/M + Pan + Fader & VU
        .child(
            v_flex()
                .w_full()
                .flex_1()
                .min_h_0()
                .p(px(2.0))
                .gap(px(2.0))
                .overflow_hidden()
                .child(render_smr_buttons(target, false, soloed, muted))
                .child(render_pan_slider(pan_target, pan))
                .child(render_fader_vu_well(vol_target, volume, peak_level, master_cap))
        )
        // Acento inferior Master
        .child(
            div()
                .w_full()
                .h(px(3.0))
                .flex_shrink_0()
                .bg(rgb(theme::DAW_ACCENT_ORANGE)),
        )
        .into_any_element()
}

#[derive(Clone, Debug)]
pub struct DspSlot {
    pub id: usize,
    pub name: String,
    /// Si el slot pasa audio.
    pub active: bool,
    /// Si el editor extendido está desplegado en el rack.
    pub is_open: bool,
    /// Estado del sintetizador de Wavetable. Vive anidado y no plano en el slot
    /// porque el editor tiene demasiado estado (osciladores, moduladores,
    /// cámara, envolvente, filtro, FX) como para que una lista plana siga
    /// siendo legible.
    pub wavetable: WavetableEditor,
    pub dms_state: Option<OpenDms>,
    pub menu_open: bool,
    pub options_open: bool,
    /// Metadata del plugin VST3/CLAP cargado en este slot.
    ///
    /// Va aparte de `name` a propósito. `name` es lo que se muestra y se
    /// guarda en el proyecto, y alcanza para un plugin interno
    /// ("OpenWavetable"). Para uno externo hace falta saber **qué binario**
    /// es, porque dos plugins distintos pueden llamarse igual y porque hay que
    /// recargar la ruta si el usuario movió el plugin. `None` = slot vacío o
    /// plugin interno.
    pub external: Option<hikaru_plugin_host::PluginDescriptor>,
    /// Error de la última carga, para mostrarlo en la tarjeta sin romper nada.
    ///
    /// El objetivo es que un plugin roto se vea marcado en el rack y no se
    /// lleve por delante la app: la carga devuelve `Result`, y este campo es
    /// dónde aterriza el mensaje.
    pub load_error: Option<String>,
    /// Caché de parámetros del plugin externo (knobs del rack).
    ///
    /// Se llena en diferido: asignar sólo guarda el descriptor, y un sondeo
    /// en hilo aparte instancia el plugin, lee `snapshot_params` y lo vuelca
    /// acá vía `AppState::drain_param_probes`. `value` lo escriben los gestos
    /// del rack y el feedback del editor nativo.
    pub external_params: Vec<ExternalParamState>,
    /// Cambios de parámetro que el motor todavía no recibió.
    ///
    /// Es una cola y no una bandera por parámetro porque el editor nativo
    /// suele tener **más** controles que el rack (un sintetizador con 50
    /// controles y 29 en el rack). Los que no están en el rack no tienen
    /// dónde anotarse, así que también van acá: sin ellos, mover un control
    /// en el editor que el rack no muestra no cambiaría el audio.
    ///
    /// Un mismo parámetro no aparece dos veces: al encolar se reemplaza el
    /// valor anterior, que ya quedó obsoleto.
    pending_external_params: Vec<(u32, f32)>,
    /// En qué punto está el sondeo de parámetros (ver `AppState`).
    pub params_status: ExternalParamsStatus,
    /// Si el plugin externo de este slot declara editor gráfico.
    ///
    /// Vive acá y no en `descriptor.has_gui` porque el escaneo de disco no
    /// instancia nada (a propósito: ejecutar el `libmain` de un binario
    /// desconocido durante un escaneo cuelga) y por lo tanto deja ese campo en
    /// `false` siempre. El sondeo en diferido sí crea una instancia real y
    /// deja la respuesta honesta acá.
    ///
    /// `None` = todavía no seINSTANCEó (o es un plugin interno).
    pub has_gui: Option<bool>,
}

use crate::views::hikaru_native_plugins::hikaru_opendms::OpenDms;
use crate::views::hikaru_native_plugins::hikaru_openwavetable::WavetableEditor;

impl DspSlot {
    pub fn new(id: usize, name: String) -> Self {
        Self {
            id,
            name,
            active: true,
            is_open: false,
            wavetable: WavetableEditor::new(),
            dms_state: None,
            menu_open: false,
            options_open: false,
            external: None,
            load_error: None,
            external_params: Vec::new(),
            pending_external_params: Vec::new(),
            params_status: ExternalParamsStatus::Idle,
            has_gui: None,
        }
    }

    /// ¿Es un slot que tiene un editor extendido detrás?
    ///
    /// La decisión de qué panel mostrar la toma el rack, y tiene que estar de
    /// acuerdo con el nombre del plugin: un slot vacío no tiene editor, así que
    /// seleccionarlo no puede abrir un panel en blanco.
    pub fn has_editor(&self) -> bool {
        // Un plugin externo cuenta si declara editor; sin `has_gui` no hay
        // ventana que abrir y no tiene sentido ofrecer el botón.
        if self.external.is_some() {
            return self.external.as_ref().is_some_and(|p| p.has_gui);
        }
        matches!(self.name.as_str(), "OpenWavetable" | "Hikaru OpenDMS")
    }

    /// ¿Es un slot sin nada cargado?
    ///
    /// El nombre por sí solo no alcanza: un proyecto puede traer un slot con
    /// `name = "FabFilter Pro-Q 3"` y el plugin que falta en disco. Eso se ve
    /// como slot vacío con un error, no como un plugin cargado.
    pub fn is_empty_slot(&self) -> bool {
        self.external.is_none() && self.name == EMPTY_SLOT_NAME
    }

    /// Cambios de parámetros que el motor aún no aplicó a su instancia de
    /// audio: `(id, normalizado)`.
    ///
    /// Los producen los knobs del rack **y** el feedback del editor nativo
    /// (el usuario movió un control dentro de la ventana del plugin): el
    /// hilo de control los drena y los manda con
    /// `hikaru_plugin_host::apply_params_to_instance` (o
    /// `DspPluginNode::sync_params`).
    ///
    /// Drenar limpia la cola: llamar dos veces seguidas sin gestos en medio
    /// devuelve vacío la segunda.
    pub fn take_pending_external_params(&mut self) -> Vec<(u32, f32)> {
        std::mem::take(&mut self.pending_external_params)
    }

    /// Encola un cambio de parámetro para el motor.
    ///
    /// Un mismo parámetro no se encola dos veces: el valor nuevo reemplaza al
    /// viejo, que ya quedó obsoleto. Es lo que permite que un arrastre que
    /// emite un cambio por píxel no amontone la cola.
    pub fn enqueue_external_param(&mut self, id: u32, value: f32) {
        if let Some(entry) = self
            .pending_external_params
            .iter_mut()
            .find(|(existing, _)| *existing == id)
        {
            entry.1 = value;
        } else {
            self.pending_external_params.push((id, value));
        }
    }

    /// Fija un parámetro desde la UI: actualiza la caché y lo encola para el
    /// motor.
    ///
    /// Devuelve el valor **efectivamente aplicado** (ya redondeado por `snap`),
    /// o `None` si el id no existe en este slot **o si el valor ya era ese**.
    ///
    /// Lo segundo importa tanto como lo primero. El drag de un parámetro llega
    /// en un evento por píxel, y al pasar por `snap` la mayoría de los píxeles no
    /// mueven nada: en un parámetro discreto de 8 escalones, ~7 de cada 8
    /// eventos calculan el mismo valor. Si eso contara como "cambió", cada uno de
    /// esos eventos pagaría un `cx.notify()` —que reconstruye la ventana entera,
    /// con el clonado de `track.effects` incluido— y el fader se sentiría
    /// pegajoso. Por eso el llamador puede distinguir "no encontré el id" de
    /// "no pasó nada" y saltarse el trabajo.
    pub fn set_external_param(&mut self, id: u32, value: f32) -> Option<f32> {
        let param = self.external_params.iter_mut().find(|p| p.info.id == id)?;
        let snapped = param.info.snap(value);
        if (param.value - snapped).abs() <= f32::EPSILON {
            return None;
        }
        param.value = snapped;
        self.enqueue_external_param(id, snapped);
        Some(snapped)
    }
}

/// Nombre que el rack usa para un slot sin plugin. Un `const` y no un literal
/// repetido, porque `is_empty_slot` compara contra él y un typo en cualquiera
/// de los dos lados rompería la comparación en silencio.
pub const EMPTY_SLOT_NAME: &str = "Empty Slot";

/// Un parámetro externo cacheado para los knobs del rack.
///
/// La `info` (rango, nombre, escalones) viene del sondeo y no cambia; `value`
/// es la única parte viva.
#[derive(Clone, Debug)]
pub struct ExternalParamState {
    pub info: hikaru_plugin_host::PluginParamInfo,
    pub value: f32,
}

impl ExternalParamState {
    pub fn display(&self) -> String {
        self.info.display_value(self.value)
    }
}

/// Estado del sondeo de parámetros de un slot externo.
///
/// `Idle` = asignado pero aún no sondeado (el rack pide el sondeo en el
/// primer frame); `Probing` = hilo aparte instanciando el plugin;
/// `Ready` = lista usable (quizá vacía: el plugin no expone nada);
/// `Failed` = el sondeo falló con mensaje para la tarjeta.
#[derive(Clone, Debug, Default)]
pub enum ExternalParamsStatus {
    #[default]
    Idle,
    Probing,
    Ready,
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct SendConnection {
    pub target_id: usize,
    pub amount: f32,
}

#[derive(Clone, Debug)]
pub struct Track {
    pub id: usize,
    pub name: String,
    pub volume: f32,
    pub pan: f32,
    pub pan_mode: crate::app::PanMode,
    pub mute: bool,
    pub solo: bool,
    /// Armado de grabación ([R] del channel strip).
    pub arm: bool,
    pub is_master: bool,
    pub route_destination_id: usize,
    pub sends: Vec<SendConnection>,
    pub effects: Vec<DspSlot>,
    pub matrix_idx: Option<usize>,
    pub width: f32,
}

impl Track {
    pub fn new(id: usize, name: String, is_master: bool) -> Self {
        Self {
            id,
            name,
            volume: 0.75,
            pan: 0.0,
            pan_mode: crate::app::PanMode::Stereo,
            mute: false,
            solo: false,
            arm: false,
            is_master,
            route_destination_id: 0,
            sends: Vec::new(),
            effects: Vec::new(),
            matrix_idx: None,
            width: arranger_view::DEFAULT_TRACK_WIDTH,
        }
    }
}

/// Clona la cadena de DEVICES de una pista para compartirla con otro canal.
///
/// Los slots son valores `Clone` (sin aliasing): la copia es transparente
/// entre canales de OpenLive y OpenStudio porque ambos trabajan sobre el
/// mismo tipo [`Track`]. El canal destino procesa sus clips con la cadena
/// recibida sin más pasos.
pub fn duplicate_device_chain(slots: &[DspSlot]) -> Vec<DspSlot> {
    slots.to_vec()
}

// =========================================================================
// CURVAS DE dB
// =========================================================================

/// Texto del fader: mapea 0..=1 a -60..+6 dB con el 0.75 como 0 dB.
pub fn db_text(volume: f32) -> String {
    let db_val = if volume <= 0.0 {
        -60.0
    } else if volume <= 0.75 {
        -60.0 + (volume / 0.75) * 60.0
    } else {
        ((volume - 0.75) / 0.25) * 6.0
    };
    format!("{:.1} dB", db_val)
}

/// Texto del nivel de señal del VU: 20*log10 con suelo en -60 dB.
pub fn level_db_text(level: f32) -> String {
    let db_val = if level <= 0.0 {
        -60.0
    } else {
        (20.0 * level.log10()).clamp(-60.0, 6.0)
    };
    format!("{:.1}dB", db_val)
}

/// Lectura de pico estilo Bitwig 5.1.7 para la barra superior del pozo.
///
/// Devuelve el texto monoespaciado (`-12.4`, `+2.4`, `-inf`) y si la señal
/// entró en zona de clip (> 0.0 dBFS o tramo rojo del VU, `level > 0.9`),
/// para que la vista la pinte en rojo `#FF3B30`.
pub fn peak_db_readout(level: f32) -> (String, bool) {
    let level = if level.is_finite() { level.max(0.0) } else { 0.0 };
    if level <= 0.001 {
        return ("-inf".to_string(), false);
    }
    let db = (20.0 * level.log10()).clamp(-60.0, 12.0);
    let clip = db > 0.0 || level > 0.9;
    let text = if db > 0.0 {
        format!("+{:.1}", db)
    } else {
        format!("{:.1}", db)
    };
    (text, clip)
}

/// Rojo de clip compartido por la lectura de pico y el tramo superior del VU.
pub const VU_CLIP_RED: u32 = 0xFF3B30;
/// Ámbar de la marca de pico instantáneo del VU.
pub const VU_PEAK_AMBER: u32 = 0xFF9E2C;

// =========================================================================
// ENRUTAMIENTO I/O (etiquetas y opciones del selector de salida)
// =========================================================================
//
// `Track::route_destination_id` es estado de GUI (el motor todavía suma todo
// al master). Convención de ids:
//   0              -> Master (o salida de hardware si la pista ES el master)
//   1..ROUTE_FX_BASE -> índice de otra pista del mismo modo (live/studio)
//   ROUTE_FX_BASE+n -> pista de retorno FX n+1

/// Base de ids para las pistas de retorno (`FX 1` = `ROUTE_FX_BASE`).
pub const ROUTE_FX_BASE: usize = 1000;

/// Etiqueta legible de un destino de ruteo.
pub fn route_dest_label(id: usize, is_master: bool, track_names: &[String]) -> String {
    if is_master {
        return "Out 1/2".to_string();
    }
    if id == 0 {
        return "Master".to_string();
    }
    if id >= ROUTE_FX_BASE {
        return format!("FX {}", id - ROUTE_FX_BASE + 1);
    }
    track_names
        .get(id)
        .cloned()
        .unwrap_or_else(|| format!("Track {}", id))
}

/// Opciones del selector de salida para `target`, en el orden de Bitwig:
/// Master, retornos FX y luego el resto de pistas (sin la propia).
pub fn route_options(app: &crate::app::AppState, target: StripTarget) -> Vec<(usize, String)> {
    if target == StripTarget::Master {
        return vec![(0, "Out 1/2".to_string())];
    }
    let mut out = vec![(0, "Master".to_string())];
    let fx_count = app.fx_tracks.len().max(1);
    if !matches!(target, StripTarget::Fx(_)) {
        for n in 0..fx_count {
            out.push((ROUTE_FX_BASE + n, format!("FX {}", n + 1)));
        }
    }
    let (tracks, own): (&Vec<Track>, Option<usize>) = match target {
        StripTarget::Live(idx) => (&app.live_tracks, Some(idx + 1)),
        StripTarget::Studio(idx) => (&app.studio_tracks, Some(idx)),
        _ => (app.tracks(), None),
    };
    for (i, t) in tracks.iter().enumerate().skip(1) {
        if Some(i) == own || t.is_master {
            continue;
        }
        out.push((i, t.name.clone()));
    }
    out
}

// =========================================================================
// VU METER VERTICAL (renderizado directo, sin relayout)
// =========================================================================
//
// El medidor es un `canvas` de tamaño FIJO (`VU_WIDTH` x `VU_HEIGHT`): el nivel
// sólo cambia lo que se pinta (quads por textura), nunca el tamaño ni los
// hijos del layout. Así el DOM/GUI no se re-mide a 60 FPS aunque el pico se
// actualice en cada frame. El suavizado ataque/liberación se aplica una vez
// por frame en `AppState::sync_frame` (`smoothed_track_peaks`), no acá.

/// Ancho fijo del VU según blueprint (columna estrecha a la izquierda).
pub const VU_WIDTH: f32 = 14.0;
/// Alto fijo del VU: idéntico al recorrido del fader para lectura paralela.
/// 84px calibrado para dar 12px por cada una de las 7 marcas de dB (0..∞).
pub const VU_HEIGHT: f32 = 84.0;

/// Color del relleno según tramo: verde -> verde-amarillo -> rojo de clip.
pub fn vu_fill_color(level: f32) -> Hsla {
    if level > 0.9 {
        rgb(VU_CLIP_RED).into()
    } else if level > 0.75 {
        rgb(0xC8E62C).into()
    } else {
        rgb(0x2CE64B).into()
    }
}

/// Suavizado ataque rápido / liberación lenta para los picos atómicos.
///
/// `previous` es el valor ya suavizado del frame anterior y `target` el pico
/// crudo del motor. Sin esto el canvas parpadearía entre 0 y 1 siguiendo el
/// buffer de audio en vez de mostrar una aguja legible.
pub fn smooth_peak(previous: f32, target: f32) -> f32 {
    let prev = if previous.is_finite() { previous } else { 0.0 };
    let tgt = if target.is_finite() {
        target.clamp(0.0, 1.0)
    } else {
        0.0
    };
    if tgt > prev {
        prev + (tgt - prev) * 0.6
    } else {
        prev + (tgt - prev) * 0.12
    }
}

/// VU meter vertical de tamaño fijo pintado directo en canvas.
///
/// Ni el `div` ni el `canvas` cambian de tamaño o hijos con `level`: todo el
/// movimiento ocurre dentro del callback de pintado.
///
/// Diseño Bitwig 5.1.7: dos barras estéreo L/R (5.5px) separadas por un
/// divisor de 1px, tramos segmentados verde (`#2CE64B`, hasta 0.75 = 0 dB),
/// verde-amarillo (`#C8E62C`, 0.75..0.90) y rojo de clip (`#FF3B30`,
/// 0.90..1.0), más la marca ámbar de pico. Todo queda dentro de un
/// contenedor `overflow_hidden`, y cada quad se recorta a los bounds del
/// canvas, así la señal nunca desborda el margen inferior de la tarjeta.
pub fn vertical_vu_meter(level: f32) -> AnyElement {
    let level = if level.is_finite() {
        level.clamp(0.0, 1.0)
    } else {
        0.0
    };
    div()
        .w(px(VU_WIDTH))
        .h_full()
        .min_h_0()
        .flex_shrink_0()
        .overflow_hidden()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let quad = |window: &mut Window, x: f32, y: f32, w: f32, h: f32, col: u32| {
                        if w <= 0.0 || h <= 0.0 {
                            return;
                        }
                        window.paint_quad(PaintQuad {
                            bounds: Bounds::new(point(px(x), px(y)), size(px(w), px(h))),
                            background: rgb(col).into(),
                            border_color: Hsla::default(),
                            corner_radii: gpui_kit::Corners::default(),
                            border_widths: gpui_kit::Edges::default(),
                            border_style: BorderStyle::default(),
                        });
                    };
                    let ox = bounds.origin.x.as_f32();
                    let oy = bounds.origin.y.as_f32();
                    let fw = bounds.size.width.as_f32();
                    let fh = bounds.size.height.as_f32();
                    if fw <= 2.0 || fh <= 2.0 {
                        return;
                    }
                    // Fondo empotrado: siempre el mismo quad, mismo costo.
                    quad(window, ox, oy, fw, fh, 0x0A0A0B);
                    let bar_w = ((fw - 1.0) / 2.0).max(1.0);
                    // Divisor central L/R.
                    quad(window, ox + bar_w, oy, 1.0, fh, 0x1E2024);
                    // Marca sutil de 0 dB (0.75) en el pozo.
                    let unity_y = oy + fh * (1.0 - 0.75);
                    quad(window, ox, unity_y, fw, 1.0, 0x2A2D33);
                    if level <= 0.001 {
                        return;
                    }
                    // Leve desfasaje R para que el estéreo se lea vivo sin
                    // un segundo pico del motor (señal mono-sumada).
                    let levels = [level, (level * 0.97).min(1.0)];
                    let zones: [(f32, f32, u32); 3] = [
                        (0.0, 0.75, 0x2CE64B),
                        (0.75, 0.90, 0xC8E62C),
                        (0.90, 1.0, VU_CLIP_RED),
                    ];
                    for (ch, lv) in levels.iter().enumerate() {
                        let bx = ox + ch as f32 * (bar_w + 1.0);
                        for (lo, hi, col) in zones {
                            if *lv <= lo {
                                break;
                            }
                            let top = lv.min(hi);
                            let y0 = oy + fh * (1.0 - top);
                            let y1 = oy + fh * (1.0 - lo);
                            quad(window, bx, y0, bar_w, (y1 - y0).max(0.0), col);
                        }
                        // Marca de pico ámbar, recortada dentro del pozo.
                        let peak_y = (oy + fh * (1.0 - lv)).clamp(oy, oy + fh - 1.5);
                        quad(window, bx, peak_y, bar_w, 1.5, VU_PEAK_AMBER);
                    }
                },
            )
            .w(px(VU_WIDTH))
            .h_full(),
        )
        .into_any_element()
}
