/*
 * Hikaru OpenGrid - Patch State, Camera & Persistence (GNU AGPL v3)
 * Copyright (C) 2026 Hikaru Corporation
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 */

use hikaru_dsp::synth::open_grid::{
    GridCable, GridCategory, GridModuleKind, GridNode, OpenGridPatch,
};
use hikaru_project_file::{OpenGridCableData, OpenGridNodeData, OpenGridParams};

#[derive(Debug, Clone, Copy)]
pub struct OpenGridFactoryPreset {
    pub name: &'static str,
    pub category: &'static str,
}

pub const GRID_FACTORY_PRESETS: [OpenGridFactoryPreset; 4] = [
    OpenGridFactoryPreset { name: "Init Poly Grid", category: "Init" },
    OpenGridFactoryPreset { name: "Subtractive MG", category: "Synth" },
    OpenGridFactoryPreset { name: "Swarm Chorus Pad", category: "Pad" },
    OpenGridFactoryPreset { name: "Acid Wavefolder", category: "Acid" },
];

#[derive(Clone, Debug)]
pub struct HikaruOpenGrid {
    pub patch: OpenGridPatch,
    pub preset_name: String,

    // Editor camera state (world coordinates)
    pub pan: [f32; 2],
    pub zoom: f32,
    pub last_canvas_size: [f32; 2],

    // Editor UI state
    pub modulators_open: bool,
    pub selected_category: GridCategory,
    pub selected_node: Option<u32>,
    /// Active cable being dragged from `(from_node_id, from_port_idx)`.
    pub dragging_cable: Option<(u32, usize)>,

    // Incremental audio sync revision
    pub patch_rev: u64,
}

impl Default for HikaruOpenGrid {
    fn default() -> Self {
        Self {
            patch: OpenGridPatch::default_poly_grid(),
            preset_name: "Init Poly Grid".to_string(),
            pan: [110.0, 40.0],
            zoom: 1.0,
            last_canvas_size: [960.0, 320.0],
            modulators_open: true,
            selected_category: GridCategory::Io,
            selected_node: None,
            dragging_cable: None,
            patch_rev: 1,
        }
    }
}

impl HikaruOpenGrid {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mark_dirty(&mut self) {
        self.patch_rev = self.patch_rev.wrapping_add(1);
    }

    pub fn apply_factory_preset(&mut self, name: &str) {
        self.patch = match name {
            "Subtractive MG" => OpenGridPatch::preset_subtractive_mg(),
            "Swarm Chorus Pad" => OpenGridPatch::preset_swarm_chorus(),
            "Acid Wavefolder" => OpenGridPatch::preset_acid_wavefolder(),
            _ => OpenGridPatch::default_poly_grid(),
        };
        self.preset_name = name.to_string();
        self.selected_node = None;
        self.dragging_cable = None;
        self.fit_view();
        self.mark_dirty();
    }

    /// Computes bounding box `(min_x, min_y, max_x, max_y)` of all nodes in world space.
    pub fn content_bounds(&self) -> (f32, f32, f32, f32) {
        if self.patch.nodes.is_empty() {
            return (0.0, 0.0, 800.0, 360.0);
        }
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for n in &self.patch.nodes {
            let [w, h] = n.kind.default_size();
            min_x = min_x.min(n.pos[0]);
            min_y = min_y.min(n.pos[1]);
            max_x = max_x.max(n.pos[0] + w);
            max_y = max_y.max(n.pos[1] + h);
        }
        (min_x - 60.0, min_y - 50.0, max_x + 60.0, max_y + 50.0)
    }

    /// Centers camera on the current patch.
    pub fn fit_view(&mut self) {
        let (min_x, min_y, max_x, max_y) = self.content_bounds();
        let cx = (min_x + max_x) * 0.5;
        let cy = (min_y + max_y) * 0.5;
        let vw = self.last_canvas_size[0].max(420.0);
        let vh = self.last_canvas_size[1].max(220.0);
        let bw = (max_x - min_x).max(200.0);
        let bh = (max_y - min_y).max(160.0);
        let fit_z = ((vw * 0.88) / bw).min((vh * 0.86) / bh).clamp(0.55, 1.15);
        self.zoom = fit_z;
        self.pan = [cx - vw / (2.0 * fit_z), cy - vh / (2.0 * fit_z)];
    }

