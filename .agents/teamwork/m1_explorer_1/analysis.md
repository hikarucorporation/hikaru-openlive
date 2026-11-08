# Analysis: Audio Engine Real-Time Scheduling Strategy (Milestone 1)

**Author**: M1 Explorer 1 (Audio Engine Real-Time Scheduling Strategy)  
**Date**: 2026-11-08  
**Scope**: `crates/hikaru_audio_engine`, `crates/hikaru_core`, `crates/hikaru_transport`, `crates/hikaru_dsp`, and Worker command handling in `crates/hikaru_gui`.

---

## 1. Executive Summary & Scope

Hikaru OpenLive is designed to deliver Bitwig-grade production workflows across both the Arranger Timeline (`OpenStudio` mode) and Session Matrix Clip Launcher (`OpenLive` mode). 

Prior to Milestone 1, the audio engine (`crates/hikaru_audio_engine`) contained a dormant MIDI clip vector (`pub midi_clips: Vec<MidiClipInstance>`) that was **completely omitted from `AudioEngine::process()`**. The audio callback solely processed audio clips and one-shot drum triggers. Furthermore, `trigger_clip()` and `trigger_scene()` only iterated `self.clips`, leaving MIDI clips unable to play, loop, or trigger instruments.

This document presents the comprehensive architectural blueprint for Milestone 1:
1. **Iterating and Scheduling `self.midi_clips`** in `AudioEngine::process()` with sample-accurate Note-On and Note-Off events for both `OpenStudio` and `OpenLive` engine modes.
2. **Multi-Target Instrument Dispatch**: routing MIDI note events to both `OpenDMS` (`dms_voice_pool`), external VST3/CLAP plugins (`DspPluginNode`), and internal polyphonic synth voices.
3. **Strict Audio Thread Invariant Guarantees**: zero heap allocations, lock-free callback safety, FTZ/DAZ denormal protection, integer beat-quantized loop wrapping (960 PPQN), and categorical zero voice leakage past clip or loop boundaries.
4. **Concrete Structural Diffs**: exact struct enhancements for `MidiClipInstance`, `AudioEngine`, and the background `Worker` thread in `crates/hikaru_gui/src/main.rs`.

---

## 2. Data Structure Architecture

### 2.1 Canonical `MidiNoteInstance` in `crates/hikaru_core`

Currently, four disjointed representations of MIDI notes exist in the repository. Milestone 1 establishes the single canonical definition in `crates/hikaru_core/src/midi_note.rs`:

```rust
// crates/hikaru_core/src/midi_note.rs
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidiNoteInstance {
    pub start_tick: u64,
    pub pitch: u8,
    pub velocity: u8,
    pub duration_ticks: u32,
}

impl MidiNoteInstance {
    #[inline]
    pub fn end_tick(&self) -> u64 {
        self.start_tick.saturating_add(self.duration_ticks as u64)
    }
}
```

### 2.2 Comprehensive `MidiClipInstance` in `crates/hikaru_audio_engine`

The current `MidiClipInstance` in `crates/hikaru_audio_engine/src/lib.rs:30-35` is minimal:
```rust
// Existing minimal struct:
pub struct MidiClipInstance {
    pub track_index: usize,
    pub scene_index: usize,
    pub notes: Vec<MidiNoteInstance>,
    pub is_playing: bool,
}
```

It must be expanded to provide full feature parity with `AudioClipInstance`:

