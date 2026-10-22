// crates/hikaru_mcp/src/main.rs
// SPDX-License-Identifier: AGPL-3.0-or-later

//! El binario del servidor MCP de Hikaru OpenLive.
//!
//! Arranca el servidor y lo deja hablando MCP. En modo `stdio` (el que usan
//! OpenCode y Claude Code) habla JSON-RPC por la entrada y la salida estándar
//! del proceso; en modo `unix` publica un socket de dominio Unix.
//!
//! **Regla de oro del modo stdio**: nada puede escribir en la salida
//! estándar, porque ahí va el protocolo. Todo el diagnóstico va al error
//! estándar.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;
use rmcp::transport::async_rw::AsyncRwTransport;
use rmcp::service::ServiceExt;

use hikaru_mcp::cli::{gui_socket, Cli, TransportArg};
use hikaru_mcp::server::HikaruServer;
use hikaru_mcp::{Backend, Mode};

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    // El proyecto: el que se pasó por línea de comandos, o el de la variable
    // de entorno, o el que se encuentre en el directorio actual.
    let ruta_proyecto = match resolver_proyecto(cli.project.as_deref()) {
        Ok(ruta) => ruta,
        Err(err) => {
            eprintln!("[Hikaru MCP] {err}");
            return ExitCode::from(1);
        }
    };

    let modo: Mode = cli.mode.into();
    let backend = Arc::new(Backend::new(modo, Some(gui_socket())));

    // El proyecto se carga acá y no al vuelo: si el archivo está roto, el
    // agente tiene que saberlo al arrancar, no en la primera tool call.
    if let Some(ruta) = &ruta_proyecto {
        match hikaru_mcp::project::load(ruta) {
            Ok(proyecto) => {
                eprintln!(
                    "[Hikaru MCP] proyecto cargado: {} ({})",
                    ruta.display(),
                    proyecto.format()
                );
                if let Ok(mut guard) = backend.project.lock() {
                    *guard = Some(proyecto);
                }
            }
            Err(err) => {
                eprintln!("[Hikaru MCP] {err}");
                return ExitCode::from(1);
            }
        }
    }

    if modo == Mode::Live {
        eprintln!(
            "[Hikaru MCP] modo vivo: el socket IPC de la GUI ({}) todavía no lo implementa \
             la ventana, así que el estado viene del disco.",
            gui_socket().display()
        );
    }

    if cli.status {
        return estado(&backend, &ruta_proyecto);
    }

    if cli.cli {
        // La consola interactiva: banner, ayuda y un bucle que lee comandos.
        // Sirve para probar la ingeniería inversa sin levantar un agente.
        let consola = hikaru_mcp::console::Console::new(backend);
        consola.run();
        return ExitCode::SUCCESS;
    }

    match cli.transport {
        TransportArg::Stdio => servir_stdio(backend).await,
        TransportArg::Unix => {
            let Some(socket) = resolver_socket(&cli) else {
                eprintln!(
                    "[Hikaru MCP] falta el socket: pasá --socket <ruta> o --port <N> \
                     con un slot habilitado en la configuración de puertos."
                );
                return ExitCode::from(2);
            };
            servir_unix(backend, socket).await
        }
    }
}

/// El socket donde escuchar en modo Unix.
///
/// Tiene dos fuentes: el `--socket` explícito, que manda sobre todo, y el
/// slot de la configuración de puertos MCP (el mismo archivo que edita la
/// ventana de ajustes del DAW).
fn resolver_socket(cli: &Cli) -> Option<PathBuf> {
    if let Some(socket) = &cli.socket {
        return Some(socket.clone());
    }

    let port = cli.port?;
    let config = hikaru_mcp_port::load_config(&hikaru_mcp_port::default_config_path()).ok()?;
    let slot = config.slot(port)?;

    match &slot.transport {
        hikaru_mcp_port::McpTransport::UnixSocket { path } => Some(path.clone()),
        _ => None,
    }
}

/// El proyecto a cargar: el de la línea de comandos, el de la variable de
/// entorno, o el primero que haya en el directorio actual.
fn resolver_proyecto(explicito: Option<&Path>) -> Result<Option<PathBuf>, String> {
    if let Some(ruta) = explicito {
        return Ok(Some(ruta.to_path_buf()));
    }

    if let Ok(ruta) = std::env::var(hikaru_mcp::cli::PROJECT_ENV) {
        if !ruta.is_empty() {
            return Ok(Some(PathBuf::from(ruta)));
        }
    }

    let actual = std::env::current_dir()
        .map_err(|err| format!("no se pudo leer el directorio actual: {err}"))?;
    Ok(hikaru_mcp::project::discover_in(&actual))
}

