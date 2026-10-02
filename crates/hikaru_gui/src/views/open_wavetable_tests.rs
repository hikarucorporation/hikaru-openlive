// crates/hikaru_gui/src/views/open_wavetable_tests.rs

//! Tests de la interacción del knob `WT POS`.
//!
//! # Por qué viven en su propio archivo
//!
//! No es una cuestión de orden ni de tamaño: un `#[cfg(test)] mod tests` dentro
//! de `open_wavetable.rs` no compila. Ese módulo hace `use gpui_kit::*`, y al
//! combinar ese glob con el `use super::*` que el harness de `#[test]` inyecta,
//! el compilador se entra en una recursión de expansión que no termina: sube el
//! límite de recursión de 128 a 512, a 1024, y pide 2048, siempre sin converger.
//!
//! Es un problema del compilador, no del código, y sube el `recursion_limit` no
//! lo arregla. Lo que funciona es sacar los tests a un módulo hermano que
//! importa lo que necesita **por nombre**. Eso los deja sin el glob que dispara
//! la recursión, y compilan al toque.
//!
//! `wavetable_io.rs` ya tenía tests y nunca felló, y es la prueba de que el
//! diagnóstico es el glob y no el hecho de testear este crate: ese módulo no
//! importa `gpui_kit::*`.

use super::open_wavetable::{SettingParam, WavetableEditor, DRAG_RANGE_PX};
use super::wavetable_io::Wavetable;

/// El ángulo de la marca del knob, en grados, que es como se lee en pantalla.
///
/// Se re-deriva del ángulo en radianes que devuelve el renderer en vez de
/// re-exportar ese ángulo: el módulo bajo prueba no debe importar `render` para
/// no arrastrar el contexto de wgpu a un test de layout.
fn marker_degrees(fill: f32) -> f32 {
    crate::render::knob_angle(fill).to_degrees()
}

/// Un editor con la tabla de fábrica, que es la que tiene 256 ciclos.
fn factory_editor() -> WavetableEditor {
    WavetableEditor::default()
}

/// El número de ciclo que muestra el pie del panel. Va 1-based.
fn displayed_cycle(editor: &WavetableEditor) -> usize {
    editor.frame + 1
}

#[test]
fn dragging_all_the_way_down_reaches_the_first_cycle() {
    // El bug que reportó el usuario: al arrastrar el knob hacia abajo se quedaba
    // en el ciclo 67 en vez de llegar al 0.
    //
    // La causa no estaba en esta matemática sino en que el seguimiento del
    // arrastre moría al salir del módulo (ver `register_drag_tracking`), así que
    // estos tests no la habrían detectado: fijan el rango, no el wiring. Aun
    // así, el recorrido completo tiene que llegar a los dos extremos, y son las
    // dos aserciones que se rompen si alguien cambia `drag_to`.
    let mut editor = factory_editor();
    assert_eq!(displayed_cycle(&editor), 1, "la tabla de fábrica arranca en el ciclo 1");

    editor.begin_drag(1000.0);
    // Muy por debajo del tope: 2000px es unas tres veces el rango entero.
    editor.drag_to(2000.0, false);

    assert_eq!(editor.position(), 0.0, "el arrastre completo no llegó al mínimo");
    assert_eq!(displayed_cycle(&editor), 1, "el contador no quedó en el ciclo 1");
    assert_eq!(editor.frame, 0);
}

#[test]
fn dragging_all_the_way_up_reaches_the_last_cycle() {
    let mut editor = factory_editor();
    let last = editor.frame_count() - 1;

    editor.begin_drag(1000.0);
    editor.drag_to(0.0, false);

    assert_eq!(editor.position(), 1.0, "el arrastre completo no llegó al máximo");
    assert_eq!(editor.frame, last, "el contador no quedó en el último ciclo");
}

#[test]
fn the_drag_is_relative_to_the_grab_point() {
    // Agarrar el knob en cualquier punto del recorrido no lo teletransporta: el
    // valor sólo cambia con el delta desde donde se agarró. Con arrastre
    // absoluto, un error de 2px al agarrar el knob movería varios ciclos.
    let mut editor = factory_editor();
    editor.set_position(0.5);
    editor.begin_drag(1000.0);

    editor.drag_to(995.0, false);
    let first = editor.position();
    let expected = 0.5 + 5.0 / crate::views::open_wavetable::DRAG_RANGE_PX;
    assert!(
        (first - expected).abs() < 1.0e-4,
        "un delta de 5px dejó la posición en {first} y se esperaba {expected}"
    );

    // Y sigue avanzando en la misma dirección, acumulando.
    editor.drag_to(1005.0, false);
    assert!(editor.position() < 0.5, "el arrastre no volvió atrás al seguir subiendo");
}

