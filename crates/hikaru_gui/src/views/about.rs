use gpui_kit::component::button::Button;
use gpui_kit::component::label::Label;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use std::sync::{Arc, OnceLock};

use crate::app::{state, AppMode, HikaruApp};
use crate::render::render_image_from_rgba;
use crate::version::VERSION;

const BUILDER_NAME: &str = "Hikaru Corporation";

/// Lado del logo, en px. Cuadrado porque el asset es 512x512.
const LOGO_SIZE: f32 = 96.0;

/// El logo "Energy Core" embebido en el binario.
///
/// Va con `include_bytes!` y no como ruta en disco a propósito: el ejecutable
/// tiene que verse igual sin el árbol de assets al lado (ver el mismo criterio
/// en `views/wavetable_io.rs` con la wavetable de fábrica).
///
/// **Ojo con la extensión**: el diseño vive en
/// `assets/logos/hikaru_energy_core_2.svg`, pero lo que se embebe es el `.png`
/// rasterizado a mano del mismo logo. GPUI no trae un rasterizador de SVG --
/// el `img()` sólo acepta un `RenderImage` que ya son bytes de píxeles--, así
/// que un SVG suelto no se podría pintar. Los dos archivos tienen que seguir
/// siendo el mismo diseño: si se edita el SVG hay que re-rasterizar el PNG.
const ENERGY_CORE_LOGO: &[u8] =
    include_bytes!("../../../../assets/logos/hikaru_energy_core_2.png");

/// El [`RenderImage`] del logo, decodificado una sola vez y cacheado.
///
/// Decodificar son 512x512 RGBA más el `swap` a BGRA: hecho por render del
/// About wastea CPU en cada frame que se abre el modal. `OnceLock` alcanza
/// porque el asset es una constante de la app -- si algún día el logo pasa a
/// depender del tema o del modo, hay que cambiar la clave a algo tipo un
/// `HashMap` en el estado global.
fn energy_core_logo() -> Option<Arc<RenderImage>> {
    static LOGO: OnceLock<Option<Arc<RenderImage>>> = OnceLock::new();

    LOGO.get_or_init(|| {
        let decoded = image::load_from_memory(ENERGY_CORE_LOGO)
            .expect("el PNG del logo va embebido y fue verificado al compilar");
        let rgba = decoded.to_rgba8();
        let (width, height) = (rgba.width(), rgba.height());

        // `to_rgba8` devuelve RGBA y `render_image_from_rgba` lo pasa a BGRA,
        // que es lo que espera el atlas de GPUI. Ver ese helper y el test
        // `render_image_swaps_red_and_blue`.
        Some(Arc::new(render_image_from_rgba(
            width,
            height,
            rgba.into_raw(),
        )))
    })
    .clone()
}

pub fn render(cx: &mut Context<HikaruApp>) -> AnyElement {
    let mode = state(cx).read(cx).mode;

    let title = match mode {
        AppMode::OpenLive => "Hikaru OpenLive",
        AppMode::OpenStudio => "Hikaru OpenStudio",
    };
    let daw_name = title;

    v_flex()
        .id("about_window")
        .absolute()
        .left(px(20.0))
        .top(px(80.0))
        .w(px(420.0))
        .h(px(420.0))
        .bg(rgb(0x181A20))
        .border_1()
        .border_color(rgb(0x2A2D37))
        .rounded(px(6.0))
        .p(px(16.0))
        .gap(px(8.0))
        .items_center()
        .child(match energy_core_logo() {
            Some(image) => img(image)
                .id("about_energy_core_logo")
                .w(px(LOGO_SIZE))
                .h(px(LOGO_SIZE))
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
            // Sólo pasa si el PNG embebido no decodifica, que para un asset
            // versionado junto al código ya sería un bug de build. Preferimos
            // un hueco vacío a un `expect` que revienta la app al abrir el About.
            None => div().w(px(LOGO_SIZE)).h(px(LOGO_SIZE)).into_any_element(),
        })
        .child(Label::new(title).text_xl().font_weight(FontWeight::BOLD))
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
                        cx.notify();
                    });
                }),
        )
        .into_any_element()
}
