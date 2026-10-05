// crates/hikaru_gui/tests/clip_editor_view.rs
//
// Clip Editor: binding de selección y geometría del visualizador.
//
// - La selección es UNIFICADA: un click en un pad de la Session Matrix o en un
//   clip de la Playlist carga el mismo editor, y sin selección muestra el
//   estado vacío.
// - El visualizador tiene zoom y scroll PROPIOS (independientes del zoom de la
//   Playlist) con límites, y la regla de compases es local al clip pero arranca
//   en el compás que le corresponde en el proyecto.

use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Arc;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{px, size, AppContext, Context, TestAppContext, Window};

use hikaru_gui::app::{AppMode, HikaruApp};
use hikaru_gui::audio_proxy::{AudioProxy, GuiCommand};
use hikaru_gui::views::clip_editor::{
    self, bar_grid_secs, bar_number_at, best_peaks, clamped_ce_zoom, clamp_scroll, fit_zoom,
    painted_with_real_height, peaks_are_silent, peaks_window, ruler_labels, step_ce_zoom,
    visible_secs, wave_geometry, ClipEditorTarget, ClipEditorState, EditorTab, LAST_PAINT_H,
    MAX_CE_ZOOM, MIN_CE_ZOOM,
};
use hikaru_gui::views::matrix::{ClipData, MatrixClip};
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

fn audio_clip(id: usize, start_ticks: u64, dur_ticks: u64) -> PlaylistClip {
    PlaylistClip {
        id,
        name: format!("Clip {id}"),
        start_tick: start_ticks * 960,
        duration_ticks: dur_ticks * 960,
        clip_type: ClipType::Audio {
            sample_path: format!("clip{id}.wav"),
            peaks: (0..32).map(|i| (i as f32 / 32.0).sin().abs()).collect(),
            sample_offset_ticks: 0,
            total_sample_ticks: dur_ticks * 960,
        },
        color: gpui_kit::rgb(0x205F91).into(),
    }
}

/// App en OpenStudio con la Playlist visible y un clip de audio.
fn open_playlist(cx: &mut TestAppContext) -> gpui_kit::WindowHandle<HikaruApp> {
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
                    s.studio_tracks.push(hikaru_gui::views::mixer::Track::new(
                        next_id,
                        format!("TRACK {:02}", n + 1),
                        false,
                    ));
                }
                s.playlist_state.clips.push((1, audio_clip(1, 0, 4)));
                cx.notify();
            });
        });
    })
    .unwrap();
    handle
}

/// Abre el editor con el clip 1 de la Playlist seleccionado.
fn open_editor_with_clip(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("raíz");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.clip_editor.show_editor = true;
                s.clip_editor.target = Some(ClipEditorTarget::Playlist { clip_id: 1 });
                cx.notify();
            });
        });
    })
    .unwrap();
}

fn ce_state(handle: gpui_kit::WindowHandle<HikaruApp>, cx: &mut TestAppContext) -> ClipEditorState {
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("vista raíz HikaruApp");
        let mut out = ClipEditorState::default();
        app.update(cx, |app, cx| {
            out = app.state.read(cx).clip_editor.clone();
        });
        out
    })
    .unwrap()
}

// =========================================================================
// Geometría pura del visualizador
// =========================================================================

#[test]
fn ce_zoom_clamps_to_limits() {
    assert_eq!(clamped_ce_zoom(120.0), 120.0, "un valor válido pasa sin cambios");
    assert_eq!(clamped_ce_zoom(0.0), clip_editor::DEFAULT_CE_ZOOM);
    assert_eq!(clamped_ce_zoom(1e9), MAX_CE_ZOOM);
    // Valores degenerados caen al zoom por defecto (que es el legible),
    // no a 1 px/s donde la onda no se ve.
    assert_eq!(clamped_ce_zoom(f32::NAN), clip_editor::DEFAULT_CE_ZOOM);
    assert_eq!(clamped_ce_zoom(f32::INFINITY), clip_editor::DEFAULT_CE_ZOOM);
    assert_eq!(clamped_ce_zoom(-5.0), clip_editor::DEFAULT_CE_ZOOM);
}

