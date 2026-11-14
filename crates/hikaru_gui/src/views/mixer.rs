// Hikaru OpenLive / OpenStudio - Mixer model + Channel Strip kit (egui)
// GNU AGPLv3
// crates/hikaru_gui/src/views/mixer.rs

use egui::{pos2, vec2, Color32, Rect, RichText, ScrollArea, Sense, Stroke, Ui};

use crate::app::{AppState, PanMode};
use crate::audio_proxy::GuiCommand;
use crate::theme::*;
use crate::views::arranger_view::{self, StripTarget};
use crate::views::hikaru_native_plugins::hikaru_opendms::OpenDms;
use crate::views::hikaru_native_plugins::hikaru_openwavetable::WavetableEditor;

pub const BOTTOM_MIXER_TRACK_WIDTH: f32 = 85.0;
pub const BOTTOM_MIXER_SIDEBAR_WIDTH: f32 = 40.0;
pub const BOTTOM_MIXER_PINNED_WIDTH: f32 = 172.0;

#[derive(Clone, Debug)]
pub struct MixerDeviceDrag {
    pub source_target: StripTarget,
    pub source_slot: usize,
    pub device_name: String,
}

#[derive(Clone, Debug)]
pub struct MixerDeviceChipData {
    pub slot: usize,
    pub name: String,
    pub active: bool,
    pub has_gui: bool,
}

pub const EMPTY_SLOT_NAME: &str = "Empty Slot";

#[derive(Clone, Debug)]
pub struct ExternalParamState {
    pub info: hikaru_plugin_host::PluginParamInfo,
    pub value: f32,
    pub pending_to_engine: bool,
}

impl ExternalParamState {
    pub fn display(&self) -> String {
        self.info.display_value(self.value)
    }
}

#[derive(Clone, Debug, Default)]
pub enum ExternalParamsStatus {
    #[default]
    Idle,
    Probing,
    Ready,
    Error(String),
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct DspSlot {
    pub id: usize,
    pub name: String,
    pub active: bool,
    pub is_open: bool,
    pub wavetable: WavetableEditor,
    pub dms_state: Option<OpenDms>,
    pub sampler_state: Option<crate::views::hikaru_native_plugins::HikaruSampler>,
    pub menu_open: bool,
    pub options_open: bool,
    pub external: Option<hikaru_plugin_host::PluginDescriptor>,
    pub load_error: Option<String>,
    pub cached_params: Vec<ExternalParamState>,
    pending_external_params: Vec<(u32, f32)>,
    pub params_status: ExternalParamsStatus,
    pub has_gui: Option<bool>,
}

impl Default for DspSlot {
    fn default() -> Self {
        Self::new(0, EMPTY_SLOT_NAME.to_string())
    }
}

impl DspSlot {
    pub fn new(id: usize, name: String) -> Self {
        Self {
            id,
            name,
            active: true,
            is_open: false,
            wavetable: WavetableEditor::new(),
            dms_state: None,
            sampler_state: None,
            menu_open: false,
            options_open: false,
            external: None,
            load_error: None,
            cached_params: Vec::new(),
            pending_external_params: Vec::new(),
            params_status: ExternalParamsStatus::Idle,
            has_gui: None,
        }
    }

    pub fn has_editor(&self) -> bool {
        if self.external.is_some() {
            return self.external.as_ref().is_some_and(|p| p.has_gui);
        }
        matches!(self.name.as_str(), "OpenWavetable" | "Hikaru OpenDMS" | "Sampler" | "Hikaru Sampler")
    }

    pub fn is_empty_slot(&self) -> bool {
        self.external.is_none() && self.name == EMPTY_SLOT_NAME
    }

    pub fn take_pending_external_params(&mut self) -> Vec<(u32, f32)> {
        std::mem::take(&mut self.pending_external_params)
    }

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

