# Documentación para LLMs / OpenCode: Referencias Visuales en ASCII

> **Nota para OpenCode / LLMs de generación de código:**
> Para implementar la UI en GPUI / Rust, **NO ignores la estructura de la Playlist/Timeline**. Basate estrictamente en el siguiente esquema ASCII. Los modelos de lenguaje interpretan mejor la jerarquía, dimensiones y distribución espacial mediante caracteres de texto alineados que mediante imágenes o capturas de pantalla.

---

## Tip de Arquitectura: ASCII vs Imágen

Cuando solicites cambios o refactorizaciones en la interfaz a modelos de código:

1. **Evita enviar únicamente Screenshots:** Las redes de visión pierden precisión en coordenadas de layout y jerarquías estrictas.
2. **Usa diagramas ASCII explícitos:** Un diagrama de cajas define bordes, alineaciones, nombres de contenedores GPUI y relaciones jerárquicas exactas.

---

## 📐 Referencia Visual ASCII: Modo OpenStudio (Playlist / Timeline)

```
+-----------------------------------------------------------------------------------------------------------------------+
| TOP BAR / TOOLBAR: [PLAY] [STOP] [REC] [LOOP] | 00:00:00:000 | OPENLIVE [OPENSTUDIO] | BPM: 140 | 4/4 | [ARRANGER]... |
+-----------------------------------------------------------------------------------------------------------------------+
| TRACK HEADERS (Izquierda)      | Playlist / Timeline TRACKS (Derecha)                                                 |
|                                | 1        2        3        4        5        6        7        8        9       10   |
+--------------------------------+--------------------------------------------------------------------------------------+
| [T1] Synth Lead   [S][M][R] v  | [======= Audio/Midi Clip 01 =======]         | [=== Clip 02 ===]                     |
|    - Vol: |===---]  Pan: C     |                                             |                                        |
+--------------------------------+--------------------------------------------------------------------------------------+
| [T2] Bass Synth   [S][M][R] v  |                   | [============== Bass Pattern 01 ==============]                  |
|    - Vol: |====--]  Pan: L10   |                   |                                                                  |
+--------------------------------+--------------------------------------------------------------------------------------+
| [T3] Drums (Bus)  [S][M][R] v  | [== Kick Clip ==] [== Kick Clip ==] [== Kick Clip ==] [== Kick Clip ==]              |
|    - Vol: |=====--] Pan: C     |                                                                                      |
+--------------------------------+--------------------------------------------------------------------------------------+
| [T4] FX / Reverb  [S][M][R] v  |                               | [~~~~~~~~ Riser FX ~~~~~~~~]                         |
|    - Vol: |==----]  Pan: R25   |                               |                                                      |
+--------------------------------+--------------------------------------------------------------------------------------+
| + Add Track                    | (Área vacía de la Playlist / Clic para agregar clips o selecciones de tiempo)        |
|                                |                                                                                      |
+-----------------------------------------------------------------------------------------------------------------------+
| BOTTOM PANEL / DOCK: [ CLIP EDITOR ] [ PIANO ROLL ] [ DSP RACK ] [ EXPLORER ]                                         |
+-----------------------------------------------------------------------------------------------------------------------+

```

---

## 🛠️ Desglose de Componentes para GPUI (Rust)

1. **`ArrangerView` (Contenedor Principal):**
* Divide el área central en un Split Layout Horizontal: **TrackHeaders** (Panel izquierdo, ancho fijo ~200px-250px) y **PlaylistTimeline** (Panel derecho, scrollable horizontalmente).


2. **`TrackHeader` (Panel Izquierdo):**
* Nombre del Track, indicador de color.
* Botones de estado: `Solo (S)`, `Mute (M)`, `Record Arm (R)`.
* Faders / Knobs compactos para `Volume` y `Pan`.
* Desplegable (`v`) para automatizaciones.


3. **`PlaylistTimeline` (Panel Derecho):**
* **Ruler / Timebar (Arriba):** Muestra los compases/tiempos (1, 2, 3, 4...).
* **Track Lanes:** Filas horizontales alineadas 1 a 1 con cada `TrackHeader`.
* **Clips:** Bloques absolutos o relativos posicionados por tiempo de inicio (`start_time`) y duración (`duration`). Contienen waveform o representación MIDI.


4. **`Playhead` (Línea de Reproducción):**
* Overlay vertical que cruza todas las pistas según la posición del transporte actual.