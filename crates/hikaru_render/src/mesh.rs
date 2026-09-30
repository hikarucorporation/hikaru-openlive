// crates/hikaru_render/src/mesh.rs

//! Pipeline 3D inicial, pensado para la malla de la Wavetable.
//!
//! # Qué resuelve
//!
//! La vista de OpenWavetable muestra osciladores como volúmenes. La
//! representación más barata que se ve bien es una *cinta* (ribbon): la
//! forma de onda recorrida a lo largo de un eje, con un thickness en el eje
//! perpendicular. Con eso se obtiene una superficie 3D real —con volumen y
//! normales— sin necesidad de marching cubes ni de compute.
//!
//! [`WavetableMesh::from_waveform`] hace exactamente esa construcción a partir
//! de los samples de audio, y [`MeshRenderer`] la dibuja con iluminación
//! simple y buffer de profundidad.
//!
//! # Alcance de esta sesión
//!
//! Es la base estructural, no el look final. Está:
//!
//! - la geometría (cinta cerrada con normales y UVs),
//! - el pipeline con depth test y blending normal,
//! - la cámara y la matriz de view-projection (módulo [`crate::camera`]),
//!
//! y falta: animación, colores por oscilador, overlay de morph entre wavetables
//! y composición sobre el fondo 2D. Esas piezas se agregan encima sin cambiar
//! esta API.

use bytemuck::{Pod, Zeroable};

use crate::context::GpuContext;

/// Shader del pipeline de malla.
///
/// Lambert difuso con una luz puntual más un término de rim, para que el
/// volumen se lea sin necesidad de materiales PBR.
const MESH_SHADER: &str = r#"
struct Uniforms {
    // view * projection, en columna mayor.
    view_proj: mat4x4<f32>,
    // Dirección de la luz, normalizada, en espacio LOCAL (no de vista): la
    // normal del vértice no pasa por la view matrix, así que la luz tiene que
    // venir des-rotada por la cámara. Ver `Camera::local_light_dir`.
    light_dir: vec3<f32>,
    // Intensidad de la luz, 0..1.
    light_intensity: f32,
    // Tinte base de la malla (RGBA).
    tint: vec4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) local_normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs_main(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>,
           @location(2) uv: vec2<f32>) -> VsOut {
    var out: VsOut;
    out.clip_position = uniforms.view_proj * vec4<f32>(position, 1.0);
    out.local_normal = normal;
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let normal = normalize(in.local_normal);
    let light = normalize(uniforms.light_dir);

    // Lambert clásico, con `max` para que la cara opuesta a la luz no se hunda.
    let diffuse = max(dot(normal, light), 0.0) * uniforms.light_intensity;

    // Rim contra el eje del thickness (Z local): la cinta es una lámina a lo
    // largo de Z, así que las caras que lo ven de perfil son la silueta y es
    // justo lo que el rim tiene que resaltar.
    let rim = pow(1.0 - abs(dot(normal, vec3<f32>(0.0, 0.0, 1.0))), 2.0);

    // Brillo por vértice. El `uv.y` lo usa [`WavetableMesh::from_table`] como
    // factor de realce: 1.0 en el ciclo que está mirando el knob de índice y
    // menos en el resto, que es lo que da la lectura de "estoy parado en el
    // frame 7 de 64" sin rotular nada sobre la imagen.
    let shade = in.uv.y;

    let lit = 0.25 + 0.75 * diffuse;
    let color = vec4<f32>(uniforms.tint.rgb * (lit + 0.35 * rim) * shade, uniforms.tint.a);
    return color;
}
"#;

/// Vértice de la malla.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct MeshVertex {
    /// Posición en espacio local. El origen es el centro del oscilador.
    pub position: [f32; 3],
    /// Normal. Se calcula al construir la malla, no se interpola.
    pub normal: [f32; 3],
    /// UV. `x` recorre el lazo de la forma de onda; `y` es el factor de brillo
    /// del ciclo, 1.0 para el que está seleccionado (ver
    /// [`WavetableMesh::from_table`]).
    pub uv: [f32; 2],
}

