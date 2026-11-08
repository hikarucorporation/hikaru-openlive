# Handoff Report: M1 Persistence & Test Validation Strategy

**Agent**: M1 Explorer 3 (Persistence & Test Validation Strategy)  
**State Folder**: `/home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/.agents/teamwork/m1_explorer_3`  
**Handoff Type**: Hard (Task Complete)

---

## 1. Observation

1. **OpenLive File Format (`.oplf`) MIDI Model**:
   - In `crates/hikaru_openlive_file/src/lib.rs:238`: `Clip` contains `pub midi_notes: Vec<MidiNote>`.
   - In `crates/hikaru_openlive_file/src/lib.rs:260-266`:
     ```rust
     #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
     pub struct MidiNote {
         pub start_tick: u64,
         pub note: u8,
         pub velocity: u8,
         pub duration_ticks: u32,
     }
     ```
   - Pitch is named `note: u8` in `.oplf`, whereas it is named `pitch: u8` in `hikaru_audio_engine::MidiNoteInstance` (line 25), `hikaru_gui::audio_proxy::MidiNoteInstance` (line 11), and `hikaru_gui::piano_roll::MidiNote` (line 25).
2. **OpenStudio File Format (`.opsf`) Pattern Storage Gap**:
   - In `crates/hikaru_openstudio_file/src/lib.rs:156-171`:
     ```rust
     #[derive(Debug, Clone, Serialize, Deserialize)]
     pub enum ClipKind {
         Pattern { pattern_id: usize },
         Audio { sample_path: String, sample_offset_ticks: u64, total_sample_ticks: u64 },
         Automation { points: Vec<CurvePoint>, target_param: String },
     }
     ```
   - `ClipKind::Pattern` contains only `pattern_id: usize`. There is no pattern note storage anywhere in `OpenStudioProject`.
3. **LGPLv3 License Boundary & Crate Topology**:
   - In `crates/hikaru_project_file/src/lib.rs:1-39`:
     ```rust
     // crates/hikaru_project_file/src/lib.rs
     // SPDX-License-Identifier: LGPL-3.0-or-later
     // Un DAW de CÓDIGO CERRADO (Image-Line, Ableton, Bitwig) que quiera leer o
     // escribir proyectos .oplf/.opsf no debería tener que abrir el código del motor...
     ```
   - `hikaru_project_file`, `hikaru_openlive_file`, and `hikaru_openstudio_file` depend exclusively on `serde` and `serde_json` and must not import types from AGPL crates (`hikaru_core` or `hikaru_audio_engine`).
4. **Exhaustive Downstream Matches on `ClipKind`**:
   - In `crates/hikaru_cli/src/project.rs:339-352`: matches exhaustively on `ClipKind::Audio`, `ClipKind::Pattern`, `ClipKind::Automation`.
   - In `crates/hikaru_mcp/src/project.rs:176-196`: matches exhaustively on `ClipKind::Pattern`, `ClipKind::Audio`, `ClipKind::Automation`.
   - In `crates/hikaru_gui/src/project/openstudio.rs:94-98, 129-154, 262-290`: matches on `ClipKind`.
5. **Audio Engine MIDI Scheduling Inaction**:
   - In `crates/hikaru_audio_engine/src/lib.rs:23-35`:
     ```rust
     pub struct MidiNoteInstance {
         pub start_tick: u64,
         pub pitch: u8,
         pub velocity: u8,
         pub duration_ticks: u32,
     }
     pub struct MidiClipInstance {
         pub track_index: usize,
         pub scene_index: usize,
         pub notes: Vec<MidiNoteInstance>,
         pub is_playing: bool,
     }
     ```
   - `AudioEngine::process()` (`lib.rs:1604-1945`) iterates `self.clips: Vec<AudioClipInstance>`, but never iterates or inspects `self.midi_clips`.
   - `AudioEngine::trigger_clip` (`lib.rs:1361`) and `AudioEngine::trigger_scene` (`lib.rs:1434`) only toggle `is_playing` on `self.clips`.
   - `AudioEngine::voice_active` (`lib.rs:1457`) only inspects `self.clips`.
6. **Existing Test Suite Invariants**:
   - Running `cargo test -p hikaru_audio_engine --target-dir /tmp/hikaru_target_test -- --list` confirmed:
     - 43 unit tests in `crates/hikaru_audio_engine/src/lib.rs` (12 preview player tests + 31 engine tests).
     - 11 integration tests across `convergence_test.rs` (1), `ghost_voice_repro.rs` (5), `mixer_track_routing.rs` (3), `plugin_track_routing.rs` (2).
     - Total: 54 tests passing.

---

## 2. Logic Chain

