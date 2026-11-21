// crates/hikaru_gui/src/views/remote_controls.rs
//
// Bitwig Studio-style Project Remotes & Track Remotes panels (plus collapsible
// Project / Track Modulator bays) for the bottom Device Panel (`dsp_rack.rs`).
// GNU AGPLv3

use egui::{pos2, vec2, Align2, Color32, Painter, Pos2, Rect, RichText, Sense, Stroke, Ui};

use crate::app::{AppState, BPM_MAX, BPM_MIN};
use crate::audio_proxy::GuiCommand;
use crate::ui::fonts;
use crate::theme;
use crate::ui::icons;
use crate::views::hikaru_native_plugins::hikaru_openmodulation;
use crate::views::mixer::{self, ROUTE_FX_BASE};

/// Reserved modulator bay slot ID for Project-level modulators (track 0).
pub const PROJECT_MODULATOR_SLOT: usize = usize::MAX - 1;
/// Reserved modulator bay slot ID for Track-level modulators (per `track_idx`).
pub const TRACK_MODULATOR_SLOT: usize = usize::MAX - 2;

/// Number of remote control knobs per page (2 rows × 4 columns, Bitwig layout).
pub const REMOTE_SLOTS_PER_PAGE: usize = 8;

/// Width of the vertical `PROJECT` / `<TRACK>` sidebar strip.
pub const STRIP_WIDTH: f32 = 28.0;
/// Width of the 2×4 `Project Remotes` / `Track Remotes` panel body.
pub const REMOTES_BODY_WIDTH: f32 = 216.0;

const REMOTE_CYAN: Color32 = Color32::from_rgb(0x2E, 0xC5, 0xE8);
const REMOTE_ORANGE: Color32 = Color32::from_rgb(0xFF, 0x72, 0x16);

/// Parameter target that a Remote Control knob can drive bidirectionally.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteTarget {
    /// Global project BPM (`BPM_MIN..=BPM_MAX`).
    ProjectTempo,
    /// Master bus volume (`0.0..=1.0`).
    MasterVolume,
    /// Master bus pan (`-1.0..=1.0` mapped to `0.0..=1.0`).
    MasterPan,
    /// Specific track volume (`0.0..=1.0`).
    TrackVolume { track_idx: usize },
    /// Specific track pan (`-1.0..=1.0` mapped to `0.0..=1.0`).
    TrackPan { track_idx: usize },
    /// Specific track send amount (`0.0..=1.0`) to FX bus `fx_idx`.
    TrackSend { track_idx: usize, fx_idx: usize },
    /// External/cached device parameter on `(track_idx, slot_idx, param_idx)`.
    DeviceParam {
        track_idx: usize,
        slot_idx: usize,
        param_idx: usize,
    },
    /// OpenPoly native synth Cutoff (`0.0..=1.0`).
    OpenPolyCutoff { track_idx: usize, slot_idx: usize },
    /// OpenPoly native synth Resonance (`0.0..=1.0`).
    OpenPolyReso { track_idx: usize, slot_idx: usize },
    /// OpenPoly native synth Drive (`0.0..=1.0`).
    OpenPolyDrive { track_idx: usize, slot_idx: usize },
}

/// One of the 8 knobs on a Remote Controls page.
#[derive(Clone, Debug)]
pub struct RemoteControlSlot {
    pub target: Option<RemoteTarget>,
    pub custom_label: Option<String>,
    pub value: f32,
    pub default_value: f32,
}

impl Default for RemoteControlSlot {
    fn default() -> Self {
        Self {
            target: None,
            custom_label: None,
            value: 0.5,
            default_value: 0.5,
        }
    }
}

impl RemoteControlSlot {
    pub fn assign(&mut self, target: RemoteTarget, current_norm: f32, default_norm: f32) {
        self.target = Some(target);
        self.custom_label = None;
        self.value = current_norm.clamp(0.0, 1.0);
        self.default_value = default_norm.clamp(0.0, 1.0);
    }

    pub fn unmap(&mut self) {
        self.target = None;
        self.custom_label = None;
        self.value = 0.5;
        self.default_value = 0.5;
    }
}

/// A named page of 8 Remote Control slots (e.g. `"Perform"`).
#[derive(Clone, Debug)]
pub struct RemoteControlPage {
    pub name: String,
    pub slots: [RemoteControlSlot; REMOTE_SLOTS_PER_PAGE],
}

impl RemoteControlPage {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            slots: std::array::from_fn(|_| RemoteControlSlot::default()),
        }
    }

    pub fn clear(&mut self) {
        for s in &mut self.slots {
            s.unmap();
        }
    }
}

/// Complete state of a Project Remotes or Track Remotes panel + its modulator bay toggle.
#[derive(Clone, Debug)]
pub struct RemoteControlsState {
    pub pages: Vec<RemoteControlPage>,
    pub active_page: usize,
    pub remotes_open: bool,
    pub modulators_open: bool,
}

impl Default for RemoteControlsState {
    fn default() -> Self {
        Self {
            pages: vec![RemoteControlPage::new("Perform")],
            active_page: 0,
            remotes_open: false,
            modulators_open: false,
        }
    }
}

impl RemoteControlsState {
    pub fn ensure_valid_page(&mut self) -> usize {
        if self.pages.is_empty() {
            self.pages.push(RemoteControlPage::new("Perform"));
        }
        if self.active_page >= self.pages.len() {
            self.active_page = self.pages.len() - 1;
        }
        self.active_page
    }

    pub fn active_page_ref(&self) -> &RemoteControlPage {
        let idx = self.active_page.min(self.pages.len().saturating_sub(1));
        &self.pages[idx]
    }

    pub fn active_page_mut(&mut self) -> &mut RemoteControlPage {
        let idx = self.ensure_valid_page();
        &mut self.pages[idx]
    }

    pub fn add_page(&mut self) -> usize {
        let n = self.pages.len() + 1;
        self.pages.push(RemoteControlPage::new(format!("Page {}", n)));
        self.active_page = self.pages.len() - 1;
        self.active_page
    }

    pub fn remove_active_page(&mut self) {
        if self.pages.len() > 1 {
            let idx = self.ensure_valid_page();
            self.pages.remove(idx);
            self.ensure_valid_page();
        } else {
            self.active_page_mut().clear();
        }
    }
}

// =============================================================================
// Bidirectional target read / write / formatting
// =============================================================================

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

pub fn default_norm_for_target(target: &RemoteTarget) -> f32 {
    match target {
        RemoteTarget::ProjectTempo => bpm_to_norm(120.0),
        RemoteTarget::MasterVolume | RemoteTarget::TrackVolume { .. } => 0.75,
        RemoteTarget::MasterPan | RemoteTarget::TrackPan { .. } => 0.5,
        RemoteTarget::TrackSend { .. } => 0.0,
        RemoteTarget::DeviceParam { .. } => 0.5,
        RemoteTarget::OpenPolyCutoff { .. } => 0.8,
        RemoteTarget::OpenPolyReso { .. } => 0.15,
        RemoteTarget::OpenPolyDrive { .. } => 0.0,
    }
}