```rust
// Enhanced MidiClipInstance in crates/hikaru_audio_engine/src/lib.rs
use hikaru_core::MidiNoteInstance;
use hikaru_transport::TransportPosition;

#[derive(Clone, Debug)]
pub struct MidiClipInstance {
    pub id: usize,
    pub track_index: usize,
    pub scene_index: usize,
    /// Notes pre-sorted by `start_tick` on the worker thread.
    pub notes: Vec<MidiNoteInstance>,
    
    // Playback state
    pub is_playing: bool,
    pub is_matrix_clip: bool,
    
    // Timing & Reference Clocks
    pub start_frame: u64,          // Reference frame (timeline offset or legato start)
    pub start_absolute: u64,       // Linear absolute frame when launched in OpenLive
    pub duration_ticks: u64,       // Total clip duration in 960 PPQN ticks
    
    // Looping Parameters (in musical ticks)
    pub clip_loop_enabled: bool,
    pub clip_loop_start_tick: u64, // Loop start in ticks
    pub clip_loop_end_tick: u64,   // Loop end in ticks
    
    // Real-Time Playback Cursors
    pub prev_tick: u64,            // Last processed tick (for boundary detection)
    pub prev_frame: usize,         // Last relative frame (for loop wrap detection)
}

impl MidiClipInstance {
    /// Natural duration in ticks computed from the notes if not explicitly set.
    pub fn natural_ticks(&self) -> u64 {
        if self.duration_ticks > 0 {
            return self.duration_ticks;
        }
        let max_note_end = self.notes.iter()
            .map(|n| n.end_tick())
            .max()
            .unwrap_or(0);
        // Round up to nearest bar (3840 ticks) or minimum 1 bar
        let bar = 3840u64;
        if max_note_end == 0 {
            bar
        } else {
            ((max_note_end + bar - 1) / bar) * bar
        }
    }

    /// Checks if the clip loop is valid and configured.
    #[inline]
    pub fn has_valid_clip_loop(&self) -> bool {
        self.clip_loop_enabled && self.clip_loop_end_tick > self.clip_loop_start_tick
    }

    /// Loop length in ticks, quantized to integer beats (960 PPQN).
    pub fn quantized_loop_len_ticks(&self) -> u64 {
        let raw_len = self.clip_loop_end_tick.saturating_sub(self.clip_loop_start_tick);
        let ppqn = 960u64;
        if raw_len < ppqn {
            // Sub-beat stutter loops are preserved as-is
            raw_len.max(1)
        } else {
            // Quantize down to nearest beat multiple
            let beats = raw_len / ppqn;
            (beats * ppqn).max(1)
        }
    }

    /// Emission duration in sample frames based on active tempo.
    #[inline]
    pub fn emission_len_frames(&self, transport: &TransportPosition) -> u64 {
        transport.ticks_to_samples(self.natural_ticks())
    }
}
```

### 2.3 Active Voice & Note Tracking Table

To guarantee **zero voice leakage** (never leaving hanging notes or ghost voices when loops wrap or clips stop), `AudioEngine` must maintain a fixed-capacity active note registry:

```rust
pub const MAX_ACTIVE_NOTES_PER_TRACK: usize = 16;
pub const MAX_TRACKS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActiveNoteState {
    pub pitch: u8,
    pub velocity: u8,
    pub channel: u8,
    pub off_tick: u64,
    pub clip_id: usize,
    pub is_active: bool,
}

impl ActiveNoteState {
    pub const EMPTY: Self = Self {
        pitch: 0,
        velocity: 0,
        channel: 0,
        off_tick: 0,
        clip_id: 0,
        is_active: false,
    };
}
```

---

## 3. Real-Time Scheduling Engine Pipeline (`AudioEngine::process`)

### 3.1 Execution Flow Overview

Inside `AudioEngine::process(&mut self, out_buffer: &mut AudioBuffer<'_>)`, MIDI scheduling executes in synchronous lock-step with audio clip processing:

```
┌────────────────────────────────────────────────────────────────────────┐
│ AudioEngine::process(&mut self, out_buffer)                            │
├────────────────────────────────────────────────────────────────────────┤
│ 1. Clear output buffer & process PreviewPlayer                         │
│ 2. If Playing:                                                         │
│    a. Snapshot transport: frame_start, abs_start, BPM, SR              │
│    b. Process self.clips (existing audio clip pipeline)                │
│    c. Process self.midi_clips (NEW: MIDI Scheduling Stage)             │
│       ├── Mode Check: OpenStudio (Timeline) vs OpenLive (Matrix)       │
│       ├── Window Mapping: [sample_start, sample_end) -> [tick0, tick1) │
│       ├── Detect Note-On events (start_tick in window)                 │
│       ├── Detect Note-Off events (end_tick in window or active notes)  │
│       ├── Loop Wrap Evaluation (Quantized wrap & Voice Cut/Kill)       │
│       └── Dispatch to:                                                 │
│           ├── OpenDMS Voice Pool (dms_voice_pool)                      │
│           ├── Track External Plugins (track_effects[t])                │
│           └── Polyphonic Synth Voices (synth_voice_pool)               │
│    d. Process Track Effects Rack & sum track_output_bufs               │
│    e. Advance Transport Clocks (sample_count, absolute_frame)          │
│ 3. FTZ/DAZ Flush & Master Tanh Saturation Limiter                      │
│ 4. Store Atomic Clocks & VU Meter Peaks                                │
└────────────────────────────────────────────────────────────────────────┘
```

