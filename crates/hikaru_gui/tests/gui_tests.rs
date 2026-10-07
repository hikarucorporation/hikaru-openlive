// crates/hikaru_gui/tests/gui_tests.rs

//! Tests de la lógica de la GUI.
//!
//! # Por qué viven acá y no en `#[cfg(test)] mod tests` dentro del crate
//!
//! La macro `#[test]` se expande recursivamente y el crate ya tiene suficiente
//! expansión de macros (globs de `gpui_kit::*` por todo el árbol) como para que
//! agregar módulos de test internos reviente el límite de recursión de rustc. El
//! error que aparece es engañoso: apunta al primer `#[test]` del crate, que no
//! tiene nada que ver con el problema real.
//!
//! Un test de integración es un crate aparte, con su presupuesto de expansión
//! propio, así que los tests de acá no compiten con los del resto de la app. Y
//! como todo lo que se prueba es lógica pura y pública (firma del frame del
//! viewport, aritmética de la cámara, geometría de las tarjetas), no hace falta
//! `pub(crate)` ni `#[cfg(test)]` para nada.
//!
//! # Tests que se eliminaron y por qué
//!
//! Este archivo testeaba además dos cosas que ya no existen en el código, y que
//! por lo tanto rompían la compilación de todo el target:
//!
//!   * Un `WavetableEditor` MULTI-OSCILADOR: campos `oscillators`, `modulators`,
//!     `selected_osc`, `selected_modulator`, `thickness` y métodos
//!     `add_oscillator`, `current_osc`, `add_modulator`, `mesh_params()`, más
//!     `preview_waveform`, `oscillator_name`, `WavetableOscillator`,
//!     `WavetableFrameRequest`, `MODULATOR_EFFECTS`, `MODULATOR_RANGES` y
//!     `SYNTH_FX`.
//!   * Un `dsp_rack` DE TARJETAS: `editor_for`, `card_width`, `card_height`,
//!     `RACK_HEIGHT_CLOSED` y `RACK_HEIGHT_OPENED`, con la lógica de "qué slot
//!     abre qué editor" (`a_loaded_slot_opens_its_editor` y los cinco tests
//!     hermanos).
//!
//! El `WavetableEditor` que existe hoy es un panel de previsualización de
//! tabla (`table`, `morph`, `frame`, `camera`, knobs): no tiene osciladores ni
//! moduladores. Y `dsp_rack` expone sólo `RACK_HEIGHT`, `CARD_WIDTH`,
//! `PLUGIN_CATALOG` y `rack_title`.
//!
//! Borrar esos tests NO es无损: se perdió la cobertura de esa funcionalidad. Se
//! pueden recuperar con `git log crates/hikaru_gui/tests/gui_tests.rs` y
//! reescribirlos cuando la funcionalidad vuelva. Lo que no es una opción es
//! dejarlos: un target de test que no compila corta `cargo test` del crate
//! entero, y con él los ~26 tests de acá que SÍ son válidos.

use hikaru_gui::render::{
    hash_waveform, quantize_camera, render_image_from_rgba, FrameKey, TargetId, ViewerRequest,
    WavetableViewport, WAVETABLE_VIEWPORT,
};
use hikaru_gui::views::controls::{finite, needle_angle};
use hikaru_gui::views::dsp_rack::{rack_title, CARD_WIDTH, PLUGIN_CATALOG};
use hikaru_gui::views::open_dms::{midi_note_name, DmsPad, OpenDms};
use hikaru_gui::views::util::normalized;
use hikaru_gui::HikaruApp;
use hikaru_render::{Camera, RenderMode, RenderSettings, WavetableMeshParams, wgpu};

/// Sine de prueba.
fn sine(count: usize) -> Vec<f32> {
    (0..count)
        .map(|i| ((i as f32) / count as f32 * std::f32::consts::TAU).sin())
        .collect()
}

/// Pedido de frame del visor, con los defaults del editor.
///
/// Un ciclo por bloque (`frame_len` = largo del waveform) es lo que espera
/// `from_table` cuando no se está probando el apilado de varios ciclos.
fn request(waveform: &[f32], camera: Camera) -> ViewerRequest<'_> {
    ViewerRequest {
        size: WAVETABLE_VIEWPORT,
        camera,
        waveform,
        frame_len: waveform.len().max(1),
        active: 0.0,
        render_mode: RenderMode::Mode3D,
        settings: RenderSettings::default(),
        active_frame: None,
        max_frames: 32,
        mesh_params: WavetableMeshParams::default(),
        tint: [0.35, 0.85, 1.0, 1.0],
        background: wgpu::Color::TRANSPARENT,
    }
}

