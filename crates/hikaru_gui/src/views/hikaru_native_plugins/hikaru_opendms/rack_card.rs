// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_opendms/rack_card.rs

//! La tarjeta compacta de **Hikaru OpenDMS** en la tira de módulos del rack.
//!
//! # Qué muestra
//!
//! El rack dibuja cada plugin en una tarjeta de ancho fijo. Para el sampler de
//! percusión, la tarjeta **no** muestra la grilla de pads: la grilla no entra
//! en 388px de ancho y la única información que interesa mientras se programa
//! es cuántos pads hay y cuántos tienen sample cargado. Por eso la tarjeta
//! muestra el nombre del sampler y el conteo `cargados/total`.
//!
//! # Por qué vive acá y no en `dsp_rack.rs`
//!
//! Antes esta función era un `fn` privado dentro de `views/dsp_rack.rs`, y era
//! el único pedazo de un plugin nativo incrustado en el archivo del rack. Con
//! el subsistema de plugins nativos, el rack debería conocer a los plugins
//! **sólo** para delegar: cada rama de su `match` es una llamada a un módulo de
//! `views::hikaru_native_plugins`, y el panel de cada plugin vive en la
//! carpeta del plugin.
//!
//! Ojo con el nombre: acá `render_dms_compact` sería ambiguo, porque
//! [`crate::views::hikaru_native_plugins::hikaru_opendms::open_dms::render_dms_compact`]
//! ya existe y es otra cosa (la grilla de pads, con hit-testing y drag & drop).
//! Ésta es la *tarjeta*, aquélla es la *grilla*.

use gpui_kit::component::label::Label;
use gpui_kit::*;

use crate::app::{state, HikaruApp};

/// Dibuja la tarjeta del sampler en la tira de módulos del rack.
///
/// Si el slot no tiene estado de DMS todavía (recién asignado, o la carga del
/// sample todavía en curso), muestra el placeholder en vez de un conteo que
/// mentiría con cero pads.
pub fn render_rack_card(cx: &mut Context<HikaruApp>, track_idx: usize, idx: usize) -> AnyElement {
    let app = state(cx).read(cx);
    let Some(dms) = app.slot(track_idx, idx).and_then(|slot| slot.dms_state.clone()) else {
        return div()
            .w_full()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .child(Label::new("No DMS state").text_xs().text_color(rgb(0x555555)))
            .into_any_element();
    };
    let pads = dms.pad_count;
    let loaded = dms.pads.iter().filter(|pad| pad.sample_path.is_some()).count();

    div()
        .w_full()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(4.0))
        .child(
            Label::new("OpenDMS Sampler")
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0x00FFC8)),
        )
        .child(
            Label::new(format!("{loaded}/{pads} pads"))
                .text_xs()
                .text_color(rgb(0x888888)),
        )
        .into_any_element()
}
