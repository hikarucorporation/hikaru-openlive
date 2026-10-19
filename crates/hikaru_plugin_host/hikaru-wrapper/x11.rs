// Copyright (C) Hikaru Corporation - 2026
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/src/hikaru_wrapper/x11.rs

//! Primitivas X11 del envoltorio: hints, activación, foco y redirección de
//! entrada.
//!
//! Este módulo es deliberadamente "tonto" en la parte de política: la
//! *decisión* se toma en [`super::config`], [`super::input`] y
//! [`InputDisposition`], funciones puras que se testean sin servidor de
//! display. Acá sólo se habla con el servidor.

use super::{plan_hints, WrapperConfig, WrapperHintPlan, WrapperWindowType};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;

/// Bits de `WM_HINTS.flags` que se ponen para pedir foco de teclado.
const WM_HINT_INPUT: u32 = 1 << 10;
/// `WM_HINTS.flags`: el estado inicial es válido, hay que mandarlo.
const WM_HINT_STATE: u32 = 1 << 11;
/// `WM_HINTS.initial_state` = `NormalState`.
const WM_INITIAL_STATE_NORMAL: u32 = 1;

/// `WM_HINTS` son 18 `CARD32`. Van a mano porque los bindings de `xproto` de
/// `x11rb` no exponen la estructura (vive en el protocolo ICCCM, no en el core
/// del servidor). La disposición es la de la especificación:
///
/// ```text
/// flags, input, initial_state, icon_pixmap, icon_window,
/// icon_x, icon_y, min_width, min_height, max_width, max_height,
/// width_inc, height_inc, min_aspect_x, min_aspect_y,
/// base_size_x, base_size_y, win_gravity
/// ```
const WM_HINTS_LEN: usize = 18;

/// Interna un átomo y lo devuelve.
///
/// `None` si el servidor no lo conoce, en vez de panic: un WM al que le
/// falte `_NET_WM_STATE_SKIP_PAGER` no puede impedir que la ventana abra,
/// sólo que abra menos integrada.
fn intern(conn: &impl Connection, name: &[u8]) -> Option<u32> {
    conn.intern_atom(false, name).ok()?.reply().ok().map(|r| r.atom)
}

/// Escribe una propiedad tipo `ATOM` (formato 32) desde una lista de
/// nombres de átomo.
fn set_atom_list(
    conn: &impl Connection,
    window: Window,
    property: &[u8],
    names: &[&[u8]],
) -> Result<(), String> {
    if names.is_empty() {
        return Ok(());
    }
    let label = String::from_utf8_lossy(property).into_owned();
    let property_atom = intern(conn, property)
        .ok_or_else(|| format!("el servidor no internó {label}"))?;

    let mut values = Vec::with_capacity(names.len());
    for name in names {
        values.push(
            intern(conn, name).ok_or_else(|| {
                format!("el servidor no internó {}", String::from_utf8_lossy(name))
            })?,
        );
    }

    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_ne_bytes()).collect();
    conn.change_property(
        PropMode::REPLACE,
        window,
        property_atom,
        u32::from(AtomEnum::ATOM),
        32,
        values.len() as u32,
        &bytes,
    )
    .map_err(|e| format!("change_property({label}): {e}"))?
    .check()
    .map_err(|e| format!("change_property({label}): {e}"))
}

/// Instala `WM_HINTS` con `input = True` y estado inicial normal.
///
/// # Por qué importa
///
/// Sin este hint el gestor de ventanas trata la ventana como una tool-window
/// decorativa: muchos WMs (KWin entre ellos) asumen que una utility window no
/// quiere teclado y **no le asignan el foco** aunque el usuario la clickee. En
/// un editor de plugin eso se ve como "hice click y no pasó nada": el control
/// de abajo nunca entra en modo edición, y los desplegables abren y no eligen.
///
/// Hay que ponerlo **antes** del `map`, como el resto de los hints: el WM lee
/// `WM_HINTS` una vez, al mapear.
pub fn set_wm_hints_input(conn: &impl Connection, window: Window) -> Result<(), String> {
    let atom = intern(conn, b"WM_HINTS").ok_or("el servidor no internó WM_HINTS")?;

    let mut fields = [0u32; WM_HINTS_LEN];
    fields[0] = WM_HINT_INPUT | WM_HINT_STATE;
    fields[1] = 1; // input = True
    fields[2] = WM_INITIAL_STATE_NORMAL;
    let bytes: Vec<u8> = fields.iter().flat_map(|v| v.to_ne_bytes()).collect();

    conn.change_property(
        PropMode::REPLACE,
        window,
        atom,
        u32::from(AtomEnum::WM_HINTS),
        32,
        WM_HINTS_LEN as u32,
        &bytes,
    )
    .map_err(|e| format!("change_property(WM_HINTS): {e}"))?
    .check()
    .map_err(|e| format!("change_property(WM_HINTS): {e}"))
}

