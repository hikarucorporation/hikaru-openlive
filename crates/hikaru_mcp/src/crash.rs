// crates/hikaru_mcp/src/crash.rs
// SPDX-License-Identifier: AGPL-3.0-or-later

//! El diagnóstico del último fallo del host de plugins.
//!
//! Cuando un plugin VST3 o CLAP revienta, el wrapper (`hikaru-wrapper`) deja
//! un log. Este módulo busca esos logs y devuelve el último, para que el
//! agente pueda entender qué pasó sin tener que pescar a mano en el disco.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// La variable de entorno que dice dónde están los logs del crash.
pub const CRASH_DIR_ENV: &str = "HIKARU_CRASH_DIR";

/// El directorio donde se escriben los logs de crash.
///
/// Por omisión, `$XDG_STATE_HOME/hikaru/crashes` (o `~/.local/state/hikaru/
/// crashes`). Los logs son efímeros: son de la última sesión, no un archivo
/// de proyecto.
pub fn crash_dir() -> PathBuf {
    if let Ok(dir) = std::env::var(CRASH_DIR_ENV) {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }

    let base = match std::env::var("XDG_STATE_HOME") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => match std::env::var("HOME") {
            Ok(home) if !home.is_empty() => PathBuf::from(home).join(".local/state"),
            _ => PathBuf::from("/tmp"),
        },
    };

    base.join("hikaru/crashes")
}

/// Un log de crash.
#[derive(Debug)]
pub struct CrashLog {
    /// De qué archivo salió.
    pub path: PathBuf,
    /// Cuándo se escribió (del nombre del archivo, si el formato lo dice).
    pub modified: Option<std::time::SystemTime>,
    /// El contenido.
    pub text: String,
}

/// Los logs de crash, del más nuevo al más viejo.
pub fn list() -> Vec<CrashLog> {
    let dir = crash_dir();
    let Ok(entradas) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut logs: Vec<CrashLog> = entradas
        .flatten()
        .map(|entrada| entrada.path())
        .filter(|p| p.is_file())
        .filter_map(|path| {
            let texto = std::fs::read_to_string(&path).ok()?;
            let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
            Some(CrashLog {
                path,
                modified,
                text: texto,
            })
        })
        .collect();

    // Del más nuevo al más viejo.
    logs.sort_by(|a, b| b.modified.cmp(&a.modified));
    logs
}

/// El último crash, como JSON.
///
/// Si no hay ninguno, devuelve `null` en vez de un error: que no haya crashes
/// es una respuesta válida, no una falla.
pub fn last() -> Value {
    let logs = list();

    let Some(crash) = logs.into_iter().next() else {
        return json!({
            "hay_crash": false,
            "detalle": "no hay logs de crash en el directorio de estado",
            "directorio": crash_dir().display().to_string(),
        });
    };

    json!({
        "hay_crash": true,
        "ruta": crash.path.display().to_string(),
        "directorio": crash_dir().display().to_string(),
        "modificado": crash.modified.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs()),
        "contenido": crash.text,
    })
}

/// El resumen de un crash: las primeras líneas, que es donde está el panic.
pub fn resumen(crash: &CrashLog, lineas: usize) -> String {
    crash.text.lines().take(lineas).collect::<Vec<_>>().join("\n")
}

/// Busca el directorio de crash dentro de una ruta (para tests).
pub fn crash_dir_in(base: &Path) -> PathBuf {
    base.join("crashes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_logs_devuelve_un_mensaje_y_no_una_error() {
        let dir = std::env::temp_dir().join(format!("hikaru_mcp_crashes_{}", std::process::id()));
        std::env::set_var(CRASH_DIR_ENV, &dir);

        let valor = last();
        assert_eq!(valor["hay_crash"], json!(false));

        std::env::remove_var(CRASH_DIR_ENV);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn un_crash_se_lee_como_json() {
        let dir = std::env::temp_dir().join(format!("hikaru_mcp_crashes2_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("crash-1.log"), "panic: boom\n").unwrap();

        std::env::set_var(CRASH_DIR_ENV, &dir);
        let valor = last();
        assert_eq!(valor["hay_crash"], json!(true));
        assert!(valor["contenido"].as_str().unwrap().contains("panic"));

        std::env::remove_var(CRASH_DIR_ENV);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
