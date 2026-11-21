// Hikaru OpenLive - Control Panel / Dashboard (Bitwig-inspired egui)
// GNU AGPLv3
// crates/hikaru_gui/src/views/control_panel.rs

use std::path::{Path, PathBuf};

use egui::{
    pos2, vec2, Align, Align2, Area, Color32, Context, CornerRadius, CursorIcon, Frame, Id, Key,
    Layout, Margin, Order, Rect, RichText, ScrollArea, Sense, Shadow, Stroke, StrokeKind,
    TextEdit, Ui, UiBuilder, Window,
};

use crate::app::{AppMode, AppState, OpenLiveView, OpenStudioView};
use crate::project;
use crate::theme::*;
use crate::version::VERSION;
use crate::views::audio_settings::AudioBackend;
use crate::views::branding::energy_core_logo_widget;
use crate::views::menu_bar;

// =========================================================================
// PALETA BITWIG DASHBOARD
// =========================================================================

const DASH_BG: Color32 = hex(0x2B2B2B);
const DASH_TOPBAR_BG: Color32 = hex(0x2E2E2E);
const DASH_BORDER: Color32 = hex(0x3F3F3F);
const DASH_DIVIDER: Color32 = hex(0x212121);
const DASH_SEARCH_BG: Color32 = hex(0x1B1B1B);
const DASH_SEARCH_BORDER: Color32 = hex(0x383838);
const DASH_BTN_BG: Color32 = hex(0x3A3A3A);
const DASH_BTN_HOVER: Color32 = hex(0x474747);
const DASH_BTN_BORDER: Color32 = hex(0x4D4D4D);
const DASH_ROW_HOVER: Color32 = hex(0x333333);

// =========================================================================
// NAVEGACIÓN DEL DASHBOARD
// =========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DashboardTopTab {
    User,
    Settings,
    Plugins,
    Help,
}

impl Default for DashboardTopTab {
    fn default() -> Self {
        Self::User
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DashboardSubTab {
    // USER
    QuickStart,
    RecentProjects,
    MyProjects,
    MyTemplates,
    // SETTINGS
    Audio,
    ExternalPlugins,
    McpServers,
    Interface,
    // PLUGINS
    VstClap,
    NativeDevices,
    // HELP
    About,
    Shortcuts,
}

impl Default for DashboardSubTab {
    fn default() -> Self {
        Self::RecentProjects
    }
}

pub struct ControlPanelState {
    pub is_open: bool,
    pub top_tab: DashboardTopTab,
    pub sub_tab: DashboardSubTab,
    pub search_query: String,
    pub recent_projects: Vec<PathBuf>,
    pub status_notice: Option<String>,
}

impl Default for ControlPanelState {
    fn default() -> Self {
        Self {
            is_open: false,
            top_tab: DashboardTopTab::User,
            sub_tab: DashboardSubTab::RecentProjects,
            search_query: String::new(),
            recent_projects: discover_workspace_projects(),
            status_notice: None,
        }
    }
}

impl ControlPanelState {
    pub fn record_recent(&mut self, path: &Path) {
        let p = path.to_path_buf();
        self.recent_projects.retain(|existing| existing != &p);
        self.recent_projects.insert(0, p);
        if self.recent_projects.len() > 24 {
            self.recent_projects.truncate(24);
        }
    }

    fn select_top_tab(&mut self, tab: DashboardTopTab) {
        self.top_tab = tab;
        self.sub_tab = match tab {
            DashboardTopTab::User => DashboardSubTab::RecentProjects,
            DashboardTopTab::Settings => DashboardSubTab::Audio,
            DashboardTopTab::Plugins => DashboardSubTab::VstClap,
            DashboardTopTab::Help => DashboardSubTab::About,
        };
    }
}

fn discover_workspace_projects() -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(".") {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if ext == project::OPENLIVE_EXTENSION
                    || ext == project::OPENSTUDIO_EXTENSION
                    || ext.eq_ignore_ascii_case(project::DAWPROJECT_EXTENSION)
                    || ext.eq_ignore_ascii_case(project::BWPROJECT_EXTENSION)
                {
                    found.push(path);
                }
            }
        }
    }
    found
}

// =========================================================================
// RENDER PRINCIPAL
// =========================================================================

