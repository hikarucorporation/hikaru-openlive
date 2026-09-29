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
//! produce texturas que la GUI compone después. Mantener las dos cosas separadas
//! es lo que permite que ambas compartan una sola copia de wgpu (ver el
//! `Cargo.toml` raíz).
//!
//! # Cómo entra un render wgpu en el layout de GPUI Kit
//!
//! GPUI compone sus elementos sobre su propia superficie y no expone un
//! `TextureView` arbitrario para painting, así que no hay forma de "pegar" la
//! textura de wgpu en un `div`. El camino que sí funciona es:
//!
//! 1. render offscreen a una textura propia de `hikaru_render`,
//! 2. lectura de vuelta a CPU (`GpuContext::read_color_rgba8`),
//! 3. empaquetado como `RenderImage` de GPUI (que es un atlas propio suyo),
//! 4. `img(RenderImage)` como un elemento más del árbol 2D.
//!
//! El paso 3 tiene un detalle que no es negociable: `RenderImage` espera los
//! bytes en **BGRA**, no en RGBA. Ver [`render_image_from_rgba`].
//!
//! El costo del readback es el precio de esta integración: el visor de la
//! Wavetable son 512x320, o sea medio megabyte de CPU por redibujado. Por eso
//! [`WavetableViewport`] cachea por *firma del frame* (cámara + waveform +
//! tinte) y sólo vuelve a renderizar cuando algo cambió de verdad.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use gpui_kit::{App, Global, RenderImage};
use hikaru_render::{
    upload_knob, Camera, GpuContext, GpuMesh, KnobMesh, KnobRenderer, KnobUniforms, MeshRenderer,
    MeshUniforms, QuadInstance, QuadRenderer, ReadbackError, RenderTarget, SpriteLayout,
    SpriteSheet, SpriteSheetResources, WavetableMesh, WavetableMeshParams,
};
// Se usa el reexport de `hikaru_render` en vez de declarar wgpu como dependencia
// directa: garantiza que la GUI hable con exactamente la misma versión que el
// crate de render y con la que ya usa gpui-kit.
use hikaru_render::wgpu;

/// Resolución del visor offscreen de la Wavetable.
///
/// Fija a propósito: es la textura que se sube al atlas de GPUI, y hacerlo con
/// el tamaño del `div` obligaría a re-renderizar en cada resize del layout, con
/// un readback de CPU cada vez. Con tamaño fijo, el `img` escala con
/// `ObjectFit::Contain` y el render sólo depende de la cámara.
///
/// El aspect es 1.875 porque es el de la caja real del visor dentro del módulo
/// cuadrado (unos 254 x 135). Con el 1.6 que tenía antes, la imagen se encajaba
/// por el alto y quedaban barras negras a los lados; y bajarlo de 512x320 además
/// ahorra un 25% de los bytes de readback, que en CPU es lo caro del camino.
pub const WAVETABLE_VIEWPORT: (u32, u32) = (480, 256);

/// Columnas de la cinta en el eje de la forma de onda.
///
/// 256 columnas sobre 512 píxeles de ancho dan dos píxeles por columna: más que
/// suficiente para que la cinta se vea lisa, y bastante menos trabajo de
/// geometría que las 2048 muestras de una tabla.
const WAVETABLE_COLUMNS: u32 = 256;

/// Errores al armar el renderer offscreen.
#[derive(Debug)]
pub enum RendererError {
    /// No se encontró una GPU usable (drivers ausentes, Vulkan deshabilitado).
    ///
    /// La GUI sigue funcionando: sólo pierde el render offscreen.
    NoGpu(hikaru_render::context::ContextError),
    /// El target no tiene dimensión cero, que es lo único que wgpu acepta.
    DegenerateSize {
        /// Ancho pedido.
        width: u32,
        /// Alto pedido.
        height: u32,
    },
    /// Se leyó un target que todavía no fue creado.
    ///
    /// Es un estado normal y no un error de configuración, así que tiene su
    /// propia variante: reutilizar [`RendererError::DegenerateSize`] para esto
    /// hacía que el panel mostrara "tamaño de render inválido: 512x320" en el
    /// primer frame, cuando 512x320 es un tamaño perfectamente válido y lo
    /// único que faltaba era que la textura existiera.
    MissingTarget {
        /// Qué target se pidió.
        target: TargetId,
    },
    /// La geometría de la malla no se pudo construir o subir.
    Mesh(hikaru_render::mesh::MeshError),
    /// La lectura de vuelta de la textura falló.
    Readback(ReadbackError),
    /// La geometría del knob no se pudo construir o subir.
    Knob(hikaru_render::knob::KnobError),
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
            RendererError::MissingTarget { target } => {
                write!(f, "el target {target:?} todavía no fue creado")
            }
            RendererError::Mesh(error) => write!(f, "no se pudo preparar la malla: {error}"),
            RendererError::Readback(error) => write!(f, "no se pudo leer el render: {error}"),
            RendererError::Knob(error) => write!(f, "no se pudo preparar el knob: {error}"),
        }
    }
}