### 3.2 Time-Base Mathematical Foundations

1. **PPQN Authority**: `DEFAULT_PPQN = 960` ticks per quarter note (beat).
2. **Tempo Conversions**:
   $$\text{seconds\_per\_tick} = \frac{60.0}{\text{BPM} \times 960}$$
   $$\text{samples\_to\_ticks}(\text{samples}) = \text{round}\left(\frac{\text{samples}}{\text{SR} \times \text{seconds\_per\_tick}}\right)$$
   $$\text{ticks\_to\_samples}(\text{ticks}) = \text{round}\left(\text{ticks} \times \text{seconds\_per\_tick} \times \text{SR}\right)$$
3. **Sub-Buffer Offset Calculation**:
   For any note starting at `note_tick`, its sample position within the global audio stream is:
   $$\text{note\_sample} = \text{transport.ticks\_to\_samples}(\text{note\_tick})$$
   Within an audio buffer spanning $[\text{buf\_start\_sample}, \text{buf\_start\_sample} + \text{buffer\_frames})$:
   $$\text{sub\_frame\_offset} = \text{note\_sample}.saturating\_sub(\text{buf\_start\_sample}) \text{ as usize}$$
   where $\text{sub\_frame\_offset} < \text{buffer\_frames}$.

### 3.3 Scheduling Logic: OpenStudio (Arranger Timeline)

In `EngineMode::OpenStudio`:
- The timeline playhead advances monotonically through `transport.sample_count`.
- Global loop wrapping is evaluated per sample frame via `transport.wrap_sample_count(frame)`.
- For each `clip` in `self.midi_clips`:
  ```rust
  let clip_frame_start = clip.start_frame;
  let clip_frame_end = clip_frame_start.saturating_add(transport.ticks_to_samples(clip.natural_ticks()));
  
  for f in 0..buffer_frames as usize {
      let global_frame = transport.wrap_sample_count(frame_start + f as u64);
      if global_frame < clip_frame_start || global_frame >= clip_frame_end {
          continue;
      }
      let relative_frame = global_frame - clip_frame_start;
      
      // If clip-level looping is active on the timeline clip:
      let effective_frame = if clip.has_valid_clip_loop() {
          let loop_start_f = transport.ticks_to_samples(clip.clip_loop_start_tick);
          let loop_len_f = transport.ticks_to_samples(clip.quantized_loop_len_ticks());
          let loop_end_f = loop_start_f + loop_len_f;
          if relative_frame < loop_end_f {
              relative_frame
          } else {
              loop_start_f + ((relative_frame - loop_end_f) % loop_len_f)
          }
      } else {
          relative_frame
      };
      
      let current_tick = transport.samples_to_ticks(effective_frame);
      // Process note events falling at boundary current_tick
  }
  ```

### 3.4 Scheduling Logic: OpenLive (Session Matrix)

In `EngineMode::OpenLive`:
- Tracks play clips independently. Time for each clip is measured relative to its trigger:
  $$\text{elapsed\_start} = \text{abs\_start}.saturating\_sub(\text{clip.start\_absolute})$$
  $$\text{elapsed\_end} = \text{elapsed\_start} + \text{buffer\_frames}$$
- **Non-Looping Clips**:
  $$\text{emission\_frames} = \text{clip.emission\_len\_frames}(\&transport)$$
  If $\text{elapsed\_start} \ge \text{emission\_frames}$:
  - `clip.is_playing = false;`
  - Immediately kill all active notes belonging to this clip (`self.kill_track_midi_notes(clip.track_index)`).
  - Skip further processing.
