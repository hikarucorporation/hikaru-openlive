// Copyright (C) Hikaru Corporation - 2026
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/src/hikaru_wrapper/input.rs

//! Política de entrada del envoltorio, sin X11.
//!
//! # El problema que resuelve
//!
//! Un editor VST3/CLAP no dibuja en la ventana que le da el host: crea una
//! **ventana hija** con su propio toolkit (JUCE, Qt, web…) y ahí recibe los
//! eventos. El envoltorio, en cambio, crea un `InputOutput` con
//! `EXPOSURE | KEY_PRESS | STRUCTURE_NOTIFY` y nada más.
//!
//! En X11, un evento se entrega a la ventana que lo *seleccionó*; si ninguna
//! de la cadena lo seleccionó, sube al primer ancestro que sí. Con la máscara
//! anterior hay dos consecuencias reales:
//!
//! 1. La ventana contenedora no selecciona puntero, así que **nada** se le
//!    entrega a ella y no hay forma de recuperar un gesto que la vista hija
//!    dejó sin tratar.
//! 2. El contenedor no declara `WM_HINTS.input`, y hay WMs que asumen que
//!    una tool-window no quiere teclado y **nunca le dan el foco**: el click se
//!    ve pero el control de abajo no entra en modo edición.
//!
//! La corrección tiene dos mitades, y por eso este módulo existe:
//!
//! - [`crate::hikaru_wrapper::x11::set_wm_hints_input`] pone el hint de
//!   teclado y pide la activación al abrir ([`InputActivation::OnOpen`]).
//! - La ventana contenedor selecciona **todo** el puntero y reenvía a la vista
//!   hija lo que caiga dentro de ella ([`InputDisposition::Forward`]).
//!
//! La segunda mitad sólo es un respaldo: una vista que selecciona sus eventos
//! los recibe directamente y el reenvío no se activa nunca (por eso
//! [`InputDisposition::Native`]). Existe para los toolkits que dibujan sobre
//! la ventana del host sin crear una hija, que entonces no tienen a nadie que
//! entregarle el click.

/// Cuándo se pide al gestor de ventanas que active la ventana.
///
/// La tentación es activar siempre, pero eso rompe el DAW: cada re-activación
/// le roba el foco mientras el usuario arrastra un fader. La política correcta
/// es activar **una vez**, al abrir, que es justo cuando el usuario viene de
/// clickear el botón `Editor` y espera que el foco vaya al editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputActivation {
    /// Activar al mapear la ventana, y sólo entonces (`_NET_ACTIVE_WINDOW`).
    ///
    /// Es lo que hace un Fruity Wrapper: al abrir, el foco pasa al editor.
    OnOpen,
    /// No pedir activación: la ventana se abre detrás del DAW y el foco se
    /// queda donde estaba.
    ///
    /// Para los plugins cuyo editor abre su propio diálogo modal (un selector
    /// de archivos, un prompt de licencia), donde robarle el foco al DAW a
    /// mitad de una edición es molesto.
    Never,
}

/// Bits de `EventMask` que el contenedor tiene que seleccionar.
///
/// [`crate::hikaru_plugin_host::hikaru_wrapper::window`] los aplica al crear
/// la ventana. La lista completa, y por qué cada grupo está:
///
/// - **Exposición**: repintar tras un `map` o al volver de minimizada. Sin
///   ella la ventana aparece con el contenido viejo del compositor.
/// - **Estructura**: `ConfigureNotify` (resize) y `DestroyNotify`. Es lo que
///   permite seguir al plugin cuando reorganiza su layout.
/// - **Foco**: `FocusChange` para saber si hay que reparar el foco.
/// - **Teclado**: `KeyPress`/`KeyRelease`, para teclas que el plugin no
///   capture y para atajos de la ventana.
/// - **Puntero**: el grupo que faltaba y que es la mitad del arreglo de
///   "los clicks no hacen nada". `ButtonPress`/`ButtonRelease` para los
///   clics, `ButtonMotion`/`MotionNotify` para los arrastres de knob,
///   `EnterWindow`/`LeaveWindow` para el hover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputMask {
    pub exposure: bool,
    pub structure_notify: bool,
    pub focus_change: bool,
    pub key_press: bool,
    pub key_release: bool,
    pub button_press: bool,
    pub button_release: bool,
    pub button_motion: bool,
    pub pointer_motion: bool,
    pub enter_window: bool,
    pub leave_window: bool,
}