pub fn read_target_normalized(app: &AppState, target: &RemoteTarget) -> Option<f32> {
    match *target {
        RemoteTarget::ProjectTempo => Some(bpm_to_norm(app.transport.bpm)),
        RemoteTarget::MasterVolume => app.current_tracks().first().map(|t| t.volume.clamp(0.0, 1.0)),
        RemoteTarget::MasterPan => app
            .current_tracks()
            .first()
            .map(|t| ((t.pan + 1.0) * 0.5).clamp(0.0, 1.0)),
        RemoteTarget::TrackVolume { track_idx } => app
            .current_tracks()
            .get(track_idx)
            .map(|t| t.volume.clamp(0.0, 1.0)),
        RemoteTarget::TrackPan { track_idx } => app
            .current_tracks()
            .get(track_idx)
            .map(|t| ((t.pan + 1.0) * 0.5).clamp(0.0, 1.0)),
        RemoteTarget::TrackSend { track_idx, fx_idx } => app
            .current_tracks()
            .get(track_idx)
            .map(|t| t.send_amount(ROUTE_FX_BASE + fx_idx).clamp(0.0, 1.0)),
        RemoteTarget::DeviceParam {
            track_idx,
            slot_idx,
            param_idx,
        } => app
            .current_tracks()
            .get(track_idx)
            .and_then(|t| t.effects.get(slot_idx))
            .and_then(|s| s.cached_params.get(param_idx))
            .map(|p| p.value.clamp(0.0, 1.0)),
        RemoteTarget::OpenPolyCutoff { track_idx, slot_idx } => app
            .current_tracks()
            .get(track_idx)
            .and_then(|t| t.effects.get(slot_idx))
            .and_then(|s| s.open_poly.as_ref())
            .map(|p| ((p.filter_cutoff_hz.clamp(20.0, 20000.0) / 20.0).log10() / 3.0).clamp(0.0, 1.0)),
        RemoteTarget::OpenPolyReso { track_idx, slot_idx } => app
            .current_tracks()
            .get(track_idx)
            .and_then(|t| t.effects.get(slot_idx))
            .and_then(|s| s.open_poly.as_ref())
            .map(|p| p.filter_resonance.clamp(0.0, 1.0)),
        RemoteTarget::OpenPolyDrive { track_idx, slot_idx } => app
            .current_tracks()
            .get(track_idx)
            .and_then(|t| t.effects.get(slot_idx))
            .and_then(|s| s.open_poly.as_ref())
            .map(|p| p.filter_drive.clamp(0.0, 1.0)),
    }
}

pub fn write_target_normalized(app: &mut AppState, target: &RemoteTarget, norm: f32) {
    let v = if norm.is_finite() {
        norm.clamp(0.0, 1.0)
    } else {
        0.5
    };
    match *target {
        RemoteTarget::ProjectTempo => {
            app.set_bpm(norm_to_bpm(v));
        }
        RemoteTarget::MasterVolume => {
            if let Some(t) = app.current_tracks_mut().first_mut() {
                t.volume = v;
            }
            app.audio_proxy.send(GuiCommand::SetMasterVolume { volume_db: v });
            app.audio_proxy.send(GuiCommand::SetTrackVolume {
                track_idx: 0,
                volume_db: v,
            });
        }
        RemoteTarget::MasterPan => {
            let pan = (v * 2.0 - 1.0).clamp(-1.0, 1.0);
            if let Some(t) = app.current_tracks_mut().first_mut() {
                t.pan = pan;
            }
            app.audio_proxy.send(GuiCommand::SetTrackPan { track_idx: 0, pan });
        }
        RemoteTarget::TrackVolume { track_idx } => {
            if let Some(t) = app.current_tracks_mut().get_mut(track_idx) {
                t.volume = v;
            }
            app.audio_proxy.send(GuiCommand::SetTrackVolume {
                track_idx,
                volume_db: v,
            });
        }
        RemoteTarget::TrackPan { track_idx } => {
            let pan = (v * 2.0 - 1.0).clamp(-1.0, 1.0);
            if let Some(t) = app.current_tracks_mut().get_mut(track_idx) {
                t.pan = pan;
            }
            app.audio_proxy.send(GuiCommand::SetTrackPan { track_idx, pan });
        }
        RemoteTarget::TrackSend { track_idx, fx_idx } => {
            if let Some(t) = app.current_tracks_mut().get_mut(track_idx) {
                t.set_send_amount(ROUTE_FX_BASE + fx_idx, v);
            }
        }
        RemoteTarget::DeviceParam {
            track_idx,
            slot_idx,
            param_idx,
        } => {
            if let Some(slot) = app
                .current_tracks_mut()
                .get_mut(track_idx)
                .and_then(|t| t.effects.get_mut(slot_idx))
            {
                if let Some(p) = slot.cached_params.get_mut(param_idx) {
                    p.value = v;
                    p.pending_to_engine = true;
                }
            }
        }
        RemoteTarget::OpenPolyCutoff { track_idx, slot_idx } => {
            if let Some(poly) = app
                .current_tracks_mut()
                .get_mut(track_idx)
                .and_then(|t| t.effects.get_mut(slot_idx))
                .and_then(|s| s.open_poly.as_mut())
            {
                poly.filter_cutoff_hz = (20.0 * 1000.0_f32.powf(v)).clamp(20.0, 20000.0);
                poly.mark_params_dirty();
            }
        }
        RemoteTarget::OpenPolyReso { track_idx, slot_idx } => {
            if let Some(poly) = app
                .current_tracks_mut()
                .get_mut(track_idx)
                .and_then(|t| t.effects.get_mut(slot_idx))
                .and_then(|s| s.open_poly.as_mut())
            {
                poly.filter_resonance = v;
                poly.mark_params_dirty();
            }
        }
        RemoteTarget::OpenPolyDrive { track_idx, slot_idx } => {
            if let Some(poly) = app
                .current_tracks_mut()
                .get_mut(track_idx)
                .and_then(|t| t.effects.get_mut(slot_idx))
                .and_then(|s| s.open_poly.as_mut())
            {
                poly.filter_drive = v;
                poly.mark_params_dirty();
            }
        }
    }
}

pub fn target_short_label(app: &AppState, target: &RemoteTarget) -> String {
    match *target {
        RemoteTarget::ProjectTempo => "Tempo".to_string(),
        RemoteTarget::MasterVolume => "Mst Vol".to_string(),
        RemoteTarget::MasterPan => "Mst Pan".to_string(),
        RemoteTarget::TrackVolume { track_idx } => {
            let name = app
                .current_tracks()
                .get(track_idx)
                .map(|t| t.name.as_str())
                .unwrap_or("Trk");
            if name.len() <= 5 {
                format!("{} Vol", name)
            } else {
                "Volume".to_string()
            }
        }
        RemoteTarget::TrackPan { .. } => "Pan".to_string(),
        RemoteTarget::TrackSend { fx_idx, .. } => format!("Send {}", fx_idx + 1),
        RemoteTarget::DeviceParam {
            track_idx,
            slot_idx,
            param_idx,
        } => app
            .current_tracks()
            .get(track_idx)
            .and_then(|t| t.effects.get(slot_idx))
            .and_then(|s| s.cached_params.get(param_idx))
            .map(|p| truncate_label(&p.info.name, 7))
            .unwrap_or_else(|| "Param".to_string()),
        RemoteTarget::OpenPolyCutoff { .. } => "Cutoff".to_string(),
        RemoteTarget::OpenPolyReso { .. } => "Reso".to_string(),
        RemoteTarget::OpenPolyDrive { .. } => "Drive".to_string(),
    }
}

