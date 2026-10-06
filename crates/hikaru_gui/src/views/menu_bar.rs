use std::path::PathBuf;

use gpui_kit::component::*;
use gpui_kit::component::button::Button;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, AppMode, HikaruApp, OpenLiveView, update_state};
use crate::project;

/// Nombres de los menús, en el orden en que se dibujan.
///
/// El índice de esta lista es lo que guarda `AppState::open_menu` como ESPEJO
/// del popover, así que agregar uno en el medio mueve los demás.
pub const MENUS: [&str; 5] = ["FILE", "EDIT", "VIEW", "SETTINGS", "HELP"];

/// Índice de `FILE`: es el único que lleva el punto de "sin guardar", así que
/// tiene nombre propio y no un `0` suelto.
const FILE_MENU: usize = 0;

/// Radio del punto de "sin guardar", en px. Chiquito a propósito: informa, no
/// grita.
const DIRTY_DOT_PX: f32 = 4.0;

/// Un ítem de menú: etiqueta visible y acción.
///
/// Las acciones que todavía no están implementadas llevan `_ => {}` y no un
/// TODO silencioso: `Export Audio`, `Undo`/`Redo` y `Cut`/`Copy`/`Paste` son
/// funcionalidades pendientes, no bugs.
type MenuItem = (String, Box<dyn Fn(&mut App)>);

pub fn render(cx: &mut Context<HikaruApp>) -> impl IntoElement {
    let (open_menu, dirty, mode) = {
        let app = state(cx).read(cx);
        (app.open_menu, app.project_dirty, app.mode)
    };

    h_flex()
        .id("menu_bar")
        .bg(rgb(0x2A2A2A))
        .px(px(4.0))
        .pb(px(2.0))
        .children(MENUS.iter().enumerate().map(|(idx, name)| {
            div()
                .relative()
                // `dropdown_menu` (y no un `div().absolute()` hecho a mano)
                // porque el popover sabe anclarse al botón, cerrarse con Esc o
                // con un click afuera y navegar con flechas. Un dropdown
                // posicionado a mano quedó aplastado a una franja: el bloque
                // contenedor del botón mide lo que el botón, así que el hijo
                // absoluto se layoutaba contra un padre sin espacio.
                .child(
                    Button::new(format!("menu_{}", name.to_lowercase()))
                        .label((*name).to_string())
                        .compact()
                        .bg(if open_menu == Some(idx) {
                            rgb(0x4A4A4A)
                        } else {
                            rgb(0x3D3D3D)
                        })
                        .text_color(rgb(0xE0E0E0))
                        .dropdown_menu(move |menu, _window, _cx| {
                            // Los ítems se arman en el momento de abrir, no en
                            // el de render: el rótulo de About depende del modo
                            // vigente y el popover se construye una sola vez.
                            menu_items(idx, mode)
                                .into_iter()
                                .fold(menu, |menu, (label, action)| {
                                    menu.item(
                                        PopupMenuItem::new(label).on_click(move |_, _, cx| {
                                            action(cx)
                                        }),
                                    )
                                })
                        })
                        // Espejo del estado del popover en `AppState`. NO es la
                        // fuente de verdad (esa vive adentro del popover): está
                        // para que el resto de la app pueda leer qué menú está
                        // abierto y para que los tests lo asserten.
                        //
                        // El `else if` importa: al abrir un menú el anterior se
                        // cierra, y sin la comparación su `on_open_change(false)`
                        // borraría el estado del recién abierto.
                        .on_open_change(move |&open, _, cx| {
                            update_state(cx, |state| {
                                state.open_menu = if open {
                                    Some(idx)
                                } else if state.open_menu == Some(idx) {
                                    None
                                } else {
                                    state.open_menu
                                };
                            });
                        }),
                )
                // Punto de "sin guardar" al lado de FILE: la señal mínima para
                // no perder trabajo por un Ctrl+S de memoria. Es un elemento
                // propio y no parte del label del botón para poder pedirlo
                // desde un test.
                .when(
                    idx == FILE_MENU && dirty,
                    |this| {
                        this.child(
                            div()
                                .id("menu_file_dirty")
                                // Sin esto el test no lo encuentra: `try_find`
                                // sólo ve elementos registrados.
                                .test_support()
                                .absolute()
                                .top(px(2.0))
                                .left(px(2.0))
                                .size(px(DIRTY_DOT_PX))
                                .rounded_full()
                                .bg(rgb(0xE0A030)),
                        )
                    },
                )
        }))
}

