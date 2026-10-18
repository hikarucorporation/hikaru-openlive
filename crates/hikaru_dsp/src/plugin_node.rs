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
            tap: None,
        }
    }

    /// Conecta (o desconecta) la cinta hacia el editor nativo.
    pub fn set_tap(&mut self, tap: Option<Arc<PluginTap>>) {
        self.tap = tap;
    }

    pub fn process(&mut self, buffer: &mut AudioBuffer, _midi_events: &[MidiEvent]) {
        if !self.is_active {
            return;
        }
        
        // Ejecuta el buffer a través de la instancia del VST3 o CLAP
        self.instance.process(buffer);

        // Se publica DESPUÉS del efecto: el display tiene que mostrar lo que
        // sale del plugin, no lo que entró. `publish` es RT (sin locks ni
        // allocaciones) y sale temprano si no hay consumidor conectado.
        if let Some(tap) = &self.tap {
            tap.publish(buffer.samples);
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
}