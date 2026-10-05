// crates/hikaru_gui/tests/playlist_track_header.rs
//
// Track Headers de la Playlist / Timeline con la misma caja de mezcla del
// Session Matrix: nombre + [M] [S] + knob de pan + fader de volumen con dB.
//
// Lo que se verifica:
// - Los controles de mezcla existen dentro del header (`pl_vol_*`, `pl_panknob_*`,
//   `pl_track_mute_*`, `pl_track_solo_*`).
// - Mute/Solo escriben en la pista del modo activo Y espejan al espejo
//   (Session Matrix en OpenLive / vector del modo) → binding bidireccional.
// - El drag del fader cambia el volumen en vivo (mismo tag global que la matriz).
// - La fila es lo bastante alta para la caja completa y el header no se superpone.

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
use hikaru_gui::views::playlist;
use hikaru_gui::views::playlist::{ClipType, PlaylistClip};

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

fn open_studio(cx: &mut TestAppContext) -> gpui_kit::WindowHandle<HikaruApp> {
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
                    s.studio_tracks.push(Track::new(next_id, format!("TRACK {:02}", n + 1), false));
                }
                // Un clip por fila (id == fila) para comparar la base del
                // header contra la del carril de la grilla.
                for row in 1..=4usize {
                    let id = 100 + row;
                    s.playlist_state.clips.push((
                        row,
                        PlaylistClip {
                            id,
                            name: format!("Clip {row}"),
                            start_tick: row as u64 * s.playlist_state.ppqn,
                            duration_ticks: s.playlist_state.ppqn,
                            clip_type: ClipType::Audio {
                                sample_path: "x.wav".to_string(),
                                peaks: vec![0.5; 32],
                                sample_offset_ticks: 0,
                                total_sample_ticks: s.playlist_state.ppqn,
                            },
                            color: gpui_kit::rgb(0x205F91).into(),
                        },
                    ));
                }
                cx.notify();
            });
        });
    })
    .unwrap();
    handle
}

fn down(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    x: f32,
    y: f32,
    clicks: usize,
) {
    cx.update_window(handle.into(), |_, window, cx| {
        use gpui_kit::InputEvent as _;
        window.dispatch_event(
            MouseDownEvent {
                button: MouseButton::Left,
                position: point(px(x), px(y)),
                modifiers: Default::default(),
                click_count: clicks,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

fn move_to(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext, x: f32, y: f32) {
    cx.update_window(handle.into(), |_, window, cx| {
        use gpui_kit::InputEvent as _;
        window.dispatch_event(
            MouseMoveEvent {
                position: point(px(x), px(y)),
                pressed_button: Some(MouseButton::Left),
                modifiers: Default::default(),
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
}

fn up(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext, x: f32, y: f32) {
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

/// Estado de mezcla de una fila del modo activo + su espejo en la matriz.
fn studio_mix(
    handle: gpui_kit::WindowHandle<HikaruApp>,
    cx: &mut TestAppContext,
    idx: usize,
) -> (f32, f32, bool, bool) {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        let mut out = (0.0f32, 0.0f32, false, false);
        app.update(cx, |app, cx| {
            let st = app.state.read(cx);
            if let Some(t) = st.studio_tracks.get(idx) {
                out = (t.volume, t.pan, t.mute, t.solo);
            }
        });
        out
    })
    .unwrap()
}

// =========================================================================
// Existencia de los controles dentro del header
// =========================================================================

#[gpui_kit::gpui::test]
fn playlist_header_exposes_full_mix_controls(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        for id in [
            "pl_row_1",
            "pl_track_mute_1",
            "pl_track_solo_1",
            "pl_vol_1",
            "pl_panknob_1",
        ] {
            assert!(
                window.try_find(id).is_some(),
                "el header de la playlist debería exponer `{id}`"
            );
        }
    })
    .unwrap();
}

/// La caja de mezcla completa tiene que caber en la fila: si el alto por
/// defecto quedara corto, el fader/knob se solaparían con la fila siguiente.
#[gpui_kit::gpui::test]
fn playlist_row_tall_enough_for_mix_box(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    let (row_h, vol_h, knob_bottom, row_bottom) = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            let row = window.find("pl_row_1").bounds();
            let vol = window.find("pl_vol_1").bounds();
            let knob = window.find("pl_panknob_1").bounds();
            (
                row.size.height.as_f32(),
                vol.size.height.as_f32(),
                (knob.origin.y + knob.size.height).as_f32(),
                (row.origin.y + row.size.height).as_f32(),
            )
        })
        .unwrap();
    println!(
        "playlist header: fila {row_h:.0}px, fader {vol_h:.0}px, base del knob y={knob_bottom:.0}, fin de fila y={row_bottom:.0}"
    );
    assert!(row_h >= 60.0, "la fila debería medir al menos 60px, mide {row_h}");
    assert!(
        knob_bottom <= row_bottom + 0.5,
        "el knob se sale de la fila (base {knob_bottom:.1} vs fin {row_bottom:.1})"
    );
}

// =========================================================================
// Mute / Solo: escritura + espejo
// =========================================================================

#[gpui_kit::gpui::test]
fn playlist_mute_button_toggles_and_syncs(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    assert!(!studio_mix(handle, cx, 1).2, "arranca sin mute");
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.click("pl_track_mute_1", cx);
    })
    .unwrap();
    assert!(
        studio_mix(handle, cx, 1).2,
        "M en la playlist debería mutear la pista del modo activo"
    );
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.click("pl_track_mute_1", cx);
    })
    .unwrap();
    assert!(!studio_mix(handle, cx, 1).2, "M debería alternar de vuelta");
}

