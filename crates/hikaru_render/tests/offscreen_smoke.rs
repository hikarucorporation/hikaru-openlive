// crates/hikaru_render/tests/offscreen_smoke.rs

//! Prueba de humo que ejecuta los pipelines contra una GPU real.
//!
//! Por qué hace falta: **las validaciones de wgpu no ocurren al compilar**.
//! Un bind group mal declarado, un vertex layout que no matchea el shader o un
//! draw call fuera de rango compilan sin quejarse y sólo explotan en runtime,
//! cuando la aplicación ya está andando. Estos tests son lo único que
//! convierte esos errores en fallas de CI.
//!
//! Se marcan `#[ignore]` porque necesitan un adaptador disponible. En CI con
//! `lavapipe` (el rasterizador por software de Mesa) o cualquier GPU real, se
//! corren así:
//!
//! ```text
//! cargo test -p hikaru_render --test offscreen_smoke -- --ignored
//! ```

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use hikaru_render::{
    upload_knob, Camera, GpuContext, KnobMesh, MeshRenderer, MeshUniforms, QuadInstance,
    QuadRenderer, SpriteLayout, SpriteSheet, SpriteSheetResources, WavetableMesh, wgpu,
};

/// `block_on` mínimo, para no arrastrar un runtime asíncrono entero como
/// dependencia de desarrollo.
///
/// El executor no hace nada: sólo cede el control cuando el future devuelve
/// `Pending`, que es el caso de las llamadas a la GPU. Basta para tests.
fn block_on<F: Future>(mut future: F) -> F::Output {
    fn noop_waker() -> Waker {
        fn noop(_: *const ()) {}
        fn clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
        // SAFETY: las cuatro funciones del vtable son no-ops que nunca
        // desreferencian el puntero, así que el dato nulo nunca se lee.
        unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
    }

    // SAFETY: `future` se mueve al pin y no vuelve a moverse mientras vive
    // `Pin`, y el waker no hace nada con datos compartidos.
    let mut future = unsafe { Pin::new_unchecked(&mut future) };
    let waker = noop_waker();
    let mut context = Context::from_waker(&waker);

    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

/// Levanta el contexto, o devuelve `None` si la máquina no tiene GPU.
///
/// Se prefiere saltar el test a fallarlo: un runner sin GPU no es un bug del
/// código bajo prueba.
fn context() -> Option<GpuContext> {
    match block_on(GpuContext::new_offscreen()) {
        Ok(ctx) => Some(ctx),
        Err(error) => {
            eprintln!("omitiendo: no hay adaptador de GPU ({error})");
            None
        }
    }
}

/// Imagen de prueba: 4 celdas de 8x8 px, cada una de un color distinto.
fn test_sheet_bytes() -> Vec<u8> {
    const CELL: usize = 8;
    const COLUMNS: usize = 4;
    const ROWS: usize = 1;
    let width = CELL * COLUMNS;
    let height = CELL * ROWS;

    let mut data = vec![0u8; width * height * 4];
    for y in 0..height {
        for x in 0..width {
            let cell = (x / CELL) % COLUMNS;
            let offset = (y * width + x) * 4;
            // Tonos distintos por celda, para poder distinguirlas al leer
            // píxeles más adelante.
            let shade = (cell as u8 + 1) * 40;
            data[offset] = shade;
            data[offset + 1] = 255 - shade;
            data[offset + 2] = 128;
            data[offset + 3] = 255;
        }
    }
    data
}

#[test]
#[ignore = "requiere un adaptador de GPU"]
fn quad_pipeline_draws_without_validation_errors() {
    let Some(ctx) = context() else { return };

    // Todo el bloque va dentro del error scope: si el layout del bind group, el
    // pipeline o el draw call no son válidos, el error aparece en `pop()`.
    let scope = ctx.device.push_error_scope(wgpu::ErrorFilter::Validation);

    let sheets = SpriteSheetResources::new(&ctx);
    let sheet = SpriteSheet::from_rgba8(
        &ctx,
        &sheets,
        "test_sheet",
        32,
        8,
        &test_sheet_bytes(),
        SpriteLayout::Grid { columns: 4, rows: 1 },
    )
    .expect("la hoja de prueba debería ser válida");

    let mut renderer = QuadRenderer::new(
        &ctx,
        &sheets,
        "test_quad",
        GpuContext::preferred_texture_format(),
    );

    let target = ctx.create_render_target("test_target", 64, 64, false);

    // Dos quads que usan celdas distintas de la hoja, en posicion distintas.
    let quads = [
        QuadInstance::new(0.0, 0.0, 32.0, 32.0)
            .with_uv(sheet.cell_uv(0).expect("celda 0 existe")),
        QuadInstance::new(32.0, 32.0, 32.0, 32.0)
            .with_uv(sheet.cell_uv(3).expect("celda 3 existe"))
            .with_tint([0.5, 0.5, 0.5, 1.0]),
    ];

    renderer.draw(&ctx, &target.color_view, 64.0, 64.0, &quads, &sheet);

    // Forzar la ejecución para que el driver reporte los errores ahora y no en
    // algún momento posterior del test.
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("el poll del device debería completar");

    let error = block_on(scope.pop());
    assert!(error.is_none(), "wgpu reportó un error de validación: {error:?}");
}

#[test]
#[ignore = "requiere un adaptador de GPU"]
fn mesh_pipeline_draws_without_validation_errors() {
    let Some(ctx) = context() else { return };

    let scope = ctx.device.push_error_scope(wgpu::ErrorFilter::Validation);

    // Sine de una sola etapa: sirve para probar el layout, no la música.
    let waveform: Vec<f32> = (0..256)
        .map(|i| ((i as f32) / 256.0 * std::f32::consts::TAU).sin())
        .collect();

    let mesh = WavetableMesh::from_waveform(&waveform, 64, Default::default())
        .expect("la malla debería construirse")
        .upload(&ctx, "test_mesh")
        .expect("la malla debería subirse");

    let mut renderer = MeshRenderer::new(
        &ctx,
        "test_mesh_pipeline",
        GpuContext::preferred_texture_format(),
        GpuContext::depth_format(),
    );

    // El pipeline 3D exige profundidad.
    let target = ctx.create_render_target("test_mesh_target", 64, 64, true);

    renderer.draw(&ctx, &target, &mesh, &MeshUniforms::default());

    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("el poll del device debería completar");

    let error = block_on(scope.pop());
    assert!(error.is_none(), "wgpu reportó un error de validación: {error:?}");
}

/// Sine de una sola etapa, para las pruebas de render.
fn sine_waveform() -> Vec<f32> {
    (0..256)
        .map(|i| ((i as f32) / 256.0 * std::f32::consts::TAU).sin())
        .collect()
}

/// Cuenta los píxeles con contenido, ignorando el canal alfa.
///
/// El target se limpia transparente y la malla se compone encima, así que
/// "se dibujó algo" es exactamente "quedaron píxeles no transparentes".
fn non_empty_pixels(pixels: &[u8]) -> usize {
    pixels.chunks_exact(4).filter(|pixel| pixel[3] != 0).count()
}

#[test]
#[ignore = "requiere un adaptador de GPU"]
fn a_perspective_camera_makes_the_wavetable_land_inside_the_viewport() {
    // Esta es la prueba que cierra el circuito de la vista 3D: cámara -> uniforms
    // -> pipeline -> píxeles. Si `view_proj` saliera mal (orden de las
    // matrices, signo de `w`, near/far al revés) la malla se dibuja fuera de
    // pantalla: el frame vuelve con los píxeles vacíos y no hay ningún error
    // de wgpu que lo explique.
    let Some(ctx) = context() else { return };

    let scope = ctx.device.push_error_scope(wgpu::ErrorFilter::Validation);

    let (width, height) = (320u32, 200u32);
    let target = ctx.create_render_target("test_camera_target", width, height, true);

    let mut renderer = MeshRenderer::new(
        &ctx,
        "test_camera_pipeline",
        GpuContext::preferred_texture_format(),
        GpuContext::depth_format(),
    );

    let mesh = WavetableMesh::from_waveform(&sine_waveform(), 64, Default::default())
        .expect("la malla debería construirse")
        .upload(&ctx, "test_camera_mesh")
        .expect("la malla debería subirse");

    let mut camera = Camera::default();
    // `fit_to_box` toma SEMIEJES 3D: la caja del visor de wavetable es
    // 320 x 120 x 110, así que el Z va. Sin el, el test no compila; y no es un
    // detalle cosmético: desde que la vista apila los ciclos, el encuadre se
    // calcula con las ocho esquinas y un Z de 0 deja la malla pegada al plano
    // cercano (ver `Camera::fit_to_box`).
    camera.fit_to_box([160.0, 100.0, 12.0], width as f32 / height as f32);
    let uniforms = camera.uniforms(width as f32 / height as f32, [0.35, 0.85, 1.0, 1.0]);

    // Fondo transparente, como en la GUI: si no, el readback daría "contenido"
    // siempre y el test no probaría nada.
    clear(&ctx, &target);

    renderer.draw(&ctx, &target, &mesh, &uniforms);

    let pixels = ctx.read_color_rgba8(&target).expect("el readback debería completar");

    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("el poll del device debería completar");
    let error = block_on(scope.pop());
    assert!(error.is_none(), "wgpu reportó un error de validación: {error:?}");

    assert_eq!(
        pixels.len(),
        (width * height * 4) as usize,
        "el readback devolvió un tamaño inesperado"
    );

    let lit = non_empty_pixels(&pixels);
    assert!(lit > 0, "la cámara no dejó la malla dentro del viewport");
    // Una cinta de 320x200 dentro de un frame de 320x200 con la cámara
    // inclinada ocupa una fracción apreciable. El piso es holgado a propósito:
    // un número exacto de píxeles varies con el driver.
    assert!(
        lit > (width * height) as usize / 100,
        "apenas se dibujaron {lit} píxeles: la matriz de cámara no está encuadrando bien"
    );
}

#[test]
#[ignore = "requiere un adaptador de GPU"]
fn the_knob_renders_and_rotates_with_its_value() {
    // Cierra el circuito del knob: geometría -> uniforms con ángulo -> píxeles.
    // Si el ángulo no llegara al vertex shader, el knob se dibujaría siempre en
    // la misma posición y los dos valores darían frames idénticos.
    let Some(ctx) = context() else { return };

    let scope = ctx.device.push_error_scope(wgpu::ErrorFilter::Validation);

    let size = hikaru_render::KNOB_RESOLUTION;
    let target = ctx.create_render_target("test_knob_target", size, size, true);

    let mut renderer = hikaru_render::KnobRenderer::new(
        &ctx,
        "test_knob_pipeline",
        GpuContext::preferred_texture_format(),
        GpuContext::depth_format(),
    );
    let mesh =
        upload_knob(&ctx, "test_knob_mesh", &KnobMesh::new(Default::default()))
            .expect("la malla del knob debería subirse");

    let camera = Camera::knob();
    let aspect = 1.0;
    let view_proj = camera.view_proj(aspect);
    let light = camera.local_light_dir([0.35, 0.55, 1.0]);

    // Dos valores opuestos: el del medio del recorrido y el del extremo.
    let mut render_at = |angle: f32| {
        clear(&ctx, &target);
        let uniforms = hikaru_render::KnobUniforms {
            view_proj,
            light_dir: light,
            angle,
            body: [0.2, 0.22, 0.26, 1.0],
            marker: [1.0, 0.43, 0.0, 1.0],
        };
        renderer.draw(&ctx, &target, &mesh, &uniforms);
        ctx.read_color_rgba8(&target).expect("el readback debería completar")
    };

    let low = render_at(-2.35);
    let high = render_at(2.35);

    let lit_low = non_empty_pixels(&low);
    let lit_high = non_empty_pixels(&high);
    assert_eq!(low.len(), (size * size * 4) as usize);
    assert!(lit_low > 0, "el knob no dibujó nada: la cámara no lo encuadra");
    // El disco ocupa buena parte del viewport: es lo que distingue un knob de
    // un punto en el medio del panel.
    assert!(
        lit_low > (size * size) as usize / 20,
        "sólo {lit_low} píxeles: el knob quedó chico o fuera de cuadro"
    );
    // Girar el valor tiene que cambiar la imagen: con la marca puesta, el
    // cambio va de un borde del disco al otro.
    assert_ne!(low, high, "el ángulo del knob no llegó al shader");

    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("el poll del device debería completar");
    let error = block_on(scope.pop());
    assert!(error.is_none(), "wgpu reportó un error de validación: {error:?}");
}

#[test]
#[ignore = "requiere un adaptador de GPU"]
fn the_readback_reports_a_degenerate_target_instead_of_panicking() {
    // El target de dimensión cero no se puede crear (wgpu lo rechaza), pero el
    // readback se llama desde la GUI con tamaños que vienen del layout. El
    // error tiene que ser un `Err` con mensaje, no un panic.
    let Some(ctx) = context() else { return };
    // `create_render_target` no acepta 0x0 (wgpu lo rechaza), así que se parte
    // de un target válido y se le cambia el tamaño declarado: es el estado que
    // la GUI puede llegar a tener si el layout colapsa a cero.
    let target = hikaru_render::RenderTarget {
        size: (0, 0),
        ..ctx.create_render_target("test_readback_zero", 1, 1, false)
    };
    assert!(ctx.read_color_rgba8(&target).is_err());
}

/// Limpia el target a transparente, igual que el fondo de la vista 3D.
fn clear(ctx: &GpuContext, target: &hikaru_render::RenderTarget) {
    let mut encoder =
        ctx.device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("test_clear") });
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("test_clear::pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: target.depth_view.as_ref().map(|view| {
                wgpu::RenderPassDepthStencilAttachment {
                    view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
    ctx.queue.submit(Some(encoder.finish()));
}

#[test]
#[ignore = "requiere un adaptador de GPU"]
fn drawing_more_quads_than_the_initial_capacity_grows_the_buffer() {
    // Cubre la ruta que sólo se ve con más controles que los que caben en la
    // capacidad inicial (256): ahí se recrea el storage buffer y su bind group.
    let Some(ctx) = context() else { return };

    let scope = ctx.device.push_error_scope(wgpu::ErrorFilter::Validation);

    let sheets = SpriteSheetResources::new(&ctx);
    let sheet = SpriteSheet::from_rgba8(
        &ctx,
        &sheets,
        "test_sheet",
        32,
        8,
        &test_sheet_bytes(),
        SpriteLayout::Grid { columns: 4, rows: 1 },
    )
    .expect("la hoja de prueba debería ser válida");

    let mut renderer = QuadRenderer::new(
        &ctx,
        &sheets,
        "test_quad",
        GpuContext::preferred_texture_format(),
    );

    let target = ctx.create_render_target("test_target", 64, 64, false);

    // 1000 quads, muy por encima de la capacidad inicial de 256.
    let quads: Vec<QuadInstance> = (0..1000)
        .map(|i| {
            let x = (i % 32) as f32 * 2.0;
            let y = (i / 32) as f32 * 2.0;
            QuadInstance::new(x, y, 2.0, 2.0).with_uv(sheet.cell_uv_or_first(i % 4))
        })
        .collect();

    renderer.draw(&ctx, &target.color_view, 64.0, 64.0, &quads, &sheet);

    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("el poll del device debería completar");

    let error = block_on(scope.pop());
    assert!(error.is_none(), "wgpu reportó un error de validación: {error:?}");
}
