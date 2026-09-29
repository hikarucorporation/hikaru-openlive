// crates/hikaru_gui/src/views/dsp_rack.rs

//! Rack de DSP: la tira de slots y el editor extendido del slot abierto.
//!
//! # Cómo se abre un editor
//!
//! Clickear la tarjeta de un slot la selecciona **y** la abre; el botón `▣`/`□`
//! de la esquina es el que sólo hace toggle. Antes las dos cosas eran el mismo
//! botón (el número del slot) y el editor no se desplegaba nunca: `is_open` se
//! guardaba en el estado pero ninguna vista lo leía, así que la única forma de
//! llegar al panel era nonexistent.
//!
//! La tira y el editor se dibujan en la misma columna a propósito. Poner el
//! editor aparte, flotante o en otra ventana, hacía que el panel quedara debajo
//! del área de la matriz y fuera invisible con el rack de 200px de alto: el
//! `div` del rack no crece, así que cualquier hijo que exceda el alto queda
//! recortado. Ver [`RACK_HEIGHT_OPENED`].
//!
//! Los dos editores (Wavetable y OpenDMS) son tiras bajas y anchas, no racks de
//! perillas: el plugin se abre a la altura de una tarjeta, no media pantalla.

use gpui_kit::component::*;
use gpui_kit::component::button::{Button, ButtonRounded};
use gpui_kit::component::label::Label;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, HikaruApp};
use crate::views::mixer::DspSlot;
use crate::views::{open_dms, open_wavetable};

/// Padding del contenedor del rack, en cada lado.
///
/// Lo usa el `p()` de [`render`], y entra en el cálculo del alto: si el
/// contenedor mide menos de lo que suman sus hijos, el editor queda recortado
/// por abajo y se ve cortado sin error.
const RACK_PADDING: f32 = 8.0;

/// Separación entre los hijos del `v_flex` del rack: entre el título y la tira,
/// y entre la tira y el editor.
const RACK_GAP: f32 = 8.0;

/// Alto de la fila del título.
const TITLE_HEIGHT: f32 = 20.0;

/// Alto de la tira de slots, que es la banda de tarjetas.
///
/// El peor caso es la tarjeta de OpenDMS, que es la única con columnas; el resto
/// son tarjetas de una fila.
const SLOT_STRIP_HEIGHT: f32 = 220.0;

/// Alto del rack con un editor desplegado.
///
/// Es la suma real de las piezas del `v_flex` de [`render`]: padding por arriba y
/// por abajo, el título, dos separaciones, la tira de slots y el editor.
///
/// Se deriva de los valores que el layout usa de verdad en vez de estar escrita
/// a mano, porque las dos formas de equivocarse acá son silenciosas. Si la
/// constante se queda corta, el editor se ve recortado por abajo; si se pasa, el
/// rack deja un hueco muerto entre la tira y el fondo. La primera vez que se
/// cambió el alto del editor de 168 a 132, esta constante seguía con el 24 viejo
/// y el editor se salía del `div` (ver la nota de módulo, sobre por qué el `div`
/// no crece solo).
pub const RACK_HEIGHT_OPENED: f32 = 2.0 * RACK_PADDING
    + TITLE_HEIGHT
    + 2.0 * RACK_GAP
    + SLOT_STRIP_HEIGHT
    + open_wavetable::EDITOR_HEIGHT;

/// Alto del rack con la tira de slots solamente.
pub const RACK_HEIGHT_CLOSED: f32 = 200.0;

/// Ancho de la tarjeta de cada slot, por plugin.
pub fn card_width(name: &str) -> f32 {
    match name {
        "OpenWavetable" => 240.0,
        "Hikaru OpenDMS" => 540.0,
        "OpenSpectralFX" => 200.0,
        "Empty Slot" => 170.0,
        _ => 190.0,
    }
}

/// Alto de la tarjeta del slot.
pub fn card_height(name: &str) -> f32 {
    if name == "Hikaru OpenDMS" { 220.0 } else { 100.0 }
}

