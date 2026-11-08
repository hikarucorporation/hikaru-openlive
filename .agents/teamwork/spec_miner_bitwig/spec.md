# Clean-Room Behavioral & Visual Specification: Bitwig Studio 5.1.7 Piano Roll & MIDI Clips

> **Authoritative Specification Document**  
> **Source Evidence:**  
> - Reference Video: `.opencode/references/bitwig_5.1.7/screenshots/video/bitwig-comportamiento-piano-roll-1.mp4` (190.5s duration, 1092×614 @ 30fps)
> - Reference Captures: `.opencode/references/bitwig_5.1.7/screenshots/bitwig-footer/` (bitwig-footer-1.png, 2.png, 3.png), `bitwig-clip-launcher.png`, `bitwig-playlist.png`
> - Codebase & In-Tree Shaders: `crates/hikaru_gui/src/ui/bitwig_wgpu.rs` (WGSL SDF Shader), `crates/hikaru_gui/src/views/piano_roll.rs`, `crates/hikaru_render`

---

## 1. Executive Summary & Topology

In Bitwig Studio 5.1.7, the Note Editor (Piano Roll) operates in two topologically unified modes:
1. **Docked Detail Editor Panel (`Bottom Dock`)**:
   - Resides below the central Clip Launcher / Arranger Timeline / Mix console.
   - Opens automatically when a MIDI clip is double-clicked in either the Session Matrix (`matrix.rs`) or the Arranger Timeline (`playlist.rs`).
   - Can be toggled manually via the **Detail Editor Panel icon** (`❖` / multi-dot icon) or Note Editor icon (`🎹`) in the footer bar.
   - Separated by an interactive horizontal splitter (`#242424` line with `DRAG Move splitter` cursor `↕`).
2. **Full Edit Workspace View (`StatusViewTab::Edit`)**:
   - Triggered by clicking `EDIT` in the bottom view-mode switcher (`ARRANGE | MIX | EDIT`).
   - The Piano Roll expands to occupy the central workspace with maximum vertical octave range and horizontal timeline view.

The Piano Roll is organized into six primary spatial zones:
```
+---------------------------------------------------------------------------------------------------+
| [Vertical Track Strip] | [Octave Strip + Piano Keys] | [Bar Ruler & Loop Markers Strip]           |
| (Width: 24px)          | (Width: 48px - 120px)       | (Height: 24px - 32px)                      |
| Track Color Accent     | - Octave labels (C1..C4)    | - Bar / Beat Numbers (1.1, 1.2, 1.3...)    |
| Track / Scene Title    | - Keys / Drums Mode Toggle  | - Orange/Color Loop Flags & Fill Bar       |
| Pinned at left edge    | - Pointer Tool Dropdown     |                                            |
|                        +-----------------------------+--------------------------------------------+
|                        | Piano Keyboard Sidebar      | Note Canvas & Pitch Grid                   |
|                        | - White keys (matte ivory)  | - Alternating white/black row backgrounds  |
|                        | - Black keys (matte ebony)  | - Vertical snap grid lines                 |
|                        | - Audition on click         | - Rounded note cards with pitch text       |
|                        | - Visual highlight on hover | - Real-time vertical playhead line         |
|                        +-----------------------------+--------------------------------------------+
|                        | Velocity / Mod Lane Header  | Velocity / Modulation Lane                 |
|                        | [🔊 Audition] [🎚 Velocity]  | - Vertical stems with circular caps        |
|                        | [Ψ Pitch] [↔ Pan]           | - 0..127 range aligned to note start ticks |
+------------------------+-----------------------------+--------------------------------------------+
| Status Footer Strip    | Horizontal Scrollbar        | Grid Snap Dropdown [1/16 ▴] [▶|◀] [➔| Auto]|
+---------------------------------------------------------------------------------------------------+
```

---

## 2. Features Discovered

