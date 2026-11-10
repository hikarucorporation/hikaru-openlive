/*
 * Hikaru OpenStudio - Audio DAW
 * Copyright (C) 2026 Hikaru OpenStudio Developers
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 */

//! Menús conceptuales de click derecho estilo Bitwig Studio 5.1.7 para el Clip Launcher.
//!
//! Implementación limpia (clean-room) que reproduce la estructura, paleta de colores,
//! iconografía y comportamiento observados en:
//! - Clip Launcher con MIDI Clip (`Captura de pantalla_20261110_185858.png`)
//! - Clip Launcher con Audio Clip (`Captura de pantalla_20261110_185755.png`)
//! - Clip Launcher en ranura vacía / sin audio (`Captura de pantalla_20261110_185638.png`)
//!
//! Para clips con contenido (Audio o MIDI), comprime y organiza la información en
//! dos columnas elegantes lado a lado (evitando desbordamiento vertical fuera de pantalla).

use egui::{
    pos2, vec2, Align2, Color32, Id, Key, Order, Pos2, Rect, Response, Sense,
    Stroke, Ui,
};

use crate::app::AppState;
use crate::ui::fonts;
use crate::ui::icons;
use crate::views::matrix::{ClipData, SlotState};

/// Paleta de 24 colores para clips observada en Bitwig Studio 5.1.7 (3 filas × 8 columnas)
pub const BITWIG_CLIP_PALETTE: [[Color32; 8]; 3] = [
    // Fila 1: Tonos neutros, oscuros y tierra
    [
        Color32::from_rgb(0x40, 0x42, 0x48), // Gris oscuro pizarra
        Color32::from_rgb(0x6A, 0x6E, 0x78), // Gris medio
        Color32::from_rgb(0x9E, 0xA2, 0xAC), // Gris claro
        Color32::from_rgb(0x8C, 0x90, 0xA0), // Lavanda grisáceo
        Color32::from_rgb(0x9C, 0x76, 0x54), // Ocre tostado
        Color32::from_rgb(0xB8, 0x98, 0x72), // Arena cálida
        Color32::from_rgb(0x56, 0x6B, 0x85), // Azul pizarra
        Color32::from_rgb(0x76, 0x5C, 0x8C), // Púrpura apagado
    ],
    // Fila 2: Espectro vivo característico de Bitwig
    [
        Color32::from_rgb(0xE6, 0x24, 0x38), // Rojo intenso
        Color32::from_rgb(0xFF, 0x62, 0x14), // Naranja Bitwig
        Color32::from_rgb(0xF8, 0xA4, 0x00), // Ámbar dorado
        Color32::from_rgb(0x8E, 0xB8, 0x1E), // Lima oliva
        Color32::from_rgb(0x28, 0xAA, 0x48), // Verde esmeralda
        Color32::from_rgb(0x00, 0xA8, 0xCE), // Cian eléctrico
        Color32::from_rgb(0x18, 0x7C, 0xE0), // Azul brillante
        Color32::from_rgb(0x92, 0x48, 0xD4), // Violeta vivo
    ],
    // Fila 3: Variaciones suaves / pastel
    [
        Color32::from_rgb(0xF0, 0x74, 0x7E), // Salmón coral
        Color32::from_rgb(0xFF, 0x9A, 0x58), // Naranja suave
        Color32::from_rgb(0xFA, 0xCC, 0x54), // Trigo dorado
        Color32::from_rgb(0xB8, 0xDC, 0x64), // Menta lima
        Color32::from_rgb(0x60, 0xCA, 0x82), // Menta verde
        Color32::from_rgb(0x5A, 0xCC, 0xE8), // Celeste cielo
        Color32::from_rgb(0x66, 0xAA, 0xF4), // Azul aciano
        Color32::from_rgb(0xC0, 0x8E, 0xE8), // Lila pastel
    ],
];

// Colores del menú contextual
const MENU_BG: Color32 = Color32::from_rgb(0x22, 0x24, 0x28);
const MENU_BORDER: Color32 = Color32::from_rgb(0x2F, 0x32, 0x38);
const MENU_SEPARATOR: Color32 = Color32::from_rgb(0x38, 0x3B, 0x43);
const TEXT_ACTIVE: Color32 = Color32::from_rgb(0xE4, 0xE7, 0xED);
const TEXT_MUTED: Color32 = Color32::from_rgb(0x6E, 0x72, 0x7B);
const SHORTCUT_COLOR: Color32 = Color32::from_rgb(0x76, 0x7B, 0x86);
const HOVER_BG: Color32 = Color32::from_rgb(0x34, 0x38, 0x42);

