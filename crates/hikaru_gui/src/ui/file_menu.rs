// Hikaru OpenLive - Bitwig Studio 5.x style FILE dropdown menu
// GNU AGPLv3
// crates/hikaru_gui/src/ui/file_menu.rs
//
// Menú desplegable de [FILE] pintado a mano para reproducir 1:1 el layout de
// Bitwig Studio 5.x con todas las funciones desbloqueadas y operativas en
// Hikaru OpenLive:
//
//   New Project              Ctrl+N        📌
//   Open...                  Ctrl+O        📌
//   Save                     Ctrl+S        📌
//   Collect and Save...                    📌
//   Save as...               Ctrl+Shift+S  📌
//   Save as Template...                    📌
//   Close                    Ctrl+W        📌
//   ─────────────────────────────────────────
//   Export Audio...          Ctrl+Shift+B  📌
//   Export MIDI...                         📌
//   Export DAWproject...                   📌
//   ─────────────────────────────────────────
//   Show Dashboard                         📌
//   Settings                 Ctrl+,        📌
//   Quit                     Ctrl+Q        📌

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use egui::{
    pos2, vec2, Align2, Color32, Context, CursorIcon, Id, Order, Pos2, Rect, Sense, Stroke, Ui,
};

use crate::app::{AppMode, AppState};
use crate::project;
use crate::ui::bitwig_kit::BW_ACCENT_ORANGE;
use crate::ui::fonts;
use crate::ui::icons::{self, VectorIcon};
use crate::views::control_panel::{DashboardSubTab, DashboardTopTab};
use crate::views::matrix::ClipData;
use crate::views::menu_bar;
use crate::views::playlist::ClipType;
use crate::wav_io;

const MENU_W: f32 = 248.0;
const PAD_X: f32 = 8.0;
const PAD_Y: f32 = 4.0;
const ROW_H: f32 = 26.0;
const SEP_H: f32 = 11.0;

const MENU_BG: Color32 = Color32::from_rgb(0x2D, 0x2D, 0x2F);
const MENU_BORDER: Color32 = Color32::from_rgb(0x18, 0x18, 0x1A);
const SEP: Color32 = Color32::from_rgb(0x46, 0x46, 0x49);
const ROW_HOVER: Color32 = Color32::from_rgb(0xBE, 0xBE, 0xBE);
const TXT: Color32 = Color32::from_rgb(0xEA, 0xEA, 0xEA);
const TXT_HINT: Color32 = Color32::from_rgb(0x98, 0x98, 0x98);
const TXT_ON_HOVER: Color32 = Color32::from_rgb(0x1A, 0x1A, 0x1A);
const PIN_IDLE: Color32 = Color32::from_rgb(0x9A, 0x9A, 0xA0);

const fn vi(d: &'static str) -> VectorIcon {
    VectorIcon { filled: false, d }
}

