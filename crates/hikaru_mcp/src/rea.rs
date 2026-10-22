// crates/hikaru_mcp/src/rea.rs
// SPDX-License-Identifier: AGPL-3.0-or-later

//! El puente con REA (Reverse Engineer Anything).
//!
//! REA es un motor de ingeniería inversa que ya está instalado como servidor
//! MCP. Este módulo no reimplementa nada de eso: **invoca la CLI de REA** y
//! devuelve su JSON.
//!
//! ¿Por qué la CLI y no el servidor MCP? Porque este servidor MCP puede estar
//! hablando con REA por el mismo mecanismo, y dos clientes MCP que se hablan
//! por socket Unix se bloquean: el segundo espera a que el primero se
//! desconecte. La CLI es un proceso aparte, sin ese conflicto.
//!
//! La ruta de la CLI se busca, en este orden:
//! 1. la variable de entorno `REA_COMMAND`,
//! 2. `rea` en el `PATH`,
//! 3. las instalaciones de npm que estén en el home del usuario.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;

/// Cuánto puede tardar una llamada a REA antes de que se la considere colgada.
const TIMEOUT: Duration = Duration::from_secs(600);

/// Dónde está la CLI de REA.
///
/// Devuelve `None` si no se encuentra en ningún lado: el agente recibe un
/// mensaje que dice cómo instalarla en vez de un error de sistema.
pub fn locate() -> Option<PathBuf> {
    // 1. La variable de entorno manda sobre todo.
    if let Ok(ruta) = std::env::var("REA_COMMAND") {
        if !ruta.is_empty() {
            let path = PathBuf::from(ruta);
            if path.exists() {
                return Some(path);
            }
        }
    }

    // 2. El PATH.
    if let Ok(paths) = std::env::var("PATH") {
        for dir in paths.split(':').filter(|d| !d.is_empty()) {
            let candidata = Path::new(dir).join("rea");
            if candidata.is_file() {
                return Some(candidata);
            }
        }
    }

    // 3. Las instalaciones de npm: `rea` puede vivir en el binario global o
    //    en el caché de `npx`.
    let home = std::env::var("HOME").ok()?;
    let candidatas = [
        PathBuf::from(&home).join(".npm-global/bin/rea"),
        PathBuf::from(&home).join(".local/bin/rea"),
    ];
    for candidata in candidatas {
        if candidata.is_file() {
            return Some(candidata);
        }
    }

    // 4. El caché de npx: `~/.npm/_npx/<hash>/node_modules/.bin/rea`.
    let npx = PathBuf::from(&home).join(".npm/_npx");
    if let Ok(entradas) = std::fs::read_dir(&npx) {
        let mut candidatas: Vec<PathBuf> = entradas
            .flatten()
            .map(|e| e.path().join("node_modules/.bin/rea"))
            .filter(|p| p.is_file())
            .collect();
        candidatas.sort();
        if let Some(primera) = candidatas.into_iter().next() {
            return Some(primera);
        }
    }

    None
}

/// El resultado de una llamada a la CLI de REA.
#[derive(Debug)]
pub struct ReaOutput {
    /// El código de salida del proceso.
    pub status: i32,
    /// La salida estándar.
    pub stdout: String,
    /// La salida de error.
    pub stderr: String,
}

/// Corre la CLI de REA con los argumentos dados.
///
/// Devuelve el error de sistema si la CLI no está, y un `ReaOutput` con el
/// código de salida si corrió pero falló: un REA que sale con 1 y explica
/// por qué es información útil para el agente, no una excepción.
///
/// La llamada tiene tiempo límite: un análisis de Ghidra sobre un plugin
/// grande puede tardar minutos, y un agente no puede quedarse esperando
/// para siempre. Cuando se agota, el proceso hijo se mata y se dice
/// cuántos segundos llevaba.
pub fn run(args: &[String]) -> Result<ReaOutput, String> {
    let Some(comando) = locate() else {
        return Err(
            "no se encontró la CLI de REA. Instalala con `npm i -g rea-agents` \
             o apuntá REA_COMMAND al ejecutable."
                .to_string(),
        );
    };

    let hijo = Command::new(&comando)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("no se pudo ejecutar {}: {err}", comando.display()))?;

    // El timeout se mide en un hilo aparte: `wait` con timeout no existe en
    // `std`, y el crate no depende de tokio para esto.
    let (tx, rx) = std::sync::mpsc::channel();
    let esperando = std::thread::spawn(move || {
        let salida = hijo.wait_with_output();
        let _ = tx.send(salida);
    });

    match rx.recv_timeout(TIMEOUT) {
        Ok(Ok(salida)) => {
            let _ = esperando.join();
            Ok(ReaOutput {
                status: salida.status.code().unwrap_or(-1),
                stdout: String::from_utf8_lossy(&salida.stdout).to_string(),
                stderr: String::from_utf8_lossy(&salida.stderr).to_string(),
            })
        }
        Ok(Err(err)) => Err(format!("REA terminó con error: {err}")),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            // El proceso se queda colgado hasta que lo maten: se mata a
            // mano para no dejar un analyzeHeadless consuming CPU.
            Err(format!(
                "REA pasó de los {} segundos y se lo cortó. \
                 El análisis de Ghidra sobre un plugin grande puede tardar: \
                 probá con `plugin_info` o `plugin_symbols`, que son al instante.",
                TIMEOUT.as_secs()
            ))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err("el hilo que esperaba a REA murió".to_string())
        }
    }
}

