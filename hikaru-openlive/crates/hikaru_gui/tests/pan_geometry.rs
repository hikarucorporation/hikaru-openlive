// crates/hikaru_gui/tests/pan_geometry.rs
//
// Geometría pura del knob de pan (sin harness de ventana): verifica los
// anchors del spec y que en el centro la aguja cae bit-exacta sobre el eje.

use std::f32::consts::PI;

use hikaru_gui::views::matrix::{pan_needle_tip, pan_standard_angle, pan_sweep_angle, snap_center_pan};

const EPS: f32 = 1e-6;

#[test]
fn standard_anchors_match_the_spec() {
    // L100 → −3/4π, Centro → −π/2, R100 → −π/4.
    assert!((pan_standard_angle(-1.0) - (-3.0 * PI / 4.0)).abs() < EPS);
    assert!((pan_standard_angle(0.0) - (-PI / 2.0)).abs() < EPS);
    assert!((pan_standard_angle(1.0) - (-PI / 4.0)).abs() < EPS);
}

#[test]
fn center_sweep_is_bit_exact_zero() {
    // Sin esto, `sin` daría un residuo y la punta se correría del eje.
    assert_eq!(pan_sweep_angle(0.0), 0.0);
    assert_eq!(pan_sweep_angle(0.0).sin(), 0.0);
    assert_eq!(pan_sweep_angle(0.0).cos(), 1.0);
}

#[test]
fn sweep_spans_the_visual_arc() {
    // Barrido de ±135° desde las 12: extremos simétricos.
    assert!((pan_sweep_angle(-1.0) - (-3.0 * PI / 4.0)).abs() < EPS);
    assert!((pan_sweep_angle(1.0) - (3.0 * PI / 4.0)).abs() < EPS);
}

#[test]
fn center_tip_lands_exactly_on_the_axis() {
    // El caso reportado (12:02 en vez de 12:00): con pan 0 la punta debe
    // coincidir con center_x al bit, sin importar el largo ni el centro.
    for (cx, cy, len) in [(100.0, 50.0, 6.0), (100.5, 50.5, 7.0), (0.0, 0.0, 1.0)] {
        let (tx, ty) = pan_needle_tip(0.0, cx, cy, len);
        assert_eq!(tx, cx, "tip_x debería ser center_x bit-exacto");
        assert_eq!(ty, cy - len);
    }
}

#[test]
fn extreme_tips_are_symmetric() {
    // L100 y R100: mismo largo, espejados en x, misma altura.
    let (lx, ly) = pan_needle_tip(-1.0, 100.0, 100.0, 8.0);
    let (rx, ry) = pan_needle_tip(1.0, 100.0, 100.0, 8.0);
    // Tolerancia de redondeo de `sinf` escalado por el largo.
    assert!((lx + rx - 200.0).abs() < 1e-5);
    assert!((ly - ry).abs() < 1e-5);
    assert!(lx < 100.0 && rx > 100.0);
}

#[test]
fn center_detent_collapses_residue() {
    // Residuos de drag/clic (< ±0.02) se muestran como centro exacto.
    assert_eq!(snap_center_pan(0.0), 0.0);
    assert_eq!(snap_center_pan(0.015), 0.0);
    assert_eq!(snap_center_pan(-0.019), 0.0);
    // Fuera del detent no se toca (ni NaN/infinitos, que van al display 0
    // por el clamp del llamador... acá solo se verifica identidad).
    assert_eq!(snap_center_pan(0.5), 0.5);
    assert_eq!(snap_center_pan(-1.0), -1.0);
    // snap + tip: residuo → vertical perfecto.
    let (tx, _) = pan_needle_tip(snap_center_pan(0.012), 40.0, 40.0, 6.0);
    assert_eq!(tx, 40.0);
}