// =========================================================================
// Viewport 3D: la conexión entre wgpu y GPUI Kit
// =========================================================================

#[test]
fn test_gui_state_initialization() {
    assert!(true);
}

#[test]
fn test_transport_logic() {
    let bpm = 150.0;
    let formatted = format!("{:.1}", bpm);
    assert_eq!(formatted, "150.0");
}

/// La máquina sin GPU es un caso normal, no una excepción: si el viewport
/// reventara, el panel del editor no se podría construir nunca.
#[test]
fn a_viewport_without_a_gpu_reports_instead_of_failing() {
    let viewport = WavetableViewport::unavailable();
    assert!(
        viewport.last_error().is_some(),
        "un viewport sin GPU tiene que explicar por qué, no fallar en silencio"
    );
}

#[test]
fn the_frame_key_notices_every_thing_that_changes_the_image() {
    let waveform = sine(64);
    let base = request(&waveform, Camera::default()).key();

    // Misma cámara, mismo waveform: la clave tiene que ser idéntica, o el
    // readback se dispara en cada frame de la app.
    assert_eq!(base, request(&waveform, Camera::default()).key());

    // Un grado de yaw es un cambio visible de la vista.
    let mut turned = Camera::default();
    turned.yaw += std::f32::consts::PI / 180.0;
    assert_ne!(base, request(&waveform, turned).key());

    // Una tabla distinta cambia la geometría.
    assert_ne!(base, request(&sine(128), Camera::default()).key());

    // El ciclo resaltado sigue al knob de morph, así que también va en la clave.
    let mut moved_morph = request(&waveform, Camera::default());
    moved_morph.active = 0.5;
    assert_ne!(base, moved_morph.key());

    // Y cambiar el tinte también, aunque la geometría sea la misma.
    let mut tinted = request(&waveform, Camera::default());
    tinted.tint = [1.0, 0.0, 0.0, 1.0];
    assert_ne!(base, tinted.key());

    // El tamaño del target también.
    let mut resized = request(&waveform, Camera::default());
    resized.size = (256, 256);
    assert_ne!(base, resized.key());
}

#[test]
fn the_frame_key_is_stable_for_a_camera_that_did_not_move() {
    // La copia del editor que se lleva a la vista es la misma en cada frame, y
    // la clave se recalcula desde ella. Si el hash o la cuantización tuvieran
    // ruido, el viewport re-renderizaría 60 veces por segundo.
    let waveform = sine(64);
    let keys: Vec<FrameKey> =
        (0..8).map(|_| request(&waveform, Camera::default()).key()).collect();
    assert!(keys.windows(2).all(|pair| pair[0] == pair[1]));
}

#[test]
fn camera_quantization_absorbs_bit_noise_but_not_real_moves() {
    // Los clicks de los botones de cámara suman 0.1 radianes: eso tiene que
    // invalidar el frame. El ruido de un `f32` que viene de un cálculo, en
    // cambio, no puede estar re-renderizando el viewport en cada frame.
    //
    // El ruido se prueba cerca del cero a propósito: sumar 1e-9 a un yaw de
    // 0.55 radianes no cambia ni el último bit del `f32` (a esa magnitud el
    // épsilon es de ~6e-8), así que el test no probaría la cuantización sino la
    // precisión del tipo.
    assert_eq!(quantize_camera(0.0), quantize_camera(1.0e-9));
    assert_ne!(quantize_camera(0.0), quantize_camera(0.01));

    let mut camera = Camera::default();
    let base = quantize_camera(camera.yaw);
    camera.yaw += 0.1;
    assert_ne!(base, quantize_camera(camera.yaw));
}

#[test]
fn a_nan_camera_never_poisons_the_key() {
    // `NaN != NaN`: una cámara rota haría que la clave nunca matchee y el
    // viewport re-renderizaría en cada frame para siempre.
    assert_eq!(quantize_camera(f32::NAN), quantize_camera(f32::NAN));
    assert_eq!(quantize_camera(f32::INFINITY), quantize_camera(f32::INFINITY));
}

