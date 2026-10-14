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

use crate::camera::Mat4;
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
    // Ancho del suavizado de borde en píxeles (ver `RenderSettings::aa_feather`):
    // 0 es trazo duro. Llega por uniforme para no reconstruir geometría al
    // mover el slider del panel.
    aa_feather: f32,
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

    // Antialiasing analítico del borde: `uv.x` es la coordenada transversal
    // de la línea (-1..+1, ver `push_terrain_line` y `push_ribbon`). El alfa
    // cae a 0 en `aa_feather` píxeles (vía `fwidth`) en vez de serruchar el
    // borde, y las líneas lejanas —cuya proyección es más fina que el píxel—
    // se atenúan en vez de titilar o saltearse. Funciona con el blending
    // normal del pipeline (MSAA real exigiría texturas multimuestra en el
    // `RenderTarget`, que es de `context`, no de este pipeline).
    let edge = 1.0 - abs(in.uv.x);
    let aa = clamp(edge / (fwidth(in.uv.x) * uniforms.aa_feather + 1e-6), 0.0, 1.0);

    let lit = 0.25 + 0.75 * diffuse;
    let color = vec4<f32>(uniforms.tint.rgb * (lit + 0.35 * rim) * shade, uniforms.tint.a * aa);
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
    /// UV. `x` es la coordenada transversal de la línea (-1 en un borde, +1 en
    /// el otro): el fragment shader la usa para el antialiasing del borde.
    /// `y` es el factor de brillo del ciclo, 1.0 para el que está seleccionado
    /// (ver [`WavetableMesh::from_table`]).
    pub uv: [f32; 2],
}

/// Parámetros de construcción de la cinta.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WavetableMeshParams {
    /// Ancho total de la cinta en unidades locales.
    pub width: f32,
    /// Alto total: la forma de onda se escala a esta caja.
    pub height: f32,
    /// Profundidad en unidades locales. En [`WavetableMesh::from_waveform`] es
    /// el grosor de la cinta aislada; en [`WavetableMesh::from_table`] es la
    /// extensión total del terreno en el eje de profundidad (Z): se reparte a
    /// spacing constante entre las líneas (ver [`TERRAIN_LINE_WIDTH`]) y la
    /// serie completa cabe en la caja con aire entre ondas.
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

/// Ancho de cada línea del terreno, en unidades locales.
///
/// Constante en el plano de la onda y perpendicular a la curva (ver
/// [`push_terrain_line`]): la línea mide lo mismo mire hacia donde mire la
/// pendiente, como en Vital/Serum. Son 3u frente a 260 de ancho y 96 de alto:
/// con cuerpo continuo para que la forma no se punteé al componer el
/// offscreen de 768px dentro del módulo (~210px, el downscale se come las
/// líneas de 1px), y con aire de sobra entre filas para que los ciclos se
/// lean como líneas independientes.
///
/// Es `pub` porque el encuadre 2D de la UI (`render.rs`) necesita la media
/// extensión en Z de una línea plana para enmarcarla sin aire de más.
pub const TERRAIN_LINE_WIDTH: f32 = 3.0;

/// Modo del visor de wavetable: terreno 3D o ciclo 2D de diagnóstico.
///
/// Vive en el crate de render (y no sólo como bool en la UI) porque decide qué
/// geometría construye [`WavetableMesh`] y entra en la clave de caché del
/// visor: 2D y 3D con los mismos samples son imágenes distintas y el modo
/// tiene que invalidar el render.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderMode {
    /// El array completo en perspectiva (ver [`WavetableMesh::from_table`]).
    #[default]
    Mode3D,
    /// Un solo ciclo plano de frente (ver [`WavetableMesh::from_active_line`]):
    /// diagnóstico de que el `.wav` se lee e interpola bien antes del 3D.
    Mode2D,
}

impl RenderMode {
    /// Rótulo corto para el botón del visor.
    pub fn label(self) -> &'static str {
        match self {
            RenderMode::Mode3D => "3D",
            RenderMode::Mode2D => "2D",
        }
    }

    /// El otro modo: lo que pone el toggle del visor.
    pub fn toggle(self) -> Self {
        match self {
            RenderMode::Mode3D => RenderMode::Mode2D,
            RenderMode::Mode2D => RenderMode::Mode3D,
        }
    }
}

/// Columnas mínimas por fila del terreno.
///
/// La UI alimenta 512 (ver `WAVETABLE_COLUMNS` en `hikaru_gui`), de sobra para
/// 2048 samples por ciclo con interpolación lineal, pero la malla no depende
/// de quién la llame: con menos de ~128 columnas las pendientes empinadas se
/// vuelven vértices angulosos visibles. El piso lo garantiza por construcción
/// sin cambiar la topología (sólo la densidad).
const TERRAIN_MIN_COLUMNS: u32 = 128;

/// Ancho de trazo saneado del panel: finito y dentro de un rango que ni
/// desaparece (invisible) ni tapa a las filas vecinas.
fn sanitize_width(value: f32) -> f32 {
    if value.is_finite() { value.clamp(0.5, 12.0) } else { TERRAIN_LINE_WIDTH }
}

/// Fracción del viewport que ocupa el terreno encuadrado.
const TERRAIN_FILL: f32 = 0.85;

/// Atenuación del fondo por defecto: la última fila rinde `1 - esto` del
/// brillo de la primera, a igual distancia de `WT POS`. Es leve a propósito:
/// el fondo sigue legible como referencia y el realce del activo sigue
/// mandando. El panel de settings la expone como slider (ver
/// [`RenderSettings::depth_fade`]).
const TERRAIN_DEPTH_FADE: f32 = 0.35;

/// Yaw por defecto del terreno, en grados. Ver [`RenderSettings::yaw_deg`].
const TERRAIN_YAW_DEG: f32 = 15.0;

/// Pitch por defecto del terreno, en grados. Ver [`RenderSettings::pitch_deg`].
const TERRAIN_PITCH_DEG: f32 = 25.0;

/// Tipo de malla del terreno 3D: lo que dibuja cada fila (y, en `Solid`, lo
/// que las conecta).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MeshType {
    /// Líneas de ancho constante (ver [`push_terrain_line`]): el trazo Vital.
    #[default]
    Wireframe,
    /// Cintas con volumen mínimo (tubo en el plano X-Y, fino en Z): la línea
    /// con cuerpo y sombreado en la silueta.
    Ribbon,
    /// Superficie continua que conecta las filas (heightfield X-Z con altura
    /// en Y): el terreno sólido.
    Solid,
}

impl MeshType {
    /// Rótulo corto para el selector del panel.
    pub fn label(self) -> &'static str {
        match self {
            MeshType::Wireframe => "Líneas",
            MeshType::Ribbon => "Cintas",
            MeshType::Solid => "Sólido",
        }
    }

    /// El siguiente tipo, para el botón cíclico del panel.
    pub fn cycle(self) -> Self {
        match self {
            MeshType::Wireframe => MeshType::Ribbon,
            MeshType::Ribbon => MeshType::Solid,
            MeshType::Solid => MeshType::Wireframe,
        }
    }
}

/// Parámetros de look del visor de wavetable, editables en vivo desde el
/// panel "Hikaru OpenWavetable Settings" de la UI.
///
/// Es `Copy` a propósito: viaja por valor dentro del pedido de render y entra
/// en la clave de caché, así que mover un slider invalida el frame cacheado y
/// re-renderiza con los valores nuevos sin más plomería.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderSettings {
    /// Grosor del trazo en 3D, en unidades locales (ver `TERRAIN_LINE_WIDTH`
    /// como referencia). En `Ribbon` es también la profundidad del tubo.
    pub line_width: f32,
    /// Grosor del trazo en el modo 2D de diagnóstico.
    pub line_width_2d: f32,
    /// Multiplicador del espaciado en profundidad: <1 junta los sub-frames,
    /// >1 los separa.
    pub depth_scale: f32,
    /// Yaw de la vista 3D en grados (desvío lateral del fondo).
    pub yaw_deg: f32,
    /// Pitch de la vista 3D en grados (cuánto sube el fondo en pantalla).
    pub pitch_deg: f32,
    /// Fade back-to-front: la última fila rinde `1 - esto` del brillo de la
    /// primera, a igual distancia de `WT POS`.
    pub depth_fade: f32,
    /// Qué geometría se dibuja por fila en 3D.
    pub mesh: MeshType,
    /// Ancho del suavizado de borde en píxeles (0 = trazo duro). Llega al
    /// shader como uniforme, así que no reconstruye geometría.
    pub aa_feather: f32,
    /// Multiplicador del tinte (brillo/glow general del visor).
    pub glow: f32,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            line_width: TERRAIN_LINE_WIDTH,
            line_width_2d: TERRAIN_LINE_WIDTH,
            depth_scale: 1.0,
            yaw_deg: TERRAIN_YAW_DEG,
            pitch_deg: TERRAIN_PITCH_DEG,
            depth_fade: TERRAIN_DEPTH_FADE,
            mesh: MeshType::default(),
            aa_feather: 1.0,
            glow: 1.0,
        }
    }
}

