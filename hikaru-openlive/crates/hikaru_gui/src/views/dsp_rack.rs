// crates/hikaru_gui/src/views/dsp_rack.rs

use gpui_kit::component::*;
use gpui_kit::component::button::{Button, ButtonRounded};
use gpui_kit::component::input::Input;
use gpui_kit::component::label::Label;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::Size;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, HikaruApp, ParamDragState};
use crate::ui::text_style_scope::TextStyleScope;
use crate::views::external_plugins_settings::capture_host_parent;
use crate::views::mixer::{DspSlot, ExternalParamsStatus};
use crate::views::open_wavetable;

pub const RACK_HEIGHT: f32 = 340.0;

pub const CARD_WIDTH: f32 = 388.0;

const CARD_PADDING: f32 = 6.0;

const CARD_GAP: f32 = 6.0;

const CARD_HEADER_HEIGHT: f32 = 24.0;

const CARD_FOOTER_HEIGHT: f32 = 50.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginEntry {
    pub category: &'static str,
    pub name: &'static str,
    pub label: &'static str,
}

pub const PLUGIN_CATALOG: [PluginEntry; 3] = [
    PluginEntry { category: "Efectos", name: "OpenEQ3", label: "OpenEQ3" },
    PluginEntry { category: "Generadores", name: "OpenWavetable", label: "Hikaru OpenWavetable" },
    PluginEntry { category: "Generadores", name: "Hikaru OpenDMS", label: "Hikaru OpenDMS" },
];

/// Filtro de formato del navegador de plugins del rack.
///
/// Vive en el estado global de la app (`AppState::plugin_browser_filter`)
/// porque el navegador se reconstruye en cada frame: un valor local no
/// sobreviviría al primer `notify` y el chip activo parpadearía.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PluginBrowserFilter {
    #[default]
    All,
    Native,
    Vst3,
    Clap,
}

