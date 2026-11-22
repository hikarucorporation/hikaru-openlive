// Serializador del formato binario nativo de Bitwig Studio (`BtWg`).
// GNU AGPLv3
// crates/hikaru_gui/src/project/btwg.rs
//
// Genera archivos `template.bwproject` y `view-settings` conformes al formato
// de objetos binario `BtWg0003000200b8` de Bitwig Studio 5.x:
// - Cabecera ASCII de 42 bytes:
//   `BtWg0003000200b8{body_offset:08x}000000000000000000`
// - Objeto con nombre `meta` (`cls = 4`) con sub-documento `structure`
// - Árbol binario de proyecto (`cls = 0x2e`) con tabla de referencias 1-based
//   en pre-orden DFS (`REF1(id)` / `ref(id)`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::app::AppState;
use crate::views::matrix::ClipData;

/// Metadatos de un archivo de audio embebido en `project/samples/` para el
/// pool de samples (`0xd3c` -> `0x2c1` -> `0x4a6`/`0x4a9`) de Bitwig `BtWg`.
#[derive(Debug, Clone)]
pub struct BtWgSampleInfo {
    pub src_path: PathBuf,
    pub filename: String,
    pub rel_path: String,
    pub file_size: i32,
    pub sample_rate: i32,
    pub channels: u8,
    pub duration_secs: f64,
    pub hash16: [u8; 16],
}

/// Extrae `(sample_rate, channels, duration_secs)` de un buffer WAV PCM/float.
pub fn parse_wav_metadata(bytes: &[u8], fallback_dur_secs: f64) -> (i32, u8, f64) {
    let mut sr: i32 = 44100;
    let mut ch: u8 = 2;
    let mut bits_per_sample: u16 = 16;
    let mut data_bytes: Option<u32> = None;

    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        let mut pos = 12usize;
        while pos + 8 <= bytes.len() {
            let chunk_id = &bytes[pos..pos + 4];
            let chunk_len = u32::from_le_bytes([
                bytes[pos + 4],
                bytes[pos + 5],
                bytes[pos + 6],
                bytes[pos + 7],
            ]) as usize;
            pos += 8;
            if pos + chunk_len > bytes.len() {
                break;
            }
            if chunk_id == b"fmt " && chunk_len >= 16 {
                ch = u16::from_le_bytes([bytes[pos + 2], bytes[pos + 3]]).clamp(1, 8) as u8;
                let parsed_sr = u32::from_le_bytes([
                    bytes[pos + 4],
                    bytes[pos + 5],
                    bytes[pos + 6],
                    bytes[pos + 7],
                ]);
                if (8000..=384000).contains(&parsed_sr) {
                    sr = parsed_sr as i32;
                }
                bits_per_sample =
                    u16::from_le_bytes([bytes[pos + 14], bytes[pos + 15]]).max(8);
            } else if chunk_id == b"data" {
                data_bytes = Some(chunk_len as u32);
            }
            pos += chunk_len + (chunk_len & 1);
        }
    }

    let dur = if let Some(db) = data_bytes {
        let bytes_per_frame = (ch as usize) * ((bits_per_sample as usize) / 8).max(1);
        if bytes_per_frame > 0 && sr > 0 && db > 0 {
            (db as f64 / bytes_per_frame as f64) / sr as f64
        } else {
            fallback_dur_secs.max(0.1)
        }
    } else {
        fallback_dur_secs.max(0.1)
    };
    (sr, ch, dur)
}

/// Hash determinístico de 16 bytes para identificar el contenido de un sample.
pub fn sample_hash16(bytes: &[u8]) -> [u8; 16] {
    let mut h1: u64 = 0xcbf29ce484222325;
    let mut h2: u64 = 0x84222325cbf29ce4;
    for (i, &b) in bytes.iter().enumerate() {
        h1 ^= b as u64;
        h1 = h1.wrapping_mul(0x100000001b3);
        h2 ^= (b as u64).wrapping_add(i as u64);
        h2 = h2.wrapping_mul(0x9e3779b97f4a7c15);
    }
    let mut out = [0u8; 16];
    out[0..8].copy_from_slice(&h1.to_be_bytes());
    out[8..16].copy_from_slice(&h2.to_be_bytes());
    out
}

// =========================================================================
// ESCRITOR BINARIO `BtWg`
// =========================================================================

pub struct BtWgWriter {
    buf: Vec<u8>,
    next_obj_id: u32,
}