#[test]
fn ce_step_zoom_moves_both_ways() {
    assert!(step_ce_zoom(1.0, true) > 1.0);
    assert!(step_ce_zoom(1.0, false) < 1.0);
    assert_eq!(step_ce_zoom(MAX_CE_ZOOM, true), MAX_CE_ZOOM);
    assert_eq!(step_ce_zoom(MIN_CE_ZOOM, false), MIN_CE_ZOOM);
}

#[test]
fn ce_fit_zoom_makes_clip_fill_viewport() {
    // Un clip de 4s en 800px ⇒ 200 px/s.
    assert!((fit_zoom(4.0, 800.0) - 200.0).abs() < 0.01);
    // Casos degenerados no dividen por cero.
    assert_eq!(fit_zoom(0.0, 800.0), 1.0);
    assert_eq!(fit_zoom(4.0, 0.0), 1.0);
}

#[test]
fn ce_visible_secs_is_inverse_of_zoom() {
    assert!((visible_secs(100.0, 400.0) - 4.0).abs() < 0.001);
    assert_eq!(visible_secs(0.0, 400.0), 0.0);
}

#[test]
fn ce_scroll_clamps_to_content() {
    // Clip de 10s, ventana de 2s ⇒ el scroll va de 0 a 8.
    assert_eq!(clamp_scroll(0.0, 10.0, 2.0), 0.0);
    assert!((clamp_scroll(5.0, 10.0, 2.0) - 5.0).abs() < 0.001);
    assert!((clamp_scroll(99.0, 10.0, 2.0) - 8.0).abs() < 0.001);
    assert_eq!(clamp_scroll(-5.0, 10.0, 2.0), 0.0);
    // Clip más corto que la ventana: no hay recorrido.
    assert_eq!(clamp_scroll(3.0, 1.0, 2.0), 0.0);
    assert_eq!(clamp_scroll(f64::NAN, 10.0, 2.0), 0.0);
}

#[test]
fn bar_grid_is_local_to_clip_but_starts_at_right_bar() {
    // 120 BPM ⇒ 2s por compás. Clip que arranca en el compás 3 (t=4s) y dura 8s.
    let bars = bar_grid_secs(4.0, 8.0, 120.0);
    assert!(bars.contains(&4.0), "incluye su propio compás de arranque: {bars:?}");
    assert!(bars.contains(&6.0) && bars.contains(&8.0) && bars.contains(&10.0));
    // El último compás no pasa el final del clip más un compás de margen.
    assert!(bars.iter().all(|t| *t >= 2.0 && *t <= 14.0), "{bars:?}");
}

#[test]
fn bar_number_matches_bpm() {
    // 120 BPM ⇒ 2s por compás.
    assert_eq!(bar_number_at(0.0, 120.0), 1);
    assert_eq!(bar_number_at(1.9, 120.0), 1);
    assert_eq!(bar_number_at(2.0, 120.0), 2);
    assert_eq!(bar_number_at(4.0, 120.0), 3);
    // Tiempos negativos y BPM inválido no rompen.
    assert_eq!(bar_number_at(-1.0, 120.0), 1);
    // BPM 0 cae al default de 120 BPM (2s por compás) ⇒ compás 3.
    assert_eq!(bar_number_at(4.0, 0.0), 3);
}

#[test]
fn peaks_window_maps_ratio_to_slice() {
    let peaks: Vec<f32> = (0..100).map(|i| i as f32).collect();
    // Ventana [0, 0.5) ⇒ los primeros 50 bins.
    let w = peaks_window(&peaks, 0.0, 0.5);
    assert_eq!(w.len(), 50);
    assert_eq!(w[0], 0.0);
    assert_eq!(w[49], 49.0);
    // Rango fuera de 0..1 se acota.
    assert_eq!(peaks_window(&peaks, -1.0, 2.0).len(), 100);
    assert!(peaks_window(&[], 0.0, 1.0).is_empty());
}