#[derive(Clone, Debug)]
pub struct ClipLauncherContextMenu {
    pub track_idx: usize,
    pub scene_idx: usize,
    pub click_pos: Pos2,
}

/// Estado de la ranura para decidir qué menú renderizar
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotTargetType {
    Empty,
    AudioClip,
    MidiClip,
}

/// Acción producida por el menú contextual
pub enum ContextMenuAction {
    Recolor(Color32),
    ToggleMute,
    ToggleLoop,
    Cut,
    Copy,
    Paste,
    Duplicate,
    Delete,
    LoopSelectedRegion,
    Consolidate,
    DoubleContent,
    Scale50,
    Scale200,
    ScaleCustom,
    Normalize,
    BounceInPlace,
    Bounce,
    Expand,
    SliceToDrumMachine,
    SliceToMultisample,
    TransposeOctaveUp,
    TransposeOctaveDown,
    Quantize,
    QuantizeOptions,
    QuantizeAudio,
    QuantizeAudioOptions,
    MakeLegato,
    InsertFromLibrary,
    SaveClipToLibrary,
    CreateClip,
    ToggleStopButton,
    InsertScene,
    DeleteScene,
}

/// Dibuja el menú contextual completo de Bitwig sobre la pantalla
pub fn render_clip_context_menu(ui: &mut Ui, state: &mut AppState) {
    let Some(menu) = state.matrix_state.context_menu.clone() else {
        return;
    };

    let track_idx = menu.track_idx;
    let scene_idx = menu.scene_idx;

    let target_type = match state.matrix_state.slot(track_idx, scene_idx).and_then(|s| s.clip.as_ref()) {
        None => SlotTargetType::Empty,
        Some(c) => match &c.content {
            ClipData::Audio { .. } => SlotTargetType::AudioClip,
            ClipData::Midi { .. } => SlotTargetType::MidiClip,
        },
    };

    let has_clipboard_slot = state.matrix_clipboard.has_content();
    let has_stop_btn = state
        .matrix_state
        .slot(track_idx, scene_idx)
        .map(|s| s.has_stop_button)
        .unwrap_or(true);

    let is_clip_muted = state
        .matrix_state
        .slot(track_idx, scene_idx)
        .and_then(|s| s.clip.as_ref())
        .map(|c| c.muted)
        .unwrap_or(false);

    let is_clip_looping = state
        .matrix_state
        .slot(track_idx, scene_idx)
        .and_then(|s| s.clip.as_ref())
        .map(|c| c.loop_enabled)
        .unwrap_or(true);

    let screen_rect = ui.ctx().screen_rect();
    // Para clips con contenido, usamos diseño comprimido a dos columnas (w ~ 484px, h ~ 300px)
    let menu_width = if target_type == SlotTargetType::Empty {
        210.0
    } else {
        484.0
    };
    let estimated_height = if target_type == SlotTargetType::Empty {
        145.0
    } else {
        300.0
    };

    let clamped_x = if menu.click_pos.x + menu_width > screen_rect.max.x - 12.0 {
        (screen_rect.max.x - menu_width - 12.0).max(6.0)
    } else {
        menu.click_pos.x.max(6.0)
    };
    let clamped_y = if menu.click_pos.y + estimated_height > screen_rect.max.y - 12.0 {
        (screen_rect.max.y - estimated_height - 12.0).max(6.0)
    } else {
        menu.click_pos.y.max(6.0)
    };
    let origin = pos2(clamped_x, clamped_y);

    let mut action: Option<ContextMenuAction> = None;
    let mut close_menu = false;

    // Chequeo si el usuario presionó Escape
    if ui.input(|i| i.key_pressed(Key::Escape)) {
        state.matrix_state.context_menu = None;
        return;
    }

    egui::Area::new(Id::new("bitwig_clip_context_menu_area"))
        .fixed_pos(origin)
        .order(Order::Foreground)
        .show(ui.ctx(), |ui| {
            let frame = egui::Frame::new()
                .fill(MENU_BG)
                .stroke(Stroke::new(1.0_f32, MENU_BORDER))
                .corner_radius(egui::CornerRadius::same(3))
                .shadow(egui::Shadow {
                    offset: [0, 4],
                    blur: 16,
                    spread: 2,
                    color: Color32::from_black_alpha(180),
                })
                .inner_margin(egui::Margin::symmetric(4, 4));

            let response = frame.show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 1.0);

                match target_type {
                    SlotTargetType::Empty => {
                        ui.set_width(menu_width);
                        // =====================================================
                        // 1. RANURA VACÍA / SIN AUDIO (Compacto y vertical)
                        // =====================================================
                        if menu_item(ui, "Insert from Library...", Some("B"), true, Some(icons::FOLDER)).clicked() {
                            action = Some(ContextMenuAction::InsertFromLibrary);
                            close_menu = true;
                        }
                        if menu_item(ui, "Paste", Some("Ctrl+V"), has_clipboard_slot, None).clicked() {
                            action = Some(ContextMenuAction::Paste);
                            close_menu = true;
                        }

                        render_separator(ui);

                        if menu_item(ui, "Create Clip", None, true, Some(icons::PLUS)).clicked() {
                            action = Some(ContextMenuAction::CreateClip);
                            close_menu = true;
                        }

                        let stop_label = if has_stop_btn {
                            "Remove Stop Button"
                        } else {
                            "Insert Stop Button"
                        };
                        if menu_item(ui, stop_label, None, true, Some(icons::PLAYER_STOP)).clicked() {
                            action = Some(ContextMenuAction::ToggleStopButton);
                            close_menu = true;
                        }

                        render_separator(ui);

                        if menu_item(ui, "Insert Scene", None, true, None).clicked() {
                            action = Some(ContextMenuAction::InsertScene);
                            close_menu = true;
                        }
                        if menu_item(ui, "Delete Scene", None, true, None).clicked() {
                            action = Some(ContextMenuAction::DeleteScene);
                            close_menu = true;
                        }
                    }
                    SlotTargetType::AudioClip | SlotTargetType::MidiClip => {
                        let is_audio = target_type == SlotTargetType::AudioClip;
                        let is_midi = target_type == SlotTargetType::MidiClip;
                        let col_w = 236.0;

                        // =====================================================
                        // 2 & 3. AUDIO Y MIDI CLIP: COMPRESIÓN A LA DERECHA (2 COLUMNAS)
                        // =====================================================
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = vec2(4.0, 0.0);

                            // -------------------------------------------------
                            // COLUMNA IZQUIERDA: Paleta, Estados, Edit, Scales
                            // -------------------------------------------------
                            ui.vertical(|ui| {
                                ui.set_width(col_w);
                                ui.spacing_mut().item_spacing = vec2(0.0, 1.0);

                                // Paleta de Colores 3x8 compacta
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing = vec2(1.5, 1.5);
                                    ui.vertical(|ui| {
                                        for row in BITWIG_CLIP_PALETTE.iter() {
                                            ui.horizontal(|ui| {
                                                for &col in row.iter() {
                                                    let (swatch_rect, swatch_resp) =
                                                        ui.allocate_exact_size(vec2(15.0, 10.0), Sense::click());
                                                    let p = ui.painter();
                                                    if swatch_resp.hovered() {
                                                        p.rect_filled(swatch_rect, 1.5, col);
                                                        p.rect_stroke(
                                                            swatch_rect,
                                                            1.5,
                                                            Stroke::new(1.0_f32, Color32::WHITE),
                                                            egui::StrokeKind::Inside,
                                                        );
                                                    } else {
                                                        p.rect_filled(swatch_rect, 1.5, col);
                                                    }
                                                    if swatch_resp.clicked() {
                                                        action = Some(ContextMenuAction::Recolor(col));
                                                        close_menu = true;
                                                    }
                                                }
                                            });
                                        }
                                    });
                                });

                                render_separator(ui);

                                // Estados Muted / Looping con círculo toggle
                                if toggle_bullet_item(ui, "Muted", is_clip_muted, true).clicked() {
                                    action = Some(ContextMenuAction::ToggleMute);
                                    close_menu = true;
                                }
                                if toggle_bullet_item(ui, "Looping", is_clip_looping, true).clicked() {
                                    action = Some(ContextMenuAction::ToggleLoop);
                                    close_menu = true;
                                }

                                render_separator(ui);

                                // Barra horizontal [CUT | COPY | PASTE | DUPL. | DELETE]
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing = vec2(1.0, 0.0);
                                    let item_w = (col_w - 4.0) / 5.0;

                                    if action_toolbar_btn(ui, item_w, "CUT", Some(icons::CUT), true).clicked() {
                                        action = Some(ContextMenuAction::Cut);
                                        close_menu = true;
                                    }
                                    if action_toolbar_btn(ui, item_w, "COPY", Some(icons::FOLDERS), true).clicked() {
                                        action = Some(ContextMenuAction::Copy);
                                        close_menu = true;
                                    }
                                    if action_toolbar_btn(ui, item_w, "PASTE", None, has_clipboard_slot).clicked() {
                                        action = Some(ContextMenuAction::Paste);
                                        close_menu = true;
                                    }
                                    if action_toolbar_btn(ui, item_w, "DUPL.", Some(icons::DUPLICATE_PLUS), true).clicked() {
                                        action = Some(ContextMenuAction::Duplicate);
                                        close_menu = true;
                                    }
                                    if action_toolbar_btn(ui, item_w, "DELETE", Some(icons::CIRCLE_X), true).clicked() {
                                        action = Some(ContextMenuAction::Delete);
                                        close_menu = true;
                                    }
                                });

                                render_separator(ui);

                                // Loop Selected & Consolidate
                                if menu_item(ui, "Loop Selected Region", Some("Ctrl+L"), true, None).clicked() {
                                    action = Some(ContextMenuAction::LoopSelectedRegion);
                                    close_menu = true;
                                }
                                if menu_item(ui, "Consolidate", Some("Ctrl+J"), true, None).clicked() {
                                    action = Some(ContextMenuAction::Consolidate);
                                    close_menu = true;
                                }

                                render_separator(ui);

                                // Double & Scale
                                if menu_item(ui, "Double Content", None, true, None).clicked() {
                                    action = Some(ContextMenuAction::DoubleContent);
                                    close_menu = true;
                                }
                                if menu_item(ui, "Scale 50%", None, true, None).clicked() {
                                    action = Some(ContextMenuAction::Scale50);
                                    close_menu = true;
                                }
                                if menu_item(ui, "Scale 200%", None, true, None).clicked() {
                                    action = Some(ContextMenuAction::Scale200);
                                    close_menu = true;
                                }
                                if menu_item(ui, "Scale...", None, true, None).clicked() {
                                    action = Some(ContextMenuAction::ScaleCustom);
                                    close_menu = true;
                                }

                                render_separator(ui);

                                // Normalize: Activo en Audio, Deshabilitado en MIDI
                                if menu_item(ui, "Normalize", None, is_audio, None).clicked() {
                                    if is_audio {
                                        action = Some(ContextMenuAction::Normalize);
                                        close_menu = true;
                                    }
                                }
                            });

                            // -------------------------------------------------
                            // LÍNEA DIVISORIA VERTICAL ENTRE COLUMNAS
                            // -------------------------------------------------
                            let (sep_rect, _) = ui.allocate_exact_size(vec2(1.0, estimated_height - 16.0), Sense::hover());
                            let p = ui.painter();
                            p.line_segment(
                                [
                                    pos2(sep_rect.center().x, sep_rect.min.y + 2.0),
                                    pos2(sep_rect.center().x, sep_rect.max.y - 2.0),
                                ],
                                Stroke::new(1.0_f32, MENU_SEPARATOR),
                            );

                            // -------------------------------------------------
                            // COLUMNA DERECHA: Biblioteca, Bounce, Transpose, Quantize
                            // -------------------------------------------------
                            ui.vertical(|ui| {
                                ui.set_width(col_w);
                                ui.spacing_mut().item_spacing = vec2(0.0, 1.0);

                                // Biblioteca (comprimida arriba a la derecha como en Bitwig)
                                if menu_item(ui, "Insert from Library...", Some("B"), true, Some(icons::FOLDER)).clicked() {
                                    action = Some(ContextMenuAction::InsertFromLibrary);
                                    close_menu = true;
                                }
                                if menu_item(ui, "Save Launcher clip To Library", None, true, Some(icons::FILE_EXPORT)).clicked() {
                                    action = Some(ContextMenuAction::SaveClipToLibrary);
                                    close_menu = true;
                                }

                                render_separator(ui);

                                // Bounce & Slicing
                                if menu_item(ui, "Bounce In Place", Some("Ctrl+B"), true, None).clicked() {
                                    action = Some(ContextMenuAction::BounceInPlace);
                                    close_menu = true;
                                }
                                if menu_item(ui, "Bounce", None, true, None).clicked() {
                                    action = Some(ContextMenuAction::Bounce);
                                    close_menu = true;
                                }
                                if menu_item(ui, "Expand...", None, true, None).clicked() {
                                    action = Some(ContextMenuAction::Expand);
                                    close_menu = true;
                                }
                                if menu_item(ui, "Slice to Drum Machine...", Some("Ctrl+Alt+D"), true, None).clicked() {
                                    action = Some(ContextMenuAction::SliceToDrumMachine);
                                    close_menu = true;
                                }
                                if menu_item(ui, "Slice to Multisample...", None, true, None).clicked() {
                                    action = Some(ContextMenuAction::SliceToMultisample);
                                    close_menu = true;
                                }
                                let _ = menu_item(ui, "Fold to Takes...", None, false, None);

                                render_separator(ui);

                                // Octave Transposition
                                if menu_item(ui, "Transpose an Octave Up", Some("Shift+Alt++"), true, None).clicked() {
                                    action = Some(ContextMenuAction::TransposeOctaveUp);
                                    close_menu = true;
                                }
                                if menu_item(ui, "Transpose an Octave Down", Some("Shift+Alt+-"), true, None).clicked() {
                                    action = Some(ContextMenuAction::TransposeOctaveDown);
                                    close_menu = true;
                                }

                                render_separator(ui);

                                // Quantization & Timing
                                if menu_item(ui, "Quantize", Some("Q"), is_midi, None).clicked() {
                                    if is_midi {
                                        action = Some(ContextMenuAction::Quantize);
                                        close_menu = true;
                                    }
                                }
                                if menu_item(ui, "Quantize...", Some("Alt+Q"), is_midi, None).clicked() {
                                    if is_midi {
                                        action = Some(ContextMenuAction::QuantizeOptions);
                                        close_menu = true;
                                    }
                                }
                                if menu_item(ui, "Quantize Audio", None, is_audio, None).clicked() {
                                    if is_audio {
                                        action = Some(ContextMenuAction::QuantizeAudio);
                                        close_menu = true;
                                    }
                                }
                                if menu_item(ui, "Quantize Audio...", None, is_audio, None).clicked() {
                                    if is_audio {
                                        action = Some(ContextMenuAction::QuantizeAudioOptions);
                                        close_menu = true;
                                    }
                                }
                                if menu_item(ui, "Make Legato", Some("Ctrl+Shift+Alt+L"), is_midi, None).clicked() {
                                    if is_midi {
                                        action = Some(ContextMenuAction::MakeLegato);
                                        close_menu = true;
                                    }
                                }
                            });
                        });
                    }
                }
            });

            // Si se hace clic fuera del menú, cerrarlo
            if ui.input(|i| i.pointer.any_pressed()) {
                if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                    if !response.response.rect.contains(pos) {
                        close_menu = true;
                    }
                }
            }
        });

    if close_menu {
        state.matrix_state.context_menu = None;
    }

    // Ejecutar la acción seleccionada
    if let Some(act) = action {
        execute_context_menu_action(state, track_idx, scene_idx, act);
    }
}

