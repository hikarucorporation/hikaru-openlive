// Hikaru OpenLive - Branding (egui)
// GNU AGPLv3
// crates/hikaru_gui/src/views/branding.rs

use egui::{ColorImage, Context, TextureHandle, TextureOptions};
use image::{Rgba, RgbaImage};
use std::sync::OnceLock;

const ENERGY_CORE_LOGO: &[u8] =
    include_bytes!("../../../../assets/logos/hikaru_energy_core_2.png");

const ALPHA_CUTOFF: u8 = 8;

fn decode_energy_core() -> Option<RgbaImage> {
    let decoded = image::load_from_memory(ENERGY_CORE_LOGO)
        .expect("el PNG del logo va embebido y fue verificado al compilar");
    let rgba = decoded.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());

    let (mut min_x, mut min_y) = (width, height);
    let (mut max_x, mut max_y) = (0u32, 0u32);
    let mut found = false;

    for (x, y, pixel) in rgba.enumerate_pixels() {
        if pixel.0[3] > ALPHA_CUTOFF {
            found = true;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }

    if !found {
        return None;
    }

    let cropped = image::imageops::crop_imm(
        &rgba,
        min_x,
        min_y,
        (max_x - min_x + 1).min(width - min_x),
        (max_y - min_y + 1).min(height - min_y),
    )
    .to_image();

    let side = cropped.width().max(cropped.height());
    let mut square = RgbaImage::from_pixel(side, side, Rgba([0, 0, 0, 0]));
    let offset_x = i64::from((side - cropped.width()) / 2);
    let offset_y = i64::from((side - cropped.height()) / 2);
    image::imageops::replace(&mut square, &cropped, offset_x, offset_y);

    Some(square)
}

pub fn energy_core_color_image() -> Option<ColorImage> {
    static IMAGE: OnceLock<Option<ColorImage>> = OnceLock::new();
    IMAGE
        .get_or_init(|| {
            let rgba = decode_energy_core()?;
            let (width, height) = (rgba.width() as usize, rgba.height() as usize);
            Some(ColorImage::from_rgba_unmultiplied(
                [width, height],
                &rgba.into_raw(),
            ))
        })
        .clone()
}

pub fn energy_core_texture(ctx: &Context) -> Option<TextureHandle> {
    let img = energy_core_color_image()?;
    Some(ctx.load_texture("energy_core_logo", img, TextureOptions::LINEAR))
}

pub fn energy_core_logo_widget(ui: &mut egui::Ui, size: f32) {
    if let Some(tex) = energy_core_texture(ui.ctx()) {
        ui.image((tex.id(), egui::vec2(size, size)));
    }
}
