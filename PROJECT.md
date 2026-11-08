# Project: Hikaru OpenLive - Bitwig 1-to-1 MIDI Clips & Piano Roll

## Architecture
Hikaru OpenLive is a high-performance DAW and live performance host written in Rust.
The system is divided into modular crates:
- `crates/hikaru_core`: Core audio types, events, and buffer primitives.
- `crates/hikaru_transport`: Master musical clock, tempo, time signatures, PPQN (960 ticks/beat), tick-to-sample conversions.
- `crates/hikaru_sequencer`: Sequencer logic, clip state machine (Stopped, Queued, Playing, Stopping), clip quantization.
- `crates/hikaru_audio_engine`: Real-time audio engine, multi-track mixing, plugin hosting, voice scheduling, denormals flush, zero voice leakage.
- `crates/hikaru_openlive_file` & `crates/hikaru_openstudio_file`: JSON project file persistence (`.oplf` and `.opsf`).
- `crates/hikaru_render`: Offscreen WGPU renderer, procedural 3D meshes (knobs, wavetables).
- `crates/hikaru_gui`: Immediate-mode UI with `egui 0.31` + `wgpu 24.0.5` + Naga WGSL:
  - `ui/bitwig_wgpu.rs`: In-tree swapchain SDF render pass evaluating widget and note card primitives on GPU.
  - `ui/bitwig_kit.rs`, `ui/icons.rs`, `ui/meters.rs`, `ui/fonts.rs`: Lemon Studio design system assets.
  - `views/piano_roll.rs`: Bitwig-style interactive Piano Roll & Velocity Lane editor.
  - `views/matrix.rs`: Session Matrix clip launcher & Arranger timeline view.
  - `views/playlist.rs`: Arranger timeline clip models and rendering.
  - `audio_proxy.rs`: Lock-free bidirectional command/event ring buffer between GUI and audio engine.

```
+-------------------------------------------------------------------------------+
|                             hikaru_gui (egui + wgpu)                          |
|  [Matrix View]     [Playlist Arranger]       [Piano Roll & Velocity Lane]     |
|         │                   │                             │                   |
|         └───────────────────┴─────────────────────────────┘                   |
|                                     │                                         |
|                               AudioProxy                                      |
|                                     │                                         |
+─────────────────────────────────────┼─────────────────────────────────────────+
                                      │ Commands & Sync
+─────────────────────────────────────▼─────────────────────────────────────────+
|                        hikaru_audio_engine (Audio Thread)                     |
|  [Transport & Clock] ──► [Sequencer & Clips] ──► [Synth / Sampler / VST Node] |
|                                                        │                      |
|                                                  Master Output                |
+───────────────────────────────────────────────────────────────────────────────+
```

---

## Feature Inventory

