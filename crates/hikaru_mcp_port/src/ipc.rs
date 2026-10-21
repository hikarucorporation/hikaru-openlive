// crates/hikaru_mcp_port/src/ipc.rs

//! Stub de la comunicación IPC / socket entre el DAW y los agentes MCP.
//!
//! # Qué es y qué no es este módulo
//!
//! Esto **no** habla con nadie todavía: es el esqueleto del transporte, con
//! las reglas que van a tener que cumplirse y los eventos que van a existir,
//! pero sin sockets reales. Está así a propósito por dos razones:
//!
//! 1. El transporte de verdad necesita vivir en un hilo aparte del render de
//!    GPUI (un `accept()` bloqueando el hilo principal congela la ventana), y
//!    esa parte se implementa cuando haya el servidor MCP del otro lado.
//! 2. Las reglas (un slot deshabilitado no acepta conexiones, un slot lleno
//!    rechaza al cliente que sobra) se pueden fijar AHORA con tests, y que el
//!    transporte real las cumpla después es cambiar la implementación, no el
//!    contrato.
//!
//! El estado que se lleva es todo en memoria: qué eventos pasaron y qué
//! agentes hay enganchados en cada slot.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use crate::config::{McpPortConfig, McpTransport, PortSlotId};

/// Hacia dónde apunta un endpoint de IPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcDirection {
    /// El DAW escucha y el agente se conecta. Es el caso normal: el DAW vive
    /// mucho más que el agente, así que es el que publica el socket.
    Listen,
    /// El DAW se conecta a un servidor MCP externo (el modo `Server Flags`).
    Connect,
}

/// Un evento del ciclo de vida de una conexión.
///
/// El stub los registra en orden: cuando llegue el transporte real, esta
/// lista es el log que se le pasa al agente y el que la UI muestra.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpcEvent {
    /// El DAW abrió un socket para un slot.
    Listen {
        port_slot: PortSlotId,
        path: Option<PathBuf>,
    },
    /// El DAW se conectó a un servidor externo.
    Connect {
        port_slot: PortSlotId,
        path: Option<PathBuf>,
    },
    /// Un agente terminó el saludo inicial.
    Handshake {
        port_slot: PortSlotId,
        agent: String,
    },
    /// Un agente se fue.
    Disconnect { port_slot: PortSlotId },
    /// Algo falló y se avisó por el canal de errores, no por el del agente.
    Failed {
        port_slot: PortSlotId,
        reason: String,
    },
}

impl fmt::Display for IpcEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IpcEvent::Listen { port_slot, path } => {
                write!(
                    f,
                    "MCP Port {port_slot}: escuchando en {}",
                    path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "stdio".into())
                )
            }
            IpcEvent::Connect { port_slot, path } => {
                write!(
                    f,
                    "MCP Port {port_slot}: conectado a {}",
                    path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "stdio".into())
                )
            }
            IpcEvent::Handshake { port_slot, agent } => {
                write!(f, "MCP Port {port_slot}: handshake de '{agent}'")
            }
            IpcEvent::Disconnect { port_slot } => {
                write!(f, "MCP Port {port_slot}: cliente desconectado")
            }
            IpcEvent::Failed { port_slot, reason } => {
                write!(f, "MCP Port {port_slot}: falló — {reason}")
            }
        }
    }
}

/// Por qué una operación de IPC no se pudo hacer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpcError {
    /// El transporte elegido todavía no tiene transporte real: este stub sólo
    /// modela el socket de dominio Unix.
    UnsupportedTransport(McpTransport),
    /// El slot no existe en la configuración.
    UnknownPort(PortSlotId),
    /// El slot está deshabilitado: no acepta conexiones.
    PortDisabled(PortSlotId),
    /// El slot ya tiene todos sus clientes.
    PortFull(PortSlotId),
    /// El socket no está listo.
    SocketUnavailable(PathBuf),
}

impl fmt::Display for IpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IpcError::UnsupportedTransport(transport) => {
                write!(f, "el transporte '{}' todavía no está implementado", transport.label())
            }
            IpcError::UnknownPort(id) => write!(f, "el slot {id} no existe"),
            IpcError::PortDisabled(id) => write!(f, "el slot {id} está deshabilitado"),
            IpcError::PortFull(id) => write!(f, "el slot {id} ya tiene todos sus clientes"),
            IpcError::SocketUnavailable(path) => {
                write!(f, "el socket {} no está disponible", path.display())
            }
        }
    }
}

impl std::error::Error for IpcError {}

/// El esqueleto del transporte IPC.
///
/// No hace E/S: registra lo que **haría** y aplica las reglas que tienen que
/// valer cuando el transporte real exista.
#[derive(Debug, Clone)]
pub struct IpcStub {
    direction: IpcDirection,
    events: Vec<IpcEvent>,
    /// Agentes enganchados por slot.
    clients: BTreeMap<PortSlotId, Vec<String>>,
}

impl Default for IpcStub {
    fn default() -> Self {
        Self::new(IpcDirection::Listen)
    }
}

impl IpcStub {
    /// Un stub vacío en la dirección indicada.
    pub fn new(direction: IpcDirection) -> Self {
        Self {
            direction,
            events: Vec::new(),
            clients: BTreeMap::new(),
        }
    }

    /// La dirección con la que se creó.
    pub fn direction(&self) -> IpcDirection {
        self.direction
    }

