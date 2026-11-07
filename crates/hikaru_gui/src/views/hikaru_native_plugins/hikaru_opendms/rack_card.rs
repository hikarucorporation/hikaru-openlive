// La tarjeta compacta de Hikaru OpenDMS en la tira de módulos del rack.
use crate::app::AppState;
use crate::theme;

pub fn render_rack_card(ui: &mut egui::Ui, app: &AppState, track_idx: usize, idx: usize) {
    let Some(dms) = app.slot(track_idx, idx).and_then(|slot| slot.dms_state.as_ref()) else {
        ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new("No DMS state").size(11.0).color(theme::TEXT_MUTED));
        });
        return;
    };

    let pads = dms.pad_count;
    let loaded = dms.pads.iter().filter(|pad| pad.sample_path.is_some()).count();

    ui.vertical_centered(|ui| {
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("OpenDMS Sampler")
                .size(13.0)
                .strong()
                .color(theme::ACCENT_PRIMARY),
        );
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(format!("{loaded}/{pads} pads loaded"))
                .size(11.0)
                .color(theme::TEXT_SECONDARY),
        );
        ui.add_space(8.0);
        // Mini pad grid visualization
        let (rect, _) = ui.allocate_exact_size(egui::vec2(160.0, 80.0), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, theme::DAW_BG_LAYER_2);

        let cols = 4;
        let rows = 4;
        let pad_w = (rect.width() - 5.0 * 2.0) / cols as f32;
        let pad_h = (rect.height() - 5.0 * 2.0) / rows as f32;

        for r in 0..rows {
            for c in 0..cols {
                let pad_idx = (rows - 1 - r) * cols + c;
                let px = rect.min.x + 2.0 + c as f32 * pad_w;
                let py = rect.min.y + 2.0 + r as f32 * pad_h;
                let pad_rect = egui::Rect::from_min_size(egui::pos2(px, py), egui::vec2(pad_w - 2.0, pad_h - 2.0));

                let has_sample = dms.pads.get(pad_idx).map(|p| p.sample_path.is_some()).unwrap_or(false);
                let fill = if has_sample {
                    theme::ACCENT_PRIMARY
                } else {
                    theme::DAW_BG_LAYER_3
                };
                painter.rect_filled(pad_rect, 2.0, fill);
            }
        }
    });
}
