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
const MIN_PPB: f32 = 8.0;
const MAX_PPB: f32 = 1200.0;
const DEFAULT_PPB: f32 = 76.0;

const WAVE_BG: Color32 = Color32::from_rgb(0xC9, 0xC9, 0xC9);
const WAVE_FG: Color32 = Color32::from_rgb(0x2E, 0x2E, 0x2E);
const HEADER_BG: Color32 = Color32::from_rgb(0xA9, 0xAC, 0xAE);
const OUTSIDE_BG: Color32 = Color32::from_rgb(0x3A, 0x3A, 0x3A);
const ONSET_BLUE: Color32 = Color32::from_rgb(0x2F, 0x6F, 0xE0);

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
}

#[derive(Clone)]
struct Env {
    frames: usize,
    ch: Vec<Vec<f32>>,
    onsets: Vec<f32>,
}

/// `Some((track, slot))` si el slot seleccionado contiene un clip de audio.
pub fn selected_audio_slot(app: &AppState) -> Option<(usize, usize)> {
    let (t, s) = app.matrix_state.selected_slot?;
    let clip = app.matrix_state.slot(t, s)?.clip.as_ref()?;
    matches!(clip.content, ClipData::Audio { .. }).then_some((t, s))
}

fn build_env(clip: &MatrixClip) -> Env {
    let Some(ev) = clip.audio_events().first() else {
        return Env { frames: 0, ch: vec![], onsets: vec![] };
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
    // Detección simple de onsets sobre la envolvente combinada.
    let env: Vec<f32> = (0..bins).map(|i| ch.iter().map(|c| c[i]).fold(0.0, f32::max)).collect();
    let mut onsets = Vec::new();
    let mut last = 0usize;
    let w = (bins / 256).max(2);
    for i in w..bins {
        let prev: f32 = env[i - w..i].iter().sum::<f32>() / w as f32;
        if env[i] > prev * 1.9 + 0.04 && i - last > w * 3 {
            onsets.push(i as f32 / bins as f32);
            last = i;
        }
    }
    Env { frames, ch, onsets }
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

    let (name, ev_start, ev_len, sr, loop_en, loop_s, loop_e, track_color, env_key) = {
        let Some(clip) = app.matrix_state.slot(slot.0, slot.1).and_then(|s| s.clip.as_ref()) else { return };
        let ev = clip.audio_events().first();
        let sr = ev.map(|e| e.sample_rate).unwrap_or(44100).max(1) as f64;
        (
            clip.name.clone(),
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
                .unwrap_or(Env { frames: 0, ch: vec![], onsets: vec![] });
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
        stretch: false,
        onsets: true,
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

    // ── Strip vertical con nombre rotado ──
    p.rect_filled(strip, 0.0, Color32::from_rgb(0x2B, 0x2B, 0x2B));
    let galley = ui.painter().layout_no_wrap(name.to_uppercase(), FontId::proportional(10.5), theme::TEXT_PRIMARY);
    let mut ts = egui::epaint::TextShape::new(pos2(strip.center().x - 6.0, strip.max.y - 28.0), galley, theme::TEXT_PRIMARY);
    ts.angle = -std::f32::consts::FRAC_PI_2;
    p.add(Shape::Text(ts));

    // ── Panel de herramientas ──
    p.rect_filled(tools, 0.0, Color32::from_rgb(0x25, 0x25, 0x25));
    {
        let mut tui = ui.new_child(egui::UiBuilder::new().max_rect(tools.shrink(4.0)));
        tui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        let w = tui.available_width();
        tui.add_space(34.0);
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
    // Zoom estilo Bitwig: click de la rueda + drag horizontal.
    // Derecha = acercar, izquierda = alejar, anclado al punto donde se hizo click.
    let zoom_id = vid.with("mmb_zoom");
    if resp.drag_started_by(egui::PointerButton::Middle) {
        if let Some(o) = ui.input(|i| i.pointer.press_origin()) {
            let anchor_beat = v.scroll_beats + ((o.x - main.min.x) / v.ppb) as f64;
            ui.ctx().data_mut(|d| d.insert_temp(zoom_id, (o.x, anchor_beat, v.ppb)));
        }
    }
    if resp.dragged_by(egui::PointerButton::Middle) {
        if let (Some((ox, abeat, ppb0)), Some(cur)) = (
            ui.ctx().data(|d| d.get_temp::<(f32, f64, f32)>(zoom_id)),
            ui.input(|i| i.pointer.interact_pos()),
        ) {
            let dx = cur.x - ox;
            v.ppb = (ppb0 * (dx * 0.006).exp()).clamp(MIN_PPB, MAX_PPB);
            v.scroll_beats = abeat - ((ox - main.min.x) / v.ppb) as f64;
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    let clip_beats = ev_len / spb;
    let start_beats = ev_start / spb;
    v.scroll_beats = v.scroll_beats.clamp(-8.0, start_beats + clip_beats + 8.0);

    let b2x = |b: f64| main.min.x + ((b - v.scroll_beats) * v.ppb as f64) as f32;
    let x2b = |x: f32| v.scroll_beats + ((x - main.min.x) / v.ppb) as f64;

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
                let fa = ((x - body.min.x) / body.width()).clamp(0.0, 1.0);
                let fb = ((x + 1.0 - body.min.x) / body.width()).clamp(0.0, 1.0);
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
        // Onsets
        if v.onsets {
            for &o in &env.onsets {
                let x = cx0 + o * body.width();
                if x >= vis.min.x - 4.0 && x <= vis.max.x + 4.0 {
                    wp.add(Shape::convex_polygon(
                        vec![pos2(x, body.min.y), pos2(x + 6.0, body.min.y), pos2(x, body.min.y + 6.0)],
                        ONSET_BLUE,
                        Stroke::NONE,
                    ));
                    wp.line_segment([pos2(x, body.min.y), pos2(x, body.max.y)], Stroke::new(1.0, Color32::from_black_alpha(35)));
                }
            }
        }
        // Bordes del clip
        mp.line_segment([pos2(cx0, hdr.min.y), pos2(cx0, body.max.y)], Stroke::new(1.0, Color32::from_black_alpha(120)));
        mp.line_segment([pos2(cx1, hdr.min.y), pos2(cx1, body.max.y)], Stroke::new(1.0, Color32::from_black_alpha(120)));
    }
    let _ = env.frames;

    // Tira de loop: barra de rango + marcadores naranja
    let (ls, le) = if loop_en && loop_e > loop_s { (loop_s / spb, loop_e / spb) } else { (start_beats, start_beats + clip_beats) };
    let lx0 = b2x(ls);
    let lx1 = b2x(le);
    let bar = Rect::from_min_max(pos2(lx0, loopr.min.y + 4.0), pos2(lx1, loopr.min.y + 18.0));
    mp.rect_filled(bar, 0.0, Color32::from_rgb(0x4A, 0x4A, 0x4A));
    mp.rect_stroke(bar, 0.0, Stroke::new(1.0, accent), StrokeKind::Inside);
    let tri = |x: f32, dir: f32, col: Color32| {
        let y = loopr.min.y + 22.0;
        mp.add(Shape::convex_polygon(
            vec![pos2(x, y), pos2(x, y + 14.0), pos2(x + dir * 12.0, y + 7.0)],
            col,
            Stroke::NONE,
        ));
    };
    tri(lx0, 1.0, accent);
    tri(lx1, -1.0, Color32::from_rgb(0x6A, 0x6A, 0x6A));

    // Scrollbar + resolución de grilla
    mp.rect_filled(scroll_r, 0.0, Color32::from_rgb(0x22, 0x22, 0x22));
    let total_b = (start_beats + clip_beats + 16.0) - (-8.0);
    let vis_b = (main.width() / v.ppb) as f64;
    let tw = (scroll_r.width() - 60.0).max(20.0);
    let th = ((vis_b / total_b) as f32 * tw).clamp(24.0, tw);
    let tx = scroll_r.min.x + 14.0 + (((v.scroll_beats + 8.0) / total_b) as f32 * tw).clamp(0.0, tw - th);
    mp.rect_filled(Rect::from_min_size(pos2(tx, scroll_r.min.y + 4.0), vec2(th, 10.0)), 5.0, Color32::from_rgb(0x5A, 0x5A, 0x5A));
    mp.text(pos2(scroll_r.max.x - 52.0, scroll_r.center().y), Align2::LEFT_CENTER, "1/16 ▴", FontId::proportional(11.0), theme::TEXT_PRIMARY);

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
