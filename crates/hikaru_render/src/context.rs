// crates/hikaru_render/src/context.rs

//! Creación del contexto de GPU (instancia, adapter, device y queue) para
//! render offscreen.

/// Contexto de GPU para renders offscreen.
///
/// No está asociado a ninguna ventana: se usa para renderizar a texturas en
/// memoria (bake de assets, thumbnails, renders de la wavetable). La
/// presentación en pantalla queda para una etapa posterior.
pub struct GpuContext {
    /// Instancia de wgpu. Mantiene vivos los backends habilitados.
    pub instance: wgpu::Instance,
    /// Adapter elegido, útil para consultar límites y features soportadas.
    pub adapter: wgpu::Adapter,
    /// Device de render.
    pub device: wgpu::Device,
    /// Queue para enviar comandos.
    pub queue: wgpu::Queue,
}

impl GpuContext {
    /// Crea un contexto offscreen usando el adapter por defecto.
    ///
    /// Se piden los límites que reporta el adapter. Para los renders previstos
    /// (texturas chicas: perillas, faders) eso sobra; si más adelante hace
    /// falta raytracing o texturas grandes, hay que subir los
    /// `required_limits` explícitos en el `DeviceDescriptor`.
    pub async fn new_offscreen() -> Result<Self, ContextError> {
        let instance = wgpu::Instance::default();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .map_err(ContextError::Adapter)?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("hikaru_render::device"),
                // Sin features opcionales: sólo lo básico, soportado por todos
                // los backends. Se agregan explícitamente si hacen falta.
                required_features: wgpu::Features::empty(),
                required_limits: adapter.limits(),
                ..Default::default()
            })
            .await
            .map_err(ContextError::Device)?;

        Ok(Self { instance, adapter, device, queue })
    }

    /// Formato de color para las texturas offscreen de este crate.
    ///
    /// Fijado a `Rgba8UnormSrgb`: es el formato con sampling correcto para
    /// assets de interfaz y lo soportan todos los backends.
    ///
    /// En wgpu 29 ya no existe `Adapter::get_preferred_texture_format` (era
    /// para elegir el formato de una *surface*). Para render offscreen no hay
    /// negotiate que hacer: el formato lo elegimos nosotros.
    ///
    /// Ojo con el nombre: esto cubre **solo** render a textura. Para presentar
    /// en una ventana hay que usar [`GpuContext::surface_format`], que sí
    /// depende del hardware.
    pub const fn preferred_texture_format() -> wgpu::TextureFormat {
        wgpu::TextureFormat::Rgba8UnormSrgb
    }

    /// Formato de presentación para una ventana, según lo que soporte el
    /// adapter.
    ///
    /// A diferencia de [`GpuContext::preferred_texture_format`], acá el
    /// hardware manda: una `Surface` sólo acepta formatos de su lista de
    /// capacidades, y la presentación también suele exigir `Bgra8UnormSrgb`
    /// (es el formato nativo de la mayoría de las compositors Wayland/X11).
    ///
    /// Devuelve `None` si la surface es incompatible con el adapter o si no
    /// soporta ninguno de los dos formatos sRGB habituales; en ese caso no
    /// hay nada que presentar y hay que reconfigurar la surface.
    pub fn surface_format(
        &self,
        capabilities: &wgpu::SurfaceCapabilities,
    ) -> Option<wgpu::TextureFormat> {
        const CANDIDATES: [wgpu::TextureFormat; 2] = [
            wgpu::TextureFormat::Bgra8UnormSrgb,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        ];
        CANDIDATES
            .into_iter()
            .find(|format| capabilities.formats.contains(format))
    }

    /// Formato de profundidad usado por los pipelines 3D.
    ///
    /// `Depth24Plus` está garantizado por la especificación de WebGPU para
    /// texturas de profundidad, así que no hace falta negociar nada.
    pub const fn depth_format() -> wgpu::TextureFormat {
        wgpu::TextureFormat::Depth24Plus
    }

    /// Crea la textura de destino (color + profundidad) para un render
    /// offscreen del tamaño dado.
    ///
    /// Concentrar acá el `TextureDescriptor` evita que cada renderer
    /// ([`crate::quad`], [`crate::mesh`]) olvide un usage y se encuentre con un
    /// `AccessError` en la primera sesión de la GUI.
    ///
    /// La textura de color lleva siempre `COPY_SRC`: es lo que permite
    /// leerla de vuelta con [`GpuContext::read_color_rgba8`] y así llevar el
    /// render a un `RenderImage` de GPUI Kit. Cuesta nada si nadie la lee, y
    /// agregar el usage más tarde obligaría a recrear todos los targets.
    ///
    /// # Dimensiones
    ///
    /// El ancho y el alto se amoldan a un mínimo de 1. wgpu rechaza una textura
    /// 0x0 con un error de validación opaco que no dice qué dimensión es el
    /// problema, y un tamaño 0 puede entrar por el camino real del layout: un
    /// panel que todavía no tiene tamaño en el primer frame del layout, o un
    /// viewport redimensionado a cero durante un minimize.
    ///
    /// Un target de 1x1 es inútil pero perfectamente válido: el pipeline de
    /// render se comporta igual y se lee como una imagen diminuta en vez de
    /// romper el contexto. El recorte real a 0x0 lo hace quien pide el render,
    /// que sí puede decidir qué hacer con un tamaño sin sentido.
    pub fn create_render_target(
        &self,
        label: &str,
        width: u32,
        height: u32,
        with_depth: bool,
    ) -> RenderTarget {
        let width = width.max(1);
        let height = height.max(1);

        let color = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::preferred_texture_format(),
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        let depth = with_depth.then(|| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: Self::depth_format(),
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
        });

        RenderTarget {
            color_view: color.create_view(&wgpu::TextureViewDescriptor::default()),
            depth_view: depth
                .as_ref()
                .map(|texture| texture.create_view(&wgpu::TextureViewDescriptor::default())),
            color,
            depth,
            color_format: Self::preferred_texture_format(),
            size: (width, height),
        }
    }

    /// Lee la textura de color de un target y la devuelve como bytes RGBA8.
    ///
    /// Es el puente entre el render offscreen y la GUI: GPUI Kit compone sus
    /// elementos sobre su propia superficie y no expone un `TextureView` de
    /// wgpu arbitrario, así que la única forma de meter un render 3D en el
    /// layout 2D es leer los píxeles y subirlos como imagen.
    ///
    /// El formato del target es `Rgba8UnormSrgb` (ver
    /// [`GpuContext::preferred_texture_format`]), así que los bytes vuelven en
    /// **RGBA, codificados en sRGB**: es exactamente lo que espera un decoder
    /// de imágenes, sin ninguna conversión de espacio de color de por medio.
    ///
    /// # Bloquea el hilo
    ///
    /// `map_async` es asíncrono, pero el readback se fuerza acá con
    /// `device.poll(PollType::wait())`. Es deliberado: el llamador es el hilo
    /// de UI de GPUI, y el render es de unos pocos KiB (una cinta de
    /// 320x200 RGBA son 256 KB), así que la espera es del orden del
    /// milisegundo. Encadenarlo como future obligaría a pumping manual del
    /// executor de GPUI desde el render, que es bastante más frágil.
    ///
    /// Los errores de validación de wgpu no se detectan acá: si la copia está
    /// mal declarada, el error llega por el scope del device y el buffer
    /// aparece sin inicializar. Por eso los tests de humo envuelven el render
    /// en un error scope.
    pub fn read_color_rgba8(&self, target: &RenderTarget) -> Result<Vec<u8>, ReadbackError> {
        let (width, height) = target.size;
        if width == 0 || height == 0 {
            return Err(ReadbackError::DegenerateSize { width, height });
        }

        let unpadded_bytes_per_row = width as usize * 4;
        // wgpu exige que `bytes_per_row` sea múltiplo de 256. Con anchos que no
        // lo son (cualquiera que no sea múltiplo de 64 píxeles) hay que
        // sobre-reservar y des-padear al copiar.
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;

        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hikaru_render::readback"),
            size: (padded_bytes_per_row * height as usize) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("hikaru_render::readback::copy"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target.color,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row as u32),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        self.queue.submit(Some(encoder.finish()));

        // El callback del map corre en un hilo de wgpu: se pasa el resultado por
        // un canal y se fuerza el device a avanzar hasta que llegue.
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
            // Si el poll cortó antes por timeout, el receiver ya no está y el
            // envío falla: se ignora, el error de abajo ya se/reportó.
            let _ = sender.send(result);
        });

        // El timeout es lo que evita que un driver colgado congele la GUI de
        // forma indefinida: peor un frame sin la imagen 3D que un freeze.
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(MAP_TIMEOUT),
            })
            .map_err(ReadbackError::Poll)?;

        // Tras un poll OK los callbacks ya se invocaron, así que el recv no
        // debería bloquear; el timeout es sólo una red por si el backend
        // WebGPU (donde el poll no bloquea) llega a este camino.
        match receiver.recv_timeout(MAP_TIMEOUT) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(ReadbackError::Map(error)),
            Err(_) => return Err(ReadbackError::MapAborted),
        }

        let mapped = buffer.slice(..).get_mapped_range();
        // Se des-padea fila por fila: el padding es basura de alineamiento y no
        // forma parte de la imagen.
        let mut pixels = Vec::with_capacity(unpadded_bytes_per_row * height as usize);
        for row in 0..height as usize {
            let start = row * padded_bytes_per_row;
            pixels.extend_from_slice(&mapped[start..start + unpadded_bytes_per_row]);
        }
        drop(mapped);
        buffer.unmap();

        Ok(pixels)
    }
}

