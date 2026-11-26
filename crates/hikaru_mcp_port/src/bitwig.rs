// crates/hikaru_mcp/src/bitwig.rs
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Lector *clean-room* (solo lectura) de `.bwdevice` y `.bwpreset` de Bitwig Studio.
//!
//! # Frontera legal y técnica (enfoque Wine / Proton)
//!
//! - **`.bwpreset` (`BtWg00030002`):** contenedor público sin cifrar. Este módulo
//!   parsea su cabecera ASCII de 42 bytes, su diccionario `meta` (`cls = 4`) y su
//!   árbol binario de objetos `body` (`cls = 0x561` u otros) para exponer el
//!   contrato público: `device_id` (UUID), `device_name`, `device_category`,
//!   `device_type`, parámetros (`CONTENTS`), moduladores (`MODULATORS`) y
//!   subcadenas (`FX` / `Chain`).
//! - **`.bwdevice` (`BtWg00030004`):** contenedor protegido de fábrica que
//!   empaqueta grafos Nitro (`*.nitro`). En estricto respeto del aislamiento
//!   *clean-room* (AGPLv3/LGPLv3), **no** desofusca ni lee el payload protegido:
//!   únicamente valida la cabecera ASCII de 42 bytes, identifica el artefacto por
//!   su nombre/catálogo público, busca opcionalmente un `Default.bwpreset` en
//!   texto claro si existe en disco y lo mapea contra los equivalentes nativos
//!   de `hikaru_dsp`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// Límite de profundidad recursiva para evitar desbordamiento ante binarios dañados.
const MAX_AST_DEPTH: usize = 64;
/// Límite de elementos por lista o campos por objeto.
const MAX_CONTAINER_ITEMS: usize = 16_384;

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
        let body_offset = usize::from_str_radix(body_offset_str, 16).map_err(|_| {
            format!("body_offset BtWg inválido ('{body_offset_str}')")
        })?;
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

    pub fn to_json(&self) -> Value {
        json!({
            "magic": "BtWg",
            "version": self.version,
            "format_type": self.format_type,
            "revision": self.revision,
            "body_offset": self.body_offset,
            "tail_hex": self.tail_hex,
        })
    }
}

/// Valor tipado dentro del árbol `BtWg00030002`.
#[derive(Debug, Clone)]
pub enum BtWgValue {
    I8(u8),
    I16(i16),
    I32(i32),
    I64(u64),
    Bool(bool),
    F32(f32),
    F64(f64),
    Str(String),
    BytesLen(usize),
    Null,
    Ref(u32),
    Uuid(String),
    Color([f32; 4]),
    StrArray(Vec<String>),
    Obj(BtWgObject),
    ObjList(Vec<BtWgObject>),
    ObjKeyRef { obj_id: u32, key: String },
    ObjKeyInline { obj: BtWgObject },
    UnknownTag { tag: u8 },
}

impl BtWgValue {
    fn as_scalar_json(&self) -> Option<Value> {
        match self {
            BtWgValue::I8(v) => Some(json!(v)),
            BtWgValue::I16(v) => Some(json!(v)),
            BtWgValue::I32(v) => Some(json!(v)),
            BtWgValue::I64(v) => Some(json!(v)),
            BtWgValue::Bool(v) => Some(json!(v)),
            BtWgValue::F32(v) if v.is_finite() => Some(json!(v)),
            BtWgValue::F64(v) if v.is_finite() => Some(json!(v)),
            BtWgValue::Str(s) => Some(json!(s)),
            BtWgValue::Uuid(u) => Some(json!(u)),
            BtWgValue::Color(c) => Some(json!(c)),
            BtWgValue::StrArray(arr) => Some(json!(arr)),
            BtWgValue::BytesLen(n) => Some(json!({ "bytes_len": n })),
            BtWgValue::Null => Some(Value::Null),
            BtWgValue::Ref(id) => Some(json!({ "ref": id })),
            _ => None,
        }
    }
}

/// Un objeto dentro de `BtWg00030002` (identificado por `obj_id` 1-based DFS y `cls`).
#[derive(Debug, Clone)]
pub struct BtWgObject {
    pub obj_id: u32,
    pub cls: u32,
    pub name: Option<String>,
    pub named_fields: Vec<(String, BtWgValue)>,
    pub fields: Vec<(u32, BtWgValue)>,
}