| # | Feature | Description | Milestone | Source |
|---|---------|-------------|-----------|--------|
| 1 | Canonical MIDI Note & Clip Structs | Unified `MidiNoteInstance` and `MidiClipInstance` representation with 960 PPQN | M1 | Engine Survey |
| 2 | Audio Engine MIDI Note Scheduling | Real-time scheduling of MIDI note events inside `AudioEngine::process()` without allocations/locks | M1 | Engine Survey |
| 3 | Clip Triggering & Looping in Engine | `trigger_clip` and `trigger_scene` support for MIDI clips with quantized beat wrapping | M1 | Engine Survey |
| 4 | Persistence in `.oplf` (OpenLive) | Serializing and deserializing MIDI clips, notes, and loop boundaries in `.oplf` | M1 | Engine Survey |
| 5 | Persistence in `.opsf` (OpenStudio) | Schema extension supporting pattern/MIDI note sequences in Arranger timeline `.opsf` | M1 | Engine Survey |
| 6 | AudioProxy MIDI Command Dispatch | Ring-buffer commands (`UpdateMidiClipNotes`, `TriggerMidiClip`, `SetClipLoop`) | M2 | GUI/Engine Survey |
| 7 | Active Clip State Caching & Sync | Bi-directional binding between active clip in GUI state and engine playback instance | M2 | GUI Survey |
| 8 | Timeline Grid Coordinate Mapping | Mapping ticks to pixels and semitones to Y coordinates with zoom and scroll offsets | M3 | GUI/Spec Survey |
| 9 | 3D Piano Keyboard Sidebar | Shaded ivory/ebony keys with black key layout, octave gutter, and pitch names | M3 | Spec Frame 0037 |
| 10 | Octave & Pitch Labels | Displaying C1..C4 labels at octave divisions and note pitch names when zoomed in | M3 | Spec Frame 0037 |
| 11 | Key Hover & Playback Highlight | Illuminating piano keys on hover, audition click, or active note playback | M3 | Spec Frame 0038 |
| 12 | Piano Key Audition Click | Clicking piano keys plays sound audition via active track instrument | M3 | Spec Frame 0038 |
| 13 | Bar Ruler & Loop Markers | Bar/beat ruler with start/end loop marker triangles and colored region highlight | M3 | Spec Frame 0080 |
| 14 | Real-time Playhead Sweep | Vertical line sweeping across canvas during playback at current transport tick | M3 | Spec Frame 0096 |
| 15 | GPU SDF Note Card Rendering | Instanced SDF anti-aliased rounded note card rendering via `bitwig_wgpu.rs` | M3 | GUI Survey |
| 16 | Double-Click Note Creation | Double-clicking empty grid cell creates note at snapped tick and clicked pitch | M4 | Spec Frame 0071 |
| 17 | Single-Click Note Selection | Left-clicking note selects it; clears prior selection; highlights note with white border | M4 | Spec Frame 0076 |
| 18 | Multi-Select Toggle (Ctrl+Click) | Ctrl+Click toggles note in/out of selection | M4 | Spec Frame 0076 |
| 19 | Marquee Box Selection | Dragging across empty grid draws rubberband selection box | M4 | Spec Frame 0096 |
| 20 | Select All (`Ctrl+A`) | Selecting all notes belonging to the active clip | M4 | Spec Mining |
| 21 | Double-Click Note Deletion | Double-clicking existing note deletes it immediately | M4 | Spec Frame 0076 |
| 22 | Key Deletion (`Delete`/`Backspace`) | Pressing Delete or Backspace removes all selected notes | M4 | Spec Mining |
| 23 | Note Pitch & Time Drag | Dragging note body moves selected notes with pitch clamping (0..127) and grid snap | M4 | Spec Frame 0076 |
| 24 | Note Right Edge Resize (`[\|]`) | Dragging right edge adjusts duration with magnetic snap or Shift override | M4 | Spec Frame 0072 |
| 25 | Note Left Edge Resize (`[\|`) | Dragging left edge adjusts start and duration preserving note end tick | M4 | Spec Mining |
| 26 | In-Situ Velocity Drag (`Ctrl+Shift+Drag`) | Dragging note body with Ctrl+Shift modifies velocity directly (1..127) | M4 | Spec Frame 0076 |
| 27 | Shift Snapping Override | Holding Shift during note drag or resize disables grid snap (1-tick precision) | M4 | Spec Frame 0072 |
| 28 | Keyboard Transpose & Arrow Movement | Arrow keys transpose notes by semitone, octave (`Shift+Arrows`), or time step | M4 | Spec Mining |
| 29 | Velocity Lane Sub-Panel Toggle | Clicking `🎚` button opens/collapses bottom velocity editing lane | M4 | Spec Frame 0006 |
| 30 | Velocity Stems & Handles Display | Stems and round caps drawn in track color aligned with note start ticks | M4 | Spec Frame 0006 |
| 31 | Velocity Interactive Drag | Dragging handle or stem modifies note velocity | M4 | Spec Mining |
| 32 | Velocity Multi-Note Linear Ramp | Dragging across multiple handles draws linear interpolation slope | M4 | Spec Mining |
| 33 | Grid Snap Selector Dropdown | Dropdown selector: Off, 1/1, 1/2, 1/4, 1/8, 1/16, 1/32, Triplet modes | M4 | Spec Frame 0037 |
| 34 | Adaptive Grid Subdivisions | Grid dynamically subdivides or consolidates based on horizontal zoom | M4 | Spec Mining |
| 35 | Middle-Click Pan & Zoom | Middle-click dragging pans horizontally/vertically or zooms | M4 | Spec Frame 0015 |
| 36 | Ctrl+Alt Pan & Wheel Navigation | Ctrl+Alt+Drag pans; wheel scrolls V; Shift+Wheel scrolls H; Ctrl+Wheel zooms H | M4 | Spec Frame 0096 |
| 37 | Fit Viewport (`▶\|◀`) & Auto-Scroll | Zoom to fit clip notes/loop; follow playhead toggle | M4 | Spec Frame 0037 |
| 38 | Loop Marker Dragging & Sliding | Dragging loop start/end flags or loop body bar | M4 | Spec Frame 0080 |
| 39 | Keys vs Drum Pads Mode Switcher | Chromatic piano keyboard vs named drum pad grid (OpenDMS) | M4 | Spec Frame 0006 |
| 40 | Matrix Clip Launcher MIDI Previews | Slot cards render mini note rectangles in track color for MIDI clips | M5 | GUI Survey |
| 41 | Arranger Timeline `ClipType::Midi` | Timeline playlist supports MIDI clips with mini-note preview | M5 | GUI Survey |
| 42 | Double-Click Clip to Open Piano Roll | Double-clicking clip card in Matrix or Arranger opens and binds Detail Editor | M5 | Spec Frame 0036 |
| 43 | Detail Editor Dock Splitter Drag | Resizing bottom dock height via interactive splitter (`DRAG Move splitter`) | M5 | Spec Frame 0145 |
| 44 | Footer Note Editor Toggle Sync | Syncing `show_piano_roll` toggle in footer and detail editor panels | M5 | GUI Survey |
| 45 | E2E Regression & Anti-Glitch Invariants | Zero voice leakage, denormals flush, clean audio mix under continuous editing | M6 | E2E Testing Track |

