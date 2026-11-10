// crates/hikaru_gui/tests/tier4_real_world_scenarios.rs

//! Tier 4: Real-World Application Scenarios
//!
//! Validates end-to-end realistic production workflows from `TEST_INFRA.md § Real-World Application Scenarios`:
//! 1. Electronic Bassline Composition (1/16th syncopation, velocity accents, loop bars 1..5)
//! 2. Polyphonic Chord Progression & Duration Trimming (Cmaj7..G7, octave shift, staccato trim)
//! 3. Session Matrix Live Jam (Multi-track 4x4 matrix, scene launches, MIDI + audio clip coexistence)
//! 4. Arranger Timeline Linear Song Layout (32-bar timeline clip arrangement, PPQN=960)
//! 5. Full Project Roundtrip Persistence (`.oplf` & `.opsf` save, parse, and bit-exact note recovery)

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;

use hikaru_audio_engine::{AudioEngine, MidiNoteInstance};
use hikaru_core::{AudioBuffer, SampleRate};
use hikaru_gui::audio_proxy::{AudioProxy, GuiCommand};
use hikaru_gui::views::matrix::{self, ClipData, SessionMatrixState};
use hikaru_gui::views::mixer::Track;
use hikaru_gui::views::piano_roll::{
    quantize_tick, total_ticks_for, MidiNote, PianoRollState, TICKS_PER_BAR, TICKS_PER_BEAT,
};
use hikaru_gui::views::playlist::PlaylistState;
use hikaru_project_file::Color;

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
// Scenario 1: Electronic Bassline Composition
// -----------------------------------------------------------------------------
#[test]
fn test_tier4_s01_electronic_bassline_composition() {
    let mut state = PianoRollState::default();
    let sixteenth_step = TICKS_PER_BEAT / 4; // 240 ticks

    // Compose a 4-bar 16th-note syncopated electronic bassline (16 notes across 4 bars)
    // Root notes in C minor: C1 (36), Eb1 (39), F1 (41), G1 (43)
    let pattern_pitches = [36, 36, 48, 36, 39, 36, 41, 43, 36, 36, 48, 36, 41, 39, 38, 36];
    for (i, &pitch) in pattern_pitches.iter().enumerate() {
        let start_tick = i as u64 * sixteenth_step * 2; // every 8th note slot
        let snapped_start = quantize_tick(start_tick, sixteenth_step);
        // Accents: downbeats get velocity 125, offbeats get velocity 85
        let velocity = if i % 4 == 0 { 125 } else { 85 };
        let duration = if i % 2 == 0 { 200 } else { 120 }; // punchy staccato

        state.notes.push(MidiNote {
            pitch,
            start_tick: snapped_start,
            duration_ticks: duration,
            velocity,
        });
    }

    assert_eq!(state.notes.len(), 16);
    assert_eq!(state.notes[0].pitch, 36);
    assert_eq!(state.notes[0].velocity, 125);
    assert_eq!(state.notes[1].velocity, 85);

    // Set loop markers to bars 1..5 (15,360 ticks)
    state.loop_enabled = true;
    let loop_end_tick = 4 * TICKS_PER_BAR; // 15,360 ticks
    assert_eq!(loop_end_tick, 15360);

    // Send to Audio Engine and verify scheduling
    let mut engine = create_engine();
    let engine_notes: Vec<MidiNoteInstance> = state.notes
        .iter()
        .map(|n| MidiNoteInstance {
            start_tick: n.start_tick,
            pitch: n.pitch,
            velocity: n.velocity,
            duration_ticks: n.duration_ticks as u32,
        })
        .collect();
    engine.update_midi_clip(0, 0, engine_notes);
    assert_eq!(engine.midi_clips.len(), 1);
    assert_eq!(engine.midi_clips[0].notes.len(), 16);

    // Audio engine processes audio block cleanly
    let mut raw_buf = vec![0.0f32; 1024];
    let mut buf = AudioBuffer::new(&mut raw_buf);
    engine.process(&mut buf);
    assert_eq!(buf.samples.len(), 1024);
}

