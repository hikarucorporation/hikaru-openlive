# E2E Test Infra: Bitwig MIDI Clips & Piano Roll

## Test Philosophy
- Opaque-box, requirement-driven derived from `ORIGINAL_REQUEST.md` and Bitwig Studio 5.1.7 video reference.
- Strict anti-regression policy: all existing 43+ audio engine unit tests, routing tests, and denormal/leakage tests must remain 100% passing.
- Methodology: Category-Partition + Boundary Value Analysis + Pairwise Combinatorial + Real-World Workload Testing.

## Test Architecture
- Test Runner: `cargo test --workspace` and dedicated test crates/binaries.
- Test Targets:
  - Unit tests in `crates/hikaru_core`, `crates/hikaru_audio_engine`, `crates/hikaru_gui`, `crates/hikaru_openlive_file`, `crates/hikaru_openstudio_file`.
  - Integration tests in `crates/hikaru_gui/tests/` and `crates/hikaru_audio_engine/tests/`.
  - Opaque-box E2E test suites in `tests/e2e_midi_piano_roll/` verifying full workflows.

## Feature Inventory & Test Coverage Matrix

| # | Feature | Requirement Source | Tier 1 (Isolated) | Tier 2 (Boundary/Edge) | Tier 3 (Pairwise) | Tier 4 (Workload) |
|---|---------|-------------------|:-----------------:|:----------------------:|:-----------------:|:-----------------:|
| 1 | MIDI Data Model & Canonical Notes | R1, R3 | 5 | 5 | ✓ | ✓ |
| 2 | Audio Engine MIDI Scheduling | R3 | 5 | 5 | ✓ | ✓ |
| 3 | Clip Triggering & Loop Quantization | R1, R3 | 5 | 5 | ✓ | ✓ |
| 4 | Persistence in `.oplf` & `.opsf` | R3 | 5 | 5 | ✓ | ✓ |
| 5 | AudioProxy Command Dispatch | R2, R3 | 5 | 5 | ✓ | ✓ |
| 6 | Timeline Coordinate Mapping & Zoom/Pan | R2 | 5 | 5 | ✓ | ✓ |
| 7 | 3D Piano Keyboard & Key Audition | R2 | 5 | 5 | ✓ | ✓ |
| 8 | Bar Ruler, Playhead & Loop Markers | R1, R2 | 5 | 5 | ✓ | ✓ |
| 9 | Note Creation (Double-Click) | R2 | 5 | 5 | ✓ | ✓ |
| 10 | Note Selection (Click, Ctrl+Click, Marquee, Ctrl+A) | R2 | 5 | 5 | ✓ | ✓ |
| 11 | Note Deletion (Double-Click, Del/Backspace) | R2 | 5 | 5 | ✓ | ✓ |
| 12 | Note Pitch & Time Drag with Grid Snap | R2 | 5 | 5 | ✓ | ✓ |
| 13 | Note Edge Resizing (Right & Left, Shift Override) | R2 | 5 | 5 | ✓ | ✓ |
| 14 | In-Situ Velocity Drag & Velocity Lane | R2 | 5 | 5 | ✓ | ✓ |
| 15 | Matrix Clip Launcher MIDI Integration | R1 | 5 | 5 | ✓ | ✓ |
| 16 | Arranger Timeline `ClipType::Midi` Integration | R1 | 5 | 5 | ✓ | ✓ |
| 17 | Double-Click Clip to Open Detail Editor Dock | R1, R2 | 5 | 5 | ✓ | ✓ |
| 18 | Detail Editor Splitter Drag & Footer Sync | R2 | 5 | 5 | ✓ | ✓ |

## Real-World Application Scenarios (Tier 4)
| # | Scenario | Features Exercised | Complexity |
|---|----------|--------------------|------------|
| 1 | Electronic Bassline Composition | Double-click create, pitch drag, 1/16 grid, velocity accents, loop bar 1..5 playback | High |
| 2 | Chord Progression & Left/Right Trimming | Polyphonic chord entry, multi-select, transpose octaves, edge duration trimming | High |
| 3 | Session Matrix Live Jam | Trigger multiple MIDI clip slots across scenes, loop playback with audio tracks | High |
| 4 | Arranger Timeline Linear Song Layout | Drag MIDI clips along timeline, resize boundaries, detail editor navigation | High |
| 5 | Full Project Roundtrip Persistence | Compose notes, save to `.oplf` and `.opsf`, reload project, verify bit-exact notes | High |