#[test]
fn shift_makes_the_drag_four_times_finer() {
    let mut normal = factory_editor();
    let mut fine = factory_editor();

    normal.begin_drag(1000.0);
    fine.begin_drag(1000.0);
    normal.drag_to(900.0, false);
    fine.drag_to(900.0, true);

    let coarse = normal.position();
    let thin = fine.position();
    assert!(
        (coarse / 4.0 - thin).abs() < 1.0e-4,
        "con Shift se movió {thin} y sin Shift {coarse}; debería ser la cuarta parte"
    );
}

#[test]
fn position_and_active_cycle_stay_in_step() {
    // El contador del pie, el realce de la malla y la rotación del disco tienen
    // que hablar del mismo ciclo. Si `position()` y `active_cycle()` se
    // desfasaran, el knob apuntaría a un lado y la pila se iluminaría en otro.
    let mut editor = factory_editor();

    for step in 0..=40 {
        let target = step as f32 / 40.0;
        editor.set_position(target);

        let expected_cycle = target * (editor.frame_count() - 1) as f32;
        assert!(
            (editor.active_cycle() - expected_cycle).abs() < 1.0e-3,
            "posición {target}: active_cycle dio {} y se esperaba {expected_cycle}",
            editor.active_cycle()
        );
        assert!((editor.position() - target).abs() < 1.0e-3, "no vuelve la posición");
        assert!(editor.frame < editor.frame_count());
        assert!((0.0..1.0).contains(&editor.morph), "morph fuera de rango");
    }
}

#[test]
fn a_move_before_the_press_does_not_move_the_knob() {
    // El listener de movimiento vive en la ventana, así que puede llegar un
    // movimiento con el botón sin apretar. La guarda de `dragging` es lo que
    // evita que eso mueva el valor.
    let mut editor = factory_editor();
    editor.set_position(0.25);
    editor.drag_to(500.0, false);
    assert_eq!(editor.position(), 0.25, "movió el knob sin botón apretado");
}

#[test]
fn releasing_the_button_stops_the_drag() {
    let mut editor = factory_editor();
    editor.begin_drag(1000.0);
    editor.drag_to(950.0, false);
    assert!(editor.dragging);

    editor.end_drag();
    assert!(!editor.dragging);

    // Y un movimiento posterior ya no toca el valor.
    let after = editor.position();
    editor.drag_to(500.0, false);
    assert_eq!(editor.position(), after, "el arrastre siguió vivo tras soltar");
}

#[test]
fn a_single_cycle_table_stays_at_zero() {
    // Con un solo ciclo la posición no tiene a dónde ir: tiene que quedarse en 0
    // sin entrar en pánico. Es el estado en el que el knob se desactiva.
    let mut editor = WavetableEditor::with_table(Wavetable::default_sine());
    editor.set_position(0.9);
    assert_eq!(editor.position(), 0.0);

    editor.begin_drag(500.0);
    editor.drag_to(0.0, false);
    assert_eq!(editor.position(), 0.0);
}

#[test]
fn a_non_finite_drag_does_not_corrupt_the_position() {
    // Un `NaN` en la coordenada del mouse no se puede acotar con `clamp`: todas
    // las comparaciones dan falso y el valor se propagaría al uniforme de la
    // malla, donde la cinta desaparecería.
    let mut editor = factory_editor();
    editor.set_position(0.5);
    editor.begin_drag(f32::NAN);

    editor.drag_to(f32::NAN, false);
    assert!(editor.position().is_finite(), "un NaN dejó la posición rota");

    editor.set_position(f32::NAN);
    assert!(editor.position().is_finite());
}

#[test]
fn loading_a_table_drops_any_drag_in_progress() {
    // Un arrastre vivo con `drag_grab` de la matriz anterior saltaría a un ciclo
    // arbitrario de la nueva con el primer movimiento.
    let mut editor = factory_editor();
    editor.begin_drag(1000.0);
    editor.drag_to(900.0, false);
    assert!(editor.dragging);

    editor.load(Wavetable::default_sine());
    assert!(!editor.dragging, "el arrastre sobrevivió a la carga");
    assert_eq!(editor.position(), 0.0);
}