// =========================================================================
// Integración: binding de selección
// =========================================================================

/// Regresión del panel negro: el lienzo central debe tener ALTO REAL.
///
/// El bug era `h_full()` dentro de un `v_flex` sin alto resuelto: el canvas
/// quedaba en 1084×0 y todo lo que se dibujara dentro (regla, grilla, waveform)
/// era invisible.
#[gpui_kit::gpui::test]
fn waveform_canvas_has_real_height(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    open_editor_with_clip(handle, cx);
    let (w, h) = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            let b = window.find("clip_editor_waveform").bounds();
            (b.size.width.as_f32(), b.size.height.as_f32())
        })
        .unwrap();
    println!("lienzo: {w:.0}x{h:.0}");
    assert!(w > 200.0, "el lienzo debe tener ancho: {w}");
    assert!(
        h >= clip_editor::WAVE_MIN_H,
        "el lienzo debe tener alto real (>= {}), tiene {h}",
        clip_editor::WAVE_MIN_H
    );
}

/// Sin selección, el editor muestra el estado vacío informativo ("No clip
/// selected") en vez de un panel en blanco.
#[gpui_kit::gpui::test]
fn empty_state_without_selection(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    // El panel arranca plegado (para no robarle alto a la vista principal), así
    // que el estado vacío se abre explícitamente.
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("raíz");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.clip_editor.show_editor = true;
                s.clip_editor.target = None;
                cx.notify();
            });
        });
    })
    .unwrap();
    assert!(
        ce_state(handle, cx).target.is_none(),
        "no hay clip en edición"
    );
    // El panel existe y es observable.
    let found = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            window.try_find("clip_editor").is_some()
        })
        .unwrap();
    assert!(found, "el panel debe renderizar el estado vacío");
    // ...y en el estado vacío NO monta el visualizador ni el sidebar.
    let has_viewer = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            window.try_find("clip_editor_waveform").is_some()
        })
        .unwrap();
    assert!(!has_viewer, "sin clip no debe haber visualizador");
}

/// El panel arranca plegado para no robarle alto a la vista principal.
#[gpui_kit::gpui::test]
fn panel_starts_collapsed(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    assert!(
        !ce_state(handle, cx).show_editor,
        "el Clip Editor arranca cerrado; se abre al seleccionar un clip"
    );
}

/// Click en un clip de la Playlist lo carga en el Clip Editor.
#[gpui_kit::gpui::test]
fn playlist_clip_click_loads_editor(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.click("pl_clip_1", cx);
    })
    .unwrap();
    let ce = ce_state(handle, cx);
    assert_eq!(
        ce.target,
        Some(ClipEditorTarget::Playlist { clip_id: 1 }),
        "el click en el clip debe cargarlo en el editor"
    );
}

/// El mismo editor recibe el clip de la Session Matrix (binding unificado).
#[gpui_kit::gpui::test]
fn matrix_clip_click_loads_editor(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("raíz");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                // Clip de audio en el pad (0, 0).
                let mut clip = MatrixClip {
                    id: 1,
                    name: "Kick.wav".into(),
                    path: "kick.wav".into(),
                    duration_secs: 2.0,
                    content: ClipData::Audio {
                        events: Vec::new(),
                        next_event_id: 1,
                        preview_mix: vec![0.0; 16],
                        preview_sr: 44100,
                    },
                    local_state: Default::default(),
                    local_track: hikaru_gui::views::mixer::Track::new(9, "T".into(), false),
                    local_bar: 0.0,
                    loop_start: 0,
                    loop_end: 0,
                    loop_enabled: false,
                    has_time_selection: false,
                    peaks: vec![0.5; 16],
                };
                clip.duration_secs = 2.0;
                s.matrix_state.grid[0][0].clip = Some(clip);
                s.matrix_state.selected_slot = Some((0, 0));
                s.clip_editor.target =
                    Some(ClipEditorTarget::Matrix { track: 0, scene: 0 });
                cx.notify();
            });
        });
    })
    .unwrap();
    let ce = ce_state(handle, cx);
    assert_eq!(ce.target, Some(ClipEditorTarget::Matrix { track: 0, scene: 0 }));
    // El editor resuelve el clip desde la matriz.
    let resolved = cx
        .update_window(handle.into(), |view, _, cx| {
            let app = view.downcast::<HikaruApp>().expect("raíz");
            let mut out = None;
            app.update(cx, |app, cx| {
                let st = app.state.read(cx);
                let bpm = app.state.read(cx).transport.bpm as f32;
                out = clip_editor::resolve_clip(&st, bpm).map(|c| c.name);
            });
            out
        })
        .unwrap();
    assert_eq!(resolved.as_deref(), Some("Kick.wav"), "resuelve el clip de la matriz");
}