// Iconos 24x24 calibrados al menú FILE de Bitwig Studio
const IC_NEW_PROJECT: VectorIcon = icons::FILE_PAGE;
const IC_OPEN: VectorIcon = icons::FOLDER;
const IC_SAVE: VectorIcon = vi(
    "M 3 12 L 10 12 M 7 9 L 10 12 L 7 15 M 12 4 L 17 4 L 20 7 L 20 20 L 12 20 Z M 16 4 L 16 8 L 20 8",
);
const IC_COLLECT_SAVE: VectorIcon = vi(
    "M 3 12 L 8 12 M 6 9.5 L 8.5 12 L 6 14.5 M 10 6 L 14 6 L 16 8 L 21 8 L 21 19 L 10 19 Z",
);
const IC_SAVE_AS: VectorIcon = vi(
    "M 3 12 L 9 12 M 6.5 9.5 L 9 12 L 6.5 14.5 M 11 4 L 17 4 L 20 7 L 20 20 L 11 20 Z M 14 13 L 17 13 M 15.5 11.5 L 15.5 14.5",
);
const IC_SAVE_TEMPLATE: VectorIcon = vi(
    "M 3 12 L 9 12 M 6.5 9.5 L 9 12 L 6.5 14.5 M 11 4 L 20 4 L 20 20 L 11 20 Z M 13.5 9 L 17.5 9 M 13.5 13 L 17.5 13 M 13.5 17 L 16.5 17",
);
const IC_CLOSE_DOC: VectorIcon = vi(
    "M 6 3 L 15 3 L 19 7 L 19 21 L 6 21 Z M 15 3 L 15 7 L 19 7 M 10 11 L 15 16 M 15 11 L 10 16",
);
const IC_EXPORT_AUDIO: VectorIcon = vi(
    "M 3 12 L 6 12 L 8 6 L 10.5 18 L 13 4 L 15.5 20 L 18 12 L 21 12",
);
const IC_EXPORT_MIDI: VectorIcon = vi(
    "M 3 12 C 3 16.971 7.029 21 12 21 C 16.971 21 21 16.971 21 12 C 21 7.029 16.971 3 12 3 C 7.029 3 3 7.029 3 12 M 7.5 12 L 7.6 12 M 9 8.5 L 9.1 8.5 M 12 7.5 L 12.1 7.5 M 15 8.5 L 15.1 8.5 M 16.5 12 L 16.6 12",
);
const IC_EXPORT_DAWPROJECT: VectorIcon = vi(
    "M 4 5 L 13 5 L 13 7 M 4 5 L 4 19 L 19 19 L 19 11 M 11 13 L 20 4 M 15 4 L 20 4 L 20 9",
);
const IC_PIN: VectorIcon = vi(
    "M 15 4 L 20 9 M 16 5 L 11 10 L 8 10 L 14 16 L 14 13 L 19 8 M 11 13 L 5 19",
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileMenuAction {
    NewProject,
    NewFromTemplate,
    Open,
    Save,
    CollectAndSave,
    SaveAs,
    SaveAsTemplate,
    Close,
    ExportAudio,
    ExportMidi,
    ExportDawProject,
    ExportBwProject,
    ShowDashboard,
    Settings,
    Quit,
}

#[derive(Clone, Copy)]
enum RowIcon {
    Vector(VectorIcon),
    DashboardEmblem,
}

struct MenuRow {
    label: &'static str,
    shortcut: &'static str,
    icon: RowIcon,
    action: FileMenuAction,
}

const SECTION_PROJECT: [MenuRow; 8] = [
    MenuRow {
        label: "New Project",
        shortcut: "Ctrl+N",
        icon: RowIcon::Vector(IC_NEW_PROJECT),
        action: FileMenuAction::NewProject,
    },
    MenuRow {
        label: "New from Template (.bwtemplate)...",
        shortcut: "",
        icon: RowIcon::Vector(IC_SAVE_TEMPLATE),
        action: FileMenuAction::NewFromTemplate,
    },
    MenuRow {
        label: "Open...",
        shortcut: "Ctrl+O",
        icon: RowIcon::Vector(IC_OPEN),
        action: FileMenuAction::Open,
    },
    MenuRow {
        label: "Save",
        shortcut: "Ctrl+S",
        icon: RowIcon::Vector(IC_SAVE),
        action: FileMenuAction::Save,
    },
    MenuRow {
        label: "Collect and Save...",
        shortcut: "",
        icon: RowIcon::Vector(IC_COLLECT_SAVE),
        action: FileMenuAction::CollectAndSave,
    },
    MenuRow {
        label: "Save as...",
        shortcut: "Ctrl+Shift+S",
        icon: RowIcon::Vector(IC_SAVE_AS),
        action: FileMenuAction::SaveAs,
    },
    MenuRow {
        label: "Save as Template...",
        shortcut: "",
        icon: RowIcon::Vector(IC_SAVE_TEMPLATE),
        action: FileMenuAction::SaveAsTemplate,
    },
    MenuRow {
        label: "Close",
        shortcut: "Ctrl+W",
        icon: RowIcon::Vector(IC_CLOSE_DOC),
        action: FileMenuAction::Close,
    },
];

const SECTION_EXPORT: [MenuRow; 4] = [
    MenuRow {
        label: "Export Audio...",
        shortcut: "Ctrl+Shift+B",
        icon: RowIcon::Vector(IC_EXPORT_AUDIO),
        action: FileMenuAction::ExportAudio,
    },
    MenuRow {
        label: "Export MIDI...",
        shortcut: "",
        icon: RowIcon::Vector(IC_EXPORT_MIDI),
        action: FileMenuAction::ExportMidi,
    },
    MenuRow {
        label: "Export DAWproject...",
        shortcut: "",
        icon: RowIcon::Vector(IC_EXPORT_DAWPROJECT),
        action: FileMenuAction::ExportDawProject,
    },
    MenuRow {
        label: "Export Bitwig Project (.bwproject)...",
        shortcut: "",
        icon: RowIcon::Vector(IC_EXPORT_DAWPROJECT),
        action: FileMenuAction::ExportBwProject,
    },
];

const SECTION_APP: [MenuRow; 3] = [
    MenuRow {
        label: "Show Dashboard",
        shortcut: "",
        icon: RowIcon::DashboardEmblem,
        action: FileMenuAction::ShowDashboard,
    },
    MenuRow {
        label: "Settings",
        shortcut: "Ctrl+,",
        icon: RowIcon::Vector(icons::SETTINGS),
        action: FileMenuAction::Settings,
    },
    MenuRow {
        label: "Quit",
        shortcut: "Ctrl+Q",
        icon: RowIcon::Vector(icons::X_CLOSE),
        action: FileMenuAction::Quit,
    },
];

pub fn is_open(ctx: &Context, key: Id) -> bool {
    ctx.data(|d| d.get_temp::<bool>(key.with("open")))
        .unwrap_or(false)
}

pub fn toggle(ctx: &Context, key: Id, pos: Pos2) {
    let currently_open = is_open(ctx, key);
    ctx.data_mut(|d| {
        d.insert_temp(key.with("open"), !currently_open);
        d.insert_temp(key.with("pos"), pos);
    });
}

pub fn close(ctx: &Context, key: Id) {
    ctx.data_mut(|d| d.insert_temp(key.with("open"), false));
}

fn load_pinned(ctx: &Context, key: Id) -> HashSet<FileMenuAction> {
    ctx.data(|d| d.get_temp::<HashSet<FileMenuAction>>(key.with("pinned")))
        .unwrap_or_default()
}

fn save_pinned(ctx: &Context, key: Id, pinned: HashSet<FileMenuAction>) {
    ctx.data_mut(|d| d.insert_temp(key.with("pinned"), pinned));
}

pub fn show(ctx: &Context, key: Id, anchor_rect: Rect) -> Option<FileMenuAction> {
    if !is_open(ctx, key) {
        return None;
    }

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close(ctx, key);
        return None;
    }

    let raw_pos = ctx
        .data(|d| d.get_temp::<Pos2>(key.with("pos")))
        .unwrap_or_else(|| pos2(anchor_rect.min.x, anchor_rect.max.y + 2.0));

    let total_rows = SECTION_PROJECT.len() + SECTION_EXPORT.len() + SECTION_APP.len();
    let total_h = (total_rows as f32) * ROW_H + 2.0 * SEP_H + 2.0 * PAD_Y;
    let scr = ctx.screen_rect();
    let pos = pos2(
        raw_pos.x.min(scr.max.x - MENU_W - 4.0).max(scr.min.x + 2.0),
        raw_pos.y.min(scr.max.y - total_h - 4.0).max(scr.min.y + 2.0),
    );

    let mut pinned = load_pinned(ctx, key);
    let mut pinned_changed = false;
    let mut chosen: Option<FileMenuAction> = None;

    let area_resp = egui::Area::new(key.with("area"))
        .fixed_pos(pos)
        .order(Order::Foreground)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(MENU_W, total_h), Sense::click());
            let p = ui.painter();
            p.rect_filled(
                rect.translate(vec2(2.0, 4.0)),
                2.5,
                Color32::from_black_alpha(135),
            );
            p.rect_filled(rect, 2.5, MENU_BG);
            p.rect_stroke(
                rect,
                2.5,
                Stroke::new(1.0_f32, MENU_BORDER),
                egui::StrokeKind::Inside,
            );

            let mut y = rect.min.y + PAD_Y;
            for row in &SECTION_PROJECT {
                match draw_row(ui, key, rect, y, row, pinned.contains(&row.action)) {
                    RowClick::Action => chosen = Some(row.action),
                    RowClick::TogglePin => {
                        if !pinned.remove(&row.action) {
                            pinned.insert(row.action);
                        }
                        pinned_changed = true;
                    }
                    RowClick::None => {}
                }
                y += ROW_H;
            }

            draw_separator(ui, rect, y);
            y += SEP_H;

            for row in &SECTION_EXPORT {
                match draw_row(ui, key, rect, y, row, pinned.contains(&row.action)) {
                    RowClick::Action => chosen = Some(row.action),
                    RowClick::TogglePin => {
                        if !pinned.remove(&row.action) {
                            pinned.insert(row.action);
                        }
                        pinned_changed = true;
                    }
                    RowClick::None => {}
                }
                y += ROW_H;
            }

            draw_separator(ui, rect, y);
            y += SEP_H;

            for row in &SECTION_APP {
                match draw_row(ui, key, rect, y, row, pinned.contains(&row.action)) {
                    RowClick::Action => chosen = Some(row.action),
                    RowClick::TogglePin => {
                        if !pinned.remove(&row.action) {
                            pinned.insert(row.action);
                        }
                        pinned_changed = true;
                    }
                    RowClick::None => {}
                }
                y += ROW_H;
            }
        });

    if pinned_changed {
        save_pinned(ctx, key, pinned);
    }

    if chosen.is_some() {
        close(ctx, key);
        return chosen;
    }

    let clicked_outside = ctx.input(|i| {
        i.pointer.any_pressed()
            && i.pointer.interact_pos().is_some_and(|p| {
                !area_resp.response.rect.contains(p) && !anchor_rect.contains(p)
            })
    });
    if clicked_outside {
        close(ctx, key);
    }

    None
}