/// Aplica el plan de hints a una ventana **sin mapear**.
///
/// Hay que llamarla antes de `map_window`: es el momento en que el WM lee
/// `_NET_WM_WINDOW_TYPE` y `_NET_WM_STATE` para decidir cómo tratar la
/// ventana durante el resto de su vida. Una vez mapeada, el estado se
/// cambia con un `ClientMessage` a la raíz y no con `change_property`.
///
/// Sólo el `WM_TRANSIENT_FOR` es duro: si falla, se devuelve `Err` porque
/// el resto de la envoltura puede sostener la ventana sin él, pero el
/// vínculo con el padre es justo lo que se pidió. Los hints de estado y de
/// tipo son best-effort.
pub fn apply_wrapper_hints(
    conn: &impl Connection,
    window: Window,
    config: &WrapperConfig,
) -> Result<WrapperHintPlan, String> {
    let plan = plan_hints(config);

    // 1. `WM_TRANSIENT_FOR`: atar al DAW. Es el hint que hace que el WM
    //    la stacke con su padre, la minimice con él y le devuelva el foco
    //    al cerrarlo. Sólo existe si hay un XID de padre (sesión X11).
    if let Some(parent) = plan.transient_for {
        let atom = intern(conn, b"WM_TRANSIENT_FOR")
            .ok_or("el servidor no internó WM_TRANSIENT_FOR")?;
        let data = parent.to_ne_bytes();
        conn.change_property(
            PropMode::REPLACE,
            window,
            atom,
            u32::from(AtomEnum::WINDOW),
            32,
            1,
            &data,
        )
        .map_err(|e| format!("change_property(WM_TRANSIENT_FOR): {e}"))?
        .check()
        .map_err(|e| format!("change_property(WM_TRANSIENT_FOR): {e}"))?;
    }

    // 2. `_NET_WM_WINDOW_TYPE`: esto es una tool-window del DAW, no otra
    //    aplicación. Best-effort.
    let type_name: &[u8] = match plan.window_type {
        WrapperWindowType::Utility => b"_NET_WM_WINDOW_TYPE_UTILITY",
        WrapperWindowType::Dialog => b"_NET_WM_WINDOW_TYPE_DIALOG",
    };
    let _ = set_atom_list(conn, window, b"_NET_WM_WINDOW_TYPE", &[type_name]);

    // 3. `_NET_WM_STATE`: por encima + fuera del taskbar. Uno por uno y
    //    best-effort, para que un WM al que le falte `SKIP_PAGER` no
    //    impida aplicar `ABOVE`.
    for name in &plan.states {
        let _ = set_atom_list(conn, window, b"_NET_WM_STATE", &[name.as_bytes()]);
    }

    // 4. `WM_HINTS.input`: la ventana *quiere* teclado. Sin esto hay WMs que
    //    nunca le dan el foco y el editor deja de responder al teclado y a
    //    los clics que se resuelven por foco (desplegables, edición inline).
    //    Best-effort: es una mejora, no un requisito para que la ventana abra.
    if plan.keyboard_input {
        let _ = set_wm_hints_input(conn, window);
    }

    let _ = conn.flush();
    Ok(plan)
}

/// Deja la ventana sin decoraciones aunque el WM no respete
/// `override_redirect`.
///
/// `_MOTIF_WM_HINTS` es el estándar que siguen GNOME, KDE y todos los Motif
/// (que hoy son casi todos). Pone los tres flags de decoraciones a cero y el
/// de tamaño a 0: "sin titlebar, sin bordes, sin botones, sin cambiar de
/// tamaño".
///
/// Es best-effort por diseño: `override_redirect` ya deja la ventana fuera de
/// la gestión del WM y por lo tanto sin decoraciones, así que este hint
/// existe sólo para los WMs que la ignoran o para el modo
/// [`WrapperStyle::Decorated`] con una ventana que se quiere sin adornos pero
/// sí gestionada.
pub fn strip_decorations(conn: &impl Connection, window: Window) -> Result<(), String> {
    let atom = intern(conn, b"_MOTIF_WM_HINTS").ok_or("el servidor no internó _MOTIF_WM_HINTS")?;

    // `struct MotifWmHints` (ICCCM 4): 5 CARD32.
    // Los tres primeros bits de `flags` son MWM_HINTS_DECORATIONS (1),
    // MWM_HINTS_FUNCTIONS (2) y MWM_HINTS_MAXIMIZE (4).
    let mut hints = [0u32; 5];
    hints[0] = 1 | 2 | 4; // decorations | functions | maximize
    let bytes: Vec<u8> = hints.iter().flat_map(|v| v.to_ne_bytes()).collect();

    conn.change_property(
        PropMode::REPLACE,
        window,
        atom,
        atom,
        32,
        hints.len() as u32,
        &bytes,
    )
    .map_err(|e| format!("change_property(_MOTIF_WM_HINTS): {e}"))?
    .check()
    .map_err(|e| format!("change_property(_MOTIF_WM_HINTS): {e}"))
}

/// Mueve y redimensiona la ventana con coordenadas de **raíz**.
pub fn move_resize(
    conn: &impl Connection,
    window: Window,
    x: i16,
    y: i16,
    width: u16,
    height: u16,
) -> Result<(), String> {
    conn.configure_window(
        window,
        &ConfigureWindowAux::new()
            .x(Some(i32::from(x)))
            .y(Some(i32::from(y)))
            .width(Some(u32::from(width)))
            .height(Some(u32::from(height))),
    )
    .map_err(|e| format!("configure_window(moveresize): {e}"))?
    .check()
    .map_err(|e| format!("configure_window(moveresize): {e}"))
}

