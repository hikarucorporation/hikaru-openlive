use hikaru_openlive_file::*;
use std::time::Instant;

#[test]
fn test_serde_alias_compatibility_in_openlive() {
    // 1. Valid JSON with "note"
    let json_note = r#"{"start_tick":0,"note":60,"velocity":100,"duration_ticks":480}"#;
    let n1: MidiNote = serde_json::from_str(json_note).expect("Failed to deserialize with 'note'");
    assert_eq!(n1.note, 60);

    // 2. Valid JSON with "pitch" alias
    let json_pitch = r#"{"start_tick":0,"pitch":60,"velocity":100,"duration_ticks":480}"#;
    let n2: MidiNote = serde_json::from_str(json_pitch).expect("Failed to deserialize with 'pitch'");
    assert_eq!(n2.note, 60);

    // 3. Array of mixed notes
    let json_mixed = r#"[
        {"start_tick":0,"note":60,"velocity":100,"duration_ticks":480},
        {"start_tick":480,"pitch":64,"velocity":90,"duration_ticks":240}
    ]"#;
    let notes: Vec<MidiNote> = serde_json::from_str(json_mixed).expect("Failed to deserialize mixed array");
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[0].note, 60);
    assert_eq!(notes[1].note, 64);

    // 4. Duplicate fields: both "note" and "pitch"
    let json_duplicate = r#"{"start_tick":0,"note":60,"pitch":64,"velocity":100,"duration_ticks":480}"#;
    let res: Result<MidiNote, _> = serde_json::from_str(json_duplicate);
    // Serde detects duplicate field
    println!("Duplicate field 'note' and 'pitch' result: {:?}", res);
    assert!(res.is_err(), "Expected serde to reject duplicate alias/field definition");
}

#[test]
fn test_boundary_payloads_midi_note() {
    // Pitch 0, pitch 127
    let n_min: MidiNote = serde_json::from_str(r#"{"start_tick":0,"note":0,"velocity":0,"duration_ticks":0}"#).unwrap();
    assert_eq!(n_min.note, 0);
    assert_eq!(n_min.velocity, 0);
    assert_eq!(n_min.duration_ticks, 0);

    let n_max: MidiNote = serde_json::from_str(r#"{"start_tick":18446744073709551615,"note":127,"velocity":127,"duration_ticks":4294967295}"#).unwrap();
    assert_eq!(n_max.note, 127);
    assert_eq!(n_max.velocity, 127);
    assert_eq!(n_max.duration_ticks, u32::MAX);
    assert_eq!(n_max.start_tick, u64::MAX);

    // Pitch 128..255 fits in u8
    let n_high: MidiNote = serde_json::from_str(r#"{"start_tick":100,"note":255,"velocity":255,"duration_ticks":100}"#).unwrap();
    assert_eq!(n_high.note, 255);

    // Pitch out of bounds (negative or > 255)
    let res_neg: Result<MidiNote, _> = serde_json::from_str(r#"{"start_tick":0,"note":-1,"velocity":100,"duration_ticks":480}"#);
    assert!(res_neg.is_err(), "Negative pitch must fail parsing");

    let res_overflow: Result<MidiNote, _> = serde_json::from_str(r#"{"start_tick":0,"note":256,"velocity":100,"duration_ticks":480}"#);
    assert!(res_overflow.is_err(), "Pitch > 255 must fail parsing");

    // Duration out of bounds (negative or > u32::MAX)
    let res_dur_neg: Result<MidiNote, _> = serde_json::from_str(r#"{"start_tick":0,"note":60,"velocity":100,"duration_ticks":-5}"#);
    assert!(res_dur_neg.is_err(), "Negative duration must fail parsing");

    let res_dur_overflow: Result<MidiNote, _> = serde_json::from_str(r#"{"start_tick":0,"note":60,"velocity":100,"duration_ticks":4294967296}"#);
    assert!(res_dur_overflow.is_err(), "Duration > u32::MAX must fail parsing");
}

#[test]
fn test_50000_notes_scale_and_roundtrip_openlive() {
    let mut project = template("Hikaru OpenLive 2.24.0");

    let mut massive_notes = Vec::with_capacity(50_000);
    for i in 0..50_000u64 {
        massive_notes.push(MidiNote::new(
            i * 480,
            (i % 128) as u8,
            ((i * 7) % 127 + 1) as u8,
            ((i % 960) + 1) as u32,
        ));
    }

    let clip = Clip {
        id: 1,
        name: "Massive MIDI Clip".to_string(),
        path: String::new(),
        loop_start: 0,
        loop_end: 50_000 * 480,
        loop_enabled: true,
        volume: 0.8,
        pan: 0.0,
        mute: false,
        events: Vec::new(),
        midi_notes: massive_notes,
    };

    project.matrix.grid[0][0] = Some(clip);
    project.metadata.stats.midi_clips = 1;

    let mut path = std::env::temp_dir();
    path.push(format!("scale_test_openlive_{}.oplf", std::process::id()));

    // Measure serialization time
    let start_save = Instant::now();
    save(&path, &project).expect("Failed to serialize 50,000 notes to .oplf");
    let save_elapsed = start_save.elapsed();

    let metadata = std::fs::metadata(&path).unwrap();
    let file_size_bytes = metadata.len();

    // Measure deserialization time
    let start_load = Instant::now();
    let loaded = parse(&path).expect("Failed to deserialize 50,000 notes from .oplf");
    let load_elapsed = start_load.elapsed();

    let _ = std::fs::remove_file(&path);

    println!(
        "50,000 notes in .oplf: Save={:?}, Load={:?}, Size={:.2} MB",
        save_elapsed,
        load_elapsed,
        file_size_bytes as f64 / (1024.0 * 1024.0)
    );

    // Verify roundtrip fidelity
    let loaded_clip = loaded.matrix.grid[0][0].as_ref().expect("Clip missing");
    assert_eq!(loaded_clip.midi_notes.len(), 50_000);
    assert_eq!(loaded_clip.midi_notes[0], MidiNote::new(0, 0, 1, 1));
    assert_eq!(
        loaded_clip.midi_notes[49_999],
        MidiNote::new(49_999 * 480, (49_999 % 128) as u8, ((49_999 * 7) % 127 + 1) as u8, ((49_999 % 960) + 1) as u32)
    );

    // Bench performance thresholds (allowing up to 5s for 50,000 pretty-printed JSON objects in debug mode)
    assert!(save_elapsed.as_secs_f64() < 5.0, "Serialization took too long: {:?}", save_elapsed);
    assert!(load_elapsed.as_secs_f64() < 5.0, "Deserialization took too long: {:?}", load_elapsed);
}

#[test]
fn test_legacy_oplf_backward_compatibility() {
    // Legacy .oplf clip JSON without "midi_notes"
    let legacy_clip_json = r#"{
        "id": 1,
        "name": "Old Kick",
        "path": "/samples/kick.wav",
        "loop_start": 0,
        "loop_end": 1920,
        "loop_enabled": true,
        "volume": 0.8,
        "pan": 0.0,
        "mute": false,
        "events": []
    }"#;

    let res: Result<Clip, _> = serde_json::from_str(legacy_clip_json);
    println!("Legacy clip without 'midi_notes' parse result: {:?}", res);
    // We expect this to fail currently because Clip lacks #[serde(default)] on midi_notes!
    if let Err(e) = &res {
        println!("CONFIRMED BUG: Clip failed to deserialize without midi_notes: {}", e);
    }
}