pub fn format_target_value(app: &AppState, target: &RemoteTarget, norm: f32) -> String {
    match *target {
        RemoteTarget::ProjectTempo => format!("{:.2} BPM", norm_to_bpm(norm)),
        RemoteTarget::MasterVolume | RemoteTarget::TrackVolume { .. } => mixer::db_text(norm),
        RemoteTarget::MasterPan | RemoteTarget::TrackPan { .. } => {
            let pan = norm * 2.0 - 1.0;
            if pan.abs() < 0.01 {
                "C".to_string()
            } else if pan < 0.0 {
                format!("{:.0}% L", -pan * 100.0)
            } else {
                format!("{:.0}% R", pan * 100.0)
            }
        }
        RemoteTarget::TrackSend { .. } => mixer::send_db_text(norm),
        RemoteTarget::DeviceParam {
            track_idx,
            slot_idx,
            param_idx,
        } => app
            .current_tracks()
            .get(track_idx)
            .and_then(|t| t.effects.get(slot_idx))
            .and_then(|s| s.cached_params.get(param_idx))
            .map(|p| p.display())
            .unwrap_or_else(|| format!("{:.0}%", norm * 100.0)),
        RemoteTarget::OpenPolyCutoff { .. } => {
            let hz = (20.0 * 1000.0_f32.powf(norm.clamp(0.0, 1.0))).clamp(20.0, 20000.0);
            if hz >= 1000.0 {
                format!("{:.2} kHz", hz / 1000.0)
            } else {
                format!("{:.0} Hz", hz)
            }
        }
        RemoteTarget::OpenPolyReso { .. }
        | RemoteTarget::OpenPolyDrive { .. } => format!("{:.0}%", norm * 100.0),
    }
}

