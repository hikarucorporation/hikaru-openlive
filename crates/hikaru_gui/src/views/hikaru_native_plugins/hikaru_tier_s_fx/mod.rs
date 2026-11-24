// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_tier_s_fx/mod.rs

//! Tarjetas nativas compactas para los 8 efectos **Tier S** de Hikaru OpenLive:
//! `Tool`, `DC Offset`, `Dual Pan`, `Test Tone`, `Time Shift`, `Tremolo`,
//! `Ring-Mod` y `Hikaru Open8-Bit`.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use crate::app::AppState;
use crate::theme;

#[derive(Clone, Copy)]
enum ParamKind {
    Knob {
        min: f32,
        max: f32,
        unit: &'static str,
        log_freq: bool,
    },
    Toggle,
    Enum(&'static [&'static str]),
}

#[derive(Clone, Copy)]
struct ParamDesc {
    name: &'static str,
    default_norm: f32,
    kind: ParamKind,
}

struct DeviceSpec {
    canonical_name: &'static str,
    card_width: f32,
    accent: egui::Color32,
    params: &'static [ParamDesc],
}

const TOOL_PARAMS: &[ParamDesc] = &[
    ParamDesc {
        name: "Gain",
        default_norm: 0.5,
        kind: ParamKind::Knob {
            min: -36.0,
            max: 36.0,
            unit: "dB",
            log_freq: false,
        },
    },
    ParamDesc {
        name: "Pan",
        default_norm: 0.5,
        kind: ParamKind::Knob {
            min: -100.0,
            max: 100.0,
            unit: "%",
            log_freq: false,
        },
    },
    ParamDesc {
        name: "Width",
        default_norm: 0.5,
        kind: ParamKind::Knob {
            min: 0.0,
            max: 200.0,
            unit: "%",
            log_freq: false,
        },
    },
    ParamDesc {
        name: "Inv L",
        default_norm: 0.0,
        kind: ParamKind::Toggle,
    },
    ParamDesc {
        name: "Inv R",
        default_norm: 0.0,
        kind: ParamKind::Toggle,
    },
    ParamDesc {
        name: "Swap",
        default_norm: 0.0,
        kind: ParamKind::Toggle,
    },
];

const DC_OFFSET_PARAMS: &[ParamDesc] = &[
    ParamDesc {
        name: "Offset",
        default_norm: 0.5,
        kind: ParamKind::Knob {
            min: -100.0,
            max: 100.0,
            unit: "%",
            log_freq: false,
        },
    },
    ParamDesc {
        name: "Rem DC",
        default_norm: 0.0,
        kind: ParamKind::Toggle,
    },
];

const DUAL_PAN_PARAMS: &[ParamDesc] = &[
    ParamDesc {
        name: "Left Pan",
        default_norm: 0.0,
        kind: ParamKind::Knob {
            min: -100.0,
            max: 100.0,
            unit: "%",
            log_freq: false,
        },
    },
    ParamDesc {
        name: "Right Pan",
        default_norm: 1.0,
        kind: ParamKind::Knob {
            min: -100.0,
            max: 100.0,
            unit: "%",
            log_freq: false,
        },
    },
    ParamDesc {
        name: "Gain",
        default_norm: 24.0 / 36.0,
        kind: ParamKind::Knob {
            min: -24.0,
            max: 12.0,
            unit: "dB",
            log_freq: false,
        },
    },
];

const TEST_TONE_PARAMS: &[ParamDesc] = &[
    ParamDesc {
        name: "Freq",
        default_norm: 0.4475,
        kind: ParamKind::Knob {
            min: 20.0,
            max: 20_000.0,
            unit: "Hz",
            log_freq: true,
        },
    },
    ParamDesc {
        name: "Gain",
        default_norm: 0.8,
        kind: ParamKind::Knob {
            min: -60.0,
            max: 0.0,
            unit: "dB",
            log_freq: false,
        },
    },
    ParamDesc {
        name: "Wave",
        default_norm: 0.0,
        kind: ParamKind::Enum(&["Sine", "White", "Pink"]),
    },
    ParamDesc {
        name: "Mix",
        default_norm: 1.0,
        kind: ParamKind::Knob {
            min: 0.0,
            max: 100.0,
            unit: "%",
            log_freq: false,
        },
    },
];

const TIME_SHIFT_PARAMS: &[ParamDesc] = &[
    ParamDesc {
        name: "Delay",
        default_norm: 0.0,
        kind: ParamKind::Knob {
            min: 0.0,
            max: 100.0,
            unit: "ms",
            log_freq: false,
        },
    },
    ParamDesc {
        name: "Fine",
        default_norm: 0.0,
        kind: ParamKind::Knob {
            min: 0.0,
            max: 100.0,
            unit: "smp",
            log_freq: false,
        },
    },
    ParamDesc {
        name: "Mix",
        default_norm: 1.0,
        kind: ParamKind::Knob {
            min: 0.0,
            max: 100.0,
            unit: "%",
            log_freq: false,
        },
    },
];

