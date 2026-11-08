# Technical Analysis: Data Structures, Sequencer & Crate Unification Strategy

**Explorer**: M1 Explorer 2  
**Date**: 2026-11-08  
**Scope**: `crates/hikaru_core`, `crates/hikaru_transport`, `crates/hikaru_sequencer`, `crates/hikaru_audio_engine`, `crates/hikaru_gui`  
**Reference Target**: Bitwig Studio 5.1.7 1-to-1 Parity for MIDI Clips & Piano Roll

---

## 1. Executive Summary

This investigation delivers the unified architecture, data structures, and mathematical proofs required to support production-grade MIDI Clips and a 1:1 Bitwig-style Piano Roll in Hikaru OpenLive.

### Core Survey Findings
1. **Disconnected Data Representations**: Currently, MIDI notes are represented inconsistently across at least four locations:
   - `hikaru_audio_engine::lib.rs:23`: `MidiNoteInstance { start_tick: u64, pitch: u8, velocity: u8, duration_ticks: u32 }`
   - `hikaru_gui::audio_proxy.rs:9`: Identical copy of `MidiNoteInstance`
   - `hikaru_gui::piano_roll.rs:24`: Divergent `MidiNote { pitch: u8, start_tick: u64, duration_ticks: u64, velocity: u8 }`
   - `hikaru_gui::views::matrix.rs:174`: Raw tuple `(u64, u8, u8, u32)` in `ClipData::Midi`
   - `hikaru_core`: Currently lacks any MIDI note or event definition.
2. **Sequencer Audio-Centricity**: `crates/hikaru_sequencer/src/clip.rs` models `Clip` and `ClipAudioTimeline` strictly for sample buffers (`audio_buffer_id: u32`, frame offsets). MIDI clips have no representation in `hikaru_sequencer`, preventing unified quantization, state management (`Stopped`, `Queued`, `Playing`, `Stopping`), and voice polling (`VoiceState::Finished`).
3. **Audio Engine MIDI Execution Gap**: In `hikaru_audio_engine`, `AudioEngine::process()` only iterates `self.clips: Vec<AudioClipInstance>`. `self.midi_clips: Vec<MidiClipInstance>` is never processed, scheduled, or polled. Furthermore, `trigger_clip()` and `trigger_scene()` only toggle audio clips.
4. **Clock Mathematics Verification**: Analysis of `hikaru_transport/src/transport_state.rs` demonstrates that tick-to-sample and sample-to-tick math is fundamentally sound because calculations are evaluated against absolute positions on the timeline rather than accumulating delta steps. Truncation error is strictly bounded to $\pm 0.5$ samples across arbitrary playback durations.

---

## 2. Investigation 1: Canonical `MidiNoteInstance` Data Model