impl PluginBrowserFilter {
    const TABS: [(Self, &'static str); 4] = [
        (Self::All, "Todos"),
        (Self::Native, "Nativos"),
        (Self::Vst3, "VST3"),
        (Self::Clap, "CLAP"),
    ];

    fn matches_external(self, format: hikaru_plugin_host::PluginFormat) -> bool {
        match self {
            Self::All => true,
            Self::Native => false,
            Self::Vst3 => format == hikaru_plugin_host::PluginFormat::VST3,
            Self::Clap => format == hikaru_plugin_host::PluginFormat::CLAP,
        }
    }

    fn show_natives(self) -> bool {
        matches!(self, Self::All | Self::Native)
    }
}

/// Cierra el navegador de plugins del track dado, sea cual sea el slot que
/// lo abrió. Lo llaman el backdrop, la ✕, la tecla de cierre y cada
/// asignación: un navegador abierto tras asignar apuntaría a un slot cuyo
/// contenido ya cambió.
fn close_browser(cx: &mut gpui_kit::App, track_idx: usize) {
    state(cx).update(cx, |state, cx| {
        if let Some(track) = state.tracks_mut().get_mut(track_idx) {
            for slot in track.effects.iter_mut() {
                slot.menu_open = false;
            }
        }
        state.add_slot_menu_open = false;
        cx.notify();
    });
}

fn rack_button(id: impl Into<gpui_kit::ElementId>) -> Button {
    Button::new(id)
        .rounded(ButtonRounded::None)
        .compact()
        .with_size(gpui_kit::component::Size::XSmall)
}

fn assign_plugin(cx: &mut gpui_kit::App, track_idx: usize, slot_idx: usize, name: &str) {
    let st = state(cx);
    let name = name.to_string();
    cx.defer(move |cx| {
        st.update(cx, |state, cx| {
            if let Some(slot) = state.slot_mut(track_idx, slot_idx) {
                slot.name = name.clone();
                // Cambiar de plugin limpia cualquier error de carga anterior:
                // el mensaje era de otro plugin y ya no aplica.
                slot.load_error = None;
                slot.menu_open = false;
                slot.options_open = false;
            }
            state.selected_slot_index = slot_idx;
            state.add_slot_menu_open = false;
            cx.notify();
        });
    });
}

/// Asigna un plugin externo (VST3/CLAP) a un slot.
///
/// No carga el binario: guarda la metadata del descriptor. Instanciarlo en el
/// hilo de la GUI los dejaría haciendo audio fuera del hilo de audio (ver
/// `hikaru_plugin_host::PluginInstance::process`), así que la carga real la
/// hace el motor cuando este slot entre en la cadena de efectos. Esa
/// separación es la que permite que un plugin roto se marque en el slot en vez
/// de romper la apertura del proyecto.
fn assign_external_plugin(
    cx: &mut gpui_kit::App,
    track_idx: usize,
    slot_idx: usize,
    descriptor: hikaru_plugin_host::PluginDescriptor,
) {
    let st = state(cx);
    let audio_proxy = st.read(cx).audio_proxy.clone();
    cx.defer(move |cx| {
        st.update(cx, |state, cx| {
            if let Some(slot) = state.slot_mut(track_idx, slot_idx) {
                slot.name = descriptor.name.clone();
                slot.external = Some(descriptor.clone());
                slot.load_error = None;
                // Plugin nuevo = parámetros nuevos: se tira la caché vieja y
                // se sondea en diferido (hilo aparte, ver `request_external_params`).
                slot.external_params.clear();
                slot.params_status = ExternalParamsStatus::Idle;
                slot.menu_open = false;
                slot.options_open = false;
            }
            state.request_external_params(&descriptor);
            state.selected_slot_index = slot_idx;
            state.add_slot_menu_open = false;
            cx.notify();
        });
        // Cargar el plugin en el motor de audio (hilo de control del engine).
        // El `dlopen` + init corre en el engine, nunca en el hilo de la GUI.
        audio_proxy.send(crate::audio_proxy::GuiCommand::LoadExternalPlugin {
            track_idx,
            slot_idx,
            descriptor,
        });
    });
}

/// Abre el editor nativo del plugin en una ventana hija de la del DAW.
///
/// Captura el handle nativo de GPUI en el click (`window` sólo existe en el
/// handler, no en el render) y lo reduce a `HostParentKind`: en sesión X11
/// la ventana del plugin nace hija de la del DAW (transient, centrada, sin
/// flash vacía ni cierres fantasma); en Wayland cae al flotante de XWayland
/// porque una superficie Wayland no puede emparentar ventanas X11. Todo el
/// trabajo pesado (`dlopen`, X11, embebido) corre en el hilo de la ventana:
/// el hilo principal vuelve de inmediato y nunca se cuelga.
fn open_external_editor(
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
    track_idx: usize,
    slot_idx: usize,
) {
    let descriptor = {
        let app = state(cx).read(cx);
        app.slot(track_idx, slot_idx).and_then(|slot| slot.external.clone())
    };
    let Some(descriptor) = descriptor else {
        return;
    };
    let parent = capture_host_parent(window);

    state(cx).update(cx, |state, cx| {
        state.plugin_settings_state.open_plugin(&descriptor, parent);
        cx.notify();
    });
}

/// Cierra la ventana nativa del editor de un plugin por nombre.
///
/// El cierre va por nombre y no por slot porque `PluginSettingsState` indexa
/// las ventanas abiertas por nombre de plugin: es lo que permite que dos slots
/// con el mismo plugin compartan una sola ventana en vez de abrir dos.
fn close_external_editor(cx: &mut gpui_kit::App, plugin_name: &str) {
    let st = state(cx);
    st.update(cx, |state, _| {
        state.plugin_settings_state.close_plugin(plugin_name);
    });
}

/// Fija un parámetro externo desde un knob del rack.
///
/// Escribe la caché del slot (lo que pintan los knobs), la marca `dirty`
/// para el motor (`take_dirty_external_params`) y la manda en vivo a la
/// ventana abierta si la hay (el editor nativo la refleja al instante).
/// Todo no bloqueante: si la ventana cayó, el gesto igual queda guardado.
fn commit_external_param(
    cx: &mut gpui_kit::App,
    track_idx: usize,
    slot_idx: usize,
    id: u32,
    value: f32,
) {
    let st = state(cx);
    let audio_proxy = st.read(cx).audio_proxy.clone();
    st.update(cx, |state, cx| {
        let live: Option<(String, f32)> =
            if let Some(slot) = state.slot_mut(track_idx, slot_idx) {
                if !slot.set_external_param(id, value) {
                    None
                } else {
                    let current = slot
                        .external_params
                        .iter()
                        .find(|p| p.info.id == id)
                        .map(|p| p.value)
                        .unwrap_or(value);
                    slot.external
                        .as_ref()
                        .map(|ext| (ext.name.clone(), current))
                }
            } else {
                None
            };
        if let Some((plugin_name, current)) = live {
            state
                .plugin_settings_state
                .send_param_to_open_plugin(&plugin_name, id, current);
        }
        cx.notify();
    });
    // Enviar el cambio al motor de audio (lock-free hacia el hilo de audio).
    audio_proxy.send(crate::audio_proxy::GuiCommand::SetExternalPluginParam {
        track_idx,
        slot_idx,
        param_id: id,
        value,
    });
}

/// Empieza un drag vertical de knob: agarre en `start_y` (px de ventana).
fn param_drag_begin(
    cx: &mut gpui_kit::App,
    track_idx: usize,
    slot_idx: usize,
    id: u32,
    start_y: f32,
) {
    let value = state(cx)
        .read(cx)
        .slot(track_idx, slot_idx)
        .and_then(|slot| {
            slot.external_params
                .iter()
                .find(|p| p.info.id == id)
                .map(|p| p.value)
        })
        .unwrap_or(0.0);
    state(cx).update(cx, |state, cx| {
        state.param_drag = Some(ParamDragState {
            track: track_idx,
            slot: slot_idx,
            param_id: id,
            start_y,
            start_value: value,
        });
        cx.notify();
    });
}

/// Continúa el drag: subir = más (0.005 por px, gesto fino de DAW).
fn param_drag_move(cx: &mut gpui_kit::App, y: f32) {
    let drag = state(cx).read(cx).param_drag;
    let Some(drag) = drag else {
        return;
    };
    let value = (drag.start_value + (drag.start_y - y) * 0.005).clamp(0.0, 1.0);
    commit_external_param(cx, drag.track, drag.slot, drag.param_id, value);
}

fn param_drag_end(cx: &mut gpui_kit::App) {
    state(cx).update(cx, |state, cx| {
        state.param_drag = None;
        cx.notify();
    });
}

fn open_plugin_menu(cx: &mut gpui_kit::App, track_idx: usize, slot_idx: usize) {
    state(cx).update(cx, |state, cx| {
        if let Some(track) = state.tracks_mut().get_mut(track_idx) {
            for (i, s) in track.effects.iter_mut().enumerate() {
                if i == slot_idx {
                    s.menu_open = true;
                    s.options_open = false;
                } else {
                    s.menu_open = false;
                    s.options_open = false;
                }
            }
        }
        state.selected_slot_index = slot_idx;
        state.add_slot_menu_open = false;
        cx.notify();
    });
}

fn cycle_card_menu(cx: &mut gpui_kit::App, track_idx: usize, slot_idx: usize) {
    let options_open = state(cx)
        .read(cx)
        .slot(track_idx, slot_idx)
        .is_some_and(|slot| slot.options_open);
    if options_open {
        open_plugin_menu(cx, track_idx, slot_idx);
    } else {
        open_options_menu(cx, track_idx, slot_idx);
    }
}

fn open_options_menu(cx: &mut gpui_kit::App, track_idx: usize, slot_idx: usize) {
    state(cx).update(cx, |state, cx| {
        if let Some(track) = state.tracks_mut().get_mut(track_idx) {
            for (i, s) in track.effects.iter_mut().enumerate() {
                if i == slot_idx {
                    s.options_open = !s.options_open;
                    s.menu_open = false;
                } else {
                    s.options_open = false;
                    s.menu_open = false;
                }
            }
        }
        state.selected_slot_index = slot_idx;
        state.add_slot_menu_open = false;
        cx.notify();
    });
}

fn remove_slot_at(cx: &mut gpui_kit::App, track_idx: usize, slot_idx: usize) {
    state(cx).update(cx, |state, cx| {
        let selected = state.selected_slot_index;
        let new_len = if let Some(track) = state.tracks_mut().get_mut(track_idx) {
            if slot_idx < track.effects.len() {
                track.effects.remove(slot_idx);
            }
            track.effects.len()
        } else {
            0
        };
        state.selected_slot_index = if new_len == 0 {
            0
        } else {
            selected.min(new_len - 1)
        };
        cx.notify();
    });
}

fn remove_selected_slot(cx: &mut gpui_kit::App, track_idx: usize) {
    state(cx).update(cx, |state, cx| {
        let selected = state.selected_slot_index;
        if let Some(track) = state.tracks_mut().get_mut(track_idx) {
            if selected < track.effects.len() {
                track.effects.remove(selected);
                state.selected_slot_index = if track.effects.is_empty() {
                    0
                } else {
                    selected.min(track.effects.len() - 1)
                };
            }
        }
        cx.notify();
    });
}

fn insert_slot_at(cx: &mut gpui_kit::App, track_idx: usize, at_idx: usize) {
    state(cx).update(cx, |state, cx| {
        if let Some(track) = state.tracks_mut().get_mut(track_idx) {
            let new_id = track.effects.len();
            let insert_pos = at_idx.min(track.effects.len());
            track.effects.insert(insert_pos, DspSlot::new(new_id, "Empty Slot".to_string()));
            crate::views::open_wavetable::log_initial_wavetable(&format!("slot t{track_idx}s{new_id}"));
            state.selected_slot_index = insert_pos;
        }
        cx.notify();
    });
}

fn move_slot(cx: &mut gpui_kit::App, track_idx: usize, from: usize, to: usize) {
    state(cx).update(cx, |state, cx| {
        if let Some(track) = state.tracks_mut().get_mut(track_idx) {
            if from < track.effects.len() && to < track.effects.len() {
                let slot = track.effects.remove(from);
                track.effects.insert(to, slot);
                state.selected_slot_index = to;
            }
        }
        cx.notify();
    });
}

pub fn render(cx: &mut Context<HikaruApp>) -> impl IntoElement {
    let (track_idx, selected_slot, effects, title) = {
        let app = state(cx).read(cx);
        let track_idx = app.safe_track_index();
        if !app.tracks().get(track_idx).is_some() {
            return v_flex().size_full().into_any_element();
        }
        let track = &app.tracks()[track_idx];

        (
            track_idx,
            app.selected_slot_index,
            track.effects.clone(),
            rack_title(&track.name, app.matrix_state.selected_slot.map(|(_, scene)| scene)),
        )
    };

    let (open_browser, open_options) = {
        let app = state(cx).read(cx);
        let track = &app.tracks()[track_idx];
        let open_browser = track.effects.iter().position(|slot| slot.menu_open);
        let open_options = track.effects.iter().position(|slot| slot.options_open);
        (open_browser, open_options)
    };
    let options_left = open_options.map(|idx| card_x_offset(&effects, idx));

    div()
        .id("dsp_rack")
        .w_full()
        .flex()
        .flex_col()
        .child(
            div()
                .w_full()
                .h(px(RACK_HEIGHT))
                .flex()
                .flex_col()
                .bg(rgb(0x14141A))
                .border_t_1()
                .border_color(rgb(0x2A2A2A))
                .child(
                    div()
                        .w_full()
                        .h(px(20.0))
                        .flex()
                        .items_center()
                        .px(px(8.0))
                        .bg(rgb(0x1A1A1F))
                        .child(Label::new(title).text_xs().font_weight(FontWeight::BOLD).text_color(rgb(0x888888)))
                )
                .child(
                    div()
                        .id("dsp_strip")
                        .w_full()
                        .flex_1()
                        .min_h(px(RACK_HEIGHT - 26.0))
                        .overflow_x_scrollbar()
                        .child(
                            h_flex()
                                .h_full()
                                .gap(px(CARD_GAP))
                                .px(px(8.0))
                                .py(px(6.0))
                                .children(effects.iter().enumerate().map(|(idx, slot)| {
                                    render_card(cx, track_idx, idx, slot, idx == selected_slot)
                                }))
                                .child(render_add_slot_button(cx, track_idx, effects.len()))
                        )
                )
        )
        .when_some(open_browser, |this, idx| {
            this.child(render_plugin_browser(cx, track_idx, idx))
        })
        .when_some(open_options.zip(options_left), |this, (idx, left)| {
            this.child(render_options_menu(cx, track_idx, idx, left))
        })
        .into_any_element()
}

fn card_x_offset(effects: &[DspSlot], idx: usize) -> f32 {
    let before: f32 = effects
        .iter()
        .take(idx)
        .map(|_| CARD_WIDTH + CARD_GAP)
        .sum();
    8.0 + before
}

fn render_add_slot_button(cx: &mut Context<HikaruApp>, track_idx: usize, slot_count: usize) -> AnyElement {
    let menu_open = {
        let app = state(cx).read(cx);
        app.add_slot_menu_open
    };

    div()
        .id(format!("dsp_add_slot_{track_idx}"))
        .w(px(60.0))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(if menu_open { rgb(0x252530) } else { rgb(0x1C1C22) })
        .border_1()
        .border_color(if menu_open { rgb(0xFF6E00) } else { rgb(0x2A2A30) })
        .rounded(px(4.0))
        .on_click(move |_, _, cx| {
            state(cx).update(cx, |state, cx| {
                if let Some(track) = state.tracks_mut().get_mut(track_idx) {
                    let new_id = track.effects.len();
                    track.effects.push(DspSlot::new(new_id, "Empty Slot".to_string()));
                    crate::views::open_wavetable::log_initial_wavetable(&format!("slot t{track_idx}s{new_id}"));
                    state.selected_slot_index = new_id;
                    if let Some(slot) = state.slot_mut(track_idx, new_id) {
                        slot.menu_open = true;
                    }
                }
                state.add_slot_menu_open = false;
                cx.notify();
            });
        })
        .child(
            Label::new("[ + ]")
                .text_sm()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0x888888))
        )
        .into_any_element()
}

