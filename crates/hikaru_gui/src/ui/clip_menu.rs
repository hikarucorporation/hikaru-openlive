// Hikaru OpenLive - Bitwig Studio 5.x style CLIP dropdown menu
// GNU AGPLv3
// crates/hikaru_gui/src/ui/clip_menu.rs
//
// Menú desplegable de [CLIP] pintado a mano para reproducir 1:1 el layout de
// dos columnas, iconos, atajos, separadores, pines y comportamiento contextual
// de Bitwig Studio 5.x:
//
//   COLUMNA IZQUIERDA                          COLUMNA DERECHA
//   [⟳] Loop Selected Region     Ctrl+L    📌 | 8vb Transpose an Octave Down Shift+Alt+- 📌
//   ───────────────────────────────────────── | ───────────────────────────────────────────
//   [◫] Consolidate              Ctrl+J    📌 | [⌈] Quantize                  Q          📌
//   ───────────────────────────────────────── | [⌈] Quantize...               Alt+Q      📌
//   [x2] Double Content                    📌 | [|||] Quantize Audio                     📌
//   [◁|▷] Reverse                          📌 | [|||] Quantize Audio...                  📌
//   [|||◂] Scale Each 50%                  📌 | [♫] Make Legato         Ctrl+Shift+Alt+L 📌
//   [|||▸] Scale Each 200%                 📌 | ───────────────────────────────────────────
//   [||||◂] Scale 50%                      📌 | [→]] Save Arranger clip To Library       📌
//   [||||▸] Scale 200%                     📌
//   [|||%] Scale...                        📌
//   ─────────────────────────────────────────
//   [↕] Normalize                          📌
//   ─────────────────────────────────────────
//   [↷] Bounce In Place          Ctrl+B    📌
//   [↗] Bounce                             📌
//   ─────────────────────────────────────────
//   [┼] Slice In Place...                  📌
//   [☷→] Slice to Drum Machine... Ctrl+Alt+D 📌
//   [⫴→] Slice to Multisample...           📌
//   [↓=] Fold to Takes...                  📌
//   ─────────────────────────────────────────
//   [⋀×] Reset Fades             Ctrl+Alt+F 📌
//   [◢]  Auto-Fade               Ctrl+F    📌
//   [⧓]  Auto-Crossfade          Ctrl+Shift+F 📌
//   ─────────────────────────────────────────
//   #    Transpose a Semitone Up  Alt++    📌
//   ♭    Transpose a Semitone Down Alt+-   📌
//   8va  Transpose an Octave Up Shift+Alt++ 📌

use std::collections::HashSet;

use egui::{
    pos2, vec2, Align2, Color32, Context, CursorIcon, Id, Order, Pos2, Rect, Sense, Stroke, Ui,
};

use crate::app::AppState;
use crate::ui::bitwig_kit::BW_ACCENT_ORANGE;
use crate::ui::fonts;
use crate::ui::icons::{self, VectorIcon};
use crate::views::playlist::{self, ClipType};

const LEFT_COL_W: f32 = 278.0;
const RIGHT_COL_W: f32 = 286.0;
const COL_GAP: f32 = 10.0;
const PAD_X: f32 = 6.0;
const PAD_Y: f32 = 4.0;
const ROW_H: f32 = 26.0;
const SEP_H: f32 = 11.0;

const MENU_BG: Color32 = Color32::from_rgb(0x2D, 0x2D, 0x2F);
const MENU_BORDER: Color32 = Color32::from_rgb(0x18, 0x18, 0x1A);
const SEP: Color32 = Color32::from_rgb(0x46, 0x46, 0x49);
const ROW_HOVER: Color32 = Color32::from_rgb(0xBE, 0xBE, 0xBE);
const TXT: Color32 = Color32::from_rgb(0xEA, 0xEA, 0xEA);
const TXT_DISABLED: Color32 = Color32::from_rgb(0x74, 0x74, 0x74);
const TXT_HINT: Color32 = Color32::from_rgb(0x98, 0x98, 0x98);
const TXT_ON_HOVER: Color32 = Color32::from_rgb(0x1A, 0x1A, 0x1A);
const PIN_IDLE: Color32 = Color32::from_rgb(0x9A, 0x9A, 0xA0);

const fn vi(d: &'static str) -> VectorIcon {
    VectorIcon { filled: false, d }
}

