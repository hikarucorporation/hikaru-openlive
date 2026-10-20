// Copyright (C) Hikaru Corporation - 2026
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/src/hikaru_wrapper/window.rs

//! La ventana nativa del envoltorio: su conexión, su XID y su hints.
//!
//! [`HikaruWrapperWindow`] es dueña de la conexión X11 y del recurso gráfico.
//! While exists, la ventana existe: el `Drop` suelta el vínculo padre-hijo y
//! destruye la ventana en ese orden, que es el que no deja handles colgados
//! en XWayland.

use std::time::Instant;

use raw_window_handle::{RawWindowHandle, XlibWindowHandle};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::x11_utils::Serialize;

use super::config::HostFrame;
use super::frame::{CursorHint, FrameMetrics, FrameRegion, ResizeDir};
use super::input::{InputActivation, InputMask, InputPolicy};
use super::keep_alive::WrapperKeepAlive;
use super::x11;
use super::{WrapperConfig, WrapperHintPlan};

/// Qué hizo el marco con un evento de puntero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameAction {
    /// El evento era del editor: va para el plugin.
    None,
    /// El marco se lo comió: el plugin no tiene que enterarse.
    Handled,
    /// Cerrar la ventana.
    Close,
}

/// Qué está haciendo el usuario con el marco ahora mismo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameDrag {
    /// Arrastrando la ventana desde el título.
    Move {
        /// Puntero en coordenadas de raíz al empezar.
        origin: (i16, i16),
        /// Posición de la ventana en raíz al empezar.
        window_origin: (i16, i16),
    },
    /// Redimensionando desde un borde o una esquina.
    Resize {
        dir: ResizeDir,
        /// Tamaño al empezar.
        start_size: (u16, u16),
        /// Puntero en coordenadas de raíz al empezar.
        origin: (i16, i16),
    },
}

/// Si el plugin ya avisó el tamaño que quiere, es la vista la que manda.
const MIN_FRAME_WIDTH: u16 = 240;
const MIN_FRAME_HEIGHT: u16 = 180;

/// Lo que pasó al drenar los eventos de la ventana en un tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowEvent {
    /// Nada relevante.
    Idle,
    /// El WM pidió cerrar la ventana (botón de cerrar del titlebar).
    CloseRequested,
    /// Alguien —el WM o el plugin— destruyó la ventana.
    Destroyed,
    /// El tamaño de la ventana cambió: hay que redimensionar el editor.
    Resized { width: u32, height: u32 },
    /// La conexión con el servidor se perdió. La ventana ya no existe.
    ConnectionLost(String),
}

/// Mensaje de fallo durante la creación de la ventana.
///
/// Se distingue del `String` de `Err` para que el llamador pueda desarmar la
/// ventana ya creada sin tener que adivinar si llegó a crearse.
#[derive(Debug)]
pub enum CreateError {
    /// No se pudo ni conectar al servidor.
    Connect(String),
    /// No se pudo reservar el XID o crear la ventana.
    Create(String),
    /// La ventana se creó pero un hint duro (`WM_TRANSIENT_FOR`) falló.
    Hints(String),
}

/// Tamaño por defecto de la ventana del editor, en píxeles.
pub const DEFAULT_WIDTH: u16 = 1024;
pub const DEFAULT_HEIGHT: u16 = 700;

/// Ventana nativa del envoltorio tipo Fruity Wrapper.
///
/// # Ciclo de vida
///
/// 1. [`Self::create`] conecta a X11 y crea la ventana **sin mapear**,
///    aplicando los hints. Sin `RESIZE_REDIRECT` (esa máscara pide al
///    servidor redirigir los resizes y bajo algunos WMs dejaba la ventana
///    sin mapear para siempre).
/// 2. El plugin se embebe en esa ventana.
/// 3. [`Self::show`] la mapea y pide la activación.
/// 4. [`Self::poll_events`] la sondea cada tick.
/// 5. El `Drop` la destruye.
///
/// Nada visible aparece hasta el paso 3: si la carga del plugin falla, el
/// usuario no ve nunca una ventana vacía.
pub struct HikaruWrapperWindow {
    /// La conexión. Vive hasta el final: es lo que mantiene vivos el
    /// contexto gráfico y la ventana.
    conn: x11rb::rust_connection::RustConnection,
    screen_num: usize,
    root: Window,
    window: Window,
    /// Profundidad del root: el contenido tiene que usar el mismo visual.
    parent_depth: u8,
    /// `WM_DELETE_WINDOW`, si el WM lo{theorem}ó.
    wm_delete: Option<u32>,
    plan: WrapperHintPlan,
    keep_alive: WrapperKeepAlive,
    policy: InputPolicy,
    /// Ventana de contenido: la que se le da al plugin.
    ///
    /// Es la pieza que hace que el marco no se pueda tapar. El contenedor es
    /// sólo el marco; el plugin dibuja en su propia ventana hija, colocada en
    /// el hueco interior. Hay dos estilos de plugin y con esto funcionan los
    /// dos:
    ///
    /// - Los que crean una ventana hija propia (JUCE, Qt): esa ventana se
    ///   anida en el contenido y se ajusta sola.
    /// - Los que dibujan directamente sobre lo que les pasaron: dibujan en el
    ///   contenido, que ya está en el hueco. **Antes esto pintaba encima del
    ///   marco** y el editor aparecía sin título ni bordes.
    content: Option<Window>,
    /// Dónde está la ventana de contenido dentro del contenedor.
    ///
    /// La vista del plugin es **hija del contenido**, así que sus
    /// coordenadas son relativas al contenido. Para traducir un evento que
    /// llegó al contenedor hay que sumar este offset: sin él, un click
    /// caería desplazado por el alto del titlebar.
    content_origin: (i16, i16),
    /// Vista propia del plugin dentro del contenido. Sólo hace falta para
    /// redirigir entrada: cacheada porque `QueryTree` es un viaje al servidor.
    view: Option<x11::ViewGeometry>,
    /// Si se le reenvió una presión a la vista del plugin.
    ///
    /// Es lo que obliga a reenviarle también la liberación: un editor que se
    /// come la presión y no la liberación deja el control **pegado** (el knob
    /// sigue al puntero y el clic siguiente se cuenta dos veces).
    pressed_in_view: bool,
    /// Cuántas veces se buscó la vista sin éxito, para avisar una sola vez.
    view_scan_failures: u32,
    /// Si el puntero está sobre la ventana.
    ///
    /// Se mantiene con los `EnterNotify`/`LeaveNotify` que ya pedía la
    /// máscara, así que no cuesta ningún viaje al servidor. Es lo que permite
    /// reparar el foco sólo cuando el usuario está mirando la ventana:
    /// repararlo sin ese dato le robaría el teclado al DAW mientras trabaja.
    pointer_inside: bool,
    /// Cuadro pedido por el DAW, en coordenadas de cliente del DAW.
    ///
    /// `None` mientras el rack no diga dónde va el editor: hasta entonces la
    /// ventana se centra en la pantalla.
    host_frame: Option<HostFrame>,
    /// `true` si la ventana se creó con `override_redirect`.
    override_redirect: bool,
    /// Si esta ventana sigue atada a la del DAW.
    strict_child: bool,
    /// Último cuadro aplicado en raíz. Evita mandar un `ConfigureWindow` por
    /// tick cuando nada se movió, que es el caso normal.
    applied_root_frame: Option<(i16, i16, u16, u16)>,
    /// Si la ventana del DAW estaba visible en el último chequeo.
    host_visible: bool,
    /// XID de la ventana del DAW, si la sesión es X11.
    ///
    /// `None` en Wayland: ahí el DAW es una superficie Wayland y no hay XID
    /// que seguir. Es el motivo por el que el editor no puede ser un hijo
    /// real de la ventana del DAW bajo Wayland.
    daw_xid: Option<Window>,
    /// Si la ventana nació como hija de la del DAW en el árbol de X11.
    child_of_daw: bool,
    /// Medidas del marco.
    frame: FrameMetrics,
    /// Contexto de dibujo del marco.
    paint: Option<x11::Paint>,
    /// Texto del título.
    title: String,
    /// Arrastre/redimensionado en curso.
    drag: Option<FrameDrag>,
    /// Cursor que hay que mostrar, pendiente de aplicar.
    cursor: Option<CursorHint>,
    /// Si la ventana tiene el foco del teclado (para pintar el título).
    focused: bool,
    /// Si el botón de cerrar está bajo el puntero.
    close_hot: bool,
    /// Tamaño exterior de la ventana, en píxeles.
    ///
    /// Cacheado porque el marco lo necesita en cada repintado y en cada
    /// hit-test, y `GetGeometry` es un viaje al servidor por consulta.
    size: (u16, u16),
    /// Posición de la ventana en coordenadas de raíz.
    root_pos: (i16, i16),
    /// Último puntero conocido, en coordenadas de la ventana.
    last_pointer: (i16, i16),
    /// Si esta ventana está mapeada ahora mismo.
    ///
    /// Va por debajo de "visible" a propósito: el editor se puede dejar
    /// destapado mientras el DAW está minimizado, y hay que poder
    /// re-mapearlo sin volver a crearlo.
    mapped: bool,
    /// Reloj monotónico del hilo. Base del keep-alive.
    started: Instant,
    /// `false` una vez que alguien (el WM o nosotros) destruyó la ventana, para
    /// no destruir dos veces lo que el servidor ya no tiene.
    alive: bool,
}