- **Looping Clips (Seamless Beat-Quantized Loop)**:
  `loop_start_f = transport.ticks_to_samples(clip.clip_loop_start_tick);`
  `loop_len_f = transport.ticks_to_samples(clip.quantized_loop_len_ticks());`
  `loop_end_f = loop_start_f + loop_len_f;`

  A buffer crosses the loop wrap point when:
  $$\text{elapsed\_start} < \text{loop\_end\_f} \quad \text{and} \quad \text{elapsed\_end} \ge \text{loop\_end\_f}$$
  or in subsequent cycles:
  $$\text{relative\_frame\_end} < \text{relative\_frame\_start}$$
  
  When a wrap is detected at sub-frame sample index $W$:
  1. **Pre-wrap segment $[0, W)$**: evaluate events up to `clip.clip_loop_end_tick`.
  2. **Wrap instant $W$**:
     - All notes currently active from the previous loop cycle that do not legally continue are issued a **Note-Off** at frame $W$.
     - Reset clip tick cursor to `clip.clip_loop_start_tick`.
  3. **Post-wrap segment $[W, \text{buffer\_frames})$**: evaluate Note-On events starting at or after `clip.clip_loop_start_tick`.

### 3.5 Dispatch Targets

For each generated Note event on track $T$:

1. **Target 1: OpenDMS (`dms_voice_pool`)**:
   If the track contains OpenDMS drum pads:
   - On Note-On:
     ```rust
     let pad_idx = note.pitch as usize;
     let vel_norm = (note.velocity as f32) / 127.0;
     self.trigger_dms_note(pad_idx, 1.0, 0.0, vel_norm, 1.0, &DEFAULT_DMS_ADSR, T);
     ```
   - On Note-Off:
     ```rust
     self.release_dms_note_for_track(T, note.pitch as usize);
     ```

2. **Target 2: External VST3 / CLAP Plugins (`track_effects[T]`)**:
   Collect `MidiEvent` triplets into the preallocated buffer `track_midi_events[T]`:
   - Note-On: `MidiEvent { status: 0x90 | (channel & 0x0F), data1: pitch, data2: velocity }`
   - Note-Off: `MidiEvent { status: 0x80 | (channel & 0x0F), data1: pitch, data2: 0 }`
   - Passed directly to plugin nodes during the track effects pass:
     ```rust
     for node in self.track_effects[T].iter_mut() {
         if node.is_active {
             node.process(&mut audio_buffer, &track_midi_events[T][..event_count]);
         }
     }
     ```

3. **Target 3: Internal Polyphonic Synth Voice Pool (`SynthVoicePool`)**:
   For melodic tracks without external plugins or drum kits, an integrated 16-voice polyphonic synth pool generates audio directly into `self.track_output_bufs[T]`:
   - Note-On: voice allocates from pool, frequency computed via $f = 440.0 \times 2^{(pitch - 69)/12}$, starts Attack envelope.
   - Note-Off: voice transitions to Release envelope.
   - Upon envelope completion, voice marks `is_active = false`.

---

## 4. Invariant Verification & Audio Thread Hardening

| # | Invariant | Threat / Failure Mode | Hardening Mechanism in Milestone 1 |
|---|---|---|---|
| 1 | **Zero Allocation** | Dynamic allocations (`Vec::push`, `Box`, `vec![]`, `clone`) during `process()` cause memory allocator lock contention and kernel page faults, leading to PipeWire/ALSA buffer xruns. | 1. `track_midi_events` preallocated as fixed stack/struct arrays `[MidiEvent; 128]`.<br>2. Fixed array `active_notes: [[ActiveNoteState; 16]; 16]` eliminates dynamic note allocation.<br>3. Fix existing allocation bugs: replace `let mut dms_l = vec![0.0f32; buf_frames]` with engine-level scratch vectors.<br>4. Replace `track_buf.clone()` in plugin insert with preallocated `fx_scratch_buf`. |
| 2 | **Lock-Free Processing** | Holding Mutexes across the audio thread while decoding WAVs or saving JSON blocks the real-time callback. | 1. Real-time parameters (`volumes`, `pans`, `mutes`, `master_gain`) remain lock-free `AtomicU32` / `AtomicBool`.<br>2. The background worker performs note parsing and sorting outside the engine mutex.<br>3. Mutex acquisition by the worker is strictly bounded to pointer/slice updates (sub-microsecond). |
| 3 | **FTZ / DAZ Denormals** | Recursive IIR filters, synth voice envelopes, or reverb tails decaying toward zero can produce subnormal floats, triggering x86 CPU microcode stalls (100x CPU spike). | 1. `enable_ftz_daz()` sets MXCSR bits (DAZ bit 6, FTZ bit 15).<br>2. `flush_denormals(samples)` clears denormals before the master soft-saturator.<br>3. Envelope levels are clamped to zero when falling below $10^{-5}$. |
| 4 | **Beat-Quantized Loop Wrapping** | Fractional loop lengths cause phase drift across successive iterations, desynchronizing parallel tracks over long performances. | 1. All MIDI loop bounds (`clip_loop_start_tick`, `clip_loop_end_tick`) are integer multiples of 960 PPQN ticks.<br>2. `quantized_loop_len_ticks` enforces beat-multiple boundaries.<br>3. Mathematical tick math eliminates floating-point accumulation drift entirely. |
| 5 | **Zero Voice Leakage** | Non-looping clips past end, clip deactivations, scene changes, or transport stops leaving hung notes or non-zero DC bias (violating `tests/ghost_voice_repro.rs`). | 1. Emission boundary strictly deactivates clip: `clip.is_playing = false`.<br>2. Loop wrap issues immediate Note-Off for pre-wrap notes.<br>3. `stop()`, `stop_track()`, and `trigger_scene()` issue immediate `AllNotesOff` and clear voice pools.<br>4. Verification requires $- \infty\text{ dB}$ (0.0 sample output) past clip end. |

