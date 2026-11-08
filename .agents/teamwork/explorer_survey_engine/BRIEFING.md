# BRIEFING — 2026-11-08T00:15:00Z

## Mission
Survey core, engine, daw, and playback crates for data structures, MIDI representation, real-time scheduling, project persistence, and tests to inform MIDI Clips & Bitwig-style Piano Roll implementation.

## 🔒 My Identity
- Archetype: Teamwork explorer
- Roles: Read-only investigation, analyze problems, synthesize findings, produce structured reports
- Working directory: /home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive
- State folder: /home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/.agents/teamwork/explorer_survey_engine
- Original parent: 3b10c257-464e-4b7c-926b-aebaa894c34a
- Milestone: Engine & Data Architecture Survey

## 🔒 Key Constraints
- Read-only investigation — do NOT implement
- Produce analysis.md and handoff.md in state folder
- Use send_message to report back to parent

## Current Parent
- Conversation ID: 3b10c257-464e-4b7c-926b-aebaa894c34a
- Updated: 2026-11-08T00:10:00Z

## Investigation State
- **Explored paths**:
  - `Cargo.toml`
  - `crates/hikaru_core` (`src/audio_clip_event.rs`, `src/audio_buffer.rs`, `src/lib.rs`)
  - `crates/hikaru_transport` (`src/transport_state.rs`, `src/lib.rs`)
  - `crates/hikaru_sequencer` (`src/clip.rs`, `src/matrix.rs`, `src/quantizer.rs`, `src/lib.rs`)
  - `crates/hikaru_audio_engine` (`src/lib.rs`, `src/preview_player.rs`, `tests/*`)
  - `crates/hikaru_dsp` (`src/plugin_node.rs`, `src/synth/wavetable.rs`)
  - `crates/hikaru_plugin_host` (`src/lib.rs`)
  - `crates/hikaru_midi` (`src/events.rs`, `src/file.rs`, `src/device.rs`)
  - `crates/hikaru_project_file` (`src/lib.rs`)
  - `crates/hikaru_openlive_file` (`src/lib.rs`)
  - `crates/hikaru_openstudio_file` (`src/lib.rs`)
  - `crates/hikaru_gui` (`src/audio_proxy.rs`, `src/views/piano_roll.rs`, `src/views/matrix.rs`, `src/views/playlist.rs`, `src/views/arranger_view.rs`, `src/views/detail_editor_panel.rs`, `src/project/*`, `src/main.rs`)
- **Key findings**:
  1. Data structures: Multiple disjoint representations of MIDI notes and clips exist (`hikaru_audio_engine::MidiNoteInstance`, `hikaru_gui::audio_proxy::MidiNoteInstance`, `hikaru_gui::piano_roll::MidiNote`, `hikaru_openlive_file::MidiNote`).
  2. Engine Gap: `AudioEngine::process()` only renders audio clips (`self.clips`); `self.midi_clips` is never iterated or scheduled to `track_effects` or any instrument.
  3. Sequencer decoupling: `hikaru_sequencer::TrackMatrix` exists as a separate audio-oriented fixed array struct, but `AudioEngine` uses `Vec<AudioClipInstance>` and `Vec<MidiClipInstance>` directly.
  4. Timing: `DEFAULT_PPQN = 960` in `hikaru_transport` is the central clock authority. Conversions (`ticks_to_samples`, `samples_to_ticks`) are precise.
  5. Persistence: `.oplf` supports `midi_notes: Vec<MidiNote>` on `Clip`. `.opsf` only has `ClipKind::Pattern { pattern_id }` with no notes storage.
  6. Glitch-Free Audio Protections: FTZ/DAZ denormal flushing, integer-quantized loops, 32-sample loop crossfade, strict zeroing past natural clip frames.
- **Unexplored areas**: None for this milestone.

## Key Decisions Made
- All tests verified with isolated `--target-dir` to avoid Cargo workspace build lock contention.
- Documented full gap analysis and exact structural recommendations for MIDI clip pipeline.

## Artifact Index
- analysis.md — Full investigation report with data structures, architecture, call graphs, constraints, and recommendations
- handoff.md — 5-component handoff report (Observation, Logic Chain, Caveats, Conclusion, Verification Method)
- progress.md — Liveness heartbeat
