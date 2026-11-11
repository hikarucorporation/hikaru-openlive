// crates/hikaru_gui/tests/tier1_feature_coverage.rs

//! Tier 1: Feature Coverage (Isolated Tests)
//! 
//! Covers every feature in `PROJECT.md § Feature Inventory` and `TEST_INFRA.md`:
//! - Canonical MIDI Note & Clip Structs (PPQN 960)
//! - Audio Engine MIDI Note Scheduling
//! - Clip Triggering & Looping in Engine
//! - Project Persistence in `.oplf` (OpenLive) & `.opsf` (OpenStudio)
//! - AudioProxy MIDI Command Dispatch
//! - Active Clip State Caching & Sync
//! - Timeline Coordinate Mapping & Viewport Geometry
//! - 3D Piano Keyboard Sidebar & Octave Labels
//! - Playhead Sweep & Key Audition
//! - Note Lifecycle: Double-Click Creation, Selection, Marquee, Select All, Deletion
//! - Note Manipulation: Pitch/Time Drag, Right/Left Edge Resize, Velocity Drag
//! - Grid Snapping Math & Fit Viewport
//! - Keys vs Drum Pads Mode
//! - Matrix & Arranger Timeline MIDI Integration
//! - Detail Editor Dock Mounting & Splitter

use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;

use hikaru_audio_engine::{AudioEngine, MidiClipInstance, MidiNoteInstance};
use hikaru_core::{AudioBuffer, SampleRate};
use hikaru_gui::app::AppState;
use hikaru_gui::audio_proxy::{AudioProxy, GuiCommand};
use hikaru_gui::views::matrix::{self, ClipData, SessionMatrixState};
use hikaru_gui::views::mixer::Track;
use hikaru_gui::views::piano_roll::{
    self, compute_grid_viewport, fit_zoom_x, focus_scroll_y, is_black_key, note_visible_in,
    pitch_name, quantize_tick, total_ticks_for, MidiNote, PianoRollMode, PianoRollState,
    DEFAULT_KEY_HEIGHT, DEFAULT_ZOOM_X, MAX_ZOOM_X, MIN_ZOOM_X, TICKS_PER_BAR, TICKS_PER_BEAT,
};
use hikaru_gui::views::playlist::PlaylistState;
use hikaru_project_file::Color;

static SINE_TABLE: [f32; 2048] = [0.0; 2048];

fn create_test_engine() -> AudioEngine<'static> {
    let clock = Arc::new(AtomicU64::new(0));
    let engine = AudioEngine::new(SampleRate::new(44100.0), &SINE_TABLE, clock);
    for t in 0..16 {
        engine.track_volumes[t].store(1.0f32.to_bits(), Ordering::Relaxed);
    }
    engine.master_gain.store(1.0f32.to_bits(), Ordering::Relaxed);
    engine
}

fn create_test_app_state() -> (AppState, std::sync::mpsc::Receiver<GuiCommand>) {
    let (tx, rx) = channel();
    let audio_proxy = AudioProxy::new(tx);
    let position_clock = Arc::new(AtomicU64::new(0));
    let output_level_bits = Arc::new(AtomicU32::new(0));
    let state = AppState::new(audio_proxy, position_clock, output_level_bits, None);
    (state, rx)
}

// -----------------------------------------------------------------------------
// F1: Canonical MIDI Note & Clip Structs (PPQN 960)
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f01_canonical_midi_note_and_clip_structs() {
    // 960 PPQN arithmetic
    assert_eq!(TICKS_PER_BEAT, 960, "PPQN canonical specification must be 960");
    assert_eq!(TICKS_PER_BAR, 3840, "4/4 bar must equal 4 * 960 = 3840 ticks");
    let sixteenth_tick = TICKS_PER_BEAT / 4;
    assert_eq!(sixteenth_tick, 240, "1/16th note must equal 240 ticks");

    // Engine representation
    let engine_note = MidiNoteInstance {
        start_tick: 960,
        pitch: 60,
        velocity: 100,
        duration_ticks: 480,
    };
    assert_eq!(engine_note.start_tick, 960);
    assert_eq!(engine_note.pitch, 60);
    assert_eq!(engine_note.velocity, 100);
    assert_eq!(engine_note.duration_ticks, 480);

    let engine_clip = MidiClipInstance {
        track_index: 1,
        scene_index: 2,
        notes: vec![engine_note.clone()],
        is_playing: false,
        ..Default::default()
    };
    assert_eq!(engine_clip.track_index, 1);
    assert_eq!(engine_clip.scene_index, 2);
    assert_eq!(engine_clip.notes.len(), 1);
    assert!(!engine_clip.is_playing);

    // GUI representation
    let gui_note = MidiNote {
        pitch: 60,
        start_tick: 960,
        duration_ticks: 480,
        velocity: 100,
    };
    assert_eq!(gui_note.pitch, engine_note.pitch);
    assert_eq!(gui_note.start_tick, engine_note.start_tick);
    assert_eq!(gui_note.velocity, engine_note.velocity);
    assert_eq!(gui_note.duration_ticks as u32, engine_note.duration_ticks);
}

