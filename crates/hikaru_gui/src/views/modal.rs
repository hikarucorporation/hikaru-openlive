// Hikaru OpenLive - Modal Support (egui)
// GNU AGPLv3
// crates/hikaru_gui/src/views/modal.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModalId {
    AudioSettings,
    ExternalPluginsSettings,
    McpPortSettings,
    ControlPanel,
    About,
}

#[derive(Debug, Default, Clone)]
pub struct ModalDrags;

impl ModalDrags {
    pub fn new() -> Self {
        Self
    }

    pub fn reset(&mut self, _id: ModalId) {}
}
