// crates/hikaru_render/src/knob.rs

//! Knob 3D: un disco con relieve que gira, para los controles rotativos.
//!
//! # Por qué geometría y no un dial pintado
//!
//! Los diales 2D que se dibujan con `canvas` (ver el canvas de la Wavetable en
//! `hikaru_gui`) alcanzan para un valor estático, pero un knob que gira con
//! perspectiva se lee como un objeto físico: el canto, la sombra y la marca que
//! sube con el valor. Eso necesita un segundo pase de profundidad y una normal
//! por cara, o sea el mismo pipeline 3D que la malla de la Wavetable.
//!
//! # La marca va en la geometría, no en la rotación
//!
//! El ángulo del valor se aplica en el **vertex shader**, sobre posición y
//! normal. La marca (la rayita que señala el valor) es geometría aparte, con
//! `uv.x = 1.0`, y el fragment shader la pinta con otro color. Así el valor no
//! obliga a re-subir ningún buffer: el knob es siempre la misma malla y lo
//! único que cambia son 4 bytes de uniform.
//!
//! # Ejes
//!
//! El eje del knob es **+Z**, que es hacia donde mira la cámara en la convención
//! de [`crate::camera`]. Por eso la rotación del valor es sobre Z y no sobre Y.

use bytemuck::{Pod, Zeroable};

use crate::context::GpuContext;
use crate::mesh::MeshVertex;

/// Shader del pipeline del knob.
///
/// Comparte el layout de vértice de [`crate::mesh`] (posición, normal, uv) para
/// poder reutilizar [`MeshVertex`] y la misma descripción de vertex buffer.
const KNOB_SHADER: &str = r#"
struct Uniforms {
    // view * projection, en columna mayor.
    view_proj: mat4x4<f32>,
    // Dirección de la luz, normalizada, en espacio local.
    light_dir: vec3<f32>,
    // Ángulo de rotación del valor, en radianes, sobre el eje Z.
    angle: f32,
    // Color del cuerpo.
    body: vec4<f32>,
    // Color de la marca del valor.
    marker: vec4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) local_normal: vec3<f32>,
    // x = 1.0 en la marca del valor, 0.0 en el cuerpo.
    @location(1) is_marker: f32,
};

fn rotate_z(p: vec3<f32>, angle: f32) -> vec3<f32> {
    // WGSL no tiene destructuring de vectores: `let (s, c) = vec2<f32>(...)` no
    // compila, hay que leer los componentes por separado.
    let s = sin(angle);
    let c = cos(angle);
    return vec3<f32>(p.x * c - p.y * s, p.x * s + p.y * c, p.z);
}

@vertex
fn vs_main(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>,
           @location(2) uv: vec2<f32>) -> VsOut {
    var out: VsOut;
    // La rotación va primero y en local: el axis no se rota, así que el
    // resultado es el mismo que rotar la malla en el mundo, sin tocar la view.
    let rotated = rotate_z(position, uniforms.angle);
    out.clip_position = uniforms.view_proj * vec4<f32>(rotated, 1.0);
    out.local_normal = rotate_z(normal, uniforms.angle);
    out.is_marker = uv.x;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(in.local_normal);
    let l = normalize(uniforms.light_dir);
    // La cámara mira al knob de frente, así que el ojo está en +Z local: el
    // reflejo especular va contra esa dirección y no hace falta mandar la
    // posición de la cámara.
    let v = vec3<f32>(0.0, 0.0, 1.0);

    let diffuse = max(dot(n, l), 0.0);
    // Especular de baja potencia: en un knob lo que da la forma es el brillo
    // difuso del canto, un highlight duro lo vuelve plástico.
    let half_vec = normalize(l + v);
    let spec = pow(max(dot(n, half_vec), 0.0), 16.0) * 0.35;

    // La marca se interpola, así que se decide con el paso en 0.5 en vez de
    // comparar floats interpolados contra cero.
    let base = select(uniforms.body.rgb, uniforms.marker.rgb, in.is_marker > 0.5);

    // El ambiente sube con la normal Z: la cara plana del knob queda más clara
    // que el canto, que es lo que lo hace leer como un cilindro.
    let facing = 0.55 + 0.45 * max(n.z, 0.0);
    let color = base * (0.22 + 0.78 * diffuse) * facing + vec3<f32>(spec);

    return vec4<f32>(color, 1.0);
}
"#;

