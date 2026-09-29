/*
 * Hikaru OpenStudio - Audio DAW
 * Copyright (C) 2026 Hikaru OpenStudio Developers
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 */

//! Integración de `hikaru_render` con la GUI.
//!
//! La GPUI Kit ya tiene su propio contexto de GPU y su propia superficie de
//! ventana. Este módulo **no** lo reemplaza: `hikaru_render` trabaja offscreen y
//! produce texturas que la GUI compone después. Mantener las dos cosas
//! separadas es lo que permite que ambas compartan una sola copia de wgpu
//! (ver el `Cargo.toml` raíz).
//!
//! Para qué sirve hoy:
//!
//! - [`HikaruRenderer`] es el punto de entrada único: un contexto, un pipeline
//!   de quads y uno de malla.
//! - [`HikaruRenderer::draw_sprite_sheet`] dibuja los frames de los controles
//!   (perillas, faders, sliders) a una textura.
//! - [`HikaruRenderer::draw_wavetable_mesh`] dibuja la malla 3D de un
//!   oscilador.
//!
//! Ninguna vista usa esto todavía: es la infraestructura para las sesiones
//! siguientes.

use hikaru_render::{
    GpuContext, GpuMesh, MeshRenderer, MeshUniforms, QuadInstance, QuadRenderer, RenderTarget,
    SpriteLayout, SpriteSheet, SpriteSheetResources, WavetableMesh,
};
// Se usa el reexport de `hikaru_render` en vez de declarar wgpu como dependencia
// directa: garantiza que la GUI hable con exactamente la misma versión que el
// crate de render y con la que ya usa gpui-kit.
use hikaru_render::wgpu;

/// Errores al armar el renderer offscreen.
#[derive(Debug)]
pub enum RendererError {
    /// No se encontró una GPU usable (drivers ausentes, Vulkan deshabilitado).
    ///
    /// La GUI sigue funcionando: sólo pierde el render offscreen.
    NoGpu(hikaru_render::context::ContextError),
    /// El target no tiene dimensión cero, que es lo único que wgpu acepta.
    DegenerateSize { width: u32, height: u32 },
}

impl std::fmt::Display for RendererError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RendererError::NoGpu(error) => {
                write!(f, "no hay GPU disponible para el render offscreen: {error}")
            }
            RendererError::DegenerateSize { width, height } => {
                write!(f, "tamaño de render inválido: {width}x{height}")
            }
        }
    }
}

impl std::error::Error for RendererError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RendererError::NoGpu(error) => Some(error),
            RendererError::DegenerateSize { .. } => None,
        }
    }
}

/// Punto de entrada único al render offscreen desde la GUI.
///
/// Es dueño del contexto y de los pipelines. Los assets (sprite sheets) se
/// suben una vez y se reutilizan entre frames; el target de render se
/// re-crea sólo cuando cambia el tamaño.
pub struct HikaruRenderer {
    ctx: GpuContext,
    sheets: SpriteSheetResources,
    quads: QuadRenderer,
    mesh: MeshRenderer,
    /// Target offscreen actual. `None` hasta el primer `resize`.
    target: Option<RenderTarget>,
    /// Tamaño en píxeles lógicos del target actual.
    size: (u32, u32),
}

impl HikaruRenderer {
    /// Inicializa el contexto offscreen y ambos pipelines.
    ///
    /// Es `async` porque pedir el adapter y el device a wgpu lo es. El
    /// pipeline se compila de forma sincrónica una vez que se tiene el device.
    pub async fn new() -> Result<Self, RendererError> {
        let ctx = GpuContext::new_offscreen().await.map_err(RendererError::NoGpu)?;

        let sheets = SpriteSheetResources::new(&ctx);
        let quads = QuadRenderer::new(
            &ctx,
            &sheets,
            "hikaru::quad",
            GpuContext::preferred_texture_format(),
        );
        let mesh = MeshRenderer::new(
            &ctx,
            "hikaru::mesh",
            GpuContext::preferred_texture_format(),
            GpuContext::depth_format(),
        );

        Ok(Self { ctx, sheets, quads, mesh, target: None, size: (0, 0) })
    }

