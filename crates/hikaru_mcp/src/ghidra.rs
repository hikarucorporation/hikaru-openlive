// crates/hikaru_mcp/src/ghidra.rs
// SPDX-License-Identifier: AGPL-3.0-or-later

//! El puente con Ghidra headless.
//!
//! # Por qué Ghidra directo y no a través de REA
//!
//! REA es un motor de ingeniería inversa excelente, pero su proveedor de
//! Ghidra exige la línea **12.1.x**. Si el usuario tiene otra versión (aquí
//! hay un 12.3), REA la rechaza y no hay decompilación.
//!
//! Este módulo habla con `analyzeHeadless` directamente, así que funciona
//! con cualquier Ghidra que tenga el decompilador nativo. REA queda como
//! segunda opción, para cuando su proveedor esté disponible.
//!
//! # El script
//!
//! El script de Ghidra va embebido en el binario (`include_str!`) y se
//! escribe en un directorio temporal antes de cada corrida: así el servidor
//! no depende de ningún archivo suelto en el disco.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde_json::{json, Value};

/// El script de Ghidra que hace el trabajo. Va embebido.
const SCRIPT_DECOMP: &str = include_str!("ghidra_scripts/Decomp.java");

/// La variable de entorno que dice dónde está Ghidra.
pub const GHIDRA_ENV: &str = "GHIDRA_INSTALL_DIR";

/// Cuánto puede tardar un análisis. Ghidra headless sobre un plugin de
/// 7 MB tarda varios minutos.
const TIMEOUT: Duration = Duration::from_secs(1800);

/// Cuánto puede analizar Ghidra antes de seguir adelante.
///
/// El análisis automático de un binario con quince mil funciones puede
/// llevar horas. El tope hace que el script se corra igual, con lo que se
/// haya encontrado: para una pregunta puntual alcanza, y para el resto está
/// el listado completo de funciones del proyecto.
const ANALYSIS_TIMEOUT: u32 = 900;

/// Dónde está Ghidra.
///
/// Se busca en este orden:
/// 1. la variable de entorno `GHIDRA_INSTALL_DIR`,
/// 2. los Ghidra que estén en el workspace de Hikaru,
/// 3. `/opt/ghidra`, `/usr/share/ghidra`.
pub fn locate() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var(GHIDRA_ENV) {
        if !dir.is_empty() {
            let path = PathBuf::from(dir);
            if tiene_analyze_headless(&path) {
                return Some(path);
            }
        }
    }

    // El workspace de Hikaru: es donde el usuario tiene su fork.
    if let Some(home) = std::env::var("HOME").ok() {
        let workspace = Path::new(&home)
            .join("Documentos/Codigo_Fuente/hikaru-workspace");
        if let Ok(entradas) = std::fs::read_dir(&workspace) {
            let mut candidatas: Vec<PathBuf> = entradas
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .map(|n| n.to_string_lossy().contains("ghidra"))
                        .unwrap_or(false)
                })
                .flat_map(|raiz| {
                    // El layout del build: `<raiz>/dist/ghidra_<version>/`.
                    let dist = raiz.join("dist");
                    let Ok(releases) = std::fs::read_dir(&dist) else {
                        return Vec::new();
                    };
                    releases
                        .flatten()
                        .map(|e| e.path())
                        .filter(|p| p.join("support/analyzeHeadless").is_file())
                        .collect::<Vec<_>>()
                })
                .collect();
            candidatas.sort();
            if let Some(primera) = candidatas.into_iter().next() {
                return Some(primera);
            }
        }
    }

    for candidata in ["/opt/ghidra", "/usr/share/ghidra", "/usr/local/ghidra"] {
        let path = PathBuf::from(candidata);
        if tiene_analyze_headless(&path) {
            return Some(path);
        }
    }

    None
}

/// ¿Este directorio es una instalación de Ghidra con el headless?
fn tiene_analyze_headless(dir: &Path) -> bool {
    dir.join("support/analyzeHeadless").is_file()
}

/// La versión de Ghidra, leída de `application.properties`.
pub fn version(install_dir: &Path) -> Option<String> {
    let texto = std::fs::read_to_string(
        install_dir.join("Ghidra/application.properties"),
    )
    .ok()?;

    texto
        .lines()
        .find_map(|linea| {
            let mut partes = linea.splitn(2, '=');
            let clave = partes.next()?.trim();
            let valor = partes.next()?.trim();
            (clave == "application.version").then(|| valor.to_string())
        })
}

/// Qué análisis se pide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modo {
    /// El resumen del binario.
    Overview,
    /// El pseudocode de una función.
    Decompile,
    /// El listado de funciones.
    Functions,
    /// Los strings del binario.
    Strings,
}

