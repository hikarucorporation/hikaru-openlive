# Auditoría de paridad — Hikaru OpenLive vs Bitwig Studio 5.1.7

Fecha de auditoría: **2026-11-17**
Referencia instalada: **Bitwig Studio 5.1.7**, revisión **143754**
(`559a1bad96cc2565f28fbcefaeddec30e6fe4b53`), en `/opt/bitwig-studio`.

Este documento es la evidencia que respalda `features_bitwig.json`. El
porcentaje del gráfico mide **paridad funcional sobre un catálogo de features**,
no igualdad binaria ni cobertura línea por línea del producto propietario.

---

## 1. Instalación de Bitwig Studio 5.1.7

Datos leídos de `dpkg`, `/opt/bitwig-studio/resources/build-info.sh` y del
árbol `Library/` de la instalación local:

| Dato | Valor |
|---|---|
| Paquete | `bitwig-studio` 5.1.7 amd64 |
| Revisión | 143754 |
| Tamaño instalado | ~605 MB |
| Dispositivos nativos (`.bwdevice`) | **118** |
| Moduladores nativos (`.bwmodulator`) | **43** |
| Módulos de Grid (`.bwmodule`) | **218** |
| Scripts de controlador (`.control.js`) | **46** |
| Familias/vendors de controladores | **17** (más `api`, `doc` y `template`) |
| Temas instalados | **1** (`default.theme`) |
| Extensión de controladores | `Controllers.bwextension` + 16 extensiones de fabricante |

### Catálogo de dispositivos (118)

`Amp`, `Arpeggiator`, `Audio MOD`, `Audio Receiver`, `Bend`, `Bit-8`, `Blur`,
`Chain`, `Channel Filter`, `Channel Map`, `Chorus+`, `Chorus`, `Comb`,
`Compressor`, `Convolution`, `DC Offset`, `De-Esser`, `Delay-1`, `Delay-2`,
`Delay-4`, `Delay+`, `Distortion`, `Dribble`, `Drum Machine`, `Dual Pan`,
`Dynamics`, `Echo`, `E-Clap`, `E-Cowbell`, `E-Hat`, `E-Kick`, `EQ-2`, `EQ-5`,
`EQ+`, `EQ-DJ`, `E-Snare`, `E-Tom`, `Filter+`, `Filter`, `Flanger+`, `Flanger`,
`FM-4`, `Freq Shifter`, `Freq Split`, `FX Grid`, `FX Layer`, `FX Selector`,
`Gate`, `Harmonic Split`, `Harmonize`, `Humanize`, `HW Clock Out`,
`HW CV Instrument`, `HW CV Out`, `HW FX`, `HW Instrument`, `Instrument Layer`,
`Instrument Selector`, `Key Filter`, `Ladder`, `Latch`, `LFO MOD`, `Loud Split`,
`Micro-pitch`, `MIDI CC`, `MIDI Program Change`, `MIDI Song Select`,
`Mid-Side Split`, `Multiband FX-2`, `Multiband FX-3`, `Multi-note`,
`Note Delay`, `Note Filter`, `Note FX Layer`, `Note FX Selector`, `Note Grid`,
`Note Length`, `Note MOD`, `Note Receiver`, `Note Repeats`, `Note Transpose`,
`Organ`, `Oscilloscope`, `Peak Limiter`, `Phase-4`, `Phaser+`, `Phaser`,
`Pitch Shifter`, `Poly Grid`, `Polymer`, `Polysynth`, `Quantize`, `Randomize`,
`Replacer`, `Resonator Bank`, `Reverb`, `Ricochet`, `Ring-Mod`, `Rotary`,
`Sampler`, `Saturator`, `Spectrum`, `Step MOD`, `Stereo Split`, `Strum`,
`Sweep`, `Test Tone`, `Time Shift`, `Tool`, `Transient Control`,
`Transient Split`, `Transpose Map`, `Treemonster`, `Tremolo`, `Velocity Curve`,
`Vocoder`, `XY FX`, `XY Instrument`.

### Catálogo de moduladores (43)