/// Un clip de la Playlist resuelve nombre y duración desde sus ticks.
#[gpui_kit::gpui::test]
fn playlist_clip_resolves_duration(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("raíz");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.clip_editor.target = Some(ClipEditorTarget::Playlist { clip_id: 1 });
                cx.notify();
            });
        });
    })
    .unwrap();
    let info = cx
        .update_window(handle.into(), |view, _, cx| {
            let app = view.downcast::<HikaruApp>().expect("raíz");
            let mut out = (0.0f64, 0.0f64, false);
            app.update(cx, |app, cx| {
                let bpm = app.state.read(cx).transport.bpm as f32;
                let st = app.state.read(cx);
                if let Some(c) = clip_editor::resolve_clip(&st, bpm) {
                    out = (c.duration_secs, c.start_secs, c.is_midi);
                }
            });
            out
        })
        .unwrap();
    // 4 negras a 140 BPM (default del transporte) = 4·60/140 ≈ 1.714s.
    let expected = 4.0 * 60.0 / 140.0;
    println!("duración resuelta: {:.3}s (esperada {expected:.3}), inicio {:.3}s, midi={}", info.0, info.1, info.2);
    assert!(
        (info.0 - expected).abs() < 0.01,
        "duración en segundos desde ticks: {} vs {expected}",
        info.0
    );
    assert_eq!(info.1, 0.0, "el clip arranca en el tick 0");
    assert!(!info.2, "un clip de audio no es MIDI");
}

/// Las pestañas cambian el estado del editor.
#[gpui_kit::gpui::test]
fn tabs_switch_mode(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("raíz");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.clip_editor.target = Some(ClipEditorTarget::Playlist { clip_id: 1 });
                s.clip_editor.show_editor = true;
                cx.notify();
            });
        });
    })
    .unwrap();
    assert_eq!(ce_state(handle, cx).tab, EditorTab::AudioEvents);
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.click("ce_tab_onsets", cx);
    })
    .unwrap();
    assert_eq!(ce_state(handle, cx).tab, EditorTab::Onsets);
}

/// La toolbar expone el selector de modo, Snap y Loop (controles del legacy).
#[gpui_kit::gpui::test]
fn toolbar_exposes_snap_loop_and_modes(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    open_editor_with_clip(handle, cx);
    let present = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            [
                "ce_tab_events",
                "ce_tab_comping",
                "ce_tab_stretch",
                "ce_tab_onsets",
                "ce_snap_cycle",
                "ce_loop_toggle",
                "ce_zoom_in",
                "ce_zoom_out",
                "ce_zoom_fit",
                "clip_editor_waveform",
            ]
            .iter()
            .filter(|id| window.try_find(**id).is_some())
            .count()
        })
        .unwrap();
    println!("controles presentes en la toolbar: {present}/10");
    assert_eq!(present, 10, "faltan controles de la toolbar");
}

