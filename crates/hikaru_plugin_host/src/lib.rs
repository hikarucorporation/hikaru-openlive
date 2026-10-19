// Copyright (C) Hikaru Corporation - 2026
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/src/lib.rs

pub mod vst3;
pub mod clap;
pub mod platform;
pub mod registry;
pub mod scratch;
pub mod floating_window;
pub mod tap;

// El envoltorio de ventana vive en `hikaru-wrapper/`, al lado de `src/` y no
// dentro. El `#[path]` con `..` es lo que permite que el directorio físico sea
// ese y no `src/hikaru_wrapper/`: la carpeta del módulo tiene que poder
// auditarse por separado de la del crate, porque es la que tiene que explicar
// por qué el editor del plugin no es una ventana del escritorio.
#[path = "../hikaru-wrapper/mod.rs"]
pub mod hikaru_wrapper;

pub use tap::PluginTap;
pub use hikaru_wrapper::{
    HostFrame, HostRelation, WrapperConfig, WrapperStyle, WrapperWindowType,
};

/// El envoltorio de ventana completo.
///
/// Se re-exporta desde la raíz porque es lo que el DAW usa para mostrar un
/// editor de plugin como ventana propia y no como una ventana suelta del
/// escritorio. [`hikaru_wrapper`] tiene el detalle.
#[cfg(target_os = "linux")]
pub use hikaru_wrapper::HikaruWrapper;

/// Alias del nombre histórico de [`WrapperConfig`].
///
/// El módulo pasó a llamarse `hikaru_wrapper` y la config `WrapperConfig`;
/// este alias existe para no romper a los que importaban el nombre viejo.
pub use hikaru_wrapper::WrapperConfig as PluginWrapperConfig;

/// Serializa la instanciación de plugins en todo el proceso.
///
/// Hikaru carga el mismo plugin desde dos hilos a la vez: el motor de audio
/// (cuando entra en la cadena) y el hilo de sondeo de la GUI (que crea una
/// instancia desechable sólo para leer los parámetros de la tarjeta).
///
/// Vital —y varios synths comerciales— no son reentrantes durante
/// `init`/`create_plugin`. La segunda carga solapada con la primera lanza una
/// excepción C++ que cruza el FFI de Rust, y el runtime aborta el proceso
/// entero con *"Rust cannot catch foreign exceptions"*. Una excepción C++ no
/// se puede atrapar con `catch_unwind`: hace falta que nadie las deje pasar.
///
/// Serializar la carga completa cierra la ventana. Es un lock de proceso y no
/// por plugin a propósito: cargar un plugin tarda decenas de milisegundos, así
/// que la contención es despreciable y un lock por ruta agregaría bookkeeping
/// sin ganar nada.
pub(crate) fn load_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    &LOCK
}

use hikaru_core::AudioBuffer;
pub use raw_window_handle::RawWindowHandle;
use std::path::Path;

pub use registry::{PluginDescriptor, PluginKind, PluginScanError, ScanReport};
pub use scratch::PlanarScratch;
pub use vst3::Vst3Instance;
pub use clap::ClapInstance;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginFormat {
    VST3,
    CLAP,
}

/// Handle nativo del DAW, capturado en el hilo de la GUI y enviado al hilo
/// de la ventana del plugin.
///
/// Es `Copy + Send` a propósito: `RawWindowHandle` contiene punteros crudos
/// (Wayland) que no cruzan hilos, así que en el click se reduce a este enum
/// mínimo. En X11 lleva el XID de la ventana de GPUI para emparentar la del
/// plugin (`WM_TRANSIENT_FOR`); en Wayland no hay XID que emparentar y el
/// hilo cae al flotante de XWayland.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostParentKind {
    Xlib(u64),
    Xcb(u32),
    Wayland,
    Unknown,
}

impl HostParentKind {
    pub fn from_raw(handle: RawWindowHandle) -> Self {
        match handle {
            RawWindowHandle::Xlib(h) => Self::Xlib(h.window as u64),
            RawWindowHandle::Xcb(h) => Self::Xcb(h.window.get().into()),
            RawWindowHandle::Wayland(_) => Self::Wayland,
            _ => Self::Unknown,
        }
    }

    /// XID de la ventana del DAW cuando la sesión es X11/XWayland.
    pub fn x11_id(self) -> Option<u32> {
        match self {
            Self::Xlib(id) => Some(id as u32),
            Self::Xcb(id) => Some(id),
            Self::Wayland | Self::Unknown => None,
        }
    }
}