/// Resolución del knob: 96x96.
///
/// Chiquito a propósito: son dos knobs en una tira de 168px, y el readback
/// devuelve `width * height * 4` bytes. 96x96 son 36 KB por knob.
pub const KNOB_RESOLUTION: u32 = 96;

/// Geometría del knob, lista para subir a la GPU.
///
/// La malla es la misma para todos los valores: la rotación va en los uniforms.
#[derive(Debug, Clone)]
pub struct KnobMesh {
    /// Vértices con `uv.x` marcando la marca del valor.
    pub vertices: Vec<MeshVertex>,
    /// Índices en tripletes.
    pub indices: Vec<u32>,
}

/// Parámetros de construcción del knob.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KnobMeshParams {
    /// Radio del disco.
    pub radius: f32,
    /// Profundidad total, de la cara de atrás a la de adelante.
    pub depth: f32,
    /// Segmentos del canto. 48 se ve redondo sin gastar geometría de más.
    pub segments: u32,
    /// Ancho angular de la marca, en radianes.
    pub marker_width: f32,
}

impl Default for KnobMeshParams {
    fn default() -> Self {
        Self { radius: 1.0, depth: 0.42, segments: 48, marker_width: 0.11 }
    }
}

impl KnobMesh {
    /// Construye el knob.
    ///
    /// La forma es un cilindro con el canto biselado y la cara de adelante
    /// ligeramente achaflanada hacia dentro, más una marca radial en la cara
    /// superior. El bisel es lo que produce la línea de luz que separa la cara
    /// del canto y hace legible el volumen con una sola luz.
    pub fn new(params: KnobMeshParams) -> Self {
        let segments = params.segments.max(3);
        let radius = params.radius.abs().max(f32::EPSILON);
        let half_depth = params.depth.abs().max(f32::EPSILON) * 0.5;

        // Perfil de la sección, de atrás hacia adelante: (radio, z, normal_z).
        // El último punto se cierra con la cara plana.
        let back_bevel = 0.90;
        let front_inset = 0.94;
        let profile = [
            (radius * back_bevel, -half_depth),
            (radius, -half_depth * 0.55),
            (radius, half_depth * 0.45),
            (radius * front_inset, half_depth),
        ];

        let mut vertices = Vec::with_capacity(profile.len() * segments as usize + 2);
        let mut indices = Vec::with_capacity(profile.len() * segments as usize * 6);

        // Anillos del perfil. La normal de cada anillo apunta hacia afuera y
        // hacia el cono al que pertenece, interpolando el Z del bisel.
        for (ring, (ring_radius, z)) in profile.iter().enumerate() {
            for segment in 0..segments {
                let angle = segment as f32 / segments as f32 * std::f32::consts::TAU;
                let (sin, cos) = angle.sin_cos();

                // Normal del canto: radial, con el Z invertido en los biseles
                // para que el cono inclinado también se ilumine.
                let normal_z = match ring {
                    0 => -0.5,
                    1 | 2 => 0.0,
                    _ => 0.5,
                };
                let normal = normalize([cos, sin, normal_z]);

                vertices.push(MeshVertex {
                    position: [cos * ring_radius, sin * ring_radius, *z],
                    normal,
                    // 0.0 = cuerpo.
                    uv: [0.0, segment as f32 / segments as f32],
                });
            }
        }

        // Caras entre anillos consecutivos.
        for ring in 0..profile.len() - 1 {
            let current = ring as u32 * segments;
            let next = (ring + 1) as u32 * segments;

            for segment in 0..segments {
                let a = current + segment;
                let b = current + (segment + 1) % segments;
                let c = next + segment;
                let d = next + (segment + 1) % segments;

                // Doble cara: el knob se ve desde los dos lados del canto y sin
                // culling se evita elavor de un orden de índices que dependa de
                // hacia dónde mire la cámara.
                indices.extend_from_slice(&[a, c, b]);
                indices.extend_from_slice(&[b, c, d]);
            }
        }

        // Cara de adelante: un abanico desde el centro.
        let center = vertices.len() as u32;
        vertices.push(MeshVertex {
            position: [0.0, 0.0, half_depth],
            normal: [0.0, 0.0, 1.0],
            uv: [0.0, 0.5],
        });

        let front_ring = (profile.len() - 1) as u32 * segments;
        for segment in 0..segments {
            let a = front_ring + segment;
            let b = front_ring + (segment + 1) % segments;
            indices.extend_from_slice(&[center, a, b]);
        }

        // La marca: una cuña plana sobre la cara, un pelo por encima para que
        // no compita en profundidad con el abanico.
        let marker_radius_outer = radius * front_inset;
        let marker_radius_inner = marker_radius_outer * 0.52;
        let marker_z = half_depth + 0.004;
        let half_marker = params.marker_width.abs().max(1e-3) * 0.5;
        // La marca se construye apuntando a +X (0°), y el shader le **suma** el
        // ángulo que manda el uniforme.
        //
        // Antes la marca se construía en 225° (abajo-izquierda) y el uniforme
        // mandaba `-135° + 270*fill`, de modo que la posición final era
        // `225° - 135° = 90°` con el valor en cero, y `225° + 135° = 360°` con
        // el valor en uno. El marcador recorría un cuarto de vuelta entre las
        // 12 y las 3, en un solo cuadrante, en vez de los 270° del dial. Con el
        // 3D inclinado se veía como un arco chico arriba a la derecha y el knob
        // no se correspondía con su propio valor.
        //
        // Con la base en 0°, el ángulo del uniforme es la posición absoluta de la
        // marca: `225° - 270*fill` va de las 7:30 (mínimo) a las 4:30 (máximo)
        // pasando por las 12, que es el recorrido del dial habitual.
        let base_angle = 0.0;

        let marker_start = vertices.len() as u32;
        for (inner, angle_offset) in
            [(false, -half_marker), (true, -half_marker), (true, half_marker), (false, half_marker)]
        {
            let angle = base_angle + angle_offset;
            let (sin, cos) = angle.sin_cos();
            let r = if inner { marker_radius_inner } else { marker_radius_outer };
            vertices.push(MeshVertex {
                position: [cos * r, sin * r, marker_z],
                normal: [0.0, 0.0, 1.0],
                // 1.0 = marca: el fragment shader la pinta con otro color.
                uv: [1.0, 0.0],
            });
        }
        indices.extend_from_slice(&[
            marker_start,
            marker_start + 1,
            marker_start + 2,
            marker_start,
            marker_start + 2,
            marker_start + 3,
        ]);

        Self { vertices, indices }
    }
}