/// La geometría de una ventana en coordenadas de **raíz**.
///
/// `GetGeometry` devuelve la posición relativa al padre, que para una ventana
/// de primer nivel es un número sin sentido. `TranslateCoordinates` es lo que
/// la pone en un sistema que se puede usar para colocar la ventana del editor.
pub fn root_frame(
    conn: &impl Connection,
    window: Window,
) -> Result<(i16, i16, u16, u16), String> {
    let geom = conn
        .get_geometry(window)
        .map_err(|e| format!("get_geometry: {e}"))?
        .reply()
        .map_err(|e| format!("get_geometry: {e}"))?;

    let translated = conn
        .translate_coordinates(window, geom.root, 0, 0)
        .map_err(|e| format!("translate_coordinates: {e}"))?
        .reply()
        .map_err(|e| format!("translate_coordinates: {e}"))?;

    Ok((
        translated.dst_x,
        translated.dst_y,
        geom.width,
        geom.height,
    ))
}

/// Si una ventana está visible (`IsViewable`).
///
/// Es lo que usa el envoltorio para decidir si el DAW sigue en pantalla: una
/// ventana del DAW minimizada o tapada por un diálogo a pantalla completa tiene
/// que esconder el editor, o el plugin queda flotando solo sobre un DAW que el
/// usuario no está viendo.
pub fn is_viewable_root(
    conn: &impl Connection,
    window: Window,
) -> Result<bool, String> {
    let reply = conn
        .get_window_attributes(window)
        .map_err(|e| format!("get_window_attributes: {e}"))?
        .reply()
        .map_err(|e| format!("get_window_attributes: {e}"))?;
    Ok(reply.map_state == MapState::VIEWABLE)
}

/// Un contexto gráfico y una fuente, para pintar el marco.
///
/// Se crean una sola vez por ventana y se reutilizan: cada `CreateGc` es un
/// viaje al servidor y el marco se repinta en cada `Expose`, que puede pasar
/// decenas de veces por segundo mientras se arrastra la ventana.
#[derive(Clone)]
pub struct Paint {
    gc: u32,
    font: Option<u32>,
}

impl Paint {
    /// Prepara el contexto sobre la ventana.
    ///
    /// `depth` es el de la ventana: pedirlos con un depth distinto a un
    /// contexto es un `BadMatch` y una르는 de abortar el proceso si el error
    /// llega al manejador por defecto de Xlib.
    pub fn new(conn: &impl Connection, window: Window) -> Result<Self, String> {
        // `CreateGC` pide el `drawable` aparte del `depth`: no es redundante,
        // el servidor lo usa para validar que la profundidad del contexto
        // coincida con la del drawable (un `BadMatch` si no).
        // `CreateGC` pide el `drawable` aparte: no es redundante, el servidor
        // lo usa para validar que el contexto y el destino tengan la misma
        // profundidad (un `BadMatch` si no).
        let drawable: Drawable = window.into();
        let id = conn
            .generate_id()
            .map_err(|e| format!("generate_id(gc): {e}"))?;
        conn.create_gc(id, drawable, &CreateGCAux::new().graphics_exposures(0))
            .map_err(|e| format!("create_gc: {e}"))?
            .check()
            .map_err(|e| format!("create_gc: {e}"))?;
        let gc = id;

        // La fuente de X11 "fixed" viene del servidor. En
        // Xorg y XWayland sigue estando, pero no en todas las instalaciones;
        // si no está, el marco se dibuja sin texto en vez de no dibujar.
        let font = ["fixed", "9x15", "6x13"]
            .into_iter()
            .find_map(|name| load_font(conn, name));

        Ok(Self { gc, font })
    }

    /// Si hay fuente cargada.
    ///
    /// El wrapper lo consulta para avisarle a la UI cuando el marco se dibuja
    /// sin título, que es la señal de que a este servidor le faltan fuentes.
    pub fn has_font(&self) -> bool {
        self.font.is_some()
    }

