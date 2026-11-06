# Clean-Room UI & DSP Re-implementation Spec: Rust + GPUI

## 0. Alcance y Método (leer primero)

Esta especificación describe cómo re-implementar el **comportamiento y layout** de un panel DAW propietario (ejemplo: Detail Editor y Audio Clip Inspector de Bitwig) como código Rust libre y open-source para **Hikaru OpenLive** usando **GPUI**.

Utiliza deliberadamente un método **clean-room, black-box**:

- **Entradas permitidas:** capturas de pantalla, grabaciones de pantalla, manuales de usuario públicos, listas de atajos de teclado públicos, comportamiento observado de la aplicación en ejecución, y formatos de archivo que el usuario posee y puede leer con herramientas documentadas.
- **Entradas no permitidas:** código fuente propietario filtrado, decompilación de bytecode Java `.class`/`.jar` para copiar lógica, desensamblado de binarios C++ `.so`/`.dll` para extraer algoritmos, fragmentos de código copiados, iconos, fuentes o assets bitmap copiados, y reutilización de nombres de identificadores propietarios.
- **Por qué:** replicar el comportamiento observado es una práctica estándar de la industria (casos: Wine, ReactOS, los clones de limones/clean-room de IBM BIOS). No estamos accediendo a código propietario, sino documentando comportamiento visible al usuario. Esto mantiene la licencia GPL de Hikaru limpia y facilita la auditoría del código.

Cada ítem de la especificación debe citar su **fuente observada** (por ejemplo: `screenshot-01`, `manual p.42`, `observed: double-click on clip opens editor`). Los ítems sin fuente se marcan `UNVERIFIED` y no deben implementarse hasta ser verificados.

---

## 1. Objetivo

Producir modelos Rust idiomáticos y thread-safe, y vistas GPUI que:

1. Coincidan con el layout observado y el comportamiento de interacción del panel objetivo.
2. Usen el modelo de datos propio de Hikaru, nomenclatura propia y tokens de estilo propios.
3. Mantengan el hilo de audio libre de allocations, locks y llamadas bloqueantes.

Esto **no** es una traducción línea por línea. La unidad de trabajo es un **comportamiento observado**, no un archivo fuente.

---

## 2. Entregables por Panel

Para cada panel, deben existir los siguientes artefactos antes de comenzar la implementación:

| Artefacto | Formato | Propósito |
|---|---|---|
| `SPEC-<panel>.md` | Markdown | Layout observado, estados, interacciones, con fuentes |
| `model.rs` | Rust | Tipos de datos propiedad de Hikaru, derivados solo de la spec |
| `events.rs` | Rust | Enum explícito de eventos para mensajes UI-to-engine |
| `view.rs` | Rust (GPUI) | Árbol de renderizado que coincida con la spec |
| `tests/` | Rust | Tests de comportamiento (transiciones de estado, valores límite) |
| `SCREENSHOTS/` | PNG | Capturas de referencia, con la fuente anotada |

---

## 3. Extracción de Layout UI (Basada en Observación)

### 3.1 Auditoría estructural

Describir el panel como un árbol de regiones, no como código:

- Panes y splitters: ancho, alto, comportamiento de resize, tamaños mínimos.
- Regiones canvas: área de waveform, regla de tiempo, grilla de notas, con bounds en píxeles medidos desde screenshots.
- Controles: tipo (slider, toggle, botón, campo numérico), rango, default, y comportamiento de teclas modificadoras (Shift, Alt, doble-click).
- Roles de color: dar a cada color un nombre semántico (`track.accent`, `canvas.background`). No reutilizar los valores de la paleta del vendor a menos que sean re-medidos y mapeados a tokens de Hikaru.

### 3.2 Mapeo a GPUI

| Región observada | Constructo GPUI |
|---|---|
| Splitter / pane | `div().flex()` con `.flex_col()` o `.flex_row()` |
| Waveform / ruler / grid | Elemento `canvas()` con callback de paint |
| Controles numéricos y toggles | Vistas hijas stateful vinculadas a entidades del modelo |
| Iconos vectoriales | Assets SVG autoreados para Hikaru (sin iconos del vendor) |

---

## 4. Modelo de Estado y Eventos

### 4.1 Fuente única de verdad

Cada valor mutable en la UI vive en una entidad Rust. Las vistas leen de ella y emiten eventos. Las vistas nunca mutan el engine de audio directamente.

Usar la API actual de GPUI: `Entity<T>`, `Context<Self>`, `Window`, `cx.notify()`. Los nombres antiguos (`Model<T>`, `ViewContext`) están deprecados y no deben usarse en código nuevo.

### 4.2 Enum de eventos

```rust
pub enum DetailEditorEvent {
    SetClipGain { clip_id: ClipId, gain_db: f32 },
    MoveWarpMarker { clip_id: ClipId, marker_id: MarkerId, sample_index: u64 },
    SetLoopEnabled { clip_id: ClipId, enabled: bool },
}
```

