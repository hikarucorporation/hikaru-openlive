// crates/hikaru_gui/src/views/footer.rs
use egui::{Align, Layout, RichText, Ui};

pub fn show(
    ui: &mut Ui,
    cpu_usage: f32,
    show_clip_editor: &mut bool,
    show_piano_roll: &mut bool,
    show_dsp_rack: &mut bool,
) {
    ui.horizontal(|ui| {
        // Toggle Clip Editor
        let clip_icon = if *show_clip_editor { "🎛 CLIP EDITOR [▼]" } else { "🎛 CLIP EDITOR [▲]" };
        if ui.selectable_label(*show_clip_editor, RichText::new(clip_icon).small().strong()).clicked() {
            *show_clip_editor = !*show_clip_editor;
            if *show_clip_editor {
                *show_piano_roll = false;
                *show_dsp_rack = false;
            }
        }

        ui.separator();

        // Toggle Piano Roll / Drum Sequencer
        let pr_icon = if *show_piano_roll { "🎹 PIANO ROLL [▼]" } else { "🎹 PIANO ROLL [▲]" };
        if ui.selectable_label(*show_piano_roll, RichText::new(pr_icon).small().strong()).clicked() {
            *show_piano_roll = !*show_piano_roll;
            if *show_piano_roll {
                *show_clip_editor = false;
                *show_dsp_rack = false;
            }
        }

        ui.separator();

        // Toggle DSP FX Rack
        let dsp_icon = if *show_dsp_rack { "🎚 DSP RACK [▼]" } else { "🎚 DSP RACK [▲]" };
        if ui.selectable_label(*show_dsp_rack, RichText::new(dsp_icon).small().strong()).clicked() {
            *show_dsp_rack = !*show_dsp_rack;
            if *show_dsp_rack {
                *show_clip_editor = false;
                *show_piano_roll = false;
            }
        }

        ui.separator();
        ui.label(RichText::new("Hikaru OpenLive | AGPLv3").small());
        ui.separator();
        ui.label(RichText::new(format!("CPU: {:.1}%", cpu_usage * 100.0)).small());

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new("ENGINE: IDLE").small());
        });
    });
}