// Iconos vectoriales 24x24 calibrados al menú CLIP de Bitwig Studio 5.x
const IC_LOOP_REGION: VectorIcon = vi(
    "M 4 7 L 20 7 L 20 17 L 4 17 Z M 9 10 L 15 10 M 13 8 L 15 10 L 13 12 M 15 14 L 9 14 M 11 12 L 9 14 L 11 16",
);
const IC_CONSOLIDATE: VectorIcon = vi("M 4 6 L 20 6 L 20 18 L 4 18 Z M 12 6 L 12 18");
const IC_DOUBLE_CONTENT: VectorIcon = vi(
    "M 4 6 L 20 6 L 20 18 L 4 18 Z M 7 10 L 11 14 M 11 10 L 7 14 M 14 10 L 17 10 L 17 12 L 14 12 L 14 14 L 17 14",
);
const IC_REVERSE: VectorIcon = vi(
    "M 12 5 L 12 19 M 9 7 L 4 17 L 9 17 Z M 15 7 L 20 17 L 15 17 Z",
);
const IC_SCALE_EACH_50: VectorIcon = vi(
    "M 4 8 L 4 16 M 7 8 L 7 16 M 10 8 L 10 16 M 19 12 L 14 12 M 16 9 L 14 12 L 16 15",
);
const IC_SCALE_EACH_200: VectorIcon = vi(
    "M 4 8 L 4 16 M 7 8 L 7 16 M 10 8 L 10 16 M 14 12 L 19 12 M 17 9 L 19 12 L 17 15",
);
const IC_SCALE_50: VectorIcon = vi(
    "M 4 7 L 4 17 M 7 7 L 7 17 M 10 7 L 10 17 M 13 7 L 13 17 M 20 12 L 15 12 M 17 9 L 15 12 L 17 15",
);
const IC_SCALE_200: VectorIcon = vi(
    "M 4 7 L 4 17 M 7 7 L 7 17 M 10 7 L 10 17 M 13 7 L 13 17 M 15 12 L 20 12 M 18 9 L 20 12 L 18 15",
);
const IC_SCALE_CUSTOM: VectorIcon = vi(
    "M 4 7 L 4 17 M 7 7 L 7 17 M 10 7 L 10 17 M 14 16 L 20 8 M 14 9 L 15 9 M 19 15 L 20 15",
);
const IC_NORMALIZE: VectorIcon = vi(
    "M 4 5 L 20 5 M 4 19 L 20 19 M 12 7 L 12 17 M 9 10 L 12 7 L 15 10 M 9 14 L 12 17 L 15 14",
);
const IC_BOUNCE_IN_PLACE: VectorIcon = vi(
    "M 5 13 L 9 8 L 14 8 L 17 11 L 17 16 M 14 13 L 17 16 L 20 13 M 4 19 L 20 19",
);
const IC_BOUNCE: VectorIcon = vi(
    "M 6 14 L 10 9 L 15 9 L 18 12 L 18 18 M 15 15 L 18 18 L 21 15",
);
const IC_SLICE_IN_PLACE: VectorIcon = vi(
    "M 4 12 L 20 12 M 8 6 L 8 18 M 14 6 L 14 18",
);
const IC_SLICE_DRUM: VectorIcon = vi(
    "M 4 8 L 8 8 M 4 12 L 10 12 M 4 16 L 8 16 M 12 12 L 19 12 M 16 9 L 19 12 L 16 15",
);
const IC_SLICE_MULTI: VectorIcon = vi(
    "M 5 17 L 5 9 M 8 17 L 8 6 M 11 17 L 11 11 M 14 13 L 20 13 M 17 10 L 20 13 L 17 16",
);
const IC_FOLD_TAKES: VectorIcon = vi(
    "M 4 14 L 20 14 M 4 18 L 20 18 M 12 5 L 12 11 M 9 8 L 12 11 L 15 8",
);
const IC_RESET_FADES: VectorIcon = vi(
    "M 4 17 L 10 7 L 14 7 L 20 17 M 15 13 L 20 18 M 20 13 L 15 18",
);
const IC_AUTO_FADE: VectorIcon = vi("M 4 18 L 18 18 L 18 6 Z");
const IC_AUTO_CROSSFADE: VectorIcon = vi("M 4 18 L 19 6 L 19 18 L 4 6 Z");
const IC_TRANSPOSE_SEMI_UP: VectorIcon = vi(
    "M 9 5 L 7 19 M 16 5 L 14 19 M 5 9 L 19 9 M 5 15 L 19 15",
);
const IC_TRANSPOSE_SEMI_DOWN: VectorIcon = vi(
    "M 8 5 L 8 19 M 8 13 C 12 11 16 12.5 16 15.5 C 16 18 12 19 8 19",
);
const IC_TRANSPOSE_OCT_UP: VectorIcon = vi(
    "M 12 5 L 12 19 M 7 10 L 12 5 L 17 10 M 7 15 L 12 10 L 17 15",
);
const IC_TRANSPOSE_OCT_DOWN: VectorIcon = vi(
    "M 12 5 L 12 19 M 7 14 L 12 19 L 17 14 M 7 9 L 12 14 L 17 9",
);
const IC_QUANTIZE: VectorIcon = vi("M 4 17 L 9 17 L 9 13 L 14 13 L 14 9 L 20 9");
const IC_QUANTIZE_OPTS: VectorIcon = vi(
    "M 4 17 L 9 17 L 9 12 L 14 12 L 14 7 L 19 7 M 5 7 L 9 7 M 7 5 L 7 9",
);
const IC_QUANTIZE_AUDIO: VectorIcon = vi(
    "M 4 6 L 4 18 M 8 9 L 8 15 M 12 5 L 12 19 M 16 8 L 16 16 M 20 6 L 20 18",
);
const IC_MAKE_LEGATO: VectorIcon = vi(
    "M 6 15 L 6 7 L 15 5 L 15 13 M 4 15 L 6 15 M 13 13 L 15 13 M 6 18 L 18 18",
);
const IC_SAVE_LIB: VectorIcon = vi(
    "M 4 12 L 15 12 M 12 9 L 15 12 L 12 15 M 16 6 L 20 6 L 20 18 L 16 18",
);
const IC_PIN: VectorIcon = vi(
    "M 15 4 L 20 9 M 16 5 L 11 10 L 8 10 L 14 16 L 14 13 L 19 8 M 11 13 L 5 19",
);

// =========================================================================
// ACCIONES Y ESTRUCTURA DEL MENÚ CLIP
// =========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClipMenuAction {
    LoopSelectedRegion,
    Consolidate,
    DoubleContent,
    Reverse,
    ScaleEach50,
    ScaleEach200,
    Scale50,
    Scale200,
    ScaleCustom,
    Normalize,
    BounceInPlace,
    Bounce,
    SliceInPlace,
    SliceToDrumMachine,
    SliceToMultisample,
    FoldToTakes,
    ResetFades,
    AutoFade,
    AutoCrossfade,
    TransposeSemitoneUp,
    TransposeSemitoneDown,
    TransposeOctaveUp,
    TransposeOctaveDown,
    Quantize,
    QuantizeOptions,
    QuantizeAudio,
    QuantizeAudioOptions,
    MakeLegato,
    SaveArrangerClipToLibrary,
}

pub const ALL_ACTIONS: [ClipMenuAction; 29] = [
    ClipMenuAction::LoopSelectedRegion,
    ClipMenuAction::Consolidate,
    ClipMenuAction::DoubleContent,
    ClipMenuAction::Reverse,
    ClipMenuAction::ScaleEach50,
    ClipMenuAction::ScaleEach200,
    ClipMenuAction::Scale50,
    ClipMenuAction::Scale200,
    ClipMenuAction::ScaleCustom,
    ClipMenuAction::Normalize,
    ClipMenuAction::BounceInPlace,
    ClipMenuAction::Bounce,
    ClipMenuAction::SliceInPlace,
    ClipMenuAction::SliceToDrumMachine,
    ClipMenuAction::SliceToMultisample,
    ClipMenuAction::FoldToTakes,
    ClipMenuAction::ResetFades,
    ClipMenuAction::AutoFade,
    ClipMenuAction::AutoCrossfade,
    ClipMenuAction::TransposeSemitoneUp,
    ClipMenuAction::TransposeSemitoneDown,
    ClipMenuAction::TransposeOctaveUp,
    ClipMenuAction::TransposeOctaveDown,
    ClipMenuAction::Quantize,
    ClipMenuAction::QuantizeOptions,
    ClipMenuAction::QuantizeAudio,
    ClipMenuAction::QuantizeAudioOptions,
    ClipMenuAction::MakeLegato,
    ClipMenuAction::SaveArrangerClipToLibrary,
];

