// crates/hikaru_gui/tests/playlist_clip_drag.rs
//
// Drag & Drop de clips de audio en la Playlist / Timeline (OpenStudio):
// - Click sin mover selecciona sin reposicionar.
// - Arrastrar horizontal mueve en tiempo con snap a la grilla.
// - Arrastrar vertical reasigna el track destino.

use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Arc;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{px, point, size, AppContext, Context, TestAppContext, Window};

use hikaru_gui::app::{AppMode, HikaruApp};
use hikaru_gui::audio_proxy::{AudioProxy, GuiCommand};
use hikaru_gui::views::mixer::Track;
use hikaru_gui::views::playlist::{ClipType, PlaylistClip, TRACK_ROW_H};

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

fn open_studio_playlist(cx: &mut TestAppContext) -> gpui_kit::WindowHandle<HikaruApp> {
    cx.update(gpui_kit::init);
    let handle = cx.add_window(|window, cx| test_app(window, cx));
    cx.simulate_window_resize(handle.into(), size(px(1280.0), px(720.0)));
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.mode = AppMode::OpenStudio;
                // Tres filas no-master para el drag vertical.
                while s.studio_tracks.len() < 4 {
                    let next_id = s.studio_tracks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
                    let n = s.studio_tracks.iter().filter(|t| !t.is_master).count();
                    s.studio_tracks.push(Track::new(
                        next_id,
                        format!("TRACK {:02}", n + 1),
                        false,
                    ));
                }
                // Clip de audio en la primera fila, compás 2.
                let ppqn = s.playlist_state.ppqn.max(1);
                s.playlist_state.clips.push((
                    1,
                    PlaylistClip {
                        id: 7,
                        name: "Loop".to_string(),
                        start_tick: 4 * ppqn,
                        duration_ticks: 2 * ppqn,
                        clip_type: ClipType::Audio {
                            sample_path: "loop.wav".to_string(),
                            peaks: vec![0.5; 64],
                            sample_offset_ticks: 0,
                            total_sample_ticks: 2 * ppqn,
                        },
                        color: gpui_kit::rgb(0x205F91).into(),
                    },
                ));
                cx.notify();
            });
        });
    })
    .unwrap();
    handle
}

/// `(start_tick, track_key, selected, ppqn, zoom_x, grid_den)` del clip 7.
fn clip_state(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
) -> (u64, usize, Vec<usize>, u64, f32, u32) {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        let mut out = (0u64, 0usize, Vec::new(), 0u64, 0.0f32, 0u32);
        app.update(cx, |app, cx| {
            let st = app.state.read(cx);
            let (_, clip) = st
                .playlist_state
                .clips
                .iter()
                .find(|(_, c)| c.id == 7)
                .expect("clip 7");
            out = (
                clip.start_tick,
                st.playlist_state
                    .clips
                    .iter()
                    .find(|(_, c)| c.id == 7)
                    .map(|(k, _)| *k)
                    .unwrap(),
                st.playlist_state.selected_clips.clone(),
                st.playlist_state.ppqn,
                st.playlist_state.zoom_x,
                st.playlist_state.grid_denominator,
            );
        });
        out
    })
    .unwrap()
}

fn clip_center(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
) -> gpui_kit::Point<gpui_kit::Pixels> {
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        let b = window.find("pl_clip_7").bounds();
        b.center()
    })
    .unwrap()
}

fn snap_step(ppqn: u64, grid_den: u32) -> u64 {
    let ppqn = ppqn.max(1);
    (match grid_den.max(1) {
        2 => ppqn * 2,
        4 => ppqn,
        8 => ppqn / 2,
        16 => ppqn / 4,
        d => ppqn * 4 / d as u64,
    })
    .max(1)
}

#[gpui_kit::gpui::test]
fn clip_click_selects_without_moving(cx: &mut TestAppContext) {
    let handle = open_studio_playlist(cx);
    let (t0, k0, _, _, _, _) = clip_state(handle, cx);
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.click("pl_clip_7", cx);
    })
    .unwrap();
    let (t1, k1, sel, _, _, _) = clip_state(handle, cx);
    assert_eq!(sel, vec![7], "click debería seleccionar el clip");
    assert_eq!((t1, k1), (t0, k0), "click sin mover no debe reposicionar");
}

#[gpui_kit::gpui::test]
fn clip_drag_moves_horizontally_with_snap(cx: &mut TestAppContext) {
    let handle = open_studio_playlist(cx);
    let (t0, k0, _, ppqn, zoom, grid_den) = clip_state(handle, cx);
    let from = clip_center(handle, cx);
    let dx = 200.0f32;
    let to = from + point(px(dx), px(0.0));
    cx.update_window(handle.into(), |_, window, cx| {
        window.drag(from, to, cx);
    })
    .unwrap();
    let (t1, k1, sel, _, _, _) = clip_state(handle, cx);
    let step = snap_step(ppqn, grid_den);
    let raw = t0 + (dx / zoom) as u64;
    let expected = ((raw as f64 / step as f64).round() as u64) * step;
    println!("drag horizontal: {t0} -> {t1} (esperado {expected}, snap {step})");
    assert_eq!(t1, expected, "el clip debería moverse en tiempo con snap");
    assert_eq!(k1, k0, "el track no debería cambiar en drag horizontal");
    assert_eq!(sel, vec![7], "el clip arrastrado queda seleccionado");
}

#[gpui_kit::gpui::test]
fn clip_drag_moves_vertically_to_other_track(cx: &mut TestAppContext) {
    let handle = open_studio_playlist(cx);
    let (t0, k0, _, _, _, _) = clip_state(handle, cx);
    assert_eq!(k0, 1);
    let from = clip_center(handle, cx);
    // Dos filas hacia abajo (TRACK_ROW_H = 54): fila 0 -> fila 2.
    let to = from + point(px(0.0), px(2.0 * TRACK_ROW_H + 10.0));
    cx.update_window(handle.into(), |_, window, cx| {
        window.drag(from, to, cx);
    })
    .unwrap();
    let (t1, k1, _, _, _, _) = clip_state(handle, cx);
    println!("drag vertical: track {k0} -> {k1}, tick {t0} -> {t1}");
    assert_eq!(k1, 3, "el clip debería reasignarse a la fila 2 (vec idx 3)");
    assert_eq!(t1, t0, "el tick no debería cambiar en drag puramente vertical");
}
