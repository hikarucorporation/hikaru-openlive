// Hikaru OpenLive - Explorer
// GNU AGPLv3
// crates/hikaru_gui/src/views/explorer.rs

use std::fs;
use std::path::PathBuf;

use gpui_kit::component::button::Button;
use gpui_kit::component::label::Label;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, HikaruApp};
use crate::audio_proxy::GuiCommand;

pub struct FileExplorerState {
    pub current_path: PathBuf,
    pub path_input: String,
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
}

impl Default for FileExplorerState {
    fn default() -> Self {
        let root = PathBuf::from("/");
        Self {
            current_path: root.clone(),
            path_input: root.to_string_lossy().to_string(),
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
        }
    }
}

impl FileExplorerState {
    pub fn navigate_to(&mut self, new_path: PathBuf) {
        if new_path.exists() && new_path.is_dir() {
            self.history_back.push(self.current_path.clone());
            self.history_forward.clear();
            self.current_path = new_path.clone();
            self.path_input = new_path.to_string_lossy().to_string();
        }
    }

    pub fn go_back(&mut self) {
        if let Some(prev) = self.history_back.pop() {
            self.history_forward.push(self.current_path.clone());
            self.current_path = prev.clone();
            self.path_input = prev.to_string_lossy().to_string();
        }
    }

    pub fn go_forward(&mut self) {
        if let Some(next) = self.history_forward.pop() {
            self.history_back.push(self.current_path.clone());
            self.current_path = next.clone();
            self.path_input = next.to_string_lossy().to_string();
        }
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
            return;
        }

        let mut peaks = vec![0.0_f32; target_bins];

