# M1 Analysis Report: Persistence Schemas & Test Validation Strategy

**Explorer**: M1 Explorer 3 — Persistence & Test Validation Strategy  
**Date**: 2026-11-08  
**Scope**: `crates/hikaru_openlive_file`, `crates/hikaru_openstudio_file`, `crates/hikaru_project_file`, `crates/hikaru_audio_engine`, `crates/hikaru_core`, `crates/hikaru_cli`, `crates/hikaru_mcp`, and `crates/hikaru_gui` (project persistence bridges).

---

## 1. Executive Summary

This investigation analyzed the project persistence architecture (`.oplf` and `.opsf`) and formulated the test validation strategy for **Milestone 1 (MIDI Data Model, Engine Scheduling & Project Persistence)** in Hikaru OpenLive.

### Core Discoveries:
1. **OpenLive File (`.oplf`) Already Encodes MIDI Notes**: The `.oplf` schema (`crates/hikaru_openlive_file/src/lib.rs`) already contains `midi_notes: Vec<MidiNote>` on `Clip`. However, the pitch field is named `note: u8` (rather than `pitch: u8`). To guarantee zero deserialization failures across versions and conform to the canonical `MidiNoteInstance` naming, a serde alias `#[serde(alias = "pitch")]` is required.
2. **OpenStudio File (`.opsf`) Gap & Backward Compatibility**: The `.opsf` schema (`crates/hikaru_openstudio_file/src/lib.rs`) currently provides only `ClipKind::Pattern { pattern_id: usize }`, which stores no note data on disk. To persist arranger MIDI clips without breaking backwards compatibility with existing `.opsf` v1 files, `ClipKind` must be extended with `Midi { #[serde(default)] notes: Vec<MidiNote> }`. All existing files containing `Audio`, `Pattern`, or `Automation` continue to deserialize without error.
3. **Licensing Isolation (LGPLv3 vs. AGPLv3)**: `hikaru_openlive_file`, `hikaru_openstudio_file`, and `hikaru_project_file` are strictly licensed under **LGPL-3.0-or-later** and depend only on `serde` and `serde_json`. They cannot import types from AGPLv3 crates (`hikaru_core` or `hikaru_audio_engine`). Therefore, the shared `MidiNote` persistence primitive must reside in `hikaru_project_file` and be re-exported by both `.oplf` and `.opsf`. The AGPL GUI/engine crates convert between `MidiNote` and `MidiNoteInstance` via `From`/`Into` traits.
4. **Audio Engine MIDI Scheduling Gap**: In `AudioEngine`, `self.midi_clips: Vec<MidiClipInstance>` exists but is completely unreferenced inside `AudioEngine::process()`. Furthermore, `MidiClipInstance` lacks playback fields (`start_frame`, `start_absolute`, `duration_ticks`, `clip_loop_enabled`, `clip_loop_start_ticks`, `clip_loop_end_ticks`, `active_notes`).
5. **Anti-Regression & Glitch-Free Invariants**: The existing `hikaru_audio_engine` test suite consists of 43 unit tests (12 preview player, 31 engine) plus 11 integration tests (5 ghost voice, 3 mixer routing, 2 plugin routing, 1 convergence). All 54 tests must pass cleanly. The MIDI scheduling implementation must adhere to strict real-time safety: zero heap allocations in `process()`, zero lock contention, and categorical silence (peak < 1e-6) outside clip boundaries.

---

## 2. Persistence Schemas & Compatibility Analysis

### 2.1 License Boundary Architecture
The persistence crates form a modular, self-contained sub-ecosystem:

```
crates/
├── hikaru_project_file     [LGPL-3.0-or-later] Shared primitives: Track, DspSlot, Color, MidiNote
│   ├── hikaru_openlive_file   [LGPL-3.0-or-later] .oplf: Session Matrix, Scenes, Pads, MidiNotes
│   └── hikaru_openstudio_file [LGPL-3.0-or-later] .opsf: Arranger Playlist, Timeline, ClipKind::Midi
│
└── [AGPLv3 App Layer] ─────────────────────────────────────────────────────────────┐
    ├── hikaru_core            Canonical audio types (zero external dependencies)   │
    ├── hikaru_audio_engine    Real-time audio callback & voice scheduling          │
    └── hikaru_gui/project/    Bidirectional AGPL <-> LGPL translation bridge       │
```

