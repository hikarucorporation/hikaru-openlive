// crates/hikaru_gui/src/views/open_wavetable.rs

//! Editor del sintetizador de Wavetable, en tira horizontal.
//!
//! # La forma
//!
//! Es un panel bajo y ancho, no un rack de perillas: el mismo criterio que el
//! `DSP Rack`, y por la misma razón. La primera versión de este editor era un
//! panel de 460px con columnas de perillas, envolvente y moduladores, y lo que
//! ganaba en controles lo perdía en la única cosa que importa cuando se está
//! programando un sonido: **mirar la forma de onda**. Acá la pila 3D se lleva
//! el centro del panel y todo lo demás lo rodea en
//! [`EDITOR_HEIGHT`] de alto.
//!
//! ```
//! ┌────────────────────────────┐
//! │ OpenWavetable       64 cic │
//! │ [WAVETABLE ▾] Bender      │
//! │ ┌────────────────────────┐ │
//! │ │      ╱▔▔▔╲            │ │
//! │ │     ╱  pila 3D  ╲      │ │
//! │ └────────────────────────┘ │
//! │ (◉)WT POS     7 / 64  < >  │
//! │            ▁▂▃▅ espectro  │
//! └────────────────────────────┘
//! ```
//!
//! # La pila de ciclos
//!
//! El visor no muestra un ciclo suelto: muestra la **tabla entera**, con un
//! ciclo por fila de la pila en el eje Z, que es la vista de Serum, Vital y
//! Bitwig. Los ciclos salen de [`wavetable_io`], que parte el `.wav` en bloques
//! de [`wavetable_io::FRAME_SAMPLES`] muestras, y el que está bajo el knob de
//! índice queda encendido. Por eso el knob de morph recorre la matriz y no
//! interpola una forma nueva: mueve el realce a lo largo de la profundidad.
//!
//! # Los dos knobs
//!
//! Los dos son geometría 3D de `hikaru_render` ([`hikaru_render::knob`]), no
//! diales pintados: son discos con relieve que giran con el valor, y la marca
//! sube con él. Un dial 2D comunica el número; uno con volumen comunica el gesto.
//!
//! - **MORPH**: posición continua dentro de la tabla. Recorre la matriz de
//!   ciclos y el realce de la pila lo sigue de forma continua.
//! - **INDEX**: qué frame se está mirando, de `1` a `frames`.
//!
//! Con una wavetable de un solo ciclo (que es lo que se descarga de internet en
//! la mayoría de los casos) `INDEX` queda en 1/1 y no hace nada: no hay frames
//! que recorrer. Es el comportamiento honesto, y en cuanto se carga un archivo
//! de varios ciclos el knob cobra vida.
//!
//! # El knob `WT POS`
//!
//! Es el control que recorre la matriz de ciclos, y el único que mueve el
//! realce de la pila. Se arrastra en vertical: [`DRAG_RANGE_PX`] píxeles
//! recorren la tabla entera, y con Shift apretado el recorrido es un
//! [`FINE_TUNE_FACTOR`] de eso.
//!
//! El arrastre es relativo al punto de agarre, no absoluto: ver
//! [`WavetableEditor::begin_drag`]. Y el knob se apaga visiblemente cuando la
//! tabla tiene un solo ciclo, porque no hay a dónde ir.
//!
//! # Dónde se cargan las wavetables
//!
//! El rótulo `WAVETABLE ▾` de la izquierda abre el menú de la tabla. Desde ahí
//! se puede ir al explorador, que ya sabe navegar los directorios de Linux, o
//! usar el diálogo nativo del sistema. La lectura del archivo está en
//! [`crate::views::wavetable_io`].

use std::path::PathBuf;

use gpui_kit::component::*;
use gpui_kit::component::button::{Button, ButtonRounded};
use gpui_kit::component::label::Label;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, Styled as _};
use gpui_kit::*;
use hikaru_render::{Camera, RenderMode, RenderSettings};

use crate::app::{state, HikaruApp};
use crate::render::{
    ViewerRequest, WavetableView, WavetableViewRequest, WavetableViewportHandle,
    WAVETABLE_VIEWPORT,
};
use crate::views::wavetable_io::{self, Wavetable};

/// Lado del módulo, en píxeles.
///
/// El editor es un **bloque cuadrado**, no una banda. Antes ocupaba el ancho
/// completo del rack con 132 de alto, y en una pantalla ancha eso era un rectángulo
/// de 1600x132 con un visor angosto en el medio y el resto en negro: mucho ancho
/// para una sola forma de onda.
///
/// Cuadrado y con ancho fijo hace dos cosas: entra en la cadena de módulos del
/// rack al lado de los demás sin empujar al que sigue, y el visor queda cerca de
/// cuadrado, que es el aspecto que hace legible la pila de ciclos.
pub const MODULE_SIZE: f32 = 220.0;

/// Ancho del módulo. Igual a [`MODULE_SIZE`]; existe con nombre propio para que
/// quien lo use se pregunte por qué un cuadrado tiene ancho.
pub const EDITOR_WIDTH: f32 = MODULE_SIZE;

/// Alto del módulo. Lo lee `dsp_rack` para calcular el alto del rack, así que
/// tiene que ser el valor real del `div`, no una estimación.
pub const EDITOR_HEIGHT: f32 = MODULE_SIZE;

/// Padding interno del módulo.
const MODULE_PADDING: f32 = 5.0;

/// Separación entre las cuatro bandas verticales: header, selector, visor y pie.
const MODULE_GAP: f32 = 5.0;

/// Alto de la fila del header con el nombre del plugin.
const HEADER_HEIGHT: f32 = 14.0;

/// Alto de la fila del selector de wavetable.
const SELECTOR_HEIGHT: f32 = 22.0;

/// Alto del pie: el knob, el contador de ciclos y los botones de paso.
const FOOTER_HEIGHT: f32 = 44.0;

/// Lado del knob en pantalla. El target offscreen es de 96px y el `img` escala
/// con `Contain`, así que el knob se ve con la resolución de la pantalla.
///
/// Va en el pie, no sobre el visor: como elemento de la cadena ocupa su lugar en
/// el layout, y el visor queda libre entero para la pila de ciclos.
///
/// 48px es lo que se puede agarrar con un mouse sin que el click sea un tiro al
/// blanco. Con 42 (el valor chico de la versión comprimida) el área efectiva de
/// clickeo se vuelve veterinaria; el fine-tune no lo arregla porque depende de
/// [`DRAG_RANGE_PX`], no del tamaño del disco.
const KNOB_SIZE: f32 = 36.0;

/// Caja de la malla 3D.
///
/// # Por qué `thickness` tiene que ser grande respecto de `height`
///
/// La cámara mira la pila con 30° de inclinación, así que el apilado en Z se
/// proyecta en pantalla a `thickness × sin(30°)`, o sea la mitad. Para que las
/// láminas no se tapen entre ellas hace falta que esa proyección iguale o supere
/// el alto de una:
///
/// ```text
/// thickness × sin(30°) ≥ height   →   thickness ≥ height × 2
/// ```
///
/// Con `thickness = 80` y `height = 100` eso da 40 contra 100: cada lámina
/// tapaba a las dos y media vecinas y la pila se veía como un enredo de líneas
/// cruzadas en vez de ciclos ordenados. 240 da 120 contra 100, con aire.
///
/// # Por qué `height` bajó a 70
///
/// El espesor de cada cinta es `thickness / count × RIBBON_FILL`. Con 100 de
/// alto y una cinta de 18.9 de espesor, la relación era 0.19: vistas a 30° las
/// cintas se proyectaban como astillas y la pila se leía como un manojo de
/// palitos. Bajar el alto deja la onda proporcional al grosor de su banda.
const MESH_WIDTH: f32 = 260.0;
const MESH_HEIGHT: f32 = 96.0;
const MESH_THICKNESS: f32 = 240.0;

/// Ciclos que se dibujan como máximo en la pila.
///
/// 24 es el punto en el que un ciclo deja de ocupar un píxel de ancho en el
/// visor. Más que eso es geometría que no se ve: el `WAVETABLE_MAX_FRAMES` de la
/// lectura pone el tope de 64 ciclos en memoria, y acá se elige cuántos de esos
/// se dibujan.
const MAX_STACK_FRAMES: usize = 24;

/// Frames máximos que se leen de un archivo.
///
/// Un `.wav` de dos minutos son 2000 ciclos: 16 MB por slot del rack. Con 64
/// alcanza para cualquier tabla de verdad y el archivo se lee entero en
/// memoria una vez.
pub(crate) const MAX_FRAMES: usize = 64;

pub fn load_table_file(
    path: &std::path::Path,
) -> Result<crate::views::wavetable_io::Wavetable, crate::views::wavetable_io::WavetableError> {
    crate::views::wavetable_io::load_wavetable(path, MAX_FRAMES)
}

/// Colores de los knobs, en el espacio que espera el shader (lineal 0..1).
const KNOB_BODY: [f32; 4] = [0.13, 0.14, 0.17, 1.0];
const KNOB_MARKER: [f32; 4] = [1.0, 0.43, 0.0, 1.0];

/// Color de la cinta: el mismo naranja de la marca de selección del rack, para
/// que el visor se lea como parte del plugin y no como un recuadro suelto.
const VIEWER_TINT: [f32; 4] = [1.0, 0.55, 0.15, 1.0];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KnobTarget {
    #[default]
    Position,
    Unison,
    Detune,
    Phase,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VoiceMode {
    Mono,
    Legato,
    #[default]
    Poly,
}

impl VoiceMode {
    pub fn label(self) -> &'static str {
        match self {
            VoiceMode::Mono => "Mono",
            VoiceMode::Legato => "Legato",
            VoiceMode::Poly => "Poly",
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            VoiceMode::Mono => VoiceMode::Legato,
            VoiceMode::Legato => VoiceMode::Poly,
            VoiceMode::Poly => VoiceMode::Mono,
        }
    }
}

