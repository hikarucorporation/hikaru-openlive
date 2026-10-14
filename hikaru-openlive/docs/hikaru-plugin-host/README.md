# Documentación: Hikaru Plugin Host (LGPLv3)

## Módulo Frontend: Representación Nativa de Perillas en el DSP Rack

Cuando un plugin externo (`.vst3` o `.clap`) es cargado en el DSP Rack de Hikaru OpenLive, el host no solo tiene la opción de abrir la interfaz gráfica original del fabricante (vía X11/Wayland nativo), sino que expone un **panel de control genérico con perillas nativas (GPUI)** mapeadas automáticamente a los parámetros del backend. Esto permite modular, automatizar y ver los valores de forma unificada sin depender de que la GUI del plugin sea un desastre flotante.

A continuación, se detalla la estructura visual y conceptual mediante esquemas en ASCII para el frontend:

### 1. Estructura General del Slot en el DSP Rack (Vista Frontend)

```text
+--------------------------------------------------------------------------+
| DSP RACK: Track 1 | Scene 1                                              |
+--------------------------------------------------------------------------+
| [●] 01 Vital                                                         [v] |
+--------------------------------------------------------------------------+
|  [ CLAP ]  -  Sintetizador Wavetable                                     |
|  Vital.clap                                                              |
+--------------------------------------------------------------------------+
|  PARÁMETROS NATIVOS (HOST GPUI RENDER)                                   |
|                                                                          |
|    ( O )          ( O )          ( O )          ( O )                    |
|   Cutoff         Resonance      Wavetable Pos   Unison Detune            |
|   [ 12.4 kHz ]   [ 45% ]        [ Wav 03 ]      [ 0.15 ]                 |
|                                                                          |
|    ( O )          ( O )          ( O )          ( O )                    |
|   Env Attack     Env Decay      LFO 1 Rate      FX Mix                   |
|   [ 10 ms ]      [ 350 ms ]     [ 1/4 Beat ]    [ 80% ]                  |
+--------------------------------------------------------------------------+
| [+] ⟨ Añadir Efecto / Instrumento ⟩                                      |
+--------------------------------------------------------------------------+

```

---

### 2. Caso A: Sintetizador Wavetable (Ej. Estilo *Vital*)

Para un instrumento virtual o sinte, el frontend del DSP Rack agrupa los parámetros expuestos por el backend (osciladores, filtros, envolventes y modulaciones) en una grilla compacta de perillas vectoriales renderizadas por GPUI:

```text
+--------------------------------------------------------------------------+
| [●] 01 Vital (Instrument)                                            [v] |
+--------------------------------------------------------------------------+
| OSCILLATORS                     FILTERS             ENVELOPES            |
|                                                                          |
|    ( O )       ( O )             ( O )               ( O )       ( O )   |
|   Wavetable    Spectral         Cutoff              Attack      Release  |
|   [ Form A ]   [ Morph ]        [ 8.2 kHz ]         [ 5ms ]     [ 400ms ]|
|                                                                          |
|    ( O )       ( O )             ( O )               ( O )       ( O )   |
|   Voice Count  Detune           Resonance           Decay       Sustain  |
|   [ 7 Voices ] [ 0.12 ]         [ 30% ]             [ 200ms ]   [ 75% ]  |
+--------------------------------------------------------------------------+

```

---

### 3. Caso B: Efecto Espectral / Procesador (Ej. Estilo *UZU* - Spectral Phaser)

Para plugins de efectos de audio, el diseño se adapta a la cadena de procesamiento de la pista, mostrando controles dinámicos de mezcla, frecuencia y modulación espacial:

```text
+--------------------------------------------------------------------------+
| [●] 02 UZU (Audio Effect)                                            [v] |
+--------------------------------------------------------------------------+
| SPECTRAL PHASER / FX MODULE                                              |
|                                                                          |
|    ( O )          ( O )          ( O )          ( O )                    |
|    Width          Offset         Depth          Speed                    |
|   [ 100% ]       [ 0.00dB ]     [ 0.00 ]       [ 50.0 ]                  |
|                                                                          |
|    ( O )          ( O )                                                  |
|    Mix            Blur                                                   |
|   [ 100% ]       [ 0.00 ]                                                |
|                                                                          |
| ──────────────────────────────────────────────────────────────────────── |
| [ Botón: Abrir GUI Nativa X11/Wayland ]   [ Estado: Vinculado (OK) ]     |
+--------------------------------------------------------------------------+

```

---

### 4. Especificaciones Técnicas del Frontend (GPUI Component)

* **Contenedor Principal (`dsp_rack_slot.rs`):** Elemento flexible con bordes redondeados, indicadores de estado de CPU en tiempo real (punto verde/rojo) y selector desplegable de opciones del slot.
* **Sistema de Perillas (`KnobComponent`):** Renderizado vectorial escalable mediante GPUI, con soporte para lectura de valores en doble clic, control por desplazamiento vertical del mouse (drag) y tipografía compacta acorde al estilo estético heredado de Bitwig Studio.
* **Enlace de Datos (`ParameterBinding`):** Cada perilla interactúa directamente con un `Arc<AtomicF32>` o canal lock-free conectado al hilo de audio del backend del plugin, evitando bloqueos en la interfaz gráfica.

---