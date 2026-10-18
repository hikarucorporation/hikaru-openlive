# MANUAL Y ESPECIFICACIÓN DE ORDEN DE DESARROLLO — HIKARU OPENSTUDIO

## 1. REGLAS INVARIABLES DE DESARROLLO
1. **LENGUAJE ÚNICO:** 100% Rust idiomático.
2. **FILOSOFÍA UI FIRST:** Primero se maqueta la interfaz con `egui`. Una vez validada la estética y el ruteo de eventos (clicks, sliders), se conecta al backend.
3. **AISLAMIENTO DE TIEMPO REAL:** El motor de audio (`hikaru_audio_engine`) NUNCA debe alocar memoria dinámicamente ni bloquear el callback de CPAL.

## 2. ORDEN ESTRICTO DE IMPLEMENTACIÓN Y ESTADO

### FASE 1: `hikaru_gui` (Maquetado Visual y Layout) -> [EN PROGRESO / CASI COMPLETO]
- Header Bar (Transporte, Modos, BPM).
- Vista OPENLIVE (Matriz de pads) / OPENSTUDIO (Timeline).
- Explorador de archivos inferior con renderizado de Waveform y reproducción interactiva (PreviewPlayer).

### FASE 2 & 3: `hikaru_audio_engine` & Canal IPC -> [EN PROGRESO ACTIVO]
- **Objetivo actual:** Conectar la GUI maquetada con el backend de audio mediante `mpsc::channel`.
- **Implementación:** La UI utiliza un `AudioProxy` para enviar enums `GuiCommand` de forma asíncrona al motor.
- **Audio Host:** Integración con `cpal` para salida de baja latencia; manejo de resampleo lineal para cargar archivos .wav al vuelo.

### FASE 4: `hikaru_sequencer` & `hikaru_dsp` -> [PENDIENTE]
- Lógica de disparo cuantizado de clips sincronizados al `AtomicU64` de posición de transporte.
- Sintetizador Wavetable interno.

### FASE 5: `hikaru_plugin_host` -> [PENDIENTE]
- Soporte VST3 / CLAP.