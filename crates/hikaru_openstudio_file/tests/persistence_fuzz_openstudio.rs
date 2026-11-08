use hikaru_openstudio_file::*;
use hikaru_project_file::Color;
use std::time::Instant;

#[test]
fn test_serde_alias_compatibility_in_openstudio() {
    let json_note = r#"{"start_tick":0,"note":60,"velocity":100,"duration_ticks":480}"#;
    let n1: MidiNote = serde_json::from_str(json_note).expect("Failed with 'note'");
    assert_eq!(n1.note, 60);

    let json_pitch = r#"{"start_tick":0,"pitch":60,"velocity":100,"duration_ticks":480}"#;
    let n2: MidiNote = serde_json::from_str(json_pitch).expect("Failed with 'pitch'");
    assert_eq!(n2.note, 60);
}

#[test]
fn test_empty_notes_default_in_clipkind_midi() {
    // ClipKind::Midi with omitted notes field: #[serde(default)] should make it empty Vec
    let json_midi_empty = r#"{"Midi":{}}"#;
    let kind: ClipKind = serde_json::from_str(json_midi_empty).expect("Failed to deserialize Midi with default empty notes");
    match kind {
        ClipKind::Midi { notes } => assert!(notes.is_empty()),
        _ => panic!("Expected ClipKind::Midi"),
    }

    let json_midi_explicit_empty = r#"{"Midi":{"notes":[]}}"#;
    let kind2: ClipKind = serde_json::from_str(json_midi_explicit_empty).expect("Failed to deserialize explicit empty notes");
    match kind2 {
        ClipKind::Midi { notes } => assert!(notes.is_empty()),
        _ => panic!("Expected ClipKind::Midi"),
    }
}

#[test]
fn test_legacy_opsf_backward_compatibility() {
    // Legacy .opsf with only Audio, Pattern, Automation clips
    let legacy_json = r#"{
        "format_version": 1,
        "created_by": "Hikaru OpenStudio 2.22.0",
        "engine_mode": "OpenStudio",
        "metadata": {
            "title": "Legacy Studio Project",
            "artist": null,
            "genre": null,
            "key": null,
            "stats": {
                "total_tracks": 2,
                "total_clips": 3,
                "audio_clips": 1,
                "midi_clips": 0
            }
        },
        "transport": {
            "bpm": 130.0,
            "beats_per_bar": 4,
            "beat_division": 4,
            "loop_start_ticks": 0,
            "loop_end_ticks": 7680,
            "loop_enabled": true
        },
        "studio_tracks": [
            {
                "id": 0,
                "name": "MASTER",
                "volume": 0.7,
                "pan": 0.0,
                "pan_mode": 0,
                "mute": false,
                "solo": false,
                "arm": false,
                "is_master": true,
                "route_destination_id": 0,
                "sends": [],
                "effects": [],
                "matrix_idx": null
            },
            {
                "id": 1,
                "name": "Audio Track",
                "volume": 0.8,
                "pan": 0.0,
                "pan_mode": 0,
                "mute": false,
                "solo": false,
                "arm": false,
                "is_master": false,
                "route_destination_id": 0,
                "sends": [],
                "effects": [],
                "matrix_idx": null
            }
        ],
        "playlist": {
            "clips": [
                [
                    1,
                    {
                        "id": 101,
                        "name": "Vocal Hook",
                        "start_tick": 0,
                        "duration_ticks": 3840,
                        "color": {"h": 0.1, "s": 0.8, "l": 0.5, "a": 1.0},
                        "kind": {
                            "Audio": {
                                "sample_path": "/audio/vox.wav",
                                "sample_offset_ticks": 0,
                                "total_sample_ticks": 3840
                            }
                        }
                    }
                ],
                [
                    1,
                    {
                        "id": 102,
                        "name": "Pattern 1",
                        "start_tick": 3840,
                        "duration_ticks": 1920,
                        "color": {"h": 0.2, "s": 0.8, "l": 0.5, "a": 1.0},
                        "kind": {
                            "Pattern": {
                                "pattern_id": 42
                            }
                        }
                    }
                ],
                [
                    1,
                    {
                        "id": 103,
                        "name": "Filter Sweep",
                        "start_tick": 0,
                        "duration_ticks": 7680,
                        "color": {"h": 0.3, "s": 0.8, "l": 0.5, "a": 1.0},
                        "kind": {
                            "Automation": {
                                "points": [
                                    {"rel_tick": 0, "value": 0.1, "tension": 0.0},
                                    {"rel_tick": 7680, "value": 0.9, "tension": 0.5}
                                ],
                                "target_param": "cutoff"
                            }
                        }
                    }
                ]
            ],
            "ppqn": 960,
            "next_clip_id": 104,
            "loop_start_ticks": 0,
            "loop_end_ticks": 7680,
            "loop_region_active": true
        },
        "ui": {
            "openstudio_view": 0,
            "show_dsp_rack": false,
            "show_explorer": false
        }
    }"#;

    let mut path = std::env::temp_dir();
    path.push(format!("legacy_opsf_{}.opsf", std::process::id()));
    std::fs::write(&path, legacy_json).unwrap();

    let loaded = parse(&path).expect("Failed to parse legacy .opsf project");
    let _ = std::fs::remove_file(&path);

    assert_eq!(loaded.playlist.clips.len(), 3);
    assert_eq!(loaded.playlist.clips[0].1.name, "Vocal Hook");
    assert_eq!(loaded.playlist.clips[1].1.name, "Pattern 1");
    assert_eq!(loaded.playlist.clips[2].1.name, "Filter Sweep");
}

