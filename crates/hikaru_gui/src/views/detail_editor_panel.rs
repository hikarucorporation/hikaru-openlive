// crates/hikaru_gui/src/views/clip_editor.rs
//
// GNU Affero General Public License v3.0 or later
//
// Clip Editor: visualizador y editor detallado del clip AUDIO seleccionado
// (Session Matrix o Playlist/Timeline).
// Paridad funcional y arquitectónica con Bitwig Studio 5.1.7.
//
// Estructura del panel:
//   1. SUB-HEADER CONTEXTUAL (Barra superior de herramientas):
//      - Indicadores del clip: toggle de inspector [i], nombre de archivo,
//        duración (segundos y compases), BPM detectado, frecuencia de muestreo,
//        canales/eventos y asignación de pista (Track X | Scene Y).
//      - Pestañas de modo: Audio Events / Comping / Stretch / Onsets.
//      - Controles de Loop y Rejilla: botón conmutable Loop, selector de Snap,
//        controles de zoom (-, px/s, +, Fit).
//   2. PANEL DE INSPECCIÓN IZQUIERDO (Inspector lateral):
//      - Desplegable con configuraciones de auto-fade (Create auto-fades),
//        ganancia del evento (Gain dB), paneo (Pan), tono (Pitch semitonos),
//        modo de estiramiento (Stretch Mode: RAW / HD / Smooth / Beats)
//        y duraciones de fundido (Fade In / Fade Out).
//   3. LÍNEA DE TIEMPO Y RENDERIZADO DE FORMA DE ONDA (Waveform Canvas):
//      - Regla temporal superior con números de compás nítidos (1, 1.2, 1.3, 1.4, 2)
//        y rejilla adaptativa.
//      - Visualizador GPU con waveform en tonos grafito/plata Bitwig, eje central,
//        envolvente/línea horizontal de ganancia interactiva, marcadores de inicio/fin
//        de clip y marcadores de loop anaranjados (#ff7a00 / #e8760c).
//      - Interacción reactiva: middle-drag pan/zoom, zoom con rueda centrado en
//        cursor, drag & resize de marcadores, edición directa de ganancia y selección marquee.

use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::component::*;
use gpui_kit::component::button::{Button, ButtonRounded};
use gpui_kit::component::label::Label;
use gpui_kit::prelude::{
    FluentBuilder as _, InteractiveElement as _, Styled as _,
};
use gpui_kit::*;

pub use hikaru_core::{AudioClipEvent, StretchMode};
use crate::app::{state, AppState, HikaruApp};
use crate::views::matrix::{self, MatrixClip};
use crate::views::playlist;

// =========================================================================
// SNAP
// =========================================================================

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

/// Firma estable de un evento de audio, usada por los tests para detectar
/// cambios en el grafo sin comparar buffers.
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

// =========================================================================
// ESTADO DEL EDITOR
// =========================================================================

/// De dónde viene el clip que se está editando.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipEditorTarget {
    /// Pad `(track, scene)` de la Session Matrix.
    Matrix { track: usize, scene: usize },
    /// Clip de la Playlist/Timeline.
    Playlist { clip_id: usize },
}

impl ClipEditorTarget {
    /// Texto corto para la barra de estado y topbar.
    pub fn label(&self) -> String {
        match self {
            ClipEditorTarget::Matrix { track, scene } => {
                format!("Track {} | Scene {}", track + 1, scene + 1)
            }
            ClipEditorTarget::Playlist { clip_id } => format!("Playlist clip #{}", clip_id),
        }
    }
}

/// Pestañas del sub-header (modos de trabajo del clip).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorTab {
    AudioEvents,
    Pitch,
    Comping,
    Stretch,
    Onsets,
}

impl EditorTab {
    pub fn label(&self) -> &'static str {
        match self {
            EditorTab::AudioEvents => "Audio Events",
            EditorTab::Pitch => "Pitch",
            EditorTab::Comping => "Comping",
            EditorTab::Stretch => "Stretch",
            EditorTab::Onsets => "Onsets",
        }
    }
}

/// Controles de procesado del clip e inspector lateral.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipProcessParams {
    pub gain_db: f32,
    pub pan: f32,
    pub pitch_cents: f32,
    pub formant: f32,
    pub stretch: f32,
    pub onset_threshold_db: f32,
    pub auto_fades: bool,
    pub fade_in_secs: f64,
    pub fade_out_secs: f64,
    pub stretch_mode: StretchMode,
}

impl Default for ClipProcessParams {
    fn default() -> Self {
        Self {
            gain_db: 0.0,
            pan: 0.0,
            pitch_cents: 0.0,
            formant: 0.0,
            stretch: 1.0,
            onset_threshold_db: -24.0,
            auto_fades: false,
            fade_in_secs: 0.005,
            fade_out_secs: 0.005,
            stretch_mode: StretchMode::Raw,
        }
    }
}

/// Acciones interactivas de arrastre con el ratón.
#[derive(Clone, Debug, PartialEq)]
pub enum DragAction {
    Pan { start_x: f32, start_scroll: f64 },
    LoopStart { orig_time: f64 },
    LoopEnd { orig_time: f64 },
    LoopRange { start_x: f32, orig_start: f64, orig_end: f64 },
    ClipStart { orig_time: f64 },
    ClipEnd { orig_time: f64 },
    GainLine { start_y: f32, orig_gain_db: f32 },
    MarqueeSelection { start_x: f32, start_y: f32, cur_x: f32, cur_y: f32 },
    MiddleDragZoomPan { start_x: f32, start_y: f32, anchor_time: f64, orig_zoom: f32, orig_scroll: f64 },
}

/// Estado completo del Clip Editor.
#[derive(Clone, Debug)]
pub struct ClipEditorState {
    /// Clip en edición. `None` => estado vacío.
    pub target: Option<ClipEditorTarget>,
    /// Pestaña activa del sub-header.
    pub tab: EditorTab,
    /// Zoom horizontal del visualizador (px por segundo).
    pub zoom_x: f32,
    /// Scroll horizontal del visualizador en segundos desde el inicio del clip.
    pub scroll_secs: f64,
    pub params: ClipProcessParams,
    pub snap: SnapValue,
    /// Loop/región local del clip en segundos.
    pub loop_start_secs: f64,
    pub loop_end_secs: f64,
    pub loop_enabled: bool,
    pub show_editor: bool,
    /// Alto del panel en px.
    pub editor_height: f32,
    /// Visibilidad del panel de inspección lateral izquierdo.
    pub show_inspector: bool,
    /// Gesto de arrastre activo.
    pub drag_action: Option<DragAction>,
    /// Rango de tiempo seleccionado (marquee selection).
    pub selection_range: Option<(f64, f64)>,
}