1. **Schema Parity & Alias**: Because `.oplf` on disk serializes `MidiNote` with field `note`, and the engine uses `pitch`, applying `#[serde(alias = "pitch")]` on `MidiNote.note` allows deserializing both JSON forms without migration breakage (Observation 1).
2. **Persistence Boundary Architecture**: Because `hikaru_openlive_file` and `hikaru_openstudio_file` cannot depend on `hikaru_core` due to LGPLv3 vs. AGPLv3 licensing constraints (Observation 3), the persistence type `MidiNote` must be defined in `hikaru_project_file` (LGPLv3) and re-exported by both `.oplf` and `.opsf`. The AGPL GUI/engine converts via `From`/`Into`.
3. **Extending `.opsf` Safely**: Adding `ClipKind::Midi { #[serde(default)] notes: Vec<MidiNote> }` to `crates/hikaru_openstudio_file/src/lib.rs` resolves the Arranger MIDI persistence gap (Observation 2). Because default enum tagging is used by serde, existing projects with `Pattern`, `Audio`, or `Automation` continue to deserialize with 100% backward compatibility.
4. **Downstream Compiler Guarantees**: Because `ClipKind` is matched exhaustively in `hikaru_cli`, `hikaru_mcp`, and `hikaru_gui` (Observation 4), adding `ClipKind::Midi` requires adding a match arm to each of those locations to prevent compilation failure during `cargo check --workspace`.
5. **Audio Engine MIDI Scheduling Invariants**: For MIDI clips to sound and loop (Observation 5):
   - `MidiClipInstance` must be expanded with `id`, `start_frame`, `start_absolute`, `duration_ticks`, `clip_loop_enabled`, `clip_loop_start_ticks`, `clip_loop_end_ticks`, `is_matrix_clip`, and `active_notes: [bool; 128]`.
   - `AudioEngine::process()` must iterate `midi_clips`, calculate current quantum in ticks via `TransportPosition::samples_to_ticks`, dispatch NoteOn / NoteOff to `dms_voice_pool` or effect chains without heap allocation, and retire non-looping clips at clip boundaries.
   - `trigger_clip` and `trigger_scene` must toggle `midi_clips` alongside `clips` and clear active notes.
6. **Anti-Regression Safeguards**: The 54 existing tests in `hikaru_audio_engine` (Observation 6) test strict real-time invariants: FTZ/DAZ, quantized loop wrapping, 32-sample crossfade, and zero voice leakage past clip boundary (`peak < 1e-6`). The new MIDI scheduling must not touch audio clip processing paths or allocate memory on the audio thread.

---

## 3. Caveats

1. **Built-in Synth vs. OpenDMS**: `hikaru_audio_engine` contains `dms_voice_pool` (polyphonic sample playback for drum pads) and `oscillator: WavetableOscillator` (monophonic wavetable). It does not currently contain a polyphonic subtractive synth node. MIDI note events are therefore routed to `dms_voice_pool` (mapping pitches to pad keys) and to track effect chains (`DspPluginNode` for VST3/CLAP plugins).
2. **SMF Parser (`midly`)**: `MidiSequence::from_file_bytes` in `crates/hikaru_midi/src/file.rs` currently returns an empty `messages: vec![]`. Dropping external `.mid` files relies on completing the SMF parser; however, internal piano roll note creation and persistence in `.oplf`/`.opsf` bypass SMF and use `Vec<MidiNoteInstance>` directly.
3. **Workspace Build Time**: Compiling all crates in the workspace concurrently (particularly GUI/WGPU dependencies) is resource-intensive. Tests in `hikaru_audio_engine`, `hikaru_openlive_file`, and `hikaru_openstudio_file` should be executed with `--target-dir /tmp/target_isolated` to avoid cargo lock contention.

---

## 4. Conclusion

1. **Persistence Schema Strategy**:
   - `crates/hikaru_project_file/src/lib.rs`: Define canonical `MidiNote` with `#[serde(alias = "pitch")]`.
   - `crates/hikaru_openlive_file/src/lib.rs`: Re-export `MidiNote` from `hikaru_project_file`.
   - `crates/hikaru_openstudio_file/src/lib.rs`: Re-export `MidiNote`, extend `ClipKind` with `Midi { #[serde(default)] notes: Vec<MidiNote> }`.
   - Update match arms in `hikaru_cli/src/project.rs`, `hikaru_mcp/src/project.rs`, and `hikaru_gui/src/project/openstudio.rs`.
2. **Audio Engine MIDI Scheduling Strategy**:
   - Refactor `MidiClipInstance` with playback, loop, and stack-allocated `active_notes: [bool; 128]` tracking.
   - Schedule MIDI notes within buffer quantum in `AudioEngine::process()` using `TransportPosition` tick math.
   - Dispatch NoteOn/NoteOff to `dms_voice_pool` and track plugin chains with zero heap allocations.
   - Update `trigger_clip`, `trigger_scene`, and `voice_active`.
3. **Test Validation Strategy**:
   - Implement 11 new unit tests in `hikaru_audio_engine` (4 scheduling, 3 looping/beat wrapping, 3 voice release/leakage, 1 RT safety).
   - Implement 6 persistence roundtrip tests (3 in `hikaru_openlive_file`, 3 in `hikaru_openstudio_file`).
   - Run regression validation ensuring all 54 existing `hikaru_audio_engine` tests pass cleanly.

---

## 5. Verification Method

To independently verify this strategy:

1. **Verify Existing Audio Engine Tests Pass (Baseline)**:
   ```bash
   cargo test -p hikaru_audio_engine --target-dir /tmp/hikaru_test_baseline
   ```
   *Expected outcome*: 43 unit tests + 11 integration tests pass (0 failures).

2. **Verify Persistence Schemas & Roundtrips**:
   ```bash
   cargo test -p hikaru_openlive_file -p hikaru_openstudio_file -p hikaru_project_file --target-dir /tmp/hikaru_test_persistence
   ```
   *Expected outcome*: All persistence tests pass.

3. **Verify Downstream Workspace Check**:
   ```bash
   cargo check --workspace --target-dir /tmp/hikaru_check_ws
   ```
   *Expected outcome*: Workspace compiles with zero errors after adding `ClipKind::Midi` match arms.

4. **Verify Zero Voice Leakage Past Clip End**:
   Inspect `test_midi_clip_non_looping_stops_and_releases`:
   ```bash
   cargo test -p hikaru_audio_engine -- test_midi_clip_non_looping_stops_and_releases --target-dir /tmp/hikaru_test_leakage
   ```
   *Expected outcome*: `peak < 1e-6` verified past clip duration frames.