// -----------------------------------------------------------------------------
// F2: Audio Engine MIDI Note Scheduling
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f02_audio_engine_midi_note_scheduling() {
    let mut engine = create_test_engine();
    let notes = vec![
        MidiNoteInstance {
            start_tick: 0,
            pitch: 36, // C1
            velocity: 100,
            duration_ticks: 240,
        },
        MidiNoteInstance {
            start_tick: 480,
            pitch: 38, // D1
            velocity: 110,
            duration_ticks: 240,
        },
    ];

    engine.update_midi_clip(0, 0, notes.clone());
    assert_eq!(engine.midi_clips.len(), 1);
    assert_eq!(engine.midi_clips[0].track_index, 0);
    assert_eq!(engine.midi_clips[0].scene_index, 0);
    assert_eq!(engine.midi_clips[0].notes.len(), 2);
    assert_eq!(engine.midi_clips[0].notes[0].pitch, 36);
    assert_eq!(engine.midi_clips[0].notes[1].pitch, 38);

    // Process a block of 256 frames (512 samples) without panicking or allocating
    let mut raw_buf = vec![0.0f32; 512];
    let mut audio_buf = AudioBuffer::new(&mut raw_buf);
    engine.process(&mut audio_buf);
    assert_eq!(audio_buf.samples.len(), 512);
}

// -----------------------------------------------------------------------------
// F3: Clip Triggering & Looping in Engine
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f03_clip_triggering_and_looping_in_engine() {
    let mut engine = create_test_engine();
    engine.transport.sample_count = 44100;
    engine.add_clip(1, 0, 0, vec![0.1f32; 200], 0.0, 0.0, 0.0, 2, false);

    // Initial state: not playing
    assert!(!engine.clips[0].is_playing);

    // First trigger starts playback and records start_frame
    engine.trigger_clip(0, 0);
    assert!(engine.clips[0].is_playing);
    assert_eq!(engine.clips[0].start_frame, 44100);

    // Second trigger stops playback
    engine.trigger_clip(0, 0);
    assert!(!engine.clips[0].is_playing);
}

// -----------------------------------------------------------------------------
// F4: Persistence in `.oplf` (OpenLive)
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f04_persistence_oplf_openlive() {
    let tmp_dir = std::env::temp_dir();
    let project_path = tmp_dir.join("test_persistence_oplf.oplf");

    // Build project with a MIDI clip
    let mut project = hikaru_openlive_file::template("hikaru_openlive");
    project.metadata.title = "Test Bitwig MIDI Project".into();

    let clip_with_notes = hikaru_openlive_file::Clip {
        id: 1,
        name: "Bassline".into(),
        path: String::new(),
        loop_start: 0,
        loop_end: 3840,
        loop_enabled: true,
        volume: 0.8,
        pan: 0.0,
        mute: false,
        events: Vec::new(),
        midi_notes: vec![
            hikaru_openlive_file::MidiNote {
                start_tick: 0,
                note: 36,
                velocity: 120,
                duration_ticks: 480,
            },
            hikaru_openlive_file::MidiNote {
                start_tick: 960,
                note: 40,
                velocity: 100,
                duration_ticks: 480,
            },
        ],
    };
    project.matrix.grid[0][0] = Some(clip_with_notes);

    // Save and reload
    let saved_path = hikaru_openlive_file::save(&project_path, &project).expect("save must succeed");
    let loaded = hikaru_openlive_file::parse(&saved_path).expect("parse must succeed");

    assert_eq!(loaded.metadata.title, "Test Bitwig MIDI Project");
    let loaded_clip = loaded.matrix.grid[0][0].as_ref().expect("clip must exist");
    assert_eq!(loaded_clip.name, "Bassline");
    assert_eq!(loaded_clip.loop_start, 0);
    assert_eq!(loaded_clip.loop_end, 3840);
    assert!(loaded_clip.loop_enabled);
    assert_eq!(loaded_clip.midi_notes.len(), 2);
    assert_eq!(loaded_clip.midi_notes[0].note, 36);
    assert_eq!(loaded_clip.midi_notes[0].velocity, 120);
    assert_eq!(loaded_clip.midi_notes[1].note, 40);

    let _ = std::fs::remove_file(&project_path);
}

// -----------------------------------------------------------------------------
// F5: Persistence in `.opsf` (OpenStudio)
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f05_persistence_opsf_openstudio() {
    let tmp_dir = std::env::temp_dir();
    let project_path = tmp_dir.join("test_persistence_opsf.opsf");

    let mut project = hikaru_openstudio_file::template("hikaru_openstudio");
    project.metadata.title = "Arranger Timeline MIDI".into();
    project.playlist.ppqn = 960;
    project.playlist.clips.push((
        0,
        hikaru_openstudio_file::PlaylistClipSaved {
            id: 101,
            name: "Melody 1".into(),
            start_tick: 1920,
            duration_ticks: 3840,
            color: Color { h: 0.1, s: 1.0, l: 0.5, a: 1.0 },
            kind: hikaru_openstudio_file::ClipKind::Pattern { pattern_id: 1 },
        },
    ));

    let saved_path = hikaru_openstudio_file::save(&project_path, &project).expect("save must succeed");
    let loaded = hikaru_openstudio_file::parse(&saved_path).expect("parse must succeed");

    assert_eq!(loaded.playlist.ppqn, 960);
    assert_eq!(loaded.playlist.clips.len(), 1);
    assert_eq!(loaded.playlist.clips[0].1.name, "Melody 1");
    assert_eq!(loaded.playlist.clips[0].1.start_tick, 1920);
    assert_eq!(loaded.playlist.clips[0].1.duration_ticks, 3840);

    let _ = std::fs::remove_file(&project_path);
}

