// Hikaru OpenLive - File Explorer / Right-Side Bitwig Browser (egui)
// GNU AGPLv3
// crates/hikaru_gui/src/views/explorer.rs

use std::fs;
use std::path::PathBuf;

use egui::{pos2, vec2, Align2, Color32, Rect, RichText, ScrollArea, Sense, Stroke, Ui};

use crate::app::AppState;
use crate::audio_proxy::GuiCommand;
use crate::theme::*;
use crate::ui::bitwig_kit::{
    capsule_icon_button, capsule_text_button, fader_horizontal_range, JoinSide,
};
use crate::ui::fonts;
use crate::ui::icons::{self, BrowserBadgeKind};
use crate::views::dsp_rack::PLUGIN_CATALOG;
use crate::views::mixer::DspSlot;

/// Entrada de archivo/directorio cacheada en memoria.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplorerEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_midi: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplorerAudioDrag(pub PathBuf);

pub fn mark_dragged_sample(state: &mut AppState, path: &PathBuf) {
    state.dragged_sample = Some(path.clone());
}

pub fn clear_dragged_sample(state: &mut AppState) {
    state.dragged_sample = None;
}

pub fn begin_wavetable_pick(state: &mut AppState, track_idx: usize, slot_idx: usize) {
    state.pending_wavetable_slot = Some((track_idx, slot_idx));
    state.show_explorer = true;
}

pub fn default_start_path() -> PathBuf {
    match std::env::var("HOME") {
        Ok(home) if PathBuf::from(&home).is_dir() => PathBuf::from(home),
        _ => PathBuf::from("/"),
    }
}

pub fn is_supported_file(path: &PathBuf) -> bool {
    is_audio_file(path) || is_midi_file(path)
}

pub fn is_audio_file(path: &PathBuf) -> bool {
    if let Some(ext) = path.extension() {
        let ext_str = ext.to_string_lossy().to_lowercase();
        matches!(
            ext_str.as_str(),
            "wav" | "flac" | "ogg" | "mp3" | "aiff" | "synth"
        )
    } else {
        false
    }
}

pub fn is_midi_file(path: &PathBuf) -> bool {
    if let Some(ext) = path.extension() {
        let ext_str = ext.to_string_lossy().to_lowercase();
        matches!(ext_str.as_str(), "mid" | "midi")
    } else {
        false
    }
}

pub fn parse_bpm_from_filename(filename: &str) -> Option<f32> {
    let lower = filename.to_lowercase();

    if let Some(bpm_idx) = lower.find("bpm") {
        let prefix = &lower[..bpm_idx];
        let digits: String = prefix
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '_' || *c == ' ')
            .collect::<String>()
            .chars()
            .rev()
            .filter(|c| c.is_ascii_digit() || *c == '.')
            .collect();

        if let Ok(bpm) = digits.parse::<f32>() {
            if (40.0..=300.0).contains(&bpm) {
                return Some(bpm);
            }
        }
    }
    None
}

/// Estado persistente del explorador de archivos.
#[derive(Clone, Debug)]
pub struct FileExplorerState {
    pub current_path: PathBuf,
    pub path_input: String,
    pub show_hidden: bool,
    pub selected_file: Option<PathBuf>,
    pub history_back: Vec<PathBuf>,
    pub history_forward: Vec<PathBuf>,
    pub preview_volume: f32,
    pub preview_position: f32,
    pub is_playing_preview: bool,
    pub sample_bpm: f32,
    pub is_synced: bool,
    pub cached_path: Option<PathBuf>,
    pub cached_waveform: Vec<f32>,
    pub sample_duration_secs: f32,
    pub cached_entries: Vec<ExplorerEntry>,
    pub search_query: String,
    /// `0` = Samples & Files, `1` = Devices
    pub active_tab: u8,
}

