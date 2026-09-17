/*
 * Hikaru OpenStudio - Global Menu Bar
 * License: AGPL-3.0-or-later
 */

use egui::Ui;
use crate::app::{HikaruApp, AppMode};

pub fn show(ui: &mut Ui, app: &mut HikaruApp) {
    egui::menu::bar(ui, |ui| {
        ui.style_mut().visuals.button_frame = false;

        // --- FILE ---
        ui.menu_button("FILE", |ui| {
            if ui.button("📄 New Project").clicked() {
                ui.close_menu();
            }
            if ui.button("📂 Open Project...").clicked() {
                ui.close_menu();
            }
            if ui.button("💾 Save").clicked() {
                ui.close_menu();
            }
            if ui.button("💾 Save As...").clicked() {
                ui.close_menu();
            }
            ui.separator();
            if ui.button("🎵 Export Audio (WAV/FLAC)...").clicked() {
                ui.close_menu();
            }
            ui.separator();
            if ui.button("❌ Exit").clicked() {
                std::process::exit(0);
            }
        });

        // --- EDIT ---
        ui.menu_button("EDIT", |ui| {
            if ui.button("↩ Undo").clicked() {
                ui.close_menu();
            }
            if ui.button("↪ Redo").clicked() {
                ui.close_menu();
            }
            ui.separator();
            if ui.button("✂ Cut").clicked() {
                ui.close_menu();
            }
            if ui.button("📋 Copy").clicked() {
                ui.close_menu();
            }
            if ui.button("📋 Paste").clicked() {
                ui.close_menu();
            }
        });

        // --- VIEW ---
        ui.menu_button("VIEW", |ui| {
            // Submenú OpenLive (Ableton-style workflow)
            ui.menu_button("🎛 OpenLive View", |ui| {
                if ui.button("🔲 Session Matrix (Tab)").clicked() {
                    app.mode = AppMode::OpenLive;
                    // TODO: Activar pestaña Session Matrix
                    ui.close_menu();
                }
                if ui.button("🎼 Arranger View (Tab)").clicked() {
                    app.mode = AppMode::OpenLive;
                    // TODO: Activar pestaña Arranger View
                    ui.close_menu();
                }
            });

            // Submenú OpenStudio (FL Studio/Traditional workflow)
            ui.menu_button("🎧 OpenStudio View", |ui| {
                if ui.button("🎹 Playlist / Timeline (F5)").clicked() {
                    app.mode = AppMode::OpenStudio;
                    ui.close_menu();
                }
            });

            ui.separator();

            if ui.button("🎛 Mixer (F9)").clicked() {
                app.show_mixer = !app.show_mixer;
                ui.close_menu();
            }
            if ui.button("🎚 DSP Rack (F10)").clicked() {
                app.show_dsp_rack = !app.show_dsp_rack;
                ui.close_menu();
            }
            if ui.button("🎹 Piano Roll (F4)").clicked() {
                ui.close_menu();
            }
            ui.separator();
            if ui.button("🔄 Reset Layout").clicked() {
                ui.close_menu();
            }
        });

        // --- SETTINGS ---
        ui.menu_button("SETTINGS", |ui| {
            if ui.button("🔊 Audio Setup (JACK/ALSA/PipeWire)...").clicked() {
                // Abrimos el modal nativo de Audio Setup
                app.audio_settings_state.is_open = true;
                ui.close_menu();
            }
            if ui.button("🎹 MIDI Devices...").clicked() {
                ui.close_menu();
            }
            if ui.button("🔌 External VST3 / CLAP Plugin Settings...").clicked() {
                app.plugin_settings_state.is_open = true; // <-- Asignar true y cerrar con punto y coma ';'
                // ui.close_menu();
            }
            ui.separator();
            if ui.button("🎨 Interface & Themes").clicked() {
                ui.close_menu();
            }
        });

        // --- HELP ---
        ui.menu_button("HELP", |ui| {
            if ui.button("📖 Manual / Docs").clicked() {
                ui.close_menu();
            }
            if ui.button("⌨ Keyboard Shortcuts").clicked() {
                ui.close_menu();
            }
            ui.separator();

            let about_label = match app.mode {
                AppMode::OpenLive => "ℹ About Hikaru OpenLive",
                AppMode::OpenStudio => "ℹ About Hikaru OpenStudio",
            };

            if ui.button(about_label).clicked() {
                app.show_about = true;
                ui.close_menu();
            }
        });
    });
}