impl HikaruWrapperWindow {
    /// Conecta a X11 y crea la ventana del envoltorio, sin mapear.
    ///
    /// `title` es el texto del titlebar. `config` decide los hints y
    /// `policy`, el comportamiento de entrada.
    pub fn create(
        title: &str,
        config: &WrapperConfig,
        policy: InputPolicy,
    ) -> Result<Self, CreateError> {
        let (conn, screen_num) = x11rb::rust_connection::RustConnection::connect(None)
            .map_err(|e| CreateError::Connect(format!("{e:?}")))?;

        let screen = &conn.setup().roots[screen_num];
        let root = screen.root;

        // Padre del DAW cuando la sesión es X11: la ventana del plugin nace
        // como hija suya en el árbol de X11 (una ventana X11 puede tener por
        // padre la de otro cliente de la misma DISPLAY). Se usa su geometría
        // para centrar y su depth para no pedir un visual incompatible.
        //
        // Ojo: esto es el *árbol* de X11, que es distinto del vínculo con el
        // WM. El árbol da la geometría y el nivel de profundidad; el apilado,
        // la pertenencia al grupo y el "mantenerse arriba" los decide la
        // envoltura, vía `WM_TRANSIENT_FOR` y `_NET_WM_STATE_ABOVE`.
        let daw_parent: Option<u32> = config.parent.x11_id();
        let (parent_geom, parent_depth): (Option<(i16, i16)>, u8) = match daw_parent {
            Some(xid) => match conn.get_geometry(xid) {
                Ok(cookie) => match cookie.reply() {
                    Ok(geom) => (Some((geom.width as i16, geom.height as i16)), geom.depth),
                    Err(_) => (None, screen.root_depth),
                },
                Err(_) => (None, screen.root_depth),
            },
            None => (None, screen.root_depth),
        };

        let (width, height) = (DEFAULT_WIDTH, DEFAULT_HEIGHT);

        // Dónde nace la ventana. Hay tres casos y no son intercambiables:
        //
        // 1. Cuadro pedido por el DAW + X11: la ventana es hija del DAW en el
        //    árbol de X11 y sus coordenadas son de cliente. Es el embedding de
        //    verdad, el que no existe bajo Wayland.
        // 2. Cuadro pedido por el DAW + Wayland: no hay XID del DAW, así que la
        //    ventana nace top-level en la raíz y el bucle va a recalcularlo cada tick
        //    calculando su posición de raíz a partir de la del DAW.
        // 3. Sin cuadro: centrada en la pantalla. Es el modo de depuración.
        let child_of_daw = config.strict_child() && parent_geom.is_some();
        let want_child_of_daw = child_of_daw;

        let (pos_x, pos_y) = match (want_child_of_daw, parent_geom) {
            (true, Some(_)) => {
                let frame = config
                    .host_frame
                    .filter(|f| f.is_usable())
                    .unwrap_or_else(|| HostFrame::new(0, 0, width, height));
                (frame.x, frame.y)
            }
            _ => (
                ((screen.width_in_pixels as i16 - width as i16) / 2).max(0),
                ((screen.height_in_pixels as i16 - height as i16) / 2).max(0),
            ),
        };
        let window_parent = match (want_child_of_daw, daw_parent) {
            (true, Some(xid)) => xid,
            _ => root,
        };

        let window = conn
            .generate_id()
            .map_err(|e| CreateError::Create(format!("no se pudo reservar el XID: {e}")))?;

        // `override_redirect` es lo que le saca las decoraciones: una ventana
        // override-redirect no la gestiona el WM, así que el WM no le pone
        // titlebar, ni bordes, ni botón de cerrar, ni entrada en el taskbar.
        // El usuario no la puede mover ni redimensionar; eso es del DAW.
        let window_aux = CreateWindowAux::new()
            .event_mask(event_mask(policy.mask))
            .override_redirect(Some(u32::from(config.override_redirect())));

        conn.create_window(
            parent_depth,
            window,
            window_parent,
            pos_x,
            pos_y,
            width,
            height,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &window_aux,
        )
        .map_err(|e| CreateError::Create(format!("no se pudo crear la ventana: {e}")))?;

        let mut me = Self {
            conn,
            screen_num,
            root,
            window,
            parent_depth,
            wm_delete: None,
            plan: super::plan_hints(config),
            keep_alive: WrapperKeepAlive::disabled(),
            policy,
            content: None,
            content_origin: (0, 0),
            view: None,
            pressed_in_view: false,
            view_scan_failures: 0,
            pointer_inside: false,
            host_frame: config.host_frame,
            override_redirect: config.override_redirect(),
            strict_child: config.strict_child(),
            daw_xid: config.parent.x11_id(),
            child_of_daw,
            applied_root_frame: None,
            host_visible: true,
            mapped: false,
            frame: FrameMetrics::default(),
            paint: None,
            title: title.to_string(),
            drag: None,
            cursor: None,
            focused: false,
            close_hot: false,
            size: (width, height),
            root_pos: (pos_x, pos_y),
            last_pointer: (0, 0),
            started: Instant::now(),
            alive: true,
        };

        // Un fallo a partir de acá tiene que dejar la ventana destruida: si
        // se sale con `?`, el `Drop` la limpia igual, pero el mensaje se
        // pierde. Por eso se acumula el error y se limpia a mano.
        if let Err(e) = me.init_metadata(title) {
            let _ = me.destroy_window();
            return Err(CreateError::Create(e));
        }

        match x11::apply_wrapper_hints(&me.conn, me.window, config) {
            Ok(plan) => me.plan = plan,
            Err(e) => {
                // Los hints de estado y de tipo son best-effort y
                // `apply_wrapper_hints` ya los suprimió: si llegó un `Err`
                // es el `WM_TRANSIENT_FOR`, que es el vínculo duro con el
                // padre. Sin él la ventana abriría top-level, que es
                // justamente el comportamiento roto que este módulo arregla,
                // así que se aborta destruyendo la ventana invisible.
                let _ = me.destroy_window();
                return Err(CreateError::Hints(e));
            }
        }

        me.keep_alive = WrapperKeepAlive::for_plan(elapsed_ms(me.started), &me.plan);
        Ok(me)
    }

    /// `_NET_WM_NAME` y `WM_CLASS`. Best-effort: si fallan, la ventana sigue
    /// válida, sólo sale sin nombre en la barra de tareas.
    fn init_metadata(&mut self, title: &str) -> Result<(), String> {
        if let Ok(cookie) = self.conn.intern_atom(false, b"_NET_WM_NAME") {
            if let Ok(reply) = cookie.reply() {
                let _ = self.conn.change_property(
                    PropMode::REPLACE,
                    self.window,
                    reply.atom,
                    u32::from(AtomEnum::STRING),
                    8,
                    title.len() as u32,
                    title.as_bytes(),
                );
            }
        }
        if let Ok(cookie) = self.conn.intern_atom(false, b"WM_CLASS") {
            if let Ok(reply) = cookie.reply() {
                let class = b"hikaru-plugin\0Hikaru\0";
                let _ = self.conn.change_property(
                    PropMode::REPLACE,
                    self.window,
                    reply.atom,
                    u32::from(AtomEnum::STRING),
                    8,
                    class.len() as u32,
                    &class[..],
                );
            }
        }

        // Protocolo de cierre: se guarda el átomo para el bucle. Si no se puede
        // negociar, la ventana se cierra igual por DestroyNotify.
        self.wm_delete = self
            .conn
            .intern_atom(false, b"WM_DELETE_WINDOW")
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map(|reply| {
                let atom = reply.atom;
                if let Ok(proto_cookie) = self.conn.intern_atom(false, b"WM_PROTOCOLS") {
                    if let Ok(proto_reply) = proto_cookie.reply() {
                        let data = atom.to_ne_bytes();
                        let _ = self.conn.change_property(
                            PropMode::REPLACE,
                            self.window,
                            proto_reply.atom,
                            u32::from(AtomEnum::ATOM),
                            32,
                            1,
                            &data[..],
                        );
                    }
                }
                atom
            });

        // Cinturón y tirantes: `override_redirect` ya deja la ventana fuera
        // del WM (y por lo tanto sin decoraciones), pero hay WMs que lo
        // ignoran en algunos caminos. `_MOTIF_WM_HINTS` cubre esos.
        if self.override_redirect {
            let _ = x11::strip_decorations(&self.conn, self.window);
        }

        self.conn
            .flush()
            .map_err(|e| format!("flush tras create: {e}"))?;
        Ok(())
    }

