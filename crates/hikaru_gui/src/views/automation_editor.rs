// crates/hikaru_gui/src/views/automation_editor.rs
//
// Bitwig Studio-style Bottom Automation Editor Panel:
// - Left vertical CLIP / TRACK (<MASTER> / <TRACK NAME>) strip
// - Category & Parameter selector (`Mixer -> Volume / Pan / Sends`, `Transport -> Tempo`)
// - Beat-synced timeline with blue automation curve, translucent fill, draggable nodes,
//   live BPM / dB / Pan tooltip badge, and real-time playback evaluation (20.00..666.00 BPM).
// GNU AGPLv3

use std::collections::HashMap;

use egui::{pos2, vec2, Align2, Color32, Pos2, Rect, Sense, Stroke, Ui};

use crate::app::{AppState, BPM_MAX, BPM_MIN};
use crate::audio_proxy::GuiCommand;
use crate::theme;
use crate::ui::{fonts, icons};
use crate::views::mixer::{self, ROUTE_FX_BASE};

const PPQN: f64 = 960.0;
const AUTO_BLUE: Color32 = Color32::from_rgb(0x3A, 0xA8, 0xE0);
const AUTO_NODE_ACTIVE: Color32 = Color32::from_rgb(0x70, 0xCC, 0xFA);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AutomationTarget {
    TransportTempo,
    MixerVolume { track_idx: usize },
    MixerPan { track_idx: usize },
    TrackSend { track_idx: usize, fx_idx: usize },
}

impl AutomationTarget {
    pub fn category_name(&self) -> &'static str {
        match self {
            AutomationTarget::TransportTempo => "Transport",
            AutomationTarget::MixerVolume { .. }
            | AutomationTarget::MixerPan { .. }
            | AutomationTarget::TrackSend { .. } => "Mixer",
        }
    }

    pub fn param_name(&self, app: &AppState) -> String {
        match self {
            AutomationTarget::TransportTempo => "Tempo".to_string(),
            AutomationTarget::MixerVolume { .. } => "Volume".to_string(),
            AutomationTarget::MixerPan { .. } => "Pan".to_string(),
            AutomationTarget::TrackSend { fx_idx, .. } => app
                .fx_tracks
                .get(*fx_idx)
                .map(|f| format!("Send: {}", f.name))
                .unwrap_or_else(|| format!("Send {}", fx_idx + 1)),
        }
    }

    pub fn default_norm(&self, app: &AppState) -> f32 {
        match self {
            AutomationTarget::TransportTempo => bpm_to_norm(app.transport.bpm),
            AutomationTarget::MixerVolume { track_idx } => app
                .current_tracks()
                .get(*track_idx)
                .map(|t| t.volume.clamp(0.0, 1.0))
                .unwrap_or(0.75),
            AutomationTarget::MixerPan { track_idx } => app
                .current_tracks()
                .get(*track_idx)
                .map(|t| ((t.pan + 1.0) * 0.5).clamp(0.0, 1.0))
                .unwrap_or(0.5),
            AutomationTarget::TrackSend { track_idx, fx_idx } => app
                .current_tracks()
                .get(*track_idx)
                .map(|t| t.send_amount(ROUTE_FX_BASE + *fx_idx).clamp(0.0, 1.0))
                .unwrap_or(0.0),
        }
    }

    pub fn format_norm(&self, norm: f32) -> String {
        let n = norm.clamp(0.0, 1.0);
        match self {
            AutomationTarget::TransportTempo => format!("{:.2} BPM", norm_to_bpm(n)),
            AutomationTarget::MixerVolume { .. } => mixer::db_text(n),
            AutomationTarget::MixerPan { .. } => {
                let pan = n * 2.0 - 1.0;
                if pan.abs() < 0.01 {
                    "C".to_string()
                } else if pan < 0.0 {
                    format!("{:.0}% L", -pan * 100.0)
                } else {
                    format!("{:.0}% R", pan * 100.0)
                }
            }
            AutomationTarget::TrackSend { .. } => mixer::send_db_text(n),
        }
    }
}

pub fn bpm_to_norm(bpm: f64) -> f32 {
    (((bpm - BPM_MIN) / (BPM_MAX - BPM_MIN)).clamp(0.0, 1.0)) as f32
}