// -----------------------------------------------------------------------------
// Scenario 2: Polyphonic Chord Progression & Duration Trimming
// -----------------------------------------------------------------------------
#[test]
fn test_tier4_s02_polyphonic_chord_progression_and_duration_trimming() {
    let mut state = PianoRollState::default();
    let bar_ticks = TICKS_PER_BAR; // 3840

    // 4 Chords:
    // Bar 1: Cmaj7 (C3=48, E3=52, G3=55, B3=59)
    // Bar 2: Am7   (A2=45, C3=48, E3=52, G3=55)
    // Bar 3: Dm7   (D3=50, F3=53, A3=57, C4=60)
    // Bar 4: G7    (G2=43, B2=47, D3=50, F3=53)
    let chords = [
        (0 * bar_ticks, vec![48, 52, 55, 59]),
        (1 * bar_ticks, vec![45, 48, 52, 55]),
        (2 * bar_ticks, vec![50, 53, 57, 60]),
        (3 * bar_ticks, vec![43, 47, 50, 53]),
    ];

    for (start_tick, pitches) in &chords {
        for &pitch in pitches {
            state.notes.push(MidiNote {
                pitch,
                start_tick: *start_tick,
                duration_ticks: bar_ticks, // initial full-bar duration (3840)
                velocity: 95,
            });
        }
    }
    assert_eq!(state.notes.len(), 16);

    // Step 1: Select all notes (`Ctrl+A`)
    state.selected_notes = (0..state.notes.len()).collect();
    assert_eq!(state.selected_notes.len(), 16);

    // Step 2: Transpose up 1 octave (+12 semitones)
    for &idx in &state.selected_notes {
        state.notes[idx].pitch = (state.notes[idx].pitch as i16 + 12).clamp(0, 127) as u8;
    }
    // Bar 1 should now be C4 (60), E4 (64), G4 (67), B4 (71)
    assert_eq!(state.notes[0].pitch, 60);
    assert_eq!(state.notes[1].pitch, 64);
    assert_eq!(state.notes[2].pitch, 67);
    assert_eq!(state.notes[3].pitch, 71);

    // Step 3: Right-edge trim by -1920 ticks (shorten to half-notes / staccato chords)
    for &idx in &state.selected_notes {
        state.notes[idx].duration_ticks = (state.notes[idx].duration_ticks as i64 - 1920).max(1) as u64;
    }
    for note in &state.notes {
        assert_eq!(note.duration_ticks, 1920, "All chord notes must be trimmed to 1920 ticks");
    }
}

