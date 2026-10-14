// Hikaru OpenLive - Theme
// GNU AGPLv3
// crates/hikaru_gui/src/theme.rs

use gpui_kit::Hsla;

// =========================================================================
// PALETA BASE: ABLETON / BITWIG DARK THEME
// =========================================================================

// Fondos y Superficies
pub const WINDOW_BG: Hsla = Hsla { h: 0.0, s: 0.0, l: 0.08, a: 1.0 };    // #141414 (Fondo principal)
pub const PANEL_BG: Hsla  = Hsla { h: 0.0, s: 0.0, l: 0.12, a: 1.0 };    // #1f1f1f (Contenedores)
pub const HEADER_BG: Hsla = Hsla { h: 0.0, s: 0.0, l: 0.10, a: 1.0 };    // #1a1a1a (Barra superior)
pub const SURFACE_BG: Hsla = Hsla { h: 0.0, s: 0.0, l: 0.16, a: 1.0 };   // #292929 (Tarjetas y elementos)

// Estados de Interacción (Botones / Items)
pub const HOVER_BG: Hsla   = Hsla { h: 0.0, s: 0.0, l: 0.22, a: 1.0 };   // Highlight al pasar el mouse
pub const ACTIVE_BG: Hsla  = Hsla { h: 0.0, s: 0.0, l: 0.28, a: 1.0 };   // Clic / Presionado
pub const SELECTED_BG: Hsla = Hsla { h: 0.60, s: 0.35, l: 0.25, a: 1.0 }; // Selección activa de track/clip

// Bordes y Divisores
pub const BORDER_COLOR: Hsla = Hsla { h: 0.0, s: 0.0, l: 0.18, a: 1.0 }; // Separadores sutiles
pub const BORDER_FOCUS: Hsla = Hsla { h: 0.12, s: 1.0, l: 0.50, a: 1.0 }; // Borde activo al seleccionar

// Texto
pub const TEXT_PRIMARY: Hsla   = Hsla { h: 0.0, s: 0.0, l: 0.88, a: 1.0 }; // Blanco/Gris claro (#e0e0e0)
pub const TEXT_MUTED: Hsla     = Hsla { h: 0.0, s: 0.0, l: 0.55, a: 1.0 }; // Gris medio para labels (#8c8c8c)
pub const TEXT_DISABLED: Hsla  = Hsla { h: 0.0, s: 0.0, l: 0.35, a: 1.0 }; // Elementos desactivados

// Acentos de DAW (Play, Record, Solo, Mute, Metrónomo)
pub const PLAY_GREEN: Hsla  = Hsla { h: 0.38, s: 0.85, l: 0.50, a: 1.0 }; // Play / Activo (#1feb54)
pub const REC_RED: Hsla     = Hsla { h: 0.00, s: 0.85, l: 0.55, a: 1.0 }; // Grabación (#f02a2a)
pub const ACCENT_ORANGE: Hsla = Hsla { h: 0.08, s: 0.95, l: 0.55, a: 1.0 }; // Ableton Orange (#f26c0d)
pub const SOLO_YELLOW: Hsla = Hsla { h: 0.14, s: 0.90, l: 0.50, a: 1.0 }; // Solo track
pub const MUTE_BLUE: Hsla   = Hsla { h: 0.55, s: 0.80, l: 0.45, a: 1.0 }; // Mute track
pub const ACCENT_COLOR: Hsla = Hsla { h: 0.08, s: 0.95, l: 0.55, a: 1.0 }; // Acento primario (naranja)
pub const ACCENT_STUDIO: Hsla = Hsla { h: 0.55, s: 0.80, l: 0.50, a: 1.0 }; // Acento studio (azul)
pub const SLOT_ACTIVE_BG: Hsla = Hsla { h: 0.60, s: 0.35, l: 0.25, a: 1.0 }; // Fondo de slot activo

// =========================================================================
// CONVERSORES RGB / RGBA
// =========================================================================

pub fn rgb(r: u8, g: u8, b: u8) -> Hsla {
    let rf = r as f32 / 255.0;
    let gf = g as f32 / 255.0;
    let bf = b as f32 / 255.0;
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let l = (max + min) / 2.0;
    let (h, s) = if (max - min).abs() < f32::EPSILON {
        (0.0, 0.0)
    } else {
        let d = max - min;
        let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
        let h = if (max - rf).abs() < f32::EPSILON {
            ((gf - bf) / d + if gf < bf { 6.0 } else { 0.0 }) / 6.0
        } else if (max - gf).abs() < f32::EPSILON {
            ((bf - rf) / d + 2.0) / 6.0
        } else {
            ((rf - gf) / d + 4.0) / 6.0
        };
        (h, s)
    };
    Hsla { h, s, l, a: 1.0 }
}

pub fn rgba(r: u8, g: u8, b: u8, a: f32) -> Hsla {
    let base = rgb(r, g, b);
    Hsla {
        h: base.h,
        s: base.s,
        l: base.l,
        a,
    }
}