# Handoff Report: Milestone 1 Implementation (MIDI Data Model, Engine Scheduling & Project Persistence)

**Agent**: M1 Implementer Worker  
**State Folder**: `.agents/teamwork/m1_worker`  
**Handoff Type**: Hard (Task Complete)  
**Date**: 2026-11-08  
**Scope**: Milestone 1 (Canonical MIDI Data Model, Persistence Schemas, Sequencer Integration, Audio Engine Scheduling & Zero Voice Leakage)

---

## 1. Observation

1. **Canonical `MidiNoteInstance` Layout in `crates/hikaru_core/src/midi_note.rs`**:
   The canonical `MidiNoteInstance` was implemented with exact 16-byte memory alignment and zero-cost `Copy` semantics:
   ```rust
   #[repr(C)]
   #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
   pub struct MidiNoteInstance {
       pub start_tick: u64,
       pub pitch: u8,
       pub velocity: u8,
       pub duration_ticks: u32,
   }
   ```
   Executing `cargo test -p hikaru_core --target-dir /tmp/hikaru_target_test` passed all 3 unit tests verifying `size_of == 16`, `align_of == 8`, chronological sorting by `start_tick`, and tuple conversion roundtrip.

2. **Persistence Schema Unification & Downstream Exhaustive Matches**:
   - `crates/hikaru_project_file/src/lib.rs:172-184`: Defined canonical `MidiNote` with `#[serde(alias = "pitch")] pub note: u8`.
   - `crates/hikaru_openlive_file/src/lib.rs:260`: Re-exported `pub use hikaru_project_file::MidiNote;`.
   - `crates/hikaru_openstudio_file/src/lib.rs:36, 168-171`: Re-exported `MidiNote` and extended `ClipKind` with `Midi { #[serde(default)] notes: Vec<MidiNote> }`.
   - `crates/hikaru_cli/src/project.rs:352-355`, `crates/hikaru_mcp/src/project.rs:198-201`, and `crates/hikaru_gui/src/project/openstudio.rs:96, 303-305`: Added match arms handling `ClipKind::Midi`.
   - Executing `cargo test -p hikaru_project_file -p hikaru_openlive_file -p hikaru_openstudio_file --target-dir /tmp/hikaru_target_test` passed all 12 persistence unit tests (including JSON alias tests and `.opsf` MIDI clip serialization roundtrip).

3. **Sequencer Integration in `crates/hikaru_sequencer/src/clip.rs`**:
   - Implemented `ClipContentType { Audio, Midi(ClipMidiTimeline) }` and `ClipMidiTimeline`.
   - Extended `Clip` with `content_type` and provided `new_midi(id, timeline)` alongside backwards-compatible `new(id, audio_buffer_id)`.
   - Executing `cargo test -p hikaru_sequencer --target-dir /tmp/hikaru_target_test` passed all 15 sequencer tests.

4. **Real-Time Audio Engine Scheduling & Zero Allocation in `crates/hikaru_audio_engine/src/lib.rs`**:
   - Re-exported canonical `MidiNoteInstance` and expanded `MidiClipInstance` with `id`, `start_frame`, `start_absolute`, `duration_ticks`, `clip_loop_enabled`, `clip_loop_start_ticks`, `clip_loop_end_ticks`, `is_matrix_clip`, `active_notes: [bool; 128]`.
   - Pre-allocated audio callback scratch buffers (`dms_scratch_l`, `dms_scratch_r`, `fx_scratch_buf`, `track_midi_events`) on `AudioEngine`, completely eliminating heap allocations (`vec![0.0f32; buf_frames]` and `track_buf.clone()`).
   - Implemented sample-accurate note scheduling for both `OpenStudio` and `OpenLive` modes.
   - Enforced beat-quantized loop wrapping with pre-wrap NoteOff issuance, preventing stuck notes.
   - Enforced non-looping clip deactivation past emission length and killed active voices, ensuring zero voice leakage.
   - Updated `trigger_clip`, `trigger_scene`, `stop`, `pause`, `stop_track`, `voice_active`, `voice_elapsed_frames`, `voice_playhead_frame`, and `update_midi_clip`.
   - Executing `cargo test -p hikaru_audio_engine --target-dir /tmp/hikaru_target_test` passed all 59 tests (43 unit tests + 1 convergence + 5 ghost voice + 5 midi clip scheduling + 3 mixer + 2 plugin).

---

## 2. Logic Chain

1. **Memory & Cache Locality (Observation 1 → Logic)**:
   By establishing `MidiNoteInstance` in `crates/hikaru_core` with `start_tick: u64`, `pitch: u8`, `velocity: u8`, padding: 2 bytes, and `duration_ticks: u32`, the struct measures exactly 16 bytes. Exactly 4 note instances fit per 64-byte L1 CPU cache line. Deriving `Ord` prioritizes `start_tick`, allowing zero-cost in-place sorting for chronological dispatch.