impl RenderSettings {
    /// Firma estable para la clave de caché del visor: cualquier slider mueve
    /// algún bit y el frame se re-renderiza.
    pub fn key_hash(&self) -> u64 {
        const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0000_0100_0000_01b3;

        let mut hash = OFFSET;
        let mut mix = |word: u64| {
            for byte in word.to_le_bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(PRIME);
            }
        };
        mix(self.line_width.to_bits() as u64);
        mix(self.line_width_2d.to_bits() as u64);
        mix(self.depth_scale.to_bits() as u64);
        mix(self.yaw_deg.to_bits() as u64);
        mix(self.pitch_deg.to_bits() as u64);
        mix(self.depth_fade.to_bits() as u64);
        mix(self.mesh as u64);
        mix(self.aa_feather.to_bits() as u64);
        mix(self.glow.to_bits() as u64);
        hash
    }
}

/// Matriz de proyección oblicua fija para el terreno 3D.
///
/// Es la fórmula directa del diseño, no una cámara orbital:
///
/// ```text
/// ndc_x = (X + Z·skew_x) · s / aspect
/// ndc_y = (Y - Z·skew_y) · s
/// ndc_z = 0.5 - Z / total        (frente → 0, fondo → 1)
/// w     = 1
/// ```
///
/// Cada fila es la misma línea 2D replicada en Z (ver
/// [`WavetableMesh::from_table`]) y `w = 1` en todos los vértices: SIN
/// división perspectiva ninguna fila se achica ni se deforma por distancia y
/// todas conservan el trazo 2D exacto. La "profundidad" es sólo el shear fijo
/// (las filas traseras suben y se corren a la derecha) más el depth buffer:
/// las filas delanteras tienen menor `ndc_z` y la oclusión la resuelve el
/// depth test, sin importar el orden de dibujado.
///
/// `half` son los semiejes de la caja `[ancho, alto, profundidad]` y `aspect`
/// el del viewport (ancho / alto). La escala `s` es uniforme (sin estirar) y
/// la fija el eje limitante, igual que `Camera::fit_to_box` pero para esta
/// proyección.
pub fn terrain_view_proj(half: [f32; 3], aspect: f32) -> Mat4 {
    terrain_view_proj_angled(half, aspect, TERRAIN_YAW_DEG, TERRAIN_PITCH_DEG)
}

