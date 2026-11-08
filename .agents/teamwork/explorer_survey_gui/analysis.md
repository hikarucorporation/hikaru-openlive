# Analysis Report: GUI & Renderer Architecture for Bitwig-Style Piano Roll and MIDI Clips

**Author:** GUI & Renderer Explorer (`crates/hikaru_gui`)  
**Workspace:** `hikaru-openlive`  
**Target:** 1-to-1 Bitwig Studio 5.1.7 MIDI Clips & Piano Roll Integration  
**Date:** 2026-11-07  

---

## 1. Executive Summary

This investigation explores the GUI, rendering, and asset architecture of Hikaru OpenLive (`crates/hikaru_gui`, `crates/hikaru_render`, and connected workspace crates).

Hikaru OpenLive possesses a **dual GPU rendering architecture**:
1. **In-Tree Immediate-Mode SDF Pipeline (`crates/hikaru_gui/src/ui/bitwig_wgpu.rs`)**: Runs natively inside `eframe`'s swapchain `wgpu::RenderPass` via `egui_wgpu::Callback::new_paint_callback`. It evaluates signed distance functions (SDF) directly on the GPU for sub-pixel anti-aliased Bitwig controls (capsules, rotary knobs, faders, meters, and LCD bezels).
2. **Offscreen 3D WGPU Pipeline (`crates/hikaru_render` & `crates/hikaru_gui/src/render.rs`)**: An isolated offscreen `GpuContext` compiling WGSL shaders (`MESH_SHADER`, `KNOB_SHADER`, `QUAD_SHADER`) with frame signature caching for 3D procedural meshes (such as `OpenWavetable`'s 3D ribbon and rotary knobs), with CPU readback into `egui::ColorImage`.

Additionally, **Lemon Studio assets and conventions** form the foundational design kit of `crates/hikaru_gui`:
- Vector icon definitions derived from Tabler 24×24 (`ui/icons.rs`) teselated via quadratic/cubic Bézier curves.
- Studio-calibrated decibel meter ballistics (`ui/meters.rs`).
- Barlow Condensed/SemiCondensed typographic hierarchy (`ui/fonts.rs`).
- Tactical immediate-mode widget kit (`ui/bitwig_kit.rs`).

However, **the existing Piano Roll (`crates/hikaru_gui/src/views/piano_roll.rs`) is currently a rudimentary stub**:
- It renders plain 2D rectangles without GPU SDF acceleration or velocity shading.
- While `PianoRollState` already includes field declarations for note resizing, note moving, and marquee selection, **none of these interactions are currently wired up** in `piano_roll::render()`.
- The velocity lane is completely absent.
- MIDI Clips are partially represented in the Session Matrix (`matrix::ClipData::Midi`), but their mini-note preview inside slot cards is unimplemented (rendering only empty cards).
- In the Arranger Timeline (`matrix.rs` and `playlist.rs`), `ClipType` lacks a `Midi` variant, meaning timeline tracks cannot currently host or display MIDI clips with mini-notes.
- Double-clicking a MIDI clip does not currently auto-open and bind the Piano Roll.

Below is the complete, evidence-based survey with exact file paths, line numbers, code snippets, architectural findings, and concrete recommendations.

---

## 2. egui + wgpu + wgsl/naga Integration Points, Pipelines & Shaders

### 2.1 Dependency and Version Alignment
In the root `Cargo.toml`:
```toml
# Versión unificada con eframe 0.31 y egui-wgpu 0.31
wgpu = "24.0.5"
bytemuck = { version = "1.25", features = ["derive"] }
naga = { version = "24.0.0", features = ["wgsl-in"] }
egui = "0.31"
eframe = { version = "0.31", default-features = false, features = ["default_fonts", "wgpu", "wayland", "x11"] }
egui-wgpu = "0.31"
pollster = "0.4"
```
Both `hikaru_gui` and `hikaru_render` share these exact dependencies, preventing ABI version mismatch across the workspace.

### 2.2 On-Screen In-Tree SDF Pipeline (`ui/bitwig_wgpu.rs`)
Located in `crates/hikaru_gui/src/ui/bitwig_wgpu.rs`:
- **Shader**: `BITWIG_SDF_SHADER` (lines 78–525) is an embedded WGSL shader compiled with `wgpu::ShaderSource::Wgsl`.
- **Initialization**: `install_render_resources(render_state: Option<&egui_wgpu::RenderState>)` is invoked during `main.rs:492` inside `eframe::run_native` startup:
  ```rust
  hikaru_gui::ui::bitwig_wgpu::install_render_resources(cc.wgpu_render_state.as_ref());
  ```
- **GPU Resources**: `BitwigGpuResources` maintains a dynamic storage buffer (`wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST`) initialized to `INITIAL_INSTANCE_CAPACITY = 1024` instances of `SdfWidgetGpu` (80 bytes each, 16-byte aligned: 5 × `vec4`).
- **Callback Trait Integration**:
  - `BitwigSdfCallback` implements `egui_wgpu::CallbackTrait`:
    - `prepare()`: Pushes `SdfWidgetGpu` instances into `BitwigGpuResources::push_instance()`, copying to GPU via `queue.write_buffer()`. Reallocates buffer capacity if count exceeds capacity.
    - `paint()`: Sets viewport (`info.viewport_in_pixels()`), scissor rectangle (`info.clip_rect_in_pixels()`), pipeline, bind group, and executes instanced draw:
      ```rust
      render_pass.draw(0..6, idx..idx + 1);
      ```
- **Painter Integration**: `paint_sdf_widget(painter: &Painter, rect: Rect, widget: SdfWidgetGpu)` constructs `egui_wgpu::Callback::new_paint_callback(rect, BitwigSdfCallback { ... })` and appends `egui::Shape::Callback(cb)` to the egui display list.
- **Current SDF Primitives**:
  - `KIND_CAPSULE` (0): Beveled rectangular buttons with independent corner radii, active/hovered/sunken shading, triangular corner notch.
  - `KIND_ROTARY_KNOB` (1): 270° rotary dial with cast shadow, beveled cap, unipolar/bipolar indicator arc, and needle segment.
  - `KIND_FADER_VERTICAL` (2): Sunken fader well, copper-amber bar, 0 dB tick, bright orange needle.
  - `KIND_FADER_HORIZONTAL` (3): Horizontal fader / bipolar pan slider.
  - `KIND_STEREO_METER` (4): Dual-channel stereo level meter with green/lime/amber/red ballistics, 2px micro-segment LEDs, and peak-hold line.
  - `KIND_LCD_BEZEL` (5): Sunken LCD glass display bezel.

### 2.3 Offscreen 3D WGPU Pipeline (`hikaru_render` & `crates/hikaru_gui/src/render.rs`)
- `crates/hikaru_render`:
  - `GpuContext`: Async creation of offscreen `wgpu::Device` and `wgpu::Queue` (`context.rs:60`).
  - `MeshRenderer`: Compiles `MESH_SHADER` (lines 38–174) for rendering 3D indexed vertex meshes with orbital/isometric camera view projections (`camera.rs`).
  - `KnobRenderer`: Compiles `KNOB_SHADER` (lines 35–80) for rotating 3D beveled cylinders with dynamic lighting.
  - `QuadRenderer`: Compiles `QUAD_SHADER` for 2D textured sprites.
- `crates/hikaru_gui/src/render.rs`:
  - `WavetableViewportHandle`: Owns `HikaruRenderer` wrapped in `Arc<Mutex<WavetableViewport>>`.
  - Caches offscreen render output by frame signature (camera + waveform + parameters).
  - Uses `GpuContext::read_color_rgba8` to convert GPU offscreen textures to `egui::ColorImage`.
  - In `open_wavetable.rs:938-949`, the image is loaded into egui's texture manager via `ctx().load_texture()` and painted onto the canvas.

### 2.4 Naga Validation
Both `BITWIG_SDF_SHADER` and `MESH_SHADER` are strictly validated in CI without requiring a physical GPU via `naga::front::wgsl::parse_str()` and `naga::valid::Validator` (tested in `crates/hikaru_gui/tests/gui_tests.rs:600-606` and `crates/hikaru_render/src/mesh.rs:1612-1620`).

---

## 3. Lemon Studio 3D Assets, Shaders, Textures, Meshes, and Immediate-Mode Conventions

### 3.1 Provenance
Lemon Studio (`os-ui`, `os-app`) is the direct progenitor of Hikaru's visual and procedural styling. The heritage is explicitly acknowledged across multiple files:
- `crates/hikaru_gui/src/ui/bitwig_wgpu.rs:231`: `"Perilla rotativa 270° estilo Bitwig / Lemon-Studio con arco luminoso y aguja SDF"`
- `crates/hikaru_gui/src/ui/icons.rs:10-18`: Vector outlines derived from Lemon-Studio (`crates/os-ui/src/tabler.rs`).
- `crates/hikaru_gui/src/ui/meters.rs:10-25`: Calibrated from Lemon-Studio (`crates/os-app/src/meters.rs`).
- `crates/hikaru_gui/src/ui/fonts.rs:14-29`: Barlow Condensed font kit adapted from Lemon-Studio `os-ui`.
- `crates/hikaru_gui/src/ui/bitwig_kit.rs:13`: `"Inspirado en la arquitectura kit de Lemon-Studio..."`

### 3.2 Vector Iconography Kit (`ui/icons.rs`)
- Completely eliminates third-party icon fonts or Unicode emoji dependencies.
- Tabler 24×24 outlines are defined as `VectorIcon { filled: bool, d: &'static str }` and parsed via `parse_svg_subpaths(d)` to tessellate cubic and quadratic Bézier segments (`M, L, Q, C, Z`).
- Geometric custom Bitwig icons are implemented with pixel-snapped painter primitives:
  - `paint_note_editor(painter, rect, color)` (`icons.rs:428`): 3 staggered MIDI note cards, used as the Note Editor footer button.
  - `paint_piano_keyboard` (`icons.rs:95`): Piano key icon.
  - `paint_arranger_lanes` (`icons.rs:410`): Two horizontal arranger bars.
  - `paint_clip_launcher_grid` (`icons.rs:394`): 3×2 grid icon.
  - `paint_automation_nodes`, `paint_device_box`, `paint_mixer_faders`.

### 3.3 Meter Ballistics (`ui/meters.rs`)
- Implements `StereoBallistics` modeling real hardware VU meter ballistics: attack constant `0.02s`, decay `0.30s`, peak-hold time `1.2s`.
- Calibrated logarithmic-to-linear mapping `db_to_frac(db)` aligning with visual ticks `0, -6, -12, -18, -24, -36, -∞ dB`.

### 3.4 Immediate-Mode Kit Conventions (`ui/bitwig_kit.rs`)
All widgets follow standard immediate-mode patterns:
1. Allocate exact bounds: `let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());`
2. Visibility cull: `if ui.is_rect_visible(rect) { ... }`
3. Hover / interaction cues: Cursor icon updates (`CursorIcon::PointingHand`, `CursorIcon::ResizeVertical`, `CursorIcon::ResizeHorizontal`).
4. Shift modifier fine-tuning:
   ```rust
   let fine = ui.input(|i| i.modifiers.shift);
   let sens = if fine { 0.0012 } else { 0.0055 };
   ```
5. Double-click to reset to default value.

---

## 4. Arranger & Matrix Views, Clip Rendering, and Piano Roll Mounting Points

### 4.1 Arranger and Clip Launcher Layout (`matrix.rs`)
The primary arrangement layout in `crates/hikaru_gui/src/views/matrix.rs` implements Bitwig's signature dual view (controlled by `panel_layout.show_clip_launcher` and `panel_layout.show_arranger`):
- **Track Headers Column** (left, `width = 168.0`, lines 1350–1494):
  - Track color stripe (4px).
  - Track name + compact `Arm (●)`, `Solo (S)`, `Mute (M)` buttons.
  - Horizontal volume fader + level meter + Track stop (`■`) button.
- **Horizontal Clip Launcher** (center-left, lines 1740–1893):
  - Scenes header row: `▶ Scene 1` to `▶ Scene 8` + `+` button.
  - Clip matrix: row = track, column = scene.
  - Populated cells render a top header (`▶`/`■` state icon + clip name) and a body area.
- **Arranger Timeline** (center-right, lines 1904–2250):
  - Top Beat Ruler (`#212226`) with bar numbers, subdivision lines, loop region bracket, and playhead cursor.
  - Track Lanes: horizontal lanes aligned with Track Headers.
  - Populated clips are rendered as rounded cards with headers and waveforms.

### 4.2 Current State of MIDI Clip Data & Rendering
1. **In `matrix::ClipData` (`matrix.rs:166-176`)**:
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
   ```
   - In `matrix.rs:1789-1810`, only `clip.peaks` (audio waveform) is rendered in slot cards. For `ClipData::Midi`, the slot body is completely blank!
2. **In `playlist::ClipType` (`playlist.rs:47-59`)**:
   ```rust
   pub enum ClipType {
       Pattern { pattern_id: usize },
       Audio { sample_path, peaks, sample_offset_ticks, total_sample_ticks },
       Automation { points, target_param },
   }
   ```
   - **Crucial gap**: `ClipType` does NOT have a `Midi` variant.
   - On the Arranger Timeline (`matrix.rs:2117`), only `ClipType::Audio` is rendered. Any non-audio clip on the timeline renders as an empty card without notes.

### 4.3 Current Piano Roll Editor (`views/piano_roll.rs`)
`piano_roll.rs` contains:
- `PianoRollState`: Comprehensive state struct with fields for `zoom_x`, `key_height`, `scroll_offset`, `notes: Vec<MidiNote>`, `selected_notes: HashSet<usize>`, `note_resize_*`, `note_move_*`, and `note_marquee_*`.
- `piano_roll::render(ui, app)`:
  - Top ruler with bar numbers (`RULER_HEIGHT = 24.0`).
  - Piano keys sidebar (`SIDEBAR_WIDTH = 120.0`) with white and black key labels (`PITCH_CLASS_NAMES`).
  - Grid background with black key row shading and bar division lines.
  - Simple note rectangle rendering with `grid_painter.rect_filled()`.
  - **Missing**: No velocity lane, no note duration resizing interaction, no pitch/time note dragging, no marquee box, no note insertion on double-click, no deletion, and no integration with `bitwig_wgpu.rs` SDF shaders.

### 4.4 Mounting Points for the Bitwig-Style Piano Roll
The Piano Roll mounts into the global UI topology at four distinct integration points:
1. **Bottom Dock Panel (`crates/hikaru_gui/src/app.rs:1106-1137`)**:
   - Currently, `bottom_dock` checks:
     ```rust
     if self.state.clip_editor.show_editor {
         detail_editor_panel::render(ui, &mut self.state);
     } else if self.state.show_piano_roll {
         piano_roll::render(ui, &mut self.state);
     } else if self.state.show_dsp_rack {
         dsp_rack::render(ui, &mut self.state);
     } else if self.state.panel_layout.show_mixer {
         mixer::render_bottom_panel(ui, &mut self.state);
     }
     ```
   - In Bitwig, the **Detail Editor Panel** is a unified bottom dock that dynamically hosts:
     - The Audio Event Editor when an Audio Clip is active.
     - The Note Editor (Piano Roll) when a MIDI Clip is active.
   - The height should be resizable via a draggable top splitter border (`editor_height`).
2. **Footer Bar Toggle (`crates/hikaru_gui/src/views/footer.rs:147-164`)**:
   - `flat_icon_toggle` with `paint_note_editor` icon toggles `state.show_piano_roll`.
   - Activating it should close conflicting bottom panels (`show_dsp_rack = false`, `show_mixer = false`).
3. **Session Matrix Slot Double-Click (`crates/hikaru_gui/src/views/matrix.rs:1875`)**:
   - When the user double-clicks a slot containing `ClipData::Midi`:
     - Set `state.show_piano_roll = true; state.clip_editor.show_editor = false; state.show_dsp_rack = false;`
     - Bind `piano_roll_state.notes` to the slot's notes.
     - Record `piano_roll_state.notes_source_slot = Some((track_idx, scene_idx))`.
4. **Arranger Timeline Clip Double-Click (`crates/hikaru_gui/src/views/matrix.rs:2150`)**:
   - When the user double-clicks a MIDI clip on the timeline:
     - Set `state.show_piano_roll = true`.
     - Bind `piano_roll_state.notes` to the playlist clip's notes.

---

## 5. Input Handling Architecture in egui

### 5.1 Event Polling and Response Pattern
In egui 0.31, input querying uses two layers:
1. **Widget-local `Response`**:
   - Click detection: `response.clicked()`, `response.double_clicked()`, `response.secondary_clicked()`.
   - Drag detection: `response.drag_started()`, `response.drag_started_by(PointerButton::Primary)`, `response.dragged()`, `response.drag_delta()`, `response.drag_stopped()`.
   - Pointer position: `response.interact_pointer_pos()`.
2. **Global Context Input (`ui.input(|i| ...)` or `ctx.input(|i| ...)` )**:
   - Modifier keys:
     - `i.modifiers.shift`: Fine adjustment, additive selection, constraint modifier.
     - `i.modifiers.ctrl` (or `i.modifiers.command`): Multi-selection, duplication, zoom modifier.
     - `i.modifiers.alt`: Note content slipping, alternative drag mode.
   - Mouse release: `i.pointer.any_released()`.
   - Scroll wheel: `i.smooth_scroll_delta` (`Vec2 { x, y }`).

### 5.2 Interaction Mechanics for the Piano Roll

| Interaction | Gesture / Event | Implementation Details |
|---|---|---|
| **Note Creation** | Double-click on empty grid cell | `let tick = quantize_tick(tick_at_x, snap_ticks); let pitch = pitch_at_y;` Append note with default duration and velocity 100. |
| **Note Selection** | Single click on note | If `Shift` held: toggle note in `selected_notes`. Otherwise: `selected_notes.clear(); selected_notes.insert(idx);` |
| **Marquee Selection** | Drag on empty grid background | `note_marquee_active = true;` Draw semi-transparent rectangle between `note_marquee_start` and current pos. Select all notes whose bounding box intersects `marquee_rect`. |
| **Note Move (Pitch/Time)** | Drag inside selected note body | Store original `(tick, pitch)` for all selected notes. Calculate `delta_ticks = quantize_tick(drag_delta_ticks, snap_ticks)` and `delta_pitch = (delta_y / key_height).round()`. Update preview; apply on release. |
| **Note Duplicate Move** | `Alt` + Drag selected notes | Duplicate selected notes on drag start and move copies. |
| **Duration Resize** | Drag note left/right edge (±4px) | Detect hover near edge: set `ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal)`. On drag: `duration_ticks = (orig_duration + delta_ticks).max(snap_ticks)`. |
| **Note Deletion** | `Delete` / `Backspace` or Right-Click | `notes.retain(...)` for all indices in `selected_notes`. |
| **Velocity Adjustment** | Drag velocity bar handle in bottom lane | Detect click/drag on velocity stem/circle: `velocity = ((pos.y - base_y) / lane_height * 127.0).clamp(1.0, 127.0) as u8`. |
| **Velocity Ramp** | Drag across multiple velocity stems | Interpolate velocity line across swept notes. |
| **Horizontal/Vertical Pan** | Normal scroll wheel or Middle-Drag | Vertical wheel scrolls pitch rows; `Shift` + wheel scrolls timeline ticks. Middle-Drag pans both axes. |
| **Horizontal/Vertical Zoom** | `Ctrl` + Wheel / `Ctrl` + `Shift` + Wheel | `Ctrl` + wheel adjusts `zoom_x`. `Ctrl` + `Shift` + wheel adjusts `key_height`. |

---

## 6. Gap Analysis & Implementation Roadmap

```
+---------------------------------------------------------------------------------------------------+
|                                      GLOBAL WORKSPACE (HikaruApp)                                 |
|                                                                                                   |
|  [Header / Top Bar]                                                                               |
|  +---------------------------------------------------------------------------------------------+  |
|  | [FILE] [PLAY] [▶] [■] [●]  [110.00 BPM 4/4]  [1.1.1.00]  [⟳] [ADD] [EDIT] [Hikaru Core]    |  |
|  +---------------------------------------------------------------------------------------------+  |
|                                                                                                   |
|  [Central Workspace: Arrange View (matrix.rs)]                                                    |
|  +--------------------+--------------------------------+---------------------------------------+  |
|  | Track Headers      | Horizontal Clip Launcher       | Arranger Timeline                     |  |
|  |                    |                                |                                       |  |
|  | [●][S][M] Inst 1   | [▶ MIDI Clip]  [ ■ ]           | [ Inst 1: ======= MIDI Notes ======= ]|  |
|  |   [ Fader GPU ]    |  └─ Mini Note Preview (SDF)    |  └─ Timeline Mini Note Rectangles     |  |
|  |                    |                                |                                       |  |
|  | [●][S][M] Audio 2  | [▶ Audio Pad]  [ ■ ]           | [ Audio 2: ~~~ Waveform ~~~          ]|  |
|  +--------------------+--------------------------------+---------------------------------------+  |
|                                                                                                   |
|  [Bottom Dock: Detail Editor / Piano Roll (app.rs: bottom_dock)]                                   |
|  +---------------------------------------------------------------------------------------------+  |
|  | Piano Roll Editor (piano_roll.rs + bitwig_wgpu.rs)                                          |  |
|  | +------------------+----------------------------------------------------------------------+  |
|  | | Piano Keys       | Time Grid & Notes (Bitwig SDF accelerated)                           |  |
|  | | [ C4 ]           |   [====== Note C4 ======]                                            |  |
|  | | [ B3 ]           |                                                                      |  |
|  | | [ A#3] (Black)   |                 [==== Note A#3 ====]                                 |  |
|  | | [ A3 ]           |                                                                      |  |
|  | +------------------+----------------------------------------------------------------------+  |
|  | | Velocity Lane    | !                    !                                               |  |
|  | | (0..127)         | o                    o                                               |  |
|  | +------------------+----------------------------------------------------------------------+  |
|  +---------------------------------------------------------------------------------------------+  |
|                                                                                                   |
|  [Footer Status Bar (footer.rs)]                                                                  |
|  +---------------------------------------------------------------------------------------------+  |
|  | [ARRANGE] [MIX] [EDIT] | [Note Editor Icon] [Automation] [Device Panel] [Mixer]             |  |
|  +---------------------------------------------------------------------------------------------+  |
+---------------------------------------------------------------------------------------------------+
```

### 6.1 Concrete Implementation Tasks

1. **GPU SDF Shader Extension (`crates/hikaru_gui/src/ui/bitwig_wgpu.rs`)**:
   - Add `KIND_NOTE = 6` to `BITWIG_SDF_SHADER` with beveled rounded rect, dark border, and selection glow, OR use `paint_capsule` with customized radius and flags.
   - Add helper `paint_note_card(painter, rect, color, selected, velocity_norm)` and `paint_velocity_stem(painter, x, bottom_y, height, color)`.
2. **Piano Roll Full Interactivity (`crates/hikaru_gui/src/views/piano_roll.rs`)**:
   - Implement note creation on double-click with snap quantization.
   - Implement note edge dragging for duration resize.
   - Implement note body dragging for pitch/time repositioning with snap.
   - Implement multi-note marquee rubberband selection.
   - Implement velocity lane below the note grid with interactive draggable stems.
   - Implement grid snap dropdown selector (Off, 1 Bar, 1/2, 1/4, 1/8, 1/16, 1/32).
   - Add sync back to `AudioProxy` (`GuiCommand::UpdateMidiClipNotes`).
3. **MIDI Clip Integration in Session Matrix (`crates/hikaru_gui/src/views/matrix.rs`)**:
   - Inside Clip Launcher slots: render miniature note bars for `ClipData::Midi { notes }` using scaled tick and pitch coordinates.
   - Double-click on MIDI slot: sets `show_piano_roll = true`, loads slot notes into `piano_roll_state`, sets `notes_source_slot = Some((track_idx, scene_idx))`.
4. **MIDI Clip Integration in Arranger Timeline (`crates/hikaru_gui/src/views/playlist.rs` & `matrix.rs`)**:
   - Add `ClipType::Midi { notes: Vec<MidiNoteInstance> }` to `playlist::ClipType`.
   - In `matrix.rs` Arranger timeline clip loop: render mini-note rectangles inside the timeline clip card when `ClipType::Midi`.
   - Double-click on timeline MIDI clip: sets `show_piano_roll = true`, loads notes into `piano_roll_state`.
5. **Layout & Splitter Polish (`crates/hikaru_gui/src/app.rs`)**:
   - Wire `editor_height` resizing on `bottom_dock` when `show_piano_roll` is active.
   - Verify keyboard shortcuts: Space (transport), Delete/Backspace (delete selected notes in Piano Roll), Ctrl+A (select all notes in Piano Roll).