/// Estado del editor de un slot de Wavetable.
#[derive(Clone, Debug)]
pub struct WavetableEditor {
    /// La tabla cargada. Es la fuente de verdad de la forma de onda: el visor
    /// 3D dibuja de acá y no de un waveform sintetizado.
    pub table: Wavetable,
    /// Posición continua dentro de la tabla, 0..1.
    pub morph: f32,
    /// Frame que se está mirando, como índice. El knob lo muestra 1-based.
    pub frame: usize,
    /// Si el morph interpola entre frames o salta de uno en uno.
    pub smooth: bool,
    /// Modo del visor: terreno 3D o ciclo 2D de diagnóstico. El 2D muestra el
    /// ciclo activo interpolado plano de frente, para verificar la lectura del
    /// `.wav` antes de la proyección 3D.
    pub render_mode: RenderMode,
    /// Ajustes de look del panel "Hikaru OpenWavetable Settings". Viajan al
    /// renderer en cada pedido y entran en la clave de caché.
    pub render_settings: RenderSettings,
    /// Si el panel de settings está abierto.
    pub settings_open: bool,
    /// Arrastre en curso sobre un slider del panel, si lo hay. Igual que el
    /// del knob pero en horizontal: relativo al punto de agarre.
    setting_drag: Option<SettingDrag>,
    /// Cámara del visor 3D.
    pub camera: Camera,
    pub unison: u8,
    pub detune: f32,
    pub phase: f32,
    pub pitch: i8,
    pub octave: i8,
    pub voices: VoiceMode,
    pub drag_target: KnobTarget,
    /// Si el selector de wavetables está abierto.
    pub menu_open: bool,
    /// Si el botón izquierdo está apretado sobre el knob.
    ///
    /// Es lo que hace que el `on_mouse_move` del módulo no mueva el knob cuando
    /// el cursor pasa por encima: sin esta guarda, cruzar el módulo con el ratón
    /// de mover el WT POS.
    pub dragging: bool,
    /// Posición normalizada del knob en el instante del `mouse down`.
    ///
    /// El arrastre es **relativo** a este valor, no absoluto. Es la diferencia
    /// entre un knob usable y uno que salta: con arrastre absoluto, agarrar el
    /// knob en cualquier parte del recorrido lo teletransporta a ese punto, y
    /// un toque de 2 píxeles con un error de 1 píxel ya mueve 3 ciclos.
    drag_grab: f32,
    /// coordenada Y de la ventana donde se apretó el botón, en píxeles.
    ///
    /// En coordenadas de ventana y no del elemento: el `on_mouse_move` que
    /// calcula el delta está en el módulo, que puede estar en otra posición
    /// dentro de la ventana si el layout se mueve durante el arrastre.
    drag_origin_y: f32,
}

impl Default for WavetableEditor {
    fn default() -> Self {
        Self {
            // La wavetable con la que arranca el slot: el `.wav` embebido en el
            // binario, no la tabla sintetizada. Si el embebido no se puede
            // leer se cae a la sintetizada, porque arrancar sin wavetable es
            // peor que arrancar con otra.
            table: wavetable_io::bundled_wavetable(MAX_FRAMES)
                .unwrap_or_else(|error| {
                    eprintln!(
                        "[ Hikaru OpenLive ] : no se pudo leer la wavetable embebida, se usa la sintetizada | {}",
                        error
                    );
                    Wavetable::default_table()
                }),
            morph: 0.0,
            frame: 0,
            smooth: true,
            render_mode: RenderMode::default(),
            render_settings: RenderSettings::default(),
            settings_open: false,
            setting_drag: None,
            camera: Camera::default(),
            unison: 1,
            detune: 0.0,
            phase: 0.0,
            pitch: 0,
            octave: 0,
            voices: VoiceMode::Poly,
            drag_target: KnobTarget::Position,
            menu_open: false,
            dragging: false,
            drag_grab: 0.0,
            drag_origin_y: 0.0,
        }
    }
}

impl WavetableEditor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Un editor con la tabla dada, en la posición inicial.
    ///
    /// Existe para no exponer los campos privados de arrastre: armar el struct
    /// literal desde afuera obligaría a que `drag_grab` y `drag_origin_y` fueran
    /// `pub`, que es estado de una interacción en curso y no de la tabla.
    pub fn with_table(table: Wavetable) -> Self {
        Self { table, ..Self::default() }
    }

    /// Posición normalizada en la matriz, 0..1.
    ///
    /// Es lo que muestran la rotación del knob y la malla 3D a la vez, y por eso
    /// las dos cosas no pueden desincronizarse: leen la misma función.
    ///
    /// Antes el knob mostraba `morph`, que es sólo la fracción entre dos ciclos,
    /// y no la posición absoluta. Con 64 ciclos, arrastrar el knob de abajo
    /// arriba movía la fracción entre el frame 0 y el 1 y se recorrían 64 ciclos
    /// en la primera vuelta para después no volver a moverse: el knob no
    /// representaba lo que estaba haciendo.
    pub fn position(&self) -> f32 {
        let frames = self.table.selectable_frames();
        if frames <= 1 {
            return 0.0;
        }
        (self.active_cycle() / (frames - 1) as f32).clamp(0.0, 1.0)
    }

    /// Fija la posición normalizada en la matriz, repartiendo entre `frame` y
    /// `morph`.
    ///
    /// Se guardan las dos partes porque son las que ya usan el resto del editor:
    /// el contador del pie muestra `frame` y la malla interpola con `morph`.
    pub fn set_position(&mut self, position: f32) {
        let frames = self.table.selectable_frames();
        if frames <= 1 {
            self.frame = 0;
            self.morph = 0.0;
            return;
        }

        let last = (frames - 1) as f32;
        // Un `NaN` no se puede acotar con `clamp`: todas las comparaciones con
        // `NaN` dan false y el valor se propaga al uniforme de la malla.
        let position = if position.is_finite() { position.clamp(0.0, 1.0) } else { 0.0 };
        let cycle = (position * last).clamp(0.0, last);

        self.frame = cycle.floor() as usize;
        // Con `floor`, la fracción nunca llega a 1.0, así que `morph` queda en
        // [0, 1) y no se solapa con el `frame` siguiente.
        self.morph = cycle - self.frame as f32;
    }

    /// Comienza un arrastre en `window_y`.
    pub fn begin_drag(&mut self, window_y: f32) {
        self.begin_drag_target(KnobTarget::Position, window_y);
    }

    pub fn begin_drag_target(&mut self, target: KnobTarget, window_y: f32) {
        self.drag_target = target;
        self.dragging = true;
        self.drag_grab = match target {
            KnobTarget::Position => self.position(),
            KnobTarget::Unison => self.norm_unison(),
            KnobTarget::Detune => self.detune,
            KnobTarget::Phase => self.phase,
        };
        self.drag_origin_y = if window_y.is_finite() { window_y } else { 0.0 };
    }

    /// Continúa un arrastre que está en `window_y` y devuelve la posición nueva.
    ///
    /// Devolver el valor evita tener que leer el estado otra vez desde el
    /// handler, que ya tiene el lock tomado.
    pub fn drag_to(&mut self, window_y: f32, fine: bool) -> f32 {
        if !self.dragging || !window_y.is_finite() {
            return self.position();
        }

        // Hacia arriba sube el valor, que es la convención de todos los faders
        // verticales: se arrastra hacia donde uno quiere que vaya.
        let delta_px = self.drag_origin_y - window_y;
        let mut value = self.drag_grab + delta_px / DRAG_RANGE_PX * value_gain(fine);

        if !value.is_finite() {
            value = self.drag_grab;
        }
        match self.drag_target {
            KnobTarget::Position => {
                self.set_position(value);
                self.position()
            }
            KnobTarget::Unison => {
                self.set_unison(1 + (value.clamp(0.0, 1.0) * 15.0).round() as u8);
                self.norm_unison()
            }
            KnobTarget::Detune => {
                self.detune = value.clamp(0.0, 1.0);
                self.detune
            }
            KnobTarget::Phase => {
                self.phase = value.clamp(0.0, 1.0);
                self.phase
            }
        }
    }

    pub fn norm_unison(&self) -> f32 {
        (self.unison.clamp(1, 16) as f32 - 1.0) / 15.0
    }

    pub fn set_unison(&mut self, voices: u8) {
        self.unison = voices.clamp(1, 16);
    }

    pub fn step_unison(&mut self, delta: i32) {
        self.set_unison((self.unison as i32 + delta).clamp(1, 16) as u8);
    }

    pub fn step_pitch(&mut self, delta: i32) {
        self.pitch = (self.pitch as i32 + delta).clamp(-24, 24) as i8;
    }

    pub fn step_octave(&mut self, delta: i32) {
        self.octave = (self.octave as i32 + delta).clamp(-3, 3) as i8;
    }

    pub fn cycle_voices(&mut self) {
        self.voices = self.voices.cycle();
    }

    /// Termina el arrastre (del knob o de un slider del panel).
    pub fn end_drag(&mut self) {
        self.dragging = false;
        self.setting_drag = None;
    }

    /// Comienza el arrastre de un slider del panel en `window_x`.
    pub fn begin_setting_drag(&mut self, param: SettingParam, window_x: f32) {
        self.dragging = false;
        self.setting_drag = Some(SettingDrag {
            param,
            grab: param.get(&self.render_settings),
            origin_x: if window_x.is_finite() { window_x } else { 0.0 },
        });
    }

    /// Continúa el arrastre del slider y devuelve el valor nuevo.
    ///
    /// Horizontal y relativo al agarre, como el knob pero en X: hacia la
    /// derecha sube el valor. Con Shift el recorrido es un
    /// [`FINE_TUNE_FACTOR`] de eso.
    pub fn drag_setting_to(&mut self, window_x: f32, fine: bool) -> f32 {
        let Some(drag) = self.setting_drag else {
            return 0.0;
        };
        if !window_x.is_finite() {
            return drag.grab;
        }
        let (min, max) = drag.param.range();
        let mut value =
            drag.grab + (window_x - drag.origin_x) / SETTING_DRAG_PX * (max - min) * value_gain(fine);
        if !value.is_finite() {
            value = drag.grab;
        }
        drag.param.set(&mut self.render_settings, value);
        drag.param.get(&self.render_settings)
    }

    /// Valor actual del ajuste bajo arrastre, si hay uno en curso.
    pub fn setting_drag_value(&self) -> Option<f32> {
        self.setting_drag.map(|drag| drag.param.get(&self.render_settings))
    }

    /// Cuántos frames hay, para el rótulo `3 / 64`.
    pub fn frame_count(&self) -> usize {
        self.table.selectable_frames()
    }

    /// El frame que se dibuja, ya interpolado si corresponde.
    pub fn current_samples(&self) -> Vec<f32> {
        let position = self.active_cycle();
        self.table.frame(position, self.smooth)
    }

    /// Posición continua dentro de la tabla, en unidades de ciclo.
    ///
    /// Es lo que recorre el knob de morph (la fracción entre dos frames) y lo
    /// que la malla 3D usa para saber qué ciclo de la pila deja encendido. Las
    /// dos cosas leen el mismo número, así que el realce de la pila y el valor
    /// del knob no pueden desincronizarse.
    pub fn active_cycle(&self) -> f32 {
        let frames = self.table.selectable_frames();
        if frames <= 1 {
            return 0.0;
        }
        ((self.frame as f32 + self.morph) / (frames - 1) as f32).clamp(0.0, 1.0)
            * (frames - 1) as f32
    }

    /// La tabla entera, para la malla 3D.
    ///
    /// La pila se arma con la tabla completa y no con un solo ciclo: el eje Z
    /// es el índice del frame, así que ver la matriz es ver los ciclos reales
    /// que parseó [`wavetable_io`] y no una forma sintetizada aparte.
    pub fn table_samples(&self) -> &[f32] {
        &self.table.samples
    }

    /// Samples por ciclo de la tabla cargada.
    pub fn frame_len(&self) -> usize {
        wavetable_io::FRAME_SAMPLES
    }

    /// Mueve el morph un paso, respecting los límites de la tabla.
    pub fn nudge_morph(&mut self, delta: f32) {
        let frames = self.table.selectable_frames();
        if frames <= 1 {
            return;
        }
        let last = (frames - 1) as f32;
        self.morph = (self.morph + delta * last).clamp(0.0, 1.0);
    }

    /// Mueve el frame de a un paso.
    pub fn step_frame(&mut self, delta: i32) {
        let frames = self.table.selectable_frames();
        if frames <= 1 {
            return;
        }
        self.frame = (self.frame as i32 + delta).clamp(0, frames as i32 - 1) as usize;
    }

    /// Reemplaza la tabla cargada.
    ///
    /// Al cargar una tabla nueva el morph y el frame se van a 0: dejarlos donde
    /// estaban llevaría a un frame que en la tabla nueva no existe, y el knob
    /// mostraría una posición que no corresponde.
    pub fn load(&mut self, table: Wavetable) {
        self.table = table;
        self.morph = 0.0;
        self.frame = 0;
        // Un arrastre en curso sobre la tabla anterior no puede seguir vivo: su
        // `drag_grab` era una posición de una matriz que ya no existe, y al
        // primer movimiento saltaría a un frame arbitrario de la nueva.
        self.end_drag();
    }
}

