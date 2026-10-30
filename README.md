# Hikaru OpenLive

> Modular, free and cross-platform DAW in Rust for live music production and performance.
> **Independent FOSS alternative to Ableton Live and Bitwig Studio.** *(In active development)*

[![License: AGPL v3](https://img.shields.io/badge/License-AGPLv3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)
[![Rust](https://img.shields.io/badge/rust-1.97%2B-orange.svg)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20Windows%20%7C%20FreeBSD-lightgrey.svg)](#-quick-build-linux)
[![GUI: gpui-kit](https://img.shields.io/badge/GUI-gpui--kit-6E4AFF.svg)](https://github.com/zed-industries/gpui)
[![Audio: CPAL](https://img.shields.io/badge/Audio-CPAL%200.18.2-green.svg)](https://github.com/RustAudio/cpal)

**Hikaru OpenLive** is a digital audio workstation (DAW) designed for **live performance, real-time clip launching and linear composition**. Built entirely in Rust, it offers an ultra-low-latency audio engine with no dynamic memory allocations on the critical thread.

---

## 📸 Preview

### Arranger View
![Arranger View](assets/screenshots/arranger-view.png)
*Timeline for traditional composition, track editing and linear arrangement.*

### Session Matrix
![Session Matrix](assets/screenshots/session-matrix.png)
*Ableton/Bitwig-style clip grid for quantized scene triggering and real-time waveforms.*

### About
![About](assets/screenshots/about.png)
*Version info, credits and project links.*

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

## 🧭 Project Direction: Reverse Engineering Bitwig Studio

We're making a **call to the open-source audio community**: the goal of **Hikaru OpenLive** is to perform **reverse engineering of Bitwig Studio**.

Why Bitwig? Because the open-source community lacks **free/libre and open-source software (FOSS)** that lets you **loopear** like the proprietary programs — Ableton Live and Bitwig Studio — and especially not Bitwig, which owns the deepest live-looping / session-view / clip-launching workflow in the industry.

Ableton and Bitwig perfected the realtime looping, clip launching and performance workflows, and the open-source world couldn't reach that level. **Hikaru OpenLive is our answer**: a free and open-source DAW built with the single purpose of **reverse engineering those proprietary workflows** and re-implementing them from scratch in Rust — session matrix, arranger, DSP, realtime engine — without depending on any closed-source code.

If you want to help steer this direction: this is the **north star**. 🚀

### Bitwig Is Doing Programmed Obsolescence

**Bitwig is doing programmed obsolescence on certain Intel processors** (like my current one, an i3-3110m).

The point: **Hikaru OpenLive doesn't demand an Intel NASA-grade processor** and **costs nothing** — while **Bitwig charges three mortgages, an egg and half of the other**, and even if you spend all your money on a Bitwig license, **they can technically revoke it at any time** (legally, it's not yours).

On top of that, Bitwig leans on **instruction sets** (like AVX1, AVX2, and whatever comes next) as an excuse to **ban you from running Bitwig on an "old" processor**.

### 🔧 Reverse Engineering Accepted: Ghidra, REA and the Hikaru MCP Port

Reverse engineering of proprietary software is **explicitly accepted** here. We welcome:

* **Ghidra** — the open-source reverse engineering suite.
* **REA** — our reverse engineering agent pipeline (OpenCode + Frida/Ghidra) used to dig into closed-source binaries.
* **The Hikaru OpenLive MCP port / server** — the bridge that turns legacy closed-source code (C/C++, JUCE, Steinberg SDK) into readable Rust-like pseudocode that AI agents (Claude Code, OpenCode, Cursor, Kimi Code, Google Antigravity, etc.) can consume.

Applied to proprietary plugins like **Serum 1 and 2**, **reFX Nexus** and other legacy C/C++ instruments/effects, so that hard-earned knowledge gets turned into **free/libre and open-source software** (Rust) for the whole community — under the same AGPLv3 terms.

### Hikaru OpenLive MCP Port

The **Hikaru OpenLive MCP port** is not just for reverse engineering — it also **connects Hikaru OpenLive to AI agents** like **Claude Code, OpenCode**, or any agentic CLI AI.

It also serves **development**: analyzing bugs, adding features, refactoring, and so on.

And in the future, **music production**: for example, having **Google Antigravity** (just as an example) generate a **Hikaru OpenStudio File (`.opsf`)** with all the samples, audio clips and MIDI clips properly placed, with structure and all.

This is **a thousand times better than Suno/Udio**, which can't make a proper dubstep track to save their lives — full of artifacts and white noise self-perceived as growls and heavy basses. Enough of that garbage for Bass Music (Dubstep, Future Riddim, Hi-Tech, Future Bass, etc.). With Hikaru OpenLive + an AI agent, you get full control and a real result.

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

## 📖 Manual Page

The CLI ships a native man page, so `man hikaru_openlive` works on any
Unix/BSD/Linux system:

```bash
# Install it for the current user (no root needed):
make man-install-user

# Or for the whole system:
sudo make man-install PREFIX=/usr

man hikaru_openlive
```

The page lives in [`crates/hikaru_cli/man/hikaru_openlive.1`](crates/hikaru_cli/man/hikaru_openlive.1)
(mirrored in [`docs/man/`](docs/man/)) and is kept in sync with the real CLI
grammar by the test suite. See
[`docs/hikaru-openlive-cli/`](docs/hikaru-openlive-cli/hikaru-openlive-cli-README.md)
for the details.

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

### 🗺️ Roadmap & Next Steps: Reverse Engineering Bitwig (legacy Java/C++ → Rust)

1. **Clip Launcher (Session Matrix)**: scene/clip triggering, quantized launching, real-time waveforms — Bitwig's deepest live workflow. **[IN PROGRESS]**
2. **Playlist**: clip arrangement, scenes, follow actions, looping — Bitwig's session-view side.
3. **Arrange / Mix / Edit views**: linear timeline, mixer console, edit view — layout, panels and button positions reverse-engineered.
4. **Piano Roll & MIDI editor**: note editing, clip editing, rhythmic control. **[IN PROGRESS]**
5. **Mixer & DSP rack**: channel strips, filters, flanger, phaser, wavetable/spectral synthesis — from proprietary instruments. **[IN PROGRESS]**
6. **Reverse-engineer proprietary plugins**: Serum 1 & 2, reFX Nexus, UZU (spectral phaser) — convert legacy C/C++/JUCE to Rust. **[IN PROGRESS]**
7. **Automation clips**: Bitwig/FL Studio-style automation, real-time modulation.
8. **Hardware & MIDI mapping**: controllers, physical input, real-time timestretching, bug fixes.
9. **GPU-based UI (gpui-kit)**: keep the proprietary-DAW look & feel with free/open-source software. **[DONE]**

---

## 👥 Community & Contributors

We're **looking for contributors** to keep pushing **Hikaru OpenLive** forward! 🙌

Special thanks to all the contributors and the open-source community supporting **Hikaru OpenLive**:

* **Contributors:** [View direct contributors list](https://github.com/hikarucorporation/hikaru-openlive/graphs/contributors)
* **Forks & Extensions:** [Explore community forks](https://github.com/hikarucorporation/hikaru-openlive/network/members)

Creators:

* [Hikaru Corporation](https://github.com/hikarucorporation)
* [DTG](https://github.com/doujintechnologicsgroup)
* 🤖 [OpenCode](https://github.com/opencode-agent)

### 🤝 Code Contributors Wanted

We're actively **seeking new code contributors** for the Rust codebase of **Hikaru OpenLive**! If you want to dive into real-time audio engines, lock-free Rust, DSP, GPU rendering or hardware integration — this is your invitation.

* **Pull Requests are highly welcome!** (MIDI controller mapping, physical hardware support, real-time timestretching, and bug fixes are top priority).

### 🤖 AI-Assisted Issues: With or Without Agents

**We accept all types of issues**, whether they come from a human or from an AI coding agent such as:

* Claude Code
* OpenCode
* Cursor
* Kimi Code
* Google Antigravity
* ...or any other AI agent of your choice (Codex, Gemini CLI, Qwen Code, DeepSeek, Copilot, etc.).

That includes bug reports, feature requests, reproductions and Pull Requests — **with or without AI agents**. If an AI agent helped you triage, reproduce or even fix something, **open the issue anyway**: the agent's findings, logs or proposals are more than welcome. Plain human-written issues are equally valid and always accepted.

Don't hesitate: open the issue however it came to be. If it's valid, it's valid. 🚀

---

## License

This program is free software under the terms of the **GNU Affero General Public License (AGPLv3)**. See [`LICENSE`](https://www.gnu.org/licenses/agpl-3.0.en.html) for more details.

The file-format and audio-driver libraries (`hikaru_project_file`, `hikaru_openlive_file`, `hikaru_openstudio_file`, `hikaru_audio_drivers`) are **LGPL-3.0-or-later** so third-party DAWs can use them without opening the engine. Their license text is in [`LICENSE-LGPL-3.0`](LICENSE-LGPL-3.0).

The project vendors three third-party crates with their own licenses: `gpui-pre` / `gpui-pre-linux` (Zed, **Apache-2.0** upstream, relicensed here to **LGPL-3.0-or-later**) and `vst3-host` (**MIT** upstream, relicensed here to **LGPL-2.1-or-later**). See [`docs/licencias.md`](docs/licencias.md) for the full license map and the per-crate `NOTICE` files.

---

### 🛡️ Note on Development and Licensing (GNU AGPLv3)

To those in the community crying *"AI Slop"* just for using LLMs/OpenCode to translate architecture and design vision into functional Rust code:

> **Hikaru OpenLive / OpenStudio is 100% Free/Libre and Open Software under the GNU AGPLv3 license.** (as stated above)

This means that:
1. You can use, study, modify, redistribute it and even commercialize it or deploy it on Cloud services.
2. **DON'T BE A SH!T:** If you use Hikaru OpenLive code for a cloud service (like Ableton Cloud, FL Studio Web, Splice or SaaS platforms), AGPLv3 **explicitly requires you to release the source code of your modifications** to the community.
3. You don't need to release your server's private infrastructure, but **ANY change, wrapper, kernel/OS-level binding or adaptation of the Hikaru OpenLive engine MUST BE PUBLISHED** under the same license. Tivoization and closed code in the cloud are strictly prohibited.

If you didn't understand how free software works under AGPLv3 nor how artificial intelligence is used as a high-level idea compiler... the fault is not in the code, it's on you as an irresponsible sysadmin XD. 🚀

---