/// Descripción de un parámetro expuesto por el plugin, en espacio
/// normalizado `0.0..=1.0`.
///
/// VST3 sólo habla normalizado (el mapeo a Hz/dB es privado del plugin; para
/// texto exacto está `format_param_value`, que le pregunta al plugin en
/// vivo). CLAP expone rangos planos, que el host reduce a normalizado al
/// leer. Los knobs del rack trabajan siempre en este espacio: un gesto vale
/// para ambos formatos sin ramas por formato en la UI.
#[derive(Debug, Clone)]
pub struct PluginParamInfo {
    pub id: u32,
    pub name: String,
    pub unit: String,
    pub min: f32,
    pub max: f32,
    pub default_value: f32,
    pub step_count: i32,
    pub can_automate: bool,
}

impl PluginParamInfo {
    pub fn is_boolean(&self) -> bool {
        self.step_count == 2
    }

    pub fn is_discrete(&self) -> bool {
        self.step_count > 1
    }

    /// Texto honesto para el valor: discreto exacto, continuo en % (o en
    /// unidades si el plugin declara rango plano). No inventa Hz/dB: eso
    /// sólo lo sabe el plugin (`format_param_value`).
    pub fn display_value(&self, normalized: f32) -> String {
        let n = normalized.clamp(0.0, 1.0);
        if self.is_boolean() {
            return if n >= 0.5 { "On".to_string() } else { "Off".to_string() };
        }
        if self.is_discrete() {
            let steps = self.step_count as f32;
            let plain = self.min + (n * steps).round() / steps * (self.max - self.min);
            return if self.unit.is_empty() {
                format!("{plain:.0}")
            } else {
                format!("{plain:.0} {}", self.unit)
            };
        }
        if self.unit.is_empty() {
            format!("{:.1}%", n * 100.0)
        } else {
            format!("{:.2} {}", self.min + n * (self.max - self.min), self.unit)
        }
    }

    /// Redondea un valor al escalón discreto que le toca (booleanos y
    /// stepped). Los continuos pasan intactos.
    pub fn snap(&self, normalized: f32) -> f32 {
        let n = normalized.clamp(0.0, 1.0);
        if self.is_discrete() {
            let steps = self.step_count as f32;
            ((n * steps).round() / steps).clamp(0.0, 1.0)
        } else {
            n
        }
    }
}

pub trait PluginInstance: Send {
    /// Procesa un bloque de audio.
    ///
    /// Se llama desde el hilo de audio. La implementación **no debe** alocar,
    /// bloquear ni loguear: cualquier `println!` toma el lock de stderr y
    /// cualquier `Mutex::lock()` puede quedar bloqueado por el hilo de control,
    /// y las dos cosas terminan en un xrun del quantum de PipeWire.
    ///
    /// `buffer` viene interleaved estéreo. Quien lo implementa es responsable
    /// de convertirlo a planar (ver [`PlanarScratch`]).
    fn process(&mut self, buffer: &mut AudioBuffer);

    /// Ajusta el sample rate del host.
    ///
    /// VST3 lo exige: sin él, el `ProcessContext` que se arma en cada bloque
    /// lleva un sample rate equivocado, y todo plugin que derive retardos de
    /// fase o frecuencias a partir de él suena mal. Lo llama el hilo de
    /// control, nunca el de audio.
    fn set_sample_rate(&mut self, _sample_rate: f32) {}

    /// Si el plugin declara editor. La GUI lo consulta para decidir si muestra
    /// el botón de abrir editor.
    fn has_gui(&self) -> bool {
        false
    }

    /// Latencia introducida por el plugin, en samples. La reporta el host para
    /// compensarla contra el resto de la señal.
    fn latency_samples(&self) -> u32 {
        0
    }

    /// Incrusta el editor del plugin en una ventana nativa ya existente.
    ///
    /// Devuelve `Err` con un mensaje legible en vez de dejar una ventana
    /// vacía o un hilo zombie: el llamador (hilo de la ventana del plugin)
    /// reporta ese mensaje a la UI por el canal de notificaciones, así un
    /// plugin roto se muestra marcado en el rack sin tocar el hilo del DAW.
    fn show_gui_embedded(&mut self, window_handle: RawWindowHandle) -> Result<(), String>;
    fn show_gui_floating(&mut self) -> Result<(), String>;
    fn hide_gui(&mut self);
    fn get_name(&self) -> &str;