Because LGPLv3 crates cannot depend on AGPLv3 crates, any attempt to import `MidiNoteInstance` from `hikaru_core` or `hikaru_audio_engine` into `hikaru_openlive_file` or `hikaru_openstudio_file` would violate the license boundary and fail to build.

### 2.2 OpenLive File Format (`.oplf`)
- **Specification**: `docs/hikaru-project-files/hikaru-openlive-file/OPLF-README.md`
- **Location**: `crates/hikaru_openlive_file/src/lib.rs`
- **Format Version**: `1`
- **Current Definition**:
  ```rust
  pub struct Clip {
      pub id: usize,
      pub name: String,
      pub path: String,
      pub loop_start: u64,
      pub loop_end: u64,
      pub loop_enabled: bool,
      pub volume: f32,
      pub pan: f32,
      pub mute: bool,
      pub events: Vec<Event>,
      pub midi_notes: Vec<MidiNote>,
  }

  #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
  pub struct MidiNote {
      pub start_tick: u64,
      pub note: u8,
      pub velocity: u8,
      pub duration_ticks: u32,
  }
  ```
- **Discrepancy**: The pitch field is named `note` in `hikaru_openlive_file`, but named `pitch` in `hikaru_audio_engine::MidiNoteInstance`, `hikaru_gui::audio_proxy::MidiNoteInstance`, and `hikaru_gui::piano_roll::MidiNote`.
- **Compatibility Solution**:
  In `MidiNote`, apply `#[serde(alias = "pitch")]` on `pub note: u8`:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
  pub struct MidiNote {
      pub start_tick: u64,
      #[serde(alias = "pitch")]
      pub note: u8,
      pub velocity: u8,
      pub duration_ticks: u32,
  }
  ```
  This allows incoming JSON with either `"note": 60` or `"pitch": 60` to deserialize without error, while serializing as `"note": 60` in accordance with `.oplf` specification v1.

### 2.3 OpenStudio File Format (`.opsf`)
- **Specification**: `docs/hikaru-project-files/hikaru-openstudio-file/OPSF-README.md`
- **Location**: `crates/hikaru_openstudio_file/src/lib.rs`
- **Format Version**: `1`
- **Current Definition**:
  ```rust
  pub struct PlaylistClipSaved {
      pub id: usize,
      pub name: String,
      pub start_tick: u64,
      pub duration_ticks: u64,
      pub color: Color,
      pub kind: ClipKind,
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub enum ClipKind {
      Pattern { pattern_id: usize },
      Audio {
          sample_path: String,
          sample_offset_ticks: u64,
          total_sample_ticks: u64,
      },
      Automation {
          points: Vec<CurvePoint>,
          target_param: String,
      },
  }
  ```
- **Identified Gap**: `ClipKind::Pattern` only stores a scalar `pattern_id: usize`. There is no note sequence stored in `.opsf`. An Arranger MIDI clip created in the timeline loses all notes when saved to `.opsf`.
- **Extension Strategy**:
  Add `ClipKind::Midi` to `ClipKind`:
  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub enum ClipKind {
      Pattern { pattern_id: usize },
      Audio {
          sample_path: String,
          sample_offset_ticks: u64,
          total_sample_ticks: u64,
      },
      Automation {
          points: Vec<CurvePoint>,
          target_param: String,
      },
      Midi {
          #[serde(default)]
          notes: Vec<MidiNote>,
      },
  }
  ```
- **Exhaustive Pattern Matching Audit**:
  Adding a variant to `ClipKind` affects downstream crates that match on `ClipKind`:
  1. `crates/hikaru_cli/src/project.rs:339`:
     ```rust
     hikaru_openstudio_file::ClipKind::Midi { .. } => {
         midi_clips += 1;
         ("midi", None)
     }
     ```
  2. `crates/hikaru_mcp/src/project.rs:176`:
     ```rust
     hikaru_openstudio_file::ClipKind::Midi { notes } => json!({
         "tipo": "midi",
         "total_notas": notes.len(),
     }),
     ```
  3. `crates/hikaru_gui/src/project/openstudio.rs:94`:
     ```rust
     ClipKind::Pattern { .. } | ClipKind::Automation { .. } | ClipKind::Midi { .. } => midi_clips += 1,
     ```
  4. `crates/hikaru_gui/src/project/openstudio.rs:129` (`capture_playlist_clip`):
     ```rust
     ClipType::Midi { notes } => ClipKind::Midi {
         notes: notes.iter().map(|n| MidiNote {
             start_tick: n.start_tick,
             note: n.pitch,
             velocity: n.velocity,
             duration_ticks: n.duration_ticks,
         }).collect(),
     },
     ```
  5. `crates/hikaru_gui/src/project/openstudio.rs:262` (`build_playlist_clip`):
     ```rust
     ClipKind::Midi { notes } => ClipType::Midi {
         notes: notes.iter().map(|n| MidiNoteInstance {
             start_tick: n.start_tick,
             pitch: n.note,
             velocity: n.velocity,
             duration_ticks: n.duration_ticks,
         }).collect(),
     },
     ```

### 2.4 Canonical Shared Type in `hikaru_project_file`
To avoid code duplication and schema drift:
1. Place `MidiNote` in `crates/hikaru_project_file/src/lib.rs`:
   ```rust
   #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
   pub struct MidiNote {
       pub start_tick: u64,
       #[serde(alias = "pitch")]
       pub note: u8,
       pub velocity: u8,
       pub duration_ticks: u32,
   }
   ```
2. Re-export in `crates/hikaru_openlive_file/src/lib.rs`:
   ```rust
   pub use hikaru_project_file::MidiNote;
   ```
3. Re-export in `crates/hikaru_openstudio_file/src/lib.rs`:
   ```rust
   pub use hikaru_project_file::MidiNote;
   ```
This provides 100% source-level and binary-level backward compatibility.

---

## 3. Audio Engine MIDI Scheduling Architecture

### 3.1 `MidiClipInstance` Struct Refactoring
In `crates/hikaru_audio_engine/src/lib.rs`:
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MidiNoteInstance {
    pub start_tick: u64,
    pub pitch: u8,
    pub velocity: u8,
    pub duration_ticks: u32,
}

pub struct MidiClipInstance {
    pub id: usize,
    pub track_index: usize,
    pub scene_index: usize,
    pub notes: Vec<MidiNoteInstance>,
    pub is_playing: bool,
    pub start_frame: u64,
    pub start_absolute: u64,
    pub duration_ticks: u64,
    pub clip_loop_enabled: bool,
    pub clip_loop_start_ticks: u64,
    pub clip_loop_end_ticks: u64,
    pub is_matrix_clip: bool,
    /// Real-time active sounding notes bitset (zero-allocation note-off tracker)
    pub active_notes: [bool; 128],
}
```

### 3.2 Time Base & Clock Conversion Authority
- `PPQN = 960` (`DEFAULT_PPQN` in `hikaru_transport`).
- For any buffer block starting at `abs_start` (or `sample_count`) with length `buffer_frames`:
  $$\text{elapsed\_start\_frames} = \text{abs\_start} - \text{clip.start\_absolute}$$
  $$\text{elapsed\_end\_frames} = \text{elapsed\_start\_frames} + \text{buffer\_frames}$$
  $$\text{tick\_start} = \text{transport.samples\_to\_ticks}(\text{elapsed\_start\_frames})$$
  $$\text{tick\_end} = \text{transport.samples\_to\_ticks}(\text{elapsed\_end\_frames})$$

### 3.3 Loop Wrapping Logic (Ticks)
If `clip.clip_loop_enabled && clip.clip_loop_end_ticks > clip.clip_loop_start_ticks`:
- Loop length: $L = \text{clip.clip\_loop\_end\_ticks} - \text{clip.clip\_loop\_start\_ticks}$.
- A note at $N.\text{start\_tick}$ triggers if:
  $$(N.\text{start\_tick} \pmod L) \in [\text{tick\_start} \pmod L, \text{tick\_end} \pmod L)$$
  taking into account wrap-around when the buffer boundary spans across the loop boundary ($(\text{tick\_start} \pmod L) > (\text{tick\_end} \pmod L)$).
- When looping is disabled:
  The clip terminates when $\text{elapsed\_start\_frames} \ge \text{transport.ticks\_to\_samples}(\text{clip.duration\_ticks})$.
  All active notes are immediately released/killed, and `clip.is_playing = false`.

### 3.4 Real-Time Dispatch & Safety
In `AudioEngine::process()`:
- No heap allocations: notes are dispatched directly to `self.dms_voice_pool` or copied into fixed-size stack buffers `[MidiEvent; 64]` for track effect chains.
- Dispatch channels:
  1. **OpenDMS (Drum Sampler)**:
     - NoteOn: `self.trigger_dms_note(note.pitch as usize, gain, pan, note.velocity as f32 / 127.0, 1.0, adsr, track_idx)`
     - NoteOff: `self.release_dms_note_for_track(track_idx, note.pitch as usize)`
     - Kill: `self.kill_dms_note_for_track(track_idx, note.pitch as usize)`
  2. **VST3/CLAP Plugin Nodes**:
     - `DspPluginNode::process(&mut track_buf, &[MidiEvent { status: 0x90, data1: pitch, data2: vel }])`

---

## 4. Concrete Test Validation Plan for Milestone 1

### 4.1 Unit Test Matrix (`hikaru_audio_engine`)

| Test Identifier | Category | Verification Objective | Assertion / Pass Criteria |
| :--- | :--- | :--- | :--- |
| `test_midi_clip_note_on_at_exact_sample_quantum` | Scheduling | Verifies NoteOn is dispatched at the exact sample frame inside the buffer corresponding to note's `start_tick`. | Voice in `dms_voice_pool` becomes active at target sample; audio output in that frame > 0.0. |
| `test_midi_clip_note_off_released_at_exact_duration` | Scheduling | Verifies NoteOff/Release is dispatched when playhead reaches `start_tick + duration_ticks`. | Active note transitions from `Attack/Sustain` to `Release` stage; voice stops after release decay. |
| `test_midi_clip_polyphony_simultaneous_notes` | Scheduling | Verifies multiple simultaneous notes (e.g. 3-note chord) trigger without voice dropping. | All 3 pitches exist as active voices; no voice starvation. |
| `test_midi_clip_note_sustained_across_buffers` | Scheduling | Verifies a long note spanning multiple 512-sample buffers stays sounding without repeated NoteOns. | Voice stays continuously active across 5 successive `process()` calls; exactly 1 NoteOn, exactly 1 NoteOff. |
| `test_midi_clip_loop_wrapping_exact_beat` | Looping | Verifies note at tick 0 repeats with zero phase drift across 1-bar (3840 ticks) loop iterations. | Note triggers again at exactly sample $N \times 3840 \text{ ticks}$; trigger timestamp matches integer multiple. |
| `test_midi_clip_loop_sub_bar_and_fractional` | Looping | Verifies 2-beat (1920 ticks) loop wraps and triggers intermediate notes accurately. | Note at tick 480 triggers at 480, 2400, 4320 ticks. |
| `test_midi_clip_note_at_loop_boundary_release` | Boundary | Verifies note extending beyond loop end does not hang or leak. | Active note bitset clears; voice released at loop boundary; no stuck note. |
| `test_midi_clip_non_looping_stops_and_releases` | Leakage | Verifies non-looping clip past duration sets `is_playing = false` and mutes completely. | `voice_active(t, s) == false`; audio buffer is absolute silence (`peak < 1e-6`). |
| `test_midi_clip_trigger_toggle_stops_and_kills` | Voice Lifecycle | Verifies toggling clip via `trigger_clip` immediately kills active voices. | Voices killed immediately; subsequent buffer peak is 0.0. |
| `test_midi_clip_legato_trigger_phase_inheritance` | Synchronization | Verifies switching from one MIDI clip to another inherits phase if legato is active. | `start_absolute` matches running legato reference. |
| `test_midi_scheduling_zero_allocations` | RT Safety | Verifies scheduling runs without invoking memory allocators on the audio thread. | Buffer processing benchmark/hook confirms zero heap allocations during playback. |

### 4.2 Persistence Roundtrip Test Matrix (`.oplf` & `.opsf`)

| Test Identifier | Crate | Verification Objective | Assertion / Pass Criteria |
| :--- | :--- | :--- | :--- |
| `test_oplf_midi_clip_roundtrip` | `hikaru_openlive_file` | Saves and loads `.oplf` with MIDI clips, notes, velocities, and loop boundaries. | Deserialized project matches original `notes`, `loop_start`, `loop_end`, `loop_enabled`. `metadata.stats.midi_clips == 1`. |
| `test_oplf_pitch_alias_compatibility` | `hikaru_openlive_file` | Validates that JSON containing `"pitch"` instead of `"note"` parses successfully. | `serde_json::from_str` succeeds; `note == 60`. |
| `test_oplf_legacy_empty_midi_clip` | `hikaru_openlive_file` | Validates that an `.oplf` without MIDI notes loads cleanly. | Deserialization succeeds; `midi_notes.is_empty()`. |
| `test_opsf_midi_clip_roundtrip` | `hikaru_openstudio_file` | Saves and loads `.opsf` with `ClipKind::Midi`. | Deserialized `PlaylistClipSaved` has `ClipKind::Midi` with exact note vector; `ppqn == 960`. |
| `test_opsf_legacy_v1_backward_compat` | `hikaru_openstudio_file` | Validates that existing v1 `.opsf` containing `ClipKind::Pattern`, `Audio`, and `Automation` continue to parse without errors. | All 3 clip kinds parse; `stats.audio_clips` and `stats.midi_clips` match v1 counts. |
| `test_opsf_corrupt_payload_rejection` | `hikaru_openstudio_file` | Validates that invalid format versions or malformed JSON are rejected via `ProjectError`. | Returns `ProjectError::UnsupportedVersion` or `ProjectError::Parse`. |

### 4.3 Regression Validation Suite
All 43 existing unit tests in `crates/hikaru_audio_engine/src/lib.rs` and 11 integration tests must pass cleanly:
1. `preview_player::tests` (12 tests): additive mixing, clamping, generation increments, mono/stereo playback, volume clamps.
2. `tests` in `hikaru_audio_engine::lib.rs` (31 tests): FTZ/DAZ, tempo tracking, quantized loop beats, floating-point drift prevention, global loop wrapping, legato pad switching, multi-event pad mixing, strict silence past native length, trigger toggling.
3. Integration tests:
   - `ghost_voice_repro.rs` (5 tests): verifies -inf dB silence past bar 5 in both OpenLive and OpenStudio modes.
   - `mixer_track_routing.rs` (3 tests): verifies per-track DMS volume and panning isolation.
   - `plugin_track_routing.rs` (2 tests): verifies VST3/CLAP chain isolation.
   - `convergence_test.rs` (1 test): verifies audio convergence under stress.

---

## 5. Worker Implementation Directives

### 5.1 Code Changes in `crates/hikaru_project_file/src/lib.rs`
Define `MidiNote` in the shared LGPL crate:
```rust
/// Nota MIDI persistible: `(inicio_en_ticks, nota/pitch, velocity, duración_en_ticks)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidiNote {
    pub start_tick: u64,
    #[serde(alias = "pitch")]
    pub note: u8,
    pub velocity: u8,
    pub duration_ticks: u32,
}
```

### 5.2 Code Changes in `crates/hikaru_openlive_file/src/lib.rs`
Re-export `MidiNote` from `hikaru_project_file`:
```rust
pub use hikaru_project_file::MidiNote;
```
Remove the local `pub struct MidiNote` definition (lines 260-266) to avoid duplicate type symbols.

### 5.3 Code Changes in `crates/hikaru_openstudio_file/src/lib.rs`
1. Re-export `MidiNote`:
   ```rust
   pub use hikaru_project_file::MidiNote;
   ```
2. Extend `ClipKind` (lines 156-171):
   ```rust
   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub enum ClipKind {
       Pattern { pattern_id: usize },
       Audio {
           sample_path: String,
           sample_offset_ticks: u64,
           total_sample_ticks: u64,
       },
       Automation {
           points: Vec<CurvePoint>,
           target_param: String,
       },
       Midi {
           #[serde(default)]
           notes: Vec<MidiNote>,
       },
   }
   ```

### 5.4 Code Changes in Downstream Crates
1. `crates/hikaru_cli/src/project.rs`:
   Add match arm in `capture_studio_project`:
   ```rust
   hikaru_openstudio_file::ClipKind::Midi { .. } => {
       midi_clips += 1;
       ("midi", None)
   }
   ```
2. `crates/hikaru_mcp/src/project.rs`:
   Add match arm in clip inspect handler:
   ```rust
   hikaru_openstudio_file::ClipKind::Midi { notes } => json!({
       "tipo": "midi",
       "total_notas": notes.len(),
   }),
   ```
3. `crates/hikaru_gui/src/project/openstudio.rs`:
   Update `capture_metadata`, `capture_playlist_clip`, and `build_playlist_clip` to handle `ClipKind::Midi`.
4. `crates/hikaru_gui/src/views/playlist.rs`:
   Add `Midi { notes: Vec<MidiNoteInstance> }` to `ClipType`.

### 5.5 Code Changes in `crates/hikaru_audio_engine/src/lib.rs`
1. Update `MidiClipInstance`:
   ```rust
   pub struct MidiClipInstance {
       pub id: usize,
       pub track_index: usize,
       pub scene_index: usize,
       pub notes: Vec<MidiNoteInstance>,
       pub is_playing: bool,
       pub start_frame: u64,
       pub start_absolute: u64,
       pub duration_ticks: u64,
       pub clip_loop_enabled: bool,
       pub clip_loop_start_ticks: u64,
       pub clip_loop_end_ticks: u64,
       pub is_matrix_clip: bool,
       pub active_notes: [bool; 128],
   }
   ```
2. Implement `AudioEngine::add_midi_clip`:
   ```rust
   pub fn add_midi_clip(
       &mut self,
       id: usize,
       track_index: usize,
       scene_index: usize,
       notes: Vec<MidiNoteInstance>,
       duration_ticks: u64,
       clip_loop_enabled: bool,
       clip_loop_start_ticks: u64,
       clip_loop_end_ticks: u64,
       is_matrix_clip: bool,
   )
   ```
3. Update `trigger_clip` and `trigger_scene` to activate `midi_clips` and clear `active_notes`.
4. Update `voice_active` to check both `clips` and `midi_clips`.
5. In `AudioEngine::process()`:
   Iterate `self.midi_clips`, calculate current quantum in ticks, detect NoteOn / NoteOff, trigger `dms_voice_pool` or dispatch events, update `active_notes`, and terminate non-looping clips at duration boundary.

---

## 6. Concrete Test Code Definitions for Worker

### 6.1 Audio Engine MIDI Unit Tests
```rust
#[test]
fn test_midi_clip_note_on_at_exact_sample_quantum() {
    let mut engine = test_engine();
    engine.set_mode(EngineMode::OpenLive);
    engine.load_dms_sample(36, vec![1.0f32; 44100], 1);
    
    // Note at tick 960 (1 beat = 22050 samples at 120 BPM, 44.1kHz)
    let notes = vec![MidiNoteInstance {
        start_tick: 960,
        pitch: 36,
        velocity: 100,
        duration_ticks: 480,
    }];
    engine.add_midi_clip(1, 0, 0, notes, 3840, false, 0, 3840, true);
    engine.play();
    engine.trigger_clip(0, 0);
    
    // Run 40 buffers of 512 samples = 20480 samples (< 22050)
    for _ in 0..40 {
        run_frames(&mut engine, 512);
    }
    // Quantum [20480..22528) contains sample 22050
    let mut raw = vec![0.0f32; 512 * 2];
    let mut buf = AudioBuffer::new(&mut raw);
    engine.process(&mut buf);
    
    let peak = raw.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(peak > 0.1, "NoteOn must trigger within quantum containing sample 22050 (peak={peak})");
}