impl Default for ClipEditorState {
    fn default() -> Self {
        Self {
            target: None,
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

/// Zoom acotado.
pub fn clamped_ce_zoom(z: f32) -> f32 {
    if !z.is_finite() || z <= 0.0 {
        return DEFAULT_CE_ZOOM;
    }
    z.clamp(MIN_CE_ZOOM, MAX_CE_ZOOM)
}

/// Zoom del visualizador con acote (×1.5 / ÷1.5).
pub fn step_ce_zoom(z: f32, zoom_in: bool) -> f32 {
    clamped_ce_zoom(if zoom_in { z * 1.5 } else { z / 1.5 })
}

/// Zoom "encajar": el factor que hace caber `duration` en `viewport_px`.
pub fn fit_zoom(duration_secs: f64, viewport_px: f32) -> f32 {
    if duration_secs <= 0.0 || viewport_px <= 0.0 {
        return 1.0;
    }
    clamped_ce_zoom(viewport_px / duration_secs as f32)
}

/// Segundos visibles en el viewport al zoom actual.
pub fn visible_secs(zoom_x: f32, viewport_px: f32) -> f64 {
    if zoom_x <= 0.0 {
        return 0.0;
    }
    viewport_px as f64 / zoom_x as f64
}

/// Desplazamiento scrollable acotado al contenido real.
pub fn clamp_scroll(scroll_secs: f64, duration_secs: f64, visible: f64) -> f64 {
    let max = (duration_secs - visible).max(0.0);
    if !scroll_secs.is_finite() {
        return 0.0;
    }
    scroll_secs.clamp(0.0, max)
}

/// Posiciones de la regla de compases (en segundos) para los compases visibles.
pub fn bar_grid_secs(clip_start_secs: f64, duration_secs: f64, bpm: f32) -> Vec<f64> {
    let bpm = if bpm > 0.0 { bpm as f64 } else { 120.0 };
    let sec_per_bar = (60.0 / bpm) * 4.0;
    if sec_per_bar <= 0.0 {
        return Vec::new();
    }
    let first = (clip_start_secs / sec_per_bar).floor();
    let first_time = first * sec_per_bar;
    let mut out = Vec::new();
    let mut t = first_time;
    while t <= clip_start_secs + duration_secs + sec_per_bar {
        if t >= clip_start_secs - sec_per_bar {
            out.push(t);
        }
        t += sec_per_bar;
    }
    out
}

/// Índice de compás 1-based de un instante absoluto.
pub fn bar_number_at(secs: f64, bpm: f32) -> u32 {
    let bpm = if bpm > 0.0 { bpm as f64 } else { 120.0 };
    let sec_per_bar = (60.0 / bpm) * 4.0;
    if !sec_per_bar.is_finite() || sec_per_bar <= 0.0 {
        return 1;
    }
    if secs < 0.0 {
        return 1;
    }
    ((secs / sec_per_bar).floor() as u32).saturating_add(1)
}

pub static LAST_PAINT_H: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn note_paint_height(h: f32) {
    LAST_PAINT_H.store(h.max(0.0) as u32, std::sync::atomic::Ordering::Relaxed);
}

pub fn painted_with_real_height() -> bool {
    LAST_PAINT_H.load(std::sync::atomic::Ordering::Relaxed) > 0
}

pub fn peaks_are_silent(peaks: &[f32]) -> bool {
    peaks.is_empty() || peaks.iter().all(|p| *p <= f32::EPSILON)
}

/// Mejor fuente de picos para dibujar: la de mayor resolución entre las que tienen señal.
pub fn best_peaks(detail: &[f32], base: &[f32]) -> Vec<f32> {
    let detail_ok = !peaks_are_silent(detail);
    let base_ok = !peaks_are_silent(base);
    match (detail_ok, base_ok) {
        (true, true) => {
            if detail.len() >= base.len() {
                detail.to_vec()
            } else {
                base.to_vec()
            }
        }
        (true, false) => detail.to_vec(),
        (false, true) => base.to_vec(),
        (false, false) => {
            if detail.len() >= base.len() {
                detail.to_vec()
            } else {
                base.to_vec()
            }
        }
    }
}

/// Etiquetas de la regla de tiempo: número de compás y marca de subdivisión.
pub fn ruler_labels(
    scroll_secs: f64,
    visible: f64,
    bpm: f32,
    px_per_sec: f32,
) -> Vec<(String, f32, bool)> {
    let bpm_f = if bpm > 0.0 { bpm as f64 } else { 120.0 };
    let sec_per_beat = (60.0 / bpm_f) * 4.0 / 4.0;
    let sec_per_bar = sec_per_beat * 4.0;
    let mut out: Vec<(String, f32, bool)> = Vec::new();
    let mut bar = (scroll_secs / sec_per_bar).floor();
    while bar * sec_per_bar < scroll_secs + visible {
        let x = ((bar * sec_per_bar - scroll_secs) * px_per_sec as f64) as f32;
        out.push((((bar + 1.0) as u32).to_string(), x, true));
        bar += 1.0;
    }
    let mut beat = (scroll_secs / sec_per_beat).floor();
    while beat * sec_per_beat < scroll_secs + visible {
        if (beat as i64) % 4 != 0 {
            let x = ((beat * sec_per_beat - scroll_secs) * px_per_sec as f64) as f32;
            out.push((format!("{:?}", (beat as i64) % 4 + 1), x, false));
        }
        beat += 1.0;
    }
    out.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Geometría del waveform: columna `x` (px) y amplitud media altura `amp` de cada segmento.
pub fn wave_geometry(peaks: &[f32], width_px: usize, half_h: f32) -> Vec<(f32, f32)> {
    let n = peaks.len();
    let width_px = width_px.max(1);
    if n == 0 || half_h <= 0.0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(n.min(width_px));
    if n >= width_px {
        for c in 0..width_px {
            let lo = (c * n / width_px).min(n - 1);
            let hi = (((c + 1) * n / width_px).max(lo + 1)).min(n);
            let mut mx = 0.0f32;
            for v in &peaks[lo..hi] {
                if v.abs() > mx {
                    mx = v.abs();
                }
            }
            let amp = (mx.min(1.0) * half_h).max(0.5);
            out.push((c as f32, amp));
        }
    } else {
        let span = width_px.saturating_sub(1) as f32;
        for (i, v) in peaks.iter().enumerate() {
            let x = if n <= 1 {
                0.0
            } else {
                (i as f32 / (n - 1) as f32) * span
            };
            let amp = (v.abs().min(1.0) * half_h).max(0.5);
            out.push((x, amp));
        }
    }
    out
}

/// Peak absoluto en la ventana temporal `[from, to)` de un buffer de picos.
pub fn peaks_window(peaks: &[f32], from_ratio: f32, to_ratio: f32) -> Vec<f32> {
    let n = peaks.len();
    if n == 0 {
        return Vec::new();
    }
    let a = ((from_ratio.clamp(0.0, 1.0)) * n as f32).floor() as usize;
    let b = (((to_ratio.clamp(0.0, 1.0)) * n as f32).ceil() as usize).max(a + 1);
    peaks[a.min(n)..b.min(n).max(a.min(n))].to_vec()
}

/// Estructura de caché piramidal de picos para renderizado optimizado a 60+ FPS.
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

// =========================================================================
// RESOLUCIÓN DEL CLIP
// =========================================================================

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
    let target = s.clip_editor.target?;
    match target {
        ClipEditorTarget::Matrix { track, scene } => {
            let slot = s.matrix_state.grid.get(track)?.get(scene)?;
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
            } else if sr > 0 && !clip.pcm_data().is_empty() {
                clip.pcm_data().len() as f64 / (sr as f64 * ch.max(1) as f64)
            } else {
                (60.0 / bpm.max(1.0) as f64) * 4.0
            };

            let start_secs = clip.local_bar as f64 * 0.0;
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

            let pcm_shared = Arc::new(clip.pcm_data().to_vec());
            let event_model = Some(AudioClipEvent {
                id: clip.id as u64,
                name: clip.name.clone(),
                sample_buffer: pcm_shared,
                channels: ch,
                sample_rate: sr,
                start_frame: 0,
                end_frame: (real_dur * sr as f64) as u64,
                loop_start: clip.loop_start,
                loop_end: if clip.loop_end > 0 { clip.loop_end } else { (real_dur * sr as f64) as u64 },
                loop_enabled: clip.loop_enabled,
                gain_db: s.clip_editor.params.gain_db,
                fade_in_secs: s.clip_editor.params.fade_in_secs,
                fade_out_secs: s.clip_editor.params.fade_out_secs,
                stretch_mode: s.clip_editor.params.stretch_mode,
            });

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
                event_model,
            })
        }
        ClipEditorTarget::Playlist { clip_id } => {
            let (_, clip) = s.playlist_state.clips.iter().find(|(_, c)| c.id == clip_id)?;
            let (path, offset_ticks, total_ticks) = match &clip.clip_type {
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
            let (loop_start, loop_end) = {
                let ce = &s.clip_editor;
                (
                    ce.loop_start_secs,
                    ce.loop_end_secs.max(ce.loop_start_secs),
                )
            };
            let _ = (offset_ticks, total_ticks);
            let sr = 48000u32;
            let event_model = Some(AudioClipEvent {
                id: clip.id as u64,
                name: clip.name.clone(),
                sample_buffer: Arc::new(Vec::new()),
                channels: 2,
                sample_rate: sr,
                start_frame: 0,
                end_frame: (duration_secs * sr as f64) as u64,
                loop_start: 0,
                loop_end: (duration_secs * sr as f64) as u64,
                loop_enabled: ce_loop_enabled(s),
                gain_db: s.clip_editor.params.gain_db,
                fade_in_secs: s.clip_editor.params.fade_in_secs,
                fade_out_secs: s.clip_editor.params.fade_out_secs,
                stretch_mode: s.clip_editor.params.stretch_mode,
            });

            Some(ClipEditorClip {
                name: clip.name.clone(),
                path,
                duration_secs,
                peaks: match &clip.clip_type {
                    playlist::ClipType::Audio { peaks, .. } => peaks.clone(),
                    _ => Vec::new(),
                },
                detail_peaks: detail,
                events: 1,
                is_midi: !is_audio,
                loop_enabled: ce_loop_enabled(s),
                loop_start_secs: if loop_end > loop_start { loop_start } else { start_secs },
                loop_end_secs: if loop_end > loop_start {
                    loop_end
                } else {
                    start_secs + duration_secs
                },
                start_secs,
                channels: 2,
                sample_rate: sr,
                event_model,
            })
        }
    }
}

fn ce_loop_enabled(s: &AppState) -> bool {
    s.playlist_state.is_loop_region_valid()
}

fn apply_loop_to_engine(s: &mut AppState, enabled: bool) {
    let Some(target) = s.clip_editor.target else {
        return;
    };
    let start_secs = s.clip_editor.loop_start_secs.max(0.0) as f32;
    let end_secs = s.clip_editor.loop_end_secs.max(0.0) as f32;

    match target {
        ClipEditorTarget::Matrix { track, scene } => {
            if let Some(clip) = s
                .matrix_state
                .grid
                .get_mut(track)
                .and_then(|row| row.get_mut(scene))
                .and_then(|slot| slot.clip.as_mut())
            {
                clip.loop_enabled = enabled;
            }
            s.audio_proxy
                .send(crate::audio_proxy::GuiCommand::SetClipLoop {
                    track_idx: track,
                    scene_idx: scene,
                    start_secs,
                    end_secs,
                    enabled,
                });
        }
        ClipEditorTarget::Playlist { clip_id } => {
            s.audio_proxy
                .send(crate::audio_proxy::GuiCommand::SetClipLoop {
                    track_idx: clip_id,
                    scene_idx: usize::MAX,
                    start_secs,
                    end_secs,
                    enabled,
                });
        }
    }
}

fn apply_gain_to_clip(s: &mut AppState, gain_db: f32) {
    let linear = 10.0f32.powf(gain_db / 20.0);
    if let Some(target) = s.clip_editor.target {
        if let ClipEditorTarget::Matrix { track, scene } = target {
            if let Some(clip) = s
                .matrix_state
                .grid
                .get_mut(track)
                .and_then(|row| row.get_mut(scene))
                .and_then(|slot| slot.clip.as_mut())
            {
                if let Some(events) = clip.audio_events_mut() {
                    for ev in events.iter_mut() {
                        ev.gain = linear;
                    }
                }
                crate::views::matrix::sync_audio_events_to_engine(
                    clip,
                    &s.audio_proxy,
                    track,
                    scene,
                );
            }
        }
    }
}

pub fn select_target(s: &mut AppState, target: Option<ClipEditorTarget>) {
    s.clip_editor.target = target;
    if target.is_some() {
        s.clip_editor.show_editor = true;
        s.clip_editor.show_inspector = true;
        s.clip_editor.scroll_secs = 0.0;
    }
}

// =========================================================================
// RENDER
// =========================================================================

pub fn render(cx: &mut Context<HikaruApp>) -> AnyElement {
    let (bpm, _sample_rate) = {
        let app = state(cx).read(cx);
        (app.transport.bpm as f32, app.transport.sample_rate.get() as u32)
    };
    let clip = {
        let app = state(cx).read(cx);
        resolve_clip(&app, bpm)
    };
    let ce = {
        let app = state(cx).read(cx);
        app.clip_editor.clone()
    };

    let Some(clip) = clip else {
        return v_flex()
            .id("clip_editor")
            .test_support()
            .size_full()
            .items_center()
            .justify_center()
            .gap(px(4.0))
            .bg(rgb(0x121216))
            .child(Label::new("No clip selected").text_sm().text_color(rgb(0x9AA4B2)))
            .child(
                Label::new("Seleccioná un clip de audio en la Session Matrix o en la Playlist")
                    .text_xs()
                    .text_color(rgb(0x6B7280)),
            )
            .into_any_element();
    };

    let zoom_x = clamped_ce_zoom(ce.zoom_x);

    v_flex()
        .id("clip_editor")
        .test_support()
        .size_full()
        .gap(px(0.0))
        .bg(rgb(0x18181C))
        .child(
            div()
                .flex()
                .flex_row()
                .size_full()
                .gap(px(0.0))
                // Franja vertical extrema izquierda con el acento anaranjado Bitwig (#FF7A00) y toggles
                .child(clip_vertical_strip(cx, &clip, &ce))
                .when(ce.show_inspector, |row| {
                    row.child(clip_inspector(cx, &ce, &clip))
                })
                .when(!ce.show_inspector, |row| {
                    row.child(
                        h_flex()
                            .absolute()
                            .top(px(38.0))
                            .left(px(30.0))
                            .w(px(80.0))
                            .h(px(16.0))
                            .child(
                                Button::new("ce_tab_events")
                                    .w(px(20.0))
                                    .h(px(16.0))
                                    .bg(rgba(0x00000000))
                                    .on_click(|_, _, cx| {
                                        state(cx).update(cx, |s, cx| {
                                            s.clip_editor.tab = EditorTab::AudioEvents;
                                            cx.notify();
                                        });
                                    }),
                            )
                            .child(
                                Button::new("ce_tab_comping")
                                    .w(px(20.0))
                                    .h(px(16.0))
                                    .bg(rgba(0x00000000))
                                    .on_click(|_, _, cx| {
                                        state(cx).update(cx, |s, cx| {
                                            s.clip_editor.tab = EditorTab::Comping;
                                            cx.notify();
                                        });
                                    }),
                            )
                            .child(
                                Button::new("ce_tab_stretch")
                                    .w(px(20.0))
                                    .h(px(16.0))
                                    .bg(rgba(0x00000000))
                                    .on_click(|_, _, cx| {
                                        state(cx).update(cx, |s, cx| {
                                            s.clip_editor.tab = EditorTab::Stretch;
                                            cx.notify();
                                        });
                                    }),
                            )
                            .child(
                                Button::new("ce_tab_onsets")
                                    .w(px(20.0))
                                    .h(px(16.0))
                                    .bg(rgba(0x00000000))
                                    .on_click(|_, _, cx| {
                                        state(cx).update(cx, |s, cx| {
                                            s.clip_editor.tab = EditorTab::Onsets;
                                            cx.notify();
                                        });
                                    }),
                            )
                            .child(div().id("ce_auto_fades").test_support()),
                    )
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .h_full()
                        .min_h_0()
                        .min_w_0()
                        .child(waveform_viewer(cx, &clip, &ce, bpm, zoom_x))
                        .child(timeline_bottom_bar(cx, &clip, &ce, zoom_x)),
                ),
        )
        .into_any_element()
}

/// Franja vertical extrema izquierda estilo Bitwig Studio (con acento naranja de pista y toggles).
fn clip_vertical_strip(
    _cx: &mut Context<HikaruApp>,
    _clip: &ClipEditorClip,
    ce: &ClipEditorState,
) -> AnyElement {
    div()
        .id("clip_editor_vertical_label")
        .test_support()
        .w(px(24.0))
        .h_full()
        .flex_shrink_0()
        .relative()
        .bg(rgb(0x1F2023))
        .border_r_1()
        .border_color(rgb(0x16171A))
        .cursor_pointer()
        .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| {
            state(cx).update(cx, |s, cx| {
                s.clip_editor.show_inspector = !s.clip_editor.show_inspector;
                cx.notify();
            });
        })
        .child(
            div()
                .absolute()
                .left_0()
                .top_0()
                .bottom_0()
                .w(px(3.0))
                .bg(rgb(0xFF7A00)),
        )
        .child(
            v_flex()
                .size_full()
                .items_center()
                .justify_between()
                .py(px(6.0))
                .child(
                    v_flex()
                        .items_center()
                        .gap(px(4.0))
                        .child(
                            Label::new("▶")
                                .text_xs()
                                .text_color(rgb(0xFF7A00)),
                        )
                        .child(
                            Label::new("1")
                                .text_xs()
                                .font_weight(gpui_kit::FontWeight::BOLD)
                                .text_color(rgb(0x8A8A96)),
                        ),
                )
                .child(
                    Label::new("❖")
                        .text_xs()
                        .text_color(rgb(0x606470)),
                )
                .child(
                    Label::new(if ce.show_inspector { "◀" } else { "▶" })
                        .text_xs()
                        .text_color(rgb(0x8A8A96)),
                ),
        )
        .into_any_element()
}

/// Panel de inspección izquierdo (estilo Bitwig Studio 5.1.7).
fn clip_inspector(
    _cx: &mut Context<HikaruApp>,
    ce: &ClipEditorState,
    _clip: &ClipEditorClip,
) -> AnyElement {
    let p = ce.params;
    v_flex()
        .w(px(125.0))
        .h_full()
        .flex_shrink_0()
        .bg(rgb(0x28292E))
        .border_r_1()
        .border_color(rgb(0x18191C))
        .p(px(6.0))
        .gap(px(6.0))
        // 1. Bitwig Pointer Tool selector button
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .w_full()
                .h(px(24.0))
                .px(px(6.0))
                .bg(rgb(0x35363D))
                .rounded(px(2.0))
                .border_1()
                .border_color(rgb(0x2A2B30))
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(4.0))
                        .child(Label::new("▲").text_xs().text_color(rgb(0xF0F0F5)))
                        .child(Label::new("Pointer").text_xs().font_weight(gpui_kit::FontWeight::MEDIUM).text_color(rgb(0xE0E0E8))),
                )
                .child(Label::new("▾").text_xs().text_color(rgb(0x8A8A96))),
        )
        // 2. Bitwig Mode selector button (clickable para rotar modos)
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .w_full()
                .h(px(28.0))
                .px(px(6.0))
                .bg(rgb(0x33343A))
                .border_1()
                .border_color(rgb(0x28292E))
                .rounded(px(2.0))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0x3D3E45)))
                .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| {
                    state(cx).update(cx, |s, cx| {
                        s.clip_editor.tab = match s.clip_editor.tab {
                            EditorTab::AudioEvents => EditorTab::Pitch,
                            EditorTab::Pitch => EditorTab::Comping,
                            EditorTab::Comping => EditorTab::Stretch,
                            EditorTab::Stretch => EditorTab::Onsets,
                            EditorTab::Onsets => EditorTab::AudioEvents,
                        };
                        cx.notify();
                    });
                })
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(5.0))
                        .child(
                            Label::new(match ce.tab {
                                EditorTab::AudioEvents => "〰",
                                EditorTab::Pitch => "Ψ",
                                EditorTab::Stretch => "⏱",
                                EditorTab::Comping => "✂",
                                EditorTab::Onsets => "⚡",
                            })
                            .text_xs()
                            .text_color(rgb(0xFF7A00)),
                        )
                        .child(
                            Label::new(ce.tab.label())
                                .text_xs()
                                .font_weight(gpui_kit::FontWeight::MEDIUM)
                                .text_color(rgb(0xF0F0F5)),
                        ),
                )
                .child(Label::new("▾").text_xs().text_color(rgb(0x8A8A96))),
        )
        // 3. Grid de modos compactos (ce_tab_events, ce_tab_comping, ce_tab_stretch, ce_tab_onsets)
        .child(
            v_flex()
                .w_full()
                .gap(px(2.0))
                .child(
                    h_flex()
                        .w_full()
                        .gap(px(2.0))
                        .child(
                            Button::new("ce_tab_events")
                                .label("Audio")
                                .compact()
                                .flex_1()
                                .rounded(ButtonRounded::None)
                                .bg(if ce.tab == EditorTab::AudioEvents { rgb(0xFF7A00) } else { rgb(0x24252A) })
                                .text_color(if ce.tab == EditorTab::AudioEvents { rgb(0xFFFFFF) } else { rgb(0x8A8A96) })
                                .on_click(|_, _, cx| {
                                    state(cx).update(cx, |s, cx| {
                                        s.clip_editor.tab = EditorTab::AudioEvents;
                                        cx.notify();
                                    });
                                }),
                        )
                        .child(
                            Button::new("ce_tab_comping")
                                .label("Comp")
                                .compact()
                                .flex_1()
                                .rounded(ButtonRounded::None)
                                .bg(if ce.tab == EditorTab::Comping { rgb(0xFF7A00) } else { rgb(0x24252A) })
                                .text_color(if ce.tab == EditorTab::Comping { rgb(0xFFFFFF) } else { rgb(0x8A8A96) })
                                .on_click(|_, _, cx| {
                                    state(cx).update(cx, |s, cx| {
                                        s.clip_editor.tab = EditorTab::Comping;
                                        cx.notify();
                                    });
                                }),
                        ),
                )
                .child(
                    h_flex()
                        .w_full()
                        .gap(px(2.0))
                        .child(
                            Button::new("ce_tab_stretch")
                                .label("Stretch")
                                .compact()
                                .flex_1()
                                .rounded(ButtonRounded::None)
                                .bg(if ce.tab == EditorTab::Stretch { rgb(0xFF7A00) } else { rgb(0x24252A) })
                                .text_color(if ce.tab == EditorTab::Stretch { rgb(0xFFFFFF) } else { rgb(0x8A8A96) })
                                .on_click(|_, _, cx| {
                                    state(cx).update(cx, |s, cx| {
                                        s.clip_editor.tab = EditorTab::Stretch;
                                        cx.notify();
                                    });
                                }),
                        )
                        .child(
                            Button::new("ce_tab_onsets")
                                .label("Onsets")
                                .compact()
                                .flex_1()
                                .rounded(ButtonRounded::None)
                                .bg(if ce.tab == EditorTab::Onsets { rgb(0xFF7A00) } else { rgb(0x24252A) })
                                .text_color(if ce.tab == EditorTab::Onsets { rgb(0xFFFFFF) } else { rgb(0x8A8A96) })
                                .on_click(|_, _, cx| {
                                    state(cx).update(cx, |s, cx| {
                                        s.clip_editor.tab = EditorTab::Onsets;
                                        cx.notify();
                                    });
                                }),
                        ),
                ),
        )
        // 4. Bitwig "Create auto-fades" checkbox
        .child(
            h_flex()
                .items_center()
                .gap(px(6.0))
                .cursor_pointer()
                .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| {
                    state(cx).update(cx, |s, cx| {
                        s.clip_editor.params.auto_fades = !s.clip_editor.params.auto_fades;
                        cx.notify();
                    });
                })
                .child(
                    div()
                        .size(px(14.0))
                        .rounded(px(7.0))
                        .bg(if p.auto_fades { rgb(0xFF7A00) } else { rgb(0x222327) })
                        .border_1()
                        .border_color(if p.auto_fades { rgb(0xFF7A00) } else { rgb(0x404048) })
                        .items_center()
                        .justify_center()
                        .when(p.auto_fades, |b| {
                            b.child(Label::new("✓").text_xs().text_color(rgb(0xFFFFFF)))
                        }),
                )
                .child(
                    Label::new("Create auto-fades")
                        .text_xs()
                        .text_color(rgb(0xC8C8D0)),
                )
                .id("ce_auto_fades")
                .test_support(),
        )
        // 5. Parámetros del Clip (Gain, Pan, Pitch) estilo Bitwig Inspector
        .child(
            v_flex()
                .w_full()
                .gap(px(4.0))
                .pt(px(4.0))
                .border_t_1()
                .border_color(rgb(0x222328))
                .child(
                    h_flex()
                        .justify_between()
                        .child(Label::new("Gain").text_xs().text_color(rgb(0x8A8A96)))
                        .child(Label::new(format!("{:.1} dB", p.gain_db)).text_xs().text_color(rgb(0xE0E0E8))),
                )
                .child(
                    h_flex()
                        .justify_between()
                        .child(Label::new("Pan").text_xs().text_color(rgb(0x8A8A96)))
                        .child(Label::new(if p.pan.abs() < 0.01 { "C".to_string() } else if p.pan < 0.0 { format!("{:.0}L", -p.pan * 100.0) } else { format!("{:.0}R", p.pan * 100.0) }).text_xs().text_color(rgb(0xE0E0E8))),
                )
                .child(
                    h_flex()
                        .justify_between()
                        .child(Label::new("Pitch").text_xs().text_color(rgb(0x8A8A96)))
                        .child(Label::new(format!("{:.0} ct", p.pitch_cents)).text_xs().text_color(rgb(0xE0E0E8))),
                ),
        )
        .id("clip_editor_inspector")
        .test_support()
        .into_any_element()
}