    /// Bompea el editor: el equivalente por formato a un `idle()` del host.
    ///
    /// Lo llama el tick de [`hikaru_wrapper::WrapperEventLoop`], a ritmo fijo
    /// (60 Hz por defecto), desde el hilo de la ventana.
    ///
    /// # Por qué no es opcional para CLAP
    ///
    /// La extensión `timer` de CLAP deja que el plugin registre temporizadores
    /// y espere que el host los dispare. Si nadie los dispara, el plugin se
    /// queda esperando: sus medidores no se redibujan y, si el editor se
    /// invalida desde un callback de temporizador, no vuelve a pintar. Es la
    /// mitad de la razón por la que el display de los plugins CLAP queda
    /// plano (la otra mitad es el audio, ver [`PluginInstance::process`]).
    ///
    /// # Por qué VST3 no hace nada
    ///
    /// El SDK de VST3 no tiene un `idle()` llamado por el host en la versión
    /// de bindings que se usa acá: el temporizador del editor lo maneja el
    /// propio plugin, en su hilo. Lo que sí depende del host es que
    /// `process()` se siga llamando y que los parámetros lleguen al
    /// controlador, y ambas cosas las hace el tick del envoltorio. Por eso la
    /// implementación por defecto es un no-op y no un error.
    ///
    /// # Contrato
    ///
    /// Se llama desde el hilo de la ventana, fuera de la ruta de audio, así
    /// que sí puede asignar, loguear y hablar con el servidor X11. Aun así no
    /// debe bloquear: el tick comparte hilo con el manejo de la ventana.
    fn gui_tick(&mut self) {}

    /// Si el editor está actualmente adjunto a una ventana.
    ///
    /// El hilo de la ventana lo consulta tras `show_gui_embedded` para
    /// decidir si mapea la ventana o si la destruye sin mostrarla y avisa
    /// a la UI con el error correspondiente.
    fn is_gui_open(&self) -> bool {
        false
    }

    fn get_gui_size(&self) -> Option<(u32, u32)> {
        None
    }

    /// Called after `show_gui_embedded` to let the plugin query the actual preferred
    /// size now that the GUI context exists. Returns the preferred `(width, height)`
    /// if the plugin reports one, so the caller can resize the container window.
    fn notify_gui_embedded(&mut self) -> Option<(u32, u32)> {
        None
    }

    fn resize_gui(&mut self, _width: u32, _height: u32) {}

    /// Drena un pedido de resize que el editor del plugin hizo por su cuenta.
    ///
    /// Un sintetizador con editor redimensionable reorganiza su layout al
    /// abrir un filtro o cambiar de preset, y avisa al host. El hilo de la
    /// GUI lo consulta desde `sync_frame` y agranda la ventana; si la ventana
    /// flotante está abierta, el launcher la redimensiona él mismo.
    ///
    /// Devuelve el tamaño pedido, o `None` si no hubo pedido.
    fn poll_editor_resize(&mut self) -> Option<(u32, u32)> {
        None
    }

    /// Si el formato expone parámetros legibles por el host en esta versión.
    ///
    /// VST3 sí (`get_parameters`). CLAP todavía no: su extensión `params` en
    /// `clack-extensions 0.2` es sólo el wrapper crudo sin API segura, y
    /// llamarla a mano es `unsafe` sobre punteros del plugin. Existe para
    /// que la UI distinga "sin parámetros" de "no vinculados".
    fn params_supported(&self) -> bool {
        false
    }

    fn param_count(&mut self) -> usize {
        0
    }

    fn param_info(&mut self, _index: usize) -> Option<PluginParamInfo> {
        None
    }

    /// Valor normalizado actual. Lo llama el hilo que posee la instancia
    /// (sondeo o ventana), nunca el de audio.
    fn get_param_normalized(&mut self, _id: u32) -> Option<f32> {
        None
    }

    /// Cambio originado en la UI del rack. Debe ser lock-free hacia el
    /// audio (anillo, atómicos): nunca `Mutex`, nunca bloquear.
    ///
    /// Devuelve `false` si el plugin lo rechazó o la cola estaba llena (el
    /// cambio se pierde antes que taquear el deadline del quantum).
    fn set_param_normalized(&mut self, _id: u32, _value: f32) -> bool {
        false
    }

    /// Texto exacto del plugin para un valor ("440.00 Hz"). `None` = usar
    /// `PluginParamInfo::display_value` (aproximación honesta).
    fn format_param_value(&self, _id: u32, _normalized: f32) -> Option<String> {
        None
    }

    /// Cambios que el plugin hizo por su cuenta (su propio editor, LFO
    /// interno): `(id, normalizado)`. El hilo de la ventana lo sondea y los
    /// manda a la UI, que mueve los knobs del rack en tiempo real.
    fn poll_param_changes(&mut self) -> Vec<(u32, f32)> {
        Vec::new()
    }