/// Texturas de destino listas para un render pass.
pub struct RenderTarget {
    /// Vista de la textura de color.
    pub color_view: wgpu::TextureView,
    /// Vista de profundidad, si se pidió.
    pub depth_view: Option<wgpu::TextureView>,
    /// Textura de color. Se guarda además de la vista porque sólo desde la
    /// textura se puede pedir un `copy_texture_to_buffer`: una `TextureView` no
    /// conserva el handle de la textura.
    pub color: wgpu::Texture,
    /// Textura de profundidad, si se pidió. Vive acá por el mismo motivo que
    /// [`RenderTarget::color`]: permite recrear el target conservando los
    /// recursos, en vez de dejarlos al GC de wgpu.
    pub depth: Option<wgpu::Texture>,
    /// Formato de la textura de color, para armar el render pass.
    pub color_format: wgpu::TextureFormat,
    /// Tamaño del target en píxeles. Lo necesita el readback para saber
    /// cuántas filas tiene que des-padear.
    pub size: (u32, u32),
}

/// Errores al leer un target de vuelta a CPU.
#[derive(Debug)]
pub enum ReadbackError {
    /// El target tiene dimensión cero, que es lo único que wgpu acepta.
    DegenerateSize {
        /// Ancho del target.
        width: u32,
        /// Alto del target.
        height: u32,
    },
    /// El device no terminó el trabajo pendiente.
    Poll(wgpu::PollError),
    /// El map del buffer falló.
    Map(wgpu::BufferAsyncError),
    /// El device terminó el poll sin que el map se completara.
    MapAborted,
}

