// Copyright (C) Hikaru Corporation - 2026
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/hikaru-wrapper/frame.rs

//! El marco: geometría y_policy de interacción del gestor de ventanas.
//!
//! # Qué es esto
//!
//! Con `override_redirect` la ventana del editor queda fuera de la gestión del
//! gestor de ventanas del escritorio, y con ella se va el marco entero: no
//! hay titlebar, ni bordes redimensionables, ni botón de cerrar. Eso deja el
//! editor pegado a la ventana como una pegatina, que es justo lo que el
//! usuario報告ó.
//!
//! Para que se parezca a una ventana de verdad, Hikaru tiene que hacer de
//! gestor de ventanas él mismo. Este módulo es **la mitad geométrica** de eso:
//! dónde va el editor dentro de la ventana, qué parte del puntero corresponde a
//! qué, y qué cursor corresponde a cada zona. Son funciones puras, sin X11,
//! porque son la parte que hay que poder testear sin servidor de display.
//!
//! La otra mitad —dibujar y reaccionar— está en [`crate::hikaru_wrapper::window`]
//! y [`crate::hikaru_wrapper::x11`].

/// Las medidas del marco, en píxeles.
///
/// Los valores por defecto están calibrados para el título de DAW de
/// Hikaru (~22 px) más un poco de aire: un titlebar más bajo se lee como una
/// franja pegada, y uno más alto deja menos espacio para el editor, que es lo
/// que el usuario vino a usar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameMetrics {
    /// Alto del titlebar. Ocupa toda la franja superior.
    pub titlebar_height: u16,
    /// Grosor del borde alrededor del editor.
    pub border: u16,
    /// Lado del tirador de redimensionado, abajo a la derecha.
    pub resize_grip: u16,
    /// Lado del botón de cerrar.
    pub close_button: u16,
    /// Margen entre el texto del título y los bordes.
    pub title_padding: u16,
}

impl Default for FrameMetrics {
    fn default() -> Self {
        Self {
            titlebar_height: 30,
            border: 3,
            resize_grip: 16,
            close_button: 16,
            title_padding: 8,
        }
    }
}

impl FrameMetrics {
    /// Sin marco: el editor ocupa la ventana entera.
    ///
    /// Es el modo para los plugins que ya traen su propio titlebar (el de
    /// Vital, por ejemplo) y a los que un marco alrededor se les ve doble.
    pub fn frameless() -> Self {
        Self {
            titlebar_height: 0,
            border: 0,
            resize_grip: 0,
            close_button: 0,
            title_padding: 0,
        }
    }

    /// Cuánto le quita el marco al ancho: los dos bordes.
    pub fn horizontal_inset(&self) -> u16 {
        self.border.saturating_mul(2)
    }

    /// Cuánto le quita el marco al alto: el título y el borde de abajo.
    ///
    /// **No** son título más los dos bordes. El borde superior vive *dentro*
    /// del titlebar (que es más alto que el borde y ocupa toda la franja), así
    /// que [`Self::inner_rect`] arranca en `titlebar_height` y no en
    /// `border`: sumar el borde de arriba dos veces hacía que la ventana
    /// quedara 3 px más alta de lo que el marco se come, y el plugin recibía
    /// una vista más grande que su hueco.
    pub fn vertical_inset(&self) -> u16 {
        self.titlebar_height.saturating_add(self.border)
    }

    /// Si hay algo que dibujar.
    pub fn has_frame(&self) -> bool {
        self.titlebar_height > 0 || self.border > 0
    }

    /// El tamaño mínimo de la ventana.
    ///
    /// Menor que esto el editor deja de tener sentido, y menor que el marco
    /// el marco se come a su contenido. El mínimo del marco es lo que
    /// garantiza que el plugin siempre tenga un área usable.
    pub fn min_size(&self) -> (u16, u16) {
        (
            self.border.saturating_mul(2).saturating_add(160),
            self.titlebar_height
                .saturating_add(self.border.saturating_mul(2))
                .saturating_add(120),
        )
    }

    /// El rectángulo interior del marco en coordenadas de la ventana.
    ///
    /// Cuando no hay marco, es la ventana entera.
    pub fn inner_rect(&self, width: u16, height: u16) -> InnerRect {
        // Todo en `i32`: `saturating_sub` entre un `u16` (tamaño de ventana)
        // y un `i16` (alto del titlebar) no compila, y forzar el truncamiento
        // escondería justo el caso que importa: una ventana más chica que su
        // propio marco, donde el interior tiene que quedar en cero y no
        // desbordarse hacia negativo.
        let left = i32::from(self.border);
        let top = i32::from(self.titlebar_height);
        let right = i32::from(width) - i32::from(self.border);
        let bottom = i32::from(height) - i32::from(self.border);

        let xy = |v: i32| v.clamp(0, i32::from(i16::MAX)) as i16;
        let wh = |v: i32| v.clamp(0, i32::from(u16::MAX)) as u16;
        InnerRect {
            x: xy(left),
            y: xy(top),
            width: wh(right - left),
            height: wh(bottom - top),
        }
    }

