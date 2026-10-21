// crates/hikaru_gui/src/views/modal.rs

//! La capa modal: el fondo que se come los clics, y las ventanas flotantes
//! que el usuario puede arrastrar.
//!
//! # Por qué hace falta el fondo
//!
//! En GPUI el mouse no se despacha a "la ventana activa": se despacha a los
//! **hitboxes** que quedan debajo del puntero, y el evento viaja de lo que
//! está más adelante en la pantalla hacia lo que está más atrás, salvo que
//! algún handler llame a `cx.stop_propagation()`.
//!
//! Un panel que se dibuja encima del fondo, pero no tapa sus hitboxes,
//! deja que el clic siga bajando: el `on_mouse_down` de un pad de la Session
//! Matrix se dispara aunque el usuario haya clicado un texto de la modal.
//! Eso es el "click-through" que enciende clips y escenas detrás del panel.
//!
//! Un detalle que hace el bug más fácil de reproducir de lo que parece: en
//! GPUI un `div` **sin handlers de mouse no genera hitbox**. El rótulo de una
//! modal es un `Label` (que no tiene hitbox) y el root del panel tampoco la
//! tenía, así que el clic 이어 al que el usuario veía, atravesaba el panel
//! y terminaba en el pad que estaba debajo.
//!
//! # La receta
//!
//! Una modal de verdad tiene dos piezas:
//!
//! 1. Un **backdrop** que cubre el 100% del viewport y se come los clics.
//! 2. El **root de la modal**, que también para la propagación, para que los
//!    clics dentro del diálogo no le lleguen al backdrop (y, por lo tanto, al
//!    fondo de la app).
//!
//! Este módulo implementa las dos, para que ningún panel tenga que acordarse
//! de hacerlo a mano (y para que todos se comporten igual).
//!
//! # Las ventanas flotantes
//!
//! Además, las modales de configuración son **arrastrables**: el usuario las
//! mueve agarrando su barra de título. La máquina del gesto vive acá (ver
//! [`ModalDrag`], [`drag_handle`] y [`drag_tracking_keepalive`]) para que
//! todas las ventanas se comporten igual y no haya que reinventar el arrastre
//! en cada panel.

use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, update_state, AppState};

/// Los botones del mouse que una modal tiene que tragarse.
///
/// No sólo el izquierdo: el derecho abre los menús contextuales de la Session
/// Matrix y el del medio hace zoom/pan en la Playlist. Una modal que sólo
/// bloquea el click izquierdo deja pasar a los otros dos.
const MOUSE_BUTTONS: [MouseButton; 3] = [
    MouseButton::Left,
    MouseButton::Right,
    MouseButton::Middle,
];

/// Le cuelga a un elemento los handlers que se tragan el mouse.
///
/// Cada handler corta la propagación (`stop_propagation`), así que el evento
/// no sigue bajando hacia los hitboxes que están detrás: en la Session
/// Matrix, un clic deja de encender el clip o la escena que está debajo.
fn absorb_mouse<T: InteractiveElement>(mut el: T) -> T {
    for button in MOUSE_BUTTONS {
        el = el.on_mouse_down(button, |_, _, cx| cx.stop_propagation());
    }
    el
}

/// El fondo de una modal.
///
/// Cubre el 100% del viewport de la ventana principal y se come los clics:
/// mientras la modal está abierta, la Session Matrix, el playlist y el mixer
/// dejan de recibir eventos de puntero.
///
/// El `id` tiene que ser único por modal: GPUI aborta si dos elementos del
/// mismo frame comparten id.
pub fn backdrop(element_id: &'static str) -> AnyElement {
    absorb_mouse(
        div()
            .id(element_id)
            .absolute()
            .inset_0()
            .w_full()
            .h_full()
            // `occlude` le dice a GPUI que los hitboxes de atrás no cuentan
            // como "hovered": sin esto el fondo seguiría mostrando sus
            // highlights de hover y seguiría recibiendo los `on_mouse_move`
            // (el gesto de pan/zoom del playlist, por ejemplo).
            .occlude(),
    )
    .into_any_element()
}

