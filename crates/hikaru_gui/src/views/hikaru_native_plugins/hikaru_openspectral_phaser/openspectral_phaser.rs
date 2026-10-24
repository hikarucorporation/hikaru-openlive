// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_openspectral_phaser/openspectral_phaser.rs

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use gpui_kit::component::label::Label;
use gpui_kit::component::v_flex;
use gpui_kit::{InteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, HikaruApp};
use crate::render::WavetableViewportHandle;
use crate::views::controls;

const PARAM_NAMES: [&str; 11] = [
    "Width", "Offset", "Depth", "Speed", "Mix", "Blur", "Hz/Oct", "1x/5x",
    "Keep Bass", "Blocksize", "Bypass",
];

const DEFAULTS: [f32; 11] = [50.0, 0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 0.0, 0.0, 1024.0, 0.0];

pub const BLOCKSIZE_OPTIONS: [f32; 6] = [512.0, 1024.0, 2048.0, 4096.0, 8192.0, 16384.0];

type StateMap = Mutex<HashMap<(usize, usize), [f32; 11]>>;
fn state_map() -> &'static StateMap {
    static CELL: OnceLock<StateMap> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(HashMap::new()))
}

fn param_min(idx: usize) -> f32 {
    match idx {
        0 | 2 | 4 | 5 => 0.0,
        1 | 3 | 6 => -100.0,
        7 | 8 | 10 => 0.0,
        9 => 512.0,
        _ => 0.0,
    }
}

fn param_max(idx: usize) -> f32 {
    match idx {
        0 | 2 | 4 | 5 => 100.0,
        1 | 3 | 6 => 100.0,
        7 | 8 | 10 => 1.0,
        9 => 16384.0,
        _ => 100.0,
    }
}

fn snap_blocksize(value: f32) -> f32 {
    let mut best = BLOCKSIZE_OPTIONS[0];
    let mut best_dist = (value - best).abs();
    for &opt in &BLOCKSIZE_OPTIONS {
        let d = (value - opt).abs();
        if d < best_dist {
            best_dist = d;
            best = opt;
        }
    }
    best
}

fn clamp_param(idx: usize, value: f32) -> f32 {
    if idx == 9 {
        snap_blocksize(value)
    } else {
        value.clamp(param_min(idx), param_max(idx))
    }
}

fn get_params(track_idx: usize, slot_idx: usize) -> [f32; 11] {
    let mut map = state_map().lock().unwrap();
    *map.entry((track_idx, slot_idx)).or_insert(DEFAULTS)
}

fn set_param(track_idx: usize, slot_idx: usize, idx: usize, value: f32) {
    let mut map = state_map().lock().unwrap();
    let entry = map.entry((track_idx, slot_idx)).or_insert(DEFAULTS);
    if idx < 11 {
        entry[idx] = clamp_param(idx, value);
    }
}

fn notify(cx: &mut gpui_kit::App) {
    let st = state(cx);
    st.update(cx, |_, cx| cx.notify());
}

fn param_fill(idx: usize, value: f32) -> f32 {
    match idx {
        0 | 2 | 4 | 5 => (value / 100.0).clamp(0.0, 1.0),
        1 | 3 | 6 => ((value + 100.0) / 200.0).clamp(0.0, 1.0),
        _ => 0.0,
    }
}

// ---------------------------------------------------------------------------
// Drag state (global, keyed by slot) — mismo patrón que el Wavetable.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct PhaserDrag {
    knob_idx: usize,
    grab_fill: f32,
    origin_y: f32,
}

type DragMap = Mutex<HashMap<(usize, usize), PhaserDrag>>;
fn drag_map() -> &'static DragMap {
    static CELL: OnceLock<DragMap> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(HashMap::new()))
}

const DRAG_RANGE_PX: f32 = 160.0;

fn phaser_dragging(track_idx: usize, slot_idx: usize) -> bool {
    match drag_map().lock() {
        Ok(map) => map.contains_key(&(track_idx, slot_idx)),
        Err(_) => false,
    }
}

fn dragging_knob(track_idx: usize, slot_idx: usize, idx: usize) -> bool {
    match drag_map().lock() {
        Ok(map) => map
            .get(&(track_idx, slot_idx))
            .map(|d| d.knob_idx == idx)
            .unwrap_or(false),
        Err(_) => false,
    }
}

