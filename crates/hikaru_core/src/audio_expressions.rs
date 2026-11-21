// crates/hikaru_core/src/audio_expressions.rs
//
// Modelo de curvas de expresión por clip de audio (Gain, Pan, Pitch, Formant)
// con paridad funcional y visual respecto a Bitwig Studio.

/// Tipo de curva de expresión editable sobre un clip de audio.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AudioExpressionKind {
    Gain,
    Pan,
    Pitch,
    Formant,
}

impl AudioExpressionKind {
    pub const ALL: [AudioExpressionKind; 4] = [
        AudioExpressionKind::Gain,
        AudioExpressionKind::Pan,
        AudioExpressionKind::Pitch,
        AudioExpressionKind::Formant,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            AudioExpressionKind::Gain => "Gain",
            AudioExpressionKind::Pan => "Pan",
            AudioExpressionKind::Pitch => "Pitch",
            AudioExpressionKind::Formant => "Formant",
        }
    }

    /// Rango `(min, max)` en unidades del parámetro.
    pub fn range(&self) -> (f32, f32) {
        match self {
            AudioExpressionKind::Gain => (-48.0, 12.0),
            AudioExpressionKind::Pan => (-1.0, 1.0),
            AudioExpressionKind::Pitch => (-24.0, 24.0),
            AudioExpressionKind::Formant => (-24.0, 24.0),
        }
    }

    /// Valor neutro de referencia (`0` en el eje vertical).
    pub fn default_value(&self) -> f32 {
        0.0
    }

    /// Etiqueta del valor neutro en el margen izquierdo.
    pub fn zero_label(&self) -> &'static str {
        match self {
            AudioExpressionKind::Gain => "0",
            AudioExpressionKind::Pan => "C",
            AudioExpressionKind::Pitch => "0",
            AudioExpressionKind::Formant => "0",
        }
    }

    pub fn clamp(&self, v: f32) -> f32 {
        let (lo, hi) = self.range();
        if v.is_finite() { v.clamp(lo, hi) } else { self.default_value() }
    }

    /// Formato estilo Bitwig para el tooltip flotante al editar un nodo.
    pub fn format_value(&self, v: f32) -> String {
        let v = self.clamp(v);
        match self {
            AudioExpressionKind::Gain => {
                if v <= -47.9 {
                    "-∞ dB".to_string()
                } else if v.abs() < 0.05 {
                    "0.0 dB".to_string()
                } else {
                    format!("{:+.1} dB", v)
                }
            }
            AudioExpressionKind::Pan => {
                if v.abs() < 0.005 {
                    "C".to_string()
                } else if v < 0.0 {
                    format!("{:.0}% L", -v * 100.0)
                } else {
                    format!("{:.0}% R", v * 100.0)
                }
            }
            AudioExpressionKind::Pitch | AudioExpressionKind::Formant => {
                if v.abs() < 0.005 {
                    "0.00".to_string()
                } else {
                    format!("{:.2}", v)
                }
            }
        }
    }
}

/// Nodo individual en una curva de expresión de clip de audio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExpressionPoint {
    /// Posición normalizada dentro del clip `[0.0, 1.0]`.
    pub norm_pos: f64,
    /// Valor en unidades del parámetro (dB, pan `-1..1`, semitonos).
    pub value: f32,
    /// Tensión del segmento que sale hacia el siguiente punto `[-1.0, 1.0]` (`0.0` = lineal).
    pub curve: f32,
}

impl ExpressionPoint {
    pub fn new(norm_pos: f64, value: f32) -> Self {
        Self {
            norm_pos: if norm_pos.is_finite() { norm_pos.clamp(0.0, 1.0) } else { 0.0 },
            value: if value.is_finite() { value } else { 0.0 },
            curve: 0.0,
        }
    }
}

/// Curva continua por tramos con tensión opcional entre nodos.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExpressionCurve {
    pub points: Vec<ExpressionPoint>,
}

impl ExpressionCurve {
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Devuelve `true` si algún nodo desvía la curva respecto a `default_val`.
    pub fn is_active(&self, default_val: f32) -> bool {
        self.points.iter().any(|p| (p.value - default_val).abs() > 1e-4)
    }

