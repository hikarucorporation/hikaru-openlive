// Hikaru OpenLive - Theme (egui)
// GNU AGPLv3
// crates/hikaru_gui/src/theme.rs

use egui::Color32;

pub const fn hex(v: u32) -> Color32 {
    let r = ((v >> 16) & 0xFF) as u8;
    let g = ((v >> 8) & 0xFF) as u8;
    let b = (v & 0xFF) as u8;
    Color32::from_rgb(r, g, b)
}

pub const fn hex_to_color32(v: u32) -> Color32 {
    hex(v)
}

pub fn hsla_to_color32(h: f32, s: f32, l: f32, a: f32) -> Color32 {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = if h < 0.0 { (h % 360.0 + 360.0) / 60.0 } else { (h % 360.0) / 60.0 };
    let x = c * (1.0 - ((hp % 2.0) - 1.0).abs());
    let (r1, g1, b1) = if hp < 1.0 {
        (c, x, 0.0)
    } else if hp < 2.0 {
        (x, c, 0.0)
    } else if hp < 3.0 {
        (0.0, c, x)
    } else if hp < 4.0 {
        (0.0, x, c)
    } else if hp < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    let m = l - c / 2.0;
    Color32::from_rgba_unmultiplied(
        ((r1 + m).clamp(0.0, 1.0) * 255.0) as u8,
        ((g1 + m).clamp(0.0, 1.0) * 255.0) as u8,
        ((b1 + m).clamp(0.0, 1.0) * 255.0) as u8,
        (a.clamp(0.0, 1.0) * 255.0) as u8,
    )
}

pub fn color32_to_hsla(color: Color32) -> (f32, f32, f32, f32) {
    let r = color.r() as f32 / 255.0;
    let g = color.g() as f32 / 255.0;
    let b = color.b() as f32 / 255.0;
    let a = color.a() as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let l = (max + min) / 2.0;
    if delta == 0.0 {
        return (0.0, 0.0, l, a);
    }
    let s = if l > 0.5 { delta / (2.0 - max - min) } else { delta / (max + min) };
    let h = if max == r {
        let mut segment = (g - b) / delta;
        if segment < 0.0 { segment += 6.0; }
        segment * 60.0
    } else if max == g {
        (((b - r) / delta) + 2.0) * 60.0
    } else {
        (((r - g) / delta) + 4.0) * 60.0
    };
    (h, s, l, a)
}

// =========================================================================
// PALETA BASE: ABLETON / BITWIG DARK THEME
// =========================================================================

// Fondos y Superficies
pub const WINDOW_BG: Color32 = hex(0x121212);    // #121212 (Fondo principal Bitwig)
pub const PANEL_BG: Color32  = hex(0x1F1F1F);    // #1f1f1f (Contenedores)
pub const HEADER_BG: Color32 = hex(0x1A1A1A);    // #1a1a1a (Barra superior)
pub const SURFACE_BG: Color32 = hex(0x292929);   // #292929 (Tarjetas y elementos)

// Estados de Interacción (Botones / Items)
pub const HOVER_BG: Color32   = hex(0x383838);   // Highlight al pasar el mouse
pub const ACTIVE_BG: Color32  = hex(0x474747);   // Clic / Presionado
pub const SELECTED_BG: Color32 = Color32::from_rgb(41, 64, 86); // Selección activa de track/clip

// Bordes y Divisores
pub const BORDER_COLOR: Color32 = hex(0x2E2E2E); // Separadores sutiles
pub const BORDER_FOCUS: Color32 = hex(0xFF7A00); // Borde activo al seleccionar (#ff7a00)

// Texto
pub const TEXT_PRIMARY: Color32   = hex(0xE0E0E0); // Blanco/Gris claro (#e0e0e0)
pub const TEXT_MUTED: Color32     = hex(0x8C8C8C); // Gris medio para labels (#8c8c8c)
pub const TEXT_SECONDARY: Color32 = hex(0xAAAAAA);
pub const TEXT_DISABLED: Color32  = hex(0x595959); // Elementos desactivados

// Acentos de DAW (Play, Record, Solo, Mute, Metrónomo)
pub const PLAY_GREEN: Color32  = hex(0x1FEB54); // Play / Activo (#1feb54)
pub const REC_RED: Color32     = hex(0xF02A2A); // Grabación (#f02a2a)
pub const ACCENT_ORANGE: Color32 = hex(0xFF7A00); // Bitwig Orange (#ff7a00)
pub const ACCENT_PRIMARY: Color32 = hex(0xFF7A00);
pub const ACCENT_HOVER: Color32   = hex(0xFFA040);
pub const ACCENT_SECONDARY: Color32 = hex(0x1E88E5);
pub const SOLO_YELLOW: Color32 = hex(0xF2CA19); // Solo track
pub const MUTE_BLUE: Color32   = hex(0x1990E6); // Mute track
pub const ACCENT_COLOR: Color32 = hex(0xFF7A00); // Acento primario (naranja #ff7a00)
pub const ACCENT_STUDIO: Color32 = hex(0x1E88E5); // Acento studio (azul)
pub const SLOT_ACTIVE_BG: Color32 = Color32::from_rgb(41, 64, 86); // Fondo de slot activo
pub const PLAYHEAD_COLOR: Color32 = hex(0x3AA8E0);
pub const PLAYHEAD_GLOW: Color32  = hex(0x3AA8E0);