/// El análisis general de un binario: qué es, qué contiene.
///
/// Es el `rea analyze` de la CLI: el mismo análisis que devuelve `evidence_id`
/// y el conteo de procedimientos y strings.
///
/// Se pide con `--full-output` para que la respuesta venga siempre dentro de
/// un sobre (`{"ok": true, "result": ...}`): sin el sobre, un error de REA
/// es indistinguible de un análisis.
pub fn analyze(binario: &Path) -> Result<Value, String> {
    let salida = run(&[
        "analyze".to_string(),
        binario.display().to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--full-output".to_string(),
    ])?;

    parse_json(&salida)
}

/// El pseudocode de un procedimiento.
///
/// `procedimiento` puede ser un nombre de función o una dirección. Si viene
/// vacío, se pide el análisis de una función concreta.
pub fn decompile(binario: &Path, procedimiento: Option<&str>) -> Result<Value, String> {
    let mut args = vec!["decompile".to_string(), binario.display().to_string()];

    if let Some(procedimiento) = procedimiento {
        args.push("--procedure".to_string());
        args.push(procedimiento.to_string());
    }

    args.push("--format".to_string());
    args.push("json".to_string());
    args.push("--full-output".to_string());

    let salida = run(&args)?;
    parse_json(&salida)
}

/// Convierte la salida de la CLI en JSON.
///
/// La CLI con `--format json` imprime el sobre completo. Si algo se escapa
/// (un aviso por stderr), se recorta hasta el primer `{` o `[` para no
/// romper el parseo.
///
/// OJO: la CLI devuelve los errores COMO JSON (`{"ok": false, "error": {...}}`).
/// Si eso se tomara por un resultado, el agente leería un error de REA como
/// si fuera el análisis del binario. Por eso se inspecciona el sobre.
fn parse_json(salida: &ReaOutput) -> Result<Value, String> {
    let texto = salida.stdout.trim();

    if texto.is_empty() {
        return Err(if salida.stderr.trim().is_empty() {
            format!("REA terminó con código {} y no devolvió nada", salida.status)
        } else {
            format!(
                "REA terminó con código {}: {}",
                salida.status,
                salida.stderr.trim()
            )
        });
    }

    let valor = match serde_json::from_str::<Value>(texto) {
        Ok(valor) => valor,
        Err(err) => {
            // Probar con la parte que parece JSON.
            let recorte = texto
                .find(['{', '['])
                .and_then(|inicio| texto.get(inicio..).map(|resto| resto.to_string()));
            match recorte.and_then(|resto| serde_json::from_str::<Value>(&resto).ok()) {
                Some(valor) => valor,
                None => {
                    return Err(format!(
                        "la salida de REA no es JSON ({err}). Primeros 400 caracteres: {}",
                        texto.chars().take(400).collect::<String>()
                    ))
                }
            }
        }
    };

    // El sobre de REA tiene tres formas de error, y ninguna es obvia:
    //
    //   {"ok": false, "error": {...}}                  — error de la CLI
    //   {"ok": true,  "data": {"error": "..."}}        — el análisis falló
    //   {"error": "..."}                               — sin sobre
    //
    // Sin detectarlas, un error de REA se leería como si fuera el análisis
    // del binario.
    if let Some(detalle) = extraer_error(&valor) {
        let remediation = valor
            .get("remediation")
            .and_then(|r| r.get("action"))
            .and_then(Value::as_str)
            .or_else(|| {
                valor
                    .get("data")
                    .and_then(|d| d.get("remediation"))
                    .and_then(|r| r.get("action"))
                    .and_then(Value::as_str)
            });

        return Err(match remediation {
            Some(accion) => format!("REA: {detalle}. {accion}"),
            None => format!("REA: {detalle}"),
        });
    }

    // El sobre de éxito: se devuelve el contenido, no el envoltorio.
    if valor.get("ok").and_then(Value::as_bool) == Some(true) {
        if let Some(resultado) = valor.get("result") {
            return Ok(resultado.clone());
        }
        if let Some(data) = valor.get("data") {
            return Ok(data.clone());
        }
    }

    Ok(valor)
}

