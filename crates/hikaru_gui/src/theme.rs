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
// TOKENS DE ALTA DENSIDAD (SESSION MATRIX & ARRANGER)
// =========================================================================

pub const DAW_MAIN_BG: u32        = 0x2B2B2B; // #2b2b2b - Panel / TrackHeader base
pub const DAW_GRID_BG: u32        = 0x1E1E1E; // #1e1e1e - Fondo de grilla oscura
pub const DAW_CELL_EMPTY_BG: u32  = 0x262626; // #262626 - Celda vacía de Clip Launcher
pub const DAW_CELL_HOVER_BG: u32  = 0x303030; // #303030 - Celda en hover
pub const DAW_HEADER_DARK_BG: u32 = 0x222222; // #222222 - Cabeceras de escena / sub-barras
pub const DAW_STATUS_BG: u32      = 0x1A1A1A; // #1a1a1a - Barra de estado inferior
pub const DAW_BORDER_SUBTLE: u32  = 0x141414; // #141414 - Líneas divisorias de 1px
pub const DAW_BORDER_LIGHT: u32   = 0x3A3A3A; // #3a3a3a - Contorno de controles
pub const DAW_ACCENT_ORANGE: u32  = 0xE8782A; // #e8782a - Acento primario (Inst / Activo)
pub const DAW_FADER_FILL: u32     = 0x8A5A2A; // #8a5a2a - Relleno de fader horizontal
pub const DAW_AUDIO_YELLOW: u32   = 0xE8C52A; // #e8c52a - Acento de pista de audio
pub const DAW_PLAYHEAD_BLUE: u32  = 0x3AA8E0; // #3aa8e0 - Cabezal de reproducción / Selección
pub const DAW_SOLO_GOLD: u32      = 0xD9B32A; // #d9b32a - Solo activo
pub const DAW_MUTE_AMBER: u32     = 0xC86428; // #c86428 - Mute activo
pub const DAW_ARM_RED: u32        = 0xD93838; // #d93838 - Record Arm activo

/// Paleta determinista de colores de banda lateral para pistas (TrackHeader / Playlist).
pub const TRACK_STRIPE_PALETTE: [u32; 8] = [
    0xE8782A, // Track 1: Naranja cálido (Instrument)
    0xE8C52A, // Track 2: Amarillo dorado (Audio)
    0x3AA8E0, // Track 3: Azul cian
    0x9B59B6, // Track 4: Púrpura
    0x2ECC71, // Track 5: Esmeralda
    0xE74C3C, // Track 6: Coral/Rojo
    0x1ABC9C, // Track 7: Turquesa
    0xF39C12, // Track 8: Ámbar
];

pub fn track_stripe_hex(track_idx: usize) -> u32 {
    TRACK_STRIPE_PALETTE[track_idx % TRACK_STRIPE_PALETTE.len()]
}

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