fn begin_drag(track_idx: usize, slot_idx: usize, knob_idx: usize, fill: f32, window_y: f32) {
    let fill = controls::finite(fill).clamp(0.0, 1.0);
    if let Ok(mut map) = drag_map().lock() {
        map.insert(
            (track_idx, slot_idx),
            PhaserDrag {
                knob_idx,
                grab_fill: fill,
                origin_y: window_y,
            },
        );
    }
}

fn end_drag(track_idx: usize, slot_idx: usize) {
    if let Ok(mut map) = drag_map().lock() {
        map.remove(&(track_idx, slot_idx));
    }
}

fn fill_to_value(idx: usize, fill: f32) -> f32 {
    match idx {
        0 | 2 | 4 | 5 => (fill * 100.0).clamp(0.0, 100.0),
        1 | 3 | 6 => (fill * 200.0 - 100.0).clamp(-100.0, 100.0),
        _ => 0.0,
    }
}

fn drag_move(
    cx: &mut gpui_kit::App,
    track_idx: usize,
    slot_idx: usize,
    window_y: f32,
    fine: bool,
) -> bool {
    let mut changed = false;
    if let Ok(mut map) = drag_map().lock() {
        let Some(drag) = map.get_mut(&(track_idx, slot_idx)) else {
            return false;
        };
        let idx = drag.knob_idx;
        let delta_px = drag.origin_y - window_y;
        let gain = if fine { 0.25 } else { 1.0 };
        let mut fill = drag.grab_fill + delta_px / DRAG_RANGE_PX * gain;
        if !fill.is_finite() {
            fill = drag.grab_fill;
        }
        let fill = fill.clamp(0.0, 1.0);
        if (fill - drag.grab_fill).abs() < 1e-6 {
            return false;
        }
        drag.grab_fill = fill;
        drop(map);
        let value = fill_to_value(idx, fill);
        set_param(track_idx, slot_idx, idx, value);
        changed = true;
    }
    if changed {
        notify(cx);
    }
    changed
}

fn register_drag_tracking(window: &mut gpui_kit::Window, track_idx: usize, slot_idx: usize) {
    window.on_mouse_event(move |event: &MouseMoveEvent, _phase, _window, cx| {
        let window_y = event.position.y.as_f32();
        let fine = event.modifiers.shift;
        let changed = drag_move(cx, track_idx, slot_idx, window_y, fine);
        if changed {
            notify(cx);
        }
    });
}

fn drag_keepalive(track_idx: usize, slot_idx: usize) -> AnyElement {
    canvas(
        |_bounds, _window, _cx| {},
        move |_bounds, _state, window, _cx| {
            if phaser_dragging(track_idx, slot_idx) {
                register_drag_tracking(window, track_idx, slot_idx);
            }
        },
    )
    .absolute()
    .inset_0()
    .into_any_element()
}

// ---------------------------------------------------------------------------
// WGPU scope: respuesta en magnitud de la cascada de biquads en vivo.
// ---------------------------------------------------------------------------

const SCOPE_BINS: usize = 48;

/// Cuadros por segundo del scope cuando el LFO está en movimiento.
///
/// La curva se anima con la fase del LFO, pero cada cuadro implica un readback
/// de GPU. A 15 fps el barrido se ve fluido y el costo es acotado; con Speed en
/// cero la curva es estática y el cache la reusa sin leer la GPU.
const SCOPE_FPS: f32 = 15.0;

/// Tiempo de pared desde el primer uso, para animar el LFO del scope.
fn scope_time_secs() -> f32 {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f32()
}

