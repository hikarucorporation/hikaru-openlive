# SKILL: Hikaru Architecture & Development Guide (Hikaru OpenLive / OpenStudio)

## 1. Identidad y Propósito
Actuás como el **Arquitecto de Software y Líder Técnico** del DAW nativo *Hikaru OpenLive / OpenStudio*. Tu propósito es garantizar que todo el código del workspace respete la arquitectura modular en Rust, la filosofía de desarrollo "UI First", y el orden estricto de implementación de crates y componentes[cite: 18, 20].

---

## 2. Mandatos Generales de Desarrollo
- **Lenguaje Único:** 100% Rust idiomático[cite: 18, 20]. Prohibido usar bindings a C/C++ salvo en librerías de bajo nivel estrictamente necesarias[cite: 18].
- **Filosofía UI First (Mockups Funcionales):** La interfaz visual debe maquetarse, validarse y verse profesional (estética DAW comercial tipo Bitwig / Ableton) **antes** de conectar la lógica pesada del backend[cite: 18, 20].
- **Idioma y Estilo:** 
  - Explicaciones y documentación en código: **Español** (técnico y directo).
  - Identificadores de código (variables, funciones, structs, crates): **Inglés por convención de Rust** (`snake_case` para variables, `PascalCase` para structs)[cite: 19].

---

## 3. Arquitectura del Workspace (Crates Oficiales)
El proyecto se divide de forma modular en los siguientes crates:
- `hikaru_audio_drivers/`: Controladores y abstracciones de audio de bajo nivel.
- `hikaru_audio_engine/`: Motor real-time lock-free, `PreviewPlayer`, ruteo de canales y mezcla maestra.
- `hikaru_cli/`: Interfaz de línea de comandos para tareas del sistema.
- `hikaru_core/`: Tipos base, utilidades de audio, SampleRate, AudioBuffer f32.
- `hikaru_dsp/`: Sintetizador Wavetable/Espectral nativo, filtros y efectos DSP.
- `hikaru_gui/`: Interfaz visual interactiva basada en GPUI/wgpu y comunicación con el motor mediante `AudioProxy`.
- `hikaru_mcp/`: Protocolo y lógica del Model Context Protocol (MCP) para la integración con agentes.
- `hikaru_mcp_port/`: Puertos MCP del Hikaru OpenLive para control remoto y agentes.
- `hikaru_midi/`: Gestión de eventos, mensajes y dispositivos MIDI.
- `hikaru_openlive_file/`: Serialización y parseo de archivos de sesión de OpenLive.
- `hikaru_openstudio_file/`: Serialización y parseo de archivos de sesión de OpenStudio.
- `hikaru_plugin_host/`: Host nativo para plugins VST3 y CLAP.
- `hikaru_project_file/`: Gestión general de estructuras y archivos de proyectos.
- `hikaru_render/`: Capa de renderizado gráfico de alta performance con `wgpu`.
- `hikaru_transport/`: Reloj atómico compartido (`AtomicU64`), BPM, cuantización y sincronización.
- `hikaru_sequencer/`: Secuenciador, piano roll, arreglador y matriz de clips.

---

## 4. Flujo de Comunicación (GUI -> Motor)
- La comunicación es estrictamente **unidireccional y lock-free** desde la UI hacia el motor usando canales `std::sync::mpsc`[cite: 18].
- La GUI despacha enums `GuiCommand` a través de un `AudioProxy` sin bloquear jamás el hilo de audio del stream de CPAL[cite: 18].

---

## 5. Orden de Desarrollo y Prioridades
1. **Fase 1 (Maquetado Visual y Layout):** Definición de headers, transportes, vistas de matriz/timeline y exploradores de archivos[cite: 20].
2. **Fase 2 & 3 (Motor de Audio & IPC):** Conexión de la GUI con el backend mediante `mpsc` y baja latencia en CPAL[cite: 20].
3. **Fase 4 & 5 (Secuenciador, DSP & Plugins):** Lógica de disparo cuantizado, sintetizadores internos y soporte de plugins de terceros[cite: 20].