#[test]
fn test_midi_clip_loop_wrapping_exact_beat() {
    let mut engine = test_engine();
    engine.set_mode(EngineMode::OpenLive);
    engine.load_dms_sample(36, vec![1.0f32; 44100], 1);
    
    // Note at tick 0, loop length 1 bar (3840 ticks = 88200 samples)
    let notes = vec![MidiNoteInstance {
        start_tick: 0,
        pitch: 36,
        velocity: 100,
        duration_ticks: 240,
    }];
    engine.add_midi_clip(1, 0, 0, notes, 3840, true, 0, 3840, true);
    engine.play();
    engine.trigger_clip(0, 0);
    
    // Render past the 1-bar loop (88200 samples / 512 = ~172 buffers)
    for _ in 0..172 {
        run_frames(&mut engine, 512);
    }
    // Loop wraps: note at tick 0 triggers again
    let mut raw = vec![0.0f32; 512 * 2];
    let mut buf = AudioBuffer::new(&mut raw);
    engine.process(&mut buf);
    let peak = raw.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(peak > 0.1, "Loop wrapped note at tick 0 must retrigger (peak={peak})");
}

#[test]
fn test_midi_clip_non_looping_stops_and_releases() {
    let mut engine = test_engine();
    engine.set_mode(EngineMode::OpenLive);
    engine.load_dms_sample(36, vec![1.0f32; 44100], 1);
    
    // Note at tick 0, duration 480 ticks, clip duration 960 ticks (22050 samples)
    let notes = vec![MidiNoteInstance {
        start_tick: 0,
        pitch: 36,
        velocity: 100,
        duration_ticks: 480,
    }];
    engine.add_midi_clip(1, 0, 0, notes, 960, false, 0, 0, true);
    engine.play();
    engine.trigger_clip(0, 0);
    assert!(engine.voice_active(0, 0));
    
    // Run 50 buffers of 512 samples = 25600 samples (> 22050)
    for _ in 0..50 {
        run_frames(&mut engine, 512);
    }
    assert!(!engine.voice_active(0, 0), "Clip must stop after clip duration");
    
    // Measure silence past clip boundary
    let mut raw = vec![0.0f32; 512 * 2];
    let mut buf = AudioBuffer::new(&mut raw);
    engine.process(&mut buf);
    let peak = raw.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(peak < 1e-6, "Must be absolute silence past clip end (peak={peak})");
}
```

### 6.2 Persistence Roundtrip Tests
```rust
#[test]
fn test_opsf_midi_clip_roundtrip() {
    let mut path = std::env::temp_dir();
    path.push(format!("hikaru_opsf_midi_{}.opsf", std::process::id()));

    let mut project = hikaru_openstudio_file::template("Hikaru OpenStudio 2.22.2");
    project.playlist.clips.push((
        1,
        hikaru_openstudio_file::PlaylistClipSaved {
            id: 1,
            name: "Midi Pattern 1".into(),
            start_tick: 0,
            duration_ticks: 3840,
            color: hikaru_project_file::Color { h: 0.2, s: 0.8, l: 0.5, a: 1.0 },
            kind: hikaru_openstudio_file::ClipKind::Midi {
                notes: vec![
                    hikaru_project_file::MidiNote {
                        start_tick: 0,
                        note: 60,
                        velocity: 100,
                        duration_ticks: 480,
                    },
                    hikaru_project_file::MidiNote {
                        start_tick: 480,
                        note: 64,
                        velocity: 90,
                        duration_ticks: 480,
                    },
                ],
            },
        },
    ));
    project.metadata.stats.midi_clips = 1;
    project.metadata.stats.total_clips = 1;

    hikaru_openstudio_file::save(&path, &project).unwrap();

    let loaded = hikaru_openstudio_file::parse(&path).unwrap();
    assert_eq!(loaded.playlist.ppqn, 960);
    assert_eq!(loaded.playlist.clips.len(), 1);
    match &loaded.playlist.clips[0].1.kind {
        hikaru_openstudio_file::ClipKind::Midi { notes } => {
            assert_eq!(notes.len(), 2);
            assert_eq!(notes[0].note, 60);
            assert_eq!(notes[0].duration_ticks, 480);
            assert_eq!(notes[1].note, 64);
        }
        _ => panic!("Expected ClipKind::Midi"),
    }
    let _ = std::fs::remove_file(&path);
}
```