/// Lo mismo que [`terrain_view_proj`] pero con los ángulos del panel de
/// settings: `yaw_deg` desvía el fondo en X y `pitch_deg` lo eleva en Y.
///
/// Los valores se acotan y sanean acá (no sólo en la UI) porque la matriz no
/// puede permitirse un `tan` de 90° ni un NaN: la malla desaparecería sin
/// ningún error de wgpu que lo delate.
pub fn terrain_view_proj_angled(
    half: [f32; 3],
    aspect: f32,
    yaw_deg: f32,
    pitch_deg: f32,
) -> Mat4 {
    let aspect = if aspect.is_finite() && aspect > f32::EPSILON { aspect } else { 1.0 };
    let (hx, hy, hz) = (half[0].max(0.0), half[1].max(0.0), half[2].max(0.0));

    // `clamp` solo no alcanza: con NaN devuelve NaN y la matriz envenenaría
    // toda la escena. Se cae a los ángulos por defecto.
    let yaw = if yaw_deg.is_finite() { yaw_deg.clamp(-60.0, 60.0) } else { TERRAIN_YAW_DEG };
    let pitch =
        if pitch_deg.is_finite() { pitch_deg.clamp(5.0, 80.0) } else { TERRAIN_PITCH_DEG };
    let (yaw, pitch) = (yaw.to_radians(), pitch.to_radians());
    // Mismo convenio de signos que la fórmula de diseño: con Z frente `+` y
    // fondo `-`, el fondo se corre a la derecha y sube en pantalla.
    let skew_x = -yaw.tan();
    let skew_y = pitch.tan();

    // Semi-extensiones proyectadas (peor esquina): con shear, una fila del
    // fondo suma `|hz·skew|` en cada eje además de su propio semieje.
    let ex = hx + hz * skew_x.abs();
    let ey = hy + hz * skew_y.abs();
    let s = TERRAIN_FILL / ey.max(ex / aspect).max(f32::EPSILON);
    let total = (hz * 2.0).max(f32::EPSILON);

    [
        [s / aspect, 0.0, 0.0, 0.0],
        [0.0, s, 0.0, 0.0],
        [s * skew_x / aspect, -s * skew_y, -1.0 / total, 0.0],
        [0.0, 0.0, 0.5, 1.0],
    ]
}

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

    /// Terreno completo de la wavetable, estilo Serum/Vital/Bitwig.
    ///
    /// No un ciclo suelto, sino la matriz completa con el frame bajo `WT POS`
    /// destacado. Sistema de coordenadas fijo y uniforme:
    ///
    /// - **X**: ancho normalizado del ciclo (muestra 0 a `frame_len`), de
    ///   izquierda a derecha. Todos los sub-frames usan exactamente el mismo
    ///   ancho y las mismas columnas, así que cada columna `k` cae en la misma
    ///   `x` en todas las filas: la matriz queda alineada.
    /// - **Y**: `amplitud * sample_value`. Altura vertical de la onda (±0.9 de
    ///   la media caja): cada ciclo mapea las muestras de su propio sub-frame
    ///   sin estirarse ni salirse de su banda.
    /// - **Z**: `offset_profundidad * i`. El sub-frame `i` (de `0` a `N-1`) va
    ///   en un plano de `z` constante, con espaciado uniforme entre filas. La
    ///   fila 0 (el primer ciclo, la senoide) queda al frente (`+Z`, lo más
    ///   cercano a la cámara que mira desde `+Z`) y la última al fondo: las
    ///   ondas se apilan hacia el fondo en perspectiva, no como una torre
    ///   hacia arriba.
    ///
    /// Cada fila se dibuja según `style.mesh` (ver [`RenderSettings`]):
    /// `Wireframe` son líneas de ancho constante (ver [`push_terrain_line`]),
    /// `Ribbon` cintas con volumen mínimo y `Solid` la superficie continua que
    /// las conecta (ver [`push_terrain_solid`]). En los tres casos, sin faldón
    /// ni relleno hacia ninguna base: las filas son estrictamente paralelas en
    /// profundidad y comparten la misma perspectiva X/Y, así que no hay
    /// aristas cruzadas entre sub-frames ni barras sólidas: el terreno se lee
    /// de frente-arriba como en Vital/Serum.
    ///
    /// # Cómo se reparten
    ///
    /// `table` es la tabla entera tal como la entrega `wavetable_io`: `frames`
    /// bloques consecutivos de `frame_len` muestras. Se dibujan como máximo
    /// `max_frames` filas, repartidas en la extensión total
    /// `params.thickness × style.depth_scale` en Z: la serie completa siempre
    /// cabe en la caja y la escala global no depende de cuántos ciclos traiga
    /// el archivo.
    ///
    /// Con más ciclos que `max_frames` no se dibujan todos, y los que se dibujan
    /// representan la tabla completa: se reparte el rango, no se recortan los
    /// primeros. Un archivo de dos minutos son 2000 ciclos, y 2000 cintas de 512
    /// columnas no entran en un panel de 500 píxeles ni aportan algo que se pueda
    /// distinguir.
    ///
    /// # El realce
    ///
    /// `active` es la posición flotante del knob `WT POS` en ciclos reales. Se
    /// mapea al espacio de filas y el brillo viaja en `uv.y` (ver
    /// [`ACTIVE_SHADE`]): el ciclo bajo el knob va a brillo pleno y el resto
    /// queda atenuado pero legible como referencia del terreno. El falloff de
    /// 1.5 filas hace que barrer el knob se vea como un barrido continuo y no
    /// como saltos entre filas, incluso cuando se muestrea una tabla grande.
    /// Encima va el fade back-to-front (`style.depth_fade`): a igual distancia
    /// del knob, las filas traseras rinden menos, así que el terreno se lee
    /// de atrás hacia adelante con el activo destacado.
    pub fn from_table(
        table: &[f32],
        frame_len: usize,
        active: f32,
        max_frames: usize,
        frame_width: u32,
        params: WavetableMeshParams,
        style: &RenderSettings,
    ) -> Result<Self, MeshError> {
        if table.is_empty() || frame_len == 0 {
            return Err(MeshError::WaveformTooShort);
        }
        // Piso de resolución (ver `TERRAIN_MIN_COLUMNS`): la UI alimenta 512
        // columnas interpoladas sobre 2048 samples, y por debajo de ~128 las
        // pendientes empinadas se vuelven angulosas.
        let columns = frame_width.max(TERRAIN_MIN_COLUMNS).max(2) as usize;
        let available = table.len() / frame_len;
        if available == 0 {
            return Err(MeshError::WaveformTooShort);
        }
        let active = if active.is_finite() { active.clamp(0.0, (available - 1).max(0) as f32) } else { 0.0 };

        let limit = if max_frames == 0 { available } else { max_frames.max(1) };
        let count = available.min(limit).max(1);

        // Posición del knob en espacio de filas (0..count-1), repartiendo el
        // rango completo: con `count == available` es identidad; si se
        // muestrea, el primer y el último ciclo caen exacto en la primera y
        // última fila y el realce sigue al knob sin saltos.
        let active_slot = if count <= 1 || available <= 1 {
            0.0
        } else {
            active / (available - 1) as f32 * (count - 1) as f32
        };

        // Ajustes del panel, saneados acá (no sólo en la UI): la geometría no
        // puede permitirse un NaN ni un cero que la aplaste sin que ningún
        // error de wgpu lo delate.
        let line_width = sanitize_width(style.line_width);
        let depth_scale = if style.depth_scale.is_finite() {
            style.depth_scale.clamp(0.1, 4.0)
        } else {
            1.0
        };
        let fade = if style.depth_fade.is_finite() {
            style.depth_fade.clamp(0.0, 0.9)
        } else {
            TERRAIN_DEPTH_FADE
        };

        // Capacidad para el peor caso (alambre/cinta: pares por fila; sólido:
        // un vértice por punto de grilla, que es menos).
        let mut vertices = Vec::with_capacity(count * columns * 2);
        let mut indices = Vec::with_capacity(count * (columns - 1) * 6);

        // Extensión total del terreno en Z (con la escala del panel). El offset
        // entre filas es constante (`total / count`, con medio hueco de aire
        // en cada borde para que la primera y la última línea no queden
        // pegadas al borde del encuadre). Con una sola fila no hay profundidad
        // que recorrer y va centrada en 0.
        let total = (params.thickness * depth_scale).max(f32::EPSILON);
        let spacing = total / count as f32;

        // Filas para el modo sólido: alturas ya muestreadas + plano + brillo.
        // En alambre/cinta se emite directo por fila; en sólido se tiende la
        // superficie una vez reunidas todas.
        let mut grid: Vec<Vec<f32>> = Vec::new();
        let mut zrows: Vec<f32> = Vec::new();
        let mut row_shades: Vec<f32> = Vec::new();

        for slot in 0..count {
            // Fila -> índice real de la tabla, redondeado: el mismo criterio
            // con el que el knob de índice recorre la matriz, así que lo que
            // brilla y lo que se ve coinciden.
            let table_idx = if count <= 1 {
                0
            } else {
                ((slot as f32 * (available - 1) as f32 / (count - 1) as f32).round()
                    .clamp(0.0, (available - 1) as f32)) as usize
            };
            let start = (table_idx * frame_len).min(table.len());
            let end = (start + frame_len).min(table.len());
            let Some(frame) = table.get(start..end) else { continue };
            if frame.is_empty() {
                continue;
            }

            // Fila 0 al frente (+Z, primer plano ante la cámara) y la última al
            // fondo (-Z): equivale a girar la vista 180° sobre Y respecto del
            // orden inverso, sin tocar la cámara.
            let z_center = if count > 1 {
                total * 0.5 - spacing * (slot as f32 + 0.5)
            } else {
                0.0
            };

            let distance = (slot as f32 - active_slot).abs();
            let highlight = ACTIVE_SHADE
                + (DIM_SHADE - ACTIVE_SHADE) * (distance / SHADE_FALLOFF).clamp(0.0, 1.0);
            // Fade back-to-front del panel: a igual distancia de `WT POS`,
            // las filas traseras rinden menos. El activo sigue siendo el más
            // brillante de su zona en todos los casos.
            let depth = if count > 1 {
                1.0 - fade * slot as f32 / (count - 1) as f32
            } else {
                1.0
            };
            let shade = highlight * depth;

            match style.mesh {
                MeshType::Wireframe => {
                    push_terrain_line(
                        &mut vertices,
                        &mut indices,
                        frame,
                        columns,
                        params,
                        z_center,
                        line_width,
                        shade,
                    );
                }
                MeshType::Ribbon => {
                    // Tubo en el plano X-Y con espesor en Z (ver
                    // `push_ribbon`): la misma curva con cuerpo y sombreado
                    // en la silueta.
                    push_ribbon(
                        &mut vertices,
                        &mut indices,
                        frame,
                        columns,
                        params,
                        z_center,
                        line_width,
                        shade,
                    );
                }
                MeshType::Solid => {
                    // La superficie se tiende al final, con todas las filas
                    // reunidas: necesita las vecinas para las normales.
                    grid.push(sample_heights(frame, columns, &params));
                    zrows.push(z_center);
                    row_shades.push(shade);
                }
            }
        }

        if style.mesh == MeshType::Solid && !grid.is_empty() {
            let half_width = params.width * 0.5;
            push_terrain_solid(
                &mut vertices,
                &mut indices,
                &grid,
                |c| -half_width + c as f32 / columns as f32 * params.width,
                &zrows,
                &row_shades,
                line_width,
            );
        }

        if vertices.is_empty() {
            return Err(MeshError::Empty);
        }

        Ok(Self { vertices, indices })
    }

    /// Ciclo único plano para el modo 2D de diagnóstico.
    ///
    /// `frame` es el ciclo activo YA interpolado (la UI lo calcula con
    /// `Wavetable::frame(active, smooth)`): la malla no interpola, sólo
    /// dibuja, así que al mover `WT POS` la onda 2D muestra el morphing exacto
    /// del ciclo activo. Una sola línea en el plano X-Y a `z = 0`, a brillo
    /// pleno (`ACTIVE_SHADE`), con el mismo trazo y antialiasing que el
    /// terreno: sirve para verificar que el `.wav` se lee e interpola bien
    /// antes de proyectarse en 3D.
    ///
    /// Devuelve `Err(MeshError::WaveformTooShort)` si el frame llega vacío.
    pub fn from_active_line(
        frame: &[f32],
        frame_width: u32,
        params: WavetableMeshParams,
        style: &RenderSettings,
    ) -> Result<Self, MeshError> {
        if frame.is_empty() {
            return Err(MeshError::WaveformTooShort);
        }
        // Mismo piso de resolución que el terreno: el diagnóstico tiene que
        // mostrar la curva real, no una versión angulosa de pocos puntos. El
        // grosor es el del panel para el modo 2D.
        let columns = frame_width.max(TERRAIN_MIN_COLUMNS).max(2) as usize;

        let mut vertices = Vec::with_capacity(columns * 2);
        let mut indices = Vec::with_capacity(columns.saturating_sub(1) * 6);

        push_terrain_line(
            &mut vertices,
            &mut indices,
            frame,
            columns,
            params,
            0.0,
            sanitize_width(style.line_width_2d),
            ACTIVE_SHADE,
        );

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
/// La usa [`WavetableMesh::from_waveform`] para el ciclo aislado y el modo
/// `Ribbon` de [`WavetableMesh::from_table`]: tubo en el plano X-Y con espesor
/// en Z. Las líneas planas del modo alambre viven en [`push_terrain_line`].
///
/// El `peak` de la forma de onda es el de su propio ciclo y no el de la tabla
/// entera. Normalizar contra el pico global haría que un frame casi en silencio
/// se viera plano al lado de uno fuerte, que es justamente el detalle que un
/// selector de wavetable tiene que mostrar.
///
/// SIN duplicar la primera columna al final y SIN segmento de cierre (la misma
/// razón que en las líneas: unir el borde derecho con el izquierdo dibujaría
/// una banda horizontal de todo el ancho sobre la onda).
///
/// `z_center` y `depth` colocan la cinta en el eje Z; `shade` es el factor de
/// brillo que aplica el fragment shader.
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
    for column in 0..columns {
        let phase_u = column as f32 / columns as f32;

        let sample = resample(waveform, phase_u);
        let x = -half_width + phase_u * params.width;
        // Normalizado a -1..1, centrando la forma de onda en la caja.
        let y = sample / peak * 0.9 * half_height;

        // Cara delantera y trasera. `uv.x` transversal (-1/+1): el borde de la
        // cinta se suaviza en el shader y la silueta no serrucha.
        vertices.push(MeshVertex {
            position: [x, y, z_center - half_depth],
            normal: [0.0, 0.0, -1.0],
            uv: [-1.0, shade],
        });
        vertices.push(MeshVertex {
            position: [x, y, z_center + half_depth],
            normal: [0.0, 0.0, 1.0],
            uv: [1.0, shade],
        });
    }

    // Cada segmento entre columnas consecutivas aporta dos triángulos que unen
    // la cara delantera con la trasera. El winding sigue la convención del
    // pipeline con back-face culling (ver [`MeshRenderer::new`]).
    for column in 0..columns.saturating_sub(1) {
        // Cada columna aporta un par de vértices (frontal, posterior), corridos
        // por `base` porque la pila mete varias cintas en el mismo buffer.
        let front_a = base + (column as u32) * 2;
        let back_a = front_a + 1;
        let front_b = base + (column as u32 + 1) * 2;
        let back_b = front_b + 1;

        indices.extend_from_slice(&[front_a, back_a, front_b]);
        indices.extend_from_slice(&[front_b, back_a, back_b]);
    }
}

