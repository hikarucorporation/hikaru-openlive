// crates/hikaru_gui/tests/playlist_clip_edit.rs
//
// Edición de clips de la Playlist / Timeline:
// - Atajos `Ctrl+C/X/V/D` (copiar / cortar / pegar / duplicar) sobre la
//   selección, con buffer interno y sincronización al motor.
// - Marquee: click derecho + arrastrar dibuja la caja y selecciona lo que
//   toca; click derecho simple sin arrastre NO selecciona (deja el menú).

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
    self, ClipType, MarqueeState, PlaylistClip, PlaylistState, MARQUEE_DRAG_THRESHOLD,
};

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

fn clip(id: usize, track: usize, start_ticks: u64, dur_ticks: u64, ppqn: u64) -> (usize, PlaylistClip) {
    (
        track,
        PlaylistClip {
            id,
            name: format!("Clip {id}"),
            start_tick: start_ticks * ppqn,
            duration_ticks: dur_ticks * ppqn,
            clip_type: ClipType::Audio {
                sample_path: format!("clip{id}.wav"),
                peaks: vec![0.5; 32],
                sample_offset_ticks: 0,
                total_sample_ticks: dur_ticks * ppqn,
            },
            color: gpui_kit::rgb(0x205F91).into(),
        },
    )
}

/// App con 3 pistas y 3 clips (uno por fila), todo en el estado.
fn base_state() -> PlaylistState {
    let ppqn = 960;
    let mut pl = PlaylistState::default();
    pl.clips = vec![
        clip(1, 1, 0, 1, ppqn),
        clip(2, 2, 1, 1, ppqn),
        clip(3, 3, 2, 1, ppqn),
    ];
    pl.selected_clips = vec![2];
    pl
}

fn open_studio_with(cx: &mut TestAppContext, pl: PlaylistState) -> gpui_kit::WindowHandle<HikaruApp> {
    cx.update(gpui_kit::init);
    let handle = cx.add_window(|window, cx| test_app(window, cx));
    cx.simulate_window_resize(handle.into(), size(px(1280.0), px(720.0)));
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.mode = AppMode::OpenStudio;
                while s.studio_tracks.len() < 5 {
                    let next_id = s.studio_tracks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
                    let n = s.studio_tracks.iter().filter(|t| !t.is_master).count();
                    s.studio_tracks
                        .push(Track::new(next_id, format!("TRACK {:02}", n + 1), false));
                }
                s.playlist_state = pl;
                cx.notify();
            });
        });
    })
    .unwrap();
    handle
}

/// `(ids seleccionados, [(track, id, start_ticks)])` del estado de la playlist.
fn sel_and_clips(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
) -> (Vec<usize>, Vec<(usize, usize, u64)>) {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        let mut out = (Vec::new(), Vec::new());
        app.update(cx, |app, cx| {
            let pl = &app.state.read(cx).playlist_state;
            out = (
                pl.selected_clips.clone(),
                pl.clips
                    .iter()
                    .map(|(t, c)| (*t, c.id, c.start_tick))
                    .collect(),
            );
        });
        out
    })
    .unwrap()
}

fn clipboard_len(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) -> usize {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        let mut n = 0usize;
        app.update(cx, |app, cx| {
            n = app.state.read(cx).playlist_state.clipboard.len();
        });
        n
    })
    .unwrap()
}

fn marquee_of(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) -> Option<MarqueeState> {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        let mut o = None;
        app.update(cx, |app, cx| {
            o = app.state.read(cx).playlist_state.marquee;
        });
        o
    })
    .unwrap()
}

// =========================================================================
// Geometría pura del marquee
// =========================================================================

#[test]
fn marquee_normalizes_backwards_drag() {
    // Arrastrar hacia arriba-izquierda debe dar un rect con origen en la
    // esquina superior izquierda, no uno "al revés".
    let m = MarqueeState {
        start_x: 100.0,
        start_y: 100.0,
        cur_x: 40.0,
        cur_y: 20.0,
    };
    assert_eq!(m.normalized(), (40.0, 20.0, 100.0, 100.0));
}

#[test]
fn marquee_threshold_distinguishes_click_from_drag() {
    let click = MarqueeState {
        start_x: 10.0,
        start_y: 10.0,
        cur_x: 12.0,
        cur_y: 11.0,
    };
    assert!(!click.is_drag(), "un temblor de 2px no es arrastre");
    let drag = MarqueeState {
        start_x: 10.0,
        start_y: 10.0,
        cur_x: 10.0 + MARQUEE_DRAG_THRESHOLD,
        cur_y: 10.0,
    };
    assert!(drag.is_drag(), "al superar el umbral es arrastre");
}

