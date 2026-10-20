// Copyright (C) Hikaru Corporation - 2026
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/src/xlib.rs

//! Xlib y el servidor X: lo que hay que dejar en orden antes de que un
//! plugin abra su editor.
//!
//! Son dos problemas distintos, ambos con la misma raíz: **el editor de un
//! plugin es un proceso ajeno que habla con el servidor X por su cuenta**.
//!
//! # 1. `XInitThreads()`: Xlib tiene que saber que el proceso es multihilo
//!
//! Un editor de plugin no dibuja en la ventana que le da el host: crea su
//! propia ventana con su propio toolkit y **se levanta un hilo propio** para
//! bombear los eventos de X11. JUCE, por ejemplo, levanta un hilo que llama
//! a `XNextEvent` en loop y atiende ahí el `Expose`, el `ConfigureNotify` y
//! el repintado.
//!
//! O sea: desde el momento en que el plugin abre su editor hay **dos hilos
//! hablando con la misma librería Xlib**: el del toolkit y el del host, que
//! está llamando a `IPlugView::attached`.
//!
//! Xlib, por diseño, **no es seguro entre hilos** salvo que se llame a
//! `XInitThreads()` antes de cualquier otra función de Xlib. Sin eso, los dos
//! hilos escriben en el mismo socket sin serializar, las respuestas del
//! servidor se desalinean y aparecen dos síntomas:
//!
//! - **Deadlock.** El hilo de X11 del plugin queda esperando una respuesta
//!   que el otro hilo ya se llevó, y el hilo del host queda esperando al
//!   primero. La apertura del editor se cuelga para siempre, con la ventana
//!   a medio hacer: el contenedor existe, el plugin llegó a crear su
//!   ventana de 1×1 y no pasó de ahí.
//! - **Corrupción silenciosa.** Las respuestas se mezclan y cada hilo se
//!   cree dueño de la del otro.
//!
//! # 2. XInput2 no sirve bajo XWayland: el ratón del plugin queda muerto
//!
//! Los toolkits modernos (JUCE 7 en adelante) leen el ratón por **XInput2**
//! en vez de por los eventos del núcleo de X11, y lo hacen así a propósito:
//! XI2 distingue el ratón real de los "ratones emulados" por pantalla táctil.
//!
//! El problema es que **XWayland no entrega los eventos XI2 a los clientes
//! X11**: la extensión está (`XQueryExtension` responde que sí, y
//! `XIQueryVersion` devuelve 2.2), la selección de eventos también se acepta
//! (`XISelectEvents` responde `Success`), pero los eventos nunca llegan.
//! En el servidor X.org de verdad sí llegan, así que el toolkit no tiene
//! forma de saber que está en un entorno donde no van a llegar.
//!
//! Y el toolkit no tiene plan B: cuando ve que XI2 está disponible,
//! **descarta los eventos de ratón del núcleo**:
//!
//! ```text
//! // juce_XWindowSystem_linux.cpp (JUCE 9)
//! if (const auto xInputOpcode = XInputHelpers::setupXI2 (display))
//! {
//!     switch (event.xany.type)
//!     {
//!         case ButtonPress:
//!         case ButtonRelease:
//!         case MotionNotify:
//!             return;                       // <-- el ratón del núcleo se descarta
//!     }
//!     ...
//! }
//! ```
//!
//! O sea: el plugin recibe los eventos del núcleo (los tiene seleccionados
//! en su propia ventana) y **los tira a la basura**, esperando los de XI2 que
//! no van a llegar nunca. El síntoma es un editor que se dibuja perfecto y
//! no reacciona a ningún clic.
//!
//! # La solución: mentirle sobre la versión de XI2
//!
//! `setupXI2` de JUCE sólo se activa si la versión de XI2 es **2.2 o
//! mayor**:
//!
//! ```text
//! if (std::tuple (major, minor) < std::tuple (2, 2))
//!     return {};                            // <-- sin XI2: usa el ratón del núcleo
//! ```
//!
//! O sea: **si el toolkit cree que XI2 es viejo, usa los eventos del núcleo,
//! que bajo XWayland funcionan perfectamente.** Por eso el arreglo es
//! interceptar la llamada a `XIQueryVersion` y responder con una versión
//! anterior.
//!
//! El punto delicado es *cómo* se intercepta. JUCE no llama a `XIQueryVersion`
//! por su PLT: la resuelve con `dlsym` sobre el handle de `libXi`, así que
//! exportar un `XIQueryVersion` propio no alcanzaría. Lo que sí funciona es
//! interceptar **`dlsym`**: es la función que usa para resolver todo lo demás,
//! y su referencia en el plugin es versionada (`dlsym@GLIBC_2.34`), así que
//! el binario que lo aloja tiene que exportar un `dlsym` **con esa misma
//! versión**. De eso se encarga el script de versiones del crate que arma el
//! binario (ver [`HIKARU_DLSYM_VERSION_SCRIPT`]).
//!
//! El interposor **no hace nada** salvo que se detecte el entorno roto: en un
//! servidor X de verdad todo sigue su camino normal.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