    /// El handle nativo que hay que darle al plugin para que se embeba.
    ///
    /// Es la ventana de **contenido**, nunca el contenedor: el contenedor es el
    /// marco y no debe tocarlo nadie. Ver [`Self::prepare`].
    ///
    /// `XlibWindowHandle` y no `XcbWindowHandle`: los bindings de la ventana
    /// creada por `x11rb` son Xlib y es lo que espera
    /// `WindowHandle::from_x11` de VST3 y `Window::from_x11_handle` de CLAP.
    pub fn raw_handle(&self) -> RawWindowHandle {
        let target = self.content.unwrap_or(self.window);
        RawWindowHandle::Xlib(XlibWindowHandle::new(target as u64))
    }

    /// Prepara la ventana de contenido y devuelve el handle del plugin.
    ///
    /// Hay que llamarla **antes** de `show_gui_embedded`: el plugin necesita
    /// una ventana que ya exista, y el tamaño final del marco depende del
    /// tamaño que el plugin pidió.
    ///
    /// `preferred` es el tamaño que quiere la **vista** del plugin, así que
    /// la ventana del contenedor se agranda por los insets del marco para que
    /// el contenido reciba lo que pidió y no `pedido - marco`.
    pub fn prepare(&mut self, preferred: Option<(u32, u32)>) -> Result<RawWindowHandle, String> {
        if let Some((w, h)) = preferred {
            if w > 0 && h > 0 {
                let fw = u16::try_from(w)
                    .unwrap_or(DEFAULT_WIDTH)
                    .saturating_add(self.frame.horizontal_inset())
                    .max(self.frame.min_size().0);
                let fh = u16::try_from(h)
                    .unwrap_or(DEFAULT_HEIGHT)
                    .saturating_add(self.frame.vertical_inset())
                    .max(self.frame.min_size().1);
                self.resize(fw, fh);
            }
        }
        self.ensure_content()?;
        Ok(self.raw_handle())
    }

    /// Crea la ventana de contenido si no existe, y la coloca en el hueco.
    fn ensure_content(&mut self) -> Result<(), String> {
        if self.content.is_none() {
            let inner = self.frame.inner_rect(self.width(), self.height());
            let id = self
                .conn
                .generate_id()
                .map_err(|e| format!("generate_id(content): {e}"))?;

            // Fondo opaco y explícito: sin esto el contenido hereda el
            // fondo del root —que en una sesión de escritorio es negro— y
            // cada repintado del plugin muestra el rectángulo negro por
            // debajo de un editor que todavía no pintó nada.
            let attributes = CreateWindowAux::new()
                .border_pixel(0)
                .background_pixel(x11::Color::Surface.pixel())
                .event_mask(EventMask::EXPOSURE | EventMask::STRUCTURE_NOTIFY);

            self.conn
                .create_window(
                    self.parent_depth,
                    id,
                    self.window,
                    inner.x,
                    inner.y,
                    inner.width,
                    inner.height,
                    0,
                    WindowClass::INPUT_OUTPUT,
                    0,
                    &attributes,
                )
                .map_err(|e| format!("create_window(content): {e}"))?;

            // **La ventana de contenido tiene que mapearse explícitamente.**
            //
            // En X11 el estado de mapeo es **por ventana**: mapear el padre no
            // mapea a los hijos. Una ventana hija de una ventana que no está
            // mapeada queda `IsUnviewable` *para siempre*: el servidor no le
            // manda `Expose`, así que el toolkit del plugin (JUCE, Qt) nunca
            // tiene un framebuffer visible que pintar.
            //
            // El síntoma es exactamente el cuadro negro fantasma: el
            // contenedor dibuja su marco (se ve el título), la ventana de
            // contenido existe y tiene el tamaño correcto, pero la vista
            // que el plugin creó adentro está `IsUnviewable` y no pinta un
            // solo pixel. Sin este `MapWindow` el editor es invisible aunque
            // todo lo demás esté bien.
            //
            // Mapearla antes de que el contenedor esté mapeado no es un
            // problema: en X11 una ventana mapeada dentro de otra no mapeada
            // queda `IsUnviewable` y pasa a `IsViewable` —con su `Expose`—
            // en cuanto el padre se mapea.
            self.conn
                .map_window(id)
                .map_err(|e| format!("map_window(content): {e}"))?;

            let _ = self.conn.flush();
            self.content = Some(id);
        }
        self.layout_content();
        Ok(())
    }

    /// Pone la ventana de contenido en el hueco interior del marco.
    fn layout_content(&mut self) {
        let Some(content) = self.content else {
            return;
        };
        let inner = self.frame.inner_rect(self.width(), self.height());
        if !inner.is_usable() {
            return;
        }
        // El origen del contenido dentro del contenedor se guarda acá porque
        // la vista del plugin es **hija del contenido**, no del contenedor:
        // sin esta traducción, un click que cae en el contenedor hay que
        // restarle dos offsets y no uno.
        self.content_origin = (inner.x, inner.y);
        let _ = x11::move_resize(
            &self.conn,
            content,
            inner.x,
            inner.y,
            inner.width,
            inner.height,
        );
    }

    /// XID de la ventana del envoltorio.
    pub fn id(&self) -> u32 {
        self.window as u32
    }

    /// XID de la ventana de contenido, si ya existe.
    ///
    /// Es la ventana que se le da al plugin, y la que tiene que estar
    /// **mapeada** para que la vista del plugin pueda ser visible: en X11 el
    /// estado de mapeo es por ventana, así que una ventana de contenido sin
    /// mapear deja a la vista del plugin en `IsUnviewable` para siempre —con
    /// el editor congelado en negro— aunque el contenedor esté en pantalla.
    ///
    /// Se expone para que los tests puedan preguntarle al servidor X por el
    /// estado real de la superficie.
    pub fn content_id(&self) -> Option<u32> {
        self.content
    }

    /// El plan de hints que se aplicó.
    pub fn hint_plan(&self) -> &WrapperHintPlan {
        &self.plan
    }

    /// Muestra la ventana y, según la política, pide la activación.
    ///
    /// Recibe `preferred: Option<(u32, u32)>` con el tamaño que pidió el
    /// plugin: se ajusta antes del primer `map` para que el primer frame ya
    /// tenga el tamaño bueno en vez de estirarse después.
    pub fn show(&mut self, preferred: Option<(u32, u32)>) -> Result<(), String> {
        // El tamaño preferido del plugin es el de **su** vista. Como el marco
        // se come el título y los bordes, hay que agrandarlo para que el editor
        // reciba lo que pidió y no `pedido - marco`: si no, el plugin se
        // descuadra contra su propio layout.
        if let Some((w, h)) = preferred {
            if w > 0 && h > 0 {
                let fw = u16::try_from(w)
                    .unwrap_or(DEFAULT_WIDTH)
                    .saturating_add(self.frame.horizontal_inset())
                    .max(self.frame.min_size().0);
                let fh = u16::try_from(h)
                    .unwrap_or(DEFAULT_HEIGHT)
                    .saturating_add(self.frame.vertical_inset())
                    .max(self.frame.min_size().1);
                self.resize(fw, fh);
            }
        }

        // El contexto de dibujo se crea una vez, con la ventana ya en su
        // tamaño final: crearlo antes y redimensionar después deja el marco
        // despintado hasta el próximo `Expose`.
        if self.paint.is_none() {
            match x11::Paint::new(&self.conn, self.window) {
                Ok(paint) => {
                    if !paint.has_font() {
                        eprintln!(
                            "[hikaru-wrapper] el servidor X no tiene fuentes del servidor: \
                             el marco se dibuja sin el texto del título"
                        );
                    }
                    self.paint = Some(paint);
                }
                Err(e) => eprintln!("[hikaru-wrapper] sin contexto de dibujo: {e}"),
            }
        }

        self.layout_content();
        self.repaint();
        self.map()?;

        // El editor del plugin tiene que saber que ya hay alguien mirando.
        // El `Expose` sintético es lo que lo saca del "estoy mapeado pero no
        // me miran" en el que un toolkit se queda si su ventana ya estaba
        // mapeada antes de que existiera la jerarquía.
        self.request_repaint();

        // La activación va UNA vez, aquí. `keep_alive` sólo re-apila, nunca
        // enfoca: reenfocar periódicamente le robaría el foco al DAW.
        //
        // Con `override_redirect` no se pide: el WM no gestiona la ventana, así
        // que `_NET_ACTIVE_WINDOW` no la va a activar. El foco lo toma el
        // propio envoltorio con `SetInputFocus`, que es la vía directa y
        // no depende de que ningún WM colabore.
        if self.policy.activation == InputActivation::OnOpen && !self.override_redirect {
            let _ = x11::activate(&self.conn, self.root, self.window);
        }
        if self.policy.activation == InputActivation::OnOpen && self.override_redirect {
            let _ = x11::focus(&self.conn, self.window);
        }
        Ok(())
    }

