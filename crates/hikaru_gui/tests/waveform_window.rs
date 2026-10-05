// crates/hikaru_gui/tests/waveform_window.rs
//
// Matemática pura de la ventana móvil del waveform vertical (sin ventana):
// mapea cada tick al pico del clip activo, o a silencio (0.0) fuera de él.

use hikaru_gui::views::arranger_view::waveform_peak_at_tick;
use hikaru_gui::views::arranger_view::waveform_window_tick;

/// Rampa 0..1 en 100 picos: el índice i vale i/100.
fn ramp() -> Vec<f32> {
    (0..100).map(|i| i as f32 / 100.0).collect()
}

#[test]
fn peak_inside_clip_maps_proportionally() {
    let peaks = ramp();
    // tick a mitad del clip (1000 de [0, 2000), total 2000) → pico 50.
    let pv = waveform_peak_at_tick(&peaks, 0, 2000, 0, 2000, 1000);
    assert!((pv - 0.5).abs() < 0.015, "pv={pv}");
    // Un cuarto → pico 25.
    let pv = waveform_peak_at_tick(&peaks, 0, 2000, 0, 2000, 500);
    assert!((pv - 0.25).abs() < 0.015, "pv={pv}");
}

#[test]
fn peak_outside_clip_is_silence() {
    let peaks = ramp();
    // Antes del inicio.
    assert_eq!(waveform_peak_at_tick(&peaks, 1000, 2000, 0, 2000, 999), 0.0);
    // En el fin (exclusivo) y más allá.
    assert_eq!(waveform_peak_at_tick(&peaks, 1000, 2000, 0, 2000, 3000), 0.0);
    assert_eq!(waveform_peak_at_tick(&peaks, 1000, 2000, 0, 2000, 9999), 0.0);
    // Último tick válido sí suena.
    assert!(waveform_peak_at_tick(&peaks, 1000, 2000, 0, 2000, 2999) > 0.0);
}

#[test]
fn peak_respects_sample_offset() {
    let peaks = ramp();
    // Clip que arranca a mitad del sample: el tick inicial mapea al pico 50.
    let pv = waveform_peak_at_tick(&peaks, 0, 1000, 1000, 2000, 0);
    assert!((pv - 0.5).abs() < 0.015, "pv={pv}");
    // Y más allá del sample total es silencio aunque el clip siga.
    assert_eq!(waveform_peak_at_tick(&peaks, 0, 2000, 1000, 2000, 1000), 0.0);
}

#[test]
fn peak_degenerate_inputs_are_silence() {
    // Sin picos, sin duración de sample o duración cero → 0.0, nunca NaN.
    assert_eq!(waveform_peak_at_tick(&[], 0, 2000, 0, 2000, 1000), 0.0);
    assert_eq!(waveform_peak_at_tick(&ramp(), 0, 2000, 0, 0, 1000), 0.0);
    assert_eq!(waveform_peak_at_tick(&ramp(), 0, 0, 0, 2000, 0), 0.0);
    let pv = waveform_peak_at_tick(&ramp(), 0, 2000, 0, 2000, 1000);
    assert!(pv.is_finite() && (0.0..=1.0).contains(&pv));
}

#[test]
fn peak_clamps_above_one() {
    let peaks = vec![2.5; 10];
    assert_eq!(waveform_peak_at_tick(&peaks, 0, 100, 0, 100, 50), 1.0);
}

#[test]
fn window_future_on_top_past_on_bottom() {
    // Ventana de 1000 ticks, 100 filas, playhead en 5000: arriba el futuro
    // (+500), abajo el pasado (-500) y el presente al centro.
    assert_eq!(waveform_window_tick(5000, 1000, 0, 100), 5500);
    assert_eq!(waveform_window_tick(5000, 1000, 100, 100), 4500);
    assert_eq!(waveform_window_tick(5000, 1000, 50, 100), 5000);
}

#[test]
fn window_tick_decreases_downwards_monotonically() {
    // Fila a fila hacia abajo, el tick sólo puede bajar (cascada).
    let rows = 96;
    let mut prev = waveform_window_tick(20000, 7680, 0, rows);
    for row in 1..rows {
        let tick = waveform_window_tick(20000, 7680, row, rows);
        assert!(tick <= prev, "fila {row}: {tick} debería ser <= {prev}");
        prev = tick;
    }
}

#[test]
fn window_event_moves_down_as_playhead_advances() {
    // Un evento fijo en tick 10000 baja de fila cuando el playhead avanza de
    // 9000 a 9200 (el futuro "baja" hacia la línea de presente).
    let rows = 96;
    let window = 7680u64;
    let row_at = |playhead: u64| {
        (0..rows)
            .min_by_key(|&row| {
                waveform_window_tick(playhead, window, row, rows).abs_diff(10000)
            })
            .unwrap()
    };
    let before = row_at(9000);
    let after = row_at(9200);
    assert!(
        after > before,
        "el evento debería bajar (fila {before} -> {after}) al avanzar el playhead"
    );
}

#[test]
fn window_saturates_at_zero_without_underflow() {
    // Playhead antes de media ventana: las filas de arriba saturan en 0.
    assert_eq!(waveform_window_tick(100, 1000, 0, 100), 600);
    assert_eq!(waveform_window_tick(100, 1000, 99, 100), 0);
    // Degenerados: devuelve el playhead sin pánico.
    assert_eq!(waveform_window_tick(5000, 0, 10, 100), 5000);
    assert_eq!(waveform_window_tick(5000, 1000, 10, 0), 5000);
}
