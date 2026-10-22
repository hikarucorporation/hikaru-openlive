# Hikaru MCP Port — Estado de implementación

**Ubicación:** `/docs/hikaru-mcp-port/ESTADO-IMPLEMENTACION.md`
**Fecha:** 2026-10-22

Este documento es el estado real de lo que hay implementado, a diferencia de
[HIKARU-MCP-PORT-README-ES.md](HIKARU-MCP-PORT-README-ES.md), que es la
especificación original.

---

## 1. Resumen

| Pieza | Estado | Dónde |
|---|---|---|
| Configuración de los slots de puerto | ✅ Hecho | `crates/hikaru_mcp_port/src/config.rs` |
| CLI de administración de puertos | ✅ Hecho | `crates/hikaru_cli/src/mcp.rs` |
| UI de los slots en la GUI | ✅ Hecho | `crates/hikaru_gui/src/views/mcp_port_settings.rs` |
| Transporte IPC (sockets reales) | ⬜ Esqueleto | `crates/hikaru_mcp_port/src/ipc.rs` |
| **Servidor MCP (JSON-RPC)** | ✅ **Hecho** | `crates/hikaru_mcp/` |
| Proyecto `.oplf` / `.opsf` por MCP | ✅ Hecho | `crates/hikaru_mcp/src/project.rs` |
| Descubrimiento de plugins por MCP | ✅ Hecho | `crates/hikaru_mcp/src/plugins.rs` |
| Ingeniería inversa por MCP (Ghidra) | ✅ Hecho | `crates/hikaru_mcp/src/ghidra.rs` |
| Puente con REA | ✅ Hecho | `crates/hikaru_mcp/src/rea.rs` |
| Modo vivo (GUI ↔ servidor) | ⬜ Pendiente | la GUI no publica `hikaru.sock` |

---

## 2. El servidor MCP

`crates/hikaru_mcp` es el binario que **habla MCP**. Es el otro lado del
puerto: `hikaru_mcp_port` tiene la configuración de los slots, este crate
tiene el proceso que los atiende.

```sh
hikaru_mcp --mode files --project mi-sesion.oplf              # por stdio
hikaru_mcp --mode files --transport unix --socket /ruta.sock  # por socket
hikaru_mcp --status                                           # diagnóstico
```

### 2.1 Recursos

| URI | Contenido |
|---|---|
| `hikaru://project/current` | Metadatos, BPM, pistas, escenas del proyecto cargado |
| `hikaru://matrix/grid` | La matriz de pads (o la playlist, en OpenStudio) |
| `hikaru://plugins/discovered` | Los VST3/CLAP que el host encontró |
| `hikaru://plugins/telemetry` | La telemetría viva (CPU, xruns, editor) |
| `hikaru://diagnostics/last-crash` | El último fallo del host de plugins |

### 2.2 Herramientas

| Herramienta | Qué hace |
|---|---|
| `list_plugins` | Lista los plugins instalados, con su ruta y su ABI |
| `plugin_info` | Identidad, CID de clases, binario y entry points |
| `plugin_parameters` | Los parámetros del plugin (con la instancia real) |
| `set_parameter` | Mueve un parámetro (normalizado 0..1) |
| `inspect_plugin_wrapper` | La superficie nativa del editor |
| `decompile_plugin_stub` | Ingeniería inversa con Ghidra headless |
| `plugin_symbols` | Los símbolos exportados (sin Ghidra) |
| `audit_project` | Recursos faltantes, plugins ausentes, ruteo roto |
| `project_info` | Los metadatos del proyecto |

### 2.3 Prompts

`diagnosticar-fallo-audio`, `reconstruir-plugin-rust`, `auditar-proyecto`.

---

## 3. Cómo se conecta un agente

### 3.1 OpenCode (o cualquier cliente MCP)

```sh
opencode mcp add hikaru --global \
  --env GHIDRA_INSTALL_DIR=/ruta/a/ghidra \
  -- /ruta/a/hikaru_mcp --mode files
```

O a mano, en `~/.config/opencode/opencode.jsonc`:

```jsonc
"mcp": {
  "servers": {
    "hikaru": {
      "type": "local",
      "command": ["/ruta/a/hikaru_mcp", "--mode", "files"],
      "environment": { "GHIDRA_INSTALL_DIR": "/ruta/a/ghidra" }
    }
  }
}
```

### 3.2 El DAW (slot de puerto)

`~/.config/hikaru/mcp-ports.json`, campo `server_flags`:

```json
"server_flags": {
  "executable": "/ruta/a/hikaru_mcp",
  "args": ["--mode", "files"],
  "env": { "GHIDRA_INSTALL_DIR": "/ruta/a/ghidra" },
  "working_dir": "/ruta/al/workspace"
}
```

