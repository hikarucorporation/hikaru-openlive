// crates/hikaru_mcp/src/plugins.rs
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Los plugins VST3 y CLAP: descubrirlos, instanciarlos y leer sus
//! parámetros.
//!
//! El descubrimiento lo hace `hikaru_plugin_host::registry`, que ya recorre
//! los directorios estándar (`~/.vst3`, `/usr/lib/vst3`, `~/.clap`,
//! `~/.local/lib/clap`, `/usr/lib/clap`) y normaliza bundles de VST3 y
//! binarios sueltos de CLAP.
//!
//! Lo que el escáner NO hace es instanciar los plugins: mira el filesystem, no
//! carga el `.so`. Por eso `vendor` viene vacío y `kind` siempre es `Effect`.
//! Para leer los parámetros hay que cargar la instancia de verdad, que es lo
//! que hacen `plugin_parameters` y `set_parameter`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

use hikaru_plugin_host::registry::{self, PluginDescriptor, ScanReport};
use hikaru_plugin_host::{PluginFormat, PluginInstance};

/// Los símbolos exportados que delatan el ABI de un plugin.
///
/// Un VST3 exporta `GetPluginFactory` y `ModuleEntry`; un CLAP exporta
/// `clap_entry`. Si aparecen, el binario habla el protocolo y se puede
/// auditar.
const ENTRY_POINTS_VST3: [&str; 6] = [
    "GetPluginFactory",
    "ModuleEntry",
    "ModuleExit",
    "BundleEntry",
    "BundleExit",
    "main",
];

const ENTRY_POINTS_CLAP: [&str; 3] = ["clap_entry", "clap_init", "main"];

/// Escanea los directorios estándar de plugins.
///
/// El escaneo puede tardar (mira `/usr/lib`), así que el resultado se cachea
/// en el backend: se vuelve a calcular sólo si el agente lo pide de nuevo
/// con `refrescar`.
pub fn scan() -> ScanReport {
    registry::scan_standard_paths()
}

/// El informe de escaneo como JSON.
pub fn scan_json(report: &ScanReport) -> Value {
    json!({
        "total": report.plugin_count(),
        "plugins": report.plugins.iter().map(descriptor_json).collect::<Vec<_>>(),
        "errores": report.errors.iter().map(|e| json!({
            "ruta": e.path.display().to_string(),
            "motivo": e.reason,
        })).collect::<Vec<_>>(),
    })
}

/// Un plugin como JSON.
pub fn descriptor_json(descriptor: &PluginDescriptor) -> Value {
    json!({
        "id": descriptor.id,
        "nombre": descriptor.name,
        "vendor": descriptor.vendor,
        "ruta": descriptor.path.display().to_string(),
        "formato": format_label(descriptor.format),
        "tipo": match descriptor.kind {
            registry::PluginKind::Effect => "efecto",
            registry::PluginKind::Instrument => "instrumento",
        },
        "tiene_gui": descriptor.has_gui,
        "canales_entrada": descriptor.input_channels,
    })
}

/// El nombre corto del formato.
fn format_label(format: PluginFormat) -> &'static str {
    match format {
        PluginFormat::VST3 => "VST3",
        PluginFormat::CLAP => "CLAP",
    }
}

/// Busca un plugin por su id estable (`"vst3:/ruta"` o `"clap:/ruta"`) o por
/// su ruta.
///
/// El agente no siempre tiene el id a mano: la ruta es lo que aparece en los
/// archivos de proyecto, así que se acepta cualquiera de las dos formas.
pub fn binario_de(
    backend: &crate::Backend,
    referencia: &str,
) -> Result<PathBuf, String> {
    let como_ruta = Path::new(referencia);
    if como_ruta.is_file() {
        return Ok(como_ruta.to_path_buf());
    }

    let descriptor = resolve(&backend.scan(false), referencia).ok_or_else(|| {
        format!("no se encontró el plugin '{referencia}'")
    })?;

    binary_of(&descriptor).ok_or_else(|| {
        format!(
            "el plugin '{}' no tiene binario dentro (bundle vacío o formato raro)",
            descriptor.display_label()
        )
    })
}

pub fn resolve(report: &ScanReport, referencia: &str) -> Option<PluginDescriptor> {
    if let Some(encontrado) = report.plugins.iter().find(|p| p.id == referencia) {
        return Some(encontrado.clone());
    }

    // Por ruta: puede venir con o sin el prefijo de formato.
    let ruta = referencia
        .split_once(':')
        .map(|(_, resto)| resto)
        .unwrap_or(referencia);

    report
        .plugins
        .iter()
        .find(|p| p.path.display().to_string() == ruta)
        .cloned()
}

