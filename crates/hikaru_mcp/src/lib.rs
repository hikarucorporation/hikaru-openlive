// crates/hikaru_mcp/src/lib.rs
// SPDX-License-Identifier: AGPL-3.0-or-later

/*!
 # Hikaru MCP Server

 El **servidor MCP** de Hikaru OpenLive: un binario standalone que habla
 [Model Context Protocol] con los agentes de IA (OpenCode, Claude Code, REA)
 y les da acceso a lo que el DAW sabe.

 Es el lado que falta de `hikaru_mcp_port`: ese crate tiene la *configuración*
 de los slots y el esqueleto del transporte, pero ningún proceso que hable
 JSON-RPC. Este crate es ese proceso.

 ## Qué expone

 **Recursos** (lectura del estado):
 - `hikaru://project/current` — el proyecto cargado: BPM, pistas, clips, escenas.
 - `hikaru://matrix/grid` — la matriz de clips de la vista Live/Session.
 - `hikaru://plugins/discovered` — los plugins VST3/CLAP del sistema.
 - `hikaru://plugins/telemetry` — la telemetría viva de los plugins.
 - `hikaru://diagnostics/last-crash` — el último fallo del host de plugins.

 **Herramientas** (acciones):
 - `list_plugins` — qué plugins hay instalados y dónde.
 - `plugin_parameters` — los parámetros de un plugin, con la instancia real.
 - `set_parameter` — mover un parámetro de un plugin.
 - `inspect_plugin_wrapper` — la superficie nativa del editor de un plugin.
 - `decompile_plugin_stub` — ingeniería inversa de un plugin cerrado vía REA.
 - `plugin_symbols` — símbolos y secciones del binario (sin REA, sin Ghidra).
 - `audit_project` — recursos faltantes, plugins descolgados, ruteo.

 **Prompts**: `diagnosticar-fallo-audio`, `reconstruir-plugin-rust`,
 `auditar-proyecto`.

 ## Los dos modos

 - **Archivo (`--mode=files`, por omisión)**: lee y valida proyectos `.oplf` y
   `.opsf` desde disco con los crates LGPLv3. No necesita la GUI, ni el motor
   de audio, ni una placa de sonido. Es el modo que sirve en un servidor.
 - **Vivo (`--mode=live`)**: habla con la GUI por el socket IPC de
   `$XDG_RUNTIME_DIR/hikaru/hikaru.sock`. Cuando la GUI no está corriendo, el
   servidor lo dice con claridad en vez de fallar en silencio.

 ## Por qué un binario aparte

 La GUI no puede linkearse en el binario del agente: arrastraría el stack de
 render, el backend de audio y la ventana. Este crate sólo depende de los
 formatos de archivo, del host de plugins y del SDK de MCP.

 [Model Context Protocol]: https://modelcontextprotocol.io
*/

use std::path::PathBuf;

use serde_json::Value;

/// En qué modo corre el servidor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Sólo archivos: lee proyectos del disco. No necesita la GUI.
    Files,
    /// Vivo: además de los archivos, habla con la GUI por el socket IPC.
    Live,
}

impl Mode {
    /// El nombre que se muestra en las herramientas.
    pub fn label(self) -> &'static str {
        match self {
            Mode::Files => "files",
            Mode::Live => "live",
        }
    }
}

/// El estado compartido por todos los handlers.
///
/// Va detrás de un `Arc` porque el servidor MCP atiende varias conexiones a
/// la vez (o una, en el caso de un slot con `max_clients = 1`) y todas ven el
/// mismo proyecto y la misma telemetría.
#[derive(Debug)]
pub struct Backend {
    /// El modo con el que arrancó el servidor.
    pub mode: Mode,
    /// El proyecto cargado, si hay alguno.
    pub project: std::sync::Mutex<Option<project::LoadedProject>>,
    /// La telemetría de los plugins (CPU, xruns, editor).
    pub telemetry: std::sync::Mutex<hikaru_mcp_port::TelemetryExporter>,
    /// El escaneo de plugins, cacheado: mirar `/usr/lib` tarda.
    pub scan: std::sync::Mutex<Option<hikaru_plugin_host::registry::ScanReport>>,
    /// El socket de la GUI, en modo vivo.
    pub gui_socket: Option<PathBuf>,
}

impl Backend {
    /// Un backend nuevo, sin proyecto cargado.
    pub fn new(mode: Mode, gui_socket: Option<PathBuf>) -> Self {
        Self {
            mode,
            project: std::sync::Mutex::new(None),
            telemetry: std::sync::Mutex::new(hikaru_mcp_port::TelemetryExporter::default()),
            scan: std::sync::Mutex::new(None),
            gui_socket,
        }
    }

    /// El escaneo de plugins.
    ///
    /// Se cachea porque el escaneo recorre `/usr/lib/vst3` y `/usr/lib/clap`,
    /// y eso tarda lo suficiente como para que el agente lo note si se repite
    /// en cada llamada. `refrescar` lo fuerza a rehacerlo.
    pub fn scan(&self, refrescar: bool) -> hikaru_plugin_host::registry::ScanReport {
        if !refrescar {
            if let Ok(cache) = self.scan.lock() {
                if let Some(informe) = cache.as_ref() {
                    return informe.clone();
                }
            }
        }

        let informe = plugins::scan();
        if let Ok(mut cache) = self.scan.lock() {
            *cache = Some(informe.clone());
        }
        informe
    }

    /// Un resumen del estado del servidor, para las respuestas de error.
    pub fn describe(&self) -> Value {
        let has_project = self.project.lock().map(|p| p.is_some()).unwrap_or(false);
        serde_json::json!({
            "mode": self.mode.label(),
            "proyecto_cargado": has_project,
            "socket_gui": self.gui_socket.as_ref().map(|p| p.display().to_string()),
        })
    }
}

/// Convierte un error en la respuesta JSON-RPC de error correspondiente.
///
/// Los errores de una herramienta son de dos tipos y la diferencia importa:
/// un error de protocolo (`Err`) se ve opaco en el cliente, mientras que un
/// error de herramienta (`Ok` con `is_error`) muestra el mensaje al usuario.
/// Casi todo lo que falla acá —un path que no existe, un plugin que no
/// carga— es un error de herramienta, no de protocolo.
pub fn tool_error(message: impl Into<String>) -> rmcp::model::CallToolResult {
    rmcp::model::CallToolResult::error(vec![rmcp::model::ContentBlock::text(message.into())])
}

/// El resultado de una herramienta que salió bien.
pub fn tool_ok(value: &Value) -> rmcp::model::CallToolResult {
    let texto = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    rmcp::model::CallToolResult::success(vec![rmcp::model::ContentBlock::text(texto)])
}

pub mod catalog;
pub mod cli;
pub mod console;
pub mod man;
pub mod crash;
pub mod ghidra;
pub mod plugins;
pub mod project;
pub mod rea;
pub mod schema;
pub mod server;