/// Agrega la línea de un sub-frame a los buffers del terreno.
///
/// Sistema de coordenadas del terreno: **X** fase del ciclo (izquierda a
/// derecha), **Y** altura vertical, **Z** profundidad (plano de la línea,
/// constante por fila). La línea es una cinta plana en el plano X-Y: cero
/// volumen en Z, ancho [`TERRAIN_LINE_WIDTH`] medido en el plano y
/// perpendicular a la curva, igual en pendientes suaves y empinadas.
///
/// Por qué plana y no un tubo: el tubo visto de frente-arriba muestra su cara
/// como una banda rellena y N bandas superpuestas se leen como barras rígidas
/// cruzadas. La línea plana, en cambio, es la curva 1D de las muestras con
/// grosor de trazo: paralela entre filas, sin aristas cruzadas ni barras
/// sólidas, como en Vital.
///
/// - Sin faldón ni relleno hacia ninguna base común.
/// - La `x` depende sólo de la columna, nunca de la fila: todas las filas
///   comparten las mismas `x` y la matriz queda alineada en X por construcción.
/// - Pico del propio ciclo (igual que `push_ribbon`): cada fila llena la misma
///   altura para que los timbres se comparen por forma.
/// - Normal `+Z` uniforme (hacia la cámara, que mira desde `+Z` de frente con
///   pitch de 30°): sombreado parejo; la forma se lee por silueta.
///
/// Alturas de un ciclo re-muestreado a `columns` puntos, normalizadas a ±0.9
/// de la media caja con el pico del propio ciclo.
///
/// Es el muestreo compartido por la línea ([`push_line_strip`]) y la
/// superficie ([`push_terrain_solid`]): un solo lugar donde la forma de onda
/// se convierte en geometría, así que los tres tipos de malla dibujan la
/// misma curva.
fn sample_heights(waveform: &[f32], columns: usize, params: &WavetableMeshParams) -> Vec<f32> {
    let peak = waveform
        .iter()
        .fold(0.0f32, |acc, sample| acc.max(sample.abs()))
        .max(f32::EPSILON);

    (0..columns)
        .map(|column| {
            let phase_u = column as f32 / columns as f32;
            resample(waveform, phase_u) / peak * 0.9 * params.height * 0.5
        })
        .collect()
}

/// `z_center` coloca la línea en el eje Z; `line_width` es el ancho del trazo
/// (ver `RenderSettings`); `shade` es el factor de brillo del fragment shader
/// (tracking de `WT POS`).
fn push_terrain_line(
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u32>,
    waveform: &[f32],
    columns: usize,
    params: WavetableMeshParams,
    z_center: f32,
    line_width: f32,
    shade: f32,
) {
    let half_width = params.width * 0.5;
    let heights = sample_heights(waveform, columns, &params);

    // Puntos de la curva en el plano X-Y, de izquierda a derecha. SIN duplicar
    // la primera columna al final y SIN segmento de cierre: unir el último
    // punto (borde derecho) con el primero (borde izquierdo) dibujaría un
    // triángulo de todo el ancho cruzando la pantalla — la diagonal fantasma
    // que se veía entre ciclos. La onda periódica ya empalma visualmente
    // porque el último sample colinda con el primero en fase.
    let points: Vec<(f32, f32)> = heights
        .iter()
        .enumerate()
        .map(|(column, &y)| {
            (-half_width + column as f32 / columns as f32 * params.width, y)
        })
        .collect();

    push_line_strip(vertices, indices, &points, z_center, line_width, shade);
}

/// Emite la tira de triángulos de una curva en el plano X-Y a profundidad
/// `z_center`, con ancho `line_width` perpendicular a la curva.
///
/// Es el trazado compartido por el modo alambre y por el fallback de una sola
/// fila del modo sólido: la misma curva, el mismo grosor, el mismo winding
/// (`+Z`, verificado) para el back-face culling del pipeline.
fn push_line_strip(
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u32>,
    points: &[(f32, f32)],
    z_center: f32,
    line_width: f32,
    shade: f32,
) {
    let half_line = (line_width * 0.5).max(0.125);
    let columns = points.len();

    let base = vertices.len() as u32;
    for (i, &(x, y)) in points.iter().enumerate() {
        // Dirección central con extremos clamped (sin wraparound: no hay
        // segmento de cierre). La `x` estrictamente creciente garantiza que
        // nunca degenere: siempre hay avance en X entre vecinos.
        let (prev_x, prev_y) = points[i.saturating_sub(1)];
        let (next_x, next_y) = points[(i + 1).min(columns - 1)];
        let (mut dx, mut dy) = (next_x - prev_x, next_y - prev_y);
        let len = (dx * dx + dy * dy).sqrt();
        if len > f32::EPSILON {
            dx /= len;
            dy /= len;
        } else {
            dx = 1.0;
            dy = 0.0;
        }
        // Perpendicular en el plano: el ancho no depende de la pendiente.
        // `uv.x` transversal (-1/+1): el shader suaviza el borde con `fwidth`.
        let (ox, oy) = (-dy * half_line, dx * half_line);

        vertices.push(MeshVertex {
            position: [x + ox, y + oy, z_center],
            normal: [0.0, 0.0, 1.0],
            uv: [-1.0, shade],
        });
        vertices.push(MeshVertex {
            position: [x - ox, y - oy, z_center],
            normal: [0.0, 0.0, 1.0],
            uv: [1.0, shade],
        });
    }

    // Un quad por segmento entre columnas consecutivas, sin cierre. El orden da
    // normal geométrica `+Z` (`d × perp(d)` en el plano X-Y apunta a `-Z`, así
    // que se emite en este orden), consistente con el back-face culling del
    // pipeline ante la cámara que mira desde `+Z`.
    for column in 0..columns.saturating_sub(1) {
        let left_a = base + (column as u32) * 2;
        let right_a = left_a + 1;
        let left_b = base + (column as u32 + 1) * 2;
        let right_b = left_b + 1;

        indices.extend_from_slice(&[left_a, right_a, left_b]);
        indices.extend_from_slice(&[left_b, right_a, right_b]);
    }
}

/// Agrega la superficie continua del terreno: las filas conectadas entre sí
/// como un heightfield (X = fase, Z = profundidad, Y = altura).
///
/// `heights` son las filas ya muestreadas (ver [`sample_heights`]), ordenadas
/// de adelante (`zrows[0]`, la más cercana a la cámara) hacia atrás;
/// `shades` el brillo por fila. Cada vértice lleva la normal analítica de la
/// superficie (`(-hx, 1, -hz)` normalizada por diferencias centrales), así que
/// la luz modela las pendientes en vez de pintar plano.
///
/// Con una sola fila no hay superficie que tender y se cae a la línea
/// ([`push_line_strip`]): el modo sólido de una tabla de un ciclo se ve igual
/// que el alambre.
///
/// `uv.x` es 0 en el interior (alfa pleno) y ±1 en el borde de la grilla, para
/// que la silueta de la superficie también tenga el suavizado del shader.
#[allow(clippy::too_many_arguments)]
fn push_terrain_solid(
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u32>,
    heights: &[Vec<f32>],
    x_of: impl Fn(usize) -> f32,
    zrows: &[f32],
    shades: &[f32],
    line_width: f32,
) {
    let rows = heights.len();
    let columns = heights.first().map_or(0, Vec::len);
    if rows == 0 || columns == 0 {
        return;
    }
    if rows < 2 {
        // Sin segunda fila no hay quads: la curva sola, con el mismo trazo.
        let points: Vec<(f32, f32)> =
            heights[0].iter().enumerate().map(|(c, &y)| (x_of(c), y)).collect();
        push_line_strip(vertices, indices, &points, zrows[0], line_width, shades[0]);
        return;
    }

    // Normaliza un gradiente a vector unitario, con eje de reserva para el
    // caso degenerado (filas idénticas planas: la normal es +Y igual).
    fn unit(x: f32, y: f32, z: f32) -> [f32; 3] {
        let len = (x * x + y * y + z * z).sqrt();
        if len > f32::EPSILON { [x / len, y / len, z / len] } else { [0.0, 1.0, 0.0] }
    }

    let base = vertices.len() as u32;
    for i in 0..rows {
        // Filas vecinas para la derivada en Z (clamped en los bordes).
        let up = heights[i.saturating_sub(1)].as_slice();
        let down = heights[(i + 1).min(rows - 1)].as_slice();
        let dz = (zrows[(i + 1).min(rows - 1)] - zrows[i.saturating_sub(1)]).abs().max(f32::EPSILON);
        for j in 0..columns {
            let prev = heights[i][j.saturating_sub(1)];
            let next = heights[i][(j + 1).min(columns - 1)];
            // `x_of` es lineal en columnas: el paso es la diferencia real,
            // sin asumir resolución.
            let dx = (x_of((j + 1).min(columns - 1)) - x_of(j.saturating_sub(1))).abs().max(f32::EPSILON);
            let hx = (next - prev) / dx;
            let hz = (down[j] - up[j]) / dz;
            let shade = shades[i];
            // Borde de la grilla: silueta suavizada; interior: alfa pleno.
            let edge = if i == 0 || i + 1 == rows || j == 0 || j + 1 == columns {
                1.0
            } else {
                0.0
            };
            vertices.push(MeshVertex {
                position: [x_of(j), heights[i][j], zrows[i]],
                normal: unit(-hx, 1.0, -hz),
                uv: [edge, shade],
            });
        }
    }

    // Quads de la grilla. Con filas ordenadas de adelante (+Z) hacia atrás
    // (-Z), este orden da normal geométrica `+Y` (verificado por construcción:
    // en plano da exactamente +Y), consistente con el culling.
    for i in 0..rows - 1 {
        for j in 0..columns - 1 {
            let a = base + (i * columns + j) as u32;
            let b = base + (i * columns + j + 1) as u32;
            let c = base + ((i + 1) * columns + j) as u32;
            let d = base + ((i + 1) * columns + j + 1) as u32;

            indices.extend_from_slice(&[a, b, c]);
            indices.extend_from_slice(&[b, d, c]);
        }
    }
}

