# Roadmap de Ingeniería Inversa: De Bitwig Studio a Hikaru OpenLive (Rust + GPUI)

> **Documento de análisis y hoja de ruta arquitectónica**  
> Proyecto: **Hikaru OpenLive**  
> Objetivo: Reimplementar las capacidades del DAW privativo **Bitwig Studio** (Java / C++) en un ecosistema libre, moderno y seguro en **Rust + GPUI Kit**.

---

## 1. Diagnóstico del Estado Actual de Hikaru OpenLive

Actualmente, Hikaru OpenLive cuenta con cimientos sólidos y modernos:
- **Interfaz Acelerada por GPU:** Construida con `gpui-kit` de Zed Industries (`hikaru_gui`), incluyendo Session Matrix, Arranger View, Playlist, Piano Roll, Mixer, Canvas interactivo y DSP Rack.
- **Audio Engine Concurrente:** Procesamiento de audio en tiempo real lock-free sobre CPAL (`hikaru_audio_engine`), drivers multi-backend y un preview player dedicado.
- **Host de Plugins:** Implementación en desarrollo de hosting para VST3 y CLAP (`hikaru_plugin_host`) con ventanas flotantes nativas en X11/Xlib.
- **Ecosistema MCP:** Soporte para agentes de IA mediante el puerto MCP (`hikaru_mcp`, `hikaru_mcp_port`) para automatización, generación de proyectos e ingeniería inversa asistida.
- **Formatos de Proyecto e Instrumentos Nativos:** Serialización con `hikaru_project_file`, `.opsf` y `.oplf`, junto con sintetizadores y efectos en desarrollo (`OpenWavetable`, `OpenSpectralPhaser`, `OpenDMS`, `OpenEQ3`).

---

## 2. Diferenciales Arquitectónicos de Bitwig Studio

Bitwig Studio se destaca en el mercado de DAWs por cuatro pilares arquitectónicos que lo diferencian de herramientas tradicionales como Ableton Live o FL Studio:

1. **Frontend Desacoplado en Java:** Orquestación de UI basada en grafos de escena, gestión de proyectos, ruteo de eventos y ejecución de scripts.
2. **Motor de Tiempo Real en C++:** DSP estricto sin recolección de basura, procesamiento de bloques sample-accurate y el motor modular **The Grid**.
3. **Aislamiento Multi-proceso (Plugin Sandboxing):** Los plugins externos (VST2, VST3, CLAP) corren en subprocesos independientes (`bitwig-plugin-host`). Si un plugin genera un fallo de segmentación (segfault) o agota la memoria, el motor de audio y la UI del DAW permanecen intactos sin interrumpir la sesión en vivo.
4. **Sistema Universal de Moduladores y MPE:** Todo parámetro en cualquier dispositivo (nativo o de terceros) es modulable de forma polifónica y por bloques por LFOs, envolventes, macros, expresiones MPE y generadores matemáticos.

---

## 3. Requerimientos para ser un Clon / Sucesor Libre de Bitwig

Para completar la transición de la arquitectura Java/C++ a Rust + GPUI, se identifican las siguientes áreas críticas de desarrollo:

### 3.1. Sistema Universal de Moduladores (Modulation Architecture)
- **Concepto:** En Bitwig, cada dispositivo o rack contiene una bahía de moduladores anidables (LFO, ADSR, ParSeq-8, Audio Rate, Steps, Math, Expressions).
- **Implementación en Rust:**
  - Diseñar un sistema de buffers de modulación desacoplados en el crate de DSP (`hikaru_dsp`).
  - Capacidad de calcular señales de control por bloque (`audio buffer`) y a nivel de muestra (`sample-accurate`).
  - Mapeo lock-free hacia parámetros de plugins nativos y de terceros (automatización y modulación unificada).

### 3.2. Entorno Modular "The Grid" (Poly Grid y FX Grid)
- **Concepto:** El diferenciador más potente de Bitwig; un entorno de síntesis y procesamiento de efectos modular basado en nodos.
- **Implementación en Rust:**
  - **Grafo de DSP (DAG):** Ejecución de topología de nodos sin alocaciones dinámicas en el bucle caliente de audio (`render()`).
  - **Optimizaciones SIMD:** Uso de crates como `pulp` o `wide` para vectorización en bloques de osciladores, filtros y lógica booleana/matemática.
  - **Visualización en GPUI:** Lienzo interactivo basado en nodos con renderizado vectorial por GPU para cables y módulos.

