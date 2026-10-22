// crates/hikaru_mcp/src/server.rs
// SPDX-License-Identifier: AGPL-3.0-or-later

//! El handler MCP: lo que el agente ve y lo que puede pedir.
//!
//! Acá está el despacho de las herramientas, la lectura de los recursos y la
//! construcción de los prompts. La lógica de cada dominio vive en su módulo
//! (`project`, `plugins`, `rea`, `crash`); este modulo es el que las conecta
//! con el protocolo.

use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, GetPromptRequestParams,
    GetPromptResponse, GetPromptResult, Implementation, InitializeResult, ListPromptsResult,
    ListResourcesResult, ListToolsResult, PaginatedRequestParams, Prompt, PromptArgument,
    PromptMessage, ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, Resource,
    ResourceContents, Role, ServerCapabilities, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData as McpError, ServerHandler};

use crate::catalog;
use crate::plugins;
use crate::project;
use crate::{Backend, Mode};

/// El servidor MCP de Hikaru.
#[derive(Clone)]
pub struct HikaruServer {
    /// El estado compartido.
    backend: Arc<Backend>,
}

impl HikaruServer {
    /// Un servidor nuevo sobre el backend dado.
    pub fn new(backend: Arc<Backend>) -> Self {
        Self { backend }
    }

    /// El proyecto cargado, o un error que lo dice.
    fn proyecto(&self) -> Result<project::LoadedProject, String> {
        let guard = self
            .backend
            .project
            .lock()
            .map_err(|_| "el estado del proyecto se rompió (poisoned)".to_string())?;

        guard.as_ref().cloned().ok_or_else(|| {
            "no hay ningún proyecto cargado. Levantá el servidor con \
             --project <archivo.oplf|.opsf> o poné HIKARU_PROJECT."
                .to_string()
        })
    }
}

// =========================================================================
// EL HANDLER
// =========================================================================

