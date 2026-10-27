# Ingeniería inversa de UZU — reporte

**Objetivo:** `/home/user/.vst3/UZU.vst3` (VST3, propietario, vendor CRQL, v1.0.4)
**Método:** `hikaru_mcp` (servidor MCP de Hikaru OpenLive) + Ghidra 12.3 headless
**Fecha:** 2026-10-22

## 1. Lo que se sabía antes de empezar

Casi nada: un plugin VST3 de efecto ("Fx") hecho en C++/JUCE para Windows,
portado a Linux. El único dato público era el nombre del binario y la
extensión `.vst3`.

## 2. Lo que se extrajo

### 2.1 Identidad (sin cargar el plugin)

`plugin_info` + `moduleinfo.json` del bundle:

- **Nombre:** UZU
- **Versión:** 1.0.4
- **Vendor:** CRQL
- **SDK:** VST 3.8.0
- **CID (componente):** `ABCDEF019182FAEB4352514C43727133`
- **CID (controlador):** `ABCDEF011234ABCD4352514C43727133`
- **Subcategoría:** Fx
- **Binario:** `Contents/x86_64-linux/UZU.so`, 7.5 MB, ELF x86-64, **no stripped**
- **Entry points:** `ModuleEntry@0x99800`, `ModuleExit@0x99820`, `GetPluginFactory@0x99840`
- **Framework:** JUCE (símbodo `_ZN4juce18moduleEntryCounterE`) + libpd (Pure Data)

### 2.2 Los parámetros (instanciando el plugin)

`plugin_parameters` hizo `dlopen` del binario y leyó los 11 parámetros:

| # | Nombre | Rango | Default |
|---|--------|-------|---------|
| 0 | Width | 0..1 | 0.5 |
| 1 | Offset | 0..1 | 0 |
| 2 | Depth | 0..1 | 0 |
| 3 | Speed | 0..1 | 0.5 |
| 4 | Mix | 0..1 | 1 |
| 5 | Blur | 0..1 | 0 |
| 6 | Hz/Oct | 0..1 | 0.5 |
| 7 | 1x/5x | 0..1 | 0 |
| 8 | Keep Bass | 0..1 | 0 |
| 9 | Blocksize | 0..1 | 0 |
| 10 | Bypass | 0..1 | 0 |

### 2.3 El motor DSP: Pure Data embebido

El DSP no está en C compilado: está en un **patch de Pure Data** embebido
como recurso binario (`BinaryData::EBPhaselock_pd`). El binario de UZU es en
realidad un **host de libpd** (Pure Data 0.55.2) que carga el patch al
inicializarse.

Patchs embebidos encontrados:
- `EBPhaselock.pd` (el principal)
- `HipBW.pd`, `LopBW.pd` (filtros de banda)
- `hilbert.pd` (transformada de Hilbert)
- `insert.pd`, `hann.pd`

Librerías Pd embebidas: `bob~`, `bonk~`, `fiddle~`, `loop~`, `lrshift~`,
`pd~`, `pique`, `sigmund~`, `helmholtz~` (pitch tracker), `mayer_fft`/`mayer_ifft`.

### 2.4 Los patches Pd reales (extraídos del binario)

**Corrección:** el archivo que antes se guardó como `EBPhaselock.pd` (3.239
bytes) no era el patch principal, sino `HipBW.pd`, un filtro de banda de 3
polos. El patch principal real mide **22.487 bytes** y es un procesador
**espectral con FFT**, no un phaser de biquads.

Los seis recursos Pd ya están extraídos en
`docs/reverse-engineering/uzu/patches/`:

```
EBPhaselock.pd   patch principal (FFT 4096, overlap 4, keep bass, modulación)
HipBW.pd         filtro de banda alto (biquads en cascada, ELSE)
LopBW.pd         filtro de banda bajo (biquads en cascada, ELSE)
hann.pd          ventana Hann (ELSE)
hilbert.pd       transformada de Hilbert (no usada por el patch principal)
insert.pd        utilidad de inserción (no usada por el patch principal)
```

