/*
 * Hikaru OpenStudio - Audio DAW
 * Copyright (C) 2026 Hikaru OpenStudio Developers
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 */

//! Lectura de WAV tolerante a archivos del mundo real.
//!
//! `hound` 3.5 no salta el byte de relleno (pad) que la spec RIFF exige
//! después de un chunk de tamaño impar. Un `.wav` exportado por iZotope,
//! Adobe Audition o cualquier herramienta que escriba un chunk `bext` de
//! 603 bytes queda ilegible: al leer el chunk siguiente el lector queda
//! desfasado un byte y termina con `Failed to read enough bytes`.
//!
//! [`open`] normaliza el archivo en memoria antes de entregárselo a
//! `hound`: se queda sólo con los chunks `fmt ` y `data`, agrega el pad
//! donde corresponde y corrige el tamaño declarado. Así todos los sitios
//! que abren un WAV aceptan los mismos archivos.

use std::io::Cursor;
use std::path::Path;

/// `WavReader` sobre un buffer normalizado en memoria.
pub type Reader = hound::WavReader<Cursor<Vec<u8>>>;

/// Abre un WAV de disco normalizando su estructura RIFF.
pub fn open<P: AsRef<Path>>(path: P) -> Result<Reader, String> {
    let raw = std::fs::read(path.as_ref()).map_err(|error| error.to_string())?;
    from_bytes(&raw)
}

/// Abre un WAV cuyos bytes ya están en memoria (por ejemplo, un recurso
/// embebido) aplicando la misma normalización que [`open`].
pub fn from_bytes(raw: &[u8]) -> Result<Reader, String> {
    let normalized = normalize_bytes(raw)?;
    hound::WavReader::new(Cursor::new(normalized)).map_err(|error| error.to_string())
}

/// Escala de full-scale para muestras PCM enteras de `bits_per_sample` bits.
///
/// Usa `u64` para no desbordar `i32` con archivos de 32 bits, donde
/// `1 << 31` es negativo (o entra en pánico en builds de debug).
pub fn int_scale(bits_per_sample: u16) -> f32 {
    let shift = bits_per_sample.saturating_sub(1).min(63);
    (1u64 << shift) as f32
}

/// Decodifica todas las muestras de un WAV a `f32` intercalado.
///
/// Devuelve `(muestras, canales, sample_rate)`.
pub fn decode_all(reader: Reader) -> Result<(Vec<f32>, usize, u32), String> {
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let scale = int_scale(spec.bits_per_sample);
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => {
            reader.into_samples::<f32>().filter_map(Result::ok).collect()
        }
        hound::SampleFormat::Int => reader
            .into_samples::<i32>()
            .filter_map(Result::ok)
            .map(|sample| sample as f32 / scale)
            .collect(),
    };
    Ok((samples, channels, spec.sample_rate))
}

/// Abre y decodifica un WAV de disco.
pub fn read_file<P: AsRef<Path>>(path: P) -> Result<(Vec<f32>, usize, u32), String> {
    decode_all(open(path)?)
}

/// Reconstruye un WAV canónico (`RIFF` + `fmt ` + `data`) a partir de otro.
///
/// Descarta chunks de metadatos (`bext`, `LIST`, `id3 `, …), respeta el pad
/// de los chunks impares, clampa un `data` truncado a frames completos y
/// deja el tamaño de `RIFF` consistente.
pub fn normalize_bytes(raw: &[u8]) -> Result<Vec<u8>, String> {
    if raw.len() < 12 || &raw[0..4] != b"RIFF" || &raw[8..12] != b"WAVE" {
        return Err("no es un WAV RIFF/WAVE".to_string());
    }

    let mut fmt_chunk: Option<&[u8]> = None;
    let mut data_chunk: Option<&[u8]> = None;
    let mut block_align = 1usize;

    let mut offset = 12usize;
    while offset + 8 <= raw.len() {
        let id = &raw[offset..offset + 4];
        let declared_len = u32::from_le_bytes([
            raw[offset + 4],
            raw[offset + 5],
            raw[offset + 6],
            raw[offset + 7],
        ]) as usize;
        let content_start = offset + 8;
        // Un chunk que declara más bytes de los que hay es un archivo
        // truncado: tomamos lo disponible en lugar de fallar.
        let content_end = content_start.saturating_add(declared_len).min(raw.len());

        match id {
            b"fmt " if fmt_chunk.is_none() => {
                let body = &raw[content_start..content_end];
                if body.len() >= 14 {
                    block_align =
                        u16::from_le_bytes([body[12], body[13]]).max(1) as usize;
                }
                fmt_chunk = Some(body);
            }
            b"data" if data_chunk.is_none() => {
                data_chunk = Some(&raw[content_start..content_end]);
            }
            _ => {}
        }

        let declared_end = content_start.saturating_add(declared_len);
        if declared_end > raw.len() {
            break;
        }
        offset = next_chunk_offset(raw, declared_end, declared_len & 1 == 1);
    }

    let fmt_chunk = fmt_chunk.ok_or_else(|| "WAV sin chunk fmt ".to_string())?;
    let data_chunk = data_chunk.ok_or_else(|| "WAV sin chunk data".to_string())?;
    if fmt_chunk.len() < 16 {
        return Err("chunk fmt demasiado corto".to_string());
    }

    // hound exige que `data` contenga frames completos.
    let usable = data_chunk.len() - (data_chunk.len() % block_align);

    let mut out = Vec::with_capacity(12 + 8 + fmt_chunk.len() + 8 + usable + 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&[0, 0, 0, 0]); // tamaño, se completa al final
    out.extend_from_slice(b"WAVE");
    push_chunk(&mut out, b"fmt ", fmt_chunk);
    push_chunk(&mut out, b"data", &data_chunk[..usable]);

    let riff_size = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&riff_size.to_le_bytes());
    Ok(out)
}