/// Parámetros de construcción de la cinta.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WavetableMeshParams {
    /// Ancho total de la cinta en unidades locales.
    pub width: f32,
    /// Alto total: la forma de onda se escala a esta caja.
    pub height: f32,
    /// Profundidad (thickness) de la cinta.
    pub thickness: f32,
}

impl Default for WavetableMeshParams {
    fn default() -> Self {
        Self { width: 260.0, height: 100.0, thickness: 24.0 }
    }
}

/// Brillo del ciclo que está bajo el knob de índice.
const ACTIVE_SHADE: f32 = 1.0;

/// Brillo de los ciclos alejados del seleccionado.
///
/// Deliberadamente no es cero: los ciclos de alrededor tienen que seguir siendo
/// legibles, porque son los que muestran que la tabla tiene más de un frame y
/// hacia dónde se está moviendo el knob.
const DIM_SHADE: f32 = 0.32;

/// Distancia, en cantidad de ciclos, a la que el realce se apaga del todo.
///
/// Con 1.5 se superponen los halos de dos ciclos vecinos, así que mover el knob
/// de a un frame no produce un salto de brillo sino un barrido continuo.
const SHADE_FALLOFF: f32 = 1.5;

/// Porción del hueco entre ciclos que ocupa el espesor de cada cinta.
///
/// El resto es aire: es lo que hace que dos ciclos se lean como dos volúmenes
/// y no como una sola capa.
const RIBBON_FILL: f32 = 0.55;

/// Malla triangulada en la GPU.
pub struct GpuMesh {
    pub(crate) vertex_buffer: wgpu::Buffer,
    pub(crate) index_buffer: wgpu::Buffer,
    index_count: u32,
}

impl GpuMesh {
    /// Enlaza los buffers de una malla ya subida.
    ///
    /// Lo usa [`crate::knob`], que tiene su propia geometría pero comparte el
    /// layout de vértice con la malla de la Wavetable: mantener los buffers
    /// accesibles entre módulos es lo que evita que el layout se desincronice.
    pub(crate) fn bind(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
    }

    /// Sube una malla a la GPU.
    pub fn new(
        ctx: &GpuContext,
        label: &str,
        vertices: &[MeshVertex],
        indices: &[u32],
    ) -> Result<Self, MeshError> {
        validate_geometry(vertices, indices)?;

        let vertex_buffer = create_buffer_with_data(
            ctx,
            wgpu::BufferUsages::VERTEX,
            bytemuck::cast_slice(vertices),
            label,
        );
        let index_buffer = create_buffer_with_data(
            ctx,
            wgpu::BufferUsages::INDEX,
            bytemuck::cast_slice(indices),
            label,
        );

        Ok(Self::from_buffers(vertex_buffer, index_buffer, indices.len() as u32))
    }

    /// Envuelve buffers ya subidos, sin volver a crearlos.
    ///
    /// Lo usa [`crate::knob`], que construye su propia geometría pero comparte el
    /// pipeline (y por lo tanto el layout de vértice) con la malla de la
    /// Wavetable. Duplicar el struct obligaría a que los dos buffers de vértice
    /// compartieran el layout, que es justo lo que no puede cambiar sin romper
    /// los dos.
    pub fn from_buffers(
        vertex_buffer: wgpu::Buffer,
        index_buffer: wgpu::Buffer,
        index_count: u32,
    ) -> Self {
        Self { vertex_buffer, index_buffer, index_count }
    }

    /// Cantidad de índices que se van a dibujar.
    pub fn index_count(&self) -> u32 {
        self.index_count
    }
}

/// Cinta cerrada que representa una forma de onda.
///
/// Se construye Offline: la UI pasa los samples de audio y después sólo dibuja.
#[derive(Debug, Clone)]
pub struct WavetableMesh {
    /// Vértices listos para la GPU.
    pub vertices: Vec<MeshVertex>,
    /// Índices, en tripletes.
    pub indices: Vec<u32>,
}

