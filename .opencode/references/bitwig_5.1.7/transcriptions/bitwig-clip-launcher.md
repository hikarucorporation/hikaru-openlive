# Referencia de interfaz: Clip Launcher (Bitwig Studio)

> **Propósito:** Documento de referencia visual y funcional para el rediseño de **Hikaru OpenLive** en **Rust + GPUI**. Describe la disposición, los componentes y el comportamiento observables en la captura, para que herramientas de desarrollo asistido (Antigravity, OpenCode) puedan replicar el *patrón de interacción* sin copiar código, iconos ni recursos del software propietario.
>
> **Nota de licencia:** Esta captura pertenece a Bitwig GmbH y se usa solo como referencia de usabilidad. No copiar íconos, tipografías, paleta exacta ni nombres de marca (Bitwig, nombres de dispositivos propietarios) en el código de Hikaru OpenLive. Los conceptos (clip launcher, escenas, pistas, navegador) son funcionales y libres de implementar.

---

## 1. Resumen general

La captura muestra la vista **ARRANGE** de un DAW con el **Clip Launcher** activo: una grilla de clips organizada en **pistas (filas)** y **escenas (columnas)**, junto a un **navegador de dispositivos/archivos** en el panel derecho. La estética es oscura, de alto contraste en acentos naranjas y azules, con controles compactos.

Tema general: **oscuro**.

---

## 2. Estructura de la pantalla

La interfaz se divide en cinco zonas principales:

| Zona | Ubicación | Función |
|---|---|---|
| Barra superior (Top bar) | Franja superior | Menú de archivo, transporte, tempo, compás, posición, acciones de edición |
| Área de Clip Launcher | Centro, grilla principal | Clips organizados por pista y escena |
| Panel de navegador | Derecha | Búsqueda y explorador de dispositivos, plugins y archivos |
| Barra de estado | Inferior | Pestañas de vista, herramientas de edición y pista de ayuda contextual |
| Pistas de sistema | Parte inferior de la grilla | Pistas FX y Master, fijas al pie |

---

## 3. Barra superior (Top bar)

### 3.1 Identidad y archivo
- **Etiqueta "DEMO"** (esquina superior izquierda): indica versión de demostración.
- **Pestaña de proyecto** "New 1" con botón de cierre (×).
- **Botón FILE**: menú de archivo (abrir, guardar, exportar).
- **Logotipo de cuadrícula de puntos** (centro superior): marca de la aplicación.
- **Controles de ventana** (derecha): minimizar, maximizar, cerrar.

### 3.2 Transporte
- **PLAY**: pestaña/botón de reproducción de la línea de tiempo.
- **▶ (Play)**, **■ (Stop)**, **● (Record)**: controles de transporte clásicos.
- **Botón de grabación de clip / modo de captura** (ícono de flecha curva).
- **Botón de metrónomo / pre-conteo** (ícono de cuadrado con marca).
- **Botón de modo de edición de automatización** (ícono de forma de onda, ~esquina inferior).

### 3.3 Pantalla de tempo y compás
- **Tempo:** `110.00` (BPM), en color azul sobre fondo oscuro.
- **Compás:** `4/4`.
- **Posición:** `1.1.1.00` (compás.tiempo.subdivisión.tick) y `0:00.000` (tiempo absoluto mm:ss.ms).
- Pequeños controles adyacentes: **tap tempo**, **loop** (ícono de cadena), **quantize/alineación**.

### 3.4 Acciones
- **ADD** (con menú desplegable): agregar pistas, dispositivos o escenas.
- **EDIT** (con menú desplegable): edición general.
- **Deshacer / Rehacer** (flechas curvas, deshabilitadas en la captura).
- **Duplicar** y **Cancelar/Detener** (ícono ⊗).

### 3.5 Vistas
- Botones de vista en la barra inferior: **ARRANGE** (activo, naranja), **MIX**, **EDIT**.

---

## 4. Área del Clip Launcher (grilla central)