impl BtWgWriter {
    pub fn new() -> Self {
        Self {
            buf: Vec::with_capacity(8192),
            next_obj_id: 1,
        }
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub fn i16(&mut self, v: i16) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    pub fn i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    pub fn f32(&mut self, v: f32) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    pub fn f64(&mut self, v: f64) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    pub fn raw_str(&mut self, s: &str) {
        let bytes = s.as_bytes();
        self.u32(bytes.len() as u32);
        self.buf.extend_from_slice(bytes);
    }

    /// Inicia un objeto numérico y devuelve su ID 1-based en la tabla DFS.
    pub fn begin_obj(&mut self, cls: u32) -> u32 {
        let id = self.next_obj_id;
        self.next_obj_id += 1;
        self.u32(cls);
        id
    }

    pub fn end_obj(&mut self) {
        self.u32(0);
    }

    // ---- Campos con ID numérico ----

    pub fn field_i8(&mut self, fid: u32, v: u8) {
        self.u32(fid);
        self.u8(0x01);
        self.u8(v);
    }

    pub fn field_i16(&mut self, fid: u32, v: i16) {
        self.u32(fid);
        self.u8(0x02);
        self.i16(v);
    }

    pub fn field_i32(&mut self, fid: u32, v: i32) {
        self.u32(fid);
        self.u8(0x03);
        self.i32(v);
    }

    pub fn field_i64(&mut self, fid: u32, v: u64) {
        self.u32(fid);
        self.u8(0x04);
        self.u64(v);
    }

    pub fn field_bool(&mut self, fid: u32, v: bool) {
        self.u32(fid);
        self.u8(0x05);
        self.u8(u8::from(v));
    }

    pub fn field_f32(&mut self, fid: u32, v: f32) {
        self.u32(fid);
        self.u8(0x06);
        self.f32(v);
    }

    pub fn field_f64(&mut self, fid: u32, v: f64) {
        self.u32(fid);
        self.u8(0x07);
        self.f64(v);
    }

    pub fn field_str(&mut self, fid: u32, s: &str) {
        self.u32(fid);
        self.u8(0x08);
        self.raw_str(s);
    }

    pub fn field_bytes(&mut self, fid: u32, data: &[u8]) {
        self.u32(fid);
        self.u8(0x0d);
        self.u32(data.len() as u32);
        self.buf.extend_from_slice(data);
    }

    pub fn field_obj_header(&mut self, fid: u32, cls: u32) -> u32 {
        self.u32(fid);
        self.u8(0x09);
        self.begin_obj(cls)
    }

    pub fn field_null(&mut self, fid: u32) {
        self.u32(fid);
        self.u8(0x0a);
    }

    pub fn field_ref(&mut self, fid: u32, obj_id: u32) {
        self.u32(fid);
        self.u8(0x0b);
        self.u32(obj_id);
    }

    pub fn field_obj_list_start(&mut self, fid: u32) {
        self.u32(fid);
        self.u8(0x12);
    }

    pub fn obj_list_end(&mut self) {
        self.u32(3);
    }

    pub fn field_empty_obj_list(&mut self, fid: u32) {
        self.field_obj_list_start(fid);
        self.obj_list_end();
    }

    pub fn field_uuid(&mut self, fid: u32, uuid: [u8; 16]) {
        self.u32(fid);
        self.u8(0x15);
        self.buf.extend_from_slice(&uuid);
    }

    pub fn field_color(&mut self, fid: u32, rgba: [f32; 4]) {
        self.u32(fid);
        self.u8(0x16);
        for c in rgba {
            self.f32(c);
        }
    }

    pub fn field_obj_key_ref(&mut self, fid: u32, obj_id: u32, key: &str) {
        self.u32(fid);
        self.u8(0x1a);
        self.u32(1); // REF1
        self.u32(obj_id);
        self.raw_str(key);
    }

    pub fn field_obj_key_inline_start(&mut self, fid: u32, cls: u32) -> u32 {
        self.u32(fid);
        self.u8(0x1a);
        self.begin_obj(cls)
    }

    // ---- Campos con nombre (para objeto `meta`, `cls = 4`) ----

    pub fn named_field_str(&mut self, name: &str, value: &str) {
        self.u32(1);
        self.raw_str(name);
        self.u8(0x08);
        self.raw_str(value);
    }

    pub fn named_field_i32(&mut self, name: &str, value: i32) {
        self.u32(1);
        self.raw_str(name);
        self.u8(0x03);
        self.i32(value);
    }

    pub fn named_field_bytes(&mut self, name: &str, value: &[u8]) {
        self.u32(1);
        self.raw_str(name);
        self.u8(0x0d);
        self.u32(value.len() as u32);
        self.buf.extend_from_slice(value);
    }

    pub fn named_field_str_arr(&mut self, name: &str, values: &[String]) {
        self.u32(1);
        self.raw_str(name);
        self.u8(0x19);
        self.u32(values.len() as u32);
        for s in values {
            self.raw_str(s);
        }
    }
}

/// Genera un UUID v4 determinístico a partir de una semilla (sin dependencias extra).
fn det_uuid(seed: u64, salt: u64) -> [u8; 16] {
    let mut x = seed.wrapping_mul(0x9E3779B97F4A7C15) ^ salt.wrapping_mul(0xBF58476D1CE4E5B9);
    let mut out = [0u8; 16];
    for chunk in out.chunks_exact_mut(8) {
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        let v = x.wrapping_mul(0x2545F4914F6CDD1D);
        chunk.copy_from_slice(&v.to_be_bytes());
    }
    out[6] = (out[6] & 0x0f) | 0x40;
    out[8] = (out[8] & 0x3f) | 0x80;
    out
}

fn color32_to_rgba_f32(c: egui::Color32) -> [f32; 4] {
    [
        c.r() as f32 / 255.0,
        c.g() as f32 / 255.0,
        c.b() as f32 / 255.0,
        c.a() as f32 / 255.0,
    ]
}

// =========================================================================
// ARCHIVOS AUXILIARES DEL ZIP `.bwtemplate`
// =========================================================================

/// Genera `template-info.json` en formato HOCON nativo de Bitwig Studio.
pub fn encode_template_info(title: &str) -> Vec<u8> {
    let safe_title = title.replace('"', "\\\"");
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(1700000000000);
    format!(
        "{{\n\
         \tname : \"{safe_title}\"\n\
         \tauthor : \"Hikaru OpenLive\"\n\
         \tcategory : Template\n\
         \tdescription : \"Exported from Hikaru OpenLive.\"\n\
         \turl : \"\"\n\
         \ttags : \"\"\n\
         \tshow_welcome_screen : false\n\
         \tcreation_date : {now_ms}\n\
         }}"
    )
    .into_bytes()
}

/// Genera un archivo `view-settings` binario `BtWg` (`cls = 0x987`) válido mínimo.
pub fn encode_minimal_view_settings() -> Vec<u8> {
    let mut out = b"BtWg0003000200000000002b000000000000000001\n".to_vec();
    let mut w = BtWgWriter::new();
    w.begin_obj(0x987);
    w.field_obj_list_start(0x219a);
    w.begin_obj(0x985);
    w.field_str(0x2191, "Single Display (Large)");
    w.field_str(0x2192, "ARRANGE");
    w.field_empty_obj_list(0x2196);
    w.end_obj();
    w.obj_list_end();
    w.end_obj();
    out.extend_from_slice(&w.into_bytes());
    out
}

// =========================================================================
// SERIALIZACIÓN DE `template.bwproject` (`BtWg0003000200b8`)
// =========================================================================

fn encode_structure_subdoc(tracks: &[(String, bool)]) -> Vec<u8> {
    let mut out = b"BtWg0003000200000000002b000000000000000001\n".to_vec();
    let mut w = BtWgWriter::new();
    w.begin_obj(0x285);
    w.field_str(0xad3, "");
    w.field_obj_header(0xad2, 0x283);
    w.field_str(0xad1, "Project");
    w.field_obj_list_start(0xace);
    for (name, is_audio) in tracks {
        // En BtWg structure: 0x287 = Audio Track, 0x288 = Instrument Track
        w.begin_obj(if *is_audio { 0x287 } else { 0x288 });
        w.field_str(0xad1, name);
        w.end_obj();
    }
    w.obj_list_end();
    w.field_empty_obj_list(0xacf);
    w.field_obj_header(0xad0, 0x28b);
    w.field_str(0xad1, "Master");
    w.end_obj();
    w.end_obj();
    w.end_obj();
    out.extend_from_slice(&w.into_bytes());
    out
}

fn write_param_common(w: &mut BtWgWriter, name: &str) {
    w.field_null(0x1832);
    w.field_i8(0x25f, 0);
    w.field_i8(0x260, 0);
    w.field_str(0x2bd, name);
    w.field_str(0x335, "");
    w.field_bool(0x2941, false);
    w.field_str(0xef2, "");
    w.field_bool(0x1d41, true);
    w.field_bool(0x1d42, true);
}

fn write_f64_param(w: &mut BtWgWriter, fid: u32, cls: u32, name: &str, value: f64) -> u32 {
    let id = w.field_obj_header(fid, cls);
    write_param_common(w, name);
    w.field_f64(0x2c8, value);
    w.end_obj();
    id
}

fn write_volume_param(w: &mut BtWgWriter, fid: u32, name: &str, value: f64) -> u32 {
    let id = w.field_obj_header(fid, 0x4c4);
    write_param_common(w, name);
    w.field_f64(0x2c8, value);
    w.field_bool(0x3643, false);
    w.end_obj();
    id
}

fn write_pan_param(w: &mut BtWgWriter, fid: u32, pan: f64) {
    w.field_obj_header(fid, 0x4c3);
    write_param_common(w, "PANORAMA");
    w.field_f64(0x2c8, pan);
    w.field_bool(0x3644, true);
    w.end_obj();
}

fn write_bool_param(w: &mut BtWgWriter, fid: u32, cls: u32, name: &str, value: bool) {
    w.field_obj_header(fid, cls);
    write_param_common(w, name);
    w.field_bool(0xd2, value);
    w.end_obj();
}

fn write_enum_param(w: &mut BtWgWriter, fid: u32, cls: u32, name: &str, value: u8) {
    w.field_obj_header(fid, cls);
    write_param_common(w, name);
    w.field_i8(0x1c9, value);
    w.end_obj();
}

fn write_track_controls(w: &mut BtWgWriter, seed: u64) {
    w.field_obj_header(0x35cb, 0x1275);
    w.field_null(0x1832);
    w.field_uuid(0xf, det_uuid(seed, 0x35cb));

    w.field_obj_header(0x339, 0x188);
    w.field_bool(0x32d, true);
    w.field_bool(0xe22, false);
    w.field_bool(0x1aab, false);
    w.field_i8(0x3a4, 255);
    w.field_str(0xba9, "");
    w.field_bool(0xbaa, false);
    w.field_i8(0xbab, 1);
    w.field_i8(0x138e, 0);
    w.field_i8(0x1722, 0);
    w.field_bool(0x25ce, false);
    w.field_bool(0x1cf4, false);
    w.field_i8(0x1cf5, 0);
    w.field_i8(0x1cf6, 0);
    w.field_bool(0x25c2, false);
    w.field_i8(0x1cf7, 0);
    w.field_i8(0x1cf8, 0);
    w.field_bool(0x25d2, false);
    w.field_i8(0x25d7, 255);
    w.end_obj();

    w.field_str(0x155a, "");

    w.field_obj_header(0x197, 0x1279);
    w.field_str(0x2b9, "");
    w.field_str(0x12de, "");
    w.field_str(0x9a, "Track Controls");
    w.field_str(0x1559, "");
    w.field_str(0x9b, "Track");
    w.field_str(0x9c, "");
    w.field_str(0x9e, "");
    w.field_str(0x9f, "");
    w.field_str(0xa1, "");
    w.field_str(0xa2, "");
    w.field_bool(0xa3, true);
    w.field_i8(0x3550, 1);
    w.field_i8(0x9d, 1);
    w.field_bool(0x137e, true);

    w.field_obj_header(0x18f5, 0x75f);
    w.field_str(0x2b9, "MODULATORS");
    w.field_empty_obj_list(0x1a46);
    w.end_obj();

    w.field_obj_header(0x1a85, 0x77d);
    w.field_empty_obj_list(0x1a7e);
    w.end_obj();

    w.field_obj_header(0x1b75, 0x7b1);
    w.field_str(0x1b72, "");
    w.field_i8(0x1b73, 0);
    w.field_bool(0x1b74, true);
    w.field_i8(0x37ba, 255);
    w.end_obj();

    w.field_null(0x17cd);
    w.field_empty_obj_list(0x1945);
    w.field_bool(0x1a8c, true);
    w.field_bool(0x1a8d, false);
    w.field_bool(0x1aa9, false);

    w.field_obj_header(0x35d9, 0xd3);
    w.field_str(0x2b9, "CONTENTS");
    w.field_empty_obj_list(0x20c);
    w.field_i8(0x2bae, 0);
    w.field_i8(0x2baf, 11);
    w.field_i8(0x2bb0, 1);
    w.end_obj();

    w.end_obj(); // 0x1279

    w.field_str(0x199, "");
    w.field_str(0x19a, "");
    w.field_str(0x19b, "");
    w.field_str(0x554, "");
    w.field_str(0xea0, "");
    w.field_str(0xea1, "");
    w.field_str(0x185d, "");
    w.field_i8(0x3551, 1);
    w.field_bool(0x1a9f, false);
    w.field_bool(0x137f, true);

    w.field_obj_header(0x366b, 0x240);
    w.field_null(0x89a);
    w.field_null(0x1178);
    w.end_obj();

    w.end_obj(); // 0x1275
}

fn write_grid_settings(w: &mut BtWgWriter, fid: u32) {
    w.field_obj_header(fid, 0xd2);
    w.field_bool(0x208, true);
    w.field_bool(0x209, true);
    w.field_bool(0x20a, true);
    w.field_bool(0x1203, true);
    w.field_i8(0x11fd, 4);
    w.field_i8(0x120c, 4);
    w.field_i8(0x11ff, 1);
    w.end_obj();
}

fn write_audio_launcher_clip(
    w: &mut BtWgWriter,
    clip: &crate::views::matrix::MatrixClip,
    bpm: f64,
    time_sig_packed: i32,
    sample_info: &BtWgSampleInfo,
    stream_ref_id: u32,
) {
    let dur_secs = if sample_info.duration_secs > 0.0 {
        sample_info.duration_secs
    } else {
        clip.duration_secs.max(0.25)
    };
    let dur_beats = (dur_secs * (bpm / 60.0)).max(0.25);
    let color = clip
        .color
        .map(color32_to_rgba_f32)
        .unwrap_or([0.0, 0.0, 0.0, 0.0]);

    // Audio Launcher Clip (`0xee`)
    w.field_obj_header(0x240, 0xee);
    w.field_f64(0x2af, 0.0);
    w.field_f64(0x26, dur_beats);
    w.field_bool(0x10f8, clip.muted);

    // Contenido de Audio Clip (`0x105`)
    w.field_obj_header(0x288, 0x105);
    w.field_i8(0x1fd, 0);

    w.field_obj_header(0x98f, 0x267);
    w.field_i8(0x1fd, 0);
    w.field_empty_obj_list(0x21f);
    w.field_bool(0x991, clip.loop_enabled);
    w.field_obj_header(0x992, 0x58);
    w.field_f64(0x2af, 0.0);
    w.field_f64(0x26, dur_beats);
    w.end_obj();
    w.field_obj_header(0x98c, 0x269);
    w.field_f64(0x2af, 0.0);
    w.end_obj();
    w.field_obj_header(0x98d, 0x268);
    w.field_f64(0x2af, dur_beats);
    w.end_obj();
    w.end_obj(); // 0x267

    w.field_empty_obj_list(0x49d);
    w.field_empty_obj_list(0xbac);
    w.field_empty_obj_list(0xbad);
    w.field_bool(0x281, true);
    w.field_f64(0x940, 0.0);
    w.field_i32(0x1d4b, time_sig_packed);

    // Contenedor de Audio Events (`0x6ce` -> `0xce` -> `0xd4`)
    w.field_obj_header(0x49c, 0x6ce);
    w.field_i8(0x1fd, 0);
    w.field_obj_list_start(0x18cb);
    w.begin_obj(0xce);
    w.field_i8(0x1fd, 0);
    w.field_obj_list_start(0x21f);

    // Audio Event (`0xd4`)
    w.begin_obj(0xd4);
    w.field_f64(0x2af, 0.0);
    w.field_f64(0x26, dur_beats);
    w.field_bool(0x10f8, false);

    // Sample Playback (`0x9f`)
    w.field_obj_header(0x288, 0x9f);
    w.field_i8(0x1fd, 0);
    w.field_null(0xc33);
    w.field_null(0xc34);
    w.field_null(0xc35);
    w.field_obj_header(0x20d, 0x1db);
    w.field_ref(0x4b4, stream_ref_id);
    w.end_obj();
    w.field_bool(0x10ad, false);
    w.field_i8(0x4b0, 3);
    w.field_i8(0xecc, 1);
    w.field_i8(0x25bf, 0);
    w.field_i8(0x1424, 1);
    w.field_i8(0x25cb, 2);
    w.field_bool(0xa09, false);
    w.field_bool(0x1d26, true);
    w.field_i16(0x1d27, 128);

    // Warp / Stretch markers (`0x192` -> `0x8b` -> `0x43`)
    w.field_obj_header(0x192, 0x8b);
    w.field_i8(0x1fd, 0);
    w.field_obj_list_start(0x21f);
    w.begin_obj(0x43);
    w.field_f64(0x2af, 0.0);
    w.field_f64(0xa6, 0.0);
    w.end_obj();
    w.begin_obj(0x43);
    w.field_f64(0x2af, dur_beats);
    w.field_f64(0xa6, dur_secs);
    w.end_obj();
    w.obj_list_end();
    w.end_obj();

    // Onsets container (`0x10ea` -> `0x36b` -> `0xb6`)
    w.field_obj_header(0x10ea, 0x36b);
    w.field_obj_header(0x10e9, 0xb6);
    w.field_i8(0x1fd, 0);
    w.field_empty_obj_list(0x21f);
    w.end_obj();
    w.end_obj();

    w.field_null(0x1cd9);
    w.end_obj(); // 0x9f

    w.field_f64(0xc04, 0.0);
    w.field_bool(0x2e04, true);
    w.field_f64(0x2dfc, 1.0);
    w.field_bool(0x2e05, true);
    w.field_i8(0x2dfe, 0);
    w.field_bool(0x2e06, true);
    w.field_i8(0x2e02, 8);
    w.field_bool(0x2e5c, true);
    w.field_i8(0x2e5e, 0);
    w.field_f64(0x2e6d, 0.0);
    w.field_f64(0xc05, dur_secs);
    w.field_f64(0x1c0a, 0.0);
    w.field_f64(0x1259, 0.0);
    w.field_f64(0x125a, 0.0);
    w.field_f64(0x19f7, 1.0);
    w.field_f64(0x19f8, 1.0);
    w.field_i8(0x19f9, 1);
    w.field_i8(0x19fa, 1);
    w.field_f64(0x2f59, 0.215443469);
    w.end_obj(); // 0xd4

    w.obj_list_end();
    w.field_i8(0xee, 0);
    w.field_str(0x18ca, "");
    w.end_obj(); // 0xce
    w.obj_list_end();
    w.end_obj(); // 0x6ce

    w.field_null(0x1aa5);
    w.field_bool(0x1aa6, false);
    w.field_empty_obj_list(0x2d52);
    w.field_empty_obj_list(0x2df2);
    w.field_obj_header(0x2db7, 0xf89);
    w.field_i8(0x1fd, 0);
    w.field_empty_obj_list(0x21f);
    w.end_obj();
    w.field_f64(0x1d56, 0.02);
    w.end_obj(); // 0x105

    w.field_str(0x286, &clip.name);
    w.field_color(0x280, color);
    w.field_i8(0x4a2, 0);
    w.field_bool(0x3776, false);
    w.field_bool(0xbe1, clip.launch_legato);
    w.field_bool(0x3771, true);
    w.field_i8(0x3772, 1);
    w.field_f64(0x4a3, dur_beats);
    w.field_i8(0x2845, 255);
    w.field_i8(0x36db, 255);
    w.field_i8(0x36da, 255);
    w.field_i8(0x36dc, 255);
    w.field_i8(0x94b, 0);
    w.field_i8(0x36dd, 0);

    w.field_obj_header(0x1d5b, 0x812);
    w.field_bool(0x1d61, false);
    w.field_f64(0x1d5e, 0.01);
    w.field_i8(0x1d5f, 255);
    write_grid_settings(w, 0x1d60);
    w.field_obj_header(0x1d5c, 0xc1);
    w.field_i8(0xc1d, 255);
    w.field_f64(0xc1e, 10.0);
    w.field_i8(0x955, 0);
    w.end_obj();
    w.field_obj_header(0x1d5d, 0x2a6);
    w.field_i8(0xc1d, 255);
    w.field_f64(0xc1e, 12.0);
    w.end_obj();
    w.end_obj(); // 0x812

    w.field_i8(0x2ed4, 0);
    w.field_f64(0x1709, -1.0);
    w.field_str(0xb8e, "");
    w.field_null(0x11ae);
    w.field_f64(0x1947, 0.0);
    w.field_f64(0x1948, 0.0);
    w.field_f64(0x19fd, 1.0);
    w.field_f64(0x19fe, 1.0);
    w.field_i8(0x19ff, 1);
    w.field_i8(0x1a00, 1);
    w.end_obj(); // 0xee
}

fn write_launcher_clip(
    w: &mut BtWgWriter,
    clip: &crate::views::matrix::MatrixClip,
    bpm: f64,
    sample_rel_path: Option<&str>,
) {
    let dur_beats = if clip.duration_secs > 0.0 {
        (clip.duration_secs * (bpm / 60.0)).max(1.0)
    } else {
        4.0
    };
    let color = clip
        .color
        .map(color32_to_rgba_f32)
        .unwrap_or([0.0, 0.0, 0.0, 0.0]);

    w.field_obj_header(0x240, 0x47);
    w.field_f64(0x2af, 0.0);
    w.field_f64(0x26, dur_beats);
    w.field_bool(0x10f8, clip.muted);

    // Contenido del clip (`0xbf`)
    w.field_obj_header(0x288, 0xbf);
    w.field_i8(0x1fd, 0);

    w.field_obj_header(0x98f, 0x267);
    w.field_i8(0x1fd, 0);
    w.field_empty_obj_list(0x21f);
    w.field_bool(0x991, clip.loop_enabled);
    w.field_obj_header(0x992, 0x58);
    w.field_f64(0x2af, 0.0);
    w.field_f64(0x26, dur_beats);
    w.end_obj();
    w.field_obj_header(0x98c, 0x269);
    w.field_f64(0x2af, 0.0);
    w.end_obj();
    w.field_obj_header(0x98d, 0x268);
    w.field_f64(0x2af, dur_beats);
    w.end_obj();
    w.end_obj(); // 0x267

    w.field_empty_obj_list(0x49d);
    w.field_empty_obj_list(0xbac);
    w.field_empty_obj_list(0xbad);
    w.field_bool(0x281, true);
    w.field_f64(0x940, 0.0);
    w.field_i32(0x1d4b, 268435492);

    // Notas MIDI (`0x6cc`)
    w.field_obj_header(0x49c, 0x6cc);
    w.field_i8(0x1fd, 0);
    w.field_obj_list_start(0x18cb);
    if let ClipData::Midi { notes } = &clip.content {
        // Agrupar por pitch
        let mut by_key: std::collections::BTreeMap<u8, Vec<(u64, u8, u32)>> =
            std::collections::BTreeMap::new();
        for &(st, key, vel, dur) in notes {
            by_key.entry(key).or_default().push((st, vel, dur));
        }
        for (key, key_notes) in by_key {
            w.begin_obj(0x42);
            w.field_i8(0x1fd, 0);
            w.field_obj_list_start(0x21f);
            for (st, vel, dur) in key_notes {
                w.begin_obj(0x66);
                w.field_f64(0x2af, st as f64 / 960.0);
                w.field_f64(0x26, (dur as f64 / 960.0).max(0.0625));
                w.field_bool(0x10f8, false);
                w.field_obj_header(0x288, 0x29);
                w.field_i8(0x1fd, 0);
                w.field_null(0xc33);
                w.field_null(0xc34);
                w.field_null(0xc35);
                w.end_obj();
                w.field_f64(0xc04, 0.0);
                w.field_bool(0x2e04, true);
                w.field_f64(0x2dfc, 1.0);
                w.field_bool(0x2e05, true);
                w.field_i8(0x2dfe, 0);
                w.field_bool(0x2e06, true);
                w.field_i8(0x2e02, 8);
                w.field_bool(0x2e5c, true);
                w.field_i8(0x2e5e, 0);
                w.field_f64(0x2e6d, 0.0);
                w.field_f64(0xef, (vel as f64 / 127.0).clamp(0.0, 1.0));
                w.field_f64(0xf0, 0.5);
                w.field_f64(0x2e62, 0.0);
                w.field_f64(0x2e6a, 0.0);
                w.field_f64(0x2e6e, 0.0);
                w.end_obj();
            }
            w.obj_list_end();
            w.field_i8(0xee, key);
            w.field_i8(0x2681, 0);
            w.end_obj();
        }
    }
    w.obj_list_end();
    w.end_obj(); // 0x6cc

    w.field_null(0x1aa5);
    w.field_bool(0x1aa6, false);
    w.end_obj(); // 0xbf

    w.field_str(0x286, &clip.name);
    w.field_color(0x280, color);
    w.field_i8(0x4a2, 0);
    w.field_bool(0x3776, false);
    w.field_bool(0xbe1, clip.launch_legato);
    w.field_bool(0x3771, true);
    w.field_i8(0x3772, 1);
    w.field_f64(0x4a3, dur_beats);
    w.field_i8(0x2845, 255);
    w.field_i8(0x36db, 255);
    w.field_i8(0x36da, 255);
    w.field_i8(0x36dc, 255);
    w.field_i8(0x94b, 0);
    w.field_i8(0x36dd, 0);

    w.field_obj_header(0x1d5b, 0x812);
    w.field_bool(0x1d61, true);
    w.field_f64(0x1d5e, 0.02);
    w.field_i64(0x1d5f, 2251799813685236);
    write_grid_settings(w, 0x1d60);
    w.field_obj_header(0x1d5c, 0xc1);
    w.field_i16(0xc1d, 402);
    w.field_f64(0xc1e, 11.0);
    w.field_i8(0x955, 0);
    w.end_obj();
    w.field_obj_header(0x1d5d, 0x2a6);
    w.field_i8(0xc1d, 255);
    w.field_f64(0xc1e, 12.0);
    w.end_obj();
    w.end_obj(); // 0x812

    w.field_i8(0x2ed4, 0);
    w.field_f64(0x1709, -1.0);
    w.field_str(0xb8e, sample_rel_path.unwrap_or(""));
    w.end_obj(); // 0x47
}

fn write_master_track_inline(
    w: &mut BtWgWriter,
    project_id: u32,
    transport_id: u32,
    master: &crate::views::mixer::Track,
    num_scenes: usize,
) -> u32 {
    let master_id = w.field_obj_key_inline_start(0x89b, 0x90);
    w.field_null(0x1832);
    w.field_uuid(0xf, det_uuid(0, 0x1001));
    w.field_str(0x15b, "");
    w.field_color(0x54c, [0.0, 0.0, 0.0, 0.0]);
    w.field_str(0x350c, "");

    w.field_obj_header(0x168, 0x88);
    w.field_i8(0x2603, 64);
    w.field_bool(0x580, false);
    w.field_i8(0x1aa7, 42);
    let lane_id = w.field_obj_header(0x4e5, 0x23c);
    w.field_obj_header(0x2a3, 0xf1);
    let vol_id = write_volume_param(w, 0x41, "VOLUME", master.volume.clamp(0.0, 1.5) as f64);
    w.field_str(0x133b, "Mixer");
    w.field_str(0x133a, "Volume");
    w.end_obj();
    w.field_i8(0xbbd, 1);
    w.end_obj();
    w.field_ref(0x4e6, lane_id);
    w.field_bool(0x4f6, false);
    w.field_i8(0x4fd, 61);
    w.field_bool(0xb32, false);
    w.field_bool(0xdfd, false);
    w.field_bool(0x115a, true);
    w.field_obj_key_ref(0x12a9, master_id, "2196");
    w.field_bool(0x1432, false);
    w.field_i8(0x16c7, 0);
    w.field_bool(0x16d0, true);
    w.field_bool(0x16ca, false);
    w.field_str(0x16cc, "velocity");
    w.field_i8(0x16cd, 0);
    w.field_bool(0x16ce, true);
    w.field_i8(0x26bd, 0);
    w.field_i8(0x271f, 0);
    w.end_obj();

    w.field_bool(0x1380, true);
    w.field_i8(0x26d5, 255);
    w.field_i8(0x26d6, 255);
    w.field_uuid(0xa51, det_uuid(0, 0x1002));
    w.field_bool(0x162, true);
    write_track_controls(w, 0);
    w.field_bool(0x1aa0, false);
    w.field_i8(0x927, 2); // Master track type = 2

    w.field_obj_header(0x15d, 0xe1);
    w.field_i8(0x1fd, 0);
    w.field_str(0x236, "Untitled");
    w.field_obj_header(0x238, 0x52);
    w.field_i8(0x1fd, 0);
    w.field_empty_obj_list(0x21f);
    w.end_obj();
    w.field_empty_obj_list(0x239);
    w.end_obj();

    w.field_obj_header(0x896, 0x240);
    w.field_null(0x89a);
    w.field_null(0x1178);
    w.end_obj();

    w.field_obj_header(0x897, 0x241);
    w.field_obj_key_ref(0x89b, project_id, "4489/4488");
    w.field_null(0x1177);
    w.field_i8(0x13d3, 0);
    w.end_obj();

    w.field_empty_obj_list(0x4ba);

    w.field_obj_header(0x15e, 0x1e);
    w.field_obj_list_start(0x231);
    for _ in 0..num_scenes {
        w.begin_obj(0xe3);
        w.field_null(0x240);
        w.field_bool(0xbcd, true);
        w.end_obj();
    }
    w.obj_list_end();
    w.end_obj();

    w.field_null(0x15f);

    w.field_obj_header(0x164, 0x8a);
    w.field_null(0x1832);
    w.field_obj_header(0x331, 0x27);
    w.field_bool(0xb3f, false);
    w.end_obj();
    w.field_empty_obj_list(0x144);
    w.end_obj();

    w.field_obj_header(0x165, 0xab);
    w.field_null(0x1832);
    w.field_ref(0x1a4, vol_id);
    write_pan_param(w, 0x1a5, master.pan.clamp(-1.0, 1.0) as f64);
    write_bool_param(w, 0x1a6, 0x4ce, "MUTE", master.mute);
    w.field_bool(0x1a8, master.solo);
    w.field_empty_obj_list(0xa5);
    write_enum_param(w, 0x1456, 0x57d, "CROSSFADE_MODE", 1);
    w.end_obj();

    w.field_bool(0xf2, false);
    w.field_i8(0xaa7, 1);
    w.field_obj_header(0xbfd, 0x2a5);
    w.field_null(0x1832);
    w.field_empty_obj_list(0xbfc);
    w.end_obj();
    w.field_bool(0xbfe, true);
    w.field_i8(0x12c3, 2);
    w.field_i8(0x12c4, 254);
    w.field_bool(0xacc, false);
    w.field_ref(0x190, transport_id);
    w.field_f64(0x1374, -1.0);
    w.field_f64(0x1375, -1.0);
    w.field_obj_header(0x1c15, 0x22e);
    w.field_null(0x1832);
    w.field_i8(0x25f, 0);
    w.field_str(0x176, "");
    w.field_bool(0x2afa, true);
    w.field_f32(0xa9, 100.0);
    w.end_obj();

    w.end_obj(); // Master 0x90
    w.raw_str("361");
    master_id
}

fn write_editor_pane_common(w: &mut BtWgWriter, tool_cls: u32, tool_fid: u32) {
    w.field_obj_header(0xe70, 0x307);
    w.field_null(0xb5e);
    w.field_empty_obj_list(0x25b);
    w.field_f64(0xe45, 0.0);
    w.field_f64(0xe46, 0.0);
    w.end_obj();

    w.field_obj_header(0xf76, 0x328);
    w.field_null(0xb5e);
    w.field_empty_obj_list(0x25b);
    w.field_bool(0xfc2, false);
    w.field_f64(0x1ac8, 0.0);
    w.field_f64(0x1ac9, 0.0);
    w.end_obj();

    w.field_bool(0x57b, false);
    w.field_f64(0x455, 0.01);
    w.field_i8(0x1258, 255);
    w.field_bool(0x1186, true);
    write_grid_settings(w, 0x11fc);
    w.field_bool(0x2f18, true);
    w.field_obj_header(0x1a5a, tool_cls);
    w.field_i8(tool_fid, 0);
    w.end_obj();
    w.field_bool(0x171e, true);
    w.field_bool(0x1721, true);
    w.field_bool(0x2926, false);
    w.field_obj_header(0x29dc, 0xdd5);
    w.field_i16(0x29e9, 1001);
    w.field_i8(0x29ee, 1);
    w.end_obj();
    w.field_bool(0xe5e, false);
}

fn write_arranger_editor_pane(w: &mut BtWgWriter) {
    w.field_obj_header(0x737, 0x208);
    write_editor_pane_common(w, 0x768, 0x1a61);
    w.field_obj_header(0xe7e, 0x30c);
    w.field_bool(0xdf8, true);
    w.field_empty_obj_list(0xe7f);
    w.end_obj();
    w.field_bool(0xe18, true);
    w.field_i16(0x1606, 200);
    w.field_i8(0x27a, 255);
    w.field_i8(0x27b, 0);
    w.field_bool(0x27d, true);
    w.field_bool(0x27e, false);
    w.field_bool(0x27f, false);
    w.field_bool(0x33f, true);
    w.field_bool(0x1c8b, false);
    w.field_f64(0xab5, 547.0);
    w.field_bool(0xbdc, false);
    w.end_obj();
}

fn write_note_editor_pane(
    w: &mut BtWgWriter,
    fid: u32,
    cls: u32,
    proj_group_ref: Option<u32>,
) {
    w.field_obj_header(fid, cls);
    write_editor_pane_common(w, 0x769, 0x1a62);
    w.field_bool(0x2f19, false);
    w.field_obj_header(0x191e, 0x6f3);
    w.field_null(0xb5e);
    w.field_empty_obj_list(0x25b);
    w.field_bool(0xfc2, false);
    w.end_obj();
    w.field_i8(0xf94, 0);
    w.field_bool(0x110d, false);
    w.field_bool(0x11db, true);
    w.field_bool(0x11e7, false);
    w.field_bool(0x116f, false);
    w.field_bool(0x117c, true);
    w.field_bool(0x110c, true);
    w.field_bool(0x1180, false);
    w.field_str(0x1181, "velocity");
    w.field_i8(0x1182, 0);
    w.field_bool(0x18a2, false);
    w.field_i16(0x1223, 130);
    w.field_i8(0x1980, 114);
    w.field_obj_header(0x1b1, 0xc1);
    w.field_i8(0xc1d, 255);
    w.field_f64(0xc1e, 11.0);
    w.field_i8(0x955, 0);
    w.end_obj();
    w.field_obj_header(0xc1a, 0x2a6);
    w.field_i8(0xc1d, 255);
    w.field_f64(0xc1e, 12.0);
    w.end_obj();
    w.field_obj_header(0x3985, 0x2a6);
    w.field_i8(0xc1d, 255);
    w.field_f64(0xc1e, 12.0);
    w.end_obj();
    w.field_i8(0x1ba, 100);
    w.field_i8(0x1bb, 100);
    w.field_i8(0x1bc, 100);
    w.field_i8(0x1bd, 255);
    w.field_i8(0x1be, 0);
    w.field_bool(0x15ad, true);
    w.field_i8(0x18c5, 100);
    w.field_i8(0x2712, 0);
    w.field_i8(0x272b, 0);
    w.field_i8(0x272c, 1);
    if let Some(pg) = proj_group_ref {
        w.field_ref(0x155c, pg);
        w.field_bool(0x15ac, true);
    }
    w.end_obj();
}

fn write_audio_editor_pane(w: &mut BtWgWriter, fid: u32, cls: u32, with_mode: bool) {
    w.field_obj_header(fid, cls);
    write_editor_pane_common(w, 0x766, 0x1a59);
    w.field_null(0x1225);
    w.field_null(0x1226);
    if with_mode {
        w.field_i8(0x114e, 1);
    }
    w.end_obj();
}

/// Serializa el estado de `AppState` en un binario nativo Bitwig `BtWg0003000200b8`.
pub fn encode_state_to_btwg(
    state: &AppState,
    title: &str,
    samples: &[BtWgSampleInfo],
) -> Vec<u8> {
    let bpm = state.transport.bpm.clamp(20.0, 999.0);
    let beats_per_bar = state.transport.beats_per_bar.clamp(1, 32);
    let beat_division = state.transport.beat_division.clamp(1, 32);
    // Empaquetado Bitwig de compás (para 4/4 = 0x10000024 = 268435492)
    let denom_shift = match beat_division {
        1 => 0x0400_0000,
        2 => 0x0800_0000,
        4 => 0x1000_0000,
        8 => 0x2000_0000,
        16 => 0x4000_0000,
        _ => 0x1000_0000,
    };
    let time_sig_packed: i32 = denom_shift | 0x0000_0020 | (beats_per_bar as i32);

    let master_track = state
        .live_tracks
        .iter()
        .find(|t| t.is_master)
        .cloned()
        .unwrap_or_else(|| crate::views::mixer::Track::new(0, "Master".to_string(), true));

    let mut regular_tracks: Vec<&crate::views::mixer::Track> =
        state.live_tracks.iter().filter(|t| !t.is_master).collect();
    let fallback_track = crate::views::mixer::Track::new(1, "Track 1".to_string(), false);
    if regular_tracks.is_empty() {
        regular_tracks.push(&fallback_track);
    }

    let num_scenes = state.matrix_state.scenes.len().max(5);

    // Determinar si cada pista regular es de Audio (`0x927 = 1`) o Instrumento (`0x927 = 0`)
    let track_is_audio: Vec<bool> = regular_tracks
        .iter()
        .enumerate()
        .map(|(t_idx, tr)| {
            let m_idx = tr.matrix_idx.unwrap_or(t_idx);
            let mut has_audio = false;
            let mut has_midi = false;
            if let Some(col) = state.matrix_state.grid.get(m_idx) {
                for slot in col {
                    if let Some(mclip) = &slot.clip {
                        match &mclip.content {
                            ClipData::Audio { .. } => has_audio = true,
                            ClipData::Midi { .. } => has_midi = true,
                        }
                    }
                }
            }
            has_audio && !has_midi
        })
        .collect();

    let structure_tracks: Vec<(String, bool)> = regular_tracks
        .iter()
        .zip(track_is_audio.iter())
        .map(|(t, &is_audio)| (t.name.clone(), is_audio))
        .collect();

    let mut used_files: Vec<String> = samples.iter().map(|s| s.rel_path.clone()).collect();
    used_files.sort();
    used_files.dedup();

    // ---- 1. Construir sección `meta` (cls = 4, "meta") ----
    let structure_bytes = encode_structure_subdoc(&structure_tracks);
    let mut meta_w = BtWgWriter::new();
    meta_w.u32(4);
    meta_w.raw_str("meta");
    meta_w.named_field_str("album", "");
    meta_w.named_field_str("application_version_name", "5.0");
    meta_w.named_field_str("artist", "");
    meta_w.named_field_str("branch", "releases");
    meta_w.named_field_str("comment", "Generated by Hikaru OpenLive");
    meta_w.named_field_str("copyright", "");
    meta_w.named_field_str("creator", "Hikaru OpenLive");
    meta_w.named_field_str("genre", "");
    meta_w.named_field_str("orig_artist", "");
    meta_w.named_field_str("producer", "");
    meta_w.named_field_str_arr("referenced_packaged_file_ids", &[]);
    meta_w.named_field_str("revision_id", "b120ffa9f0adb730822600479f5a9ba174605103");
    meta_w.named_field_i32("revision_no", 132920);
    meta_w.named_field_bytes("structure", &structure_bytes);
    meta_w.named_field_str("tags", "");
    meta_w.named_field_str("title", title);
    meta_w.named_field_str("type", "application/bitwig-project");
    meta_w.named_field_str_arr("used_files", &used_files);
    meta_w.named_field_str("writer", "");
    meta_w.named_field_str("year", "");
    meta_w.end_obj();
    let meta_bytes = meta_w.into_bytes();

    // Alinear `body_offset` con padding de espacios igual que Bitwig
    let body_offset = 42 + meta_bytes.len() + 16;

    // ---- 2. Construir cuerpo binario `0x2e` (Project) ----
    let mut w = BtWgWriter::new();
    let project_id = w.begin_obj(0x2e);
    w.field_null(0x1832);
    w.field_uuid(0xf, det_uuid(1, 0x2e));
    w.field_str(0x44, "template");
    w.field_str(0x11b3, "");
    w.field_str(0x45, "Hikaru OpenLive");
    w.field_str(0x11bb, "");
    w.field_str(0x11b4, "");
    w.field_str(0x11b5, "");
    w.field_str(0x11b6, "");
    w.field_str(0x11b7, "");
    w.field_str(0x11b8, "");
    w.field_str(0x11b9, "");
    w.field_str(0x11ba, "");
    w.field_str(0x46, "");
    w.field_str(0x48, "");
    w.field_bool(0x156b, false);

    // Transporte (`0xd7`)
    let transport_id = w.field_obj_header(0xbd9, 0xd7);
    w.field_null(0x1832);
    write_f64_param(&mut w, 0x211, 0x4c1, "TEMPO", bpm);

    w.field_obj_header(0x212, 0x7f4);
    write_param_common(&mut w, "");
    w.field_str(0x176, "");
    w.field_obj_header(0x2be, 0x113);
    w.field_i32(0x1ce6, time_sig_packed);
    w.end_obj();
    w.field_i32(0x1cbe, time_sig_packed);
    w.end_obj();

    // Groove (`0x6b`)
    w.field_obj_header(0x213, 0x6b);
    w.field_null(0x1832);
    write_bool_param(&mut w, 0xfc, 0x4c8, "GROOVE_ENABLED", false);
    write_enum_param(&mut w, 0xfd, 0x4cd, "SHUFFLE_RATE", 1);
    write_f64_param(&mut w, 0xfe, 0x4c5, "SHUFFLE_AMOUNT", 0.5);
    write_enum_param(&mut w, 0x100, 0x4cb, "ACCENT_RATE", 0);
    write_f64_param(&mut w, 0x101, 0x4c6, "ACCENT_AMOUNT", 1.0);
    write_f64_param(&mut w, 0x102, 0x4c7, "ACCENT_PHASE", 0.0);
    w.end_obj();

    // Tuning (`0xfb`)
    w.field_obj_header(0x214, 0xfb);
    w.field_f64(0x251, 69.0);
    w.field_f64(0x252, 440.0);
    w.field_obj_header(0x253, 0x24);
    w.end_obj();
    w.end_obj();

    w.field_null(0x215);
    w.field_f64(0xb17, 0.0);
    w.field_bool(0x835, false);
    w.field_bool(0x836, false);
    w.field_bool(0xbb3, false);
    w.field_bool(0xd40, false);
    w.field_i8(0xbaf, 0);
    w.end_obj(); // 0xd7

    // Metrónomo (`0xb3`)
    w.field_obj_header(0x191, 0xb3);
    w.field_null(0x1832);
    w.field_bool(0x1c6, false);
    w.field_f64(0x1c7, -12.0);
    w.field_bool(0x1c8, false);
    w.end_obj();

    // Pool de samples del proyecto (`0xd3c` -> `0x2c1` -> `0x4e: [0x4a6 -> 0x4a9]`)
    let mut sample_stream_ids: HashMap<PathBuf, (u32, usize)> = HashMap::new();
    if !samples.is_empty() {
        w.field_obj_header(0xd3c, 0x2c1);
        w.field_obj_list_start(0x4e);
        for (idx, sinfo) in samples.iter().enumerate() {
            let file_id = w.begin_obj(0x4a6);
            w.field_str(0x512, &sinfo.filename);
            w.field_str(0xd3a, &sinfo.rel_path);
            w.field_str(0xcd4, "");
            w.field_obj_header(0xd3b, 0x2a);
            w.field_str(0x38, "HIKARU");
            w.field_str(0x4cd, "");
            w.field_str(0x3a, "");
            w.field_str(0x3b, &sinfo.src_path.to_string_lossy());
            w.end_obj();
            w.field_obj_header(0x15, 0x2b4);
            w.field_empty_obj_list(0xc5b);
            w.end_obj();
            w.field_obj_header(0x513, 0x1e2);
            w.field_bytes(0x16, &sinfo.hash16);
            w.end_obj();
            w.field_i32(0x17, sinfo.file_size);
            w.field_obj_list_start(0x129c);
            let stream_id = w.begin_obj(0x4a9);
            w.field_str(0x129f, &sinfo.filename);
            w.field_ref(0x129e, file_id);
            w.field_str(0xfb8, "");
            w.field_i32(0x94, sinfo.sample_rate);
            w.field_f64(0x95, sinfo.duration_secs);
            w.field_i8(0x96, sinfo.channels);
            w.field_bool(0x8bd, false);
            w.field_f64(0x8be, 0.0);
            w.field_f64(0x8bf, 0.0);
            w.field_i8(0x8c0, 255);
            w.field_f64(0x8c1, 0.0);
            w.field_bool(0x299f, false);
            w.end_obj(); // 0x4a9
            w.obj_list_end();
            w.end_obj(); // 0x4a6
            sample_stream_ids.insert(sinfo.src_path.clone(), (stream_id, idx));
        }
        w.obj_list_end();
        w.end_obj(); // 0x2c1
    }

    w.field_bool(0x29a4, false);
    write_f64_param(&mut w, 0x29a5, 0xdb7, "", 1.0);
    write_volume_param(&mut w, 0x29a6, "CUE_LEVEL", 1.0);

    w.field_obj_header(0x36ef, 0x12ec);
    w.field_empty_obj_list(0x372a);
    w.field_empty_obj_list(0x36e7);
    w.field_empty_obj_list(0x3731);
    w.end_obj();

    // Grupo raíz de pistas (`0x1dd` "Project")
    let proj_group_id = w.field_obj_header(0x4dd, 0x1dd);
    w.field_null(0x1832);
    w.field_uuid(0xf, det_uuid(2, 0x1dd));
    w.field_str(0x15b, "Project");
    w.field_color(0x54c, [0.0, 0.0, 0.0, 0.0]);
    w.field_str(0x350c, "");
    w.field_obj_header(0x168, 0x1e6);
    w.field_i8(0x2603, 64);
    w.field_bool(0x13a, false);
    w.field_bool(0x1b85, true);
    w.field_bool(0xe35, false);
    w.end_obj();
    w.field_bool(0x1380, true);
    w.field_i8(0x26d5, 255);
    w.field_i8(0x26d6, 255);
    w.field_uuid(0xa51, det_uuid(3, 0x1dd));

    // Pistas regulares (`0x4de`)
    w.field_obj_list_start(0x4de);
    let mut master_id: Option<u32> = None;
    let mut first_track_id: u32 = project_id;

    for (t_idx, tr) in regular_tracks.iter().enumerate() {
        let track_id = w.begin_obj(0x90);
        if t_idx == 0 {
            first_track_id = track_id;
        }
        w.field_null(0x1832);
        w.field_uuid(0xf, det_uuid(100 + t_idx as u64, 0x90));
        w.field_str(0x15b, &tr.name);
        w.field_color(0x54c, color32_to_rgba_f32(tr.color));
        w.field_str(0x350c, "");

        w.field_obj_header(0x168, 0x88);
        w.field_i8(0x2603, 64);
        w.field_bool(0x580, false);
        w.field_i8(0x1aa7, 42);
        let lane_id = w.field_obj_header(0x4e5, 0x23c);
        w.field_obj_header(0x2a3, 0xf1);
        let vol_id = write_volume_param(&mut w, 0x41, "VOLUME", tr.volume.clamp(0.0, 1.5) as f64);
        w.field_str(0x133b, "Mixer");
        w.field_str(0x133a, "Volume");
        w.end_obj();
        w.field_i8(0xbbd, 1);
        w.end_obj();
        w.field_ref(0x4e6, lane_id);
        w.field_bool(0x4f6, false);
        w.field_i8(0x4fd, 61);
        w.field_bool(0xb32, false);
        w.field_bool(0xdfd, false);
        w.field_bool(0x115a, true);
        w.field_obj_key_ref(0x12a9, track_id, "2196");
        w.field_bool(0x1432, false);
        w.field_i8(0x16c7, 0);
        w.field_bool(0x16d0, true);
        w.field_bool(0x16ca, false);
        w.field_str(0x16cc, "velocity");
        w.field_i8(0x16cd, 0);
        w.field_bool(0x16ce, true);
        w.field_i8(0x26bd, 0);
        w.field_i8(0x271f, 0);
        w.end_obj();

        w.field_bool(0x1380, true);
        w.field_i8(0x26d5, 255);
        w.field_i8(0x26d6, 255);
        w.field_uuid(0xa51, det_uuid(200 + t_idx as u64, 0xa51));
        w.field_bool(0x162, true);
        write_track_controls(&mut w, 300 + t_idx as u64);
        w.field_bool(0x1aa0, false);
        let is_audio_track = track_is_audio[t_idx];
        w.field_i8(0x927, if is_audio_track { 1 } else { 0 });

        // Arranger lane (`0x15d`)
        w.field_obj_header(0x15d, 0xe1);
        w.field_i8(0x1fd, 0);
        w.field_str(0x236, "Untitled");
        w.field_obj_header(0x238, 0x52);
        w.field_i8(0x1fd, 0);
        w.field_empty_obj_list(0x21f);
        w.end_obj();
        w.field_empty_obj_list(0x239);
        w.end_obj();

        // Input (`0x896`)
        w.field_obj_header(0x896, 0x240);
        if is_audio_track {
            w.field_null(0x89a);
        } else {
            w.field_obj_key_ref(0x89a, project_id, "82");
        }
        w.field_null(0x1178);
        w.end_obj();

        // Audio Out (`0x897`) -> Master inline la primera vez, REF1 después
        w.field_obj_header(0x897, 0x241);
        if let Some(mid) = master_id {
            w.field_obj_key_ref(0x89b, mid, "361");
        } else {
            let mid = write_master_track_inline(
                &mut w,
                project_id,
                transport_id,
                &master_track,
                num_scenes,
            );
            master_id = Some(mid);
        }
        w.field_null(0x1177);
        w.field_i8(0x13d3, 0);
        w.end_obj();

        w.field_empty_obj_list(0x4ba);

        // Clip Launcher Bank (`0x15e`)
        let m_idx = tr.matrix_idx.unwrap_or(t_idx);
        w.field_obj_header(0x15e, 0x1e);
        w.field_obj_list_start(0x231);
        for s_idx in 0..num_scenes {
            w.begin_obj(0xe3);
            let maybe_clip = state
                .matrix_state
                .grid
                .get(m_idx)
                .and_then(|col| col.get(s_idx))
                .and_then(|slot| slot.clip.as_ref());
            if let Some(mclip) = maybe_clip {
                let audio_lookup = match &mclip.content {
                    ClipData::Audio { events, .. } => {
                        let p = if !mclip.path.as_os_str().is_empty() {
                            Some(mclip.path.clone())
                        } else {
                            events.iter().find_map(|e| e.source_path.clone())
                        };
                        p.and_then(|src| sample_stream_ids.get(&src).copied())
                    }
                    ClipData::Midi { .. } => None,
                };
                if let Some((stream_id, s_idx_info)) = audio_lookup {
                    write_audio_launcher_clip(
                        &mut w,
                        mclip,
                        bpm,
                        time_sig_packed,
                        &samples[s_idx_info],
                        stream_id,
                    );
                } else {
                    write_launcher_clip(&mut w, mclip, bpm, None);
                }
            } else {
                w.field_null(0x240);
            }
            w.field_bool(0xbcd, true);
            w.end_obj();
        }
        w.obj_list_end();
        w.end_obj();

        w.field_null(0x15f);

        // Device Chain vacía (`0x164`)
        w.field_obj_header(0x164, 0x8a);
        w.field_null(0x1832);
        w.field_obj_header(0x331, 0x27);
        w.field_bool(0xb3f, false);
        w.end_obj();
        w.field_empty_obj_list(0x144);
        w.end_obj();

        // Mixer Channel (`0x165`)
        w.field_obj_header(0x165, 0xab);
        w.field_null(0x1832);
        w.field_ref(0x1a4, vol_id);
        write_pan_param(&mut w, 0x1a5, tr.pan.clamp(-1.0, 1.0) as f64);
        write_bool_param(&mut w, 0x1a6, 0x4ce, "MUTE", tr.mute);
        w.field_bool(0x1a8, tr.solo);
        w.field_empty_obj_list(0xa5);
        write_enum_param(&mut w, 0x1456, 0x57d, "CROSSFADE_MODE", 1);
        w.end_obj();

        w.field_bool(0xf2, false);
        w.field_i8(0xaa7, 1);
        w.field_obj_header(0xbfd, 0x2a5);
        w.field_null(0x1832);
        w.field_empty_obj_list(0xbfc);
        w.end_obj();
        w.field_bool(0xbfe, true);
        w.field_i8(0x12c3, 2);
        w.field_i8(0x12c4, 254);
        w.field_bool(0xacc, false);
        w.field_null(0x190);
        w.field_f64(0x1374, 7.0);
        w.field_f64(0x1375, -1.0);
        w.field_obj_header(0x1c15, 0x22e);
        w.field_null(0x1832);
        w.field_i8(0x25f, 0);
        w.field_str(0x176, "");
        w.field_bool(0x2afa, true);
        w.field_f32(0xa9, 100.0);
        w.end_obj();

        w.end_obj(); // Track 0x90
    }
    w.obj_list_end(); // 0x4de

    w.field_empty_obj_list(0x4df);
    if let Some(mid) = master_id {
        w.field_ref(0x4e0, mid);
    }

    // Scene Bank (`0x4e2`)
    w.field_obj_header(0x4e2, 0x10d);
    w.field_obj_list_start(0x293);
    for s_idx in 0..num_scenes {
        let sname = state
            .matrix_state
            .scenes
            .get(s_idx)
            .map(|s| s.name.as_str())
            .unwrap_or("");
        w.begin_obj(0x6d);
        w.field_str(0x109, sname);
        w.field_color(0x25ec, [0.0, 0.0, 0.0, 0.0]);
        w.field_str(0x3508, "");
        w.field_str(0xb8c, "");
        w.field_i8(0x10b, 90);
        w.field_obj_header(0x2665, 0xd09);
        w.field_i8(0x2664, 85);
        w.end_obj();
        w.field_bool(0x3708, false);
        w.field_i8(0x36fd, 0);
        w.field_i8(0x36fe, 0);
        w.field_i8(0x3709, 255);
        w.field_i8(0x370a, 255);
        w.field_i8(0x3712, 255);
        w.field_i8(0x3713, 255);
        w.end_obj();
    }
    w.obj_list_end();
    w.end_obj(); // 0x10d

    // Arranger meta (`0x58`)
    w.field_obj_header(0x58, 0x79);
    w.field_i8(0x1fd, 0);
    w.field_obj_header(0x98f, 0x26a);
    w.field_i8(0x1fd, 0);
    w.field_empty_obj_list(0x21f);
    w.field_bool(0x991, true);
    w.field_obj_header(0x992, 0x58);
    w.field_f64(0x2af, 0.0);
    w.field_f64(0x26, 16.0);
    w.end_obj();
    w.end_obj();
    w.field_obj_header(0x1607, 0x60a);
    w.field_i8(0x1fd, 0);
    w.field_empty_obj_list(0x21f);
    w.end_obj();
    w.end_obj(); // 0x79

    w.end_obj(); // 0x1dd Project Group

    w.field_empty_obj_list(0x987);
    w.field_empty_obj_list(0xabf);
    w.field_obj_header(0x60, 0x95);
    w.field_empty_obj_list(0x17e);
    w.end_obj();
    w.field_empty_obj_list(0xabb);

    // Editor state (`0x4d`)
    w.field_obj_header(0x4d, 0xe4);
    w.field_obj_header(0xee6, 0x325);
    w.field_obj_header(0xb5e, 0x29e);
    w.field_ref(0xb5d, first_track_id);
    w.end_obj();
    w.field_obj_list_start(0x25b);
    w.u32(1);
    w.u32(first_track_id);
    w.obj_list_end();
    w.field_bool(0xfc2, false);
    w.end_obj();

    w.field_obj_header(0x138, 0x1b5);
    w.field_null(0xb5e);
    w.field_empty_obj_list(0x25b);
    w.field_bool(0xfc2, true);
    w.end_obj();

    w.field_bool(0x1232, false);
    w.field_bool(0x271c, false);
    write_arranger_editor_pane(&mut w);
    write_note_editor_pane(&mut w, 0xf02, 0x336, Some(proj_group_id));
    write_audio_editor_pane(&mut w, 0xf03, 0x337, false);
    write_note_editor_pane(&mut w, 0xf04, 0x352, None);
    write_audio_editor_pane(&mut w, 0xf05, 0x464, true);
    write_note_editor_pane(&mut w, 0xf06, 0x351, None);
    write_audio_editor_pane(&mut w, 0xf07, 0x465, true);
    w.field_obj_header(0x367e, 0x12b5);
    w.field_null(0xb5e);
    w.field_empty_obj_list(0x25b);
    w.field_bool(0xfc2, false);
    w.end_obj();

    // Propiedades restantes de `float_document_display_settings` (`0xe4`)
    w.field_i8(0x57c, 1);
    w.field_ref(0x515, proj_group_id);
    w.field_obj_header(0x795, 0x21d);
    w.field_bool(0x10c2, false);
    w.field_i8(0x78c, 16);
    w.field_i8(0x10bd, 1);
    w.field_f64(0x78f, 1.0);
    w.field_f64(0x790, 0.0);
    w.field_f64(0x792, 0.0);
    w.field_f64(0x10bf, 0.38);
    w.field_i8(0x798, 0);
    w.field_bool(0x10c3, false);
    w.end_obj();
    w.field_obj_header(0x1245, 0x46e);
    w.field_bool(0x10c2, false);
    w.field_i8(0x78c, 32);
    w.field_i8(0x10bd, 1);
    w.end_obj();
    w.field_obj_header(0x2d36, 0xf59);
    w.field_f64(0x2d37, 1.0);
    w.field_bool(0x2d38, false);
    w.field_f64(0x2d39, 0.0);
    w.end_obj();
    w.field_bool(0x6cb, true);
    w.field_bool(0x13f2, true);
    w.field_obj_header(0x1127, 0x384);
    w.field_i8(0x12a2, 0);
    w.field_i8(0x1126, 6);
    w.field_obj_header(0x12a8, 0x4aa);
    w.field_i8(0x12a4, 1);
    w.field_bool(0x1bb9, false);
    w.field_bool(0x2d35, false);
    w.field_obj_header(0x2e21, 0xfad);
    w.field_bool(0x2e22, true);
    w.field_i8(0x2e23, 2);
    w.field_bool(0x301e, true);
    w.field_i8(0x2e25, 0);
    w.field_i8(0x2e26, 1);
    w.field_i8(0x2e27, 1);
    w.field_i8(0x2e28, 1);
    w.field_i8(0x2e2c, 1);
    w.field_bool(0x2e2f, false);
    w.field_bool(0x2e30, false);
    w.field_bool(0x2e31, false);
    w.field_bool(0x2e32, false);
    w.field_bool(0x2e35, false);
    w.end_obj();
    w.end_obj();
    w.end_obj();
    w.field_bool(0x18fc, true);
    w.field_bool(0x12d2, true);
    w.field_i8(0x12eb, 120);
    w.field_obj_header(0x1436, 0x56c);
    w.field_f64(0x1434, 0.0);
    w.field_f64(0x1435, 0.0);
    w.end_obj();
    w.field_i16(0x178e, 4043);
    w.field_bool(0x281c, true);
    w.field_i8(0x2d66, 1);
    w.field_obj_header(0x2eea, 0x101c);
    w.field_obj_header(0x2eee, 0x101e);
    w.field_i8(0x2ee8, 2);
    w.field_bool(0x2ee9, false);
    w.field_bool(0x2f08, false);
    w.end_obj();
    w.end_obj();
    w.field_str(0x301a, "Classic");
    w.field_bool(0x3536, false);
    w.field_bool(0x3546, false);
    w.field_bool(0x3548, true);
    w.field_bool(0x3549, true);
    w.field_bool(0x38ca, true);
    w.field_bool(0x3854, true);
    w.field_i8(0x38c8, 8);
    w.end_obj(); // 0xe4

    // Banderas finales del proyecto
    w.field_bool(0xaa1, false);
    w.field_bool(0x134d, true);
    w.field_i8(0xbdb, 5);
    w.field_i8(0x3855, 1);
    w.field_i8(0x2845, 10);
    w.field_i8(0x3856, 13);
    w.field_i8(0x3857, 0);
    w.field_i8(0x3858, 2);
    w.field_f64(0x180f, 4.0);
    w.field_i8(0x1810, 0);
    w.field_obj_header(0x1646, 0x63a);
    w.field_empty_obj_list(0x166b);
    w.field_obj_header(0x1715, 0x66c);
    w.field_bool(0x1714, true);
    w.end_obj();
    w.end_obj();
    w.field_i8(0x172d, 1);
    w.field_bool(0x38cb, true);
    w.end_obj(); // 0x2e Project

    let body_bytes = w.into_bytes();

    // ---- 3. Ensamblar archivo final: Header (42) + meta + espacios + '\n' + body ----
    let header = format!("BtWg0003000200b8{:08x}000000000000000000", body_offset);
    debug_assert_eq!(header.len(), 42);

    let mut out = Vec::with_capacity(body_offset + body_bytes.len());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(&meta_bytes);
    while out.len() + 1 < body_offset {
        out.push(b' ');
    }
    out.push(b'\n');
    out.extend_from_slice(&body_bytes);
    out
}