/// Envuelve el panel de una modal con su backdrop.
///
/// El contenedor cubre toda la ventana, así que el panel (que es `absolute`)
/// sigue quedándose donde estaba: su posición es relativa al contenedor, y
/// el contenedor es la ventana entera.
pub fn overlay(element_id: &'static str, panel: AnyElement) -> AnyElement {
    div()
        .absolute()
        .inset_0()
        .child(backdrop(element_id))
        .child(panel)
        .into_any_element()
}

/// El root de una modal.
///
/// Le pone los handlers que paran la propagación: los clics dentro del
/// diálogo se quedan ahí y no bajan al backdrop (ni, por lo tanto, al fondo
/// de la app).
pub trait ModalRoot: Sized {
    /// Deja el elemento como raíz de modal: absorbe los clics.
    fn absorb_clicks(self) -> Self;
}

impl ModalRoot for Stateful<Div> {
    fn absorb_clicks(self) -> Self {
        // `on_click` además del `on_mouse_down`: el click completo (el par
        // down/up) es lo que dispara los `on_click` de los controles que
        // están detrás, y esos tampoco tienen que activarse.
        absorb_mouse(self).on_click(|_, _, cx| cx.stop_propagation())
    }
}

// =========================================================================
// VENTANAS FLOTANTES: LAS MODALES QUE SE ARRASTRAN
// =========================================================================

/// Cuál de las ventanas modales es.
///
/// Cada una lleva su propio desplazamiento: arrastrar una no mueve a las
/// demás.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalId {
    /// La ventana «About».
    About,
    /// La ventana «CONFIGURACIÓN DE AUDIO & HARDWARE».
    AudioSettings,
    /// La ventana «PLUGINS EXTERNOS · VST3 / CLAP».
    PluginSettings,
    /// La ventana «HIKARU MCP PORT · MODEL CONTEXT PROTOCOL».
    McpPortSettings,
}

/// El desplazamiento de una ventana modal y el gesto de arrastre en curso.
///
/// # Por qué el delta es incremental
///
/// El seguimiento del gesto no puede vivir en el `on_mouse_move` del asa: los
/// listeners de elemento de GPUI sólo se despachan mientras el hitbox del
/// elemento está bajo el cursor (`div.rs`: el closure arranca con
/// `if phase == DispatchPhase::Bubble && hitbox.is_hovered(window)`), y la
/// barra de título es una franja finita: en cuanto el puntero se adelantaba
/// al panel —que se mueve con el frame, no con el evento— el gesto se
/// congelaba a mitad de camino.
///
/// Por eso el seguimiento es a nivel de ventana (ver
/// [`drag_tracking_keepalive`]) y el desplazamiento se calcula como el delta
/// respecto del **último punto conocido**. Con esa cuenta, el mismo evento
/// aplicarse dos veces (GPUI despacha los eventos de mouse en fase de
/// captura y en fase de burbuja) no mueve la ventana dos veces: la segunda
/// pasada ve que el puntero no se movió y no hace nada.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ModalDrag {
    /// Desplazamiento acumulado (px) respecto de la posición base del panel.
    offset: (f32, f32),
    /// ¿Hay un arrastre en curso?
    dragging: bool,
    /// Última posición del puntero conocida (coordenadas de ventana).
    pointer: (f32, f32),
}

impl ModalDrag {
    /// El desplazamiento acumulado del panel, en px.
    pub fn offset(&self) -> (f32, f32) {
        self.offset
    }

    /// ¿Se está arrastrando la ventana ahora mismo?
    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    /// Empieza el gesto: memoriza dónde está el puntero.
    pub fn begin(&mut self, x: f32, y: f32) {
        self.dragging = true;
        self.pointer = (x, y);
    }

    /// Mueve el panel con el puntero.
    ///
    /// El delta es respecto del último punto conocido, no del punto de
    /// agarre: así el panel sigue al puntero aunque el mismo evento se
    /// despache dos veces.
    pub fn drag_to(&mut self, x: f32, y: f32) {
        if !self.dragging {
            return;
        }
        let (px, py) = self.pointer;
        // El puntero no se movió: este evento ya se aplicó (o no había nada
        // que aplicar).
        if px == x && py == y {
            return;
        }
        self.offset.0 += x - px;
        self.offset.1 += y - py;
        self.pointer = (x, y);
    }

    /// Termina el gesto.
    pub fn end(&mut self) {
        self.dragging = false;
    }
}

