// Copyright (C) Hikaru Corporation - 2026
// GNU Affero General Public License v3
// VST3 / CLAP Plugins Settings

use egui::Context;
use hikaru_plugin_host::{ClapInstance, PluginInstance, Vst3Instance};
use raw_window_handle::RawWindowHandle;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PluginFormat {
    VST3,
    CLAP,
}

#[derive(Clone, Debug)]
pub struct DiscoveredPlugin {
    pub name: String,
    pub path: PathBuf,
    pub format: PluginFormat,
}

enum ScanMessage {
    ScanningFile(String),
    Found(DiscoveredPlugin),
    Finished(usize),
}

/// Notification from a plugin thread that its window was closed.
struct PluginClosedNotification {
    name: String,
}

pub struct PluginSettingsState {
    pub is_open: bool,
    pub custom_paths: Vec<String>,
    pub status_message: String,
    pub is_scanning: bool,
    pub discovered_plugins: Vec<DiscoveredPlugin>,

    tx: Sender<ScanMessage>,
    rx: Receiver<ScanMessage>,

    /// Map of plugin name → thread handle. When the thread finishes, the plugin window was closed.
    open_plugins: HashMap<String, std::thread::JoinHandle<()>>,
    /// Receiver for close notifications from plugin threads.
    close_rx: Receiver<PluginClosedNotification>,
    /// Sender to pass to plugin threads so they can notify when they close.
    close_tx: Sender<PluginClosedNotification>,
}

impl Default for PluginSettingsState {
    fn default() -> Self {
        let (tx, rx) = channel();
        let (close_tx, close_rx) = channel();
        Self {
            is_open: false,
            custom_paths: vec![
                format!("{}/.vst3", std::env::var("HOME").unwrap_or_default()),
                "/usr/lib/vst3".to_string(),
                format!("{}/.clap", std::env::var("HOME").unwrap_or_default()),
                "/usr/lib/clap".to_string(),
            ],
            status_message: "Listo para escanear.".to_string(),
            is_scanning: false,
            discovered_plugins: Vec::new(),
            tx,
            rx,
            open_plugins: HashMap::new(),
            close_rx,
            close_tx,
        }
    }
}

impl PluginSettingsState {
    pub fn start_scan(&mut self) {
        if self.is_scanning {
            return;
        }

        self.is_scanning = true;
        self.discovered_plugins.clear();
        self.status_message = "Iniciando escaneo de plugins...".to_string();

        let paths = self.custom_paths.clone();
        let tx = self.tx.clone();

        std::thread::spawn(move || {
            let mut total_found = 0;

            for base_path in paths {
                let dir_path = std::path::Path::new(&base_path);
                if !dir_path.exists() {
                    continue;
                }

                if let Ok(entries) = std::fs::read_dir(dir_path) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        let file_name = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();

                        let _ = tx.send(ScanMessage::ScanningFile(file_name.clone()));

                        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
                        let is_vst3 = ext == "vst3" || (path.is_dir() && file_name.ends_with(".vst3"));
                        let is_clap = ext == "clap";
                        let is_so = ext == "so";

                        if is_vst3 || is_clap || is_so {
                            let format = if is_clap {
                                PluginFormat::CLAP
                            } else {
                                PluginFormat::VST3
                            };

                            let name = path
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();

                            total_found += 1;
                            let _ = tx.send(ScanMessage::Found(DiscoveredPlugin {
                                name,
                                path,
                                format,
                            }));
                        }
                    }
                }
            }

            let _ = tx.send(ScanMessage::Finished(total_found));
        });
    }

    pub fn poll_updates(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                ScanMessage::ScanningFile(file) => {
                    self.status_message = format!("Escaneando: {}", file);
                }
                ScanMessage::Found(plugin) => {
                    self.discovered_plugins.push(plugin);
                }
                ScanMessage::Finished(count) => {
                    self.is_scanning = false;
                    self.status_message = format!("Escaneo finalizado. {} plugins listos.", count);
                }
            }
        }

        // Collect closed plugin names first to avoid borrow issues
        let mut closed_names = Vec::new();
        while let Ok(notif) = self.close_rx.try_recv() {
            closed_names.push(notif.name);
        }
        for name in closed_names {
            self.open_plugins.remove(&name);
            println!("[PluginSettings] Plugin '{}' window closed", name);
        }
    }

    fn open_plugin(&mut self, plugin: &DiscoveredPlugin) {
        if self.open_plugins.contains_key(&plugin.name) {
            println!(
                "[PluginSettings] '{}' ya tiene una ventana abierta",
                plugin.name
            );
            return;
        }

        let plugin_path = plugin.path.clone();
        let plugin_name = plugin.name.clone();
        let format = plugin.format;
        let close_tx = self.close_tx.clone();

        let handle = std::thread::spawn(move || {
            run_plugin_window(plugin_path, plugin_name, format, close_tx);
        });

        self.open_plugins.insert(plugin.name.clone(), handle);
    }

    #[allow(dead_code)]
    fn close_plugin(&mut self, name: &str) {
        if let Some(handle) = self.open_plugins.remove(name) {
            // The thread will finish when the winit window is closed.
            // We can't forcefully kill a thread in Rust, but the window close
            // event will cause the event loop to exit.
            println!("[PluginSettings] Closing plugin '{}' (thread will exit)", name);
            // Don't block on join - the thread will finish on its own
            let _ = handle;
        }
    }
}

