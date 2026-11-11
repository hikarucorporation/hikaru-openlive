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

use hikaru_gui::audio_proxy::{AudioProxy, GuiCommand};
use hikaru_gui::views::matrix::{
    self, AudioEvent, ClipData, MatrixClip, MatrixSlot, SessionMatrixState, SlotState,
};
use hikaru_gui::views::mixer::Track;
use hikaru_gui::views::playlist::PlaylistState;

fn create_test_state() -> (SessionMatrixState, AudioProxy, std::sync::mpsc::Receiver<GuiCommand>) {
    let (tx, rx) = channel();
    let proxy = AudioProxy::new(tx);
    let matrix_state = SessionMatrixState::default();
    (matrix_state, proxy, rx)
}

fn create_sample_audio_clip(id: usize, name: &str) -> MatrixClip {
    let ev = AudioEvent {
        id: id as u64,
        name: format!("{name} Event"),
        source_path: Some(PathBuf::from("test.wav")),
        samples: vec![0.1, -0.2, 0.3, -0.4],
        channels: 1,
        sample_rate: 44100,
        start_secs: 0.0,
        trim_left_frames: 0,
        visible_frames: 4,
        gain: 1.0,
        fade_in_secs: 0.0,
        fade_out_secs: 0.0,
    };

    MatrixClip {
        id,
        name: name.to_string(),
        path: PathBuf::from("test.wav"),
        duration_secs: 1.0,
        content: ClipData::Audio {
            events: vec![ev],
            next_event_id: (id + 1) as u64,
            preview_mix: vec![0.1, -0.2, 0.3, -0.4],
            preview_sr: 44100,
        },
        local_state: PlaylistState::default(),
        local_track: Track::new(0, "Track".into(), false),
        local_bar: 1.0,
        loop_start: 0,
        loop_end: 44100,
        loop_enabled: true,
        has_time_selection: true,
        peaks: Vec::new(),
        muted: false,
        color: None,
    }
}

#[test]
fn test_move_midi_clip_to_empty_slot() {
    let (mut matrix, proxy, rx) = create_test_state();

    // Crear clip MIDI en (0, 0)
    matrix::create_empty_midi_clip(&mut matrix, &proxy, 0, 0);
    assert!(matrix.grid[0][0].clip.is_some());
    assert!(matrix.grid[0][1].clip.is_none());
    matrix.selected_slot = Some((0, 0));

    // Mover a (0, 1)
    matrix.move_slot(0, 0, 0, 1, &proxy);

    // Origen vacío, destino con clip
    assert!(matrix.grid[0][0].clip.is_none());
    assert!(matrix.grid[0][1].clip.is_some());
    assert_eq!(matrix.selected_slot, Some((0, 1)));

    // Verificar comandos enviados al audio proxy
    let mut moved_clip = false;
    let mut updated_destination = false;
    while let Ok(cmd) = rx.try_recv() {
        match cmd {
            GuiCommand::MoveMatrixClip { src_track, src_scene, dst_track, dst_scene } => {
                if src_track == 0 && src_scene == 0 && dst_track == 0 && dst_scene == 1 {
                    moved_clip = true;
                }
            }
            GuiCommand::UpdateMidiClipNotes { track_idx, scene_idx, .. } => {
                if track_idx == 0 && scene_idx == 1 {
                    updated_destination = true;
                }
            }
            _ => {}
        }
    }
    assert!(moved_clip, "Audio proxy debió recibir MoveMatrixClip(0, 0 -> 0, 1)");
    assert!(updated_destination, "Audio proxy debió sincronizar notas en destino (0, 1)");
}

#[test]
fn test_move_audio_clip_across_tracks_and_scenes() {
    let (mut matrix, proxy, rx) = create_test_state();

    // Asegurar que tengamos al menos 2 pistas y 3 escenas
    while matrix.grid.len() < 2 {
        matrix.add_track();
    }
    while matrix.scenes.len() < 3 {
        matrix.add_scene();
    }

    let clip = create_sample_audio_clip(42, "Kick 42");
    matrix.grid[0][0] = MatrixSlot {
        state: SlotState::Stopped,
        has_stop_button: true,
        clip: Some(clip),
    };
    matrix.selected_slot = Some((0, 0));

    // Mover desde pista 0 escena 0 a pista 1 escena 2
    matrix.move_slot(0, 0, 1, 2, &proxy);

    assert!(matrix.grid[0][0].clip.is_none());
    assert!(matrix.grid[1][2].clip.is_some());
    assert_eq!(matrix.grid[1][2].clip.as_ref().unwrap().name, "Kick 42");
    assert_eq!(matrix.selected_slot, Some((1, 2)));

    let mut moved_clip = false;
    let mut set_loop_dst = false;
    while let Ok(cmd) = rx.try_recv() {
        match cmd {
            GuiCommand::MoveMatrixClip { src_track, src_scene, dst_track, dst_scene } => {
                if src_track == 0 && src_scene == 0 && dst_track == 1 && dst_scene == 2 {
                    moved_clip = true;
                }
            }
            GuiCommand::SetClipLoop { track_idx, scene_idx, .. } => {
                if track_idx == 1 && scene_idx == 2 {
                    set_loop_dst = true;
                }
            }
            _ => {}
        }
    }
    assert!(moved_clip, "Audio proxy debió recibir MoveMatrixClip(0, 0 -> 1, 2)");
    assert!(set_loop_dst, "Audio proxy debió configurar loop en nueva pista y escena");
}

