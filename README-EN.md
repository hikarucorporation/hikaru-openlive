# Hikaru OpenLive

> Modular, free and cross-platform DAW in Rust for live music production and performance.
> **Independent FOSS alternative to Ableton Live and Bitwig Studio.** *(In active development)*

[![License: AGPL v3](https://img.shields.io/badge/License-AGPLv3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)
[![Rust](https://img.shields.io/badge/rust-1.97%2B-orange.svg)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20Windows%20%7C%20FreeBSD-lightgrey.svg)](#-quick-build-linux)
[![GUI: egui](https://img.shields.io/badge/GUI-egui%200.27-ff4154.svg)](https://github.com/emilk/egui)
[![Audio: CPAL](https://img.shields.io/badge/Audio-CPAL%200.18-green.svg)](https://github.com/RustAudio/cpal)

**Hikaru OpenLive** is a digital audio workstation (DAW) designed for **live performance, real-time clip launching and linear composition**. Built entirely in Rust, it offers an ultra-low-latency audio engine with no dynamic memory allocations on the critical thread.

---

## 📸 Preview

### Arranger View
![Arranger View](assets/screenshots/arranger-view.png)
*Timeline for traditional composition, track editing and linear arrangement.*

### Session Matrix
![Session Matrix](assets/screenshots/session-matrix.png)
*Ableton/Bitwig-style clip grid for quantized scene triggering and real-time waveforms.*

### Piano Roll & MIDI Editor
![Piano Roll](assets/screenshots/piano-roll-dev-1.png)
*Sequenced MIDI note editing, clip editing and rhythmic control.*

### VST3 / CLAP Plugin Host (Experimental/Beta)
![Plugin Host](assets/screenshots/lgpl-vst3-clap-host-dev-1.png)
*Loading and management of third-party plugins in VST3 and CLAP formats.*

---

## 🚀 Key Features

- **Session Matrix (Live Workflow):** Scene-based clip launching, tight quantization and instant triggers.
- **Arranger View:** Timeline editing parallel to the session matrix.
- **Lock-Free Audio Engine:** Real-time audio thread powered by CPAL and a sample-accurate sequencer.
- **DSP & Native Synthesis:** Effect racks (filters, flanger, phaser) and integrated wavetable generator.
- **VST3 / CLAP Host:** Experimental integration for external instruments and effects.

---

## 🛠️ Quick Build (Linux)

```bash
# Base dependencies (Debian/Ubuntu)
sudo apt update && sudo apt install -y build-essential pkg-config git libasound2-dev libx11-dev libgl1-mesa-dev

# Clone and run
git clone https://github.com/hikarucorporation/hikaru-openlive.git
cd hikaru-openlive
cargo run --release -p hikaru_gui

```

## 🛠️ Full Release Build (Linux)

In your project's root folder (e.g. `/miyu-shrine-workspace/*`)

```bash
cargo build --release --bin hikaru_openlive
```

---

## Cross-compilation for Windows

Hikaru OpenLive can be compiled directly from Linux to generate a native Windows executable (`.exe`) using Rust's GNU target.

### Prerequisites

Make sure you have the toolchain and cross linker installed on Debian/Ubuntu:

```bash
# Install Rust toolchain for Windows x86_64
rustup target add x86_64-pc-windows-gnu

# Install MinGW-w64 compiler
sudo apt update && sudo apt install gcc-mingw-w64-x86-64

```

### Build

To generate the optimized production executable:

```bash
cargo build --release --target x86_64-pc-windows-gnu -p hikaru_gui

```

The resulting binary will be located at:
`target/x86_64-pc-windows-gnu/release/hikaru_gui.exe`

### Testing on Linux (Wine)

You can test the `.exe` directly using Wine:

```bash
wine target/x86_64-pc-windows-gnu/release/hikaru_gui.exe
```

---

# 🗺️ Roadmap & Next Steps (to switch graphics framework from **`egui`** to **`gpui-kit`**)

1. **Fixed buggy looping**: I think the `hikaru_audio_engine` crate broke and that's why looping works like crap. **[DONE]**
2. **Fixed Piano Roll**: Because the current Piano Roll is totally broken **[DONE]**
3. **Virtual MIDI Input from Piano Roll**: that **[DONE]**
4. **Wavetable and Spectral Synthesis and Effects**: that too
5. **Hikaru OpenDMS properly fixed**: Right now it's super buggy lol **[DONE]**
6. **Automation Clips**: Kind of similar to FL Studio's but well haha

---

## License

This program is free software under the terms of the **GNU Affero General Public License (AGPLv3)**. See [`LICENSE`](https://www.gnu.org/licenses/agpl-3.0.en.html) for more details.

---

### P.S.:

Personal note: Switch graphics framework from **`egui`** to **`gpui-kit`** once you've done most of the stuff in **`egui`**