    pub fn sort_points(&mut self) {
        for p in &mut self.points {
            p.norm_pos = if p.norm_pos.is_finite() { p.norm_pos.clamp(0.0, 1.0) } else { 0.0 };
            p.curve = if p.curve.is_finite() { p.curve.clamp(-1.0, 1.0) } else { 0.0 };
        }
        self.points.sort_by(|a, b| {
            a.norm_pos
                .partial_cmp(&b.norm_pos)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    /// Inserta un punto manteniendo el orden por `norm_pos` y devuelve su índice.
    pub fn insert_point(&mut self, norm_pos: f64, value: f32) -> usize {
        let p = ExpressionPoint::new(norm_pos, value);
        let idx = self
            .points
            .iter()
            .position(|existing| existing.norm_pos > p.norm_pos)
            .unwrap_or(self.points.len());
        self.points.insert(idx, p);
        idx
    }

    /// Evalúa la curva en `norm_pos` (`[0.0, 1.0]`).
    #[inline]
    pub fn evaluate(&self, norm_pos: f64, default_val: f32) -> f32 {
        if self.points.is_empty() {
            return default_val;
        }
        let u = if norm_pos.is_finite() { norm_pos.clamp(0.0, 1.0) } else { 0.0 };
        let first = &self.points[0];
        if self.points.len() == 1 || u <= first.norm_pos {
            return first.value;
        }
        let last = &self.points[self.points.len() - 1];
        if u >= last.norm_pos {
            return last.value;
        }

        // Búsqueda binaria del segmento [a, b] tal que a.norm_pos <= u <= b.norm_pos
        let mut lo = 0usize;
        let mut hi = self.points.len() - 1;
        while lo + 1 < hi {
            let mid = (lo + hi) / 2;
            if self.points[mid].norm_pos <= u {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let a = &self.points[lo];
        let b = &self.points[hi];
        let span = b.norm_pos - a.norm_pos;
        if span <= 1e-9 {
            return b.value;
        }
        let t = ((u - a.norm_pos) / span).clamp(0.0, 1.0) as f32;
        let shaped = if a.curve.abs() < 1e-4 {
            t
        } else {
            let exp = 2.0f32.powf(-a.curve.clamp(-1.0, 1.0) * 3.0);
            t.powf(exp)
        };
        a.value + (b.value - a.value) * shaped
    }
}

/// Conjunto completo de las 4 curvas de expresión de un clip de audio.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioClipExpressions {
    pub gain: ExpressionCurve,
    pub pan: ExpressionCurve,
    pub pitch: ExpressionCurve,
    pub formant: ExpressionCurve,
}

impl AudioClipExpressions {
    pub fn curve(&self, kind: AudioExpressionKind) -> &ExpressionCurve {
        match kind {
            AudioExpressionKind::Gain => &self.gain,
            AudioExpressionKind::Pan => &self.pan,
            AudioExpressionKind::Pitch => &self.pitch,
            AudioExpressionKind::Formant => &self.formant,
        }
    }

    pub fn curve_mut(&mut self, kind: AudioExpressionKind) -> &mut ExpressionCurve {
        match kind {
            AudioExpressionKind::Gain => &mut self.gain,
            AudioExpressionKind::Pan => &mut self.pan,
            AudioExpressionKind::Pitch => &mut self.pitch,
            AudioExpressionKind::Formant => &mut self.formant,
        }
    }

    pub fn has_pitch_or_formant(&self) -> bool {
        self.pitch.is_active(0.0) || self.formant.is_active(0.0)
    }

    pub fn has_gain_or_pan(&self) -> bool {
        self.gain.is_active(0.0) || self.pan.is_active(0.0)
    }

    /// Evalúa multiplicadores `(left_mul, right_mul)` en tiempo real para `Gain` y `Pan`.
    /// En neutro (`0 dB`, `pan = 0`) devuelve exactamente `(1.0, 1.0)`.
    #[inline]
    pub fn eval_rt_gain_pan(&self, norm_pos: f64) -> (f32, f32) {
        let g = if self.gain.is_empty() {
            1.0
        } else {
            let db = self.gain.evaluate(norm_pos, 0.0).clamp(-48.0, 12.0);
            if db <= -47.9 {
                0.0
            } else {
                10.0f32.powf(db / 20.0)
            }
        };

        if self.pan.is_empty() {
            return (g, g);
        }

        let pan = self.pan.evaluate(norm_pos, 0.0).clamp(-1.0, 1.0);
        // Balance estéreo suave: centro = (1.0, 1.0), izquierda = (1.0, 0.0), derecha = (0.0, 1.0)
        let l_pan = (1.0 - pan).min(1.0);
        let r_pan = (1.0 + pan).min(1.0);
        (g * l_pan, g * r_pan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluate_linear_and_curved_segments() {
        let mut c = ExpressionCurve::default();
        assert_eq!(c.evaluate(0.5, 0.0), 0.0);
        assert!(!c.is_active(0.0));

        c.insert_point(0.0, 0.0);
        c.insert_point(1.0, 2.0);
        assert!(c.is_active(0.0));
        assert!((c.evaluate(0.5, 0.0) - 1.0).abs() < 1e-5);
        assert!((c.evaluate(0.0, 0.0) - 0.0).abs() < 1e-5);
        assert!((c.evaluate(1.0, 0.0) - 2.0).abs() < 1e-5);

        // Tensión positiva arquea hacia arriba (valor mayor en el punto medio)
        c.points[0].curve = 0.5;
        assert!(c.evaluate(0.5, 0.0) > 1.2);
        // Tensión negativa arquea hacia abajo
        c.points[0].curve = -0.5;
        assert!(c.evaluate(0.5, 0.0) < 0.8);
    }

    #[test]
    fn rt_gain_and_pan_math() {
        let mut expr = AudioClipExpressions::default();
        assert_eq!(expr.eval_rt_gain_pan(0.5), (1.0, 1.0));

        expr.gain.insert_point(0.0, -6.0206);
        let (l, r) = expr.eval_rt_gain_pan(0.5);
        assert!((l - 0.5).abs() < 0.01 && (r - 0.5).abs() < 0.01);

        expr.gain.points[0].value = -48.0;
        assert_eq!(expr.eval_rt_gain_pan(0.5), (0.0, 0.0));

        expr.gain.points.clear();
        expr.pan.insert_point(0.0, -1.0);
        expr.pan.insert_point(1.0, 1.0);
        assert_eq!(expr.eval_rt_gain_pan(0.0), (1.0, 0.0));
        assert_eq!(expr.eval_rt_gain_pan(0.5), (1.0, 1.0));
        assert_eq!(expr.eval_rt_gain_pan(1.0), (0.0, 1.0));
    }
}