struct BtWgCursor<'a> {
    data: &'a [u8],
    pos: usize,
    next_obj_id: u32,
}

impl<'a> BtWgCursor<'a> {
    fn new(data: &'a [u8], start: usize) -> Self {
        Self {
            data,
            pos: start.min(data.len()),
            next_obj_id: 1,
        }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        let Some(&b) = self.data.get(self.pos) else {
            return Err("EOF inesperado leyendo u8".to_string());
        };
        self.pos += 1;
        Ok(b)
    }

    fn read_i16(&mut self) -> Result<i16, String> {
        let bytes = self.read_exact(2)?;
        Ok(i16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn read_u32(&mut self) -> Result<u32, String> {
        let bytes = self.read_exact(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn read_i32(&mut self) -> Result<i32, String> {
        let bytes = self.read_exact(4)?;
        Ok(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn read_u64(&mut self) -> Result<u64, String> {
        let bytes = self.read_exact(8)?;
        Ok(u64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn read_f32(&mut self) -> Result<f32, String> {
        let bytes = self.read_exact(4)?;
        Ok(f32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn read_f64(&mut self) -> Result<f64, String> {
        let bytes = self.read_exact(8)?;
        Ok(f64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or_else(|| "overflow leyendo bytes".to_string())?;
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
                let _ = self.read_exact(len)?;
                Ok(BtWgValue::BytesLen(len))
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
                Ok(BtWgValue::Uuid(format_uuid(b)))
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
                    // Objeto inline donde `kind` es su `cls`
                    let obj = self.read_object_with_cls(kind, depth + 1)?;
                    Ok(BtWgValue::ObjKeyInline { obj })
                }
            }
            other => Ok(BtWgValue::UnknownTag { tag: other }),
        }
    }

    fn read_object(&mut self, depth: usize) -> Result<BtWgObject, String> {
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
            // Objeto con nombre (`meta`)
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

fn format_uuid(b: &[u8]) -> String {
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

/// Entrada del registro *clean-room* de compatibilidad hacia `hikaru_dsp`.
#[derive(Debug, Clone, Copy)]
pub struct BwCompatibilityEntry {
    pub bitwig_name: &'static str,
    pub canonical_uuid: Option<&'static str>,
    pub category: &'static str,
    pub topology: &'static str,
    pub hikaru_crate: &'static str,
    pub hikaru_module: &'static str,
    pub implementation_status: &'static str,
}

/// Catálogo *clean-room* que mapea dispositivos públicos de Bitwig hacia sus
/// reimplementaciones nativas en Hikaru OpenLive (enfoque Wine `builtin`).
pub fn compatibility_registry() -> &'static [BwCompatibilityEntry] {
    &[
        BwCompatibilityEntry {
            bitwig_name: "Bit-8",
            canonical_uuid: None,
            category: "Audio FX",
            topology: "audio_to_audio",
            hikaru_crate: "hikaru_dsp",
            hikaru_module: "crates/hikaru_dsp/src/effects/bit8.rs",
            implementation_status: "builtin_ready",
        },
        BwCompatibilityEntry {
            bitwig_name: "DC Offset",
            canonical_uuid: None,
            category: "Utility",
            topology: "audio_to_audio",
            hikaru_crate: "hikaru_dsp",
            hikaru_module: "crates/hikaru_dsp/src/effects/dc_offset.rs",
            implementation_status: "builtin_ready",
        },
        BwCompatibilityEntry {
            bitwig_name: "Tool",
            canonical_uuid: None,
            category: "Utility",
            topology: "audio_to_audio",
            hikaru_crate: "hikaru_dsp",
            hikaru_module: "crates/hikaru_dsp/src/effects/tool.rs",
            implementation_status: "builtin_ready",
        },
        BwCompatibilityEntry {
            bitwig_name: "Dual Pan",
            canonical_uuid: None,
            category: "Utility",
            topology: "audio_to_audio",
            hikaru_crate: "hikaru_dsp",
            hikaru_module: "crates/hikaru_dsp/src/effects/dual_pan.rs",
            implementation_status: "builtin_ready",
        },
        BwCompatibilityEntry {
            bitwig_name: "Test Tone",
            canonical_uuid: None,
            category: "Utility",
            topology: "audio_to_audio",
            hikaru_crate: "hikaru_dsp",
            hikaru_module: "crates/hikaru_dsp/src/effects/test_tone.rs",
            implementation_status: "builtin_ready",
        },
        BwCompatibilityEntry {
            bitwig_name: "Time Shift",
            canonical_uuid: None,
            category: "Utility",
            topology: "audio_to_audio",
            hikaru_crate: "hikaru_dsp",
            hikaru_module: "crates/hikaru_dsp/src/effects/time_shift.rs",
            implementation_status: "builtin_ready",
        },
        BwCompatibilityEntry {
            bitwig_name: "Tremolo",
            canonical_uuid: None,
            category: "Audio FX",
            topology: "audio_to_audio",
            hikaru_crate: "hikaru_dsp",
            hikaru_module: "crates/hikaru_dsp/src/effects/tremolo.rs",
            implementation_status: "builtin_ready",
        },
        BwCompatibilityEntry {
            bitwig_name: "Ring-Mod",
            canonical_uuid: None,
            category: "Audio FX",
            topology: "audio_to_audio",
            hikaru_crate: "hikaru_dsp",
            hikaru_module: "crates/hikaru_dsp/src/effects/ring_mod.rs",
            implementation_status: "builtin_ready",
        },
        BwCompatibilityEntry {
            bitwig_name: "Phaser+",
            canonical_uuid: None,
            category: "Audio FX",
            topology: "audio_to_audio",
            hikaru_crate: "hikaru_dsp",
            hikaru_module: "crates/hikaru_dsp/src/effects/phaser_plus.rs",
            implementation_status: "builtin_ready",
        },
        BwCompatibilityEntry {
            bitwig_name: "Freq Shifter+",
            canonical_uuid: None,
            category: "Audio FX",
            topology: "audio_to_audio",
            hikaru_crate: "hikaru_dsp",
            hikaru_module: "crates/hikaru_dsp/src/effects/freq_shifter_plus.rs",
            implementation_status: "builtin_ready",
        },
        BwCompatibilityEntry {
            bitwig_name: "Flanger+",
            canonical_uuid: None,
            category: "Audio FX",
            topology: "audio_to_audio",
            hikaru_crate: "hikaru_dsp",
            hikaru_module: "crates/hikaru_dsp/src/effects/flanger_plus.rs",
            implementation_status: "builtin_ready",
        },
        BwCompatibilityEntry {
            bitwig_name: "Comb",
            canonical_uuid: Some("20e18780-8438-48d3-b234-40dcbaa947b8"),
            category: "Audio FX",
            topology: "audio_to_audio",
            hikaru_crate: "hikaru_dsp",
            hikaru_module: "crates/hikaru_dsp/src/effects/ultracomb.rs",
            implementation_status: "builtin_ready",
        },
        BwCompatibilityEntry {
            bitwig_name: "EQ-2",
            canonical_uuid: Some("01af068e-1e49-4777-a6e6-7f1dc679227a"),
            category: "EQ",
            topology: "audio_to_audio",
            hikaru_crate: "hikaru_gui",
            hikaru_module: "hikaru_openeq3",
            implementation_status: "adapter_ready",
        },
        BwCompatibilityEntry {
            bitwig_name: "EQ-5",
            canonical_uuid: None,
            category: "EQ",
            topology: "audio_to_audio",
            hikaru_crate: "hikaru_gui",
            hikaru_module: "hikaru_openeq3",
            implementation_status: "adapter_ready",
        },
        BwCompatibilityEntry {
            bitwig_name: "Polymer",
            canonical_uuid: None,
            category: "Synth",
            topology: "note_to_audio",
            hikaru_crate: "hikaru_dsp",
            hikaru_module: "crates/hikaru_dsp/src/synth/open_poly.rs",
            implementation_status: "builtin_ready",
        },
        BwCompatibilityEntry {
            bitwig_name: "Polysynth",
            canonical_uuid: None,
            category: "Synth",
            topology: "note_to_audio",
            hikaru_crate: "hikaru_dsp",
            hikaru_module: "crates/hikaru_dsp/src/synth/open_poly.rs",
            implementation_status: "builtin_ready",
        },
        BwCompatibilityEntry {
            bitwig_name: "Sampler",
            canonical_uuid: None,
            category: "Sampler",
            topology: "note_to_audio",
            hikaru_crate: "hikaru_dsp",
            hikaru_module: "crates/hikaru_dsp/src/synth/sampler.rs",
            implementation_status: "builtin_ready",
        },
        BwCompatibilityEntry {
            bitwig_name: "Drum Machine",
            canonical_uuid: None,
            category: "Drums",
            topology: "note_to_audio",
            hikaru_crate: "hikaru_gui",
            hikaru_module: "hikaru_opendms",
            implementation_status: "builtin_ready",
        },
        BwCompatibilityEntry {
            bitwig_name: "Poly Grid",
            canonical_uuid: None,
            category: "The Grid",
            topology: "note_to_audio",
            hikaru_crate: "hikaru_dsp",
            hikaru_module: "crates/hikaru_dsp/src/synth/open_grid.rs",
            implementation_status: "builtin_ready",
        },
        BwCompatibilityEntry {
            bitwig_name: "FX Grid",
            canonical_uuid: None,
            category: "The Grid",
            topology: "audio_to_audio",
            hikaru_crate: "hikaru_dsp",
            hikaru_module: "crates/hikaru_dsp/src/synth/open_grid.rs",
            implementation_status: "builtin_ready",
        },
        BwCompatibilityEntry {
            bitwig_name: "Note Grid",
            canonical_uuid: Some("264d6f4e-5067-46c9-a4fa-a75a295d9e01"),
            category: "The Grid",
            topology: "note_to_note",
            hikaru_crate: "hikaru_dsp",
            hikaru_module: "crates/hikaru_dsp/src/synth/open_grid.rs",
            implementation_status: "builtin_ready",
        },
        BwCompatibilityEntry {
            bitwig_name: "Note Transpose",
            canonical_uuid: Some("0815cd9e-3a31-4429-a268-dabd952a3b68"),
            category: "Note FX",
            topology: "note_to_note",
            hikaru_crate: "hikaru_gui",
            hikaru_module: "PassthroughPlaceholderShim",
            implementation_status: "planned",
        },
    ]
}

fn lookup_compatibility(name: Option<&str>, uuid: Option<&str>) -> Value {
    let reg = compatibility_registry();
    let matched = reg.iter().find(|entry| {
        uuid.is_some() && entry.canonical_uuid.map(|u| u.eq_ignore_ascii_case(uuid.unwrap())).unwrap_or(false)
    }).or_else(|| {
        let target = name?.trim();
        reg.iter().find(|entry| entry.bitwig_name.eq_ignore_ascii_case(target))
    });

    match matched {
        Some(e) => json!({
            "mapeo": "hikaru_builtin",
            "bitwig_name": e.bitwig_name,
            "canonical_uuid": e.canonical_uuid,
            "category": e.category,
            "topology": e.topology,
            "hikaru_crate": e.hikaru_crate,
            "hikaru_module": e.hikaru_module,
            "status": e.implementation_status,
        }),
        None => json!({
            "mapeo": "passthrough_placeholder",
            "bitwig_name": name,
            "canonical_uuid": uuid,
            "hikaru_module": "PassthroughPlaceholderShim",
            "status": "read_only_bypass",
        }),
    }
}

/// Mapea el nombre público de un dispositivo Bitwig al identificador nativo de `DspSlot`.
pub fn resolve_hikaru_device_name(bitwig_name: &str) -> Option<&'static str> {
    match bitwig_name.trim().to_ascii_lowercase().as_str() {
        "bit-8" => Some("Hikaru Open8-Bit"),
        "dc offset" => Some("DC Offset"),
        "tool" => Some("Tool"),
        "dual pan" => Some("Dual Pan"),
        "test tone" => Some("Test Tone"),
        "time shift" => Some("Time Shift"),
        "tremolo" => Some("Tremolo"),
        "ring-mod" => Some("Ring-Mod"),
        "phaser+" => Some("Hikaru OpenPhaser+"),
        "freq shifter+" => Some("Hikaru OpenFreqShifter+"),
        "flanger+" => Some("Hikaru OpenFlanger+"),
        "comb" => Some("Comb"),
        "eq-2" | "eq-5" | "eq-dj" => Some("OpenEQ3"),
        "polymer" | "polysynth" => Some("OpenPoly"),
        "sampler" => Some("Sampler"),
        "drum machine" => Some("OpenDMS"),
        "poly grid" | "fx grid" | "note grid" => Some("OpenGrid"),
        _ => None,
    }
}

/// Extrae el contrato público de un árbol `BtWg00030002` (`.bwpreset` / `Default.bwpreset`).
fn extract_public_contract(
    meta: Option<&BtWgObject>,
    body: Option<&BtWgObject>,
) -> Value {
    let mut meta_map = BTreeMap::<String, Value>::new();
    if let Some(m) = meta {
        for (k, v) in &m.named_fields {
            if let Some(jv) = v.as_scalar_json() {
                meta_map.insert(k.clone(), jv);
            }
        }
    }

    let mut parameters = Vec::<Value>::new();
    let mut modulators = Vec::<Value>::new();
    let mut uuids = BTreeSet::<String>::new();
    let mut class_histogram = BTreeMap::<String, usize>::new();
    let mut total_objects = 0usize;

    if let Some(root) = body {
        collect_ast_public_contract(
            root,
            &mut parameters,
            &mut modulators,
            &mut uuids,
            &mut class_histogram,
            &mut total_objects,
            0,
        );
    }

    json!({
        "meta": meta_map,
        "parametros_publicos": parameters,
        "moduladores_y_keys": modulators,
        "uuids_referenciados": uuids.into_iter().collect::<Vec<_>>(),
        "ast_resumen": {
            "total_objetos": total_objects,
            "clases_vistas": class_histogram,
        }
    })
}

fn collect_ast_public_contract(
    obj: &BtWgObject,
    parameters: &mut Vec<Value>,
    modulators: &mut Vec<Value>,
    uuids: &mut BTreeSet<String>,
    class_histogram: &mut BTreeMap<String, usize>,
    total_objects: &mut usize,
    depth: usize,
) {
    if depth > MAX_AST_DEPTH {
        return;
    }
    *total_objects += 1;
    *class_histogram
        .entry(format!("0x{:x}", obj.cls))
        .or_insert(0) += 1;

    // Heurística limpia sobre objetos de parámetros: muchos objetos de parámetro
    // tienen al menos un string (nombre/etiqueta) y un escalar numérico/booleano.
    let mut first_str: Option<&str> = None;
    let mut first_num: Option<Value> = None;

    for (fid, val) in &obj.fields {
        match val {
            BtWgValue::Uuid(u) => {
                uuids.insert(u.clone());
            }
            BtWgValue::Str(s) => {
                if !s.is_empty() && first_str.is_none() {
                    first_str = Some(s.as_str());
                }
            }
            BtWgValue::F64(v) if v.is_finite() => {
                if first_num.is_none() {
                    first_num = Some(json!(v));
                }
            }
            BtWgValue::F32(v) if v.is_finite() => {
                if first_num.is_none() {
                    first_num = Some(json!(v));
                }
            }
            BtWgValue::I32(v) => {
                if first_num.is_none() {
                    first_num = Some(json!(v));
                }
            }
            BtWgValue::Bool(v) => {
                if first_num.is_none() {
                    first_num = Some(json!(v));
                }
            }
            BtWgValue::ObjKeyRef { obj_id, key } => {
                modulators.push(json!({
                    "parent_obj_id": obj.obj_id,
                    "field_id": format!("0x{fid:x}"),
                    "target_obj_id": obj_id,
                    "key": key,
                }));
            }
            BtWgValue::ObjKeyInline { obj: child } => {
                collect_ast_public_contract(
                    child,
                    parameters,
                    modulators,
                    uuids,
                    class_histogram,
                    total_objects,
                    depth + 1,
                );
            }
            BtWgValue::Obj(child) => {
                collect_ast_public_contract(
                    child,
                    parameters,
                    modulators,
                    uuids,
                    class_histogram,
                    total_objects,
                    depth + 1,
                );
            }
            BtWgValue::ObjList(list) => {
                for child in list {
                    collect_ast_public_contract(
                        child,
                        parameters,
                        modulators,
                        uuids,
                        class_histogram,
                        total_objects,
                        depth + 1,
                    );
                }
            }
            _ => {}
        }
    }

    if let (Some(nombre), Some(valor)) = (first_str, first_num) {
        if parameters.len() < 512 {
            parameters.push(json!({
                "obj_id": obj.obj_id,
                "cls": format!("0x{:x}", obj.cls),
                "nombre": nombre,
                "valor": valor,
            }));
        }
    }
}

/// Parsea un contenedor `BtWg00030002` en claro (`.bwpreset`, `Default.bwpreset`, etc.).
pub fn parse_btwg_0002(bytes: &[u8]) -> Result<Value, String> {
    let header = BtWgHeader::parse(bytes)?;
    if header.format_type != "0002" {
        return Err(format!(
            "se esperaba tipo BtWg 0002 en claro, se encontró '{}'",
            header.format_type
        ));
    }

    let mut meta_cursor = BtWgCursor::new(bytes, header.header_len);
    let meta = if meta_cursor.peek_u32() == Some(4) {
        meta_cursor.read_object(0).ok()
    } else {
        None
    };

    let body = if header.body_offset < bytes.len() {
        let mut body_cursor = BtWgCursor::new(bytes, header.body_offset);
        body_cursor.read_object(0).ok()
    } else {
        None
    };

    let contract = extract_public_contract(meta.as_ref(), body.as_ref());
    let device_name = contract["meta"]["device_name"].as_str();
    let device_id = contract["meta"]["device_id"].as_str();
    let compat = lookup_compatibility(device_name, device_id);

    Ok(json!({
        "header": header.to_json(),
        "contrato_publico": contract,
        "compatibilidad_hikaru": compat,
    }))
}

/// Busca un `Default.bwpreset` público asociado a un `.bwdevice` en disco.
fn find_companion_preset(
    device_path: &Path,
    device_name: &str,
    uuid: Option<&str>,
    library_dir: Option<&Path>,
) -> Option<PathBuf> {
    // 1. Mismo nombre `.bwpreset` o `Default.bwpreset` al lado del `.bwdevice`.
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

    // 2. Biblioteca explícita o `/opt/bitwig-studio/Library`.
    let default_opt = Path::new("/opt/bitwig-studio/Library");
    let mut roots: Vec<&Path> = Vec::new();
    if let Some(dir) = library_dir {
        roots.push(dir);
    }
    if default_opt.is_dir() {
        roots.push(default_opt);
    }

    if let Some(id) = uuid {
        for root in &roots {
            let candidate = root.join("device-settings").join(id).join("Default.bwpreset");
            if candidate.is_file() {
                return Some(candidate);
            }
            let direct = root.join(id).join("Default.bwpreset");
            if direct.is_file() {
                return Some(direct);
            }
        }
    }

    // 3. Búsqueda por `device_name` en `device-settings/*/Default.bwpreset` si es pequeña.
    for root in &roots {
        let settings = root.join("device-settings");
        if let Ok(entries) = fs::read_dir(settings) {
            for entry in entries.flatten().take(300) {
                let candidate = entry.path().join("Default.bwpreset");
                if !candidate.is_file() {
                    continue;
                }
                if let Ok(bytes) = fs::read(&candidate) {
                    if let Ok(parsed) = parse_btwg_0002(&bytes) {
                        if parsed["contrato_publico"]["meta"]["device_name"]
                            .as_str()
                            .map(|n| n.eq_ignore_ascii_case(device_name))
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

/// Inspecciona en modo *clean-room* (solo lectura) un `.bwdevice` o `.bwpreset`.
pub fn inspect_file(path: &Path, library_dir: Option<&Path>) -> Result<Value, String> {
    if !path.is_file() {
        return Err(format!("no existe el archivo: {}", path.display()));
    }
    let bytes = fs::read(path)
        .map_err(|err| format!("no se pudo leer {}: {err}", path.display()))?;
    let header = BtWgHeader::parse(&bytes)?;

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match header.format_type.as_str() {
        "0002" => {
            let parsed = parse_btwg_0002(&bytes)?;
            Ok(json!({
                "modo": "clean_room_read_only",
                "archivo": path.display().to_string(),
                "nombre_archivo": file_name,
                "extension": ext,
                "tamano_bytes": bytes.len(),
                "contenido_protegido": false,
                "header": parsed["header"],
                "contrato_publico": parsed["contrato_publico"],
                "compatibilidad_hikaru": parsed["compatibilidad_hikaru"],
            }))
        }
        "0004" => {
            let compat = lookup_compatibility(Some(&stem), None);
            let known_uuid = compat["canonical_uuid"].as_str();
            let companion = find_companion_preset(path, &stem, known_uuid, library_dir);
            let companion_contract = companion.as_ref().and_then(|p| {
                let cbytes = fs::read(p).ok()?;
                let parsed = parse_btwg_0002(&cbytes).ok()?;
                Some(json!({
                    "ruta": p.display().to_string(),
                    "contrato_publico": parsed["contrato_publico"],
                }))
            });

            Ok(json!({
                "modo": "clean_room_read_only",
                "archivo": path.display().to_string(),
                "nombre_archivo": file_name,
                "nombre_inferido": stem,
                "extension": ext,
                "tamano_bytes": bytes.len(),
                "contenido_protegido": true,
                "limite_clean_room": "Contenedor BtWg00030004 (meta ofuscado y grafo Nitro protegido). No se desofusca ni ejecuta código propietario.",
                "header": header.to_json(),
                "compatibilidad_hikaru": compat,
                "default_bwpreset_publico": companion_contract,
            }))
        }
        other => Err(format!(
            "tipo de contenedor BtWg desconocido '{other}' en {}",
            path.display()
        )),
    }
}

/// Lista artefactos `.bwdevice` / `.bwpreset` en un directorio (si existe) y devuelve el catálogo *clean-room* de compatibilidad.
pub fn list_devices(dir: Option<&Path>) -> Value {
    let mut discovered = Vec::<Value>::new();
    let search_dir = dir.or_else(|| {
        let default_lib = Path::new("/opt/bitwig-studio/Library");
        default_lib.is_dir().then_some(default_lib)
    });

    if let Some(root) = search_dir {
        collect_bitwig_files(root, &mut discovered, 0);
    }

    let registry: Vec<Value> = compatibility_registry()
        .iter()
        .map(|e| {
            json!({
                "bitwig_name": e.bitwig_name,
                "canonical_uuid": e.canonical_uuid,
                "category": e.category,
                "topology": e.topology,
                "hikaru_crate": e.hikaru_crate,
                "hikaru_module": e.hikaru_module,
                "status": e.implementation_status,
            })
        })
        .collect();

    json!({
        "directorio_escaneado": search_dir.map(|p| p.display().to_string()),
        "archivos_encontrados": discovered.len(),
        "archivos": discovered,
        "registro_compatibilidad_hikaru": registry,
    })
}

fn collect_bitwig_files(dir: &Path, out: &mut Vec<Value>, depth: usize) {
    if depth > 4 || out.len() >= 512 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_bitwig_files(&path, out, depth + 1);
        } else if crate::bitwig_inbox::is_supported_bitwig_artifact(&path) {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            out.push(json!({
                "nombre": stem,
                "extension": ext,
                "ruta": path.display().to_string(),
                "compatibilidad_hikaru": lookup_compatibility(Some(&stem), None),
            }));
            if out.len() >= 512 {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_u32_be(buf: &mut Vec<u8>, v: u32) {
        buf.extend_from_slice(&v.to_be_bytes());
    }

    fn write_str_be(buf: &mut Vec<u8>, s: &str) {
        write_u32_be(buf, s.len() as u32);
        buf.extend_from_slice(s.as_bytes());
    }

    fn build_synthetic_bwpreset() -> Vec<u8> {
        let mut meta = Vec::new();
        write_u32_be(&mut meta, 4);
        write_str_be(&mut meta, "meta");
        for (k, v) in [
            ("device_id", "20e18780-8438-48d3-b234-40dcbaa947b8"),
            ("device_name", "Comb"),
            ("device_category", "Audio FX"),
            ("device_type", "audio_to_audio"),
        ] {
            write_u32_be(&mut meta, 1);
            write_str_be(&mut meta, k);
            meta.push(0x08);
            write_str_be(&mut meta, v);
        }
        write_u32_be(&mut meta, 0);

        let body_offset = 42 + meta.len();
        let header = format!("BtWg0003000200ba{body_offset:08x}000000000000000000");

        let mut body = Vec::new();
        // Objeto raíz 0x561 (preset) con un sub-objeto parámetro ("FEEDBACK" = 0.45) y un obj_key modulado
        write_u32_be(&mut body, 0x561);
        // Campo 0x100 = sub-objeto parámetro cls=0x777
        write_u32_be(&mut body, 0x100);
        body.push(0x09);
        write_u32_be(&mut body, 0x777);
        write_u32_be(&mut body, 0x10);
        body.push(0x08);
        write_str_be(&mut body, "FEEDBACK");
        write_u32_be(&mut body, 0x11);
        body.push(0x07);
        body.extend_from_slice(&0.45f64.to_be_bytes());
        write_u32_be(&mut body, 0); // fin 0x777
        // Campo 0x101 = obj_key ref (0x1a) apuntando a obj 2 ("FEEDBACK")
        write_u32_be(&mut body, 0x101);
        body.push(0x1a);
        write_u32_be(&mut body, 1);
        write_u32_be(&mut body, 2);
        write_str_be(&mut body, "CONTENTS/FEEDBACK");
        write_u32_be(&mut body, 0); // fin 0x561

        let mut out = header.into_bytes();
        out.extend_from_slice(&meta);
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn parsea_bwpreset_btwg0002_y_extrae_contrato_publico() {
        let bytes = build_synthetic_bwpreset();
        let parsed = parse_btwg_0002(&bytes).expect("parsea preset sintético");

        assert_eq!(parsed["header"]["format_type"], "0002");
        assert_eq!(parsed["contrato_publico"]["meta"]["device_name"], "Comb");
        assert_eq!(
            parsed["contrato_publico"]["meta"]["device_id"],
            "20e18780-8438-48d3-b234-40dcbaa947b8"
        );
        assert_eq!(
            parsed["compatibilidad_hikaru"]["hikaru_module"],
            "crates/hikaru_dsp/src/effects/ultracomb.rs"
        );

        let params = parsed["contrato_publico"]["parametros_publicos"]
            .as_array()
            .expect("array de parámetros");
        assert_eq!(params.len(), 1);
        assert_eq!(params[0]["nombre"], "FEEDBACK");
        assert_eq!(params[0]["valor"], 0.45);

        let mods = parsed["contrato_publico"]["moduladores_y_keys"]
            .as_array()
            .expect("array de keys");
        assert_eq!(mods[0]["key"], "CONTENTS/FEEDBACK");
    }

    #[test]
    fn inspecciona_bwdevice_0004_sin_tocar_payload_y_resuelve_default_bwpreset() {
        let dir = std::env::temp_dir().join(format!("hikaru_bw_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let settings_dir = dir
            .join("device-settings")
            .join("20e18780-8438-48d3-b234-40dcbaa947b8");
        fs::create_dir_all(&settings_dir).unwrap();
        fs::write(settings_dir.join("Default.bwpreset"), build_synthetic_bwpreset()).unwrap();

        let dev_path = dir.join("Comb.bwdevice");
        let mut dev_bytes = b"BtWg0003000400c500001600000000000000000000".to_vec();
        dev_bytes.extend_from_slice(&[0xaa; 64]); // payload ofuscado simulado
        fs::write(&dev_path, &dev_bytes).unwrap();

        let info = inspect_file(&dev_path, Some(&dir)).expect("inspecciona bwdevice");
        assert_eq!(info["header"]["format_type"], "0004");
        assert_eq!(info["contenido_protegido"], true);
        assert_eq!(info["nombre_inferido"], "Comb");
        assert_eq!(
            info["compatibilidad_hikaru"]["hikaru_module"],
            "crates/hikaru_dsp/src/effects/ultracomb.rs"
        );
        assert_eq!(
            info["default_bwpreset_publico"]["contrato_publico"]["meta"]["device_name"],
            "Comb"
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