/// Rack completo: tira de slots y, debajo, el editor del slot abierto.
pub fn render(cx: &mut Context<HikaruApp>) -> impl IntoElement {
    let (track_idx, selected_slot, effects, dragged_sample, audio_proxy, title, open_editor) = {
        let app = state(cx).read(cx);
        let track_idx = app.safe_track_index();
        let selected_slot = app.selected_slot_index;
        if !app.tracks().get(track_idx).is_some() {
            return v_flex().size_full().into_any_element();
        }
        let track = &app.tracks()[track_idx];

        (
            track_idx,
            selected_slot,
            track.effects.clone(),
            app.dragged_sample.clone(),
            app.audio_proxy.clone(),
            rack_title(&track.name, app.matrix_state.selected_slot.map(|(_, scene)| scene)),
            editor_for(&track.effects, selected_slot).map(str::to_string),
        )
    };

    v_flex()
        .id("dsp_rack")
        .size_full()
        .bg(rgb(0x14141A))
        .p(px(RACK_PADDING))
        .gap(px(RACK_GAP))
        .child(Label::new(title).text_sm().font_weight(FontWeight::BOLD))
        .child(render_strip(cx, track_idx, selected_slot, &effects))
        // El editor va debajo de la tira, en un `h_flex` para que quede alineado
        // a la izquierda y con su ancho propio.
        //
        // El `h_flex` es lo que evita que el módulo se estire a lo ancho de la
        // pantalla: un hijo directo del `v_flex` con `flex_1` toma todo el ancho
        // disponible, y el editor es un bloque cuadrado que tiene que mantener su
        // proporción para entrar en la cadena de módulos.
        .when_some(open_editor, |this, plugin| {
            this.child(
                h_flex()
                    .w_full()
                    .flex_1()
                    .min_h(px(0.0))
                    .items_start()
                    .gap(px(RACK_GAP))
                    .child(
                        div()
                            .overflow_hidden()
                            .child(match plugin.as_str() {
                                "Hikaru OpenDMS" => render_dms_editor(
                                    cx,
                                    track_idx,
                                    selected_slot,
                                    &dragged_sample,
                                    &audio_proxy,
                                ),
                                _ => open_wavetable::render(cx, track_idx, selected_slot),
                            }),
                    ),
            )
        })
        .into_any_element()
}

/// La tira de slots: botones de add/remove y las tarjetas.
fn render_strip(
    cx: &mut Context<HikaruApp>,
    track_idx: usize,
    selected_slot: usize,
    effects: &[DspSlot],
) -> AnyElement {
    let _ = cx;

    h_flex()
        .id("dsp_strip")
        .w_full()
        .items_start()
        .gap(px(8.0))
        .child(
            v_flex()
                .w(px(150.0))
                .gap(px(4.0))
                .child(Label::new("SLOTS").text_xs().font_weight(FontWeight::BOLD))
                .child(
                    Button::new("dsp_add_slot")
                        .rounded(ButtonRounded::None)
                        .label(" [ + ] Add Slot ")
                        .compact()
                        .on_click(move |_, _, cx| {
                            state(cx).update(cx, |state, cx| {
                                let track_idx = state.safe_track_index();
                                if let Some(track) = state.tracks_mut().get_mut(track_idx) {
                                    let new_id = track.effects.len();
                                    track.effects.push(DspSlot::new(new_id, "Empty Slot".to_string()));
                                    state.selected_slot_index = new_id;
                                }
                                cx.notify();
                            });
                        }),
                )
                .child(
                    Button::new("dsp_remove_slot")
                        .rounded(ButtonRounded::None)
                        .label(" [ - ] Remove ")
                        .compact()
                        .on_click(move |_, _, cx| {
                            state(cx).update(cx, |state, cx| {
                                let track_idx = state.safe_track_index();
                                let selected = state.selected_slot_index;
                                if let Some(track) = state.tracks_mut().get_mut(track_idx) {
                                    track.effects.pop();
                                    // La selección no puede quedar apuntando al
                                    // slot que se acaba de borrar: el rack
                                    // dibuja las tarjetas de `effects` y un
                                    // índice fuera de rango deja la tira vacía.
                                    state.selected_slot_index = if track.effects.is_empty() {
                                        0
                                    } else {
                                        selected.min(track.effects.len() - 1)
                                    };
                                }
                                cx.notify();
                            });
                        }),
                ),
        )
        .child(
            h_flex()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(6.0))
                .overflow_x_scrollbar()
                .children(effects.iter().enumerate().map(|(idx, slot)| {
                    render_card(cx, track_idx, idx, slot, idx == selected_slot)
                })),
        )
        .into_any_element()
}

