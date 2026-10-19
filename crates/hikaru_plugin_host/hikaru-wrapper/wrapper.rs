// Copyright (C) Hikaru Corporation - 2026
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/hikaru-wrapper/wrapper.rs

//! El envoltorio completo: ventana + plugin + bucle, en un solo objeto.
//!
//! # Por qué un tipo y no tres
//!
//! Abrir el editor de un plugin son cinco pasos que **no** se pueden separar:
//!
//! 1. Crear la ventana del envoltorio (sin mapear).
//! 2. Cargar el plugin (el `dlopen`, que es lo que más falla).
//! 3. Embeberlo y ajustar la ventana a su tamaño preferido.
//! 4. Mapearla, ya con el primer frame del plugin detrás.
//! 5. Correr el bucle de 60 Hz hasta que alguien la cierre.
//!
//! Cada uno tiene su camino de error y una regla de limpieza. Antes de este
//! tipo el orden vivía pegado en el hilo de la ventana de `hikaru_gui`, así
//! que un `?` en el paso equivocado dejaba una ventana invisible colgada, un
//! `dlopen` a medias o un consumidor de la cinta de audio conectado a una
//! ventana que ya no existía.
//!
//! Con [`HikaruWrapper::open`] el orden es una sola función con un solo
//! `Result`, y los tipos que se necesitan para llegar hasta el paso 4 —la
//! ventana y el handle nativo— se manejan adentro. El llamador nunca ve un
//! `RawWindowHandle` ni un XID.

use std::path::Path;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use crate::tap::{PluginTap, TapKey};
use crate::{ClapInstance, PluginFormat, PluginInstance, Vst3Instance};

use super::config::{HostFrame, WrapperConfig};
use super::event_loop::{LoopExit, ParamFeedback, TickBudget, WrapperCommand, WrapperEventLoop};
use super::input::InputPolicy;
use super::window::{CreateError, HikaruWrapperWindow};

/// Error al abrir el editor de un plugin.
///
/// Es un enum y no un `String` porque hay tres fallas que el llamador tiene que
/// tratar distinto: la ausencia de servidor de display (el DAW se puede
/// abrir sin X11 para tocar la library sin abrir nunca un editor), el fallo al
/// cargar el binario (que en la UI es "ese plugin no sirve en esta máquina") y
/// el rechazo del propio plugin a ser embebido (que es "este plugin no se
/// puede mostrar acá", un problema del plugin y no del host).
#[derive(Debug)]
pub enum WrapperError {
    /// No hay servidor X11/XWayland disponible.
    ///
    /// En una sesión Wayland sin XWayland no hay forma de mostrar un editor
    /// VST3/CLAP: el editor es una ventana X11 y XWayland es lo único que la
    /// puede mostrar en pantalla.
    NoDisplay(String),
    /// El plugin no se pudo cargar.
    Load(String),
    /// El plugin se cargó pero no aceptó el embebido.
    Embed(String),
    /// La ventana del envoltorio no se pudo crear.
    Window(String),
}

impl std::fmt::Display for WrapperError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDisplay(m) => write!(f, "{m}"),
            Self::Load(m) => write!(f, "No se pudo cargar el plugin: {m}"),
            Self::Embed(m) => write!(f, "El plugin no abrió su editor: {m}"),
            Self::Window(m) => write!(f, "No se pudo crear la ventana del editor: {m}"),
        }
    }
}

/// Qué va a hacer el bucle con cada tick.
///
/// Se separa del [`WrapperConfig`] porque una cosa es cómo se ve la ventana y
/// otra es qué bombchea a 60 Hz. Cambiar el ritmo no debería obligar a
/// redefinir hints, ni al revés.
#[derive(Debug, Clone, Copy)]
pub struct WrapperRuntime {
    /// Ritmo del bucle.
    pub budget: TickBudget,
    /// Política de entrada.
    pub input: InputPolicy,
    /// Periodo con el que se manda el feedback de parámetros a la UI.
    ///
    /// `None` = todos los ticks. La UI prefiere un valor alto porque cada
    /// evento pasa por el hilo principal y reconstruye la ventana del rack.
    pub feedback_every: Option<Duration>,
}