/// Buffers de uniforms del pipeline de malla.
///
/// El layout tiene que seguir en sync con el `struct Uniforms` del WGSL: 64
/// bytes de matriz, 16 de luz, 16 de tinte y 16 de suavizado. Agregar un campo
/// lo cambia de tamaño, y como `min_binding_size` es `None` en el layout, un
/// desfasaje no lo detecta wgpu: el shader lee floats desplazados y la malla
/// sale con la iluminación corrida.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct MeshUniforms {
    /// Matriz `projection * view`, en columna mayor. La arma
    /// [`crate::camera::Camera::view_proj`] (o [`terrain_view_proj`] para el
    /// terreno 3D).
    pub view_proj: [[f32; 4]; 4],
    /// Dirección de la luz, normalizada, en espacio **local** de la malla.
    pub light_dir: [f32; 3],
    /// Intensidad de la luz.
    pub light_intensity: f32,
    /// Tinte base.
    pub tint: [f32; 4],
    /// Ancho del suavizado de borde en píxeles (ver `RenderSettings`).
    pub aa_feather: f32,
    /// Relleno a 16 bytes: el tamaño total tiene que ser múltiplo de 16 como
    /// el `struct Uniforms` del WGSL.
    pub _pad: [f32; 3],
}

impl Default for MeshUniforms {
    /// Defaults con la cámara de frente, no con la matriz identidad.
    ///
    /// La identidad "funciona" (el shader no falla) pero aplana la cinta contra
    /// el plano z = 0: se ve una línea, no un volumen. La cámara de frente
    /// hace que el estado neutro ya sea algo que se pueda mirar.
    fn default() -> Self {
        let mut uniforms = crate::camera::Camera::front().uniforms(1.0, [0.35, 0.85, 1.0, 1.0]);
        uniforms.aa_feather = 1.0;
        uniforms
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
                // Las líneas del terreno son cintas planas de una sola cara
                // (ver `push_terrain_line`) con winding para normal `+Y`,
                // hacia la cámara fija de frente-arriba. El culling quita lo
                // que el depth test taparía igual; `push_ribbon` (ciclo
                // aislado) es un tubo cerrado y usa el mismo modo sin
                // cambios.
                cull_mode: Some(wgpu::Face::Back),
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
        // Dos triángulos por segmento entre columnas consecutivas, sin cierre.
        assert_eq!(mesh.indices.len(), 15 * 6);
    }

    #[test]
    fn single_waveform_has_no_closing_segment_across_the_width() {
        // La cinta aislada tampoco duplica ni cierra: unir el borde derecho
        // con el izquierdo dibujaría la banda horizontal fantasma sobre la
        // onda (el mismo defecto que el terreno).
        let columns = 16u32;
        let mesh =
            WavetableMesh::from_waveform(&test_waveform(), columns, Default::default()).unwrap();
        assert_eq!(mesh.vertices.len(), columns as usize * 2);
        let step = 260.0 / columns as f32;
        for tri in mesh.indices.chunks_exact(3) {
            let xs: Vec<f32> =
                tri.iter().map(|&v| mesh.vertices[v as usize].position[0]).collect();
            let span = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max)
                - xs.iter().cloned().fold(f32::INFINITY, f32::min);
            assert!(span < step + 1e-4, "triángulo {tri:?} cruza {span} en X");
        }
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
        // Una sola columna no alcanza para un segmento; el piso de 2 columnas
        // es lo que garantiza un quad y que los índices queden en rango.
        let mesh = WavetableMesh::from_waveform(&test_waveform(), 1, Default::default()).unwrap();
        assert_eq!(mesh.indices.len(), 1 * 6);
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
        // mat4x4 (64) + vec3 + f32 (16) + vec4 (16) + f32 + pad (16) = 112
        // bytes. Con `min_binding_size: None` en el layout, wgpu no valida
        // esto: el síntoma es iluminación corrida.
        assert_eq!(std::mem::size_of::<MeshUniforms>(), 112);
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

