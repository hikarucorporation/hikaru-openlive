// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_openwavetable/open_wavetable.rs

use hikaru_render::{Camera, RenderMode, RenderSettings};

use crate::app::AppState;
use crate::render::{
    ViewerRequest, WavetableView, WavetableViewRequest, WAVETABLE_VIEWPORT,
};
use crate::theme;
use super::wavetable_io::{self, Wavetable};

pub const MODULE_SIZE: f32 = 178.0;
pub const EDITOR_WIDTH: f32 = 440.0;
pub const EDITOR_HEIGHT: f32 = 178.0;

const MESH_WIDTH: f32 = 260.0;
const MESH_HEIGHT: f32 = 96.0;
const MESH_THICKNESS: f32 = 240.0;
pub(crate) const MAX_FRAMES: usize = 64;

pub fn load_table_file(
    path: &std::path::Path,
) -> Result<wavetable_io::Wavetable, wavetable_io::WavetableError> {
    wavetable_io::load_wavetable(path, MAX_FRAMES)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KnobTarget {
    #[default]
    Position,
    Unison,
    Detune,
    Phase,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VoiceMode {
    Mono,
    Legato,
    #[default]
    Poly,
}

impl VoiceMode {
    pub fn label(self) -> &'static str {
        match self {
            VoiceMode::Mono => "Mono",
            VoiceMode::Legato => "Legato",
            VoiceMode::Poly => "Poly",
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            VoiceMode::Mono => VoiceMode::Legato,
            VoiceMode::Legato => VoiceMode::Poly,
            VoiceMode::Poly => VoiceMode::Mono,
        }
    }
}

#[derive(Clone, Debug)]
pub struct WavetableEditor {
    pub table: Wavetable,
    pub morph: f32,
    pub frame: usize,
    pub smooth: bool,
    pub render_mode: RenderMode,
    pub render_settings: RenderSettings,
    pub settings_open: bool,
    setting_drag: Option<SettingDrag>,
    panel_offset: (f32, f32),
    panel_drag: Option<PanelDrag>,
    viewport_size: (f32, f32),
    pub camera: Camera,
    pub unison: u8,
    pub detune: f32,
    pub phase: f32,
    pub pitch: i8,
    pub octave: i8,
    pub voices: VoiceMode,
    pub drag_target: KnobTarget,
    pub menu_open: bool,
    pub dragging: bool,
    drag_grab: f32,
    drag_origin_y: f32,
}

impl Default for WavetableEditor {
    fn default() -> Self {
        Self {
            table: wavetable_io::bundled_wavetable(MAX_FRAMES)
                .unwrap_or_else(|_| Wavetable::default_table()),
            morph: 0.0,
            frame: 0,
            smooth: true,
            render_mode: RenderMode::default(),
            render_settings: RenderSettings::default(),
            settings_open: false,
            setting_drag: None,
            panel_offset: (0.0, 0.0),
            panel_drag: None,
            viewport_size: (0.0, 0.0),
            camera: Camera::default(),
            unison: 1,
            detune: 0.0,
            phase: 0.0,
            pitch: 0,
            octave: 0,
            voices: VoiceMode::Poly,
            drag_target: KnobTarget::Position,
            menu_open: false,
            dragging: false,
            drag_grab: 0.0,
            drag_origin_y: 0.0,
        }
    }
}

impl WavetableEditor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_table(table: Wavetable) -> Self {
        Self { table, ..Self::default() }
    }

    pub fn position(&self) -> f32 {
        let frames = self.table.selectable_frames();
        if frames <= 1 {
            return 0.0;
        }
        (self.active_cycle() / (frames - 1) as f32).clamp(0.0, 1.0)
    }

    pub fn set_position(&mut self, position: f32) {
        let frames = self.table.selectable_frames();
        if frames <= 1 {
            self.frame = 0;
            self.morph = 0.0;
            return;
        }

        let last = (frames - 1) as f32;
        let position = if position.is_finite() { position.clamp(0.0, 1.0) } else { 0.0 };
        let cycle = (position * last).clamp(0.0, last);

        self.frame = cycle.floor() as usize;
        self.morph = cycle - self.frame as f32;
    }

    pub fn begin_drag(&mut self, window_y: f32) {
        self.begin_drag_target(KnobTarget::Position, window_y);
    }

    pub fn begin_drag_target(&mut self, target: KnobTarget, window_y: f32) {
        self.drag_target = target;
        self.dragging = true;
        self.setting_drag = None;
        self.panel_drag = None;
        self.drag_grab = match target {
            KnobTarget::Position => self.position(),
            KnobTarget::Unison => self.norm_unison(),
            KnobTarget::Detune => self.detune,
            KnobTarget::Phase => self.phase,
        };
        self.drag_origin_y = if window_y.is_finite() { window_y } else { 0.0 };
    }

    pub fn drag_to(&mut self, window_y: f32, fine: bool) -> f32 {
        if !self.dragging || !window_y.is_finite() {
            return self.position();
        }

        let delta_px = self.drag_origin_y - window_y;
        let mut value = self.drag_grab + delta_px / DRAG_RANGE_PX * value_gain(fine);

        if !value.is_finite() {
            value = self.drag_grab;
        }
        match self.drag_target {
            KnobTarget::Position => {
                self.set_position(value);
                self.position()
            }
            KnobTarget::Unison => {
                self.set_unison(1 + (value.clamp(0.0, 1.0) * 15.0).round() as u8);
                self.norm_unison()
            }
            KnobTarget::Detune => {
                self.detune = value.clamp(0.0, 1.0);
                self.detune
            }
            KnobTarget::Phase => {
                self.phase = value.clamp(0.0, 1.0);
                self.phase
            }
        }
    }

    pub fn norm_unison(&self) -> f32 {
        (self.unison.clamp(1, 16) as f32 - 1.0) / 15.0
    }

    pub fn set_unison(&mut self, voices: u8) {
        self.unison = voices.clamp(1, 16);
    }

    pub fn step_unison(&mut self, delta: i32) {
        self.set_unison((self.unison as i32 + delta).clamp(1, 16) as u8);
    }

    pub fn step_pitch(&mut self, delta: i32) {
        self.pitch = (self.pitch as i32 + delta).clamp(-24, 24) as i8;
    }

    pub fn step_octave(&mut self, delta: i32) {
        self.octave = (self.octave as i32 + delta).clamp(-3, 3) as i8;
    }

    pub fn cycle_voices(&mut self) {
        self.voices = self.voices.cycle();
    }

    pub fn end_drag(&mut self) {
        self.dragging = false;
        self.setting_drag = None;
        self.panel_drag = None;
    }

    fn measured_viewport(&self) -> (f32, f32) {
        let (w, h) = self.viewport_size;
        (
            if w.is_finite() && w > 0.0 { w } else { FALLBACK_VIEWPORT_W },
            if h.is_finite() && h > 0.0 { h } else { FALLBACK_VIEWPORT_H },
        )
    }

    fn panel_offset_range(&self) -> ((f32, f32), (f32, f32)) {
        let (view_w, view_h) = self.measured_viewport();
        let right_min = -(PANEL_WIDTH - PANEL_GRAB_STRIP);
        let right_max = (view_w - PANEL_GRAB_STRIP).max(right_min);
        let top_max = (view_h - PANEL_GRIP_H).max(0.0);
        (
            (PANEL_DEFAULT_RIGHT - right_max, PANEL_DEFAULT_RIGHT - right_min),
            (0.0 - PANEL_DEFAULT_TOP, top_max - PANEL_DEFAULT_TOP),
        )
    }

    pub fn panel_position(&self) -> (f32, f32) {
        let ((ox_min, ox_max), (oy_min, oy_max)) = self.panel_offset_range();
        let ox = self.panel_offset.0.clamp(ox_min, ox_max);
        let oy = self.panel_offset.1.clamp(oy_min, oy_max);
        (PANEL_DEFAULT_RIGHT - ox, PANEL_DEFAULT_TOP + oy)
    }

    pub fn record_viewport_size(&mut self, width: f32, height: f32) -> bool {
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return false;
        }
        let size = (width.min(4096.0), height.min(4096.0));
        if size == self.viewport_size {
            return false;
        }
        self.viewport_size = size;
        true
    }

    pub fn begin_panel_drag(&mut self, window_x: f32, window_y: f32) {
        self.dragging = false;
        self.setting_drag = None;
        self.panel_drag = Some(PanelDrag {
            grab: self.panel_offset,
            origin: (
                if window_x.is_finite() { window_x } else { 0.0 },
                if window_y.is_finite() { window_y } else { 0.0 },
            ),
        });
    }

    pub fn drag_panel_to(&mut self, window_x: f32, window_y: f32) -> bool {
        let Some(drag) = self.panel_drag else {
            return false;
        };
        if !window_x.is_finite() || !window_y.is_finite() {
            return false;
        }
        let ((ox_min, ox_max), (oy_min, oy_max)) = self.panel_offset_range();
        let next = (
            (drag.grab.0 + (window_x - drag.origin.0)).clamp(ox_min, ox_max),
            (drag.grab.1 + (window_y - drag.origin.1)).clamp(oy_min, oy_max),
        );
        if next == self.panel_offset {
            return false;
        }
        self.panel_offset = next;
        true
    }

    pub fn begin_setting_drag(&mut self, param: SettingParam, window_x: f32) {
        self.dragging = false;
        self.panel_drag = None;
        self.setting_drag = Some(SettingDrag {
            param,
            grab: param.get(&self.render_settings),
            origin_x: if window_x.is_finite() { window_x } else { 0.0 },
        });
    }

    pub fn drag_setting_to(&mut self, window_x: f32, fine: bool) -> f32 {
        let Some(drag) = self.setting_drag else {
            return 0.0;
        };
        if !window_x.is_finite() {
            return drag.grab;
        }
        let (min, max) = drag.param.range();
        let mut value =
            drag.grab + (window_x - drag.origin_x) / SETTING_DRAG_PX * (max - min) * value_gain(fine);
        if !value.is_finite() {
            value = drag.grab;
        }
        drag.param.set(&mut self.render_settings, value);
        drag.param.get(&self.render_settings)
    }

    pub fn setting_drag_value(&self) -> Option<f32> {
        self.setting_drag.map(|drag| drag.param.get(&self.render_settings))
    }

    pub fn frame_count(&self) -> usize {
        self.table.selectable_frames()
    }

    pub fn current_samples(&self) -> Vec<f32> {
        let position = self.active_cycle();
        self.table.frame(position, self.smooth)
    }

    pub fn active_cycle(&self) -> f32 {
        let frames = self.table.selectable_frames();
        if frames <= 1 {
            return 0.0;
        }
        ((self.frame as f32 + self.morph) / (frames - 1) as f32).clamp(0.0, 1.0)
            * (frames - 1) as f32
    }

    pub fn table_samples(&self) -> &[f32] {
        &self.table.samples
    }

    pub fn frame_len(&self) -> usize {
        wavetable_io::FRAME_SAMPLES
    }

    pub fn nudge_morph(&mut self, delta: f32) {
        let frames = self.table.selectable_frames();
        if frames <= 1 {
            return;
        }
        let last = (frames - 1) as f32;
        self.morph = (self.morph + delta * last).clamp(0.0, 1.0);
    }

    pub fn step_frame(&mut self, delta: i32) {
        let frames = self.table.selectable_frames();
        if frames <= 1 {
            return;
        }
        self.frame = (self.frame as i32 + delta).clamp(0, frames as i32 - 1) as usize;
    }

    pub fn load(&mut self, table: Wavetable) {
        self.table = table;
        self.morph = 0.0;
        self.frame = 0;
        self.end_drag();
    }
}

