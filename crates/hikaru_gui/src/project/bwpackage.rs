// Parser, indexador e instalador clean-room para contenedores `.bwpackage` de Bitwig Studio.
// GNU AGPLv3
// crates/hikaru_gui/src/project/bwpackage.rs
//
// Implementa el soporte completo para `.bwpackage`:
// - bpk-01: Lector de archivo ZIP y parser de metadatos (`package.json` / `manifest.json` / `package.xml`).
// - bpk-02: Indexación de samples, presets, clips y multisamples del paquete para el Browser.
// - bpk-03: Instalación en librería local, resolución de dependencias y de `referenced_packaged_file_ids`.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use quick_xml::events::Event;
use quick_xml::Reader;
use serde_json::{json, Value};

use crate::project::dawproject::{build_zip_store, read_zip_entries};

/// Extensión del formato de Sound Package nativo de Bitwig Studio.
pub const BWPACKAGE_EXTENSION: &str = "bwpackage";

const PACKAGE_INDEX_FILE: &str = ".hikaru-package-index.json";

/// Devuelve `true` si la ruta corresponde a un archivo `.bwpackage`.
pub fn is_bwpackage_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| ext.eq_ignore_ascii_case(BWPACKAGE_EXTENSION))
        .unwrap_or(false)
}

/// Clasificación de un recurso contenido dentro de un `.bwpackage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BwPackageAssetKind {
    Preset,
    Clip,
    Device,
    Module,
    Modulator,
    Multisample,
    Wavetable,
    RemoteControls,
    Audio,
    Midi,
    Other,
}

impl BwPackageAssetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preset => "preset",
            Self::Clip => "clip",
            Self::Device => "device",
            Self::Module => "module",
            Self::Modulator => "modulator",
            Self::Multisample => "multisample",
            Self::Wavetable => "wavetable",
            Self::RemoteControls => "remote_controls",
            Self::Audio => "audio",
            Self::Midi => "midi",
            Self::Other => "other",
        }
    }

    pub fn from_str_kind(s: &str) -> Self {
        match s {
            "preset" => Self::Preset,
            "clip" => Self::Clip,
            "device" => Self::Device,
            "module" => Self::Module,
            "modulator" => Self::Modulator,
            "multisample" => Self::Multisample,
            "wavetable" => Self::Wavetable,
            "remote_controls" => Self::RemoteControls,
            "audio" => Self::Audio,
            "midi" => Self::Midi,
            _ => Self::Other,
        }
    }

    pub fn from_path(path: &str) -> Self {
        let ext = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "bwpreset" => Self::Preset,
            "bwclip" => Self::Clip,
            "bwdevice" => Self::Device,
            "bwmodule" => Self::Module,
            "bwmodulator" => Self::Modulator,
            "multisample" => Self::Multisample,
            "wt" => Self::Wavetable,
            "bwremotecontrols" => Self::RemoteControls,
            "wav" | "flac" | "ogg" | "mp3" | "aiff" | "aif" => Self::Audio,
            "mid" | "midi" => Self::Midi,
            _ => Self::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Preset => "Preset",
            Self::Clip => "Clip",
            Self::Device => "Device",
            Self::Module => "Grid Module",
            Self::Modulator => "Modulator",
            Self::Multisample => "Multisample",
            Self::Wavetable => "Wavetable",
            Self::RemoteControls => "Remote Controls",
            Self::Audio => "Audio Sample",
            Self::Midi => "MIDI",
            Self::Other => "Asset",
        }
    }
}

/// Dependencia declarada por un `.bwpackage` hacia otro paquete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BwPackageDependency {
    pub id: String,
    pub name: Option<String>,
    pub min_version: Option<String>,
}

/// Entrada indexada de un recurso dentro de un `.bwpackage`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BwPackageAsset {
    pub path_in_zip: String,
    pub file_name: String,
    pub kind: BwPackageAssetKind,
    pub size_bytes: usize,
    pub asset_id: Option<String>,
}

/// Metadatos del paquete `.bwpackage`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BwPackageManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub creator: String,
    pub category: String,
    pub description: String,
    pub tags: Vec<String>,
    pub dependencies: Vec<BwPackageDependency>,
}

