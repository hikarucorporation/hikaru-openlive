// crates/hikaru_gui/tests/playlist_clip_trim.rs
//
// Recorte de clips (trimming) en los bordes de la Playlist:
// - Hit test de los bordes (izq/der) y cursor de resize.
// - Borde derecho: cambia la duración (revela/oculta waveform) sin mover el inicio.
// - Borde izquierdo: mueve inicio Y offset interno del audio a la vez, para
//   que el contenido no "salte" dentro del clip.
// - Snap a la grilla y límites (duración mínima, fin del audio disponible).

use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Arc;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    px, point, size, AppContext, Context, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    TestAppContext, Window,
};

use hikaru_gui::app::{AppMode, HikaruApp};
use hikaru_gui::audio_proxy::{AudioProxy, GuiCommand};
use hikaru_gui::views::mixer::Track;
use hikaru_gui::views::playlist::{
    self, audio_span, min_clip_ticks, trim_apply, trim_edge_at, ClipType, PlaylistClip,
    PlaylistState, TrimEdge, TRIM_EDGE_PX,
};

const PPQN: u64 = 960;

fn test_app(window: &mut Window, cx: &mut Context<HikaruApp>) -> HikaruApp {
    let (tx, _rx) = std::sync::mpsc::channel::<GuiCommand>();
    HikaruApp::build(
        window,
        cx,
        AudioProxy::new(tx),
        None,
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU32::new(0.0f32.to_bits())),
        None,
    )
}

/// Clip de audio: `start_ticks`, duración `dur_ticks`, audio de `total_ticks`.
fn audio_clip(id: usize, start_ticks: u64, dur_ticks: u64, total_ticks: u64) -> PlaylistClip {
    PlaylistClip {
        id,
        name: format!("Clip {id}"),
        start_tick: start_ticks * PPQN,
        duration_ticks: dur_ticks * PPQN,
        clip_type: ClipType::Audio {
            sample_path: format!("clip{id}.wav"),
            peaks: vec![0.5; 64],
            sample_offset_ticks: 0,
            total_sample_ticks: total_ticks * PPQN,
        },
        color: gpui_kit::rgb(0x205F91).into(),
    }
}

/// Un clip en la pista 1: empieza en el tick 0, dura 2 negras, audio de 8.
fn base_state() -> PlaylistState {
    let mut pl = PlaylistState::default();
    pl.clips = vec![(1, audio_clip(1, 0, 2, 8))];
    pl.selected_clips = vec![1];
    pl
}

fn open_studio(cx: &mut TestAppContext) -> gpui_kit::WindowHandle<HikaruApp> {
    cx.update(gpui_kit::init);
    let handle = cx.add_window(|window, cx| test_app(window, cx));
    cx.simulate_window_resize(handle.into(), size(px(1280.0), px(720.0)));
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.mode = AppMode::OpenStudio;
                while s.studio_tracks.len() < 4 {
                    let next_id = s.studio_tracks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
                    let n = s.studio_tracks.iter().filter(|t| !t.is_master).count();
                    s.studio_tracks
                        .push(Track::new(next_id, format!("TRACK {:02}", n + 1), false));
                }
                s.playlist_state = base_state();
                cx.notify();
            });
        });
    })
    .unwrap();
    handle
}

/// `(start_tick, duration_ticks, sample_offset_ticks)` del clip 1.
fn clip_bounds_state(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
) -> (u64, u64, u64) {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        let mut out = (0u64, 0u64, 0u64);
        app.update(cx, |app, cx| {
            if let Some((_, c)) = app.state.read(cx).playlist_state.clips.first() {
                out = (c.start_tick, c.duration_ticks, match &c.clip_type {
                    ClipType::Audio { sample_offset_ticks, .. } => *sample_offset_ticks,
                    _ => 0,
                });
            }
        });
        out
    })
    .unwrap()
}

fn clip_rect(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) -> (f32, f32, f32) {
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        let b = window.find("pl_clip_1").bounds();
        (
            b.origin.x.as_f32(),
            b.size.width.as_f32(),
            b.origin.y.as_f32() + b.size.height.as_f32() / 2.0,
        )
    })
    .unwrap()
}