    /// Rellena un rectángulo con un color.
    pub fn fill_rect(
        &self,
        conn: &impl Connection,
        drawable: Drawable,
        color: Color,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
    ) -> Result<(), String> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        self.set_foreground(conn, color)?;
        conn.poly_fill_rectangle(drawable, self.gc, &[Rectangle { x, y, width, height }])
            .map_err(|e| format!("poly_fill_rectangle: {e}"))?
            .check()
            .map_err(|e| format!("poly_fill_rectangle: {e}"))
    }

    /// Dibuja el marco de un rectángulo de 1 px.
    pub fn stroke_rect(
        &self,
        conn: &impl Connection,
        drawable: Drawable,
        color: Color,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
    ) -> Result<(), String> {
        if width < 2 || height < 2 {
            return Ok(());
        }
        self.set_foreground(conn, color)?;
        conn.poly_rectangle(drawable, self.gc, &[Rectangle { x, y, width, height }])
            .map_err(|e| format!("poly_rectangle: {e}"))?
            .check()
            .map_err(|e| format!("poly_rectangle: {e}"))
    }

    /// Dibuja una línea entre dos puntos.
    pub fn draw_line(
        &self,
        conn: &impl Connection,
        drawable: Drawable,
        color: Color,
        x1: i16,
        y1: i16,
        x2: i16,
        y2: i16,
    ) -> Result<(), String> {
        self.set_foreground(conn, color)?;
        // `PolySegment` toma segmentos (pares de puntos), no puntos sueltos:
        // un `Point` suelto ahí es un segmento de longitud cero.
        let segments = [
            Segment { x1, y1, x2, y2 },
        ];
        conn.poly_segment(drawable, self.gc, &segments)
            .map_err(|e| format!("poly_segment: {e}"))?
            .check()
            .map_err(|e| format!("poly_segment: {e}"))
    }

    /// Escribe un texto.
    ///
    /// No-op si no hay fuente: el marco se dibuja igual, sólo sin el título.
    pub fn draw_text(
        &self,
        conn: &impl Connection,
        drawable: Drawable,
        color: Color,
        x: i16,
        y: i16,
        text: &str,
    ) -> Result<(), String> {
        let Some(font) = self.font else {
            return Ok(());
        };
        // X11 `ImageText8` es Latin-1, no UTF-8: mandar bytes UTF-8 produce
        // basura, y convertir cada `char` a `u8` a la fuerza produce `?`.
        //
        // Se filtran en vez de sustituir: un separador decorativo como la
        // raya de "UZU — Hikaru" salía como "UZU ? Hikaru". El título va en
        // Latin-1 o no va.
        let bytes: Vec<u8> = text
            .chars()
            .filter(|c| (*c as u32) < 0x100)
            .map(|c| c as u8)
            .collect();
        if bytes.is_empty() {
            return Ok(());
        }
        conn.change_gc(
            self.gc,
            &ChangeGCAux::new().font(font).foreground(color.pixel()),
        )
        .map_err(|e| format!("change_gc(font): {e}"))?
        .check()
        .map_err(|e| format!("change_gc(font): {e}"))?;

        conn.image_text8(
            drawable,
            self.gc,
            x,
            y,
            &bytes,
        )
        .map_err(|e| format!("image_text8: {e}"))?
        .check()
        .map_err(|e| format!("image_text8: {e}"))
    }

    /// Cambia el color de fondo del contexto.
    pub fn set_background(
        &self,
        conn: &impl Connection,
        color: Color,
    ) -> Result<(), String> {
        conn.change_gc(self.gc, &ChangeGCAux::new().background(color.pixel()))
            .map_err(|e| format!("change_gc(background): {e}"))?
            .check()
            .map_err(|e| format!("change_gc(background): {e}"))
    }

    fn set_foreground(&self, conn: &impl Connection, color: Color) -> Result<(), String> {
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(color.pixel()))
            .map_err(|e| format!("change_gc: {e}"))?
            .check()
            .map_err(|e| format!("change_gc: {e}"))
    }
}

/// Un color del marco.
///
/// Se declara acá y no como `u32` suelto para que los tests puedan compararlos
/// y para que un cambio de paleta no se confunda con un número mágico.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// Fondo del titlebar.
    Titlebar,
    /// Fondo del título cuando la ventana tiene el foco.
    TitlebarFocused,
    /// Texto del título.
    Text,
    /// Bordes del marco.
    Border,
    /// Tirador de redimensionado.
    Grip,
    /// Fondo del botón de cerrar.
    CloseHover,
}

impl Color {
    /// El valor RGB del color.
    pub fn rgb(self) -> u32 {
        match self {
            // El titlebar usa la misma paleta oscura del DAW: un azul
            // saturado sobre un fondo casi negro canta demasiado y delata que
            // la ventana la dibujo otro programa.
            Self::Titlebar => 0x2A2D37,
            Self::TitlebarFocused => 0x3A3F4D,
            Self::Text => 0xE8EAF0,
            Self::Border => 0x4A5060,
            Self::Grip => 0x6A7080,
            Self::CloseHover => 0xB4644C,
        }
    }

    /// El píxel de 24 bits que espera X11.
    pub fn pixel(self) -> u32 {
        let rgb = self.rgb();
        // X11 en TrueColor de 24/32 bits espera 0x00RRGGBB en el campo de la
        // máscara de píxel, que es lo que los toolkits X más comunes usan.
        rgb & 0x00FF_FFFF
    }
}

/// Carga una fuente del servidor, o `None` si no existe.
///
/// Un `NameError` o un `BadName` aquí son **normales**: significa que a este
/// servidor no le interesa la tipografía, no que algo esté roto.
fn load_font(conn: &impl Connection, name: &str) -> Option<Font> {
    let id = conn.generate_id().ok()?;
    // `NameError` y `BadName` son respuestas normales: significan que a este
    // servidor no le interesa la tipografía, no que algo esté roto.
    conn.open_font(id, name.as_bytes()).ok()?.check().ok()?;
    Some(id)
}

/// El nombre del cursor de X11 para una zona del marco.
///
/// X11 no tiene un enum de cursores: `_NET_WM_CURSOR` lleva el **nombre** del
/// puntero como CARD16, y el backend de la ventana busca el bitmap
/// correspondiente. `None` significa "el que corresponde por defecto".
pub fn cursor_name(hint: crate::hikaru_wrapper::frame::CursorHint) -> Option<&'static [u8]> {
    use crate::hikaru_wrapper::frame::{CursorHint, ResizeDir};
    Some(match hint {
        CursorHint::Default => b"left_ptr",
        CursorHint::Hand => b"hand2",
        CursorHint::Resize(ResizeDir::Top) => b"top_side",
        CursorHint::Resize(ResizeDir::Bottom) => b"bottom_side",
        CursorHint::Resize(ResizeDir::Left) => b"left_side",
        CursorHint::Resize(ResizeDir::Right) => b"right_side",
        CursorHint::Resize(ResizeDir::TopLeft) => b"top_left_corner",
        CursorHint::Resize(ResizeDir::TopRight) => b"top_right_corner",
        CursorHint::Resize(ResizeDir::BottomLeft) => b"bottom_left_corner",
        CursorHint::Resize(ResizeDir::BottomRight) => b"bottom_right_corner",
    })
}