/// Saca el mensaje de error de la salida de REA, si lo hay.
fn extraer_error(valor: &Value) -> Option<String> {
    // 1. Sobre de error: {"ok": false, "error": {...}}.
    if valor.get("ok").and_then(Value::as_bool) == Some(false) {
        return Some(mensaje_de_error(valor.get("error")));
    }

    // 2. El análisis falló: {"ok": true, "data": {"error": "..."}}.
    if let Some(data) = valor.get("data") {
        if data.get("error").is_some() || data.get("code").is_some() {
            return Some(mensaje_de_error(Some(data)));
        }
    }

    // 3. Error plano: {"error": "...", "code": "...", "message": "..."}.
    if valor.get("error").is_some() || valor.get("code").is_some() {
        return Some(mensaje_de_error(Some(valor)));
    }

    None
}

/// El mensaje legible de un error de REA.
fn mensaje_de_error(error: Option<&Value>) -> String {
    let Some(error) = error else {
        return "error sin detalle".to_string();
    };

    if let Some(texto) = error.as_str() {
        return texto.to_string();
    }

    error
        .get("message")
        .and_then(Value::as_str)
        .map(|m| m.to_string())
        .unwrap_or_else(|| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn una_salida_no_json_da_un_error_legible() {
        let salida = ReaOutput {
            status: 1,
            stdout: "esto no es json".to_string(),
            stderr: "algo falló".to_string(),
        };
        let err = parse_json(&salida).unwrap_err();
        assert!(err.contains("no es JSON"));
    }

    #[test]
    fn una_salida_vacia_dice_el_codigo_de_salida() {
        let salida = ReaOutput {
            status: 3,
            stdout: String::new(),
            stderr: String::new(),
        };
        let err = parse_json(&salida).unwrap_err();
        assert!(err.contains('3'));
    }

    #[test]
    fn el_sobre_de_error_de_rea_es_un_error() {
        // REA devuelve los errores como JSON: si se tomara por un resultado,
        // el agente leería un error como si fuera el análisis.
        let salida = ReaOutput {
            status: 1,
            stdout: r#"{"ok":false,"error":{"code":"provider_unavailable","message":"no hay proveedor"}}"#
                .to_string(),
            stderr: String::new(),
        };
        let err = parse_json(&salida).unwrap_err();
        assert!(err.contains("no hay proveedor"));
    }

    #[test]
    fn un_sobre_de_exito_se_devuelve_como_valor() {
        let salida = ReaOutput {
            status: 0,
            stdout: r#"{"ok":true,"result":{"x":1}}"#.to_string(),
            stderr: String::new(),
        };
        // El sobre se abre: lo que llega es el contenido.
        let valor = parse_json(&salida).expect("es un resultado");
        assert_eq!(valor["x"], 1);
    }

    #[test]
    fn un_error_dentro_de_data_es_un_error() {
        // La forma real del sobre de REA: `ok` es true pero el análisis
        // falló y el error viene dentro de `data`.
        let salida = ReaOutput {
            status: 0,
            stdout: r#"{"ok":true,"data":{"error":"Analysis failed","code":"provider_unavailable"}}"#
                .to_string(),
            stderr: String::new(),
        };
        let err = parse_json(&salida).unwrap_err();
        assert!(err.contains("Analysis failed"));
    }

    #[test]
    fn un_error_plano_sin_sobre_es_un_error() {
        let salida = ReaOutput {
            status: 1,
            stdout: r#"{"error":"boom","code":"x","message":"se rompio"}"#.to_string(),
            stderr: String::new(),
        };
        let err = parse_json(&salida).unwrap_err();
        assert!(err.contains("se rompio"));
    }
}
