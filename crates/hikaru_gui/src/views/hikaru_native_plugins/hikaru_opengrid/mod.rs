/*
 * Hikaru OpenGrid - Polyphonic Modular Patch Graph Synthesizer (GNU AGPL v3)
 * Copyright (C) 2026 Hikaru Corporation
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 */

//! **Hikaru OpenGrid**: entorno de síntesis modular polifónica por nodos y cables
//! libre bajo GNU AGPL-v3, alternativa a Bitwig Poly Grid / The Grid.
//!
//! - [`open_grid`]: estado del grafo de patch, cámara (pan/zoom), presets de
//!   fábrica y persistencia `.oplf` / `.opsf` / `.hdp`.
//! - [`open_grid_ui`]: tarjeta compacta con minimapa interactivo en el DSP Rack
//!   y editor modular completo en el panel central.

pub mod open_grid;
pub mod open_grid_ui;

pub use open_grid::{
    load_opengrid_preset, save_opengrid_preset, HikaruOpenGrid, OpenGridFactoryPreset,
    GRID_FACTORY_PRESETS,
};
pub use open_grid_ui::{
    render_editor_panel, render_preset_menu, render_rack_card, OPEN_GRID_CARD_WIDTH,
    OPEN_GRID_COMPACT_CARD_WIDTH,
};