impl std::error::Error for RendererError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RendererError::NoGpu(error) => Some(error),
            RendererError::DegenerateSize { .. } => None,
            RendererError::MissingTarget { .. } => None,
            RendererError::Mesh(error) => Some(error),
            RendererError::Readback(error) => Some(error),
            RendererError::Knob(error) => Some(error),
        }
    }
}

impl From<hikaru_render::mesh::MeshError> for RendererError {
    fn from(error: hikaru_render::mesh::MeshError) -> Self {
        RendererError::Mesh(error)
    }
}

impl From<ReadbackError> for RendererError {
    fn from(error: ReadbackError) -> Self {
        RendererError::Readback(error)
    }
}

impl From<hikaru_render::knob::KnobError> for RendererError {
    fn from(error: hikaru_render::knob::KnobError) -> Self {
        RendererError::Knob(error)
    }
}

/// Los targets del panel de Wavetable.
///
/// # Por qué uno por imagen
///
/// Cada target es una textura con su propia resolución, y el readback devuelve
/// `width * height * 4` bytes. Compartir un solo target significaría renderizar
/// el visor, leerlo, renderizar un knob, leerlo, y el segundo readback
/// sobreescribiría el del visor. Con targets separados, cada render va a su
/// textura y los readbacks son independientes.
///
/// El del knob es chico a propósito (96x96, ver
/// [`hikaru_render::KNOB_RESOLUTION`]): es una perilla de 52px en pantalla, y un
/// target del tamaño del visor serían 640 KB de CPU tirados a la basura por
/// cada cambio de valor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TargetId {
    /// La pila 3D de la Wavetable, la imagen grande del panel.
    WavetableViewer,
    /// El knob de `WT POS`.
    ///
    /// Hay uno solo. Antes había un segundo knob de índice con la misma función,
    /// y aunque ya no se pintaba seguía en el pipeline: se renderizaba y se
    /// leía de vuelta un target entero en cada frame, para taparlo con nada.
    WtPosKnob,
}

impl TargetId {
    /// Resolución con la que se crea el target.
    ///
    /// La del visor sale de [`WAVETABLE_VIEWPORT`] y la del knob de
    /// `hikaru_render`, así que la decisión de "de qué tamaño renderizo" vive
    /// en un solo lugar por cada tipo de imagen.
    pub fn resolution(self) -> (u32, u32) {
        match self {
            TargetId::WavetableViewer => WAVETABLE_VIEWPORT,
            TargetId::WtPosKnob => {
                (hikaru_render::KNOB_RESOLUTION, hikaru_render::KNOB_RESOLUTION)
            }
        }
    }
}

/// Punto de entrada único al render offscreen desde la GUI.
///
/// Es dueño del contexto y de los pipelines. Los assets (sprite sheets) se
/// suben una vez y se reutilizan entre frames; los targets se crean la primera
/// vez que se piden y no se re-crean salvo que cambie su resolución.
pub struct HikaruRenderer {
    ctx: GpuContext,
    sheets: SpriteSheetResources,
    quads: QuadRenderer,
    mesh: MeshRenderer,
    knob: KnobRenderer,
    /// Geometría del knob, subida una sola vez: no depende del valor, que viaja
    /// en los uniforms.
    knob_mesh: GpuMesh,
    /// Un target por imagen. `BTreeMap` y no array porque el enum es chico y
    /// esto se lee más claro que un índice.
    targets: BTreeMap<TargetId, RenderTarget>,
}

impl HikaruRenderer {
    /// Inicializa el contexto offscreen y los tres pipelines.
    ///
    /// Es `async` porque pedir el adapter y el device a wgpu lo es. Los
    /// pipelines se compilan de forma sincrónica una vez que se tiene el device.
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
        // El knob comparte formato con la malla: los dos pipelines 3D escriben
        // en targets del mismo tipo.
        let knob = KnobRenderer::new(
            &ctx,
            "hikaru::knob",
            GpuContext::preferred_texture_format(),
            GpuContext::depth_format(),
        );
        // La geometría del knob se sube una vez al arrancar: no depende del
        // valor, que viaja en los uniforms, así que nunca se vuelve a tocar.
        let knob_mesh =
            upload_knob(&ctx, "hikaru::knob_mesh", &KnobMesh::new(Default::default()))?;

        Ok(Self { ctx, sheets, quads, mesh, knob, knob_mesh, targets: BTreeMap::new() })
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

    /// Devuelve el target pedido, creándolo la primera vez.
    ///
    /// Es un no-op si ya existe con la misma resolución, así que se puede llamar
    /// en cada draw sin que nada se re-crete.
    fn target(&mut self, id: TargetId) -> Result<&RenderTarget, RendererError> {
        let (width, height) = id.resolution();
        if width == 0 || height == 0 {
            return Err(RendererError::DegenerateSize { width, height });
        }

        if !self.targets.contains_key(&id) {
            let target = self.ctx.create_render_target("hikaru::target", width, height, true);
            self.targets.insert(id, target);
        }

        Ok(self.targets.get(&id).expect("el target se acaba de insertar"))
    }

