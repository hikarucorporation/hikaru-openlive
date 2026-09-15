use egui::{Align, Layout, RichText, Ui};

pub fn show(ui: &mut Ui, cpu_usage: f32, show_clip_editor: &mut bool) {
    ui.horizontal(|ui| {
        // Usamos .strong() en lugar de .bold()
        let icon = if *show_clip_editor { "🎹 CLIP EDITOR [▼]" } else { "🎹 CLIP EDITOR [▲]" };
        if ui.selectable_label(*show_clip_editor, RichText::new(icon).small().strong()).clicked() {
            *show_clip_editor = !*show_clip_editor;
        }

        ui.separator();
        ui.label(RichText::new("HIKARU OPENSTUDIO | AGPLv3").small());
        ui.separator();
        ui.label(RichText::new(format!("CPU: {:.1}%", cpu_usage * 100.0)).small());

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new("ENGINE: IDLE").small());
        });
    });
}