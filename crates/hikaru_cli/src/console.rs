// La consola interactiva (`hikaru_openlive --cli`).
// SPDX-License-Identifier: AGPL-3.0-or-later
// crates/hikaru_cli/src/console.rs
//
// =========================================================================
// QUÉ ES ESTO
// =========================================================================
//
// Un REPL mínimo: el banner, la ayuda y un bucle que lee comandos de la
// entrada estándar y los ejecuta. Es el modo que usa el diseño de la
// especificación (docs/hikaru-openlive-cli/).
//
// =========================================================================
// POR QUÉ LEE DE LA ENTRADA ESTÁNDAR Y NO DE UNA TERMINAL CRUDO
// =========================================================================
//
// Leer de stdin tiene una propiedad que vale oro para los agentes de IA: el
// MISMO código sirve para una terminal humana y para un pipe. Un agente puede
// hacer:
//
//     printf 'new x.oplf\ninspect x.oplf\nexit\n' | hikaru_openlive --cli
//
// y obtener exactamente la misma salida que un humano que escribe los
// comandos a mano. Sin una dependencia de terminal cruda (que además no
// existe en Windows sin `winapi`), sin manejo de teclado crudo y con el EOF
// como salida natural: cuando stdin se cierra, la consola sale con código 0.
//
// =========================================================================
// LA GRAMÁTICA
// =========================================================================
//
// Los comandos de la consola son los MISMOS comandos de la línea de
// comandos: cada línea se parsea con el parser de `clap`, así que lo que
// funciona en la terminal funciona en la consola y al revés. No hay una
// segunda gramática que mantener.

use std::io::{BufRead, Write};
use std::process::ExitCode;

use clap::Parser;

use crate::cli::Cli;
use crate::Dispatch;

/// Ejecuta la consola interactiva.
pub fn run(json: bool) -> ExitCode {
    println!("{}", crate::banner::banner());
    println!();
    print_help();
    println!();
    println!("  Escribí `help` para la ayuda, `exit` para salir.");
    println!();

    let stdin = std::io::stdin();
    let mut line = String::new();

    loop {
        print!("hikaru> ");
        let _ = std::io::stdout().flush();

        line.clear();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => {
                // EOF: se terminó la entrada. Se sale limpio.
                println!();
                return ExitCode::SUCCESS;
            }
            Ok(_) => {}
            Err(err) => {
                eprintln!("[Hikaru] no se pudo leer la entrada: {}", err);
                return ExitCode::from(crate::EXIT_FAILURE);
            }
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        match run_line(trimmed, json) {
            LineResult::Continue => {}
            LineResult::Exit(code) => return code,
        }
    }
}

/// Qué pasó con una línea de la consola.
enum LineResult {
    /// Seguir leyendo.
    Continue,
    /// Salir de la consola con este código.
    Exit(ExitCode),
}

/// Ejecuta una línea de la consola.
///
/// La línea se parte por espacios y se parsea con el MISMO `Cli` que la
/// línea de comandos: `new x.oplf`, `inspect x.oplf`, `mcp enable 3`,
/// `validate x.opsf`, `help`, `exit`.
fn run_line(line: &str, json: bool) -> LineResult {
    let partes: Vec<String> = line.split_whitespace().map(|s| s.to_string()).collect();

    match partes.first().map(|s| s.as_str()) {
        Some("exit") | Some("quit") | Some("salir") => return LineResult::Exit(ExitCode::SUCCESS),
        Some("help") | Some("?") => {
            print_help();
            return LineResult::Continue;
        }
        _ => {}
    }

    // `Cli` con el primer argumento inventado: el parser no necesita el
    // nombre del binario para nada.
    let mut argv = vec!["hikaru_openlive".to_string()];
    argv.extend(partes);

    let cli = match Cli::try_parse_from(argv) {
        Ok(cli) => cli,
        Err(err) => {
            // En la consola un error no mata el proceso: se muestra y se
            // sigue. Un agente que se equivoca en una línea no pierde la
            // sesión entera.
            let _ = err.print();
            return LineResult::Continue;
        }
    };

    // La consola no se anida y no abre la ventana: las dos cosas se dicen y
    // se sigue.
    match &cli.command {
        Some(crate::Command::Cli) => {
            println!("[Hikaru] ya estás en la consola.");
            return LineResult::Continue;
        }
        Some(crate::Command::Gui) | None => {
            println!(
                "[Hikaru] La consola no abre la ventana: eso lo hace `hikaru_openlive --gui`."
            );
            return LineResult::Continue;
        }
        _ => {}
    }

    // El `--json` de la línea pisa al de la invocación: lo que se escribe en
    // la consola manda sobre lo con que se arrancó.
    let json = cli.json || json;

    // La línea se ejecuta por el MISMO camino que la línea de comandos: se
    // reconstruye el argv y se despacha. Así no hay dos implementaciones de
    // `new`/`inspect`/`validate` que puedan divergir.
    match crate::dispatch(cli_to_argv(&cli, json)) {
        // El comando ya corrió y su código de salida se imprimió con su
        // salida: un comando que falló no cierra la consola.
        Dispatch::Exit(_) | Dispatch::Gui(_) => LineResult::Continue,
    }
}