`4-Stage`, `ADSR`, `AHD on Release`, `AHDSR`, `Audio Rate`, `Audio Sidechain`,
`Beat LFO`, `Button`, `Buttons`, `Channel-16`, `Classic LFO`, `Curves`,
`Envelope Follower`, `Expressions`, `Globals`, `HW CV In`, `Keytrack+`, `LFO`,
`Macro-4`, `Macro`, `Math`, `MIDI`, `Mix`, `Note Counter`, `Note Sidechain`,
`ParSeq-8`, `Pitch-12`, `Polynom`, `Quantize`, `Ramp`, `Random`,
`Relative Keytrack`, `Sample and Hold`, `Segments`, `Select-4`, `Stack Spread`,
`Steps`, `Vector-4`, `Vector-8`, `Vibrato`, `Voice Control`, `Wavetable LFO`,
`XY`.

### Capacidades de referencia relevantes

Observadas en la arquitectura de la instalación y en el modelo de producto
5.1.7:

- **Pistas**: instrumento, audio, híbrida, grupo, FX y master; grupos
  recursivos con plegado, color, entrada/salida de grupo y enrutamiento.
- **Clip Launcher**: clips por pista/escena, lanzamiento cuantizado, follow
  actions, launch modes, legato, grabación en slots, captura de escena y
  automatización de clip.
- **Arranger**: timeline lineal, clips de audio/MIDI, fades, comping, take
  lanes, marcadores, automatización, tempo y compás.
- **Audio**: warp markers, time-stretching en tiempo real, modos de warp,
  preservación de formantes, congelado/render y edición de onsets.
- **Dispositivos**: 118 dispositivos nativos, racks, capas, selectores,
  multiband splits y los tres Grids (Poly/FX/Note).
- **Modulación**: 43 moduladores, macros, modulación por voz, audio-rate,
  sidechain, CV y MPE.
- **Browser**: navegador emergente, filtros por metadata, colecciones,
  presets, packs, favoritos y drag-and-drop.
- **Mixer/I/O**: channel strips, sends, sidechain, VCA, latency compensation,
  cue/monitor, surround y calibración.
- **Controladores**: API de scripts, MIDI learn, MPE, Mackie/HUI, MIDI clock,
  Ableton Link y CV/gate.

---

## 2. Auditoría del workspace Hikaru OpenLive

Versión del workspace: **2.24.0** (`Cargo.toml`).
Método: lectura estática de todos los crates, sus módulos, tests y documentos.

| Crate | Qué aporta | Estado observado |
|---|---|---|
| `hikaru_core` | Buffers, notas MIDI, eventos de audio | Base estable |
| `hikaru_transport` | PPQN 960, tempo, compás, posición | Implementado |
| `hikaru_sequencer` | Matriz, estados de clip, cuantización | Implementado; follow actions y launch modes parciales |
| `hikaru_audio_engine` | Mezcla, voces, WSOLA, warp, sampler/wavetable | Implementado; grupos y streaming de stretch parciales |
| `hikaru_audio_drivers` | Backend ALSA | Implementado en Linux |
| `hikaru_dsp` | Filtro, flanger, phaser, LFO, Ultracomb, OpenSpectral | Implementado |
| `hikaru_plugin_host` | Host VST3 y CLAP, wrapper nativo | Implementado; sandbox/latencia parciales |
| `hikaru_midi` | Eventos MIDI, archivos, Launchpad, controladores | Base implementada; learn/MPE parciales |
| `hikaru_gui` | Matrix, Arranger, Piano Roll, Mixer, Browser, DSP rack, grupos | Núcleo de la paridad actual |
| `hikaru_render` | Render WGPU, mallas, knobs | Implementado |
| `hikaru_project_file` / `hikaru_openlive_file` / `hikaru_openstudio_file` | Persistencia `.oplf`/`.opsf` | Implementado |
| `hikaru_cli` | Consola, proyectos, man pages | Implementado |
| `hikaru_mcp_port` / `hikaru_mcp` | Puerto y servidor MCP para agentes | Implementado |
| `vst3-host-patched` | Host VST3 parcheado (path dependency) | Implementado |

