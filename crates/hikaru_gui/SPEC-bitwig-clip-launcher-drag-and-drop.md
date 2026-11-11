# Clean-Room Specification: Bitwig Studio Clip Launcher Drag & Drop (Pad Movement)

## 1. Evidence & Provenance
- **Visual & Behavioral References**:
  - `Videocaptura de pantalla_20261110_234847.webm` (Bitwig Studio 5.1.7 observed behavior: click + drag on clip pads in horizontal launcher moves clips across scenes and tracks for both Audio and MIDI clips).
  - `Videocaptura de pantalla_20261110_234348.webm` (Hikaru OpenLive observed bug: click only selects/triggers; drag does not initiate; clips cannot be moved between slots).
  - Clean-Room Reference: `.agents/skills/reverse-engineer-anything/references/propietary-java-and-cpp-converter-to-rust.md`
- **Methodology**: Black-box clean-room observation (`reverse-engineer-anything`). No proprietary code, binaries, or vendor assets used. Re-implemented using Hikaru tokens, Rust data models, and `egui` immediate-mode rendering.

---

## 2. Behavioral Specification

### 2.1 Drag Initiation & Discrimination
- A click without dragging (movement < 4px) preserves original selection and clip triggering behavior:
  - Clicking on the play icon (`ic_rect`) triggers clip playback.
  - Clicking on the card body selects the slot and targets the clip editor.
  - Double clicking opens the Piano Roll / Detail Editor.
  - Right clicking opens the Bitwig Clip Context Menu.
- When the mouse button (Primary) is pressed on a populated slot and dragged beyond the threshold:
  - Dragging initiates cleanly.
  - Playback is **not** triggered while dragging.
  - Source track and scene index are captured from `press_origin` / click position.
  - Modifier check: `Ctrl` or `Alt` keys toggle Copy mode (`is_copy = true`).

### 2.2 In-Flight Drag Visual Feedback (Impeccable Craft Floor)
- **Source Slot**:
  - Rendered with reduced opacity (dimmed ghost card) and subtle border indicating original placement.
- **Hovered Target Slot**:
  - Prominent 2.0px `ACCENT_ORANGE` (`Color32::from_rgb(0xFF, 0x8A, 0x00)`) border stroke.
  - Warm inner glow / semi-transparent orange wash (`Color32::from_rgba_unmultiplied(255, 138, 0, 35)`).
- **Floating Drag Ghost Card (Attached to Cursor)**:
  - Position: follows cursor offset by initial click grab point (`pointer_pos - drag.grab_offset`).
  - Size: matches clip card dimensions (`BITWIG_SCENE_W - 3.0` × `row_h - 3.5`).
  - Soft ambient drop shadow beneath ghost card (`Color32::from_black_alpha(100)`).
  - Header strip with clip color, play/stop icon, and clip name.
  - Body miniature: rendered audio waveform peaks or MIDI note blocks.
  - Distinct 1.5px border (`Color32::from_rgba_unmultiplied(255, 255, 255, 200)` or `ACCENT_ORANGE`).
  - Copy mode badge: `+` symbol badge on top-right corner when `Ctrl` or `Alt` is held.

### 2.3 Drop & Engine Synchronization
- **On Mouse Release (Drop)**:
  - If dropped on a valid grid cell `(dst_track, dst_scene)` different from `(src_track, src_scene)`:
    - If `is_copy`: duplicate clip into destination slot (`copy_to_slot`).
    - Else (default): move clip from source to destination (`move_slot`).
    - Update `selected_slot = Some((dst_track, dst_scene))`.
  - If dropped out of bounds or on the same slot:
    - Cancel drag gracefully without mutating slots.
- **Audio Engine Real-Time Safety**:
  - Lock-free command queue: GUI sends non-blocking messages via `AudioProxy::send(GuiCommand::...)`.
  - Source slot voices stopped and cleared (`SetClipEvents` empty or `UpdateMidiClipNotes` empty).
  - Destination slot synchronized with audio file (`LoadClip`), loop bounds, or MIDI notes.

---

## 3. Data Model

```rust
#[derive(Clone, Debug)]
pub struct MatrixClipDrag {
    pub from_track: usize,
    pub from_scene: usize,
    pub clip: MatrixClip,
    pub grab_offset: egui::Vec2,
    pub is_copy: bool,
    pub moved: bool,
}
```
Added to `SessionMatrixState`:
```rust
pub struct SessionMatrixState {
    ...
    pub clip_drag: Option<MatrixClipDrag>,
}
```

Methods implemented on `SessionMatrixState`:
- `move_slot(src_track, src_scene, dst_track, dst_scene, audio_proxy)`
- `copy_to_slot(src_track, src_scene, dst_track, dst_scene, audio_proxy)`