const LEFT_SECTIONS: [&[ClipMenuAction]; 8] = [
    &[ClipMenuAction::LoopSelectedRegion],
    &[ClipMenuAction::Consolidate],
    &[
        ClipMenuAction::DoubleContent,
        ClipMenuAction::Reverse,
        ClipMenuAction::ScaleEach50,
        ClipMenuAction::ScaleEach200,
        ClipMenuAction::Scale50,
        ClipMenuAction::Scale200,
        ClipMenuAction::ScaleCustom,
    ],
    &[ClipMenuAction::Normalize],
    &[ClipMenuAction::BounceInPlace, ClipMenuAction::Bounce],
    &[
        ClipMenuAction::SliceInPlace,
        ClipMenuAction::SliceToDrumMachine,
        ClipMenuAction::SliceToMultisample,
        ClipMenuAction::FoldToTakes,
    ],
    &[
        ClipMenuAction::ResetFades,
        ClipMenuAction::AutoFade,
        ClipMenuAction::AutoCrossfade,
    ],
    &[
        ClipMenuAction::TransposeSemitoneUp,
        ClipMenuAction::TransposeSemitoneDown,
        ClipMenuAction::TransposeOctaveUp,
    ],
];

const RIGHT_SECTIONS: [&[ClipMenuAction]; 3] = [
    &[ClipMenuAction::TransposeOctaveDown],
    &[
        ClipMenuAction::Quantize,
        ClipMenuAction::QuantizeOptions,
        ClipMenuAction::QuantizeAudio,
        ClipMenuAction::QuantizeAudioOptions,
        ClipMenuAction::MakeLegato,
    ],
    &[ClipMenuAction::SaveArrangerClipToLibrary],
];

#[derive(Clone, Copy, Debug)]
struct ClipContext {
    selected_count: usize,
    has_audio_clip: bool,
    has_note_clip: bool,
    has_time_region: bool,
    has_piano_roll_notes: bool,
}

impl ClipContext {
    fn from_state(state: &AppState) -> Self {
        let selected = &state.playlist_state.selected_clips;
        let mut selected_count = 0usize;
        let mut has_audio_clip = false;
        let mut has_note_clip = false;

        for (_, clip) in &state.playlist_state.clips {
            if selected.contains(&clip.id) {
                selected_count += 1;
                match clip.clip_type {
                    ClipType::Audio { .. } => has_audio_clip = true,
                    ClipType::Pattern { .. } => has_note_clip = true,
                    ClipType::Automation { .. } => {}
                }
            }
        }

        let has_time_region = state.playlist_state.loop_region_active
            && state.playlist_state.loop_end_ticks > state.playlist_state.loop_start_ticks;
        let has_piano_roll_notes =
            state.show_piano_roll && !state.piano_roll_state.notes.is_empty();

        Self {
            selected_count,
            has_audio_clip,
            has_note_clip,
            has_time_region,
            has_piano_roll_notes,
        }
    }
}

pub fn action_icon(action: ClipMenuAction) -> VectorIcon {
    match action {
        ClipMenuAction::LoopSelectedRegion => IC_LOOP_REGION,
        ClipMenuAction::Consolidate => IC_CONSOLIDATE,
        ClipMenuAction::DoubleContent => IC_DOUBLE_CONTENT,
        ClipMenuAction::Reverse => IC_REVERSE,
        ClipMenuAction::ScaleEach50 => IC_SCALE_EACH_50,
        ClipMenuAction::ScaleEach200 => IC_SCALE_EACH_200,
        ClipMenuAction::Scale50 => IC_SCALE_50,
        ClipMenuAction::Scale200 => IC_SCALE_200,
        ClipMenuAction::ScaleCustom => IC_SCALE_CUSTOM,
        ClipMenuAction::Normalize => IC_NORMALIZE,
        ClipMenuAction::BounceInPlace => IC_BOUNCE_IN_PLACE,
        ClipMenuAction::Bounce => IC_BOUNCE,
        ClipMenuAction::SliceInPlace => IC_SLICE_IN_PLACE,
        ClipMenuAction::SliceToDrumMachine => IC_SLICE_DRUM,
        ClipMenuAction::SliceToMultisample => IC_SLICE_MULTI,
        ClipMenuAction::FoldToTakes => IC_FOLD_TAKES,
        ClipMenuAction::ResetFades => IC_RESET_FADES,
        ClipMenuAction::AutoFade => IC_AUTO_FADE,
        ClipMenuAction::AutoCrossfade => IC_AUTO_CROSSFADE,
        ClipMenuAction::TransposeSemitoneUp => IC_TRANSPOSE_SEMI_UP,
        ClipMenuAction::TransposeSemitoneDown => IC_TRANSPOSE_SEMI_DOWN,
        ClipMenuAction::TransposeOctaveUp => IC_TRANSPOSE_OCT_UP,
        ClipMenuAction::TransposeOctaveDown => IC_TRANSPOSE_OCT_DOWN,
        ClipMenuAction::Quantize => IC_QUANTIZE,
        ClipMenuAction::QuantizeOptions => IC_QUANTIZE_OPTS,
        ClipMenuAction::QuantizeAudio | ClipMenuAction::QuantizeAudioOptions => IC_QUANTIZE_AUDIO,
        ClipMenuAction::MakeLegato => IC_MAKE_LEGATO,
        ClipMenuAction::SaveArrangerClipToLibrary => IC_SAVE_LIB,
    }
}

fn action_glyph(action: ClipMenuAction) -> Option<&'static str> {
    match action {
        ClipMenuAction::TransposeSemitoneUp => Some("#"),
        ClipMenuAction::TransposeSemitoneDown => Some("♭"),
        ClipMenuAction::TransposeOctaveUp => Some("8va"),
        ClipMenuAction::TransposeOctaveDown => Some("8vb"),
        _ => None,
    }
}

