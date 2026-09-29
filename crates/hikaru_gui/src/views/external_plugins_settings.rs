use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

use gpui_kit::component::button::Button;
use gpui_kit::component::label::Label;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, HikaruApp};

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

    open_plugins: HashMap<String, std::thread::JoinHandle<()>>,
    close_rx: Receiver<PluginClosedNotification>,
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

        let mut closed_names = Vec::new();
        while let Ok(notif) = self.close_rx.try_recv() {
            closed_names.push(notif.name);
        }
        for name in closed_names {
            self.open_plugins.remove(&name);
            println!("[PluginSettings] Plugin '{}' window closed", name);
        }
    }


    pub fn open_plugin_names(&self) -> Vec<String> {
        self.open_plugins.keys().cloned().collect()
    }

    pub fn open_plugin(&mut self, plugin: &DiscoveredPlugin) {
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

    pub fn close_plugin(&mut self, name: &str) {
        if let Some(handle) = self.open_plugins.remove(name) {
            println!("[PluginSettings] Closing plugin '{}' (thread will exit)", name);
            let _ = handle;
        }
    }

}

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
        eprintln!("[PluginWindow] Failed to flush for '{}': {:?}", plugin_name, e);
    }

    std::thread::sleep(std::time::Duration::from_millis(250));
    let _ = conn.flush();

    println!(
        "[PluginWindow] X11 window {} created for '{}' ({}x{})",
        window, plugin_name, width, height
    );

    let xlib_handle = XlibWindowHandle::new(window as u64);
    let raw_handle = RawWindowHandle::Xlib(xlib_handle);

    let instance: Option<Box<dyn hikaru_plugin_host::PluginInstance>> = match format {
        PluginFormat::CLAP => match hikaru_plugin_host::ClapInstance::load(&plugin_path) {
            Ok(inst) => {
                println!("[PluginWindow] CLAP cargado para '{}'", plugin_name);
                Some(Box::new(inst))
            }
            Err(e) => {
                eprintln!("[PluginWindow] Error al cargar CLAP: {}", e);
                None
            }
        },
        PluginFormat::VST3 => match hikaru_plugin_host::Vst3Instance::load(&plugin_path) {
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

    inst.show_gui_embedded(raw_handle);

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

    std::thread::sleep(std::time::Duration::from_millis(100));
    let _ = conn.flush();

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

    drop(inst);
    let _ = conn.destroy_window(window);
    let _ = conn.flush();

    let _ = close_tx.send(PluginClosedNotification {
        name: plugin_name,
    });
}

pub fn render(cx: &mut Context<HikaruApp>) -> AnyElement {
    let app = state(cx).read(cx);
    let is_open = app.plugin_settings_state.is_open;
    let paths = app.plugin_settings_state.custom_paths.clone();
    let status = app.plugin_settings_state.status_message.clone();
    let is_scanning = app.plugin_settings_state.is_scanning;
    let plugins = app.plugin_settings_state.discovered_plugins.clone();
    let open_names: Vec<String> = app
        .plugin_settings_state
        .open_plugin_names();
    drop(app);

    if !is_open {
        return div().into_any_element();
    }

    let mut path_rows: Vec<AnyElement> = Vec::new();
    for (idx, p) in paths.iter().enumerate() {
        path_rows.push(
            h_flex()
                .gap(px(4.0))
                .child(Label::new("-").text_xs())
                .child(Label::new(p.clone()).text_xs())
                .child(
                    Button::new(format!("plugin_path_remove_{}", idx)).rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("X")
                        .compact()
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            cx.update_entity(&st, |state, cx| {
                                state.plugin_settings_state.custom_paths.remove(idx);
                                cx.notify();
                            });
                        }),
                )
                .into_any_element(),
        );
    }

    let mut plugin_rows: Vec<AnyElement> = Vec::new();
    for plugin in plugins.iter() {
        let is_open = open_names.contains(&plugin.name);
        let badge = match plugin.format {
            PluginFormat::CLAP => "[CLAP]",
            PluginFormat::VST3 => "[VST3]",
        };
        let badge_col = match plugin.format {
            PluginFormat::CLAP => rgb(0xB464FF),
            PluginFormat::VST3 => rgb(0x64B4FF),
        };
        let name = plugin.name.clone();
        let plugin = plugin.clone();
        plugin_rows.push(
            h_flex()
                .gap(px(6.0))
                .items_center()
                .child(Label::new(badge).text_xs().text_color(badge_col))
                .child(Label::new(name.clone()).text_xs())
                .child(div().flex_1())
                .child(
                    Button::new(format!("plugin_open_{}", name)).rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label(if is_open { "Abierto" } else { "Abrir Plugin" })
                        .compact()
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            cx.update_entity(&st, |state, cx| {
                                state.plugin_settings_state.open_plugin(&plugin);
                                cx.notify();
                            });
                        }),
                )
                .into_any_element(),
        );
    }

    v_flex()
        .id("plugin_settings")
        .absolute()
        .left(px(20.0))
        .top(px(80.0))
        .w(px(520.0))
        .h(px(380.0))
        .bg(rgb(0x181A20))
        .border_1()
        .border_color(rgb(0x2A2D37))
        .rounded(px(6.0))
        .p(px(12.0))
        .gap(px(8.0))
        .child(Label::new("Rutas de Busqueda").text_sm().font_weight(FontWeight::BOLD))
        .children(path_rows)
        .child(
            Button::new("plugin_add_path").rounded(gpui_kit::component::button::ButtonRounded::None)
                .label("+ Agregar Ruta...")
                .compact()
                .on_click(move |_, _, cx| {
                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                        let st = state(cx);
                        cx.update_entity(&st, |state, cx| {
                            state
                                .plugin_settings_state
                                .custom_paths
                                .push(folder.display().to_string());
                            cx.notify();
                        });
                    }
                }),
        )
        .child(
            h_flex()
                .gap(px(6.0))
                .items_center()
                .child(
                    Button::new("plugin_rescan").rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("Rescan Plugins")
                        .compact()
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            cx.update_entity(&st, |state, cx| {
                                state.plugin_settings_state.start_scan();
                                cx.notify();
                            });
                        }),
                )
                .when(is_scanning, |this| {
                    this.child(Label::new("Escaneando...").text_xs())
                })
                .when(!is_scanning, |this| {
                    this.child(Label::new(status.clone()).text_xs())
                }),
        )
        .child(
            v_flex()
                .gap(px(4.0))
                .child(Label::new("Plugins Encontrados").text_sm().font_weight(FontWeight::BOLD))
                .children(plugin_rows),
        )
        .child(
            Button::new("plugin_close").rounded(gpui_kit::component::button::ButtonRounded::None)
                .label("Cerrar")
                .compact()
                .on_click(move |_, _, cx| {
                    let st = state(cx);
                    cx.update_entity(&st, |state, cx| {
                        state.plugin_settings_state.is_open = false;
                        cx.notify();
                    });
                }),
        )
        .into_any_element()
}
