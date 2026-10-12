// Copyright (C) Hikaru Corporation - 2026
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/src/registry.rs
//
// Descubrimiento de plugins VST3/CLAP y su mapeo a la Metadata unificada que
// consume el DSP Rack.
//
// Este módulo NO carga ni ejecuta nada: sólo camina directorios y produce
// [`PluginDescriptor`]. Esa separación es deliberada y es la base de la
// resiliencia: escanear nunca puede hacer caer el DAW, porque un binario roto,
// un bundle a medio copiar o un symlink colgado se reportan como
// [`PluginScanError`], nunca como panic.
//
// LaMetadata que sale de acá es "flat" a propósito (nombre/vendor/ruta/formato/
// tipo). El `DspSlot` la guarda y la muestra; el `PluginInstance` real se
// construye aparte, recién cuando el usuario elige el plugin. Así un proyecto
// guardado puede abrir y *mostrar* sus plugins aunque uno no esté instalado,
// en vez de romperse al deserializar.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::PluginFormat;

/// Qué hace un plugin con el audio. Determina si puede ir en una cadena de
/// efectos o si necesita recibir MIDI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginKind {
    Effect,
    Instrument,
}

/// Metadata unificada de un plugin descubierto en disco.
///
/// Es `Clone + PartialEq` a propósito: el estado de la GUI la copia por
/// frame al renderizar, y el `DspSlot` la compara para saber si un slot apunta
/// al mismo plugin que antes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDescriptor {
    /// Clave estable y única dentro de un escaneo. Se usa como identidad en el
    /// `DspSlot` y como clave de deduplicación, así que **no** debe cambiar
    /// entre sesiones ni depender del orden de `read_dir` (que es arbitrario).
    /// Se deriva de `formato:ruta_canónica` de abajo.
    pub id: String,
    /// Nombre a mostrar. Sale de la metadata del binario cuando se puede leer
    /// sin instanciar el plugin, y del nombre del archivo como respaldo.
    pub name: String,
    pub vendor: String,
    /// Ruta al *bundle* (VST3: el directorio `.vst3`) o al `.so` (CLAP).
    /// Es la ruta que se le pasa al cargador.
    pub path: PathBuf,
    pub format: PluginFormat,
    pub kind: PluginKind,
    /// Si el plugin declara editor. `false` significa que el slot no va a
    /// ofrecer botón de abrir editor, pero el audio sigue funcionando.
    pub has_gui: bool,
    /// Cantidad de canales de entrada que declara el plugin. `None` cuando no
    /// se pudo determinar sin cargarlo.
    pub input_channels: Option<usize>,
}

impl PluginDescriptor {
    /// Etiqueta compacta para el menú del rack: "FabFilter — Pro-Q 3".
    pub fn display_label(&self) -> String {
        if self.vendor.is_empty() {
            self.name.clone()
        } else {
            format!("{} — {}", self.vendor, self.name)
        }
    }
}

/// Un directorio que no se pudo leer, o un candidato que no se pudo inspeccionar.
///
/// Se acumulan en vez de propagarse: un solo bundle corrupto no puede abortar
/// el escaneo de los otros trescientos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginScanError {
    pub path: PathBuf,
    pub reason: String,
}

impl std::fmt::Display for PluginScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.reason)
    }
}

/// Resultado completo de un escaneo: lo que se encontró y lo que falló.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanReport {
    pub plugins: Vec<PluginDescriptor>,
    pub errors: Vec<PluginScanError>,
}

impl ScanReport {
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }
}