fn render_card(
    cx: &mut Context<HikaruApp>,
    track_idx: usize,
    idx: usize,
    slot: &DspSlot,
    is_selected: bool,
) -> AnyElement {
    let name = slot.name.clone();
    let slot_active = slot.active;
    let options_open = slot.options_open || slot.menu_open;
    let external = slot.external.clone();
    let load_error = slot.load_error.clone();

    div()
        .id(format!("dsp_card_{track_idx}_{idx}"))
        .w(px(CARD_WIDTH))
        .h_full()
        .flex()
        .flex_col()
        .bg(if is_selected { rgb(0x252530) } else { rgb(0x1C1C22) })
        .border_1()
        .border_color(if is_selected { rgb(0xFF6E00) } else { rgb(0x2A2A30) })
        .rounded(px(4.0))
        .overflow_hidden()
        .on_click(move |_, _, cx| {
            state(cx).update(cx, |state, cx| {
                state.selected_slot_index = idx;
                cx.notify();
            });
        })
        .on_mouse_down(gpui_kit::MouseButton::Right, move |_, _, cx| {
            eprintln!("[Hikaru] card RIGHT t{track_idx}s{idx}");
            open_plugin_menu(cx, track_idx, idx);
        })
        .on_aux_click(move |_, _, cx| {
            eprintln!("[Hikaru] card AUX t{track_idx}s{idx}");
            open_plugin_menu(cx, track_idx, idx);
        })
        .child(render_card_header(cx, track_idx, idx, &name, slot_active, options_open, is_selected))
        .child(render_card_body(
            cx,
            track_idx,
            idx,
            &name,
            external.as_ref(),
            load_error.as_deref(),
        ))
        .child(render_card_footer(cx, track_idx, idx, &name))
        .into_any_element()
}

