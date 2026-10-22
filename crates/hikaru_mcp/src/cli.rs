// crates/hikaru_mcp/src/cli.rs
// SPDX-License-Identifier: AGPL-3.0-or-later

//! La línea de comandos del servidor MCP.
//!
//! El servidor se levanta como un proceso que habla MCP por stdin/stdout
//! (lo que launchean OpenCode y Claude Code) o por un socket de dominio Unix
//! (lo que el DAW publica para los slots que habilitó el usuario).

use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use crate::Mode;

/// El servidor MCP de Hikaru OpenLive.
#[derive(Debug, Clone, Parser)]
#[command(
    name = "hikaru_mcp",
    version,
    about = "Servidor MCP (Model Context Protocol) de Hikaru OpenLive",
    long_about = "Expone el DAW a los agentes de IA por MCP: el proyecto, la matriz de \
clips, los plugins VST3/CLAP del sistema y la ingeniería inversa de plugins \
cerrados.\n\n\
Por omisión habla JSON-RPC por stdin/stdout, que es lo que usan OpenCode y \
Claude Code. Con --transport unix publica un socket de dominio Unix."
)]
pub struct Cli {
    /// El modo de operación.
    #[arg(long, value_enum, default_value_t = ModeArg::Files)]
    pub mode: ModeArg,

    /// El transporte con el que habla MCP.
    #[arg(long, value_enum, default_value_t = TransportArg::Stdio)]
    pub transport: TransportArg,

    /// El proyecto a cargar (`.oplf` o `.opsf`).
    #[arg(long, value_name = "ARCHIVO")]
    pub project: Option<PathBuf>,

    /// El socket donde escuchar, con `--transport unix`.
    #[arg(long, value_name = "RUTA")]
    pub socket: Option<PathBuf>,

    /// El slot de puerto MCP que se publica, según `mcp-ports.json`.
    ///
    /// El número del slot (1..N). El socket sale de la configuración; si no
    /// está, se usa el que viene con `--socket`.
    #[arg(long, value_name = "N")]
    pub port: Option<u16>,

    /// Imprime el estado del servidor y sale, sin hablar MCP.
    #[arg(long)]
    pub status: bool,
}

/// El modo de operación, como lo ve la línea de comandos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ModeArg {
    /// Sólo archivos: lee proyectos del disco. No necesita la GUI.
    Files,
    /// Vivo: además de los archivos, habla con la GUI por su socket IPC.
    Live,
}

impl From<ModeArg> for Mode {
    fn from(arg: ModeArg) -> Self {
        match arg {
            ModeArg::Files => Mode::Files,
            ModeArg::Live => Mode::Live,
        }
    }
}

/// El transporte, como lo ve la línea de comandos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TransportArg {
    /// JSON-RPC por la entrada y la salida estándar del proceso.
    Stdio,
    /// Un socket de dominio Unix.
    Unix,
}

/// La variable de entorno que dice qué proyecto cargar.
pub const PROJECT_ENV: &str = "HIKARU_PROJECT";

/// La variable de entorno que dice dónde está el socket de la GUI.
pub const GUI_SOCKET_ENV: &str = "HIKARU_GUI_SOCKET";

/// El directorio de runtime donde viven los sockets.
pub fn runtime_dir() -> PathBuf {
    match std::env::var("XDG_RUNTIME_DIR") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir).join("hikaru"),
        _ => PathBuf::from("/tmp/hikaru"),
    }
}

/// El socket de la GUI, en modo vivo.
///
/// La GUI lo publica en `$XDG_RUNTIME_DIR/hikaru/hikaru.sock`. Todavía no
/// lo implementa (el `ipc.rs` de `hikaru_mcp_port` es un esqueleto), así que
/// el modo vivo arranca y avisa en vez de fallar.
pub fn gui_socket() -> PathBuf {
    match std::env::var(GUI_SOCKET_ENV) {
        Ok(ruta) if !ruta.is_empty() => PathBuf::from(ruta),
        _ => runtime_dir().join("hikaru.sock"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn la_definicion_de_la_cli_es_valida() {
        Cli::command().debug_assert();
    }

    #[test]
    fn el_modo_por_omision_es_archivos() {
        let cli = Cli::parse_from(["hikaru_mcp"]);
        assert_eq!(cli.mode, ModeArg::Files);
        assert_eq!(cli.transport, TransportArg::Stdio);
    }

    #[test]
    fn el_socket_por_omision_vive_en_el_runtime_dir() {
        let ruta = runtime_dir().join("hikaru-mcp-1.sock");
        assert!(ruta.to_string_lossy().contains("hikaru"));
    }
}
