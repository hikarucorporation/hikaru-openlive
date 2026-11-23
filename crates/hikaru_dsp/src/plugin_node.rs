// crates/hikaru_dsp/src/plugin_node.rs

use hikaru_core::AudioBuffer;
use hikaru_midi::MidiEvent;
use hikaru_plugin_host::{PluginInstance, PluginTap};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginCategory {
    Vst3Generator,
    Vst3Fx,
    ClapGenerator,
    ClapFx,
}

pub struct DspPluginNode {
    pub name: String,
    pub category: PluginCategory,
    pub instance: Box<dyn PluginInstance + Send>,
    pub is_active: bool,
    /// Índice de slot en `Track::effects` de la GUI.
    ///
    /// Las pistas mezclan dispositivos nativos (`OpenPoly`, `OpenEQ3`) y plugins
    /// externos; guardar el `slot_idx` real garantiza que `SetExternalPluginParam`,
    /// modulación y bypass siempre peguen al nodo correcto aunque haya nativos antes.
    pub slot_idx: usize,
    /// A dónde se publica el buffer ya procesado.
    ///
    /// Lo consume la ventana del editor nativo, que corre en otro hilo con
    /// **otra** instancia del plugin (ver `hikaru_plugin_host::tap`). Sin esto
    /// el display del editor no tiene audio que mostrar: los knobs andan
    /// porque son derivados de parámetro, el espectro no.
    ///
    /// `None` cuando no hay editor abierto que lo pida: el motor se ahorra
    /// hasta la copia.
    pub tap: Option<Arc<PluginTap>>,
}

impl DspPluginNode {
    pub fn new(name: String, category: PluginCategory, instance: Box<dyn PluginInstance + Send>) -> Self {
        Self {
            name,
            category,
            instance,
            is_active: true,
            slot_idx: 0,
            tap: None,
        }
    }

    pub fn with_slot_idx(mut self, slot_idx: usize) -> Self {
        self.slot_idx = slot_idx;
        self
    }

    /// Conecta (o desconecta) la cinta hacia el editor nativo.
    pub fn set_tap(&mut self, tap: Option<Arc<PluginTap>>) {
        self.tap = tap;
    }

    /// Latencia reportada por este nodo en samples (`0` cuando está en bypass).
    pub fn latency_samples(&self) -> u32 {
        if self.is_active {
            self.instance.latency_samples()
        } else {
            0
        }
    }

    /// Actualiza el transporte musical del host antes de procesar el bloque.
    pub fn set_transport(&mut self, transport: hikaru_plugin_host::HostTransportInfo) {
        self.instance.set_transport(transport);
    }

    pub fn process(&mut self, buffer: &mut AudioBuffer, midi_events: &[MidiEvent]) {
        if !self.is_active {
            return;
        }

        // Entregamos las notas MIDI de la pista al plugin antes de renderizar
        // el bloque (lock-free vía anillo SPSC en VST3 y CLAP).
        for ev in midi_events {
            let _ = self.instance.queue_midi_event(ev.status, ev.data1, ev.data2);
        }

        // Ejecuta el buffer a través de la instancia del VST3 o CLAP
        self.instance.process(buffer);

        // Se publica DESPUÉS del efecto: el display tiene que mostrar lo que
        // sale del plugin, no lo que entró. `publish` es RT (sin locks ni
        // allocaciones) y sale temprano si no hay consumidor conectado.
        //
        // Luego mezclamos el monitoreo de retorno (audio generado por el
        // teclado en pantalla o preview interno de la ventana del editor).
        // Hacerlo DESPUÉS de `publish` evita cualquier lazo de feedback.
        if let Some(tap) = &self.tap {
            tap.publish(buffer.samples);
            tap.mix_monitor_into(buffer.samples);
        }
    }

    /// Lleva los parámetros pendientes del slot a la instancia de audio.
    ///
    /// Lo llama el hilo de control con lo que drenó de
    /// `DspSlot::take_dirty_external_params` (knobs del rack + feedback del
    /// editor nativo). Cada `set_param_normalized` viaja por el anillo
    /// lock-free de la instancia: vuelve de inmediato sin tocar el deadline
    /// del quantum. Devuelve cuántos entraron.
    pub fn sync_params(&mut self, params: &[(u32, f32)]) -> usize {
        hikaru_plugin_host::apply_params_to_instance(self.instance.as_mut(), params)
    }

    /// Cambio individual de parámetro (desde un knob del rack).
    ///
    /// Delega en `set_param_normalized` de la instancia, que es lock-free
    /// hacia el audio (anillo en VST3, cola en CLAP).
    pub fn set_param_normalized(&mut self, id: u32, value: f32) -> bool {
        self.instance.set_param_normalized(id, value)
    }

    /// Aplica un desplazamiento de modulación no destructivo (CLAP `ParamModEvent`).
    pub fn set_param_modulation(&mut self, id: u32, delta_normalized: f32) -> bool {
        self.instance.set_param_modulation(id, delta_normalized)
    }

    /// Info de un parámetro por id (para la UI del rack).
    pub fn param_info(&mut self, id: u32) -> Option<hikaru_plugin_host::PluginParamInfo> {
        for i in 0..self.instance.param_count() {
            if let Some(info) = self.instance.param_info(i) {
                if info.id == id {
                    return Some(info);
                }
            }
        }
        None
    }

    /// Snapshot de todos los parámetros (para sondeo inicial).
    pub fn snapshot_params(&mut self) -> Vec<(hikaru_plugin_host::PluginParamInfo, f32)> {
        self.instance.snapshot_params()
    }

    /// Restaura el estado binario completo (preset + ajustes internos) en la instancia DSP.
    pub fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
        self.instance.load_state(data)
    }
}