> **Ojo:** el DAW **todavía no** lanza este comando. `server_flags` se lee y
> se muestra en la UI, pero ningún código lo ejecuta (ver §4).

---

## 4. Lo que falta

### 4.1 El transporte del DAW

`crates/hikaru_mcp_port/src/ipc.rs` es un esqueleto a propósito: modela las
reglas de los slots (deshabilitado = no acepta, lleno = no entra otro) sin
abrir sockets. Para que el DAW publique un slot hay que:

1. Implementar `UnixListener` por slot habilitado, en un hilo aparte del
   render de GPUI.
2. Lanzar `hikaru_mcp --transport unix --socket <ruta>` con `server_flags`.
3. Conectar el socket de la GUI (`hikaru.sock`) para el modo vivo.

### 4.2 El modo vivo

`--mode=live` está implementado en el servidor, pero la GUI todavía no
publica `hikaru.sock`: el modo vivo arranca, avisa por el error estándar y
sigue sirviendo el estado del disco.

### 4.3 La telemetría

`hikaru_mcp_port::telemetry` existe (el buffer y el exportador) pero **no
tiene productor**: nadie lo llena. Los recursos
`hikaru://plugins/telemetry` devuelven un array vacío hasta que el motor
reporte.

---

## 5. El motor de ingeniería inversa

`decompile_plugin_stub` habla con **Ghidra headless** a través de un script
Java embebido (`crates/hikaru_mcp/src/ghidra_scripts/Decomp.java`).

### 5.1 Por qué Ghidra directo y no REA

El proveedor de Ghidra de REA exige la línea **12.1.x** y rechaza cualquier
otra versión (`unsupported_version`). Hablar con `analyzeHeadless`
directamente funciona con cualquier Ghidra que tenga el decompilador nativo.

El puente con REA (`crates/hikaru_mcp/src/rea.rs`) queda como segunda opción:
si REA tiene un proveedor disponible, se usa; si no, se informa.

### 5.2 El caché de proyectos

Importar y analizar un plugin de 7 MB con Ghidra tarda minutos. Los
proyectos se cachean en `/var/tmp/hikaru/ghidra/`:

- La primera llamada sobre un binario importa y analiza.
- Las siguientes reutilizan el proyecto y sólo corren el script (segundos).

La huella del binario (path + tamaño + mtime) decide si el caché sirve.

> **OJO con la ruta del caché:** Ghidra rechaza cualquier elemento del path
> que empiece con `.` (`Path element starting with '.' is not permitted`).
> Eso deja fuera a `~/.cache` y a `~/.local/state`, por eso el caché vive en
> `/var/tmp`. Se puede cambiar con `HIKARU_GHIDRA_CACHE`.

### 5.3 Los tres detalles que hacen que el script funcione

Del debugging del script de Ghidra, en orden de importancia:

1. **La cabecera `/* ### ... ### */` no es decorativa.** El proveedor de
   scripts Java de Ghidra la usa para armar el bundle de la clase. Sin ella,
   el headless aborta con `ClassNotFoundException: Decomp not found`.
2. **El archivo tiene que estar en ASCII puro.** Los acentos rompen la
   compilación si `javac` no lee el archivo como UTF-8, y —peor— un bloque
   de comentario anidado cierra el comentario de la cabecera antes de
   tiempo y convierte el resto del archivo en código inválido.
3. **`-process` no acepta la ruta del archivo**, sino el nombre *dentro* del
   proyecto. Con el path completo, Ghidra aborta con
   `invalid filename specified`.

### 5.4 Modos

| Modo | Qué devuelve |
|---|---|
| `overview` | Arquitectura, secciones, entry points exportados |
| `functions` | El listado de funciones con dirección y tamaño |
| `strings` | Los strings del binario |
| `decompile` | El pseudocode de una función (por nombre o dirección) |

En los binarios de plugin (bibliotecas compartidas) el punto de entrada suele
quedar como `Label` y no como `Function`: el script lo desensambla y crea la
función antes de decompilar.

### 5.5 El puente con REA

`crates/hikaru_mcp/src/rea.rs` invoca la CLI de REA (`rea analyze`,
`rea decompile`) como subproceso, y no como cliente MCP: dos clientes MCP que
se hablan por el mismo socket se bloquean, y la CLI es un proceso aparte.

La salida de REA se pide con `--full-output` para que venga en un sobre
(`{"ok": true, "data": ...}`). Sin el sobre, un error de REA es
indistinguible de un análisis: el error viene DENTRO de `data`, con `ok` en
`true`.

---

## 6. Licencias

El crate `hikaru_mcp` es AGPLv3, como el DAW. Los formatos de archivo que
manipula (`.oplf`/`.opsf`) son LGPLv3 y viven en crates aparte.
