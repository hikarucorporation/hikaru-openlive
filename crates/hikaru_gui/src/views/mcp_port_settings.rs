// crates/hikaru_gui/src/views/mcp_port_settings.rs

//! El panel `Hikaru MCP Port Settings...`.
//!
//! # Qué muestra
//!
//! Tres cosas, en este orden:
//!
//! 1. **`Advanced Settings...`**: el transporte del slot elegido (socket Unix,
//!    stdio o server flags), su ruta y cuántos clientes admite.
//! 2. **`Available Ports`**: los slots que existen. MCP Port 1 a MCP Port 10
//!    de fábrica, y los que el usuario agregue después.
//! 3. **`Add MCP Port...`**: el botón que crea slots adicionales para cuando
//!    diez conexiones simultáneas no alcanzan.
//!
//! # Qué NO hace todavía
//!
//! El panel edita la configuración; no levanta el servidor. El transporte real
//! (el `accept()` en un hilo aparte del render) es la fase siguiente del
//! plan, y hasta entonces la UI dice la verdad: el servidor está detenido.

use gpui_kit::component::button::Button;
use gpui_kit::component::label::Label;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, Styled as _};
use gpui_kit::*;

use hikaru_mcp_port::{McpPortConfig, McpServerConfig, McpTransport, PortSlotId};

use crate::app::{state, update_state, HikaruApp};
use crate::views::modal::{self, ModalId, ModalRoot};

/// Posición base de la ventana (px, desde la esquina superior izquierda de la
/// ventana principal). El arrastre la mueve desde acá.
const BASE_LEFT: f32 = 20.0;
const BASE_TOP: f32 = 80.0;

/// El estado del panel.
///
/// Vive en `AppState` (y no en la vista) por el mismo motivo que el resto de
/// los paneles: el estado tiene que sobrevivir a los `notify` del render, y
/// la configuración de los puertos no puede depender de un frame en
/// particular.
pub struct McpPortSettingsState {
    /// ¿El panel está abierto?
    pub is_open: bool,
    /// La configuración que se está editando.
    pub config: McpServerConfig,
    /// El slot que se está editando en `Advanced Settings...`.
    ///
    /// `None` = todavía no se eligió ninguno. El panel muestra el primero
    /// disponible sin cambiar el valor, para que la sección no aparezca
    /// vacía al abrirse.
    pub selected_port: Option<PortSlotId>,
    /// ¿La sección de transporte está desplegada?
    pub show_advanced: bool,
    /// La última noteworthy, para el pie del panel.
    pub status_message: String,
}

impl Default for McpPortSettingsState {
    fn default() -> Self {
        Self {
            is_open: false,
            config: McpServerConfig::default(),
            selected_port: None,
            show_advanced: false,
            status_message: "Servidor MCP detenido.".to_string(),
        }
    }
}

impl McpPortSettingsState {
    /// Abre el panel.
    pub fn open(&mut self) {
        self.is_open = true;
    }

    /// Cierra el panel.
    pub fn close(&mut self) {
        self.is_open = false;
    }

    /// El slot que se está editando.
    ///
    /// Si el usuario todavía no eligió ninguno, es el primero de la lista:
    /// la sección de transporte siempre muestra algo.
    pub fn selected(&self) -> Option<&McpPortConfig> {
        match self.selected_port {
            Some(id) => self.config.slot(id),
            None => self.config.ports.first(),
        }
    }

    /// Elige el slot que se edita en `Advanced Settings...`.
    pub fn select(&mut self, id: PortSlotId) {
        self.selected_port = Some(id);
    }

    /// Crea un slot más y lo deja seleccionado.
    ///
    /// Devuelve el id del slot nuevo, para que la UI lo pueda anunciar.
    pub fn add_port(&mut self) -> PortSlotId {
        let id = self.config.add_port();
        self.selected_port = Some(id);
        self.status_message = format!("MCP Port {id} creado.");
        id
    }

    /// Borra un slot.
    ///
    /// Si el que se estaba editando era ese, la selección pasa al primero que
    /// quede: dejar la sección de transporte apuntando a un slot inexistente
    /// es la forma rápida de que la UI muestre datos de otro puerto.
    pub fn remove_port(&mut self, id: PortSlotId) {
        if self.config.remove_port(id) {
            if self.selected_port == Some(id) {
                self.selected_port = self.config.ports.first().map(|p| p.id);
            }
            self.status_message = format!("MCP Port {id} eliminado.");
        }
    }