    /// Mapea la ventana sin volver a crearla.
    ///
    /// Idempotente: el tick la llama en cada vuelta y mapear una ventana ya
    /// mapeada no tiene efecto, pero sí cuesta un viaje al servidor.
    pub fn map(&mut self) -> Result<(), String> {
        if self.mapped {
            return Ok(());
        }
        self.conn
            .map_window(self.window)
            .map_err(|e| format!("map_window: {e}"))?;
        self.mapped = true;
        if let Err(e) = self.conn.flush() {
            eprintln!("[hikaru-wrapper] aviso: flush tras map falló: {e:?}");
        }
        // La ventana de contenido es hija del contenedor: mapping el
        // contenedor no la mapea a ella (en X11 el estado de mapeo es por
        // ventana). Sin mapearla, la vista del plugin queda `IsUnviewable`
        // y nunca recibe un `Expose`: el editor no pinta nunca.
        if let Some(content) = self.content {
            let _ = self.conn.map_window(content);
        }
        let _ = self.conn.flush();
        // Recién ahora la superficie del plugin es visible de verdad: se le
        // manda el `Expose` para que empiece a emitir frames.
        self.request_repaint();
        Ok(())
    }

    /// Desmapea la ventana sin destruirla.
    ///
    /// Es lo que se hace cuando el DAW se minimiza o se oculta: el editor
    /// tiene que dejar de verse, pero no se destruye, porque el plugin sigue
    /// sonando y el usuario va a volver a la ventana.
    pub fn unmap(&mut self) {
        if !self.mapped {
            return;
        }
        let _ = self.conn.unmap_window(self.window);
        let _ = self.conn.flush();
        self.mapped = false;
    }

    /// Cambia el tamaño de la ventana del envoltorio.
    pub fn resize(&mut self, width: u16, height: u16) {
        if (width, height) == self.size {
            return;
        }
        let values = ConfigureWindowAux::new()
            .width(Some(width as u32))
            .height(Some(height as u32));
        let _ = self.conn.configure_window(self.window, &values);
        let _ = self.conn.flush();
        self.size = (width, height);
        self.layout_content();
        self.layout_view();
        self.repaint();
        self.request_repaint();
    }

    /// Busca, dentro de la ventana de contenido, la vista del plugin.
    ///
    /// Hay que llamarla **después** de que el plugin se haya embebido, y
    /// repetirla si el plugin recrea su vista. Es un viaje al servidor, así
    /// que no va en cada tick.
    pub fn refresh_view(&mut self) {
        // Se busca dentro del contenido, no del contenedor: el contenido es
        // lo que se le dio al plugin, así que su vista (si tiene) vive ahí.
        let Some(content) = self.content else {
            return;
        };
        match x11::find_view_window(&self.conn, content) {
            Ok(view) => {
                let changed = view.map(|v| v.window) != self.view.map(|v| v.window);
                self.view = view;
                self.layout_view();
                if changed {
                    // La vista del plugin acaba de aparecer: hay que
                    // colocarla y, sobre todo, avisarle de que su superficie
                    // ya es visible. Sin el `Expose` el editor se queda
                    // congelado en el color de fondo (el "cuadro negro
                    // fantasma").
                    self.repaint();
                    self.request_repaint();
                }
            }
            Err(e) => {
                // Perder la vista no es fatal: la ventana sigue visible y el
                // editor sigue dibujando, sólo no hay a quién redirigir un
                // evento si el plugin no seleccionó el suyo.
                eprintln!("[hikaru-wrapper] no se pudo leer la vista del editor: {e}");
            }
        }
    }

    /// Si se encontró una ventana hija a la que redirigir la entrada.
    pub fn view(&self) -> Option<&x11::ViewGeometry> {
        self.view.as_ref()
    }

    /// Le pide al plugin que repinte su superficie.
    ///
    /// X11 no tiene un "repintá esto": la única forma de que un toolkit
    /// vuelva a dibujar su ventana es que le llegue un `Expose`. El servidor
    /// lo genera cuando la ventana se mapea, pero **no** cuando la jerarquía
    /// se arma después: la vista del plugin ya estaba mapeada desde antes de
    /// que existiera su padre, así que su toolkit la cree expuesta, no le
    /// llega ningún `Expose` y se queda congelado sin pintar un pixel.
    ///
    /// Por eso el envoltorio genera el `Expose` a mano: es la forma barata y
    /// estándar de decirle "tu superficie es visible, emití un frame".
    ///
    /// Se manda a la vista del plugin y también al contenido, para el caso
    /// de los plugins que dibujan directamente sobre la ventana que les pasó
    /// el host y no crean ninguna hija.
    pub fn request_repaint(&mut self) {
        let inner = self.frame.inner_rect(self.width(), self.height());
        if let Some(content) = self.content {
            let _ = x11::send_expose(&self.conn, content, inner.width, inner.height);
        }
        if let Some(view) = self.view {
            let _ = x11::send_expose(&self.conn, view.window, view.width, view.height);
        }
        let _ = self.conn.flush();
    }

    /// Dónde quiere el DAW que viva el editor.
    ///
    /// Lo llama la vista del rack cuando el layout cambia (el usuario agranda
    /// el panel, cambia de pestaña, redimensiona la ventana). El rect es en
    /// coordenadas **de cliente de la ventana del DAW**.
    ///
    /// No reposiciona acá: el ajuste real lo hace el próximo
    /// [`Self::sync_with_host`], que ya tiene que preguntar la posición del
    /// DAW. Aplicarlo de las dos partes dejaría un frame de salto.
    pub fn set_host_frame(&mut self, frame: HostFrame) {
        if !frame.is_usable() {
            // Un rect vacío casi siempre es un layout que todavía no calculó
            // su editor. Respetarlo dejaría la ventana en 0×0 con el plugin
            // dentro y sin salida visible.
            return;
        }
        self.host_frame = Some(frame);
    }

    /// Si la ventana sigue atada a la del DAW.
    pub fn is_strict_child(&self) -> bool {
        self.strict_child
    }

    /// Re-engancha la ventana a la del DAW: posición, tamaño y visibilidad.
    ///
    /// Es el sentido de "hija modal estricta del proceso principal". Corre en
    /// cada tick y hace tres cosas, en orden:
    ///
    /// 1. Si el DAW dejó de estar visible (minimizado, en otro workspace,
    ///    tapado por un modal), el editor se desmapea. Sin esto el plugin
    ///    queda flotando solo sobre un escritorio donde no hay DAW.
    /// 2. Se traduce el cuadro de cliente del DAW a coordenadas de raíz,
    ///    sumándole la posición actual del DAW. Hace falta porque el DAW se
    ///    puede mover con el editor abierto.
    /// 3. Se limita el cuadro al tamaño del DAW y se aplica — pero sólo si
    ///    cambió, porque un `ConfigureWindow` por tick son 60 viajes al
    ///    servidor por ventana para no hacer nada.
    ///
    /// Con el DAW en Wayland no hay XID y no hay nada que atar: el editor es
    /// top-level y este método no hace nada. Es el límite duro del embedding
    /// X11 dentro de una superficie Wayland.
    pub fn sync_with_host(&mut self) {
        if !self.strict_child || !self.alive {
            return;
        }
        let Some(daw) = self.daw_xid else {
            return;
        };

        let Ok((daw_x, daw_y, daw_w, daw_h)) = x11::root_frame(&self.conn, daw) else {
            // Si el DAW no responde, tampoco se lo puede seguir. Se deja la
            // ventana como está en vez de adivinar.
            return;
        };

        match x11::is_viewable_root(&self.conn, daw) {
            Ok(true) => {}
            Ok(false) => {
                if self.host_visible {
                    self.host_visible = false;
                    self.unmap();
                }
                return;
            }
            Err(_) => return,
        }
        if !self.host_visible {
            self.host_visible = true;
            let _ = self.map();
        }

        let Some(frame) = self.host_frame else {
            return;
        };
        let frame = frame.clamp_to(daw_w, daw_h);
        let root = (
            (daw_x.saturating_add(frame.x)).max(0),
            (daw_y.saturating_add(frame.y)).max(0),
            frame.width,
            frame.height,
        );

        if self.applied_root_frame == Some(root) {
            return;
        }
        let is_child_of_daw = self.window_parent() == Some(daw);
        let moved = self.applied_root_frame.map(|(x, y, _, _)| (x, y) != (root.0, root.1)).unwrap_or(true);

        // Una ventana que **ya es hija** del DAW en el árbol de X11 no quiere
        // coordenadas de raíz: quiere coordenadas de cliente. Pasarle las de
        // raíz la manda al lugar equivocado.
        let resized = self.size != (root.2, root.3);
        let applied = if is_child_of_daw {
            let (_, _, aw, ah) = self.applied_root_frame.unwrap_or((0, 0, 0, 0));
            if !moved && aw == root.2 && ah == root.3 {
                return;
            }
            let _ = x11::move_resize(&self.conn, self.window, frame.x, frame.y, root.2, root.3);
            (frame.x, frame.y, root.2, root.3)
        } else {
            if !moved && ax_eq(self.applied_root_frame, root) {
                return;
            }
            let _ = x11::move_resize(&self.conn, self.window, root.0, root.1, root.2, root.3);
            root
        };
        let _ = self.conn.flush();
        self.applied_root_frame = Some(applied);

        // Si el cuadro del DAW cambió de tamaño, la vista del plugin tiene que
        // volver a colocarse: sin esto el editor se queda con el tamaño viejo
        // pegado a la esquina cuando el usuario agranda el panel del rack.
        if resized {
            self.size = (root.2, root.3);
            self.layout_view();
            self.repaint();
            self.request_repaint();
        }
    }

