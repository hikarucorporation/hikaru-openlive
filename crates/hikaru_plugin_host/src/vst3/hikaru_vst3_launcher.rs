// Copyright (C) Hikaru Corporation - 2026
// Hikaru VST3 Launcher
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/src/vst3/hikaru_vst3_launcher.rs

use crate::PluginInstance;
use hikaru_core::AudioBuffer;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::path::{Path, PathBuf};
use vst3_host::simple;
use vst3_host::plugin::WindowHandle;

#[cfg(target_os = "linux")]
use crate::platform::linux::X11Connection;

pub struct Vst3Instance {
    plugin: vst3_host::plugin::Plugin,
    name: String,
    window_attached: bool,
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
        println!("[VST3] Cargando '{}'...", path.display());

        let resolved_path = Self::resolve_vst3_path(path);
        let plugin = simple::load_plugin(&resolved_path)
            .map_err(|e| format!("[VST3] Fallo al cargar: {}", e))?;

        let info = plugin.info();
        let name = info.name.clone();

        println!(
            "[VST3] Plugin: '{}' vendor='{}' gui={}",
            name, info.vendor, info.has_gui
        );

        if !info.has_gui {
            eprintln!("[VST3] '{}' no tiene GUI", name);
        }

        Ok(Self {
            plugin,
            name,
            window_attached: false,
            #[cfg(target_os = "linux")]
            _x11_conn: None,
        })
    }

    fn open_floating_window(&mut self) -> Result<(), String> {
        if !self.plugin.has_editor() {
            return Err(format!("[VST3] '{}' sin editor", self.name));
        }

        println!("[VST3] Creando ventana flotante para '{}'", self.name);

        let event_loop = winit::event_loop::EventLoop::new()
            .map_err(|e| format!("[VST3] EventLoop: {}", e))?;
        let window = winit::window::WindowBuilder::new()
            .with_title(format!("VST3: {}", self.name))
            .with_inner_size(winit::dpi::LogicalSize::new(900.0, 600.0))
            .build(&event_loop)
            .map_err(|e| format!("[VST3] WindowBuilder: {}", e))?;

        let raw_handle = window
            .window_handle()
            .map(|h| h.as_raw())
            .map_err(|e| format!("[VST3] window_handle: {}", e))?;

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

        self.plugin
            .open_editor(wh)
            .map_err(|e| format!("[VST3] open_editor: {}", e))?;

        self.window_attached = true;

        event_loop
            .run(move |event, elwt| {
                if let winit::event::Event::WindowEvent {
                    window_id: _,
                    event: winit::event::WindowEvent::CloseRequested,
                } = event
                {
                    elwt.exit();
                }
                if let winit::event::Event::AboutToWait = event {
                    window.request_redraw();
                }
            })
            .map_err(|e| format!("[VST3] EventLoop: {}", e))?;

        Ok(())
    }
}

impl PluginInstance for Vst3Instance {
    fn process(&mut self, _buffer: &mut AudioBuffer) {}

    fn show_gui_embedded(&mut self, handle: RawWindowHandle) {
        if !self.plugin.has_editor() {
            eprintln!(
                "[VST3] '{}' no tiene editor — no se puede embebir",
                self.name
            );
            return;
        }

        #[cfg(target_os = "linux")]
        let embedded_result = {
            match crate::platform::linux::get_or_create_x11_window(handle, 800, 600) {
                Ok((window_id, x11_conn)) => {
                    println!(
                        "[VST3] Embebiendo GUI de '{}' en X11 window {}",
                        self.name, window_id
                    );
                    let wh = WindowHandle::from_x11(window_id);
                    let result = self.plugin.open_editor(wh);
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
                    eprintln!("[VST3] Handle no soportado: {:?}", handle);
                    return;
                }
            };
            let wh = WindowHandle::from_x11(window_id);
            self.plugin.open_editor(wh)
        };

        match embedded_result {
            Ok(()) => {
                self.window_attached = true;
                println!("[VST3] GUI '{}' incrustada OK", self.name);
            }
            Err(e) => {
                eprintln!(
                    "[VST3] Embebido falló ({}), intentando flotante...",
                    e
                );
                if let Err(e2) = self.open_floating_window() {
                    eprintln!("[VST3] Flotante también falló: {}", e2);
                }
            }
        }
    }

    fn show_gui_floating(&mut self) -> Result<(), String> {
        self.open_floating_window()
    }

    fn hide_gui(&mut self) {
        if self.window_attached {
            if let Err(e) = self.plugin.close_editor() {
                eprintln!("[VST3] close_editor() failed: {}", e);
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

    fn get_gui_size(&self) -> Option<(u32, u32)> {
        self.plugin
            .get_editor_size()
            .ok()
            .map(|(w, h)| (w as u32, h as u32))
    }

    fn resize_gui(&mut self, _width: u32, _height: u32) {
        if let Some((w, h)) = self.plugin.take_editor_resize_request() {
            eprintln!(
                "[VST3] Plugin solicita resize a {}x{} — no implementado aún",
                w, h
            );
        }
    }
}

impl Drop for Vst3Instance {
    fn drop(&mut self) {
        self.hide_gui();
    }
}
