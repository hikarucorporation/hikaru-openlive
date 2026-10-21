// crates/hikaru_gui/src/views/dsp_rack.rs

use gpui_kit::component::*;
use gpui_kit::component::button::{Button, ButtonRounded};
use gpui_kit::component::input::Input;
use gpui_kit::component::label::Label;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::Size;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{InteractiveElement as _, StatefulInteractiveElement as _, Styled as _};
use gpui_kit::*;

use crate::app::{state, HikaruApp, ParamDragState};
use crate::ui::text_style_scope::TextStyleScope;
use crate::views::external_plugins_settings::capture_host_parent;
use crate::views::hikaru_native_plugins::hikaru_opendms::rack_card::render_rack_card;
use crate::views::hikaru_native_plugins::hikaru_openeq3;
use crate::views::hikaru_native_plugins::hikaru_openwavetable::open_wavetable;
use crate::views::mixer::{DspSlot, ExternalParamsStatus};

pub const RACK_HEIGHT: f32 = 340.0;

pub const CARD_WIDTH: f32 = 388.0;

const CARD_PADDING: f32 = 6.0;

const CARD_GAP: f32 = 6.0;

const CARD_HEADER_HEIGHT: f32 = 24.0;

const CARD_FOOTER_HEIGHT: f32 = 50.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginEntry {
    pub category: &'static str,
    pub name: &'static str,
    pub label: &'static str,
}

pub const PLUGIN_CATALOG: [PluginEntry; 3] = [
    PluginEntry { category: "Efectos", name: "OpenEQ3", label: "OpenEQ3" },
    PluginEntry { category: "Generadores", name: "OpenWavetable", label: "Hikaru OpenWavetable" },
    PluginEntry { category: "Generadores", name: "Hikaru OpenDMS", label: "Hikaru OpenDMS" },
];

/// Filtro de formato del navegador de plugins del rack.
///
/// Vive en el estado global de la app (`AppState::plugin_browser_filter`)
/// porque el navegador se reconstruye en cada frame: un valor local no
/// sobreviviría al primer `notify` y el chip activo parpadearía.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PluginBrowserFilter {
    #[default]
    All,
    Native,
    Vst3,
    Clap,
}