/// Reconstruye los argumentos de una línea ya parseada.
///
/// Es al revés de lo que hace `dispatch`: en vez de parsear, arma el argv
/// que la representaba. Sirve para que la consola y la línea de comandos
/// ejecuten EXACTAMENTE el mismo código.
fn cli_to_argv(cli: &Cli, json: bool) -> Vec<std::ffi::OsString> {
    let mut argv: Vec<std::ffi::OsString> = vec!["hikaru_openlive".into()];

    if json {
        argv.push("--json".into());
    }
    if cli.no_audio {
        argv.push("--no-audio".into());
    }
    if let Some(path) = &cli.project {
        argv.push("--project".into());
        argv.push(path.clone().into());
    }

    match &cli.command {
        Some(crate::Command::New(args)) => {
            argv.push("new".into());
            argv.push(args.path.clone().into());
            if args.empty {
                argv.push("--empty".into());
            }
            if let Some(title) = &args.title {
                argv.push("--title".into());
                argv.push(title.clone().into());
            }
            if args.force {
                argv.push("--force".into());
            }
        }
        Some(crate::Command::Inspect(args)) => {
            argv.push("inspect".into());
            argv.push(args.path.clone().into());
        }
        Some(crate::Command::Metadata(args)) => {
            argv.push("metadata".into());
            argv.push(args.path.clone().into());
        }
        Some(crate::Command::Validate(args)) => {
            argv.push("validate".into());
            argv.push(args.path.clone().into());
        }
        Some(crate::Command::Check(args)) => {
            argv.push("check".into());
            argv.push(args.path.clone().into());
        }
        Some(crate::Command::Mcp(args)) => {
            argv.push("mcp".into());
            match &args.action {
                Some(crate::McpAction::List) | None => argv.push("list".into()),
                Some(crate::McpAction::Status { port }) => {
                    argv.push("status".into());
                    if let Some(port) = port {
                        argv.push(port.to_string().into());
                    }
                }
                Some(crate::McpAction::Enable { port }) => {
                    argv.push("enable".into());
                    argv.push(port.to_string().into());
                }
                Some(crate::McpAction::Disable { port }) => {
                    argv.push("disable".into());
                    argv.push(port.to_string().into());
                }
                Some(crate::McpAction::Add) => argv.push("add".into()),
                Some(crate::McpAction::Remove { port }) => {
                    argv.push("remove".into());
                    argv.push(port.to_string().into());
                }
                Some(crate::McpAction::Transport { port, transport }) => {
                    argv.push("transport".into());
                    argv.push(port.to_string().into());
                    argv.push(match transport {
                        crate::TransportKind::Unix => "unix",
                        crate::TransportKind::Stdio => "stdio",
                        crate::TransportKind::ServerFlags => "server-flags",
                    }
                    .into());
                }
            }
        }
        Some(crate::Command::Cli) => argv.push("cli".into()),
        Some(crate::Command::Gui) | None => argv.push("gui".into()),
    }

    argv
}

/// La ayuda de la consola.
fn print_help() {
    println!("  Comandos de la consola:");
    println!("    new <archivo.oplf|archivo.opsf>   Crea un proyecto nuevo");
    println!("    inspect <ruta>                    Muestra los metadatos del proyecto");
    println!("    metadata <ruta>                   Alias de `inspect`");
    println!("    validate <ruta>                   Chequea la integridad del proyecto");
    println!("    check <ruta>                      Alias de `validate`");
    println!("    mcp [list|status|enable|disable|add|remove|transport]");
    println!("                                      Administra los puertos MCP");
    println!("    help                              Esta ayuda");
    println!("    exit                              Sale de la consola");
    println!();
    println!("  Opciones: --json (salida JSON), --template <default|empty>, --force");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_sale_de_la_consola() {
        assert!(matches!(
            run_line("exit", false),
            LineResult::Exit(ExitCode::SUCCESS)
        ));
        assert!(matches!(
            run_line("quit", false),
            LineResult::Exit(ExitCode::SUCCESS)
        ));
    }

    #[test]
    fn una_linea_vacia_no_hace_nada() {
        assert!(matches!(run_line("   ", false), LineResult::Continue));
    }

    #[test]
    fn una_comando_desconocido_no_mata_la_consola() {
        assert!(matches!(run_line("no-existe", false), LineResult::Continue));
    }

    #[test]
    fn el_argv_se_reconstruye_igual() {
        let cli = Cli::try_parse_from(["hikaru_openlive", "mcp", "enable", "3"]).unwrap();
        let argv = cli_to_argv(&cli, false);
        assert_eq!(argv[1], "mcp");
        assert_eq!(argv[2], "enable");
        assert_eq!(argv[3], "3");
    }
}