/// Ejecuta las acciones funcionales del menú sobre el modelo de datos de Hikaru
fn execute_context_menu_action(
    state: &mut AppState,
    track_idx: usize,
    scene_idx: usize,
    action: ContextMenuAction,
) {
    match action {
        ContextMenuAction::Recolor(color) => {
            if let Some(slot) = state.matrix_state.slot_mut(track_idx, scene_idx) {
                if let Some(clip) = &mut slot.clip {
                    clip.color = Some(color);
                }
            }
        }
        ContextMenuAction::ToggleMute => {
            if let Some(slot) = state.matrix_state.slot_mut(track_idx, scene_idx) {
                if let Some(clip) = &mut slot.clip {
                    clip.muted = !clip.muted;
                    clip.local_track.mute = clip.muted;
                }
            }
        }
        ContextMenuAction::ToggleLoop => {
            if let Some(slot) = state.matrix_state.slot_mut(track_idx, scene_idx) {
                if let Some(clip) = &mut slot.clip {
                    clip.loop_enabled = !clip.loop_enabled;
                }
            }
        }
        ContextMenuAction::Cut => {
            if let Some(slot) = state.matrix_state.slot(track_idx, scene_idx) {
                state.matrix_clipboard.copy(slot);
            }
            if let Some(slot) = state.matrix_state.slot_mut(track_idx, scene_idx) {
                slot.clip = None;
                slot.state = SlotState::Empty;
            }
        }
        ContextMenuAction::Copy => {
            if let Some(slot) = state.matrix_state.slot(track_idx, scene_idx) {
                state.matrix_clipboard.copy(slot);
            }
        }
        ContextMenuAction::Paste => {
            if let Some(copied) = state.matrix_clipboard.copied_slot.clone() {
                if let Some(slot) = state.matrix_state.slot_mut(track_idx, scene_idx) {
                    slot.clip = copied.clip;
                    slot.state = if slot.clip.is_some() {
                        SlotState::Stopped
                    } else {
                        SlotState::Empty
                    };
                }
            }
        }
        ContextMenuAction::Duplicate => {
            if let Some(slot) = state.matrix_state.slot(track_idx, scene_idx).cloned() {
                let target_scene = scene_idx + 1;
                while state.matrix_state.scenes.len() <= target_scene {
                    state.matrix_state.add_scene();
                }
                if let Some(target) = state.matrix_state.slot_mut(track_idx, target_scene) {
                    target.clip = slot.clip;
                    target.state = if target.clip.is_some() {
                        SlotState::Stopped
                    } else {
                        SlotState::Empty
                    };
                }
            }
        }
        ContextMenuAction::Delete => {
            if let Some(slot) = state.matrix_state.slot_mut(track_idx, scene_idx) {
                slot.clip = None;
                slot.state = SlotState::Empty;
            }
        }
        ContextMenuAction::LoopSelectedRegion => {
            if let Some(slot) = state.matrix_state.slot_mut(track_idx, scene_idx) {
                if let Some(clip) = &mut slot.clip {
                    clip.loop_enabled = true;
                    clip.loop_start = 0;
                    clip.loop_end = (clip.duration_secs * 960.0).round() as u64;
                    if clip.loop_end == 0 {
                        clip.loop_end = 3840; // 1 compás a 960 PPQ
                    }
                }
            }
        }
        ContextMenuAction::Consolidate => {
            if let Some(slot) = state.matrix_state.slot_mut(track_idx, scene_idx) {
                if let Some(clip) = &mut slot.clip {
                    match &mut clip.content {
                        ClipData::Audio { .. } => {
                            clip.refresh_preview();
                        }
                        ClipData::Midi { notes } => {
                            notes.sort_by_key(|n| n.0);
                        }
                    }
                }
            }
        }
        ContextMenuAction::DoubleContent => {
            if let Some(slot) = state.matrix_state.slot_mut(track_idx, scene_idx) {
                if let Some(clip) = &mut slot.clip {
                    match &mut clip.content {
                        ClipData::Midi { notes } => {
                            let len = clip.loop_end.saturating_sub(clip.loop_start).max(3840);
                            let mut dup = Vec::new();
                            for n in notes.iter() {
                                dup.push((n.0 + len, n.1, n.2, n.3));
                            }
                            notes.extend(dup);
                            clip.loop_end += len;
                            clip.duration_secs *= 2.0;
                        }
                        ClipData::Audio { events, next_event_id, .. } => {
                            let dur = clip.duration_secs.max(1.0);
                            let mut dup = Vec::new();
                            for ev in events.iter() {
                                let mut cloned = ev.clone();
                                cloned.id = *next_event_id;
                                *next_event_id += 1;
                                cloned.start_secs += dur;
                                dup.push(cloned);
                            }
                            events.extend(dup);
                            clip.refresh_preview();
                            clip.duration_secs *= 2.0;
                        }
                    }
                }
            }
        }
        ContextMenuAction::Scale50 => {
            if let Some(slot) = state.matrix_state.slot_mut(track_idx, scene_idx) {
                if let Some(clip) = &mut slot.clip {
                    if let ClipData::Midi { notes } = &mut clip.content {
                        for n in notes.iter_mut() {
                            n.0 /= 2;
                            n.3 = (n.3 / 2).max(60);
                        }
                        clip.loop_start /= 2;
                        clip.loop_end = (clip.loop_end / 2).max(1);
                        clip.duration_secs /= 2.0;
                    }
                }
            }
        }
        ContextMenuAction::Scale200 => {
            if let Some(slot) = state.matrix_state.slot_mut(track_idx, scene_idx) {
                if let Some(clip) = &mut slot.clip {
                    if let ClipData::Midi { notes } = &mut clip.content {
                        for n in notes.iter_mut() {
                            n.0 *= 2;
                            n.3 *= 2;
                        }
                        clip.loop_start *= 2;
                        clip.loop_end *= 2;
                        clip.duration_secs *= 2.0;
                    }
                }
            }
        }
        ContextMenuAction::ScaleCustom => {}
        ContextMenuAction::Normalize => {
            if let Some(slot) = state.matrix_state.slot_mut(track_idx, scene_idx) {
                if let Some(clip) = &mut slot.clip {
                    if let ClipData::Audio { events, .. } = &mut clip.content {
                        let mut max_peak = 0.0f32;
                        for ev in events.iter() {
                            for &s in ev.samples.iter() {
                                let abs = s.abs();
                                if abs > max_peak {
                                    max_peak = abs;
                                }
                            }
                        }
                        if max_peak > 0.0001 {
                            let norm_factor = 1.0 / max_peak;
                            for ev in events.iter_mut() {
                                ev.gain *= norm_factor;
                            }
                            clip.refresh_preview();
                        }
                    }
                }
            }
        }
        ContextMenuAction::BounceInPlace | ContextMenuAction::Bounce => {}
        ContextMenuAction::Expand => {}
        ContextMenuAction::SliceToDrumMachine => {
            state.show_dsp_rack = true;
        }
        ContextMenuAction::SliceToMultisample => {
            state.show_dsp_rack = true;
        }
        ContextMenuAction::TransposeOctaveUp => {
            if let Some(slot) = state.matrix_state.slot_mut(track_idx, scene_idx) {
                if let Some(clip) = &mut slot.clip {
                    if let ClipData::Midi { notes } = &mut clip.content {
                        for n in notes.iter_mut() {
                            n.1 = (n.1 + 12).min(127);
                        }
                    }
                }
            }
        }
        ContextMenuAction::TransposeOctaveDown => {
            if let Some(slot) = state.matrix_state.slot_mut(track_idx, scene_idx) {
                if let Some(clip) = &mut slot.clip {
                    if let ClipData::Midi { notes } = &mut clip.content {
                        for n in notes.iter_mut() {
                            n.1 = n.1.saturating_sub(12);
                        }
                    }
                }
            }
        }
        ContextMenuAction::Quantize => {
            if let Some(slot) = state.matrix_state.slot_mut(track_idx, scene_idx) {
                if let Some(clip) = &mut slot.clip {
                    if let ClipData::Midi { notes } = &mut clip.content {
                        let grid = 240u64; // 1/16 a 960 PPQ
                        for n in notes.iter_mut() {
                            let rem = n.0 % grid;
                            if rem >= grid / 2 {
                                n.0 += grid - rem;
                            } else {
                                n.0 -= rem;
                            }
                        }
                    }
                }
            }
        }
        ContextMenuAction::QuantizeOptions => {}
        ContextMenuAction::QuantizeAudio => {}
        ContextMenuAction::QuantizeAudioOptions => {}
        ContextMenuAction::MakeLegato => {
            if let Some(slot) = state.matrix_state.slot_mut(track_idx, scene_idx) {
                if let Some(clip) = &mut slot.clip {
                    if let ClipData::Midi { notes } = &mut clip.content {
                        notes.sort_by_key(|n| n.0);
                        let n_len = notes.len();
                        for i in 0..n_len.saturating_sub(1) {
                            let next_start = notes[i + 1].0;
                            let cur_start = notes[i].0;
                            if next_start > cur_start {
                                notes[i].3 = (next_start - cur_start) as u32;
                            }
                        }
                    }
                }
            }
        }
        ContextMenuAction::InsertFromLibrary => {
            state.show_explorer = true;
        }
        ContextMenuAction::SaveClipToLibrary => {}
        ContextMenuAction::CreateClip => {
            crate::views::matrix::create_empty_midi_clip(
                &mut state.matrix_state,
                &state.audio_proxy,
                track_idx,
                scene_idx,
            );
        }
        ContextMenuAction::ToggleStopButton => {
            if let Some(slot) = state.matrix_state.slot_mut(track_idx, scene_idx) {
                slot.has_stop_button = !slot.has_stop_button;
            }
        }
        ContextMenuAction::InsertScene => {
            state.matrix_state.add_scene();
        }
        ContextMenuAction::DeleteScene => {
            if state.matrix_state.scenes.len() > 1 && scene_idx < state.matrix_state.scenes.len() {
                state.matrix_state.scenes.remove(scene_idx);
                for row in state.matrix_state.grid.iter_mut() {
                    if scene_idx < row.len() {
                        row.remove(scene_idx);
                    }
                }
            }
        }
    }
}