| # | Category | Feature | Description | Inputs | Outputs | Error Behavior | Discovered Via |
|---|----------|---------|-------------|--------|---------|----------------|----------------|
| 1 | Note Lifecycle | Double-Click Note Creation | Double-clicking empty grid cell creates note at snapped tick and clicked pitch | Left Double-Click on empty grid cell | New `MidiNote` inserted; audition audio triggered if audition enabled | If overlapping existing note exactly, selects existing note | Video 0:71, Frame 0071, Footer cue |
| 2 | Note Lifecycle | Single-Click Select | Clicking a note clears prior selection and selects target note | Left Click on note body | `selected_notes = {idx}`, note highlighted with white stroke | If clicking outside note, selection cleared | Video 0:76, Frame 0076 |
| 3 | Note Lifecycle | Multi-Select Toggle | Toggles individual notes into/out of selection without clearing other selected notes | Ctrl + Left Click (or Shift + Click) on note | `selected_notes.insert(idx)` or `remove(idx)` | None | Video 0:76, Footer `CTRL+CLICK Remove from selection` |
| 4 | Note Lifecycle | Marquee Box Selection | Dragging across empty grid area draws a rubberband marquee rectangle | Left Click + Drag on empty grid background | `note_marquee_active = true`, highlights intersecting notes | Marquee outside canvas bounds is clamped to viewport | Video 0:96, Frame 0096, Footer `DRAG Rectangular selection` |
| 5 | Note Lifecycle | Select All | Selects all notes belonging to the active MIDI clip | Keyboard `Ctrl + A` | All note indices inserted into `selected_notes` | No-op if clip has 0 notes | Standard Bitwig DAW shortcut |
| 6 | Note Lifecycle | Double-Click Deletion | Double-clicking directly on a note deletes it immediately | Left Double-Click on note body | Target note removed from clip; selection updated | If double-clicked in empty space, creates note instead | Video 0:76, Footer `DOUBLE-CLICK Delete note` |
| 7 | Note Lifecycle | Key Deletion | Pressing Delete or Backspace removes all currently selected notes | `Delete` or `Backspace` key | All `selected_notes` removed from clip; selection cleared | No-op if `selected_notes.is_empty()` | Keyboard event handler |
| 8 | Note Manipulation | Pitch & Time Drag | Dragging note body moves all selected notes in pitch (semitones) and time (ticks) | Left Click + Drag on note body | Updates `start_tick` snapped to grid and `pitch` clamped 0..127 | Clamp to tick 0 and pitch range 0..127; notes cannot cross boundaries | Video 0:76, 1:05, Footer `DRAG Drag note` |
| 9 | Note Manipulation | Right Edge Resize | Hovering right edge (~4px) shows `[|]` cursor; dragging adjusts note duration | Left Click + Drag on note right edge | Modifies `duration_ticks` snapped to grid (or un-snapped if Shift held) | Clamped to minimum duration = 1 tick (or 1 grid step) | Video 0:72, 0:83, Footer `DRAG Adjust end    SHIFT Disable snapping` |
| 10 | Note Manipulation | Left Edge Resize | Hovering left edge (~4px) shows `[|` cursor; dragging adjusts note start and duration | Left Click + Drag on note left edge | Modifies `start_tick` and `duration_ticks` maintaining fixed note end tick | Start tick cannot be dragged past end tick (minimum 1 tick) | Video 0:72, Standard Bitwig left trim |
| 11 | Note Manipulation | In-Situ Velocity Drag | Dragging note with Ctrl+Shift directly modifies velocity without opening velocity lane | `Ctrl + Shift + Left Drag` vertically on note body | Adjusts `note.velocity` up/down in real time (1..127) | Clamped strictly to `1..=127` | Video 0:76, Footer `CTRL+SHIFT+DRAG Adjust velocity` |
| 12 | Note Manipulation | Free Drag Snapping Override | Holding Shift while dragging notes or resizing edges disables magnetic grid snap | Hold `Shift` during any note/loop drag | Offsets apply continuously with 1-tick resolution | None | Video 0:72, 0:80, Footer `SHIFT Disable snapping` |
| 13 | Note Manipulation | Keyboard Transpose & Shift | Arrow keys transpose selected notes in pitch or move along timeline | `Up`/`Down` (1 semitone), `Shift+Up`/`Down` (1 octave), `Left`/`Right` (grid step) | Selected notes updated with delta | Clamped at pitch 0..127 and tick >= 0 | Standard Bitwig interaction |
| 14 | Velocity Lane | Velocity Lane Toggle | Toggles display of the velocity editing sub-lane beneath the note grid | Click `🎚` button in bottom-left toolbar | Expands/collapses velocity lane (`height = 70px`) | Canvas height dynamically adjusts | Video 0:06, Bottom-left toolbar |
| 15 | Velocity Lane | Stem & Handle Display | Displays vertical stem from zero line with round/capsule handle at note velocity height | Velocity lane visible | Draws vertical stem + circular handle in track color for each note in view | Unselected notes drawn with muted gray stem/handle | Video 0:06, Frame 0006 |
| 16 | Velocity Lane | Handle Interactive Drag | Dragging a velocity handle vertically adjusts the note's velocity | Left Drag on handle or stem | `note.velocity` set to mapped Y value (0..127) | Clamped to `1..=127` | Bitwig Velocity Lane Spec |
| 17 | Velocity Lane | Multi-Note Scale / Ramp | Dragging across multiple handles or holding Alt draws a linear velocity slope | Left Drag across velocity lane with selection | Applies linear interpolation from start to end handle | Individual velocities clamped 1..127 | Bitwig Velocity Lane Spec |
| 18 | Grid & Snapping | Snap Grid Selector | Dropdown in bottom-right selects magnetic grid resolution | Click `1/16 ▴` in bottom toolbar | Grid subdivision changes: Off, 1/1, 1/2, 1/4, 1/8, 1/16, 1/32, 1/64, Triplet modes | Invalid values rejected; fallback to 1/16 | Video 0:37, Frame 0037, `1/16 ▴` dropdown |
| 19 | Grid & Snapping | Adaptive Grid Mode | Grid dynamically subdivides or consolidates lines based on horizontal zoom level | Zoom in / Zoom out | Grid lines appear/disappear smoothly without cluttering view | Subdivisions never closer than 8px | Video 0:06, 0:71 |
| 20 | Grid & Snapping | Triplet Mode Toggle | Switches grid subdivision between standard binary divisions and triplet subdivisions | Select `1/4T`, `1/8T`, or `1/16T` | Grid ticks calculated as `(TICKS_PER_BEAT * 4) / (division * 1.5)` | None | Bitwig Grid Engine |
| 21 | Timeline Navigation | Middle-Click Pan & Zoom | Middle-click dragging pans horizontally/vertically or zooms when dragging with modifier | `Middle Click + Drag` | Shifts `scroll_offset.x` and `scroll_offset.y` | Clamped to document extents | Video 0:15, 0:50, Footer `MIDDLE-DRAG Zoom & pan` |
| 22 | Timeline Navigation | Ctrl+Alt Pan | Holding Ctrl+Alt and left-dragging pans the timeline | `Ctrl + Alt + Left Drag` | Shifts `scroll_offset` | Clamped to bounds | Video 0:96, Footer `CTRL+ALT+DRAG Pan` |
| 23 | Timeline Navigation | Mouse Wheel Zoom & Pan | Wheel scrolls vertically; Shift+Wheel scrolls horizontally; Ctrl+Wheel zooms horizontally | Mouse Wheel, `Shift + Wheel`, `Ctrl + Wheel` | Adjusts `scroll_offset` or `zoom_x` centered at cursor position | `zoom_x` clamped between `MIN_ZOOM_X` (0.008) and `MAX_ZOOM_X` (0.5) | Frame 0006, 0157 |
| 24 | Timeline Navigation | Fit to Viewport Button | Zooms and centers timeline horizontally so all clip notes or loop bounds fit in window | Click `▶|◀` button in bottom-right toolbar | Computes `fit_zoom_x()` and resets `scroll_offset.x = 0` | If clip empty, fits default 4 bars | Video 0:37, Frame 0037 |
| 25 | Timeline Navigation | Follow Playhead (Auto-scroll) | Keeps timeline viewport centered on active playback position | Click Auto-scroll toggle icon in bottom-right | During playback, adjusts `scroll_offset.x` when playhead leaves viewport | Does not disrupt user manual drag | Video 0:37, Frame 0037 (orange icon) |
| 26 | Piano Keyboard | Key Visual Representation | 3D-shaded keyboard on left showing white keys (ivory) and black keys (matte charcoal) | Render cycle | Keys rendered with correct black key placement: C#, D#, F#, G#, A# | Octave boundary lines at C | Video 0:37, Frame 0037 |
| 27 | Piano Keyboard | Octave & Pitch Labels | Displays `C1`, `C2`, `C3`, `C4` at octave boundaries; displays all pitch names when zoomed in | Key height >= 14px | Text rendered inside octave gutter or key face | Label hidden if key height < 10px | Video 0:37 (C1..C4), Video 0:71 (all keys) |
| 28 | Piano Keyboard | Key Hover & Play Highlight | Piano key illuminates brightly when hovered, clicked, or when a note triggers on that pitch | Mouse hover, note playback, or note selection | Key background illuminates in bright white/cyan (`#FFFFFF` / `#B8D8F8`) | Highlight clears on mouse exit / note-off | Video 0:38, 0:71, 0:89, 1:05 |
| 29 | Piano Keyboard | Audition Key Click | Clicking any white or black piano key plays the note through the active track instrument | Left Click on piano key | Sends `PreviewSample` or MIDI NoteOn to track instrument node | Only triggers if audition speaker `🔊` is active | Video 0:38 |
| 30 | Clip Boundaries | Active Clip Loop Region | Ruler displays loop region bar with start flag `▶`, end flag `◀`, and colored fill | Clip loop enabled | Highlights bars within loop range; dims canvas outside loop | Loop start must be strictly < loop end | Video 0:06, 0:80, Frame 0080 |
| 31 | Clip Boundaries | Loop Marker Dragging | Dragging start or end triangle flag resizes clip loop duration | Left Click + Drag on loop marker flag | Updates `loop_start_tick` or `loop_end_tick` | Prevent dragging end before start (minimum 1 beat) | Video 0:80, Footer `DRAG Adjust loop end    SHIFT Disable snapping` |
| 32 | Clip Boundaries | Loop Body Sliding | Dragging the bar between loop markers moves the loop region along the timeline | Left Click + Drag on top loop bar body | Shifts both `loop_start_tick` and `loop_end_tick` by same delta | Loop start clamped to >= 0 | Video 0:80 |
| 33 | Clip Integration | Clip Launcher Mini-Notes Sync | Note changes in Piano Roll reflect immediately on the clip card in Clip Launcher | Note added/moved/resized in Piano Roll | Updates mini note vectors in `matrix.rs` Clip Slot preview | Real-time update via `AudioProxy` / Clip cache | Video 0:80, 0:89, 0:96 |
| 34 | Clip Integration | Detail Editor Dock Splitter | Splitter between main view and Detail Editor dock can be dragged to resize height | Left Click + Drag on horizontal divider | Resizes bottom dock height (`min_height = 160px`, `max_height = 500px`) | Clamped to prevent obscuring upper view | Video 1:45, Frame 0145, Footer `DRAG Move splitter` |
| 35 | Modes | Keys vs Drum Pads Mode | Toggles between melodic chromatic piano roll and named drum pad grid (OpenDMS) | Click Piano icon vs Drum Grid icon above keys | Switches left sidebar between piano keyboard and named pad buttons | Preserves note tick positions | Video 0:06, `piano_roll.rs:18-21` |