fn render_card_header(
    cx: &mut Context<HikaruApp>,
    track_idx: usize,
    idx: usize,
    name: &str,
    active: bool,
    options_open: bool,
    is_selected: bool,
) -> AnyElement {
    h_flex()
        .w_full()
        .h(px(CARD_HEADER_HEIGHT))
        .items_center()
        .gap(px(4.0))
        .px(px(CARD_PADDING))
        .bg(if is_selected { rgb(0x2A2A35) } else { rgb(0x222228) })
        .border_b_1()
        .border_color(rgb(0x2A2A30))
        .child(
            rack_button(format!("dsp_card_toggle_{track_idx}_{idx}"))
                .rounded(ButtonRounded::None)
                .label(if active { "●" } else { "○" })
                .compact()
                .when(active, |b| b.text_color(rgb(0x00FF88)))
                .when(!active, |b| b.text_color(rgb(0x666666)))
                .on_click(move |_, _, cx| {
                    state(cx).update(cx, |state, cx| {
                        if let Some(slot) = state.slot_mut(track_idx, idx) {
                            slot.active = !slot.active;
                        }
                        cx.notify();
                    });
                })
        )
        .child(
            Label::new(format!("{:02} {}", idx + 1, name))
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(if active { rgb(0xE0E0E0) } else { rgb(0x666666) })
                .flex_1()
                .truncate()
        )
        .child(
            div()
                .id(format!("dsp_card_menu_{track_idx}_{idx}"))
                .min_w(px(22.0))
                .px(px(4.0))
                .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| {
                    eprintln!("[Hikaru] triangle LEFT t{track_idx}s{idx}");
                    cycle_card_menu(cx, track_idx, idx);
                })
                .on_mouse_down(gpui_kit::MouseButton::Right, move |_, _, cx| {
                    eprintln!("[Hikaru] triangle RIGHT t{track_idx}s{idx}");
                    open_plugin_menu(cx, track_idx, idx);
                })
                .on_aux_click(move |_, _, cx| {
                    eprintln!("[Hikaru] triangle AUX t{track_idx}s{idx}");
                    open_plugin_menu(cx, track_idx, idx);
                })
                .child(
                    Label::new(if options_open { "▲" } else { "▼" })
                        .text_xs()
                        .text_color(rgb(0xE0E0E0)),
                )
                .into_any_element()
        )
        .into_any_element()
}

fn render_card_body(
    cx: &mut Context<HikaruApp>,
    track_idx: usize,
    idx: usize,
    name: &str,
    external: Option<&hikaru_plugin_host::PluginDescriptor>,
    load_error: Option<&str>,
) -> AnyElement {
    // Un plugin externo gana sobre el `match` por nombre: el nombre de un
    // VST3 podría coincidir con el de un interno, y el descriptor es la
    // identidad fiable.
    if let Some(descriptor) = external {
        return render_external_card(cx, track_idx, idx, descriptor, load_error);
    }

    match name {
        "OpenWavetable" => open_wavetable::render_module(cx, track_idx, idx),
        "Hikaru OpenDMS" => render_dms_compact(cx, track_idx, idx),
        "OpenEQ3" => v_flex()
            .w_full()
            .flex_1()
            .min_h(px(100.0))
            .items_center()
            .justify_center()
            .gap(px(2.0))
            .child(
                Label::new("OpenEQ3")
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(0xE8EAF0)),
            )
            .child(
                Label::new("Low | Mid | Hi")
                    .text_xs()
                    .text_color(rgb(0x888888)),
            )
            .into_any_element(),
        _ => {
            div()
                .id(format!("dsp_empty_{track_idx}_{idx}"))
                .w_full()
                .flex_1()
                .min_h(px(100.0))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| {
                    open_plugin_menu(cx, track_idx, idx);
                })
                .on_mouse_down(gpui_kit::MouseButton::Right, move |_, _, cx| {
                    open_plugin_menu(cx, track_idx, idx);
                })
                .on_aux_click(move |_, _, cx| {
                    open_plugin_menu(cx, track_idx, idx);
                })
                .child(
                    Label::new("Empty Slot")
                        .text_sm()
                        .text_color(rgb(0x888888))
                )
                .into_any_element()
        }
    }
}

fn render_card_footer(
    cx: &mut Context<HikaruApp>,
    track_idx: usize,
    idx: usize,
    name: &str,
) -> AnyElement {
    match name {
        "OpenWavetable" => open_wavetable::render_module_footer(cx, track_idx, idx),
        _ => div().w_full().h(px(CARD_FOOTER_HEIGHT)).into_any_element(),
    }
}