pub const DRAG_RANGE_PX: f32 = 160.0;
const FINE_TUNE_FACTOR: f32 = 0.25;

fn value_gain(fine: bool) -> f32 {
    if fine { FINE_TUNE_FACTOR } else { 1.0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingParam {
    LineWidth,
    LineWidth2d,
    DepthScale,
    Yaw,
    Pitch,
    DepthFade,
    AaFeather,
    Glow,
}

impl SettingParam {
    pub const ALL: [SettingParam; 8] = [
        SettingParam::LineWidth,
        SettingParam::LineWidth2d,
        SettingParam::DepthScale,
        SettingParam::Yaw,
        SettingParam::Pitch,
        SettingParam::DepthFade,
        SettingParam::AaFeather,
        SettingParam::Glow,
    ];

    pub fn title(self) -> &'static str {
        match self {
            SettingParam::LineWidth => "Trazo",
            SettingParam::LineWidth2d => "Tr.2D",
            SettingParam::DepthScale => "Prof.Z",
            SettingParam::Yaw => "Yaw",
            SettingParam::Pitch => "Pitch",
            SettingParam::DepthFade => "Fade",
            SettingParam::AaFeather => "Suav.",
            SettingParam::Glow => "Glow",
        }
    }

    pub fn range(self) -> (f32, f32) {
        match self {
            SettingParam::LineWidth => (1.0, 6.0),
            SettingParam::LineWidth2d => (1.0, 8.0),
            SettingParam::DepthScale => (0.3, 2.0),
            SettingParam::Yaw => (-30.0, 30.0),
            SettingParam::Pitch => (5.0, 60.0),
            SettingParam::DepthFade => (0.0, 0.8),
            SettingParam::AaFeather => (0.0, 2.0),
            SettingParam::Glow => (0.3, 2.0),
        }
    }

    pub fn get(self, settings: &RenderSettings) -> f32 {
        match self {
            SettingParam::LineWidth => settings.line_width,
            SettingParam::LineWidth2d => settings.line_width_2d,
            SettingParam::DepthScale => settings.depth_scale,
            SettingParam::Yaw => settings.yaw_deg,
            SettingParam::Pitch => settings.pitch_deg,
            SettingParam::DepthFade => settings.depth_fade,
            SettingParam::AaFeather => settings.aa_feather,
            SettingParam::Glow => settings.glow,
        }
    }

    pub fn set(self, settings: &mut RenderSettings, value: f32) {
        let (min, max) = self.range();
        let value = if value.is_finite() { value.clamp(min, max) } else { min };
        match self {
            SettingParam::LineWidth => settings.line_width = value,
            SettingParam::LineWidth2d => settings.line_width_2d = value,
            SettingParam::DepthScale => settings.depth_scale = value,
            SettingParam::Yaw => settings.yaw_deg = value,
            SettingParam::Pitch => settings.pitch_deg = value,
            SettingParam::DepthFade => settings.depth_fade = value,
            SettingParam::AaFeather => settings.aa_feather = value,
            SettingParam::Glow => settings.glow = value,
        }
    }

    pub fn format(self, value: f32) -> String {
        match self {
            SettingParam::Yaw | SettingParam::Pitch => format!("{value:.0}°"),
            SettingParam::DepthFade => format!("{:.0}%", value * 100.0),
            SettingParam::Glow => format!("{value:.1}×"),
            SettingParam::DepthScale => format!("{value:.2}"),
            _ => format!("{value:.1}"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct SettingDrag {
    param: SettingParam,
    grab: f32,
    origin_x: f32,
}

#[derive(Clone, Copy, Debug)]
struct PanelDrag {
    grab: (f32, f32),
    origin: (f32, f32),
}

const PANEL_DEFAULT_RIGHT: f32 = 3.0;
const PANEL_DEFAULT_TOP: f32 = 24.0;
const PANEL_WIDTH: f32 = 140.0;
const PANEL_GRAB_STRIP: f32 = 30.0;
const PANEL_GRIP_H: f32 = 14.0;
const FALLBACK_VIEWPORT_W: f32 = 260.0;
const FALLBACK_VIEWPORT_H: f32 = 150.0;
const SETTING_DRAG_PX: f32 = 160.0;

fn mesh_params() -> hikaru_render::WavetableMeshParams {
    hikaru_render::WavetableMeshParams {
        width: MESH_WIDTH,
        height: MESH_HEIGHT,
        thickness: MESH_THICKNESS,
    }
}

const KNOB_BODY: [f32; 4] = [0.13, 0.14, 0.17, 1.0];
const KNOB_MARKER: [f32; 4] = [1.0, 0.43, 0.0, 1.0];

fn request_view(viewport: &crate::render::WavetableViewportHandle, editor: &WavetableEditor) -> WavetableView {
    let active_frame = if editor.render_mode == RenderMode::Mode2D {
        Some(editor.current_samples())
    } else {
        None
    };
    let camera = if editor.render_mode == RenderMode::Mode2D {
        Camera::front()
    } else {
        editor.camera
    };
    let request = WavetableViewRequest {
        viewer: ViewerRequest {
            size: WAVETABLE_VIEWPORT,
            camera,
            waveform: editor.table_samples(),
            frame_len: editor.frame_len(),
            active: editor.active_cycle(),
            render_mode: editor.render_mode,
            settings: editor.render_settings,
            active_frame: active_frame.as_deref(),
            max_frames: MAX_FRAMES,
            mesh_params: mesh_params(),
            tint: [1.0, 0.55, 0.15, 1.0],
            background: hikaru_render::wgpu::Color::TRANSPARENT,
        },
        morph: editor.position(),
        knob_body: KNOB_BODY,
        knob_marker: KNOB_MARKER,
        knob_camera: Camera::knob(),
    };
    viewport.view(&request)
}

pub fn render_module(ui: &mut egui::Ui, app: &mut AppState, track_idx: usize, slot_idx: usize) {
    let view = {
        let Some(slot) = app.slot(track_idx, slot_idx) else {
            ui.label("No slot");
            return;
        };
        request_view(&app.wavetable_viewport, &slot.wavetable)
    };

    let dropped_wav = if ui.input(|i| i.pointer.any_released()) {
        app.dragged_sample.clone()
    } else {
        None
    };
    let is_dragging_wav = app.dragged_sample.is_some();

    let mut pick_wav = false;
    let mut dropped_on_wt = None;

    {
        let Some(slot) = app.slot_mut(track_idx, slot_idx) else {
            return;
        };
        let editor = &mut slot.wavetable;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            // Left: Compact 3D / 2D Wavetable Display (192px wide)
            ui.vertical(|ui| {
                ui.set_min_width(192.0);
                ui.set_max_width(192.0);
                ui.spacing_mut().item_spacing.y = 2.0;

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    let tbl_name = if editor.table.name.len() > 14 {
                        format!("{}…", &editor.table.name[..13])
                    } else {
                        editor.table.name.clone()
                    };
                    ui.label(
                        egui::RichText::new(tbl_name)
                            .size(10.0)
                            .strong()
                            .color(theme::ACCENT_PRIMARY),
                    );
                    if ui
                        .add(egui::Button::new(egui::RichText::new("2D/3D").size(9.0)).small())
                        .clicked()
                    {
                        editor.render_mode = match editor.render_mode {
                            RenderMode::Mode3D => RenderMode::Mode2D,
                            RenderMode::Mode2D => RenderMode::Mode3D,
                        };
                    }
                    let (load_r, load_resp) =
                        ui.allocate_exact_size(egui::vec2(20.0, 18.0), egui::Sense::click());
                    ui.painter().rect_filled(
                        load_r,
                        2.5,
                        if load_resp.hovered() {
                            egui::Color32::from_rgb(0x44, 0x47, 0x50)
                        } else {
                            egui::Color32::from_rgb(0x32, 0x34, 0x3C)
                        },
                    );
                    crate::ui::icons::paint_icon(
                        ui.painter(),
                        load_r.shrink(3.0),
                        crate::ui::icons::FOLDER,
                        theme::ACCENT_ORANGE,
                        1.6,
                    );
                    if load_resp.on_hover_text("Load WAV into Wavetable").clicked() {
                        pick_wav = true;
                    }
                });

                // 3D Viewport canvas (compact 190 x 106 so it fits cleanly inside 178px rack height)
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(190.0, 106.0),
                    egui::Sense::click_and_drag(),
                );
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 3.0, egui::Color32::from_rgb(0x11, 0x12, 0x15));

                if let Some(ref image) = view.viewer {
                    let texture = ui.ctx().load_texture(
                        format!("wt_{track_idx}_{slot_idx}"),
                        (**image).clone(),
                        egui::TextureOptions::LINEAR,
                    );
                    painter.image(
                        texture.id(),
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                } else {
                    // Waveform wireframe fallback
                    let samples = editor.current_samples();
                    if !samples.is_empty() {
                        let step = rect.width() / samples.len() as f32;
                        let cy = rect.center().y;
                        let hh = rect.height() * 0.4;
                        for i in 0..samples.len() - 1 {
                            let p0 = egui::pos2(rect.min.x + i as f32 * step, cy - samples[i] * hh);
                            let p1 = egui::pos2(rect.min.x + (i + 1) as f32 * step, cy - samples[i + 1] * hh);
                            painter.line_segment([p0, p1], egui::Stroke::new(1.2_f32, theme::ACCENT_PRIMARY));
                        }
                    }
                }

                let hovered_drop = is_dragging_wav
                    && ui.input(|i| i.pointer.hover_pos().is_some_and(|p| rect.contains(p)));
                painter.rect_stroke(
                    rect,
                    3.0,
                    egui::Stroke::new(
                        if hovered_drop { 1.5_f32 } else { 1.0_f32 },
                        if hovered_drop {
                            theme::ACCENT_ORANGE
                        } else {
                            egui::Color32::from_rgb(0x2D, 0x30, 0x38)
                        },
                    ),
                    egui::StrokeKind::Inside,
                );

                if let Some(ref path) = dropped_wav {
                    if ui.input(|i| i.pointer.hover_pos().is_some_and(|p| rect.contains(p))) {
                        dropped_on_wt = Some(path.clone());
                    }
                }

                // Camera rotate on drag
                if response.dragged_by(egui::PointerButton::Primary) {
                    let delta = response.drag_delta();
                    editor.camera.yaw += delta.x * 0.01;
                    editor.camera.pitch = (editor.camera.pitch + delta.y * 0.01).clamp(-1.4, 1.4);
                }
            });

            ui.separator();

            // Right: Bitwig Polysynth Oscillator Controls (GPU SDF Rotary Knobs + Fader!)
            ui.vertical(|ui| {
                ui.set_min_width(218.0);
                ui.set_max_width(218.0);
                ui.spacing_mut().item_spacing.y = 5.0;

                // Row 1: GPU Rotary Knobs (`INDEX`, `DETUNE`, `PHASE`, `UNISON`)
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;

                    // INDEX (Position)
                    ui.vertical_centered(|ui| {
                        ui.set_width(44.0);
                        let mut pos = editor.position();
                        let (r_idx, idx_changed) = crate::ui::bitwig_kit::rotary_knob(
                            ui,
                            30.0,
                            &mut pos,
                            0.0..=1.0,
                            0.0,
                            theme::ACCENT_ORANGE,
                            false,
                        );
                        r_idx.on_hover_text("Wavetable Index / Position");
                        if idx_changed {
                            editor.set_position(pos);
                        }
                        ui.label(
                            egui::RichText::new("INDEX")
                                .font(crate::ui::fonts::semibold(8.5))
                                .color(theme::TEXT_SECONDARY),
                        );
                    });

                    // DETUNE
                    ui.vertical_centered(|ui| {
                        ui.set_width(44.0);
                        let (r_det, _) = crate::ui::bitwig_kit::rotary_knob(
                            ui,
                            30.0,
                            &mut editor.detune,
                            0.0..=1.0,
                            0.0,
                            theme::BW_LCD_CYAN,
                            false,
                        );
                        r_det.on_hover_text("Unison Detune");
                        ui.label(
                            egui::RichText::new("DETUNE")
                                .font(crate::ui::fonts::semibold(8.5))
                                .color(theme::TEXT_SECONDARY),
                        );
                    });

                    // PHASE
                    ui.vertical_centered(|ui| {
                        ui.set_width(44.0);
                        let (r_phs, _) = crate::ui::bitwig_kit::rotary_knob(
                            ui,
                            30.0,
                            &mut editor.phase,
                            0.0..=1.0,
                            0.0,
                            theme::BW_FADER_AMBER,
                            false,
                        );
                        r_phs.on_hover_text("Oscillator Phase");
                        ui.label(
                            egui::RichText::new("PHASE")
                                .font(crate::ui::fonts::semibold(8.5))
                                .color(theme::TEXT_SECONDARY),
                        );
                    });

                    // UNISON
                    ui.vertical_centered(|ui| {
                        ui.set_width(44.0);
                        let mut u_f = editor.unison as f32;
                        let (r_uni, uni_changed) = crate::ui::bitwig_kit::rotary_knob(
                            ui,
                            30.0,
                            &mut u_f,
                            1.0..=16.0,
                            1.0,
                            theme::SOLO_YELLOW,
                            false,
                        );
                        r_uni.on_hover_text(format!("Unison Voices: {}", editor.unison));
                        if uni_changed {
                            editor.set_unison(u_f.round() as u8);
                        }
                        ui.label(
                            egui::RichText::new(format!("UNI {}v", editor.unison))
                                .font(crate::ui::fonts::semibold(8.5))
                                .color(theme::TEXT_SECONDARY),
                        );
                    });
                });

                // Row 2: GPU Horizontal Morph Bar + Frame Readout
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("MORPH")
                            .font(crate::ui::fonts::semibold(9.0))
                            .color(theme::ACCENT_ORANGE),
                    );
                    let mut pos = editor.position();
                    if crate::ui::bitwig_kit::fader_horizontal_range(
                        ui,
                        egui::vec2(118.0, 12.0),
                        &mut pos,
                        0.0..=1.0,
                        0.0,
                        theme::BW_FADER_AMBER,
                        theme::BW_FADER_ORANGE,
                        false,
                    )
                    .1
                    {
                        editor.set_position(pos);
                    }
                    ui.label(
                        egui::RichText::new(format!("{}/{}", editor.frame + 1, editor.frame_count()))
                            .font(crate::ui::fonts::lcd(9.5))
                            .color(theme::BW_LCD_CYAN),
                    );
                });

                // Row 3: Voice Mode + Octave & Semitone
                ui.horizontal(|ui| {
                    if crate::ui::bitwig_kit::capsule_text_button(
                        ui,
                        egui::vec2(46.0, 18.0),
                        editor.voices.label(),
                        9.5,
                        editor.voices == VoiceMode::Poly,
                        theme::ACCENT_ORANGE,
                        crate::ui::bitwig_kit::JoinSide::Standalone,
                        false,
                    )
                    .clicked()
                    {
                        editor.cycle_voices();
                    }

                    ui.label(egui::RichText::new("OCT").size(9.5).color(theme::TEXT_SECONDARY));
                    let mut oct = editor.octave as i32;
                    if ui.add(egui::DragValue::new(&mut oct).range(-3..=3)).changed() {
                        editor.octave = oct as i8;
                    }

                    ui.label(egui::RichText::new("SEMI").size(9.5).color(theme::TEXT_SECONDARY));
                    let mut p = editor.pitch as i32;
                    if ui.add(egui::DragValue::new(&mut p).range(-24..=24)).changed() {
                        editor.pitch = p as i8;
                    }
                });
            });
        });
    }

    if pick_wav {
        crate::views::explorer::begin_wavetable_pick(app, track_idx, slot_idx);
    }
    if let Some(path) = dropped_on_wt {
        load_wavetable_from_path(app, track_idx, slot_idx, &path);
        app.dragged_sample = None;
    }
}