impl PluginBrowserFilter {
    const TABS: [(Self, &'static str); 4] = [
        (Self::All, "Todos"),
        (Self::Native, "Nativos"),
        (Self::Vst3, "VST3"),
        (Self::Clap, "CLAP"),
    ];

    fn matches_external(self, format: hikaru_plugin_host::PluginFormat) -> bool {
        match self {
            Self::All => true,
            Self::Native => false,
            Self::Vst3 => format == hikaru_plugin_host::PluginFormat::VST3,
            Self::Clap => format == hikaru_plugin_host::PluginFormat::CLAP,
        }
    }

    fn show_natives(self) -> bool {
        matches!(self, Self::All | Self::Native)
    }
}

/// Cierra el navegador de plugins del track dado, sea cual sea el slot que
/// lo abrió. Lo llaman el backdrop, la ✕, la tecla de cierre y cada
/// asignación: un navegador abierto tras asignar apuntaría a un slot cuyo
/// contenido ya cambió.
fn close_browser(cx: &mut gpui_kit::App, track_idx: usize) {
    state(cx).update(cx, |state, cx| {
        if let Some(track) = state.tracks_mut().get_mut(track_idx) {
            for slot in track.effects.iter_mut() {
                slot.menu_open = false;
            }
        }
        state.add_slot_menu_open = false;
        cx.notify();
    });
}

fn rack_button(id: impl Into<gpui_kit::ElementId>) -> Button {
    Button::new(id)
        .rounded(ButtonRounded::None)
        .compact()
        .with_size(gpui_kit::component::Size::XSmall)
}

fn assign_plugin(cx: &mut gpui_kit::App, track_idx: usize, slot_idx: usize, name: &str) {
    let st = state(cx);
    let name = name.to_string();
    cx.defer(move |cx| {
        st.update(cx, |state, cx| {
            if let Some(slot) = state.slot_mut(track_idx, slot_idx) {
                slot.name = name.clone();
                // Cambiar de plugin limpia cualquier error de carga anterior:
                // el mensaje era de otro plugin y ya no aplica.
                slot.load_error = None;
                slot.menu_open = false;
                slot.options_open = false;
            }
            state.selected_slot_index = slot_idx;
            state.add_slot_menu_open = false;
            cx.notify();
        });
    });
}

/// Asigna un plugin externo (VST3/CLAP) a un slot.
///
/// No carga el binario: guarda la metadata del descriptor. Instanciarlo en el
/// hilo de la GUI los dejaría haciendo audio fuera del hilo de audio (ver
/// `hikaru_plugin_host::PluginInstance::process`), así que la carga real la
/// hace el motor cuando este slot entre en la cadena de efectos. Esa
/// separación es la que permite que un plugin roto se marque en el slot en vez
/// de romper la apertura del proyecto.
fn assign_external_plugin(
    cx: &mut gpui_kit::App,
    track_idx: usize,
    slot_idx: usize,
    descriptor: hikaru_plugin_host::PluginDescriptor,
) {
    let st = state(cx);
    let audio_proxy = st.read(cx).audio_proxy.clone();
    // El motor indexa sus pistas 0..=15 SIN master, mientras que `track_idx`
    // viene del mixer, que mete el MASTER en 0. Sin esta conversión la cadena
    // se inserta una pista más abajo de la que mezcla el clip, y el plugin
    // procesa siempre un buffer mudo. Se resuelve acá, antes del `defer`, para
    // capturar la pista que estaba seleccionada al hacer clic (y no la que
    // quede seleccionada cuando corra el deferred).
    let engine_idx = st.read(cx).engine_track_index();
    cx.defer(move |cx| {
        st.update(cx, |state, cx| {
            if let Some(slot) = state.slot_mut(track_idx, slot_idx) {
                slot.name = descriptor.name.clone();
                slot.external = Some(descriptor.clone());
                slot.load_error = None;
                // Plugin nuevo = parámetros nuevos: se tira la caché vieja y
                // se sondea en diferido (hilo aparte, ver `request_external_params`).
                slot.external_params.clear();
                slot.params_status = ExternalParamsStatus::Idle;
                slot.menu_open = false;
                slot.options_open = false;
            }
            state.request_external_params(&descriptor);
            state.selected_slot_index = slot_idx;
            state.add_slot_menu_open = false;
            // El master no tiene pista equivalente en el motor: una cadena de
            // insert no tiene dónde insertarse, así que se dice en el slot en
            // vez de mandar un comando que el motor no puede satisfacer.
            if engine_idx.is_none() {
                if let Some(slot) = state.slot_mut(track_idx, slot_idx) {
                    slot.load_error = Some("El master no tiene cadena de efectos".to_string());
                }
            }
            cx.notify();
        });
        // Cargar el plugin en el motor de audio (hilo de control del engine).
        // El `dlopen` + init corre en el engine, nunca en el hilo de la GUI.
        let Some(engine_idx) = engine_idx else {
            return;
        };
        audio_proxy.send(crate::audio_proxy::GuiCommand::LoadExternalPlugin {
            track_idx: engine_idx,
            slot_idx,
            descriptor,
        });
    });
}

/// Abre el editor nativo del plugin en una ventana hija de la del DAW.
///
/// Captura el handle nativo de GPUI en el click (`window` sólo existe en el
/// handler, no en el render) y lo reduce a `HostParentKind`: en sesión X11
/// la ventana del plugin nace hija de la del DAW (transient, centrada, sin
/// flash vacía ni cierres fantasma); en Wayland cae al flotante de XWayland
/// porque una superficie Wayland no puede emparentar ventanas X11. Todo el
/// trabajo pesado (`dlopen`, X11, embebido) corre en el hilo de la ventana:
/// el hilo principal vuelve de inmediato y nunca se cuelga.
fn open_external_editor(
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
    track_idx: usize,
    slot_idx: usize,
) {
    let descriptor = {
        let app = state(cx).read(cx);
        app.slot(track_idx, slot_idx).and_then(|slot| slot.external.clone())
    };
    let Some(descriptor) = descriptor else {
        return;
    };
    let parent = capture_host_parent(window);

    // La cinta que alimenta al editor se busca por la clave del MOTOR
    // `(pista, slot)`, y `track_idx` es el índice del mixer, que es uno más. Sin
    // esta conversión el editor se queda escuchando la pista de al lado y el
    // display muestra otra cosa (o nada).
    let tap_key = {
        let app = state(cx).read(cx);
        match app.engine_track_index() {
            Some(engine_idx) => (engine_idx, slot_idx),
            None => {
                eprintln!(
                    "[DSP Rack] La pista del mixer {} no tiene índice de motor; el editor abrirá sin audio",
                    track_idx
                );
                hikaru_plugin_host::tap::NO_SLOT
            }
        }
    };

    state(cx).update(cx, |state, cx| {
        state
            .plugin_settings_state
            .open_plugin(&descriptor, parent, tap_key);
        cx.notify();
    });
}

/// Cierra la ventana nativa del editor de un plugin por nombre.
///
/// El cierre va por nombre y no por slot porque `PluginSettingsState` indexa
/// las ventanas abiertas por nombre de plugin: es lo que permite que dos slots
/// con el mismo plugin compartan una sola ventana en vez de abrir dos.
fn close_external_editor(cx: &mut gpui_kit::App, plugin_name: &str) {
    let st = state(cx);
    st.update(cx, |state, _| {
        state.plugin_settings_state.close_plugin(plugin_name);
    });
}

/// Re-eleva los editores nativos abiertos sin robarles el foco al DAW.
///
/// Esto es la mitad "GPUI" del envoltorio tipo Fruity Wrapper. La otra mitad
/// son los hints de `WM_TRANSIENT_FOR` / `_NET_WM_STATE_ABOVE` que se aplican
/// al abrir la ventana (ver `external_plugins_settings`).
///
/// # Por qué hace falta si ya hay `_NET_WM_STATE_ABOVE`
///
/// El hint se aplica una vez, antes del `map`, y después manda el gestor de
/// ventanas. Cada vez que el usuario clickea cualquier parte del DAW —una
/// pista, un clip, un knob del rack— el WM sube su propia ventana y deja el
/// editor del plugin detrás: exactamente el síntoma que se quiere eliminar.
///
/// Cuando el DAW recupera la activación, se avisa a cada ventana abierta para
/// que se re-eleve. El re-elevado usa `ConfigureWindow(stack=Above)`, que
/// Re-elevar **sin** activar. Si focused el editor, el usuario perdería el foco
/// del knob que estaba por tocar.
///
/// No hay forma de "no enviar un Unfocus": GPUI no emite `LowerWindow`, lo que
/// baja la ventana del plugin es el propio WM reacting al click. La única
/// manera de que el editor vuelva a quedar arriba sin quitarle el foco al DAW
/// es pedir el re-apilado después del click, que es lo que hace esto.
pub fn keep_plugin_editors_above(cx: &mut gpui_kit::App) {
    let st = state(cx);
    let any_open = st.read(cx).plugin_settings_state.any_plugin_open();
    if !any_open {
        return;
    }
    st.update(cx, |state, _| {
        state.plugin_settings_state.raise_open_plugins();
    });
}

/// Fija un parámetro externo desde un knob del rack.
///
/// Escribe la caché del slot (lo que pintan los knobs), lo deja pendiente
/// para el motor (`flush_pending_external_params`) y la manda en vivo a la
/// ventana abierta si la hay (el editor nativo la refleja al instante).
/// Todo no bloqueante: si la ventana cayó, el gesto igual queda guardado.
///
/// Devuelve `false` cuando el gesto no cambió nada —id inexistente, o valor que
/// tras el snapping ya era el mismo— y en ese caso no se notifica ni se envía
/// nada. Eso es lo que evita el "arrastre lento": un `cx.notify()` reconstruye
/// la ventana entera, y hacerlo por cada píxel de un drag que en su mayoría no
/// mueve el valor convertía cada clic en dos reconstrucciones completas
/// (armar y desarmar el gesto) más una por evento.
fn commit_external_param(
    cx: &mut gpui_kit::App,
    track_idx: usize,
    slot_idx: usize,
    id: u32,
    value: f32,
) -> bool {
    let st = state(cx);

    // `applied` es el valor ya redondeado: es el que se muestra, el que va al
    // editor nativo y el que tiene que ir al motor. Mandar el `value` crudo
    // hacía que un parámetro discreto sonara en un valor y se viera en otro.
    let mut applied: Option<f32> = None;
    st.update(cx, |state, _| {
        if let Some(slot) = state.slot_mut(track_idx, slot_idx) {
            applied = slot.set_external_param(id, value);
        }
    });
    let Some(applied) = applied else {
        return false;
    };

    // Avisarle al editor nativo abierto, si lo hay. El `is_empty` va **antes**
    // de clonar el nombre: `HashMap::is_empty` no cuesta nada, mientras que la
    // `String` del nombre del plugin se clonaba en cada píxel del arrastre
    // aunque no hubiera ninguna ventana abierta (el caso normal).
    if st.read(cx).plugin_settings_state.any_plugin_open() {
        let plugin_name = st
            .read(cx)
            .slot(track_idx, slot_idx)
            .and_then(|slot| slot.external.as_ref().map(|ext| ext.name.clone()));
        if let Some(plugin_name) = plugin_name {
            st.update(cx, |state, _| {
                state
                    .plugin_settings_state
                    .send_param_to_open_plugin(&plugin_name, id, applied);
            });
        }
    }

    // Sólo ahora se repinta: el valor de la caché ya es el nuevo.
    st.update(cx, |_, cx| cx.notify());

    // Al motor de audio lo mandan los pendientes del slot, que se drenan una
    // vez por frame (`AppState::flush_pending_external_params`). Mandarlo acá
    // también sería mandarlo dos veces: el knob del rack y el del editor
    // nativo escriben en el mismo lugar.
    true
}

/// Empieza un drag de la barra de un parámetro: agarre en la posición de
/// ventana `(x, y)`.
///
/// Sin `cx.notify()`: `param_drag` no lo pinta nadie, así que notificar acá
/// reconstruía la ventana entera sin que cambiara un solo píxel. El gesto igual
/// queda vivo porque el seguimiento no depende de un repintar: los listeners se
/// registran **siempre** en cada paint del rack (ver
/// [`param_drag_keepalive`]), no sólo mientras hay arrastre.
fn param_drag_begin(
    cx: &mut gpui_kit::App,
    track_idx: usize,
    slot_idx: usize,
    id: u32,
    x: f32,
    y: f32,
) {
    let value = state(cx)
        .read(cx)
        .slot(track_idx, slot_idx)
        .and_then(|slot| {
            slot.external_params
                .iter()
                .find(|p| p.info.id == id)
                .map(|p| p.value)
        })
        .unwrap_or(0.0);
    state(cx).update(cx, |state, _| {
        state.param_drag = Some(ParamDragState::new(track_idx, slot_idx, id, x, y, value));
    });
}

/// Continúa el drag hasta la posición de ventana `(x, y)`.
///
/// El valor lo calcula [`ParamDragState::advance`], que devuelve `None` cuando
/// el gesto todavía no pasó el umbral de eje: en ese caso no se toca nada y no
/// se paga un `cx.notify()`.
///
/// El `ParamDragState` avanzado se escribe **antes** de confirmar el valor para
/// que el eje quede fijado en el estado global. `param_drag` no lo pinta nadie,
/// así que esa escritura no dispara un repintar por su cuenta.
fn param_drag_move(cx: &mut gpui_kit::App, x: f32, y: f32, fine: bool) {
    let st = state(cx);
    let current = st.read(cx).param_drag;
    let Some(mut drag) = current else {
        return;
    };
    let Some(value) = drag.advance(x, y, fine) else {
        return;
    };
    st.update(cx, |state, _| {
        state.param_drag = Some(drag);
    });
    commit_external_param(cx, drag.track, drag.slot, drag.param_id, value);
}

/// Cierra el drag. Sin `notify()` por el mismo motivo que en
/// [`param_drag_begin`]: el último `commit_external_param` que movió algo ya
/// notificó, y si el gesto no llegó a mover nada no hay nada que repintar.
fn param_drag_end(cx: &mut gpui_kit::App) {
    state(cx).update(cx, |state, _| {
        state.param_drag = None;
    });
}

/// Registra el seguimiento del arrastre a nivel de ventana.
///
/// # Por qué no `on_mouse_move` de la tarjeta
///
/// Los listeners de elemento de GPUI Kit se despachan **sólo si el hitbox del
/// elemento está bajo el cursor** (`div.rs`: el closure arranca con
/// `if phase == DispatchPhase::Bubble && hitbox.is_hovered(window)`). Con el
/// seguimiento en la tarjeta, en cuanto el cursor salía de ella el gesto se
/// congelaba a mitad de camino —y el `mouse_up` también se perdía, dejando
/// `param_drag` colgado para el movimiento de mouse que viniera después—. Como
/// la barra es horizontal y baja 10px, sacarla de la tarjeta era cuestión de
/// pocos píxeles: la mitad de la caja del arrastre se quedaba sin responder,
/// que es justo lo que se veía como lag.
///
/// `Window::on_mouse_event` no lleva esa condición: el gesto sigue aunque el
/// cursor salga de la tarjeta, de la ventana o de la pantalla.
///
/// # Por qué se registra siempre
///
/// `Window::on_mouse_event` borra sus listeners en cada frame renderizado y sólo
/// admite registrarse en fase de paint, así que no se puede registrar desde el
/// `mouse_down`. Y como `param_drag_begin` no notifica (no hay nada que
/// repintar), **no hay frame nuevo entre el clic y el primer movimiento**. Por
/// eso el registro va en el paint del rack y no se condiciona a que haya un
/// arrastre en curso: el listener de siempre es el que atiende el primer
/// movimiento. Cuando no hay gesto, el listener vuelve en su primera guarda.
fn register_param_drag_tracking(window: &mut gpui_kit::Window) {
    window.on_mouse_event(move |event: &MouseMoveEvent, _phase, _window, cx| {
        param_drag_move(
            cx,
            f32::from(event.position.x),
            f32::from(event.position.y),
            event.modifiers.shift,
        );
    });
    window.on_mouse_event(move |event: &MouseUpEvent, _phase, _window, cx| {
        // Sólo el izquierdo cierra el gesto: el derecho abre el menú de la
        // tarjeta y no debe abortar un arrastre que sigue vivo.
        if event.button == gpui_kit::MouseButton::Left {
            param_drag_end(cx);
        }
    });
}

/// Canvas invisible que mantiene vivo el seguimiento del arrastre.
///
/// `Window::on_mouse_event` borra sus listeners en cada frame renderizado, así
/// que hay que volver a registrar en cada frame. Este canvas lo hace desde su
/// fase de **paint**, que es la única desde la que la API lo permite
/// (`window.rs` hace `debug_assert_paint()`).
///
/// El invariante que sostiene el gesto: si se renderizó un frame, el paint de
/// este canvas corrió y el listener quedó registrado de nuevo; y si no se
/// renderizó ningún frame, entonces no se borró ningún listener. No hay forma
/// de quedarse sin seguimiento a mitad de arrastre.
///
/// No tiene hitbox (no registra interactividad), así que no intercepta el mouse
/// de la barra ni de los botones de la tarjeta.
fn param_drag_keepalive() -> AnyElement {
    canvas(
        |_bounds, _window, _cx| {},
        move |_bounds, _state, window, _cx| {
            register_param_drag_tracking(window);
        },
    )
    .absolute()
    .inset_0()
    .into_any_element()
}

fn open_plugin_menu(cx: &mut gpui_kit::App, track_idx: usize, slot_idx: usize) {
    state(cx).update(cx, |state, cx| {
        if let Some(track) = state.tracks_mut().get_mut(track_idx) {
            for (i, s) in track.effects.iter_mut().enumerate() {
                if i == slot_idx {
                    s.menu_open = true;
                    s.options_open = false;
                } else {
                    s.menu_open = false;
                    s.options_open = false;
                }
            }
        }
        state.selected_slot_index = slot_idx;
        state.add_slot_menu_open = false;
        cx.notify();
    });
}

fn cycle_card_menu(cx: &mut gpui_kit::App, track_idx: usize, slot_idx: usize) {
    let options_open = state(cx)
        .read(cx)
        .slot(track_idx, slot_idx)
        .is_some_and(|slot| slot.options_open);
    if options_open {
        open_plugin_menu(cx, track_idx, slot_idx);
    } else {
        open_options_menu(cx, track_idx, slot_idx);
    }
}

fn open_options_menu(cx: &mut gpui_kit::App, track_idx: usize, slot_idx: usize) {
    state(cx).update(cx, |state, cx| {
        if let Some(track) = state.tracks_mut().get_mut(track_idx) {
            for (i, s) in track.effects.iter_mut().enumerate() {
                if i == slot_idx {
                    s.options_open = !s.options_open;
                    s.menu_open = false;
                } else {
                    s.options_open = false;
                    s.menu_open = false;
                }
            }
        }
        state.selected_slot_index = slot_idx;
        state.add_slot_menu_open = false;
        cx.notify();
    });
}

fn remove_slot_at(cx: &mut gpui_kit::App, track_idx: usize, slot_idx: usize) {
    state(cx).update(cx, |state, cx| {
        let selected = state.selected_slot_index;
        let new_len = if let Some(track) = state.tracks_mut().get_mut(track_idx) {
            if slot_idx < track.effects.len() {
                track.effects.remove(slot_idx);
            }
            track.effects.len()
        } else {
            0
        };
        state.selected_slot_index = if new_len == 0 {
            0
        } else {
            selected.min(new_len - 1)
        };
        cx.notify();
    });
}

fn remove_selected_slot(cx: &mut gpui_kit::App, track_idx: usize) {
    state(cx).update(cx, |state, cx| {
        let selected = state.selected_slot_index;
        if let Some(track) = state.tracks_mut().get_mut(track_idx) {
            if selected < track.effects.len() {
                track.effects.remove(selected);
                state.selected_slot_index = if track.effects.is_empty() {
                    0
                } else {
                    selected.min(track.effects.len() - 1)
                };
            }
        }
        cx.notify();
    });
}

fn insert_slot_at(cx: &mut gpui_kit::App, track_idx: usize, at_idx: usize) {
    state(cx).update(cx, |state, cx| {
        if let Some(track) = state.tracks_mut().get_mut(track_idx) {
            let new_id = track.effects.len();
            let insert_pos = at_idx.min(track.effects.len());
            track.effects.insert(insert_pos, DspSlot::new(new_id, "Empty Slot".to_string()));
            open_wavetable::log_initial_wavetable(&format!("slot t{track_idx}s{new_id}"));
            state.selected_slot_index = insert_pos;
        }
        cx.notify();
    });
}

fn move_slot(cx: &mut gpui_kit::App, track_idx: usize, from: usize, to: usize) {
    state(cx).update(cx, |state, cx| {
        if let Some(track) = state.tracks_mut().get_mut(track_idx) {
            if from < track.effects.len() && to < track.effects.len() {
                let slot = track.effects.remove(from);
                track.effects.insert(to, slot);
                state.selected_slot_index = to;
            }
        }
        cx.notify();
    });
}

pub fn render(cx: &mut Context<HikaruApp>) -> impl IntoElement {
    let (track_idx, selected_slot, cards, title) = {
        let app = state(cx).read(cx);
        let track_idx = app.safe_track_index();
        if !app.tracks().get(track_idx).is_some() {
            return v_flex().size_full().into_any_element();
        }
        let track = &app.tracks()[track_idx];

        (
            track_idx,
            app.selected_slot_index,
            // Proyección, no `track.effects.clone()`. La tarjeta sólo necesita
            // estos campos (ver `CardSnapshot`) y el editor de cada módulo lee
            // su propio estado por `track_idx`/`slot_idx`. El clonado entero
            // arrastraba la `WavetableEditor` de cada slot —y con ella los
            // `256 * TABLE_RESOLUTION` f32 de la tabla, ~2 MB por slot— en
            // CADA render. Con un fader de plugin abajo, cada `cx.notify()`
            // del arrastre pagaba esa copia y el gesto se arrastraba.
            track
                .effects
                .iter()
                .map(CardSnapshot::from_slot)
                .collect::<Vec<_>>(),
            rack_title(&track.name, app.matrix_state.selected_slot.map(|(_, scene)| scene)),
        )
    };

    let (open_browser, open_options) = {
        let app = state(cx).read(cx);
        let track = &app.tracks()[track_idx];
        let open_browser = track.effects.iter().position(|slot| slot.menu_open);
        let open_options = track.effects.iter().position(|slot| slot.options_open);
        (open_browser, open_options)
    };
    let options_left = open_options.map(|idx| card_x_offset(cards.len(), idx));

    div()
        .id("dsp_rack")
        .w_full()
        .flex()
        .flex_col()
        .child(
            div()
                .w_full()
                .h(px(RACK_HEIGHT))
                .flex()
                .flex_col()
                .bg(rgb(0x14141A))
                .border_t_1()
                .border_color(rgb(0x2A2A2A))
                .child(
                    div()
                        .w_full()
                        .h(px(20.0))
                        .flex()
                        .items_center()
                        .px(px(8.0))
                        .bg(rgb(0x1A1A1F))
                        .child(Label::new(title).text_xs().font_weight(FontWeight::BOLD).text_color(rgb(0x888888)))
                )
                .child(
                    div()
                        .id("dsp_strip")
                        .w_full()
                        .flex_1()
                        .min_h(px(RACK_HEIGHT - 26.0))
                        .overflow_x_scrollbar()
                        .child(
                            h_flex()
                                .h_full()
                                .gap(px(CARD_GAP))
                                .px(px(8.0))
                                .py(px(6.0))
                                .children(cards.iter().enumerate().map(|(idx, card)| {
                                    render_card(cx, track_idx, idx, card, idx == selected_slot)
                                }))
                                .child(render_add_slot_button(cx, track_idx, cards.len()))
                        )
                )
        )
        .when_some(open_browser, |this, idx| {
            this.child(render_plugin_browser(cx, track_idx, idx))
        })
        .when_some(open_options.zip(options_left), |this, (idx, left)| {
            this.child(render_options_menu(cx, track_idx, idx, left))
        })
        // Red de seguridad del seguimiento del arrastre. Va al final para que
        // quede por encima: es invisible y no registra hitbox, así que no
        // intercepta el mouse de las barras ni de los botones. Uno solo para
        // todo el rack, y no uno por tarjeta, para que sean un listener por
        // frame.
        .child(param_drag_keepalive())
        .into_any_element()
}

/// Lo único de un [`DspSlot`] que la tarjeta necesita para dibujarse.
///
/// Existe para no clonar la cadena entera en cada render (ver [`render`]).
/// `Clone` es barato: son dos `String` y una [`hikaru_plugin_host::PluginDescriptor`].
#[derive(Clone)]
struct CardSnapshot {
    name: String,
    active: bool,
    options_open: bool,
    external: Option<hikaru_plugin_host::PluginDescriptor>,
    load_error: Option<String>,
}

impl CardSnapshot {
    fn from_slot(slot: &DspSlot) -> Self {
        Self {
            name: slot.name.clone(),
            active: slot.active,
            options_open: slot.options_open || slot.menu_open,
            external: slot.external.clone(),
            load_error: slot.load_error.clone(),
        }
    }
}

/// X de la tarjeta `idx` dentro de la tira. Todas las tarjetas miden lo mismo,
/// así que sólo hace falta cuántas hay: por eso el conteo y no un `&[DspSlot]`.
fn card_x_offset(slot_count: usize, idx: usize) -> f32 {
    let before = idx.min(slot_count) as f32 * (CARD_WIDTH + CARD_GAP);
    8.0 + before
}

fn render_add_slot_button(cx: &mut Context<HikaruApp>, track_idx: usize, slot_count: usize) -> AnyElement {
    let menu_open = {
        let app = state(cx).read(cx);
        app.add_slot_menu_open
    };

    div()
        .id(format!("dsp_add_slot_{track_idx}"))
        .w(px(60.0))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(if menu_open { rgb(0x252530) } else { rgb(0x1C1C22) })
        .border_1()
        .border_color(if menu_open { rgb(0xFF6E00) } else { rgb(0x2A2A30) })
        .rounded(px(4.0))
        .on_click(move |_, _, cx| {
            state(cx).update(cx, |state, cx| {
                if let Some(track) = state.tracks_mut().get_mut(track_idx) {
                    let new_id = track.effects.len();
                    track.effects.push(DspSlot::new(new_id, "Empty Slot".to_string()));
                    open_wavetable::log_initial_wavetable(&format!("slot t{track_idx}s{new_id}"));
                    state.selected_slot_index = new_id;
                    if let Some(slot) = state.slot_mut(track_idx, new_id) {
                        slot.menu_open = true;
                    }
                }
                state.add_slot_menu_open = false;
                cx.notify();
            });
        })
        .child(
            Label::new("[ + ]")
                .text_sm()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0x888888))
        )
        .into_any_element()
}