pub fn render(ctx: &Context, state: &mut AppState) {
    if !state.control_panel_state.is_open {
        return;
    }

    // Cerrar con Escape
    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        state.control_panel_state.is_open = false;
        return;
    }

    // Backdrop oscuro translúcido estilo Bitwig
    let screen_rect = ctx.screen_rect();
    Area::new(Id::new("hikaru_dashboard_backdrop"))
        .order(Order::Middle)
        .fixed_pos(screen_rect.min)
        .show(ctx, |ui| {
            let resp = ui.allocate_rect(screen_rect, Sense::click());
            ui.painter().rect_filled(
                screen_rect,
                0.0,
                Color32::from_black_alpha(135),
            );
            if resp.clicked() {
                state.control_panel_state.is_open = false;
            }
        });

    let mut is_open = state.control_panel_state.is_open;
    let frame = Frame::NONE
        .fill(DASH_BG)
        .stroke(Stroke::new(1.0_f32, DASH_BORDER))
        .corner_radius(CornerRadius::same(6))
        .shadow(Shadow {
            offset: [0, 10],
            blur: 28,
            spread: 2,
            color: Color32::from_black_alpha(160),
        })
        .inner_margin(Margin::ZERO);

    Window::new("hikaru_dashboard_window")
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .order(Order::Foreground)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size([840.0, 560.0])
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_min_size(vec2(840.0, 560.0));
            ui.set_max_size(vec2(840.0, 560.0));

            // 1. Barra superior estilo Bitwig (USER | SETTINGS | PLUGINS | HELP | ×)
            render_top_bar(ui, state, &mut is_open);

            // Línea divisoria sutil bajo la barra superior
            let sep_rect = ui.available_rect_before_wrap();
            let line_y = sep_rect.min.y;
            ui.painter().line_segment(
                [pos2(sep_rect.min.x, line_y), pos2(sep_rect.max.x, line_y)],
                Stroke::new(1.0_f32, DASH_DIVIDER),
            );

            // 2. Cuerpo de 2 columnas: Sidebar izquierdo + Contenido derecho
            let body_rect = ui.available_rect_before_wrap();
            let sidebar_w = 196.0;
            let sidebar_rect = Rect::from_min_max(
                body_rect.min,
                pos2(body_rect.min.x + sidebar_w, body_rect.max.y),
            );
            let content_rect = Rect::from_min_max(
                pos2(body_rect.min.x + sidebar_w, body_rect.min.y),
                body_rect.max,
            );

            ui.allocate_new_ui(UiBuilder::new().max_rect(sidebar_rect), |ui| {
                render_sidebar(ui, state, &mut is_open, sidebar_rect);
            });
            ui.allocate_new_ui(UiBuilder::new().max_rect(content_rect), |ui| {
                render_main_content(ui, state, &mut is_open, content_rect);
            });
        });

    state.control_panel_state.is_open = is_open;
}

// =========================================================================
// TOP BAR (BITWIG STYLE)
// =========================================================================

fn render_top_bar(ui: &mut Ui, state: &mut AppState, is_open: &mut bool) {
    let top_h = 54.0;
    let (top_rect, _) = ui.allocate_exact_size(vec2(840.0, top_h), Sense::hover());

    ui.painter().rect_filled(
        top_rect,
        CornerRadius {
            nw: 6,
            ne: 6,
            sw: 0,
            se: 0,
        },
        DASH_TOPBAR_BG,
    );

    let tabs = [
        ("USER", DashboardTopTab::User),
        ("SETTINGS", DashboardTopTab::Settings),
        ("PLUGINS", DashboardTopTab::Plugins),
        ("HELP", DashboardTopTab::Help),
    ];

    let mut x = top_rect.min.x + 24.0;
    let center_y = top_rect.center().y;

    for (label, tab) in tabs {
        let active = state.control_panel_state.top_tab == tab;
        let galley = ui.painter().layout_no_wrap(
            label.to_string(),
            egui::FontId::proportional(17.5),
            Color32::WHITE,
        );
        let text_w = galley.size().x;
        let tab_rect = Rect::from_min_size(
            pos2(x, top_rect.min.y + 8.0),
            vec2(text_w + 16.0, top_h - 16.0),
        );
        let resp = ui.interact(tab_rect, ui.id().with(("dash_top", label)), Sense::click());

        let color = if active {
            Color32::WHITE
        } else if resp.hovered() {
            Color32::from_rgb(0xD2, 0xD2, 0xD2)
        } else {
            Color32::from_rgb(0x8E, 0x8E, 0x8E)
        };

        ui.painter().text(
            pos2(tab_rect.min.x + 4.0, center_y),
            Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(17.5),
            color,
        );

        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if resp.clicked() {
            state.control_panel_state.select_top_tab(tab);
        }

        // Espaciado generoso estilo Bitwig entre las pestañas superiores
        x += text_w + 68.0;
    }

    // Botón cerrar (×) minimalista a la derecha
    let close_size = 28.0;
    let close_rect = Rect::from_center_size(
        pos2(top_rect.max.x - 26.0, center_y),
        vec2(close_size, close_size),
    );
    let close_resp = ui.interact(close_rect, ui.id().with("dash_close_x"), Sense::click());
    let x_col = if close_resp.hovered() {
        Color32::WHITE
    } else {
        Color32::from_rgb(0x9A, 0x9A, 0x9A)
    };
    let r = 5.5_f32;
    let c = close_rect.center();
    ui.painter().line_segment(
        [pos2(c.x - r, c.y - r), pos2(c.x + r, c.y + r)],
        Stroke::new(1.3_f32, x_col),
    );
    ui.painter().line_segment(
        [pos2(c.x + r, c.y - r), pos2(c.x - r, c.y + r)],
        Stroke::new(1.3_f32, x_col),
    );
    if close_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if close_resp.clicked() {
        *is_open = false;
    }
}

