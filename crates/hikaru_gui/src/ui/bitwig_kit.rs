/*
 * Hikaru OpenStudio - Audio DAW
 * Copyright (C) 2026 Hikaru OpenStudio Developers
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 */

//! Kit de widgets táctiles estilo **Bitwig Studio 5.1.7** respaldados por `wgpu` SDF.
//!
//! Inspirado en la arquitectura `kit` de `Lemon-Studio`, pero moldeado visualmente
//! sobre la geometría, colores y leyes de interacción de Bitwig Studio 5.1.7:
//! - Grupos de cápsulas biseladas unidas (`[PLAY | ▶ | ■ | ● | Auto]`, `[● | S | M]`)
//! - Perillas rotativas de `270°` con arrastre vertical fino (`Shift` = precisión, doble clic = reset)
//! - Faders cobre-ámbar verticales y horizontales sin el `egui::Slider` genérico
//! - Barras de paneo bipolares y vúmetros estéreo duales con Peak-Hold.

use std::ops::RangeInclusive;

use egui::{Align2, Color32, CursorIcon, Painter, Rect, Response, Sense, Ui, Vec2};

use crate::ui::bitwig_wgpu::{
    paint_capsule, paint_fader_horizontal, paint_fader_vertical, paint_rotary_knob,
    paint_stereo_meter, FLAG_ACTIVE, FLAG_CORNER_NOTCH, FLAG_HOVERED, FLAG_JOIN_LEFT,
    FLAG_JOIN_RIGHT,
};
use crate::ui::fonts;
use crate::ui::meters::{db_to_fader_norm, fader_norm_to_db};

// Colores calibrados directamente contra los screenshots de referencia de Bitwig 5.1.7
pub const BW_CAPSULE_IDLE: Color32 = Color32::from_rgb(80, 82, 88);       // #505258
pub const BW_CAPSULE_DARK: Color32 = Color32::from_rgb(56, 58, 63);       // #383A3F
pub const BW_KNOB_CAP: Color32 = Color32::from_rgb(92, 95, 102);          // #5C5F66
pub const BW_FADER_AMBER: Color32 = Color32::from_rgb(138, 78, 24);       // #8A4E18
pub const BW_FADER_ORANGE: Color32 = Color32::from_rgb(255, 124, 16);     // #FF7C10
pub const BW_ACCENT_ORANGE: Color32 = Color32::from_rgb(255, 114, 0);     // #FF7200
pub const BW_LCD_CYAN: Color32 = Color32::from_rgb(68, 184, 255);         // #44B8FF

/// Posición de un segmento dentro de una tira de cápsulas unidas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinSide {
    /// Cápsula independiente con sus 4 esquinas redondeadas.
    Standalone,
    /// Primer segmento izquierdo (redondeado a la izquierda, plano a la derecha).
    Left,
    /// Segmento central (plano en ambos lados).
    Middle,
    /// Último segmento derecho (plano a la izquierda, redondeado a la derecha).
    Right,
}

impl JoinSide {
    #[inline]
    pub fn to_flags(self) -> u32 {
        match self {
            JoinSide::Standalone => 0,
            JoinSide::Left => FLAG_JOIN_RIGHT,
            JoinSide::Middle => FLAG_JOIN_LEFT | FLAG_JOIN_RIGHT,
            JoinSide::Right => FLAG_JOIN_LEFT,
        }
    }
}

/// Determina si un fondo activo pide texto oscuro para contraste óptimo.
#[inline]
pub fn prefers_dark_fg(c: Color32) -> bool {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    let lum = 0.299 * (r as f32) + 0.587 * (g as f32) + 0.114 * (b as f32);
    lum > 138.0
}

/// Botón de texto en cápsula biselada GPU (`FILE`, `PLAY`, `ADD`, `EDIT`, `TRACK`, `S`, `M`).
pub fn capsule_text_button(
    ui: &mut Ui,
    size: Vec2,
    label: &str,
    font_size: f32,
    active: bool,
    accent: Color32,
    join: JoinSide,
    corner_notch: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let mut flags = join.to_flags();
        if active {
            flags |= FLAG_ACTIVE;
        }
        if response.hovered() {
            flags |= FLAG_HOVERED;
        }
        if corner_notch {
            flags |= FLAG_CORNER_NOTCH;
        }
        paint_capsule(ui.painter(), rect, 3.0, BW_CAPSULE_IDLE, accent, flags);

        let fg = if active {
            if prefers_dark_fg(accent) {
                Color32::from_rgb(18, 18, 20)
            } else {
                Color32::WHITE
            }
        } else if response.hovered() {
            Color32::WHITE
        } else {
            Color32::from_rgb(234, 236, 240)
        };

        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            label,
            fonts::semibold(font_size),
            fg,
        );
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