/// Una tarjeta de slot.
///
/// La tarjeta entera es clickeable y abre el editor; los tres botones de la
/// esquina (número, activo, abrir) quedan por encima y conservan su
/// comportamiento puntual.
fn render_card(
    cx: &mut Context<HikaruApp>,
    track_idx: usize,
    idx: usize,
    slot: &DspSlot,
    is_selected: bool,
) -> AnyElement {
    let name = slot.name.clone();
    let slot_active = slot.active;
    let slot_open = slot.is_open;
    let can_open = slot.has_editor();
    let width = card_width(&name);
    let height = card_height(&name);

    v_flex()
        .w(px(width))
        .h(px(height))
        .bg(if is_selected { rgb(0x28282D) } else { rgb(0x19191C) })
        .border_1()
        .border_color(if is_selected { rgb(0xFF6E00) } else { rgb(0x323232) })
        .rounded(px(4.0))
        .p(px(6.0))
        .gap(px(4.0))
        .id(format!("dsp_card_{track_idx}_{idx}"))
        // Click en cualquier parte de la tarjeta: seleccionar y abrir. Es el
        // gesto que el usuario hace en un rack real y el que antes no existía.
        .when(can_open, |this| {
            this.on_click(move |_, _, cx| {
                state(cx).update(cx, |state, cx| {
                    state.selected_slot_index = idx;
                    if let Some(slot) = state.slot_mut(track_idx, idx) {
                        slot.is_open = true;
                    }
                    cx.notify();
                });
            })
        })
        .child(
            h_flex()
                .items_center()
                .gap(px(4.0))
                .child(
                    Button::new(format!("dsp_slot_num_{track_idx}_{idx}"))
                        .rounded(ButtonRounded::None)
                        .label(format!("{:02}", idx + 1))
                        .compact()
                        .on_click(move |_, _, cx| {
                            state(cx).update(cx, |state, cx| {
                                state.selected_slot_index = idx;
                                cx.notify();
                            });
                        }),
                )
                .child(Label::new(name.clone()).text_xs())
                .child(div().flex_1())
                .child(
                    Button::new(format!("dsp_slot_active_{track_idx}_{idx}"))
                        .rounded(ButtonRounded::None)
                        .label(if slot_active { "●" } else { "○" })
                        .compact()
                        .on_click(move |_, _, cx| {
                            state(cx).update(cx, |state, cx| {
                                if let Some(slot) = state.slot_mut(track_idx, idx) {
                                    slot.active = !slot.active;
                                }
                                cx.notify();
                            });
                        }),
                )
                .child(
                    Button::new(format!("dsp_slot_open_{track_idx}_{idx}"))
                        .rounded(ButtonRounded::None)
                        // Un slot sin editor no tiene nada que abrir: mostrar el
                        // botón ahí sería una promesa que el panel no cumple.
                        .label(if !can_open {
                            "-"
                        } else if slot_open {
                            "▣"
                        } else {
                            "□"
                        })
                        .compact()
                        .on_click(move |_, _, cx| {
                            state(cx).update(cx, |state, cx| {
                                state.selected_slot_index = idx;
                                if let Some(slot) = state.slot_mut(track_idx, idx) {
                                    slot.is_open = !slot.is_open;
                                }
                                cx.notify();
                            });
                        }),
                ),
        )
        .child(match name.as_str() {
            "OpenWavetable" => v_flex()
                .gap(px(2.0))
                .child(Label::new("Wavetable Synth").text_xs())
                .child(Label::new(if can_open { "click para abrir" } else { "sin editor" }).text_xs().text_color(rgb(0x6A7080)))
                .into_any_element(),
            "Hikaru OpenDMS" => render_dms_summary(cx, track_idx, idx),
            "OpenSpectralFX" => v_flex()
                .gap(px(2.0))
                .child(Label::new("Spectral Processor").text_xs())
                .child(Label::new("FFT Size / Mix").text_xs())
                .into_any_element(),
            _ => v_flex()
                .gap(px(2.0))
                .child(Label::new("Empty Slot").text_xs())
                .child(
                    Button::new(format!("dsp_empty_select_{track_idx}_{idx}"))
                        .rounded(ButtonRounded::None)
                        .label("Select Plugin")
                        .compact()
                        .on_click(move |_, _, cx| {
                            state(cx).update(cx, |state, cx| {
                                if let Some(slot) = state.slot_mut(track_idx, idx) {
                                    slot.name = "OpenWavetable".to_string();
                                    // Cargar un plugin en un slot lo deja
                                    // seleccionado y abierto: si no, asignarlo
                                    // parece que no hizo nada.
                                    slot.is_open = true;
                                }
                                state.selected_slot_index = idx;
                                cx.notify();
                            });
                        }),
                )
                .into_any_element(),
        })
        .into_any_element()
}