// =========================================================================
// SIDEBAR IZQUIERDO
// =========================================================================

fn render_sidebar(
    ui: &mut Ui,
    state: &mut AppState,
    is_open: &mut bool,
    side_rect: Rect,
) {
    let width = side_rect.width();
    let height = side_rect.height();
    let items: &[(&str, DashboardSubTab)] = match state.control_panel_state.top_tab {
        DashboardTopTab::User => &[
            ("Quick Start", DashboardSubTab::QuickStart),
            ("Recent Projects", DashboardSubTab::RecentProjects),
            ("My Projects", DashboardSubTab::MyProjects),
            ("My Templates", DashboardSubTab::MyTemplates),
        ],
        DashboardTopTab::Settings => &[
            ("Audio & Hardware", DashboardSubTab::Audio),
            ("External Plugins", DashboardSubTab::ExternalPlugins),
            ("MCP Servers", DashboardSubTab::McpServers),
            ("Interface & Mode", DashboardSubTab::Interface),
        ],
        DashboardTopTab::Plugins => &[
            ("VST3 / CLAP", DashboardSubTab::VstClap),
            ("Native Devices", DashboardSubTab::NativeDevices),
        ],
        DashboardTopTab::Help => &[
            ("About Hikaru", DashboardSubTab::About),
            ("Shortcuts", DashboardSubTab::Shortcuts),
        ],
    };

    let pad_x = 22.0;

    // Lista superior de sub-pestañas con indicador naranja vertical a la izquierda
    let mut y = side_rect.min.y + 22.0;
    for &(label, sub) in items {
        let active = state.control_panel_state.sub_tab == sub;
        let row_rect = Rect::from_min_size(pos2(side_rect.min.x + pad_x, y), vec2(width - pad_x * 2.0, 26.0));
        let resp = ui.interact(row_rect, ui.id().with(("dash_sub", label)), Sense::click());

        if active {
            // Barra vertical naranja Bitwig (2.5px)
            let bar_rect = Rect::from_min_size(
                pos2(row_rect.min.x, row_rect.min.y + 4.0),
                vec2(2.5, row_rect.height() - 8.0),
            );
            ui.painter().rect_filled(bar_rect, 1.0, ACCENT_ORANGE);
        }

        let text_col = if active {
            Color32::WHITE
        } else if resp.hovered() {
            Color32::from_rgb(0xD8, 0xD8, 0xD8)
        } else {
            Color32::from_rgb(0x96, 0x96, 0x96)
        };

        ui.painter().text(
            pos2(row_rect.min.x + 10.0, row_rect.center().y),
            Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(12.5),
            text_col,
        );

        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if resp.clicked() {
            state.control_panel_state.sub_tab = sub;
        }

        y += 30.0;
    }

    // Botones de acción apilados abajo (New Project..., Open File..., Save Project..., Audio Setup...)
    let btn_w = width - pad_x * 2.0;
    let btn_h = 26.0;
    let btn_gap = 7.0;
    let bottom_pad = 24.0;
    let start_y = side_rect.min.y + height - bottom_pad - (btn_h * 4.0 + btn_gap * 3.0);

    let actions = [
        ("New Project...", 0),
        ("Open File...", 1),
        ("Save Project...", 2),
        ("Audio Setup...", 3),
    ];

    for (idx, (label, action_id)) in actions.iter().enumerate() {
        let by = start_y + idx as f32 * (btn_h + btn_gap);
        let rect = Rect::from_min_size(pos2(side_rect.min.x + pad_x, by), vec2(btn_w, btn_h));
        if paint_bitwig_button(ui, rect, label, ("dash_side_btn", *action_id)) {
            match action_id {
                0 => {
                    menu_bar::new_project(state);
                    *is_open = false;
                }
                1 => {
                    menu_bar::open_project(state);
                    if let Some(p) = state.project_path.clone() {
                        state.control_panel_state.record_recent(&p);
                    }
                    *is_open = false;
                }
                2 => {
                    menu_bar::save_project(state, false);
                    if let Some(p) = state.project_path.clone() {
                        state.control_panel_state.record_recent(&p);
                    }
                }
                3 => {
                    state.control_panel_state.top_tab = DashboardTopTab::Settings;
                    state.control_panel_state.sub_tab = DashboardSubTab::Audio;
                }
                _ => {}
            }
        }
    }
}

