// crates/hikaru_dsp/src/plugin_node.rs

use hikaru_core::AudioBuffer;
use hikaru_midi::MidiEvent;
use hikaru_plugin_host::PluginInstance;

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
}

impl DspPluginNode {
    pub fn new(name: String, category: PluginCategory, instance: Box<dyn PluginInstance + Send>) -> Self {
        Self {
            name,
            category,
            instance,
            is_active: true,
        }
    }

    pub fn process(&mut self, buffer: &mut AudioBuffer, _midi_events: &[MidiEvent]) {
        if !self.is_active {
            return;
        }
        
        // Ejecuta el buffer a través de la instancia del VST3 o CLAP
        self.instance.process(buffer);
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
}