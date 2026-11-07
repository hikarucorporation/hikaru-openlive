// Hikaru OpenLive - UI helpers (egui)
// crates/hikaru_gui/src/ui/text_style_scope.rs

use egui::Color32;

/// En egui no hace falta un scope de layout como en GPUI;
/// este struct se mantiene para compatibilidad con código que estilizaba inputs.
pub struct TextStyleScope;

impl TextStyleScope {
    pub fn text_color<R>(_color: Color32, add_contents: impl FnOnce() -> R) -> R {
        add_contents()
    }
}
