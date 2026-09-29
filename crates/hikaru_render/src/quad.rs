// crates/hikaru_render/src/quad.rs

//! Render 2D de quads con textura, orientado a sprite sheets.
//!
//! Los controles de Hikaru (perillas, faders, sliders) van a ser assets
//! esqueumórficos: cada estado visual es un recorte rectangular de una hoja de
//! sprites. Una perilla, por ejemplo, puede tener 32 frames de rotación y un
//! fader 2 (idle / hover).
//!
//! Este módulo resuelve justamente eso:
//!
//! - [`SpriteSheet`] sube una imagen RGBA8 una sola vez a la GPU y expone
//!   recortes por índice de celda ([`SpriteSheet::cell_uv`]).
//! - [`QuadInstance`] es *un* rectángulo a dibujar: dónde, qué región de la
//!   hoja usa, y con qué tinte.
//! - [`QuadRenderer`] mantiene pipeline, sampler y buffers, y dibuja un slice
//!   de quads en un solo draw call.
//!
//! # Por qué instancing y no un quad por control
//!
//! Un DAW lleno de controles dibuja cientos de quads por frame. Mandarlos todos
//! en un storage buffer y hacer un `draw(0..6, 0..n)` cuesta una sola llamada y
//! deja que el vertex shader genere las dos triángulas: el CPU sólo escribe
//! floats, nunca índices.
//!
//! # Coordenadas
//!
//! Todo el módulo trabaja en **píxeles lógicos con origen arriba-izquierda**,
//! igual que el resto de la GUI. El vertex shader hace el flip a NDC, así el
//! código que arma la lista de quads desde un layout de controles no necesita
//! conocer la convención de wgpu.

use bytemuck::{Pod, Zeroable};

use crate::context::GpuContext;

/// Shader del pipeline de quads.
///
/// No hay vertex buffer: las posiciones salen de `vertex_index` y los datos de
/// cada quad del storage buffer, indexado por `instance_index`.
const QUAD_SHADER: &str = r#"
struct Globals {
    // Tamaño del target en píxeles lógicos.
    resolution: vec2<f32>,
    // Cantidad de quads válidos. Acota la lectura del storage buffer para no
    // dibujar la capacidad reservada y no usada.
    quad_count: u32,
    _pad: u32,
};

struct Quad {
    // x, y, ancho, alto en píxeles lógicos, origen arriba-izquierda.
    rect: vec4<f32>,
    // uv origin (x, y) y uv size (ancho, alto), normalizado a [0, 1].
    uv: vec4<f32>,
    // Multiplicador RGBA sobre el color muestreado.
    tint: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var sheet: texture_2d<f32>;
@group(1) @binding(1) var sheet_sampler: sampler;
@group(2) @binding(0) var<storage, read> quads: array<Quad>;

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) tint: vec4<f32>,
};

// Los dos triángulos de un quad, como offsets (u, v) dentro del rectángulo.
const CORNERS = array<vec2<f32>, 6>(
    vec2<f32>(0.0, 0.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(1.0, 1.0),
);

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VsOut {
    let quad = quads[ii];
    let corner = CORNERS[vi];

    // Offset en píxeles dentro del rectángulo.
    let offset_px = quad.rect.zw * corner;
    let position_px = quad.rect.xy + offset_px;

    // Píxeles lógicos (Y hacia abajo) -> clip space NDC (Y hacia arriba).
    let ndc = (position_px / globals.resolution) * 2.0 - 1.0;

    var out: VsOut;
    out.clip_position = vec4<f32>(ndc.x, -ndc.y, 0.0, 1.0);
    out.uv = quad.uv.xy + quad.uv.zw * corner;
    out.tint = quad.tint;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(sheet, sheet_sampler, in.uv) * in.tint;
}
"#;

/// Región normalizada `[0, 1]` dentro de una [`SpriteSheet`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UvRect {
    /// Esquina superior izquierda de la región.
    pub origin: [f32; 2],
    /// Tamaño de la región. Un `0` en cualquier eje degenera la región y la
    /// vuelve invisible.
    pub size: [f32; 2],
}

impl UvRect {
    /// Región completa de la hoja.
    pub const FULL: Self = Self { origin: [0.0, 0.0], size: [1.0, 1.0] };

    /// Rectángulo completo.
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { origin: [x, y], size: [width, height] }
    }
}