fn phaser_scope_magnitudes(params: [f32; 11]) -> Vec<f32> {
    use hikaru_dsp::effects::openspectral_phaser::{SpectralPhaser, SpectralPhaserParams};

    // Bypass: respuesta plana (0 dB) en todo el espectro.
    if params[10] >= 0.5 {
        return vec![1.0; SCOPE_BINS];
    }

    let mut p = SpectralPhaserParams::default();
    p.width = params[0];
    p.offset = params[1];
    p.depth = params[2];
    p.speed = params[3];
    p.mix = params[4];
    p.blur = params[5];
    p.hz_per_oct = params[6];
    p.times_five = params[7];
    p.keep_bass = 0.0;
    p.blocksize = params[9];
    p.bypass = 0.0;

    let sr = 44100.0_f32;
    let nyq = sr * 0.49;
    let f_min = 40.0_f32;

    let mut phaser = SpectralPhaser::new(sr);
    phaser.set_params(p);

    // Fase del LFO desde el reloj, cuantizada para no pedir un readback por
    // frame. La respuesta se calcula analíticamente: no procesa audio.
    let rate = phaser.lfo_rate_hz();
    let t = (scope_time_secs() * SCOPE_FPS).floor() / SCOPE_FPS;
    let phase = std::f32::consts::TAU * rate * t;
    phaser.set_analysis_cutoff(phaser.cutoff_at_phase(phase));

    let wet = (params[4] / 100.0).clamp(0.0, 1.0);
    let sections = phaser.num_sections().max(1) as f32;
    let mut db = Vec::with_capacity(SCOPE_BINS);
    for i in 0..SCOPE_BINS {
        let t = (i as f32 + 0.5) / SCOPE_BINS as f32;
        let f = (f_min * (nyq / f_min).powf(t)).clamp(20.0, nyq);
        let cascade = phaser.magnitude_response(f);
        // Ganancia media por sección: la cascada completa acumula cientos de
        // dB y aplastaría la curva contra el techo. El promedio conserva la
        // forma del filtro y hace visible el codo de corte.
        let mag = ((1.0 - wet) + wet * cascade).max(1.0e-9);
        db.push(20.0 * mag.log10() / sections);
    }

    // Normaliza el rango de dB del frame a 0..1: el scope muestra la *forma*
    // de la respuesta, no un valor absoluto calibrado.
    let min_db = db.iter().copied().fold(f32::INFINITY, f32::min);
    let max_db = db.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let span = (max_db - min_db).max(1.0);
    db.into_iter()
        .map(|value| ((value - min_db) / span).clamp(0.0, 1.0))
        .collect()
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

const KNOB_DIAL: f32 = 32.0;
const KNOB_CELL_W: f32 = 108.0;

fn dial_view(track_idx: usize, slot_idx: usize, idx: usize, fill: f32, dragging: bool) -> AnyElement {
    let fill = controls::finite(fill).clamp(0.0, 1.0);
    let accent = rgb(0xFF6E00);
    let border_color = if dragging { accent } else { rgb(0x3A4152) };
    let body_color = if dragging { rgb(0x262B37) } else { rgb(0x1E212B) };

    div()
        .id(format!("osp_dial_{track_idx}_{slot_idx}_{idx}"))
        .w(px(KNOB_DIAL))
        .h(px(KNOB_DIAL))
        .cursor_pointer()
        .on_mouse_down(gpui_kit::MouseButton::Left, move |event, _window, cx| {
            begin_drag(track_idx, slot_idx, idx, fill, event.position.y.as_f32());
            notify(cx);
        })
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let left = bounds.origin.x.as_f32();
                    let top = bounds.origin.y.as_f32();
                    let side = f32::min(bounds.size.width.as_f32(), bounds.size.height.as_f32());
                    let center_x = left + side / 2.0;
                    let center_y = top + side / 2.0;
                    let radius = side / 2.0 - 1.0;

                    // Cuerpo del dial.
                    window.paint_quad(PaintQuad {
                        bounds: Bounds::new(
                            point(px(center_x - radius), px(center_y - radius)),
                            gpui_kit::size(px(radius * 2.0), px(radius * 2.0)),
                        ),
                        background: body_color.into(),
                        border_color: border_color.into(),
                        corner_radii: Corners {
                            top_left: px(radius),
                            top_right: px(radius),
                            bottom_right: px(radius),
                            bottom_left: px(radius),
                        },
                        border_widths: gpui_kit::Edges { top: px(1.0), right: px(1.0), bottom: px(1.0), left: px(1.0) },
                        border_style: BorderStyle::default(),
                    });

                    // Marcas del recorrido (270°): dan la lectura de perilla
                    // de plugin comercial sin ocupar layout.
                    let mut ticks = PathBuilder::stroke(px(1.0));
                    for step in 0..=10 {
                        let angle = controls::needle_angle(step as f32 / 10.0);
                        let (sin, cos) = angle.sin_cos();
                        let inner = radius - 3.0;
                        let outer = radius - 6.0;
                        ticks.move_to(point(
                            px(center_x + inner * cos),
                            px(center_y + inner * sin),
                        ));
                        ticks.line_to(point(
                            px(center_x + outer * cos),
                            px(center_y + outer * sin),
                        ));
                    }
                    if let Ok(path) = ticks.build() {
                        window.paint_path(path, rgb(0x555B6B));
                    }

                    // Aguja.
                    let angle = controls::needle_angle(fill);
                    let needle = (radius - 7.0).max(1.0);
                    let mut path = PathBuilder::stroke(px(2.0));
                    path.move_to(point(px(center_x), px(center_y)));
                    path.line_to(point(
                        px(center_x + needle * angle.cos()),
                        px(center_y + needle * angle.sin()),
                    ));
                    if let Ok(path) = path.build() {
                        window.paint_path(path, accent);
                    }

                    // Eje.
                    window.paint_quad(PaintQuad {
                        bounds: Bounds::new(
                            point(px(center_x - 1.5), px(center_y - 1.5)),
                            gpui_kit::size(px(3.0), px(3.0)),
                        ),
                        background: accent.into(),
                        border_color: accent.into(),
                        corner_radii: Corners {
                            top_left: px(1.5),
                            top_right: px(1.5),
                            bottom_right: px(1.5),
                            bottom_left: px(1.5),
                        },
                        border_widths: gpui_kit::Edges { top: px(0.0), right: px(0.0), bottom: px(0.0), left: px(0.0) },
                        border_style: BorderStyle::default(),
                    });
                },
            )
            .w(px(KNOB_DIAL))
            .h(px(KNOB_DIAL)),
        )
        .into_any_element()
}