/// Navegador de plugins del rack (estilo DAW pro).
///
/// Sustituye al antiguo menú flotante anclado a la tarjeta: en vez de un
/// popup estrecho con el listado crudo, es un panel que ocupa toda la tira
/// del rack con cabecera (título + buscador + cierre), fila de filtros por
/// formato, lista con scroll y acciones de escaneo. Todo con el tema oscuro
/// de Hikaru (fondos `#14141A`/`#1C1C22`, acento naranja `#FF6E00`).
///
/// Vive como overlay absoluto sobre la tira (`top(20)` = debajo del título
/// del rack): no mueve ni superpone tarjetas, y al asignar o cerrar se
/// desmonta sin dejar estado colgado (`close_browser`).
fn render_plugin_browser(
    cx: &mut Context<HikaruApp>,
    track_idx: usize,
    slot_idx: usize,
) -> AnyElement {
    let (all_externals, query, filter, scan_status, is_scanning, scan_error_count, gui_errors, search_input) = {
        let app = state(cx).read(cx);
        (
            app.plugin_settings_state.discovered_plugins.clone(),
            app.plugin_browser_query.clone(),
            app.plugin_browser_filter,
            app.plugin_settings_state.status_message.clone(),
            app.plugin_settings_state.is_scanning,
            app.plugin_settings_state.scan_errors.len(),
            app.plugin_settings_state.gui_errors.clone(),
            app.plugin_search_input.clone(),
        )
    };
    let needle = query.trim().to_lowercase();

    let natives: Vec<PluginEntry> = PLUGIN_CATALOG
        .iter()
        .copied()
        .filter(|entry| {
            needle.is_empty()
                || entry.label.to_lowercase().contains(&needle)
                || entry.name.to_lowercase().contains(&needle)
        })
        .collect();
    // El id identifica al plugin entre escaneos, así que dos plugins con el
    // mismo nombre no colisionan en el DOM de GPUI (que indexa por id).
    let filtered_externals: Vec<hikaru_plugin_host::PluginDescriptor> = all_externals
        .into_iter()
        .filter(|d| filter.matches_external(d.format))
        .filter(|d| {
            needle.is_empty()
                || d.name.to_lowercase().contains(&needle)
                || d.vendor.to_lowercase().contains(&needle)
        })
        .collect();
    let show_natives = filter.show_natives();

    let target_label = format!("Pista {} · Slot {:02}", track_idx + 1, slot_idx + 1);

    div()
        .id("plugin_browser")
        .absolute()
        .left(px(0.0))
        .w_full()
        .top(px(20.0))
        .bottom(px(0.0))
        .flex()
        .flex_col()
        .bg(rgb(0x101014))
        .border_t_1()
        .border_color(rgb(0xFF6E00))
        .child(
            // Cabecera: qué slot se está vistiendo + buscador + cierre.
            h_flex()
                .w_full()
                .h(px(32.0))
                .flex_shrink_0()
                .items_center()
                .gap(px(8.0))
                .px(px(10.0))
                .bg(rgb(0x1A1A1F))
                .border_b_1()
                .border_color(rgb(0x2A2A30))
                .child(
                    Label::new("AÑADIR PLUGIN")
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0xE0E0E0)),
                )
                .child(
                    Label::new(target_label)
                        .text_xs()
                        .text_color(rgb(0x888888)),
                )
                .child(
                    TextStyleScope::text_color(
                        rgb(0xE0E0E0),
                        div()
                            .flex_1()
                            .h(px(22.0))
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .bg(rgb(0x0C0C10))
                            .border_1()
                            .border_color(rgb(0x2A2A30))
                            .rounded(px(4.0))
                            .px(px(6.0))
                            .child(
                                Label::new("⌕")
                                    .text_xs()
                                    .text_color(rgb(0x666666)),
                            )
                            .child(
                                Input::new(&search_input)
                                    .appearance(false)
                                    .bordered(false)
                                    .cleanable(true)
                                    .h_full()
                                    .w_full()
                                    .with_size(Size::XSmall),
                            ),
                    ),
                )
                .child(
                    rack_button("dsp_browser_close")
                        .label("✕")
                        .on_click(move |_, _, cx| {
                            close_browser(cx, track_idx);
                        }),
                ),
        )
        .child(
            // Filtros por formato + estado del escaneo + acciones.
            h_flex()
                .w_full()
                .h(px(28.0))
                .flex_shrink_0()
                .items_center()
                .gap(px(4.0))
                .px(px(10.0))
                .children(PluginBrowserFilter::TABS.iter().map(|(tab, label)| {
                    let active = *tab == filter;
                    rack_button(format!("dsp_browser_tab_{label}"))
                        .label(*label)
                        .when(active, |b| {
                            b.bg(rgb(0x3A2410)).text_color(rgb(0xFF9E40))
                        })
                        .when(!active, |b| b.text_color(rgb(0x888888)))
                        .on_click(move |_, _, cx| {
                            let tab = *tab;
                            state(cx).update(cx, |state, cx| {
                                state.plugin_browser_filter = tab;
                                cx.notify();
                            });
                        })
                        .into_any_element()
                }))
                .child(div().flex_1())
                .child(
                    Label::new(if is_scanning {
                        "Escaneando…".to_string()
                    } else if scan_error_count > 0 {
                        format!("{scan_status} ({scan_error_count} omitidos)")
                    } else {
                        scan_status.clone()
                    })
                    .text_xs()
                    .truncate()
                    .max_w(px(260.0))
                    .text_color(rgb(0x666666)),
                )
                .child(
                    rack_button("dsp_browser_rescan")
                        .label(if is_scanning { "…" } else { "⟳ Rescan" })
                        .on_click(move |_, _, cx| {
                            state(cx).update(cx, |state, cx| {
                                state.plugin_settings_state.start_scan();
                                cx.notify();
                            });
                        }),
                )
                .child(
                    rack_button("dsp_browser_paths")
                        .label("Rutas…")
                        .on_click(move |_, _, cx| {
                            state(cx).update(cx, |state, cx| {
                                state.plugin_settings_state.is_open = true;
                                cx.notify();
                            });
                        }),
                ),
        )
        .child(
            // Resultados con scroll: nativos arriba, externos debajo.
            v_flex()
                .w_full()
                .flex_1()
                .min_h(px(0.0))
                .overflow_y_scrollbar()
                .px(px(10.0))
                .py(px(6.0))
                .gap(px(3.0))
                .when(show_natives && !natives.is_empty(), |this| {
                    this.child(browser_section_header("Plugins nativos · Hikaru"))
                })
                .children(natives.iter().map(|entry| {
                    browser_native_row(track_idx, slot_idx, *entry)
                }))
                .when(show_natives && natives.is_empty() && filtered_externals.is_empty(), |this| {
                    this.child(browser_empty_state("Sin nativos para esa búsqueda."))
                })
                .when(!filtered_externals.is_empty(), |this| {
                    this.child(browser_section_header(&format!(
                        "VST3 / CLAP externos ({})",
                        filtered_externals.len()
                    )))
                })
                .children(filtered_externals.iter().map(|descriptor| {
                    let gui_error = gui_errors.get(&descriptor.name).cloned();
                    browser_external_row(track_idx, slot_idx, descriptor.clone(), gui_error)
                }))
                .when(filtered_externals.is_empty(), |this| {
                    this.child(browser_empty_state(
                        "Ningún externo coincide. Pulsa ⟳ Rescan o revisa Rutas…",
                    ))
                })
                .into_any_element(),
        )
        .into_any_element()
}

/// Cabecera de sección del navegador ("Plugins nativos", "VST3 / CLAP (3)").
fn browser_section_header(title: &str) -> AnyElement {
    Label::new(title)
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(0x6A7080))
        .into_any_element()
}

fn browser_empty_state(message: &str) -> AnyElement {
    div()
        .w_full()
        .py(px(6.0))
        .flex()
        .justify_center()
        .child(
            Label::new(message)
                .text_xs()
                .text_color(rgb(0x4A5060)),
        )
        .into_any_element()
}