impl Modo {
    /// El nombre del modo, como lo entiende el script.
    pub fn as_str(self) -> &'static str {
        match self {
            Modo::Overview => "overview",
            Modo::Decompile => "decompile",
            Modo::Functions => "functions",
            Modo::Strings => "strings",
        }
    }

    /// El modo a partir del nombre.
    pub fn parse(nombre: &str) -> Option<Self> {
        match nombre.to_lowercase().as_str() {
            "overview" | "analyze" | "analisis" => Some(Modo::Overview),
            "decompile" | "decompilar" | "pseudocode" => Some(Modo::Decompile),
            "functions" | "funciones" => Some(Modo::Functions),
            "strings" => Some(Modo::Strings),
            _ => None,
        }
    }
}

/// La variable de entorno que dice dónde vive el caché de proyectos.
pub const CACHE_ENV: &str = "HIKARU_GHIDRA_CACHE";

/// El directorio donde viven los proyectos de Ghidra.
///
/// Los proyectos se cachean: importar y analizar un plugin de 7 MB con
/// Ghidra tarda minutos, y preguntar por cinco funciones no puede costar
/// cinco análisis.
///
/// **OJO con la ruta**: Ghidra rechaza cualquier elemento del path que
/// empiece con `.` (`Path element starting with '.' is not permitted`). Eso
/// deja fuera a `~/.cache` y a `~/.local/state`, así que el caché vive en
/// `/var/tmp`, que además está en disco y no en RAM.
pub fn cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var(CACHE_ENV) {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }

    let primario = PathBuf::from("/var/tmp/hikaru/ghidra");
    if std::fs::create_dir_all(&primario).is_ok() {
        return primario;
    }

    // Si `/var/tmp` no se puede usar, el runtime dir (tmpfs) es el plan B.
    match std::env::var("XDG_RUNTIME_DIR") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir).join("hikaru/ghidra"),
        _ => std::env::temp_dir().join("hikaru-ghidra"),
    }
}

/// La huella de un binario: path, tamaño y fecha de modificación.
///
/// Si cualquiera de los tres cambia, el proyecto cacheado ya no sirve: hay
/// que reimportar.
fn huella(binario: &Path) -> String {
    let meta = std::fs::metadata(binario).ok();
    let tamano = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let fecha = meta
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    format!("{}|{tamano}|{fecha}", binario.display())
}

/// El nombre del proyecto de Ghidra para un binario.
///
/// Se deriva del path, así que dos binarios distintos no se pisan.
fn nombre_proyecto(binario: &Path) -> String {
    let crudo = binario
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "binario".to_string());

    let limpio: String = crudo
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();

    format!("hikaru_{limpio}")
}