    pub fn set_external_param(&mut self, id: u32, value: f32) -> Option<f32> {
        let param = self.cached_params.iter_mut().find(|p| p.info.id == id)?;
        let snapped = param.info.snap(value);
        if (param.value - snapped).abs() <= f32::EPSILON {
            return None;
        }
        param.value = snapped;
        self.enqueue_external_param(id, snapped);
        Some(snapped)
    }
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
    pub color: Color32,
    pub volume: f32,
    pub pan: f32,
    pub pan_mode: PanMode,
    pub mute: bool,
    pub solo: bool,
    pub arm: bool,
    pub is_master: bool,
    pub is_group: bool,
    pub parent_group_id: Option<usize>,
    pub is_expanded: bool,
    pub route_destination_id: usize,
    pub sends: Vec<SendConnection>,
    pub effects: Vec<DspSlot>,
    pub matrix_idx: Option<usize>,
    pub width: f32,
}

impl Track {
    pub fn new(id: usize, name: String, is_master: bool) -> Self {
        let palette = [
            Color32::from_rgb(0xFF, 0x55, 0x00), // Bitwig Orange (Track 1)
            Color32::from_rgb(0xE5, 0xAE, 0x19), // Bitwig Yellow (Track 2)
            Color32::from_rgb(0x3A, 0xA8, 0xE0), // Bitwig Sky Blue (Track 3)
            Color32::from_rgb(0x28, 0xD9, 0x59), // Bitwig Emerald (Track 4)
            Color32::from_rgb(0xB8, 0x6B, 0xF5), // Bitwig Violet (Track 5)
            Color32::from_rgb(0xF2, 0x44, 0x3D), // Bitwig Coral Red (Track 6)
            Color32::from_rgb(0x00, 0xC7, 0xE6), // Bitwig Cyan (Track 7)
            Color32::from_rgb(0xFF, 0x9E, 0x1B), // Bitwig Amber (Track 8)
        ];
        let color = if is_master {
            Color32::from_rgb(0x82, 0x84, 0x88)
        } else {
            palette[id.saturating_sub(1) % palette.len()]
        };
        Self {
            id,
            name,
            color,
            volume: 0.75,
            pan: 0.0,
            pan_mode: PanMode::Stereo,
            mute: false,
            solo: false,
            arm: false,
            is_master,
            is_group: false,
            parent_group_id: None,
            is_expanded: true,
            route_destination_id: 0,
            sends: Vec::new(),
            effects: Vec::new(),
            matrix_idx: None,
            width: arranger_view::DEFAULT_TRACK_WIDTH,
        }
    }