#[cfg(target_os = "linux")]
use x11rb::protocol::xproto::ConnectionExt;

// ─────────────────────────── `XInitThreads` ───────────────────────────

/// `RTLD_GLOBAL` de glibc.
///
/// No está en el `libc` de Rust porque es una extensión de GNU, no de POSIX.
/// El valor es el mismo en glibc y en musl.
#[cfg(target_os = "linux")]
const RTLD_GLOBAL: libc::c_int = 0x00100;

/// Le dice a Xlib que este proceso es multihilo.
///
/// Es idempotente y segura de llamar desde cualquier hilo: la primera
/// llamada gana y las demás no hacen nada.
///
/// Devuelve `true` si se pudo llamar. `false` significa que no hay Xlib en
/// el sistema o que el símbolo no existe: no es un error, porque un plugin
/// puede no usar X11 en absoluto. En ese caso se avisa una sola vez.
#[cfg(target_os = "linux")]
pub fn ensure_xlib_threads() -> bool {
    static DONE: OnceLock<bool> = OnceLock::new();
    *DONE.get_or_init(call_x_init_threads)
}

/// Llamada real a `XInitThreads()`, una sola vez por proceso.
#[cfg(target_os = "linux")]
fn call_x_init_threads() -> bool {
    // `RTLD_LAZY`: no hace falta resolver todos los símbolos de Xlib para
    // llamar a `XInitThreads`. `RTLD_GLOBAL` para que el símbolo quede en el
    // ámbito global: si el plugin resuelve Xlib por `dlsym` sobre el
    // ámbito global, tiene que encontrar la misma librería.
    let lib = CString::new("libX11.so.6").expect("nombre de librería válido");
    // `unsafe`: `dlopen` con un nombre literal no puede fallar por memoria
    // inválida, y el handle devuelto se usa inmediatamente.
    let handle = unsafe { libc::dlopen(lib.as_ptr(), libc::RTLD_LAZY | RTLD_GLOBAL) };
    if handle.is_null() {
        eprintln!(
            "[hikaru-plugin-host] no se encontró libX11: los editores de plugin \
             que usen Xlib pueden colgarse al abrirse (falta XInitThreads)"
        );
        return false;
    }

    let sym = CString::new("XInitThreads").expect("nombre de símbolo válido");
    let raw = unsafe { libc::dlsym(handle, sym.as_ptr()) };
    if raw.is_null() {
        eprintln!(
            "[hikaru-plugin-host] libX11 no exporta XInitThreads: los editores \
             de plugin que usen Xlib pueden colgarse al abrirse"
        );
        return false;
    }

    // `XInitThreads` no tiene argumentos y devuelve `int` (0 = fallo).
    let func: unsafe extern "C" fn() -> libc::c_int = unsafe { std::mem::transmute(raw) };
    let ok = unsafe { func() } != 0;
    if !ok {
        eprintln!("[hikaru-plugin-host] XInitThreads() falló");
    }
    ok
}