    /// Abre el socket de un slot para que los agentes se enganchen.
    ///
    /// Falla si el slot está deshabilitado o si el transporte no es un socket
    /// de dominio Unix: el stub no simula un `accept()` sobre stdio porque
    /// sería una mentira (en stdio el proceso entero ES el canal).
    pub fn listen(&mut self, port: &McpPortConfig) -> Result<(), IpcError> {
        if !port.enabled {
            return Err(IpcError::PortDisabled(port.id));
        }
        let path = match &port.transport {
            McpTransport::UnixSocket { path } => Some(path.clone()),
            other => {
                self.events.push(IpcEvent::Failed {
                    port_slot: port.id,
                    reason: IpcError::UnsupportedTransport(other.clone()).to_string(),
                });
                return Err(IpcError::UnsupportedTransport(other.clone()));
            }
        };

        self.events.push(IpcEvent::Listen {
            port_slot: port.id,
            path,
        });
        Ok(())
    }

    /// Se conecta a un servidor MCP externo (transporte `Server Flags`).
    pub fn connect(&mut self, port: &McpPortConfig) -> Result<(), IpcError> {
        if !port.enabled {
            return Err(IpcError::PortDisabled(port.id));
        }
        let path = port.transport.socket_path().map(|p| p.to_path_buf());
        self.events.push(IpcEvent::Connect {
            port_slot: port.id,
            path,
        });
        Ok(())
    }

    /// Registra el saludo de un agente.
    ///
    /// Falla si el slot ya tiene el máximo de clientes: el límite es por
    /// slot, no por servidor, así que un agente que se cuelgue en el slot 1
    /// no puede ocupar el 2.
    pub fn handshake(&mut self, port: &McpPortConfig, agent: &str) -> Result<(), IpcError> {
        let clients = self.clients.entry(port.id).or_default();
        if clients.len() >= port.max_clients {
            return Err(IpcError::PortFull(port.id));
        }
        clients.push(agent.to_string());
        self.events.push(IpcEvent::Handshake {
            port_slot: port.id,
            agent: agent.to_string(),
        });
        Ok(())
    }

    /// Saca a un agente del slot.
    pub fn disconnect(&mut self, port_slot: PortSlotId) {
        if let Some(clients) = self.clients.get_mut(&port_slot) {
            clients.pop();
        }
        self.events.push(IpcEvent::Disconnect { port_slot });
    }

    /// Los eventos, en orden.
    pub fn events(&self) -> &[IpcEvent] {
        &self.events
    }

    /// El último evento registrado.
    pub fn last_event(&self) -> Option<&IpcEvent> {
        self.events.last()
    }

    /// Los agentes enganchados a un slot.
    pub fn clients(&self, port_slot: PortSlotId) -> &[String] {
        self.clients.get(&port_slot).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Cuántos agentes hay enganchados en total.
    pub fn client_count(&self) -> usize {
        self.clients.values().map(|v| v.len()).sum()
    }

    /// Borra el historial de eventos (los clientes enganchados se quedan).
    pub fn clear_events(&mut self) {
        self.events.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::McpServerConfig;

    fn port(config: &McpServerConfig, id: PortSlotId) -> McpPortConfig {
        config.slot(id).cloned().expect("slot")
    }

    #[test]
    fn un_slot_deshabilitado_no_acepta_conexiones() {
        let config = McpServerConfig::new();
        let mut stub = IpcStub::default();
        let err = stub.listen(&port(&config, 1)).unwrap_err();
        assert_eq!(err, IpcError::PortDisabled(1));
    }

    #[test]
    fn un_slot_habilitado_publica_el_socket() {
        let mut config = McpServerConfig::new();
        config.slot_mut(1).unwrap().enabled = true;
        let mut stub = IpcStub::default();

        stub.listen(&port(&config, 1)).unwrap();

        assert!(matches!(
            stub.last_event(),
            Some(IpcEvent::Listen { port_slot: 1, .. })
        ));
    }

    #[test]
    fn el_salto_de_mano_respeta_el_maximo_de_clientes() {
        let mut config = McpServerConfig::new();
        config.slot_mut(1).unwrap().enabled = true;
        let mut stub = IpcStub::default();

        stub.handshake(&port(&config, 1), "opencode").unwrap();
        let err = stub.handshake(&port(&config, 1), "claude").unwrap_err();
        assert_eq!(err, IpcError::PortFull(1));
        assert_eq!(stub.client_count(), 1);
    }

    #[test]
    fn al_desconectar_queda_lugar_para_otro() {
        let mut config = McpServerConfig::new();
        config.slot_mut(1).unwrap().enabled = true;
        let mut stub = IpcStub::default();

        stub.handshake(&port(&config, 1), "opencode").unwrap();
        stub.disconnect(1);
        stub.handshake(&port(&config, 1), "claude").unwrap();

        assert_eq!(stub.client_count(), 1);
    }

    #[test]
    fn un_transporte_sin_socket_no_se_puede_escuchar() {
        let mut config = McpServerConfig::new();
        config.slot_mut(1).unwrap().enabled = true;
        config.slot_mut(1).unwrap().transport = McpTransport::Stdio;
        let mut stub = IpcStub::default();

        assert!(matches!(
            stub.listen(&port(&config, 1)),
            Err(IpcError::UnsupportedTransport(McpTransport::Stdio))
        ));
    }
}
