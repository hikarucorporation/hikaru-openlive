use std::f32::consts::PI;

use gpui_kit::component::*;
use gpui_kit::component::button::Button;
use gpui_kit::component::label::Label;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, HikaruApp};

const GRID_SIZE: f32 = 20.0;

#[derive(Clone, Debug)]
pub enum EffectModule {
    WarpBend(f32),
    WarpSync(f32),
    WarpPW(f32),
    WarpAsymmetry(f32),
    ModRing(f32),
    ModFM(f32),
    ModAM(f32),
    CrossfadeSmooth(f32),
}

impl EffectModule {
    pub fn name_and_range(&mut self) -> (&'static str, &mut f32, f32, f32) {
        match self {
            EffectModule::WarpBend(val) => ("Bend", val, -100.0, 100.0),
            EffectModule::WarpSync(val) => ("Sync", val, 0.0, 100.0),
            EffectModule::WarpPW(val) => ("PW", val, -50.0, 50.0),
            EffectModule::WarpAsymmetry(val) => ("Asym", val, -100.0, 100.0),
            EffectModule::ModRing(val) => ("Ring", val, 0.0, 100.0),
            EffectModule::ModFM(val) => ("FM", val, 0.0, 100.0),
            EffectModule::ModAM(val) => ("AM", val, 0.0, 100.0),
            EffectModule::CrossfadeSmooth(val) => ("Smooth", val, 0.0, 100.0),
        }
    }
}

#[derive(Clone, Debug)]
pub struct WavetableOscillator {
    pub id: usize,
    pub name: String,
    pub wt_pos: f32,
    pub pos: Point<Pixels>,
    pub size: Point<Pixels>,
    pub colors: Vec<Hsla>,
}

impl WavetableOscillator {
    pub fn new(id: usize, name: &str, initial_pos: Point<Pixels>) -> Self {
        Self {
            id,
            name: name.to_string(),
            wt_pos: 0.0,
            pos: initial_pos,
            size: point(px(320.0), px(200.0)),
            colors: vec![rgb(0x00FFFF).into(), rgb(0xFF00B4).into()],
        }
    }
}

#[derive(Clone, Debug)]
pub struct ModulatorNode {
    pub id: usize,
    pub target_osc_id: usize,
    pub effect: EffectModule,
    pub pos: Point<Pixels>,
}

impl ModulatorNode {
    pub fn new(id: usize, target_osc_id: usize, effect: EffectModule, pos: Point<Pixels>) -> Self {
        Self {
            id,
            target_osc_id,
            effect,
            pos,
        }
    }
}

fn snap_to_grid(val: f32, grid_size: f32) -> f32 {
    (val / grid_size).round() * grid_size
}