/// Píxeles de arrastre vertical que recorren la matriz entera.
///
/// 160px y no el alto del knob: con los 48 del knob, un pixel de error del
/// cursor son 3% de la matriz, o dos ciclos de una tabla de 64, y el control se
/// siente pegajoso. A 160, el mismo error es medio ciclo.
pub const DRAG_RANGE_PX: f32 = 160.0;

/// Factor de sensibilidad con Shift apretado.
///
/// Un cuarto de la velocidad normal, que es el orden de magnitud con el que
/// funcionan los faders de audio en fine-tune: permite llevar el realce a un
/// ciclo concreto de una tabla de 64, que a velocidad normal es imposible.
const FINE_TUNE_FACTOR: f32 = 0.25;

/// Ganancia del arrastre según el modificador.
fn value_gain(fine: bool) -> f32 {
    if fine { FINE_TUNE_FACTOR } else { 1.0 }
}

/// Parámetro ajustable del panel de settings, con su rango y formato.
///
/// Es el equivalente horizontal del knob: el arrastre es relativo al punto de
/// agarre y recorre el rango entero en [`SETTING_DRAG_PX`] píxeles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingParam {
    /// Grosor del trazo en 3D (también profundidad del tubo en `Ribbon`).
    LineWidth,
    /// Grosor del trazo en el modo 2D de diagnóstico.
    LineWidth2d,
    /// Multiplicador del espaciado en profundidad.
    DepthScale,
    /// Yaw de la vista 3D en grados.
    Yaw,
    /// Pitch de la vista 3D en grados.
    Pitch,
    /// Fade back-to-front de las filas traseras.
    DepthFade,
    /// Ancho del suavizado de borde en píxeles (0 = trazo duro).
    AaFeather,
    /// Multiplicador del tinte (brillo/glow general).
    Glow,
}

impl SettingParam {
    /// Todos los sliders del panel, en orden de aparición.
    pub const ALL: [SettingParam; 8] = [
        SettingParam::LineWidth,
        SettingParam::LineWidth2d,
        SettingParam::DepthScale,
        SettingParam::Yaw,
        SettingParam::Pitch,
        SettingParam::DepthFade,
        SettingParam::AaFeather,
        SettingParam::Glow,
    ];

    /// Rótulo corto de la fila del panel (máx. ~6 caracteres para que entre
    /// en una línea del menú compacto).
    pub fn title(self) -> &'static str {
        match self {
            SettingParam::LineWidth => "Trazo",
            SettingParam::LineWidth2d => "Tr.2D",
            SettingParam::DepthScale => "Prof.Z",
            SettingParam::Yaw => "Yaw",
            SettingParam::Pitch => "Pitch",
            SettingParam::DepthFade => "Fade",
            SettingParam::AaFeather => "Suav.",
            SettingParam::Glow => "Glow",
        }
    }

    /// Rango del slider (mínimo, máximo).
    pub fn range(self) -> (f32, f32) {
        match self {
            SettingParam::LineWidth => (1.0, 6.0),
            SettingParam::LineWidth2d => (1.0, 8.0),
            SettingParam::DepthScale => (0.3, 2.0),
            SettingParam::Yaw => (-30.0, 30.0),
            SettingParam::Pitch => (5.0, 60.0),
            SettingParam::DepthFade => (0.0, 0.8),
            SettingParam::AaFeather => (0.0, 2.0),
            SettingParam::Glow => (0.3, 2.0),
        }
    }

    /// Lee el valor desde los settings.
    pub fn get(self, settings: &RenderSettings) -> f32 {
        match self {
            SettingParam::LineWidth => settings.line_width,
            SettingParam::LineWidth2d => settings.line_width_2d,
            SettingParam::DepthScale => settings.depth_scale,
            SettingParam::Yaw => settings.yaw_deg,
            SettingParam::Pitch => settings.pitch_deg,
            SettingParam::DepthFade => settings.depth_fade,
            SettingParam::AaFeather => settings.aa_feather,
            SettingParam::Glow => settings.glow,
        }
    }

    /// Escribe el valor acotado a su rango.
    pub fn set(self, settings: &mut RenderSettings, value: f32) {
        let (min, max) = self.range();
        let value = if value.is_finite() { value.clamp(min, max) } else { min };
        match self {
            SettingParam::LineWidth => settings.line_width = value,
            SettingParam::LineWidth2d => settings.line_width_2d = value,
            SettingParam::DepthScale => settings.depth_scale = value,
            SettingParam::Yaw => settings.yaw_deg = value,
            SettingParam::Pitch => settings.pitch_deg = value,
            SettingParam::DepthFade => settings.depth_fade = value,
            SettingParam::AaFeather => settings.aa_feather = value,
            SettingParam::Glow => settings.glow = value,
        }
    }

    /// Texto del valor para la fila del panel.
    pub fn format(self, value: f32) -> String {
        match self {
            SettingParam::Yaw | SettingParam::Pitch => format!("{value:.0}°"),
            SettingParam::DepthFade => format!("{:.0}%", value * 100.0),
            SettingParam::Glow => format!("{value:.1}×"),
            SettingParam::DepthScale => format!("{value:.2}"),
            _ => format!("{value:.1}"),
        }
    }
}

/// Arrastre en curso sobre un slider del panel.
#[derive(Clone, Copy, Debug)]
struct SettingDrag {
    /// Qué ajuste se está moviendo.
    param: SettingParam,
    /// Valor del ajuste en el instante del `mouse down`.
    grab: f32,
    /// Coordenada X de la ventana donde se apretó el botón, en píxeles.
    origin_x: f32,
}

/// Píxeles de arrastre horizontal que recorren el rango entero de un slider.
///
/// El mismo orden de magnitud que [`DRAG_RANGE_PX`] del knob: el gesto se
/// siente igual en los dos controles.
const SETTING_DRAG_PX: f32 = 160.0;

