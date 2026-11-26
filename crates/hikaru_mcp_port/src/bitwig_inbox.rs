// crates/hikaru_mcp_port/src/bitwig_inbox.rs
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Buzón compartido de archivos `.bwdevice` y `.bwpreset` arrastrados al DAW.
//!
//! # Propósito (Clean-Room Wine/Proton)
//!
//! Cuando el usuario arrastra un `.bwdevice` o `.bwpreset` desde su gestor de
//! archivos (p. ej. Dolphin) hacia la ventana de Hikaru OpenLive, la GUI
//! registra únicamente la ruta y metadatos básicos del drop en este buzón en
//! `$XDG_RUNTIME_DIR/hikaru/dropped-bitwig-files.json`.
//!
//! De ese modo, el servidor standalone `hikaru_mcp` puede leer en tiempo real
//! qué artefacto soltó el usuario e inspeccionar su contrato público en modo
//! solo lectura sin linkear `hikaru_gui` ni tocar SDKs propietarios.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::default_runtime_dir;

/// Límite de entradas retenidas en el buzón efímero.
const MAX_INBOX_ENTRIES: usize = 64;

/// Registro de un archivo `.bwdevice` o `.bwpreset` soltado sobre el DAW.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BitwigDropRecord {
    /// Ruta absoluta o tal como la entregó el gestor de ventanas.
    pub path: PathBuf,
    /// Nombre de archivo (ej. `Bit-8.bwdevice`).
    pub file_name: String,
    /// Extensión normalizada en minúsculas (`bwdevice` o `bwpreset`).
    pub extension: String,
    /// Marca temporal Unix en segundos.
    pub dropped_at_unix_secs: u64,
    /// Origen del evento (ej. `gui-drop`).
    pub source: String,
}

/// Estado persistido del buzón en runtime.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BitwigDropInbox {
    /// Entradas ordenadas de la más antigua a la más reciente.
    pub entries: Vec<BitwigDropRecord>,
}

/// Ruta por omisión del buzón en `$XDG_RUNTIME_DIR/hikaru/dropped-bitwig-files.json`.
pub fn default_bitwig_inbox_path() -> PathBuf {
    default_runtime_dir().join("dropped-bitwig-files.json")
}

/// Devuelve `true` si la extensión corresponde a `.bwdevice` o `.bwpreset`.
pub fn is_supported_bitwig_artifact(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("bwdevice") || ext.eq_ignore_ascii_case("bwpreset"))
        .unwrap_or(false)
}

/// Carga el buzón desde disco; si aún no existe, devuelve uno vacío.
pub fn load_bitwig_inbox(inbox_path: &Path) -> BitwigDropInbox {
    let Ok(bytes) = fs::read_to_string(inbox_path) else {
        return BitwigDropInbox::default();
    };
    serde_json::from_str(&bytes).unwrap_or_default()
}

/// Registra un drop en el buzón por omisión.
pub fn record_bitwig_drop(dropped_file: &Path, source: &str) -> Result<BitwigDropRecord, String> {
    record_bitwig_drop_to(&default_bitwig_inbox_path(), dropped_file, source)
}

/// Registra un drop en una ruta de buzón explícita (útil para tests).
pub fn record_bitwig_drop_to(
    inbox_path: &Path,
    dropped_file: &Path,
    source: &str,
) -> Result<BitwigDropRecord, String> {
    if !is_supported_bitwig_artifact(dropped_file) {
        return Err(format!(
            "extensión no soportada para inspección Bitwig: {}",
            dropped_file.display()
        ));
    }

    if let Some(parent) = inbox_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "no se pudo crear el directorio del buzón {}: {err}",
                parent.display()
            )
        })?;
    }

    let file_name = dropped_file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    let extension = dropped_file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let dropped_at_unix_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let record = BitwigDropRecord {
        path: dropped_file.to_path_buf(),
        file_name,
        extension,
        dropped_at_unix_secs,
        source: source.to_string(),
    };

    let mut inbox = load_bitwig_inbox(inbox_path);
    inbox.entries.push(record.clone());
    if inbox.entries.len() > MAX_INBOX_ENTRIES {
        let excess = inbox.entries.len() - MAX_INBOX_ENTRIES;
        inbox.entries.drain(0..excess);
    }

    let json = serde_json::to_string_pretty(&inbox)
        .map_err(|err| format!("error serializando buzón Bitwig: {err}"))?;
    fs::write(inbox_path, json).map_err(|err| {
        format!(
            "no se pudo escribir el buzón Bitwig {}: {err}",
            inbox_path.display()
        )
    })?;

    Ok(record)
}

/// Devuelve la entrada más reciente del buzón, si hay alguna.
pub fn latest_bitwig_drop(inbox_path: &Path) -> Option<BitwigDropRecord> {
    load_bitwig_inbox(inbox_path).entries.pop()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filtra_extensiones_bitwig_soportadas() {
        assert!(is_supported_bitwig_artifact(Path::new("/tmp/Bit-8.bwdevice")));
        assert!(is_supported_bitwig_artifact(Path::new("/tmp/Default.BWPRESET")));
        assert!(!is_supported_bitwig_artifact(Path::new("/tmp/kick.wav")));
    }

    #[test]
    fn registra_y_recupera_ultimo_drop() {
        let inbox = std::env::temp_dir().join(format!(
            "hikaru_bw_inbox_test_{}.json",
            std::process::id()
        ));
        let _ = fs::remove_file(&inbox);

        record_bitwig_drop_to(&inbox, Path::new("/tmp/Comb.bwdevice"), "gui-drop")
            .expect("registra bwdevice");
        record_bitwig_drop_to(&inbox, Path::new("/tmp/Default.bwpreset"), "gui-drop")
            .expect("registra bwpreset");

        let loaded = load_bitwig_inbox(&inbox);
        assert_eq!(loaded.entries.len(), 2);
        let latest = latest_bitwig_drop(&inbox).expect("hay último drop");
        assert_eq!(latest.file_name, "Default.bwpreset");
        assert_eq!(latest.extension, "bwpreset");

        let _ = fs::remove_file(&inbox);
    }
}