impl WavetableMesh {
    /// Construye la cinta a partir de los samples de una forma de onda.
    ///
    /// `waveform` son los valores del oscilador. Se re-muestrea a
    /// `frame_width` columnas para que la resolución del mesh no dependa de la
    /// resolución con la que se calculó la wavetable: una tabla de 2048 samples
    /// y una de 512 dan el mismo mesh.
    ///
    /// El waveform es periódico: la cinta se cierra sobre sí misma
    /// (columna `n` comparte posición con la columna 0), que es lo que
    /// corresponde a un oscilador de audio.
    ///
    /// Devuelve `Err(MeshError::WaveformTooShort)` si no hay muestras para
    /// trabajar.
    pub fn from_waveform(
        waveform: &[f32],
        frame_width: u32,
        params: WavetableMeshParams,
    ) -> Result<Self, MeshError> {
        if waveform.is_empty() {
            return Err(MeshError::WaveformTooShort);
        }
        // Con 2 columnas se forma un único cuadrilátero cerrado, que es el
        // mínimo para que la cinta tenga volumen.
        let columns = frame_width.max(2) as usize;

        let mut vertices = Vec::with_capacity((columns + 1) * 2);
        let mut indices = Vec::with_capacity(columns * 6);

        push_ribbon(
            &mut vertices,
            &mut indices,
            waveform,
            columns,
            params,
            0.0,
            params.thickness,
            ACTIVE_SHADE,
        );

        Ok(Self { vertices, indices })
    }

    /// Apila los ciclos reales de una wavetable a lo largo del eje Z.
    ///
    /// Es la vista que muestran Serum, Vital y Bitwig: no un ciclo suelto, sino
    /// la matriz completa con el frame seleccionado destacado. El eje Z *es* el
    /// índice del frame, así que la profundidad de la pila dice cuántas formas
    /// distintas tiene la tabla.
    ///
    /// # Cómo se reparten
    ///
    /// `table` es la tabla entera tal como la entrega
    /// `wavetable_io`: `frames` bloques consecutivos de `frame_len` muestras.
    /// Se apilan como máximo `max_frames` de ellos, repartidos de forma uniforme
    /// en `params.thickness`: el hueco entre ciclos es lo que hace legible la
    /// profundidad, y un hueco de un píxel no se ve.
    ///
    /// Con más ciclos que `max_frames` no se dibujan todos, y los que se dibujan
    /// representan la tabla completa: se reparte el rango, no se recortan los
    /// primeros. Un archivo de dos minutos son 2000 ciclos, y 2000 cintas de 256
    /// columnas no entran en un panel de 500 píxeles ni aportan algo que se pueda
    /// distinguir.
    ///
    /// # El realce
    ///
    /// `active` es la posición continua del frame que se está mirando, no un
    /// índice: el halo de luz sigue al knob de morph de forma continua, así que
    /// barrer la matriz se ve como un barrido y no como una serie de saltos. El
    /// brillo viaja en `uv.y` (ver [`ACTIVE_SHADE`]) y lo aplica el fragment
    /// shader.
    pub fn from_table(
        table: &[f32],
        frame_len: usize,
        active: f32,
        max_frames: usize,
        frame_width: u32,
        params: WavetableMeshParams,
    ) -> Result<Self, MeshError> {
        if table.is_empty() || frame_len == 0 {
            return Err(MeshError::WaveformTooShort);
        }
        let columns = frame_width.max(2) as usize;
        let available = table.len() / frame_len;
        let count = available.max(1).min(max_frames.max(1));

        let mut vertices = Vec::with_capacity(count * (columns + 1) * 2);
        let mut indices = Vec::with_capacity(count * columns * 6);

        // La tabla completa se mapea sobre `count` filas: si hay más ciclos de
        // los que entran, cada fila representa un salto de la tabla real en vez
        // de ser un ciclo contiguo.
        let stride = available as f32 / count as f32;
        let spacing = if count > 1 { params.thickness / count as f32 } else { params.thickness };
        let depth = (spacing * RIBBON_FILL).max(f32::EPSILON);
        let active = if active.is_finite() { active.clamp(0.0, (available - 1).max(0) as f32) } else { 0.0 };

        for slot in 0..count {
            // El frame se toma de la tabla real, redondeando al ciclo que le
            // toca: es el mismo criterio con el que el knob de índice recorre
            // la matriz, así que lo que brilla y lo que se ve coinciden.
            let source = (slot as f32 * stride).floor() as usize;
            let start = (source * frame_len).min(table.len());
            let end = (start + frame_len).min(table.len());
            let Some(frame) = table.get(start..end) else {
                break;
            };
            if frame.is_empty() {
                continue;
            }

            // El ciclo seleccionado va al frente del volumen y el último al
            // fondo: con la cámara inclinada 30° se ve la pila completa, y el
            // realce marca en qué punto de la matriz se está.
            let t = if count > 1 { slot as f32 / (count - 1) as f32 } else { 0.0 };
            let z_center = -params.thickness * 0.5 + t * params.thickness;

            // `active` viene en ciclos de la tabla real (0..available-1) y las
            // filas son un muestreo de `count` puntos. Se mapea el rango
            // **completo** al rango de filas, y no dividiendo por el paso.
            //
            // La diferencia importa en los extremos: con 256 ciclos muestreados
            // a 24 filas el paso es 10.67, y `active / paso` llega a 23.9 en el
            // último ciclo. Ninguna fila está a 23.9, así que la fila 23 queda
            // a distancia 0.9 y el realce nunca llega al máximo: el último ciclo
            // se veía al 59% y era indistinguible del penúltimo. Con el mapeo
            // proporcional, el último ciclo cae exacto en la última fila.
            let row_of_active = if available > 1 && count > 1 {
                active / (available - 1) as f32 * (count - 1) as f32
            } else {
                0.0
            };
            let distance = (slot as f32 - row_of_active).abs();
            let shade = ACTIVE_SHADE
                + (DIM_SHADE - ACTIVE_SHADE) * (distance / SHADE_FALLOFF).clamp(0.0, 1.0);

            push_ribbon(
                &mut vertices,
                &mut indices,
                frame,
                columns,
                params,
                z_center,
                depth,
                shade,
            );
        }

        if vertices.is_empty() {
            return Err(MeshError::Empty);
        }

        Ok(Self { vertices, indices })
    }

