# Engine & Data Architecture Survey Report: Hikaru OpenLive
**Explorer**: Engine & Data Explorer  
**Date**: 2026-11-08  
**Scope**: `crates/hikaru_core`, `crates/hikaru_transport`, `crates/hikaru_sequencer`, `crates/hikaru_audio_engine`, `crates/hikaru_dsp`, `crates/hikaru_midi`, `crates/hikaru_plugin_host`, `crates/hikaru_project_file`, `crates/hikaru_openlive_file`, `crates/hikaru_openstudio_file`, and `crates/hikaru_gui` (audio proxy, project, views).

---

## 1. Executive Summary

Hikaru OpenLive is a dual-paradigm Digital Audio Workstation supporting non-linear live performance (`OpenLive`, Session Matrix) and linear arrangement (`OpenStudio`, Arranger Timeline). 

This investigation surveyed the entire data model, sequencing pipeline, real-time audio playback engine, persistence formats, and test infrastructure in preparation for implementing production-ready **MIDI Clips** and a 1:1 **Bitwig-style Piano Roll Editor**.

### Core Discoveries:
1. **Existing Audio Pipeline is Mature & Glitch-Hardened**: Audio clip playback in both Arranger (`OpenStudio`) and Matrix (`OpenLive`) features strict real-time safety, FTZ/DAZ denormal protection, integer-quantized loop wrapping (`quantized_loop_len`), 32-sample linear crossfading, categorical silent muting past end-of-sample (`VoiceState::Finished`), and tanh soft-saturation master limiting.
2. **Disconnected MIDI Structures (The "Missing Bridge")**: There is no unified `MidiClip` type. Instead, at least four separate, uncoordinated representations of MIDI notes exist across `hikaru_audio_engine`, `hikaru_gui::audio_proxy`, `hikaru_gui::piano_roll`, and `hikaru_openlive_file`.
3. **Engine Audio Callback Ignores MIDI**: While `AudioEngine` contains `pub midi_clips: Vec<MidiClipInstance>`, `AudioEngine::process()` **does not process or schedule any MIDI clips or note events**. It exclusively processes `self.clips: Vec<AudioClipInstance>` and `self.dms_voice_pool` (one-shot drum sampler).
4. **Plugin Host Lacks MIDI Dispatch**: `DspPluginNode::process` receives a dummy `_midi_events: &[MidiEvent]` slice that is discarded; `PluginInstance` in `hikaru_plugin_host` exposes only `fn process(&mut self, buffer: &mut AudioBuffer)` without MIDI inputs.
5. **GUI Piano Roll Was Mocked via Sample Previews**: `piano_roll::trigger_playhead_notes` in `hikaru_gui` plays notes by issuing `GuiCommand::PreviewSample` to preview drum files on the UI frame tick, rather than audio-thread scheduled MIDI synthesis.
6. **Persistence Asymmetry**: The `.oplf` (OpenLive) format fully persists `midi_notes` on session matrix clips. Conversely, `.opsf` (OpenStudio timeline) only has a stub `ClipKind::Pattern { pattern_id }` with zero pattern note data stored on disk.

---

## 2. Workspace Topology & Crate Boundaries

```
crates/
├── hikaru_core             # Fundamental audio types (AudioBuffer, AudioClipEvent, SampleRate) [AGPLv3]
├── hikaru_transport        # TransportPosition, clock conversions, DEFAULT_PPQN = 960 [AGPLv3]
├── hikaru_sequencer        # TrackMatrix, Clip, ClipAudioTimeline, QuantizationEngine [AGPLv3]
├── hikaru_audio_engine     # AudioEngine, AudioClipInstance, MidiClipInstance, DmsVoicePool [AGPLv3]
├── hikaru_dsp              # DspPluginNode, WavetableOscillator, StateVariableFilter [AGPLv3]
├── hikaru_midi             # MidiEvent, MidiMessage, MidiSequence (midly parser stub), midir [AGPLv3]
├── hikaru_plugin_host      # VST3 and CLAP plugin hosting, floating windows [AGPLv3]
├── hikaru_project_file     # LGPLv3 shared project primitives (Track, DspSlot, JSON I/O)
├── hikaru_openlive_file    # LGPLv3 .oplf parser/serializer (Matrix, Scenes, Clips, MidiNote)
├── hikaru_openstudio_file  # LGPLv3 .opsf parser/serializer (Playlist, Arranger Clips, Transport)
└── hikaru_gui              # egui + wgpu frontend, AudioProxy, AppState, Views [AGPLv3]
```

