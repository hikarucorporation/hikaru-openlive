// Copyright (C) Hikaru Corporation - 2026
// Hikaru Clap Launcher
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/src/clap/hikaru_clap_launcher.rs

use crate::PluginInstance;
use clack_extensions::gui::{GuiApiType, GuiConfiguration, GuiSize, HostGui, HostGuiImpl, PluginGui, Window};
use clack_extensions::log::{HostLog, HostLogImpl, LogSeverity};
use clack_extensions::posix_fd::{FdFlags, HostPosixFd, HostPosixFdImpl};
use clack_extensions::timer::{HostTimer, HostTimerImpl, TimerId};
use clack_host::prelude::*;
use hikaru_core::AudioBuffer;
use raw_window_handle::RawWindowHandle;
use std::path::Path;
use std::sync::Mutex;

#[cfg(target_os = "linux")]
use crate::platform::linux::X11Connection;

struct HikaruShared {
    log_to_stdout: bool,
}

impl<'a> SharedHandler<'a> for HikaruShared {
    fn request_restart(&self) {
        eprintln!("[CLAP Host] Plugin request_restart");
    }
    fn request_process(&self) {}
    fn request_callback(&self) {}
}

impl HostLogImpl for HikaruShared {
    fn log(&self, severity: LogSeverity, message: &str) {
        if self.log_to_stdout {
            match severity {
                LogSeverity::Info => println!("[CLAP Plugin] {}", message),
                LogSeverity::Warning => eprintln!("[CLAP Plugin Warning] {}", message),
                LogSeverity::Error => eprintln!("[CLAP Plugin Error] {}", message),
                LogSeverity::Debug => eprintln!("[CLAP Plugin Debug] {}", message),
                _ => eprintln!("[CLAP Plugin Log] {}", message),
            }
        }
    }
}

impl HostGuiImpl for HikaruShared {
    fn resize_hints_changed(&self) {}
    fn request_resize(&self, new_size: GuiSize) -> Result<(), HostError> {
        println!(
            "[CLAP Host] Plugin requests resize to {}x{}",
            new_size.width, new_size.height
        );
        Ok(())
    }
    fn request_show(&self) -> Result<(), HostError> {
        println!("[CLAP Host] Plugin requests show");
        Ok(())
    }
    fn request_hide(&self) -> Result<(), HostError> {
        println!("[CLAP Host] Plugin requests hide");
        Ok(())
    }
    fn closed(&self, was_destroyed: bool) {
        println!(
            "[CLAP Host] GUI closed (was_destroyed={})",
            was_destroyed
        );
    }
}

struct HikaruMainThread<'a> {
    #[allow(dead_code)]
    shared: &'a HikaruShared,
    timers: Mutex<Vec<TimerId>>,
}

impl<'a> MainThreadHandler<'a> for HikaruMainThread<'a> {}

impl HostTimerImpl for HikaruMainThread<'_> {
    fn register_timer(&self, period_ms: u32) -> Result<TimerId, HostError> {
        let id = TimerId(period_ms);
        if let Ok(mut timers) = self.timers.lock() {
            timers.push(id);
        }
        println!("[CLAP Host] Timer registered: {}ms", period_ms);
        Ok(id)
    }

    fn unregister_timer(&self, timer_id: TimerId) -> Result<(), HostError> {
        if let Ok(mut timers) = self.timers.lock() {
            timers.retain(|t| *t != timer_id);
        }
        println!("[CLAP Host] Timer unregistered: {:?}", timer_id);
        Ok(())
    }
}

impl HostPosixFdImpl for HikaruMainThread<'_> {
    fn register_fd(&self, fd: std::os::unix::io::RawFd, flags: FdFlags) -> Result<(), HostError> {
        println!("[CLAP Host] POSIX FD registered: fd={}, flags={:?}", fd, flags);
        Ok(())
    }

    fn modify_fd(&self, fd: std::os::unix::io::RawFd, flags: FdFlags) -> Result<(), HostError> {
        println!("[CLAP Host] POSIX FD modified: fd={}, flags={:?}", fd, flags);
        Ok(())
    }

    fn unregister_fd(&self, fd: std::os::unix::io::RawFd) -> Result<(), HostError> {
        println!("[CLAP Host] POSIX FD unregistered: fd={}", fd);
        Ok(())
    }
}