#[test]
fn test_move_playing_clip_stops_old_voice() {
    let (mut matrix, proxy, rx) = create_test_state();

    let clip = create_sample_audio_clip(10, "Playing Clip");
    matrix.grid[0][0] = MatrixSlot {
        state: SlotState::Playing,
        has_stop_button: true,
        clip: Some(clip),
    };

    matrix.move_slot(0, 0, 0, 1, &proxy);

    // Ranura original queda vacía
    assert!(matrix.grid[0][0].clip.is_none());
    // Ranura destino tiene el clip parado
    assert!(matrix.grid[0][1].clip.is_some());
    assert_eq!(matrix.grid[0][1].state, SlotState::Stopped);

    let mut moved_clip = false;
    while let Ok(cmd) = rx.try_recv() {
        if let GuiCommand::MoveMatrixClip { src_track, src_scene, dst_track, dst_scene } = cmd {
            if src_track == 0 && src_scene == 0 && dst_track == 0 && dst_scene == 1 {
                moved_clip = true;
            }
        }
    }
    assert!(moved_clip, "MoveMatrixClip delega el stop y move atómico al audio engine");
}

#[test]
fn test_move_clip_replaces_occupied_slot_cleanly() {
    let (mut matrix, proxy, rx) = create_test_state();

    let clip_a = create_sample_audio_clip(1, "Clip A");
    let clip_b = create_sample_audio_clip(2, "Clip B");

    matrix.grid[0][0] = MatrixSlot {
        state: SlotState::Stopped,
        has_stop_button: true,
        clip: Some(clip_a),
    };
    matrix.grid[0][1] = MatrixSlot {
        state: SlotState::Playing,
        has_stop_button: true,
        clip: Some(clip_b),
    };

    // Mover Clip A sobre Clip B
    matrix.move_slot(0, 0, 0, 1, &proxy);

    assert!(matrix.grid[0][0].clip.is_none());
    assert!(matrix.grid[0][1].clip.is_some());
    assert_eq!(matrix.grid[0][1].clip.as_ref().unwrap().name, "Clip A");

    // Verificar que se envía MoveMatrixClip
    let mut moved_clip = false;
    while let Ok(cmd) = rx.try_recv() {
        if let GuiCommand::MoveMatrixClip { src_track, src_scene, dst_track, dst_scene } = cmd {
            if src_track == 0 && src_scene == 0 && dst_track == 0 && dst_scene == 1 {
                moved_clip = true;
            }
        }
    }
    assert!(moved_clip, "MoveMatrixClip debe limpiar el destino y reubicar en el engine");
}

#[test]
fn test_delete_slot_sends_clear_matrix_slot() {
    let (mut matrix, proxy, rx) = create_test_state();
    let clip = create_sample_audio_clip(7, "To Delete");
    matrix.grid[0][0] = MatrixSlot {
        state: SlotState::Playing,
        has_stop_button: true,
        clip: Some(clip),
    };
    matrix.delete_slot(0, 0, &proxy);
    assert!(matrix.grid[0][0].clip.is_none());

    let mut cleared = false;
    while let Ok(cmd) = rx.try_recv() {
        if let GuiCommand::ClearMatrixSlot { track_idx, scene_idx } = cmd {
            if track_idx == 0 && scene_idx == 0 {
                cleared = true;
            }
        }
    }
    assert!(cleared, "Delete slot debe mandar ClearMatrixSlot al backend");
}

#[test]
fn test_copy_clip_to_slot_preserves_source() {
    let (mut matrix, proxy, _rx) = create_test_state();

    let clip_a = create_sample_audio_clip(5, "Original Clip");
    matrix.grid[0][0] = MatrixSlot {
        state: SlotState::Stopped,
        has_stop_button: true,
        clip: Some(clip_a),
    };

    matrix.copy_to_slot(0, 0, 0, 1, &proxy);

    // Ambas ranuras deben tener clip
    assert!(matrix.grid[0][0].clip.is_some());
    assert!(matrix.grid[0][1].clip.is_some());
    assert_eq!(matrix.grid[0][0].clip.as_ref().unwrap().name, "Original Clip");
    assert_eq!(matrix.grid[0][1].clip.as_ref().unwrap().name, "Original Clip");

    // El clip copiado debe tener un ID nuevo independiente
    assert_ne!(
        matrix.grid[0][0].clip.as_ref().unwrap().id,
        matrix.grid[0][1].clip.as_ref().unwrap().id
    );
    assert_eq!(matrix.selected_slot, Some((0, 1)));
}

#[test]
fn test_move_out_of_bounds_noop() {
    let (mut matrix, proxy, _rx) = create_test_state();

    let clip = create_sample_audio_clip(99, "Bound Test");
    matrix.grid[0][0] = MatrixSlot {
        state: SlotState::Stopped,
        has_stop_button: true,
        clip: Some(clip),
    };

    // Fuera de límites no debe mutar ni causar pánico
    matrix.move_slot(0, 0, 999, 999, &proxy);
    assert!(matrix.grid[0][0].clip.is_some());

    matrix.move_slot(999, 999, 0, 0, &proxy);
    assert!(matrix.grid[0][0].clip.is_some());
}