/// Botón de icono vectorial en cápsula biselada GPU (`▶`, `■`, `●`, `Undo`, `Redo`, etc.).
pub fn capsule_icon_button(
    ui: &mut Ui,
    size: Vec2,
    active: bool,
    accent: Color32,
    join: JoinSide,
    paint_fg: impl FnOnce(&Painter, Rect, Color32),
) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let mut flags = join.to_flags();
        if active {
            flags |= FLAG_ACTIVE;
        }
        if response.hovered() {
            flags |= FLAG_HOVERED;
        }
        paint_capsule(ui.painter(), rect, 3.0, BW_CAPSULE_IDLE, accent, flags);

        let fg = if active {
            if prefers_dark_fg(accent) {
                Color32::from_rgb(18, 18, 20)
            } else {
                Color32::WHITE
            }
        } else if response.hovered() {
            Color32::WHITE
        } else {
            Color32::from_rgb(232, 234, 238)
        };

        let pad = (size.x.min(size.y) * 0.22).clamp(3.0, 6.5);
        paint_fg(ui.painter(), rect.shrink(pad), fg);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

/// Botón plano de barra/sub-barra con icono vectorial (ej. selectores Clip Launcher / Arranger / Footer).
pub fn flat_icon_toggle(
    ui: &mut Ui,
    size: Vec2,
    active: bool,
    active_color: Color32,
    inactive_color: Color32,
    paint_fg: impl FnOnce(&Painter, Rect, Color32),
) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        if response.hovered() {
            ui.painter()
                .rect_filled(rect.shrink(1.0), 2.5, Color32::from_rgba_unmultiplied(255, 255, 255, 16));
        }
        let fg = if active {
            active_color
        } else if response.hovered() {
            Color32::WHITE
        } else {
            inactive_color
        };
        let pad = (size.x.min(size.y) * 0.18).clamp(2.0, 5.0);
        paint_fg(ui.painter(), rect.shrink(pad), fg);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

/// Perilla rotativa `270°` en GPU (`wgpu` SDF) con arrastre vertical y doble clic de reset.
#[allow(clippy::too_many_arguments)]
pub fn rotary_knob(
    ui: &mut Ui,
    diameter: f32,
    value: &mut f32,
    range: RangeInclusive<f32>,
    default_value: f32,
    arc_color: Color32,
    bipolar: bool,
) -> (Response, bool) {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::click_and_drag());
    let lo = *range.start();
    let hi = *range.end();
    let span = (hi - lo).max(1.0e-6);
    let mut changed = false;

    if response.double_clicked() {
        *value = default_value.clamp(lo, hi);
        changed = true;
    } else if response.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let sens = if fine { 0.0012 } else { 0.0055 };
        let delta_norm = -response.drag_delta().y * sens;
        if delta_norm.abs() > 0.0 {
            let cur_norm = ((*value - lo) / span).clamp(0.0, 1.0);
            let next_norm = (cur_norm + delta_norm).clamp(0.0, 1.0);
            *value = lo + next_norm * span;
            changed = true;
        }
    }

    if ui.is_rect_visible(rect) {
        let norm = ((*value - lo) / span).clamp(0.0, 1.0);
        paint_rotary_knob(
            ui.painter(),
            rect,
            norm,
            BW_KNOB_CAP,
            arc_color,
            response.hovered() || response.dragged(),
            bipolar,
        );
    }

    if response.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::None);
    } else if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
    }

    (response, changed)
}

/// Fader vertical normalizado (`0.0 ..= 1.0`, con `0.75 = 0.0 dB`) compatible con `Track::volume`
/// y `mixer::db_text`, renderizado en GPU estilo Mix Panel de Bitwig Studio 5.1.7.
pub fn fader_vertical_track_vol(
    ui: &mut Ui,
    size: Vec2,
    volume: &mut f32,
    default_vol: f32,
) -> (Response, bool) {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let mut changed = false;

    if response.double_clicked() {
        *volume = default_vol.clamp(0.0, 1.0);
        changed = true;
    } else if response.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        if fine {
            let next = (*volume - response.drag_delta().y / (rect.height().max(24.0) * 4.0)).clamp(0.0, 1.0);
            *volume = next;
            changed = true;
        } else if let Some(pos) = response.interact_pointer_pos() {
            let norm = (1.0 - (pos.y - rect.min.y) / rect.height().max(1.0)).clamp(0.0, 1.0);
            *volume = norm;
            changed = true;
        }
    } else if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let norm = (1.0 - (pos.y - rect.min.y) / rect.height().max(1.0)).clamp(0.0, 1.0);
            *volume = norm;
            changed = true;
        }
    }

    if ui.is_rect_visible(rect) {
        paint_fader_vertical(
            ui.painter(),
            rect,
            volume.clamp(0.0, 1.0),
            BW_FADER_AMBER,
            BW_FADER_ORANGE,
            response.hovered() || response.dragged(),
        );
    }

    if response.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::None);
    } else if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
    }

    (response, changed)
}