/// Botón compacto de paso (−/+) de una perilla.
fn step_button(
    id: String,
    label: &'static str,
    track_idx: usize,
    slot_idx: usize,
    idx: usize,
    delta: f32,
) -> AnyElement {
    div()
        .id(id)
        .w(px(18.0))
        .h(px(16.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(3.0))
        .bg(rgb(0x23232B))
        .border_1()
        .border_color(rgb(0x33333D))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(0x2E2E38)).border_color(rgb(0x4A4A56)))
        .on_click(move |_, _, cx| {
            let cur = get_params(track_idx, slot_idx)[idx];
            set_param(track_idx, slot_idx, idx, cur + delta);
            notify(cx);
        })
        .child(Label::new(label).text_xs().text_color(rgb(0xB8BCC8)))
        .into_any_element()
}

/// Valor de perilla con un decimal, sin `-0.0`.
fn format_value(value: f32) -> String {
    let value = if value.abs() < 0.05 { 0.0 } else { value };
    format!("{value:.1}")
}

fn knob_cell(track_idx: usize, slot_idx: usize, idx: usize, params: [f32; 11]) -> AnyElement {
    let value = params[idx];
    let fill = param_fill(idx, value);
    let is_dragging = dragging_knob(track_idx, slot_idx, idx);

    gpui_kit::component::v_flex()
        .id(format!("osp_knob_{track_idx}_{slot_idx}_{idx}"))
        .w(px(KNOB_CELL_W))
        .items_center()
        .gap(px(2.0))
        .child(dial_view(track_idx, slot_idx, idx, fill, is_dragging))
        .child(
            Label::new(PARAM_NAMES[idx].to_string())
                .text_xs()
                .text_center()
                .max_w(px(KNOB_CELL_W - 6.0))
                .truncate()
                .text_color(rgb(0x9AA0AE)),
        )
        .child(
            gpui_kit::component::h_flex()
                .items_center()
                .justify_center()
                .gap(px(3.0))
                .child(step_button(
                    format!("osp_dec_{track_idx}_{slot_idx}_{idx}"),
                    "−",
                    track_idx,
                    slot_idx,
                    idx,
                    -1.0,
                ))
                .child(
                    Label::new(format_value(value))
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_center()
                        .w(px(50.0))
                        .text_color(rgb(0xE8EAF0)),
                )
                .child(step_button(
                    format!("osp_inc_{track_idx}_{slot_idx}_{idx}"),
                    "+",
                    track_idx,
                    slot_idx,
                    idx,
                    1.0,
                )),
        )
        .into_any_element()
}

