// crates/hikaru_mcp_port/src/config.rs

//! La configuración del servidor MCP: con qué transporte se habla y qué slots
//! de puerto quedan disponibles para que los agentes se enganchen.
//!
//! # Por qué los puertos son "slots" y no un número
//!
//! Un DAW exposes muchas superficies a la vez (la matriz, el rack de DSP, los
//! plugins, el transporte) y cada agente de IA que se conecta quiere una
//! *sesión* propia: un cliente MCP connects, pregunta y se desconecta, sin
//! mezclarse con el que está de al lado. Por eso el puerto no es un número de
//! red sino un **slot numerado** (`MCP Port 1`, `MCP Port 2`, ...) que el
//! agente elige al conectarse.
//!
//! El mapeo slot → socket es lo único que la app tiene que garantizar: dos
//! agentes distintos nunca pueden terminar hablando por el mismo socket, y un
//! slot liberado tiene que quedar libre para el siguiente.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Identificador de un slot de puerto MCP.
///
/// Es un número y no un enum a propósito: la idea es que el usuario pueda
/// instanciar slots *extra* (`MCP Port 11`, `MCP Port 12`, ...) cuando diez
/// conexiones simultáneas no alcanzan, y un enum cerrado obligaría a tocar el
/// código cada vez que se agrega uno.
pub type PortSlotId = u16;

/// El primer slot válido.
///
/// No existe el slot 0: "puerto 0" en Unix significa "elegí uno libre
/// automáticamente", y acá el slot ES la dirección, no un puerto del sistema.
pub const FIRST_PORT_SLOT: PortSlotId = 1;

/// Cuántos slots se crean por omisión.
///
/// Diez es un número redondo que cubre el caso real (un agente por tarea:
/// auditoría, ingeniería inversa, generación de material, QA...) sin que la
/// lista de la ventana sea un scroll infinito. Los que falten se agregan con
/// `Add MCP Port...`.
pub const DEFAULT_PORT_SLOTS: usize = 10;

/// El transporte con el que se habla MCP en un slot.
///
/// Los tres son reales en el protocolo MCP; lo que cambia es quién habla y
/// por dónde:
///
/// - **Unix Socket**: el DAW escucha y el agente se conecta. Es el modo por
///   omisión porque no abre puertos de red y sobrevive a que el agente se
///   muera a mitad de una frase.
/// - **Stdio**: el agente lanza el servidor y habla JSON-RPC por la entrada y
///   la salida estándar del proceso. Es el modo que usan los CLI de OpenCode y
///   Claude Code.
/// - **Server Flags**: el DAW lanza un servidor MCP externo con una línea de
///   comandos, en vez de hablar él mismo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum McpTransport {
    /// Socket de dominio Unix. El DAW escucha; el agente se conecta.
    UnixSocket {
        /// Ruta del socket dentro del runtime dir.
        path: PathBuf,
    },
    /// JSON-RPC 2.0 por stdin/stdout del propio proceso.
    Stdio,
    /// Línea de comandos con la que se lanza un servidor MCP externo.
    ServerFlags(ServerFlags),
}

impl McpTransport {
    /// Rótulo corto para la UI.
    ///
    /// Es lo que se ve en la lista de puertos: el usuario tiene que poder
    /// distinguir de un vistazo cómo habla cada slot.
    pub fn label(&self) -> &'static str {
        match self {
            McpTransport::UnixSocket { .. } => "Unix Socket",
            McpTransport::Stdio => "Stdio",
            McpTransport::ServerFlags(_) => "Server Flags",
        }
    }

    /// ¿Este transporte termina en un socket de dominio Unix?
    ///
    /// Lo que decide si hace falta crear el archivo de socket en el disco.
    pub fn uses_unix_socket(&self) -> bool {
        matches!(self, McpTransport::UnixSocket { .. })
    }

    /// La ruta del socket, si el transporte tiene una.
    pub fn socket_path(&self) -> Option<&Path> {
        match self {
            McpTransport::UnixSocket { path } => Some(path.as_path()),
            _ => None,
        }
    }
}