/// Pone el cursor del marco.
///
/// Si el backend no encuentra el bitmap del nombre, deja el cursor del
/// servidor: es cosmético y no vale la penapenalizar el gesto con un error.
pub fn set_cursor(
    conn: &impl Connection,
    window: Window,
    hint: crate::hikaru_wrapper::frame::CursorHint,
) {
    let Some(name) = cursor_name(hint) else {
        return;
    };
    // Un cursor de X11 es un glifo de una fuente. El nombre (`left_ptr`,
    // `hand2`, `top_side`...) es el **nombre de la fuente**: hay que abrirla
    // con `OpenFont` y después sacar el glifo. Pasar font 0 funcionaría pero
    // dibujaría siempre el cursor por defecto, que es el caso común y
    // justamente el que no arregla nada.
    let Some(font) = load_font(conn, std::str::from_utf8(name).unwrap_or("left_ptr")) else {
        return;
    };
    let Some(id) = conn.generate_id().ok() else {
        return;
    };
    let fg = Color::Text.rgb();
    let bg = Color::Border.rgb();
    let created = conn
        .create_glyph_cursor(
            id,
            font,
            font,
            0,
            0,
            (fg >> 16) as u16,
            (fg >> 8) as u16,
            fg as u16,
            (bg >> 16) as u16,
            (bg >> 8) as u16,
            bg as u16,
        )
        .ok()
        .map(|cookie| cookie.check().is_ok())
        .unwrap_or(false);
    if !created {
        return;
    }
    let cursor = id;
    let Some(atom) = intern(conn, b"_NET_WM_CURSOR") else {
        let _ = conn.free_cursor(cursor);
        return;
    };
    let _ = conn.change_property(
        PropMode::REPLACE,
        window,
        atom,
        u32::from(AtomEnum::CURSOR),
        16,
        1,
        &cursor.to_ne_bytes(),
    )
    .map(|cookie| cookie.check());
    let _ = conn.flush();
}

/// Re-ordena la ventana por encima de las demás **sin enfocarla**.
///
/// Funciona también con `override_redirect`: X11 permite reordenar en el
/// stacking cualquier ventana, gestionada o no. Es lo que hace que una
/// ventana de Hikaru siga arriba sin necesitar el WM.
///
/// `stack-mode = Above` es la forma de subir en X11 sin robar el foco, y
/// es justo lo que hace falta cuando el DAW recupera el foco: el usuario
/// acaba de clickear un knob del DAW y no puede perder el foco a mitad del
/// gesto por una ventana que él no está tocando.
pub fn raise(conn: &impl Connection, window: Window) -> Result<(), String> {
    conn.configure_window(window, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE))
        .map_err(|e| format!("configure_window(stack=Above): {e}"))?
        .check()
        .map_err(|e| format!("configure_window(stack=Above): {e}"))
}

/// Pide al WM que active la ventana (`_NET_ACTIVE_WINDOW`).
///
/// **Sólo al abrir**, nunca en el keep-alive. Es la diferencia entre
/// "Fruity Wrapper" y "el editor me roba el foco cada 750 ms": al abrir, el
/// usuario viene de clickear el botón `Editor` del DAW y espera que el foco
/// vaya al editor; un rato después ya está trabajando en otra cosa y no puede
/// perder el foco de forma periódica.
///
/// Idempotente y sin efectos si el WM no implementa EWMH.
pub fn activate(conn: &impl Connection, root: Window, window: Window) -> Result<(), String> {
    let atom = intern(conn, b"_NET_ACTIVE_WINDOW").ok_or("el servidor no internó _NET_ACTIVE_WINDOW")?;

    // `ClientMessage` con `format = 32`: data[0] = source indication
    // (2 = pager, la convención de EWMH), data[1] = timestamp, data[2] = la
    // ventana. `CurrentTime` (0) le dice al WM "usá la hora actual".
    let mut data = [0u8; 20];
    data[0..4].copy_from_slice(&2u32.to_ne_bytes());
    data[4..8].copy_from_slice(&0u32.to_ne_bytes());
    data[8..12].copy_from_slice(&window.to_ne_bytes());

    conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
        ClientMessageEvent {
            response_type: CLIENT_MESSAGE_EVENT,
            format: 32,
            sequence: 0,
            window,
            type_: atom,
            data: ClientMessageData::from(data),
        },
    )
    .map_err(|e| format!("send_event(_NET_ACTIVE_WINDOW): {e}"))?
    .check()
    .map_err(|e| format!("send_event(_NET_ACTIVE_WINDOW): {e}"))?;

    let _ = conn.flush();
    Ok(())
}