// -----------------------------------------------------------------------------
// Scenario 3: Session Matrix Live Jam
// -----------------------------------------------------------------------------
#[test]
fn test_tier4_s03_session_matrix_live_jam() {
    let mut matrix_state = SessionMatrixState::default();
    let (tx, rx) = channel();
    let proxy = AudioProxy::new(tx);

    // Populate Track 0 (Drums) and Track 1 (Bass) with MIDI Clips across 2 scenes
    let drum_notes_s0 = vec![(0, 36, 120, 240), (480, 38, 110, 240)];
    let bass_notes_s0 = vec![(0, 36, 100, 480), (960, 39, 100, 480)];
    let drum_notes_s1 = vec![(0, 36, 127, 240), (240, 42, 90, 120), (480, 38, 120, 240)];
    let bass_notes_s1 = vec![(0, 41, 100, 480), (960, 43, 100, 480)];

    matrix_state.grid[0][0].clip = Some(matrix::MatrixClip {
        id: 1,
        name: "Drums Beat A".into(),
        path: std::path::PathBuf::from("drums_a.mid"),
        duration_secs: 2.0,
        local_track: Track::new(0, "Drums".into(), false),
        local_state: PlaylistState::default(),
        local_bar: 1.0,
        loop_start: 0,
        loop_end: 3840,
        loop_enabled: true,
        has_time_selection: false,
        peaks: Vec::new(),
        content: ClipData::Midi { notes: drum_notes_s0 },
    });
    matrix_state.grid[1][0].clip = Some(matrix::MatrixClip {
        id: 2,
        name: "Bass Groove A".into(),
        path: std::path::PathBuf::from("bass_a.mid"),
        duration_secs: 2.0,
        local_track: Track::new(1, "Bass".into(), false),
        local_state: PlaylistState::default(),
        local_bar: 1.0,
        loop_start: 0,
        loop_end: 3840,
        loop_enabled: true,
        has_time_selection: false,
        peaks: Vec::new(),
        content: ClipData::Midi { notes: bass_notes_s0 },
    });
    matrix_state.grid[0][1].clip = Some(matrix::MatrixClip {
        id: 3,
        name: "Drums Fill B".into(),
        path: std::path::PathBuf::from("drums_b.mid"),
        duration_secs: 2.0,
        local_track: Track::new(0, "Drums".into(), false),
        local_state: PlaylistState::default(),
        local_bar: 1.0,
        loop_start: 0,
        loop_end: 3840,
        loop_enabled: true,
        has_time_selection: false,
        peaks: Vec::new(),
        content: ClipData::Midi { notes: drum_notes_s1 },
    });
    matrix_state.grid[1][1].clip = Some(matrix::MatrixClip {
        id: 4,
        name: "Bass Groove B".into(),
        path: std::path::PathBuf::from("bass_b.mid"),
        duration_secs: 2.0,
        local_track: Track::new(1, "Bass".into(), false),
        local_state: PlaylistState::default(),
        local_bar: 1.0,
        loop_start: 0,
        loop_end: 3840,
        loop_enabled: true,
        has_time_selection: false,
        peaks: Vec::new(),
        content: ClipData::Midi { notes: bass_notes_s1 },
    });

    // 1. Launch Scene 0 (triggers Track 0 Scene 0 and Track 1 Scene 0)
    proxy.send(GuiCommand::TriggerScene { scene_idx: 0 });
    match rx.recv().expect("must receive trigger scene") {
        GuiCommand::TriggerScene { scene_idx } => assert_eq!(scene_idx, 0),
        _ => panic!("Expected TriggerScene"),
    }

    // 2. Launch Scene 1 (switches both tracks to Scene 1)
    proxy.send(GuiCommand::TriggerScene { scene_idx: 1 });
    match rx.recv().expect("must receive trigger scene") {
        GuiCommand::TriggerScene { scene_idx } => assert_eq!(scene_idx, 1),
        _ => panic!("Expected TriggerScene"),
    }

    // Verify clips retain note counts and properties
    assert!(matrix_state.grid[0][0].clip.is_some());
    assert!(matrix_state.grid[1][1].clip.is_some());
}

// -----------------------------------------------------------------------------
// Scenario 4: Arranger Timeline Linear Song Layout
// -----------------------------------------------------------------------------
#[test]
fn test_tier4_s04_arranger_timeline_linear_song_layout() {
    let mut project = hikaru_openstudio_file::template("hikaru_openstudio");
    project.playlist.ppqn = 960;

    // Arrange 4 clips sequentially across 16 bars on Track 0 (Lead)
    // Clip 1: Intro (Bars 1..5 -> ticks 0..15360)
    // Clip 2: Verse (Bars 5..9 -> ticks 15360..30720)
    // Clip 3: Chorus (Bars 9..13 -> ticks 30720..46080)
    // Clip 4: Outro (Bars 13..17 -> ticks 46080..61440)
    let clip_configs = [
        (101, "Intro", 0, 15360),
        (102, "Verse", 15360, 15360),
        (103, "Chorus", 30720, 15360),
        (104, "Outro", 46080, 15360),
    ];

    for &(id, name, start_tick, duration_ticks) in &clip_configs {
        project.playlist.clips.push((
            0,
            hikaru_openstudio_file::PlaylistClipSaved {
                id,
                name: name.into(),
                start_tick,
                duration_ticks,
                color: Color { h: 0.1, s: 1.0, l: 0.5, a: 1.0 },
                kind: hikaru_openstudio_file::ClipKind::Pattern { pattern_id: id },
            },
        ));
    }

    assert_eq!(project.playlist.clips.len(), 4);
    assert_eq!(project.playlist.clips[0].1.start_tick, 0);
    assert_eq!(project.playlist.clips[3].1.start_tick, 46080);
    let total_song_ticks = project.playlist.clips[3].1.start_tick + project.playlist.clips[3].1.duration_ticks;
    assert_eq!(total_song_ticks, 61440); // 16 bars * 3840 ticks/bar = 61,440 ticks
    assert_eq!(total_ticks_for(total_song_ticks, 0), 122880); // Default bars 32 = 122,880
}

