# Clean-Room Specification: Bitwig Studio 5.1.7 1:1 Workspace Layout, Device Panel & Drag-and-Drop

## 1. Evidence & Provenance
- **Visual & Behavioral Reference**:
  - `.opencode/references/bitwig_5.1.7/screenshots/video/bitwig-layout-1.webm`
  - `.opencode/references/bitwig_5.1.7/screenshots/bitwig-clip-launcher.png`
  - `.opencode/references/bitwig_5.1.7/screenshots/bitwig-playlist.png`
  - `.opencode/references/bitwig_5.1.7/screenshots/bitwig-clip-launcher-and-playlist.png`
  - `.opencode/references/bitwig_5.1.7/screenshots/bitwig-device-panel-(in-the-clip-launcher).png`
  - `.opencode/references/bitwig_5.1.7/screenshots/bitwig-mix-panel.png`
- **Methodology**: Black-box clean-room observation (`reverse-engineer-anything`). No proprietary code or assets used.

---

## 2. Global Workspace Topology (1:1 Bitwig Studio 5.1.7)

### 2.1 Top Transport & Application Bar (`header.rs` & `menu_bar.rs`)
- **Single Unified Top Bar** (`#323438` surface over `#1E2024` frame):
  1. **Left**: `FILE` menu button, `PLAY` menu button, Transport `[▶]`, `[■]`, `[●]`, Automation Write toggle.
  2. **Transport LCD Display** (`#121212` fill, `#2B2E36` 1px border):
     - Left column: Tempo (`110.00` / `140.00` in `#EAECEF`) + Time Signature (`4/4` in `#9DA3AE`) with interactive drag/stepper.
     - Right column: Musical position (`1.1.1.00` in `#5CC8FF`) + Timecode (`0:00.000` in `#7FA8C4`).
  3. **Middle-Left**: Loop toggle (`⟳`), `ADD` menu, `EDIT` menu, Undo/Redo (`↩`, `↪`).
  4. **Center**: Hikaru Energy Core / Bitwig-style emblem button (opens Studio Control Panel).
  5. **Right**: Active Project tab (`New 1` / project filename).

### 2.2 Central Arrange View (`StatusViewTab::Arrange`)
Unlikevertical-column session grids, Bitwig's **Arrange** view always places **horizontal Tracks on the left** and allows toggling the **Clip Launcher** and **Arranger Timeline** independently or simultaneously side-by-side:
1. **Top-Left Track Toolbar (`width = 168px`, `height = 30px`)**:
   - **Clip Launcher Toggle** (`28×22 px`, 3×2 grid icon): Toggles `panel_layout.show_clip_launcher`.
   - **Arranger Timeline Toggle** (`28×22 px`, 2 horizontal track bars icon): Toggles `panel_layout.show_arranger`.
   - **Invariant**: At least one of `show_clip_launcher` or `show_arranger` remains `true` at all times. Both may be `true` simultaneously (split view).
   - **Stop All Clips (`■`)** button on the right edge of the track toolbar.
2. **Track Headers Column (`width = 168px`, below toolbar)**:
   - Horizontal track rows (`height = 54px`) stacked vertically (`Inst 1`, `Audio 2`, ...), plus `+` button below the last track to add new tracks, and pinned `Master` row at the bottom.
   - Each Track Header contains:
     - `4px` left color stripe (`track.color`).
     - Top row: Track Name + compact `●` (Arm), `S` (Solo), `M` (Mute) buttons (`18×16 px`).
     - Bottom row: Horizontal volume fader + mini level meter + Track Stop (`■`) button.
3. **Horizontal Clip Launcher (`visible when show_clip_launcher == true`)**:
   - **Top Scene Row (`height = 30px`)**: `▶ Scene 1`, `▶ Scene 2`, ..., `▶ Scene 8` (`width = 96px` each) + `+` scene button.
   - **Clip Matrix**: Rows = Tracks (aligned 1:1 vertically with Track Headers), Columns = Scenes.
   - Empty slots show a subtle stop icon `■`; populated slots show colored clip cards with `▶`/`■` state icon, clip name, and mini waveform preview.
4. **Arranger Timeline (`visible when show_arranger == true`)**:
   - Sits immediately to the right of the Clip Launcher (when both are active) or directly to the right of Track Headers (when only Timeline is active).
   - **Top Ruler (`height = 30px`)**: Bar numbers (`1`, `2`, `3`, ...) and orange loop region bar.
   - **Track Lanes**: Horizontal lanes aligned 1:1 vertically with Track Headers and Clip Launcher rows, rendering audio/MIDI clips and the vertical playhead.

### 2.3 Central Mix View (`StatusViewTab::Mix` — `arranger_view.rs`)
- **Left Scene Column (`width = 88px`, when `show_clip_launcher` is active)**:
  - Vertically stacked `▶ Scene 1..N` buttons aligned with each strip's vertical clip slots, plus `■ Stop All` and the Clip Launcher toggle at the bottom-left.
- **Vertical Channel Strips**:
  - Top color bar (`4px`) + Track Name header.
  - Vertical Clip Launcher slots (when `show_clip_launcher` is active).
  - Compact `DEVICES` chain list with `+` button.
  - Pan slider, `●` / `S` / `M` buttons, vertical Volume Fader + stereo VU Meter + dB readout.
  - `MASTER` strip pinned on the right.

### 2.4 Compact Bottom Device Panel (`dsp_rack.rs` & `open_wavetable.rs`)
- **Dock Height**: `247px` total (`RACK_HEIGHT = 241.0`).
- **Left Vertical Strip**: `PROJECT` / `TRACK` tab bar (`24px` wide).
- **Fixed-Width Device Cards**:
  - Cards enforce both `min_width` and `max_width` (`OPEN_WAVETABLE_CARD_WIDTH = 440.0`, `CARD_WIDTH = 340.0`) and avoid unbounded `Layout::right_to_left` inside horizontal scroll areas.
  - `OpenWavetable` renders its 3D/2D viewport (`190×112 px`) on the left and compact synth controls (`WT POS`, `Detune`, `Phase`, `Unison`, `Volume`, `Morph`, `Voices`, `Octave`, `Pitch`) on the right, followed immediately by the `+` button to add devices.

### 2.5 Right-Side Browser & WAV Drag-and-Drop (`explorer.rs`)
- Docked on the right side (`SidePanel::right("right_explorer")`, default width `270px`).
- File entries use `Sense::click_and_drag()` to initiate `state.dragged_sample = Some(path)`.
- While dragging, a floating cursor tooltip displays `🎵 <filename>`.
- Releasing the pointer over any **Clip Launcher slot** (in Arrange or Mix view) loads the WAV/MIDI into `(track_idx, scene_idx)` via `matrix::load_clip_into_slot`.
- Releasing the pointer over any **Arranger Timeline lane** creates an audio clip at `(track_idx, snapped_tick)` via `playlist::build_audio_clip` and syncs with `AudioProxy`.