    /// Dibuja una lista de quads de una hoja de sprites sobre el visor.
    ///
    /// `quads` van en píxeles lógicos con origen arriba-izquierda, igual que el
    /// resto de la GUI. Van al target del visor, que es el único con resolución
    /// de 2D: los targets de los knobs son cuadrados y chicos a propósito.
    pub fn draw_sprite_quads(
        &mut self,
        quads: &[QuadInstance],
        sheet: &SpriteSheet,
        background: Option<wgpu::Color>,
    ) -> Result<(), RendererError> {
        let (width, height) = WAVETABLE_VIEWPORT;
        if let Some(background) = background {
            self.clear(background, TargetId::WavetableViewer)?;
        }

        // El target se pide antes de tocar `quads`: `self.target` toma `&mut
        // self` y el borrow no puede convivir con el draw.
        self.target(TargetId::WavetableViewer)?;
        let target = self.targets.get(&TargetId::WavetableViewer).expect("recién creado");
        self.quads.draw(&self.ctx, &target.color_view, width as f32, height as f32, quads, sheet);
        Ok(())
    }

    /// Sube al GPU una cinta construida a partir de los samples de audio.
    pub fn upload_wavetable(
        &self,
        label: &str,
        waveform: &[f32],
        frame_width: u32,
        params: WavetableMeshParams,
    ) -> Result<GpuMesh, hikaru_render::mesh::MeshError> {
        WavetableMesh::from_waveform(waveform, frame_width, params)?.upload(&self.ctx, label)
    }

    /// Sube al GPU la pila de ciclos de una wavetable completa.
    ///
    /// `table` es la tabla entera tal como la devuelve `wavetable_io`, y
    /// `active` el ciclo en primer plano. La geometría sale de
    /// [`WavetableMesh::from_table`], que apila los ciclos en el eje Z.
    pub fn upload_wavetable_table(
        &self,
        label: &str,
        table: &[f32],
        frame_len: usize,
        active: f32,
        max_frames: usize,
        frame_width: u32,
        params: WavetableMeshParams,
    ) -> Result<GpuMesh, hikaru_render::mesh::MeshError> {
        WavetableMesh::from_table(table, frame_len, active, max_frames, frame_width, params)?
            .upload(&self.ctx, label)
    }

    /// Dibuja la cinta de la Wavetable en el visor.
    ///
    /// Hace el clear con `background` antes de dibujar: el visor se compone
    /// sobre la grilla que pinta la propia vista, así que el fondo es
    /// transparente. Encadenar dos draws sin clear dejaría la Accumulate de
    ///Depth y el color del frame anterior.
    pub fn draw_wavetable_mesh(
        &mut self,
        mesh: &GpuMesh,
        uniforms: &MeshUniforms,
        target: TargetId,
        background: wgpu::Color,
    ) -> Result<(), RendererError> {
        self.clear(background, target)?;
        // `clear` crea el target si faltaba, así que el borrow inmutable es
        // válido y no hace falta volver a pedirlo.
        let target = self.targets.get(&target).expect("`clear` deja el target listo");
        self.mesh.draw(&self.ctx, target, mesh, uniforms);
        Ok(())
    }

    /// Dibuja un knob.
    ///
    /// `fill` es el valor normalizado 0..1. El ángulo se calcula acá y no en el
    /// shader porque el recorrido del dial es una decisión de la UI (los 270°
    /// con el hueco abajo), y el shader sólo rota.
    pub fn draw_knob(
        &mut self,
        target: TargetId,
        fill: f32,
        body: [f32; 4],
        marker: [f32; 4],
        camera: &Camera,
    ) -> Result<(), RendererError> {
        // Fondo transparente: el knob se compone sobre el fondo del panel, y un
        // rectángulo opaco alrededor del disco se vería como un cuadro.
        self.clear(wgpu::Color::TRANSPARENT, target)?;

        let aspect = 1.0;
        let uniforms = KnobUniforms {
            view_proj: camera.view_proj(aspect),
            light_dir: camera.local_light_dir([0.35, 0.55, 1.0]),
            angle: knob_angle(fill),
            body,
            marker,
        };

        self.target(target)?;
        let target_ref = self.targets.get(&target).expect("`clear` deja el target listo");
        self.knob.draw(&self.ctx, target_ref, &self.knob_mesh, &uniforms);
        Ok(())
    }

    /// Lee el target pedido y devuelve los píxeles como RGBA8.
    ///
    /// Ver [`GpuContext::read_color_rgba8`] para el detalle de por qué esto
    /// bloquea y cuánto puede tardar.
    ///
    /// Un target que todavía no se dibujó da [`RendererError::MissingTarget`], no
    /// un error de tamaño: el tamaño es correcto, lo que falta es la textura.
    pub fn read_frame(&self, target: TargetId) -> Result<Vec<u8>, RendererError> {
        let target =
            self.targets.get(&target).ok_or(RendererError::MissingTarget { target })?;
        Ok(self.ctx.read_color_rgba8(target)?)
    }

    /// Contexto de GPU, para las operaciones que este wrapper no expone.
    pub fn context(&self) -> &GpuContext {
        &self.ctx
    }