fn truncate_label(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

// =============================================================================
// Custom Bitwig-faithful vector icons & rotary knob painter
// =============================================================================

/// Draws the signature Bitwig cyan wireless/remote arcs icon (unassigned remote slot).
fn paint_remote_wireless_icon(painter: &Painter, center: Pos2, radius: f32, color: Color32) {
    let base_y = center.y + radius * 0.42;
    let origin = pos2(center.x, base_y);
    // Small dot at bottom
    painter.circle_filled(origin, (radius * 0.14).max(1.2), color);
    // 3 concentric upward arcs (-135° to -45°)
    for &scale in &[0.44_f32, 0.74_f32, 1.04_f32] {
        let r = radius * scale;
        let steps = 10;
        let a0 = -std::f32::consts::FRAC_PI_4 * 3.0; // -135 deg
        let a1 = -std::f32::consts::FRAC_PI_4;       // -45 deg
        let pts: Vec<Pos2> = (0..=steps)
            .map(|i| {
                let t = i as f32 / steps as f32;
                let a = a0 + (a1 - a0) * t;
                pos2(origin.x + r * a.cos(), origin.y + r * a.sin())
            })
            .collect();
        painter.add(egui::Shape::line(pts, Stroke::new(1.4_f32, color)));
    }
}

/// Draws the small Bitwig orange knob-dot toggle icon on the vertical strip.
fn paint_remote_toggle_icon(painter: &Painter, center: Pos2, active: bool, hovered: bool) {
    let r = 5.0_f32;
    let ring_col = if active {
        REMOTE_ORANGE
    } else if hovered {
        Color32::from_rgb(0xD0, 0xD4, 0xDC)
    } else {
        Color32::from_rgb(0x76, 0x7A, 0x84)
    };
    let fill_col = if active {
        REMOTE_ORANGE
    } else {
        Color32::from_rgb(0x38, 0x3B, 0x42)
    };
    painter.circle_filled(center, r, fill_col);
    painter.circle_stroke(center, r, Stroke::new(1.1_f32, ring_col));
    // Vertical tick from center upwards (like Bitwig's remote icon)
    let tick_col = if active {
        Color32::from_rgb(0x1A, 0x14, 0x10)
    } else {
        ring_col
    };
    painter.line_segment(
        [center, pos2(center.x, center.y - r + 1.0)],
        Stroke::new(1.3_f32, tick_col),
    );
}

/// Draws the small Bitwig `o->` modulator toggle icon on the vertical strip.
fn paint_mod_toggle_icon(painter: &Painter, center: Pos2, active: bool, hovered: bool) {
    let col = if active {
        REMOTE_CYAN
    } else if hovered {
        Color32::from_rgb(0xD6, 0xDA, 0xE2)
    } else {
        Color32::from_rgb(0x88, 0x8C, 0x96)
    };
    let left_dot = pos2(center.x - 4.0, center.y);
    let right_tip = pos2(center.x + 5.0, center.y);
    painter.circle_stroke(left_dot, 2.1, Stroke::new(1.2_f32, col));
    painter.line_segment([pos2(left_dot.x + 2.1, center.y), right_tip], Stroke::new(1.2_f32, col));
    painter.line_segment([right_tip, pos2(right_tip.x - 2.6, center.y - 2.2)], Stroke::new(1.2_f32, col));
    painter.line_segment([right_tip, pos2(right_tip.x - 2.6, center.y + 2.2)], Stroke::new(1.2_f32, col));
}

/// Draws a Bitwig-style compact rotary knob for an assigned Remote Control slot.
fn paint_remote_knob(painter: &Painter, center: Pos2, radius: f32, norm_val: f32, accent: Color32) {
    let norm = norm_val.clamp(0.0, 1.0);
    // Knob body
    painter.circle_filled(center, radius - 2.5, Color32::from_rgb(0x2E, 0x31, 0x38));
    painter.circle_stroke(
        center,
        radius - 2.5,
        Stroke::new(1.0_f32, Color32::from_rgb(0x16, 0x18, 0x1C)),
    );

    // Arc range: 135° (bottom-left) clockwise 270° to 405° (bottom-right)
    let start_a = std::f32::consts::FRAC_PI_4 * 3.0; // 135°
    let sweep = std::f32::consts::PI * 1.5;          // 270°

    // Background track arc
    let bg_pts: Vec<Pos2> = (0..=20)
        .map(|i| {
            let a = start_a + sweep * (i as f32 / 20.0);
            pos2(center.x + radius * a.cos(), center.y + radius * a.sin())
        })
        .collect();
    painter.add(egui::Shape::line(
        bg_pts,
        Stroke::new(2.0_f32, Color32::from_rgb(0x46, 0x49, 0x52)),
    ));

    // Active orange value arc
    let val_steps = ((norm * 20.0).ceil() as usize).max(1);
    if norm > 0.005 {
        let fg_pts: Vec<Pos2> = (0..=val_steps)
            .map(|i| {
                let t = (i as f32 / val_steps as f32) * norm;
                let a = start_a + sweep * t;
                pos2(center.x + radius * a.cos(), center.y + radius * a.sin())
            })
            .collect();
        painter.add(egui::Shape::line(fg_pts, Stroke::new(2.2_f32, accent)));
    }

    // Indicator pointer line
    let needle_a = start_a + sweep * norm;
    let p_in = pos2(
        center.x + (radius * 0.25) * needle_a.cos(),
        center.y + (radius * 0.25) * needle_a.sin(),
    );
    let p_out = pos2(
        center.x + (radius - 3.0) * needle_a.cos(),
        center.y + (radius - 3.0) * needle_a.sin(),
    );
    painter.line_segment([p_in, p_out], Stroke::new(1.6_f32, Color32::from_rgb(0xE8, 0xEC, 0xF2)));
}

fn paint_vertical_strip_label(
    painter: &Painter,
    center_x: f32,
    bottom_y: f32,
    max_chars: usize,
    text: &str,
    font_size: f32,
    color: Color32,
) {
    let clipped = truncate_label(text, max_chars);
    let galley = painter.layout_no_wrap(clipped, fonts::semibold(font_size), color);
    let mut ts = egui::epaint::TextShape::new(
        pos2(center_x - font_size * 0.56, bottom_y),
        galley,
        color,
    );
    ts.angle = -std::f32::consts::FRAC_PI_2;
    painter.add(egui::Shape::Text(ts));
}

// =============================================================================
// Shared 2×4 Remote Controls Grid Renderer
// =============================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
enum RemoteScope {
    Project,
    Track(usize),
}

fn render_remotes_body(
    ui: &mut Ui,
    rect: Rect,
    app: &mut AppState,
    scope: RemoteScope,
) {
    let p = ui.painter();
    p.rect_filled(rect, 2.0, Color32::from_rgb(0x2B, 0x2D, 0x33));

    // Header (Title + "Perform" + vector caret page dropdown)
    let header_h = 44.0;
    let header_rect = Rect::from_min_size(rect.min, vec2(rect.width(), header_h));
    p.rect_filled(header_rect, 2.0, Color32::from_rgb(0x36, 0x39, 0x41));
    p.line_segment(
        [
            pos2(header_rect.min.x, header_rect.max.y),
            pos2(header_rect.max.x, header_rect.max.y),
        ],
        Stroke::new(1.0_f32, Color32::from_rgb(0x1C, 0x1E, 0x22)),
    );

    let title = match scope {
        RemoteScope::Project => "Project Remotes",
        RemoteScope::Track(_) => "Track Remotes",
    };
    p.text(
        pos2(header_rect.min.x + 10.0, header_rect.min.y + 13.0),
        Align2::LEFT_CENTER,
        title,
        fonts::semibold(11.0),
        Color32::from_rgb(0x9E, 0xA2, 0xAC),
    );

    // Page selector row ("● Perform ▾")
    let (active_page_idx, page_names) = match scope {
        RemoteScope::Project => {
            let idx = app.project_remotes.ensure_valid_page();
            let names: Vec<String> = app.project_remotes.pages.iter().map(|pg| pg.name.clone()).collect();
            (idx, names)
        }
        RemoteScope::Track(ti) => {
            if let Some(t) = app.current_tracks_mut().get_mut(ti) {
                let idx = t.remotes.ensure_valid_page();
                let names: Vec<String> = t.remotes.pages.iter().map(|pg| pg.name.clone()).collect();
                (idx, names)
            } else {
                (0, vec!["Perform".to_string()])
            }
        }
    };
    let page_name = page_names
        .get(active_page_idx)
        .cloned()
        .unwrap_or_else(|| "Perform".to_string());

    let page_btn_rect = Rect::from_min_size(
        pos2(header_rect.min.x + 6.0, header_rect.min.y + 23.0),
        vec2(header_rect.width() - 12.0, 17.0),
    );
    let page_salt = match scope {
        RemoteScope::Project => ("proj_remote_page", 0usize),
        RemoteScope::Track(ti) => ("trk_remote_page", ti),
    };
    let page_resp = ui.interact(page_btn_rect, ui.id().with(page_salt), Sense::click());
    if page_resp.hovered() {
        p.rect_filled(page_btn_rect, 2.0, Color32::from_rgb(0x42, 0x45, 0x4F));
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    // Small knob dot on the left of page name
    let dot_c = pos2(page_btn_rect.min.x + 7.0, page_btn_rect.center().y);
    p.circle_filled(dot_c, 3.6, Color32::from_rgb(0xA4, 0xAA, 0xB4));
    p.line_segment(
        [dot_c, pos2(dot_c.x, dot_c.y - 3.6)],
        Stroke::new(1.0_f32, Color32::from_rgb(0x28, 0x2A, 0x30)),
    );
    let galley = p.layout_no_wrap(
        page_name.clone(),
        fonts::regular(10.5),
        Color32::from_rgb(0xD2, 0xD6, 0xDE),
    );
    let text_w = galley.size().x;
    let text_pos = pos2(
        page_btn_rect.min.x + 16.0,
        page_btn_rect.center().y - galley.size().y * 0.5,
    );
    p.galley(text_pos, galley, Color32::from_rgb(0xD2, 0xD6, 0xDE));
    // Crisp vector caret instead of missing Unicode glyph
    let caret_rect = Rect::from_center_size(
        pos2(page_btn_rect.min.x + 16.0 + text_w + 7.0, page_btn_rect.center().y + 0.5),
        vec2(6.0, 4.0),
    );
    icons::paint_caret_down(p, caret_rect, Color32::from_rgb(0xB8, 0xBC, 0xC6));

    let page_popup_id = ui.id().with((page_salt, "popup"));
    if page_resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(page_popup_id));
    }
    egui::popup_below_widget(
        ui,
        page_popup_id,
        &page_resp,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(130.0);
            for (pi, pname) in page_names.iter().enumerate() {
                let selected = pi == active_page_idx;
                if ui.selectable_label(selected, pname).clicked() {
                    match scope {
                        RemoteScope::Project => app.project_remotes.active_page = pi,
                        RemoteScope::Track(ti) => {
                            if let Some(t) = app.current_tracks_mut().get_mut(ti) {
                                t.remotes.active_page = pi;
                            }
                        }
                    }
                    ui.close_menu();
                }
            }
            ui.separator();
            if ui.button("+ Add Page").clicked() {
                match scope {
                    RemoteScope::Project => {
                        app.project_remotes.add_page();
                    }
                    RemoteScope::Track(ti) => {
                        if let Some(t) = app.current_tracks_mut().get_mut(ti) {
                            t.remotes.add_page();
                        }
                    }
                }
                ui.close_menu();
            }
            if ui.button("Clear Current Page").clicked() {
                match scope {
                    RemoteScope::Project => app.project_remotes.active_page_mut().clear(),
                    RemoteScope::Track(ti) => {
                        if let Some(t) = app.current_tracks_mut().get_mut(ti) {
                            t.remotes.active_page_mut().clear();
                        }
                    }
                }
                ui.close_menu();
            }
        },
    );

    // 2×4 Recessed Grid Well
    let grid_rect = Rect::from_min_max(
        pos2(rect.min.x + 5.0, header_rect.max.y + 5.0),
        pos2(rect.max.x - 5.0, rect.max.y - 5.0),
    );
    p.rect_filled(grid_rect, 3.0, Color32::from_rgb(0x23, 0x25, 0x2A));
    p.rect_stroke(
        grid_rect,
        3.0,
        Stroke::new(1.0_f32, Color32::from_rgb(0x1B, 0x1D, 0x21)),
        egui::StrokeKind::Inside,
    );

    let cols = 4usize;
    let rows = 2usize;
    let cell_w = grid_rect.width() / cols as f32;
    let cell_h = grid_rect.height() / rows as f32;

    // Snapshot the 8 slots for rendering & interaction
    let slots_snapshot: [RemoteControlSlot; REMOTE_SLOTS_PER_PAGE] = match scope {
        RemoteScope::Project => app.project_remotes.active_page_ref().slots.clone(),
        RemoteScope::Track(ti) => app
            .current_tracks()
            .get(ti)
            .map(|t| t.remotes.active_page_ref().slots.clone())
            .unwrap_or_else(|| std::array::from_fn(|_| RemoteControlSlot::default())),
    };

    for slot_i in 0..REMOTE_SLOTS_PER_PAGE {
        let col = slot_i % cols;
        let row = slot_i / cols;
        let cell_rect = Rect::from_min_size(
            pos2(
                grid_rect.min.x + col as f32 * cell_w,
                grid_rect.min.y + row as f32 * cell_h,
            ),
            vec2(cell_w, cell_h),
        );
        let slot = &slots_snapshot[slot_i];
        let slot_id = ui.id().with((page_salt, "slot", active_page_idx, slot_i));
        let assign_popup_id = ui.id().with((page_salt, "assign_popup", active_page_idx, slot_i));

        if let Some(target) = slot.target.clone() {
            // Sync live value from target
            let live_norm = read_target_normalized(app, &target).unwrap_or(slot.value);
            let label = slot
                .custom_label
                .clone()
                .unwrap_or_else(|| target_short_label(app, &target));
            let val_text = format_target_value(app, &target, live_norm);

            let resp = ui.interact(cell_rect, slot_id, Sense::click_and_drag());
            let mut new_norm = live_norm;
            let mut changed = false;

            if resp.dragged() {
                let speed = if ui.input(|i| i.modifiers.shift) {
                    0.0012
                } else {
                    0.0055
                };
                let dy = -resp.drag_delta().y;
                if dy.abs() > 0.0 {
                    new_norm = (live_norm + dy * speed).clamp(0.0, 1.0);
                    changed = (new_norm - live_norm).abs() > 1e-6;
                }
            }
            if resp.double_clicked() {
                new_norm = slot.default_value;
                changed = true;
            }
            if changed {
                write_target_normalized(app, &target, new_norm);
                match scope {
                    RemoteScope::Project => {
                        app.project_remotes.active_page_mut().slots[slot_i].value = new_norm;
                    }
                    RemoteScope::Track(ti) => {
                        if let Some(t) = app.current_tracks_mut().get_mut(ti) {
                            t.remotes.active_page_mut().slots[slot_i].value = new_norm;
                        }
                    }
                }
            }

            if resp.hovered() || resp.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }

            let knob_r = (cell_w.min(cell_h - 14.0) * 0.36).clamp(9.0, 14.0);
            let knob_c = pos2(cell_rect.center().x, cell_rect.min.y + cell_h * 0.42);
            paint_remote_knob(ui.painter(), knob_c, knob_r, new_norm, REMOTE_ORANGE);

            ui.painter().text(
                pos2(cell_rect.center().x, cell_rect.max.y - 8.0),
                Align2::CENTER_CENTER,
                truncate_label(&label, 7),
                fonts::regular(8.5),
                if resp.hovered() || resp.dragged() {
                    Color32::WHITE
                } else {
                    Color32::from_rgb(0xB8, 0xBC, 0xC6)
                },
            );

            resp.clone()
                .on_hover_text(format!("{}: {}\nDrag to adjust · Double-click to reset · Right-click to remap", label, val_text));

            resp.context_menu(|ui| {
                ui.label(RichText::new(format!("{}: {}", label, val_text)).strong());
                ui.separator();
                if ui.button("Reset to Default").clicked() {
                    write_target_normalized(app, &target, slot.default_value);
                    ui.close_menu();
                }
                if ui.button("Unmap Remote").clicked() {
                    match scope {
                        RemoteScope::Project => {
                            app.project_remotes.active_page_mut().slots[slot_i].unmap();
                        }
                        RemoteScope::Track(ti) => {
                            if let Some(t) = app.current_tracks_mut().get_mut(ti) {
                                t.remotes.active_page_mut().slots[slot_i].unmap();
                            }
                        }
                    }
                    ui.close_menu();
                }
            });
        } else {
            // Unassigned slot — Bitwig cyan wireless remote icon
            let resp = ui.interact(cell_rect, slot_id, Sense::click());
            let icon_col = if resp.hovered() {
                Color32::from_rgb(0x5E, 0xE2, 0xFF)
            } else {
                REMOTE_CYAN
            };
            if resp.hovered() {
                ui.painter()
                    .rect_filled(cell_rect.shrink(3.0), 3.0, Color32::from_rgb(0x2D, 0x31, 0x3A));
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            paint_remote_wireless_icon(ui.painter(), cell_rect.center(), 9.0, icon_col);

            if resp
                .clone()
                .on_hover_text("Click to assign Remote Control parameter")
                .clicked()
            {
                ui.memory_mut(|m| m.toggle_popup(assign_popup_id));
            }

            egui::popup_below_widget(
                ui,
                assign_popup_id,
                &resp,
                egui::PopupCloseBehavior::CloseOnClickOutside,
                |ui| {
                    render_assign_target_menu(ui, app, scope, slot_i);
                },
            );
        }
    }
}