### 4.1 Encabezado de escenas
- Fila superior con **8 escenas visibles**: `Scene 1` a `Scene 8`.
- Cada escena tiene un **botón de disparo** (triángulo ▶ a la izquierda del nombre) que lanza todos los clips de esa fila.
- Botón **"+"** al final para añadir una escena nueva.
- Columna de control a la izquierda: selector de modo de puntero (flecha con menú) y botón de encabezado de escenas.

### 4.2 Encabezado de pistas (columna izquierda)
Cada pista tiene un bloque de control de ~40 px de alto con:
- **Franja de color lateral** (izquierda): identifica el color de la pista.
- **Nombre de pista** (`Inst 1`, `Audio 2`, `FX 1`, `Master`).
- **Botón de armado/grabación** (●): gris si no está armada.
- **S** (Solo) y **M** (Mute): botones cuadrados.
- **Fader de volumen horizontal** con relleno naranja y marcador de nivel.
- **Ícono de tipo de pista** (teclado para instrumento, onda para audio, flecha para FX, corona/casa para Master).
- **Menú de pista** (≡) a la derecha.
- **Medidor de nivel vertical** (barra estrecha a la derecha del bloque).

Pistas listadas en la captura:
1. **Inst 1**: instrumento MIDI (franja naranja).
2. **Audio 2**: pista de audio (franja amarilla).
3. **FX 1**: pista de efectos, en la sección inferior fija.
4. **Master**: salida principal, en la sección inferior fija.

Botón **"+"** bajo las pistas para agregar una nueva.

### 4.3 Celdas de clip
- Grilla de celdas de **~85 × 43 px** por intersección pista/escena.
- Cada celda vacía muestra un **punto/cuadrado pequeño gris** centrado: indica "vacía, se puede soltar un clip aquí".
- El fondo de celda es ligeramente más claro que el fondo general.
- Las celdas de la fila de cada pista tienen un **controlador de arrastre** (≡ a la derecha, en la columna de cierre).

Comportamiento esperado (no visible en captura, inferido del patrón):
- Clic en celda vacía: crear clip.
- Clic en triángulo de escena: disparar fila.
- Clic en celda con clip: reproducir/parar clip (launch/stop).
- Estados visuales: **vacío**, **lleno (en espera)**, **reproduciendo**, **detenido**.

---

## 5. Panel de navegador (derecha)

### 5.1 Barra de pestañas de categorías
Íconos en la parte superior del panel, de izquierda a derecha:
1. Cuadrícula (vista general).
2. Estrella/filtros (activo, resaltado).
3. Vista de panel lateral.
4. Dispositivos / instrumentos.
5. Muestras o archivos.
6. Usuario/presets.
7. Pantalla/salida.

### 5.2 Encabezado
- Título **"Everything"** (todo) con **ícono de estrella** (favoritos) a la derecha.
- **Campo de búsqueda** con lupa, texto de placeholder y botón de borrar (×).

### 5.3 Filtros
Cuatro pares de filtros en formato de lista con ícono de asterisco:
- **Location** (ubicación)
- **Category** (categoría)
- **Device** (dispositivo)
- **File Kind** (tipo de archivo)
- **Creator** (creador)
- **Tags** (etiquetas)

### 5.4 Lista de resultados
- Lista desplazable con **ícono de tipo** a la izquierda (cuadrado naranja para dispositivos nativos, cuadrado azul para procesadores de señal, gris para modulación) y **nombre** a la derecha.
- Ejemplos de la lista (solo como referencia de estructura): dispositivos de sonido, dinámica, modulación, efectos de tiempo, utilidades.

---

## 6. Barra de estado inferior