// =========================================================================
// CONTENIDO PRINCIPAL DERECHO
// =========================================================================

fn render_main_content(
    ui: &mut Ui,
    state: &mut AppState,
    is_open: &mut bool,
    content_rect: Rect,
) {
    let width = content_rect.width();
    let height = content_rect.height();
    let pad_left = 10.0;
    let pad_right = 26.0;
    let pad_top = 20.0;
    let inner_rect = Rect::from_min_max(
        pos2(content_rect.min.x + pad_left, content_rect.min.y + pad_top),
        pos2(content_rect.min.x + width - pad_right, content_rect.min.y + height - 20.0),
    );

    ui.allocate_new_ui(UiBuilder::new().max_rect(inner_rect), |ui| {
        ui.set_width(inner_rect.width());

        let show_search = matches!(
            state.control_panel_state.sub_tab,
            DashboardSubTab::RecentProjects
                | DashboardSubTab::MyProjects
                | DashboardSubTab::MyTemplates
                | DashboardSubTab::ExternalPlugins
                | DashboardSubTab::VstClap
                | DashboardSubTab::NativeDevices
        );

        if show_search {
            render_search_bar(ui, &mut state.control_panel_state.search_query, inner_rect.width());
            ui.add_space(14.0);
        }

        ScrollArea::vertical()
            .id_salt(("dash_content_scroll", state.control_panel_state.sub_tab as u8))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(inner_rect.width());
                match state.control_panel_state.sub_tab {
                    DashboardSubTab::RecentProjects => render_recent_projects(ui, state, is_open),
                    DashboardSubTab::QuickStart => render_quick_start(ui, state, is_open),
                    DashboardSubTab::MyProjects => render_my_projects(ui, state, is_open),
                    DashboardSubTab::MyTemplates => render_templates(ui, state, is_open),
                    DashboardSubTab::Audio => render_audio_settings(ui, state),
                    DashboardSubTab::ExternalPlugins | DashboardSubTab::VstClap => {
                        render_plugins_tab(ui, state);
                    }
                    DashboardSubTab::NativeDevices => render_native_devices(ui, state),
                    DashboardSubTab::McpServers => render_mcp_tab(ui, state),
                    DashboardSubTab::Interface => render_interface_tab(ui, state),
                    DashboardSubTab::About => render_about_tab(ui),
                    DashboardSubTab::Shortcuts => render_shortcuts_tab(ui),
                }
            });
    });
}

fn render_search_bar(ui: &mut Ui, query: &mut String, width: f32) {
    let h = 28.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, h), Sense::hover());

    ui.painter().rect(
        rect,
        CornerRadius::same(3),
        DASH_SEARCH_BG,
        Stroke::new(1.0_f32, DASH_SEARCH_BORDER),
        StrokeKind::Inside,
    );

    // Icono lupa minimalista
    let icon_c = pos2(rect.min.x + 14.0, rect.center().y - 1.0);
    let icon_col = Color32::from_rgb(0x80, 0x80, 0x80);
    ui.painter().circle_stroke(icon_c, 4.2, Stroke::new(1.2_f32, icon_col));
    ui.painter().line_segment(
        [pos2(icon_c.x + 3.0, icon_c.y + 3.0), pos2(icon_c.x + 6.2, icon_c.y + 6.2)],
        Stroke::new(1.2_f32, icon_col),
    );

    // Campo de texto
    let edit_rect = Rect::from_min_max(
        pos2(rect.min.x + 26.0, rect.min.y + 3.0),
        pos2(rect.max.x - 26.0, rect.max.y - 3.0),
    );
    ui.allocate_new_ui(UiBuilder::new().max_rect(edit_rect), |ui| {
        let edit = TextEdit::singleline(query)
            .frame(false)
            .hint_text(RichText::new("Type to search...").color(Color32::from_rgb(0x70, 0x70, 0x70)))
            .text_color(TEXT_PRIMARY)
            .desired_width(edit_rect.width());
        ui.add(edit);
    });

    // Botón × para limpiar búsqueda
    if !query.is_empty() {
        let clr_rect = Rect::from_center_size(pos2(rect.max.x - 14.0, rect.center().y), vec2(18.0, 18.0));
        let clr_resp = ui.interact(clr_rect, ui.id().with("dash_search_clear"), Sense::click());
        let c = clr_rect.center();
        let col = if clr_resp.hovered() { Color32::WHITE } else { icon_col };
        ui.painter().line_segment([pos2(c.x - 3.5, c.y - 3.5), pos2(c.x + 3.5, c.y + 3.5)], Stroke::new(1.2_f32, col));
        ui.painter().line_segment([pos2(c.x + 3.5, c.y - 3.5), pos2(c.x - 3.5, c.y + 3.5)], Stroke::new(1.2_f32, col));
        if clr_resp.clicked() {
            query.clear();
        }
    }
}