        if let Ok(mut reader) = hound::WavReader::open(path) {
            let spec = reader.spec();
            let total_samples = reader.len() as usize;

            if spec.sample_rate > 0 {
                self.sample_duration_secs =
                    total_samples as f32 / (spec.sample_rate as f32 * spec.channels as f32);
            }

            if total_samples > 0 {
                let samples_per_bin = (total_samples / target_bins).max(1);

                match spec.sample_format {
                    hound::SampleFormat::Int => {
                        let max_val = (1 << (spec.bits_per_sample - 1)) as f32;
                        let mut samples_iter = reader.samples::<i32>().filter_map(|s| s.ok());

                        for bin in 0..target_bins {
                            let mut max_peak = 0.0_f32;
                            for _ in 0..samples_per_bin {
                                if let Some(s) = samples_iter.next() {
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
                        let mut samples_iter = reader.samples::<f32>().filter_map(|s| s.ok());

                        for bin in 0..target_bins {
                            let mut max_peak = 0.0_f32;
                            for _ in 0..samples_per_bin {
                                if let Some(s) = samples_iter.next() {
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

/// Abre el explorador en modo "elegir una wavetable para este slot".
///
/// El editor de Wavetable llama a esto en vez de armar su propio navegador: el
/// explorer ya recorre los directorios de Linux con historial, barra de ruta y
/// atajo a la raíz, y duplicar todo eso para elegir un archivo sería mantener
/// dos navegadores que se desincronizan.
///
/// El modo se guarda como un slot pendiente en el estado: el click sobre un
/// archivo consulta esa bandera y, si está, carga la wavetable en vez de armar
/// un clip. Un flag y no un closure, porque el click se resuelve en otra vista.
pub fn begin_wavetable_pick(cx: &mut App, track_idx: usize, slot_idx: usize) {
    state(cx).update(cx, |state, cx| {
        state.pending_wavetable_slot = Some((track_idx, slot_idx));
        state.show_explorer = true;
        cx.notify();
    });
}

/// Directorio en el que arranca el explorador.
///
/// El home del usuario, no `/`: la raíz del filesystem es el peor lugar para
/// buscar una wavetable, y desde ahí hay que navegar hacia abajo en cada
/// intento. Si el home no existe (contenedor, servicio), se cae a la raíz.
pub fn default_start_path() -> PathBuf {
    match std::env::var("HOME") {
        Ok(home) if PathBuf::from(&home).is_dir() => PathBuf::from(home),
        _ => PathBuf::from("/"),
    }
}

pub fn render(cx: &mut Context<HikaruApp>) -> AnyElement {
    let app = state(cx).read(cx);
    let ex = &app.explorer_state;
    let current_path = ex.current_path.clone();
    let path_input = ex.path_input.clone();
    let selected_file = ex.selected_file.clone();
    let preview_volume = ex.preview_volume;
    let preview_position = ex.preview_position;
    let is_playing = ex.is_playing_preview;
    let sample_bpm = ex.sample_bpm;
    let is_synced = ex.is_synced;
    let cached_waveform = ex.cached_waveform.clone();
    let audio_proxy = app.audio_proxy.clone();
    let project_bpm = app.transport.bpm as f32;
    drop(app);

    let mut entries: Vec<(String, PathBuf, bool)> = Vec::new();
    if let Ok(rd) = fs::read_dir(&current_path) {
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if is_supported_file(&path) {
                files.push(path);
            }
        }
        dirs.sort();
        files.sort();
        for d in dirs {
            let name = d.file_name().unwrap_or_default().to_string_lossy().to_string();
            entries.push((format!("📁 {}", name), d, true));
        }
        for f in files {
            let name = f.file_name().unwrap_or_default().to_string_lossy().to_string();
            let is_midi = is_midi_file(&f);
            let icon = if is_midi { "🎹" } else { "🎵" };
            entries.push((format!("{} {}", icon, name), f, false));
        }
    }

    let selected_name = selected_file
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let wave_canvas = div()
        .id("explorer_wave_canvas")
        .w_full()
        .h(px(72.0))
        .bg(crate::theme::SURFACE_BG)
        .rounded(px(4.0))
        .border_1()
        .border_color(crate::theme::BORDER_COLOR)
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let bg = crate::theme::SURFACE_BG;
                    window.paint_quad(PaintQuad {
                        bounds,
                        background: bg.into(),
                        border_color: Hsla::default(),
                        corner_radii: gpui_kit::Corners::default(),
                        border_widths: gpui_kit::Edges::default(),
                        border_style: BorderStyle::default(),
                    });
                    if cached_waveform.is_empty() {
                        return;
                    }
                    let origin_x: f32 = bounds.origin.x.into();
                    let origin_y: f32 = bounds.origin.y.into();
                    let height_f: f32 = bounds.size.height.into();
                    let center_y = origin_y + height_f / 2.0;
                    let max_h = height_f * 0.42;
                    let n = cached_waveform.len();
                    let w: f32 = bounds.size.width.into();
                    let bar_w = (w / n as f32).max(1.0);
                    for (i, &amp) in cached_waveform.iter().enumerate() {
                        let x = origin_x + (i as f32 / n as f32) * w;
                        let h = (amp * max_h).max(1.0);
                        let mut pb = PathBuilder::fill();
                        pb.move_to(point(px(x), px(center_y - h)));
                        pb.line_to(point(px(x + bar_w), px(center_y - h)));
                        pb.line_to(point(px(x + bar_w), px(center_y + h)));
                        pb.line_to(point(px(x), px(center_y + h)));
                        pb.close();
                        window.paint_path(pb.build().unwrap(), crate::theme::ACCENT_COLOR);
                    }
                    let phx = origin_x + preview_position * w;
                    let mut pb = PathBuilder::stroke(px(2.0));
                    pb.move_to(point(px(phx), px(origin_y)));
                    pb.line_to(point(px(phx), px(origin_y + height_f)));
                    window.paint_path(pb.build().unwrap(), crate::theme::ACCENT_STUDIO);
                },
            )
            .w_full()
            .h_full(),
        )
        .on_click(move |event, _, cx| {
            let Some(pos) = event.mouse_position() else { return; };
            let norm_x: f32 = (pos.x.as_f32() / 260.0).clamp(0.0, 1.0);
            let st = state(cx);
            cx.update_entity(&st, |state, cx| {
                state.explorer_state.preview_position = norm_x;
                state.explorer_state.is_playing_preview = true;
                if let Some(ref file) = state.explorer_state.selected_file {
                    state.audio_proxy.send(GuiCommand::SetPreviewVolume(
                        state.explorer_state.preview_volume,
                    ));
                    state.audio_proxy.send(GuiCommand::PreviewSample {
                        path: file.to_string_lossy().to_string(),
                        volume: state.explorer_state.preview_volume,
                        speed: state.explorer_state.current_speed(project_bpm),
                    });
                }
                cx.notify();
            });
        })
        .into_any_element();

    v_flex()
        .id("file_explorer")
        .size_full()
        .bg(crate::theme::PANEL_BG)
        .p(px(8.0))
        .gap(px(6.0))
        // 1. Navegación y Ruta Actual
        .child(
            h_flex()
                .gap(px(4.0))
                .child(
                    Button::new("explorer_back").rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("⮜")
                        .compact()
                        .text_color(rgb(0xB0B0B0))
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            cx.update_entity(&st, |state, cx| {
                                state.explorer_state.go_back();
                                cx.notify();
                            });
                        }),
                )
                .child(
                    Button::new("explorer_forward").rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("⮞")
                        .compact()
                        .text_color(rgb(0xB0B0B0))
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            cx.update_entity(&st, |state, cx| {
                                state.explorer_state.go_forward();
                                cx.notify();
                            });
                        }),
                )
                .child(
                    Button::new("explorer_up").rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("⬆")
                        .compact()
                        .text_color(rgb(0xB0B0B0))
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            cx.update_entity(&st, |state, cx| {
                                if let Some(parent) = state.explorer_state.current_path.parent() {
                                    state.explorer_state.navigate_to(parent.to_path_buf());
                                }
                                cx.notify();
                            });
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .h(px(24.0))
                        .px(px(6.0))
                        .flex()
                        .items_center()
                        .bg(crate::theme::SURFACE_BG)
                        .rounded(px(3.0))
                        .border_1()
                        .border_color(crate::theme::BORDER_COLOR)
                        .child(
                            Label::new(path_input.clone())
                                .text_xs()
                                .text_color(crate::theme::TEXT_PRIMARY),
                        ),
                ),
        )
        // 2. Lista de Archivos / Carpetas con Scroll
        .child(
            div()
                .flex_1()
                .w_full()
                .overflow_y_scrollbar()
                .child(
                    v_flex()
                        .gap(px(2.0))
                        .children(entries.into_iter().map(|(label, path, is_dir)| {
                            let is_selected = selected_file.as_ref() == Some(&path);
                            let label_clone = label.clone();
                            let project_bpm = project_bpm;
                            
                            let bg_color = if is_selected {
                                crate::theme::SLOT_ACTIVE_BG
                            } else {
                                crate::theme::SURFACE_BG
                            };

                            div()
                                .id(format!("explorer_entry_{}", path.display()))
                                .w_full()
                                .h(px(22.0))
                                .px(px(6.0))
                                .flex()
                                .items_center()
                                .bg(bg_color)
                                .rounded(px(3.0))
                                .child(Label::new(label_clone).text_xs().text_color(crate::theme::TEXT_PRIMARY))
                                .on_click(move |_, _, cx| {
                                    let st = state(cx);
                                    cx.update_entity(&st, |state, cx| {
                                        if is_dir {
                                            state.explorer_state.navigate_to(path.clone());
                                        } else if let Some((track_idx, slot_idx)) =
                                            state.pending_wavetable_slot
                                        {
                                            // Modo "elegir wavetable": el archivo
                                            // va al slot del rack y no al proyecto,
                                            // y el modo se cierra para que el
                                            // siguiente click sea el normal.
                                            state.pending_wavetable_slot = None;
                                            let wavetable_path = path.clone();
                                            let st = st.clone();
                                            cx.update_entity(&st, |state, cx| {
                                                crate::views::open_wavetable::load_wavetable_from_path(
                                                    cx,
                                                    track_idx,
                                                    slot_idx,
                                                    &wavetable_path,
                                                );
                                            });
                                        } else {
                                            state.explorer_state.selected_file = Some(path.clone());
                                            if !is_midi_file(&path) {
                                                state.explorer_state.is_playing_preview = true;
                                                state.explorer_state.preview_position = 0.0;
                                                state.explorer_state.load_waveform_peaks(&path, 256);
                                                if let Some(detected) =
                                                    parse_bpm_from_filename(&path.to_string_lossy())
                                                {
                                                    state.explorer_state.sample_bpm = detected;
                                                }
                                                state.audio_proxy.send(GuiCommand::SetPreviewVolume(
                                                    state.explorer_state.preview_volume,
                                                ));
                                                state.audio_proxy.send(GuiCommand::PreviewSample {
                                                    path: path.to_string_lossy().to_string(),
                                                    volume: state.explorer_state.preview_volume,
                                                    speed: state
                                                        .explorer_state
                                                        .current_speed(project_bpm),
                                                });
                                            } else {
                                                state.explorer_state.is_playing_preview = false;
                                                state.audio_proxy.send(GuiCommand::StopPreview);
                                            }
                                        }
                                        cx.notify();
                                    });
                                })
                                .into_any_element()
                        })),
                ),
        )
        // 3. Controles de Reproducción y Preescucha (BPM / Sync / Vol)
        .child(
            h_flex()
                .items_center()
                .gap(px(6.0))
                .child(
                    Button::new("explorer_play").rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label(if is_playing { "⏸" } else { "▶" })
                        .compact()
                        .text_color(rgb(0xB0B0B0))
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            cx.update_entity(&st, |state, cx| {
                                state.explorer_state.is_playing_preview =
                                    !state.explorer_state.is_playing_preview;
                                if state.explorer_state.is_playing_preview {
                                    if let Some(ref file) = state.explorer_state.selected_file {
                                        state.audio_proxy.send(GuiCommand::SetPreviewVolume(
                                            state.explorer_state.preview_volume,
                                        ));
                                        state.audio_proxy.send(GuiCommand::PreviewSample {
                                            path: file.to_string_lossy().to_string(),
                                            volume: state.explorer_state.preview_volume,
                                            speed: state
                                                .explorer_state
                                                .current_speed(project_bpm),
                                        });
                                    }
                                } else {
                                    state.audio_proxy.send(GuiCommand::StopPreview);
                                }
                                cx.notify();
                            });
                        }),
                )
                .child(Label::new("🔊").text_xs().text_color(crate::theme::TEXT_PRIMARY))
                .child(
                    div()
                        .w(px(60.0))
                        .h(px(10.0))
                        .bg(crate::theme::SURFACE_BG)
                        .rounded(px(3.0))
                        .on_mouse_down(gpui_kit::MouseButton::Left, move |event, _, cx| {
                            let x: f32 = event.position.x.as_f32() / 60.0;
                            let st = state(cx);
                            cx.update_entity(&st, |state, cx| {
                                state.explorer_state.preview_volume = x.clamp(0.0, 1.0);
                                state.audio_proxy.send(GuiCommand::SetPreviewVolume(
                                    state.explorer_state.preview_volume,
                                ));
                                cx.notify();
                            });
                        })
                        .into_any_element(),
                )
                .child(
                    Button::new("explorer_sync").rounded(gpui_kit::component::button::ButtonRounded::None)
                        .label("Sync")
                        .compact()
                        .text_color(rgb(0xB0B0B0))
                        .when(is_synced, |b| b.text_color(crate::theme::ACCENT_COLOR))
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            cx.update_entity(&st, |state, cx| {
                                state.explorer_state.is_synced = !state.explorer_state.is_synced;
                                cx.notify();
                            });
                        }),
                )
                .child(Label::new(format!("{} BPM", sample_bpm as u32)).text_xs().text_color(crate::theme::TEXT_PRIMARY)),
        )
        // 4. Visualización de Formas de Onda (Waveform)
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .w_full()
                        .flex()
                        .justify_center()
                        .child(
                            Label::new(selected_name)
                                .text_xs()
                                .text_color(crate::theme::TEXT_PRIMARY),
                        ),
                )
                .child(wave_canvas),
        )
        .into_any_element()
}

pub fn is_supported_file(path: &PathBuf) -> bool {
    is_audio_file(path) || is_midi_file(path)
}

pub fn is_audio_file(path: &PathBuf) -> bool {
    if let Some(ext) = path.extension() {
        let ext_str = ext.to_string_lossy().to_lowercase();
        matches!(ext_str.as_str(), "wav" | "flac" | "ogg" | "mp3" | "aiff" | "synth")
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