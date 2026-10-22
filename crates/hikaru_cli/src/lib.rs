// La consola de línea de comandos de Hikaru OpenLive.
// SPDX-License-Identifier: AGPL-3.0-or-later
// crates/hikaru_cli/src/lib.rs
//
// =========================================================================
// QUÉ ES ESTA CRATE
// =========================================================================
//
// Es el "cerebro de terminal" del producto: el mismo binario `hikaru_openlive`
// que abre la ventana abre también una consola con la que un agente de IA
// (OpenCode, Claude Code) puede crear proyectos, inspeccionarlos, validarlos
// y administrar los puertos MCP.
//
// =========================================================================
// POR QUÉ ESTA CRATE NO LINKEE GPUI
// =========================================================================
//
// El camino de la consola tiene que ser RÁPIDO y sin dependencias del entorno
// gráfico: un agente la llama decenas de veces por sesión y cada llamada que
// levanta un stack de render, un backend de audio y un thread de eventos es
// una llamada que tarda segundos y puede fallar por un `DISPLAY` que no
// existe.
//
// Por eso esta crate depende SÓLO de los esquemas de archivo (LGPLv3) y de la
// configuración de los puertos MCP. El `dispatcher` del binario la consulta
// PRIMERO y, si el comando es de consola, sale sin tocar GPUI ni el motor de
// audio. La GUI queda para el camino por omisión (`hikaru_openlive` a secas o
// `hikaru_openlive --gui`).
//
// =========================================================================
// LA FRONTERA DE LICENCIA
// =========================================================================
//
// Esta crate es AGPLv3, como la app. Los formatos de archivo que manipula
// (`.oplf`/`.opsf`) son LGPLv3 y viven en crates aparte: la consola no los
// reimplementa, los usa. Ver docs/hikaru-project-files/.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

pub mod banner;
pub mod cli;
pub mod console;
pub mod man;
pub mod mcp;
pub mod project;
pub mod version;

pub use cli::{Cli, Command, McpAction, McpArgs, NewArgs, Template, TransportKind};

/// Las opciones con las que se levanta la interfaz gráfica.
///
/// La consola las arma y se las pasa al binario: la CLI no sabe nada de
/// GPUI, sólo de qué quiere el usuario que la GUI haga al arrancar.
#[derive(Debug, Clone, Default)]
pub struct GuiOptions {
    /// Levantar la GUI sin el backend de audio (servidores sin placa de
    /// sonido, debugging, Containers).
    pub no_audio: bool,
    /// Proyecto a cargar al abrir la ventana.
    pub project: Option<PathBuf>,
}

/// Qué tiene que hacer el binario después de mirar los argumentos.
#[derive(Debug)]
pub enum Dispatch {
    ///seguir por el camino de la GUI.
    Gui(GuiOptions),
    /// La consola ya hizo su trabajo: salir con este código.
    Exit(ExitCode),
}

/// El código de salida de un error de uso (la convención de Unix: 2 = uso
/// incorrecto).
pub const EXIT_USAGE: u8 = 2;

/// El código de salida de un comando que falló (1).
pub const EXIT_FAILURE: u8 = 1;

/// Mira los argumentos del proceso y decide qué hay que hacer.
///
/// Devuelve [`Dispatch::Gui`] cuando no hay nada que hacer por terminal (o
/// cuando el usuario pidió la GUI explícitamente) y [`Dispatch::Exit`] cuando
/// la consola ya corrió y el binario tiene que salir.
///
/// La función NO llama a `exit()`: devuelve el código y deja que sea el
/// binario el que salga. Así se puede testear sin matar el proceso de tests.
pub fn dispatch_from_env() -> Dispatch {
    dispatch(std::env::args_os().collect())
}

