// crates/hikaru_transport/src/tap_tempo.rs

/// Intervalo máximo entre taps consecutivos antes de reiniciar la secuencia
/// (2.5 segundos ≈ 24 BPM).
pub const DEFAULT_TAP_TIMEOUT_SECS: f64 = 2.5;

/// Intervalo mínimo aceptado entre taps para filtrar rebotes accidentales
/// (0.15 segundos = 400 BPM).
pub const DEFAULT_MIN_INTERVAL_SECS: f64 = 0.15;

/// Cantidad máxima de intervalos promediados en la ventana móvil.
pub const DEFAULT_TAP_WINDOW: usize = 8;

/// Rango de BPM válido por omisión (coincide con la GUI: 40..=300 BPM).
pub const DEFAULT_TAP_MIN_BPM: f64 = 40.0;
pub const DEFAULT_TAP_MAX_BPM: f64 = 300.0;

/// Detector determinista de Tap Tempo basado en ventana móvil de intervalos.
///
/// Acepta timestamps explícitos en segundos (`f64`), lo que permite probarlo
/// sin sleeps ni relojes de pared y conectarlo directamente al reloj de egui
/// o a un controlador MIDI.
#[derive(Debug, Clone)]
pub struct TapTempoDetector {
    taps: Vec<f64>,
    max_intervals: usize,
    timeout_secs: f64,
    min_interval_secs: f64,
    min_bpm: f64,
    max_bpm: f64,
    last_bpm: Option<f64>,
}

impl Default for TapTempoDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl TapTempoDetector {
    pub fn new() -> Self {
        Self::with_bpm_range(DEFAULT_TAP_MIN_BPM, DEFAULT_TAP_MAX_BPM)
    }

    pub fn with_bpm_range(min_bpm: f64, max_bpm: f64) -> Self {
        let lo = min_bpm.max(1.0);
        let hi = max_bpm.max(lo);
        Self {
            taps: Vec::with_capacity(DEFAULT_TAP_WINDOW + 1),
            max_intervals: DEFAULT_TAP_WINDOW,
            timeout_secs: DEFAULT_TAP_TIMEOUT_SECS,
            min_interval_secs: DEFAULT_MIN_INTERVAL_SECS,
            min_bpm: lo,
            max_bpm: hi,
            last_bpm: None,
        }
    }

    /// Registra un tap en `timestamp_secs` (reloj monotónico en segundos).
    ///
    /// - Primer tap (o tras superar el timeout de inactividad): inicia nueva
    ///   secuencia y devuelve `None`.
    /// - A partir del segundo tap: promedia los últimos `max_intervals`
    ///   intervalos, clampea a `[min_bpm, max_bpm]` y redondea a centésimas.
    /// - Rebotes más rápidos que `min_interval_secs` se descartan manteniendo
    ///   el último BPM calculado.
    pub fn tap_at(&mut self, timestamp_secs: f64) -> Option<f64> {
        if !timestamp_secs.is_finite() {
            return None;
        }

        if let Some(&last) = self.taps.last() {
            let dt = timestamp_secs - last;
            if dt < 0.0 || dt > self.timeout_secs {
                self.taps.clear();
                self.last_bpm = None;
            } else if dt < self.min_interval_secs {
                return self.last_bpm;
            }
        }

        self.taps.push(timestamp_secs);
        let max_taps = self.max_intervals + 1;
        if self.taps.len() > max_taps {
            let overflow = self.taps.len() - max_taps;
            self.taps.drain(0..overflow);
        }

        if self.taps.len() < 2 {
            return None;
        }

        let intervals = (self.taps.len() - 1) as f64;
        let span = self.taps.last().unwrap() - self.taps.first().unwrap();
        if span <= 0.0 {
            return None;
        }

        let avg_dt = span / intervals;
        let raw_bpm = (60.0 / avg_dt).clamp(self.min_bpm, self.max_bpm);
        let rounded = (raw_bpm * 100.0).round() / 100.0;
        self.last_bpm = Some(rounded);
        Some(rounded)
    }