    /// Spawns a new module near the center of the current viewport, nudging to avoid exact overlap.
    pub fn add_module(&mut self, kind: GridModuleKind) -> u32 {
        let id = self.patch.next_node_id();
        let vw = self.last_canvas_size[0].max(420.0) / self.zoom.max(0.25);
        let vh = self.last_canvas_size[1].max(220.0) / self.zoom.max(0.25);
        let [w, h] = kind.default_size();
        let mut x = ((self.pan[0] + vw * 0.5 - w * 0.5) / 12.0).round() * 12.0;
        let mut y = ((self.pan[1] + vh * 0.5 - h * 0.5) / 12.0).round() * 12.0;

        for _ in 0..16 {
            let collides = self
                .patch
                .nodes
                .iter()
                .any(|n| (n.pos[0] - x).abs() < 28.0 && (n.pos[1] - y).abs() < 28.0);
            if !collides {
                break;
            }
            x += 36.0;
            y += 24.0;
        }

        self.patch.nodes.push(GridNode::new(id, kind, [x, y]));
        self.selected_node = Some(id);
        self.mark_dirty();
        id
    }

    pub fn remove_node(&mut self, node_id: u32) {
        self.patch.nodes.retain(|n| n.id != node_id);
        self.patch
            .cables
            .retain(|c| c.from_node != node_id && c.to_node != node_id);
        if self.selected_node == Some(node_id) {
            self.selected_node = None;
        }
        if self.dragging_cable.map(|(n, _)| n) == Some(node_id) {
            self.dragging_cable = None;
        }
        self.mark_dirty();
    }

    pub fn duplicate_node(&mut self, node_id: u32) -> Option<u32> {
        let src = self.patch.nodes.iter().find(|n| n.id == node_id)?.clone();
        let new_id = self.patch.next_node_id();
        let mut copy = src;
        copy.id = new_id;
        copy.pos = [copy.pos[0] + 36.0, copy.pos[1] + 36.0];
        self.patch.nodes.push(copy);
        self.selected_node = Some(new_id);
        self.mark_dirty();
        Some(new_id)
    }

    pub fn connect_cable(&mut self, from_node: u32, from_port: usize, to_node: u32, to_port: usize) {
        if from_node == to_node {
            return;
        }
        let cable = GridCable {
            from_node,
            from_port,
            to_node,
            to_port,
        };
        if !self.patch.cables.contains(&cable) {
            self.patch.cables.push(cable);
            self.mark_dirty();
        }
    }

    pub fn disconnect_input(&mut self, node_id: u32, port_idx: usize) {
        let before = self.patch.cables.len();
        self.patch
            .cables
            .retain(|c| !(c.to_node == node_id && c.to_port == port_idx));
        if self.patch.cables.len() != before {
            self.mark_dirty();
        }
    }

    pub fn disconnect_output(&mut self, node_id: u32, port_idx: usize) {
        let before = self.patch.cables.len();
        self.patch
            .cables
            .retain(|c| !(c.from_node == node_id && c.from_port == port_idx));
        if self.patch.cables.len() != before {
            self.mark_dirty();
        }
    }

    pub fn to_project_params(&self) -> OpenGridParams {
        OpenGridParams {
            preset_name: self.preset_name.clone(),
            nodes: self
                .patch
                .nodes
                .iter()
                .map(|n| OpenGridNodeData {
                    id: n.id,
                    kind: n.kind.display_name().to_string(),
                    pos: n.pos,
                    params: n.params,
                    bypassed: n.bypassed,
                })
                .collect(),
            cables: self
                .patch
                .cables
                .iter()
                .map(|c| OpenGridCableData {
                    from_node: c.from_node,
                    from_port: c.from_port,
                    to_node: c.to_node,
                    to_port: c.to_port,
                })
                .collect(),
            global_pitch_st: self.patch.global_pitch_st,
            glide_ms: self.patch.glide_ms,
            legato: self.patch.legato,
            mono_mode: self.patch.mono_mode,
            output_gain: self.patch.output_gain,
            lfo_enabled: self.patch.lfo_enabled,
            lfo_rate_hz: self.patch.lfo_rate_hz,
            lfo_depth: self.patch.lfo_depth,
            modulators_open: self.modulators_open,
            pan: self.pan,
            zoom: self.zoom,
        }
    }