// =========================================================================
// SUB-VISTAS DE USER
// =========================================================================

fn render_recent_projects(ui: &mut Ui, state: &mut AppState, is_open: &mut bool) {
    let query = state.control_panel_state.search_query.trim().to_lowercase();

    // Proyecto actualmente abierto
    if let Some(current) = state.project_path.clone() {
        let name = current
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled Project")
            .to_string();
        if query.is_empty() || name.to_lowercase().contains(&query) {
            if render_project_row(ui, &name, "Current active session", "Active", ("curr_proj", 0)) {
                *is_open = false;
            }
        }
    }

    let recents = state.control_panel_state.recent_projects.clone();
    let mut shown = 0usize;
    for (idx, path) in recents.iter().enumerate() {
        if state.project_path.as_ref() == Some(path) {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Project")
            .to_string();
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_uppercase();
        if !query.is_empty() && !name.to_lowercase().contains(&query) && !ext.to_lowercase().contains(&query) {
            continue;
        }
        shown += 1;
        let subtitle = format!("{} · {}", ext, path.display());
        if render_project_row(ui, &name, &subtitle, "Open", ("recent_proj", idx)) {
            if let Err(err) = project::load(state, path) {
                state.control_panel_state.status_notice = Some(format!("Error al abrir: {}", err));
            } else {
                state.project_path = Some(path.clone());
                state.project_dirty = false;
                state.control_panel_state.record_recent(path);
                *is_open = false;
            }
        }
    }

    if state.project_path.is_none() && shown == 0 {
        ui.add_space(18.0);
        ui.label(
            RichText::new("No recent projects yet.")
                .size(13.0)
                .color(TEXT_MUTED),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("Use \"New Project...\" or \"Open File...\" on the left to start a .oplf (OpenLive) or .opsf (OpenStudio) session.")
                .size(11.5)
                .color(Color32::from_rgb(0x76, 0x76, 0x76)),
        );
    }

    if let Some(notice) = &state.control_panel_state.status_notice {
        ui.add_space(10.0);
        ui.label(RichText::new(notice).size(11.5).color(ACCENT_ORANGE));
    }
}

fn render_quick_start(ui: &mut Ui, state: &mut AppState, is_open: &mut bool) {
    render_section_title(ui, "Quick Start");
    ui.add_space(8.0);

    let mode_name = match state.mode {
        AppMode::OpenLive => "OpenLive (Session Matrix)",
        AppMode::OpenStudio => "OpenStudio (Linear Arranger)",
    };

    if render_project_row(
        ui,
        "Switch to OpenLive Mode",
        "Clip launcher, live performance matrix & scenes",
        if state.mode == AppMode::OpenLive { "Active" } else { "Switch" },
        ("qs_live", 0),
    ) {
        state.mode = AppMode::OpenLive;
        state.openlive_view = OpenLiveView::SessionMatrix;
        *is_open = false;
    }

    if render_project_row(
        ui,
        "Switch to OpenStudio Mode",
        "Linear timeline arranger, tracks & automation",
        if state.mode == AppMode::OpenStudio { "Active" } else { "Switch" },
        ("qs_studio", 1),
    ) {
        state.mode = AppMode::OpenStudio;
        state.openstudio_view = OpenStudioView::Playlist;
        *is_open = false;
    }

    if render_project_row(
        ui,
        "Configure Audio Engine",
        &format!(
            "{:?} · {} Hz · {} samples",
            state.audio_settings_state.selected_backend,
            state.audio_settings_state.sample_rate,
            state.audio_settings_state.buffer_size
        ),
        "Configure",
        ("qs_audio", 2),
    ) {
        state.control_panel_state.top_tab = DashboardTopTab::Settings;
        state.control_panel_state.sub_tab = DashboardSubTab::Audio;
    }

    ui.add_space(12.0);
    ui.label(
        RichText::new(format!("Current workspace mode: {}", mode_name))
            .size(11.5)
            .color(TEXT_MUTED),
    );
}

fn render_my_projects(ui: &mut Ui, state: &mut AppState, is_open: &mut bool) {
    let query = state.control_panel_state.search_query.trim().to_lowercase();
    let files = discover_workspace_projects();
    if files.is_empty() {
        ui.add_space(12.0);
        ui.label(
            RichText::new("No .oplf or .opsf project files found in the current workspace directory.")
                .size(12.0)
                .color(TEXT_MUTED),
        );
        return;
    }
    for (idx, path) in files.iter().enumerate() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Project");
        if !query.is_empty() && !name.to_lowercase().contains(&query) {
            continue;
        }
        let subtitle = path.display().to_string();
        if render_project_row(ui, name, &subtitle, "Open", ("my_proj", idx)) {
            if project::load(state, path).is_ok() {
                state.project_path = Some(path.clone());
                state.project_dirty = false;
                state.control_panel_state.record_recent(path);
                *is_open = false;
            }
        }
    }
}