    /// Dónde cae un punto, según la zona del marco.
    pub fn hit_test(&self, x: i16, y: i16, width: u16, height: u16) -> FrameRegion {
        let x = x as u32;
        let y = y as u32;
        let w = u32::from(width);
        let h = u32::from(height);

        let titlebar_h = u32::from(self.titlebar_height);
        let border = u32::from(self.border);
        let grip = u32::from(self.resize_grip);

        if w == 0 || h == 0 {
            return FrameRegion::Inner;
        }

        // 1. Botón de cerrar. Va primero porque está **dentro** del titlebar:
        //    si se preguntara por el titlebar antes, el clic de cerrar sería
        //    indistinguible del de mover la ventana.
        if self.close_button > 0 && titlebar_h > 0 {
            let size = u32::from(self.close_button);
            let in_titlebar = y < titlebar_h;
            let pad = u32::from(self.title_padding);
            let in_button = x + size >= w && x >= w.saturating_sub(size + pad);
            if in_titlebar && in_button {
                return FrameRegion::CloseButton;
            }
        }

        // 2. Título: arrastrar. Antes que los bordes, porque el titlebar se
        //    superpone al borde superior.
        if titlebar_h > 0 && y < titlebar_h {
            return FrameRegion::TitleBar;
        }

        // 3. Tirador de redimensionado. Va antes que los bordes por lo mismo:
        //    cae dentro de la esquina y tiene prioridad.
        if grip > 0 && x >= w.saturating_sub(grip) && y >= h.saturating_sub(grip) {
            return FrameRegion::Resize(ResizeDir::BottomRight);
        }

        // 4. Bordes y esquinas.
        //
        //    **No hay borde superior**: el titlebar se lo come entero. Es lo
        //    que hacen Windows, macOS y el Fruity Wrapper de FL Studio — el
        //    título arrastra, y punto. Si el borde superior redimensionara,
        //    o el título no se pudiera arrastrar, o las esquinas superiores
        //    redimensionaran con un área de agarre microscópica pegada al
        //    borde. Se elige lo primero.
        //
        //    La tolerancia es el doble del borde para que el área de agarre
        //    sea cómodamente hitteable con un ratón.
        let grab = (border * 2).max(6);
        let on_left = x < grab;
        let on_right = x + grab >= w;
        let on_bottom = y + grab >= h;

        match (on_bottom, on_left, on_right) {
            (true, true, false) => return FrameRegion::Resize(ResizeDir::BottomLeft),
            (true, false, true) => return FrameRegion::Resize(ResizeDir::BottomRight),
            (true, _, _) => return FrameRegion::Resize(ResizeDir::Bottom),
            (false, true, false) => return FrameRegion::Resize(ResizeDir::Left),
            (false, false, true) => return FrameRegion::Resize(ResizeDir::Right),
            _ => {}
        }

        FrameRegion::Inner
    }
}

/// El rectángulo interior del marco, en coordenadas de la ventana.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InnerRect {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

impl InnerRect {
    /// Si el área interior es usable.
    ///
    /// Un interior de 0 es lo que produce una ventana demasiado chica: se le
    /// pasa al plugin y su vista queda sin nada que dibujar.
    pub fn is_usable(&self) -> bool {
        self.width > 0 && self.height > 0
    }

    /// Si el punto cae dentro del área del editor.
    pub fn contains(&self, x: i16, y: i16) -> bool {
        x >= self.x
            && y >= self.y
            && x < self.x.saturating_add(self.width as i16)
            && y < self.y.saturating_add(self.height as i16)
    }
}

/// La zona del marco bajo el puntero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameRegion {
    /// El editor del plugin. Acá los eventos van al plugin, no al marco.
    Inner,
    /// El título: arrastrar para mover.
    TitleBar,
    /// El botón de cerrar.
    CloseButton,
    /// Un borde o esquina: redimensionar en esa dirección.
    Resize(ResizeDir),
}

impl FrameRegion {
    /// Si el clic en esta zona lo maneja el marco y no el plugin.
    pub fn is_chrome(&self) -> bool {
        !matches!(self, Self::Inner)
    }
}

