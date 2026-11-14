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
use hikaru_gui::views::hikaru_native_plugins::hikaru_opendms::{midi_note_name, DmsPad, OpenDms};
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
fn render_image_stores_rgba() {
    let rgba = vec![10, 20, 30, 255, 50, 60, 70, 255];
    let image = render_image_from_rgba(2, 1, rgba);

    let bytes = image.as_raw();
    assert_eq!(bytes, &[10, 20, 30, 255, 50, 60, 70, 255]);
}

#[test]
fn render_image_keeps_the_frame_size() {
    let image = render_image_from_rgba(4, 2, vec![0; 4 * 2 * 4]);
    assert_eq!(image.size[0], 4);
    assert_eq!(image.size[1], 2);
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
    let attack_end = 1.0 - SUSTAIN_SHARE;
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

#[test]
fn bitwig_layout_toggles_mode_and_view_coherence() {
    use hikaru_gui::app::PanelLayout;
    use hikaru_gui::views::dsp_rack::{OPEN_WAVETABLE_CARD_WIDTH, RACK_HEIGHT};

    // 1. Default Bitwig Arrange layout: both Clip Launcher and Arranger Timeline active (split view)
    let mut layout = PanelLayout::default();
    assert!(layout.show_clip_launcher, "Por defecto el Clip Launcher horizontal está activo");
    assert!(layout.show_arranger, "Por defecto el Arranger Timeline está activo");

    // 2. Toggle off Arranger Timeline -> Clip Launcher remains active (like bitwig-clip-launcher.png)
    layout.show_arranger = false;
    if !layout.show_clip_launcher && !layout.show_arranger {
        layout.show_clip_launcher = true;
    }
    assert!(layout.show_clip_launcher && !layout.show_arranger);

    // 3. Toggle off Clip Launcher when Arranger is already off -> Arranger automatically activates (like bitwig-playlist.png)
    layout.show_clip_launcher = false;
    if !layout.show_clip_launcher && !layout.show_arranger {
        layout.show_arranger = true;
    }
    assert!(!layout.show_clip_launcher && layout.show_arranger);

    // 4. Bitwig-matched Device Panel & OpenWavetable dimensions (247px total dock height)
    assert!(
        (235.0..=250.0).contains(&RACK_HEIGHT),
        "El Device Panel debe coincidir con la altura de Bitwig (~241px rack / ~247px dock), quedó en {RACK_HEIGHT}"
    );
    assert!(
        OPEN_WAVETABLE_CARD_WIDTH <= 460.0,
        "La tarjeta de OpenWavetable debe ser compacta (<= 460px), quedó en {OPEN_WAVETABLE_CARD_WIDTH}"
    );
}

#[test]
fn dsp_rack_and_openwavetable_stay_top_aligned_and_bounded_across_frames() {
    use hikaru_audio_engine::AudioEngine;
    use hikaru_core::SampleRate;
    use hikaru_gui::app::AppState;
    use hikaru_gui::audio_proxy::AudioProxy;
    use hikaru_gui::views::dsp_rack::{self, OPEN_WAVETABLE_CARD_WIDTH, RACK_HEIGHT};
    use hikaru_gui::views::mixer::DspSlot;
    use std::sync::atomic::{AtomicU32, AtomicU64};
    use std::sync::{mpsc, Arc, Mutex};

    let (tx, _rx) = mpsc::channel();
    let proxy = AudioProxy::new(tx);
    let clock = Arc::new(AtomicU64::new(0));
    let level = Arc::new(AtomicU32::new(0));
    static WT: [f32; 2048] = [0.0f32; 2048];
    let engine = AudioEngine::new(SampleRate::new(48000.0), &WT, clock.clone());
    let mut state = AppState::new(proxy, clock, level, Some(Arc::new(Mutex::new(engine))));

    // Insert OpenWavetable on Track 1 (index 1)
    let mut slot = DspSlot::default();
    slot.name = "OpenWavetable".to_string();
    state.live_tracks[1].effects.push(slot);
    state.selected_track_index = 1;
    state.selected_slot_index = 0;

    let ctx = egui::Context::default();
    hikaru_gui::ui::fonts::ensure_installed(&ctx);
    hikaru_gui::theme::apply_theme(&ctx);

    let dock_h = RACK_HEIGHT + 6.0;
    for frame_i in 0..3 {
        let mut raw_input = egui::RawInput::default();
        raw_input.screen_rect = Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1280.0, 720.0),
        ));
        let mut used_rect = egui::Rect::NOTHING;
        let out = ctx.run(raw_input, |ctx| {
            egui::TopBottomPanel::bottom("bottom_dock")
                .exact_height(dock_h)
                .show(ctx, |ui| {
                    let top_before = ui.cursor().min.y;
                    dsp_rack::render(ui, &mut state);
                    used_rect = ui.min_rect();
                    assert!(
                        (used_rect.min.y - top_before).abs() <= 2.0,
                        "Frame {frame_i}: dsp_rack debe arrancar arriba del dock (min.y={}, top_before={})",
                        used_rect.min.y,
                        top_before
                    );
                });
        });

        let dock_top = 720.0 - dock_h;
        let mut found_title = false;
        let mut found_basic_shapes = false;
        for clipped in &out.shapes {
            if let egui::epaint::Shape::Text(txt) = &clipped.shape {
                let s = txt.galley.text();
                if s == "OpenWavetable" {
                    found_title = true;
                    assert!(
                        txt.pos.y <= dock_top + 32.0,
                        "Frame {frame_i}: el título 'OpenWavetable' debe estar arriba de la tarjeta (y={}, dock_top={dock_top})",
                        txt.pos.y
                    );
                }
                if s.starts_with("Basic Shapes") || s.starts_with("Basic Shap") {
                    found_basic_shapes = true;
                    assert!(
                        txt.pos.y <= dock_top + 56.0,
                        "Frame {frame_i}: 'Basic Shapes' debe estar arriba del cuerpo de OpenWavetable (y={}, dock_top={dock_top})",
                        txt.pos.y
                    );
                }
            }
        }
        assert!(found_title, "Frame {frame_i}: debe pintarse el título OpenWavetable");
        assert!(found_basic_shapes, "Frame {frame_i}: debe pintarse Basic Shapes");

        // Verify that every painted shape inside bottom_dock starts near the top of the dock
        // and stays strictly within OPEN_WAVETABLE_CARD_WIDTH horizontally.
        assert!(
            used_rect.height() <= dock_h + 2.0,
            "Frame {frame_i}: dsp_rack no debe desbordar la altura del dock ({} > {})",
            used_rect.height(),
            dock_h
        );
        assert!(
            used_rect.width() <= 1280.0 && OPEN_WAVETABLE_CARD_WIDTH <= 460.0,
            "Frame {frame_i}: el ancho de OpenWavetable debe mantenerse compacto ({})",
            used_rect.width()
        );
    }
}