/// Directorios donde Linux estándar deja los plugins.
///
/// El orden importa: es la prioridad de resolución si el mismo plugin aparece
/// en más de un lugar. El del usuario gana sobre el del sistema, así que
/// *reemplazar* un plugin del sistema se logra dejando el nuevo en `~/.vst3`.
pub fn standard_search_paths() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut paths = Vec::new();

    if let Some(home) = home.as_ref() {
        // VST3: bundles en el home.
        paths.push(home.join(".vst3"));
    }
    // VST3: del sistema. Sólo en Unix; en Windows/macOS no aplica.
    if cfg!(unix) {
        paths.push(PathBuf::from("/usr/lib/vst3"));
    }
    if let Some(home) = home.as_ref() {
        // CLAP: hay dos convenciones y ambas están en uso real (DistroDSP y
        // builds caseros las usan indistintamente), así que escaneamos las dos.
        paths.push(home.join(".clap"));
        paths.push(home.join(".local/lib/clap"));
    }
    if cfg!(unix) {
        paths.push(PathBuf::from("/usr/lib/clap"));
    }

    paths
}

/// `id` estable para un plugin. Canonicamos la ruta para que `~/.vst3` y un
/// symlink al mismo archivo no produzcan dos entradas distintas.
fn descriptor_id(format: PluginFormat, path: &Path) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let tag = match format {
        PluginFormat::VST3 => "vst3",
        PluginFormat::CLAP => "clap",
    };
    format!("{tag}:{}", canonical.display())
}

/// Escanea `paths` en orden de prioridad y devuelve el reporte.
///
/// Nunca entra en panic: cualquier error de I/O queda en
/// [`ScanReport::errors`].
pub fn scan_paths<I, P>(paths: I) -> ScanReport
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let mut report = ScanReport::default();
    // Dedup por id. Gana el primer camino que produjo un plugin dado, porque
    // `paths` viene en orden de prioridad.
    let mut seen: BTreeMap<String, ()> = BTreeMap::new();

    for path in paths {
        let path = path.as_ref();
        if !path.exists() {
            // Un directorio configurado que no existe es normal (el usuario
            // puede no tener CLAP instalado). No es un error que reportar.
            continue;
        }

        let mut candidates = Vec::new();
        collect_candidates(path, &mut candidates, &mut report.errors);

        for candidate in candidates {
            let id = descriptor_id(candidate.format, &candidate.path);
            if seen.contains_key(&id) {
                continue;
            }
            match inspect_candidate(candidate) {
                Ok(descriptor) => {
                    seen.insert(descriptor.id.clone(), ());
                    report.plugins.push(descriptor);
                }
                Err(err) => report.errors.push(err),
            }
        }
    }

    // Orden estable por nombre para que el menú del rack no se reordene entre
    // escaneos (read_dir no garantiza orden).
    report.plugins.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });

    report
}

/// Atajo: escanea [`standard_search_paths`].
pub fn scan_standard_paths() -> ScanReport {
    scan_paths(standard_search_paths())
}

/// Un `.so` que podría ser un plugin, aún sin inspeccionar.
#[derive(Debug, Clone)]
struct Candidate {
    path: PathBuf,
    format: PluginFormat,
}

const MAX_SCAN_DEPTH: usize = 3;

/// Camina `dir` (hasta [`MAX_SCAN_DEPTH`] niveles) y acumula candidatas.
///
/// El límite de profundidad es lo que hace que esto sea seguro de correr sobre
/// `/usr/lib`: los bundles VST3 son directorios, así que sin tope una carpeta
/// con datos del usuario podría colgarse en un árbol profundo.
/// Acumula candidatas y errores sin propagarlos.
///
/// Que `walk` no devuelva `Result` es deliberado: si un subdirectorio falla, el
/// resto del árbol se sigue recorriendo y el problema queda anotado en
/// `errors`. Abortar en el primer `read_dir` fallido perdería todos los plugins
/// que sí están bien.
fn collect_candidates(
    dir: &Path,
    out: &mut Vec<Candidate>,
    errors: &mut Vec<PluginScanError>,
) {
    walk(dir, 0, out, errors);
}

