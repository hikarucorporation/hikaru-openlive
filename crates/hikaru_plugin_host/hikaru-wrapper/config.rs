// Copyright (C) Hikaru Corporation - 2026
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/src/hikaru_wrapper/config.rs

//! Configuración del envoltorio: qué se le pide al gestor de ventanas.
//!
//! La *decisión* vive acá, separada de la escritura al servidor X11 ([`crate::hikaru_wrapper::x11`]).
//! Es lo que permite ejercitar toda la política de hints en tests sin un
//! servidor de display.

use crate::HostParentKind;

/// Cómo se le presenta la ventana al gestor de ventanas.
///
/// No es cosmético: cambia cómo el WM la trata (tamaño, decoraciones, tarea,
/// agrupación con el padre).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapperWindowType {
    /// `_NET_WM_WINDOW_TYPE_UTILITY`: una tool-window del DAW. Es el valor
    /// correcto para un editor de plugin embebido —no es un diálogo modal, no
    /// pide atención aparte en el taskbar— y es el que deja ver las
    /// decoraciones normales (mover/redimensionar), que el usuario espera de
    /// una ventana de editor.
    Utility,
    /// `_NET_WM_WINDOW_TYPE_DIALOG`: variante modal, más restrictiva. Se deja
    /// disponible porque algunos WMs mini-apilan los diálogos de forma más
    /// consistente que las utility windows, pero a costa de no dejar redimensionar.
    Dialog,
}

/// ¿Quién pone el borde de la ventana?
///
/// # Por qué `Owned` es el default
///
/// El editor de un plugin VST3/CLAP en Linux es una ventana **X11** (JUCE y
/// Qt usan X11 siempre). La sesión del DAW es **Wayland** con XWayland al lado.
/// Un cliente X11 no se puede empotrar dentro de una superficie Wayland: no
/// existe equivalente a `XReparentWindow` entre los dos mundos, y XWayland
/// compone sus clientes como superficies propias.
///
/// O sea: no hay subsurface embedding posible mientras el DAW corra en Wayland.
/// Lo que sí se puede —y lo que hacen un Fruity Wrapper de verdad y también
/// DAWs como Reaper o Ableton— es que el editor sea una ventana **del DAW**,
/// no una ventana **del escritorio**:
///
/// - Sin decoraciones del gestor de ventanas: ni titlebar, ni bordes, ni
///   botones del sistema. El usuario no la mueve ni la redimensiona con el
///   ratón; la posiciona el DAW.
/// - Con `override_redirect`, que la saca de la gestión del WM y por lo tanto
///   de la lista de ventanas del usuario.
/// - Con posición y tamaño atados a los de la ventana del DAW.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapperStyle {
    /// Ventana de Hikaru: sin decoraciones, fuera del taskbar, posicionada por
    /// el DAW. Es el modo Fruity Wrapper.
    Owned,
    /// Ventana normal, con las decoraciones del gestor de ventanas.
    ///
    /// Existe para depuración: ver los bordes es la forma más rápida de
    /// confirmar dónde terminó el editor.
    Decorated,
}

/// Relación entre la ventana del editor y la del DAW.
///
/// "Modal estricta" no significa foco bloqueante: el editor toma el foco
/// mientras el usuario trabaja en él y lo devuelve al DAW cuando sale
/// (`InputFocus::PARENT`). Significa que la ventana del editor está **atada**
/// a la del DAW: se mueve con ella, se limita a su tamaño, se oculta cuando el
/// DAW se minimiza y no sobrevive a que el DAW muera.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostRelation {
    /// Atada a la ventana del DAW: sigue sus movimientos y se oculta con ella.
    StrictChild,
    /// Suelta. El editor se comporta como una ventana independiente.
    ///
    /// Sólo para depuración: reproduce el comportamiento roto que el
    /// envoltorio viene a arreglar.
    Detached,
}

/// Cuadro que ocupa el editor dentro de la ventana del DAW.
///
/// Coordenadas **de cliente** de la ventana del DAW (relativas a su esquina
/// interior), no a la raíz de la pantalla. El bucle las traduce a coordenadas
/// de raíz en cada tick, porque el DAW se puede mover mientras el editor está
/// abierto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostFrame {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

