// Hikaru OpenLive / OpenStudio - Bitwig 5.1.7 Bottom Device Panel (`dsp_rack.rs`)
// GNU Affero General Public License v3

use egui::{pos2, vec2, Align2, Color32, Rect, RichText, Sense, Stroke};

use crate::app::AppState;
use crate::theme;
use crate::ui::fonts;
use crate::ui::icons;
use crate::views::hikaru_native_plugins::hikaru_bit8;
use crate::views::hikaru_native_plugins::hikaru_freq_shifter_plus;
use crate::views::hikaru_native_plugins::hikaru_opendms::rack_card::render_rack_card;
use crate::views::hikaru_native_plugins::hikaru_openeq3;
use crate::views::hikaru_native_plugins::hikaru_opengrid::{
    self, OPEN_GRID_COMPACT_CARD_WIDTH,
};
use crate::views::hikaru_native_plugins::hikaru_openmodulation::{self, MODULATION_CARD_WIDTH};
use crate::views::hikaru_native_plugins::hikaru_opencomb::{
    self, OPENCOMB_CARD_WIDTH, OPENCOMB_WET_CARD_WIDTH,
};
use crate::views::hikaru_native_plugins::hikaru_openflanger_plus::{
    self, OPEN_FLANGER_PLUS_CARD_WIDTH, OPEN_FLANGER_PLUS_WET_CARD_WIDTH,
};
use crate::views::hikaru_native_plugins::hikaru_openphaser_plus::{
    self, OPEN_PHASER_PLUS_CARD_WIDTH, OPEN_PHASER_PLUS_WET_CARD_WIDTH,
};
use crate::views::hikaru_native_plugins::hikaru_openpoly::{
    self, OPEN_POLY_COMPACT_CARD_WIDTH,
};
use crate::views::hikaru_native_plugins::hikaru_openspectral_phaser::openspectral_phaser;
use crate::views::hikaru_native_plugins::hikaru_sampler::{sampler_editor, SAMPLER_CARD_WIDTH};

pub const RACK_HEIGHT: f32 = 241.0;
pub const CARD_WIDTH: f32 = 340.0;
pub const OPEN_WAVETABLE_CARD_WIDTH: f32 = 446.0;
pub use crate::views::hikaru_native_plugins::hikaru_openpoly::OPEN_POLY_CARD_WIDTH as OPEN_POLY_WIDTH;
pub const OPEN_DMS_CARD_WIDTH: f32 = 430.0;
pub const PHASER_CARD_WIDTH: f32 = 480.0;
pub const EXTERNAL_CARD_WIDTH: f32 = 360.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginEntry {
    pub category: &'static str,
    pub name: &'static str,
    pub label: &'static str,
}