### 2.1 Struct Definition & Memory Alignment
The canonical MIDI note representation must reside in `crates/hikaru_core/src/midi_note.rs`.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MidiNoteInstance {
    pub start_tick: u64,
    pub pitch: u8,
    pub velocity: u8,
    pub duration_ticks: u32,
}
```

#### Memory Layout Analysis (x86_64 / aarch64)
| Field | Type | Offset | Size | Alignment | Purpose |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `start_tick` | `u64` | 0 | 8 bytes | 8 bytes | Timeline start position in 960 PPQN ticks |
| `pitch` | `u8` | 8 | 1 byte | 1 byte | MIDI pitch (0..=127, where 60 = Middle C) |
| `velocity` | `u8` | 9 | 1 byte | 1 byte | Note velocity (1..=127, 0 = note off) |
| *[compiler padding]* | — | 10 | 2 bytes | — | Automatic alignment padding |
| `duration_ticks` | `u32` | 12 | 4 bytes | 4 bytes | Note length in ticks |
| **Total Struct Size** | — | — | **16 bytes** | **8 bytes** | **Exact power of two (16 bytes)** |

#### Key Benefits of this Layout:
1. **Zero Wasted Memory**: At exactly 16 bytes, four `MidiNoteInstance` structs fit precisely into a standard 64-byte L1 CPU cache line.
2. **`duration_ticks: u32` vs `u64`**:
   - At `DEFAULT_PPQN = 960`, 1 beat = 960 ticks. In 4/4 time, 1 bar = 3,840 ticks.
   - Max value of `u32` is $4,294,967,295$ ticks, which equals $4,473,924$ beats or **$1,118,481$ bars** (over 380 days of continuous playback at 120 BPM).
   - If `duration_ticks` were `u64`, 6 bytes of padding would be added after `velocity`, blowing the struct up to 24 bytes (33% memory bloat and misaligned with 64-byte cache lines).
3. **`start_tick: u64`**: Timeline start position uses `u64` to prevent overflow in long arrangements or infinite live performance loops.
4. **Copy Semantics**: Because all fields are primitive integer types, `MidiNoteInstance` implements `Copy`. Passing by value has zero overhead.
5. **Natural Chronological Ordering (`Ord`, `PartialOrd`)**:
   Rust derives ordering in declaration order:
   1. `start_tick` (chronological order)
   2. `pitch` (low to high)
   3. `velocity`
   4. `duration_ticks`
   This allows standard `.sort()` or `.sort_unstable()` to sort notes chronologically without custom closures.

### 2.2 Methods & Interoperability
`MidiNoteInstance` provides zero-cost helper methods and conversions:
- `end_tick(&self) -> u64`: `self.start_tick.saturating_add(self.duration_ticks as u64)`
- `contains_tick(&self, tick: u64) -> bool`: `tick >= self.start_tick && tick < self.end_tick()`
- `overlaps_range(&self, start: u64, end: u64) -> bool`: `self.start_tick < end && self.end_tick() > start`
- Bidirectional `From`/`Into` conversions with `(u64, u8, u8, u32)` for backward compatibility with legacy tuples.

### 2.3 Layer Boundary & License Preservation
- `hikaru_core` is AGPLv3.
- The file format crates (`hikaru_openlive_file` and `hikaru_openstudio_file`) are LGPLv3 and depend only on `serde` and `serde_json`.
- `hikaru_openlive_file::MidiNote` has `note: u8` while `MidiNoteInstance` has `pitch: u8`.
- The GUI crate (`crates/hikaru_gui/src/project/openlive.rs`) acts as the AGPL translation bridge:
  ```rust
  // From .oplf to canonical MidiNoteInstance
  MidiNoteInstance {
      start_tick: saved.start_tick,
      pitch: saved.note,
      velocity: saved.velocity,
      duration_ticks: saved.duration_ticks,
  }
  ```

---

## 3. Investigation 2: Sequencer Integration (`crates/hikaru_sequencer`)

### 3.1 Clip State Machine & Audio-Only Gap
In `crates/hikaru_sequencer/src/clip.rs`:
- `ClipState` defines the 4-phase lifecycle: `Stopped (0)`, `Queued (1)`, `Playing (2)`, `Stopping (3)`.
- `VoiceState` defines voice emission: `Active`, `Finished`.
- `Clip` struct:
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
  `Clip` is currently tightly coupled to PCM audio frames (`audio_buffer_id`, `start_sample`, `total_frames`).

### 3.2 Unifying Clip Model for Audio and MIDI
To establish complete parity in the sequencer without breaking existing tests:
1. Introduce `ClipMidiTimeline` in `crates/hikaru_sequencer/src/clip.rs`:
   ```rust
   use hikaru_core::MidiNoteInstance;

   #[derive(Debug, Clone, Default, PartialEq)]
   pub struct ClipMidiTimeline {
       pub notes: Vec<MidiNoteInstance>,
       pub total_ticks: u64,
   }

   impl ClipMidiTimeline {
       pub fn new(total_ticks: u64) -> Self {
           Self {
               notes: Vec::new(),
               total_ticks,
           }
       }

       pub fn effective_total_ticks(&self) -> u64 {
           self.notes.iter().map(|n| n.end_tick()).max().unwrap_or(0).max(self.total_ticks)
       }

       pub fn add_note(&mut self, note: MidiNoteInstance) {
           self.notes.push(note);
           self.notes.sort();
       }
   }
   ```
2. Extend `Clip` with `ClipContentType`:
   ```rust
   #[derive(Debug, Clone, Copy, PartialEq, Eq)]
   pub enum ClipContentType {
       Audio,
       Midi,
   }
   ```
   Add fields to `Clip`:
   - `pub content_type: ClipContentType`
   - `pub midi_timeline: Option<ClipMidiTimeline>`
   - `pub loop_start_tick: u64`
   - `pub loop_end_tick: u64`
   - `pub total_ticks: u64`
3. Provide constructor `Clip::new_midi(id: u32, total_ticks: u64) -> Self`:
   Defaults `content_type = ClipContentType::Midi`, `audio_buffer_id = 0`, `loop_enabled = true`, `total_ticks = total_ticks`.

### 3.3 Quantized Triggering & Math Audit (`quantizer.rs`)
In `crates/hikaru_sequencer/src/quantizer.rs`:
```rust
pub enum QuantizationGrid {
    Bar, HalfNote, Quarter, Eighth, Sixteenth, None
}
```
Currently:
1. `QuantizationEngine::samples_until_next_sync` hardcodes `QuantizationGrid::Bar => 4.0`.
   **Fix**: Use `current_pos.beats_per_bar as f64`.
2. Float modulo `current_sample % samples_per_grid_unit` with `if remainder < 0.001` introduces floating-point jitter.
3. **Tick-Based Quantization (Zero-Drift Standard)**:
   Add `QuantizationEngine::ticks_until_next_sync`:
   ```rust
   pub fn ticks_until_next_sync(
       current_tick: u64,
       grid: QuantizationGrid,
       beats_per_bar: u32,
       ppqn: u64,
   ) -> u64 {
       let grid_ticks = match grid {
           QuantizationGrid::Bar => (beats_per_bar as u64).max(1) * ppqn,
           QuantizationGrid::HalfNote => 2 * ppqn,
           QuantizationGrid::Quarter => ppqn,
           QuantizationGrid::Eighth => ppqn / 2,
           QuantizationGrid::Sixteenth => ppqn / 4,
           QuantizationGrid::None => return 0,
       };
       let rem = current_tick % grid_ticks;
       if rem == 0 { 0 } else { grid_ticks - rem }
   }
   ```
   At 960 PPQN:
   - Bar (4 beats) = 3840 ticks
   - Half = 1920 ticks
   - Quarter = 960 ticks
   - 8th = 480 ticks
   - 16th = 240 ticks
   All values divide evenly with zero remainder and zero floating-point imprecision.

---

## 4. Investigation 3: Transport Clock & Timing Mathematics (`crates/hikaru_transport`)

### 4.1 Evaluation of Existing Transport Math
In `crates/hikaru_transport/src/transport_state.rs`:
- `DEFAULT_PPQN: u64 = 960`
- `seconds_per_tick(&self) -> f64 = (60.0 / bpm) / ppqn`
- `ticks_to_samples(&self, ticks: u64) -> u64 = (ticks as f64 * seconds_per_tick * sr).round() as u64`
- `samples_to_ticks(&self, samples: u64) -> u64 = ((samples as f64 / sr) / seconds_per_tick).round() as u64`

### 4.2 Mathematical Analysis of Drift Invariance
#### Question: Does tick-to-sample and sample-to-tick conversion drift over time?
**Theorem**: The conversion methods in `TransportPosition` have **zero cumulative drift**.

**Proof**:
Cumulative phase drift occurs when a sequencer increments its clock by a rounded per-tick delta at each step:
$$S_{N} = \sum_{k=1}^{N} \text{round}(\Delta s)$$
The rounding error $\epsilon_k \in [-0.5, 0.5]$ accumulates linearly: $E_N = \sum \epsilon_k \sim \mathcal{O}(N)$. Over $10^6$ ticks, drift reaches thousands of samples.

In contrast, `TransportPosition::ticks_to_samples(T)` evaluates directly from the absolute timeline position $T$:
$$S(T) = \text{round}\left( T \times \frac{60 \cdot f_s}{\text{BPM} \cdot \text{PPQN}} \right)$$
Because $S(T)$ is computed as a closed-form function of the absolute integer $T$:
1. The error $\epsilon(T) = S(T) - T \cdot \frac{60 f_s}{\text{BPM} \cdot \text{PPQN}}$ is strictly bounded: $|\epsilon(T)| \le 0.5$ samples for all $T \in [0, \infty)$.
2. There is no memory of prior calculations; errors cannot accumulate.
3. The inverse function $T(S) = \text{round}\left( S \times \frac{\text{BPM} \cdot \text{PPQN}}{60 \cdot f_s} \right)$ has the identical bound: $|\epsilon(S)| \le 0.5$ ticks.

#### Precision Optimization
To reduce floating-point rounding noise from multiple intermediate divisions:
```rust
// Current (3 divisions):
(ticks as f64 * (60.0 / bpm) / self.ppqn() as f64 * sr).round() as u64

