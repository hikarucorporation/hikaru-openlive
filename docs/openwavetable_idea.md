# Especificación Visual y Funcional: Hikaru OpenWavetable (`openwavetable_idea.md`)

Este documento define la arquitectura visual y el flujo de interfaz para el módulo **Hikaru OpenWavetable** integrado dentro del sistema de tarjetas fijas modulares del **DSP Rack**.

---

## 1. Diseño del Módulo dentro del Slot (Ancho Fijo ~230px, Altura Completa)

El dispositivo actúa como un oscilador / sintetizador de tabla de ondas primario que cabe exactamente dentro de una tarjeta estándar del DSP Rack.


```

DSP Rack : [ Track 1 | Scene 1 ]

+---------------------------------------------------------------+
| [●] 01 Hikaru OpenWavetable                               [▼] |
+---------------------------------------------------------------+
| ┌───────────────────────────────────────────────────────────┐ |
| │                                                           │ |
| │                 [ VISOR 3D WGPU CANVAS ]                  │ |
| │               /\  /\                                      │ |
| │              /  \/  \  /\                                 │ |
| │             /        \/  \                   < 113/256 >  │ |
| └───────────────────────────────────────────────────────────┘ |
|  < Growl Table 03.wav >                                       |
| ┌────────────────────────────────────────────────┐ ┌────────┐ |
| │    (O) UNISON    | (O) DETUNE  | (O)  PHASE    │ | (O)    | |
| │    [ 16 Voices ] |     [ 15% ] |     [ 180° ]  │ | WT POS | |
| └────────────────────────────────────────────────┘ └────────┘ |
|  [PITCH: 0st]                [VOICES: Poly]     [OCT: 0]      |
+---------------------------------------------------------------+
```

Nota: **`(O)`** = Knobs.

---

## 2. Desglose Estructural del Módulo

### A. Cabecera (Header)
- **Indicador de Estado `[●]`:** Botón verde/rojo para mutear/activar (Bypass / ON) el sintetizador.
- **Título del Módulo:** `01 Hikaru OpenWavetable`.
- **Menú de Opciones `[▼]`:** Menú contextual para guardar presets (`Save Preset...`), cargar rutas externas (`Add Wavetable Paths...`) o eliminar el slot (`Delete Slot`).

### B. Visor 3D WGPU (Canvas Principal)
- **Render de Onda:** Ocupa el centro superior de la tarjeta.
- **Interactividad:** Muestra la malla 3D/2D del archivo `.wav` cargado actualmente.
- **Drag & Drop Target:** Acepta directamente el arrastre de archivos `.wav` de tablas de ondas desde el File Explorer lateral.

### C. Selector / Navegador de Wavetables
- **Barra de Selección `< Nombre_Tabla.wav >`:**
  - Flechas `<` y `>` para iterar rápidamente entre las tablas de ondas contenidas en la carpeta actual (ej. *Cymatics - Growl Wavetables Vol 1*).
  - Hacer click sobre el nombre abre un popover flotante con la lista completa de tablas escaneadas.

### D. Panel de Controles Principal (Knobs)
- **`WT POS` (Wavetable Position):** Controla la posición del frame de la tabla (de 1 a 256).
- **`UNISON`:** Define el número de voces en unísono (1 a 16 voces).
- **`DETUNE`:** Controla la dispersión de afinación del unísono.

### E. Pie de Módulo (Footer Parameters)
- **`PITCH`:** Ajuste fino/semitonos (-24st a +24st).
- **`VOICES`:** Modo de polifonía (Mono, Legato, Poly).
- **`PHASE`:** Fase inicial del oscilador.

---

## 3. Instrucciones de Implementación para OpenCode

1. **Ubicación del Código:**
   - La lógica de renderizado debe encontrarse en `crates/hikaru_gui/src/views/open_wavetable.rs`.
2. **Encajonado en Tarjeta:**
   - Debe usar un contenedor `v_flex()` ajustado al `CARD_WIDTH = 230.0px` sin desbordar los límites del `RACK_HEIGHT`.
3. **Carga de Archivos:**
   - Al soltar un `.wav` desde el File Explorer o cambiar con las flechas `<` `>`, el motor actualiza la malla en el canvas WGPU sin bloqueos en el hilo de UI.