/// Fader vertical en dB (`-60.0 .. +6.0 dB`) renderizado en GPU estilo Mix Panel de Bitwig.
pub fn fader_vertical_db(
    ui: &mut Ui,
    size: Vec2,
    volume_db: &mut f32,
    default_db: f32,
) -> (Response, bool) {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let mut changed = false;

    if response.double_clicked() {
        *volume_db = default_db.clamp(-60.0, 6.0);
        changed = true;
    } else if response.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        if fine {
            let cur = db_to_fader_norm(*volume_db);
            let next = (cur - response.drag_delta().y / (rect.height().max(24.0) * 4.0)).clamp(0.0, 1.0);
            *volume_db = fader_norm_to_db(next);
            changed = true;
        } else if let Some(pos) = response.interact_pointer_pos() {
            let norm = (1.0 - (pos.y - rect.min.y) / rect.height().max(1.0)).clamp(0.0, 1.0);
            *volume_db = fader_norm_to_db(norm);
            changed = true;
        }
    } else if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let norm = (1.0 - (pos.y - rect.min.y) / rect.height().max(1.0)).clamp(0.0, 1.0);
            *volume_db = fader_norm_to_db(norm);
            changed = true;
        }
    }

    if ui.is_rect_visible(rect) {
        let norm = db_to_fader_norm(*volume_db);
        paint_fader_vertical(
            ui.painter(),
            rect,
            norm,
            BW_FADER_AMBER,
            BW_FADER_ORANGE,
            response.hovered() || response.dragged(),
        );
    }

    if response.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::None);
    } else if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
    }

    (response, changed)
}

/// Fader horizontal en dB (`-60.0 .. +6.0 dB`) estilo Track Header de Bitwig Arrange View.
pub fn fader_horizontal_db(
    ui: &mut Ui,
    size: Vec2,
    volume_db: &mut f32,
    default_db: f32,
) -> (Response, bool) {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let mut changed = false;

    if response.double_clicked() {
        *volume_db = default_db.clamp(-60.0, 6.0);
        changed = true;
    } else if response.dragged() || response.clicked() {
        let fine = ui.input(|i| i.modifiers.shift) && response.dragged();
        if fine {
            let cur = db_to_fader_norm(*volume_db);
            let next = (cur + response.drag_delta().x / (rect.width().max(24.0) * 4.0)).clamp(0.0, 1.0);
            *volume_db = fader_norm_to_db(next);
            changed = true;
        } else if let Some(pos) = response.interact_pointer_pos() {
            let norm = ((pos.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 1.0);
            *volume_db = fader_norm_to_db(norm);
            changed = true;
        }
    }

    if ui.is_rect_visible(rect) {
        let norm = db_to_fader_norm(*volume_db);
        paint_fader_horizontal(
            ui.painter(),
            rect,
            norm,
            BW_FADER_AMBER,
            BW_FADER_ORANGE,
            response.hovered() || response.dragged(),
            false,
        );
    }

    if response.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::None);
    } else if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }

    (response, changed)
}

/// Fader horizontal genérico o barra de paneo bipolar (`bipolar = true`) renderizado en GPU.
pub fn fader_horizontal_range(
    ui: &mut Ui,
    size: Vec2,
    value: &mut f32,
    range: RangeInclusive<f32>,
    default_value: f32,
    fill_color: Color32,
    cap_color: Color32,
    bipolar: bool,
) -> (Response, bool) {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let lo = *range.start();
    let hi = *range.end();
    let span = (hi - lo).max(1.0e-6);
    let mut changed = false;

    if response.double_clicked() {
        *value = default_value.clamp(lo, hi);
        changed = true;
    } else if response.dragged() || response.clicked() {
        let fine = ui.input(|i| i.modifiers.shift) && response.dragged();
        if fine {
            let cur = ((*value - lo) / span).clamp(0.0, 1.0);
            let next = (cur + response.drag_delta().x / (rect.width().max(24.0) * 4.0)).clamp(0.0, 1.0);
            *value = lo + next * span;
            changed = true;
        } else if let Some(pos) = response.interact_pointer_pos() {
            let norm = ((pos.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 1.0);
            *value = lo + norm * span;
            changed = true;
        }
    }

    if ui.is_rect_visible(rect) {
        let norm = ((*value - lo) / span).clamp(0.0, 1.0);
        paint_fader_horizontal(
            ui.painter(),
            rect,
            norm,
            fill_color,
            cap_color,
            response.hovered() || response.dragged(),
            bipolar,
        );
    }

    if response.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::None);
    } else if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }

    (response, changed)
}

/// Vúmetro estéreo vertical dual (`[level_l, level_r, hold_l, hold_r]`) renderizado en GPU.
pub fn stereo_meter_bar(ui: &mut Ui, size: Vec2, levels_and_holds: [f32; 4]) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        paint_stereo_meter(ui.painter(), rect, levels_and_holds);
    }
    response
}