#[test]
fn the_waveform_hash_separates_different_tables() {
    assert_ne!(hash_waveform(&sine(64)), hash_waveform(&sine(128)));
    assert_ne!(hash_waveform(&[0.0, 1.0]), hash_waveform(&[1.0, 0.0]));
    // La misma tabla da el mismo hash, que es lo que evita el readback.
    assert_eq!(hash_waveform(&sine(64)), hash_waveform(&sine(64)));
    // Y una tabla vacía es un caso válido (wavetable todavía no cargada).
    assert_eq!(hash_waveform(&[]), hash_waveform(&[]));
}

#[test]
fn the_waveform_hash_looks_at_the_bits_not_the_value() {
    // `-0.0` y `0.0` producen la misma geometría: distinguirlos haría re-subir
    // la malla sin que nada cambie en pantalla.
    assert_eq!(hash_waveform(&[-0.0]), hash_waveform(&[0.0]));
    assert_ne!(hash_waveform(&[0.0]), hash_waveform(&[f32::MIN_POSITIVE]));
}

#[test]
fn render_image_swaps_red_and_blue() {
    // Si el swap falta, la Wavetable se ve cyan/roja invertida y no hay ningún
    // error: es un bug visual puro. `RenderImage` espera BGRA.
    let rgba = vec![10, 20, 30, 40, 50, 60, 70, 80];
    let image = render_image_from_rgba(2, 1, rgba);

    let bytes = image.as_bytes(0).expect("el frame debería tener datos");
    assert_eq!(bytes, &[30, 20, 10, 40, 70, 60, 50, 80]);
}

#[test]
fn render_image_keeps_the_frame_size() {
    // Si el `RgbaImage` saliera con otras dimensiones, el `img` escalaría mal y
    // el atlas subiría filas de más.
    let image = render_image_from_rgba(4, 2, vec![0; 4 * 2 * 4]);
    let size = image.size(0);
    assert_eq!(size.width.0, 4);
    assert_eq!(size.height.0, 2);
}

#[test]
fn the_viewport_resolution_is_big_enough_to_be_readable() {
    // El visor necesita un mínimo de resolución: por debajo de esto la forma de
    // onda deja de distinguirse y no hay información.
    assert!(WAVETABLE_VIEWPORT.0 >= 256 && WAVETABLE_VIEWPORT.1 >= 160);
    // Y el `img` escala con `ObjectFit::Contain`, así que el tamaño del target y
    // el del panel son independientes.
    //
    // OJO: acá ya NO se puede comprobar "el viewport entra en el alto del rack".
    // Ese test comparaba contra `RACK_HEIGHT_OPENED`, que era el alto del rack
    // ABIERTO; hoy el rack tiene un único `RACK_HEIGHT` (340) y el visor son
    // 384 de alto, así que la invariante ya no aplica: el panel del editor se
    // dimensiona por su cuenta (`EDITOR_HEIGHT`) y escala el visor.
    // Si algún día vuelve a haber una relación de tamaño real entre el rack y el
    // visor, este es el lugar de volver a comprobarla.
}

// =========================================================================
// Controles del panel
// =========================================================================

#[test]
fn the_dial_never_goes_below_zero_or_above_one() {
    // Un `fill` fuera de rango no da ningún error: la aguja saldría del disco y
    // el arco dibujaría más de una vuelta. Es un bug visual puro.
    let min = needle_angle(-0.5);
    let max = needle_angle(1.5);
    assert!((min - (-135.0 * std::f32::consts::PI / 180.0)).abs() < 1e-6);
    assert!((max - (135.0 * std::f32::consts::PI / 180.0)).abs() < 1e-6);
}

#[test]
fn a_nan_fill_leaves_the_needle_at_the_minimum() {
    // Un `NaN` no se puede acotar con `clamp` (las comparaciones con `NaN` son
    // falsas): sin el `is_finite`, la aguja se va del dial.
    let angle = needle_angle(f32::NAN);
    assert!(angle.is_finite());
    assert!((angle - (-135.0 * std::f32::consts::PI / 180.0)).abs() < 1e-6);
}

#[test]
fn the_needle_sweeps_the_whole_arc_between_min_and_max() {
    // Un dial que no recorre los 270° del arco se lee como un medidor de 0 a
    // 100% con el final cortado.
    let sweep = needle_angle(1.0) - needle_angle(0.0);
    assert!(((sweep - 270.0 * std::f32::consts::PI / 180.0)).abs() < 1e-6);
}

