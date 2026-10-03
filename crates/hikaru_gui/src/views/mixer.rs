// Hikaru OpenLive / OpenStudio - Mixer model + Channel Strip kit
// GNU AGPLv3
// crates/hikaru_gui/src/views/mixer.rs
//
// Este módulo YA NO es una vista independiente: el dock horizontal del Mixer
// inferior (que duplicaba faders y picos) se eliminó. Ahora contiene:
//
// - El modelo de mezcla (`Track`, `DspSlot`, `SendConnection`).
// - El kit de widgets del Channel Strip que `arranger_view.rs` reutiliza en
//   cada columna de pista (OpenLive y OpenStudio comparten el mismo strip):
//   `vertical_vu_meter`, curva de dB y suavizado de picos.
//
// La vista vive en `crate::views::arranger_view`.

use gpui_kit::*;

// =========================================================================
// MODELO
// =========================================================================

#[derive(Clone, Debug)]
pub struct DspSlot {
    pub id: usize,
    pub name: String,
    /// Si el slot pasa audio.
    pub active: bool,
    /// Si el editor extendido está desplegado en el rack.
    pub is_open: bool,
    /// Estado del sintetizador de Wavetable. Vive anidado y no plano en el slot
    /// porque el editor tiene demasiado estado (osciladores, moduladores,
    /// cámara, envolvente, filtro, FX) como para que una lista plana siga
    /// siendo legible.
    pub wavetable: WavetableEditor,
    pub dms_state: Option<OpenDms>,
    pub menu_open: bool,
    pub options_open: bool,
}

use crate::views::open_dms::OpenDms;
use crate::views::open_wavetable::WavetableEditor;

impl DspSlot {
    pub fn new(id: usize, name: String) -> Self {
        Self {
            id,
            name,
            active: true,
            is_open: false,
            wavetable: WavetableEditor::new(),
            dms_state: None,
            menu_open: false,
            options_open: false,
        }
    }

    /// ¿Es un slot que tiene un editor extendido detrás?
    ///
    /// La decisión de qué panel mostrar la toma el rack, y tiene que estar de
    /// acuerdo con el nombre del plugin: un slot vacío no tiene editor, así que
    /// seleccionarlo no puede abrir un panel en blanco.
    pub fn has_editor(&self) -> bool {
        matches!(self.name.as_str(), "OpenWavetable" | "Hikaru OpenDMS")
    }
}

#[derive(Clone, Debug)]
pub struct SendConnection {
    pub target_id: usize,
    pub amount: f32,
}

#[derive(Clone, Debug)]
pub struct Track {
    pub id: usize,
    pub name: String,
    pub volume: f32,
    pub pan: f32,
    pub pan_mode: crate::app::PanMode,
    pub mute: bool,
    pub solo: bool,
    /// Armado de grabación ([R] del channel strip).
    pub arm: bool,
    pub is_master: bool,
    pub route_destination_id: usize,
    pub sends: Vec<SendConnection>,
    pub effects: Vec<DspSlot>,
    pub matrix_idx: Option<usize>,
}

impl Track {
    pub fn new(id: usize, name: String, is_master: bool) -> Self {
        Self {
            id,
            name,
            volume: 0.75,
            pan: 0.0,
            pan_mode: crate::app::PanMode::Stereo,
            mute: false,
            solo: false,
            arm: false,
            is_master,
            route_destination_id: 0,
            sends: Vec::new(),
            effects: Vec::new(),
            matrix_idx: None,
        }
    }
}

// =========================================================================
// CURVAS DE dB
// =========================================================================

/// Texto del fader: mapea 0..=1 a -60..+6 dB con el 0.75 como 0 dB.
pub fn db_text(volume: f32) -> String {
    let db_val = if volume <= 0.0 {
        -60.0
    } else if volume <= 0.75 {
        -60.0 + (volume / 0.75) * 60.0
    } else {
        ((volume - 0.75) / 0.25) * 6.0
    };
    format!("{:.1} dB", db_val)
}

/// Texto del nivel de señal del VU: 20*log10 con suelo en -60 dB.
pub fn level_db_text(level: f32) -> String {
    let db_val = if level <= 0.0 {
        -60.0
    } else {
        (20.0 * level.log10()).clamp(-60.0, 6.0)
    };
    format!("{:.1}dB", db_val)
}