### Licensing & Layer Boundary Rule:
The file format crates (`hikaru_project_file`, `hikaru_openlive_file`, `hikaru_openstudio_file`) are strictly LGPL-3.0-or-later and depend **only** on `serde` and `serde_json`. They **must never** import types from `hikaru_core`, `hikaru_gui`, or `hikaru_audio_engine`. The GUI crate (`hikaru_gui/src/project/`) acts as the AGPL translation bridge.

---

## 3. Clip, Track, Event & Timeline Data Structures

### 3.1 Audio Clip Event Model (`crates/hikaru_core/src/audio_clip_event.rs`)
Direct Bitwig 5.1.7 architectural parity for audio clip events:
```rust
// crates/hikaru_core/src/audio_clip_event.rs:35-60
pub struct AudioClipEvent {
    pub id: u64,
    pub name: String,
    pub sample_buffer: Arc<Vec<f32>>,
    pub channels: usize,
    pub sample_rate: u32,
    pub start_frame: u64,
    pub end_frame: u64,
    pub loop_start: u64,
    pub loop_end: u64,
    pub loop_enabled: bool,
    pub gain_db: f32,
    pub fade_in_secs: f64,
    pub fade_out_secs: f64,
    pub stretch_mode: StretchMode, // Raw, Hd, Smooth, Beats
}
```

### 3.2 Sequencer Audio Timeline (`crates/hikaru_sequencer/src/clip.rs`)
Non-destructive sub-region clip timeline:
```rust
// crates/hikaru_sequencer/src/clip.rs:57-72
pub struct ClipAudioEvent {
    pub id: u64,
    pub source_id: u32,
    pub clip_start: u64,     // Frame position within clip timeline
    pub source_offset: u64,  // Offset in audio source
    pub length: u64,         // Length in frames
    pub gain: f32,
    pub fade_in: u64,
    pub fade_out: u64,
}

pub struct ClipAudioTimeline {
    pub events: Vec<ClipAudioEvent>,
    pub next_id: u64,
}
```

### 3.3 Audio Engine Clip Instance (`crates/hikaru_audio_engine/src/lib.rs`)
Runtime clip instance executed in the real-time callback:
```rust
// crates/hikaru_audio_engine/src/lib.rs:150-172
pub struct AudioClipInstance {
    pub id: usize,
    pub track_index: usize,
    pub scene_index: usize,
    pub samples: Vec<f32>,
    pub start_frame: u64,
    pub start_absolute: u64,
    pub duration_frames: u64,
    pub sample_offset: usize,
    pub channels: usize,
    pub is_playing: bool,
    pub clip_loop_enabled: bool,
    pub clip_loop_start: u64,
    pub clip_loop_end: u64,
    pub is_matrix_clip: bool,
    pub prev_frame: usize,
    pub xfade_remaining: u32,
    pub xfade_len: u32,
    pub events: Vec<EngineAudioEvent>,
}
```

### 3.4 Session Matrix Structures
1. **Sequencer Matrix (`crates/hikaru_sequencer/src/matrix.rs:8-12`)**:
   ```rust
   pub const MAX_TRACKS: usize = 16;
   pub const MAX_SCENES: usize = 32;
   pub struct TrackMatrix {
       pub slots: [[Option<Clip>; MAX_SCENES]; MAX_TRACKS],
   }
   ```