fn walk(
    dir: &Path,
    depth: usize,
    out: &mut Vec<Candidate>,
    errors: &mut Vec<PluginScanError>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            errors.push(PluginScanError {
                path: dir.to_path_buf(),
                reason: format!("read_dir: {e}"),
            });
            return;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                // Una entrada ilegible no debe abortar el directorio entero.
                errors.push(PluginScanError {
                    path: dir.to_path_buf(),
                    reason: format!("entrada ilegible: {e}"),
                });
                continue;
            }
        };
        let path = entry.path();
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        // Tope de profundidad: sin esto, un bundle VST3 mal formado o un
        // symlink a `/` nos recorería el sistema de archivos entero.
        if path.is_dir() {
            // Bundle VST3 (`Foo.vst3`) o directorio intermedio dentro de él
            // (`Contents/x86_64-linux`). Ambos se recorren igual.
            //
            // El corte de profundidad va sólo sobre la recursión, no sobre la
            // lectura de archivos de ese nivel. Un bundle real es
            // root/<Foo>.vst3/Contents/<arch>/<x>.so: tres descensos hasta el
            // directorio que contiene el binario, y el `.so` se lee ahí. Si el
            // corte alcanzara también a los archivos de ese nivel interno,
            // ningún bundle se indexaría nunca.
            if depth < MAX_SCAN_DEPTH {
                walk(&path, depth + 1, out, errors);
            }
            continue;
        }

        // Archivos sueltos. Un `.so` suelto en un directorio de plugins es la
        // convención de CLAP en la práctica.
        let ext = path.extension().map(|e| e.to_str().unwrap_or("")).unwrap_or("");
        let format = match ext {
            "so" | "clap" => PluginFormat::CLAP,
            "vst3" => PluginFormat::VST3,
            _ => continue,
        };

        // Ignoramos el bundle vacío: `Foo.vst3/` sin `Contents` no es un plugin.
        if file_name.ends_with(".vst3") {
            errors.push(PluginScanError {
                path: path.clone(),
                reason: "bundle .vst3 sin binario dentro".to_string(),
            });
            continue;
        }

        out.push(Candidate { path, format });
    }
}

/// Rutea un candidato a un [`PluginDescriptor`] o a un error.
///
/// Deliberadamente **no** instancia el plugin. Instanciar aquí significa
/// ejecutar `libmain` de un binario desconocido durante un escaneo, en un hilo
/// cualquiera: justo lo que produce cuelgues y leaks de display servers. Por
/// eso el `name` cae al nombre de archivo, y por eso `has_gui` / `input_channels`
/// se quedan en `None`/`false` hasta que el usuario carga el plugin de verdad.
fn inspect_candidate(candidate: Candidate) -> Result<PluginDescriptor, PluginScanError> {
    let Candidate { path, format } = candidate;

    // Un `.so` o bundle vacío no sirve. Un binario de 0 bytes es el caso
    // típico de una descarga interrumpida.
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.is_file() && meta.len() == 0 {
            return Err(PluginScanError {
                path,
                reason: "archivo vacío (descarga interrumpida?)".to_string(),
            });
        }
    }

    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| format!("{:?}", format));

    // Para un bundle `.vst3` el `file_stem` es "Foo.vst3" -> "Foo"; el nombre
    // legible suele estar un nivel más adentro.
    let name = bundle_display_name(&path, format).unwrap_or(stem);

    Ok(PluginDescriptor {
        id: descriptor_id(format, &path),
        name,
        vendor: String::new(),
        path,
        format,
        // Por defecto asumimos efecto: es el caso mayoritario y el slot puede
        // siempre recibir MIDI igualmente. Se reclasifica al cargar.
        kind: PluginKind::Effect,
        has_gui: false,
        input_channels: None,
    })
}

