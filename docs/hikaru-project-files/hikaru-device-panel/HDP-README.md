# Especificación del Formato Hikaru Device Panel (`.hdp`)

**Versión de especificación:** 1.0  
**Versión del software:** 2.24.0  
**Licencia de formato:** LGPL-3.0-or-later  
**MIME Type:** `application/x-hikaru-device-panel` / `application/json`  

---

## 1. Filosofía y Propósito

El formato **Hikaru Device Panel (`*.hdp`)** es la especificación estándar y abierta para almacenar, exportar, importar y compartir **presets individuales de dispositivos** y **cadenas completas de efectos/instrumentos (*Device Chains*)** dentro del ecosistema **Hikaru OpenLive** y **Hikaru OpenStudio**.

> **Nota de identidad:** Su nombre oficial es **Hikaru Device Panel**, aunque en la comunidad y en el estudio se lo conoce cariñosamente por su acrónimo rioplatense **HDP** (*Hijo de Puta*), en homenaje a esos growls y patches tan pesados y agresivos que ameritan el apodo.

### 1.1 ¿Por qué nace este formato?
En la producción musical electrónica moderna (Dubstep, Melodic Bass, Drum & Bass, Future Bass), un sonido complejo no depende únicamente de un sintetizador aislado: depende de la interacción entre el oscilador (por ejemplo, `Hikaru OpenWavetable` con tablas como `Uber Growl.wav`) y sus moduladores interactivos en tiempo real (como `Hikaru OpenModulation` ruteado por LFO sincronizado a compás contra el `WT POS`).

Mientras que el archivo de proyecto `.oplf` contiene la sesión entera de lanzamiento en vivo y el `.opsf` contiene la línea de tiempo del arreglo:
* El formato **`.hdp`** es puramente modular, liviano y portable.
* Permite al productor guardar un preset de bajo o sintetizador y reutilizarlo en cualquier pista o compartirlo con otros músicos sin tener que exportar el proyecto entero.

---

## 2. Tipos de Presets Soportados (`preset_type`)

Un archivo `.hdp` maneja dos variantes operativas mediante el campo `"preset_type"`:

1. **`SingleDevice` (Preset Individual):**
   Almacena el estado de un único dispositivo del Device Panel (un sintetizador como `OpenWavetable` o un modulador como `OpenModulation`).
2. **`Chain` (Cadena Completa de Pista / Device Rack):**
   Almacena la lista ordenada de dispositivos de una pista entera, preservando intactos los ruteos de modulación entre ellos (por ejemplo, el modulador LFO apuntando al parámetro `WT POS` del sintetizador precedente).

---

## 3. Esquema del Archivo JSON (`.hdp`)

### 3.1 Ejemplo: Preset de Cadena Completa (`Chain`) — Growl Estilo Skrillex
```json
{
  "format_version": 1,
  "created_by": "Hikaru OpenLive 2.24.0",
  "preset_type": "Chain",
  "name": "Skrillex Monster Growl Bass",
  "author": "Producer",
  "category": "Bass / Dubstep",
  "description": "Aggressive vocal growl using Uber Growl wavetable with 2/1 tempo-synced LFO modulation on WT POS",
  "slot": null,
  "chain": [
    {
      "id": 1,
      "name": "OpenWavetable",
      "active": true,
      "wavetable_path": "/../../../Xfer/Serum 2 Presets/Uber Growl.wav",
      "plugin_path": null,
      "plugin_format": null,
      "open_wavetable": {
        "table_name": "Uber Growl",
        "position": 0.640625,
        "morph": 0.0,
        "frame": 41,
        "detune": 0.0,
        "phase": 0.0,
        "unison": 1,
        "octave": -3,
        "pitch": 0,
        "voices": "Poly",
        "render_mode_3d": true,
        "render_mode_lines": true,
        "color_primary": [0.93, 0.51, 0.19],
        "color_secondary": [0.12, 0.65, 0.90],
        "color_active": [1.0, 0.65, 0.0]
      },
      "open_modulation": null
    },
    {
      "id": 2,
      "name": "OpenModulation",
      "active": true,
      "wavetable_path": null,
      "plugin_path": null,
      "plugin_format": null,
      "open_wavetable": null,
      "open_modulation": {
        "waveform": "SawDown",
        "sync_mode": "TempoSync",
        "rate_hz": 1.0,
        "sync_rate": "2/1",
        "depth": 0.16,
        "phase_deg": 0.0,
        "bipolar": true,
        "amount": 0.14,
        "target": "OpenWavetable: WT POS"
      }
    }
  ]
}
```

### 3.2 Ejemplo: Preset de Dispositivo Individual (`SingleDevice`)
```json
{
  "format_version": 1,
  "created_by": "Hikaru OpenLive 2.24.0",
  "preset_type": "SingleDevice",
  "name": "Sub Vocal Growl",
  "author": "Tomoyo Sakurai",
  "category": "Bass",
  "description": "Deep sub wavetable tuned down 3 octaves",
  "slot": {
    "id": 1,
    "name": "OpenWavetable",
    "active": true,
    "wavetable_path": "/../../../Xfer/Serum 2 Presets/Uber Growl.wav",
    "plugin_path": null,
    "plugin_format": null,
    "open_wavetable": {
      "table_name": "Uber Growl",
      "position": 0.640625,
      "morph": 0.0,
      "frame": 41,
      "detune": 0.0,
      "phase": 0.0,
      "unison": 1,
      "octave": -3,
      "pitch": 0,
      "voices": "Poly",
      "render_mode_3d": true,
      "render_mode_lines": true,
      "color_primary": [0.93, 0.51, 0.19],
      "color_secondary": [0.12, 0.65, 0.90],
      "color_active": [1.0, 0.65, 0.0]
    },
    "open_modulation": null
  },
  "chain": null
}
```