impl Default for BwPackageManifest {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: "Bitwig Sound Package".to_string(),
            version: "1.0.0".to_string(),
            creator: "Bitwig".to_string(),
            category: "Sound Package".to_string(),
            description: String::new(),
            tags: Vec::new(),
            dependencies: Vec::new(),
        }
    }
}

/// Documento `.bwpackage` completo parseado en memoria.
#[derive(Debug, Clone)]
pub struct BwPackageDocument {
    pub manifest: BwPackageManifest,
    pub assets: Vec<BwPackageAsset>,
    pub raw_entries: HashMap<String, Vec<u8>>,
}

/// Índice resumido de un paquete instalado en disco.
#[derive(Debug, Clone)]
pub struct InstalledBwPackageIndex {
    pub manifest: BwPackageManifest,
    pub assets: Vec<BwPackageAsset>,
}

impl InstalledBwPackageIndex {
    fn to_json_value(&self) -> Value {
        json!({
            "manifest": {
                "id": self.manifest.id,
                "name": self.manifest.name,
                "version": self.manifest.version,
                "creator": self.manifest.creator,
                "category": self.manifest.category,
                "description": self.manifest.description,
                "tags": self.manifest.tags,
                "dependencies": self.manifest.dependencies.iter().map(|d| json!({
                    "id": d.id,
                    "name": d.name,
                    "min_version": d.min_version,
                })).collect::<Vec<_>>(),
            },
            "assets": self.assets.iter().map(|a| json!({
                "path_in_zip": a.path_in_zip,
                "file_name": a.file_name,
                "kind": a.kind.as_str(),
                "size_bytes": a.size_bytes,
                "asset_id": a.asset_id,
            })).collect::<Vec<_>>(),
        })
    }

    fn from_json_bytes(bytes: &[u8]) -> Option<Self> {
        let v: Value = serde_json::from_slice(bytes).ok()?;
        let m_obj = v.get("manifest")?;
        let mut manifest = BwPackageManifest::default();
        if let Some(s) = m_obj.get("id").and_then(|x| x.as_str()) {
            manifest.id = s.to_string();
        }
        if let Some(s) = m_obj.get("name").and_then(|x| x.as_str()) {
            manifest.name = s.to_string();
        }
        if let Some(s) = m_obj.get("version").and_then(|x| x.as_str()) {
            manifest.version = s.to_string();
        }
        if let Some(s) = m_obj.get("creator").and_then(|x| x.as_str()) {
            manifest.creator = s.to_string();
        }
        if let Some(s) = m_obj.get("category").and_then(|x| x.as_str()) {
            manifest.category = s.to_string();
        }
        if let Some(s) = m_obj.get("description").and_then(|x| x.as_str()) {
            manifest.description = s.to_string();
        }
        if let Some(arr) = m_obj.get("tags").and_then(|x| x.as_array()) {
            manifest.tags = arr
                .iter()
                .filter_map(|t| t.as_str().map(|s| s.to_string()))
                .collect();
        }
        if let Some(arr) = m_obj.get("dependencies").and_then(|x| x.as_array()) {
            for d in arr {
                if let Some(id) = d.get("id").and_then(|x| x.as_str()) {
                    manifest.dependencies.push(BwPackageDependency {
                        id: id.to_string(),
                        name: d.get("name").and_then(|x| x.as_str()).map(|s| s.to_string()),
                        min_version: d.get("min_version").and_then(|x| x.as_str()).map(|s| s.to_string()),
                    });
                }
            }
        }
        let mut assets = Vec::new();
        if let Some(arr) = v.get("assets").and_then(|x| x.as_array()) {
            for a in arr {
                if let Some(path_in_zip) = a.get("path_in_zip").and_then(|x| x.as_str()) {
                    assets.push(BwPackageAsset {
                        path_in_zip: path_in_zip.to_string(),
                        file_name: a
                            .get("file_name")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                        kind: a
                            .get("kind")
                            .and_then(|x| x.as_str())
                            .map(BwPackageAssetKind::from_str_kind)
                            .unwrap_or(BwPackageAssetKind::Other),
                        size_bytes: a.get("size_bytes").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
                        asset_id: a.get("asset_id").and_then(|x| x.as_str()).map(|s| s.to_string()),
                    });
                }
            }
        }
        Some(Self { manifest, assets })
    }
}