---

## Milestones

| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| M1 | MIDI Data Model, Engine Scheduling & Project Persistence | Canonical note/clip structs, `AudioEngine::process()` MIDI scheduling, clip triggering/looping, `.oplf`/`.opsf` persistence | none | IN_PROGRESS |
| M2 | Audio Proxy & Bridge Synchronization | Lock-free command ring buffer (`UpdateMidiClipNotes`, `TriggerMidiClip`, `SetClipLoop`), bi-directional state caching between GUI and audio engine | M1 | PLANNED |
| M3 | Piano Roll Core View, Coordinate Geometry & Lemon Studio / WGPU Rendering | Viewport math, adaptive zoom/pan, 3D ivory/ebony piano keyboard with hover/playback highlight, ruler & loop markers, instanced GPU SDF note rendering (`bitwig_wgpu.rs`) | M2 | PLANNED |
| M4 | Piano Roll Interactive Editing & Velocity Lane (1-to-1 Bitwig) | Note lifecycle (create, select, marquee, delete), note manipulation (pitch/time drag, edge resize, in-situ velocity, Shift override, arrow keys), velocity lane (toggle, stems, handles, ramp), grid snapping | M3 | PLANNED |
| M5 | Matrix & Arranger MIDI Clip Integration & Detail Editor Dock Mounting | `ClipType::Midi` in Arranger and Matrix, mini-note previews, double-click clip to mount/open Piano Roll, resizable dock splitter (`DRAG Move splitter`), footer toggle sync | M4 | PLANNED |
| M6 | Final Verification, Full E2E Test Suite Pass & Adversarial Hardening | Pass 100% of E2E tests (Tiers 1-4), Tier 5 adversarial coverage hardening, zero audio regressions, clean forensic audit | M5 | PLANNED |

---

## Interface Contracts

### M1 ↔ M2 (Engine ↔ AudioProxy)
```rust
// Canonical Note & Clip definitions
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidiNoteInstance {
    pub start_tick: u64,
    pub pitch: u8,
    pub velocity: u8,
    pub duration_ticks: u32,
}

pub enum GuiCommand {
    UpdateMidiClipNotes {
        track_idx: usize,
        scene_idx: usize,
        notes: Vec<MidiNoteInstance>,
    },
    TriggerMidiClip {
        track_idx: usize,
        scene_idx: usize,
    },
    SetClipLoop {
        track_idx: usize,
        scene_idx: usize,
        loop_start_tick: u64,
        loop_end_tick: u64,
        loop_enabled: bool,
    },
}
```

### M2 ↔ M3 / M4 (AudioProxy ↔ PianoRollState)
```rust
// In PianoRollState
pub struct PianoRollState {
    pub active_track_idx: Option<usize>,
    pub active_scene_idx: Option<usize>,
    pub active_clip_is_timeline: bool,
    pub notes: Vec<MidiNoteInstance>,
    pub selected_notes: HashSet<usize>,
    pub loop_start_tick: u64,
    pub loop_end_tick: u64,
    pub loop_enabled: bool,
    pub zoom_x: f32,
    pub key_height: f32,
    pub scroll_offset: egui::Vec2,
    pub snap_division: SnapDivision,
    pub velocity_lane_open: bool,
    pub audition_enabled: bool,
    // Interaction state fields...
}
```

### M3 / M4 ↔ M5 (Piano Roll ↔ Matrix & Arranger Timeline)
```rust
// Double-click clip handler in Matrix / Playlist:
fn open_clip_in_detail_editor(
    state: &mut AppState,
    track_idx: usize,
    clip_id: ClipIdentifier,
    notes: &[MidiNoteInstance],
    loop_start_tick: u64,
    loop_end_tick: u64,
);
```

---

## Code Layout

- `crates/hikaru_core/src/`:
  - `midi_note.rs`: Canonical `MidiNoteInstance` definition shared across engine, persistence, and GUI.
- `crates/hikaru_audio_engine/src/`:
  - `lib.rs`: MIDI clip scheduling inside `AudioEngine::process()`, `trigger_clip` integration.
- `crates/hikaru_openlive_file/src/lib.rs` & `crates/hikaru_openstudio_file/src/lib.rs`:
  - Serialization / deserialization of MIDI clips and notes.
- `crates/hikaru_gui/src/`:
  - `audio_proxy.rs`: GuiCommand dispatch for MIDI note and clip updates.
  - `ui/bitwig_wgpu.rs`: WGSL SDF instanced shader note card primitive.
  - `views/piano_roll.rs`: Complete 1-to-1 Bitwig piano roll view, velocity lane, interaction state machine.
  - `views/matrix.rs`: Session Matrix clip launcher MIDI clip slot rendering and double-click trigger.
  - `views/playlist.rs`: Arranger timeline `ClipType::Midi` integration and mini-note preview.
  - `app.rs`: Bottom dock detail editor layout and resizable splitter.
  - `views/footer.rs`: Footer button toggle synchronization.
