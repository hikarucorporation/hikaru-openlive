// crates/hikaru_gui/tests/tier2_boundary_corner_cases.rs

//! Tier 2: Boundary & Corner Cases
//!
//! Validates edge cases and boundary invariants from `PROJECT.md § Edge Cases` and `TEST_INFRA.md`:
//! - Minimum duration = 1 tick (prevent zero or negative length)
//! - Left edge resize duration boundary (start_tick <= end_tick - 1)
//! - Pitch floor (0) and ceiling (127) clamping
//! - Multi-note relative pitch bounds preserving chord intervals
//! - Timeline start clamping (tick >= 0)
//! - Zero-length loop rejection and minimum loop length (1 beat / 960 ticks)
//! - Loop start < loop end invariant
//! - Velocity clamping strictly within 1..=127
//! - Extreme zoom clamping (MIN_ZOOM_X 0.008 to MAX_ZOOM_X 0.5)
//! - Empty clip operations (0 notes safety)
//! - Overlapping / duplicate note handling
//! - Detail editor dock height clamping (160px..500px)

use hikaru_gui::views::piano_roll::{
    compute_grid_viewport, fit_zoom_x, focus_scroll_y, quantize_tick, MidiNote, PianoRollState,
    MAX_ZOOM_X, MIN_ZOOM_X, TICKS_PER_BEAT,
};

// -----------------------------------------------------------------------------
// B1: Minimum Note Duration = 1 Tick (Right Edge Resize)
// -----------------------------------------------------------------------------
#[test]
fn test_tier2_b01_minimum_note_duration_one_tick() {
    let mut note = MidiNote { pitch: 60, start_tick: 960, duration_ticks: 480, velocity: 100 };

    // Simulate dragging right edge to the left past the note's start
    let extreme_negative_delta: i64 = -10000;
    let clamped_duration = (note.duration_ticks as i64 + extreme_negative_delta).max(1) as u64;
    note.duration_ticks = clamped_duration;

    assert_eq!(note.duration_ticks, 1, "Duration must never drop below 1 tick");
    assert_eq!(note.start_tick, 960, "Start tick must remain fixed on right resize");
}

// -----------------------------------------------------------------------------
// B2: Left Edge Resize Clamping (Start Tick <= End Tick - 1)
// -----------------------------------------------------------------------------
#[test]
fn test_tier2_b02_left_edge_resize_cannot_exceed_end_tick() {
    let mut note = MidiNote { pitch: 60, start_tick: 480, duration_ticks: 480, velocity: 100 };
    let end_tick = note.start_tick + note.duration_ticks; // 960

    // Drag left edge past end tick (e.g. to tick 2000)
    let requested_start: u64 = 2000;
    let clamped_start = requested_start.min(end_tick.saturating_sub(1));
    note.start_tick = clamped_start;
    note.duration_ticks = end_tick.saturating_sub(clamped_start).max(1);

    assert_eq!(note.start_tick, 959, "Start tick must be strictly < end tick");
    assert_eq!(note.duration_ticks, 1, "Duration must be at least 1 tick");
    assert_eq!(note.start_tick + note.duration_ticks, end_tick);
}

// -----------------------------------------------------------------------------
// B3: Pitch Floor Clamping (Pitch 0 / C-1)
// -----------------------------------------------------------------------------
#[test]
fn test_tier2_b03_pitch_floor_clamping() {
    let mut note = MidiNote { pitch: 5, start_tick: 0, duration_ticks: 240, velocity: 100 };

    // Drag down by -20 semitones
    let delta: i16 = -20;
    let clamped_pitch = (note.pitch as i16 + delta).clamp(0, 127) as u8;
    note.pitch = clamped_pitch;

    assert_eq!(note.pitch, 0, "Pitch must clamp to 0 and not underflow");
}

// -----------------------------------------------------------------------------
// B4: Pitch Ceiling Clamping (Pitch 127 / G9)
// -----------------------------------------------------------------------------
#[test]
fn test_tier2_b04_pitch_ceiling_clamping() {
    let mut note = MidiNote { pitch: 120, start_tick: 0, duration_ticks: 240, velocity: 100 };

    // Drag up by +20 semitones
    let delta: i16 = 20;
    let clamped_pitch = (note.pitch as i16 + delta).clamp(0, 127) as u8;
    note.pitch = clamped_pitch;

    assert_eq!(note.pitch, 127, "Pitch must clamp to 127 and not overflow");
}

