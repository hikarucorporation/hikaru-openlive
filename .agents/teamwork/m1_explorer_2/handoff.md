# Handoff Report: Data Structures, Sequencer & Crate Unification Strategy

**Explorer**: M1 Explorer 2  
**Date**: 2026-11-08  
**Working Directory**: `/home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive`  
**State Folder**: `/home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/.agents/teamwork/m1_explorer_2`  
**Target Milestone**: M1 Architecture Alignment & Deep-Dive Exploration

---

## 1. Observation

Direct observations and evidence from code analysis:

1. **Fragmentation of MIDI Note Representations**:
   - `crates/hikaru_core`: Contains `audio_buffer.rs` and `audio_clip_event.rs`, but contains zero MIDI note or clip definitions.
   - `crates/hikaru_audio_engine/src/lib.rs:22-28`:
     ```rust
     #[derive(Clone, Debug)]
     pub struct MidiNoteInstance {
         pub start_tick: u64,
         pub pitch: u8,
         pub velocity: u8,
         pub duration_ticks: u32,
     }
     ```
   - `crates/hikaru_gui/src/audio_proxy.rs:8-14`:
     ```rust
     #[derive(Clone, Debug)]
     pub struct MidiNoteInstance {
         pub start_tick: u64,
         pub pitch: u8,
         pub velocity: u8,
         pub duration_ticks: u32,
     }
     ```
   - `crates/hikaru_gui/src/views/piano_roll.rs:23-29`:
     ```rust
     #[derive(Debug, Clone)]
     pub struct MidiNote {
         pub pitch: u8,
         pub start_tick: u64,
         pub duration_ticks: u64,
         pub velocity: u8,
     }
     ```
   - `crates/hikaru_gui/src/views/matrix.rs:173-175`:
     ```rust
     ClipData::Midi {
         notes: Vec<(u64, u8, u8, u32)>,
     }
     ```
   - `crates/hikaru_openlive_file/src/lib.rs:260-266`:
     ```rust
     #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
     pub struct MidiNote {
         pub start_tick: u64,
         pub note: u8,
         pub velocity: u8,
         pub duration_ticks: u32,
     }
     ```

2. **Sequencer Audio-Only Clip and Matrix Model**:
   - `crates/hikaru_sequencer/src/clip.rs:332-350`:
     ```rust
     pub struct Clip {
         pub id: u32,
         pub audio_buffer_id: u32,
         pub state: AtomicU8,
         pub trigger_mode: TriggerMode,
         pub start_sample: u64,
         pub loop_enabled: bool,
         pub loop_start: u64,
         pub loop_end: u64,
         pub total_frames: u64,
     }
     ```
   - `crates/hikaru_sequencer/src/matrix.rs:8-12`:
     ```rust
     pub const MAX_TRACKS: usize = 16;
     pub const MAX_SCENES: usize = 32;
     pub struct TrackMatrix {
         pub slots: [[Option<Clip>; MAX_SCENES]; MAX_TRACKS],
     }
     ```
   - There is no `ClipMidiTimeline` or MIDI content type in `hikaru_sequencer`.

3. **Audio Engine Ignores MIDI Clips in Playback Callback**:
   - In `crates/hikaru_audio_engine/src/lib.rs:1604-1945`, `AudioEngine::process()` iterates `self.clips` (audio clips) at lines 1705-1873, but `self.midi_clips` is never iterated.
   - In `crates/hikaru_audio_engine/src/lib.rs:1361-1395`, `trigger_clip(track_index, scene_index)` only checks and mutates `self.clips`, ignoring `self.midi_clips`.
   - In `crates/hikaru_audio_engine/src/lib.rs:1434-1455`, `trigger_scene(scene_index)` only operates on `self.clips`.
   - In `crates/hikaru_audio_engine/src/lib.rs:1457-1470`, `voice_active(track_index, scene_index)` only checks `self.clips`.

4. **Transport Math Evaluation in `transport_state.rs`**:
   - In `crates/hikaru_transport/src/transport_state.rs:101-136`:
     ```rust
     pub fn ticks_to_samples(&self, ticks: u64) -> u64 {
         let sr = self.sample_rate.get() as f64;
         if sr <= 0.0 { return 0; }
         (ticks as f64 * self.seconds_per_tick() * sr).round() as u64
     }

     pub fn samples_to_ticks(&self, samples: u64) -> u64 {
         let sr = self.sample_rate.get() as f64;
         if sr <= 0.0 { return 0; }
         let seconds = samples as f64 / sr;
         let spt = self.seconds_per_tick();
         if spt <= 0.0 { return 0; }
         (seconds / spt).round() as u64
     }
     ```
   - Conversions evaluate from absolute timeline coordinates, not incremental step accumulation.

---

## 2. Logic Chain

1. **From Observation 1 to Canonical Data Model**:
   - Since `hikaru_audio_engine` and `hikaru_gui::audio_proxy` both defined `MidiNoteInstance` independently, and `piano_roll.rs` defined a divergent `MidiNote` with `duration_ticks: u64`, code must bridge types with manual mappings in `main.rs:337-353`.
   - Defining `MidiNoteInstance` in `crates/hikaru_core/src/midi_note.rs` establishes a single source of truth across all engine and UI layers.
   - Using `duration_ticks: u32` allows `MidiNoteInstance` to occupy exactly 16 bytes (8 bytes `start_tick`, 1 byte `pitch`, 1 byte `velocity`, 2 bytes padding, 4 bytes `duration_ticks`). 4 instances fit evenly in a 64-byte L1 CPU cache line.
   - At 960 PPQN, `u32` accommodates over 1.1 million bars ($4.29 \times 10^9$ ticks), which is more than sufficient for any musical duration.
   - Deriving `Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default` provides zero-cost value passing and automatic chronological sorting by `start_tick`.