/// Los ítems del menú `idx`, en orden.
///
/// El orden importa para los tests: el popover identifica cada ítem por índice,
/// y un ítem insertado en el medio mueve los que quedan abajo.
fn menu_items(idx: usize, mode: AppMode) -> Vec<MenuItem> {
    match idx {
        0 => vec![
            (
                "📄 New Project".to_string(),
                Box::new(new_project) as Box<dyn Fn(&mut App)>,
            ),
            ("📂 Open Project...".to_string(), Box::new(open_project)),
            (
                "💾 Save".to_string(),
                Box::new(|cx: &mut App| save_project(cx, false)),
            ),
            (
                "💾 Save As...".to_string(),
                Box::new(|cx: &mut App| save_project(cx, true)),
            ),
            (
                "🎵 Export Audio (WAV/FLAC)...".to_string(),
                Box::new(|_: &mut App| {}),
            ),
            (
                "❌ Exit".to_string(),
                Box::new(|_: &mut App| std::process::exit(0)),
            ),
        ],
        1 => vec![
            ("↩ Undo".to_string(), Box::new(|_: &mut App| {})),
            ("↪ Redo".to_string(), Box::new(|_: &mut App| {})),
            ("✂ Cut".to_string(), Box::new(|_: &mut App| {})),
            ("📋 Copy".to_string(), Box::new(|_: &mut App| {})),
            ("📋 Paste".to_string(), Box::new(|_: &mut App| {})),
        ],
        2 => vec![
            (
                "🔲 Session Matrix (Tab)".to_string(),
                Box::new(|cx: &mut App| {
                    update_state(cx, |state| {
                        state.mode = AppMode::OpenLive;
                        state.openlive_view = OpenLiveView::SessionMatrix;
                    });
                }),
            ),
            (
                "🎼 Arranger View (Tab)".to_string(),
                Box::new(|cx: &mut App| {
                    update_state(cx, |state| {
                        state.mode = AppMode::OpenLive;
                        state.openlive_view = OpenLiveView::ArrangerView;
                    });
                }),
            ),
            (
                "🎹 Playlist / Timeline".to_string(),
                Box::new(|cx: &mut App| {
                    update_state(cx, |state| {
                        state.mode = AppMode::OpenStudio;
                    });
                }),
            ),
            (
                "🎚 Arranger / Mixer Columns (F9)".to_string(),
                Box::new(|cx: &mut App| {
                    update_state(cx, |state| {
                        if state.mode == AppMode::OpenLive {
                            state.openlive_view = match state.openlive_view {
                                OpenLiveView::SessionMatrix => OpenLiveView::ArrangerView,
                                OpenLiveView::ArrangerView => OpenLiveView::SessionMatrix,
                            };
                        }
                    });
                }),
            ),
            (
                "🎚 DSP Rack (F10)".to_string(),
                Box::new(|cx: &mut App| {
                    update_state(cx, |state| {
                        state.show_dsp_rack = !state.show_dsp_rack;
                    });
                }),
            ),
        ],
        3 => vec![
            (
                "🔊 Audio Setup (JACK/ALSA/PipeWire)...".to_string(),
                Box::new(|cx: &mut App| {
                    update_state(cx, |state| {
                        state.audio_settings_state.is_open = true;
                    });
                }),
            ),
            (
                "🔌 External VST3 / CLAP Plugin Settings...".to_string(),
                Box::new(|cx: &mut App| {
                    update_state(cx, |state| {
                        state.plugin_settings_state.is_open = true;
                    });
                }),
            ),
        ],
        _ => {
            let label = match mode {
                AppMode::OpenLive => "ℹ About Hikaru OpenLive",
                AppMode::OpenStudio => "ℹ About Hikaru OpenStudio",
            };
            vec![(
                label.to_string(),
                Box::new(|cx: &mut App| {
                    update_state(cx, |state| {
                        state.show_about = true;
                    });
                }) as Box<dyn Fn(&mut App)>,
            )]
        }
    }
}

