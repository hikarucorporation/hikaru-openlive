// Hikaru OpenLive - UI helpers
// crates/hikaru_gui/src/ui/text_style_scope.rs

use gpui_kit::*;

/// Aplica un `TextStyle` a todo el subárbol durante el pre-pintado y el pintado.
///
/// Los `Input` de gpui-kit no leen el color de su propio `StyleRefinement`: lo
/// toman de `Window::text_style()`, cuyo valor por defecto en GPUI es negro y
/// no sigue al tema. Un `div().text_color(..)` tampoco alcanza, porque `Div`
/// empuja su estilo de texto sólo durante `paint`, y el input decide el color
/// de sus text runs en `prepaint`.
///
/// `Window::with_text_style` tiene alcance léxico, así que hace falta un
/// elemento que lo empuje en las dos fases del frame.
pub struct TextStyleScope {
    style: TextStyleRefinement,
    child: AnyElement,
}

impl TextStyleScope {
    pub fn new(style: TextStyleRefinement, child: impl IntoElement) -> Self {
        Self {
            style,
            child: child.into_any_element(),
        }
    }

    /// Atajo para el caso habitual: sólo cambiar el color del texto.
    pub fn text_color(color: impl Into<Hsla>, child: impl IntoElement) -> Self {
        let mut style = TextStyleRefinement::default();
        style.color = Some(color.into());
        Self::new(style, child)
    }
}

impl IntoElement for TextStyleScope {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextStyleScope {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let style = self.style.clone();
        window.with_text_style(Some(style), |window| {
            self.child.prepaint(window, cx);
        });
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let style = self.style.clone();
        window.with_text_style(Some(style), |window| {
            self.child.paint(window, cx);
        });
    }
}