    /// Sube la cinta a la GPU.
    pub fn upload(&self, ctx: &GpuContext, label: &str) -> Result<GpuMesh, MeshError> {
        GpuMesh::new(ctx, label, &self.vertices, &self.indices)
    }
}

/// Valida una malla antes de subirla a la GPU.
///
/// Se separa de [`GpuMesh::new`] a propósito: son comprobaciones puras, y
/// tenerlas aparte permite testearlas sin levantar un contexto de GPU. También
///centraliza por qué la validación existe: wgpu no valida los índices al crear
/// el buffer, sólo al dibujar, y el síntoma de un índice malo es una lectura
/// fuera de rango dentro del driver.
pub fn validate_geometry(
    vertices: &[MeshVertex],
    indices: &[u32],
) -> Result<(), MeshError> {
    if vertices.is_empty() || indices.is_empty() {
        return Err(MeshError::Empty);
    }
    if !indices.len().is_multiple_of(3) {
        return Err(MeshError::IndexCountNotMultipleOfThree { got: indices.len() });
    }
    if let Some(&bad) = indices.iter().find(|&&index| index as usize >= vertices.len()) {
        return Err(MeshError::IndexOutOfRange { index: bad, vertex_count: vertices.len() });
    }
    Ok(())
}

/// Re-muestrea un waveform periódico en la posición `u` de `[0, 1)`.
///
/// Interpolación lineal: suficiente para reconstruir la forma visual, y evita
/// tener que copiar el waveform a una tabla de resolución fija.
fn resample(waveform: &[f32], u: f32) -> f32 {
    let len = waveform.len();
    let position = u * len as f32;
    // `floor` puede dar `len` justo en el borde de 1.0, así que se envuelve.
    let index0 = position.floor() as usize % len;
    let index1 = (index0 + 1) % len;
    let fraction = position - position.floor();

    waveform[index0] * (1.0 - fraction) + waveform[index1] * fraction
}