/// Fila de plugin nativo: toda la fila asigna (el "Añadir ›" es affordance
/// visual, no un botón anidado, para no disparar dos asignaciones).
fn browser_native_row(track_idx: usize, slot_idx: usize, entry: PluginEntry) -> AnyElement {
    div()
        .id(format!("dsp_browser_nat_{}_{slot_idx}", entry.name))
        .w_full()
        .h(px(28.0))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(8.0))
        .px(px(8.0))
        .bg(rgb(0x1C1C22))
        .border_1()
        .border_color(rgb(0x2A2A30))
        .rounded(px(4.0))
        .on_click(move |_, _, cx| {
            assign_plugin(cx, track_idx, slot_idx, entry.name);
        })
        .child(
            Label::new("INT")
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0x00FF88)),
        )
        .child(
            Label::new(entry.label)
                .text_xs()
                .truncate()
                .flex_1()
                .text_color(rgb(0xE8EAF0)),
        )
        .child(
            Label::new(entry.category)
                .text_xs()
                .flex_shrink_0()
                .text_color(rgb(0x666666)),
        )
        .child(
            Label::new("Añadir ›")
                .text_xs()
                .flex_shrink_0()
                .text_color(rgb(0xFF9E40)),
        )
        .into_any_element()
}

/// Fila de plugin externo. Si su GUI falló alguna vez (`gui_errors`), la
/// fila lo muestra en rojo en vez de prometer un editor que no abre: el
/// plugin sigue asignable para audio, pero el usuario sabe qué pasa.
fn browser_external_row(
    track_idx: usize,
    slot_idx: usize,
    descriptor: hikaru_plugin_host::PluginDescriptor,
    gui_error: Option<String>,
) -> AnyElement {
    let is_vst3 = descriptor.format == hikaru_plugin_host::PluginFormat::VST3;
    let badge = if is_vst3 { "VST3" } else { "CLAP" };
    let badge_color = if is_vst3 { rgb(0x64B4FF) } else { rgb(0xB464FF) };
    let row_id = descriptor.id.clone();
    let plugin_name = descriptor.name.clone();
    let sub = if descriptor.vendor.is_empty() {
        descriptor
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    } else {
        descriptor.vendor.clone()
    };

    v_flex()
        .w_full()
        .flex_shrink_0()
        .bg(rgb(0x1C1C22))
        .border_1()
        .border_color(rgb(0x2A2A30))
        .rounded(px(4.0))
        .px(px(8.0))
        .py(px(2.0))
        .child(
            div()
                .id(format!("dsp_browser_ext_{row_id}_{slot_idx}"))
                .w_full()
                .h(px(24.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .on_click(move |_, _, cx| {
                    assign_external_plugin(cx, track_idx, slot_idx, descriptor.clone());
                })
                .child(
                    Label::new(badge)
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .flex_shrink_0()
                        .w(px(36.0))
                        .text_color(badge_color),
                )
                .child(
                    Label::new(plugin_name)
                        .text_xs()
                        .truncate()
                        .flex_1()
                        .text_color(rgb(0xE8EAF0)),
                )
                .child(
                    Label::new(sub)
                        .text_xs()
                        .truncate()
                        .flex_shrink_0()
                        .w(px(140.0))
                        .text_color(rgb(0x666666)),
                )
                .child(
                    Label::new("Añadir ›")
                        .text_xs()
                        .flex_shrink_0()
                        .text_color(rgb(0xFF9E40)),
                )
                .into_any_element(),
        )
        .when_some(gui_error, |this, message| {
            this.child(
                Label::new(format!("⚠ {message}"))
                    .text_xs()
                    .truncate()
                    .text_color(rgb(0xFF7A5C)),
            )
        })
        .into_any_element()
}

fn render_options_menu(
    cx: &mut Context<HikaruApp>,
    track_idx: usize,
    slot_idx: usize,
    left: f32,
) -> AnyElement {
    div()
        .absolute()
        .left(px(left))
        .bottom(px(RACK_HEIGHT))
        .w(px(CARD_WIDTH))
        .bg(rgb(0x1E1E26))
        .border_1()
        .border_color(rgb(0x3A3A45))
        .rounded(px(4.0))
        .shadow(vec![gpui_kit::BoxShadow {
            color: gpui_kit::black().opacity(0.5),
            offset: point(px(0.0), px(-4.0)),
            blur_radius: px(8.0),
            spread_radius: px(0.0),
            inset: false,
        }])
        .p(px(4.0))
        .gap(px(2.0))
        .child(
            rack_button(format!("dsp_opt_save_{track_idx}_{slot_idx}"))
                .rounded(ButtonRounded::None)
                .label("Save Preset...")
                .compact()
                .w_full()
                .on_click(move |_, _, cx| {
                    state(cx).update(cx, |state, cx| {
                        if let Some(slot) = state.slot_mut(track_idx, slot_idx) {
                            slot.options_open = false;
                        }
                        cx.notify();
                    });
                }),
        )
        .child(
            rack_button(format!("dsp_opt_paths_{track_idx}_{slot_idx}"))
                .rounded(ButtonRounded::None)
                .label("Add Wavetable Paths...")
                .compact()
                .w_full()
                .on_click(move |_, _, cx| {
                    crate::views::explorer::begin_wavetable_pick(cx, track_idx, slot_idx);
                    state(cx).update(cx, |state, cx| {
                        if let Some(slot) = state.slot_mut(track_idx, slot_idx) {
                            slot.options_open = false;
                        }
                        cx.notify();
                    });
                }),
        )
        .child(
            rack_button(format!("dsp_opt_delete_{track_idx}_{slot_idx}"))
                .rounded(ButtonRounded::None)
                .label("Delete Slot")
                .compact()
                .w_full()
                .on_click(move |_, _, cx| {
                    remove_slot_at(cx, track_idx, slot_idx);
                }),
        )
        .into_any_element()
}

/// Tarjeta de un plugin externo VST3/CLAP.
///
/// Es un placeholder, no el editor: el editor nativo es una ventana del sistema
/// (XWayland, ver `hikaru_plugin_host::platform::linux`), no un árbol de
/// elementos GPUI. Lo que se dibuja acá es el ancla visual dentro del rack --
/// formato, archivo y el botón que abre la ventana -- para que el usuario sepa
/// qué tiene cargado sin abrir nada.
///
/// El contenedor no reserva el tamaño real del editor porque no se puede
/// conocer sin instanciar el plugin. Cuando el motor lo cargue se puede
/// pintar el tamaño preferido; hasta entonces el layout del rack no depende de
/// plugins externos, que es lo que evita que la ventana salte al cargarlos.
fn render_external_card(
    cx: &mut Context<HikaruApp>,
    track_idx: usize,
    idx: usize,
    descriptor: &hikaru_plugin_host::PluginDescriptor,
    load_error: Option<&str>,
) -> AnyElement {
    let is_vst3 = descriptor.format == hikaru_plugin_host::PluginFormat::VST3;
    let badge = if is_vst3 { "VST3" } else { "CLAP" };
    let badge_color = if is_vst3 { rgb(0x64B4FF) } else { rgb(0xB464FF) };
    let file_name = descriptor
        .path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    // Un slot con error de carga muestra el mensaje y no ofrece abrir el
    // editor: insistir produciría el mismo fallo y, si el plugin llegó a
    // cargarse a medias, dejaría una ventana zombie.
    // Error de GUI nativa (hilo de la ventana → `gui_errors` vía
    // `poll_updates`): el plugin cargó pero su editor no abrió. Se pinta en
    // la tarjeta en vez de cerrar el DAW o dejar una ventana vacía.
    let (gui_error, editor_open, editor_opening, params, params_status, slot_has_gui) = {
        let app = state(cx).read(cx);
        let slot_params = app
            .slot(track_idx, idx)
            .map(|slot| {
                (
                    slot.external_params.clone(),
                    slot.params_status.clone(),
                    slot.has_gui,
                )
            })
            .unwrap_or_default();
        (
            app.plugin_settings_state.gui_error_for(&descriptor.name),
            app.plugin_settings_state.is_plugin_open(&descriptor.name),
            app.plugin_settings_state.is_plugin_opening(&descriptor.name),
            slot_params.0,
            slot_params.1,
            slot_params.2,
        )
    };
    let blocked_by_gui_error = gui_error.is_some();

    // `descriptor.has_gui` viene del escaneo de disco, que no instancia nada
    // y por lo tanto lo deja siempre en `false`: usarlo haría que este botón
    // no se dibujara jamás. El sondeo diferido sí crea una instancia real, así
    // que su respuesta es la que manda. `descriptor.has_gui` queda como
    // respaldo para el frame previo a que termine el sondeo.
    let can_open_editor = slot_has_gui.unwrap_or(descriptor.has_gui) && load_error.is_none();

    // Primer frame con externo asignado: pedir el sondeo de parámetros. Corre
    // una sola vez (la petición marca `Probing` en el acto) en un hilo
    // aparte; el resultado vuelve vía `drain_param_probes`.
    if matches!(params_status, ExternalParamsStatus::Idle) {
        let descriptor = descriptor.clone();
        let st = state(cx);
        cx.update_entity(&st, |state, cx| {
            state.request_external_params(&descriptor);
            cx.notify();
        });
    }

    div()
        .id(format!("dsp_external_{track_idx}_{idx}"))
        .w_full()
        .flex_1()
        .min_h(px(100.0))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(4.0))
        .px(px(6.0))
        .py(px(4.0))
        .on_mouse_down(gpui_kit::MouseButton::Right, move |_, _, cx| {
            open_plugin_menu(cx, track_idx, idx);
        })
        // Fin del drag de knob en cualquier suelta sobre la tarjeta, y
        // seguimiento del gesto en movimiento (sólo actúa si hay drag).
        .on_mouse_move(move |event, _, cx| {
            param_drag_move(cx, f32::from(event.position.y));
        })
        .on_mouse_up(gpui_kit::MouseButton::Left, move |_, _, cx| {
            param_drag_end(cx);
        })
        .child(
            Label::new(badge)
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(badge_color),
        )
        .child(
            Label::new(descriptor.name.clone())
                .text_sm()
                .text_color(rgb(0xE8EAF0)),
        )
        .child(
            Label::new(file_name)
                .text_xs()
                .truncate()
                .text_color(rgb(0x666666)),
        )
        .when_some(load_error.map(|e| e.to_string()), |this, message| {
            this.child(
                Label::new(format!("⚠ {message}"))
                    .text_xs()
                    .text_color(rgb(0xFF6E00))
                    .truncate(),
            )
        })
        .when_some(gui_error, |this, message| {
            this.child(
                Label::new(format!("⚠ No abrió su ventana: {message}"))
                    .text_xs()
                    .text_color(rgb(0xFF7A5C))
                    .truncate(),
            )
        })
        // Toggle de editor nativo. Es un botón porque abrir la GUI del plugin es
        // costoso (dlopen + creación de ventana nativa en un hilo aparte) y no
        // tiene sentido mantenerlo cargado si el usuario no lo está mirando.
        //
        // `editor_open` refleja el estado real que devuelve
        // `plugin_settings_state`, no un bool local: la ventana se puede cerrar
        // desde el WM y el toggle tiene que enterarse.
        .when(can_open_editor && !blocked_by_gui_error, |this| {
            let (label, active) = if editor_open {
                ("■  Editor", true)
            } else if editor_opening {
                ("…  Abriendo", false)
            } else {
                ("▸  Editor", false)
            };
            // El nombre se clona para que el closure lo tenga por valor: el
            // toggle necesita poder cerrar por nombre, no reabrir por slot.
            let plugin_name = descriptor.name.clone();
            this.child(
                rack_button(format!("dsp_editor_{track_idx}_{idx}"))
                    .rounded(ButtonRounded::None)
                    .label(label)
                    .compact()
                    .on_click(move |_, window, cx| {
                        if active {
                            close_external_editor(cx, &plugin_name);
                        } else {
                            open_external_editor(window, cx, track_idx, idx);
                        }
                    }),
            )
        })
        .child(render_external_params(
            cx,
            track_idx,
            idx,
            descriptor,
            &params,
            &params_status,
        ))
        .into_any_element()
}

/// Knobs del plugin externo: parámetros automatizables con drag vertical,
/// steppers de precisión y valor con el formato del plugin.
///
/// Máximo 8 filas (la tarjeta vive en un rack de 340px): el resto vive en el
/// editor nativo. Cada gesto escribe la caché del slot, la marca `dirty`
/// para el motor y la manda en vivo a la ventana abierta
/// (`commit_external_param`). Lo que el editor nativo mueve por su cuenta
/// vuelve por `apply_param_feedback` y los knobs lo reflejan sin pelear con
/// el drag en curso (el feedback cede ante el dedo).
fn render_external_params(
    _cx: &mut Context<HikaruApp>,
    track_idx: usize,
    idx: usize,
    descriptor: &hikaru_plugin_host::PluginDescriptor,
    params: &[crate::views::mixer::ExternalParamState],
    status: &ExternalParamsStatus,
) -> AnyElement {
    let is_clap = descriptor.format == hikaru_plugin_host::PluginFormat::CLAP;

    let header: AnyElement = match status {
        ExternalParamsStatus::Probing => Label::new("Leyendo parámetros…")
            .text_xs()
            .text_color(rgb(0x666666))
            .into_any_element(),
        ExternalParamsStatus::Failed(message) => {
            let descriptor = descriptor.clone();
            h_flex()
                .gap(px(6.0))
                .items_center()
                .child(
                    Label::new(format!("⚠ {message}"))
                        .text_xs()
                        .truncate()
                        .flex_1()
                        .text_color(rgb(0xFF7A5C)),
                )
                .child(
                    rack_button(format!("dsp_param_retry_{track_idx}_{idx}"))
                        .label("Reintentar")
                        .compact()
                        .flex_shrink_0()
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            cx.update_entity(&st, |state, cx| {
                                state.request_external_params(&descriptor);
                                cx.notify();
                            });
                        }),
                )
                .into_any_element()
        }
        ExternalParamsStatus::Ready if params.is_empty() => Label::new(if is_clap {
            "CLAP: parámetros no vinculados por el host."
        } else {
            "Sin parámetros automatizables."
        })
        .text_xs()
        .text_color(rgb(0x555555))
        .into_any_element(),
        ExternalParamsStatus::Ready => {
            let extra = params.len().saturating_sub(MAX_PARAM_ROWS);
            Label::new(if extra > 0 {
                format!("PARÁMETROS ({}) · +{extra} en el editor", params.len())
            } else {
                format!("PARÁMETROS ({})", params.len())
            })
            .text_xs()
            .font_weight(FontWeight::BOLD)
            .text_color(rgb(0x6A7080))
            .into_any_element()
        }
        ExternalParamsStatus::Idle => Label::new("Preparando parámetros…")
            .text_xs()
            .text_color(rgb(0x555555))
            .into_any_element(),
    };

    v_flex()
        .w_full()
        .gap(px(2.0))
        .child(header)
        .children(
            params
                .iter()
                .take(MAX_PARAM_ROWS)
                .map(|param| external_param_row(track_idx, idx, param)),
        )
        .into_any_element()
}