/// Dibuja una línea de ítem de menú estilo Bitwig con atajo alineado a la derecha
fn menu_item(
    ui: &mut Ui,
    label: &str,
    shortcut: Option<&str>,
    enabled: bool,
    icon: Option<icons::VectorIcon>,
) -> Response {
    let height = 20.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());

    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if enabled && resp.hovered() {
            p.rect_filled(rect, 2.0, HOVER_BG);
        }

        let text_color = if enabled {
            TEXT_ACTIVE
        } else {
            TEXT_MUTED
        };

        let mut text_x = rect.min.x + 6.0;

        if let Some(ic) = icon {
            let ic_rect = Rect::from_center_size(
                pos2(rect.min.x + 12.0, rect.center().y),
                vec2(11.0, 11.0),
            );
            icons::paint_icon(p, ic_rect, ic, text_color, 1.2_f32);
            text_x += 18.0;
        }

        p.text(
            pos2(text_x, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            fonts::regular(10.5),
            text_color,
        );

        if let Some(sc) = shortcut {
            let sc_col = if enabled {
                SHORTCUT_COLOR
            } else {
                TEXT_MUTED
            };
            p.text(
                pos2(rect.max.x - 6.0, rect.center().y),
                Align2::RIGHT_CENTER,
                sc,
                fonts::regular(9.5),
                sc_col,
            );
        }
    }

    resp
}

