/*
 * Hikaru OpenStudio - Audio DAW
 * Copyright (C) 2026 Hikaru OpenStudio Developers
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 */

use std::path::PathBuf;
use std::sync::mpsc::channel;

use hikaru_gui::audio_proxy::AudioProxy;
use hikaru_gui::views::clip_context_menu::BITWIG_CLIP_PALETTE;
use hikaru_gui::views::matrix::{
    self, AudioEvent, ClipData, MatrixClip, MatrixSlot, SessionMatrixState, SlotState,
};
use hikaru_gui::views::mixer::Track;
use hikaru_gui::views::playlist::PlaylistState;

fn create_test_state() -> (SessionMatrixState, AudioProxy) {
    let (tx, _rx) = channel();
    let proxy = AudioProxy::new(tx);
    let matrix_state = SessionMatrixState::default();
    (matrix_state, proxy)
}

#[test]
fn test_empty_slot_context_menu_creation_and_stop_button() {
    let (mut matrix, proxy) = create_test_state();

    // Ranura vacía inicial
    let slot = &matrix.grid[0][0];
    assert!(slot.clip.is_none());
    assert!(slot.has_stop_button);

    // Toggle stop button a false
    matrix.grid[0][0].has_stop_button = false;
    assert!(!matrix.grid[0][0].has_stop_button);

    // Toggle stop button de vuelta a true
    matrix.grid[0][0].has_stop_button = true;
    assert!(matrix.grid[0][0].has_stop_button);

    // Crear clip MIDI en ranura vacía
    matrix::create_empty_midi_clip(&mut matrix, &proxy, 0, 0);
    assert!(matrix.grid[0][0].clip.is_some());
    let clip = matrix.grid[0][0].clip.as_ref().unwrap();
    assert_eq!(clip.loop_enabled, true);
    assert_eq!(clip.muted, false);
}

#[test]
fn test_audio_clip_recolor_mute_and_normalization() {
    let (mut matrix, _proxy) = create_test_state();

    let ev = AudioEvent {
        id: 1,
        name: "Kick Event".into(),
        source_path: Some(PathBuf::from("kick.wav")),
        samples: vec![0.25, -0.5, 0.4, -0.3],
        channels: 1,
        sample_rate: 44100,
        start_secs: 0.0,
        trim_left_frames: 0,
        visible_frames: 4,
        gain: 1.0,
        fade_in_secs: 0.0,
        fade_out_secs: 0.0,
    };

    let audio_clip = MatrixClip {
        id: 100,
        name: "Audio 100".into(),
        path: PathBuf::from("kick.wav"),
        duration_secs: 1.0,
        content: ClipData::Audio {
            events: vec![ev],
            next_event_id: 2,
            preview_mix: vec![0.25, -0.5, 0.4, -0.3],
            preview_sr: 44100,
        },
        local_state: PlaylistState::default(),
        local_track: Track::new(0, "Audio Track".into(), false),
        local_bar: 1.0,
        loop_start: 0,
        loop_end: 44100,
        loop_enabled: true,
        has_time_selection: true,
        peaks: Vec::new(),
        muted: false,
        color: None,
    };

    matrix.grid[0][0] = MatrixSlot {
        state: SlotState::Stopped,
        has_stop_button: true,
        clip: Some(audio_clip),
    };

    // 1. Recolor con un color de la paleta Bitwig
    let bitwig_orange = BITWIG_CLIP_PALETTE[1][1]; // Naranja Bitwig
    matrix.grid[0][0].clip.as_mut().unwrap().color = Some(bitwig_orange);
    assert_eq!(matrix.grid[0][0].clip.as_ref().unwrap().color, Some(bitwig_orange));

    // 2. Mute toggle
    let clip = matrix.grid[0][0].clip.as_mut().unwrap();
    clip.muted = true;
    clip.local_track.mute = true;
    assert!(matrix.grid[0][0].clip.as_ref().unwrap().muted);

    // 3. Normalización: el pico es 0.5, por lo tanto el factor de normalización es 1.0 / 0.5 = 2.0
    let clip = matrix.grid[0][0].clip.as_mut().unwrap();
    if let ClipData::Audio { events, .. } = &mut clip.content {
        let max_peak = events[0].samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!((max_peak - 0.5).abs() < 1e-6);
        let factor = 1.0 / max_peak;
        events[0].gain *= factor;
        assert!((events[0].gain - 2.0).abs() < 1e-6);
    }
}

#[test]
fn test_midi_clip_transpose_double_and_legato() {
    let (mut matrix, _proxy) = create_test_state();

    let initial_notes = vec![
        (0u64, 60u8, 100u8, 240u32),    // C4 en tick 0, dur 240
        (480u64, 64u8, 100u8, 240u32),  // E4 en tick 480, dur 240
    ];

    let midi_clip = MatrixClip {
        id: 200,
        name: "MIDI Groove".into(),
        path: PathBuf::new(),
        duration_secs: 2.0,
        content: ClipData::Midi { notes: initial_notes },
        local_state: PlaylistState::default(),
        local_track: Track::new(0, "MIDI Track".into(), false),
        local_bar: 1.0,
        loop_start: 0,
        loop_end: 3840,
        loop_enabled: true,
        has_time_selection: true,
        peaks: Vec::new(),
        muted: false,
        color: None,
    };

    matrix.grid[0][0] = MatrixSlot {
        state: SlotState::Stopped,
        has_stop_button: true,
        clip: Some(midi_clip),
    };

    // 1. Transpose an Octave Up (+12 semitones)
    let clip = matrix.grid[0][0].clip.as_mut().unwrap();
    if let ClipData::Midi { notes } = &mut clip.content {
        for n in notes.iter_mut() {
            n.1 = (n.1 + 12).min(127);
        }
        assert_eq!(notes[0].1, 72); // C4 -> C5
        assert_eq!(notes[1].1, 76); // E4 -> E5
    }

    // 2. Transpose an Octave Down (-12 semitones)
    let clip = matrix.grid[0][0].clip.as_mut().unwrap();
    if let ClipData::Midi { notes } = &mut clip.content {
        for n in notes.iter_mut() {
            n.1 = n.1.saturating_sub(12);
        }
        assert_eq!(notes[0].1, 60); // C5 -> C4
        assert_eq!(notes[1].1, 64); // E5 -> E4
    }

    // 3. Make Legato: la primera nota empieza en 0 y debe extenderse hasta 480 (dur 480)
    let clip = matrix.grid[0][0].clip.as_mut().unwrap();
    if let ClipData::Midi { notes } = &mut clip.content {
        notes.sort_by_key(|n| n.0);
        let n_len = notes.len();
        for i in 0..n_len.saturating_sub(1) {
            let next_start = notes[i + 1].0;
            let cur_start = notes[i].0;
            if next_start > cur_start {
                notes[i].3 = (next_start - cur_start) as u32;
            }
        }
        assert_eq!(notes[0].3, 480); // Duración extendida sin gap
    }
}