fn assign_slot(app: &mut AppState, scope: RemoteScope, slot_i: usize, target: RemoteTarget) {
    let cur = read_target_normalized(app, &target).unwrap_or_else(|| default_norm_for_target(&target));
    let def = default_norm_for_target(&target);
    match scope {
        RemoteScope::Project => {
            app.project_remotes.active_page_mut().slots[slot_i].assign(target, cur, def);
        }
        RemoteScope::Track(ti) => {
            if let Some(t) = app.current_tracks_mut().get_mut(ti) {
                t.remotes.active_page_mut().slots[slot_i].assign(target, cur, def);
            }
        }
    }
}

fn render_assign_target_menu(ui: &mut Ui, app: &mut AppState, scope: RemoteScope, slot_i: usize) {
    ui.set_min_width(180.0);
    let mut chosen: Option<RemoteTarget> = None;

    match scope {
        RemoteScope::Project => {
            ui.label(RichText::new("Assign Project Remote").strong().size(11.0));
            ui.separator();
            if ui.button("⏱ Project Tempo (BPM)").clicked() {
                chosen = Some(RemoteTarget::ProjectTempo);
            }
            if ui.button("🔊 Master Volume").clicked() {
                chosen = Some(RemoteTarget::MasterVolume);
            }
            if ui.button("🎚 Master Pan").clicked() {
                chosen = Some(RemoteTarget::MasterPan);
            }
            ui.separator();

            // Snapshot track names & devices to avoid borrowing `app` mutably inside closures
            let track_summaries: Vec<(usize, String, Vec<(usize, String, bool, Vec<(usize, String)>)>)> = app
                .current_tracks()
                .iter()
                .enumerate()
                .skip(1)
                .map(|(ti, tr)| {
                    let devs = tr
                        .effects
                        .iter()
                        .enumerate()
                        .map(|(si, sl)| {
                            let params: Vec<(usize, String)> = sl
                                .cached_params
                                .iter()
                                .enumerate()
                                .take(16)
                                .map(|(pi, p)| (pi, p.info.name.clone()))
                                .collect();
                            (si, sl.name.clone(), sl.is_open_poly(), params)
                        })
                        .collect();
                    (ti, tr.name.clone(), devs)
                })
                .collect();

            for (ti, tname, devs) in track_summaries {
                ui.menu_button(format!("Track: {}", tname), |ui| {
                    if ui.button("Volume").clicked() {
                        chosen = Some(RemoteTarget::TrackVolume { track_idx: ti });
                        ui.close_menu();
                    }
                    if ui.button("Pan").clicked() {
                        chosen = Some(RemoteTarget::TrackPan { track_idx: ti });
                        ui.close_menu();
                    }
                    for (si, dname, is_poly, params) in &devs {
                        if *is_poly {
                            ui.menu_button(dname, |ui| {
                                if ui.button("Filter Cutoff").clicked() {
                                    chosen = Some(RemoteTarget::OpenPolyCutoff {
                                        track_idx: ti,
                                        slot_idx: *si,
                                    });
                                    ui.close_menu();
                                }
                                if ui.button("Filter Resonance").clicked() {
                                    chosen = Some(RemoteTarget::OpenPolyReso {
                                        track_idx: ti,
                                        slot_idx: *si,
                                    });
                                    ui.close_menu();
                                }
                                if ui.button("Filter Drive").clicked() {
                                    chosen = Some(RemoteTarget::OpenPolyDrive {
                                        track_idx: ti,
                                        slot_idx: *si,
                                    });
                                    ui.close_menu();
                                }
                            });
                        } else if !params.is_empty() {
                            ui.menu_button(dname, |ui| {
                                for (pi, pname) in params {
                                    if ui.button(pname).clicked() {
                                        chosen = Some(RemoteTarget::DeviceParam {
                                            track_idx: ti,
                                            slot_idx: *si,
                                            param_idx: *pi,
                                        });
                                        ui.close_menu();
                                    }
                                }
                            });
                        }
                    }
                });
            }
        }
        RemoteScope::Track(track_idx) => {
            let track_name = app
                .current_tracks()
                .get(track_idx)
                .map(|t| t.name.clone())
                .unwrap_or_else(|| "Track".to_string());
            ui.label(
                RichText::new(format!("Assign {} Remote", track_name))
                    .strong()
                    .size(11.0),
            );
            ui.separator();
            if track_idx == 0 {
                if ui.button("⏱ Project Tempo (BPM)").clicked() {
                    chosen = Some(RemoteTarget::ProjectTempo);
                }
                if ui.button("🔊 Master Volume").clicked() {
                    chosen = Some(RemoteTarget::MasterVolume);
                }
                if ui.button("🎚 Master Pan").clicked() {
                    chosen = Some(RemoteTarget::MasterPan);
                }
            } else {
                if ui.button("🔊 Track Volume").clicked() {
                    chosen = Some(RemoteTarget::TrackVolume { track_idx });
                }
                if ui.button("🎚 Track Pan").clicked() {
                    chosen = Some(RemoteTarget::TrackPan { track_idx });
                }
                let fx_names: Vec<String> = app.fx_tracks.iter().map(|f| f.name.clone()).collect();
                if !fx_names.is_empty() {
                    ui.separator();
                    for (fx_idx, fx_name) in fx_names.iter().enumerate() {
                        if ui.button(format!("↗ Send to {}", fx_name)).clicked() {
                            chosen = Some(RemoteTarget::TrackSend { track_idx, fx_idx });
                        }
                    }
                }
            }

            let devs: Vec<(usize, String, bool, Vec<(usize, String)>)> = app
                .current_tracks()
                .get(track_idx)
                .map(|tr| {
                    tr.effects
                        .iter()
                        .enumerate()
                        .map(|(si, sl)| {
                            let params: Vec<(usize, String)> = sl
                                .cached_params
                                .iter()
                                .enumerate()
                                .take(20)
                                .map(|(pi, p)| (pi, p.info.name.clone()))
                                .collect();
                            (si, sl.name.clone(), sl.is_open_poly(), params)
                        })
                        .collect()
                })
                .unwrap_or_default();

            if !devs.is_empty() {
                ui.separator();
                for (si, dname, is_poly, params) in devs {
                    if is_poly {
                        ui.menu_button(dname, |ui| {
                            if ui.button("Filter Cutoff").clicked() {
                                chosen = Some(RemoteTarget::OpenPolyCutoff {
                                    track_idx,
                                    slot_idx: si,
                                });
                                ui.close_menu();
                            }
                            if ui.button("Filter Resonance").clicked() {
                                chosen = Some(RemoteTarget::OpenPolyReso {
                                    track_idx,
                                    slot_idx: si,
                                });
                                ui.close_menu();
                            }
                            if ui.button("Filter Drive").clicked() {
                                chosen = Some(RemoteTarget::OpenPolyDrive {
                                    track_idx,
                                    slot_idx: si,
                                });
                                ui.close_menu();
                            }
                        });
                    } else if !params.is_empty() {
                        ui.menu_button(dname, |ui| {
                            for (pi, pname) in params {
                                if ui.button(pname).clicked() {
                                    chosen = Some(RemoteTarget::DeviceParam {
                                        track_idx,
                                        slot_idx: si,
                                        param_idx: pi,
                                    });
                                    ui.close_menu();
                                }
                            }
                        });
                    }
                }
            }
        }
    }

    if let Some(target) = chosen {
        assign_slot(app, scope, slot_i, target);
        ui.close_menu();
    }
}