/// Errores al construir o subir la malla del knob.
#[derive(Debug, Clone, PartialEq)]
pub enum KnobError {
    /// La geometría quedó vacía.
    Empty,
    /// La cantidad de índices no es múltiplo de 3.
    IndexCountNotMultipleOfThree {
        /// Índices recibidos.
        got: usize,
    },
    /// Un índice apunta más allá del último vértice.
    IndexOutOfRange {
        /// Índice inválido.
        index: u32,
        /// Vértices disponibles.
        vertex_count: usize,
    },
}

impl std::fmt::Display for KnobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KnobError::Empty => write!(f, "la malla del knob está vacía"),
            KnobError::IndexCountNotMultipleOfThree { got } => {
                write!(f, "{got} índices no es múltiplo de 3")
            }
            KnobError::IndexOutOfRange { index, vertex_count } => {
                write!(f, "índice {index} fuera de rango ({vertex_count} vértices)")
            }
        }
    }
}

impl std::error::Error for KnobError {}

/// Normaliza un vector 3D.
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if !length.is_finite() || length <= f32::EPSILON {
        return [0.0, 0.0, 1.0];
    }
    [v[0] / length, v[1] / length, v[2] / length]
}

/// Uniforms del knob.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct KnobUniforms {
    /// Matriz `projection * view`.
    pub view_proj: [[f32; 4]; 4],
    /// Dirección de la luz en espacio local.
    pub light_dir: [f32; 3],
    /// Ángulo del valor, en radianes.
    pub angle: f32,
    /// Color del cuerpo.
    pub body: [f32; 4],
    /// Color de la marca.
    pub marker: [f32; 4],
}