/// Filas visibles por tarjeta: el rack mide 340px y cada fila 20px.
const MAX_PARAM_ROWS: usize = 8;

/// Una fila de parámetro: stepper −, nombre, barra arrastrable (drag
/// vertical sobre ella), valor, stepper +.
///
/// La barra es el knob: `mouse_down` congela el agarre (`param_drag_begin`)
/// y el `mouse_move` de la tarjeta lo continúa (`param_drag_move`) hasta el
/// `mouse_up`. Los steppers usan el escalón del parámetro (discretos
/// saltan de escalón en escalón; continuos, de 0.05 en 0.05).
fn external_param_row(
    track_idx: usize,
    idx: usize,
    param: &crate::views::mixer::ExternalParamState,
) -> AnyElement {
    let id = param.info.id;
    let value = param.value;
    let name = param.info.name.clone();
    let display = param.display();
    let step = if param.info.is_discrete() {
        1.0 / param.info.step_count.max(1) as f32
    } else {
        0.05
    };
    let fill_w = (value.clamp(0.0, 1.0) * PARAM_BAR_WIDTH).max(2.0);

    h_flex()
        .w_full()
        .h(px(20.0))
        .flex_shrink_0()
        .items_center()
        .gap(px(4.0))
        .child(
            rack_button(format!("dsp_param_dec_{track_idx}_{idx}_{id}"))
                .label("−")
                .compact()
                .flex_shrink_0()
                .w(px(20.0))
                .on_click(move |_, _, cx| {
                    commit_external_param(cx, track_idx, idx, id, value - step);
                }),
        )
        .child(
            Label::new(name)
                .text_xs()
                .truncate()
                .flex_1()
                .text_color(rgb(0xB8BCC8)),
        )
        .child(
            div()
                .id(format!("dsp_parambar_{track_idx}_{idx}_{id}"))
                .flex_shrink_0()
                .w(px(PARAM_BAR_WIDTH))
                .h(px(10.0))
                .flex()
                .items_center()
                .bg(rgb(0x0C0C10))
                .border_1()
                .border_color(rgb(0x2A2A30))
                .rounded(px(3.0))
                .overflow_hidden()
                .on_mouse_down(gpui_kit::MouseButton::Left, move |event, _, cx| {
                    param_drag_begin(cx, track_idx, idx, id, f32::from(event.position.y));
                })
                .child(
                    div()
                        .w(px(fill_w))
                        .h_full()
                        .bg(rgb(0xFF6E00))
                        .into_any_element(),
                )
                .into_any_element(),
        )
        .child(
            Label::new(display)
                .text_xs()
                .truncate()
                .flex_shrink_0()
                .w(px(64.0))
                .text_color(rgb(0x888888)),
        )
        .child(
            rack_button(format!("dsp_param_inc_{track_idx}_{idx}_{id}"))
                .label("+")
                .compact()
                .flex_shrink_0()
                .w(px(20.0))
                .on_click(move |_, _, cx| {
                    commit_external_param(cx, track_idx, idx, id, value + step);
                }),
        )
        .into_any_element()
}