impl HostFrame {
    pub fn new(x: i16, y: i16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Si el cuadro tiene un tamaño utilizable.
    ///
    /// Un cuadro de 0×0 es la forma en que un layout todavía no calculó su
    /// editor; mandarle eso al plugin deja su vista en tamaño cero.
    pub fn is_usable(&self) -> bool {
        self.width > 0 && self.height > 0
    }

    /// El cuadro limitado al tamaño actual del DAW.
    ///
    /// Hace falta porque el DAW se puede hacer más chico que el editor: sin
    /// esto la ventana del plugin se sale de la pantalla.
    pub fn clamp_to(&self, host_width: u16, host_height: u16) -> Self {
        Self {
            x: self.x.clamp(0, host_width.saturating_sub(1) as i16),
            y: self.y.clamp(0, host_height.saturating_sub(1) as i16),
            width: self.width.min(host_width),
            height: self.height.min(host_height),
        }
    }
}

/// Configuración del envoltorio de un editor de plugin.
///
/// Los valores por defecto reproducen el comportamiento de Fruity Wrapper:
/// la ventana del plugin es una tool-window del DAW, siempre encima, que se
/// abre y cierra con él.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrapperConfig {
    /// Ventana del DAW a la que atarse como padre transitorio.
    ///
    /// `HostParentKind::Wayland` (el caso de una sesión Wayland pura) no
    /// aporta ningún XID, así que [`WrapperConfig::transient_for`] devuelve
    /// `None` y el resto de los hints siguen aplicándose: el comportamiento se
    /// sostiene sobre `_NET_WM_STATE_ABOVE`.
    pub parent: HostParentKind,

    /// Pedir `_NET_WM_STATE_ABOVE`: la ventana queda por encima del resto.
    pub always_on_top: bool,

    /// Atarse al padre (`WM_TRANSIENT_FOR`) para que el WM la apile,
    /// minimice y enfoque junto a la ventana del DAW.
    ///
    /// Desactivarlo deja la ventana como top-level independiente, que es
    /// justo el comportamiento roto que este módulo viene a arreglar.
    pub keep_focused_with_parent: bool,

    /// Tipo de ventana que se pide al WM. Ver [`WrapperWindowType`].
    pub window_type: WrapperWindowType,

    /// Sacar la ventana del taskbar y del pager.
    ///
    /// Encaja con el modelo "el editor pertenece al DAW": apareciendo en el
    /// taskbar, el usuario la trata como una app más y el WM la baja al
    /// cambiar de ventana.
    pub skip_taskbar: bool,

    /// Pedir el foco de teclado en la ventana (`WM_HINTS.input = True`).
    ///
    /// Sin esto el WM trata la ventana como una tool-window que no quiere
    /// entrada de teclado y hay WMs que **no le dan el foco** aunque el
    /// usuario la clickee. Para un editor de plugin eso se traduce en clickes
    /// que se ven pero no activan el control que tienen debajo (un knob entra
    /// en modo edición y nunca lo suelta; un desplegable abre y no elige).
    ///
    /// Ver [`Self::keyboard_input`] para la política completa.
    pub want_keyboard_focus: bool,

    /// Si la ventana es de Hikaru (`Owned`) o del escritorio (`Decorated`).
    pub style: WrapperStyle,

    /// Si la ventana sigue a la del DAW o anda suelta.
    pub relation: HostRelation,

    /// Cuadro del editor dentro de la ventana del DAW.
    ///
    /// `None` = centrar en la pantalla (el modo de depuración, el que no
    /// necesita saber nada del DAW). `Some` = poner el editor donde el DAW
    /// pida. Sólo tiene efecto con [`HostRelation::StrictChild`].
    pub host_frame: Option<HostFrame>,
}