fn render_templates(ui: &mut Ui, state: &mut AppState, is_open: &mut bool) {
    let query = state.control_panel_state.search_query.trim().to_lowercase();
    let templates = [
        ("Empty OpenLive Session", "4 tracks · Session Matrix · 140 BPM", AppMode::OpenLive),
        ("Empty OpenStudio Arrangement", "Linear Timeline · Arranger View · 140 BPM", AppMode::OpenStudio),
    ];
    for (idx, (name, desc, mode)) in templates.iter().enumerate() {
        if !query.is_empty() && !name.to_lowercase().contains(&query) {
            continue;
        }
        if render_project_row(ui, name, desc, "Create", ("tpl", idx)) {
            menu_bar::new_project(state);
            state.mode = *mode;
            *is_open = false;
        }
    }
}

// =========================================================================
// SUB-VISTAS DE SETTINGS
// =========================================================================

fn render_audio_settings(ui: &mut Ui, state: &mut AppState) {
    render_section_title(ui, "Audio & Hardware");
    ui.add_space(10.0);

    let backend_label = format!("{:?}", state.audio_settings_state.selected_backend);
    if render_setting_row(ui, "Driver Model", &backend_label, ("aud_backend", 0)) {
        state.audio_settings_state.selected_backend = match state.audio_settings_state.selected_backend {
            AudioBackend::HikaruNative => AudioBackend::PipeWire,
            AudioBackend::PipeWire => AudioBackend::Jack,
            AudioBackend::Jack => AudioBackend::Alsa,
            AudioBackend::Alsa => AudioBackend::PulseAudio,
            AudioBackend::PulseAudio => AudioBackend::HikaruNative,
        };
    }

    let dev_name = state.audio_settings_state.selected_device.clone();
    if render_setting_row(ui, "Output Device", &dev_name, ("aud_dev", 1)) {
        let devs = &state.audio_settings_state.available_devices;
        if !devs.is_empty() {
            let idx = devs
                .iter()
                .position(|d| *d == state.audio_settings_state.selected_device)
                .unwrap_or(0);
            state.audio_settings_state.selected_device = devs[(idx + 1) % devs.len()].clone();
        }
    }

    let sr_str = format!("{} Hz", state.audio_settings_state.sample_rate);
    if render_setting_row(ui, "Sample Rate", &sr_str, ("aud_sr", 2)) {
        state.audio_settings_state.sample_rate = match state.audio_settings_state.sample_rate {
            44100 => 48000,
            48000 => 96000,
            96000 => 192000,
            _ => 44100,
        };
    }

    let buf_str = format!("{} samples", state.audio_settings_state.buffer_size);
    if render_setting_row(ui, "Buffer Size", &buf_str, ("aud_buf", 3)) {
        state.audio_settings_state.buffer_size = match state.audio_settings_state.buffer_size {
            64 => 128,
            128 => 256,
            256 => 512,
            512 => 1024,
            _ => 64,
        };
    }

    ui.add_space(12.0);
    let latency_ms = (state.audio_settings_state.buffer_size as f32
        / state.audio_settings_state.sample_rate.max(1) as f32)
        * 1000.0;
    ui.label(
        RichText::new(format!("Estimated output latency: {:.2} ms", latency_ms))
            .size(11.5)
            .color(TEXT_MUTED),
    );
}

fn render_plugins_tab(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        let scan_label = if state.plugin_settings_state.is_scanning {
            "Scanning..."
        } else {
            "Rescan Plugins"
        };
        let (btn_rect, _) = ui.allocate_exact_size(vec2(124.0, 26.0), Sense::hover());
        if paint_bitwig_button(ui, btn_rect, scan_label, ("plg_rescan", 0)) {
            state.plugin_settings_state.start_scan();
        }
        ui.add_space(10.0);
        let count = state.plugin_settings_state.discovered_plugins.len();
        ui.label(
            RichText::new(format!(
                "{} discovered · {}",
                count, state.plugin_settings_state.status_message
            ))
            .size(11.5)
            .color(TEXT_MUTED),
        );
    });

    ui.add_space(10.0);

    let query = state.control_panel_state.search_query.trim().to_lowercase();
    let plugins = state.plugin_settings_state.discovered_plugins.clone();
    if plugins.is_empty() {
        ui.label(
            RichText::new("No external VST3/CLAP plugins scanned yet. Click \"Rescan Plugins\" to scan standard system paths.")
                .size(11.5)
                .color(TEXT_MUTED),
        );
        return;
    }

    for (idx, plugin) in plugins.iter().enumerate() {
        if !query.is_empty()
            && !plugin.name.to_lowercase().contains(&query)
            && !plugin.vendor.to_lowercase().contains(&query)
        {
            continue;
        }
        let subtitle = format!("{:?} · {}", plugin.format, plugin.vendor);
        let _ = render_project_row(ui, &plugin.name, &subtitle, "Ready", ("vst_row", idx));
    }
}