/// Dibuja un ítem con viñeta circular / toggle de Bitwig (para Muted y Looping)
fn toggle_bullet_item(ui: &mut Ui, label: &str, checked: bool, enabled: bool) -> Response {
    let height = 20.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());

    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if enabled && resp.hovered() {
            p.rect_filled(rect, 2.0, HOVER_BG);
        }

        let circle_center = pos2(rect.min.x + 12.0, rect.center().y);
        let circle_r = 4.5;

        if checked {
            p.circle_filled(circle_center, circle_r, Color32::from_rgb(0x8A, 0x90, 0x9C));
            // Marca de verificación en el centro
            let mark_col = Color32::from_rgb(0x18, 0x1A, 0x1E);
            p.line_segment(
                [
                    pos2(circle_center.x - 2.0, circle_center.y),
                    pos2(circle_center.x - 0.5, circle_center.y + 1.8),
                ],
                Stroke::new(1.3_f32, mark_col),
            );
            p.line_segment(
                [
                    pos2(circle_center.x - 0.5, circle_center.y + 1.8),
                    pos2(circle_center.x + 2.2, circle_center.y - 1.8),
                ],
                Stroke::new(1.3_f32, mark_col),
            );
        } else {
            p.circle_filled(circle_center, circle_r, Color32::from_rgb(0x40, 0x43, 0x4B));
        }

        let text_color = if enabled {
            TEXT_ACTIVE
        } else {
            TEXT_MUTED
        };
        p.text(
            pos2(rect.min.x + 24.0, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            fonts::regular(10.5),
            text_color,
        );
    }

    resp
}