    /// Cambia el transporte de un slot.
    ///
    /// El ciclo es Unix Socket → Stdio → Server Flags → Unix Socket. Es el
    /// mismo idioma que el resto de los selectores de la app (el backend de
    /// audio, el dispositivo de salida): un botón que muestra el valor actual
    /// y lo cambia al click.
    pub fn cycle_transport(&mut self, id: PortSlotId) {
        // Se copian ANTES de tomar el borrowing mutable del slot: el socket
        // vuelve a su lugar en el runtime dir y los flags son los del
        // servidor, no los del puerto.
        let socket = self.config.socket_path_for(id);
        let flags = self.config.server_flags.clone();

        let Some(port) = self.config.slot_mut(id) else {
            return;
        };
        port.transport = match &port.transport {
            McpTransport::UnixSocket { .. } => McpTransport::Stdio,
            McpTransport::Stdio => McpTransport::ServerFlags(flags),
            McpTransport::ServerFlags(_) => McpTransport::UnixSocket { path: socket },
        };
    }

    /// Activa o desactiva un slot.
    pub fn toggle_port(&mut self, id: PortSlotId) {
        if let Some(port) = self.config.slot_mut(id) {
            port.enabled = !port.enabled;
            let enabled = port.enabled;
            self.status_message = if enabled {
                format!("MCP Port {id} habilitado.")
            } else {
                format!("MCP Port {id} deshabilitado.")
            };
        }
    }
}