/// Devuelve la ventana que tiene el foco de teclado, o `None` si ninguna.
pub fn focused_window(conn: &impl Connection) -> Result<Option<Window>, String> {
    conn.get_input_focus()
        .map_err(|e| format!("get_input_focus: {e}"))?
        .reply()
        .map(|r| Some(r.focus))
        .map_err(|e| format!("get_input_focus: {e}"))
}

/// Le da el foco de teclado a `window` sin activar el WM.
///
/// `revert_to = Parent` deja el foco como estaba antes de que el usuario
/// clickeara el DAW, que es lo que quiere un envoltorio: el editor toma el
/// foco mientras el usuario trabaja en él y lo devuelve al DAW cuando sale.
///
/// Es el complemento de `_NET_ACTIVE_WINDOW` para los casos en que el WM se
/// niega a dar el foco (típicamente `WM_HINTS.input` ausente o un WM con
/// `focusFollowsMouse` agresivo). Es idempotente: si la ventana ya tenía el
/// foco, `SetInputFocus` con `CurrentTime` es un no-op.
pub fn focus(conn: &impl Connection, window: Window) -> Result<(), String> {
    conn.set_input_focus(
        InputFocus::PARENT,
        window,
        x11rb::CURRENT_TIME,
    )
    .map_err(|e| format!("set_input_focus: {e}"))?
    .check()
    .map_err(|e| format!("set_input_focus: {e}"))?;
    let _ = conn.flush();
    Ok(())
}

/// Rompe el vínculo padre-hijo antes de destruir la ventana.
///
/// Sin esto, el `WM_TRANSIENT_FOR` queda apuntando al XID del DAW y el WM
/// conserva una entrada para una ventana que ya no existe. Es la fuente
/// clásica de fugas de handles bajo XWayland: no rompe nada visible, pero
/// cada abrir/cerrar suma una entrada, y si el DAW se cerrara antes el WM
/// puede intentar descargar una ventana ya destruida.
///
/// Se llama justo antes de `destroy_window` y suprime errores: si el
/// servidor ya no conoce la ventana no hay nada que soltar, y el
/// `destroy_window` que sigue se encarga igual.
pub fn detach(conn: &impl Connection, window: Window) {
    // `0` = None: la forma canónica de quitar un transient parent.
    if let Some(atom) = intern(conn, b"WM_TRANSIENT_FOR") {
        let zero = 0u32.to_ne_bytes();
        if let Ok(cookie) = conn.change_property(
            PropMode::REPLACE,
            window,
            atom,
            u32::from(AtomEnum::WINDOW),
            32,
            1,
            &zero,
        ) {
            let _ = cookie.check();
        }
    }
    // Vaciar `_NET_WM_STATE`: sin `ABOVE` ni `SKIP_TASKBAR` colgando, el
    // WM no intenta re-aplicarlos sobre el frame durante el teardown.
    if let Some(atom) = intern(conn, b"_NET_WM_STATE") {
        if let Ok(cookie) = conn.change_property(
            PropMode::REPLACE,
            window,
            atom,
            u32::from(AtomEnum::ATOM),
            32,
            0,
            &[],
        ) {
            let _ = cookie.check();
        }
    }
    let _ = conn.flush();
}

/// Si la ventana sigue mapiada y visible.
///
/// El keep-alive lo consulta antes de re-aplicar el stack: `ConfigureWindow`
/// sobre una ventana que el WM ya destruyó devuelve `BadWindow`, y el
/// chequeo evita ensuciar el log con errores de una ventana que ya no
/// existe.
pub fn is_viewable(conn: &impl Connection, window: Window) -> Result<bool, String> {
    let reply = conn
        .get_window_attributes(window)
        .map_err(|e| format!("get_window_attributes: {e}"))?
        .reply()
        .map_err(|e| format!("get_window_attributes: {e}"))?;
    Ok(reply.map_state == MapState::VIEWABLE)
}

// ───────────────────────── Vista hija del plugin ─────────────────────────

/// Geometría de la ventana hija en la que el plugin dibuja su editor.
///
/// Coordenadas relativas a la ventana contenedora, que es lo que devuelve
/// `GetGeometry` para una ventana hija directa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewGeometry {
    pub window: Window,
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

impl ViewGeometry {
    /// La geometría del contenedor.
    pub fn new(window: Window, x: i16, y: i16, width: u16, height: u16) -> Self {
        Self {
            window,
            x,
            y,
            width,
            height,
        }
    }

    /// Traduce un punto del contenedor a coordenadas de la vista.
    ///
    /// Devuelve `None` si el punto cae fuera de la vista: reenviarle un
    /// click a una ventana que no lo contiene es peor que dejarlo morir,
    /// porque el plugin lo interpretaría como un click en el borde.
    pub fn to_view_coords(&self, container_x: i16, container_y: i16) -> Option<(i16, i16)> {
        let vx = container_x.checked_sub(self.x)?;
        let vy = container_y.checked_sub(self.y)?;
        if vx < 0 || vy < 0 || vx >= self.width as i16 || vy >= self.height as i16 {
            return None;
        }
        Some((vx, vy))
    }
}