/// Un rectángulo a dibujar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuadInstance {
    /// Posición y tamaño en píxeles lógicos: `x`, `y`, `ancho`, `alto`. `x`/`y`
    /// son la esquina superior izquierda.
    pub rect: [f32; 4],
    /// Qué parte de la hoja se usa.
    pub uv: UvRect,
    /// Multiplicador RGBA. [`WHITE`] no altera el asset.
    pub tint: [f32; 4],
}

/// Tinte neutro: deja el color del asset intacto.
pub const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

impl QuadInstance {
    /// Quad con la región completa de la hoja y sin tinte.
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { rect: [x, y, width, height], uv: UvRect::FULL, tint: WHITE }
    }

    /// Fija la región de la hoja a usar.
    pub fn with_uv(mut self, uv: UvRect) -> Self {
        self.uv = uv;
        self
    }

    /// Fija el tinte.
    pub fn with_tint(mut self, tint: [f32; 4]) -> Self {
        self.tint = tint;
        self
    }
}

/// Cómo se distribuyen los frames a lo largo de una hoja.
///
/// Los sprite sheets de controles suelen venir en rejilla, pero no siempre:
/// algunas sheets apilan todos los frames en una sola fila.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpriteLayout {
    /// Rejilla de `columns` x `rows`, leída de izquierda a derecha y de arriba
    /// abajo.
    Grid { columns: u32, rows: u32 },
    /// Todos los frames en una fila.
    Strip { count: u32 },
}

impl SpriteLayout {
    /// Cantidad total de celdas declaradas.
    pub fn count(&self) -> u32 {
        match self {
            SpriteLayout::Grid { columns, rows } => columns * rows,
            SpriteLayout::Strip { count } => *count,
        }
    }
}

/// Layout de bind group y sampler compartidos por todas las hojas.
///
/// Se crea una vez y se comparte entre [`SpriteSheet`] y [`QuadRenderer`] por
/// dos razones:
///
/// - evita crear un sampler por cada asset (cada uno ocupa memoria de
///   driver y tiene su propio estado de filtrado),
/// - y garantiza que los bind groups de las hojas sean intercambiables: todos
///   son compatibles con el mismo layout, así que cualquier pipeline 2D puede
///   samplerear cualquier hoja sin recompilar.
///
/// Vive fuera de [`GpuContext`] a propósito: el contexto es sólo ciclo de vida
/// de la GPU, esto es política de assets.
pub struct SpriteSheetResources {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl SpriteSheetResources {
    /// Crea el layout y el sampler que usarán todas las hojas.
    pub fn new(ctx: &GpuContext) -> Self {
        let layout = ctx.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hikaru_render::quad::sheet"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        // `address_mode` clampeado a propósito: los assets de controles tienen
        // bordes suaves y el filtrado lineal inevitablemente toca píxeles
        // vecinos de la celda. Con clamp, un frame en el borde de la hoja no
        // envuelve hacia la celda opuesta.
        let sampler = ctx.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("hikaru_render::quad::sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        Self { layout, sampler }
    }
}

/// Una imagen RGBA8 residente en la GPU.
pub struct SpriteSheet {
    /// Layout con el que se indexan las celdas.
    pub layout: SpriteLayout,
    /// Ancho de la textura en píxeles.
    pub width: u32,
    /// Alto de la textura en píxeles.
    pub height: u32,
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

impl SpriteSheet {
    /// Sube una imagen RGBA8 a la GPU.
    ///
    /// `data` debe traer exactamente `width * height * 4` bytes en orden
    /// row-major. Se usa `queue.write_texture`, que sube directo desde el CPU
    /// sin pasar por un buffer intermedio.
    pub fn from_rgba8(
        ctx: &GpuContext,
        resources: &SpriteSheetResources,
        label: &str,
        width: u32,
        height: u32,
        data: &[u8],
        layout: SpriteLayout,
    ) -> Result<Self, QuadError> {
        if width == 0 || height == 0 {
            return Err(QuadError::ZeroSizedSheet);
        }

        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(QuadError::ImageTooLarge)?;

        if data.len() != expected {
            return Err(QuadError::DataLengthMismatch { expected, got: data.len() });
        }

        let texture = ctx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: GpuContext::preferred_texture_format(),
            // COPY_DST para poder subirla; TEXTURE_BINDING para samplerearla.
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        ctx.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &resources.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&resources.sampler),
                },
            ],
        });

        Ok(Self { layout, width, height, view, bind_group })
    }

    /// Región UV de la celda `index`, o `None` si el índice cae fuera de la
    /// hoja.
    ///
    /// Devolver `None` en vez de paniquear deja que la UI decida qué hacer con
    /// un índice mal calculado (normalmente, dibujar el frame 0).
    pub fn cell_uv(&self, index: u32) -> Option<UvRect> {
        cell_uv(self.layout, index)
    }

    /// Región UV de la celda `index`, o la celda 0 si el índice es inválido.
    pub fn cell_uv_or_first(&self, index: u32) -> UvRect {
        self.cell_uv(index).unwrap_or(UvRect::FULL)
    }

    /// Vista de la textura, para componerla manualmente.
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// Bind group listo para el pipeline de quads.
    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }
}

