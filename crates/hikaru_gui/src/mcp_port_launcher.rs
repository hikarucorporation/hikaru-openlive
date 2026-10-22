// crates/hikaru_gui/src/mcp_port_launcher.rs
// SPDX-License-Identifier: AGPL-3.0-or-later

//! El lanzamiento de los servidores MCP por los slots habilitados.
//!
//! # Por qué este módulo existe
//!
//! La configuración de los puertos MCP (`~/.config/hikaru/mcp-ports.json`)
//! es un archivo que el usuario edita con la ventana de ajustes del DAW. Pero
//! alguien tiene que *ejecutar* lo que esa configuración pide: por cada slot
//! habilitado con transporte Unix, el DAW tiene que lanzar un proceso
//! `hikaru_mcp --transport unix --socket <ruta>` para que el agente se
//! conecte.
//!
//! Sin este módulo, el archivo se edita, la UI dice "habilitado" y nada
//! escucha: es el bug que hacía que `man hikaru_mcp` fuera la única forma de
//! saber si el servidor existía.
//!
//! # Por qué se lanza un proceso y no se habla en el mismo
//!
//! El servidor MCP es un binario aparte a propósito: si viviera adentro de la
//! GUI, el agente heredaría el stack de render, el backend de audio y la
//! ventana. Lanzar un proceso hijo mantiene la frontera de licencia y de
//! dependencias (la GUI es AGPL, el modo archivos del servidor es LGPL).
//!
//! # Lo que falta (modo vivo)
//!
//! El modo `live` (el que habla con el motor de audio por `hikaru.sock`)
//! todavía no tiene el protocolo IPC implementado: la GUI no publica
//! `hikaru.sock`. Por eso los slots se lanzan en modo `files` por omisión;
//! cuando el IPC exista, se pasará `--mode live`.

use std::process::{Child, Command, Stdio};

use hikaru_mcp_port::{load_config, default_config_path, McpTransport};

/// Los procesos hijos que el DAW lanzó, para que no se los mate el drop.
///
/// Se guardan acá y se olvidan: el proceso hijo sobrevive al `Child` de Rust
/// (Rust no lo mata al soltarlo). Cuando el DAW cierra, el sistema los
/// reaperá.
static PROCESOS_LANZADOS: std::sync::Mutex<Vec<Child>> = std::sync::Mutex::new(Vec::new());

/// Lanza los servidores MCP que pide la configuración.
///
/// Se llama una sola vez, al arrancar la GUI, antes de abrir la ventana.
/// Los errores se loguean y no abortan el arranque: un slot mal configurado
/// no debe impedir que el DAW abra.
pub fn lanzar_slots_habilitados() {
    let path = default_config_path();

    let config = match load_config(&path) {
        Ok(config) => config,
        Err(err) => {
            eprintln!(
                "[Hikaru MCP Port] no se pudo leer la configuración de los puertos ({}): {}",
                path.display(),
                err
            );
            return;
        }
    };

    if !config.enabled {
        eprintln!("[Hikaru MCP Port] el servidor MCP está deshabilitado en la configuración");
        return;
    }

    for slot in config.enabled_ports() {
        let (socket, max_clientes) = match &slot.transport {
            McpTransport::UnixSocket { path } => (path.clone(), slot.max_clients),
            _ => continue, // stdio y server-flags no se lanzan acá
        };

        // El ejecutable sale de server_flags (la misma línea de comandos que
        // edita el usuario).
        let comando = &config.server_flags;

        // El socket tiene que vivir en un directorio que exista.
        if let Some(dir) = socket.parent() {
            if let Err(err) = std::fs::create_dir_all(dir) {
                eprintln!(
                    "[Hikaru MCP Port] no se pudo crear el directorio del socket {}: {}",
                    dir.display(),
                    err
                );
                continue;
            }
        }

        // Un socket viejo de una sesión anterior haría que el bind falle.
        if socket.exists() {
            let _ = std::fs::remove_file(&socket);
        }

        let mut proceso = Command::new(&comando.executable);
        proceso
            .args(&comando.args)
            .arg("--transport")
            .arg("unix")
            .arg("--socket")
            .arg(&socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        for (clave, valor) in &comando.env {
            proceso.env(clave, valor);
        }

        if let Some(dir) = &comando.working_dir {
            proceso.current_dir(dir);
        }

        match proceso.spawn() {
            Ok(hijo) => {
                eprintln!(
                    "[Hikaru MCP Port] slot {} lanzado en {} (max {} clientes)",
                    slot.id,
                    socket.display(),
                    max_clientes
                );
                if let Ok(mut guard) = PROCESOS_LANZADOS.lock() {
                    guard.push(hijo);
                }
            }
            Err(err) => {
                eprintln!(
                    "[Hikaru MCP Port] no se pudo lanzar el slot {}: {}",
                    slot.id,
                    err
                );
            }
        }
    }
}

/// Cuántos servidores lanzó el DAW.
pub fn lanzados() -> usize {
    PROCESOS_LANZADOS.lock().map(|g| g.len()).unwrap_or(0)
}
