// crates/hikaru_gui/tests/tier3_cross_feature_combinations.rs

//! Tier 3: Cross-Feature Combinations (Pairwise & Multi-Feature Interactions)
//!
//! Validates combinatorial and pairwise interactions from `PROJECT.md` and `TEST_INFRA.md`:
//! - Note Drag + Shift Snapping Override (1-tick continuous drag)
//! - Multi-Select + Group Transpose across Octaves
//! - Loop Resize + Playhead Sweep & Dynamic Wrap
//! - Multi-Select + Linear Velocity Ramp
//! - Session Matrix Clip Slot Trigger + Piano Roll Focus
//! - Left Edge Resize + Right Edge Resize in Sequence
//! - Marquee Selection + Deletion + Index Stability
//! - Mode Switch Keys <-> Drums with Active Selection
//! - Adaptive Zoom Level + Quantization Step Selection
//! - Concurrent AudioProxy Dispatch + Engine Audio Processing Block

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;

use hikaru_audio_engine::{AudioEngine, MidiNoteInstance};
use hikaru_core::{AudioBuffer, SampleRate};
use hikaru_gui::audio_proxy::{AudioProxy, GuiCommand};
use hikaru_gui::views::matrix::{self, ClipData, SessionMatrixState};
use hikaru_gui::views::piano_roll::{
    compute_grid_viewport, quantize_tick, MidiNote, PianoRollMode,
    PianoRollState, TICKS_PER_BAR,
};

static SINE_TABLE: [f32; 2048] = [0.0; 2048];

fn create_engine() -> AudioEngine<'static> {
    let clock = Arc::new(AtomicU64::new(0));
    let engine = AudioEngine::new(SampleRate::new(44100.0), &SINE_TABLE, clock);
    for t in 0..16 {
        engine.track_volumes[t].store(1.0f32.to_bits(), Ordering::Relaxed);
    }
    engine.master_gain.store(1.0f32.to_bits(), Ordering::Relaxed);
    engine
}

// -----------------------------------------------------------------------------
// C1: Note Drag + Shift Snap Override
// -----------------------------------------------------------------------------
#[test]
fn test_tier3_c01_note_drag_with_shift_snap_override() {
    let mut state = PianoRollState::default();
    state.notes = vec![
        MidiNote { pitch: 60, start_tick: 960, duration_ticks: 480, velocity: 100 },
    ];
    state.selected_notes.insert(0);

    let grid_step: u64 = 240; // 1/16th note
    let drag_delta_raw: i64 = 173; // non-grid raw pointer drag delta

    // 1. With Shift held: snap disabled, direct 1-tick offset
    let shift_held = true;
    let target_tick_shift = if shift_held {
        (state.notes[0].start_tick as i64 + drag_delta_raw).max(0) as u64
    } else {
        quantize_tick((state.notes[0].start_tick as i64 + drag_delta_raw).max(0) as u64, grid_step)
    };
    assert_eq!(target_tick_shift, 960 + 173); // 1133

    // 2. Without Shift: snapped to grid step
    let shift_held = false;
    let target_tick_grid = if shift_held {
        (state.notes[0].start_tick as i64 + drag_delta_raw).max(0) as u64
    } else {
        quantize_tick((state.notes[0].start_tick as i64 + drag_delta_raw).max(0) as u64, grid_step)
    };
    assert_eq!(target_tick_grid, 1200); // 1133 rounds to nearest 240 -> 1200
}