Cadena principal:

```
adc~ ──┬──> LOP (keep bass) ──┬──> FilterCalc (FFT 4096) ──> dac~
       │                      └──> delay~ 16384 (compensación) ──> dac~
```

`FilterCalc` hace `hann → rfft~ → /1024 → cartopol~ → curva espectral →
poltocar~ → rifft~ → hann → *0.5`. La curva se construye con `Freq`, `Width`,
`Blur`, `Mix`, `Param_4`/`Param_5`, `ConversionArray` y `LINFILT`.

Los detalles completos (direcciones, tamaños, mapeo de los 10 parámetros y por
qué la implementación Rust actual suena a low-pass) están en
[`PATCHES.md`](PATCHES.md).

### 2.5 El wrapper JUCE

`AudioPluginAudioProcessor` (JUCE) envuelve el patch de Pd:
- `setupPdAudioIfNeeded(sampleRate, inChannels, outChannels)`: inicializa
  libpd con la tasa de muestreo y los canales.
- `processBlock`: llama a `processBlockOffline`, que ejecuta el patch.
- La latencia se calcula dinámicamente con `blocksizeChoices` (pipeline
  latency de 100 a ~? muestras).

### 2.6 Pseudocode C (Ghidra)

Extraído con `decompile_plugin_stub` (Ghidra 12.3 headless):

- `GetPluginFactory` (el factory VST3) → crea la estructura con "CRQL"
- `setup_freq0x2eshift_tilde` → registra el objeto Pd `freq.shift~`
- `processBlock` → el callback de audio
- `setupPdAudioIfNeeded` → la inicialización de libpd

Los archivos están en `docs/reverse-engineering/uzu/`.

## 3. Lo que no se pudo extraer

- **El código C++ del wrapper JUCE**: solo se tienen funciones sueltas
  (`processBlock`, `processBlockOffline`, `setupPdAudioIfNeeded`, etc.).
- **El layout exacto de la GUI**: el plugin no expone editor propio
  (`tiene_gui: false`); los objetos GUI del patch son internos.
- **Los recursos de UI** (`keepbass.png`, `xy.png`, fuentes) no hacen falta
  para el DSP y no se exportaron.
- **El binario libpd/ELSE estático**: el plugin los enlaza dentro de
  `UZU.so`, pero no se pueden reutilizar como bibliotecas separadas.

## 4. Cómo reproducir

```sh
# 1. Arrancar el servidor MCP
./hikaru_mcp --mode files

# 2. En otra terminal, o desde el agente:
#    - overview del plugin
#    - decompile GetPluginFactory
#    - params del plugin
#    - strings del binario

# O desde la consola:
./hikaru_mcp --cli
hikaru_mcp> overview /home/user/.vst3/UZU.vst3
hikaru_mcp> decompile /home/user/.vst3/UZU.vst3 GetPluginFactory
hikaru_mcp> params /home/user/.vst3/UZU.vst3
```

## 5. Archivos generados

```
docs/reverse-engineering/uzu/
├── AudioPluginAudioProcessor.txt   (constructor del procesador JUCE)
├── processBlock.txt                (callback de audio)
├── setupPdAudioIfNeeded.txt        (inicialización de libpd)
├── setupParameters.txt             (configuración de los 11 parámetros)
├── shiftAt.txt                     (utilidad de buffer de Steinberg)
├── PATCHES.md                      (extracción y análisis de los patches reales)
├── EBPhaselock.pd                  (patch principal real, 22.487 bytes)
└── patches/
    ├── EBPhaselock.pd              (patch principal, FFT espectral)
    ├── HipBW.pd                    (filtro de banda alto, ELSE)
    ├── LopBW.pd                    (filtro de banda bajo, ELSE)
    ├── hann.pd                     (ventana Hann, ELSE)
    ├── hilbert.pd                  (Hilbert, no usado por el principal)
    ├── insert.pd                   (utilidad, no usado por el principal)
    └── HipBW_legacy_3239.pd        (el archivo mal identificado que había antes)
```
