/*
 * Hikaru OpenStudio - Playlist / Timeline View
 * License: AGPL-3.0-later
 */

use egui::{
    Ui, RichText, Color32, ScrollArea, Frame, Stroke, Vec2, Rect, Pos2,
    Sense, Align2, PointerButton, CursorIcon
};
use std::f32::consts::PI;
use std::path::PathBuf;
use crate::views::mixer::Track;
use crate::audio_proxy::{AudioProxy, GuiCommand};
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
        sample_offset_ticks: u64, // Slip editing / Trim inicio en ticks
        total_sample_ticks: u64,  // Duración total original del audio en ticks
    },
    Automation { 
        points: Vec<CurvePoint>,
        target_param: String, 
    },
}

#[derive(Clone, Debug)]
pub struct PlaylistClip {
    pub id: usize,
    pub name: String,
    pub start_tick: u64,
    pub duration_ticks: u64,
    pub clip_type: ClipType,
    pub color: Color32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopDragHandle {
    None,
    Left,
    Right,
    Body,
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
    pub selected_clips: Vec<usize>,
    pub clipboard: Vec<(usize, PlaylistClip)>,
    pub needs_full_sync: bool, // Bandera para resincronización maestra
    /// Inicio de la región de loop en ticks.
    pub loop_start_ticks: u64,
    /// Fin de la región de loop en ticks.
    pub loop_end_ticks: u64,
    /// Si hay una selección de loop activa.
    pub loop_region_active: bool,
    /// Estado temporal para dibujar la región mientras se arrastra.
    pub loop_dragging: bool,
    /// Región visual (translúcida) mientras dura el Shift+Drag.
    /// PATRÓN MATRIX: durante `.dragged()` SOLO se toca este estado
    /// visual; `loop_start_ticks` / `loop_end_ticks` (los que lee el
    /// transporte global / engine) SOLO se confirman en `drag_stopped()`.
    /// Así se evita notificar al engine en cada frame (congelamiento).
    pub loop_preview_start_ticks: u64,
    pub loop_preview_end_ticks: u64,
    pub loop_preview_active: bool,
    /// Drag handle activo (Left, Right, o Body para mover todo).
    pub loop_drag_handle: LoopDragHandle,
    /// Se activa en el frame en que termina un drag de loop para evitar limpiar selección.
    /// El consumidor (matrix.rs / transporte global) propaga al engine
    /// ÚNICAMENTE cuando esta bandera está activa (`drag_stopped`).
    pub loop_drag_completed_this_frame: bool,
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
        }
    }
}

impl PlaylistState {
    /// Longitud de la región de loop en ticks (0 si es inválida).
    pub fn loop_length_ticks(&self) -> u64 {
        self.loop_end_ticks.saturating_sub(self.loop_start_ticks)
    }

    /// Indica si la región de loop es válida para enviar al transporte.
    /// Guarda GUI: `end > start` (el saneado previo garantiza además
    /// `len >= ppqn`, 1 beat).
    pub fn is_loop_region_valid(&self) -> bool {
        self.loop_region_active && self.loop_end_ticks > self.loop_start_ticks
    }

    /// Limpia completamente la región de loop (time selection).
    /// Resetea todos los campos relacionados y marca
    /// `loop_drag_completed_this_frame = true` para que el consumidor
    /// (app.rs / matrix.rs) propague la desactivación al engine.
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

    /// Final del último clip en la Playlist en ticks (0 si no hay clips).
    /// Estilo REAPER: el loop por defecto abarca hasta el final del proyecto.
    pub fn total_project_ticks(&self) -> u64 {
        self.clips
            .iter()
            .map(|(_, c)| c.start_tick.saturating_add(c.duration_ticks))
            .max()
            .unwrap_or(0)
    }

    /// Guarda GUI previa al envío al transporte (anti-congelamiento).
    /// Antes de `transport.set_loop_region(start, end)` /
    /// `set_loop_enabled(true)`:
    /// - si `end <= start`, fuerza `end = start + (4 * ppqn)`;
    /// - si `end - start < ppqn`, fuerza `end = start + ppqn`.
    /// Solo con `end > start` se considera válida para enviar.
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

    /// Guarda de auto-selección estilo REAPER para el transporte GLOBAL.
    /// Debe llamarse ANTES de notificar al transporte
    /// (`transport.set_loop_region(start, end)` +
    /// `transport.set_loop_enabled(true)`).
    /// Guarda GUI previa al envío:
    /// - si `end <= start`, `end = start + (4 * ppqn)` (o `0..fin_proyecto`
    ///   con mínimo de 1 compás si no hay selección válida);
    /// - si `end - start < ppqn`, `end = start + ppqn`;
    /// - SOLO se confirma si `end > start`.
    /// Solo debe llamarse en modo Arranger; no toca el estado visual de drag
    /// mientras hay un arrastre en curso.
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
            // REAPER: sin selección válida -> todo el proyecto desde el tick 0.
            let total = self.total_project_ticks();
            let start = 0u64;
            // Guarda previa al envío: mínimo 4 beats si no hay clips.
            let mut end = total.max(4u64.saturating_mul(ppqn));
            // Verifica el rango en ticks: si end <= start, end = start + 4*ppqn.
            if end <= start {
                end = start.saturating_add(4u64.saturating_mul(ppqn));
            }
            // Si end - start < ppqn, fuerza end = start + ppqn.
            if end.saturating_sub(start) < ppqn {
                end = start.saturating_add(ppqn);
            }
            // SOLO confirma si la región es válida (end > start).
            if end > start {
                self.loop_start_ticks = start;
                self.loop_end_ticks = end;
                self.loop_preview_start_ticks = start;
                self.loop_preview_end_ticks = end;
                self.loop_region_active = true;
                self.loop_drag_completed_this_frame = true;
            }
        } else {
            // Región ya existente: aplicar la misma guarda por seguridad.
            self.sanitize_loop_region();
        }
    }

    /// Resincroniza todos los clips del timeline con el motor de audio de forma atómica.
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

#[inline]
fn px_to_ticks(px: f32, zoom_x: f32) -> u64 {
    (px.max(0.0) / zoom_x) as u64
}

#[inline]
fn ticks_to_px(ticks: u64, zoom_x: f32) -> f32 {
    ticks as f32 * zoom_x
}

/// Cuantiza `ticks` a la rejilla activa (`grid_ticks`).
/// Redondeo al múltiplo más cercano: `round(ticks / grid) * grid`.
/// Si `grid_ticks == 0` devuelve `ticks` sin modificar (snap desactivado).
#[inline]
pub fn snap_ticks(ticks: u64, grid_ticks: u64) -> u64 {
    if grid_ticks == 0 {
        return ticks;
    }
    ((ticks as f64 / grid_ticks as f64).round() as u64).saturating_mul(grid_ticks)
}

/// Mapeo exacto Ticks -> Píxeles con el PPQN único del motor.
///
/// Fórmula canónica:
/// `pixel_x = (ticks as f32 / ppqn as f32) * pixels_per_beat + playlist_offset_x`
/// donde `pixels_per_beat = ppqn as f32 * zoom_x`, por lo que equivale a
/// `ticks * zoom_x + offset`. Se mantiene explícita para auditar que el
/// PPQN usado aquí sea siempre `DEFAULT_PPQN` / `transport.ppqn()`.
#[inline]
fn ticks_to_pixel_x(ticks: u64, ppqn: u64, zoom_x: f32, playlist_offset_x: f32) -> f32 {
    let ppqn = ppqn.max(1) as f32;
    let pixels_per_beat = ppqn * zoom_x;
    (ticks as f32 / ppqn) * pixels_per_beat + playlist_offset_x
}

/// Mapeo visual del playhead dentro del Time Selection cuando el loop
/// está activo (OpenLive).
///
/// Si `loop_enabled` está activo y la región `[loop_start, loop_end)` es
/// válida, mapea `current_tick` estrictamente dentro del rango para que la
/// coordenada X nunca se dibuje fuera de los corchetes `[` y `]`.
///
/// Si `current_tick` excede `loop_end` por delay de renderizado de frame,
/// aplica wrap visual:
/// `display = loop_start + ((current - loop_start) % len)`.
/// Si `current < loop_start` se devuelve sin modificar (el playhead aún no
/// entró al loop: es válido mostrarlo fuera, antes del `[`).
#[inline]
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

/// Samples -> ticks con el BPM activo y el Sample Rate real del motor.
/// Inversa exacta de `ticks_to_samples_precise`.
#[inline]
fn samples_to_ticks_precise(sample_count: u64, ppqn: u64, bpm: f64, sample_rate: u32) -> u64 {
    let ppqn = ppqn.max(1);
    if bpm <= 0.0 || sample_rate == 0 {
        return 0;
    }
    let seconds = sample_count as f64 / sample_rate as f64;
    let seconds_per_tick = (60.0 / bpm) / ppqn as f64;
    (seconds / seconds_per_tick).round() as u64
}