/// Lo mismo que [`dispatch_from_env`], pero con los argumentos a mano.
///
/// Los tests usan esta: el proceso de test no puede cambiar sus propios
/// argumentos.
pub fn dispatch(argv: Vec<OsString>) -> Dispatch {
    let argv = normalize_argv(argv);

    // Sin argumentos no hay nada que decidir: el producto abre su ventana.
    if argv.len() <= 1 {
        return Dispatch::Gui(GuiOptions::default());
    }

    let cli = match Cli::try_parse_from(argv) {
        Ok(cli) => cli,
        Err(err) => {
            // La ayuda y la versión no son errores: se muestran y se sale con
            // código 0, como hace cualquier herramienta de línea de comandos.
            if matches!(
                err.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                if err.kind() == clap::error::ErrorKind::DisplayHelp {
                    // El banner va ANTES de la ayuda: es la cara del producto
                    // en la terminal y va primero (ver el diseño en
                    // docs/hikaru-openlive-cli/).
                    println!("{}", banner::banner());
                    println!();
                }
                let _ = err.print();
                return Dispatch::Exit(ExitCode::SUCCESS);
            }
            // Un argumento que no existe es un error de uso: se explica por
            // stderr y se sale con 2.
            let _ = err.print();
            eprintln!();
            eprintln!("{}", banner::hint_uso());
            return Dispatch::Exit(ExitCode::from(EXIT_USAGE));
        }
    };

    let json = cli.json;

    match cli.command {
        Some(Command::New(args)) => Dispatch::Exit(project::new(&args, json)),
        Some(Command::Inspect(args)) | Some(Command::Metadata(args)) => {
            Dispatch::Exit(project::inspect(&args, json))
        }
        Some(Command::Validate(args)) | Some(Command::Check(args)) => {
            Dispatch::Exit(project::validate(&args, json))
        }
        Some(Command::Mcp(args)) => Dispatch::Exit(mcp::run(&args, json)),
        Some(Command::Cli) => Dispatch::Exit(console::run(json)),
        Some(Command::Gui) | None => Dispatch::Gui(GuiOptions {
            no_audio: cli.no_audio,
            project: cli.project,
        }),
    }
}

/// Traduce los atajos de bandera a los subcomandos que los implementan.
///
/// `--cli` y `--mcp` son AZÚCAR: lo que el usuario escribe es
/// `hikaru_openlive mcp enable 3`, y `--mcp enable 3` es lo mismo. Si no se
/// hiciese esta traducción, el parser tendría que duplicar la gramática de
/// los puertos MCP en dos lugares (la bandera y el subcomando) y cualquier
/// acción nueva habría que agregarla en los dos.
///
/// La comparación es exacta: `--mcp` es la bandera, `mcp` es el subcomando, y
/// `--mcp=1` (que no existe) no se confunde con nada.
fn normalize_argv(argv: Vec<OsString>) -> Vec<OsString> {
    argv.into_iter()
        .map(|arg| {
            if arg == "--cli" {
                OsString::from("cli")
            } else if arg == "--mcp" {
                OsString::from("mcp")
            } else {
                arg
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    #[test]
    fn sin_argumentos_se_abre_la_gui() {
        match dispatch(argv(&["hikaru_openlive"])) {
            Dispatch::Gui(options) => {
                assert!(!options.no_audio);
                assert!(options.project.is_none());
            }
            Dispatch::Exit(_) => panic!("sin argumentos no debe salir por consola"),
        }
    }

    #[test]
    fn el_subcomando_gui_es_la_gui() {
        assert!(matches!(
            dispatch(argv(&["hikaru_openlive", "gui"])),
            Dispatch::Gui(_)
        ));
    }

    #[test]
    fn la_bandera_mcp_es_el_subcomando_mcp() {
        let normalizado = normalize_argv(argv(&["hikaru_openlive", "--mcp", "list"]));
        assert_eq!(normalizado[1], OsString::from("mcp"));
        assert_eq!(normalizado[2], OsString::from("list"));
    }

    #[test]
    fn la_bandera_cli_es_el_subcomando_cli() {
        let normalizado = normalize_argv(argv(&["hikaru_openlive", "--cli"]));
        assert_eq!(normalizado[1], OsString::from("cli"));
    }

    #[test]
    fn una_bandera_que_no_existe_es_un_error_de_uso() {
        match dispatch(argv(&["hikaru_openlive", "--no-existe"])) {
            Dispatch::Exit(code) => assert_eq!(code, ExitCode::from(EXIT_USAGE)),
            Dispatch::Gui(_) => panic!("una bandera desconocida no puede abrir la GUI"),
        }
    }

    #[test]
    fn la_ayuda_no_es_un_error() {
        match dispatch(argv(&["hikaru_openlive", "--help"])) {
            Dispatch::Exit(code) => assert_eq!(code, ExitCode::SUCCESS),
            Dispatch::Gui(_) => panic!("la ayuda no puede abrir la GUI"),
        }
    }
}
