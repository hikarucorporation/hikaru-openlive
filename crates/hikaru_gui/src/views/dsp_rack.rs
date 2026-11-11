// Hikaru OpenLive / OpenStudio - Bitwig 5.1.7 Bottom Device Panel (`dsp_rack.rs`)
// GNU Affero General Public License v3

use egui::{pos2, vec2, Align2, Color32, Rect, RichText, Sense, Stroke};

use crate::app::AppState;
use crate::theme;
use crate::ui::fonts;
use crate::ui::icons;
use crate::views::hikaru_native_plugins::hikaru_opendms::rack_card::render_rack_card;
use crate::views::hikaru_native_plugins::hikaru_openeq3;
use crate::views::hikaru_native_plugins::hikaru_openmodulation::{self, MODULATION_CARD_WIDTH};
use crate::views::hikaru_native_plugins::hikaru_openspectral_phaser::openspectral_phaser;
use crate::views::hikaru_native_plugins::hikaru_openwavetable::open_wavetable;
use crate::views::mixer::DspSlot;

pub const RACK_HEIGHT: f32 = 178.0;
pub const CARD_WIDTH: f32 = 340.0;
pub const OPEN_WAVETABLE_CARD_WIDTH: f32 = 446.0;
pub const PHASER_CARD_WIDTH: f32 = 480.0;
pub const EXTERNAL_CARD_WIDTH: f32 = 360.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginEntry {
    pub category: &'static str,
    pub name: &'static str,
    pub label: &'static str,
}

