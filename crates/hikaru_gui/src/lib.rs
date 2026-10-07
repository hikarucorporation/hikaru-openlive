/*
 * Hikaru OpenStudio - Audio DAW
 * Copyright (C) 2026 Hikaru OpenStudio Developers
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 */

pub mod app;
pub mod views;
pub mod theme;
pub mod ui;
/// Formatos de proyecto `.oplf` (OpenLive) y `.opsf` (OpenStudio): qué se
/// guarda, cómo y desde dónde.
pub mod project;
/// Versión del producto, única fuente de verdad.
pub mod version;

pub use app::HikaruApp;

pub mod audio_proxy;
pub mod render;

pub use render::HikaruRenderer;