    /// Limpia un target a un color, opaco o transparente.
    ///
    /// Se llama antes de componer cada pieza en vez de dejar que cada renderer
    /// decida: si el render 3D preserva el color pero limpia la profundidad, y
    /// el 2D al revés, el orden de las llamadas pasa a importar de forma
    /// invisible.
    ///
    /// # El target se crea acá
    ///
    /// Antes esta función devolvía `DegenerateSize` cuando el target no estaba
    /// todavía en el mapa, y todos los `draw_*` llaman a `clear()` **antes** de
    /// pedir el target. El resultado era que el primer frame de cada sesión
    /// fallaba siempre con "tamaño de render inválido: 512x320" y el panel
    /// quedaba en el placeholder con ese texto, aunque 512x320 es un tamaño
    /// válido y la textura se creaba un instante después.
    ///
    /// Ahora el clear es el que crea el target si falta, así que a partir de acá
    /// el borrow inmutable es válido y ningún draw depende de un target
    /// preexistente.
    fn clear(&mut self, background: wgpu::Color, id: TargetId) -> Result<(), RendererError> {
        let (width, height) = id.resolution();
        if width == 0 || height == 0 {
            // Éste sí es el tamaño inválido de verdad: una dimensión cero.
            return Err(RendererError::DegenerateSize { width, height });
        }
        self.target(id)?;
        let target = self.targets.get(&id).expect("`target` se acaba de resolver");

        let mut encoder =
            self.ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
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
                        load: wgpu::LoadOp::Clear(background),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                // La profundidad se limpia siempre: es el estado que decide qué
                // cara de la malla 3D se ve, y dejarla sucia hace que el
                // segundo pase no aparezca.
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
        Ok(())
    }
}

/// Ángulo de la marca de un knob, en radianes, como posición **absoluta**.
///
/// El dial recorre los 270° habituales con el hueco abajo: la marca arranca en
/// las 7:30 (abajo-izquierda) y termina en las 4:30 (abajo-derecha), pasando por
/// las 9, las 12 y las 3. Con `fill` en 0 la marca queda en 225° y con `fill` en
/// 1 en -45°.
///
/// # Por qué absoluto y no relativo
///
/// La geometría de la marca en `hikaru_render` se construye en 0° y el shader
/// le **suma** este ángulo, así que el valor es directamente la posición final en
/// el disco. Mapear `225° - 270*fill` sobre ese 0 da el dial completo.
///
/// La versión anterior sumaba `-135° + 270*fill` sobre una base de 225°, y el
/// resultado era un cuarto de vuelta entre las 12 y las 3: el knob no se
/// correspondía con su valor.
pub fn knob_angle(fill: f32) -> f32 {
    let fill = if fill.is_finite() { fill.clamp(0.0, 1.0) } else { 0.0 };
    (225.0 - 270.0 * fill) * std::f32::consts::PI / 180.0
}
/// Una imagen cacheada por su firma.
struct CachedImage {
    /// Firma de lo que se dibujó. `None` hasta el primer render.
    key: Option<u64>,
    /// La imagen, lista para el `img` de GPUI.
    image: Option<Arc<RenderImage>>,
}

impl CachedImage {
    const EMPTY: Self = Self { key: None, image: None };

    /// Devuelve la imagen cacheada si la firma coincide.
    ///
    /// Es el caso normal: el panel se redibuja en cada frame por culpa de los
    /// medidores de audio del resto de la app, y sin esto habría un readback por
    /// frame.
    fn get(&self, key: u64) -> Option<Arc<RenderImage>> {
        if self.key == Some(key) { self.image.clone() } else { None }
    }

    fn store(&mut self, key: u64, image: Arc<RenderImage>) {
        self.key = Some(key);
        self.image = Some(image);
    }

    /// Invalida la cache sin borrar la imagen.
    ///
    /// Se usa después de un error: si no, un pedido idéntico devolvería para
    /// siempre la imagen vieja.
    fn invalidate(&mut self) {
        self.key = None;
    }
}

/// Estado del viewport 3D: el renderer, lo último dibujado y su cache.
///
/// No es `Sync` (guarda recursos de wgpu), así que va siempre detrás de un
/// [`Mutex`] y se accede con [`WavetableViewportHandle`].
pub struct WavetableViewport {
    /// Renderer offscreen. `None` mientras no haya GPU o mientras la
    /// inicialización asíncrona no haya terminado.
    renderer: Option<HikaruRenderer>,
    /// Cache del visor de la Wavetable.
    viewer: CachedImage,
    /// Cache del knob de `WT POS`.
    ///
    /// Uno solo: el knob de índice que había antes ya no se pintaba, así que
    /// mantener su cache era reservar una imagen y una firma que nada leía.
    knob: CachedImage,
    /// Malla cacheada con la firma del waveform con la que se construyó.
    /// Reconstruirla en cada frame es lo más caro del camino: son dos buffers y
    /// una validación de índices.
    mesh: Option<(u64, GpuMesh)>,
    /// Último error, para poder mostrarlo en el panel en vez de un recuadro
    /// negro sin explicación.
    last_error: Option<String>,
    /// Si la inicialización asíncrona ya se pidió, para no pedirla en cada
    /// frame mientras el adapter no responde.
    init_started: bool,
}