/// El modo stdio: el proceso ES el canal.
///
/// El servidor se lanza, habla MCP por stdin/stdout y termina cuando el
/// cliente cierra la conexión.
///
/// OJO: `serve()` devuelve el *handle* del servicio, no espera a que termine.
/// Sin `waiting()`, el proceso se iría derecho después de arrancar y el
/// cliente se encontraría con el socket cerrado.
async fn servir_stdio(backend: Arc<Backend>) -> ExitCode {
    let servicio = HikaruServer::new(backend);

    let (lectura, escritura) = rmcp::transport::stdio();

    match servicio.serve((lectura, escritura)).await {
        Ok(servidor) => {
            let _ = servidor.waiting().await;
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("[Hikaru MCP] el servidor MCP terminó con error: {err}");
            ExitCode::from(1)
        }
    }
}

/// El modo Unix: el servidor publica un socket y atiende a los que se
/// conecten.
///
/// Cada conexión es un cliente MCP con su propia sesión. El límite de
/// clientes sale de la configuración del slot.
async fn servir_unix(backend: Arc<Backend>, socket: PathBuf) -> ExitCode {
    // El socket tiene que estar en un directorio que exista.
    if let Some(dir) = socket.parent() {
        if let Err(err) = std::fs::create_dir_all(dir) {
            eprintln!(
                "[Hikaru MCP] no se pudo crear el directorio del socket {}: {err}",
                dir.display()
            );
            return ExitCode::from(1);
        }
    }

    // Un socket viejo de una sesión anterior haría que el bind falle.
    if socket.exists() {
        let _ = std::fs::remove_file(&socket);
    }

    let listener = match tokio::net::UnixListener::bind(&socket) {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!(
                "[Hikaru MCP] no se pudo abrir el socket {}: {err}",
                socket.display()
            );
            return ExitCode::from(1);
        }
    };

    eprintln!(
        "[Hikaru MCP] escuchando en {}",
        socket.display()
    );

    loop {
        let (stream, _) = match listener.accept().await {
            Ok(par) => par,
            Err(err) => {
                eprintln!("[Hikaru MCP] error aceptando una conexión: {err}");
                continue;
            }
        };

        let (lectura, escritura) = tokio::io::split(stream);
        let transporte = AsyncRwTransport::new_server(lectura, escritura);
        let servicio = HikaruServer::new(Arc::clone(&backend));

        // Cada cliente se atiende en su propia tarea: uno que se cuelga no
        // puede bloquear a los demás.
        tokio::spawn(async move {
            match servicio.serve(transporte).await {
                Ok(servidor) => {
                    // `serve` devuelve el handle: `waiting()` es lo que
                    // espera a que la sesión termine.
                    let _ = servidor.waiting().await;
                    eprintln!("[Hikaru MCP] cliente desconectado");
                }
                Err(err) => eprintln!("[Hikaru MCP] la sesión del cliente terminó con error: {err}"),
            }
        });
    }
}

/// El estado del servidor, para depurar sin levantar el protocolo.
fn estado(backend: &Arc<Backend>, ruta_proyecto: &Option<PathBuf>) -> ExitCode {
    let informe = backend.scan(false);

    let mut resumen = serde_json::json!({
        "modo": backend.mode.label(),
        "proyecto": ruta_proyecto.as_ref().map(|p| p.display().to_string()),
        "socket_gui": backend.gui_socket.as_ref().map(|p| p.display().to_string()),
        "plugins_encontrados": informe.plugin_count(),
        "errores_de_escaneo": informe.errors.len(),
        "rea": hikaru_mcp::rea::locate().map(|p| p.display().to_string()),
        "ghidra": hikaru_mcp::ghidra::resumen(),
        "directorio_de_crashes": hikaru_mcp::crash::crash_dir().display().to_string(),
    });

    if let Some(ruta) = &ruta_proyecto {
        let existe = ruta.exists();
        resumen["proyecto_existe"] = serde_json::json!(existe);
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&resumen).unwrap_or_default()
    );
    ExitCode::SUCCESS
}