/// La línea de comandos con la que se lanza un servidor MCP externo.
///
/// Vive en la configuración (y no hardcodeada en el código) porque el
/// ejecutable y sus flags cambian entre distribuciones: el mismo DAW puede
/// hablar con `hikaru_mcp --mode=live` en una máquina y con un wrapper
/// propio del usuario en otra.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerFlags {
    /// Ruta (o nombre) del ejecutable del servidor MCP.
    pub executable: String,
    /// Argumentos con los que se invoca.
    pub args: Vec<String>,
    /// Variables de entorno extra para el proceso hijo.
    pub env: BTreeMap<String, String>,
    /// Directorio de trabajo del proceso hijo.
    pub working_dir: Option<PathBuf>,
}

impl Default for ServerFlags {
    fn default() -> Self {
        Self {
            executable: "hikaru_mcp".to_string(),
            args: vec!["--mode=live".to_string()],
            env: BTreeMap::new(),
            working_dir: None,
        }
    }
}

impl ServerFlags {
    /// La línea de comandos tal como se la ve en la UI.
    pub fn command_line(&self) -> String {
        let mut line = self.executable.clone();
        for arg in &self.args {
            line.push(' ');
            line.push_str(arg);
        }
        line
    }
}

/// La configuración de UN slot de puerto.
///
/// Un slot es una dirección: tiene número, nombre, transporte y si está
/// activo. Lo que el slot *hace* (JSON-RPC, herramientas, recursos) es
/// responsabilidad del servidor, no de la configuración.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpPortConfig {
    /// Número del slot. Estable: es lo que el agente usa para elegir.
    pub id: PortSlotId,
    /// Nombre visible ("MCP Port 3").
    pub label: String,
    /// ¿El slot acepta conexiones?
    pub enabled: bool,
    /// Por dónde se habla.
    pub transport: McpTransport,
    /// Cuántos clientes pueden estar enganchados a la vez.
    pub max_clients: usize,
}

impl McpPortConfig {
    /// Un slot nuevo, apuntando a un socket Unix.
    ///
    /// Nace **deshabilitado**: que aparezca en la lista no significa que esté
    /// escuchando. El usuario lo activa cuando quiere que un agente se
    /// conecte.
    pub fn new(id: PortSlotId, socket_path: PathBuf) -> Self {
        Self {
            id,
            label: format!("MCP Port {id}"),
            enabled: false,
            transport: McpTransport::UnixSocket { path: socket_path },
            max_clients: 1,
        }
    }
}

/// La configuración completa del servidor MCP.
///
/// Es la estructura que la ventana `Hikaru MCP Port Settings...` edita y la
/// que el servidor MCP usa para saber qué slots existen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// ¿El servidor MCP está activo?
    pub enabled: bool,
    /// Dónde viven los sockets de dominio Unix.
    pub runtime_dir: PathBuf,
    /// Flags con los que se lanza un servidor MCP externo.
    pub server_flags: ServerFlags,
    /// Los slots, en orden de id.
    pub ports: Vec<McpPortConfig>,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        let runtime_dir = default_runtime_dir();
        let ports = (FIRST_PORT_SLOT..)
            .take(DEFAULT_PORT_SLOTS)
            .map(|id| McpPortConfig::new(id, socket_path_in(&runtime_dir, id)))
            .collect();

        Self {
            enabled: false,
            runtime_dir,
            server_flags: ServerFlags::default(),
            ports,
        }
    }
}

impl McpServerConfig {
    /// Una configuración con los slots por omisión.
    pub fn new() -> Self {
        Self::default()
    }

    /// El slot con ese id, si existe.
    pub fn slot(&self, id: PortSlotId) -> Option<&McpPortConfig> {
        self.ports.iter().find(|p| p.id == id)
    }

    /// El slot con ese id, para editarlo.
    pub fn slot_mut(&mut self, id: PortSlotId) -> Option<&mut McpPortConfig> {
        self.ports.iter_mut().find(|p| p.id == id)
    }