    /// Tabla de `frames` ciclos constantes: el ciclo `i` vale `i + 1` en todas
    /// sus muestras. Cada fila normaliza por su propio pico, así que todas las
    /// cintas quedan a la misma altura y lo único que distingue a las filas es
    /// su plano Y y su brillo.
    fn flat_table(frames: usize, frame_len: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|frame| vec![frame as f32 + 1.0; frame_len])
            .collect()
    }

    fn terrain_params() -> WavetableMeshParams {
        WavetableMeshParams { width: 260.0, height: 96.0, thickness: 240.0 }
    }

    /// Vértices de la fila `slot`: `columns` sale del conteo total.
    fn row_range(slot: usize, columns: usize) -> std::ops::Range<usize> {
        let stride = columns * 2;
        slot * stride..(slot + 1) * stride
    }

    #[test]
    fn terrain_has_no_triangle_crossing_the_full_width() {
        // Rampa (diente de sierra): la discontinuidad en la costura del lazo
        // convierte cualquier segmento de cierre en una diagonal de todo el
        // ancho. Ningún triángulo puede unir el final de una fila con el
        // principio de otra ni cruzar el ancho: cada triángulo vive dentro de
        // su fila y abarca como máximo un paso de columna más el trazo.
        let frame: Vec<f32> = (0..16).map(|i| i as f32 / 15.0 * 2.0 - 1.0).collect();
        let table: Vec<f32> =
            frame.iter().copied().cycle().take(frame.len() * 3).collect();
        let columns = 128u32;
        let params = terrain_params();
        let mesh = WavetableMesh::from_table(&table, frame.len(), 0.0, 64, columns, params, &RenderSettings::default())
            .expect("el terreno debería construirse");

        let rows = 3usize;
        let stride = mesh.vertices.len() / rows;
        // Piso de resolución: aunque se pidan 8, la malla sale con 128.
        let built = TERRAIN_MIN_COLUMNS as usize;
        assert_eq!(stride, built * 2);
        let max_step = params.width / built as f32 + TERRAIN_LINE_WIDTH;
        for tri in mesh.indices.chunks_exact(3) {
            let row0 = tri[0] as usize / stride;
            assert!(
                tri.iter().all(|&v| v as usize / stride == row0),
                "triángulo {tri:?} mezcla filas"
            );
            let xs: Vec<f32> =
                tri.iter().map(|&v| mesh.vertices[v as usize].position[0]).collect();
            let span = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max)
                - xs.iter().cloned().fold(f32::INFINITY, f32::min);
            assert!(span < max_step, "triángulo {tri:?} cruza {span} en X");
        }
    }

    #[test]
    fn terrain_draws_one_line_per_sub_frame() {
        // 7 sub-frames como la "Basic Shapes": una línea por ciclo, sin
        // faldón, sin relleno y sin volumen.
        let table = flat_table(7, 8);
        let columns = 128u32;
        let mesh = WavetableMesh::from_table(&table, 8, 0.0, 64, columns, terrain_params(), &RenderSettings::default())
            .expect("el terreno debería construirse");
        assert_eq!(mesh.vertices.len(), 7 * (columns as usize) * 2);
        assert_eq!(mesh.indices.len(), 7 * (columns as usize - 1) * 6);
        assert_eq!(validate_geometry(&mesh.vertices, &mesh.indices), Ok(()));
    }

    #[test]
    fn terrain_enforces_minimum_sample_resolution() {
        // Aunque se pidan 2 columnas, cada sub-frame sale interpolado con
        // TERRAIN_MIN_COLUMNS puntos: las pendientes empinadas no se vuelven
        // angulosas. La UI alimenta 512, así que el piso sólo protege a otros
        // llamantes sin cambiar la topología.
        let table = flat_table(3, 8);
        let mesh = WavetableMesh::from_table(&table, 8, 0.0, 64, 2, terrain_params(), &RenderSettings::default())
            .expect("el terreno debería construirse");
        assert_eq!(
            mesh.vertices.len(),
            3 * TERRAIN_MIN_COLUMNS as usize * 2
        );
        assert_eq!(validate_geometry(&mesh.vertices, &mesh.indices), Ok(()));
    }

    /// Centro de la curva en la columna `c` de la fila `slot`: el promedio del
    /// par de vértices (el offset perpendicular se cancela).
    fn row_center(mesh: &WavetableMesh, slot: usize, columns: usize, c: usize) -> [f32; 3] {
        let base = row_range(slot, columns).start + c * 2;
        let a = mesh.vertices[base].position;
        let b = mesh.vertices[base + 1].position;
        [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5, (a[2] + b[2]) * 0.5]
    }

    #[test]
    fn terrain_rows_are_parallel_sharing_xy_with_depth_in_z() {
        let table = flat_table(5, 8);
        let columns = 128usize;
        let params = terrain_params();
        let mesh = WavetableMesh::from_table(&table, 8, 0.0, 64, columns as u32, params, &RenderSettings::default()).unwrap();

        // Paralelismo continuo: los centros comparten X e Y por columna en
        // todas las filas (misma perspectiva X/Y; la tabla plana repite la
        // misma curva); sólo cambia Z, la profundidad.
        for column in 0..columns {
            let c0 = row_center(&mesh, 0, columns, column);
            for slot in 1..5 {
                let c = row_center(&mesh, slot, columns, column);
                assert!((c[0] - c0[0]).abs() < 1e-5, "la fila {slot} se corrió en X");
                assert!((c[1] - c0[1]).abs() < 1e-5, "la fila {slot} se corrió en Y");
            }
        }

        // Offset constante sólo en Z: fila 0 al frente (+Z, primer plano ante
        // la cámara) y última al fondo, con medio hueco de aire en cada borde
        // del encuadre. Y como la línea es plana, cada vértice está exactamente
        // en el plano de su fila: nada se apila hacia arriba.
        let spacing = params.thickness / 5.0;
        let z_of = |slot: usize| row_center(&mesh, slot, columns, 0)[2];
        let step = z_of(0) - z_of(1);
        assert!((step - spacing).abs() < 1e-4, "el espaciado no es total/count: {step}");
        for slot in 1..5 {
            let s = z_of(slot - 1) - z_of(slot);
            assert!((s - step).abs() < 1e-4, "offset en Z no constante: {s} vs {step}");
        }
        assert!((z_of(0) - (params.thickness * 0.5 - spacing * 0.5)).abs() < 1e-4);
        assert!((z_of(4) + (params.thickness * 0.5 - spacing * 0.5)).abs() < 1e-4);
        // Planitud: cero extensión en Z dentro de cada fila (no hay tubo ni
        // barra, sólo trazo) y ninguna fila fuera de su plano.
        for slot in 0..5 {
            let row = row_range(slot, columns);
            let z = mesh.vertices[row.start].position[2];
            assert!(mesh.vertices[row].iter().all(|v| (v.position[2] - z).abs() < 1e-6));
        }
        // Aire entre líneas: el ancho es fijo y mucho menor que el hueco.
        assert!(TERRAIN_LINE_WIDTH < spacing * 0.5);
    }

    #[test]
    fn terrain_lines_have_constant_width_without_skirt() {
        // Ciclo con positivo y negativo: la altura vertical sigue el signo,
        // contenida en ±0.9 de la media caja.
        let table: Vec<f32> = [1.0f32, -1.0].iter().copied().cycle().take(2 * 3).collect();
        let columns = 128usize;
        let params = terrain_params();
        let mesh =
            WavetableMesh::from_table(&table, 2, 0.0, 64, columns as u32, params, &RenderSettings::default()).unwrap();

        // Columna 0 muestrea u=0 -> +1: arriba. Columna `columns/2` muestrea
        // u=0.5 -> -1. El centro del par cae sobre la curva (el offset se
        // cancela).
        let y0 = row_center(&mesh, 0, columns, 0)[1];
        let y2 = row_center(&mesh, 0, columns, columns / 2)[1];
        assert!((y0 - 0.9 * params.height * 0.5).abs() < 1e-3, "y={y0}");
        assert!((y2 + 0.9 * params.height * 0.5).abs() < 1e-3, "y={y2}");

        // Ancho constante en el plano X-Y, mire donde mire la pendiente: cada
        // par dista TERRAIN_LINE_WIDTH y su punto medio está sobre la curva.
        // Sin faldón: nada conecta la curva hacia ninguna base.
        for slot in 0..3 {
            let row = row_range(slot, columns);
            for c in 0..columns {
                let a = mesh.vertices[row.start + c * 2].position;
                let b = mesh.vertices[row.start + c * 2 + 1].position;
                let dx = a[0] - b[0];
                let dy = a[1] - b[1];
                assert!(((dx * dx + dy * dy).sqrt() - TERRAIN_LINE_WIDTH).abs() < 1e-4);
                assert!((a[2] - b[2]).abs() < 1e-6, "la línea tiene volumen en Z");
            }
        }

        // Normal +Z uniforme (hacia la cámara): sombreado parejo, sin caras
        // invertidas.
        assert!(mesh.vertices.iter().all(|v| v.normal == [0.0, 0.0, 1.0]));
    }

    /// Brillo esperado de la fila `slot` con el knob en `active_slot`: realce
    /// de `WT POS` por fade back-to-front (la misma fórmula de `from_table`).
    fn expected_shade(slot: usize, count: usize, active_slot: f32) -> f32 {
        let highlight = ACTIVE_SHADE
            + (DIM_SHADE - ACTIVE_SHADE)
                * ((slot as f32 - active_slot).abs() / SHADE_FALLOFF).clamp(0.0, 1.0);
        let depth = 1.0 - TERRAIN_DEPTH_FADE * slot as f32 / (count - 1) as f32;
        highlight * depth
    }

    #[test]
    fn terrain_highlights_the_active_row_and_dims_the_rest() {
        let table = flat_table(7, 8);
        let columns = 128u32;
        let mesh =
            WavetableMesh::from_table(&table, 8, 2.0, 64, columns, terrain_params(), &RenderSettings::default()).unwrap();

        let shade_of = |slot: usize| {
            mesh.vertices[row_range(slot, columns as usize).start].uv[1]
        };
        // La fila activa rinde realce pleno por su fade de profundidad; las
        // vecinas, menos; la lejana queda en el mínimo legible pero visible.
        for slot in 0..7 {
            assert!((shade_of(slot) - expected_shade(slot, 7, 2.0)).abs() < 1e-6);
        }
        assert!(shade_of(2) > shade_of(1) && shade_of(2) > shade_of(3));
        assert!(shade_of(6) > 0.0, "el fondo no puede apagarse del todo");
        // El activo al frente rinde más que el mismo activo al fondo: el fade
        // ordena el terreno de atrás hacia adelante.
        let front_bias =
            WavetableMesh::from_table(&table, 8, 0.0, 64, columns, terrain_params(), &RenderSettings::default()).unwrap();
        let back_bias =
            WavetableMesh::from_table(&table, 8, 6.0, 64, columns, terrain_params(), &RenderSettings::default()).unwrap();
        let shade_at = |mesh: &WavetableMesh, slot: usize| {
            mesh.vertices[row_range(slot, columns as usize).start].uv[1]
        };
        assert!(shade_at(&front_bias, 0) > shade_at(&back_bias, 6));
    }

    #[test]
    fn terrain_tracks_a_fractional_wt_pos_between_rows() {
        // WT POS a mitad de camino entre la fila 2 y la 3: el realce (quitado
        // el fade, que es por fila) es simétrico y ninguna llega al máximo. Es
        // lo que hace continuo el barrido.
        let table = flat_table(7, 8);
        let columns = 128u32;
        let mesh =
            WavetableMesh::from_table(&table, 8, 2.5, 64, columns, terrain_params(), &RenderSettings::default()).unwrap();
        let shade_of = |slot: usize| {
            mesh.vertices[row_range(slot, columns as usize).start].uv[1]
        };
        let fade = |slot: usize| 1.0 - TERRAIN_DEPTH_FADE * slot as f32 / 6.0;
        assert!(((shade_of(2) / fade(2)) - (shade_of(3) / fade(3))).abs() < 1e-6);
        assert!(shade_of(2) < ACTIVE_SHADE && shade_of(2) > DIM_SHADE * fade(2));
    }

    #[test]
    fn terrain_sampling_covers_the_full_range_when_capped() {
        // 8 ciclos muestreados a 4 filas: primera fila = ciclo 0, última = 7.
        // Se verifica por el contenido: la fila i de tabla plana vale i+1, y el
        // pico propio la deja a altura completa; lo que distingue es el índice
        // mapeado, así que se usa una tabla con firmas por ciclo.
        let frame_len = 4usize;
        let table: Vec<f32> = (0..8)
            .flat_map(|frame| {
                let mut f = vec![0.0f32; frame_len];
                f[0] = frame as f32 + 1.0;
                f
            })
            .collect();
        let columns = 128u32;
        let mesh =
            WavetableMesh::from_table(&table, frame_len, 0.0, 4, columns, terrain_params(), &RenderSettings::default())
                .unwrap();
        assert_eq!(mesh.vertices.len(), 4 * (columns as usize) * 2);

        // Primera fila mapea al ciclo 0 y la última al 7: resample en u=0 da el
        // sample 0 del frame, que es la firma. Se lee el centro del par (el
        // offset perpendicular del trazo se cancela); la altura va en Y.
        let first_top = row_center(&mesh, 0, columns as usize, 0)[1];
        let last_top = row_center(&mesh, 3, columns as usize, 0)[1];
        // Ciclo 0: firma 1.0 con pico 1.0 -> +0.9*half. Ciclo 7: firma 8.0 con
        // pico 8.0 -> también +0.9*half (normalizado). Ambos a tope: la prueba
        // real es que hay 4 filas y el realce del extremo cae exacto.
        let params = terrain_params();
        assert!((first_top - 0.9 * params.height * 0.5).abs() < 1e-3);
        assert!((last_top - 0.9 * params.height * 0.5).abs() < 1e-3);

        // Con active en el último ciclo, la última fila lleva el realce pleno
        // por su fade de fondo (ver `expected_shade`).
        let mesh =
            WavetableMesh::from_table(&table, frame_len, 7.0, 4, columns, params, &RenderSettings::default()).unwrap();
        let shade_last =
            mesh.vertices[row_range(3, columns as usize).start].uv[1];
        assert!((shade_last - expected_shade(3, 4, 3.0)).abs() < 1e-6);
    }

    #[test]
    fn terrain_fits_the_declared_box() {
        // La serie completa dentro de la caja: es lo que encuadra la cámara con
        // `fit_to_box`, así que nada puede salirse o el viewport la recorta.
        let table = flat_table(24, 16);
        let params = terrain_params();
        let mesh =
            WavetableMesh::from_table(&table, 16, 3.0, 24, 128, params, &RenderSettings::default()).unwrap();
        for v in &mesh.vertices {
            let [x, y, z] = v.position;
            // Margen de medio trazo: el ancho perpendicular puede asomar hasta
            // `TERRAIN_LINE_WIDTH/2` fuera de la curva en X o Y. En Z la línea
            // es plana y cada fila está en su plano exacto.
            let m = TERRAIN_LINE_WIDTH * 0.5 + 1e-4;
            assert!(x >= -params.width / 2.0 - m && x <= params.width / 2.0 + m, "x={x}");
            assert!(y >= -params.height / 2.0 - m && y <= params.height / 2.0 + m, "y={y}");
            assert!(z >= -params.thickness / 2.0 - 1e-4 && z <= params.thickness / 2.0 + 1e-4, "z={z}");
            assert!(v.position.iter().all(|c| c.is_finite()));
            assert!(v.uv[1].is_finite());
        }
    }

    #[test]
    fn oblique_projection_frames_the_terrain_without_deforming_rows() {
        // Cierra el circuito óptico sin GPU con la proyección real del modo
        // 3D: la misma que usa la UI (ver `render_viewer`).
        let params = terrain_params();
        let aspect = 768.0 / 384.0;
        let view_proj = terrain_view_proj(
            [params.width * 0.5, params.height * 0.5, params.thickness * 0.5],
            aspect,
        );

        // Sin división perspectiva: w = 1 en todos los vértices, así que
        // ninguna fila se achica por distancia.
        for z in [-120.0f32, 0.0, 120.0] {
            let (_, w) = crate::camera::transform_point(&view_proj, [0.0, 0.0, z]);
            assert!((w - 1.0).abs() < 1e-6, "w={w}");
        }

        // La caja entera entra en pantalla con profundidad válida: el frente
        // (Z+) va a z≈0 (gana el depth test) y el fondo (Z-) a z≈1.
        for x in [-130.0f32, 130.0] {
            for y in [-48.0f32, 48.0] {
                for z in [-120.0f32, 120.0] {
                    let (clip, w) =
                        crate::camera::transform_point(&view_proj, [x, y, z]);
                    let ndc = [clip[0] / w, clip[1] / w, clip[2] / w];
                    assert!(
                        ndc[0].abs() <= 1.0 && ndc[1].abs() <= 1.0,
                        "esquina ({x}, {y}, {z}) fuera de pantalla: {ndc:?}"
                    );
                    assert!((0.0..=1.0).contains(&ndc[2]), "z fuera de [0, 1]: {ndc:?}");
                }
            }
        }
        let (_, front_w) = crate::camera::transform_point(&view_proj, [0.0, 0.0, 120.0]);
        let (front_clip, _) = crate::camera::transform_point(&view_proj, [0.0, 0.0, 120.0]);
        let (back_clip, _) = crate::camera::transform_point(&view_proj, [0.0, 0.0, -120.0]);
        assert!(front_clip[2] / front_w < back_clip[2] / front_w);
    }

    #[test]
    fn oblique_projection_pushes_back_rows_up_and_right() {
        // El ritmo diagonal del terreno (shear de `terrain_view_proj_angled`):
        // a igual X/Y, una fila del fondo aparece más arriba y más a la
        // derecha que una del frente, como en Vital. Es shear puro, sin
        // convergencia.
        let params = terrain_params();
        let view_proj = terrain_view_proj(
            [params.width * 0.5, params.height * 0.5, params.thickness * 0.5],
            768.0 / 384.0,
        );
        let ndc = |p: [f32; 3]| {
            let (clip, w) = crate::camera::transform_point(&view_proj, p);
            [clip[0] / w, clip[1] / w]
        };
        let front = ndc([0.0, 0.0, 120.0]);
        let back = ndc([0.0, 0.0, -120.0]);
        assert!(back[1] > front[1], "el fondo no sube: {back:?} vs {front:?}");
        assert!(back[0] > front[0], "el fondo no se corre a la derecha");

        // Sin convergencia: dos puntos separados en X mantienen su distancia
        // en el frente y en el fondo (la proyección es afín).
        let spread = |z: f32| (ndc([130.0, 0.0, z])[0] - ndc([-130.0, 0.0, z])[0]).abs();
        assert!((spread(120.0) - spread(-120.0)).abs() < 1e-5);
    }

    #[test]
    fn terrain_lands_in_projection_with_frame_zero_nearest() {
        // El terreno bajo la proyección oblicua: todo vértice dentro de
        // pantalla con profundidad válida, y la fila 0 (primer ciclo) con la
        // menor profundidad NDC (gana el depth test = primer plano).
        let table = flat_table(7, 16);
        let params = terrain_params();
        let columns = 128usize;
        let mesh =
            WavetableMesh::from_table(&table, 16, 0.0, 64, columns as u32, params, &RenderSettings::default()).unwrap();

        let view_proj = terrain_view_proj(
            [params.width * 0.5, params.height * 0.5, params.thickness * 0.5],
            768.0 / 384.0,
        );

        let mut nearest = f32::INFINITY;
        let mut nearest_row = usize::MAX;
        for (i, v) in mesh.vertices.iter().enumerate() {
            let (clip, w) = crate::camera::transform_point(&view_proj, v.position);
            assert!((w - 1.0).abs() < 1e-6);
            let ndc = [clip[0] / w, clip[1] / w, clip[2] / w];
            assert!(
                ndc[0].abs() <= 1.0 + 1e-3 && ndc[1].abs() <= 1.0 + 1e-3,
                "vértice {i} fuera de pantalla: {ndc:?}"
            );
            assert!((0.0..=1.0).contains(&ndc[2]), "z fuera de [0, 1]: {ndc:?}");
            if ndc[2] < nearest {
                nearest = ndc[2];
                nearest_row = i / (columns * 2);
            }
        }
        assert_eq!(nearest_row, 0, "el primer ciclo no está en primer plano");
    }

    #[test]
    fn render_mode_toggles_between_2d_and_3d() {
        assert_eq!(RenderMode::default(), RenderMode::Mode3D);
        assert_eq!(RenderMode::Mode3D.toggle(), RenderMode::Mode2D);
        assert_eq!(RenderMode::Mode2D.toggle(), RenderMode::Mode3D);
        assert_eq!(RenderMode::Mode3D.label(), "3D");
        assert_eq!(RenderMode::Mode2D.label(), "2D");
    }

    /// Settings de prueba con el tipo de malla dado y el resto por defecto.
    fn style_with(mesh: MeshType) -> RenderSettings {
        RenderSettings { mesh, ..RenderSettings::default() }
    }

    #[test]
    fn mesh_type_cycles_through_the_three_geometries() {
        assert_eq!(MeshType::default(), MeshType::Wireframe);
        assert_eq!(MeshType::Wireframe.cycle(), MeshType::Ribbon);
        assert_eq!(MeshType::Ribbon.cycle(), MeshType::Solid);
        assert_eq!(MeshType::Solid.cycle(), MeshType::Wireframe);
        assert_eq!(MeshType::Wireframe.label(), "Líneas");
        assert_eq!(MeshType::Ribbon.label(), "Cintas");
        assert_eq!(MeshType::Solid.label(), "Sólido");
    }

    #[test]
    fn render_settings_default_to_the_verified_look() {
        // Los defaults tienen que ser el look ya validado: si alguien los
        // cambia, este test recuerda qué se consideraba correcto.
        let settings = RenderSettings::default();
        assert_eq!(settings.line_width, TERRAIN_LINE_WIDTH);
        assert_eq!(settings.line_width_2d, TERRAIN_LINE_WIDTH);
        assert_eq!(settings.depth_scale, 1.0);
        assert_eq!(settings.yaw_deg, 15.0);
        assert_eq!(settings.pitch_deg, 25.0);
        assert_eq!(settings.depth_fade, 0.35);
        assert_eq!(settings.mesh, MeshType::Wireframe);
        assert_eq!(settings.aa_feather, 1.0);
        assert_eq!(settings.glow, 1.0);
    }

    #[test]
    fn any_setting_moves_the_cache_key() {
        let base = RenderSettings::default().key_hash();
        // Y los mismos valores dan la misma clave (sin re-render espurio).
        assert_eq!(RenderSettings::default().key_hash(), base);
        for set in [
            (|s: &mut RenderSettings| s.line_width = 5.0) as fn(&mut RenderSettings),
            |s| s.line_width_2d = 5.0,
            |s| s.depth_scale = 1.5,
            |s| s.yaw_deg = 20.0,
            |s| s.pitch_deg = 30.0,
            |s| s.depth_fade = 0.5,
            |s| s.mesh = MeshType::Solid,
            |s| s.aa_feather = 0.0,
            |s| s.glow = 1.5,
        ] {
            let mut other = RenderSettings::default();
            set(&mut other);
            assert_ne!(other.key_hash(), base, "un ajuste no mueve la clave");
        }
    }

    #[test]
    fn ribbon_rows_are_tubes_with_depth_and_no_wrap_segment() {
        // 3 filas como tubos en X-Y con espesor en Z: pares por columna y
        // quads sólo entre columnas consecutivas.
        let table = flat_table(3, 8);
        let columns = 128usize;
        let style = style_with(MeshType::Ribbon);
        let mesh = WavetableMesh::from_table(&table, 8, 0.0, 64, columns as u32, terrain_params(), &style)
            .expect("el terreno de cintas debería construirse");
        assert_eq!(mesh.vertices.len(), 3 * columns * 2);
        assert_eq!(mesh.indices.len(), 3 * (columns - 1) * 6);
        assert_eq!(validate_geometry(&mesh.vertices, &mesh.indices), Ok(()));

        // Espesor en Z igual al grosor del panel, alrededor del plano de fila.
        let row = row_range(0, columns);
        let z_front = mesh.vertices[row.start].position[2];
        let z_back = mesh.vertices[row.start + 1].position[2];
        assert!(((z_front - z_back).abs() - style.line_width).abs() < 1e-4);
        // Normales ±Z alternadas (delantera/trasera del tubo).
        for (i, v) in mesh.vertices.iter().enumerate() {
            let expected = if i % 2 == 0 { [0.0, 0.0, -1.0] } else { [0.0, 0.0, 1.0] };
            assert_eq!(v.normal, expected, "vértice {i}");
        }
    }

    #[test]
    fn solid_surface_connects_rows_with_upward_normals() {
        // Tabla plana de ceros: la superficie es el plano y=0 y toda normal
        // (de vértice y geométrica) tiene que mirar hacia arriba.
        let columns = 128usize;
        let table = vec![0.0f32; 4 * 8];
        let style = style_with(MeshType::Solid);
        let mesh = WavetableMesh::from_table(&table, 8, 0.0, 64, columns as u32, terrain_params(), &style)
            .expect("la superficie debería construirse");

        // Un vértice por punto de grilla (compartidos entre quads).
        assert_eq!(mesh.vertices.len(), 4 * columns);
        assert_eq!(mesh.indices.len(), 3 * (columns - 1) * 6);
        assert_eq!(validate_geometry(&mesh.vertices, &mesh.indices), Ok(()));

        assert!(mesh.vertices.iter().all(|v| v.normal == [0.0, 1.0, 0.0]));
        for tri in mesh.indices.chunks_exact(3) {
            let p: Vec<[f32; 3]> =
                tri.iter().map(|&ix| mesh.vertices[ix as usize].position).collect();
            let e1 = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
            let e2 = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
            let ny = e1[2] * e2[0] - e1[0] * e2[2];
            assert!(ny > 0.0, "cara invertida en {tri:?}");
        }

        // Silueta suavizada en el borde de la grilla, alfa pleno adentro.
        let at = |r: usize, c: usize| mesh.vertices[r * columns + c].uv[0];
        assert_eq!(at(0, 0), 1.0);
        assert_eq!(at(3, 127), 1.0);
        assert_eq!(at(0, 64), 1.0);
        assert_eq!(at(1, 1), 0.0);
        assert_eq!(at(2, 40), 0.0);
    }

    #[test]
    fn solid_with_a_single_row_falls_back_to_a_line() {
        // Sin segunda fila no hay quads que tender: la curva sola, con los
        // mismos conteos que el alambre.
        let table = vec![0.5f32; 8];
        let style = style_with(MeshType::Solid);
        let mesh = WavetableMesh::from_table(&table, 8, 0.0, 64, 128, terrain_params(), &style)
            .expect("el fallback debería construirse");
        assert_eq!(mesh.vertices.len(), 128 * 2);
        assert_eq!(mesh.indices.len(), 127 * 6);
        assert_eq!(validate_geometry(&mesh.vertices, &mesh.indices), Ok(()));
    }

    #[test]
    fn angled_projection_matches_the_fixed_one_at_default_angles() {
        // La variante con ángulos existe para los sliders; con 15°/25° tiene
        // que dar exactamente la matriz fija (mismo `tan`, mismo código).
        let half = [130.0, 48.0, 120.0];
        assert_eq!(terrain_view_proj(half, 2.0), terrain_view_proj_angled(half, 2.0, 15.0, 25.0));
    }

    #[test]
    fn angled_projection_survives_extreme_and_degenerate_angles() {
        // Yaw/pitch en los bordes del rango y valores rotos: la escena se
        // re-encuadra sola y la matriz sigue siendo finita y afín (w = 1).
        for (yaw, pitch) in [
            (-30.0, 5.0),
            (30.0, 60.0),
            (0.0, 25.0),
            (f32::NAN, f32::INFINITY),
            (1000.0, -1000.0),
        ] {
            let view =
                terrain_view_proj_angled([130.0, 48.0, 120.0], 768.0 / 384.0, yaw, pitch);
            assert!(
                view.iter().all(|col| col.iter().all(|v| v.is_finite())),
                "matriz no finita con yaw={yaw} pitch={pitch}"
            );
            for x in [-130.0f32, 130.0] {
                for y in [-48.0f32, 48.0] {
                    for z in [-120.0f32, 120.0] {
                        let (clip, w) =
                            crate::camera::transform_point(&view, [x, y, z]);
                        assert!((w - 1.0).abs() < 1e-6);
                        assert!(
                            (clip[0] / w).abs() <= 1.0 + 1e-3
                                && (clip[1] / w).abs() <= 1.0 + 1e-3,
                            "esquina fuera de pantalla con yaw={yaw} pitch={pitch}"
                        );
                        assert!((0.0..=1.0).contains(&(clip[2] / w)));
                    }
                }
            }
        }
    }

    #[test]
    fn active_line_is_a_single_full_bright_curve_at_zero_depth() {
        // El ciclo ya interpolado que entrega la UI: seno de un período.
        let frame: Vec<f32> = (0..256)
            .map(|i| (i as f32 / 256.0 * std::f32::consts::TAU).sin())
            .collect();
        let params = terrain_params();
        let mesh = WavetableMesh::from_active_line(&frame, 512, params, &RenderSettings::default())
            .expect("la línea 2D debería construirse");

        // Una sola curva: 512 pares, 511 quads, sin filas apiladas.
        assert_eq!(mesh.vertices.len(), 512 * 2);
        assert_eq!(mesh.indices.len(), 511 * 6);
        assert_eq!(validate_geometry(&mesh.vertices, &mesh.indices), Ok(()));

        // En el plano z = 0 (el modo 2D no tiene profundidad) y a brillo pleno:
        // es el ciclo de diagnóstico, no una fila atenuada del terreno.
        assert!(mesh.vertices.iter().all(|v| v.position[2] == 0.0));
        assert!(mesh.vertices.iter().all(|v| (v.uv[1] - ACTIVE_SHADE).abs() < 1e-6));

        // Recorre todo el ancho y la altura sigue a la muestra: pico del seno
        // arriba en el primer cuarto (u=0.25) y valle abajo en el tercero.
        let xs: Vec<f32> = (0..512).map(|c| mesh.vertices[c * 2].position[0]).collect();
        // Margen de medio trazo: el offset perpendicular asoma hasta 1.5u.
        assert!((xs[0] + params.width * 0.5).abs() < 2.0);
        assert!(xs.windows(2).all(|w| w[1] > w[0]));
        // Centro de la curva (el offset se cancela entre el par): pico y valle
        // exactos del seno.
        let peak = (mesh.vertices[128 * 2].position[1] + mesh.vertices[128 * 2 + 1].position[1]) * 0.5;
        let valley =
            (mesh.vertices[384 * 2].position[1] + mesh.vertices[384 * 2 + 1].position[1]) * 0.5;
        assert!((peak - 0.9 * params.height * 0.5).abs() < 1e-3, "pico={peak}");
        assert!((valley + 0.9 * params.height * 0.5).abs() < 1e-3, "valle={valley}");
    }

    #[test]
    fn active_line_rejects_an_empty_frame() {
        assert!(matches!(
            WavetableMesh::from_active_line(&[], 512, terrain_params(), &RenderSettings::default()),
            Err(MeshError::WaveformTooShort)
        ));
    }

    #[test]
    fn terrain_rejects_empty_input() {
        assert!(matches!(
            WavetableMesh::from_table(&[], 8, 0.0, 8, 16, terrain_params(), &RenderSettings::default()),
            Err(MeshError::WaveformTooShort)
        ));
        assert!(matches!(
            WavetableMesh::from_table(&[0.0; 16], 0, 0.0, 8, 16, terrain_params(), &RenderSettings::default()),
            Err(MeshError::WaveformTooShort)
        ));
    }
}
