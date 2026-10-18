// crates/hikaru_gui/src/views/controls.rs

//! Controles reutilizables del editor de plugins.
//!
//! Los dos son deliberadamente aburridos: rótulo, valor y un par de botones
//! `−`/`+`. La app no tiene un knob con drag en ningún otro lado, y meter un
//! control nuevo sólo en el editor haría que el panel se sienta de otra GUI que
//! el resto del DAW. Lo que aporta este módulo es la forma: los controles se
//! ven igual en el panel de Wavetable, en el de OpenDMS y en el mixer.
//!
//! El dial se dibuja con `canvas` y primitivas de `Window` (quads y paths), que
//! es lo que permite un círculo sin una textura: un quad con los cuatro radios
//! iguales a la mitad del lado ya es un disco.

use gpui_kit::component::button::{Button, ButtonRounded};
use gpui_kit::component::label::Label;
use gpui_kit::*;

/// Handler de un botón de ajuste.
///
/// Es exactamente el tipo que acepta [`Button::on_click`]. Se declara aparte
/// para que las firmas de [`param_row`] y [`knob_row`] no sean un muro de
/// genéricos.
pub type ClickHandler = dyn Fn(&ClickEvent, &mut Window, &mut App);

/// Tamaño del dial, en píxeles.
const DIAL_SIZE: f32 = 26.0;

/// Tope de los valores que llegan a un dial o a un dibujo.
///
/// Un `NaN` no se puede acotar con `clamp` (todas las comparaciones con `NaN` son
/// falsas), y sin esta guarda se propaga a las coordenadas del canvas: el
/// `PathBuilder` acepta el `NaN` y el draw dibuja nada, sin ningún error.
pub fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

/// Color de los títulos de sección.
const SECTION_COLOR: Hsla = Hsla {
    h: 0.075,
    s: 1.0,
    l: 0.52,
    a: 1.0,
};

/// Color de los rótulos secundarios.
const LABEL_COLOR: Hsla = Hsla { h: 0.62, s: 0.12, l: 0.58, a: 1.0 };

/// Título de sección del panel.
pub fn section(text: &str) -> AnyElement {
    Label::new(text.to_string())
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .text_color(SECTION_COLOR)
        .into_any_element()
}

/// Fila de parámetro: rótulo, `−`, valor, `+`.
///
/// Es el control para lo que no necesita un dial (el morph, los ejes de
/// cámara): son valores que se cambian de a pasos grandes y donde el dial sería
/// más clickeable que legible.
pub fn param_row(
    id: &str,
    label: &str,
    value: &str,
    decrement: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    increment: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    gpui_kit::component::h_flex()
        .items_center()
        .gap(px(2.0))
        .child(
            Button::new(format!("{id}_dec"))
                .rounded(ButtonRounded::None)
                .label("-")
                .compact()
                .on_click(decrement),
        )
        .child(
            Label::new(value.to_string())
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .w(px(52.0)),
        )
        .child(
            Button::new(format!("{id}_inc"))
                .rounded(ButtonRounded::None)
                .label("+")
                .compact()
                .on_click(increment),
        )
        .child(Label::new(label.to_string()).text_xs().text_color(LABEL_COLOR))
        .into_any_element()
}

/// Fila de perilla: dial, rótulo, valor y `−`/`+`.
pub fn knob_row(
    id: &str,
    label: &str,
    value: &str,
    fill: f32,
    decrement: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    increment: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    gpui_kit::component::h_flex()
        .id(id.to_string())
        .items_center()
        .gap(px(4.0))
        .child(dial(fill))
        .child(
            gpui_kit::component::v_flex()
                .flex_1()
                .gap(px(1.0))
                .child(Label::new(label.to_string()).text_xs().text_color(LABEL_COLOR))
                .child(
                    gpui_kit::component::h_flex()
                        .items_center()
                        .gap(px(2.0))
                        .child(
                            Button::new(format!("{id}_dec"))
                                .rounded(ButtonRounded::None)
                                .label("-")
                                .compact()
                                .on_click(decrement),
                        )
                        .child(
                            Label::new(value.to_string())
                                .text_xs()
                                .font_weight(FontWeight::BOLD),
                        )
                        .child(
                            Button::new(format!("{id}_inc"))
                                .rounded(ButtonRounded::None)
                                .label("+")
                                .compact()
                                .on_click(increment),
                        ),
                ),
        )
        .into_any_element()
}

/// Ángulo de la aguja del dial, en radianes, para un `fill` dado.
///
/// Vive aparte de [`dial`] porque es la única parte con regla: el resto es
/// dibujo. El rango es de -135° a +135°, los 270° del arco habitual de un dial,
/// con el hueco de abajo como "mínimo".
pub fn needle_angle(fill: f32) -> f32 {
    let fill = finite(fill).clamp(0.0, 1.0);
    (-135.0 + 270.0 * fill) * std::f32::consts::PI / 180.0
}

