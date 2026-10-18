// crates/hikaru_gui/src/views/util.rs

//! Aritmética compartida entre los editores.

/// Normaliza `value` al rango 0..1 según `[min, max]`, acotando.
///
/// Un rango invertido o de ancho cero devuelve 0 en vez de `NaN`: el `NaN` es
/// un valor silenciosamente peor que un 0, porque pasa los `clamp` (todas las
/// comparaciones con `NaN` son falsas) y termina mandando una aguja fuera del
/// dial o una coordenada imposible al canvas.
pub fn normalized(value: f32, min: f32, max: f32) -> f32 {
    if !value.is_finite() || (max - min).abs() <= f32::EPSILON {
        return 0.0;
    }
    ((value - min) / (max - min)).clamp(0.0, 1.0)
}