/// Run a plugin GUI in its own raw X11 window on a dedicated thread.
/// This blocks until the window is closed.
fn run_plugin_window(
    plugin_path: PathBuf,
    plugin_name: String,
    format: PluginFormat,
    close_tx: Sender<PluginClosedNotification>,
) {
    use raw_window_handle::{RawWindowHandle, XlibWindowHandle};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::*;
    use x11rb::protocol::Event;

    // Connect to X11 display (XWayland if on Wayland)
    let (conn, screen_num) = match x11rb::rust_connection::RustConnection::connect(None) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "[PluginWindow] Failed to connect to X11 for '{}': {:?}",
                plugin_name, e
            );
            let _ = close_tx.send(PluginClosedNotification {
                name: plugin_name,
            });
            return;
        }
    };

    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    // Create the window
    let window = match conn.generate_id() {
        Ok(id) => id,
        Err(e) => {
            eprintln!(
                "[PluginWindow] Failed to generate window id for '{}': {:?}",
                plugin_name, e
            );
            let _ = close_tx.send(PluginClosedNotification {
                name: plugin_name,
            });
            return;
        }
    };

    let width: u16 = 1024;
    let height: u16 = 700;

    let window_aux = CreateWindowAux::new()
        .event_mask(
            EventMask::EXPOSURE
                | EventMask::KEY_PRESS
                | EventMask::STRUCTURE_NOTIFY
                | EventMask::RESIZE_REDIRECT,
        );

    if let Err(e) = conn.create_window(
        0,
        window,
        root,
        0,
        0,
        width,
        height,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &window_aux,
    ) {
        eprintln!(
            "[PluginWindow] Failed to create window for '{}': {:?}",
            plugin_name, e
        );
        let _ = close_tx.send(PluginClosedNotification {
            name: plugin_name,
        });
        return;
    }

    // Set window title via WM_NAME
    if let Ok(cookie) = conn.intern_atom(true, b"WM_NAME") {
        if let Ok(reply) = cookie.reply() {
            let title = format!("{} - Hikaru", plugin_name);
            let _ = conn.change_property(
                PropMode::REPLACE,
                window,
                reply.atom,
                u32::from(AtomEnum::STRING),
                8,
                title.len() as u32,
                title.as_bytes(),
            );
        }
    }

    // Set WM_DELETE_WINDOW protocol
    if let Ok(proto_cookie) = conn.intern_atom(true, b"WM_PROTOCOLS") {
        if let Ok(delete_cookie) = conn.intern_atom(true, b"WM_DELETE_WINDOW") {
            if let (Ok(proto_reply), Ok(delete_reply)) = (proto_cookie.reply(), delete_cookie.reply())
            {
                let atom_bytes = delete_reply.atom.to_ne_bytes();
                let _ = conn.change_property(
                    PropMode::REPLACE,
                    window,
                    proto_reply.atom,
                    u32::from(AtomEnum::ATOM),
                    32,
                    1,
                    &atom_bytes,
                );
            }
        }
    }

    // Map (show) the window
    if let Err(e) = conn.map_window(window) {
        eprintln!(
            "[PluginWindow] Failed to map window for '{}': {:?}",
            plugin_name, e
        );
        let _ = close_tx.send(PluginClosedNotification {
            name: plugin_name,
        });
        return;
    }

    if let Err(e) = conn.flush() {
        eprintln!(
            "[PluginWindow] Failed to flush for '{}': {:?}",
            plugin_name, e
        );
    }

    // Give X11 time to fully map the window and process decorations
    std::thread::sleep(std::time::Duration::from_millis(250));

    // Flush any pending X11 requests
    let _ = conn.flush();

    println!(
        "[PluginWindow] X11 window {} created for '{}' ({}x{})",
        window, plugin_name, width, height
    );

    // Create RawWindowHandle for the plugin
    let xlib_handle = XlibWindowHandle::new(window as u64);
    let raw_handle = RawWindowHandle::Xlib(xlib_handle);

    // 1. Cargar la instancia del plugin
    let instance: Option<Box<dyn PluginInstance>> = match format {
        PluginFormat::CLAP => match ClapInstance::load(&plugin_path) {
            Ok(inst) => {
                println!("[PluginWindow] CLAP cargado para '{}'", plugin_name);
                Some(Box::new(inst))
            }
            Err(e) => {
                eprintln!("[PluginWindow] Error al cargar CLAP: {}", e);
                None
            }
        },
        PluginFormat::VST3 => match Vst3Instance::load(&plugin_path) {
            Ok(inst) => {
                println!("[PluginWindow] VST3 cargado para '{}'", plugin_name);
                Some(Box::new(inst))
            }
            Err(e) => {
                eprintln!("[PluginWindow] Error al cargar VST3: {}", e);
                None
            }
        },
    };

    let mut inst = match instance {
        Some(i) => i,
        None => {
            eprintln!("[PluginWindow] Cerrando ventana por fallo de carga en '{}'", plugin_name);
            let _ = conn.destroy_window(window);
            let _ = conn.flush();
            let _ = close_tx.send(PluginClosedNotification { name: plugin_name });
            return;
        }
    };

    // 2. Si el plugin reporta un tamaño preferido, ajustar la ventana X11 ANTES de embeber
    if let Some((pref_w, pref_h)) = inst.get_gui_size() {
        if pref_w > 0 && pref_h > 0 {
            let values = ConfigureWindowAux::new()
                .width(pref_w)
                .height(pref_h);
            let _ = conn.configure_window(window, &values);
            let _ = conn.flush();
            println!("[PluginWindow] Ventana X11 redimensionada a {}x{} según la preferencia del plugin", pref_w, pref_h);
        }
    }

    // 3. Vincular e iniciar el renderizado FFI
    inst.show_gui_embedded(raw_handle);

    // 4. Post-embed: query the actual preferred size and resize the X11 window to match.
    //    Some plugins (like Vital) only report the correct size after the GUI is attached.
    if let Some((actual_w, actual_h)) = inst.notify_gui_embedded() {
        if actual_w > 0 && actual_h > 0 && (actual_w != width as u32 || actual_h != height as u32) {
            let values = ConfigureWindowAux::new()
                .width(actual_w)
                .height(actual_h);
            let _ = conn.configure_window(window, &values);
            let _ = conn.flush();
            println!(
                "[PluginWindow] Ventana X11 redimensionada post-embebido a {}x{} (era {}x{})",
                actual_w, actual_h, width, height
            );
        }
    }

    // Dar tiempo a la superficie X11 para inicializar los buffers de pintado
    std::thread::sleep(std::time::Duration::from_millis(100));
    let _ = conn.flush();

    // 4. Run X11 event loop — keep the plugin instance alive until the window is closed.
    //    Without this loop the function returns immediately, dropping `inst` and destroying
    //    the plugin GUI before the user can interact with it.
    let wm_delete = {
        let cookie = conn.intern_atom(true, b"WM_DELETE_WINDOW").unwrap();
        cookie.reply().unwrap().atom
    };

    loop {
        match conn.wait_for_event() {
            Ok(Event::ClientMessage(msg)) => {
                if msg.type_ == wm_delete {
                    println!(
                        "[PluginWindow] WM_DELETE_WINDOW received for '{}' — closing",
                        plugin_name
                    );
                    break;
                }
            }
            Ok(Event::DestroyNotify(ev)) => {
                if ev.window == window {
                    println!(
                        "[PluginWindow] DestroyNotify for '{}' — closing",
                        plugin_name
                    );
                    break;
                }
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!(
                    "[PluginWindow] X11 error for '{}': {:?}",
                    plugin_name, e
                );
                break;
            }
        }
    }

    // Cleanup: drop the plugin instance (calls hide_gui / close_editor / dlclose)
    drop(inst);
    let _ = conn.destroy_window(window);
    let _ = conn.flush();

    // Notify that this plugin window is closed
    let _ = close_tx.send(PluginClosedNotification {
        name: plugin_name,
    });
}