impl ServerHandler for HikaruServer {
    fn get_info(&self) -> InitializeResult {
        let capacidades = ServerCapabilities::builder()
            .enable_prompts()
            .enable_resources()
            .enable_tools()
            .build();

        InitializeResult::new(capacidades)
            .with_server_info(
                Implementation::new("hikaru-openlive", env!("CARGO_PKG_VERSION"))
                    .with_title("Hikaru OpenLive")
                    .with_description(
                        "El DAW Hikaru OpenLive: proyecto, matriz de clips, plugins VST3/CLAP \
                         e ingeniería inversa de plugins cerrados."
                            .to_string(),
                    ),
            )
            .with_instructions(
                "Conectate con `hikaru://project/current` para ver el proyecto, con \
                 `list_plugins` para ver los plugins del sistema y con `decompile_plugin_stub` \
                 para ingeniería inversa de un plugin propietario. El modo del servidor \
                 determina si el estado viene del disco (files) o de la GUI (live)."
                    .to_string(),
            )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let herramientas: Vec<Tool> = catalog::tools()
            .into_iter()
            .map(|spec| {
                let schema = crate::schema::object_schema(&spec.params);
                Tool::new(spec.name, spec.description, schema)
            })
            .collect();

        Ok(ListToolsResult::with_all_items(herramientas))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let nombre = request.name.to_string();
        let argumentos = request.arguments.unwrap_or_default();

        let llamada = match nombre.as_str() {
            "list_plugins" => ToolCall::ListPlugins {
                refrescar: arg_bool(&argumentos, "refrescar").unwrap_or(false),
                formato: arg_str(&argumentos, "formato"),
            },
            "plugin_info" => ToolCall::PluginInfo {
                plugin: arg_requerido(&argumentos, "plugin", &nombre)?,
            },
            "plugin_parameters" => ToolCall::PluginParameters {
                plugin: arg_requerido(&argumentos, "plugin", &nombre)?,
            },
            "set_parameter" => ToolCall::SetParameter {
                plugin: arg_requerido(&argumentos, "plugin", &nombre)?,
                indice: arg_requerido(&argumentos, "indice", &nombre)?,
                valor: arg_requerido(&argumentos, "valor", &nombre)?,
            },
            "inspect_plugin_wrapper" => ToolCall::InspectWrapper {
                plugin: arg_requerido(&argumentos, "plugin", &nombre)?,
            },
            "decompile_plugin_stub" => ToolCall::DecompileStub {
                plugin: arg_requerido(&argumentos, "plugin", &nombre)?,
                procedimiento: arg_str(&argumentos, "procedimiento"),
                analisis: arg_str(&argumentos, "analisis").unwrap_or_else(|| "analyze".to_string()),
            },
            "plugin_symbols" => ToolCall::PluginSymbols {
                plugin: arg_requerido(&argumentos, "plugin", &nombre)?,
            },
            "audit_project" => ToolCall::AuditProject,
            "project_info" => ToolCall::ProjectInfo,
            otra => {
                return Err(McpError::invalid_params(
                    format!("no existe la herramienta '{otra}'"),
                    None,
                ));
            }
        };

        // Las herramientas tocan el disco y pueden instanciar plugins: van al
        // pool de hilos bloqueantes para no trabar el runtime del servidor.
        let backend = Arc::clone(&self.backend);
        let resultado = tokio::task::spawn_blocking(move || ejecutar(backend, llamada))
            .await
            .map_err(|err| {
                McpError::internal_error(
                    format!("el hilo de la herramienta falló: {err}"),
                    None,
                )
            })?;

        Ok(resultado.into())
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let recursos: Vec<Resource> = catalog::resources()
            .into_iter()
            .map(|spec| {
                let mut recurso = Resource::new(spec.uri, spec.name);
                recurso.description = Some(spec.description.to_string());
                recurso.mime_type = Some(spec.mime_type.to_string());
                recurso
            })
            .collect();

        Ok(ListResourcesResult::with_all_items(recursos))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        let uri = request.uri.clone();
        let contenido = self.leer_recurso(&uri);

        let texto = match contenido {
            Ok(valor) => serde_json::to_string_pretty(&valor).unwrap_or_else(|_| valor.to_string()),
            Err(err) => {
                // Un recurso que no se puede leer es un error de la
                // herramienta, no del protocolo: el agente tiene que ver por
                // qué.
                return Ok(ReadResourceResponse::Complete(ReadResourceResult::new(vec![
                    ResourceContents::TextResourceContents {
                        uri,
                        mime_type: Some("application/json".to_string()),
                        text: serde_json::to_string_pretty(&serde_json::json!({
                            "error": err,
                            "servidor": self.backend.describe(),
                        }))
                        .unwrap_or_default(),
                        meta: None,
                    },
                ])));
            }
        };

        Ok(ReadResourceResponse::Complete(ReadResourceResult::new(vec![
            ResourceContents::TextResourceContents {
                uri,
                mime_type: Some("application/json".to_string()),
                text: texto,
                meta: None,
            },
        ])))
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        let prompts: Vec<Prompt> = catalog::prompts()
            .into_iter()
            .map(|spec| {
                let argumentos = spec
                    .arguments
                    .into_iter()
                    .map(|(nombre, descripcion, obligatorio)| {
                        let mut argumento = PromptArgument::new(nombre);
                        argumento.description = Some(descripcion.to_string());
                        argumento.required = Some(obligatorio);
                        argumento
                    })
                    .collect();

                Prompt::new(spec.name, Some(spec.description), Some(argumentos))
            })
            .collect();

        Ok(ListPromptsResult::with_all_items(prompts))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, McpError> {
        let spec = catalog::prompts()
            .into_iter()
            .find(|p| p.name == request.name)
            .ok_or_else(|| {
                McpError::invalid_params(format!("no existe el prompt '{}'", request.name), None)
            })?;

        let argumentos = request.arguments.unwrap_or_default();
        let mut texto = spec.template.to_string();

        for (nombre, _, _) in &spec.arguments {
            let marcador = format!("{{{nombre}}}");
            let valor = argumentos
                .get(*nombre)
                .map(|v| match v {
                    serde_json::Value::String(s) => s.clone(),
                    otro => otro.to_string(),
                })
                .unwrap_or_else(|| "(sin especificar)".to_string());
            texto = texto.replace(&marcador, &valor);
        }

        Ok(GetPromptResponse::Complete(
            GetPromptResult::new(vec![PromptMessage::new_text(Role::User, texto)])
                .with_description(spec.description),
        ))
    }
}

// =========================================================================
// LA LECTURA DE LOS RECURSOS
// =========================================================================

impl HikaruServer {
    /// Lee un recurso por su URI.
    fn leer_recurso(&self, uri: &str) -> Result<serde_json::Value, String> {
        match uri {
            "hikaru://project/current" => self.proyecto().map(|p| p.info()),
            "hikaru://matrix/grid" => self.proyecto().map(|p| p.matrix_grid()),
            "hikaru://plugins/discovered" => {
                let informe = self.backend.scan(false);
                Ok(plugins::scan_json(&informe))
            }
            "hikaru://plugins/telemetry" => {
                let exportador = self
                    .backend
                    .telemetry
                    .lock()
                    .map_err(|_| "la telemetría se rompió (poisoned)".to_string())?;
                serde_json::from_str(&exportador.export_json())
                    .map_err(|err| format!("la telemetría no es JSON válido: {err}"))
            }
            "hikaru://diagnostics/last-crash" => Ok(crate::crash::last()),
            otra => Err(format!("no existe el recurso '{otra}'")),
        }
    }
}

// =========================================================================
// EL DESPACHO DE LAS HERRAMIENTAS
// =========================================================================

/// Una llamada a una herramienta, ya parseada.
enum ToolCall {
    ListPlugins {
        refrescar: bool,
        formato: Option<String>,
    },
    PluginInfo {
        plugin: String,
    },
    PluginParameters {
        plugin: String,
    },
    SetParameter {
        plugin: String,
        indice: usize,
        valor: f32,
    },
    InspectWrapper {
        plugin: String,
    },
    DecompileStub {
        plugin: String,
        procedimiento: Option<String>,
        analisis: String,
    },
    PluginSymbols {
        plugin: String,
    },
    AuditProject,
    ProjectInfo,
}

/// Ejecuta la llamada y devuelve el resultado MCP.
///
/// Corre en el pool de hilos bloqueantes: instanciar un plugin o analizar un
/// binario con REA tarda, y el runtime del servidor tiene que seguir
/// atendiendo el ping del cliente mientras tanto.
fn ejecutar(backend: Arc<Backend>, llamada: ToolCall) -> CallToolResult {
    match llamada {
        ToolCall::ListPlugins { refrescar, formato } => {
            // El escaneo mira `/usr/lib` y puede tardar: se cachea salvo que
            // el agente pida refrescarlo.
            let informe = backend.scan(refrescar);

            let mut plugins: Vec<serde_json::Value> = informe
                .plugins
                .iter()
                .map(crate::plugins::descriptor_json)
                .collect();

            if let Some(formato) = formato {
                let formato = formato.to_lowercase();
                plugins.retain(|p| {
                    p.get("formato")
                        .and_then(|f| f.as_str())
                        .map(|f| f.eq_ignore_ascii_case(&formato))
                        .unwrap_or(false)
                });
            }

            crate::tool_ok(&serde_json::json!({
                "total": plugins.len(),
                "plugins": plugins,
                "errores": informe.errors.len(),
            }))
        }

        ToolCall::PluginInfo { plugin } => match resolver(&backend, &plugin) {
            Ok(descriptor) => crate::tool_ok(&crate::plugins::summarize(&descriptor)),
            Err(err) => crate::tool_error(err),
        },

        ToolCall::PluginParameters { plugin } => match resolver(&backend, &plugin) {
            Ok(descriptor) => match crate::plugins::instantiate(&descriptor) {
                Ok(mut instancia) => {
                    let parametros = crate::plugins::parameters(instancia.as_mut());
                    crate::tool_ok(&serde_json::json!({
                        "plugin": crate::plugins::descriptor_json(&descriptor),
                        "parametros": parametros,
                    }))
                }
                Err(err) => crate::tool_error(format!(
                    "no se pudo instanciar {}: {err}",
                    descriptor.display_label()
                )),
            },
            Err(err) => crate::tool_error(err),
        },

        ToolCall::SetParameter {
            plugin,
            indice,
            valor,
        } => match resolver(&backend, &plugin) {
            Ok(descriptor) => match crate::plugins::instantiate(&descriptor) {
                Ok(mut instancia) => {
                    match crate::plugins::set_parameter(instancia.as_mut(), indice, valor) {
                        Ok(resultado) => crate::tool_ok(&resultado),
                        Err(err) => crate::tool_error(err),
                    }
                }
                Err(err) => crate::tool_error(format!(
                    "no se pudo instanciar {}: {err}",
                    descriptor.display_label()
                )),
            },
            Err(err) => crate::tool_error(err),
        },

        ToolCall::InspectWrapper { plugin } => match resolver(&backend, &plugin) {
            Ok(descriptor) => crate::tool_ok(&crate::plugins::wrapper_info(&descriptor)),
            Err(err) => crate::tool_error(err),
        },

        ToolCall::DecompileStub {
            plugin,
            procedimiento,
            analisis,
        } => match crate::plugins::binario_de(&backend, &plugin) {
            Ok(binario) => {
                // Ghidra directo es el camino principal: funciona con
                // cualquier versión de Ghidra. REA queda de reserva para
                // cuando su proveedor esté disponible.
                let modo = crate::ghidra::Modo::parse(&analisis);
                let mut resultado = None;
                let mut errores: Vec<String> = Vec::new();

                if let Some(modo) = modo {
                    match crate::ghidra::run(&binario, modo, procedimiento.as_deref()) {
                        Ok(valor) => resultado = Some(valor),
                        Err(err) => errores.push(format!("ghidra: {err}")),
                    }
                }

                if resultado.is_none() {
                    match crate::rea::analyze(&binario) {
                        Ok(valor) => resultado = Some(valor),
                        Err(err) => errores.push(format!("rea: {err}")),
                    }
                }

                match resultado {
                    Some(mut valor) => {
                        valor["binario"] = serde_json::json!(binario.display().to_string());
                        valor["motor"] = serde_json::json!("ghidra");
                        if !errores.is_empty() {
                            valor["notas"] = serde_json::json!(errores);
                        }
                        crate::tool_ok(&valor)
                    }
                    None => crate::tool_error(format!(
                        "no se pudo analizar el binario. {}",
                        errores.join(" | ")
                    )),
                }
            }
            Err(err) => crate::tool_error(err),
        },

        ToolCall::PluginSymbols { plugin } => match crate::plugins::binario_de(&backend, &plugin) {
            Ok(binario) => {
                let formato = if binario.extension().map(|e| e == "clap").unwrap_or(false) {
                    hikaru_plugin_host::PluginFormat::CLAP
                } else {
                    hikaru_plugin_host::PluginFormat::VST3
                };

                match crate::plugins::entry_points(&binario, formato) {
                    Ok(puntos) => crate::tool_ok(&serde_json::json!({
                        "binario": binario.display().to_string(),
                        "entry_points": puntos,
                        "total_simbolos_exportados": puntos.len(),
                    })),
                    Err(err) => crate::tool_error(err),
                }
            }
            Err(err) => crate::tool_error(err),
        },

        ToolCall::AuditProject => {
            let guard = match backend.project.lock() {
                Ok(guard) => guard,
                Err(_) => return crate::tool_error("el estado del proyecto se rompió"),
            };
            let Some(proyecto) = guard.as_ref() else {
                return crate::tool_error(
                    "no hay ningún proyecto cargado. Levantá el servidor con \
                     --project <archivo.oplf|.opsf> o poné HIKARU_PROJECT.",
                );
            };

            let informe = backend.scan(false);
            let rutas: Vec<String> = informe
                .plugins
                .iter()
                .map(|p| p.path.display().to_string())
                .collect();

            crate::tool_ok(&proyecto.audit(&rutas))
        }

        ToolCall::ProjectInfo => {
            let guard = match backend.project.lock() {
                Ok(guard) => guard,
                Err(_) => return crate::tool_error("el estado del proyecto se rompió"),
            };
            match guard.as_ref() {
                Some(proyecto) => crate::tool_ok(&proyecto.info()),
                None => crate::tool_error(
                    "no hay ningún proyecto cargado. Levantá el servidor con \
                     --project <archivo.oplf|.opsf> o poné HIKARU_PROJECT.",
                ),
            }
        }
    }
}

/// Busca un plugin por id o por ruta y devuelve su descriptor.
///
/// Usa el escaneo cacheado del backend: mirar `/usr/lib` en cada llamada
/// seria criminal.
fn resolver(
    backend: &Backend,
    referencia: &str,
) -> Result<hikaru_plugin_host::registry::PluginDescriptor, String> {
    let informe = backend.scan(false);

    crate::plugins::resolve(&informe, referencia).ok_or_else(|| {
        let nombres: Vec<&str> = informe
            .plugins
            .iter()
            .map(|p| p.name.as_str())
            .take(20)
            .collect();
        format!(
            "no se encontró el plugin '{referencia}'. \
             Usá `list_plugins` para ver los disponibles (algunos: {nombres:?})"
        )
    })
}

// =========================================================================
// LOS ARGUMENTOS
// =========================================================================

/// Un argumento de texto, si está.
fn arg_str(args: &serde_json::Map<String, serde_json::Value>, nombre: &str) -> Option<String> {
    args.get(nombre)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Un argumento booleano, si está.
fn arg_bool(
    args: &serde_json::Map<String, serde_json::Value>,
    nombre: &str,
) -> Option<bool> {
    args.get(nombre).and_then(|v| v.as_bool())
}

/// Un argumento obligatorio: si falta o está en el tipo equivocado, es un
/// error de protocolo (el agente mandó la llamada mal).
fn arg_requerido<T>(
    args: &serde_json::Map<String, serde_json::Value>,
    nombre: &str,
    herramienta: &str,
) -> Result<T, McpError>
where
    T: FromArgument,
{
    let valor = args.get(nombre).ok_or_else(|| {
        McpError::invalid_params(
            format!("a la herramienta '{herramienta}' le falta el argumento '{nombre}'"),
            None,
        )
    })?;

    T::from_argument(valor).ok_or_else(|| {
        McpError::invalid_params(
            format!(
                "el argumento '{nombre}' de '{herramienta}' tiene un tipo equivocado: {valor}"
            ),
            None,
        )
    })
}

/// Un tipo que se puede leer de un valor JSON.
trait FromArgument: Sized {
    /// La conversión, o `None` si el tipo no corresponde.
    fn from_argument(valor: &serde_json::Value) -> Option<Self>;
}

impl FromArgument for String {
    fn from_argument(valor: &serde_json::Value) -> Option<Self> {
        valor.as_str().map(|s| s.to_string())
    }
}

impl FromArgument for usize {
    fn from_argument(valor: &serde_json::Value) -> Option<Self> {
        valor.as_u64().map(|v| v as usize)
    }
}

impl FromArgument for f32 {
    fn from_argument(valor: &serde_json::Value) -> Option<Self> {
        valor.as_f64().map(|v| v as f32)
    }
}

/// El modo del servidor, para el mensaje de arranque.
pub fn modo_label(modo: Mode) -> &'static str {
    modo.label()
}