    /// El padre de la ventana en el árbol de X11, si lo tiene.
    ///
    /// `None` para una ventana top-level, que es el caso normal en Wayland.
    pub fn window_parent(&self) -> Option<Window> {
        self.daw_xid.filter(|_| self.child_of_daw)
    }

    /// Si la ventana se creó como hija del DAW en el árbol de X11.
    pub fn is_child_of_daw(&self) -> bool {
        self.child_of_daw
    }

    /// Si todavía no se encontró la ventana hija del plugin.
    pub fn needs_view_scan(&self) -> bool {
        self.view.is_none()
    }

    /// Mantenimiento por tick del marco.
    ///
    /// Descubre la ventana hija del plugin mientras no se encuentre, y avisa
    /// si pasa el rato sin aparecer.
    ///
    /// # Por qué hay que seguir buscando
    ///
    /// `show_gui_embedded` **no** garantiza que la vista exista: hay plugins
    /// que crean su ventana hija en el primer pintado, otros recién cuando
    /// les llega un `ConfigureNotify`, y hay toolkits que la recrean cada vez
    /// que cambian de tamaño. Si se busca una sola vez y no estaba, la vista
    /// queda `None`, [`Self::layout_view`] no hace nada, el plugin se queda
    /// con toda la ventana y **pinta encima del marco**: es exactamente lo que
    /// se vio con MonaChroma, cuyo título y bordes no aparecían.
    ///
    /// No se rinde nunca:_some plugins son lentos y rendirse dejaría al
    /// usuario con un editor sin marco y sin explicación. El aviso sale una
    /// sola vez para no llenar el log.
    pub fn maintain(&mut self, tick: u64) {
        if !self.needs_view_scan() {
            return;
        }
        const SCAN_EVERY: u64 = 20;
        if tick % SCAN_EVERY != 0 {
            return;
        }
        let had = self.view;
        self.refresh_view();
        if had.is_some() || self.view.is_some() {
            return;
        }
        self.view_scan_failures += 1;
        if self.view_scan_failures == 6 {
            eprintln!(
                "[hikaru-wrapper] el plugin no creó una ventana hija. Va a dibujar \
                 sobre el contenedor, así que el marco queda tapado por el editor. \
                 Es normal en plugins que renderizan directamente sobre la ventana \
                 que les pasa el host."
            );
        }
    }

    /// Si la ventana está mapeada ahora mismo.
    pub fn is_visible(&self) -> bool {
        self.mapped && self.alive
    }

    /// Si el DAW se está viendo ahora mismo.
    pub fn is_host_visible(&self) -> bool {
        self.host_visible
    }