const TREMOLO_PARAMS: &[ParamDesc] = &[
    ParamDesc {
        name: "Rate",
        default_norm: (4.0 - 0.1) / 19.9,
        kind: ParamKind::Knob {
            min: 0.1,
            max: 20.0,
            unit: "Hz",
            log_freq: false,
        },
    },
    ParamDesc {
        name: "Depth",
        default_norm: 0.5,
        kind: ParamKind::Knob {
            min: 0.0,
            max: 100.0,
            unit: "%",
            log_freq: false,
        },
    },
    ParamDesc {
        name: "Stereo",
        default_norm: 0.0,
        kind: ParamKind::Knob {
            min: 0.0,
            max: 180.0,
            unit: "°",
            log_freq: false,
        },
    },
    ParamDesc {
        name: "Shape",
        default_norm: 0.0,
        kind: ParamKind::Enum(&["Sine", "Tri", "Sq"]),
    },
];

const RING_MOD_PARAMS: &[ParamDesc] = &[
    ParamDesc {
        name: "Freq",
        default_norm: 0.560,
        kind: ParamKind::Knob {
            min: 20.0,
            max: 5_000.0,
            unit: "Hz",
            log_freq: true,
        },
    },
    ParamDesc {
        name: "Wave",
        default_norm: 0.0,
        kind: ParamKind::Enum(&["Sine", "Tri", "Saw", "Sq"]),
    },
    ParamDesc {
        name: "Mix",
        default_norm: 0.5,
        kind: ParamKind::Knob {
            min: 0.0,
            max: 100.0,
            unit: "%",
            log_freq: false,
        },
    },
];

fn spec_for(name: &str) -> Option<DeviceSpec> {
    match name {
        "Tool" | "Hikaru Tool" => Some(DeviceSpec {
            canonical_name: "Tool",
            card_width: 210.0,
            accent: theme::BW_LCD_CYAN,
            params: TOOL_PARAMS,
        }),
        "DC Offset" | "Hikaru DC Offset" => Some(DeviceSpec {
            canonical_name: "DC Offset",
            card_width: 156.0,
            accent: theme::BW_LCD_CYAN,
            params: DC_OFFSET_PARAMS,
        }),
        "Dual Pan" | "Hikaru Dual Pan" => Some(DeviceSpec {
            canonical_name: "Dual Pan",
            card_width: 196.0,
            accent: theme::BW_LCD_CYAN,
            params: DUAL_PAN_PARAMS,
        }),
        "Test Tone" | "Hikaru Test Tone" => Some(DeviceSpec {
            canonical_name: "Test Tone",
            card_width: 204.0,
            accent: theme::ACCENT_ORANGE,
            params: TEST_TONE_PARAMS,
        }),
        "Time Shift" | "Hikaru Time Shift" => Some(DeviceSpec {
            canonical_name: "Time Shift",
            card_width: 196.0,
            accent: theme::BW_LCD_CYAN,
            params: TIME_SHIFT_PARAMS,
        }),
        "Tremolo" | "Hikaru Tremolo" => Some(DeviceSpec {
            canonical_name: "Tremolo",
            card_width: 204.0,
            accent: theme::ACCENT_ORANGE,
            params: TREMOLO_PARAMS,
        }),
        "Ring-Mod" | "Ring Mod" | "Hikaru Ring-Mod" => Some(DeviceSpec {
            canonical_name: "Ring-Mod",
            card_width: 196.0,
            accent: theme::ACCENT_ORANGE,
            params: RING_MOD_PARAMS,
        }),
        _ => None,
    }
}

pub fn is_tier_s_fx(name: &str) -> bool {
    spec_for(name).is_some()
}

pub fn card_width(name: &str) -> Option<f32> {
    spec_for(name).map(|s| s.card_width)
}

type StateMap = Mutex<HashMap<(usize, usize, String), Vec<f32>>>;
type LoadedSet = Mutex<HashSet<(usize, usize, String)>>;
type WetStateMap = Mutex<HashMap<(usize, usize, usize, String), Vec<f32>>>;

fn state_map() -> &'static StateMap {
    static CELL: OnceLock<StateMap> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(HashMap::new()))
}

fn loaded_set() -> &'static LoadedSet {
    static CELL: OnceLock<LoadedSet> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(HashSet::new()))
}