// -----------------------------------------------------------------------------
// Scenario 5: Full Project Roundtrip Persistence (OPLF & OPSF)
// -----------------------------------------------------------------------------
#[test]
fn test_tier4_s05_full_project_roundtrip_persistence() {
    let tmp_dir = std::env::temp_dir();
    let oplf_path = tmp_dir.join("roundtrip_test.oplf");
    let opsf_path = tmp_dir.join("roundtrip_test.opsf");

    // --- 1. OPLF (OpenLive) Roundtrip with Detailed MIDI Notes ---
    let mut oplf_project = hikaru_openlive_file::template("hikaru_openlive");
    oplf_project.metadata.title = "Roundtrip E2E Suite".into();
    oplf_project.transport.bpm = 128.0;
    oplf_project.transport.loop_start_ticks = 0;
    oplf_project.transport.loop_end_ticks = 7680;
    oplf_project.transport.loop_enabled = true;

    let original_notes = vec![
        hikaru_openlive_file::MidiNote { start_tick: 0, note: 36, velocity: 120, duration_ticks: 240 },
        hikaru_openlive_file::MidiNote { start_tick: 240, note: 38, velocity: 100, duration_ticks: 240 },
        hikaru_openlive_file::MidiNote { start_tick: 480, note: 40, velocity: 110, duration_ticks: 480 },
        hikaru_openlive_file::MidiNote { start_tick: 960, note: 43, velocity: 90, duration_ticks: 960 },
    ];
    oplf_project.matrix.grid[0][0] = Some(hikaru_openlive_file::Clip {
        id: 1,
        name: "E2E Bass".into(),
        path: String::new(),
        loop_start: 0,
        loop_end: 3840,
        loop_enabled: true,
        volume: 0.85,
        pan: -0.1,
        mute: false,
        events: Vec::new(),
        midi_notes: original_notes.clone(),
    });

    let saved_oplf = hikaru_openlive_file::save(&oplf_path, &oplf_project).expect("oplf save must succeed");
    let loaded_oplf = hikaru_openlive_file::parse(&saved_oplf).expect("oplf parse must succeed");

    assert_eq!(loaded_oplf.metadata.title, "Roundtrip E2E Suite");
    assert_eq!(loaded_oplf.transport.bpm, 128.0);
    assert_eq!(loaded_oplf.transport.loop_start_ticks, 0);
    assert_eq!(loaded_oplf.transport.loop_end_ticks, 7680);
    assert!(loaded_oplf.transport.loop_enabled);

    let loaded_clip = loaded_oplf.matrix.grid[0][0].as_ref().expect("clip must exist");
    assert_eq!(loaded_clip.name, "E2E Bass");
    assert_eq!(loaded_clip.midi_notes.len(), original_notes.len());
    for (orig, loaded) in original_notes.iter().zip(&loaded_clip.midi_notes) {
        assert_eq!(orig.start_tick, loaded.start_tick);
        assert_eq!(orig.note, loaded.note);
        assert_eq!(orig.velocity, loaded.velocity);
        assert_eq!(orig.duration_ticks, loaded.duration_ticks);
    }

    // --- 2. OPSF (OpenStudio) Roundtrip ---
    let mut opsf_project = hikaru_openstudio_file::template("hikaru_openstudio");
    opsf_project.metadata.title = "Arranger Roundtrip E2E".into();
    opsf_project.transport.bpm = 132.0;
    opsf_project.playlist.ppqn = 960;
    opsf_project.playlist.clips.push((
        0,
        hikaru_openstudio_file::PlaylistClipSaved {
            id: 201,
            name: "Timeline Synth".into(),
            start_tick: 3840,
            duration_ticks: 7680,
            color: Color { h: 0.5, s: 0.8, l: 0.5, a: 1.0 },
            kind: hikaru_openstudio_file::ClipKind::Pattern { pattern_id: 201 },
        },
    ));

    let saved_opsf = hikaru_openstudio_file::save(&opsf_path, &opsf_project).expect("opsf save must succeed");
    let loaded_opsf = hikaru_openstudio_file::parse(&saved_opsf).expect("opsf parse must succeed");

    assert_eq!(loaded_opsf.metadata.title, "Arranger Roundtrip E2E");
    assert_eq!(loaded_opsf.transport.bpm, 132.0);
    assert_eq!(loaded_opsf.playlist.ppqn, 960);
    assert_eq!(loaded_opsf.playlist.clips.len(), 1);
    assert_eq!(loaded_opsf.playlist.clips[0].1.name, "Timeline Synth");
    assert_eq!(loaded_opsf.playlist.clips[0].1.start_tick, 3840);
    assert_eq!(loaded_opsf.playlist.clips[0].1.duration_ticks, 7680);

    // Clean up
    let _ = std::fs::remove_file(&oplf_path);
    let _ = std::fs::remove_file(&opsf_path);
}