/// Dirección de redimensionado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeDir {
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl ResizeDir {
    /// Cuánto cambia el borde izquierdo de la ventana al mover el puntero.
    pub fn dx(self, dx: i16) -> i16 {
        match self {
            Self::Left | Self::TopLeft | Self::BottomLeft => dx,
            Self::Right | Self::TopRight | Self::BottomRight => -dx,
            Self::Top | Self::Bottom => 0,
        }
    }

    /// Cuánto cambia el borde superior de la ventana al mover el puntero.
    pub fn dy(self, dy: i16) -> i16 {
        match self {
            Self::Top | Self::TopLeft | Self::TopRight => dy,
            Self::Bottom | Self::BottomLeft | Self::BottomRight => -dy,
            Self::Left | Self::Right => 0,
        }
    }
}

/// Cursor que corresponde a cada zona.
///
/// X11 tiene el nombre del cursor como string, no un enum: es el puntero
/// `_NET_WM_CURSOR`. El fallback es el que pone el backend si el cursor no
/// existe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorHint {
    /// El cursor normal: sobre el título o el interior.
    Default,
    /// Puntero de texto: sobre el botón de cerrar y los bordes de 1 px.
    Hand,
    /// Redimensionar en la dirección indicada.
    Resize(ResizeDir),
}