impl Default for WrapperRuntime {
    fn default() -> Self {
        Self {
            budget: TickBudget::default(),
            input: InputPolicy::default(),
            feedback_every: None,
        }
    }
}

/// Un editor de plugin embebido en una ventana de Hikaru.
///
/// Se abre con [`Self::open`] y se deja correr con [`Self::run`], que no
/// vuelve hasta que la ventana se cierra. No es `Send`: vive y muere en el
/// hilo que la creó.
pub struct HikaruWrapper {
    loop_: WrapperEventLoop,
    instance: Box<dyn PluginInstance>,
    feed: Option<Arc<PluginTap>>,
    /// Cuadro actual pedido por el DAW.
    host_frame: Option<HostFrame>,
}

/// Cierra el editor **antes** de que se destruya la ventana.
///
/// # Por qué hace falta un `Drop` explícito
///
/// Rust dropea los campos de un struct **en orden de declaración**. Con
/// `loop_` declarado antes que `instance`, sin este `Drop` la ventana se
/// destruye primero y el plugin se desengancha después, contra un XID que ya
/// no existe.
///
/// El síntoma no es un conflicto de memoria sino un **aborto del proceso**:
/// al cerrar el editor, JUCE (y cualquier toolkit que use Xlib) consulta
/// propiedades de la ventana padre, el servidor responde `BadWindow`, y el
/// manejador de errores por defecto de Xlib —que aborta— lo mata todo:
///
/// ```text
/// X Error of failed request:  BadWindow (invalid Window parameter)
///   Major opcode of failed request:  20 (X_GetProperty)
/// ```
///
/// Un `Drop` propio se ejecuta **antes** que el de los campos, así que
/// invierte el orden sin tener que depender de cómo se declaren.
impl Drop for HikaruWrapper {
    fn drop(&mut self) {
        // Idempotente: `hide_gui` lo es, y `run` ya lo llamó en el camino
        // normal. Esto cubre los caminos de error, donde el plugin quedó
        // embebido y la `HikaruWrapper` se suelta sin llegar a `run`.
        self.instance.hide_gui();
    }
}