/// Agrega una cinta cerrada a los buffers de la malla.
///
/// La geometría está en unidades locales, con el centro de la cinta en el
/// origen: la pila de [`WavetableMesh::from_table`] la corre después con
/// `z_center`, así que el `peak` de la forma de onda es el de su propio ciclo y
/// no el de la tabla entera. Normalizar contra el pico global haría que un
/// frame casi en silencio se viera plano al lado de uno fuerte, que es
/// justamente el detalle que un selector de wavetable tiene que mostrar.
///
/// `z_center` y `depth` separan la cinta de las demás en el eje Z; `shade` es
/// el factor de brillo que aplica el fragment shader.
fn push_ribbon(
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u32>,
    waveform: &[f32],
    columns: usize,
    params: WavetableMeshParams,
    z_center: f32,
    depth: f32,
    shade: f32,
) {
    let half_width = params.width * 0.5;
    let half_height = params.height * 0.5;
    let half_depth = depth * 0.5;

    // Escala para que el pico a pico del waveform llene la caja, con un
    // pequeño margen para que un sample a 1.0 no quede pegado al borde.
    let peak = waveform
        .iter()
        .fold(0.0f32, |acc, sample| acc.max(sample.abs()))
        .max(f32::EPSILON);

    let base = vertices.len() as u32;
    // Malla de (columns + 1) x 2: la última columna repite la primera para
    // cerrar el lazo.
    for column in 0..=columns {
        // La UV recorre el lazo completo, de 0 a 1, para que la última columna
        // tenga `uv.x == 1.0` y la textura no muestre una costura.
        let uv_u = column as f32 / columns as f32;
        // La posición y la muestra, en cambio, dan la vuelta: la última
        // columna vuelve al principio. Si `u` valiera 1.0, la cinta terminaría
        // en `+width/2` y el lazo quedaría abierto con un segmento de más.
        let phase_u = (column % columns) as f32 / columns as f32;

        let sample = resample(waveform, phase_u);
        let x = -half_width + phase_u * params.width;
        // Normalizado a -1..1, centrando la forma de onda en la caja.
        let y = sample / peak * 0.9 * half_height;

        // Cara delantera y trasera.
        vertices.push(MeshVertex {
            position: [x, y, z_center - half_depth],
            normal: [0.0, 0.0, -1.0],
            uv: [uv_u, shade],
        });
        vertices.push(MeshVertex {
            position: [x, y, z_center + half_depth],
            normal: [0.0, 0.0, 1.0],
            uv: [uv_u, shade],
        });
    }

    // Cada segmento del lazo aporta dos triángulos que unen la cara delantera
    // con la trasera. El orden de los vértices es irrelevante porque el pipeline
    // no hace culling (ver [`MeshRenderer::new`]).
    let ring = columns + 1;
    for column in 0..columns {
        let next = (column + 1) % ring;
        // Cada columna aporta un par de vértices (frontal, posterior), corridos
        // por `base` porque la pila mete varias cintas en el mismo buffer.
        let front_a = base + (column as u32) * 2;
        let back_a = front_a + 1;
        let front_b = base + (next as u32) * 2;
        let back_b = front_b + 1;

        indices.extend_from_slice(&[front_a, back_a, front_b]);
        indices.extend_from_slice(&[front_b, back_a, back_b]);
    }
}

/// Buffers de uniforms del pipeline de malla.
///
/// El layout tiene que seguir en sync con el `struct Uniforms` del WGSL: 64
/// bytes de matriz, 16 de luz y 16 de tinte. Agregar un campo lo cambia de
/// tamaño, y como `min_binding_size` es `None` en el layout, un desfasaje no lo
/// detecta wgpu: el shader lee floats desplazados y la malla sale con la
/// iluminación corrida.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct MeshUniforms {
    /// Matriz `projection * view`, en columna mayor. La arma
    /// [`crate::camera::Camera::view_proj`].
    pub view_proj: [[f32; 4]; 4],
    /// Dirección de la luz, normalizada, en espacio **local** de la malla.
    pub light_dir: [f32; 3],
    /// Intensidad de la luz.
    pub light_intensity: f32,
    /// Tinte base.
    pub tint: [f32; 4],
}