fn render_card(
    cx: &mut Context<HikaruApp>,
    track_idx: usize,
    idx: usize,
    card: &CardSnapshot,
    is_selected: bool,
) -> AnyElement {
    let name = card.name.clone();
    let slot_active = card.active;
    let options_open = card.options_open;
    let external = card.external.clone();
    let load_error = card.load_error.clone();

    div()
        .id(format!("dsp_card_{track_idx}_{idx}"))
        .w(px(CARD_WIDTH))
        .h_full()
        .flex()
        .flex_col()
        .bg(if is_selected { rgb(0x252530) } else { rgb(0x1C1C22) })
        .border_1()
        .border_color(if is_selected { rgb(0xFF6E00) } else { rgb(0x2A2A30) })
        .rounded(px(4.0))
        .overflow_hidden()
        .on_click(move |_, _, cx| {
            state(cx).update(cx, |state, cx| {
                state.selected_slot_index = idx;
                cx.notify();
            });
        })
        .on_mouse_down(gpui_kit::MouseButton::Right, move |_, _, cx| {
            eprintln!("[Hikaru] card RIGHT t{track_idx}s{idx}");
            open_plugin_menu(cx, track_idx, idx);
        })
        .on_aux_click(move |_, _, cx| {
            eprintln!("[Hikaru] card AUX t{track_idx}s{idx}");
            open_plugin_menu(cx, track_idx, idx);
        })
        .child(render_card_header(cx, track_idx, idx, &name, slot_active, options_open, is_selected))
        .child(render_card_body(
            cx,
            track_idx,
            idx,
            &name,
            external.as_ref(),
            load_error.as_deref(),
        ))
        .child(render_card_footer(cx, track_idx, idx, &name))
        .into_any_element()
}