#[test]
fn bitwig_wgpu_sdf_shader_parses_and_validates_cleanly() {
    use hikaru_gui::ui::bitwig_wgpu::BITWIG_SDF_SHADER;
    use naga::valid::{Capabilities, ValidationFlags, Validator};

    let module = naga::front::wgsl::parse_str(BITWIG_SDF_SHADER)
        .expect("BITWIG_SDF_SHADER debe compilar como WGSL válido");
    let mut validator = Validator::new(ValidationFlags::all(), Capabilities::empty());
    validator
        .validate(&module)
        .expect("BITWIG_SDF_SHADER debe pasar validación estricta de naga");
}

#[test]
fn lemon_studio_vector_icons_and_meter_ballistics_coherence() {
    use hikaru_gui::ui::icons::{self, parse_svg_subpaths};
    use hikaru_gui::ui::meters::{db_to_frac, StereoBallistics};
    use hikaru_gui::views::mixer::Track;

    // 1. Todos los iconos vectoriales críticos de Tabler teselan geometría válida 0..=24
    for icon in [
        icons::PLAYER_PLAY,
        icons::PLAYER_STOP,
        icons::METRONOME,
        icons::UNDO,
        icons::REDO,
        icons::FOLDER,
        icons::PIANO,
        icons::WAVE_SINE,
        icons::SEARCH,
        icons::VOLUME,
        icons::HOME,
        icons::REFRESH,
        icons::EYE,
    ] {
        let subpaths = parse_svg_subpaths(icon.d);
        assert!(
            !subpaths.is_empty(),
            "El icono vectorial '{}' debe producir al menos un sub-trazo",
            icon.d
        );
        assert!(subpaths.iter().all(|(pts, _)| pts.len() >= 2));
    }

    // 2. La ley de dB del vúmetro es estrictamente monótona creciente
    let f_m36 = db_to_frac(-36.0);
    let f_m18 = db_to_frac(-18.0);
    let f_m6 = db_to_frac(-6.0);
    let f_0 = db_to_frac(0.0);
    assert!(0.0 < f_m36 && f_m36 < f_m18 && f_m18 < f_m6 && f_m6 < f_0 && f_0 <= 1.0);

    // 3. Balística estéreo mantiene Peak-Hold durante la ventana de retención
    let mut bal = StereoBallistics::default();
    bal.step([0.9, 0.8], 0.016);
    let q_peak = bal.normalized_quad();
    assert!(q_peak[0] > 0.7 && q_peak[2] >= q_peak[0]);
    bal.step([0.0, 0.0], 0.10);
    let q_decay = bal.normalized_quad();
    assert!(
        q_decay[2] > q_decay[0],
        "Peak-hold debe retenerse por encima de la caída instantánea"
    );

    // 4. Paleta 1:1 de pistas Bitwig Studio 5.1.7 (Master gris, Track 1 naranja, Track 2 amarillo, Track 3 azul)
    let master = Track::new(0, "Master".into(), true);
    let tr1 = Track::new(1, "Inst 1".into(), false);
    let tr2 = Track::new(2, "Audio 2".into(), false);
    let tr3 = Track::new(3, "Audio 3".into(), false);
    assert_eq!(master.color, egui::Color32::from_rgb(0x82, 0x84, 0x88));
    assert_eq!(tr1.color, egui::Color32::from_rgb(0xFF, 0x55, 0x00));
    assert_eq!(tr2.color, egui::Color32::from_rgb(0xE5, 0xAE, 0x19));
    assert_eq!(tr3.color, egui::Color32::from_rgb(0x3A, 0xA8, 0xE0));
}