/// Resultado de instalar un `.bwpackage` en una librería local.
#[derive(Debug, Clone)]
pub struct BwPackageInstallReport {
    pub package_id: String,
    pub package_name: String,
    pub version: String,
    pub install_dir: PathBuf,
    pub extracted_files: Vec<PathBuf>,
    pub asset_index: Vec<BwPackageAsset>,
    pub resolved_dependencies: Vec<String>,
    pub missing_dependencies: Vec<BwPackageDependency>,
}

fn is_manifest_entry(name: &str) -> bool {
    let lower = name.trim_start_matches("./").to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "package.json"
            | "manifest.json"
            | "metadata.json"
            | "info.json"
            | "package.xml"
            | "manifest.xml"
    )
}

fn parse_json_manifest(bytes: &[u8]) -> Result<BwPackageManifest, String> {
    let raw: Value =
        serde_json::from_slice(bytes).map_err(|e| format!("JSON de paquete inválido: {e}"))?;
    let mut m = BwPackageManifest::default();
    if let Some(id) = raw
        .get("id")
        .or_else(|| raw.get("uuid"))
        .and_then(|x| x.as_str())
    {
        if !id.trim().is_empty() {
            m.id = id.trim().to_string();
        }
    }
    if let Some(name) = raw
        .get("name")
        .or_else(|| raw.get("title"))
        .and_then(|x| x.as_str())
    {
        if !name.trim().is_empty() {
            m.name = name.trim().to_string();
        }
    }
    if let Some(ver) = raw.get("version").and_then(|x| x.as_str()) {
        if !ver.trim().is_empty() {
            m.version = ver.trim().to_string();
        }
    }
    if let Some(creator) = raw
        .get("creator")
        .or_else(|| raw.get("author"))
        .and_then(|x| x.as_str())
    {
        if !creator.trim().is_empty() {
            m.creator = creator.trim().to_string();
        }
    }
    if let Some(cat) = raw.get("category").and_then(|x| x.as_str()) {
        if !cat.trim().is_empty() {
            m.category = cat.trim().to_string();
        }
    }
    if let Some(desc) = raw.get("description").and_then(|x| x.as_str()) {
        m.description = desc.trim().to_string();
    }
    if let Some(tags) = raw.get("tags").and_then(|x| x.as_array()) {
        m.tags = tags
            .iter()
            .filter_map(|t| t.as_str().map(|s| s.trim().to_string()))
            .filter(|t| !t.is_empty())
            .collect();
    }
    if let Some(deps) = raw.get("dependencies").and_then(|x| x.as_array()) {
        for d in deps {
            if let Some(id) = d.as_str().filter(|s| !s.trim().is_empty()) {
                m.dependencies.push(BwPackageDependency {
                    id: id.trim().to_string(),
                    name: None,
                    min_version: None,
                });
            } else if let Some(dep_id) = d
                .get("id")
                .or_else(|| d.get("package_id"))
                .and_then(|x| x.as_str())
                .filter(|s| !s.trim().is_empty())
            {
                m.dependencies.push(BwPackageDependency {
                    id: dep_id.trim().to_string(),
                    name: d.get("name").and_then(|x| x.as_str()).map(|s| s.to_string()),
                    min_version: d
                        .get("minVersion")
                        .or_else(|| d.get("min_version"))
                        .or_else(|| d.get("version"))
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string()),
                });
            }
        }
    }
    Ok(m)
}