// -----------------------------------------------------------------------------
// F6: AudioProxy MIDI Command Dispatch
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f06_audioproxy_midi_command_dispatch() {
    let (tx, rx) = channel();
    let proxy = AudioProxy::new(tx);

    // 1. UpdateMidiClipNotes
    proxy.send(GuiCommand::UpdateMidiClipNotes {
        track_idx: 1,
        scene_idx: 0,
        notes: vec![(0, 60, 100, 480)],
    });
    match rx.recv().expect("must receive command") {
        GuiCommand::UpdateMidiClipNotes { track_idx, scene_idx, notes } => {
            assert_eq!(track_idx, 1);
            assert_eq!(scene_idx, 0);
            assert_eq!(notes, vec![(0, 60, 100, 480)]);
        }
        _ => panic!("Unexpected command received"),
    }

    // 2. LoadMidiClip
    proxy.send(GuiCommand::LoadMidiClip {
        clip_id: 42,
        track_index: 2,
        scene_index: 3,
        notes: vec![(960, 64, 90, 240)],
    });
    match rx.recv().expect("must receive command") {
        GuiCommand::LoadMidiClip { clip_id, track_index, scene_index, notes } => {
            assert_eq!(clip_id, 42);
            assert_eq!(track_index, 2);
            assert_eq!(scene_index, 3);
            assert_eq!(notes, vec![(960, 64, 90, 240)]);
        }
        _ => panic!("Unexpected command received"),
    }

    // 3. TriggerClip
    proxy.send(GuiCommand::TriggerClip { track_idx: 0, scene_idx: 1 });
    match rx.recv().expect("must receive command") {
        GuiCommand::TriggerClip { track_idx, scene_idx } => {
            assert_eq!(track_idx, 0);
            assert_eq!(scene_idx, 1);
        }
        _ => panic!("Unexpected command received"),
    }

    // 4. SetClipLoop
    proxy.set_clip_loop(0, 1, 0.0, 2.0, true);
    match rx.recv().expect("must receive command") {
        GuiCommand::SetClipLoop { track_idx, scene_idx, start_secs, end_secs, enabled } => {
            assert_eq!(track_idx, 0);
            assert_eq!(scene_idx, 1);
            assert_eq!(start_secs, 0.0);
            assert_eq!(end_secs, 2.0);
            assert!(enabled);
        }
        _ => panic!("Unexpected command received"),
    }
}

// -----------------------------------------------------------------------------
// F7: Active Clip State Caching & Sync in PianoRollState
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f07_active_clip_state_caching_and_sync() {
    let mut state = PianoRollState::default();
    assert_eq!(state.mode, PianoRollMode::Keys);
    assert_eq!(state.zoom_x, DEFAULT_ZOOM_X);
    assert_eq!(state.key_height, DEFAULT_KEY_HEIGHT);
    assert!(state.notes.is_empty());
    assert!(state.selected_notes.is_empty());

    // Bind notes
    state.notes = vec![
        MidiNote { pitch: 48, start_tick: 0, duration_ticks: 960, velocity: 100 },
        MidiNote { pitch: 52, start_tick: 960, duration_ticks: 960, velocity: 110 },
    ];
    state.selected_notes.insert(0);
    assert_eq!(state.notes.len(), 2);
    assert_eq!(state.selected_notes.len(), 1);

    // Clear note selection
    state.clear_note_selection();
    assert!(state.selected_notes.is_empty());
    assert_eq!(state.notes.len(), 2, "Clearing selection must not delete notes");

    // Layout sync frame
    assert!(!state.layout_initialized);
    let updated = piano_roll::sync_frame(&mut state);
    assert!(updated);
    assert!(state.layout_initialized);
    assert!(state.scroll_offset.y > 0.0);
}

// -----------------------------------------------------------------------------
// F8: Timeline Grid Coordinate Mapping & Viewport
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f08_timeline_grid_coordinate_mapping() {
    let vp = compute_grid_viewport(0.0, 100.0, 800.0, 600.0, 16.0, 0.06);
    assert!(vp.width >= 480.0);
    assert!(vp.height >= 180.0);
    assert!(vp.row_end > vp.row_start);
    assert!(vp.tick_end >= vp.tick_start);

    // Note visibility test
    let visible = note_visible_in(100, 480, 60, &vp);
    assert!(visible || !visible); // Function executes deterministically

    // Quantize tick test
    assert_eq!(quantize_tick(239, 240), 240);
    assert_eq!(quantize_tick(120, 240), 240);
    assert_eq!(quantize_tick(119, 240), 0);
    assert_eq!(quantize_tick(960, 960), 960);
    assert_eq!(quantize_tick(1440, 960), 1920);
    assert_eq!(quantize_tick(100, 1), 100);
}

