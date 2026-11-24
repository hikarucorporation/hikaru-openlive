// crates/hikaru_gui/src/views/external_plugin_card.rs
//
// Bitwig Studio 5.3.13-style External VST3/CLAP Plugin Device Card:
// 1. Vertical left sidebar (Power/Bypass, Preset Folder menu, rotated title,
//    bottom 3 toggles: Show Plug-in Window, Show Remote Controls, Show Modulators)
// 2. Collapsible Device Remote Controls 2×4 grid (`init` + `Perform ▾`)
// 3. Collapsible Device Modulator Bay (3-row `+` slots, cyan modulation routing)
// 4. Collapsible Public Parameters inspector (search bar + virtualized rotary knobs
//    with bidirectional native window sync, modulation arcs, remote mapping & automation)
// GNU AGPLv3

use egui::{
    pos2, vec2, Align2, Color32, CursorIcon, Painter, Pos2, Rect, RichText, Sense, Stroke, Ui,
};

use crate::app::AppState;
use crate::audio_proxy::GuiCommand;
use crate::theme;
use crate::ui::{fonts, icons};
use crate::views::automation_editor;
use crate::views::dsp_rack;
use crate::views::external_plugins_settings::{
    capture_host_parent, slot_window_key, wet_fx_window_key,
};
use crate::views::hikaru_native_plugins::hikaru_openmodulation::{self, MOD_ARROW_CYAN};
use crate::views::mixer::{DspSlot, ExternalParamsStatus};
use crate::views::remote_controls::{self, RemoteTarget, REMOTES_BODY_WIDTH};

pub const EXT_SIDEBAR_W: f32 = 26.0;
pub const EXT_PARAMS_EXPANDED_W: f32 = 190.0;
pub const EXT_PARAMS_COLLAPSED_W: f32 = 22.0;

const BITWIG_ORANGE: Color32 = Color32::from_rgb(0xFF, 0x72, 0x16);
const SIDEBAR_BG_SEL: Color32 = Color32::from_rgb(0x36, 0x39, 0x41);
const SIDEBAR_BG_IDLE: Color32 = Color32::from_rgb(0x28, 0x2A, 0x30);
const PARAMS_WELL_BG: Color32 = Color32::from_rgb(0x22, 0x24, 0x29);
const TOOLBAR_BG: Color32 = Color32::from_rgb(0x33, 0x36, 0x3E);
const SEARCH_BG: Color32 = Color32::from_rgb(0x16, 0x18, 0x1C);
const ROW_BG_EVEN: Color32 = Color32::from_rgb(0x26, 0x28, 0x2E);
const ROW_BG_ODD: Color32 = Color32::from_rgb(0x23, 0x25, 0x2B);
const ROW_HOVER_BG: Color32 = Color32::from_rgb(0x30, 0x33, 0x3B);
const DIVIDER_DARK: Color32 = Color32::from_rgb(0x1A, 0x1C, 0x20);

/// Identifica si la tarjeta de plugin externo reside en la cadena principal del rack
/// o anidada dentro de `Wet FX` (p. ej. `Hikaru Open8-Bit`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ExternalSlotTarget {
    TopLevel {
        track_idx: usize,
        slot_idx: usize,
    },
    Bit8WetFx {
        track_idx: usize,
        parent_slot_idx: usize,
        wet_idx: usize,
    },
}

impl ExternalSlotTarget {
    fn track_idx(self) -> usize {
        match self {
            Self::TopLevel { track_idx, .. } | Self::Bit8WetFx { track_idx, .. } => track_idx,
        }
    }

    fn removal_index(self) -> usize {
        match self {
            Self::TopLevel { slot_idx, .. } => slot_idx,
            Self::Bit8WetFx { wet_idx, .. } => wet_idx,
        }
    }

    fn top_level_slot(self) -> Option<(usize, usize)> {
        match self {
            Self::TopLevel {
                track_idx,
                slot_idx,
            } => Some((track_idx, slot_idx)),
            Self::Bit8WetFx { .. } => None,
        }
    }

    fn window_key(self) -> String {
        match self {
            Self::TopLevel {
                track_idx,
                slot_idx,
            } => slot_window_key(track_idx, slot_idx),
            Self::Bit8WetFx {
                track_idx,
                parent_slot_idx,
                wet_idx,
            } => wet_fx_window_key(track_idx, parent_slot_idx, wet_idx),
        }
    }

    fn slot_cloned(self, app: &AppState) -> Option<DspSlot> {
        match self {
            Self::TopLevel {
                track_idx,
                slot_idx,
            } => app
                .current_tracks()
                .get(track_idx)
                .and_then(|t| t.effects.get(slot_idx))
                .cloned(),
            Self::Bit8WetFx {
                track_idx,
                parent_slot_idx,
                wet_idx,
            } => app
                .current_tracks()
                .get(track_idx)
                .and_then(|t| t.effects.get(parent_slot_idx))
                .and_then(|p| p.wet_fx_chain.get(wet_idx))
                .cloned(),
        }
    }

    fn slot_mut(self, app: &mut AppState) -> Option<&mut DspSlot> {
        match self {
            Self::TopLevel {
                track_idx,
                slot_idx,
            } => app.slot_mut(track_idx, slot_idx),
            Self::Bit8WetFx {
                track_idx,
                parent_slot_idx,
                wet_idx,
            } => app.wet_fx_slot_mut(track_idx, parent_slot_idx, wet_idx),
        }
    }
}

/// Ancho dinámico de la tarjeta de un plugin externo VST3/CLAP según qué paneles
/// auxiliares estén abiertos (Remotes 2×4, Device Modulator Bay y lista de parámetros).
pub fn card_width(slot: &DspSlot, track_idx: usize, slot_idx: usize) -> f32 {
    let mut w = EXT_SIDEBAR_W;
    if slot.remotes.remotes_open {
        w += REMOTES_BODY_WIDTH;
    }
    if slot.modulators_open {
        hikaru_openmodulation::ensure_empty_bay_for_slot(track_idx, slot_idx);
        w += hikaru_openmodulation::bay_width_for_slot(track_idx, slot_idx) + 2.0;
    }
    w += if slot.params_expanded {
        EXT_PARAMS_EXPANDED_W
    } else {
        EXT_PARAMS_COLLAPSED_W
    };
    w
}

/// Ancho dinámico de la tarjeta de un plugin externo VST3/CLAP anidado dentro de `Wet FX`.
pub fn wet_fx_card_width(slot: &DspSlot) -> f32 {
    EXT_SIDEBAR_W
        + if slot.params_expanded {
            EXT_PARAMS_EXPANDED_W
        } else {
            EXT_PARAMS_COLLAPSED_W
        }
}

