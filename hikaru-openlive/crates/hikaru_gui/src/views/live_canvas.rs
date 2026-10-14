use gpui_kit::*;

pub struct LiveCanvas<T> {
    prepaint: Box<dyn FnMut(Bounds<Pixels>, &mut Window, &mut App) -> T>,
    paint: Box<dyn FnMut(Bounds<Pixels>, &mut T, &mut Window, &mut App)>,
    style: StyleRefinement,
}

impl<T: 'static> LiveCanvas<T> {
    pub fn new(
        prepaint: impl FnMut(Bounds<Pixels>, &mut Window, &mut App) -> T + 'static,
        paint: impl FnMut(Bounds<Pixels>, &mut T, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            prepaint: Box::new(prepaint),
            paint: Box::new(paint),
            style: StyleRefinement::default(),
        }
    }
}

impl<T: 'static> IntoElement for LiveCanvas<T> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<T> Styled for LiveCanvas<T> {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl<T: 'static> Element for LiveCanvas<T> {
    type RequestLayoutState = Style;
    type PrepaintState = Option<T>;

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
        let mut style = Style::default();
        style.refine(&self.style);
        let layout_id = window.request_layout(style.clone(), [], cx);
        (layout_id, style)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Style,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<T> {
        Some((self.prepaint)(bounds, window, cx))
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        style: &mut Style,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let mut prepaint = prepaint.take().unwrap();
        style.paint(bounds, window, cx, |window, cx| {
            (self.paint)(bounds, &mut prepaint, window, cx)
        })
    }
}
