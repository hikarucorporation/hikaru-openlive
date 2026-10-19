// Copyright (C) Hikaru Corporation - 2026
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/src/hikaru_wrapper/mod.rs

//! Envoltorio tipo "Fruity Wrapper" para editores de plugin.
//!
//! # Qué resuelve
//!
//! Un editor VST3/CLAP abre su propia ventana nativa (X11, o XWayland cuando
//! la sesión es Wayland). Sin hints de WM, el gestor de ventanas la trata como
//! una aplicación independiente: es una *top-level* sin padre, así que en
//! cuanto el usuario clickea el DAW el WM la baja y la deja atrás del canvas
//! principal — o directamente en otro workspace. El plugin sigue vivo y con
//! todo su estado, pero para el usuario "desapareció".
//!
//! Aparte de eso hay dos fallos que sólo se ven con el editor abierto, y que
//! este módulo arregla de paso:
//!
//! - **El display no se mueve** (el espectro se queda en "Play audio to see
//!   the spectrum"). La causa no es el plugin ni el audio: es que el hilo de
//!   la ventana sólo llamaba `process()` cuando la cinta del motor entregaba un
//!   bloque, y la cinta está muda con el transporte parado. Sin `process()`
//!   el anillo de control de VST3 no se drena nunca. Ver [`event_loop`].
//! - **Los controles del editor no responden al click.** La ventana
//!   contenedor se creaba seleccionando `EXPOSURE | KEY_PRESS |
//!   STRUCTURE_NOTIFY`, sin un solo bit de puntero, y sin `WM_HINTS.input`.
//!   Ver [`input`] y [`x11::set_wm_hints_input`].
//!
//! # Cómo está partido
//!
//! | Módulo | Responsabilidad | ¿Habla con X11? |
//! |---|---|---|
//! | [`config`] | Qué hints se piden | no |
//! | [`input`] | Qué eventos se seleccionan y a quién se reenvían | no |
//! | [`x11`] | Escribir esos hints y paquetes en el servidor | sí |
//! | [`window`] | La ventana: conexión, XID, ciclo de vida | sí |
//! | [`event_loop`] | El tick de 60 Hz que la mantiene viva | vía [`window`] |
//!
//! La regla es que **la política es pura y testeable sin display**: todo lo
//! que se decide ("¿qué bits selecciono?", "¿este click va a la vista del
//! plugin?") vive en funciones sin servidor de display, y lo único que
//! habla con X11 es la ejecución de esas decisiones.
//!
//! # La solución de ventana
//!
//! Cuatro hints, aplicados por [`x11::apply_wrapper_hints`]:
//!
//! 1. `WM_TRANSIENT_FOR` → la ventana del DAW. El WM entonces la apila junto
//!    a su padre, la minimiza con él y le devuelve el foco al cerrar el padre.
//!    Sólo funciona si la sesión es X11: una superficie Wayland no se puede
//!    emparentar en X11, así que en Wayland puro este hint no se puede poner.
//! 2. `_NET_WM_STATE_ABOVE` → siempre por encima de las demás ventanas *de su
//!    nivel*. Es el hint que **sí** funciona en XWayland, y por eso es el que
//!    sostiene el comportamiento cuando el DAW corre en Wayland.
//! 3. `_NET_WM_WINDOW_TYPE_UTILITY` (o `DIALOG`) → le dice al WM que es una
//!    ventana auxiliar y no otra aplicación: no merece entrada propia en el
//!    taskbar, no se maximiza, y se cierra junto al padre.
//! 4. `WM_HINTS.input` → la ventana quiere teclado. Sin esto hay WMs que no
//!    le dan el foco nunca y el editor deja de responder al teclado.
//!
//! El hint 2 fija el estado *una vez*, en el momento del `map`. Si después el
//! usuario clickea el DAW, el WM vuelve a subir su propia ventana y la del
//! plugin queda debajo otra vez. Por eso existe también [`keep_alive`],
//! que la recuerda periódicamente desde el hilo de la ventana del plugin: es lo
//! que convierte "subió una vez" en "se queda arriba".
//!
//! # Re-raise sin robar el foco
//!
//! [`x11::raise`] usa `ConfigureWindow` con `stack-mode = Above`, que reordena
//! el stacking **sin** activar la ventana. Es deliberado: la alternativa
//! habitual (`_NET_ACTIVE_WINDOW`) también enfocaría el editor, y entonces el
//! usuario no podría clickear un knob del DAW — el DAW perdería el foco en el
//! mismo gesto que lo levantó. Re-elevar no debe robarle el foco al DAW.
//! La activación ocurre **una sola vez**, al abrir ([`input::InputActivation`]),
//! que es cuando el usuario viene de clickear el botón `Editor` y espera que
//! el foco vaya al editor.

mod config;
mod frame;
mod input;
mod keep_alive;
mod wrapper;

#[cfg(target_os = "linux")]
pub mod event_loop;
#[cfg(target_os = "linux")]
pub mod window;
#[cfg(target_os = "linux")]
pub mod x11;

pub use frame::{
    CursorHint, FrameMetrics, FrameRegion, InnerRect, ResizeDir,
};
pub use config::{
    plan_hints, HostFrame, HostRelation, WrapperConfig, WrapperHintPlan, WrapperStyle,
    WrapperWindowType,
};
pub use input::{InputActivation, InputMask, InputPolicy};
pub use keep_alive::WrapperKeepAlive;
#[cfg(target_os = "linux")]
pub use wrapper::{HikaruWrapper, WrapperError, WrapperRuntime};

#[cfg(target_os = "linux")]
pub use event_loop::{
    pump_plugin, FeedbackOrigin, LoopExit, ParamFeedback, TickBudget, WrapperCommand,
    WrapperEventLoop,
};
#[cfg(target_os = "linux")]
pub use window::{CreateError, HikaruWrapperWindow, WindowEvent};

/// Alias de [`window::CreateError`] con un nombre que no colisiona con el del
/// crate cuando alguien importa los dos con `use ...::*`.
#[cfg(target_os = "linux")]
pub use window::CreateError as WindowCreateError;