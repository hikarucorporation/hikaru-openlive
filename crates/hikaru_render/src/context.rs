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
    pub fn create_render_target(
        &self,
        label: &str,
        width: u32,
        height: u32,
        with_depth: bool,
    ) -> RenderTarget {
        let color = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::preferred_texture_format(),
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
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
            color_format: Self::preferred_texture_format(),
        }
    }
}

/// Texturas de destino listas para un render pass.
pub struct RenderTarget {
    /// Vista de la textura de color.
    pub color_view: wgpu::TextureView,
    /// Vista de profundidad, si se pidió.
    pub depth_view: Option<wgpu::TextureView>,
    /// Formato de la textura de color, para armar el render pass.
    pub color_format: wgpu::TextureFormat,
}

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
