**Arquitectura del Arranger/Mix View de Bitwig (Especificación para egui)**

**1. Distribución General (Layout Superior y Central)**

* **Top Bar (Transporte y Menús):** Barra superior horizontal fija. Contiene controles de reproducción (Play, Stop, Rec), displays de Tempo/BPM (150.00), Compass/Time signature (4/4), posición de tiempo (`1.1.1.00`), herramientas de edición y botones de toggle de vista.
* **Canvas Central (Timeline / Grilla):** El área central oscura es la grilla principal del Arranger. Está delimitada y limpia, reservada exclusivamente para el scroll horizontal/vertical de las pistas y los clips de audio/MIDI.
* **Bottom Status Bar:** Barra inferior de información contextual (e.g., *"DOUBLE-CLICK Insertar pista desde dispositivo"*) y herramientas secundarias/zooms a la derecha.

**2. Panel Izquierdo: Estructura Vertical de Pistas (Track Columns)**
El lado izquierdo **no es una lista lineal común**, sino que cada pista es una **columna vertical modular** dividida en tres secciones superpuestas:

* **Sección Superior (Headers de Pista & Clip Launcher):**
* Solapas superiores con el nombre de la pista (*"Drum Machine"*, *"Ultracomb Riddim Bass"*).
* Matriz integrada de Clip Launcher por escenas (*Scene 1* a *Scene 8*) con botones individuales de Play por slot.

* **Sección Media (Dispositivos / Chains):**
* Bloque colapsable que muestra los instrumentos y efectos cargados en la pista (*"Drum Machine"*, *"Poly Grid"*, *"LFO MOD"*).

* **Sección Inferior (Channel Strips / Mixer Controls):**
* Botones compactos de **Arm/Rec (punto), Solo (S), Mute (M)**.
* Faders verticales de volumen por canal con vúmetros integrados (lectura en dB como `-11.4`, `-10.0`) y slider de paneo horizontal.

**3. Panel Derecho: Canal Master Fijo**

* Un panel vertical independiente fijado a la extrema derecha.
* Contiene el header de la pista **Master**, su cadena de efectos (*"Evil Otto"*, *"Compressor+"*, *"Peak Limiter"*) y su fader/vúmetro de salida principal sin mezclarse con las pistas de usuario.

---

### **Directivas Técnicas de Implementación para Claude (Do's & Don'ts)**

#### **LO QUE SÍ DEBES HACER (DO'S):**
1. **Fidelidad al Firma de Función Actual:** Mantener la firma exacta de la función `show()` en `arranger_view.rs`:
   ```rust
   pub fn show(
       ui: &mut Ui,
       tracks: &mut [mixer::Track],
       matrix_state: &SessionMatrixState,
       transport: &TransportPosition,
       audio_proxy: &AudioProxy,
   )

    ```

2. **Sincronización con AudioProxy:** Enviar los comandos mediante `audio_proxy.send(GuiCommand::...)` al interactuar con Faders, Paneo, Mute, Solo o la barra de transporte/Seek.
3. **Uso Eficiente de `egui`:** Utilizar `egui::ScrollArea::both()` o paneles bien delimitados (`egui::SidePanel` / `egui::CentralPanel`) para asegurar que la grilla del timeline central sea la que escrolea y el panel Master derecho quede fijo.

#### **LO QUE NO DEBES HACER (DON'TS):**

1. **NO reinventar tipos de datos:** Usar exclusivamente las estructuras de datos existentes (`mixer::Track`, `SessionMatrixState`, `SlotState`, `TransportPosition`). No instanciar structs ficticias.
2. **NO romper el lienzo central:** El canvas central (Timeline) debe ser amplio e interactivo; bajo ninguna circunstancia los paneles de la izquierda deben devorarse el ancho horizontal de la pantalla.
3. **NO hardcodear lógica de audio:** La interfaz solo debe renderizar el estado actual recibido por parámetro y despachar eventos a la proxy de audio.

---