fn render_card_header(
    cx: &mut Context<HikaruApp>,
    track_idx: usize,
    idx: usize,
    name: &str,
    active: bool,
    options_open: bool,
    is_selected: bool,
) -> AnyElement {
    h_flex()
        .w_full()
        .h(px(CARD_HEADER_HEIGHT))
        .items_center()
        .gap(px(4.0))
        .px(px(CARD_PADDING))
        .bg(if is_selected { rgb(0x2A2A35) } else { rgb(0x222228) })
        .border_b_1()
        .border_color(rgb(0x2A2A30))
        .child(
            rack_button(format!("dsp_card_toggle_{track_idx}_{idx}"))
                .rounded(ButtonRounded::None)
                .label(if active { "●" } else { "○" })
                .compact()
                .when(active, |b| b.text_color(rgb(0x00FF88)))
                .when(!active, |b| b.text_color(rgb(0x666666)))
                .on_click(move |_, _, cx| {
                    state(cx).update(cx, |state, cx| {
                        if let Some(slot) = state.slot_mut(track_idx, idx) {
                            slot.active = !slot.active;
                        }
                        cx.notify();
                    });
                })
        )
        .child(
            Label::new(format!("{:02} {}", idx + 1, name))
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(if active { rgb(0xE0E0E0) } else { rgb(0x666666) })
                .flex_1()
                .truncate()
        )
        .child(
            div()
                .id(format!("dsp_card_menu_{track_idx}_{idx}"))
                .min_w(px(22.0))
                .px(px(4.0))
                .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| {
                    eprintln!("[Hikaru] triangle LEFT t{track_idx}s{idx}");
                    cycle_card_menu(cx, track_idx, idx);
                })
                .on_mouse_down(gpui_kit::MouseButton::Right, move |_, _, cx| {
                    eprintln!("[Hikaru] triangle RIGHT t{track_idx}s{idx}");
                    open_plugin_menu(cx, track_idx, idx);
                })
                .on_aux_click(move |_, _, cx| {
                    eprintln!("[Hikaru] triangle AUX t{track_idx}s{idx}");
                    open_plugin_menu(cx, track_idx, idx);
                })
                .child(
                    Label::new(if options_open { "▲" } else { "▼" })
                        .text_xs()
                        .text_color(rgb(0xE0E0E0)),
                )
                .into_any_element()
        )
        .into_any_element()
}