/// Dibuja un botón de la tira horizontal de acciones [CUT | COPY | PASTE | DUPL. | DELETE]
fn action_toolbar_btn(
    ui: &mut Ui,
    width: f32,
    label: &str,
    icon: Option<icons::VectorIcon>,
    enabled: bool,
) -> Response {
    let height = 28.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click());

    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if enabled && resp.hovered() {
            p.rect_filled(rect, 2.0, HOVER_BG);
        }

        let col = if enabled {
            TEXT_ACTIVE
        } else {
            TEXT_MUTED
        };

        if let Some(ic) = icon {
            let ic_rect = Rect::from_center_size(
                pos2(rect.center().x, rect.min.y + 9.0),
                vec2(11.0, 11.0),
            );
            icons::paint_icon(p, ic_rect, ic, col, 1.2_f32);
        }

        p.text(
            pos2(rect.center().x, rect.max.y - 6.0),
            Align2::CENTER_CENTER,
            label,
            fonts::semibold(8.5),
            col,
        );
    }

    resp
}

/// Línea divisoria horizontal sutil
fn render_separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 4.0), Sense::hover());
    let p = ui.painter();
    p.line_segment(
        [
            pos2(rect.min.x + 2.0, rect.center().y),
            pos2(rect.max.x - 2.0, rect.center().y),
        ],
        Stroke::new(1.0_f32, MENU_SEPARATOR),
    );
}
