// crates/hikaru_gui/src/views/clip_editor.rs
//
// Clip Editor: visualizador y editor detallado del clip AUDIO seleccionado
// (Session Matrix o Playlist/Timeline). Port del panel legacy de egui al
// widget kit.
//
// Estructura del panel (de izquierda a derecha):
//   1. SIDEBAR de propiedades: nombre del archivo, pestañas (Audio Events /
//      Comping / Stretch / Onsets) y los controles de procesado de esa pestaña
//      (Gain, Pan, Pitch, Formant, Stretch, Onsets) + auto-fades.
//   2. VISUALIZADOR: regla de compases sincronizada al INICIO del clip,
//      waveform con zoom y scroll propios, y la región de loop encima.
//
// La selección es UNIFICADA (`ClipEditorState::target`): el mismo clip se ve
// tanto si viene de un pad de la Session Matrix como de la Playlist, y se
// resuelve contra la fuente que corresponda. Sin selección se muestra el
// estado vacío informativo.

use std::path::PathBuf;

use gpui_kit::component::*;
use gpui_kit::component::button::{Button, ButtonRounded};
use gpui_kit::component::label::Label;
use gpui_kit::prelude::{
    FluentBuilder as _, InteractiveElement as _, StatefulInteractiveElement as _, Styled as _,
};
use gpui_kit::*;

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
///
/// Las dos vistas (Session Matrix y Playlist) alimentan el MISMO editor: sólo
/// cambia dónde se busca el buffer de audio y los picos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipEditorTarget {
    /// Pad `(track, scene)` de la Session Matrix.
    Matrix { track: usize, scene: usize },
    /// Clip de la Playlist/Timeline.
    Playlist { clip_id: usize },
}

impl ClipEditorTarget {
    /// Texto corto para la barra de estado.
    pub fn label(&self) -> String {
        match self {
            ClipEditorTarget::Matrix { track, scene } => {
                format!("Track {} | Scene {}", track + 1, scene + 1)
            }
            ClipEditorTarget::Playlist { clip_id } => format!("Playlist clip #{}", clip_id),
        }
    }
}

/// Pestañas del sidebar (modos de edición del clip).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorTab {
    AudioEvents,
    Comping,
    Stretch,
    Onsets,
}

impl EditorTab {
    pub fn label(&self) -> &'static str {
        match self {
            EditorTab::AudioEvents => "Audio Events",
            EditorTab::Comping => "Comping",
            EditorTab::Stretch => "Stretch",
            EditorTab::Onsets => "Onsets",
        }
    }
}

/// Controles de procesado del clip, en dB / cents / % según el caso.
///
/// Son valores de GUI (los controles existen y son editables); el motor todavía
/// no los aplica al audio, así que se guardan acá para que el port no los
/// pierda y para que la UI sea utilizable.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipProcessParams {
    pub gain_db: f32,
    pub pan: f32,
    pub pitch_cents: f32,
    pub formant: f32,
    pub stretch: f32,
    /// Umbral de detección de onsets en dBFS.
    pub onset_threshold_db: f32,
    /// Genera fades automáticamente al recortar bordes.
    pub auto_fades: bool,
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
        }
    }
}

/// Estado completo del Clip Editor (global: el panel se reconstruye cada frame).
#[derive(Clone, Debug)]
pub struct ClipEditorState {
    /// Clip en edición. `None` ⇒ estado vacío.
    pub target: Option<ClipEditorTarget>,
    /// Pestaña activa del sidebar.
    pub tab: EditorTab,
    /// Zoom horizontal del visualizador (px por segundo). Independiente del
    /// zoom de la Playlist: `1.0` es "encajar el clip".
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
}

impl Default for ClipEditorState {
    fn default() -> Self {
        Self {
            target: None,
            tab: EditorTab::AudioEvents,
            // Zoom por DEFECTO al abrir el editor. 1 px/render era ilegible:
            // un clip de 2s medía 2 píxeles y la waveform no se veía. 400
            // px/s deja un clip de 2s en 800px, que entra de sobra en el
            // panel y se lee con claridad.
            zoom_x: DEFAULT_CE_ZOOM,
            scroll_secs: 0.0,
            params: ClipProcessParams::default(),
            snap: SnapValue::Beat1_4,
            loop_start_secs: 0.0,
            loop_end_secs: 0.0,
            loop_enabled: false,
            show_editor: false,
            // Alto por defecto del panel plegable. Debe ser MODESTO: la
            // vista principal (matriz / playlist) tiene que seguir entrando
            // sin scroll vertical con 8 pistas.
            editor_height: 150.0,
        }
    }
}

