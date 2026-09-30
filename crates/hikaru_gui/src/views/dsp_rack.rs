// crates/hikaru_gui/src/views/dsp_rack.rs

use gpui_kit::component::*;
use gpui_kit::component::button::{Button, ButtonRounded};
use gpui_kit::component::label::Label;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, HikaruApp};
use crate::views::mixer::DspSlot;
use crate::views::open_wavetable;

pub const RACK_HEIGHT: f32 = 230.0;

pub const CARD_WIDTH: f32 = 230.0;

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

const PLUGIN_CATEGORIES: [&str; 3] = ["Efectos", "Generadores", "VST3 / Externos"];

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
                slot.menu_open = false;
                slot.options_open = false;
            }
            state.selected_slot_index = slot_idx;
            state.add_slot_menu_open = false;
            cx.notify();
        });
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

    let (open_menu, open_options, add_menu_open) = {
        let app = state(cx).read(cx);
        let track = &app.tracks()[track_idx];
        let open_menu = track.effects.iter().position(|slot| slot.menu_open);
        let open_options = track.effects.iter().position(|slot| slot.options_open);
        (open_menu, open_options, app.add_slot_menu_open)
    };
    let menu_left = open_menu.map(|idx| card_x_offset(&effects, idx));
    let options_left = open_options.map(|idx| card_x_offset(&effects, idx));
    let add_menu_left = card_x_offset(&effects, effects.len());

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
        .when_some(open_menu.zip(menu_left), |this, (idx, left)| {
            this.child(render_plugin_menu(cx, track_idx, idx, left))
        })
        .when_some(open_options.zip(options_left), |this, (idx, left)| {
            this.child(render_options_menu(cx, track_idx, idx, left))
        })
        .when(add_menu_open, |this| {
            this.child(render_plugin_menu(cx, track_idx, effects.len(), add_menu_left))
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
        .child(render_card_body(cx, track_idx, idx, &name))
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
) -> AnyElement {
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

fn render_plugin_menu(
    cx: &mut Context<HikaruApp>,
    track_idx: usize,
    slot_idx: usize,
    left: f32,
) -> AnyElement {
    let externals: Vec<String> = {
        let app = state(cx).read(cx);
        app.plugin_settings_state
            .discovered_plugins
            .iter()
            .map(|plugin| plugin.name.clone())
            .collect()
    };

    div()
        .absolute()
        .left(px(left))
        .bottom(px(RACK_HEIGHT))
        .w(px(CARD_WIDTH))
        .h(px(220.0))
        .overflow_y_scrollbar()
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
        .children(PLUGIN_CATEGORIES.iter().map(|category| {
            let natives: Vec<PluginEntry> =
                PLUGIN_CATALOG.iter().filter(|entry| entry.category == *category).copied().collect();
            let external_count = if *category == "VST3 / Externos" { externals.len() } else { 0 };

            v_flex()
                .gap(px(1.0))
                .child(
                    Label::new(*category)
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0x6A7080)),
                )
                .children(natives.iter().map(|entry| {
                    let plugin_name = entry.name;
                    let plugin_label = entry.label;
                    rack_button(format!("dsp_menu_{}_{plugin_name}_{slot_idx}", entry.category))
                        .rounded(ButtonRounded::None)
                        .label(plugin_label)
                        .compact()
                        .w_full()
                        .on_click(move |_, _, cx| {
                            assign_plugin(cx, track_idx, slot_idx, plugin_name);
                        })
                        .into_any_element()
                }))
                .children(externals.iter().map(|name| {
                    let plugin_name = name.clone();
                    rack_button(format!("dsp_menu_ext_{plugin_name}_{slot_idx}"))
                        .rounded(ButtonRounded::None)
                        .label(plugin_name.clone())
                        .compact()
                        .w_full()
                        .on_click(move |_, _, cx| {
                            assign_plugin(cx, track_idx, slot_idx, &plugin_name);
                        })
                        .into_any_element()
                }))
                .when(
                    natives.is_empty() && external_count == 0,
                    |this| {
                        this.child(
                            Label::new(match *category {
                                "VST3 / Externos" => "ninguno escaneado",
                                _ => "vacío",
                            })
                            .text_xs()
                            .text_color(rgb(0x4A5060)),
                        )
                    },
                )
                .when(*category == "VST3 / Externos" && external_count == 0, |this| {
                    this.child(
                        rack_button(format!("dsp_menu_scan_{slot_idx}"))
                            .rounded(ButtonRounded::None)
                            .label("Escanear VST3 / CLAP...")
                            .compact()
                            .w_full()
                            .on_click(move |_, _, cx| {
                                state(cx).update(cx, |state, cx| {
                                    state.plugin_settings_state.is_open = true;
                                    if let Some(slot) = state.slot_mut(track_idx, slot_idx) {
                                        slot.menu_open = false;
                                    }
                                    cx.notify();
                                });
                            }),
                    )
                })
                .into_any_element()
        }))
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

fn render_dms_compact(cx: &mut Context<HikaruApp>, track_idx: usize, idx: usize) -> AnyElement {
    let app = state(cx).read(cx);
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
