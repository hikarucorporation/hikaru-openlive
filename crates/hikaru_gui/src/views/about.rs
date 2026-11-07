// Hikaru OpenLive - About Window (egui)
// GNU AGPLv3
// crates/hikaru_gui/src/views/about.rs

use egui::{vec2, Context, RichText, Window};
use crate::app::{AppMode, AppState};
use crate::version::VERSION;
use crate::views::branding;

const BUILDER_NAME: &str = "Hikaru Corporation";

pub fn render(ctx: &Context, state: &mut AppState) {
    show(ctx, state);
}

pub fn show(ctx: &Context, state: &mut AppState) {
    if !state.show_about {
        return;
    }

    let title = match state.mode {
        AppMode::OpenLive => "About Hikaru OpenLive",
        AppMode::OpenStudio => "About Hikaru OpenStudio",
    };

    let mut open = state.show_about;
    Window::new(title)
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .default_width(420.0)
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                if let Some(texture) = branding::energy_core_texture(ctx) {
                    ui.image((texture.id(), vec2(96.0, 96.0)));
                }

                ui.heading(match state.mode {
                    AppMode::OpenLive => "Hikaru OpenLive",
                    AppMode::OpenStudio => "Hikaru OpenStudio",
                });

                ui.label(RichText::new("An Advanced Digital Audio Workstation for UNIX sysadmins.").size(11.0));
                ui.label(RichText::new("Copyright © Hikaru Corporation - 2026").size(11.0));
                ui.label(RichText::new("Free software protected by GNU AGPLv3").size(11.0));

                ui.separator();

                ui.label(RichText::new("Coming Soon").strong());
                ui.label(
                    RichText::new(
                        "Available on Windows, Linux, BSD distros like Free/Open/NetBSD, microwaves, calculators, anything that runs Rust.",
                    )
                    .size(10.0),
                );

                ui.add_space(8.0);
                ui.separator();

                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Version {}", VERSION)).size(10.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(format!("Compiled by {}", BUILDER_NAME)).size(10.0));
                    });
                });

                ui.add_space(4.0);
                if ui.button("Cerrar").clicked() {
                    state.show_about = false;
                }
            });
        });

    state.show_about = open;
}
