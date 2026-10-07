// Primitivas compartidas por los formatos de proyecto de Hikaru.
// SPDX-License-Identifier: LGPL-3.0-only
// crates/hikaru_project_file/src/lib.rs
//
// =========================================================================
// POR QUÉ ESTE CRATE ES LGPL Y NO AGPL
// =========================================================================
//
// El ejecutable principal de Hikaru OpenLive/OpenStudio y su motor son AGPLv3.
// Los formatos de archivo, en cambio, son LGPLv3, y la razón es de
// interoperabilidad, no estética:
//
// Un DAW de CÓDIGO CERRADO (Image-Line, Ableton, Bitwig) que quiera leer o
// escribir proyectos `.oplf`/`.opsf` no debería tener que abrir el código del
// motor. Con LGPLv3 alcanza con que use esta librería tal cual, sin tocar el
// código AGPL del DAW propietario; si necesita MODIFICAR el formato, tiene que
// publicar sus cambios al formato bajo LGPLv3, y sólo a esa parte.
//
// La frontera se sostiene mientras el código del FORMATO no dependa del motor:
// si un struct del formato arrastrara un tipo de GPUI o del secuenciador, el
// "formato" dejaría de ser una librería y volvería a ser parte de la app
// AGPL. Por eso este crate depende sólo de `serde`.
//
// Ver docs/hikaru-project-files/hikaru-openlive-file/OPLF-README.md §4.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Versión del formato de archivo.
///
/// Sube cuando cambie la FORMA del JSON (un campo nuevo obligatorio, un tipo
/// distinto). Un archivo con versión mayor se rechaza al abrir en vez de
/// interpretarse a medias, que es como se corrompen los proyectos.
///
/// NO confundirse con la versión del programa: esa va informativa en la
/// cabecera (`created_by`) y la resuelve el binario, no la librería.
pub const FORMAT_VERSION: u32 = 1;

/// Errores de lectura/escritura de un proyecto.
#[derive(Debug)]
pub enum ProjectError {
    Io(std::io::Error),
    /// JSON inválido o con campos cuyo tipo no coincide con el del formato.
    Parse(serde_json::Error),
    /// El archivo es de otra versión del formato.
    UnsupportedVersion { found: u32, expected: u32 },
    /// La extensión no es de ningún formato de Hikaru, o el contenido no es
    /// JSON con la cabecera que corresponde.
    NotAProject(PathBuf),
}

impl std::fmt::Display for ProjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProjectError::Io(e) => write!(f, "no se pudo escribir el proyecto: {}", e),
            ProjectError::Parse(e) => write!(f, "el archivo no es un proyecto válido: {}", e),
            ProjectError::UnsupportedVersion { found, expected } => write!(
                f,
                "el proyecto es de la versión {} y esta build sólo lee hasta la {}",
                found, expected
            ),
            ProjectError::NotAProject(p) => write!(
                f,
                "{} no parece un proyecto de Hikaru (se esperaba un .oplf o un .opsf)",
                p.display()
            ),
        }
    }
}

impl From<std::io::Error> for ProjectError {
    fn from(e: std::io::Error) -> Self {
        ProjectError::Io(e)
    }
}

impl From<serde_json::Error> for ProjectError {
    fn from(e: serde_json::Error) -> Self {
        ProjectError::Parse(e)
    }
}

impl std::error::Error for ProjectError {}

// =========================================================================
// CANAL DE MEZCLA
// =========================================================================

/// Un canal de mezcla, tal como aparece en `live_tracks` (`.oplf`) y en
/// `studio_tracks` (`.opsf`).
///
/// Es el MISMO tipo en los dos formatos a propósito: el motor tiene un solo
/// modelo de bus, y duplicar la struct obligaría a mantener dos conversores
/// idénticos que divergen en seis meses. Lo que cambia entre modos no es el
/// canal sino QUIÉN lo lista: la fila de la matriz o la pista del arranger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: usize,
    pub name: String,
    pub volume: f32,
    pub pan: f32,
    /// 0 = estéreo, 1 = mid/side.
    pub pan_mode: u8,
    pub mute: bool,
    pub solo: bool,
    pub arm: bool,
    pub is_master: bool,
    pub route_destination_id: usize,
    pub sends: Vec<Send>,
    pub effects: Vec<DspSlot>,
    /// Índice de fila en la Session Matrix, o `None` para el master. Sólo
    /// tiene sentido en `.oplf`; en un `.opsf` viene `None` y no se usa.
    pub matrix_idx: Option<usize>,
}