2. **From Observation 2 to Sequencer Unification**:
   - In `crates/hikaru_sequencer/src/clip.rs`, `Clip` cannot currently manage MIDI clips because it lacks tick-based timing and a MIDI container.
   - Introducing `ClipMidiTimeline` with `notes: Vec<MidiNoteInstance>` and `ClipContentType { Audio, Midi }` allows `Clip` to represent both Audio and MIDI clips while preserving the existing 4-state state machine (`Stopped`, `Queued`, `Playing`, `Stopping`) and voice polling (`VoiceState::Finished`).
   - In `quantizer.rs`, `QuantizationEngine::samples_until_next_sync` hardcodes `QuantizationGrid::Bar => 4.0` instead of respecting `current_pos.beats_per_bar`. Adding `ticks_until_next_sync` using integer PPQN arithmetic ensures zero floating-point remainder jitter.

3. **From Observation 3 to Real-Time Engine Scheduling**:
   - Because `AudioEngine::process()` does not iterate `self.midi_clips`, MIDI notes loaded into the engine never produce sound.
   - In `AudioEngine::process()`, each audio quantum spans a sample range `[frame_start, frame_start + buffer_frames)`. Converting this window to `[tick_start, tick_end)` via `transport.samples_to_ticks` allows scheduled detection of NoteOn and NoteOff events.
   - Sub-buffer frame offsets can be computed with sample precision using `ticks_to_samples(note.start_tick).saturating_sub(elapsed_start)`, dispatching NoteOn to `dms_voice_pool` (for OpenDMS tracks) or `DspPluginNode` (for synth/plugin tracks) without audio-thread memory allocations.

4. **From Observation 4 to Clock Mathematics Soundness**:
   - Cumulative drift only occurs when a clock adds incremental rounded deltas ($S_{N} = S_{N-1} + \Delta$).
   - Because `ticks_to_samples` and `samples_to_ticks` evaluate directly as closed-form functions of the global timeline counter, rounding error is bounded to $\pm 0.5$ samples across arbitrary playback durations.
   - Rewriting the conversions into a single-division formulation eliminates intermediate ULP truncation.

---

## 3. Caveats

1. **Licensing Boundary with Persistence Crates**:
   `crates/hikaru_openlive_file` and `crates/hikaru_openstudio_file` are LGPLv3 and intentionally do not import from `hikaru_core`. Their internal `MidiNote` structs (`note: u8` vs `pitch: u8`) must remain decoupled; translation is handled by `crates/hikaru_gui/src/project/openlive.rs`.
2. **Built-in Polyphonic Synthesizer Node**:
   While OpenDMS drum playback is natively supported via `dms_voice_pool.trigger()`, polyphonic melodic synthesis on non-drum tracks requires routing MIDI events to hosted VST3/CLAP plugins or `OpenWavetable` (planned in M2/M3).
3. **Studio Arranger Pattern Persistence**:
   In `crates/hikaru_openstudio_file`, `ClipKind::Pattern` currently stores only `pattern_id: usize`. Pattern note sequences for linear arranger timeline clips will require extending `ClipKind` with an optional notes payload.

---

## 4. Conclusion

1. **Data Model**: The canonical `MidiNoteInstance` must be placed in `crates/hikaru_core/src/midi_note.rs` with `start_tick: u64`, `pitch: u8`, `velocity: u8`, `duration_ticks: u32`, 16-byte alignment, and `Copy` semantics.
2. **Sequencer**: `crates/hikaru_sequencer/src/clip.rs` should be extended with `ClipMidiTimeline` and `ClipContentType` to track MIDI clips alongside audio clips with identical quantized triggering and loop bounds.
3. **Transport**: `crates/hikaru_transport/src/transport_state.rs` is mathematically sound with zero cumulative drift; a single-division optimization will further minimize roundoff noise.
4. **Engine**: `crates/hikaru_audio_engine/src/lib.rs` must expand `MidiClipInstance` and implement real-time sample-accurate MIDI note scheduling in `AudioEngine::process()` and toggle `midi_clips` in `trigger_clip()` / `trigger_scene()`.

---

## 5. Verification Method

To independently verify the architecture and recommendations:

1. **Baseline Suite Verification (Executed & Confirmed)**:
   - Command: `cargo test -p hikaru_audio_engine --target-dir /tmp/hikaru_test_m1_exp2`
   - Result: **54 tests passed; 0 failed** across `src/lib.rs` (43), `ghost_voice_repro.rs` (5), `mixer_track_routing.rs` (3), `plugin_track_routing.rs` (2), `convergence_test.rs` (1).
2. **Inspect Data Layout**:
   Verify `MidiNoteInstance` size and alignment:
   ```bash
   cargo test -p hikaru_core -- test_midi_note_instance_size_and_alignment
   ```
3. **Inspect Transport Clock Zero-Drift**:
   ```bash
   cargo test -p hikaru_transport -- test_transport_clock_zero_drift
   ```
4. **Verify Workspace Compilation**:
   ```bash
   cargo check --workspace
   ```
5. **Run Sequencer & Audio Engine Suites**:
   ```bash
   cargo test -p hikaru_sequencer
   cargo test -p hikaru_audio_engine --target-dir /tmp/hikaru_test_verify
   ```
6. **Invalidation Conditions**:
   - If any `MidiNoteInstance` field sizes change (e.g. changing `duration_ticks` to `u64`), the 16-byte alignment and cache efficiency claims are invalidated.
   - If `AudioEngine::process()` performs heap allocations (`Vec::new()`) in the MIDI loop, the RT-safety claim is invalidated.
