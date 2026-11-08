# Handoff Report: Audio Engine Real-Time Scheduling Strategy (Milestone 1)

**Agent**: M1 Explorer 1 (Audio Engine Real-Time Scheduling Strategy)  
**Recipient**: Parent Coordinator (`3b10c257-464e-4b7c-926b-aebaa894c34a`) / M1 Implementer Worker  
**Date**: 2026-11-08  
**Artifact Path**: `.agents/teamwork/m1_explorer_1/handoff.md`  
**Reference Analysis**: `.agents/teamwork/m1_explorer_1/analysis.md`

---

## 1. Observation

1. **`AudioEngine::midi_clips` is Completely Omitted from the Audio Callback**:
   In `crates/hikaru_audio_engine/src/lib.rs:771`, `pub midi_clips: Vec<MidiClipInstance>` is defined on `AudioEngine`. However, in `AudioEngine::process()` (lines 1604-1945), `self.clips` is iterated and mixed (lines 1705-1873), while `self.midi_clips` is **never mentioned or iterated anywhere in `process()`**. The only method that interacts with `self.midi_clips` is `update_midi_clip()` (lines 1947-1967).
2. **`trigger_clip` and `trigger_scene` Only Touch Audio Clips**:
   In `crates/hikaru_audio_engine/src/lib.rs:1361-1395` (`trigger_clip`) and lines 1434-1455 (`trigger_scene`), the code exclusively iterates `self.clips`:
   ```rust
   // crates/hikaru_audio_engine/src/lib.rs:1376
   for clip in self.clips.iter_mut().filter(|c| c.track_index == track_index) { ... }
   ```
   Triggering a clip or scene in the Session Matrix has zero effect on `self.midi_clips`.
3. **`MidiClipInstance` Lacks Timing and Looping Metadata**:
   In `crates/hikaru_audio_engine/src/lib.rs:30-35`:
   ```rust
   pub struct MidiClipInstance {
       pub track_index: usize,
       pub scene_index: usize,
       pub notes: Vec<MidiNoteInstance>,
       pub is_playing: bool,
   }
   ```
   It completely lacks `start_frame`, `start_absolute`, `duration_ticks`, `clip_loop_enabled`, `clip_loop_start_tick`, `clip_loop_end_tick`, and playback cursors (`prev_tick`, `prev_frame`).
4. **Existing Audio Thread Allocations in `process()`**:
   - `crates/hikaru_audio_engine/src/lib.rs:1641-1642`:
     ```rust
     let mut dms_l = vec![0.0f32; buf_frames];
     let mut dms_r = vec![0.0f32; buf_frames];
     ```
     Allocates dynamic vectors on the heap in every audio callback quantum.
   - `crates/hikaru_audio_engine/src/lib.rs:1887`:
     ```rust
     let mut fx_buf = track_buf.clone();
     ```
     Clones a heap vector inside the per-track plugin insert loop.
5. **Plugin MIDI Dispatch Discarded**:
   In `crates/hikaru_audio_engine/src/lib.rs:1891`:
   ```rust
   node.process(&mut audio_buffer, &[]);
   ```
   The plugin node is passed an empty slice `&[]`, dropping any MIDI note events intended for VST3/CLAP instruments.
6. **Existing Anti-Leakage / Ghost Voice Tests Pass**:
   Running `cargo test -p hikaru_audio_engine --test ghost_voice_repro` verifies that audio clips achieve $- \infty\text{ dB}$ (0.0 sample output) past clip and loop boundaries.

---

## 2. Logic Chain

1. **Scheduling Gap (Observation 1 & 2 → Logic)**:
   Because `AudioEngine::process()` does not iterate `self.midi_clips` and `trigger_clip`/`trigger_scene` do not set `is_playing` on `midi_clips`, no MIDI notes are ever translated into sound, even if loaded and triggered from the GUI.
2. **Data Model Deficit (Observation 3 → Logic)**:
   To schedule notes in real-time, the engine must know when the clip started in absolute time (`start_absolute`), its timeline reference (`start_frame`), its loop points (`clip_loop_start_tick`, `clip_loop_end_tick`), and its duration. Without these fields, beat-quantized looping and non-looping voice termination are impossible to compute.
3. **Sub-Buffer Sample-Accurate Timing (Observation 1, 3 → Logic)**:
   In `hikaru_transport`, `DEFAULT_PPQN = 960`. A note at `note.start_tick` occurs at sample `transport.ticks_to_samples(note.start_tick)`. Inside a buffer quantum spanning $[S_{\text{start}}, S_{\text{start}} + \text{frames})$, the note's exact sub-frame offset is:
   $$\text{offset} = (\text{note\_sample} - S_{\text{start}}) \text{ as usize}$$
   This provides sample-accurate Note-On and Note-Off events to instruments.