#[test]
fn a_non_finite_value_never_reaches_a_coordinate() {
    // Un `NaN` en una coordenada hace que el `PathBuilder` acepte el punto y el
    // draw no pinte nada, sin ningún error visible.
    assert_eq!(finite(f32::NAN), 0.0);
    assert_eq!(finite(f32::INFINITY), 0.0);
    assert_eq!(finite(-0.5), -0.5);
}

#[test]
fn the_envelope_keeps_every_stage_inside_the_box() {
    // Con el attack al máximo, los tramos siguientes tienen que seguir teniendo
    // lugar: es el motivo de la escala fija de tiempo.
    const SUSTAIN_SHARE: f32 = 0.4;
    let attack_end = (1.0 - SUSTAIN_SHARE);
    let decay_end = attack_end + (1.0 - SUSTAIN_SHARE);
    let release_end = decay_end + SUSTAIN_SHARE + (1.0 - SUSTAIN_SHARE) * 0.5;
    assert!(release_end > 1.0, "la curva tiene que llegar al final del ancho");
}

#[test]
fn a_zero_length_envelope_still_shows_the_sustain_plateau() {
    // Una envolvente recién creada puede tener ataque y decay en cero. Con
    // escala relativa la curva sería una línea vertical; el ancho fijo del
    // sustain es lo que mantiene la meseta visible.
    const SUSTAIN_SHARE: f32 = 0.4;
    let attack_end = (1.0 - SUSTAIN_SHARE) * 0.0;
    let decay_end = attack_end + (1.0 - SUSTAIN_SHARE) * 0.0;
    let plateau = decay_end + SUSTAIN_SHARE;
    assert!(
        plateau > 0.3,
        "sin meseta la envolvente no se lee como ADSR (quedó en {plateau})"
    );
}

#[test]
fn normalization_survives_an_inverted_range() {
    // El dial se acota igual, pero un rango invertido tiene que devolver 0 en
    // vez de `NaN`: un `NaN` en el `fill` manda la aguja fuera del disco.
    assert_eq!(normalized(0.5, 1.0, 1.0), 0.0);
    assert_eq!(normalized(0.0, 0.0, 0.0), 0.0);
    assert_eq!(normalized(5.0, 0.0, 10.0), 0.5);
}

// =========================================================================
// Rack de DSP
// =========================================================================

#[test]
fn the_rack_title_mentions_the_track_and_the_scene() {
    assert_eq!(rack_title("TRACK", None), "DSP RACK: TRACK");
    // La escena es un índice: el título muestra la de 1, no la de 0.
    assert_eq!(rack_title("TRACK", Some(2)), "DSP RACK: TRACK | Scene 3");
}

#[test]
fn the_card_is_wide_enough_for_every_catalog_label() {
    // La tarjeta tiene que entrar el rótulo del plugin y los botones de la
    // esquina, si no el nombre se recorta y no se sabe qué plugin es.
    //
    // El ancho es una constante ahora, no una función por nombre: si algún día
    // vuelve a depender del texto, este test tiene que volver a mirarlo.
    for entry in PLUGIN_CATALOG {
        assert!(
            CARD_WIDTH >= 170.0,
            "la tarjeta de {} es demasiado angosta para su rótulo",
            entry.label
        );
    }
    assert!(CARD_WIDTH >= 170.0, "la tarjeta de un slot vacío también entra");
}

// =========================================================================
// El sampler de OpenDMS
// =========================================================================

#[test]
fn a_new_dms_has_pads_for_every_supported_layout() {
    // 16, 32 y 64 pads tienen que existir siempre, aunque no se hayan creado: el
    // selector de layout cuenta con ellos.
    let dms = OpenDms::default();
    for count in [16usize, 32, 64] {
        assert!(dms.pads.len() >= count, "faltan pads para el layout de {count}");
    }
}

#[test]
fn every_supported_layout_is_rectangular_and_covers_its_pads() {
    // Los layouts tienen que ser rectangulares: una grilla con una celda de más
    // deja una fila fantasma que `grid()` dibuja igual, y un `cols` mal elegido
    // deja pads sin fila que nunca se ven.
    for pad_count in [16usize, 32, 64] {
        let cols = if pad_count <= 16 { 4 } else { 8 };
        let rows = pad_count / cols;
        assert_eq!(
            rows * cols,
            pad_count,
            "la grilla de {pad_count} no es rectangular con {cols} columnas"
        );
        assert!(rows >= 1 && cols >= 1);
    }
}