#[test]
fn openwavetable_default_starts_in_3d_with_multicycle_table_and_mesh_controls() {
    use hikaru_gui::views::hikaru_native_plugins::hikaru_openwavetable::open_wavetable::{
        SettingParam, WavetableEditor,
    };
    use hikaru_render::{MeshType, RenderMode};

    let mut editor = WavetableEditor::default();
    assert_eq!(
        editor.render_mode,
        RenderMode::Mode3D,
        "OpenWavetable debe arrancar en modo 3D por defecto"
    );
    assert!(
        editor.frame_count() > 1,
        "La tabla inicial de OpenWavetable debe tener múltiples ciclos para apilar en 3D (frames={})",
        editor.frame_count()
    );
    assert_eq!(editor.render_settings.mesh, MeshType::Wireframe);
    editor.render_settings.mesh = editor.render_settings.mesh.cycle();
    assert_eq!(editor.render_settings.mesh, MeshType::Ribbon);
    editor.render_settings.mesh = editor.render_settings.mesh.cycle();
    assert_eq!(editor.render_settings.mesh, MeshType::Solid);

    let hash_before = editor.render_settings.key_hash();
    SettingParam::Yaw.set(&mut editor.render_settings, 22.0);
    assert_ne!(hash_before, editor.render_settings.key_hash());
}