2. **GUI Session Matrix (`crates/hikaru_gui/src/views/matrix.rs:166-350`)**:
   ```rust
   pub enum ClipData {
       Audio {
           events: Vec<AudioEvent>,
           next_event_id: u64,
           preview_mix: Vec<f32>,
           preview_sr: u32,
       },
       Midi {
           notes: Vec<(u64, u8, u8, u32)>, // (start_tick, pitch, velocity, duration_ticks)
       },
   }

   pub struct MatrixClip {
       pub id: usize,
       pub name: String,
       pub path: PathBuf,
       pub duration_secs: f64,
       pub content: ClipData,
       pub loop_start: u64,
       pub loop_end: u64,
       pub loop_enabled: bool,
       pub peaks: Vec<f32>,
       // ...
   }

   pub struct MatrixSlot {
       pub state: SlotState, // Empty, Stopped, Queued, Playing
       pub clip: Option<MatrixClip>,
   }

   pub struct SessionMatrixState {
       pub grid: Vec<Vec<MatrixSlot>>,
       pub tracks: Vec<TrackMeta>,
       pub scenes: Vec<SceneMeta>,
       pub selected_slot: Option<(usize, usize)>,
       // ...
   }
   ```

### 3.5 Arranger Timeline Structures (`crates/hikaru_gui/src/views/playlist.rs`)
```rust
// crates/hikaru_gui/src/views/playlist.rs:47-71
pub enum ClipType {
    Pattern { pattern_id: usize },
    Audio {
        sample_path: String,
        peaks: Vec<f32>,
        sample_offset_ticks: u64,
        total_sample_ticks: u64,
    },
    Automation {
        points: Vec<CurvePoint>,
        target_param: String,
    },
}

pub struct PlaylistClip {
    pub id: usize,
    pub name: String,
    pub start_tick: u64,
    pub duration_ticks: u64,
    pub clip_type: ClipType,
    pub color: egui::Color32,
}

pub struct PlaylistState {
    pub clips: Vec<(usize, PlaylistClip)>, // (track_idx, clip)
    pub playhead_tick: u64,
    pub ppqn: u64,
    pub zoom_x: f32,
    pub loop_start_ticks: u64,
    pub loop_end_ticks: u64,
    pub loop_region_active: bool,
    // ...
}
```

---

## 4. Audio vs MIDI Representation & Note Events

### 4.1 Comparative Note Event Models

| Location | Struct / Type | Fields | Time Base | Note Pitch | Velocity | Duration |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| `hikaru_audio_engine::lib.rs:23` | `MidiNoteInstance` | `start_tick, pitch, velocity, duration_ticks` | `u64` ticks | `u8` (0-127) | `u8` (0-127) | `u32` ticks |
| `hikaru_gui::audio_proxy.rs:9` | `MidiNoteInstance` | `start_tick, pitch, velocity, duration_ticks` | `u64` ticks | `u8` (0-127) | `u8` (0-127) | `u32` ticks |
| `hikaru_gui::piano_roll.rs:24` | `MidiNote` | `pitch, start_tick, duration_ticks, velocity` | `u64` ticks | `u8` (0-127) | `u8` (0-127) | `u64` ticks |
| `hikaru_openlive_file::lib.rs:261` | `MidiNote` | `start_tick, note, velocity, duration_ticks` | `u64` ticks | `u8` (0-127) named `note` | `u8` (0-127) | `u32` ticks |
| `hikaru_gui::views::matrix.rs:174` | `ClipData::Midi` | `notes: Vec<(u64, u8, u8, u32)>` | `u64` ticks | `u8` (0-127) | `u8` (0-127) | `u32` ticks |
| `hikaru_midi::events.rs:2` | `MidiMessage` | `NoteOn { channel, pitch, velocity }` | delta ticks (external) | `u8` (0-127) | `u8` (0-127) | split NoteOff |
| `hikaru_midi::lib.rs:9` | `MidiEvent` | `status: u8, data1: u8, data2: u8` | raw MIDI 1.0 byte triplet | `data1` | `data2` | separate NoteOff |