fn render_card_body(
    cx: &mut Context<HikaruApp>,
    track_idx: usize,
    idx: usize,
    name: &str,
    external: Option<&hikaru_plugin_host::PluginDescriptor>,
    load_error: Option<&str>,
) -> AnyElement {
    // Un plugin externo gana sobre el `match` por nombre: el nombre de un
    // VST3 podría coincidir con el de un interno, y el descriptor es la
    // identidad fiable.
    if let Some(descriptor) = external {
        return render_external_card(cx, track_idx, idx, descriptor, load_error);
    }

    match name {
        "OpenWavetable" => open_wavetable::render_module(cx, track_idx, idx),
        "Hikaru OpenDMS" => render_rack_card(cx, track_idx, idx),
        "OpenEQ3" => hikaru_openeq3::render_module(),
        _ => {
            div()
                .id(format!("dsp_empty_{track_idx}_{idx}"))
                .w_full()
                .flex_1()
                .min_h(px(100.0))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| {
                    open_plugin_menu(cx, track_idx, idx);
                })
                .on_mouse_down(gpui_kit::MouseButton::Right, move |_, _, cx| {
                    open_plugin_menu(cx, track_idx, idx);
                })
                .on_aux_click(move |_, _, cx| {
                    open_plugin_menu(cx, track_idx, idx);
                })
                .child(
                    Label::new("Empty Slot")
                        .text_sm()
                        .text_color(rgb(0x888888))
                )
                .into_any_element()
        }
    }
}

fn render_card_footer(
    cx: &mut Context<HikaruApp>,
    track_idx: usize,
    idx: usize,
    name: &str,
) -> AnyElement {
    match name {
        "OpenWavetable" => open_wavetable::render_module_footer(cx, track_idx, idx),
        _ => div().w_full().h(px(CARD_FOOTER_HEIGHT)).into_any_element(),
    }
}

/// Navegador de plugins del rack (estilo DAW pro).
///
/// Sustituye al antiguo menú flotante anclado a la tarjeta: en vez de un
/// popup estrecho con el listado crudo, es un panel que ocupa toda la tira
/// del rack con cabecera (título + buscador + cierre), fila de filtros por
/// formato, lista con scroll y acciones de escaneo. Todo con el tema oscuro
/// de Hikaru (fondos `#14141A`/`#1C1C22`, acento naranja `#FF6E00`).
///
/// Vive como overlay absoluto sobre la tira (`top(20)` = debajo del título
/// del rack): no mueve ni superpone tarjetas, y al asignar o cerrar se
/// desmonta sin dejar estado colgado (`close_browser`).
fn render_plugin_browser(
    cx: &mut Context<HikaruApp>,
    track_idx: usize,
    slot_idx: usize,
) -> AnyElement {
    let (all_externals, query, filter, scan_status, is_scanning, scan_error_count, gui_errors, search_input) = {
        let app = state(cx).read(cx);
        (
            app.plugin_settings_state.discovered_plugins.clone(),
            app.plugin_browser_query.clone(),
            app.plugin_browser_filter,
            app.plugin_settings_state.status_message.clone(),
            app.plugin_settings_state.is_scanning,
            app.plugin_settings_state.scan_errors.len(),
            app.plugin_settings_state.gui_errors.clone(),
            app.plugin_search_input.clone(),
        )
    };
    let needle = query.trim().to_lowercase();

    let natives: Vec<PluginEntry> = PLUGIN_CATALOG
        .iter()
        .copied()
        .filter(|entry| {
            needle.is_empty()
                || entry.label.to_lowercase().contains(&needle)
                || entry.name.to_lowercase().contains(&needle)
        })
        .collect();
    // El id identifica al plugin entre escaneos, así que dos plugins con el
    // mismo nombre no colisionan en el DOM de GPUI (que indexa por id).
    let filtered_externals: Vec<hikaru_plugin_host::PluginDescriptor> = all_externals
        .into_iter()
        .filter(|d| filter.matches_external(d.format))
        .filter(|d| {
            needle.is_empty()
                || d.name.to_lowercase().contains(&needle)
                || d.vendor.to_lowercase().contains(&needle)
        })
        .collect();
    let show_natives = filter.show_natives();

    let target_label = format!("Pista {} · Slot {:02}", track_idx + 1, slot_idx + 1);

    div()
        .id("plugin_browser")
        .absolute()
        .left(px(0.0))
        .w_full()
        .top(px(20.0))
        .bottom(px(0.0))
        .flex()
        .flex_col()
        .bg(rgb(0x101014))
        .border_t_1()
        .border_color(rgb(0xFF6E00))
        .child(
            // Cabecera: qué slot se está vistiendo + buscador + cierre.
            h_flex()
                .w_full()
                .h(px(32.0))
                .flex_shrink_0()
                .items_center()
                .gap(px(8.0))
                .px(px(10.0))
                .bg(rgb(0x1A1A1F))
                .border_b_1()
                .border_color(rgb(0x2A2A30))
                .child(
                    Label::new("AÑADIR PLUGIN")
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0xE0E0E0)),
                )
                .child(
                    Label::new(target_label)
                        .text_xs()
                        .text_color(rgb(0x888888)),
                )
                .child(
                    TextStyleScope::text_color(
                        rgb(0xE0E0E0),
                        div()
                            .flex_1()
                            .h(px(22.0))
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .bg(rgb(0x0C0C10))
                            .border_1()
                            .border_color(rgb(0x2A2A30))
                            .rounded(px(4.0))
                            .px(px(6.0))
                            .child(
                                Label::new("⌕")
                                    .text_xs()
                                    .text_color(rgb(0x666666)),
                            )
                            .child(
                                Input::new(&search_input)
                                    .appearance(false)
                                    .bordered(false)
                                    .cleanable(true)
                                    .h_full()
                                    .w_full()
                                    .with_size(Size::XSmall),
                            ),
                    ),
                )
                .child(
                    rack_button("dsp_browser_close")
                        .label("✕")
                        .on_click(move |_, _, cx| {
                            close_browser(cx, track_idx);
                        }),
                ),
        )
        .child(
            // Filtros por formato + estado del escaneo + acciones.
            h_flex()
                .w_full()
                .h(px(28.0))
                .flex_shrink_0()
                .items_center()
                .gap(px(4.0))
                .px(px(10.0))
                .children(PluginBrowserFilter::TABS.iter().map(|(tab, label)| {
                    let active = *tab == filter;
                    rack_button(format!("dsp_browser_tab_{label}"))
                        .label(*label)
                        .when(active, |b| {
                            b.bg(rgb(0x3A2410)).text_color(rgb(0xFF9E40))
                        })
                        .when(!active, |b| b.text_color(rgb(0x888888)))
                        .on_click(move |_, _, cx| {
                            let tab = *tab;
                            state(cx).update(cx, |state, cx| {
                                state.plugin_browser_filter = tab;
                                cx.notify();
                            });
                        })
                        .into_any_element()
                }))
                .child(div().flex_1())
                .child(
                    Label::new(if is_scanning {
                        "Escaneando…".to_string()
                    } else if scan_error_count > 0 {
                        format!("{scan_status} ({scan_error_count} omitidos)")
                    } else {
                        scan_status.clone()
                    })
                    .text_xs()
                    .truncate()
                    .max_w(px(260.0))
                    .text_color(rgb(0x666666)),
                )
                .child(
                    rack_button("dsp_browser_rescan")
                        .label(if is_scanning { "…" } else { "⟳ Rescan" })
                        .on_click(move |_, _, cx| {
                            state(cx).update(cx, |state, cx| {
                                state.plugin_settings_state.start_scan();
                                cx.notify();
                            });
                        }),
                )
                .child(
                    rack_button("dsp_browser_paths")
                        .label("Rutas…")
                        .on_click(move |_, _, cx| {
                            state(cx).update(cx, |state, cx| {
                                state.plugin_settings_state.is_open = true;
                                cx.notify();
                            });
                        }),
                ),
        )
        .child(
            // Resultados con scroll: nativos arriba, externos debajo.
            v_flex()
                .w_full()
                .flex_1()
                .min_h(px(0.0))
                .overflow_y_scrollbar()
                .px(px(10.0))
                .py(px(6.0))
                .gap(px(3.0))
                .when(show_natives && !natives.is_empty(), |this| {
                    this.child(browser_section_header("Plugins nativos · Hikaru"))
                })
                .children(natives.iter().map(|entry| {
                    browser_native_row(track_idx, slot_idx, *entry)
                }))
                .when(show_natives && natives.is_empty() && filtered_externals.is_empty(), |this| {
                    this.child(browser_empty_state("Sin nativos para esa búsqueda."))
                })
                .when(!filtered_externals.is_empty(), |this| {
                    this.child(browser_section_header(&format!(
                        "VST3 / CLAP externos ({})",
                        filtered_externals.len()
                    )))
                })
                .children(filtered_externals.iter().map(|descriptor| {
                    let gui_error = gui_errors.get(&descriptor.name).cloned();
                    browser_external_row(track_idx, slot_idx, descriptor.clone(), gui_error)
                }))
                .when(filtered_externals.is_empty(), |this| {
                    this.child(browser_empty_state(
                        "Ningún externo coincide. Pulsa ⟳ Rescan o revisa Rutas…",
                    ))
                })
                .into_any_element(),
        )
        .into_any_element()
}

