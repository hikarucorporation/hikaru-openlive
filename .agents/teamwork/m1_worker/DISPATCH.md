# Task Assignment: M1 Implementer Worker

## Working Directory
/home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive

## State Folder
/home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/.agents/teamwork/m1_worker

## Scope Document
/home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/PROJECT.md

## Explorer Reports (Mandatory Reading)
1. `/home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/.agents/teamwork/m1_explorer_1/handoff.md` (and `analysis.md`)
2. `/home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/.agents/teamwork/m1_explorer_2/handoff.md` (and `analysis.md`)
3. `/home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/.agents/teamwork/m1_explorer_3/handoff.md` (and `analysis.md`)

## Exclusive Write Ownership
You exclusively own and may edit:
- `crates/hikaru_core/src/midi_note.rs` (new)
- `crates/hikaru_core/src/lib.rs`
- `crates/hikaru_project_file/src/lib.rs`
- `crates/hikaru_openlive_file/src/lib.rs`
- `crates/hikaru_openstudio_file/src/lib.rs`
- `crates/hikaru_cli/src/project.rs`
- `crates/hikaru_mcp/src/project.rs`
- `crates/hikaru_gui/src/project/openstudio.rs`
- `crates/hikaru_sequencer/src/clip.rs`
- `crates/hikaru_audio_engine/src/lib.rs`
- `crates/hikaru_audio_engine/tests/`

Do NOT touch files owned by other tracks (e.g. `crates/hikaru_gui/tests/tier*.rs`).

## Implementation Tasks for Milestone 1

### 1. Canonical MIDI Note Model (`crates/hikaru_core/src/midi_note.rs`)
Implement the 16-byte canonical `MidiNoteInstance`:
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
pub struct MidiNoteInstance {
    pub start_tick: u64,
    pub pitch: u8,
    pub velocity: u8,
    pub duration_ticks: u32,
}
```
Provide tuple conversion traits and re-export from `hikaru_core`.

### 2. Persistence Schemas (`hikaru_project_file`, `hikaru_openlive_file`, `hikaru_openstudio_file`)
- In `crates/hikaru_project_file/src/lib.rs`, define `MidiNote` with `#[serde(alias = "pitch")] pub note: u8`.
- In `crates/hikaru_openlive_file/src/lib.rs`, re-export `MidiNote`.
- In `crates/hikaru_openstudio_file/src/lib.rs`, re-export `MidiNote` and add `ClipKind::Midi { #[serde(default)] notes: Vec<MidiNote> }`.
- Add match arms for `ClipKind::Midi` in `hikaru_cli/src/project.rs`, `hikaru_mcp/src/project.rs`, and `hikaru_gui/src/project/openstudio.rs` to ensure `cargo check --workspace` compiles cleanly.

### 3. Sequencer Integration (`crates/hikaru_sequencer/src/clip.rs`)
- Add `ClipMidiTimeline` and support `ClipContentType { Audio, Midi }` on `Clip`.
- Ensure quantized triggering, loop start/end, and playback state machine apply cleanly to MIDI clips.

### 4. Audio Engine Real-Time Scheduling (`crates/hikaru_audio_engine/src/lib.rs`)
- Expand `MidiClipInstance` with timing and loop bounds: `id`, `start_frame`, `start_absolute`, `duration_ticks`, `clip_loop_enabled`, `clip_loop_start_ticks`, `clip_loop_end_ticks`, `is_matrix_clip`, `active_notes: [bool; 128]`.
- In `AudioEngine::process()`:
  - Iterate `self.midi_clips`.
  - Calculate quantum tick window via `TransportPosition`.
  - Schedule NoteOn and NoteOff events with sample accuracy.
  - Handle loop wrapping (issue NoteOff on wrap to prevent stuck notes).
  - Deactivate non-looping clips at emission boundary and kill active notes (guaranteeing zero voice leakage).
  - Eliminate existing RT heap allocations (`vec![0.0; buf_frames]` and `fx_buf.clone()`) by pre-allocating scratch buffers on `AudioEngine`.
- Update `trigger_clip`, `trigger_scene`, `stop`, `pause`, and `voice_active` to handle MIDI clips.