/// Dibuja el panel.
///
/// Devuelve un div vacío cuando está cerrado: el panel se mounts siempre en
/// el árbol (lo hace `app.rs`), yitempty es lo que evita que se lleve un
/// `notify` de más.
pub fn render(cx: &mut Context<HikaruApp>) -> AnyElement {
    let (is_open, config, selected_port, show_advanced, status, offset, dragging) = {
        let app = state(cx).read(cx);
        let drag = app.modal_drags.get(ModalId::McpPortSettings);
        (
            app.mcp_port_settings_state.is_open,
            app.mcp_port_settings_state.config.clone(),
            app.mcp_port_settings_state.selected_port,
            app.mcp_port_settings_state.show_advanced,
            app.mcp_port_settings_state.status_message.clone(),
            drag.offset(),
            drag.is_dragging(),
        )
    };

    if !is_open {
        return div().into_any_element();
    }

    // El slot que se edita: el elegido, o el primero si todavía no se eligió
    // ninguno. Se calcula acá (y no dentro del estado) para que el panel
    // muestre siempre algo aunque el estado apunte a un slot que ya no existe.
    let active = match selected_port {
        Some(id) => config.slot(id).cloned(),
        None => config.ports.first().cloned(),
    };

    // ---------------------------------------------------------------------
    // SECCIÓN 1: ADVANCED SETTINGS (transporte)
    // ---------------------------------------------------------------------
    let mut advanced_rows: Vec<AnyElement> = Vec::new();

    if let Some(port) = &active {
        let transport_label = port.transport.label().to_string();
        let port_id = port.id;

        // El transporte: un botón que cicla entre los tres modos.
        advanced_rows.push(
            h_flex()
                .w_full()
                .gap(px(6.0))
                .items_center()
                .child(
                    Label::new("Transporte:")
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0x8A90A0))
                        .w(px(96.0)),
                )
                .child(
                    Button::new("mcp_transport_cycle")
                        .rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label(transport_label)
                        .compact()
                        .flex_shrink_0()
                        .on_click(move |_, _, cx| {
                            update_state(cx, |state| {
                                state.mcp_port_settings_state.cycle_transport(port_id);
                            });
                        }),
                )
                .into_any_element(),
        );

        // El detalle del transporte elegido: la ruta del socket, o la línea de
        // comandos del servidor externo. No se muestra lo que no aplica.
        match &port.transport {
            McpTransport::UnixSocket { path } => {
                advanced_rows.push(
                    h_flex()
                        .w_full()
                        .gap(px(6.0))
                        .items_center()
                        .child(
                            Label::new("Socket:")
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(0x8A90A0))
                                .w(px(96.0)),
                        )
                        .child(
                            Label::new(path.display().to_string())
                                .text_xs()
                                .truncate()
                                .flex_1()
                                .text_color(rgb(0xB8BCC8)),
                        )
                        .into_any_element(),
                );
            }
            McpTransport::Stdio => {
                advanced_rows.push(
                    h_flex()
                        .w_full()
                        .gap(px(6.0))
                        .items_center()
                        .child(
                            Label::new("Canal:")
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(0x8A90A0))
                                .w(px(96.0)),
                        )
                        .child(
                            Label::new("stdin / stdout (JSON-RPC 2.0)")
                                .text_xs()
                                .truncate()
                                .flex_1()
                                .text_color(rgb(0xB8BCC8)),
                        )
                        .into_any_element(),
                );
            }
            McpTransport::ServerFlags(flags) => {
                advanced_rows.push(
                    h_flex()
                        .w_full()
                        .gap(px(6.0))
                        .items_center()
                        .child(
                            Label::new("Comando:")
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(0x8A90A0))
                                .w(px(96.0)),
                        )
                        .child(
                            Label::new(flags.command_line())
                                .text_xs()
                                .truncate()
                                .flex_1()
                                .text_color(rgb(0xB8BCC8)),
                        )
                        .into_any_element(),
                );
            }
        }

        // El interruptor del slot.
        let enabled = port.enabled;
        advanced_rows.push(
            h_flex()
                .w_full()
                .gap(px(6.0))
                .items_center()
                .child(
                    Label::new("Estado:")
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0x8A90A0))
                        .w(px(96.0)),
                )
                .child(
                    Button::new("mcp_port_toggle_selected")
                        .rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label(if enabled { "● Habilitado" } else { "○ Deshabilitado" })
                        .compact()
                        .flex_shrink_0()
                        .text_color(if enabled {
                            rgb(0x64E08A)
                        } else {
                            rgb(0x8A90A0)
                        })
                        .on_click(move |_, _, cx| {
                            update_state(cx, |state| {
                                state.mcp_port_settings_state.toggle_port(port_id);
                            });
                        }),
                )
                .into_any_element(),
        );
    }

    // ---------------------------------------------------------------------
    // SECCIÓN 2: AVAILABLE PORTS
    // ---------------------------------------------------------------------
    let mut port_rows: Vec<AnyElement> = Vec::new();
    for port in &config.ports {
        let id = port.id;
        let is_selected = selected_port == Some(id);
        let enabled = port.enabled;
        let label = port.label.clone();
        let transport = port.transport.label().to_string();

        port_rows.push(
            h_flex()
                .w_full()
                .gap(px(8.0))
                .items_center()
                .py(px(3.0))
                .px(px(6.0))
                .bg(rgb(0x20232C))
                .rounded(px(4.0))
                .child(
                    Button::new(format!("mcp_port_select_{}", id))
                        .rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label(label)
                        .compact()
                        .flex_shrink_0()
                        .w(px(110.0))
                        // El slot editado se pinta de otro color: con diez
                        // filas, el usuario tiene que ver de un vistazo sobre
                        // cuál está editando.
                        .text_color(if is_selected {
                            rgb(0x00FFFF)
                        } else {
                            rgb(0xE8EAF0)
                        })
                        .on_click(move |_, _, cx| {
                            update_state(cx, |state| {
                                state.mcp_port_settings_state.select(id);
                            });
                        }),
                )
                .child(
                    Label::new(transport)
                        .text_xs()
                        .truncate()
                        .flex_1()
                        .text_color(rgb(0x8A90A0)),
                )
                .child(
                    Label::new(if enabled { "Escuchando" } else { "Inactivo" })
                        .text_xs()
                        .flex_shrink_0()
                        .text_color(if enabled {
                            rgb(0x64E08A)
                        } else {
                            rgb(0x4A5060)
                        }),
                )
                .child(
                    Button::new(format!("mcp_port_remove_{}", id))
                        .rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("✕")
                        .compact()
                        .flex_shrink_0()
                        .w(px(28.0))
                        .on_click(move |_, _, cx| {
                            update_state(cx, |state| {
                                state.mcp_port_settings_state.remove_port(id);
                            });
                        }),
                )
                .into_any_element(),
        );
    }

    // ---------------------------------------------------------------------
    // ENSAMBLE
    // ---------------------------------------------------------------------
    // La posición de la ventana: la base más el desplazamiento del arrastre.
    let (left, top) = modal::modal_position(BASE_LEFT, BASE_TOP, offset);

    let panel = v_flex()
        .id("mcp_port_settings")
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(560.0))
        .max_h(px(520.0))
        .bg(rgb(0x181A20))
        .border_1()
        .border_color(rgb(0x2A2D37))
        .rounded(px(8.0))
        .shadow(vec![gpui_kit::BoxShadow {
            color: gpui_kit::black().opacity(0.5),
            offset: point(px(0.0), px(8.0)),
            blur_radius: px(24.0),
            spread_radius: px(0.0),
            inset: false,
        }])
        .p(px(14.0))
        .gap(px(10.0))
        .child(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(
                    // El título es el ASA de arrastre de la ventana: el
                    // usuario la agarra y la mueve. El `✕` queda fuera del
                    // asa para que siga siendo un botón y no un asa.
                    modal::drag_handle(
                        h_flex()
                            .id("mcp_port_settings_title")
                            .flex_1()
                            .items_center()
                            .child(
                                Label::new("HIKARU MCP PORT · MODEL CONTEXT PROTOCOL")
                                    .text_sm()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(0xE8EAF0)),
                            ),
                        ModalId::McpPortSettings,
                        dragging,
                    ),
                )
                .child(
                    Button::new("mcp_close_top")
                        .rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("✕")
                        .compact()
                        .flex_shrink_0()
                        .w(px(28.0))
                        .on_click(move |_, _, cx| {
                            update_state(cx, |state| {
                                state.mcp_port_settings_state.close();
                                // La ventana vuelve a su posición base la
                                // próxima vez que abra.
                                state.modal_drags.reset(ModalId::McpPortSettings);
                            });
                        }),
                ),
        )
        // 1. ADVANCED SETTINGS
        .child(
            h_flex()
                .w_full()
                .gap(px(8.0))
                .items_center()
                .child(
                    Button::new("mcp_advanced_toggle")
                        .rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label(if show_advanced {
                            "▾ Advanced Settings..."
                        } else {
                            "▸ Advanced Settings..."
                        })
                        .compact()
                        .flex_shrink_0()
                        .on_click(move |_, _, cx| {
                            update_state(cx, |state| {
                                state.mcp_port_settings_state.show_advanced =
                                    !state.mcp_port_settings_state.show_advanced;
                            });
                        }),
                )
                .child(
                    Label::new("Transporte del puerto · Unix Socket / Stdio / Server Flags")
                        .text_xs()
                        .truncate()
                        .flex_1()
                        .text_color(rgb(0x6A7080)),
                ),
        )
        .when(show_advanced, |this| {
            this.child(
                v_flex()
                    .w_full()
                    .gap(px(4.0))
                    .py(px(6.0))
                    .px(px(8.0))
                    .bg(rgb(0x20232C))
                    .rounded(px(4.0))
                    .children(advanced_rows),
            )
        })
        // 2. AVAILABLE PORTS
        .child(
            h_flex()
                .w_full()
                .items_center()
                .gap(px(6.0))
                .child(
                    Label::new(format!("Available Ports ({})", config.port_count()))
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0x8A90A0)),
                )
                .child(div().flex_1())
                .child(
                    Label::new(format!("Runtime: {}", config.runtime_dir.display()))
                        .text_xs()
                        .truncate()
                        .text_color(rgb(0x4A5060)),
                ),
        )
        .child(
            v_flex()
                .w_full()
                .gap(px(4.0))
                .max_h(px(190.0))
                .overflow_y_scrollbar()
                .when(config.ports.is_empty(), |this| {
                    this.child(
                        Label::new("No hay puertos: usá «Add MCP Port...».")
                            .text_xs()
                            .text_color(rgb(0x4A5060)),
                    )
                })
                .children(port_rows),
        )
        // 3. ADD MCP PORT
        .child(
            h_flex()
                .w_full()
                .gap(px(8.0))
                .items_center()
                .child(
                    Button::new("mcp_add_port")
                        .rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("+ Add MCP Port...")
                        .compact()
                        .flex_shrink_0()
                        .on_click(move |_, _, cx| {
                            update_state(cx, |state| {
                                state.mcp_port_settings_state.add_port();
                            });
                        }),
                )
                .child(
                    Label::new("Slots extra para más de 10 agentes en paralelo.")
                        .text_xs()
                        .truncate()
                        .flex_1()
                        .text_color(rgb(0x4A5060)),
                ),
        )
        // Pie: el estado del servidor y el botón de cerrar.
        .child(
            h_flex()
                .w_full()
                .gap(px(8.0))
                .items_center()
                .child(
                    Label::new(status.clone())
                        .text_xs()
                        .truncate()
                        .flex_1()
                        .text_color(rgb(0x8A90A0)),
                )
                .child(
                    Button::new("mcp_close")
                        .rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("Cerrar")
                        .compact()
                        .flex_shrink_0()
                        .on_click(move |_, _, cx| {
                            update_state(cx, |state| {
                                state.mcp_port_settings_state.close();
                                // La ventana vuelve a su posición base la
                                // próxima vez que abra.
                                state.modal_drags.reset(ModalId::McpPortSettings);
                            });
                        }),
                ),
        )
        // El root del diálogo se traga los clics: los del panel nunca bajan
        // al fondo de la app (ni a la Session Matrix que está detrás).
        .absorb_clicks()
        .into_any_element();

    // El backdrop es lo que hace de verdad de modal: cubre toda la ventana y
    // se come los clics que caen fuera del panel. Sin él, el clic en el fondo
    // atraviesa la capa modal y enciende clips y escenas en la Session Matrix.
    modal::overlay("mcp_port_settings_backdrop", panel)
}