### Features recientes verificadas

| Feature | Evidencia | Estado |
|---|---|---|
| Group Tracks recursivos | `views/mixer.rs` (`is_group`, `parent_group_id`, `track_group_depth`), `views/matrix.rs` | `done` (UI/estructura); suma de bus en motor `planned` |
| Scene Launcher | `views/scenes_headers.rs`, `views/matrix.rs`, `sequencer/quantizer.rs` | `done` |
| Time-stretching en tiempo real | `audio_engine::time_stretch_wsola`, `time_stretch_wsola_warped`, warp markers | `done` (WSOLA); streaming/auto-warp `in_progress` |
| Browser | `views/popup_browser.rs`, `views/explorer.rs`, `plugin_host/registry.rs` | `done` (núcleo); packs/colecciones `in_progress` |
| Mixer | `views/mixer.rs`, `ui/meters.rs`, `audio_engine` | `done` (strips, pan, mute/solo, master); sends `in_progress`; latencia `planned` |
| Piano Roll | `views/piano_roll.rs` + tests tier 1–4 | `done` con bugs conocidos de UI |

---

## 3. Base de datos de paridad

`features_bitwig.json` contiene 13 áreas y 616 features. Cada feature tiene
`id`, `name` y `status` (`done`, `in_progress`, `planned`); las features
implementadas o parciales incluyen `evidence` con la ruta del código.

| Área | Features | Done | In progress | Planned | Bugs |
|---|---:|---:|---:|---:|---:|
| Clip Launcher | 26 | 21 | 0 | 5 | 0 |
| Arranger | 28 | 14 | 4 | 10 | 0 |
| Transport & Sync | 18 | 12 | 1 | 5 | 0 |
| Group Tracks & Routing | 22 | 14 | 2 | 6 | 0 |
| Time-stretching & Audio | 25 | 14 | 2 | 9 | 0 |
| Built-in Devices & FX | 118 | 9 | 3 | 106 | 0 |
| Modulators | 43 | 1 | 1 | 41 | 0 |
| Grid Devices | 218 | 97 | 0 | 121 | 0 |
| Mixer & I/O | 27 | 18 | 1 | 8 | 0 |
| Browser & Files | 24 | 15 | 2 | 7 | 0 |
| MIDI & Controllers | 21 | 9 | 0 | 12 | 0 |
| UI & Workflow | 30 | 20 | 0 | 10 | 0 |
| Build & Platform | 16 | 10 | 2 | 4 | 1 |
| **Total** | **616** | **254** | **18** | **344** | **1** |

Paridad global: **254 / 616 = 41 %**.

### Bugs abiertos

Consulta a la API de GitHub del repositorio
`hikarucorporation/hikaru-openlive` el 2026-11-17: **1 issue abierto**.

| Issue | Título | Área asignada |
|---|---|---|
| [#1](https://github.com/hikarucorporation/hikaru-openlive/issues/1) | `compiling error on Zorin os 18` | `build-platform` |

El issue reporta warnings de compilación (`redundant_semicolons`,
`unused_variables`, `dead_code`) en `dsp_rack.rs` y `piano_roll.rs`. No hay
labels `area:` en el repositorio, así que el mapeo a área es manual.

---

## 4. Límites de la medición

- Bitwig Studio es software cerrado: no se auditó su código fuente, sólo su
  instalación, catálogo de dispositivos/moduladores y comportamiento público.
- Los 118 dispositivos y los 43 moduladores se cuentan **uno por uno** porque
  son artefactos reales del producto de referencia. Hikaru implementa
  equivalentes funcionales parciales (p. ej. `Polymer` ≈ Hikaru OpenPoly,
  `Drum Machine` ≈ OpenDMS), no binarios.
- Una feature se marca `done` sólo si hay código y, cuando existe, test que la
  respalde. `in_progress` significa modelo/UI parcial sin flujo completo.
- El gráfico usa el mismo criterio que el status de Suhina: porcentaje =
  `done / total` de features rastreadas, y el área de cada tile = cantidad de
  features del área.
