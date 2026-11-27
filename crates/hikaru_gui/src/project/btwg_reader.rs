// Lector AST del formato binario nativo de Bitwig Studio (`BtWg`).
// GNU AGPLv3
// crates/hikaru_gui/src/project/btwg_reader.rs
//
// Lector recursivo simétrico a `BtWgWriter` (`btwg.rs`) que reconstruye:
// - La cabecera ASCII de 42 bytes (`BtWgHeader`)
// - El árbol completo de objetos (`meta` + `body`) para documentos públicos
//   `BtWg00030002` (`.bwpreset`, `.bwproject`, `.bwtemplate`, `.bwremotecontrols`)
// - La tabla de objetos 1-based en pre-orden DFS para resolver `Ref(id)` y
//   `ObjKeyRef { obj_id, key }`
// - El reconocimiento limpio (*clean-room*) de contenedores protegidos
//   `BtWg00030004` (`.bwdevice`, `.bwmodule`, `.bwmodulator`) sin desofuscar
//   ni leer su payload Nitro protegido.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Límite de profundidad recursiva para evitar desbordamiento ante binarios dañados.
const MAX_AST_DEPTH: usize = 64;
/// Límite de elementos por lista o campos por objeto.
const MAX_CONTAINER_ITEMS: usize = 16_384;

/// Clasificación del contenedor `BtWg` según su `format_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BtWgContainerKind {
    /// `0002`: Contenedor público en texto/binario claro (`.bwpreset`, `.bwproject`, etc.).
    PublicAst,
    /// `0004`: Contenedor protegido de fábrica (`.bwdevice`, `.bwmodule`, `.bwmodulator`).
    ProtectedDevice,
    /// Otro tipo de formato no catalogado.
    Unknown,
}

/// Cabecera ASCII de 42 bytes de un contenedor `BtWg`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtWgHeader {
    pub version: String,
    pub format_type: String,
    pub revision: String,
    pub body_offset: usize,
    pub tail_hex: String,
    pub header_len: usize,
}

impl BtWgHeader {
    /// Parsea la cabecera ASCII de 42 bytes (`BtWg` + version + type + rev + body_offset + tail).
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 42 {
            return Err(format!(
                "archivo demasiado corto para cabecera BtWg ({} bytes < 42)",
                bytes.len()
            ));
        }
        if &bytes[0..4] != b"BtWg" {
            return Err("no es un contenedor BtWg válido (falta magia 'BtWg')".to_string());
        }
        let version = std::str::from_utf8(&bytes[4..8])
            .map_err(|_| "versión BtWg no es UTF-8".to_string())?
            .to_string();
        let format_type = std::str::from_utf8(&bytes[8..12])
            .map_err(|_| "tipo BtWg no es UTF-8".to_string())?
            .to_string();
        let revision = std::str::from_utf8(&bytes[12..16])
            .map_err(|_| "revisión BtWg no es UTF-8".to_string())?
            .to_string();
        let body_offset_str = std::str::from_utf8(&bytes[16..24])
            .map_err(|_| "body_offset BtWg no es UTF-8".to_string())?;
        let body_offset = usize::from_str_radix(body_offset_str, 16)
            .map_err(|_| format!("body_offset BtWg inválido ('{body_offset_str}')"))?;
        let tail_hex = std::str::from_utf8(&bytes[24..42])
            .map_err(|_| "cola de cabecera BtWg no es UTF-8".to_string())?
            .to_string();

        let header_len = if bytes.get(42) == Some(&b'\n') { 43 } else { 42 };

        Ok(Self {
            version,
            format_type,
            revision,
            body_offset,
            tail_hex,
            header_len,
        })
    }

    /// Devuelve la clasificación del contenedor según `format_type`.
    pub fn kind(&self) -> BtWgContainerKind {
        match self.format_type.as_str() {
            "0002" => BtWgContainerKind::PublicAst,
            "0004" => BtWgContainerKind::ProtectedDevice,
            _ => BtWgContainerKind::Unknown,
        }
    }
}

/// Valor tipado dentro del árbol `BtWg00030002` (tags `0x01..0x1a`).
#[derive(Debug, Clone, PartialEq)]
pub enum BtWgValue {
    I8(u8),
    I16(i16),
    I32(i32),
    I64(u64),
    Bool(bool),
    F32(f32),
    F64(f64),
    Str(String),
    Obj(BtWgObject),
    Null,
    Ref(u32),
    Bytes(Vec<u8>),
    ObjList(Vec<BtWgObject>),
    Uuid([u8; 16]),
    Color([f32; 4]),
    StrArray(Vec<String>),
    ObjKeyRef { obj_id: u32, key: String },
    ObjKeyInline { obj: BtWgObject },
    UnknownTag { tag: u8 },
}