/// Región UV de la celda `index` en un layout dado.
///
/// Se separa de [`SpriteSheet`] porque es aritmética pura: es la parte con los
/// casos borde (división por cero, índices fuera de rango) y la que conviene
/// poder testear sin levantar un contexto de GPU.
pub fn cell_uv(layout: SpriteLayout, index: u32) -> Option<UvRect> {
    let total = layout.count();
    if total == 0 || index >= total {
        return None;
    }

    match layout {
        SpriteLayout::Grid { columns, rows } => {
            // `total > 0` garantiza `columns > 0 && rows > 0`.
            let column = index % columns;
            let row = index / columns;
            Some(UvRect::new(
                column as f32 / columns as f32,
                row as f32 / rows as f32,
                1.0 / columns as f32,
                1.0 / rows as f32,
            ))
        }
        SpriteLayout::Strip { count } => Some(UvRect::new(
            index as f32 / count as f32,
            0.0,
            1.0 / count as f32,
            1.0,
        )),
    }
}

/// Estado compartido del target, subirdo una vez por frame.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Globals {
    /// Tamaño del target en píxeles lógicos.
    pub resolution: [f32; 2],
    /// Cuántos quads son válidos. El draw call ya acota el rango, pero
    /// documentarlo aquí evita que un `debug_assert` futuro encuentre basura.
    pub quad_count: u32,
    /// Relleno hasta 16 bytes.
    pub _pad: u32,
}

/// Pipeline y buffers para dibujar quads texturizados.
pub struct QuadRenderer {
    pipeline: wgpu::RenderPipeline,
    instance_layout: wgpu::BindGroupLayout,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    /// Bind group del storage buffer. Se recrea sólo cuando crece la capacidad.
    instance_bind_group: wgpu::BindGroup,
    /// Cuántos quads entran en el buffer antes de tener que recrearlo.
    capacity: u64,
}

/// Capacidad inicial del buffer de quads.
const INITIAL_CAPACITY: u64 = 256;

/// Bytes por quad empaquetado: `rect` (4) + `uv` (4) + `tint` (4) = 12 floats.
const QUAD_STRIDE: u64 = 12 * 4;