/// Identidad de un frame del visor.
///
/// Es lo que decide si hace falta volver a renderizar. Incluye la cámara, el
/// waveform y el tinte: los tres cambian la imagen, y comparar sólo la cámara
/// dejaría la cinta con la forma del oscilador anterior cuando se cambia de
/// wavetable o de color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameKey {
    /// Tamaño del target offscreen.
    pub width: u32,
    /// Alto del target offscreen.
    pub height: u32,
    /// Hash del waveform.
    pub waveform: u64,
    /// Ciclo en primer plano, cuantizado.
    pub active: i32,
    /// Yaw cuantizado.
    pub yaw: i32,
    /// Pitch cuantizado.
    pub pitch: i32,
    /// Distancia cuantizada.
    pub distance: i32,
    /// Tinte cuantizado a bits de `f32`.
    pub tint: [u32; 4],
}

/// Factor de cuantización de la cámara: un microradiano.
const CAMERA_QUANTUM: f32 = 1.0e-6;

/// Cuantiza un ángulo o una distancia de cámara para meterlo en la clave del
/// frame.
///
/// Es pública porque la regla (qué cambio de cámara invalida el render y cuál
/// no) es la parte del cacheo que más fácil se rompe en silencio.
pub fn quantize_camera(value: f32) -> i32 {
    if value.is_finite() { (value / CAMERA_QUANTUM).round() as i32 } else { 0 }
}

/// Cuantización del ciclo en primer plano.
///
/// Un milésimo de ciclo: la diferencia de brillo entre dos de estos valores es
/// invisible, así que subirla más fino sólo produce readbacks de una imagen
/// idéntica a la anterior.
const ACTIVE_QUANTUM: f32 = 1.0e-3;

/// Cuantiza la posición del ciclo activo para la clave del frame.
fn quantize_active(value: f32) -> i32 {
    if value.is_finite() { (value / ACTIVE_QUANTUM).round() as i32 } else { 0 }
}

/// Cuantización del valor de un knob.
///
/// Más gruesa que la de la cámara a propósito: el knob se dibuja entre 60 y 96
/// píxeles, así que un milésimo de recorrido son dos centésimas de píxel. finer
/// sólo produce readbacks cuya imagen es idéntica a la anterior.
const KNOB_QUANTUM: f32 = 1.0 / 1024.0;

/// Firma de un knob: valor cuantizado y colores.
///
/// Los colores entran porque un knob que cambia de color con el mismo valor es
/// otra imagen, y sin ellos el knob cacheado conservaría el color viejo.
pub fn knob_key(fill: f32, body: [f32; 4], marker: [f32; 4]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    let quantized =
        if fill.is_finite() { (fill / KNOB_QUANTUM).round() as i64 as u64 } else { 0 };

    let mut key = OFFSET;
    for word in quantized
        .to_le_bytes()
        .into_iter()
        .chain(body.iter().flat_map(|c| c.to_bits().to_le_bytes()))
        .chain(marker.iter().flat_map(|c| c.to_bits().to_le_bytes()))
    {
        key ^= u64::from(word);
        key = key.wrapping_mul(PRIME);
    }
    key
}

impl WavetableViewport {
    /// Levanta el contexto offscreen. Es `async` porque pedir adapter y device a
    /// wgpu lo es.
    pub async fn connect() -> Result<Self, RendererError> {
        Ok(Self {
            renderer: Some(HikaruRenderer::new().await?),
            viewer: CachedImage::EMPTY,
            knob: CachedImage::EMPTY,
            mesh: None,
            last_error: None,
            init_started: true,
        })
    }

    /// Un viewport sin renderer: todas las solicitudes devuelven imágenes
    /// vacías y la vista dibuja su placeholder. Es el estado en una máquina sin
    /// GPU.
    pub fn unavailable() -> Self {
        Self {
            renderer: None,
            viewer: CachedImage::EMPTY,
            knob: CachedImage::EMPTY,
            mesh: None,
            last_error: Some("sin GPU: la vista 3D está desactivada".to_string()),
            init_started: true,
        }
    }

    /// Un viewport todavía sin conectar: la inicialización se puede pedir una
    /// vez desde el arranque de la app.
    fn pending() -> Self {
        Self { init_started: false, ..Self::unavailable() }
    }