    /// Si la ventana se creó sin decoraciones del gestor de ventanas.
    pub fn is_borderless(&self) -> bool {
        self.override_redirect
    }


// ───────────────────────────── El marco ─────────────────────────────

/// Las medidas del marco del editor.
///
/// `FrameMetrics::frameless()` para los plugins que ya traen su propio
/// titlebar: envolverlos en otro marco se ve doble.
pub fn set_frame_metrics(&mut self, metrics: FrameMetrics) {
    self.frame = metrics;
    self.repaint();
}

/// El texto del titlebar.
pub fn set_title(&mut self, title: &str) {
    if self.title == title {
        return;
    }
    self.title = title.to_string();
    self.repaint();
}

/// Dibuja el marco completo.
///
/// Cheap porque no hace nada sin marco: el caso `frameless` paga un `if` por
/// repintado en vez de un viaje al servidor.
pub fn repaint(&mut self) {
    if !self.frame.has_frame() {
        return;
    }
    if self.paint.is_none() {
        return;
    }
    if let Err(e) = self.paint_frame() {
        eprintln!("[hikaru-wrapper] no se pudo pintar el marco: {e}");
    }
}

fn paint_frame(&mut self) -> Result<(), String> {
    // `let ... else` sobre `&self` prestaría el struct entero y después
    // `paint_frame` lo necesita mutable. El contexto de dibujo son dos `u32`
    // (ids del servidor), así que copiarlo no cuesta nada.
    let Some(paint) = self.paint.clone() else {
        return Ok(());
    };
    let (w, h) = (self.width(), self.height());
    let m = self.frame;
    let window: Drawable = self.window.into();

    // 1. Fondo del titlebar. Se rellena la franja entera, no sólo hasta el
    //    botón: así el botón queda incluido en el mismo color y no hay que
    //    calcular un solapamiento.
    let titlebar_bg = if self.focused {
        x11::Color::TitlebarFocused
    } else {
        x11::Color::Titlebar
    };
    paint.fill_rect(&self.conn, window, titlebar_bg, 0, 0, w, m.titlebar_height)?;
    paint.fill_rect(
        &self.conn,
        window,
        x11::Color::Border,
        0,
        0,
        w,
        m.border,
    )?;
    paint.fill_rect(
        &self.conn,
        window,
        x11::Color::Border,
        0,
        h.saturating_sub(m.border) as i16,
        w,
        m.border,
    )?;
    paint.fill_rect(&self.conn, window, x11::Color::Border, 0, 0, m.border, h)?;
    paint.fill_rect(
        &self.conn,
        window,
        x11::Color::Border,
        w.saturating_sub(m.border) as i16,
        0,
        m.border,
        h,
    )?;

    // 2. Texto del título. La línea base va unos píxeles antes del borde
    //    inferior del titlebar, que es donde una fuente de 13 px queda
    //    centrada sin tener que medir nada.
    if m.titlebar_height > 0 {
        let baseline = m.titlebar_height.saturating_sub(5) as i16;
        paint.draw_text(
            &self.conn,
            window,
            x11::Color::Text,
            m.title_padding as i16,
            baseline,
            &self.title,
        )?;
    }

    // 3. Botón de cerrar: una "X" de dos líneas. Es un glifo, no una fuente,
    //    así que se ve igual en cualquier servidor.
    if m.close_button > 0 {
        let size = m.close_button as i16;
        let pad = m.title_padding as i16;
        let cx = w as i16 - pad - size / 2;
        let cy = m.titlebar_height as i16 / 2;
        let arm = (size / 3).max(2);
        let color = if self.close_hot {
            x11::Color::CloseHover
        } else {
            x11::Color::Border
        };
        if self.close_hot {
            // Relleno del botón sólo cuando está bajo el puntero: es la
            // única señal de que el clic va a cerrar y no a mover.
            paint.fill_rect(
                &self.conn,
                window,
                x11::Color::CloseHover,
                cx - arm - 2,
                cy - arm - 2,
                (arm * 2 + 4) as u16,
                (arm * 2 + 4) as u16,
            )?;
            let x_color = x11::Color::Text;
            paint.draw_line(&self.conn, window, x_color, cx - arm, cy - arm, cx + arm, cy + arm)?;
            paint.draw_line(&self.conn, window, x_color, cx + arm, cy - arm, cx - arm, cy + arm)?;
        } else {
            let _ = color;
            paint.draw_line(&self.conn, window, color, cx - arm, cy - arm, cx + arm, cy + arm)?;
            paint.draw_line(&self.conn, window, color, cx + arm, cy - arm, cx - arm, cy + arm)?;
        }
    }

    // 4. Tirador de redimensionado: tres diagonales.
    if m.resize_grip > 0 {
        let g = m.resize_grip as i16;
        let bx = w as i16 - 1;
        let by = h as i16 - 1;
        for i in 0..3 {
            let offset = i * 4;
            paint.draw_line(
                &self.conn,
                window,
                x11::Color::Grip,
                bx - g + offset,
                by,
                bx,
                by - g + offset,
            )?;
        }
    }

    let _ = self.conn.flush();
    Ok(())
}

/// Pone la vista del plugin en el rectángulo interior del marco.
///
/// Es la mitad importante de "el marco": el plugin cree que ocupa toda la
/// ventana que se le pasó, así que hay que moverla y redimensionarla a mano al
/// hueco que deja el marco. Sin esto, el editor se dibuja **debajo** del
/// titlebar y del botón de cerrar.
///
/// # El espacio de coordenadas
///
/// La vista es hija de la ventana de **contenido**, no del contenedor: lo que
/// se le pasó al plugin fue el contenido (ver [`Self::prepare`]). Por eso sus
/// coordenadas son relativas al contenido y va en `(0, 0)` ocupando todo el
/// hueco. Usar el rect interior del marco —que es relativo al contenedor—
/// dejaba la vista desplazada por el alto del titlebar y un borde más abajo
/// del corte.
pub fn layout_view(&mut self) {
    self.layout_content();
    let Some(view) = self.view else {
        return;
    };
    let inner = self.frame.inner_rect(self.width(), self.height());
    if !inner.is_usable() {
        return;
    }
    if view.x == 0 && view.y == 0 && view.width == inner.width && view.height == inner.height {
        return;
    }
    if let Err(e) = x11::move_resize(
        &self.conn,
        view.window,
        0,
        0,
        inner.width,
        inner.height,
    ) {
        eprintln!("[hikaru-wrapper] no se pudo colocar la vista del editor: {e}");
    }
    self.view = Some(x11::ViewGeometry::new(
        view.window,
        0,
        0,
        inner.width,
        inner.height,
    ));
}

/// La geometría de la vista en coordenadas del **contenedor**.
///
/// `GetGeometry` devuelve las coordenadas de la vista relativas a su padre, y
/// su padre es la ventana de contenido. Para traducir un evento que llegó al
/// contenedor hay que convertirlas a relativas al contenedor: se le suma el
/// origen del contenido (ver [`Self::content_origin`]).
fn view_in_container(&self) -> Option<x11::ViewGeometry> {
    let view = self.view.as_ref()?;
    let (cx, cy) = self.content_origin;
    Some(x11::ViewGeometry::new(
        view.window,
        cx.saturating_add(view.x),
        cy.saturating_add(view.y),
        view.width,
        view.height,
    ))
}

/// El tamaño del editor, con el marco descontado.
///
/// Es lo que hay que pasarle a `resize_gui`: mandar el tamaño de la ventana
/// entera haría que el plugin intente dibujar más grande que su hueco.
pub fn inner_size(&self) -> (u16, u16) {
    let inner = self.frame.inner_rect(self.width(), self.height());
    (inner.width, inner.height)
}

/// El tamaño exterior actual de la ventana.
pub fn width(&self) -> u16 {
    self.size.0
}

pub fn height(&self) -> u16 {
    self.size.1
}

/// Qué hizo el marco con un clic.
///
/// Se devuelve para que el bucle sepa si tiene que seguir reenviando el
/// evento al plugin o si ya se lo consumió.
pub fn on_button_press(
    &mut self,
    x: i16,
    y: i16,
    root_x: i16,
    root_y: i16,
) -> FrameAction {
    let region = self.frame.hit_test(x, y, self.width(), self.height());
    match region {
        FrameRegion::Inner => FrameAction::None,
        FrameRegion::CloseButton => FrameAction::Close,
        FrameRegion::TitleBar => {
            self.drag = Some(FrameDrag::Move {
                origin: (root_x, root_y),
                window_origin: (self.root_pos.0, self.root_pos.1),
            });
            FrameAction::Handled
        }
        FrameRegion::Resize(dir) => {
            self.drag = Some(FrameDrag::Resize {
                dir,
                start_size: (self.width(), self.height()),
                origin: (root_x, root_y),
            });
            FrameAction::Handled
        }
    }
}

/// Un clic en la barra de título, para actualizar el botón de cerrar.
pub fn on_button_release(&mut self) {
    self.drag = None;
}

/// Movimiento del puntero: mueve o redimensiona la ventana.
pub fn on_motion(&mut self, root_x: i16, root_y: i16) -> FrameAction {
    // Con un gesto en curso, el puntero manda sobre el hover.
    if let Some(drag) = self.drag {
        let origin = match drag {
            FrameDrag::Move { origin, .. } | FrameDrag::Resize { origin, .. } => origin,
        };
        let (dx, dy) = (
            root_x.saturating_sub(origin.0),
            root_y.saturating_sub(origin.1),
        );
        match drag {
            FrameDrag::Move { window_origin, .. } => {
                let nx = window_origin.0.saturating_add(dx).max(0);
                let ny = window_origin.1.saturating_add(dy).max(0);
                let (nx, ny) = self.clamp_to_host(nx, ny, self.width(), self.height());
                if (nx, ny) != self.root_pos {
                    let _ = x11::move_resize(&self.conn, self.window, nx, ny, self.width(), self.height());
                    let _ = self.conn.flush();
                    self.root_pos = (nx, ny);
                }
            }
            FrameDrag::Resize {
                dir,
                start_size,
                ..
            } => {
                let (min_w, min_h) = self.frame.min_size();
                let min_w = min_w.max(MIN_FRAME_WIDTH);
                let min_h = min_h.max(MIN_FRAME_HEIGHT);

                let w = (start_size.0 as i32 - i32::from(dir.dx(dx))).max(i32::from(min_w));
                let h = (start_size.1 as i32 - i32::from(dir.dy(dy))).max(i32::from(min_h));
                let w = (w as u16).min(u16::MAX);
                let h = (h as u16).min(u16::MAX);

                // Al redimensionar desde un borde que no es el inferior, hay
                // que mover la ventana para que el borde se quede pegado al
                // puntero en vez de arrastrarse con él.
                let nx = if dir.dx(dx) > 0 {
                    self.root_pos.0.saturating_add(dx)
                } else {
                    self.root_pos.0
                };
                let ny = if dir.dy(dy) > 0 {
                    self.root_pos.1.saturating_add(dy)
                } else {
                    self.root_pos.1
                };

                if (w, h) != (self.width(), self.height()) {
                    let _ = x11::move_resize(&self.conn, self.window, nx, ny, w, h);
                    let _ = self.conn.flush();
                    self.size = (w, h);
                    self.layout_view();
                    self.repaint();
                    self.request_repaint();
                }
            }
        }
        return FrameAction::Handled;
    }

    // Sin gesto: hover. Sólo hay que saber si el botón de cerrar se
    // ilumina, y eso no pasa todos los ticks.
    let region = self.frame.hit_test(self.last_pointer.0, self.last_pointer.1, self.width(), self.height());
    let hot = region == FrameRegion::CloseButton;
    if hot != self.close_hot {
        self.close_hot = hot;
        self.repaint();
    }
    FrameAction::None
}

/// Deja el puntero donde estaba, para el hit-test del hover.
pub fn set_pointer(&mut self, x: i16, y: i16) {
    self.last_pointer = (x, y);
}

/// Aplica el cursor al servidor.
pub fn set_cursor(&mut self, hint: CursorHint) {
    x11::set_cursor(&self.conn, self.window, hint);
}

/// Si el gesto en curso es el del título (para el cursor).
pub fn is_dragging(&self) -> bool {
    matches!(self.drag, Some(FrameDrag::Move { .. }))
}

/// Impide que la ventana se salga del DAW.
///
/// Sin esto el usuario puede arrastrar el editor hasta media pantalla y
/// dejarlo pegado al borde, con el DAW detrás: exactamente lo que un
/// gestor de ventanas modal no dejaría.
fn clamp_to_host(&self, x: i16, y: i16, w: u16, h: u16) -> (i16, i16) {
    let Some(daw) = self.daw_xid else {
        return (x, y);
    };
    let Ok((dx, dy, dw, dh)) = x11::root_frame(&self.conn, daw) else {
        return (x, y);
    };
    let max_x = (dx + dw as i16).saturating_sub(w as i16).max(dx);
    let max_y = (dy + dh as i16).saturating_sub(h as i16).max(dy);
    (x.clamp(dx, max_x), y.clamp(dy, max_y))
}

/// Re-afirma el estado de "arriba" si toca.

/// El cursor que hay que mostrar ahora, si cambió.
///
/// Devuelve `None` cuando no hay nada que hacer: se compara con el cursor
/// anterior para no reescribir la propiedad en cada tick.
pub fn cursor(&mut self) -> Option<CursorHint> {
    let hint = if self.is_dragging() {
        CursorHint::Default
    } else {
        self.frame.cursor_for(
            self.frame
                .hit_test(self.last_pointer.0, self.last_pointer.1, self.width(), self.height()),
        )
    };
    if self.cursor == Some(hint) {
        return None;
    }
    self.cursor = Some(hint);
    Some(hint)
}

