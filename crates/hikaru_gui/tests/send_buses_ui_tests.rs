// Integration tests for Send Buses UI (Bitwig 5.1.7 Parity: gr-16, mx-19).

use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::mpsc::channel;
use std::sync::Arc;

use hikaru_gui::app::AppState;
use hikaru_gui::audio_proxy::AudioProxy;
use hikaru_gui::project::openlive;
use hikaru_gui::views::mixer::{self, Track, ROUTE_FX_BASE};

fn make_app_state() -> AppState {
    let (tx, _rx) = channel();
    let proxy = AudioProxy::new(tx);
    AppState::new(
        proxy,
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU32::new(0)),
        None,
    )
}

#[test]
fn track_send_amount_clamps_updates_and_clears_at_zero() {
    let mut track = Track::new(1, "Track 1".to_string(), false);
    let fx0 = ROUTE_FX_BASE;
    let fx1 = ROUTE_FX_BASE + 1;

    assert_eq!(track.send_amount(fx0), 0.0);
    assert_eq!(mixer::send_db_text(0.0), "-inf dB");

    track.set_send_amount(fx0, 0.75);
    assert!((track.send_amount(fx0) - 0.75).abs() < 1e-6);
    assert_eq!(track.sends.len(), 1);
    assert_eq!(mixer::send_db_text(0.75), "0.0 dB");

    // Clamping > 1.0 and NaN
    track.set_send_amount(fx1, 2.5);
    assert!((track.send_amount(fx1) - 1.0).abs() < 1e-6);
    track.set_send_amount(fx1, f32::NAN);
    assert_eq!(track.send_amount(fx1), 0.0);
    assert_eq!(track.sends.len(), 1);

    // Clearing at 0.0 removes the connection
    track.set_send_amount(fx0, 0.0);
    assert_eq!(track.send_amount(fx0), 0.0);
    assert!(track.sends.is_empty());
}

#[test]
fn add_and_delete_fx_send_buses_reindexes_track_sends() {
    let mut state = make_app_state();
    assert_eq!(state.fx_tracks.len(), 1);

    let fx1 = state.add_fx_track();
    let fx2 = state.add_fx_track();
    assert_eq!(fx1, 1);
    assert_eq!(fx2, 2);
    assert_eq!(state.fx_tracks.len(), 3);

    // Route Track 1 to FX 1 (idx 0), FX 2 (idx 1), FX 3 (idx 2)
    let tr = &mut state.live_tracks[1];
    tr.set_send_amount(ROUTE_FX_BASE, 0.25);
    tr.set_send_amount(ROUTE_FX_BASE + 1, 0.50);
    tr.set_send_amount(ROUTE_FX_BASE + 2, 0.80);

    // Delete FX 2 (idx 1): FX 1 stays at BASE+0, FX 3 shifts down to BASE+1
    state.delete_fx_track(1);
    assert_eq!(state.fx_tracks.len(), 2);

    let tr = &state.live_tracks[1];
    assert!((tr.send_amount(ROUTE_FX_BASE) - 0.25).abs() < 1e-6);
    assert!((tr.send_amount(ROUTE_FX_BASE + 1) - 0.80).abs() < 1e-6);
    assert_eq!(tr.send_amount(ROUTE_FX_BASE + 2), 0.0);
}

#[test]
fn bottom_mixer_panel_renders_send_buses_ui_with_and_without_sends() {
    let mut state = make_app_state();
    state.add_fx_track();
    state.live_tracks[1].set_send_amount(ROUTE_FX_BASE, 0.6);
    state.live_tracks[1].set_send_amount(ROUTE_FX_BASE + 1, 0.3);

    let ctx = egui::Context::default();
    hikaru_gui::ui::fonts::ensure_installed(&ctx);
    for show_sends in [true, false] {
        state.bottom_mixer_show_sends = show_sends;
        let mut raw_input = egui::RawInput::default();
        raw_input.screen_rect = Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1280.0, 300.0),
        ));
        let _ = ctx.run(raw_input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                mixer::render_bottom_panel(ui, &mut state);
            });
        });
    }

    assert!((state.live_tracks[1].send_amount(ROUTE_FX_BASE) - 0.6).abs() < 1e-6);
    assert!((state.live_tracks[1].send_amount(ROUTE_FX_BASE + 1) - 0.3).abs() < 1e-6);
}

#[test]
fn track_sends_persist_in_oplf_roundtrip() {
    let mut state = make_app_state();
    state.add_fx_track();
    state.live_tracks[1].set_send_amount(ROUTE_FX_BASE, 0.42);
    state.live_tracks[1].set_send_amount(ROUTE_FX_BASE + 1, 0.88);

    let snapshot = openlive::capture(&state);
    let mut restored = make_app_state();
    openlive::apply(&mut restored, &snapshot, None);

    assert!((restored.live_tracks[1].send_amount(ROUTE_FX_BASE) - 0.42).abs() < 1e-5);
    assert!((restored.live_tracks[1].send_amount(ROUTE_FX_BASE + 1) - 0.88).abs() < 1e-5);
}