// Optimized (Single division):
let num = ticks as f64 * sr * 60.0;
let den = bpm * self.ppqn() as f64;
(num / den).round() as u64
```
And for `samples_to_ticks`:
```rust
let num = samples as f64 * bpm * self.ppqn() as f64;
let den = sr * 60.0;
(num / den).round() as u64
```
This formulation eliminates intermediate ULP truncation.

---

## 5. Investigation 4: Real-time Audio Engine MIDI Scheduling (`crates/hikaru_audio_engine`)

### 5.1 `MidiClipInstance` Schema Alignment
In `crates/hikaru_audio_engine/src/lib.rs`, expand `MidiClipInstance` to full operational parity with `AudioClipInstance`:

```rust
pub struct MidiClipInstance {
    pub id: usize,
    pub track_index: usize,
    pub scene_index: usize,
    pub notes: Vec<MidiNoteInstance>,
    pub is_playing: bool,
    pub start_frame: u64,        // Absolute sample position when triggered
    pub start_absolute: u64,     // Unwrapped transport position for legato sync
    pub duration_ticks: u64,     // Natural clip length in ticks
    pub clip_loop_enabled: bool,
    pub clip_loop_start: u64,    // Loop start in ticks
    pub clip_loop_end: u64,      // Loop end in ticks
    pub is_matrix_clip: bool,
    pub last_scheduled_tick: Option<u64>,
}
```

### 5.2 RT-Safe Audio Thread Scheduling in `AudioEngine::process()`
During `AudioEngine::process(&mut self, out_buffer: &mut AudioBuffer<'_>)`:

```
┌────────────────────────────────────────────────────────────────────────┐
│                   AudioEngine::process() Callback                      │
│                                                                        │
│  Transport: frame_start .. frame_start + buffer_frames                 │
│  Ticks:     tick_window = [samples_to_ticks(f_start),                  │
│                            samples_to_ticks(f_start + buf_len))        │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │
               For each playing MidiClipInstance in track:
                                    │
                                    ▼
       ┌─────────────────────────────────────────────────────────┐
       │ 1. Resolve Clip Tick Window                             │
       │    - Elapsed frames: abs_start - clip.start_absolute    │
       │    - Handle Loop Wrap if loop_enabled                   │
       │      (loop_start_tick .. loop_end_tick)                 │
       │    - If past duration and !loop_enabled ->              │
       │      clip.is_playing = false; release active notes      │
       └────────────────────────────┬────────────────────────────┘
                                    │
                                    ▼
       ┌─────────────────────────────────────────────────────────┐
       │ 2. Detect & Schedule Note Events (Zero Allocations)     │
       │    - NoteOn:  start_tick in tick_window                 │
       │               compute exact frame_offset within buffer  │
       │    - NoteOff: end_tick in tick_window                   │
       │               compute exact frame_offset within buffer  │
       └────────────────────────────┬────────────────────────────┘
                                    │
                                    ▼
       ┌─────────────────────────────────────────────────────────┐
       │ 3. Dispatch to Track Voice Targets                      │
       │    - If OpenDMS track:                                  │
       │      NoteOn  -> dms_voice_pool.trigger(...)             │
       │      NoteOff -> dms_voice_pool.release_pad(...)         │
       │    - If Synth / Plugin track:                           │
       │      Push to track MidiEvent ring buffer / slice        │
       └─────────────────────────────────────────────────────────┘
```

#### RT-Safety Invariants:
1. **Zero Allocations**: Never construct dynamic `Vec<MidiEvent>` inside `process()`. Iterate `clip.notes` by reference.
2. **Sub-Buffer Sample Accuracy**:
   For a note starting at `start_tick`:
   $$\text{note\_frame} = \text{ticks\_to\_samples}(\text{start\_tick})$$
   $$\text{frame\_offset} = \text{note\_frame}.\text{saturating\_sub}(\text{elapsed\_start}).\min(\text{buffer\_frames} - 1)$$
   The event triggers at the exact sample offset within the audio block.
3. **Loop Wraparound Note Triggering**:
   When the buffer crosses the loop boundary (`loop_end_tick`), notes starting at `loop_start_tick` are triggered at the exact sub-buffer frame where wrap occurs.

### 5.3 Unified Triggering & Voice State
1. In `trigger_clip(track_index, scene_index)`:
   Check both `self.clips` and `self.midi_clips`. Activating a MIDI clip mutes any active audio clips on that track, and vice versa (exclusive per-track slot playback).
2. In `voice_active(track_index, scene_index)`:
   Return `true` if either audio or MIDI clip is playing.
3. In `voice_playhead_frame(track_index, scene_index)`:
   Return `(current_tick, total_ticks)` mapped to frames so GUI slot progress bars sweep accurately.

---

## 6. Actionable Recommendations for Worker

### 6.1 `crates/hikaru_core`
- **File**: `crates/hikaru_core/Cargo.toml`
  - Add `serde = { version = "1", default-features = false, features = ["derive"], optional = true }`
  - Add `[features] default = ["serde"], serde = ["dep:serde"]`
- **File**: `crates/hikaru_core/src/midi_note.rs` (New file)
  - Define `MidiNoteInstance` (16 bytes, `Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize`).
  - Implement `new`, `end_tick`, `contains_tick`, `overlaps_range`, `From<(u64, u8, u8, u32)>`, `From<MidiNoteInstance> for (u64, u8, u8, u32)`.
- **File**: `crates/hikaru_core/src/lib.rs`
  - Export `pub mod midi_note; pub use midi_note::MidiNoteInstance;`.

### 6.2 `crates/hikaru_sequencer`
- **File**: `crates/hikaru_sequencer/src/clip.rs`
  - Import `hikaru_core::MidiNoteInstance`.
  - Define `ClipMidiTimeline` with `notes: Vec<MidiNoteInstance>` and `total_ticks: u64`.
  - Define `ClipContentType { Audio, Midi }`.
  - Add fields to `Clip`: `content_type`, `midi_timeline`, `loop_start_tick`, `loop_end_tick`, `total_ticks`.
  - Add constructor `Clip::new_midi(id: u32, total_ticks: u64) -> Self`.
- **File**: `crates/hikaru_sequencer/src/quantizer.rs`
  - Fix `QuantizationEngine::samples_until_next_sync` to use `current_pos.beats_per_bar`.
  - Add `QuantizationEngine::ticks_until_next_sync(current_tick, grid, beats_per_bar, ppqn) -> u64`.

### 6.3 `crates/hikaru_transport`
- **File**: `crates/hikaru_transport/src/transport_state.rs`
  - Optimize `ticks_to_samples` and `samples_to_ticks` with single-division formulation to minimize float roundoff.
  - Add unit test proving zero drift over $10^7$ ticks across multiple tempos (120, 135, 174 BPM) and sample rates (44.1 kHz, 48 kHz, 96 kHz).

### 6.4 `crates/hikaru_audio_engine`
- **File**: `crates/hikaru_audio_engine/src/lib.rs`
  - Replace local `MidiNoteInstance` with `hikaru_core::MidiNoteInstance`.
  - Expand `MidiClipInstance` with `id`, `start_frame`, `start_absolute`, `duration_ticks`, `clip_loop_enabled`, `clip_loop_start`, `clip_loop_end`, `is_matrix_clip`.
  - Wire `trigger_clip` and `trigger_scene` to activate `midi_clips` alongside `clips`.
  - In `AudioEngine::process()`, implement real-time MIDI note scheduling loop with zero allocations.
  - Dispatch note events to `dms_voice_pool` (OpenDMS) and `DspPluginNode` (instruments/plugins).
  - Update `voice_active`, `voice_playhead_frame`, and `stop_track` to query both `clips` and `midi_clips`.

### 6.5 `crates/hikaru_gui`
- **File**: `crates/hikaru_gui/src/audio_proxy.rs`
  - Remove duplicate `MidiNoteInstance`; use `hikaru_core::MidiNoteInstance`.
  - Update `UpdateMidiClipNotes` to use `Vec<MidiNoteInstance>`.
- **File**: `crates/hikaru_gui/src/views/piano_roll.rs`
  - Unify `MidiNote` to `hikaru_core::MidiNoteInstance` (adapting `duration_ticks` from `u64` to `u32`).
- **File**: `crates/hikaru_gui/src/views/matrix.rs`
  - Change `ClipData::Midi { notes }` from tuples to `Vec<MidiNoteInstance>`.
- **File**: `crates/hikaru_gui/src/main.rs`
  - Simplify `UpdateMidiClipNotes` command handler (no tuple translation needed).

---

## 7. Verification & Anti-Regression Invariants

### 7.1 Verified Baseline Execution
Verified via isolated target build (`/tmp/hikaru_test_m1_exp2`):
- `hikaru_audio_engine` unit tests: **43 passed; 0 failed** (2.11s)
- `tests/convergence_test.rs`: **1 passed; 0 failed** (0.00s)
- `tests/ghost_voice_repro.rs`: **5 passed; 0 failed** (1.77s)
- `tests/mixer_track_routing.rs`: **3 passed; 0 failed** (0.00s)
- `tests/plugin_track_routing.rs`: **2 passed; 0 failed** (18.65s)
- **Total**: **54 tests passed; 0 failed**. Baseline integrity confirmed.

### 7.2 Independent Verification Steps for Implementation
1. `cargo check --workspace`
2. `cargo test -p hikaru_core -p hikaru_transport -p hikaru_sequencer -p hikaru_audio_engine`
3. Targeted tests:
   - `test_midi_note_instance_size_and_alignment` (assert size == 16 bytes)
   - `test_transport_clock_zero_drift`
   - `test_quantization_tick_precision`
   - `test_audio_engine_midi_clip_playback`