/// Envío a otro bus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Send {
    pub target_id: usize,
    pub amount: f32,
}

/// Un slot del rack de DSP.
///
/// OJO con el alcance: se guardan identidad y bypass (`id`, `name`, `active`),
/// NO el contenido del sintetizador. La tabla de wavetable sí se referencia por
/// ruta ([`DspSlot::wavetable_path`]); el resto de los parámetros del panel
/// todavía no entra en el formato. Cuando entren, es agregar campos acá.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DspSlot {
    pub id: usize,
    pub name: String,
    pub active: bool,
    pub wavetable_path: Option<String>,
}

// =========================================================================
// COLOR
// =========================================================================

/// Color en HSLA plano.
///
/// Deliberadamente NO es el `Hsla` de GPUI: un formato de archivo que depende
/// de la librería de UI de la app deja de ser una librería y vuelve a ser parte
/// del programa AGPL. El consumidor decide cómo convertirlo.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Color {
    pub h: f32,
    pub s: f32,
    pub l: f32,
    pub a: f32,
}

// =========================================================================
// I/O
// =========================================================================

/// Escribe JSON lindo a propósito.
///
/// Los `.oplf`/`.opsf` están pensados para leerse y diffearse a ojo, y pesan
/// kilobytes (no megabytes, porque el audio va por ruta).
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<PathBuf, ProjectError> {
    let json = serde_json::to_string_pretty(value)?;
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    std::fs::write(path, json)?;
    Ok(path.to_path_buf())
}

/// Lee el texto y valida la cabecera antes de deserializar el cuerpo entero.
///
/// La versión se mira ANTES que el resto: si el archivo es de otra generación,
/// el error tiene que ser "versión" y no un `missing field` de serde que no
/// dice nada.
pub fn parse_with_probe<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, ProjectError> {
    let text = std::fs::read_to_string(path)?;

    #[derive(Deserialize)]
    struct VersionProbe {
        format_version: Option<u32>,
    }
    let probe: VersionProbe = serde_json::from_str(&text)
        .map_err(|_| ProjectError::NotAProject(path.to_path_buf()))?;
    let version = probe
        .format_version
        .ok_or_else(|| ProjectError::NotAProject(path.to_path_buf()))?;
    if version > FORMAT_VERSION {
        return Err(ProjectError::UnsupportedVersion {
            found: version,
            expected: FORMAT_VERSION,
        });
    }

    let value: T = serde_json::from_str(&text)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_json_sin_cabecera_no_es_un_proyecto() {
        let mut path = std::env::temp_dir();
        path.push(format!("hikaru_sonda_{}.json", std::process::id()));
        std::fs::write(&path, b"RIFF....WAVE esto es audio").unwrap();

        let err = parse_with_probe::<Track>(&path);
        assert!(
            matches!(err, Err(ProjectError::NotAProject(_))),
            "un WAV tiene que rechazarse como proyecto, no parsearse a medias"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn una_version_futura_se_rechaza_antes_de_parsear() {
        let mut path = std::env::temp_dir();
        path.push(format!("hikaru_futuro_{}.json", std::process::id()));
        std::fs::write(&path, format!("{{ \"format_version\": {} }}", FORMAT_VERSION + 1))
            .unwrap();

        match parse_with_probe::<Track>(&path) {
            Err(ProjectError::UnsupportedVersion { found, expected }) => {
                assert_eq!(found, FORMAT_VERSION + 1);
                assert_eq!(expected, FORMAT_VERSION);
            }
            other => panic!("error inesperado: {:?}", other),
        }
        let _ = std::fs::remove_file(&path);
    }
}