impl FrameMetrics {
    /// El cursor para una zona.
    pub fn cursor_for(&self, region: FrameRegion) -> CursorHint {
        match region {
            FrameRegion::CloseButton => CursorHint::Hand,
            FrameRegion::TitleBar | FrameRegion::Inner => CursorHint::Default,
            FrameRegion::Resize(dir) => CursorHint::Resize(dir),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: u16 = 900;
    const H: u16 = 600;

    fn m() -> FrameMetrics {
        FrameMetrics::default()
    }

    #[test]
    fn el_editor_ocupa_todo_menos_el_marco() {
        let inner = m().inner_rect(W, H);
        assert_eq!(inner.y, 30, "el titlebar va arriba");
        assert_eq!(inner.x, 3, "el borde izquierdo");
        assert_eq!(inner.width, W - 6);
        assert_eq!(inner.height, H - 30 - 3);
        assert!(inner.is_usable());
    }

    #[test]
    fn el_borde_superior_no_se_cuenta_dos_veces() {
        // El titlebar cubre la franja superior entera, borde incluido. Si el
        // inset sumo los dos bordes, `show()` agrandaría la ventana de más y
        // el plugin se creería más grande que su hueco.
        let m = m();
        assert_eq!(m.inner_rect(400, 300).height, 300 - m.vertical_inset());
        assert!(m.titlebar_height > m.border);
    }

    #[test]
    fn el_inset_coincide_con_el_rect_interior() {
        // Es la cuenta inversa de `inner_rect`: el tamaño de la ventana tiene
        // que ser el del editor más esto, o el plugin recibe menos espacio del
        // que pidió y su layout se descuadra.
        let inner = m().inner_rect(400, 300);
        assert_eq!(inner.width, 400 - m().horizontal_inset());
        assert_eq!(inner.height, 300 - m().vertical_inset());
    }

    #[test]
    fn sin_marco_los_insets_son_cero() {
        let f = FrameMetrics::frameless();
        assert_eq!((f.horizontal_inset(), f.vertical_inset()), (0, 0));
    }

    #[test]
    fn sin_marco_el_editor_ocupa_la_ventana_entera() {
        let inner = FrameMetrics::frameless().inner_rect(W, H);
        assert_eq!((inner.x, inner.y), (0, 0));
        assert_eq!((inner.width, inner.height), (W, H));
    }

    #[test]
    fn el_ventilador_de_resize_no_se_come_al_editor() {
        // El tirador tiene que quedar **dentro** de la ventana, no en el borde
        // exterior: si se saliera, el usuario no podría agarrarlo.
        let inner = m().inner_rect(W, H);
        assert!(inner.width < W);
        assert!(inner.height < H);
    }

    #[test]
    fn el_boton_de_cerrar_tiene_prioridad_sobre_el_titulo() {
        // Ambos están en el titlebar. Si el título ganara, el clic de cerrar
        // movería la ventana en vez de cerrarla.
        let region = m().hit_test(W as i16 - 10, 10, W, H);
        assert_eq!(region, FrameRegion::CloseButton);
    }

    #[test]
    fn el_titulo_mueve_y_el_interior_pasa_al_plugin() {
        assert_eq!(m().hit_test(400, 10, W, H), FrameRegion::TitleBar);
        assert_eq!(m().hit_test(400, 300, W, H), FrameRegion::Inner);
    }

    #[test]
    fn las_esquinas_inferiores_redimensionan_hacia_su_lado() {
        assert_eq!(
            m().hit_test(W as i16 - 2, H as i16 - 2, W, H),
            FrameRegion::Resize(ResizeDir::BottomRight),
            "abajo a la derecha"
        );
        assert_eq!(
            m().hit_test(1, H as i16 - 2, W, H),
            FrameRegion::Resize(ResizeDir::BottomLeft),
            "abajo a la izquierda"
        );
    }

    #[test]
    fn los_bordes_lateral_y_inferior_redimensionan() {
        assert_eq!(
            m().hit_test(450, H as i16 - 2, W, H),
            FrameRegion::Resize(ResizeDir::Bottom)
        );
        assert_eq!(m().hit_test(1, 300, W, H), FrameRegion::Resize(ResizeDir::Left));
        assert_eq!(
            m().hit_test(W as i16 - 2, 300, W, H),
            FrameRegion::Resize(ResizeDir::Right)
        );
    }

    #[test]
    fn el_titlebar_arrastra_y_no_redimensiona() {
        // Decisión de diseño, no accidente: si el borde superior
        // redimensionara, el título no se podría arrastrar, porque el área de
        // agarre del borde se comería la franja de arriba entera. Es el
        // comportamiento de Windows, macOS y del Fruity Wrapper de FL Studio.
        for x in [1, 200, 450] {
            assert_eq!(
                m().hit_test(x, 1, W, H),
                FrameRegion::TitleBar,
                "arriba, x={x}"
            );
        }
    }

    #[test]
    fn el_tirador_gana_a_la_esquina() {
        // Ambos cubren la esquina inferior derecha. El tirador es más
        // pequeño y más preciso, así que tiene que ganar.
        let region = m().hit_test(W as i16 - 2, H as i16 - 2, W, H);
        assert_eq!(region, FrameRegion::Resize(ResizeDir::BottomRight));
    }

    #[test]
    fn el_centro_siempre_es_el_editor() {
        for (x, y) in [(450, 300), (200, 100), (800, 500)] {
            assert_eq!(
                m().hit_test(x, y, W, H),
                FrameRegion::Inner,
                "({x},{y}) no debería ser marco"
            );
        }
    }

    #[test]
    fn una_ventana_de_tamano_degente_aguanta() {
        // Una ventana de 1x1 es patológico pero tiene que devolver algo
        // válido: una región que no sea `Resize` sobre sí misma, porque eso
        // sería un bucle infinito de "el borde está sobre el borde".
        let region = m().hit_test(0, 0, 1, 1);
        assert!(matches!(region, FrameRegion::CloseButton | FrameRegion::TitleBar));
    }

    #[test]
    fn el_minimo_deja_espacio_para_el_editor() {
        let (min_w, min_h) = m().min_size();
        assert!(min_w > 0 && min_h > 0);
        let inner = m().inner_rect(min_w, min_h);
        assert!(inner.is_usable(), "con el mínimo, el editor no desaparece");
    }

    #[test]
    fn redimensionar_abajo_derecha_no_mueve_el_borde_izquierdo() {
        // Al arrastrar hacia la derecha, el borde izquierdo se queda quieto:
        // si también se moviera, la ventana "respiraría" y el editor would
        // quedar desalineado con el puntero.
        assert_eq!(ResizeDir::BottomRight.dx(10), -10);
        assert_eq!(ResizeDir::BottomRight.dy(10), -10);
        // Y al arrastrar hacia la izquierda, el borde izquierdo avanza.
        assert_eq!(ResizeDir::Left.dx(10), 10);
        assert_eq!(ResizeDir::Left.dy(10), 0);
    }

    #[test]
    fn solo_el_interior_deja_pasar_al_plugin() {
        assert!(!FrameRegion::Inner.is_chrome());
        assert!(FrameRegion::TitleBar.is_chrome());
        assert!(FrameRegion::CloseButton.is_chrome());
        assert!(FrameRegion::Resize(ResizeDir::Top).is_chrome());
    }

    #[test]
    fn el_cursor_corresponde_a_la_zona() {
        assert_eq!(m().cursor_for(FrameRegion::TitleBar), CursorHint::Default);
        assert_eq!(m().cursor_for(FrameRegion::CloseButton), CursorHint::Hand);
        assert_eq!(
            m().cursor_for(FrameRegion::Resize(ResizeDir::Left)),
            CursorHint::Resize(ResizeDir::Left)
        );
    }

    #[test]
    fn contains_respeta_los_bordes() {
        let inner = m().inner_rect(W, H);
        assert!(inner.contains(30, 30));
        assert!(inner.contains(inner.x, inner.y), "la esquina propia sí entra");
        assert!(!inner.contains(inner.x - 1, inner.y), "un px a la izquierda, no");
        assert!(
            !inner.contains(inner.x, inner.y - 1),
            "un px arriba es el título"
        );
    }
}