    ///
    /// Devuelve `true` si hizo un viaje al servidor. `raise_requested` es
    /// para el caso de que el DAW acaba de recuperar la activación: entonces
    /// el WM acaba de subir su propia ventana y hay que devolver la del
    /// editor a su sitio ya, sin esperar al próximo turno del keep-alive.
    pub fn tick_keep_alive(&mut self, raise_requested: bool) {
        let now = elapsed_ms(self.started);
        let due = raise_requested || self.keep_alive.due(now);
        if !due || !self.alive {
            return;
        }
        // Se consulta `is_viewable` antes de tocar nada: si el WM ya
        // destruyó la ventana, `ConfigureWindow` devuelve `BadWindow` y
        // llenaría el log de errores sobre una ventana que ya no existe.
        match x11::is_viewable(&self.conn, self.window) {
            Ok(true) => {
                if let Err(e) = x11::raise(&self.conn, self.window) {
                    eprintln!("[hikaru-wrapper] re-elevar falló: {e}");
                }
            }
            Ok(false) => self.alive = false,
            Err(e) => {
                eprintln!("[hikaru-wrapper] no se pudo consultar la ventana: {e}");
                self.alive = false;
            }
        }
    }

    /// Si hay que reparar el foco de teclado.
    ///
    /// Sólo cuando la política lo pide **y** el puntero está sobre la
    /// ventana: exigir el foco sin que el usuario esté mirando la ventana es
    /// exactamente el comportamiento molesto que rompe un envoltorio — sería
    /// el mismo problema que robar el foco con `_NET_ACTIVE_WINDOW` en bucle.
    ///
    /// El puntero se sigue con `EnterNotify`/`LeaveNotify`, que ya estaban en
    /// la máscara: no cuesta ningún viaje al servidor por tick.
    pub fn should_repair_focus(&self) -> bool {
        self.policy.repair_focus && self.alive && self.pointer_inside
    }

    /// Le devuelve el foco a la ventana del envoltorio.
    pub fn repair_focus(&self) {
        match x11::focused_window(&self.conn) {
            Ok(Some(focused)) if focused == self.window => {}
            _ => {
                let _ = x11::focus(&self.conn, self.window);
            }
        }
    }

    /// Drena los eventos pendientes de la ventana.
    ///
    /// No bloquea: devuelve en cuanto no queda nada. Traduce los eventos de
    /// puntero que cayeron en el contenedor y se los reenvía a la vista del
    /// plugin (ver el módulo [`super::input`]), y devuelve lo que el bucle
    /// necesita saber.
    pub fn poll_events(&mut self) -> WindowEvent {
        let mut outcome = WindowEvent::Idle;

        loop {
            match self.conn.poll_for_event() {
                Ok(None) => break,
                Ok(Some(Event::ClientMessage(msg))) => {
                    if self.wm_delete.is_some_and(|atom| msg.type_ == atom) {
                        return WindowEvent::CloseRequested;
                    }
                }
                Ok(Some(Event::DestroyNotify(ev))) if ev.window == self.window => {
                    self.alive = false;
                    return WindowEvent::Destroyed;
                }
                Ok(Some(Event::ConfigureNotify(ev))) if ev.window == self.window => {
                    // El `ConfigureNotify` también llega por los `ConfigureWindow`
                    // que hacemos nosotros, así que esta es la única vía por la
                    // que la caché de tamaño queda al día cuando el cambio lo
                    // origina el DAW o `sync_with_host` y no nuestro propio
                    // `resize`. Sin esto el marco calcularía el rect interior
                    // sobre un tamaño viejo y colocaría la vista del plugin en
                    // el lugar equivocado.
                    if ev.width > 0 && ev.height > 0 {
                        if (ev.width, ev.height) != self.size {
                            self.size = (ev.width, ev.height);
                            self.applied_root_frame = None;
                            self.layout_view();
                            self.repaint();
                        }
                        outcome = WindowEvent::Resized {
                            width: ev.width as u32,
                            height: ev.height as u32,
                        };
                    }
                    if ev.x != self.root_pos.0 || ev.y != self.root_pos.1 {
                        self.root_pos = (ev.x, ev.y);
                    }
                }
                Ok(Some(Event::ButtonPress(e))) => {
                    let (w, x, y) = (e.event, e.event_x, e.event_y);
                    // El marco sólo existe en el contenedor. Un evento que
                    // ocurrió en la ventana de contenido llega con coordenadas
                    // relativas a **ella**, y pasárselas al hit-test haría que
                    // un clic en la esquina del editor se leyera como un clic
                    // en el título. La aritmética sería un error esperando:
                    // "si el evento no fue en el contenedor, no es del marco".
                    if w == self.window {
                        self.set_pointer(x, y);
                        match self.on_button_press(x, y, e.root_x, e.root_y) {
                            FrameAction::None => {
                                if self.forward(
                                    w,
                                    x,
                                    y,
                                    EventMask::BUTTON_PRESS,
                                    e.serialize(),
                                ) {
                                    // El plugin se quedó con la presión:
                                    // hay que acordarse para soltarle el
                                    // botón aunque el puntero se vaya de la
                                    // vista (si no, el control queda pegado).
                                    self.pressed_in_view = true;
                                    // Y el foco de teclado pasa al editor:
                                    // el foco del teclado va a una ventana
                                    // y sube a sus ancestros, nunca baja a
                                    // los hijos, así que enfocando el
                                    // contenedor el plugin no vería
                                    // ninguna tecla.
                                    self.focus_view();
                                }
                            }
                            FrameAction::Close => return WindowEvent::CloseRequested,
                            FrameAction::Handled => {}
                        }
                    } else {
                        self.forward(w, x, y, EventMask::BUTTON_PRESS, e.serialize());
                    }
                }
                Ok(Some(Event::ButtonRelease(e))) => {
                    // El final de un arrastre no es un clic del plugin: si se
                    // reenvía, el editor se come un `ButtonRelease` sin su
                    // `ButtonPress` y se queda con un knob pegado.
                    let ended_drag = self.drag.is_some();
                    self.drag = None;
                    if !ended_drag {
                        let (w, x, y) = (e.event, e.event_x, e.event_y);
                        self.forward(w, x, y, EventMask::BUTTON_RELEASE, e.serialize());
                        self.pressed_in_view = false;
                    }
                }
                Ok(Some(Event::MotionNotify(e))) => {
                    let (w, x, y) = (e.event, e.event_x, e.event_y);
                    // Durante un arrastre el puntero puede haber salido del
                    // contenedor (típico: se arrastra más rápido de lo que el
                    // servidor entrega eventos). Con el grab implícito de X11
                    // los eventos siguen llegando acá, así que basta con mirar
                    // que haya un gesto vivo, no dónde cayó el puntero.
                    if self.drag.is_some() {
                        self.on_motion(e.root_x, e.root_y);
                        continue;
                    }
                    if w == self.window {
                        self.set_pointer(x, y);
                        let _ = self.on_motion(e.root_x, e.root_y);
                    }
                    self.forward(w, x, y, EventMask::POINTER_MOTION, e.serialize());
                }
                Ok(Some(Event::Expose(ev))) if ev.window == self.window => {
                    // Repintar en cada `Expose` es lo que mantiene vivo el
                    // marco mientras se arrastra la ventana: sin esto el
                    // titlebar se borra y queda del color del escritorio.
                    if ev.count == 0 {
                        self.repaint();
                    }
                }
                Ok(Some(Event::FocusIn(_))) => {
                    self.focused = true;
                    self.repaint();
                }
                Ok(Some(Event::FocusOut(_))) => {
                    self.focused = false;
                    self.close_hot = false;
                    self.repaint();
                }
                Ok(Some(Event::EnterNotify(ev))) if ev.event == self.window => {
                    self.pointer_inside = true;
                }
                Ok(Some(Event::LeaveNotify(ev))) if ev.event == self.window => {
                    self.pointer_inside = false;
                }
                Ok(Some(_)) => {}
                Err(e) => {
                    self.alive = false;
                    return WindowEvent::ConnectionLost(format!("{e:?}"));
                }
            }
        }

        outcome
    }

