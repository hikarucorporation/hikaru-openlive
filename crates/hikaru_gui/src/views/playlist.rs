// Playlist / Timeline (OpenStudio) — lienzo de composición lineal.
//
// LÍMITE DEL MÓDULO: esta vista es SÓLO tiempo. Contiene la regla de
// compases (ruler), filas horizontales limpias por pista, grilla con snap,
// clips de audio/MIDI y playhead. NO contiene ningún control de mezcla
// (faders, pan, S/M/R, volúmenes en dB): esos viven en el mixer lateral de
// `arranger_view.rs` y no deben sangrar adentro del lienzo temporal. El
// lienzo sólo recibe arrastre de samples, creación/selección de clips y
// navegación temporal (seek, zoom, scroll).
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::*;
use gpui_kit::component::button::Button;
use gpui_kit::component::label::Label;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::prelude::StatefulInteractiveElement as _;
use gpui_kit::*;

use crate::app::{state, AppState, HikaruApp};
use crate::audio_proxy::{AudioProxy, GuiCommand};
use crate::views::mixer::Track;
use hikaru_transport::DEFAULT_PPQN;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewMode<'a> {
    Arranger,
    ClipEditor { title: &'a str },
}

impl<'a> ViewMode<'a> {
    #[inline]
    fn drives_engine(&self) -> bool {
        matches!(self, ViewMode::Arranger)
    }

    #[inline]
    fn allows_track_add_remove(&self) -> bool {
        matches!(self, ViewMode::Arranger)
    }

