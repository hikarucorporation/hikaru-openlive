# Especificación Arquitectónica: Hikaru MCP Port (Model Context Protocol)

**Documento de Diseño Técnico — Versión 2.22.2**  
**Ubicación:** `/docs/hikaru-mcp-port/HIKARU-MCP-PORT-README-ES.md`  
**Estado:** Propuesta de Arquitectura / Borrador  
**Licencia:** Dual (Modo Offline/Files: LGPLv3 | Modo Live/GUI: AGPLv3)

---

## 1. Resumen Ejecutivo

El **Hikaru MCP Port** expone las capacidades internas del DAW **Hikaru OpenLive** a agentes de Inteligencia Artificial (OpenCode, Claude Code, REA) mediante el estándar abierto **Model Context Protocol (MCP)**. 

El objetivo es permitir la inspección, manipulación en tiempo real del motor de audio/DSP Rack, diagnóstico de crashes en plugins VST3/CLAP y automatización del flujo de producción sin acoplar la interfaz gráfica ni violar la filosofía de licencias del workspace.

---

## 2. Arquitectura del Sistema: Binario Standalone vía Socket Unix

Para evitar un refactor masivo de la GUI en GPUI o bloquear el thread principal de procesamiento de audio, la arquitectura adoptada es un servidor independiente comunicándose mediante Sockets de dominio Unix.


```

+-------------------------------------------------------+
|                  Cliente MCP (Agent)                  |
|           (OpenCode CLI o Claude Code / REA)          |
+-------------------------------------------------------+
                            |
                  Stdio / JSON-RPC 2.0
                            v
+-------------------------------------------------------+
|                 Binario: `hikaru_mcp`                 |
|        (Servidor MCP Standalone en Rust)              |
+-------------------------------------------------------+
             /                             \
            / (Socket Unix)                 \ (Modo Offline)
           v                                 v
+------------------------+        +------------------------+
| GUI / Engine Ejecutado |        |   Crates LGPL del DAW  |
|  `hikaru.sock` (AGPL)  |        |   (Proyectos/Archivos) |
+------------------------+        +------------------------+

```

### Modos de Operación y Personalidades (Licenciamiento)
1. **Modo Vivo (`--mode=live`):**
   - Se conecta al IPC de la interfaz en `$XDG_RUNTIME_DIR/hikaru/hikaru.sock`.
   - Permite controlar faders, disparar clips en la Session Matrix, auditar la carga de CPU/DSP y capturar estados de plugins VST3/CLAP en tiempo real.
   - **Licencia activa:** AGPLv3.

2. **Modo Archivo/Offline (`--mode=files`):**
   - Funciona sin requerir la GUI o el motor de audio en ejecución.
   - Lee, inspecciona, valida y convierte estructuras de proyectos, presets o mapas de parámetros leyendo directo los crates modulares.
   - **Licencia activa:** LGPLv3 (Apto para interoperabilidad con DAWs o herramientas de terceros).

---

## 3. Endpoints del Protocolo MCP (Resources & Tools)

### Recurso / URI Endpoints
- `hikaru://project/current`: Retorna el árbol JSON estructurado de la sesión activa (tracks, clips, ruteos y BPM).
- `hikaru://matrix/grid`: Retorna la matriz de escenas y clips de la vista Live/Session.
- `hikaru://plugins/discovered`: Lista los plugins VST3/CLAP escaneados en el sistema y sus estados de sandbox en `hikaru_plugin_host`.
- `hikaru://diagnostics/last-crash`: Vuelca el stacktrace y los logs de FFI del último fallo o pánico en el wrapper.

### Herramientas Principales (Tools)
1. **`set_parameter(track_id, plugin_id, param_idx, value)`**: Ajusta un parámetro del DSP Rack enviando la notificación bidireccional al VST3/CLAP.
2. **`trigger_scene(scene_index)`**: Lanza la reproducción síncrona de una escena en la matriz.
3. **`inspect_plugin_wrapper(plugin_id)`**: Devuelve el mapa de memoria del framebuffer, el XID o la superficie nativa en `hikaru-wrapper`.
4. **`decompile_plugin_stub(path)`**: Conecta con los binarios de ingeniería inversa (ej. `decomp.wtf` / REA) para auditar stubs de plugins cerrados o sin soporte nativo en Linux.

---

## 4. Prompts Preconfigurados (MCP Prompts)

- **`diagnosticar-fallo-audio`**: Analiza buffer underruns (xruns), conflictos en hilos de JACK/ALSA/Pipewire y pánicos FFI.
- **`reconstruir-plugin-rust`**: Mapea la interfaz de un plugin descompilado para generar un wrapper de baja latencia nativo.
- **`auditar-proyecto`**: Verifica dependencias de muestras de audio faltantes, plugins descolgados y coherencia en el ruteo de buses.

---

## 5. Plan de Implementación

1. **Fase 1:** Crear el crate `crates/hikaru_mcp` con la estructura de comunicación Stdio y serialización JSON-RPC 2.0.
2. **Fase 2:** Implementar la escucha del socket en `crates/hikaru_gui` para aceptar comandos externos sin latencia en el renderizado de GPUI.
3. **Fase 3:** Integrar el transporte de parámetros bidireccionales con `hikaru_plugin_host/hikaru-wrapper`.