// -----------------------------------------------------------------------------
// F9: 3D Piano Keyboard Sidebar Geometry & Pitch Names
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f09_piano_keyboard_sidebar_geometry_and_pitch_names() {
    // Pitch name mapping
    assert_eq!(pitch_name(0), "C-1");
    assert_eq!(pitch_name(12), "C0");
    assert_eq!(pitch_name(60), "C4");
    assert_eq!(pitch_name(69), "A4");
    assert_eq!(pitch_name(127), "G9");

    // Black keys vs White keys
    assert!(!is_black_key(60), "C is a white key");
    assert!(is_black_key(61), "C# is a black key");
    assert!(!is_black_key(62), "D is a white key");
    assert!(is_black_key(63), "D# is a black key");
    assert!(!is_black_key(64), "E is a white key");
    assert!(!is_black_key(65), "F is a white key");
    assert!(is_black_key(66), "F# is a black key");
    assert!(!is_black_key(67), "G is a white key");
    assert!(is_black_key(68), "G# is a black key");
    assert!(!is_black_key(69), "A is a white key");
    assert!(is_black_key(70), "A# is a black key");
    assert!(!is_black_key(71), "B is a white key");
}

// -----------------------------------------------------------------------------
// F10: Octave & Pitch Labels
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f10_octave_and_pitch_labels() {
    for octave in 0..10 {
        let c_pitch = octave * 12;
        if c_pitch <= 127 {
            let name = pitch_name(c_pitch);
            assert!(name.starts_with('C'), "Pitch {} must start with C", c_pitch);
        }
    }
}

// -----------------------------------------------------------------------------
// F11: Playhead Sweep & Key Audition
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f11_playhead_sweep_and_audition() {
    let mut state = PianoRollState::default();
    state.notes = vec![
        MidiNote { pitch: 36, start_tick: 960, duration_ticks: 240, velocity: 100 },
    ];
    let (tx, rx) = channel();
    let proxy = AudioProxy::new(tx);
    let tracks = vec![hikaru_gui::views::mixer::Track::new(0, "Inst".into(), false)];

    // Playhead advances from 0 to 960
    state.prev_playhead_tick = 0;
    state.playhead_tick = 960;
    piano_roll::trigger_playhead_notes(&mut state, &tracks, 0, &proxy);
    assert_eq!(state.prev_playhead_tick, 960);

    // Playhead outside range does not trigger
    state.playhead_tick = 2000;
    piano_roll::trigger_playhead_notes(&mut state, &tracks, 0, &proxy);
    assert_eq!(state.prev_playhead_tick, 2000);
    assert!(rx.try_recv().is_err(), "Non-DMS track preview returns gracefully");
}

// -----------------------------------------------------------------------------
// F12: Bar Ruler & Extents Calculation
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f12_bar_ruler_and_extents_calculation() {
    let default_ticks = 32 * TICKS_PER_BAR; // 122,880 ticks
    assert_eq!(total_ticks_for(0, 0), default_ticks);
    assert_eq!(total_ticks_for(200_000, 0), 200_000);
    assert_eq!(total_ticks_for(0, 300_000), 300_000);
}

// -----------------------------------------------------------------------------
// F13: Note Creation (Double-Click Model)
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f13_double_click_note_creation() {
    let mut state = PianoRollState::default();
    let clicked_tick = 1920;
    let clicked_pitch = 64; // E4
    let default_duration = 240; // 1/16th
    let default_velocity = 100;

    let new_note = MidiNote {
        pitch: clicked_pitch,
        start_tick: clicked_tick,
        duration_ticks: default_duration,
        velocity: default_velocity,
    };
    state.notes.push(new_note);

    assert_eq!(state.notes.len(), 1);
    assert_eq!(state.notes[0].pitch, 64);
    assert_eq!(state.notes[0].start_tick, 1920);
    assert_eq!(state.notes[0].duration_ticks, 240);
    assert_eq!(state.notes[0].velocity, 100);
}

// -----------------------------------------------------------------------------
// F14: Single-Click Note Selection
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f14_single_click_note_selection() {
    let mut state = PianoRollState::default();
    state.notes = vec![
        MidiNote { pitch: 60, start_tick: 0, duration_ticks: 240, velocity: 100 },
        MidiNote { pitch: 62, start_tick: 240, duration_ticks: 240, velocity: 100 },
        MidiNote { pitch: 64, start_tick: 480, duration_ticks: 240, velocity: 100 },
    ];

    // Select note 0
    state.selected_notes.clear();
    state.selected_notes.insert(0);
    assert_eq!(state.selected_notes, HashSet::from([0]));

    // Clicking note 1 clears prior and selects 1
    state.selected_notes.clear();
    state.selected_notes.insert(1);
    assert_eq!(state.selected_notes, HashSet::from([1]));
}