/// El módulo completo.
///
/// Cuatro bandas verticales dentro de un cuadrado: header, selector, visor y pie.
/// El visor toma lo que sobra con `flex_1`, así que el alto de las otras tres
/// bandas es lo único que hay que mantener en cuenta.
pub fn render(cx: &mut Context<HikaruApp>, track_idx: usize, slot_idx: usize) -> AnyElement {
    let editor = {
        let app = state(cx).read(cx);
        match app.slot(track_idx, slot_idx) {
            // Se clona y se suelta el guard antes de seguir: los handlers de la
            // vista necesitan el lock de escritura y un `read` vivo lo
            // bloquearía.
            Some(slot) => slot.wavetable.clone(),
            None => return loading_panel().into_any_element(),
        }
    };

    // El render 3D se pide una sola vez acá y sus dos imágenes se reparten entre
    // el visor y el pie. Pedirlas por separado tomaría el lock dos veces por
    // frame y podría pintar el knob con el valor viejo junto a la pila nueva.
    let view = request_view(cx, &editor);

    v_flex()
        .id("open_wavetable_editor")
        .w(px(EDITOR_WIDTH))
        .h(px(EDITOR_HEIGHT))
        .p(px(MODULE_PADDING))
        .gap(px(MODULE_GAP))
        .bg(rgb(0x0E0F13))
        .border_1()
        .border_color(rgb(0x2A2E3A))
        .rounded(px(4.0))
        // `relative` para que el canvas del keepalive se posicione contra el
        // módulo y no contra la ventana entera. Sin esto, un `absolute()` sin
        // ancestro posicionado se ancla al viewport y el canvas ocuparía toda la
        // pantalla. Hoy no molesta (no dibuja nada y no tiene hitbox), pero es
        // un acoplamiento silencioso al layout.
        .relative()
        .overflow_hidden()
        // Fin del arrastre, con los dos handlers de elemento.
        //
        // `on_mouse_up` sólo dispara con el cursor **dentro** del módulo, así que
        // hace falta también el `_out`. Son disjuntos por construcción
        // (`is_hovered` y `!is_hovered`), así que entre los dos el arrastre
        // termina siempre, se suelte el botón donde se suelte.
        //
        // Van en el elemento y no en el listener de ventana de `on_mouse_event`
        // a propósito: los listeners de elemento se reinstalan en cada paint del
        // módulo, o sea que están vivos de forma continua. Los de ventana se
        // borran en cada frame, así que si el fin del arrastre dependiera de
        // ellos habría una ventana de un frame en la que un clic rápido
        // (apretar y soltar antes del siguiente render) los dejaría pasar y el
        // knob quedaría con `dragging` en `true` para siempre.
        .on_mouse_up(gpui_kit::MouseButton::Left, move |_, _, cx| {
            with_editor(cx, track_idx, slot_idx, WavetableEditor::end_drag);
        })
        .on_mouse_up_out(gpui_kit::MouseButton::Left, move |_, _, cx| {
            with_editor(cx, track_idx, slot_idx, WavetableEditor::end_drag);
        })
        .child(render_header(&editor))
        .child(render_table_navigator(cx, track_idx, slot_idx, &editor))
        .child(render_viewport(track_idx, slot_idx, &editor, &view))
        .child(render_footer(cx, track_idx, slot_idx, &editor, &view))
        // Red de seguridad del seguimiento del arrastre. Va al final para que
        // quede por encima: es invisible y no registra hitbox, así que no
        // intercepta el mouse del knob.
        .child(drag_keepalive(track_idx, slot_idx))
        .into_any_element()
}

/// Si el knob o un slider de este slot tiene un arrastre en curso.
///
/// Lee por `&App` compartido, a diferencia de [`with_editor`], que pide el lock
/// de escritura: el seguimiento consulta el flag en cada movimiento del mouse y
/// no puede tomar un write lock para eso.
fn knob_dragging(cx: &gpui_kit::App, track_idx: usize, slot_idx: usize) -> bool {
    let app = state(cx).read(cx);
    app.slot(track_idx, slot_idx)
        .is_some_and(|slot| slot.wavetable.dragging || slot.wavetable.setting_drag.is_some())
}

/// Registra el seguimiento del arrastre a nivel de ventana.
///
/// # Por qué no `on_mouse_move` de un elemento
///
/// Los listeners de elemento de GPUI Kit se despachan **sólo si el hitbox del
/// elemento está bajo el cursor** (en `on_mouse_move`, el closure arranca con
/// `if ... && hitbox.is_hovered(window)`). Arrastrar el knob hacia abajo lo saca
/// del módulo en cuestión de píxeles, el listener deja de dispararse y el valor
/// queda congelado a mitad de camino: el gesto se cortaba a los ~40px y con 256
/// ciclos el contador se clavaba en el 67 en vez de llegar al 0.
///
/// `Window::on_mouse_event` no lleva esa condición, así que el seguimiento va por
/// ahí y el arrastre sigue aunque el cursor salga del módulo, de la ventana o de
/// la pantalla.
///
/// # Sólo el movimiento
///
/// El `mouse up` se resuelve con los handlers de elemento del módulo
/// (`on_mouse_up` y `on_mouse_up_out`). Esos no necesitan re-registrarse: cada
/// paint del módulo los vuelve a instalar, así que están vivos de forma continua
/// y no hay ventana de un frame en la que se puedan perder. Este listener sí se
/// borra frame a frame, y esa es exactamente la razón por la que el fin del
/// arrastre no debe depender de él.
fn register_drag_tracking(
    window: &mut gpui_kit::Window,
    track_idx: usize,
    slot_idx: usize,
) {
    window.on_mouse_event(move |event: &MouseMoveEvent, _phase, _window, cx| {
        // Se lee la posición antes y después para no notificar cuando el arrastre no
        // movió nada. `with_editor` notifica siempre, y como este listener se
        // re-registra en el paint de cada frame, un `notify` por movimiento de
        // cursor se realimenta: frame nuevo -> paint -> listener nuevo -> otro
        // notify. Los listeners se acumulaban y la interfaz dejo de terminar un
        // frame limpio.
        let before = state(cx)
            .read(cx)
            .slot(track_idx, slot_idx)
            .map(|slot| slot.wavetable.position());

        let changed = with_editor_if(cx, track_idx, slot_idx, |editor| {
            // Los sliders del panel se arrastran en horizontal; el knob, en
            // vertical. Un solo gesto activo por vez: el `mouse down` que lo
            // empezó ya apagó el otro.
            if editor.setting_drag.is_some() {
                let previous = editor.setting_drag_value();
                editor.drag_setting_to(event.position.x.as_f32(), event.modifiers.shift);
                return editor.setting_drag_value() != previous;
            }
            if !editor.dragging {
                return false;
            }
            let previous = editor.position();
            editor.drag_to(event.position.y.as_f32(), event.modifiers.shift);
            editor.position() != previous
        });

        if changed && before.is_some() {
            // `with_editor_if` ya notificó sólo cuando cambió; acá no hay nada
            // que hacer. Se deja el bloque para que el motivo quede escrito.
        }
    });
}

/// Canvas invisible que mantiene vivo el seguimiento del arrastre.
///
/// `Window::on_mouse_event` borra sus listeners en cada frame renderizado, así
/// que hay que volver a registrar en cada frame mientras haya arrastre. Este
/// canvas lo hace desde su fase de **paint**.
///
/// # Por qué el closure de paint y no el de prepaint
///
/// `Window::on_mouse_event` hace `debug_assert!` de que la fase de dibujo sea
/// `DrawPhase::Paint`, y no `Prepaint`. Registrar el listener desde el prepaint
/// de este canvas entraba en panic igual que hacerlo desde `on_mouse_down`. El
/// panic sale como `recursion`-style assert en `window.rs`, lejos de la causa.
///
/// El invariante que sostiene el gesto: si se renderizó un frame, el paint de
/// este canvas corrió y el listener quedó registrado de nuevo; y si no se
/// renderizó ningún frame, entonces no se borró ningún listener. No hay forma
/// de quedarse sin seguimiento a mitad de arrastre.
///
/// No tiene hitbox (no registra interactividad), así que no intercepta el mouse
/// del knob ni de los botones.
fn drag_keepalive(track_idx: usize, slot_idx: usize) -> AnyElement {
    canvas(
        |_bounds, _window, _cx| {},
        move |_bounds, _state, window, cx| {
            if knob_dragging(cx, track_idx, slot_idx) {
                register_drag_tracking(window, track_idx, slot_idx);
            }
        },
    )
    .absolute()
    .inset_0()
    .into_any_element()
}

/// La banda de arriba: el nombre del plugin y qué tabla tiene cargada.
///
/// Es un header y no un rótulo suelto porque dentro de una cadena de módulos es
/// lo único que dice *qué* dispositivo es cuál: sin él, tres módulos cuadrados
/// iguales en fila son indistinguibles.
fn render_header(editor: &WavetableEditor) -> AnyElement {
    h_flex()
        .w_full()
        .h(px(HEADER_HEIGHT))
        .items_center()
        .justify_between()
        .gap(px(4.0))
        .child(
            Label::new("OpenWavetable")
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0xE8EAF0)),
        )
        .child(
            Label::new(format!("{} {}", editor.frame_count(), plural(editor.frame_count(), "ciclo")))
                .text_xs()
                .text_color(rgb(0x6A7080)),
        )
        .into_any_element()
}

/// Un número y su sustantivo, para los rótulos de conteo.
///
/// "1 ciclos" en el header era la señal de que la tabla tenía un solo ciclo, que
/// es justo el caso que el usuario necesita ver escrito bien.
fn plural(count: usize, noun: &str) -> String {
    if count == 1 { noun.to_string() } else { format!("{noun}s") }
}

/// Panel de transición, mientras el slot cambia o se está deseleccionando.
fn loading_panel() -> AnyElement {
    v_flex()
        .w(px(EDITOR_WIDTH))
        .h(px(EDITOR_HEIGHT))
        .items_center()
        .justify_center()
        .bg(rgb(0x0E0F13))
        .child(Label::new("Abriendo editor...").text_xs().text_color(rgb(0x6A7080)))
        .into_any_element()
}

