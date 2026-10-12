// Hikaru OpenLive - Branding
// GNU AGPLv3
// crates/hikaru_gui/src/views/branding.rs
//
// Identidad visual de la app: el logo "Energy Core" y sus usos en la UI.
//
// Vive en su propio módulo (y no duplicado en cada vista) porque el mismo
// asset se pinta hoy en dos lugares con requisitos distintos: el modal About
// (logo grande) y la barra de transporte (logo chico, clickeable). Decodificar
// el PNG es caro --512x512 RGBA más el intercambio a BGRA--, así que la
// decodificación se hace UNA vez y se cachea en un `OnceLock`; cada vista sólo
// elige el tamaño y el envoltorio.

use gpui_kit::{
    div, img, px, AnyElement, IntoElement as _, InteractiveElement as _, ObjectFit, RenderImage,
    StatefulInteractiveElement as _, Styled as _, StyledImage as _, TestSupportExt as _,
};
use image::{Rgba, RgbaImage};

use std::sync::{Arc, OnceLock};

use crate::render::render_image_from_rgba;

/// El logo "Energy Core" embebido en el binario.
///
/// Va con `include_bytes!` y no como ruta en disco a propósito: el ejecutable
/// tiene que verse igual sin el árbol de assets al lado (mismo criterio que
/// `views/wavetable_io.rs` con la wavetable de fábrica).
///
/// **Ojo con la extensión**: el diseño vive en
/// `assets/logos/hikaru_energy_core_2.svg`, pero lo que se embebe es el `.png`
/// rasterizado a mano del mismo logo. GPUI no trae un rasterizador de SVG --
/// el `img()` sólo acepta un `RenderImage` que ya son bytes de píxeles--, así
/// que un SVG suelto no se podría pintar. Los dos archivos tienen que seguir
/// siendo el mismo diseño: si se edita el SVG hay que re-rasterizar el PNG.
const ENERGY_CORE_LOGO: &[u8] =
    include_bytes!("../../../../assets/logos/hikaru_energy_core_2.png");

/// Umbral de alfa para considerar un píxel "parte del logo".
///
/// Bajo 8/255 sólo quedan halos de antialiasing del rasterizado, que al recortar
/// se convierten en un borde de 1px sucio alrededor del logo.
const ALPHA_CUTOFF: u8 = 8;

/// Recorta el PNG a su rectángulo opaco y lo re-encuadra en un cuadrado
/// transparente centrado.
///
/// **Por qué hace falta**: el asset es un lienzo de 512x512 pero el dibujo sólo
/// ocupa una franja central (~256x281). Pintarlo tal cual con
/// `ObjectFit::Contain` deja el logo renderizado a la mitad del tamaño pedido y
/// descentrado respecto de su caja. En un control de 40px eso es un logo de
/// ~20px metido en una caja de 40px: se ve diminuto al lado de los botones
/// vecinos. Recortando, el `size` que pide la vista es el tamaño real visible.
///
/// Devolver un cuadrado (y no el recorte tal cual) hace que el logo se centre
/// igual en horizontal y en vertical, así una caja `w == h` lo muestra centrado
/// sin tener que compensarlo a mano en cada vista.
fn decode_energy_core() -> Option<RgbaImage> {
    let decoded = image::load_from_memory(ENERGY_CORE_LOGO)
        .expect("el PNG del logo va embebido y fue verificado al compilar");
    let rgba = decoded.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());

    // Bounding box de los píxeles con alfa visible. `enumerate_pixels` entrega
    // las coordenadas ya en espacio de imagen, así que el `crop_imm` de abajo
    // usa los mismos valores sin offset de por medio.
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

    // Un PNG totalmente transparente no se puede recortar: si algún día el
    // asset llega roto así, se devuelve `None` y cada vista dibuja su hueco
    // vacío en vez de entrar en panic al abrir la ventana.
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

    // Lienzo cuadrado del lado del eje mayor, con el logo centrado.
    let side = cropped.width().max(cropped.height());
    let mut square = RgbaImage::from_pixel(side, side, Rgba([0, 0, 0, 0]));
    let offset_x = i64::from((side - cropped.width()) / 2);
    let offset_y = i64::from((side - cropped.height()) / 2);
    // `replace` copia los píxeles tal cual (no compone alfa), que es lo que
    // hace falta: el recorte ya trae su propia transparencia y componerla
    // contra un fondo transparente no aportaría nada.
    image::imageops::replace(&mut square, &cropped, offset_x, offset_y);

    Some(square)
}

/// El [`RenderImage`] del logo, decodificado una sola vez y cacheado.
///
/// `OnceLock` alcanza porque el asset es una constante de la app: si algún día
/// el logo pasa a depender del tema o del modo, hay que cambiar la clave a algo
/// tipo un `HashMap` en el estado global.
pub fn energy_core_logo() -> Option<Arc<RenderImage>> {
    static LOGO: OnceLock<Option<Arc<RenderImage>>> = OnceLock::new();

    LOGO.get_or_init(|| {
        let rgba = decode_energy_core()?;
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

/// El logo como elemento `img` de lado `size`, identificado con `id`.
///
/// El `id` no es decorativo: los tests de layout lo usan para medir los bounds
/// reales del logo dentro de su control (`tests/transport_logo.rs`). Va también
/// `.test_support()` porque `try_find` sólo ve elementos registrados.
///
/// Si el PNG no decodifica devuelve un hueco vacío del mismo tamaño en vez de
/// reventar la app: para un asset versionado junto al código eso ya sería un bug
/// de build, y una ventana que no abre por un logo es peor que un logo faltante.
pub fn energy_core_logo_img(id: &'static str, size: f32) -> AnyElement {
    match energy_core_logo() {
        // El estilo va ANTES de `test_support()`: el envoltorio `Observed` que
        // devuelve no reexpone los métodos de `Styled`/`StyledImage`, y sí
        // expone `.id(...)`, que es lo único que hace falta después.
        Some(image) => img(image)
            .w(px(size))
            .h(px(size))
            .object_fit(ObjectFit::Contain)
            .id(id)
            // Va al final: el envoltorio `Observed` que devuelve no reexpone
            // los métodos de estilo, y `test_support` exige que el elemento ya
            // tenga un `ElementId` asignado.
            .test_support()
            .into_any_element(),
        None => div().w(px(size)).h(px(size)).into_any_element(),
    }
}