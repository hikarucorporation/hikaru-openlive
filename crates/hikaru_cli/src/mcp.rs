// La administración de los puertos MCP desde la terminal.
// SPDX-License-Identifier: AGPL-3.0-or-later
// crates/hikaru_cli/src/mcp.rs
//
// =========================================================================
// QUÉ SON LOS "PUERTOS MCP"
// =========================================================================
//
// El DAW expone muchas superficies (la matriz, el rack, los plugins) y cada
// agente de IA que se conecta quiere una SESIÓN propia. Por eso el puerto no
// es un número de red sino un SLOT numerado: "MCP Port 1", "MCP Port 2", ...
// (ver `hikaru_mcp_port::config`).
//
// La consola administra esos slots: cuáles existen, habilitar y deshabilitar,
// agregar y quitar, y cambiar el transporte. La configuración se guarda en un
// archivo JSON que la app puede leer (y que un humano puede editar a mano
// cuando algo sale mal).
//
// =========================================================================
// POR QUÉ ESTE MÓDULO NO HABLA CON EL DAW
// =========================================================================
//
// La configuración de los puertos es un ARCHIVO. El DAW la levanta cuando
// arranca y la usa mientras corre. Esta consola edita el archivo: no abre
// sockets ni manda JSON-RPC. Lo que la app hace con la configuración en
// caliente (aceptar conexiones, reconectar) es cosa del servidor MCP, que
// todavía no está implementado (ver `hikaru_mcp_port::ipc`).

use std::path::PathBuf;
use std::process::ExitCode;

use colored::Colorize;
use hikaru_mcp_port::{
    default_config_path, load_config, save_config, McpServerConfig, McpTransport, PortSlotId,
};
use serde_json::{json, Value};

use crate::cli::{McpAction, McpArgs, TransportKind};

/// La variable de entorno que dice dónde vive el archivo de configuración.
///
/// Sirve para dos cosas: para los tests (que escriben en un directorio
/// temporal y no tocan la configuración real del usuario) y para un agente
/// que quiere tener una configuración por proyecto.
pub const CONFIG_ENV: &str = "HIKARU_MCP_CONFIG";

/// La ruta del archivo de configuración.
pub fn config_path() -> PathBuf {
    match std::env::var(CONFIG_ENV) {
        Ok(path) if !path.is_empty() => PathBuf::from(path),
        _ => default_config_path(),
    }
}

/// Ejecuta la acción de puertos MCP que se pidió.
pub fn run(args: &McpArgs, json: bool) -> ExitCode {
    let path = config_path();

    let mut config = match load_config(&path) {
        Ok(config) => config,
        Err(err) => {
            eprintln!(
                "[Hikaru] no se pudo leer la configuración de los puertos MCP ({}): {}",
                path.display(),
                err
            );
            return ExitCode::from(crate::EXIT_FAILURE);
        }
    };

    let Some(action) = args.action.as_ref() else {
        // Sin acción es lo mismo que `list`: es lo que el usuario espera
        // cuando escribe `hikaru_openlive --mcp`.
        return list(&config, &path, json);
    };

    match action {
        McpAction::List => list(&config, &path, json),
        McpAction::Status { port } => status(&config, &path, *port, json),
        McpAction::Enable { port } => set_enabled(&mut config, &path, *port, true, json),
        McpAction::Disable { port } => set_enabled(&mut config, &path, *port, false, json),
        McpAction::Add => add(&mut config, &path, json),
        McpAction::Remove { port } => remove(&mut config, &path, *port, json),
        McpAction::Transport { port, transport } => {
            set_transport(&mut config, &path, *port, *transport, json)
        }
    }
}

/// `mcp list`: la tabla de slots.
fn list(config: &McpServerConfig, path: &PathBuf, json: bool) -> ExitCode {
    if json {
        let payload = json!({
            "config": path.display().to_string(),
            "server": {
                "enabled": config.enabled,
                "runtime_dir": config.runtime_dir.display().to_string(),
                "server_flags": config.server_flags.command_line(),
                "port_count": config.port_count(),
            },
            "ports": config.ports.iter().map(port_json).collect::<Vec<Value>>(),
        });
        println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_default());
        return ExitCode::SUCCESS;
    }

    println!("[Hikaru MCP Ports]  ({})", path.display());
    println!(
        "  Servidor: {} | Runtime dir: {}",
        if config.enabled {
            "habilitado".green().to_string()
        } else {
            "detenido".red().to_string()
        },
        config.runtime_dir.display()
    );
    println!(
        "  Flags del servidor externo: {}",
        config.server_flags.command_line()
    );
    println!();

    if config.ports.is_empty() {
        println!("  (no hay slots: agregá uno con `hikaru_openlive --mcp add`)");
        return ExitCode::SUCCESS;
    }

    println!(
        "  {:<4}  {:<9}  {:<14}  {:<6}  {}",
        "ID", "ESTADO", "TRANSPORTE", "MAX", "SOCKET / DESTINO"
    );
    for port in &config.ports {
        let estado = if port.enabled {
            "habilitado".green().to_string()
        } else {
            "deshabilitado".dimmed().to_string()
        };
        let destino = match &port.transport {
            McpTransport::UnixSocket { path } => path.display().to_string(),
            McpTransport::Stdio => "stdin / stdout (JSON-RPC 2.0)".to_string(),
            McpTransport::ServerFlags(_) => config.server_flags.command_line(),
        };
        println!(
            "  {:<4}  {:<9}  {:<14}  {:<6}  {}",
            port.id, estado, port.transport.label(), port.max_clients, destino
        );
    }

    ExitCode::SUCCESS
}