---

## 5. Worker & AudioProxy Adjustments

### 5.1 Command Handling in `crates/hikaru_gui/src/main.rs`

The background Worker event loop receives `GuiCommand` messages and manages `AudioEngine`. The following modifications are required:

1. **`GuiCommand::TriggerClip { track_idx, scene_idx }`**:
   Currently calls `engine.trigger_clip(track_idx, scene_idx)`, which only triggers `self.clips`.
   **Adjustment**: Update `AudioEngine::trigger_clip` to search both `self.clips` and `self.midi_clips`. When triggering a track:
   - Deactivate any currently playing clip (audio or MIDI) on that track.
   - Toggle `is_playing` on the target MIDI clip.
   - Inherit legato reference phase (`start_frame`, `start_absolute`) or set to current time.

2. **`GuiCommand::TriggerScene { scene_idx }`**:
   Currently calls `engine.trigger_scene(scene_idx)`, which only triggers `self.clips`.
   **Adjustment**: Update `AudioEngine::trigger_scene` to activate both audio and MIDI clips belonging to `scene_idx` with beat-synchronized phase restart (`start_frame = now`, `start_absolute = now_abs`), deactivating clips in affected tracks on other scenes.

3. **`GuiCommand::UpdateMidiClipNotes { track_idx, scene_idx, notes }`**:
   **Adjustment**:
   - Pre-sort the incoming `notes` vector by `start_tick` on the worker thread *before* acquiring the engine lock.
   - Calculate clip `duration_ticks` from notes if unset.
   - Update `MidiClipInstance` in `self.midi_clips`.

4. **`GuiCommand::SetClipLoop { track_idx, scene_idx, start_secs, end_secs, enabled }`**:
   **Adjustment**:
   Add `GuiCommand::SetMidiClipLoop` or extend `SetClipLoop`:
   - Set `clip_loop_start_tick` and `clip_loop_end_tick` on the matching `MidiClipInstance`.

5. **`GuiCommand::SyncPlaylistClips { clips }`**:
   **Adjustment**:
   Extend to support Arranger timeline MIDI clips (`ClipType::Midi` / `ClipType::Pattern`). Convert start ticks to frames (`transport.ticks_to_samples(clip.start_tick)`) and populate `engine.midi_clips` with `is_matrix_clip = false`.

---

## 6. Concrete Implementation Blueprint

### 6.1 Diffs for `crates/hikaru_audio_engine/src/lib.rs`