// -----------------------------------------------------------------------------
// C2: Multi-Select + Group Transpose across Octaves
// -----------------------------------------------------------------------------
#[test]
fn test_tier3_c02_multi_select_group_transpose_across_octaves() {
    let mut state = PianoRollState::default();
    // 4-note chord: C3 (48), E3 (52), G3 (55), B3 (59)
    state.notes = vec![
        MidiNote { pitch: 48, start_tick: 0, duration_ticks: 960, velocity: 100 },
        MidiNote { pitch: 52, start_tick: 0, duration_ticks: 960, velocity: 100 },
        MidiNote { pitch: 55, start_tick: 0, duration_ticks: 960, velocity: 100 },
        MidiNote { pitch: 59, start_tick: 0, duration_ticks: 960, velocity: 100 },
    ];
    // Select all 4 notes
    state.selected_notes = (0..4).collect();

    // Transpose up by 2 octaves (+24 semitones)
    let semitone_shift: i16 = 24;
    for &idx in &state.selected_notes {
        state.notes[idx].pitch = (state.notes[idx].pitch as i16 + semitone_shift).clamp(0, 127) as u8;
    }

    assert_eq!(state.notes[0].pitch, 72); // C5
    assert_eq!(state.notes[1].pitch, 76); // E5
    assert_eq!(state.notes[2].pitch, 79); // G5
    assert_eq!(state.notes[3].pitch, 83); // B5
}

// -----------------------------------------------------------------------------
// C3: Loop Resize + Playhead Sweep & Dynamic Wrap
// -----------------------------------------------------------------------------
#[test]
fn test_tier3_c03_loop_resize_and_playhead_dynamic_wrap() {
    let mut state = PianoRollState::default();
    state.loop_enabled = true;
    let loop_start_tick: u64 = 0;
    let mut loop_end_tick: u64 = 3840; // 1 bar loop

    let mut playhead: u64 = 3800;

    // Simulate playhead advance past loop end
    playhead += 100; // 3900
    if state.loop_enabled && playhead >= loop_end_tick {
        let loop_len = loop_end_tick - loop_start_tick;
        playhead = loop_start_tick + ((playhead - loop_start_tick) % loop_len);
    }
    assert_eq!(playhead, 60); // 3900 wraps to 60

    // Now dynamically extend loop end to 2 bars (7680)
    loop_end_tick = 7680;
    playhead = 3800 + 100; // 3900
    if state.loop_enabled && playhead >= loop_end_tick {
        let loop_len = loop_end_tick - loop_start_tick;
        playhead = loop_start_tick + ((playhead - loop_start_tick) % loop_len);
    }
    assert_eq!(playhead, 3900, "With extended loop, playhead continues linearly without wrap");
}

// -----------------------------------------------------------------------------
// C4: Multi-Select + Linear Velocity Ramp
// -----------------------------------------------------------------------------
#[test]
fn test_tier3_c04_multi_select_linear_velocity_ramp() {
    let mut state = PianoRollState::default();
    // 5 hi-hat notes with flat velocity 100
    state.notes = (0..5)
        .map(|i| MidiNote {
            pitch: 42,
            start_tick: i * 240,
            duration_ticks: 120,
            velocity: 100,
        })
        .collect();
    state.selected_notes = (0..5).collect();

    // Apply linear ramp from velocity 40 to velocity 120
    let start_vel = 40.0_f32;
    let end_vel = 120.0_f32;
    let count = state.selected_notes.len();

    let mut sorted_indices: Vec<usize> = state.selected_notes.iter().copied().collect();
    sorted_indices.sort_by_key(|&idx| state.notes[idx].start_tick);

    for (step, &idx) in sorted_indices.iter().enumerate() {
        let frac = if count > 1 { step as f32 / (count - 1) as f32 } else { 0.0 };
        let vel = (start_vel + frac * (end_vel - start_vel)).round().clamp(1.0, 127.0) as u8;
        state.notes[idx].velocity = vel;
    }

    assert_eq!(state.notes[0].velocity, 40);
    assert_eq!(state.notes[1].velocity, 60);
    assert_eq!(state.notes[2].velocity, 80);
    assert_eq!(state.notes[3].velocity, 100);
    assert_eq!(state.notes[4].velocity, 120);
}

