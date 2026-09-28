// Hikaru OpenLive - Header Bar Redesigned
// GNU AGPLv3
// crates/hikaru_gui/src/views/header_bar.rs

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, AppMode, HikaruApp};
use crate::audio_proxy::GuiCommand;
use crate::theme;

pub fn render(cx: &mut Context<HikaruApp>) -> impl IntoElement {
    let app = state(cx).read(cx);
    let mode = app.mode;
    let bpm = app.transport.bpm;
    let is_playing = app.transport.playback_state
        == hikaru_transport::transport_state::TransportPlaybackState::Playing;
    
    // Formatear el tiempo transcurrido (e.g. 00:00:19)
    let elapsed_secs = app.transport.position_seconds;
    let mins = (elapsed_secs / 60.0) as u32;
    let secs = (elapsed_secs % 60.0) as u32;
    let ms = ((elapsed_secs - elapsed_secs.floor()) * 1000.0) as u32;
    let time_str = format!("{:02}:{:02}.{:03}", mins, secs, ms);

    h_flex()
        .id("header_bar")
        .h(px(38.0))
        .w_full()
        .bg(theme::HEADER_BG)
        .border_b_1()
        .border_color(theme::BORDER_COLOR)
        .items_center()
        .px(px(10.0))
        .gap(px(12.0))
        
        // -----------------------------------------------------------------
        // SECCIÓN IZQUIERDA: LOGO + SWITCH DE MODOS (OPENLIVE / OPENSTUDIO)
        // -----------------------------------------------------------------
        .child(
            h_flex()
                .gap(px(8.0))
                .items_center()
                .child(
                    div()
                        .px(px(6.0))
                        .py(px(2.0))
                        .bg(theme::ACCENT_ORANGE)
                        .rounded(px(3.0))
                        .child(
                            Label::new("HIKARU")
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .color(theme::WINDOW_BG),
                        ),
                )
                // Botón OpenLive
                .child(
                    div()
                        .id("mode_live")
                        .px(px(10.0))
                        .py(px(4.0))
                        .rounded(px(3.0))
                        .bg(if mode == AppMode::OpenLive { theme::SURFACE_BG } else { theme::HEADER_BG })
                        .border_1()
                        .border_color(if mode == AppMode::OpenLive { theme::ACCENT_ORANGE } else { theme::BORDER_COLOR })
                        .cursor_pointer()
                        .on_click(|_, _, cx| {
                            state(cx).update(cx, |st, _| st.mode = AppMode::OpenLive);
                        })
                        .child(
                            Label::new("OPENLIVE")
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .color(if mode == AppMode::OpenLive { theme::TEXT_PRIMARY } else { theme::TEXT_MUTED }),
                        ),
                )
                // Botón OpenStudio
                .child(
                    div()
                        .id("mode_studio")
                        .px(px(10.0))
                        .py(px(4.0))
                        .rounded(px(3.0))
                        .bg(if mode == AppMode::OpenStudio { theme::SURFACE_BG } else { theme::HEADER_BG })
                        .border_1()
                        .border_color(if mode == AppMode::OpenStudio { theme::ACCENT_ORANGE } else { theme::BORDER_COLOR })
                        .cursor_pointer()
                        .on_click(|_, _, cx| {
                            state(cx).update(cx, |st, _| st.mode = AppMode::OpenStudio);
                        })
                        .child(
                            Label::new("OPENSTUDIO")
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .color(if mode == AppMode::OpenStudio { theme::TEXT_PRIMARY } else { theme::TEXT_MUTED }),
                        ),
                ),
        )

        .child(div().flex_1()) // Spacer

        // -----------------------------------------------------------------
        // SECCIÓN CENTRAL: TRANSPORT CONTROLS (PLAY / STOP / BPM / TEMPO)
        // -----------------------------------------------------------------
        .child(
            h_flex()
                .gap(px(6.0))
                .items_center()
                .bg(theme::WINDOW_BG)
                .px(px(8.0))
                .py(px(3.0))
                .rounded(px(4.0))
                .border_1()
                .border_color(theme::BORDER_COLOR)
                
                // Botón Play / Pause
                .child(
                    div()
                        .id("btn_play")
                        .w(px(28.0))
                        .h(px(24.0))
                        .rounded(px(3.0))
                        .bg(if is_playing { theme::PLAY_GREEN } else { theme::SURFACE_BG })
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            st.update(cx, |s, _| {
                                s.transport.playback_state = if is_playing {
                                    hikaru_transport::transport_state::TransportPlaybackState::Paused
                                } else {
                                    hikaru_transport::transport_state::TransportPlaybackState::Playing
                                };
                                if !is_playing {
                                    s.audio_proxy.send(GuiCommand::Play);
                                } else {
                                    s.audio_proxy.send(GuiCommand::Pause);
                                }
                            });
                        })
                        .child(
                            Label::new(if is_playing { "⏸" } else { "▶" })
                                .text_xs()
                                .color(if is_playing { theme::WINDOW_BG } else { theme::TEXT_PRIMARY }),
                        ),
                )
                
                // Reloj de Tiempo LCD
                .child(
                    div()
                        .px(px(8.0))
                        .py(px(2.0))
                        .bg(theme::HEADER_BG)
                        .rounded(px(2.0))
                        .border_1()
                        .border_color(theme::BORDER_COLOR)
                        .child(
                            Label::new(time_str)
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .color(theme::ACCENT_CYAN),
                        ),
                )
                
                // BPM Control
                .child(
                    h_flex()
                        .gap(px(4.0))
                        .items_center()
                        .child(Label::new("BPM").text_xs().color(theme::TEXT_MUTED))
                        .child(
                            div()
                                .id("btn_bpm")
                                .px(px(6.0))
                                .py(px(2.0))
                                .bg(theme::SURFACE_BG)
                                .rounded(px(2.0))
                                .cursor_pointer()
                                .on_click(move |_, _, cx| {
                                    let st = state(cx);
                                    st.update(cx, |s, _| {
                                        s.transport.bpm = if s.transport.bpm >= 180.0 { 60.0 } else { s.transport.bpm + 5.0 };
                                        s.audio_proxy.send(GuiCommand::SetBpm(s.transport.bpm as f32));
                                    });
                                })
                                .child(
                                    Label::new(format!("{:.0}", bpm))
                                        .text_xs()
                                        .font_weight(FontWeight::BOLD)
                                        .color(theme::TEXT_PRIMARY),
                                ),
                        ),
                ),
        )

        .child(div().flex_1()) // Spacer

        // -----------------------------------------------------------------
        // SECCIÓN DERECHA: ESTADO DEL MOTOR
        // -----------------------------------------------------------------
        .child(
            h_flex()
                .gap(px(6.0))
                .items_center()
                .child(
                    div()
                        .w(px(8.0))
                        .h(px(8.0))
                        .rounded(px(4.0))
                        .bg(theme::PLAY_GREEN),
                )
                .child(
                    Label::new("AUDIO ENGINE READY")
                        .text_xs()
                        .color(theme::TEXT_MUTED),
                ),
        )
}