pub fn action_label(action: ClipMenuAction) -> &'static str {
    match action {
        ClipMenuAction::LoopSelectedRegion => "Loop Selected Region",
        ClipMenuAction::Consolidate => "Consolidate",
        ClipMenuAction::DoubleContent => "Double Content",
        ClipMenuAction::Reverse => "Reverse",
        ClipMenuAction::ScaleEach50 => "Scale Each 50%",
        ClipMenuAction::ScaleEach200 => "Scale Each 200%",
        ClipMenuAction::Scale50 => "Scale 50%",
        ClipMenuAction::Scale200 => "Scale 200%",
        ClipMenuAction::ScaleCustom => "Scale...",
        ClipMenuAction::Normalize => "Normalize",
        ClipMenuAction::BounceInPlace => "Bounce In Place",
        ClipMenuAction::Bounce => "Bounce",
        ClipMenuAction::SliceInPlace => "Slice In Place...",
        ClipMenuAction::SliceToDrumMachine => "Slice to Drum Machine...",
        ClipMenuAction::SliceToMultisample => "Slice to Multisample...",
        ClipMenuAction::FoldToTakes => "Fold to Takes...",
        ClipMenuAction::ResetFades => "Reset Fades",
        ClipMenuAction::AutoFade => "Auto-Fade",
        ClipMenuAction::AutoCrossfade => "Auto-Crossfade",
        ClipMenuAction::TransposeSemitoneUp => "Transpose a Semitone Up",
        ClipMenuAction::TransposeSemitoneDown => "Transpose a Semitone Down",
        ClipMenuAction::TransposeOctaveUp => "Transpose an Octave Up",
        ClipMenuAction::TransposeOctaveDown => "Transpose an Octave Down",
        ClipMenuAction::Quantize => "Quantize",
        ClipMenuAction::QuantizeOptions => "Quantize...",
        ClipMenuAction::QuantizeAudio => "Quantize Audio",
        ClipMenuAction::QuantizeAudioOptions => "Quantize Audio...",
        ClipMenuAction::MakeLegato => "Make Legato",
        ClipMenuAction::SaveArrangerClipToLibrary => "Save Arranger clip To Library",
    }
}

pub fn action_shortcut(action: ClipMenuAction) -> &'static str {
    match action {
        ClipMenuAction::LoopSelectedRegion => "Ctrl+L",
        ClipMenuAction::Consolidate => "Ctrl+J",
        ClipMenuAction::BounceInPlace => "Ctrl+B",
        ClipMenuAction::SliceToDrumMachine => "Ctrl+Alt+D",
        ClipMenuAction::ResetFades => "Ctrl+Alt+F",
        ClipMenuAction::AutoFade => "Ctrl+F",
        ClipMenuAction::AutoCrossfade => "Ctrl+Shift+F",
        ClipMenuAction::TransposeSemitoneUp => "Alt++",
        ClipMenuAction::TransposeSemitoneDown => "Alt+-",
        ClipMenuAction::TransposeOctaveUp => "Shift+Alt++",
        ClipMenuAction::TransposeOctaveDown => "Shift+Alt+-",
        ClipMenuAction::Quantize => "Q",
        ClipMenuAction::QuantizeOptions => "Alt+Q",
        ClipMenuAction::MakeLegato => "Ctrl+Shift+Alt+L",
        _ => "",
    }
}

pub fn action_enabled(state: &AppState, action: ClipMenuAction) -> bool {
    let ctx = ClipContext::from_state(state);
    let has_clip = ctx.selected_count > 0;
    match action {
        ClipMenuAction::LoopSelectedRegion => has_clip || ctx.has_time_region,
        ClipMenuAction::Consolidate
        | ClipMenuAction::DoubleContent
        | ClipMenuAction::Reverse
        | ClipMenuAction::ScaleEach50
        | ClipMenuAction::ScaleEach200
        | ClipMenuAction::Scale50
        | ClipMenuAction::Scale200
        | ClipMenuAction::ScaleCustom
        | ClipMenuAction::BounceInPlace
        | ClipMenuAction::Bounce
        | ClipMenuAction::SliceInPlace
        | ClipMenuAction::SliceToDrumMachine
        | ClipMenuAction::SliceToMultisample
        | ClipMenuAction::SaveArrangerClipToLibrary => has_clip,
        ClipMenuAction::FoldToTakes => ctx.selected_count >= 2,
        ClipMenuAction::Normalize
        | ClipMenuAction::ResetFades
        | ClipMenuAction::AutoFade
        | ClipMenuAction::AutoCrossfade
        | ClipMenuAction::QuantizeAudio
        | ClipMenuAction::QuantizeAudioOptions => ctx.has_audio_clip,
        ClipMenuAction::TransposeSemitoneUp
        | ClipMenuAction::TransposeSemitoneDown
        | ClipMenuAction::TransposeOctaveUp
        | ClipMenuAction::TransposeOctaveDown
        | ClipMenuAction::Quantize
        | ClipMenuAction::QuantizeOptions => has_clip || ctx.has_piano_roll_notes,
        ClipMenuAction::MakeLegato => ctx.has_note_clip || ctx.has_piano_roll_notes,
    }
}

// =========================================================================
// ESTADO DEL POPUP Y BOTONES ANCLADOS (DEFAULT BITWIG: NINGUNO ANCLADO)
// =========================================================================

pub fn default_pinned() -> HashSet<ClipMenuAction> {
    HashSet::new()
}

pub fn is_open(ctx: &Context, key: Id) -> bool {
    ctx.data(|d| d.get_temp::<bool>(key.with("open")))
        .unwrap_or(false)
}

pub fn toggle(ctx: &Context, key: Id, pos: Pos2) {
    let currently_open = is_open(ctx, key);
    ctx.data_mut(|d| {
        d.insert_temp(key.with("open"), !currently_open);
        d.insert_temp(key.with("pos"), pos);
    });
}

pub fn close(ctx: &Context, key: Id) {
    ctx.data_mut(|d| d.insert_temp(key.with("open"), false));
}

fn load_pinned(ctx: &Context, key: Id) -> HashSet<ClipMenuAction> {
    ctx.data(|d| d.get_temp::<HashSet<ClipMenuAction>>(key.with("pinned")))
        .unwrap_or_else(default_pinned)
}

fn save_pinned(ctx: &Context, key: Id, pinned: HashSet<ClipMenuAction>) {
    ctx.data_mut(|d| d.insert_temp(key.with("pinned"), pinned));
}