    /// El siguiente id libre.
    ///
    /// Se calcula como "el más alto + 1" y no como "la cantidad de slots + 1":
    /// si el usuario borra un slot del medio, la cantidad baja pero el id más
    /// alto sigue ocupado, y "cantidad + 1" devolvería un id que ya existe.
    ///
    /// Los ids que quedan huecos (al borrar el 3 de 1..10) no se reutilizan:
    /// un id es la dirección que el agente usa para elegir puerto, y que
    /// vuelva a apuntar a otro socket sería confuso.
    pub fn next_port_id(&self) -> PortSlotId {
        self.ports
            .iter()
            .map(|p| p.id)
            .max()
            .map(|max| max.saturating_add(1))
            .unwrap_or(FIRST_PORT_SLOT)
    }

    /// Agrega un slot nuevo al final.
    ///
    /// Devuelve el id del slot creado, para que la UI pueda seleccionarlo.
    pub fn add_port(&mut self) -> PortSlotId {
        let id = self.next_port_id();
        let path = socket_path_in(&self.runtime_dir, id);
        self.ports.push(McpPortConfig::new(id, path));
        self.ports.sort_by_key(|p| p.id);
        id
    }

    /// Borra un slot. Devuelve si había algo que borrar.
    pub fn remove_port(&mut self, id: PortSlotId) -> bool {
        let before = self.ports.len();
        self.ports.retain(|p| p.id != id);
        self.ports.len() != before
    }

    /// Cuántos slots hay.
    pub fn port_count(&self) -> usize {
        self.ports.len()
    }

    /// Los slots que aceptan conexiones.
    pub fn enabled_ports(&self) -> impl Iterator<Item = &McpPortConfig> {
        self.ports.iter().filter(|p| p.enabled)
    }

    /// La ruta del socket de un slot, tal como quedaría en disco.
    ///
    /// Se recalcula desde `runtime_dir` en vez de leer la del slot: si el
    /// usuario cambia el runtime dir, los slots que todavía no se tocaron
    /// tienen que seguir apuntando al lugar nuevo.
    pub fn socket_path_for(&self, id: PortSlotId) -> PathBuf {
        socket_path_in(&self.runtime_dir, id)
    }

    /// Chequea que la configuración sea coherente.
    ///
    /// No aborta ni corrige: devuelve la lista de problemas para que la UI
    /// los muestre. Un slot con el id duplicado o con más clientes de los
    /// permitidos tiene que verse, no desaparecer.
    pub fn validate(&self) -> Vec<ConfigIssue> {
        let mut issues = Vec::new();
        let mut seen = BTreeMap::new();

        for port in &self.ports {
            if port.id < FIRST_PORT_SLOT {
                issues.push(ConfigIssue::InvalidSlotId(port.id));
            }
            if seen.insert(port.id, ()).is_some() {
                issues.push(ConfigIssue::DuplicateSlot(port.id));
            }
            if port.max_clients == 0 {
                issues.push(ConfigIssue::InvalidMaxClients(port.id));
            }
            if let McpTransport::UnixSocket { path } = &port.transport {
                if path.as_os_str().is_empty() {
                    issues.push(ConfigIssue::InvalidSocketPath(port.id));
                }
            }
        }

        issues
    }
}

/// Un problema encontrado en la configuración.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigIssue {
    /// Dos slots con el mismo id.
    DuplicateSlot(PortSlotId),
    /// Un slot con id inválido (0 o menor).
    InvalidSlotId(PortSlotId),
    /// Un socket Unix sin ruta.
    InvalidSocketPath(PortSlotId),
    /// Un slot que no admite clientes (siempre va a rechazar la conexión).
    InvalidMaxClients(PortSlotId),
}

impl ConfigIssue {
    /// El mensaje para la UI.
    pub fn message(&self) -> String {
        match self {
            ConfigIssue::DuplicateSlot(id) => {
                format!("El slot {id} está duplicado: dos puertos con el mismo id.")
            }
            ConfigIssue::InvalidSlotId(id) => {
                format!("El slot {id} no es válido: los ids empiezan en {FIRST_PORT_SLOT}.")
            }
            ConfigIssue::InvalidSocketPath(id) => {
                format!("El slot {id} no tiene ruta de socket Unix.")
            }
            ConfigIssue::InvalidMaxClients(id) => {
                format!("El slot {id} no admite clientes: el máximo es 0.")
            }
        }
    }
}