/// En plataformas sin Xlib no hay nada que hacer.
#[cfg(not(target_os = "linux"))]
pub fn ensure_xlib_threads() -> bool {
    true
}

// ────────────────────── Detección del XInput2 inservible ──────────────────────

/// ¿El servidor X entrega eventos XInput2 a los clientes X11?
///
/// La respuesta se busca en el servidor: la extensión `XWAYLAND` está
/// presente justamente cuando el servidor es el de XWayland, que es el único
/// caso conocido en el que la extensión XInput2 está disponible pero sus
/// eventos no llegan a los clientes X11.
///
/// Devuelve `true` cuando se sospecha que XI2 no sirve (y hay que usar el
/// ratón del núcleo de X11).
#[cfg(target_os = "linux")]
fn xi2_events_reach_x_clients() -> bool {
    let Ok((conn, _)) = x11rb::rust_connection::RustConnection::connect(None) else {
        // Sin display no hay nada que mirar: se asume que XI2 funciona y no
        // se toca nada.
        return true;
    };
    let Ok(cookie) = conn.query_extension(b"XWAYLAND") else {
        return true;
    };
    let Ok(reply) = cookie.reply() else {
        return true;
    };
    // Con XWayland, XI2 no llega a los clientes X11.
    !reply.present
}

/// Deja el proceso listo para que un plugin abra su editor.
///
/// Hace las dos cosas que hacen falta antes de que un plugin toque Xlib:
///
/// 1. `XInitThreads()`, porque el toolkit del plugin levanta su propio hilo
///    de eventos X11 (ver el módulo).
/// 2. Decide si hay que **desactivar XInput2** en el proceso, porque en
///    XWayland los eventos XI2 no llegan a los clientes X11 y los toolkits
///    que dependen de ellos (JUCE 7 en adelante) se quedan sin ratón.
///
/// Es idempotente: la primera llamada resuelve y las demás no hacen nada.
#[cfg(target_os = "linux")]
pub fn prepare_for_plugin_gui() {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        ensure_xlib_threads();

        if !xi2_events_reach_x_clients() {
            set_xi2_unusable(true);
            eprintln!(
                "[hikaru-plugin-host] el servidor X (XWayland) no entrega eventos \
                 XInput2 a los clientes X11: los editores de plugin usarán los \
                 eventos de ratón del núcleo de X11"
            );
        }
    });
}

/// En plataformas sin Xlib no hay nada que hacer.
#[cfg(not(target_os = "linux"))]
pub fn prepare_for_plugin_gui() {}


// ─────────────────────── Intercepción de XInput2 ───────────────────────

/// ¿El servidor X no entrega eventos XInput2 a los clientes X11?
///
/// Es el caso de **XWayland**: la extensión está y las llamadas se aceptan,
/// pero los eventos nunca llegan. Los toolkits que dependan de XI2 para el
/// ratón (JUCE 7 en adelante) se quedan entonces sin ratón ninguno.
///
/// Se activa **sólo** en ese entorno: en un servidor X de verdad, XI2
/// funciona y hay que dejarlo en paz.
#[cfg(target_os = "linux")]
static XI2_INUTIL: AtomicBool = AtomicBool::new(false);

/// Activa o desactiva el rodeo de XInput2.
///
/// `true` hace que el interposor de `dlsym` responda con una versión de XI2
/// anterior a la 2.2, que es lo que hace que los toolkits vuelvan a los
/// eventos del núcleo de X11.
#[cfg(target_os = "linux")]
pub fn set_xi2_unusable(unusable: bool) {
    XI2_INUTIL.store(unusable, Ordering::Relaxed);
}

/// Si el rodeo de XInput2 está activo.
#[cfg(target_os = "linux")]
pub fn xi2_unusable() -> bool {
    XI2_INUTIL.load(Ordering::Relaxed)
}