// =========================================================================
// VU METER VERTICAL (renderizado directo, sin relayout)
// =========================================================================
//
// El medidor es un `canvas` de tamaño FIJO (`VU_WIDTH` x `VU_HEIGHT`): el nivel
// sólo cambia lo que se pinta (quads por textura), nunca el tamaño ni los
// hijos del layout. Así el DOM/GUI no se re-mide a 60 FPS aunque el pico se
// actualice en cada frame. El suavizado ataque/liberación se aplica una vez
// por frame en `AppState::sync_frame` (`smoothed_track_peaks`), no acá.

/// Ancho fijo del VU según blueprint (columna estrecha a la izquierda).
pub const VU_WIDTH: f32 = 14.0;
/// Alto fijo del VU: idéntico al recorrido del fader para lectura paralela.
pub const VU_HEIGHT: f32 = 160.0;

/// Color del relleno según tramo: verde -> ámbar -> rojo de clip.
pub fn vu_fill_color(level: f32) -> Hsla {
    if level > 0.9 {
        rgb(0xFF3C3C).into()
    } else if level > 0.75 {
        rgb(0xFFC800).into()
    } else {
        rgb(0x00FF64).into()
    }
}

/// Suavizado ataque rápido / liberación lenta para los picos atómicos.
///
/// `previous` es el valor ya suavizado del frame anterior y `target` el pico
/// crudo del motor. Sin esto el canvas parpadearía entre 0 y 1 siguiendo el
/// buffer de audio en vez de mostrar una aguja legible.
pub fn smooth_peak(previous: f32, target: f32) -> f32 {
    let prev = if previous.is_finite() { previous } else { 0.0 };
    let tgt = if target.is_finite() {
        target.clamp(0.0, 1.0)
    } else {
        0.0
    };
    if tgt > prev {
        prev + (tgt - prev) * 0.6
    } else {
        prev + (tgt - prev) * 0.12
    }
}

/// VU meter vertical de tamaño fijo pintado directo en canvas.
///
/// Ni el `div` ni el `canvas` cambian de tamaño o hijos con `level`: todo el
/// movimiento ocurre dentro del callback de pintado.
pub fn vertical_vu_meter(level: f32) -> AnyElement {
    let level = if level.is_finite() {
        level.clamp(0.0, 1.0)
    } else {
        0.0
    };
    div()
        .w(px(VU_WIDTH))
        .h(px(VU_HEIGHT))
        .flex_shrink_0()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    // Fondo: siempre el mismo quad, mismo costo.
                    window.paint_quad(PaintQuad {
                        bounds,
                        background: rgb(0x0A0A0A).into(),
                        border_color: Hsla::default(),
                        corner_radii: gpui_kit::Corners::default(),
                        border_widths: gpui_kit::Edges::default(),
                        border_style: BorderStyle::default(),
                    });
                    if level <= 0.001 {
                        return;
                    }
                    let fw = bounds.size.width;
                    let fh = bounds.size.height;
                    let sig_h = fh * level;
                    // Relleno desde abajo: un solo quad por frame.
                    window.paint_quad(PaintQuad {
                        bounds: Bounds::new(
                            point(bounds.origin.x, bounds.origin.y + fh - sig_h),
                            size(fw, sig_h),
                        ),
                        background: vu_fill_color(level).into(),
                        border_color: Hsla::default(),
                        corner_radii: gpui_kit::Corners::default(),
                        border_widths: gpui_kit::Edges::default(),
                        border_style: BorderStyle::default(),
                    });
                    // Filo superior brillante: marca el pico instantáneo.
                    window.paint_quad(PaintQuad {
                        bounds: Bounds::new(
                            point(bounds.origin.x, bounds.origin.y + fh - sig_h),
                            size(fw, px(2.0)),
                        ),
                        background: rgb(0xFFFFFF).into(),
                        border_color: Hsla::default(),
                        corner_radii: gpui_kit::Corners::default(),
                        border_widths: gpui_kit::Edges::default(),
                        border_style: BorderStyle::default(),
                    });
                },
            )
            .w(px(VU_WIDTH))
            .h(px(VU_HEIGHT)),
        )
        .into_any_element()
}