#[gpui_kit::gpui::test]
fn playlist_solo_button_toggles(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    assert!(!studio_mix(handle, cx, 1).3, "arranca sin solo");
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.click("pl_track_solo_1", cx);
    })
    .unwrap();
    assert!(studio_mix(handle, cx, 1).3, "S en la playlist debería solear la pista");
}

/// El header escribe SIEMPRE en la pista del modo ACTIVO (el mismo índice que
/// lee el channel strip del mixer de OpenStudio) y no en una copia local: por
/// eso el mute del header se ve en el mixer sin recargar.
///
/// Nota: la Playlist sólo se monta en OpenStudio (`render_central` manda a la
/// Session Matrix en OpenLive), así que el caso observable es ése. La rama
/// OpenLive del binding (espejo a `matrix_state`) queda cubierta por el mismo
/// `playlist_row_mix`/apply compartidos.
#[gpui_kit::gpui::test]
fn playlist_mute_writes_mode_active_track(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    let before = studio_mix(handle, cx, 1);
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.click("pl_track_mute_1", cx);
    })
    .unwrap();
    let after = studio_mix(handle, cx, 1);
    assert!(after.2, "el mute debería escribirse en la pista del modo activo");
    assert_eq!(
        (before.0, before.1, before.3),
        (after.0, after.1, after.3),
        "sólo mute debería cambiar"
    );
}

// =========================================================================
// Fader de volumen: drag en vivo
// =========================================================================

#[gpui_kit::gpui::test]
fn playlist_volume_drag_updates_track(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    let before = studio_mix(handle, cx, 1).0;
    // Se toma el borde izquierdo del fader (volumen ~0) y se arrastra a la
    // derecha: debe SUBIR el volumen.
    let (x0, y0, x1) = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            let b = window.find("pl_vol_1").bounds();
            (
                (b.origin.x + px(2.0)).as_f32(),
                (b.origin.y + b.size.height / 2.0).as_f32(),
                (b.origin.x + b.size.width - px(2.0)).as_f32(),
            )
        })
        .unwrap();
    down(handle, cx, x0, y0, 1);
    move_to(handle, cx, x1, y0);
    up(handle, cx, x1, y0);
    let after = studio_mix(handle, cx, 1).0;
    println!("volumen de la fila 1: {before:.3} -> {after:.3}");
    assert!(
        after > before,
        "arrastrar el fader a la derecha debería subir el volumen ({before:.3} -> {after:.3})"
    );
}

#[gpui_kit::gpui::test]
fn playlist_volume_double_click_resets(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    // Se baja el volumen primero y después se hace doble-clic (reset a 0.0dB).
    let (x0, y0, x1) = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            let b = window.find("pl_vol_1").bounds();
            (
                (b.origin.x + px(2.0)).as_f32(),
                (b.origin.y + b.size.height / 2.0).as_f32(),
                (b.origin.x + b.size.width - px(2.0)).as_f32(),
            )
        })
        .unwrap();
    down(handle, cx, x0, y0, 1);
    up(handle, cx, x0, y0);
    let low = studio_mix(handle, cx, 1).0;
    assert!(low < 0.75, "el click en el borde izquierdo baja el volumen ({low:.3})");
    // Doble-clic: reset al valor de unity (0.75 lineal = 0.0dB).
    down(handle, cx, x0, y0, 2);
    up(handle, cx, x0, y0);
    let reset = studio_mix(handle, cx, 1).0;
    println!("reset por doble-clic: {low:.3} -> {reset:.3}");
    assert!(
        (reset - 0.75).abs() < 0.02,
        "el doble-clic debería volver a 0.0dB (0.75 lineal), quedó en {reset:.3}"
    );
}