impl HikaruWrapper {
    /// Abre el editor de un plugin en una ventana de Hikaru.
    ///
    /// `path` es el `.vst3` o `.clap`; `format` dice cuál de los dos es.
    /// `commands` es el canal por el que el DAW manda [`WrapperCommand`]s y
    /// `feedback` el por el que vuelve el estado de los parámetros.
    ///
    /// `tap_key` es la cinta de audio del motor (ver [`crate::tap`]). Si el
    /// slot no tiene plugin cargado, pasale [`crate::tap::NO_SLOT`]: el editor abre igual,
    /// los parámetros se mueven, pero no hay display reactivo al audio.
    ///
    /// `config.host_frame` decide **dónde** vive el editor dentro de la
    /// ventana del DAW. Sin cuadro, se centra en la pantalla y se comporta
    /// como una ventana normal del escritorio (modo de depuración).
    pub fn open(
        path: &Path,
        format: PluginFormat,
        title: &str,
        config: WrapperConfig,
        runtime: WrapperRuntime,
        commands: Receiver<WrapperCommand>,
        feedback: Sender<ParamFeedback>,
        tap_key: TapKey,
    ) -> Result<Self, WrapperError> {
        // 1. Ventana, sin mapear. Nada visible todavía: si el `dlopen` falla,
        //    el usuario no llega a ver una ventana vacía.
        let mut window = HikaruWrapperWindow::create(title, &config, runtime.input)
            .map_err(wrapper_error_from_create)?;

        // 2. Plugin. Es lo que más falla: arquitectura, símbolos, dependencias.
        let mut instance: Box<dyn PluginInstance> = match format {
            PluginFormat::VST3 => Vst3Instance::load(path)
                .map(|i| Box::new(i) as Box<dyn PluginInstance>)
                .map_err(WrapperError::Load)?,
            PluginFormat::CLAP => ClapInstance::load(path)
                .map(|i| Box::new(i) as Box<dyn PluginInstance>)
                .map_err(WrapperError::Load)?,
        };

        // 3. Tinta de audio. La tasa sale del tap y no de una constante: VST3
        //    la sella en `setupProcessing`, así que si esta instancia
        //    arrancara a 44100 mientras el motor corre a 48000 el display se
        //    dibujaría con la escala mal.
        let feed = crate::tap::lookup(tap_key);
        if let Some(tap) = &feed {
            instance.set_sample_rate(tap.sample_rate());
        }

        // 4. Tamaño preferido antes de embebir. VST3 lo sabe siempre; CLAP lo
        //    confirma recién después de `gui.create`, que es lo que devuelve
        //    `notify_gui_embedded` más abajo.
        let preferred = instance.get_gui_size().filter(|(w, h)| *w > 0 && *h > 0);

        // 5. Embeber. `prepare` crea la ventana de contenido en el hueco del
        //    marco y devuelve su handle —nunca el del contenedor, que es sólo
        //    cromo— para que el plugin no pueda pintar encima del título.
        let handle = window.prepare(preferred).map_err(WrapperError::Window)?;
        instance
            .show_gui_embedded(handle)
            .map_err(WrapperError::Embed)?;

        if !instance.is_gui_open() {
            instance.hide_gui();
            return Err(WrapperError::Embed(
                "lo aceptó pero no confirma que el editor esté abierto".into(),
            ));
        }

        // 6. Algunos plugins sólo saben su tamaño real una vez que la vista
        //    está adjunta a una ventana.
        let preferred = instance
            .notify_gui_embedded()
            .filter(|(w, h)| *w > 0 && *h > 0)
            .or(preferred);

        // 7. Buscar la vista hija del plugin, para poder redirigirle los
        //    eventos de puntero que caigan en el contenedor.
        window.refresh_view();

        // 8. Recién ahora se muestra: el primer frame ya tiene contenido del
        //    plugin detrás, no una ventana vacía.
        //
        //    `show` vuelve a pasar `preferred` porque CLAP sólo declara su
        //    tamaño después de `gui.create`: el marco tiene que ajustarse otra
        //    vez y con él la ventana de contenido.
        if let Err(e) = window.show(preferred) {
            instance.hide_gui();
            return Err(WrapperError::Window(e));
        }

        Ok(Self {
            loop_: WrapperEventLoop::new(window, commands, feedback, feed.clone(), runtime.budget),
            instance,
            feed,
            host_frame: config.host_frame,
        })
    }

    /// Corre el bucle hasta que la ventana se cierra.
    ///
    /// No vuelve antes. El hilo que la llama es el que se ocupa del apagado:
    /// soltar la `HikaruWrapper` y soltar el `JoinHandle` del hilo.
    pub fn run(mut self) -> LoopExit {
        let exit = self.loop_.run(self.instance.as_mut());

        // La cinta se desconecta adentro de `run`, pero un `open` que terminó
        // en `Err` puede haber dejado un consumidor conectado. `run` no llega
        // a ese caso, así que el `detach` defensivo es barato y evita que el
        // motor copie bloques a una ventana que ya no existe.
        if let Some(tap) = &self.feed {
            tap.detach_consumer();
        }

        exit
    }

    /// Mueve y redimensiona el editor dentro de la ventana del DAW.
    ///
    /// Ruta corta para el DAW: aplica el cuadro ahora en vez de encolarlo y
    /// esperar el próximo tick. Se llama desde el hilo de la GUI, que es el
    /// que sabe dónde quedó el panel.
    pub fn set_host_frame(&mut self, frame: HostFrame) {
        self.host_frame = Some(frame);
        self.loop_.set_host_frame(frame);
    }

    /// El cuadro actual del editor dentro de la ventana del DAW.
    pub fn host_frame(&self) -> Option<HostFrame> {
        self.host_frame
    }

    /// Si la ventana se creó sin decoraciones del gestor de ventanas.
    ///
    /// Lo consulta la UI para mostrar "Editor (incrustado)" vs "Editor
    /// (flotante)" en el rack: es la diferencia entre una ventana del DAW y
    /// una del escritorio.
    pub fn is_borderless(&self) -> bool {
        self.loop_.is_borderless()
    }