/// El binario real de un plugin.
///
/// Para un bundle de VST3 el `descriptor.path` es el directorio `.vst3`, y el
/// `.so` vive adentro, en `Contents/<arquitectura>/`. Para un CLAP, el path
/// ya es el binario.
pub fn binary_of(descriptor: &PluginDescriptor) -> Option<PathBuf> {
    match descriptor.format {
        PluginFormat::CLAP => Some(descriptor.path.clone()),
        PluginFormat::VST3 => {
            // `Contents/<arquitectura>/*.so`: se busca sin hardcodear la
            // arquitectura para que funcione con `arm64` también.
            let contents = descriptor.path.join("Contents");
            let Ok(entradas) = std::fs::read_dir(&contents) else {
                return None;
            };

            let mut candidatos: Vec<PathBuf> = entradas
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .flat_map(|dir| {
                    std::fs::read_dir(dir)
                        .into_iter()
                        .flatten()
                        .flatten()
                        .map(|e| e.path())
                        .filter(|p| {
                            p.extension().map(|e| e == "so").unwrap_or(false)
                                || p.file_name()
                                    .map(|n| n.to_string_lossy().ends_with(".module"))
                                    .unwrap_or(false)
                        })
                        .collect::<Vec<_>>()
                })
                .collect();

            candidatos.sort();
            candidatos.into_iter().next()
        }
    }
}

/// Los metadatos de un bundle VST3 (`Contents/Resources/moduleinfo.json`).
///
/// Es lo que dice qué es el plugin sin cargarlo: nombre, versión, vendor y
/// los CID de las clases.
pub fn module_info(bundle: &Path) -> Option<Value> {
    let ruta = bundle.join("Contents/Resources/moduleinfo.json");
    let texto = std::fs::read_to_string(ruta).ok()?;
    let json: Value = serde_json::from_str(&texto).ok()?;
    Some(json)
}

/// Los puntos de entrada exportados por un binario de plugin.
///
/// Se leen de la tabla de símbolos dinámicos con `nm`. Si `nm` no está
/// instalado, se devuelve un error claro en vez de un resultado vacío: un
/// vacío sería indistinguible de "este plugin no habla VST3".
pub fn entry_points(binario: &Path, formato: PluginFormat) -> Result<Vec<String>, String> {
    let salida = Command::new("nm")
        .arg("-D")
        .arg("--defined-only")
        .arg(binario)
        .output()
        .map_err(|err| {
            format!(
                "no se pudo ejecutar 'nm' para leer los símbolos: {err}. \
                 Instalá binutils o usá la herramienta 'decompile_plugin_stub'."
            )
        })?;

    if !salida.status.success() {
        return Err(format!(
            "nm no pudo leer {}: {}",
            binario.display(),
            String::from_utf8_lossy(&salida.stderr).trim()
        ));
    }

    let texto = String::from_utf8_lossy(&salida.stdout);
    let candidatos: &[&str] = match formato {
        PluginFormat::VST3 => &ENTRY_POINTS_VST3,
        PluginFormat::CLAP => &ENTRY_POINTS_CLAP,
    };

    let mut encontrados: Vec<String> = Vec::new();
    for linea in texto.lines() {
        let Some(nombre) = linea.split_whitespace().last() else {
            continue;
        };
        // Los símbolos versionados (`GetPluginFactory@@VitalPLUGINABI_1.5.5`)
        // se comparan por la parte antes de la arroba.
        let limpio = nombre.split('@').next().unwrap_or(nombre);
        if candidatos.contains(&limpio) && !encontrados.iter().any(|e| e == limpio) {
            encontrados.push(limpio.to_string());
        }
    }
    encontrados.sort();
    Ok(encontrados)
}

/// Carga la instancia de un plugin.
///
/// La carga es real: se hace `dlopen` del binario y se instancian sus
/// clases. Por eso puede fallar (faltan dependencias, el plugin aborta al
/// inicializarse) y por eso hay que devolver el error al agente en vez de
/// tragárselo.
pub fn instantiate(descriptor: &PluginDescriptor) -> Result<Box<dyn PluginInstance>, String> {
    match descriptor.format {
        PluginFormat::VST3 => {
            let instancia = hikaru_plugin_host::vst3::Vst3Instance::load(&descriptor.path)?;
            Ok(Box::new(instancia))
        }
        PluginFormat::CLAP => {
            let instancia = hikaru_plugin_host::clap::ClapInstance::load(&descriptor.path)?;
            Ok(Box::new(instancia))
        }
    }
}

/// Los parámetros de una instancia, con su valor actual.
pub fn parameters(instancia: &mut dyn PluginInstance) -> Value {
    if !instancia.params_supported() {
        return json!({
            "soporta_parametros": false,
            "detalle": "este plugin no expone parámetros por la API de host",
            "parametros": [],
        });
    }

    let cantidad = instancia.param_count();
    let mut lista: Vec<Value> = Vec::new();

    for indice in 0..cantidad {
        let Some(info) = instancia.param_info(indice) else {
            continue;
        };
        let valor = instancia.get_param_normalized(info.id);
        lista.push(json!({
            "indice": indice,
            "id": info.id,
            "nombre": info.name,
            "unidad": info.unit,
            "min": info.min,
            "max": info.max,
            "default": info.default_value,
            "pasos": info.step_count,
            "automatizable": info.can_automate,
            "booleano": info.is_boolean(),
            "discreto": info.is_discrete(),
            "valor_normalizado": valor,
            "valor_legible": valor.map(|v| info.display_value(v)),
        }));
    }

    json!({
        "soporta_parametros": true,
        "total": cantidad,
        "parametros": lista,
    })
}

