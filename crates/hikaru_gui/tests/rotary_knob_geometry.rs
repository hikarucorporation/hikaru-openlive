// crates/hikaru_gui/tests/rotary_knob_geometry.rs

use hikaru_gui::views::dsp_rack::{rotary_knob_angle_deg, rotary_knob_point};

#[test]
fn test_rotary_knob_angle_calibration() {
    let eps = 1e-5;
    // 0.0 (mínimo): -135.0° (inferior izquierda, posición 7:30)
    assert!((rotary_knob_angle_deg(0.0) - (-135.0)).abs() < eps);
    // 0.5 (centro / neutro): 0.0° (arriba / 12 en punto exacto)
    assert!((rotary_knob_angle_deg(0.5) - 0.0).abs() < eps);
    // 1.0 (máximo): +135.0° (inferior derecha, posición 4:30)
    assert!((rotary_knob_angle_deg(1.0) - 135.0).abs() < eps);

    // Amplitud de giro limpia de 270°
    let sweep = rotary_knob_angle_deg(1.0) - rotary_knob_angle_deg(0.0);
    assert!((sweep - 270.0).abs() < eps);

    // Valores fuera de rango o NaN se acotan correctamente
    assert!((rotary_knob_angle_deg(-0.5) - (-135.0)).abs() < eps);
    assert!((rotary_knob_angle_deg(1.5) - 135.0).abs() < eps);
    assert!((rotary_knob_angle_deg(f32::NAN) - (-135.0)).abs() < eps);
}

#[test]
fn test_rotary_knob_screen_coordinates() {
    let cx = 10.0;
    let cy = 10.0;
    let r = 6.0;
    let eps = 1e-4;

    // Mínimo (0.0): inferior izquierda (x < cx, y > cy)
    let (x_min, y_min) = rotary_knob_point(cx, cy, r, rotary_knob_angle_deg(0.0));
    assert!(x_min < cx, "x en 0% debe estar a la izquierda");
    assert!(y_min > cy, "y en 0% debe estar abajo (Y de pantalla crece hacia abajo)");
    assert!((x_min - (cx - r * 0.7071068)).abs() < eps);
    assert!((y_min - (cy + r * 0.7071068)).abs() < eps);

    // Centro (0.5): arriba centrado exacto (x == cx, y < cy)
    let (x_mid, y_mid) = rotary_knob_point(cx, cy, r, rotary_knob_angle_deg(0.5));
    assert!((x_mid - cx).abs() < eps, "x en centro debe coincidir exactamente con cx");
    assert!((y_mid - (cy - r)).abs() < eps, "y en centro debe apuntar recto hacia arriba");

    // Máximo (1.0): inferior derecha (x > cx, y > cy)
    let (x_max, y_max) = rotary_knob_point(cx, cy, r, rotary_knob_angle_deg(1.0));
    assert!(x_max > cx, "x en 100% debe estar a la derecha");
    assert!(y_max > cy, "y en 100% debe estar abajo");
    assert!((x_max - (cx + r * 0.7071068)).abs() < eps);
    assert!((y_max - (cy + r * 0.7071068)).abs() < eps);
}