    /// Reinicia el historial de taps.
    pub fn reset(&mut self) {
        self.taps.clear();
        self.last_bpm = None;
    }

    /// Número de taps en la ventana actual.
    pub fn tap_count(&self) -> usize {
        self.taps.len()
    }

    /// Último BPM calculado en la secuencia actual.
    pub fn last_bpm(&self) -> Option<f64> {
        self.last_bpm
    }

    /// Timestamp del último tap registrado.
    pub fn last_tap_secs(&self) -> Option<f64> {
        self.taps.last().copied()
    }

    /// Indica si hubo un tap reciente dentro de la ventana de timeout.
    pub fn is_active_at(&self, now_secs: f64) -> bool {
        self.taps
            .last()
            .is_some_and(|&last| now_secs >= last && (now_secs - last) <= self.timeout_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_tap_returns_none() {
        let mut det = TapTempoDetector::new();
        assert_eq!(det.tap_at(1.0), None);
        assert_eq!(det.tap_count(), 1);
        assert!(det.is_active_at(1.5));
        assert!(!det.is_active_at(4.0));
    }

    #[test]
    fn steady_taps_compute_exact_bpm() {
        let mut det = TapTempoDetector::new();
        // 120 BPM = 0.5 s por negra
        assert_eq!(det.tap_at(0.0), None);
        assert_eq!(det.tap_at(0.5), Some(120.0));
        assert_eq!(det.tap_at(1.0), Some(120.0));
        assert_eq!(det.tap_at(1.5), Some(120.0));
        assert_eq!(det.last_bpm(), Some(120.0));

        // 140 BPM ≈ 0.4285714 s tras reset
        det.reset();
        let step = 60.0 / 140.0;
        assert_eq!(det.tap_at(10.0), None);
        let bpm = det.tap_at(10.0 + step).unwrap();
        assert!((bpm - 140.0).abs() < 0.02, "expected ~140 BPM, got {bpm}");
    }

    #[test]
    fn timeout_resets_sequence() {
        let mut det = TapTempoDetector::new();
        assert_eq!(det.tap_at(0.0), None);
        assert_eq!(det.tap_at(0.5), Some(120.0));
        // Pausa > 2.5s reinicia
        assert_eq!(det.tap_at(5.0), None);
        assert_eq!(det.tap_count(), 1);
        // Nueva secuencia a 100 BPM (0.6 s)
        assert_eq!(det.tap_at(5.6), Some(100.0));
    }

    #[test]
    fn debounce_and_clamping_work() {
        let mut det = TapTempoDetector::new();
        assert_eq!(det.tap_at(1.0), None);
        // Rebote < 0.15 s descartado
        assert_eq!(det.tap_at(1.05), None);
        assert_eq!(det.tap_count(), 1);
        // Tap muy rápido pero >= min_interval (0.16 s => 375 BPM -> clamp 300)
        assert_eq!(det.tap_at(1.16), Some(300.0));

        // Tap lento dentro del timeout (2.0 s => 30 BPM -> clamp 40)
        det.reset();
        assert_eq!(det.tap_at(0.0), None);
        assert_eq!(det.tap_at(2.0), Some(40.0));
    }

    #[test]
    fn sliding_window_tracks_tempo_changes() {
        let mut det = TapTempoDetector::new();
        let mut t = 0.0;
        // 100 BPM (0.6 s) durante 5 taps
        for _ in 0..5 {
            det.tap_at(t);
            t += 0.6;
        }
        assert_eq!(det.last_bpm(), Some(100.0));

        // Cambiar a 150 BPM (0.4 s) por 9 taps: la ventana de 8 intervalos
        // queda 100% poblada por 150 BPM.
        for _ in 0..9 {
            det.tap_at(t);
            t += 0.4;
        }
        assert_eq!(det.tap_count(), DEFAULT_TAP_WINDOW + 1);
        assert_eq!(det.last_bpm(), Some(150.0));
    }
}