/// Dibuja el ícono cuadrado de ventana del plugin (estilo Bitwig Screenshot 4:
/// pequeño rectángulo con barra de título superior).
fn paint_plugin_window_icon(
    painter: &Painter,
    center: Pos2,
    is_open: bool,
    is_opening: bool,
    hovered: bool,
) {
    let r = Rect::from_center_size(center, vec2(11.0, 9.5));
    let (fill, stroke, bar) = if is_open {
        (
            Color32::from_rgba_unmultiplied(0xFF, 0x72, 0x16, 48),
            BITWIG_ORANGE,
            BITWIG_ORANGE,
        )
    } else if is_opening {
        (
            Color32::from_rgba_unmultiplied(0xF5, 0xB8, 0x41, 40),
            Color32::from_rgb(0xF5, 0xB8, 0x41),
            Color32::from_rgb(0xF5, 0xB8, 0x41),
        )
    } else if hovered {
        (
            Color32::from_rgb(0x3A, 0x3E, 0x46),
            Color32::from_rgb(0xE2, 0xE6, 0xEE),
            Color32::from_rgb(0xE2, 0xE6, 0xEE),
        )
    } else {
        (
            Color32::from_rgb(0x2E, 0x31, 0x38),
            Color32::from_rgb(0x9A, 0x9F, 0xAA),
            Color32::from_rgb(0x9A, 0x9F, 0xAA),
        )
    };
    painter.rect_filled(r, 1.5, fill);
    painter.rect_stroke(r, 1.5, Stroke::new(1.15_f32, stroke), egui::StrokeKind::Inside);
    painter.rect_filled(
        Rect::from_min_size(r.min, vec2(r.width(), 2.6)),
        0.5,
        bar,
    );
}

fn paint_rotated_title(
    painter: &Painter,
    center_x: f32,
    bottom_y: f32,
    max_chars: usize,
    text: &str,
    font_size: f32,
    color: Color32,
) {
    let display: String = if text.chars().count() > max_chars {
        let mut s: String = text.chars().take(max_chars.saturating_sub(1)).collect();
        s.push('…');
        s
    } else {
        text.to_string()
    };
    let galley = painter.layout_no_wrap(display, fonts::semibold(font_size), color);
    let mut ts = egui::epaint::TextShape::new(
        pos2(center_x - font_size * 0.58, bottom_y),
        galley,
        color,
    );
    ts.angle = -std::f32::consts::FRAC_PI_2;
    painter.add(egui::Shape::Text(ts));
}

/// Dibuja un knob rotatorio compacto estilo Bitwig con soporte para:
/// - Arco naranja de valor base
/// - Arcos cian de rango + señal en vivo de modulación
/// - Tapa cian en modo mapping de moduladores (drag bipolar de profundidad)
#[allow(clippy::too_many_arguments)]
fn paint_param_knob(
    painter: &Painter,
    center: Pos2,
    radius: f32,
    base_norm: f32,
    mod_range_amount: Option<f32>,
    mod_live_delta: Option<f32>,
    mapping_mode: bool,
    mapping_amount: f32,
    hovered: bool,
) {
    let norm = base_norm.clamp(0.0, 1.0);
    let start_a = std::f32::consts::FRAC_PI_4 * 3.0; // 135°
    let sweep = std::f32::consts::PI * 1.5;          // 270°

    // Outer recessed track arc
    let track_pts: Vec<Pos2> = (0..=20)
        .map(|i| {
            let a = start_a + sweep * (i as f32 / 20.0);
            pos2(center.x + radius * a.cos(), center.y + radius * a.sin())
        })
        .collect();
    painter.add(egui::Shape::line(
        track_pts,
        Stroke::new(2.0_f32, Color32::from_rgb(0x46, 0x4A, 0x54)),
    ));

    // Inner metallic cap
    let cap_r = (radius - 2.6).max(3.5);
    let cap_fill = if mapping_mode {
        Color32::from_rgb(0x2B, 0x8F, 0xB8)
    } else if hovered {
        Color32::from_rgb(0x3B, 0x3F, 0x48)
    } else {
        Color32::from_rgb(0x2E, 0x31, 0x38)
    };
    painter.circle_filled(center, cap_r, cap_fill);
    painter.circle_stroke(
        center,
        cap_r,
        Stroke::new(
            1.0_f32,
            if mapping_mode {
                MOD_ARROW_CYAN
            } else {
                Color32::from_rgb(0x16, 0x18, 0x1C)
            },
        ),
    );

    // Base orange value arc (from 0 to norm)
    if norm > 0.005 {
        let steps = ((norm * 20.0).ceil() as usize).max(2);
        let fg_pts: Vec<Pos2> = (0..=steps)
            .map(|i| {
                let t = (i as f32 / steps as f32) * norm;
                let a = start_a + sweep * t;
                pos2(center.x + radius * a.cos(), center.y + radius * a.sin())
            })
            .collect();
        painter.add(egui::Shape::line(fg_pts, Stroke::new(2.2_f32, BITWIG_ORANGE)));
    }

    // Helper for drawing a cyan arc from `from_n` to `to_n`
    let paint_arc = |from_n: f32, to_n: f32, stroke: Stroke| {
        let a_n = from_n.clamp(0.0, 1.0);
        let b_n = to_n.clamp(0.0, 1.0);
        if (b_n - a_n).abs() < 0.004 {
            return;
        }
        let steps = (((b_n - a_n).abs() * 24.0).ceil() as usize).max(2);
        let pts: Vec<Pos2> = (0..=steps)
            .map(|i| {
                let t = i as f32 / steps as f32;
                let n = a_n + (b_n - a_n) * t;
                let a = start_a + sweep * n;
                pos2(center.x + radius * a.cos(), center.y + radius * a.sin())
            })
            .collect();
        painter.add(egui::Shape::line(pts, stroke));
    };

    if mapping_mode && mapping_amount.abs() > 0.004 {
        paint_arc(
            norm,
            norm + mapping_amount,
            Stroke::new(2.5_f32, MOD_ARROW_CYAN),
        );
    } else if let Some(range_amt) = mod_range_amount {
        // Translucent cyan modulation range
        paint_arc(
            norm,
            norm + range_amt,
            Stroke::new(2.3_f32, Color32::from_rgba_unmultiplied(0x46, 0xC8, 0xF5, 95)),
        );
        // Live animated cyan signal + tip dot
        if let Some(delta) = mod_live_delta {
            let eff = (norm + delta).clamp(0.0, 1.0);
            paint_arc(norm, eff, Stroke::new(2.4_f32, MOD_ARROW_CYAN));
            let tip_a = start_a + sweep * eff;
            let tip = pos2(center.x + radius * tip_a.cos(), center.y + radius * tip_a.sin());
            painter.circle_filled(tip, 1.7, Color32::WHITE);
        }
    }

    // Crisp white pointer needle at base_norm
    let needle_a = start_a + sweep * norm;
    let p_in = pos2(
        center.x + (cap_r * 0.25) * needle_a.cos(),
        center.y + (cap_r * 0.25) * needle_a.sin(),
    );
    let p_out = pos2(
        center.x + (cap_r - 0.6) * needle_a.cos(),
        center.y + (cap_r - 0.6) * needle_a.sin(),
    );
    painter.line_segment([p_in, p_out], Stroke::new(1.5_f32, Color32::from_rgb(0xEC, 0xEF, 0xF5)));
}

