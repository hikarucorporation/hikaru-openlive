# Handoff Report: Engine & Data Architecture Survey

## 1. Observation

1. **Clip & Event Data Structures**:
   - `crates/hikaru_core/src/audio_clip_event.rs:35-60`: `AudioClipEvent` defines the audio event model with fields `id: u64`, `sample_buffer: Arc<Vec<f32>>`, `channels: usize`, `sample_rate: u32`, `start_frame: u64`, `end_frame: u64`, `loop_start: u64`, `loop_end: u64`, `loop_enabled: bool`, `gain_db: f32`, `fade_in_secs: f64`, `fade_out_secs: f64`, `stretch_mode: StretchMode`.
   - `crates/hikaru_sequencer/src/clip.rs:57-72`: `ClipAudioEvent` defines audio sub-regions within a pad (`clip_start`, `source_offset`, `length`, `gain`, `fade_in`, `fade_out`), managed by `ClipAudioTimeline`.
   - `crates/hikaru_sequencer/src/clip.rs:332-350`: `Clip` defines sequencer playback state with `AtomicU8` (`Stopped = 0`, `Queued = 1`, `Playing = 2`, `Stopping = 3`), `loop_start`, `loop_end`, `loop_enabled`, `total_frames`.
   - `crates/hikaru_audio_engine/src/lib.rs:23-35`:
     ```rust
     #[derive(Clone, Debug)]
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
   - `crates/hikaru_audio_engine/src/lib.rs:150-172`: `AudioClipInstance` represents active audio clips with `start_frame: u64`, `start_absolute: u64`, `duration_frames: u64`, `is_matrix_clip: bool`, `clip_loop_enabled: bool`, `events: Vec<EngineAudioEvent>`, `xfade_remaining: u32`.

2. **MIDI Scheduling & Audio Engine Processing**:
   - `crates/hikaru_audio_engine/src/lib.rs:771`: `AudioEngine` contains `pub midi_clips: Vec<MidiClipInstance>`.
   - `crates/hikaru_audio_engine/src/lib.rs:1604-1945`: In `AudioEngine::process(&mut self, out_buffer: &mut AudioBuffer<'_>)`, only `self.clips` (audio) are iterated (`for clip in self.clips.iter_mut()`, lines 1705-1873) and mixed. `self.midi_clips` is **never read, iterated, or scheduled** during audio processing.
   - `crates/hikaru_audio_engine/src/lib.rs:1361-1395`: `trigger_clip` only matches and toggles `self.clips`, leaving `self.midi_clips` unactivated.
   - `crates/hikaru_dsp/src/plugin_node.rs:49-55`: `DspPluginNode::process(&mut self, buffer: &mut AudioBuffer, _midi_events: &[MidiEvent])` discards `_midi_events` and only calls `self.instance.process(buffer)`.
   - `crates/hikaru_plugin_host/src/lib.rs:179-189`: `pub trait PluginInstance: Send` defines `fn process(&mut self, buffer: &mut AudioBuffer)` without MIDI inputs.

3. **Clock Authority & Conversions**:
   - `crates/hikaru_transport/src/transport_state.rs:8`: `pub const DEFAULT_PPQN: u64 = 960;` is the canonical tick resolution.
   - `crates/hikaru_transport/src/transport_state.rs:101-136`: Provides bidirectional conversions: `ticks_to_samples` and `samples_to_ticks`.
   - `crates/hikaru_gui/src/views/piano_roll.rs:184-185`: `pub const TICKS_PER_BEAT: u64 = 960;` and `pub const TICKS_PER_BAR: u64 = 3840;` match `hikaru_transport`.

4. **Persistence Formats**:
   - `crates/hikaru_openlive_file/src/lib.rs:223-266`: `.oplf` format persists `Clip` with `events: Vec<Event>` and `midi_notes: Vec<MidiNote>`, where `MidiNote` has `start_tick: u64`, `note: u8`, `velocity: u8`, `duration_ticks: u32`.
   - `crates/hikaru_openstudio_file/src/lib.rs:157-171`: `.opsf` format has `ClipKind::Pattern { pattern_id: usize }` without note storage.

5. **Test Execution**:
   - `cargo test -p hikaru_sequencer`: 14 tests passed in 0.01s.
   - `cargo test -p hikaru_transport`: 3 tests passed in 0.00s.
   - `cargo test -p hikaru_project_file -p hikaru_openlive_file -p hikaru_openstudio_file`: 10 tests passed in 0.02s.
   - `cargo test -p hikaru_audio_engine --lib`: 43 tests passed in 3.66s.
   - `cargo test -p hikaru_audio_engine --test ghost_voice_repro`: 5 tests passed in 0.02s.

---

## 2. Logic Chain

1. **Finding on MIDI Execution**:
   - Observation 1.2 shows `AudioEngine::process()` only iterates `self.clips` and dispatches no MIDI from `self.midi_clips`.
   - Observation 1.2 shows `trigger_clip()` toggles `self.clips` but never `self.midi_clips`.
   - Observation 1.2 shows `DspPluginNode::process()` discards `_midi_events`.
   - *Therefore*, any MIDI notes inserted into `MidiClipInstance` or sent via `GuiCommand::UpdateMidiClipNotes` remain inert in the audio engine callback. For MIDI clips to sound, `AudioEngine::process()` must schedule note events to a synthesis target (OpenDMS drum pads, an instrument synthesizer, or external VST3/CLAP plugins).

2. **Finding on Data Representation**:
   - Observation 1.1 and 1.4 show 4 separate `MidiNote` structs (`hikaru_audio_engine::MidiNoteInstance`, `hikaru_gui::audio_proxy::MidiNoteInstance`, `hikaru_gui::piano_roll::MidiNote`, `hikaru_openlive_file::MidiNote`).
   - *Therefore*, data must be converted between layers unless unified in `hikaru_core` or `hikaru_midi`. The field semantics are identical: `start_tick: u64`, `pitch/note: u8`, `velocity: u8`, `duration_ticks: u32/u64`.

3. **Finding on Timeline Arranger Gap**:
   - Observation 1.4 shows `.opsf` only stores `pattern_id: usize` for `ClipKind::Pattern`.
   - Observation 1.1 shows `PlaylistClip` in `hikaru_gui` holds `ClipType::Pattern { pattern_id }` without notes.
   - *Therefore*, supporting MIDI clips on the Arranger Timeline requires extending `ClipType` and `ClipKind` to store note vectors, or establishing a pattern-to-notes lookup table.

4. **Finding on Real-Time Glitch Safety**:
   - Observation 1.5 and Observation 1.2 show the audio engine has strict regression tests for zero-leakage (`ghost_voice_repro`), FTZ/DAZ denormal flushing, and quantized beat loops.
   - *Therefore*, introducing MIDI scheduling into `AudioEngine::process()` must strictly avoid memory allocation, locks, or denormal hazards in the real-time path.

---

## 3. Caveats

1. External plugin (VST3/CLAP) MIDI event reception is currently unsupported by `hikaru_plugin_host::PluginInstance`. Adding MIDI to VST3/CLAP requires updating `PluginInstance::process` or adding `process_with_events`.
2. Hardware MIDI input via `hikaru_midi::device::HardwareController` was not tested against live USB hardware in this read-only survey.
3. No code changes were implemented in source files, in strict accordance with the explorer read-only constraint.

---

## 4. Conclusion

1. Hikaru OpenLive's audio engine is robust, thoroughly tested (43 unit + 8 integration tests passing), and enforces clean RT anti-glitch invariants.
2. The fundamental block preventing MIDI playback is the disconnection in `AudioEngine::process()`: `self.midi_clips` is maintained but never scheduled, and `trigger_clip` does not toggle MIDI clip playback.
3. The `.oplf` format is ready for MIDI clips, while `.opsf` requires schema extension for Arranger pattern notes.
4. The Piano Roll UI in `hikaru_gui` already has basic coordinate math, zoom, and scrolling, but requires velocity lane rendering, drag/resize/draw mouse interactions, and automatic linkage when clips are double-clicked.

---

## 5. Verification Method

To independently verify the observations and findings in this report:

1. **Verify sequencer & transport logic**:
   ```bash
   cargo test -p hikaru_sequencer -p hikaru_transport --target-dir /tmp/hikaru_verify_test
   ```
2. **Verify audio engine regression tests (silence past clip end, quantized loops, routing)**:
   ```bash
   cargo test -p hikaru_audio_engine --lib --test ghost_voice_repro --test mixer_track_routing --target-dir /tmp/hikaru_verify_test
   ```
3. **Verify project file persistence schemas**:
   ```bash
   cargo test -p hikaru_project_file -p hikaru_openlive_file -p hikaru_openstudio_file --target-dir /tmp/hikaru_verify_test
   ```
4. **Inspect code references**:
   - Inspect `crates/hikaru_audio_engine/src/lib.rs:1604-1873` to confirm `self.midi_clips` is never iterated in `process()`.
   - Inspect `crates/hikaru_dsp/src/plugin_node.rs:49-55` to confirm `_midi_events` is unused.
   - Inspect `crates/hikaru_openstudio_file/src/lib.rs:157-171` to confirm `ClipKind::Pattern` lacks notes.
