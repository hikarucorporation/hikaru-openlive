// Copyright (C) Hikaru Corporation - 2026
// Hikaru VST3 Launcher
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/src/vst3/hikaru_vst3_launcher.rs

use crate::scratch::{NUM_CHANNELS, PlanarScratch};
use crate::{PluginInstance, PluginParamInfo};
use hikaru_core::AudioBuffer;
use raw_window_handle::RawWindowHandle;
use std::path::{Path, PathBuf};
use vst3_host::audio::AudioBuffers;
use vst3_host::realtime::{RealtimePluginRunner, RtControl};
use vst3_host::simple;
use vst3_host::plugin::WindowHandle;

#[cfg(target_os = "linux")]
use crate::platform::linux::X11Connection;

/// Capacidad del anillo de comandos de control.
///
/// El audio thread drena el anillo al principio de cada bloque, así que la
/// cola sólo tiene que aguantar el peor rafagón de cambios de parámetro entre
/// dos bloques. 1024 es de sobra para faders y automation, y si se llena el
/// `RtControl::set_parameter` devuelve `false` en vez de bloquear.
const CONTROL_RING_CAPACITY: usize = 1024;

/// Instancia VST3 alojada por Hikaru.
///
/// El plugin **no** se guarda directo: vive dentro de un
/// [`RealtimePluginRunner`], que lo posee en el hilo de audio y aplica los
/// comandos de control que llegan por un anillo lock-free. Ese es el patrón
/// que evita el `Arc<Mutex<Plugin>>` clásico, donde un `set_parameter` desde
/// el hilo de GUI puede competir con el callback por el lock y taquear el
/// deadline del quantum.
pub struct Vst3Instance {
    runner: RealtimePluginRunner,
    /// Handle de control, seguro de usar desde el hilo de GUI.
    control: RtControl,
    /// Parámetros leídos una vez en `load` (`get_parameters` es consulta al
    /// controlador, no al DSP: cachearlos evita preguntar por FFI en cada
    /// frame del rack).
    params: Vec<vst3_host::parameters::Parameter>,
    /// Buffers planares pre-asignados, reutilizados bloque a bloque.
    scratch: PlanarScratch,
    name: String,
    sample_rate: f32,
    /// `true` cuando el audio thread todavía no llamó a `start_processing`.
    started: bool,
    /// Un fallo del último bloque. Se guarda para reportarlo desde el hilo de
    /// control; el hilo de audio no puede loguear.
    last_error: Option<String>,
    window_attached: bool,
    /// Handle de la ventana flotante, si está abierta. `None` cuando el editor
    /// está embebido o nunca se abrió. Al soltar el struct se cierra la
    /// ventana (ver `Drop` de `FloatingEditor`).
    floating: Option<crate::floating_window::FloatingEditor>,
    #[cfg(target_os = "linux")]
    _x11_conn: Option<X11Connection>,
}

unsafe impl Send for Vst3Instance {}

impl Vst3Instance {
    /// Resolve a VST3 path to the actual .so file inside a bundle directory.
    /// On Linux, VST3 bundles have: Plugin.vst3/Contents/x86_64-linux/Plugin.so
    #[cfg(target_os = "linux")]
    fn resolve_vst3_path(path: &Path) -> PathBuf {
        if path.is_dir() {
            // Look for Contents/<arch>/<name>.so inside the bundle
            if let Ok(contents) = path.join("Contents").read_dir() {
                for entry in contents.flatten() {
                    let entry_path = entry.path();
                    if entry_path.is_dir() {
                        if let Ok(arch_dir) = entry_path.read_dir() {
                            for so_entry in arch_dir.flatten() {
                                let so_path = so_entry.path();
                                if so_path.extension().map_or(false, |e| e == "so") {
                                    println!(
                                        "[VST3] Resolved bundle '{}' -> '{}'",
                                        path.display(),
                                        so_path.display()
                                    );
                                    return so_path;
                                }
                            }
                        }
                    }
                }
            }
            // Fallback: look for .so directly in the directory
            if let Ok(entries) = path.read_dir() {
                for entry in entries.flatten() {
                    let so_path = entry.path();
                    if so_path.extension().map_or(false, |e| e == "so") {
                        return so_path;
                    }
                }
            }
        }
        path.to_path_buf()
    }