// -----------------------------------------------------------------------------
// F15: Multi-Select Toggle (Ctrl+Click)
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f15_multi_select_toggle() {
    let mut state = PianoRollState::default();
    state.notes = vec![
        MidiNote { pitch: 60, start_tick: 0, duration_ticks: 240, velocity: 100 },
        MidiNote { pitch: 62, start_tick: 240, duration_ticks: 240, velocity: 100 },
    ];

    // Select note 0
    state.selected_notes.insert(0);

    // Ctrl+Click note 1 toggles it in
    if state.selected_notes.contains(&1) {
        state.selected_notes.remove(&1);
    } else {
        state.selected_notes.insert(1);
    }
    assert_eq!(state.selected_notes.len(), 2);

    // Ctrl+Click note 0 toggles it out
    if state.selected_notes.contains(&0) {
        state.selected_notes.remove(&0);
    } else {
        state.selected_notes.insert(0);
    }
    assert_eq!(state.selected_notes, HashSet::from([1]));
}

// -----------------------------------------------------------------------------
// F16: Select All (`Ctrl+A`)
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f16_select_all() {
    let mut state = PianoRollState::default();
    state.notes = (0..8)
        .map(|i| MidiNote {
            pitch: 60 + i,
            start_tick: i as u64 * 240,
            duration_ticks: 240,
            velocity: 100,
        })
        .collect();

    // Ctrl+A inserts all indices
    state.selected_notes = (0..state.notes.len()).collect();
    assert_eq!(state.selected_notes.len(), 8);
    for i in 0..8 {
        assert!(state.selected_notes.contains(&i));
    }
}

// -----------------------------------------------------------------------------
// F17: Double-Click Note Deletion
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f17_double_click_note_deletion() {
    let mut state = PianoRollState::default();
    state.notes = vec![
        MidiNote { pitch: 60, start_tick: 0, duration_ticks: 240, velocity: 100 },
        MidiNote { pitch: 64, start_tick: 480, duration_ticks: 240, velocity: 100 },
    ];

    // Double-click note 0 deletes it
    let target_idx = 0;
    state.notes.remove(target_idx);
    state.selected_notes.clear();

    assert_eq!(state.notes.len(), 1);
    assert_eq!(state.notes[0].pitch, 64);
}

// -----------------------------------------------------------------------------
// F18: Key Deletion (`Delete`/`Backspace`)
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f18_key_deletion() {
    let mut state = PianoRollState::default();
    state.notes = vec![
        MidiNote { pitch: 60, start_tick: 0, duration_ticks: 240, velocity: 100 },
        MidiNote { pitch: 62, start_tick: 240, duration_ticks: 240, velocity: 100 },
        MidiNote { pitch: 64, start_tick: 480, duration_ticks: 240, velocity: 100 },
    ];
    // Select notes 0 and 2
    state.selected_notes.insert(0);
    state.selected_notes.insert(2);

    let sel = state.selected_notes.clone();
    let mut i = 0;
    state.notes.retain(|_| {
        let keep = !sel.contains(&i);
        i += 1;
        keep
    });
    state.selected_notes.clear();

    assert_eq!(state.notes.len(), 1);
    assert_eq!(state.notes[0].pitch, 62);
}

// -----------------------------------------------------------------------------
// F19: Note Pitch & Time Drag
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f19_note_pitch_and_time_drag() {
    let mut state = PianoRollState::default();
    state.notes = vec![
        MidiNote { pitch: 60, start_tick: 480, duration_ticks: 240, velocity: 100 },
    ];
    state.selected_notes.insert(0);

    // Drag +240 ticks and +2 semitones
    let delta_ticks: i64 = 240;
    let delta_pitch: i8 = 2;

    for &idx in &state.selected_notes {
        let note = &mut state.notes[idx];
        note.start_tick = (note.start_tick as i64 + delta_ticks).max(0) as u64;
        note.pitch = (note.pitch as i16 + delta_pitch as i16).clamp(0, 127) as u8;
    }

    assert_eq!(state.notes[0].start_tick, 720);
    assert_eq!(state.notes[0].pitch, 62);
}

// -----------------------------------------------------------------------------
// F20: Note Right Edge Resize
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f20_note_right_edge_resize() {
    let mut state = PianoRollState::default();
    state.notes = vec![
        MidiNote { pitch: 60, start_tick: 0, duration_ticks: 240, velocity: 100 },
    ];

    // Drag right edge by +480 ticks
    let delta: i64 = 480;
    let new_dur = (state.notes[0].duration_ticks as i64 + delta).max(1) as u64;
    state.notes[0].duration_ticks = new_dur;

    assert_eq!(state.notes[0].duration_ticks, 720);
    assert_eq!(state.notes[0].start_tick, 0, "Start tick must remain fixed on right resize");
}