    pub fn new_group(id: usize, name: String) -> Self {
        let mut tr = Self::new(id, name, false);
        tr.is_group = true;
        tr.color = Color32::from_rgb(0xE5, 0xAE, 0x19); // Bitwig Group Yellow
        tr
    }
}

/// Returns `true` if the track at `idx` is visible (all ancestor groups are expanded).
pub fn is_track_visible(tracks: &[Track], idx: usize) -> bool {
    let mut cur = tracks.get(idx).and_then(|t| t.parent_group_id);
    let mut guard = 0;
    while let Some(gid) = cur {
        guard += 1;
        if guard > 32 {
            break;
        }
        if let Some(parent) = tracks.iter().find(|t| t.id == gid) {
            if !parent.is_expanded {
                return false;
            }
            cur = parent.parent_group_id;
        } else {
            break;
        }
    }
    true
}

/// Returns the nesting depth of `tracks[idx]` (0 for top-level tracks).
pub fn track_group_depth(tracks: &[Track], idx: usize) -> usize {
    let mut depth = 0;
    let mut cur = tracks.get(idx).and_then(|t| t.parent_group_id);
    while let Some(gid) = cur {
        depth += 1;
        if depth > 32 {
            break;
        }
        cur = tracks.iter().find(|t| t.id == gid).and_then(|p| p.parent_group_id);
    }
    depth
}

/// Returns indices of direct children of `group_id`.
pub fn group_children_indices(tracks: &[Track], group_id: usize) -> Vec<usize> {
    tracks
        .iter()
        .enumerate()
        .filter_map(|(i, t)| (t.parent_group_id == Some(group_id)).then_some(i))
        .collect()
}

pub fn duplicate_device_chain(slots: &[DspSlot]) -> Vec<DspSlot> {
    slots.to_vec()
}

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

pub fn level_db_text(level: f32) -> String {
    let db_val = if level <= 0.0 {
        -60.0
    } else {
        (20.0 * level.log10()).clamp(-60.0, 6.0)
    };
    format!("{:.1}dB", db_val)
}

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

pub const VU_CLIP_RED: u32 = 0xFF3B30;
pub const VU_PEAK_AMBER: u32 = 0xFF9E2C;
pub const VU_WIDTH: f32 = 14.0;
pub const VU_HEIGHT: f32 = 84.0;

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

pub fn vertical_vu_meter(ui: &mut Ui, level: f32, width: f32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let painter = ui.painter_at(rect);

    let fw = rect.width();
    let fh = rect.height();
    let ox = rect.left();
    let oy = rect.top();

    painter.rect_filled(rect, 0.0, Color32::from_rgb(0x0A, 0x0A, 0x0B));

    let bar_w = ((fw - 1.0) / 2.0).max(1.0);
    painter.line_segment([pos2(ox + bar_w, oy), pos2(ox + bar_w, oy + fh)], Stroke::new(1.0_f32, Color32::from_rgb(0x1E, 0x20, 0x24)));

    let unity_y = oy + fh * (1.0 - 0.75);
    painter.line_segment([pos2(ox, unity_y), pos2(ox + fw, unity_y)], Stroke::new(1.0_f32, Color32::from_rgb(0x2A, 0x2D, 0x33)));

    if level > 0.001 {
        let levels = [level, (level * 0.97).min(1.0)];
        let zones: [(f32, f32, Color32); 3] = [
            (0.0, 0.75, Color32::from_rgb(0x2C, 0xE6, 0x4B)),
            (0.75, 0.90, Color32::from_rgb(0xC8, 0xE6, 0x2C)),
            (0.90, 1.0, Color32::from_rgb(0xFF, 0x3B, 0x30)),
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
                painter.rect_filled(
                    Rect::from_min_max(pos2(bx, y0), pos2(bx + bar_w, y1)),
                    0.0,
                    col,
                );
            }
            let peak_y = (oy + fh * (1.0 - lv)).clamp(oy, oy + fh - 1.5);
            painter.rect_filled(
                Rect::from_min_max(pos2(bx, peak_y), pos2(bx + bar_w, peak_y + 1.5)),
                0.0,
                Color32::from_rgb(0xFF, 0x9E, 0x2C),
            );
        }
    }
}

pub const ROUTE_FX_BASE: usize = 1000;

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