---

## 4. Definición de Campos

### 4.1 Metadatos Generales
| Campo | Tipo | Obligatorio | Descripción |
|---|---|---|---|
| `format_version` | `u32` | Sí | Versión del formato de archivo (actualmente `1`). |
| `created_by` | `String` | Sí | Versión de Hikaru que generó el archivo (ej. `"Hikaru OpenLive 2.24.0"`). |
| `preset_type` | `String` | Sí | `"SingleDevice"` o `"Chain"`. |
| `name` | `String` | Sí | Nombre representativo del preset. |
| `author` | `Option<String>` | No | Nombre del autor o diseñador de sonido. |
| `category` | `Option<String>` | No | Categoría musical (ej. `"Bass / Growl"`, `"Lead"`, `"Pad"`, `"Modulation"`). |
| `description` | `Option<String>` | No | Notas sobre el preset o instrucciones de uso. |

### 4.2 Parámetros de `OpenWavetableParams`
* `table_name`: Nombre visible de la tabla wavetable cargada.
* `position`: Posición normalizada `0.0 ..= 1.0` del cursor `WT POS`.
* `morph`: Valor fraccionario del morph entre ciclos adyacentes (`0.0 .. 1.0`).
* `frame`: Índice entero del ciclo seleccionado.
* `detune`: Cantidad de desafinación unísono (`0.0 ..= 1.0`).
* `phase`: Desfase inicial de fase del oscilador (`0.0 ..= 1.0`).
* `unison`: Número de voces unísono (`1 ..= 16`).
* `octave`: Transposición de octava (`-3 ..= +3`).
* `pitch`: Afinación fina / semitonos (`-24 ..= +24`).
* `voices`: Modo polifónico (`"Poly"`, `"Mono"`, `"Legato"`).
* `render_mode_3d`: `true` para vista tridimensional, `false` para modo 2D.
* `render_mode_lines`: `true` para renderizado en líneas (*Wireframe*), `false` para cintas (*Ribbon*).
* `color_primary`, `color_secondary`, `color_active`: Tríos RGB flotantes `[r, g, b]` en rango `0.0 ..= 1.0`.

### 4.3 Parámetros de `OpenModulationParams`
* `waveform`: Forma de onda del LFO (`"Sine"`, `"Triangle"`, `"SawUp"`, `"SawDown"`, `"Square"`, `"SampleAndHold"`).
* `sync_mode`: Modo de sincronización (`"TempoSync"` o `"FreeHz"`).
* `rate_hz`: Frecuencia en Hercios cuando está en modo `FreeHz` (`0.02 ..= 30.0`).
* `sync_rate`: División métrica musical en notación fraccional (ej. `"2/1"`, `"1/1"`, `"1/2"`, `"1/4"`, `"1/8"`, etc.).
* `depth`: Amplitud o profundidad del oscilador (`0.0 ..= 1.0`).
* `phase_deg`: Desplazamiento de fase en grados (`0.0 ..= 360.0`).
* `bipolar`: `true` para modulación simétrica `[-1.0, +1.0]`, `false` para unipolar `[0.0, 1.0]`.
* `amount`: Intensidad aplicada al destino (`-1.0 ..= +1.0`).
* `target`: Etiqueta del parámetro destino mapeado (ej. `"OpenWavetable: WT POS"`, `"OpenWavetable: DETUNE"`, etc.).

---

## 5. Integración con la Interfaz de Usuario (UI)

1. **Tarjeta de OpenWavetable:**
   * El nombre del wavetable en la barra superior actúa como selector de presets. Al hacer click, despliega:
     * `💾 Guardar Preset (*.hdp)`
     * `📂 Cargar Preset (*.hdp)`
     * `📁 Cargar WAV externo...`
2. **Tarjeta de OpenModulation:**
   * Botón `💾 HDP` dedicado en la barra de ruteo para guardar y cargar curvas LFO y asignaciones de knobs.
3. **Pista y DSP Rack:**
   * Menú contextual con click derecho en la cabecera vertical de la pista para guardar o cargar la cadena entera de la pista (`.hdp`).
   * Menú en el botón circular `+` de inserción de dispositivos con opciones para cargar o guardar cadenas completas.
4. **Navegador de Archivos (Explorer):**
   * Reconocimiento directo de archivos `.hdp` con insignia color cian de dispositivo.
   * Doble click para instanciar el preset o la cadena directamente en la pista seleccionada.

---

## 6. Arquitectura Legal e Interoperabilidad (LGPL-3.0-or-later)

El formato `.hdp`, al igual que `.oplf` y `.opsf`, vive en el crate **`hikaru_project_file`** bajo licencia **GNU LGPLv3**.
Esto garantiza que cualquier desarrollador de software de audio (abierto o propietario) pueda implementar compatibilidad con presets de Hikaru sin riesgo de violar licencias copyleft y permitiendo la libre distribución de soundbanks comerciales o comunitarios.