    /// Foto `(info, valor)` de todos los parámetros. La usa el sondeo de la
    /// GUI en su propia instancia temporal: leer no toca el audio.
    fn snapshot_params(&mut self) -> Vec<(PluginParamInfo, f32)> {
        (0..self.param_count())
            .filter_map(|i| {
                let info = self.param_info(i)?;
                if !info.can_automate {
                    return None;
                }
                let value = self
                    .get_param_normalized(info.id)
                    .unwrap_or(info.default_value);
                Some((info, value))
            })
            .collect()
    }
}

/// Aplica valores normalizados a la instancia de audio del motor.
///
/// La llama el hilo de control con lo que drenó de
/// `DspSlot::take_dirty_external_params`. Devuelve cuántos entraron: es el
/// extremo del motor del camino rack → anillo lock-free → bloque.
pub fn apply_params_to_instance(
    instance: &mut dyn PluginInstance,
    params: &[(u32, f32)],
) -> usize {
    params
        .iter()
        .filter(|(id, value)| instance.set_param_normalized(*id, *value))
        .count()
}

/// Crea una ventana nativa independiente (Top-Level) y le adjunta el plugin
pub fn spawn_floating_gui(
    path: &Path, 
    format: PluginFormat
) -> Result<Box<dyn PluginInstance>, String> {
    let path_buf = path.to_path_buf();

    let plugin_instance: Option<Box<dyn PluginInstance>> = match format {
        PluginFormat::VST3 => Vst3Instance::load(&path_buf)
            .map(|inst| Box::new(inst) as Box<dyn PluginInstance>)
            .ok(),
        PluginFormat::CLAP => ClapInstance::load(&path_buf)
            .map(|inst| Box::new(inst) as Box<dyn PluginInstance>)
            .ok(),
    };

    if let Some(mut plugin) = plugin_instance {
        println!("[hikaru_plugin_host] Creando ventana flotante dedicada para '{}'", plugin.get_name());
        plugin.show_gui_floating()?;
        Ok(plugin)
    } else {
        Err(format!("No se pudo cargar el archivo ejecutable: {:?}", path_buf))
    }
}

pub struct PluginHost;

impl PluginHost {
    /// Escanea los directorios estándar de Linux y devuelve el reporte.
    ///
    /// Envuelve [`registry::scan_standard_paths`]; existe sólo como punto de
    /// entrada nominal para las capas superiores (GUI, persistencia).
    pub fn scan_plugins() -> ScanReport {
        registry::scan_standard_paths()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubInstance {
        applied: Vec<(u32, f32)>,
    }

    impl PluginInstance for StubInstance {
        fn process(&mut self, _buffer: &mut AudioBuffer) {}
        fn show_gui_embedded(&mut self, _window_handle: RawWindowHandle) -> Result<(), String> {
            Ok(())
        }
        fn show_gui_floating(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn hide_gui(&mut self) {}
        fn get_name(&self) -> &str {
            "stub"
        }
        fn set_param_normalized(&mut self, id: u32, value: f32) -> bool {
            self.applied.push((id, value));
            true
        }
    }

    fn info() -> PluginParamInfo {
        PluginParamInfo {
            id: 7,
            name: "Cutoff".to_string(),
            unit: String::new(),
            min: 0.0,
            max: 1.0,
            default_value: 0.5,
            step_count: 0,
            can_automate: true,
        }
    }

    #[test]
    fn param_display_continuous_is_percent_without_unit() {
        assert_eq!(info().display_value(0.5), "50.0%");
    }

    #[test]
    fn param_display_boolean_snaps_to_on_off() {
        let mut boolean = info();
        boolean.step_count = 2;
        assert_eq!(boolean.display_value(0.9), "On");
        assert_eq!(boolean.display_value(0.1), "Off");
        assert_eq!(boolean.snap(0.7), 0.5);
    }

    #[test]
    fn param_display_discrete_reports_step_index() {
        let mut stepped = info();
        stepped.step_count = 4;
        stepped.min = 0.0;
        stepped.max = 4.0;
        stepped.unit = "st".to_string();
        assert_eq!(stepped.display_value(0.26), "1 st");
        assert_eq!(stepped.snap(0.9), 1.0);
    }

    #[test]
    fn apply_params_forwards_everything_to_the_instance() {
        let mut instance = StubInstance { applied: Vec::new() };
        let dirty = vec![(1u32, 0.25f32), (2u32, 0.75f32)];
        assert_eq!(apply_params_to_instance(&mut instance, &dirty), 2);
        assert_eq!(instance.applied, dirty);
    }
}