### 4.2 Clock & Timing Reference Authority
`DEFAULT_PPQN = 960` in `crates/hikaru_transport/src/transport_state.rs:8` is the **single source of truth** across the entire workspace.
- `TICKS_PER_BEAT = 960`
- `TICKS_PER_BAR = 3840` (in 4/4 time)
- Conversion formulae in `TransportPosition`:
  $$\text{seconds\_per\_tick} = \frac{60.0}{\text{BPM} \times \text{PPQN}}$$
  $$\text{ticks\_to\_samples}(\text{ticks}) = \text{round}\left(\text{ticks} \times \text{seconds\_per\_tick} \times \text{sample\_rate}\right)$$
  $$\text{samples\_to\_ticks}(\text{samples}) = \text{round}\left(\frac{\text{samples} / \text{sample\_rate}}{\text{seconds\_per\_tick}}\right)$$

### 4.3 MIDI File Ingestion
In `crates/hikaru_midi/src/file.rs`, `MidiSequence::from_file_bytes` parses bytes with `midly::Smf`, but currently returns an empty `messages: vec![]`.
In `crates/hikaru_gui/src/views/matrix.rs:715`, dropping a `.mid` file creates `ClipData::Midi { notes: Vec::new() }`. **The note events from the SMF file are not yet parsed into the clip.**

---

## 5. Real-Time Audio Engine & Playback Architecture

### 5.1 Concurrency & Audio Thread Safety

```
┌────────────────────────────────────────────────────────┐
│                      GUI Thread                        │
│   egui / wgpu event loop; PianoRollState; MatrixState  │
└──────────────────────────┬─────────────────────────────┘
                           │ GuiCommand (via MPSC Channel)
                           ▼
┌────────────────────────────────────────────────────────┐
│                   Background Worker                    │
│   main.rs: decodes WAVs, acquires engine.lock(),       │
│   updates AudioEngine structs (clips, track params)     │
└──────────────────────────┬─────────────────────────────┘
                           │ Shared via Arc<Mutex<AudioEngine>>
                           │ + Lock-free Atomics
                           ▼
┌────────────────────────────────────────────────────────┐
│                 Real-Time Audio Thread                 │
│   CPAL / PipeWire driver callback -> process(&mut buf) │
│                                                        │
│   1. Denormals FTZ/DAZ enabled                         │
│   2. PreviewPlayer mixed                               │
│   3. DmsVoicePool processed (with track gains/pans)    │
│   4. If Playing:                                       │
│      - Per-clip sample generation (Studio or Live)     │
│      - Loop wraparound quantized to beat multiple      │
│      - 32-sample linear loop crossfade                 │
│      - Non-looping clips finished -> silence           │
│      - Per-track VST3/CLAP FX processing               │
│      - Per-track peak measurement                      │
│   5. Denormals flushed                                 │
│   6. Master gain & tanh soft-clipping saturation       │
│   7. Atomic position & peak store                      │
└────────────────────────────────────────────────────────┘
```

### 5.2 Glitch-Free Anti-Regression Mechanisms in `hikaru_audio_engine`
1. **FTZ/DAZ (Flush-To-Zero / Denormals-Are-Zero)**:
   - Configured via x86/x86_64 MXCSR register bits 15 and 6 (`enable_ftz_daz()` in `lib.rs:41-48`).
   - Software backup fallback: `flush_denormals(buf)` (lines 61-68) clears denormals before they cause CPU pipeline stalls in floating point filters.