    /// Último error del pipeline, para mostrarlo en el panel.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Renderiza las imágenes del panel, re-renderizando sólo lo que cambió.
    ///
    /// Devolverlas juntas y no por separado importa: cada llamada al handle toma
    /// el lock, y con dos llamadas separadas el panel podría llegar a pintar el
    /// knob con el valor viejo durante un frame.
    pub fn view(&mut self, request: &WavetableViewRequest<'_>) -> WavetableView {
        let viewer_key = request.viewer.key();
        let knob_key = knob_key(request.morph, request.knob_body, request.knob_marker);

        // Atajo de cache: los dos están igual, no hay nada que hacer.
        if let (Some(viewer), Some(knob)) =
            (self.viewer.get(frame_key_hash(viewer_key)), self.knob.get(knob_key))
        {
            return WavetableView { viewer: Some(viewer), knob: Some(knob), error: None };
        }

        // El renderer se mueve por un rato porque `ensure_mesh` lo necesita por
        // referencia y los draws por `&mut`, contra el mismo `self`.
        let Some(mut renderer) = self.renderer.take() else {
            return WavetableView::empty(self.last_error.clone());
        };

        let result = self.render_view(&mut renderer, request, (viewer_key, knob_key));

        // Se devuelve siempre, incluso si el render falló: si no, el primer
        // error mataría el renderer para el resto de la sesión.
        self.renderer = Some(renderer);

        match result {
            Ok(view) => {
                self.last_error = None;
                view
            }
            Err(error) => {
                // Un error por frame llenaría la consola. Se recuerda el último
                // y el panel lo muestra: reintentar en el próximo frame es lo
                // correcto porque puede ser transitorio (un resize a mitad de
                // frame, un driver que se reinició).
                eprintln!("[Hikaru] No se pudo renderizar la vista 3D: {error}");
                self.last_error = Some(error.to_string());
                // Las claves se limpian para que un pedido idéntico reintente en
                // vez de devolver una imagen vieja para siempre.
                self.viewer.invalidate();
                self.knob.invalidate();
                WavetableView::empty(self.last_error.clone())
            }
        }
    }

    /// Dibuja las dos piezas y las sube como imágenes.
    fn render_view(
        &mut self,
        renderer: &mut HikaruRenderer,
        request: &WavetableViewRequest<'_>,
        keys: (FrameKey, u64),
    ) -> Result<WavetableView, RendererError> {
        let viewer = self.render_viewer(renderer, request)?;
        let knob = self.render_knob(renderer, request)?;

        self.viewer.store(frame_key_hash(keys.0), viewer.clone());
        self.knob.store(keys.1, knob.clone());

        Ok(WavetableView { viewer: Some(viewer), knob: Some(knob), error: None })
    }

    /// El visor: la cinta de la Wavetable.
    fn render_viewer(
        &mut self,
        renderer: &mut HikaruRenderer,
        request: &WavetableViewRequest<'_>,
    ) -> Result<Arc<RenderImage>, RendererError> {
        let viewer = &request.viewer;
        let (width, height) = viewer.size;
        let aspect = if height > 0 { width as f32 / height as f32 } else { 1.0 };

        // El encuadre se calcula contra la caja de la malla y el aspect real del
        // viewport: con un `distance` fijo, la misma cámara encuadra bien en 16:9
        // y corta la cinta en un panel angosto. El semieje de Z es el de la pila
        // de ciclos, que es lo que la vista muestra en profundidad.
        let mut camera = viewer.camera;
        camera.fit_to_box(
            [
                viewer.mesh_params.width * 0.5,
                viewer.mesh_params.height * 0.5,
                viewer.mesh_params.thickness * 0.5,
            ],
            aspect,
        );

        self.ensure_mesh(renderer, viewer)?;

        // `ensure_mesh` terminó y soltó el `&mut self`, así que el borrow
        // inmutable de la malla y el uso mutable del renderer no se pisan:
        // son objetos distintos.
        let mesh = &self.mesh.as_ref().expect("la malla se acaba de asegurar").1;
        renderer.draw_wavetable_mesh(
            mesh,
            &camera.uniforms(aspect, viewer.tint),
            TargetId::WavetableViewer,
            viewer.background,
        )?;

        let rgba = renderer.read_frame(TargetId::WavetableViewer)?;
        Ok(Arc::new(render_image_from_rgba(width, height, rgba)))
    }

    /// El knob de `WT POS`.
    fn render_knob(
        &mut self,
        renderer: &mut HikaruRenderer,
        request: &WavetableViewRequest<'_>,
    ) -> Result<Arc<RenderImage>, RendererError> {
        let camera = request.knob_camera;
        let target = TargetId::WtPosKnob;

        renderer.draw_knob(target, request.morph, request.knob_body, request.knob_marker, &camera)?;

        let (width, height) = target.resolution();
        let rgba = renderer.read_frame(target)?;
        Ok(Arc::new(render_image_from_rgba(width, height, rgba)))
    }

    /// Sube la malla de la tabla pedida si no es la que ya está en la GPU.
    ///
    /// La clave de cache combina el contenido de la tabla con el ciclo en primer
    /// plano: mover el knob de índice cambia el realce de la pila, así que es
    /// otra imagen aunque los samples sean los mismos. La geometría se
    /// reconstruye en ese caso, que es lo correcto porque es una vez por cambio
    /// de índice y no una vez por frame.
    fn ensure_mesh(
        &mut self,
        renderer: &HikaruRenderer,
        viewer: &ViewerRequest<'_>,
    ) -> Result<(), RendererError> {
        let key = frame_key_hash(viewer.key());

        let needs_upload = self.mesh.as_ref().is_none_or(|(cached, _)| *cached != key);
        if needs_upload {
            self.mesh = Some((
                key,
                renderer.upload_wavetable_table(
                    "hikaru::wavetable",
                    viewer.waveform,
                    viewer.frame_len,
                    viewer.active,
                    viewer.max_frames,
                    WAVETABLE_COLUMNS,
                    viewer.mesh_params,
                )?,
            ));
        }

        Ok(())
    }
}