/// Ticks -> samples con el BPM activo y el Sample Rate real del motor.
#[inline]
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
    if ppqn == 0 || bpm <= 0.0 { return 0.0; }
    let seconds_per_tick = (60.0 / bpm) / (ppqn as f64);
    (ticks as f64 * seconds_per_tick) as f32
}

pub fn knob_ui(ui: &mut Ui, value: &mut f32, radius: f32) -> egui::Response {
    let desired_size = Vec2::splat(radius * 2.0);
    let (rect, response) = ui.allocate_at_least(desired_size, Sense::click_and_drag());

    if response.dragged() {
        let delta = response.drag_delta();
        *value += (delta.x - delta.y) * 0.8; 
        *value = value.clamp(-100.0, 100.0);
    }

    if response.double_clicked() {
        *value = 0.0;
    }

    if ui.is_rect_visible(rect) {
        let center = rect.center();
        let painter = ui.painter();

        painter.circle_filled(center, radius, Color32::from_rgb(35, 35, 42));
        painter.circle_stroke(center, radius, Stroke::new(1.5_f32, Color32::from_gray(80)));

        let norm_val = (*value + 100.0) / 200.0;
        let angle = PI * 1.25 - norm_val * (PI * 1.5);

        let indicator_len = radius - 2.5;
        let line_end = Pos2::new(
            center.x + angle.cos() * indicator_len,
            center.y - angle.sin() * indicator_len,
        );

        painter.line_segment([center, line_end], Stroke::new(2.0_f32, Color32::from_rgb(0, 255, 255)));
    }

    response
}