/// Cabecera de sección del navegador ("Plugins nativos", "VST3 / CLAP (3)").
fn browser_section_header(title: &str) -> AnyElement {
    Label::new(title)
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(0x6A7080))
        .into_any_element()
}

fn browser_empty_state(message: &str) -> AnyElement {
    div()
        .w_full()
        .py(px(6.0))
        .flex()
        .justify_center()
        .child(
            Label::new(message)
                .text_xs()
                .text_color(rgb(0x4A5060)),
        )
        .into_any_element()
}

/// Fila de plugin nativo: toda la fila asigna (el "Añadir ›" es affordance
/// visual, no un botón anidado, para no disparar dos asignaciones).
fn browser_native_row(track_idx: usize, slot_idx: usize, entry: PluginEntry) -> AnyElement {
    div()
        .id(format!("dsp_browser_nat_{}_{slot_idx}", entry.name))
        .w_full()
        .h(px(28.0))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(8.0))
        .px(px(8.0))
        .bg(rgb(0x1C1C22))
        .border_1()
        .border_color(rgb(0x2A2A30))
        .rounded(px(4.0))
        .on_click(move |_, _, cx| {
            assign_plugin(cx, track_idx, slot_idx, entry.name);
        })
        .child(
            Label::new("INT")
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0x00FF88)),
        )
        .child(
            Label::new(entry.label)
                .text_xs()
                .truncate()
                .flex_1()
                .text_color(rgb(0xE8EAF0)),
        )
        .child(
            Label::new(entry.category)
                .text_xs()
                .flex_shrink_0()
                .text_color(rgb(0x666666)),
        )
        .child(
            Label::new("Añadir ›")
                .text_xs()
                .flex_shrink_0()
                .text_color(rgb(0xFF9E40)),
        )
        .into_any_element()
}