impl Default for KnobUniforms {
    fn default() -> Self {
        Self {
            view_proj: crate::camera::identity(),
            light_dir: normalize([0.35, 0.55, 1.0]),
            angle: 0.0,
            body: [0.16, 0.17, 0.20, 1.0],
            marker: [1.0, 0.43, 0.0, 1.0],
        }
    }
}

/// Pipeline del knob.
pub struct KnobRenderer {
    pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
}

impl KnobRenderer {
    /// Crea el pipeline para un formato de color y uno de profundidad.
    pub fn new(
        ctx: &GpuContext,
        label: &str,
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
    ) -> Self {
        let uniform_layout = ctx.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hikaru_render::knob::uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let shader =
            ctx.device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("hikaru_render::knob::shader"),
                    source: wgpu::ShaderSource::Wgsl(KNOB_SHADER.into()),
                });

        let pipeline_layout = ctx.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hikaru_render::knob::layout"),
            bind_group_layouts: &[Some(&uniform_layout)],
            immediate_size: 0,
        });

        let pipeline = ctx.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<MeshVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
                }],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                // Sin culling: la marca y la cara están una sobre la otra y el
                // orden de índices depende del anillo. Con culling, media malla
                // desaparecía según el lado desde el que se mirara.
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: Some(true),
                // `LessEqual` y no `Less`: la marca está un pelo por encima de
                // la cara y con `Less` la marca, que se dibuja después, pasa el
                // test contra sí misma.
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let uniform_buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hikaru_render::knob::uniforms"),
            size: std::mem::size_of::<KnobUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let uniform_bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hikaru_render::knob::uniforms"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        Self { pipeline, uniform_buffer, uniform_bind_group }
    }

    /// Dibuja el knob.
    ///
    /// `depth_view` es obligatorio por lo mismo que en
    /// [`crate::mesh::MeshRenderer::draw`]: el pipeline está compilado con
    /// depth test.
    pub fn draw(
        &mut self,
        ctx: &GpuContext,
        target: &crate::context::RenderTarget,
        mesh: &crate::mesh::GpuMesh,
        uniforms: &KnobUniforms,
    ) {
        let Some(depth_view) = target.depth_view.as_ref() else {
            debug_assert!(false, "KnobRenderer::draw requiere un RenderTarget con profundidad");
            return;
        };

        ctx.queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(uniforms));

        let mut encoder =
            ctx.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("hikaru_render::knob::draw"),
                });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hikaru_render::knob::pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            mesh.bind(&mut pass);
            pass.draw_indexed(0..mesh.index_count(), 0, 0..1);
        }

        ctx.queue.submit(Some(encoder.finish()));
    }
}

/// Sube la malla del knob a la GPU.
pub fn upload_knob(
    ctx: &GpuContext,
    label: &str,
    mesh: &KnobMesh,
) -> Result<crate::mesh::GpuMesh, KnobError> {
    validate(&mesh.vertices, &mesh.indices)?;

    let vertex_buffer = buffer_with_data(
        ctx,
        wgpu::BufferUsages::VERTEX,
        bytemuck::cast_slice(&mesh.vertices),
        label,
    );
    let index_buffer = buffer_with_data(
        ctx,
        wgpu::BufferUsages::INDEX,
        bytemuck::cast_slice(&mesh.indices),
        label,
    );

    Ok(crate::mesh::GpuMesh::from_buffers(
        vertex_buffer,
        index_buffer,
        mesh.indices.len() as u32,
    ))
}

