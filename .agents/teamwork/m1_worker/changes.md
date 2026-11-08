# Changes Report — Milestone 1 (M1 Implementer Worker)

## Summary of Code Modifications

### 1. Canonical MIDI Note Model (`crates/hikaru_core`)
- **File**: `crates/hikaru_core/src/midi_note.rs` (new file)
  - Implemented 16-byte aligned `MidiNoteInstance`:
    - `start_tick: u64` (8 bytes)
    - `pitch: u8` (1 byte)
    - `velocity: u8` (1 byte)
    - padding (2 bytes)
    - `duration_ticks: u32` (4 bytes)
  - Derives: `Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default`.
  - Added constructors `new`, `end_tick`, `is_active_at`, and tuple conversions `From<(u64, u8, u8, u32)>` and `From<MidiNoteInstance>`.
  - Added comprehensive unit tests for struct layout, 16-byte size, 8-byte alignment, sorting by tick, and tuple conversions.
- **File**: `crates/hikaru_core/src/lib.rs`
  - Re-exported `pub mod midi_note;` and `pub use midi_note::MidiNoteInstance;`.

### 2. Persistence Schemas (`hikaru_project_file`, `hikaru_openlive_file`, `hikaru_openstudio_file`)
- **File**: `crates/hikaru_project_file/src/lib.rs`
  - Defined canonical persistence struct `MidiNote` with `#[serde(alias = "pitch")] pub note: u8` to guarantee bi-directional compatibility between file formats and engine representations.
  - Added unit tests for serde deserialization supporting both `"note"` and `"pitch"`.
- **File**: `crates/hikaru_openlive_file/src/lib.rs`
  - Re-exported `MidiNote` from `hikaru_project_file`.
- **File**: `crates/hikaru_openstudio_file/src/lib.rs`
  - Re-exported `MidiNote` from `hikaru_project_file`.
  - Extended `ClipKind` with `Midi { #[serde(default)] notes: Vec<MidiNote> }`.
  - Added persistence test `un_opsf_guarda_y_lee_clips_midi`.
- **File**: `crates/hikaru_cli/src/project.rs`
  - Added match arm for `ClipKind::Midi { notes }` in summary generation.
- **File**: `crates/hikaru_mcp/src/project.rs`
  - Added match arm for `ClipKind::Midi { notes }` in MCP tool payload rendering.
- **File**: `crates/hikaru_gui/src/project/openstudio.rs`
  - Added match arm for `ClipKind::Midi` in metadata clip count capture and playlist clip restoration.

### 3. Sequencer Integration (`crates/hikaru_sequencer`)
- **File**: `crates/hikaru_sequencer/src/clip.rs`
  - Implemented `ClipContentType { Audio, Midi(ClipMidiTimeline) }`.
  - Implemented `ClipMidiTimeline` with note vector sorting, `add_note`, `duration_ticks`.
  - Extended `Clip` with `content_type: ClipContentType`.
  - Added constructors `new_midi`, helper inspection methods `is_midi()`, `is_audio()`, `midi_timeline()`, and `midi_timeline_mut()`.
  - Added unit test `midi_clip_timeline_integration`.

### 4. Audio Engine Real-Time Scheduling (`crates/hikaru_audio_engine`)
- **File**: `crates/hikaru_audio_engine/src/lib.rs`
  - Re-exported canonical `MidiNoteInstance` from `hikaru_core`.
  - Expanded `MidiClipInstance` with `id`, `start_frame`, `start_absolute`, `duration_ticks`, `clip_loop_enabled`, `clip_loop_start_ticks`, `clip_loop_end_ticks`, `is_matrix_clip`, `active_notes: [bool; 128]`.
  - Pre-allocated audio callback scratch buffers on `AudioEngine`: `dms_scratch_l`, `dms_scratch_r`, `fx_scratch_buf`, and `track_midi_events: Vec<Vec<MidiEvent>>`.
  - Replaced heap allocations (`vec![0.0f32; buf_frames]` and `track_buf.clone()`) in `AudioEngine::process()` with pre-allocated scratch buffers.
  - Implemented real-time sample-accurate MIDI scheduling in `AudioEngine::process()`:
    - Handled `OpenStudio` timeline clip scheduling.
    - Handled `OpenLive` session matrix clip scheduling.
    - Handled beat-quantized loop wrapping with pre-wrap NoteOff issuance to eliminate hanging/stuck notes.
    - Handled non-looping clip deactivation past emission length and killed active notes to guarantee $-\infty\text{ dB}$ (0.0 sample output) zero voice leakage.
    - Dispatched NoteOn and NoteOff events to `DmsVoicePool` and track plugin chains (`DspPluginNode`).
  - Updated `trigger_clip`, `trigger_scene`, `stop`, `pause`, `stop_track`, `voice_active`, `voice_elapsed_frames`, `voice_playhead_frame`, and `update_midi_clip`.
- **File**: `crates/hikaru_audio_engine/tests/midi_clip_scheduling.rs` (new file)
  - Added 5 integration tests:
    - `test_midi_clip_playback_triggers_notes`
    - `test_midi_clip_non_looping_past_emission_silent`
    - `test_midi_clip_loop_wrap_no_stuck_notes`
    - `test_midi_clip_scene_trigger_phase_restart`
    - `test_midi_clip_openstudio_mode_scheduling`