### 3.3. Sandboxing de Plugins Fuera de Proceso (Out-of-Process Plugin Host)
- **Concepto:** Evitar cuelgues del DAW causados por código C/C++ inestable de terceros durante un show en vivo.
- **Implementación en Rust:**
  - Separar `hikaru_plugin_host` en un binario independiente trabajador (`hikaru_plugin_worker`).
  - **Comunicación IPC de Ultra Baja Latencia:** Uso de memoria compartida (Shared Memory con ringbuffers lock-free / SPSC queues) para el paso de audio estéreo/multicanal y eventos MIDI/CLAP.
  - Embebido o reparenting de ventanas gráficas (X11 `XReparentWindow`, Wayland protocols y Win32 `SetParent`).

### 3.4. Motor Elástico de Audio (Time-Stretching y Warp en Tiempo Real)
- **Concepto:** Disparo elástico de clips de audio sincronizados al tempo maestro en la Session Matrix sin alterar el pitch.
- **Implementación en Rust:**
  - Integración o desarrollo de algoritmos de alta fidelidad: WSOLA, Phase Vocoder de fase bloqueada o wrappers a bibliotecas libres modernas como `signalsmith-stretch`.
  - Múltiples modos de estiramiento: *Stretch HD*, *Beats/Slice*, *Texture* y *Repitch*.

### 3.5. Soporte Integral MPE (MIDI Polyphonic Expression)
- **Concepto:** Manipulación de notas individuales con expresión tridimensional (presión per-note, glide horizontal y timbre vertical).
- **Implementación en Rust:**
  - Extender `hikaru_midi` para gestionar el protocolo MPE estándar y MIDI 2.0.
  - Editor en `hikaru_gui` (Piano Roll) para dibujar y editar curvas de expresión por cada nota individual.
  - Enrutamiento polifónico a plugins CLAP y sintetizadores internos (`OpenWavetable`).

### 3.6. API y Scripting para Controladores Hardware
- **Concepto:** El *Controller Scripting API* de Bitwig permite integrar cualquier hardware (Launchpad, APC, Push, etc.) con retroalimentación bidireccional visual en pads y pantallas.
- **Implementación en Rust:**
  - Runtime embebido ligero y seguro (ej. WebAssembly con `wasmtime` o scripts en `Rhai`/`Lua`).
  - Alternativamente, aprovechar el puente existente `hikaru_mcp` para exponer una API de eventos y control de sesión a scripts y agentes externos.

### 3.7. Suite de Dispositivos Nativos Esenciales (Native Device Rack)
- **Advanced Sampler:** Multisample layers, keyzones, velocity split, round-robin, modulación de puntos de loop y síntesis granular.
- **Drum Machine / Drum Rack:** Celdas de pads independientes con cadenas de procesamiento dedicadas.
- **Toolbox de Utilidades:** Inversores de fase, gestión Mid/Side estéreo, DC offset filter y crossfader de performance.

---

## 4. Matriz Comparativa: Hikaru OpenLive vs. Bitwig Studio

| Componente | Bitwig Studio (Privativo) | Hikaru OpenLive (Estado Actual) | Hikaru OpenLive (Objetivo Final) |
| :--- | :--- | :--- | :--- |
| **Lenguaje Core** | Java (UI/Orquestación) + C++ (DSP) | Rust 100% | Rust 100% (seguridad de memoria y rendimiento bare-metal) |
| **Tecnología GUI** | Scene Graph personalizado en Java | GPUI Kit (Zed) acelerado por GPU | GPUI Kit con vistas modulares acoplables y The Grid |
| **Arquitectura de Plugins**| Subprocesos aislados (Crash-proof) | En el mismo proceso (In-process) | Multi-proceso mediante Shared Memory IPC |
| **Modulación** | Moduladores globales anidables | Automatización estática básica | Bahía de moduladores sample-accurate anidables |
| **Síntesis Modular** | The Grid (Poly Grid, FX Grid) | No disponible | Entorno de nodos JIT / SIMD compilado en Rust |
| **Warp / Stretching** | Múltiples modos HD propietarios | Reproducción y resampling básico | Motor elástico de alta fidelidad (WSOLA / Phase Vocoder) |
| **Expresividad** | MPE nativo en todo el pipeline | MIDI estándar | MPE completo en Piano Roll, DSP y CLAP |
| **Controladores** | Java / JavaScript Controller API | Mapeo directo en código | Runtime Wasm / Rhai o extensión vía MCP Port |
| **Licencia** | Código cerrado, suscripciones anuales | AGPL v3 (100% Software Libre) | AGPL v3 (Soberanía tecnológica comunitaria) |

---

Hecho con **Gemini 3.8 Flash (Thinking)** en **Google Antigravity**

---