pub const DAW_BG_LAYER_1: Color32 = hex(0x121212);
pub const DAW_BG_LAYER_2: Color32 = hex(0x1F1F1F);
pub const DAW_BG_LAYER_3: Color32 = hex(0x292929);
pub const DAW_PANEL_BG: Color32   = hex(0x1F1F1F);
pub const DAW_CANVAS_VOID: Color32 = hex(0x121212);
pub const DAW_RULER_BG: Color32   = hex(0x232325);
pub const DAW_BORDER_SUBTLE: Color32 = hex(0x2E2E2E);
pub const DAW_GRID_MAJOR: Color32 = hex(0x2E2E2E);
pub const DAW_GRID_MINOR: Color32 = hex(0x1F1F1F);

// =========================================================================
// TOKENS DE ALTA DENSIDAD (SESSION MATRIX & ARRANGER)
// =========================================================================

pub const BITWIG_BG_DARK: Color32     = hex(0x121212); // #121212 - Bitwig Canvas Void / Dark base
pub const BITWIG_ORANGE: Color32      = hex(0xFF7A00); // #ff7a00 - Bitwig Signature Orange
pub const DAW_MAIN_BG: Color32        = hex(0x2B2B2B); // #2b2b2b - Panel / TrackHeader base
pub const DAW_GRID_BG: Color32        = hex(0x1E1E1E); // #1e1e1e - Fondo de grilla oscura
pub const DAW_LANE_EVEN_BG: Color32   = hex(0x28282A); // #28282a - Carril horizontal par en Arranger
pub const DAW_LANE_ODD_BG: Color32    = hex(0x242426); // #242426 - Carril horizontal impar en Arranger
pub const DAW_LANE_SELECTED_BG: Color32 = hex(0x2F333B); // #2f333b - Carril horizontal seleccionado
pub const DAW_HEADER_SELECTED_BG: Color32 = hex(0x393C44); // #393c44 - TrackHeader seleccionado
pub const DAW_LOOP_STRIP_BG: Color32  = hex(0x19191B); // #19191b - Carril de región de loop bajo regla
pub const DAW_LOOP_ACTIVE_BAR: Color32 = hex(0xA2A8B0); // #a2a8b0 - Barra de loop activa de alto contraste
pub const DAW_LOOP_INACTIVE_BAR: Color32 = hex(0x4A4E54); // #4a4e54 - Barra de loop desactivada
pub const DAW_CELL_EMPTY_BG: Color32  = hex(0x262626); // #262626 - Celda vacía de Clip Launcher
pub const DAW_CELL_HOVER_BG: Color32  = hex(0x303030); // #303030 - Celda en hover
pub const DAW_HEADER_DARK_BG: Color32 = hex(0x222222); // #222222 - Cabeceras de escena / sub-barras
pub const DAW_STATUS_BG: Color32      = hex(0x1A1A1A); // #1a1a1a - Barra de estado inferior
pub const DAW_BORDER_LIGHT: Color32   = hex(0x3A3A3A); // #3a3a3a - Contorno de controles
pub const DAW_ACCENT_ORANGE: Color32  = hex(0xFF7A00); // #ff7a00 - Acento primario Bitwig (Inst / Activo)
pub const DAW_FADER_WELL_BG: Color32  = hex(0x161618); // #161618 - Pozo empotrado de fader y vúmetro
pub const DAW_FADER_FILL: Color32     = hex(0x8A5A2A); // #8a5a2a - Relleno ámbar de fader
pub const DAW_FADER_CAP_ORANGE: Color32 = hex(0xFF7A00); // #ff7a00 - Indicador brillante de fader Bitwig
pub const DAW_MASTER_SLATE: Color32   = hex(0x8A8E94); // #8a8e94 - Cabezal neutro anodizado (Master / FX)
pub const DAW_CLIP_BODY_BG: Color32   = hex(0x15171B); // #15171b - Cámara oscura de waveform en clip
pub const DAW_CLIP_SELECTED_BG: Color32 = hex(0x1C2530); // #1c2530 - Cámara oscura de clip seleccionado
pub const DAW_AUDIO_YELLOW: Color32   = hex(0xE8C52A); // #e8c52a - Acento de pista de audio
pub const DAW_PLAYHEAD_BLUE: Color32  = hex(0x3AA8E0); // #3aa8e0 - Cabezal de reproducción / Selección
pub const DAW_SOLO_GOLD: Color32      = hex(0xD9B32A); // #d9b32a - Solo activo
pub const DAW_MUTE_AMBER: Color32     = hex(0xC86428); // #c86428 - Mute activo
pub const DAW_ARM_RED: Color32        = hex(0xD93838); // #d93838 - Record Arm activo