fn wet_state_map() -> &'static WetStateMap {
    static CELL: OnceLock<WetStateMap> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn clear_wet_fx_state(track_idx: usize, parent_slot_idx: usize) {
    let mut map = wet_state_map().lock().unwrap();
    map.retain(|k, _| !(k.0 == track_idx && k.1 == parent_slot_idx));
}

pub fn remove_wet_fx_slot_state(track_idx: usize, parent_slot_idx: usize, wet_idx: usize) {
    let mut map = wet_state_map().lock().unwrap();
    let old_entries: Vec<((usize, usize, usize, String), Vec<f32>)> = map
        .iter()
        .filter(|(k, _)| k.0 == track_idx && k.1 == parent_slot_idx)
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    map.retain(|k, _| !(k.0 == track_idx && k.1 == parent_slot_idx));
    for ((t, p, w, name), vals) in old_entries {
        if w < wet_idx {
            map.insert((t, p, w, name), vals);
        } else if w > wet_idx {
            map.insert((t, p, w - 1, name), vals);
        }
    }
}

fn format_param_value(desc: &ParamDesc, norm: f32) -> String {
    match desc.kind {
        ParamKind::Knob {
            min,
            max,
            unit,
            log_freq,
        } => {
            let val = if log_freq {
                min * (max / min).powf(norm.clamp(0.0, 1.0))
            } else {
                min + norm.clamp(0.0, 1.0) * (max - min)
            };
            if log_freq && val >= 1000.0 {
                format!("{:.1} kHz", val / 1000.0)
            } else if (max - min) <= 20.0 {
                format!("{val:.1} {unit}")
            } else {
                format!("{val:.0} {unit}")
            }
        }
        ParamKind::Toggle => {
            if norm >= 0.5 {
                "ON".to_string()
            } else {
                "OFF".to_string()
            }
        }
        ParamKind::Enum(labels) => {
            let idx = ((norm * (labels.len().saturating_sub(1)) as f32).round() as usize)
                .min(labels.len().saturating_sub(1));
            labels[idx].to_string()
        }
    }
}

pub fn render_module(
    ui: &mut egui::Ui,
    app: &mut AppState,
    track_idx: usize,
    slot_idx: usize,
    slot_name: &str,
) {
    let Some(spec) = spec_for(slot_name) else {
        return;
    };

    let key = (track_idx, slot_idx, spec.canonical_name.to_string());
    let mut values = {
        let mut map = state_map().lock().unwrap();
        map.entry(key.clone())
            .or_insert_with(|| spec.params.iter().map(|p| p.default_norm).collect())
            .clone()
    };

    let engine_track_idx = track_idx.checked_sub(1);
    if let Some(et_idx) = engine_track_idx {
        if loaded_set().lock().unwrap().insert(key.clone()) {
            app.audio_proxy.send(crate::audio_proxy::GuiCommand::LoadNativeEffect {
                track_idx: et_idx,
                slot_idx,
                name: spec.canonical_name.to_string(),
            });
            for (i, &v) in values.iter().enumerate() {
                app.audio_proxy.send(crate::audio_proxy::GuiCommand::SetExternalPluginParam {
                    track_idx: et_idx,
                    slot_idx,
                    param_id: i as u32,
                    value: v.clamp(0.0, 1.0),
                });
            }
        }
    }

    let changed = render_params_ui(ui, &spec, &mut values);

    if changed {
        state_map().lock().unwrap().insert(key, values.clone());
        if let Some(et_idx) = engine_track_idx {
            for (i, &v) in values.iter().enumerate() {
                app.audio_proxy.send(crate::audio_proxy::GuiCommand::SetExternalPluginParam {
                    track_idx: et_idx,
                    slot_idx,
                    param_id: i as u32,
                    value: v.clamp(0.0, 1.0),
                });
            }
        }
    }
}

pub fn render_wet_fx_module(
    ui: &mut egui::Ui,
    app: &mut AppState,
    track_idx: usize,
    parent_slot_idx: usize,
    wet_idx: usize,
    slot_name: &str,
) {
    let Some(spec) = spec_for(slot_name) else {
        return;
    };

    let key = (
        track_idx,
        parent_slot_idx,
        wet_idx,
        spec.canonical_name.to_string(),
    );
    let mut values = {
        let mut map = wet_state_map().lock().unwrap();
        map.entry(key.clone())
            .or_insert_with(|| spec.params.iter().map(|p| p.default_norm).collect())
            .clone()
    };

    let changed = render_params_ui(ui, &spec, &mut values);

    if changed {
        wet_state_map().lock().unwrap().insert(key, values.clone());
        if let Some(et_idx) = track_idx.checked_sub(1) {
            for (i, &v) in values.iter().enumerate() {
                app.audio_proxy.send(crate::audio_proxy::GuiCommand::SetBit8WetFxParam {
                    track_idx: et_idx,
                    slot_idx: parent_slot_idx,
                    wet_idx,
                    param_id: i as u32,
                    value: v.clamp(0.0, 1.0),
                });
            }
        }
    }
}