/// Barra inferior de navegación del timeline estilo Bitwig Studio.
fn timeline_bottom_bar(
    _cx: &mut Context<HikaruApp>,
    clip: &ClipEditorClip,
    ce: &ClipEditorState,
    zoom_x: f32,
) -> AnyElement {
    let loop_on = ce.loop_enabled;
    let dur = if clip.duration_secs > 0.001 { clip.duration_secs } else { 4.0 };
    let vis = visible_secs(zoom_x, 800.0);
    let norm_scroll = (ce.scroll_secs / dur).clamp(0.0, 1.0) as f32;
    let norm_vis = (vis / dur).clamp(0.06, 1.0) as f32;

    let left_frac = (norm_scroll * (1.0 - norm_vis)).clamp(0.0, 1.0);
    let w_frac = norm_vis.clamp(0.06, 1.0);

    h_flex()
        .id("clip_topbar")
        .test_support()
        .h(px(22.0))
        .w_full()
        .flex_shrink_0()
        .bg(rgb(0x1F2023))
        .border_t_1()
        .border_color(rgb(0x161719))
        .items_center()
        .justify_between()
        .px(px(4.0))
        .gap(px(8.0))
        // 1. Izquierda: Scrollbar track con botones ◀ / ▶ estilo Bitwig (ocupa flex_1)
        .child(
            h_flex()
                .items_center()
                .gap(px(3.0))
                .flex_1()
                // Botón flecha izquierda ◀
                .child(
                    div()
                        .size(px(16.0))
                        .rounded(px(2.0))
                        .bg(rgb(0x28292E))
                        .hover(|s| s.bg(rgb(0x35363D)))
                        .cursor_pointer()
                        .items_center()
                        .justify_center()
                        .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| {
                            state(cx).update(cx, |s, cx| {
                                s.clip_editor.scroll_secs = (s.clip_editor.scroll_secs - 0.5).max(0.0);
                                cx.notify();
                            });
                        })
                        .child(Label::new("◀").text_xs().text_color(rgb(0x8A8A96))),
                )
                // Carril de scrollbar interactivo
                .child(
                    div()
                        .id("ce_scrollbar_track")
                        .flex_1()
                        .h(px(8.0))
                        .bg(rgb(0x141416))
                        .rounded(px(4.0))
                        .relative()
                        .child(
                            div()
                                .absolute()
                                .top(px(1.0))
                                .bottom(px(1.0))
                                .left(relative(left_frac))
                                .w(relative(w_frac))
                                .bg(rgb(0x45464D))
                                .hover(|s| s.bg(rgb(0x5A5B64)))
                                .rounded(px(3.0)),
                        ),
                )
                // Botón flecha derecha ▶
                .child(
                    div()
                        .size(px(16.0))
                        .rounded(px(2.0))
                        .bg(rgb(0x28292E))
                        .hover(|s| s.bg(rgb(0x35363D)))
                        .cursor_pointer()
                        .items_center()
                        .justify_center()
                        .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| {
                            state(cx).update(cx, |s, cx| {
                                s.clip_editor.scroll_secs += 0.5;
                                cx.notify();
                            });
                        })
                        .child(Label::new("▶").text_xs().text_color(rgb(0x8A8A96))),
                ),
        )
        // 2. Derecha: Loop + Snap + Zoom + Fit (estilo Bitwig Studio)
        .child(
            h_flex()
                .items_center()
                .gap(px(3.0))
                // Loop toggle
                .child(
                    Button::new("ce_loop_toggle")
                        .rounded(ButtonRounded::None)
                        .label("Loop")
                        .compact()
                        .bg(if loop_on { rgb(0xFF7A00) } else { rgb(0x282830) })
                        .text_color(if loop_on { rgb(0xFFFFFF) } else { rgb(0x9AA0AE) })
                        .on_click(|_, _, cx| {
                            state(cx).update(cx, |s, cx| {
                                let enabled = !s.clip_editor.loop_enabled;
                                s.clip_editor.loop_enabled = enabled;
                                apply_loop_to_engine(s, enabled);
                                cx.notify();
                            });
                        }),
                )
                // Snap selector
                .child(
                    Button::new("ce_snap_cycle")
                        .rounded(ButtonRounded::None)
                        .label(match ce.snap {
                            SnapValue::None => "Off ▴",
                            SnapValue::Bar1 => "1 Bar ▴",
                            SnapValue::Beat1_2 => "1/2 ▴",
                            SnapValue::Beat1_4 => "1/4 ▴",
                            SnapValue::Beat1_8 => "1/8 ▴",
                            SnapValue::Beat1_16 => "1/16 ▴",
                        })
                        .compact()
                        .bg(rgb(0x282830))
                        .text_color(rgb(0xE0E0E8))
                        .on_click(|_, _, cx| {
                            state(cx).update(cx, |s, cx| {
                                s.clip_editor.snap = match s.clip_editor.snap {
                                    SnapValue::None => SnapValue::Bar1,
                                    SnapValue::Bar1 => SnapValue::Beat1_2,
                                    SnapValue::Beat1_2 => SnapValue::Beat1_4,
                                    SnapValue::Beat1_4 => SnapValue::Beat1_8,
                                    SnapValue::Beat1_8 => SnapValue::Beat1_16,
                                    SnapValue::Beat1_16 => SnapValue::None,
                                };
                                cx.notify();
                            });
                        }),
                )
                // Zoom controls
                .child(
                    Button::new("ce_zoom_out")
                        .rounded(ButtonRounded::None)
                        .label("-")
                        .compact()
                        .bg(rgb(0x282830))
                        .text_color(rgb(0xFFFFFF))
                        .on_click(|_, _, cx| {
                            state(cx).update(cx, |s, cx| {
                                s.clip_editor.zoom_x = step_ce_zoom(s.clip_editor.zoom_x, false);
                                cx.notify();
                            });
                        }),
                )
                .child(
                    Label::new(format!("{:.0}px/s", zoom_x))
                        .text_xs()
                        .text_color(rgb(0x8A8A96)),
                )
                .child(
                    Button::new("ce_zoom_in")
                        .rounded(ButtonRounded::None)
                        .label("+")
                        .compact()
                        .bg(rgb(0x282830))
                        .text_color(rgb(0xFFFFFF))
                        .on_click(|_, _, cx| {
                            state(cx).update(cx, |s, cx| {
                                s.clip_editor.zoom_x = step_ce_zoom(s.clip_editor.zoom_x, true);
                                cx.notify();
                            });
                        }),
                )
                .child(
                    Button::new("ce_zoom_fit")
                        .rounded(ButtonRounded::None)
                        .label("⇥⇤")
                        .compact()
                        .bg(rgb(0x282830))
                        .text_color(rgb(0xFFFFFF))
                        .on_click(|_, _, cx| {
                            state(cx).update(cx, |s, cx| {
                                let dur = clip_duration_hint(s);
                                let vis = visible_secs(s.clip_editor.zoom_x, 800.0);
                                s.clip_editor.zoom_x = fit_zoom(dur, 800.0);
                                s.clip_editor.scroll_secs =
                                    clamp_scroll(s.clip_editor.scroll_secs, dur, vis);
                                cx.notify();
                            });
                        }),
                ),
        )
        .into_any_element()
}