fn lmove(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    x: f32,
    y: f32,
    pressed: bool,
) {
    cx.update_window(handle.into(), |_, window, cx| {
        use gpui_kit::InputEvent as _;
        window.dispatch_event(
            MouseMoveEvent {
                position: point(px(x), px(y)),
                pressed_button: if pressed { Some(MouseButton::Left) } else { None },
                modifiers: Default::default(),
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

fn ldown(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext, x: f32, y: f32) {
    cx.update_window(handle.into(), |_, window, cx| {
        use gpui_kit::InputEvent as _;
        window.dispatch_event(
            MouseDownEvent {
                button: MouseButton::Left,
                position: point(px(x), px(y)),
                modifiers: Default::default(),
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

fn lup(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext, x: f32, y: f32) {
    cx.update_window(handle.into(), |_, window, cx| {
        use gpui_kit::InputEvent as _;
        window.dispatch_event(
            MouseUpEvent {
                button: MouseButton::Left,
                position: point(px(x), px(y)),
                modifiers: Default::default(),
                click_count: 1,
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

// =========================================================================
// Hit test de bordes (puro)
// =========================================================================

#[test]
fn edge_hit_test_respects_the_pixel_zone() {
    // Clip de 100px en x=200: bordes en 200 y 300.
    assert_eq!(trim_edge_at(201.0, 200.0, 100.0, TRIM_EDGE_PX), Some(TrimEdge::Left));
    assert_eq!(trim_edge_at(299.0, 200.0, 100.0, TRIM_EDGE_PX), Some(TrimEdge::Right));
    // El centro NO es borde: ahí empieza el drag de mover.
    assert_eq!(trim_edge_at(250.0, 200.0, 100.0, TRIM_EDGE_PX), None);
    assert_eq!(trim_edge_at(230.0, 200.0, 100.0, TRIM_EDGE_PX), None);
    // Fuera del clip: nada.
    assert_eq!(trim_edge_at(150.0, 200.0, 100.0, TRIM_EDGE_PX), None);
}

#[test]
fn narrow_clip_still_has_grabbable_edges() {
    // Un clip de 8px es más angosto que 2× la zona: las zonas se solapan y
    // gana el borde más cercano al centro.
    assert_eq!(trim_edge_at(2.0, 0.0, 8.0, TRIM_EDGE_PX), Some(TrimEdge::Left));
    assert_eq!(trim_edge_at(6.0, 0.0, 8.0, TRIM_EDGE_PX), Some(TrimEdge::Right));
}

#[test]
fn zero_width_clip_has_no_edges() {
    assert_eq!(trim_edge_at(10.0, 10.0, 0.0, TRIM_EDGE_PX), None);
}

// =========================================================================
// Lógica de recorte (puro)
// =========================================================================

#[test]
fn right_edge_shortens_duration_only() {
    // start 0, dur 2 negras, offset 0, audio 8 negras. Borde a la negra 1.
    let (start, dur, offset) = trim_apply(
        0,
        2 * PPQN,
        0,
        8 * PPQN,
        TrimEdge::Right,
        1 * PPQN,
        PPQN,
        min_clip_ticks(PPQN),
    );
    assert_eq!(start, 0, "recortar el final no mueve el inicio");
    assert_eq!(dur, PPQN, "la duración baja a 1 negra");
    assert_eq!(offset, 0, "el offset de audio no se toca");
}

#[test]
fn right_edge_cannot_extend_past_available_audio() {
    // El audio sólo tiene 4 negras: no se puede alargar a 10.
    let (start, dur, _) = trim_apply(
        PPQN,
        2 * PPQN,
        PPQN,
        4 * PPQN,
        TrimEdge::Right,
        10 * PPQN,
        PPQN,
        min_clip_ticks(PPQN),
    );
    assert_eq!(start, PPQN);
    // El sample mide 4 negras y el offset es 1 ⇒ quedan 3 de audio: el clip
    // puede crecer de 2 a 3 negras, no a 9.
    assert_eq!(dur, 3 * PPQN, "acotado al audio disponible (total-offset = 3)");
}

#[test]
fn left_edge_moves_start_and_audio_offset_together() {
    // Trim clásico: el start avanza 1 negra y el offset interno también, así
    // el waveform no salta dentro del clip.
    let (start, dur, offset) = trim_apply(
        0,
        4 * PPQN,
        0,
        8 * PPQN,
        TrimEdge::Left,
        1 * PPQN,
        PPQN,
        min_clip_ticks(PPQN),
    );
    assert_eq!(start, PPQN, "el clip empieza una negra después");
    assert_eq!(dur, 3 * PPQN, "la duración se acorta lo mismo");
    assert_eq!(offset, PPQN, "el offset de audio avanza igual que el start");
}

#[test]
fn left_edge_cannot_go_before_the_sample_start() {
    // Empujar el borde izquierdo "antes de cero" (offset 0) queda acotado: no
    // hay audio anterior al inicio del sample.
    let (start, dur, offset) = trim_apply(
        2 * PPQN,
        4 * PPQN,
        0,
        8 * PPQN,
        TrimEdge::Left,
        0,
        PPQN,
        min_clip_ticks(PPQN),
    );
    assert_eq!(start, 0, "no baja del inicio del audio");
    assert_eq!(offset, 0);
    assert_eq!(dur, 6 * PPQN, "se correspondingly alargó");
}

#[test]
fn left_edge_respects_minimum_duration() {
    // Intentar dejar 0 ticks: se queda en el mínimo, no en 0 ni negativo.
    let min = min_clip_ticks(PPQN);
    let (start, dur, _) = trim_apply(
        4 * PPQN,
        2 * PPQN,
        0,
        8 * PPQN,
        TrimEdge::Left,
        10 * PPQN,
        PPQN,
        min,
    );
    assert!(dur >= min, "la duración nunca baja de {min}, quedó en {dur}");
    assert_eq!(dur, min);
    assert_eq!(start, 6 * PPQN - min);
}

#[test]
fn min_clip_ticks_is_never_zero() {
    assert_eq!(min_clip_ticks(0), 1);
    assert_eq!(min_clip_ticks(4), 1);
    assert_eq!(min_clip_ticks(PPQN), PPQN / 4);
}

#[test]
fn audio_span_helper() {
    assert_eq!(audio_span(PPQN, 4 * PPQN), (PPQN, 5 * PPQN));
    assert_eq!(audio_span(0, 0), (0, 0));
}

// =========================================================================
// Integración: gesto real sobre el borde
// =========================================================================

#[gpui_kit::gpui::test]
fn right_edge_drag_shortens_clip(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    let (x, w, y) = clip_rect(handle, cx);
    let before = clip_bounds_state(handle, cx);
    assert_eq!(before.0, 0);
    assert_eq!(before.1, 2 * PPQN);

    // Agarra el borde derecho y arrastra a la izquierda ~40px.
    ldown(handle, cx, x + w - 2.0, y);
    lmove(handle, cx, x + w - 42.0, y, true);
    lup(handle, cx, x + w - 42.0, y);

    let (start, dur, offset) = clip_bounds_state(handle, cx);
    println!("right trim: start={start} dur={dur} offset={offset}");
    assert_eq!(start, 0, "el inicio no se mueve al recortar el final");
    assert!(dur < before.1, "la duración debe bajar ({} -> {dur})", before.1);
    assert_eq!(offset, 0, "el offset de audio no cambia");
}

#[gpui_kit::gpui::test]
fn left_edge_drag_moves_start_and_offset(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    let (x, _w, y) = clip_rect(handle, cx);
    let (start0, dur0, off0) = clip_bounds_state(handle, cx);

    ldown(handle, cx, x + 2.0, y);
    lmove(handle, cx, x + 30.0, y, true);
    lup(handle, cx, x + 30.0, y);

    let (start, dur, offset) = clip_bounds_state(handle, cx);
    println!(
        "left trim: start {start0}->{start}, dur {dur0}->{dur}, offset {off0}->{offset}"
    );
    assert!(start > start0, "el inicio avanza ({start0} -> {start})");
    assert!(dur < dur0, "la duración se acorta ({dur0} -> {dur})");
    assert!(
        offset > off0,
        "el offset interno del audio avanza con el start ({off0} -> {offset})"
    );
    assert_eq!(
        start - start0,
        offset - off0,
        "start y offset deben moverse lo mismo para no desplazar el audio"
    );
}

#[gpui_kit::gpui::test]
fn trimming_snaps_to_grid(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    // Grid 1/16 ⇒ según `snap_step_ticks`, ppqn/4 = 240 ticks.
    const SNAP: u64 = 240;
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("raíz");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.playlist_state.grid_denominator = 16;
                cx.notify();
            });
        });
    });
    let (x, w, y) = clip_rect(handle, cx);
    // Arrastre a mitad de subdivisión: el borde debe caer en un múltiplo de 480.
    ldown(handle, cx, x + w - 2.0, y);
    lmove(handle, cx, x + w - 13.0, y, true);
    lup(handle, cx, x + w - 13.0, y);
    let (_, dur, _) = clip_bounds_state(handle, cx);
    println!("dur tras arrastre = {dur} (snap {SNAP})");
    assert_eq!(dur % SNAP, 0, "el borde debe quedar en la grilla, quedó en {dur}");
}

#[gpui_kit::gpui::test]
fn trimming_never_zeroes_duration(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    let (x, _w, y) = clip_rect(handle, cx);
    // Arrastrar el borde izquierdo muy a la derecha (pasado el final).
    ldown(handle, cx, x + 2.0, y);
    lmove(handle, cx, x + 5000.0, y, true);
    lup(handle, cx, x + 5000.0, y);
    let (start, dur, _) = clip_bounds_state(handle, cx);
    println!("trim extremo: start={start} dur={dur}");
    assert!(dur >= min_clip_ticks(PPQN), "no puede quedar en {dur}");
}

#[gpui_kit::gpui::test]
fn center_drag_moves_clip_not_trims(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    let (x, w, y) = clip_rect(handle, cx);
    let before = clip_bounds_state(handle, cx);
    // Agarra el CENTRO (fuera de las zonas de borde) y mueve.
    ldown(handle, cx, x + w / 2.0, y);
    lmove(handle, cx, x + w / 2.0 + 40.0, y, true);
    lup(handle, cx, x + w / 2.0 + 40.0, y);
    let (start, dur, offset) = clip_bounds_state(handle, cx);
    println!(
        "center drag: start {}->{}, dur {}->{}, offset {}->{}",
        before.0, start, before.1, dur, before.2, offset
    );
    assert_eq!(dur, before.1, "la duración no cambia al MOVER el clip");
    assert_eq!(offset, before.2, "el offset no cambia al mover");
}

#[gpui_kit::gpui::test]
fn edge_handles_are_visible(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    let ok = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            window.try_find("pl_trim_left_1").is_some()
                && window.try_find("pl_trim_right_1").is_some()
        })
        .unwrap();
    assert!(ok, "el clip debería exponer los dos handles de recorte");
}