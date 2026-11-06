# Clean-Room UI Specification: Detail Editor Panel (Bitwig Studio 5.1.7 Parity)

## 0. Referencias de Observación (Black-Box)
- **Fuentes:**
  - `bitwig-clip-editor-1.webm` (Frames 00 a 45 a 1366x768).
  - `bitwig-detail-editor-panel-2.mp4` (76 keyframes extraídos a 1366x768 cubriendo selección de compases, automatización de pitch y zoom).
  - Grabación de pantalla del usuario `Videocaptura de pantalla_20261105_202158.webm`.
- **Método:** Clean-room reverse engineering sin descompilación ni copia de código propietario.

## 1. Arquitectura de Layout y Jerarquía Visual

### 1.1 Altura y Splitter
- **Altura estándar observada:** ~195px - 220px.
- **Splitter superior:** borde gris grafito sutil (`#282828`) con cursor de resize vertical.

### 1.2 Región Izquierda: Columna Vertical de Pista (Vertical Track Strip)
- **Ancho:** 24px exactos.
- **Borde izquierdo:** Línea vertical acentuada en color anaranjado de pista (`#FF7A00`) de 3px de grosor.
- **Iconos/Etiquetas:**
  - Parte superior: Icono triangular de play/clip launcher (`▶`).
  - Centro/cuerpo: Nombre del clip en orientación vertical (primeras letras mayúsculas `#8A8A96`).
  - Parte inferior: Icono de clip/diamante (`❖`) y chevron (`◀` / `▶`) indicador de colapso/expansión del inspector.
- **Color de fondo:** `#1F2023`.

### 1.3 Panel de Inspección del Clip (Clip Inspector Card)
- **Ancho:** 115px (cuando está visible).
- **Color de fondo:** `#28292E` con borde derecho sutil `#18191C`.
- **Estructura vertical:**
  1. **Fila 1 (Superior, alineada con la regla de tiempo y loop):**
     - Botón selector de herramienta (Tool Selector): Icono de cursor blanco (`▲`) + chevron dropdown (`▾`), fondo `#35363D`, borde `#2A2B30`, altura 26px.
  2. **Fila 2 (Botón de Modo Principal):**
     - Botón grande de 46px de altura: Icono superior (onda de audio `〰`, tenedor de afinación `Ψ`, tijera `✂`, reloj `⏱` o transiente `⚡`) + texto inferior (`Audio Events`, `Pitch`, `Comping`, `Stretch`, `Onsets`) + chevron dropdown `▾` a la derecha. Fondo `#33343A` con borde `#28292E` y hover sutil `#3D3E45`.
  3. **Fila 3 (Casilla Auto-Fades):**
     - Toggle redondo anaranjado (`#FF7A00`) con tilde de verificación blanca (`✓`) y texto plateado `Create auto-fades`.
  4. **Fondo inferior:**
     - Superficie limpia y despejada en color grafito `#28292E` (sin faders ni controles desalineados).

### 1.4 Región Central/Derecha: Línea de Tiempo y Waveform Canvas
- **Estructura vertical:**
  1. **Regla de Compases (Bar Ruler) [0..22px]:**
     - Fondo grafito oscuro (`#323232`).
     - Números de compás limpios (`1`, `2`, `3`, `4`...) en `#F0F0F5`. Las subdivisiones de beat solo aparecen si el zoom supera los 140px/s para evitar saturación visual.
  2. **Carril de Región de Loop (Loop Region Lane) [22..36px]:**
     - Fondo `#353535`.
     - Barra de loop activa con resalte ámbar (`#FF7A0022`), líneas superior e inferior naranja (`#FF7A00`) y marcadores triangulares en el inicio (`▶`) y fin (`◀`).
  3. **Banner Superior del Clip [36..54px]:**
     - Línea naranja superior de 2.5px (`#FF7A00`).
     - Fondo gris pizarra (`#32343A`).
     - Nombre del clip / sample y metadata (`BPM`, duración).
  4. **Cuerpo del Waveform [54..cierre]:**
     - **Fondo de la tarjeta del clip:** Gris plateado satinado (Silver `#C0C4D0`).
     - **Fondo de la rejilla fuera del clip:** Gris grafito pizarra oscuro (`#28292E`), idéntico al tema de Bitwig, con líneas verticales de compás y beat sutiles (`#202125` / `#35363D`).
     - **Forma de onda:** Grafito/Charcoal (`#282828`).
     - **Línea central:** Eje de cruce por cero de 1px (`#787C8888`).
     - **Banderas de Onset / Transientes:** Triangulitos lavanda/periwinkle (`#7B8CDE`) en la parte superior.
     - **Línea de Ganancia:** Trazo horizontal interactivo anaranjado (`#E8760C`).
     - **Playhead:** Línea vertical blanca continua de 1.5px.
  5. **Barra de Desplazamiento Inferior (Timeline Scrollbar Strip) [altura 20px]:**
     - Fondo `#1F2023`, borde superior `#161719`.
     - Botón de flecha izquierda `◀` (16x16px).
     - Carril de scroll continuo (`#141416`, 8px altura) con thumb deslizable interactivo (`#45464D`, 6px) proporcional a la ventana visible.
     - Botón de flecha derecha `▶` (16x16px).
     - Extremo derecho (exclusivo de transporte/zoom, sin botones de modo redundantes): botón `Loop` (`#FF7A00` activo), selector de Snap compacto (`1/4 ▴`), botones Zoom `-` / `+`, y botón de Fit to Viewport (`⇥⇤`). Los modos de edición residen exclusivamente en el panel Inspector izquierdo.

## 2. Comportamiento Interactivo y Context Cues (Footer Bar)
- Al posicionar el cursor sobre el canvas del Waveform, la barra inferior (Footer) muestra en tiempo real las leyendas exactas de Bitwig:
  - En modo Audio Events: `DRAG Select time    ALT+DRAG Slide Content    MIDDLE-DRAG Zoom & pan`
  - En modo Pitch: `DRAG Rectangular selection    CTRL+ALT+DRAG Pan    DOUBLE-CLICK Insert event`
  - Al posicionar sobre marcadores de loop: `DRAG Move loop    SHIFT Disable snapping`
