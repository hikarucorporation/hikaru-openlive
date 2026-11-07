// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_openeq3/mod.rs

use egui::{Color32, RichText, Ui};

pub fn render_module(ui: &mut Ui) {
    ui.vertical_centered(|ui| {
        ui.label(RichText::new("OpenEQ3").size(12.0).strong().color(Color32::from_rgb(0xE8, 0xEA, 0xF0)));
        ui.label(RichText::new("Low | Mid | Hi").size(10.0).color(Color32::from_rgb(0x88, 0x88, 0x88)));
    });
}