/// Estado observable del phaser (reexportado para compatibilidad).
#[derive(Clone, Debug)]
pub struct OpenSpectralPhaserState {
    pub params: [f32; 11],
}

impl Default for OpenSpectralPhaserState {
    fn default() -> Self {
        Self { params: DEFAULTS }
    }
}

impl OpenSpectralPhaserState {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Chip compacto de 20 px: toggles y blocksize. Los `Button` de GPUI Kit
/// miden 32 px de alto por defecto y en una tarjeta de 266 px de cuerpo
/// empujaban la grilla de perillas fuera de la tarjeta.
fn chip_button(
    id: String,
    label: String,
    active: bool,
    track_idx: usize,
    slot_idx: usize,
    idx: usize,
) -> AnyElement {
    let border = if active { rgb(0xFF6E00) } else { rgb(0x3A3A46) };
    let background = if active { rgb(0x33200F) } else { rgb(0x23232B) };
    let text = if active { rgb(0xFF8A33) } else { rgb(0xB8BCC8) };

    div()
        .id(id)
        .h(px(20.0))
        .px(px(8.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(3.0))
        .bg(background)
        .border_1()
        .border_color(border)
        .cursor_pointer()
        .hover(|style| style.bg(rgb(0x2E2E38)))
        .on_click(move |_, _, cx| {
            let cur = get_params(track_idx, slot_idx)[idx];
            set_param(track_idx, slot_idx, idx, if cur >= 0.5 { 0.0 } else { 1.0 });
            notify(cx);
        })
        .child(Label::new(label).text_xs().text_color(text))
        .into_any_element()
}

/// Chip de blocksize: cicla 512 → 1024 → … → 16384.
fn blocksize_chip(
    label: String,
    track_idx: usize,
    slot_idx: usize,
) -> AnyElement {
    div()
        .id(format!("osp_blocksize_{track_idx}_{slot_idx}"))
        .h(px(20.0))
        .px(px(8.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(3.0))
        .bg(rgb(0x23232B))
        .border_1()
        .border_color(rgb(0x3A3A46))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(0x2E2E38)))
        .on_click(move |_, _, cx| {
            let cur = get_params(track_idx, slot_idx)[9];
            let mut next = BLOCKSIZE_OPTIONS[0];
            for (i, &opt) in BLOCKSIZE_OPTIONS.iter().enumerate() {
                if (cur - opt).abs() < 0.5 {
                    next = BLOCKSIZE_OPTIONS[(i + 1) % BLOCKSIZE_OPTIONS.len()];
                    break;
                }
            }
            set_param(track_idx, slot_idx, 9, next);
            notify(cx);
        })
        .child(
            Label::new(label)
                .text_xs()
                .text_color(rgb(0xB8BCC8)),
        )
        .into_any_element()
}

/// Dibuja la tarjeta del plugin en el rack.
///
/// Layout de dos columnas maestras: a la izquierda el scope wgpu con la curva
/// de los biquads y a la derecha los controles (Hz/Oct, toggles, estado y la
/// grilla 3x2 de perillas). La tarjeta es ancha a propósito (ver
/// `PHASER_CARD_WIDTH`) para que ninguna columna quede apretada.
pub fn render_module(cx: &mut Context<HikaruApp>, track_idx: usize, slot_idx: usize) -> AnyElement {
    let params = get_params(track_idx, slot_idx);

    // Scope espectral en tiempo real: wgpu offscreen → RenderImage.
    let magnitudes = phaser_scope_magnitudes(params);
    let scope_image = {
        let handle = cx.global::<WavetableViewportHandle>().clone();
        handle.spawn_connect(cx);
        handle.render_phaser_scope(&magnitudes)
    };

    let rate_on = params[7] >= 0.5;
    let keep_bass_on = params[8] >= 0.5;
    let bypass_on = params[10] >= 0.5;

    // ------------------------------------------------------------------
    // Columna izquierda: display / scope.
    // ------------------------------------------------------------------
    let fallback = {
        let mut spectrum = gpui_kit::component::h_flex()
            .w_full()
            .h_full()
            .items_end()
            .gap(px(2.0));
        for i in 0..48 {
            let frac = ((i as f32 * 0.7).sin().abs() * 0.5
                + (params[2] / 100.0) * 0.35
                + 0.12)
                .clamp(0.05, 1.0);
            spectrum = spectrum.child(
                div()
                    .flex_1()
                    .h(relative(frac))
                    .bg(rgb(0xFF6E00)),
            );
        }
        spectrum.into_any_element()
    };

    let display = gpui_kit::component::v_flex()
        .id(format!("osp_display_{track_idx}_{slot_idx}"))
        .flex_1()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .gap(px(6.0))
        .p(px(8.0))
        .bg(rgb(0x0E0E13))
        .border_1()
        .border_color(rgb(0x2A2A30))
        .rounded(px(4.0))
        .child(
            gpui_kit::component::h_flex()
                .w_full()
                .items_center()
                .child(
                    Label::new("DISPLAY / FFT SCOPE VIEW")
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0x7A8090)),
                )
                .child(div().flex_1())
                .child(
                    Label::new("BIQUAD CASCADE")
                        .text_xs()
                        .text_color(rgb(0x474C58)),
                ),
        )
        .child(
            match scope_image {
                Some(image) => gpui_kit::img(image)
                    .id(format!("osp_scope_{track_idx}_{slot_idx}"))
                    .flex_1()
                    .w_full()
                    .min_h(px(0.0))
                    .object_fit(ObjectFit::Fill)
                    .into_any_element(),
                None => div()
                    .flex_1()
                    .w_full()
                    .min_h(px(0.0))
                    .child(fallback)
                    .into_any_element(),
            },
        )
        .child(
            gpui_kit::component::h_flex()
                .w_full()
                .items_center()
                .child(
                    Label::new(if bypass_on {
                        "BYPASS · DRY"
                    } else if params[3].abs() > 0.05 {
                        "LIVE · LFO"
                    } else {
                        "LIVE · STATIC"
                    })
                    .text_xs()
                    .text_color(rgb(0xFF6E00)),
                )
                .child(div().flex_1())
                .child(
                    Label::new("44.1 kHz")
                        .text_xs()
                        .text_color(rgb(0x474C58)),
                ),
        );

    // ------------------------------------------------------------------
    // Columna derecha: controles.
    // ------------------------------------------------------------------
    let hz_value = params[6];
    let hz_fill = param_fill(6, hz_value);
    let hz_text = if hz_value.abs() < 0.05 {
        "0.0".to_string()
    } else {
        format!("{hz_value:+.1}")
    };

    let hz_header = gpui_kit::component::h_flex()
        .w_full()
        .items_center()
        .child(
            Label::new("FREQ / CUTOFF (Hz / Oct)")
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0xFF6E00)),
        )
        .child(div().flex_1())
        .child(
            Label::new(format!("{hz_text} Hz/Oct"))
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0xD0D3DB)),
        );

    let blocksize_value = params[9];
    let toggles = gpui_kit::component::h_flex()
        .w_full()
        .items_center()
        .gap(px(6.0))
        .child(chip_button(
            format!("osp_toggle_rate_{track_idx}_{slot_idx}"),
            if rate_on { "Rate: 5x".to_string() } else { "Rate: 1x".to_string() },
            rate_on,
            track_idx,
            slot_idx,
            7,
        ))
        .child(chip_button(
            format!("osp_toggle_keepbass_{track_idx}_{slot_idx}"),
            "Keep Bass".to_string(),
            keep_bass_on,
            track_idx,
            slot_idx,
            8,
        ))
        .child(chip_button(
            format!("osp_toggle_bypass_{track_idx}_{slot_idx}"),
            if bypass_on { "Bypass: On".to_string() } else { "Bypass: Off".to_string() },
            bypass_on,
            track_idx,
            slot_idx,
            10,
        ))
        .child(blocksize_chip(
            format!("Blocksize: {}", blocksize_value as i32),
            track_idx,
            slot_idx,
        ));

    let hz_slider = gpui_kit::component::h_flex()
        .w_full()
        .items_center()
        .gap(px(8.0))
        .child(
            Label::new("HZ")
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0x7A8090)),
        )
        .child(
            div()
                .id(format!("osp_hzbar_{track_idx}_{slot_idx}"))
                .flex_1()
                .h(px(10.0))
                .bg(rgb(0x24242C))
                .rounded(px(5.0))
                .relative()
                .overflow_hidden()
                .child(
                    div()
                        .absolute()
                        .left(px(0.0))
                        .top(px(0.0))
                        .bottom(px(0.0))
                        .w(relative(hz_fill))
                        .rounded(px(5.0))
                        .bg(rgb(0xFF6E00)),
                ),
        )
        .child(
            Label::new("OCT")
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0x7A8090)),
        );

    let hz_control = gpui_kit::component::v_flex()
        .w_full()
        .gap(px(4.0))
        .child(hz_header)
        .child(toggles)
        .child(hz_slider);

    let controls_title = gpui_kit::component::h_flex()
        .w_full()
        .items_center()
        .child(
            Label::new("CONTROLES DE DSP & MODULACIÓN")
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0xFF6E00)),
        )
        .child(div().flex_1())
        .child(
            Label::new("DRAG ↕ · SHIFT = FINE")
                .text_xs()
                .text_color(rgb(0x474C58)),
        );

    let knobs_row1 = gpui_kit::component::h_flex()
        .w_full()
        .items_start()
        .justify_center()
        .gap(px(14.0))
        .child(knob_cell(track_idx, slot_idx, 0, params))
        .child(knob_cell(track_idx, slot_idx, 1, params))
        .child(knob_cell(track_idx, slot_idx, 2, params));

    let knobs_row2 = gpui_kit::component::h_flex()
        .w_full()
        .items_start()
        .justify_center()
        .gap(px(14.0))
        .child(knob_cell(track_idx, slot_idx, 3, params))
        .child(knob_cell(track_idx, slot_idx, 4, params))
        .child(knob_cell(track_idx, slot_idx, 5, params));

    let knobs_grid = gpui_kit::component::v_flex()
        .w_full()
        .gap(px(8.0))
        .child(knobs_row1)
        .child(knobs_row2);

    let right = gpui_kit::component::v_flex()
        .flex_1()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .gap(px(3.0))
        .p(px(2.0))
        .child(hz_control)
        .child(div().w_full().h(px(1.0)).bg(rgb(0x2A2A30)))
        .child(controls_title)
        .child(knobs_grid)
        .child(div().flex_1());

    let root = gpui_kit::component::h_flex()
        .id(format!("osp_module_{track_idx}_{slot_idx}"))
        .w_full()
        .flex_1()
        .min_h(px(0.0))
        .gap(px(8.0))
        .p(px(6.0))
        .relative()
        .on_mouse_up(gpui_kit::MouseButton::Left, move |_, _, cx| {
            end_drag(track_idx, slot_idx);
            notify(cx);
        })
        .on_mouse_up_out(gpui_kit::MouseButton::Left, move |_, _, cx| {
            end_drag(track_idx, slot_idx);
            notify(cx);
        })
        .child(display)
        .child(right)
        .child(drag_keepalive(track_idx, slot_idx));

    root.into_any_element()
}