impl Default for MeshUniforms {
    /// Defaults con la cámara de frente, no con la matriz identidad.
    ///
    /// La identidad "funciona" (el shader no falla) pero aplana la cinta contra
    /// el plano z = 0: se ve una línea, no un volumen. La cámara de frente
    /// hace que el estado neutro ya sea algo que se pueda mirar.
    fn default() -> Self {
        crate::camera::Camera::front().uniforms(1.0, [0.35, 0.85, 1.0, 1.0])
    }
}

impl MeshUniforms {
    /// U uniforms para una cámara y un viewport de `aspect` (ancho / alto).
    pub fn with_camera(
        camera: &crate::camera::Camera,
        aspect: f32,
        tint: [f32; 4],
    ) -> Self {
        camera.uniforms(aspect, tint)
    }
}

/// Pipeline de malla con depth test.
pub struct MeshRenderer {
    pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
}

impl MeshRenderer {
    /// Crea el pipeline 3D.
    ///
    /// A diferencia de [`crate::quad::QuadRenderer`], éste sí lleva
    /// `depth_stencil`: es geometría con volumen, y sin depth test las caras
    /// traseras se painted encima de las delanteras.
    pub fn new(
        ctx: &GpuContext,
        label: &str,
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
    ) -> Self {
        let uniform_layout = ctx.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hikaru_render::mesh::uniforms"),
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

        let shader = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("hikaru_render::mesh::shader"),
                source: wgpu::ShaderSource::Wgsl(MESH_SHADER.into()),
            });

        let pipeline_layout = ctx.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hikaru_render::mesh::layout"),
            bind_group_layouts: &[Some(&uniform_layout)],
            // `var<immediate>` no se usa en este shader.
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
                // Sin culling: la cinta son dos láminas opuestas, no un volumen
                // cerrado, y la vista de OpenWavetable la muestra desde los dos
                // lados. Con back-face culling una de las dos desaparece.
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
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
            label: Some("hikaru_render::mesh::uniforms"),
            size: std::mem::size_of::<MeshUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let uniform_bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hikaru_render::mesh::uniforms"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        Self { pipeline, uniform_buffer, uniform_bind_group }
    }

    /// Dibuja la malla sobre el target dado.
    ///
    /// `depth_view` es obligatorio: es un campo opcional justamente para poder
    /// reutilizar un target sin profundidad (thumbnails), pero el pipeline está
    /// compilado con depth test, así que acá siempre hace falta.
    pub fn draw(
        &mut self,
        ctx: &GpuContext,
        target: &crate::context::RenderTarget,
        mesh: &GpuMesh,
        uniforms: &MeshUniforms,
    ) {
        let Some(depth_view) = target.depth_view.as_ref() else {
            // Sin depth attachment el render pass es inválido. Es un error de
            // programación, no una condición de runtime esperable.
            debug_assert!(false, "MeshRenderer::draw requiere un RenderTarget con profundidad");
            return;
        };

        ctx.queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(uniforms));

        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("hikaru_render::mesh::draw"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hikaru_render::mesh::pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // La malla se compone sobre el fondo, así que el color
                        // se preserva; la profundidad sí se limpia.
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
            pass.draw_indexed(0..mesh.index_count, 0, 0..1);
        }

        ctx.queue.submit(Some(encoder.finish()));
    }
}