fn parse_xml_manifest(bytes: &[u8]) -> Result<BwPackageManifest, String> {
    let xml_str =
        std::str::from_utf8(bytes).map_err(|e| format!("XML de paquete no UTF-8: {e}"))?;
    let mut reader = Reader::from_str(xml_str);
    reader.config_mut().trim_text(true);
    let mut m = BwPackageManifest::default();
    let mut current_tag: Option<String> = None;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_ascii_lowercase();
                if tag == "package" || tag == "manifest" {
                    for attr in e.attributes().flatten() {
                        let k = String::from_utf8_lossy(attr.key.as_ref()).to_ascii_lowercase();
                        if let Ok(v) = attr.normalized_value(Default::default()) {
                            let val = v.into_owned();
                            match k.as_str() {
                                "id" | "uuid" if !val.trim().is_empty() => m.id = val,
                                "name" | "title" if !val.trim().is_empty() => m.name = val,
                                "version" if !val.trim().is_empty() => m.version = val,
                                _ => {}
                            }
                        }
                    }
                } else if tag == "dependency" {
                    let mut dep_id = None;
                    let mut dep_name = None;
                    let mut min_ver = None;
                    for attr in e.attributes().flatten() {
                        let k = String::from_utf8_lossy(attr.key.as_ref()).to_ascii_lowercase();
                        if let Ok(v) = attr.normalized_value(Default::default()) {
                            let val = v.into_owned();
                            match k.as_str() {
                                "id" | "uuid" => dep_id = Some(val),
                                "name" => dep_name = Some(val),
                                "version" | "min-version" => min_ver = Some(val),
                                _ => {}
                            }
                        }
                    }
                    if let Some(id) = dep_id.filter(|s| !s.trim().is_empty()) {
                        m.dependencies.push(BwPackageDependency {
                            id,
                            name: dep_name,
                            min_version: min_ver,
                        });
                    }
                }
                current_tag = Some(tag);
            }
            Ok(Event::Empty(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_ascii_lowercase();
                if tag == "dependency" {
                    let mut dep_id = None;
                    let mut dep_name = None;
                    let mut min_ver = None;
                    for attr in e.attributes().flatten() {
                        let k = String::from_utf8_lossy(attr.key.as_ref()).to_ascii_lowercase();
                        if let Ok(v) = attr.normalized_value(Default::default()) {
                            let val = v.into_owned();
                            match k.as_str() {
                                "id" | "uuid" => dep_id = Some(val),
                                "name" => dep_name = Some(val),
                                "version" | "min-version" => min_ver = Some(val),
                                _ => {}
                            }
                        }
                    }
                    if let Some(id) = dep_id.filter(|s| !s.trim().is_empty()) {
                        m.dependencies.push(BwPackageDependency {
                            id,
                            name: dep_name,
                            min_version: min_ver,
                        });
                    }
                }
            }
            Ok(Event::Text(ref e)) => {
                if let Some(tag) = current_tag.as_deref() {
                    let text = std::str::from_utf8(e.as_ref()).unwrap_or("").trim().to_string();
                    if !text.is_empty() {
                        match tag {
                            "id" | "uuid" => m.id = text,
                            "name" | "title" => m.name = text,
                            "version" => m.version = text,
                            "creator" | "author" => m.creator = text,
                            "category" => m.category = text,
                            "description" => m.description = text,
                            "tag" | "keyword" => m.tags.push(text),
                            _ => {}
                        }
                    }
                }
            }
            Ok(Event::End(_)) => {
                current_tag = None;
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("error parseando XML de paquete: {e}")),
            _ => {}
        }
        buf.clear();
    }
    Ok(m)
}

fn sanitize_id(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "bwpackage".to_string()
    } else {
        cleaned
    }
}