// =============================================================================
// Public Panel Renderers: PROJECT Panel & Selected TRACK Panel
// =============================================================================

/// Renders the Bitwig-style **`PROJECT`** panel (vertical strip + collapsible
/// `Project Remotes` 2×4 grid + collapsible `Project Modulators` bay).
pub fn render_project_panel(ui: &mut Ui, app: &mut AppState, card_h: f32) {
    let is_project_focused = app.safe_track_index() == 0;
    let remotes_open = app.project_remotes.remotes_open;
    let mods_open = app.project_remotes.modulators_open;
    let mods_w = if mods_open {
        hikaru_openmodulation::bay_width_for_slot(0, PROJECT_MODULATOR_SLOT)
    } else {
        0.0
    };
    let total_w = STRIP_WIDTH
        + if remotes_open { REMOTES_BODY_WIDTH } else { 0.0 }
        + mods_w;

    let (card_rect, _) = ui.allocate_exact_size(vec2(total_w, card_h), Sense::hover());
    if !ui.is_rect_visible(card_rect) {
        return;
    }

    let p = ui.painter();
    p.rect_filled(card_rect, 3.0, Color32::from_rgb(0x2B, 0x2D, 0x33));
    p.rect_stroke(
        card_rect,
        3.0,
        Stroke::new(1.0_f32, Color32::from_rgb(0x46, 0x49, 0x52)),
        egui::StrokeKind::Inside,
    );
    // Bitwig neutral grey top & bottom header bars on PROJECT panel
    let proj_bar_col = Color32::from_rgb(0x6E, 0x72, 0x7A);
    p.rect_filled(
        Rect::from_min_size(card_rect.min, vec2(card_rect.width(), 3.0)),
        1.0,
        proj_bar_col,
    );
    p.rect_filled(
        Rect::from_min_size(
            pos2(card_rect.min.x, card_rect.max.y - 3.0),
            vec2(card_rect.width(), 3.0),
        ),
        1.0,
        proj_bar_col,
    );

    // Left vertical strip (`PROJECT`)
    let strip_rect = Rect::from_min_max(
        pos2(card_rect.min.x, card_rect.min.y + 3.0),
        pos2(card_rect.min.x + STRIP_WIDTH, card_rect.max.y - 3.0),
    );
    p.rect_filled(strip_rect, 0.0, Color32::from_rgb(0x32, 0x34, 0x3B));
    p.line_segment(
        [
            pos2(strip_rect.max.x, strip_rect.min.y),
            pos2(strip_rect.max.x, strip_rect.max.y),
        ],
        Stroke::new(1.0_f32, Color32::from_rgb(0x1C, 0x1E, 0x22)),
    );

    // Crown icon at top
    icons::paint_crown(
        p,
        Rect::from_center_size(pos2(strip_rect.center().x, strip_rect.min.y + 14.0), vec2(12.0, 10.0)),
        if is_project_focused {
            REMOTE_ORANGE
        } else {
            Color32::from_rgb(0xB8, 0xBC, 0xC6)
        },
    );

    // Clickable label region selects Master/Project track
    let label_rect = Rect::from_min_max(
        pos2(strip_rect.min.x, strip_rect.min.y + 24.0),
        pos2(strip_rect.max.x, strip_rect.max.y - 36.0),
    );
    let label_resp = ui
        .interact(label_rect, ui.id().with("bw_proj_strip_label"), Sense::click())
        .on_hover_text("Select Project / Master Track");
    if label_resp.clicked() {
        app.selected_track_index = 0;
    }
    paint_vertical_strip_label(
        ui.painter(),
        strip_rect.center().x,
        label_rect.center().y + 22.0,
        10,
        "PROJECT",
        8.5,
        if is_project_focused {
            Color32::WHITE
        } else {
            Color32::from_rgb(0xBC, 0xC0, 0xCA)
        },
    );

    // Bottom toggle 1: Project Remotes
    let rem_btn_r = Rect::from_center_size(
        pos2(strip_rect.center().x, strip_rect.max.y - 25.0),
        vec2(16.0, 14.0),
    );
    let rem_resp = ui
        .interact(rem_btn_r, ui.id().with("bw_proj_rem_toggle"), Sense::click())
        .on_hover_text("Toggle Project Remotes");
    paint_remote_toggle_icon(ui.painter(), rem_btn_r.center(), remotes_open, rem_resp.hovered());
    if rem_resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if rem_resp.clicked() {
        app.project_remotes.remotes_open = !app.project_remotes.remotes_open;
    }

    // Bottom toggle 2: Project Modulators
    let mod_btn_r = Rect::from_center_size(
        pos2(strip_rect.center().x, strip_rect.max.y - 10.0),
        vec2(16.0, 14.0),
    );
    let mod_resp = ui
        .interact(mod_btn_r, ui.id().with("bw_proj_mod_toggle"), Sense::click())
        .on_hover_text("Toggle Project Modulators");
    paint_mod_toggle_icon(ui.painter(), mod_btn_r.center(), mods_open, mod_resp.hovered());
    if mod_resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if mod_resp.clicked() {
        app.project_remotes.modulators_open = !app.project_remotes.modulators_open;
    }

    // Expanded body: Project Remotes (2×4)
    let mut cursor_x = strip_rect.max.x;
    if remotes_open {
        let body_rect = Rect::from_min_size(
            pos2(cursor_x, card_rect.min.y + 3.0),
            vec2(REMOTES_BODY_WIDTH - 2.0, card_h - 6.0),
        );
        render_remotes_body(ui, body_rect, app, RemoteScope::Project);
        cursor_x += REMOTES_BODY_WIDTH;
    }

    // Expanded body: Project Modulators bay
    if mods_open {
        let mut request_add_modulator = false;
        hikaru_openmodulation::render_device_modulator_bay(
            ui,
            pos2(cursor_x, card_rect.min.y + 3.0),
            card_h - 6.0,
            0,
            PROJECT_MODULATOR_SLOT,
            &mut request_add_modulator,
        );
        if request_add_modulator {
            app.popup_browser
                .open_insert_modulator(0, PROJECT_MODULATOR_SLOT);
        }
    }
}