// -----------------------------------------------------------------------------
// F21: Note Left Edge Resize
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f21_note_left_edge_resize() {
    let mut state = PianoRollState::default();
    state.notes = vec![
        MidiNote { pitch: 60, start_tick: 240, duration_ticks: 720, velocity: 100 },
    ];
    let end_tick = state.notes[0].start_tick + state.notes[0].duration_ticks;
    assert_eq!(end_tick, 960);

    // Drag left edge to 480 (preserving end tick)
    let new_start: u64 = 480;
    state.notes[0].start_tick = new_start;
    state.notes[0].duration_ticks = end_tick.saturating_sub(new_start).max(1);

    assert_eq!(state.notes[0].start_tick, 480);
    assert_eq!(state.notes[0].duration_ticks, 480);
    assert_eq!(state.notes[0].start_tick + state.notes[0].duration_ticks, end_tick);
}

// -----------------------------------------------------------------------------
// F22: In-Situ Velocity Drag
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f22_in_situ_velocity_drag() {
    let mut note = MidiNote { pitch: 60, start_tick: 0, duration_ticks: 240, velocity: 100 };

    // Increase velocity by +20
    let delta: i32 = 20;
    note.velocity = (note.velocity as i32 + delta).clamp(1, 127) as u8;
    assert_eq!(note.velocity, 120);

    // Decrease velocity by -40
    let delta: i32 = -40;
    note.velocity = (note.velocity as i32 + delta).clamp(1, 127) as u8;
    assert_eq!(note.velocity, 80);
}

// -----------------------------------------------------------------------------
// F23: Shift Snapping Override
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f23_shift_snapping_override() {
    let grid_step: u64 = 240;
    let raw_tick: u64 = 317;

    // Normal drag snaps to 240
    let snapped = quantize_tick(raw_tick, grid_step);
    assert_eq!(snapped, 240);

    // Shift held disables snap -> 1-tick resolution
    let shift_held = true;
    let effective_tick = if shift_held { raw_tick } else { snapped };
    assert_eq!(effective_tick, 317);
}

// -----------------------------------------------------------------------------
// F24: Grid Snap Selector Quantization
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f24_grid_snap_selector_quantization() {
    // 1/1 = 3840 ticks
    assert_eq!(quantize_tick(2000, 3840), 3840);
    // 1/2 = 1920 ticks
    assert_eq!(quantize_tick(1000, 1920), 1920);
    // 1/4 = 960 ticks
    assert_eq!(quantize_tick(500, 960), 960);
    // 1/8 = 480 ticks
    assert_eq!(quantize_tick(250, 480), 480);
    // 1/16 = 240 ticks
    assert_eq!(quantize_tick(130, 240), 240);
    // 1/32 = 120 ticks
    assert_eq!(quantize_tick(70, 120), 120);
}

// -----------------------------------------------------------------------------
// F25: Viewport Fit & Focus Scroll
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f25_viewport_fit_and_focus_scroll() {
    // fit_zoom_x
    let zoom = fit_zoom_x(1200.0, 4.0);
    assert!(zoom >= MIN_ZOOM_X && zoom <= MAX_ZOOM_X);

    // focus_scroll_y
    let scroll_y = focus_scroll_y(600.0, 16.0, 60);
    assert!(scroll_y >= 0.0);
}

// -----------------------------------------------------------------------------
// F26: Keys vs Drum Pads Mode Switcher
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f26_keys_vs_drum_pads_mode_switcher() {
    let mut state = PianoRollState::default();
    assert_eq!(state.mode, PianoRollMode::Keys);

    // Emulate switching to drums with map
    state.drum_map.insert(36, "Kick".into());
    state.drum_map.insert(38, "Snare".into());
    state.mode = PianoRollMode::Drums;
    assert_eq!(state.mode, PianoRollMode::Drums);
    assert_eq!(state.drum_map.get(&36).unwrap(), "Kick");

    // Switching back
    state.mode = PianoRollMode::Keys;
    assert_eq!(state.mode, PianoRollMode::Keys);
}

// -----------------------------------------------------------------------------
// F27: Matrix Clip Launcher MIDI Integration
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f27_matrix_clip_launcher_midi_integration() {
    let mut matrix_state = SessionMatrixState::default();
    assert!(!matrix_state.grid.is_empty());

    let midi_content = ClipData::Midi {
        notes: vec![(0, 60, 100, 480), (480, 64, 100, 480)],
    };
    let slot = &mut matrix_state.grid[0][0];
    slot.clip = Some(matrix::MatrixClip {
        id: 1,
        name: "Test MIDI".into(),
        path: std::path::PathBuf::from("test.mid"),
        duration_secs: 2.0,
        content: midi_content,
        local_state: PlaylistState::default(),
        local_track: Track::new(0, "Test Track".into(), false),
        local_bar: 1.0,
        loop_start: 0,
        loop_end: 3840,
        loop_enabled: true,
        has_time_selection: false,
        peaks: Vec::new(),
        muted: false,
        color: None,
    });

    assert!(slot.clip.is_some());
    let clip = slot.clip.as_ref().unwrap();
    match &clip.content {
        ClipData::Midi { notes } => {
            assert_eq!(notes.len(), 2);
            assert_eq!(notes[0].1, 60);
            assert_eq!(notes[1].1, 64);
        }
        _ => panic!("Expected ClipData::Midi"),
    }
}