pub fn pinned_actions(ctx: &Context, key: Id) -> Vec<ClipMenuAction> {
    let pinned = load_pinned(ctx, key);
    ALL_ACTIONS
        .iter()
        .copied()
        .filter(|a| pinned.contains(a))
        .collect()
}

fn column_height(sections: &[&[ClipMenuAction]]) -> f32 {
    let rows: usize = sections.iter().map(|s| s.len()).sum();
    let seps = sections.len().saturating_sub(1);
    (rows as f32) * ROW_H + (seps as f32) * SEP_H
}

pub fn show(
    ctx: &Context,
    key: Id,
    anchor_rect: Rect,
    state: &AppState,
) -> Option<ClipMenuAction> {
    if !is_open(ctx, key) {
        return None;
    }

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close(ctx, key);
        return None;
    }

    let raw_pos = ctx
        .data(|d| d.get_temp::<Pos2>(key.with("pos")))
        .unwrap_or_else(|| pos2(anchor_rect.min.x, anchor_rect.max.y + 2.0));

    let total_w = PAD_X * 2.0 + LEFT_COL_W + COL_GAP + RIGHT_COL_W;
    let body_h = column_height(&LEFT_SECTIONS).max(column_height(&RIGHT_SECTIONS));
    let total_h = body_h + PAD_Y * 2.0;

    let scr = ctx.screen_rect();
    let pos = pos2(
        raw_pos.x.min(scr.max.x - total_w - 4.0).max(scr.min.x + 2.0),
        raw_pos.y.min(scr.max.y - total_h - 4.0).max(scr.min.y + 2.0),
    );

    let mut pinned = load_pinned(ctx, key);
    let mut pinned_changed = false;
    let mut chosen: Option<ClipMenuAction> = None;

    let area_resp = egui::Area::new(key.with("area"))
        .fixed_pos(pos)
        .order(Order::Foreground)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(total_w, total_h), Sense::click());
            let p = ui.painter();
            p.rect_filled(
                rect.translate(vec2(2.0, 4.0)),
                2.5,
                Color32::from_black_alpha(145),
            );
            p.rect_filled(rect, 2.5, MENU_BG);
            p.rect_stroke(
                rect,
                2.5,
                Stroke::new(1.0_f32, MENU_BORDER),
                egui::StrokeKind::Inside,
            );

            let left_x = rect.min.x + PAD_X;
            let right_x = left_x + LEFT_COL_W + COL_GAP;
            let top_y = rect.min.y + PAD_Y;

            draw_column(
                ui,
                key,
                left_x,
                top_y,
                LEFT_COL_W,
                &LEFT_SECTIONS,
                state,
                &mut pinned,
                &mut pinned_changed,
                &mut chosen,
            );

            draw_column(
                ui,
                key,
                right_x,
                top_y,
                RIGHT_COL_W,
                &RIGHT_SECTIONS,
                state,
                &mut pinned,
                &mut pinned_changed,
                &mut chosen,
            );
        });

    if pinned_changed {
        save_pinned(ctx, key, pinned);
    }

    if chosen.is_some() {
        close(ctx, key);
        return chosen;
    }

    let clicked_outside = ctx.input(|i| {
        i.pointer.any_pressed()
            && i.pointer.interact_pos().is_some_and(|p| {
                !area_resp.response.rect.contains(p) && !anchor_rect.contains(p)
            })
    });
    if clicked_outside {
        close(ctx, key);
    }

    None
}

enum RowClick {
    None,
    Action,
    TogglePin,
}

#[allow(clippy::too_many_arguments)]
fn draw_column(
    ui: &mut Ui,
    key: Id,
    col_x: f32,
    start_y: f32,
    col_w: f32,
    sections: &[&[ClipMenuAction]],
    state: &AppState,
    pinned: &mut HashSet<ClipMenuAction>,
    pinned_changed: &mut bool,
    chosen: &mut Option<ClipMenuAction>,
) {
    let mut y = start_y;
    for (sec_idx, section) in sections.iter().enumerate() {
        if sec_idx > 0 {
            draw_separator(ui, col_x, col_w, y);
            y += SEP_H;
        }
        for &action in *section {
            let enabled = action_enabled(state, action);
            let is_pinned = pinned.contains(&action);

            match draw_row(ui, key, col_x, col_w, y, action, enabled, is_pinned) {
                RowClick::Action if enabled => *chosen = Some(action),
                RowClick::TogglePin => {
                    if !pinned.remove(&action) {
                        pinned.insert(action);
                    }
                    *pinned_changed = true;
                }
                RowClick::Action | RowClick::None => {}
            }
            y += ROW_H;
        }
    }
}