/// Pie compacto de la tarjeta del phaser (18 px): identidad del bloque nativo.
pub fn render_module_footer(
    _cx: &mut Context<HikaruApp>,
    _track_idx: usize,
    _slot_idx: usize,
) -> AnyElement {
    gpui_kit::component::h_flex()
        .w_full()
        .h(px(18.0))
        .px(px(8.0))
        .items_center()
        .bg(rgb(0x1A1A20))
        .border_t_1()
        .border_color(rgb(0x2A2A30))
        .child(
            Label::new("PARÁMETROS (11) · Bloque nativo Rust + GPUI Kit")
                .text_xs()
                .text_color(rgb(0x6A6F7C)),
        )
        .child(div().flex_1())
        .child(
            Label::new("Editor")
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0xFF6E00)),
        )
        .into_any_element()
}

/// Variante sin estado de slot, para compatibilidad.
pub fn render_module_simple() -> AnyElement {
    v_flex()
        .w_full()
        .flex_1()
        .min_h(px(100.0))
        .items_center()
        .justify_center()
        .child(
            Label::new("OpenSpectral Phaser")
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0xE8EAF0)),
        )
        .into_any_element()
}

// Aliases comunes.
pub use crate::views::hikaru_native_plugins::hikaru_openspectral_phaser::OpenSpectralPhaserState as OpenSpectralPhaser;
// Reexport del DSP para conveniencia.
pub use hikaru_dsp::effects::openspectral_phaser::*;