/// El dial: círculo y aguja.
///
/// `fill` es la posición normalizada del valor, 0..1. Se acota acá y no en el
/// llamador porque un valor fuera de rango no da ningún error: la aguja
/// simplemente sale del disco y el arco dibuja más de una vuelta, que es un
/// bug puramente visual.
fn dial(fill: f32) -> AnyElement {
    let fill = finite(fill).clamp(0.0, 1.0);

    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            // Todo el cálculo va en `f32` y se convierte recién al construir los
            // puntos: mezclar `Pixels` con `f32` obliga a convertir en cada
            // operación, y `Pixels` no tiene constructor público.
            let left = bounds.origin.x.as_f32();
            let top = bounds.origin.y.as_f32();
            let side = f32::min(bounds.size.width.as_f32(), bounds.size.height.as_f32());
            let center_x = left + side / 2.0;
            let center_y = top + side / 2.0;
            let radius = side / 2.0 - 1.0;

            // Un quad con los cuatro radios iguales a la mitad del lado es un
            // círculo: no hace falta una primitiva de arco.
            window.paint_quad(PaintQuad {
                bounds: Bounds::new(
                    point(px(center_x), px(center_y)),
                    gpui_kit::size(px(radius * 2.0), px(radius * 2.0)),
                ),
                background: rgb(0x1E212B).into(),
                border_color: rgb(0x3A4152).into(),
                corner_radii: Corners {
                    top_left: px(radius),
                    top_right: px(radius),
                    bottom_right: px(radius),
                    bottom_left: px(radius),
                },
                border_widths: gpui_kit::Edges { top: px(1.0), right: px(1.0), bottom: px(1.0), left: px(1.0) },
                border_style: BorderStyle::default(),
            });

            let angle = needle_angle(fill);
            let needle = (radius - 3.0).max(0.0);
            let mut path = PathBuilder::stroke(px(1.5));
            path.move_to(point(px(center_x), px(center_y)));
            path.line_to(point(
                px(center_x + needle * angle.cos()),
                px(center_y + needle * angle.sin()),
            ));
            if let Ok(path) = path.build() {
                window.paint_path(path, rgb(0xFF6E00));
            }
        },
    )
    .w(px(DIAL_SIZE))
    .h(px(DIAL_SIZE))
    .into_any_element()
}

/// Escala de tiempos de la envolvente, en milisegundos.
///
/// Fija, y no relativa al tramo más largo: si el attack sube a 2 s, con escala
/// relativa los otros tres tramos quedan aplastados contra la base y la
/// envolvente pierde su forma justo cuando el usuario la está editando.
pub const ENVELOPE_MAX_MS: f32 = 2000.0;

/// Fracción del ancho que se reserva para el tramo de sustain.
const SUSTAIN_SHARE: f32 = 0.4;

/// Dibujo de la envolvente ADSR.
///
/// Recibe los cuatro valores sueltos y no un struct: la envolvente del
/// sintetizador y la del sampler son tipos distintos con el mismo significado, y
/// un parámetro con cuatro `f32` los cubre a los dos sin que uno tenga que
/// depender del otro.
pub fn envelope_display(attack: f32, decay: f32, sustain: f32, release: f32) -> AnyElement {
    let attack = finite(attack).clamp(0.0, ENVELOPE_MAX_MS) / ENVELOPE_MAX_MS;
    let decay = finite(decay).clamp(0.0, ENVELOPE_MAX_MS) / ENVELOPE_MAX_MS;
    let sustain = finite(sustain).clamp(0.0, 1.0);
    let release = finite(release).clamp(0.0, ENVELOPE_MAX_MS) / ENVELOPE_MAX_MS;

    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            // Igual que en el dial: se calcula en `f32` y se convierte al
            // construir cada punto.
            let left = bounds.origin.x.as_f32();
            let top = bounds.origin.y.as_f32();
            let width = bounds.size.width.as_f32().max(1.0);
            let height = bounds.size.height.as_f32().max(1.0);
            let baseline = top + height * 0.92;

            let x_of = |t: f32| px(left + width * t);
            let y_of = |level: f32| px(baseline - height * 0.85 * level);

            // Los tramos de tiempo se reparten el ancho restante en partes
            // iguales; el sustain conserva su parte fija para que la envolvente
            // siga teniendo una meseta visible con ataque y decay en cero.
            let flexible = 1.0 - SUSTAIN_SHARE;
            let attack_end = flexible * attack;
            let decay_end = attack_end + flexible * decay;
            let release_end = decay_end + SUSTAIN_SHARE + flexible * release * 0.5;
            let sustain_level = y_of(sustain);

            let mut path = PathBuilder::stroke(px(1.4));
            path.move_to(point(x_of(0.0), px(baseline)));
            path.line_to(point(x_of(attack_end), y_of(1.0)));
            path.line_to(point(x_of(decay_end), sustain_level));
            path.line_to(point(x_of(release_end), sustain_level));
            path.line_to(point(x_of(1.0), px(baseline)));

            if let Ok(path) = path.build() {
                window.paint_path(path, rgb(0x00FFC8));
            }

            // La línea de base cierra la figura y da la referencia del 0.
            let mut axis = PathBuilder::stroke(px(1.0));
            axis.move_to(point(x_of(0.0), px(baseline)));
            axis.line_to(point(x_of(1.0), px(baseline)));
            if let Ok(axis) = axis.build() {
                window.paint_path(axis, rgb(0x2A2E3A));
            }
        },
    )
    .w_full()
    .h(px(34.0))
    .into_any_element()
}
