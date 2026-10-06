# Especificación del Formato Hikaru OpenLive File (`.oplf`)

**Versión de especificación:** 1.0

**Versión del software:** 2.22.2

**Licencia de formato:** LGPL-3.0-only

**MIME Type:** `application/x-hikaru-openlive` / `application/json`

---

## 1. Filosofía y Propósito

El archivo `.oplf` es la estructura de datos orientada exclusivamente a la **ejecución, performance en vivo y lanzamiento de clips no lineales** (Matriz de Escenas/Clips).

* **Es un formato puramente no lineal:** Representa la grilla (*Session Matrix View*), el estado de los buses de mezcla en vivo (*Live*), los parámetros de cuantización de disparo (*launch quantize*) y el rack de efectos/moduladores asignados a la performance.
* **Separación estricta:** Un archivo `.oplf` **no debe contener** timelines de arreglos (*Playlist*), ni clips posicionados en tiempo absoluto (*PPQN/ticks* de canción).

---

## 2. Esquema del Archivo JSON (`.oplf`)

```json
/* 
==========================================================================================
  Hikaru OpenLive CLI | Hikaru Corporation (C) 2026 | GNU AGPLv3
  Hikaru OpenLive File Format (.oplf) | GNU Lesser General Public License v3.0 (LGPLv3)
==========================================================================================

  Hikaru OpenLive CLI Version: 2.22.2
  Hikaru OpenLive File Format Version: 1.0

  Shrine Host OS: "Windows 10 21H2 (x64)"
  Shrine Host Kernel Version: 10.0.19044
  Shrine Host Kernel Architecture: x86_64
  Shrine Host CPU: Intel(R) Core(TM) i3-2500K CPU @ 3.30GHz
  Shrine Host CPU Cores: 2 | Threads: 4
  Shrine Host GPU: Intel(R) UHD Graphics 630
  Shrine Host GPU Memory: 4 GB

  Hikaru VST3/CLAP Host Version: 2.22.2

  File Name: "Tomoyo Sakurai - Yakumo's Squizofrenia.oplf"
  Artist(s): "Tomoyo Sakurai"
  Genre: Future Bass / Melodic Dubstep / J-EDM
  BPM: 160
  Tonal: B Minor
  Time Signature: 4/4
  Launch Quantization: 1/1
  Tap Tempo Enabled: true
  Scenes Number: 160
  Tracks Number: 20

  VST3 Devices in Total Project: 15 
    [(Serum 1) : x10]
    [(OTT) : x15]
    [(Chroma) : x20]
    [(Vital) : x15]

  CLAP Devices in Total Project: 15
    [(LSP Compressor) : x10]
    [(LSP Reverb) : x15]
    [(LSP EQ) : x20]
    [(LSP Delay) : x15]

  Hikaru Native Plugins in Total Project: 15
    [(Hikaru OpenWavetable) : x10]
    [(Hikaru OpenDMS) : x45]
    [(Hikaru OpenModulation) : x15]

  Audio Clips in Total Project: 50
  MIDI Clips in Total Project: 15

  Tracks in Total Project: 20
   [Scene 1:
      Track 1: Drum Loop 160BPM,
      Track 2: Future Bass Drop Loop - 160BPM - B minor,
      Track 3: FX 160BPM,
      // Rest of tracks...
    ]

   [Scene 2:
      Track 1: Buildup Snare Roll Loop 160BPM,
      Track 2: FX 160BPM,
      // Rest of tracks...
    ]

   [Scene 3:
      Track 1: Dubstep Drum Loop 160BPM,
      Track 2: Dubstep Bass Drop Loop - 160BPM - B minor,
      Track 3: FX 160BPM,
      // Rest of tracks...
    ]
    // Rest of scenes...
*/

{
  "format_version": 1,
  "created_by": "Hikaru OpenLive 2.22.2",
  "engine_mode": "OpenLive",
  "metadata": {
    "title": "Yakumo's Squizofrenia",
    "artist": "Tomoyo Sakurai",
    "genre": "Future Bass / Melodic Dubstep / J-EDM",
    "key": "B Minor",
    "stats": {
      "total_scenes": 160,
      "total_tracks": 20,
      "audio_clips": 50,
      "midi_clips": 15
    }
  },
  "transport": {
    "bpm": 160.0,
    "time_signature": [4, 4],
    "launch_quantization": "1/1",
    "tap_tempo_enabled": true
  },
  "dsp_global": {
    "master_volume": 0.75,
    "master_pan": 0.0,
    "effects_rack": []
  },
  "live_tracks": [
    {
      "id": 1,
      "name": "Track 1",
      "volume": 0.75,
      "pan": 0.0,
      "mute": false,
      "solo": false,
      "arm": false,
      "color": "#FF5500",
      "sends": [],
      "effects": []
    },
    {
      "id": 2,
      "name": "Track 2",
      "volume": 0.80,
      "pan": 0.0,
      "mute": false,
      "solo": false,
      "arm": false,
      "color": "#00AEFF",
      "sends": [],
      "effects": []
    }
  ],
  "matrix": {
    "scenes": [
      { "id": 0, "name": "Scene 1", "color": null },
      { "id": 1, "name": "Scene 2", "color": null },
      { "id": 2, "name": "Scene 3", "color": null }
    ],
    "grid": [
      [
        {
          "id": 101,
          "name": "Drum Loop 160BPM",
          "sample_path": "/path/to/samples/drum_loop_160bpm.wav",
          "clip_type": "Audio",
          "loop_enabled": true,
          "loop_start_samples": 0,
          "loop_end_samples": 132300,
          "volume": 0.75,
          "pitch_shift": 0.0,
          "launch_mode": "Trigger"
        },
        {
          "id": 102,
          "name": "Future Bass Drop Loop - 160BPM - B minor",
          "sample_path": "/path/to/samples/fb_drop_loop_160bpm_bmin.wav",
          "clip_type": "Audio",
          "loop_enabled": true,
          "loop_start_samples": 0,
          "loop_end_samples": 264600,
          "volume": 0.80,
          "pitch_shift": 0.0,
          "launch_mode": "Trigger"
        }
      ]
    ]
  }
}

```