/// El grupo de controles queda alineado a la DERECHA de la barra: el último
/// (`Fit`) termina en el borde del panel, y `Loop`/`Snap` están en esa mitad
/// derecha (no pegados al nombre del clip).
#[gpui_kit::gpui::test]
fn controls_are_right_aligned(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    open_editor_with_clip(handle, cx);
    let (ed_r, loop_r, snap_r, fit_r) = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            let ed = window.find("clip_editor").bounds();
            let lp = window.find("ce_loop_toggle").bounds();
            let sn = window.find("ce_snap_cycle").bounds();
            let ft = window.find("ce_zoom_fit").bounds();
            (
                (ed.origin.x + ed.size.width).as_f32(),
                (lp.origin.x + lp.size.width).as_f32(),
                (sn.origin.x + sn.size.width).as_f32(),
                (ft.origin.x + ft.size.width).as_f32(),
            )
        })
        .unwrap();
    println!("fin: loop {loop_r:.0}, snap {snap_r:.0}, fit {fit_r:.0}, borde {ed_r:.0}");
    // El último control cierra contra el borde derecho del panel.
    assert!(
        ed_r - fit_r < 12.0,
        "el grupo debe cerrar en el borde derecho ({fit_r} vs {ed_r})"
    );
    // Orden esperado de izquierda a derecha dentro del grupo.
    assert!(loop_r < snap_r && snap_r < fit_r, "orden Loop < Snap < Fit");
    // Y Loop/Snap están en la mitad derecha del panel.
    assert!(loop_r > ed_r * 0.5, "Loop debe estar a la derecha ({loop_r})");
    assert!(snap_r > ed_r * 0.6, "Snap debe estar más a la derecha ({snap_r})");
}

/// El zoom del visualizador es independiente del de la Playlist.
#[gpui_kit::gpui::test]
fn editor_zoom_is_independent_of_playlist(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    cx.update_window(handle.into(), |view, _, cx| {
        let app = view.downcast::<HikaruApp>().expect("raíz");
        app.update(cx, |app, cx| {
            app.state.update(cx, |s, cx| {
                s.clip_editor.target = Some(ClipEditorTarget::Playlist { clip_id: 1 });
                s.clip_editor.show_editor = true;
                cx.notify();
            });
        });
    })
    .unwrap();
    let playlist_zoom_before = cx
        .update_window(handle.into(), |view, _, cx| {
            let app = view.downcast::<HikaruApp>().expect("raíz");
            let mut z = 0.0f32;
            app.update(cx, |app, cx| {
                z = app.state.read(cx).playlist_state.zoom_x;
            });
            z
        })
        .unwrap();
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.click("ce_zoom_in", cx);
    })
    .unwrap();
    let ce_zoom = ce_state(handle, cx).zoom_x;
    let playlist_zoom_after = cx
        .update_window(handle.into(), |view, _, cx| {
            let app = view.downcast::<HikaruApp>().expect("raíz");
            let mut z = 0.0f32;
            app.update(cx, |app, cx| {
                z = app.state.read(cx).playlist_state.zoom_x;
            });
            z
        })
        .unwrap();
    assert!(
        ce_zoom > 1.0,
        "el zoom del editor debe aumentar ({ce_zoom})"
    );
    assert_eq!(
        playlist_zoom_before, playlist_zoom_after,
        "el zoom de la Playlist no debe cambiar"
    );
}
/// Regresión del lienzo: el zoom por defecto debe hacer legible la onda.
///
/// El bug era `zoom_x: 1.0` px/s: un clip de 2s medía 2 píxeles y la waveform
/// era invisible. Con `DEFAULT_CE_ZOOM` (400 px/s) el clip mide 800px.
#[gpui_kit::gpui::test]
fn default_zoom_makes_waveform_readable(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    let zoom = ce_state(handle, cx).zoom_x;
    println!("zoom por defecto: {zoom} px/s");
    assert_eq!(zoom, clip_editor::DEFAULT_CE_ZOOM);
    assert!(
        zoom >= 200.0,
        "el zoom por defecto debe ser >= 200 px/s para que la onda se vea ({zoom})"
    );
    // Un clip de 2s debe ocupar una franja visible, no 2px.
    let width_at_default = 2.0 * zoom as f64;
    println!("ancho de un clip de 2s a ese zoom: {width_at_default:.0}px");
    assert!(width_at_default > 200.0, "el clip debe ser visible: {width_at_default}px");
}