pub fn norm_to_bpm(norm: f32) -> f64 {
    let n = if norm.is_finite() {
        norm.clamp(0.0, 1.0) as f64
    } else {
        0.5
    };
    BPM_MIN + n * (BPM_MAX - BPM_MIN)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutomationPoint {
    pub tick: u64,
    pub value: f32,
}

#[derive(Clone, Debug)]
pub struct AutomationLane {
    pub target: AutomationTarget,
    pub points: Vec<AutomationPoint>,
}

impl AutomationLane {
    pub fn new(target: AutomationTarget) -> Self {
        Self {
            target,
            points: Vec::new(),
        }
    }

    pub fn sort_points(&mut self) {
        self.points.sort_by_key(|p| p.tick);
    }

    pub fn insert_point(&mut self, tick: u64, value: f32) -> usize {
        let v = if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.5
        };
        if let Some(idx) = self.points.iter().position(|p| p.tick == tick) {
            self.points[idx].value = v;
            return idx;
        }
        let idx = self
            .points
            .iter()
            .position(|p| p.tick > tick)
            .unwrap_or(self.points.len());
        self.points.insert(idx, AutomationPoint { tick, value: v });
        idx
    }

    /// Evaluates the lane at `tick`. Returns `None` if the lane has no points.
    pub fn evaluate_at_tick(&self, tick: u64) -> Option<f32> {
        if self.points.is_empty() {
            return None;
        }
        if self.points.len() == 1 || tick <= self.points[0].tick {
            return Some(self.points[0].value);
        }
        let last = self.points[self.points.len() - 1];
        if tick >= last.tick {
            return Some(last.value);
        }
        let mut lo = 0usize;
        let mut hi = self.points.len() - 1;
        while lo + 1 < hi {
            let mid = (lo + hi) / 2;
            if self.points[mid].tick <= tick {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let a = self.points[lo];
        let b = self.points[hi];
        let span = b.tick.saturating_sub(a.tick);
        if span == 0 {
            return Some(b.value);
        }
        let t = ((tick - a.tick) as f64 / span as f64).clamp(0.0, 1.0) as f32;
        Some(a.value + (b.value - a.value) * t)
    }
}

#[derive(Clone, Debug)]
pub struct AutomationEditorState {
    pub lanes: Vec<AutomationLane>,
    pub selected_target_by_track: HashMap<usize, AutomationTarget>,
    pub scroll_x_beats: f64,
    pub px_per_beat: f32,
    pub selected_point: Option<usize>,
    pub dragging_point: Option<usize>,
    pub scope_is_clip: bool,
    pub last_sample_count: u64,
    pub accum_tick_f64: f64,
}

impl Default for AutomationEditorState {
    fn default() -> Self {
        Self {
            lanes: Vec::new(),
            selected_target_by_track: HashMap::new(),
            scroll_x_beats: 0.0,
            px_per_beat: 76.0,
            selected_point: None,
            dragging_point: None,
            scope_is_clip: false,
            last_sample_count: 0,
            accum_tick_f64: 0.0,
        }
    }
}

impl AutomationEditorState {
    /// Smoothly integrates `delta_samples` into ticks using the instantaneous BPM so
    /// changing tempo never causes the playhead to jump backwards or freeze.
    pub fn advance_playhead_tick(
        &mut self,
        sample_count: u64,
        sample_rate: f64,
        current_bpm: f64,
        ppqn: f64,
    ) -> u64 {
        if sample_count < self.last_sample_count || self.last_sample_count == 0 {
            self.last_sample_count = sample_count;
            let sr = sample_rate.max(1.0);
            self.accum_tick_f64 = (sample_count as f64 / sr) * (current_bpm / 60.0) * ppqn;
        } else {
            let delta_samples = sample_count - self.last_sample_count;
            self.last_sample_count = sample_count;
            let sr = sample_rate.max(1.0);
            self.accum_tick_f64 += (delta_samples as f64 / sr) * (current_bpm / 60.0) * ppqn;
        }
        self.accum_tick_f64.round().max(0.0) as u64
    }

    pub fn reset_playhead_integrator(&mut self, sample_count: u64, tick: u64) {
        self.last_sample_count = sample_count;
        self.accum_tick_f64 = tick as f64;
    }
    pub fn active_target_for_track(&self, track_idx: usize) -> AutomationTarget {
        self.selected_target_by_track
            .get(&track_idx)
            .cloned()
            .unwrap_or(AutomationTarget::MixerVolume { track_idx })
    }

    pub fn set_active_target_for_track(&mut self, track_idx: usize, target: AutomationTarget) {
        self.selected_target_by_track.insert(track_idx, target);
        self.selected_point = None;
        self.dragging_point = None;
    }

    pub fn lane_for(&self, target: &AutomationTarget) -> Option<&AutomationLane> {
        self.lanes.iter().find(|l| &l.target == target)
    }

    pub fn lane_for_mut(&mut self, target: &AutomationTarget) -> &mut AutomationLane {
        if let Some(pos) = self.lanes.iter().position(|l| &l.target == target) {
            return &mut self.lanes[pos];
        }
        self.lanes.push(AutomationLane::new(target.clone()));
        let idx = self.lanes.len() - 1;
        &mut self.lanes[idx]
    }

    pub fn has_tempo_automation(&self) -> bool {
        self.lane_for(&AutomationTarget::TransportTempo)
            .is_some_and(|l| !l.points.is_empty())
    }

    pub fn evaluate_tempo_at_tick(&self, tick: u64) -> Option<f64> {
        self.lane_for(&AutomationTarget::TransportTempo)
            .and_then(|l| l.evaluate_at_tick(tick))
            .map(norm_to_bpm)
    }
}

/// Applies all active automation lanes at `playhead_tick` during playback.
pub fn apply_automation_at_tick(app: &mut AppState, tick: u64) {
    if let Some(bpm) = app.automation.evaluate_tempo_at_tick(tick) {
        if (app.transport.bpm - bpm).abs() > 0.02 {
            app.set_bpm_live(bpm);
        }
    }

    // Collect Mixer Volume / Pan updates to apply cleanly
    let updates: Vec<(AutomationTarget, f32)> = app
        .automation
        .lanes
        .iter()
        .filter(|l| !l.points.is_empty() && !matches!(l.target, AutomationTarget::TransportTempo))
        .filter_map(|l| l.evaluate_at_tick(tick).map(|v| (l.target.clone(), v)))
        .collect();

    for (target, val) in updates {
        match target {
            AutomationTarget::MixerVolume { track_idx } => {
                if let Some(tr) = app.current_tracks_mut().get_mut(track_idx) {
                    if (tr.volume - val).abs() > 1e-4 {
                        tr.volume = val;
                        if track_idx == 0 {
                            app.audio_proxy.send(GuiCommand::SetMasterVolume { volume_db: val });
                        }
                        app.audio_proxy.send(GuiCommand::SetTrackVolume {
                            track_idx,
                            volume_db: val,
                        });
                    }
                }
            }
            AutomationTarget::MixerPan { track_idx } => {
                let pan = (val * 2.0 - 1.0).clamp(-1.0, 1.0);
                if let Some(tr) = app.current_tracks_mut().get_mut(track_idx) {
                    if (tr.pan - pan).abs() > 1e-4 {
                        tr.pan = pan;
                        app.audio_proxy.send(GuiCommand::SetTrackPan { track_idx, pan });
                    }
                }
            }
            AutomationTarget::TrackSend { track_idx, fx_idx } => {
                if let Some(tr) = app.current_tracks_mut().get_mut(track_idx) {
                    tr.set_send_amount(ROUTE_FX_BASE + fx_idx, val);
                }
            }
            AutomationTarget::TransportTempo => {}
        }
    }
}

fn paint_rotated_label(
    painter: &egui::Painter,
    center_x: f32,
    bottom_y: f32,
    text: &str,
    font_size: f32,
    color: Color32,
) {
    let galley = painter.layout_no_wrap(text.to_string(), fonts::semibold(font_size), color);
    let mut ts = egui::epaint::TextShape::new(
        pos2(center_x - font_size * 0.55, bottom_y),
        galley,
        color,
    );
    ts.angle = -std::f32::consts::FRAC_PI_2;
    painter.add(egui::Shape::Text(ts));
}

pub fn render(ui: &mut Ui, app: &mut AppState) {
    let track_idx = app.safe_track_index();
    let (track_name, track_color, is_master) = app
        .current_tracks()
        .get(track_idx)
        .map(|t| (t.name.clone(), t.color, t.is_master))
        .unwrap_or_else(|| ("Master".to_string(), Color32::from_rgb(0x82, 0x84, 0x88), true));

    let full_rect = ui.available_rect_before_wrap();
    ui.allocate_rect(full_rect, Sense::hover());
    ui.painter().rect_filled(full_rect, 0.0, Color32::from_rgb(0x1E, 0x20, 0x24));

    // 1. Left vertical CLIP / TRACK strip (Bitwig style)
    let tab_w = 24.0;
    let name_w = 26.0;
    let strip_w = tab_w + name_w;
    let tab_rect = Rect::from_min_size(full_rect.min, vec2(tab_w, full_rect.height()));
    let name_rect = Rect::from_min_size(pos2(tab_rect.max.x, full_rect.min.y), vec2(name_w, full_rect.height()));

    {
        let p = ui.painter();
        p.rect_filled(tab_rect, 0.0, Color32::from_rgb(0x28, 0x2A, 0x2F));
        let half_h = tab_rect.height() * 0.5;
        let clip_tab = Rect::from_min_size(tab_rect.min, vec2(tab_w, half_h));
        let track_tab = Rect::from_min_max(pos2(tab_rect.min.x, clip_tab.max.y), tab_rect.max);

        let clip_r = ui.interact(clip_tab, ui.id().with("auto_clip_tab"), Sense::click());
        let trk_r = ui.interact(track_tab, ui.id().with("auto_trk_tab"), Sense::click());
        if clip_r.clicked() {
            app.automation.scope_is_clip = true;
        }
        if trk_r.clicked() {
            app.automation.scope_is_clip = false;
        }

        let is_clip = app.automation.scope_is_clip;
        p.rect_filled(
            clip_tab,
            0.0,
            if is_clip { Color32::from_rgb(0x3A, 0x3D, 0x45) } else { Color32::from_rgb(0x26, 0x28, 0x2D) },
        );
        p.rect_filled(
            track_tab,
            0.0,
            if !is_clip { Color32::from_rgb(0x3A, 0x3D, 0x45) } else { Color32::from_rgb(0x26, 0x28, 0x2D) },
        );
        paint_rotated_label(
            p,
            clip_tab.center().x,
            clip_tab.center().y + 10.0,
            "CLIP",
            8.5,
            if is_clip { theme::ACCENT_ORANGE } else { Color32::from_rgb(0x9A, 0x9E, 0xA8) },
        );
        paint_rotated_label(
            p,
            track_tab.center().x,
            track_tab.center().y + 14.0,
            "TRACK",
            8.5,
            if !is_clip { theme::ACCENT_ORANGE } else { Color32::from_rgb(0x9A, 0x9E, 0xA8) },
        );

        p.rect_filled(name_rect, 0.0, Color32::from_rgb(0x2F, 0x32, 0x38));
        p.rect_filled(
            Rect::from_min_size(name_rect.min, vec2(3.0, name_rect.height())),
            0.0,
            track_color,
        );
        paint_rotated_label(
            p,
            name_rect.center().x + 1.0,
            name_rect.center().y + 22.0,
            &track_name.to_uppercase(),
            9.0,
            Color32::from_rgb(0xD8, 0xDC, 0xE4),
        );
        p.line_segment(
            [name_rect.right_top(), name_rect.right_bottom()],
            Stroke::new(1.0_f32, Color32::from_rgb(0x16, 0x18, 0x1C)),
        );
    }

    // 2. Parameter Selector Column (`136px` wide, Bitwig `Mixer / Volume` -> `Transport -> Tempo`)
    let selector_w = 136.0;
    let selector_rect = Rect::from_min_size(
        pos2(full_rect.min.x + strip_w, full_rect.min.y),
        vec2(selector_w, full_rect.height()),
    );
    let active_target = app.automation.active_target_for_track(track_idx);
    let cat_label = active_target.category_name();
    let param_label = active_target.param_name(app);

    {
        let p = ui.painter();
        p.rect_filled(selector_rect, 0.0, Color32::from_rgb(0x2A, 0x2C, 0x32));
        p.line_segment(
            [selector_rect.right_top(), selector_rect.right_bottom()],
            Stroke::new(1.0_f32, Color32::from_rgb(0x14, 0x16, 0x1A)),
        );

        // Top mini toolbar (pointer tool)
        let tool_bar = Rect::from_min_size(selector_rect.min, vec2(selector_w, 26.0));
        p.rect_filled(tool_bar, 0.0, Color32::from_rgb(0x36, 0x38, 0x40));
        icons::paint_pointer_cursor(
            p,
            Rect::from_center_size(pos2(tool_bar.max.x - 16.0, tool_bar.center().y), vec2(12.0, 12.0)),
            Color32::from_rgb(0xE4, 0xE8, 0xF0),
        );
        p.line_segment(
            [tool_bar.left_bottom(), tool_bar.right_bottom()],
            Stroke::new(1.0_f32, Color32::from_rgb(0x18, 0x1A, 0x1F)),
        );
    }

    let dropdown_rect = Rect::from_min_size(
        pos2(selector_rect.min.x + 5.0, selector_rect.min.y + 32.0),
        vec2(selector_w - 10.0, 34.0),
    );
    let dd_resp = ui.interact(dropdown_rect, ui.id().with(("auto_param_dd", track_idx)), Sense::click());
    {
        let p = ui.painter();
        p.rect_filled(
            dropdown_rect,
            2.5,
            if dd_resp.hovered() {
                Color32::from_rgb(0x3C, 0x40, 0x49)
            } else {
                Color32::from_rgb(0x33, 0x36, 0x3E)
            },
        );
        p.rect_stroke(
            dropdown_rect,
            2.5,
            Stroke::new(1.0_f32, Color32::from_rgb(0x1E, 0x20, 0x26)),
            egui::StrokeKind::Inside,
        );
        p.text(
            pos2(dropdown_rect.min.x + 7.0, dropdown_rect.min.y + 10.0),
            Align2::LEFT_CENTER,
            cat_label,
            fonts::regular(9.5),
            Color32::from_rgb(0xA4, 0xA8, 0xB2),
        );
        p.text(
            pos2(dropdown_rect.min.x + 7.0, dropdown_rect.min.y + 24.0),
            Align2::LEFT_CENTER,
            &param_label,
            fonts::semibold(10.5),
            Color32::from_rgb(0xE8, 0xEC, 0xF4),
        );
        icons::paint_caret_down(
            p,
            Rect::from_center_size(
                pos2(dropdown_rect.max.x - 10.0, dropdown_rect.center().y),
                vec2(7.0, 4.5),
            ),
            Color32::from_rgb(0xB8, 0xBC, 0xC6),
        );
    }
    if dd_resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let dd_popup_id = ui.id().with(("auto_param_popup", track_idx));
    if dd_resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(dd_popup_id));
    }

    let fx_names: Vec<String> = app.fx_tracks.iter().map(|f| f.name.clone()).collect();
    egui::popup_below_widget(
        ui,
        dd_popup_id,
        &dd_resp,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(140.0);
            ui.menu_button("Mixer", |ui| {
                if ui.button("Volume").clicked() {
                    app.automation.set_active_target_for_track(
                        track_idx,
                        AutomationTarget::MixerVolume { track_idx },
                    );
                    ui.close_menu();
                }
                if ui.button("Pan").clicked() {
                    app.automation.set_active_target_for_track(
                        track_idx,
                        AutomationTarget::MixerPan { track_idx },
                    );
                    ui.close_menu();
                }
                if !is_master && !fx_names.is_empty() {
                    ui.separator();
                    for (fx_idx, fx_name) in fx_names.iter().enumerate() {
                        if ui.button(format!("Send: {}", fx_name)).clicked() {
                            app.automation.set_active_target_for_track(
                                track_idx,
                                AutomationTarget::TrackSend { track_idx, fx_idx },
                            );
                            ui.close_menu();
                        }
                    }
                }
            });
            ui.menu_button("Transport", |ui| {
                if ui.button("Tempo").clicked() {
                    app.automation
                        .set_active_target_for_track(track_idx, AutomationTarget::TransportTempo);
                    ui.close_menu();
                }
            });
            if app.automation.lane_for(&active_target).is_some_and(|l| !l.points.is_empty()) {
                ui.separator();
                if ui.button("Clear Lane Automation").clicked() {
                    app.automation.lane_for_mut(&active_target).points.clear();
                    ui.close_menu();
                }
            }
        },
    );

    // 3. Automation Timeline Canvas (Ruler + Blue Curve + Nodes)
    let timeline_rect = Rect::from_min_max(
        pos2(selector_rect.max.x, full_rect.min.y),
        full_rect.max,
    );
    let ruler_h = 22.0;
    let ruler_rect = Rect::from_min_size(timeline_rect.min, vec2(timeline_rect.width(), ruler_h));
    let canvas_rect = Rect::from_min_max(
        pos2(timeline_rect.min.x, ruler_rect.max.y),
        timeline_rect.max,
    );

    // Handle horizontal zoom (Ctrl+Scroll) and pan (Scroll)
    let hover_timeline = ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| timeline_rect.contains(p));
    if hover_timeline {
        let (scroll, ctrl) = ui.input(|i| (i.raw_scroll_delta, i.modifiers.ctrl));
        if ctrl && scroll.y.abs() > 0.0 {
            let factor = if scroll.y > 0.0 { 1.12 } else { 0.89 };
            app.automation.px_per_beat = (app.automation.px_per_beat * factor).clamp(18.0, 260.0);
        } else if scroll.x.abs() > 0.0 || scroll.y.abs() > 0.0 {
            let dx = if scroll.x.abs() > 0.0 { scroll.x } else { scroll.y };
            app.automation.scroll_x_beats =
                (app.automation.scroll_x_beats - dx as f64 / app.automation.px_per_beat as f64).max(0.0);
        }
    }

    let px_per_beat = app.automation.px_per_beat;
    let scroll_beats = app.automation.scroll_x_beats;
    let beat_to_x = |beat: f64| -> f32 {
        canvas_rect.min.x + ((beat - scroll_beats) as f32) * px_per_beat
    };
    let tick_to_x = |tick: u64| -> f32 { beat_to_x(tick as f64 / PPQN) };
    let x_to_tick = |x: f32| -> u64 {
        let beat = scroll_beats + ((x - canvas_rect.min.x) / px_per_beat).max(0.0) as f64;
        (beat * PPQN).round().max(0.0) as u64
    };
    let val_to_y = |v: f32| -> f32 {
        let pad = 6.0;
        let usable = (canvas_rect.height() - pad * 2.0).max(12.0);
        canvas_rect.max.y - pad - v.clamp(0.0, 1.0) * usable
    };
    let y_to_val = |y: f32| -> f32 {
        let pad = 6.0;
        let usable = (canvas_rect.height() - pad * 2.0).max(12.0);
        ((canvas_rect.max.y - pad - y) / usable).clamp(0.0, 1.0)
    };

    // Draw background, beat grid, and ruler
    {
        let p = ui.painter().with_clip_rect(timeline_rect);
        p.rect_filled(ruler_rect, 0.0, Color32::from_rgb(0x2B, 0x2D, 0x33));
        p.rect_filled(canvas_rect, 0.0, Color32::from_rgb(0x52, 0x54, 0x5A));
        p.line_segment(
            [ruler_rect.left_bottom(), ruler_rect.right_bottom()],
            Stroke::new(1.0_f32, Color32::from_rgb(0x18, 0x1A, 0x1E)),
        );

        let visible_beats = (canvas_rect.width() / px_per_beat).ceil() as i32 + 2;
        let start_beat = scroll_beats.floor() as i32;
        for b in start_beat..=(start_beat + visible_beats) {
            if b < 0 {
                continue;
            }
            let x = beat_to_x(b as f64);
            let is_bar = b % 4 == 0;
            let bar_num = b / 4 + 1;
            let beat_in_bar = b % 4 + 1;

            p.line_segment(
                [pos2(x, ruler_rect.min.y), pos2(x, canvas_rect.max.y)],
                Stroke::new(
                    if is_bar { 1.1_f32 } else { 0.8_f32 },
                    if is_bar {
                        Color32::from_rgb(0x36, 0x38, 0x3E)
                    } else {
                        Color32::from_rgb(0x45, 0x47, 0x4D)
                    },
                ),
            );

            let lbl = if is_bar {
                format!("{}", bar_num)
            } else {
                format!("{}.{}", bar_num, beat_in_bar)
            };
            p.text(
                pos2(x + 3.0, ruler_rect.center().y),
                Align2::LEFT_CENTER,
                lbl,
                fonts::regular(if is_bar { 10.0 } else { 9.0 }),
                if is_bar {
                    Color32::from_rgb(0xE0, 0xE4, 0xEC)
                } else {
                    Color32::from_rgb(0x98, 0x9C, 0xA6)
                },
            );
        }

        // Playhead vertical line (synced to Arranger/Playlist playhead, not Piano Roll loop)
        let ph_x = tick_to_x(app.playlist_state.playhead_tick);
        if ph_x >= canvas_rect.min.x && ph_x <= canvas_rect.max.x {
            p.line_segment(
                [pos2(ph_x, ruler_rect.min.y), pos2(ph_x, canvas_rect.max.y)],
                Stroke::new(1.3_f32, AUTO_BLUE),
            );
            let tri = vec![
                pos2(ph_x - 4.0, ruler_rect.min.y),
                pos2(ph_x + 4.0, ruler_rect.min.y),
                pos2(ph_x, ruler_rect.min.y + 7.0),
            ];
            p.add(egui::Shape::convex_polygon(tri, AUTO_BLUE, Stroke::NONE));
        }
    }

    // Interact with canvas (insert, drag, delete nodes)
    let canvas_resp = ui.interact(
        canvas_rect,
        ui.id().with(("auto_canvas", track_idx)),
        Sense::click_and_drag(),
    );
    let baseline_norm = active_target.default_norm(app);

    let pointer_pos = ui.input(|i| i.pointer.interact_pos());
    let shift = ui.input(|i| i.modifiers.shift);

    // Hit-test existing node
    let hovered_node_idx: Option<usize> = pointer_pos.and_then(|pos| {
        app.automation.lane_for(&active_target).and_then(|lane| {
            lane.points.iter().enumerate().find_map(|(idx, pt)| {
                let pt_pos = pos2(tick_to_x(pt.tick), val_to_y(pt.value));
                if pt_pos.distance(pos) <= 8.5 {
                    Some(idx)
                } else {
                    None
                }
            })
        })
    });

    let right_clicked = ui.input(|i| i.pointer.secondary_clicked());
    if right_clicked {
        if let Some(idx) = hovered_node_idx {
            app.automation.lane_for_mut(&active_target).points.remove(idx);
            app.automation.selected_point = None;
            app.automation.dragging_point = None;
        }
    } else if canvas_resp.dragged_by(egui::PointerButton::Middle) {
        let delta = canvas_resp.drag_delta();
        // Bitwig-style MIDDLE-DRAG: vertical = zoom, horizontal = pan
        if delta.y.abs() > 0.0 {
            let old_ppb = app.automation.px_per_beat;
            let factor = (1.0 - delta.y * 0.008).clamp(0.85, 1.15);
            let new_ppb = (old_ppb * factor).clamp(18.0, 260.0);
            if let Some(pos) = pointer_pos {
                let rel_x = (pos.x - canvas_rect.min.x).max(0.0) as f64;
                let anchor_beat = app.automation.scroll_x_beats + rel_x / old_ppb as f64;
                app.automation.scroll_x_beats = (anchor_beat - rel_x / new_ppb as f64).max(0.0);
            }
            app.automation.px_per_beat = new_ppb;
        }
        if delta.x.abs() > 0.0 {
            app.automation.scroll_x_beats = (app.automation.scroll_x_beats
                - delta.x as f64 / app.automation.px_per_beat as f64)
                .max(0.0);
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if canvas_resp.clicked_by(egui::PointerButton::Primary) {
        if let Some(idx) = hovered_node_idx {
            app.automation.selected_point = Some(idx);
        } else if let Some(pos) = pointer_pos {
            if canvas_rect.contains(pos) {
                let raw_tick = x_to_tick(pos.x);
                let tick = if shift {
                    raw_tick
                } else {
                    // Snap to 1/4 beat (240 ticks)
                    ((raw_tick as f64 / 240.0).round() * 240.0) as u64
                };
                let val = y_to_val(pos.y);
                let idx = app.automation.lane_for_mut(&active_target).insert_point(tick, val);
                app.automation.selected_point = Some(idx);
                if matches!(active_target, AutomationTarget::TransportTempo) {
                    app.set_bpm(norm_to_bpm(val));
                }
            }
        }
    }

    if canvas_resp.drag_started_by(egui::PointerButton::Primary) {
        if let Some(idx) = hovered_node_idx {
            app.automation.dragging_point = Some(idx);
            app.automation.selected_point = Some(idx);
        } else if let Some(pos) = pointer_pos {
            if canvas_rect.contains(pos) {
                let raw_tick = x_to_tick(pos.x);
                let tick = if shift {
                    raw_tick
                } else {
                    ((raw_tick as f64 / 240.0).round() * 240.0) as u64
                };
                let val = y_to_val(pos.y);
                let idx = app.automation.lane_for_mut(&active_target).insert_point(tick, val);
                app.automation.dragging_point = Some(idx);
                app.automation.selected_point = Some(idx);
            }
        }
    }

    if canvas_resp.dragged_by(egui::PointerButton::Primary) {
        if let (Some(drag_idx), Some(pos)) = (app.automation.dragging_point, pointer_pos) {
            let raw_tick = x_to_tick(pos.x);
            let tick = if shift {
                raw_tick
            } else {
                ((raw_tick as f64 / 240.0).round() * 240.0) as u64
            };
            let val = y_to_val(pos.y);
            let lane = app.automation.lane_for_mut(&active_target);
            if let Some(pt) = lane.points.get_mut(drag_idx) {
                pt.tick = tick;
                pt.value = val;
                lane.sort_points();
                app.automation.dragging_point = lane.points.iter().position(|p| p.tick == tick);
                app.automation.selected_point = app.automation.dragging_point;
            }
            if matches!(active_target, AutomationTarget::TransportTempo) {
                app.set_bpm(norm_to_bpm(val));
            }
        }
    }
    if canvas_resp.drag_stopped() {
        app.automation.dragging_point = None;
    }

    // Draw the blue curve, translucent blue fill, nodes, and floating value tooltip
    {
        let p = ui.painter().with_clip_rect(canvas_rect);
        let points_snapshot: Vec<AutomationPoint> = app
            .automation
            .lane_for(&active_target)
            .map(|l| l.points.clone())
            .unwrap_or_default();

        if points_snapshot.is_empty() {
            let base_y = val_to_y(baseline_norm);
            p.line_segment(
                [pos2(canvas_rect.min.x, base_y), pos2(canvas_rect.max.x, base_y)],
                Stroke::new(1.5_f32, AUTO_BLUE),
            );
        } else {
            let mut poly_pts: Vec<Pos2> = Vec::with_capacity(points_snapshot.len() + 2);
            let first_y = val_to_y(points_snapshot[0].value);
            poly_pts.push(pos2(canvas_rect.min.x, first_y));
            for pt in &points_snapshot {
                poly_pts.push(pos2(tick_to_x(pt.tick), val_to_y(pt.value)));
            }
            let last_y = val_to_y(points_snapshot[points_snapshot.len() - 1].value);
            poly_pts.push(pos2(canvas_rect.max.x, last_y));

            // Translucent fill under the curve (trapezoid strips)
            let bottom_y = canvas_rect.max.y;
            let fill_col = Color32::from_rgba_unmultiplied(0x3A, 0xA8, 0xE0, 62);
            for seg in poly_pts.windows(2) {
                let a = seg[0];
                let b = seg[1];
                let quad = vec![a, b, pos2(b.x, bottom_y), pos2(a.x, bottom_y)];
                p.add(egui::Shape::convex_polygon(quad, fill_col, Stroke::NONE));
            }

            // Curve stroke
            p.add(egui::Shape::line(poly_pts, Stroke::new(1.7_f32, AUTO_BLUE)));

            // Nodes
            for (idx, pt) in points_snapshot.iter().enumerate() {
                let c = pos2(tick_to_x(pt.tick), val_to_y(pt.value));
                let is_active = app.automation.dragging_point == Some(idx)
                    || app.automation.selected_point == Some(idx)
                    || hovered_node_idx == Some(idx);
                p.circle_filled(
                    c,
                    if is_active { 4.2 } else { 3.5 },
                    if is_active { Color32::WHITE } else { Color32::from_rgb(0x10, 0x12, 0x15) },
                );
                p.circle_stroke(
                    c,
                    if is_active { 4.2 } else { 3.5 },
                    Stroke::new(
                        1.5_f32,
                        if is_active { AUTO_NODE_ACTIVE } else { AUTO_BLUE },
                    ),
                );
            }

            // Bitwig floating value badge (`20.00 BPM`, `145.00 BPM`, etc.)
            if let Some(active_idx) = app.automation.dragging_point.or(hovered_node_idx) {
                if let Some(pt) = points_snapshot.get(active_idx) {
                    let node_pos = pos2(tick_to_x(pt.tick), val_to_y(pt.value));
                    let txt = active_target.format_norm(pt.value);
                    let badge_w = 72.0;
                    let badge_h = 18.0;
                    let bx = (node_pos.x + 10.0).min(canvas_rect.max.x - badge_w - 4.0);
                    let by = (node_pos.y - badge_h - 4.0).max(canvas_rect.min.y + 4.0);
                    let br = Rect::from_min_size(pos2(bx, by), vec2(badge_w, badge_h));
                    p.rect_filled(br, 2.5, Color32::from_rgb(0x14, 0x16, 0x1A));
                    p.rect_stroke(
                        br,
                        2.5,
                        Stroke::new(1.0_f32, Color32::from_rgb(0x3A, 0x3E, 0x48)),
                        egui::StrokeKind::Inside,
                    );
                    p.text(
                        br.center(),
                        Align2::CENTER_CENTER,
                        &txt,
                        fonts::semibold(10.0),
                        Color32::WHITE,
                    );
                    app.footer_context_help = Some((
                        active_target.param_name(app).to_uppercase(),
                        txt,
                    ));
                }
            }
        }
    }

    if hovered_node_idx.is_some() || app.automation.dragging_point.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    if canvas_resp.hovered() {
        app.footer_context_cues = vec![
            ("CLICK".to_string(), "Insert / Select point".to_string()),
            ("DRAG".to_string(), "Move point".to_string()),
            ("RIGHT-CLICK".to_string(), "Delete point".to_string()),
            ("MIDDLE-DRAG".to_string(), "Zoom & pan".to_string()),
            ("SHIFT".to_string(), "Free time".to_string()),
        ];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tempo_automation_range_20_to_666_and_interpolation() {
        assert!((BPM_MIN - 20.0).abs() < 1e-6);
        assert!((BPM_MAX - 666.0).abs() < 1e-6);

        let mut lane = AutomationLane::new(AutomationTarget::TransportTempo);
        assert_eq!(lane.evaluate_at_tick(0), None);

        lane.insert_point(0, bpm_to_norm(20.0));
        lane.insert_point(3840, bpm_to_norm(666.0)); // 4 beats later

        let start_bpm = norm_to_bpm(lane.evaluate_at_tick(0).unwrap());
        let mid_bpm = norm_to_bpm(lane.evaluate_at_tick(1920).unwrap());
        let end_bpm = norm_to_bpm(lane.evaluate_at_tick(3840).unwrap());

        assert!((start_bpm - 20.0).abs() < 0.05);
        assert!((mid_bpm - 343.0).abs() < 0.1);
        assert!((end_bpm - 666.0).abs() < 0.05);
    }
}