Reglas:

- Los eventos transportan valores, nunca referencias ni closures.
- Las unidades son explícitas en los nombres (`gain_db`, `sample_index`). Evitar `f64` desnudo para tiempo; usar newtypes `SampleIndex(u64)` o `Beats(f64)`.
- Cada evento que cambia estado de audio tiene un test de cambio de estado correspondiente.

---

## 5. Modelo de Datos

### 5.1 Metadata de Clip

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct AudioClip {
    pub id: ClipId,
    pub sample_range: std::ops::Range<u64>,
    pub loop_range: Option<std::ops::Range<u64>>,
    pub transpose_semitones: i8,
    pub gain_linear: f32,
    pub warp_markers: Vec<WarpMarker>, // solo hilo UI
}
```

Notas:

- Usar tipos Rust idiomáticos. **No** usar `#[repr(C)]` a menos que exista un boundary FFI real. El layout matching a un binario del vendor no es necesario y acoplaría Hikaru a un formato desconocido.
- `loop_range` como `Option` reemplaza valores centinela como `loop_start == 0 && loop_end == 0`.
- `transpose_semitones` debe ser range-checked en construcción (por ejemplo `-48..=48`).
- Unidades: la spec debe declarar si `sample_range` está en frames a la sample rate del proyecto.

### 5.2 Formatos de archivo

Si el formato objetivo es uno que el usuario posee (por ejemplo, un `.wav` o un proyecto exportado), leerlo con un parser abierto documentado. No reverse-engineering del contenedor propietario del vendor a menos que su formato esté públicamente documentado.

---

## 6. Reglas del Hilo de Audio en Tiempo Real

Estas reglas son mandatorias y se hacen cumplir en code review.

- **Sin allocation** en el hilo de audio: sin `Vec::push`, `Box::new`, `String`, ni `format!`. Preasignar buffers durante el setup.
- **Sin locks** en el hilo de audio: sin `Mutex`, `RwLock`, ni receives bloqueantes de canales.
- **Mensajería lock-free:** usar un ringbuffer SPSC (por ejemplo el crate `rtrb`) desde la UI hacia el engine de audio. Para escalares compartidos, usar `std::sync::atomic::AtomicU32` almacenando `f32::to_bits()` (Rust std no tiene `AtomicF32`), o el crate `atomic_float`.
- **Buffers zero-copy:** pasar slices `&[f32]` o `&mut [f32]`. No copiar bloques de audio para pasarlos entre hilos.
- **Sin panics:** evitar `unwrap`, `expect`, e indexación sin bounds checks en el hilo de audio.

---

## 7. Pipeline REA (Clean-Room)

### Paso 1: Auditoría de comportamiento
- Grabar screenshots al 100% y 200%.
- Documentar cada estado y transición de los controles en `SPEC-<panel>.md`.
- Registrar cada interacción como: precondición, acción, resultado, fuente.

### Paso 2: Aislamiento del modelo
- Derivar `model.rs` solo de la spec de comportamiento.
- No abrir el binario del vendor para este paso.
- Revisión: cualquier nombre de tipo o campo que solo pudiera venir de los internals del vendor se renombra.

### Paso 3: Construcción de la vista GPUI

```rust
impl Render for DetailEditorPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .w_full()
            .h_64()
            .bg(rgb(0x1e1e1e)) // reemplazar con un token del tema Hikaru
            .child(self.waveform_canvas(cx))
            .child(self.controls(cx))
    }
}
```

### Paso 4: Verificación
- Comparar la salida GPUI contra los screenshots de referencia a la misma escala y anotar deltas de píxeles en la spec.
- Cada interacción en la spec tiene un test que maneja la vista y afirma el evento emitido.
- Marcar el ítem de la spec `DONE` solo cuando pasen tanto el test visual como el de comportamiento.

---

## 8. Checklist de Aceptación

- [ ] Cada ítem de la spec tiene una fuente o está marcado `UNVERIFIED`.
- [ ] Sin identificadores, iconos, fuentes ni código copiado del vendor en el repositorio.
- [ ] `model.rs` compila sin nombres derivados del vendor.
- [ ] El código del hilo de audio pasa las reglas de la sección 6.
- [ ] El código GPUI usa la API actual (`Entity`, `Context`, `Window`).
- [ ] Las notas de comparación visual están adjuntas.
- [ ] Los headers de licencia del repositorio son compatibles con GPL.

---

## 9. Preguntas Abiertas

- ¿Qué panel de Hikaru es el primer objetivo, y qué screenshots o manuales lo cubren?
- ¿Qué comportamiento de warp-marker está documentado públicamente y cuál debe testearse por observación?
- ¿Cuál es la sample rate objetivo y el tamaño de buffer para los tests del engine de audio?