/// Renderiza una tarjeta completa de plugin externo VST3/CLAP en el DSP Rack
/// replicando fielmente el flujo de Bitwig Studio 5.3.13 (Screenshots 1–4).
#[allow(clippy::too_many_arguments)]
pub fn render_card(
    ui: &mut Ui,
    app: &mut AppState,
    card_rect: Rect,
    track_idx: usize,
    slot_idx: usize,
    is_selected: bool,
    slot_to_remove: &mut Option<usize>,
) {
    render_card_impl(
        ui,
        app,
        card_rect,
        ExternalSlotTarget::TopLevel {
            track_idx,
            slot_idx,
        },
        is_selected,
        slot_to_remove,
    );
}

/// Renderiza la misma tarjeta dedicada de plugin externo VST3/CLAP dentro de la cadena
/// anidada `Wet FX` de `Hikaru Open8-Bit` (idéntica a Bitwig Studio).
#[allow(clippy::too_many_arguments)]
pub fn render_wet_fx_card(
    ui: &mut Ui,
    app: &mut AppState,
    card_rect: Rect,
    track_idx: usize,
    parent_slot_idx: usize,
    wet_idx: usize,
    wet_to_remove: &mut Option<usize>,
) {
    render_card_impl(
        ui,
        app,
        card_rect,
        ExternalSlotTarget::Bit8WetFx {
            track_idx,
            parent_slot_idx,
            wet_idx,
        },
        false,
        wet_to_remove,
    );
}