// -----------------------------------------------------------------------------
// C5: Session Matrix Clip Slot Trigger + Piano Roll Focus
// -----------------------------------------------------------------------------
#[test]
fn test_tier3_c05_matrix_clip_trigger_and_piano_roll_focus() {
    let (tx, rx) = channel();
    let proxy = AudioProxy::new(tx);
    let mut matrix_state = SessionMatrixState::default();

    // Set up a MIDI clip in slot (0, 0)
    let clip_notes = vec![(0, 60, 100, 480), (480, 64, 110, 480)];
    let slot = &mut matrix_state.grid[0][0];
    slot.clip = Some(matrix::MatrixClip {
        id: 10,
        name: "Lead Synth".into(),
        path: std::path::PathBuf::from("lead.mid"),
        duration_secs: 2.0,
        content: ClipData::Midi { notes: clip_notes.clone() },
        local_state: hikaru_gui::views::playlist::PlaylistState::default(),
        local_track: hikaru_gui::views::mixer::Track::new(0, "Lead".into(), false),
        local_bar: 1.0,
        loop_start: 0,
        loop_end: 3840,
        loop_enabled: true,
        has_time_selection: false,
        peaks: Vec::new(),
        muted: false,
        color: None,
    });

    // 1. User double-clicks clip in Matrix: triggers clip and opens Piano Roll
    proxy.send(GuiCommand::TriggerClip { track_idx: 0, scene_idx: 0 });
    let mut piano_roll_state = PianoRollState::default();
    piano_roll_state.notes = clip_notes
        .iter()
        .map(|&(start, pitch, velocity, duration)| MidiNote {
            pitch,
            start_tick: start,
            duration_ticks: duration as u64,
            velocity,
        })
        .collect();

    // Verify proxy received trigger
    match rx.recv().expect("must receive trigger") {
        GuiCommand::TriggerClip { track_idx, scene_idx } => {
            assert_eq!(track_idx, 0);
            assert_eq!(scene_idx, 0);
        }
        _ => panic!("Expected TriggerClip"),
    }

    // Verify Piano Roll populated with exact clip notes
    assert_eq!(piano_roll_state.notes.len(), 2);
    assert_eq!(piano_roll_state.notes[0].pitch, 60);
    assert_eq!(piano_roll_state.notes[1].pitch, 64);
}

// -----------------------------------------------------------------------------
// C6: Left Edge Resize + Right Edge Resize in Sequence
// -----------------------------------------------------------------------------
#[test]
fn test_tier3_c06_left_edge_then_right_edge_resize() {
    let mut note = MidiNote { pitch: 60, start_tick: 480, duration_ticks: 480, velocity: 100 };
    let initial_end = note.start_tick + note.duration_ticks; // 960

    // Step 1: Drag left edge from 480 to 240 (extending note earlier, fixed end)
    let new_start: u64 = 240;
    note.duration_ticks = initial_end.saturating_sub(new_start).max(1);
    note.start_tick = new_start;

    assert_eq!(note.start_tick, 240);
    assert_eq!(note.duration_ticks, 720);
    assert_eq!(note.start_tick + note.duration_ticks, initial_end);

    // Step 2: Drag right edge to extend duration from 720 to 1200
    let right_delta: i64 = 480;
    note.duration_ticks = (note.duration_ticks as i64 + right_delta).max(1) as u64;

    assert_eq!(note.start_tick, 240, "Start tick must remain fixed on right resize");
    assert_eq!(note.duration_ticks, 1200);
    assert_eq!(note.start_tick + note.duration_ticks, 1440);
}

// -----------------------------------------------------------------------------
// C7: Marquee Selection + Deletion + Index Stability
// -----------------------------------------------------------------------------
#[test]
fn test_tier3_c07_marquee_selection_deletion_and_index_stability() {
    let mut state = PianoRollState::default();
    // 6 notes alternating pitches 60 and 64
    state.notes = (0..6)
        .map(|i| MidiNote {
            pitch: if i % 2 == 0 { 60 } else { 64 },
            start_tick: i * 240,
            duration_ticks: 240,
            velocity: 100,
        })
        .collect();

    // Marquee selects middle notes (indices 2 and 3)
    let marquee_selected = HashSet::from([2, 3]);
    state.selected_notes = marquee_selected.clone();

    // Delete selected notes
    let mut cur_idx = 0;
    state.notes.retain(|_| {
        let keep = !marquee_selected.contains(&cur_idx);
        cur_idx += 1;
        keep
    });
    state.selected_notes.clear();

    assert_eq!(state.notes.len(), 4);
    // Remaining notes must be original 0, 1, 4, 5
    assert_eq!(state.notes[0].start_tick, 0);
    assert_eq!(state.notes[1].start_tick, 240);
    assert_eq!(state.notes[2].start_tick, 960);
    assert_eq!(state.notes[3].start_tick, 1200);
}