/// Devuelve el offset del próximo chunk a partir del final del contenido.
///
/// La spec RIFF exige un byte de relleno tras chunks impares, pero hay
/// archivos que lo omiten. Detectamos el pad por el byte nulo: si justo en
/// `end` no hay un id imprimible y en `end + 1` sí lo hay, saltamos el pad.
fn next_chunk_offset(raw: &[u8], end: usize, odd: bool) -> usize {
    fn looks_like_chunk_id(raw: &[u8], at: usize) -> bool {
        at + 4 <= raw.len()
            && raw[at..at + 4]
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || *b == b' ' || *b == b'_' || *b == b'-')
    }

    if end >= raw.len() || !odd {
        return end;
    }
    if !looks_like_chunk_id(raw, end) && looks_like_chunk_id(raw, end + 1) {
        end + 1
    } else {
        end
    }
}

fn push_chunk(out: &mut Vec<u8>, id: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(id);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    if body.len() & 1 == 1 {
        out.push(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Construye un WAV PCM 16-bit mono con un chunk `bext` de tamaño impar
    /// antes del `data`, tal como lo exportan iZotope/Adobe.
    fn wav_with_odd_bext(pad: bool) -> Vec<u8> {
        let samples: [i16; 4] = [0, 1000, -1000, 0];
        let mut data = Vec::new();
        for s in samples {
            data.extend_from_slice(&s.to_le_bytes());
        }

        let bext = vec![0u8; 603]; // impar a propósito

        let mut body = Vec::new();
        body.extend_from_slice(b"WAVE");

        body.extend_from_slice(b"fmt ");
        body.extend_from_slice(&16u32.to_le_bytes());
        body.extend_from_slice(&1u16.to_le_bytes()); // PCM
        body.extend_from_slice(&1u16.to_le_bytes()); // mono
        body.extend_from_slice(&44100u32.to_le_bytes());
        body.extend_from_slice(&88200u32.to_le_bytes());
        body.extend_from_slice(&2u16.to_le_bytes()); // block align
        body.extend_from_slice(&16u16.to_le_bytes());

        body.extend_from_slice(b"bext");
        body.extend_from_slice(&(bext.len() as u32).to_le_bytes());
        body.extend_from_slice(&bext);
        if pad {
            body.push(0);
        }

        body.extend_from_slice(b"data");
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.extend_from_slice(&data);

        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn normaliza_bext_impar_sin_pad() {
        // Variante que omite el byte de relleno (fuera de spec, pero existe).
        let raw = wav_with_odd_bext(false);

        let reader = from_bytes(&raw).expect("el WAV debía abrirse tras normalizar");
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.len(), 4);
    }

    #[test]
    fn normaliza_bext_impar_con_pad() {
        let raw = wav_with_odd_bext(true);
        let reader = from_bytes(&raw).expect("el WAV debía abrirse");
        assert_eq!(reader.len(), 4);
    }

    #[test]
    fn int_scale_no_desborda_a_32_bits() {
        assert_eq!(int_scale(8), 128.0);
        assert_eq!(int_scale(16), 32768.0);
        assert_eq!(int_scale(24), 8_388_608.0);
        assert!(int_scale(32).is_sign_positive());
        assert_eq!(int_scale(32), 2_147_483_648.0);
    }

    #[test]
    fn rechaza_no_riff() {
        assert!(normalize_bytes(b"no soy un wav").is_err());
    }
}