### 5. Verification Commands
- `cargo check --workspace`
- `cargo test -p hikaru_core -p hikaru_transport -p hikaru_sequencer -p hikaru_project_file -p hikaru_openlive_file -p hikaru_openstudio_file`
- `cargo test -p hikaru_audio_engine` (all 54+ tests must pass!)
- Add integration tests for MIDI clip scheduling and zero leakage.

## Mandatory Warning
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.


## 2026-11-08T00:48:33Z
You are the Implementer Worker for Milestone 1 of Hikaru OpenLive (MIDI Data Model, Engine Scheduling & Project Persistence).
Read the original request at: /home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/.agents/teamwork/ORIGINAL_REQUEST.md
Your detailed task assignment is at: /home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/.agents/teamwork/m1_worker/DISPATCH.md
Scope document: /home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/PROJECT.md
Explorer reports to follow:
- /home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/.agents/teamwork/m1_explorer_1/handoff.md
- /home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/.agents/teamwork/m1_explorer_2/handoff.md
- /home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/.agents/teamwork/m1_explorer_3/handoff.md
Your working directory is: /home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive
Your dedicated state folder is: /home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/.agents/teamwork/m1_worker

Exclusive Write Ownership:
- `crates/hikaru_core/src/midi_note.rs`
- `crates/hikaru_core/src/lib.rs`
- `crates/hikaru_project_file/src/lib.rs`
- `crates/hikaru_openlive_file/src/lib.rs`
- `crates/hikaru_openstudio_file/src/lib.rs`
- `crates/hikaru_cli/src/project.rs`
- `crates/hikaru_mcp/src/project.rs`
- `crates/hikaru_gui/src/project/openstudio.rs`
- `crates/hikaru_sequencer/src/clip.rs`
- `crates/hikaru_audio_engine/src/lib.rs`
- `crates/hikaru_audio_engine/tests/`

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

Implement:
1. Canonical `MidiNoteInstance` in `crates/hikaru_core/src/midi_note.rs` (16 bytes, sorted by tick, re-exported).
2. Persistence schemas in `hikaru_project_file`, `hikaru_openlive_file`, and `hikaru_openstudio_file` (add `ClipKind::Midi`, update match arms in cli, mcp, and gui).
3. Sequencer `ClipMidiTimeline` and `ClipContentType` in `crates/hikaru_sequencer/src/clip.rs`.
4. Audio Engine real-time scheduling of `self.midi_clips` in `crates/hikaru_audio_engine/src/lib.rs` with sample-accurate NoteOn/NoteOff, beat-quantized looping, zero voice leakage at clip end, and eliminate audio thread heap allocations (`vec![]` and `.clone()`).
5. Run build and tests (`cargo check --workspace`, `cargo test -p hikaru_core -p hikaru_sequencer -p hikaru_project_file -p hikaru_openlive_file -p hikaru_openstudio_file -p hikaru_audio_engine`).

Write your changes report to `changes.md` and your 5-component handoff report to `handoff.md` in your state folder.
Update progress.md as you work.
When done, message me with a concise summary and link to your handoff file.

## 2026-11-08T00:59:15Z
**Context**: Milestone 1 Implementation
**Content**: Status check on implementing canonical `MidiNoteInstance`, persistence schemas, sequencer integration, audio engine scheduling, and test runs.
**Action**: Please update progress.md and report current status / progress on code changes.

## 2026-11-08T01:04:39Z
**Context**: Milestone 1 Implementation - Compiler Fix
**Content**: Compile check noticed a duplicate definition in `crates/hikaru_openlive_file/src/lib.rs`:
`error[E0252]: the name MidiNote is defined multiple times` (imported at line 47 and defined/re-exported at line 260).
**Action**: Please remove the duplicate `MidiNote` import at line 47 in `crates/hikaru_openlive_file/src/lib.rs` and continue your implementation and testing.

## 2026-11-08T01:10:08Z
**Context**: Milestone 1 Implementation Status
**Content**: Status check on Tasks 3 and 4 (`crates/hikaru_sequencer` and `crates/hikaru_audio_engine`) and test executions.
**Action**: Please update progress.md and report current status / progress.

## 2026-11-08T01:15:42Z
**Context**: Milestone 1 Implementation
**Content**: Status query on completing Task 4 (`AudioEngine::process()` scheduling loop), running `cargo check --workspace` and `cargo test -p hikaru_audio_engine`.
**Action**: Please report current status and estimated completion.
