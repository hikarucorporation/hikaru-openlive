// Clip Detail Editor Panel (Waveform, Warp, Audio Events)
use std::path::PathBuf;

pub use hikaru_core::{AudioClipEvent, StretchMode};
use crate::app::AppState;
use crate::theme;
use crate::views::matrix::{self, MatrixClip};
use crate::views::playlist;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SnapValue {
    None,
    Bar1,
    Beat1_2,
    Beat1_4,
    Beat1_8,
    Beat1_16,
}

impl SnapValue {
    pub fn label(&self) -> &'static str {
        match self {
            SnapValue::None => "Off (Free)",
            SnapValue::Bar1 => "1 Bar",
            SnapValue::Beat1_2 => "1/2 Beat",
            SnapValue::Beat1_4 => "1/4 Beat",
            SnapValue::Beat1_8 => "1/8 Beat",
            SnapValue::Beat1_16 => "1/16 Beat",
        }
    }

    pub fn interval_secs(&self, bpm: f32) -> f64 {
        let current_bpm = if bpm > 0.0 { bpm as f64 } else { 120.0 };
        let sec_per_beat = 60.0 / current_bpm;

        match self {
            SnapValue::None => 0.0,
            SnapValue::Bar1 => sec_per_beat * 4.0,
            SnapValue::Beat1_2 => sec_per_beat * 2.0,
            SnapValue::Beat1_4 => sec_per_beat,
            SnapValue::Beat1_8 => sec_per_beat / 2.0,
            SnapValue::Beat1_16 => sec_per_beat / 4.0,
        }
    }
}