pub fn render(
    cx: &mut Context<HikaruApp>,
    oscillators: &mut Vec<WavetableOscillator>,
    modulators: &mut Vec<ModulatorNode>,
    cam_x: &mut f32,
    cam_y: &mut f32,
    cam_z: &mut f32,
) -> AnyElement {
    let oscs = oscillators.clone();
    let mods = modulators.clone();
    let cx_val = *cam_x;
    let cy_val = *cam_y;
    let cz_val = *cam_z;

    let mut osc_elems: Vec<AnyElement> = Vec::new();
    for osc in &oscs {
        let osc_id = osc.id;
        let name = osc.name.clone();
        let wt_pos = osc.wt_pos;
        let colors = osc.colors.clone();
        osc_elems.push(
            v_flex()
                .w(px(320.0))
                .h(px(200.0))
                .bg(rgb(0x12141A))
                .border_1()
                .border_color(rgb(0x2D3241))
                .rounded(px(6.0))
                .p(px(6.0))
                .gap(px(4.0))
                .child(Label::new(name.clone()).text_xs())
                .child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            let cx0 = bounds.origin.x;
                            let cy = bounds.origin.y + px(bounds.size.height.as_f32() / 2.0 - (cy_val * 0.4) as f32);
                            let total_frames = 256.0;
                            let rendered = 24;
                            let wave_w = bounds.size.width.as_f32() * 0.7;
                            let depth = 90.0;
                            let h_scale = 32.0;
                            let cos_y = cz_val.cos();
                            let sin_y = cz_val.sin();
                            let cos_p = cx_val.cos();
                            let sin_p = cx_val.sin();
                            let active = ((wt_pos / (total_frames - 1.0)) * (rendered - 1) as f32).round() as usize;
                            let draw_order: Vec<usize> = if cos_y >= 0.0 {
                                (0..rendered).rev().collect()
                            } else {
                                (0..rendered).collect()
                            };
                            for &z in &draw_order {
                                let zf = z as f32;
                                let morph = zf / (rendered - 1) as f32;
                                let is_sel = z == active;
                                let mut pts: Vec<Point<Pixels>> = Vec::new();
                                for i in 0..=32 {
                                    let nx = i as f32 / 32.0;
                                    let phase = nx * PI * 2.0;
                                    let wave = phase.sin() * (1.0 - morph)
                                        + (phase * 3.0).sin() * 0.3 * (morph * PI).sin()
                                        + (phase * 5.0).sin() * 0.2 * morph;
                                    let xl = (nx - 0.5) * wave_w;
                                    let zl = (zf / (rendered - 1) as f32 - 0.5) * depth;
                                    let yl = wave * h_scale;
                                    let rx = xl * cos_y - zl * sin_y;
                                    let rz = xl * sin_y + zl * cos_y;
                                    let fy = yl * cos_p - rz * sin_p;
                                    let fz = yl * sin_p + rz * cos_p;
                                    pts.push(point(cx0 + px(rx), cy + px(-fy + (fz * 0.25) as f32)));
                                }
                                let col = if is_sel {
                                    rgb(0xFFFFFF).into()
                                } else {
                                    let t = morph.clamp(0.0, 1.0);
                                    let n = colors.len();
                                    if n == 0 {
                                        rgb(0xFFFFFF).into()
                                    } else if n == 1 {
                                        colors[0]
                                    } else {
                                        let idx = (t * (n - 1) as f32).floor() as usize;
                                        let idx = idx.min(n - 2);
                                        let lt = t * (n - 1) as f32 - idx as f32;
                                        let c1 = colors[idx];
                                        let c2 = colors[idx + 1];
                                        Hsla {
                                            h: c1.h + (c2.h - c1.h) * lt,
                                            s: c1.s + (c2.s - c1.s) * lt,
                                            l: c1.l + (c2.l - c1.l) * lt,
                                            a: c1.a + (c2.a - c1.a) * lt,
                                        }
                                    }
                                };
                                let mut pb = PathBuilder::stroke(px(if is_sel { 2.2 } else { 0.9 }));
                                for (i, p) in pts.iter().enumerate() {
                                    if i == 0 {
                                        pb.move_to(*p);
                                    } else {
                                        pb.line_to(*p);
                                    }
                                }
                                if let Ok(path) = pb.build() {
                                    window.paint_path(path, col);
                                }
                            }
                        },
                    )
                    .w_full()
                    .h(px(140.0))
                    .into_any_element(),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(4.0))
                        .child(Label::new("WT POS").text_xs())
                        .child(
                            Button::new(format!("osc_wt_{}", osc_id))
                                .label(format!("{:.0}", wt_pos))
                                .compact()
                                .on_click(move |_, _, cx| {
                                    let st = state(cx);
                                    st.update(cx, |state, cx| {
                                        if let Some(track) = state.live_tracks.get_mut(state.selected_track_index) {
                                            if let Some(slot) = track.effects.iter_mut().find(|s| s.name == "OpenWavetable") {
                                                if let Some(o) = slot.wavetable_oscillators.iter_mut().find(|o| o.id == osc_id) {
                                                    o.wt_pos = (o.wt_pos + 8.0).min(255.0);
                                                }
                                            }
                                        }
                                        cx.notify();
                                    });
                                }),
                        ),
                )
                .into_any_element(),
        );
    }

    let mut mod_elems: Vec<AnyElement> = Vec::new();
    for m in &mods {
        let mod_id = m.id;
        let (name, val, min, max) = {
            let mut eff = m.effect.clone();
            let (n, v, mn, mx) = eff.name_and_range();
            (n.to_string(), *v, mn, mx)
        };
        mod_elems.push(
            v_flex()
                .w(px(80.0))
                .h(px(80.0))
                .bg(rgb(0x181410))
                .border_1()
                .border_color(rgb(0x503C1E))
                .rounded(px(4.0))
                .p(px(4.0))
                .gap(px(2.0))
                .items_center()
                .child(Label::new(name.clone()).text_xs())
                .child(
                            Button::new(format!("mod_val_{}", mod_id))
                                .label(format!("{:.0}", val))
                                .compact()
                                .on_click(move |_, _, cx| {
                                    let st = state(cx);
                                    st.update(cx, |state, cx| {
                                        if let Some(track) = state.live_tracks.get_mut(state.selected_track_index) {
                                            if let Some(slot) = track.effects.iter_mut().find(|s| s.name == "OpenWavetable") {
                                                if let Some(md) = slot.modulators.iter_mut().find(|md| md.id == mod_id) {
                                                    let (_n, v, mn, mx) = md.effect.name_and_range();
                                                    *v = (*v + (mx - mn) / 10.0).min(mx);
                                        }
                                    }
                                }
                                cx.notify();
                            });
                        }),
                )
                .into_any_element(),
        );
    }

    v_flex()
        .id("open_wavetable")
        .size_full()
        .gap(px(4.0))
        .child(
            h_flex()
                .items_center()
                .gap(px(6.0))
                .child(Label::new("3D VIEW:").text_xs().font_weight(FontWeight::BOLD).text_color(rgb(0x00FFFF)))
                .child(Label::new("X").text_xs().text_color(rgb(0xFF6464)))
                .child(
                    Button::new("cam_x_btn")
                        .label(format!("{:.2}", cx_val))
                        .compact()
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            st.update(cx, |state, cx| {
                                if let Some(track) = state.live_tracks.get_mut(state.selected_track_index) {
                                    if let Some(slot) = track.effects.iter_mut().find(|s| s.name == "OpenWavetable") {
                                        slot.cam_x = (slot.cam_x + 0.1).min(PI * 0.45);
                                    }
                                }
                                cx.notify();
                            });
                        }),
                )
                .child(Label::new("Y").text_xs().text_color(rgb(0x64B464)))
                .child(
                    Button::new("cam_y_btn")
                        .label(format!("{:.0}", cy_val))
                        .compact()
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            st.update(cx, |state, cx| {
                                if let Some(track) = state.live_tracks.get_mut(state.selected_track_index) {
                                    if let Some(slot) = track.effects.iter_mut().find(|s| s.name == "OpenWavetable") {
                                        slot.cam_y = (slot.cam_y + 5.0).min(100.0);
                                    }
                                }
                                cx.notify();
                            });
                        }),
                )
                .child(Label::new("Z").text_xs().text_color(rgb(0x64B4FF)))
                .child(
                    Button::new("cam_z_btn")
                        .label(format!("{:.2}", cz_val))
                        .compact()
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            st.update(cx, |state, cx| {
                                if let Some(track) = state.live_tracks.get_mut(state.selected_track_index) {
                                    if let Some(slot) = track.effects.iter_mut().find(|s| s.name == "OpenWavetable") {
                                        slot.cam_z = (slot.cam_z + 0.1).min(PI);
                                    }
                                }
                                cx.notify();
                            });
                        }),
                )
                .child(div().flex_1())
                .child(
                    Button::new("wt_add_osc")
                        .label("+ Osc")
                        .compact()
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            st.update(cx, |state, cx| {
                                if let Some(track) = state.live_tracks.get_mut(state.selected_track_index) {
                                    if let Some(slot) = track.effects.iter_mut().find(|s| s.name == "OpenWavetable") {
                                        let count = slot.wavetable_oscillators.len();
                                        let letter = (b'A' + count as u8) as char;
                                        slot.wavetable_oscillators.push(WavetableOscillator::new(
                                            count,
                                            &format!("OSC {}", letter),
                                            point(px(20.0 + count as f32 * 30.0), px(50.0 + count as f32 * 30.0)),
                                        ));
                                    }
                                }
                                cx.notify();
                            });
                        }),
                ),
        )
        .child(
            div()
                .flex_1()
                .relative()
                .child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            window.paint_quad(PaintQuad {
                                bounds,
                                background: rgb(0x0C0D11).into(),
                                border_color: Hsla::default(),
                                corner_radii: gpui_kit::Corners::default(),
                                border_widths: gpui_kit::Edges::default(),
                                border_style: BorderStyle::default(),
                            });
                            let mut x = bounds.origin.x;
                            while x < bounds.origin.x + bounds.size.width {
                                let mut path = PathBuilder::stroke(px(1.0));
                                path.move_to(point(x, bounds.origin.y));
                                path.line_to(point(x, bounds.origin.y + bounds.size.height));
                                window.paint_path(path.build().unwrap(), rgb(0x161921));
                                x += px(GRID_SIZE);
                            }
                            let mut y = bounds.origin.y;
                            while y < bounds.origin.y + bounds.size.height {
                                let mut path = PathBuilder::stroke(px(1.0));
                                path.move_to(point(bounds.origin.x, y));
                                path.line_to(point(bounds.origin.x + bounds.size.width, y));
                                window.paint_path(path.build().unwrap(), rgb(0x161921));
                                y += px(GRID_SIZE);
                            }
                        },
                    )
                    .absolute()
                    .left(px(0.0))
                    .top(px(0.0))
                    .w_full()
                    .h_full()
                    .into_any_element(),
                )
                .children(osc_elems)
                .children(mod_elems),
        )
        .into_any_element()
}