/// El directorio de runtime donde viven los sockets.
///
/// Se usa `$XDG_RUNTIME_DIR/hikaru` cuando está definido (es efímero y
/// permissions 0700: lo correcto para un socket que abre el DAW) y se cae a
/// `/tmp/hikaru` si no.
pub fn default_runtime_dir() -> PathBuf {
    match std::env::var("XDG_RUNTIME_DIR") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir).join("hikaru"),
        _ => PathBuf::from("/tmp/hikaru"),
    }
}

/// La ruta del socket de un slot dentro de un runtime dir.
///
/// El nombre lleva el id del slot y no un índice: si el usuario borra el slot
/// 3 y crea uno nuevo, el id 3 vuelve a ser el 3 y el socket vuelve a ser el
/// mismo. Un índice ("slot-4") haría que los sockets se corrieran de nombre
/// cada vez que se agrega o borra uno, y los agentes que ya están conectados
/// se quedaron hablando con un socket que ya no existe.
pub fn socket_path_in(runtime_dir: &Path, id: PortSlotId) -> PathBuf {
    runtime_dir.join(format!("hikaru-mcp-{id}.sock"))
}

// =========================================================================
// PERSISTENCIA
// =========================================================================

/// La ruta del archivo donde vive la configuración de los puertos MCP.
///
/// Es `$XDG_CONFIG_HOME/hikaru/mcp-ports.json` (o `~/.config/hikaru/...` si
/// `XDG_CONFIG_HOME` no está definido), que es donde un DAW guarda lo que el
/// usuario configura y no lo que la app calcula.
///
/// El archivo NO está en el runtime dir: aquel es efímero (se borra al
/// reiniciar la sesión) y la configuración de los puertos tiene que
/// sobrevivir a un reinicio de la máquina. Los SOCKETS sí viven en el runtime
/// dir, porque son archivos de vida corta.
pub fn default_config_path() -> PathBuf {
    let base = match std::env::var("XDG_CONFIG_HOME") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
            PathBuf::from(home).join(".config")
        }
    };
    base.join("hikaru").join("mcp-ports.json")
}

/// Lee la configuración del archivo.
///
/// Si el archivo NO existe, devuelve la de por omisión: es lo que quiere el
/// usuario la primera vez, y se diferencia de "hay un archivo roto" (que sí
/// es un error) porque un archivo que no se puede leer no se puede
/// distinguir de uno que nunca se escribió.
pub fn load_config(path: &Path) -> Result<McpServerConfig, ConfigError> {
    if !path.exists() {
        return Ok(McpServerConfig::default());
    }
    let text = std::fs::read_to_string(path)?;
    let config: McpServerConfig = serde_json::from_str(&text)?;
    Ok(config)
}

/// Escribe la configuración al archivo.
///
/// El archivo se escribe con JSON lindo porque lo edita una persona a mano
/// cuando algo sale mal (y es lo primero que se le pide al agente: "pasame
/// la config del puerto 3").
pub fn save_config(
    config: &McpServerConfig,
    path: &Path,
) -> Result<(), ConfigError> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    let json = serde_json::to_string_pretty(config)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Por qué no se pudo leer o escribir la configuración.
#[derive(Debug)]
pub enum ConfigError {
    /// Error de E/S al leer o escribir el archivo.
    Io(std::io::Error),
    /// El archivo existe pero no es una configuración válida.
    Parse(serde_json::Error),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "no se pudo acceder a la configuración: {}", e),
            ConfigError::Parse(e) => write!(f, "la configuración no es válida: {}", e),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<std::io::Error> for ConfigError {
    fn from(e: std::io::Error) -> Self {
        ConfigError::Io(e)
    }
}