2. **Quantized Loop Lengths (`quantized_loop_len` in `lib.rs:220-233`)**:
   - Loops longer than 1 beat are clamped to exact integer multiples of `samples_per_beat`.
   - Prevents fractional phase drift between multiple simultaneously looping tracks across long cycles.
3. **Loop Wraparound Crossfade (`lib.rs:1817-1865`)**:
   - When a loop wraps around, 32 samples are smoothly crossfaded between the end and the start to eliminate DC-offset clicks.
4. **Strict Silence & Voice Retirement (`lib.rs:1753-1756, 1799-1802`)**:
   - Non-looping clips past `emission_len_frames()` categorically mute and set `is_playing = false`. Zero sample leakage into subsequent bars.
5. **Output Peak & Limiter (`lib.rs:1934-1941`)**:
   - Every sample is passed through `s.tanh()` to soft-saturate extreme peaks without wrapping or harsh digital square waves.

### 5.3 The Execution Gap for MIDI Clips
In `AudioEngine::process()` (`lib.rs:1604-1945`):
- `self.clips` (audio clips) are processed in lines 1705-1873.
- `self.midi_clips` is **never iterated**.
- When `trigger_clip(track_idx, scene_idx)` is called (`lib.rs:1361`), it only toggles `clip.is_playing` on `self.clips`. It leaves `self.midi_clips` completely untouched.
- Therefore, even if a MIDI clip has notes and is loaded into the engine, it will never emit sound or trigger instruments during `process()`.

---

## 6. Project Persistence Architecture (.oplf & .opsf)

### 6.1 OpenLive File Format (`.oplf`)
Defined in `crates/hikaru_openlive_file/src/lib.rs`.
- File extension: `.oplf` (Version 1).
- JSON formatted with `serde_json::to_string_pretty`.
- Validated via `parse_with_probe` checking `format_version == 1` before deserializing.
- Schema:
  - `metadata`: `title`, `artist`, `genre`, `key`, `stats` (`total_scenes`, `total_tracks`, `audio_clips`, `midi_clips`).
  - `transport`: `bpm`, `time_signature: [4, 4]`, `launch_quantization`, `loop_start_ticks`, `loop_end_ticks`, `loop_enabled`.
  - `live_tracks`: `Vec<Track>` (from `hikaru_project_file`).
  - `matrix`: `grid: Vec<Vec<Option<Clip>>>`.
    - `Clip`:
      ```rust
      pub struct Clip {
          pub id: usize,
          pub name: String,
          pub path: String,
          pub loop_start: u64,
          pub loop_end: u64,
          pub loop_enabled: bool,
          pub volume: f32,
          pub pan: f32,
          pub mute: bool,
          pub events: Vec<Event>,
          pub midi_notes: Vec<MidiNote>, // (start_tick, note, velocity, duration_ticks)
      }
      ```
- **Finding**: `.oplf` already contains full persistence support for Session Matrix MIDI notes!

### 6.2 OpenStudio File Format (`.opsf`)
Defined in `crates/hikaru_openstudio_file/src/lib.rs`.
- File extension: `.opsf` (Version 1).
- Schema:
  - `metadata`: stats include `midi_clips`.
  - `transport`: `bpm`, `beats_per_bar`, `beat_division`, `loop_start_ticks`, `loop_end_ticks`, `loop_enabled`.
  - `playlist`: `clips: Vec<(usize, PlaylistClipSaved)>`, `ppqn: u64 = 960`.
    - `PlaylistClipSaved`:
      ```rust
      pub struct PlaylistClipSaved {
          pub id: usize,
          pub name: String,
          pub start_tick: u64,
          pub duration_ticks: u64,
          pub color: Color,
          pub kind: ClipKind,
      }

      pub enum ClipKind {
          Pattern { pattern_id: usize },
          Audio { sample_path: String, sample_offset_ticks: u64, total_sample_ticks: u64 },
          Automation { points: Vec<CurvePoint>, target_param: String },
      }
      ```
