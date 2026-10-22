// crates/hikaru_mcp/src/catalog.rs
// SPDX-License-Identifier: AGPL-3.0-or-later

//! El catálogo: qué herramientas, qué recursos y qué prompts expone el
//! servidor.
//!
//! Está aparte del handler por una razón práctica: el catálogo es la
//! *documentación* que el agente ve, y tiene que poder compararse con lo que
//! el handler realmente despacha. Un test recorre las dos listas y verifica
//! que no haya una herramienta declarada que no se pueda llamar.

use crate::schema::Param;

/// Una herramienta del servidor.
#[derive(Debug, Clone)]
pub struct ToolSpec {
    /// El nombre con el que la llama el agente.
    pub name: &'static str,
    /// Qué hace, en una línea.
    pub description: &'static str,
    /// Los argumentos que acepta.
    pub params: Vec<Param>,
}

/// Un recurso del servidor.
#[derive(Debug, Clone)]
pub struct ResourceSpec {
    /// La URI con la que se lee.
    pub uri: &'static str,
    /// El nombre corto.
    pub name: &'static str,
    /// Qué contiene.
    pub description: &'static str,
    /// El tipo MIME del contenido.
    pub mime_type: &'static str,
}

/// Un prompt del servidor.
#[derive(Debug, Clone)]
pub struct PromptSpec {
    /// El nombre con el que lo pide el agente.
    pub name: &'static str,
    /// Qué hace.
    pub description: &'static str,
    /// Los argumentos que acepta: (nombre, descripción, obligatorio).
    pub arguments: Vec<(&'static str, &'static str, bool)>,
    /// El texto que se arma con los argumentos.
    pub template: &'static str,
}

/// Las herramientas del servidor.
pub fn tools() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: "list_plugins",
            description: "Lista los plugins VST3 y CLAP instalados en el sistema, con su ruta, \
                          formato y punto de entrada exportado. Es el primer paso para auditar un \
                          plugin: sin la ruta no hay nada más que hacer.",
            params: vec![
                Param::opt_bool(
                    "refrescar",
                    "Volver a escanear el disco en vez de usar el resultado cacheado.",
                ),
                Param::opt_string("formato", "Filtrar por formato.")
                    .with_choices(&["vst3", "clap"]),
            ],
        },
        ToolSpec {
            name: "plugin_info",
            description: "El resumen de un plugin: identidad (nombre, versión, vendor, CID de las \
                          clases), binario real, tamaño y símbolos de entrada exportados. Para VST3 \
                          lee el moduleinfo.json del bundle, así que no hace falta cargarlo.",
            params: vec![Param::string(
                "plugin",
                "El id del plugin (\"vst3:/ruta\") o su ruta.",
            )],
        },
        ToolSpec {
            name: "plugin_parameters",
            description: "Los parámetros de un plugin, con su valor actual. Instancia el plugin de \
                          verdad (hace dlopen), así que puede fallar si le faltan dependencias.",
            params: vec![Param::string(
                "plugin",
                "El id del plugin (\"vst3:/ruta\") o su ruta.",
            )],
        },
        ToolSpec {
            name: "set_parameter",
            description: "Mueve un parámetro de un plugin. El valor va normalizado (0.0 a 1.0), \
                          como en la API de host. Instancia el plugin, ajusta el parámetro y \
                          devuelve el valor legible.",
            params: vec![
                Param::string("plugin", "El id del plugin o su ruta."),
                Param::int("indice", "El índice del parámetro, como lo devuelve plugin_parameters."),
                Param::number("valor", "El valor normalizado, entre 0.0 y 1.0."),
            ],
        },
        ToolSpec {
            name: "inspect_plugin_wrapper",
            description: "La superficie nativa del editor de un plugin: si tiene GUI, de qué clase es \
                          y qué handle reporta el wrapper de X11. Sirve para entender por qué el \
                          plugin se dibuja mal o no aparece.",
            params: vec![Param::string(
                "plugin",
                "El id del plugin o su ruta.",
            )],
        },
        ToolSpec {
            name: "decompile_plugin_stub",
            description: "Ingeniería inversa de un plugin cerrado con Ghidra: el análisis general \
                          del binario, el pseudocode de un procedimiento, el listado de funciones o \
                          los strings. Es la herramienta para reconstruir un plugin propietario en \
                          Rust. El motor es Ghidra headless, así que funciona con cualquier \
                          versión de Ghidra instalada.",
            params: vec![
                Param::string("plugin", "El id del plugin o la ruta de su binario."),
                Param::opt_string(
                    "procedimiento",
                    "El nombre o la dirección de la función a descompilar. Sólo se usa con \
                     analisis=decompile.",
                ),
                Param::opt_string("analisis", "Qué análisis pedir.")
                    .with_choices(&["overview", "decompile", "functions", "strings"])
                    .with_default(serde_json::json!("overview")),
            ],
        },
        ToolSpec {
            name: "plugin_symbols",
            description: "Los símbolos exportados de un binario de plugin, leídos de la tabla de \
                          símbolos dinámicos. No necesita REA ni Ghidra: sirve para ver el ABI a \
                          simple vista.",
            params: vec![Param::string(
                "plugin",
                "El id del plugin o la ruta de su binario.",
            )],
        },
        ToolSpec {
            name: "audit_project",
            description: "Audita el proyecto cargado: recursos que faltan en disco, plugins que el \
                          proyecto pide y el host no tiene, y ruteos que apuntan a pistas \
                          inexistentes.",
            params: vec![],
        },
        ToolSpec {
            name: "project_info",
            description: "Los metadatos del proyecto cargado: BPM, pistas, escenas, conteo de clips y \
                          los plugins que usa cada pista.",
            params: vec![],
        },
    ]
}