    /// Las medidas del marco del editor.
    ///
    /// Con [`super::FrameMetrics::frameless`] el plugin queda sin titlebar ni
    /// bordes, que es lo que corresponde a los editores que ya traen los
    /// suyos: envolverlos en otro marco se ve doble.
    pub fn set_frame_metrics(&mut self, metrics: super::frame::FrameMetrics) {
        self.loop_.set_frame_metrics(metrics);
    }

    /// Si la ventana quedó realmente como hija del DAW en el árbol de X11.
    ///
    /// Casi siempre es `false` en una sesión Wayland, y esa es la razón por la
    /// que el editor no puede empotrarse de verdad: no hay XID del DAW al que
    /// colgarse.
    pub fn is_child_of_daw(&self) -> bool {
        self.loop_.is_child_of_daw()
    }

    /// Si la ventana se está mostrando ahora mismo.
    pub fn is_visible(&self) -> bool {
        self.loop_.is_visible()
    }

    /// El nombre del plugin, para los mensajes de error y el log.
    pub fn plugin_name(&self) -> &str {
        self.instance.get_name()
    }
}

/// Traduce el error de creación de la ventana al error del envoltorio.
fn wrapper_error_from_create(err: CreateError) -> WrapperError {
    match err {
        CreateError::Connect(m) => WrapperError::NoDisplay(format!(
            "Sin conexión X11/XWayland, no se puede abrir un editor. {m}. \
             Si estás en Wayland puro, revisá que XWayland esté activo."
        )),
        CreateError::Create(m) => WrapperError::Window(m),
        CreateError::Hints(m) => WrapperError::Window(format!("falló la envoltura: {m}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_tres_fallos_dicen_cosas_distintas() {
        // La UI los muestra en lugares distintos: `NoDisplay` es un problema
        // del entorno, `Load` del plugin en esta máquina, y `Embed` del
        // plugin mismo. Confundirlos hace que un plugin roto se reporte como
        // "no hay display".
        let messages = [
            WrapperError::NoDisplay("sin X11".into()).to_string(),
            WrapperError::Load("arquitectura incorrecta".into()).to_string(),
            WrapperError::Embed("se niega a embeberse".into()).to_string(),
            WrapperError::Window("no se pudo crear".into()).to_string(),
        ];
        for (i, a) in messages.iter().enumerate() {
            for (j, b) in messages.iter().enumerate() {
                assert_eq!(i == j, a == b, "dos errores con el mismo texto: {a} / {b}");
            }
        }
    }

    #[test]
    fn sin_display_explica_que_xwayland_es_el_camino() {
        // El mensaje es lo único que el usuario ve cuando abre un editor en
        // una sesión sin XWayland, así que tiene que decir qué arreglar. Y
        // tiene que salir por la traducción real, no por el `Display` pelado.
        let err = wrapper_error_from_create(CreateError::Connect("connection refused".into()));
        let msg = err.to_string();
        assert!(matches!(err, WrapperError::NoDisplay(_)));
        assert!(msg.contains("XWayland"), "no dice cómo resolverlo: {msg}");
    }

    #[test]
    fn el_runtime_por_defecto_bombea_a_60hz_y_con_entrada() {
        let runtime = WrapperRuntime::default();
        assert_eq!(runtime.budget.fps(), 60);
        assert!(runtime.input.mask.selects_pointer());
        // El feedback por defecto es a 60 Hz: la UI lo filtra y el rack
        // descarta lo que llega mientras el usuario está arrastrando un knob.
        assert!(runtime.feedback_every.is_none());
    }

    #[test]
    fn la_ventana_se_abre_sin_decoraciones_por_defecto() {
        // Es la diferencia entre "editor del DAW" y "ventana suelta": si esto
        // cambia, la UI tiene que poder avisar al usuario de que hay una
        // ventana del escritorio abierta.
        let config = WrapperConfig::default();
        assert!(config.override_redirect());
    }
}