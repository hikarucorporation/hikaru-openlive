// crates/hikaru_mcp_port/src/telemetry.rs

//! Exportación de telemetría de plugins en tiempo real.
//!
//! # Para qué sirve
//!
//! Cuando un agente de IA audita el DAW, la pregunta que aparece siempre es
//! la misma: *¿qué estaba pasando exactamente en el momento en que el audio
//! se cortó?*. Contestarla a mano exige mirar logs mientras la cosa pasa; la
//! telemetría es la foto del plugin en ese instante.
//!
//! Por eso lo que se guarda acá no es "métricas de rendimiento" genéricas sino
//! lo que hace falta para una auditoría (y para una ingeniería inversa con
//! agentes): quién estaba cargado, cuántas veces se cortó el audio, cuántas
//! veces se movió un knob, y cuál era la superficie nativa del editor (el XID
//! de la ventana embebida), que es lo que hay que mirar cuando un plugin se
//! dibuja en el lugar equivocado.
//!
//! # Estado
//!
//! El exportador es un buffer en memoria con tope: la telemetría se exporta a
//! JSON para que el agente la consuma, no se queda acumulando para siempre.

use serde::{Deserialize, Serialize};

/// La foto de un plugin en un instante.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginTelemetry {
    /// Id estable del descriptor (el mismo que usa el host de plugins).
    pub plugin_id: String,
    /// Nombre del plugin.
    pub plugin_name: String,
    /// Formato: "VST3", "CLAP", ...
    pub format: String,
    /// Fabricante, si el host lo conoce.
    pub vendor: String,
    /// Carga de CPU del plugin, en fracción (0.0 a 1.0).
    pub cpu_load: f32,
    /// Cuántos xruns (cortes de audio) acumuló el plugin.
    pub xrun_count: u64,
    /// Cuántos cambios de parámetro recibió.
    pub parameters_changed: u64,
    /// Sample rate con la que está corriendo el host.
    pub sample_rate: f32,
    /// Superficie nativa del editor: XID, handle o ruta del framebuffer.
    ///
    /// Es el dato que hace falta para auditar un plugin mal embebido.
    pub editor_surface: Option<String>,
}

impl PluginTelemetry {
    /// Una foto mínima, con los contadores en cero.
    pub fn new(plugin_id: &str, plugin_name: &str, format: &str) -> Self {
        Self {
            plugin_id: plugin_id.to_string(),
            plugin_name: plugin_name.to_string(),
            format: format.to_string(),
            vendor: String::new(),
            cpu_load: 0.0,
            xrun_count: 0,
            parameters_changed: 0,
            sample_rate: 48_000.0,
            editor_surface: None,
        }
    }
}

/// El buffer de telemetría.
///
/// Se vacía a mano (`clear`) o cuando se supera la capacidad: la telemetría
/// vieja no le sirve a nadie y el objetivo es que el agente la pida fresca.
#[derive(Debug, Clone)]
pub struct TelemetryExporter {
    entries: Vec<PluginTelemetry>,
    capacity: usize,
}

impl Default for TelemetryExporter {
    fn default() -> Self {
        Self::new(256)
    }
}

impl TelemetryExporter {
    /// Un exportador que guarda hasta `capacity` fotos.
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::new(),
            capacity: capacity.max(1),
        }
    }

    /// Guarda una foto.
    ///
    /// Si el buffer está lleno, se descarta la más vieja: la foto que interesa
    /// para auditar un fallo es la más reciente.
    pub fn record(&mut self, entry: PluginTelemetry) {
        self.entries.push(entry);
        while self.entries.len() > self.capacity {
            self.entries.remove(0);
        }
    }

    /// Las fotos guardadas, de la más vieja a la más reciente.
    pub fn snapshot(&self) -> &[PluginTelemetry] {
        &self.entries
    }

    /// Cuántas fotos hay guardadas.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// ¿No hay ninguna foto?
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Vacía el buffer.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Exporta las fotos a JSON.
    ///
    /// Es lo que el agente recibe por el recurso `hikaru://plugins/telemetry`.
    /// Si la serialización falla (no debería: son datos planos), se devuelve
    /// un JSON de error en vez de romper la respuesta entera.
    pub fn export_json(&self) -> String {
        match serde_json::to_string_pretty(&self.entries) {
            Ok(json) => json,
            Err(err) => format!("{{\"error\":\"{err}\"}}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_buffer_descarta_lo_mas_viejo_al_llenarse() {
        let mut exporter = TelemetryExporter::new(2);
        for i in 0..3 {
            exporter.record(PluginTelemetry::new(
                &format!("id-{i}"),
                &format!("plugin-{i}"),
                "VST3",
            ));
        }
        assert_eq!(exporter.len(), 2);
        assert_eq!(exporter.snapshot()[0].plugin_id, "id-1");
        assert_eq!(exporter.snapshot()[1].plugin_id, "id-2");
    }

    #[test]
    fn la_exportacion_es_json_legible() {
        let mut exporter = TelemetryExporter::default();
        exporter.record(PluginTelemetry::new("id-1", "Vital", "VST3"));
        let json = exporter.export_json();
        assert!(json.contains("Vital"));
        assert!(json.contains("\"xrun_count\": 0"));
    }
}
