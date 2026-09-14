/* 
 * HIKARU OPENSTUDIO - MIXER VIEW
 * Licencia: GNU AGPLv3
 */

use egui::{Context, Ui, SidePanel};
use crate::AppState;
use crate::audio_proxy::{AudioProxy, GuiCommand};

pub fn show_window(ctx: &Context, state: &mut AppState, proxy: &AudioProxy) {
    SidePanel::right("mixer_panel")
        .resizable(true)
        .default_width(300.0)
        .show(ctx, |ui| {
            ui.heading("📊 Mixer");
            ui.separator();

            egui::ScrollArea::horizontal().show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Render de Canales
                    for (idx, track) in state.tracks.iter_mut().enumerate() {
                        ui.vertical(|ui| {
                            ui.set_width(60.0);
                            ui.label(&track.name);
                            
                            // Fader de Volumen
                            if ui.add(egui::Slider::new(&mut track.volume, -60.0..=6.0).vertical()).changed() {
                                proxy.send(GuiCommand::SetTrackVolume { track_idx: idx, volume_db: track.volume });
                            }
                            
                            // Pan
                            // REEMPLAZALO POR ESTO:
                            if ui.add(egui::Slider::new(&mut track.pan, -100.0..=100.0).show_value(false)).changed() {
                                proxy.send(GuiCommand::SetTrackPan { track_idx: idx, pan: track.pan / 100.0 });
                            }

                            if ui.selectable_label(track.is_muted, "M").clicked() {
                                track.is_muted = !track.is_muted;
                                proxy.send(GuiCommand::SetTrackMute { track_idx: idx, mute: track.is_muted });
                            }
                        });
                        ui.separator();
                    }

                    // Master Fader
                    ui.vertical(|ui| {
                        ui.set_width(60.0);
                        ui.label("MASTER");
                        if ui.add(egui::Slider::new(&mut state.master_volume, -60.0..=6.0).vertical()).changed() {
                            proxy.send(GuiCommand::SetMasterVolume { volume_db: state.master_volume });
                        }
                    });
                });
            });
        });
}
