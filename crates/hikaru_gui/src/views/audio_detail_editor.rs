// Hikaru OpenLive - Audio Clip Detail Editor (réplica del Detail Editor de Bitwig)
// GNU AGPLv3
// crates/hikaru_gui/src/views/audio_detail_editor.rs
//
//  ┌──┬─────────────┬────────────────────────────────────────────────┐
//  │v │ [tool]      │ regla  -1.3 -1.4 1.1 1.2 ...                    │
//  │  │ Audio Events│ tira de loop (rango + marcadores naranja)       │
//  │n │ Comping     │ cabecera del clip (nombre) + línea naranja      │
//  │o │ Stretch Ons.│ waveform estéreo sobre fondo claro + onsets     │
//  │m │ Gain Pan .. │                                                 │
//  │b │ auto-fades  │ scrollbar horizontal                    1/16    │
//  └──┴─────────────┴────────────────────────────────────────────────┘

use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Shape, Stroke, StrokeKind};

use crate::app::AppState;
use crate::theme;
use crate::views::matrix::{ClipData, MatrixClip};

const STRIP_W: f32 = 30.0;
const TOOLS_W: f32 = 148.0;
const RULER_H: f32 = 20.0;
const LOOP_H: f32 = 49.0;
const HEADER_H: f32 = 24.0;
const SCROLL_H: f32 = 18.0;
const MIN_PPB: f32 = 16.0;
const MAX_PPB: f32 = 2400.0;
const DEFAULT_PPB: f32 = 76.0;

const WAVE_BG: Color32 = Color32::from_rgb(0xC9, 0xC9, 0xC9);
const WAVE_FG: Color32 = Color32::from_rgb(0x2E, 0x2E, 0x2E);
const HEADER_BG: Color32 = Color32::from_rgb(0xA9, 0xAC, 0xAE);
const OUTSIDE_BG: Color32 = Color32::from_rgb(0x3A, 0x3A, 0x3A);
const ONSET_BLUE: Color32 = Color32::from_rgb(0x2F, 0x6F, 0xE0);
const ONSET_HOT: Color32 = Color32::from_rgb(0x74, 0xB3, 0xFF);
const DEFAULT_ONSET_SENS: f32 = 0.65;

#[derive(Clone, Copy, PartialEq, Eq)]
enum LoopHandle {
    None,
    Start,
    End,
    Body,
}

#[derive(Clone, Copy)]
struct LoopDrag {
    handle: LoopHandle,
    orig_x: f32,
    orig_ls: f64,
    orig_le: f64,
}

#[derive(Clone, Copy)]
struct ViewState {
    ppb: f32,
    scroll_beats: f64,
    key: (usize, usize),
    auto_fades: bool,
    gain: bool,
    pan: bool,
    pitch: bool,
    formant: bool,
    stretch: bool,
    onsets: bool,
    snap_beats: f64,
}

#[derive(Clone)]
struct StretchClipData {
    orig_beats: f64,
    stretch_beats: f64,
    markers: Vec<(f64, f64)>,
    dirty_engine: bool,
}

#[derive(Clone, Copy)]
struct StretchDrag {
    marker_idx: usize,
    orig_x: f32,
    orig_beat: f64,
    orig_total_beats: f64,
    alt_clip_stretch: bool,
}