impl Default for InputMask {
    /// Todo activado. Es la configuración del envoltorio: una ventana que
    /// embebe un editor no tiene ningún motivo para_no_ seleccionar eventos.
    fn default() -> Self {
        Self {
            exposure: true,
            structure_notify: true,
            focus_change: true,
            key_press: true,
            key_release: true,
            button_press: true,
            button_release: true,
            button_motion: true,
            pointer_motion: true,
            enter_window: true,
            leave_window: true,
        }
    }
}

impl InputMask {
    /// La máscara mínima que sirve para una ventana que sólo muestra.
    ///
    /// Existe como contraprueba: si el bit de puntero no estuviera en el
    /// default, este perfil no lo tendría y el default tampoco.
    pub fn display_only() -> Self {
        Self {
            exposure: true,
            structure_notify: true,
            focus_change: false,
            key_press: false,
            key_release: false,
            button_press: false,
            button_release: false,
            button_motion: false,
            pointer_motion: false,
            enter_window: false,
            leave_window: false,
        }
    }

    /// Si el contenedor recibe algún evento de puntero.
    ///
    /// Si da `false`, no hay nada que redirigir: los clicks sólo pueden
    /// llegar a la vista hija por la vía normal.
    pub fn selects_pointer(&self) -> bool {
        self.button_press
            || self.button_release
            || self.pointer_motion
            || self.button_motion
    }
}

/// Política completa de entrada del envoltorio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputPolicy {
    /// Máscara de eventos del contenedor.
    pub mask: InputMask,
    /// Cuándo activarlo.
    pub activation: InputActivation,
    /// Reparar el foco con `SetInputFocus` si el WM no lo concede.
    ///
    /// Es el respaldo de `WM_HINTS.input`: hay WMs que ignoran el hint y
    /// dejan la ventana sin enfocar. Comprobar el foco en cada tick y
    /// re-afirmarlo **sólo si el puntero está sobre la ventana** mantiene el
    /// arreglo sin robarle el foco al DAW.
    pub repair_focus: bool,
}

impl Default for InputPolicy {
    fn default() -> Self {
        Self {
            mask: InputMask::default(),
            activation: InputActivation::OnOpen,
            repair_focus: true,
        }
    }
}

impl InputPolicy {
    /// Perfil sin eventos de puntero: la ventana se ve pero no se toca.
    ///
    /// Es el modo "sólo referencia" — el caso de abrir un editor desde el
    /// panel de plugins para ver sus parámetros sin tocarlo mientras suena.
    pub fn view_only() -> Self {
        Self {
            mask: InputMask::display_only(),
            activation: InputActivation::Never,
            repair_focus: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_selects_the_whole_pointer() {
        let mask = InputMask::default();
        assert!(mask.button_press, "sin ButtonPress no hay click");
        assert!(mask.button_release, "sin ButtonRelease se quedan presionados");
        assert!(mask.pointer_motion, "sin MotionNotify no hay arrastre de knob");
        assert!(mask.button_motion);
        assert!(mask.enter_window && mask.leave_window, "el hover no llega");
        assert!(mask.key_press && mask.key_release);
        assert!(mask.selects_pointer());
    }

    #[test]
    fn view_only_selects_nothing_clickable() {
        let policy = InputPolicy::view_only();
        assert!(!policy.mask.selects_pointer());
        assert!(!policy.mask.button_press);
        assert!(!policy.repair_focus);
        assert_eq!(policy.activation, InputActivation::Never);
    }

    #[test]
    fn view_only_still_repaints_and_follows_resizes() {
        // Sacar el puntero no puede romper el redibujado: un editor que no
        // repinta ni sigue al resize es peor que uno que no responde al click.
        let mask = InputPolicy::view_only().mask;
        assert!(mask.exposure);
        assert!(mask.structure_notify);
    }

    #[test]
    fn default_repairs_focus_only_once_the_window_is_up() {
        let policy = InputPolicy::default();
        assert!(policy.repair_focus);
        assert_eq!(policy.activation, InputActivation::OnOpen);
    }
}