# Hikaru OpenLive: Modo Multijugador LAN & Colaboración con Agentes MCP

## 1. Filosofía del Sistema
El modo multijugador de *Hikaru OpenLive* no busca ser una plataforma de streaming de audio masivo, sino un entorno de **colaboración determinista y de baja latencia** basado en el intercambio de deltas de estado mediante el formato de proyecto `.oplf` (Hikaru OpenLive File). 

En lugar de transmitir audio comprimido por red (lo cual destruye la sincronización y genera latencia), cada cliente ejecuta localmente el motor de audio en Rust (`hikaru_audio_engine`) y el DSP Rack. La red únicamente sincroniza:
- Los disparos en la **Session Matrix** (clips, escenas, mute/solo).
- La automatización de parámetros de perillas y efectos (ej. el *Hikaru OpenSpectral Phaser*).
- Los comandos de control enviados por colaboradores humanos o agentes de IA.

---

## 2. Integración de Agentes con MCP (Model Context Protocol)
A diferencia de las herramientas de generación de audio sintético (Suno/Udio) que actúan como "cajas negras" creativas desvinculadas del proyecto, Hikaru implementa un **servidor MCP nativo** (`hikaru_mcp`).

### ¿Cómo operan los agentes?
1. **Acceso Local al Contexto:** El agente conectado vía MCP lee el estado actual del proyecto `.oplf` (pistas, clips activos, BPM, escala, cadena de efectos).
2. **Uso de Recursos Locales:** El agente manipula y reordena los **samples locales** que ya están en la máquina de los participantes o en rutas compartidas de la LAN, asegurando que no haya dependencia de servidores cloud externos para la síntesis.
3. **Ejecución Segura en el Motor:** Cualquier modificación sugerida por el agente pasa por el filtro de validación en tiempo real del motor en Rust (cero asignaciones de memoria en el hilo de audio), evitando *xruns* o cuelgues en plena sesión en vivo.

---

## 3. Arquitectura Técnica Propuesta
- **Transporte:** Sockets UDP/TCP livianos en red local (LAN-first) para asegurar latencias menores a 5ms entre estaciones de trabajo.
- **Sincronización de Estado:** Basada en CRDTs (Conflict-free Replicated Data Types) aplicados a la estructura JSON del `.oplf`, permitiendo que dos productores (o un productor y un agente MCP) editen la misma pista simultáneamente sin corromper el arreglo.
- **Roles en la Sesión:**
  - `Host`: Administra el master clock y el estado canónico del proyecto.
  - `Peer (Humano)`: Colaborador conectado desde otra PC en la LAN.
  - `Agent (MCP)`: Instancia de IA asistente conectada para sugerir arreglos, automatizar transiciones de efectos o disparar patrones rítmicos en la Session Matrix.