/// Los recursos del servidor.
pub fn resources() -> Vec<ResourceSpec> {
    vec![
        ResourceSpec {
            uri: "hikaru://project/current",
            name: "Proyecto actual",
            description: "El proyecto cargado: metadatos, transporte, pistas y escenas.",
            mime_type: "application/json",
        },
        ResourceSpec {
            uri: "hikaru://matrix/grid",
            name: "Matriz de clips",
            description: "La grilla de pads de la vista Live/Session (o la playlist de OpenStudio).",
            mime_type: "application/json",
        },
        ResourceSpec {
            uri: "hikaru://plugins/discovered",
            name: "Plugins descubiertos",
            description: "Los plugins VST3 y CLAP que el host encontró en el sistema.",
            mime_type: "application/json",
        },
        ResourceSpec {
            uri: "hikaru://plugins/telemetry",
            name: "Telemetría de plugins",
            description: "La telemetría viva de los plugins: carga de CPU, xruns y superficie del editor.",
            mime_type: "application/json",
        },
        ResourceSpec {
            uri: "hikaru://diagnostics/last-crash",
            name: "Último crash",
            description: "El último fallo registrado por el host de plugins, con su stacktrace.",
            mime_type: "application/json",
        },
    ]
}

/// Los prompts del servidor.
pub fn prompts() -> Vec<PromptSpec> {
    vec![
        PromptSpec {
            name: "diagnosticar-fallo-audio",
            description: "Analiza un fallo de audio: xruns, conflictos de hilo y pánicos del FFI.",
            arguments: vec![
                ("sintoma", "Lo que se escucha o lo que pasó.", true),
                ("plugin", "El plugin sospechoso, si lo hay.", false),
            ],
            template: "Diagnostica este fallo de audio en Hikaru OpenLive.\n\n\
                Síntoma: {sintoma}\n\
                Plugin sospechoso: {plugin}\n\n\
                Revisá el recurso `hikaru://diagnostics/last-crash` y el recurso \
                `hikaru://plugins/telemetry`. Buscá xruns, pánicos de FFI en el wrapper del \
                plugin y conflictos entre el hilo de audio y el hilo de render. \
                Empezá por lo más probable y decí qué evidencia de cada afirmación.",
        },
        PromptSpec {
            name: "reconstruir-plugin-rust",
            description: "Mapea la interfaz de un plugin descompilado para escribir un wrapper en Rust.",
            arguments: vec![
                ("plugin", "El plugin a reconstruir.", true),
                ("objetivo", "Qué se quiere lograr con el wrapper.", false),
            ],
            template: "Reconstruí en Rust el plugin '{plugin}'.\n\n\
                Objetivo: {objetivo}\n\n\
                Pasos sugeridos:\n\
                1. `plugin_info` para la identidad y el ABI.\n\
                2. `decompile_plugin_stub` para el pseudocode del punto de entrada \
                (`GetPluginFactory` o `clap_entry`) y de las funciones de audio.\n\
                3. `plugin_parameters` para el mapa de parámetros.\n\
                4. Escribí un crate Rust que implemente la interfaz VST3 o CLAP del plugin.\n\n\
                Compará cada afirmación con el pseudocode: lo que no esté en el binario, \
                marcalo como inferencia.",
        },
        PromptSpec {
            name: "auditar-proyecto",
            description: "Verifica las dependencias de un proyecto: muestras, plugins y ruteo.",
            arguments: vec![("proyecto", "El proyecto a auditar.", false)],
            template: "Auditá el proyecto '{proyecto}'.\n\n\
                Usá la herramienta `audit_project` y el recurso `hikaru://project/current`. \
                Reportá: recursos de audio que faltan, plugins que el proyecto pide y el host \
                no tiene, y buses de ruteo que apuntan a pistas inexistentes. \
                Para cada problema, decí qué archivo y qué pista lo causan.",
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_nombres_de_herramienta_no_se_repiten() {
        let mut nombres: Vec<&str> = tools().iter().map(|t| t.name).collect();
        let total = nombres.len();
        nombres.sort_unstable();
        nombres.dedup();
        assert_eq!(nombres.len(), total, "hay herramientas con el mismo nombre");
    }

    #[test]
    fn las_uris_de_recurso_no_se_repiten() {
        let mut uris: Vec<&str> = resources().iter().map(|r| r.uri).collect();
        let total = uris.len();
        uris.sort_unstable();
        uris.dedup();
        assert_eq!(uris.len(), total, "hay recursos con la misma URI");
    }

    #[test]
    fn todas_las_uris_arrancan_con_el_prefijo_del_daw() {
        for recurso in resources() {
            assert!(
                recurso.uri.starts_with("hikaru://"),
                "la URI {} no usa el esquema hikaru://",
                recurso.uri
            );
        }
    }

    #[test]
    fn los_prompts_tienen_plantilla_con_los_sus_argumentos() {
        for prompt in prompts() {
            for (nombre, _, _) in &prompt.arguments {
                let marcador = format!("{{{nombre}}}");
                assert!(
                    prompt.template.contains(&marcador),
                    "el prompt '{}' no usa el argumento {nombre}",
                    prompt.name
                );
            }
        }
    }

    #[test]
    fn la_herramienta_de_decompilar_acepta_el_analisis_general() {
        let catalogo = tools();
        let decompile = catalogo
            .iter()
            .find(|t| t.name == "decompile_plugin_stub")
            .expect("está la herramienta");
        assert!(decompile.params.iter().any(|p| p.name == "procedimiento"));
        assert!(decompile.params.iter().any(|p| p.name == "analisis"));
    }
}
