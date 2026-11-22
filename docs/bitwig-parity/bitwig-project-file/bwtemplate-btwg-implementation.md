# Implementación nativa de `.bwtemplate` y formato binario `BtWg` (Bitwig Studio 5.1.7)

Este documento describe la ingeniería inversa (realizada mediante **REA — Reverse Engineer Anything** sobre plantillas oficiales de Bitwig Studio como `Performance_Set.bwtemplate`, `Play_Drums.bwtemplate` y `Spectral Suite.bwtemplate`) y la implementación en Rust que permite a **Hikaru OpenLive** leer y exportar archivos `.bwtemplate` **100% compatibles con Bitwig Studio 5.1.7**.

---

## 1. Contenedor externo `.bwtemplate` (ZIP)

Un archivo `.bwtemplate` nativo de Bitwig Studio es un **archivo ZIP estándar** (`PK\x03\x04`) con la siguiente estructura interna:

```text
mi_plantilla.bwtemplate (ZIP)
├── template-info.json          # Metadatos en formato HOCON
├── view-settings               # Estado de ventana/UI en formato binario BtWg (cls 0x987)
├── template.bwproject          # Proyecto binario nativo BtWg0003000200b8 (sin compresión interna)
├── project/
│   └── samples/
│       └── *.wav               # Samples de audio embebidos referenciados por el proyecto
└── hikaru-session.bwproject    # (Opcional, solo Hikaru) Copia TLV sin pérdida para re-apertura en Hikaru
```

- **Bitwig Studio 5.1.7** lee `template-info.json`, `view-settings`, `template.bwproject` y `project/samples/*`, ignorando entradas adicionales como `hikaru-session.bwproject`.
- **Hikaru OpenLive** detecta si el archivo es ZIP (`PK\x03\x04`), extrae `project/samples/*` a un directorio caché temporal y prioriza `hikaru-session.bwproject` (fidelidad total de loop en frames, legato, etc.) o, en su defecto, parsea `template.bwproject` (`BtWg`) para abrir plantillas nativas de Bitwig.

---

## 2. Formato binario `BtWg` (Wire Format)

El archivo `template.bwproject` **no está comprimido** internamente y usa big-endian (`BE`) para todos los enteros y flotantes.

### 2.1 Cabecera ASCII de 42 bytes

```text
BtWg0003000200b800001d62000000000000000000
│   │   │   │   │       │       │       └─ 2 bytes: "00" (tiene sección meta) / "01" (sin meta, seguido de '\n')
│   │   │   │   │       │       └───────── 8 hex: reservado (00000000)
│   │   │   │   │       └───────────────── 8 hex: reservado (00000000)
│   │   │   │   └───────────────────────── 8 hex: body_offset (inicio del árbol binario 0x2e)
│   │   │   └───────────────────────────── 4 hex: revisión de esquema (0x00b8 = 184, Bitwig 5.0/5.1.7)
│   │   └───────────────────────────────── 4 hex: tipo (0002)
│   └───────────────────────────────────── 4 hex: versión de contenedor (0003)
└───────────────────────────────────────── 4 bytes: magic "BtWg"
```

### 2.2 Sección `meta` y espacio reservado

Inmediatamente en el byte `42` (`0x2a`) comienza un objeto con nombre (`cls = 4`, `"meta"`) que contiene 20 campos estándar (`application_version_name = "5.0"`, `revision_no = 132920`, `type = "application/bitwig-project"`, `used_files`, y `structure`, un sub-documento `BtWg` con el árbol de nombres y tipos de pista).

> **Regla crítica de validación en Bitwig 5.1.7:**  
> Entre el fin del objeto `meta` y `body_offset`, el espacio reservado debe rellenarse exclusivamente con espacios ASCII (`0x20`), **y el último byte (`body_offset - 1`) debe ser obligatoriamente `\n` (`0x0a`)**. De lo contrario Bitwig rechaza el archivo con:
> `Invalid byte used for reserved space in meta data`.

### 2.3 Tags de tipo del árbol de objetos

Cada objeto numérico comienza con `cls: u32 BE`, seguido de pares `(field_id: u32 BE, value_type: u8, payload)` y termina con `0x00000000`.