pub const PLUGIN_CATALOG: [PluginEntry; 19] = [
    PluginEntry {
        category: "Audio FX",
        name: "OpenEQ3",
        label: "OpenEQ3",
    },
    PluginEntry {
        category: "Audio FX",
        name: "Hikaru OpenFlanger+",
        label: "Hikaru OpenFlanger+",
    },
    PluginEntry {
        category: "Audio FX",
        name: "Hikaru OpenPhaser+",
        label: "Hikaru OpenPhaser+",
    },
    PluginEntry {
        category: "Audio FX",
        name: "OpenSpectral Phaser",
        label: "OpenSpectral Phaser",
    },
    PluginEntry {
        category: "Audio FX",
        name: "Hikaru OpenComb",
        label: "Hikaru OpenComb",
    },
    PluginEntry {
        category: "Audio FX",
        name: "Tool",
        label: "Tool",
    },
    PluginEntry {
        category: "Audio FX",
        name: "DC Offset",
        label: "DC Offset",
    },
    PluginEntry {
        category: "Audio FX",
        name: "Dual Pan",
        label: "Dual Pan",
    },
    PluginEntry {
        category: "Audio FX",
        name: "Test Tone",
        label: "Test Tone",
    },
    PluginEntry {
        category: "Audio FX",
        name: "Time Shift",
        label: "Time Shift",
    },
    PluginEntry {
        category: "Audio FX",
        name: "Tremolo",
        label: "Tremolo",
    },
    PluginEntry {
        category: "Audio FX",
        name: "Ring-Mod",
        label: "Ring-Mod",
    },
    PluginEntry {
        category: "Audio FX",
        name: "Hikaru Open8-Bit",
        label: "Hikaru Open8-Bit",
    },
    PluginEntry {
        category: "Audio FX",
        name: "Hikaru OpenFreqShifter+",
        label: "Hikaru OpenFreqShifter+",
    },
    PluginEntry {
        category: "Polysynth",
        name: "OpenPoly",
        label: "Hikaru OpenPoly",
    },
    PluginEntry {
        category: "Modular Synth",
        name: "OpenGrid",
        label: "Hikaru OpenGrid",
    },
    PluginEntry {
        category: "Sampler",
        name: "Sampler",
        label: "Hikaru Sampler",
    },
    PluginEntry {
        category: "Drum Synth",
        name: "Hikaru OpenDMS",
        label: "Hikaru OpenDMS",
    },
    PluginEntry {
        category: "Modulation",
        name: "OpenLFO MOD",
        label: "Hikaru OpenLFO MOD",
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

pub fn keep_plugin_editors_above(app: &mut AppState) {
    app.plugin_settings_state.raise_open_plugins();
}

/// Elimina un dispositivo del rack limpiando su ventana nativa, su bay de
/// moduladores y su instancia en el motor de audio (si es plugin externo).
pub fn remove_device_slot(app: &mut AppState, track_idx: usize, slot_idx: usize) {
    let removed_info = app
        .current_tracks_mut()
        .get_mut(track_idx)
        .and_then(|track| {
            if slot_idx < track.effects.len() {
                let removed = track.effects.remove(slot_idx);
                let new_len = track.effects.len();
                Some((removed, new_len))
            } else {
                None
            }
        });

    let Some((removed, new_len)) = removed_info else {
        return;
    };

    if app.open_grid_editor == Some((track_idx, slot_idx)) {
        app.open_grid_editor = None;
    }

    if app.selected_track_index == track_idx
        && app.selected_slot_index >= new_len
        && new_len > 0
    {
        app.selected_slot_index = new_len - 1;
    }

    hikaru_openmodulation::clear_bay_for_slot(track_idx, slot_idx);
    hikaru_bit8::clear_slot_state(track_idx, slot_idx);
    hikaru_freq_shifter_plus::clear_slot_state(track_idx, slot_idx);
    hikaru_openphaser_plus::clear_slot_state(track_idx, slot_idx);
    hikaru_openflanger_plus::clear_slot_state(track_idx, slot_idx);
    hikaru_opencomb::clear_slot_state(track_idx, slot_idx);
    let wkey = crate::views::external_plugins_settings::slot_window_key(track_idx, slot_idx);
    app.plugin_settings_state.close_window(&wkey);
    for wet_idx in 0..removed.wet_fx_chain.len() {
        let wet_wkey = crate::views::external_plugins_settings::wet_fx_window_key(
            track_idx, slot_idx, wet_idx,
        );
        app.plugin_settings_state.close_window(&wet_wkey);
    }

    if removed.external.is_some()
        || hikaru_opencomb::is_opencomb_name(&removed.name)
        || hikaru_bit8::is_bit8_name(&removed.name)
        || hikaru_freq_shifter_plus::is_freq_shifter_plus_name(&removed.name)
        || hikaru_openphaser_plus::is_openphaser_plus_name(&removed.name)
        || hikaru_openflanger_plus::is_openflanger_plus_name(&removed.name)
        || crate::views::hikaru_native_plugins::hikaru_tier_s_fx::is_tier_s_fx(&removed.name)
    {
        if let Some(eng_track) = track_idx.checked_sub(1) {
            app.audio_proxy.send(crate::audio_proxy::GuiCommand::RemoveExternalPlugin {
                track_idx: eng_track,
                slot_idx,
            });
        }
    }
    app.project_dirty = true;
}

pub fn handle_delete_key(app: &mut AppState) {
    let track_idx = app.safe_track_index();
    let slot_idx = app.selected_slot_index;
    remove_device_slot(app, track_idx, slot_idx);
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

pub fn build_bwpreset_export_spec(
    slot: &crate::views::mixer::DspSlot,
    track_idx: usize,
    slot_idx: usize,
) -> crate::project::bwdevice_shim::BwPresetExportSpec {
    let mut params = Vec::new();
    if let Some(poly) = &slot.open_poly {
        params.push(("CUTOFF".to_string(), ((poly.filter_cutoff_hz - 20.0) / 19980.0).clamp(0.0, 1.0) as f64));
        params.push(("RESONANCE".to_string(), poly.filter_resonance as f64));
        params.push(("DRIVE".to_string(), poly.filter_drive as f64));
        params.push(("OUTPUT".to_string(), poly.output_gain as f64));
        params.push(("WIDTH".to_string(), poly.stereo_width as f64));
        params.push(("SUB_LEVEL".to_string(), poly.sub_level as f64));
        params.push(("NOISE_LEVEL".to_string(), poly.noise_level as f64));
    }
    for p in &slot.cached_params {
        params.push((p.info.name.clone(), p.value as f64));
    }

    let grid = slot.open_grid.as_ref().map(|g| g.to_project_params());
    let modulators =
        crate::views::hikaru_native_plugins::hikaru_openmodulation::capture_bay_params(
            track_idx,
            slot_idx,
            slot.modulators_open,
        )
        .unwrap_or_default();

    let remote_labels = slot
        .remotes
        .pages
        .first()
        .map(|page| {
            page.slots
                .iter()
                .enumerate()
                .filter_map(|(i, s)| {
                    s.custom_label
                        .as_ref()
                        .filter(|l| !l.trim().is_empty())
                        .map(|l| (i, l.clone()))
                })
                .collect()
        })
        .unwrap_or_default();

    let device_name = if slot.is_open_grid() {
        "Poly Grid".to_string()
    } else if slot.is_open_poly() {
        "Polymer".to_string()
    } else {
        slot.name.clone()
    };

    let preset_name = if slot.preset_name.trim().is_empty() || slot.preset_name == "init" {
        slot.name.clone()
    } else {
        slot.preset_name.clone()
    };

    crate::project::bwdevice_shim::BwPresetExportSpec {
        device_name,
        preset_name,
        creator: format!("Hikaru OpenLive {}", crate::version::VERSION),
        category: if slot.is_open_grid() {
            "The Grid".to_string()
        } else if slot.is_open_poly() {
            "Synth".to_string()
        } else {
            "Audio FX".to_string()
        },
        comment: format!("Exported from Hikaru OpenLive slot '{}'", slot.name),
        params,
        modulators,
        grid,
        remote_labels,
    }
}

pub fn export_device_bwpreset_dialog(app: &AppState, track_idx: usize, slot_idx: usize) {
    let Some(track) = app.current_tracks().get(track_idx) else { return };
    let Some(slot) = track.effects.get(slot_idx) else { return };
    let default_name = format!("{}.bwpreset", slot.name.to_lowercase().replace(' ', "_"));
    if let Some(path) = rfd::FileDialog::new()
        .set_file_name(&default_name)
        .add_filter("Bitwig Preset (*.bwpreset)", &["bwpreset"])
        .save_file()
    {
        let spec = build_bwpreset_export_spec(slot, track_idx, slot_idx);
        let _ = crate::project::bwdevice_shim::export_bwpreset_file(&spec, &path);
    }
}

pub fn load_device_chain_dialog(app: &mut AppState, track_idx: usize) {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter(
            "Presets (Hikaru .hdp / Bitwig .bwpreset, .bwdevice)",
            &[hikaru_project_file::HDP_EXTENSION, "bwpreset", "bwdevice", "bwmodulator"],
        )
        .pick_file()
    {
        load_device_preset_from_path(app, track_idx, &path);
    }
}

fn apply_open_poly_translated_params(
    poly: &mut crate::views::hikaru_native_plugins::hikaru_openpoly::HikaruOpenPoly,
    params: &std::collections::BTreeMap<String, crate::project::bwdevice_shim::TranslatedParam>,
) {
    for (k, tp) in params {
        let low = k.trim().to_ascii_lowercase();
        let raw = tp.raw_value;
        let v = tp.normalized.clamp(0.0, 1.0);
        match low.as_str() {
            "cutoff" | "filter_cutoff" | "freq" | "frequency" => {
                if (20.0..=20000.0).contains(&raw) {
                    // Evitar que un parámetro de submódulo (ej. 29 Hz) cierre el filtro principal
                    poly.filter_cutoff_hz = (raw as f32).clamp(240.0, 20000.0);
                } else if raw > 0.0 && raw <= 1.0 {
                    poly.filter_cutoff_hz =
                        (20.0 * 1000.0_f32.powf(raw as f32)).clamp(240.0, 20000.0);
                }
            }
            "resonance" | "reso" | "filter_resonance" => {
                if (0.0..=1.0).contains(&raw) {
                    poly.filter_resonance = v;
                }
            }
            "drive" | "filter_drive" => {
                if raw >= 0.0 {
                    poly.filter_drive = v;
                }
            }
            "sub_level" | "sub" => {
                if (0.0..=1.0).contains(&raw) {
                    poly.sub_level = v;
                }
            }
            "noise_level" | "noise" => {
                if (0.0..=1.0).contains(&raw) {
                    poly.noise_level = v;
                }
            }
            "width" | "stereo_width" => {
                if raw >= 0.0 {
                    poly.stereo_width = v;
                }
            }
            "output_gain" | "output" | "gain" | "volume" => {
                if raw < 0.0 && raw >= -60.0 {
                    poly.output_gain = 10.0_f32.powf((raw as f32) / 20.0).clamp(0.65, 1.0);
                } else if raw > 0.0 && raw <= 1.0 {
                    poly.output_gain = (raw as f32).clamp(0.65, 1.0);
                }
            }
            "attack" | "amp_attack" => {
                if raw >= 0.0 {
                    poly.amp_attack_ms = 1.0 + v * 2000.0;
                }
            }
            "decay" | "amp_decay" => {
                if raw >= 0.0 {
                    poly.amp_decay_ms = 5.0 + v * 3000.0;
                }
            }
            "sustain" | "amp_sustain" => {
                if raw > 0.0 && raw <= 1.0 {
                    poly.amp_sustain = v.clamp(0.35, 1.0);
                }
            }
            "release" | "amp_release" => {
                if raw >= 0.0 {
                    poly.amp_release_ms = 5.0 + v * 4000.0;
                }
            }
            _ => {}
        }
    }
    poly.params_rev = poly.params_rev.wrapping_add(1);
}

fn slot_from_bw_node(
    node: &crate::project::bwdevice_shim::BwDeviceNode,
    preset_stem: &str,
    grid_patch: Option<&hikaru_project_file::OpenGridParams>,
) -> crate::views::mixer::DspSlot {
    let (slot_name, params_opt, macro_mappings) = match &node.translation {
        crate::project::bwdevice_shim::BwDeviceTranslation::Builtin {
            hikaru_slot_name,
            params,
            macro_mappings,
            ..
        } => (hikaru_slot_name.clone(), Some(params), macro_mappings.as_slice()),
        crate::project::bwdevice_shim::BwDeviceTranslation::Placeholder(p) => (
            format!("{} (Read-Only)", p.bitwig_name),
            None,
            p.macro_mappings.as_slice(),
        ),
    };

    let mut slot = crate::views::mixer::DspSlot::new(0, slot_name);
    slot.preset_name = preset_stem.to_string();
    slot.modulators_open = !node.modulators.slots.is_empty() || node.modulators.open;

    if let Some(params) = params_opt {
        if slot.is_open_poly() {
            apply_open_poly_translated_params(slot.open_poly_mut(), params);
            slot.open_poly_mut().preset_name = preset_stem.to_string();
        }
    }
    if slot.is_open_grid() {
        if let Some(gp) = grid_patch {
            slot.open_grid_mut().apply_project_params(gp);
            slot.open_grid_mut().preset_name = preset_stem.to_string();
        }
    }

    if let Some(page) = slot.remotes.pages.first_mut() {
        for m in macro_mappings {
            if let Some(rslot) = page.slots.get_mut(m.slot_index) {
                rslot.custom_label = Some(m.param_name.clone());
            }
        }
    }

    for chain in &node.nested_chains {
        for child_node in &chain.devices {
            slot.wet_fx_chain
                .push(slot_from_bw_node(child_node, preset_stem, None));
        }
    }

    slot
}

pub fn load_device_preset_from_path(app: &mut AppState, track_idx: usize, path: &std::path::Path) {
    if crate::project::bwmodulator_shim::is_bwmodulator_file(path) {
        if let Ok(translation) =
            crate::project::bwmodulator_shim::load_bwmodulator_file(path, None)
        {
            let slot_idx = app
                .current_tracks()
                .get(track_idx)
                .and_then(|t| {
                    (!t.effects.is_empty())
                        .then(|| app.selected_slot_index.min(t.effects.len() - 1))
                })
                .unwrap_or(0);
            crate::project::bwmodulator_shim::instantiate_bwmodulator_in_bay(
                track_idx,
                slot_idx,
                &translation,
            );
            let _ = hikaru_mcp_port::record_bitwig_drop(path, "dsp-rack-modulator-load");
        }
        return;
    }

    if crate::project::bwdevice_shim::is_bitwig_browser_artifact(path) {
        let _ = hikaru_mcp_port::record_bitwig_drop(path, "dsp-rack-load");
        let fallback_stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Bitwig Preset");

        if let Ok(node) = crate::project::bwdevice_shim::load_bitwig_browser_item(path, None) {
            let grid_patch = std::fs::read(path).ok().and_then(|bytes| {
                let doc = crate::project::btwg_reader::BtWgReader::parse_document(&bytes).ok()?;
                Some(crate::project::bwdevice_shim::translate_grid_patch(&doc))
            });

            let new_slot = slot_from_bw_node(&node, fallback_stem, grid_patch.as_ref());
            let slot_name = new_slot.name.clone();
            let mut inserted_slot = None;
            if let Some(track) = app.current_tracks_mut().get_mut(track_idx) {
                let idx = track.effects.len();
                track.effects.push(new_slot);
                app.selected_slot_index = idx;
                inserted_slot = Some(idx);
            }
            if let Some(slot_idx) = inserted_slot {
                crate::views::hikaru_native_plugins::hikaru_openmodulation::restore_bay_params(
                    track_idx,
                    slot_idx,
                    &node.modulators,
                );
                if crate::views::hikaru_native_plugins::hikaru_opencomb::is_opencomb_name(&slot_name)
                    || crate::views::hikaru_native_plugins::hikaru_bit8::is_bit8_name(&slot_name)
                    || crate::views::hikaru_native_plugins::hikaru_freq_shifter_plus::is_freq_shifter_plus_name(&slot_name)
                    || crate::views::hikaru_native_plugins::hikaru_openphaser_plus::is_openphaser_plus_name(&slot_name)
                    || crate::views::hikaru_native_plugins::hikaru_openflanger_plus::is_openflanger_plus_name(&slot_name)
                    || crate::views::hikaru_native_plugins::hikaru_tier_s_fx::is_tier_s_fx(&slot_name)
                {
                    if let Some(et_idx) = track_idx.checked_sub(1) {
                        app.audio_proxy.send(crate::audio_proxy::GuiCommand::LoadNativeEffect {
                            track_idx: et_idx,
                            slot_idx,
                            name: slot_name,
                        });
                    }
                }
                app.project_dirty = true;
            }
            return;
        }

        let inspected = hikaru_mcp_port::bitwig::inspect_file(path, None).ok();
        let bw_name = inspected
            .as_ref()
            .and_then(|v| {
                v["contrato_publico"]["meta"]["device_name"]
                    .as_str()
                    .or_else(|| v["nombre_inferido"].as_str())
            })
            .unwrap_or(fallback_stem);

        let slot_name = hikaru_mcp_port::bitwig::resolve_hikaru_device_name(bw_name)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("{bw_name} (Read-Only)"));

        let new_slot = crate::views::mixer::DspSlot::new(0, slot_name);
        if let Some(track) = app.current_tracks_mut().get_mut(track_idx) {
            track.effects.push(new_slot);
            app.selected_slot_index = track.effects.len() - 1;
        }
        return;
    }

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

fn paint_rotated_label(
    painter: &egui::Painter,
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

struct DeviceSidebarOutcome {
    toggled_power: bool,
    clicked_folder: bool,
    folder_resp: egui::Response,
    clicked_title: bool,
    clicked_modulators: bool,
}

fn render_bitwig_device_sidebar(
    ui: &mut egui::Ui,
    rect: Rect,
    id_salt: impl std::hash::Hash + Copy,
    title: &str,
    active: bool,
    is_selected: bool,
) -> DeviceSidebarOutcome {
    let bg = if is_selected {
        Color32::from_rgb(0x36, 0x39, 0x41)
    } else {
        Color32::from_rgb(0x26, 0x28, 0x2D)
    };
    let p = ui.painter();
    p.rect_filled(
        rect,
        egui::CornerRadius {
            nw: 3,
            ne: 0,
            sw: 3,
            se: 0,
        },
        bg,
    );
    p.line_segment(
        [pos2(rect.max.x, rect.min.y), pos2(rect.max.x, rect.max.y)],
        Stroke::new(1.0_f32, Color32::from_rgb(0x1C, 0x1E, 0x23)),
    );

    // Power switch (arriba)
    let pwr_r = Rect::from_center_size(pos2(rect.center().x, rect.min.y + 13.0), vec2(14.0, 14.0));
    let pwr_resp = ui.interact(pwr_r, ui.id().with((id_salt, "pwr")), Sense::click());
    icons::paint_power_switch(
        ui.painter(),
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
    let toggled_power = pwr_resp.on_hover_text("Device Active / Bypass").clicked();

    // Folder / Preset icon (debajo del power)
    let fld_r = Rect::from_center_size(pos2(rect.center().x, rect.min.y + 32.0), vec2(13.0, 13.0));
    let fld_resp = ui
        .interact(fld_r, ui.id().with((id_salt, "fld")), Sense::click())
        .on_hover_text("Presets & Wavetable (.wav / .wt / .hdp)");
    icons::paint_icon(
        ui.painter(),
        fld_r,
        icons::FOLDER,
        if fld_resp.hovered() {
            Color32::WHITE
        } else {
            Color32::from_rgb(0x9C, 0xA2, 0xAC)
        },
        1.5,
    );
    if fld_resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let clicked_folder = fld_resp.clicked();

    // Título vertical rotado (centro)
    let title_r = Rect::from_min_max(
        pos2(rect.min.x, rect.min.y + 42.0),
        pos2(rect.max.x, rect.max.y - 24.0),
    );
    let title_resp = ui.interact(title_r, ui.id().with((id_salt, "title")), Sense::click());
    let title_col = if is_selected {
        Color32::WHITE
    } else {
        Color32::from_rgb(0xC6, 0xCA, 0xD2)
    };
    paint_rotated_label(
        ui.painter(),
        rect.center().x,
        title_r.max.y - 4.0,
        21,
        title,
        9.5,
        title_col,
    );
    let clicked_title = title_resp.clicked();

    // Ícono moduladores / remoto (abajo)
    let mod_r = Rect::from_center_size(pos2(rect.center().x, rect.max.y - 12.0), vec2(14.0, 14.0));
    let mod_resp = ui
        .interact(mod_r, ui.id().with((id_salt, "mod")), Sense::click())
        .on_hover_text("Toggle Modulators Panel");
    icons::paint_icon(
        ui.painter(),
        mod_r.shrink(1.0),
        icons::LINK,
        if mod_resp.hovered() {
            theme::ACCENT_ORANGE
        } else {
            Color32::from_rgb(0x88, 0x8E, 0x98)
        },
        1.4,
    );
    if mod_resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let clicked_modulators = mod_resp.clicked();

    DeviceSidebarOutcome {
        toggled_power,
        clicked_folder,
        folder_resp: fld_resp,
        clicked_title,
        clicked_modulators,
    }
}

fn render_dms_fx_plus_slot(
    ui: &mut egui::Ui,
    app: &mut AppState,
    track_idx: usize,
    slot_idx: usize,
    sel_pad: usize,
    note_lbl: &str,
    card_h: f32,
    salt: &str,
) {
    let (slot_col, _) = ui.allocate_exact_size(vec2(18.0, card_h), Sense::hover());
    let plus_btn_rect = Rect::from_center_size(slot_col.center(), vec2(16.0, 26.0));
    let plus_id = ui
        .id()
        .with(("dms_fx_insert_btn", track_idx, slot_idx, sel_pad, salt));
    let plus_resp = ui
        .interact(plus_btn_rect, plus_id, Sense::click())
        .on_hover_text(format!("Insert device ({note_lbl})"));
    if ui.is_rect_visible(plus_btn_rect) {
        let p = ui.painter();
        let dot_center = pos2(plus_btn_rect.center().x, plus_btn_rect.min.y + 4.0);
        p.circle_filled(dot_center, 2.0, Color32::from_rgb(0x40, 0x6A, 0x80));
        p.text(
            pos2(plus_btn_rect.center().x, plus_btn_rect.center().y + 3.0),
            Align2::CENTER_CENTER,
            "+",
            fonts::semibold(13.0),
            if plus_resp.hovered() {
                Color32::WHITE
            } else {
                theme::TEXT_SECONDARY
            },
        );
    }
    if plus_resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let popup_id = ui
        .id()
        .with(("dms_fx_insert_popup", track_idx, slot_idx, sel_pad, salt));
    if plus_resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }
    egui::popup_below_widget(
        ui,
        popup_id,
        &plus_resp,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(185.0);
            ui.label(
                RichText::new(format!("Insert Device ({note_lbl})"))
                    .font(fonts::semibold(11.0))
                    .color(theme::ACCENT_ORANGE),
            );
            ui.separator();
            if ui.button("Hikaru Sampler  [Sampler]").clicked() {
                if let Some(track) = app.current_tracks_mut().get_mut(track_idx) {
                    if let Some(s) = track.effects.get_mut(slot_idx) {
                        if let Some(ref mut dms) = s.dms_state {
                            if let Some(pad) = dms.pads.get_mut(sel_pad) {
                                if pad.sampler.is_none() {
                                    pad.sampler = Some(
                                        crate::views::hikaru_native_plugins::HikaruSampler::default(),
                                    );
                                }
                            }
                        }
                    }
                }
                ui.close_menu();
            }
        },
    );
}

fn wet_fx_nested_card_width(wet_slot: &crate::views::mixer::DspSlot) -> f32 {
    if wet_slot.external.is_some() {
        crate::views::external_plugin_card::wet_fx_card_width(wet_slot)
    } else if let Some(w) =
        crate::views::hikaru_native_plugins::hikaru_tier_s_fx::card_width(&wet_slot.name)
    {
        w
    } else if hikaru_openphaser_plus::is_openphaser_plus_name(&wet_slot.name) {
        OPEN_PHASER_PLUS_WET_CARD_WIDTH
    } else if hikaru_openflanger_plus::is_openflanger_plus_name(&wet_slot.name) {
        OPEN_FLANGER_PLUS_WET_CARD_WIDTH
    } else if hikaru_opencomb::is_opencomb_name(&wet_slot.name) {
        OPENCOMB_WET_CARD_WIDTH
    } else {
        220.0
    }
}

/// Renderiza la cadena de dispositivos anidada `Wet FX` de `Hikaru Open8-Bit`
/// exactamente como Bitwig Studio (`Bit-8 -> Wet FX`): un marco contenedor acoplado
/// al borde derecho con dispositivos internos en serie y su propio botón `+` para
/// insertar efectos de audio dentro de la señal wet.
fn render_bit8_wet_fx_chain(
    ui: &mut egui::Ui,
    app: &mut AppState,
    track_idx: usize,
    parent_slot_idx: usize,
    card_h: f32,
) {
    let wet_slots = app
        .current_tracks()
        .get(track_idx)
        .and_then(|t| t.effects.get(parent_slot_idx))
        .map(|s| s.wet_fx_chain.clone())
        .unwrap_or_default();

    let pad_x = 6.0_f32;
    let gap = 5.0_f32;
    let plus_w = 22.0_f32;
    let top_bar_h = 13.0_f32;
    let bot_pad = 5.0_f32;

    let total_w = if wet_slots.is_empty() {
        24.0
    } else {
        let cards_w: f32 = wet_slots.iter().map(wet_fx_nested_card_width).sum();
        let gaps_w = gap * wet_slots.len() as f32;
        pad_x * 2.0 + cards_w + gaps_w + plus_w
    };

    let (outer_rect, _) = ui.allocate_exact_size(vec2(total_w, card_h), Sense::hover());

    // Marco contenedor anidado estilo Bitwig (borde gris-azulado + barra superior cyan)
    let frame_rounding = egui::CornerRadius {
        nw: 2,
        sw: 2,
        ne: 4,
        se: 4,
    };
    let bracket_accent = Color32::from_rgb(0x40, 0x6A, 0x80);
    let border_col = Color32::from_rgb(0x74, 0x7C, 0x88);
    {
        let p = ui.painter();
        p.rect_filled(outer_rect, frame_rounding, Color32::from_rgb(0x14, 0x16, 0x1A));
        p.rect_stroke(
            outer_rect,
            frame_rounding,
            Stroke::new(1.25_f32, border_col),
            egui::StrokeKind::Inside,
        );
        // Barra superior de acento Wet FX conectando visualmente con Open8-Bit
        let bar_r = Rect::from_min_max(
            pos2(outer_rect.min.x + 1.0, outer_rect.min.y + 1.0),
            pos2(outer_rect.max.x - 1.0, outer_rect.min.y + 4.0),
        );
        p.rect_filled(bar_r, 1.0, bracket_accent);

        if !wet_slots.is_empty() {
            p.text(
                pos2(outer_rect.min.x + 7.0, outer_rect.min.y + 8.5),
                Align2::LEFT_CENTER,
                "WET FX",
                fonts::semibold(8.0),
                theme::BW_LCD_CYAN,
            );
        }
    }

    let mut wet_to_remove: Option<usize> = None;
    let mut cursor_x = outer_rect.min.x + if wet_slots.is_empty() { 1.0 } else { pad_x };
    let nested_card_top = outer_rect.min.y + top_bar_h;
    let nested_card_bot = outer_rect.max.y - bot_pad;
    let nested_card_h = (nested_card_bot - nested_card_top).max(120.0);

    for (wet_idx, wet_slot) in wet_slots.iter().enumerate() {
        let w = wet_fx_nested_card_width(wet_slot);
        let nested_rect = Rect::from_min_size(pos2(cursor_x, nested_card_top), vec2(w, nested_card_h));
        cursor_x += w + gap;

        if wet_slot.external.is_some() {
            let p = ui.painter();
            p.rect_filled(nested_rect, 3.5, Color32::from_rgb(0x2E, 0x30, 0x35));
            p.rect_stroke(
                nested_rect,
                3.5,
                Stroke::new(
                    1.0_f32,
                    if wet_slot.active {
                        Color32::from_rgb(0x46, 0x49, 0x52)
                    } else {
                        Color32::from_rgb(0x34, 0x36, 0x3C)
                    },
                ),
                egui::StrokeKind::Inside,
            );
            crate::views::external_plugin_card::render_wet_fx_card(
                ui,
                app,
                nested_rect,
                track_idx,
                parent_slot_idx,
                wet_idx,
                &mut wet_to_remove,
            );
            continue;
        }

        let p = ui.painter();
        p.rect_filled(nested_rect, 3.0, Color32::from_rgb(0x2B, 0x2D, 0x33));
        p.rect_stroke(
            nested_rect,
            3.0,
            Stroke::new(
                1.0_f32,
                if wet_slot.active {
                    Color32::from_rgb(0x4A, 0x4E, 0x58)
                } else {
                    Color32::from_rgb(0x34, 0x36, 0x3C)
                },
            ),
            egui::StrokeKind::Inside,
        );

        let inner_rect = nested_rect.shrink2(vec2(5.0, 4.0));
        let inner_w = inner_rect.width();
        let mut card_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        card_ui.set_clip_rect(inner_rect.intersect(ui.clip_rect()));
        card_ui.spacing_mut().item_spacing = vec2(4.0, 2.0);

        // Barra superior del dispositivo anidado: Power + Título + Quitar (×)
        let (hdr_rect, _) = card_ui.allocate_exact_size(vec2(inner_w, 19.0), Sense::hover());
        {
            let active = wet_slot.active;
            let pwr_r = Rect::from_min_size(
                pos2(hdr_rect.min.x, hdr_rect.center().y - 8.0),
                vec2(16.0, 16.0),
            );
            let pwr_resp = card_ui.interact(
                pwr_r,
                card_ui.id().with(("bit8_wet_pwr", track_idx, parent_slot_idx, wet_idx)),
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
            if pwr_resp.on_hover_text("Wet FX Active / Bypass").clicked() {
                let mut new_active = None;
                if let Some(t) = app.current_tracks_mut().get_mut(track_idx) {
                    if let Some(parent) = t.effects.get_mut(parent_slot_idx) {
                        if let Some(ws) = parent.wet_fx_chain.get_mut(wet_idx) {
                            ws.active = !ws.active;
                            new_active = Some(ws.active);
                        }
                    }
                }
                if let (Some(et_idx), Some(act)) = (track_idx.checked_sub(1), new_active) {
                    app.audio_proxy
                        .send(crate::audio_proxy::GuiCommand::SetBit8WetFxActive {
                            track_idx: et_idx,
                            slot_idx: parent_slot_idx,
                            wet_idx,
                            active: act,
                        });
                }
                app.project_dirty = true;
            }

            let title_clip = Rect::from_min_max(
                pos2(pwr_r.max.x + 4.0, hdr_rect.min.y),
                pos2(hdr_rect.max.x - 18.0, hdr_rect.max.y),
            );
            card_ui.painter().with_clip_rect(title_clip).text(
                pos2(title_clip.min.x, hdr_rect.center().y),
                Align2::LEFT_CENTER,
                &wet_slot.name,
                fonts::semibold(10.5),
                if active {
                    theme::TEXT_PRIMARY
                } else {
                    theme::TEXT_MUTED
                },
            );

            let close_r = Rect::from_min_size(
                pos2(hdr_rect.max.x - 16.0, hdr_rect.center().y - 8.0),
                vec2(16.0, 16.0),
            );
            let close_resp = card_ui.interact(
                close_r,
                card_ui
                    .id()
                    .with(("bit8_wet_close", track_idx, parent_slot_idx, wet_idx)),
                Sense::click(),
            );
            if close_resp.hovered() {
                card_ui
                    .painter()
                    .rect_filled(close_r, 2.5, Color32::from_rgb(0x48, 0x4A, 0x52));
            }
            icons::paint_icon(
                card_ui.painter(),
                close_r.shrink(3.0),
                icons::X_CLOSE,
                if close_resp.hovered() {
                    Color32::WHITE
                } else {
                    theme::TEXT_MUTED
                },
                1.5,
            );
            if close_resp.on_hover_text("Remove from Wet FX").clicked() {
                wet_to_remove = Some(wet_idx);
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
            pos2(inner_rect.min.x, (sep_y + 2.0).min(inner_rect.max.y)),
            inner_rect.max,
        );
        let mut body_ui = card_ui.new_child(
            egui::UiBuilder::new()
                .max_rect(body_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        body_ui.set_clip_rect(body_rect.intersect(card_ui.clip_rect()));

        if crate::views::hikaru_native_plugins::hikaru_tier_s_fx::is_tier_s_fx(&wet_slot.name) {
            crate::views::hikaru_native_plugins::hikaru_tier_s_fx::render_wet_fx_module(
                &mut body_ui,
                app,
                track_idx,
                parent_slot_idx,
                wet_idx,
                &wet_slot.name,
            );
        } else if hikaru_openphaser_plus::is_openphaser_plus_name(&wet_slot.name) {
            hikaru_openphaser_plus::render_wet_fx_module(
                &mut body_ui,
                app,
                track_idx,
                parent_slot_idx,
                wet_idx,
            );
        } else if hikaru_openflanger_plus::is_openflanger_plus_name(&wet_slot.name) {
            hikaru_openflanger_plus::render_wet_fx_module(
                &mut body_ui,
                app,
                track_idx,
                parent_slot_idx,
                wet_idx,
            );
        } else if hikaru_opencomb::is_opencomb_name(&wet_slot.name) {
            hikaru_opencomb::render_wet_fx_module(
                &mut body_ui,
                app,
                track_idx,
                parent_slot_idx,
                wet_idx,
            );
        } else if wet_slot.name == "OpenEQ3" {
            hikaru_openeq3::render_module(&mut body_ui);
        } else {
            body_ui.vertical_centered(|ui| {
                ui.add_space(16.0);
                ui.label(
                    RichText::new(&wet_slot.name)
                        .font(fonts::semibold(11.0))
                        .color(theme::TEXT_SECONDARY),
                );
                ui.label(
                    RichText::new("Wet FX Device")
                        .font(fonts::regular(9.5))
                        .color(theme::TEXT_MUTED),
                );
            });
        }
    }

    if let Some(rem_idx) = wet_to_remove {
        let wet_wkey = crate::views::external_plugins_settings::wet_fx_window_key(
            track_idx,
            parent_slot_idx,
            rem_idx,
        );
        app.plugin_settings_state.close_window(&wet_wkey);
        if let Some(t) = app.current_tracks_mut().get_mut(track_idx) {
            if let Some(parent) = t.effects.get_mut(parent_slot_idx) {
                if rem_idx < parent.wet_fx_chain.len() {
                    parent.wet_fx_chain.remove(rem_idx);
                }
            }
        }
        crate::views::hikaru_native_plugins::hikaru_tier_s_fx::remove_wet_fx_slot_state(
            track_idx,
            parent_slot_idx,
            rem_idx,
        );
        hikaru_openphaser_plus::remove_wet_fx_slot_state(track_idx, parent_slot_idx, rem_idx);
        hikaru_openflanger_plus::remove_wet_fx_slot_state(track_idx, parent_slot_idx, rem_idx);
        hikaru_opencomb::remove_wet_fx_slot_state(track_idx, parent_slot_idx, rem_idx);
        if let Some(et_idx) = track_idx.checked_sub(1) {
            app.audio_proxy
                .send(crate::audio_proxy::GuiCommand::RemoveBit8WetFx {
                    track_idx: et_idx,
                    slot_idx: parent_slot_idx,
                    wet_idx: rem_idx,
                });
        }
        app.project_dirty = true;
    }

    // Botón `+` interno dentro del marco Wet FX (exactamente como Bitwig Studio)
    let plus_center = pos2(
        outer_rect.max.x - (plus_w * 0.5) - 1.0,
        outer_rect.center().y,
    );
    let plus_rect = Rect::from_center_size(plus_center, vec2(18.0, 28.0));
    let plus_resp = ui
        .interact(
            plus_rect,
            ui.id().with(("bit8_wet_fx_plus", track_idx, parent_slot_idx)),
            Sense::click(),
        )
        .on_hover_text("Insert Audio FX into Wet FX chain");
    if plus_resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        ui.painter().rect_filled(
            plus_rect.shrink2(vec2(1.0, 4.0)),
            3.0,
            Color32::from_rgb(0x2A, 0x33, 0x3E),
        );
    }
    ui.painter().text(
        plus_rect.center(),
        Align2::CENTER_CENTER,
        "+",
        fonts::semibold(14.0),
        if plus_resp.hovered() {
            Color32::WHITE
        } else {
            Color32::from_rgb(0xB8, 0xC0, 0xCC)
        },
    );
    if plus_resp.clicked() {
        app.popup_browser
            .open_insert_wet_fx(track_idx, parent_slot_idx);
    }
}

pub fn render(ui: &mut egui::Ui, app: &mut AppState) {
    crate::views::remote_controls::sync_global_mapping_flag(app);
    let track_idx = app.safe_track_index();
    let track_name = app
        .current_tracks()
        .get(track_idx)
        .map(|t| t.name.clone())
        .unwrap_or_else(|| "Track".into());

    // =========================================================================
    // Pozo negro profundo `#0B0C0E` de la cadena de dispositivos Bitwig:
    // [ PROJECT (Remotes/Mods) ] [ <TRACK> (Remotes/Mods) ] [ Devices... ] [ + ]
    // Tomamos el rectángulo completo ANTES de cualquier `ui.horizontal` para que
    // ocupe el 100% de la altura del panel inferior (como Bitwig Studio).
    // =========================================================================
    let well_rect = ui.available_rect_before_wrap();
    ui.allocate_rect(well_rect, Sense::hover());
    ui.painter().rect_filled(well_rect, 0.0, theme::BW_DARK_WELL);

    // Aceptar drag & drop desde el Explorador interno (`.hdp`, `.bwdevice`, `.bwpreset`)
    let is_dragging_preset = app
        .dragged_sample
        .as_ref()
        .map(crate::views::explorer::is_preset_file)
        .unwrap_or(false);
    if is_dragging_preset {
        let pointer_pos = ui.input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()));
        let hovered = pointer_pos.map(|p| well_rect.contains(p)).unwrap_or(false);
        if hovered {
            ui.painter().rect_stroke(
                well_rect.shrink(1.0),
                2.0,
                egui::Stroke::new(1.5_f32, theme::ACCENT_ORANGE),
                egui::StrokeKind::Inside,
            );
            if ui.input(|i| i.pointer.any_released()) {
                if let Some(path) = app.dragged_sample.take() {
                    load_device_preset_from_path(app, track_idx, &path);
                    app.show_dsp_rack = true;
                }
            }
        }
    }
    let content_rect = well_rect.shrink2(vec2(5.0, 4.0));
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

                let pill_h = (content_rect.height() - 2.0).max(180.0);

                // 1. Panel PROJECT (Project Remotes + Project Modulators)
                crate::views::remote_controls::render_project_panel(ui, app, pill_h);

                // 2. Panel de la pista seleccionada (Track Remotes + Track Modulators)
                let mut req_save_chain = false;
                let mut req_load_chain = false;
                crate::views::remote_controls::render_track_panel(
                    ui,
                    app,
                    track_idx,
                    pill_h,
                    &mut req_save_chain,
                    &mut req_load_chain,
                );
                if req_save_chain {
                    save_device_chain_dialog(app, track_idx);
                }
                if req_load_chain {
                    load_device_chain_dialog(app, track_idx);
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
                        let card_w = if slot.external.is_some() {
                            crate::views::external_plugin_card::card_width(&slot, track_idx, slot_idx)
                        } else {
                            match slot.name.as_str() {
                                "OpenPoly" | "Hikaru OpenPoly" | "OpenWavetable" | "Hikaru OpenWavetable" => {
                                    let mods_open = slot
                                        .open_poly
                                        .as_ref()
                                        .map(|p| p.modulators_open)
                                        .unwrap_or(true);
                                    if mods_open {
                                        OPEN_POLY_COMPACT_CARD_WIDTH
                                            + hikaru_openmodulation::bay_width_for_slot(
                                                track_idx, slot_idx,
                                            )
                                    } else {
                                        OPEN_POLY_COMPACT_CARD_WIDTH
                                    }
                                }
                                "OpenGrid" | "Hikaru OpenGrid" | "Poly Grid" => {
                                    let mods_open = slot
                                        .open_grid
                                        .as_ref()
                                        .map(|g| g.modulators_open)
                                        .unwrap_or(true);
                                    if mods_open {
                                        OPEN_GRID_COMPACT_CARD_WIDTH
                                            + hikaru_openmodulation::bay_width_for_slot(
                                                track_idx, slot_idx,
                                            )
                                    } else {
                                        OPEN_GRID_COMPACT_CARD_WIDTH
                                    }
                                }
                                "Hikaru OpenDMS" => OPEN_DMS_CARD_WIDTH,
                                "Sampler" | "Hikaru Sampler" => SAMPLER_CARD_WIDTH,
                                "OpenSpectral Phaser" | "OpenSpectralPhaser" => PHASER_CARD_WIDTH,
                                name if hikaru_opencomb::is_opencomb_name(name) => OPENCOMB_CARD_WIDTH,
                                name if hikaru_bit8::is_bit8_name(name) => {
                                    hikaru_bit8::card_width_for_slot(track_idx, slot_idx)
                                }
                                name if hikaru_freq_shifter_plus::is_freq_shifter_plus_name(name) => {
                                    hikaru_freq_shifter_plus::card_width_for_slot(track_idx, slot_idx)
                                }
                                name if hikaru_openphaser_plus::is_openphaser_plus_name(name) => {
                                    OPEN_PHASER_PLUS_CARD_WIDTH
                                }
                                name if hikaru_openflanger_plus::is_openflanger_plus_name(name) => {
                                    OPEN_FLANGER_PLUS_CARD_WIDTH
                                }
                                "OpenLFO MOD" | "Hikaru OpenLFO MOD" | "LFO MOD" | "OpenModulation" | "Hikaru OpenModulation" => MODULATION_CARD_WIDTH,
                                name if crate::views::hikaru_native_plugins::hikaru_tier_s_fx::is_tier_s_fx(name) => {
                                    crate::views::hikaru_native_plugins::hikaru_tier_s_fx::card_width(name).unwrap_or(CARD_WIDTH)
                                }
                                _ => CARD_WIDTH,
                            }
                        };
                        let card_h = pill_h;
                        let (card_rect, _) =
                            ui.allocate_exact_size(vec2(card_w, card_h), Sense::hover());

                        let p = ui.painter();
                        p.rect_filled(card_rect, 3.5, Color32::from_rgb(0x2E, 0x30, 0x35));
                        let is_lfo_mod_card = matches!(
                            slot.name.as_str(),
                            "OpenLFO MOD"
                                | "Hikaru OpenLFO MOD"
                                | "LFO MOD"
                                | "OpenModulation"
                                | "Hikaru OpenModulation"
                        );
                        let mod_scope_active = if slot.external.is_some() {
                            hikaru_openmodulation::active_mapping_source_for_external(
                                track_idx, slot_idx,
                            )
                            .is_some()
                        } else {
                            !is_lfo_mod_card
                                && hikaru_openmodulation::active_mapping_for_track(track_idx)
                                    .is_some()
                        };
                        p.rect_stroke(
                            card_rect,
                            3.5,
                            Stroke::new(
                                if is_selected || mod_scope_active { 1.5_f32 } else { 1.0_f32 },
                                if mod_scope_active {
                                    hikaru_openmodulation::MOD_ARROW_CYAN
                                } else if is_selected {
                                    theme::ACCENT_ORANGE
                                } else {
                                    Color32::from_rgb(0x46, 0x49, 0x52)
                                },
                            ),
                            egui::StrokeKind::Inside,
                        );
                        if mod_scope_active {
                            // Bitwig-style top cyan modulation scope bar above target device
                            let bar_r = Rect::from_min_size(
                                pos2(card_rect.min.x + 1.0, card_rect.min.y - 3.5),
                                vec2(card_rect.width() - 2.0, 2.5),
                            );
                            p.rect_filled(bar_r, 1.0, hikaru_openmodulation::MOD_ARROW_CYAN);
                        }

                        if slot.external.is_some() {
                            crate::views::external_plugin_card::render_card(
                                ui,
                                app,
                                card_rect,
                                track_idx,
                                slot_idx,
                                is_selected,
                                &mut slot_to_remove,
                            );
                        } else if slot.is_open_poly() {
                            let sb_rect = Rect::from_min_size(card_rect.min, vec2(22.0, card_rect.height()));
                            let sb_out = render_bitwig_device_sidebar(
                                ui,
                                sb_rect,
                                ("openpoly_sb", track_idx, slot_idx),
                                "OPENPOLY",
                                slot.active,
                                is_selected,
                            );
                            if sb_out.toggled_power {
                                if let Some(t) = app.current_tracks_mut().get_mut(track_idx) {
                                    if let Some(s) = t.effects.get_mut(slot_idx) {
                                        s.active = !s.active;
                                    }
                                }
                            }
                            if sb_out.clicked_title {
                                app.selected_slot_index = slot_idx;
                                app.matrix_state.selected_scene = None;
                            }
                            if sb_out.clicked_modulators {
                                if let Some(t) = app.current_tracks_mut().get_mut(track_idx) {
                                    if let Some(s) = t.effects.get_mut(slot_idx) {
                                        let poly = s.open_poly_mut();
                                        poly.modulators_open = !poly.modulators_open;
                                        poly.mark_params_dirty();
                                    }
                                }
                            }
                            let preset_popup_id = ui.id().with(("openpoly_preset_popup", track_idx, slot_idx));
                            if sb_out.clicked_folder {
                                ui.memory_mut(|m| m.toggle_popup(preset_popup_id));
                            }
                            egui::popup_below_widget(
                                ui,
                                preset_popup_id,
                                &sb_out.folder_resp,
                                egui::PopupCloseBehavior::CloseOnClickOutside,
                                |ui| {
                                    hikaru_openpoly::open_poly_ui::render_preset_menu(
                                        ui, app, track_idx, slot_idx,
                                    );
                                },
                            );

                            let body_rect = Rect::from_min_max(
                                pos2(sb_rect.max.x + 3.0, card_rect.min.y + 4.0),
                                pos2(card_rect.max.x - 4.0, card_rect.max.y - 4.0),
                            );
                            let mut body_ui = ui.new_child(
                                egui::UiBuilder::new()
                                    .max_rect(body_rect)
                                    .layout(egui::Layout::top_down(egui::Align::Min)),
                            );
                            body_ui.set_clip_rect(body_rect.intersect(ui.clip_rect()));
                            hikaru_openpoly::render_module(&mut body_ui, app, track_idx, slot_idx);

                            let close_r = Rect::from_min_size(
                                pos2(card_rect.max.x - 15.0, card_rect.min.y + 2.0),
                                vec2(13.0, 13.0),
                            );
                            let close_resp = ui.interact(
                                close_r,
                                ui.id().with(("dsp_close", slot_idx)),
                                Sense::click(),
                            );
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
                            if close_resp.on_hover_text("Remove Device (Del)").clicked() {
                                slot_to_remove = Some(slot_idx);
                            }
                        } else if slot.is_open_grid() {
                            let sb_rect = Rect::from_min_size(card_rect.min, vec2(22.0, card_rect.height()));
                            let sb_out = render_bitwig_device_sidebar(
                                ui,
                                sb_rect,
                                ("opengrid_sb", track_idx, slot_idx),
                                "OPENGRID",
                                slot.active,
                                is_selected,
                            );
                            if sb_out.toggled_power {
                                if let Some(t) = app.current_tracks_mut().get_mut(track_idx) {
                                    if let Some(s) = t.effects.get_mut(slot_idx) {
                                        s.active = !s.active;
                                    }
                                }
                            }
                            if sb_out.clicked_title {
                                app.selected_slot_index = slot_idx;
                                app.matrix_state.selected_scene = None;
                            }
                            if sb_out.clicked_modulators {
                                if let Some(t) = app.current_tracks_mut().get_mut(track_idx) {
                                    if let Some(s) = t.effects.get_mut(slot_idx) {
                                        let g = s.open_grid_mut();
                                        g.modulators_open = !g.modulators_open;
                                        g.mark_dirty();
                                    }
                                }
                            }
                            let preset_popup_id = ui.id().with(("opengrid_preset_popup", track_idx, slot_idx));
                            if sb_out.clicked_folder {
                                ui.memory_mut(|m| m.toggle_popup(preset_popup_id));
                            }
                            egui::popup_below_widget(
                                ui,
                                preset_popup_id,
                                &sb_out.folder_resp,
                                egui::PopupCloseBehavior::CloseOnClickOutside,
                                |ui| {
                                    hikaru_opengrid::render_preset_menu(ui, app, track_idx, slot_idx);
                                },
                            );

                            let body_rect = Rect::from_min_max(
                                pos2(sb_rect.max.x + 3.0, card_rect.min.y + 4.0),
                                pos2(card_rect.max.x - 4.0, card_rect.max.y - 4.0),
                            );
                            let mut body_ui = ui.new_child(
                                egui::UiBuilder::new()
                                    .max_rect(body_rect)
                                    .layout(egui::Layout::top_down(egui::Align::Min)),
                            );
                            body_ui.set_clip_rect(body_rect.intersect(ui.clip_rect()));
                            hikaru_opengrid::render_rack_card(&mut body_ui, app, track_idx, slot_idx);

                            let close_r = Rect::from_min_size(
                                pos2(card_rect.max.x - 15.0, card_rect.min.y + 2.0),
                                vec2(13.0, 13.0),
                            );
                            let close_resp = ui.interact(
                                close_r,
                                ui.id().with(("dsp_close", slot_idx)),
                                Sense::click(),
                            );
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
                            if close_resp.on_hover_text("Remove Device (Del)").clicked() {
                                if app.open_grid_editor == Some((track_idx, slot_idx)) {
                                    app.open_grid_editor = None;
                                }
                                slot_to_remove = Some(slot_idx);
                            }
                        } else if matches!(
                            slot.name.as_str(),
                            "OpenLFO MOD"
                                | "Hikaru OpenLFO MOD"
                                | "LFO MOD"
                                | "OpenModulation"
                                | "Hikaru OpenModulation"
                        ) {
                            let sb_rect = Rect::from_min_size(card_rect.min, vec2(22.0, card_rect.height()));
                            let sb_out = render_bitwig_device_sidebar(
                                ui,
                                sb_rect,
                                ("openlfo_mod_sb", track_idx, slot_idx),
                                "OPENLFO MOD",
                                slot.active,
                                is_selected,
                            );
                            if sb_out.toggled_power {
                                if let Some(t) = app.current_tracks_mut().get_mut(track_idx) {
                                    if let Some(s) = t.effects.get_mut(slot_idx) {
                                        s.active = !s.active;
                                    }
                                }
                            }
                            if sb_out.clicked_title {
                                app.selected_slot_index = slot_idx;
                                app.matrix_state.selected_scene = None;
                            }
                            let preset_popup_id =
                                ui.id().with(("openlfo_mod_preset_popup", track_idx, slot_idx));
                            if sb_out.clicked_folder {
                                ui.memory_mut(|m| m.toggle_popup(preset_popup_id));
                            }
                            egui::popup_below_widget(
                                ui,
                                preset_popup_id,
                                &sb_out.folder_resp,
                                egui::PopupCloseBehavior::CloseOnClickOutside,
                                |ui| {
                                    hikaru_openmodulation::render_preset_menu(
                                        ui, track_idx, slot_idx,
                                    );
                                },
                            );

                            let body_rect = Rect::from_min_max(
                                pos2(sb_rect.max.x + 3.0, card_rect.min.y + 4.0),
                                pos2(card_rect.max.x - 4.0, card_rect.max.y - 4.0),
                            );
                            let mut body_ui = ui.new_child(
                                egui::UiBuilder::new()
                                    .max_rect(body_rect)
                                    .layout(egui::Layout::top_down(egui::Align::Min)),
                            );
                            body_ui.set_clip_rect(body_rect.intersect(ui.clip_rect()));
                            hikaru_openmodulation::render_module(
                                &mut body_ui,
                                app,
                                track_idx,
                                slot_idx,
                            );

                            let close_r = Rect::from_min_size(
                                pos2(card_rect.max.x - 15.0, card_rect.min.y + 2.0),
                                vec2(13.0, 13.0),
                            );
                            let close_resp = ui.interact(
                                close_r,
                                ui.id().with(("dsp_close", slot_idx)),
                                Sense::click(),
                            );
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
                            if close_resp.on_hover_text("Remove Device (Del)").clicked() {
                                slot_to_remove = Some(slot_idx);
                            }
                        } else if slot.name == "Hikaru OpenDMS" {
                            let sb_rect = Rect::from_min_size(card_rect.min, vec2(22.0, card_rect.height()));
                            let sb_out = render_bitwig_device_sidebar(
                                ui,
                                sb_rect,
                                ("dms_sb", slot_idx),
                                "DRUM MACHINE",
                                slot.active,
                                is_selected,
                            );
                            if sb_out.toggled_power {
                                if let Some(t) = app.current_tracks_mut().get_mut(track_idx) {
                                    if let Some(s) = t.effects.get_mut(slot_idx) {
                                        s.active = !s.active;
                                    }
                                }
                            }
                            if sb_out.clicked_title {
                                app.selected_slot_index = slot_idx;
                                app.matrix_state.selected_scene = None;
                            }

                            let body_rect = Rect::from_min_max(
                                pos2(sb_rect.max.x + 3.0, card_rect.min.y + 4.0),
                                pos2(card_rect.max.x - 4.0, card_rect.max.y - 4.0),
                            );
                            let mut body_ui = ui.new_child(
                                egui::UiBuilder::new()
                                    .max_rect(body_rect)
                                    .layout(egui::Layout::top_down(egui::Align::Min)),
                            );
                            body_ui.set_clip_rect(body_rect.intersect(ui.clip_rect()));
                            render_rack_card(&mut body_ui, app, track_idx, slot_idx);

                            let close_r = Rect::from_min_size(
                                pos2(card_rect.max.x - 16.0, card_rect.min.y + 3.0),
                                vec2(13.0, 13.0),
                            );
                            let close_resp = ui.interact(
                                close_r,
                                ui.id().with(("dsp_close", slot_idx)),
                                Sense::click(),
                            );
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
                            if close_resp.on_hover_text("Remove Device (Del)").clicked() {
                                slot_to_remove = Some(slot_idx);
                            }
                        } else if hikaru_bit8::is_bit8_name(&slot.name) {
                            let sb_rect = Rect::from_min_size(card_rect.min, vec2(22.0, card_rect.height()));
                            let sb_out = render_bitwig_device_sidebar(
                                ui,
                                sb_rect,
                                ("bit8_sb", track_idx, slot_idx),
                                "OPEN8-BIT",
                                slot.active,
                                is_selected,
                            );
                            if sb_out.toggled_power {
                                let mut new_active = None;
                                if let Some(t) = app.current_tracks_mut().get_mut(track_idx) {
                                    if let Some(s) = t.effects.get_mut(slot_idx) {
                                        s.active = !s.active;
                                        new_active = Some(s.active);
                                    }
                                }
                                if let (Some(et_idx), Some(active)) = (track_idx.checked_sub(1), new_active) {
                                    app.audio_proxy.send(crate::audio_proxy::GuiCommand::SetExternalPluginActive {
                                        track_idx: et_idx,
                                        slot_idx,
                                        active,
                                    });
                                }
                            }
                            if sb_out.clicked_title {
                                app.selected_slot_index = slot_idx;
                                app.matrix_state.selected_scene = None;
                            }
                            let preset_popup_id = ui.id().with(("bit8_preset_popup", track_idx, slot_idx));
                            if sb_out.clicked_folder {
                                ui.memory_mut(|m| m.toggle_popup(preset_popup_id));
                            }
                            egui::popup_below_widget(
                                ui,
                                preset_popup_id,
                                &sb_out.folder_resp,
                                egui::PopupCloseBehavior::CloseOnClickOutside,
                                |ui| {
                                    hikaru_bit8::render_preset_menu(ui, app, track_idx, slot_idx);
                                },
                            );

                            let body_rect = Rect::from_min_max(
                                pos2(sb_rect.max.x + 2.0, card_rect.min.y + 2.0),
                                pos2(card_rect.max.x - 2.0, card_rect.max.y - 2.0),
                            );
                            let mut body_ui = ui.new_child(
                                egui::UiBuilder::new()
                                    .max_rect(body_rect)
                                    .layout(egui::Layout::top_down(egui::Align::Min)),
                            );
                            body_ui.set_clip_rect(body_rect.intersect(ui.clip_rect()));
                            hikaru_bit8::render_module(&mut body_ui, app, track_idx, slot_idx);

                            let close_r = Rect::from_min_size(
                                pos2(card_rect.max.x - 14.0, card_rect.min.y + 2.0),
                                vec2(12.0, 12.0),
                            );
                            let close_resp = ui.interact(
                                close_r,
                                ui.id().with(("dsp_close", slot_idx)),
                                Sense::click(),
                            );
                            icons::paint_icon(
                                ui.painter(),
                                close_r.shrink(2.0),
                                icons::X_CLOSE,
                                if close_resp.hovered() {
                                    Color32::WHITE
                                } else {
                                    Color32::from_rgba_unmultiplied(0x8C, 0x90, 0x9A, 150)
                                },
                                1.4,
                            );
                            if close_resp.on_hover_text("Remove Device (Del)").clicked() {
                                slot_to_remove = Some(slot_idx);
                            }
                        } else if hikaru_freq_shifter_plus::is_freq_shifter_plus_name(&slot.name) {
                            let sb_rect = Rect::from_min_size(card_rect.min, vec2(22.0, card_rect.height()));
                            let sb_out = render_bitwig_device_sidebar(
                                ui,
                                sb_rect,
                                ("fsp_sb", track_idx, slot_idx),
                                "FREQ SHIFTER+",
                                slot.active,
                                is_selected,
                            );
                            if sb_out.toggled_power {
                                let mut new_active = None;
                                if let Some(t) = app.current_tracks_mut().get_mut(track_idx) {
                                    if let Some(s) = t.effects.get_mut(slot_idx) {
                                        s.active = !s.active;
                                        new_active = Some(s.active);
                                    }
                                }
                                if let (Some(et_idx), Some(active)) = (track_idx.checked_sub(1), new_active) {
                                    app.audio_proxy.send(crate::audio_proxy::GuiCommand::SetExternalPluginActive {
                                        track_idx: et_idx,
                                        slot_idx,
                                        active,
                                    });
                                }
                            }
                            if sb_out.clicked_title {
                                app.selected_slot_index = slot_idx;
                                app.matrix_state.selected_scene = None;
                            }
                            let preset_popup_id = ui.id().with(("fsp_preset_popup", track_idx, slot_idx));
                            if sb_out.clicked_folder {
                                ui.memory_mut(|m| m.toggle_popup(preset_popup_id));
                            }
                            egui::popup_below_widget(
                                ui,
                                preset_popup_id,
                                &sb_out.folder_resp,
                                egui::PopupCloseBehavior::CloseOnClickOutside,
                                |ui| {
                                    hikaru_freq_shifter_plus::render_preset_menu(ui, app, track_idx, slot_idx);
                                },
                            );

                            let body_rect = Rect::from_min_max(
                                pos2(sb_rect.max.x + 2.0, card_rect.min.y + 2.0),
                                pos2(card_rect.max.x - 2.0, card_rect.max.y - 2.0),
                            );
                            let mut body_ui = ui.new_child(
                                egui::UiBuilder::new()
                                    .max_rect(body_rect)
                                    .layout(egui::Layout::top_down(egui::Align::Min)),
                            );
                            body_ui.set_clip_rect(body_rect.intersect(ui.clip_rect()));
                            hikaru_freq_shifter_plus::render_module(&mut body_ui, app, track_idx, slot_idx);

                            let close_r = Rect::from_min_size(
                                pos2(card_rect.max.x - 14.0, card_rect.min.y + 2.0),
                                vec2(12.0, 12.0),
                            );
                            let close_resp = ui.interact(
                                close_r,
                                ui.id().with(("dsp_close", slot_idx)),
                                Sense::click(),
                            );
                            icons::paint_icon(
                                ui.painter(),
                                close_r.shrink(2.0),
                                icons::X_CLOSE,
                                if close_resp.hovered() {
                                    Color32::WHITE
                                } else {
                                    Color32::from_rgba_unmultiplied(0x8C, 0x90, 0x9A, 150)
                                },
                                1.4,
                            );
                            if close_resp.on_hover_text("Remove Device (Del)").clicked() {
                                slot_to_remove = Some(slot_idx);
                            }
                        } else if hikaru_openflanger_plus::is_openflanger_plus_name(&slot.name) {
                            let sb_rect = Rect::from_min_size(card_rect.min, vec2(22.0, card_rect.height()));
                            let sb_out = render_bitwig_device_sidebar(
                                ui,
                                sb_rect,
                                ("flp_sb", track_idx, slot_idx),
                                "OPENFLANGER+",
                                slot.active,
                                is_selected,
                            );
                            if sb_out.toggled_power {
                                let mut new_active = None;
                                if let Some(t) = app.current_tracks_mut().get_mut(track_idx) {
                                    if let Some(s) = t.effects.get_mut(slot_idx) {
                                        s.active = !s.active;
                                        new_active = Some(s.active);
                                    }
                                }
                                if let (Some(et_idx), Some(active)) = (track_idx.checked_sub(1), new_active) {
                                    app.audio_proxy.send(crate::audio_proxy::GuiCommand::SetExternalPluginActive {
                                        track_idx: et_idx,
                                        slot_idx,
                                        active,
                                    });
                                }
                            }
                            if sb_out.clicked_title {
                                app.selected_slot_index = slot_idx;
                                app.matrix_state.selected_scene = None;
                            }
                            let preset_popup_id = ui.id().with(("flp_preset_popup", track_idx, slot_idx));
                            if sb_out.clicked_folder {
                                ui.memory_mut(|m| m.toggle_popup(preset_popup_id));
                            }
                            egui::popup_below_widget(
                                ui,
                                preset_popup_id,
                                &sb_out.folder_resp,
                                egui::PopupCloseBehavior::CloseOnClickOutside,
                                |ui| {
                                    hikaru_openflanger_plus::render_preset_menu(ui, app, track_idx, slot_idx);
                                },
                            );

                            let body_rect = Rect::from_min_max(
                                pos2(sb_rect.max.x + 2.0, card_rect.min.y + 2.0),
                                pos2(card_rect.max.x - 2.0, card_rect.max.y - 2.0),
                            );
                            let mut body_ui = ui.new_child(
                                egui::UiBuilder::new()
                                    .max_rect(body_rect)
                                    .layout(egui::Layout::top_down(egui::Align::Min)),
                            );
                            body_ui.set_clip_rect(body_rect.intersect(ui.clip_rect()));
                            hikaru_openflanger_plus::render_module(&mut body_ui, app, track_idx, slot_idx);

                            let close_r = Rect::from_min_size(
                                pos2(card_rect.max.x - 14.0, card_rect.min.y + 2.0),
                                vec2(12.0, 12.0),
                            );
                            let close_resp = ui.interact(
                                close_r,
                                ui.id().with(("dsp_close", slot_idx)),
                                Sense::click(),
                            );
                            icons::paint_icon(
                                ui.painter(),
                                close_r.shrink(2.0),
                                icons::X_CLOSE,
                                if close_resp.hovered() {
                                    Color32::WHITE
                                } else {
                                    Color32::from_rgba_unmultiplied(0x8C, 0x90, 0x9A, 150)
                                },
                                1.4,
                            );
                            if close_resp.on_hover_text("Remove Device (Del)").clicked() {
                                slot_to_remove = Some(slot_idx);
                            }
                        } else if hikaru_openphaser_plus::is_openphaser_plus_name(&slot.name) {
                            let sb_rect = Rect::from_min_size(card_rect.min, vec2(22.0, card_rect.height()));
                            let sb_out = render_bitwig_device_sidebar(
                                ui,
                                sb_rect,
                                ("php_sb", track_idx, slot_idx),
                                "OPENPHASER+",
                                slot.active,
                                is_selected,
                            );
                            if sb_out.toggled_power {
                                let mut new_active = None;
                                if let Some(t) = app.current_tracks_mut().get_mut(track_idx) {
                                    if let Some(s) = t.effects.get_mut(slot_idx) {
                                        s.active = !s.active;
                                        new_active = Some(s.active);
                                    }
                                }
                                if let (Some(et_idx), Some(active)) = (track_idx.checked_sub(1), new_active) {
                                    app.audio_proxy.send(crate::audio_proxy::GuiCommand::SetExternalPluginActive {
                                        track_idx: et_idx,
                                        slot_idx,
                                        active,
                                    });
                                }
                            }
                            if sb_out.clicked_title {
                                app.selected_slot_index = slot_idx;
                                app.matrix_state.selected_scene = None;
                            }
                            let preset_popup_id = ui.id().with(("php_preset_popup", track_idx, slot_idx));
                            if sb_out.clicked_folder {
                                ui.memory_mut(|m| m.toggle_popup(preset_popup_id));
                            }
                            egui::popup_below_widget(
                                ui,
                                preset_popup_id,
                                &sb_out.folder_resp,
                                egui::PopupCloseBehavior::CloseOnClickOutside,
                                |ui| {
                                    hikaru_openphaser_plus::render_preset_menu(ui, app, track_idx, slot_idx);
                                },
                            );

                            let body_rect = Rect::from_min_max(
                                pos2(sb_rect.max.x + 2.0, card_rect.min.y + 2.0),
                                pos2(card_rect.max.x - 2.0, card_rect.max.y - 2.0),
                            );
                            let mut body_ui = ui.new_child(
                                egui::UiBuilder::new()
                                    .max_rect(body_rect)
                                    .layout(egui::Layout::top_down(egui::Align::Min)),
                            );
                            body_ui.set_clip_rect(body_rect.intersect(ui.clip_rect()));
                            hikaru_openphaser_plus::render_module(&mut body_ui, app, track_idx, slot_idx);

                            let close_r = Rect::from_min_size(
                                pos2(card_rect.max.x - 14.0, card_rect.min.y + 2.0),
                                vec2(12.0, 12.0),
                            );
                            let close_resp = ui.interact(
                                close_r,
                                ui.id().with(("dsp_close", slot_idx)),
                                Sense::click(),
                            );
                            icons::paint_icon(
                                ui.painter(),
                                close_r.shrink(2.0),
                                icons::X_CLOSE,
                                if close_resp.hovered() {
                                    Color32::WHITE
                                } else {
                                    Color32::from_rgba_unmultiplied(0x8C, 0x90, 0x9A, 150)
                                },
                                1.4,
                            );
                            if close_resp.on_hover_text("Remove Device (Del)").clicked() {
                                slot_to_remove = Some(slot_idx);
                            }
                        } else {
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
                                    app.matrix_state.selected_scene = None;
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
                                "Sampler" | "Hikaru Sampler" => {
                                    sampler_editor::render_module(&mut body_ui, app, track_idx, slot_idx);
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
                                name if hikaru_opencomb::is_opencomb_name(name) => {
                                    let sb_rect = Rect::from_min_size(card_rect.min, vec2(22.0, card_rect.height()));
                                    let sb_out = render_bitwig_device_sidebar(
                                        ui,
                                        sb_rect,
                                        ("oc_sb", track_idx, slot_idx),
                                        "OPENCOMB",
                                        slot.active,
                                        is_selected,
                                    );
                                    if sb_out.toggled_power {
                                        let mut new_active = None;
                                        if let Some(t) = app.current_tracks_mut().get_mut(track_idx) {
                                            if let Some(s) = t.effects.get_mut(slot_idx) {
                                                s.active = !s.active;
                                                new_active = Some(s.active);
                                            }
                                        }
                                        if let (Some(et_idx), Some(active)) = (track_idx.checked_sub(1), new_active) {
                                            app.audio_proxy.send(crate::audio_proxy::GuiCommand::SetExternalPluginActive {
                                                track_idx: et_idx,
                                                slot_idx,
                                                active,
                                            });
                                        }
                                    }
                                    if sb_out.clicked_title {
                                        app.selected_slot_index = slot_idx;
                                        app.matrix_state.selected_scene = None;
                                    }
                                    let preset_popup_id = ui.id().with(("oc_preset_popup", track_idx, slot_idx));
                                    if sb_out.clicked_folder {
                                        ui.memory_mut(|m| m.toggle_popup(preset_popup_id));
                                    }
                                    egui::popup_below_widget(
                                        ui,
                                        preset_popup_id,
                                        &sb_out.folder_resp,
                                        egui::PopupCloseBehavior::CloseOnClickOutside,
                                        |ui| {
                                            hikaru_opencomb::render_preset_menu(ui, app, track_idx, slot_idx);
                                        },
                                    );

                                    let oc_body_rect = Rect::from_min_max(
                                        pos2(sb_rect.max.x + 2.0, card_rect.min.y + 2.0),
                                        pos2(card_rect.max.x - 2.0, card_rect.max.y - 2.0),
                                    );
                                    let mut oc_body_ui = ui.new_child(
                                        egui::UiBuilder::new()
                                            .max_rect(oc_body_rect)
                                            .layout(egui::Layout::top_down(egui::Align::Min)),
                                    );
                                    oc_body_ui.set_clip_rect(oc_body_rect.intersect(ui.clip_rect()));
                                    hikaru_opencomb::render_module(&mut oc_body_ui, app, track_idx, slot_idx);
                                }
                                name if crate::views::hikaru_native_plugins::hikaru_tier_s_fx::is_tier_s_fx(name) => {
                                    crate::views::hikaru_native_plugins::hikaru_tier_s_fx::render_module(
                                        &mut body_ui,
                                        app,
                                        track_idx,
                                        slot_idx,
                                        name,
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

                        // Cadena anidada Wet FX (Hikaru Open8-Bit y Hikaru OpenFreqShifter+)
                        if (hikaru_bit8::is_bit8_name(&slot.name)
                            && hikaru_bit8::is_wet_fx_open(track_idx, slot_idx))
                            || (hikaru_freq_shifter_plus::is_freq_shifter_plus_name(&slot.name)
                                && hikaru_freq_shifter_plus::is_wet_fx_open(track_idx, slot_idx))
                        {
                            render_bit8_wet_fx_chain(ui, app, track_idx, slot_idx, card_h);
                        }

                        // Cadena FX anidada del pad seleccionado en Hikaru OpenDMS (ej. Hikaru Sampler)
                        if slot.name == "Hikaru OpenDMS" {
                            let pad_fx_info = app
                                .current_tracks()
                                .get(track_idx)
                                .and_then(|t| t.effects.get(slot_idx))
                                .and_then(|s| s.dms_state.as_ref())
                                .filter(|dms| dms.fx_open)
                                .map(|dms| {
                                    let sel = dms.selected_pad.min(dms.pads.len().saturating_sub(1));
                                    let note_lbl = dms
                                        .pads
                                        .get(sel)
                                        .map(|p| {
                                            crate::views::hikaru_native_plugins::midi_note_name(
                                                p.midi_note,
                                            )
                                        })
                                        .unwrap_or_else(|| "Pad".to_string());
                                    let smp = dms.pads.get(sel).and_then(|p| p.sampler.clone());
                                    (sel, note_lbl, smp)
                                });

                            if let Some((sel_pad, note_lbl, maybe_smp)) = pad_fx_info {
                                // Ranura `+` antes del dispositivo del pad (estilo Bitwig)
                                render_dms_fx_plus_slot(
                                    ui,
                                    app,
                                    track_idx,
                                    slot_idx,
                                    sel_pad,
                                    &note_lbl,
                                    card_h,
                                    "before",
                                );

                                if let Some(mut pad_smp) = maybe_smp {
                                    // Corchete superior azul estilo Bitwig (`┍━━━━━━━━━━┑`) para dispositivos dentro de FX
                                    let fx_bracket_color = Color32::from_rgb(0x40, 0x6A, 0x80);
                                    let (nested_outer_rect, _) = ui.allocate_exact_size(
                                        vec2(SAMPLER_CARD_WIDTH + 24.0 + 18.0, card_h),
                                        Sense::hover(),
                                    );
                                    {
                                        let np = ui.painter();
                                        np.rect_filled(
                                            nested_outer_rect,
                                            2.5,
                                            Color32::from_rgb(0x18, 0x1A, 0x1E),
                                        );

                                        let bar_h = 4.5_f32;
                                        let foot_w = 8.0_f32;
                                        let foot_h = 9.0_f32;
                                        // Barra horizontal superior
                                        np.rect_filled(
                                            Rect::from_min_max(
                                                nested_outer_rect.min,
                                                pos2(
                                                    nested_outer_rect.max.x,
                                                    nested_outer_rect.min.y + bar_h,
                                                ),
                                            ),
                                            1.0,
                                            fx_bracket_color,
                                        );
                                        // Pie izquierdo y pie derecho hacia abajo
                                        np.rect_filled(
                                            Rect::from_min_size(
                                                nested_outer_rect.min,
                                                vec2(foot_w, foot_h),
                                            ),
                                            1.0,
                                            fx_bracket_color,
                                        );
                                        np.rect_filled(
                                            Rect::from_min_size(
                                                pos2(
                                                    nested_outer_rect.max.x - foot_w,
                                                    nested_outer_rect.min.y,
                                                ),
                                                vec2(foot_w, foot_h),
                                            ),
                                            1.0,
                                            fx_bracket_color,
                                        );
                                    }

                                    let smp_card_rect = Rect::from_min_max(
                                        pos2(
                                            nested_outer_rect.min.x + 9.0,
                                            nested_outer_rect.min.y + 5.0,
                                        ),
                                        pos2(
                                            nested_outer_rect.max.x - 9.0,
                                            nested_outer_rect.max.y - 5.0,
                                        ),
                                    );
                                    let sp = ui.painter();
                                    sp.rect_filled(
                                        smp_card_rect,
                                        3.5,
                                        Color32::from_rgb(0x2E, 0x30, 0x35),
                                    );
                                    sp.rect_stroke(
                                        smp_card_rect,
                                        3.5,
                                        Stroke::new(1.0_f32, Color32::from_rgb(0x46, 0x49, 0x52)),
                                        egui::StrokeKind::Inside,
                                    );

                                    let smp_sb_rect = Rect::from_min_size(
                                        smp_card_rect.min,
                                        vec2(22.0, smp_card_rect.height()),
                                    );
                                    let smp_title = if pad_smp.sample_name.is_empty() {
                                        format!("Sampler ({note_lbl})")
                                    } else {
                                        pad_smp.sample_name.clone()
                                    };
                                    let smp_sb = render_bitwig_device_sidebar(
                                        ui,
                                        smp_sb_rect,
                                        ("dms_pad_smp_sb", slot_idx * 256 + sel_pad),
                                        &smp_title,
                                        true,
                                        true,
                                    );
                                    if smp_sb.clicked_folder {
                                        if let Some(path) = rfd::FileDialog::new()
                                            .add_filter("Audio WAV", &["wav", "WAV"])
                                            .pick_file()
                                        {
                                            let _ = pad_smp.load_wav(&path);
                                        }
                                    }

                                    let close_r = Rect::from_min_size(
                                        pos2(smp_card_rect.max.x - 16.0, smp_card_rect.min.y + 3.0),
                                        vec2(13.0, 13.0),
                                    );
                                    let close_resp = ui.interact(
                                        close_r,
                                        ui.id().with(("dms_pad_smp_close", slot_idx, sel_pad)),
                                        Sense::click(),
                                    );
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
                                    let remove_pad_smp = close_resp
                                        .on_hover_text("Quitar Sampler del pad")
                                        .clicked();

                                    let s_body = Rect::from_min_max(
                                        pos2(smp_sb_rect.max.x + 4.0, smp_card_rect.min.y + 4.0),
                                        pos2(smp_card_rect.max.x - 4.0, smp_card_rect.max.y - 4.0),
                                    );
                                    let mut s_body_ui = ui.new_child(
                                        egui::UiBuilder::new()
                                            .max_rect(s_body)
                                            .layout(egui::Layout::top_down(egui::Align::Min)),
                                    );
                                    s_body_ui.set_clip_rect(s_body.intersect(ui.clip_rect()));
                                    sampler_editor::render_sampler_state(
                                        &mut s_body_ui,
                                        &mut pad_smp,
                                        slot_idx * 256 + sel_pad + 1000,
                                    );

                                    if let Some(track) = app.current_tracks_mut().get_mut(track_idx) {
                                        if let Some(s) = track.effects.get_mut(slot_idx) {
                                            if let Some(ref mut dms) = s.dms_state {
                                                if let Some(pad) = dms.pads.get_mut(sel_pad) {
                                                    pad.sampler = if remove_pad_smp {
                                                        None
                                                    } else {
                                                        Some(pad_smp)
                                                    };
                                                }
                                            }
                                        }
                                    }

                                    // Ranura `+` después del Sampler (estilo Bitwig)
                                    render_dms_fx_plus_slot(
                                        ui,
                                        app,
                                        track_idx,
                                        slot_idx,
                                        sel_pad,
                                        &note_lbl,
                                        card_h,
                                        "after",
                                    );
                                }
                            }
                        }
                    }

                    if let Some(idx) = slot_to_remove {
                        remove_device_slot(app, track_idx, idx);
                    }

                    // Botón `+` centrado verticalmente estilo ranura de inserción Bitwig
                    ui.vertical(|ui| {
                        ui.add_space((pill_h * 0.5 - 14.0).max(40.0));
                        let plus_btn = ui.button(
                            RichText::new("+")
                                .font(fonts::semibold(15.0))
                                .color(theme::TEXT_PRIMARY),
                        );
                        if plus_btn
                            .clone()
                            .on_hover_text(format!("Insert Device on {track_name} (Right-click for .hdp Presets)"))
                            .clicked()
                        {
                            app.popup_browser.open_insert_device(track_idx);
                        }
                        plus_btn.context_menu(|ui| {
                            ui.label(
                                RichText::new(format!("Presets on {track_name}"))
                                    .font(fonts::semibold(11.0))
                                    .color(theme::ACCENT_ORANGE),
                            );
                            ui.separator();
                            if ui.button("📂 Cargar Preset (.hdp / .bwpreset / .bwdevice)...").clicked() {
                                load_device_chain_dialog(app, track_idx);
                                ui.close_menu();
                            }
                            if ui.button("💾 Guardar Cadena de Pista (*.hdp)...").clicked() {
                                save_device_chain_dialog(app, track_idx);
                                ui.close_menu();
                            }
                            if ui.button("💾 Exportar Dispositivo (.bwpreset)...").clicked() {
                                let slot_idx = app.selected_slot_index;
                                export_device_bwpreset_dialog(app, track_idx, slot_idx);
                                ui.close_menu();
                            }
                        });
                    });
                });
            });
}
