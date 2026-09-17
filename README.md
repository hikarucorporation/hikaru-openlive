# Hikaru OpenLive

> DAW modular, libre y multiplataforma escrito en Rust. Tiempo real, baja latencia y UI inmediata.

[![License: AGPL v3](https://img.shields.io/badge/License-AGPLv3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)
[![Rust](https://img.shields.io/badge/rust-1.97%2B-orange.svg)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20Windows%20%7C%20FreeBSD-lightgrey.svg)](#instalación)
[![GUI: egui](https://img.shields.io/badge/GUI-egui%200.27-ff4154.svg)](https://github.com/emilk/egui)
[![Audio: CPAL](https://img.shields.io/badge/Audio-CPAL%200.18-green.svg)](https://github.com/RustAudio/cpal)

**Hikaru OpenLive** es una estación de trabajo de audio digital (DAW) de código abierto y multiplataforma (Linux, Windows y FreeBSD) escrita en Rust enfocado en:

- **Rendimiento en tiempo real** sin allocations ni bloqueos en el hilo de audio.
- **UI inmediata con `egui`** — Session View por escenas, Mixer Rack flotante y editor de clips.
- **Motor de audio con CPAL** + secuenciador sample-accurate con compensación de latencia.

---

## Vista Previa

![Hikaru OpenLive UI Preview](assets/screenshots/preview.png)

*Session Matrix disparando escenas, File Explorer integrado con preview de samples, Clip Track Editor y Mixer Rack con vúmetros en tiempo real.*

---

## Características Principal

- **Session Matrix / OpenLive:** Grilla de clips con lanzamiento cuantizado y triggers por escena.
- **Mixer:** Canales independientes + Master, faders, mute/solo/arm y vúmetros Peak/RMS.
- **Transporte global:** Play/stop/record, BPM, metrónomo y loop con Time Selection.
- **Secuenciador (`hikaru_sequencer`):** Scheduler sample-accurate, lookahead y prioridad rítmica en el hilo de audio.
- **DSP Rack (`hikaru_dsp`):** Filtros, flanger, phaser, ultracomb, open-harmonic y sintesis wavetable nativa.
- **Host de plugins (`hikaru_plugin_host`):** Soporte VST3 y CLAP.

---

## Compilación Rápida (Linux / Debian / Ubuntu)

```bash
# 1. Instalar dependencias base
sudo apt update && sudo apt install -y build-essential pkg-config git libasound2-dev libx11-dev libgl1-mesa-dev

# 2. Clonar y ejecutar
git clone https://github.com/hikarucorporation/hikaru-openlive.git
cd hikaru-openlive
cargo run --release -p hikaru_gui

```

---

## Compilación Cruzada para Windows (Cross-compilation)

Hikaru OpenLive puede compilarse directamente desde Linux para generar el ejecutable nativo de Windows (`.exe`) utilizando el target GNU de Rust.

### Requisitos previos

Asegurate de tener instalado el toolchain y el linker cruzado en Debian/Ubuntu:

```bash
# Instalar el toolchain de Rust para Windows x86_64
rustup target add x86_64-pc-windows-gnu

# Instalar el compilador MinGW-w64
sudo apt update && sudo apt install gcc-mingw-w64-x86-64

```

### Compilación

Para generar el ejecutable optimizado de producción:

```bash
cargo build --release --target x86_64-pc-windows-gnu -p hikaru_gui

```

El binario resultante se encontrará en:
`target/x86_64-pc-windows-gnu/release/hikaru_gui.exe`

### Pruebas en Linux (Wine)

Podés probar el ejecutable `.exe` directamente usando Wine:

```bash
wine target/x86_64-pc-windows-gnu/release/hikaru_gui.exe
```

---

## Licencia

Este programa es software libre bajo los términos de la **GNU Affero General Public License (AGPLv3)**. Ver [`LICENSE`](https://www.gnu.org/licenses/agpl-3.0.en.html) para más detalles.
