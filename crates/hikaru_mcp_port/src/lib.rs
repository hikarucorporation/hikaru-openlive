// crates/hikaru_mcp_port/src/lib.rs

/*!
 * # Hikaru MCP Port
 *
 * La frontera que expone el DAW **Hikaru OpenLive** a los agentes de
 * inteligencia artificial (OpenCode, Claude Code, REA) por el estándar
 * abierto **Model Context Protocol**.
 *
 * ## Qué hay en este crate
 *
 * - [`config`]: la configuración del servidor MCP y de los *slots* de puerto
 *   (MCP Port 1, MCP Port 2, ... hasta los que hagan falta).
 * - [`ipc`]: el esqueleto del transporte por socket de dominio Unix, con las
 *   reglas de los slots ya fijadas (deshabilitado = no conecta, lleno = no
 *   entra un cliente más).
 * - [`telemetry`]: el buffer de telemetría de plugins en tiempo real, que es
 *   lo que el agente consume para auditar un fallo de audio o para
 *   ingeniería inversa de un plugin que se dibuja mal.
 *
 * ## Por qué un crate aparte
 *
 * El servidor MCP habla con la GUI y con el motor de audio, y tiene que
 * poder vivir en un binario standalone (el `hikaru_mcp` de la especificación)
 * sin arrastrar a GPUI. Si esta estructura viviera adentro de `hikaru_gui`,
 * el binario del agente tendría que linkear la ventana entera.
 *
 * ## Licencia
 *
 * AGPLv3, como el resto del DAW: el modo vivo (el que controla el motor)
 * es AGPL. El modo archivos, que sólo lee y escribe los crates LGPL de
 * proyecto, se puede linkear sin problemas.
 */

pub mod config;
pub mod ipc;
pub mod telemetry;

pub use config::{
    default_config_path, load_config, save_config, socket_path_in, ConfigError, ConfigIssue,
    McpPortConfig, McpServerConfig, McpTransport, PortSlotId, ServerFlags, DEFAULT_PORT_SLOTS,
    FIRST_PORT_SLOT,
};
pub use ipc::{IpcDirection, IpcError, IpcEvent, IpcStub};
pub use telemetry::{PluginTelemetry, TelemetryExporter};