---

## 3. Reglas Estrictas para OpenCode (`STRUCT_RULES.md`)

1. **Versionado de Software Obligatorio:**
* El campo `"created_by"` **debe** resolverse dinámicamente desde la constante `VERSION` de Rust (`2.22.2`). Queda estrictamente prohibido usar hardcodeos obsoletos (como `0.1.0`).

2. **Prohibición de Contaminación de Datos Híbridos:**
* Los bloques `playlist`, `ppqn`, `studio_tracks` y `arranger_clips` quedan **completamente prohibidos** dentro de un archivo `.oplf`.

* Si el usuario guarda desde el **Modo OpenLive**, la app solo procesa estructuras pertenecientes a la matriz, transport global y `live_tracks`.

3. **Extensiones y Diálogos de Guardado:**
* La extensión por defecto es únicamente `.oplf`.
* El filtro del modal `rfd` debe declararse exactamente como:
`"Hikaru OpenLive Project (*.oplf)" -> ["oplf"]`.

---

## 4. Licenciamiento y Arquitectura Legal (¿Por qué LGPLv3?)

Mientras que el ejecutable principal y el núcleo del DAW **Hikaru OpenLive** están licenciados bajo **GNU AGPLv3**, la especificación del formato `.oplf` y su crate de serialización están licenciados bajo **GNU LGPLv3 (GNU Lesser General Public License v3.0)**.

### Justificación de Interoperabilidad
1. **Aislamiento de Licencia para Contenido y Presets Propietarios:**
   Bajo **AGPLv3**, cualquier formato de archivo atado fuertemente al ejecutable podría generar fricción legal con presets, estados de proyectos o bancos de sonidos generados por software de producción musical propietario legacy (por ejemplo, instancias de *Serum 1/2*, *VSTs propietarios* o código de Windows). La **LGPLv3** actúa como un puente neutral (*boundary crate*): permite que productores y terceros consuman, guarden y ejecuten estados de proyectos `.oplf` sin que sus proyectos artísticos o plugins propietarios se vean forzados bajo los términos copyleft agresivos de la AGPLv3.

2. **Estrategia de Sub-Crates bajo LGPLv3 dentro de Hikaru Workspace:**
   Para mantener este diseño de fronteras bien definido, los siguientes componentes del proyecto utilizan intencionalmente la licencia **LGPLv3**:
   * **`hikaru_openlive_file` (`.oplf`) / `hikaru_openstudio_file` (`.opsf`):** Permiten que cualquier DAW externo, conversor o herramienta de terceros lea/escriba proyectos nativos de Hikaru libremente siempre y cuando se aporte código bajo la LGPLv3.
   * **`hikaru_plugin_host`:** Encargado de cargar, aislar y ejecutar binarios de plugins propietarios y VST3/CLAP legacy mediante separación de procesos/ABI sin violar la AGPLv3 del motor principal.
   * **`hikaru_audio_drivers` (`HikaruNative` para Linux / `Hikaru ASIO` para Windows):** Diseñados bajo LGPLv3 para que puedan ser enlazados o reutilizados como un driver/backend de bajo nivel por otros DAWs o software de audio (como REAPER, FL Studio, etc.) sin forzarlos a abrir su código fuente.

---

## 4.1. Como utilizar este formato dentro de otros DAWs de **código cerrado**?

1. Sí sos un desarrollador de un Digital Audio Workstation (DAW) de **código abierto**, y quieres utilizar este formato dentro de tu propio DAW lo podés hacer **siempre y cuando** se **aporte código** bajo la **LGPLv3.**

2. Sí sos un desarrollador de un DAW de **código cerrado** (por ejemplo; sí laburás en **Image-Line**, **Ableton** o **Bitwig**) y querés implementar compatibilidad con **`.oplf`**, tenés que aportar código bajo la LGPLv3 para que tu DAW pueda utilizar este formato sin necesidad de abrir todo el código fuente de tu propio DAW propietario. Unicamente tenés que aportar el código modificado del formato **`Hikaru OpenLive File`** para que tu DAW propietario pueda utilizar este formato.

3. Para los desarrolladores de DAWs de código cerrado, no se puede cerrar los cambios al formato `.oplf` sin infringir la GNU Lesser General Public License v3.0. Por lo tanto, sí querés utilizar este formato, tenés que sí o sí aportar tus cambios bajo la misma licencia. No hace falta que tu DAW sea de código abierto para que puedas utilizar este formato.

---