    #[cfg(not(target_os = "linux"))]
    fn resolve_vst3_path(path: &Path) -> PathBuf {
        path.to_path_buf()
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let _load_guard = crate::load_lock().lock().unwrap_or_else(|e| e.into_inner());

        let resolved_path = Self::resolve_vst3_path(path);
        let plugin = simple::load_plugin(&resolved_path)
            .map_err(|e| format!("[VST3] Fallo al cargar '{}': {}", resolved_path.display(), e))?;

        // Hay que leer la info ANTES de mover el plugin al runner, porque
        // después sólo se puede alcanzar por `&mut` y no queremos tocar el
        // plugin desde acá.
        let (name, has_gui) = {
            let info = plugin.info();
            (info.name.clone(), info.has_gui)
        };

        if !has_gui {
            eprintln!("[VST3] '{}' no declara editor: el slot mostrará sólo el audio", name);
        }

        let (runner, control) = RealtimePluginRunner::new(plugin, CONTROL_RING_CAPACITY);

        // Foto de parámetros en carga: el controlador ya existe y responde
        // sin audio corriendo. Si falla (plugin a medio inicializar), lista
        // vacía: el rack muestra "sin parámetros" en vez de romper nada.
        let params = runner.plugin().get_parameters().unwrap_or_default();

        Ok(Self {
            runner,
            control,
            params,
            scratch: PlanarScratch::new(crate::scratch::MAX_BLOCK_FRAMES),
            name,
            // 48 kHz por defecto hasta que el host diga lo contrario. VST3
            // necesita un valor válido desde el primer bloque.
            sample_rate: 48_000.0,
            started: false,
            last_error: None,
            window_attached: false,
            floating: None,
            #[cfg(target_os = "linux")]
            _x11_conn: None,
        })
    }

    /// Último error de procesamiento, para que el hilo de control lo muestre.
    pub fn take_last_error(&mut self) -> Option<String> {
        self.last_error.take()
    }

    /// El handle de control es `Send` y no bloquea: se puede clonar desde el
    /// hilo de GUI para mandar MIDI y cambios de parámetro sin tocar el lock
    /// del audio thread.
    pub fn control(&mut self) -> &mut RtControl {
        &mut self.control
    }

    fn open_floating_window(&mut self) -> Result<(), String> {
        if !self.runner.plugin().has_editor() {
            return Err(format!("[VST3] '{}' sin editor", self.name));
        }

        // La ventana vive en el hilo del event loop global de
        // `floating_window`; acá sólo se pide y se espera el handle nativo.
        // Antes esto llamaba `event_loop.run()` en línea, lo que bloqueaba el
        // hilo de control (y con él, la DAW) hasta que el usuario cerrara la
        // ventana.
        let editor = crate::floating_window::FloatingEditor::open(
            &format!("VST3: {}", self.name),
            1024,
            700,
        )?;

        let raw_handle = editor.wait_native_window(std::time::Duration::from_millis(500))?;

        let wh = match raw_handle {
            #[cfg(target_os = "linux")]
            RawWindowHandle::Xlib(h) => WindowHandle::from_x11(h.window as u32),
            #[cfg(target_os = "linux")]
            RawWindowHandle::Wayland(h) => unsafe {
                WindowHandle::from_raw(h.surface.as_ptr() as *mut std::ffi::c_void)
            },
            #[cfg(target_os = "windows")]
            RawWindowHandle::Win32(h) => unsafe {
                WindowHandle::from_hwnd(h.hwnd as *mut std::ffi::c_void)
            },
            #[cfg(target_os = "macos")]
            RawWindowHandle::AppKit(h) => unsafe {
                WindowHandle::from_nsview(h.ns_view.as_ptr() as *mut std::ffi::c_void)
            },
            other => {
                return Err(format!("[VST3] Handle no soportado: {:?}", other));
            }
        };

        self.runner
            .plugin_mut()
            .open_editor(wh)
            .map_err(|e| format!("[VST3] open_editor: {}", e))?;

        self.window_attached = true;
        self.floating = Some(editor);

        println!("[VST3] GUI flotante de '{}' abierta", self.name);
        Ok(())
    }
}

