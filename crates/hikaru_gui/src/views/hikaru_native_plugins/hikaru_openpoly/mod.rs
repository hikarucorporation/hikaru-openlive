/*
 * Hikaru OpenPoly - Hybrid Modular Polyphonic Synthesizer (GNU AGPL v3)
 * Copyright (C) 2026 Hikaru Corporation
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 */

//! **Hikaru OpenPoly**: sintetizador polifónico híbrido modular libre (AGPL-v3),
//! reemplazo de `Hikaru OpenWavetable` y equivalente funcional de Bitwig Polymer.
//!
//! - [`open_poly`]: modelo de estado, carga de wavetables `.wav`, conversión a
//!   [`hikaru_dsp::synth::open_poly::OpenPolyDspParams`], presets de fábrica y
//!   persistencia `.oplf` / `.opsf` / `.hdp`.
//! - [`open_poly_ui`]: interfaz `egui` + `bitwig_kit` estilo rack modular.

pub mod open_poly;
pub mod open_poly_ui;

pub use open_poly::{
    load_openpoly_preset, load_wavetable_from_path, save_openpoly_preset, HikaruOpenPoly,
    OpenPolyFactoryPreset, FACTORY_PRESETS,
};
pub use open_poly_ui::{
    render_module, OPEN_POLY_CARD_WIDTH, OPEN_POLY_COMPACT_CARD_WIDTH,
};