impl Default for WrapperConfig {
    fn default() -> Self {
        Self {
            parent: HostParentKind::Unknown,
            always_on_top: true,
            keep_focused_with_parent: true,
            window_type: WrapperWindowType::Utility,
            skip_taskbar: true,
            want_keyboard_focus: true,
            style: WrapperStyle::Owned,
            relation: HostRelation::StrictChild,
            host_frame: None,
        }
    }
}

impl WrapperConfig {
    /// Configuración por defecto atada al handle nativo del DAW.
    pub fn with_parent(parent: HostParentKind) -> Self {
        Self {
            parent,
            ..Self::default()
        }
    }

    /// Coloca el editor en un cuadro concreto de la ventana del DAW.
    ///
    /// Lo llama la vista del rack cuando el layout ya sabe dónde va a estar
    /// el editor (por ejemplo, el panel flotante del DAW). Devuelve `self`
    /// para encadenar.
    pub fn at(mut self, frame: HostFrame) -> Self {
        self.host_frame = Some(frame);
        self
    }

    /// Devuelve el editor a centrar en la pantalla.
    pub fn floating(mut self) -> Self {
        self.host_frame = None;
        self.relation = HostRelation::Detached;
        self.style = WrapperStyle::Decorated;
        self
    }

    /// XID al que hay que poner `WM_TRANSIENT_FOR`, o `None` si esta
    /// configuración no puede emparentarse.
    ///
    /// `None` en tres casos, y sólo en esos tres:
    ///
    /// - `keep_focused_with_parent` está apagado (es lo que pidió el usuario).
    /// - El padre no es X11/XWayland (Wayland puro, o handle desconocido).
    /// - El XID del DAW no resolvió geometría y la ventana quedó top-level.
    ///   Eso lo decide el hilo de la ventana, no la config.
    pub fn transient_for(&self) -> Option<u32> {
        if self.keep_focused_with_parent {
            self.parent.x11_id()
        } else {
            None
        }
    }

    /// Si hay que instalar `WM_HINTS` con `input = True`.
    ///
    /// Un hint más, pero es el que hace que los WMs traten la ventana como
    /// una ventana *de verdad* en lugar de una tool-window decorativa.
    pub fn keyboard_input(&self) -> bool {
        self.want_keyboard_focus
    }

    /// Si la ventana se crea con `override_redirect`.
    ///
    /// Es lo que la saca de la gestión del gestor de ventanas y por lo tanto
    /// de sus decoraciones. Con [`WrapperStyle::Decorated`] se deja de
    /// memakai: ahí el usuario quiere una ventana de verdad, con bordes y con
    /// la capacidad de moverla.
    pub fn override_redirect(&self) -> bool {
        self.style == WrapperStyle::Owned
    }

    /// Si la ventana tiene que seguir a la del DAW.
    pub fn strict_child(&self) -> bool {
        self.relation == HostRelation::StrictChild && self.host_frame.is_some()
    }
}

/// Los hints concretos que se van a escribir, derivados de la configuración.
///
/// Existe separado de la escritura al servidor para que la *decisión* sea
/// verificable sin X11: los tests corrigen sobre esta función y no sobre
/// llamadas de red, que fallarían en CI sin un servidor de display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrapperHintPlan {
    /// Destino de `WM_TRANSIENT_FOR`. `None` = no se emparenta.
    pub transient_for: Option<u32>,
    /// Valor para `_NET_WM_WINDOW_TYPE`.
    pub window_type: WrapperWindowType,
    /// Átomos para `_NET_WM_STATE`, en el orden en que se escriben.
    pub states: Vec<&'static str>,
    /// Si hay que instalar `WM_HINTS.input = True`.
    pub keyboard_input: bool,
    /// Si la ventana se crea con `override_redirect` (sin decoraciones).
    pub override_redirect: bool,
    /// Si la ventana sigue a la del DAW en cada tick.
    pub strict_child: bool,
    /// Si el usuario pidió que el editor quede arriba del DAW.
    pub always_on_top: bool,
}

