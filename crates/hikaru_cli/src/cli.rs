// La gramática de la línea de comandos.
// SPDX-License-Identifier: AGPL-3.0-or-later
// crates/hikaru_cli/src/cli.rs
//
// =========================================================================
// POR QUÉ `clap` Y NO UN PARSER A MANO
// =========================================================================
//
// La línea de comandos de un DAW la escriben tres tipos de gente: el usuario
// que la descubre con `--help`, el agente de IA que la genera desde un script
// y el que la mantiene dentro de seis meses. Los tres necesitan lo mismo:
// que un error de tipeo diga QUÉ se esperaba y no "argumento inválido", y que
// `--help` no se desactualice con respecto a lo que el programa acepta.
//
// `clap` con la macro `Parser` da las dos cosas: la gramática queda escrita
// una vez (declarativa, junto a los tipos que la usan) y la ayuda se genera
// sola.
//
// =========================================================================
// EL AZÚCAR `--cli` Y `--mcp`
// =========================================================================
//
// La especificación pide las banderas `--cli` y `--mcp`. Son atajos de los
// subcomandos `cli` y `mcp`: la traducción la hace el `dispatcher` (ver
// `normalize_argv` en el `lib.rs`) para que la gramática de los puertos MCP
// esté escrita UN SOLO VEZ.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

/// La línea de comandos de `hikaru_openlive`.
#[derive(Debug, Parser)]
#[command(
    name = "hikaru_openlive",
    version,
    about = "Hikaru OpenLive: DAW de performance en vivo y arreglo, con consola para agentes de IA",
    long_about = "Hikaru OpenLive: DAW de performance en vivo y arreglo, con consola para agentes de IA.\n\n\
                  Sin argumentos, levanta la interfaz gráfica. Con un comando, responde por terminal \
                  sin inicializar GPUI ni el motor de audio.",
    after_help = "Ejemplos:\n  \
                  hikaru_openlive                              Levanta la GUI\n  \
                  hikaru_openlive --help                       Muestra el banner y la ayuda\n  \
                  hikaru_openlive new mi-sesion.oplf           Crea un proyecto nuevo\n  \
                  hikaru_openlive inspect mi-sesion.oplf       Vuelca los metadatos\n  \
                  hikaru_openlive validate mi-sesion.oplf      Chequea la integridad\n  \
                  hikaru_openlive --mcp                       Lista los puertos MCP\n  \
                  hikaru_openlive --mcp enable 3              Habilita el puerto MCP 3\n  \
                  hikaru_openlive --cli                       Consola interactiva",
    disable_help_subcommand = true
)]
pub struct Cli {
    /// Levanta la interfaz gráfica (es lo que pasa si no se pasa ningún comando).
    #[arg(long, global = true)]
    pub gui: bool,

    /// Levanta la GUI sin el backend de audio (para servidores sin placa de sonido).
    #[arg(long, global = true)]
    pub no_audio: bool,

    /// Carga un proyecto (.oplf/.opsf) al abrir la ventana.
    #[arg(long, global = true, value_name = "PATH")]
    pub project: Option<PathBuf>,

    /// La salida es JSON en vez de texto: lo que prefieren los agentes de IA.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Los comandos de la consola.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Crea un proyecto nuevo (.oplf o .opsf) con la plantilla por omisión.
    New(NewArgs),

    /// Muestra los metadatos del proyecto: BPM, pistas, clips y escenas.
    Inspect(InspectArgs),

    /// Alias de `inspect`.
    #[command(hide = true)]
    Metadata(InspectArgs),

    /// Chequea la integridad del proyecto y de los recursos que referencia.
    Validate(ValidateArgs),

    /// Alias de `validate`.
    #[command(hide = true)]
    Check(ValidateArgs),

    /// Administra los puertos MCP (los que usan los agentes de IA).
    Mcp(McpArgs),

    /// Consola interactiva: banner, ayuda y comandos por la entrada estándar.
    Cli,

    /// Levanta la interfaz gráfica.
    Gui,
}

/// `new`: los argumentos de la creación de un proyecto.
#[derive(Debug, Clone, Parser)]
pub struct NewArgs {
    /// Ruta del proyecto a crear. La extensión decide el formato:
    /// `.oplf` (OpenLive) o `.opsf` (OpenStudio).
    pub path: PathBuf,

    /// La plantilla del proyecto nuevo.
    #[arg(long, value_enum, default_value_t = Template::Default)]
    pub template: Template,

    /// Alias de `--template empty`: el proyecto sin pistas ni escenas.
    #[arg(long, conflicts_with = "template")]
    pub empty: bool,

    /// El título del proyecto (va en `metadata.title`).
    #[arg(long, value_name = "TEXTO")]
    pub title: Option<String>,

    /// El BPM inicial del proyecto.
    #[arg(long, default_value_t = 140.0, value_name = "BPM")]
    pub bpm: f64,

    /// Sobrescribe el archivo si ya existe.
    #[arg(long)]
    pub force: bool,
}

/// La plantilla con la que se crea un proyecto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Template {
    /// La plantilla por omisión del DAW: la Session Matrix de 8x8 en `.oplf`
    /// y el master + una pista en `.opsf`.
    Default,
    /// El proyecto pelado: el master y nada más.
    Empty,
}

/// `inspect` / `metadata`: los argumentos de la inspección.
#[derive(Debug, Clone, Parser)]
pub struct InspectArgs {
    /// Ruta del proyecto a inspeccionar (.oplf o .opsf).
    pub path: PathBuf,
}