pub fn event_signature(clip: &MatrixClip) -> Vec<(u64, u64, u64, u32)> {
    clip.audio_events()
        .iter()
        .map(|e| {
            (
                e.id,
                (e.start_secs * 1_000_000.0).round() as u64,
                e.frames(),
                e.gain.to_bits(),
            )
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipEditorTarget {
    Matrix { track_idx: usize, slot_idx: usize },
    Playlist { clip_id: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorTab {
    AudioEvents,
    AudioMarkers,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClipProcessParams {
    pub gain_db: f32,
    pub pan: f32,
    pub pitch_semitones: f32,
    pub stretch_mode: StretchMode,
    pub fade_in_secs: f64,
    pub fade_out_secs: f64,
    pub reverse: bool,
    pub normalize: bool,
}

impl Default for ClipProcessParams {
    fn default() -> Self {
        Self {
            gain_db: 0.0,
            pan: 0.0,
            pitch_semitones: 0.0,
            stretch_mode: StretchMode::Raw,
            fade_in_secs: 0.0,
            fade_out_secs: 0.0,
            reverse: false,
            normalize: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DragAction {
    LoopStart { orig_time: f64 },
    LoopEnd { orig_time: f64 },
    ClipStart { orig_time: f64 },
    ClipEnd { orig_time: f64 },
    GainLine { start_y: f32, orig_gain_db: f32 },
    MarqueeSelection { start_x: f32, start_y: f32, cur_x: f32, cur_y: f32 },
    MiddleDragZoomPan { start_x: f32, start_y: f32, anchor_time: f64, orig_zoom: f32, orig_scroll: f64 },
}

#[derive(Clone, Debug)]
pub struct ClipEditorState {
    pub target: Option<ClipEditorTarget>,
    pub active_target: Option<ClipEditorTarget>,
    pub cached_name: String,
    pub cached_duration_secs: f64,
    pub cached_sample_rate: u32,
    pub cached_gain: f32,
    pub cached_pan: f32,
    pub cached_stretch_mode: StretchMode,
    pub tab: EditorTab,
    pub zoom_x: f32,
    pub scroll_secs: f64,
    pub params: ClipProcessParams,
    pub snap: SnapValue,
    pub loop_start_secs: f64,
    pub loop_end_secs: f64,
    pub loop_enabled: bool,
    pub show_editor: bool,
    pub editor_height: f32,
    pub show_inspector: bool,
    pub drag_action: Option<DragAction>,
    pub selection_range: Option<(f64, f64)>,
}

impl Default for ClipEditorState {
    fn default() -> Self {
        Self {
            target: None,
            active_target: None,
            cached_name: String::new(),
            cached_duration_secs: 0.0,
            cached_sample_rate: 44100,
            cached_gain: 1.0,
            cached_pan: 0.0,
            cached_stretch_mode: StretchMode::Raw,
            tab: EditorTab::AudioEvents,
            zoom_x: DEFAULT_CE_ZOOM,
            scroll_secs: 0.0,
            params: ClipProcessParams::default(),
            snap: SnapValue::Beat1_4,
            loop_start_secs: 0.0,
            loop_end_secs: 0.0,
            loop_enabled: false,
            show_editor: false,
            editor_height: 220.0,
            show_inspector: false,
            drag_action: None,
            selection_range: None,
        }
    }
}

pub const WAVE_MIN_H: f32 = 72.0;
pub const DEFAULT_CE_ZOOM: f32 = 400.0;
pub const MIN_CE_ZOOM: f32 = 0.25;
pub const MAX_CE_ZOOM: f32 = 400.0;

pub fn clamped_ce_zoom(z: f32) -> f32 {
    if !z.is_finite() {
        return DEFAULT_CE_ZOOM;
    }
    z.clamp(MIN_CE_ZOOM, MAX_CE_ZOOM)
}

pub fn step_ce_zoom(z: f32, zoom_in: bool) -> f32 {
    clamped_ce_zoom(if zoom_in { z * 1.25 } else { z / 1.25 })
}

pub fn fit_zoom(duration_secs: f64, viewport_px: f32) -> f32 {
    if duration_secs <= 0.001 || viewport_px <= 1.0 {
        return DEFAULT_CE_ZOOM;
    }
    clamped_ce_zoom((viewport_px as f64 / duration_secs) as f32)
}

pub fn visible_secs(zoom_x: f32, viewport_px: f32) -> f64 {
    let z = clamped_ce_zoom(zoom_x) as f64;
    (viewport_px as f64 / z).max(0.001)
}

pub fn clamp_scroll(scroll_secs: f64, duration_secs: f64, visible: f64) -> f64 {
    let max_scroll = (duration_secs - visible).max(0.0);
    scroll_secs.clamp(0.0, max_scroll)
}

pub fn bar_grid_secs(clip_start_secs: f64, duration_secs: f64, bpm: f32) -> Vec<f64> {
    let current_bpm = if bpm > 0.0 { bpm as f64 } else { 120.0 };
    let bar_dur = (60.0 / current_bpm) * 4.0;
    let mut bars = Vec::new();

    let first_bar_idx = (clip_start_secs / bar_dur).floor() as i64;
    let last_time = clip_start_secs + duration_secs;

    let mut t = first_bar_idx as f64 * bar_dur;
    while t <= last_time + bar_dur {
        if t >= clip_start_secs - 0.001 {
            bars.push(t);
        }
        t += bar_dur;
    }
    bars
}

pub fn bar_number_at(secs: f64, bpm: f32) -> u32 {
    let current_bpm = if bpm > 0.0 { bpm as f64 } else { 120.0 };
    let bar_dur = (60.0 / current_bpm) * 4.0;
    (secs.max(0.0) / bar_dur).floor() as u32 + 1
}

pub static LAST_PAINT_H: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn note_paint_height(h: f32) {
    LAST_PAINT_H.store(h.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

pub fn painted_with_real_height() -> bool {
    let bits = LAST_PAINT_H.load(std::sync::atomic::Ordering::Relaxed);
    f32::from_bits(bits) > 1.0
}

pub fn peaks_are_silent(peaks: &[f32]) -> bool {
    peaks.iter().all(|&p| p.abs() < 1e-4)
}

pub fn best_peaks(detail: &[f32], base: &[f32]) -> Vec<f32> {
    if !detail.is_empty() && !peaks_are_silent(detail) {
        detail.to_vec()
    } else {
        base.to_vec()
    }
}

pub fn ruler_labels(
    clip_start_secs: f64,
    duration_secs: f64,
    bpm: f32,
    _zoom_x: f32,
) -> Vec<(f64, String)> {
    let bars = bar_grid_secs(clip_start_secs, duration_secs, bpm);
    bars.into_iter()
        .map(|t| (t, format!("{}", bar_number_at(t, bpm))))
        .collect()
}

pub fn wave_geometry(peaks: &[f32], width_px: usize, half_h: f32) -> Vec<(f32, f32)> {
    if peaks.is_empty() || width_px == 0 {
        return Vec::new();
    }
    let mut geom = Vec::with_capacity(width_px);
    let step = peaks.len() as f32 / width_px as f32;
    for i in 0..width_px {
        let idx = (i as f32 * step).floor() as usize;
        let peak = peaks.get(idx).copied().unwrap_or(0.0);
        let ph = (peak * half_h).max(1.0);
        geom.push((-ph, ph));
    }
    geom
}

pub fn peaks_window(peaks: &[f32], from_ratio: f32, to_ratio: f32) -> Vec<f32> {
    let n = peaks.len();
    if n == 0 {
        return Vec::new();
    }
    let a = ((from_ratio.clamp(0.0, 1.0)) * n as f32).floor() as usize;
    let b = (((to_ratio.clamp(0.0, 1.0)) * n as f32).ceil() as usize).max(a + 1);
    peaks[a.min(n)..b.min(n).max(a.min(n))].to_vec()
}

#[derive(Clone, Debug)]
pub struct PeakCache {
    pub levels: Vec<Vec<f32>>,
}

impl PeakCache {
    pub fn new(source_peaks: &[f32]) -> Self {
        let mut levels = Vec::new();
        if !source_peaks.is_empty() {
            levels.push(source_peaks.to_vec());
            let mut cur = source_peaks.to_vec();
            while cur.len() > 64 {
                let next_len = cur.len() / 2;
                let mut down = Vec::with_capacity(next_len);
                for i in 0..next_len {
                    let a = cur[i * 2];
                    let b = cur[i * 2 + 1];
                    down.push(if a.abs() > b.abs() { a } else { b });
                }
                levels.push(down.clone());
                cur = down;
            }
        }
        Self { levels }
    }

    pub fn get_peaks_for_width(&self, width_px: usize) -> &[f32] {
        if self.levels.is_empty() {
            return &[];
        }
        for lvl in self.levels.iter().rev() {
            if lvl.len() >= width_px {
                return lvl;
            }
        }
        &self.levels[0]
    }
}

#[derive(Clone, Debug, Default)]
pub struct ClipEditorClip {
    pub name: String,
    pub path: PathBuf,
    pub duration_secs: f64,
    pub peaks: Vec<f32>,
    pub detail_peaks: Vec<f32>,
    pub events: usize,
    pub is_midi: bool,
    pub loop_enabled: bool,
    pub loop_start_secs: f64,
    pub loop_end_secs: f64,
    pub start_secs: f64,
    pub channels: usize,
    pub sample_rate: u32,
    pub event_model: Option<AudioClipEvent>,
}

pub fn resolve_clip(s: &AppState, bpm: f32) -> Option<ClipEditorClip> {
    let target = s.clip_editor.active_target.or(s.clip_editor.target)?;
    match target {
        ClipEditorTarget::Matrix { track_idx, slot_idx } => {
            let slot = s.matrix_state.grid.get(track_idx)?.get(slot_idx)?;
            let clip = slot.clip.as_ref()?;
            let is_midi = matches!(clip.content, matrix::ClipData::Midi { .. });
            let detail = if is_midi {
                Vec::new()
            } else {
                crate::views::hikaru_native_plugins::hikaru_opendms::open_dms_sampler::load_peaks_from_wav(
                    &clip.path.to_string_lossy(),
                    2048,
                )
            };
            let sr = clip
                .audio_events()
                .first()
                .map(|e| e.sample_rate)
                .unwrap_or(48000) as u32;
            let ch = clip
                .audio_events()
                .first()
                .map(|e| e.channels)
                .unwrap_or(2);
            let real_dur = if clip.duration_secs > 0.001 {
                clip.duration_secs
            } else if let Ok(reader) = hound::WavReader::open(&clip.path) {
                let spec = reader.spec();
                let samples = reader.duration();
                if spec.sample_rate > 0 {
                    samples as f64 / spec.sample_rate as f64
                } else {
                    4.0
                }
            } else {
                (60.0 / bpm.max(1.0) as f64) * 4.0
            };

            let start_secs = 0.0;
            let loop_end_secs = if clip.loop_end > 0 {
                start_secs + clip.loop_end as f64 / sr as f64
            } else {
                start_secs + real_dur
            };

            let peaks = if !detail.is_empty() && !peaks_are_silent(&detail) {
                detail.clone()
            } else if !clip.peaks.is_empty() && !peaks_are_silent(&clip.peaks) {
                clip.peaks.clone()
            } else {
                crate::views::hikaru_native_plugins::hikaru_opendms::open_dms_sampler::load_peaks_from_wav(
                    &clip.path.to_string_lossy(),
                    1024,
                )
            };

            Some(ClipEditorClip {
                name: clip.name.clone(),
                path: clip.path.clone(),
                duration_secs: real_dur,
                peaks,
                detail_peaks: detail,
                events: clip.audio_events().len().max(1),
                is_midi,
                loop_enabled: clip.loop_enabled,
                loop_start_secs: start_secs + clip.loop_start as f64 / sr as f64,
                loop_end_secs,
                start_secs,
                channels: ch,
                sample_rate: sr,
                event_model: None,
            })
        }
        ClipEditorTarget::Playlist { clip_id } => {
            let (_, clip) = s.playlist_state.clips.iter().find(|(_, c)| c.id == clip_id)?;
            let (path, _offset_ticks, _total_ticks) = match &clip.clip_type {
                playlist::ClipType::Audio {
                    sample_path,
                    sample_offset_ticks,
                    total_sample_ticks,
                    ..
                } => (
                    PathBuf::from(sample_path),
                    *sample_offset_ticks,
                    *total_sample_ticks,
                ),
                _ => (PathBuf::new(), 0, 0),
            };
            let ppqn = s.playlist_state.ppqn.max(1) as f64;
            let bpm_f = if bpm > 0.0 { bpm as f64 } else { 120.0 };
            let secs_per_tick = (60.0 / bpm_f) / ppqn;
            let start_secs = clip.start_tick as f64 * secs_per_tick;
            let duration_secs = clip.duration_ticks as f64 * secs_per_tick;
            let is_audio = !path.as_os_str().is_empty();
            let detail = if is_audio {
                crate::views::hikaru_native_plugins::hikaru_opendms::open_dms_sampler::load_peaks_from_wav(
                    &path.to_string_lossy(),
                    2048,
                )
            } else {
                Vec::new()
            };
            let peaks = match &clip.clip_type {
                playlist::ClipType::Audio { peaks, .. } => peaks.clone(),
                _ => Vec::new(),
            };

            Some(ClipEditorClip {
                name: clip.name.clone(),
                path,
                duration_secs,
                peaks,
                detail_peaks: detail,
                events: 1,
                is_midi: false,
                loop_enabled: false,
                loop_start_secs: 0.0,
                loop_end_secs: duration_secs,
                start_secs,
                channels: 2,
                sample_rate: 44100,
                event_model: None,
            })
        }
    }
}

pub fn select_target(s: &mut AppState, target: Option<ClipEditorTarget>) {
    s.clip_editor.target = target;
    s.clip_editor.show_editor = target.is_some();
}

/// Render the Clip Editor / Waveform Detail panel in egui.
pub fn render(ui: &mut egui::Ui, app: &mut AppState) {
    let resolved = resolve_clip(app, app.transport.bpm as f32);

    ui.vertical(|ui| {
        // Toolbar header
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("CLIP EDITOR").size(11.0).strong().color(theme::TEXT_MUTED));
            ui.separator();

            if let Some(ref clip) = resolved {
                ui.label(egui::RichText::new(&clip.name).strong().color(theme::ACCENT_PRIMARY));
                ui.label(format!("{:.2} s", clip.duration_secs));

                ui.separator();

                let mut snap_idx = match app.clip_editor.snap {
                    SnapValue::None => 0,
                    SnapValue::Bar1 => 1,
                    SnapValue::Beat1_2 => 2,
                    SnapValue::Beat1_4 => 3,
                    SnapValue::Beat1_8 => 4,
                    SnapValue::Beat1_16 => 5,
                };
                egui::ComboBox::from_id_salt("ce_snap")
                    .selected_text(app.clip_editor.snap.label())
                    .show_index(ui, &mut snap_idx, 6, |i| match i {
                        0 => SnapValue::None.label(),
                        1 => SnapValue::Bar1.label(),
                        2 => SnapValue::Beat1_2.label(),
                        3 => SnapValue::Beat1_4.label(),
                        4 => SnapValue::Beat1_8.label(),
                        _ => SnapValue::Beat1_16.label(),
                    });

                app.clip_editor.snap = match snap_idx {
                    0 => SnapValue::None,
                    1 => SnapValue::Bar1,
                    2 => SnapValue::Beat1_2,
                    3 => SnapValue::Beat1_4,
                    4 => SnapValue::Beat1_8,
                    _ => SnapValue::Beat1_16,
                };
            }
        });

        ui.separator();

        if let Some(ref clip) = resolved {
            let total_width = ui.available_width();
            let total_height = ui.available_height().max(100.0);
            let (rect, _response) = ui.allocate_exact_size(
                egui::vec2(total_width, total_height),
                egui::Sense::click_and_drag(),
            );

            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 4.0, theme::DAW_BG_LAYER_1);

            let peaks = best_peaks(&clip.detail_peaks, &clip.peaks);
            if !peaks.is_empty() {
                let cy = rect.center().y;
                let hh = (rect.height() - 16.0) * 0.45;
                let step = rect.width() / peaks.len() as f32;

                for (i, &p) in peaks.iter().enumerate() {
                    let px = rect.min.x + i as f32 * step;
                    let ph = (p * hh).max(1.0);
                    painter.line_segment(
                        [egui::pos2(px, cy - ph), egui::pos2(px, cy + ph)],
                        egui::Stroke::new(1.0_f32, theme::ACCENT_PRIMARY),
                    );
                }
            }

            // Loop region display
            if clip.loop_enabled && clip.duration_secs > 0.001 {
                let l_start_x = rect.min.x + (clip.loop_start_secs / clip.duration_secs) as f32 * rect.width();
                let l_end_x = rect.min.x + (clip.loop_end_secs / clip.duration_secs) as f32 * rect.width();
                let loop_rect = egui::Rect::from_min_max(
                    egui::pos2(l_start_x.max(rect.min.x), rect.min.y),
                    egui::pos2(l_end_x.min(rect.max.x), rect.max.y),
                );
                painter.rect_stroke(
                    loop_rect,
                    2.0,
                    egui::Stroke::new(1.5_f32, theme::ACCENT_SECONDARY),
                    egui::StrokeKind::Inside,
                );
            }
        } else {
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new("NO CLIP SELECTED\nClick a clip in the Session Matrix or Playlist to edit")
                        .size(12.0)
                        .color(theme::TEXT_MUTED),
                );
            });
        }
    });
}
