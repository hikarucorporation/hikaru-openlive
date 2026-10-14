# ARQUITECTURA Y ESPECIFICACIÓN TÉCNICA: HIKARU OPENSTUDIO

## 1. MANDATOS DIRECTOS DE DESARROLLO
- **Lenguaje:** 100% Rust idiomático. Prohibido usar bindings a C/C++ salvo en librerías de bajo nivel estrictamente necesarias.
- **Estrategia de Desarrollo:** **UI First / Mockups Funcionales**. La interfaz debe ser visualmente atractiva, fluida y profesional ANTES de conectar la lógica pesada del backend.
- **Diseño Visual (GUI):** Estética nivel DAW comercial (Bitwig, Ableton, FL Studio) con tema oscuro, alto contraste y renderizado acelerado por hardware via GPU (`egui` + `eframe`).
- **Layout Rígido por Paneles:** Para evitar colapsos y desposicionamiento de widgets, la interfaz debe estructurarse mediante:
  - `egui::TopBottomPanel::top()`: Header bar (Transporte, Modos, BPM).
  - `egui::SidePanel::right()`: Mixer multipista y DSP Rack.
  - `egui::CentralPanel::default()`: Vista principal (Matriz de Clips en OPENLIVE / Timeline en OPENSTUDIO).
  - `egui::TopBottomPanel::bottom()`: Footer de monitoreo y Explorador de Archivos (Waveform/Preview).

## 2. ARQUITECTURA DEL WORKSPACE (CRATES)
- `hikaru_gui/`: Interfaz visual interactiva, paneles egui y `AudioProxy` para envío de comandos.
- `hikaru_core/`: Tipos base, utilidades de audio, SampleRate, AudioBuffer f32.
- `hikaru_transport/`: Reloj atómico compartido (`AtomicU64`), BPM, cuantización.
- `hikaru_audio_engine/`: Motor real-time lock-free, `PreviewPlayer`, ruteo de canales y mezcla maestra.
- `hikaru_dsp/`: Sintetizador Wavetable/Espectral nativo, filtros y efectos DSP.
- `hikaru_sequencer/`: Secuenciador, piano roll, arreglador y matriz de clips.
- `hikaru_plugin_host/`: Host nativo para plugins VST3 y CLAP.

## 3. FLUJOS DE COMUNICACIÓN (GUI -> MOTOR DE AUDIO)
La comunicación es estrictamente unidireccional y lock-free desde la UI hacia el motor usando `std::sync::mpsc`.

[hikaru_gui (egui)] ---> AudioProxy (Sender) ---> mpsc::channel ---> [hikaru_audio_engine (Audio Thread / CPAL)]
- La GUI despacha enums `GuiCommand` (Play, Stop, LoadClip, SetPreviewVolume).
- El motor procesa los comandos sin bloquear la salida del stream de CPAL.