#[test]
fn test_50000_notes_scale_and_roundtrip_openstudio() {
    let mut project = template("Hikaru OpenStudio 2.24.0");

    let mut massive_notes = Vec::with_capacity(50_000);
    for i in 0..50_000u64 {
        massive_notes.push(MidiNote::new(
            i * 480,
            (i % 128) as u8,
            ((i * 7) % 127 + 1) as u8,
            ((i % 960) + 1) as u32,
        ));
    }

    project.playlist.clips.push((
        1,
        PlaylistClipSaved {
            id: 200,
            name: "Mega MIDI Sequence".to_string(),
            start_tick: 0,
            duration_ticks: 50_000 * 480,
            color: Color { h: 0.8, s: 0.7, l: 0.6, a: 1.0 },
            kind: ClipKind::Midi { notes: massive_notes },
        },
    ));

    let mut path = std::env::temp_dir();
    path.push(format!("scale_test_openstudio_{}.opsf", std::process::id()));

    // Measure serialization
    let start_save = Instant::now();
    save(&path, &project).expect("Failed to save .opsf with 50,000 notes");
    let save_elapsed = start_save.elapsed();

    let metadata = std::fs::metadata(&path).unwrap();
    let file_size_bytes = metadata.len();

    // Measure deserialization
    let start_load = Instant::now();
    let loaded = parse(&path).expect("Failed to load .opsf with 50,000 notes");
    let load_elapsed = start_load.elapsed();

    let _ = std::fs::remove_file(&path);

    println!(
        "50,000 notes in .opsf: Save={:?}, Load={:?}, Size={:.2} MB",
        save_elapsed,
        load_elapsed,
        file_size_bytes as f64 / (1024.0 * 1024.0)
    );

    // Verify roundtrip fidelity
    assert_eq!(loaded.playlist.clips.len(), 1);
    let (_, clip) = &loaded.playlist.clips[0];
    match &clip.kind {
        ClipKind::Midi { notes } => {
            assert_eq!(notes.len(), 50_000);
            assert_eq!(notes[0], MidiNote::new(0, 0, 1, 1));
            assert_eq!(
                notes[49_999],
                MidiNote::new(49_999 * 480, (49_999 % 128) as u8, ((49_999 * 7) % 127 + 1) as u8, ((49_999 % 960) + 1) as u32)
            );
        }
        _ => panic!("Expected ClipKind::Midi"),
    }

    assert!(save_elapsed.as_secs_f64() < 2.0, "Serialization took too long: {:?}", save_elapsed);
    assert!(load_elapsed.as_secs_f64() < 2.0, "Deserialization took too long: {:?}", load_elapsed);
}

#[test]
fn test_boundary_payloads_openstudio() {
    // Extreme values in Midi notes inside ClipKind::Midi
    let json_extreme_clip = r#"{
        "id": 999,
        "name": "Extreme Note Clip",
        "start_tick": 0,
        "duration_ticks": 10000,
        "color": {"h": 0.0, "s": 0.0, "l": 0.0, "a": 1.0},
        "kind": {
            "Midi": {
                "notes": [
                    {"start_tick": 0, "pitch": 0, "velocity": 0, "duration_ticks": 0},
                    {"start_tick": 18446744073709551615, "note": 127, "velocity": 127, "duration_ticks": 4294967295},
                    {"start_tick": 1000, "pitch": 255, "velocity": 255, "duration_ticks": 100}
                ]
            }
        }
    }"#;

    let clip: PlaylistClipSaved = serde_json::from_str(json_extreme_clip).expect("Failed to deserialize extreme clip");
    match clip.kind {
        ClipKind::Midi { notes } => {
            assert_eq!(notes.len(), 3);
            assert_eq!(notes[0].note, 0);
            assert_eq!(notes[0].duration_ticks, 0);
            assert_eq!(notes[1].note, 127);
            assert_eq!(notes[1].duration_ticks, u32::MAX);
            assert_eq!(notes[1].start_tick, u64::MAX);
            assert_eq!(notes[2].note, 255);
        }
        _ => panic!("Expected ClipKind::Midi"),
    }
}