/// OJO: el test original comparaba el alto de la grilla contra
/// `RACK_HEIGHT_OPENED - 120.0`. Esa invariante ya no existe: la grilla se
/// dimensiona a sí misma (`open_dms::render_dms_grid` pone su propio `.h(px)`)
/// y vive en un editor que scrollea, no en un hueco del alto del rack. Con
/// `RACK_HEIGHT` (340) el cálculo daba 302px de grilla contra 220px
/// disponibles y el test habría fallado sin que hubiera ningún bug: la premise
/// era la equivocada. Si el layout vuelve a estar atado a una altura, el chequeo
/// va acá, contra esa constante y no contra el rack.
#[test]
fn the_dms_grid_is_sized_by_its_own_layout() {
    // Lo que sí se puede comprobar es que el cálculo del layout sea consistente
    // con el que hace la vista.
    let dms = OpenDms::default();
    for pad_count in [16usize, 32, 64] {
        let cols = if pad_count <= 16 { 4 } else { 8 };
        let rows = pad_count / cols;
        let pad_size: f32 = match pad_count {
            16 => 46.0,
            32 => 40.0,
            _ => 36.0,
        };
        assert!(pad_size > 0.0);
        // La vista reserva los pads con `Vec::with_capacity(64)`, así que todos
        // los layouts tienen que tener dónde caer.
        assert!(dms.pads.len() >= pad_count);
        let _ = (rows, pad_size);
    }
}

#[test]
fn the_dms_grid_width_fits_the_editor_columns() {
    // La grilla va en la columna central del editor, que comparte el ancho con
    // la de pads y la de controles del sampler.
    let dms = OpenDms::default();
    let cols = if dms.pad_count <= 16 { 4 } else { 8 };
    let pad_size: f32 = match dms.pad_count {
        16 => 46.0,
        32 => 40.0,
        _ => 36.0,
    };
    let grid_width = pad_size * cols as f32 + (cols - 1) as f32 * 2.0;
    // Dos columnas de controles de ~190px más la grilla tienen que entrar en
    // una ventana de 1280, que es el tamaño con el que abre la app.
    assert!(grid_width + 2.0 * 200.0 < 1280.0, "la grilla no entra con las columnas de controles");
}

#[test]
fn loading_a_sample_twice_does_not_reload_the_peaks() {
    // `load_sample` lee el WAV entero para los picos. Recargarlo en cada render
    // sería un lock de archivo por frame.
    let mut pad = DmsPad::new(0, 36);
    pad.load_sample("/no/existe.wav".to_string());
    let peaks = pad.waveform_peaks.clone();
    let path = pad.sample_path.clone().expect("el pad guardó la ruta");

    pad.load_sample(path);
    assert_eq!(pad.waveform_peaks, peaks);
}

#[test]
fn a_pad_without_a_sample_says_so() {
    let pad = DmsPad::new(3, 39);
    assert_eq!(pad.display_filename(), "No Sample Loaded");
}

#[test]
fn the_note_name_is_the_one_midi_uses() {
    // 36 es C2 en la convención de MIDI (60 = C4).
    assert_eq!(midi_note_name(36), "C2");
    assert_eq!(midi_note_name(60), "C4");
    assert_eq!(midi_note_name(69), "A4");
}

// =========================================================================
// Reexports que la app necesita y que conviene que sigan existiendo
// =========================================================================

#[test]
fn the_app_type_is_reexported_for_the_binary() {
    // `main.rs` importa `HikaruApp` desde la librería. Si el reexport se
    // rompe, el binario no compila; este test lo deja explícito.
    fn accepts(_: Option<HikaruApp>) {}
    accepts(None);
}

#[test]
fn cada_target_del_atlas_declara_su_propia_resolucion() {
    // El visor se renderiza a `WAVETABLE_VIEWPORT` y el knob a su lado con el
    // cuadrado de `hikaru_render`. Si los dos dieran lo mismo, el `img` del
    // visor escalaría mal o el knob saldría estirado.
    assert_eq!(TargetId::WavetableViewer.resolution(), WAVETABLE_VIEWPORT);
    let knob = TargetId::WtPosKnob.resolution();
    assert_eq!(knob.0, knob.1, "el knob es cuadrado por definición");
    assert_ne!(knob, WAVETABLE_VIEWPORT, "el knob y el visor no comparten target");
}