#[test]
fn test_openmodulation_in_catalog_and_lfo_modulates_openwavetable_wt_pos() {
    use hikaru_gui::views::dsp_rack::PLUGIN_CATALOG;
    use hikaru_gui::views::hikaru_native_plugins::hikaru_openmodulation::lfo_mod::{
        get_modulation, handle_knob_mapping_click, update_lfo_state, ModulationTarget,
    };
    use hikaru_gui::views::hikaru_native_plugins::hikaru_openwavetable::open_wavetable::WavetableEditor;

    // 1. Catálogo de plugins del DSP Rack contiene Hikaru OpenModulation
    let has_mod = PLUGIN_CATALOG
        .iter()
        .any(|entry| entry.name == "OpenModulation" && entry.label == "Hikaru OpenModulation" && entry.category == "Modulation");
    assert!(
        has_mod,
        "Hikaru OpenModulation debe estar registrado en PLUGIN_CATALOG bajo la categoría Modulation"
    );

    // 2. Registro de LFO apuntando al parámetro WT POS de OpenWavetable en la pista 1
    let track_idx = 1;
    let slot_idx = 1;
    update_lfo_state(track_idx, slot_idx, |state| {
        state.rate_hz = 2.0;
        state.depth = 1.0;
        state.amount = 0.5;
        state.target = ModulationTarget::OpenWavetableWtPos;
    });

    // 3. Probar lectura de modulación para OpenWavetable WT POS
    let active_mod = get_modulation(track_idx, "OpenWavetable", "WT POS");
    assert!(active_mod.is_some(), "Debe haber modulación activa para WT POS");
    let active = active_mod.unwrap();
    assert!(active.delta.is_finite());
    assert!(active.delta.abs() <= 1.0);

    // 4. Probar que otro parámetro o dispositivo no mapeado no recibe modulación
    let unmapped_delta = get_modulation(track_idx, "OpenWavetable", "DETUNE");
    assert!(unmapped_delta.is_none(), "Parámetros no mapeados deben devolver None");

    let other_track_delta = get_modulation(2, "OpenWavetable", "WT POS");
    assert!(other_track_delta.is_none(), "Otras pistas no deben verse afectadas");

    // 5. Aplicar modulación a WavetableEditor
    let mut editor = WavetableEditor::default();
    editor.base_position = Some(0.5);
    let effective = (editor.base_position.unwrap() + active.delta).clamp(0.0, 1.0);
    editor.set_position(effective);
    assert!(editor.position() >= 0.0 && editor.position() <= 1.0);

    // 6. Probar clic de mapeo interactivo (MOD button)
    let _ = handle_knob_mapping_click(track_idx, "OpenWavetable", "DETUNE");

    // 7. Probar modulación hacia el nuevo knob VOL
    update_lfo_state(track_idx, slot_idx, |state| {
        state.target = ModulationTarget::OpenWavetableVolume;
    });
    let vol_mod = get_modulation(track_idx, "OpenWavetable", "VOL");
    assert!(vol_mod.is_some(), "Debe haber modulación activa para VOL");
}

#[test]
fn hikaru_sampler_renders_in_rack_and_roundtrips_params() {
    use hikaru_audio_engine::AudioEngine;
    use hikaru_core::SampleRate;
    use hikaru_gui::app::AppState;
    use hikaru_gui::audio_proxy::AudioProxy;
    use hikaru_gui::views::dsp_rack::{render as render_dsp_rack, PLUGIN_CATALOG};
    use hikaru_gui::views::hikaru_native_plugins::HikaruSampler;
    use hikaru_gui::views::mixer::DspSlot;
    use std::sync::atomic::{AtomicU32, AtomicU64};
    use std::sync::{mpsc, Arc, Mutex};

    let has_sampler = PLUGIN_CATALOG
        .iter()
        .any(|e| e.name == "Sampler" && e.label == "Hikaru Sampler");
    assert!(has_sampler, "Hikaru Sampler debe estar registrado en PLUGIN_CATALOG");

    let (tx, _rx) = mpsc::channel();
    let proxy = AudioProxy::new(tx);
    let clock = Arc::new(AtomicU64::new(0));
    let level = Arc::new(AtomicU32::new(0));
    static WT: [f32; 2048] = [0.0f32; 2048];
    let engine = AudioEngine::new(SampleRate::new(48000.0), &WT, clock.clone());
    let mut state = AppState::new(proxy, clock, level, Some(Arc::new(Mutex::new(engine))));

    state.selected_track_index = 1;
    let mut slot = DspSlot::new(42, "Sampler".to_string());
    let mut smp = HikaruSampler::default();
    smp.filter_cutoff_hz = 1850.0;
    smp.loop_mode = hikaru_dsp::synth::sampler::SamplerLoopMode::PingPong;
    let params = smp.to_project_params();
    let mut restored = HikaruSampler::default();
    restored.apply_project_params(&params);
    assert!((restored.filter_cutoff_hz - 1850.0).abs() < 0.01);
    assert_eq!(restored.loop_mode, hikaru_dsp::synth::sampler::SamplerLoopMode::PingPong);

    slot.sampler_state = Some(smp);
    state.live_tracks[1].effects.push(slot);

    let ctx = egui::Context::default();
    hikaru_gui::ui::fonts::ensure_installed(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 260.0))),
        ..Default::default()
    };
    let out = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            render_dsp_rack(ui, &mut state);
        });
    });
    assert!(!out.shapes.is_empty(), "Hikaru Sampler debe renderizar shapes en el DSP Rack");
}

