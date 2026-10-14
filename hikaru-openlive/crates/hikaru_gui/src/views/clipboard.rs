use crate::views::matrix::{AudioEvent, MatrixSlot};

#[derive(Clone, Debug, Default)]
pub struct MatrixClipboard {
    pub copied_slot: Option<MatrixSlot>,
    pub copied_events: Vec<AudioEvent>,
}

impl MatrixClipboard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn copy(&mut self, slot: &MatrixSlot) {
        self.copied_slot = Some(slot.clone());
    }

    pub fn has_content(&self) -> bool {
        self.copied_slot.is_some()
    }

    pub fn copy_events(&mut self, events: &[AudioEvent]) {
        self.copied_events = events.to_vec();
    }

    pub fn has_events(&self) -> bool {
        !self.copied_events.is_empty()
    }

    pub fn clear(&mut self) {
        self.copied_slot = None;
    }
}