/// Las imágenes del panel, ya montadas para pintar.
#[derive(Debug, Default, Clone)]
pub struct WavetableView {
    /// La pila 3D de ciclos.
    pub viewer: Option<Arc<RenderImage>>,
    /// El knob de `WT POS`.
    pub knob: Option<Arc<RenderImage>>,
    /// Error del render, si lo hubo.
    pub error: Option<String>,
}

impl WavetableView {
    /// Las dos vacías, con el motivo a la vista.
    fn empty(error: Option<String>) -> Self {
        Self { viewer: None, knob: None, error }
    }
}

/// Lo que la vista 3D pide renderizar en un frame.
#[derive(Debug, Clone, Copy)]
pub struct WavetableViewRequest<'a> {
    /// La pila de ciclos.
    pub viewer: ViewerRequest<'a>,
    /// Valor del knob de `WT POS`, 0..1.
    pub morph: f32,
    /// Color del cuerpo del knob.
    pub knob_body: [f32; 4],
    /// Color de la marca del knob.
    pub knob_marker: [f32; 4],
    /// Cámara del knob. Es la misma para el visor y para el knob: si
    /// difirieran, el knob parecería más grande que la pila sin motivo.
    pub knob_camera: Camera,
}

/// La parte del pedido que dibuja la cinta.
#[derive(Debug, Clone, Copy)]
pub struct ViewerRequest<'a> {
    /// Tamaño del target offscreen.
    pub size: (u32, u32),
    /// Cámara: yaw, pitch, distancia y punto de mira.
    pub camera: Camera,
    /// La tabla **entera** de la wavetable: `frames` bloques consecutivos de
    /// `frame_len` muestras, tal como la devuelve
    /// [`crate::views::wavetable_io::load_wavetable`].
    ///
    /// Pasa la tabla y no un solo ciclo a propósito: la malla apila los ciclos
    /// en el eje Z, que es la vista de wavetable que se espera de un
    /// sintetizador. Es una referencia, no una copia: la tabla tiene miles de
    /// muestras y copiarla en cada frame sería tirar memoria a la basura.
    pub waveform: &'a [f32],
    /// Samples por ciclo. Tiene que ser el mismo con el que el parser cortó la
    /// tabla, si no la malla mostraría mitades de ciclo.
    pub frame_len: usize,
    /// Ciclo que se está mirando, en posición continua: la misma que devuelve el
    /// knob de morph. El realce de la malla lo sigue de forma continua.
    pub active: f32,
    /// Cuántos ciclos se dibujan como máximo. Acota la geometría para que una
    /// tabla de 2000 ciclos no sea 2000 cintas.
    pub max_frames: usize,
    /// Caja de la cinta: `[ancho, alto, profundidad]`. La profundidad es la de
    /// la pila de ciclos, y entra en el encuadre de la cámara.
    pub mesh_params: WavetableMeshParams,
    /// Tinte de la malla, en RGBA 0..1.
    pub tint: [f32; 4],
    /// Color de fondo del viewport.
    pub background: wgpu::Color,
}

impl ViewerRequest<'_> {
    /// La firma de lo que dibuja.
    pub fn key(&self) -> FrameKey {
        FrameKey {
            width: self.size.0,
            height: self.size.1,
            waveform: hash_waveform(self.waveform),
            active: quantize_active(self.active),
            yaw: quantize_camera(self.camera.yaw),
            pitch: quantize_camera(self.camera.pitch),
            distance: quantize_camera(self.camera.distance),
            tint: self.tint.map(f32::to_bits),
        }
    }
}

/// Reduce una [`FrameKey`] a un entero para usarla como clave de cache.
///
/// Los campos son todos enteros o bits de `f32`, así que la clave son 56 bytes
/// que hashear es más barato que compararlos campo por campo tres veces por
/// frame.
fn frame_key_hash(key: FrameKey) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = OFFSET;
    for word in [
        u64::from(key.width),
        u64::from(key.height),
        key.waveform,
        key.active as u64,
        key.yaw as u64,
        key.pitch as u64,
        key.distance as u64,
        u64::from(key.tint[0]),
        u64::from(key.tint[1]),
        u64::from(key.tint[2]),
        u64::from(key.tint[3]),
    ] {
        for byte in word.to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(PRIME);
        }
    }
    hash
}

/// Manejo global del viewport 3D.
///
/// Va en un `Global` de GPUI y no en [`crate::app::AppState`] a propósito: el
/// estado de la app es lo que el usuario edita y se serializa con `Debug`, y
/// meter recursos de GPU ahí los haría imposible de derivar y de clonar.
#[derive(Clone)]
pub struct WavetableViewportHandle(Arc<Mutex<WavetableViewport>>);

impl Global for WavetableViewportHandle {}

impl Default for WavetableViewportHandle {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(WavetableViewport::pending())))
    }
}

impl WavetableViewportHandle {
    /// Crea un handle con el viewport todavía sin conectar.
    pub fn new() -> Self {
        Self::default()
    }