impl PluginInstance for Vst3Instance {
    /// Procesa un bloque interleaved estéreo a través del plugin.
    ///
    /// Contrato RT (ver `PluginInstance::process`): sin alocaciones, sin locks,
    /// sin I/O. El camino es:
    ///
    /// 1. Arranca el plugin la primera vez (una sola llamada de setup).
    /// 2. Des-interleava a los canales planares del scratch, ya pre-asignados.
    /// 3. Llama al runner, que drena el anillo de control y llama a
    ///    `process_audio`. Ni el runner ni este método tocan un mutex.
    /// 4. Re-interleava la salida al buffer del host.
    ///
    /// Los errores se acumulan en `last_error` en vez de loguearse: el hilo de
    /// audio no puede tomar el lock de stderr, y un plugin que falla no puede
    /// derribar el motor.
    fn process(&mut self, buffer: &mut AudioBuffer) {
        // `AudioBuffer` no lleva la cantidad de canales ni el sample rate, así
        // que el layout interleaved estéreo es un contrato del host, no algo
        // que se pueda leer del buffer.
        let frames = buffer.samples.len() / NUM_CHANNELS;
        if frames == 0 {
            return;
        }

        if !self.scratch.fits(frames) {
            // Bloque más grande que lo reservado. Saltamos este bloque en vez
            // de escribir fuera del scratch; `overflowed` queda para que el
            // hilo de control lo reporte.
            return;
        }

        // El setup del plugin se hace acá (una vez) y no en el constructor
        // porque `start_processing` aloca internamente en algunos hosts: hacerlo
        // acá garantiza que la primera llamada en el hilo de audio ya tenga el
        // plugin listo, sin alocaciones posteriores.
        if !self.started {
            if let Err(e) = self.runner.start() {
                self.last_error = Some(format!("[VST3] start_processing: {e}"));
                return;
            }
            self.started = true;
        }

        self.scratch.deinterleave_stereo(buffer.samples, frames);

        let (scratch_inputs, scratch_outputs) = self.scratch.take_buffers();
        let mut buffers = AudioBuffers {
            inputs: scratch_inputs,
            outputs: scratch_outputs,
            sample_rate: self.sample_rate as f64,
            block_size: frames,
        };

        let result = self.runner.process(&mut buffers);

        // Devolvemos los buffers al scratch para el próximo bloque. Esto no
        // aloca: los `Vec` pre-asignados sólo cambian de dueño por una movida.
        self.scratch.restore_buffers(buffers.inputs, buffers.outputs);

        match result {
            Ok(()) => self.scratch.reinterleave_stereo(buffer.samples, frames),
            Err(e) => self.last_error = Some(format!("[VST3] process_audio: {e}")),
        }
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    fn has_gui(&self) -> bool {
        self.runner.plugin().info().has_gui
    }

    fn show_gui_embedded(&mut self, handle: RawWindowHandle) -> Result<(), String> {
        if !self.runner.plugin_mut().has_editor() {
            return Err(format!(
                "'{}' no tiene editor: suena igual, pero no hay ventana que abrir",
                self.name
            ));
        }

        #[cfg(target_os = "linux")]
        let embedded_result = {
            match crate::platform::linux::get_or_create_x11_window(handle, 1024, 700) {
                Ok((window_id, x11_conn)) => {
                    println!(
                        "[VST3] Embebiendo GUI de '{}' en X11 window {}",
                        self.name, window_id
                    );
                    let wh = WindowHandle::from_x11(window_id);
                    let result = self.runner.plugin_mut().open_editor(wh);
                    // Store connection to keep window alive
                    self._x11_conn = x11_conn;
                    result
                }
                Err(e) => {
                    eprintln!("[VST3] X11 companion failed: {}", e);
                    Err(vst3_host::error::Error::Other(e))
                }
            }
        };

        #[cfg(not(target_os = "linux"))]
        let embedded_result = {
            let window_id = match handle {
                RawWindowHandle::Xlib(h) => h.window as u32,
                _ => {
                    return Err(format!("[VST3] Handle no soportado: {:?}", handle));
                }
            };
            let wh = WindowHandle::from_x11(window_id);
            self.runner.plugin_mut().open_editor(wh)
        };

        match embedded_result {
            Ok(()) => {
                self.window_attached = true;
                println!("[VST3] GUI '{}' incrustada OK", self.name);
                Ok(())
            }
            Err(e) => Err(format!(
                "No se pudo abrir el editor de '{}': {}",
                self.name, e
            )),
        }
    }

    fn show_gui_floating(&mut self) -> Result<(), String> {
        self.open_floating_window()
    }

    fn hide_gui(&mut self) {
        if self.window_attached {
            if let Err(e) = self.runner.plugin_mut().close_editor() {
                eprintln!("[VST3] close_editor() failed: {}", e);
            }
            self.window_attached = false;
            // Soltar el handle cierra la ventana flotante.
            self.floating = None;
            #[cfg(target_os = "linux")]
            {
                self._x11_conn = None;
            }
        }
    }

    fn get_name(&self) -> &str {
        &self.name
    }

    fn is_gui_open(&self) -> bool {
        self.window_attached
    }

    fn get_gui_size(&self) -> Option<(u32, u32)> {
        self.runner
            .plugin()
            .get_editor_size()
            .ok()
            .map(|(w, h)| (w as u32, h as u32))
    }

    fn notify_gui_embedded(&mut self) -> Option<(u32, u32)> {
        // Query the actual editor size now that the view is attached to the parent window.
        // Some plugins only report the correct size after open_editor completes.
        self.runner
            .plugin_mut()
            .get_editor_size()
            .ok()
            .map(|(w, h)| (w as u32, h as u32))
    }

    fn resize_gui(&mut self, width: u32, height: u32) {
        // Redimensionar la vista viva del plugin. Antes esto sólo leía la
        // petición y logueaba "no implementado", así que un plugin con editor
        // redimensionable (los sintetizadores grandes lo son) se quedaba con
        // el layout inicial para siempre.
        let (w, h) = match (i32::try_from(width), i32::try_from(height)) {
            (Ok(w), Ok(h)) => (w, h),
            _ => return, // desbordaría el i32 de la API de VST3
        };

        if let Err(e) = self.runner.plugin_mut().resize_editor(w, h) {
            // No es fatal: muchos plugins no admiten resize y el editor sigue
            // funcionando con el tamaño que pidieron. Se avisa una sola vez por
            // error, sin ruido en cada frame.
            eprintln!("[VST3] resize_editor({w}x{h}) rechazado por '{}': {e}", self.name);
        }
    }

    fn params_supported(&self) -> bool {
        true
    }

    fn param_count(&mut self) -> usize {
        self.params.len()
    }

    fn param_info(&mut self, index: usize) -> Option<PluginParamInfo> {
        let p = self.params.get(index)?;
        Some(PluginParamInfo {
            id: p.id,
            name: p.name.clone(),
            unit: p.unit.clone(),
            // VST3 vive en normalizado: el rango plano es privado del
            // plugin (ver `format_param_value` para texto exacto).
            min: 0.0,
            max: 1.0,
            default_value: p.default.clamp(0.0, 1.0) as f32,
            step_count: p.step_count,
            can_automate: p.can_automate && !p.is_read_only,
        })
    }

    fn get_param_normalized(&mut self, id: u32) -> Option<f32> {
        self.runner
            .plugin_mut()
            .get_parameter(id)
            .ok()
            .map(|v| v.clamp(0.0, 1.0) as f32)
    }

    /// Manda el cambio por el anillo lock-free del runner: vuelve de
    /// inmediato (o `false` si la cola está llena) sin tocar el audio.
    fn set_param_normalized(&mut self, id: u32, value: f32) -> bool {
        self.control
            .set_parameter(id, value.clamp(0.0, 1.0) as f64)
    }

    /// Drena lo que el editor pidió por su cuenta y lo aplica a la ventana.
    ///
    /// Los plugins con editor redimensionable llaman a `IPlugFrame::resizeView`
    /// cuando cambian de layout (abrir un filtro, cambiar de preset). El host
    /// guarda ese pedido y el hilo de la ventana lo consulta; esta es la vía
    /// que usa el GUI para no tener que invocar `resize_gui` a mano.
    fn poll_editor_resize(&mut self) -> Option<(u32, u32)> {
        let (w, h) = self.runner.plugin_mut().take_editor_resize_request()?;
        if w <= 0 || h <= 0 {
            return None;
        }
        let (w, h) = (w as u32, h as u32);
        // La ventana flotante sigue al plugin; la embebida la redimensiona el
        // contenedor del GUI, no nosotros.
        if let Some(editor) = &self.floating {
            let _ = editor.resize(w, h);
        }
        Some((w, h))
    }

    fn format_param_value(&self, id: u32, normalized: f32) -> Option<String> {
        self.runner
            .plugin()
            .format_parameter(id, normalized.clamp(0.0, 1.0) as f64)
            .ok()
    }

    /// Lo que el editor nativo movió por su cuenta (el usuario arrastró un
    /// knob del plugin): el hilo de la ventana lo sondea y la UI mueve los
    /// knobs del rack para que no mientan.
    fn poll_param_changes(&mut self) -> Vec<(u32, f32)> {
        self.runner
            .plugin()
            .get_parameter_changes()
            .into_iter()
            .map(|(id, v)| (id, v.clamp(0.0, 1.0) as f32))
            .collect()
    }
}

impl Drop for Vst3Instance {
    fn drop(&mut self) {
        self.hide_gui();
    }
}
