# Hikaru OpenStudio: Colaboración Multijugador LAN y Agentes MCP en Playlist/Timeline (Modo Canción)

## 1. Visión General (Playlist / Timeline & Mixer)
A diferencia de la Session Matrix pensada para el directo y la improvisación basada en clips, **Hikaru OpenStudio** es el entorno enfocado en **componer y producir canciones completas (temitas de verdad)** dentro de la Playlist/Timeline. 

En este modo, los colaboradores (humanos y agentes MCP) trabajan sobre la línea de tiempo principal, la **Mixer** y la vista de organización para estructurar temas complejos (ej. producciones de Dubstep, Drum & Bass, etc.) mediante edición colaborativa en tiempo real sin romper la continuidad del motor de audio en Rust (`hikaru_audio_engine`).

---

## 2. Sincronización de Playlist, Samples y Mixer
Para armar canciones completas en conjunto sin pisarse ni arruinar la mezcla:

- **Organización de Samples y Pistas:** Los colaboradores pueden arrastrar samples, clips de audio y arreglos directamente sobre la Playlist/Timeline de manera sincronizada mediante deltas del formato `.opsf` (JSON).
- **Control Colaborativo de Mixer:** Ajuste multijugador de niveles de ganancia, paneo, envíos de bus y la cadena del DSP Rack (como la automatización de parámetros del *Hikaru OpenSpectral Phaser*).
- **Bloqueo Fino de Clips:** Si un productor está editando un clip de audio o patrón específico en la Playlist, esa región se resalta visualmente para evitar conflictos de arrastre.
- **Reproducción Continua (Playhead Sync):** Sincronización del tiempo de reproducción para que todos los pares en la LAN escuchen la estructura del tema en el mismo punto de la Playlist.

---

## 3. Asistencia de Agentes MCP en la Composición (OpenStudio MCP)
Los agentes conectados a través del servidor nativo `hikaru_mcp` actúan como **asistentes de producción e ingeniería de mezcla dentro de la Playlist y la Mixer**, sin recurrir a IAs generativas externas de baja calidad:

### Capacidades del Agente MCP en OpenStudio:
1. **Estructura de la Canción:** El agente puede proponer y organizar la maquetación de secciones completas (Intro, Drop, Breakdown, Outro) acomodando los clips en la Playlist.
2. **Edición MIDI y Piano Roll:** Apoyo en progresiones armónicas, bajo rítmico y variaciones de notas dentro del entorno.
3. **Automatizaciones en la Mixer:** Creación automática de curvas de automatización para efectos en la Mixer y en el DSP Rack utilizando los efectos y samples locales.
4. **Procesamiento de Audio Seguro:** Todas las sugerencias del agente se procesan a través del sistema *lock-free* y unidireccional de Rust, garantizando estabilidad total y cero tirones en el buffer.

---

## 4. Flujo de Trabajo Típico en OpenStudio (Productor + Agente + Peer LAN)
1. **Host:** Abre el proyecto `.opsf` en OpenStudio y establece el tempo (BPM) y la estructura base del tema en la Playlist.
2. **Peer (Humano):** Se conecta por LAN y acomoda los arreglos de la batería y la línea de bajo en la Playlist y la Mixer.
3. **Agent (MCP):** Analiza el contexto de la canción por MCP, automatiza los envíos de efectos en la Mixer para las transiciones y sugiere variaciones en la Playlist usando los recursos locales.