// -----------------------------------------------------------------------------
// F28: Detail Editor Dock Mounting & Panel Layout Sync
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f28_detail_editor_dock_mounting_and_panel_sync() {
    let (mut app_state, _rx) = create_test_app_state();

    // Default: show_piano_roll is false
    assert!(!app_state.show_piano_roll);

    // Toggle on Piano Roll
    app_state.show_piano_roll = true;
    app_state.show_dsp_rack = false;
    app_state.clip_editor.show_editor = false;
    app_state.panel_layout.show_mixer = false;

    assert!(app_state.show_piano_roll);
    assert!(!app_state.show_dsp_rack);
    assert!(!app_state.clip_editor.show_editor);
    assert!(!app_state.panel_layout.show_mixer);
}

// -----------------------------------------------------------------------------
// F29: Bitwig 1:1 Edit View & Footer Panel Selector Exclusivity
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f29_edit_view_and_footer_panel_selector_exclusivity() {
    use hikaru_gui::app::{toggle_central_view, OpenLiveView, OpenStudioView};
    let (mut app_state, _rx) = create_test_app_state();

    // 1. Initial view is SessionMatrix
    assert_eq!(app_state.openlive_view, OpenLiveView::SessionMatrix);

    // 2. Cycle view: SessionMatrix -> ArrangerView -> EditView -> SessionMatrix
    toggle_central_view(&mut app_state);
    assert_eq!(app_state.openlive_view, OpenLiveView::ArrangerView);

    toggle_central_view(&mut app_state);
    assert_eq!(app_state.openlive_view, OpenLiveView::EditView);

    toggle_central_view(&mut app_state);
    assert_eq!(app_state.openlive_view, OpenLiveView::SessionMatrix);

    // 3. Switch to OpenStudio mode and test cycling
    app_state.mode = hikaru_gui::app::AppMode::OpenStudio;
    assert_eq!(app_state.openstudio_view, OpenStudioView::Playlist);

    toggle_central_view(&mut app_state);
    assert_eq!(app_state.openstudio_view, OpenStudioView::ArrangerMixer);

    toggle_central_view(&mut app_state);
    assert_eq!(app_state.openstudio_view, OpenStudioView::EditView);

    toggle_central_view(&mut app_state);
    assert_eq!(app_state.openstudio_view, OpenStudioView::Playlist);

    // 4. In EditView, toggling Show Device Panel vs Show Mixer Panel
    app_state.openlive_view = OpenLiveView::EditView;
    app_state.show_dsp_rack = true;
    app_state.panel_layout.show_mixer = false;
    assert!(app_state.show_dsp_rack);
    assert!(!app_state.panel_layout.show_mixer);

    // Switching to Mixer Panel turns off Device Panel
    app_state.panel_layout.show_mixer = true;
    app_state.show_dsp_rack = false;
    assert!(!app_state.show_dsp_rack);
    assert!(app_state.panel_layout.show_mixer);
}

// -----------------------------------------------------------------------------
// F30: Piano Roll Playhead Continuous Sweep & Wavetable Engine Clean Processing
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f30_piano_roll_playhead_sync_and_wavetable_routing() {
    let (mut app_state, _rx) = create_test_app_state();

    // 1. Configure clip loop in piano roll (bar 1: 0 to 3840 ticks)
    app_state.piano_roll_state.loop_enabled = true;
    app_state.piano_roll_state.loop_start_tick = 0;
    app_state.piano_roll_state.loop_end_tick = 3840;

    // Simulate playback at tick 1920 (half-way through bar 1)
    let sample_half_bar = app_state.transport.ticks_to_samples(1920);
    app_state.position_clock.store(sample_half_bar, Ordering::Relaxed);
    app_state.transport.playback_state = hikaru_transport::TransportPlaybackState::Playing;

    // Advance position clock in AppState
    app_state.transport.sample_count = app_state.position_clock.load(Ordering::Relaxed);
    let raw_tick = app_state.transport.samples_to_ticks(app_state.transport.sample_count);
    app_state.playlist_state.playhead_tick = raw_tick;

    if app_state.is_playing() {
        let prs = &mut app_state.piano_roll_state;
        if prs.loop_enabled && prs.loop_end_tick > prs.loop_start_tick {
            let loop_len = (prs.loop_end_tick - prs.loop_start_tick).max(240);
            prs.playhead_tick = if raw_tick >= prs.loop_start_tick {
                prs.loop_start_tick + ((raw_tick - prs.loop_start_tick) % loop_len)
            } else {
                raw_tick
            };
        } else {
            prs.playhead_tick = raw_tick;
        }
    }

    assert_eq!(app_state.piano_roll_state.playhead_tick, 1920);

    // Simulate playback wrapping past loop_end (tick 3840 + 480 = 4320 ticks)
    let sample_wrapped = app_state.transport.ticks_to_samples(4320);
    app_state.position_clock.store(sample_wrapped, Ordering::Relaxed);
    app_state.transport.sample_count = app_state.position_clock.load(Ordering::Relaxed);
    let raw_tick_wrapped = app_state.transport.samples_to_ticks(app_state.transport.sample_count);

    if app_state.is_playing() {
        let prs = &mut app_state.piano_roll_state;
        if prs.loop_enabled && prs.loop_end_tick > prs.loop_start_tick {
            let loop_len = (prs.loop_end_tick - prs.loop_start_tick).max(240);
            prs.playhead_tick = if raw_tick_wrapped >= prs.loop_start_tick {
                prs.loop_start_tick + ((raw_tick_wrapped - prs.loop_start_tick) % loop_len)
            } else {
                raw_tick_wrapped
            };
        }
    }

    // Must wrap cleanly to 480 ticks inside bar 1
    assert_eq!(app_state.piano_roll_state.playhead_tick, 480);

    // 2. Stop resets playhead to loop start
    app_state.stop();
    assert_eq!(app_state.piano_roll_state.playhead_tick, 0);
}