pub const PLUGIN_CATALOG: [PluginEntry; 5] = [
    PluginEntry {
        category: "Audio FX",
        name: "OpenEQ3",
        label: "OpenEQ3",
    },
    PluginEntry {
        category: "Audio FX",
        name: "OpenSpectral Phaser",
        label: "OpenSpectral Phaser",
    },
    PluginEntry {
        category: "Polysynth",
        name: "OpenWavetable",
        label: "Hikaru OpenWavetable",
    },
    PluginEntry {
        category: "Drum Synth",
        name: "Hikaru OpenDMS",
        label: "Hikaru OpenDMS",
    },
    PluginEntry {
        category: "Modulation",
        name: "OpenModulation",
        label: "Hikaru OpenModulation",
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PluginBrowserFilter {
    #[default]
    All,
    Native,
    Vst3,
    Clap,
}

pub fn rotary_knob_angle_deg(normalized: f32) -> f32 {
    let norm = if normalized.is_finite() {
        normalized.clamp(0.0, 1.0)
    } else {
        0.0
    };
    -135.0 + norm * 270.0
}

pub fn rotary_knob_point(cx: f32, cy: f32, r: f32, deg: f32) -> (f32, f32) {
    let rad = deg.to_radians();
    (cx + r * rad.sin(), cy - r * rad.cos())
}

pub fn rack_title(track_name: &str, scene: Option<usize>) -> String {
    match scene {
        Some(scene) => format!("DSP RACK: {track_name} | Scene {}", scene + 1),
        None => format!("DSP RACK: {track_name}"),
    }
}

pub fn keep_plugin_editors_above(_app: &mut AppState) {}

pub fn handle_delete_key(app: &mut AppState) {
    let track_idx = app.safe_track_index();
    let slot_idx = app.selected_slot_index;
    if let Some(track) = app.current_tracks_mut().get_mut(track_idx) {
        if slot_idx < track.effects.len() {
            track.effects.remove(slot_idx);
            if slot_idx >= track.effects.len() && !track.effects.is_empty() {
                app.selected_slot_index = track.effects.len() - 1;
            }
        }
    }
}

pub fn save_device_chain_dialog(app: &AppState, track_idx: usize) {
    let Some(track) = app.current_tracks().get(track_idx) else { return };
    if track.effects.is_empty() {
        return;
    }
    let default_name = format!("{}_chain.hdp", track.name.to_lowercase().replace(' ', "_"));
    if let Some(path) = rfd::FileDialog::new()
        .set_file_name(&default_name)
        .add_filter(hikaru_project_file::HDP_FILE_FILTER, &[hikaru_project_file::HDP_EXTENSION])
        .save_file()
    {
        let captured = crate::project::common::capture_track(track);
        let preset = hikaru_project_file::HikaruDevicePreset {
            format_version: hikaru_project_file::FORMAT_VERSION,
            created_by: format!("Hikaru OpenLive {}", crate::version::VERSION),
            preset_type: hikaru_project_file::PresetType::Chain,
            name: track.name.clone(),
            author: None,
            category: Some("Track Device Chain".to_string()),
            description: Some(format!("Device chain preset for {}", track.name)),
            slot: None,
            chain: Some(captured.effects),
        };
        let _ = hikaru_project_file::save_hdp(&path, &preset);
    }
}

pub fn load_device_chain_dialog(app: &mut AppState, track_idx: usize) {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter(hikaru_project_file::HDP_FILE_FILTER, &[hikaru_project_file::HDP_EXTENSION])
        .pick_file()
    {
        load_device_preset_from_path(app, track_idx, &path);
    }
}

pub fn load_device_preset_from_path(app: &mut AppState, track_idx: usize, path: &std::path::Path) {
    let Ok(preset) = hikaru_project_file::parse_hdp(path) else { return };
    if let Some(chain) = preset.chain {
        let dummy_track = hikaru_project_file::Track {
            id: track_idx,
            name: preset.name,
            volume: 0.7,
            pan: 0.0,
            pan_mode: 0,
            mute: false,
            solo: false,
            arm: false,
            is_master: track_idx == 0,
            route_destination_id: 0,
            sends: Vec::new(),
            effects: chain,
            matrix_idx: None,
        };
        let built = crate::project::common::build_track(&dummy_track);
        if let Some(track) = app.current_tracks_mut().get_mut(track_idx) {
            track.effects = built.effects;
            if !track.effects.is_empty() {
                app.selected_slot_index = 0;
            }
        }
    } else if let Some(slot) = preset.slot {
        let dummy_track = hikaru_project_file::Track {
            id: track_idx,
            name: String::new(),
            volume: 0.7,
            pan: 0.0,
            pan_mode: 0,
            mute: false,
            solo: false,
            arm: false,
            is_master: track_idx == 0,
            route_destination_id: 0,
            sends: Vec::new(),
            effects: vec![slot],
            matrix_idx: None,
        };
        let built = crate::project::common::build_track(&dummy_track);
        if let Some(track) = app.current_tracks_mut().get_mut(track_idx) {
            if let Some(new_slot) = built.effects.into_iter().next() {
                track.effects.push(new_slot);
                app.selected_slot_index = track.effects.len() - 1;
            }
        }
    }
}

pub fn render(ui: &mut egui::Ui, app: &mut AppState) {
    let track_idx = app.safe_track_index();
    let is_project_master = track_idx == 0;
    let (track_name, track_color) = app
        .current_tracks()
        .get(track_idx)
        .map(|t| (t.name.clone(), t.color))
        .unwrap_or_else(|| ("Track".into(), theme::ACCENT_ORANGE));

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);

        // =====================================================================
        // 1. Tira vertical izquierda de Bitwig (`PROJECT` corona / `TRACK`)
        // =====================================================================
        let strip_h = ui.available_height().max(RACK_HEIGHT - 4.0);
        let (strip_rect, _) = ui.allocate_exact_size(vec2(26.0, strip_h), Sense::hover());
        if ui.is_rect_visible(strip_rect) {
            let p = ui.painter();
            p.rect_filled(strip_rect, 0.0, Color32::from_rgb(0x23, 0x25, 0x29));

            let half_h = (strip_rect.height() * 0.48).round();
            let proj_tab = Rect::from_min_size(strip_rect.min, vec2(26.0, half_h));
            let trk_tab = Rect::from_min_max(pos2(strip_rect.min.x, proj_tab.max.y + 1.0), strip_rect.max);

            let proj_resp = ui.interact(proj_tab, ui.id().with("dsp_proj_tab"), Sense::click());
            let trk_resp = ui.interact(trk_tab, ui.id().with("dsp_trk_tab"), Sense::click());

            p.rect_filled(
                proj_tab,
                0.0,
                if is_project_master {
                    Color32::from_rgb(0x52, 0x54, 0x5A)
                } else {
                    Color32::from_rgb(0x32, 0x34, 0x3A)
                },
            );
            icons::paint_crown(
                p,
                Rect::from_center_size(pos2(proj_tab.center().x, proj_tab.min.y + 12.0), vec2(12.0, 10.0)),
                if is_project_master {
                    theme::ACCENT_ORANGE
                } else {
                    Color32::from_rgb(0x9E, 0xA2, 0xAA)
                },
            );
            for (ci, ch) in "PROJ".chars().enumerate() {
                p.text(
                    pos2(proj_tab.center().x, proj_tab.min.y + 27.0 + ci as f32 * 10.5),
                    Align2::CENTER_CENTER,
                    ch.to_string(),
                    fonts::semibold(8.5),
                    if is_project_master {
                        Color32::from_rgb(0x11, 0x12, 0x14)
                    } else {
                        Color32::from_rgb(0xA6, 0xAA, 0xB2)
                    },
                );
            }

            p.rect_filled(
                trk_tab,
                0.0,
                if !is_project_master {
                    Color32::from_rgb(0x58, 0x5A, 0x60)
                } else {
                    Color32::from_rgb(0x32, 0x34, 0x3A)
                },
            );
            p.rect_filled(
                Rect::from_min_size(trk_tab.min, vec2(3.5, trk_tab.height())),
                0.0,
                track_color,
            );
            for (ci, ch) in "TRACK".chars().enumerate() {
                p.text(
                    pos2(trk_tab.center().x + 1.0, trk_tab.min.y + 14.0 + ci as f32 * 11.0),
                    Align2::CENTER_CENTER,
                    ch.to_string(),
                    fonts::semibold(8.5),
                    if !is_project_master {
                        Color32::from_rgb(0x11, 0x12, 0x14)
                    } else {
                        Color32::from_rgb(0xA6, 0xAA, 0xB2)
                    },
                );
            }

            p.line_segment(
                [strip_rect.right_top(), strip_rect.right_bottom()],
                Stroke::new(1.0_f32, theme::BW_CAPSULE_BORDER),
            );

            if proj_resp.on_hover_text("Project / Master Devices").clicked() {
                app.selected_track_index = 0;
            }
            if trk_resp.on_hover_text("Selected Track Devices").clicked() && app.selected_track_index == 0 {
                if app.current_tracks().len() > 1 {
                    app.selected_track_index = 1;
                }
            }
        }

        // =====================================================================
        // 2. Pozo negro profundo `#0B0C0E` de la cadena de dispositivos Bitwig
        // =====================================================================
        let well_rect = ui.available_rect_before_wrap();
        ui.painter().rect_filled(well_rect, 0.0, theme::BW_DARK_WELL);
        let content_rect = well_rect.shrink2(vec2(6.0, 4.0));
        let mut well_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(content_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Min)),
        );
        egui::ScrollArea::horizontal()
            .id_salt("dsp_rack_scroll")
            .auto_shrink([false; 2])
            .show(&mut well_ui, |ui| {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 0.0);

                    // Píldora vertical de cabecera de pista a la izquierda de la cadena
                    let pill_h = (content_rect.height() - 2.0).clamp(136.0, RACK_HEIGHT - 10.0);
                    let (pill_rect, pill_resp) = ui.allocate_exact_size(vec2(22.0, pill_h), Sense::click());
                    pill_resp.context_menu(|ui| {
                        if ui.button("💾 Guardar Cadena de Pista (*.hdp)...").clicked() {
                            save_device_chain_dialog(app, track_idx);
                            ui.close_menu();
                        }
                        if ui.button("📂 Cargar Cadena de Pista (*.hdp)...").clicked() {
                            load_device_chain_dialog(app, track_idx);
                            ui.close_menu();
                        }
                    });
                    if ui.is_rect_visible(pill_rect) {
                        let p = ui.painter();
                        p.rect_filled(pill_rect, 3.0, Color32::from_rgb(0x46, 0x48, 0x4E));
                        p.rect_filled(
                            Rect::from_min_size(pill_rect.min, vec2(pill_rect.width(), 12.0)),
                            2.5,
                            track_color,
                        );
                        icons::paint_power_switch(
                            p,
                            Rect::from_center_size(
                                pos2(pill_rect.center().x, pill_rect.min.y + 24.0),
                                vec2(14.0, 14.0),
                            ),
                            theme::ACCENT_ORANGE,
                            Color32::from_rgb(0x14, 0x12, 0x10),
                        );
                        icons::paint_icon(
                            p,
                            Rect::from_center_size(
                                pos2(pill_rect.center().x, pill_rect.max.y - 16.0),
                                vec2(13.0, 13.0),
                            ),
                            icons::LINK,
                            Color32::from_rgb(0x18, 0x19, 0x1C),
                            1.7,
                        );
                    }

                    let slot_count = app
                        .current_tracks()
                        .get(track_idx)
                        .map(|t| t.effects.len())
                        .unwrap_or(0);

                    let mut slot_to_remove = None;
                    for slot_idx in 0..slot_count {
                        let slot_info = app
                            .current_tracks()
                            .get(track_idx)
                            .and_then(|t| t.effects.get(slot_idx))
                            .cloned();
                        let Some(slot) = slot_info else { continue };

                        let is_selected = app.selected_slot_index == slot_idx;
                        let card_w = match slot.name.as_str() {
                            "OpenWavetable" => OPEN_WAVETABLE_CARD_WIDTH,
                            "OpenSpectral Phaser" | "OpenSpectralPhaser" => PHASER_CARD_WIDTH,
                            "OpenModulation" | "Hikaru OpenModulation" | "LFO MOD" => MODULATION_CARD_WIDTH,
                            _ => CARD_WIDTH,
                        };
                        let card_h = pill_h;
                        let (card_rect, _) =
                            ui.allocate_exact_size(vec2(card_w, card_h), Sense::hover());

                        let p = ui.painter();
                        p.rect_filled(card_rect, 3.5, Color32::from_rgb(0x2E, 0x30, 0x35));
                        p.rect_stroke(
                            card_rect,
                            3.5,
                            Stroke::new(
                                if is_selected { 1.5_f32 } else { 1.0_f32 },
                                if is_selected {
                                    theme::ACCENT_ORANGE
                                } else {
                                    Color32::from_rgb(0x46, 0x49, 0x52)
                                },
                            ),
                            egui::StrokeKind::Inside,
                        );

                        let inner_rect = card_rect.shrink2(vec2(6.0, 4.0));
                        let inner_w = inner_rect.width();
                        let mut card_ui = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(inner_rect)
                                .layout(egui::Layout::top_down(egui::Align::Min)),
                        );
                        card_ui.set_clip_rect(inner_rect.intersect(ui.clip_rect()));
                        card_ui.spacing_mut().item_spacing = vec2(4.0, 3.0);

                        // Barra de título estilo módulo Bitwig (Power switch vectorial + Título + Close vectorial)
                        let (hdr_rect, _) =
                            card_ui.allocate_exact_size(vec2(inner_w, 20.0), Sense::hover());
                        {
                            let active = slot.active;
                            let pwr_r = Rect::from_min_size(
                                pos2(hdr_rect.min.x, hdr_rect.center().y - 9.0),
                                vec2(18.0, 18.0),
                            );
                            let pwr_resp = card_ui.interact(
                                pwr_r,
                                card_ui.id().with(("dsp_pwr", slot_idx)),
                                Sense::click(),
                            );
                            icons::paint_power_switch(
                                card_ui.painter(),
                                pwr_r,
                                if active {
                                    theme::ACCENT_ORANGE
                                } else {
                                    Color32::from_rgb(0x44, 0x47, 0x50)
                                },
                                if active {
                                    Color32::from_rgb(0x14, 0x12, 0x10)
                                } else {
                                    Color32::from_rgb(0xB8, 0xBC, 0xC4)
                                },
                            );
                            if pwr_resp.on_hover_text("Device Active / Bypass").clicked() {
                                if let Some(t) = app.current_tracks_mut().get_mut(track_idx) {
                                    if let Some(s) = t.effects.get_mut(slot_idx) {
                                        s.active = !s.active;
                                    }
                                }
                            }

                            let title_r = Rect::from_min_max(
                                pos2(pwr_r.max.x + 5.0, hdr_rect.min.y),
                                pos2(hdr_rect.max.x - 22.0, hdr_rect.max.y),
                            );
                            let title_resp = card_ui.interact(
                                title_r,
                                card_ui.id().with(("dsp_title", slot_idx)),
                                Sense::click(),
                            );
                            if is_selected {
                                let pill_w =
                                    (slot.name.len() as f32 * 6.6 + 12.0).min(title_r.width());
                                let title_pill = Rect::from_min_size(
                                    pos2(title_r.min.x, hdr_rect.center().y - 8.5),
                                    vec2(pill_w, 17.0),
                                );
                                card_ui
                                    .painter()
                                    .rect_filled(title_pill, 2.5, theme::ACCENT_ORANGE);
                            }
                            card_ui.painter().text(
                                pos2(title_r.min.x + 5.0, hdr_rect.center().y),
                                Align2::LEFT_CENTER,
                                &slot.name,
                                fonts::semibold(11.0),
                                if is_selected {
                                    Color32::from_rgb(0x12, 0x13, 0x15)
                                } else {
                                    theme::TEXT_PRIMARY
                                },
                            );
                            if title_resp.clicked() {
                                app.selected_slot_index = slot_idx;
                            }

                            let close_r = Rect::from_min_size(
                                pos2(hdr_rect.max.x - 18.0, hdr_rect.center().y - 9.0),
                                vec2(18.0, 18.0),
                            );
                            let close_resp = card_ui.interact(
                                close_r,
                                card_ui.id().with(("dsp_close", slot_idx)),
                                Sense::click(),
                            );
                            if close_resp.hovered() {
                                card_ui.painter().rect_filled(
                                    close_r,
                                    2.5,
                                    Color32::from_rgb(0x48, 0x4A, 0x52),
                                );
                            }
                            icons::paint_icon(
                                card_ui.painter(),
                                close_r.shrink(3.5),
                                icons::X_CLOSE,
                                if close_resp.hovered() {
                                    Color32::WHITE
                                } else {
                                    theme::TEXT_MUTED
                                },
                                1.7,
                            );
                            if close_resp.on_hover_text("Remove Device (Del)").clicked() {
                                slot_to_remove = Some(slot_idx);
                            }
                        }

                        let sep_y = hdr_rect.max.y + 1.5;
                        card_ui.painter().line_segment(
                            [
                                pos2(inner_rect.min.x, sep_y),
                                pos2(inner_rect.max.x, sep_y),
                            ],
                            Stroke::new(1.0_f32, Color32::from_rgb(0x1E, 0x20, 0x25)),
                        );

                        let body_rect = Rect::from_min_max(
                            pos2(inner_rect.min.x, (sep_y + 3.0).min(inner_rect.max.y)),
                            inner_rect.max,
                        );
                        let mut body_ui = card_ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(body_rect)
                                .layout(egui::Layout::top_down(egui::Align::Min)),
                        );
                        body_ui.set_clip_rect(body_rect.intersect(card_ui.clip_rect()));

                        // Cuerpo del dispositivo DSP
                        match slot.name.as_str() {
                            "OpenWavetable" => {
                                open_wavetable::render_module(&mut body_ui, app, track_idx, slot_idx);
                            }
                            "Hikaru OpenDMS" => {
                                render_rack_card(&mut body_ui, app, track_idx, slot_idx);
                            }
                            "OpenEQ3" => {
                                hikaru_openeq3::render_module(&mut body_ui);
                            }
                            "OpenSpectral Phaser" | "OpenSpectralPhaser" => {
                                openspectral_phaser::render_module(
                                    &mut body_ui,
                                    app,
                                    track_idx,
                                    slot_idx,
                                );
                            }
                            "OpenModulation" | "Hikaru OpenModulation" | "LFO MOD" => {
                                hikaru_openmodulation::render_module(
                                    &mut body_ui,
                                    app,
                                    track_idx,
                                    slot_idx,
                                );
                            }
                            _ => {
                                body_ui.vertical_centered(|ui| {
                                    ui.add_space(16.0);
                                    ui.label(
                                        RichText::new(&slot.name)
                                            .font(fonts::semibold(12.0))
                                            .color(theme::TEXT_SECONDARY),
                                    );
                                    ui.label(
                                        RichText::new("External Plugin")
                                            .font(fonts::regular(10.0))
                                            .color(theme::TEXT_MUTED),
                                    );
                                });
                            }
                        }
                    }

                    if let Some(idx) = slot_to_remove {
                        if let Some(track) = app.current_tracks_mut().get_mut(track_idx) {
                            if idx < track.effects.len() {
                                track.effects.remove(idx);
                            }
                        }
                    }

                    // Botón circular `+` estilo ranura de inserción Bitwig
                    ui.vertical(|ui| {
                        ui.add_space(56.0);
                        ui.menu_button(
                            RichText::new("+")
                                .font(fonts::semibold(15.0))
                                .color(theme::TEXT_PRIMARY),
                            |ui| {
                                ui.label(
                                    RichText::new(format!("Insert Device on {track_name}"))
                                        .font(fonts::semibold(11.0))
                                        .color(theme::ACCENT_ORANGE),
                                );
                                ui.separator();
                                for plugin in &PLUGIN_CATALOG {
                                    let row_txt = format!("{}  [{}]", plugin.label, plugin.category);
                                    if ui.button(row_txt).clicked() {
                                        if let Some(track) = app.current_tracks_mut().get_mut(track_idx) {
                                            let mut new_slot = DspSlot::default();
                                            new_slot.name = plugin.name.to_string();
                                            track.effects.push(new_slot);
                                            app.selected_slot_index = track.effects.len() - 1;
                                        }
                                        ui.close_menu();
                                    }
                                }
                                ui.separator();
                                if ui.button("📂 Cargar Preset / Cadena (*.hdp)...").clicked() {
                                    load_device_chain_dialog(app, track_idx);
                                    ui.close_menu();
                                }
                                if ui.button("💾 Guardar Cadena de Pista (*.hdp)...").clicked() {
                                    save_device_chain_dialog(app, track_idx);
                                    ui.close_menu();
                                }
                            },
                        );
                    });
                });
            });
        ui.allocate_rect(well_rect, Sense::hover());
    });
}