pub fn render(
    ctx: &Context,
    state: &mut PluginSettingsState,
    _parent_handle: Option<RawWindowHandle>,
) {
    if !state.is_open {
        return;
    }

    state.poll_updates();

    egui::CentralPanel::default().show(ctx, |ui| {
        ui.heading("Rutas de Busqueda");
        ui.add_space(6.0);

        let mut to_remove = None;
        for (idx, path) in state.custom_paths.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label("-");
                ui.monospace(path);
                if ui.button("X").clicked() {
                    to_remove = Some(idx);
                }
            });
        }

        if let Some(idx) = to_remove {
            state.custom_paths.remove(idx);
        }

        ui.add_space(6.0);
        if ui.button("+ Agregar Ruta...").clicked() {
            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                state.custom_paths.push(folder.display().to_string());
            }
        }

        ui.separator();

        ui.horizontal(|ui| {
            if ui
                .add_enabled(!state.is_scanning, egui::Button::new("Rescan Plugins"))
                .clicked()
            {
                state.start_scan();
            }

            if state.is_scanning {
                ui.spinner();
            }

            ui.label(&state.status_message);
        });

        ui.separator();

        // Show open plugins
        if !state.open_plugins.is_empty() {
            ui.heading("Plugins Abiertos");
            ui.add_space(4.0);
            for (name, _) in &state.open_plugins {
                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::GREEN, format!("* {}", name));
                    if ui.button("Cerrar").clicked() {
                        // Mark for removal (can't modify during iteration)
                    }
                });
            }
            ui.separator();
        }

        ui.heading("Plugins Encontrados");
        ui.add_space(6.0);

        // Collect close requests to avoid borrow issues
        let mut close_request = None;

        egui::ScrollArea::vertical().max_height(250.0).show(ui, |ui| {
            if state.discovered_plugins.is_empty() {
                ui.label("No se han detectado plugins VST3 o CLAP.");
            } else {
                for plugin in &state.discovered_plugins {
                    ui.horizontal(|ui| {
                        let (badge_text, badge_color) = match plugin.format {
                            PluginFormat::CLAP => ("[CLAP]", egui::Color32::from_rgb(180, 100, 255)),
                            PluginFormat::VST3 => ("[VST3]", egui::Color32::from_rgb(100, 180, 255)),
                        };

                        ui.colored_label(badge_color, badge_text);
                        ui.label(&plugin.name);

                        let is_open = state.open_plugins.contains_key(&plugin.name);
                        let button_text = if is_open { "Abierto" } else { "Abrir Plugin" };

                        if ui
                            .add_enabled(!is_open, egui::Button::new(button_text))
                            .clicked()
                        {
                            close_request = Some(plugin.clone());
                        }
                    });
                }
            }
        });

        // Handle open request outside the scroll area borrow
        if let Some(plugin) = close_request {
            state.open_plugin(&plugin);
        }
    });
}