- **Finding / Gap**: `ClipKind::Pattern` only stores a scalar `pattern_id: usize`. There is no pattern definition or note storage table in `.opsf`. An Arranger MIDI clip cannot save or load its notes in `.opsf` without extending `ClipKind` (e.g. adding `Midi { notes: Vec<MidiNote> }` or adding a `patterns: Vec<MidiPattern>` table to `OpenStudioProject`).

---

## 7. Test Suites & Verification Infrastructure

### 7.1 Existing Test Suite Status
Tested and verified with `cargo test --target-dir /tmp/hikaru_target_test`:

| Package | Test Binary / Target | Result | Test Count | Notable Cases Covered |
| :--- | :--- | :--- | :--- | :--- |
| `hikaru_core` | unittests `src/lib.rs` | **PASSED** | 0 tests (pure types) | Compilation verified |
| `hikaru_transport` | unittests `src/lib.rs` | **PASSED** | 3 passed | Loop wrapping, identity when disabled, invalid region clamp |
| `hikaru_sequencer` | unittests `src/lib.rs` | **PASSED** | 14 passed | Clip loop clamping, dynamic clip length, cut/paste timeline, session slot deactivation on finished voice, quantization math |
| `hikaru_project_file` | unittests `src/lib.rs` | **PASSED** | 2 passed | JSON version probe, header rejection of non-project files |
| `hikaru_openlive_file` | unittests `src/lib.rs` | **PASSED** | 5 passed | Launch quantization notation parsing, default matrix grid shape, project round-trip |
| `hikaru_openstudio_file` | unittests `src/lib.rs` | **PASSED** | 3 passed | Template track layout, PPQN preservation in `.opsf` |
| `hikaru_audio_engine` | unittests `src/lib.rs` | **PASSED** | 43 passed | FTZ/DAZ, quantized loop beats, 32-sample xfade, phase drift prevention, multi-event pad mixing, preview player |
| `hikaru_audio_engine` | integration `tests/ghost_voice_repro.rs` | **PASSED** | 5 passed | Absolute silence past bar 5, Time Selection wrapping, -inf dB vumeter past clip end |
| `hikaru_audio_engine` | integration `tests/mixer_track_routing.rs` | **PASSED** | 3 passed | Per-track DMS volume isolation, pan normalization -1..1 to -100..100 |

### 7.2 Cargo Build Lock Finding
When running tests directly in the workspace root without an isolated target dir, `cargo test --workspace` can block on `target/debug/.cargo-lock` if another process or agent is compiling concurrently. Using `--target-dir /tmp/target_xyz` guarantees isolated, unblocked compilation and execution.

---

## 8. Identified Architectural Gaps & Breakages

```
┌────────────────────────────────────────────────────────────────────────────────┐
│                         CURRENT ARCHITECTURAL GAPS                             │
└────────────────────────────────────────────────────────────────────────────────┘

 1. DATA MODEL FRAGMENTATION
    - hikaru_audio_engine::MidiNoteInstance
    - hikaru_gui::audio_proxy::MidiNoteInstance
    - hikaru_gui::piano_roll::MidiNote
    - hikaru_openlive_file::MidiNote
    -> Need a single canonical MidiNote in hikaru_core or hikaru_midi, or 
       standardized conversions.

 2. AUDIO ENGINE SCHEDULING DISCONNECTION
    - AudioEngine::midi_clips exists, but AudioEngine::process() never iterates it.
    - trigger_clip() and trigger_scene() do not toggle is_playing on midi_clips.
    - When is_playing is active, MIDI notes within the current buffer quantum 
      [transport.sample_count, transport.sample_count + buffer_frames) are never 
      translated to note-on / note-off events.

 3. INSTRUMENT / SYNTH DISPATCH MISSING
    - DspPluginNode::process takes _midi_events: &[MidiEvent], but drops it.
    - PluginInstance trait has no MIDI input parameter.
    - AudioEngine has no built-in polyphonic synth (WavetableOscillator is mono,
      DmsVoicePool is sample-only for drums).
    - To hear MIDI notes from keys/piano roll, either:
      a) OpenDMS triggers drum pads on corresponding pitches (already works for drums).
      b) OpenWavetable or a synthesizer node receives NoteOn/NoteOff.
      c) External VST3/CLAP plugins receive MIDI events.

 4. ARRANGER TIMELINE (PLAYLIST) PERSISTENCE & SYNC GAP
    - SyncPlaylistClips only syncs ClipType::Audio.
    - ClipType::Pattern contains no note vector.
    - .opsf format cannot store notes for ClipType::Pattern.

 5. PIANO ROLL UI INTERACTIVITY GAPS
    - Velocity lane is currently not rendered or editable in piano_roll.rs.
    - Interactive note creation (double-click / draw), note moving in pitch/time, 
      and note resizing are only partially represented in PianoRollState fields, 
      not wired in the render() loop.
    - Piano roll header/footer toggle is not automatically opened when a MIDI clip 
      is selected or double-clicked in Matrix or Arranger.
```