/// El candado que serializa las corridas de Ghidra.
///
/// Ghidra **no** puede abrir el mismo proyecto desde dos procesos a la vez
/// (`LockException: Unable to lock project!`). Y un agente puede pedir dos
/// decompilaciones en paralelo: sin este candado, la segunda muere con un
/// error de lock que no dice nada del motivo real.
///
/// El candado es del proceso, no del sistema: alcanza porque todas las
/// corridas salen de acá.
static CANDADO_GHIDRA: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Corre Ghidra headless con el script y devuelve lo que el script imprimió.
///
/// El proyecto de Ghidra se cachea: la primera llamada sobre un binario
/// importa y analiza (minutos); las siguientes reutilizan el proyecto y sólo
/// corren el script (segundos).
pub fn run(
    binario: &Path,
    modo: Modo,
    objetivo: Option<&str>,
) -> Result<Value, String> {
    // Una corrida a la vez: Ghidra no admite dos procesos sobre el mismo
    // proyecto.
    let _candado = CANDADO_GHIDRA
        .lock()
        .map_err(|_| "el candado de Ghidra se rompió (poisoned)".to_string())?;

    let Some(install) = locate() else {
        return Err(
            "no se encontró Ghidra. Poné GHIDRA_INSTALL_DIR apuntando a la \
             instalación (la que tiene support/analyzeHeadless)."
                .to_string(),
        );
    };

    let proyectos = cache_dir();
    std::fs::create_dir_all(&proyectos)
        .map_err(|err| format!("no se pudo crear el directorio de cache: {err}"))?;

    let scripts = proyectos.join("scripts");
    std::fs::create_dir_all(&scripts)
        .map_err(|err| format!("no se pudo crear el directorio de scripts: {err}"))?;

    // El script va embebido: se escribe antes de cada corrida.
    let ruta_script = scripts.join("Decomp.java");
    std::fs::write(&ruta_script, SCRIPT_DECOMP)
        .map_err(|err| format!("no se pudo escribir el script de Ghidra: {err}"))?;

    let headless = install.join("support/analyzeHeadless");
    let proyecto = nombre_proyecto(binario);
    let marcador = proyectos.join(format!("{proyecto}.huella"));

    // ¿El proyecto cacheado corresponde a este binario?
    let huella_actual = huella(binario);
    let cacheado = std::fs::read_to_string(&marcador)
        .map(|guardado| guardado.trim() == huella_actual)
        .unwrap_or(false);

    let mut comando = Command::new(&headless);
    comando
        .arg(&proyectos)
        .arg(&proyecto)
        .arg("-scriptPath")
        .arg(&scripts);

    if cacheado {
        // El programa ya está importado y analizado: sólo se corre el script.
        //
        // OJO: `-process` NO acepta la ruta del archivo, sino el nombre
        // DENTRO del proyecto. Con el path completo Ghidra aborta con
        // "invalid filename specified".
        let nombre_en_proyecto = binario
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| binario.display().to_string());

        comando
            .arg("-process")
            .arg(nombre_en_proyecto)
            .arg("-noanalysis");
    } else {
        comando
            .arg("-import")
            .arg(binario)
            .arg("-analysisTimeoutPerFile")
            .arg(ANALYSIS_TIMEOUT.to_string());
    }

    comando
        .arg("-postScript")
        .arg("Decomp.java")
        .arg(modo.as_str());

    if let Some(objetivo) = objetivo {
        comando.arg(objetivo);
    }

    // `java` tiene que estar en el PATH: Ghidra lo busca por ahí.
    comando.env("GHIDRA_INSTALL_DIR", &install);

    let (tx, rx) = std::sync::mpsc::channel();
    let esperando = std::thread::spawn(move || {
        let salida = comando.output();
        let _ = tx.send(salida);
    });

    let salida = match rx.recv_timeout(TIMEOUT) {
        Ok(Ok(salida)) => {
            let _ = esperando.join();
            salida
        }
        Ok(Err(err)) => {
            return Err(format!("no se pudo ejecutar {}: {err}", headless.display()));
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            return Err(format!(
                "Ghidra pasó de los {} minutos y se lo cortó. Un análisis \
                 completo de un plugin de varios megabytes puede tardar: \
                 para una mirada rápida usá `plugin_symbols` o `plugin_info`.",
                TIMEOUT.as_secs() / 60
            ));
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            return Err("el hilo que esperaba a Ghidra murió".to_string());
        }
    };

    let texto = String::from_utf8_lossy(&salida.stdout).to_string();
    let valor = parsear(&texto);

    if valor.is_none() {
        let stderr = String::from_utf8_lossy(&salida.stderr);

        // Si el script no compiló, el error de javac está en la salida: es
        // mucho más útil que el "ClassNotFoundException" que Ghidra escupe
        // después.
        let errores_javac: Vec<&str> = texto
            .lines()
            .filter(|l| l.contains("error:") || l.contains("symbol:") || l.contains("location:"))
            .take(12)
            .collect();

        if !errores_javac.is_empty() {
            return Err(format!(
                "el script de Ghidra no compiló: {}",
                errores_javac.join(" | ")
            ));
        }

        return Err(format!(
            "Ghidra no devolvió nada utilizable (código {}). \
             Errores: {} | Últimas líneas: {}",
            salida.status.code().unwrap_or(-1),
            stderr.lines().rev().take(5).collect::<Vec<_>>().join(" | "),
            texto.lines().rev().take(8).collect::<Vec<_>>().join(" | ")
        ));
    }

    // Si el import funcionó, se guarda la huella para reutilizar el proyecto.
    if !cacheado && valor.is_some() {
        let _ = std::fs::write(&marcador, &huella_actual);
    }

    Ok(valor.unwrap())
}

/// Le saca el prefijo de nivel de log a una línea.
///
/// Ghidra headless escribe `INFO  ` (o `ERROR  `) adelante de cada línea que
/// sale del script. Para los mensajes multilínea (el pseudocode) el prefijo
/// va sólo en la primera línea, y ahí además viene el nombre del script:
///
/// ```text
/// INFO  Decomp.java> int FUN_0012fb40(long *param_1)
/// {
///   ...
/// }
/// ```
fn sin_prefijo_de_log(linea: &str) -> String {
    let mut resto = linea.trim_start();

    // 1. El nivel del log.
    for nivel in ["INFO", "ERROR", "WARN", "DEBUG"] {
        if let Some(r) = resto.strip_prefix(nivel) {
            resto = r.trim_start();
            break;
        }
    }

    // 2. El nombre del script, que aparece en la primera línea.
    if let Some(pos) = resto.find(".java>") {
        let nombre = &resto[..pos];
        if !nombre.is_empty()
            && nombre
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '-')
        {
            resto = resto[pos + ".java>".len()..].trim_start();
        }
    }

    resto.to_string()
}