- **Ícono de información** (i) a la izquierda.
- **Pestañas de vista**: `ARRANGE` (activo, naranja), `MIX`, `EDIT`.
- **Iconos de herramientas**: puntero, curva/automatización, bucle, mezcla, selección.
- **Mensaje contextual**: `DOUBLE-CLICK  Insert track from device` (doble clic: insertar pista desde dispositivo). Esto indica que la barra muestra ayuda según el elemento bajo el cursor.
- **Iconos de la derecha**: búsqueda, archivo, notificaciones, mano (modo de desplazamiento), piano/teclado.
- **Barra de desplazamiento horizontal** en la parte inferior de la grilla, con flechas ◀ ▶.
- **Pistas de la fila inferior**: barra de herramientas pequeña con iconos de vista de pistas (cruz/X para cerrar).

---

## 7. Paleta de colores aproximada

> Valores aproximados extraídos visualmente de la captura. Ajustar al diseño propio de Hikaru.

| Elemento | Color aprox. |
|---|---|
| Fondo principal | `#2b2b2b` |
| Fondo de grilla | `#1e1e1e` |
| Fondo de celdas | `#262626` |
| Texto principal | `#e0e0e0` |
| Texto secundario | `#8a8a8a` |
| Acento primario (naranja) | `#e8782a` |
| Acento de pista de audio | `#e8c52a` |
| Acento de tempo / posición | `#3aa8e0` |
| Azul de iconos de procesador | `#3aa8e0` |
| Fader de volumen (relleno) | `#8a5a2a` |
| Bordes | `#3a3a3a` |

---

## 8. Tipografía

- Sans-serif compacta, tamaño base aproximado **12 px**.
- Pantallas numéricas (tempo, posición): fuente monoespaciada o tabular, tamaño mayor (~16 px), color azul.
- Etiquetas de escena y pista: peso regular, tamaño ~11–12 px.

---

## 9. Componentes para GPUI (mapa de implementación)

| Componente visual | Propuesta de widget en GPUI |
|---|---|
| Grilla de clips | `ClipGrid` (vista virtualizada por fila/columna) |
| Celda de clip | `ClipCell` (estados: vacío, lleno, reproduciendo, detenido) |
| Encabezado de pista | `TrackHeader` (color, nombre, armado, S/M, fader, medidor) |
| Encabezado de escena | `SceneHeader` (botón de disparo, nombre) |
| Fader horizontal | `HSlider` con relleno y marcador |
| Medidor de nivel | `LevelMeter` (vertical) |
| Pantalla de tempo | `LcdReadout` (texto monoespaciado, color acento) |
| Botón de transporte | `TransportButton` (icono + estado activo) |
| Panel de navegador | `BrowserPanel` (búsqueda, filtros, lista) |
| Barra de estado | `StatusBar` (ayuda contextual, pestañas de vista) |
| Pestañas de vista | `ViewTabs` (ARRANGE / MIX / EDIT) |

---

## 10. Comportamiento clave a replicar

1. **Lanzamiento por fila:** disparar una escena lanza todos los clips de esa fila.
2. **Lanzamiento individual:** cada celda puede lanzarse o detenerse por separado.
3. **Sincronización:** los clips se alinean al compás configurado (tempo y 4/4).
4. **Ayuda contextual:** la barra inferior explica la acción disponible al pasar el cursor.
5. **Modo ARRANGE/MIX/EDIT:** cambio de vista sin perder el estado del proyecto.
6. **Navegador con filtros:** búsqueda y filtrado por ubicación, categoría, tipo, creador y etiquetas.
7. **Pistas fijas inferiores:** FX y Master siempre visibles al pie de la grilla.

---

## 11. Preguntas abiertas para el rediseño

- ¿El Clip Launcher de OpenLive debe ser el modo principal o una vista alternativa a ARRANGE?
- ¿Cuántas escenas y pistas visibles se requieren por defecto, y es necesaria la virtualización desde el inicio?
- ¿El navegador usa el mismo modelo de datos que el resto del motor (dispositivos en Rust) o un índice separado?
- ¿Qué paleta usará Hikaru OpenLive? Se recomienda definir tokens propios en vez de copiar los valores de esta referencia.