#[test]
fn rects_intersect_touching_edges_and_disjoint() {
    let a = (0.0, 0.0, 10.0, 10.0);
    assert!(playlist::rects_intersect(a, (5.0, 5.0, 15.0, 15.0)), "solape");
    assert!(playlist::rects_intersect(a, (10.0, 10.0, 20.0, 20.0)), "toque justo");
    assert!(!playlist::rects_intersect(a, (10.5, 0.0, 20.0, 5.0)), "separados");
}

#[test]
fn clips_in_marquee_picks_intersecting_and_valid_tracks() {
    // (track, clip_id, x, y, w, h)
    let rows = vec![
        (1usize, 10usize, 100.0f32, 100.0f32, 50.0f32, 30.0f32),
        (2, 11, 300.0, 100.0, 50.0, 30.0),
        (3, 12, 500.0, 400.0, 50.0, 30.0),
    ];
    let keys = [1usize, 2, 3];
    // Caja sobre el primero y parte del segundo.
    let hits = playlist::clips_in_marquee(&rows, (90.0, 90.0, 320.0, 140.0), &keys);
    assert_eq!(hits, vec![10, 11]);
    // El clip de la pista 3 está fuera de la caja.
    let hits = playlist::clips_in_marquee(&rows, (490.0, 390.0, 560.0, 440.0), &keys);
    assert_eq!(hits, vec![12]);
    // Una pista que ya no existe no aporta clips (pista borrada).
    let hits = playlist::clips_in_marquee(&rows, (90.0, 90.0, 320.0, 140.0), &[2]);
    assert_eq!(hits, vec![11]);
}

// =========================================================================
// Edición: copiar / cortar / duplicar / pegar (puro, sobre PlaylistState)
// =========================================================================

#[test]
fn copy_fills_clipboard_without_touching_grid() {
    let mut pl = base_state();
    let n = playlist::copy_selected_clips(&mut pl);
    assert_eq!(n, 1, "sólo el clip 2 está seleccionado");
    assert_eq!(pl.clipboard.len(), 1);
    assert_eq!(pl.clips.len(), 3, "copiar no borra de la grilla");
    assert_eq!(pl.clipboard[0].1.id, 2);
}

#[test]
fn copy_without_selection_keeps_existing_clipboard() {
    let mut pl = base_state();
    playlist::copy_selected_clips(&mut pl);
    pl.selected_clips.clear();
    let n = playlist::copy_selected_clips(&mut pl);
    assert_eq!(n, 0);
    assert_eq!(pl.clipboard.len(), 1, "no debe vaciar el clipboard");
}

#[test]
fn cut_removes_clips_and_keeps_selection_consistent() {
    let mut pl = base_state();
    let n = playlist::cut_selected_clips(&mut pl);
    assert_eq!(n, 1);
    assert_eq!(pl.clips.len(), 2);
    assert!(!pl.clips.iter().any(|(_, c)| c.id == 2), "el clip 2 salió");
    assert!(pl.selected_clips.is_empty());
    assert_eq!(pl.clipboard.len(), 1);
}

#[test]
fn duplicate_places_copy_right_after_original_end() {
    let mut pl = base_state();
    // El clip 2 arranca en tick 960 y dura 960 ⇒ termina en 1920; el duplicado
    // debe empezar en 1920.
    let ids = playlist::duplicate_selected_clips(&mut pl, 960);
    assert_eq!(ids.len(), 1);
    let dup = pl.clips.iter().find(|(_, c)| c.id == ids[0]).unwrap();
    assert_eq!(dup.1.start_tick, 1920);
    assert_eq!(dup.1.duration_ticks, 960);
    assert_eq!(dup.0, 2, "conserva la pista");
    assert_eq!(pl.selected_clips, ids, "queda seleccionado lo nuevo");
}

#[test]
fn duplicate_whole_selection_stacks_after_max_end() {
    let mut pl = base_state();
    pl.selected_clips = vec![1, 2, 3];
    let ids = playlist::duplicate_selected_clips(&mut pl, 960);
    assert_eq!(ids.len(), 3);
    // Original: 0..960, 960..1920, 1920..2880 ⇒ el bloque duplicado arranca en
    // 2880 conservando los offsets relativos del original.
    let mut starts: Vec<u64> = ids
        .iter()
        .map(|id| pl.clips.iter().find(|(_, c)| c.id == *id).unwrap().1.start_tick)
        .collect();
    starts.sort();
    assert_eq!(
        starts,
        vec![2880, 3840, 4800],
        "el duplicado arranca al final del original y conserva los offsets"
    );
    assert_eq!(pl.clips.len(), 6);
}

