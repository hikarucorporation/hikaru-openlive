## El Secreto del "Zoom Bidireccional Centrado en Cursor" de Bitwig

Eso que intentás explicar "en gaucho criollo" es una de las mayores joyas de UX de Bitwig. No es un zoom clásico con la ruedita del mouse (que va a saltos/ticks); es un **Zoom Continuo basado en Dragging de 2 Ejes**.

En términos de código e ingeniería de UI, funciona combinando tres matemáticas simultáneas:

### 1. La Mecánica del Input (Middle Click Drag / Wheel Drag)

* **Drag Vertical (Eje Y):** Controla el **factor de escala horizontal (Zoom In / Zoom Out)** del tiempo/arranger. Si arrastrás el cursor hacia arriba, la escala del tiempo se expande linealmente; si arrastrás hacia abajo, se contrae.
* **Drag Horizontal (Eje X):** Controla el **Scroll/Pan temporal**. Si te movés a la izquierda o derecha mientras hacés drag, la vista se desplaza en el eje del tiempo.
* **Respuesta Logarítmica:** El zoom no aumenta en $1, 2, 3...$ sino de manera exponencial/logarítmica. Esto hace que pasar de ver 100 compases a ver 1 solo compás se sienta suave y no como un salto brusco.

### 2. El "Anchor Point" (Punto Ancla Fijo en el Cursor)

El truco mágico es que **el instante de tiempo (en ticks de audio/muestras) sobre el que pusiste el puntero del mouse NO SE MUEVE de la pantalla**.

* Al presionar la ruedita, la posición del mouse en píxeles ($X_{pixel}$) queda bloqueada como el **pivote**.
* A medida que el factor de zoom cambia con el arrastre del mouse, la vista calcula la nueva posición inicial ($StartTicks$) para que el evento justo debajo de tu cursor permanezca en la misma posición de la pantalla.

---

## ¿Cómo se traduce esto a Rust + GPUI para Hikaru OpenLive?

Con **Hikaru Ghidra + REA** podés desensamblar los gestores de eventos de mouse de Bitwig para corroborar la curva, pero para implementarlo directo en GPUI con Rust, la fórmula lógica de ese componente es:

```rust
// Pseudocódigo de la matemática del zoom estilo Bitwig en GPUI
fn on_mouse_drag(event: &MouseDragEvent, state: &mut TimelineState) {
    let delta_y = event.delta.y; // Controla la magnitud del Zoom
    let anchor_time = state.pixel_to_time(event.position.x); // Tiempo bajo el cursor

    // 1. Calcular nuevo factor de zoom con escala exponencial
    let zoom_factor = (delta_y * 0.005).exp();
    state.zoom_level = (state.zoom_level * zoom_factor).clamp(MIN_ZOOM, MAX_ZOOM);

    // 2. Ajustar el offset del tiempo para mantener el anchor_time en event.position.x
    let new_pixel_x = state.time_to_pixel(anchor_time);
    state.scroll_offset_x += event.position.x - new_pixel_x;
}

```

---

¡Con eso tenés exactamente la matemática que hace que Bitwig se sienta tan orgánico! Ahora que tenés a REA integrado vía MCP con tu fork de Ghidra, podés decirle al OpenCode que analice los controladores de mouse de Bitwig y te arme esa misma rutina fluida de zoom en el Arranger de Hikaru OpenLive. 🚀🔥

---

#### PD: Sí, esta documentación está escrito por Google Gemini. Estoy cansado en este momento (no me rompan las pelotas XD)

---

### Estoy haciendo un fork personal de Ghidra (llamado **`Hikaru Ghidra`**) para poder analizar los controladores de mouse de Bitwig. 

Quizás cuando tenga más control, tiempo y conocimiento sobre Ghidra, (sí, el mismisimo software de la NSA...), suba mi fork personal a Github bajo la LGPLv2.1

Olvidé decir que es para utilizarlo con el REA (Reverse Engineering Anything), basicamente el Hikaru Workspace actual se compone en dos directorios:

1. `/hikaru-ghidra`: El fork personal de Ghidra, para utilizarlo con **`REA (Reverse Engineering Anything) + OpenCode conectados mediante MCP + Hikaru Ghidra ayudandome a hacer ingeniería inversa y entender el código de Bitwig de C++/Java a Rust/GPUI.`**
2. `/hikaru-openlive`: El código fuente del Hikaru OpenLive de toda la vida.

En este momento (13 de octubre del 2026, 11:50 PM; Horario Argentino.) sí o sí subo esta documentación al repo más otros cambios medios boludos que ni me acuerdo jaja

---