/// Busca la ventana hija en la que el plugin dibujó su editor.
///
/// Devuelve la **más alta** en el apilado: si el plugin crea varias (un
/// canvas, una capa de overlay, un popup), el control que el usuario quiere
/// pulsar es la de arriba.
///
/// `None` si el plugin no creó ninguna ventana hija. Eso no es un error: un
/// editor puede dibujar directamente sobre la ventana que le pasó el host,
/// y en ese caso no hay nada que redirigir.
pub fn find_view_window(
    conn: &impl Connection,
    container: Window,
) -> Result<Option<ViewGeometry>, String> {
    let tree = conn
        .query_tree(container)
        .map_err(|e| format!("query_tree: {e}"))?
        .reply()
        .map_err(|e| format!("query_tree: {e}"))?;

    // `QueryTree` devuelve los hijos de abajo hacia arriba: el último es el
    // de más arriba, que es el que está encima de todos los demás.
    for child in tree.children.iter().rev() {
        let Ok(cookie) = conn.get_geometry(*child) else {
            continue;
        };
        let Ok(geom) = cookie.reply() else {
            continue;
        };
        if geom.width == 0 || geom.height == 0 {
            continue;
        }
        // `GetGeometryReply` no trae la clase de la ventana, asi que hace
        // falta una segunda consulta. No es cara: esto corre una vez al
        // embeberse el editor, no por tick. Una `InputOnly` es un overlay de
        // captura de puntero; sin este filtro se robaría los clicks de todo
        // lo que tiene debajo.
        let Ok(attrs_cookie) = conn.get_window_attributes(*child) else {
            continue;
        };
        let Ok(attrs) = attrs_cookie.reply() else {
            continue;
        };
        if attrs.class != WindowClass::INPUT_OUTPUT {
            continue;
        }
        return Ok(Some(ViewGeometry::new(
            *child,
            geom.x,
            geom.y,
            geom.width,
            geom.height,
        )));
    }
    Ok(None)
}

/// Qué hay que hacer con un evento de entrada que llegó al contenedor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputDisposition {
    /// El evento **nació** en la vista del plugin y sólo nos llegó de rebote.
    ///
    /// En X11 un evento sube por la cadena de ancestros hasta la primera
    /// ventana que lo seleccionó. Si la vista seleccionó `MotionNotify` pero
    /// no `ButtonPress`, un click cae en la vista (que lo procesa) y un
    /// movimiento cae en el contenedor. En ese caso las coordenadas ya
    /// vienen en el espacio de la vista y **reenviarlas las volvería a
    /// desplazar**: el puntero aparecería corrido por exactamente el offset de
    /// la vista dentro del contenedor. Es el error clásico de un reenvío de
    /// entrada mal hecho.
    Native,
    /// Reenviar a la vista hija en estas coordenadas **relativas a ella**.
    Forward { x: i16, y: i16 },
    /// Dejarlo morir: no hay vista, o el punto cae fuera de ella.
    Drop,
}

/// Decide qué hacer con un evento de puntero que llegó a la ventana
/// contenedora.
///
/// `event_window` es la ventana en la que el servidor dice que ocurrió el
/// evento — no necesariamente la que lo seleccionó, porque un evento que
/// nadie seleccionó sube por los ancestros. `container_x`/`container_y` son
/// las coordenadas del evento, que siempre son relativas a `event_window`.
///
/// Función pura a propósito: es la política de "el contenedor no se queda con
/// los clicks del plugin" y se testea sin servidor de display.
pub fn plan_input_disposition(
    event_window: Window,
    container_x: i16,
    container_y: i16,
    container: Window,
    view: Option<&ViewGeometry>,
) -> InputDisposition {
    let Some(view) = view else {
        return InputDisposition::Drop;
    };
    // Ya venía de la vista: sus coordenadas están en el espacio de la vista.
    if event_window == view.window {
        return InputDisposition::Native;
    }
    if event_window != container {
        // Es de otra ventana (por ejemplo una hija distinta). Sin una
        // traducción de coordenadas fiable no se puede reenviar sin
        // mandarlo al lugar equivocado, y mandarlo al lugar equivocado en
        // un editor de plugin es peor que dejarlo morir.
        return InputDisposition::Drop;
    }
    match view.to_view_coords(container_x, container_y) {
        Some((x, y)) => InputDisposition::Forward { x, y },
        None => InputDisposition::Drop,
    }
}

/// Reenvía un evento de puntero del contenedor a la vista del plugin.
///
/// `event_mask` es **un solo bit**: el del evento que se reenvía. Mandar la
/// máscara completa haría que la vista recibiera `MotionNotify` por cada
/// `ButtonPress` y que los gestos del plugin interpretaran la entrada como
/// movimiento de ratón continuo.
///
/// `propagate = false` porque ya se decidió el destino; si no, un evento que
/// la vista no seleccione volvería a subir a otro cliente y se entregaría
/// dos veces.
pub fn forward_pointer_event(
    conn: &impl Connection,
    view: &ViewGeometry,
    event_mask: EventMask,
    event: [u8; 32],
) -> Result<(), String> {
    conn.send_event(false, view.window, event_mask, event)
        .map_err(|e| format!("send_event({:?}): {e}", event_mask))?
        .check()
        .map_err(|e| format!("send_event({:?}): {e}", event_mask))
}