| Tag | Tipo | Codificación |
|-----|------|--------------|
| `0x01` | `i8` / `u8` / enum | 1 byte |
| `0x02` | `i16` | 2 bytes BE |
| `0x03` | `i32` | 4 bytes BE |
| `0x04` | `i64` / `u64` | 8 bytes BE |
| `0x05` | `bool` | 1 byte (`0` / `1`) |
| `0x06` | `f32` | 4 bytes IEEE-754 BE |
| `0x07` | `f64` | 8 bytes IEEE-754 BE |
| `0x08` | `string` | `len: u32 BE` + UTF-8 (si bit 31 = 1, UTF-16BE) |
| `0x09` | `object` | Objeto inline (`cls` + campos + `0`) |
| `0x0a` | `null` | 0 bytes |
| `0x0b` | `ref` | `obj_id: u32 BE` (referencia 1-based) |
| `0x0d` | `bytes` | `len: u32 BE` + bytes crudos |
| `0x12` | `obj_list` | Secuencia de objetos terminada en `u32 BE == 3` (`0` = elemento `null`) |
| `0x15` | `uuid` | 16 bytes |
| `0x16` | `color` | `4 × f32 BE` (RGBA en `0.0..=1.0`) |
| `0x19` | `str_arr` | `count: u32 BE` + `count` strings |
| `0x1a` | `obj_key` | `(object, string)` — referencia a puerto/parámetro de un objeto |

### 2.4 Grafo de referencias 1-based (DFS pre-orden)

Bitwig serializa grafos de objetos con ciclos/compartición asignando un ID secuencial **1-based** en orden **DFS pre-orden** a cada objeto inline (`cls >= 5`):
- La **primera vez** que se serializa un objeto (por ejemplo, la pista `Master` en el ruteo de salida `0x897` de la pista 1, o el parámetro `VOLUME` `0x4c4` dentro de `0x168`), se escribe inline y recibe su `obj_id`.
- Las **referencias posteriores** al mismo objeto usan `0x0b` + `obj_id` (en campos) o `cls = 1` (`REF1`) + `obj_id` (en listas `0x12` y pares `0x1a`).

---

## 3. Esquema `float_core` mapeado (Bitwig 5.1.7)

### 3.1 Proyecto raíz (`cls = 0x2e` — `float_core.float_document`)

- `0xbd9` (`0xd7`): **Transporte**
  - `0x211` (`0x4c1`): `TEMPO` (`0x2c8: f64` = BPM)
  - `0x212` (`0x7f4`): Compás empaquetado (`4/4` = `0x10000024` = `268435492`)
  - `0x213` (`0x6b`): Groove (`GROOVE_ENABLED`, `SHUFFLE_*`, `ACCENT_*`)
  - `0x214` (`0xfb`): Afinación (`A4 = 440.0 Hz`)
- `0x191` (`0xb3`): Metrónomo
- `0xd3c` (`0x2c1`): **Sample Pool** (`0x4e: [0x4a6]`)
  - Cada `0x4a6` describe un archivo embebido:
    - `0x512`: nombre de archivo (`"sample.wav"`)
    - `0xd3a`: ruta relativa dentro de `project/` (`"samples/sample.wav"`)
    - `0x513` (`0x1e2`): hash de 16 bytes (`0x16: bytes`)
    - `0x17`: tamaño en bytes (`i32`)
    - `0x129c`: lista con `0x4a9` (**AudioStream**): `0x129e: ref(0x4a6)`, `0x94: sample_rate`, `0x95: duration_secs`, `0x96: channels`.
- `0x4dd` (`0x1dd`): **Grupo raíz de pistas (`"Project"`)**
  - `0x4de`: lista de pistas regulares (`0x90`)
  - `0x4df`: lista de pistas FX (`0x90`)
  - `0x4e0`: `ref(master_id)`
  - `0x4e2` (`0x10d`): **Banco de Escenas** (`0x293: [0x6d]`)
- `0x4d` (`0xe4`): **`float_document_display_settings`**
  - `0xee6` (`0x325`): `track_selection` (referencia a `0x90`)
  - `0x138` (`0x1b5`): `slot_selection` (debe referenciar `0xe3` o estar vacío `null` + `[]`)
  - `0x737` (`0x208`): Arranger editor (`tool_settings` `0x768`, campo `0x1a61`)
  - `0xf02`, `0xf04`, `0xf06`: Note editors (`tool_settings` `0x769`, campo **`0x1a62`**)
  - `0xf03`, `0xf05`, `0xf07`: Audio/Automation editors (`tool_settings` `0x766`, campo **`0x1a59`**)

### 3.2 Pistas (`cls = 0x90`)