/// El `dlsym` real, para no meterse en recursión.
///
/// `dlvsym(RTLD_NEXT, ...)` busca el siguiente `dlsym` en el orden de
/// búsqueda de la biblioteca: el de libc. Hace falta la versión porque
/// `dlsym` está versionado en glibc y el `RTLD_NEXT` pelado no encuentra
/// el símbolo.
#[cfg(target_os = "linux")]
fn real_dlsym() -> Option<unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void> {
    static REAL: OnceLock<Option<unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void>> =
        OnceLock::new();
    *REAL.get_or_init(|| unsafe {
        extern "C" {
            fn dlvsym(handle: *mut c_void, symbol: *const c_char, version: *const c_char)
                -> *mut c_void;
        }
        const RTLD_NEXT: *mut c_void = -1isize as *mut c_void;
        // `dlsym` cambió de versión en glibc 2.34; se prueban las dos
        // conocidas y se usa la primera que exista.
        for version in ["GLIBC_2.34\0", "GLIBC_2.2.5\0"] {
            let name = b"dlsym\0";
            let p = dlvsym(
                RTLD_NEXT,
                name.as_ptr() as *const c_char,
                version.as_ptr() as *const c_char,
            );
            if !p.is_null() {
                return Some(std::mem::transmute(p));
            }
        }
        None
    })
}

/// Versión de XI2 que se le informa al toolkit.
///
/// 1.0 es anterior a la 2.2 que JUCE exige, así que `setupXI2` devuelve
/// "no hay XI2" y el toolkit vuelve a los eventos del núcleo de X11.
#[cfg(target_os = "linux")]
unsafe extern "C" fn xi_query_version_stub(
    _display: *mut c_void,
    major: *mut c_int,
    minor: *mut c_int,
) -> c_int {
    if !major.is_null() {
        *major = 1;
    }
    if !minor.is_null() {
        *minor = 0;
    }
    0 // Success
}

/// El `dlsym` real, envuelto para poder interceptar una llamada.
#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dlsym_impl(
    handle: *mut c_void,
    symbol: *const c_char,
) -> *mut c_void {
    if XI2_INUTIL.load(Ordering::Relaxed) && !symbol.is_null() {
        // Éste es el nombre exacto con el que JUCE pide la función.
        if CStr::from_ptr(symbol).to_bytes() == b"XIQueryVersion" {
            eprintln!("[xlib] interceptado XIQueryVersion");
            return xi_query_version_stub as *mut c_void;
        }
    }

    match real_dlsym() {
        Some(f) => f(handle, symbol),
        // Sin `dlsym` real no hay nada que hacer: devolver `null` es lo que
        // haría Xlib cuando no encuentra el símbolo.
        None => {
            eprintln!("[xlib] no se encontró el dlsym real");
            std::ptr::null_mut()
        }
    }
}

// Exporta el interposor con los dos nombres de versión que usa glibc.
//
// Sin esto el símbolo no llega a la tabla dinámica del binario y los
// plugins siguen usando el `dlsym` de libc.
#[cfg(target_os = "linux")]
core::arch::global_asm!(
    ".symver dlsym_impl, dlsym@@GLIBC_2.34",
    ".symver dlsym_impl, dlsym@GLIBC_2.2.5",
);

/// El script de versiones que hay que pasarle al enlazador.
///
/// Exporta `dlsym` con las dos versiones de glibc, para que el interposor
/// sea el que usen los plugins sin importar con qué glibc se compilaran.
///
/// El crate que arma el binario final tiene que aplicarlo:
///
/// ```toml
/// # build.rs del crate del binario
/// println!("cargo:rustc-link-arg=-Wl,--version-script=ruta/al/hikaru-dlsym.map");
/// ```
#[cfg(target_os = "linux")]
pub const HIKARU_DLSYM_VERSION_SCRIPT: &str = include_str!("../hikaru-dlsym.map");
