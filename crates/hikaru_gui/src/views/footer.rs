// Hikaru OpenLive - Bitwig 5.1.7 1:1 Bottom Status & Panel Selector Bar
// GNU AGPLv3
// crates/hikaru_gui/src/views/footer.rs

use egui::{pos2, vec2, Align2, Color32, CursorIcon, RichText, Sense, Stroke, Ui};

use crate::app::{AppMode, AppState, OpenLiveView, OpenStudioView};
use crate::ui::bitwig_kit::{flat_icon_toggle, BW_ACCENT_ORANGE};
use crate::ui::fonts;
use crate::ui::icons;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusViewTab {
    Arrange,
    Mix,
    Edit,
}

fn draw_footer_sep(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(7.0, 16.0), Sense::hover());
    if ui.is_rect_visible(rect) {
        let x = rect.center().x;
        ui.painter().line_segment(
            [pos2(x, rect.min.y + 2.0), pos2(x, rect.max.y - 2.0)],
            Stroke::new(1.0_f32, Color32::from_rgb(76, 78, 84)),
        );
    }
}

pub fn render(ui: &mut Ui, state: &mut AppState) {
    let _show_clip_editor = state.clip_editor.show_editor;
    let show_piano_roll = state.show_piano_roll;
    let show_dsp_rack = state.show_dsp_rack;
    let show_explorer = state.show_explorer;
    let show_bottom_mixer = state.panel_layout.show_mixer;
    let show_automation = state.panel_layout.show_automation;

    let active_mode = state.mode;
    let openlive_view = state.openlive_view;
    let openstudio_view = state.openstudio_view;

    let context_help = state.footer_context_help.clone();
    let context_cues = state.footer_context_cues.clone();
    let active_mods = state.active_modifiers;

    let active_tab = match active_mode {
        AppMode::OpenLive => match openlive_view {
            OpenLiveView::SessionMatrix => StatusViewTab::Arrange,
            OpenLiveView::ArrangerView => StatusViewTab::Mix,
            OpenLiveView::EditView => StatusViewTab::Edit,
        },
        AppMode::OpenStudio => match openstudio_view {
            OpenStudioView::Playlist => StatusViewTab::Arrange,
            OpenStudioView::ArrangerMixer => StatusViewTab::Mix,
            OpenStudioView::EditView => StatusViewTab::Edit,
        },
    };

    let display_cues: Vec<(String, String)> = if !context_cues.is_empty() {
        context_cues
    } else if let Some((act, desc)) = context_help {
        vec![(act, desc)]
    } else {
        vec![
            ("DOUBLE-CLICK".into(), "Insert track or open Devices".into()),
            ("DRAG WAV".into(), "Drop sample onto Launcher or Arranger".into()),
        ]
    };

    let inactive_fg = Color32::from_rgb(206, 208, 214);

    egui::Frame::NONE
        .fill(Color32::from_rgb(38, 39, 43))
        .inner_margin(egui::Margin::symmetric(6, 2))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(18, 19, 22)))
        .show(ui, |ui| {
            ui.set_height(22.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;

                // 1. Inspector [i]
                if flat_icon_toggle(
                    ui,
                    vec2(20.0, 20.0),
                    state.panel_layout.show_inspector,
                    BW_ACCENT_ORANGE,
                    inactive_fg,
                    |p, r, fg| icons::paint_icon(p, r, icons::INFO_CIRCLE, fg, 1.4),
                )
                .on_hover_text("Toggle Inspector Panel")
                .clicked()
                {
                    state.panel_layout.show_inspector = !state.panel_layout.show_inspector;
                }

                draw_footer_sep(ui);

                // 2. Pestañas de espacio de trabajo de texto limpio estilo Bitwig: ARRANGE  MIX  EDIT
                let text_tab = |ui: &mut Ui, label: &str, tab: StatusViewTab, w: f32| -> bool {
                    let active = active_tab == tab;
                    let (rect, resp) = ui.allocate_exact_size(vec2(w, 20.0), Sense::click());
                    if ui.is_rect_visible(rect) {
                        let col = if active {
                            BW_ACCENT_ORANGE
                        } else if resp.hovered() {
                            Color32::WHITE
                        } else {
                            inactive_fg
                        };
                        ui.painter().text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            label,
                            fonts::semibold(11.0),
                            col,
                        );
                    }
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                    resp.clicked()
                };

                if text_tab(ui, "ARRANGE", StatusViewTab::Arrange, 48.0) {
                    state.openlive_view = OpenLiveView::SessionMatrix;
                    state.openstudio_view = OpenStudioView::Playlist;
                    state.clip_editor.show_editor = false;
                }
                if text_tab(ui, "MIX", StatusViewTab::Mix, 28.0) {
                    state.openlive_view = OpenLiveView::ArrangerView;
                    state.openstudio_view = OpenStudioView::ArrangerMixer;
                    state.clip_editor.show_editor = false;
                }
                if text_tab(ui, "EDIT", StatusViewTab::Edit, 30.0) {
                    state.openlive_view = OpenLiveView::EditView;
                    state.openstudio_view = OpenStudioView::EditView;
                    state.clip_editor.show_editor = false;
                }

                draw_footer_sep(ui);

                // 3. Selectores de panel inferior:
                // En pestaña EDIT estilo 1:1 Bitwig Studio, solamente están activos los dos iconitos:
                // Show Device Panel y Show Mixer Panel.
                // En pestañas ARRANGE y MIX se muestran los 4 selectores: Note Editor, Automation, Device Panel, Mixer.
                if active_tab == StatusViewTab::Edit {
                    let dev_resp = flat_icon_toggle(
                        ui,
                        vec2(22.0, 20.0),
                        show_dsp_rack,
                        BW_ACCENT_ORANGE,
                        inactive_fg,
                        |p, r, fg| icons::paint_device_box(p, r, fg, show_dsp_rack),
                    );
                    let dev_resp = dev_resp.on_hover_text("Show Device Panel");
                    if dev_resp.hovered() {
                        state.footer_context_help = Some(("CLICK".into(), "Show Device Panel".into()));
                    }
                    if dev_resp.clicked() {
                        state.show_dsp_rack = !state.show_dsp_rack;
                        if state.show_dsp_rack {
                            state.panel_layout.show_mixer = false;
                            state.show_piano_roll = false;
                            state.clip_editor.show_editor = false;
                        }
                    }

                    let mix_resp = flat_icon_toggle(
                        ui,
                        vec2(22.0, 20.0),
                        show_bottom_mixer,
                        BW_ACCENT_ORANGE,
                        inactive_fg,
                        |p, r, fg| icons::paint_mixer_faders(p, r, fg),
                    );
                    let mix_resp = mix_resp.on_hover_text("Show Mixer Panel");
                    if mix_resp.hovered() {
                        state.footer_context_help = Some(("CLICK".into(), "Show Mixer Panel".into()));
                    }
                    if mix_resp.clicked() {
                        state.panel_layout.show_mixer = !state.panel_layout.show_mixer;
                        if state.panel_layout.show_mixer {
                            state.show_dsp_rack = false;
                            state.show_piano_roll = false;
                            state.clip_editor.show_editor = false;
                        }
                    }
                } else {
                    if flat_icon_toggle(
                        ui,
                        vec2(22.0, 20.0),
                        show_piano_roll,
                        BW_ACCENT_ORANGE,
                        inactive_fg,
                        |p, r, fg| icons::paint_note_editor(p, r, fg),
                    )
                    .on_hover_text("Toggle Note Editor Panel")
                    .clicked()
                    {
                        state.show_piano_roll = !state.show_piano_roll;
                        if state.show_piano_roll {
                            state.show_dsp_rack = false;
                            state.clip_editor.show_editor = false;
                            state.panel_layout.show_mixer = false;
                        }
                    }

                    if flat_icon_toggle(
                        ui,
                        vec2(22.0, 20.0),
                        show_automation,
                        BW_ACCENT_ORANGE,
                        inactive_fg,
                        |p, r, fg| icons::paint_automation_nodes(p, r, fg, false),
                    )
                    .on_hover_text("Toggle Automation Lane")
                    .clicked()
                    {
                        state.panel_layout.show_automation = !state.panel_layout.show_automation;
                    }

                    if flat_icon_toggle(
                        ui,
                        vec2(22.0, 20.0),
                        show_dsp_rack,
                        BW_ACCENT_ORANGE,
                        inactive_fg,
                        |p, r, fg| icons::paint_device_box(p, r, fg, show_dsp_rack),
                    )
                    .on_hover_text("Toggle Device Panel (F10)")
                    .clicked()
                    {
                        state.show_dsp_rack = !state.show_dsp_rack;
                        if state.show_dsp_rack {
                            state.show_piano_roll = false;
                            state.clip_editor.show_editor = false;
                            state.panel_layout.show_mixer = false;
                        }
                    }

                    if flat_icon_toggle(
                        ui,
                        vec2(22.0, 20.0),
                        show_bottom_mixer,
                        BW_ACCENT_ORANGE,
                        inactive_fg,
                        |p, r, fg| icons::paint_mixer_faders(p, r, fg),
                    )
                    .on_hover_text("Toggle Sub-Mixer Panel")
                    .clicked()
                    {
                        state.panel_layout.show_mixer = !state.panel_layout.show_mixer;
                        if state.panel_layout.show_mixer {
                            state.show_dsp_rack = false;
                            state.show_piano_roll = false;
                            state.clip_editor.show_editor = false;
                        }
                    }
                }

                draw_footer_sep(ui);

                // 4. Ayudas contextuales estilo Bitwig
                ui.add_space(4.0);
                for (action, desc) in display_cues.iter().take(2) {
                    ui.label(
                        RichText::new(action)
                            .font(fonts::semibold(10.5))
                            .color(Color32::from_rgb(218, 220, 226)),
                    );
                    ui.label(
                        RichText::new(desc)
                            .font(fonts::regular(10.5))
                            .color(Color32::from_rgb(148, 152, 160)),
                    );
                    ui.add_space(8.0);
                }

                if active_mods.ctrl || active_mods.command {
                    ui.label(
                        RichText::new("CTRL")
                            .font(fonts::semibold(9.5))
                            .color(BW_ACCENT_ORANGE),
                    );
                }
                if active_mods.shift {
                    ui.label(
                        RichText::new("SHIFT")
                            .font(fonts::semibold(9.5))
                            .color(BW_ACCENT_ORANGE),
                    );
                }

                // 5. Extremo inferior derecho 1:1 Bitwig: Lupa Browser | Archivo | Plugins | Audio | Teclado
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 5.0;

                    if flat_icon_toggle(
                        ui,
                        vec2(22.0, 20.0),
                        state.panel_layout.show_keyboard,
                        BW_ACCENT_ORANGE,
                        inactive_fg,
                        |p, r, fg| icons::paint_icon(p, r, icons::PIANO, fg, 1.35),
                    )
                    .on_hover_text("Toggle On-Screen Musical Keyboard")
                    .clicked()
                    {
                        state.panel_layout.show_keyboard = !state.panel_layout.show_keyboard;
                    }

                    draw_footer_sep(ui);

                    if flat_icon_toggle(
                        ui,
                        vec2(20.0, 20.0),
                        state.audio_settings_state.is_open,
                        BW_ACCENT_ORANGE,
                        inactive_fg,
                        |p, r, fg| icons::paint_icon(p, r, icons::SETTINGS, fg, 1.35),
                    )
                    .on_hover_text("Audio Engine Settings")
                    .clicked()
                    {
                        state.audio_settings_state.is_open = !state.audio_settings_state.is_open;
                    }

                    if flat_icon_toggle(
                        ui,
                        vec2(20.0, 20.0),
                        state.plugin_settings_state.is_open,
                        BW_ACCENT_ORANGE,
                        inactive_fg,
                        |p, r, fg| icons::paint_icon(p, r, icons::PLUG, fg, 1.35),
                    )
                    .on_hover_text("VST3 / CLAP Plugin Locations")
                    .clicked()
                    {
                        state.plugin_settings_state.is_open = !state.plugin_settings_state.is_open;
                    }

                    if flat_icon_toggle(
                        ui,
                        vec2(20.0, 20.0),
                        state.mcp_port_settings_state.is_open,
                        BW_ACCENT_ORANGE,
                        inactive_fg,
                        |p, r, fg| icons::paint_icon(p, r, icons::FILE_PAGE, fg, 1.35),
                    )
                    .on_hover_text("Project & MCP Ports")
                    .clicked()
                    {
                        state.mcp_port_settings_state.open();
                    }

                    if flat_icon_toggle(
                        ui,
                        vec2(22.0, 20.0),
                        show_explorer,
                        BW_ACCENT_ORANGE,
                        inactive_fg,
                        |p, r, fg| icons::paint_icon(p, r, icons::SEARCH, fg, 1.55),
                    )
                    .on_hover_text("Toggle Popup / Side Browser (F11)")
                    .clicked()
                    {
                        state.show_explorer = !state.show_explorer;
                    }
                });
            });
        });
}