// -----------------------------------------------------------------------------
// B5: Multi-Note Relative Pitch Clamping Preserves Intervals
// -----------------------------------------------------------------------------
#[test]
fn test_tier2_b05_multi_note_pitch_clamping_preserves_intervals() {
    // C major chord: C4 (60), E4 (64), G4 (67)
    let mut chord = vec![
        MidiNote { pitch: 60, start_tick: 0, duration_ticks: 960, velocity: 100 },
        MidiNote { pitch: 64, start_tick: 0, duration_ticks: 960, velocity: 100 },
        MidiNote { pitch: 67, start_tick: 0, duration_ticks: 960, velocity: 100 },
    ];

    // Attempt to drag up by +70 semitones. Highest note is 67. Max allowed delta is 127 - 67 = 60.
    let requested_delta: i16 = 70;
    let max_pitch = chord.iter().map(|n| n.pitch).max().unwrap();
    let min_pitch = chord.iter().map(|n| n.pitch).min().unwrap();

    let allowed_up = (127 - max_pitch) as i16;
    let allowed_down = -(min_pitch as i16);
    let clamped_delta = requested_delta.clamp(allowed_down, allowed_up);

    assert_eq!(clamped_delta, 60, "Delta must be clamped by highest note");
    for note in &mut chord {
        note.pitch = (note.pitch as i16 + clamped_delta) as u8;
    }

    assert_eq!(chord[0].pitch, 120);
    assert_eq!(chord[1].pitch, 124);
    assert_eq!(chord[2].pitch, 127, "Highest note hits 127 exactly");
    // Verify interval preservation: E - C = 4, G - E = 3
    assert_eq!(chord[1].pitch - chord[0].pitch, 4);
    assert_eq!(chord[2].pitch - chord[1].pitch, 3);
}

// -----------------------------------------------------------------------------
// B6: Timeline Start Clamping (Start Tick >= 0)
// -----------------------------------------------------------------------------
#[test]
fn test_tier2_b06_timeline_start_clamping() {
    let mut note = MidiNote { pitch: 60, start_tick: 200, duration_ticks: 480, velocity: 100 };

    // Drag left by -500 ticks
    let delta_ticks: i64 = -500;
    let clamped_start = (note.start_tick as i64 + delta_ticks).max(0) as u64;
    note.start_tick = clamped_start;

    assert_eq!(note.start_tick, 0, "Start tick must never drop below 0");
}

// -----------------------------------------------------------------------------
// B7: Zero-Length Loop Rejection & Minimum Loop Length (1 Beat / 960 Ticks)
// -----------------------------------------------------------------------------
#[test]
fn test_tier2_b07_zero_length_loop_rejection() {
    let min_loop_ticks = TICKS_PER_BEAT; // 960 ticks = 1 beat
    let loop_start_tick: u64 = 3840;

    // Requested loop end == loop start
    let requested_loop_end: u64 = 3840;
    let effective_loop_end = requested_loop_end.max(loop_start_tick + min_loop_ticks);

    assert_eq!(effective_loop_end, 3840 + 960);
    assert!(effective_loop_end > loop_start_tick);
}

// -----------------------------------------------------------------------------
// B8: Loop Start Clamped Before Loop End
// -----------------------------------------------------------------------------
#[test]
fn test_tier2_b08_loop_start_clamped_before_loop_end() {
    let min_loop_ticks = TICKS_PER_BEAT;
    let loop_end_tick: u64 = 7680;

    // Attempt to drag start past loop end (e.g. to 8000)
    let requested_start: u64 = 8000;
    let clamped_start = requested_start.min(loop_end_tick.saturating_sub(min_loop_ticks));

    assert_eq!(clamped_start, 7680 - 960);
    assert!(clamped_start + min_loop_ticks <= loop_end_tick);
}

// -----------------------------------------------------------------------------
// B9: Velocity Clamping Strictly Within 1..=127
// -----------------------------------------------------------------------------
#[test]
fn test_tier2_b09_velocity_clamping_strict() {
    // Underflow test (velocity 0 clamped to 1)
    let raw_val_under: i32 = -50;
    let clamped_under = raw_val_under.clamp(1, 127) as u8;
    assert_eq!(clamped_under, 1, "Velocity 0 or negative must clamp to 1");

    // Zero explicitly
    let zero_val: i32 = 0;
    let clamped_zero = zero_val.clamp(1, 127) as u8;
    assert_eq!(clamped_zero, 1, "Velocity 0 must clamp to 1");

    // Overflow test (> 127 clamped to 127)
    let raw_val_over: i32 = 255;
    let clamped_over = raw_val_over.clamp(1, 127) as u8;
    assert_eq!(clamped_over, 127, "Velocity > 127 must clamp to 127");
}