/// Crea un buffer con datos ya inicializados.
///
/// Se sube con `write_buffer` en vez de `mapped_at_creation` porque las mallas
/// son chicas (una cinta son unos pocos KiB) y así el camino de código es uno
/// solo.
fn create_buffer_with_data(
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

/// Errores al construir o subir una malla.
#[derive(Debug, Clone, PartialEq)]
pub enum MeshError {
    /// La malla no tiene vértices o índices.
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
    /// La forma de onda llegó vacía.
    WaveformTooShort,
}

impl std::fmt::Display for MeshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MeshError::Empty => write!(f, "la malla está vacía"),
            MeshError::IndexCountNotMultipleOfThree { got } => {
                write!(f, "{got} índices no es múltiplo de 3")
            }
            MeshError::IndexOutOfRange { index, vertex_count } => {
                write!(f, "índice {index} fuera de rango ({vertex_count} vértices)")
            }
            MeshError::WaveformTooShort => write!(f, "la forma de onda no tiene muestras"),
        }
    }
}

impl std::error::Error for MeshError {}

#[cfg(test)]
mod tests {
    use super::*;
    use naga::valid::{Capabilities, ValidationFlags, Validator};

    /// Valida el WGSL del pipeline sin necesitar una GPU.
    ///
    /// `cargo check` no compila shaders, así que un error acá sólo aparecería en
    /// runtime, con la aplicación ya andando.
    #[test]
    fn mesh_shader_is_valid_wgsl() {
        let module = naga::front::wgsl::parse_str(MESH_SHADER)
            .unwrap_or_else(|error| panic!("WGSL no parsea: {error:?}"));
        Validator::new(ValidationFlags::all(), Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|error| panic!("WGSL no valida: {error:?}"));
    }

    /// Onda de 8 muestras, suficiente para las pruebas de geometría.
    fn test_waveform() -> Vec<f32> {
        vec![0.0, 1.0, 0.0, -1.0, 0.0, 0.5, 0.0, -0.5]
    }

    fn dummy_vertex() -> MeshVertex {
        MeshVertex { position: [0.0; 3], normal: [0.0, 0.0, 1.0], uv: [0.0, 0.0] }
    }

    #[test]
    fn vertex_layout_matches_the_shader_inputs() {
        // El pipeline declara Float32x3, Float32x3, Float32x2 en los atributos
        // 0, 1 y 2; el WGSL lee `@location(0) position`, `(1) normal`,
        // `(2) uv`. Agregar un campo a `MeshVertex` obliga a tocar ambos lados.
        assert_eq!(std::mem::size_of::<MeshVertex>(), 3 * 4 + 3 * 4 + 2 * 4);
    }

    #[test]
    fn every_index_points_at_a_real_vertex() {
        let mesh = WavetableMesh::from_waveform(&test_waveform(), 16, Default::default()).unwrap();
        assert_eq!(validate_geometry(&mesh.vertices, &mesh.indices), Ok(()));
    }

    #[test]
    fn index_count_is_a_multiple_of_three() {
        let mesh = WavetableMesh::from_waveform(&test_waveform(), 16, Default::default()).unwrap();
        assert_eq!(mesh.indices.len() % 3, 0);
        // Dos triángulos por segmento del lazo.
        assert_eq!(mesh.indices.len(), 16 * 6);
    }

    #[test]
    fn the_loop_closes_on_itself() {
        // El oscilador es periódico: la última columna tiene que caer en la
        // misma posición que la primera, o la cinta muestra una costura.
        let columns = 16u32;
        let mesh =
            WavetableMesh::from_waveform(&test_waveform(), columns, Default::default()).unwrap();
        assert_eq!(mesh.vertices[0].position, mesh.vertices[columns as usize * 2].position);
    }

    #[test]
    fn geometry_respects_the_requested_bounds() {
        let params =
            WavetableMeshParams { width: 300.0, height: 200.0, thickness: 20.0 };
        let mesh = WavetableMesh::from_waveform(&test_waveform(), 8, params).unwrap();

        for vertex in &mesh.vertices {
            let [x, y, z] = vertex.position;
            assert!(x >= -params.width / 2.0 && x <= params.width / 2.0, "x fuera de caja: {x}");
            // El margen de 0.9 deja los picos dentro de la caja.
            assert!(y.abs() <= params.height / 2.0, "y fuera de caja: {y}");
            assert!(z.abs() <= params.thickness / 2.0, "z fuera de caja: {z}");
        }
    }