fn render_native_devices(ui: &mut Ui, state: &mut AppState) {
    render_section_title(ui, "Hikaru Native Devices");
    ui.add_space(8.0);
    let query = state.control_panel_state.search_query.trim().to_lowercase();
    let devices = [
        ("Hikaru Wavetable", "Polyphonic wavetable synthesizer with 3D terrain visualizer"),
        ("Hikaru Sampler", "Multi-mode audio sample player & slicer"),
        ("Hikaru EQ", "Parametric equalizer for track & master shaping"),
        ("Hikaru Compressor", "Dynamics processor with threshold, ratio & makeup"),
        ("Hikaru Reverb", "Algorithmic stereo space reverb"),
        ("Hikaru Delay", "Stereo tempo-synced delay"),
    ];
    for (idx, (name, desc)) in devices.iter().enumerate() {
        if !query.is_empty() && !name.to_lowercase().contains(&query) {
            continue;
        }
        let _ = render_project_row(ui, name, desc, "Built-in", ("nat_dev", idx));
    }
}

fn render_mcp_tab(ui: &mut Ui, state: &mut AppState) {
    render_section_title(ui, "MCP Servers (Model Context Protocol)");
    ui.add_space(8.0);
    ui.label(
        RichText::new(&state.mcp_port_settings_state.status_message)
            .size(12.0)
            .color(TEXT_MUTED),
    );
    ui.add_space(12.0);
    let (rect, _) = ui.allocate_exact_size(vec2(200.0, 26.0), Sense::hover());
    if paint_bitwig_button(ui, rect, "Configure MCP Ports...", ("mcp_open_btn", 0)) {
        state.mcp_port_settings_state.open();
    }
}

fn render_interface_tab(ui: &mut Ui, state: &mut AppState) {
    render_section_title(ui, "Interface & Workspace");
    ui.add_space(10.0);

    let mode_str = match state.mode {
        AppMode::OpenLive => "OpenLive (Session Matrix)",
        AppMode::OpenStudio => "OpenStudio (Linear Arranger)",
    };
    if render_setting_row(ui, "Active Mode", mode_str, ("ui_mode", 0)) {
        state.mode = match state.mode {
            AppMode::OpenLive => AppMode::OpenStudio,
            AppMode::OpenStudio => AppMode::OpenLive,
        };
    }

    let browser_str = if state.show_explorer { "Visible" } else { "Hidden" };
    if render_setting_row(ui, "File Browser", browser_str, ("ui_browser", 1)) {
        state.show_explorer = !state.show_explorer;
    }

    let rack_str = if state.show_dsp_rack { "Visible" } else { "Hidden" };
    if render_setting_row(ui, "Device Panel (F10)", rack_str, ("ui_rack", 2)) {
        state.show_dsp_rack = !state.show_dsp_rack;
    }

    let roll_str = if state.show_piano_roll { "Visible" } else { "Hidden" };
    if render_setting_row(ui, "Note Editor (F11)", roll_str, ("ui_roll", 3)) {
        state.show_piano_roll = !state.show_piano_roll;
    }
}

// =========================================================================
// SUB-VISTAS DE HELP
// =========================================================================

fn render_about_tab(ui: &mut Ui) {
    ui.horizontal(|ui| {
        energy_core_logo_widget(ui, 54.0);
        ui.add_space(14.0);
        ui.vertical(|ui| {
            ui.label(
                RichText::new("Hikaru OpenLive & OpenStudio")
                    .size(16.0)
                    .strong()
                    .color(Color32::WHITE),
            );
            ui.label(
                RichText::new(format!("Version {}", VERSION))
                    .size(12.0)
                    .color(ACCENT_ORANGE),
            );
            ui.label(
                RichText::new("Hybrid clip-launcher & linear arrangement DAW")
                    .size(12.0)
                    .color(TEXT_MUTED),
            );
        });
    });

    ui.add_space(18.0);
    ui.label(
        RichText::new("License: GNU AGPL-3.0 (Application) · LGPL-3.0 (Project File Crates)")
            .size(11.5)
            .color(TEXT_SECONDARY),
    );
    ui.add_space(4.0);
    ui.label(
        RichText::new("Project formats: .oplf (OpenLive Session) and .opsf (OpenStudio Timeline)")
            .size(11.5)
            .color(TEXT_MUTED),
    );
}