/// Reescribe un evento de puntero para mandarlo a la vista hija.
///
/// Cambia la ventana destino y las coordenadas, y deja el resto (tiempo,
/// botón, modificadores, `same_screen`) intacto: son exactamente los datos que
/// el toolkit del plugin necesita para interpretar el gesto.
///
/// `event` es el evento crudo tal cual lo entregó el servidor, con las
/// coordenadas **relativas al contenedor**. `x`/`y` ya vienen traducidos al
/// espacio de la vista (ver [`InputDisposition::Forward`]).
pub fn rewrite_pointer_event(
    view: &ViewGeometry,
    x: i16,
    y: i16,
    mut event: [u8; 32],
) -> [u8; 32] {
    // Offset 12..16 = `event` (la ventana en la que ocurrió).
    // Offset 24..26 = `event_x`, 26..28 = `event_y`.
    event[12..16].copy_from_slice(&view.window.to_ne_bytes());
    event[24..26].copy_from_slice(&x.to_ne_bytes());
    event[26..28].copy_from_slice(&y.to_ne_bytes());
    event
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wm_hints_layout_is_18_card32() {
        // Si la longitud cambia, el `change_property` manda una propiedad
        // que ningún WM puede leer y el hint se pierde en silencio.
        assert_eq!(WM_HINTS_LEN, 18);
        assert_eq!(WM_HINT_INPUT | WM_HINT_STATE, 0xC00);
    }

    #[test]
    fn view_coords_translate_and_clip() {
        let view = ViewGeometry::new(0xAA, 10, 20, 100, 50);

        // Dentro: se traduce restando el origen de la vista.
        assert_eq!(view.to_view_coords(15, 25), Some((5, 5)));
        assert_eq!(view.to_view_coords(10, 20), Some((0, 0)));

        // Fuera por el borde superior/izquierdo: `checked_sub` evita el
        // wrap-around de `i16` que convertiría el click en el borde opuesto.
        assert_eq!(view.to_view_coords(5, 25), None);
        assert_eq!(view.to_view_coords(15, 15), None);

        // Fuera por el inferior/derecho (límites excluyentes).
        assert_eq!(view.to_view_coords(110, 25), None);
        assert_eq!(view.to_view_coords(15, 70), None);
        assert_eq!(view.to_view_coords(109, 69), Some((99, 49)));
    }

    #[test]
    fn sin_una_vista_no_hay_nada_que_reenviar() {
        assert_eq!(
            plan_input_disposition(0x11, 5, 5, 0x11, None),
            InputDisposition::Drop
        );
    }

    #[test]
    fn clicks_dentro_de_la_vista_se_reenvian_a_ella() {
        // Éste es el bug: sin esto el contenedor se queda con los clicks
        // del editor y los knobs no responden.
        let view = ViewGeometry::new(0xAA, 0, 0, 400, 300);
        assert_eq!(
            plan_input_disposition(0x11, 120, 80, 0x11, Some(&view)),
            InputDisposition::Forward { x: 120, y: 80 }
        );
    }

    #[test]
    fn clicks_fuera_de_la_vista_se_dejan_morir() {
        let view = ViewGeometry::new(0xAA, 100, 100, 50, 50);
        assert_eq!(
            plan_input_disposition(0x11, 10, 10, 0x11, Some(&view)),
            InputDisposition::Drop
        );
        assert_eq!(
            plan_input_disposition(0x11, 120, 120, 0x11, Some(&view)),
            InputDisposition::Forward { x: 20, y: 20 }
        );
    }

    #[test]
    fn un_evento_que_nacio_en_la_vista_no_se_reenvia() {
        // Éste es el error clásico: la vista seleccionó `MotionNotify` pero
        // no `ButtonPress`, así que los movimientos le llegan igual y suben al
        // contenedor. Sus coordenadas ya están en el espacio de la vista;
        // reenviarlas las desplazaría por segunda vez y el punstre aparecería
        // corrido por el offset de la vista.
        let view = ViewGeometry::new(0xAA, 100, 50, 200, 200);
        assert_eq!(
            plan_input_disposition(0xAA, 10, 10, 0x11, Some(&view)),
            InputDisposition::Native
        );
    }

    #[test]
    fn un_evento_de_otra_ventana_no_se_reenvia() {
        // Sin una traducción de coordenadas fiable, mandarlo a la vista lo
        // llevaría al lugar equivocado.
        let view = ViewGeometry::new(0xAA, 0, 0, 200, 200);
        assert_eq!(
            plan_input_disposition(0x99, 10, 10, 0x11, Some(&view)),
            InputDisposition::Drop
        );
    }

    #[test]
    fn rewriting_moves_the_event_to_the_child_window() {
        let view = ViewGeometry::new(0xBB, 5, 7, 100, 100);
        // Evento crudo con event_x/event_y en 15/25 (relativos al contenedor).
        let mut raw = [0u8; 32];
        raw[24..26].copy_from_slice(&15i16.to_ne_bytes());
        raw[26..28].copy_from_slice(&25i16.to_ne_bytes());

        let out = rewrite_pointer_event(&view, 10, 18, raw);
        assert_eq!(
            u32::from_ne_bytes([out[12], out[13], out[14], out[15]]),
            0xBB,
            "el evento tiene que quedar dirigido a la vista"
        );
        assert_eq!(
            i16::from_ne_bytes([out[24], out[25]]),
            10,
            "x se traduce al espacio de la vista"
        );
        assert_eq!(i16::from_ne_bytes([out[26], out[27]]), 18);
    }
}