enum RowClick {
    None,
    Action,
    TogglePin,
}

fn draw_separator(ui: &mut Ui, menu_rect: Rect, y: f32) {
    let sep_y = y + SEP_H * 0.5;
    ui.painter().line_segment(
        [
            pos2(menu_rect.min.x + PAD_X, sep_y),
            pos2(menu_rect.max.x - PAD_X, sep_y),
        ],
        Stroke::new(1.0_f32, SEP),
    );
}

fn draw_row(
    ui: &mut Ui,
    key: Id,
    menu_rect: Rect,
    y: f32,
    row: &MenuRow,
    is_pinned: bool,
) -> RowClick {
    let row_rect = Rect::from_min_size(pos2(menu_rect.min.x + 2.0, y), vec2(MENU_W - 4.0, ROW_H));
    let pin_rect = Rect::from_center_size(
        pos2(row_rect.max.x - 13.0, row_rect.center().y),
        vec2(18.0, ROW_H - 4.0),
    );
    let main_rect = Rect::from_min_max(row_rect.min, pos2(pin_rect.min.x - 2.0, row_rect.max.y));

    let pin_resp = ui.interact(pin_rect, key.with(("file_pin", row.label)), Sense::click());
    let main_resp = ui.interact(main_rect, key.with(("file_row", row.label)), Sense::click());

    let hovered_row = main_resp.hovered() || pin_resp.hovered();
    let p = ui.painter();

    if hovered_row {
        p.rect_filled(row_rect, 2.0, ROW_HOVER);
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    let fg = if hovered_row { TXT_ON_HOVER } else { TXT };
    let hint_fg = if hovered_row { TXT_ON_HOVER } else { TXT_HINT };

    // Icono izquierdo
    let ic_rect = Rect::from_center_size(
        pos2(row_rect.min.x + 16.0, row_rect.center().y),
        vec2(14.0, 14.0),
    );
    match row.icon {
        RowIcon::Vector(ic) => icons::paint_icon(p, ic_rect, ic, fg, 1.3_f32),
        RowIcon::DashboardEmblem => icons::paint_bitwig_emblem(
            p,
            Rect::from_center_size(
                pos2(row_rect.min.x + 16.0, row_rect.center().y),
                vec2(18.0, 12.0),
            ),
            fg,
        ),
    }

    // Etiqueta principal
    p.text(
        pos2(row_rect.min.x + 32.0, row_rect.center().y),
        Align2::LEFT_CENTER,
        row.label,
        fonts::regular(11.5),
        fg,
    );

    // Atajo de teclado (a la izquierda del pin)
    if !row.shortcut.is_empty() {
        p.text(
            pos2(pin_rect.min.x - 6.0, row_rect.center().y),
            Align2::RIGHT_CENTER,
            row.shortcut,
            fonts::regular(10.5),
            hint_fg,
        );
    }

    // Icono Pin derecho
    let pin_col = if is_pinned {
        BW_ACCENT_ORANGE
    } else if hovered_row {
        TXT_ON_HOVER
    } else {
        PIN_IDLE
    };
    let pin_ic_rect = Rect::from_center_size(pin_rect.center(), vec2(11.0, 11.0));
    icons::paint_icon(p, pin_ic_rect, IC_PIN, pin_col, 1.2_f32);

    if pin_resp.clicked() {
        RowClick::TogglePin
    } else if main_resp.clicked() {
        RowClick::Action
    } else {
        RowClick::None
    }
}

// =========================================================================
// EJECUCIÓN DE ACCIONES (DESBLOQUEADAS)
// =========================================================================

pub fn execute_action(ctx: &Context, state: &mut AppState, action: FileMenuAction) {
    match action {
        FileMenuAction::NewProject => {
            menu_bar::new_project(state);
        }
        FileMenuAction::NewFromTemplate => {
            new_from_template(state);
        }
        FileMenuAction::Open => {
            menu_bar::open_project(state);
        }
        FileMenuAction::Save => {
            menu_bar::save_project(state, false);
        }
        FileMenuAction::CollectAndSave => {
            collect_and_save(state);
        }
        FileMenuAction::SaveAs => {
            menu_bar::save_project(state, true);
        }
        FileMenuAction::SaveAsTemplate => {
            save_as_template(state);
        }
        FileMenuAction::Close => {
            menu_bar::new_project(state);
        }
        FileMenuAction::ExportAudio => {
            export_audio(state);
        }
        FileMenuAction::ExportMidi => {
            export_midi(state);
        }
        FileMenuAction::ExportDawProject => {
            export_dawproject(state);
        }
        FileMenuAction::ExportBwProject => {
            export_bwproject(state);
        }
        FileMenuAction::ShowDashboard => {
            state.control_panel_state.is_open = true;
            state.control_panel_state.top_tab = DashboardTopTab::User;
            state
                .modal_drags
                .reset(crate::views::modal::ModalId::ControlPanel);
        }
        FileMenuAction::Settings => {
            state.control_panel_state.is_open = true;
            state.control_panel_state.top_tab = DashboardTopTab::Settings;
            state.control_panel_state.sub_tab = DashboardSubTab::Audio;
            state
                .modal_drags
                .reset(crate::views::modal::ModalId::ControlPanel);
        }
        FileMenuAction::Quit => {
            if state.project_dirty {
                let discard = rfd::MessageDialog::new()
                    .set_title("Hikaru OpenLive")
                    .set_description("El proyecto tiene cambios sin guardar.\n\n¿Salir de todas formas?")
                    .set_buttons(rfd::MessageButtons::YesNo)
                    .show()
                    == rfd::MessageDialogResult::Yes;
                if !discard {
                    return;
                }
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn project_stem(state: &AppState) -> String {
    state
        .project_path
        .as_ref()
        .and_then(|p| p.file_stem())
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Untitled".to_string())
}

fn collect_and_save(state: &mut AppState) {
    let current = state.project_path.clone();
    let Some(target) = project::ask_save_path(state.mode, current.as_deref()) else {
        return;
    };

    if let Some(parent) = target.parent() {
        let samples_dir = parent.join("samples");
        if let Err(err) = std::fs::create_dir_all(&samples_dir) {
            eprintln!("[PROJECT] no se pudo crear carpeta samples/: {}", err);
        } else {
            let mut copied_count = 0usize;

            // 1. Recolectar samples del Arranger/Playlist
            for (_, clip) in &mut state.playlist_state.clips {
                if let ClipType::Audio { sample_path, .. } = &mut clip.clip_type {
                    let src = PathBuf::from(&*sample_path);
                    if let Some(dest) = copy_sample_into(&src, &samples_dir) {
                        *sample_path = dest.to_string_lossy().to_string();
                        copied_count += 1;
                    }
                }
            }

            // 2. Recolectar samples de la Session Matrix
            for col in &mut state.matrix_state.grid {
                for slot in col {
                    if let Some(clip) = &mut slot.clip {
                        if clip.path.is_file() {
                            if let Some(dest) = copy_sample_into(&clip.path, &samples_dir) {
                                clip.path = dest;
                                copied_count += 1;
                            }
                        }
                        if let ClipData::Audio { events, .. } = &mut clip.content {
                            for ev in events {
                                if let Some(src) = &ev.source_path {
                                    if let Some(dest) = copy_sample_into(src, &samples_dir) {
                                        ev.source_path = Some(dest);
                                        copied_count += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            println!(
                "[PROJECT] Collect and Save: {} referencia(s) recolectada(s) en {}",
                copied_count,
                samples_dir.display()
            );
        }
    }

    match project::save(state, &target) {
        Ok(saved) => {
            state.project_path = Some(saved.clone());
            state.project_dirty = false;
            state.control_panel_state.record_recent(&saved);
            println!("[PROJECT] proyecto recolectado y guardado: {}", saved.display());
        }
        Err(err) => eprintln!("[PROJECT] error en Collect and Save: {}", err),
    }
}

fn copy_sample_into(src: &Path, samples_dir: &Path) -> Option<PathBuf> {
    if !src.is_file() {
        return None;
    }
    let file_name = src.file_name()?;
    let dest = samples_dir.join(file_name);
    if src == dest {
        return Some(dest);
    }
    if dest.exists() {
        // Si ya existe con el mismo tamaño, reutilizar; si no, desambiguar.
        let same_len = std::fs::metadata(src)
            .ok()
            .zip(std::fs::metadata(&dest).ok())
            .is_some_and(|(a, b)| a.len() == b.len());
        if same_len {
            return Some(dest);
        }
        let stem = src.file_stem()?.to_string_lossy();
        let ext = src.extension().map(|e| e.to_string_lossy()).unwrap_or_default();
        for idx in 1..1000 {
            let candidate = if ext.is_empty() {
                samples_dir.join(format!("{stem}_{idx}"))
            } else {
                samples_dir.join(format!("{stem}_{idx}.{ext}"))
            };
            if !candidate.exists() {
                if std::fs::copy(src, &candidate).is_ok() {
                    return Some(candidate);
                }
                return None;
            }
        }
        return None;
    }
    if std::fs::copy(src, &dest).is_ok() {
        Some(dest)
    } else {
        None
    }
}

fn templates_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if !xdg.trim().is_empty() {
            return PathBuf::from(xdg).join("hikaru-openlive").join("templates");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("hikaru-openlive")
            .join("templates");
    }
    PathBuf::from("templates")
}

fn save_as_template(state: &mut AppState) {
    let dir = templates_dir();
    let _ = std::fs::create_dir_all(&dir);
    let (label, ext) = match state.mode {
        AppMode::OpenLive => (hikaru_openlive_file::FILE_FILTER, project::OPENLIVE_EXTENSION),
        AppMode::OpenStudio => (
            hikaru_openstudio_file::FILE_FILTER,
            project::OPENSTUDIO_EXTENSION,
        ),
    };
    let default_name = format!("{}-template.{}", project_stem(state), ext);
    let Some(target) = rfd::FileDialog::new()
        .set_title("Save as Template")
        .set_directory(&dir)
        .set_file_name(&default_name)
        .add_filter(label, &[ext])
        .save_file()
    else {
        return;
    };

    match project::save(state, &target) {
        Ok(saved) => {
            println!("[PROJECT] plantilla guardada: {}", saved.display());
            state.control_panel_state.status_notice =
                Some(format!("Plantilla guardada: {}", saved.display()));
        }
        Err(err) => eprintln!("[PROJECT] no se pudo guardar plantilla: {}", err),
    }
}

fn export_audio(state: &mut AppState) {
    let default_name = format!("{}.wav", project_stem(state));
    let mut dialog = rfd::FileDialog::new()
        .set_title("Export Audio (WAV)")
        .set_file_name(&default_name)
        .add_filter("WAV Audio (*.wav)", &["wav"]);
    if let Some(dir) = state.project_path.as_ref().and_then(|p| p.parent()) {
        dialog = dialog.set_directory(dir);
    }
    let Some(mut target) = dialog.save_file() else {
        return;
    };
    if target.extension().is_none() {
        target.set_extension("wav");
    }

    let sr = (state.transport.sample_rate.get().round() as u32).max(8000);
    let bpm = state.transport.bpm.max(1.0);
    let ppqn = state.playlist_state.ppqn.max(1) as f64;
    let ticks_to_frames = |ticks: u64| -> usize {
        let beats = ticks as f64 / ppqn;
        let secs = beats * 60.0 / bpm;
        (secs * sr as f64).round().max(0.0) as usize
    };

    // Determinar duración total del bounce
    let mut max_frames = 0usize;
    for (_, clip) in &state.playlist_state.clips {
        if !clip.muted {
            let end_frame = ticks_to_frames(clip.start_tick.saturating_add(clip.duration_ticks));
            max_frames = max_frames.max(end_frame);
        }
    }
    // Si no hay clips en playlist, usar clips de la Session Matrix o 4 compases de silencio
    let use_matrix_fallback = max_frames == 0;
    if use_matrix_fallback {
        for col in &state.matrix_state.grid {
            for slot in col {
                if let Some(clip) = &slot.clip {
                    if !clip.muted {
                        let f = (clip.duration_secs * sr as f64).round() as usize;
                        max_frames = max_frames.max(f);
                    }
                }
            }
        }
    }
    if max_frames == 0 {
        let four_bars_beats = (state.transport.beats_per_bar.max(1) * 4) as f64;
        max_frames = ((four_bars_beats * 60.0 / bpm) * sr as f64).round() as usize;
    }

    let mut mix_l = vec![0.0f32; max_frames];
    let mut mix_r = vec![0.0f32; max_frames];

    let master_gain = state
        .live_tracks
        .first()
        .map(|t| if t.mute { 0.0 } else { t.volume })
        .unwrap_or(1.0);

    if !use_matrix_fallback {
        for (track_id, clip) in &state.playlist_state.clips {
            if clip.muted {
                continue;
            }
            let (trk_gain, trk_pan, trk_mute) = state
                .live_tracks
                .get(*track_id)
                .map(|t| (t.volume, t.pan, t.mute))
                .unwrap_or((1.0, 0.0, false));
            if trk_mute {
                continue;
            }
            let ClipType::Audio {
                sample_path,
                sample_offset_ticks,
                ..
            } = &clip.clip_type
            else {
                continue;
            };
            let Ok((samples, ch, src_sr)) = wav_io::read_file(sample_path) else {
                continue;
            };
            let src_ch = ch.max(1);
            let src_total_frames = samples.len() / src_ch;
            if src_total_frames == 0 || src_sr == 0 {
                continue;
            }

            let dst_start = ticks_to_frames(clip.start_tick);
            let dst_len = ticks_to_frames(clip.duration_ticks);
            let offset_secs = (*sample_offset_ticks as f64 / ppqn) * (60.0 / bpm);
            let src_start_f = offset_secs * src_sr as f64;
            let ratio = src_sr as f64 / sr as f64;

            let pan = trk_pan.clamp(-1.0, 1.0);
            let gain_l = trk_gain * ((1.0 - pan) * 0.5).sqrt();
            let gain_r = trk_gain * ((1.0 + pan) * 0.5).sqrt();

            for i in 0..dst_len {
                let dst_idx = dst_start + i;
                if dst_idx >= max_frames {
                    break;
                }
                let src_pos = src_start_f + i as f64 * ratio;
                let idx0 = src_pos.floor() as usize;
                if idx0 >= src_total_frames {
                    break;
                }
                let l = samples[idx0 * src_ch];
                let r = if src_ch > 1 {
                    samples[idx0 * src_ch + 1]
                } else {
                    l
                };
                mix_l[dst_idx] += l * gain_l;
                mix_r[dst_idx] += r * gain_r;
            }
        }
    } else {
        for (col_idx, col) in state.matrix_state.grid.iter().enumerate() {
            let (trk_gain, trk_mute) = state
                .live_tracks
                .get(col_idx + 1)
                .map(|t| (t.volume, t.mute))
                .unwrap_or((1.0, false));
            if trk_mute {
                continue;
            }
            for slot in col {
                let Some(clip) = &slot.clip else { continue };
                if clip.muted {
                    continue;
                }
                if let ClipData::Audio { events, .. } = &clip.content {
                    for ev in events {
                        let src_ch = ev.channels.max(1);
                        let win = ev.window_samples();
                        let frames = win.len() / src_ch;
                        let start_f = (ev.start_secs * sr as f64).round().max(0.0) as usize;
                        let ratio = ev.sample_rate.max(1) as f64 / sr as f64;
                        let out_frames = ((frames as f64) / ratio).round() as usize;
                        for i in 0..out_frames {
                            let dst_idx = start_f + i;
                            if dst_idx >= max_frames {
                                break;
                            }
                            let idx0 = ((i as f64) * ratio).floor() as usize;
                            if idx0 >= frames {
                                break;
                            }
                            let l = win[idx0 * src_ch] * ev.gain * trk_gain;
                            let r = if src_ch > 1 {
                                win[idx0 * src_ch + 1] * ev.gain * trk_gain
                            } else {
                                l
                            };
                            mix_l[dst_idx] += l;
                            mix_r[dst_idx] += r;
                        }
                    }
                }
            }
        }
    }

    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: sr,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    match hound::WavWriter::create(&target, spec) {
        Ok(mut writer) => {
            for i in 0..max_frames {
                let l = (mix_l[i] * master_gain).clamp(-1.0, 1.0);
                let r = (mix_r[i] * master_gain).clamp(-1.0, 1.0);
                let _ = writer.write_sample(l);
                let _ = writer.write_sample(r);
            }
            if let Err(err) = writer.finalize() {
                eprintln!("[EXPORT] error finalizando WAV: {}", err);
            } else {
                println!("[EXPORT] Audio exportado: {}", target.display());
            }
        }
        Err(err) => eprintln!("[EXPORT] no se pudo crear WAV: {}", err),
    }
}

fn export_midi(state: &mut AppState) {
    let default_name = format!("{}.mid", project_stem(state));
    let mut dialog = rfd::FileDialog::new()
        .set_title("Export MIDI")
        .set_file_name(&default_name)
        .add_filter("Standard MIDI File (*.mid)", &["mid", "midi"]);
    if let Some(dir) = state.project_path.as_ref().and_then(|p| p.parent()) {
        dialog = dialog.set_directory(dir);
    }
    let Some(mut target) = dialog.save_file() else {
        return;
    };
    if target.extension().is_none() {
        target.set_extension("mid");
    }

    let mut midi_tracks: Vec<(String, Vec<(u64, u8, u8, u32)>)> = Vec::new();
    for (col_idx, col) in state.matrix_state.grid.iter().enumerate() {
        let track_name = state
            .matrix_state
            .tracks
            .get(col_idx)
            .map(|t| t.name.clone())
            .unwrap_or_else(|| format!("Track {}", col_idx + 1));
        let mut combined_notes = Vec::new();
        for slot in col {
            if let Some(clip) = &slot.clip {
                if let ClipData::Midi { notes } = &clip.content {
                    combined_notes.extend_from_slice(notes);
                }
            }
        }
        if !combined_notes.is_empty() {
            midi_tracks.push((track_name, combined_notes));
        }
    }

    if midi_tracks.is_empty() {
        // Exportar pista MIDI vacía válida con el tempo y compás del proyecto
        midi_tracks.push(("Track 1".to_string(), Vec::new()));
    }

    match hikaru_midi::file::write_midi_tracks_to_bytes(
        &midi_tracks,
        state.transport.bpm,
        state.transport.beats_per_bar,
        state.transport.beat_division,
    ) {
        Ok(bytes) => {
            if let Err(err) = std::fs::write(&target, bytes) {
                eprintln!("[EXPORT] no se pudo escribir MIDI: {}", err);
            } else {
                println!("[EXPORT] MIDI exportado: {}", target.display());
            }
        }
        Err(err) => eprintln!("[EXPORT] error serializando MIDI: {}", err),
    }
}

fn export_dawproject(state: &mut AppState) {
    let default_name = format!("{}.dawproject", project_stem(state));
    let mut dialog = rfd::FileDialog::new()
        .set_title("Export DAWproject")
        .set_file_name(&default_name)
        .add_filter(
            project::dawproject::FILE_FILTER,
            &[project::DAWPROJECT_EXTENSION],
        );
    if let Some(dir) = state.project_path.as_ref().and_then(|p| p.parent()) {
        dialog = dialog.set_directory(dir);
    }
    let Some(mut target) = dialog.save_file() else {
        return;
    };
    if target.extension().is_none() {
        target.set_extension(project::DAWPROJECT_EXTENSION);
    }

    match project::dawproject::export_to_path(state, &target) {
        Ok(()) => println!("[EXPORT] DAWproject exportado: {}", target.display()),
        Err(err) => eprintln!("[EXPORT] no se pudo escribir DAWproject: {}", err),
    }
}

fn new_from_template(state: &mut AppState) {
    let Some(path) = project::bwtemplate::ask_template_path(state.project_path.as_deref()) else {
        return;
    };
    match project::bwtemplate::new_from_template(state, &path) {
        Ok(()) => println!("[TEMPLATE] proyecto nuevo desde plantilla: {}", path.display()),
        Err(err) => eprintln!("[TEMPLATE] no se pudo cargar .bwtemplate: {}", err),
    }
}

fn export_bwproject(state: &mut AppState) {
    let default_name = format!("{}.bwproject", project_stem(state));
    let mut dialog = rfd::FileDialog::new()
        .set_title("Export Bitwig Project (.bwproject)")
        .set_file_name(&default_name)
        .add_filter(
            project::bwproject::FILE_FILTER,
            &[project::BWPROJECT_EXTENSION],
        );
    if let Some(dir) = state.project_path.as_ref().and_then(|p| p.parent()) {
        dialog = dialog.set_directory(dir);
    }
    let Some(mut target) = dialog.save_file() else {
        return;
    };
    if target.extension().is_none() {
        target.set_extension(project::BWPROJECT_EXTENSION);
    }

    match project::bwproject::export_to_path(state, &target) {
        Ok(()) => println!("[EXPORT] .bwproject exportado: {}", target.display()),
        Err(err) => eprintln!("[EXPORT] no se pudo escribir .bwproject: {}", err),
    }
}