/// La banda del selector: el botón `WAVETABLE ▾` y el nombre de la tabla.
///
/// En una fila, porque el módulo es angosto: con el rótulo arriba y el nombre
/// abajo, el nombre larga se parte en dos líneas y empuja al visor.
///
/// El menú es una lista de archivos ya cargados más las dos entradas que abren
/// un selector. No se arma un `Menu` de gpui-kit: hace falta un popover
/// posicionado, y con dos entradas y una lista cortita un `v_flex` condicional
/// alcanza y no depende de dónde esté la ventana.
fn render_table_navigator(
    _cx: &mut Context<HikaruApp>,
    track_idx: usize,
    slot_idx: usize,
    editor: &WavetableEditor,
) -> AnyElement {
    let table_name = editor.table.name.clone();
    let siblings = sibling_tables(editor.table.path.as_ref());
    let current_path = editor.table.path.clone();

    h_flex()
        .w_full()
        .h(px(SELECTOR_HEIGHT))
        .items_center()
        .gap(px(2.0))
        .relative()
        .child(
            Button::new(format!("wt_table_prev_{track_idx}_{slot_idx}"))
                .rounded(ButtonRounded::None)
                .label("<")
                .compact()
                .on_click(move |_, _, cx| {
                    step_table(cx, track_idx, slot_idx, -1);
                }),
        )
        .child(
            Button::new(format!("wt_table_menu_{track_idx}_{slot_idx}"))
                .rounded(ButtonRounded::None)
                .compact()
                .flex_1()
                .min_w(px(0.0))
                .on_click(move |_, _, cx| {
                    with_editor(cx, track_idx, slot_idx, |editor| {
                        editor.menu_open = !editor.menu_open;
                    });
                })
                .child(
                    Label::new(format!("{table_name}.wav"))
                        .text_xs()
                        .font_weight(FontWeight::NORMAL)
                        .text_color(rgb(0xC8CEDA))
                        .truncate(),
                ),
        )
        .child(
            Button::new(format!("wt_table_next_{track_idx}_{slot_idx}"))
                .rounded(ButtonRounded::None)
                .label(">")
                .compact()
                .on_click(move |_, _, cx| {
                    step_table(cx, track_idx, slot_idx, 1);
                }),
        )
        .when(editor.menu_open, |this| {
            this.child(
                v_flex()
                    .absolute()
                    .left(px(0.0))
                    .bottom(px(SELECTOR_HEIGHT + 2.0))
                    .w_full()
                    .h(px(150.0))
                    .overflow_y_scrollbar()
                    .p(px(4.0))
                    .gap(px(2.0))
                    .bg(rgb(0x181B22))
                    .border_1()
                    .border_color(rgb(0x3A4152))
                    .rounded(px(3.0))
                    .children(siblings.iter().map(|path| {
                        let name = path
                            .file_stem()
                            .map(|stem| stem.to_string_lossy().to_string())
                            .unwrap_or_else(|| "Wavetable".to_string());
                        let target = path.clone();
                        let is_current = current_path.as_ref() == Some(path);
                        Button::new(format!("wt_table_pick_{track_idx}_{slot_idx}_{name}"))
                            .rounded(ButtonRounded::None)
                            .label(name.clone())
                            .compact()
                            .w_full()
                            .when(is_current, |b| b.text_color(rgb(0xFF6E00)))
                            .on_click(move |_, _, cx| {
                                load_table_deferred(cx, track_idx, slot_idx, target.clone());
                            })
                            .into_any_element()
                    }))
                    .child(
                        Button::new(format!("wt_browse_explorer_{track_idx}_{slot_idx}"))
                            .rounded(ButtonRounded::None)
                            .label("Buscar en el Explorer (F11)")
                            .compact()
                            .w_full()
                            .on_click(move |_, _, cx| {
                                crate::views::explorer::begin_wavetable_pick(cx, track_idx, slot_idx);
                            }),
                    )
                    .child(
                        Button::new(format!("wt_browse_native_{track_idx}_{slot_idx}"))
                            .rounded(ButtonRounded::None)
                            .label("Buscar archivo...")
                            .compact()
                            .w_full()
                            .on_click(move |_, _, cx| {
                                pick_wavetable_with_native_dialog(cx, track_idx, slot_idx);
                            }),
                    )
                    .child(
                        Button::new(format!("wt_use_sine_{track_idx}_{slot_idx}"))
                            .rounded(ButtonRounded::None)
                            .label(format!("Volver a la tabla de fábrica ({} ciclos)", wavetable_io::DEFAULT_CYCLES))
                            .compact()
                            .w_full()
                            .on_click(move |_, _, cx| {
                                load_log::factory_restored(wavetable_io::DEFAULT_CYCLES);
                                with_editor(cx, track_idx, slot_idx, |editor| {
                                    editor.load(Wavetable::default_table());
                                    editor.menu_open = false;
                                });
                            }),
                    ),
            )
        })
        .into_any_element()
}

/// La banda de abajo: el knob `WT POS` y, al lado, el ciclo y su navegación.
///
/// # Por qué un solo knob
///
/// Antes había dos: MORPH e INDEX. Los dos movían lo mismo —la posición en la
/// matriz de ciclos— por dos caminos distintos, y con el mismo valor el módulo
/// mostraba dos perillas idénticas sin que se supiera cuál mandar. El knob es
/// de recorrido continuo y los botones de paso de a uno quedan como atajos.
///
/// El knob va como elemento del layout y no superpuesto sobre el visor: dentro
/// de un módulo cuadrado no hay ancho de sobra, y tapar la imagen con una
/// perilla es justo lo que hay que evitar cuando la imagen es de 135px de alto.
fn render_footer(
    cx: &mut Context<HikaruApp>,
    track_idx: usize,
    slot_idx: usize,
    editor: &WavetableEditor,
    view: &WavetableView,
) -> AnyElement {
    // `cx` no se usa directo: los handlers de los botones pasan por
    // `with_editor`, que toma el lock del estado por su cuenta.
    let _ = cx;
    let frames = editor.frame_count();

    h_flex()
        .w_full()
        .h(px(FOOTER_HEIGHT))
        .items_center()
        .gap(px(6.0))
        .child(
            v_flex()
                .items_center()
                .justify_center()
                .gap(px(1.0))
                .child(knob_image(track_idx, slot_idx, &editor, view.knob.clone()))
                .child(
                    Label::new("WT POS")
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0xFF6E00)),
                ),
        )
        .child(
            v_flex()
                .min_w(px(0.0))
                .flex_1()
                .items_center()
                .justify_center()
                .gap(px(3.0))
                // `frame + 1` porque el rótulo va 1-based, como el número de un
                // track.
                .child(
                    Label::new(format!("{} / {}", editor.frame + 1, frames))
                        .text_xs()
                        .text_color(rgb(0x8A90A0)),
                )
                .child(
                    h_flex()
                        .gap(px(3.0))
                        .child(
                            Button::new(format!("wt_frame_prev_{track_idx}_{slot_idx}"))
                                .rounded(ButtonRounded::None)
                                .label("<")
                                .compact()
                                .on_click(move |_, _, cx| {
                                    with_editor(cx, track_idx, slot_idx, |editor| {
                                        editor.step_frame(-1)
                                    });
                                }),
                        )
                        .child(
                            Button::new(format!("wt_frame_next_{track_idx}_{slot_idx}"))
                                .rounded(ButtonRounded::None)
                                .label(">")
                                .compact()
                                .on_click(move |_, _, cx| {
                                    with_editor(cx, track_idx, slot_idx, |editor| {
                                        editor.step_frame(1)
                                    });
                                }),
                        ),
                )
                // El espectro de la tabla actual, como la barrita de armónicos de
                // Bitwig: muestra qué tan brillante es la forma sin abrir nada.
                .child(spectrum(&editor.current_samples(), rgb(0xFF6E00))),
        )
        .into_any_element()
}

/// El knob 3D, con su `mouse down` para empezar el arrastre.
///
/// El disco se dibuja con fondo transparente, así que va sobre el fondo del
/// módulo sin un recuadro alrededor.
///
/// # El mouse down no salta el valor
///
/// Se guarda el punto de agarre ([`WavetableEditor::begin_drag`]) en vez de
/// poner el knob en la posición del cursor. Es la diferencia entre un knob
/// usable y uno que se teletransporta: si el click fuera absoluto, un error de
/// dos píxeles al agarrar el knob movería la matriz varios ciclos, y más en una
/// tabla de 64.
///
/// # Sin ciclos no hay knob
///
/// Con una tabla de un solo ciclo no hay a dónde ir: `position()` devuelve 0
/// siempre. Un knob que acepta el arrastre y no se mueve parece roto, así que
/// en ese caso no se registra el `mouse down` y el disco queda atenuado. El
/// contador del pie dice `1 / 1`, que es la explicación.
///
/// # El cursor
///
/// El `mouse down` con botón izquierdo no captura el puntero: el del elemento
/// sigue siendo la flecha, y una flecha no dice que el control se arrastra. Sin
/// una API de captura en GPUI Kit no hay más opción; el anillo naranja de abajo
/// es la señal explícita de que el knob está activo.
fn knob_image(
    track_idx: usize,
    slot_idx: usize,
    editor: &WavetableEditor,
    image: Option<std::sync::Arc<RenderImage>>,
) -> AnyElement {
    // Con un solo ciclo no hay recorrido, así que el control se apaga.
    let enabled = editor.frame_count() > 1;

    let knob = div()
        .id("wt_knob_pos")
        .w(px(KNOB_SIZE))
        .h(px(KNOB_SIZE))
        .relative()
        .rounded(px(KNOB_SIZE / 2.0))
        .border_1()
        .border_color(match (enabled, editor.dragging) {
            (true, true) => rgb(0xFF6E00),
            (true, false) => rgb(0x2A2E3A),
            // Apagado: el mismo borde, pero el disco va atenuado abajo.
            (false, _) => rgb(0x1E212B),
        })
        .child(match image {
            Some(image) => img(image)
                .id("wt_knob_pos_img")
                .absolute()
                .left(px(0.0))
                .top(px(0.0))
                .w_full()
                .h_full()
                // El `opacity` es lo que comunica que el control no anda: un
                // disco normal con un solo ciclo se lee como roto.
                .opacity(if enabled { 1.0 } else { 0.35 })
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
            None => div().w_full().h_full().into_any_element(),
        });

    // `on_mouse_down` se registra sólo cuando hay recorrido. Es condicional
    // porque el builder se consume: aplicarlo siempre y dejar que el handler no
    // haga nada igual cobraría el `on_mouse_move` de cada movimiento del cursor
    // sobre el módulo.
    if enabled {
        knob
            .on_mouse_down(gpui_kit::MouseButton::Left, move |event, _window, cx| {
                // Sólo se guarda el punto de agarre. El seguimiento del
                // movimiento lo registra `drag_keepalive` desde la fase de
                // paint: `Window::on_mouse_event` hace `debug_assert!` de que la
                // fase sea `DrawPhase::Paint`, y un `mouse down` corre en
                // dispatch de eventos, así que llamarlo desde acá entraba en
                // panic al primer click sobre el knob.
                with_editor(cx, track_idx, slot_idx, |editor| {
                    editor.begin_drag(event.position.y.as_f32());
                });
            })
            .into_any_element()
    } else {
        knob.into_any_element()
    }
}