// Bitwig Studio 5.1.7 GPU SDF tokens re-exported in `theme` for workspace views
pub const BW_DARK_WELL: Color32         = Color32::from_rgb(0x0A, 0x0B, 0x0D);
pub const BW_SUNKEN_DARK: Color32       = Color32::from_rgb(0x13, 0x14, 0x17);
pub const BW_CAPSULE_BORDER: Color32    = Color32::from_rgb(0x14, 0x15, 0x18);
pub const BW_CAPSULE_HOVER_TOP: Color32 = Color32::from_rgb(0x64, 0x67, 0x6F);
pub const BW_CAPSULE_HOVER_BOT: Color32 = Color32::from_rgb(0x52, 0x55, 0x5C);
pub const BW_FADER_AMBER: Color32       = Color32::from_rgb(138, 78, 24);
pub const BW_FADER_ORANGE: Color32      = Color32::from_rgb(255, 124, 16);
pub const BW_LCD_CYAN: Color32          = Color32::from_rgb(68, 184, 255);

/// Paleta determinista de colores de banda lateral para pistas (TrackHeader / Playlist / Mix).
/// Calibrada 1:1 contra Bitwig Studio 5.1.7 (`Inst 1` #FF5500, `Audio 2` #E5AE19, etc.).
pub const TRACK_STRIPE_PALETTE: [u32; 8] = [
    0xFF5500, // Track 1: Bitwig Signature Vivid Orange (Inst 1)
    0xE5AE19, // Track 2: Bitwig Warm Gold (Audio 2)
    0x3AA8E0, // Track 3: Bitwig Sky Cyan
    0xA35FE6, // Track 4: Bitwig Violet
    0x34C769, // Track 5: Bitwig Mint Green
    0xF04747, // Track 6: Bitwig Coral Red
    0x22C4B8, // Track 7: Bitwig Teal
    0xF28C28, // Track 8: Bitwig Tangerine
];

pub fn track_stripe_hex(track_idx: usize) -> u32 {
    TRACK_STRIPE_PALETTE[track_idx % TRACK_STRIPE_PALETTE.len()]
}

pub fn track_stripe_color(track_idx: usize) -> Color32 {
    hex(track_stripe_hex(track_idx))
}

// =========================================================================
// CONVERSORES RGB / RGBA
// =========================================================================

pub fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color32 {
    let alpha_u8 = (a.clamp(0.0, 1.0) * 255.0) as u8;
    Color32::from_rgba_unmultiplied(r, g, b, alpha_u8)
}

/// Aplica el tema visual oscuro Bitwig Studio 5.1.7 + tipografía Barlow a egui.
pub fn apply_theme(ctx: &egui::Context) {
    crate::ui::fonts::ensure_installed(ctx);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(4.0, 4.0);
    style.spacing.button_padding = egui::vec2(6.0, 3.0);
    style.spacing.scroll.bar_width = 7.0;
    style.spacing.scroll.bar_inner_margin = 1.0;
    style.spacing.scroll.bar_outer_margin = 1.0;

    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(TEXT_PRIMARY);
    visuals.panel_fill = hex(0x2E2F33);
    visuals.window_fill = hex(0x1E1F22);
    visuals.extreme_bg_color = hex(0x0B0C0E);
    visuals.faint_bg_color = hex(0x25262A);

    let r2 = egui::CornerRadius::same(2);
    visuals.window_corner_radius = egui::CornerRadius::same(4);
    visuals.menu_corner_radius = egui::CornerRadius::same(3);

    visuals.widgets.noninteractive.bg_fill = hex(0x26272B);
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, hex(0x16171A));
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.noninteractive.corner_radius = r2;

    visuals.widgets.inactive.bg_fill = hex(0x46484E);
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, hex(0x18191C));
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.inactive.corner_radius = r2;

    visuals.widgets.hovered.bg_fill = hex(0x565860);
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, BORDER_FOCUS);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, Color32::WHITE);
    visuals.widgets.hovered.corner_radius = r2;

    visuals.widgets.active.bg_fill = hex(0xFF7200);
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, ACCENT_COLOR);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, Color32::from_rgb(16, 16, 18));
    visuals.widgets.active.corner_radius = r2;

    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(255, 114, 0, 72);
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, ACCENT_COLOR);

    style.visuals = visuals;
    ctx.set_style(style);
}