/// Busca un nombre más legible dentro de un bundle VST3.
///
/// Un bundle típico es `FabFilter.vst3/Contents/x86_64-linux/FabFilter Pro-Q 3.so`,
/// así que el `.so` interno es el mejor nombre disponible sin cargar nada.
fn bundle_display_name(path: &Path, format: PluginFormat) -> Option<String> {
    match format {
        PluginFormat::CLAP => None,
        PluginFormat::VST3 => {
            if !path.is_dir() {
                return None;
            }
            let contents = path.join("Contents");
            let arch = std::fs::read_dir(&contents).ok()?;
            for dir in arch.flatten() {
                let dir_path = dir.path();
                if !dir_path.is_dir() {
                    continue;
                }
                let inner = std::fs::read_dir(&dir_path).ok()?;
                for file in inner.flatten() {
                    let p = file.path();
                    if p.extension().map(|e| e == "so").unwrap_or(false) {
                        if let Some(stem) = p.file_stem() {
                            let name = stem.to_string_lossy().to_string();
                            // Un `.so` genérico tipo "Plugin.so" no le gana al
                            // nombre del bundle.
                            if name != "Plugin" && !name.is_empty() {
                                return Some(name);
                            }
                        }
                    }
                }
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hikaru_scan_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scan_finds_clap_and_vst3_bundles() {
        let root = tmpdir("mixed");

        // CLAP plano.
        std::fs::write(root.join("TAL-Reverb.clap"), b"\x7fELF....").unwrap();
        // Ruido que debe ignorarse.
        std::fs::write(root.join("leeme.txt"), b"hola").unwrap();

        // Bundle VST3 anidado como en Linux real.
        let bundle = root.join("FabFilter.vst3/Contents/x86_64-linux");
        std::fs::create_dir_all(&bundle).unwrap();
        std::fs::write(bundle.join("FabFilter Pro-Q 3.so"), b"\x7fELF....").unwrap();

        let report = scan_paths([root.clone()]);
        let names: Vec<&str> = report.plugins.iter().map(|p| p.name.as_str()).collect();

        assert!(report.errors.is_empty(), "errores inesperados: {:?}", report.errors);
        assert!(names.contains(&"FabFilter Pro-Q 3"), "no vio el VST3: {names:?}");
        // El nombre sale del `file_stem`, así que ya no trae la extensión.
        assert!(names.contains(&"TAL-Reverb"), "no vio el CLAP: {names:?}");
        assert!(!names.contains(&"leeme.txt"));
    }

    #[test]
    fn empty_binary_is_reported_not_indexed() {
        let root = tmpdir("empty");
        std::fs::write(root.join("Roto.so"), b"").unwrap();

        let report = scan_paths([root.clone()]);

        assert!(report.plugins.is_empty(), "un .so vacío no debe indexarse");
        assert_eq!(report.errors.len(), 1);
        assert!(report.errors[0].reason.contains("vacío"));
    }

    #[test]
    fn unreadable_root_does_not_panic() {
        let report = scan_paths([PathBuf::from("/proc/1/mem/does-not-exist")]);
        assert!(report.plugins.is_empty());
    }

    #[test]
    fn dedupes_same_plugin_via_two_paths() {
        let root = tmpdir("dedup");
        let real = root.join("Real.so");
        std::fs::write(&real, b"\x7fELF....").unwrap();
        let link = root.join("Alias.so");
        let _ = std::os::unix::fs::symlink(&real, &link);

        let report = scan_paths([root.clone()]);
        assert_eq!(report.plugin_count(), 1, "el symlink duplicó el plugin");
    }

    #[test]
    fn standard_paths_cover_linux_conventions() {
        let paths = standard_search_paths();
        let joined: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
        let all = joined.join(" ");

        assert!(all.contains(".vst3"), "falta ~/.vst3: {all}");
        assert!(all.contains("/usr/lib/vst3"), "falta /usr/lib/vst3: {all}");
        assert!(all.contains(".clap"), "falta ~/.clap: {all}");
        assert!(all.contains(".local/lib/clap"), "falta ~/.local/lib/clap: {all}");
    }
}