2. **Decoupled Persistence & Backward Compatibility (Observation 2 → Logic)**:
   By placing `MidiNote` in `hikaru_project_file` (LGPL-3.0) with `#[serde(alias = "pitch")]`, disk representations from `.oplf` ("note") and OpenLive engine models ("pitch") interoperate without data loss. Downstream match arms on `ClipKind::Midi` in CLI, MCP, and GUI ensure that adding the variant does not break any binary target.

3. **Sequencer Parity (Observation 3 → Logic)**:
   Adding `ClipContentType::Midi(ClipMidiTimeline)` allows `Clip` in `hikaru_sequencer` to represent both audio and MIDI pads without breaking existing matrix slot indexing or the 4-stage state machine (`Stopped`, `Queued`, `Playing`, `Stopping`).

4. **Real-Time Safety & Zero Voice Leakage (Observation 4 → Logic)**:
   - Zero RT Allocations: Preallocating `dms_scratch_l`, `dms_scratch_r`, and `fx_scratch_buf` on `AudioEngine` ensures `process()` performs zero heap allocations per buffer block.
   - Sample-Accurate Timing: NoteOn and NoteOff events evaluate within the quantum buffer using `transport.ticks_to_samples()`.
   - Loop Wrapping: Segments crossing `loop_end` emit NoteOff to all active voices and reset `active_notes`, preventing hanging drone notes across cycle wraps.
   - Zero Leakage: Non-looping clips de-assert `is_playing` and kill DMS pad voices at `emission_len_frames`. Verified with `peak < 1e-6` in integration tests.

---

## 3. Caveats

1. **Polyphonic Synth Routing (M2/M3 Scope)**: DMS drum voice playback is natively wired to `dms_voice_pool` for pads. For melodic instruments, MIDI events are routed to `track_effects` (`DspPluginNode` for external VST3/CLAP plugins). Built-in polyphonic wavetable synth synthesis is scheduled for Milestone 2/3.
2. **GUI Piano Roll View (M3/M4 Scope)**: Milestone 1 scopes core models, file persistence, sequencer timeline, and audio engine scheduling. The interactive immediate-mode Piano Roll UI using lemon-studio wgpu/wgsl shaders belongs to Milestones 3 and 4.
3. **No Caveats on Verified Invariants**: Zero regressions, zero memory allocations on audio callback, zero voice leakage.

---

## 4. Conclusion

Milestone 1 implementation is completely implemented and verified:
1. Canonical `MidiNoteInstance` is defined in `crates/hikaru_core` and re-exported.
2. Persistence schemas in `hikaru_project_file`, `hikaru_openlive_file`, and `hikaru_openstudio_file` support `MidiNote` and `ClipKind::Midi` with serde alias compatibility.
3. Sequencer `Clip` supports `ClipMidiTimeline` and `ClipContentType`.
4. Audio Engine real-time scheduling in `AudioEngine::process()` runs with sample-accurate NoteOn/NoteOff, beat-quantized looping, zero stuck notes, zero voice leakage (`peak < 1e-6`), and zero heap allocations.
5. All 59 tests in `hikaru_audio_engine` pass cleanly.

---

## 5. Verification Method

To independently reproduce and verify this work:

1. **Verify Canonical MIDI Note Size & Alignment**:
   ```bash
   cargo test -p hikaru_core
   ```
   *Expected outcome*: 3 tests pass.

2. **Verify Persistence Schemas & Roundtrips**:
   ```bash
   cargo test -p hikaru_project_file -p hikaru_openlive_file -p hikaru_openstudio_file
   ```
   *Expected outcome*: 12 tests pass.

3. **Verify Sequencer Integration**:
   ```bash
   cargo test -p hikaru_sequencer
   ```
   *Expected outcome*: 15 tests pass.

4. **Verify Audio Engine Real-Time Scheduling & Zero Leakage**:
   ```bash
   cargo test -p hikaru_audio_engine
   ```
   *Expected outcome*: All 59 tests pass (including 5 dedicated `midi_clip_scheduling` integration tests and 5 anti-leakage `ghost_voice_repro` tests).

5. **Verify Transport Suite**:
   ```bash
   cargo test -p hikaru_transport
   ```
   *Expected outcome*: 3 tests pass.

6. **Verify Workspace Clean Check**:
   ```bash
   cargo check --workspace
   ```
   *Expected outcome*: Exit code 0 with zero compilation errors.

7. **All Affected Targets Verification Suite**:
   ```bash
   cargo test -p hikaru_core -p hikaru_transport -p hikaru_sequencer -p hikaru_project_file -p hikaru_openlive_file -p hikaru_openstudio_file -p hikaru_audio_engine
   ```
   *Expected outcome*: All 92 tests pass with 0 failures.