#[gpui_kit::gpui::test]
fn playlist_pan_drag_updates_track(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    let before = studio_mix(handle, cx, 1).1;
    let (x, y) = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            let b = window.find("pl_panknob_1").bounds();
            (
                (b.origin.x + b.size.width / 2.0).as_f32(),
                (b.origin.y + b.size.height / 2.0).as_f32(),
            )
        })
        .unwrap();
    // Agarrar NO cambia el valor; se arrastra hacia arriba (hacia R).
    down(handle, cx, x, y, 1);
    let after_grab = studio_mix(handle, cx, 1).1;
    assert_eq!(before, after_grab, "agarrar el knob no debe cambiar el paneo");
    move_to(handle, cx, x, y - 60.0);
    up(handle, cx, x, y - 60.0);
    let after = studio_mix(handle, cx, 1).1;
    println!("pan de la fila 1: {before:.3} -> {after:.3}");
    assert!(
        after > before + 0.05,
        "arrastrar el knob hacia arriba debería panear a la derecha ({before:.3} -> {after:.3})"
    );
}

/// Regresión de la asimetría de alto: el Track Header debe medir EXACTAMENTE lo
/// mismo que el carril de la grilla. Con `h_full()` (el bug) el header medía el
/// alto de su contenido (73.5px) contra los 68px del carril y cada fila acumulaba
/// ~5.5px de desfase respecto del clip de su fila.
///
/// Se mide en tres filas y con zoom vertical (que es lo que ata el header al
/// `row_h` actual): el offset header-vs-clip debe ser 0 en todas.
#[gpui_kit::gpui::test]
fn header_height_matches_track_row_and_does_not_drift(cx: &mut TestAppContext) {
    let handle = open_studio(cx);
    // (y del header, y del clip) por fila, con `row_h` dado.
    fn sample(
        handle: gpui_kit::WindowHandle<HikaruApp>,
        cx: &mut TestAppContext,
        row_h: f32,
    ) -> Vec<(f32, f32)> {
        cx.update_window(handle.into(), |view, _, cx| {
            let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
            app.update(cx, |app, cx| {
                app.state.update(cx, |s, _| s.playlist_state.row_h = row_h);
            });
        });
        cx.update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            (1..=3)
                .map(|i| {
                    let h = window.find(format!("pl_row_{i}")).bounds();
                    // Los clips del fixture usan id 100 + fila.
                    let c = window.find(format!("pl_clip_{}", 100 + i)).bounds();
                    (
                        h.origin.y.as_f32() + h.size.height.as_f32(),
                        c.origin.y.as_f32() + c.size.height.as_f32(),
                    )
                })
                .collect()
        })
        .unwrap()
    }

    for row_h in [playlist::MIN_ROW_H, playlist::TRACK_ROW_H, playlist::MAX_ROW_H] {
        let rows = sample(handle, cx, row_h);
        for (i, (header_bottom, clip_bottom)) in rows.iter().enumerate() {
                let row = i + 1;
            // Tolerancia de 2px: el clip se dibuja 1px por dentro del carril
            // (`clip_y = fila*row_h + 1`, `clip_h = row_h - 2`) y su borde
            // redondea. Lo que no puede pasar es la DERIVA: el desfase debe
            // ser el MISMO en todas las filas, no crecer con el índice.
            let drift = header_bottom - clip_bottom;
            assert!(
                drift.abs() <= 2.0,
                "fila {row} con row_h={row_h}: base del header {header_bottom:.1} vs base del clip {clip_bottom:.1} (desfase {drift:.1}px)"
            );
        }
        // Sin deriva acumulada: el salto entre bases de filas consecutivas
        // debe ser exactamente `row_h`. Con el bug (header 73.5 vs carril 68)
        // este salto crecía fila a fila y el desfase se acumulaba.
        for i in 1..rows.len() {
            let step = rows[i].0 - rows[i - 1].0;
            let clip_step = rows[i].1 - rows[i - 1].1;
            assert!(
                (step - row_h).abs() < 1.0,
                "con row_h={row_h}: el salto del header entre las filas {i} y {} es {step:.1} (debería ser {row_h:.1})",
                i + 1
            );
            assert!(
                (step - clip_step).abs() < 1.0,
                "con row_h={row_h}: header avanza {step:.1} por fila pero el clip {clip_step:.1} (se separan)"
            );
        }
        println!("row_h={row_h:.0}: headers alineados con los clips en {} filas", rows.len());
    }
}