    /// Sube una imagen RGBA8 como hoja de sprites.
    pub fn load_sprite_sheet(
        &self,
        label: &str,
        width: u32,
        height: u32,
        data: &[u8],
        layout: SpriteLayout,
    ) -> Result<SpriteSheet, hikaru_render::QuadError> {
        SpriteSheet::from_rgba8(&self.ctx, &self.sheets, label, width, height, data, layout)
    }

    /// Prepara el target offscreen para el tamaño dado.
    ///
    /// Es un no-op si el tamaño no cambió, así que se puede llamar en cada
    /// frame: el target se re-crea sólo cuando el layout cambia de verdad.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), RendererError> {
        if width == 0 || height == 0 {
            return Err(RendererError::DegenerateSize { width, height });
        }
        if self.size == (width, height) {
            return Ok(());
        }

        self.target = Some(self.ctx.create_render_target("hikaru::target", width, height, true));
        self.size = (width, height);
        Ok(())
    }

    /// Dibuja una lista de quads de una hoja de sprites.
    ///
    /// `quads` van en píxeles lógicos con origen arriba-izquierda, igual que el
    /// resto de la GUI. `clear` decide si el target se borra antes: los
    /// controles se dibujan sobre un fondo, así que normalmente es `false`
    /// después de haber pintado el fondo.
    pub fn draw_sprite_quads(
        &mut self,
        quads: &[QuadInstance],
        sheet: &SpriteSheet,
        clear: bool,
    ) -> Result<(), RendererError> {
        let (width, height) = self.size;
        if width == 0 || height == 0 {
            return Err(RendererError::DegenerateSize { width, height });
        }

        let target = self
            .target
            .as_ref()
            .expect("resize() corre antes de cualquier draw; el target existe");

        if clear {
            self.clear(target);
        }

        self.quads.draw(&self.ctx, &target.color_view, width as f32, height as f32, quads, sheet);
        Ok(())
    }

    /// Dibuja la malla 3D de un oscilador.
    ///
    /// La malla se construye en CPU ([`WavetableMesh::from_waveform`]) y se
    /// sube una vez; después sólo se dibuja.
    pub fn draw_wavetable_mesh(
        &mut self,
        mesh: &GpuMesh,
        uniforms: &MeshUniforms,
    ) -> Result<(), RendererError> {
        let (width, height) = self.size;
        if width == 0 || height == 0 {
            return Err(RendererError::DegenerateSize { width, height });
        }

        let target = self
            .target
            .as_ref()
            .expect("resize() corre antes de cualquier draw; el target existe");

        self.mesh.draw(&self.ctx, target, mesh, uniforms);
        Ok(())
    }

    /// Sube al GPU una cinta construida a partir de los samples de audio.
    pub fn upload_wavetable(
        &self,
        label: &str,
        waveform: &[f32],
        frame_width: u32,
    ) -> Result<GpuMesh, hikaru_render::mesh::MeshError> {
        WavetableMesh::from_waveform(waveform, frame_width, Default::default())?.upload(&self.ctx, label)
    }

    /// Vista del target offscreen actual, lista para componer en la GUI.
    ///
    /// `None` si todavía no se llamó a [`HikaruRenderer::resize`].
    pub fn target_view(&self) -> Option<&wgpu::TextureView> {
        self.target.as_ref().map(|target| &target.color_view)
    }

    /// Contexto de GPU, para las operaciones que este wrapper no expone.
    pub fn context(&self) -> &GpuContext {
        &self.ctx
    }

    /// Limpia el target por completo.
    ///
    /// Se llama antes de componer una capa nueva (fondo + quads, o fondo +
    /// malla) en vez de dejar que cada renderer decida: si el render 3D
    /// preserva el color pero limpia la profundidad, y el 2D al revés, el
    /// orden de las llamadas pasa a importar de forma invisible.
    fn clear(&self, target: &RenderTarget) {
        let mut encoder = self.ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("hikaru::clear"),
        });

        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hikaru::clear::pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                // La profundidad se limpia siempre: es el estado que decide qué
                // cara de la malla 3D se ve, y dejarlo sucio hace que el
                // segundo oscilador no aparezca.
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

        self.ctx.queue.submit(Some(encoder.finish()));
    }
}