/// Valida la geometría antes de subirla.
///
/// Misma razón que en [`crate::mesh::validate_geometry`]: wgpu no valida los
/// índices al crear el buffer, sólo al dibujar, y el síntoma es una lectura
/// fuera de rango adentro del driver.
pub fn validate(vertices: &[MeshVertex], indices: &[u32]) -> Result<(), KnobError> {
    if vertices.is_empty() || indices.is_empty() {
        return Err(KnobError::Empty);
    }
    if !indices.len().is_multiple_of(3) {
        return Err(KnobError::IndexCountNotMultipleOfThree { got: indices.len() });
    }
    if let Some(&bad) = indices.iter().find(|&&index| index as usize >= vertices.len()) {
        return Err(KnobError::IndexOutOfRange { index: bad, vertex_count: vertices.len() });
    }
    Ok(())
}

fn buffer_with_data(
    ctx: &GpuContext,
    usage: wgpu::BufferUsages,
    data: &[u8],
    label: &str,
) -> wgpu::Buffer {
    let buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: data.len().max(4) as u64,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    if !data.is_empty() {
        ctx.queue.write_buffer(&buffer, 0, data);
    }
    buffer
}

#[cfg(test)]
mod tests {
    use super::*;
    use naga::valid::{Capabilities, ValidationFlags, Validator};

    fn validate_wgsl(source: &str) {
        let module = naga::front::wgsl::parse_str(source)
            .unwrap_or_else(|error| panic!("WGSL no parsea: {error:?}"));
        Validator::new(ValidationFlags::all(), Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|error| panic!("WGSL no valida: {error:?}"));
    }

    #[test]
    fn knob_shader_is_valid_wgsl() {
        validate_wgsl(KNOB_SHADER);
    }

    #[test]
    fn uniform_buffer_matches_the_wgsl_struct() {
        // mat4x4 (64) + vec3 + f32 (16) + vec4 (16) + vec4 (16) = 112 bytes. Con
        // `min_binding_size: None` un desfasaje no lo detecta wgpu: el ángulo
        // se lee como color y el knob deja de girar.
        assert_eq!(std::mem::size_of::<KnobUniforms>(), 112);
    }

    #[test]
    fn the_default_knob_geometry_is_valid() {
        let mesh = KnobMesh::new(KnobMeshParams::default());
        assert_eq!(validate(&mesh.vertices, &mesh.indices), Ok(()));
        assert_eq!(mesh.indices.len() % 3, 0);
        assert!(!mesh.vertices.is_empty());
    }

    #[test]
    fn the_marker_is_tagged_for_the_fragment_shader() {
        // La marca se distingue por `uv.x`. Si se olvidara el tag, el fragment
        // shader la pintaría del color del cuerpo y el knob no señalaría nada.
        let mesh = KnobMesh::new(KnobMeshParams::default());
        let marked: Vec<&MeshVertex> = mesh.vertices.iter().filter(|v| v.uv[0] == 1.0).collect();
        // La cuña son dos triángulos: 4 vértices.
        assert_eq!(marked.len(), 4, "la marca tiene que ser una cuña de 4 vértices");
        assert!(marked.iter().all(|v| v.uv[0] > 0.5));
    }

    #[test]
    fn the_body_is_not_tagged_as_marker() {
        // Al revés del test anterior: si el cuerpo quedara con `uv.x = 1.0`, el
        // knob entero se vería del color de la marca.
        let mesh = KnobMesh::new(KnobMeshParams::default());
        assert!(mesh.vertices.iter().filter(|v| v.uv[0] == 0.0).count() > 4);
    }