---

## 3. Edge Cases & Boundary Constraints

| # | Feature | Input | Observed & Expected Behavior |
|---|---------|-------|------------------------------|
| 1 | Note Resizing | Drag right edge to the left past note start tick | Clamped to minimum length of 1 tick (or 1 grid step). Duration cannot become zero or negative. |
| 2 | Note Resizing | Drag left edge to the right past note end tick | Clamped so `start_tick <= end_tick - 1`. Start tick cannot exceed note end tick. |
| 3 | Note Movement | Drag notes below lowest MIDI pitch (Pitch 0 / C-1) | Clamped to `pitch = 0`. Delta across multi-selected notes stops when lowest note hits 0. |
| 4 | Note Movement | Drag notes above highest MIDI pitch (Pitch 127 / G9) | Clamped to `pitch = 127`. Delta across multi-selected notes stops when highest note hits 127. |
| 5 | Note Movement | Drag notes to negative timeline position (Tick < 0) | Clamped to `start_tick = 0`. Relative offsets across selected notes preserve alignment. |
| 6 | Note Creation | Double-click directly on top of an existing note | Does NOT create a duplicate overlapping note. Instead, selects the existing note or deletes it if double-click on note. |
| 7 | Note Creation | Double-click near clip end boundary | Note created with duration clamped to loop end or standard 1/16th duration. |
| 8 | Loop Markers | Drag loop start marker past loop end marker | Clamped to `loop_end_tick - min_loop_ticks` (minimum loop length = 1 beat / 960 ticks). |
| 9 | Loop Markers | Drag loop end marker before loop start marker | Clamped to `loop_start_tick + min_loop_ticks`. |
| 10 | Marquee Selection | Rubberband drag starting inside a note | If drag starts on note body, initiates Note Drag instead of Marquee. Marquee only starts on empty space. |
| 11 | Note Deletion | Delete pressed when no notes selected | No-op. Does not alter clip data or crash. |
| 12 | Empty Clip State | Opening empty MIDI clip in Detail Editor | Renders clean empty grid with loop region; displays footer cue: `DOUBLE-CLICK Draw single note`. |
| 13 | Playback Loop Wrap | Playhead reaches `loop_end_tick` during playback | Playhead wraps smoothly back to `loop_start_tick` with continuous note scheduling and no audio dropout. |
| 14 | Rapid Zoom | Extreme zoom in (`MAX_ZOOM_X = 0.5`) or zoom out (`MIN_ZOOM_X = 0.008`) | Grid viewport calculation clamps without NaN or overflow; visible row and tick culling active. |
| 15 | Track Color Switch | Active track color changes in mixer or project | All note cards, velocity stems, loop markers, and left header strip update to new color immediately. |