struct HikaruClapHost;

impl HostHandlers for HikaruClapHost {
    type Shared<'a> = HikaruShared;
    type MainThread<'a> = HikaruMainThread<'a>;
    type AudioProcessor<'a> = ();

    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &Self::Shared<'_>) {
        builder
            .register::<HostLog>()
            .register::<HostGui>()
            .register::<HostTimer>()
            .register::<HostPosixFd>();
    }
}

pub struct ClapInstance {
    name: String,
    plugin: clack_host::plugin::PluginInstance<HikaruClapHost>,
    gui: Option<PluginGui>,
    window_attached: bool,
    #[cfg(target_os = "linux")]
    _x11_conn: Option<X11Connection>,
}

unsafe impl Send for ClapInstance {}

impl ClapInstance {
    pub fn load(path: &Path) -> Result<Self, String> {
        println!("[CLAP] Cargando '{}'...", path.display());

        let entry = unsafe { PluginEntry::load(path) }
            .map_err(|e| format!("[CLAP] Fallo al cargar entry: {}", e))?;

        let plugin_factory = entry
            .get_plugin_factory()
            .ok_or("[CLAP] Entry no tiene plugin factory")?;

        let plugin_count = plugin_factory.plugin_count();
        if plugin_count == 0 {
            return Err("[CLAP] Factory sin plugins".into());
        }

        let desc = plugin_factory
            .plugin_descriptors()
            .next()
            .ok_or("[CLAP] Sin descriptor")?;

        let plugin_id = desc.id().ok_or("[CLAP] Descriptor sin id")?;
        let plugin_name = desc
            .name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| {
                path.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            });

        println!(
            "[CLAP] Plugin: '{}' (id={})",
            plugin_name,
            plugin_id.to_string_lossy()
        );

        let host_info = HostInfo::new("Hikaru", "Hikaru Corporation", "https://hikaru.dev", "0.1.0")
            .map_err(|e| format!("[CLAP] HostInfo: {}", e))?;

        let plugin = clack_host::plugin::PluginInstance::<HikaruClapHost>::new(
            |_| HikaruShared { log_to_stdout: true },
            |shared| HikaruMainThread {
                shared,
                timers: Mutex::new(Vec::new()),
            },
            &entry,
            plugin_id,
            &host_info,
        )
        .map_err(|e| format!("[CLAP] Instantiation failed: {:?}", e))?;

        println!("[CLAP] Plugin instanciado OK");

        let shared_handle = plugin.plugin_shared_handle();

        let gui = shared_handle.get_extension::<PluginGui>();

        if gui.is_some() {
            println!("[CLAP] GUI extension disponible para '{}'", plugin_name);
        } else {
            println!(
                "[CLAP] '{}' no tiene GUI extension",
                plugin_name
            );
        }

        Ok(Self {
            name: plugin_name,
            plugin,
            gui,
            window_attached: false,
            #[cfg(target_os = "linux")]
            _x11_conn: None,
        })
    }
}

impl PluginInstance for ClapInstance {
    fn process(&mut self, _buffer: &mut AudioBuffer) {}

    fn get_gui_size(&self) -> Option<(u32, u32)> {
        // En CLAP gui.get_size requiere un gui.create previo.
        // Devolvemos None aquí para consultar el tamaño tras la creación en show_gui_embedded.
        None
    }

    fn notify_gui_embedded(&mut self) -> Option<(u32, u32)> {
        // After gui.create + gui.set_parent + gui.show, query the actual native size.
        let gui = self.gui.as_ref()?;
        let mut plugin_handle = self.plugin.plugin_handle();
        gui.get_size(&mut plugin_handle)
            .map(|size| (size.width, size.height))
    }