    #[test]
    fn a_silent_waveform_stays_inside_the_box() {
        // Un waveform de ceros tiene pico 0. El `max(f32::EPSILON)` del
        // escalado evita que la división produzca NaN. Es el caso borde más
        // probable: una wavetable todavía no cargada.
        let mesh = WavetableMesh::from_waveform(&[0.0; 4], 8, Default::default()).unwrap();
        assert!(mesh.vertices.iter().all(|v| v.position[1] == 0.0));
        assert!(mesh.vertices.iter().all(|v| v.position.iter().all(|c| c.is_finite())));
    }

    #[test]
    fn minimum_width_yields_a_valid_mesh() {
        // Una sola columna no puede cerrar el lazo; el piso de 2 columnas es
        // lo que garantiza que los índices queden dentro del rango.
        let mesh = WavetableMesh::from_waveform(&test_waveform(), 1, Default::default()).unwrap();
        assert_eq!(mesh.indices.len(), 2 * 6);
        assert_eq!(validate_geometry(&mesh.vertices, &mesh.indices), Ok(()));
    }

    #[test]
    fn empty_waveform_is_rejected() {
        let error = WavetableMesh::from_waveform(&[], 16, Default::default()).unwrap_err();
        assert!(matches!(error, MeshError::WaveformTooShort));
    }

    #[test]
    fn resampling_wraps_around_the_period() {
        let waveform = [1.0, 2.0];
        // u = 1.0 es el mismo punto del waveform que u = 0.0: el oscilador
        // empieza y termina en la misma fase.
        assert!((resample(&waveform, 0.0) - resample(&waveform, 1.0)).abs() < 1e-6);
        // Con dos muestras, la fase 0.5 cae en `u = 0.25` (u mapea a fase
        // `u * len`). Ahí sí va la mitad del camino entre ambas muestras.
        assert!((resample(&waveform, 0.25) - 1.5).abs() < 1e-6);
    }

    #[test]
    fn validation_rejects_broken_geometry() {
        assert!(matches!(validate_geometry(&[], &[]), Err(MeshError::Empty)));
        assert!(matches!(
            validate_geometry(&[dummy_vertex()], &[0, 1]),
            Err(MeshError::IndexCountNotMultipleOfThree { got: 2 })
        ));
        // Con un solo vértice, el primer índice fuera de rango es el 1 (no el
        // 9): el error reporta el primero que encuentra, no el peor.
        assert!(matches!(
            validate_geometry(&[dummy_vertex()], &[0, 1, 9]),
            Err(MeshError::IndexOutOfRange { index: 1, vertex_count: 1 })
        ));
    }

    #[test]
    fn mesh_params_default_to_the_open_wavetable_viewport() {
        // 260x100 es el tamaño que ya usa `WavetableOscillator::new` en
        // hikaru_gui; mantenerlo alineado evita que la malla aparezca escalada
        // dentro del recuadro.
        let params = WavetableMeshParams::default();
        assert_eq!(params.width, 260.0);
        assert_eq!(params.height, 100.0);
    }

    #[test]
    fn uniform_buffer_matches_the_wgsl_struct() {
        // mat4x4 (64) + vec3 + f32 (16) + vec4 (16) = 96 bytes. Con
        // `min_binding_size: None` en el layout, wgpu no valida esto: el
        // síntoma es iluminación corrida.
        assert_eq!(std::mem::size_of::<MeshUniforms>(), 96);
    }

    #[test]
    fn default_uniforms_are_a_real_camera_not_the_identity() {
        // Con la identidad la cinta se aplana contra el plano z = 0 y se ve
        // una línea. El default tiene que ser una cámara usable.
        let uniforms = MeshUniforms::default();
        assert_ne!(uniforms.view_proj, crate::camera::identity());

        // Y el origen tiene que caer dentro de la pantalla con ella.
        let (clip, w) = crate::camera::transform_point(&uniforms.view_proj, [0.0; 3]);
        assert!(w > 0.0, "el origen quedó detrás de la cámara");
        assert!(clip[0].abs() < w && clip[1].abs() < w, "{clip:?} / {w}");
    }
}
