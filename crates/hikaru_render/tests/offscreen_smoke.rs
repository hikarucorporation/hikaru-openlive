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
    GpuContext, MeshRenderer, MeshUniforms, QuadInstance, QuadRenderer, SpriteLayout, SpriteSheet,
    SpriteSheetResources, WavetableMesh, wgpu,
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