    fn header_label(&self) -> String {
        match self {
            ViewMode::Arranger => "PLAYLIST / TIMELINE".to_string(),
            ViewMode::ClipEditor { title } => title.to_string(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct CurvePoint {
    pub rel_tick: u64,
    pub value: f32,
    pub tension: f32,
}

#[derive(Clone, Debug)]
pub enum ClipType {
    Pattern { pattern_id: usize },
    Audio {
        sample_path: String,
        peaks: Vec<f32>,
        sample_offset_ticks: u64,
        total_sample_ticks: u64,
    },
    Automation {
        points: Vec<CurvePoint>,
        target_param: String,
    },
}

pub type Color32 = Hsla;

#[derive(Clone, Debug)]
pub struct PlaylistClip {
    pub id: usize,
    pub name: String,
    pub start_tick: u64,
    pub duration_ticks: u64,
    pub clip_type: ClipType,
    pub color: Hsla,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopDragHandle {
    None,
    Left,
    Right,
    Body,
}

/// Arrastre de un clip de audio/MIDI por la grilla (botón izquierdo).
///
/// `grab_dx_px` / `grab_dy_px` son el offset del cursor respecto al origen
/// del clip (coords de ventana) al momento del agarre: durante el `MouseMove`
/// la nueva posición se calcula como `cursor - grab`, así el clip no salta
/// al agarrarlo fuera de su esquina. `moved` distingue click de drag para
/// suprimir la re-selección del `on_click` posterior al drop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaylistClipDrag {
    pub clip_id: usize,
    pub orig_tick: u64,
    pub orig_track: usize,
    pub grab_dx_px: f32,
    pub grab_dy_px: f32,
    pub moved: bool,
}

#[derive(Clone, Debug)]
pub struct PlaylistState {
    pub clips: Vec<(usize, PlaylistClip)>,
    pub playhead_tick: u64,
    pub ppqn: u64,
    pub next_clip_id: usize,
    pub header_width: f32,
    pub grid_numerator: u32,
    pub grid_denominator: u32,
    pub zoom_x: f32,
    /// Altura de fila (zoom vertical con `Alt` + rueda). Arranca en
    /// `TRACK_ROW_H` (sincronizada con el mixer) y el usuario la agranda o
    /// achica con la rueda; la geometría siempre usa `track_height()`
    /// (valor acotado), nunca esta cifra en crudo.
    pub row_h: f32,
    pub selected_clips: Vec<usize>,
    pub clipboard: Vec<(usize, PlaylistClip)>,
    pub needs_full_sync: bool,
    pub loop_start_ticks: u64,
    pub loop_end_ticks: u64,
    pub loop_region_active: bool,
    pub loop_dragging: bool,
    pub loop_preview_start_ticks: u64,
    pub loop_preview_end_ticks: u64,
    pub loop_preview_active: bool,
    pub loop_drag_handle: LoopDragHandle,
    pub loop_drag_completed_this_frame: bool,
    /// Gesto de arrastre de clip en curso (`None` = sin drag). Global —y no
    /// flag local del widget— porque los clips se reconstruyen en cada frame
    /// (mismo motivo que los drags del mixer en `AppState`).
    pub clip_drag: Option<PlaylistClipDrag>,
    /// Suprime el `on_click` de selección que sigue al `mouse_up` de un drag
    /// con movimiento (la selección ya quedó fijada al agarrar el clip).
    pub clip_click_suppress: bool,
}

impl Default for PlaylistState {
    fn default() -> Self {
        Self {
            clips: Vec::new(),
            playhead_tick: 0,
            ppqn: DEFAULT_PPQN,
            next_clip_id: 1,
            header_width: 180.0,
            grid_numerator: 1,
            grid_denominator: 4,
            zoom_x: 0.04,
            row_h: TRACK_ROW_H,
            selected_clips: Vec::new(),
            clipboard: Vec::new(),
            needs_full_sync: true,
            loop_start_ticks: 0,
            loop_end_ticks: 0,
            loop_region_active: false,
            loop_dragging: false,
            loop_preview_start_ticks: 0,
            loop_preview_end_ticks: 0,
            loop_preview_active: false,
            loop_drag_handle: LoopDragHandle::None,
            loop_drag_completed_this_frame: false,
            clip_drag: None,
            clip_click_suppress: false,
        }
    }
}

impl PlaylistState {
    /// Altura de fila efectiva, acotada a `[MIN_ROW_H, MAX_ROW_H]`.
    ///
    /// Toda la geometría (headers, clips, drag, drop) debe usar este método
    /// y no `row_h` en crudo, para que un valor extremo nunca deforme el
    /// lienzo ni rompa el mapeo px→fila.
    pub fn track_height(&self) -> f32 {
        clamped_row_h(self.row_h)
    }

    pub fn loop_length_ticks(&self) -> u64 {
        self.loop_end_ticks.saturating_sub(self.loop_start_ticks)
    }

    pub fn is_loop_region_valid(&self) -> bool {
        self.loop_region_active && self.loop_end_ticks > self.loop_start_ticks
    }

    pub fn clear_loop_region(&mut self) {
        self.loop_start_ticks = 0;
        self.loop_end_ticks = 0;
        self.loop_region_active = false;
        self.loop_dragging = false;
        self.loop_preview_start_ticks = 0;
        self.loop_preview_end_ticks = 0;
        self.loop_preview_active = false;
        self.loop_drag_handle = LoopDragHandle::None;
        self.loop_drag_completed_this_frame = true;
    }

    pub fn total_project_ticks(&self) -> u64 {
        self.clips
            .iter()
            .map(|(_, c)| c.start_tick.saturating_add(c.duration_ticks))
            .max()
            .unwrap_or(0)
    }

    pub fn sanitize_loop_region(&mut self) {
        let ppqn = self.ppqn.max(1);
        let start = self.loop_start_ticks;
        let mut end = self.loop_end_ticks;
        if end <= start {
            end = start.saturating_add(4u64.saturating_mul(ppqn));
        }
        if end.saturating_sub(start) < ppqn {
            end = start.saturating_add(ppqn);
        }
        self.loop_start_ticks = start;
        self.loop_end_ticks = end;
        if self.loop_end_ticks > self.loop_start_ticks {
            self.loop_region_active = true;
        }
    }

    pub fn ensure_minimum_global_loop(&mut self, loop_enabled: bool) {
        if !loop_enabled || self.loop_dragging {
            return;
        }
        let ppqn = self.ppqn.max(1);
        let len = self.loop_end_ticks.saturating_sub(self.loop_start_ticks);
        let needs_init = !self.loop_region_active
            || self.loop_end_ticks <= self.loop_start_ticks
            || len < ppqn;
        if needs_init {
            let total = self.total_project_ticks();
            let start = 0u64;
            let mut end = total.max(4u64.saturating_mul(ppqn));
            if end <= start {
                end = start.saturating_add(4u64.saturating_mul(ppqn));
            }
            if end.saturating_sub(start) < ppqn {
                end = start.saturating_add(ppqn);
            }
            if end > start {
                self.loop_start_ticks = start;
                self.loop_end_ticks = end;
                self.loop_preview_start_ticks = start;
                self.loop_preview_end_ticks = end;
                self.loop_region_active = true;
                self.loop_drag_completed_this_frame = true;
            }
        } else {
            self.sanitize_loop_region();
        }
    }

    pub fn sync_all_clips_to_engine(&mut self, audio_proxy: &AudioProxy, bpm: f64) {
        for (track_id, clip) in &self.clips {
            if let ClipType::Audio { ref sample_path, sample_offset_ticks, .. } = clip.clip_type {
                let position_secs = ticks_to_secs_precise(clip.start_tick, self.ppqn, bpm);
                let duration_secs = ticks_to_secs_precise(clip.duration_ticks, self.ppqn, bpm);
                let offset_secs = ticks_to_secs_precise(sample_offset_ticks, self.ppqn, bpm);

                audio_proxy.send(GuiCommand::LoadClip {
                    clip_id: clip.id,
                    path: sample_path.clone(),
                    position_secs,
                    duration_secs,
                    offset_secs,
                    track_index: *track_id,
                    scene_index: 0,
                });
            }
        }
        self.needs_full_sync = false;
    }
}

/// Geometría compartida de la Playlist / Timeline (OpenStudio).
///
/// `TRACK_ROW_H` es la altura de fila POR DEFECTO (`PlaylistState::row_h`
/// arranca acá, sincronizada con el mixer lateral). El zoom vertical
/// (`Alt` + rueda) la cambia sólo en la playlist: el scroll vertical de ambos
/// paneles se sincroniza por índice de fila, así que con zoom vertical activo
/// las filas pueden desalinearse del mixer — se acepta a cambio del zoom.
pub const TRACK_ROW_H: f32 = 54.0;
/// Alto de la regla de compases (ruler / timebar superior).
pub const RULER_H: f32 = 24.0;
/// Límites del zoom temporal (px por tick).
pub const MIN_ZOOM_X: f32 = 0.005;
pub const MAX_ZOOM_X: f32 = 2.0;
/// Límites del zoom vertical (altura de fila en px).
///
/// `MIN_ROW_H` deja lugar al título del clip (14px) + forma de onda mínima;
/// `MAX_ROW_H` evita filas gigantes que rompan el scroll o el layout.
pub const MIN_ROW_H: f32 = 28.0;
pub const MAX_ROW_H: f32 = 160.0;

#[inline]
fn px_to_ticks(px: f32, zoom_x: f32) -> u64 {
    (px.max(0.0) / zoom_x.max(0.0001)) as u64
}

#[inline]
fn ticks_to_px(ticks: u64, zoom_x: f32) -> f32 {
    ticks as f32 * zoom_x
}

/// Ticks por compás según la firma actual (`SIG num/den`).
///
/// Un compás tiene `beats_per_bar` negras; cada negra son `ppqn` ticks.
/// La división (`den`) no cambia la duración en este secuenciador (el PPQN es
/// por negra), pero se conserva como parámetro para validar la firma.
pub fn ticks_per_bar(ppqn: u64, beats_per_bar: u32) -> u64 {
    ppqn.max(1) * beats_per_bar.max(1) as u64
}

/// Número de compás (1-based) que contiene `tick`.
pub fn bar_number_at_tick(tick: u64, ticks_per_bar: u64) -> u32 {
    (tick / ticks_per_bar.max(1)) as u32 + 1
}

/// Posición X (px, relativa al inicio del grid) del compás `bar` (1-based).
pub fn bar_to_px(bar: u32, ticks_per_bar: u64, zoom_x: f32) -> f32 {
    (bar.saturating_sub(1) as u64 * ticks_per_bar.max(1)) as f32 * zoom_x
}

/// Ticks de la subdivisión de grilla (`quantize`) para un `denominador` dado.
///
/// `1/4` = una negra (`ppqn`), `1/8` = media negra, `1/16` = un cuarto.
/// El numerador se ignora a propósito: el snap siempre subdivide la redonda
/// (`4/den` de compás de 4/4), igual que el selector del footer.
pub fn snap_step_ticks(ppqn: u64, grid_denominator: u32) -> u64 {
    let ppqn = ppqn.max(1);
    match grid_denominator.max(1) {
        2 => ppqn * 2,
        4 => ppqn,
        8 => ppqn / 2,
        16 => ppqn / 4,
        d => ppqn * 4 / d as u64,
    }
    .max(1)
}

/// Zoom con acote a `[MIN_ZOOM_X, MAX_ZOOM_X]`. Función pura para testear.
pub fn clamped_zoom(zoom: f32) -> f32 {
    if !zoom.is_finite() {
        return 0.04;
    }
    zoom.clamp(MIN_ZOOM_X, MAX_ZOOM_X)
}

/// Factor de zoom tras pulsar `+` / `-` (×1.25 / ÷1.25, acotado).
pub fn step_zoom(zoom: f32, zoom_in: bool) -> f32 {
    clamped_zoom(if zoom_in { zoom * 1.25 } else { zoom / 1.25 })
}

/// Altura de fila acotada a `[MIN_ROW_H, MAX_ROW_H]`. Función pura para testear.
pub fn clamped_row_h(h: f32) -> f32 {
    if !h.is_finite() {
        return TRACK_ROW_H;
    }
    h.clamp(MIN_ROW_H, MAX_ROW_H)
}

/// Altura de fila tras un paso de zoom vertical (×1.25 / ÷1.25, acotada).
pub fn step_row_h(h: f32, zoom_in: bool) -> f32 {
    clamped_row_h(if zoom_in { h * 1.25 } else { h / 1.25 })
}

/// `true` si la rueda sube (acercar / agrandar).
///
/// En GPUI la rueda que baja suma delta positivo (`scroll_offset += delta`),
/// así que subir es `y < 0`. Acepta `Pixels` (trackpads) y `Lines` (rueda
/// clásica); si el gesto es puramente horizontal se mira `x`.
pub fn wheel_zoom_in(delta: &ScrollDelta) -> bool {
    let (dx, dy) = match *delta {
        ScrollDelta::Pixels(p) => (p.x.as_f32(), p.y.as_f32()),
        ScrollDelta::Lines(p) => (p.x, p.y),
    };
    let d = if dy != 0.0 { dy } else { dx };
    d < 0.0
}

/// Nuevo offset horizontal (`<= 0`) tras un zoom anclado al cursor.
///
/// `cursor_vx` es el x del cursor relativo al viewport; `header_w` el ancho
/// de la columna de headers (contenido no temporal). Se calcula el tick bajo
/// el cursor con el zoom viejo y se elige el offset que lo deja bajo el
/// cursor con el zoom nuevo, acotado al recorrido real del contenido.
/// Función pura para testear.
pub fn anchor_h_offset(
    cursor_vx: f32,
    header_w: f32,
    old_zoom: f32,
    new_zoom: f32,
    old_offset_x: f32,
    viewport_w: f32,
    content_w: f32,
) -> f32 {
    let old_zoom = old_zoom.max(0.0001);
    let tick = (cursor_vx - old_offset_x - header_w) / old_zoom;
    let new_offset = cursor_vx - header_w - tick * new_zoom;
    let min = -(content_w - viewport_w).max(0.0);
    new_offset.clamp(min, 0.0)
}

/// Nuevo offset vertical (`<= 0`) tras un zoom de altura de fila anclado.
///
/// Las filas escalan uniformemente bajo la regla (`ruler_h` queda fija), así
/// que el contenido bajo el cursor se reubica por factor `new/old` y el
/// offset compensa para que no se mueva. Función pura para testear.
pub fn anchor_v_offset(
    cursor_vy: f32,
    ruler_h: f32,
    old_row_h: f32,
    new_row_h: f32,
    old_offset_y: f32,
    viewport_h: f32,
    content_h: f32,
) -> f32 {
    let scale = if old_row_h > 0.0 {
        new_row_h / old_row_h
    } else {
        1.0
    };
    let content_y = cursor_vy - old_offset_y;
    let new_content_y = ruler_h + (content_y - ruler_h) * scale;
    let new_offset = cursor_vy - new_content_y;
    let min = -(content_h - viewport_h).max(0.0);
    new_offset.clamp(min, 0.0)
}

pub fn snap_ticks(ticks: u64, grid_ticks: u64) -> u64 {
    if grid_ticks == 0 {
        return ticks;
    }
    ((ticks as f64 / grid_ticks as f64).round() as u64).saturating_mul(grid_ticks)
}

fn ticks_to_pixel_x(ticks: u64, ppqn: u64, zoom_x: f32, playlist_offset_x: f32) -> f32 {
    let ppqn = ppqn.max(1) as f32;
    let pixels_per_beat = ppqn * zoom_x;
    (ticks as f32 / ppqn) * pixels_per_beat + playlist_offset_x
}

pub fn loop_display_tick(
    current_tick: u64,
    loop_start_ticks: u64,
    loop_end_ticks: u64,
    loop_enabled: bool,
) -> u64 {
    if !loop_enabled {
        return current_tick;
    }
    let len = loop_end_ticks.saturating_sub(loop_start_ticks);
    if len == 0 || loop_end_ticks <= loop_start_ticks {
        return current_tick;
    }
    if current_tick < loop_start_ticks {
        return current_tick;
    }
    if current_tick >= loop_end_ticks {
        return loop_start_ticks + ((current_tick - loop_start_ticks) % len);
    }
    current_tick
}

fn samples_to_ticks_precise(sample_count: u64, ppqn: u64, bpm: f64, sample_rate: u32) -> u64 {
    let ppqn = ppqn.max(1);
    if bpm <= 0.0 || sample_rate == 0 {
        return 0;
    }
    let seconds = sample_count as f64 / sample_rate as f64;
    let seconds_per_tick = (60.0 / bpm) / ppqn as f64;
    (seconds / seconds_per_tick).round() as u64
}

fn ticks_to_samples_precise(ticks: u64, ppqn: u64, bpm: f64, sample_rate: u32) -> u64 {
    let ppqn = ppqn.max(1);
    if bpm <= 0.0 || sample_rate == 0 {
        return 0;
    }
    let seconds_per_tick = (60.0 / bpm) / ppqn as f64;
    (ticks as f64 * seconds_per_tick * sample_rate as f64).round() as u64
}

#[inline]
fn ticks_to_secs_precise(ticks: u64, ppqn: u64, bpm: f64) -> f32 {
    if ppqn == 0 || bpm <= 0.0 {
        return 0.0;
    }
    let seconds_per_tick = (60.0 / bpm) / (ppqn as f64);
    (ticks as f64 * seconds_per_tick) as f32
}

fn load_sample_info(path: &PathBuf, ppqn: u64, bpm: f64) -> (u64, Vec<f32>) {
    let mut peaks = Vec::new();

    match hound::WavReader::open(path) {
        Ok(mut reader) => {
            let spec = reader.spec();
            let total_frames = reader.duration() as u64;
            let channels = spec.channels.max(1) as u64;

            if spec.sample_rate > 0 && total_frames > 0 && bpm > 0.0 {
                let duration_sec = total_frames as f64 / spec.sample_rate as f64;
                let seconds_per_tick = (60.0 / bpm) / ppqn.max(1) as f64;
                let calculated_ticks = (duration_sec / seconds_per_tick).round() as u64;

                let target_peaks = 512;
                let step_frames = ((total_frames as usize) / target_peaks).max(1);
                let step_samples = step_frames * channels as usize;

                let samples: Vec<f32> = match spec.sample_format {
                    hound::SampleFormat::Int => {
                        let max_val = (1i64 << (spec.bits_per_sample - 1)) as f32;
                        reader.samples::<i32>()
                            .filter_map(|s| s.ok())
                            .map(|s| (s as f32 / max_val).abs())
                            .collect()
                    }
                    hound::SampleFormat::Float => {
                        reader.samples::<f32>()
                            .filter_map(|s| s.ok())
                            .map(|s| s.abs())
                            .collect()
                    }
                };

                for chunk in samples.chunks(step_samples) {
                    let max_peak = chunk.iter().cloned().fold(0.0f32, f32::max);
                    peaks.push(max_peak.clamp(0.0, 1.0));
                }

                return (calculated_ticks.max(1), peaks);
            }
        }
        Err(err) => {
            eprintln!("[PLAYLIST ERROR] Hound no pudo abrir {:?}: {}", path, err);
        }
    }

    (0, peaks)
}

pub fn build_audio_clip(
    id: usize,
    name: String,
    path: &PathBuf,
    start_tick: u64,
    ppqn: u64,
    bpm: f64,
    color: Hsla,
) -> PlaylistClip {
    let (duration_ticks, peaks) = load_sample_info(path, ppqn, bpm);
    let duration_ticks = duration_ticks.max(1);

    PlaylistClip {
        id,
        name,
        start_tick,
        duration_ticks,
        clip_type: ClipType::Audio {
            sample_path: path.to_string_lossy().to_string(),
            peaks,
            sample_offset_ticks: 0,
            total_sample_ticks: duration_ticks,
        },
        color,
    }
}

// =========================================================================
// DRAG & DROP DE CLIPS (botón izquierdo sobre el clip)
// =========================================================================

/// Claves de pista (índices al vector del modo activo) en orden de fila.
fn playlist_row_keys(s: &AppState) -> Vec<usize> {
    match s.mode {
        crate::app::AppMode::OpenLive => &s.live_tracks,
        crate::app::AppMode::OpenStudio => &s.studio_tracks,
    }
    .iter()
    .enumerate()
    .filter(|(_, t)| !t.is_master)
    .map(|(i, _)| i)
    .collect()
}

/// Finaliza el gesto de arrastre activo, si lo hay.
///
/// Si hubo movimiento y es un clip de audio, sincroniza la nueva posición
/// con el motor (`UpdateClipBounds`, un solo mensaje al soltar, no por
/// frame). Siempre limpia el gesto, arma la supresión del click posterior y
/// notifica (repaint inmediato).
fn finish_clip_drag(cx: &mut App) {
    let st = state(cx);
    st.update(cx, |s, cx| {
        let Some(drag) = s.playlist_state.clip_drag.take() else {
            return;
        };
        let clip_id = drag.clip_id;
        if drag.moved {
            s.playlist_state.clip_click_suppress = true;
            let ppqn = s.playlist_state.ppqn.max(1);
            let bpm = s.transport.bpm;
            let found = s
                .playlist_state
                .clips
                .iter()
                .find(|(_, c)| c.id == clip_id)
                .map(|(k, c)| (*k, c.start_tick, c.duration_ticks, c.clip_type.clone()));
            if let Some((key, start, dur, clip_type)) = found {
                if let ClipType::Audio {
                    sample_offset_ticks,
                    ..
                } = clip_type
                {
                    s.audio_proxy.send(GuiCommand::UpdateClipBounds {
                        clip_id,
                        track_index: key,
                        scene_index: 0,
                        position_secs: ticks_to_secs_precise(start, ppqn, bpm),
                        duration_secs: ticks_to_secs_precise(dur, ppqn, bpm),
                        offset_secs: ticks_to_secs_precise(sample_offset_ticks, ppqn, bpm),
                    });
                }
            }
        }
        cx.notify();
    });
}

/// Aplica un paso del arrastre: nueva posición con snap + fila destino.
///
/// Traduce `cursor - grab` (coords de ventana) a ticks/fila con el origen
/// medido de la zona (`zone`), reasigna el clip EN VIVO —el propio clip en
/// movimiento con su borde de selección es la previsualización en tiempo
/// real— y notifica (un frame por evento, como los faders).
fn update_clip_drag(cx: &mut App, cursor: (f32, f32), zone: [f32; 4]) {
    let st = state(cx);
    st.update(cx, |s, cx| {
        let Some(drag) = s.playlist_state.clip_drag else {
            return;
        };
        let clip_id = drag.clip_id;
        let ppqn = s.playlist_state.ppqn.max(1);
        let zoom = s.playlist_state.zoom_x;
        let row_h = s.playlist_state.track_height();
        let grid_step = snap_step_ticks(ppqn, s.playlist_state.grid_denominator).max(1);
        // Horizontal: cursor menos agarre → px relativos a la zona → ticks → snap.
        let rel_x = (cursor.0 - drag.grab_dx_px - zone[0]).max(0.0);
        let new_tick = snap_ticks(px_to_ticks(rel_x, zoom), grid_step);
        // Vertical: fila bajo el punto de agarre, acotada a las existentes.
        let rows = playlist_row_keys(s);
        if rows.is_empty() {
            return;
        }
        let rel_y = cursor.1 - drag.grab_dy_px - zone[1];
        let row = ((rel_y / row_h).floor() as isize)
            .clamp(0, rows.len() as isize - 1) as usize;
        let new_key = rows[row];
        let mut changed = false;
        if let Some(entry) = s
            .playlist_state
            .clips
            .iter_mut()
            .find(|(_, c)| c.id == clip_id)
        {
            if entry.1.start_tick != new_tick {
                entry.1.start_tick = new_tick;
                changed = true;
            }
            if entry.0 != new_key {
                entry.0 = new_key;
                changed = true;
            }
        }
        if changed {
            if let Some(d) = s.playlist_state.clip_drag.as_mut() {
                d.moved = true;
            }
        }
        cx.notify();
    });
}

/// Aplica un evento de rueda con modificadores como zoom de la Playlist.
///
/// - `Ctrl` + rueda (sin `Shift` ni `Alt`): zoom horizontal (`zoom_x`,
///   acotado a `[MIN_ZOOM_X, MAX_ZOOM_X]`).
/// - `Alt` + rueda o `Ctrl` + `Shift` + rueda: zoom vertical (`row_h`,
///   acotado a `[MIN_ROW_H, MAX_ROW_H]`).
/// - Sin modificadores: no hace nada (deja el scroll normal) → `false`.
///
/// Rueda arriba acerca/agranda y rueda abajo aleja/achica. El viewport se
/// compensa con `anchor_*_offset` para que el tick y la fila bajo el cursor
/// no se muevan (focus point en el cursor). Devuelve `true` si consumió el
/// evento (el llamador debe frenar la propagación para que no scrollee).
/// `total_ticks` / `header_width` / `ruler_h` / `row_count` describen el
/// lienzo actual (los calcula `render`).
fn apply_playlist_wheel_zoom(
    s: &mut AppState,
    delta: &ScrollDelta,
    control: bool,
    alt: bool,
    shift: bool,
    cursor_win: (f32, f32),
    total_ticks: u64,
    header_width: f32,
    ruler_h: f32,
    row_count: usize,
) -> bool {
    let vertical = alt || (control && shift);
    let horizontal = control && !shift && !alt;
    if !vertical && !horizontal {
        return false;
    }
    let zoom_in = wheel_zoom_in(delta);
    if horizontal {
        let old_zoom = clamped_zoom(s.playlist_state.zoom_x);
        let new_zoom = step_zoom(old_zoom, zoom_in);
        if (new_zoom - old_zoom).abs() <= f32::EPSILON {
            // Ya en el límite: igual se consume para no scrollear de más.
            return true;
        }
        let h = s.playlist_scroll_h.clone();
        let bounds = h.bounds();
        let viewport_w = bounds.size.width.as_f32();
        let cursor_vx = cursor_win.0 - bounds.origin.x.as_f32();
        let content_w = (header_width + total_ticks as f32 * new_zoom).max(800.0);
        let new_offset_x = anchor_h_offset(
            cursor_vx,
            header_width,
            old_zoom,
            new_zoom,
            h.offset().x.as_f32(),
            viewport_w,
            content_w,
        );
        h.set_offset(point(px(new_offset_x), h.offset().y));
        s.playlist_state.zoom_x = new_zoom;
    } else {
        let old_h = s.playlist_state.track_height();
        let new_h = step_row_h(old_h, zoom_in);
        if (new_h - old_h).abs() <= f32::EPSILON {
            return true;
        }
        let v = s.playlist_scroll_v.clone();
        let bounds = v.bounds();
        let viewport_h = bounds.size.height.as_f32();
        let cursor_vy = cursor_win.1 - bounds.origin.y.as_f32();
        let content_h = ruler_h + row_count as f32 * new_h;
        let new_offset_y = anchor_v_offset(
            cursor_vy,
            ruler_h,
            old_h,
            new_h,
            v.offset().y.as_f32(),
            viewport_h,
            content_h,
        );
        v.set_offset(point(v.offset().x, px(new_offset_y)));
        s.playlist_state.row_h = new_h;
    }
    true
}

pub fn render(cx: &mut Context<HikaruApp>) -> AnyElement {
    let app = state(cx).read(cx);
    let pl = &app.playlist_state;
    let ppqn = pl.ppqn.max(1);
    let zoom_x = pl.zoom_x;
    let header_width = pl.header_width;
    let _grid_num = pl.grid_numerator;
    let grid_den = pl.grid_denominator;
    let playhead_tick = pl.playhead_tick;
    let loop_start = pl.loop_start_ticks;
    let loop_end = pl.loop_end_ticks;
    let loop_active = pl.loop_region_active;
    let selected = pl.selected_clips.clone();
    let is_looping = app.is_looping;
    let audio_proxy = app.audio_proxy.clone();
    let bpm = app.transport.bpm;
    let sample_rate = app.transport.sample_rate.get() as u32;
    let beats_per_bar = app.transport.beats_per_bar;
    let beat_division = app.transport.beat_division;
    let tracks = match app.mode {
        crate::app::AppMode::OpenLive => &app.live_tracks,
        crate::app::AppMode::OpenStudio => &app.studio_tracks,
    };
    let dragged_sample = app.dragged_sample.clone();
    let selected_track = app.selected_track_index;
    let playlist_scroll_h = app.playlist_scroll_h.clone();
    let playlist_scroll_v = app.playlist_scroll_v.clone();
    drop(app);

    // Compás real según SIG (BPM sólo afecta a segundos, no a ticks).
    let ticks_per_bar_val = ticks_per_bar(ppqn, beats_per_bar);
    let display_tick = loop_display_tick(playhead_tick, loop_start, loop_end, is_looping && loop_active);

    let non_master: Vec<(usize, &Track)> = tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.is_master)
        .collect();

    let track_height = clamped_row_h(pl.row_h);
    let ruler_h = RULER_H;
    // Geometría del lienzo: columna fija de headers a la izquierda y grilla
    // temporal a su derecha. `grid_ox` es el origen X de TODO lo temporal
    // (regla, líneas, clips, playhead, zonas de seek/drop): el compás 1
    // arranca exactamente ahí y el área bajo él queda limpia para clips.
    let grid_ox = header_width;
    // Altura del lienzo: N filas × altura actual (zoom vertical).
    let row_count = non_master.len().max(1);
    let canvas_h = ruler_h + row_count as f32 * track_height;
    let total_ticks = (ticks_per_bar_val * 128).max(playhead_tick + ticks_per_bar_val * 16);
    let canvas_w = (header_width + total_ticks as f32 * zoom_x).max(800.0);
    let grid_w = (canvas_w - header_width).max(1.0);

    // Cabeceras de fila LIMPIAS: sólo identidad de pista (número + nombre).
    // Sin faders, sin pan, sin S/M/R, sin dB — la mezcla vive en el mixer
    // lateral (`arranger_view.rs`). El click selecciona la pista destino
    // (navegación de arreglo, no mezcla).
    let mut track_headers: Vec<AnyElement> = Vec::new();
    for (idx, track) in &non_master {
        let vec_idx = *idx;
        let tname = track.name.clone();
        let row_label = format!("TRK {:02}", track.id);
        let is_selected_row = selected_track == vec_idx;
        track_headers.push(
            v_flex()
                .w(px(header_width))
                .h(px(track_height))
                .bg(rgb(0x1C1C20))
                .border_1()
                .border_color(if is_selected_row {
                    rgb(0x5AB4FF)
                } else {
                    rgb(0x2D2D37)
                })
                .p(px(4.0))
                .gap(px(1.0))
                .justify_center()
                .id(format!("pl_row_{}", vec_idx))
                .test_support()
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, cx| {
                        state.selected_track_index = vec_idx;
                        cx.notify();
                    });
                })
                .child(Label::new(row_label).text_xs())
                .child(Label::new(tname).text_xs())
                .into_any_element(),
        );
    }

