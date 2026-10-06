#### 1. `/docs/hikaru-openlive-file/` (`.oplf`)

**Propósito:** Enfocado 100% en la matriz de clips, escenas y perfomance en vivo (estilo Clip Launcher / Session View).

* **Contenido exclusivo:**
* **Información del Engine / Versionado:** `"format_version": 1`, `"created_by": "Hikaru OpenLive 2.22.2"`.
* **Transporte:** BPM, métrica ($4/4$, etc.), división de tiempo y estado de transporte.
* **Matriz de Lanzamiento (Grid):** Escenas (filas), Tracks de Live (columnas) y la grilla de Clips (loops de audio, disparadores MIDI, colores, cuantización de disparo).
* **Tracks de Mezcla de Live:** `live_tracks` con su ruteo, volumen, pan, solo/mute/arm y cadenas de DSP Racks dedicadas al modo Live.
* **DSP Rack Global de Live:** Moduladores y efectos de master/envíos en tiempo real.


* **Qué NO incluye:** No contiene la línea de tiempo (*Playlist/Timeline*), ni regiones de arreglos, ni automatizaciones fijas del secuenciador.

---

#### 2. `/docs/hikaru-openstudio-file/` (`.opsf`)

**Propósito:** Enfocado 100% en la producción lineal, arreglos de canciones y secuenciación en timeline (estilo Playlist / Studio View).

* **Contenido exclusivo:**
* **Información del Engine / Versionado:** `"format_version": 1`, `"created_by": "Hikaru OpenStudio 2.22.2"`.
* **Transporte & Timeline:** PPQN (p. ej., $960$), marcas de compás, marcadores de tiempo/secciones, bucles de región.
* **Playlist / Arranger:**
* **Tracks de Studio:** Inserciones de clips de audio/MIDI alineados por *ticks* absolutos.
* **Clips lineales:** Eventos MIDI (notas, pitch bend, CC) y Clips de Audio (referencias a muestras, estiramiento de tiempo, marcadores de transientes).
* **Carriles de Automatización:** Curvas Bézier / puntos de automatización fijados a parámetros de DSP/sintetizadores en el tiempo.


* **Mezclador de Studio:** `studio_tracks` con arquitectura de buses, envíos auxiliares y FX Racks en serie/paralelo.


* **Qué NO incluye:** No contiene matrices de escenas, ni clips sin tiempo asignado para disparos al vuelo.

---

### Plan de Acción para la IA (marcarle la cancha a OpenCode)

1. **Creación de Specs en `/docs/`:**
* Crear `/docs/hikaru-openlive-file/SPECIFICATION.md` y `schema.json`.
* Crear `/docs/hikaru-openstudio-file/SPECIFICATION.md` y `schema.json`.


2. **Refactor de `src/project.rs` (o el módulo de serialización):**
* Eliminar la estructura única `Project` genérica.
* Separar en dos structs fuertemente tipadas en Rust: `OpenLiveProject` (`.oplf`) y `OpenStudioProject` (`.opsf`).
* Actualizar las constantes para que el header tome siempre `VERSION` desde el `Cargo.toml` / `about.rs` (`2.22.2`).


3. **Filtro de Diálogo de Archivo (`rfd`):**
* Si la interfaz está en **Modo OpenLive**, la opción de guardado debe ser exclusivamente **`Hikaru OpenLive File (*.oplf)`**.
* Si está en **Modo OpenStudio**, la opción de guardado debe ser exclusivamente **`Hikaru OpenStudio File (*.opsf)`**.
