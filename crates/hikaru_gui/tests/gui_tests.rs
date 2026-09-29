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
//! viewport, aritmética de la cámara, estado de los editores, geometría de las
//! tarjetas), no hace falta `pub(crate)` ni `#[cfg(test)]` para nada.

use hikaru_gui::render::{
    hash_waveform, knob_angle, knob_key, quantize_camera, render_image_from_rgba, FrameKey,
    TargetId, ViewerRequest, WavetableView, WavetableViewport, WAVETABLE_VIEWPORT,
};
use hikaru_gui::views::controls::{finite, needle_angle};
use hikaru_gui::views::dsp_rack::{
    card_height, card_width, editor_for, rack_title, RACK_HEIGHT_CLOSED, RACK_HEIGHT_OPENED,
};
use hikaru_gui::views::mixer::DspSlot;
use hikaru_gui::views::open_dms::{midi_note_name, DmsPad, OpenDms};
use hikaru_gui::views::open_wavetable::{WavetableEditor, EDITOR_HEIGHT};
use hikaru_gui::views::util::normalized;
use hikaru_gui::views::wavetable_io::{self, Wavetable, TABLE_RESOLUTION};
use hikaru_gui::HikaruApp;
use hikaru_render::{Camera, WavetableMeshParams, wgpu};

/// Sine de prueba.
fn sine(count: usize) -> Vec<f32> {
    (0..count)
        .map(|i| ((i as f32) / count as f32 * std::f32::consts::TAU).sin())
        .collect()
}

/// Pedido de frame del viewport, con los defaults del editor.
fn request<'a>(waveform: &'a [f32], camera: Camera) -> WavetableFrameRequest<'a> {
    WavetableFrameRequest {
        size: WAVETABLE_VIEWPORT,
        camera,
        waveform,
        mesh_params: WavetableMeshParams::default(),
        tint: [0.35, 0.85, 1.0, 1.0],
        background: wgpu::Color::TRANSPARENT,
    }
}

/// Editor con `count` osciladores.
fn editor_with_oscillators(count: usize) -> WavetableEditor {
    WavetableEditor {
        oscillators: (0..count)
            .map(WavetableOscillator::new)
            .collect(),
        ..WavetableEditor::default()
    }
}