fn render_shortcuts_tab(ui: &mut Ui) {
    render_section_title(ui, "Keyboard Shortcuts");
    ui.add_space(8.0);
    let shortcuts = [
        ("Space", "Play / Stop transport"),
        ("Tab", "Switch between Arrange / Mix / Edit views"),
        ("F9", "Toggle Session Matrix / Arranger View"),
        ("F10", "Toggle Device Panel"),
        ("F11", "Toggle Note Editor / Piano Roll"),
        ("Esc", "Close Dashboard / Modal"),
    ];
    for (key, desc) in shortcuts {
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(vec2(90.0, 22.0), Layout::left_to_right(Align::Center), |ui| {
                ui.label(RichText::new(key).size(11.5).strong().color(ACCENT_ORANGE));
            });
            ui.label(RichText::new(desc).size(11.5).color(TEXT_PRIMARY));
        });
    }
}

// =========================================================================
// HELPERS DE DIBUJO ESTILO BITWIG
// =========================================================================

fn render_section_title(ui: &mut Ui, title: &str) {
    ui.label(
        RichText::new(title)
            .size(14.0)
            .strong()
            .color(Color32::from_rgb(0xE8, 0xE8, 0xE8)),
    );
}

fn render_project_row(
    ui: &mut Ui,
    title: &str,
    subtitle: &str,
    action_label: &str,
    id_salt: (&str, usize),
) -> bool {
    let row_h = 36.0;
    let w = ui.available_width();
    let (row_rect, row_resp) = ui.allocate_exact_size(vec2(w, row_h), Sense::hover());

    if row_resp.hovered() {
        ui.painter().rect_filled(row_rect, CornerRadius::same(3), DASH_ROW_HOVER);
    }

    // Icono de documento / proyecto Bitwig minimalista
    let icon_rect = Rect::from_center_size(
        pos2(row_rect.min.x + 12.0, row_rect.center().y),
        vec2(12.0, 14.0),
    );
    let icon_col = Color32::from_rgb(0xB8, 0xB8, 0xB8);
    ui.painter().rect_filled(icon_rect, CornerRadius::same(1), icon_col);
    ui.painter().line_segment(
        [
            pos2(icon_rect.min.x + 2.5, icon_rect.min.y + 4.5),
            pos2(icon_rect.max.x - 2.5, icon_rect.min.y + 4.5),
        ],
        Stroke::new(1.0_f32, DASH_BG),
    );
    ui.painter().line_segment(
        [
            pos2(icon_rect.min.x + 2.5, icon_rect.min.y + 8.0),
            pos2(icon_rect.max.x - 2.5, icon_rect.min.y + 8.0),
        ],
        Stroke::new(1.0_f32, DASH_BG),
    );

    // Título (y subtítulo sutil si entra)
    let btn_w = 84.0;
    let btn_h = 22.0;
    let btn_rect = Rect::from_center_size(
        pos2(row_rect.max.x - btn_w * 0.5 - 4.0, row_rect.center().y),
        vec2(btn_w, btn_h),
    );

    let text_x = row_rect.min.x + 28.0;
    ui.painter().text(
        pos2(text_x, row_rect.center().y - 5.5),
        Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(12.0),
        Color32::from_rgb(0xE4, 0xE4, 0xE4),
    );
    ui.painter().text(
        pos2(text_x, row_rect.center().y + 7.5),
        Align2::LEFT_CENTER,
        subtitle,
        egui::FontId::proportional(10.0),
        Color32::from_rgb(0x82, 0x82, 0x82),
    );

    paint_bitwig_button(ui, btn_rect, action_label, id_salt)
}

fn render_setting_row(
    ui: &mut Ui,
    label: &str,
    value: &str,
    id_salt: (&str, usize),
) -> bool {
    let row_h = 34.0;
    let w = ui.available_width();
    let (row_rect, _) = ui.allocate_exact_size(vec2(w, row_h), Sense::hover());

    ui.painter().text(
        pos2(row_rect.min.x + 2.0, row_rect.center().y),
        Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.0),
        TEXT_MUTED,
    );

    let btn_w = (w * 0.56).clamp(180.0, 320.0);
    let btn_rect = Rect::from_center_size(
        pos2(row_rect.max.x - btn_w * 0.5 - 4.0, row_rect.center().y),
        vec2(btn_w, 24.0),
    );
    paint_bitwig_button(ui, btn_rect, value, id_salt)
}

fn paint_bitwig_button(ui: &mut Ui, rect: Rect, label: &str, id_salt: (&str, usize)) -> bool {
    let resp = ui.interact(rect, ui.id().with(id_salt), Sense::click());
    let fill = if resp.is_pointer_button_down_on() {
        ACTIVE_BG
    } else if resp.hovered() {
        DASH_BTN_HOVER
    } else {
        DASH_BTN_BG
    };

    ui.painter().rect(
        rect,
        CornerRadius::same(3),
        fill,
        Stroke::new(1.0_f32, DASH_BTN_BORDER),
        StrokeKind::Inside,
    );

    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(11.5),
        Color32::from_rgb(0xE4, 0xE4, 0xE4),
    );

    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    resp.clicked()
}