/// `mcp status`: el detalle de un slot o de toda la configuración.
fn status(
    config: &McpServerConfig,
    path: &PathBuf,
    port: Option<u16>,
    json: bool,
) -> ExitCode {
    match port {
        Some(id) => {
            let Some(slot) = config.slot(id) else {
                eprintln!("[Hikaru] no existe el puerto MCP {}.", id);
                return ExitCode::from(crate::EXIT_USAGE);
            };
            if json {
                let payload = json!({
                    "config": path.display().to_string(),
                    "port": port_json(slot),
                });
                println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_default());
            } else {
                println!("[Hikaru MCP Port {}]  ({})", slot.id, path.display());
                println!("  Etiqueta:   {}", slot.label);
                println!(
                    "  Estado:     {}",
                    if slot.enabled { "habilitado" } else { "deshabilitado" }
                );
                println!("  Transporte: {}", slot.transport.label());
                println!("  Max clientes: {}", slot.max_clients);
                match &slot.transport {
                    McpTransport::UnixSocket { path } => {
                        println!("  Socket:     {}", path.display());
                        println!(
                            "  Existe:     {}",
                            if path.exists() { "sí" } else { "no (el DAW lo crea al arrancar)" }
                        );
                    }
                    McpTransport::Stdio => {
                        println!("  Canal:      stdin / stdout (JSON-RPC 2.0)");
                    }
                    McpTransport::ServerFlags(_) => {
                        println!("  Comando:    {}", config.server_flags.command_line());
                    }
                }
            }
            ExitCode::SUCCESS
        }
        None => {
            // Sin número: la configuración completa, con los problemas que
            // tenga. Los problemas se muestran, no se corrigen solos: un slot
            // con el id duplicado tiene que verse.
            let issues = config.validate();
            if json {
                let payload = json!({
                    "config": path.display().to_string(),
                    "server": {
                        "enabled": config.enabled,
                        "runtime_dir": config.runtime_dir.display().to_string(),
                        "server_flags": config.server_flags.command_line(),
                        "port_count": config.port_count(),
                    },
                    "ports": config.ports.iter().map(port_json).collect::<Vec<Value>>(),
                    "issues": issues.iter().map(|i| i.message().to_string()).collect::<Vec<String>>(),
                });
                println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_default());
            } else {
                println!("[Hikaru MCP Server]  ({})", path.display());
                println!("  Habilitado: {}", config.enabled);
                println!("  Runtime dir: {}", config.runtime_dir.display());
                println!("  Slots:       {}", config.port_count());
                println!("  Flags:       {}", config.server_flags.command_line());
                if issues.is_empty() {
                    println!("  Problemas:   ninguno");
                } else {
                    println!("  Problemas:");
                    for issue in &issues {
                        println!("    - {}", issue.message());
                    }
                }
            }
            ExitCode::SUCCESS
        }
    }
}

/// `mcp enable` / `mcp disable`.
fn set_enabled(
    config: &mut McpServerConfig,
    path: &PathBuf,
    port: u16,
    enabled: bool,
    json: bool,
) -> ExitCode {
    let Some(slot) = config.slot_mut(port) else {
        eprintln!("[Hikaru] no existe el puerto MCP {}.", port);
        return ExitCode::from(crate::EXIT_USAGE);
    };
    slot.enabled = enabled;

    if let Err(err) = save_config(config, path) {
        eprintln!("[Hikaru] no se pudo guardar la configuración: {}", err);
        return ExitCode::from(crate::EXIT_FAILURE);
    }

    if json {
        let payload = json!({
            "config": path.display().to_string(),
            "port": port_json(config.slot(port).expect("el slot existe")),
        });
        println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_default());
    } else {
        println!(
            "[Hikaru] Puerto MCP {} {}.",
            port,
            if enabled { "habilitado" } else { "deshabilitado" }
        );
        println!("  Configuración: {}", path.display());
    }
    ExitCode::SUCCESS
}

/// `mcp add`: agrega un slot al final de la lista.
fn add(config: &mut McpServerConfig, path: &PathBuf, json: bool) -> ExitCode {
    let id = config.add_port();

    if let Err(err) = save_config(config, path) {
        eprintln!("[Hikaru] no se pudo guardar la configuración: {}", err);
        return ExitCode::from(crate::EXIT_FAILURE);
    }

    if json {
        let payload = json!({
            "config": path.display().to_string(),
            "created": port_json(config.slot(id).expect("el slot existe")),
        });
        println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_default());
    } else {
        println!("[Hikaru] Puerto MCP {} creado.", id);
        println!("  Configuración: {}", path.display());
    }
    ExitCode::SUCCESS
}