/// El estado de arrastre de todas las ventanas modales.
///
/// Vive en el estado global (`AppState::modal_drags`) y no en cada panel
/// porque el gesto tiene que sobrevivir a los `notify` del render: el panel
/// se reconstruye en cada frame y un flag local moriría con el primer
/// `notify`, cortando el arrastre tras un solo paso.
#[derive(Clone, Copy, Debug, Default)]
pub struct ModalDrags {
    about: ModalDrag,
    audio_settings: ModalDrag,
    plugin_settings: ModalDrag,
    mcp_port_settings: ModalDrag,
}

impl ModalDrags {
    /// El estado de arrastre de una modal.
    pub fn get(&self, id: ModalId) -> &ModalDrag {
        match id {
            ModalId::About => &self.about,
            ModalId::AudioSettings => &self.audio_settings,
            ModalId::PluginSettings => &self.plugin_settings,
            ModalId::McpPortSettings => &self.mcp_port_settings,
        }
    }

    /// El estado de arrastre de una modal (mutable).
    pub fn get_mut(&mut self, id: ModalId) -> &mut ModalDrag {
        match id {
            ModalId::About => &mut self.about,
            ModalId::AudioSettings => &mut self.audio_settings,
            ModalId::PluginSettings => &mut self.plugin_settings,
            ModalId::McpPortSettings => &mut self.mcp_port_settings,
        }
    }

    /// La modal que se está arrastrando, si hay alguna.
    pub fn dragging_mut(&mut self) -> Option<&mut ModalDrag> {
        [
            &mut self.about,
            &mut self.audio_settings,
            &mut self.plugin_settings,
            &mut self.mcp_port_settings,
        ]
        .into_iter()
        .find(|drag| drag.dragging)
    }

    /// ¿Hay alguna modal arrastrándose?
    pub fn is_dragging(&self) -> bool {
        self.get(ModalId::About).dragging
            || self.get(ModalId::AudioSettings).dragging
            || self.get(ModalId::PluginSettings).dragging
            || self.get(ModalId::McpPortSettings).dragging
    }

    /// Olvida el gesto y el desplazamiento de una modal.
    ///
    /// Se llama al cerrarla: la próxima vez que abra, arranca en su posición
    /// base (y no en un lugar de la pantalla que el usuario ya no recuerda).
    pub fn reset(&mut self, id: ModalId) {
        *self.get_mut(id) = ModalDrag::default();
    }
}

/// Empieza el arrastre de una ventana modal.
fn begin_drag(cx: &mut gpui_kit::App, id: ModalId, x: f32, y: f32) {
    update_state(cx, |state| {
        state.modal_drags.get_mut(id).begin(x, y);
    });
}

/// Mueve la ventana modal que se esté arrastrando.
///
/// No notifica si el panel no se movió de verdad: el puntero puede generar
/// movimientos que no cambian nada (por ejemplo, el segundo despacho del
/// mismo evento) y repintar la ventana entera para eso es tirar cuadros.
fn drag_move(cx: &mut gpui_kit::App, x: f32, y: f32) {
    let st = state(cx);
    if !st.read(cx).modal_drags.is_dragging() {
        return;
    }
    let mut moved = false;
    st.update(cx, |state, _| {
        if let Some(drag) = state.modal_drags.dragging_mut() {
            let before = drag.offset();
            drag.drag_to(x, y);
            moved = drag.offset() != before;
        }
    });
    if moved {
        cx.notify(st.entity_id());
    }
}

/// Termina el arrastre de la ventana modal que se esté arrastrando.
fn end_drag(cx: &mut gpui_kit::App) {
    let st = state(cx);
    if !st.read(cx).modal_drags.is_dragging() {
        return;
    }
    st.update(cx, |state, _| {
        if let Some(drag) = state.modal_drags.dragging_mut() {
            drag.end();
        }
    });
    cx.notify(st.entity_id());
}

