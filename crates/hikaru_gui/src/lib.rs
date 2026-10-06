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
/// Formato de proyecto `.hikaru`: qué se guarda, cómo y desde dónde.
pub mod project;

pub use app::HikaruApp;

pub mod audio_proxy;
pub mod render;

pub use render::HikaruRenderer;
