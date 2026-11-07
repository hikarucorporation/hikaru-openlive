/*
 * Hikaru OpenStudio - Audio DAW
 * Copyright (C) 2026 Hikaru OpenStudio Developers
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 *
 * Adapted from Lemon-Studio `crates/os-app/src/meters.rs` (GPL-3.0-or-later,
 * compatible with AGPL-3.0-or-later under GPLv3 §13).
 */

//! Ley de escala dB y balística de vúmetros estéreo (rise, fall, peak-hold),
//! adaptada de `Lemon-Studio` y calibrada para las marcas `0 / -6 / -12 / -18 / -24 / -36 / -∞`
//! del Mix Panel de Bitwig Studio 5.1.7.

/// Piso por debajo del cual el vúmetro se considera en silencio absoluto.
pub const SILENT_DB: f32 = -200.0;

/// Piso visual del medidor Bitwig (`-60 dB`).
pub const FLOOR_DB: f32 = -60.0;

/// Curva de referencia `(dB, posición_y)` medida de `Lemon-Studio` para dar
/// mayor resolución visual en la zona crítica `0..-18 dB`.
const LAW: [(f32, f32); 17] = [
    (3.0, 181.0),
    (0.0, 241.0),
    (-3.0, 301.0),
    (-6.0, 360.0),
    (-9.0, 419.0),
    (-12.0, 479.0),
    (-15.0, 535.0),
    (-18.0, 588.0),
    (-21.0, 639.0),
    (-24.0, 686.0),
    (-27.0, 728.0),
    (-30.0, 766.0),
    (-33.0, 799.0),
    (-36.0, 826.0),
    (-39.0, 848.0),
    (-42.0, 866.0),
    (-45.0, 880.0),
];
const LAW_TOP: f32 = 165.0;
const LAW_BOTTOM: f32 = 924.0;

/// Convierte amplitud lineal (`0.0 .. 2.0`) a decibeles.
#[inline]
pub fn linear_to_db(amp: f32) -> f32 {
    if amp.is_finite() && amp > 1.0e-7 {
        20.0 * amp.log10()
    } else {
        SILENT_DB
    }
}

/// Convierte un nivel en `dB` a fracción de altura `[0.0 .. 1.0]` (`0 = -∞`, `1 = +3.8 dB`).
pub fn db_to_frac(db: f32) -> f32 {
    if !db.is_finite() || db <= FLOOR_DB {
        return 0.0;
    }
    let y = if db >= LAW[0].0 {
        (LAW[0].1 - (db - LAW[0].0) * 20.0).max(LAW_TOP)
    } else {
        let mut found = None;
        for w in LAW.windows(2) {
            let ((d0, y0), (d1, y1)) = (w[0], w[1]);
            if db <= d0 && db >= d1 {
                found = Some(y0 + (y1 - y0) * (d0 - db) / (d0 - d1));
                break;
            }
        }
        found.unwrap_or_else(|| {
            let (d_last, y_last) = LAW[LAW.len() - 1];
            y_last + (LAW_BOTTOM - y_last) * (d_last - db) / (d_last - FLOOR_DB).max(1.0e-3)
        })
    };
    ((LAW_BOTTOM - y) / (LAW_BOTTOM - LAW_TOP)).clamp(0.0, 1.0)
}

/// Convierte una fracción de fader `[0.0 .. 1.0]` a `dB` (`-60.0 .. +6.0 dB`) y viceversa
/// con ley perceptual suave donde `0.0 dB` cae cerca del `78%` superior como en Bitwig.
#[inline]
pub fn db_to_fader_norm(volume_db: f32) -> f32 {
    if !volume_db.is_finite() || volume_db <= -59.9 {
        return 0.0;
    }
    let clamped = volume_db.clamp(-60.0, 6.0);
    if clamped >= 0.0 {
        0.78 + 0.22 * (clamped / 6.0)
    } else {
        // Curva cuadrática suave para dar tacto fino entre 0 dB y -24 dB
        let t = (clamped + 60.0) / 60.0;
        0.78 * t.powf(1.45)
    }
}

/// Inversa exacta de [`db_to_fader_norm`]: mapea `[0.0 .. 1.0]` a `[-60.0 .. +6.0] dB`.
#[inline]
pub fn fader_norm_to_db(norm: f32) -> f32 {
    let n = if norm.is_finite() { norm.clamp(0.0, 1.0) } else { 0.0 };
    if n <= 0.001 {
        -60.0
    } else if n >= 0.78 {
        ((n - 0.78) / 0.22) * 6.0
    } else {
        let t = (n / 0.78).powf(1.0 / 1.45);
        t * 60.0 - 60.0
    }
}

/// Estado balístico estéreo (nivel actual + línea de Peak-Hold) para una pista.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StereoBallistics {
    pub db: [f32; 2],
    pub hold_db: [f32; 2],
    pub hold_elapsed: [f32; 2],
}

impl Default for StereoBallistics {
    fn default() -> Self {
        Self {
            db: [SILENT_DB; 2],
            hold_db: [SILENT_DB; 2],
            hold_elapsed: [0.0; 2],
        }
    }
}

impl StereoBallistics {
    /// Avanza la balística un paso `dt` (segundos) con picos lineales `[L, R]`.
    pub fn step(&mut self, peaks_linear: [f32; 2], dt: f32) {
        let dt = dt.clamp(0.001, 0.25);
        const FALL_DB_PER_SEC: f32 = 42.0;
        const HOLD_TIME_SECS: f32 = 1.35;
        const HOLD_FALL_DB_PER_SEC: f32 = 22.0;

        for ch in 0..2 {
            let target_db = linear_to_db(peaks_linear[ch]);
            if target_db >= self.db[ch] {
                self.db[ch] = target_db;
            } else {
                self.db[ch] = (self.db[ch] - FALL_DB_PER_SEC * dt).max(target_db);
            }
            if self.db[ch] < FLOOR_DB {
                self.db[ch] = SILENT_DB;
            }

            if self.db[ch] >= self.hold_db[ch] {
                self.hold_db[ch] = self.db[ch];
                self.hold_elapsed[ch] = 0.0;
            } else {
                self.hold_elapsed[ch] += dt;
                if self.hold_elapsed[ch] > HOLD_TIME_SECS {
                    self.hold_db[ch] =
                        (self.hold_db[ch] - HOLD_FALL_DB_PER_SEC * dt).max(self.db[ch]);
                }
            }
            if self.hold_db[ch] < FLOOR_DB {
                self.hold_db[ch] = SILENT_DB;
            }
        }
    }

    /// Fracciones normalizadas `[level_l, level_r, hold_l, hold_r]` listas para el shader WGSL.
    #[inline]
    pub fn normalized_quad(&self) -> [f32; 4] {
        [
            db_to_frac(self.db[0]),
            db_to_frac(self.db[1]),
            db_to_frac(self.hold_db[0]),
            db_to_frac(self.hold_db[1]),
        ]
    }
}