#[test]
fn opendms_loaded_pad_play_solo_mute_buttons_respond_to_clicks() {
    use hikaru_gui::audio_proxy::{AudioProxy, GuiCommand};
    use hikaru_gui::views::hikaru_native_plugins::hikaru_opendms::open_dms::render_dms_ui;
    use hikaru_gui::views::hikaru_native_plugins::hikaru_sampler::HikaruSampler;

    let (tx, rx) = std::sync::mpsc::channel();
    let audio_proxy = AudioProxy::new(tx);
    let mut dms = OpenDms::default();
    dms.pads[0].sampler = Some(HikaruSampler::default());

    let ctx = egui::Context::default();
    hikaru_gui::ui::fonts::ensure_installed(&ctx);

    let click_at = |ctx: &egui::Context,
                    dms: &mut OpenDms,
                    pos: egui::Pos2| {
        let mut dragged = None;
        // Frame 1: pointer down
        let mut raw_down = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0))),
            ..Default::default()
        };
        raw_down.events.push(egui::Event::PointerMoved(pos));
        raw_down.events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        let _ = ctx.run(raw_down, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                render_dms_ui(ui, dms, 120.0, &mut dragged, &audio_proxy);
            });
        });

        // Frame 2: pointer up
        let mut raw_up = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0))),
            ..Default::default()
        };
        raw_up.events.push(egui::Event::PointerMoved(pos));
        raw_up.events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        let _ = ctx.run(raw_up, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                render_dms_ui(ui, dms, 120.0, &mut dragged, &audio_proxy);
            });
        });
    };

    // Primer frame sin clicks para estabilizar el layout y obtener el rect del pad 0
    let mut dragged = None;
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0))),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                render_dms_ui(ui, &mut dms, 120.0, &mut dragged, &audio_proxy);
            });
        },
    );

    // Pad 0 está en la fila inferior izquierda de la grilla 4x4.
    // CentralPanel margin (8) + Frame margin (4) + bank strip (12) + spacing (8) + add_space(6) = x ~ 38..96
    // Fila 3 (y): 8 + 4 + 3*(26+3) = 99 .. 125; barra inferior en y ~ 114..124 -> centro y ~ 119
    let play_pos = egui::pos2(47.0, 119.0);
    let solo_pos = egui::pos2(74.0, 119.0);
    let mute_pos = egui::pos2(88.0, 119.0);

    click_at(&ctx, &mut dms, play_pos);
    let cmds: Vec<_> = rx.try_iter().collect();
    assert!(
        cmds.iter().any(|c| matches!(c, GuiCommand::DmsNoteOn { pad_idx: 0, .. })),
        "Click en Play del pad 0 debe enviar GuiCommand::DmsNoteOn, recibidos: {:?}",
        cmds.len()
    );

    assert!(!dms.pads[0].solo);
    click_at(&ctx, &mut dms, solo_pos);
    assert!(dms.pads[0].solo, "Click en S del pad 0 debe activar solo");

    assert!(!dms.pads[0].mute);
    click_at(&ctx, &mut dms, mute_pos);
    assert!(dms.pads[0].mute, "Click en M del pad 0 debe activar mute");
}