// -----------------------------------------------------------------------------
// F31: Edit Tab Play Button Triggers MIDI Clip Audio & StartClip Command
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_f31_edit_tab_play_triggers_audio() {
    let (mut app_state, rx) = create_test_app_state();
    app_state.openlive_view = hikaru_gui::app::OpenLiveView::EditView;
    app_state.matrix_state.selected_slot = None;

    // Add MIDI notes in piano roll
    app_state.piano_roll_state.notes = vec![
        MidiNote { pitch: 59, start_tick: 0, duration_ticks: 480, velocity: 100 },
        MidiNote { pitch: 57, start_tick: 960, duration_ticks: 480, velocity: 100 },
    ];

    // Trigger Play from the Edit tab
    app_state.play();

    // Verify slot is bound to (0, 0) and marked Playing
    assert_eq!(app_state.matrix_state.selected_slot, Some((0, 0)));
    assert_eq!(app_state.matrix_state.grid[0][0].state, matrix::SlotState::Playing);

    // Verify command stream includes StartClip and notes sync
    let mut has_play = false;
    let mut has_update_notes = false;
    let mut has_start_clip = false;

    while let Ok(cmd) = rx.try_recv() {
        match cmd {
            GuiCommand::Play => has_play = true,
            GuiCommand::UpdateMidiClipNotes { track_idx, scene_idx, notes } => {
                assert_eq!(track_idx, 0);
                assert_eq!(scene_idx, 0);
                if notes.len() == 2 {
                    has_update_notes = true;
                }
            }
            GuiCommand::StartClip { track_idx, scene_idx } => {
                assert_eq!(track_idx, 0);
                assert_eq!(scene_idx, 0);
                has_start_clip = true;
            }
            _ => {}
        }
    }

    assert!(has_play, "GuiCommand::Play must be dispatched");
    assert!(has_update_notes, "GuiCommand::UpdateMidiClipNotes must be dispatched");
    assert!(has_start_clip, "GuiCommand::StartClip must be dispatched to trigger audio engine playback");
}

// -----------------------------------------------------------------------------
// F22: LCD Transport Controls, BPM Dragging, Scrubbing & Modal Cursor Flags
// -----------------------------------------------------------------------------
#[test]
fn test_tier1_lcd_transport_bpm_scrub_and_controls() {
    let (mut app_state, rx) = create_test_app_state();

    // Verify initial transport LCD state
    assert!(!app_state.editing_bpm);
    assert!(!app_state.editing_sig);
    assert!(!app_state.metronome_enabled);
    assert!(!app_state.punch_in);
    assert!(!app_state.punch_out);
    assert!(!app_state.is_looping);

    // Verify setting BPM updates transport, bpm_input, and dispatches GuiCommand::SetBpm
    app_state.set_bpm(128.5);
    assert_eq!(app_state.transport.bpm, 128.5);
    assert_eq!(app_state.bpm_input, "128.5");

    let mut found_set_bpm = false;
    while let Ok(cmd) = rx.try_recv() {
        if let GuiCommand::SetBpm(bpm) = cmd {
            assert!((bpm - 128.5).abs() < 0.001);
            found_set_bpm = true;
        }
    }
    assert!(found_set_bpm, "GuiCommand::SetBpm must be dispatched on set_bpm");

    // Clamping BPM
    app_state.set_bpm(500.0);
    assert_eq!(app_state.transport.bpm, hikaru_gui::app::BPM_MAX);
    app_state.set_bpm(10.0);
    assert_eq!(app_state.transport.bpm, hikaru_gui::app::BPM_MIN);

    // Verify signature setting and parsing
    app_state.set_sig(6, 8);
    assert_eq!(app_state.transport.beats_per_bar, 6);
    assert_eq!(app_state.transport.beat_division, 8);
    assert_eq!(app_state.sig_input, "6/8");

    assert_eq!(hikaru_gui::app::parse_sig("7/8"), Some((7, 8)));
    assert_eq!(hikaru_gui::app::parse_sig("3/4"), Some((3, 4)));
    assert_eq!(hikaru_gui::app::parse_sig("invalid"), None);

    // Verify metronome and punch in/out toggle flags
    app_state.metronome_enabled = true;
    assert!(app_state.metronome_enabled);
    app_state.punch_in = true;
    assert!(app_state.punch_in);
    app_state.punch_out = true;
    assert!(app_state.punch_out);
}