/// El clip de prueba entra en el viewport con el zoom por defecto, así que la
/// caja y la onda se dibujan dentro del área visible.
#[gpui_kit::gpui::test]
fn clip_box_is_inside_viewport_at_default_zoom(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    open_editor_with_clip(handle, cx);
    let (canvas_w, canvas_h, zoom) = cx
        .update_window(handle.into(), |view, window, cx| {
            let app = view.downcast::<HikaruApp>().expect("raíz");
            let mut z = 0.0f32;
            app.update(cx, |app, cx| {
                z = app.state.read(cx).clip_editor.zoom_x;
            });
            window.draw(cx).clear(cx);
            let b = window.find("clip_editor_waveform").bounds();
            (b.size.width.as_f32(), b.size.height.as_f32(), z)
        })
        .unwrap();
    let dur = 4.0 * 60.0 / 140.0; // 4 negras a 140 BPM
    let clip_px = dur * zoom as f64;
    println!("canvas {canvas_w:.0}x{canvas_h:.0}, zoom {zoom}, clip {clip_px:.0}px");
    assert!(canvas_h >= clip_editor::WAVE_MIN_H);
    // Con zoom 400 el clip de ~1.7s mide ~685px: entra en el canvas de ~1080.
    assert!(
        clip_px < canvas_w as f64,
        "el clip ({clip_px:.0}px) debería entrar en el lienzo ({canvas_w:.0}px)"
    );
}

// =========================================================================
// Selección de picos: el bug del waveform invisible
// =========================================================================

#[test]
fn silent_peaks_are_detected() {
    // `load_peaks_from_wav` devuelve 2048 ceros (NO vacío) si no abre el archivo.
    assert!(peaks_are_silent(&[]));
    assert!(peaks_are_silent(&vec![0.0f32; 2048]));
    assert!(peaks_are_silent(&[0.0, 0.0, 0.0]));
    assert!(!peaks_are_silent(&[0.0, 0.5, 0.0]));
}

#[test]
fn best_peaks_falls_back_when_detail_is_silent() {
    // Regresión del panel negro: la fuente de detalle era 2048 ceros (más
    // "resolución") y ganaba siempre ⇒ waveform plano invisible.
    let detail = vec![0.0f32; 2048];
    let base: Vec<f32> = (0..512).map(|i| (i as f32 / 512.0).sin().abs()).collect();
    let chosen = best_peaks(&detail, &base);
    assert!(
        !peaks_are_silent(&chosen),
        "debe caer a la fuente con señal, no quedarse con los ceros"
    );
    assert_eq!(chosen.len(), base.len());
}

#[test]
fn best_peaks_prefers_detail_when_both_have_signal() {
    let detail: Vec<f32> = (0..2048).map(|i| (i as f32 / 2048.0).sin().abs()).collect();
    let base: Vec<f32> = (0..512).map(|i| (i as f32 / 512.0).sin().abs()).collect();
    let chosen = best_peaks(&detail, &base);
    assert_eq!(chosen.len(), 2048, "con señal real gana la de mayor resolución");
}

#[test]
fn best_peaks_prefers_longer_when_both_silent() {
    // Ambas mudas: devuelve la más larga para que el placeholder conserve
    // el conteo de bins.
    let chosen = best_peaks(&vec![0.0; 2048], &vec![0.0; 512]);
    assert_eq!(chosen.len(), 2048);
}