/// Resumen del sampler dentro de la tarjeta: sólo los pads.
///
/// El resumen es una copia del estado y todos los handlers vuelven a leer del
/// estado, así que no puede quedar desincronizado con la lista real.
fn render_dms_summary(cx: &mut Context<HikaruApp>, track_idx: usize, idx: usize) -> AnyElement {
    let _ = cx;
    let app = state(cx).read(cx);
    let Some(dms) = app.slot(track_idx, idx).and_then(|slot| slot.dms_state.clone()) else {
        return Label::new("No DMS state").text_xs().into_any_element();
    };
    let pads = dms.pad_count;
    let loaded = dms.pads.iter().filter(|pad| pad.sample_path.is_some()).count();

    v_flex()
        .gap(px(2.0))
        .child(Label::new("OpenDMS Sampler").text_xs().font_weight(FontWeight::BOLD).text_color(rgb(0x00FFC8)))
        .child(Label::new(format!("{loaded}/{pads} pads")).text_xs())
        .into_any_element()
}

/// Editor extendido de OpenDMS, o su resumen si el slot no tiene estado.
///
/// Igual que el de Wavetable, el editor real vive en [`open_dms`] y se dibuja
/// en la misma columna que la tira.
fn render_dms_editor(
    cx: &mut Context<HikaruApp>,
    track_idx: usize,
    slot_idx: usize,
    dragged_sample: &Option<std::path::PathBuf>,
    audio_proxy: &crate::audio_proxy::AudioProxy,
) -> AnyElement {
    let has_state = {
        let app = state(cx).read(cx);
        app.slot(track_idx, slot_idx).and_then(|slot| slot.dms_state.as_ref()).is_some()
    };

    if !has_state {
        // El estado se crea la primera vez que se abre el editor: un `DmsSlot`
        // recién hecho no lo tiene, y sin esto el panel aparecería vacío para
        // siempre porque no hay ningún otro camino que lo inicialice.
        state(cx).update(cx, |state, cx| {
            if let Some(slot) = state.slot_mut(track_idx, slot_idx) {
                if slot.dms_state.is_none() {
                    slot.dms_state = Some(open_dms::OpenDms::default());
                }
            }
            cx.notify();
        });
        return v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .child(Label::new("Inicializando sampler...").text_xs())
            .into_any_element();
    }

    open_dms::render_editor(cx, track_idx, slot_idx, dragged_sample, audio_proxy)
}

/// ¿Debería el rack mostrar el editor de `slot_idx`?
///
/// Es una función pura sobre la lista de slots a propósito: el layout raíz la
/// consulta en cada frame para decidir el alto del panel, y el layout no
/// necesita ni puede tomar un lock del estado para eso. Al ser pura, el caso
/// interesante (selección fuera de rango, slot vacío, selección corrida) se
/// testea sin levantar la app.
pub fn editor_for(effects: &[DspSlot], slot_idx: usize) -> Option<&'static str> {
    let slot = effects.get(slot_idx)?;
    if !slot.is_open || !slot.has_editor() {
        return None;
    }
    // El nombre del plugin decide el editor. Se compara por referencia a
    // estáticos porque sólo hay dos plugins con panel.
    match slot.name.as_str() {
        "OpenWavetable" => Some("OpenWavetable"),
        "Hikaru OpenDMS" => Some("Hikaru OpenDMS"),
        _ => None,
    }
}

/// Título del rack.
pub fn rack_title(track_name: &str, scene: Option<usize>) -> String {
    match scene {
        Some(scene) => format!("DSP RACK: {track_name} | Scene {}", scene + 1),
        None => format!("DSP RACK: {track_name}"),
    }
}
