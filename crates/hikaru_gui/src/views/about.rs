/*
 * Hikaru OpenLive / OpenStudio - About Dialog
 * License: AGPL-3.0-or-later
 *
 * NOTA IMPORTANTE - TAMAÑO DE VENTANA:
 * ---------------------------------------------------------
 * El tamaño fijo de la ventana "About Hikaru OpenLive..." NO se define acá.
 * Este archivo solo dibuja el CONTENIDO (logo, textos, links, footer).
 *
 * El tamaño de la ventana/viewport se define en:
 *   -> crates/hikaru_gui/src/app.rs:840-845 aprox.
 *   -> dentro de `ctx.show_viewport_immediate(ViewportId::from_hash_of("hikaru_about_viewport"), ...)`
 *   -> con `ViewportBuilder::default().with_inner_size([ANCHO, ALTO])`
 *
 * Valor actual: [520.0, 420.0] (con min/max iguales para que sea fija y no resizable)
 * Si ves que el texto se corta (como en tu screenshot), aumentá ese valor acá y en app.rs.
 * ---------------------------------------------------------
 */

use egui::{include_image, CursorIcon, Hyperlink, Image, Label, RichText, Ui, vec2};
use crate::app::AppMode;

// Constantes personalizables para el build
const BUILDER_NAME: &str = "Hikaru Corporation";
const BUILD_VERSION: &str = "1.11.1";

pub fn show(ui: &mut Ui, mode: AppMode) {
    // El `ui` que recibimos ya está limitado al tamaño definido en app.rs
    // (520x420). Todo lo que agregues acá adentro debe entrar en ese alto,
    // o va a quedar cortado / con scroll invisible.
    ui.vertical_centered(|ui| {
        ui.add_space(10.0);

        // Logo SVG "Energy Core" embebido desde la ruta del workspace
        // FIX crash wgpu: antes usaba .png de 10000x10000 que excedía el límite de 8192px de wgpu.
        // Ahora usa .svg (vectorial) que se rasteriza a 64x64 sin crear textura gigante.
        ui.add(
            Image::new(include_image!("../../../../assets/logos/hikaru_energy_core_2.svg"))
                .fit_to_exact_size(vec2(64.0, 64.0))
        );

        ui.add_space(8.0);

        // Título e Identificador del DAW
        match mode {
            AppMode::OpenLive => {
                ui.heading("Hikaru OpenLive");
            }
            AppMode::OpenStudio => {
                ui.heading("Hikaru OpenStudio");
            }
        }

        ui.add_space(8.0);
        ui.label("An Advanced Digital Audio Workstation for UNIX sysadmins.");
        ui.add_space(6.0);
        ui.label("Copyright © Hikaru Corporation - 2026");

        ui.add_space(12.0);
        ui.separator();
        ui.add_space(8.0);

        ui.label("This is a free software protected by GNU AGPLv3:");
        ui.add_space(6.0);

        // Link a la licencia AGPLv3
        ui.add(Hyperlink::from_label_and_url(
            "GNU AGPLv3 License",
            "https://www.gnu.org/licenses/agpl-3.0.en.html"
        ));

        ui.add_space(12.0);
        ui.separator();
        ui.add_space(8.0);

        // Nota sobre plataformas
        ui.strong("Coming Soon");
        ui.add_space(4.0);
        // OJO: este texto es muy largo. Si el ancho de ventana en app.rs es chico (ej. 380px)
        // se envuelve en 3-4 líneas y se corta por alto. Con 520px entra en 2 líneas.
        ui.label("Available on Windows, Linux, BSD distros like Free/Open/NetBSD, microwaves, calculator, potatoes, your mother, apache helicopter, anything shit that run Rust, etc.");

        ui.add_space(10.0);
    });

    // Separador e info de compilación alineada a la izquierda al final
    // Este footer es el que se cortaba en tu screenshot con [380,300].
    ui.separator();
    ui.add_space(6.0);

    ui.with_layout(egui::Layout::left_to_right(egui::Align::LEFT), |ui| {
        let daw_name = match mode {
            AppMode::OpenLive => "Hikaru OpenLive",
            AppMode::OpenStudio => "Hikaru OpenStudio",
        };

        // Este footer necesita ~460px de ancho para entrar en una sola línea.
        // Con [380, 300] se cortaba. Con [520, 420] entra cómodo.
        let footer_text = format!("{} | Compiled by {} | Version {}", daw_name, BUILDER_NAME, BUILD_VERSION);

        // Label estática en gris sutil con cursor por defecto (flecha normal)
        ui.add(Label::new(RichText::new(footer_text).weak()))
            .on_hover_cursor(CursorIcon::Default);
    });
}
