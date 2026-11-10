# Clean-Room Specification: Bitwig Studio Clip Launcher Context Menus

## 1. Evidence & Provenance
- **Visual & Behavioral Reference**:
  - `Captura de pantalla_20261110_185858.png` (MIDI Clip Context Menu in Bitwig Studio 5.1.7)
  - `Captura de pantalla_20261110_185755.png` (Audio Clip Context Menu observed behavior)
  - `Captura de pantalla_20261110_185638.png` (Empty Clip Slot Context Menu observed behavior)
  - Clean-Room Reference: `.agents/skills/reverse-engineer-anything/references/propietary-java-and-cpp-converter-to-rust.md`
- **Methodology**: Black-box clean-room observation (`reverse-engineer-anything`). No proprietary code, binaries, or vendor assets used. Re-implemented using Hikaru tokens, Rust data models, and `egui` immediate-mode rendering.

---

## 2. Visual & Structural Architecture

The context menu is a floating modal panel triggered on secondary click (right-click) over any cell in the horizontal Clip Launcher matrix (`views/matrix.rs`).

### 2.1 Color Tokens & Metrics
- **Menu Container Background**: `#222428` (`Color32::from_rgb(34, 36, 40)`)
- **Menu Border**: 1px `#2B2D33` (`Color32::from_rgb(43, 45, 51)`), Corner Radius: `3.0px`
- **Menu Drop Shadow**: Deep dark tint (`Color32::from_black_alpha(160)`)
- **Separator Line**: 1px `#32353B` (`Color32::from_rgb(50, 53, 59)`)
- **Row Heights**: Item rows `24.0px`, Header palette block `48.0px`, Action icon toolbar `36.0px`
- **Text Styling**:
  - Labels: Barlow / Arimo Medium, `11.0px`, `#E1E4EA`
  - Disabled Items: `#686C75`
  - Shortcuts: Barlow Regular, `10.5px`, `#767B86` right-aligned
- **Hover Highlighting**:
  - Row Hover: `#353840` (`Color32::from_rgb(53, 56, 64)`) with `2.0px` corner radius

---

## 3. The Three Distinct Menu Typologies

### 3.1 Empty Slot Menu ("sin un audio")
Triggered on a matrix slot where `slot.clip.is_none()`.
1. **Header Items**:
   - `Insert from Library...` [B] (Icon: Folder)
   - `Paste` [Ctrl+V] (Icon: Clipboard, disabled if clipboard is empty)
2. **Separator**
3. **Creation & Slot Controls**:
   - `Create Clip` (Creates 1-bar empty clip; MIDI if instrument track, audio record placeholder if audio track)
   - `Remove Stop Button` / `Add Stop Button` (Toggles `slot.has_stop_button`)
4. **Separator**
5. **Scene Operations**:
   - `Insert Scene`
   - `Delete Scene`

### 3.2 Audio Clip Menu ("con Audio")
Triggered on a matrix slot where `slot.clip.as_ref()` contains `ClipData::Audio`.
1. **Top Section**:
   - **24-Color Palette Swatches** (3 rows × 8 columns):
     - Clicking a swatch assigns `clip.color = Some(color)` override.
   - **Library Actions**:
     - `Insert from Library...` [B]
     - `Save Launcher clip To Library`
2. **Separator**
3. **Clip State**:
   - `Muted` (Checkbox / circle toggle) -> `clip.muted`
   - `Looping` (Checkbox / circle toggle with checkmark) -> `clip.loop_enabled`
4. **Separator**
5. **Action Toolbar Row** (5 horizontal icon buttons):
   - `[✂ CUT]`
   - `[📋 COPY]`
   - `[📋 PASTE]`
   - `[➕ DUPL.]`
   - `[🗑 DELETE]`
6. **Separator**
7. **Region & Arrangement**:
   - `Loop Selected Region` [Ctrl+L]
   - `Consolidate` [Ctrl+J]
8. **Separator**
9. **Time Scaling**:
   - `Double Content` [x2]
   - `Scale 50%`
   - `Scale 200%`
   - `Scale...`
10. **Separator**
11. **Audio Dynamics & Processing**:
    - `Normalize` (Active: scales audio peak to 0 dBFS / 1.0)
12. **Separator**
13. **Bounce & Slicing**:
    - `Bounce In Place` [Ctrl+B]
    - `Bounce`
    - `Expand...`
    - `Slice to Drum Machine...` [Ctrl+Alt+D] (Active)
    - `Slice to Multisample...` (Active)
    - `Fold to Takes...` (Disabled)
14. **Separator**
15. **Pitch Transposition**:
    - `Transpose an Octave Up` [Shift+Alt++] (Active: +12 semitones)
    - `Transpose an Octave Down` [Shift+Alt+-] (Active: -12 semitones)
16. **Separator**
17. **Quantization & Timing**:
    - `Quantize` [Q]
    - `Quantize...` [Alt+Q]
    - `Quantize Audio` (Active)
    - `Quantize Audio...` (Active)
    - `Make Legato` [Ctrl+Shift+Alt+L] (Disabled for audio)

### 3.3 MIDI Clip Menu ("con un Midi Clip")
Observed 1:1 in `Captura de pantalla_20261110_185858.png`:
1. **Top Section**:
   - **24-Color Palette Swatches** (3 rows × 8 columns) + Library actions
2. **Separator**
3. **Clip State**:
   - `Muted` -> toggles `clip.muted`
   - `Looping` -> toggles `clip.loop_enabled`
4. **Separator**
5. **Action Toolbar Row**:
   - `CUT`, `COPY`, `PASTE`, `DUPL.`, `DELETE`
6. **Separator**
7. **Region & Arrangement**:
   - `Loop Selected Region` [Ctrl+L]
   - `Consolidate` [Ctrl+J]
8. **Separator**
9. **Time Scaling**:
   - `Double Content` [x2] (Duplicates MIDI notes and doubles loop duration)
   - `Scale 50%` (Compresses note timings by 0.5)
   - `Scale 200%` (Expands note timings by 2.0)
   - `Scale...`
10. **Separator**
11. **Audio Dynamics**:
    - `Normalize` (Disabled for MIDI)
12. **Separator**
13. **Bounce & Slicing**:
    - `Bounce In Place` [Ctrl+B]
    - `Bounce`
    - `Expand...`
    - `Slice to Drum Machine...` [Ctrl+Alt+D]
    - `Slice to Multisample...`
    - `Fold to Takes...`
14. **Separator**
15. **Pitch Transposition**:
    - `Transpose an Octave Up` [Shift+Alt++] (Transposes note pitches +12)
    - `Transpose an Octave Down` [Shift+Alt+-] (Transposes note pitches -12)
16. **Separator**
17. **Quantization & Timing**:
    - `Quantize` [Q] (Quantizes note start ticks to 1/16th grid)
    - `Quantize...` [Alt+Q]
    - `Quantize Audio` (Disabled for MIDI)
    - `Quantize Audio...` (Disabled for MIDI)
    - `Make Legato` [Ctrl+Shift+Alt+L] (Active: extends note lengths to adjacent notes)