    fn show_gui_embedded(&mut self, handle: RawWindowHandle) {
        let Some(gui) = &self.gui else {
            eprintln!("[CLAP] '{}' sin GUI extension", self.name);
            return;
        };

        let mut plugin_handle = self.plugin.plugin_handle();

        #[cfg(target_os = "linux")]
        let (window_id, x11_conn) = match crate::platform::linux::get_or_create_x11_window(handle, 1024, 700) {
            Ok((wid, conn)) => (wid, conn),
            Err(e) => {
                eprintln!("[CLAP] Fallo al crear ventana X11: {}", e);
                return;
            }
        };

        #[cfg(not(target_os = "linux"))]
        let window_id = match handle {
            RawWindowHandle::Xlib(h) => h.window as u32,
            RawWindowHandle::Xcb(h) => h.window.get().into(),
            _ => return,
        };

        // 1. gui.create siempre antes de cualquier otra llamada a la GUI
        let config = GuiConfiguration {
            api_type: GuiApiType::X11,
            is_floating: false,
        };

        if let Err(e) = gui.create(&mut plugin_handle, config) {
            eprintln!("[CLAP] gui.create() falló para '{}': {:?}", self.name, e);
            return;
        }

        // 2. Consultar y aplicar el tamaño nativo del plugin
        if let Some(native_size) = gui.get_size(&mut plugin_handle) {
            println!(
                "[CLAP] Ajustando GUI de '{}' a tamaño nativo: {}x{}",
                self.name, native_size.width, native_size.height
            );
            let _ = gui.set_size(&mut plugin_handle, native_size);
        }

        // 3. Vincular ventana padre
        let parent_window = Window::from_x11_handle(window_id as std::ffi::c_ulong);

        if let Err(e) = unsafe { gui.set_parent(&mut plugin_handle, parent_window) } {
            eprintln!("[CLAP] gui.set_parent() falló: {:?}", e);
            let _ = gui.destroy(&mut plugin_handle);
            return;
        }

        // 4. Mostrar la GUI
        if let Err(e) = gui.show(&mut plugin_handle) {
            eprintln!("[CLAP] gui.show() falló: {:?}", e);
            let _ = gui.destroy(&mut plugin_handle);
            return;
        }

        self.window_attached = true;

        #[cfg(target_os = "linux")]
        {
            self._x11_conn = x11_conn;
        }

        println!("[CLAP] GUI de '{}' abierta OK", self.name);
    }

    fn show_gui_floating(&mut self) -> Result<(), String> {
        let Some(gui) = &self.gui else {
            return Err(format!("[CLAP] '{}' sin GUI", self.name));
        };

        let mut plugin_handle = self.plugin.plugin_handle();

        let config = GuiConfiguration {
            api_type: GuiApiType::X11,
            is_floating: true,
        };

        gui.create(&mut plugin_handle, config)
            .map_err(|e| format!("[CLAP] gui.create(floating) failed: {:?}", e))?;

        gui.show(&mut plugin_handle)
            .map_err(|e| format!("[CLAP] gui.show(floating) failed: {:?}", e))?;

        self.window_attached = true;
        Ok(())
    }

    fn hide_gui(&mut self) {
        if self.window_attached {
            if let Some(gui) = &self.gui {
                let mut plugin_handle = self.plugin.plugin_handle();
                let _ = gui.hide(&mut plugin_handle);
                let _ = gui.destroy(&mut plugin_handle);
            }
            self.window_attached = false;
            #[cfg(target_os = "linux")]
            {
                self._x11_conn = None;
            }
        }
    }

    fn get_name(&self) -> &str {
        &self.name
    }

    fn resize_gui(&mut self, width: u32, height: u32) {
        if let Some(gui) = &self.gui {
            let mut plugin_handle = self.plugin.plugin_handle();
            if gui.can_resize(&plugin_handle) {
                let _ = gui.set_size(&mut plugin_handle, GuiSize { width, height });
            }
        }
    }
}

impl Drop for ClapInstance {
    fn drop(&mut self) {
        self.hide_gui();
    }
}