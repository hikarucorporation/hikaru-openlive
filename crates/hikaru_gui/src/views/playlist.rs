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
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::prelude::StatefulInteractiveElement as _;
use gpui_kit::*;

use crate::app::{state, AppState, HikaruApp};
use crate::audio_proxy::{AudioProxy, GuiCommand};
use crate::views::clip_editor::ClipEditorTarget;
use crate::views::matrix::{
    self, end_mix_drag, h_mix_slider_ex, ms_button, pan_knob_ex, step_mix_pan_gesture,
    step_mix_slider_gesture, MatrixMixTarget,
};
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
    /// Gesto de selección rectangular en curso (click derecho + arrastrar).
    /// `None` = sin gesto. Global por la misma razón que `clip_drag`: la
    /// grilla se reconstruye cada frame.
    pub marquee: Option<MarqueeState>,
    /// Gesto de recorte de un borde de clip en curso. `None` = sin gesto.
    /// Global por la misma razón que `clip_drag`/`marquee`.
    pub trim_drag: Option<TrimDrag>,
    /// Clip cuyo borde está bajo el cursor y por cuál lado, para pintar el
    /// cursor `e-resize`/`w-resize` y resaltar el handle.
    pub trim_hover: Option<(usize, TrimEdge)>,
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
            marquee: None,
            trim_drag: None,
            trim_hover: None,
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

    /// Tick de pegado por defecto: el playhead visible (con el loop aplicado),
    /// que es donde un DAW pega por omisión.
    pub fn paste_tick(&self) -> u64 {
        loop_display_tick(
            self.playhead_tick,
            self.loop_start_ticks,
            self.loop_end_ticks,
            self.loop_region_active,
        )
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
/// arranca acá). Son 68px porque el Track Header ahora replica la caja de
/// mezcla del Session Matrix (nombre + M/S + knob de pan de 22px + fader de
/// volumen de 12px + lecturas dB/pan): con los 54px de antes los controles se
/// superponían. El zoom vertical (`Alt` + rueda) la cambia sólo en la playlist,
/// y `MIN_ROW_H` mantiene el mínimo justo para que la caja siga entrando sin
/// solaparse.
pub const TRACK_ROW_H: f32 = 68.0;
/// Alto de la regla de compases (ruler / timebar superior).
pub const RULER_H: f32 = 24.0;
/// Límites del zoom temporal (px por tick).
pub const MIN_ZOOM_X: f32 = 0.005;
pub const MAX_ZOOM_X: f32 = 2.0;
/// Límites del zoom vertical (altura de fila en px).
///
/// `MIN_ROW_H` es la caja de mezcla del header completa (68px por defecto:
/// 28px del knob + padding de 3px de cada lado + la fila del título) para que
/// los controles nunca se pisen al achicar; `MAX_ROW_H` evita filas gigantes
/// que rompan el scroll o el layout.
pub const MIN_ROW_H: f32 = 60.0;
pub const MAX_ROW_H: f32 = 180.0;

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

/// Inicia un gesto de recorte al agarrar un borde del clip.
///
/// Congela los valores originales (para arrastre incremental e inversión de
/// dirección sin zona muerta) y el offset del cursor respecto del borde.
fn start_clip_trim(cx: &mut App, clip_id: usize, edge: TrimEdge, cursor_x: f32, edge_x: f32) {
    let st = state(cx);
    st.update(cx, |s, cx| {
        let Some((_, clip)) = s.playlist_state.clips.iter().find(|(_, c)| c.id == clip_id) else {
            return;
        };
        let (orig_offset, orig_total) = match clip.clip_type {
            ClipType::Audio {
                sample_offset_ticks,
                total_sample_ticks,
                ..
            } => (sample_offset_ticks, total_sample_ticks),
            // Pattern/MIDI no tienen audio que recortar: no hay gesto.
            _ => return,
        };
        s.playlist_state.trim_drag = Some(TrimDrag {
            clip_id,
            edge,
            orig_edge_tick: match edge {
                TrimEdge::Left => clip.start_tick,
                TrimEdge::Right => clip.start_tick.saturating_add(clip.duration_ticks),
            },
            orig_start: clip.start_tick,
            orig_duration: clip.duration_ticks,
            orig_offset,
            orig_total,
            grab_dx_px: cursor_x - edge_x,
            moved: false,
        });
        cx.notify();
    });
}

/// Aplica un paso del recorte: snap del borde al cursor, en vivo.
///
/// `zone` son los bounds de la grilla para pasar de px de ventana a ticks.
fn update_clip_trim(cx: &mut App, cursor_x: f32, zone: [f32; 4]) {
    let st = state(cx);
    st.update(cx, |s, cx| {
        let Some(drag) = s.playlist_state.trim_drag else {
            return;
        };
        let ppqn = s.playlist_state.ppqn.max(1);
        let zoom = s.playlist_state.zoom_x;
        let grid = snap_step_ticks(ppqn, s.playlist_state.grid_denominator).max(1);
        let min_ticks = min_clip_ticks(ppqn);
        // Cursor menos agarre → px relativos a la grilla → ticks → snap.
        let rel_x = (cursor_x - drag.grab_dx_px - zone[0]).max(0.0);
        let edge_tick = snap_ticks(px_to_ticks(rel_x, zoom), grid);
        let Some((_, clip)) = s.playlist_state.clips.iter_mut().find(|(_, c)| c.id == drag.clip_id)
        else {
            s.playlist_state.trim_drag = None;
            return;
        };
        let (start, dur, offset) = trim_apply(
            drag.orig_start,
            drag.orig_duration,
            drag.orig_offset,
            drag.orig_total,
            drag.edge,
            edge_tick,
            grid,
            min_ticks,
        );
        if clip.start_tick == start && clip.duration_ticks == dur {
            return;
        }
        clip.start_tick = start;
        clip.duration_ticks = dur;
        if let ClipType::Audio {
            sample_offset_ticks,
            ..
        } = &mut clip.clip_type
        {
            *sample_offset_ticks = offset;
        }
        if let Some(d) = s.playlist_state.trim_drag.as_mut() {
            d.moved = true;
        }
        cx.notify();
    });
}

/// Cierra el gesto de recorte y sincroniza el clip con el motor.
///
/// Se manda un único `UpdateClipBounds` al soltar (no por frame), igual que el
/// drag de clips: el motor así no reprograma la Scheduler en cada mousemove.
fn finish_clip_trim(cx: &mut App) {
    let st = state(cx);
    st.update(cx, |s, cx| {
        let Some(drag) = s.playlist_state.trim_drag.take() else {
            return;
        };
        if drag.moved {
            // El gesto de trim no debe disparar la re-selección del `on_click`.
            s.playlist_state.clip_click_suppress = true;
            let ppqn = s.playlist_state.ppqn.max(1);
            let bpm = s.transport.bpm;
            let found = s
                .playlist_state
                .clips
                .iter()
                .find(|(_, c)| c.id == drag.clip_id)
                .map(|(k, c)| (*k, c.start_tick, c.duration_ticks, c.clip_type.clone()));
            if let Some((key, start, dur, clip_type)) = found {
                if let ClipType::Audio {
                    sample_offset_ticks, ..
                } = clip_type
                {
                    s.audio_proxy.send(GuiCommand::UpdateClipBounds {
                        clip_id: drag.clip_id,
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

/// Handle visual de un borde de clip (indicador de que se puede recortar).
///
/// Es decorativo: la zona de agarre real la decide `trim_edge_at` en el
/// `mouse_down`/`mouse_move` del clip, así el hit test no depende del layout
/// de estos hijos. Se resalta (más ancho y claro) cuando el cursor está encima
/// o cuando ese borde está siendo arrastrado.
fn clip_edge_handle(id: String, side: TrimEdge, active: bool) -> AnyElement {
    let w = if active { 3.0 } else { 2.0 };
    let col = if active {
        rgb(0xFFFFFF)
    } else {
        rgba(0xFFFFFF80)
    };
    let base = div()
        .id(SharedString::from(id))
        .test_support()
        .absolute()
        .top(px(0.0))
        .bottom(px(0.0))
        .w(px(w))
        .bg(col);
    base.left(px(if side == TrimEdge::Left { 0.0 } else { -w }))
        .into_any_element()
}

/// Origen de la grilla (panel temporal) en coords de VENTANA, a partir de los
/// bounds medidos de la zona de seek.
///
/// `zone_bounds` registra la zona de seek, que va Montada en `top = ruler_h`
/// dentro del panel: su origen en Y es el del panel + la regla. Por eso acá se
/// descuenta `ruler_h` para recuperar el origen real del panel (donde arranca
/// el compás 1 y donde se posiciona el rect del marquee).
///
/// Se usa en los HANDLERS (no en el render): leer el `Cell` durante el render
/// da el valor del frame anterior, y en el primer frame del marquee todavía
/// valía 0 — eso corría el rect por todo el ancho de los headers + la regla.
pub fn grid_origin_from_zone(zone: [f32; 4], ruler_h: f32) -> (f32, f32) {
    (zone[0], zone[1] - ruler_h)
}

/// Avanza el rectángulo del marquee con la posición del cursor.
///
/// Se llama desde el catcher global (así el gesto sobrevive aunque el cursor
/// pase por encima de un clip o salga del panel). La posición del evento viene en
/// coords de ventana y se convierte a LOCALES de la grilla al vuelo, así el
/// estado siempre vive en el mismo sistema que `clip_rects`.
fn update_marquee(x: f32, y: f32, zone: [f32; 4], ruler_h: f32, cx: &mut App) {
    let (gx, gy) = grid_origin_from_zone(zone, ruler_h);
    let st = state(cx);
    st.update(cx, |s, cx| {
        if let Some(m) = s.playlist_state.marquee {
            s.playlist_state.marquee = Some(MarqueeState {
                cur_x: x - gx,
                cur_y: y - gy,
                ..m
            });
            cx.notify();
        }
    });
}

/// Cierra el gesto de marquee tras soltar el botón derecho.
///
/// - Si el gesto superó `MARQUEE_DRAG_THRESHOLD`: selecciona todos los clips
///   que tocan el rectángulo. Con `Shift` la selección se SUMA a la existente;
///   sin él, reemplaza (comportamiento estándar de caja).
/// - Si NO lo superó (click simple): es el gesto del menú contextual, así que
///   sólo se limpia el estado — el menú lo abre el `context_menu` del elemento.
///
/// Siempre limpia `marquee` y notifica (un frame por evento).
fn finish_marquee(
    cur: (f32, f32),
    zone: [f32; 4],
    ruler_h: f32,
    clip_rects: &[(usize, usize, f32, f32, f32, f32)],
    additive: bool,
    cx: &mut App,
) {
    let (gx, gy) = grid_origin_from_zone(zone, ruler_h);
    let st = state(cx);
    st.update(cx, |s, cx| {
        let Some(m) = s.playlist_state.marquee else {
            return;
        };
        s.playlist_state.marquee = None;
        // El ancla quedó fijada en coords LOCALES al apretar; sólo el extremo
        // final viene en coords de ventana y se convierte acá.
        let end = MarqueeState {
            cur_x: cur.0 - gx,
            cur_y: cur.1 - gy,
            ..m
        };
        // Click simple: no hay caja que aplicar, el menú contextual abre.
        if !end.is_drag() {
            cx.notify();
            return;
        }
        // Ambos extremos ya están en coords de grilla, igual que `clip_rects`:
        // se comparan sin ninguna conversión adicional.
        let rect = end.normalized();
        let keys = playlist_row_keys(s);
        let hits = clips_in_marquee(clip_rects, rect, &keys);
        if additive {
            for id in hits {
                if !s.playlist_state.selected_clips.contains(&id) {
                    s.playlist_state.selected_clips.push(id);
                }
            }
        } else {
            s.playlist_state.selected_clips = hits;
        }
        cx.notify();
    });
}

/// Re-sincroniza TODOS los clips con el motor tras un cambio en el conjunto
/// (pegar, duplicar, cortar).
///
/// Los clips de audio se recargan con `LoadClip` (mismo id ⇒ el motor
/// reemplaza el existente). Los de patrón/MIDI no tienen mensaje equivalente
/// y quedan para el ciclo normal de `needs_full_sync`.
fn sync_clips_to_engine(s: &mut AppState, _cx: &mut App) {
    let bpm = s.transport.bpm;
    let proxy = s.audio_proxy.clone();
    s.playlist_state.sync_all_clips_to_engine(&proxy, bpm);
}

/// Gesto de desplazamiento con botón central (ruedita) en curso.
///
/// Igual que `MixerPanState` pero para la Playlist: se guarda la última
/// posición vista del puntero y cada `MouseMove` aplica sólo el delta
/// incremental. Es incremental (no absoluto `ancla + offset inicial`) porque
/// un esquema absoluto acumularía valor crudo fuera de rango al pasar un límite
/// y el viewport tardaría en responder al volver: así no hay zonas muertas.
///
/// Los offsets de scroll viven en `ScrollHandle` con signo NEGATIVO
/// (`[-max_offset, 0]`), por eso el clamp de `clamp_scroll_offset` es a la
/// izquierda, no a 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaylistPanState {
    pub last: (f32, f32),
}

/// Sensibilidad del pan: 1px de mouse = 1px de viewport.
///
/// 1:1 da control preciso compás por compás. El alcance se logra con trazos
/// largos; amplificar volvería el gesto incontrolable y haría que el viewport
/// salte de un límite al otro con un flick corto.
pub const PAN_SENSITIVITY: f32 = 1.0;

/// Acota un offset de scroll al rango real del handle: `[-max, 0]`.
///
/// Devolver el valor YA acotado es lo que mantiene sincronizada la scrollbar
/// (que lee el mismo handle) paso a paso.
pub fn clamp_scroll_offset(current: f32, max: f32) -> f32 {
    (current).clamp(-max.max(0.0), 0.0)
}

fn is_playlist_panning(cx: &mut App) -> bool {
    state(cx).read(cx).playlist_pan.is_some()
}

/// Inicia el pan: sembramos la referencia del gesto.
fn start_playlist_pan(cx: &mut App, position: Point<Pixels>) {
    let st = state(cx);
    st.update(cx, |s, cx| {
        s.playlist_pan = Some(PlaylistPanState {
            last: (position.x.as_f32(), position.y.as_f32()),
        });
        cx.notify();
    });
}

/// Aplica un paso del pan: delta incremental del cursor sobre los dos ejes.
fn update_playlist_pan(cx: &mut App, position: Point<Pixels>) {
    let st = state(cx);
    st.update(cx, |s, cx| {
        let Some(pan) = s.playlist_pan else {
            return;
        };
        // El viewport sigue al cursor: arrastrar a la derecha/abajo lo mueve
        // en ese sentido, y con offsets negativos eso es restar el delta.
        let dx = (position.x.as_f32() - pan.last.0) * PAN_SENSITIVITY;
        let dy = (position.y.as_f32() - pan.last.1) * PAN_SENSITIVITY;

        let h = s.playlist_scroll_h.clone();
        let new_x = clamp_scroll_offset(h.offset().x.as_f32() - dx, h.max_offset().x.as_f32());
        h.set_offset(point(px(new_x), h.offset().y));

        let v = s.playlist_scroll_v.clone();
        let new_y = clamp_scroll_offset(v.offset().y.as_f32() - dy, v.max_offset().y.as_f32());
        v.set_offset(point(v.offset().x, px(new_y)));

        // Avanza la referencia: el próximo evento acumula desde acá. (Si el
        // mismo evento burbujea por los dos contenedores, la segunda pasada ve
        // delta cero y es no-op.)
        if let Some(p) = s.playlist_pan.as_mut() {
            p.last = (position.x.as_f32(), position.y.as_f32());
        }
        // El `notify` es obligatorio: mutar un `ScrollHandle` no invalida nada
        // por sí solo, así que sin esto el viewport se movería a saltos.
        cx.notify();
    });
}

/// Cierra el pan y restaura el cursor por defecto.
fn stop_playlist_pan(cx: &mut App) {
    let st = state(cx);
    st.update(cx, |s, cx| {
        if s.playlist_pan.take().is_some() {
            cx.notify();
        }
    });
}

/// Cursor de edición de clips (`Ctrl`/`Cmd` + tecla). `shift` distingue
/// `Ctrl+Shift+V` (pegarasure-style) del pegado normal.
///
/// Trabaja sobre `&mut AppState` para que el pegado también sincronice al motor
/// (ids nuevos → `LoadClip`), igual que el drop por arrastre. Devuelve `true`
/// si el atajo era de edición y se consumió.
pub fn handle_edit_shortcut(key: &str, shift: bool, cx: &mut App) -> bool {
    let st = state(cx);
    match key {
        "c" => {
            st.update(cx, |s, _| {
                copy_selected_clips(&mut s.playlist_state);
            });
            true
        }
        "x" => {
            st.update(cx, |s, cx| {
                if cut_selected_clips(&mut s.playlist_state) > 0 {
                    sync_clips_to_engine(s, cx);
                }
            });
            true
        }
        "v" => {
            st.update(cx, |s, cx| {
                let grid = snap_step_ticks(s.playlist_state.ppqn, s.playlist_state.grid_denominator)
                    .max(1);
                // `Ctrl+V` pega en el playhead; `Ctrl+Shift+V` es el pegado
                // explícito del mismo buffer (ambas rutas usan el clipboard
                // interno, no el del sistema).
                let at = s.playlist_state.paste_tick();
                let keys = playlist_row_keys(s);
                let ids = paste_clips(&mut s.playlist_state, at, grid, &keys);
                if ids.is_empty() {
                    return;
                }
                sync_clips_to_engine(s, cx);
                let _ = shift;
            });
            true
        }
        "d" => {
            st.update(cx, |s, cx| {
                let grid = snap_step_ticks(s.playlist_state.ppqn, s.playlist_state.grid_denominator)
                    .max(1);
                let ids = duplicate_selected_clips(&mut s.playlist_state, grid);
                if !ids.is_empty() {
                    sync_clips_to_engine(s, cx);
                }
            });
            true
        }
        _ => false,
    }
}

// =========================================================================
// RECORTE DE CLIPS (TRIMMING) EN LOS BORDES
// =========================================================================

/// Ancho en px de la zona de agarre de cada borde del clip (izq/der).
///
/// 6px es la zona "de facility" de un DAW: entra sin Require Precisión pero
/// todavía es fácil de(err)ar fuera porque el resto del clip es superficie de
/// arrastre. Los handles dibujados son más angostos que esta zona para no
/// tapar el waveform.
pub const TRIM_EDGE_PX: f32 = 6.0;

/// Duración mínima de un clip en ticks. Nunca 0: un clip de duración 0 no se
/// puede renderizar ni recortar. Es la unidad del snap más gruesa dividida.
pub fn min_clip_ticks(ppqn: u64) -> u64 {
    (ppqn.max(1) / 4).max(1)
}

/// Qué borde del clip se está arrastrando.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrimEdge {
    Left,
    Right,
}

impl TrimEdge {
    pub fn cursor(self) -> CursorStyle {
        match self {
            TrimEdge::Left => CursorStyle::ResizeLeft,
            TrimEdge::Right => CursorStyle::ResizeRight,
        }
    }
}

/// Gesto de recorte en curso: mantiene los valores ORIGINALES del clip para
/// que el arrastre sea incremental (como el drag de clips) y para poder
/// invertir la dirección en un borde sin zona muerta.
///
/// `grab_dx_px` es la distancia entre el cursor y el borde agarrado, para que
/// el clip no salte al agarrarlo fuera del borde.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrimDrag {
    pub clip_id: usize,
    pub edge: TrimEdge,
    /// Ticks del borde agarrado al iniciar (start si `Left`, end si `Right`).
    pub orig_edge_tick: u64,
    /// Start/duración/offset de audio originales.
    pub orig_start: u64,
    pub orig_duration: u64,
    pub orig_offset: u64,
    pub orig_total: u64,
    pub grab_dx_px: f32,
    pub moved: bool,
}

/// Hit test de los bordes: devuelve el borde agarrable del clip bajo el cursor.
///
/// `x` es la posición del cursor en coords de VENTANA; `clip_x/w` también (los
///中日 los lee `render` del mismo espacio que los eventos). La zona se
/// extiende un poco más si el clip es angosto que `edge_px`, para que siempre
/// haya superficie agarrable. Función pura para testear.
pub fn trim_edge_at(x: f32, clip_x: f32, clip_w: f32, edge_px: f32) -> Option<TrimEdge> {
    if clip_w <= 0.0 {
        return None;
    }
    // Los bordes están donde están; lo que se expande es la ZONA de agarre
    // (hacia adentro) cuando el clip es más angosto que 2 zonas, para que
    // siempre haya superficie agarrable sin mover el borde real.
    let left = clip_x;
    let right = clip_x + clip_w;
    // El centro del clip gana: si `x` cae en ambos bordes, el más cercano.
    if x >= left && x <= left + edge_px {
        if x - left <= right - x {
            return Some(TrimEdge::Left);
        }
    }
    if x >= right - edge_px && x <= right {
        if right - x <= x - left {
            return Some(TrimEdge::Right);
        }
    }
    None
}

/// Resultado de un recorte: `(start_tick, duration_ticks, offset_ticks)`.
///
/// `orig_start/dur/offset` son los valores antes del gesto, `edge_tick` el tick
/// al que se snapió el borde agarrado. Función pura para testear.
///
/// - Borde derecho: sólo cambia la duración. Al alargarlo más allá del audio
///   disponible se recorta contra `max_dur` (fin real del sample), así no
///   aparece silencio inventado.
/// - Borde izquierdo: se mueve el inicio del clip Y el offset interno del
///   audio en la misma cantidad, de modo que el waveform no "salta" dentro del
///   clip (esto es el trim clásico: el contenido bajo la grilla no se mueve).
///   No se deja pasar del offset 0 ni del final del audio.
pub fn trim_apply(
    orig_start: u64,
    orig_dur: u64,
    orig_offset: u64,
    orig_total: u64,
    edge: TrimEdge,
    edge_tick: u64,
    grid_ticks: u64,
    min_ticks: u64,
) -> (u64, u64, u64) {
    match edge {
        TrimEdge::Right => {
            // El largo real del sample es `orig_total`; desde el offset
            // actual quedan `total - offset` ticks de audio. El fin no puede
            // pasar de ahí (si no, aparecería silencio inventado).
            let audio_left = orig_total.saturating_sub(orig_offset);
            let audio_end = orig_start
                .saturating_add(audio_left)
                .max(orig_start.saturating_add(min_ticks));
            let new_end = edge_tick
                .max(orig_start.saturating_add(min_ticks))
                .min(audio_end);
            let dur = new_end.saturating_sub(orig_start).max(min_ticks);
            (orig_start, dur, orig_offset)
        }
        TrimEdge::Left => {
            // El offset nunca puede ser negativo (no hay audio antes del
            // sample) ni pasar del fin del sample.
            let max_tick = orig_start
                .saturating_add(orig_dur)
                .saturating_sub(min_ticks);
            let new_start = edge_tick.clamp(orig_offset, max_tick);
            let offset = orig_offset.saturating_add(new_start.saturating_sub(orig_start));
            let dur = orig_start
                .saturating_add(orig_dur)
                .saturating_sub(new_start)
                .max(min_ticks);
            let _ = (grid_ticks, orig_total);
            (new_start, dur, offset)
        }
    }
}

/// Extremos temporales de un clip de audio: `(offset_disponible, fin_audio)`.
/// Funciones puras auxiliares para los tests de trimming.
pub fn audio_span(offset: u64, total: u64) -> (u64, u64) {
    (offset, offset.saturating_add(total))
}

// =========================================================================
// SELECCIÓN POR CAJA (marquee / rubber-band) Y EDICIÓN DE CLIPS
// =========================================================================

/// Desplazamiento mínimo (px) para que un click derecho cuente como arrastre
/// de marquee y no como "click simple" para abrir el menú contextual.
///
/// Por debajo de este umbral el gesto se resuelve como click simple: abrir un
/// menú por un temblor de 1px al querer seleccionar sería molesto.
pub const MARQUEE_DRAG_THRESHOLD: f32 = 4.0;

/// Gesto de selección rectangular en curso (click derecho + arrastrar).
///
/// Las coordenadas son de VENTANA (las que trae el evento) porque el marquee
/// se pinta en el mismo espacio que la grilla; el mapeo a ticks/filas se hace
/// al soltar usando los bounds medidos de la zona (`zone_bounds`).
///
/// Vive en `PlaylistState` (global, no flag local del widget) porque la grilla
/// se reconstruye en cada frame: con un flag local el primer `notify` cortaría
/// el gesto y el rectángulo no seguiría al cursor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarqueeState {
    pub start_x: f32,
    pub start_y: f32,
    pub cur_x: f32,
    pub cur_y: f32,
}

impl MarqueeState {
    /// Distancia manhattan recorrida: barata y suficiente para el umbral.
    pub fn drag_distance(&self) -> f32 {
        (self.cur_x - self.start_x).abs() + (self.cur_y - self.start_y).abs()
    }

    /// ¿Superó el umbral para considerarse arrastre y no click simple?
    pub fn is_drag(&self) -> bool {
        self.drag_distance() >= MARQUEE_DRAG_THRESHOLD
    }

    /// Rectángulo normalizado `(x0, y0, x1, y1)`: arrastrar hacia arriba o
    /// hacia la izquierda invierte los extremos en vez de dar un rect con
    /// ancho/alto negativo.
    pub fn normalized(&self) -> (f32, f32, f32, f32) {
        let x0 = self.start_x.min(self.cur_x);
        let y0 = self.start_y.min(self.cur_y);
        let x1 = self.start_x.max(self.cur_x);
        let y1 = self.start_y.max(self.cur_y);
        (x0, y0, x1, y1)
    }
}

/// Rectángulo normalizado a partir de dos esquinas sueltas. Función pura.
pub fn normalize_rect(ax: f32, ay: f32, bx: f32, by: f32) -> (f32, f32, f32, f32) {
    (ax.min(bx), ay.min(by), ax.max(bx), ay.max(by))
}

/// ¿Se intersectan dos rectángulos `(x0, y0, x1, y1)`?
///
/// Toque en el borde cuenta como intersección (el rect del marquee es
/// translúcido y apenas toca el clip cuando el usuario roza su borde), así que
/// la comparación es inclusiva (`<=`).
pub fn rects_intersect(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> bool {
    a.0 <= b.2 && a.2 >= b.0 && a.1 <= b.3 && a.3 >= b.1
}

/// Ids de los clips que tocan el rectángulo del marquee.
///
/// `rows` son los clips con su geometría ya en px de ventana
/// `(track_key, clip_id, x, y, w, h)`. Sólo entra el clip si su track sigue
/// existiendo (`track_keys` acota las filas visibles) — evita seleccionar
/// clips huérfanos tras borrar una pista. Función pura para testear.
pub fn clips_in_marquee(
    rows: &[(usize, usize, f32, f32, f32, f32)],
    rect: (f32, f32, f32, f32),
    track_keys: &[usize],
) -> Vec<usize> {
    rows.iter()
        .filter(|(track_key, _, x, y, w, h)| {
            track_keys.contains(track_key) && rects_intersect(rect, (*x, *y, x + w, y + h))
        })
        .map(|(_, clip_id, _, _, _, _)| *clip_id)
        .collect()
}

/// Extremos temporales de un conjunto de clips: `(min_start, max_end)`.
///
/// `max_end` es el tick final del clip más largo (start + duration). Se usa
/// para pegar en bloque justo a continuación del final del conjunto.
fn selection_span(clips: &[(usize, PlaylistClip)]) -> (u64, u64) {
    let mut min_start = u64::MAX;
    let mut max_end = 0u64;
    for (_, c) in clips {
        min_start = min_start.min(c.start_tick);
        max_end = max_end.max(c.start_tick.saturating_add(c.duration_ticks));
    }
    if min_start == u64::MAX {
        (0, 0)
    } else {
        (min_start, max_end)
    }
}

/// Copia los clips seleccionados al buffer interno (sin tocar la grilla).
///
/// Devuelve cuántos clips entraron al portapapeles. Sin selección es no-op
/// (`Ctrl+C` no debe limpiar un clipboard ya lleno).
pub fn copy_selected_clips(s: &mut PlaylistState) -> usize {
    if s.selected_clips.is_empty() {
        return 0;
    }
    let picked: Vec<(usize, PlaylistClip)> = s
        .clips
        .iter()
        .filter(|(_, c)| s.selected_clips.contains(&c.id))
        .cloned()
        .collect();
    let n = picked.len();
    if n > 0 {
        s.clipboard = picked;
    }
    n
}

/// Copia al buffer y borra los clips seleccionados de la grilla (`Ctrl+X`).
///
/// Devuelve cuántos clips se movieron al portapapeles.
pub fn cut_selected_clips(s: &mut PlaylistState) -> usize {
    let n = copy_selected_clips(s);
    if n > 0 {
        let sel = s.selected_clips.clone();
        s.clips.retain(|(_, c)| !sel.contains(&c.id));
        s.selected_clips.clear();
    }
    n
}

/// Generador de ids de clip libre (no colisiona con los ya presentes).
fn next_free_clip_id(s: &PlaylistState) -> usize {
    s.clips
        .iter()
        .map(|(_, c)| c.id)
        .max()
        .unwrap_or(0)
        .max(s.next_clip_id)
        + 1
}

/// Pega el clipboard en `at_tick` (con snap), conservando cada clip en su
/// pista y su offset relativo al inicio del conjunto copiado.
///
/// Devuelve los ids de los clips creados. Si el clipboard está vacío o el
/// track de destino ya no existe, no inserta nada.
pub fn paste_clips(
    s: &mut PlaylistState,
    at_tick: u64,
    grid_ticks: u64,
    track_keys: &[usize],
) -> Vec<usize> {
    if s.clipboard.is_empty() {
        return Vec::new();
    }
    let (min_start, _) = selection_span(&s.clipboard);
    // El primer tick del conjunto queda pegado en `at_tick` (ya snapped por el
    // llamador) y cada clip conserva su separación relativa original.
    let delta = at_tick.saturating_sub(min_start);
    let mut new_ids = Vec::new();
    for (track_key, clip) in &s.clipboard {
        if !track_keys.contains(track_key) {
            continue;
        }
        let id = next_free_clip_id(s);
        let mut copy = clip.clone();
        copy.id = id;
        copy.start_tick = snap_ticks(clip.start_tick.saturating_add(delta), grid_ticks);
        s.clips.push((*track_key, copy));
        new_ids.push(id);
        s.next_clip_id = s.next_clip_id.max(id + 1);
    }
    s.selected_clips = new_ids.clone();
    new_ids
}

/// Duplica los clips seleccionados justo a continuación de su punto final
/// (`Ctrl+D`): conserva pistas, duración y orden, y queda seleccionado lo nuevo.
///
/// Devuelve los ids de los duplicados.
pub fn duplicate_selected_clips(s: &mut PlaylistState, grid_ticks: u64) -> Vec<usize> {
    if s.selected_clips.is_empty() {
        return Vec::new();
    }
    let picked: Vec<(usize, PlaylistClip)> = s
        .clips
        .iter()
        .filter(|(_, c)| s.selected_clips.contains(&c.id))
        .cloned()
        .collect();
    if picked.is_empty() {
        return Vec::new();
    }
    let (_, max_end) = selection_span(&picked);
    // El conjunto duplicado arranca exactamente donde termina el original.
    let delta = max_end.saturating_sub(selection_span(&picked).0);
    let mut new_ids = Vec::new();
    for (track_key, clip) in &picked {
        let id = next_free_clip_id(s);
        let mut copy = clip.clone();
        copy.id = id;
        copy.start_tick = snap_ticks(clip.start_tick.saturating_add(delta), grid_ticks);
        s.clips.push((*track_key, copy));
        new_ids.push(id);
        s.next_clip_id = s.next_clip_id.max(id + 1);
    }
    s.selected_clips = new_ids.clone();
    new_ids
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
    row_keys_of(match s.mode {
        crate::app::AppMode::OpenLive => &s.live_tracks,
        crate::app::AppMode::OpenStudio => &s.studio_tracks,
    })
}

/// Índices de fila (no-master) de un vector de pistas. Función pura: la
/// comparten el render, el drag de clips y las operaciones de clipboard.
fn row_keys_of(tracks: &[Track]) -> Vec<usize> {
    tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.is_master)
        .map(|(i, _)| i)
        .collect()
}

// =========================================================================
// MEZCLA EN LOS TRACK HEADERS (M/S/volumen/pan) — espejo del Session Matrix
// =========================================================================
//
// Los headers de la Playlist usan los MISMOS widgets que el Session Matrix
// (`matrix::h_mix_slider_ex`, `pan_knob_ex`, `ms_button`), así que el diseño
// es idéntico y sólo cambia la destino de la escritura: los helpers de abajo
// escriben en la pista del MODO ACTIVO y espejan al mixer y a la Session
// Matrix (más el `GuiCommand` al motor). Así M/S/vol/pan quedan
// bidireccionales entre Playlist, Mixer y Matrix.

/// Índice de fila de la playlist → índice de pista del motor.
///
/// En OpenStudio la fila 0 es `studio_tracks[0]` (el índice coincide con el
/// vector); en OpenLive la fila 0 es `live_tracks[1]` (el 0 es el master y la
/// Matrix indexa sus pistas desde 0). Es la misma convención de
/// `playlist_row_keys` + `arranger_view::StripTarget`.
fn playlist_row_to_engine_idx(s: &AppState, row: usize) -> usize {
    match s.mode {
        crate::app::AppMode::OpenLive => row,
        crate::app::AppMode::OpenStudio => row,
    }
}

/// Escribe el volumen en la pista del modo activo, espejando al resto de vistas.
fn apply_playlist_volume(s: &mut AppState, row: usize, volume: f32) {
    let volume = volume.clamp(0.0, 1.0);
    let engine_idx = playlist_row_to_engine_idx(s, row);
    match s.mode {
        crate::app::AppMode::OpenLive => {
            if let Some(t) = s.matrix_state.tracks.get_mut(row) {
                t.volume = volume;
            }
            if let Some(live) = s.live_tracks.get_mut(engine_idx + 1) {
                live.volume = volume;
            }
        }
        crate::app::AppMode::OpenStudio => {
            if let Some(t) = s.studio_tracks.get_mut(engine_idx) {
                t.volume = volume;
            }
        }
    }
    s.audio_proxy.send(GuiCommand::SetTrackVolume {
        track_idx: engine_idx,
        volume_db: volume,
    });
}

/// Escribe el paneo en la pista del modo activo, espejando al resto de vistas.
fn apply_playlist_pan(s: &mut AppState, row: usize, pan: f32) {
    let pan = pan.clamp(-1.0, 1.0);
    let engine_idx = playlist_row_to_engine_idx(s, row);
    match s.mode {
        crate::app::AppMode::OpenLive => {
            if let Some(t) = s.matrix_state.tracks.get_mut(row) {
                t.pan = pan;
            }
            if let Some(live) = s.live_tracks.get_mut(engine_idx + 1) {
                live.pan = pan;
            }
        }
        crate::app::AppMode::OpenStudio => {
            if let Some(t) = s.studio_tracks.get_mut(engine_idx) {
                t.pan = pan;
            }
        }
    }
    s.audio_proxy.send(GuiCommand::SetTrackPan {
        track_idx: engine_idx,
        pan,
    });
}

/// Invierte Mute: pista del modo activo + espejo Matrix/Mixer + motor.
fn toggle_playlist_mute(cx: &mut App, row: usize) {
    let st = state(cx);
    st.update(cx, |s, cx| {
        let engine_idx = playlist_row_to_engine_idx(s, row);
        let muted = match s.mode {
            crate::app::AppMode::OpenLive => {
                let muted = s
                    .matrix_state
                    .tracks
                    .get(row)
                    .map(|t| !t.muted)
                    .unwrap_or(false);
                if let Some(t) = s.matrix_state.tracks.get_mut(row) {
                    t.muted = muted;
                }
                if let Some(live) = s.live_tracks.get_mut(engine_idx + 1) {
                    live.mute = muted;
                }
                muted
            }
            crate::app::AppMode::OpenStudio => {
                let Some(t) = s.studio_tracks.get_mut(engine_idx) else {
                    return;
                };
                t.mute = !t.mute;
                t.mute
            }
        };
        s.audio_proxy.send(GuiCommand::SetTrackMute {
            track_idx: engine_idx,
            mute: muted,
        });
        cx.notify();
    });
}

/// Invierte Solo: pista del modo activo + espejo Matrix/Mixer + motor.
fn toggle_playlist_solo(cx: &mut App, row: usize) {
    let st = state(cx);
    st.update(cx, |s, cx| {
        let engine_idx = playlist_row_to_engine_idx(s, row);
        let soloed = match s.mode {
            crate::app::AppMode::OpenLive => {
                let soloed = s
                    .matrix_state
                    .tracks
                    .get(row)
                    .map(|t| !t.soloed)
                    .unwrap_or(false);
                if let Some(t) = s.matrix_state.tracks.get_mut(row) {
                    t.soloed = soloed;
                }
                if let Some(live) = s.live_tracks.get_mut(engine_idx + 1) {
                    live.solo = soloed;
                }
                soloed
            }
            crate::app::AppMode::OpenStudio => {
                let Some(t) = s.studio_tracks.get_mut(engine_idx) else {
                    return;
                };
                t.solo = !t.solo;
                t.solo
            }
        };
        s.audio_proxy.send(GuiCommand::SetTrackSolo {
            track_idx: engine_idx,
            solo: soloed,
        });
        cx.notify();
    });
}

/// Estado de mezcla de una fila para pintar el header (nombre, M, S, vol, pan).
fn playlist_row_mix(s: &AppState, row: usize) -> (String, bool, bool, f32, f32) {
    let engine_idx = playlist_row_to_engine_idx(s, row);
    match s.mode {
        crate::app::AppMode::OpenLive => {
            let live = s.live_tracks.get(engine_idx + 1);
            let mx = s.matrix_state.tracks.get(row);
            match (live, mx) {
                (Some(l), Some(m)) => (l.name.clone(), m.muted, m.soloed, m.volume, m.pan),
                (Some(l), None) => (l.name.clone(), l.mute, l.solo, l.volume, l.pan),
                _ => (format!("TRK {:02}", row + 1), false, false, 0.75, 0.0),
            }
        }
        crate::app::AppMode::OpenStudio => match s.studio_tracks.get(engine_idx) {
            Some(t) => (t.name.clone(), t.mute, t.solo, t.volume, t.pan),
            None => (format!("TRK {:02}", row + 1), false, false, 0.75, 0.0),
        },
    }
}

/// Header de pista de la Playlist con la MISMA caja de mezcla que la Session
/// Matrix: nombre (con ellipsis) + [M] [S], y debajo knob de pan + lectura
/// C/Lxx/Rxx + fader de volumen + dB.
///
/// Los widgets son los de `matrix` (reutilizados vía `*_ex` con el apply de
/// esta vista), así el comportamiento de drag/doble-clic es idéntico y sólo
/// cambia la escritura. El click en el fondo selecciona la pista destino.
fn playlist_track_header_with_selected(
    row: usize,
    vec_idx: usize,
    name: String,
    muted: bool,
    soloed: bool,
    volume: f32,
    pan: f32,
    mix_drag: Option<MatrixMixTarget>,
    selected_row: bool,
    row_h: f32,
) -> AnyElement {
    v_flex()
        .w_full()
        // Alto EXPLÍCITO = alto del carril de la grilla. Antes usaba
        // `h_full()`, que dentro de la `v_flex` de headers resuelve al alto
        // del CONTENIDO (título + caja de mezcla), no al del carril: medía
        // 73.5px contra los 68px de la fila y acumulaba 5.5px de desfase por
        // pista. Fijarlo acá además ata el header al zoom vertical, porque
        // ambos leen el mismo `track_height` acotado.
        .h(px(row_h))
        .flex_shrink_0()
        .bg(rgb(0x1C1C20))
        .border_1()
        .border_color(if selected_row {
            rgb(0x5AB4FF)
        } else {
            rgb(0x2D2D37)
        })
        .rounded(px(4.0))
        .p(px(1.0))
        .gap(px(1.0))
        .overflow_hidden()
        .id(format!("pl_row_{}", vec_idx))
        .test_support()
        .on_click(move |_, _, cx| {
            let st = state(cx);
            st.update(cx, |state, cx| {
                state.selected_track_index = vec_idx;
                cx.notify();
            });
        })
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .gap(px(4.0))
                .flex_shrink_0()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .child(
                            Label::new(name)
                                .text_xs()
                                .text_color(rgb(0xE0E0E0))
                                .text_ellipsis(),
                        ),
                )
                .child(
                    h_flex()
                        .gap(px(2.0))
                        .flex_shrink_0()
                        .child(ms_button(
                            format!("pl_track_mute_{}", vec_idx),
                            "M",
                            muted,
                            rgb(0xFF5050),
                            move |cx| toggle_playlist_mute(cx, row),
                        ))
                        .child(ms_button(
                            format!("pl_track_solo_{}", vec_idx),
                            "S",
                            soloed,
                            rgb(0xFFC800),
                            move |cx| toggle_playlist_solo(cx, row),
                        )),
                ),
        )
        .child(
            h_flex()
                .items_center()
                .gap(px(4.0))
                .flex_shrink_0()
                .overflow_hidden()
                .child(pan_knob_ex(
                    SharedString::from(format!("pl_panknob_{}", vec_idx)),
                    row,
                    pan,
                    mix_drag == Some(MatrixMixTarget::Pan(row)),
                    apply_playlist_pan,
                ))
                .child(
                    Label::new(matrix::pan_text(matrix::snap_center_pan(pan)))
                        .text_size(px(9.0))
                        .text_color(rgb(0xE0E0E0))
                        .w(px(24.0)),
                )
                .child(h_mix_slider_ex(
                    format!("pl_vol_{}", vec_idx),
                    row,
                    volume,
                    mix_drag == Some(MatrixMixTarget::Volume(row)),
                    matrix::VOLUME_RESET,
                    apply_playlist_volume,
                ))
                .child(
                    Label::new(matrix::db_text(volume))
                        .text_size(px(9.0))
                        .text_color(rgb(0xE0E0E0))
                        .w(px(42.0)),
                ),
        )
        .into_any_element()
}

/// Cursor de recorte activo por clip: `clip_id` bajo el que hay un borde
/// agarrable, o `None` para volver a la flecha.
///
/// Vive en `PlaylistState` porque el clip se reconstruye cada frame: el cursor
/// se decide en el render desde acá (igual que `clip_drag`), no en un closure.
fn set_clip_edge_cursor(cx: &mut App, clip_id: usize, edge: Option<TrimEdge>) {
    let st = state(cx);
    st.update(cx, |s, cx| {
        let next = edge.map(|e| (clip_id, e));
        if s.playlist_state.trim_hover != next {
            s.playlist_state.trim_hover = next;
            cx.notify();
        }
    });
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

/// Punto de entrada compartido del zoom con rueda (`Ctrl`/`Alt` + wheel).
///
/// Lo usan tanto la grilla temporal como la columna fija de headers (ambos
/// están bajo el cursor en distintas zonas): traduce el evento a
/// `apply_playlist_wheel_zoom` y frena la propagación sólo si se consumió
/// (si no, la rueda sigue scrolleando normal).
fn playlist_wheel_zoomed(
    delta: ScrollDelta,
    control: bool,
    alt: bool,
    shift: bool,
    cursor_win: (f32, f32),
    total_ticks: u64,
    header_width: f32,
    ruler_h: f32,
    row_count: usize,
    cx: &mut App,
) {
    let st = state(cx);
    let mut consumed = false;
    st.update(cx, |s, cx| {
        consumed = apply_playlist_wheel_zoom(
            s,
            &delta,
            control,
            alt,
            shift,
            cursor_win,
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
    // El clipboard de la playlist habilita/deshabilita "Pegar" en el menú.
    let clipboard_empty = pl.clipboard.is_empty();
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
    // Pan con botón central en curso (cursor de puño cerrado mientras dura).
    let playlist_panning = app.playlist_pan.is_some();
    // Gesto de mezcla en curso (compartido con la Session Matrix: mismo tag
    // global, así el drag no se corta al re-render de los headers).
    let mix_drag = app.matrix_mix_drag;

    // Compás real según SIG (BPM sólo afecta a segundos, no a ticks).
    let ticks_per_bar_val = ticks_per_bar(ppqn, beats_per_bar);
    let display_tick = loop_display_tick(playhead_tick, loop_start, loop_end, is_looping && loop_active);

    let non_master: Vec<(usize, &Track)> = tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.is_master)
        .collect();

    // Snapshot de mezcla por fila (nombre, M, S, vol, pan) para los headers:
    // se copia acá porque el render no puede mantener el borrow de `app`
    // mientras arma la UI.
    let rows_mix: Vec<(usize, String, bool, bool, f32, f32)> = non_master
        .iter()
        .map(|(idx, _)| {
            let (name, muted, soloed, volume, pan) = playlist_row_mix(&app, *idx);
            (*idx, name, muted, soloed, volume, pan)
        })
        .collect();

    let track_height = clamped_row_h(pl.row_h);
    let ruler_h = RULER_H;
    // Geometría del lienzo en DOS columnas (headers fijos + grilla temporal):
    // la columna izquierda (`header_width`) viaja SÓLO en vertical junto a
    // las filas, y el contenido temporal (regla, líneas, clips, playhead)
    // vive en su propio panel derecho con scroll horizontal independiente.
    // Dentro de ese panel el compás 1 arranca en x=0: NADA suma ya el ancho
    // de headers (`grid_ox` anterior) a las coords temporales.
    // Altura del lienzo: N filas × altura actual (zoom vertical).
    let row_count = non_master.len().max(1);
    let canvas_h = ruler_h + row_count as f32 * track_height;
    let total_ticks = (ticks_per_bar_val * 128).max(playhead_tick + ticks_per_bar_val * 16);
    let canvas_w = (header_width + total_ticks as f32 * zoom_x).max(800.0);
    let grid_w = (canvas_w - header_width).max(1.0);

    // Cabeceras de fila con la MISCA caja de mezcla del Session Matrix
    // (nombre + M/S + knob de pan + fader de volumen con dB). El id de
    // control sigue siendo `pl_row_{idx}` (lo usan los tests de click) y el
    // apply de cada gesto escribe en la pista del modo activo espejando
    // mixer/matriz/motor, así queda bidireccional.
    let mut track_headers: Vec<AnyElement> = Vec::new();
    for (row, name, muted, soloed, volume, pan) in &rows_mix {
        let vec_idx = *row;
        // Resaltado de la fila seleccionada: se pasa por el borde del header
        // comparando contra el estado leído en el snapshot del render.
        let selected_border = selected_track == vec_idx;
        track_headers.push(
            playlist_track_header_with_selected(
                *row,
                vec_idx,
                name.clone(),
                *muted,
                *soloed,
                *volume,
                *pan,
                mix_drag,
                selected_border,
                track_height,
            )
            .into_any_element(),
        );
    }

    // Bounds de la zona de grilla en coords de ventana (patrón `record_bounds`
    // del arranger): `mouse_position()` viene en coords de ventana, así que el
    // mapeo px→ticks/fila resta el origen real medido cada frame, no una
    // constante. La zona ES la grilla (el panel derecho, sin headers).
    // Se crea acá arriba porque los handlers de drag de los clips (definidos
    // abajo) también lo necesitan para traducir el cursor a ticks/fila.
    let zone_bounds: Rc<Cell<[f32; 4]>> = Rc::new(Cell::new([0.0; 4]));
    let zone_bounds_clip = zone_bounds.clone();
    let zone_bounds_move = zone_bounds.clone();
    let zone_bounds_drop = zone_bounds.clone();
    let zone_bounds_rec = zone_bounds.clone();

    let mut clip_elems: Vec<AnyElement> = Vec::new();
    for (track_id, clip) in &pl.clips {
        let is_sel = selected.contains(&clip.id);
        let clip_name = clip.name.clone();
        let clip_start = clip.start_tick;
        let clip_dur = clip.duration_ticks;
        let clip_x = ticks_to_px(clip_start, zoom_x);
        let clip_w = ticks_to_px(clip_dur, zoom_x).max(12.0);
        let track_row = non_master.iter().position(|(i, _)| *i == *track_id).unwrap_or(0);
        let clip_y = ruler_h + track_row as f32 * track_height + 1.0;
        let clip_h = track_height - 2.0;
        let clip_id = clip.id;
        let clip_x_for_edge = clip_x;
        let clip_w_for_edge = clip_w;
        // Cursor/estado de recorte: el handle activo usa el cursor de resize.
        let trim_state = pl.trim_drag.filter(|d| d.clip_id == clip_id);
        let hover_edge = pl.trim_hover.filter(|(id, _)| *id == clip_id).map(|(_, e)| e);
        let edge_cursor = match trim_state {
            Some(d) => Some(d.edge.cursor()),
            None => hover_edge.map(|e| e.cursor()),
        };

        // --- ClipView: bloque horizontal con título + contenido ---------------
        // Posición dinámica: x = start_tick × zoom (origen del panel
        // temporal, a la derecha de los headers fijos), w = duration × zoom,
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
        let zone_move = zone_bounds_move.clone();
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
            .cursor(if let Some(c) = edge_cursor {
                c
            } else if dragging_this {
                CursorStyle::ClosedHand
            } else {
                CursorStyle::Arrow
            })
            .on_mouse_down(gpui_kit::MouseButton::Left, move |event, _, cx| {
                let zb = zone_down.get();
                let cursor_x = event.position.x.as_f32();
                // Los bordes recortan; el resto del clip mueve. Se decide por
                // posición del cursor, no por hijo: los handles son decorativos
                // (sólo cursor) y así la zona de agarre no depende del layout.
                let clip_left = zb[0] + clip_x_for_edge;
                let clip_right = clip_left + clip_w_for_edge;
                let hit = trim_edge_at(cursor_x, clip_left, clip_right - clip_left, TRIM_EDGE_PX);
                let st = state(cx);
                if let Some(edge) = hit {
                    let edge_x = match edge {
                        TrimEdge::Left => clip_left,
                        TrimEdge::Right => clip_right,
                    };
                    start_clip_trim(cx, clip_id, edge, cursor_x, edge_x);
                    return;
                }
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
                        grab_dx_px: cursor_x - (zb[0] + ticks_to_px(start, zoom)),
                        grab_dy_px: event.position.y.as_f32()
                            - (zb[1] + row as f32 * row_h + 1.0),
                        moved: false,
                    });
                    // El clip agarrado queda seleccionado (borde amarillo que
                    // lo sigue en vivo = feedback del arrastre) y el Clip
                    // Editor se carga con ese clip.
                    s.playlist_state.selected_clips = vec![clip_id];
                    crate::views::clip_editor::select_target(
                        s,
                        Some(ClipEditorTarget::Playlist { clip_id }),
                    );
                    s.playlist_state.clip_click_suppress = false;
                    cx.notify();
                });
            })
            // Cursor de recorte sobre los bordes: `on_mouse_move` sin botón
            // cambia el cursor a e-resize/w-resize según el borde más cercano.
            .on_mouse_move(move |event, _, cx| {
                if state(cx).read(cx).playlist_state.trim_drag.is_some() {
                    return;
                }
                if event.pressed_button == Some(gpui_kit::MouseButton::Left) {
                    return;
                }
                let zb = zone_move.get();
                let hit = trim_edge_at(
                    event.position.x.as_f32(),
                    zb[0] + clip_x_for_edge,
                    clip_w_for_edge,
                    TRIM_EDGE_PX,
                );
                set_clip_edge_cursor(cx, clip_id, hit);
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
                    // El Clip Editor sigue a la selección de la Playlist.
                    if let Some(id) = state.playlist_state.selected_clips.first().copied() {
                        crate::views::clip_editor::select_target(
                            state,
                            Some(ClipEditorTarget::Playlist { clip_id: id }),
                        );
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
            // Handles de recorte en los dos bordes (encima del waveform).
            let clip_el = clip_el
                .child(clip_edge_handle(
                    format!("pl_trim_left_{}", clip_id),
                    TrimEdge::Left,
                    hover_edge == Some(TrimEdge::Left)
                        || trim_state.map(|d| d.edge) == Some(TrimEdge::Left),
                ))
                .child(clip_edge_handle(
                    format!("pl_trim_right_{}", clip_id),
                    TrimEdge::Right,
                    hover_edge == Some(TrimEdge::Right)
                        || trim_state.map(|d| d.edge) == Some(TrimEdge::Right),
                ));
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
            // Handles de recorte en los dos bordes (encima del waveform).
            let clip_el = clip_el
                .child(clip_edge_handle(
                    format!("pl_trim_left_{}", clip_id),
                    TrimEdge::Left,
                    hover_edge == Some(TrimEdge::Left)
                        || trim_state.map(|d| d.edge) == Some(TrimEdge::Left),
                ))
                .child(clip_edge_handle(
                    format!("pl_trim_right_{}", clip_id),
                    TrimEdge::Right,
                    hover_edge == Some(TrimEdge::Right)
                        || trim_state.map(|d| d.edge) == Some(TrimEdge::Right),
                ));
            clip_elems.push(clip_el.into_any_element());
        }
    }

    let playhead_x = ticks_to_px(display_tick, zoom_x);
    // Grilla según quantize actual (1/4, 1/8, 1/16): subdivisión de la redonda.
    // Las líneas principales caen cada compás SIG (`ticks_per_bar_val`).
    // Ticks del snap actual, una vez por frame (la usa el drop y el marquee).
    let snap_ticks_val = snap_step_ticks(ppqn, grid_den).max(1);
    let mut grid_lines: Vec<AnyElement> = Vec::new();
    let mut step = 0u64;
    let mut bar_num = 1u32;
    while step <= total_ticks {
        let x = ticks_to_px(step, zoom_x);
        if x > grid_w {
            break;
        }
        // El compás 1 (x=0) es el origen del panel temporal: los headers
        // fijos viven en su propia columna y nunca quedan debajo.
        let gx = x;
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
        step += snap_ticks_val;
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
                .left(px(10.0))
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
                .left(px(0.0))
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
                            let drop_tick = snap_ticks(raw, snap_ticks_val);
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

    // Geometría de cada clip en px de VENTANA (para el marquee, que se dibuja
    // en el mismo espacio): `(track_key, clip_id, x, y, w, h)`. Se calcula una
    // sola vez por frame y se la lleva el cierre del gesto.
    let clip_rects: Vec<(usize, usize, f32, f32, f32, f32)> = pl
        .clips
        .iter()
        .map(|(track_key, clip)| {
            let track_row = non_master
                .iter()
                .position(|(i, _)| *i == *track_key)
                .unwrap_or(0);
            (
                *track_key,
                clip.id,
                ticks_to_px(clip.start_tick, zoom_x),
                ruler_h + track_row as f32 * track_height + 1.0,
                ticks_to_px(clip.duration_ticks, zoom_x).max(12.0),
                track_height - 2.0,
            )
        })
        .collect();

    // Capa más alta del lienzo: el rectángulo de marquee. Se registra al FINAL
    // de los hijos del panel temporal (después de clips, seek y playhead) para
    // que quede por encima de todo lo demás al pintarse.
    let marquee_overlay = match pl.marquee {
        Some(m) if m.is_drag() => {
            let (x0, y0, x1, y1) = m.normalized();
            // El estado ya está en coords LOCALES de la grilla (se convierten
            // en el handler del mouse, donde los bounds medidos ya están
            // frescos), así que acá el rect se posiciona directo: sin restar
            // offsets a mano y sin depender de un `Cell` leído en el render.
            Some(
                div()
                    .id("pl_marquee_overlay")
                    .test_support()
                    .absolute()
                    .left(px(x0))
                    .top(px(y0))
                    .w(px((x1 - x0).max(1.0)))
                    .h(px((y1 - y0).max(1.0)))
                    .bg(rgba(0x0096BE59))
                    .border_1()
                    .border_color(rgb(0x00B4E4))
                    .rounded(px(1.0))
                    .into_any_element(),
            )
        }
        _ => None,
    };

    // Delegador del gesto a ventana completa: mientras la caja está viva, este
    // overlay captura los `mouse_move`/`mouse_up` aunque el cursor pase por
    // encima de un clip o salga del panel temporal hacia los headers. Así la
    // selección la maneja SIEMPRE la caja global y nunca un clip individual
    // (que además tiene su propio drag de botón izquierdo). Se monta más abajo,
    // en `all`, cuando ya existe el vector.
    // El catcher global necesita copias propias de los closures (dos handlers
    // de mouse_up) y del rect de clips.
    let catcher_zone_right = zone_bounds.clone();
    let catcher_zone_left = zone_bounds.clone();
    let catcher_zone_move = zone_bounds.clone();
    let catcher_rects_right: Rc<Vec<(usize, usize, f32, f32, f32, f32)>> = Rc::new(clip_rects.clone());
    let catcher_rects_left = catcher_rects_right.clone();
    let clip_rects_marquee_rc: Rc<Vec<(usize, usize, f32, f32, f32, f32)>> =
        Rc::new(clip_rects);

    let zone_bounds_seek = zone_bounds.clone();
    let zone_grid_move = zone_bounds.clone();
    let zone_bounds_marquee = zone_bounds.clone();
    let zone_bounds_marquee_up = zone_bounds_marquee.clone();
    let seek_zone = div()
        .absolute()
        .left(px(0.0))
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

    // Fondo de la regla: cubre el panel temporal (la esquina sobre los
    // headers la pinta la columna fija, no este contenido scrolleable).
    let children = vec![
        div()
            .absolute()
            .left(px(0.0))
            .top(px(0.0))
            .w(px(grid_w))
            .h(px(ruler_h))
            .bg(rgb(0x18181E))
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
        .left(px(0.0))
        .top(px(ruler_h))
        .w(px(1.0))
        .h(px(canvas_h - ruler_h))
        .into_any_element(),
    ];

    // Playhead y bordes de loop en coords del panel temporal (origen x=0):
    // el canvas los pinta relativo a su propio origen, así siguen alineados
    // con compases y clips haya scroll o no.
    let loop_win = loop_render;
    let playhead_canvas = canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            // Playhead: marcador de posición de reproducción actual.
            let x = bounds.origin.x + px(playhead_x);
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
    .w(px(grid_w))
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
    // Esquina superior-izquierda fija (sobre la columna de headers).
    let corner = div()
        .w_full()
        .h(px(ruler_h))
        .bg(rgb(0x141418))
        .border_r_1()
        .border_color(rgb(0x2D2D37))
        .into_any_element();
    // Navegación: scroll vertical externo + horizontal interno (mismo patrón
    // anidado que el arranger). La fila interna tiene DOS columnas: headers
    // fijos a la izquierda y panel temporal a la derecha. El scroll vertical
    // mueve la fila entera (sincronía gratis entre headers y pistas) y el
    // horizontal vive SÓLO en el panel derecho, así los headers nunca salen
    // de vista en X. El panel derecho recorta (`overflow_x_scroll`) TODO lo
    // temporal: clips y regla mueren en su borde, sin z-index ni overlaps
    // sobre la columna fija.
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
                            .flex()
                            .flex_row()
                            .w_full()
                            .child(
                                // Columna FIJA de headers: fuera del scroll
                                // horizontal, una cabecera de `track_height`
                                // por fila. Comparte el zoom de rueda de la
                                // grilla para que `Ctrl`/`Alt` + rueda
                                // funcionen también con el cursor acá.
                                v_flex()
                                    .flex_none()
                                    .w(px(header_width))
                                    .id("playlist_headers")
                                    .on_scroll_wheel(move |event, _, cx| {
                                        playlist_wheel_zoomed(
                                            event.delta,
                                            event.modifiers.control
                                                || event.modifiers.platform,
                                            event.modifiers.alt,
                                            event.modifiers.shift,
                                            (
                                                event.position.x.as_f32(),
                                                event.position.y.as_f32(),
                                            ),
                                            total_ticks,
                                            header_width,
                                            ruler_h,
                                            row_count,
                                            cx,
                                        );
                                    })
                                    .child(corner)
                                            // Menú contextual de la Playlist: se
                                            // abre con click DERECHO simple (sin
                                            // arrastre). El marquee consume el
                                            // gesto largo en `mouse_up`, así que
                                            // acá sólo llegan los toques.
                                            .context_menu(move |menu: PopupMenu, _window, _cx| {
                                                let has_sel = !selected.is_empty();
                                                let has_clip = !clipboard_empty;
                                                menu.item(
                                                    PopupMenuItem::new("Copiar")
                                                        .disabled(!has_sel)
                                                        .on_click(move |_, _, cx| {
                                                            let st = state(cx);
                                                            st.update(cx, |s, cx| {
                                                                copy_selected_clips(
                                                                    &mut s.playlist_state,
                                                                );
                                                                cx.notify();
                                                            });
                                                        }),
                                                )
                                                .item(
                                                    PopupMenuItem::new("Cortar")
                                                        .disabled(!has_sel)
                                                        .on_click(move |_, _, cx| {
                                                            let st = state(cx);
                                                            st.update(cx, |s, cx| {
                                                                if cut_selected_clips(
                                                                    &mut s.playlist_state,
                                                                ) > 0
                                                                {
                                                                    sync_clips_to_engine(s, cx);
                                                                }
                                                                cx.notify();
                                                            });
                                                        }),
                                                )
                                                .item(
                                                    PopupMenuItem::new("Pegar")
                                                        .disabled(!has_clip)
                                                        .on_click(move |_, _, cx| {
                                                            let st = state(cx);
                                                            st.update(cx, |s, cx| {
                                                                let grid =
                                                                    snap_step_ticks(
                                                                        s.playlist_state.ppqn,
                                                                        s.playlist_state
                                                                            .grid_denominator,
                                                                    )
                                                                    .max(1);
                                                                let at =
                                                                    s.playlist_state.paste_tick();
                                                                let keys = playlist_row_keys(s);
                                                                let ids = paste_clips(
                                                                    &mut s.playlist_state,
                                                                    at,
                                                                    grid,
                                                                    &keys,
                                                                );
                                                                if !ids.is_empty() {
                                                                    sync_clips_to_engine(s, cx);
                                                                }
                                                                cx.notify();
                                                            });
                                                        }),
                                                )
                                                .item(
                                                    PopupMenuItem::new("Duplicar")
                                                        .disabled(!has_sel)
                                                        .on_click(move |_, _, cx| {
                                                            let st = state(cx);
                                                            st.update(cx, |s, cx| {
                                                                let grid =
                                                                    snap_step_ticks(
                                                                        s.playlist_state.ppqn,
                                                                        s.playlist_state
                                                                            .grid_denominator,
                                                                    )
                                                                    .max(1);
                                                                let ids = duplicate_selected_clips(
                                                                    &mut s.playlist_state,
                                                                    grid,
                                                                );
                                                                if !ids.is_empty() {
                                                                    sync_clips_to_engine(s, cx);
                                                                }
                                                                cx.notify();
                                                            });
                                                        }),
                                                )
                                                .item(PopupMenuItem::new("Deseleccionar").on_click(
                                                    move |_, _, cx| {
                                                        let st = state(cx);
                                                        st.update(cx, |s, cx| {
                                                            s.playlist_state
                                                                .selected_clips
                                                                .clear();
                                                            cx.notify();
                                                        });
                                                    },
                                                ))
                                            })
                                            .children(track_headers),
                                    )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
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
                                                    .test_support()
                                                    .w(px(grid_w))
                                                    .h(px(canvas_h))
                                                    .relative()
                                                    // Puño cerrado mientras se
                                                    // navega con el botón
                                                    // central (GPUI 0.3.x no
                                                    // tiene `Grabbing`: es el
                                                    // equivalente CSS).
                                                    .cursor(if playlist_panning {
                                                        CursorStyle::ClosedHand
                                                    } else {
                                                        CursorStyle::Arrow
                                                    })
                                            // Move/Up del drag de clips a nivel de grilla (no
                                            // del clip): el cursor sale del clip al arrastrar
                                            // y los handlers del propio clip dejarían de
                                            // disparar. Acá cubren toda la zona.
                                            .on_mouse_move(move |event, _, cx| {
                                                // Un gesto de recorte tiene
                                                // prioridad: el borde arrastrado
                                                // manda sobre el drag de clip.
                                                if state(cx).read(cx).playlist_state.trim_drag.is_some() {
                                                    let zb = zone_grid_move.get();
                                                    update_clip_trim(
                                                        cx,
                                                        event.position.x.as_f32(),
                                                        zb,
                                                    );
                                                    return;
                                                }
                                                // Botón central: el pan manda sobre
                                                // cualquier otro gesto.
                                                if event.pressed_button
                                                    == Some(gpui_kit::MouseButton::Middle)
                                                {
                                                    if is_playlist_panning(cx) {
                                                        update_playlist_pan(cx, event.position);
                                                    }
                                                    return;
                                                }
                                                if event.pressed_button
                                                    != Some(gpui_kit::MouseButton::Left)
                                                {
                                                    // Botón soltado fuera: se consolida.
                                                    finish_clip_drag(cx);
                                                    finish_clip_trim(cx);
                                                    stop_playlist_pan(cx);
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
                                                move |_, _, cx| {
                                                    finish_clip_drag(cx);
                                                    finish_clip_trim(cx);
                                                },
                                            )
                                            // Navegación con botón central
                                            // (ruedita): MMB + arrastrar mueve
                                            // el viewport en ambos ejes. El
                                            // cursor pasa a puño cerrado
                                            // mientras dura el gesto (el
                                            // `cursor` se lee en el render).
                                            .on_mouse_down(
                                                gpui_kit::MouseButton::Middle,
                                                move |event, _, cx| {
                                                    start_playlist_pan(cx, event.position);
                                                },
                                            )
                                            .on_mouse_up(
                                                gpui_kit::MouseButton::Middle,
                                                move |_, _, cx| stop_playlist_pan(cx),
                                            )
                                            // Marquee: click DERECHO + arrastrar dibuja
                                            // el rectángulo de selección. El
                                            // `mousemove`/`mouseup` viven en la
                                            // grilla (no en el clip) porque el
                                            // cursor sale del control al arrastrar.
                                            // Sólo se registra el gesto; la
                                            // resolución (seleccionar vs abrir
                                            // menú) ocurre en el `mouseup`.
                                            .on_mouse_down(
                                                gpui_kit::MouseButton::Right,
                                                move |event, _, cx| {
                                                    // El ancla se fija en coords
                                                    // LOCALES de la grilla: se
                                                    // resta el origen medido del
                                                    // panel (y no el de la zona
                                                    // de seek, que está +
                                                    // `ruler_h` más abajo).
                                                    let (gx, gy) =
                                                        grid_origin_from_zone(
                                                            zone_bounds_marquee.get(),
                                                            ruler_h,
                                                        );
                                                    let st = state(cx);
                                                    st.update(cx, |s, cx| {
                                                        let px =
                                                            event.position.x.as_f32() - gx;
                                                        let py =
                                                            event.position.y.as_f32() - gy;
                                                        s.playlist_state.marquee = Some(MarqueeState {
                                                            start_x: px,
                                                            start_y: py,
                                                            cur_x: px,
                                                            cur_y: py,
                                                        });
                                                        cx.notify();
                                                    });
                                                },
                                            )
                                            .on_mouse_move(move |event, _, cx| {
                                                // Durante el marquee manda el
                                                // catcher global: el clip no
                                                // debe interceptar el gesto.
                                                if state(cx).read(cx).playlist_state.marquee.is_some() {
                                                    return;
                                                }
                                                let st = state(cx);
                                                st.update(cx, |s, cx| {
                                                    if s.playlist_state.marquee.is_none() {
                                                        return;
                                                    }
                                                    // Botón soltado fuera: el
                                                    // `mouse_up` no llega, así que
                                                    // se cierra el gesto acá.
                                                    if event.pressed_button.is_none() {
                                                        s.playlist_state.marquee = None;
                                                        cx.notify();
                                                    }
                                                });
                                            })
                                            .on_mouse_up(
                                                gpui_kit::MouseButton::Right,
                                                move |event, _, cx| {
                                                    finish_marquee(
                                                        (
                                                            event.position.x.as_f32(),
                                                            event.position.y.as_f32(),
                                                        ),
                                                        zone_bounds_marquee_up.get(),
                                                        ruler_h,
                                                        &clip_rects_marquee_rc.as_ref(),
                                                        event.modifiers.shift,
                                                        cx,
                                                    );
                                                },
                                            )
                                            // Zoom con rueda + modificadores, anclado al cursor:
                                            // `Ctrl` + rueda = horizontal, `Alt` + rueda (o
                                            // `Ctrl` + `Shift` + rueda) = altura de filas. Sin
                                            // modificadores se deja pasar (scroll normal).
                                            .on_scroll_wheel(move |event, _, cx| {
                                                playlist_wheel_zoomed(
                                                    event.delta,
                                                    event.modifiers.control
                                                        || event.modifiers.platform,
                                                    event.modifiers.alt,
                                                    event.modifiers.shift,
                                                    (
                                                        event.position.x.as_f32(),
                                                        event.position.y.as_f32(),
                                                    ),
                                                    total_ticks,
                                                    header_width,
                                                    ruler_h,
                                                    row_count,
                                                    cx,
                                                );
                                            })
                                    .children(children)
                                    // Orden de capas (z-ordering):
                                    //   1. Fondo + líneas verticales de grilla (detrás).
                                    //   2. Clips de audio/MIDI con fondo opaco (tapan la grilla).
                                    //   3. Overlays de drop/seek + playhead (encima de todo).
                                    // Antes `grid_lines` iba DESPUÉS de `clip_elems` y las
                                    // líneas atravesaban la forma de onda del clip.
                                    .children(grid_lines)
                                    .children(clip_elems)
                                    // Marquee por encima de los clips (es el
                                    // feedback del gesto en curso).
                                    .children(drop_zones)
                                    .when_some(drop_handler, |v, h| v.child(h))
                                    .child(seek_zone.into_any_element())
                                    .child(playhead_canvas.into_any_element())
                                    // El overlay del marquee va AL FINAL: al
                                    // pintarse de atrás hacia adelante, queda
                                    // por encima de clips, grilla y playhead.
                                    .when_some(marquee_overlay, |v, h| v.child(h)),
                                )
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

    // Capturador de drag de mezcla a ventana completa: mientras haya un gesto
    // de volumen/pan vivo en los headers, este overlay recibe TODOS los
    // mouse_move/mouse_up aunque el cursor salga del control (mismo patrón que
    // `matrix_mix_drag_catcher`). Se monta sólo durante el gesto y lee el
    // tag global compartido, así sirve igual para Matrix y Playlist.
    // Delegador del pan a ventana completa: mientras el botón central está
    // presionado, este overlay recibe TODOS los mouse_move aunque el cursor
    // salga del panel (o del clip que lo empezó). Es lo que hace el gesto
    // continuo en vez de cortarse al cruzar el borde. Se desmonta al soltar.
    if playlist_panning {
        all.push(
            div()
                .id("playlist_pan_catcher")
                .test_support()
                .absolute()
                .inset_0()
                .cursor_grabbing()
                .on_mouse_move(move |event, _, cx| {
                    // Botón soltado fuera de la ventana: el `mouse_up` no
                    // llega, así que se cierra el gesto acá.
                    if event.pressed_button != Some(gpui_kit::MouseButton::Middle) {
                        stop_playlist_pan(cx);
                        return;
                    }
                    if is_playlist_panning(cx) {
                        update_playlist_pan(cx, event.position);
                    }
                })
                .on_mouse_up(gpui_kit::MouseButton::Middle, move |_, _, cx| {
                    stop_playlist_pan(cx);
                })
                .into_any_element(),
        );
    }

    // Delegador del marquee a ventana completa: captura el gesto aunque el
    // cursor esté sobre un clip o fuera del panel temporal, así la caja global
    // manda y ningún control la intercepta.
    if pl.marquee.is_some() {
        all.push(
            div()
                .id("playlist_marquee_catcher")
                .test_support()
                .absolute()
                .inset_0()
                .on_mouse_move(move |event, _, cx| {
                    let zone = catcher_zone_move.get();
                    update_marquee(
                        event.position.x.as_f32(),
                        event.position.y.as_f32(),
                        zone,
                        ruler_h,
                        cx,
                    );
                })
                .on_mouse_up(gpui_kit::MouseButton::Right, move |event, _, cx| {
                    let zone = catcher_zone_right.get();
                    let rects = catcher_rects_right.clone();
                    finish_marquee(
                        (event.position.x.as_f32(), event.position.y.as_f32()),
                        zone,
                        ruler_h,
                        &rects,
                        event.modifiers.shift,
                        cx,
                    );
                })
                .on_mouse_up(gpui_kit::MouseButton::Left, move |event, _, cx| {
                    let zone = catcher_zone_left.get();
                    let rects = catcher_rects_left.clone();
                    finish_marquee(
                        (event.position.x.as_f32(), event.position.y.as_f32()),
                        zone,
                        ruler_h,
                        &rects,
                        event.modifiers.shift,
                        cx,
                    );
                })
                .into_any_element(),
        );
    }

    if mix_drag.is_some() {
        all.push(
            div()
                .absolute()
                .inset_0()
                .id("playlist_mix_drag_catcher")
                .cursor_grabbing()
                .on_mouse_move(move |event, _, cx| {
                    if event.pressed_button != Some(gpui_kit::MouseButton::Left) {
                        end_mix_drag(cx);
                        return;
                    }
                    let drag = state(cx).read(cx).matrix_mix_drag;
                    match drag {
                        Some(MatrixMixTarget::Volume(idx)) => step_mix_slider_gesture(
                            cx,
                            idx,
                            event.position.x.as_f32(),
                            apply_playlist_volume,
                        ),
                        Some(MatrixMixTarget::Pan(idx)) => step_mix_pan_gesture(
                            cx,
                            idx,
                            event.position.y.as_f32(),
                            apply_playlist_pan,
                        ),
                        None => {}
                    }
                })
                .on_mouse_up(gpui_kit::MouseButton::Left, move |_, _, cx| {
                    end_mix_drag(cx);
                })
                .into_any_element(),
        );
    }

    v_flex()
        .id("playlist")
        .test_support()
        .relative()
        .size_full()
        .gap(px(4.0))
        .children(all)
        .into_any_element()
}