    pub fn apply_project_params(&mut self, p: &OpenGridParams) {
        if !p.preset_name.is_empty() {
            self.preset_name = p.preset_name.clone();
        }
        if !p.nodes.is_empty() {
            self.patch.nodes = p
                .nodes
                .iter()
                .filter_map(|nd| {
                    let kind = GridModuleKind::from_name(&nd.kind)?;
                    Some(GridNode {
                        id: nd.id,
                        kind,
                        pos: nd.pos,
                        params: nd.params,
                        bypassed: nd.bypassed,
                    })
                })
                .collect();
            self.patch.cables = p
                .cables
                .iter()
                .map(|c| GridCable {
                    from_node: c.from_node,
                    from_port: c.from_port,
                    to_node: c.to_node,
                    to_port: c.to_port,
                })
                .collect();
        }
        self.patch.global_pitch_st = p.global_pitch_st.clamp(-24.0, 24.0);
        self.patch.glide_ms = p.glide_ms.clamp(0.0, 2000.0);
        self.patch.legato = p.legato;
        self.patch.mono_mode = p.mono_mode;
        self.patch.output_gain = p.output_gain.clamp(0.0, 2.0);
        self.patch.lfo_enabled = p.lfo_enabled;
        self.patch.lfo_rate_hz = p.lfo_rate_hz.clamp(0.05, 25.0);
        self.patch.lfo_depth = p.lfo_depth.clamp(0.0, 1.0);
        self.modulators_open = p.modulators_open;
        self.pan = p.pan;
        self.zoom = p.zoom.clamp(0.4, 2.2);
        self.mark_dirty();
    }
}

pub fn save_opengrid_preset(grid: &HikaruOpenGrid) {
    let default_name = format!(
        "{}.hdp",
        grid.preset_name
            .to_lowercase()
            .replace(' ', "_")
            .replace(|c: char| !c.is_alphanumeric() && c != '_', "")
    );
    if let Some(path) = rfd::FileDialog::new()
        .set_file_name(&default_name)
        .add_filter(
            hikaru_project_file::HDP_FILE_FILTER,
            &[hikaru_project_file::HDP_EXTENSION],
        )
        .save_file()
    {
        let slot = hikaru_project_file::DspSlot {
            id: 0,
            name: "OpenGrid".to_string(),
            active: true,
            wavetable_path: None,
            plugin_path: None,
            plugin_format: None,
            open_wavetable: None,
            open_modulation: None,
            sampler: None,
            open_poly: None,
            open_grid: Some(grid.to_project_params()),
        };
        let preset = hikaru_project_file::HikaruDevicePreset {
            format_version: hikaru_project_file::FORMAT_VERSION,
            created_by: format!("Hikaru OpenLive {}", crate::version::VERSION),
            preset_type: hikaru_project_file::PresetType::SingleDevice,
            name: grid.preset_name.clone(),
            author: None,
            category: Some("Modular Synth".to_string()),
            description: Some("Hikaru OpenGrid patch preset (AGPL-v3)".to_string()),
            slot: Some(slot),
            chain: None,
        };
        let _ = hikaru_project_file::save_hdp(&path, &preset);
    }
}

pub fn load_opengrid_preset(grid: &mut HikaruOpenGrid) {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter(
            hikaru_project_file::HDP_FILE_FILTER,
            &[hikaru_project_file::HDP_EXTENSION],
        )
        .pick_file()
    {
        if let Ok(preset) = hikaru_project_file::parse_hdp(&path) {
            if let Some(slot) = preset.slot {
                if let Some(ref p) = slot.open_grid {
                    grid.apply_project_params(p);
                }
                if !preset.name.is_empty() {
                    grid.preset_name = preset.name;
                }
            }
        }
    }
}
