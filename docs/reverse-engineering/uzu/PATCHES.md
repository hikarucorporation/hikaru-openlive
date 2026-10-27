# UZU — parches Pd extraídos del binario

## Corrección importante

El archivo `docs/reverse-engineering/uzu/EBPhaselock.pd` que existía antes (3.239 bytes)
**no era el patch principal**. Era `HipBW.pd`, un filtro de banda de 3 polos.
Por eso la reimplementación en Rust sonaba a low-pass: estaba portando el filtro
equivocado.

El patch principal real mide **22.487 bytes** y ahora está en:

- `docs/reverse-engineering/uzu/EBPhaselock.pd`
- `docs/reverse-engineering/uzu/patches/EBPhaselock.pd`

El archivo viejo se conserva como `patches/HipBW_legacy_3239.pd`.

## Cómo se extrajeron

Los patches están embebidos como recursos JUCE (`BinaryData::temp_binary_data_N`).
Los punteros `BinaryData::*_pd` viven en `.data.rel.ro` y se inicializan con
relocations `R_X86_64_RELATIVE` que apuntan a `.rodata`. Copiando esos bytes
se obtiene el recurso exacto:

| Archivo | Símbolo | Dirección virtual | Tamaño (con NUL) |
|---|---|---|---|
| `EBPhaselock.pd` | `BinaryData::EBPhaselock_pd` | `0x523ac0` | 22.488 |
| `HipBW.pd` | `BinaryData::HipBW_pd` | `0x51c4c0` | 3.240 |
| `LopBW.pd` | `BinaryData::LopBW_pd` | `0x51d250` | 3.253 |
| `hann.pd` | `BinaryData::hann_pd` | `0x51df10` | 1.143 |
| `hilbert.pd` | `BinaryData::hilbert_pd` | `0x51e390` | 909 |
| `insert.pd` | `BinaryData::insert_pd` | `0x51e720` | 1.416 |

Los archivos se guardaron sin el NUL final. También hay recursos de UI
(`keepbass.png`, `xy.png`, fuentes) que no hacen falta para el DSP.

## Cadena de señal del patch principal

```
adc~ ──┬──> LOP (keep bass) ──┬──> FilterCalc (FFT 4096, overlap 4) ──> dac~
       │                      └──> delay~ 16384 (compensación) ───────> dac~
       └──> ...
```

### `FilterCalc` (el corazón espectral)

- `hann` → `rfft~` → `/~ 1024` → `cartopol~`
- multiplica magnitud/fase por la curva espectral (`Filter1` / `LINFILT`)
- `poltocar~` → `rifft~` → `hann` → `*~ 0.5` → salida
- `block~ 4096 4` (ventana 4096, overlap 4)
- La curva se construye con `r Freq`, `r Width`, `r Blur`, `r Mix`,
  `r Param_4`, `r Param_5`, `ConversionArray` y `LINFILT`.

### `LOP` (keep bass)

- Dos ramas: `LopBW 250 3` y `HipBW 250 3` (cascadas de biquads de orden 3).
- Crossfade controlado por `Param_8` (Keep Bass).
- Salida a `FilterCalc` y a los delays de compensación.

### Abstracciones

- `HipBW.pd` / `LopBW.pd`: calculan coeficientes de biquads y usan `biquads~`
  (ELSE) para la cascada.
- `hann.pd`: ventana Hann de ELSE (`blocksize~`, `v`, etc.).
- `hilbert.pd` / `insert.pd`: recursos presentes en el binario pero **no
  referenciados por el patch principal**.

## Mapeo real de parámetros

`processBlockOffline` manda 10 floats a Pd con los nombres `Param_1`..`Param_10`.
Leyendo el vector de parámetros de JUCE en el orden en que lo recorre el código:

| # VST | Nombre VST | `Param_N` | Destino en el patch |
|---|---|---|---|
| 0 | Width | `Param_1` | `s Freq` (curva de frecuencia) |
| 1 | Offset | `Param_2` | velocidad del LFO (`phasor~`) |
| 2 | Depth | `Param_3` | `s Width` (ancho de la curva) |
| 3 | Speed | `Param_4` | profundidad/curva + `phasor~` |
| 4 | Mix | `Param_5` | `s Mix` |
| 5 | Blur | `Param_6` | `s Blur` |
| 6 | Hz/Oct | `Param_10` | escala `0.25..2` + `ResetBang` |
| 7 | 1x/5x | `Param_9` | multiplicador `1` u `8` |
| 8 | Keep Bass | `Param_8` | `LOP` |
| 9 | Blocksize | `Param_7` | selector `512..16384` |
| 10 | Bypass | — | nativo (no va a Pd) |

Ojo: los nombres de la UI no coinciden semánticamente con los nombres internos
del patch (`Freq`, `Width`, `Phase`). El mapeo de arriba es el que hace el
código, no una interpretación de los labels.

## Por qué la implementación Rust actual no suena a UZU

`crates/hikaru_dsp/src/effects/openspectral_phaser.rs` implementa:

- una cascada de 4 biquads,
- un LFO que mueve el cutoff,
- un `keep_bass` con un low-pass de 120 Hz,
- y nada de FFT.

Eso es un phaser clásico, no el procesador espectral de UZU. Le faltan:

- FFT real de 4096 con overlap 4 y ventana Hann,
- la curva espectral (`Filter1`/`LINFILT`/`ConversionArray`),
- el crossfade de `LOP` con `LopBW`/`HipBW`,
- el mapeo real de los 10 parámetros,
- la compensación de latencia de 16384 muestras,
- el comportamiento de `blocksize`.

## Siguiente paso

Hay tres caminos realistas:

1. **Portar el patch a Rust** (FFT + curvas + LOP). Más trabajo, cero dependencias
   nuevas, integrable en Hikaru.
2. **Embeber libpd + estos patches** en el plugin. Bit-exacto, pero suma libpd
   y ELSE como dependencias C.
3. **Plugin VST3/JUCE con libpd** (el camino original de UZU). Es el más fiel y
   el más rápido de poner a sonar, pero es un proyecto C++ aparte.