/// Le cuelga a la barra de título de una modal los handlers de arrastre.
///
/// El elemento devuelto es el **asa**: el usuario la agarra y arrastra. La
/// barra de título tiene que ser el asa (y no el panel entero) para que los
/// botones que viven en ella —el `✕` de cerrar— sigan respondiendo.
///
/// `dragging` es el estado del gesto en el momento del render: sólo cambia el
/// cursor (de «mano abierta» a «mano cerrada»).
///
/// El seguimiento del gesto, en cambio, es a nivel de ventana (ver
/// [`drag_tracking_keepalive`]): si viviera en el asa, el gesto moriría en
/// cuanto el puntero saliera de la franja del título.
pub fn drag_handle<T: StatefulInteractiveElement + Styled>(
    mut el: T,
    id: ModalId,
    dragging: bool,
) -> T {
    el = el
        // Cursor de «agarrar»: la mano abierta invitation a mover la ventana.
        .cursor(if dragging {
            CursorStyle::ClosedHand
        } else {
            CursorStyle::OpenHand
        })
        .on_mouse_down(MouseButton::Left, move |event, _, cx| {
            begin_drag(
                cx,
                id,
                event.position.x.as_f32(),
                event.position.y.as_f32(),
            );
        })
        .on_mouse_up(MouseButton::Left, move |_, _, cx| {
            end_drag(cx);
        });
    el
}

/// Registra el seguimiento del arrastre a nivel de ventana.
///
/// # Por qué no `on_mouse_move` del asa
///
/// Los listeners de elemento de GPUI Kit se despachan **sólo si el hitbox del
/// elemento está bajo el cursor**. Con el seguimiento en la barra de título,
/// en cuanto el cursor salía de ella el gesto se congelaba a mitad de camino
/// —y el `mouse_up` también se perdía, dejando el arrastre colgado para el
/// movimiento de mouse que viniera después—.
///
/// `Window::on_mouse_event` no lleva esa condición: el gesto sigue aunque el
/// cursor salga de la barra, del panel o de la ventana.
///
/// # Por qué se registra siempre
///
/// `Window::on_mouse_event` borra sus listeners en cada frame renderizado y
/// sólo admite registrarse en fase de paint, así que no se puede registrar
/// desde el `mouse_down`. Por eso el registro va en el paint del canvas
/// (ver [`drag_tracking_keepalive`]) y no se condiciona a que haya un
/// arrastre en curso: el listener de siempre es el que atiende el primer
/// movimiento.
fn register_drag_tracking(window: &mut gpui_kit::Window) {
    window.on_mouse_event(move |event: &MouseMoveEvent, _phase, _window, cx| {
        drag_move(cx, event.position.x.as_f32(), event.position.y.as_f32());
    });
    window.on_mouse_event(move |event: &MouseUpEvent, _phase, _window, cx| {
        // Sólo el izquierdo cierra el gesto: el derecho abre menús
        // contextuales y no debe abortar un arrastre que sigue vivo.
        if event.button == MouseButton::Left {
            end_drag(cx);
        }
    });
}

/// El canvas que mantiene vivo el seguimiento del arrastre de las modales.
///
/// Se monta una vez, en la raíz de la app: las ventanas modales comparten el
/// mismo seguimiento (el gesto es de una sola ventana a la vez, así que un
/// único listener alcanza y evita que dos ventanas se muevan a la vez).
///
/// El invariante que sostiene el gesto: si se renderizó un frame, el paint de
/// este canvas corrió y el listener quedó registrado de nuevo; y si no se
/// renderizó ningún frame, entonces no se borró ningún listener. No hay forma
/// de quedarse sin seguimiento a mitad de arrastre.
///
/// No tiene hitbox (no registra interactividad), así que no intercepta el
/// mouse de la barra de título ni de los botones de las modales.
pub fn drag_tracking_keepalive() -> AnyElement {
    canvas(
        |_bounds, _window, _cx| {},
        move |_bounds, _state, window, _cx| {
            register_drag_tracking(window);
        },
    )
    .absolute()
    .inset_0()
    .into_any_element()
}

/// La posición (px) de una modal, ya con el desplazamiento del arrastre.
///
/// El desplazamiento se suma a la posición base: la ventana siempre nace en
/// el mismo lugar y el arrastre la mueve desde ahí.
pub fn modal_position(base_left: f32, base_top: f32, offset: (f32, f32)) -> (f32, f32) {
    (base_left + offset.0, base_top + offset.1)
}

/// El desplazamiento de una modal, leído del estado global.
///
/// Atajo para las vistas: no hace falta que cada panel sepa cómo se llama el
/// campo del estado que guarda su arrastre.
pub fn drag_offset(cx: &App, id: ModalId) -> (f32, f32) {
    state(cx).read(cx).modal_drags.get(id).offset()
}