| Campo | Significado |
|-------|-------------|
| `0x15b` | Nombre de la pista |
| `0x54c` | Color RGBA (`4 × f32`) |
| `0x927` | Tipo de pista: `0` = Instrumento/MIDI, **`1` = Audio**, `2` = Master |
| `0x15e` (`0x1e`) | **Clip Launcher** (`0x231: [0xe3]`, donde cada slot `0xe3` tiene `0x240 = null | clip`) |
| `0x164` (`0x8a`) | Cadena de dispositivos (`0x144`) |
| `0x165` (`0xab`) | Canal de mixer (`0x1a4: ref(VOLUME)`, `0x1a5: PANORAMA`, `0x1a6: MUTE`, `0x1a8: solo`, `0xa5: sends`) |

En el sub-documento `meta.structure`, las pistas usan `0x287` para **Audio** y `0x288` para **Instrumento**.

### 3.3 Audio Clips del Clip Launcher (`cls = 0xee`)

A diferencia de los clips MIDI (`0x47` → contenido `0xbf`), los **Audio Clips** usan:
- **`0xee`** (`clip_event` de audio):
  - `0x286`: nombre del clip
  - `0x280`: color RGBA
  - `0x26`: duración en beats
  - `0xbe1`: `launch_legato` (`bool`)
  - `0x2845`: `launch_mode` = **`255`** (valor por defecto en Bitwig 5.0/5.1.7; el valor `0` de Bitwig 4.4 es rechazado por el esquema `00b8`)
  - `0x288` (**`0x105`** — contenido de Audio Clip):
    - `0x98f` (`0x267`): loop start/end (`0x58`, `0x269`, `0x268`)
    - `0x49c` (`0x6ce` → `0x18cb: [0xce]` → `0x21f: [0xd4]`): **Audio Event (`0xd4`)**
      - `0x288` (`0x9f`): reproducción de sample
        - `0x20d` (`0x1db`): `0x4b4: ref(stream_0x4a9_id)` (enlaza con el Sample Pool `0xd3c`)
        - `0x192` (`0x8b`): marcadores de warp `0x43` (`(0.0 beats → 0.0 s)` y `(dur_beats → dur_secs)`)
        - `0x10ea` (`0x36b`): onsets `0x10e9: 0xb6`
      - `0xc05`: duración en segundos del audio

---

## 4. Arquitectura en Hikaru OpenLive

| Archivo | Responsabilidad |
|---------|-----------------|
| [btwg.rs](../../../crates/hikaru_gui/src/project/btwg.rs) | `BtWgWriter`, parser de cabecera WAV (`parse_wav_metadata`), `encode_state_to_btwg`, `encode_minimal_view_settings`, `encode_template_info` |
| [bwtemplate.rs](../../../crates/hikaru_gui/src/project/bwtemplate.rs) | Empaquetado ZIP (`export_to_path`) con samples embebidos en `project/samples/`, carga de plantillas ZIP nativas y flujo `new_from_template` |
| [bwproject.rs](../../../crates/hikaru_gui/src/project/bwproject.rs) | Parser `BtWg` (`parse_native_bitwig_btwg`), carga de audio clips en Launcher/Arranger (`decode_audio_file`, `load_peaks_from_wav`, `GuiCommand::LoadClip`, `loop_end` en frames y `launch_legato`) |
| [dawproject.rs](../../../crates/hikaru_gui/src/project/dawproject.rs) | Utilidades ZIP (`read_zip_entries` y `build_zip_store`) compartidas como `pub(crate)` |
| [file_menu.rs](../../../crates/hikaru_gui/src/ui/file_menu.rs) | Acciones **New from Template (.bwtemplate)...**, **Save as Template** y **Export Bitwig Template (.bwtemplate)...** |

---

## 5. Verificación

- **Tests automáticos** (`cargo test -p hikaru_gui bwtemplate && cargo test -p hikaru_gui bwproject`):
  - `export_to_path_writes_bitwig_zip_with_valid_btwg_and_embedded_samples`
  - `loads_native_bitwig_zip_bwtemplate_with_btwg_tracks_bpm_and_clips`
  - `bwtemplate_round_trip_preserves_launcher_audio_clips_and_notifies_engine`
  - `new_from_template_loads_state_without_setting_project_path`
  - `template_ensures_default_tracks_sends_and_channel_strip_bounds`
- **Prueba real en Bitwig Studio 5.1.7**:
  - Un `.bwtemplate` creado y exportado 100% desde Hikaru OpenLive (150 BPM, 4/4, 2 pistas de Audio, 8 escenas y 8 loops `.wav` de Cymatics en el Clip Launcher) abrió sin errores en Bitwig Studio 5.1.7 con sus pistas de Audio, formas de onda completas y reproducción activa.