#[test]
fn paste_at_playhead_preserves_relative_offsets_and_tracks() {
    let mut pl = base_state();
    pl.selected_clips = vec![1, 2, 3];
    playlist::copy_selected_clips(&mut pl);
    pl.selected_clips.clear();
    // Playhead en el tick 3840 (compás 5 en 4/4 con ppqn 960).
    pl.playhead_tick = 3840;
    pl.loop_region_active = false;
    let at = pl.paste_tick();
    assert_eq!(at, 3840);
    let ids = playlist::paste_clips(&mut pl, at, 960, &[1, 2, 3]);
    assert_eq!(ids.len(), 3);
    // El primer clip del conjunto (start 0) queda en el playhead y los otros
    // conservan su separación (960, 1920).
    let mut pasted: Vec<(usize, u64)> = ids
        .iter()
        .map(|id| {
            let c = pl.clips.iter().find(|(_, c)| c.id == *id).unwrap();
            (c.0, c.1.start_tick)
        })
        .collect();
    pasted.sort_by_key(|(_, start)| *start);
    assert_eq!(
        pasted,
        vec![(1, 3840), (2, 4800), (3, 5760)],
        "offsets relativos conservados"
    );
}

#[test]
fn paste_respects_grid_snap() {
    let mut pl = base_state();
    playlist::copy_selected_clips(&mut pl);
    // Snap de negra (960) y destino fuera de la grilla: debe ajustar al múltiplo.
    let ids = playlist::paste_clips(&mut pl, 1000, 960, &[1, 2, 3]);
    let start = pl.clips.iter().find(|(_, c)| c.id == ids[0]).unwrap().1.start_tick;
    assert_eq!(start % 960, 0, "el pegado respeta la grilla, quedó en {start}");
    assert_eq!(start, 960);
}

#[test]
fn paste_skips_clips_whose_track_disappeared() {
    let mut pl = base_state();
    pl.selected_clips = vec![1, 2, 3];
    playlist::copy_selected_clips(&mut pl);
    // Sólo queda la pista 1.
    let ids = playlist::paste_clips(&mut pl, 0, 960, &[1]);
    assert_eq!(ids.len(), 1, "sólo se pega el clip de la pista existente");
}

#[test]
fn paste_with_empty_clipboard_is_noop() {
    let mut pl = base_state();
    let ids = playlist::paste_clips(&mut pl, 0, 960, &[1, 2, 3]);
    assert!(ids.is_empty());
    assert_eq!(pl.clips.len(), 3);
}

#[test]
fn pasted_ids_never_collide() {
    let mut pl = base_state();
    pl.selected_clips = vec![1, 2, 3];
    playlist::copy_selected_clips(&mut pl);
    let ids = playlist::paste_clips(&mut pl, 0, 960, &[1, 2, 3]);
    for id in &ids {
        assert_eq!(
            pl.clips.iter().filter(|(_, c)| c.id == *id).count(),
            1,
            "el id {id} debe ser único"
        );
    }
    assert!(!ids.contains(&1) && !ids.contains(&2) && !ids.contains(&3));
}


// =========================================================================
// Integración: atajos de teclado sobre la ventana
// =========================================================================