```rust
// 1. In crates/hikaru_audio_engine/src/lib.rs:
// Replace minimal MidiClipInstance with full struct:
pub struct MidiClipInstance {
    pub id: usize,
    pub track_index: usize,
    pub scene_index: usize,
    pub notes: Vec<MidiNoteInstance>,
    pub is_playing: bool,
    pub is_matrix_clip: bool,
    pub start_frame: u64,
    pub start_absolute: u64,
    pub duration_ticks: u64,
    pub clip_loop_enabled: bool,
    pub clip_loop_start_tick: u64,
    pub clip_loop_end_tick: u64,
    pub prev_tick: u64,
    pub prev_frame: usize,
}

// 2. Add Active Note and Scratch fields to AudioEngine:
pub struct AudioEngine<'a> {
    // ... existing fields ...
    pub midi_clips: Vec<MidiClipInstance>,
    pub active_midi_notes: [[Option<ActiveNoteState>; MAX_ACTIVE_NOTES_PER_TRACK]; 16],
    dms_scratch_l: Vec<f32>,
    dms_scratch_r: Vec<f32>,
    fx_scratch_buf: Vec<f32>,
    track_midi_events: [[MidiEvent; 128]; 16],
    track_midi_counts: [usize; 16],
}

// 3. Update trigger_clip to handle both audio and MIDI:
pub fn trigger_clip(&mut self, track_index: usize, scene_index: usize) {
    let now = self.transport.sample_count;
    let now_abs = self.absolute_frame;
    
    // Check if target is playing in audio clips OR midi clips
    let audio_target = self.clips.iter().find(|c| c.track_index == track_index && c.scene_index == scene_index);
    let midi_target = self.midi_clips.iter().find(|c| c.track_index == track_index && c.scene_index == scene_index);
    let target_was_playing = audio_target.map(|c| c.is_playing).unwrap_or(false) 
        || midi_target.map(|c| c.is_playing).unwrap_or(false);
        
    let legato_origin = if target_was_playing {
        None
    } else {
        self.legato_reference(Some(track_index))
    };
    
    // Stop all active notes on this track immediately
    self.kill_track_midi_notes(track_index);
    
    for clip in self.clips.iter_mut().filter(|c| c.track_index == track_index) {
        if clip.scene_index == scene_index {
            clip.is_playing = !target_was_playing;
            if clip.is_playing {
                if let Some((ref_frame, ref_abs)) = legato_origin {
                    clip.start_frame = ref_frame;
                    clip.start_absolute = ref_abs;
                } else {
                    clip.start_frame = now;
                    clip.start_absolute = now_abs;
                }
                clip.prev_frame = usize::MAX;
                clip.xfade_remaining = 0;
                clip.xfade_len = 0;
            }
        } else {
            clip.is_playing = false;
        }
    }
    
    for clip in self.midi_clips.iter_mut().filter(|c| c.track_index == track_index) {
        if clip.scene_index == scene_index {
            clip.is_playing = !target_was_playing;
            if clip.is_playing {
                if let Some((ref_frame, ref_abs)) = legato_origin {
                    clip.start_frame = ref_frame;
                    clip.start_absolute = ref_abs;
                } else {
                    clip.start_frame = now;
                    clip.start_absolute = now_abs;
                }
                clip.prev_frame = usize::MAX;
                clip.prev_tick = 0;
            }
        } else {
            clip.is_playing = false;
        }
    }
}
```

---

## 7. Verification Method & Test Strategy

To verify this implementation independently without regressions:

1. **Compilation Check**:
   ```bash
   cargo check --workspace
   ```
2. **Audio Engine Unit & Integration Test Suite**:
   ```bash
   cargo test -p hikaru_audio_engine
   cargo test -p hikaru_transport
   cargo test -p hikaru_sequencer
   ```
3. **Anti-Leakage Ghost Voice Regression Verification**:
   Execute `tests/ghost_voice_repro.rs` ensuring $- \infty\text{ dB}$ (0.0 sample output) past clip and loop boundaries:
   ```bash
   cargo test -p hikaru_audio_engine --test ghost_voice_repro
   ```
4. **New Dedicated MIDI Clip Integration Tests** (`tests/midi_clip_scheduling.rs`):
   - `test_midi_clip_trigger_and_note_emission`: verify Note-On triggers DMS pads and external plugins at accurate frame offsets.
   - `test_midi_clip_loop_wrap_no_leak`: verify active notes from loop 1 receive Note-Off at loop wrap and do not hang.
   - `test_midi_clip_emission_stop_silence`: verify non-looping MIDI clips categorical silence past emission end.
   - `test_midi_clip_zero_alloc`: verify process loop executes without any heap allocations.
