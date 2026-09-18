// Copyright (C) Hikaru Corporation - 2026
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/src/lib.rs

pub mod vst3;
pub mod clap;
pub mod platform;

use hikaru_core::AudioBuffer;
pub use raw_window_handle::RawWindowHandle;
use std::path::Path;

pub use vst3::Vst3Instance;
pub use clap::ClapInstance;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginFormat {
    VST3,
    CLAP,
}

pub trait PluginInstance: Send {
    fn process(&mut self, buffer: &mut AudioBuffer);
    fn show_gui_embedded(&mut self, window_handle: RawWindowHandle);
    fn show_gui_floating(&mut self) -> Result<(), String>;
    fn hide_gui(&mut self);
    fn get_name(&self) -> &str;

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
    pub fn scan_plugins() {
        println!("[hikaru_plugin_host] Escaneando directorios...");
    }
}