impl QuadRenderer {
    /// Crea el pipeline de quads para el formato de color dado.
    ///
    /// `format` tiene que ser el de la textura de destino, normalmente
    /// [`GpuContext::preferred_texture_format`]. El blend es `source over`:
    /// los quads se acumulan sobre lo que ya esté en el target, que es lo que
    /// necesitan los sprites con bordes suaves.
    pub fn new(
        ctx: &GpuContext,
        resources: &SpriteSheetResources,
        label: &str,
        format: wgpu::TextureFormat,
    ) -> Self {
        let globals_layout = ctx.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hikaru_render::quad::globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let instance_layout = ctx.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hikaru_render::quad::instances"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                // El vertex shader lee los quads.
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let shader = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("hikaru_render::quad::shader"),
                source: wgpu::ShaderSource::Wgsl(QUAD_SHADER.into()),
            });

        let pipeline_layout = ctx.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hikaru_render::quad::layout"),
            bind_group_layouts: &[Some(&globals_layout), Some(&resources.layout), Some(&instance_layout)],
            // `var<immediate>` no se usa en este shader.
            immediate_size: 0,
        });

        let pipeline = ctx.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&pipeline_layout),
            // Sin vertex buffers: la geometría sale de `vertex_index`.
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                // Los quads son coplanarios y siempre de cara al observador:
                // culling no aporta nada y sólo agrega una restricción más.
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let uniform_buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hikaru_render::quad::globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let uniform_bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hikaru_render::quad::globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let (instance_buffer, instance_bind_group) =
            create_instance_storage(ctx, &instance_layout, INITIAL_CAPACITY);

        Self {
            pipeline,
            instance_layout,
            uniform_buffer,
            uniform_bind_group,
            instance_buffer,
            instance_bind_group,
            capacity: INITIAL_CAPACITY,
        }
    }

    /// Dibuja `quads` sobre la vista de color dada.
    ///
    /// `width`/`height` son el tamaño del target en píxeles lógicos: es lo que
    /// el shader usa para convertir a NDC, así que tienen que coincidir con la
    /// textura de destino.
    pub fn draw(
        &mut self,
        ctx: &GpuContext,
        target: &wgpu::TextureView,
        width: f32,
        height: f32,
        quads: &[QuadInstance],
        sheet: &SpriteSheet,
    ) {
        if quads.is_empty() {
            return;
        }

        self.ensure_capacity(ctx, quads.len() as u64);

        let globals = Globals {
            resolution: [width.max(1.0), height.max(1.0)],
            quad_count: quads.len() as u32,
            _pad: 0,
        };
        ctx.queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&globals));
        ctx.queue.write_buffer(&self.instance_buffer, 0, &pack_quads(quads));

        let mut encoder =
            ctx.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("hikaru_render::quad::draw"),
                });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hikaru_render::quad::pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // `Load`: los quads se acumulan sobre lo ya dibujado.
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            pass.set_bind_group(1, sheet.bind_group(), &[]);
            pass.set_bind_group(2, &self.instance_bind_group, &[]);
            // 6 vértices por quad, un quad por instancia.
            pass.draw(0..6, 0..quads.len() as u32);
        }

        ctx.queue.submit(Some(encoder.finish()));
    }

    /// Crece el storage buffer si el frame pide más quads que la capacidad.
    ///
    /// Crecer por potencias de dos evita recrear el bind group cada vez que la
    /// lista de controles crece en uno.
    fn ensure_capacity(&mut self, ctx: &GpuContext, needed: u64) {
        if needed <= self.capacity {
            return;
        }

        let mut capacity = self.capacity.max(1);
        while capacity < needed {
            capacity *= 2;
        }

        let (instance_buffer, instance_bind_group) =
            create_instance_storage(ctx, &self.instance_layout, capacity);
        self.instance_buffer = instance_buffer;
        self.instance_bind_group = instance_bind_group;
        self.capacity = capacity;
    }
}

/// Empaqueta una lista de quads a bytes para `write_buffer`.
///
/// Se aplana a floats en vez de mandar un `repr(C)` directo porque
/// `QuadInstance` tiene campos compuestos anidados y `bytemuck` no deriva `Pod`
/// sobre eso. Aplanar deja además el formato explícito y emparejado con el
/// `array<Quad>` del WGSL.
fn pack_quads(quads: &[QuadInstance]) -> Vec<u8> {
    let mut out = Vec::with_capacity(quads.len() * QUAD_STRIDE as usize);
    for quad in quads {
        for value in quad
            .rect
            .iter()
            .chain(quad.uv.origin.iter())
            .chain(quad.uv.size.iter())
            .chain(quad.tint.iter())
        {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    out
}

/// Crea un storage buffer para `capacity` quads y su bind group.
fn create_instance_storage(
    ctx: &GpuContext,
    layout: &wgpu::BindGroupLayout,
    capacity: u64,
) -> (wgpu::Buffer, wgpu::BindGroup) {
    let buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("hikaru_render::quad::instances"),
        size: capacity * QUAD_STRIDE,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("hikaru_render::quad::instances"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });

    (buffer, bind_group)
}

/// Errores al preparar un sprite sheet.
#[derive(Debug, Clone, PartialEq)]
pub enum QuadError {
    /// La imagen no entra en `usize` al multiplicar sus dimensiones.
    ImageTooLarge,
    /// `data` no tiene `width * height * 4` bytes.
    DataLengthMismatch {
        /// Bytes que el descriptor exige.
        expected: usize,
        /// Bytes que traía el buffer.
        got: usize,
    },
    /// La textura tiene dimensión cero.
    ZeroSizedSheet,
}

impl std::fmt::Display for QuadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QuadError::ImageTooLarge => write!(f, "la imagen excede el tamaño direccionable"),
            QuadError::DataLengthMismatch { expected, got } => {
                write!(f, "la imagen trae {got} bytes pero se esperaban {expected}")
            }
            QuadError::ZeroSizedSheet => write!(f, "la textura tiene dimensión cero"),
        }
    }
}

impl std::error::Error for QuadError {}