#[allow(clippy::too_many_arguments)]
fn render_card_impl(
    ui: &mut Ui,
    app: &mut AppState,
    card_rect: Rect,
    target: ExternalSlotTarget,
    is_selected: bool,
    slot_to_remove: &mut Option<usize>,
) {
    let Some(slot_snap) = target.slot_cloned(app) else {
        return;
    };
    let Some(ext_desc) = slot_snap.external.clone() else {
        return;
    };

    let track_idx = target.track_idx();
    let track_name = app
        .current_tracks()
        .get(track_idx)
        .map(|t| t.name.clone())
        .unwrap_or_else(|| "Track".to_string());
    let eng_track = track_idx.checked_sub(1);
    let wkey = target.window_key();
    let win_open = app.plugin_settings_state.is_window_open(&wkey);
    let win_opening = app.plugin_settings_state.is_window_opening(&wkey);
    let gui_err = app.plugin_settings_state.gui_error_for(&wkey);

    // =========================================================================
    // 1. LEFT VERTICAL SIDEBAR (26px, estilo Bitwig Screenshots 1–4)
    // =========================================================================
    let sb_rect = Rect::from_min_size(card_rect.min, vec2(EXT_SIDEBAR_W, card_rect.height()));
    {
        let p = ui.painter();
        p.rect_filled(
            sb_rect,
            egui::CornerRadius {
                nw: 3,
                ne: 0,
                sw: 3,
                se: 0,
            },
            if is_selected {
                SIDEBAR_BG_SEL
            } else {
                SIDEBAR_BG_IDLE
            },
        );
        p.line_segment(
            [pos2(sb_rect.max.x, sb_rect.min.y), pos2(sb_rect.max.x, sb_rect.max.y)],
            Stroke::new(1.0_f32, DIVIDER_DARK),
        );
    }

    // 1a. Power / Bypass button (top)
    let pwr_r = Rect::from_center_size(pos2(sb_rect.center().x, sb_rect.min.y + 13.0), vec2(15.0, 15.0));
    let pwr_resp = ui.interact(
        pwr_r,
        ui.id().with(("ext_sb_pwr", target)),
        Sense::click(),
    );
    icons::paint_power_switch(
        ui.painter(),
        pwr_r,
        if slot_snap.active {
            BITWIG_ORANGE
        } else {
            Color32::from_rgb(0x46, 0x4A, 0x54)
        },
        if slot_snap.active {
            Color32::from_rgb(0x14, 0x12, 0x10)
        } else {
            Color32::from_rgb(0xB8, 0xBC, 0xC4)
        },
    );
    if pwr_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        app.footer_context_help = Some((
            "ACTIVE".to_string(),
            if slot_snap.active { "On".to_string() } else { "Off".to_string() },
        ));
        app.footer_context_cues = vec![("CLICK".to_string(), "Toggle bypass".to_string())];
    }
    if pwr_resp.on_hover_text("Device Active / Bypass").clicked() {
        let next_active = !slot_snap.active;
        if let Some(s) = target.slot_mut(app) {
            s.active = next_active;
        }
        if let Some(et) = eng_track {
            match target {
                ExternalSlotTarget::TopLevel { slot_idx, .. } => {
                    app.audio_proxy.send(GuiCommand::SetExternalPluginActive {
                        track_idx: et,
                        slot_idx,
                        active: next_active,
                    });
                }
                ExternalSlotTarget::Bit8WetFx {
                    parent_slot_idx,
                    wet_idx,
                    ..
                } => {
                    app.audio_proxy.send(GuiCommand::SetBit8WetFxActive {
                        track_idx: et,
                        slot_idx: parent_slot_idx,
                        wet_idx,
                        active: next_active,
                    });
                }
            }
        }
        app.project_dirty = true;
    }

    // 1b. Preset / Plugin Menu folder icon
    let fld_r = Rect::from_center_size(pos2(sb_rect.center().x, sb_rect.min.y + 32.0), vec2(14.0, 13.0));
    let fld_resp = ui
        .interact(
            fld_r,
            ui.id().with(("ext_sb_fld", target)),
            Sense::click(),
        )
        .on_hover_text("Plug-in Options & Presets");
    icons::paint_icon(
        ui.painter(),
        fld_r,
        icons::FOLDER,
        if fld_resp.hovered() {
            Color32::WHITE
        } else {
            Color32::from_rgb(0xA4, 0xA9, 0xB4)
        },
        1.5,
    );
    if fld_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let menu_popup_id = ui.id().with(("ext_sb_menu", target));
    if fld_resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(menu_popup_id));
    }
    let mut trigger_toggle_window = false;
    let mut trigger_rescan_params = false;
    let mut trigger_reload_engine = false;
    let mut trigger_save_chain = false;
    let mut trigger_load_chain = false;
    egui::popup_below_widget(
        ui,
        menu_popup_id,
        &fld_resp,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |pop_ui| {
            pop_ui.set_min_width(195.0);
            let fmt = match ext_desc.format {
                hikaru_plugin_host::PluginFormat::VST3 => "VST3",
                hikaru_plugin_host::PluginFormat::CLAP => "CLAP",
            };
            pop_ui.label(
                RichText::new(format!("{}  [{}]", slot_snap.name, fmt))
                    .font(fonts::semibold(10.5))
                    .color(BITWIG_ORANGE),
            );
            if !ext_desc.vendor.trim().is_empty() {
                pop_ui.label(
                    RichText::new(&ext_desc.vendor)
                        .font(fonts::regular(9.5))
                        .color(theme::TEXT_MUTED),
                );
            }
            pop_ui.separator();
            let win_lbl = if win_open {
                "Hide Plug-in Window"
            } else {
                "Show Plug-in Window"
            };
            if pop_ui.button(win_lbl).clicked() {
                trigger_toggle_window = true;
                pop_ui.close_menu();
            }
            if pop_ui.button("Rescan Public Parameters").clicked() {
                trigger_rescan_params = true;
                pop_ui.close_menu();
            }
            if pop_ui.button("Reload Plug-in in Audio Engine").clicked() {
                trigger_reload_engine = true;
                pop_ui.close_menu();
            }
            pop_ui.separator();
            if pop_ui.button("Save Track Chain (*.hdp)...").clicked() {
                trigger_save_chain = true;
                pop_ui.close_menu();
            }
            if pop_ui.button("Load Track Chain (*.hdp)...").clicked() {
                trigger_load_chain = true;
                pop_ui.close_menu();
            }
            pop_ui.separator();
            pop_ui.label(
                RichText::new(ext_desc.path.display().to_string())
                    .font(fonts::regular(8.5))
                    .color(theme::TEXT_DISABLED),
            );
        },
    );

    // 1c. Rotated Vertical Plugin Title (middle)
    let title_r = Rect::from_min_max(
        pos2(sb_rect.min.x, sb_rect.min.y + 44.0),
        pos2(sb_rect.max.x, sb_rect.max.y - 54.0),
    );
    let title_resp = ui.interact(
        title_r,
        ui.id().with(("ext_sb_title", target)),
        Sense::click(),
    );
    paint_rotated_title(
        ui.painter(),
        sb_rect.center().x,
        title_r.center().y + 18.0,
        16,
        &slot_snap.name,
        9.5,
        if is_selected {
            Color32::WHITE
        } else {
            Color32::from_rgb(0xC8, 0xCC, 0xD4)
        },
    );
    if title_resp.clicked() {
        match target {
            ExternalSlotTarget::TopLevel { slot_idx, .. } => {
                app.selected_slot_index = slot_idx;
            }
            ExternalSlotTarget::Bit8WetFx {
                parent_slot_idx, ..
            } => {
                app.selected_slot_index = parent_slot_idx;
            }
        }
        app.matrix_state.selected_scene = None;
    }

    // 1d. Bottom 3 stacked Bitwig Toggle Icons (Screenshots 1–4):
    //   - Top:    SHOW PLUG-IN WINDOW (square window icon)
    //   - Middle: SHOW REMOTE CONTROLS (knob-dot icon)
    //   - Bottom: SHOW MODULATORS (`o→` icon)
    let win_btn_r = Rect::from_center_size(
        pos2(sb_rect.center().x, sb_rect.max.y - 43.0),
        vec2(17.0, 15.0),
    );
    let win_resp = ui.interact(
        win_btn_r,
        ui.id().with(("ext_sb_win", target)),
        Sense::click(),
    );
    paint_plugin_window_icon(
        ui.painter(),
        win_btn_r.center(),
        win_open,
        win_opening,
        win_resp.hovered(),
    );
    if win_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        app.footer_context_help = Some((
            "SHOW PLUG-IN WINDOW".to_string(),
            if win_open {
                "On".to_string()
            } else if win_opening {
                "Opening...".to_string()
            } else {
                "Off".to_string()
            },
        ));
        app.footer_context_cues = vec![("CLICK".to_string(), "Toggle".to_string())];
    }
    if win_resp
        .on_hover_text(if win_open {
            "Hide Plug-in Window (X11/Wayland)"
        } else {
            "Show Plug-in Window (X11/Wayland)"
        })
        .clicked()
    {
        trigger_toggle_window = true;
    }

    let rem_btn_r = Rect::from_center_size(
        pos2(sb_rect.center().x, sb_rect.max.y - 26.0),
        vec2(17.0, 15.0),
    );
    let rem_resp = ui.interact(
        rem_btn_r,
        ui.id().with(("ext_sb_rem", target)),
        Sense::click(),
    );
    let remotes_open = target.top_level_slot().is_some() && slot_snap.remotes.remotes_open;
    remote_controls::paint_remote_toggle_icon(
        ui.painter(),
        rem_btn_r.center(),
        remotes_open,
        rem_resp.hovered(),
    );
    if rem_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        app.footer_context_help = Some((
            "SHOW REMOTE CONTROLS".to_string(),
            if remotes_open {
                "On".to_string()
            } else {
                "Off".to_string()
            },
        ));
        app.footer_context_cues = vec![("CLICK".to_string(), "Toggle".to_string())];
    }
    if rem_resp.on_hover_text("Show / Hide Device Remote Controls").clicked() {
        if let Some((tr, sl)) = target.top_level_slot() {
            if let Some(s) = app.slot_mut(tr, sl) {
                s.remotes.remotes_open = !s.remotes.remotes_open;
            }
        }
    }

    let mod_btn_r = Rect::from_center_size(
        pos2(sb_rect.center().x, sb_rect.max.y - 10.0),
        vec2(17.0, 15.0),
    );
    let mod_resp = ui.interact(
        mod_btn_r,
        ui.id().with(("ext_sb_mod", target)),
        Sense::click(),
    );
    let modulators_open = target.top_level_slot().is_some() && slot_snap.modulators_open;
    remote_controls::paint_mod_toggle_icon(
        ui.painter(),
        mod_btn_r.center(),
        modulators_open,
        mod_resp.hovered(),
    );
    if mod_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        app.footer_context_help = Some((
            "SHOW MODULATORS".to_string(),
            if modulators_open {
                "On".to_string()
            } else {
                "Off".to_string()
            },
        ));
        app.footer_context_cues = vec![("CLICK".to_string(), "Toggle".to_string())];
    }
    if mod_resp.on_hover_text("Show / Hide Device Modulators").clicked() {
        if let Some((tr, sl)) = target.top_level_slot() {
            hikaru_openmodulation::ensure_empty_bay_for_slot(tr, sl);
            if let Some(s) = app.slot_mut(tr, sl) {
                s.modulators_open = !s.modulators_open;
            }
        }
    }

    // Apply sidebar actions
    if trigger_toggle_window {
        if win_open || win_opening {
            app.plugin_settings_state.close_window(&wkey);
        } else {
            let parent = capture_host_parent();
            let tap_key = match target {
                ExternalSlotTarget::TopLevel { slot_idx, .. } => eng_track
                    .map(|et| hikaru_plugin_host::tap::TapKey::Slot(et, slot_idx))
                    .unwrap_or(hikaru_plugin_host::tap::NO_SLOT),
                ExternalSlotTarget::Bit8WetFx {
                    parent_slot_idx,
                    wet_idx,
                    ..
                } => eng_track
                    .map(|et| hikaru_plugin_host::tap::TapKey::WetFx(et, parent_slot_idx, wet_idx))
                    .unwrap_or(hikaru_plugin_host::tap::NO_SLOT),
            };
            let title = match target {
                ExternalSlotTarget::TopLevel { .. } => {
                    format!("{} / {}", track_name, slot_snap.name)
                }
                ExternalSlotTarget::Bit8WetFx { .. } => {
                    format!("{} / Wet FX / {}", track_name, slot_snap.name)
                }
            };
            let initial_state = slot_snap.saved_state.clone();
            let initial_params: Vec<(u32, f32)> = if initial_state.is_some() {
                slot_snap.rack_overrides_since_state.clone()
            } else {
                slot_snap
                    .cached_params
                    .iter()
                    .map(|p| (p.info.id, p.value))
                    .collect()
            };
            app.plugin_settings_state.open_plugin_for_slot(
                wkey.clone(),
                title,
                &ext_desc,
                parent,
                tap_key,
                initial_state,
                initial_params,
            );
        }
    }
    if trigger_rescan_params {
        app.param_probing.remove(&ext_desc.id);
        app.request_external_params(&ext_desc);
    }
    if trigger_reload_engine {
        if let Some(et) = eng_track {
            match target {
                ExternalSlotTarget::TopLevel { slot_idx, .. } => {
                    app.audio_proxy.send(GuiCommand::LoadExternalPlugin {
                        track_idx: et,
                        slot_idx,
                        descriptor: ext_desc.clone(),
                    });
                    if let Some(state) = slot_snap.saved_state.clone() {
                        app.audio_proxy.send(GuiCommand::LoadExternalPluginState {
                            track_idx: et,
                            slot_idx,
                            state,
                        });
                    }
                }
                ExternalSlotTarget::Bit8WetFx {
                    parent_slot_idx,
                    wet_idx,
                    ..
                } => {
                    app.audio_proxy.send(GuiCommand::LoadBit8WetExternalPlugin {
                        track_idx: et,
                        slot_idx: parent_slot_idx,
                        wet_idx,
                        descriptor: ext_desc.clone(),
                    });
                }
            }
            if let Some(s) = target.slot_mut(app) {
                s.engine_loaded = true;
                for p in &mut s.cached_params {
                    p.pending_to_engine = true;
                }
            }
        }
    }
    if trigger_save_chain {
        dsp_rack::save_device_chain_dialog(app, track_idx);
    }
    if trigger_load_chain {
        dsp_rack::load_device_chain_dialog(app, track_idx);
    }

    let mut cursor_x = sb_rect.max.x;

    // =========================================================================
    // 2. OPTIONAL DEVICE REMOTE CONTROLS 2×4 PANEL (Screenshot 3)
    // =========================================================================
    if let Some((tr, sl)) = target.top_level_slot() {
        if slot_snap.remotes.remotes_open {
            let rem_rect = Rect::from_min_size(
                pos2(cursor_x, card_rect.min.y + 1.0),
                vec2(REMOTES_BODY_WIDTH - 1.0, card_rect.height() - 2.0),
            );
            remote_controls::render_device_remotes_body(ui, rem_rect, app, tr, sl);
            ui.painter().line_segment(
                [pos2(rem_rect.max.x, card_rect.min.y), pos2(rem_rect.max.x, card_rect.max.y)],
                Stroke::new(1.0_f32, DIVIDER_DARK),
            );
            cursor_x += REMOTES_BODY_WIDTH;
        }
    }

    // =========================================================================
    // 3. OPTIONAL DEVICE MODULATOR BAY (Screenshot 2: 3 stacked `+` slots)
    // =========================================================================
    if let Some((tr, sl)) = target.top_level_slot() {
        if slot_snap.modulators_open {
            hikaru_openmodulation::ensure_empty_bay_for_slot(tr, sl);
            let mut request_add_modulator = false;
            let bay_w = hikaru_openmodulation::render_device_modulator_bay(
                ui,
                pos2(cursor_x + 2.0, card_rect.min.y + 3.0),
                (card_rect.height() - 6.0).max(120.0),
                tr,
                sl,
                &mut request_add_modulator,
            );
            if request_add_modulator {
                app.popup_browser.open_insert_modulator(tr, sl);
            }
            cursor_x += bay_w + 2.0;
            ui.painter().line_segment(
                [pos2(cursor_x, card_rect.min.y), pos2(cursor_x, card_rect.max.y)],
                Stroke::new(1.0_f32, DIVIDER_DARK),
            );
        }
    }

    // =========================================================================
    // 4. PUBLIC PARAMETERS PANEL (Screenshot 1: search bar + scrollable knobs)
    // =========================================================================
    let params_rect = Rect::from_min_max(
        pos2(cursor_x, card_rect.min.y + 1.0),
        pos2(card_rect.max.x - 1.0, card_rect.max.y - 1.0),
    );
    ui.painter().rect_filled(params_rect, 2.0, PARAMS_WELL_BG);

    if !slot_snap.params_expanded {
        // Collapsed slim strip with expand button (`←`) and remove button (`×`)
        let exp_r = Rect::from_center_size(
            pos2(params_rect.center().x, params_rect.min.y + 13.0),
            vec2(16.0, 16.0),
        );
        let exp_resp = ui
            .interact(
                exp_r,
                ui.id().with(("ext_params_expand", target)),
                Sense::click(),
            )
            .on_hover_text("Expand Public Parameters");
        icons::paint_icon(
            ui.painter(),
            exp_r.shrink(2.5),
            icons::ARROW_LEFT,
            if exp_resp.hovered() {
                Color32::WHITE
            } else {
                Color32::from_rgb(0xA4, 0xA9, 0xB4)
            },
            1.5,
        );
        if exp_resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if exp_resp.clicked() {
            if let Some(s) = target.slot_mut(app) {
                s.params_expanded = true;
            }
        }

        let close_r = Rect::from_center_size(
            pos2(params_rect.center().x, params_rect.max.y - 12.0),
            vec2(14.0, 14.0),
        );
        let close_resp = ui
            .interact(
                close_r,
                ui.id().with(("ext_close_collapsed", target)),
                Sense::click(),
            )
            .on_hover_text("Remove Plug-in (Del)");
        icons::paint_icon(
            ui.painter(),
            close_r.shrink(2.5),
            icons::X_CLOSE,
            if close_resp.hovered() {
                Color32::WHITE
            } else {
                theme::TEXT_MUTED
            },
            1.5,
        );
        if close_resp.clicked() {
            *slot_to_remove = Some(target.removal_index());
        }
        return;
    }

    // --- Top Toolbar (`[●] [Q search... ×] [→] [×]`) matching Bitwig Screenshot 1 ---
    let tb_h = 24.0;
    let tb_rect = Rect::from_min_size(params_rect.min, vec2(params_rect.width(), tb_h));
    ui.painter().rect_filled(tb_rect, 0.0, TOOLBAR_BG);
    ui.painter().line_segment(
        [pos2(tb_rect.min.x, tb_rect.max.y), pos2(tb_rect.max.x, tb_rect.max.y)],
        Stroke::new(1.0_f32, DIVIDER_DARK),
    );

    // Orange parameter-status dot button (left of search box, click rescans params)
    let dot_btn_r = Rect::from_center_size(pos2(tb_rect.min.x + 11.0, tb_rect.center().y), vec2(15.0, 15.0));
    let dot_resp = ui
        .interact(
            dot_btn_r,
            ui.id().with(("ext_params_dot", target)),
            Sense::click(),
        )
        .on_hover_text("Public Parameters (Click to rescan from plug-in)");
    remote_controls::paint_remote_toggle_icon(
        ui.painter(),
        dot_btn_r.center(),
        true,
        dot_resp.hovered(),
    );
    if dot_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if dot_resp.clicked() {
        app.param_probing.remove(&ext_desc.id);
        app.request_external_params(&ext_desc);
    }

    // Collapse arrow `→` and close `×` on the right of the toolbar
    let close_r = Rect::from_center_size(pos2(tb_rect.max.x - 9.0, tb_rect.center().y), vec2(13.0, 13.0));
    let close_resp = ui
        .interact(
            close_r,
            ui.id().with(("ext_close", target)),
            Sense::click(),
        )
        .on_hover_text("Remove Plug-in (Del)");
    icons::paint_icon(
        ui.painter(),
        close_r.shrink(2.5),
        icons::X_CLOSE,
        if close_resp.hovered() {
            Color32::WHITE
        } else {
            theme::TEXT_MUTED
        },
        1.5,
    );
    if close_resp.clicked() {
        *slot_to_remove = Some(target.removal_index());
    }

    let collapse_r = Rect::from_center_size(pos2(close_r.min.x - 9.0, tb_rect.center().y), vec2(14.0, 14.0));
    let collapse_resp = ui
        .interact(
            collapse_r,
            ui.id().with(("ext_params_collapse", target)),
            Sense::click(),
        )
        .on_hover_text("Collapse Parameters Panel");
    icons::paint_icon(
        ui.painter(),
        collapse_r.shrink(2.5),
        icons::ARROW_RIGHT,
        if collapse_resp.hovered() {
            Color32::WHITE
        } else {
            Color32::from_rgb(0xA4, 0xA9, 0xB4)
        },
        1.5,
    );
    if collapse_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if collapse_resp.clicked() {
        if let Some(s) = target.slot_mut(app) {
            s.params_expanded = false;
        }
    }

    // Search box between dot button and collapse arrow
    let search_r = Rect::from_min_max(
        pos2(dot_btn_r.max.x + 3.0, tb_rect.min.y + 3.0),
        pos2(collapse_r.min.x - 3.0, tb_rect.max.y - 3.0),
    );
    ui.painter().rect_filled(search_r, 2.0, SEARCH_BG);
    ui.painter().rect_stroke(
        search_r,
        2.0,
        Stroke::new(1.0_f32, Color32::from_rgb(0x28, 0x2B, 0x32)),
        egui::StrokeKind::Inside,
    );
    let mg_r = Rect::from_center_size(pos2(search_r.min.x + 8.5, search_r.center().y), vec2(9.5, 9.5));
    icons::paint_icon(
        ui.painter(),
        mg_r,
        icons::SEARCH,
        Color32::from_rgb(0x98, 0x9C, 0xA6),
        1.4,
    );
    let clr_r = Rect::from_center_size(pos2(search_r.max.x - 7.5, search_r.center().y), vec2(11.0, 11.0));
    let clr_resp = ui.interact(
        clr_r,
        ui.id().with(("ext_search_clr", target)),
        Sense::click(),
    );
    icons::paint_icon(
        ui.painter(),
        clr_r.shrink(2.5),
        icons::X_CLOSE,
        if clr_resp.hovered() {
            Color32::WHITE
        } else {
            Color32::from_rgb(0x78, 0x7C, 0x86)
        },
        1.4,
    );
    let mut query = slot_snap.param_search.clone();
    let te_rect = Rect::from_min_max(
        pos2(mg_r.max.x + 2.0, search_r.min.y + 1.0),
        pos2(clr_r.min.x - 1.0, search_r.max.y - 1.0),
    );
    let mut te_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(te_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let te_resp = te_ui.add_sized(
        te_rect.size(),
        egui::TextEdit::singleline(&mut query)
            .id(ui.id().with(("ext_search_te", target)))
            .frame(false)
            .hint_text("Search")
            .font(fonts::regular(9.5))
            .text_color(Color32::from_rgb(0xE8, 0xEC, 0xF4)),
    );
    if clr_resp.clicked() {
        query.clear();
        if let Some(s) = target.slot_mut(app) {
            s.param_search.clear();
        }
    } else if te_resp.changed() {
        if let Some(s) = target.slot_mut(app) {
            s.param_search = query.clone();
        }
    }

    // --- Optional GUI error strip ---
    let mut list_top = tb_rect.max.y + 1.0;
    if let Some(err) = gui_err {
        let err_h = 22.0;
        let err_r = Rect::from_min_size(pos2(params_rect.min.x + 2.0, list_top), vec2(params_rect.width() - 4.0, err_h));
        ui.painter()
            .rect_filled(err_r, 2.0, Color32::from_rgb(0x4A, 0x20, 0x22));
        let err_resp = ui
            .interact(
                err_r,
                ui.id().with(("ext_gui_err", target)),
                Sense::click(),
            )
            .on_hover_text(format!("{err}\nClick to dismiss"));
        ui.painter().text(
            pos2(err_r.min.x + 4.0, err_r.center().y),
            Align2::LEFT_CENTER,
            truncate_str(&err, 28),
            fonts::regular(8.5),
            Color32::from_rgb(0xFF, 0xB8, 0xB8),
        );
        if err_resp.clicked() {
            app.plugin_settings_state.clear_gui_error(&wkey);
        }
        list_top = err_r.max.y + 2.0;
    }

    // --- Scrollable Public Parameters Body ---
    let list_rect = Rect::from_min_max(
        pos2(params_rect.min.x + 1.0, list_top),
        pos2(params_rect.max.x - 1.0, params_rect.max.y - 1.0),
    );
    let mut list_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(list_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    list_ui.set_clip_rect(list_rect.intersect(ui.clip_rect()));

    match &slot_snap.params_status {
        ExternalParamsStatus::Probing => {
            list_ui.add_space(20.0);
            list_ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("Scanning parameters…")
                        .font(fonts::regular(10.0))
                        .color(theme::TEXT_MUTED),
                );
            });
            list_ui.ctx().request_repaint();
            return;
        }
        ExternalParamsStatus::Error(msg) | ExternalParamsStatus::Failed(msg) => {
            list_ui.add_space(12.0);
            list_ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(truncate_str(msg, 56))
                        .font(fonts::regular(9.5))
                        .color(Color32::from_rgb(0xE8, 0x8A, 0x8A)),
                );
                ui.add_space(6.0);
                if ui.small_button("Retry Scan").clicked() {
                    app.param_probing.remove(&ext_desc.id);
                    app.request_external_params(&ext_desc);
                }
            });
            return;
        }
        ExternalParamsStatus::Ready | ExternalParamsStatus::Idle => {}
    }

    if slot_snap.cached_params.is_empty() {
        list_ui.add_space(16.0);
        list_ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("No public parameters")
                    .font(fonts::regular(10.0))
                    .color(theme::TEXT_MUTED),
            );
            ui.add_space(6.0);
            if ui.small_button("Rescan").clicked() {
                app.param_probing.remove(&ext_desc.id);
                app.request_external_params(&ext_desc);
            }
        });
        return;
    }

    let q_low = query.trim().to_ascii_lowercase();
    let filtered_indices: Vec<usize> = slot_snap
        .cached_params
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            q_low.is_empty() || p.info.name.to_ascii_lowercase().contains(&q_low)
        })
        .map(|(i, _)| i)
        .collect();

    if filtered_indices.is_empty() {
        list_ui.add_space(16.0);
        list_ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("No matching parameters")
                    .font(fonts::regular(9.5))
                    .color(theme::TEXT_MUTED),
            );
        });
        return;
    }

    let top_slot = target.top_level_slot();
    let mod_mapping_src = top_slot.and_then(|(tr, sl)| {
        hikaru_openmodulation::active_mapping_source_for_external(tr, sl)
    });
    let remote_mapping_active = top_slot.is_some() && remote_controls::is_mapping_active(app);

    let mut pending_value_updates: Vec<(usize, f32)> = Vec::new();
    let mut pending_remote_pick: Option<RemoteTarget> = None;
    let mut pending_automate_param: Option<u32> = None;
    let mut pending_clear_mod_param: Option<u32> = None;
    let mut hovered_help: Option<(String, String, Vec<(String, String)>)> = None;

    let row_h = 30.0_f32;
    egui::ScrollArea::vertical()
        .id_salt(("ext_params_scroll", target))
        .auto_shrink([false; 2])
        .show_rows(&mut list_ui, row_h, filtered_indices.len(), |ui, row_range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let row_w = ui.available_width();
            for vis_i in row_range {
                let param_idx = filtered_indices[vis_i];
                let p_state = &slot_snap.cached_params[param_idx];
                let param_id = p_state.info.id;
                let base_val = p_state.value.clamp(0.0, 1.0);
                let val_text = p_state.display();
                let active_mod = top_slot.and_then(|(tr, sl)| {
                    hikaru_openmodulation::get_external_param_modulation(tr, sl, param_id)
                });
                let map_amt = match (mod_mapping_src, top_slot) {
                    (Some(src), Some((_, sl))) => {
                        hikaru_openmodulation::get_external_route_amount(src, sl, param_id)
                    }
                    _ => 0.0,
                };

                let (row_rect, row_resp) = ui.allocate_exact_size(
                    vec2(row_w, row_h),
                    Sense::click_and_drag(),
                );
                let p = ui.painter();

                // Row background & bottom hairline divider (Bitwig Screenshot 1)
                let bg = if row_resp.hovered() || row_resp.dragged() {
                    ROW_HOVER_BG
                } else if vis_i % 2 == 0 {
                    ROW_BG_EVEN
                } else {
                    ROW_BG_ODD
                };
                p.rect_filled(row_rect, 0.0, bg);
                p.line_segment(
                    [pos2(row_rect.min.x, row_rect.max.y), pos2(row_rect.max.x, row_rect.max.y)],
                    Stroke::new(1.0_f32, DIVIDER_DARK),
                );

                // Rotary knob on the left
                let knob_c = pos2(row_rect.min.x + 16.0, row_rect.center().y);
                let knob_r = 10.5_f32;
                paint_param_knob(
                    p,
                    knob_c,
                    knob_r,
                    base_val,
                    active_mod.map(|m| m.amount),
                    active_mod.map(|m| m.delta),
                    mod_mapping_src.is_some(),
                    map_amt,
                    row_resp.hovered() || row_resp.dragged(),
                );

                // Two-line label: Name (top) + Formatted Value (bottom)
                let text_x = row_rect.min.x + 33.0;
                p.text(
                    pos2(text_x, row_rect.min.y + 9.5),
                    Align2::LEFT_CENTER,
                    truncate_str(&p_state.info.name, 22),
                    fonts::semibold(10.0),
                    if row_resp.hovered() || row_resp.dragged() {
                        Color32::WHITE
                    } else {
                        Color32::from_rgb(0xE0, 0xE4, 0xEC)
                    },
                );
                let sub_text = if mod_mapping_src.is_some() && map_amt.abs() > 0.004 {
                    format!("{val_text}  ({:+.0}%)", map_amt * 100.0)
                } else {
                    val_text.clone()
                };
                p.text(
                    pos2(text_x, row_rect.min.y + 21.5),
                    Align2::LEFT_CENTER,
                    truncate_str(&sub_text, 24),
                    fonts::regular(9.2),
                    if mod_mapping_src.is_some() && map_amt.abs() > 0.004 {
                        MOD_ARROW_CYAN
                    } else if active_mod.is_some() {
                        Color32::from_rgb(0x88, 0xD6, 0xF0)
                    } else {
                        Color32::from_rgb(0xB0, 0xB5, 0xC0)
                    },
                );

                // Cyan overlay when Remote Controls Mapping Mode is active
                if remote_mapping_active {
                    if let Some((tr, sl)) = top_slot {
                        remote_controls::paint_mappable_overlay(ui, row_rect.shrink(1.0));
                        if row_resp.hovered() {
                            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                        }
                        if row_resp.clicked() {
                            pending_remote_pick = Some(RemoteTarget::DeviceParam {
                                track_idx: tr,
                                slot_idx: sl,
                                param_idx,
                            });
                        }
                    }
                } else if let (Some(src), Some((_, sl))) = (mod_mapping_src, top_slot) {
                    // Modulator mapping mode: drag vertically to assign bipolar depth
                    if row_resp.hovered() || row_resp.dragged() {
                        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
                    }
                    if row_resp.dragged() {
                        let speed = if ui.input(|i| i.modifiers.shift) {
                            0.0015
                        } else {
                            0.0065
                        };
                        let dy = -row_resp.drag_delta().y;
                        if dy.abs() > 0.0 {
                            let next_amt = (map_amt + dy * speed).clamp(-1.0, 1.0);
                            hikaru_openmodulation::set_external_route_amount(
                                src, sl, param_id, next_amt,
                            );
                        }
                    } else if row_resp.double_clicked() {
                        hikaru_openmodulation::set_external_route_amount(
                            src, sl, param_id, 0.0,
                        );
                    }
                    if row_resp.hovered() || row_resp.dragged() {
                        hovered_help = Some((
                            format!("MOD → {}", p_state.info.name.to_uppercase()),
                            format!("{:+.0}%", map_amt * 100.0),
                            vec![
                                ("DRAG".to_string(), "Set mod depth".to_string()),
                                ("DBL-CLICK".to_string(), "Clear route".to_string()),
                                ("ESC".to_string(), "Exit mapping".to_string()),
                            ],
                        ));
                    }
                } else {
                    // Normal parameter editing
                    if row_resp.hovered() || row_resp.dragged() {
                        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
                        let mut cues = vec![
                            ("DRAG".to_string(), "Adjust".to_string()),
                            ("SHIFT".to_string(), "Fine".to_string()),
                            ("DBL-CLICK".to_string(), "Reset".to_string()),
                        ];
                        if top_slot.is_some() {
                            cues.push(("RIGHT-CLICK".to_string(), "Automate".to_string()));
                        }
                        hovered_help = Some((
                            p_state.info.name.to_uppercase(),
                            val_text.clone(),
                            cues,
                        ));
                    }
                    if row_resp.dragged() {
                        let speed = if ui.input(|i| i.modifiers.shift) {
                            0.0012
                        } else {
                            0.0055
                        };
                        let dy = -row_resp.drag_delta().y;
                        if dy.abs() > 0.0 {
                            let next = p_state.info.snap((base_val + dy * speed).clamp(0.0, 1.0));
                            if (next - base_val).abs() > 1e-6 {
                                pending_value_updates.push((param_idx, next));
                            }
                        }
                    } else if row_resp.double_clicked() {
                        let def = p_state.info.snap(p_state.info.default_value);
                        pending_value_updates.push((param_idx, def));
                    }

                    row_resp.context_menu(|menu_ui| {
                        menu_ui.label(
                            RichText::new(format!("{}: {}", p_state.info.name, val_text))
                                .font(fonts::semibold(10.0))
                                .color(BITWIG_ORANGE),
                        );
                        menu_ui.separator();
                        if menu_ui.button("Reset to Default").clicked() {
                            pending_value_updates
                                .push((param_idx, p_state.info.snap(p_state.info.default_value)));
                            menu_ui.close_menu();
                        }
                        if top_slot.is_some() && menu_ui.button("Automate Parameter").clicked() {
                            pending_automate_param = Some(param_id);
                            menu_ui.close_menu();
                        }
                        if active_mod.is_some()
                            && menu_ui.button("Clear Modulations").clicked()
                        {
                            pending_clear_mod_param = Some(param_id);
                            menu_ui.close_menu();
                        }
                    });
                }
            }
        });

    if !pending_value_updates.is_empty() {
        if let Some(s) = target.slot_mut(app) {
            for (param_idx, new_val) in pending_value_updates {
                if let Some(p) = s.cached_params.get_mut(param_idx) {
                    p.value = new_val;
                    p.display_text = None;
                    p.pending_to_engine = true;
                }
            }
        }
        app.project_dirty = true;
    }
    if let Some(target_remote) = pending_remote_pick {
        remote_controls::complete_mapping(app, target_remote);
    }
    if let (Some(param_id), Some((tr, sl))) = (pending_automate_param, top_slot) {
        automation_editor::open_external_param_automation(app, tr, sl, param_id);
    }
    if let (Some(param_id), Some((tr, sl))) = (pending_clear_mod_param, top_slot) {
        hikaru_openmodulation::clear_external_param_routes(tr, sl, param_id);
    }
    if let Some((title, val, cues)) = hovered_help {
        app.footer_context_help = Some((title, val));
        app.footer_context_cues = cues;
    }
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::mixer::DspSlot;

    #[test]
    fn card_width_grows_with_remotes_modulators_and_collapse() {
        let mut slot = DspSlot::new(1, "UZU".to_string());
        let w_base = card_width(&slot, 99, 77);
        assert!((w_base - (EXT_SIDEBAR_W + EXT_PARAMS_EXPANDED_W)).abs() < 1e-3);

        slot.params_expanded = false;
        let w_collapsed = card_width(&slot, 99, 77);
        assert!((w_collapsed - (EXT_SIDEBAR_W + EXT_PARAMS_COLLAPSED_W)).abs() < 1e-3);

        slot.params_expanded = true;
        slot.remotes.remotes_open = true;
        let w_remotes = card_width(&slot, 99, 77);
        assert!((w_remotes - (w_base + REMOTES_BODY_WIDTH)).abs() < 1e-3);

        slot.modulators_open = true;
        let w_both = card_width(&slot, 99, 77);
        assert!(w_both > w_remotes + 50.0);
        hikaru_openmodulation::clear_bay_for_slot(99, 77);
    }

    #[test]
    fn wet_fx_card_width_matches_dedicated_external_card() {
        let mut slot = DspSlot::new(2, "UZU".to_string());
        assert!(
            (wet_fx_card_width(&slot) - (EXT_SIDEBAR_W + EXT_PARAMS_EXPANDED_W)).abs() < 1e-3
        );
        slot.params_expanded = false;
        assert!(
            (wet_fx_card_width(&slot) - (EXT_SIDEBAR_W + EXT_PARAMS_COLLAPSED_W)).abs() < 1e-3
        );
    }
}