impl From<serde_json::Error> for ConfigError {
    fn from(e: serde_json::Error) -> Self {
        ConfigError::Parse(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_configuracion_por_omision_tiene_diez_slots_desde_el_1() {
        let config = McpServerConfig::new();
        assert_eq!(config.port_count(), DEFAULT_PORT_SLOTS);
        assert_eq!(config.ports[0].id, FIRST_PORT_SLOT);
        assert_eq!(config.ports[9].id, 10);
        assert!(config.ports.iter().all(|p| !p.enabled));
    }

    #[test]
    fn agregar_un_puerto_crea_el_11() {
        let mut config = McpServerConfig::new();
        let id = config.add_port();
        assert_eq!(id, 11);
        assert_eq!(config.port_count(), DEFAULT_PORT_SLOTS + 1);
        assert_eq!(config.slot(11).unwrap().label, "MCP Port 11");
    }

    #[test]
    fn borrar_un_puerto_no_pisa_los_ids_existentes() {
        let mut config = McpServerConfig::new();
        assert!(config.remove_port(3));
        assert!(config.slot(3).is_none());
        // El hueco del 3 no se reutiliza: el siguiente id es el 11, que no
        // existe todavía. "Cantidad de slots + 1" habría devuelto el 10, que
        // ya está ocupado.
        assert_eq!(config.next_port_id(), 11);
        assert!(config.slot(10).is_some());
    }

    #[test]
    fn el_ultimo_id_se_reutiliza_al_borrarlo() {
        let mut config = McpServerConfig::new();
        assert!(config.remove_port(10));
        // El 10 era el más alto: al borrarlo, el id 10 vuelve a estar libre.
        assert_eq!(config.next_port_id(), 10);
    }

    #[test]
    fn quitar_el_ultimo_slot_devuelve_el_primer_id() {
        let mut config = McpServerConfig::new();
        config.ports.clear();
        assert_eq!(config.next_port_id(), FIRST_PORT_SLOT);
    }

    #[test]
    fn la_configuracion_por_omision_no_tiene_problemas() {
        assert!(McpServerConfig::new().validate().is_empty());
    }

    #[test]
    fn un_id_duplicado_se_reporta() {
        let mut config = McpServerConfig::new();
        let mut twin = config.ports[0].clone();
        twin.label = "MCP Port 1 (copia)".to_string();
        config.ports.push(twin);

        let issues = config.validate();
        assert!(issues.contains(&ConfigIssue::DuplicateSlot(1)));
    }

    #[test]
    fn el_socket_sigue_al_runtime_dir() {
        let mut config = McpServerConfig::new();
        config.runtime_dir = PathBuf::from("/run/user/1000/hikaru");
        assert_eq!(
            config.socket_path_for(2),
            PathBuf::from("/run/user/1000/hikaru/hikaru-mcp-2.sock")
        );
    }

    #[test]
    fn una_configuracion_que_no_existe_devuelve_la_de_omision() {
        let path = std::env::temp_dir().join("hikaru_mcp_no_existe_este_archivo.json");
        let _ = std::fs::remove_file(&path);

        let config = load_config(&path).expect("no debería fallar");
        assert_eq!(config.port_count(), DEFAULT_PORT_SLOTS);
        assert!(config.ports.iter().all(|p| !p.enabled));
    }

    #[test]
    fn la_configuracion_sobrevive_al_viaje_de_ida_y_vuelta() {
        let path = std::env::temp_dir()
            .join(format!("hikaru_mcp_cfg_{}.json", std::process::id()));

        let mut config = McpServerConfig::new();
        config.enabled = true;
        config.slot_mut(3).unwrap().enabled = true;
        config.add_port();

        save_config(&config, &path).expect("no debería fallar al guardar");
        let leida = load_config(&path).expect("no debería fallar al leer");

        assert!(leida.enabled);
        assert!(leida.slot(3).unwrap().enabled);
        assert_eq!(leida.port_count(), DEFAULT_PORT_SLOTS + 1);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn una_configuracion_rota_es_un_error_y_no_un_default() {
        let path = std::env::temp_dir()
            .join(format!("hikaru_mcp_rota_{}.json", std::process::id()));
        std::fs::write(&path, b"{ esto no es json").unwrap();

        assert!(matches!(load_config(&path), Err(ConfigError::Parse(_))));

        let _ = std::fs::remove_file(&path);
    }
}