#[derive(Clone)]
struct OnsetClipData {
    sensitivity: f32,
    onsets: Vec<f32>,
    selected: Option<usize>,
    frames_key: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OnsetDragMode {
    MoveOnset,
    WarpTransient,
}

#[derive(Clone, Copy)]
struct OnsetDrag {
    onset_idx: usize,
    mode: OnsetDragMode,
    orig_x: f32,
    marker_idx: Option<usize>,
    orig_beat: f64,
}

#[derive(Clone)]
struct Env {
    frames: usize,
    ch: Vec<Vec<f32>>,
    energy: Vec<f32>,
}

/// `Some((track, slot))` si el slot seleccionado contiene un clip de audio.
pub fn selected_audio_slot(app: &AppState) -> Option<(usize, usize)> {
    let (t, s) = app.matrix_state.selected_slot?;
    let clip = app.matrix_state.slot(t, s)?.clip.as_ref()?;
    matches!(clip.content, ClipData::Audio { .. }).then_some((t, s))
}

fn detect_onsets(energy: &[f32], sensitivity: f32) -> Vec<f32> {
    let bins = energy.len();
    if bins < 8 {
        return Vec::new();
    }
    let s = sensitivity.clamp(0.0, 1.0);
    let ratio_thresh = 2.8 - s * 1.65;
    let abs_floor = 0.14 * (1.0 - s * 0.88) + 0.008;
    let w = (bins / 256).max(3);
    let look = (w / 2).max(1);
    let min_gap = (bins / 96).max(6);

    let mut onsets = Vec::new();
    let mut last = 0usize;
    let mut i = w;
    while i < bins {
        let prev: f32 = energy[i - w..i].iter().sum::<f32>() / w as f32;
        if energy[i] > prev * ratio_thresh + abs_floor
            && energy[i] >= energy[i - 1]
            && i.saturating_sub(last) >= min_gap
        {
            // Alinear con el pico local del ataque del transiente
            let mut peak_i = i;
            let mut peak_v = energy[i];
            let end = (i + look).min(bins - 1);
            for j in (i + 1)..=end {
                if energy[j] > peak_v {
                    peak_v = energy[j];
                    peak_i = j;
                }
            }
            let norm = (peak_i as f32 / bins as f32).clamp(0.002, 0.998);
            onsets.push(norm);
            last = peak_i;
            i = peak_i + min_gap;
            continue;
        }
        i += 1;
    }
    onsets
}

fn map_beat_to_norm(markers: &[(f64, f64)], beat_off: f64, clip_beats: f64) -> f64 {
    if markers.len() >= 2 {
        for w in markers.windows(2) {
            if beat_off >= w[0].0 && beat_off <= w[1].0 && (w[1].0 - w[0].0) > 1e-9 {
                let t = (beat_off - w[0].0) / (w[1].0 - w[0].0);
                return (w[0].1 + t * (w[1].1 - w[0].1)).clamp(0.0, 1.0);
            }
        }
        if beat_off <= markers[0].0 {
            return markers[0].1;
        }
        if let Some(last) = markers.last() {
            if beat_off >= last.0 {
                return last.1;
            }
        }
    }
    (beat_off / clip_beats.max(1e-6)).clamp(0.0, 1.0)
}

fn map_norm_to_beat(markers: &[(f64, f64)], norm: f64, clip_beats: f64) -> f64 {
    let n = norm.clamp(0.0, 1.0);
    if markers.len() >= 2 {
        for w in markers.windows(2) {
            if n >= w[0].1 && n <= w[1].1 && (w[1].1 - w[0].1) > 1e-9 {
                let t = (n - w[0].1) / (w[1].1 - w[0].1);
                return w[0].0 + t * (w[1].0 - w[0].0);
            }
        }
    }
    n * clip_beats
}

fn insert_or_find_stretch_marker(
    markers: &mut Vec<(f64, f64)>,
    norm: f64,
    clip_beats: f64,
) -> Option<usize> {
    let norm = norm.clamp(0.005, 0.995);
    for (idx, &(_, mn)) in markers.iter().enumerate() {
        if (mn - norm).abs() < 0.006 {
            return if idx > 0 && idx + 1 < markers.len() { Some(idx) } else { None };
        }
    }
    let mut b_off = map_norm_to_beat(markers, norm, clip_beats);
    let mut ins = markers.len().saturating_sub(1);
    for i in 1..markers.len() {
        if markers[i].1 > norm {
            ins = i;
            break;
        }
    }
    if ins == 0 || ins >= markers.len() {
        return None;
    }
    let min_b = markers[ins - 1].0 + 0.015;
    let max_b = markers[ins].0 - 0.015;
    if max_b <= min_b {
        return None;
    }
    b_off = b_off.clamp(min_b, max_b);
    markers.insert(ins, (b_off, norm));
    Some(ins)
}

fn quantize_onsets_to_markers(
    onsets: &[f32],
    stretch_beats: f64,
    grid_beats: f64,
) -> Vec<(f64, f64)> {
    let grid = grid_beats.max(0.0625);
    let mut out = vec![(0.0, 0.0)];
    let mut prev_b = 0.0;
    let mut prev_n = 0.0;
    for &n32 in onsets {
        let n = n32 as f64;
        if n <= 0.015 || n >= 0.985 {
            continue;
        }
        let target_b = ((n * stretch_beats) / grid).round() * grid;
        let clamped_b = target_b.clamp(0.0625, (stretch_beats - 0.0625).max(0.0625));
        if clamped_b > prev_b + 0.04 && n > prev_n + 0.008 && clamped_b < stretch_beats - 0.04 {
            out.push((clamped_b, n));
            prev_b = clamped_b;
            prev_n = n;
        }
    }
    out.push((stretch_beats, 1.0));
    out
}

fn build_env(clip: &MatrixClip) -> Env {
    let Some(ev) = clip.audio_events().first() else {
        return Env { frames: 0, ch: vec![], energy: vec![] };
    };
    let chn = ev.channels.max(1);
    let win = ev.window_samples();
    let frames = win.len() / chn;
    let bins = 4096usize.min(frames.max(1));
    let nch = chn.min(2);
    let mut ch = vec![Vec::with_capacity(bins); nch];
    for b in 0..bins {
        let f0 = b * frames / bins;
        let f1 = ((b + 1) * frames / bins).max(f0 + 1).min(frames);
        for (c, v) in ch.iter_mut().enumerate() {
            let mut m = 0.0f32;
            for f in f0..f1 {
                m = m.max(win[f * chn + c.min(chn - 1)].abs());
            }
            v.push(m);
        }
    }
    let energy: Vec<f32> = (0..bins).map(|i| ch.iter().map(|c| c[i]).fold(0.0, f32::max)).collect();
    Env { frames, ch, energy }
}

pub fn render(ui: &mut egui::Ui, app: &mut AppState, slot: (usize, usize)) {
    let bpm = if app.transport.bpm > 0.0 { app.transport.bpm as f64 } else { 120.0 };
    let spb = 60.0 / bpm;
    let accent = theme::ACCENT_ORANGE;

    let full = ui.available_rect_before_wrap();
    ui.allocate_rect(full, Sense::hover());
    let p = ui.painter_at(full);
    p.rect_filled(full, 0.0, theme::DAW_BG_LAYER_2);
    p.rect_stroke(full.shrink(1.0), 2.0, Stroke::new(2.0, accent), StrokeKind::Inside);
    let inner = full.shrink(3.0);

    let (clip_id, name, path, ev_start, ev_len, sr, loop_en, loop_s, loop_e, track_color, env_key) = {
        let Some(clip) = app.matrix_state.slot(slot.0, slot.1).and_then(|s| s.clip.as_ref()) else { return };
        let ev = clip.audio_events().first();
        let sr = ev.map(|e| e.sample_rate).unwrap_or(44100).max(1) as f64;
        (
            clip.id,
            clip.name.clone(),
            clip.path.clone(),
            ev.map(|e| e.start_secs).unwrap_or(0.0),
            ev.map(|e| e.duration_secs()).unwrap_or(clip.duration_secs),
            sr,
            clip.loop_enabled,
            clip.loop_start as f64 / sr,
            clip.loop_end as f64 / sr,
            clip.color,
            (clip.id, ev.map(|e| e.frames()).unwrap_or(0)),
        )
    };
    let _ = track_color;

    // Caché de envolvente
    let env_id = egui::Id::new("ade_env").with(slot).with(env_key);
    let env: Env = match ui.ctx().data(|d| d.get_temp::<std::sync::Arc<Env>>(env_id)) {
        Some(e) => (*e).clone(),
        None => {
            let e = app
                .matrix_state
                .slot(slot.0, slot.1)
                .and_then(|s| s.clip.as_ref())
                .map(build_env)
                .unwrap_or(Env { frames: 0, ch: vec![], energy: vec![] });
            ui.ctx().data_mut(|d| d.insert_temp(env_id, std::sync::Arc::new(e.clone())));
            e
        }
    };

    let vid = egui::Id::new("ade_view");
    let mut v: ViewState = ui.ctx().data(|d| d.get_temp(vid)).unwrap_or(ViewState {
        ppb: DEFAULT_PPB,
        scroll_beats: -2.4,
        key: (usize::MAX, 0),
        auto_fades: true,
        gain: false,
        pan: false,
        pitch: false,
        formant: false,
        stretch: true,
        onsets: true,
        snap_beats: 0.25,
    });
    if v.key != slot {
        v.key = slot;
        v.ppb = DEFAULT_PPB;
        v.scroll_beats = -2.4;
    }

    // ── Regiones ──
    let strip = Rect::from_min_size(inner.min, vec2(STRIP_W, inner.height()));
    let tools = Rect::from_min_size(pos2(strip.max.x + 2.0, inner.min.y), vec2(TOOLS_W, inner.height()));
    let main = Rect::from_min_max(pos2(tools.max.x + 2.0, inner.min.y), inner.max);
    let ruler = Rect::from_min_size(main.min, vec2(main.width(), RULER_H));
    let loopr = Rect::from_min_size(pos2(main.min.x, ruler.max.y), vec2(main.width(), LOOP_H));
    let scroll_r = Rect::from_min_max(pos2(main.min.x, main.max.y - SCROLL_H), main.max);
    let hdr_top = loopr.max.y + 2.0;
    let wave_top = hdr_top + HEADER_H;
    let wave_r = Rect::from_min_max(pos2(main.min.x, wave_top), pos2(main.max.x, scroll_r.min.y - 2.0));

    let file_source_bpm = crate::views::explorer::parse_bpm_from_filename(&name)
        .or_else(|| path.file_name().and_then(|n| crate::views::explorer::parse_bpm_from_filename(&n.to_string_lossy())));
    let orig_beats = match file_source_bpm {
        Some(sbpm) if sbpm > 0.0 => (ev_len * (sbpm as f64 / 60.0)).max(0.25),
        _ => (ev_len / spb).max(0.25),
    };
    let stretch_id = egui::Id::new("ade_stretch").with(clip_id);
    let mut stretch_data: StretchClipData = ui.ctx().data(|d| d.get_temp(stretch_id)).unwrap_or_else(|| StretchClipData {
        orig_beats,
        stretch_beats: orig_beats,
        markers: vec![(0.0, 0.0), (orig_beats, 1.0)],
        dirty_engine: false,
    });
    if (stretch_data.orig_beats - orig_beats).abs() > 1e-3 && stretch_data.markers.len() <= 2 {
        stretch_data.orig_beats = orig_beats;
        stretch_data.stretch_beats = orig_beats;
        stretch_data.markers = vec![(0.0, 0.0), (orig_beats, 1.0)];
    }

    let onset_id = egui::Id::new("ade_onsets").with(clip_id);
    let mut onset_data: OnsetClipData = ui.ctx().data(|d| d.get_temp(onset_id)).unwrap_or_else(|| OnsetClipData {
        sensitivity: DEFAULT_ONSET_SENS,
        onsets: detect_onsets(&env.energy, DEFAULT_ONSET_SENS),
        selected: None,
        frames_key: env.frames,
    });
    if onset_data.frames_key != env.frames {
        onset_data.onsets = detect_onsets(&env.energy, onset_data.sensitivity);
        onset_data.selected = None;
        onset_data.frames_key = env.frames;
    }

    let mut stretch_changed = false;
    let mut engine_sync_now = false;
    let rescale_stretch_total = |sd: &mut StretchClipData, new_total: f64| {
        let clamped = new_total.clamp(0.25, 256.0);
        let old_total = sd.stretch_beats.max(1e-6);
        let rel_scale = clamped / old_total;
        for m in sd.markers.iter_mut().skip(1) {
            m.0 *= rel_scale;
        }
        if let Some(last) = sd.markers.last_mut() {
            last.0 = clamped;
        }
        sd.stretch_beats = clamped;
    };

    // ── Strip vertical con nombre rotado ──
    p.rect_filled(strip, 0.0, Color32::from_rgb(0x2B, 0x2B, 0x2B));
    let galley = ui.painter().layout_no_wrap(name.to_uppercase(), FontId::proportional(10.5), theme::TEXT_PRIMARY);
    let mut ts = egui::epaint::TextShape::new(pos2(strip.center().x - 6.0, strip.max.y - 28.0), galley, theme::TEXT_PRIMARY);
    ts.angle = -std::f32::consts::FRAC_PI_2;
    p.add(Shape::Text(ts));

    // ── Panel de herramientas ──
    p.rect_filled(tools, 0.0, Color32::from_rgb(0x25, 0x25, 0x25));
    {
        let tools_body = Rect::from_min_max(tools.min + vec2(4.0, 4.0), pos2(tools.max.x - 4.0, tools.max.y - 32.0));
        let mut tui = ui.new_child(egui::UiBuilder::new().max_rect(tools_body));
        tui.set_clip_rect(tools_body);
        tui.spacing_mut().item_spacing = vec2(4.0, 3.0);
        let w = tui.available_width();
        tui.add_space(32.0);
        let _ = tool_btn(&mut tui, "Audio Events", true, w, 22.0, 11.0);
        let _ = tool_btn(&mut tui, "Comping", false, w, 22.0, 11.0);
        tui.horizontal(|ui| {
            let hw = (w - 4.0) / 2.0;
            if tool_btn(ui, "Stretch", v.stretch, hw, 22.0, 11.0).clicked() { v.stretch = !v.stretch; }
            if tool_btn(ui, "Onsets", v.onsets, hw, 22.0, 11.0).clicked() { v.onsets = !v.onsets; }
        });
        tui.horizontal(|ui| {
            let bw = (w - 12.0) / 4.0;
            if tool_btn(ui, "Gain", v.gain, bw + 3.0, 20.0, 10.0).clicked() { v.gain = !v.gain; }
            if tool_btn(ui, "Pan", v.pan, bw - 2.0, 20.0, 10.0).clicked() { v.pan = !v.pan; }
            if tool_btn(ui, "Pitch", v.pitch, bw, 20.0, 10.0).clicked() { v.pitch = !v.pitch; }
            if tool_btn(ui, "Formant", v.formant, bw + 8.0, 20.0, 10.0).clicked() { v.formant = !v.formant; }
        });
        if v.stretch && ev_len > 1e-4 {
            tui.add_space(2.0);
            let mut cur_clip_bpm = ((stretch_data.stretch_beats * 60.0) / ev_len).clamp(20.0, 400.0);
            tui.horizontal(|ui| {
                ui.label(egui::RichText::new("Tempo").size(10.0).color(theme::TEXT_SECONDARY));
                let dv = ui.add(
                    egui::DragValue::new(&mut cur_clip_bpm)
                        .speed(0.25)
                        .range(20.0..=400.0)
                        .suffix(" BPM")
                        .fixed_decimals(1),
                );
                if dv.changed() {
                    let new_total = (cur_clip_bpm * ev_len) / 60.0;
                    rescale_stretch_total(&mut stretch_data, new_total);
                    stretch_changed = true;
                    engine_sync_now = true;
                }
            });
            tui.horizontal(|ui| {
                let bw = (w - 8.0) / 3.0;
                if tool_btn(ui, "÷2", false, bw, 18.0, 10.0).clicked() {
                    let new_total = stretch_data.stretch_beats * 0.5;
                    rescale_stretch_total(&mut stretch_data, new_total);
                    stretch_changed = true;
                    engine_sync_now = true;
                }
                if tool_btn(ui, "×2", false, bw, 18.0, 10.0).clicked() {
                    let new_total = stretch_data.stretch_beats * 2.0;
                    rescale_stretch_total(&mut stretch_data, new_total);
                    stretch_changed = true;
                    engine_sync_now = true;
                }
                if tool_btn(ui, "Reset", false, bw, 18.0, 10.0).clicked() {
                    rescale_stretch_total(&mut stretch_data, orig_beats);
                    stretch_changed = true;
                    engine_sync_now = true;
                }
            });
        }
        if v.onsets {
            tui.add_space(2.0);
            let mut sens_pct = (onset_data.sensitivity * 100.0).clamp(0.0, 100.0);
            tui.horizontal(|ui| {
                ui.label(egui::RichText::new("Onsets").size(10.0).color(theme::TEXT_SECONDARY));
                let dv = ui.add(
                    egui::DragValue::new(&mut sens_pct)
                        .speed(0.5)
                        .range(0.0..=100.0)
                        .suffix("%")
                        .fixed_decimals(0),
                );
                if dv.changed() {
                    onset_data.sensitivity = (sens_pct / 100.0).clamp(0.0, 1.0);
                    onset_data.onsets = detect_onsets(&env.energy, onset_data.sensitivity);
                    onset_data.selected = None;
                }
            });
            tui.horizontal(|ui| {
                let q_resp = tool_btn(ui, "Quantize", false, 56.0, 18.0, 9.5)
                    .on_hover_text("Cuantizar transientes (onsets) a la grilla actual");
                if q_resp.clicked() {
                    stretch_data.markers = quantize_onsets_to_markers(
                        &onset_data.onsets,
                        stretch_data.stretch_beats,
                        v.snap_beats,
                    );
                    v.stretch = true;
                    stretch_changed = true;
                    engine_sync_now = true;
                }
                let d_resp = tool_btn(ui, "Detect", false, (w - 64.0) * 0.54, 18.0, 9.5)
                    .on_hover_text("Re-detectar onsets según la sensibilidad");
                if d_resp.clicked() {
                    onset_data.onsets = detect_onsets(&env.energy, onset_data.sensitivity);
                    onset_data.selected = None;
                }
                let c_resp = tool_btn(ui, "Clear", false, (w - 64.0) * 0.46, 18.0, 9.5)
                    .on_hover_text("Quitar marcadores de warp intermedios");
                if c_resp.clicked() {
                    stretch_data.markers = vec![(0.0, 0.0), (stretch_data.stretch_beats, 1.0)];
                    stretch_changed = true;
                    engine_sync_now = true;
                }
            });
        }
    }
    // Selector de herramienta (flecha)
    let tool_icon = Rect::from_min_size(tools.min + vec2(0.0, 0.0), vec2(40.0, 30.0));
    p.rect_filled(tool_icon, 0.0, Color32::from_rgb(0x3A, 0x3A, 0x3A));
    p.text(tool_icon.center(), Align2::CENTER_CENTER, "➤", FontId::proportional(13.0), theme::TEXT_PRIMARY);
    // Create auto-fades
    let cb_rect = Rect::from_min_size(pos2(tools.min.x + 4.0, tools.max.y - 28.0), vec2(TOOLS_W - 8.0, 22.0));
    let cb = ui.interact(cb_rect, vid.with("autofade"), Sense::click());
    if cb.clicked() { v.auto_fades = !v.auto_fades; }
    let dot = pos2(cb_rect.min.x + 10.0, cb_rect.center().y);
    p.circle_filled(dot, 8.0, if v.auto_fades { accent } else { Color32::from_rgb(0x44, 0x44, 0x44) });
    if v.auto_fades {
        p.add(Shape::line(vec![dot + vec2(-3.5, 0.0), dot + vec2(-1.0, 3.0), dot + vec2(4.0, -3.0)], Stroke::new(2.0, Color32::BLACK)));
    }
    p.text(pos2(dot.x + 14.0, dot.y), Align2::LEFT_CENTER, "Create auto-fades", FontId::proportional(11.0), theme::TEXT_PRIMARY);

    // ── Interacción principal: zoom/pan ──
    let resp = ui.interact(main, vid.with("main"), Sense::click_and_drag());
    if resp.hovered() {
        let (scroll, zoom, mods, hp) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers, i.pointer.hover_pos()));
        let ax = hp.map(|q| q.x).unwrap_or(main.center().x);
        let anchor = v.scroll_beats + ((ax - main.min.x) / v.ppb) as f64;
        let mut f = zoom;
        if mods.ctrl || mods.command { f *= (scroll.y * 0.01).exp(); }
        if (f - 1.0).abs() > 1e-4 {
            v.ppb = (v.ppb * f).clamp(MIN_PPB, MAX_PPB);
            v.scroll_beats = anchor - ((ax - main.min.x) / v.ppb) as f64;
        } else {
            let d = if scroll.x.abs() > 0.0 { scroll.x } else { scroll.y };
            v.scroll_beats -= (d / v.ppb) as f64;
        }
    }
    // Zoom & pan estilo Bitwig (Video 2):
    // - MIDDLE-DRAG en el editor (o DRAG izquierdo en la regla):
    //   vertical (abajo = acercar, arriba = alejar) + horizontal (paneo),
    //   manteniendo el punto clickeado pegado al cursor.
    #[derive(Clone, Copy)]
    struct ZoomPanDrag {
        oy: f32,
        anchor_beat: f64,
        ppb0: f32,
        by_ruler: bool,
    }
    let zoom_id = vid.with("mmb_zoom");
    let mid_start = resp.drag_started_by(egui::PointerButton::Middle);
    let ruler_start = resp.drag_started_by(egui::PointerButton::Primary)
        && ui.input(|i| i.pointer.press_origin().is_some_and(|o| ruler.contains(o)));
    if mid_start || ruler_start {
        if let Some(o) = ui.input(|i| i.pointer.press_origin()) {
            let anchor_beat = v.scroll_beats + ((o.x - main.min.x) / v.ppb) as f64;
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    zoom_id,
                    ZoomPanDrag {
                        oy: o.y,
                        anchor_beat,
                        ppb0: v.ppb,
                        by_ruler: ruler_start && !mid_start,
                    },
                )
            });
        }
    }
    let mut zoom_pan_active = false;
    if let Some(zd) = ui.ctx().data(|d| d.get_temp::<ZoomPanDrag>(zoom_id)) {
        let still_dragging = if zd.by_ruler {
            resp.dragged_by(egui::PointerButton::Primary)
        } else {
            resp.dragged_by(egui::PointerButton::Middle)
        };
        if still_dragging {
            zoom_pan_active = true;
            if let Some(cur) = ui.input(|i| i.pointer.interact_pos().or_else(|| i.pointer.latest_pos())) {
                let dy = cur.y - zd.oy;
                v.ppb = (zd.ppb0 * (dy * 0.01).exp()).clamp(MIN_PPB, MAX_PPB);
                v.scroll_beats = zd.anchor_beat - ((cur.x - main.min.x) / v.ppb) as f64;
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
        } else {
            ui.ctx().data_mut(|d| d.remove::<ZoomPanDrag>(zoom_id));
        }
    }
    let mut clip_beats = stretch_data.stretch_beats.max(0.25);
    let start_beats = ev_start / spb;
    v.scroll_beats = v.scroll_beats.clamp(-8.0, start_beats + clip_beats + 8.0);

    let b2x = |b: f64| main.min.x + ((b - v.scroll_beats) * v.ppb as f64) as f32;
    let x2b = |x: f32| v.scroll_beats + ((x - main.min.x) / v.ppb) as f64;

    // ── Interacción con Beat / Stretch Markers y Onsets (estilo Bitwig) ──
    let sdrag_id = vid.with("stretch_drag");
    let odrag_id = vid.with("onset_drag");
    let mut active_sdrag: Option<StretchDrag> = ui.ctx().data(|d| d.get_temp(sdrag_id));
    let mut active_odrag: Option<OnsetDrag> = ui.ctx().data(|d| d.get_temp(odrag_id));
    let (alt_held, shift_held_s, primary_down_s, hover_pos_s, press_origin_s, interact_pos_s, del_pressed) = ui.input(|i| (
        i.modifiers.alt,
        i.modifiers.shift,
        i.pointer.primary_down(),
        i.pointer.hover_pos(),
        i.pointer.press_origin(),
        i.pointer.interact_pos(),
        i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
    ));
    if !primary_down_s {
        active_sdrag = None;
        active_odrag = None;
        ui.ctx().data_mut(|d| {
            d.remove::<StretchDrag>(sdrag_id);
            d.remove::<OnsetDrag>(odrag_id);
        });
    }

    let clip_vert_zone = |pos: egui::Pos2| pos.y >= hdr_top && pos.y <= wave_r.max.y && main.x_range().contains(pos.x);
    let hit_stretch_marker = |pos: egui::Pos2, markers: &[(f64, f64)]| -> Option<usize> {
        if !v.stretch || !clip_vert_zone(pos) {
            return None;
        }
        let mut best: Option<(usize, f32)> = None;
        for (idx, &(mb, _)) in markers.iter().enumerate().skip(1) {
            let mx = b2x(start_beats + mb);
            let dist = (pos.x - mx).abs();
            if dist <= 8.0 {
                if best.map_or(true, |(_, bd)| dist < bd) {
                    best = Some((idx, dist));
                }
            }
        }
        best.map(|(i, _)| i)
    };

    let hit_onset_handle = |pos: egui::Pos2, onsets: &[f32], markers: &[(f64, f64)], cb: f64, allow_body: bool| -> Option<usize> {
        if !v.onsets || !wave_r.contains(pos) {
            return None;
        }
        let in_top_band = pos.y >= wave_r.min.y && pos.y <= wave_r.min.y + 20.0;
        if !allow_body && !in_top_band {
            return None;
        }
        let mut best: Option<(usize, f32)> = None;
        for (idx, &o) in onsets.iter().enumerate() {
            let ox = b2x(start_beats + map_norm_to_beat(markers, o as f64, cb));
            let dx = pos.x - ox;
            let hit = if in_top_band { dx >= -6.0 && dx <= 11.0 } else { dx.abs() <= 7.0 };
            if hit {
                let dist = dx.abs();
                if best.map_or(true, |(_, bd)| dist < bd) {
                    best = Some((idx, dist));
                }
            }
        }
        best.map(|(i, _)| i)
    };

    if v.onsets && del_pressed {
        if let Some(sel_idx) = onset_data.selected {
            if sel_idx < onset_data.onsets.len() && hover_pos_s.is_some_and(|p| main.contains(p)) {
                onset_data.onsets.remove(sel_idx);
                onset_data.selected = None;
            }
        }
    }

    if v.onsets && resp.secondary_clicked() {
        if let Some(pos) = hover_pos_s {
            if let Some(oidx) = hit_onset_handle(pos, &onset_data.onsets, &stretch_data.markers, clip_beats, !v.stretch || alt_held) {
                onset_data.onsets.remove(oidx);
                onset_data.selected = None;
            }
        }
    }

    if resp.double_clicked_by(egui::PointerButton::Primary) {
        if let Some(pos) = hover_pos_s {
            if wave_r.contains(pos) {
                if v.stretch {
                    if let Some(idx) = hit_stretch_marker(pos, &stretch_data.markers) {
                        if idx > 0 && idx + 1 < stretch_data.markers.len() {
                            stretch_data.markers.remove(idx);
                            stretch_changed = true;
                            engine_sync_now = true;
                        }
                    } else if let Some(oidx) = hit_onset_handle(pos, &onset_data.onsets, &stretch_data.markers, clip_beats, alt_held) {
                        if alt_held {
                            onset_data.onsets.remove(oidx);
                            onset_data.selected = None;
                        } else {
                            let norm = onset_data.onsets[oidx] as f64;
                            if insert_or_find_stretch_marker(&mut stretch_data.markers, norm, clip_beats).is_some() {
                                stretch_changed = true;
                                engine_sync_now = true;
                            }
                        }
                    } else {
                        let b_off = (x2b(pos.x) - start_beats).clamp(0.05, clip_beats - 0.05);
                        let norm = map_beat_to_norm(&stretch_data.markers, b_off, clip_beats);
                        if insert_or_find_stretch_marker(&mut stretch_data.markers, norm, clip_beats).is_some() {
                            stretch_changed = true;
                            engine_sync_now = true;
                        }
                    }
                } else if v.onsets {
                    if let Some(oidx) = hit_onset_handle(pos, &onset_data.onsets, &stretch_data.markers, clip_beats, true) {
                        onset_data.onsets.remove(oidx);
                        onset_data.selected = None;
                    } else {
                        let b_off = (x2b(pos.x) - start_beats).clamp(0.0, clip_beats);
                        let new_norm = (map_beat_to_norm(&stretch_data.markers, b_off, clip_beats) as f32).clamp(0.002, 0.998);
                        if onset_data.onsets.iter().all(|&o| (o - new_norm).abs() > 0.002) {
                            onset_data.onsets.push(new_norm);
                            onset_data.onsets.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                            onset_data.selected = onset_data.onsets.iter().position(|&o| (o - new_norm).abs() < 1e-6);
                        }
                    }
                }
            }
        }
    }

    if resp.drag_started_by(egui::PointerButton::Primary) {
        if let Some(orig) = press_origin_s {
            if let Some(m_idx) = hit_stretch_marker(orig, &stretch_data.markers) {
                let sd = StretchDrag {
                    marker_idx: m_idx,
                    orig_x: orig.x,
                    orig_beat: stretch_data.markers[m_idx].0,
                    orig_total_beats: stretch_data.stretch_beats,
                    alt_clip_stretch: false,
                };
                active_sdrag = Some(sd);
                ui.ctx().data_mut(|d| d.insert_temp(sdrag_id, sd));
            } else if let Some(o_idx) = hit_onset_handle(orig, &onset_data.onsets, &stretch_data.markers, clip_beats, !v.stretch || alt_held) {
                onset_data.selected = Some(o_idx);
                let orig_norm = onset_data.onsets[o_idx];
                if !v.stretch || alt_held {
                    let od = OnsetDrag {
                        onset_idx: o_idx,
                        mode: OnsetDragMode::MoveOnset,
                        orig_x: orig.x,
                        marker_idx: None,
                        orig_beat: 0.0,
                    };
                    active_odrag = Some(od);
                    ui.ctx().data_mut(|d| d.insert_temp(odrag_id, od));
                } else if let Some(m_idx) = insert_or_find_stretch_marker(&mut stretch_data.markers, orig_norm as f64, clip_beats) {
                    let od = OnsetDrag {
                        onset_idx: o_idx,
                        mode: OnsetDragMode::WarpTransient,
                        orig_x: orig.x,
                        marker_idx: Some(m_idx),
                        orig_beat: stretch_data.markers[m_idx].0,
                    };
                    active_odrag = Some(od);
                    ui.ctx().data_mut(|d| d.insert_temp(odrag_id, od));
                    stretch_changed = true;
                }
            } else if v.stretch && alt_held && wave_r.contains(orig) {
                let sd = StretchDrag {
                    marker_idx: stretch_data.markers.len().saturating_sub(1),
                    orig_x: orig.x,
                    orig_beat: stretch_data.stretch_beats,
                    orig_total_beats: stretch_data.stretch_beats,
                    alt_clip_stretch: true,
                };
                active_sdrag = Some(sd);
                ui.ctx().data_mut(|d| d.insert_temp(sdrag_id, sd));
            }
        }
    }

    if let (Some(sd), Some(cur)) = (active_sdrag, interact_pos_s) {
        if resp.dragged_by(egui::PointerButton::Primary) {
            let snap_b = v.snap_beats;
            let snap = |b: f64| if shift_held_s { b } else { (b / snap_b).round() * snap_b };
            let delta_b = (cur.x - sd.orig_x) as f64 / v.ppb as f64;
            let last_idx = stretch_data.markers.len().saturating_sub(1);
            if sd.alt_clip_stretch || sd.marker_idx == last_idx {
                let new_total = snap(sd.orig_total_beats + delta_b).clamp(orig_beats * 0.25, orig_beats * 4.0).max(0.25);
                let old_total = stretch_data.stretch_beats.max(1e-6);
                let rel_scale = new_total / old_total;
                for m in stretch_data.markers.iter_mut().skip(1) {
                    m.0 *= rel_scale;
                }
                if let Some(last) = stretch_data.markers.last_mut() {
                    last.0 = new_total;
                }
                stretch_data.stretch_beats = new_total;
                clip_beats = new_total;
                stretch_changed = true;
            } else if sd.marker_idx > 0 && sd.marker_idx < last_idx {
                let min_b = stretch_data.markers[sd.marker_idx - 1].0 + 0.04;
                let max_b = stretch_data.markers[sd.marker_idx + 1].0 - 0.04;
                let new_b = snap(sd.orig_beat + delta_b).clamp(min_b, max_b);
                stretch_data.markers[sd.marker_idx].0 = new_b;
                stretch_changed = true;
            }
        }
    }

    if let (Some(od), Some(cur)) = (active_odrag, interact_pos_s) {
        if resp.dragged_by(egui::PointerButton::Primary) {
            match od.mode {
                OnsetDragMode::MoveOnset => {
                    if od.onset_idx < onset_data.onsets.len() {
                        let b_off = (x2b(cur.x) - start_beats).clamp(0.0, clip_beats);
                        let raw_norm = map_beat_to_norm(&stretch_data.markers, b_off, clip_beats) as f32;
                        let lo = if od.onset_idx > 0 { onset_data.onsets[od.onset_idx - 1] + 0.002 } else { 0.002 };
                        let hi = if od.onset_idx + 1 < onset_data.onsets.len() {
                            onset_data.onsets[od.onset_idx + 1] - 0.002
                        } else {
                            0.998
                        };
                        onset_data.onsets[od.onset_idx] = raw_norm.clamp(lo, hi.max(lo));
                    }
                }
                OnsetDragMode::WarpTransient => {
                    if let Some(m_idx) = od.marker_idx {
                        let last_idx = stretch_data.markers.len().saturating_sub(1);
                        if m_idx > 0 && m_idx < last_idx {
                            let snap_b = v.snap_beats;
                            let snap = |b: f64| if shift_held_s { b } else { (b / snap_b).round() * snap_b };
                            let delta_b = (cur.x - od.orig_x) as f64 / v.ppb as f64;
                            let min_b = stretch_data.markers[m_idx - 1].0 + 0.03;
                            let max_b = stretch_data.markers[m_idx + 1].0 - 0.03;
                            let new_b = snap(od.orig_beat + delta_b).clamp(min_b, max_b);
                            stretch_data.markers[m_idx].0 = new_b;
                            stretch_changed = true;
                        }
                    }
                }
            }
        }
    }

    if stretch_changed {
        stretch_data.dirty_engine = true;
        let stretched_dur_secs = stretch_data.stretch_beats * spb;
        let was_full_loop = (loop_e - loop_s - ev_len).abs() < 0.05 || loop_e <= loop_s;
        if let Some(clip) = app.matrix_state.slot_mut(slot.0, slot.1).and_then(|s| s.clip.as_mut()) {
            clip.duration_secs = stretched_dur_secs;
            if was_full_loop {
                clip.loop_start = 0;
                clip.loop_end = (stretched_dur_secs * sr).round().max(1.0) as u64;
            }
        }
    }

    if stretch_data.dirty_engine && (engine_sync_now || !primary_down_s) {
        let effective_source_bpm = if ev_len > 1e-6 {
            (stretch_data.stretch_beats * 60.0) / ev_len
        } else {
            bpm
        };
        let total_b = stretch_data.stretch_beats.max(1e-6);
        let norm_markers: Vec<(f64, f64)> = stretch_data
            .markers
            .iter()
            .map(|&(b, n)| ((b / total_b).clamp(0.0, 1.0), n.clamp(0.0, 1.0)))
            .collect();
        app.audio_proxy.send(crate::audio_proxy::GuiCommand::SetClipWarpMarkers {
            clip_id,
            source_bpm: Some(effective_source_bpm as f32),
            warp_markers: norm_markers,
        });
        stretch_data.dirty_engine = false;
    }

    ui.ctx().data_mut(|d| {
        d.insert_temp(stretch_id, stretch_data.clone());
        d.insert_temp(onset_id, onset_data.clone());
    });

    let mut loop_en = loop_en;
    let clip_start = start_beats.max(0.0);
    let clip_end = (start_beats + clip_beats).max(clip_start);
    let (mut ls, mut le) = if loop_e > loop_s {
        ((loop_s / spb).clamp(clip_start, clip_end), (loop_e / spb).clamp(clip_start, clip_end))
    } else {
        (clip_start, clip_end)
    };
    if le <= ls {
        ls = clip_start;
        le = clip_end;
    }

    // ── Interacción con los corchetes y barra de loopeo ──
    let drag_id = vid.with("loop_drag");
    let mut active_drag = ui.ctx().data(|d| d.get_temp::<LoopDrag>(drag_id));
    let (shift_held, primary_down, hover_pos, press_origin, interact_pos) = ui.input(|i| (
        i.modifiers.shift,
        i.pointer.primary_down(),
        i.pointer.hover_pos(),
        i.pointer.press_origin(),
        i.pointer.interact_pos(),
    ));
    if !primary_down {
        active_drag = None;
        ui.ctx().data_mut(|d| d.remove::<LoopDrag>(drag_id));
    }

    let hit_loop_handle = |pos: egui::Pos2, lx0: f32, lx1: f32| -> LoopHandle {
        if !loopr.contains(pos) {
            return LoopHandle::None;
        }
        let tri_y = loopr.min.y + 22.0;
        let start_bar = Rect::from_min_max(pos2(lx0 - 8.0, loopr.min.y), pos2(lx0 + 8.0, loopr.min.y + 21.0));
        let start_tri = Rect::from_min_max(pos2(lx0 - 4.0, tri_y - 2.0), pos2(lx0 + 14.0, tri_y + 16.0));
        let end_bar = Rect::from_min_max(pos2(lx1 - 8.0, loopr.min.y), pos2(lx1 + 8.0, loopr.min.y + 21.0));
        let end_tri = Rect::from_min_max(pos2(lx1 - 14.0, tri_y - 2.0), pos2(lx1 + 4.0, tri_y + 16.0));
        let hit_start = start_bar.contains(pos) || start_tri.contains(pos);
        let hit_end = end_bar.contains(pos) || end_tri.contains(pos);
        if hit_start && hit_end {
            if (pos.x - lx0).abs() <= (pos.x - lx1).abs() {
                LoopHandle::Start
            } else {
                LoopHandle::End
            }
        } else if hit_end {
            LoopHandle::End
        } else if hit_start {
            LoopHandle::Start
        } else if (lx1 - lx0) > 16.0 {
            let body_hit = Rect::from_min_max(pos2(lx0 + 8.0, loopr.min.y + 2.0), pos2(lx1 - 8.0, loopr.min.y + 20.0));
            if body_hit.contains(pos) { LoopHandle::Body } else { LoopHandle::None }
        } else {
            LoopHandle::None
        }
    };

    let lx0_cur = b2x(ls);
    let lx1_cur = b2x(le);

    let mut loop_changed = false;
    if resp.double_clicked_by(egui::PointerButton::Primary) {
        if let Some(pos) = hover_pos {
            let bar_hit = Rect::from_min_max(pos2(lx0_cur - 6.0, loopr.min.y), pos2(lx1_cur + 6.0, loopr.min.y + 21.0));
            if bar_hit.contains(pos) {
                loop_en = !loop_en;
                loop_changed = true;
            }
        }
    }

    if resp.drag_started_by(egui::PointerButton::Primary) {
        if let Some(orig) = press_origin {
            let handle = hit_loop_handle(orig, lx0_cur, lx1_cur);
            if handle != LoopHandle::None {
                let d = LoopDrag { handle, orig_x: orig.x, orig_ls: ls, orig_le: le };
                active_drag = Some(d);
                ui.ctx().data_mut(|data| data.insert_temp(drag_id, d));
            }
        }
    }

    if let (Some(d), Some(cur)) = (active_drag, interact_pos) {
        if resp.dragged_by(egui::PointerButton::Primary) && clip_end > clip_start {
            let snap_beats = v.snap_beats;
            let snap = |b: f64| if shift_held { b } else { (b / snap_beats).round() * snap_beats };
            let min_len = 0.25_f64.min((clip_end - clip_start) * 0.5).max(0.01);
            let delta_b = (cur.x - d.orig_x) as f64 / v.ppb as f64;
            match d.handle {
                LoopHandle::Start => {
                    let max_ls = (d.orig_le - min_len).max(clip_start);
                    ls = snap(d.orig_ls + delta_b).clamp(clip_start, max_ls);
                    le = d.orig_le;
                    loop_en = true;
                    loop_changed = true;
                }
                LoopHandle::End => {
                    let min_le = (d.orig_ls + min_len).min(clip_end);
                    ls = d.orig_ls;
                    le = snap(d.orig_le + delta_b).clamp(min_le, clip_end);
                    loop_en = true;
                    loop_changed = true;
                }
                LoopHandle::Body => {
                    let len = (d.orig_le - d.orig_ls).max(min_len);
                    let max_ls = (clip_end - len).max(clip_start);
                    ls = snap(d.orig_ls + delta_b).clamp(clip_start, max_ls);
                    le = (ls + len).min(clip_end);
                    loop_changed = true;
                }
                LoopHandle::None => {}
            }
        }
    }

    if loop_changed {
        let start_secs = (ls * spb).max(0.0);
        let end_secs = (le * spb).max(start_secs);
        let start_frames = (start_secs * sr).round() as u64;
        let end_frames = ((end_secs * sr).round() as u64).max(start_frames + 1);
        if let Some(clip) = app.matrix_state.slot_mut(slot.0, slot.1).and_then(|s| s.clip.as_mut()) {
            clip.loop_start = start_frames;
            clip.loop_end = end_frames;
            clip.loop_enabled = loop_en;
        }
        app.audio_proxy.set_clip_loop(slot.0, slot.1, start_secs as f32, end_secs as f32, loop_en);
    }

    let hovered_onset = active_odrag
        .map(|od| od.onset_idx)
        .or_else(|| {
            if active_sdrag.is_some() {
                None
            } else {
                hover_pos.and_then(|p| hit_onset_handle(p, &onset_data.onsets, &stretch_data.markers, clip_beats, !v.stretch || alt_held))
            }
        });

    let hovered_handle = active_drag
        .map(|d| d.handle)
        .unwrap_or_else(|| hover_pos.map(|p| hit_loop_handle(p, lx0_cur, lx1_cur)).unwrap_or(LoopHandle::None));

    match hovered_handle {
        LoopHandle::Start => {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            crate::app::set_footer_cues(app, None, &[("DRAG", "Adjust loop start"), ("SHIFT", "Disable snapping")]);
        }
        LoopHandle::End => {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            crate::app::set_footer_cues(app, None, &[("DRAG", "Adjust loop end"), ("SHIFT", "Disable snapping")]);
        }
        LoopHandle::Body => {
            ui.ctx().set_cursor_icon(if active_drag.is_some() {
                egui::CursorIcon::Grabbing
            } else {
                egui::CursorIcon::Grab
            });
            crate::app::set_footer_cues(
                app,
                None,
                &[("DRAG", "Move loop region"), ("SHIFT", "Disable snapping"), ("DOUBLE-CLICK", "Toggle Loop")],
            );
        }
        LoopHandle::None => {
            if zoom_pan_active {
                crate::app::set_footer_cues(app, None, &[("DRAG", "Zoom & pan")]);
            } else if hovered_onset.is_some() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                if !v.stretch || alt_held {
                    crate::app::set_footer_cues(
                        app,
                        None,
                        &[("DRAG", "Move onset"), ("DOUBLE-CLICK", "Delete onset"), ("RIGHT-CLICK", "Delete onset")],
                    );
                } else {
                    crate::app::set_footer_cues(
                        app,
                        None,
                        &[("DRAG", "Warp transient"), ("ALT+DRAG", "Move onset"), ("DOUBLE-CLICK", "Pin marker"), ("SHIFT", "Disable snapping")],
                    );
                }
            } else if let Some(pos) = hover_pos {
                if ruler.contains(pos) {
                    crate::app::set_footer_cues(app, None, &[("DRAG", "Zoom & pan")]);
                } else if v.onsets && !v.stretch && wave_r.contains(pos) {
                    crate::app::set_footer_cues(
                        app,
                        None,
                        &[("DOUBLE-CLICK", "Add onset"), ("MIDDLE-DRAG", "Zoom & pan")],
                    );
                } else if main.contains(pos) {
                    crate::app::set_footer_cues(
                        app,
                        None,
                        &[("DRAG", "Select time"), ("ALT+DRAG", "Slide Content"), ("MIDDLE-DRAG", "Zoom & pan")],
                    );
                }
            }
        }
    }

    let mp = ui.painter_at(main);
    // Fondos
    mp.rect_filled(ruler, 0.0, Color32::from_rgb(0x2A, 0x2A, 0x2A));
    mp.rect_filled(loopr, 0.0, Color32::from_rgb(0x30, 0x30, 0x30));
    mp.rect_filled(Rect::from_min_max(pos2(main.min.x, hdr_top), pos2(main.max.x, wave_r.max.y)), 0.0, Color32::from_rgb(0x38, 0x38, 0x38));

    // Grilla + regla
    let (b0, b1) = (x2b(main.min.x).floor() as i64, x2b(main.max.x).ceil() as i64);
    let show_beats = v.ppb > 18.0;
    let show_sub = v.ppb > 120.0;
    for b in b0..=b1 {
        let x = b2x(b as f64);
        let bar_i = b.div_euclid(4);
        let bar = if bar_i >= 0 { bar_i + 1 } else { bar_i };
        let beat = b.rem_euclid(4) + 1;
        let is_bar = beat == 1;
        let col = if is_bar { Color32::from_rgb(0x1A, 0x1A, 0x1A) } else { Color32::from_rgb(0x30, 0x30, 0x30) };
        if is_bar || show_beats {
            mp.line_segment([pos2(x, ruler.min.y), pos2(x, wave_r.max.y)], Stroke::new(1.0, col));
        }
        if is_bar || (show_beats && v.ppb > 40.0) {
            mp.text(
                pos2(x + 4.0, ruler.center().y),
                Align2::LEFT_CENTER,
                format!("{}.{}", bar, beat),
                FontId::proportional(11.0),
                if is_bar { theme::TEXT_PRIMARY } else { theme::TEXT_SECONDARY },
            );
        }
        if show_sub {
            for s in 1..4 {
                let xs = b2x(b as f64 + s as f64 / 4.0);
                mp.line_segment([pos2(xs, wave_r.min.y), pos2(xs, wave_r.max.y)], Stroke::new(1.0, Color32::from_rgb(0x2F, 0x2F, 0x2F)));
            }
        }
    }

    // Clip
    let cx0 = b2x(start_beats);
    let cx1 = b2x(start_beats + clip_beats);
    let hdr = Rect::from_min_max(pos2(cx0, hdr_top), pos2(cx1, wave_top));
    let body = Rect::from_min_max(pos2(cx0, wave_top), pos2(cx1, wave_r.max.y));
    if cx1 > main.min.x && cx0 < main.max.x {
        mp.rect_filled(hdr, 0.0, HEADER_BG);
        mp.rect_filled(Rect::from_min_size(hdr.min, vec2(hdr.width(), 3.0)), 0.0, accent);
        let label = ui.painter_at(hdr.intersect(main));
        label.text(
            pos2(hdr.min.x.max(main.min.x) + 4.0, hdr.center().y + 2.0),
            Align2::LEFT_CENTER,
            &name,
            FontId::proportional(11.0),
            Color32::from_rgb(0x20, 0x20, 0x20),
        );
        mp.rect_filled(body, 0.0, WAVE_BG);

        let vis = body.intersect(main);
        let wp = ui.painter_at(vis);
        let nch = env.ch.len().max(1);
        let lane_h = body.height() / nch as f32;
        let bins = env.ch.first().map(|c| c.len()).unwrap_or(0);
        if bins > 0 && body.width() > 1.0 {
            let mut x = vis.min.x.floor();
            while x < vis.max.x {
                let fa = map_beat_to_norm(&stretch_data.markers, x2b(x) - start_beats, clip_beats) as f32;
                let fb = map_beat_to_norm(&stretch_data.markers, x2b(x + 1.0) - start_beats, clip_beats) as f32;
                let (fa, fb) = if fa <= fb { (fa, fb) } else { (fb, fa) };
                let i0 = ((fa * bins as f32) as usize).min(bins - 1);
                let i1 = ((fb * bins as f32).ceil() as usize).clamp(i0 + 1, bins);
                for (c, chv) in env.ch.iter().enumerate() {
                    let m = chv[i0..i1].iter().cloned().fold(0.0f32, f32::max).min(1.0);
                    let cy = body.min.y + lane_h * (c as f32 + 0.5);
                    let h = (m * lane_h * 0.48).max(0.5);
                    wp.line_segment([pos2(x + 0.5, cy - h), pos2(x + 0.5, cy + h)], Stroke::new(1.0, WAVE_FG));
                }
                x += 1.0;
            }
            for c in 1..nch {
                let y = body.min.y + lane_h * c as f32;
                wp.line_segment([pos2(vis.min.x, y), pos2(vis.max.x, y)], Stroke::new(1.0, Color32::from_black_alpha(40)));
            }
        }
        // Stretch / Beat Markers (estilo Bitwig)
        if v.stretch {
            let hovered_m = active_sdrag
                .map(|sd| sd.marker_idx)
                .or_else(|| hover_pos_s.and_then(|p| hit_stretch_marker(p, &stretch_data.markers)));
            if let Some(m_idx) = hovered_m {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                if m_idx + 1 == stretch_data.markers.len() {
                    crate::app::set_footer_cues(app, None, &[("DRAG", "Stretch audio clip"), ("SHIFT", "Disable snapping")]);
                } else {
                    crate::app::set_footer_cues(app, None, &[("DRAG", "Move beat marker"), ("SHIFT", "Disable snapping"), ("DOUBLE-CLICK", "Remove marker")]);
                }
            }
            let marker_blue = Color32::from_rgb(0x3B, 0x99, 0xFC);
            for (idx, &(mb, _)) in stretch_data.markers.iter().enumerate() {
                let mx = b2x(start_beats + mb);
                if mx >= vis.min.x - 8.0 && mx <= vis.max.x + 8.0 {
                    let is_hot = hovered_m == Some(idx);
                    let col = if is_hot { Color32::WHITE } else { marker_blue };
                    wp.line_segment(
                        [pos2(mx, body.min.y), pos2(mx, body.max.y)],
                        Stroke::new(if is_hot { 2.0_f32 } else { 1.2_f32 }, col),
                    );
                    if idx > 0 {
                        wp.add(Shape::convex_polygon(
                            vec![pos2(mx - 5.0, body.max.y), pos2(mx + 5.0, body.max.y), pos2(mx, body.max.y - 8.0)],
                            col,
                            Stroke::NONE,
                        ));
                        wp.add(Shape::convex_polygon(
                            vec![pos2(mx - 5.0, body.min.y), pos2(mx + 5.0, body.min.y), pos2(mx, body.min.y + 8.0)],
                            col,
                            Stroke::NONE,
                        ));
                    }
                }
            }
        }
        // Onsets (transientes interactivos estilo Bitwig)
        if v.onsets {
            for (idx, &o) in onset_data.onsets.iter().enumerate() {
                let x = b2x(start_beats + map_norm_to_beat(&stretch_data.markers, o as f64, clip_beats));
                if x >= vis.min.x - 8.0 && x <= vis.max.x + 8.0 {
                    let is_hot = hovered_onset == Some(idx);
                    let is_sel = onset_data.selected == Some(idx);
                    let flag_col = if is_hot {
                        Color32::WHITE
                    } else if is_sel {
                        ONSET_HOT
                    } else {
                        ONSET_BLUE
                    };
                    let line_col = if is_hot || is_sel {
                        Color32::from_rgba_unmultiplied(0x2F, 0x6F, 0xE0, 150)
                    } else {
                        Color32::from_black_alpha(40)
                    };
                    wp.line_segment(
                        [pos2(x, body.min.y), pos2(x, body.max.y)],
                        Stroke::new(if is_hot || is_sel { 1.4_f32 } else { 1.0_f32 }, line_col),
                    );
                    let sz = if is_hot || is_sel { 8.0 } else { 7.0 };
                    wp.add(Shape::convex_polygon(
                        vec![pos2(x, body.min.y), pos2(x + sz, body.min.y), pos2(x, body.min.y + sz)],
                        flag_col,
                        if is_hot { Stroke::new(1.0_f32, ONSET_BLUE) } else { Stroke::NONE },
                    ));
                }
            }
        }
        // Bordes del clip
        mp.line_segment([pos2(cx0, hdr.min.y), pos2(cx0, body.max.y)], Stroke::new(1.0_f32, Color32::from_black_alpha(120)));
        mp.line_segment([pos2(cx1, hdr.min.y), pos2(cx1, body.max.y)], Stroke::new(1.0_f32, Color32::from_black_alpha(120)));

        // Atenuar regiones fuera del rango de loopeo cuando el loop está activo
        if loop_en {
            let lx0_vis = b2x(ls);
            let lx1_vis = b2x(le);
            if lx0_vis > cx0 + 1.0 {
                let dim_l = Rect::from_min_max(pos2(cx0, body.min.y), pos2(lx0_vis.min(cx1), body.max.y));
                mp.rect_filled(dim_l.intersect(main), 0.0, Color32::from_black_alpha(55));
            }
            if lx1_vis < cx1 - 1.0 {
                let dim_r = Rect::from_min_max(pos2(lx1_vis.max(cx0), body.min.y), pos2(cx1, body.max.y));
                mp.rect_filled(dim_r.intersect(main), 0.0, Color32::from_black_alpha(55));
            }
        }
    }
    let _ = env.frames;

    // Tira de loop: barra de rango + corchetes [ ] + marcadores
    let lx0 = b2x(ls);
    let lx1 = b2x(le);
    let bar = Rect::from_min_max(pos2(lx0, loopr.min.y + 4.0), pos2(lx1, loopr.min.y + 18.0));
    let bar_fill = if loop_en {
        Color32::from_rgba_unmultiplied(0xFF, 0x7A, 0x00, 38)
    } else {
        Color32::from_rgba_unmultiplied(0x50, 0x50, 0x50, 32)
    };
    let bar_col = if loop_en { accent } else { Color32::from_rgb(0x68, 0x68, 0x68) };
    mp.rect_filled(bar, 0.0, bar_fill);
    mp.rect_stroke(bar, 0.0, Stroke::new(1.0_f32, bar_col), StrokeKind::Inside);

    // Corchetes izquierdo [ y derecho ] estilo Bitwig
    let start_col = if hovered_handle == LoopHandle::Start {
        Color32::WHITE
    } else if loop_en {
        accent
    } else {
        Color32::from_rgb(0x88, 0x88, 0x88)
    };
    let end_col = if hovered_handle == LoopHandle::End {
        Color32::WHITE
    } else if loop_en {
        accent
    } else {
        Color32::from_rgb(0x88, 0x88, 0x88)
    };
    let serif = 5.0_f32.min((lx1 - lx0) * 0.35).max(0.0);
    mp.line_segment([pos2(lx0 + 1.0, bar.min.y), pos2(lx0 + 1.0, bar.max.y)], Stroke::new(2.5_f32, start_col));
    mp.line_segment([pos2(lx0, bar.min.y + 1.0), pos2(lx0 + serif, bar.min.y + 1.0)], Stroke::new(2.0_f32, start_col));
    mp.line_segment([pos2(lx0, bar.max.y - 1.0), pos2(lx0 + serif, bar.max.y - 1.0)], Stroke::new(2.0_f32, start_col));

    mp.line_segment([pos2(lx1 - 1.0, bar.min.y), pos2(lx1 - 1.0, bar.max.y)], Stroke::new(2.5_f32, end_col));
    mp.line_segment([pos2(lx1 - serif, bar.min.y + 1.0), pos2(lx1, bar.min.y + 1.0)], Stroke::new(2.0_f32, end_col));
    mp.line_segment([pos2(lx1 - serif, bar.max.y - 1.0), pos2(lx1, bar.max.y - 1.0)], Stroke::new(2.0_f32, end_col));

    let tri = |x: f32, dir: f32, col: Color32| {
        let y = loopr.min.y + 22.0;
        mp.add(Shape::convex_polygon(
            vec![pos2(x, y), pos2(x, y + 14.0), pos2(x + dir * 12.0, y + 7.0)],
            col,
            Stroke::NONE,
        ));
    };
    tri(lx0, 1.0, start_col);
    tri(lx1, -1.0, if hovered_handle == LoopHandle::End { Color32::WHITE } else { Color32::from_rgb(0x6A, 0x6A, 0x6A) });

    // Playhead interno del clip (cuando este clip está reproduciéndose)
    if app.is_playing() {
        if let Some(frame) = app.clip_playhead_frame(slot.0, slot.1) {
            let engine_sr = app.transport.sample_rate.get().max(1.0) as f64;
            let playhead_beats = (frame as f64 / engine_sr) / spb;
            let px = b2x(playhead_beats);
            if px >= main.min.x && px <= main.max.x {
                mp.line_segment(
                    [pos2(px, ruler.min.y), pos2(px, wave_r.max.y)],
                    Stroke::new(1.5_f32, theme::PLAYHEAD_COLOR),
                );
                mp.add(Shape::convex_polygon(
                    vec![
                        pos2(px - 4.5, ruler.min.y),
                        pos2(px + 4.5, ruler.min.y),
                        pos2(px, ruler.min.y + 8.0),
                    ],
                    theme::PLAYHEAD_COLOR,
                    Stroke::NONE,
                ));
            }
        }
    }

    // Scrollbar + resolución de grilla interactiva
    mp.rect_filled(scroll_r, 0.0, Color32::from_rgb(0x22, 0x22, 0x22));
    let total_b = (start_beats + clip_beats + 16.0) - (-8.0);
    let vis_b = (main.width() / v.ppb) as f64;
    let tw = (scroll_r.width() - 60.0).max(20.0);
    let th = ((vis_b / total_b) as f32 * tw).clamp(24.0, tw);
    let tx = scroll_r.min.x + 14.0 + (((v.scroll_beats + 8.0) / total_b) as f32 * tw).clamp(0.0, tw - th);
    mp.rect_filled(Rect::from_min_size(pos2(tx, scroll_r.min.y + 4.0), vec2(th, 10.0)), 5.0, Color32::from_rgb(0x5A, 0x5A, 0x5A));
    let snap_r = Rect::from_min_max(pos2(scroll_r.max.x - 56.0, scroll_r.min.y + 1.0), pos2(scroll_r.max.x - 4.0, scroll_r.max.y - 1.0));
    let snap_hot = hover_pos.is_some_and(|p| snap_r.contains(p));
    if snap_hot {
        mp.rect_filled(snap_r, 3.0, Color32::from_rgb(0x36, 0x36, 0x36));
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        if resp.clicked() {
            v.snap_beats = if (v.snap_beats - 1.0).abs() < 1e-3 {
                0.5
            } else if (v.snap_beats - 0.5).abs() < 1e-3 {
                0.25
            } else if (v.snap_beats - 0.25).abs() < 1e-3 {
                0.125
            } else {
                1.0
            };
        }
    }
    let snap_label = if (v.snap_beats - 1.0).abs() < 1e-3 {
        "1/4 ▴"
    } else if (v.snap_beats - 0.5).abs() < 1e-3 {
        "1/8 ▴"
    } else if (v.snap_beats - 0.25).abs() < 1e-3 {
        "1/16 ▴"
    } else {
        "1/32 ▴"
    };
    mp.text(pos2(snap_r.min.x + 4.0, scroll_r.center().y), Align2::LEFT_CENTER, snap_label, FontId::proportional(11.0), theme::TEXT_PRIMARY);

    ui.ctx().data_mut(|d| d.insert_temp(vid, v));
}

