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
    /// Metadata del plugin VST3/CLAP cargado en este slot.
    ///
    /// Va aparte de `name` a propósito. `name` es lo que se muestra y se
    /// guarda en el proyecto, y alcanza para un plugin interno
    /// ("OpenWavetable"). Para uno externo hace falta saber **qué binario**
    /// es, porque dos plugins distintos pueden llamarse igual y porque hay que
    /// recargar la ruta si el usuario movió el plugin. `None` = slot vacío o
    /// plugin interno.
    pub external: Option<hikaru_plugin_host::PluginDescriptor>,
    /// Error de la última carga, para mostrarlo en la tarjeta sin romper nada.
    ///
    /// El objetivo es que un plugin roto se vea marcado en el rack y no se
    /// lleve por delante la app: la carga devuelve `Result`, y este campo es
    /// dónde aterriza el mensaje.
    pub load_error: Option<String>,
    /// Caché de parámetros del plugin externo (knobs del rack).
    ///
    /// Se llena en diferido: asignar sólo guarda el descriptor, y un sondeo
    /// en hilo aparte instancia el plugin, lee `snapshot_params` y lo vuelca
    /// acá vía `AppState::drain_param_probes`. `value` lo escriben los gestos
    /// del rack y el feedback del editor nativo; `dirty` marca lo que el
    /// motor aún no aplicó a su instancia de audio
    /// (`take_dirty_external_params`).
    pub external_params: Vec<ExternalParamState>,
    /// En qué punto está el sondeo de parámetros (ver `AppState`).
    pub params_status: ExternalParamsStatus,
    /// Si el plugin externo de este slot declara editor gráfico.
    ///
    /// Vive acá y no en `descriptor.has_gui` porque el escaneo de disco no
    /// instancia nada (a propósito: ejecutar el `libmain` de un binario
    /// desconocido durante un escaneo cuelga) y por lo tanto deja ese campo en
    /// `false` siempre. El sondeo en diferido sí crea una instancia real y
    /// deja la respuesta honesta acá.
    ///
    /// `None` = todavía no seINSTANCEó (o es un plugin interno).
    pub has_gui: Option<bool>,
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
            external: None,
            load_error: None,
            external_params: Vec::new(),
            params_status: ExternalParamsStatus::Idle,
            has_gui: None,
        }
    }

    /// ¿Es un slot que tiene un editor extendido detrás?
    ///
    /// La decisión de qué panel mostrar la toma el rack, y tiene que estar de
    /// acuerdo con el nombre del plugin: un slot vacío no tiene editor, así que
    /// seleccionarlo no puede abrir un panel en blanco.
    pub fn has_editor(&self) -> bool {
        // Un plugin externo cuenta si declara editor; sin `has_gui` no hay
        // ventana que abrir y no tiene sentido ofrecer el botón.
        if self.external.is_some() {
            return self.external.as_ref().is_some_and(|p| p.has_gui);
        }
        matches!(self.name.as_str(), "OpenWavetable" | "Hikaru OpenDMS")
    }

    /// ¿Es un slot sin nada cargado?
    ///
    /// El nombre por sí solo no alcanza: un proyecto puede traer un slot con
    /// `name = "FabFilter Pro-Q 3"` y el plugin que falta en disco. Eso se ve
    /// como slot vacío con un error, no como un plugin cargado.
    pub fn is_empty_slot(&self) -> bool {
        self.external.is_none() && self.name == EMPTY_SLOT_NAME
    }

    /// Cambios de parámetros que el motor aún no aplicó a su instancia de
    /// audio: `(id, normalizado)`. Los producen los knobs del rack y el
    /// feedback del editor nativo; el hilo de control los drena y los manda
    /// con `hikaru_plugin_host::apply_params_to_instance` (o
    /// `DspPluginNode::sync_params`). Drenar limpia las banderas: llamar dos
    /// veces seguidas sin gestos en medio devuelve vacío la segunda.
    pub fn take_dirty_external_params(&mut self) -> Vec<(u32, f32)> {
        let mut out = Vec::new();
        for param in self.external_params.iter_mut() {
            if param.dirty {
                param.dirty = false;
                out.push((param.info.id, param.value));
            }
        }
        out
    }

    /// Fija un parámetro desde la UI: actualiza la caché y lo marca para el
    /// motor.
    ///
    /// Devuelve el valor **efectivamente aplicado** (ya redondeado por `snap`),
    /// o `None` si el id no existe en este slot **o si el valor ya era ese**.
    ///
    /// Lo segundo importa tanto como lo primero. El drag de un parámetro llega
    /// en un evento por píxel, y al pasar por `snap` la mayoría de los píxeles no
    /// mueven nada: en un parámetro discreto de 8 escalones, ~7 de cada 8
    /// eventos calculan el mismo valor. Si eso contara como "cambió", cada uno de
    /// esos eventos pagaría un `cx.notify()` —que reconstruye la ventana entera,
    /// con el clonado de `track.effects` incluido— y el fader se sentiría
    /// pegajoso. Por eso el llamador puede distinguir "no encontré el id" de
    /// "no pasó nada" y saltarse el trabajo.
    pub fn set_external_param(&mut self, id: u32, value: f32) -> Option<f32> {
        let param = self.external_params.iter_mut().find(|p| p.info.id == id)?;
        let snapped = param.info.snap(value);
        if (param.value - snapped).abs() <= f32::EPSILON {
            return None;
        }
        param.value = snapped;
        param.dirty = true;
        Some(snapped)
    }
}

/// Nombre que el rack usa para un slot sin plugin. Un `const` y no un literal
/// repetido, porque `is_empty_slot` compara contra él y un typo en cualquiera
/// de los dos lados rompería la comparación en silencio.
pub const EMPTY_SLOT_NAME: &str = "Empty Slot";

/// Un parámetro externo cacheado para los knobs del rack.
///
/// La `info` (rango, nombre, escalones) viene del sondeo y no cambia; `value`
/// es la única parte viva. `dirty` es el puente con el motor: lo pone la UI
/// y lo limpia quien drene (`take_dirty_external_params`).
#[derive(Clone, Debug)]
pub struct ExternalParamState {
    pub info: hikaru_plugin_host::PluginParamInfo,
    pub value: f32,
    pub dirty: bool,
}

impl ExternalParamState {
    pub fn display(&self) -> String {
        self.info.display_value(self.value)
    }
}

/// Estado del sondeo de parámetros de un slot externo.
///
/// `Idle` = asignado pero aún no sondeado (el rack pide el sondeo en el
/// primer frame); `Probing` = hilo aparte instanciando el plugin;
/// `Ready` = lista usable (quizá vacía: el plugin no expone nada);
/// `Failed` = el sondeo falló con mensaje para la tarjeta.
#[derive(Clone, Debug, Default)]
pub enum ExternalParamsStatus {
    #[default]
    Idle,
    Probing,
    Ready,
    Failed(String),
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

/// Clona la cadena de DEVICES de una pista para compartirla con otro canal.
///
/// Los slots son valores `Clone` (sin aliasing): la copia es transparente
/// entre canales de OpenLive y OpenStudio porque ambos trabajan sobre el
/// mismo tipo [`Track`]. El canal destino procesa sus clips con la cadena
/// recibida sin más pasos.
pub fn duplicate_device_chain(slots: &[DspSlot]) -> Vec<DspSlot> {
    slots.to_vec()
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
/// 68px para que el strip quepa con margen en viewports reales (~680px de
/// área + fuentes más altas que en headless).
pub const VU_HEIGHT: f32 = 68.0;

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