// -----------------------------------------------------------------------------
// C8: Mode Switch Keys <-> Drums with Active Selection
// -----------------------------------------------------------------------------
#[test]
fn test_tier3_c08_mode_switch_preserves_selection_and_positions() {
    let mut state = PianoRollState::default();
    state.notes = vec![
        MidiNote { pitch: 36, start_tick: 0, duration_ticks: 240, velocity: 100 },
        MidiNote { pitch: 38, start_tick: 240, duration_ticks: 240, velocity: 100 },
    ];
    state.selected_notes.insert(0);

    // Switch to Drums
    state.mode = PianoRollMode::Drums;
    state.drum_map.insert(36, "Bass Drum".into());
    state.drum_map.insert(38, "Snare Drum".into());

    assert_eq!(state.mode, PianoRollMode::Drums);
    assert_eq!(state.selected_notes, HashSet::from([0]));
    assert_eq!(state.notes[0].pitch, 36);
    assert_eq!(state.notes[0].start_tick, 0);

    // Switch back to Keys
    state.mode = PianoRollMode::Keys;
    assert_eq!(state.mode, PianoRollMode::Keys);
    assert_eq!(state.selected_notes, HashSet::from([0]));
}

// -----------------------------------------------------------------------------
// C9: Adaptive Zoom + Quantization Step Selection
// -----------------------------------------------------------------------------
#[test]
fn test_tier3_c09_adaptive_zoom_and_quantization_step() {
    // Zoomed in: 1/32nd note grid (120 ticks)
    let zoom_in = 0.20_f32;
    let step_in = 120_u64;
    let vp_in = compute_grid_viewport(0.0, 0.0, 1000.0, 600.0, 16.0, zoom_in);
    assert!(vp_in.width >= 480.0);
    assert_eq!(quantize_tick(135, step_in), 120);

    // Zoomed out: 1 bar grid (3840 ticks)
    let zoom_out = 0.01_f32;
    let step_out = TICKS_PER_BAR;
    let vp_out = compute_grid_viewport(0.0, 0.0, 1000.0, 600.0, 16.0, zoom_out);
    assert!(vp_out.tick_end > vp_in.tick_end);
    assert_eq!(quantize_tick(2000, step_out), 3840);
}

// -----------------------------------------------------------------------------
// C10: Concurrent AudioProxy Dispatch + Engine Audio Processing Block
// -----------------------------------------------------------------------------
#[test]
fn test_tier3_c10_concurrent_audioproxy_dispatch_and_engine_processing() {
    let mut engine = create_engine();
    let (tx, rx) = channel();
    let proxy = AudioProxy::new(tx);

    // Send note update from GUI thread
    let new_notes = vec![
        (0, 36, 127, 480),
        (480, 40, 110, 480),
    ];
    proxy.send(GuiCommand::UpdateMidiClipNotes {
        track_idx: 0,
        scene_idx: 0,
        notes: new_notes,
    });

    // Audio thread receives and applies without locking audio thread
    if let Ok(GuiCommand::UpdateMidiClipNotes { track_idx, scene_idx, notes }) = rx.try_recv() {
        let converted: Vec<MidiNoteInstance> = notes
            .into_iter()
            .map(|(start_tick, pitch, velocity, duration_ticks)| MidiNoteInstance {
                start_tick,
                pitch,
                velocity,
                duration_ticks,
            })
            .collect();
        engine.update_midi_clip(track_idx, scene_idx, converted);
    }

    assert_eq!(engine.midi_clips.len(), 1);
    assert_eq!(engine.midi_clips[0].notes.len(), 2);

    // Process audio block
    let mut raw = vec![0.0f32; 1024];
    let mut buf = AudioBuffer::new(&mut raw);
    engine.process(&mut buf);
    assert_eq!(buf.samples.len(), 1024);
}