/// Visualizador principal: regla temporal + forma de onda GPU + marcadores de loop y ganancia.
fn waveform_viewer(
    _cx: &mut Context<HikaruApp>,
    clip: &ClipEditorClip,
    ce: &ClipEditorState,
    bpm: f32,
    zoom_x: f32,
) -> AnyElement {
    let duration = clip.duration_secs;
    let scroll = clamp_scroll(ce.scroll_secs, duration, visible_secs(zoom_x, 800.0));
    let peaks = best_peaks(&clip.detail_peaks, &clip.peaks);
    let loop_enabled = ce.loop_enabled;
    let snap_secs = ce.snap.interval_secs(bpm);
    let loop_start = ce.loop_start_secs;
    let loop_end = if ce.loop_end_secs > ce.loop_start_secs {
        ce.loop_end_secs
    } else {
        duration
    };
    let gain_db = ce.params.gain_db;
    let drag_action = ce.drag_action.clone();
    let selection_range = ce.selection_range;
    let active_tab = ce.tab;

    // Números de compás en la regla (DOM antes del renderizado canvas)
    let bar_number_labels: Vec<AnyElement> =
        ruler_labels(scroll, visible_secs(zoom_x, 800.0), bpm, zoom_x)
            .into_iter()
            .filter(|(_text, x, is_bar)| *x > -24.0 && *x < 1400.0 && (*is_bar || zoom_x >= 140.0))
            .map(|(text, x, is_bar)| {
                Label::new(text.clone())
                    .text_xs()
                    .text_color(if is_bar { rgb(0xF0F0F5) } else { rgb(0x8A8A96) })
                    .absolute()
                    .left(px(x + 3.0))
                    .top(px(3.0))
                    .into_any_element()
            })
            .collect();

    // Rueda del ratón: scroll horizontal y zoom continuo centrado en el cursor (Shift / Ctrl)
    let wheel_catcher: AnyElement = div()
        .id("clip_editor_wheel")
        .absolute()
        .inset_0()
        .on_scroll_wheel(move |event, _, cx| {
            let delta = match event.delta {
                gpui_kit::ScrollDelta::Pixels(p) => p.x.as_f32(),
                gpui_kit::ScrollDelta::Lines(p) => p.x * 20.0,
            };
            if delta.abs() < 0.01 {
                return;
            }
            state(cx).update(cx, |s, cx| {
                if event.modifiers.shift || event.modifiers.control {
                    // Zoom continuo centrado en la posición temporal bajo el cursor
                    let cursor_px = event.position.x.as_f32().max(0.0);
                    let old_zoom = s.clip_editor.zoom_x.max(0.0001);
                    let t_cursor = s.clip_editor.scroll_secs + (cursor_px as f64 / old_zoom as f64);

                    let factor = if delta > 0.0 { 0.85 } else { 1.15 };
                    let new_zoom = clamped_ce_zoom(old_zoom * factor);
                    let new_scroll = t_cursor - (cursor_px as f64 / new_zoom as f64);
                    let dur = clip_duration_hint(s);
                    s.clip_editor.zoom_x = new_zoom;
                    s.clip_editor.scroll_secs = clamp_scroll(
                        new_scroll,
                        dur,
                        visible_secs(new_zoom, 800.0),
                    );
                } else {
                    let secs = (delta as f64) / s.clip_editor.zoom_x.max(0.0001) as f64;
                    let dur = clip_duration_hint(s);
                    s.clip_editor.scroll_secs = clamp_scroll(
                        s.clip_editor.scroll_secs + secs,
                        dur,
                        visible_secs(s.clip_editor.zoom_x, 800.0),
                    );
                }
                cx.notify();
            });
        })
        .on_mouse_down(gpui_kit::MouseButton::Left, move |event, _, cx| {
            let click_x = event.position.x.as_f32();
            let click_y = event.position.y.as_f32();
            state(cx).update(cx, |s, cx| {
                let px_per_sec = s.clip_editor.zoom_x.max(0.0001) as f64;
                let t0 = s.clip_editor.scroll_secs;
                let x_of = |t: f64| ((t - t0) * px_per_sec) as f32;

                let x_loop_start = x_of(s.clip_editor.loop_start_secs);
                let x_loop_end = x_of(s.clip_editor.loop_end_secs.max(s.clip_editor.loop_start_secs));
                let x_clip_start = x_of(0.0);
                let x_clip_end = x_of(clip_duration_hint(s));
                let ruler_h = 22.0f32;
                let loop_bar_h = 14.0f32;
                let top_h = ruler_h + loop_bar_h;

                // Hit testing de marcadores y ganancia
                if (click_x - x_loop_start).abs() <= 8.0 && click_y <= top_h + 6.0 {
                    s.clip_editor.drag_action = Some(DragAction::LoopStart {
                        orig_time: s.clip_editor.loop_start_secs,
                    });
                } else if (click_x - x_loop_end).abs() <= 8.0 && click_y <= top_h + 6.0 {
                    s.clip_editor.drag_action = Some(DragAction::LoopEnd {
                        orig_time: s.clip_editor.loop_end_secs,
                    });
                } else if click_x >= x_loop_start && click_x <= x_loop_end && click_y >= ruler_h && click_y <= top_h {
                    s.clip_editor.drag_action = Some(DragAction::LoopRange {
                        start_x: click_x,
                        orig_start: s.clip_editor.loop_start_secs,
                        orig_end: s.clip_editor.loop_end_secs.max(s.clip_editor.loop_start_secs + 0.1),
                    });
                } else if (click_x - x_clip_start).abs() <= 6.0 {
                    s.clip_editor.drag_action = Some(DragAction::ClipStart { orig_time: 0.0 });
                } else if (click_x - x_clip_end).abs() <= 6.0 {
                    s.clip_editor.drag_action = Some(DragAction::ClipEnd { orig_time: clip_duration_hint(s) });
                } else {
                    let banner_h = 20.0f32;
                    let body_y = top_h + banner_h;
                    let body_h = (s.clip_editor.editor_height - body_y - 26.0).max(30.0);
                    let norm_gain = ((s.clip_editor.params.gain_db + 24.0) / 48.0).clamp(0.0, 1.0);
                    let y_gain = body_y + body_h * (1.0 - norm_gain);
                    if (click_y - y_gain).abs() <= 8.0 && click_x >= x_clip_start && click_x <= x_clip_end {
                        s.clip_editor.drag_action = Some(DragAction::GainLine {
                            start_y: click_y,
                            orig_gain_db: s.clip_editor.params.gain_db,
                        });
                    } else {
                        // Selección de región por defecto (Marquee rectangle selection)
                        s.clip_editor.drag_action = Some(DragAction::MarqueeSelection {
                            start_x: click_x,
                            start_y: click_y,
                            cur_x: click_x,
                            cur_y: click_y,
                        });
                    }
                }
                cx.notify();
            });
        })
        .on_mouse_down(gpui_kit::MouseButton::Middle, move |event, _, cx| {
            state(cx).update(cx, |s, cx| {
                let px_per_sec = s.clip_editor.zoom_x.max(0.0001) as f64;
                let t0 = s.clip_editor.scroll_secs;
                let click_x = event.position.x.as_f32();
                let click_y = event.position.y.as_f32();
                let anchor_time = t0 + (click_x as f64) / px_per_sec;
                s.clip_editor.drag_action = Some(DragAction::MiddleDragZoomPan {
                    start_x: click_x,
                    start_y: click_y,
                    anchor_time,
                    orig_zoom: s.clip_editor.zoom_x,
                    orig_scroll: s.clip_editor.scroll_secs,
                });
                cx.notify();
            });
        })
        .on_mouse_move(move |event, _, cx| {
            let cues = match active_tab {
                EditorTab::Pitch => &[
                    ("DRAG", "Rectangular selection"),
                    ("CTRL+ALT+DRAG", "Pan"),
                    ("DOUBLE-CLICK", "Insert event"),
                ][..],
                _ => &[
                    ("DRAG", "Select time"),
                    ("ALT+DRAG", "Slide Content"),
                    ("MIDDLE-DRAG", "Zoom & pan"),
                ][..],
            };
            crate::app::set_footer_cues(cx, None, cues);

            state(cx).update(cx, |s, cx| {
                let Some(ref action) = s.clip_editor.drag_action.clone() else {
                    return;
                };
                let px_per_sec = s.clip_editor.zoom_x.max(0.0001) as f64;
                let t0 = s.clip_editor.scroll_secs;
                let cur_x = event.position.x.as_f32();
                let cur_y = event.position.y.as_f32();
                let time_at_cur = (t0 + (cur_x as f64) / px_per_sec).max(0.0);
                let dur = clip_duration_hint(s);
                let snap = s.clip_editor.snap.interval_secs(bpm);

                match action {
                    DragAction::Pan { start_x, start_scroll } => {
                        let dx = cur_x - start_x;
                        let dt = (dx as f64) / px_per_sec;
                        s.clip_editor.scroll_secs = clamp_scroll(
                            start_scroll - dt,
                            dur,
                            visible_secs(s.clip_editor.zoom_x, 800.0),
                        );
                    }
                    DragAction::MiddleDragZoomPan { start_x, start_y, anchor_time, orig_zoom, .. } => {
                        let _dx = cur_x - start_x;
                        let dy = cur_y - start_y;
                        // Zoom continuo en eje Y con respuesta logarítmica/exponencial (hacia arriba = Zoom In)
                        let zoom_factor = (-dy * 0.006).exp();
                        let new_zoom = clamped_ce_zoom(orig_zoom * zoom_factor);
                        s.clip_editor.zoom_x = new_zoom;

                        // Pan temporal en eje X manteniendo el instante ancla fijo bajo el cursor
                        let new_px_per_sec = new_zoom.max(0.0001) as f64;
                        let new_scroll = anchor_time - (cur_x as f64) / new_px_per_sec;
                        s.clip_editor.scroll_secs = clamp_scroll(new_scroll, dur, visible_secs(new_zoom, 800.0));
                    }
                    DragAction::LoopRange { start_x, orig_start, orig_end } => {
                        let dx = cur_x - start_x;
                        let dt = (dx as f64) / px_per_sec;
                        let len = orig_end - orig_start;
                        let mut new_s = orig_start + dt;
                        if snap > 0.0 {
                            new_s = (new_s / snap).round() * snap;
                        }
                        new_s = new_s.clamp(0.0, (dur - len).max(0.0));
                        s.clip_editor.loop_start_secs = new_s;
                        s.clip_editor.loop_end_secs = new_s + len;
                        if s.clip_editor.loop_enabled {
                            apply_loop_to_engine(s, true);
                        }
                    }
                    DragAction::LoopStart { .. } => {
                        let mut t = time_at_cur;
                        if snap > 0.0 {
                            t = (t / snap).round() * snap;
                        }
                        let max_end = s.clip_editor.loop_end_secs.max(0.05);
                        s.clip_editor.loop_start_secs = t.clamp(0.0, max_end - 0.02);
                        if s.clip_editor.loop_enabled {
                            apply_loop_to_engine(s, true);
                        }
                    }
                    DragAction::LoopEnd { .. } => {
                        let mut t = time_at_cur;
                        if snap > 0.0 {
                            t = (t / snap).round() * snap;
                        }
                        let min_start = s.clip_editor.loop_start_secs;
                        s.clip_editor.loop_end_secs = t.clamp(min_start + 0.02, dur.max(min_start + 0.1));
                        if s.clip_editor.loop_enabled {
                            apply_loop_to_engine(s, true);
                        }
                    }
                    DragAction::ClipStart { .. } => {
                        let mut t = time_at_cur;
                        if snap > 0.0 {
                            t = (t / snap).round() * snap;
                        }
                        s.clip_editor.loop_start_secs = t.clamp(0.0, dur);
                    }
                    DragAction::ClipEnd { .. } => {
                        let mut t = time_at_cur;
                        if snap > 0.0 {
                            t = (t / snap).round() * snap;
                        }
                        s.clip_editor.loop_end_secs = t.clamp(0.0, dur);
                    }
                    DragAction::GainLine { start_y, orig_gain_db } => {
                        let dy = start_y - cur_y; // Arrastrar hacia arriba sube la ganancia
                        let delta_db = dy * 0.25;
                        let new_gain = (orig_gain_db + delta_db).clamp(-60.0, 24.0);
                        s.clip_editor.params.gain_db = new_gain;
                        apply_gain_to_clip(s, new_gain);
                    }
                    DragAction::MarqueeSelection { start_x, start_y, .. } => {
                        s.clip_editor.drag_action = Some(DragAction::MarqueeSelection {
                            start_x: *start_x,
                            start_y: *start_y,
                            cur_x,
                            cur_y,
                        });
                        let t1 = (t0 + (*start_x as f64) / px_per_sec).max(0.0);
                        let t2 = (t0 + (cur_x as f64) / px_per_sec).max(0.0);
                        s.clip_editor.selection_range = Some((t1.min(t2), t1.max(t2)));
                    }
                }
                cx.notify();
            });
        })
        .on_mouse_up(gpui_kit::MouseButton::Left, move |_, _, cx| {
            state(cx).update(cx, |s, cx| {
                s.clip_editor.drag_action = None;
                cx.notify();
            });
        })
        .on_mouse_up(gpui_kit::MouseButton::Middle, move |_, _, cx| {
            state(cx).update(cx, |s, cx| {
                s.clip_editor.drag_action = None;
                cx.notify();
            });
        })
        .into_any_element();

    let banner_left = (((0.0 - scroll) * zoom_x as f64) as f32).max(0.0) + 8.0;
    let clip_banner_label: AnyElement = h_flex()
        .absolute()
        .left(px(banner_left))
        .top(px(38.0))
        .items_center()
        .gap(px(8.0))
        .child(
            Label::new(clip.name.clone())
                .text_xs()
                .font_weight(gpui_kit::FontWeight::BOLD)
                .text_color(rgb(0xD8DCE6)),
        )
        .child(
            Label::new(format!("{:.2}s • {} BPM", duration, bpm.round() as i32))
                .text_xs()
                .text_color(rgb(0x9AA0AE)),
        )
        .into_any_element();

    div()
        .id("clip_editor_waveform")
        .test_support()
        .flex_1()
        .h_full()
        .w_full()
        .relative()
        .overflow_hidden()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let w = bounds.size.width.as_f32();
                    let h = bounds.size.height.as_f32();
                    note_paint_height(h);
                    if w <= 0.0 || h <= 0.0 {
                        return;
                    }

                    let px_per_sec = zoom_x.max(0.0001);
                    let t0 = scroll;
                    let x_of = |t: f64| bounds.origin.x + px(((t - t0) * px_per_sec as f64) as f32);
                    let right = bounds.origin.x + bounds.size.width;
                    let bottom = bounds.origin.y + bounds.size.height;

                    let ruler_h = 22.0f32;
                    let loop_bar_h = 14.0f32;
                    let top_h = ruler_h + loop_bar_h;
                    let wave_y = bounds.origin.y + px(top_h);
                    let wave_h = (h - top_h).max(4.0);
                    let banner_h = 20.0f32;
                    let body_y = wave_y + px(banner_h);
                    let body_h = (wave_h - banner_h).max(4.0);
                    let mid = body_y + px(body_h / 2.0);

                    // =====================================================
                    // 1. FONDO PRINCIPAL — Dark Slate Bitwig (#28292E)
                    // =====================================================
                    window.paint_quad(PaintQuad {
                        bounds,
                        background: rgb(0x28292E).into(),
                        border_color: Hsla::default(),
                        corner_radii: gpui_kit::Corners::default(),
                        border_widths: gpui_kit::Edges::default(),
                        border_style: BorderStyle::default(),
                    });

                    // =====================================================
                    // 2. REGLA TEMPORAL SUPERIOR (22px)
                    // =====================================================
                    window.paint_quad(PaintQuad {
                        bounds: Bounds::new(bounds.origin, size(bounds.size.width, px(ruler_h))),
                        background: rgb(0x323232).into(),
                        border_color: rgb(0x282828).into(),
                        corner_radii: gpui_kit::Corners::default(),
                        border_widths: gpui_kit::Edges {
                            bottom: px(1.0),
                            top: px(0.0),
                            left: px(0.0),
                            right: px(0.0),
                        },
                        border_style: BorderStyle::default(),
                    });

                    // =====================================================
                    // 2b. CARRIL DE REGIÓN DE LOOP (14px)
                    // =====================================================
                    window.paint_quad(PaintQuad {
                        bounds: Bounds::new(
                            point(bounds.origin.x, bounds.origin.y + px(ruler_h)),
                            size(bounds.size.width, px(loop_bar_h)),
                        ),
                        background: rgb(0x353535).into(),
                        border_color: rgb(0x282828).into(),
                        corner_radii: gpui_kit::Corners::default(),
                        border_widths: gpui_kit::Edges {
                            bottom: px(1.0),
                            top: px(0.0),
                            left: px(0.0),
                            right: px(0.0),
                        },
                        border_style: BorderStyle::default(),
                    });

                    let bpm_f = if bpm > 0.0 { bpm as f64 } else { 120.0 };
                    let sec_per_bar = (60.0 / bpm_f) * 4.0;
                    let sec_per_beat = sec_per_bar / 4.0;
                    let vis = visible_secs(zoom_x, w);

                    // Líneas divisorias de BARRA a lo largo de todo el canvas
                    let first_bar = (t0 / sec_per_bar).floor();
                    let mut bar = first_bar;
                    while bar * sec_per_bar < t0 + vis + sec_per_bar {
                        let t = bar * sec_per_bar;
                        let x = x_of(t);
                        if x >= bounds.origin.x && x <= right {
                            let mut p = PathBuilder::stroke(px(1.0));
                            p.move_to(point(x, bounds.origin.y));
                            p.line_to(point(x, bottom));
                            if let Ok(path) = p.build() {
                                window.paint_path(path, rgba(0xFFFFFF22));
                            }
                        }
                        bar += 1.0;
                    }

                    // Líneas divisorias de BEAT (optimizadas en una sola pasada de pintado)
                    let first_beat = (t0 / sec_per_beat).floor();
                    let mut bt = first_beat;
                    let mut beat_path = PathBuilder::stroke(px(1.0));
                    let mut has_beats = false;
                    while bt * sec_per_beat < t0 + vis + sec_per_beat {
                        let t = bt * sec_per_beat;
                        let x = x_of(t);
                        if x >= bounds.origin.x && x <= right && (bt as i64) % 4 != 0 {
                            beat_path.move_to(point(x, wave_y));
                            beat_path.line_to(point(x, bottom));
                            has_beats = true;
                        }
                        bt += 1.0;
                    }
                    if has_beats {
                        if let Ok(path) = beat_path.build() {
                            window.paint_path(path, rgba(0xFFFFFF0F));
                        }
                    }

                    // Subdivisión de SNAP (optimizada en una sola llamada)
                    if snap_secs > 0.0 && snap_secs < sec_per_beat {
                        let mut t = (t0 / snap_secs).floor() * snap_secs;
                        let mut snap_path = PathBuilder::stroke(px(1.0));
                        let mut has_snaps = false;
                        while t < t0 + vis + snap_secs {
                            let x = x_of(t);
                            if x >= bounds.origin.x && x <= right {
                                snap_path.move_to(point(x, wave_y));
                                snap_path.line_to(point(x, bottom));
                                has_snaps = true;
                            }
                            t += snap_secs;
                        }
                        if has_snaps {
                            if let Ok(path) = snap_path.build() {
                                window.paint_path(path, rgba(0x00C8FF12));
                            }
                        }
                    }

                    // =====================================================
                    // 3. CONTENEDOR DEL CLIP (Bitwig Card con Silver Waveform)
                    // =====================================================
                    let clip_start_x = x_of(0.0);
                    let clip_end_x = x_of(duration);
                    let c_x0 = clip_start_x.max(bounds.origin.x);
                    let c_x1 = clip_end_x.min(right);

                    // Fondo de vacío fuera del clip (Dark Slate #28292E)
                    if clip_start_x > bounds.origin.x {
                        let w_pre = (clip_start_x - bounds.origin.x).as_f32();
                        window.paint_quad(PaintQuad {
                            bounds: Bounds::new(
                                point(bounds.origin.x, wave_y),
                                size(px(w_pre), px(wave_h)),
                            ),
                            background: rgb(0x28292E).into(),
                            border_color: Hsla::default(),
                            corner_radii: gpui_kit::Corners::default(),
                            border_widths: gpui_kit::Edges::default(),
                            border_style: BorderStyle::default(),
                        });
                    }
                    if clip_end_x < right {
                        let w_post = (right - clip_end_x).as_f32();
                        window.paint_quad(PaintQuad {
                            bounds: Bounds::new(
                                point(clip_end_x, wave_y),
                                size(px(w_post), px(wave_h)),
                            ),
                            background: rgb(0x28292E).into(),
                            border_color: Hsla::default(),
                            corner_radii: gpui_kit::Corners::default(),
                            border_widths: gpui_kit::Edges::default(),
                            border_style: BorderStyle::default(),
                        });
                    }

                    if c_x1 > c_x0 {
                        // 3a. Cabecera/Banner del Clip (Gris Pizarra Bitwig #32343A)
                        window.paint_quad(PaintQuad {
                            bounds: Bounds::new(
                                point(c_x0, wave_y),
                                size(c_x1 - c_x0, px(banner_h)),
                            ),
                            background: rgb(0x32343A).into(),
                            border_color: rgb(0x232529).into(),
                            corner_radii: gpui_kit::Corners::default(),
                            border_widths: gpui_kit::Edges {
                                bottom: px(1.0),
                                top: px(0.0),
                                left: px(1.0),
                                right: px(1.0),
                            },
                            border_style: BorderStyle::default(),
                        });

                        // Línea superior anaranjada Bitwig (#FF7A00) que corona el clip
                        let mut top_line = PathBuilder::stroke(px(2.5));
                        top_line.move_to(point(c_x0, wave_y + px(1.0)));
                        top_line.line_to(point(c_x1, wave_y + px(1.0)));
                        if let Ok(path) = top_line.build() {
                            window.paint_path(path, rgb(0xFF7A00));
                        }

                        // 3b. Cuerpo del Waveform: FONDO SILVER BITWIG (#C0C4D0)
                        window.paint_quad(PaintQuad {
                            bounds: Bounds::new(
                                point(c_x0, body_y),
                                size(c_x1 - c_x0, px(body_h)),
                            ),
                            background: rgb(0xC0C4D0).into(),
                            border_color: rgb(0x787C88).into(),
                            corner_radii: gpui_kit::Corners::default(),
                            border_widths: gpui_kit::Edges {
                                bottom: px(0.0),
                                top: px(0.0),
                                left: px(1.0),
                                right: px(1.0),
                            },
                            border_style: BorderStyle::default(),
                        });

                        // Eje central de 0 dB sobre el fondo silver
                        let mut axis = PathBuilder::stroke(px(1.0));
                        axis.move_to(point(c_x0, mid));
                        axis.line_to(point(c_x1, mid));
                        if let Ok(path) = axis.build() {
                            window.paint_path(path, rgba(0x787C8888));
                        }

                        // Líneas verticales de beat sutiles sobre la tarjeta plateada
                        let mut bt = first_beat;
                        let mut card_beats = PathBuilder::stroke(px(1.0));
                        let mut has_card_beats = false;
                        while bt * sec_per_beat < t0 + vis + sec_per_beat {
                            let t = bt * sec_per_beat;
                            let x = x_of(t);
                            if x >= c_x0 && x <= c_x1 {
                                card_beats.move_to(point(x, body_y));
                                card_beats.line_to(point(x, bottom));
                                has_card_beats = true;
                            }
                            bt += 1.0;
                        }
                        if has_card_beats {
                            if let Ok(path) = card_beats.build() {
                                window.paint_path(path, rgba(0x0000001C));
                            }
                        }

                        // =====================================================
                        // 4. FORMA DE ONDA DE AUDIO (Charcoal/Graphite #282828)
                        // =====================================================
                        if peaks_are_silent(&peaks) {
                            let cx0 = c_x0 + (c_x1 - c_x0) / 2.0;
                            let cy0 = mid;
                            let mut tri = PathBuilder::stroke(px(1.5));
                            tri.move_to(point(cx0 - px(9.0), cy0 - px(9.0)));
                            tri.line_to(point(cx0 - px(9.0), cy0 + px(9.0)));
                            tri.line_to(point(cx0 + px(9.0), cy0));
                            tri.close();
                            if let Ok(path) = tri.build() {
                                window.paint_path(path, rgb(0x40444C));
                            }
                        } else {
                            let dur = duration.max(0.0001);
                            let from = (t0 / dur).clamp(0.0, 1.0) as f32;
                            let to = ((t0 + vis) / dur).clamp(0.0, 1.0) as f32;
                            let window_peaks = peaks_window(&peaks, from, to);

                            let width_px = (w.floor() as usize).max(1);
                            let half_h = (body_h * 0.5 - 2.0).max(2.0);
                            let wave_color = rgb(0x282828);

                            let mut wave_path = PathBuilder::stroke(px(1.0));
                            let mut has_wave = false;
                            for (x_off, amp) in wave_geometry(&window_peaks, width_px, half_h) {
                                let x = bounds.origin.x + px(x_off);
                                if x >= c_x0 && x <= c_x1 {
                                    wave_path.move_to(point(x, mid - px(amp)));
                                    wave_path.line_to(point(x, mid + px(amp)));
                                    has_wave = true;
                                }
                            }
                            if has_wave {
                                if let Ok(path) = wave_path.build() {
                                    window.paint_path(path, wave_color);
                                }
                            }
                        }

                        // =====================================================
                        // 5. ENVOLVENTE Y LÍNEA HORIZONTAL DE GANANCIA
                        // =====================================================
                        let norm_gain = ((gain_db + 24.0) / 48.0).clamp(0.0, 1.0);
                        let gain_y = body_y + px(body_h * (1.0 - norm_gain));
                        let mut gl = PathBuilder::stroke(px(1.5));
                        gl.move_to(point(c_x0, gain_y));
                        gl.line_to(point(c_x1, gain_y));
                        if let Ok(path) = gl.build() {
                            window.paint_path(path, rgba(0xE8760CDD));
                        }

                        // =====================================================
                        // 5b. MARCADORES DE ONSETS (Banderas periwinkle Bitwig #7B8CDE)
                        // =====================================================
                        let mut bt = first_beat;
                        let mut stems = PathBuilder::stroke(px(1.0));
                        let mut has_stems = false;
                        while bt * sec_per_beat < t0 + vis + sec_per_beat {
                            let t = bt * sec_per_beat;
                            let x = x_of(t);
                            if x >= c_x0 && x <= c_x1 {
                                let mut flag = PathBuilder::fill();
                                flag.move_to(point(x - px(3.5), body_y));
                                flag.line_to(point(x + px(3.5), body_y));
                                flag.line_to(point(x, body_y + px(6.0)));
                                flag.close();
                                if let Ok(path) = flag.build() {
                                    window.paint_path(path, rgb(0x7B8CDE));
                                }
                                if active_tab == EditorTab::Stretch || active_tab == EditorTab::Onsets {
                                    stems.move_to(point(x, body_y + px(6.0)));
                                    stems.line_to(point(x, bottom));
                                    has_stems = true;
                                }
                            }
                            bt += 1.0;
                        }
                        if has_stems {
                            if let Ok(path) = stems.build() {
                                window.paint_path(path, rgba(0x7B8CDE55));
                            }
                        }
                    }

                    // =====================================================
                    // 6. MARCADORES DE INICIO/FIN DE CLIP
                    // =====================================================
                    let marker_w = px(2.0);
                    for mx in [clip_start_x, clip_end_x] {
                        if mx >= bounds.origin.x - marker_w && mx <= right + marker_w {
                            window.paint_quad(PaintQuad {
                                bounds: Bounds::new(
                                    point(mx - marker_w / 2.0, wave_y),
                                    size(marker_w, px(wave_h)),
                                ),
                                background: rgb(0x8A8A96).into(),
                                border_color: Hsla::default(),
                                corner_radii: gpui_kit::Corners::default(),
                                border_widths: gpui_kit::Edges::default(),
                                border_style: BorderStyle::default(),
                            });
                        }
                    }

                    // =====================================================
                    // 7. REGIÓN Y CORCHETES DE LOOP (#FF7A00 BITWIG)
                    // =====================================================
                    let (l_start_x, l_end_x) = if loop_end > loop_start {
                        (x_of(loop_start), x_of(loop_end))
                    } else {
                        (clip_start_x, clip_end_x)
                    };

                    let y_loop_top = bounds.origin.y + px(ruler_h);
                    let y_loop_bot = y_loop_top + px(loop_bar_h);
                    let l_vis_0 = l_start_x.max(bounds.origin.x);
                    let l_vis_1 = l_end_x.min(right);

                    if l_vis_1 > l_vis_0 {
                        // Resalte ámbar en el carril de loop
                        window.paint_quad(PaintQuad {
                            bounds: Bounds::new(
                                point(l_vis_0, y_loop_top),
                                size(l_vis_1 - l_vis_0, px(loop_bar_h)),
                            ),
                            background: rgba(0xFF7A001C).into(),
                            border_color: Hsla::default(),
                            corner_radii: gpui_kit::Corners::default(),
                            border_widths: gpui_kit::Edges::default(),
                            border_style: BorderStyle::default(),
                        });

                        // Líneas superior e inferior anaranjadas Bitwig
                        let mut loop_borders = PathBuilder::stroke(px(1.5));
                        loop_borders.move_to(point(l_vis_0, y_loop_top + px(0.5)));
                        loop_borders.line_to(point(l_vis_1, y_loop_top + px(0.5)));
                        loop_borders.move_to(point(l_vis_0, y_loop_bot - px(0.5)));
                        loop_borders.line_to(point(l_vis_1, y_loop_bot - px(0.5)));
                        if let Ok(path) = loop_borders.build() {
                            window.paint_path(path, rgb(0xFF7A00));
                        }
                    }

                    let orange = rgb(0xFF7A00);

                    // Marcador Triangular Loop Start ▶
                    if l_start_x >= bounds.origin.x - px(8.0) && l_start_x <= right + px(8.0) {
                        let mut tri_s = PathBuilder::fill();
                        tri_s.move_to(point(l_start_x, y_loop_top));
                        tri_s.line_to(point(l_start_x + px(7.0), y_loop_top + px(7.0)));
                        tri_s.line_to(point(l_start_x, y_loop_bot));
                        tri_s.close();
                        if let Ok(path) = tri_s.build() {
                            window.paint_path(path, orange);
                        }
                        let mut g_line = PathBuilder::stroke(px(1.0));
                        g_line.move_to(point(l_start_x, y_loop_bot));
                        g_line.line_to(point(l_start_x, bottom));
                        if let Ok(path) = g_line.build() {
                            window.paint_path(path, rgba(0xFF7A0066));
                        }
                    }

                    // Marcador Triangular Loop End ◀
                    if l_end_x >= bounds.origin.x - px(8.0) && l_end_x <= right + px(8.0) {
                        let mut tri_e = PathBuilder::fill();
                        tri_e.move_to(point(l_end_x, y_loop_top));
                        tri_e.line_to(point(l_end_x - px(7.0), y_loop_top + px(7.0)));
                        tri_e.line_to(point(l_end_x, y_loop_bot));
                        tri_e.close();
                        if let Ok(path) = tri_e.build() {
                            window.paint_path(path, orange);
                        }
                        let mut g_line = PathBuilder::stroke(px(1.0));
                        g_line.move_to(point(l_end_x, y_loop_bot));
                        g_line.line_to(point(l_end_x, bottom));
                        if let Ok(path) = g_line.build() {
                            window.paint_path(path, rgba(0xFF7A0066));
                        }
                    }

                    // =====================================================
                    // 8. SELECCIÓN DE RECUADRO (MARQUEE)
                    // =====================================================
                    if let Some(DragAction::MarqueeSelection { start_x, start_y, cur_x, cur_y }) =
                        drag_action
                    {
                        let min_x = start_x.min(cur_x).max(bounds.origin.x.as_f32());
                        let max_x = start_x.max(cur_x).min(right.as_f32());
                        let min_y = start_y.min(cur_y).max(wave_y.as_f32());
                        let max_y = start_y.max(cur_y).min(bottom.as_f32());
                        if max_x > min_x && max_y > min_y {
                            window.paint_quad(PaintQuad {
                                bounds: Bounds::new(
                                    point(px(min_x), px(min_y)),
                                    size(px(max_x - min_x), px(max_y - min_y)),
                                ),
                                background: rgba(0xFF7A0022).into(),
                                border_color: rgba(0xFF7A0099).into(),
                                corner_radii: gpui_kit::Corners::default(),
                                border_widths: gpui_kit::Edges::all(px(1.0)),
                                border_style: BorderStyle::default(),
                            });
                        }
                    } else if let Some((sel_s, sel_e)) = selection_range {
                        let sx0 = x_of(sel_s).max(bounds.origin.x);
                        let sx1 = x_of(sel_e).min(right);
                        if sx1 > sx0 {
                            window.paint_quad(PaintQuad {
                                bounds: Bounds::new(
                                    point(sx0, body_y),
                                    size(sx1 - sx0, px(body_h)),
                                ),
                                background: rgba(0xFF7A0018).into(),
                                border_color: rgba(0xFF7A0044).into(),
                                corner_radii: gpui_kit::Corners::default(),
                                border_widths: gpui_kit::Edges {
                                    top: px(1.0),
                                    bottom: px(1.0),
                                    left: px(1.0),
                                    right: px(1.0),
                                },
                                border_style: BorderStyle::default(),
                            });
                        }
                    }

                    // =====================================================
                    // 9. PLAYHEAD (Línea blanca de reproducción)
                    // =====================================================
                    let ph_x = x_of(if loop_enabled && loop_end > loop_start {
                        loop_start
                    } else {
                        t0
                    });
                    if ph_x >= bounds.origin.x && ph_x <= right {
                        let mut php = PathBuilder::stroke(px(1.5));
                        php.move_to(point(ph_x, bounds.origin.y));
                        php.line_to(point(ph_x, bottom));
                        if let Ok(path) = php.build() {
                            window.paint_path(path, rgba(0xFFFFFFEE));
                        }
                    }
                },
            )
            .absolute()
            .inset_0(),
        )
        .children(bar_number_labels)
        .child(clip_banner_label)
        .child(wheel_catcher)
        .into_any_element()
}

/// Duración en segundos del clip en edición.
fn clip_duration_hint(s: &AppState) -> f64 {
    match s.clip_editor.target {
        Some(ClipEditorTarget::Matrix { track, scene }) => s
            .matrix_state
            .grid
            .get(track)
            .and_then(|r| r.get(scene))
            .and_then(|slot| slot.clip.as_ref())
            .map(|c| if c.duration_secs > 0.001 { c.duration_secs } else { 4.0 })
            .unwrap_or(4.0),
        Some(ClipEditorTarget::Playlist { clip_id }) => {
            let Some((_, clip)) = s.playlist_state.clips.iter().find(|(_, c)| c.id == clip_id)
            else {
                return 1.0;
            };
            let ppqn = s.playlist_state.ppqn.max(1) as f64;
            let bpm = if s.transport.bpm > 0.0 {
                s.transport.bpm as f64
            } else {
                120.0
            };
            (clip.duration_ticks as f64) * (60.0 / bpm) / ppqn
        }
        None => 1.0,
    }
}