    /// Reenvía un evento de puntero a la vista del plugin si corresponde.
    ///
    /// `raw` es el evento tal cual lo dio el servidor, ya serializado a los
    /// 32 bytes del protocolo X11. La serialización ocurre en el `match`
    /// ([`Self::poll_events`]) porque el enum `Event` no la implementa: hay
    /// que hacerlo sobre la variante concreta.
    /// Reenvía un evento de puntero a la vista del plugin.
    ///
    /// # Por qué hace falta
    ///
    /// La vista del plugin es una ventana **hija** de la ventana de
    /// contenido. En X11, un evento de puntero lo recibe el cliente que
    /// seleccionó ese tipo de evento en la ventana donde ocurrió, así que
    /// hay dos casos:
    ///
    /// - El plugin selecciona el evento en su propia ventana: lo recibe
    ///   solo, y acá no hay nada que hacer.
    /// - El plugin **no** lo selecciona: el evento sube por la cadena de
    ///   ancestros hasta la primera ventana que lo seleccionó, que es el
    ///   contenedor. Lo recibe el host, que es quien tiene que se lo pasar
    ///   al plugin.
    ///
    /// El segundo caso es el de los toolkits que dibujan sobre la ventana que
    /// les pasó el host, o que esperan que el host les reenvíe la entrada.
    ///
    /// # Las coordenadas
    ///
    /// El evento llega con las coordenadas relativas a la ventana que lo
    /// seleccionó (el contenedor). El plugin las necesita relativas a **su**
    /// ventana, así que hay que restarle el offset de la vista —que está
    /// dentro de la de contenido, que a su vez está dentro del contenedor.
    ///
    /// Devuelve `true` si el evento llegó al plugin.
    fn forward(
        &mut self,
        event_window: Window,
        x: i16,
        y: i16,
        mask: EventMask,
        raw: [u8; 32],
    ) -> bool {
        // La geometría que se usa para traducir coordenadas es la de la
        // vista **relativa al contenedor**: el evento llega al contenedor y
        // hay que convertirlo al espacio del editor. `GetGeometry` la da
        // relativa al contenido, que es un nivel más abajo.
        let Some(view) = self.view_in_container() else {
            return false;
        };
        let disposition =
            x11::plan_input_disposition(event_window, x, y, self.window, Some(&view));

        // Una liberación fuera de la vista no se puede descartar: el plugin
        // se quedó con la presión y necesita la liberación, o el control
        // queda **pegado** (el knob sigue siguiendo al puntero y el
        // siguiente clic se come por duplicado). Se manda igual, con las
        // coordenadas clavadas al borde de la vista.
        let (x, y) = match disposition {
            x11::InputDisposition::Forward { x, y } => (x, y),
            x11::InputDisposition::Drop
                if self.pressed_in_view && mask == EventMask::BUTTON_RELEASE =>
            {
                (view.clamp_x(x), view.clamp_y(y))
            }
            _ => return false,
        };

        let rewritten = x11::rewrite_pointer_event(&view, x, y, raw);
        if let Err(e) = x11::forward_pointer_event(&self.conn, &view, mask, rewritten) {
            eprintln!("[hikaru-wrapper] no se pudo redirigir el evento al editor: {e}");
            return false;
        }
        true
    }

    /// Le da el foco de teclado a la vista del plugin.
    ///
    /// El foco de teclado en X11 va a **una** ventana y de ahí sube a sus
    /// ancestros: nunca baja a los hijos. El editor del plugin es un hijo
    /// de la ventana de contenido, así que enfocando el contenedor el plugin
    /// no ve ninguna tecla. Hay que enfocarlo a él.
    ///
    /// Sin foco, el editor recibe el ratón pero no el teclado: los atajos del
    /// plugin (bypass,octavas,...) no funcionan y los campos de texto no
    /// admiten escritura.
    pub fn focus_view(&mut self) {
        let Some(view) = self.view else {
            return;
        };
        let _ = x11::focus(&self.conn, view.window);
    }

    /// Suelta el vínculo padre-hijo y destruye la ventana.
    ///
    /// Idempotente. El orden importa: `detach` antes de `destroy_window`, o
    /// el WM conserva una entrada para una ventana que ya no existe.
    fn destroy_window(&mut self) {
        if !self.alive {
            return;
        }
        x11::detach(&self.conn, self.window);
        let _ = self.conn.destroy_window(self.window);
        let _ = self.conn.flush();
        self.alive = false;
    }

    /// Si la ventana sigue viva según el último estado conocido.
    pub fn is_alive(&self) -> bool {
        self.alive
    }

    /// Número de pantalla de la conexión.
    pub fn screen_num(&self) -> usize {
        self.screen_num
    }
}

impl Drop for HikaruWrapperWindow {
    fn drop(&mut self) {
        self.destroy_window();
        // La conexión se cierra al salir de la función, en último lugar: así
        // ni el plugin ni el WM pisan un XID muerto.
    }
}

/// Convierte la política de entrada a la máscara de `EventMask` de X11.
///
/// Separado de la llamada porque es la decisión que hay que poder testear:
/// el arreglo de "los clicks no llegan" es, literalmente, que estos bits
/// estén todos.
fn event_mask(mask: InputMask) -> EventMask {
    let mut bits = EventMask::NO_EVENT;
    if mask.exposure {
        bits |= EventMask::EXPOSURE;
    }
    if mask.structure_notify {
        bits |= EventMask::STRUCTURE_NOTIFY;
    }
    if mask.focus_change {
        bits |= EventMask::FOCUS_CHANGE;
    }
    if mask.key_press {
        bits |= EventMask::KEY_PRESS;
    }
    if mask.key_release {
        bits |= EventMask::KEY_RELEASE;
    }
    if mask.button_press {
        bits |= EventMask::BUTTON_PRESS;
    }
    if mask.button_release {
        bits |= EventMask::BUTTON_RELEASE;
    }
    if mask.button_motion {
        bits |= EventMask::BUTTON_MOTION;
    }
    if mask.pointer_motion {
        bits |= EventMask::POINTER_MOTION;
    }
    if mask.enter_window {
        bits |= EventMask::ENTER_WINDOW;
    }
    if mask.leave_window {
        bits |= EventMask::LEAVE_WINDOW;
    }
    bits
}

/// Si el cuadro aplicado coincide con el pedido (posición **y** tamaño).
fn ax_eq(applied: Option<(i16, i16, u16, u16)>, wanted: (i16, i16, u16, u16)) -> bool {
    applied == Some(wanted)
}

/// Milisegundos transcurridos desde el arranque del hilo de la ventana.
///
/// Se usa `Instant` y no el reloj de pared a propósito: un ajuste de hora
/// (NTP, cambio de zona) no puede hacer que el keep-alive dispare en bucle ni
/// se duerma medio minuto.
fn elapsed_ms(start: Instant) -> u64 {
    start.elapsed().as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mask_selects_every_pointer_event() {
        let bits = event_mask(InputMask::default());
        for bit in [
            EventMask::BUTTON_PRESS,
            EventMask::BUTTON_RELEASE,
            EventMask::BUTTON_MOTION,
            EventMask::POINTER_MOTION,
            EventMask::ENTER_WINDOW,
            EventMask::LEAVE_WINDOW,
            EventMask::KEY_PRESS,
            EventMask::KEY_RELEASE,
            EventMask::FOCUS_CHANGE,
            EventMask::EXPOSURE,
            EventMask::STRUCTURE_NOTIFY,
        ] {
            assert!(
                bits.contains(bit),
                "falta {:?} en la máscara del contenedor",
                bit
            );
        }
    }

    #[test]
    fn view_only_mask_has_no_pointer_bits() {
        let bits = event_mask(InputMask::display_only());
        assert!(!bits.contains(EventMask::BUTTON_PRESS));
        assert!(!bits.contains(EventMask::BUTTON_RELEASE));
        assert!(!bits.contains(EventMask::POINTER_MOTION));
        // Pero sí repinta y sigue los resizes.
        assert!(bits.contains(EventMask::EXPOSURE));
        assert!(bits.contains(EventMask::STRUCTURE_NOTIFY));
    }

    #[test]
    fn mask_never_picks_up_an_implicit_bit() {
        // `event_mask` se construye bit a bit: nada de `from_bits_truncate`
        // con una constante escrita a mano que se pueda desincronizar.
        let bits = event_mask(InputMask::default());
        assert_eq!(
            bits.bits() & !(EventMask::EXPOSURE
                | EventMask::STRUCTURE_NOTIFY
                | EventMask::FOCUS_CHANGE
                | EventMask::KEY_PRESS
                | EventMask::KEY_RELEASE
                | EventMask::BUTTON_PRESS
                | EventMask::BUTTON_RELEASE
                | EventMask::BUTTON_MOTION
                | EventMask::POINTER_MOTION
                | EventMask::ENTER_WINDOW
                | EventMask::LEAVE_WINDOW)
                .bits(),
            0
        );
    }

    #[test]
    fn elapsed_never_goes_backwards() {
        let start = Instant::now();
        let a = elapsed_ms(start);
        let b = elapsed_ms(start);
        assert!(b >= a);
    }
}