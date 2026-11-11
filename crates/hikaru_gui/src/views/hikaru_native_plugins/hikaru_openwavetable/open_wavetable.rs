// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_openwavetable/open_wavetable.rs

use hikaru_project_file::OpenWavetableParams;
use hikaru_render::{Camera, GradientMode, RenderMode, RenderSettings};

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
    Volume,
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
    pub base_position: Option<f32>,
    pub render_mode: RenderMode,
    pub render_settings: RenderSettings,
    pub settings_open: bool,
    pub color_settings_open: bool,
    setting_drag: Option<SettingDrag>,
    panel_offset: (f32, f32),
    panel_drag: Option<PanelDrag>,
    viewport_size: (f32, f32),
    pub camera: Camera,
    pub unison: u8,
    pub detune: f32,
    pub phase: f32,
    pub volume: f32,
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
        let mut table = wavetable_io::bundled_wavetable(MAX_FRAMES)
            .unwrap_or_else(|_| Wavetable::default_table());
        if table.frames <= 1 {
            table = Wavetable::default_table();
            table.name = wavetable_io::BUNDLED_WAVETABLE_NAME.to_string();
        }
        Self {
            table,
            morph: 0.0,
            frame: 0,
            smooth: true,
            base_position: None,
            render_mode: RenderMode::default(),
            render_settings: RenderSettings::default(),
            settings_open: false,
            color_settings_open: false,
            setting_drag: None,
            panel_offset: (0.0, 0.0),
            panel_drag: None,
            viewport_size: (0.0, 0.0),
            camera: Camera::default(),
            unison: 1,
            detune: 0.0,
            phase: 0.0,
            volume: 1.0,
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
            KnobTarget::Volume => self.volume,
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
            KnobTarget::Volume => {
                self.volume = value.clamp(0.0, 2.0);
                self.volume
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

    pub fn to_params(&self) -> OpenWavetableParams {
        OpenWavetableParams {
            table_name: self.table.name.clone(),
            position: self.position(),
            morph: self.morph,
            frame: self.frame,
            detune: self.detune,
            phase: self.phase,
            unison: self.unison,
            volume: self.volume,
            octave: self.octave,
            pitch: self.pitch,
            voices: match self.voices {
                VoiceMode::Mono => "Mono".to_string(),
                VoiceMode::Legato => "Legato".to_string(),
                VoiceMode::Poly => "Poly".to_string(),
            },
            render_mode_3d: self.render_mode == RenderMode::Mode3D,
            render_mode_lines: self.render_settings.mesh == hikaru_render::MeshType::Wireframe,
            color_primary: self.render_settings.color_primary,
            color_secondary: self.render_settings.color_secondary,
            color_active: self.render_settings.color_active,
        }
    }

    pub fn apply_params(&mut self, p: &OpenWavetableParams) {
        if !p.table_name.is_empty() {
            self.table.name = p.table_name.clone();
        }
        self.detune = p.detune;
        self.phase = p.phase;
        self.volume = p.volume;
        self.unison = p.unison.clamp(1, 16);
        self.octave = p.octave;
        self.pitch = p.pitch;
        self.voices = match p.voices.as_str() {
            "Mono" => VoiceMode::Mono,
            "Legato" => VoiceMode::Legato,
            _ => VoiceMode::Poly,
        };
        self.render_mode = if p.render_mode_3d {
            RenderMode::Mode3D
        } else {
            RenderMode::Mode2D
        };
        self.render_settings.mesh = if p.render_mode_lines {
            hikaru_render::MeshType::Wireframe
        } else {
            hikaru_render::MeshType::Ribbon
        };
        if p.color_primary != [0.0, 0.0, 0.0] {
            self.render_settings.color_primary = p.color_primary;
        }
        if p.color_secondary != [0.0, 0.0, 0.0] {
            self.render_settings.color_secondary = p.color_secondary;
        }
        if p.color_active != [0.0, 0.0, 0.0] {
            self.render_settings.color_active = p.color_active;
        }
        self.base_position = Some(p.position);
        self.set_position(p.position);
        if p.morph > 0.0 {
            self.morph = p.morph;
        }
        if p.frame < self.table.selectable_frames() {
            self.frame = p.frame;
        }
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

#[derive(Clone, Copy, Debug)]
struct ColorPreset {
    name: &'static str,
    primary: [f32; 3],
    secondary: [f32; 3],
    active: [f32; 3],
    bg: [f32; 3],
    mode: GradientMode,
    mix: f32,
    active_boost: f32,
}

const COLOR_PRESETS: [ColorPreset; 8] = [
    ColorPreset {
        name: "Bitwig",
        primary: [1.0, 0.55, 0.15],
        secondary: [1.0, 0.22, 0.08],
        active: [1.0, 0.92, 0.55],
        bg: [0.043, 0.047, 0.059],
        mode: GradientMode::Solid,
        mix: 0.75,
        active_boost: 0.0,
    },
    ColorPreset {
        name: "Vital",
        primary: [1.0, 0.18, 0.68],
        secondary: [0.10, 0.92, 1.0],
        active: [1.0, 0.96, 0.75],
        bg: [0.045, 0.035, 0.075],
        mode: GradientMode::DepthZ,
        mix: 0.92,
        active_boost: 0.55,
    },
    ColorPreset {
        name: "Serum",
        primary: [0.12, 0.95, 0.62],
        secondary: [0.08, 0.48, 1.0],
        active: [0.85, 1.0, 0.95],
        bg: [0.035, 0.055, 0.068],
        mode: GradientMode::DepthZ,
        mix: 0.88,
        active_boost: 0.45,
    },
    ColorPreset {
        name: "Fuego",
        primary: [1.0, 0.90, 0.18],
        secondary: [0.95, 0.12, 0.06],
        active: [1.0, 1.0, 0.85],
        bg: [0.065, 0.032, 0.028],
        mode: GradientMode::AmplitudeY,
        mix: 0.95,
        active_boost: 0.40,
    },
    ColorPreset {
        name: "Océano",
        primary: [0.15, 0.88, 1.0],
        secondary: [0.18, 0.22, 0.92],
        active: [0.82, 0.98, 1.0],
        bg: [0.030, 0.045, 0.078],
        mode: GradientMode::PhaseX,
        mix: 0.90,
        active_boost: 0.50,
    },
    ColorPreset {
        name: "Aurora",
        primary: [0.22, 1.0, 0.55],
        secondary: [0.68, 0.20, 1.0],
        active: [0.92, 1.0, 0.85],
        bg: [0.038, 0.052, 0.065],
        mode: GradientMode::DepthZ,
        mix: 0.92,
        active_boost: 0.50,
    },
    ColorPreset {
        name: "Arcoíris",
        primary: [1.0, 0.45, 0.20],
        secondary: [0.20, 0.90, 1.0],
        active: [1.0, 1.0, 1.0],
        bg: [0.040, 0.042, 0.055],
        mode: GradientMode::Rainbow,
        mix: 0.95,
        active_boost: 0.35,
    },
    ColorPreset {
        name: "Oro",
        primary: [1.0, 0.78, 0.22],
        secondary: [0.58, 0.32, 0.06],
        active: [1.0, 0.96, 0.78],
        bg: [0.055, 0.045, 0.032],
        mode: GradientMode::ActiveFocus,
        mix: 0.85,
        active_boost: 0.65,
    },
];

fn rgb_to_color32(rgb: [f32; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(
        (rgb[0].clamp(0.0, 1.0) * 255.0).round() as u8,
        (rgb[1].clamp(0.0, 1.0) * 255.0).round() as u8,
        (rgb[2].clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

fn hsv_to_rgb_cpu(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = h.rem_euclid(1.0) * 6.0;
    let i = h.floor() as i32;
    let f = h - h.floor();
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    match i.rem_euclid(6) {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

fn eval_gradient_color_cpu(
    settings: &RenderSettings,
    u_phase: f32,
    v_amp: f32,
    w_depth: f32,
    is_active: bool,
    alpha: u8,
) -> egui::Color32 {
    let mut grad_t = match settings.gradient_mode {
        GradientMode::Solid => 0.0,
        GradientMode::DepthZ => w_depth,
        GradientMode::AmplitudeY => v_amp,
        GradientMode::PhaseX => u_phase,
        GradientMode::ActiveFocus => if is_active { 1.0 } else { 0.15 },
        GradientMode::Rainbow => (u_phase * 0.65 + w_depth * 0.35 + settings.rainbow_shift).rem_euclid(1.0),
    };
    if settings.gradient_invert {
        grad_t = 1.0 - grad_t;
    }
    let mix_amt = settings.gradient_mix.clamp(0.0, 1.0);
    let base = if settings.gradient_mode == GradientMode::Rainbow {
        let r = hsv_to_rgb_cpu(grad_t, 0.88, 1.0);
        [
            settings.color_primary[0] + (r[0] - settings.color_primary[0]) * mix_amt,
            settings.color_primary[1] + (r[1] - settings.color_primary[1]) * mix_amt,
            settings.color_primary[2] + (r[2] - settings.color_primary[2]) * mix_amt,
        ]
    } else if settings.gradient_mode != GradientMode::Solid {
        let t = grad_t * mix_amt;
        [
            settings.color_primary[0] + (settings.color_secondary[0] - settings.color_primary[0]) * t,
            settings.color_primary[1] + (settings.color_secondary[1] - settings.color_primary[1]) * t,
            settings.color_primary[2] + (settings.color_secondary[2] - settings.color_primary[2]) * t,
        ]
    } else {
        settings.color_primary
    };
    let final_rgb = if is_active {
        let b = settings.active_boost.clamp(0.0, 1.0);
        [
            base[0] + (settings.color_active[0] - base[0]) * b,
            base[1] + (settings.color_active[1] - base[1]) * b,
            base[2] + (settings.color_active[2] - base[2]) * b,
        ]
    } else {
        base
    };
    let glow = settings.glow.clamp(0.3, 2.0);
    egui::Color32::from_rgba_unmultiplied(
        ((final_rgb[0] * glow).clamp(0.0, 1.0) * 255.0).round() as u8,
        ((final_rgb[1] * glow).clamp(0.0, 1.0) * 255.0).round() as u8,
        ((final_rgb[2] * glow).clamp(0.0, 1.0) * 255.0).round() as u8,
        alpha,
    )
}

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
    let c_pri = editor.render_settings.color_primary;
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
            tint: [c_pri[0], c_pri[1], c_pri[2], 1.0],
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
    app.wavetable_viewport.spawn_connect(Some(ui.ctx().clone()));
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

        let body_rect = ui.available_rect_before_wrap();
        let body_h = body_rect.height().max(96.0);
        let left_w = 188.0;
        let sep_x = body_rect.min.x + left_w + 4.0;
        let right_x0 = sep_x + 5.0;
        let right_w = (body_rect.max.x - right_x0).max(180.0);

        let left_rect = egui::Rect::from_min_size(body_rect.min, egui::vec2(left_w, body_h));
        let right_rect = egui::Rect::from_min_size(
            egui::pos2(right_x0, body_rect.min.y),
            egui::vec2(right_w, body_h),
        );

        // Left: Compact 3D / 2D Wavetable Display (188px wide, anchored top-left)
        let mut left_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(left_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        left_ui.spacing_mut().item_spacing.y = 2.0;

        left_ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 1.5;
            let tbl_name = if editor.table.name.len() > 11 {
                format!("{}…", &editor.table.name[..10])
            } else {
                editor.table.name.clone()
            };
            ui.menu_button(
                egui::RichText::new(&tbl_name)
                    .size(8.8)
                    .strong()
                    .color(rgb_to_color32(editor.render_settings.color_primary)),
                |menu_ui| {
                    if menu_ui.button("💾 Guardar Preset (*.hdp)").clicked() {
                        save_openwavetable_preset(editor);
                        menu_ui.close_menu();
                    }
                    if menu_ui.button("📂 Cargar Preset (*.hdp)").clicked() {
                        load_openwavetable_preset(editor);
                        menu_ui.close_menu();
                    }
                    menu_ui.separator();
                    if menu_ui.button("📁 Cargar WAV externo...").clicked() {
                        pick_wav = true;
                        menu_ui.close_menu();
                    }
                },
            );

            // Toggle 2D/3D (GPU capsule button showing active state in 3D)
            let is_3d = editor.render_mode == RenderMode::Mode3D;
            let mode_lbl = match editor.render_mode {
                RenderMode::Mode3D => "3D",
                RenderMode::Mode2D => "2D",
            };
            if crate::ui::bitwig_kit::capsule_text_button(
                ui,
                egui::vec2(21.0, 16.0),
                mode_lbl,
                8.0,
                is_3d,
                theme::ACCENT_ORANGE,
                crate::ui::bitwig_kit::JoinSide::Standalone,
                false,
            )
            .on_hover_text("Alternar vista 2D / 3D (wgpu + WGSL)")
            .clicked()
            {
                editor.render_mode = editor.render_mode.toggle();
            }

            // Cycle MeshType (Líneas / Cintas / Sólido) in 3D
            if crate::ui::bitwig_kit::capsule_text_button(
                ui,
                egui::vec2(34.0, 16.0),
                editor.render_settings.mesh.label(),
                7.8,
                editor.render_settings.mesh != hikaru_render::MeshType::Wireframe,
                theme::BW_LCD_CYAN,
                crate::ui::bitwig_kit::JoinSide::Standalone,
                false,
            )
            .on_hover_text("Tipo de malla 3D: Líneas / Cintas / Sólido")
            .clicked()
            {
                editor.render_settings.mesh = editor.render_settings.mesh.cycle();
            }

            // Settings gear button (Tabler vector icon)
            let (gear_r, gear_resp) =
                ui.allocate_exact_size(egui::vec2(15.0, 16.0), egui::Sense::click());
            crate::ui::bitwig_wgpu::paint_capsule(
                ui.painter(),
                gear_r,
                2.5,
                egui::Color32::from_rgb(0x32, 0x34, 0x3C),
                theme::ACCENT_ORANGE,
                (if editor.settings_open { crate::ui::bitwig_wgpu::FLAG_ACTIVE } else { 0 })
                    | (if gear_resp.hovered() { crate::ui::bitwig_wgpu::FLAG_HOVERED } else { 0 }),
            );
            crate::ui::icons::paint_icon(
                ui.painter(),
                gear_r.shrink(2.4),
                crate::ui::icons::SETTINGS,
                if editor.settings_open {
                    egui::Color32::from_rgb(20, 16, 12)
                } else {
                    theme::TEXT_PRIMARY
                },
                1.4,
            );
            if gear_resp
                .on_hover_text("Ajustes del visor 3D (Trazo, Profundidad, Yaw/Pitch, Glow)")
                .clicked()
            {
                editor.settings_open = !editor.settings_open;
            }

            // Color & Gradient Palette button (Second configuration button)
            let (pal_r, pal_resp) =
                ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
            crate::ui::bitwig_wgpu::paint_capsule(
                ui.painter(),
                pal_r,
                2.5,
                egui::Color32::from_rgb(0x32, 0x34, 0x3C),
                rgb_to_color32(editor.render_settings.color_primary),
                (if editor.color_settings_open { crate::ui::bitwig_wgpu::FLAG_ACTIVE } else { 0 })
                    | (if pal_resp.hovered() { crate::ui::bitwig_wgpu::FLAG_HOVERED } else { 0 }),
            );
            {
                let p = ui.painter();
                let c = pal_r.center();
                let c1 = rgb_to_color32(editor.render_settings.color_primary);
                let c2 = if editor.render_settings.gradient_mode == GradientMode::Solid {
                    rgb_to_color32(editor.render_settings.color_active)
                } else {
                    rgb_to_color32(editor.render_settings.color_secondary)
                };
                let c3 = rgb_to_color32(editor.render_settings.color_active);
                p.circle_filled(egui::pos2(c.x - 2.4, c.y - 1.5), 2.1, c1);
                p.circle_filled(egui::pos2(c.x + 2.4, c.y - 1.5), 2.1, c2);
                p.circle_filled(egui::pos2(c.x, c.y + 2.2), 2.1, c3);
                p.circle_stroke(
                    c,
                    5.0,
                    egui::Stroke::new(
                        0.9_f32,
                        if editor.color_settings_open {
                            egui::Color32::from_rgb(20, 16, 12)
                        } else {
                            egui::Color32::from_rgba_unmultiplied(230, 234, 242, 180)
                        },
                    ),
                );
            }
            if pal_resp
                .on_hover_text("Colores y Degradados del Wavetable (Presets, Degradado Z/Y/X/Arcoíris, Colores)")
                .clicked()
            {
                editor.color_settings_open = !editor.color_settings_open;
            }

            // Load WAV folder button (Tabler vector icon)
            let (load_r, load_resp) =
                ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
            crate::ui::bitwig_wgpu::paint_capsule(
                ui.painter(),
                load_r,
                2.5,
                egui::Color32::from_rgb(0x32, 0x34, 0x3C),
                theme::ACCENT_ORANGE,
                if load_resp.hovered() { crate::ui::bitwig_wgpu::FLAG_HOVERED } else { 0 },
            );
            crate::ui::icons::paint_icon(
                ui.painter(),
                load_r.shrink(2.4),
                crate::ui::icons::FOLDER,
                theme::ACCENT_ORANGE,
                1.5,
            );
            if load_resp.on_hover_text("Load WAV into Wavetable").clicked() {
                pick_wav = true;
            }
        });

        // 3D Viewport canvas (compact 186 x remaining height so it never overflows the card)
        let vp_h = (left_ui.available_height() - 2.0).clamp(80.0, 108.0);
        let (rect, response) = left_ui.allocate_exact_size(
            egui::vec2(186.0, vp_h),
            egui::Sense::click_and_drag(),
        );
        editor.record_viewport_size(rect.width(), rect.height());
        let painter = left_ui.painter_at(rect);
        painter.rect_filled(rect, 3.0, rgb_to_color32(editor.render_settings.bg_color));

        if let Some(ref image) = view.viewer {
            let texture = left_ui.ctx().load_texture(
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
            // CPU fallback while wgpu offscreen context initializes or in headless tests:
            // Renders either 3D oblique stacked cycles or 2D active cycle with custom gradient colors!
            match editor.render_mode {
                RenderMode::Mode3D => {
                    let total_frames = editor.frame_count().clamp(1, 16);
                    let active_f = editor.position() * (total_frames.saturating_sub(1)) as f32;
                    let table_samples = editor.table_samples();
                    let f_len = editor.frame_len().max(1);
                    let avail_frames = (table_samples.len() / f_len).max(1);
                    let pad_x = 12.0;
                    let pad_y = 14.0;
                    let usable_w = (rect.width() - pad_x * 2.0 - 22.0).max(40.0);
                    let hh = (rect.height() - pad_y * 2.0) * 0.28;

                    for slot_i in (0..total_frames).rev() {
                        let depth_t = if total_frames > 1 {
                            slot_i as f32 / (total_frames - 1) as f32
                        } else {
                            0.0
                        };
                        let tbl_idx = ((depth_t * (avail_frames - 1) as f32).round() as usize)
                            .min(avail_frames - 1);
                        let start = tbl_idx * f_len;
                        let end = (start + f_len).min(table_samples.len());
                        let slice = &table_samples[start..end];
                        if slice.len() < 2 {
                            continue;
                        }
                        let ox = rect.min.x + pad_x + depth_t * 20.0;
                        let oy = rect.max.y - pad_y - depth_t * 22.0 - hh;
                        let dist = (slot_i as f32 - active_f).abs();
                        let is_active = dist < 0.75;
                        let alpha = if is_active {
                            255
                        } else {
                            (120.0 * (1.0 - depth_t * 0.45)) as u8
                        };
                        let cols = 64.min(slice.len());
                        let step_x = usable_w / (cols - 1).max(1) as f32;
                        for c in 0..cols - 1 {
                            let s0 = slice[c * slice.len() / cols];
                            let s1 = slice[(c + 1) * slice.len() / cols];
                            let u_phase = c as f32 / (cols - 1).max(1) as f32;
                            let v_amp = ((s0 + s1) * 0.25 + 0.5).clamp(0.0, 1.0);
                            let col = eval_gradient_color_cpu(
                                &editor.render_settings,
                                u_phase,
                                v_amp,
                                depth_t,
                                is_active,
                                alpha,
                            );
                            let p0 = egui::pos2(ox + c as f32 * step_x, oy - s0 * hh);
                            let p1 = egui::pos2(ox + (c + 1) as f32 * step_x, oy - s1 * hh);
                            painter.line_segment(
                                [p0, p1],
                                egui::Stroke::new(if is_active { 1.6_f32 } else { 0.9_f32 }, col),
                            );
                        }
                    }
                }
                RenderMode::Mode2D => {
                    let samples = editor.current_samples();
                    if !samples.is_empty() {
                        let step = rect.width() / samples.len() as f32;
                        let cy = rect.center().y;
                        let hh = rect.height() * 0.4;
                        let n = (samples.len() - 1).max(1) as f32;
                        for i in 0..samples.len() - 1 {
                            let u_phase = i as f32 / n;
                            let v_amp = ((samples[i] + samples[i + 1]) * 0.25 + 0.5).clamp(0.0, 1.0);
                            let col = eval_gradient_color_cpu(
                                &editor.render_settings,
                                u_phase,
                                v_amp,
                                0.0,
                                true,
                                255,
                            );
                            let p0 = egui::pos2(rect.min.x + i as f32 * step, cy - samples[i] * hh);
                            let p1 = egui::pos2(
                                rect.min.x + (i + 1) as f32 * step,
                                cy - samples[i + 1] * hh,
                            );
                            painter.line_segment(
                                [p0, p1],
                                egui::Stroke::new(1.3_f32, col),
                            );
                        }
                    }
                }
            }
        }

        let hovered_drop = is_dragging_wav
            && left_ui.input(|i| i.pointer.hover_pos().is_some_and(|p| rect.contains(p)));
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
            if left_ui.input(|i| i.pointer.hover_pos().is_some_and(|p| rect.contains(p))) {
                dropped_on_wt = Some(path.clone());
            }
        }

        // Camera / Terrain angle rotate on drag inside the 3D viewport!
        if response.dragged_by(egui::PointerButton::Primary) {
            let delta = response.drag_delta();
            editor.camera.yaw += delta.x * 0.01;
            editor.camera.pitch = (editor.camera.pitch + delta.y * 0.01).clamp(-1.4, 1.4);
            let next_yaw = editor.render_settings.yaw_deg + delta.x * 0.35;
            let next_pitch = editor.render_settings.pitch_deg - delta.y * 0.35;
            SettingParam::Yaw.set(&mut editor.render_settings, next_yaw);
            SettingParam::Pitch.set(&mut editor.render_settings, next_pitch);
        }

        // Floating popup for 3D RenderSettings ("Hikaru OpenWavetable Settings")
        if editor.settings_open {
            egui::Window::new(format!("Ajustes 3D — OpenWavetable #{slot_idx}"))
                .id(egui::Id::new(("wt_3d_settings", track_idx, slot_idx)))
                .collapsible(false)
                .resizable(false)
                .default_pos(egui::pos2(rect.max.x + 6.0, rect.min.y - 20.0))
                .fixed_size(egui::vec2(210.0, 148.0))
                .show(left_ui.ctx(), |ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    let def_settings = RenderSettings::default();
                    for param in SettingParam::ALL {
                        let (min_v, max_v) = param.range();
                        let mut val = param.get(&editor.render_settings);
                        let def_val = param.get(&def_settings);
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("{:<6}", param.title()))
                                    .font(crate::ui::fonts::semibold(9.0))
                                    .color(theme::TEXT_SECONDARY),
                            );
                            if crate::ui::bitwig_kit::fader_horizontal_range(
                                ui,
                                egui::vec2(108.0, 10.0),
                                &mut val,
                                min_v..=max_v,
                                def_val,
                                theme::BW_FADER_AMBER,
                                theme::BW_FADER_ORANGE,
                                min_v < 0.0,
                            )
                            .1
                            {
                                param.set(&mut editor.render_settings, val);
                            }
                            ui.label(
                                egui::RichText::new(param.format(val))
                                    .font(crate::ui::fonts::lcd(9.0))
                                    .color(theme::BW_LCD_CYAN),
                            );
                        });
                    }
                });
        }

        // Second floating popup: Color & Custom Gradient Settings ("Colores & Degradado")
        if editor.color_settings_open {
            egui::Window::new(format!("Colores & Degradado — OpenWavetable #{slot_idx}"))
                .id(egui::Id::new(("wt_color_settings", track_idx, slot_idx)))
                .collapsible(false)
                .resizable(false)
                .default_pos(egui::pos2(rect.max.x + 224.0, rect.min.y - 24.0))
                .fixed_size(egui::vec2(252.0, 224.0))
                .show(left_ui.ctx(), |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;

                    // 1. Live Gradient Preview Strip
                    let (bar_r, _) =
                        ui.allocate_exact_size(egui::vec2(238.0, 12.0), egui::Sense::hover());
                    let bar_p = ui.painter_at(bar_r);
                    let steps = 48usize;
                    let seg_w = bar_r.width() / steps as f32;
                    for i in 0..steps {
                        let t = i as f32 / (steps - 1).max(1) as f32;
                        let col = eval_gradient_color_cpu(
                            &editor.render_settings,
                            t,
                            t,
                            t,
                            false,
                            255,
                        );
                        bar_p.rect_filled(
                            egui::Rect::from_min_size(
                                egui::pos2(bar_r.min.x + i as f32 * seg_w, bar_r.min.y),
                                egui::vec2(seg_w + 0.8, bar_r.height()),
                            ),
                            0.0,
                            col,
                        );
                    }
                    bar_p.rect_stroke(
                        bar_r,
                        2.0,
                        egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0x36, 0x3A, 0x44)),
                        egui::StrokeKind::Inside,
                    );

                    // 2. Built-in Color & Gradient Presets (2 rows of 4 buttons)
                    ui.label(
                        egui::RichText::new("Presets de Color / Degradado")
                            .font(crate::ui::fonts::semibold(8.5))
                            .color(theme::TEXT_SECONDARY),
                    );
                    for chunk in COLOR_PRESETS.chunks(4) {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 3.5;
                            for preset in chunk {
                                let is_sel = editor.render_settings.gradient_mode == preset.mode
                                    && (editor.render_settings.color_primary[0] - preset.primary[0])
                                        .abs()
                                        < 0.03
                                    && (editor.render_settings.color_primary[1] - preset.primary[1])
                                        .abs()
                                        < 0.03
                                    && (editor.render_settings.color_secondary[0]
                                        - preset.secondary[0])
                                        .abs()
                                        < 0.03;
                                if crate::ui::bitwig_kit::capsule_text_button(
                                    ui,
                                    egui::vec2(56.0, 16.0),
                                    preset.name,
                                    8.0,
                                    is_sel,
                                    rgb_to_color32(preset.primary),
                                    crate::ui::bitwig_kit::JoinSide::Standalone,
                                    false,
                                )
                                .clicked()
                                {
                                    editor.render_settings.color_primary = preset.primary;
                                    editor.render_settings.color_secondary = preset.secondary;
                                    editor.render_settings.color_active = preset.active;
                                    editor.render_settings.bg_color = preset.bg;
                                    editor.render_settings.gradient_mode = preset.mode;
                                    editor.render_settings.gradient_mix = preset.mix;
                                    editor.render_settings.active_boost = preset.active_boost;
                                }
                            }
                        });
                    }

                    // 3. Gradient Direction / Mode Selector (2 rows of 3) + Invert toggle
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("Modo Degradado")
                                .font(crate::ui::fonts::semibold(8.5))
                                .color(theme::TEXT_SECONDARY),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if crate::ui::bitwig_kit::capsule_text_button(
                                ui,
                                egui::vec2(48.0, 15.0),
                                "Invertir",
                                8.0,
                                editor.render_settings.gradient_invert,
                                theme::BW_LCD_CYAN,
                                crate::ui::bitwig_kit::JoinSide::Standalone,
                                false,
                            )
                            .on_hover_text("Invertir dirección del degradado")
                            .clicked()
                            {
                                editor.render_settings.gradient_invert =
                                    !editor.render_settings.gradient_invert;
                            }
                        });
                    });

                    for chunk in GradientMode::ALL.chunks(3) {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            for &mode in chunk {
                                let active = editor.render_settings.gradient_mode == mode;
                                if crate::ui::bitwig_kit::capsule_text_button(
                                    ui,
                                    egui::vec2(76.0, 16.0),
                                    mode.label(),
                                    8.0,
                                    active,
                                    theme::ACCENT_ORANGE,
                                    crate::ui::bitwig_kit::JoinSide::Standalone,
                                    false,
                                )
                                .clicked()
                                {
                                    editor.render_settings.gradient_mode = mode;
                                }
                            }
                        });
                    }

                    // 4. Custom RGB Color Pickers (Inicio, Fin, Activo, Fondo) + Swap button
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 5.0;
                        ui.label(
                            egui::RichText::new("Inicio")
                                .font(crate::ui::fonts::semibold(8.5))
                                .color(theme::TEXT_SECONDARY),
                        );
                        ui.color_edit_button_rgb(&mut editor.render_settings.color_primary);

                        if crate::ui::bitwig_kit::capsule_text_button(
                            ui,
                            egui::vec2(22.0, 16.0),
                            "⇄",
                            8.5,
                            false,
                            theme::BW_LCD_CYAN,
                            crate::ui::bitwig_kit::JoinSide::Standalone,
                            false,
                        )
                        .on_hover_text("Intercambiar color inicial y final")
                        .clicked()
                        {
                            std::mem::swap(
                                &mut editor.render_settings.color_primary,
                                &mut editor.render_settings.color_secondary,
                            );
                        }

                        ui.label(
                            egui::RichText::new("Fin")
                                .font(crate::ui::fonts::semibold(8.5))
                                .color(theme::TEXT_SECONDARY),
                        );
                        ui.color_edit_button_rgb(&mut editor.render_settings.color_secondary);

                        ui.label(
                            egui::RichText::new("Activo")
                                .font(crate::ui::fonts::semibold(8.5))
                                .color(theme::TEXT_SECONDARY),
                        );
                        ui.color_edit_button_rgb(&mut editor.render_settings.color_active);

                        ui.label(
                            egui::RichText::new("Fondo")
                                .font(crate::ui::fonts::semibold(8.5))
                                .color(theme::TEXT_SECONDARY),
                        );
                        ui.color_edit_button_rgb(&mut editor.render_settings.bg_color);
                    });

                    // 5. Gradient Mix, Active Boost & Rainbow Shift Sliders
                    let mut mix_val = editor.render_settings.gradient_mix;
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("Mezcla ")
                                .font(crate::ui::fonts::semibold(8.5))
                                .color(theme::TEXT_SECONDARY),
                        );
                        if crate::ui::bitwig_kit::fader_horizontal_range(
                            ui,
                            egui::vec2(142.0, 10.0),
                            &mut mix_val,
                            0.0..=1.0,
                            0.85,
                            rgb_to_color32(editor.render_settings.color_primary),
                            rgb_to_color32(editor.render_settings.color_secondary),
                            false,
                        )
                        .1
                        {
                            editor.render_settings.gradient_mix = mix_val;
                        }
                        ui.label(
                            egui::RichText::new(format!("{:.0}%", mix_val * 100.0))
                                .font(crate::ui::fonts::lcd(9.0))
                                .color(theme::BW_LCD_CYAN),
                        );
                    });

                    let mut act_val = editor.render_settings.active_boost;
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("Realce ")
                                .font(crate::ui::fonts::semibold(8.5))
                                .color(theme::TEXT_SECONDARY),
                        );
                        if crate::ui::bitwig_kit::fader_horizontal_range(
                            ui,
                            egui::vec2(142.0, 10.0),
                            &mut act_val,
                            0.0..=1.0,
                            0.0,
                            theme::BW_FADER_AMBER,
                            rgb_to_color32(editor.render_settings.color_active),
                            false,
                        )
                        .1
                        {
                            editor.render_settings.active_boost = act_val;
                        }
                        ui.label(
                            egui::RichText::new(format!("{:.0}%", act_val * 100.0))
                                .font(crate::ui::fonts::lcd(9.0))
                                .color(theme::BW_LCD_CYAN),
                        );
                    });

                    if editor.render_settings.gradient_mode == GradientMode::Rainbow {
                        let mut shift_val = editor.render_settings.rainbow_shift;
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("Matiz  ")
                                    .font(crate::ui::fonts::semibold(8.5))
                                    .color(theme::TEXT_SECONDARY),
                            );
                            if crate::ui::bitwig_kit::fader_horizontal_range(
                                ui,
                                egui::vec2(142.0, 10.0),
                                &mut shift_val,
                                0.0..=1.0,
                                0.0,
                                theme::BW_LCD_CYAN,
                                theme::ACCENT_ORANGE,
                                false,
                            )
                            .1
                            {
                                editor.render_settings.rainbow_shift = shift_val;
                            }
                            ui.label(
                                egui::RichText::new(format!("{:.0}°", shift_val * 360.0))
                                    .font(crate::ui::fonts::lcd(9.0))
                                    .color(theme::BW_LCD_CYAN),
                            );
                        });
                    }
                });
        }

        // Vertical divider line between 3D Display and Controls
        ui.painter().line_segment(
            [
                egui::pos2(sep_x, body_rect.min.y + 2.0),
                egui::pos2(sep_x, body_rect.max.y - 2.0),
            ],
            egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0x1E, 0x20, 0x25)),
        );

        // Right: Bitwig Polysynth Oscillator Controls (GPU SDF Rotary Knobs + Fader!)
        let mut right_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(right_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        right_ui.spacing_mut().item_spacing.y = 4.0;

        // Row 1: GPU Rotary Knobs (`WT POS`, `DETUNE`, `PHASE`, `UNISON`, `VOL`) with explicit cell rects
        let knobs_row_h = 42.0;
        let (knobs_row_rect, _) =
            right_ui.allocate_exact_size(egui::vec2(right_w, knobs_row_h), egui::Sense::hover());
        let col_w = (right_w / 5.0).floor();

        for col_idx in 0..5 {
            let cell_rect = egui::Rect::from_min_size(
                egui::pos2(knobs_row_rect.min.x + col_idx as f32 * col_w, knobs_row_rect.min.y),
                egui::vec2(col_w, knobs_row_h),
            );
            let mut cell_ui = right_ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(cell_rect)
                    .layout(egui::Layout::top_down(egui::Align::Center)),
            );
            cell_ui.spacing_mut().item_spacing.y = 1.0;

            match col_idx {
                0 => {
                    let mod_opt = crate::views::hikaru_native_plugins::hikaru_openmodulation::get_modulation(
                        track_idx,
                        "OpenWavetable",
                        "WT POS",
                    );
                    let is_target = crate::views::hikaru_native_plugins::hikaru_openmodulation::is_mapping_target(
                        track_idx,
                        "OpenWavetable",
                        "WT POS",
                    );

                    let base = editor.base_position.unwrap_or_else(|| editor.position());
                    let mut effective = if let Some(m) = mod_opt {
                        (base + m.delta).clamp(0.0, 1.0)
                    } else {
                        base
                    };

                    let (r_idx, idx_changed) = crate::ui::bitwig_kit::rotary_knob(
                        &mut cell_ui,
                        28.0,
                        &mut effective,
                        0.0..=1.0,
                        0.0,
                        if mod_opt.is_some() {
                            theme::BW_LCD_CYAN
                        } else {
                            theme::ACCENT_ORANGE
                        },
                        false,
                    );

                    if is_target {
                        cell_ui.painter().rect_stroke(
                            cell_rect,
                            3.0,
                            egui::Stroke::new(1.5_f32, theme::BW_LCD_CYAN),
                            egui::StrokeKind::Inside,
                        );
                        if r_idx.clicked() {
                            crate::views::hikaru_native_plugins::hikaru_openmodulation::handle_knob_mapping_click(
                                track_idx,
                                "OpenWavetable",
                                "WT POS",
                            );
                        }
                    }

                    if idx_changed {
                        editor.base_position = Some(effective);
                        editor.set_position(effective);
                    } else if mod_opt.is_some() {
                        editor.set_position(effective);
                    }

                    if let Some(m) = mod_opt {
                        r_idx.on_hover_text(format!(
                            "WT POS [Modulated by LFO]: {:.1}% (Base: {:.1}%, Delta: {:+.2})",
                            effective * 100.0,
                            base * 100.0,
                            m.delta
                        ));
                        cell_ui.label(
                            egui::RichText::new("WT POS ●")
                                .font(crate::ui::fonts::semibold(8.5))
                                .color(theme::BW_LCD_CYAN),
                        );
                    } else {
                        r_idx.on_hover_text("Wavetable Position / Index");
                        cell_ui.label(
                            egui::RichText::new("WT POS")
                                .font(crate::ui::fonts::semibold(8.5))
                                .color(theme::TEXT_SECONDARY),
                        );
                    }
                }
                1 => {
                    let is_target = crate::views::hikaru_native_plugins::hikaru_openmodulation::is_mapping_target(
                        track_idx,
                        "OpenWavetable",
                        "DETUNE",
                    );
                    let mod_opt = crate::views::hikaru_native_plugins::hikaru_openmodulation::get_modulation(
                        track_idx,
                        "OpenWavetable",
                        "DETUNE",
                    );
                    let (r_det, _) = crate::ui::bitwig_kit::rotary_knob(
                        &mut cell_ui,
                        28.0,
                        &mut editor.detune,
                        0.0..=1.0,
                        0.0,
                        if mod_opt.is_some() {
                            theme::BW_FADER_AMBER
                        } else {
                            theme::BW_LCD_CYAN
                        },
                        false,
                    );
                    if is_target {
                        cell_ui.painter().rect_stroke(
                            cell_rect,
                            3.0,
                            egui::Stroke::new(1.5_f32, theme::BW_LCD_CYAN),
                            egui::StrokeKind::Inside,
                        );
                        if r_det.clicked() {
                            crate::views::hikaru_native_plugins::hikaru_openmodulation::handle_knob_mapping_click(
                                track_idx,
                                "OpenWavetable",
                                "DETUNE",
                            );
                        }
                    }
                    if let Some(m) = mod_opt {
                        r_det.on_hover_text(format!("Detune [LFO Delta]: {:+.2}", m.delta));
                        cell_ui.label(
                            egui::RichText::new("DETUNE ●")
                                .font(crate::ui::fonts::semibold(8.5))
                                .color(theme::BW_LCD_CYAN),
                        );
                    } else {
                        r_det.on_hover_text("Unison Detune");
                        cell_ui.label(
                            egui::RichText::new("DETUNE")
                                .font(crate::ui::fonts::semibold(8.5))
                                .color(theme::TEXT_SECONDARY),
                        );
                    }
                }
                2 => {
                    let is_target = crate::views::hikaru_native_plugins::hikaru_openmodulation::is_mapping_target(
                        track_idx,
                        "OpenWavetable",
                        "PHASE",
                    );
                    let mod_opt = crate::views::hikaru_native_plugins::hikaru_openmodulation::get_modulation(
                        track_idx,
                        "OpenWavetable",
                        "PHASE",
                    );
                    let (r_phs, _) = crate::ui::bitwig_kit::rotary_knob(
                        &mut cell_ui,
                        28.0,
                        &mut editor.phase,
                        0.0..=1.0,
                        0.0,
                        if mod_opt.is_some() {
                            theme::BW_LCD_CYAN
                        } else {
                            theme::BW_FADER_AMBER
                        },
                        false,
                    );
                    if is_target {
                        cell_ui.painter().rect_stroke(
                            cell_rect,
                            3.0,
                            egui::Stroke::new(1.5_f32, theme::BW_LCD_CYAN),
                            egui::StrokeKind::Inside,
                        );
                        if r_phs.clicked() {
                            crate::views::hikaru_native_plugins::hikaru_openmodulation::handle_knob_mapping_click(
                                track_idx,
                                "OpenWavetable",
                                "PHASE",
                            );
                        }
                    }
                    if let Some(m) = mod_opt {
                        r_phs.on_hover_text(format!("Phase [LFO Delta]: {:+.2}", m.delta));
                        cell_ui.label(
                            egui::RichText::new("PHASE ●")
                                .font(crate::ui::fonts::semibold(8.5))
                                .color(theme::BW_LCD_CYAN),
                        );
                    } else {
                        r_phs.on_hover_text("Oscillator Phase");
                        cell_ui.label(
                            egui::RichText::new("PHASE")
                                .font(crate::ui::fonts::semibold(8.5))
                                .color(theme::TEXT_SECONDARY),
                        );
                    }
                }
                3 => {
                    let mut u_f = editor.unison as f32;
                    let (r_uni, uni_changed) = crate::ui::bitwig_kit::rotary_knob(
                        &mut cell_ui,
                        28.0,
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
                    cell_ui.label(
                        egui::RichText::new(format!("UNI {}v", editor.unison))
                            .font(crate::ui::fonts::semibold(8.5))
                            .color(theme::TEXT_SECONDARY),
                    );
                }
                _ => {
                    let is_target = crate::views::hikaru_native_plugins::hikaru_openmodulation::is_mapping_target(
                        track_idx,
                        "OpenWavetable",
                        "VOL",
                    );
                    let mod_opt = crate::views::hikaru_native_plugins::hikaru_openmodulation::get_modulation(
                        track_idx,
                        "OpenWavetable",
                        "VOL",
                    );
                    let mut eff_vol = if let Some(m) = mod_opt {
                        (editor.volume + m.delta).clamp(0.0, 2.0)
                    } else {
                        editor.volume
                    };

                    let (r_vol, vol_changed) = crate::ui::bitwig_kit::rotary_knob(
                        &mut cell_ui,
                        28.0,
                        &mut eff_vol,
                        0.0..=2.0,
                        1.0,
                        if mod_opt.is_some() {
                            theme::BW_LCD_CYAN
                        } else {
                            theme::DAW_AUDIO_YELLOW
                        },
                        false,
                    );

                    if is_target {
                        cell_ui.painter().rect_stroke(
                            cell_rect,
                            3.0,
                            egui::Stroke::new(1.5_f32, theme::BW_LCD_CYAN),
                            egui::StrokeKind::Inside,
                        );
                        if r_vol.clicked() {
                            crate::views::hikaru_native_plugins::hikaru_openmodulation::handle_knob_mapping_click(
                                track_idx,
                                "OpenWavetable",
                                "VOL",
                            );
                        }
                    }

                    if vol_changed {
                        editor.volume = eff_vol;
                    }

                    let vol_pct = (editor.volume * 100.0).round();
                    if let Some(m) = mod_opt {
                        r_vol.on_hover_text(format!(
                            "Volume [LFO Delta]: {:.0}% (Base: {:.0}%, Delta: {:+.2})",
                            eff_vol * 100.0,
                            vol_pct,
                            m.delta
                        ));
                        cell_ui.label(
                            egui::RichText::new("VOL ●")
                                .font(crate::ui::fonts::semibold(8.5))
                                .color(theme::BW_LCD_CYAN),
                        );
                    } else {
                        r_vol.on_hover_text(format!("Master Wavetable Volume: {:.0}%", vol_pct));
                        cell_ui.label(
                            egui::RichText::new("VOL")
                                .font(crate::ui::fonts::semibold(8.5))
                                .color(theme::TEXT_SECONDARY),
                        );
                    }
                }
            }
        }

        // Row 2: GPU Horizontal Morph Bar + Frame Readout
        right_ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
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
                editor.base_position = Some(pos);
                editor.set_position(pos);
            }
            ui.label(
                egui::RichText::new(format!("{}/{}", editor.frame + 1, editor.frame_count()))
                    .font(crate::ui::fonts::lcd(9.5))
                    .color(theme::BW_LCD_CYAN),
            );
        });

        // Row 3: Voice Mode + Octave & Semitone
        right_ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if crate::ui::bitwig_kit::capsule_text_button(
                ui,
                egui::vec2(44.0, 17.0),
                editor.voices.label(),
                9.0,
                editor.voices == VoiceMode::Poly,
                theme::ACCENT_ORANGE,
                crate::ui::bitwig_kit::JoinSide::Standalone,
                false,
            )
            .clicked()
            {
                editor.cycle_voices();
            }

            ui.label(egui::RichText::new("OCT").size(9.0).color(theme::TEXT_SECONDARY));
            let mut oct = editor.octave as i32;
            if ui.add(egui::DragValue::new(&mut oct).range(-3..=3)).changed() {
                editor.octave = oct as i8;
            }

            ui.label(egui::RichText::new("SEMI").size(9.0).color(theme::TEXT_SECONDARY));
            let mut p = editor.pitch as i32;
            if ui.add(egui::DragValue::new(&mut p).range(-24..=24)).changed() {
                editor.pitch = p as i8;
            }
        });

        ui.allocate_rect(body_rect, egui::Sense::hover());
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