/// Id del popover de gpui-kit, reexportado para que los tests no lo escriban
/// a mano y no se desincronicen con la librería.
pub const POPUP_ID: &str = "popup-menu";

// =========================================================================
// ACCIONES DE PROYECTO (FILE)
// =========================================================================

/// `File > New Project`: deja el estado como recién salido de la app.
///
/// Pide confirmación si hay algo sin guardar: perder una sesión por un click
/// accidental es la peor forma de "funciona".
fn new_project(cx: &mut App) {
    let dirty = state(cx).read(cx).project_dirty;

    if dirty && !confirm_discard() {
        return;
    }

    update_state(cx, |state| {
        project::reset(state);
        state.project_path = None;
        state.project_dirty = false;
    });
    println!("[PROJECT] proyecto nuevo en memoria (sin archivo)");
}

/// `File > Open Project...`: diálogo de archivo y carga.
///
/// El error se guarda en una variable del closure en vez de propagarse con `?`:
/// `Entity::update` no devuelve el valor de la closure.
fn open_project(cx: &mut App) {
    let current_path = state(cx).read(cx).project_path.clone();
    let Some(path) = project::ask_open_path(current_path.as_deref()) else {
        return;
    };

    let mut failure: Option<project::ProjectError> = None;
    state(cx).update(cx, |app_state, _| {
        if let Err(err) = project::load(app_state, &path) {
            failure = Some(err);
        }
    });

    match failure {
        None => {
            let saved = path.clone();
            update_state(cx, |state| {
                state.project_path = Some(saved.clone());
                state.project_dirty = false;
            });
            println!("[PROJECT] proyecto cargado: {}", saved.display());
        }
        Some(err) => eprintln!("[PROJECT] no se pudo abrir: {}", err),
    }
}

/// `File > Save` / `File > Save As...`.
///
/// `force_dialog` es el `Save As`: sin archivo previo, o porque el usuario lo
/// pidió explícitamente, siempre se pregunta dónde.
fn save_project(cx: &mut App, force_dialog: bool) {
    let current = state(cx).read(cx).project_path.clone();

    let target = match (&current, force_dialog) {
        (Some(path), false) => path.clone(),
        _ => match project::ask_save_path(current.as_deref()) {
            Some(path) => path,
            None => return,
        },
    };

    let mut failure: Option<project::ProjectError> = None;
    let mut written: Option<PathBuf> = None;
    state(cx).update(cx, |app_state, _| match project::save(app_state, &target) {
        Ok(path) => written = Some(path),
        Err(err) => failure = Some(err),
    });

    match (failure, written) {
        (None, Some(saved)) => {
            update_state(cx, |state| {
                state.project_path = Some(saved.clone());
                state.project_dirty = false;
            });
            println!("[PROJECT] proyecto guardado: {}", saved.display());
        }
        (Some(err), _) => eprintln!("[PROJECT] no se pudo guardar: {}", err),
        // Sin escribir y sin error no puede pasar: `save` siempre devuelve uno
        // de los dos. Queda logueado por si mañana cambia la firma.
        (None, None) => eprintln!("[PROJECT] save no devolvió resultado"),
    }
}

/// Confirmación de descarte. Sin modales propios en la app, se usa el diálogo
/// del sistema: es feo, pero es el que no puede quedar pegado atrás de la
/// ventana.
fn confirm_discard() -> bool {
    rfd::MessageDialog::new()
        .set_title("Hikaru OpenLive")
        .set_description("El proyecto tiene cambios sin guardar.\n\n¿Descartarlos?")
        .set_buttons(rfd::MessageButtons::YesNo)
        .show()
        == rfd::MessageDialogResult::Yes
}