/// Ancho fijo de la barra del knob (el relleno se calcula en px).
const PARAM_BAR_WIDTH: f32 = 56.0;

fn render_dms_compact(cx: &mut Context<HikaruApp>, track_idx: usize, idx: usize) -> AnyElement {    let app = state(cx).read(cx);
    let Some(dms) = app.slot(track_idx, idx).and_then(|slot| slot.dms_state.clone()) else {
        return div()
            .w_full()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .child(Label::new("No DMS state").text_xs().text_color(rgb(0x555555)))
            .into_any_element();
    };
    let pads = dms.pad_count;
    let loaded = dms.pads.iter().filter(|pad| pad.sample_path.is_some()).count();

    div()
        .w_full()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(4.0))
        .child(Label::new("OpenDMS Sampler").text_xs().font_weight(FontWeight::BOLD).text_color(rgb(0x00FFC8)))
        .child(Label::new(format!("{loaded}/{pads} pads")).text_xs().text_color(rgb(0x888888)))
        .into_any_element()
}

pub fn rack_title(track_name: &str, scene: Option<usize>) -> String {
    match scene {
        Some(scene) => format!("DSP RACK: {track_name} | Scene {}", scene + 1),
        None => format!("DSP RACK: {track_name}"),
    }
}

pub fn handle_delete_key(cx: &mut gpui_kit::App) {
    let track_idx = state(cx).read(cx).safe_track_index();
    remove_selected_slot(cx, track_idx);
}