fn tool_btn(ui: &mut egui::Ui, label: &str, active: bool, w: f32, h: f32, size: f32) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(w.max(10.0), h), Sense::click());
    let bg = if active {
        theme::ACCENT_ORANGE
    } else if resp.hovered() {
        Color32::from_rgb(0x5A, 0x5A, 0x5A)
    } else {
        Color32::from_rgb(0x4A, 0x4A, 0x4A)
    };
    ui.painter().rect_filled(r, 3.0, bg);
    ui.painter().text(
        r.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(size),
        if active { Color32::BLACK } else { theme::TEXT_PRIMARY },
    );
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_onsets_finds_transient_peaks_and_respects_sensitivity() {
        let mut energy = vec![0.01f32; 512];
        // Tres transientes bien espaciados
        for &p in &[64usize, 192, 350] {
            energy[p] = 0.9;
            energy[p + 1] = 0.5;
        }
        // Un transiente suave adicional
        energy[440] = 0.14;

        let low_sens = detect_onsets(&energy, 0.1);
        let high_sens = detect_onsets(&energy, 0.95);
        assert!(low_sens.len() >= 3);
        assert!(high_sens.len() > low_sens.len());
        assert!((low_sens[0] - (64.0 / 512.0)).abs() < 0.02);
    }

    #[test]
    fn quantize_and_insert_markers_stay_monotonic() {
        let onsets = vec![0.23_f32, 0.48, 0.77];
        let mut markers = quantize_onsets_to_markers(&onsets, 4.0, 0.25);
        assert!(markers.len() >= 5);
        for w in markers.windows(2) {
            assert!(w[1].0 > w[0].0, "beats must strictly increase: {:?}", markers);
            assert!(w[1].1 > w[0].1, "norms must strictly increase: {:?}", markers);
        }
        let inserted = insert_or_find_stretch_marker(&mut markers, 0.62, 4.0);
        assert!(inserted.is_some());
        for w in markers.windows(2) {
            assert!(w[1].0 > w[0].0);
            assert!(w[1].1 > w[0].1);
        }
    }
}
