// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_openeq3/mod.rs

//! **OpenEQ3**: el ecualizador de tres bandas.
//!
//! # Estado actual
//!
//! OpenEQ3 está en el catálogo de plugins del rack (`views::dsp_rack::PLUGIN_CATALOG`,
//! categoría "Efectos") y su tarjeta se dibuja, pero el panel **todavía no
//! tiene controles**: la tarjeta muestra el nombre del plugin y el rótulo de
//! las tres bandas (`Low | Mid | Hi`).
//!
//! Está en su propia carpeta igual que los otros dos plugins nativos, y no
//! metido dentro de `dsp_rack.rs`, por una razón que no es de tamaño sino de
//! **coherencia**: el `match` del rack que decide qué panel se dibuja es el
//! único punto del crate que debería conocer a los plugins, y cada rama de ese
//! `match` debería ser una llamada a un módulo. Con OpenEQ3 en línea, ese `match`
//! tenía una rama que era un `v_flex` entero, y cualquier cambio en el panel
//! del ecualizador tocaba el archivo del rack.
//!
//! Cuando la tarjeta crezca, este es el lugar donde va: los knobs de las tres
//! bandas, el bypass, y el enlace con el nodo de DSP que lo procesa.
//!
//! # Por qué no hay `views/controls` acá
//!
//! El ecualizador va a necesitar sliders y curvas de respuesta, que no es lo
//! mismo que los knobs de envolvente de los instrumentos. Cuando llegue, sus
//! widgets van a ser propios de este módulo, no genéricos de `views::controls`.

use gpui_kit::component::label::Label;
use gpui_kit::component::v_flex;
use gpui_kit::*;

/// Dibuja la tarjeta del plugin para la tira de módulos del rack.
///
/// No recibe estado del slot porque hoy el panel es informativo: OpenEQ3 no
/// tiene controles, así que no hay nada que leer de la app ni que escribir
/// atrás. En el momento en que aparezcan, esta función pasa a tomar
/// `cx`/`track_idx`/`idx` como el resto de los natives.
pub fn render_module() -> AnyElement {
    v_flex()
        .w_full()
        .flex_1()
        .min_h(px(100.0))
        .items_center()
        .justify_center()
        .gap(px(2.0))
        .child(
            Label::new("OpenEQ3")
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0xE8EAF0)),
        )
        .child(
            Label::new("Low | Mid | Hi")
                .text_xs()
                .text_color(rgb(0x888888)),
        )
        .into_any_element()
}