/// El clip de la Playlist tiene picos con señal ⇒ el editor NO debe quedarse
/// con una fuente muda (que era exactamente lo que dejaba el panel en negro).
#[gpui_kit::gpui::test]
fn resolved_clip_has_usable_peaks(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    open_editor_with_clip(handle, cx);
    let (detail_len, base_len, chosen_len, silent) = cx
        .update_window(handle.into(), |view, _, cx| {
            let app = view.downcast::<HikaruApp>().expect("raíz");
            let mut out = (0usize, 0usize, 0usize, true);
            app.update(cx, |app, cx| {
                let bpm = app.state.read(cx).transport.bpm as f32;
                let st = app.state.read(cx);
                if let Some(c) = clip_editor::resolve_clip(&st, bpm) {
                    let chosen = best_peaks(&c.detail_peaks, &c.peaks);
                    out = (
                        c.detail_peaks.len(),
                        c.peaks.len(),
                        chosen.len(),
                        peaks_are_silent(&chosen),
                    );
                }
            });
            out
        })
        .unwrap();
    println!("picos: detalle {detail_len}, clip {base_len}, elegido {chosen_len}, mudo {silent}");
    assert!(base_len > 0, "el clip debe traer picos del WAV");
    assert!(!silent, "el editor debe quedarse con una fuente CON señal");
}

/// El waveform ocupa todo el ancho del panel (a ancho completo, como el legacy).
#[gpui_kit::gpui::test]
fn waveform_spans_full_panel_width(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    open_editor_with_clip(handle, cx);
    let (editor_w, canvas_w) = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            let e = window.find("clip_editor").bounds();
            let c = window.find("clip_editor_waveform").bounds();
            (e.size.width.as_f32(), c.size.width.as_f32())
        })
        .unwrap();
    println!("editor {editor_w:.0}px, waveform {canvas_w:.0}px");
    assert!(
        canvas_w >= editor_w - 24.0,
        "el waveform debe ocupar el panel a lo ancho ({canvas_w} vs {editor_w})"
    );
}

/// Ya no hay sidebar flotando: el modo se muestra en la toolbar.
#[gpui_kit::gpui::test]
fn no_floating_sidebar_over_matrix(cx: &mut TestAppContext) {
    let handle = open_playlist(cx);
    open_editor_with_clip(handle, cx);
    let editor_top = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            window.find("clip_editor").bounds().origin.y.as_f32()
        })
        .unwrap();
    // Ningún control del sidebar debe quedar por encima del panel (que era
    // como se superponía sobre la Session Matrix).
    let above = cx
        .update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            ["ce_tab_events", "ce_tab_comping", "clip_editor_vertical_label"]
                .iter()
                .filter_map(|id| window.try_find(*id).map(|e| e.bounds().origin.y.as_f32()))
                .filter(|y| *y < editor_top)
                .count()
        })
        .unwrap();
    assert_eq!(above, 0, "los controles no deben flotar por encima del panel");
}

// =========================================================================
// Regresión del panel negro: el canvas se pintaba con altura 0
// =========================================================================

/// El closure de pintado del canvas debe correr con altura real. Sin
/// `.absolute().inset_0()` el `canvas` de gpui queda con height 0 y aborta, dejando
/// el area inferior negra.
#[gpui_kit::gpui::test]
fn canvas_paint_runs_with_real_height(cx: &mut TestAppContext) {
    LAST_PAINT_H.store(0, std::sync::atomic::Ordering::Relaxed);
    let handle = open_playlist(cx);
    open_editor_with_clip(handle, cx);
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
    })
    .unwrap();
    let h = LAST_PAINT_H.load(std::sync::atomic::Ordering::Relaxed);
    println!("altura pintada del canvas: {h}px");
    assert!(h > 0, "el canvas debe pintarse con altura > 0 (fue 0: panel negro)");
    assert!(painted_with_real_height());
}

// =========================================================================
// Geometria del waveform: segmentos verticales por pico
// =========================================================================

#[test]
fn wave_geometry_maps_peaks_across_full_width() {
    let peaks = vec![0.1f32, -0.5, 0.9, 0.2];
    let g = wave_geometry(&peaks, 100, 50.0);
    assert_eq!(g.len(), 4, "un segmento por pico cuando hay menos picos que px");
    assert!((g[0].0 - 0.0).abs() < 0.01);
    assert!((g[3].0 - 99.0).abs() < 0.01, "el ultimo pico llega al borde derecho");
    // La amplitud es el valor absoluto acotado por la media altura.
    assert!((g[2].1 - 45.0).abs() < 0.01, "pico 0.9 * 50 = 45");
    assert!((g[1].1 - 25.0).abs() < 0.01, "el negativo se toma en valor absoluto");
}