pub fn render_module_footer(_ui: &mut egui::Ui, _app: &mut AppState, _track_idx: usize, _slot_idx: usize) {}

pub fn render(ui: &mut egui::Ui, app: &mut AppState, track_idx: usize, slot_idx: usize) {
    render_module(ui, app, track_idx, slot_idx);
}

pub fn log_initial_wavetable(label: &str) {
    let table = WavetableEditor::new().table;
    eprintln!("[ Hikaru OpenLive ] : Wavetable inicial cargado [{} | {} frames]", label, table.frames);
}

pub fn load_wavetable_from_path(
    app: &mut AppState,
    track_idx: usize,
    slot_idx: usize,
    path: &std::path::Path,
) {
    if let Ok(table) = load_table_file(path) {
        if let Some(slot) = app.slot_mut(track_idx, slot_idx) {
            slot.wavetable.load(table);
            slot.wavetable.menu_open = false;
        }
    }
}

fn sibling_cache() -> &'static std::sync::Mutex<Option<(std::path::PathBuf, Vec<std::path::PathBuf>)>> {
    static CACHE: std::sync::Mutex<Option<(std::path::PathBuf, Vec<std::path::PathBuf>)>> =
        std::sync::Mutex::new(None);
    &CACHE
}

pub fn clear_sibling_cache() {
    if let Ok(mut cache) = sibling_cache().lock() {
        *cache = None;
    }
}