/// `mcp remove`: borra un slot.
fn remove(config: &mut McpServerConfig, path: &PathBuf, port: u16, json: bool) -> ExitCode {
    if !config.remove_port(port) {
        eprintln!("[Hikaru] no existe el puerto MCP {}.", port);
        return ExitCode::from(crate::EXIT_USAGE);
    }

    if let Err(err) = save_config(config, path) {
        eprintln!("[Hikaru] no se pudo guardar la configuración: {}", err);
        return ExitCode::from(crate::EXIT_FAILURE);
    }

    if json {
        let payload = json!({
            "config": path.display().to_string(),
            "removed": port,
            "port_count": config.port_count(),
        });
        println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_default());
    } else {
        println!("[Hikaru] Puerto MCP {} eliminado.", port);
        println!("  Configuración: {}", path.display());
    }
    ExitCode::SUCCESS
}

/// `mcp transport`: cambia el transporte de un slot.
fn set_transport(
    config: &mut McpServerConfig,
    path: &PathBuf,
    port: u16,
    transport: TransportKind,
    json: bool,
) -> ExitCode {
    // La ruta del socket se calcula ANTES de tomar el slot: si el usuario
    // cambió el runtime dir, el slot tiene que apuntar al lugar nuevo, y
    // `socket_path_for` necesita la configuración por referencia inmutable
    // mientras el slot está prestado.
    let socket = config.socket_path_for(port);
    let server_flags = config.server_flags.clone();

    let Some(slot) = config.slot_mut(port) else {
        eprintln!("[Hikaru] no existe el puerto MCP {}.", port);
        return ExitCode::from(crate::EXIT_USAGE);
    };

    slot.transport = match transport {
        TransportKind::Unix => McpTransport::UnixSocket { path: socket },
        TransportKind::Stdio => McpTransport::Stdio,
        TransportKind::ServerFlags => McpTransport::ServerFlags(server_flags),
    };

    if let Err(err) = save_config(config, path) {
        eprintln!("[Hikaru] no se pudo guardar la configuración: {}", err);
        return ExitCode::from(crate::EXIT_FAILURE);
    }

    if json {
        let payload = json!({
            "config": path.display().to_string(),
            "port": port_json(config.slot(port).expect("el slot existe")),
        });
        println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_default());
    } else {
        println!(
            "[Hikaru] Puerto MCP {}: transporte {}.",
            port,
            transport.label()
        );
        println!("  Configuración: {}", path.display());
    }
    ExitCode::SUCCESS
}

/// Un slot, en JSON.
fn port_json(port: &hikaru_mcp_port::McpPortConfig) -> Value {
    let id: PortSlotId = port.id;
    json!({
        "id": id,
        "label": port.label,
        "enabled": port.enabled,
        "transport": port.transport.label(),
        "max_clients": port.max_clients,
        "socket_path": port.transport.socket_path().map(|p| p.display().to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Una configuración en un archivo temporal.
    fn config_temporal(nombre: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("hikaru_cli_mcp_{}_{}", std::process::id(), nombre));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn args(action: Option<McpAction>) -> McpArgs {
        McpArgs { action }
    }

    #[test]
    fn sin_accion_se_listan_los_puertos() {
        let path = config_temporal("listar.json");
        std::env::set_var(CONFIG_ENV, &path);

        let code = run(&args(None), true);
        assert_eq!(code, ExitCode::SUCCESS);

        std::env::remove_var(CONFIG_ENV);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn habilitar_un_puerto_se_persiste() {
        let path = config_temporal("habilitar.json");
        std::env::set_var(CONFIG_ENV, &path);

        assert_eq!(
            run(&args(Some(McpAction::Enable { port: 3 })), true),
            ExitCode::SUCCESS
        );

        let config = load_config(&path).expect("la configuración tiene que estar");
        assert!(config.slot(3).expect("slot 3").enabled);

        assert_eq!(
            run(&args(Some(McpAction::Disable { port: 3 })), true),
            ExitCode::SUCCESS
        );
        let config = load_config(&path).expect("la configuración tiene que estar");
        assert!(!config.slot(3).expect("slot 3").enabled);

        std::env::remove_var(CONFIG_ENV);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn agregar_un_puerto_crea_el_11() {
        let path = config_temporal("agregar.json");
        std::env::set_var(CONFIG_ENV, &path);

        assert_eq!(run(&args(Some(McpAction::Add)), true), ExitCode::SUCCESS);

        let config = load_config(&path).expect("la configuración tiene que estar");
        assert_eq!(config.port_count(), 11);
        assert!(config.slot(11).is_some());

        std::env::remove_var(CONFIG_ENV);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn un_puerto_que_no_existe_es_un_error_de_uso() {
        let path = config_temporal("inexistente.json");
        std::env::set_var(CONFIG_ENV, &path);

        assert_eq!(
            run(&args(Some(McpAction::Enable { port: 99 })), true),
            ExitCode::from(crate::EXIT_USAGE)
        );

        std::env::remove_var(CONFIG_ENV);
        let _ = std::fs::remove_file(&path);
    }
}