impl std::fmt::Display for ReadbackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadbackError::DegenerateSize { width, height } => {
                write!(f, "el target mide {width}x{height}, no se puede leer")
            }
            ReadbackError::Poll(error) => write!(f, "el poll del device falló: {error}"),
            ReadbackError::Map(error) => write!(f, "no se pudo mapear el buffer: {error}"),
            ReadbackError::MapAborted => {
                write!(f, "el map se abortó antes de completarse")
            }
        }
    }
}

impl std::error::Error for ReadbackError {}

/// Tope de espera del readback. La GUI llama a
/// [`GpuContext::read_color_rgba8`] desde el hilo de UI, así que un driver
/// colgado no puede dejar la espera abierta para siempre.
const MAP_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

/// Errores al armar el contexto de GPU.
#[derive(Debug)]
pub enum ContextError {
    /// No se encontró ningún adapter usable (GPU ausente, drivers faltantes o
    /// Vulkan deshabilitado en el entorno).
    Adapter(wgpu::RequestAdapterError),
    /// El adapter existe pero rechazó el `DeviceDescriptor` pedido.
    Device(wgpu::RequestDeviceError),
}

impl std::fmt::Display for ContextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContextError::Adapter(e) => write!(f, "no se encontró un adapter de GPU: {e}"),
            ContextError::Device(e) => write!(f, "el adapter rechazó el device pedido: {e}"),
        }
    }
}

impl std::error::Error for ContextError {}