/// Pulsa una tecla con modificadores vía el `capture_key_down` de la raíz.
fn press_key(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    key: &str,
    mods: gpui_kit::Modifiers,
) {
    cx.update_window(handle.into(), |_, window, cx| {
        use gpui_kit::{InputEvent as _, KeyDownEvent};
        let stroke = gpui_kit::Keystroke {
            modifiers: mods,
            key: key.to_string(),
            key_char: None,
        };
        window.dispatch_event(
            KeyDownEvent {
                keystroke: stroke,
                is_held: false,
                prefer_character_input: false,
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::gpui::test]
fn ctrl_c_then_ctrl_v_pastes_at_playhead(cx: &mut TestAppContext) {
    let handle = open_studio_with(cx, base_state());
    let ctrl = gpui_kit::Modifiers {
        control: true,
        ..Default::default()
    };
    press_key(handle, cx, "c", ctrl);
    assert_eq!(clipboard_len(handle, cx), 1, "Ctrl+C llenó el clipboard");

    press_key(handle, cx, "v", ctrl);

    let (sel, clips) = sel_and_clips(handle, cx);
    assert_eq!(clips.len(), 4, "quedan 3 originales + 1 pegado");
    assert_eq!(sel.len(), 1, "el pegado selecciona lo nuevo");
    // Con el playhead en 0 (reloj del motor en reposo) y el clipboard arrancando
    // en el tick 960, el pegado conserva el offset relativo: 960. Lo que
    // importa es que el nuevo clip nazca en la misma pista y sea seleccionable.
    let (track, _id, start) = *clips.iter().find(|(_, id, _)| sel.contains(id)).unwrap();
    assert_eq!(track, 2, "conserva la pista de origen");
    assert_eq!(start, 960, "pegado en el playhead + offset relativo");
}

#[gpui_kit::gpui::test]
fn ctrl_x_cuts_selection(cx: &mut TestAppContext) {
    let handle = open_studio_with(cx, base_state());
    let ctrl = gpui_kit::Modifiers {
        control: true,
        ..Default::default()
    };
    press_key(handle, cx, "x", ctrl);
    let (sel, clips) = sel_and_clips(handle, cx);
    assert_eq!(clips.len(), 2, "el clip seleccionado salió de la grilla");
    assert!(sel.is_empty());
    assert_eq!(clipboard_len(handle, cx), 1);
}

#[gpui_kit::gpui::test]
fn ctrl_d_duplicates_in_place(cx: &mut TestAppContext) {
    let handle = open_studio_with(cx, base_state());
    let ctrl = gpui_kit::Modifiers {
        control: true,
        ..Default::default()
    };
    press_key(handle, cx, "d", ctrl);
    let (sel, clips) = sel_and_clips(handle, cx);
    assert_eq!(clips.len(), 4);
    assert_eq!(sel.len(), 1, "selecciona el duplicado");
    let new_start = clips.iter().find(|(_, id, _)| sel.contains(id)).unwrap().2;
    assert_eq!(new_start, 1920, "el duplicado arranca al final del original");
}

#[gpui_kit::gpui::test]
fn edit_shortcuts_ignore_plain_keys(cx: &mut TestAppContext) {
    let handle = open_studio_with(cx, base_state());
    let before = clipboard_len(handle, cx);
    // Sin Ctrl no debe pasar nada (ni copiar ni cortar).
    press_key(handle, cx, "c", gpui_kit::Modifiers::default());
    press_key(handle, cx, "x", gpui_kit::Modifiers::default());
    let (sel, clips) = sel_and_clips(handle, cx);
    assert_eq!(clipboard_len(handle, cx), before);
    assert_eq!(clips.len(), 3, "la tecla sola no corta");
    assert_eq!(sel, vec![2]);
}

// =========================================================================
// Marquee: gesto real con botón derecho
// =========================================================================

fn rdown(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    x: f32,
    y: f32,
    mods: gpui_kit::Modifiers,
) {
    cx.update_window(handle.into(), |_, window, cx| {
        use gpui_kit::InputEvent as _;
        window.dispatch_event(
            MouseDownEvent {
                button: MouseButton::Right,
                position: point(px(x), px(y)),
                modifiers: mods,
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

fn rmove(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    x: f32,
    y: f32,
    released: bool,
) {
    cx.update_window(handle.into(), |_, window, cx| {
        use gpui_kit::InputEvent as _;
        window.dispatch_event(
            MouseMoveEvent {
                position: point(px(x), px(y)),
                pressed_button: if released { None } else { Some(MouseButton::Right) },
                modifiers: Default::default(),
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

fn rup(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    x: f32,
    y: f32,
    mods: gpui_kit::Modifiers,
) {
    cx.update_window(handle.into(), |_, window, cx| {
        use gpui_kit::InputEvent as _;
        window.dispatch_event(
            MouseUpEvent {
                button: MouseButton::Right,
                position: point(px(x), px(y)),
                modifiers: mods,
                click_count: 1,
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

/// Bounding box de un clip en la ventana.
fn clip_bounds(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    id: usize,
) -> (f32, f32, f32, f32) {
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        let b = window.find(format!("pl_clip_{id}")).bounds();
        (
            b.origin.x.as_f32(),
            b.origin.y.as_f32(),
            b.size.width.as_f32(),
            b.size.height.as_f32(),
        )
    })
    .unwrap()
}

/// Origen del panel temporal (la grilla) en coords de ventana.
fn grid_origin(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
) -> (f32, f32) {
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        let b = window.find("playlist_grid").bounds();
        (b.origin.x.as_f32(), b.origin.y.as_f32())
    })
    .unwrap()
}


/// Arrastre con botón derecho sobre el clip 1: debe seleccionarlo.
#[gpui_kit::gpui::test]
fn right_drag_over_clip_selects_it(cx: &mut TestAppContext) {
    let mut pl = base_state();
    pl.selected_clips.clear();
    let handle = open_studio_with(cx, pl);
    let (gx, gy) = grid_origin(handle, cx);
    let (x, y, w, h) = clip_bounds(handle, cx, 1);
    // Arranca dentro del panel temporal (arriba-izquierda, sobre la regla) y
    // termina sobre el clip: la caja lo toca.
    rdown(handle, cx, gx + 2.0, gy + 2.0, gpui_kit::Modifiers::default());
    rmove(handle, cx, x + w * 0.5, y + h * 0.5, false);
    rup(handle, cx, x + w * 0.5, y + h * 0.5, gpui_kit::Modifiers::default());
    let (sel, _) = sel_and_clips(handle, cx);
    assert_eq!(sel, vec![1], "el clip tocado por la caja queda seleccionado");
}

/// La caja que cubre dos filas selecciona ambos clips.
#[gpui_kit::gpui::test]
fn marquee_across_two_rows_selects_both(cx: &mut TestAppContext) {
    let mut pl = base_state();
    pl.selected_clips.clear();
    let handle = open_studio_with(cx, pl);
    let (gx, gy) = grid_origin(handle, cx);
    let (_x1, _y1, w1, _h1) = clip_bounds(handle, cx, 1);
    let (x2, y2, w2, h2) = clip_bounds(handle, cx, 2);
    rdown(handle, cx, gx + 2.0, gy + 2.0, gpui_kit::Modifiers::default());
    rmove(handle, cx, x2 + w2, y2 + h2 * 0.5, false);
    rup(handle, cx, x2 + w2, y2 + h2 * 0.5, gpui_kit::Modifiers::default());
    let (sel, _) = sel_and_clips(handle, cx);
    assert_eq!(sel.len(), 2, "seleccionó los dos: {sel:?} (w1={w1})");
}

/// Click derecho SIN arrastre no selecciona nada (queda para el menú).
#[gpui_kit::gpui::test]
fn right_click_without_drag_does_not_select(cx: &mut TestAppContext) {
    let mut pl = base_state();
    pl.selected_clips.clear();
    let handle = open_studio_with(cx, pl);
    let (x, y, w, h) = clip_bounds(handle, cx, 1);
    let mx = x + w / 2.0;
    let my = y + h / 2.0;
    rdown(handle, cx, mx, my, gpui_kit::Modifiers::default());
    // Un movimiento por debajo del umbral: sigue siendo click simple.
    rmove(handle, cx, mx + 2.0, my, false);
    rup(handle, cx, mx + 2.0, my, gpui_kit::Modifiers::default());
    let (sel, _) = sel_and_clips(handle, cx);
    assert!(
        sel.is_empty(),
        "un click derecho simple no debe seleccionar (deja el menú contextual)"
    );
}

/// `Shift` + marquee suma a la selección existente en vez de reemplazarla.
#[gpui_kit::gpui::test]
fn shift_marquee_adds_to_selection(cx: &mut TestAppContext) {
    let mut pl = base_state();
    pl.selected_clips = vec![3];
    let handle = open_studio_with(cx, pl);
    let (gx, gy) = grid_origin(handle, cx);
    let (x1, y1, w1, h1) = clip_bounds(handle, cx, 1);
    let shift = gpui_kit::Modifiers {
        shift: true,
        ..Default::default()
    };
    rdown(handle, cx, gx + 2.0, gy + 2.0, shift);
    rmove(handle, cx, x1 + w1 * 0.5, y1 + h1 * 0.5, false);
    rup(handle, cx, x1 + w1 * 0.5, y1 + h1 * 0.5, shift);
    let (sel, _) = sel_and_clips(handle, cx);
    assert_eq!(sel.len(), 2, "sumó al clip previo: {sel:?}");
    assert!(sel.contains(&1) && sel.contains(&3));
}

/// El estado del marquee se limpia al soltar (no queda colgado).
#[gpui_kit::gpui::test]
fn marquee_state_is_cleared_on_mouse_up(cx: &mut TestAppContext) {
    let mut pl = base_state();
    pl.selected_clips.clear();
    let handle = open_studio_with(cx, pl);
    let (gx, gy) = grid_origin(handle, cx);
    let (x, y, w, h) = clip_bounds(handle, cx, 1);
    rdown(handle, cx, gx + 2.0, gy + 2.0, gpui_kit::Modifiers::default());
    rmove(handle, cx, x + w * 0.5, y + h * 0.5, false);
    assert!(
        marquee_of(handle, cx).is_some(),
        "el marquee debería estar vivo durante el arrastre"
    );
    rup(handle, cx, x + w * 0.5, y + h * 0.5, gpui_kit::Modifiers::default());
    assert!(
        marquee_of(handle, cx).is_none(),
        "al soltar el gesto debe cerrarse"
    );
}