impl Default for FileExplorerState {
    fn default() -> Self {
        let root = default_start_path();
        let mut st = Self {
            current_path: root.clone(),
            path_input: root.to_string_lossy().to_string(),
            show_hidden: false,
            selected_file: None,
            history_back: Vec::new(),
            history_forward: Vec::new(),
            preview_volume: 0.8,
            preview_position: 0.0,
            is_playing_preview: false,
            sample_bpm: 140.0,
            is_synced: false,
            cached_path: None,
            cached_waveform: Vec::new(),
            sample_duration_secs: 1.0,
            cached_entries: Vec::new(),
            search_query: String::new(),
            active_tab: 0,
        };
        st.refresh_entries();
        st
    }
}

impl FileExplorerState {
    pub fn refresh_entries(&mut self) {
        let mut dirs = Vec::new();
        let mut files = Vec::new();

        if let Ok(rd) = fs::read_dir(&self.current_path) {
            for entry in rd.flatten() {
                let file_name = entry.file_name();
                let name = file_name.to_string_lossy();
                if !self.show_hidden && name.starts_with('.') {
                    continue;
                }
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(ExplorerEntry {
                        name: name.to_string(),
                        path,
                        is_dir: true,
                        is_midi: false,
                    });
                } else if is_supported_file(&path) {
                    let is_midi = is_midi_file(&path);
                    files.push(ExplorerEntry {
                        name: name.to_string(),
                        path,
                        is_dir: false,
                        is_midi,
                    });
                }
            }
        }

        dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        dirs.extend(files);
        self.cached_entries = dirs;
    }

    pub fn navigate_to(&mut self, new_path: PathBuf) {
        if new_path.exists() && new_path.is_dir() {
            if self.current_path != new_path {
                self.history_back.push(self.current_path.clone());
                self.history_forward.clear();
                self.current_path = new_path.clone();
                self.path_input = new_path.to_string_lossy().to_string();
                self.refresh_entries();
            }
        }
    }

    pub fn go_back(&mut self) {
        if let Some(prev) = self.history_back.pop() {
            self.history_forward.push(self.current_path.clone());
            self.current_path = prev.clone();
            self.path_input = prev.to_string_lossy().to_string();
            self.refresh_entries();
        }
    }

    pub fn go_forward(&mut self) {
        if let Some(next) = self.history_forward.pop() {
            self.history_back.push(self.current_path.clone());
            self.current_path = next.clone();
            self.path_input = next.to_string_lossy().to_string();
            self.refresh_entries();
        }
    }

    pub fn go_up(&mut self) {
        if let Some(parent) = self.current_path.parent() {
            let parent_path = parent.to_path_buf();
            self.navigate_to(parent_path);
        }
    }

    pub fn go_home(&mut self) {
        let home = default_start_path();
        self.navigate_to(home);
    }

    pub fn toggle_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
        self.refresh_entries();
    }

    pub fn current_speed(&self, project_bpm: f32) -> f32 {
        if self.is_synced && self.sample_bpm > 0.0 {
            (project_bpm / self.sample_bpm).clamp(0.25, 4.0)
        } else {
            1.0
        }
    }

    pub fn load_waveform_peaks(&mut self, path: &PathBuf, target_bins: usize) {
        if is_midi_file(path) {
            self.cached_waveform.clear();
            self.cached_path = Some(path.clone());
            return;
        }

        if self.cached_path.as_ref() == Some(path) && !self.cached_waveform.is_empty() {
            return;
        }

        let mut peaks = vec![0.0_f32; target_bins];

        if let Ok(mut reader) = hound::WavReader::open(path) {
            let spec = reader.spec();
            let total_samples = reader.len() as usize;

            if spec.sample_rate > 0 && spec.channels > 0 {
                self.sample_duration_secs =
                    total_samples as f32 / (spec.sample_rate as f32 * spec.channels as f32);
            }

            if total_samples > 0 {
                let samples_per_bin = (total_samples / target_bins).max(1);
                let check_count = samples_per_bin.min(32);

                match spec.sample_format {
                    hound::SampleFormat::Int => {
                        let max_val = (1 << (spec.bits_per_sample.saturating_sub(1))) as f32;
                        for bin in 0..target_bins {
                            let seek_idx = (bin * samples_per_bin) as u32;
                            if reader.seek(seek_idx).is_err() {
                                break;
                            }
                            let mut max_peak = 0.0_f32;
                            let mut iter = reader.samples::<i32>().filter_map(|s| s.ok());
                            for _ in 0..check_count {
                                if let Some(s) = iter.next() {
                                    let abs_val = (s as f32 / max_val).abs();
                                    if abs_val > max_peak {
                                        max_peak = abs_val;
                                    }
                                } else {
                                    break;
                                }
                            }
                            peaks[bin] = max_peak;
                        }
                    }
                    hound::SampleFormat::Float => {
                        for bin in 0..target_bins {
                            let seek_idx = (bin * samples_per_bin) as u32;
                            if reader.seek(seek_idx).is_err() {
                                break;
                            }
                            let mut max_peak = 0.0_f32;
                            let mut iter = reader.samples::<f32>().filter_map(|s| s.ok());
                            for _ in 0..check_count {
                                if let Some(s) = iter.next() {
                                    let abs_val = s.abs();
                                    if abs_val > max_peak {
                                        max_peak = abs_val;
                                    }
                                } else {
                                    break;
                                }
                            }
                            peaks[bin] = max_peak;
                        }
                    }
                }
            }
        }

        self.cached_waveform = peaks;
        self.cached_path = Some(path.clone());
    }
}