pub fn save_openwavetable_preset(editor: &WavetableEditor) {
    let preset_name = if editor.table.name.is_empty() {
        "Custom Wavetable".to_string()
    } else {
        editor.table.name.clone()
    };
    if let Some(path) = rfd::FileDialog::new()
        .set_file_name(&format!("{}.hdp", preset_name))
        .add_filter(hikaru_project_file::HDP_FILE_FILTER, &[hikaru_project_file::HDP_EXTENSION])
        .save_file()
    {
        let slot = hikaru_project_file::DspSlot {
            id: 0,
            name: "OpenWavetable".to_string(),
            active: true,
            wavetable_path: editor.table.path.as_ref().map(|p| p.to_string_lossy().to_string()),
            plugin_path: None,
            plugin_format: None,
            open_wavetable: Some(editor.to_params()),
            open_modulation: None,
        };
        let preset = hikaru_project_file::HikaruDevicePreset {
            format_version: hikaru_project_file::FORMAT_VERSION,
            created_by: format!("Hikaru OpenLive {}", crate::version::VERSION),
            preset_type: hikaru_project_file::PresetType::SingleDevice,
            name: preset_name,
            author: None,
            category: Some("Polysynth / Wavetable".to_string()),
            description: Some("Hikaru OpenWavetable preset".to_string()),
            slot: Some(slot),
            chain: None,
        };
        let _ = hikaru_project_file::save_hdp(&path, &preset);
    }
}

pub fn load_openwavetable_preset(editor: &mut WavetableEditor) {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter(hikaru_project_file::HDP_FILE_FILTER, &[hikaru_project_file::HDP_EXTENSION])
        .pick_file()
    {
        if let Ok(preset) = hikaru_project_file::parse_hdp(&path) {
            if let Some(slot) = preset.slot {
                if let Some(ref path_str) = slot.wavetable_path {
                    if let Some(resolved) = crate::project::common::resolve_wavetable_path(path_str) {
                        if let Ok(table) = load_table_file(&resolved) {
                            editor.load(table);
                        }
                    }
                }
                if let Some(ref p) = slot.open_wavetable {
                    editor.apply_params(p);
                }
            }
        }
    }
}