/// Alto mínimo del lienzo del waveform (px).
///
/// El lienzo NO puede depender de `h_full()`: dentro del `v_flex` del panel el
/// padre no tiene un alto resuelto que propagar, así que el canvas quedaba en
/// 0px de alto (panel negro). Se le da un mínimo explícito y `flex_1` lo hace
/// crecer con el panel.
pub const WAVE_MIN_H: f32 = 72.0;

/// Zoom por defecto del visualizador (px por segundo).
///
/// A 1 px/s un clip de 2s ocupa 2 píxeles: la waveform es invisible. 400 px/s
/// muestra un clip de 2s a lo ancho del panel.
pub const DEFAULT_CE_ZOOM: f32 = 400.0;

/// Límites del zoom del visualizador (px por segundo).
pub const MIN_CE_ZOOM: f32 = 0.25;
pub const MAX_CE_ZOOM: f32 = 400.0;

/// Zoom acotado. Función pura para testear.
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

/// Segundos visibles en el viewport al zoom actual. Función pura.
pub fn visible_secs(zoom_x: f32, viewport_px: f32) -> f64 {
    if zoom_x <= 0.0 {
        return 0.0;
    }
    viewport_px as f64 / zoom_x as f64
}

/// Desplazamiento vertical scrollable acotado al contenido real.
///
/// El scroll horizontal del visualizador no puede pasar de `[0, fin - visible]`:
/// fuera de ese rango el waveform queda en blanco.
pub fn clamp_scroll(scroll_secs: f64, duration_secs: f64, visible: f64) -> f64 {
    let max = (duration_secs - visible).max(0.0);
    if !scroll_secs.is_finite() {
        return 0.0;
    }
    scroll_secs.clamp(0.0, max)
}

/// Posiciones de la regla de compases (en segundos desde el inicio del clip) para
/// los compases visibles.
///
/// `clip_start_secs` es el offset temporal del clip en la línea: la regla es
/// LOCAL al clip pero arranca en el compás que le corresponde en el proyecto.
/// Función pura para testear.
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
    // Un compás de margen antes y después para que la regla no "entre" justo
    // en el borde del viewport mientras se scrollea.
    while t <= clip_start_secs + duration_secs + sec_per_bar {
        if t >= clip_start_secs - sec_per_bar {
            out.push(t);
        }
        t += sec_per_bar;
    }
    out
}

/// Índice de compás 1-based de un instante absoluto. Función pura.
pub fn bar_number_at(secs: f64, bpm: f32) -> u32 {
    let bpm = if bpm > 0.0 { bpm as f64 } else { 120.0 };
    let sec_per_bar = (60.0 / bpm) * 4.0;
    if !sec_per_bar.is_finite() || sec_per_bar <= 0.0 {
        return 1;
    }
    if secs < 0.0 || sec_per_bar <= 0.0 {
        return 1;
    }
    ((secs / sec_per_bar).floor() as u32).saturating_add(1)
}

/// ¿El buffer de picos es inservible (vacío o todo silencio)?
///
/// `load_peaks_from_wav` devuelve `vec![0.0; bins]` — NUESTOS ceros, no vacío
/// — cuando no puede abrir el archivo. Como son 2048 bins, "más resolución" ganaba
/// siempre y el editor acababa dibujando una línea plana: el waveform del pad
/// (que usa `clip.peaks`, con datos reales) se veía y el del editor no.
/// Altura (px) del ultimo lienzo de waveform que se pinto, o 0 si nunca se
/// pinto. Sonda de diagnostico: el `canvas` de gpui no tiene tamaño propio, asi
/// que un alto de 0 significa que el closure de pintado se esta abortando y el
/// panel queda negro.
pub static LAST_PAINT_H: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Registra la altura del lienzo pintado (ver `LAST_PAINT_H`).
pub fn note_paint_height(h: f32) {
    LAST_PAINT_H.store(h.max(0.0) as u32, std::sync::atomic::Ordering::Relaxed);
}

/// ¿Se pinto alguna vez con altura util? Si es `false`, el waveform no aparece.
pub fn painted_with_real_height() -> bool {
    LAST_PAINT_H.load(std::sync::atomic::Ordering::Relaxed) > 0
}

pub fn peaks_are_silent(peaks: &[f32]) -> bool {
    peaks.is_empty() || peaks.iter().all(|p| *p <= f32::EPSILON)
}