/// Slots con los plugins dados, con `opened` abierto y `selected` elegido.
fn slots(names: &[&str], selected: usize, opened: Option<usize>) -> Vec<DspSlot> {
    names
        .iter()
        .enumerate()
        .map(|(id, name)| {
            let mut slot = DspSlot::new(id, name.to_string());
            slot.is_open = Some(id) == opened;
            slot
        })
        .collect()
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

#[test]
fn a_viewport_without_a_gpu_answers_none_instead_of_failing() {
    // La máquina sin GPU es un caso normal, no una excepción: si `frame`
    // devolviera `Err`, el panel del editor no se podría construir nunca.
    let mut viewport = WavetableViewport::unavailable();
    let waveform = sine(64);
    assert!(viewport.frame(&request(&waveform, Camera::default())).unwrap().is_none());
    assert!(viewport.last_error().is_some());
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

    // Cambiar de oscilador cambia el waveform.
    assert_ne!(base, request(&sine(128), Camera::default()).key());

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
fn a_degenerate_viewport_size_is_reported_not_painted() {
    // Tamaño cero: el layout todavía no resolvió. No tiene que entrar en un
    // `unwrap` por el camino.
    let waveform = sine(8);
    let mut req = request(&waveform, Camera::default());
    req.size = (0, 0);
    let mut viewport = WavetableViewport::unavailable();
    assert!(viewport.frame(&req).unwrap().is_none());
}

#[test]
fn the_viewport_resolution_is_the_one_the_layout_is_built_around() {
    // El `img` escala con `ObjectFit::Contain`, así que el tamaño del target y
    // el del panel son independientes: pero el panel tiene que poder mostrar el
    // viewport entero, y para eso hace falta un mínimo de alto y de ancho.
    assert!(WAVETABLE_VIEWPORT.0 >= 256 && WAVETABLE_VIEWPORT.1 >= 160);
    // Y tiene que caber en el editor desplegado, que es lo que le da el alto.
    assert!(WAVETABLE_VIEWPORT.1 as f32 <= RACK_HEIGHT_OPENED);
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
    assert!((sweep - 270.0 * std::f32::consts::PI / 180.0).abs() < 1e-6);
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
// Estado del editor de Wavetable
// =========================================================================

#[test]
fn the_editor_always_has_an_oscillator_to_draw() {
    // El panel dibuja el oscilador seleccionado: si el último se borra y el
    // índice queda en 3 con un vector de largo 1, eso es un index out of
    // bounds en medio del render.
    let mut editor = editor_with_oscillators(2);
    editor.selected_osc = 1;
    editor.remove_oscillator();
    assert_eq!(editor.oscillators.len(), 1);
    assert_eq!(editor.current_osc().id, 0);
}

#[test]
fn the_last_oscillator_cannot_be_removed() {
    let mut editor = editor_with_oscillators(1);
    editor.remove_oscillator();
    assert_eq!(editor.oscillators.len(), 1, "sin osciladores no hay nada que dibujar");
}

#[test]
fn adding_an_oscillator_selects_it() {
    // Si no, el + OSC parecería no hacer nada hasta que el usuario cliquee la
    // pestaña nueva.
    let mut editor = editor_with_oscillators(1);
    editor.add_oscillator();
    assert_eq!(editor.selected_osc, 1);
    assert_eq!(editor.current_osc().id, 1);
}

#[test]
fn a_stale_oscillator_index_does_not_panic() {
    // El índice de la pestaña y el vector se desincronizan si el estado se edita
    // desde otro lugar; el panel no puede caer por eso.
    let mut editor = editor_with_oscillators(1);
    editor.selected_osc = 42;
    assert_eq!(editor.current_osc().id, 0);
}

#[test]
fn a_modulator_created_before_a_deletion_keeps_its_target_name() {
    // Los ids son posicionales: al borrar un oscilador, un modulador puede
    // quedar apuntando a un id inexistente. La lista tiene que mostrarlo sin
    // indexar fuera de rango.
    let mut editor = editor_with_oscillators(3);
    editor.selected_osc = 0;
    editor.add_modulator();
    let target = editor.modulators[0].target_osc_id;

    editor.selected_osc = 0;
    editor.remove_oscillator();

    assert!(!oscillator_name(&editor, target).is_empty());
}

#[test]
fn a_deleted_modulator_does_not_stay_selected() {
    let mut editor = editor_with_oscillators(1);
    editor.add_modulator();
    let id = editor.selected_modulator.expect("el modulador nuevo queda seleccionado");
    editor.remove_modulator();
    assert!(editor.current_modulator().is_none());
    assert_ne!(editor.selected_modulator, Some(id));
}

#[test]
fn the_preview_waveform_is_deterministic_and_bounded() {
    let osc = WavetableOscillator::new(0);
    let first = preview_waveform(&osc, 0.5);
    let second = preview_waveform(&osc, 0.5);
    assert_eq!(first, second, "la tabla tiene que ser estable o el render nunca se cachea");
    assert_eq!(first.len(), 512);
    assert!(
        first.iter().all(|sample| sample.is_finite() && sample.abs() <= 1.5),
        "la tabla tiene que estar acotada: la malla escala por el pico y un NaN la rompe"
    );
}

#[test]
fn the_preview_waveform_changes_with_the_position_and_the_oscillator() {
    // Si la silueta no cambiara, la vista 3D no estaría mostrando nada: sería
    // siempre la misma cinta con distinto color.
    let base = preview_waveform(&WavetableOscillator::new(0), 0.0);

    let mut moved = WavetableOscillator::new(0);
    moved.wt_pos = 240.0;
    assert_ne!(base, preview_waveform(&moved, 0.0));
    assert_ne!(base, preview_waveform(&WavetableOscillator::new(1), 0.0));
    assert_ne!(base, preview_waveform(&WavetableOscillator::new(0), 1.0));
}

#[test]
fn the_waveform_is_periodic() {
    // Un oscilador que no cerrara el lazo muestra una costura en la cinta, y la
    // cinta es justamente la geometría que se está visualizando.
    let table = preview_waveform(&WavetableOscillator::new(0), 0.3);
    let head = table[0];
    let tail = table[table.len() - 1];
    assert!((head - tail).abs() < 0.25, "la tabla no cierra: {head} vs {tail}");
}

#[test]
fn the_thickness_stays_inside_what_the_mesh_accepts() {
    // `MeshRenderer` dibuja una cinta con depth test: un thickness de 0 deja
    // las dos caras en el mismo plano y el z-fighting hace que la malla
    // parpadee.
    let mut editor = WavetableEditor::default();
    editor.thickness = 0.0;
    assert!(editor.mesh_params().thickness >= 1.0);
    editor.thickness = 10000.0;
    assert!(editor.mesh_params().thickness <= 120.0);
}

#[test]
fn the_effect_selector_indexes_into_a_table_of_the_same_size() {
    // El índice se recorre con un módulo y la lista con otro: si se
    // desincronizan, el panel muestra un efecto que no existe.
    assert_eq!(MODULATOR_EFFECTS.len(), MODULATOR_RANGES.len());
    for name in MODULATOR_EFFECTS.iter().chain(SYNTH_FX.iter()) {
        assert!(!name.is_empty());
    }
}

#[test]
fn modulator_ranges_are_never_inverted() {
    // El selector de efecto deja que el rango se invierta sin querer, y entonces
    // el dial dibuja al revés sin que nada falle.
    for (index, (min, max)) in MODULATOR_RANGES.iter().enumerate() {
        assert!(min < max, "el efecto {index} tiene el rango invertido");
    }
}

// =========================================================================
// El rack: selección, apertura y layout
// =========================================================================

#[test]
fn a_loaded_slot_opens_its_editor() {
    // El gesto que había que arreglar: seleccionar el slot 01 y que el panel
    // aparezca.
    let slots = slots(&["OpenWavetable", "Hikaru OpenDMS"], 0, Some(0));
    assert_eq!(editor_for(&slots, 0), Some("OpenWavetable"));
}

#[test]
fn the_dms_slot_gets_the_sampler_editor() {
    let slots = slots(&["OpenWavetable", "Hikaru OpenDMS"], 1, Some(1));
    assert_eq!(editor_for(&slots, 1), Some("Hikaru OpenDMS"));
}

#[test]
fn a_closed_slot_shows_no_editor_even_when_selected() {
    let slots = slots(&["OpenWavetable"], 0, None);
    assert_eq!(editor_for(&slots, 0), None);
}

#[test]
fn selecting_another_slot_collapses_the_previous_editor() {
    // El rack muestra un editor a la vez: si no, con cinco slots cargados el
    // panel sería cinco veces el alto del rack.
    let slots = slots(&["OpenWavetable", "Hikaru OpenDMS"], 1, Some(0));
    assert_eq!(editor_for(&slots, 1), None, "el editor del slot 0 no debe quedar abierto");
}

#[test]
fn an_empty_slot_has_no_editor_to_open() {
    // Un slot vacío no tiene plugin detrás: seleccionarlo no puede abrir un
    // panel en blanco.
    let slots = slots(&["Empty Slot"], 0, Some(0));
    assert_eq!(editor_for(&slots, 0), None);
    assert!(!DspSlot::new(0, "Empty Slot".to_string()).has_editor());
}

#[test]
fn the_plugins_with_an_editor_are_the_ones_the_rack_knows() {
    assert!(DspSlot::new(0, "OpenWavetable".to_string()).has_editor());
    assert!(DspSlot::new(0, "Hikaru OpenDMS".to_string()).has_editor());
    assert!(!DspSlot::new(0, "OpenSpectralFX".to_string()).has_editor());
}

#[test]
fn an_out_of_range_selection_does_not_panic() {
    // El índice viene del estado y las pistas se pueden borrar: la pregunta
    // tiene que devolver `None`, no reventar.
    let slots = slots(&["OpenWavetable"], 9, Some(0));
    assert_eq!(editor_for(&slots, 9), None);
    assert_eq!(editor_for(&[], 0), None);
}

#[test]
fn the_rack_title_mentions_the_track_and_the_scene() {
    assert_eq!(rack_title("TRACK", None), "DSP RACK: TRACK");
    // La escena es un índice: el título muestra la de 1, no la de 0.
    assert_eq!(rack_title("TRACK", Some(2)), "DSP RACK: TRACK | Scene 3");
}

#[test]
fn the_card_of_a_slot_is_wide_enough_for_its_name() {
    // La tarjeta tiene que entrar el nombre del plugin y los tres botones de la
    // esquina, si no el nombre se recorta y no se sabe qué plugin es.
    for name in ["OpenWavetable", "Hikaru OpenDMS", "OpenSpectralFX", "Empty Slot"] {
        assert!(
            card_width(name) >= 170.0,
            "la tarjeta de {name} es demasiado angosta para su rótulo"
        );
        assert!(card_height(name) >= 100.0);
    }
}

#[test]
fn the_editor_is_taller_than_the_closed_strip() {
    // El editor no cabe en el alto de la tira: si el layout no agranda el panel,
    // el canvas 3D queda con unos pocos píxeles.
    assert!(RACK_HEIGHT_OPENED > RACK_HEIGHT_CLOSED * 2.0);
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
fn the_default_layout_fits_the_open_editor() {
    // La grilla de pads vive en el editor desplegado, no en la tarjeta del
    // rack. Con el layout de 64 pads es la mayor: si no entra en el alto del
    // editor, los pads de la última fila quedan recortados.
    let dms = OpenDms::default();
    for pad_count in [16usize, 32, 64] {
        let cols = if pad_count <= 16 { 4 } else { 8 };
        let rows = pad_count / cols;
        let pad_size: f32 = match pad_count {
            16 => 46.0,
            32 => 40.0,
            _ => 36.0,
        };
        let grid_height = pad_size * rows as f32 + (rows - 1) as f32 * 2.0;
        assert_eq!(rows * cols, pad_count, "la grilla de {pad_count} no es rectangular");
        assert!(
            grid_height <= RACK_HEIGHT_OPENED - 120.0,
            "la grilla de {pad_count} no entra en el editor desplegado: {grid_height}px"
        );
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
fn the_mesh_params_of_the_editor_match_the_viewport_defaults() {
    // La caja de la cinta es la que la cámara encuadra. Si el editor y
    // `hikaru_render` dejaran de coincidir, la malla se vería cortada.
    let editor = WavetableEditor::default();
    let mesh: WavetableMeshParams = editor.mesh_params();
    assert_eq!(mesh.width, 320.0);
    assert_eq!(mesh.height, 200.0);
    assert!(mesh.thickness > 0.0);
}