/// Pide las imágenes del módulo al viewport offscreen.
///
/// Va separado de [`render_viewport`] porque el render alimenta dos bandas del
/// layout —el visor y el pie— y pedirlo por separado en cada una tomaría el lock
/// dos veces por frame.
fn request_view(cx: &mut Context<HikaruApp>, editor: &WavetableEditor) -> WavetableView {
    let handle = cx.global::<WavetableViewportHandle>().clone();
    handle.spawn_connect(cx);

    // En modo 2D la malla es el ciclo activo ya interpolado (el morphing
    // exacto bajo `WT POS`): se calcula acá y se presta por referencia al
    // pedido, que sólo lo lee durante el `view`. En 3D no se calcula para no
    // tirar una copia de 2048 samples por frame.
    let active_2d =
        (editor.render_mode == RenderMode::Mode2D).then(|| editor.current_samples());

    handle.view(&WavetableViewRequest {
        viewer: ViewerRequest {
            size: WAVETABLE_VIEWPORT,
            // La cámara viene fija del `hikaru_render` (`Camera::wavetable_viewer`,
            // 30° de inclinación). El visor ya no tiene controles de yaw/pitch:
            // la perspectiva es la que hace legible la pila de ciclos. En modo
            // 2D el renderer la reemplaza por la frontal (ver `render_viewer`).
            camera: editor.camera,
            // La tabla entera, para que la malla apile los ciclos reales en Z.
            waveform: editor.table_samples(),
            frame_len: editor.frame_len(),
            active: editor.active_cycle(),
            render_mode: editor.render_mode,
            settings: editor.render_settings,
            active_frame: active_2d.as_deref(),
            max_frames: MAX_STACK_FRAMES,
            mesh_params: mesh_params(),
            tint: VIEWER_TINT,
            background: hikaru_render::wgpu::Color::TRANSPARENT,
        },
        // El `fill` del knob y el `active` de la malla salen del mismo estado y
        // son la misma posición en dos escalas: `position()` es 0..1 sobre la
        // matriz y `active_cycle()` son ciclos absolutos. Los dos leen
        // `WavetableEditor`, así que el disco no puede girar a un lado mientras
        // el realce de la pila se ilumina en otro.
        morph: editor.position(),
        knob_body: KNOB_BODY,
        knob_marker: KNOB_MARKER,
        knob_camera: Camera::knob(),
    })
}

/// La banda del centro: el mini-canvas con la pila de ciclos.
///
/// Es la única banda con `flex_1`, así que su alto es el que sobra del cuadrado
/// menos las otras tres. El `img` va con `ObjectFit::Contain`: la imagen offscreen
/// tiene su propio aspect y encajarla a la fuerza la deformaría.
/// Controles del visor, arriba a la derecha y superpuestos sin tapar la onda:
/// el engranaje abre el panel "Hikaru OpenWavetable Settings" y el botón 2D/3D
/// alterna entre el terreno en perspectiva y el ciclo activo plano. El modo
/// actual va en naranja.
fn view_mode_controls(
    track_idx: usize,
    slot_idx: usize,
    mode: RenderMode,
    settings_open: bool,
) -> AnyElement {
    h_flex()
        .absolute()
        .right(px(3.0))
        .top(px(3.0))
        .gap(px(2.0))
        .child(
            Button::new(format!("wt_settings_{track_idx}_{slot_idx}"))
                .rounded(ButtonRounded::None)
                .compact()
                .child(
                    Label::new("⚙")
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(if settings_open { rgb(0xFF6E00) } else { rgb(0x8A90A0) }),
                )
                .on_click(move |_, _, cx| {
                    with_editor(cx, track_idx, slot_idx, |editor| {
                        editor.settings_open = !editor.settings_open;
                    });
                }),
        )
        .child(
            Button::new(format!("wt_viewmode_{track_idx}_{slot_idx}"))
                .rounded(ButtonRounded::None)
                .compact()
                .child(
                    Label::new(mode.label())
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0xFF6E00)),
                )
                .on_click(move |_, _, cx| {
                    with_editor(cx, track_idx, slot_idx, |editor| {
                        editor.render_mode = editor.render_mode.toggle();
                    });
                }),
        )
        .into_any_element()
}

/// Una fila de slider del panel: título, barra arrastrable en horizontal y
/// valor. El arrastre es relativo al agarre (igual que el knob pero en X) y
/// con Shift va a un cuarto de velocidad.
fn setting_slider(
    track_idx: usize,
    slot_idx: usize,
    param: SettingParam,
    value: f32,
) -> AnyElement {
    // Anchos fijos que suman al interior del panel (140px): 42 + 3 + 57 +
    // 3 + 32 = 137. Nada se parte en dos líneas.
    const BAR_W: f32 = 57.0;
    let (min, max) = param.range();
    let frac = ((value - min) / (max - min)).clamp(0.0, 1.0);

    h_flex()
        .w_full()
        .items_center()
        .gap(px(3.0))
        .child(
            Label::new(param.title())
                .text_xs()
                .text_color(rgb(0x8A90A0))
                .w(px(42.0))
                .truncate(),
        )
        .child(
            div()
                .w(px(BAR_W))
                .h(px(10.0))
                .bg(rgb(0x0E0F13))
                .border_1()
                .border_color(rgb(0x2A2E3A))
                .rounded(px(2.0))
                .relative()
                .overflow_hidden()
                .child(
                    div()
                        .absolute()
                        .left(px(0.0))
                        .top(px(0.0))
                        .bottom(px(0.0))
                        .w(px(BAR_W * frac))
                        .bg(rgb(0xFF6E00)),
                )
                .on_mouse_down(gpui_kit::MouseButton::Left, move |event, _window, cx| {
                    with_editor(cx, track_idx, slot_idx, |editor| {
                        editor.begin_setting_drag(param, event.position.x.as_f32());
                    });
                }),
        )
        .child(
            Label::new(param.format(value))
                .text_xs()
                .text_color(rgb(0xE8EAF0))
                .w(px(32.0))
                .truncate(),
        )
        .into_any_element()
}

/// Panel emergente "Hikaru OpenWavetable Settings": sliders en vivo del look
/// del visor. Cada cambio entra en la clave de caché del viewport, así que el
/// re-render es inmediato sin más plomería.
/// Menú contextual compacto de settings (~148px): entra en el visor sin
/// taparlo. Una sola columna de filas de una línea —sin secciones ni textos
/// largos— con los 8 ajustes y el selector de malla.
fn settings_panel(
    track_idx: usize,
    slot_idx: usize,
    settings: &RenderSettings,
) -> AnyElement {
    v_flex()
        .absolute()
        .right(px(3.0))
        .top(px(26.0))
        .w(px(148.0))
        .max_h(px(190.0))
        .overflow_y_scrollbar()
        .p(px(4.0))
        .gap(px(2.0))
        .bg(rgb(0x181B22))
        .border_1()
        .border_color(rgb(0x3A4152))
        .rounded(px(3.0))
        .child(
            Label::new("WT Settings")
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0xE8EAF0)),
        )
        .child(
            Button::new(format!("wt_meshtype_{track_idx}_{slot_idx}"))
                .rounded(ButtonRounded::None)
                .compact()
                .w_full()
                .label(format!("Malla: {}", settings.mesh.label()))
                .on_click(move |_, _, cx| {
                    with_editor(cx, track_idx, slot_idx, |editor| {
                        editor.render_settings.mesh = editor.render_settings.mesh.cycle();
                    });
                }),
        )
        .children(SettingParam::ALL.iter().map(|param| {
            setting_slider(track_idx, slot_idx, *param, param.get(settings)).into_any_element()
        }))
        .into_any_element()
}

fn render_viewport(
    track_idx: usize,
    slot_idx: usize,
    editor: &WavetableEditor,
    view: &WavetableView,
) -> AnyElement {
    div()
        .flex_1()
        .min_h(px(0.0))
        .w_full()
        .relative()
        .bg(rgb(0x07080B))
        .border_1()
        .border_color(rgb(0x2A2E3A))
        .rounded(px(3.0))
        .overflow_hidden()
        .child(match &view.viewer {
            Some(image) => img(image.clone())
                .id("wt_viewport_3d")
                .absolute()
                .left(px(0.0))
                .top(px(0.0))
                .w_full()
                .h_full()
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
            None => viewport_placeholder(view.error.as_deref()),
        })
        .child(view_mode_controls(
            track_idx,
            slot_idx,
            editor.render_mode,
            editor.settings_open,
        ))
        .when(editor.settings_open, |this| {
            this.child(settings_panel(track_idx, slot_idx, &editor.render_settings))
        })
        .into_any_element()
}

const KNOB_2D: f32 = 30.0;

const KNOBS_HEIGHT: f32 = 70.0;

const MODULE_FOOTER_HEIGHT: f32 = 26.0;

fn with_editor_target(
    cx: &mut gpui_kit::App,
    track_idx: usize,
    slot_idx: usize,
    target: KnobTarget,
    window_y: f32,
) {
    state(cx).update(cx, |state, cx| {
        if let Some(slot) = state.slot_mut(track_idx, slot_idx) {
            slot.wavetable.begin_drag_target(target, window_y);
        }
        cx.notify();
    });
}

fn knob_2d(
    track_idx: usize,
    slot_idx: usize,
    target: KnobTarget,
    value: f32,
    title: &str,
    sub: String,
    hot: bool,
) -> AnyElement {
    let value = value.clamp(0.0, 1.0);
    v_flex()
        .items_center()
        .gap(px(1.0))
        .child(
            div()
                .id(format!("wt_knob2d_{target:?}_{track_idx}_{slot_idx}"))
                .w(px(KNOB_2D))
                .h(px(KNOB_2D))
                .rounded(px(KNOB_2D / 2.0))
                .bg(rgb(0x14161C))
                .border_1()
                .border_color(if hot { rgb(0xFF6E00) } else { rgb(0x2A2E3A) })
                .on_mouse_down(gpui_kit::MouseButton::Left, move |event, _window, cx| {
                    with_editor_target(cx, track_idx, slot_idx, target, event.position.y.as_f32());
                })
                .child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            let center_x =
                                bounds.origin.x + px(bounds.size.width.as_f32() / 2.0);
                            let center_y =
                                bounds.origin.y + px(bounds.size.height.as_f32() / 2.0);
                            let radius =
                                px(bounds.size.width.as_f32() / 2.0 - 5.0);
                            let angle =
                                (-135.0 + 270.0 * value) * std::f32::consts::PI / 180.0;
                            let tip_x =
                                center_x + px(angle.sin() * radius.as_f32());
                            let tip_y =
                                center_y - px(angle.cos() * radius.as_f32());
                            let mut needle = PathBuilder::stroke(px(2.0));
                            needle.move_to(point(center_x, center_y));
                            needle.line_to(point(tip_x, tip_y));
                            window.paint_path(
                                needle.build().unwrap(),
                                if hot { rgb(0xFF6E00) } else { rgb(0xE8EAF0) },
                            );
                        },
                    )
                    .w_full()
                    .h_full()
                    .into_any_element(),
                )
                .into_any_element(),
        )
        .child(
            Label::new(title)
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0x8A90A0)),
        )
        .child(Label::new(sub).text_xs().text_color(rgb(0xE8EAF0)))
        .into_any_element()
}