impl WrapperHintPlan {
    /// Si hay algún hint de "que se quede arriba" activo.
    ///
    /// Si da `false` el hilo de la ventana no tiene nada que re-assertar cada
    /// tanto, y el keep-alive queda como no-op.
    ///
    /// Hace falta apilado en dos situaciones distintas:
    ///
    /// - La ventana la gestiona el WM y tiene `ABOVE`: el hint se aplicó una
    ///   vez, pero el WM lo reevalúa solo y cada vez que el usuario clickea el
    ///   DAW lo pierde. El keep-alive lo vuelve a afirmar.
    /// - La ventana es `override_redirect`: el WM **no** la gestiona, así que
    ///   `_NET_WM_STATE_ABOVE` no existe de hecho y el único apilado posible es
    ///   `ConfigureWindow(stack=Above)` en cada tick. Acá el keep-alive es el
    ///   único mecanismo, no un refuerzo.
    ///
    /// Si el usuario apagó `always_on_top`, no hace falta ninguno de los dos.
    pub fn needs_raising(&self) -> bool {
        self.always_on_top
            && (self.override_redirect || self.states.iter().any(|s| *s == "_NET_WM_STATE_ABOVE"))
    }
}

/// Traduce la configuración al plan de hints que se va a escribir.
pub fn plan_hints(config: &WrapperConfig) -> WrapperHintPlan {
    let mut states = Vec::new();
    if config.always_on_top {
        states.push("_NET_WM_STATE_ABOVE");
    }
    if config.skip_taskbar {
        states.push("_NET_WM_STATE_SKIP_TASKBAR");
        states.push("_NET_WM_STATE_SKIP_PAGER");
    }

    let window_type = config.window_type;

    WrapperHintPlan {
        transient_for: config.transient_for(),
        window_type,
        states,
        keyboard_input: config.keyboard_input(),
        override_redirect: config.override_redirect(),
        strict_child: config.strict_child(),
        always_on_top: config.always_on_top,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> WrapperConfig {
        WrapperConfig::default()
    }

    #[test]
    fn defaults_are_the_fruity_wrapper_semantics() {
        let c = config();
        assert!(c.always_on_top, "por defecto el editor va siempre arriba");
        assert!(c.keep_focused_with_parent);
        assert_eq!(c.window_type, WrapperWindowType::Utility);
        assert!(
            c.keyboard_input(),
            "una ventana de editor tiene que pedir foco de teclado"
        );
    }

    #[test]
    fn por_defecto_la_ventana_es_de_hikaru_no_del_escritorio() {
        let c = config();
        assert_eq!(c.style, WrapperStyle::Owned);
        assert!(
            c.override_redirect(),
            "sin override_redirect el gestor de ventanas le pone titlebar y bordes"
        );
        assert_eq!(c.relation, HostRelation::StrictChild);
        // Sin cuadro del DAW todavía no hay nada a lo que atarse, así que no
        // setracks: hasta que el rack diga dónde va el editor, se centra.
        assert!(!c.strict_child());
    }

    #[test]
    fn un_cuadro_del_daw_activa_el_seguimiento() {
        let c = config().at(HostFrame::new(10, 20, 800, 600));
        assert!(c.strict_child());
        assert!(plan_hints(&c).strict_child);
    }

    #[test]
    fn el_modo_flotante_deshace_los_dos_modos_de_la_ventana() {
        let c = config().at(HostFrame::new(10, 20, 800, 600)).floating();
        assert_eq!(c.style, WrapperStyle::Decorated);
        assert_eq!(c.relation, HostRelation::Detached);
        assert!(c.host_frame.is_none());
        assert!(!c.override_redirect());
        assert!(!c.strict_child());
    }

    #[test]
    fn una_ventana_sin_decoraciones_necesita_apilamiento_del_tick() {
        // Con override_redirect el WM no gestiona la ventana: `_NET_WM_STATE_ABOVE`
        // no se pone de hecho, así que el keep-alive es el unico mecanismo que
        // la deja arriba.
        let plan = plan_hints(&config());
        assert!(plan.override_redirect);
        assert!(plan.needs_raising());
    }

    #[test]
    fn una_ventana_sin_decoraciones_tampoco_se_apila_si_el_usuario_no_quiso() {
        let c = WrapperConfig {
            always_on_top: false,
            ..config()
        };
        let plan = plan_hints(&c);
        assert!(plan.override_redirect);
        assert!(
            !plan.needs_raising(),
            "`always_on_top: false` manda sobre el modo de ventana"
        );
    }

    #[test]
    fn un_cuadro_chico_o_iluminado_no_es_usable() {
        assert!(!HostFrame::new(0, 0, 0, 0).is_usable());
        assert!(!HostFrame::new(0, 0, 800, 0).is_usable());
        assert!(HostFrame::new(0, 0, 1, 1).is_usable());
    }

    #[test]
    fn el_cuadro_se_limita_al_tamanio_del_daw() {
        // El DAW se puede hacer mas chico que el editor; sin limitar, la
        // ventana del plugin se sale de la pantalla.
        let frame = HostFrame::new(50, 60, 800, 600).clamp_to(400, 300);
        assert_eq!(frame.width, 400);
        assert_eq!(frame.height, 300);
        assert_eq!(frame.x, 50);
        assert_eq!(frame.y, 60);

        // Y un DAW de 1x1 no puede dejar el cuadro en negativo.
        let tiny = HostFrame::new(100, 100, 800, 600).clamp_to(1, 1);
        assert_eq!((tiny.x, tiny.y, tiny.width, tiny.height), (0, 0, 1, 1));
    }

    #[test]
    fn plan_sets_above_and_skips_taskbar_by_default() {
        let plan = plan_hints(&config());
        assert_eq!(
            plan.states,
            vec![
                "_NET_WM_STATE_ABOVE",
                "_NET_WM_STATE_SKIP_TASKBAR",
                "_NET_WM_STATE_SKIP_PAGER"
            ]
        );
        assert!(plan.needs_raising());
    }

    #[test]
    fn plan_carries_the_keyboard_hint() {
        assert!(plan_hints(&config()).keyboard_input);

        let c = WrapperConfig {
            want_keyboard_focus: false,
            ..config()
        };
        assert!(!plan_hints(&c).keyboard_input);
    }

    #[test]
    fn plan_drops_above_when_always_on_top_is_off() {
        let c = WrapperConfig {
            always_on_top: false,
            ..config()
        };
        let plan = plan_hints(&c);
        assert!(!plan.states.contains(&"_NET_WM_STATE_ABOVE"));
        // Con `always_on_top` apagado no se apila de ninguna manera: ni el
        // WM lo hace, ni el tick lo fuerza.
        assert!(!plan.needs_raising());
        // El resto sigue: apagar `above` no es abandonar el envoltorio.
        assert!(plan.states.contains(&"_NET_WM_STATE_SKIP_TASKBAR"));
    }

    #[test]
    fn x11_parent_becomes_transient_for() {
        let c = WrapperConfig::with_parent(HostParentKind::Xlib(0x1234));
        assert_eq!(plan_hints(&c).transient_for, Some(0x1234));
    }

    #[test]
    fn wayland_parent_has_no_xid_so_no_transient_for() {
        // El caso real de una sesión Wayland: la ventana del DAW es una
        // superficie Wayland y no hay XID que emparentar. El plan NO puede
        // poner `WM_TRANSIENT_FOR`...
        let c = WrapperConfig::with_parent(HostParentKind::Wayland);
        let plan = plan_hints(&c);
        assert_eq!(plan.transient_for, None);
        // ...pero el comportamiento de "quedarse arriba" sí se sostiene, vía
        // `_NET_WM_STATE_ABOVE` + el keep-alive.
        assert!(plan.needs_raising());
    }

    #[test]
    fn keep_focused_with_parent_off_forces_top_level() {
        let c = WrapperConfig {
            parent: HostParentKind::Xlib(0x1234),
            keep_focused_with_parent: false,
            ..config()
        };
        assert_eq!(plan_hints(&c).transient_for, None);
        // `ABOVE` sobrevive: es el único mecanismo que funciona en Wayland.
        assert!(plan_hints(&c).needs_raising());
    }
}