pub fn route_options(app: &AppState, target: StripTarget) -> Vec<(usize, String)> {
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

pub fn render_bottom_panel(ui: &mut Ui, state: &mut AppState) {
    use crate::ui::bitwig_kit::{
        capsule_icon_button, capsule_text_button, fader_horizontal_range,
        fader_vertical_track_vol, stereo_meter_bar, JoinSide,
    };
    use crate::ui::icons;

    let tracks_count = state.tracks().len();
    let mut selected_track_idx = state.selected_track_index;

    ScrollArea::horizontal().auto_shrink([false; 2]).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;

            for idx in 0..tracks_count {
                let is_sel = selected_track_idx == idx;
                let is_master = idx == 0;

                let (name, track_color, mut vol, mut pan, mut mute, mut solo, mut arm, peak) = {
                    let tracks = state.tracks();
                    let t = &tracks[idx];
                    let peak = if is_master {
                        state.smoothed_master_peak
                    } else {
                        state
                            .smoothed_track_peaks
                            .get(idx.saturating_sub(1))
                            .copied()
                            .unwrap_or(0.0)
                    };
                    (
                        t.name.clone(),
                        t.color,
                        t.volume,
                        t.pan,
                        t.mute,
                        t.solo,
                        t.arm,
                        peak,
                    )
                };

                let bg_col = if is_sel { SELECTED_BG } else { SURFACE_BG };

                egui::Frame::NONE
                    .fill(bg_col)
                    .stroke(Stroke::new(1.0_f32, BORDER_COLOR))
                    .corner_radius(2.0)
                    .inner_margin(4.0)
                    .show(ui, |ui| {
                        ui.set_width(BOTTOM_MIXER_TRACK_WIDTH);

                        ui.vertical_centered(|ui| {
                            let (bar_r, _) =
                                ui.allocate_exact_size(vec2(BOTTOM_MIXER_TRACK_WIDTH - 4.0, 5.0), Sense::hover());
                            ui.painter().rect_filled(bar_r, 1.0, track_color);

                            // Track label
                            if ui
                                .selectable_label(is_sel, RichText::new(&name).size(11.0).strong())
                                .clicked()
                            {
                                selected_track_idx = idx;
                            }

                            // GPU [● | S | M] capsule group
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                if !is_master {
                                    if capsule_icon_button(
                                        ui,
                                        vec2(24.0, 18.0),
                                        arm,
                                        REC_RED,
                                        JoinSide::Left,
                                        |p, r, fg| icons::paint_record_circle(p, r, fg, true),
                                    )
                                    .clicked()
                                    {
                                        arm = !arm;
                                        state.tracks_mut()[idx].arm = arm;
                                    }
                                    if capsule_text_button(
                                        ui,
                                        vec2(24.0, 18.0),
                                        "S",
                                        9.5,
                                        solo,
                                        SOLO_YELLOW,
                                        JoinSide::Middle,
                                        false,
                                    )
                                    .clicked()
                                    {
                                        solo = !solo;
                                        state.tracks_mut()[idx].solo = solo;
                                    }
                                    if capsule_text_button(
                                        ui,
                                        vec2(24.0, 18.0),
                                        "M",
                                        9.5,
                                        mute,
                                        MUTE_BLUE,
                                        JoinSide::Right,
                                        false,
                                    )
                                    .clicked()
                                    {
                                        mute = !mute;
                                        state.tracks_mut()[idx].mute = mute;
                                    }
                                } else {
                                    if capsule_text_button(
                                        ui,
                                        vec2(34.0, 18.0),
                                        "S",
                                        9.5,
                                        solo,
                                        SOLO_YELLOW,
                                        JoinSide::Left,
                                        false,
                                    )
                                    .clicked()
                                    {
                                        solo = !solo;
                                        state.tracks_mut()[idx].solo = solo;
                                    }
                                    if capsule_text_button(
                                        ui,
                                        vec2(34.0, 18.0),
                                        "M",
                                        9.5,
                                        mute,
                                        MUTE_BLUE,
                                        JoinSide::Right,
                                        false,
                                    )
                                    .clicked()
                                    {
                                        mute = !mute;
                                        state.tracks_mut()[idx].mute = mute;
                                    }
                                }
                            });

                            // GPU Bipolar Pan Bar
                            if fader_horizontal_range(
                                ui,
                                vec2(BOTTOM_MIXER_TRACK_WIDTH - 6.0, 13.0),
                                &mut pan,
                                -1.0..=1.0,
                                0.0,
                                BW_FADER_AMBER,
                                BW_FADER_ORANGE,
                                true,
                            )
                            .1
                            {
                                state.tracks_mut()[idx].pan = pan;
                            }

                            // GPU Stereo Meter + GPU Vertical Fader
                            ui.horizontal(|ui| {
                                stereo_meter_bar(
                                    ui,
                                    vec2(VU_WIDTH, 110.0),
                                    [
                                        peak,
                                        (peak * 0.96).clamp(0.0, 1.0),
                                        (peak * 1.04).min(1.0),
                                        (peak * 1.01).min(1.0),
                                    ],
                                );
                                if fader_vertical_track_vol(
                                    ui,
                                    vec2(36.0, 110.0),
                                    &mut vol,
                                    0.75,
                                )
                                .1
                                {
                                    state.tracks_mut()[idx].volume = vol;
                                    state.audio_proxy.send(GuiCommand::SetTrackVolume {
                                        track_idx: idx,
                                        volume_db: vol,
                                    });
                                }
                            });

                            ui.label(RichText::new(db_text(vol)).size(9.0).color(BW_FADER_ORANGE));
                        });
                    });
            }
        });
    });

    state.selected_track_index = selected_track_idx;
}