fn sanitize_rel_path(raw: &str) -> Option<PathBuf> {
    let norm = raw.replace('\\', "/");
    let mut out = PathBuf::new();
    for comp in Path::new(&norm).components() {
        match comp {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Parsea un archivo `.bwpackage` completo (ZIP + metadatos + índice de assets).
pub fn parse_bwpackage_bytes(bytes: &[u8]) -> Result<BwPackageDocument, String> {
    let entries = read_zip_entries(bytes)?;
    if entries.is_empty() {
        return Err("el contenedor .bwpackage está vacío".to_string());
    }

    let mut manifest = BwPackageManifest::default();
    for name in [
        "package.json",
        "manifest.json",
        "metadata.json",
        "info.json",
    ] {
        if let Some((_, data)) = entries
            .iter()
            .find(|(k, _)| k.trim_start_matches("./").eq_ignore_ascii_case(name))
        {
            manifest = parse_json_manifest(data)?;
            break;
        }
    }
    if manifest.id.is_empty() {
        for name in ["package.xml", "manifest.xml"] {
            if let Some((_, data)) = entries
                .iter()
                .find(|(k, _)| k.trim_start_matches("./").eq_ignore_ascii_case(name))
            {
                manifest = parse_xml_manifest(data)?;
                break;
            }
        }
    }

    let mut assets: Vec<BwPackageAsset> = Vec::new();
    let mut sorted_keys: Vec<&String> = entries.keys().collect();
    sorted_keys.sort();

    for key in sorted_keys {
        if key.ends_with('/') || is_manifest_entry(key) {
            continue;
        }
        let data = &entries[key];
        let norm_path = key.trim_start_matches("./").replace('\\', "/");
        let file_name = Path::new(&norm_path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&norm_path)
            .to_string();
        let kind = BwPackageAssetKind::from_path(&norm_path);
        assets.push(BwPackageAsset {
            path_in_zip: norm_path.clone(),
            file_name,
            kind,
            size_bytes: data.len(),
            asset_id: Some(norm_path),
        });
    }

    if manifest.id.is_empty() {
        manifest.id = sanitize_id(&manifest.name);
    }

    Ok(BwPackageDocument {
        manifest,
        assets,
        raw_entries: entries,
    })
}

/// Carga y parsea un archivo `.bwpackage` desde disco.
pub fn load_bwpackage_file(path: &Path) -> Result<BwPackageDocument, String> {
    let bytes = std::fs::read(path)
        .map_err(|e| format!("no se pudo leer {}: {e}", path.display()))?;
    let mut doc = parse_bwpackage_bytes(&bytes)?;
    if doc.manifest.name == "Bitwig Sound Package" {
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            doc.manifest.name = stem.to_string();
            if doc.manifest.id == "Bitwig_Sound_Package" || doc.manifest.id.is_empty() {
                doc.manifest.id = sanitize_id(stem);
            }
        }
    }
    Ok(doc)
}

/// Devuelve el directorio por defecto de instalación de paquetes en Hikaru.
pub fn default_packages_library_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        home.join(".local/share/hikaru-openlive/packages")
    } else {
        std::env::temp_dir().join("hikaru-openlive-packages")
    }
}

/// Lista los paquetes ya instalados en `library_root`.
pub fn list_installed_packages(library_root: &Path) -> Vec<InstalledBwPackageIndex> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(library_root) else {
        return out;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let idx_path = dir.join(PACKAGE_INDEX_FILE);
        if let Ok(raw) = std::fs::read(&idx_path) {
            if let Some(parsed) = InstalledBwPackageIndex::from_json_bytes(&raw) {
                out.push(parsed);
            }
        }
    }
    out
}

/// Instala un `.bwpackage` en `library_root`, extrae sus assets de forma segura,
/// escribe su índice local y resuelve dependencias frente a los paquetes ya instalados.
pub fn install_bwpackage_to(
    package_path: &Path,
    library_root: &Path,
) -> Result<BwPackageInstallReport, String> {
    let doc = load_bwpackage_file(package_path)?;
    let pkg_id = sanitize_id(&doc.manifest.id);
    let install_dir = library_root.join(&pkg_id);
    std::fs::create_dir_all(&install_dir)
        .map_err(|e| format!("no se pudo crear {}: {e}", install_dir.display()))?;

    let mut extracted_files = Vec::new();
    for asset in &doc.assets {
        let Some(rel) = sanitize_rel_path(&asset.path_in_zip) else {
            continue;
        };
        let dest = install_dir.join(&rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("no se pudo crear directorio {}: {e}", parent.display()))?;
        }
        if let Some(bytes) = doc.raw_entries.get(&asset.path_in_zip) {
            std::fs::write(&dest, bytes)
                .map_err(|e| format!("error extrayendo {}: {e}", dest.display()))?;
            extracted_files.push(dest);
        }
    }

    let index = InstalledBwPackageIndex {
        manifest: doc.manifest.clone(),
        assets: doc.assets.clone(),
    };
    let idx_bytes = serde_json::to_vec_pretty(&index.to_json_value())
        .map_err(|e| format!("error serializando índice de paquete: {e}"))?;
    std::fs::write(install_dir.join(PACKAGE_INDEX_FILE), idx_bytes)
        .map_err(|e| format!("error escribiendo índice de paquete: {e}"))?;

    let installed = list_installed_packages(library_root);
    let installed_ids: HashSet<String> = installed
        .iter()
        .map(|p| p.manifest.id.to_ascii_lowercase())
        .collect();

    let mut resolved_dependencies = Vec::new();
    let mut missing_dependencies = Vec::new();
    for dep in &doc.manifest.dependencies {
        if installed_ids.contains(&dep.id.to_ascii_lowercase()) {
            resolved_dependencies.push(dep.id.clone());
        } else {
            missing_dependencies.push(dep.clone());
        }
    }

    Ok(BwPackageInstallReport {
        package_id: doc.manifest.id,
        package_name: doc.manifest.name,
        version: doc.manifest.version,
        install_dir,
        extracted_files,
        asset_index: doc.assets,
        resolved_dependencies,
        missing_dependencies,
    })
}

