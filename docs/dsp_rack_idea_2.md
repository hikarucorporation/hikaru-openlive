# Comportamiento e Interacción de Menús Contextuales en Slots (`dsp_rack_idea_2.md`)

Este documento especifica la lógica de interacción con el ratón (Click Izquierdo vs Click Derecho) en las tarjetas del **DSP Rack** y el botón del triángulo desplegable (`▼`).

---

## 1. Interacción con el Triángulo Desplegable (`▼`/`▲`)

El comportamiento del botón `[▼]` ubicado en la esquina superior derecha de la tarjeta depende del tipo de click realizado:

```

+---------------------------------------------------------------+
| [●] Empty Slot                                            [▼] | <-- Click Izquierdo o Derecho
+---------------------------------------------------------------+       +---+
|                                                               |       [   ]
|                                                               |       [   ]
|                                                               |       [   ]
|                                                               |       [   ]
|                                                               |       [   ]
|              Inserta tus plugins nativos acá                  |       [ + ]
|                                                               |       [   ]
|                                                               |       [   ]
|                                                               |       [   ]
|                                                               |       [   ]
|                                                               |       [   ]
|                                                               |       [   ]
|                                                               |       [   ]
+---------------------------------------------------------------+       +---+

```

### A. Click Izquierdo (Menú Contextual / Presets)
Abre las opciones generales de configuración y gestión del módulo o slot:
- `Save Preset...`
- `Add Wavetable Paths...`
- `Delete Slot`

### B. Click Derecho `▲` (Menú Selector de Dispositivos / Plugins Nativos)
Despliega el menú contextual de selección para instanciar o reemplazar el módulo en el slot actual:


```
┌───────────────────────────────────────────────────────────────┐
│ Efectos:                                                      │
│   1. OpenEQ3                                                  │
│                                                               │
│ Generadores:                                                  │
│   1. Hikaru OpenWavetable                                     │
│   2. Hikaru OpenDMS                                           │
└───────────────────────────────────────────────────────────────┘

+---------------------------------------------------------------+
| [●] Empty Slot                                            [▲] | 
+---------------------------------------------------------------+       +---+
|                                                               |       [   ]
|                                                               |       [   ]
|                                                               |       [   ]
|                                                               |       [   ]
|                                                               |       [   ]
|              Inserta tus plugins nativos acá                  |       [ + ]
|                                                               |       [   ]
|                                                               |       [   ]
|                                                               |       [   ]
|                                                               |       [   ]    
|                                                               |       [   ]
|                                                               |       [   ]
|                                                               |       [   ]
+---------------------------------------------------------------+       +---+

```

---

## 2. Comportamiento en Slots Vacíos (`Empty Slot`)

- **Click Izquierdo en el cuerpo del Slot Vacío / Botón `[ + ]`:** Muestra la lista de dispositivos disponibles para añadir un nuevo módulo al rack.
- **Click Derecho sobre el Slot o el botón `[▼]`:** Abre directamente el menú de selección de dispositivos nativos (*OpenEQ3*, *Hikaru OpenWavetable*, *Hikaru OpenDMS*).
- **Selección de Dispositivo:** Al seleccionar un elemento (ej. `Hikaru OpenWavetable`), el `Empty Slot` debe transformarse e instanciar dinámicamente la UI completa del módulo seleccionado.

---

## 3. Instrucciones de Código (`crates/hikaru_gui/src/views/dsp_rack.rs`)

1. Asignar manejador de eventos diferenciado:
   - `on_mouse_down(MouseButton::Left, ...)` -> Abre menú de opciones (`Save Preset...`, etc.).
   - `on_mouse_down(MouseButton::Right, ...)` -> Abre menú desplegable de selección de plugins nativos.
2. Al hacer click en `Hikaru OpenWavetable` dentro del menú flotante, instanciar la vista de `open_wavetable.rs` dentro de la tarjeta activa de forma segura (usando `cx.defer` para evitar reentrancy panics).