---

## 4. Visual Design Tokens & Palette (1:1 Bitwig 5.1.7)

Exact visual tokens extracted from video frames and reference artifacts:

### 4.1 Background & Surface Colors
- **Main Editor Canvas Background**: `#1F2023` (Dark Charcoal Slate)
- **Grid Lane — White Key Row**: `#8F8F8F` with 12% alpha over canvas (`#26272B`)
- **Grid Lane — Black Key Row**: `#838383` with 22% alpha over canvas (`#1C1D20`)
- **Grid Major Lines (Bars / Compases)**: `#35363D` (1.0px width)
- **Grid Minor Lines (Beats / 16ths)**: `#2A2B30` (0.5px width)
- **Ruler Background**: `#323232` (Upper bar ruler surface)
- **Ruler Text**: `#F0F0F5` (Clean monospace/sans tabular numbers: `1.1`, `1.2`, `1.3`...)
- **Horizontal Splitter Line**: `#242424` (Hover: `#FF7A00` or `#45464D`, height: 3px)

### 4.2 Piano Keys Palette & Dimensions
- **Piano Keys Sidebar Width**: `84px` total (Octave gutter: `36px`, Key length: `48px`)
- **White Key Face**: `#B0B4BE` to `#D8DCE6` subtle vertical gradient (Ivory/Silver)
- **White Key Divider Border**: `#6D6D6D` (0.5px stroke)
- **Black Key Face**: `#1A1A1C` with top highlight `#2D2E32` (Matte Charcoal Ebony)
- **Black Key Length**: `30px` (approx 62% of white key length)
- **Active / Hovered Key Highlight**: `#FFFFFF` with soft `#B8D8F8` glow
- **Octave Text**: `#8A8A96` (Regular font, 10px, e.g., `C1`, `C2`, `C3`, `C4`)