pub fn custom_h_slider(ui: &mut Ui, value: &mut f32, width: f32) -> egui::Response {
    let height = 14.0;
    let desired_size = Vec2::new(width, height);
    let (rect, response) = ui.allocate_at_least(desired_size, Sense::click_and_drag());

    if response.double_clicked() {
        *value = 0.75;
    } else if response.dragged() {
        let delta_x = response.drag_delta().x;
        *value = (*value + delta_x / rect.width()).clamp(0.0, 1.0);
    } else if response.clicked() {
        if let Some(mouse_pos) = response.interact_pointer_pos() {
            *value = ((mouse_pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
        }
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();

        let track_rect = Rect::from_center_size(rect.center(), Vec2::new(rect.width(), 4.0));
        painter.rect_filled(track_rect, 2.0, Color32::from_rgb(20, 20, 25));

        let fill_w = rect.width() * (*value).clamp(0.0, 1.0);
        let fill_rect = Rect::from_min_size(track_rect.min, Vec2::new(fill_w, 4.0));
        painter.rect_filled(fill_rect, 2.0, Color32::from_rgb(0, 200, 220));

        let handle_x = rect.min.x + fill_w;
        let handle_rect = Rect::from_center_size(Pos2::new(handle_x, rect.center().y), Vec2::new(8.0, 12.0));
        painter.rect_filled(handle_rect, 1.0, Color32::from_gray(180));
        painter.rect_stroke(handle_rect, 1.0, Stroke::new(1.0_f32, Color32::BLACK));
    }

    response
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
                    let max_peak = chunk.iter().cloned().fold(0.0_f32, f32::max);
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
    color: Color32,
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

pub fn show(
    ui: &mut Ui,
    state: &mut PlaylistState,
    tracks: &mut Vec<Track>,
    current_bar: &mut f32,
    dragged_sample: &mut Option<PathBuf>,
    audio_proxy: &AudioProxy,
    bpm: f64,
    sample_rate: u32,
    loop_enabled: bool,
    transport_sample_count: u64,
    beats_per_bar: u32,
) {
    show_impl(
        ui, state, tracks, current_bar, dragged_sample, audio_proxy, bpm, sample_rate,
        ViewMode::Arranger, loop_enabled, transport_sample_count, beats_per_bar,
        // Arranger: sin override, la aguja es el cursor global.
        None,
    );
}

pub fn show_embedded(
    ui: &mut Ui,
    state: &mut PlaylistState,
    tracks: &mut Vec<Track>,
    local_bar: &mut f32,
    dragged_sample: &mut Option<PathBuf>,
    audio_proxy: &AudioProxy,
    bpm: f64,
    sample_rate: u32,
    title: &str,
    transport_sample_count: u64,
    beats_per_bar: u32,
    // Override del playhead en ticks para el ClipEditor OpenLive: cuando
    // hay voz activa, la aguja deriva del elapsed LINEAL de la voz (0 al
    // disparar → línea del compás 1), no del cursor global. `None` =
    // comportamiento clásico (cursor global).
    playhead_override_ticks: Option<u64>,
) {
    // En modo OPENLIVE / ClipEditor el Shift+Drag sobre la regla define el
    // loop individual del clip (`clip.loop_start` / `clip.loop_end` via
    // `state.loop_start_ticks` / `state.loop_end_ticks`), independiente del
    // transporte global. Se habilita siempre (true).
    show_impl(
        ui, state, tracks, local_bar, dragged_sample, audio_proxy, bpm, sample_rate,
        ViewMode::ClipEditor { title }, true, transport_sample_count, beats_per_bar,
        playhead_override_ticks,
    );
}

fn show_impl(
    ui: &mut Ui,
    state: &mut PlaylistState,
    tracks: &mut Vec<Track>,
    current_bar: &mut f32,
    dragged_sample: &mut Option<PathBuf>,
    audio_proxy: &AudioProxy,
    bpm: f64,
    sample_rate: u32,
    mode: ViewMode,
    loop_enabled: bool,
    transport_sample_count: u64,
    beats_per_bar: u32,
    playhead_override_ticks: Option<u64>,
) {
    // Sincronización PPQN: la GUI no hardcodea 960; la única fuente de
    // verdad es `DEFAULT_PPQN` (= `transport.ppqn()`). Si el estado trae
    // otro valor (snapshot viejo), se corrige aquí cada frame.
    if state.ppqn != DEFAULT_PPQN {
        state.ppqn = DEFAULT_PPQN;
    }
    // Sincronización completa automática si es requerida al arrancar o cambiar vista
    if mode.drives_engine() && state.needs_full_sync {
        state.sync_all_clips_to_engine(audio_proxy, bpm);
    }

    // Playhead exacto: samples -> ticks con BPM activo + SR real + PPQN único.
    // NO se deriva de `current_bar: f32` (pierde precisión y corre más rápido
    // que el audio); el bar solo se deriva para compatibilidad visual.
    // En ClipEditor con voz activa, el override manda: elapsed lineal de la
    // voz (0 al disparar → tick 0 → aguja EXACTA sobre la línea del compás 1
    // cuando el transporte resetea a bar 1, frame 0). Sin override, cursor.
    let ppqn = state.ppqn.max(1);
    let beats_per_bar = beats_per_bar.max(1);
    let ticks_per_bar = ppqn * beats_per_bar as u64;
    let bpm_safe = bpm.max(1.0);
    state.playhead_tick = match playhead_override_ticks {
        Some(t) => t,
        None => samples_to_ticks_precise(transport_sample_count, ppqn, bpm_safe, sample_rate),
    };
    // Bar visual coherente con el mismo origen (f64, sin pasar por f32).
    let bar_from_transport =
        (state.playhead_tick as f64 / ticks_per_bar as f64) + 1.0;
    *current_bar = bar_from_transport as f32;

    let _samples_per_beat = (sample_rate as f64 * 60.0) / bpm_safe;
    let _samples_per_bar = _samples_per_beat * beats_per_bar as f64;

    let zoom_x = state.zoom_x;
    let track_height = 54.0_f32;
    let row_spacing = 0.0_f32;
    let total_row_step = track_height + row_spacing;
    let header_width = state.header_width;

    let ctrl = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
    let shift = ui.input(|i| i.modifiers.shift);

    state.loop_drag_completed_this_frame = false;

    // Guarda de distancia mínima para el transporte GLOBAL (Playlist /
    // Arranger): si el botón de loop está ENCENDIDO pero la región
    // confirmada es degenerada (`loop_start == loop_end`), forzar al menos
    // 1 compás (1 beat/bar) para que `loop_end` nunca quede pegado a
    // `loop_start`. No se aplica durante el drag ni en ClipEditor
    // (el loop individual lo gestiona matrix.rs al hacer `drag_stopped`).
    if mode.drives_engine() {
        state.ensure_minimum_global_loop(loop_enabled);
    }

    // `ppqn` ya sincronizado arriba con DEFAULT_PPQN (fuente única).
    let ppqn = state.ppqn.max(1);

    // Portapapeles y atajos
    if ctrl && ui.input(|i| i.key_pressed(egui::Key::C)) {
        state.clipboard = state.clips.iter()
            .filter(|(_, c)| state.selected_clips.contains(&c.id))
            .cloned()
            .collect();
    }

    if ctrl && ui.input(|i| i.key_pressed(egui::Key::X)) {
        state.clipboard = state.clips.iter()
            .filter(|(_, c)| state.selected_clips.contains(&c.id))
            .cloned()
            .collect();
        state.clips.retain(|(_, c)| !state.selected_clips.contains(&c.id));
        state.selected_clips.clear();
    }

    if ctrl && ui.input(|i| i.key_pressed(egui::Key::V)) {
        if !state.clipboard.is_empty() {
            let min_start = state.clipboard.iter().map(|(_, c)| c.start_tick).min().unwrap_or(0);
            let mut new_selection = Vec::new();

            for (track_id, clip) in state.clipboard.clone() {
                let offset = clip.start_tick.saturating_sub(min_start);
                let mut new_clip = clip.clone();
                new_clip.id = state.next_clip_id;
                new_clip.start_tick = state.playhead_tick + offset;

                if mode.drives_engine() {
                    if let ClipType::Audio { ref sample_path, sample_offset_ticks, .. } = new_clip.clip_type {
                        audio_proxy.send(GuiCommand::LoadClip {
                            clip_id: new_clip.id,
                            path: sample_path.clone(),
                            position_secs: ticks_to_secs_precise(new_clip.start_tick, ppqn, bpm),
                            duration_secs: ticks_to_secs_precise(new_clip.duration_ticks, ppqn, bpm),
                            offset_secs: ticks_to_secs_precise(sample_offset_ticks, ppqn, bpm),
                            track_index: track_id,
                            scene_index: 0,
                        });
                    }
                }

                new_selection.push(new_clip.id);
                state.clips.push((track_id, new_clip));
                state.next_clip_id += 1;
            }
            state.selected_clips = new_selection;
        }
    }

    if ctrl && ui.input(|i| i.key_pressed(egui::Key::D)) {
        let selected_items: Vec<(usize, PlaylistClip)> = state.clips.iter()
            .filter(|(_, c)| state.selected_clips.contains(&c.id))
            .cloned()
            .collect();

        if !selected_items.is_empty() {
            let min_start = selected_items.iter().map(|(_, c)| c.start_tick).min().unwrap_or(0);
            let max_end = selected_items.iter().map(|(_, c)| c.start_tick + c.duration_ticks).max().unwrap_or(0);
            let duration_block = max_end - min_start;
            let mut new_selection = Vec::new();

            for (track_id, clip) in selected_items {
                let mut new_clip = clip.clone();
                new_clip.id = state.next_clip_id;
                new_clip.start_tick = clip.start_tick + duration_block;

                if mode.drives_engine() {
                    if let ClipType::Audio { ref sample_path, sample_offset_ticks, .. } = new_clip.clip_type {
                        audio_proxy.send(GuiCommand::LoadClip {
                            clip_id: new_clip.id,
                            path: sample_path.clone(),
                            position_secs: ticks_to_secs_precise(new_clip.start_tick, ppqn, bpm),
                            duration_secs: ticks_to_secs_precise(new_clip.duration_ticks, ppqn, bpm),
                            offset_secs: ticks_to_secs_precise(sample_offset_ticks, ppqn, bpm),
                            track_index: track_id,
                            scene_index: 0,
                        });
                    }
                }

                new_selection.push(new_clip.id);
                state.clips.push((track_id, new_clip));
                state.next_clip_id += 1;
            }
            state.selected_clips = new_selection;
        }
    }

    if ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
        state.clips.retain(|(_, c)| !state.selected_clips.contains(&c.id));
        state.selected_clips.clear();
    }

    ui.vertical(|ui| {
        let non_master_count = tracks.iter().filter(|t| !t.is_master).count();

        ui.horizontal(|ui| {
            ui.label(RichText::new(mode.header_label()).strong().color(Color32::from_rgb(0, 255, 255)));
            ui.separator();
            
            ui.label(RichText::new("Grid:").size(11.0).color(Color32::from_gray(180)));
            
            ui.add(egui::DragValue::new(&mut state.grid_numerator)
                .clamp_range(1..=32)
                .speed(0.1));

            ui.label(RichText::new("/").size(11.0).color(Color32::from_gray(180)));

            egui::ComboBox::from_id_source("grid_denom_combo")
                .selected_text(format!("{}", state.grid_denominator))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut state.grid_denominator, 2, "2");
                    ui.selectable_value(&mut state.grid_denominator, 4, "4");
                    ui.selectable_value(&mut state.grid_denominator, 8, "8");
                    ui.selectable_value(&mut state.grid_denominator, 16, "16");
                });
        });

        ScrollArea::vertical()
            .id_source("playlist_main_scroll")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(header_width);

                        ui.allocate_ui_with_layout(
                            Vec2::new(header_width, 24.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.label(RichText::new("TRACKS").size(10.0).strong().color(Color32::from_gray(140)));
                                if mode.allows_track_add_remove() {
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.button(RichText::new("[ - ]").strong().color(Color32::from_gray(160))).clicked() {
                                            if non_master_count > 1 {
                                                if let Some(pos) = tracks.iter().rposition(|t| !t.is_master) {
                                                    tracks.remove(pos);
                                                }
                                            }
                                        }
                                        if ui.button(RichText::new("[ + ]").strong().color(Color32::from_rgb(255, 140, 0))).clicked() {
                                            let next_id = tracks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
                                            let track_num = non_master_count + 1;
                                            let new_track = Track::new(next_id, format!("TRACK {:02}", track_num), false);
                                            tracks.push(new_track);
                                        }
                                    });
                                }
                            }
                        );

                        for track in tracks.iter_mut().filter(|t| !t.is_master) {
                            let (rect, _) = ui.allocate_exact_size(
                                Vec2::new(header_width, total_row_step), 
                                Sense::hover()
                            );

                            ui.allocate_ui_at_rect(rect, |ui| {
                                Frame::none()
                                    .fill(Color32::from_rgb(28, 28, 32))
                                    .stroke(Stroke::new(1.0_f32, Color32::from_gray(45)))
                                    .inner_margin(0.0)
                                    .outer_margin(0.0)
                                    .show(ui, |ui| {
                                        ui.set_min_size(Vec2::new(rect.width() - 8.0, track_height));
                                        ui.vertical(|ui| {
                                            ui.horizontal(|ui| {
                                                let text_width = (header_width - 70.0).max(40.0);
                                                ui.add(
                                                    egui::TextEdit::singleline(&mut track.name)
                                                        .text_color(Color32::WHITE)
                                                        .font(egui::FontId::proportional(11.0))
                                                        .frame(false)
                                                        .desired_width(text_width)
                                                );

                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                    ui.toggle_value(&mut track.solo, "S");
                                                    ui.toggle_value(&mut track.mute, "M");
                                                });
                                            });

                                            ui.add_space(2.0);

                                            ui.horizontal(|ui| {
                                                knob_ui(ui, &mut track.pan, 8.0);

                                                let pan_text = if track.pan < -1.0 {
                                                    format!("L{:.0}", track.pan.abs())
                                                } else if track.pan > 1.0 {
                                                    format!("R{:.0}", track.pan)
                                                } else {
                                                    "C".to_string()
                                                };
                                                ui.label(RichText::new(pan_text).size(9.0).color(Color32::from_rgb(0, 255, 255)));

                                                ui.add_space(2.0);

                                                let slider_width = (header_width - 90.0).max(30.0);
                                                custom_h_slider(ui, &mut track.volume, slider_width);

                                                let db_val = if track.volume <= 0.0 {
                                                    -60.0
                                                } else if track.volume <= 0.75 {
                                                    -60.0 + (track.volume / 0.75) * 60.0
                                                } else {
                                                    ((track.volume - 0.75) / 0.25) * 6.0
                                                };
                                                ui.label(RichText::new(format!("{:.1}dB", db_val)).size(8.5).weak());
                                            });
                                        });
                                    });
                            });
                        }
                    });

                    let total_height = 24.0 + (non_master_count as f32 * total_row_step);
                    let (resizer_rect, resizer_response) = ui.allocate_at_least(
                        Vec2::new(6.0, total_height), 
                        Sense::click_and_drag()
                    );

                    if resizer_response.hovered() || resizer_response.dragged() {
                        ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeHorizontal);
                    }

                    if resizer_response.dragged() {
                        let delta = resizer_response.drag_delta().x;
                        state.header_width = (state.header_width + delta).clamp(120.0, 1331.0);
                    }

                    if ui.is_rect_visible(resizer_rect) {
                        let line_x = resizer_rect.center().x;
                        let line_color = if resizer_response.dragged() || resizer_response.hovered() {
                            Color32::from_rgb(0, 255, 255)
                        } else {
                            Color32::from_gray(50)
                        };
                        ui.painter().line_segment(
                            [Pos2::new(line_x, resizer_rect.min.y), Pos2::new(line_x, resizer_rect.max.y)],
                            Stroke::new(2.0_f32, line_color)
                        );
                    }

                    ScrollArea::horizontal()
                        .id_source("timeline_horizontal_scroll")
                        .show(ui, |ui| {
                            let available_w = ui.available_width();
                            let min_canvas_w = ticks_to_px(state.ppqn * 4 * 128, zoom_x); 
                            let canvas_width = available_w.max(min_canvas_w);
                            let total_tracks_height = non_master_count as f32 * total_row_step;
                            let canvas_height: f32 = 24.0 + total_tracks_height;

                            let (response, painter) = ui.allocate_painter(
                                Vec2::new(canvas_width, canvas_height), 
                                Sense::click_and_drag()
                            );

                            if response.clicked_by(PointerButton::Primary) && !shift && !state.loop_drag_completed_this_frame {
                                state.selected_clips.clear();
                            }

                            if response.hovered() {
                                let ctrl_pressed = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
                                if ctrl_pressed {
                                    let mut zoom_delta = 0.0_f32;
                                    ui.input(|i| {
                                        for event in &i.events {
                                            if let egui::Event::MouseWheel { delta, .. } = event {
                                                zoom_delta += delta.y;
                                            }
                                        }
                                    });

                                    if zoom_delta != 0.0 {
                                        let zoom_factor = if zoom_delta > 0.0 { 1.15 } else { 0.85 };
                                        state.zoom_x = (state.zoom_x * zoom_factor).clamp(0.005, 0.5);
                                    }
                                }
                            }

                            let rect = response.rect;
                            painter.rect_filled(rect, 0.0, Color32::from_rgb(16, 16, 20));

                            let slice_key_pressed = ui.input(|i| i.key_pressed(egui::Key::S));
                            let slice_hover_pos = response.interact_pointer_pos()
                                .or_else(|| ui.input(|i| i.pointer.hover_pos()));
                            let mut pending_slices: Vec<(usize, PlaylistClip)> = Vec::new();

                            let ruler_rect = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 24.0));
                            painter.rect_filled(ruler_rect, 0.0, Color32::from_rgb(24, 24, 30));
                            painter.line_segment(
                                [ruler_rect.left_bottom(), ruler_rect.right_bottom()], 
                                Stroke::new(1.0_f32, Color32::from_gray(60))
                            );

                            let ruler_id = ui.make_persistent_id("timeline_ruler");
                            let ruler_response = ui.interact(ruler_rect, ruler_id, Sense::click_and_drag());

                            let snap_step_ticks = (state.ppqn * 4 * state.grid_numerator as u64) / state.grid_denominator as u64;
                            let step_width = ticks_to_px(snap_step_ticks, zoom_x);
                            let total_grid_steps = (canvas_width / step_width.max(1.0)).ceil() as usize;
                            let steps_per_bar = (state.grid_denominator / state.grid_numerator).max(1) as usize;

                            for step in 0..total_grid_steps {
                                let x = rect.min.x + (step as f32 * step_width);
                                let is_main_bar = step % steps_per_bar == 0;

                                let line_color = if is_main_bar {
                                    Color32::from_gray(50)
                                } else {
                                    Color32::from_gray(28)
                                };

                                painter.line_segment(
                                    [Pos2::new(x, rect.min.y + 24.0), Pos2::new(x, rect.max.y)],
                                    Stroke::new(1.0_f32, line_color)
                                );

                                if is_main_bar {
                                    let bar_num = (step / steps_per_bar) + 1;
                                    painter.text(
                                        Pos2::new(x + 4.0, rect.min.y + 4.0),
                                        Align2::LEFT_TOP,
                                        format!("{}", bar_num),
                                        egui::FontId::monospace(10.0),
                                        Color32::from_gray(180)
                                    );
                                }
                            }

                            // --- LOOP REGION RENDERING & INTERACTION ---
                            // Shift + Drag sobre la regla define la región de loop.
                            // En Arranger solo cuando el botón de loop del transporte
                            // está ENCENDIDO; en ClipEditor (OPENLIVE) siempre, para
                            // el loop individual del clip.
                            // Alt + Click o Alt + Drag sobre la regla limpia
                            // (elimina) la selección de loop activa.
                            // PATRÓN MATRIX (anti-congelamiento): durante `.dragged()`
                            // SOLO se actualiza el rectángulo translúcido visual
                            // (`loop_preview_*`), SIN tocar `loop_start_ticks` /
                            // `loop_end_ticks` ni notificar al engine. La región
                            // confirmada SOLO se escribe en `drag_stopped()` y el
                            // consumidor propaga al motor vía
                            // `loop_drag_completed_this_frame`.
                            {
                                let shift_pressed = ui.input(|i| i.modifiers.shift);
                                let alt_pressed = ui.input(|i| i.modifiers.alt);
                                let allow_loop_select = loop_enabled
                                    || matches!(mode, ViewMode::ClipEditor { .. });

                                // --- ALT + CLICK/DRAG: LIMPIAR TIME SELECTION ---
                                // Si Alt está presionado y hay una región de loop
                                // activa, limpiar inmediatamente al hacer click
                                // o arrastrar sobre la regla.
                                if alt_pressed
                                    && (ruler_response.clicked() || ruler_response.dragged())
                                    && state.loop_region_active
                                {
                                    state.clear_loop_region();
                                }

                                // --- RESIZE DE EXTREMOS ESTILO REAPER ---
                                // Hitboxes de ~6px en cada borde para arrastrar
                                // `[` (loop_start) y `]` (loop_end) de forma
                                // independiente. Se crean ANTES del Shift+Drag
                                // para poder darle prioridad al resize.
                                // PATRÓN MATRIX: durante `.dragged()` solo se
                                // toca el preview visual; la confirmación al
                                // transporte es SOLO en `drag_stopped()`.
                                let bracket_hit_w = 12.0_f32; // ~±6px
                                let mut left_bracket_resp: Option<egui::Response> = None;
                                let mut right_bracket_resp: Option<egui::Response> = None;
                                // Base para hitboxes: región confirmada (en
                                // reposo) o preview si ya hay resize en curso.
                                let bracket_base = if state.loop_dragging
                                    && state.loop_preview_active
                                    && (state.loop_drag_handle == LoopDragHandle::Left
                                        || state.loop_drag_handle == LoopDragHandle::Right)
                                {
                                    if state.loop_preview_end_ticks > state.loop_preview_start_ticks {
                                        Some((
                                            state.loop_preview_start_ticks,
                                            state.loop_preview_end_ticks,
                                        ))
                                    } else {
                                        None
                                    }
                                } else if state.loop_region_active
                                    && state.loop_end_ticks > state.loop_start_ticks
                                {
                                    Some((state.loop_start_ticks, state.loop_end_ticks))
                                } else {
                                    None
                                };
                                if let Some((b_start, b_end)) = bracket_base {
                                    if !shift_pressed {
                                        let start_x =
                                            rect.min.x + ticks_to_px(b_start, zoom_x);
                                        let end_x =
                                            rect.min.x + ticks_to_px(b_end, zoom_x);
                                        let (min_x, max_x) = if start_x <= end_x {
                                            (start_x, end_x)
                                        } else {
                                            (end_x, start_x)
                                        };
                                        let ruler_center_y = ruler_rect.center().y;
                                        let left_hit = Rect::from_center_size(
                                            Pos2::new(min_x, ruler_center_y),
                                            Vec2::new(bracket_hit_w, ruler_rect.height()),
                                        );
                                        let right_hit = Rect::from_center_size(
                                            Pos2::new(max_x, ruler_center_y),
                                            Vec2::new(bracket_hit_w, ruler_rect.height()),
                                        );
                                        let left_id = ui.make_persistent_id("loop_bracket_left");
                                        let right_id =
                                            ui.make_persistent_id("loop_bracket_right");
                                        let l_resp =
                                            ui.interact(left_hit, left_id, Sense::drag());
                                        let r_resp =
                                            ui.interact(right_hit, right_id, Sense::drag());
                                        if l_resp.hovered() || l_resp.dragged() {
                                            ui.output_mut(|o| {
                                                o.cursor_icon = CursorIcon::ResizeHorizontal
                                            });
                                        }
                                        if r_resp.hovered() || r_resp.dragged() {
                                            ui.output_mut(|o| {
                                                o.cursor_icon = CursorIcon::ResizeHorizontal
                                            });
                                        }
                                        // Iniciar resize: sembrar preview desde
                                        // la región confirmada.
                                        if l_resp.drag_started() {
                                            state.loop_preview_start_ticks =
                                                state.loop_start_ticks;
                                            state.loop_preview_end_ticks =
                                                state.loop_end_ticks;
                                            state.loop_preview_active = true;
                                            state.loop_dragging = true;
                                            state.loop_drag_handle = LoopDragHandle::Left;
                                        }
                                        if r_resp.drag_started() {
                                            state.loop_preview_start_ticks =
                                                state.loop_start_ticks;
                                            state.loop_preview_end_ticks =
                                                state.loop_end_ticks;
                                            state.loop_preview_active = true;
                                            state.loop_dragging = true;
                                            state.loop_drag_handle = LoopDragHandle::Right;
                                        }
                                        // Arrastrar `[`: actualiza start
                                        // manteniendo `start < end`.
                                        // Snap to Grid: cuantiza a `snap_step_ticks`.
                                        if l_resp.dragged()
                                            && state.loop_drag_handle == LoopDragHandle::Left
                                        {
                                            if let Some(pointer_pos) =
                                                l_resp.interact_pointer_pos()
                                            {
                                                let rel_x =
                                                    (pointer_pos.x - rect.min.x).max(0.0);
                                                let mapped = snap_ticks(
                                                    px_to_ticks(rel_x, zoom_x),
                                                    snap_step_ticks,
                                                );
                                                // Guarda visual mínima: 1 beat (ppqn).
                                                let min_ticks = state.ppqn.max(1);
                                                let other =
                                                    state.loop_preview_end_ticks.max(
                                                        state.loop_end_ticks.max(
                                                            min_ticks.saturating_add(1),
                                                        ),
                                                    );
                                                let max_start = other
                                                    .saturating_sub(min_ticks);
                                                state.loop_preview_start_ticks =
                                                    mapped.min(max_start);
                                                state.loop_preview_end_ticks = other;
                                                state.loop_preview_active = true;
                                                state.loop_dragging = true;
                                            }
                                        }
                                        // Arrastrar `]`: actualiza end.
                                        // Snap to Grid: cuantiza a `snap_step_ticks`.
                                        if r_resp.dragged()
                                            && state.loop_drag_handle == LoopDragHandle::Right
                                        {
                                            if let Some(pointer_pos) =
                                                r_resp.interact_pointer_pos()
                                            {
                                                let rel_x =
                                                    (pointer_pos.x - rect.min.x).max(0.0);
                                                let mapped = snap_ticks(
                                                    px_to_ticks(rel_x, zoom_x),
                                                    snap_step_ticks,
                                                );
                                                let anchor =
                                                    state.loop_preview_start_ticks;
                                                // Guarda visual mínima: 1 beat (ppqn).
                                                let min_end = anchor.saturating_add(
                                                    state.ppqn.max(1),
                                                );
                                                state.loop_preview_end_ticks =
                                                    mapped.max(min_end);
                                                state.loop_preview_active = true;
                                                state.loop_dragging = true;
                                            }
                                        }
                                        // Confirmar resize al soltar.
                                        let left_stopped = l_resp.drag_stopped();
                                        let right_stopped = r_resp.drag_stopped();
                                        if (left_stopped || right_stopped)
                                            && (state.loop_drag_handle
                                                == LoopDragHandle::Left
                                                || state.loop_drag_handle
                                                    == LoopDragHandle::Right)
                                        {
                                            // Snap to Grid en la confirmación del
                                            // resize de corchetes: cuantiza ambos
                                            // extremos a `snap_step_ticks`.
                                            let s = snap_ticks(
                                                state
                                                    .loop_preview_start_ticks
                                                    .min(state.loop_preview_end_ticks),
                                                snap_step_ticks,
                                            );
                                            let mut e = snap_ticks(
                                                state
                                                    .loop_preview_start_ticks
                                                    .max(state.loop_preview_end_ticks),
                                                snap_step_ticks,
                                            );
                                            // Guarda GUI previa al envío (ticks):
                                            // si end <= start, end = start + 4*ppqn;
                                            // si end-start < ppqn, end = start + ppqn.
                                            let ppqn_r = state.ppqn.max(1);
                                            if e <= s {
                                                e = s.saturating_add(
                                                    4u64.saturating_mul(ppqn_r),
                                                );
                                            }
                                            if e.saturating_sub(s) < ppqn_r {
                                                e = s.saturating_add(ppqn_r);
                                            }
                                            // SOLO confirma si end > start.
                                            if e > s {
                                                state.loop_start_ticks = s;
                                                state.loop_end_ticks = e;
                                                state.loop_region_active = true;
                                            }
                                            state.sanitize_loop_region();
                                            state.loop_preview_start_ticks = s;
                                            state.loop_preview_end_ticks =
                                                state.loop_end_ticks;
                                            state.loop_preview_active = false;
                                            state.loop_dragging = false;
                                            state.loop_drag_handle = LoopDragHandle::None;
                                            state.loop_drag_completed_this_frame = true;
                                        }
                                        left_bracket_resp = Some(l_resp);
                                        right_bracket_resp = Some(r_resp);
                                    }
                                }
                                let bracket_drag_active = left_bracket_resp
                                    .as_ref()
                                    .map(|r| r.dragged())
                                    .unwrap_or(false)
                                    || right_bracket_resp
                                        .as_ref()
                                        .map(|r| r.dragged())
                                        .unwrap_or(false)
                                    || state.loop_drag_handle == LoopDragHandle::Left
                                    || state.loop_drag_handle == LoopDragHandle::Right;

                                // 2. Capturar arrastre Shift + Drag sobre la regla.
                                // Crea una selección nueva completa de punta a
                                // punta. SOLO estado visual: no tocar ticks del
                                // transporte. Se omite si hay resize de
                                // corchetes en curso (prioridad al resize).
                                if allow_loop_select
                                    && shift_pressed
                                    && !bracket_drag_active
                                    && ruler_response.dragged()
                                {
                                    if let Some(pointer_pos) = ruler_response.interact_pointer_pos() {
                                        // Snap to Grid: cuantiza la posición del
                                        // Shift+Drag a `snap_step_ticks`.
                                        let current_tick = snap_ticks(
                                            px_to_ticks((pointer_pos.x - rect.min.x).max(0.0), zoom_x),
                                            snap_step_ticks,
                                        );

                                        if ruler_response.drag_started() {
                                            // X inicial del drag como preview visual.
                                            state.loop_preview_start_ticks = current_tick;
                                            state.loop_preview_end_ticks = current_tick;
                                            state.loop_preview_active = true;
                                            state.loop_dragging = true;
                                            state.loop_drag_handle = LoopDragHandle::Body;
                                        } else if let Some(origin) = ui.input(|i| i.pointer.press_origin()) {
                                            // X inicial del drag como preview_start,
                                            // X actual del drag como preview_end.
                                            // Ambos cuantizados a `snap_step_ticks`.
                                            let anchor_tick = snap_ticks(
                                                px_to_ticks((origin.x - rect.min.x).max(0.0), zoom_x),
                                                snap_step_ticks,
                                            );
                                            state.loop_preview_start_ticks = anchor_tick.min(current_tick);
                                            state.loop_preview_end_ticks = anchor_tick.max(current_tick);
                                            state.loop_preview_active = true;
                                            state.loop_dragging = true;
                                            if state.loop_drag_handle != LoopDragHandle::Body {
                                                state.loop_drag_handle = LoopDragHandle::Body;
                                            }
                                        } else if current_tick >= state.loop_preview_start_ticks {
                                            state.loop_preview_end_ticks = current_tick;
                                            state.loop_preview_active = true;
                                            state.loop_dragging = true;
                                        } else {
                                            state.loop_preview_start_ticks = current_tick;
                                            state.loop_preview_active = true;
                                            state.loop_dragging = true;
                                        }
                                    }
                                }

                                // Confirmación del Shift+Drag (selección nueva de
                                // punta a punta). No interfiere con el resize de
                                // corchetes (Left/Right ya se confirmó arriba).
                                if ruler_response.drag_stopped()
                                    && state.loop_dragging
                                    && state.loop_drag_handle != LoopDragHandle::Left
                                    && state.loop_drag_handle != LoopDragHandle::Right
                                {
                                    // Re-mapear las X finales del drag a ticks
                                    // con la fórmula del zoom activo. Fuente de
                                    // verdad: posición actual + origen del press.
                                    // Si no hay puntero disponible (soltado fuera
                                    // de la regla), se usa el preview visual.
                                    let (mapped_start, mapped_end) = match (
                                        ruler_response.interact_pointer_pos()
                                            .or_else(|| ui.input(|i| i.pointer.hover_pos())),
                                        ui.input(|i| i.pointer.press_origin()),
                                    ) {
                                        (Some(pointer_pos), Some(origin)) => {
                                            let cur_rel_x = (pointer_pos.x - rect.min.x).max(0.0);
                                            let origin_rel_x = (origin.x - rect.min.x).max(0.0);
                                            let min_x = cur_rel_x.min(origin_rel_x);
                                            let max_x = cur_rel_x.max(origin_rel_x);
                                            // Snap to Grid en la confirmación:
                                            // cuantiza ambos extremos.
                                            let start_tick = snap_ticks(
                                                px_to_ticks(min_x, zoom_x),
                                                snap_step_ticks,
                                            );
                                            let end_tick = snap_ticks(
                                                px_to_ticks(max_x, zoom_x),
                                                snap_step_ticks,
                                            );
                                            (start_tick, end_tick)
                                        }
                                        _ => (
                                            snap_ticks(
                                                state.loop_preview_start_ticks.min(state.loop_preview_end_ticks),
                                                snap_step_ticks,
                                            ),
                                            snap_ticks(
                                                state.loop_preview_start_ticks.max(state.loop_preview_end_ticks),
                                                snap_step_ticks,
                                            ),
                                        ),
                                    };

                                    let mut start_tick = mapped_start.min(mapped_end);
                                    let mut end_tick = mapped_start.max(mapped_end);

                                    // Guarda GUI previa al envío (ticks):
                                    // si end <= start, end = start + (4 * ppqn);
                                    // si end - start < ppqn, end = start + ppqn.
                                    // Solo se confirma al transporte global si
                                    // end_tick > start_tick.
                                    let ppqn_s = state.ppqn.max(1);
                                    if end_tick <= start_tick {
                                        end_tick = start_tick.saturating_add(
                                            4u64.saturating_mul(ppqn_s),
                                        );
                                    }
                                    if end_tick.saturating_sub(start_tick) < ppqn_s {
                                        end_tick = start_tick.saturating_add(ppqn_s);
                                    }

                                    state.loop_dragging = false;
                                    state.loop_preview_active = false;
                                    // Confirmar preview -> región confirmada.
                                    state.loop_start_ticks = start_tick;
                                    state.loop_end_ticks = end_tick;
                                    state.loop_region_active = true;
                                    // Garantiza ventana válida mínima (1/16): si
                                    // `loop_end <= loop_start` o la diferencia es
                                    // menor al mínimo, extiende automáticamente.
                                    // Evita rebotes infinitos dentro del mismo
                                    // buffer de audio (congelamiento).
                                    state.sanitize_loop_region();
                                    // Refuerzo extra con la misma guarda: si la
                                    // ventana quedó menor a 1 beat (ppqn),
                                    // extender a 1 beat.
                                    if state.loop_end_ticks
                                        < state.loop_start_ticks.saturating_add(ppqn_s)
                                    {
                                        state.loop_end_ticks = state
                                            .loop_start_ticks
                                            .saturating_add(ppqn_s);
                                    }
                                    start_tick = state.loop_start_ticks;
                                    end_tick = state.loop_end_ticks;
                                    // El transporte global se actualiza fuera de
                                    // aquí (app.rs al ver
                                    // `loop_drag_completed_this_frame`):
                                    // equivalente a
                                    // `transport.set_loop_region(start_tick, end_tick)`
                                    // + `transport.set_loop_enabled(true)`.
                                    // Sincronizar preview con la región saneada.
                                    state.loop_preview_start_ticks = start_tick;
                                    state.loop_preview_end_ticks = end_tick;
                                    state.loop_drag_handle = LoopDragHandle::None;
                                    state.loop_drag_completed_this_frame = true;
                                }

                                // 3. Renderizar corchetes estilo REAPER y región
                                // translúcida. Durante el drag se dibuja el
                                // preview visual; en reposo, la región
                                // confirmada. render_start/render_end SOLO
                                // alimentan el rectángulo visual (ticks -> px).
                                let (render_active, render_start, render_end) = if state.loop_dragging && state.loop_preview_active {
                                    (true, state.loop_preview_start_ticks, state.loop_preview_end_ticks)
                                } else {
                                    (state.loop_region_active, state.loop_start_ticks, state.loop_end_ticks)
                                };
                                if render_active && render_end > render_start {
                                    let start_x = rect.min.x + ticks_to_px(render_start, zoom_x);
                                    let end_x = rect.min.x + ticks_to_px(render_end, zoom_x);
                                    let (min_x, max_x) = if start_x <= end_x { (start_x, end_x) } else { (end_x, start_x) };
                                    let selection_rect = egui::Rect::from_min_max(
                                        egui::pos2(min_x, ruler_rect.min.y),
                                        egui::pos2(max_x, ruler_rect.max.y),
                                    );

                                    // Fondo translúcido
                                    painter.rect_filled(
                                        selection_rect,
                                        0.0,
                                        egui::Color32::from_rgba_unmultiplied(100, 200, 255, 40),
                                    );

                                    // Corchete de apertura `[` en loop_start y
                                    // de cierre `]` en loop_end, estilo REAPER,
                                    // con egui::Stroke / painter.line().
                                    let bracket_color = egui::Color32::LIGHT_BLUE;
                                    let bracket_stroke =
                                        egui::Stroke::new(2.0_f32, bracket_color);
                                    let tick_len = 6.0_f32;
                                    let top_y = ruler_rect.min.y + 1.0;
                                    let bottom_y = ruler_rect.max.y - 1.0;
                                    // `[`: vertical + pestañas superiores /
                                    // inferiores hacia dentro (derecha).
                                    painter.line_segment(
                                        [egui::pos2(min_x, top_y), egui::pos2(min_x, bottom_y)],
                                        bracket_stroke,
                                    );
                                    painter.line_segment(
                                        [egui::pos2(min_x, top_y), egui::pos2(min_x + tick_len, top_y)],
                                        bracket_stroke,
                                    );
                                    painter.line_segment(
                                        [egui::pos2(min_x, bottom_y), egui::pos2(min_x + tick_len, bottom_y)],
                                        bracket_stroke,
                                    );
                                    // `]`: vertical + pestañas hacia dentro
                                    // (izquierda).
                                    painter.line_segment(
                                        [egui::pos2(max_x, top_y), egui::pos2(max_x, bottom_y)],
                                        bracket_stroke,
                                    );
                                    painter.line_segment(
                                        [egui::pos2(max_x, top_y), egui::pos2(max_x - tick_len, top_y)],
                                        bracket_stroke,
                                    );
                                    painter.line_segment(
                                        [egui::pos2(max_x, bottom_y), egui::pos2(max_x - tick_len, bottom_y)],
                                        bracket_stroke,
                                    );
                                }
                            }

                            let mut current_y = rect.min.y + 24.0;
                            let valid_tracks: Vec<usize> = tracks.iter().filter(|t| !t.is_master).map(|t| t.id).collect();
                            let mut clip_to_delete: Option<usize> = None;

                            for track_id in &valid_tracks {
                                let track_rect = Rect::from_min_size(
                                    Pos2::new(rect.min.x, current_y), 
                                    Vec2::new(rect.width(), track_height)
                                );

                                painter.line_segment(
                                    [track_rect.left_bottom(), track_rect.right_bottom()], 
                                    Stroke::new(1.0_f32, Color32::from_gray(28))
                                );

                                for (clip_track_id, clip) in state.clips.iter_mut() {
                                    if clip_track_id == track_id {
                                        let clip_x = rect.min.x + ticks_to_px(clip.start_tick, zoom_x);
                                        let clip_w = ticks_to_px(clip.duration_ticks, zoom_x).max(12.0);

                                        let clip_rect = Rect::from_min_size(
                                            Pos2::new(clip_x, current_y + 1.0), 
                                            Vec2::new(clip_w, track_height - 2.0)
                                        );

                                        let handle_w = 6.0_f32;
                                        let left_handle_rect = Rect::from_min_size(clip_rect.min, Vec2::new(handle_w, clip_rect.height()));
                                        let right_handle_rect = Rect::from_min_size(
                                            Pos2::new(clip_rect.max.x - handle_w, clip_rect.min.y), 
                                            Vec2::new(handle_w, clip_rect.height())
                                        );

                                        let id_left = ui.make_persistent_id(format!("trim_L_{}_{}", clip_track_id, clip.id));
                                        let id_right = ui.make_persistent_id(format!("trim_R_{}_{}", clip_track_id, clip.id));
                                        let id_body = ui.make_persistent_id(format!("clip_body_{}_{}", clip_track_id, clip.id));

                                        let body_rect = Rect::from_min_max(
                                            Pos2::new(clip_rect.min.x + handle_w, clip_rect.min.y),
                                            Pos2::new(clip_rect.max.x - handle_w, clip_rect.max.y),
                                        );

                                        let resp_left = ui.interact(left_handle_rect, id_left, Sense::drag());
                                        let resp_right = ui.interact(right_handle_rect, id_right, Sense::drag());
                                        let resp_body = ui.interact(body_rect, id_body, Sense::click_and_drag());

                                        if resp_left.hovered() || resp_right.hovered() || resp_left.dragged() || resp_right.dragged() {
                                            ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeHorizontal);
                                        }

                                        let is_selected = state.selected_clips.contains(&clip.id);

                                        // SLICE ('S')
                                        if slice_key_pressed {
                                            if let Some(pos) = slice_hover_pos {
                                                if clip_rect.contains(pos) {
                                                    let rel_x = (pos.x - rect.min.x).max(0.0);
                                                    let raw_slice_tick = px_to_ticks(rel_x, zoom_x);
                                                    let slice_tick = if snap_step_ticks > 0 {
                                                        ((raw_slice_tick as f64 / snap_step_ticks as f64).round() as u64) * snap_step_ticks
                                                    } else {
                                                        raw_slice_tick
                                                    };

                                                    let clip_end_tick = clip.start_tick + clip.duration_ticks;

                                                    if slice_tick > clip.start_tick && slice_tick < clip_end_tick {
                                                        if let ClipType::Audio { ref sample_path, ref peaks, sample_offset_ticks, total_sample_ticks } = clip.clip_type {
                                                            let left_duration = slice_tick - clip.start_tick;
                                                            let right_offset = sample_offset_ticks + left_duration;
                                                            let right_duration = clip_end_tick - slice_tick;

                                                            let right_clip = PlaylistClip {
                                                                id: state.next_clip_id,
                                                                name: format!("{} (Slice)", clip.name),
                                                                start_tick: slice_tick,
                                                                duration_ticks: right_duration,
                                                                clip_type: ClipType::Audio {
                                                                    sample_path: sample_path.clone(),
                                                                    peaks: peaks.clone(),
                                                                    sample_offset_ticks: right_offset,
                                                                    total_sample_ticks,
                                                                },
                                                                color: clip.color,
                                                            };

                                                            pending_slices.push((*clip_track_id, right_clip));
                                                            state.next_clip_id += 1;
                                                            clip.duration_ticks = left_duration;
                                                        }
                                                    }
                                                }
                                            }
                                        }

                                        // 1. TRIM IZQUIERDO
                                        if resp_left.dragged() {
                                            if let Some(mouse_pos) = resp_left.interact_pointer_pos() {
                                                let mouse_x_rel = (mouse_pos.x - rect.min.x).max(0.0);
                                                let raw_target_tick = px_to_ticks(mouse_x_rel, zoom_x);
                                                
                                                let snapped_target_tick = if snap_step_ticks > 0 {
                                                    ((raw_target_tick as f64 / snap_step_ticks as f64).round() as u64) * snap_step_ticks
                                                } else {
                                                    raw_target_tick
                                                };

                                                if let ClipType::Audio { ref mut sample_offset_ticks, total_sample_ticks, .. } = clip.clip_type {
                                                    let current_end_tick = clip.start_tick + clip.duration_ticks;
                                                    
                                                    if snapped_target_tick < current_end_tick {
                                                        let original_start = clip.start_tick.saturating_sub(*sample_offset_ticks);
                                                        
                                                        if snapped_target_tick >= original_start {
                                                            let new_offset = snapped_target_tick - original_start;
                                                            if new_offset < total_sample_ticks {
                                                                *sample_offset_ticks = new_offset;
                                                                clip.start_tick = snapped_target_tick;
                                                                clip.duration_ticks = current_end_tick - snapped_target_tick;
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        } else if resp_left.drag_stopped() {
                                            if mode.drives_engine() {
                                                if let ClipType::Audio { sample_offset_ticks, .. } = clip.clip_type {
                                                    audio_proxy.send(GuiCommand::UpdateClipBounds {
                                                        clip_id: clip.id,
                                                        position_secs: ticks_to_secs_precise(clip.start_tick, ppqn, bpm),
                                                        duration_secs: ticks_to_secs_precise(clip.duration_ticks, ppqn, bpm),
                                                        offset_secs: ticks_to_secs_precise(sample_offset_ticks, ppqn, bpm),
                                                        track_index: *track_id,
                                                        scene_index: 0,
                                                    });
                                                }
                                            }
                                        }
                                        // 2. TRIM DERECHO
                                        else if resp_right.dragged() {
                                            if let Some(mouse_pos) = resp_right.interact_pointer_pos() {
                                                let mouse_x_rel = (mouse_pos.x - rect.min.x).max(0.0);
                                                let raw_target_tick = px_to_ticks(mouse_x_rel, zoom_x);
                                                
                                                let snapped_target_tick = if snap_step_ticks > 0 {
                                                    ((raw_target_tick as f64 / snap_step_ticks as f64).round() as u64) * snap_step_ticks
                                                } else {
                                                    raw_target_tick
                                                };

                                                if snapped_target_tick > clip.start_tick {
                                                    let requested_duration = snapped_target_tick - clip.start_tick;

                                                    if let ClipType::Audio { sample_offset_ticks, total_sample_ticks, .. } = clip.clip_type {
                                                        let max_available_duration = total_sample_ticks.saturating_sub(sample_offset_ticks);
                                                        let new_duration = requested_duration.min(max_available_duration).max(1);
                                                        clip.duration_ticks = new_duration;
                                                    }
                                                }
                                            }
                                        } else if resp_right.drag_stopped() {
                                            if mode.drives_engine() {
                                                if let ClipType::Audio { sample_offset_ticks, .. } = clip.clip_type {
                                                    audio_proxy.send(GuiCommand::UpdateClipBounds {
                                                        clip_id: clip.id,
                                                        position_secs: ticks_to_secs_precise(clip.start_tick, ppqn, bpm),
                                                        duration_secs: ticks_to_secs_precise(clip.duration_ticks, ppqn, bpm),
                                                        offset_secs: ticks_to_secs_precise(sample_offset_ticks, ppqn, bpm),
                                                        track_index: *track_id,
                                                        scene_index: 0,
                                                    });
                                                }
                                            }
                                        }
                                        // 3. MOVER CLIP
                                        else if resp_body.dragged() {
                                            if resp_body.drag_started_by(PointerButton::Primary) {
                                                if let Some(mouse_pos) = resp_body.interact_pointer_pos() {
                                                    let grab_offset = (mouse_pos.x - clip_rect.min.x).max(0.0);
                                                    ui.data_mut(|d| d.insert_temp(id_body, grab_offset));
                                                }
                                            }

                                            if let Some(mouse_pos) = resp_body.interact_pointer_pos() {
                                                let grab_offset_x: f32 = ui.data(|d| d.get_temp(id_body)).unwrap_or(0.0);
                                                let mouse_x_relative = (mouse_pos.x - rect.min.x - grab_offset_x).max(0.0);
                                                let raw_tick = px_to_ticks(mouse_x_relative, zoom_x);

                                                clip.start_tick = if snap_step_ticks > 0 {
                                                    ((raw_tick as f64 / snap_step_ticks as f64).round() as u64) * snap_step_ticks
                                                } else {
                                                    raw_tick
                                                };

                                                let rel_y = mouse_pos.y - (rect.min.y + 24.0);
                                                if rel_y >= 0.0 {
                                                    let target_idx = (rel_y / total_row_step).floor() as usize;
                                                    if target_idx < valid_tracks.len() {
                                                        *clip_track_id = valid_tracks[target_idx];
                                                    }
                                                }
                                            }
                                        } else if resp_body.drag_stopped() {
                                            if mode.drives_engine() {
                                                if let ClipType::Audio { sample_offset_ticks, .. } = clip.clip_type {
                                                    audio_proxy.send(GuiCommand::UpdateClipBounds {
                                                        clip_id: clip.id,
                                                        position_secs: ticks_to_secs_precise(clip.start_tick, ppqn, bpm),
                                                        duration_secs: ticks_to_secs_precise(clip.duration_ticks, ppqn, bpm),
                                                        offset_secs: ticks_to_secs_precise(sample_offset_ticks, ppqn, bpm),
                                                        track_index: *track_id,
                                                        scene_index: 0,
                                                    });
                                                }
                                            }
                                        }

                                        // Selección & Delete
                                        if resp_body.clicked_by(PointerButton::Primary) {
                                            if shift {
                                                if is_selected {
                                                    state.selected_clips.retain(|&id| id != clip.id);
                                                } else {
                                                    state.selected_clips.push(clip.id);
                                                }
                                            } else {
                                                state.selected_clips = vec![clip.id];
                                            }
                                        }

                                        if resp_body.secondary_clicked() {
                                            clip_to_delete = Some(clip.id);
                                        }

                                        // Dibujado del Clip
                                        let (border_color, border_width) = if is_selected {
                                            (Color32::from_rgb(255, 200, 0), 2.0_f32)
                                        } else if resp_body.dragged() || resp_left.dragged() || resp_right.dragged() {
                                            (Color32::from_rgb(0, 255, 255), 2.0_f32)
                                        } else {
                                            (Color32::from_white_alpha(100), 1.0_f32)
                                        };

                                        painter.rect_filled(clip_rect, 2.0, clip.color);
                                        painter.rect_stroke(clip_rect, 2.0, Stroke::new(border_width, border_color));

                                        if resp_left.hovered() || resp_left.dragged() {
                                            painter.rect_filled(left_handle_rect, 0.0, Color32::from_white_alpha(80));
                                        }
                                        if resp_right.hovered() || resp_right.dragged() {
                                            painter.rect_filled(right_handle_rect, 0.0, Color32::from_white_alpha(80));
                                        }

                                        // Waveform
                                        if let ClipType::Audio { peaks, sample_offset_ticks, total_sample_ticks, .. } = &clip.clip_type {
                                            if !peaks.is_empty() && *total_sample_ticks > 0 {
                                                let inner_rect = clip_rect.shrink2(Vec2::new(2.0, 4.0));
                                                let center_y = inner_rect.center().y;
                                                let wave_color = Color32::from_rgba_unmultiplied(255, 255, 255, 180);

                                                let step_x = 2.0_f32;
                                                let total_render_steps = (inner_rect.width() / step_x) as usize;

                                                let start_ratio = *sample_offset_ticks as f32 / *total_sample_ticks as f32;
                                                let duration_ratio = clip.duration_ticks as f32 / *total_sample_ticks as f32;

                                                for i in 0..total_render_steps {
                                                    let x = inner_rect.min.x + (i as f32 * step_x);
                                                    if x >= inner_rect.max.x { break; }

                                                    let local_norm = i as f32 / total_render_steps as f32;
                                                    let sample_norm = start_ratio + (local_norm * duration_ratio);

                                                    let peak_idx = (sample_norm * peaks.len() as f32) as usize;
                                                    if let Some(&peak_val) = peaks.get(peak_idx) {
                                                        let bar_height = (inner_rect.height() * 0.8) * peak_val;
                                                        if bar_height > 0.5 {
                                                            painter.line_segment(
                                                                [
                                                                    Pos2::new(x, center_y - bar_height * 0.5),
                                                                    Pos2::new(x, center_y + bar_height * 0.5),
                                                                ],
                                                                Stroke::new(1.0_f32, wave_color),
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                        }

                                        painter.text(
                                            clip_rect.min + Vec2::new(8.0, 2.0),
                                            Align2::LEFT_TOP,
                                            &clip.name,
                                            egui::FontId::proportional(10.0),
                                            Color32::WHITE
                                        );
                                    }
                                }

                                current_y += total_row_step;
                            }

                            if let Some(id_del) = clip_to_delete {
                                state.clips.retain(|(_, c)| c.id != id_del);
                            }

                            if !pending_slices.is_empty() {
                                state.clips.extend(pending_slices);
                            }

                            // Renderizado del Playhead con la fórmula canónica:
                            // pixel_x = (ticks / ppqn) * pixels_per_beat + offset.
                            // Restricción visual OpenLive: cuando `loop_enabled`
                            // está activo, `display_tick` se mapea estrictamente
                            // dentro de `[loop_start_ticks, loop_end_ticks)` para
                            // que `playhead_x` nunca se dibuje fuera de los
                            // corchetes `[` y `]`. Coincide exactamente con las
                            // coordenadas inicial/final de la barra azul del
                            // Time Selection (mismos ticks -> mismos px).
                            let display_tick = loop_display_tick(
                                state.playhead_tick,
                                state.loop_start_ticks,
                                state.loop_end_ticks,
                                loop_enabled && state.loop_region_active,
                            );
                            let playhead_x = ticks_to_pixel_x(
                                display_tick,
                                ppqn,
                                zoom_x,
                                rect.min.x,
                            );
                            painter.line_segment(
                                [Pos2::new(playhead_x, rect.min.y), Pos2::new(playhead_x, rect.max.y)],
                                Stroke::new(2.0_f32, Color32::from_rgb(0, 255, 255))
                            );

                            // Interacción de SEEK / PLAYHEAD
                            // No hacer seek mientras se define loop con Shift + Drag.
                            if (response.dragged() || response.clicked()) && !shift {
                                if let Some(pointer_pos) = response.interact_pointer_pos() {
                                    let rel_x = (pointer_pos.x - rect.min.x).max(0.0);
                                    let clicked_ticks = px_to_ticks(rel_x, zoom_x);
                                    *current_bar = (clicked_ticks as f64 / ticks_per_bar as f64) as f32 + 1.0;

                                    if mode.drives_engine() {
                                        // Seek exacto: ticks -> samples con BPM activo
                                        // + SR real + PPQN único (sin pasar por f32).
                                        let target_samples = ticks_to_samples_precise(
                                            clicked_ticks,
                                            ppqn,
                                            bpm_safe,
                                            sample_rate,
                                        );
                                        audio_proxy.send(GuiCommand::Seek { sample_count: target_samples });
                                    }
                                }
                            }
                            
                            // DROP DE SAMPLES DESDE EL EXPLORADOR
                            if let Some(path) = dragged_sample.clone() {
                                if ui.input(|i| i.pointer.any_released() || i.pointer.primary_released()) {
                                    if let Some(drop_pos) = ui.input(|i| i.pointer.hover_pos()) {
                                        if rect.contains(drop_pos) {
                                            let rel_x = (drop_pos.x - rect.min.x).max(0.0);
                                            let raw_drop_tick = px_to_ticks(rel_x, zoom_x);
                                            
                                            let drop_tick = if snap_step_ticks > 0 {
                                                ((raw_drop_tick as f64 / snap_step_ticks as f64).round() as u64) * snap_step_ticks
                                            } else {
                                                raw_drop_tick
                                            };

                                            let rel_y = drop_pos.y - (rect.min.y + 24.0);
                                            if rel_y >= 0.0 {
                                                let track_idx = (rel_y / total_row_step).floor() as usize;

                                                let valid_tracks_list: Vec<&Track> = tracks.iter().filter(|t| !t.is_master).collect();
                                                if track_idx < valid_tracks_list.len() {
                                                    let target_track_id = valid_tracks_list[track_idx].id;
                                                    let file_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                                                    let sample_path_str = path.to_string_lossy().to_string();
                                                    
                                                    let (duration_ticks, peaks) = load_sample_info(&path, state.ppqn, bpm);

                                                    let clip = PlaylistClip {
                                                        id: state.next_clip_id,
                                                        name: file_name,
                                                        start_tick: drop_tick,
                                                        duration_ticks,
                                                        clip_type: ClipType::Audio { 
                                                            sample_path: sample_path_str.clone(),
                                                            peaks,
                                                            sample_offset_ticks: 0,
                                                            total_sample_ticks: duration_ticks,
                                                        },
                                                        color: Color32::from_rgb(32, 95, 145),
                                                    };

                                                    let created_clip_id = state.next_clip_id;
                                                    state.next_clip_id += 1;
                                                    state.clips.push((target_track_id, clip));

                                                    if mode.drives_engine() {
                                                        audio_proxy.send(GuiCommand::LoadClip {
                                                            clip_id: created_clip_id,
                                                            path: sample_path_str,
                                                            position_secs: ticks_to_secs_precise(drop_tick, ppqn, bpm),
                                                            duration_secs: ticks_to_secs_precise(duration_ticks, ppqn, bpm),
                                                            offset_secs: 0.0,
                                                            track_index: target_track_id,
                                                            scene_index: 0,
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    *dragged_sample = None;
                                }
                            }
                        });
                });
            });
    });
}