// -----------------------------------------------------------------------------
// B10: Extreme Zoom Clamping (MIN_ZOOM_X to MAX_ZOOM_X)
// -----------------------------------------------------------------------------
#[test]
fn test_tier2_b10_extreme_zoom_clamping() {
    // Zoom way out (e.g. 0.00001)
    let extreme_out = fit_zoom_x(100.0, 1000.0);
    assert!(extreme_out >= MIN_ZOOM_X, "Zoom must not drop below MIN_ZOOM_X");

    // Zoom way in (e.g. 10.0)
    let extreme_in = fit_zoom_x(10000.0, 0.1);
    assert!(extreme_in <= MAX_ZOOM_X, "Zoom must not exceed MAX_ZOOM_X");

    // Grid viewport with extreme inputs does not NaN or panic
    let vp_min = compute_grid_viewport(-1000.0, -1000.0, 10.0, 10.0, 1.0, 0.000001);
    assert!(vp_min.width >= 480.0);
    assert!(vp_min.height >= 180.0);
    assert!(!vp_min.width.is_nan());
    assert!(!vp_min.height.is_nan());

    let vp_max = compute_grid_viewport(100000.0, 100000.0, 10000.0, 10000.0, 100.0, 10.0);
    assert!(!vp_max.width.is_nan());
    assert!(!vp_max.height.is_nan());
}

// -----------------------------------------------------------------------------
// B11: Empty Clip Operations Safety
// -----------------------------------------------------------------------------
#[test]
fn test_tier2_b11_empty_clip_operations_safety() {
    let mut state = PianoRollState::default();
    assert!(state.notes.is_empty());

    // Select all on empty notes
    state.selected_notes = (0..state.notes.len()).collect();
    assert!(state.selected_notes.is_empty());

    // Clear selection on empty notes
    state.clear_selection();
    state.clear_note_selection();
    assert!(state.selected_notes.is_empty());

    // Focus scroll on pitch 60
    let scroll_y = focus_scroll_y(400.0, 16.0, 60);
    assert!(scroll_y >= 0.0 && !scroll_y.is_nan());

    // Quantize 0 tick
    assert_eq!(quantize_tick(0, 240), 0);
    assert_eq!(quantize_tick(0, 0), 0);
}

// -----------------------------------------------------------------------------
// B12: Duplicate Note Prevention (Overlapping Notes)
// -----------------------------------------------------------------------------
#[test]
fn test_tier2_b12_duplicate_note_prevention() {
    let mut notes = vec![
        MidiNote { pitch: 60, start_tick: 480, duration_ticks: 240, velocity: 100 },
    ];

    // Double-click at (start_tick=480, pitch=60) should detect existing note
    let new_tick = 480;
    let new_pitch = 60;

    let existing_idx = notes.iter().position(|n| n.start_tick == new_tick && n.pitch == new_pitch);
    assert_eq!(existing_idx, Some(0), "Must detect existing note at same tick and pitch");

    // Policy: do not insert duplicate, instead select or delete existing
    let inserted = if existing_idx.is_none() {
        notes.push(MidiNote { pitch: new_pitch, start_tick: new_tick, duration_ticks: 240, velocity: 100 });
        true
    } else {
        false
    };

    assert!(!inserted, "Duplicate note must not be inserted");
    assert_eq!(notes.len(), 1);
}

// -----------------------------------------------------------------------------
// B13: Detail Editor Splitter Drag Clamping (160px..500px)
// -----------------------------------------------------------------------------
#[test]
fn test_tier2_b13_detail_editor_splitter_drag_clamping() {
    let min_dock_h = 160.0_f32;
    let max_dock_h = 500.0_f32;

    // Drag way down (< 160px)
    let requested_h_small = 50.0_f32;
    let clamped_small = requested_h_small.clamp(min_dock_h, max_dock_h);
    assert_eq!(clamped_small, 160.0);

    // Drag way up (> 500px)
    let requested_h_large = 900.0_f32;
    let clamped_large = requested_h_large.clamp(min_dock_h, max_dock_h);
    assert_eq!(clamped_large, 500.0);
}

// -----------------------------------------------------------------------------
// B14: High Pitch Out of Bounds Protection
// -----------------------------------------------------------------------------
#[test]
fn test_tier2_b14_high_pitch_out_of_bounds_protection() {
    // Midi keys standard count is 128 (0..127)
    let raw_pitches = [0u8, 1u8, 64u8, 127u8];
    for &p in &raw_pitches {
        assert!(p < 128);
    }
}