/// `validate` / `check`: los argumentos del chequeo de integridad.
#[derive(Debug, Clone, Parser)]
pub struct ValidateArgs {
    /// Ruta del proyecto a chequear (.oplf o .opsf).
    pub path: PathBuf,
}

/// `mcp`: los argumentos de la administración de los puertos.
#[derive(Debug, Clone, Parser)]
pub struct McpArgs {
    /// La acción a hacer. Sin acción, es como `list`.
    #[command(subcommand)]
    pub action: Option<McpAction>,
}

/// Las acciones de la administración de puertos MCP.
#[derive(Debug, Clone, Subcommand)]
pub enum McpAction {
    /// Lista los slots de puerto y su estado.
    List,

    /// Muestra el detalle de un slot (o de toda la configuración).
    Status {
        /// El slot a detallar. Sin número, muestra la configuración completa.
        port: Option<u16>,
    },

    /// Habilita un slot: el puerto acepta conexiones de agentes.
    Enable {
        /// El número del slot.
        port: u16,
    },

    /// Deshabilita un slot: el puerto deja de aceptar conexiones.
    Disable {
        /// El número del slot.
        port: u16,
    },

    /// Agrega un slot nuevo al final de la lista.
    Add,

    /// Elimina un slot.
    Remove {
        /// El número del slot.
        port: u16,
    },

    /// Cambia el transporte con el que habla el slot.
    Transport {
        /// El número del slot.
        port: u16,
        /// El transporte nuevo.
        transport: TransportKind,
    },
}

/// Los transportes que puede usar un puerto MCP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TransportKind {
    /// Socket de dominio Unix: el DAW escucha y el agente se conecta.
    Unix,
    /// JSON-RPC por la entrada y la salida estándar del proceso.
    Stdio,
    /// Un servidor MCP externo, lanzado con una línea de comandos.
    ServerFlags,
}

impl TransportKind {
    /// El nombre del transporte tal como lo muestra la GUI.
    pub fn label(self) -> &'static str {
        match self {
            TransportKind::Unix => "Unix Socket",
            TransportKind::Stdio => "Stdio",
            TransportKind::ServerFlags => "Server Flags",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn la_gramatica_es_valida() {
        // `debug_assert` de clap: si un subcomando queda mal cableado, el
        // test falla acá y no cuando un usuario lo descubre.
        Cli::command().debug_assert();
    }

    #[test]
    fn sin_argumentos_no_hay_comando() {
        let cli = Cli::try_parse_from(["hikaru_openlive"]).unwrap();
        assert!(cli.command.is_none());
        assert!(!cli.json);
    }

    #[test]
    fn el_comando_new_toma_la_ruta() {
        let cli = Cli::try_parse_from(["hikaru_openlive", "new", "x.oplf"]).unwrap();
        match cli.command {
            Some(Command::New(args)) => {
                assert_eq!(args.path, PathBuf::from("x.oplf"));
                assert_eq!(args.template, Template::Default);
                assert!(!args.force);
            }
            other => panic!("esperaba `new`, {:?}", other),
        }
    }

    #[test]
    fn empty_es_alias_de_la_plantilla_vacia() {
        let cli = Cli::try_parse_from(["hikaru_openlive", "new", "x.opsf", "--empty"]).unwrap();
        match cli.command {
            Some(Command::New(args)) => {
                // La bandera existe: la plantilla se resuelve al ejecutar, pero
                // las dos no pueden pedir cosas distintas a la vez.
                assert!(args.empty);
            }
            other => panic!("esperaba `new`, {:?}", other),
        }
    }

    #[test]
    fn template_y_empty_no_se_pueden_mezclar() {
        assert!(Cli::try_parse_from([
            "hikaru_openlive",
            "new",
            "x.oplf",
            "--empty",
            "--template",
            "default"
        ])
        .is_err());
    }

    #[test]
    fn metadata_es_alias_de_inspect() {
        let cli = Cli::try_parse_from(["hikaru_openlive", "metadata", "x.oplf"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Metadata(_))));
    }

    #[test]
    fn check_es_alias_de_validate() {
        let cli = Cli::try_parse_from(["hikaru_openlive", "check", "x.opsf"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Check(_))));
    }

    #[test]
    fn mcp_sin_accion_es_lo_mismo_que_list() {
        let cli = Cli::try_parse_from(["hikaru_openlive", "mcp"]).unwrap();
        match cli.command {
            Some(Command::Mcp(args)) => assert!(args.action.is_none()),
            other => panic!("esperaba `mcp`, {:?}", other),
        }
    }

    #[test]
    fn mcp_enable_toma_el_numero_del_slot() {
        let cli = Cli::try_parse_from(["hikaru_openlive", "mcp", "enable", "3"]).unwrap();
        match cli.command {
            Some(Command::Mcp(args)) => match args.action {
                Some(McpAction::Enable { port }) => assert_eq!(port, 3),
                other => panic!("esperaba `enable`, {:?}", other),
            },
            other => panic!("esperaba `mcp`, {:?}", other),
        }
    }

    #[test]
    fn json_es_global() {
        // `--json` funciona antes y después del subcomando.
        let cli = Cli::try_parse_from(["hikaru_openlive", "--json", "mcp", "list"]).unwrap();
        assert!(cli.json);
        let cli = Cli::try_parse_from(["hikaru_openlive", "mcp", "list", "--json"]).unwrap();
        assert!(cli.json);
    }
}
