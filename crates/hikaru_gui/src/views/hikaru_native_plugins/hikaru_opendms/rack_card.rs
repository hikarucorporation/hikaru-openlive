// La tarjeta compacta de Hikaru OpenDMS en la tira de módulos del rack.
use crate::app::AppState;
use super::open_dms::{render_dms_ui, OpenDms};

pub fn render_rack_card(ui: &mut egui::Ui, app: &mut AppState, track_idx: usize, idx: usize) {
    let audio_proxy = app.audio_proxy.clone();
    let project_bpm = 120.0_f32;
    let Some(slot) = app.slot_mut(track_idx, idx) else {
        return;
    };
    let dms = slot.dms_state.get_or_insert_with(OpenDms::default);
    let mut dragged_sample = None;
    render_dms_ui(ui, dms, project_bpm, &mut dragged_sample, &audio_proxy);
}