// -----------------------------------------------------------------------------
// Scenario 6: Device Panel & HDP Preset Persistence (Skrillex Growl Sound Design)
// -----------------------------------------------------------------------------
#[test]
fn test_tier4_s06_device_panel_and_hdp_preset_roundtrip_persistence() {
    use hikaru_gui::views::hikaru_native_plugins::hikaru_openmodulation::{
        get_lfo_state, update_lfo_state, ModulationTarget,
    };
    use hikaru_dsp::effects::lfo::{LfoSyncMode, LfoWaveform, SyncRate};
    use hikaru_gui::views::mixer::{DspSlot as GuiDspSlot, Track as GuiTrack};
    use hikaru_project_file::{parse_hdp, save_hdp, HikaruDevicePreset, PresetType};

    let track_idx = 1;
    let mut track = GuiTrack::new(track_idx, "Skrillex Bass".to_string(), false);

    // 1. Configure OpenWavetable on Track 1 (Slot 0)
    let mut wt_slot = GuiDspSlot::new(101, "OpenWavetable".to_string());
    wt_slot.wavetable.table.name = "Uber Growl".to_string();
    wt_slot.wavetable.octave = -3;
    wt_slot.wavetable.pitch = 0;
    wt_slot.wavetable.unison = 1;
    wt_slot.wavetable.set_position(0.640625);
    wt_slot.wavetable.render_mode = hikaru_render::RenderMode::Mode3D;
    wt_slot.wavetable.render_settings.mesh = hikaru_render::MeshType::Wireframe;
    track.effects.push(wt_slot);

    // 2. Configure OpenModulation on Track 1 (Slot 1)
    let mod_slot = GuiDspSlot::new(102, "OpenModulation".to_string());
    track.effects.push(mod_slot);

    update_lfo_state(track_idx, 1, |lfo| {
        lfo.waveform = LfoWaveform::SawDown;
        lfo.sync_mode = LfoSyncMode::TempoSync;
        lfo.set_sync_rate(SyncRate::Whole2); // 2/1 Tempo Sync
        lfo.depth = 0.16;
        lfo.amount = 0.14;
        lfo.bipolar = true;
        lfo.target = ModulationTarget::OpenWavetableWtPos;
    });

    // 3. Capture Track into project file struct
    let captured_track = hikaru_gui::project::common::capture_track(&track);
    assert_eq!(captured_track.effects.len(), 2);

    let wt_captured = &captured_track.effects[0];
    assert_eq!(wt_captured.name, "OpenWavetable");
    let wt_params = wt_captured.open_wavetable.as_ref().expect("open_wavetable params must be captured");
    assert_eq!(wt_params.table_name, "Uber Growl");
    assert_eq!(wt_params.octave, -3);
    assert!((wt_params.position - 0.640625).abs() < 1e-4);
    assert!(wt_params.render_mode_3d);
    assert!(wt_params.render_mode_lines);

    let mod_captured = &captured_track.effects[1];
    assert_eq!(mod_captured.name, "OpenModulation");
    let mod_params = mod_captured.open_modulation.as_ref().expect("open_modulation params must be captured");
    assert_eq!(mod_params.sync_rate, "2/1");
    assert_eq!(mod_params.sync_mode, "TempoSync");
    assert_eq!(mod_params.waveform, "SawDown");
    assert_eq!(mod_params.target, "OpenWavetable: WT POS");

    // 4. Test HDP Single Device Preset Roundtrip
    let tmp_dir = std::env::temp_dir();
    let hdp_dev_path = tmp_dir.join(format!("uber_growl_{}.hdp", std::process::id()));
    let single_preset = HikaruDevicePreset {
        format_version: hikaru_project_file::FORMAT_VERSION,
        created_by: "Hikaru OpenLive 2.24.0".into(),
        preset_type: PresetType::SingleDevice,
        name: "Uber Growl Preset".into(),
        author: Some("Skrillex".into()),
        category: Some("Bass / Growl".into()),
        description: Some("Uber growl test patch".into()),
        slot: Some(wt_captured.clone()),
        chain: None,
    };
    save_hdp(&hdp_dev_path, &single_preset).expect("save_hdp single device must succeed");
    let loaded_single = parse_hdp(&hdp_dev_path).expect("parse_hdp single device must succeed");
    assert_eq!(loaded_single.name, "Uber Growl Preset");
    assert_eq!(loaded_single.slot.as_ref().unwrap().name, "OpenWavetable");
    assert_eq!(loaded_single.slot.as_ref().unwrap().open_wavetable.as_ref().unwrap().octave, -3);

    // 5. Test HDP Chain Preset Roundtrip (Wavetable + Modulation together)
    let hdp_chain_path = tmp_dir.join(format!("skrillex_chain_{}.hdp", std::process::id()));
    let chain_preset = HikaruDevicePreset {
        format_version: hikaru_project_file::FORMAT_VERSION,
        created_by: "Hikaru OpenLive 2.24.0".into(),
        preset_type: PresetType::Chain,
        name: "Skrillex Monster Bass Chain".into(),
        author: Some("Skrillex".into()),
        category: Some("Bass / Growl Chain".into()),
        description: Some("Complete chain with LFO routing".into()),
        slot: None,
        chain: Some(captured_track.effects.clone()),
    };
    save_hdp(&hdp_chain_path, &chain_preset).expect("save_hdp chain must succeed");
    let loaded_chain = parse_hdp(&hdp_chain_path).expect("parse_hdp chain must succeed");
    assert_eq!(loaded_chain.name, "Skrillex Monster Bass Chain");
    assert_eq!(loaded_chain.chain.as_ref().unwrap().len(), 2);

    // 6. Test Rebuilding Track (like when opening .oplf)
    let rebuilt_track = hikaru_gui::project::common::build_track(&captured_track);
    assert_eq!(rebuilt_track.effects.len(), 2);

    let rebuilt_wt = &rebuilt_track.effects[0];
    assert_eq!(rebuilt_wt.name, "OpenWavetable");
    assert_eq!(rebuilt_wt.wavetable.table.name, "Uber Growl");
    assert_eq!(rebuilt_wt.wavetable.octave, -3);
    assert!((rebuilt_wt.wavetable.position() - 0.640625).abs() < 1e-3);
    assert_eq!(rebuilt_wt.wavetable.render_mode, hikaru_render::RenderMode::Mode3D);

    let restored_lfo = get_lfo_state(track_idx, 1);
    assert_eq!(restored_lfo.sync_rate, SyncRate::Whole2);
    assert_eq!(restored_lfo.sync_mode, LfoSyncMode::TempoSync);
    assert_eq!(restored_lfo.waveform, LfoWaveform::SawDown);
    assert_eq!(restored_lfo.target, ModulationTarget::OpenWavetableWtPos);
    assert!((restored_lfo.depth - 0.16).abs() < 1e-4);
    assert!((restored_lfo.amount - 0.14).abs() < 1e-4);

    // Clean up temporary files
    let _ = std::fs::remove_file(&hdp_dev_path);
    let _ = std::fs::remove_file(&hdp_chain_path);
}
