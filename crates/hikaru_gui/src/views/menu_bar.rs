// Hikaru OpenLive - Menu Bar (egui)
// GNU AGPLv3
// crates/hikaru_gui/src/views/menu_bar.rs

use egui::{menu, RichText, Ui};

use crate::app::{AppMode, AppState, OpenLiveView, OpenStudioView};
use crate::project;
use crate::theme;

pub const MENUS: [&str; 5] = ["FILE", "EDIT", "VIEW", "SETTINGS", "HELP"];

pub fn render(ui: &mut Ui, state: &mut AppState) {
    menu::bar(ui, |ui| {
        // FILE
        let file_label = if state.project_dirty {
            RichText::new("FILE *").color(theme::ACCENT_COLOR)
        } else {
            RichText::new("FILE")
        };
        ui.menu_button(file_label, |ui| {
            if ui.button("New Project").clicked() {
                new_project(state);
                ui.close_menu();
            }
            if ui.button("Open Project...").clicked() {
                open_project(state);
                ui.close_menu();
            }
            if ui.button("Save").clicked() {
                save_project(state, false);
                ui.close_menu();
            }
            if ui.button("Save As...").clicked() {
                save_project(state, true);
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Export Audio (WAV/FLAC)...").clicked() {
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Exit").clicked() {
                std::process::exit(0);
            }
        });

        // EDIT
        ui.menu_button("EDIT", |ui| {
            if ui.button("Undo").clicked() {
                ui.close_menu();
            }
            if ui.button("Redo").clicked() {
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Cut").clicked() {
                ui.close_menu();
            }
            if ui.button("Copy").clicked() {
                ui.close_menu();
            }
            if ui.button("Paste").clicked() {
                ui.close_menu();
            }
        });

        // VIEW
        ui.menu_button("VIEW", |ui| {
            if ui.button("Arrange View (Tab)").clicked() {
                state.mode = AppMode::OpenLive;
                state.openlive_view = OpenLiveView::SessionMatrix;
                ui.close_menu();
            }
            if ui.button("Mix View (Tab)").clicked() {
                state.mode = AppMode::OpenLive;
                state.openlive_view = OpenLiveView::ArrangerView;
                ui.close_menu();
            }
            if ui.button("Playlist / Timeline").clicked() {
                state.mode = AppMode::OpenStudio;
                state.openstudio_view = OpenStudioView::Playlist;
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Toggle Arrange / Mix (F9)").clicked() {
                if state.mode == AppMode::OpenLive {
                    state.openlive_view = match state.openlive_view {
                        OpenLiveView::SessionMatrix => OpenLiveView::ArrangerView,
                        OpenLiveView::ArrangerView => OpenLiveView::SessionMatrix,
                    };
                }
                ui.close_menu();
            }
            if ui.button(format!("Device Panel (F10) [{}]", if state.show_dsp_rack { "ON" } else { "OFF" })).clicked() {
                state.show_dsp_rack = !state.show_dsp_rack;
                ui.close_menu();
            }
            if ui.button(format!("Note Editor (F11) [{}]", if state.show_piano_roll { "ON" } else { "OFF" })).clicked() {
                state.show_piano_roll = !state.show_piano_roll;
                ui.close_menu();
            }
            if ui.button(format!("Browser [{}]", if state.show_explorer { "ON" } else { "OFF" })).clicked() {
                state.show_explorer = !state.show_explorer;
                ui.close_menu();
            }
        });

        // SETTINGS
        ui.menu_button("SETTINGS", |ui| {
            if ui.button("Audio Setup (JACK/ALSA/PipeWire)...").clicked() {
                state.audio_settings_state.is_open = true;
                ui.close_menu();
            }
            if ui.button("External VST3 / CLAP Plugin Settings...").clicked() {
                state.plugin_settings_state.is_open = true;
                ui.close_menu();
            }
            if ui.button("Hikaru MCP Port Settings...").clicked() {
                state.mcp_port_settings_state.open();
                ui.close_menu();
            }
            if ui.button("Control Panel / Dashboard...").clicked() {
                state.control_panel_state.is_open = true;
                ui.close_menu();
            }
        });

        // HELP
        ui.menu_button("HELP", |ui| {
            let label = match state.mode {
                AppMode::OpenLive => "About Hikaru OpenLive",
                AppMode::OpenStudio => "About Hikaru OpenStudio",
            };
            if ui.button(label).clicked() {
                state.show_about = true;
                ui.close_menu();
            }
        });
    });
}

// =========================================================================
// ACCIONES DE PROYECTO (FILE)
// =========================================================================

fn new_project(state: &mut AppState) {
    if state.project_dirty && !confirm_discard() {
        return;
    }

    project::reset(state);
    state.project_path = None;
    state.project_dirty = false;
    println!("[PROJECT] proyecto nuevo en memoria (sin archivo)");
}

fn open_project(state: &mut AppState) {
    let current_path = state.project_path.clone();
    let Some(path) = project::ask_open_path(current_path.as_deref()) else {
        return;
    };

    if let Err(err) = project::load(state, &path) {
        eprintln!("[PROJECT] no se pudo abrir: {}", err);
    } else {
        state.project_path = Some(path.clone());
        state.project_dirty = false;
        println!("[PROJECT] proyecto cargado: {}", path.display());
    }
}

fn save_project(state: &mut AppState, force_dialog: bool) {
    let current = state.project_path.clone();
    let mode = state.mode;

    let target = match (&current, force_dialog) {
        (Some(path), false) => path.clone(),
        _ => match project::ask_save_path(mode, current.as_deref()) {
            Some(path) => path,
            None => return,
        },
    };

    match project::save(state, &target) {
        Ok(saved) => {
            state.project_path = Some(saved.clone());
            state.project_dirty = false;
            println!("[PROJECT] proyecto guardado: {}", saved.display());
        }
        Err(err) => eprintln!("[PROJECT] no se pudo guardar: {}", err),
    }
}

fn confirm_discard() -> bool {
    rfd::MessageDialog::new()
        .set_title("Hikaru OpenLive")
        .set_description("El proyecto tiene cambios sin guardar.\n\n¿Descartarlos?")
        .set_buttons(rfd::MessageButtons::YesNo)
        .show()
        == rfd::MessageDialogResult::Yes
}