fn render_params_ui(ui: &mut egui::Ui, spec: &DeviceSpec, values: &mut [f32]) -> bool {
    let mut changed = false;

    ui.vertical_centered(|ui| {
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(spec.canonical_name)
                .size(11.0)
                .strong()
                .color(spec.accent),
        );
        ui.add_space(6.0);

        let cols = if spec.params.len() <= 3 {
            spec.params.len().max(1)
        } else {
            3
        };
        let avail_w = ui.available_width().max(120.0);
        let col_w = (avail_w / cols as f32).floor();
        let row_h = 62.0;

        for (row_idx, chunk) in spec.params.chunks(cols).enumerate() {
            let (row_rect, _) =
                ui.allocate_exact_size(egui::vec2(avail_w, row_h), egui::Sense::hover());
            for (c_idx, desc) in chunk.iter().enumerate() {
                let p_idx = row_idx * cols + c_idx;
                let cell_rect = egui::Rect::from_min_size(
                    egui::pos2(row_rect.min.x + c_idx as f32 * col_w, row_rect.min.y),
                    egui::vec2(col_w, row_h),
                );
                let mut cell_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(cell_rect)
                        .layout(egui::Layout::top_down(egui::Align::Center)),
                );
                cell_ui.spacing_mut().item_spacing.y = 2.0;

                let val = &mut values[p_idx];
                match desc.kind {
                    ParamKind::Knob { .. } => {
                        let mut pct = *val * 100.0;
                        let (k_resp, k_changed) = crate::ui::bitwig_kit::rotary_knob(
                            &mut cell_ui,
                            30.0,
                            &mut pct,
                            0.0..=100.0,
                            desc.default_norm * 100.0,
                            spec.accent,
                            false,
                        );
                        if k_changed {
                            *val = (pct / 100.0).clamp(0.0, 1.0);
                            changed = true;
                        }
                        let readout = format_param_value(desc, *val);
                        k_resp.on_hover_text(format!("{}: {readout}", desc.name));
                        cell_ui.label(
                            egui::RichText::new(desc.name)
                                .font(crate::ui::fonts::semibold(9.0))
                                .color(theme::TEXT_SECONDARY),
                        );
                        cell_ui.label(
                            egui::RichText::new(readout)
                                .font(crate::ui::fonts::regular(8.5))
                                .color(theme::TEXT_MUTED),
                        );
                    }
                    ParamKind::Toggle => {
                        let active = *val >= 0.5;
                        cell_ui.add_space(6.0);
                        let btn = egui::Button::new(
                            egui::RichText::new(if active { "ON" } else { "OFF" })
                                .size(9.5)
                                .color(if active {
                                    egui::Color32::BLACK
                                } else {
                                    theme::TEXT_SECONDARY
                                }),
                        )
                        .fill(if active {
                            spec.accent
                        } else {
                            egui::Color32::from_rgb(0x26, 0x29, 0x30)
                        })
                        .min_size(egui::vec2(40.0, 22.0));
                        if cell_ui.add(btn).clicked() {
                            *val = if active { 0.0 } else { 1.0 };
                            changed = true;
                        }
                        cell_ui.add_space(2.0);
                        cell_ui.label(
                            egui::RichText::new(desc.name)
                                .font(crate::ui::fonts::semibold(9.0))
                                .color(theme::TEXT_SECONDARY),
                        );
                    }
                    ParamKind::Enum(labels) => {
                        let count = labels.len().max(1);
                        let cur = ((*val * (count - 1) as f32).round() as usize).min(count - 1);
                        cell_ui.add_space(6.0);
                        let btn = egui::Button::new(
                            egui::RichText::new(labels[cur])
                                .size(9.5)
                                .color(theme::TEXT_PRIMARY),
                        )
                        .fill(egui::Color32::from_rgb(0x26, 0x29, 0x30))
                        .min_size(egui::vec2(46.0, 22.0));
                        if cell_ui.add(btn).clicked() {
                            let next = (cur + 1) % count;
                            *val = if count > 1 {
                                next as f32 / (count - 1) as f32
                            } else {
                                0.0
                            };
                            changed = true;
                        }
                        cell_ui.add_space(2.0);
                        cell_ui.label(
                            egui::RichText::new(desc.name)
                                .font(crate::ui::fonts::semibold(9.0))
                                .color(theme::TEXT_SECONDARY),
                        );
                    }
                }
            }
        }
    });

    changed
}