pub fn sibling_tables(path: Option<&std::path::PathBuf>) -> Vec<std::path::PathBuf> {
    let Some(path) = path else { return Vec::new() };
    let Some(dir) = path.parent() else { return Vec::new() };

    if let Ok(cache) = sibling_cache().lock() {
        if let Some((cached_dir, tables)) = cache.as_ref() {
            if cached_dir == dir {
                return tables.clone();
            }
        }
    }

    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<std::path::PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("wav")))
        .collect();
    out.sort();

    if let Ok(mut cache) = sibling_cache().lock() {
        *cache = Some((dir.to_path_buf(), out.clone()));
    }
    out
}

pub fn sibling_step_target(path: Option<&std::path::PathBuf>, delta: i32) -> Option<std::path::PathBuf> {
    let siblings = sibling_tables(path);
    if siblings.is_empty() {
        return None;
    }
    let current = path.and_then(|p| {
        siblings.iter().position(|candidate| candidate == p)
    });
    let next = match current {
        Some(index) => {
            let len = siblings.len() as i32;
            (index as i32 + delta).rem_euclid(len) as usize
        }
        None => {
            if delta < 0 {
                siblings.len() - 1
            } else {
                0
            }
        }
    };
    siblings.into_iter().nth(next)
}

pub fn load_table_deferred(
    app: &mut AppState,
    track_idx: usize,
    slot_idx: usize,
    path: std::path::PathBuf,
) {
    load_wavetable_from_path(app, track_idx, slot_idx, &path);
}

pub fn step_table(app: &mut AppState, track_idx: usize, slot_idx: usize, delta: i32) {
    let current = app
        .slot(track_idx, slot_idx)
        .and_then(|slot| slot.wavetable.table.path.clone());
    if let Some(target) = sibling_step_target(current.as_ref(), delta) {
        load_table_deferred(app, track_idx, slot_idx, target);
    }
}