pub fn render(ui: &mut Ui, state: &mut AppState) {
    let project_bpm = state.transport.bpm as f32;

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;

        // =====================================================================
        // 1. Tira superior de categorías estilo Bitwig Browser (Samples | Devices)
        // =====================================================================
        egui::Frame::NONE
            .fill(Color32::from_rgb(0x2A, 0x2C, 0x31))
            .inner_margin(egui::Margin::symmetric(6, 4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let is_files = state.explorer_state.active_tab == 0;
                    let is_devs = state.explorer_state.active_tab == 1;

                    if capsule_text_button(
                        ui,
                        vec2(108.0, 20.0),
                        "Samples & Files",
                        10.0,
                        is_files,
                        ACCENT_ORANGE,
                        JoinSide::Left,
                        false,
                    )
                    .clicked()
                    {
                        state.explorer_state.active_tab = 0;
                    }
                    if capsule_text_button(
                        ui,
                        vec2(78.0, 20.0),
                        "Devices",
                        10.0,
                        is_devs,
                        ACCENT_ORANGE,
                        JoinSide::Right,
                        false,
                    )
                    .clicked()
                    {
                        state.explorer_state.active_tab = 1;
                    }
                });
            });

        // Banner contextual cuando OpenWavetable espera un WAV
        if state.pending_wavetable_slot.is_some() {
            egui::Frame::NONE
                .fill(Color32::from_rgb(0x3A, 0x22, 0x08))
                .stroke(Stroke::new(1.0_f32, ACCENT_ORANGE))
                .corner_radius(3.0)
                .inner_margin(5.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (b_r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                        icons::paint_browser_badge(ui.painter(), b_r, BrowserBadgeKind::DeviceNote);
                        ui.label(
                            RichText::new("Select WAV to load into OpenWavetable")
                                .font(fonts::semibold(10.5))
                                .color(ACCENT_ORANGE),
                        );
                    });
                });
        }

        // =====================================================================
        // 2. Barra de búsqueda oscura hundida estilo Bitwig (`🔍 Search... ×`)
        // =====================================================================
        egui::Frame::NONE
            .fill(BW_DARK_WELL)
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0x1E, 0x20, 0x24)))
            .corner_radius(2.5)
            .inner_margin(egui::Margin::symmetric(5, 3))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let (srch_r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                    icons::paint_icon(
                        ui.painter(),
                        srch_r,
                        icons::SEARCH,
                        TEXT_MUTED,
                        1.7,
                    );
                    let edit_w = (ui.available_width() - 20.0).max(60.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut state.explorer_state.search_query)
                            .hint_text("Search devices & samples...")
                            .frame(false)
                            .desired_width(edit_w),
                    );
                    if !state.explorer_state.search_query.is_empty() {
                        let (clr_r, clr_resp) =
                            ui.allocate_exact_size(vec2(14.0, 14.0), Sense::click());
                        icons::paint_icon(
                            ui.painter(),
                            clr_r.shrink(1.5),
                            icons::X_CLOSE,
                            if clr_resp.hovered() {
                                Color32::WHITE
                            } else {
                                TEXT_MUTED
                            },
                            1.7,
                        );
                        if clr_resp.clicked() {
                            state.explorer_state.search_query.clear();
                        }
                    }
                });
            });

        // Si estamos en la pestaña `Devices`, mostramos el catálogo nativo estilo Bitwig Browser
        if state.explorer_state.active_tab == 1 {
            let query_low = state.explorer_state.search_query.to_lowercase();
            let list_h = (ui.available_height() - 8.0).max(120.0);
            egui::Frame::NONE
                .fill(BW_DARK_WELL)
                .inner_margin(egui::Margin::symmetric(4, 4))
                .show(ui, |ui| {
                    ScrollArea::vertical()
                        .id_salt("browser_device_list")
                        .max_height(list_h)
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            let row_w = ui.available_width();
                            for plugin in &PLUGIN_CATALOG {
                                if !query_low.is_empty()
                                    && !plugin.label.to_lowercase().contains(&query_low)
                                    && !plugin.category.to_lowercase().contains(&query_low)
                                {
                                    continue;
                                }
                                let (row_r, resp) =
                                    ui.allocate_exact_size(vec2(row_w, 22.0), Sense::click());
                                if resp.hovered() {
                                    ui.painter().rect_filled(
                                        row_r,
                                        2.0,
                                        Color32::from_rgb(0x26, 0x28, 0x2E),
                                    );
                                }
                                let badge_kind = if plugin.category.contains("Synth") {
                                    BrowserBadgeKind::DeviceNote
                                } else {
                                    BrowserBadgeKind::DeviceAudio
                                };
                                icons::paint_browser_badge(
                                    ui.painter(),
                                    Rect::from_min_size(
                                        pos2(row_r.min.x + 4.0, row_r.min.y + 3.0),
                                        vec2(16.0, 16.0),
                                    ),
                                    badge_kind,
                                );
                                ui.painter().text(
                                    pos2(row_r.min.x + 25.0, row_r.center().y),
                                    Align2::LEFT_CENTER,
                                    plugin.label,
                                    fonts::semibold(11.0),
                                    Color32::from_rgb(0xEC, 0xEE, 0xF2),
                                );
                                ui.painter().text(
                                    pos2(row_r.max.x - 6.0, row_r.center().y),
                                    Align2::RIGHT_CENTER,
                                    plugin.category,
                                    fonts::regular(9.5),
                                    TEXT_MUTED,
                                );
                                if resp
                                    .on_hover_text("Click to insert device on selected track")
                                    .clicked()
                                {
                                    let t_idx = state.safe_track_index();
                                    if let Some(tr) = state.current_tracks_mut().get_mut(t_idx) {
                                        let mut slot = DspSlot::default();
                                        slot.name = plugin.name.to_string();
                                        tr.effects.push(slot);
                                        state.selected_slot_index = tr.effects.len() - 1;
                                        state.show_dsp_rack = true;
                                    }
                                }
                            }
                        });
                });
            return;
        }

        // =====================================================================
        // 3. Barra de navegación de carpetas con cápsulas vectoriales GPU
        // =====================================================================
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;

            if capsule_icon_button(
                ui,
                vec2(24.0, 20.0),
                false,
                ACCENT_ORANGE,
                JoinSide::Left,
                |p, r, fg| icons::paint_icon(p, r.shrink(1.0), icons::ARROW_LEFT, fg, 1.7),
            )
            .on_hover_text("Back")
            .clicked()
            {
                state.explorer_state.go_back();
            }

            if capsule_icon_button(
                ui,
                vec2(24.0, 20.0),
                false,
                ACCENT_ORANGE,
                JoinSide::Middle,
                |p, r, fg| icons::paint_icon(p, r.shrink(1.0), icons::ARROW_RIGHT, fg, 1.7),
            )
            .on_hover_text("Forward")
            .clicked()
            {
                state.explorer_state.go_forward();
            }

            if capsule_icon_button(
                ui,
                vec2(24.0, 20.0),
                false,
                ACCENT_ORANGE,
                JoinSide::Middle,
                |p, r, fg| icons::paint_icon(p, r.shrink(1.0), icons::ARROW_UP, fg, 1.7),
            )
            .on_hover_text("Parent Folder")
            .clicked()
            {
                state.explorer_state.go_up();
            }

            if capsule_icon_button(
                ui,
                vec2(24.0, 20.0),
                false,
                ACCENT_ORANGE,
                JoinSide::Middle,
                |p, r, fg| icons::paint_icon(p, r.shrink(1.0), icons::HOME, fg, 1.7),
            )
            .on_hover_text("Home Directory")
            .clicked()
            {
                state.explorer_state.go_home();
            }

            if capsule_icon_button(
                ui,
                vec2(24.0, 20.0),
                false,
                ACCENT_ORANGE,
                JoinSide::Right,
                |p, r, fg| icons::paint_icon(p, r.shrink(1.0), icons::REFRESH, fg, 1.7),
            )
            .on_hover_text("Refresh")
            .clicked()
            {
                state.explorer_state.refresh_entries();
            }

            ui.add_space(4.0);
            if capsule_icon_button(
                ui,
                vec2(24.0, 20.0),
                state.explorer_state.show_hidden,
                ACCENT_ORANGE,
                JoinSide::Standalone,
                |p, r, fg| icons::paint_icon(p, r.shrink(1.0), icons::EYE, fg, 1.7),
            )
            .on_hover_text("Show Hidden Files")
            .clicked()
            {
                state.explorer_state.toggle_hidden();
            }
        });

        // 4. Ruta actual editable
        let mut path_str = state.explorer_state.path_input.clone();
        if ui
            .add(
                egui::TextEdit::singleline(&mut path_str)
                    .font(fonts::regular(10.0))
                    .desired_width(ui.available_width()),
            )
            .lost_focus()
        {
            state.explorer_state.navigate_to(PathBuf::from(path_str));
        }

        // =====================================================================
        // 5. Lista negra profunda `#0A0A0C` con insignias coloreadas Bitwig
        // =====================================================================
        let entries = state.explorer_state.cached_entries.clone();
        let query_low = state.explorer_state.search_query.to_lowercase();
        let mut clicked_path = None;
        let mut double_clicked_path = None;
        let mut is_dir_click = false;

        let list_h = (ui.available_height() - 94.0).max(110.0);
        egui::Frame::NONE
            .fill(BW_DARK_WELL)
            .inner_margin(egui::Margin::symmetric(3, 3))
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .id_salt("browser_file_list")
                    .max_height(list_h)
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        let row_w = ui.available_width();

                        for entry in &entries {
                            if !query_low.is_empty()
                                && !entry.name.to_lowercase().contains(&query_low)
                            {
                                continue;
                            }

                            let is_sel =
                                state.explorer_state.selected_file.as_ref() == Some(&entry.path);
                            let (badge_kind, text_col) = if entry.is_dir {
                                (
                                    BrowserBadgeKind::Folder,
                                    Color32::from_rgb(0xE6, 0xE8, 0xEE),
                                )
                            } else if entry.is_midi {
                                (
                                    BrowserBadgeKind::DeviceNote,
                                    Color32::from_rgb(0xB8, 0xD8, 0xF8),
                                )
                            } else {
                                (
                                    BrowserBadgeKind::Audio,
                                    Color32::from_rgb(0xF0, 0xF2, 0xF6),
                                )
                            };

                            let (row_rect, resp) =
                                ui.allocate_exact_size(vec2(row_w, 20.0), Sense::click_and_drag());

                            if ui.is_rect_visible(row_rect) {
                                if is_sel {
                                    ui.painter().rect_filled(
                                        row_rect,
                                        2.0,
                                        Color32::from_rgb(0x46, 0x29, 0x10),
                                    );
                                    ui.painter().rect_stroke(
                                        row_rect,
                                        2.0,
                                        Stroke::new(1.0_f32, ACCENT_ORANGE),
                                        egui::StrokeKind::Inside,
                                    );
                                } else if resp.hovered() {
                                    ui.painter().rect_filled(
                                        row_rect,
                                        2.0,
                                        Color32::from_rgb(0x20, 0x22, 0x28),
                                    );
                                }

                                icons::paint_browser_badge(
                                    ui.painter(),
                                    Rect::from_min_size(
                                        pos2(row_rect.min.x + 4.0, row_rect.min.y + 2.0),
                                        vec2(16.0, 16.0),
                                    ),
                                    badge_kind,
                                );

                                ui.painter().text(
                                    pos2(row_rect.min.x + 24.0, row_rect.center().y),
                                    Align2::LEFT_CENTER,
                                    &entry.name,
                                    fonts::regular(11.0),
                                    if is_sel { Color32::WHITE } else { text_col },
                                );
                            }

                            if resp.clicked() {
                                clicked_path = Some(entry.path.clone());
                                is_dir_click = entry.is_dir;
                            }
                            if resp.double_clicked() && !entry.is_dir {
                                double_clicked_path = Some(entry.path.clone());
                            }
                            if (resp.drag_started() || resp.dragged()) && !entry.is_dir {
                                mark_dragged_sample(state, &entry.path);
                            }
                        }
                    });
            });

        // =====================================================================
        // 6. Reproductor inferior de preescucha Bitwig (Waveform + GPU Fader)
        // =====================================================================
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.5, BW_DARK_WELL);
        painter.rect_stroke(
            rect,
            2.5,
            Stroke::new(1.0_f32, Color32::from_rgb(0x22, 0x24, 0x2A)),
            egui::StrokeKind::Inside,
        );

        let waveform = &state.explorer_state.cached_waveform;
        if !waveform.is_empty() {
            let width = rect.width();
            let height = rect.height();
            let center_y = rect.center().y;
            let max_h = height * 0.42;
            let n = waveform.len();
            let bar_w = (width / n as f32).max(1.0);

            for (i, &amp) in waveform.iter().enumerate() {
                let x = rect.left() + (i as f32 / n as f32) * width;
                let h = (amp * max_h).max(1.0);
                painter.rect_filled(
                    Rect::from_min_max(pos2(x, center_y - h), pos2(x + bar_w, center_y + h)),
                    0.0,
                    ACCENT_ORANGE,
                );
            }

            let phx = rect.left() + state.explorer_state.preview_position * width;
            painter.line_segment(
                [pos2(phx, rect.top()), pos2(phx, rect.bottom())],
                Stroke::new(1.4_f32, BW_LCD_CYAN),
            );
        } else {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Drag WAV onto Clip Launcher or Timeline",
                fonts::regular(10.0),
                TEXT_MUTED,
            );
        }

        if response.clicked() || response.dragged() {
            if let Some(pos) = response.interact_pointer_pos() {
                let norm_x = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                state.explorer_state.preview_position = norm_x;
                state.explorer_state.is_playing_preview = true;

                if let Some(ref file) = state.explorer_state.selected_file {
                    state
                        .audio_proxy
                        .send(GuiCommand::SetPreviewVolume(state.explorer_state.preview_volume));
                    state.audio_proxy.send(GuiCommand::PreviewSample {
                        path: file.to_string_lossy().to_string(),
                        volume: state.explorer_state.preview_volume,
                        speed: state.explorer_state.current_speed(project_bpm),
                    });
                }
            }
        }

        // Controles de preescucha con botón vectorial GPU + fader horizontal GPU
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let is_prev = state.explorer_state.is_playing_preview;
            if capsule_icon_button(
                ui,
                vec2(24.0, 19.0),
                is_prev,
                PLAY_GREEN,
                JoinSide::Standalone,
                |p, r, fg| {
                    if is_prev {
                        icons::paint_stop_square(p, r, fg);
                    } else {
                        icons::paint_play_triangle(p, r, fg);
                    }
                },
            )
            .on_hover_text("Play / Stop Sample Preview")
            .clicked()
            {
                if is_prev {
                    state.explorer_state.is_playing_preview = false;
                    state.audio_proxy.send(GuiCommand::StopPreview);
                } else if let Some(ref file) = state.explorer_state.selected_file {
                    state.explorer_state.is_playing_preview = true;
                    state
                        .audio_proxy
                        .send(GuiCommand::SetPreviewVolume(state.explorer_state.preview_volume));
                    state.audio_proxy.send(GuiCommand::PreviewSample {
                        path: file.to_string_lossy().to_string(),
                        volume: state.explorer_state.preview_volume,
                        speed: state.explorer_state.current_speed(project_bpm),
                    });
                }
            }

            let (vol_icon_rect, _) = ui.allocate_exact_size(vec2(13.0, 13.0), Sense::hover());
            icons::paint_icon(
                ui.painter(),
                vol_icon_rect,
                icons::VOLUME,
                TEXT_SECONDARY,
                1.6,
            );

            let fader_w = (ui.available_width() - 52.0).max(60.0);
            if fader_horizontal_range(
                ui,
                vec2(fader_w, 12.0),
                &mut state.explorer_state.preview_volume,
                0.0..=1.0,
                0.8,
                BW_FADER_AMBER,
                BW_FADER_ORANGE,
                false,
            )
            .1
            {
                state
                    .audio_proxy
                    .send(GuiCommand::SetPreviewVolume(state.explorer_state.preview_volume));
            }

            if capsule_text_button(
                ui,
                vec2(40.0, 19.0),
                "Sync",
                9.5,
                state.explorer_state.is_synced,
                ACCENT_ORANGE,
                JoinSide::Standalone,
                false,
            )
            .clicked()
            {
                state.explorer_state.is_synced = !state.explorer_state.is_synced;
            }
        });

        if let Some(path) = double_clicked_path {
            let (t_idx, s_idx) = state.matrix_state.selected_slot.unwrap_or((0, 0));
            let safe_t = t_idx.min(state.matrix_state.tracks.len().saturating_sub(1));
            let safe_s = s_idx.min(state.matrix_state.scenes.len().saturating_sub(1));
            crate::views::matrix::load_clip_into_slot(
                &mut state.matrix_state,
                &state.audio_proxy,
                safe_t,
                safe_s,
                path,
                state.transport.bpm,
            );
        } else if let Some(path) = clicked_path {
            if is_dir_click {
                state.explorer_state.navigate_to(path);
            } else if let Some((track_idx, slot_idx)) = state.pending_wavetable_slot.take() {
                crate::views::hikaru_native_plugins::hikaru_openwavetable::open_wavetable::load_wavetable_from_path(
                    state,
                    track_idx,
                    slot_idx,
                    &path,
                );
            } else {
                state.explorer_state.selected_file = Some(path.clone());
                state.explorer_state.load_waveform_peaks(&path, 128);

                if let Some(bpm) = parse_bpm_from_filename(
                    &path.file_name().unwrap_or_default().to_string_lossy(),
                ) {
                    state.explorer_state.sample_bpm = bpm;
                }

                state.explorer_state.is_playing_preview = true;
                state
                    .audio_proxy
                    .send(GuiCommand::SetPreviewVolume(state.explorer_state.preview_volume));
                state.audio_proxy.send(GuiCommand::PreviewSample {
                    path: path.to_string_lossy().to_string(),
                    volume: state.explorer_state.preview_volume,
                    speed: state.explorer_state.current_speed(project_bpm),
                });
            }
        }
    });
}