fn render_knobs(
    cx: &mut Context<HikaruApp>,
    track_idx: usize,
    slot_idx: usize,
    editor: &WavetableEditor,
    view: &WavetableView,
) -> AnyElement {
    let _ = cx;
    let hot = editor.dragging;
    let target = editor.drag_target;
    let voices = editor.unison as usize;

    h_flex()
        .w_full()
        .h(px(KNOBS_HEIGHT))
        .items_center()
        .gap(px(4.0))
        .child(knob_2d(
            track_idx,
            slot_idx,
            KnobTarget::Unison,
            editor.norm_unison(),
            "UNISON",
            format!("{} {}", voices, if voices == 1 { "Voice" } else { "Voices" }),
            hot && target == KnobTarget::Unison,
        ))
        .child(knob_2d(
            track_idx,
            slot_idx,
            KnobTarget::Detune,
            editor.detune,
            "DETUNE",
            format!("{:.0}%", editor.detune * 100.0),
            hot && target == KnobTarget::Detune,
        ))
        .child(knob_2d(
            track_idx,
            slot_idx,
            KnobTarget::Phase,
            editor.phase,
            "PHASE",
            format!("{:.0}°", editor.phase * 360.0),
            hot && target == KnobTarget::Phase,
        ))
        .child(div().flex_1())
        .child(
            v_flex()
                .items_center()
                .justify_center()
                .gap(px(1.0))
                .child(knob_image(track_idx, slot_idx, editor, view.knob.clone()))
                .child(
                    Label::new("WT POS")
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0xFF6E00)),
                ),
        )
        .into_any_element()
}

fn render_viewport_card(
    track_idx: usize,
    slot_idx: usize,
    editor: &WavetableEditor,
    view: &WavetableView,
    frame: usize,
    frames: usize,
) -> AnyElement {
    div()
        .flex_1()
        .min_h(px(0.0))
        .w_full()
        .relative()
        .bg(rgb(0x07080B))
        .border_1()
        .border_color(rgb(0x2A2E3A))
        .rounded(px(3.0))
        .overflow_hidden()
        .child(match &view.viewer {
            Some(image) => img(image.clone())
                .id("wt_viewport_3d")
                .absolute()
                .left(px(0.0))
                .top(px(0.0))
                .w_full()
                .h_full()
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
            None => viewport_placeholder(view.error.as_deref()),
        })
        .child(view_mode_controls(
            track_idx,
            slot_idx,
            editor.render_mode,
            editor.settings_open,
        ))
        .when(editor.settings_open, |this| {
            this.child(settings_panel(track_idx, slot_idx, &editor.render_settings))
        })
        .child(
            v_flex()
                .absolute()
                .left(px(0.0))
                .top(px(0.0))
                .w_full()
                .h_full()
                .justify_end()
                .items_end()
                .p(px(3.0))
                .child(
                    Label::new(format!("< {}/{} >", frame + 1, frames))
                        .text_xs()
                        .text_color(rgb(0x8A90A0)),
                )
                .into_any_element(),
        )
        .into_any_element()
}

/// Placeholder cuando todavía no hay imagen.
///
/// Distingue los dos casos que se ven igual desde afuera: la GPU todavía no está
/// y el render falló con un motivo concreto.
fn viewport_placeholder(error: Option<&str>) -> AnyElement {
    let message = match error {
        Some(error) => error.to_string(),
        None => "Iniciando render 3D...".to_string(),
    };

    v_flex()
        .absolute()
        .left(px(0.0))
        .top(px(0.0))
        .w_full()
        .h_full()
        .items_center()
        .justify_center()
        .child(Label::new(message).text_xs().text_color(rgb(0x6A7080)))
        .into_any_element()
}

/// Espectro de la tabla: barras con la energía por banda.
///
/// Es la barrita de armónicos de Bitwig. Se calcula con una FFT ingenua porque
/// son 64 bandas sobre 2048 samples y sólo se hace una vez por frame: con
/// 2048 * 64 multiplicaciones a 60 Hz son 8 millones, que es nada al lado de un
/// readback de la GPU.
fn spectrum(samples: &[f32], color: Rgba) -> AnyElement {
    const BANDS: usize = 32;
    let mut peaks = [0.0f32; BANDS];

    // Ventana de Hann: sin ella, el leakage de la rectangular hace que todas las
    // bandas parezcan iguales.
    let window: Vec<f32> = (0..samples.len())
        .map(|index| {
            let phase = index as f32 / samples.len() as f32 * std::f32::consts::TAU;
            0.5 - 0.5 * phase.cos()
        })
        .collect();

    for band in 1..BANDS {
        // Se omiten las dos primeras bandas: son la componente de continua y el
        // primer armónico, que domina siempre.
        let bin = band * (samples.len() / BANDS);
        if bin >= samples.len() / 2 {
            break;
        }

        let mut real = 0.0;
        let mut imaginary = 0.0;
        for (index, sample) in samples.iter().enumerate() {
            let phase = 2.0 * std::f32::consts::PI * bin as f32 * index as f32 / samples.len() as f32;
            let value = sample * window[index];
            real += value * phase.cos();
            imaginary -= value * phase.sin();
        }

        peaks[band] = (real * real + imaginary * imaginary).sqrt() / samples.len() as f32;
    }

    // Normalizar contra la banda más fuerte: si no, el volumen de la tabla
    // decide el alto de todas las barras y dos wavetables se ven iguales.
    let loudest = peaks.iter().copied().fold(0.0f32, f32::max).max(1.0e-6);

    h_flex()
        .w_full()
        .h(px(20.0))
        .items_end()
        .gap(px(1.0))
        .children((0..BANDS).map(|band| {
            let height = (peaks[band] / loudest).clamp(0.0, 1.0);
            div()
                .flex_1()
                .h(px(2.0 + height * 18.0))
                .bg(if height > 0.001 { color } else { rgb(0x1A1D26) })
                .rounded(px(1.0))
        }))
        .into_any_element()
}

/// Abre el diálogo nativo para elegir un `.wav`.
///
/// El diálogo arranca en el home del usuario y no en `/`: buscar una wavetable
/// en la raíz del filesystem es una forma de no encontrar nunca nada.
fn pick_wavetable_with_native_dialog(
    cx: &mut gpui_kit::App,
    track_idx: usize,
    slot_idx: usize,
) {
    let start = std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/"));

    let Some(path) = rfd::FileDialog::new()
        .set_directory(&start)
        .add_filter("Wavetable", &wavetable_io::WAVETABLE_EXTENSIONS)
        .pick_file()
    else {
        return;
    };

    load_into_slot(cx, track_idx, slot_idx, &path);
}

/// Carga el archivo en el slot y avisa el resultado.
///
/// Separado de los dos caminos de entrada (explorer y diálogo) para que el
/// manejo del error y del aviso quede en un solo lugar.
fn load_into_slot(cx: &mut gpui_kit::App, track_idx: usize, slot_idx: usize, path: &std::path::Path) {
    match wavetable_io::load_wavetable(path, MAX_FRAMES) {
        Ok(table) => {
            load_log::loaded(path, &table);
            with_editor(cx, track_idx, slot_idx, |editor| {
                editor.load(table);
                editor.menu_open = false;
            });
        }
        Err(error) => {
            eprintln!(
                "[ Hikaru OpenLive ] : Wavetable NO cargado [{}] | error={}",
                path.display(),
                error
            );
        }
    }
}

/// Aplica un cambio al editor del slot y pide redibujar.
///
/// Es el único camino de escritura del panel: todas las perillas y botones pasan
/// por acá, así que hay un solo lugar donde se toma el lock del estado y se
/// llama a `notify`.
/// Igual que [`with_editor`], pero sólo notifica si el cambio hizo algo.
///
/// Existe para el arrastre del knob. `with_editor` notifica siempre, y como el
/// listener de mouse se re-registra en el paint de cada frame, un `notify` por
/// movimiento de cursor se realimenta: frame nuevo → paint → listener nuevo →
/// otro notify. Con el mouse quieto sobre el knob, la interfaz se quedaba
/// siempre pidiendo frames y nunca terminaba uno limpio.
pub fn with_editor_if(
    cx: &mut gpui_kit::App,
    track_idx: usize,
    slot_idx: usize,
    change: impl FnOnce(&mut WavetableEditor) -> bool,
) -> bool {
    let mut changed = false;
    state(cx).update(cx, |state, cx| {
        if let Some(slot) = state.slot_mut(track_idx, slot_idx) {
            changed = change(&mut slot.wavetable);
        }
        if changed {
            cx.notify();
        }
    });
    changed
}

pub fn with_editor(
    cx: &mut gpui_kit::App,
    track_idx: usize,
    slot_idx: usize,
    change: impl FnOnce(&mut WavetableEditor),
) {
    state(cx).update(cx, |state, cx| {
        if let Some(slot) = state.slot_mut(track_idx, slot_idx) {
            change(&mut slot.wavetable);
        }
        cx.notify();
    });
}

