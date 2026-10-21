use gpui_kit::component::*;
use gpui_kit::component::button::Button;
use gpui_kit::component::label::Label;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, HikaruApp};
use crate::views::modal::{self, ModalId, ModalRoot};

/// Posición base de la ventana (px, desde la esquina superior izquierda de la
/// ventana principal). El arrastre la mueve desde acá.
const BASE_LEFT: f32 = 20.0;
const BASE_TOP: f32 = 80.0;

#[derive(Clone, Debug, PartialEq)]
pub enum AudioBackend {
    PipeWire,
    Jack,
    Alsa,
    PulseAudio,
    HikaruNative,
}

pub struct AudioSettingsState {
    pub is_open: bool,
    pub selected_backend: AudioBackend,
    pub selected_device: String,
    pub sample_rate: u32,
    pub buffer_size: u32,
    pub available_devices: Vec<String>,
}

impl Default for AudioSettingsState {
    fn default() -> Self {
        Self {
            is_open: false,
            selected_backend: AudioBackend::HikaruNative,
            selected_device: "Hikaru Low-Latency Engine".to_string(),
            sample_rate: 44100,
            buffer_size: 128,
            available_devices: vec![
                "Hikaru Low-Latency Engine".to_string(),
                "Default Output Device".to_string(),
                "ALSA: PulseAudio / PipeWire Sound Server".to_string(),
                "JACK Audio Connection Kit".to_string(),
            ],
        }
    }
}

pub fn render(cx: &mut Context<HikaruApp>) -> AnyElement {
    let app = state(cx).read(cx);
    let st = &app.audio_settings_state;
    let backend = st.selected_backend.clone();
    let device = st.selected_device.clone();
    let sample_rate = st.sample_rate;
    let buffer_size = st.buffer_size;
    let devices = st.available_devices.clone();
    // El desplazamiento del arrastre de la ventana.
    let drag = app.modal_drags.get(ModalId::AudioSettings);
    let offset = drag.offset();
    let dragging = drag.is_dragging();
    drop(app);

    // La posición de la ventana: la base más el desplazamiento del arrastre.
    let (left, top) = modal::modal_position(BASE_LEFT, BASE_TOP, offset);

    let panel = v_flex()
        .id("audio_settings")
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(440.0))
        .h(px(320.0))
        .bg(rgb(0x181A20))
        .border_1()
        .border_color(rgb(0x2A2D37))
        .rounded(px(6.0))
        .p(px(12.0))
        .gap(px(8.0))
        // El título es el ASA de arrastre de la ventana: el usuario la agarra
        // y la mueve.
        .child(
            modal::drag_handle(
                h_flex()
                    .id("audio_settings_title")
                    .w_full()
                    .items_center()
                    .child(
                        Label::new("CONFIGURACIÓN DE AUDIO & HARDWARE")
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0x00FFFF)),
                    ),
                ModalId::AudioSettings,
                dragging,
            ),
        )
        .child(
            h_flex()
                .gap(px(6.0))
                .child(Label::new("Driver / Subsistema:").text_xs())
                .child(
                    Button::new("audio_backend_btn").rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label(format!("{:?}", backend))
                        .compact()
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            st.update(cx, |state, cx| {
                                state.audio_settings_state.selected_backend = match state.audio_settings_state.selected_backend {
                                    AudioBackend::HikaruNative => AudioBackend::PipeWire,
                                    AudioBackend::PipeWire => AudioBackend::Jack,
                                    AudioBackend::Jack => AudioBackend::Alsa,
                                    AudioBackend::Alsa => AudioBackend::PulseAudio,
                                    AudioBackend::PulseAudio => AudioBackend::HikaruNative,
                                };
                                cx.notify();
                            });
                        }),
                ),
        )
        .child(
            h_flex()
                .gap(px(6.0))
                .child(Label::new("Dispositivo de Salida:").text_xs())
                .child(
                    Button::new("audio_device_btn").rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label(device.clone())
                        .compact()
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            st.update(cx, |state, cx| {
                                let devs = &state.audio_settings_state.available_devices;
                                if !devs.is_empty() {
                                    let idx = devs.iter().position(|d| *d == device).unwrap_or(0);
                                    let next = devs[(idx + 1) % devs.len()].clone();
                                    state.audio_settings_state.selected_device = next;
                                }
                                cx.notify();
                            });
                        }),
                ),
        )
        .child(
            h_flex()
                .gap(px(6.0))
                .child(Label::new("Sample Rate:").text_xs())
                .child(
                    Button::new("audio_sr_btn").rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label(format!("{} Hz", sample_rate))
                        .compact()
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            st.update(cx, |state, cx| {
                                state.audio_settings_state.sample_rate = match state.audio_settings_state.sample_rate {
                                    44100 => 48000,
                                    48000 => 88200,
                                    88200 => 96000,
                                    _ => 44100,
                                };
                                cx.notify();
                            });
                        }),
                ),
        )
        .child(
            h_flex()
                .gap(px(6.0))
                .child(Label::new("Buffer Size:").text_xs())
                .child(
                    Button::new("audio_buf_btn").rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label(format!("{} samples", buffer_size))
                        .compact()
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            st.update(cx, |state, cx| {
                                state.audio_settings_state.buffer_size = match state.audio_settings_state.buffer_size {
                                    128 => 256,
                                    256 => 512,
                                    512 => 1024,
                                    1024 => 2048,
                                    2048 => 4096,
                                    _ => 128,
                                };
                                cx.notify();
                            });
                        }),
                ),
        )
        .child(div().flex_1())
        .child(
            h_flex()
                .gap(px(6.0))
                .child(
                    Button::new("audio_restart").rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("Reiniciar Driver Audio")
                        .compact()
                        .on_click(move |_, _, _| {}),
                )
                .child(div().flex_1())
                .child(
                    Button::new("audio_close").rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("Cerrar")
                        .compact()
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            st.update(cx, |state, cx| {
                                state.audio_settings_state.is_open = false;
                                // La ventana vuelve a su posición base la
                                // próxima vez que abra.
                                state.modal_drags.reset(ModalId::AudioSettings);
                                cx.notify();
                            });
                        }),
                ),
        )
        // El root del diálogo se traga los clics: los del panel nunca bajan
        // al fondo de la app.
        .absorb_clicks()
        .into_any_element();

    // El backdrop cubre toda la ventana: lo que se clicque fuera del panel no
    // llega a la Session Matrix ni al resto de la app.
    modal::overlay("audio_settings_backdrop", panel)
}