    // Bounds de la zona de grilla en coords de ventana (patrón `record_bounds`
    // del arranger): `mouse_position()` viene en coords de ventana, así que el
    // mapeo px→ticks/fila resta el origen real medido cada frame, no una
    // constante. La zona ES la grilla (arranca en `grid_ox`), sin headers.
    // Se crea acá arriba porque los handlers de drag de los clips (definidos
    // abajo) también lo necesitan para traducir el cursor a ticks/fila.
    let zone_bounds: Rc<Cell<[f32; 4]>> = Rc::new(Cell::new([0.0; 4]));
    let zone_bounds_clip = zone_bounds.clone();
    let zone_bounds_drop = zone_bounds.clone();
    let zone_bounds_rec = zone_bounds.clone();

    let mut clip_elems: Vec<AnyElement> = Vec::new();
    for (track_id, clip) in &pl.clips {
        let is_sel = selected.contains(&clip.id);
        let clip_name = clip.name.clone();
        let clip_start = clip.start_tick;
        let clip_dur = clip.duration_ticks;
        let clip_x = grid_ox + ticks_to_px(clip_start, zoom_x);
        let clip_w = ticks_to_px(clip_dur, zoom_x).max(12.0);
        let track_row = non_master.iter().position(|(i, _)| *i == *track_id).unwrap_or(0);
        let clip_y = ruler_h + track_row as f32 * track_height + 1.0;
        let clip_h = track_height - 2.0;
        let clip_id = clip.id;

        // --- ClipView: bloque horizontal con título + contenido ---------------
        // Posición dinámica: x = grid_ox + start_tick × zoom (siempre a la
        // derecha de los headers), w = duration × zoom,
        // y = fila de `track_index` × altura actual (`track_height`).
        let title_h = 14.0_f32;
        let body_h = (clip_h - title_h).max(8.0);
        // Fondo del clip SIEMPRE opaco (`a = 1.0`): el rectángulo del clip
        // debe tapar las líneas verticales de la grilla que pasan por detrás.
        // El orden de pintado (grilla → clips → playhead) pone la grilla
        // debajo, y la opacidad total garantiza que no se transparente.
        let mut clip_bg: Hsla = if is_sel { rgb(0x325078).into() } else { clip.color };
        clip_bg.a = 1.0;
        let clip_border = if is_sel {
            rgb(0xFFC800)
        } else {
            rgba(0xFFFFFF66)
        };
        let title_text = clip_name.clone();
        let dragging_this = pl
            .clip_drag
            .map(|d| d.clip_id == clip_id)
            .unwrap_or(false);
        let zone_down = zone_bounds_clip.clone();
        let base_clip = div()
            .w(px(clip_w))
            .h(px(clip_h))
            .absolute()
            .left(px(clip_x))
            .top(px(clip_y))
            .bg(clip_bg)
            .border_1()
            .border_color(clip_border)
            .rounded(px(2.0))
            .overflow_hidden()
            .id(format!("pl_clip_{}", clip_id))
            .test_support()
            // Cursor de movimiento durante el arrastre (GPUI no tiene
            // `Move`/`Grabbing`: `ClosedHand` es el equivalente, igual que
            // en el pan del mixer).
            .cursor(if dragging_this {
                CursorStyle::ClosedHand
            } else {
                CursorStyle::Arrow
            })
            .on_mouse_down(gpui_kit::MouseButton::Left, move |event, _, cx| {
                let zb = zone_down.get();
                let st = state(cx);
                st.update(cx, |s, cx| {
                    let (key, start) = match s
                        .playlist_state
                        .clips
                        .iter()
                        .find(|(_, c)| c.id == clip_id)
                    {
                        Some((k, c)) => (*k, c.start_tick),
                        None => return,
                    };
                    let zoom = s.playlist_state.zoom_x;
                    let row_h = s.playlist_state.track_height();
                    let row = playlist_row_keys(s)
                        .iter()
                        .position(|k| *k == key)
                        .unwrap_or(0);
                    s.playlist_state.clip_drag = Some(PlaylistClipDrag {
                        clip_id,
                        orig_tick: start,
                        orig_track: key,
                        grab_dx_px: event.position.x.as_f32()
                            - (zb[0] + ticks_to_px(start, zoom)),
                        grab_dy_px: event.position.y.as_f32()
                            - (zb[1] + row as f32 * row_h + 1.0),
                        moved: false,
                    });
                    // El clip agarrado queda seleccionado (borde amarillo que
                    // lo sigue en vivo = feedback del arrastre).
                    s.playlist_state.selected_clips = vec![clip_id];
                    s.playlist_state.clip_click_suppress = false;
                    cx.notify();
                });
            })
            .on_click(move |event, _, cx| {
                let shift = event.modifiers().shift;
                let st = state(cx);
                st.update(cx, |state, cx| {
                    if state.playlist_state.clip_click_suppress {
                        // Click que cierra un drag con movimiento: la
                        // selección ya quedó fijada al agarrar.
                        state.playlist_state.clip_click_suppress = false;
                        return;
                    }
                    if shift {
                        if state.playlist_state.selected_clips.contains(&clip_id) {
                            state.playlist_state.selected_clips.retain(|&id| id != clip_id);
                        } else {
                            state.playlist_state.selected_clips.push(clip_id);
                        }
                    } else {
                        state.playlist_state.selected_clips = vec![clip_id];
                    }
                    cx.notify();
                });
            })
            // Título del clip (ej. "Cymatics - X Full Drum Loop 29").
            .child(
                div()
                    .w_full()
                    .h(px(title_h))
                    .flex_shrink_0()
                    .bg(rgba(0x00000066))
                    .px(px(4.0))
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .child(Label::new(title_text).text_xs()),
            );
        if let ClipType::Audio { peaks, sample_offset_ticks, total_sample_ticks, .. } = &clip.clip_type {
            let peaks = peaks.clone();
            let soff = *sample_offset_ticks;
            let ttot = (*sample_offset_ticks + *total_sample_ticks).max(1);
            let clip_el = base_clip.child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let cx0 = bounds.origin.x;
                        let cy = bounds.origin.y + px(bounds.size.height.as_f32() / 2.0);
                        let n = peaks.len();
                        if n == 0 {
                            return;
                        }
                        let start_ratio = soff as f32 / ttot as f32;
                        let dur_ratio = clip_dur as f32 / ttot as f32;
                        let step = 2.0;
                        let steps = (bounds.size.width.as_f32() / step) as usize;
                        for i in 0..steps {
                            let local = i as f32 / steps.max(1) as f32;
                            let sample_norm = start_ratio + local * dur_ratio;
                            let peak_idx = (sample_norm * n as f32) as usize;
                            if let Some(&pv) = peaks.get(peak_idx) {
                                let bh = bounds.size.height.as_f32() * 0.8 * pv;
                                if bh > 0.5 {
                                    let x = cx0 + px(i as f32 * step);
                                    let mut path = PathBuilder::fill();
                                    path.move_to(point(x, cy - px(bh * 0.5)));
                                    path.line_to(point(x + px(1.0), cy - px(bh * 0.5)));
                                    path.line_to(point(x + px(1.0), cy + px(bh * 0.5)));
                                    path.line_to(point(x, cy + px(bh * 0.5)));
                                    path.close();
                                    if let Ok(p) = path.build() {
                                        window.paint_path(p, rgba(0xFFFFFFB3));
                                    }
                                }
                            }
                        }
                    },
                )
                .w_full()
                .h(px(body_h)),
            );
            clip_elems.push(clip_el.into_any_element());
        } else {
            // Pattern / Automation / MIDI: bloques de eventos proporcionales.
            let is_pattern = matches!(clip.clip_type, ClipType::Pattern { .. });
            let clip_el = base_clip.child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let w = bounds.size.width.as_f32();
                        let h = bounds.size.height.as_f32();
                        if w <= 0.0 || h <= 0.0 {
                            return;
                        }
                        if is_pattern {
                            // 16 pasos estilo step-sequencer como placeholder
                            // de eventos MIDI hasta que el clip guarde notas.
                            let steps = 16usize;
                            let bw = (w / steps as f32 * 0.6).max(2.0);
                            for i in 0..steps {
                                if i % 2 == 0 {
                                    continue;
                                }
                                let x = bounds.origin.x + px(i as f32 * w / steps as f32 + 1.0);
                                let bh = h * 0.55;
                                let mut path = PathBuilder::fill();
                                path.move_to(point(x, bounds.origin.y + px((h - bh) / 2.0)));
                                path.line_to(point(x + px(bw), bounds.origin.y + px((h - bh) / 2.0)));
                                path.line_to(point(x + px(bw), bounds.origin.y + px((h + bh) / 2.0)));
                                path.line_to(point(x, bounds.origin.y + px((h + bh) / 2.0)));
                                path.close();
                                if let Ok(p) = path.build() {
                                    window.paint_path(p, rgba(0xFFFFFFB3));
                                }
                            }
                        } else {
                            // Automation: línea horizontal central.
                            let mut path = PathBuilder::stroke(px(1.0));
                            let y = bounds.origin.y + bounds.size.height / 2.0;
                            path.move_to(point(bounds.origin.x, y));
                            path.line_to(point(bounds.origin.x + bounds.size.width, y));
                            if let Ok(p) = path.build() {
                                window.paint_path(p, rgba(0xFFFFFFB3));
                            }
                        }
                    },
                )
                .w_full()
                .h(px(body_h)),
            );
            clip_elems.push(clip_el.into_any_element());
        }
    }

    let playhead_x = ticks_to_px(display_tick, zoom_x);
    // Grilla según quantize actual (1/4, 1/8, 1/16): subdivisión de la redonda.
    // Las líneas principales caen cada compás SIG (`ticks_per_bar_val`).
    let snap_step_ticks = snap_step_ticks(ppqn, grid_den).max(1);
    let mut grid_lines: Vec<AnyElement> = Vec::new();
    let mut step = 0u64;
    let mut bar_num = 1u32;
    while step <= total_ticks {
        let x = ticks_to_px(step, zoom_x);
        if x > grid_w {
            break;
        }
        // Toda línea temporal cuelga de `grid_ox`: el compás 1 (x=0) queda
        // justo a la derecha de los headers, nunca debajo.
        let gx = grid_ox + x;
        let is_main = step % ticks_per_bar_val == 0;
        grid_lines.push(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let col = if is_main { rgb(0x323232) } else { rgb(0x1C1C1C) };
                    let mut path = PathBuilder::stroke(px(1.0));
                    path.move_to(point(bounds.origin.x, bounds.origin.y));
                    path.line_to(point(
                        bounds.origin.x,
                        bounds.origin.y + bounds.size.height,
                    ));
                    if let Ok(p) = path.build() {
                        window.paint_path(p, col);
                    }
                },
            )
            .absolute()
            .left(px(gx))
            .top(px(ruler_h))
            .w(px(1.0))
            .h(px(canvas_h - ruler_h))
            .into_any_element(),
        );
        if is_main {
            grid_lines.push(
                Label::new(bar_num.to_string())
                    .text_xs()
                    .absolute()
                    .left(px(gx + 4.0))
                    .top(px(2.0))
                    .into_any_element(),
            );
            bar_num += 1;
        }
        step += snap_step_ticks;
    }

    let loop_render = if loop_active && loop_end > loop_start {
        let lx0 = ticks_to_px(loop_start, zoom_x);
        let lx1 = ticks_to_px(loop_end, zoom_x);
        Some((lx0, lx1))
    } else {
        None
    };

    let mut drop_zones: Vec<AnyElement> = Vec::new();
    if let Some(ref path) = dragged_sample {
        let file_name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        drop_zones.push(
            div()
                .absolute()
                .left(px(header_width + 10.0))
                .top(px(ruler_h + 10.0))
                .bg(rgba(0x0096BE80))
                .rounded(px(4.0))
                .p(px(4.0))
                .child(Label::new(format!("🎵 {}", file_name)).text_xs())
                .into_any_element(),
        );
    }

    let drop_handler = if dragged_sample.is_some() {
        let audio_proxy = audio_proxy.clone();
        let track_ids: Vec<usize> = non_master.iter().map(|(tid, _)| *tid).collect();
        Some(
            div()
                .absolute()
                .left(px(grid_ox))
                .top(px(ruler_h))
                .w(px(grid_w))
                .h(px(canvas_h - ruler_h))
                .id("pl_drop_handler")
                .on_click(move |event, _, cx| {
                    let st = state(cx);
                    st.update(cx, |state, cx| {
                        if let Some(ref path) = state.dragged_sample {
                            let Some(pos) = event.mouse_position() else {
                                return;
                            };
                            let zb = zone_bounds_drop.get();
                            let rel_x = pos.x.as_f32() - zb[0];
                            let raw = px_to_ticks(rel_x.max(0.0), zoom_x);
                            let drop_tick = snap_ticks(raw, snap_step_ticks);
                            let rel_y = pos.y.as_f32() - zb[1];
                            let track_idx = (rel_y / track_height).floor() as usize;
                            if track_idx < track_ids.len() {
                                let target_tid = track_ids[track_idx];
                                let name = path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string();
                                let pstr = path.to_string_lossy().to_string();
                                let (dur, peaks) = load_sample_info(path, ppqn, bpm);
                                let clip = build_audio_clip(
                                    state.playlist_state.next_clip_id,
                                    name,
                                    path,
                                    drop_tick,
                                    ppqn,
                                    bpm,
                                    rgb(0x205F91).into(),
                                );
                                let cid = state.playlist_state.next_clip_id;
                                state.playlist_state.next_clip_id += 1;
                                state.playlist_state.clips.push((target_tid, clip));
                                state.audio_proxy.send(GuiCommand::LoadClip {
                                    clip_id: cid,
                                    path: pstr,
                                    position_secs: ticks_to_secs_precise(drop_tick, ppqn, bpm),
                                    duration_secs: ticks_to_secs_precise(dur, ppqn, bpm),
                                    offset_secs: 0.0,
                                    track_index: target_tid,
                                    scene_index: 0,
                                });
                            }
                        }
                        state.dragged_sample = None;
                        cx.notify();
                    });
                })
                .into_any_element(),
        )
    } else {
        None
    };

    let zone_bounds_seek = zone_bounds.clone();
    let zone_grid_move = zone_bounds.clone();
    let seek_zone = div()
        .absolute()
        .left(px(grid_ox))
        .top(px(ruler_h))
        .w(px(grid_w))
        .h(px(canvas_h - ruler_h))
        .id("pl_seek_zone")
        .child(
            canvas(
                move |bounds, _, _| {
                    zone_bounds_rec.set([
                        bounds.origin.x.as_f32(),
                        bounds.origin.y.as_f32(),
                        bounds.size.width.as_f32(),
                        bounds.size.height.as_f32(),
                    ]);
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        )
        .on_click(move |event, _, cx| {
            let Some(pos) = event.mouse_position() else {
                return;
            };
            let zb = zone_bounds_seek.get();
            let rel_x = pos.x.as_f32() - zb[0];
            let clicked = px_to_ticks(rel_x.max(0.0), zoom_x);
            let st = state(cx);
            st.update(cx, |state, _| {
                let target = ticks_to_samples_precise(clicked, ppqn, bpm, sample_rate);
                state.audio_proxy.send(GuiCommand::Seek { sample_count: target });
            });
        });

    // Fondo de la regla: sólo sobre la grilla (arranca en `grid_ox`).
    // La esquina superior-izquierda (sobre los headers) la pinta `corner`.
    let children = vec![
        div()
            .absolute()
            .left(px(grid_ox))
            .top(px(0.0))
            .w(px(grid_w))
            .h(px(ruler_h))
            .bg(rgb(0x18181E))
            .into_any_element(),
        div()
            .absolute()
            .left(px(0.0))
            .top(px(0.0))
            .w(px(header_width))
            .h(px(ruler_h))
            .bg(rgb(0x141418))
            .border_r_1()
            .border_color(rgb(0x2D2D37))
            .into_any_element(),
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let col = rgb(0x1C1C1C);
                let mut path = PathBuilder::stroke(px(1.0));
                path.move_to(point(bounds.origin.x, bounds.origin.y));
                path.line_to(point(
                    bounds.origin.x,
                    bounds.origin.y + bounds.size.height,
                ));
                if let Ok(p) = path.build() {
                    window.paint_path(p, col);
                }
            },
        )
        .absolute()
        .left(px(grid_ox))
        .top(px(ruler_h))
        .w(px(1.0))
        .h(px(canvas_h - ruler_h))
        .into_any_element(),
    ];

    // Playhead y bordes de loop en coords de grilla: el canvas cubre todo el
    // contenido pero pinta relativo a su propio origen + `grid_ox`, así sigue
    // alineado con compases y clips haya scroll o no.
    let playhead_win_x = grid_ox + playhead_x;
    let loop_win = loop_render.map(|(a, b)| (grid_ox + a, grid_ox + b));
    let playhead_canvas = canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            // Playhead: marcador de posición de reproducción actual.
            let x = bounds.origin.x + px(playhead_win_x);
            let mut playhead_path = PathBuilder::stroke(px(2.0));
            playhead_path.move_to(point(x, bounds.origin.y));
            playhead_path.line_to(point(x, bounds.origin.y + bounds.size.height));
            if let Ok(p) = playhead_path.build() {
                window.paint_path(p, rgb(0x00FFFF));
            }
            if let Some((lx0, lx1)) = loop_win {
                let top = bounds.origin.y;
                let bot = bounds.origin.y + bounds.size.height;
                for lx in [lx0, lx1] {
                    let mut loop_path = PathBuilder::stroke(px(2.0));
                    let lx = bounds.origin.x + px(lx);
                    loop_path.move_to(point(lx, top));
                    loop_path.line_to(point(lx, bot));
                    if let Ok(p) = loop_path.build() {
                        window.paint_path(p, rgb(0x00C8FF));
                    }
                }
            }
        },
    )
    .absolute()
    .left(px(0.0))
    .top(px(0.0))
    .w(px(canvas_w))
    .h(px(canvas_h));

    let mut all: Vec<AnyElement> = Vec::new();
    // Ruler / Timebar superior: título + compás SIG + quantize clicable.
    // La numeración de compases (1, 2, 3…) se dibuja en `grid_lines` alineada
    // a `ticks_per_bar_val` (SIG real), no a la subdivisión de snap.
    all.push(
        h_flex()
            .items_center()
            .gap(px(6.0))
            .child(Label::new("PLAYLIST / TIMELINE").text_sm().font_weight(FontWeight::BOLD))
            .child(Label::new(format!("SIG {}/{} @ {:.1} BPM", beats_per_bar, beat_division, bpm)).text_xs())
            .child(Label::new(format!("1/{}", grid_den)).text_xs())
            .id("pl_grid_header")
            .on_click(move |_, _, cx| {
                let st = state(cx);
                st.update(cx, |state, cx| {
                    state.playlist_state.grid_denominator = match state.playlist_state.grid_denominator {
                        2 => 4,
                        4 => 8,
                        8 => 16,
                        _ => 2,
                    };
                    cx.notify();
                });
            })
            .child(
                Button::new("pl_zoom_out").rounded(gpui_kit::component::button::ButtonRounded::None)
                    .label("-")
                    .compact()
                    .on_click(move |_, _, cx| {
                        let st = state(cx);
                        st.update(cx, |state, cx| {
                            state.playlist_state.zoom_x = step_zoom(state.playlist_state.zoom_x, false);
                            cx.notify();
                        });
                    }),
            )
            .child(
                Button::new("pl_zoom_in").rounded(gpui_kit::component::button::ButtonRounded::None)
                    .label("+")
                    .compact()
                    .on_click(move |_, _, cx| {
                        let st = state(cx);
                        st.update(cx, |state, cx| {
                            state.playlist_state.zoom_x = step_zoom(state.playlist_state.zoom_x, true);
                            cx.notify();
                        });
                    }),
            )
            .into_any_element(),
    );
    // Navegación: scroll vertical externo + horizontal interno (mismo patrón
    // anidado que el arranger). La columna de headers viaja DENTRO del
    // contenido, así cada cabecera queda pegada a su fila haya scroll o no.
    // Los scrolls usan handles explícitos del estado (`playlist_scroll_h/v`,
    // como el mixer) para que el zoom con rueda anclado al cursor pueda
    // compensar el viewport; las scrollbars visibles las pinta el kit sobre
    // esos mismos handles.
    all.push(
        div()
            .flex_1()
            .min_h_0()
            .relative()
            .vertical_scrollbar(&playlist_scroll_v)
            .child(
                div()
                    .id("playlist_vscroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&playlist_scroll_v)
                    .child(
                        div()
                            .w_full()
                            .relative()
                            .horizontal_scrollbar(&playlist_scroll_h)
                            .child(
                                div()
                                    .id("playlist_scroll")
                                    .w_full()
                                    .overflow_x_scroll()
                                    .track_scroll(&playlist_scroll_h)
                                    .child(
                                        div()
                                            .id("playlist_grid")
                                            .w(px(canvas_w))
                                            .h(px(canvas_h))
                                            .relative()
                                            // Move/Up del drag de clips a nivel de grilla (no
                                            // del clip): el cursor sale del clip al arrastrar
                                            // y los handlers del propio clip dejarían de
                                            // disparar. Acá cubren toda la zona.
                                            .on_mouse_move(move |event, _, cx| {
                                                if event.pressed_button
                                                    != Some(gpui_kit::MouseButton::Left)
                                                {
                                                    // Botón soltado fuera: se consolida.
                                                    finish_clip_drag(cx);
                                                    return;
                                                }
                                                if state(cx).read(cx).playlist_state.clip_drag.is_none() {
                                                    return;
                                                }
                                                let zb = zone_grid_move.get();
                                                update_clip_drag(
                                                    cx,
                                                    (
                                                        event.position.x.as_f32(),
                                                        event.position.y.as_f32(),
                                                    ),
                                                    zb,
                                                );
                                            })
                                            .on_mouse_up(
                                                gpui_kit::MouseButton::Left,
                                                move |_, _, cx| finish_clip_drag(cx),
                                            )
                                            // Zoom con rueda + modificadores, anclado al cursor:
                                            // `Ctrl` + rueda = horizontal, `Alt` + rueda (o
                                            // `Ctrl` + `Shift` + rueda) = altura de filas. Sin
                                            // modificadores se deja pasar (scroll normal).
                                            .on_scroll_wheel(move |event, _, cx| {
                                                let control =
                                                    event.modifiers.control || event.modifiers.platform;
                                                let alt = event.modifiers.alt;
                                                let shift = event.modifiers.shift;
                                                let cursor = (
                                                    event.position.x.as_f32(),
                                                    event.position.y.as_f32(),
                                                );
                                                let delta = event.delta;
                                                let st = state(cx);
                                                let mut consumed = false;
                                                st.update(cx, |s, cx| {
                                                    consumed = apply_playlist_wheel_zoom(
                                                        s,
                                                        &delta,
                                                        control,
                                                        alt,
                                                        shift,
                                                        cursor,
                                                        total_ticks,
                                                        header_width,
                                                        ruler_h,
                                                        row_count,
                                                    );
                                                    if consumed {
                                                        cx.notify();
                                                    }
                                                });
                                                if consumed {
                                                    cx.stop_propagation();
                                                }
                                            })
                                    // Columna fija de headers: apilada en vertical desde la
                                    // regla, una cabecera de `track_height` por fila — misma
                                    // altura y mismo `top` que su fila de la grilla.
                                    .child(
                                        v_flex()
                                            .absolute()
                                            .left(px(0.0))
                                            .top(px(ruler_h))
                                            .children(track_headers),
                                    )
                                    .children(children)
                                    // Orden de capas (z-ordering):
                                    //   1. Fondo + líneas verticales de grilla (detrás).
                                    //   2. Clips de audio/MIDI con fondo opaco (tapan la grilla).
                                    //   3. Overlays de drop/seek + playhead (encima de todo).
                                    // Antes `grid_lines` iba DESPUÉS de `clip_elems` y las
                                    // líneas atravesaban la forma de onda del clip.
                                    .children(grid_lines)
                                    .children(clip_elems)
                                    .children(drop_zones)
                                    .when_some(drop_handler, |v, h| v.child(h))
                                    .child(seek_zone.into_any_element())
                                    .child(playhead_canvas.into_any_element()),
                                )
                            )
                        )
                )
            .into_any_element(),
    );
    all.push(
        h_flex()
            .gap(px(4.0))
            .child(
                Button::new("pl_add_track").rounded(gpui_kit::component::button::ButtonRounded::None)
                    .label("[ + ]")
                    .compact()
                    .on_click(move |_, _, cx| {
                        let st = state(cx);
                        st.update(cx, |state, cx| {
                            // Las filas de la playlist pertenecen al set de
                            // pistas del modo activo: nunca se toca el otro.
                            let tracks = state.tracks_mut();
                            let next_id =
                                tracks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
                            let n = tracks.iter().filter(|t| !t.is_master).count();
                            tracks.push(Track::new(
                                next_id,
                                format!("TRACK {:02}", n + 1),
                                false,
                            ));
                            cx.notify();
                        });
                    }),
            )
            .child(
                Button::new("pl_remove_track").rounded(gpui_kit::component::button::ButtonRounded::None)
                    .label("[ - ]")
                    .compact()
                    .on_click(move |_, _, cx| {
                        let st = state(cx);
                        st.update(cx, |state, cx| {
                            let tracks = state.tracks_mut();
                            if let Some(pos) =
                                tracks.iter().rposition(|t| !t.is_master)
                            {
                                tracks.remove(pos);
                            }
                            cx.notify();
                        });
                    }),
            )
            .child(Label::new(format!("PPQN {}", ppqn)).text_xs())
            .child(Label::new(format!("{:.2} bars", playhead_tick as f64 / ticks_per_bar_val.max(1) as f64)).text_xs())
            .child(Label::new(format!("Bar {} | Tick {}", bar_number_at_tick(playhead_tick, ticks_per_bar_val), playhead_tick)).text_xs())
            .into_any_element(),
    );

    v_flex()
        .id("playlist")
        .test_support()
        .size_full()
        .gap(px(4.0))
        .children(all)
        .into_any_element()
}