#[cfg(test)]
mod tests {
    use super::*;
    use naga::valid::{Capabilities, ValidationFlags, Validator};

    /// Valida el WGSL del pipeline sin necesitar una GPU.
    ///
    /// `cargo check` no compila shaders: un error de WGSL sólo aparecería en
    /// runtime, con la GUI ya andando. Este test lo sube al build.
    fn validate(source: &str) {
        let module = naga::front::wgsl::parse_str(source)
            .unwrap_or_else(|error| panic!("WGSL no parsea: {error:?}"));
        Validator::new(ValidationFlags::all(), Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|error| panic!("WGSL no valida: {error:?}"));
    }

    #[test]
    fn quad_shader_is_valid_wgsl() {
        validate(QUAD_SHADER);
    }

    #[test]
    fn buffer_strides_match_the_shader_layout() {
        // El WGSL declara `Globals { vec2, u32, u32 }` y `Quad { vec4, vec4,
        // vec4 }`. Si estos tamaños se desincronizan del layout, el shader lee
        // floats desplazados y los sprites salen movidos sin que ninguna
        // validación de wgpu se queje.
        assert_eq!(std::mem::size_of::<Globals>() as u64, 16);
        assert_eq!(QUAD_STRIDE, 12 * 4);
    }

    #[test]
    fn grid_cells_partition_the_sheet() {
        let layout = SpriteLayout::Grid { columns: 4, rows: 2 };
        assert_eq!(layout.count(), 8);

        let first = cell_uv(layout, 0).unwrap();
        assert_eq!(first.origin, [0.0, 0.0]);
        assert_eq!(first.size, [0.25, 0.5]);

        // La celda 4 arranca la segunda fila.
        let fourth = cell_uv(layout, 4).unwrap();
        assert_eq!(fourth.origin, [0.0, 0.5]);

        let last = cell_uv(layout, 7).unwrap();
        assert_eq!(last.origin, [0.75, 0.5]);

        // Las ocho celdas, sumadas, cubren la hoja completa sin solaparse: ni
        // hueco (mismo origen y tamaño) ni desborde (última celda al borde).
        let area: f32 = (0..8)
            .filter_map(|i| cell_uv(layout, i))
            .map(|uv| uv.size[0] * uv.size[1])
            .sum();
        assert!((area - 1.0).abs() < f32::EPSILON, "las celdas no tapan la hoja: {area}");

        assert!(cell_uv(layout, 8).is_none(), "un índice fuera de la hoja debe ser None");
    }

    #[test]
    fn empty_grid_yields_no_cells_instead_of_dividing_by_zero() {
        // Una hoja mal declarada no debe producir NaN en las UV.
        let layout = SpriteLayout::Grid { columns: 0, rows: 0 };
        assert!(cell_uv(layout, 0).is_none());
    }

    #[test]
    fn strip_cells_have_full_height() {
        let layout = SpriteLayout::Strip { count: 3 };
        let cell = cell_uv(layout, 1).unwrap();
        assert_eq!(cell.origin, [1.0 / 3.0, 0.0]);
        assert_eq!(cell.size, [1.0 / 3.0, 1.0]);
    }

    #[test]
    fn quad_defaults_are_neutral() {
        let quad = QuadInstance::new(10.0, 20.0, 30.0, 40.0);
        assert_eq!(quad.rect, [10.0, 20.0, 30.0, 40.0]);
        assert_eq!(quad.uv, UvRect::FULL);
        assert_eq!(quad.tint, WHITE);
    }

    #[test]
    fn packing_writes_every_float_of_every_quad() {
        // El packing a mano es el punto donde un cambio de layout corrompería
        // en silencio, así que se verifica el conteo exacto de bytes.
        let quads = [
            QuadInstance::new(1.0, 2.0, 3.0, 4.0).with_uv(UvRect::new(0.1, 0.2, 0.3, 0.4)),
            QuadInstance::new(5.0, 6.0, 7.0, 8.0).with_tint([0.1, 0.2, 0.3, 0.4]),
        ];
        let packed = pack_quads(&quads);
        assert_eq!(packed.len() as u64, quads.len() as u64 * QUAD_STRIDE);

        // Los primeros floats deben ser el rect del primer quad, sin padding.
        let first_rect: [f32; 4] = bytemuck::pod_read_unaligned(&packed[..16]);
        assert_eq!(first_rect, [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn empty_quad_list_packs_to_nothing() {
        assert!(pack_quads(&[]).is_empty());
    }
}
