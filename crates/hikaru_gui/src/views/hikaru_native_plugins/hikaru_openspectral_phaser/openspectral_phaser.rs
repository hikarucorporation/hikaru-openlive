// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_openspectral_phaser/openspectral_phaser.rs

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use gpui_kit::component::button::{Button, ButtonRounded};
use gpui_kit::component::label::Label;
use gpui_kit::component::v_flex;
use gpui_kit::*;

use crate::app::{state, HikaruApp};

const PARAM_NAMES: [&str; 11] = [
    "Width", "Offset", "Depth", "Speed", "Mix", "Blur", "Hz/Oct", "1x/5x",
    "Keep Bass", "Blocksize", "Bypass",
];

const DEFAULTS: [f32; 11] = [0.5, 0.0, 0.0, 0.5, 1.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0];

type StateMap = Mutex<HashMap<(usize, usize), [f32; 11]>>;

fn state_map() -> &'static StateMap {
    static CELL: OnceLock<StateMap> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_params(track_idx: usize, slot_idx: usize) -> [f32; 11] {
    let mut map = state_map().lock().unwrap();
    *map.entry((track_idx, slot_idx)).or_insert(DEFAULTS)
}

fn set_param(track_idx: usize, slot_idx: usize, idx: usize, value: f32) {
    let mut map = state_map().lock().unwrap();
    let entry = map.entry((track_idx, slot_idx)).or_insert(DEFAULTS);
    if idx < 11 {
        entry[idx] = value.clamp(0.0, 1.0);
    }
}

/// Estado observable del phaser (reexportado para compatibilidad).
#[derive(Clone, Debug)]
pub struct OpenSpectralPhaserState {
    pub params: [f32; 11],
}

impl Default for OpenSpectralPhaserState {
    fn default() -> Self {
        Self { params: DEFAULTS }
    }
}

impl OpenSpectralPhaserState {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Dibuja la tarjeta del plugin en el rack.
pub fn render_module(cx: &mut Context<HikaruApp>, track_idx: usize, slot_idx: usize) -> AnyElement {
    let params = get_params(track_idx, slot_idx);

    // Visualización de espectro/ondas simplificada: barras según parámetros.
    let mut spectrum = gpui_kit::component::h_flex()
        .w_full()
        .h(px(24.0))
        .items_end()
        .gap(px(1.0));
    for i in 0..16 {
        let h = 4.0 + (params[2] * 18.0) + (i as f32 * 0.7).sin().abs() * 6.0;
        spectrum = spectrum.child(
            div()
                .w(px(4.0))
                .h(px(h))
                .bg(rgb(0xFF6E00)),
        );
    }

    let mut root = v_flex()
        .w_full()
        .flex_1()
        .min_h(px(100.0))
        .items_center()
        .justify_center()
        .gap(px(4.0))
        .child(
            Label::new("OpenSpectral Phaser")
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0xE8EAF0)),
        )
        .child(spectrum);

    for i in 0..11 {
        let value = params[i];
        root = root.child(
            gpui_kit::component::h_flex()
                .w_full()
                .items_center()
                .gap(px(2.0))
                .child(
                    Label::new(PARAM_NAMES[i].to_string())
                        .text_xs()
                        .w(px(70.0))
                        .text_color(rgb(0xAAAAAA)),
                )
                .child(
                    Button::new(format!("osp_dec_{track_idx}_{slot_idx}_{i}"))
                        .rounded(ButtonRounded::None)
                        .label("-")
                        .compact()
                        .on_click(|_, _, _| {}),
                )
                .child(
                    Label::new(format!("{:.2}", value))
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .w(px(40.0)),
                )
                .child(
                    Button::new(format!("osp_inc_{track_idx}_{slot_idx}_{i}"))
                        .rounded(ButtonRounded::None)
                        .label("+")
                        .compact()
                        .on_click(|_, _, _| {}),
                ),
        );
    }

    root.into_any_element()
}

/// Variante sin estado de slot, para compatibilidad.
pub fn render_module_simple() -> AnyElement {
    v_flex()
        .w_full()
        .flex_1()
        .min_h(px(100.0))
        .items_center()
        .justify_center()
        .child(
            Label::new("OpenSpectral Phaser")
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0xE8EAF0)),
        )
        .into_any_element()
}

// Aliases comunes.
pub use crate::views::hikaru_native_plugins::hikaru_openspectral_phaser::OpenSpectralPhaserState as OpenSpectralPhaser;
// Reexport del DSP para conveniencia.
pub use hikaru_dsp::effects::openspectral_phaser::*;