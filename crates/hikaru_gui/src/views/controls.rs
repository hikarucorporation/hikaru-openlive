// Hikaru OpenLive - Controls (egui)
// GNU AGPLv3
// crates/hikaru_gui/src/views/controls.rs

//! Controles reutilizables del editor de plugins en egui.

use egui::{pos2, vec2, Button, Color32, RichText, Sense, Stroke, Ui};

/// Tamaño del dial, en píxeles.
const DIAL_SIZE: f32 = 26.0;

/// Color de los títulos de sección.
const SECTION_COLOR: Color32 = Color32::from_rgb(255, 122, 0);

/// Color de los rótulos secundarios.
const LABEL_COLOR: Color32 = Color32::from_rgb(140, 150, 165);

/// Tope de los valores que llegan a un dial o a un dibujo.
pub fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

/// Título de sección del panel.
pub fn section(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).strong().color(SECTION_COLOR));
}

/// Fila de parámetro: rótulo, `−`, valor, `+`.
pub fn param_row(
    ui: &mut Ui,
    _id: &str,
    label: &str,
    value: &str,
    mut decrement: impl FnMut(),
    mut increment: impl FnMut(),
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if ui.add(Button::new("-").small()).clicked() {
            decrement();
        }
        ui.add_sized([52.0, 18.0], egui::Label::new(RichText::new(value).size(11.0).strong()));
        if ui.add(Button::new("+").small()).clicked() {
            increment();
        }
        ui.label(RichText::new(label).size(11.0).color(LABEL_COLOR));
    });
}

/// Ángulo de la aguja del dial, en radianes, para un `fill` dado.
pub fn needle_angle(fill: f32) -> f32 {
    let fill = finite(fill).clamp(0.0, 1.0);
    (-135.0 + 270.0 * fill) * std::f32::consts::PI / 180.0
}

/// El dial: círculo y aguja dibujados en egui.
pub fn dial(ui: &mut Ui, fill: f32) {
    let (rect, _response) = ui.allocate_exact_size(vec2(DIAL_SIZE, DIAL_SIZE), Sense::hover());
    let painter = ui.painter_at(rect);

    let center = rect.center();
    let radius = (DIAL_SIZE / 2.0 - 1.0).max(1.0);

    // Fondo del dial
    painter.circle_filled(center, radius, Color32::from_rgb(0x1E, 0x21, 0x2B));
    painter.circle_stroke(center, radius, Stroke::new(1.0_f32, Color32::from_rgb(0x3A, 0x41, 0x52)));

    // Aguja
    let angle = needle_angle(fill);
    let needle_len = (radius - 3.0).max(0.0);
    let needle_end = pos2(center.x + needle_len * angle.cos(), center.y + needle_len * angle.sin());
    painter.line_segment([center, needle_end], Stroke::new(1.5_f32, Color32::from_rgb(0xFF, 0x6E, 0x00)));
}

/// Fila de perilla: dial, rótulo, valor y `−`/`+`.
pub fn knob_row(
    ui: &mut Ui,
    id: &str,
    label: &str,
    value: &str,
    fill: f32,
    decrement: impl FnMut(),
    increment: impl FnMut(),
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        dial(ui, fill);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(RichText::new(label).size(10.0).color(LABEL_COLOR));
            param_row(ui, id, "", value, decrement, increment);
        });
    });
}

/// Escala de tiempos de la envolvente, en milisegundos.
pub const ENVELOPE_MAX_MS: f32 = 2000.0;

/// Fracción del ancho que se reserva para el tramo de sustain.
const SUSTAIN_SHARE: f32 = 0.4;

/// Dibujo de la envolvente ADSR con Painter.
pub fn envelope_display(ui: &mut Ui, attack: f32, decay: f32, sustain: f32, release: f32) {
    let attack = finite(attack).clamp(0.0, ENVELOPE_MAX_MS) / ENVELOPE_MAX_MS;
    let decay = finite(decay).clamp(0.0, ENVELOPE_MAX_MS) / ENVELOPE_MAX_MS;
    let sustain = finite(sustain).clamp(0.0, 1.0);
    let release = finite(release).clamp(0.0, ENVELOPE_MAX_MS) / ENVELOPE_MAX_MS;

    let available_w = ui.available_width().max(40.0);
    let (rect, _response) = ui.allocate_exact_size(vec2(available_w, 34.0), Sense::hover());
    let painter = ui.painter_at(rect);

    let baseline = rect.top() + rect.height() * 0.92;
    let width = rect.width();
    let height = rect.height();

    let x_of = |t: f32| rect.left() + width * t;
    let y_of = |level: f32| baseline - height * 0.85 * level;

    let flexible = 1.0 - SUSTAIN_SHARE;
    let attack_end = flexible * attack;
    let decay_end = attack_end + flexible * decay;
    let release_end = decay_end + SUSTAIN_SHARE + flexible * release * 0.5;
    let sustain_level = y_of(sustain);

    // Eje base
    painter.line_segment(
        [pos2(x_of(0.0), baseline), pos2(x_of(1.0), baseline)],
        Stroke::new(1.0_f32, Color32::from_rgb(0x2A, 0x2E, 0x3A)),
    );

    // Curva ADSR
    let points = vec![
        pos2(x_of(0.0), baseline),
        pos2(x_of(attack_end), y_of(1.0)),
        pos2(x_of(decay_end), sustain_level),
        pos2(x_of(release_end), sustain_level),
        pos2(x_of(1.0), baseline),
    ];

    for i in 0..points.len() - 1 {
        painter.line_segment(
            [points[i], points[i + 1]],
            Stroke::new(1.4_f32, Color32::from_rgb(0x00, 0xFF, 0xC8)),
        );
    }
}