---

## 9. Actionable Recommendations for Implementation

### R1. Data Model Unification
1. In `crates/hikaru_core` (or `crates/hikaru_midi`), define a canonical `MidiNote`:
   ```rust
   #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
   pub struct MidiNote {
       pub start_tick: u64,
       pub pitch: u8,
       pub velocity: u8,
       pub duration_ticks: u32,
   }
   ```
2. In `crates/hikaru_gui::views::playlist`:
   - Expand `ClipType` or `ClipType::Pattern` to store note data:
     ```rust
     pub enum ClipType {
         Pattern { pattern_id: usize, notes: Vec<MidiNote> },
         // ...
     }
     ```
   - In `crates/hikaru_openstudio_file`:
     - Update `ClipKind::Pattern` or add `ClipKind::Midi { notes: Vec<MidiNote> }` with `#[serde(default)]` to preserve backwards compatibility.

### R2. Audio Engine MIDI Scheduling
1. In `AudioEngine`:
   - Update `trigger_clip(track_index, scene_index)` and `trigger_scene(scene_index)` to activate `midi_clips` in tandem with `clips`.
   - In `AudioEngine::process()`:
     - For each playing `MidiClipInstance`, compute current buffer tick window:
       `[start_tick, end_tick)` from `transport.sample_count`.
     - Detect NoteOn events (`start_tick` within window) and NoteOff events (`start_tick + duration_ticks` within window).
     - Dispatch to track instrument/effect nodes:
       - For OpenDMS tracks: call `dms_voice_pool.trigger()` or trigger drum pads.
       - For external plugins / synth: pass `&[MidiEvent]` to `DspPluginNode::process`.

### R3. GUI & AudioProxy Integration
1. In `crates/hikaru_gui/src/audio_proxy.rs`:
   - Add `SyncMidiClips` and ensure `UpdateMidiClipNotes` / `LoadMidiClip` are fully dispatched in `main.rs`.
2. When double-clicking a MIDI clip in Matrix or Arranger:
   - Load its notes into `app.piano_roll_state.notes`.
   - Set `app.show_piano_roll = true`.
   - Associate `piano_roll_state.notes_source_slot = Some((track_idx, scene_idx))` or clip ID.
   - When notes are modified in Piano Roll, automatically sync back to `MatrixClip` / `PlaylistClip` and send `GuiCommand::UpdateMidiClipNotes`.

### R4. Real-Time Regression Guards
- Retain all FTZ/DAZ denormal zeroing.
- Ensure MIDI note dispatch loop does not allocate memory on the audio thread: preallocate note event buffers or use fixed ring buffers.
- Run `cargo test -p hikaru_audio_engine -p hikaru_sequencer -p hikaru_transport` after changes to verify zero audio regressions.