/// Mueve un parámetro de una instancia.
///
/// El valor va normalizado (0.0 a 1.0), como en la API de host: es lo que
/// entiende el plugin y lo que evita que el agente tenga que conocer la
/// escala real de cada parámetro.
pub fn set_parameter(
    instancia: &mut dyn PluginInstance,
    indice: usize,
    valor: f32,
) -> Result<Value, String> {
    if !instancia.params_supported() {
        return Err("este plugin no expone parámetros por la API de host".to_string());
    }

    let valor = valor.clamp(0.0, 1.0);
    let info = instancia
        .param_info(indice)
        .ok_or_else(|| format!("el plugin no tiene un parámetro en el índice {indice}"))?;

    if !instancia.set_param_normalized(info.id, valor) {
        return Err(format!("el plugin rechazó el valor para el parámetro {}", info.name));
    }

    Ok(json!({
        "aplicado": true,
        "indice": indice,
        "id": info.id,
        "nombre": info.name,
        "valor_normalizado": valor,
        "valor_legible": info.display_value(valor),
    }))
}

/// La superficie nativa del editor de un plugin.
///
/// El wrapper (`hikaru-wrapper`) es lo que hospeda la ventana nativa del
/// plugin dentro de la GUI. Sin la GUI corriendo no hay superficie que
/// reportar, pero sí hay datos estáticos que explican POR QUÉ no la hay: si
/// el plugin no declara editor, o si el host no tiene ventana abierta, el
/// agente lo ve aquí en vez de adivinar.
///
/// Lo que sí se puede levantar sin ventana es la instancia: si el plugin
/// declara `has_gui`, se instancia y se pregunta por el tamaño del editor.
pub fn wrapper_info(descriptor: &PluginDescriptor) -> Value {
    let mut info = descriptor_json(descriptor);

    let binario = binary_of(descriptor);
    if let Some(binario) = &binario {
        info["binario"] = json!(binario.display().to_string());
    }

    // Si el plugin se puede instanciar, se pregunta por su editor.
    match instantiate(descriptor) {
        Ok(instancia) => {
            info["instanciado"] = json!(true);
            info["tiene_gui"] = json!(instancia.has_gui());
            info["latencia_muestras"] = json!(instancia.latency_samples());

            if instancia.has_gui() {
                info["tamano_editor"] = json!(instancia.get_gui_size());
                info["gui_abierta"] = json!(instancia.is_gui_open());
                info["superficie"] = json!({
                    "tipo": "ventana nativa del plugin",
                    "ancho": instancia.get_gui_size().map(|s| s.0),
                    "alto": instancia.get_gui_size().map(|s| s.1),
                    "nota": "el handle X11 sólo existe con la GUI abierta; \
                             sin ventana corriendo no hay XID que reportar",
                });
            } else {
                info["superficie"] = json!(null);
                info["detalle"] = json!("este plugin no declara editor: es headless");
            }
        }
        Err(err) => {
            info["instanciado"] = json!(false);
            info["error_de_carga"] = json!(err);
        }
    }

    info
}

/// El resumen de un plugin: lo que el agente necesita antes de tocarlo.
pub fn summarize(descriptor: &PluginDescriptor) -> Value {
    let mut resumen = descriptor_json(descriptor);

    let binario = binary_of(descriptor);
    if let Some(binario) = &binario {
        resumen["binario"] = json!(binario.display().to_string());
        resumen["tamano_bytes"] = json!(std::fs::metadata(binario).map(|m| m.len()).unwrap_or(0));
    }

    if descriptor.format == PluginFormat::VST3 {
        if let Some(info) = module_info(&descriptor.path) {
            resumen["moduleinfo"] = info;
        }
    }

    if let Some(binario) = &binario {
        match entry_points(binario, descriptor.format) {
            Ok(puntos) => {
                resumen["entry_points"] = json!(puntos);
                resumen["habla_vst3"] = json!(puntos.iter().any(|p| p == "GetPluginFactory"));
                resumen["habla_clap"] = json!(puntos.iter().any(|p| p == "clap_entry"));
            }
            Err(err) => {
                resumen["entry_points"] = json!(null);
                resumen["aviso"] = json!(err);
            }
        }
    }

    resumen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_formato_se_etiqueta_en_mayusculas() {
        assert_eq!(format_label(PluginFormat::VST3), "VST3");
        assert_eq!(format_label(PluginFormat::CLAP), "CLAP");
    }

    #[test]
    fn los_puntos_de_entrada_se_normalizan_sin_la_version() {
        // El comportamiento real depende de `nm` instalado; si no está, la
        // función lo dice en vez de devolver una lista vacía.
        let ruta = Path::new("/bin/ls");
        match entry_points(ruta, PluginFormat::VST3) {
            Ok(puntos) => {
                assert!(!puntos.iter().any(|p| p.contains('@')));
            }
            Err(err) => {
                assert!(err.contains("nm"));
            }
        }
    }
}
