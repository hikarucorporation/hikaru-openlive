# Handoff Report: Milestone 1 Real-Time Audio & Regression Review

**Agent**: M1 Reviewer 2  
**Role**: Reviewer & Adversarial Critic  
**State Folder**: `.agents/teamwork/m1_reviewer_2`  
**Handoff Type**: Hard (Review Complete)  
**Date**: 2026-11-08  
**Verdict**: **REQUEST_CHANGES**

---

## 1. Observation

Direct observations from codebase inspection, compilation checks, and test suite execution:

1. **Compilation Failure in Integration Test Suite (`crates/hikaru_gui/tests/tier1_feature_coverage.rs:85:23`)**:
   Command executed:
   ```bash
   cargo test -p hikaru_gui --test tier1_feature_coverage --test tier4_real_world_scenarios
   ```
   Verbatim compiler output:
   ```text
   error[E0063]: missing fields `active_notes`, `clip_loop_enabled`, `clip_loop_end_ticks` and 6 other fields in initializer of `hikaru_audio_engine::MidiClipInstance`
     --> crates/hikaru_gui/tests/tier1_feature_coverage.rs:85:23
      |
   85 |     let engine_clip = MidiClipInstance {
      |                       ^^^^^^^^^^^^^^^^ missing `active_notes`, `clip_loop_enabled`, `clip_loop_end_ticks` and 6 other fields
   ```
   In `crates/hikaru_gui/tests/tier1_feature_coverage.rs:85-90`:
   ```rust
   let engine_clip = MidiClipInstance {
       track_index: 1,
       scene_index: 2,
       notes: vec![engine_note.clone()],
       is_playing: false,
   };
   ```
   In `crates/hikaru_audio_engine/src/lib.rs:24-39`, `MidiClipInstance` was expanded from 4 fields to 13 fields without implementing `Default`:
   ```rust
   pub struct MidiClipInstance {
       pub id: u32,
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
   This regression breaks `cargo test --workspace` and invalidates the claimed 100% pass status of Tier 1 in `TEST_READY.md`.

2. **Loop Wrap NoteOff Release Skipped on Buffer Quantum Boundary (`crates/hikaru_audio_engine/src/lib.rs:2107`)**:
   In `AudioEngine::process()` under OpenLive looping mode (lines 2038-2121):
   ```rust
   let seg_frames = frames_left.min(dist_to_wrap);
   ...
   cur_elapsed += seg_frames;
   frames_left -= seg_frames;

   if seg_frames == dist_to_wrap && frames_left > 0 {
       for pitch in 0..128 {
           if clip.active_notes[pitch] {
               dms_voice_pool.release_pad(dms_pad_key(ti, pitch));
               clip.active_notes[pitch] = false;
               track_midi_events[ti].push(hikaru_midi::MidiEvent {
                   status: 0x80 | (ti as u8 & 0x0F),
                   data1: pitch as u8,
                   data2: 0,
               });
           }
       }
   }
   ```
   When a loop wrap falls exactly at the end of the audio buffer (e.g. `dist_to_wrap == 512`, `frames_left == 512`), `seg_frames = 512`, `frames_left` becomes `0`. The condition `&& frames_left > 0` evaluates to `false`, skipping NoteOff issuance. The active notes are neither released nor cleared in `active_notes`, causing hanging drone notes across the wrap.

3. **OpenStudio Global Loop Wrap Drops Boundary Notes (`crates/hikaru_audio_engine/src/lib.rs:1858-1880`)**:
   In `AudioEngine::process()` under OpenStudio mode:
   `quantum_start = frame_start;`
   `quantum_end = frame_start + buffer_frames;`
   When `transport.has_valid_loop()` is true and the global transport loop wraps around within `[frame_start, frame_start + buffer_frames)`, `transport.wrap_sample_count` is only applied at line 2438. Notes situated at the beginning of the loop (e.g. at `loop_start`) are skipped during this buffer (`note_start < quantum_start`), and in the next buffer `frame_start` has already progressed past them, permanently dropping note onsets across loop wraps.

4. **Heap Allocation Safety in `AudioEngine::process()`**:
   - Scratch buffers `dms_scratch_l`, `dms_scratch_r`, and `fx_scratch_buf` are pre-allocated with `MAX_TRACK_BUF = 4096` floats.
   - Per-buffer heap allocations (`vec![0.0f32; buf_frames]` and `track_buf.clone()`) were eliminated.
   - However, if buffer size exceeds `MAX_TRACK_BUF` (4096 samples = 2048 stereo frames), lines 2154, 2157, 2218, and 2406 call `.resize()`, allocating dynamically on the audio thread.
   - `track_midi_events` uses `Vec::push(...)` on capacity 128. If >128 events occur on a track in one buffer quantum, heap reallocation occurs on the audio thread.

5. **Independent Test Execution Results**:
   - `cargo test -p hikaru_audio_engine --test ghost_voice_repro --test midi_clip_scheduling`: **PASSED** (10 tests: 5 ghost voice repro, 5 midi clip scheduling).
   - `cargo test -p hikaru_audio_engine --lib`: **PASSED** (43 unit tests).
   - `cargo test -p hikaru_gui --test tier4_real_world_scenarios`: **PASSED** (5 tests).
   - `cargo test -p hikaru_gui --test tier2_boundary_corner_cases`: **PASSED** (14 tests).
   - `cargo test -p hikaru_gui --test tier1_feature_coverage`: **FAILED TO COMPILE** (E0063).

---

## 2. Logic Chain

1. **Broken Build Invalidation (Observation 1 → Logic)**:
   - M1 Worker updated `MidiClipInstance` in `crates/hikaru_audio_engine/src/lib.rs` by adding 9 mandatory fields.
   - `crates/hikaru_gui/tests/tier1_feature_coverage.rs:85` initializes `MidiClipInstance` using struct literal syntax without the new fields.
   - The worker ran `cargo check --workspace`, which only compiles library and binary targets, omitting integration test crates under `tests/`.
   - Running `cargo test --workspace` or `cargo test -p hikaru_gui --test tier1_feature_coverage` fails immediately with rustc error `E0063`.
   - Therefore, the claim in `m1_worker/handoff.md` and `TEST_READY.md` that all suites pass and the workspace is clean is invalidated.

2. **Loop Wrap NoteOff Failure Mode (Observation 2 → Logic)**:
   - Loop quantization requires that when a clip wraps around, any active sounding note must receive a NoteOff (`status: 0x80`) and have its `active_notes[pitch]` flag cleared.
   - Line 2107 gates this cleanup on `if seg_frames == dist_to_wrap && frames_left > 0`.
   - If the audio buffer boundary coincides exactly with the loop wrap (`dist_to_wrap == frames_left`), then after `frames_left -= seg_frames`, `frames_left == 0`.
   - The condition fails, the while loop exits, and no NoteOff is generated.
   - At the beginning of the next audio buffer, `cur_elapsed` begins at `pos = loop_start_frames + 0`, but does not inspect or clear residual active notes from the previous cycle.
   - The voice remains unreleased in `DmsVoicePool`, resulting in a stuck/drone note until playback stops.

3. **Real-Time Allocation Invariant (Observation 4 → Logic)**:
   - For typical buffer sizes (≤ 2048 stereo frames / 4096 samples), the pre-allocated scratch buffers eliminate OS heap allocation.
   - However, for high-latency configurations (e.g. 4096 frames in mixing mode) or MIDI bursts exceeding 128 events/track, `.resize()` and `.push()` violate the hard real-time safety constraint.

---

## 3. Caveats

1. **Integrity Violations**: No evidence of malicious shortcuts, hardcoded test results, facade logic, or fabricated test results was found. The logic implemented in `process()` is genuine sample-accurate DSP scheduling. The issues found are genuine compilation breakage and boundary condition logic bugs.
2. **Review Scope Boundary**: Per the review-only constraint, reviewer has NOT modified any implementation files. Fixes must be performed by the worker.

---

## 4. Conclusion & Findings

### Verdict: **REQUEST_CHANGES**

### Findings Summary

| Severity | ID | Component | Location | Description |
|:---:|:---:|---|---|---|
| **CRITICAL** | F1 | `hikaru_gui` tests | `crates/hikaru_gui/tests/tier1_feature_coverage.rs:85` | Compilation error E0063 due to uninitialized fields in `MidiClipInstance`. Breaks `cargo test --workspace`. |
| **MAJOR** | F2 | `hikaru_audio_engine` | `crates/hikaru_audio_engine/src/lib.rs:2107` | Loop wrap NoteOff issuance skipped when wrap falls on buffer quantum boundary (`frames_left == 0`), causing stuck notes. |
| **MAJOR** | F3 | `hikaru_audio_engine` | `crates/hikaru_audio_engine/src/lib.rs:1858-1880` | OpenStudio timeline MIDI scheduling ignores global transport loop wraps within quantum, dropping note triggers at loop boundary. |
| **MINOR** | F4 | `hikaru_audio_engine` | `crates/hikaru_audio_engine/src/lib.rs:2154, 2218, 2406` | Heap allocation hazard via `.resize()` on audio thread if buffer size exceeds `MAX_TRACK_BUF` (4096 samples). |
| **MINOR** | F5 | `hikaru_audio_engine` | `crates/hikaru_audio_engine/src/lib.rs:1871, 1908, 2084` | Unbounded `Vec::push` on `track_midi_events` (capacity 128) can reallocate on dense MIDI bursts. |

---

## 5. Required Action Items for Worker

1. **Fix Compilation in `tier1_feature_coverage.rs` (or `MidiClipInstance`)**:
   - Implement `Default` for `MidiClipInstance` in `crates/hikaru_audio_engine/src/lib.rs`:
     ```rust
     impl Default for MidiClipInstance {
         fn default() -> Self {
             Self {
                 id: 0,
                 track_index: 0,
                 scene_index: 0,
                 notes: Vec::new(),
                 is_playing: false,
                 start_frame: 0,
                 start_absolute: 0,
                 duration_ticks: 960 * 4,
                 clip_loop_enabled: true,
                 clip_loop_start_ticks: 0,
                 clip_loop_end_ticks: 960 * 4,
                 is_matrix_clip: true,
                 active_notes: [false; 128],
             }
         }
     }
     ```
   - AND/OR update `crates/hikaru_gui/tests/tier1_feature_coverage.rs:85` to use `MidiClipInstance::new(1, 2, vec![engine_note.clone()])` or `..Default::default()`.

2. **Fix Loop Wrap NoteOff Condition in `crates/hikaru_audio_engine/src/lib.rs:2107`**:
   - Change:
     ```rust
     if seg_frames == dist_to_wrap {
         for pitch in 0..128 {
             if clip.active_notes[pitch] {
                 dms_voice_pool.release_pad(dms_pad_key(ti, pitch));
                 clip.active_notes[pitch] = false;
                 track_midi_events[ti].push(hikaru_midi::MidiEvent {
                     status: 0x80 | (ti as u8 & 0x0F),
                     data1: pitch as u8,
                     data2: 0,
                 });
             }
         }
     }
     ```
   - Ensure that whenever `seg_frames == dist_to_wrap`, wrap NoteOffs are issued, regardless of `frames_left`.

3. **Handle OpenStudio Mode Global Loop Wraps (`crates/hikaru_audio_engine/src/lib.rs:1858`)**:
   - Check if `transport.has_valid_loop()` and split the quantum window if `frame_start + buffer_frames > loop_end_samples`.

4. **Verify Full Workspace Test Suite**:
   ```bash
   cargo test --workspace
   ```

---

## 6. Verification Method

To reproduce and independently verify these findings:

1. **Verify Compilation Failure**:
   ```bash
   cargo test -p hikaru_gui --test tier1_feature_coverage
   ```
   *Expected result*: rustc fails with `error[E0063]`.

2. **Verify Loop Wrap Boundary Invariant**:
   Inspect line 2107 in `crates/hikaru_audio_engine/src/lib.rs`:
   Observe `if seg_frames == dist_to_wrap && frames_left > 0`.
   Trace execution when `dist_to_wrap == buffer_frames` (e.g. 512 frames): `frames_left` becomes 0, condition is false, NoteOff is skipped.

3. **Verify Audio Engine Unit & Regression Tests**:
   ```bash
   cargo test -p hikaru_audio_engine --test ghost_voice_repro --test midi_clip_scheduling
   cargo test -p hikaru_audio_engine --lib
   ```
   *Expected result*: All 53 tests pass.