4. **Zero Voice Leakage Invariant (Observation 3, 6 → Logic)**:
   For non-looping clips, when `elapsed_frames >= emission_len_frames(&transport)`, the clip must deactivate (`is_playing = false`) and all active notes on that track must be killed. For looping clips, when `elapsed` crosses `loop_end_frame`, any sounding note from the pre-wrap cycle must receive an immediate Note-Off at the wrap boundary, preventing infinite drones or stuck notes.
5. **Zero Allocation Invariant (Observation 4 → Logic)**:
   To prevent real-time xruns, the audio callback cannot allocate memory. Preallocating `dms_scratch_l`, `dms_scratch_r`, and `fx_scratch_buf` on `AudioEngine`, alongside a fixed-size `active_notes: [[Option<ActiveNoteState>; 16]; 16]` table and `track_midi_events: [[MidiEvent; 128]; 16]`, ensures that the entire MIDI scheduling stage runs with **zero heap allocations**.

---

## 3. Caveats

1. **No External VST3 Installed in Clean CI**: In environments without native VST3/CLAP plugins installed, plugin routing tests must verify the command/buffer contract using mock or stub descriptors (as already handled in `plugin_track_routing.rs`).
2. **GUI Piano Roll Interaction (M3/M4 Scope)**: This analysis scopes the real-time audio engine and Worker dispatch for Milestone 1. GUI-side coordinate mapping and rendering are planned for Milestones 3 and 4.
3. **File Format Crate Boundary**: `hikaru_openlive_file` and `hikaru_openstudio_file` are strictly LGPL-3.0 and must not import types from `hikaru_core` or `hikaru_audio_engine`. Format translations occur in the GUI/engine bridge.

---

## 4. Conclusion

Milestone 1 in `crates/hikaru_audio_engine` requires four coordinated implementations:
1. **Expand `MidiClipInstance`**: Add `id`, `start_frame`, `start_absolute`, `duration_ticks`, `clip_loop_enabled`, `clip_loop_start_tick`, `clip_loop_end_tick`, `prev_tick`, and `prev_frame`.
2. **Implement MIDI Scheduling in `AudioEngine::process()`**:
   - Evaluate `self.midi_clips` for both `OpenStudio` and `OpenLive` modes.
   - Detect Note-On and Note-Off events within the current buffer frame/tick window.
   - Handle beat-quantized loop wrapping by splitting the buffer at the wrap sample and issuing pre-wrap Note-Offs.
   - Dispatch to `dms_voice_pool` (OpenDMS), `DspPluginNode` (external plugins), and `SynthVoicePool` (internal polyphonic synth).
3. **Update Trigger & Stop Methods**:
   - `trigger_clip` and `trigger_scene` must toggle and reset phase on both `self.clips` and `self.midi_clips`.
   - `stop()`, `pause()`, and `stop_track()` must deactivate MIDI clips and force-kill all active notes (`AllNotesOff`).
4. **Harden Real-Time Invariants**:
   - Eliminate existing `vec![]` and `clone()` allocations in `process()` using preallocated scratch buffers.
   - Maintain FTZ/DAZ denormal zeroing and master saturation limiting.
   - Enforce zero voice leakage verified against `tests/ghost_voice_repro.rs`.

---

## 5. Verification Method

1. **Full Workspace Build & Test Run**:
   ```bash
   cargo check --workspace
   cargo test -p hikaru_audio_engine -p hikaru_transport -p hikaru_sequencer
   ```
2. **Ghost Voice Anti-Leakage Suite**:
   ```bash
   cargo test -p hikaru_audio_engine --test ghost_voice_repro
   ```
3. **Dedicated MIDI Scheduling Integration Tests** (to be added by M1 Worker in `tests/midi_clip_scheduling.rs`):
   - `test_midi_clip_playback_triggers_notes`: Verify Note-On and Note-Off events are scheduled within buffer boundaries.
   - `test_midi_clip_loop_wrap_no_stuck_notes`: Verify loop wrap emits Note-Off and prevents hanging notes.
   - `test_midi_clip_non_looping_past_emission_silent`: Verify absolute silence (0.0 peak) after non-looping clip finishes.
   - `test_midi_clip_scene_trigger_phase_restart`: Verify scene trigger restarts MIDI clip phase from sample 0.