/// Parsea la salida del script, que viene marcada con líneas `HIKARU_*`.
///
/// Ghidra headless prefixa cada línea con el nivel del log (`INFO  `,
/// `ERROR  `) y a veces con el nombre del script, así que el marcador se
/// busca dentro de la línea y no al principio.
fn parsear(texto: &str) -> Option<Value> {
    let mut resultado = serde_json::Map::new();
    let mut codigo: Option<String> = None;
    let mut en_codigo = false;
    let mut lineas_codigo: Vec<String> = Vec::new();

    for linea in texto.lines() {
        let recortada = linea.trim();

        // El marcador puede venir con prefijo: `INFO  HIKARU_CODE_BEGIN`.
        let marcador = recortada
            .split_whitespace()
            .find(|t| t.starts_with("HIKARU_"))
            .unwrap_or("");

        if marcador == "HIKARU_CODE_BEGIN" {
            en_codigo = true;
            lineas_codigo.clear();
            continue;
        }
        if marcador == "HIKARU_CODE_END" {
            en_codigo = false;
            resultado.insert(
                "CODE".to_string(),
                Value::String(lineas_codigo.join("\n")),
            );
            continue;
        }

        if en_codigo {
            // El cuerpo del código viene con el prefijo del log adelante
            // (`INFO  `): se saca para que el agente reciba C limpio.
            lineas_codigo.push(sin_prefijo_de_log(recortada));
            continue;
        }

        // `HIKARU_CLAVE=valor`: la clave es el primer token con `=`.
        let Some((clave, valor)) = recortada
            .split_whitespace()
            .find_map(|t| t.split_once('='))
            .filter(|(clave, _)| clave.starts_with("HIKARU_"))
        else {
            continue;
        };

        let clave = clave.trim_start_matches("HIKARU_").to_string();
        if clave == "CODE" {
            codigo = Some(lineas_codigo.join("\n"));
            continue;
        }
        resultado.insert(clave, Value::String(valor.to_string()));
    }

    if let Some(codigo) = codigo {
        resultado.insert("CODE".to_string(), Value::String(codigo));
    }

    // Si el script reportó un error, se devuelve igual: el agente tiene que
    // poder leer POR QUÉ falló.
    if resultado.is_empty() {
        return None;
    }
    // Las listas vienen separadas por comas: se vuelven arreglos.
    for clave in ["EXPORTS", "FUNC_LIST", "STRING_LIST"] {
        if let Some(texto) = resultado.get(clave).and_then(|v| v.as_str()) {
            let lista: Vec<Value> = texto
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| Value::String(s.to_string()))
                .collect();
            resultado.insert(clave.to_string(), Value::Array(lista));
        }
    }

    Some(Value::Object(resultado))
}

/// El resumen del motor, para la herramienta de estado.
pub fn resumen() -> Value {
    match locate() {
        Some(install) => json!({
            "disponible": true,
            "instalacion": install.display().to_string(),
            "version": version(&install),
        }),
        None => json!({
            "disponible": false,
            "detalle": "no se encontró GHIDRA_INSTALL_DIR ni una instalación en el workspace",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_modos_se_parsean() {
        assert_eq!(Modo::parse("overview"), Some(Modo::Overview));
        assert_eq!(Modo::parse("analyze"), Some(Modo::Overview));
        assert_eq!(Modo::parse("decompile"), Some(Modo::Decompile));
        assert_eq!(Modo::parse("functions"), Some(Modo::Functions));
        assert_eq!(Modo::parse("strings"), Some(Modo::Strings));
        assert_eq!(Modo::parse("no-existe"), None);
    }

    #[test]
    fn la_salida_del_script_se_parsea() {
        // Así es como Ghidra headless escupe las líneas: con el nivel del log
        // adelante.
        let salida = "\
INFO  HIKARU_BEGIN
INFO  HIKARU_PROGRAMA=UZU.so
INFO  HIKARU_FUNCIONES=15749
INFO  HIKARU_EXPORTS=GetPluginFactory@99840,ModuleEntry@99800
INFO  HIKARU_CODE_BEGIN
INFO  void f(void) { }
INFO  HIKARU_CODE_END
INFO  HIKARU_END
";
        let valor = parsear(salida).expect("se parsea");
        assert_eq!(valor["PROGRAMA"], "UZU.so");
        assert_eq!(valor["FUNCIONES"], "15749");
        assert_eq!(valor["CODE"], "void f(void) { }");

        let exports = valor["EXPORTS"].as_array().expect("es una lista");
        assert_eq!(exports.len(), 2);
        assert_eq!(exports[0], "GetPluginFactory@99840");
    }

    #[test]
    fn la_salida_sin_prefijo_tambien_se_parsea() {
        let salida = "HIKARU_FUNCIONES=42\n";
        let valor = parsear(salida).expect("se parsea");
        assert_eq!(valor["FUNCIONES"], "42");
    }

    #[test]
    fn una_salida_vacia_no_se_parsea() {
        assert!(parsear("nada que ver").is_none());
    }
}