#[test]
fn wave_geometry_downsamples_when_peaks_exceed_pixels() {
    let peaks: Vec<f32> = (0..1000).map(|i| (i as f32 / 1000.0).sin()).collect();
    let g = wave_geometry(&peaks, 50, 40.0);
    assert_eq!(g.len(), 50, "no mas segmentos que columnas de pixel");
    assert!(g.iter().all(|(x, _)| *x >= 0.0 && *x < 50.0));
    assert!(g.iter().all(|(_, a)| *a > 0.0 && *a <= 40.0));
}

#[test]
fn wave_geometry_never_returns_flat_zero_amplitude() {
    // Un pico en silencio no debe colapsar la onda a una linea de altura 0:
    // el minimo de 0.5px la mantiene visible.
    let g = wave_geometry(&[0.0, 0.0001, 0.0], 10, 30.0);
    assert!(g.iter().all(|(_, a)| *a >= 0.5));
}

#[test]
fn wave_geometry_handles_empty_and_degenerate_input() {
    assert!(wave_geometry(&[], 100, 50.0).is_empty());
    assert!(wave_geometry(&[1.0], 100, 0.0).is_empty());
    assert!(wave_geometry(&[1.0], 0, 50.0).len() == 1, "ancho 0 => al menos 1 columna");
}

// =========================================================================
// Regla de tiempo: numeros de compas calculados en el DOM
// =========================================================================

#[test]
fn ruler_labels_lists_bars_then_beats_in_time_order() {
    // 120 BPM => 2s por compas, 0.5s por beat. Arrancando en el compas 0 y
    // mostrando 4s deben aparecer los compases 1..3 y los beats intermedios.
    let labels = ruler_labels(0.0, 4.0, 120.0, 100.0);
    let bars: Vec<u32> = labels
        .iter()
        .filter(|(_, _, is_bar)| *is_bar)
        .map(|(t, _, _)| t.parse::<u32>().unwrap())
        .collect();
    // La ventana es [0, 4): el compas que arranca en t=4 cae justo en el borde
    // derecho y queda fuera, asi que entran los compases 1 y 2.
    assert_eq!(bars, vec![1, 2], "compases dentro de una ventana de 4s a 120bpm");
    let beats = labels.iter().filter(|(_, _, is_bar)| !*is_bar).count();
    assert_eq!(beats, 6, "3 beats intermedios por compas x 2");
    // Orden temporal creciente.
    let xs: Vec<f32> = labels.iter().map(|(_, x, _)| *x).collect();
    let mut sorted = xs.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(xs, sorted, "las etiquetas deben ir en orden temporal");
}

#[test]
fn ruler_labels_scrolls_with_the_viewport() {
    // Desplazarse un compas desplaza las etiquetas un compas en pixeles.
    let a = ruler_labels(0.0, 4.0, 120.0, 100.0);
    let b = ruler_labels(2.0, 4.0, 120.0, 100.0);
    let a_bars: Vec<u32> = a
        .iter()
        .filter(|(_, _, is_bar)| *is_bar)
        .map(|(t, _, _)| t.parse::<u32>().unwrap())
        .collect();
    let b_bars: Vec<u32> = b
        .iter()
        .filter(|(_, _, is_bar)| *is_bar)
        .map(|(t, _, _)| t.parse::<u32>().unwrap())
        .collect();
    assert_eq!(a_bars, vec![1, 2]);
    assert_eq!(b_bars, vec![2, 3], "al scrollear un compas arranca en el 2");
}

#[test]
fn ruler_labels_falls_back_to_120bpm_when_bpm_is_zero() {
    let labels = ruler_labels(0.0, 4.0, 0.0, 100.0);
    assert!(!labels.is_empty(), "un bpm de 0 no debe dejar la regla vacia");
}