#[test]
fn the_marker_sweeps_the_full_270_degrees_of_a_dial() {
    // El bug que reportó el usuario: la marca recorría un cuarto de vuelta
    // entre las 12 y las 3, en vez de los 270° del dial. Con eso el knob no se
    // correspondía con su valor y no se asociaba a ningún control conocido.
    let start = marker_degrees(0.0);
    let end = marker_degrees(1.0);
    let sweep = start - end;

    assert!(
        (sweep - 270.0).abs() < 0.01,
        "el recorrido es de {sweep}° y debería ser de 270°"
    );
}

#[test]
fn the_ends_of_the_knob_are_the_ends_of_the_table() {
    // El pedido: el ciclo 0 tiene que estar en un extremo y el último en el otro.
    // Con el valor mínimo, la marca queda en las 7:30 y el contador en el primer
    // ciclo; con el máximo, en las 4:30 y en el último.
    let mut editor = factory_editor();
    let last_cycle = editor.frame_count();

    editor.begin_drag(1000.0);
    editor.drag_to(2000.0, false);
    assert!((marker_degrees(editor.position()) - 225.0).abs() < 0.01, "el mínimo no cae en las 7:30");
    assert_eq!(displayed_cycle(&editor), 1, "el mínimo no es el primer ciclo");

    editor.begin_drag(1000.0);
    editor.drag_to(0.0, false);
    assert!((marker_degrees(editor.position()) + 45.0).abs() < 0.01, "el máximo no cae en las 4:30");
    assert_eq!(displayed_cycle(&editor), last_cycle, "el máximo no es el último ciclo");
}

#[test]
fn the_marker_advances_the_same_way_throughout() {
    // El recorrido no puede doblarse: cada paso de valor gira la marca en el
    // mismo sentido. Un `clamp` mal puesto o un salto en el mapeo lo delataría.
    let mut previous = marker_degrees(0.0);
    for step in 1..=100 {
        let angle = marker_degrees(step as f32 / 100.0);
        assert!(angle < previous, "en el paso {step} la marca no avanzó ({angle} vs {previous})");
        previous = angle;
    }
}

#[test]
fn a_full_quarter_turn_moves_through_the_whole_table() {
    // La correspondencia que hace que el knob sea usable: un cuarto de vuelta
    // completo recorre la tabla entera, sin saltos ni zonas muertas.
    let mut editor = factory_editor();
    editor.begin_drag(1000.0);

    // Media vuelta del dial es la mitad de la tabla. Se arrastra hacia arriba
    // (Y menor), que es la dirección que aumenta el valor: `drag_to` calcula
    // `origen - actual`, así que bajar la Y sube el knob.
    editor.drag_to(1000.0 - 135.0 * DRAG_RANGE_PX / 270.0, false);
    assert!((editor.position() - 0.5).abs() < 0.01, "media vuelta no es la mitad de la tabla");
}

#[test]
fn setting_sliders_drag_horizontally_and_clamp_to_range() {
    // Los sliders del panel son relativos al agarre como el knob, pero en X:
    // 160px recorren el rango entero y el valor se acota sin pasarse.
    let mut editor = factory_editor();

    editor.begin_setting_drag(SettingParam::Yaw, 1000.0);
    // Rango -30..30 (60 de recorrido): 160px a la derecha = +60, acotado a 30.
    assert!((editor.drag_setting_to(1160.0, false) - 30.0).abs() < 1e-4);
    assert!((editor.render_settings.yaw_deg - 30.0).abs() < 1e-4);

    editor.begin_setting_drag(SettingParam::Yaw, 1000.0);
    // Hacia la izquierda baja: medio recorrido son -30 desde el agarre en 30.
    assert!((editor.drag_setting_to(920.0, false) - 0.0).abs() < 1e-4);

    // Con Shift va a un cuarto de velocidad (mismo factor que el knob).
    editor.begin_setting_drag(SettingParam::Glow, 1000.0);
    // Rango 0.3..2.0 (1.7 de recorrido): 160px con fine = +0.425 sobre 1.0.
    assert!((editor.drag_setting_to(1160.0, true) - 1.425).abs() < 1e-4);
}

#[test]
fn setting_drag_ends_with_mouse_up_like_the_knob() {
    // El `mouse up` del módulo termina los dos gestos: si el de settings
    // quedara vivo, el próximo movimiento del mouse movería un slider solo.
    let mut editor = factory_editor();
    editor.begin_setting_drag(SettingParam::Pitch, 500.0);
    assert!(editor.setting_drag_value().is_some());
    editor.end_drag();
    assert!(editor.setting_drag_value().is_none());
    // Sin arrastre, el valor no se mueve.
    assert!((editor.drag_setting_to(900.0, false) - 0.0).abs() < 1e-4);
}