    /// Pide la inicialización asíncrona del renderer, una sola vez.
    ///
    /// Pedir adapter y device a wgpu no se puede hacer sin bloquear, así que va
    /// en un `spawn`. Al terminar pide un redibujado de las ventanas para que
    /// las vistas que ya muestran el placeholder se actualicen con la imagen.
    ///
    /// Es seguro llamarlo en cada frame: el segundo intento es un no-op.
    pub fn spawn_connect(&self, cx: &mut App) {
        {
            let Ok(mut viewport) = self.0.lock() else {
                return;
            };
            if viewport.init_started {
                return;
            }
            // Se marca antes de salir del lock para que dos frames concurrentes
            // no lanzen dos inicializaciones y se pelen por el mismo adapter.
            viewport.init_started = true;
        }

        let handle = self.clone();
        cx.spawn(async move |cx| {
            let result = WavetableViewport::connect().await;

            match handle.0.lock() {
                Ok(mut viewport) => match result {
                    Ok(connected) => *viewport = connected,
                    Err(error) => {
                        // Sin GPU la app sigue: el editor muestra el placeholder
                        // y el resto de la GUI no se entera.
                        eprintln!("[Hikaru] Vista 3D desactivada: {error}");
                        let message = error.to_string();
                        *viewport = WavetableViewport::unavailable();
                        viewport.last_error = Some(message);
                    }
                },
                Err(_) => eprintln!("[Hikaru] el lock del viewport 3D quedó envenenado"),
            }

            // `AsyncApp` no puede pedir el redibujado directo: el refresh va
            // dentro de un `update` al hilo de la app.
            cx.update(|cx| cx.refresh_windows());
        })
        .detach();
    }

    /// Pide las tres imágenes del panel.
    ///
    /// Cada campo es `None` si todavía no hay renderer (se está inicializando o
    /// la máquina no tiene GPU): la vista tiene que decidir qué mostrar.
    pub fn view(&self, request: &WavetableViewRequest<'_>) -> WavetableView {
        let Ok(mut viewport) = self.0.lock() else {
            return WavetableView::empty(Some("el lock del viewport 3D está envenenado".into()));
        };
        viewport.view(request)
    }

    /// Último error del viewport, o `None` si todo anda bien.
    pub fn last_error(&self) -> Option<String> {
        self.0.lock().ok().and_then(|viewport| viewport.last_error.clone())
    }

    /// ¿El viewport ya está listo para renderizar?
    ///
    /// La vista lo usa para distinguir "todavía no hay GPU" (placeholder) de
    /// "hay GPU pero este frame no cambió" (imagen cacheada).
    pub fn is_ready(&self) -> bool {
        self.0.lock().map(|viewport| viewport.renderer.is_some()).unwrap_or(false)
    }
}

/// Empaqueta píxeles RGBA8 en un [`RenderImage`] que GPUI pueda pintar.
///
/// # Por qué BGRA
///
/// `RenderImage` no documenta el orden de bytes, pero es lo que espera el atlas:
/// `WgpuAtlas` sube los bytes tal cual a una textura `Bgra8Unorm` y sólo
/// interchangea R y B cuando el atlas cayó a `Rgba8Unorm`. O sea, el contrato
/// real es BGRA, y mandar RGBA produce una imagen con los canales rojo y azul
/// cambiados. GPUI no normaliza: su propio decoder de imágenes hace el
/// `swap(0, 2)` antes de armar el `Frame`.
///
/// `frame.buffer().as_raw()` no distingue RGBA de BGRA: son los mismos cuatro
/// bytes con otro nombre, así que el `RgbaImage` de acá lleva BGRA adentro a
/// propósito.
pub fn render_image_from_rgba(width: u32, height: u32, mut rgba: Vec<u8>) -> RenderImage {
    debug_assert_eq!(
        rgba.len(),
        (width * height * 4) as usize,
        "el readback y las dimensiones del frame no coinciden"
    );

    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }

    let buffer = image::RgbaImage::from_raw(width, height, rgba)
        .expect("el readback ya validó el tamaño; un `None` acá es un bug de bytes");
    let frame = image::Frame::from_parts(
        buffer,
        0,
        0,
        image::Delay::from_numer_denom_ms(100, 1),
    );

    RenderImage::new([frame])
}

/// Hash estable de un waveform, para decidir si hay que re-subir la malla.
///
/// FNV-1a: no es criptográfico, sólo tiene que cambiar cuando cambian los
/// samples. Se hashean los **bits** de cada `f32` y no el valor, para que dos
/// tablas que suenan distintas pero con los mismos `f32` (por ejemplo con
/// distinta tolerancia de redondeo) no se comparen como iguales por aproximación.
///
/// El cero se normaliza a `+0.0`: `-0.0` y `0.0` producen exactamente la misma
/// geometría, así que distinguirlos sólo serviría para re-subir la malla sin que
/// nada cambie en pantalla.
pub fn hash_waveform(waveform: &[f32]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = OFFSET;
    for sample in waveform {
        let bits = if *sample == 0.0 { 0.0f32.to_bits() } else { sample.to_bits() };
        for byte in bits.to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(PRIME);
        }
    }
    hash
}