/// Resuelve un `referenced_packaged_file_ids` dentro de los paquetes instalados en `library_root`.
pub fn resolve_packaged_file_id(library_root: &Path, file_id: &str) -> Option<PathBuf> {
    let target = file_id.trim().trim_start_matches("./").replace('\\', "/").to_ascii_lowercase();
    if target.is_empty() {
        return None;
    }
    let Ok(entries) = std::fs::read_dir(library_root) else {
        return None;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let idx_path = dir.join(PACKAGE_INDEX_FILE);
        let Ok(raw) = std::fs::read(&idx_path) else {
            continue;
        };
        let Some(parsed) = InstalledBwPackageIndex::from_json_bytes(&raw) else {
            continue;
        };
        for asset in &parsed.assets {
            let norm = asset.path_in_zip.to_ascii_lowercase();
            let id_match = asset
                .asset_id
                .as_deref()
                .map(|id| id.to_ascii_lowercase() == target)
                .unwrap_or(false);
            if norm == target || id_match {
                if let Some(rel) = sanitize_rel_path(&asset.path_in_zip) {
                    let candidate = dir.join(rel);
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }
    }
    None
}

/// Construye un `.bwpackage` sintético en bytes (útil para exportación y tests).
pub fn build_synthetic_bwpackage_zip(
    manifest: &BwPackageManifest,
    files: &[(&str, &[u8])],
) -> Result<Vec<u8>, String> {
    let manifest_json = serde_json::json!({
        "id": manifest.id,
        "name": manifest.name,
        "version": manifest.version,
        "creator": manifest.creator,
        "category": manifest.category,
        "description": manifest.description,
        "tags": manifest.tags,
        "dependencies": manifest.dependencies.iter().map(|d| serde_json::json!({
            "id": d.id,
            "name": d.name,
            "minVersion": d.min_version,
        })).collect::<Vec<_>>(),
    });
    let manifest_bytes = serde_json::to_vec_pretty(&manifest_json)
        .map_err(|e| format!("error serializando manifest: {e}"))?;

    let mut entries: Vec<(&str, &[u8])> = Vec::with_capacity(files.len() + 1);
    entries.push(("package.json", &manifest_bytes));
    for (p, data) in files {
        entries.push((p, data));
    }
    Ok(build_zip_store(&entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_test_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hikaru_bwpackage_test_{}_{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn bpk01_parses_bwpackage_zip_and_manifest_metadata() {
        let manifest = BwPackageManifest {
            id: "com.hikaru.analog-essentials".to_string(),
            name: "Analog Essentials".to_string(),
            version: "1.2.0".to_string(),
            creator: "Hikaru Sound".to_string(),
            category: "Synth & Drums".to_string(),
            description: "Clean-room test pack".to_string(),
            tags: vec!["analog".to_string(), "bass".to_string()],
            dependencies: vec![BwPackageDependency {
                id: "com.hikaru.core-waves".to_string(),
                name: Some("Core Waves".to_string()),
                min_version: Some("1.0.0".to_string()),
            }],
        };
        let bytes = build_synthetic_bwpackage_zip(
            &manifest,
            &[
                ("samples/kick.wav", b"RIFFdemo"),
                ("presets/sub_bass.bwpreset", b"BtWg00030002"),
                ("clips/groove.bwclip", b"BtWg00030002"),
            ],
        )
        .expect("build bwpackage");

        let doc = parse_bwpackage_bytes(&bytes).expect("parse bwpackage");
        assert_eq!(doc.manifest.id, "com.hikaru.analog-essentials");
        assert_eq!(doc.manifest.name, "Analog Essentials");
        assert_eq!(doc.manifest.version, "1.2.0");
        assert_eq!(doc.manifest.dependencies.len(), 1);
        assert_eq!(doc.assets.len(), 3);
    }

    #[test]
    fn bpk02_indexes_samples_presets_clips_and_multisamples() {
        let manifest = BwPackageManifest {
            id: "com.hikaru.browser-pack".to_string(),
            name: "Browser Pack".to_string(),
            ..Default::default()
        };
        let bytes = build_synthetic_bwpackage_zip(
            &manifest,
            &[
                ("samples/snare.wav", b"RIFF"),
                ("presets/pad.bwpreset", b"BtWg"),
                ("clips/arp.bwclip", b"BtWg"),
                ("multisamples/piano.multisample", b"PK\x03\x04"),
            ],
        )
        .unwrap();

        let doc = parse_bwpackage_bytes(&bytes).unwrap();
        let kinds: Vec<BwPackageAssetKind> = doc.assets.iter().map(|a| a.kind).collect();
        assert!(kinds.contains(&BwPackageAssetKind::Audio));
        assert!(kinds.contains(&BwPackageAssetKind::Preset));
        assert!(kinds.contains(&BwPackageAssetKind::Clip));
        assert!(kinds.contains(&BwPackageAssetKind::Multisample));
    }

    #[test]
    fn bpk03_installs_package_and_resolves_dependencies_and_file_ids() {
        let root = temp_test_dir("install");

        // 1. Instalar paquete base
        let base_manifest = BwPackageManifest {
            id: "com.hikaru.core-waves".to_string(),
            name: "Core Waves".to_string(),
            version: "1.0.0".to_string(),
            ..Default::default()
        };
        let base_zip =
            build_synthetic_bwpackage_zip(&base_manifest, &[("wavetables/sine.wt", b"vawt")])
                .unwrap();
        let base_path = root.join("core-waves.bwpackage");
        std::fs::write(&base_path, base_zip).unwrap();
        let base_report = install_bwpackage_to(&base_path, &root).expect("install base");
        assert!(base_report.missing_dependencies.is_empty());

        // 2. Instalar paquete dependiente (con una dependencia resuelta y otra faltante)
        let ext_manifest = BwPackageManifest {
            id: "com.hikaru.lead-pack".to_string(),
            name: "Lead Pack".to_string(),
            version: "2.0.0".to_string(),
            dependencies: vec![
                BwPackageDependency {
                    id: "com.hikaru.core-waves".to_string(),
                    name: None,
                    min_version: None,
                },
                BwPackageDependency {
                    id: "com.hikaru.missing-pack".to_string(),
                    name: None,
                    min_version: None,
                },
            ],
            ..Default::default()
        };
        let ext_zip = build_synthetic_bwpackage_zip(
            &ext_manifest,
            &[("samples/lead_c3.wav", b"RIFFlead")],
        )
        .unwrap();
        let ext_path = root.join("lead-pack.bwpackage");
        std::fs::write(&ext_path, ext_zip).unwrap();

        let ext_report = install_bwpackage_to(&ext_path, &root).expect("install dependent");
        assert_eq!(
            ext_report.resolved_dependencies,
            vec!["com.hikaru.core-waves".to_string()]
        );
        assert_eq!(ext_report.missing_dependencies.len(), 1);
        assert_eq!(
            ext_report.missing_dependencies[0].id,
            "com.hikaru.missing-pack"
        );

        let resolved_file = resolve_packaged_file_id(&root, "samples/lead_c3.wav")
            .expect("resolve packaged file id");
        assert!(resolved_file.is_file());

        let _ = std::fs::remove_dir_all(&root);
    }
}