/// Renders the Bitwig-style selected **`<TRACK>`** panel (e.g. `AUDIO 1` / `TRACK 1` / `MASTER`)
/// with orange active border, vertical strip, collapsible `Track Remotes` 2×4 grid, and
/// collapsible `Track Modulators` bay.
pub fn render_track_panel(
    ui: &mut Ui,
    app: &mut AppState,
    track_idx: usize,
    card_h: f32,
    on_save_chain: &mut bool,
    on_load_chain: &mut bool,
) {
    let (track_name, track_color, is_master, is_group, remotes_open, mods_open) = app
        .current_tracks()
        .get(track_idx)
        .map(|t| {
            (
                t.name.clone(),
                t.color,
                t.is_master,
                t.is_group,
                t.remotes.remotes_open,
                t.remotes.modulators_open,
            )
        })
        .unwrap_or_else(|| {
            (
                "Track 1".to_string(),
                theme::ACCENT_ORANGE,
                false,
                false,
                true,
                false,
            )
        });

    let mods_w = if mods_open {
        hikaru_openmodulation::bay_width_for_slot(track_idx, TRACK_MODULATOR_SLOT)
    } else {
        0.0
    };
    let total_w = STRIP_WIDTH
        + if remotes_open { REMOTES_BODY_WIDTH } else { 0.0 }
        + mods_w;

    let (card_rect, _) = ui.allocate_exact_size(vec2(total_w, card_h), Sense::hover());
    if !ui.is_rect_visible(card_rect) {
        return;
    }

    let accent = if is_master {
        REMOTE_ORANGE
    } else {
        track_color
    };

    let p = ui.painter();
    p.rect_filled(card_rect, 3.0, Color32::from_rgb(0x2B, 0x2D, 0x33));
    p.rect_stroke(
        card_rect,
        3.0,
        Stroke::new(1.0_f32, Color32::from_rgb(0x4A, 0x4D, 0x56)),
        egui::StrokeKind::Inside,
    );
    // Bitwig signature top & bottom orange/track-color accent bars on the selected track panel
    p.rect_filled(
        Rect::from_min_size(card_rect.min, vec2(card_rect.width(), 3.0)),
        1.0,
        accent,
    );
    p.rect_filled(
        Rect::from_min_size(
            pos2(card_rect.min.x, card_rect.max.y - 3.0),
            vec2(card_rect.width(), 3.0),
        ),
        1.0,
        accent,
    );

    // Left vertical strip (`AUDIO 1` / `TRACK 1` / etc.)
    let strip_rect = Rect::from_min_max(
        pos2(card_rect.min.x, card_rect.min.y + 3.0),
        pos2(card_rect.min.x + STRIP_WIDTH, card_rect.max.y - 3.0),
    );
    p.rect_filled(strip_rect, 0.0, Color32::from_rgb(0x32, 0x34, 0x3B));
    p.line_segment(
        [
            pos2(strip_rect.max.x, strip_rect.min.y),
            pos2(strip_rect.max.x, strip_rect.max.y),
        ],
        Stroke::new(1.0_f32, Color32::from_rgb(0x1C, 0x1E, 0x22)),
    );

    // Track type icon at top
    let icon_rect = Rect::from_center_size(
        pos2(strip_rect.center().x, strip_rect.min.y + 12.0),
        vec2(12.0, 10.0),
    );
    let icon_col = Color32::from_rgb(0xC8, 0xCC, 0xD4);
    if is_master {
        icons::paint_crown(p, icon_rect, icon_col);
    } else if is_group {
        icons::paint_icon(p, icon_rect, icons::FOLDER, icon_col, 1.4);
    } else {
        icons::paint_icon(p, icon_rect, icons::WAVE_SINE, icon_col, 1.4);
    }

    // Track label region + context menu for .hdp save/load
    let label_rect = Rect::from_min_max(
        pos2(strip_rect.min.x, strip_rect.min.y + 22.0),
        pos2(strip_rect.max.x, strip_rect.max.y - 34.0),
    );
    let label_resp = ui
        .interact(
            label_rect,
            ui.id().with(("bw_trk_strip_label", track_idx)),
            Sense::click(),
        )
        .on_hover_text(format!("{} (Right-click for Device Chain .hdp)", track_name));
    label_resp.context_menu(|ui| {
        if ui.button("💾 Guardar Cadena de Pista (*.hdp)...").clicked() {
            *on_save_chain = true;
            ui.close_menu();
        }
        if ui.button("📂 Cargar Cadena de Pista (*.hdp)...").clicked() {
            *on_load_chain = true;
            ui.close_menu();
        }
    });

    let upper_name = track_name.to_uppercase();
    paint_vertical_strip_label(
        ui.painter(),
        strip_rect.center().x,
        label_rect.center().y + 22.0,
        12,
        &upper_name,
        8.5,
        Color32::from_rgb(0xE2, 0xE6, 0xEE),
    );

    // Bottom toggle 1: Track Remotes
    let rem_btn_r = Rect::from_center_size(
        pos2(strip_rect.center().x, strip_rect.max.y - 23.0),
        vec2(16.0, 14.0),
    );
    let rem_resp = ui
        .interact(
            rem_btn_r,
            ui.id().with(("bw_trk_rem_toggle", track_idx)),
            Sense::click(),
        )
        .on_hover_text("Toggle Track Remotes");
    paint_remote_toggle_icon(ui.painter(), rem_btn_r.center(), remotes_open, rem_resp.hovered());
    if rem_resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if rem_resp.clicked() {
        if let Some(t) = app.current_tracks_mut().get_mut(track_idx) {
            t.remotes.remotes_open = !t.remotes.remotes_open;
        }
    }

    // Bottom toggle 2: Track Modulators
    let mod_btn_r = Rect::from_center_size(
        pos2(strip_rect.center().x, strip_rect.max.y - 8.0),
        vec2(16.0, 14.0),
    );
    let mod_resp = ui
        .interact(
            mod_btn_r,
            ui.id().with(("bw_trk_mod_toggle", track_idx)),
            Sense::click(),
        )
        .on_hover_text("Toggle Track Modulators");
    paint_mod_toggle_icon(ui.painter(), mod_btn_r.center(), mods_open, mod_resp.hovered());
    if mod_resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if mod_resp.clicked() {
        if let Some(t) = app.current_tracks_mut().get_mut(track_idx) {
            t.remotes.modulators_open = !t.remotes.modulators_open;
        }
    }

    // Expanded body: Track Remotes (2×4)
    let mut cursor_x = strip_rect.max.x;
    if remotes_open {
        let body_rect = Rect::from_min_size(
            pos2(cursor_x, card_rect.min.y + 3.0),
            vec2(REMOTES_BODY_WIDTH - 2.0, card_h - 6.0),
        );
        render_remotes_body(ui, body_rect, app, RemoteScope::Track(track_idx));
        cursor_x += REMOTES_BODY_WIDTH;
    }

    // Expanded body: Track Modulators bay
    if mods_open {
        let mut request_add_modulator = false;
        hikaru_openmodulation::render_device_modulator_bay(
            ui,
            pos2(cursor_x, card_rect.min.y + 3.0),
            card_h - 6.0,
            track_idx,
            TRACK_MODULATOR_SLOT,
            &mut request_add_modulator,
        );
        if request_add_modulator {
            app.popup_browser
                .open_insert_modulator(track_idx, TRACK_MODULATOR_SLOT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_remote_controls_has_perform_page_with_eight_slots() {
        let state = RemoteControlsState::default();
        assert_eq!(state.pages.len(), 1);
        assert_eq!(state.pages[0].name, "Perform");
        assert_eq!(state.pages[0].slots.len(), REMOTE_SLOTS_PER_PAGE);
        assert!(!state.remotes_open);
        assert!(!state.modulators_open);
        assert!(state.pages[0].slots.iter().all(|s| s.target.is_none()));
    }

    #[test]
    fn remote_pages_add_clear_and_remove() {
        let mut state = RemoteControlsState::default();
        state.active_page_mut().slots[0].assign(RemoteTarget::ProjectTempo, 0.5, 0.3);
        assert_eq!(
            state.active_page_ref().slots[0].target,
            Some(RemoteTarget::ProjectTempo)
        );

        let p2 = state.add_page();
        assert_eq!(p2, 1);
        assert_eq!(state.active_page_ref().name, "Page 2");
        assert!(state.active_page_ref().slots[0].target.is_none());

        state.remove_active_page();
        assert_eq!(state.pages.len(), 1);
        assert_eq!(state.active_page, 0);

        state.active_page_mut().clear();
        assert!(state.active_page_ref().slots[0].target.is_none());
    }

    #[test]
    fn tempo_normalization_roundtrip() {
        for bpm in [BPM_MIN, 90.0, 120.0, 145.0, 174.0, BPM_MAX] {
            let norm = bpm_to_norm(bpm);
            let back = norm_to_bpm(norm);
            assert!((back - bpm).abs() < 1e-3, "bpm {bpm} -> {norm} -> {back}");
        }
    }
}