### 4.3 Note Card Visuals
- **Note Fill**: Track accent color (e.g., `#FF7A00` for Track 1, `#505473` for Vital track, `#E8C52A` for Audio/Aux)
- **Note Fill (Selected)**: Bright saturated accent with interior gradient highlight
- **Note Stroke**: `1.0px` stroke inside:
  - Normal: `rgba(255, 255, 255, 0.35)`
  - Selected: `rgba(255, 255, 255, 0.90)`
- **Note Corner Radius**: `2.5px`
- **Note Pitch Label**: Text inside note card (`E2`, `F#2`, etc.) in `#FFFFFF` with subtle drop shadow, aligned left with 4px padding. Hidden if note width < 22px.

### 4.4 Velocity Lane Tokens
- **Velocity Lane Height**: `72px`
- **Lane Background**: `#18191C` (Darker recessed surface)
- **Lane Top Border**: `#2A2B30` (1.0px line)
- **Velocity Stem**: Vertical line of 1.5px width in track color (`rgba(track_color, 0.7)`)
- **Velocity Handle (Capsule/Cap)**:
  - Radius: `3.5px` circle or capsule cap
  - Normal: Track color with `#FFFFFF` 1px border
  - Selected: Bright white `#FFFFFF` with track color center
  - Unselected note: Dimmed stem `#4A4B52`, handle `#6A6B72`

