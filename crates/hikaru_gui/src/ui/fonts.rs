/*
 * Hikaru OpenStudio - Audio DAW
 * Copyright (C) 2026 Hikaru OpenStudio Developers
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 */

//! Tipografía técnica condensada estilo Bitwig Studio 5.1.7.
//!
//! Incorpora la familia **Barlow Semi Condensed** (SIL Open Font License 1.1,
//! adaptada del kit `os-ui` de Lemon-Studio) como fuente proporcional primaria
//! y expone una familia `"semibold"` para cabeceras de pista, cápsulas de
//! transporte (`FILE`, `PLAY`, `ADD`, `EDIT`, `TRACK`) y selectores de vista.

use egui::{Context, FontData, FontDefinitions, FontFamily, FontId, Id};

const BARLOW_REGULAR: &[u8] =
    include_bytes!("../../assets/fonts/Barlow/BarlowSemiCondensed-Regular.ttf");
const BARLOW_SEMIBOLD: &[u8] =
    include_bytes!("../../assets/fonts/Barlow/BarlowSemiCondensed-SemiBold.ttf");
const BARLOW_CONDENSED_MEDIUM: &[u8] =
    include_bytes!("../../assets/fonts/Barlow/BarlowCondensed-Medium.ttf");

const FONTS_INSTALLED_KEY: &str = "hikaru_bitwig_fonts_installed_v1";

/// Instala las fuentes condensadas de Bitwig/Lemon-Studio una única vez por
/// [`egui::Context`]. Es seguro invocarla en cada frame (hace short-circuit
/// con `ctx.data`).
pub fn ensure_installed(ctx: &Context) {
    let key = Id::new(FONTS_INSTALLED_KEY);
    let already_installed = ctx.data(|d| d.get_temp::<bool>(key).unwrap_or(false));
    if already_installed {
        return;
    }

    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        "barlow_regular".to_owned(),
        FontData::from_static(BARLOW_REGULAR).into(),
    );
    fonts.font_data.insert(
        "barlow_semibold".to_owned(),
        FontData::from_static(BARLOW_SEMIBOLD).into(),
    );
    fonts.font_data.insert(
        "barlow_lcd".to_owned(),
        FontData::from_static(BARLOW_CONDENSED_MEDIUM).into(),
    );

    if let Some(prop) = fonts.families.get_mut(&FontFamily::Proportional) {
        prop.insert(0, "barlow_regular".to_owned());
    }

    // Familia "semibold" para botones biselados, pestañas ARRANGE/MIX/EDIT y cabeceras.
    let mut semibold_chain = vec!["barlow_semibold".to_owned(), "barlow_regular".to_owned()];
    if let Some(prop) = fonts.families.get(&FontFamily::Proportional) {
        for fallback in prop.iter().skip(1) {
            semibold_chain.push(fallback.clone());
        }
    }
    fonts
        .families
        .insert(FontFamily::Name("semibold".into()), semibold_chain);

    // Familia "lcd" condensada de alta legibilidad para lecturas digitales (110.00 / 1.1.1.00).
    let mut lcd_chain = vec!["barlow_lcd".to_owned(), "barlow_semibold".to_owned()];
    if let Some(mono) = fonts.families.get(&FontFamily::Monospace) {
        for fallback in mono {
            lcd_chain.push(fallback.clone());
        }
    }
    fonts
        .families
        .insert(FontFamily::Name("lcd".into()), lcd_chain);

    ctx.set_fonts(fonts);
    ctx.data_mut(|d| d.insert_temp(key, true));
}

/// [`FontId`] con peso SemiBold condensado (estilo cápsulas y títulos Bitwig).
#[inline]
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

/// [`FontId`] regular condensado para etiquetas densas de DAW.
#[inline]
pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

/// [`FontId`] numérico condensado estilo pantalla LCD de transporte Bitwig.
#[inline]
pub fn lcd(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("lcd".into()))
}