fn draw_separator(ui: &mut Ui, col_x: f32, col_w: f32, y: f32) {
    let sep_y = y + SEP_H * 0.5;
    ui.painter().line_segment(
        [pos2(col_x + 4.0, sep_y), pos2(col_x + col_w - 4.0, sep_y)],
        Stroke::new(1.0_f32, SEP),
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_row(
    ui: &mut Ui,
    key: Id,
    col_x: f32,
    col_w: f32,
    y: f32,
    action: ClipMenuAction,
    enabled: bool,
    is_pinned: bool,
) -> RowClick {
    let row_rect = Rect::from_min_size(pos2(col_x, y), vec2(col_w, ROW_H));
    let pin_rect = Rect::from_center_size(
        pos2(row_rect.max.x - 12.0, row_rect.center().y),
        vec2(18.0, ROW_H - 4.0),
    );
    let main_rect = Rect::from_min_max(row_rect.min, pos2(pin_rect.min.x - 2.0, row_rect.max.y));

    let pin_resp = ui.interact(pin_rect, key.with(("clip_pin", action)), Sense::click());
    let main_resp = ui.interact(main_rect, key.with(("clip_row", action)), Sense::click());

    let hovered_action = enabled && main_resp.hovered();
    let hovered_pin = pin_resp.hovered();
    let hovered_row = hovered_action || (enabled && hovered_pin);
    let p = ui.painter();

    if hovered_row {
        p.rect_filled(row_rect, 2.0, ROW_HOVER);
    }
    if hovered_action || hovered_pin {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    let fg = if !enabled {
        TXT_DISABLED
    } else if hovered_row {
        TXT_ON_HOVER
    } else {
        TXT
    };
    let hint_fg = if !enabled {
        TXT_DISABLED
    } else if hovered_row {
        TXT_ON_HOVER
    } else {
        TXT_HINT
    };

    // Icono o glifo musical izquierdo
    let ic_center = pos2(row_rect.min.x + 15.0, row_rect.center().y);
    if let Some(glyph) = action_glyph(action) {
        let sz = if glyph.len() > 2 { 9.5 } else { 12.5 };
        p.text(ic_center, Align2::CENTER_CENTER, glyph, fonts::semibold(sz), fg);
    } else {
        let ic_rect = Rect::from_center_size(ic_center, vec2(14.0, 14.0));
        icons::paint_icon(p, ic_rect, action_icon(action), fg, 1.25_f32);
    }

    // Etiqueta principal
    p.text(
        pos2(row_rect.min.x + 31.0, row_rect.center().y),
        Align2::LEFT_CENTER,
        action_label(action),
        fonts::regular(11.5),
        fg,
    );

    // Atajo de teclado (alineado a la derecha, antes del pin)
    let shortcut = action_shortcut(action);
    if !shortcut.is_empty() {
        p.text(
            pos2(pin_rect.min.x - 6.0, row_rect.center().y),
            Align2::RIGHT_CENTER,
            shortcut,
            fonts::regular(10.5),
            hint_fg,
        );
    }

    // Icono Pin derecho (siempre clickeable, igual que en Bitwig Studio)
    let pin_col = if is_pinned {
        BW_ACCENT_ORANGE
    } else if hovered_row {
        TXT_ON_HOVER
    } else if hovered_pin {
        TXT
    } else {
        PIN_IDLE
    };
    let pin_ic_rect = Rect::from_center_size(pin_rect.center(), vec2(11.0, 11.0));
    icons::paint_icon(p, pin_ic_rect, IC_PIN, pin_col, 1.2_f32);

    if pin_resp.clicked() {
        RowClick::TogglePin
    } else if main_resp.clicked() && enabled {
        RowClick::Action
    } else {
        RowClick::None
    }
}

// =========================================================================
// EJECUCIÓN DE ACCIONES DEL MENÚ CLIP
// =========================================================================

pub fn execute_action(state: &mut AppState, action: ClipMenuAction) -> bool {
    if !action_enabled(state, action) {
        return false;
    }

    let min_ticks = playlist::min_clip_ticks(state.playlist_state.ppqn);

    match action {
        ClipMenuAction::LoopSelectedRegion => {
            let sel = &state.playlist_state.selected_clips;
            let mut min_s: Option<u64> = None;
            let mut max_e: Option<u64> = None;
            for (_, c) in &state.playlist_state.clips {
                if sel.contains(&c.id) {
                    let end = c.start_tick.saturating_add(c.duration_ticks);
                    min_s = Some(min_s.map_or(c.start_tick, |m| m.min(c.start_tick)));
                    max_e = Some(max_e.map_or(end, |m| m.max(end)));
                }
            }
            if let (Some(s), Some(e)) = (min_s, max_e) {
                if e > s {
                    state
                        .edit_history
                        .push_playlist("Loop Selected Region", &state.playlist_state);
                    state.playlist_state.loop_start_ticks = s;
                    state.playlist_state.loop_end_ticks = e;
                    state.playlist_state.loop_region_active = true;
                    state.is_looping = true;
                    state.project_dirty = true;
                    return true;
                }
            }
            if state.playlist_state.is_loop_region_valid() {
                state.is_looping = true;
                return true;
            }
            false
        }
        ClipMenuAction::Consolidate => {
            state
                .edit_history
                .push_playlist("Consolidate", &state.playlist_state);
            let n = playlist::consolidate_selected_clips(&mut state.playlist_state);
            if n > 0 {
                state.project_dirty = true;
                true
            } else {
                false
            }
        }
        ClipMenuAction::DoubleContent | ClipMenuAction::ScaleEach200 => {
            state
                .edit_history
                .push_playlist(action_label(action), &state.playlist_state);
            let sel = state.playlist_state.selected_clips.clone();
            let mut changed = false;
            for (_, c) in &mut state.playlist_state.clips {
                if sel.contains(&c.id) {
                    c.duration_ticks = c.duration_ticks.saturating_mul(2).max(min_ticks);
                    changed = true;
                }
            }
            if changed {
                state.playlist_state.needs_full_sync = true;
                state.project_dirty = true;
            }
            changed
        }
        ClipMenuAction::ScaleEach50 => {
            state
                .edit_history
                .push_playlist("Scale Each 50%", &state.playlist_state);
            let sel = state.playlist_state.selected_clips.clone();
            let mut changed = false;
            for (_, c) in &mut state.playlist_state.clips {
                if sel.contains(&c.id) {
                    c.duration_ticks = (c.duration_ticks / 2).max(min_ticks);
                    changed = true;
                }
            }
            if changed {
                state.playlist_state.needs_full_sync = true;
                state.project_dirty = true;
            }
            changed
        }
        ClipMenuAction::Scale200 => {
            state
                .edit_history
                .push_playlist("Scale 200%", &state.playlist_state);
            let sel = state.playlist_state.selected_clips.clone();
            let origin = state
                .playlist_state
                .clips
                .iter()
                .filter(|(_, c)| sel.contains(&c.id))
                .map(|(_, c)| c.start_tick)
                .min()
                .unwrap_or(0);
            let mut changed = false;
            for (_, c) in &mut state.playlist_state.clips {
                if sel.contains(&c.id) {
                    let rel = c.start_tick.saturating_sub(origin);
                    c.start_tick = origin.saturating_add(rel.saturating_mul(2));
                    c.duration_ticks = c.duration_ticks.saturating_mul(2).max(min_ticks);
                    changed = true;
                }
            }
            if changed {
                state.playlist_state.needs_full_sync = true;
                state.project_dirty = true;
            }
            changed
        }
        ClipMenuAction::Scale50 => {
            state
                .edit_history
                .push_playlist("Scale 50%", &state.playlist_state);
            let sel = state.playlist_state.selected_clips.clone();
            let origin = state
                .playlist_state
                .clips
                .iter()
                .filter(|(_, c)| sel.contains(&c.id))
                .map(|(_, c)| c.start_tick)
                .min()
                .unwrap_or(0);
            let mut changed = false;
            for (_, c) in &mut state.playlist_state.clips {
                if sel.contains(&c.id) {
                    let rel = c.start_tick.saturating_sub(origin);
                    c.start_tick = origin.saturating_add(rel / 2);
                    c.duration_ticks = (c.duration_ticks / 2).max(min_ticks);
                    changed = true;
                }
            }
            if changed {
                state.playlist_state.needs_full_sync = true;
                state.project_dirty = true;
            }
            changed
        }
        ClipMenuAction::Reverse => {
            state
                .edit_history
                .push_playlist("Reverse", &state.playlist_state);
            let sel = state.playlist_state.selected_clips.clone();
            let mut changed = false;
            let mut has_pattern = false;
            for (_, c) in &mut state.playlist_state.clips {
                if sel.contains(&c.id) {
                    match &mut c.clip_type {
                        ClipType::Audio { peaks, .. } => {
                            peaks.reverse();
                            changed = true;
                        }
                        ClipType::Pattern { .. } => {
                            has_pattern = true;
                            changed = true;
                        }
                        ClipType::Automation { points, .. } => {
                            let dur = c.duration_ticks;
                            for pt in points.iter_mut() {
                                pt.rel_tick = dur.saturating_sub(pt.rel_tick);
                            }
                            points.reverse();
                            changed = true;
                        }
                    }
                }
            }
            if has_pattern && !state.piano_roll_state.notes.is_empty() {
                let max_end = state
                    .piano_roll_state
                    .notes
                    .iter()
                    .map(|n| n.end_tick())
                    .max()
                    .unwrap_or(0);
                for n in &mut state.piano_roll_state.notes {
                    let end = n.end_tick();
                    n.start_tick = max_end.saturating_sub(end);
                }
            }
            if changed {
                state.playlist_state.needs_full_sync = true;
                state.project_dirty = true;
            }
            changed
        }
        ClipMenuAction::Normalize => {
            state
                .edit_history
                .push_playlist("Normalize", &state.playlist_state);
            let sel = state.playlist_state.selected_clips.clone();
            let mut changed = false;
            for (_, c) in &mut state.playlist_state.clips {
                if sel.contains(&c.id) {
                    if let ClipType::Audio { ref mut peaks, .. } = c.clip_type {
                        let max_p = peaks.iter().copied().fold(0.0_f32, |a, b| a.max(b.abs()));
                        if max_p > 1e-4 {
                            let gain = 1.0 / max_p;
                            for p in peaks.iter_mut() {
                                *p = (*p * gain).clamp(-1.0, 1.0);
                            }
                            changed = true;
                        }
                    }
                }
            }
            if changed {
                state.playlist_state.needs_full_sync = true;
                state.project_dirty = true;
            }
            changed
        }
        ClipMenuAction::SliceInPlace => {
            state
                .edit_history
                .push_playlist("Slice In Place", &state.playlist_state);
            let playhead = state.playlist_state.playhead_tick;
            let sliced = playlist::split_selected_clips_at(&mut state.playlist_state, playhead);
            if sliced > 0 {
                state.project_dirty = true;
                true
            } else {
                // Si el playhead no está sobre ningún clip seleccionado, cortar al punto medio del primero
                let mid = state
                    .playlist_state
                    .clips
                    .iter()
                    .find(|(_, c)| state.playlist_state.selected_clips.contains(&c.id))
                    .map(|(_, c)| c.start_tick + c.duration_ticks / 2);
                if let Some(mid_tick) = mid {
                    let n = playlist::split_selected_clips_at(&mut state.playlist_state, mid_tick);
                    if n > 0 {
                        state.project_dirty = true;
                        return true;
                    }
                }
                false
            }
        }
        ClipMenuAction::TransposeSemitoneUp => transpose_notes(state, 1),
        ClipMenuAction::TransposeSemitoneDown => transpose_notes(state, -1),
        ClipMenuAction::TransposeOctaveUp => transpose_notes(state, 12),
        ClipMenuAction::TransposeOctaveDown => transpose_notes(state, -12),
        ClipMenuAction::Quantize | ClipMenuAction::QuantizeAudio => {
            state
                .edit_history
                .push_playlist(action_label(action), &state.playlist_state);
            let (snap_step, _, _) = playlist::adaptive_grid_step(
                state.playlist_state.ppqn,
                state.transport.beats_per_bar,
                state.playlist_state.zoom_x,
            );
            let sel = state.playlist_state.selected_clips.clone();
            let mut changed = false;
            for (_, c) in &mut state.playlist_state.clips {
                if sel.contains(&c.id) {
                    c.start_tick = playlist::snap_ticks(c.start_tick, snap_step);
                    changed = true;
                }
            }
            if action == ClipMenuAction::Quantize && !state.piano_roll_state.notes.is_empty() {
                let note_step = (state.playlist_state.ppqn / 4).max(1);
                for n in &mut state.piano_roll_state.notes {
                    n.start_tick = playlist::snap_ticks(n.start_tick, note_step);
                }
                changed = true;
            }
            if changed {
                state.playlist_state.needs_full_sync = true;
                state.project_dirty = true;
            }
            changed
        }
        ClipMenuAction::MakeLegato => {
            let notes = &mut state.piano_roll_state.notes;
            if notes.len() < 2 {
                return false;
            }
            notes.sort_by_key(|n| (n.start_tick, n.pitch));
            let mut changed = false;
            for idx in 0..(notes.len() - 1) {
                let cur_start = notes[idx].start_tick;
                if let Some(next_start) = notes[idx + 1..]
                    .iter()
                    .map(|n| n.start_tick)
                    .find(|&s| s > cur_start)
                {
                    let new_dur = next_start.saturating_sub(cur_start).max(1);
                    if notes[idx].duration_ticks != new_dur {
                        notes[idx].duration_ticks = new_dur;
                        changed = true;
                    }
                }
            }
            if changed {
                state.project_dirty = true;
            }
            changed
        }
        ClipMenuAction::ScaleCustom
        | ClipMenuAction::BounceInPlace
        | ClipMenuAction::Bounce
        | ClipMenuAction::SliceToDrumMachine
        | ClipMenuAction::SliceToMultisample
        | ClipMenuAction::FoldToTakes
        | ClipMenuAction::ResetFades
        | ClipMenuAction::AutoFade
        | ClipMenuAction::AutoCrossfade
        | ClipMenuAction::QuantizeOptions
        | ClipMenuAction::QuantizeAudioOptions
        | ClipMenuAction::SaveArrangerClipToLibrary => false,
    }
}

fn transpose_notes(state: &mut AppState, semitones: i32) -> bool {
    if state.piano_roll_state.notes.is_empty() {
        return false;
    }
    for n in &mut state.piano_roll_state.notes {
        let next = (n.pitch as i32 + semitones).clamp(0, 127) as u8;
        n.pitch = next;
    }
    state.project_dirty = true;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_proxy::AudioProxy;
    use crate::views::piano_roll::MidiNote;
    use crate::views::playlist::{ClipType, PlaylistClip};
    use std::sync::atomic::{AtomicU32, AtomicU64};
    use std::sync::mpsc::channel;
    use std::sync::Arc;

    fn make_state() -> AppState {
        let (tx, _rx) = channel();
        AppState::new(
            AudioProxy::new(tx),
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU32::new(0)),
            None,
        )
    }

    fn make_pattern_clip(id: usize, start_tick: u64, duration_ticks: u64) -> PlaylistClip {
        PlaylistClip {
            id,
            name: format!("Inst {id}"),
            start_tick,
            duration_ticks,
            clip_type: ClipType::Pattern { pattern_id: id },
            color: Color32::from_rgb(0x9A, 0x4E, 0xD8),
            muted: false,
            loop_enabled: false,
        }
    }

    fn make_audio_clip(id: usize, start_tick: u64, duration_ticks: u64) -> PlaylistClip {
        PlaylistClip {
            id,
            name: format!("Audio {id}"),
            start_tick,
            duration_ticks,
            clip_type: ClipType::Audio {
                sample_path: "loop.wav".to_string(),
                sample_offset_ticks: 0,
                peaks: vec![0.25, 0.5, -0.25],
                total_sample_ticks: duration_ticks,
            },
            color: Color32::from_rgb(0xFF, 0x72, 0x00),
            muted: false,
            loop_enabled: false,
        }
    }

    #[test]
    fn default_pinned_is_empty_like_bitwig() {
        assert!(default_pinned().is_empty());
        assert_eq!(ALL_ACTIONS.len(), 29);
    }

    #[test]
    fn contextual_enablement_matches_bitwig_midi_vs_audio() {
        let mut state = make_state();
        state.playlist_state.clips.push((1, make_pattern_clip(1, 0, 3840)));
        state.playlist_state.clips.push((2, make_audio_clip(2, 0, 3840)));

        // Sin selección
        assert!(!action_enabled(&state, ClipMenuAction::Consolidate));
        assert!(!action_enabled(&state, ClipMenuAction::Normalize));
        assert!(!action_enabled(&state, ClipMenuAction::MakeLegato));

        // Con clip MIDI/Instrument seleccionado (exactamente como en captura 2 de Bitwig)
        state.playlist_state.selected_clips = vec![1];
        assert!(action_enabled(&state, ClipMenuAction::LoopSelectedRegion));
        assert!(action_enabled(&state, ClipMenuAction::Consolidate));
        assert!(action_enabled(&state, ClipMenuAction::DoubleContent));
        assert!(action_enabled(&state, ClipMenuAction::Reverse));
        assert!(!action_enabled(&state, ClipMenuAction::Normalize));
        assert!(action_enabled(&state, ClipMenuAction::BounceInPlace));
        assert!(!action_enabled(&state, ClipMenuAction::FoldToTakes));
        assert!(!action_enabled(&state, ClipMenuAction::ResetFades));
        assert!(!action_enabled(&state, ClipMenuAction::AutoFade));
        assert!(!action_enabled(&state, ClipMenuAction::AutoCrossfade));
        assert!(action_enabled(&state, ClipMenuAction::TransposeSemitoneUp));
        assert!(action_enabled(&state, ClipMenuAction::Quantize));
        assert!(!action_enabled(&state, ClipMenuAction::QuantizeAudio));
        assert!(action_enabled(&state, ClipMenuAction::MakeLegato));

        // Con clip Audio seleccionado
        state.playlist_state.selected_clips = vec![2];
        assert!(action_enabled(&state, ClipMenuAction::Normalize));
        assert!(action_enabled(&state, ClipMenuAction::ResetFades));
        assert!(action_enabled(&state, ClipMenuAction::AutoFade));
        assert!(action_enabled(&state, ClipMenuAction::AutoCrossfade));
        assert!(action_enabled(&state, ClipMenuAction::QuantizeAudio));
        assert!(!action_enabled(&state, ClipMenuAction::MakeLegato));
    }

    #[test]
    fn execute_clip_actions_work_end_to_end() {
        let mut state = make_state();
        state.playlist_state.clips.push((1, make_audio_clip(1, 100, 1920)));
        state.playlist_state.selected_clips = vec![1];

        // Loop Selected Region
        assert!(execute_action(&mut state, ClipMenuAction::LoopSelectedRegion));
        assert!(state.playlist_state.loop_region_active);
        assert_eq!(state.playlist_state.loop_start_ticks, 100);
        assert_eq!(state.playlist_state.loop_end_ticks, 2020);

        // Double Content & Scale Each 50%
        assert!(execute_action(&mut state, ClipMenuAction::DoubleContent));
        assert_eq!(state.playlist_state.clips[0].1.duration_ticks, 3840);
        assert!(execute_action(&mut state, ClipMenuAction::ScaleEach50));
        assert_eq!(state.playlist_state.clips[0].1.duration_ticks, 1920);

        // Normalize audio peaks
        assert!(execute_action(&mut state, ClipMenuAction::Normalize));
        if let ClipType::Audio { ref peaks, .. } = state.playlist_state.clips[0].1.clip_type {
            assert!((peaks[1] - 1.0).abs() < 1e-5);
        } else {
            panic!("expected audio clip");
        }

        // Transpose & Make Legato on notes
        state.piano_roll_state.notes = vec![
            MidiNote::new(60, 0, 240, 100),
            MidiNote::new(64, 960, 240, 100),
        ];
        state.show_piano_roll = true;
        assert!(execute_action(&mut state, ClipMenuAction::TransposeSemitoneUp));
        assert_eq!(state.piano_roll_state.notes[0].pitch, 61);
        assert!(execute_action(&mut state, ClipMenuAction::TransposeOctaveUp));
        assert_eq!(state.piano_roll_state.notes[0].pitch, 73);
        assert!(execute_action(&mut state, ClipMenuAction::MakeLegato));
        assert_eq!(state.piano_roll_state.notes[0].duration_ticks, 960);
    }
}
