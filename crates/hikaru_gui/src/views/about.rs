use gpui_kit::component::button::Button;
use gpui_kit::component::label::Label;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, AppMode, HikaruApp};
use crate::views::branding::energy_core_logo_img;
use crate::views::modal::{self, ModalId, ModalRoot};
use crate::version::VERSION;

const BUILDER_NAME: &str = "Hikaru Corporation";

/// Lado del logo, en px. Cuadrado porque el asset se re-encuadra a cuadrado.
const LOGO_SIZE: f32 = 96.0;

/// Posición base de la ventana (px, desde la esquina superior izquierda de la
/// ventana principal). El arrastre la mueve desde acá.
const BASE_LEFT: f32 = 20.0;
const BASE_TOP: f32 = 80.0;

pub fn render(cx: &mut Context<HikaruApp>) -> AnyElement {
    let app = state(cx).read(cx);
    let mode = app.mode;
    // El desplazamiento del arrastre de la ventana.
    let drag = app.modal_drags.get(ModalId::About);
    let offset = drag.offset();
    let dragging = drag.is_dragging();
    drop(app);

    let title = match mode {
        AppMode::OpenLive => "Hikaru OpenLive",
        AppMode::OpenStudio => "Hikaru OpenStudio",
    };
    let daw_name = title;

    // La posición de la ventana: la base más el desplazamiento del arrastre.
    let (left, top) = modal::modal_position(BASE_LEFT, BASE_TOP, offset);

    let panel = v_flex()
        .id("about_window")
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(420.0))
        .h(px(420.0))
        .bg(rgb(0x181A20))
        .border_1()
        .border_color(rgb(0x2A2D37))
        .rounded(px(6.0))
        .p(px(16.0))
        .gap(px(8.0))
        .items_center()
        .child(energy_core_logo_img("about_energy_core_logo", LOGO_SIZE))
        // El título es el ASA de arrastre de la ventana: el usuario la agarra
        // y la mueve.
        .child(
            modal::drag_handle(
                h_flex()
                    .id("about_window_title")
                    .w_full()
                    .justify_center()
                    .child(Label::new(title).text_xl().font_weight(FontWeight::BOLD)),
                ModalId::About,
                dragging,
            ),
        )
        .child(Label::new("An Advanced Digital Audio Workstation for UNIX sysadmins.").text_xs())
        .child(Label::new("Copyright © Hikaru Corporation - 2026").text_xs())
        .child(Label::new("This is a free software protected by GNU AGPLv3:").text_xs())
        .child(
            Button::new("about-license").rounded(gpui_kit::component::button::ButtonRounded::None)
                .label("GNU AGPLv3 License")
                .compact()
                .on_click(move |_, _, _| {}),
        )
        .child(Label::new("Coming Soon").text_sm().font_weight(FontWeight::BOLD))
        .child(Label::new("Available on Windows, Linux, BSD distros like Free/Open/NetBSD, microwaves, calculator, potatoes, your mother, apache helicopter, anything shit that run Rust, etc.").text_xs())
        .child(div().flex_1())
        .child(
            h_flex()
                .w_full()
                .justify_between()
                .child(Label::new(daw_name).text_xs())
                .child(Label::new(format!("Compiled by {} | Version {}", BUILDER_NAME, VERSION)).text_xs()),
        )
        .child(
            Button::new("about_close").rounded(gpui_kit::component::button::ButtonRounded::None)
                .label("Cerrar")
                .compact()
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    cx.update_entity(&st, |state, cx| {
                        state.show_about = false;
                        // La ventana vuelve a su posición base la próxima vez
                        // que abra.
                        state.modal_drags.reset(ModalId::About);
                        cx.notify();
                    });
                }),
        )
        // El root del diálogo se traga los clics: los del panel nunca bajan
        // al fondo de la app.
        .absorb_clicks()
        .into_any_element();

    // El backdrop cubre toda la ventana: lo que se clicque fuera del panel no
    // llega a la Session Matrix ni al resto de la app.
    modal::overlay("about_window_backdrop", panel)
}
