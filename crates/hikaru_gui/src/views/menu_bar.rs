use gpui_kit::component::*;
use gpui_kit::component::button::Button;
use gpui_kit::component::label::Label;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, AppMode, HikaruApp, OpenLiveView, update_state};

struct MenuState {
    open: Option<usize>,
}

impl MenuState {
    fn toggle(&mut self, idx: usize) {
        self.open = if self.open == Some(idx) { None } else { Some(idx) };
    }
}

pub fn render(cx: &mut Context<HikaruApp>) -> impl IntoElement {
    let menu_state = cx.new(|_| MenuState { open: None });

    let items = ["FILE", "EDIT", "VIEW", "SETTINGS", "HELP"];

    h_flex()
        .id("menu_bar")
        .bg(rgb(0x2A2A2A))
        .px(px(4.0))
        .pb(px(2.0))
        .children(items.iter().enumerate().map(|(idx, label)| {
            let menu_state = menu_state.clone();
            let menu_state_click = menu_state.clone();
            let open = menu_state.read(cx).open == Some(idx);
            let label = label.to_string();
            div()
                .relative()
                .child(
                    Button::new(format!("menu_{}", label.to_lowercase()))
                        .label(label.clone())
                        .compact()
                        .bg(rgb(0x3D3D3D))
                        .text_color(rgb(0xE0E0E0))
                        .on_click(move |_, _, cx| {
                            menu_state_click.update(cx, |state, cx| {
                                state.toggle(idx);
                                cx.notify();
                            });
                        }),
                )
                .when(open, |this| this.child(render_dropdown(menu_state.clone(), idx, cx)))
        }))
}

fn render_dropdown(
    menu_state: Entity<MenuState>,
    idx: usize,
    cx: &mut Context<HikaruApp>,
) -> AnyElement {
    let mode = state(cx).read(cx).mode;

    let items: Vec<(&str, Box<dyn Fn(&mut App)>)> = match idx {
        0 => vec![
            ("📄 New Project", Box::new(|_| {})),
            ("📂 Open Project...", Box::new(|_| {})),
            ("💾 Save", Box::new(|_| {})),
            ("💾 Save As...", Box::new(|_| {})),
            ("🎵 Export Audio (WAV/FLAC)...", Box::new(|_| {})),
            ("❌ Exit", Box::new(|_| std::process::exit(0))),
        ],
        1 => vec![
            ("↩ Undo", Box::new(|_| {})),
            ("↪ Redo", Box::new(|_| {})),
            ("✂ Cut", Box::new(|_| {})),
            ("📋 Copy", Box::new(|_| {})),
            ("📋 Paste", Box::new(|_| {})),
        ],
        2 => vec![
            (
                "🔲 Session Matrix (Tab)",
                Box::new(|cx: &mut App| {
                    update_state(cx, |state| {
                        state.mode = AppMode::OpenLive;
                        state.openlive_view = OpenLiveView::SessionMatrix;
                    });
                }),
            ),
            (
                "🎼 Arranger View (Tab)",
                Box::new(|cx: &mut App| {
                    update_state(cx, |state| {
                        state.mode = AppMode::OpenLive;
                        state.openlive_view = OpenLiveView::ArrangerView;
                    });
                }),
            ),
            (
                "🎹 Playlist / Timeline",
                Box::new(|cx: &mut App| {
                    update_state(cx, |state| {
                        state.mode = AppMode::OpenStudio;
                    });
                }),
            ),
            (
                "🎛 Mixer (F9)",
                Box::new(|cx: &mut App| {
                    update_state(cx, |state| {
                        state.show_mixer = !state.show_mixer;
                    });
                }),
            ),
            (
                "🎚 DSP Rack (F10)",
                Box::new(|cx: &mut App| {
                    update_state(cx, |state| {
                        state.show_dsp_rack = !state.show_dsp_rack;
                    });
                }),
            ),
        ],
        3 => vec![
            (
                "🔊 Audio Setup (JACK/ALSA/PipeWire)...",
                Box::new(|cx: &mut App| {
                    update_state(cx, |state| {
                        state.audio_settings_state.is_open = true;
                    });
                }),
            ),
            (
                "🔌 External VST3 / CLAP Plugin Settings...",
                Box::new(|cx: &mut App| {
                    update_state(cx, |state| {
                        state.plugin_settings_state.is_open = true;
                    });
                }),
            ),
        ],
        _ => {
            let about_label = match mode {
                AppMode::OpenLive => "ℹ About Hikaru OpenLive",
                AppMode::OpenStudio => "ℹ About Hikaru OpenStudio",
            };
            vec![(
                about_label,
                Box::new(|cx: &mut App| {
                    update_state(cx, |state| {
                        state.show_about = true;
                    });
                }),
            )]
        }
    };

    div()
        .absolute()
        .top(px(24.0))
        .left(px(0.0))
        .w(px(220.0))
        .bg(rgb(0x1C1E24))
        .border_1()
        .border_color(rgb(0x2A2D37))
        .rounded(px(4.0))
        .p(px(4.0))
        .children(items.into_iter().map(|(label, action)| {
            let menu_state = menu_state.clone();
            div()
                .w_full()
                .child(
                    Button::new(format!("menu_item_{}", label))
                        .label(label)
                        .compact()
                        .w_full()
                        .on_click(move |_, _, cx| {
                            action(cx);
                            menu_state.update(cx, |state, cx| {
                                state.open = None;
                                cx.notify();
                            });
                        }),
                )
        }))
        .into_any_element()
}
