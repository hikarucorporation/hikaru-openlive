# Hikaru MCP Server

El **servidor MCP** de Hikaru OpenLive: el binario que habla
[Model Context Protocol] con los agentes de IA y les da acceso al DAW.

Es el otro lado de `crates/hikaru_mcp_port`: ese crate tiene la *configuración*
de los slots de puerto (qué puerto, qué transporte, cuántos clientes), este
crate tiene el *proceso* que los atiende.

## Arrancar

```sh
# Por stdin/stdout: así lo lanzan OpenCode y Claude Code.
hikaru_mcp --mode files --project mi-sesion.oplf

# Por socket de dominio Unix: así lo publica el DAW para un slot.
hikaru_mcp --mode files --transport unix --socket /run/user/1000/hikaru/hikaru-mcp-1.sock

# Sólo el estado, sin hablar MCP (para depurar).
hikaru_mcp --status
```

En modo stdio **nada puede escribir en la salida estándar**: ahí va el
protocolo. Todo el diagnóstico va al error estándar.

## Lo que expone

### Recursos

| URI | Qué contiene |
|---|---|
| `hikaru://project/current` | El proyecto cargado: metadatos, BPM, pistas, escenas. |
| `hikaru://matrix/grid` | La matriz de pads de la vista Live/Session. |
| `hikaru://plugins/discovered` | Los plugins VST3 y CLAP del sistema. |
| `hikaru://plugins/telemetry` | La telemetría viva de los plugins. |
| `hikaru://diagnostics/last-crash` | El último fallo del host de plugins. |

### Herramientas

| Herramienta | Qué hace |
|---|---|
| `list_plugins` | Lista los plugins instalados, con su ruta y su ABI. |
| `plugin_info` | Identidad, CID de clases, binario y entry points. |
| `plugin_parameters` | Los parámetros del plugin, con la instancia real. |
| `set_parameter` | Mueve un parámetro (valor normalizado 0..1). |
| `inspect_plugin_wrapper` | La superficie nativa del editor del plugin. |
| `decompile_plugin_stub` | Ingeniería inversa con Ghidra headless. |
| `plugin_symbols` | Los símbolos exportados, sin Ghidra. |
| `audit_project` | Recursos faltantes, plugins ausentes, ruteo roto. |
| `project_info` | Los metadatos del proyecto. |

### Prompts

`diagnosticar-fallo-audio`, `reconstruir-plugin-rust`, `auditar-proyecto`.

## El motor de ingeniería inversa

`decompile_plugin_stub` habla con **Ghidra headless** (`analyzeHeadless`) a
través de un script Java que va embebido en el binario
(`src/ghidra_scripts/Decomp.java`).

Se usa Ghidra directo y no a través de REA por una razón práctica: el
proveedor de Ghidra de REA exige la línea **12.1.x** y rechaza cualquier otra
versión. Hablar con `analyzeHeadless` funciona con cualquier Ghidra que
tenga el decompilador nativo.

Ghidra se busca en este orden:

1. la variable de entorno `GHIDRA_INSTALL_DIR`,
2. los Ghidra del workspace de Hikaru (`~/Documentos/Codigo_Fuente/hikaru-workspace/*/dist/ghidra_*`),
3. `/opt/ghidra`, `/usr/share/ghidra`, `/usr/local/ghidra`.

El proyecto de Ghidra se cachea en `/var/tmp/hikaru/ghidra/` (con
`HIKARU_GHIDRA_CACHE` se cambia): la primera llamada sobre un binario
importa y analiza, y las siguientes reutilizan el proyecto y sólo corren el
script. **El path del caché no puede tener elementos que empiecen con `.`**:
Ghidra los rechaza (`~/.cache` y `~/.local/state` no sirven).

### Los tres detalles del script

Del debugging del script de Ghidra (`src/ghidra_scripts/Decomp.java`):

1. La cabecera `/* ### ... ### */` **no es decorativa**: el proveedor de
   scripts Java la usa para armar el bundle de la clase. Sin ella, el
   headless aborta con `ClassNotFoundException`.
2. El archivo tiene que estar en **ASCII puro**, y sin comentarios anidados:
   un `/*` dentro del bloque de la cabecera lo cierra antes de tiempo.
3. `-process` no acepta la ruta del archivo sino el nombre **dentro** del
   proyecto.

## El modo vivo

`--mode=live` speakaría con la GUI por `$XDG_RUNTIME_DIR/hikaru/hikaru.sock`.
**La GUI todavía no implementa ese socket** (`crates/hikaru_mcp_port/src/ipc.rs`
es un esqueleto), así que el modo vivo hoy arranca y avisa: el estado sigue
viniendo del disco.

## Configuración del DAW

El archivo `~/.config/hikaru/mcp-ports.json` tiene los slots. El campo
`server_flags` es la línea de comandos con la que se lanza este servidor:

```json
"server_flags": {
  "executable": "/home/alex/Documentos/Codigo_Fuente/hikaru_mcp",
  "args": ["--mode", "files"],
  "env": { "GHIDRA_INSTALL_DIR": "..." },
  "working_dir": "/home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive"
}
```

## Licencia

AGPLv3, como el resto del DAW. Los formatos de archivo que manipula
(`.oplf`/`.opsf`) son LGPLv3 y viven en crates aparte.

[Model Context Protocol]: https://modelcontextprotocol.io