impl BtWgValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            BtWgValue::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        match self {
            BtWgValue::I32(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            BtWgValue::F64(v) => Some(*v),
            BtWgValue::F32(v) => Some(*v as f64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            BtWgValue::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_uuid_string(&self) -> Option<String> {
        match self {
            BtWgValue::Uuid(b) => Some(format_uuid(b)),
            _ => None,
        }
    }
}

/// Un objeto dentro del AST `BtWg` (identificado por `obj_id` 1-based DFS y `cls`).
#[derive(Debug, Clone, PartialEq)]
pub struct BtWgObject {
    pub obj_id: u32,
    pub cls: u32,
    pub name: Option<String>,
    pub named_fields: Vec<(String, BtWgValue)>,
    pub fields: Vec<(u32, BtWgValue)>,
}

impl BtWgObject {
    /// Busca un campo con nombre (habitual en el objeto `meta`, `cls = 4`).
    pub fn named_field(&self, key: &str) -> Option<&BtWgValue> {
        self.named_fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    /// Busca el primer campo numérico con el `fid` indicado.
    pub fn field(&self, fid: u32) -> Option<&BtWgValue> {
        self.fields.iter().find(|(id, _)| *id == fid).map(|(_, v)| v)
    }

    /// Devuelve todos los campos numéricos con el `fid` indicado.
    pub fn fields_with_id(&self, fid: u32) -> impl Iterator<Item = &BtWgValue> {
        self.fields
            .iter()
            .filter(move |(id, _)| *id == fid)
            .map(|(_, v)| v)
    }
}

/// Documento `BtWg` parseado en memoria con tabla de objetos 1-based DFS.
#[derive(Debug, Clone)]
pub struct BtWgDocument {
    pub header: BtWgHeader,
    pub kind: BtWgContainerKind,
    pub meta: Option<BtWgObject>,
    pub body: Option<BtWgObject>,
    /// Tabla de objetos indexada por `obj_id` (1-based DFS dentro del árbol `body`).
    pub objects_by_id: BTreeMap<u32, BtWgObject>,
}

impl BtWgDocument {
    /// Resuelve una referencia `BtWgValue::Ref(obj_id)` en la tabla 1-based DFS.
    pub fn resolve_ref(&self, obj_id: u32) -> Option<&BtWgObject> {
        self.objects_by_id.get(&obj_id)
    }

    /// Extrae un metadato string del objeto `meta` (`cls = 4`), si existe.
    pub fn meta_str(&self, key: &str) -> Option<&str> {
        self.meta.as_ref()?.named_field(key)?.as_str()
    }

    /// Nombre público del dispositivo (`meta.device_name`), si está presente.
    pub fn device_name(&self) -> Option<&str> {
        self.meta_str("device_name")
    }

    /// UUID público del dispositivo (`meta.device_id`), si está presente.
    pub fn device_id(&self) -> Option<&str> {
        self.meta_str("device_id")
    }

    /// Categoría pública del dispositivo (`meta.device_category`), si está presente.
    pub fn device_category(&self) -> Option<&str> {
        self.meta_str("device_category")
    }
}

/// Resultado de abrir un `.bwdevice` en modo *clean-room* (sin tocar su payload protegido).
#[derive(Debug, Clone)]
pub struct BwDeviceMetadata {
    pub header: BtWgHeader,
    pub file_name: String,
    pub device_stem: String,
    pub companion_preset_path: Option<PathBuf>,
    pub companion_preset: Option<BtWgDocument>,
}

/// Resultado de abrir un `.bwmodulator` en modo *clean-room* (sin tocar su payload protegido).
#[derive(Debug, Clone)]
pub struct BwModulatorMetadata {
    pub header: BtWgHeader,
    pub file_name: String,
    pub modulator_stem: String,
    pub resolved_uuid: Option<String>,
    pub companion_preset_path: Option<PathBuf>,
    pub companion_preset: Option<BtWgDocument>,
}

/// Resultado de abrir un `.bwmodule` en modo *clean-room* (sin tocar su payload protegido).
#[derive(Debug, Clone)]
pub struct BwModuleMetadata {
    pub header: BtWgHeader,
    pub file_name: String,
    pub module_stem: String,
    pub resolved_uuid: Option<String>,
    pub companion_preset_path: Option<PathBuf>,
    pub companion_preset: Option<BtWgDocument>,
}

/// Lector binario secuencial de contenedores `BtWg00030002`.
pub struct BtWgReader<'a> {
    data: &'a [u8],
    pos: usize,
    next_obj_id: u32,
}

impl<'a> BtWgReader<'a> {
    pub fn new(data: &'a [u8], start: usize) -> Self {
        Self {
            data,
            pos: start.min(data.len()),
            next_obj_id: 1,
        }
    }

    /// Parsea un archivo `BtWg` completo desde bytes.
    ///
    /// - Para `BtWg00030002` reconstruye `meta`, `body` y la tabla 1-based DFS.
    /// - Para `BtWg00030004` (`.bwdevice`) valida únicamente la cabecera y devuelve
    ///   un `BtWgDocument` con `kind == BtWgContainerKind::ProtectedDevice` sin leer
    ///   el payload protegido.
    pub fn parse_document(bytes: &[u8]) -> Result<BtWgDocument, String> {
        let header = BtWgHeader::parse(bytes)?;
        let kind = header.kind();

        if kind != BtWgContainerKind::PublicAst {
            return Ok(BtWgDocument {
                header,
                kind,
                meta: None,
                body: None,
                objects_by_id: BTreeMap::new(),
            });
        }

        let mut meta_cursor = BtWgReader::new(bytes, header.header_len);
        let meta = if meta_cursor.peek_u32() == Some(4) {
            Some(meta_cursor.read_object(0)?)
        } else {
            None
        };

        let mut objects_by_id = BTreeMap::new();
        let body = if header.body_offset < bytes.len() {
            let mut body_cursor = BtWgReader::new(bytes, header.body_offset);
            let root = body_cursor.read_object(0)?;
            index_objects_dfs(&root, &mut objects_by_id, 0);
            Some(root)
        } else {
            None
        };

        Ok(BtWgDocument {
            header,
            kind,
            meta,
            body,
            objects_by_id,
        })
    }

    /// Identifica un `.bwdevice` (`BtWg00030004`) de forma *clean-room* y, si existe
    /// un `.bwpreset` / `Default.bwpreset` compañero en claro, lo parsea para exponer
    /// su contrato público inicial.
    pub fn inspect_bwdevice_file(
        path: &Path,
        library_dir: Option<&Path>,
    ) -> Result<BwDeviceMetadata, String> {
        let bytes = fs::read(path)
            .map_err(|err| format!("no se pudo leer '{}': {err}", path.display()))?;
        let header = BtWgHeader::parse(&bytes)?;
        if header.kind() != BtWgContainerKind::ProtectedDevice {
            return Err(format!(
                "se esperaba contenedor BtWg tipo '0004' (.bwdevice), se encontró '{}'",
                header.format_type
            ));
        }

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown.bwdevice")
            .to_string();
        let device_stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let companion_preset_path = find_companion_preset(path, &device_stem, library_dir);
        let companion_preset = companion_preset_path
            .as_ref()
            .and_then(|p| fs::read(p).ok())
            .and_then(|b| Self::parse_document(&b).ok());

        Ok(BwDeviceMetadata {
            header,
            file_name,
            device_stem,
            companion_preset_path,
            companion_preset,
        })
    }

    /// Identifica un `.bwmodulator` (`BtWg00030004`) de forma *clean-room*, resuelve su UUID
    /// público desde el preset compañero (si existe) y expone su contrato inicial.
    pub fn inspect_bwmodulator_file(
        path: &Path,
        library_dir: Option<&Path>,
    ) -> Result<BwModulatorMetadata, String> {
        let (header, file_name, stem, resolved_uuid, companion_preset_path, companion_preset) =
            Self::inspect_protected_artifact(path, library_dir, ".bwmodulator")?;
        Ok(BwModulatorMetadata {
            header,
            file_name,
            modulator_stem: stem,
            resolved_uuid,
            companion_preset_path,
            companion_preset,
        })
    }

    /// Identifica un `.bwmodule` (`BtWg00030004`) de forma *clean-room*, resuelve su UUID
    /// público desde el preset compañero (si existe) y expone su contrato inicial.
    pub fn inspect_bwmodule_file(
        path: &Path,
        library_dir: Option<&Path>,
    ) -> Result<BwModuleMetadata, String> {
        let (header, file_name, stem, resolved_uuid, companion_preset_path, companion_preset) =
            Self::inspect_protected_artifact(path, library_dir, ".bwmodule")?;
        Ok(BwModuleMetadata {
            header,
            file_name,
            module_stem: stem,
            resolved_uuid,
            companion_preset_path,
            companion_preset,
        })
    }

    fn inspect_protected_artifact(
        path: &Path,
        library_dir: Option<&Path>,
        expected_ext: &str,
    ) -> Result<
        (
            BtWgHeader,
            String,
            String,
            Option<String>,
            Option<PathBuf>,
            Option<BtWgDocument>,
        ),
        String,
    > {
        let bytes = fs::read(path)
            .map_err(|err| format!("no se pudo leer '{}': {err}", path.display()))?;
        let header = BtWgHeader::parse(&bytes)?;
        if header.kind() != BtWgContainerKind::ProtectedDevice {
            return Err(format!(
                "se esperaba contenedor BtWg tipo '0004' ({expected_ext}), se encontró '{}'",
                header.format_type
            ));
        }

        let fallback_name = format!("unknown{expected_ext}");
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&fallback_name)
            .to_string();
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let companion_preset_path = find_companion_preset(path, &stem, library_dir);
        let companion_preset = companion_preset_path
            .as_ref()
            .and_then(|p| fs::read(p).ok())
            .and_then(|b| Self::parse_document(&b).ok());
        let resolved_uuid = companion_preset
            .as_ref()
            .and_then(|doc| doc.device_id().map(ToOwned::to_owned));

        Ok((
            header,
            file_name,
            stem,
            resolved_uuid,
            companion_preset_path,
            companion_preset,
        ))
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or_else(|| "overflow leyendo bytes BtWg".to_string())?;
        if end > self.data.len() {
            return Err(format!(
                "EOF inesperado leyendo {len} bytes en offset {}",
                self.pos
            ));
        }
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        let Some(&b) = self.data.get(self.pos) else {
            return Err("EOF inesperado leyendo u8".to_string());
        };
        self.pos += 1;
        Ok(b)
    }

    fn read_i16(&mut self) -> Result<i16, String> {
        let b = self.read_exact(2)?;
        Ok(i16::from_be_bytes([b[0], b[1]]))
    }

    fn read_u32(&mut self) -> Result<u32, String> {
        let b = self.read_exact(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn read_i32(&mut self) -> Result<i32, String> {
        let b = self.read_exact(4)?;
        Ok(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn read_u64(&mut self) -> Result<u64, String> {
        let b = self.read_exact(8)?;
        Ok(u64::from_be_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    fn read_f32(&mut self) -> Result<f32, String> {
        let b = self.read_exact(4)?;
        Ok(f32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn read_f64(&mut self) -> Result<f64, String> {
        let b = self.read_exact(8)?;
        Ok(f64::from_be_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    fn read_str(&mut self) -> Result<String, String> {
        let len = self.read_u32()? as usize;
        if len > 16 * 1024 * 1024 {
            return Err(format!("string BtWg demasiado largo ({len} bytes)"));
        }
        let bytes = self.read_exact(len)?;
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    fn peek_u32(&self) -> Option<u32> {
        if self.remaining() < 4 {
            return None;
        }
        Some(u32::from_be_bytes([
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
        ]))
    }

    fn read_value(&mut self, tag: u8, depth: usize) -> Result<BtWgValue, String> {
        if depth > MAX_AST_DEPTH {
            return Err("profundidad máxima del AST BtWg excedida".to_string());
        }
        match tag {
            0x01 => Ok(BtWgValue::I8(self.read_u8()?)),
            0x02 => Ok(BtWgValue::I16(self.read_i16()?)),
            0x03 => Ok(BtWgValue::I32(self.read_i32()?)),
            0x04 => Ok(BtWgValue::I64(self.read_u64()?)),
            0x05 => Ok(BtWgValue::Bool(self.read_u8()? != 0)),
            0x06 => Ok(BtWgValue::F32(self.read_f32()?)),
            0x07 => Ok(BtWgValue::F64(self.read_f64()?)),
            0x08 => Ok(BtWgValue::Str(self.read_str()?)),
            0x09 => Ok(BtWgValue::Obj(self.read_object(depth + 1)?)),
            0x0a => Ok(BtWgValue::Null),
            0x0b => Ok(BtWgValue::Ref(self.read_u32()?)),
            0x0d => {
                let len = self.read_u32()? as usize;
                if len > 32 * 1024 * 1024 {
                    return Err(format!("bloque bytes demasiado grande ({len} bytes)"));
                }
                let data = self.read_exact(len)?.to_vec();
                Ok(BtWgValue::Bytes(data))
            }
            0x12 => {
                let mut items = Vec::new();
                for _ in 0..MAX_CONTAINER_ITEMS {
                    let Some(marker) = self.peek_u32() else {
                        break;
                    };
                    if marker == 3 {
                        let _ = self.read_u32()?;
                        break;
                    }
                    items.push(self.read_object(depth + 1)?);
                }
                Ok(BtWgValue::ObjList(items))
            }
            0x15 => {
                let b = self.read_exact(16)?;
                let mut uuid = [0u8; 16];
                uuid.copy_from_slice(b);
                Ok(BtWgValue::Uuid(uuid))
            }
            0x16 => {
                let r = self.read_f32()?;
                let g = self.read_f32()?;
                let b = self.read_f32()?;
                let a = self.read_f32()?;
                Ok(BtWgValue::Color([r, g, b, a]))
            }
            0x19 => {
                let count = (self.read_u32()? as usize).min(MAX_CONTAINER_ITEMS);
                let mut values = Vec::with_capacity(count);
                for _ in 0..count {
                    values.push(self.read_str()?);
                }
                Ok(BtWgValue::StrArray(values))
            }
            0x1a => {
                let kind = self.read_u32()?;
                if kind == 1 {
                    let obj_id = self.read_u32()?;
                    let key = self.read_str()?;
                    Ok(BtWgValue::ObjKeyRef { obj_id, key })
                } else {
                    let obj = self.read_object_with_cls(kind, depth + 1)?;
                    Ok(BtWgValue::ObjKeyInline { obj })
                }
            }
            other => Ok(BtWgValue::UnknownTag { tag: other }),
        }
    }

    pub fn read_object(&mut self, depth: usize) -> Result<BtWgObject, String> {
        let cls = self.read_u32()?;
        self.read_object_with_cls(cls, depth)
    }

    fn read_object_with_cls(&mut self, cls: u32, depth: usize) -> Result<BtWgObject, String> {
        if depth > MAX_AST_DEPTH {
            return Err("profundidad máxima de objeto BtWg excedida".to_string());
        }
        let obj_id = self.next_obj_id;
        self.next_obj_id += 1;

        let mut name = None;
        let mut named_fields = Vec::new();
        let mut fields = Vec::new();

        if cls == 4 {
            name = Some(self.read_str()?);
            for _ in 0..MAX_CONTAINER_ITEMS {
                let Some(marker) = self.peek_u32() else {
                    break;
                };
                if marker == 0 {
                    let _ = self.read_u32()?;
                    break;
                }
                let kind = self.read_u32()?;
                if kind == 1 {
                    let field_name = self.read_str()?;
                    let tag = self.read_u8()?;
                    let val = self.read_value(tag, depth + 1)?;
                    let is_unknown = matches!(val, BtWgValue::UnknownTag { .. });
                    named_fields.push((field_name, val));
                    if is_unknown {
                        break;
                    }
                } else {
                    let tag = self.read_u8()?;
                    let val = self.read_value(tag, depth + 1)?;
                    let is_unknown = matches!(val, BtWgValue::UnknownTag { .. });
                    fields.push((kind, val));
                    if is_unknown {
                        break;
                    }
                }
            }
        } else {
            for _ in 0..MAX_CONTAINER_ITEMS {
                if self.remaining() < 4 {
                    break;
                }
                let fid = self.read_u32()?;
                if fid == 0 {
                    break;
                }
                let tag = self.read_u8()?;
                let val = self.read_value(tag, depth + 1)?;
                let is_unknown = matches!(val, BtWgValue::UnknownTag { .. });
                fields.push((fid, val));
                if is_unknown {
                    break;
                }
            }
        }

        Ok(BtWgObject {
            obj_id,
            cls,
            name,
            named_fields,
            fields,
        })
    }
}

fn index_objects_dfs(
    obj: &BtWgObject,
    table: &mut BTreeMap<u32, BtWgObject>,
    depth: usize,
) {
    if depth > MAX_AST_DEPTH {
        return;
    }
    table.insert(obj.obj_id, obj.clone());
    for (_, val) in &obj.fields {
        match val {
            BtWgValue::Obj(child) | BtWgValue::ObjKeyInline { obj: child } => {
                index_objects_dfs(child, table, depth + 1);
            }
            BtWgValue::ObjList(list) => {
                for child in list {
                    index_objects_dfs(child, table, depth + 1);
                }
            }
            _ => {}
        }
    }
}

fn format_uuid(b: &[u8; 16]) -> String {
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

/// Busca un `.bwpreset` / `Default.bwpreset` público en claro asociado a un `.bwdevice`.
fn find_companion_preset(
    device_path: &Path,
    device_stem: &str,
    library_dir: Option<&Path>,
) -> Option<PathBuf> {
    let sibling_named = device_path.with_extension("bwpreset");
    if sibling_named.is_file() {
        return Some(sibling_named);
    }
    if let Some(parent) = device_path.parent() {
        let sibling_default = parent.join("Default.bwpreset");
        if sibling_default.is_file() {
            return Some(sibling_default);
        }
    }

    let default_opt = Path::new("/opt/bitwig-studio/Library");
    let mut roots: Vec<&Path> = Vec::new();
    if let Some(dir) = library_dir {
        roots.push(dir);
    }
    if default_opt.is_dir() {
        roots.push(default_opt);
    }

    for root in roots {
        let settings = root.join("device-settings");
        if let Ok(entries) = fs::read_dir(settings) {
            for entry in entries.flatten().take(300) {
                let candidate = entry.path().join("Default.bwpreset");
                if !candidate.is_file() {
                    continue;
                }
                if let Ok(bytes) = fs::read(&candidate) {
                    if let Ok(doc) = BtWgReader::parse_document(&bytes) {
                        if doc
                            .device_name()
                            .map(|n| n.eq_ignore_ascii_case(device_stem))
                            .unwrap_or(false)
                        {
                            return Some(candidate);
                        }
                    }
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::btwg::BtWgWriter;

    fn build_sample_btwg_0002() -> Vec<u8> {
        let mut meta = BtWgWriter::new();
        meta.begin_obj(4);
        meta.raw_str("meta");
        meta.named_field_str("device_name", "Bit-8");
        meta.named_field_str("device_id", "8a1c0000-0000-4000-8000-000000000001");
        meta.named_field_str("device_category", "Distortion");
        meta.named_field_i32("schema_version", 2);
        meta.named_field_bytes("raw_tag", &[0xde, 0xad, 0xbe, 0xef]);
        meta.named_field_str_arr("tags", &["lofi".to_string(), "bitcrush".to_string()]);
        meta.end_obj();
        let meta_bytes = meta.into_bytes();

        let mut body = BtWgWriter::new();
        let root_id = body.begin_obj(0x561);
        assert_eq!(root_id, 1);
        body.field_i8(0x10, 7);
        body.field_i16(0x11, -320);
        body.field_i32(0x12, 44100);
        body.field_i64(0x13, 999_999);
        body.field_bool(0x14, true);
        body.field_f32(0x15, 0.75);
        body.field_f64(0x16, 128.5);
        body.field_str(0x17, "Drive");
        body.field_bytes(0x18, &[1, 2, 3]);
        body.field_null(0x19);
        let uuid = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        body.field_uuid(0x1a, uuid);
        body.field_color(0x1b, [0.25, 0.5, 0.75, 1.0]);

        let child_id = body.field_obj_header(0x20, 0x2c1);
        assert_eq!(child_id, 2);
        body.field_str(0x21, "ChildParam");
        body.end_obj();

        body.field_ref(0x22, child_id);
        body.field_obj_key_ref(0x23, child_id, "AMOUNT");

        body.field_obj_list_start(0x24);
        let list_item_id = body.begin_obj(0x300);
        assert_eq!(list_item_id, 3);
        body.field_f64(0x31, 0.5);
        body.end_obj();
        body.obj_list_end();

        let inline_id = body.field_obj_key_inline_start(0x25, 0x400);
        assert_eq!(inline_id, 4);
        body.field_bool(0x41, false);
        body.end_obj();

        body.end_obj();
        let body_bytes = body.into_bytes();

        let header_len = 43usize;
        let body_offset = header_len + meta_bytes.len();
        let header = format!("BtWg0003000200b8{body_offset:08x}000000000000000000\n");
        assert_eq!(header.len(), header_len);

        let mut out = Vec::with_capacity(body_offset + body_bytes.len());
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(&meta_bytes);
        out.extend_from_slice(&body_bytes);
        out
    }

    #[test]
    fn roundtrip_btwg_0002_ast_y_tabla_dfs() {
        let bytes = build_sample_btwg_0002();
        let doc = BtWgReader::parse_document(&bytes).expect("parsea BtWg00030002");

        assert_eq!(doc.kind, BtWgContainerKind::PublicAst);
        assert_eq!(doc.device_name(), Some("Bit-8"));
        assert_eq!(
            doc.device_id(),
            Some("8a1c0000-0000-4000-8000-000000000001")
        );
        assert_eq!(doc.device_category(), Some("Distortion"));

        let meta = doc.meta.as_ref().expect("tiene meta");
        assert_eq!(meta.named_field("schema_version").and_then(|v| v.as_i32()), Some(2));
        assert_eq!(
            meta.named_field("tags"),
            Some(&BtWgValue::StrArray(vec![
                "lofi".to_string(),
                "bitcrush".to_string()
            ]))
        );

        let root = doc.body.as_ref().expect("tiene body");
        assert_eq!(root.obj_id, 1);
        assert_eq!(root.cls, 0x561);
        assert_eq!(root.field(0x10), Some(&BtWgValue::I8(7)));
        assert_eq!(root.field(0x11), Some(&BtWgValue::I16(-320)));
        assert_eq!(root.field(0x12), Some(&BtWgValue::I32(44100)));
        assert_eq!(root.field(0x13), Some(&BtWgValue::I64(999_999)));
        assert_eq!(root.field(0x14), Some(&BtWgValue::Bool(true)));
        assert_eq!(root.field(0x16), Some(&BtWgValue::F64(128.5)));
        assert_eq!(root.field(0x17).and_then(|v| v.as_str()), Some("Drive"));
        assert_eq!(root.field(0x19), Some(&BtWgValue::Null));

        // Tabla 1-based DFS: 4 objetos (root=1, child=2, list_item=3, inline=4)
        assert_eq!(doc.objects_by_id.len(), 4);
        let resolved = doc.resolve_ref(2).expect("resuelve Ref(2)");
        assert_eq!(resolved.cls, 0x2c1);
        assert_eq!(resolved.field(0x21).and_then(|v| v.as_str()), Some("ChildParam"));
        assert_eq!(
            root.field(0x23),
            Some(&BtWgValue::ObjKeyRef {
                obj_id: 2,
                key: "AMOUNT".to_string()
            })
        );
        assert_eq!(doc.resolve_ref(3).map(|o| o.cls), Some(0x300));
        assert_eq!(doc.resolve_ref(4).map(|o| o.cls), Some(0x400));
    }

    #[test]
    fn identifica_bwdevice_protegido_sin_leer_payload_y_carga_companion() {
        let dir = std::env::temp_dir().join(format!("hikaru_btwg_reader_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("crea temp dir");

        // Simula un .bwdevice tipo 0004 con payload opaco de alta entropía
        let device_path = dir.join("Bit-8.bwdevice");
        let mut dev_bytes = b"BtWg0003000400ba00001600000000000000000000\n".to_vec();
        dev_bytes.extend_from_slice(&[0xff, 0x00, 0xaa, 0x55, 0x78, 0x9c]);
        fs::write(&device_path, &dev_bytes).expect("escribe bwdevice");

        // Simula su compañero público Bit-8.bwpreset en claro
        let preset_path = dir.join("Bit-8.bwpreset");
        fs::write(&preset_path, build_sample_btwg_0002()).expect("escribe preset");

        let meta = BtWgReader::inspect_bwdevice_file(&device_path, None)
            .expect("inspecciona bwdevice clean-room");
        assert_eq!(meta.header.format_type, "0004");
        assert_eq!(meta.device_stem, "Bit-8");
        assert_eq!(meta.companion_preset_path.as_deref(), Some(preset_path.as_path()));
        let preset_doc = meta.companion_preset.as_ref().expect("carga companion preset");
        assert_eq!(preset_doc.device_name(), Some("Bit-8"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rechaza_cabeceras_invalidas() {
        assert!(BtWgHeader::parse(b"corto").is_err());
        assert!(BtWgHeader::parse(&[b'X'; 42]).is_err());
    }
}