### 4.5 Loop Region Tokens
- **Loop Bar in Ruler**: `4px` height directly below bar numbers
- **Loop Fill Bar**: Semi-transparent track color (`rgba(track_color, 0.25)`)
- **Loop Edge Flags**: Triangular markers at start (`▶`) and end (`◀`) in solid track color (`#FF7A00` or `#505473`)
- **Out-of-Bounds Shading**: Canvas outside `loop_start_tick..loop_end_tick` shaded with `#000000` at 30% opacity.

---

## 5. Architectural & Data Contract

To achieve full 1-to-1 parity across the engine, sequencer, and GUI, the following data contracts must be unified:

### 5.1 Canonical Note Instance
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MidiNoteInstance {
    pub id: u64,
    pub start_tick: u64,
    pub duration_ticks: u32,
    pub pitch: u8,       // 0..=127
    pub velocity: u8,    // 1..=127 (default: 100)
    pub release_velocity: u8,
}
```

### 5.2 Canonical MIDI Clip Model
```rust
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MidiClipModel {
    pub id: u64,
    pub name: String,
    pub color_rgb: [u8; 3],
    pub loop_start_tick: u64,
    pub loop_end_tick: u64,
    pub loop_enabled: bool,
    pub notes: Vec<MidiNoteInstance>,
}
```

### 5.3 Interaction State Machine (`PianoRollInteractionState`)
- `Idle`: Standard hovering; updates cursor based on hit tests (canvas, note body, note left/right edge, loop flag).
- `NoteDragging`: Pointer captured; moves selected notes by `(delta_ticks, delta_pitch)`. Snaps to grid unless `Shift` held.
- `NoteResizingLeft`: Adjusts `start_tick` and `duration_ticks` maintaining constant note end.
- `NoteResizingRight`: Adjusts `duration_ticks` snapped to grid.
- `MarqueeSelecting`: Draws rubberband box; updates `selected_notes` to all notes intersecting `Rect`.
- `VelocityDragging`: Adjusts velocity of target handle or scales multi-note selection.
- `LoopDragging`: Adjusts loop start, loop end, or slides whole loop region.
- `TimelinePanning`: Pan offset modified via Middle-drag or Ctrl+Alt drag.

### 5.4 GPU SDF Acceleration Integration (`bitwig_wgpu.rs`)
In addition to CPU egui painting, notes and velocity handles can be emitted as instanced GPU primitives into `BitwigGpuResources`:
- Primitive kind `KIND_NOTE = 6`: Parameters `[rect.min.x, rect.min.y, rect.width, rect.height]`, color, border radius `2.5px`, and stroke thickness `1.0px`.
- Renders via WGSL signed-distance function with sub-pixel antialiasing on wgpu swapchain.