    #[test]
    fn the_marker_stays_inside_the_disc() {
        // Si la marca se sale del radio, sobresale del canto y la silueta del
        // knob queda con un pico.
        let params = KnobMeshParams::default();
        let mesh = KnobMesh::new(params);
        for vertex in mesh.vertices.iter().filter(|v| v.uv[0] == 1.0) {
            let radial = (vertex.position[0].powi(2) + vertex.position[1].powi(2)).sqrt();
            assert!(
                radial <= params.radius * 0.95,
                "la marca se sale del disco: {radial}"
            );
            // Y por delante de la cara, que es lo que garantiza que se vea.
            assert!(vertex.position[2] > 0.0);
        }
    }

    #[test]
    fn every_vertex_is_finite_and_normalized() {
        // Una normal de longitud cero hace que el `normalize` del shader
        // devuelva NaN y el triángulo no se dibuje.
        let mesh = KnobMesh::new(KnobMeshParams::default());
        for vertex in &mesh.vertices {
            assert!(vertex.position.iter().all(|c| c.is_finite()), "{:?}", vertex.position);
            assert!(vertex.normal.iter().all(|c| c.is_finite()), "{:?}", vertex.normal);
            let length =
                (vertex.normal[0].powi(2) + vertex.normal[1].powi(2) + vertex.normal[2].powi(2))
                    .sqrt();
            assert!((length - 1.0).abs() < 1e-4, "normal sin normalizar: {length}");
        }
    }

    #[test]
    fn the_geometry_fits_the_disc_radius() {
        let params = KnobMeshParams::default();
        let mesh = KnobMesh::new(params);
        for vertex in &mesh.vertices {
            let radial = (vertex.position[0].powi(2) + vertex.position[1].powi(2)).sqrt();
            assert!(radial <= params.radius + f32::EPSILON, "un vértice se sale del radio");
        }
    }

    #[test]
    fn a_knob_with_three_segments_is_still_valid() {
        // Con 0 o 1 segmentos el abanico de la cara y las costuras del canto se
        // rompen. El piso de 3 es lo que lo evita.
        let params = KnobMeshParams { segments: 0, ..KnobMeshParams::default() };
        let mesh = KnobMesh::new(params);
        assert_eq!(validate(&mesh.vertices, &mesh.indices), Ok(()));
        assert!(mesh.indices.len() > 0);
    }

    #[test]
    fn degenerate_dimensions_do_not_produce_nan() {
        // Radio o profundidad cero: tiene que salir algo finito o el knob no se
        // dibuja y no hay ningún error que lo diga.
        for params in [
            KnobMeshParams { radius: 0.0, ..Default::default() },
            KnobMeshParams { depth: 0.0, ..Default::default() },
            KnobMeshParams { marker_width: 0.0, ..Default::default() },
        ] {
            let mesh = KnobMesh::new(params);
            assert!(mesh.vertices.iter().all(|v| v.position.iter().all(|c| c.is_finite())));
            assert_eq!(validate(&mesh.vertices, &mesh.indices), Ok(()));
        }
    }

    #[test]
    fn validation_rejects_broken_geometry() {
        assert_eq!(validate(&[], &[]), Err(KnobError::Empty));
        let vertex = MeshVertex { position: [0.0; 3], normal: [0.0, 0.0, 1.0], uv: [0.0; 2] };
        assert_eq!(
            validate(&[vertex], &[0, 1]),
            Err(KnobError::IndexCountNotMultipleOfThree { got: 2 })
        );
        assert_eq!(
            validate(&[vertex], &[0, 1, 9]),
            Err(KnobError::IndexOutOfRange { index: 1, vertex_count: 1 })
        );
    }

    #[test]
    fn the_vertex_layout_matches_the_shader_inputs() {
        // El pipeline declara Float32x3, Float32x3, Float32x2 y el WGSL lee
        // position, normal y uv. Se comparte `MeshVertex` con el pipeline de la
        // malla, así que el layout tiene que ser el mismo.
        assert_eq!(std::mem::size_of::<MeshVertex>(), 3 * 4 + 3 * 4 + 2 * 4);
    }
}