/// Mejor fuente de picos para dibujar: la de mayor resolución **entre las que
/// tienen señal**. Si la de detalle está muda (archivo no legible), cae a la del
/// clip, que el pad ya sabe leer. Función pura para testear.
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
        // Ninguna tiene señal: se devuelve la de detalle para que el llamador
        // pueda dibujar el placeholder con el mismo conteo de bins.
        (false, false) => {
            if detail.len() >= base.len() {
                detail.to_vec()
            } else {
                base.to_vec()
            }
        }
    }
}

/// Etiquetas de la regla: número de compás en cada barra y marca de beat.
///
/// Se calcula **antes** de construir el árbol DOM. Antes se llenaba un slot
/// desde el closure de pintado, que corre después de que el árbol ya está
/// armado: por eso los números de compás nunca aparecían.
pub fn ruler_labels(
    scroll_secs: f64,
    visible: f64,
    bpm: f32,
    px_per_sec: f32,
) -> Vec<(String, f32, bool)> {
    let bpm_f = if bpm > 0.0 { bpm as f64 } else { 120.0 };
    let sec_per_beat = 60.0 / bpm_f * 4.0 / 4.0;
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

/// Geometria del waveform: para cada pico, la columna `x` (en px relativos al
/// lienzo) y la media altura `amp` de su segmento vertical.
///
/// Los picos se mapean proporcionalmente de 0 al ancho del lienzo. Si hay mas
/// picos que columnas de pixel se agrupan por maximo (downsample), para no
/// dibujar mas segmentos que pixeles. Funcion pura: es la geometria que despues
/// pinta el closure del canvas.
pub fn wave_geometry(peaks: &[f32], width_px: usize, half_h: f32) -> Vec<(f32, f32)> {
    let n = peaks.len();
    let width_px = width_px.max(1);
    if n == 0 || half_h <= 0.0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(n.min(width_px));
    if n >= width_px {
        // Downsample: una columna por pixel, maximo absoluto del rango.
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
        // Un segmento por pico. Se mapea de 0 al ancho del lienzo (divisor
        // `n - 1`) para que el primer y el último pico caigan en los bordes, como
        // el legacy al recorrer las muestras de 0 a `rect.width()`.
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

/// Peak absoluto en la ventana `[from, to)` de un buffer de picos.
///
/// Los picos vienen normalizados (512 bins del archivo entero), así que se
/// mapea la ventana a índices. Función pura para testear.
pub fn peaks_window(peaks: &[f32], from_ratio: f32, to_ratio: f32) -> Vec<f32> {
    let n = peaks.len();
    if n == 0 {
        return Vec::new();
    }
    let a = ((from_ratio.clamp(0.0, 1.0)) * n as f32).floor() as usize;
    let b = (((to_ratio.clamp(0.0, 1.0)) * n as f32).ceil() as usize).max(a + 1);
    peaks[a.min(n)..b.min(n).max(a.min(n))].to_vec()
}

// =========================================================================
// RESOLUCIÓN DEL CLIP (binding de selección)
// =========================================================================

/// Snapshot de lo que el editor necesita pintar, resuelto desde `AppState`.
///
/// Se arma en un solo lugar para que Matrix y Playlist produzcan exactamente
/// la misma estructura (y el render no distinguish el origen).
#[derive(Clone, Debug, Default)]
pub struct ClipEditorClip {
    pub name: String,
    pub path: PathBuf,
    /// Duración total del audio en segundos.
    pub duration_secs: f64,
    /// Picos normalizados 0..=1 del archivo entero (mini-waveform).
    pub peaks: Vec<f32>,
    /// Picos de mayor resolución, cargados del WAV para el editor.
    pub detail_peaks: Vec<f32>,
    /// Eventos de audio del clip (trim, gain, fades). Vacío en la Playlist,
    /// donde el clip es un único bloque.
    pub events: usize,
    pub is_midi: bool,
    /// Estado de loop heredado del clip (Matrix) o local (Playlist).
    pub loop_enabled: bool,
    pub loop_start_secs: f64,
    pub loop_end_secs: f64,
    /// Origen temporal del clip en la línea, para la regla de compases.
    pub start_secs: f64,
}

/// Resuelve el clip en edición desde el estado de la app.
///
/// Cubre las dos fuentes:
/// - Matrix: peaks del pad + se cargan picos de detalle del WAV.
/// - Playlist: picos del clip (los que ya trae para la miniatura) + offset.
pub fn resolve_clip(s: &AppState, bpm: f32) -> Option<ClipEditorClip> {
    let target = s.clip_editor.target?;
    match target {
        ClipEditorTarget::Matrix { track, scene } => {
            let slot = s.matrix_state.grid.get(track)?.get(scene)?;
            let clip = slot.clip.as_ref()?;
            let is_midi = matches!(clip.content, matrix::ClipData::Midi { .. });
            // Picos de detalle: se recalculan del WAV sólo para el clip en
            // edición (barato: un archivo, no uno por pad).
            let detail = if is_midi {
                Vec::new()
            } else {
                crate::views::open_dms_sampler::load_peaks_from_wav(
                    &clip.path.to_string_lossy(),
                    2048,
                )
            };
            let sr = clip
                .audio_events()
                .first()
                .map(|e| e.sample_rate)
                .unwrap_or(1) as f32;
            // El pad no guarda el instante absoluto: el compás de arranque lo
            // define `local_bar` (barras desde el inicio de la escena).
            let start_secs = clip.local_bar as f64 * 0.0;
            Some(ClipEditorClip {
                name: clip.name.clone(),
                path: clip.path.clone(),
                duration_secs: clip.duration_secs.max(0.0),
                peaks: clip.peaks.clone(),
                detail_peaks: detail,
                events: clip.audio_events().len(),
                is_midi,
                loop_enabled: clip.loop_enabled,
                loop_start_secs: start_secs + clip.loop_start as f64 / sr as f64,
                loop_end_secs: start_secs + clip.loop_end as f64 / sr as f64,
                start_secs,
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
                // Pattern/MIDI: se muestra el bloque pero sin waveform de audio.
                _ => (PathBuf::new(), 0, 0),
            };
            let ppqn = s.playlist_state.ppqn.max(1) as f64;
            let bpm_f = if bpm > 0.0 { bpm as f64 } else { 120.0 };
            let secs_per_tick = (60.0 / bpm_f) / ppqn;
            let start_secs = clip.start_tick as f64 * secs_per_tick;
            let duration_secs = clip.duration_ticks as f64 * secs_per_tick;
            let is_audio = !path.as_os_str().is_empty();
            let detail = if is_audio {
                crate::views::open_dms_sampler::load_peaks_from_wav(
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
            })
        }
    }
}

/// El estado de la Playlist guarda el loop en TICKS; el editor trabaja en
/// segundos. Convierte con el BPM vigente.
fn ce_loop_enabled(s: &AppState) -> bool {
    s.playlist_state.is_loop_region_valid()
}

/// Selecciona un clip para el editor y ABRE el panel.
///
/// Un solo punto de entrada para las dos fuentes (Session Matrix y Playlist),
/// así seleccionar siempre deja el editor visible con el clip cargado.
pub fn select_target(s: &mut AppState, target: Option<ClipEditorTarget>) {
    s.clip_editor.target = target;
    if target.is_some() {
        s.clip_editor.show_editor = true;
        // Cambiar de clip arranca el visualizador desde el comienzo.
        s.clip_editor.scroll_secs = 0.0;
    }
}

// =========================================================================
// RENDER
// =========================================================================

pub fn render(cx: &mut Context<HikaruApp>) -> AnyElement {
    let (bpm, sample_rate) = {
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

    // Estado vacío: sin clip seleccionado no hay nada que editar, y decirlo
    // es mejor que un panel en blanco.
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

    let (tab, params, target, snap) = (
        ce.tab,
        ce.params,
        ce.target,
        ce.snap,
    );
    let zoom_x = clamped_ce_zoom(ce.zoom_x);

    v_flex()
        .id("clip_editor")
        .test_support()
        .size_full()
        .gap(px(4.0))
        .bg(rgb(0x121216))
        .p(px(6.0))
        // --- Barra superior: clip actual + loop + snap + zoom ---------------
        .child(clip_topbar(cx, &clip, &ce, target, bpm, zoom_x, sample_rate))
        // --- Cuerpo: waveform A ANCHO COMPLETO del panel --------------------
        //
        // La versión legacy no tenía panel lateral: el waveform ocupaba todo el
        // ancho. Eso además evita el sidebar flotando sobre la Session Matrix
        // (el contenido se desbordaba del panel y se dibujaba encima).
        .child(
            v_flex()
                .flex_1()
                .flex_col()
                .min_h_0()
                .min_w_0()
                .child(waveform_viewer(cx, &clip, &ce, bpm, zoom_x)),
        )
        .into_any_element()
}

/// Barra superior del Clip Editor: identidad del clip a la izquierda y, a la
/// derecha y en este orden, modos de edición, Loop, Snap y zoom.
fn clip_topbar(
    cx: &mut Context<HikaruApp>,
    clip: &ClipEditorClip,
    ce: &ClipEditorState,
    target: Option<ClipEditorTarget>,
    bpm: f32,
    zoom_x: f32,
    sample_rate: u32,
) -> AnyElement {
    let snap_label = ce.snap.label();
    let loop_on = ce.loop_enabled;
    let name = clip.name.clone();
    let dur = clip.duration_secs;
    let events = clip.events;
    let is_midi = clip.is_midi;
    let target_label = target.map(|t| t.label()).unwrap_or_default();
    let visible = visible_secs(zoom_x, 800.0);

    h_flex()
        .id("clip_topbar")
        .test_support()
        .items_center()
        .gap(px(8.0))
        .h(px(28.0))
        .child(
            Label::new(name)
                .text_sm()
                .text_color(rgb(0xF2F2F5))
                .max_w(px(260.0)),
        )
        .child(
            Label::new(format!("{:.2}s", dur))
                .text_xs()
                .text_color(rgb(0x9AA4B2)),
        )
        .child(
            Label::new(format!("{} BPM", bpm.round() as i32))
                .text_xs()
                .text_color(rgb(0x9AA4B2)),
        )
        .child(
            Label::new(format!("{} kHz", sample_rate as f32 / 1000.0))
                .text_xs()
                .text_color(rgb(0x9AA4B2)),
        )
        .when(!is_midi, |d| {
            d.child(
                Label::new(format!("{events} ev"))
                    .text_xs()
                    .text_color(rgb(0x9AA4B2)),
            )
        })
        .child(
            Label::new(format!("{:.2}s vis", visible))
                .text_xs()
                .text_color(rgb(0x6B7280)),
        )
        .child(
            Label::new(target_label)
                .text_xs()
                .text_color(rgb(0x6B7280)),
        )
        // Espaciador: empuja el grupo de controles contra el borde derecho.
        .child(div().flex_1().min_w(px(8.0)))
        // Modos (Audio Events / Comping / Stretch / Onsets). El legacy no los
        // tenía, pero acá controlan el render (ej. marcas de onsets), así que
        // viven en la barra para no perderlos al quitar el sidebar lateral.
        .children(
            [
                (EditorTab::AudioEvents, "ce_tab_events"),
                (EditorTab::Comping, "ce_tab_comping"),
                (EditorTab::Stretch, "ce_tab_stretch"),
                (EditorTab::Onsets, "ce_tab_onsets"),
            ]
            .into_iter()
            .map(|(t, id)| {
                let active = ce.tab == t;
                Button::new(id)
                    .rounded(ButtonRounded::None)
                    .label(t.label())
                    .compact()
                    .bg(if active { rgb(0xE8760C) } else { rgb(0x24242B) })
                    .text_color(rgb(0xFFFFFF))
                    .on_click(move |_, _, cx| {
                        state(cx).update(cx, |s, cx| {
                            s.clip_editor.tab = t;
                            cx.notify();
                        });
                    })
                    .into_any_element()
            }),
        )
        // Toggle de Loop.
        .child(
            Button::new("ce_loop_toggle")
                .rounded(ButtonRounded::None)
                .label("Loop")
                .compact()
                .bg(if loop_on { rgb(0x1DB954) } else { rgb(0x24242B) })
                .text_color(rgb(0xFFFFFF))
                .on_click(|_, _, cx| {
                    state(cx).update(cx, |s, cx| {
                        s.clip_editor.loop_enabled = !s.clip_editor.loop_enabled;
                        cx.notify();
                    });
                }),
        )
        // Selector de Snap: cicla Off -> 1 Bar -> 1/2 -> 1/4 -> 1/8 -> 1/16.
        .child(
            Button::new("ce_snap_cycle")
                .rounded(ButtonRounded::None)
                .label(format!("Snap: {}", snap_label))
                .compact()
                .bg(rgb(0x24242B))
                .text_color(rgb(0xE8E8F0))
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
        // Zoom: - / valor / + / Fit.
        .child(
            Button::new("ce_zoom_out")
                .rounded(ButtonRounded::None)
                .label("-")
                .compact()
                .bg(rgb(0x24242B))
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
                .text_color(rgb(0x9AA4B2)),
        )
        .child(
            Button::new("ce_zoom_in")
                .rounded(ButtonRounded::None)
                .label("+")
                .compact()
                .bg(rgb(0x24242B))
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
                .label("Fit")
                .compact()
                .bg(rgb(0x24242B))
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
        )
        .into_any_element()
}

/// Ajusta el parámetro identificado por `label` un paso.
fn nudge_param(cx: &mut App, label: &'static str, up: bool) {
    state(cx).update(cx, |s, cx| {
        let p = s.clip_editor.params;
        let step = step_for(label);
        let (cur, lo, hi) = match label {
            "Gain" => (p.gain_db, -60.0, 60.0),
            "Pan" => (p.pan, -1.0, 1.0),
            "Pitch" => (p.pitch_cents, -60.0, 60.0),
            "Formant" => (p.formant, -1.0, 1.0),
            "Stretch" => (p.stretch, 0.25, 4.0),
            "Onsets" => (p.onset_threshold_db, -60.0, 0.0),
            _ => (0.0, -1.0, 1.0),
        };
        let next = if up {
            (cur + step).min(hi)
        } else {
            (cur - step).max(lo)
        };
        match label {
            "Gain" => s.clip_editor.params.gain_db = next,
            "Pan" => s.clip_editor.params.pan = next,
            "Pitch" => s.clip_editor.params.pitch_cents = next,
            "Formant" => s.clip_editor.params.formant = next,
            "Stretch" => s.clip_editor.params.stretch = next,
            "Onsets" => s.clip_editor.params.onset_threshold_db = next,
            _ => {}
        }
        cx.notify();
    });
}

fn step_for(label: &str) -> f32 {
    match label {
        "Gain" => 0.5,
        "Pan" => 0.05,
        "Pitch" => 1.0,
        "Formant" => 0.05,
        "Stretch" => 0.01,
        "Onsets" => 1.0,
        _ => 0.1,
    }
}

/// Visualizador: regla de compases + waveform + región de loop, con zoom y
/// scroll propios (independientes del zoom de la Playlist).
fn waveform_viewer(
    cx: &mut Context<HikaruApp>,
    clip: &ClipEditorClip,
    ce: &ClipEditorState,
    bpm: f32,
    zoom_x: f32,
) -> AnyElement {
    let duration = clip.duration_secs;
    let start_secs = clip.start_secs;
    let scroll = clamp_scroll(ce.scroll_secs, duration, visible_secs(zoom_x, 800.0));
    // Fuente de picos: la de detalle si tiene señal, si no la del clip (ver
    // `best_peaks`). Éste era el motivo del panel negro.
    let peaks = best_peaks(&clip.detail_peaks, &clip.peaks);
    let loop_enabled = ce.loop_enabled;
    // Los onsets se dibujan como marcas verticales; se decide FUERA del
    // closure de pintado (no hay `cx` disponible adentro).
    let show_onsets = ce.tab == EditorTab::Onsets;
    // Separación de la grilla de snap en segundos (0 = snap libre).
    let snap_secs = ce.snap.interval_secs(bpm);
    let loop_start = ce.loop_start_secs;
    let loop_end = ce.loop_end_secs;
    let target = ce.target;

    let wheel_catcher: AnyElement = div()
        .id("clip_editor_wheel")
        .absolute()
        .inset_0()
        .on_scroll_wheel(move |event, _, cx| {
            let delta = match event.delta {
                gpui_kit::ScrollDelta::Pixels(p) => p.x.as_f32(),
                gpui_kit::ScrollDelta::Lines(p) => p.x,
            };
            if delta.abs() < 0.01 {
                return;
            }
            state(cx).update(cx, |s, cx| {
                // Shift+rueda cambia el zoom del visualizador; la rueda
                // normal scrollea. Ambos son independientes del zoom de la
                // Playlist.
                if event.modifiers.shift {
                    s.clip_editor.zoom_x = clamped_ce_zoom(
                        s.clip_editor.zoom_x * if delta > 0.0 { 0.9 } else { 1.0 / 0.9 },
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
        .into_any_element();

    // Números de compás: calculados acá (no en el closure de pintado).
    let bar_number_labels: Vec<AnyElement> = ruler_labels(scroll, visible_secs(zoom_x, 800.0), bpm, zoom_x)
        .into_iter()
        .filter(|(text, x, _)| *x > -24.0 && *x < 820.0)
        .map(|(text, x, is_bar)| {
            Label::new(text.clone())
                .text_xs()
                // El compás se lee más fuerte que el beat.
                .text_color(if is_bar { rgb(0xE8E8F0) } else { rgb(0x9A9AA6) })
                .absolute()
                .left(px(x + 3.0))
                .top(px(3.0))
                .into_any_element()
        })
        .collect();

    div()
    .id("clip_editor_waveform")
    .test_support()
    .flex_1()
    .h(px(WAVE_MIN_H))
    .w_full()
    .relative()
    .children(bar_number_labels)
    .child(wheel_catcher)
    // El `canvas` de gpui NO tiene tamaño propio: sin esto su closure de
    // pintado recibe bounds de 1268x0, el `return` de guarda aborta y el panel
    // queda negro. `.absolute().inset_0()` lo hace llenar el contenedor (igual
    // que el mini-waveform del pad, en matrix.rs).
    .child(
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let w = bounds.size.width.as_f32();
                let h = bounds.size.height.as_f32();
                // Sonda de layout: el `canvas` de gpui no tiene tamaño propio y
                // sin `.absolute().inset_0()` su closure de pintado recibia
                // 1268x0 -> guarda abortaba -> panel negro. Se registra la
                // altura pintada para que el test lo verifique.
                crate::views::clip_editor::note_paint_height(h);
                if w <= 0.0 || h <= 0.0 {
                    return;
                }
            let px_per_sec = zoom_x.max(0.0001);
            let t0 = scroll;
            let x_of = |t: f64| bounds.origin.x + px(((t - t0) * px_per_sec as f64) as f32);
            let right = bounds.origin.x + bounds.size.width;
            let bottom = bounds.origin.y + bounds.size.height;

            // =====================================================
            // 1. FONDO — azul marino del legacy (no negro plano)
            // =====================================================
            window.paint_quad(PaintQuad {
                bounds,
                background: rgb(0x0A2540).into(),
                border_color: Hsla::default(),
                corner_radii: gpui_kit::Corners::default(),
                border_widths: gpui_kit::Edges::default(),
                border_style: BorderStyle::default(),
            });

            // Zona de onda: deja la banda de la regla arriba.
            let ruler_h = 20.0f32;
            let wave_y = bounds.origin.y + px(ruler_h);
            let wave_h = (h - ruler_h).max(4.0);
            let mid = wave_y + px(wave_h / 2.0);

            // =====================================================
            // 2. REGLA DE TIEMPO EN COMPASES (1, 2, 3 …) + GRILLA
            // =====================================================
            let bpm_f = if bpm > 0.0 { bpm as f64 } else { 120.0 };
            let sec_per_bar = 60.0 / bpm_f * 4.0;
            let sec_per_beat = sec_per_bar / 4.0;

            // Fondo de la regla.
            window.paint_quad(PaintQuad {
                bounds: Bounds::new(bounds.origin, size(bounds.size.width, px(ruler_h))),
                background: rgb(0x18334F).into(),
                border_color: rgb(0x2E5A85).into(),
                corner_radii: gpui_kit::Corners::default(),
                border_widths: gpui_kit::Edges {
                    bottom: px(1.0),
                    top: px(0.0),
                    left: px(0.0),
                    right: px(0.0),
                },
                border_style: BorderStyle::default(),
            });

            let vis = visible_secs(zoom_x, w);
            // Línea divisoria de cada BARRA + su número.
            let first_bar = (t0 / sec_per_bar).floor();
            let mut bar = first_bar;
            while bar * sec_per_bar < t0 + vis {
                let t = bar * sec_per_bar;
                let x = x_of(t);
                if x >= bounds.origin.x && x <= right {
                    let mut p = PathBuilder::stroke(px(1.0));
                    p.move_to(point(x, bounds.origin.y));
                    p.line_to(point(x, bottom));
                    if let Ok(path) = p.build() {
                        window.paint_path(path, rgba(0xFFFFFF66));
                    }
                }
                bar += 1.0;
            }
            // Subdivisión por BEAT (más tenue).
            let first_beat = (t0 / sec_per_beat).floor();
            let mut bt = first_beat;
            while bt * sec_per_beat < t0 + vis {
                let t = bt * sec_per_beat;
                let x = x_of(t);
                if x >= bounds.origin.x && x <= right && (bt as i64) % 4 != 0 {
                    let mut p = PathBuilder::stroke(px(1.0));
                    p.move_to(point(x, bounds.origin.y + px(ruler_h)));
                    p.line_to(point(x, bottom));
                    if let Ok(path) = p.build() {
                        window.paint_path(path, rgba(0xFFFFFF26));
                    }
                }
                bt += 1.0;
            }
            // Grilla de SNAP (cian, densa).
            if snap_secs > 0.0 {
                let mut t = (t0 / snap_secs).floor() * snap_secs;
                while t < t0 + vis {
                    let x = x_of(t);
                    if x >= bounds.origin.x && x <= right {
                        let mut p = PathBuilder::stroke(px(1.0));
                        p.move_to(point(x, wave_y));
                        p.line_to(point(x, bottom));
                        if let Ok(path) = p.build() {
                            window.paint_path(path, rgba(0x00C8FF33));
                        }
                    }
                    t += snap_secs;
                }
            }
            // Eje central (0 dB).
            let mut axis = PathBuilder::stroke(px(1.0));
            axis.move_to(point(bounds.origin.x, mid));
            axis.line_to(point(right, mid));
            if let Ok(path) = axis.build() {
                window.paint_path(path, rgba(0xFFFFFF2E));
            }

            // =====================================================
            // 3. FORMA DE ONDA — azul brillante, centrada verticalmente
            // =====================================================
            if peaks_are_silent(&peaks) {
                // Sin señal: triángulo placeholder centrado (legacy).
                let cx0 = bounds.origin.x + px(w / 2.0);
                let cy0 = mid;
                let mut tri = PathBuilder::stroke(px(1.5));
                tri.move_to(point(cx0 - px(9.0), cy0 - px(9.0)));
                tri.line_to(point(cx0 - px(9.0), cy0 + px(9.0)));
                tri.line_to(point(cx0 + px(9.0), cy0));
                tri.close();
                if let Ok(path) = tri.build() {
                    window.paint_path(path, rgba(0x4FC3F7AA));
                }
            } else {
                // Recorta los picos a la ventana temporal visible.
                let dur = duration.max(0.0001);
                let from = (t0 / dur).clamp(0.0, 1.0) as f32;
                let to = ((t0 + vis) / dur).clamp(0.0, 1.0) as f32;
                let window_peaks = peaks_window(&peaks, from, to);
                // Un segmento vertical por pico, de `mid - amp` a `mid + amp`
                // sobre el ancho del lienzo: el bucle de dibujo del legacy
                // (`painter.line_segment`).
                let width_px = (w.floor() as usize).max(1);
                let half_h = (wave_h * 0.5 - 2.0).max(2.0);
                let wave_color = rgb(0x00B4FF);
                for (x_off, amp) in wave_geometry(&window_peaks, width_px, half_h) {
                    let x = bounds.origin.x + px(x_off);
                    let mut seg = PathBuilder::stroke(px(1.0));
                    seg.move_to(point(x, mid - px(amp)));
                    seg.line_to(point(x, mid + px(amp)));
                    if let Ok(path) = seg.build() {
                        window.paint_path(path, wave_color);
                    }
                }
            }

            // =====================================================
            // 4. MARCADORES DE BORDE: inicio VERDE, final ROJO
            // =====================================================
            let marker_w = px(4.0);
            let (start_x, end_x) = if loop_enabled && loop_end > loop_start {
                (x_of(loop_start), x_of(loop_end))
            } else {
                (x_of(0.0), x_of(duration))
            };
            for (mx, col) in [(start_x, rgb(0x00E676)), (end_x, rgb(0xFF1744))] {
                if mx >= bounds.origin.x - marker_w && mx <= right + marker_w {
                    let r = Bounds::new(
                        point(mx - marker_w / 2.0, wave_y),
                        size(marker_w, bounds.size.height - px(ruler_h)),
                    );
                    window.paint_quad(PaintQuad {
                        bounds: r,
                        background: col.into(),
                        border_color: Hsla::default(),
                        corner_radii: gpui_kit::Corners::default(),
                        border_widths: gpui_kit::Edges::default(),
                        border_style: BorderStyle::default(),
                    });
                }
            }

            // =====================================================
            // 5. PLAYHEAD
            // =====================================================
            let ph_x = x_of(if loop_enabled && loop_end > loop_start {
                loop_start
            } else {
                t0
            });
            if ph_x >= bounds.origin.x && ph_x <= right {
                let mut php = PathBuilder::stroke(px(1.0));
                php.move_to(point(ph_x, bounds.origin.y + px(ruler_h)));
                php.line_to(point(ph_x, bottom));
                if let Ok(path) = php.build() {
                    window.paint_path(path, rgba(0xFFFFFFAA));
                }
            }

            let _ = (target, sec_per_bar, show_onsets);
        },
        )
        .absolute()
        .inset_0(),
    )
    .into_any_element()
}

/// Duración (en segundos) del clip en edición, para acotar el scroll.
///
/// En la Matrix sale de `duration_secs`; en la Playlist hay que convertir los
/// ticks con el BPM vigente.
fn clip_duration_hint(s: &AppState) -> f64 {
    match s.clip_editor.target {
        Some(ClipEditorTarget::Matrix { track, scene }) => s
            .matrix_state
            .grid
            .get(track)
            .and_then(|r| r.get(scene))
            .and_then(|slot| slot.clip.as_ref())
            .map(|c| c.duration_secs)
            .unwrap_or(1.0),
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