/// Fila de plugin externo. Si su GUI falló alguna vez (`gui_errors`), la
/// fila lo muestra en rojo en vez de prometer un editor que no abre: el
/// plugin sigue asignable para audio, pero el usuario sabe qué pasa.
fn browser_external_row(
    track_idx: usize,
    slot_idx: usize,
    descriptor: hikaru_plugin_host::PluginDescriptor,
    gui_error: Option<String>,
) -> AnyElement {
    let is_vst3 = descriptor.format == hikaru_plugin_host::PluginFormat::VST3;
    let badge = if is_vst3 { "VST3" } else { "CLAP" };
    let badge_color = if is_vst3 { rgb(0x64B4FF) } else { rgb(0xB464FF) };
    let row_id = descriptor.id.clone();
    let plugin_name = descriptor.name.clone();
    let sub = if descriptor.vendor.is_empty() {
        descriptor
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    } else {
        descriptor.vendor.clone()
    };

    v_flex()
        .w_full()
        .flex_shrink_0()
        .bg(rgb(0x1C1C22))
        .border_1()
        .border_color(rgb(0x2A2A30))
        .rounded(px(4.0))
        .px(px(8.0))
        .py(px(2.0))
        .child(
            div()
                .id(format!("dsp_browser_ext_{row_id}_{slot_idx}"))
                .w_full()
                .h(px(24.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .on_click(move |_, _, cx| {
                    assign_external_plugin(cx, track_idx, slot_idx, descriptor.clone());
                })
                .child(
                    Label::new(badge)
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .flex_shrink_0()
                        .w(px(36.0))
                        .text_color(badge_color),
                )
                .child(
                    Label::new(plugin_name)
                        .text_xs()
                        .truncate()
                        .flex_1()
                        .text_color(rgb(0xE8EAF0)),
                )
                .child(
                    Label::new(sub)
                        .text_xs()
                        .truncate()
                        .flex_shrink_0()
                        .w(px(140.0))
                        .text_color(rgb(0x666666)),
                )
                .child(
                    Label::new("Añadir ›")
                        .text_xs()
                        .flex_shrink_0()
                        .text_color(rgb(0xFF9E40)),
                )
                .into_any_element(),
        )
        .when_some(gui_error, |this, message| {
            this.child(
                Label::new(format!("⚠ {message}"))
                    .text_xs()
                    .truncate()
                    .text_color(rgb(0xFF7A5C)),
            )
        })
        .into_any_element()
}

fn render_options_menu(
    cx: &mut Context<HikaruApp>,
    track_idx: usize,
    slot_idx: usize,
    left: f32,
) -> AnyElement {
    div()
        .absolute()
        .left(px(left))
        .bottom(px(RACK_HEIGHT))
        .w(px(CARD_WIDTH))
        .bg(rgb(0x1E1E26))
        .border_1()
        .border_color(rgb(0x3A3A45))
        .rounded(px(4.0))
        .shadow(vec![gpui_kit::BoxShadow {
            color: gpui_kit::black().opacity(0.5),
            offset: point(px(0.0), px(-4.0)),
            blur_radius: px(8.0),
            spread_radius: px(0.0),
            inset: false,
        }])
        .p(px(4.0))
        .gap(px(2.0))
        .child(
            rack_button(format!("dsp_opt_save_{track_idx}_{slot_idx}"))
                .rounded(ButtonRounded::None)
                .label("Save Preset...")
                .compact()
                .w_full()
                .on_click(move |_, _, cx| {
                    state(cx).update(cx, |state, cx| {
                        if let Some(slot) = state.slot_mut(track_idx, slot_idx) {
                            slot.options_open = false;
                        }
                        cx.notify();
                    });
                }),
        )
        .child(
            rack_button(format!("dsp_opt_paths_{track_idx}_{slot_idx}"))
                .rounded(ButtonRounded::None)
                .label("Add Wavetable Paths...")
                .compact()
                .w_full()
                .on_click(move |_, _, cx| {
                    crate::views::explorer::begin_wavetable_pick(cx, track_idx, slot_idx);
                    state(cx).update(cx, |state, cx| {
                        if let Some(slot) = state.slot_mut(track_idx, slot_idx) {
                            slot.options_open = false;
                        }
                        cx.notify();
                    });
                }),
        )
        .child(
            rack_button(format!("dsp_opt_delete_{track_idx}_{slot_idx}"))
                .rounded(ButtonRounded::None)
                .label("Delete Slot")
                .compact()
                .w_full()
                .on_click(move |_, _, cx| {
                    remove_slot_at(cx, track_idx, slot_idx);
                }),
        )
        .into_any_element()
}

/// Tarjeta de un plugin externo VST3/CLAP.
///
/// Es un placeholder, no el editor: el editor nativo es una ventana del sistema
/// (XWayland, ver `hikaru_plugin_host::platform::linux`), no un árbol de
/// elementos GPUI. Lo que se dibuja acá es el ancla visual dentro del rack --
/// formato, archivo y el botón que abre la ventana -- para que el usuario sepa
/// qué tiene cargado sin abrir nada.
///
/// El contenedor no reserva el tamaño real del editor porque no se puede
/// conocer sin instanciar el plugin. Cuando el motor lo cargue se puede
/// pintar el tamaño preferido; hasta entonces el layout del rack no depende de
/// plugins externos, que es lo que evita que la ventana salte al cargarlos.
fn render_external_card(
    cx: &mut Context<HikaruApp>,
    track_idx: usize,
    idx: usize,
    descriptor: &hikaru_plugin_host::PluginDescriptor,
    load_error: Option<&str>,
) -> AnyElement {
    let is_vst3 = descriptor.format == hikaru_plugin_host::PluginFormat::VST3;
    let badge = if is_vst3 { "VST3" } else { "CLAP" };
    let badge_color = if is_vst3 { rgb(0x64B4FF) } else { rgb(0xB464FF) };
    let file_name = descriptor
        .path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    // Un slot con error de carga muestra el mensaje y no ofrece abrir el
    // editor: insistir produciría el mismo fallo y, si el plugin llegó a
    // cargarse a medias, dejaría una ventana zombie.
    // Error de GUI nativa (hilo de la ventana → `gui_errors` vía
    // `poll_updates`): el plugin cargó pero su editor no abrió. Se pinta en
    // la tarjeta en vez de cerrar el DAW o dejar una ventana vacía.
    let (gui_error, editor_open, editor_opening, params, params_total, params_status, slot_has_gui) = {
        let app = state(cx).read(cx);
        let slot_params = app
            .slot(track_idx, idx)
            .map(|slot| {
                (
                    // Sólo las filas que se dibujan. La lista completa se
                    // descartaba después (`.take(MAX_PARAM_ROWS)` más abajo), y
                    // clonar N `ExternalParamState` son 2 `String` por entrada:
                    // en un plugin con 300 parámetros eso eran 600 alocs por
                    // frame para pintar 8 filas, y se pagaba en cada `notify`
                    // del drag del fader.
                    (
                        slot.external_params
                            .iter()
                            .take(MAX_PARAM_ROWS)
                            .cloned()
                            .collect::<Vec<_>>(),
                        slot.external_params.len(),
                    ),
                    slot.params_status.clone(),
                    slot.has_gui,
                )
            })
            .unwrap_or_default();
        (
            app.plugin_settings_state.gui_error_for(&descriptor.name),
            app.plugin_settings_state.is_plugin_open(&descriptor.name),
            app.plugin_settings_state.is_plugin_opening(&descriptor.name),
            slot_params.0 .0,
            slot_params.0 .1,
            slot_params.1,
            slot_params.2,
        )
    };
    let blocked_by_gui_error = gui_error.is_some();

    // `descriptor.has_gui` viene del escaneo de disco, que no instancia nada
    // y por lo tanto lo deja siempre en `false`: usarlo haría que este botón
    // no se dibujara jamás. El sondeo diferido sí crea una instancia real, así
    // que su respuesta es la que manda. `descriptor.has_gui` queda como
    // respaldo para el frame previo a que termine el sondeo.
    let can_open_editor = slot_has_gui.unwrap_or(descriptor.has_gui) && load_error.is_none();

    // Primer frame con externo asignado: pedir el sondeo de parámetros. Corre
    // una sola vez (la petición marca `Probing` en el acto) en un hilo
    // aparte; el resultado vuelve vía `drain_param_probes`.
    if matches!(params_status, ExternalParamsStatus::Idle) {
        let descriptor = descriptor.clone();
        let st = state(cx);
        cx.update_entity(&st, |state, cx| {
            state.request_external_params(&descriptor);
            cx.notify();
        });
    }

    div()
        .id(format!("dsp_external_{track_idx}_{idx}"))
        .test_support()
        .w_full()
        .flex_1()
        .min_h(px(100.0))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(4.0))
        .px(px(6.0))
        .py(px(4.0))
        .on_mouse_down(gpui_kit::MouseButton::Right, move |_, _, cx| {
            open_plugin_menu(cx, track_idx, idx);
        })
        // El seguimiento del arrastre de los knobs NO va acá: un
        // `on_mouse_move` de elemento sólo se despacha mientras el hitbox está
        // bajo el cursor, así que el gesto moría en cuanto el cursor salía de
        // la tarjeta. Vive a nivel de ventana, en `param_drag_keepalive`.
        .child(
            Label::new(badge)
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(badge_color),
        )
        .child(
            Label::new(descriptor.name.clone())
                .text_sm()
                .text_color(rgb(0xE8EAF0)),
        )
        .child(
            Label::new(file_name)
                .text_xs()
                .truncate()
                .text_color(rgb(0x666666)),
        )
        .when_some(load_error.map(|e| e.to_string()), |this, message| {
            this.child(
                Label::new(format!("⚠ {message}"))
                    .text_xs()
                    .text_color(rgb(0xFF6E00))
                    .truncate(),
            )
        })
        .when_some(gui_error, |this, message| {
            this.child(
                Label::new(format!("⚠ No abrió su ventana: {message}"))
                    .text_xs()
                    .text_color(rgb(0xFF7A5C))
                    .truncate(),
            )
        })
        // Toggle de editor nativo. Es un botón porque abrir la GUI del plugin es
        // costoso (dlopen + creación de ventana nativa en un hilo aparte) y no
        // tiene sentido mantenerlo cargado si el usuario no lo está mirando.
        //
        // `editor_open` refleja el estado real que devuelve
        // `plugin_settings_state`, no un bool local: la ventana se puede cerrar
        // desde el WM y el toggle tiene que enterarse.
        .when(can_open_editor && !blocked_by_gui_error, |this| {
            let (label, active) = if editor_open {
                ("■  Editor", true)
            } else if editor_opening {
                ("…  Abriendo", false)
            } else {
                ("▸  Editor", false)
            };
            // El nombre se clona para que el closure lo tenga por valor: el
            // toggle necesita poder cerrar por nombre, no reabrir por slot.
            let plugin_name = descriptor.name.clone();
            this.child(
                rack_button(format!("dsp_editor_{track_idx}_{idx}"))
                    .rounded(ButtonRounded::None)
                    .label(label)
                    .compact()
                    .on_click(move |_, window, cx| {
                        if active {
                            close_external_editor(cx, &plugin_name);
                        } else {
                            open_external_editor(window, cx, track_idx, idx);
                        }
                    }),
            )
        })
        .child(render_external_params(
            cx,
            track_idx,
            idx,
            descriptor,
            &params,
            params_total,
            &params_status,
        ))
        .into_any_element()
}

/// Knobs del plugin externo: parámetros automatizables con drag vertical,
/// steppers de precisión y valor con el formato del plugin.
///
/// Máximo 8 filas (la tarjeta vive en un rack de 340px): el resto vive en el
/// editor nativo. Cada gesto escribe la caché del slot, la marca `dirty`
/// para el motor y la manda en vivo a la ventana abierta
/// (`commit_external_param`). Lo que el editor nativo mueve por su cuenta
/// vuelve por `apply_param_feedback` y los knobs lo reflejan sin pelear con
/// el drag en curso (el feedback cede ante el dedo).
/// `params` trae sólo las [`MAX_PARAM_ROWS`] filas que caben en la tarjeta, para
/// no clonar la lista entera a cada frame. `params_total` es el conteo real
/// del plugin, que es lo que necesita el rótulo: no se puede deducir de
/// `params.len()` justamente porque esa lista viene truncada.
fn render_external_params(
    _cx: &mut Context<HikaruApp>,
    track_idx: usize,
    idx: usize,
    descriptor: &hikaru_plugin_host::PluginDescriptor,
    params: &[crate::views::mixer::ExternalParamState],
    params_total: usize,
    status: &ExternalParamsStatus,
) -> AnyElement {
    let is_clap = descriptor.format == hikaru_plugin_host::PluginFormat::CLAP;

    let header: AnyElement = match status {
        ExternalParamsStatus::Probing => Label::new("Leyendo parámetros…")
            .text_xs()
            .text_color(rgb(0x666666))
            .into_any_element(),
        ExternalParamsStatus::Failed(message) => {
            let descriptor = descriptor.clone();
            h_flex()
                .gap(px(6.0))
                .items_center()
                .child(
                    Label::new(format!("⚠ {message}"))
                        .text_xs()
                        .truncate()
                        .flex_1()
                        .text_color(rgb(0xFF7A5C)),
                )
                .child(
                    rack_button(format!("dsp_param_retry_{track_idx}_{idx}"))
                        .label("Reintentar")
                        .compact()
                        .flex_shrink_0()
                        .on_click(move |_, _, cx| {
                            let st = state(cx);
                            cx.update_entity(&st, |state, cx| {
                                state.request_external_params(&descriptor);
                                cx.notify();
                            });
                        }),
                )
                .into_any_element()
        }
        ExternalParamsStatus::Ready if params.is_empty() => Label::new(if is_clap {
            "CLAP: parámetros no vinculados por el host."
        } else {
            "Sin parámetros automatizables."
        })
        .text_xs()
        .text_color(rgb(0x555555))
        .into_any_element(),
        ExternalParamsStatus::Ready => {
            // `params` trae sólo las filas visibles; `params_total` es el
            // conteo real del plugin, que es lo que va en el rótulo.
            let extra = params_total.saturating_sub(MAX_PARAM_ROWS);
            Label::new(if extra > 0 {
                format!("PARÁMETROS ({params_total}) · +{extra} en el editor")
            } else {
                format!("PARÁMETROS ({params_total})")
            })
            .text_xs()
            .font_weight(FontWeight::BOLD)
            .text_color(rgb(0x6A7080))
            .into_any_element()
        }
        ExternalParamsStatus::Idle => Label::new("Preparando parámetros…")
            .text_xs()
            .text_color(rgb(0x555555))
            .into_any_element(),
    };

    v_flex()
        .w_full()
        .gap(px(2.0))
        .child(header)
        .children(
            params
                .iter()
                .take(MAX_PARAM_ROWS)
                .map(|param| external_param_row(track_idx, idx, param)),
        )
        .into_any_element()
}

/// Filas visibles por tarjeta: el rack mide 340px y cada fila 20px.
const MAX_PARAM_ROWS: usize = 8;

/// Una fila de parámetro: stepper −, nombre, barra arrastrable, valor, stepper
/// +.
///
/// La barra es el fader: `mouse_down` congela el agarre (`param_drag_begin`) y
/// el seguimiento a nivel de ventana lo continúa (`param_drag_move`) hasta el
/// `mouse_up`. El gesto va **en X** porque la barra es horizontal, con `Shift`
/// para el mismo recorrido cuatro veces más fino. Los steppers usan el escalón
/// del parámetro (discretos saltan de escalón en escalón; continuos, de 0.05 en
/// 0.05).
fn external_param_row(
    track_idx: usize,
    idx: usize,
    param: &crate::views::mixer::ExternalParamState,
) -> AnyElement {
    let id = param.info.id;
    let value = param.value;
    let name = param.info.name.clone();
    let display = param.display();
    let step = if param.info.is_discrete() {
        1.0 / param.info.step_count.max(1) as f32
    } else {
        0.05
    };
    let fill_w = (value.clamp(0.0, 1.0) * PARAM_BAR_WIDTH).max(2.0);

    h_flex()
        .w_full()
        .h(px(20.0))
        .flex_shrink_0()
        .items_center()
        .gap(px(4.0))
        .child(
            rack_button(format!("dsp_param_dec_{track_idx}_{idx}_{id}"))
                .label("−")
                .compact()
                .flex_shrink_0()
                .w(px(20.0))
                .on_click(move |_, _, cx| {
                    commit_external_param(cx, track_idx, idx, id, value - step);
                }),
        )
        .child(
            Label::new(name)
                .text_xs()
                .truncate()
                .flex_1()
                .text_color(rgb(0xB8BCC8)),
        )
        .child(
            // El hitbox es más alto que la barra dibujada: 10px de alto es
            // casi imposible de agarrar y, además, el arrastre horizontal se
            // cortaba al primer píxel de desviación vertical. El riel es lo
            // único que se ve; el resto de la caja es transparente y sólo
            // existe para que el `mouse_down` llegue.
            div()
                .id(format!("dsp_parambar_{track_idx}_{idx}_{id}"))
                // Registra la caja en el registro de observación de los tests.
                // Va pegado al `.id` como en `matrix_vol_*`: sin esto
                // `window.find("dsp_parambar_…")` no ve la barra y el gesto no
                // se puede probar de punta a punta.
                .test_support()
                .flex_shrink_0()
                .w(px(PARAM_BAR_WIDTH))
                .h(px(PARAM_BAR_HIT_H))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .w(px(PARAM_BAR_WIDTH))
                        .h(px(PARAM_BAR_HEIGHT))
                        .flex()
                        .items_center()
                        .bg(rgb(0x0C0C10))
                        .border_1()
                        .border_color(rgb(0x2A2A30))
                        .rounded(px(3.0))
                        .overflow_hidden()
                        .child(
                            div()
                                .w(px(fill_w))
                                .h_full()
                                .bg(rgb(0xFF6E00))
                                .into_any_element(),
                        )
                        .into_any_element(),
                )
                .on_mouse_down(gpui_kit::MouseButton::Left, move |event, _, cx| {
                    param_drag_begin(
                        cx,
                        track_idx,
                        idx,
                        id,
                        f32::from(event.position.x),
                        f32::from(event.position.y),
                    );
                })
                .into_any_element(),
        )
        .child(
            Label::new(display)
                .text_xs()
                .truncate()
                .flex_shrink_0()
                .w(px(64.0))
                .text_color(rgb(0x888888)),
        )
        .child(
            rack_button(format!("dsp_param_inc_{track_idx}_{idx}_{id}"))
                .label("+")
                .compact()
                .flex_shrink_0()
                .w(px(20.0))
                .on_click(move |_, _, cx| {
                    commit_external_param(cx, track_idx, idx, id, value + step);
                }),
        )
        .into_any_element()
}

/// Ancho fijo de la barra del knob (el relleno se calcula en px).
const PARAM_BAR_WIDTH: f32 = 56.0;

/// Alto de la barra **dibujada**. Chiquita a propósito: la fila mide 20px y la
/// barra es una línea de valor, no un knob.
const PARAM_BAR_HEIGHT: f32 = 10.0;

/// Alto del **hitbox** de la barra: la fila entera.
///
/// El dibujo no cambia, sólo la caja que agarra el mouse. Con 10px de alto había
/// que apuntar al riel para que el `mouse_down` entrara, y un arrastre horizontal
/// con la más mínima desviación vertical se salía de la caja.
const PARAM_BAR_HIT_H: f32 = 20.0;

pub fn rack_title(track_name: &str, scene: Option<usize>) -> String {
    match scene {
        Some(scene) => format!("DSP RACK: {track_name} | Scene {}", scene + 1),
        None => format!("DSP RACK: {track_name}"),
    }
}

pub fn handle_delete_key(cx: &mut gpui_kit::App) {
    let track_idx = state(cx).read(cx).safe_track_index();
    remove_selected_slot(cx, track_idx);
}