#[allow(clippy::items_after_statements)]
mod load_log {
    /// Log de arranque: qué wavetable tiene cada slot antes de que toques nada.
    ///
    /// La tabla de fábrica no viene de un archivo: `Wavetable::default_table`
    /// la sintetiza y deja `path` en `None`. Por eso acá no hay una ruta que
    /// mostrar y se dice explícitamente, para no dejar esperando un path que
    /// nunca existió.
    pub fn initial(label: &str, table: &super::wavetable_io::Wavetable) {
        match table.path.as_ref() {
            Some(path) => eprintln!(
                "[ Hikaru OpenLive ] : Wavetable inicial cargado [{}]",
                path.display()
            ),
            None => eprintln!(
                "[ Hikaru OpenLive ] : Wavetable inicial cargado [sintetizada en memoria, {} ciclos, sin archivo asociado]",
                table.frames
            ),
        }
        eprintln!(
            "[ Hikaru OpenLive ] :   {} | {} samples | {} frames | label={}",
            label, table.samples.len(), table.frames, table.name
        );
    }

    /// Log de cada wavetable que entra desde disco o desde el popover.
    pub fn loaded(path: &std::path::Path, table: &super::wavetable_io::Wavetable) {
        eprintln!(
            "[ Hikaru OpenLive ] : Wavetable cargado [{}]",
            path.display()
        );
        eprintln!(
            "[ Hikaru OpenLive ] :   {} | {} samples | {} frames | directorio={:?}",
            table.name,
            table.samples.len(),
            table.frames,
            path.parent().map(|p| p.display().to_string())
        );
    }

    /// Log de la tabla de fábrica cuando se elige explícitamente desde el menú.
    pub fn factory_restored(cycles: usize) {
        eprintln!(
            "[ Hikaru OpenLive ] : Wavetable restaurado a fábrica [sintetizada, {} ciclos]",
            cycles
        );
    }
}

/// Log de la wavetable con la que arranca un slot recién creado.
pub fn log_initial_wavetable(label: &str) {
    load_log::initial(label, &WavetableEditor::new().table);
}

/// Carga una wavetable desde una ruta, para el camino del explorer.
pub fn load_wavetable_from_path(
    cx: &mut gpui_kit::App,
    track_idx: usize,
    slot_idx: usize,
    path: &std::path::Path,
) {
    load_into_slot(cx, track_idx, slot_idx, path);
}

/// Cache de la lista de wavetables hermanas, por directorio.
///
/// La función se llama desde `render_table_navigator`, o sea **una vez por frame
/// de render**. Antes eso significaba un `read_dir` completo del directorio de
/// la tabla en cada frame, con el filtro de extensión, el sort y un `Vec<PathBuf>`
/// nuevo: con la carpeta de un pack como el de Au5, que tiene cientos de `.wav`,
/// eso es cientos de stat de filesystem por frame, en el hilo de UI.
///
/// Con la tabla de fábrica no pasaba nada porque `path` es `None` y se salía
/// antes de tocar el disco. El `read_dir` empieza recién cuando hay un archivo
/// cargado, que es exactamente cuando empezó el problema de la interfaz.
///
/// Cachear por directorio evita el I/O repetido sin cambiar el resultado: la
/// lista de archivos de una carpeta no cambia mientras el usuario navega por los
/// knobs. Si agrega o borra wavetables desde afuera, la lista se actualiza al
/// cambiar de tabla o al reiniciar; `clear_sibling_cache` la invalida a mano.
fn sibling_cache() -> &'static std::sync::Mutex<Option<(std::path::PathBuf, Vec<std::path::PathBuf>)>> {
    static CACHE: std::sync::Mutex<Option<(std::path::PathBuf, Vec<std::path::PathBuf>)>> =
        std::sync::Mutex::new(None);
    &CACHE
}

/// Vacía la cache de hermanas: para cuando la lista de archivos puede haber
/// cambiado en disco.
pub fn clear_sibling_cache() {
    if let Ok(mut cache) = sibling_cache().lock() {
        *cache = None;
    }
}

pub fn sibling_tables(path: Option<&std::path::PathBuf>) -> Vec<std::path::PathBuf> {
    let Some(path) = path else { return Vec::new() };
    let Some(dir) = path.parent() else { return Vec::new() };

    if let Ok(cache) = sibling_cache().lock() {
        if let Some((cached_dir, tables)) = cache.as_ref() {
            if cached_dir == dir {
                return tables.clone();
            }
        }
    }

    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<std::path::PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("wav")))
        .collect();
    out.sort();

    if let Ok(mut cache) = sibling_cache().lock() {
        *cache = Some((dir.to_path_buf(), out.clone()));
    }
    out
}

pub fn sibling_step_target(path: Option<&std::path::PathBuf>, delta: i32) -> Option<std::path::PathBuf> {
    let siblings = sibling_tables(path);
    if siblings.is_empty() {
        return None;
    }
    let current = path.and_then(|p| {
        siblings.iter().position(|candidate| candidate == p)
    });
    let next = match current {
        Some(index) => {
            let len = siblings.len() as i32;
            (index as i32 + delta).rem_euclid(len) as usize
        }
        None => {
            if delta < 0 {
                siblings.len() - 1
            } else {
                0
            }
        }
    };
    siblings.into_iter().nth(next)
}

pub fn load_table_deferred(
    cx: &mut gpui_kit::App,
    track_idx: usize,
    slot_idx: usize,
    path: std::path::PathBuf,
) {
    let st = state(cx);
    cx.defer(move |cx| {
        let loaded = load_table_file(&path);
        st.update(cx, |state, cx| {
            match loaded {
                Ok(table) => {
                    if let Some(slot) = state.slot_mut(track_idx, slot_idx) {
                        slot.wavetable.load(table);
                        slot.wavetable.menu_open = false;
                        slot.menu_open = false;
                    }
                    state.selected_slot_index = slot_idx;
                }
                Err(error) => {
                    eprintln!("[Hikaru] No se pudo cargar la wavetable: {error}");
                }
            }
            cx.notify();
        });
    });
}

pub fn step_table(cx: &mut gpui_kit::App, track_idx: usize, slot_idx: usize, delta: i32) {
    let current = state(cx)
        .read(cx)
        .slot(track_idx, slot_idx)
        .and_then(|slot| slot.wavetable.table.path.clone());
    if let Some(target) = sibling_step_target(current.as_ref(), delta) {
        load_table_deferred(cx, track_idx, slot_idx, target);
    }
}

/// La caja de la cinta del visor.
fn mesh_params() -> hikaru_render::WavetableMeshParams {
    hikaru_render::WavetableMeshParams { width: MESH_WIDTH, height: MESH_HEIGHT, thickness: MESH_THICKNESS }
}

pub fn render_module(cx: &mut Context<HikaruApp>, track_idx: usize, slot_idx: usize) -> AnyElement {
    let editor = {
        let app = state(cx).read(cx);
        match app.slot(track_idx, slot_idx) {
            Some(slot) => slot.wavetable.clone(),
            None => return div().w_full().flex_1().into_any_element(),
        }
    };

    let view = request_view(cx, &editor);
    let frames = editor.frame_count();

    div()
        .w_full()
        .flex_1()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .relative()
        .overflow_hidden()
        .on_mouse_up(gpui_kit::MouseButton::Left, move |_, _, cx| {
            with_editor(cx, track_idx, slot_idx, WavetableEditor::end_drag);
        })
        .on_mouse_up_out(gpui_kit::MouseButton::Left, move |_, _, cx| {
            with_editor(cx, track_idx, slot_idx, WavetableEditor::end_drag);
        })
        .child(render_viewport_card(track_idx, slot_idx, &editor, &view, editor.frame, frames))
        .child(render_table_navigator(cx, track_idx, slot_idx, &editor))
        .child(render_knobs(cx, track_idx, slot_idx, &editor, &view))
        .child(drag_keepalive(track_idx, slot_idx))
        .into_any_element()
}

pub fn render_module_footer(cx: &mut Context<HikaruApp>, track_idx: usize, slot_idx: usize) -> AnyElement {
    let editor = {
        let app = state(cx).read(cx);
        match app.slot(track_idx, slot_idx) {
            Some(slot) => slot.wavetable.clone(),
            None => return div().w_full().h(px(MODULE_FOOTER_HEIGHT)).into_any_element(),
        }
    };
    let _ = cx;

    h_flex()
        .w_full()
        .h(px(MODULE_FOOTER_HEIGHT))
        .items_center()
        .gap(px(2.0))
        .child(
            Button::new(format!("wt_pitch_down_{track_idx}_{slot_idx}"))
                .rounded(ButtonRounded::None)
                .label("<")
                .compact()
                .on_click(move |_, _, cx| {
                    with_editor(cx, track_idx, slot_idx, |editor| {
                        editor.step_pitch(-1)
                    });
                }),
        )
        .child(
            Label::new(format!("PITCH: {:+}st", editor.pitch))
                .text_xs()
                .text_color(rgb(0x8A90A0)),
        )
        .child(
            Button::new(format!("wt_pitch_up_{track_idx}_{slot_idx}"))
                .rounded(ButtonRounded::None)
                .label(">")
                .compact()
                .on_click(move |_, _, cx| {
                    with_editor(cx, track_idx, slot_idx, |editor| {
                        editor.step_pitch(1)
                    });
                }),
        )
        .child(div().flex_1())
        .child(
            Button::new(format!("wt_voices_{track_idx}_{slot_idx}"))
                .rounded(ButtonRounded::None)
                .label(format!("VOICES: {}", editor.voices.label()))
                .compact()
                .on_click(move |_, _, cx| {
                    with_editor(cx, track_idx, slot_idx, |editor| {
                        editor.cycle_voices()
                    });
                }),
        )
        .child(div().flex_1())
        .child(
            Button::new(format!("wt_oct_down_{track_idx}_{slot_idx}"))
                .rounded(ButtonRounded::None)
                .label("<")
                .compact()
                .on_click(move |_, _, cx| {
                    with_editor(cx, track_idx, slot_idx, |editor| {
                        editor.step_octave(-1)
                    });
                }),
        )
        .child(
            Label::new(format!("OCT: {:+}", editor.octave))
                .text_xs()
                .text_color(rgb(0x8A90A0)),
        )
        .child(
            Button::new(format!("wt_oct_up_{track_idx}_{slot_idx}"))
                .rounded(ButtonRounded::None)
                .label(">")
                .compact()
                .on_click(move |_, _, cx| {
                    with_editor(cx, track_idx, slot_idx, |editor| {
                        editor.step_octave(1)
                    });
                }),
        )
        .into_any_element()
}


