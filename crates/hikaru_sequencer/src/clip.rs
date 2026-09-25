// crates/hikaru_sequencer/src/clip.rs

use std::sync::atomic::{AtomicU8, Ordering};

/// Estados posibles de un clip en la matriz.
/// Representados como u8 para manipulación atómica rápida.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClipState {
    Stopped = 0,
    Queued = 1,   // Esperando al próximo pulso de cuantización
    Playing = 2,
    Stopping = 3, // Seguirá sonando hasta el final del ciclo de cuantización
}

impl From<u8> for ClipState {
    fn from(value: u8) -> Self {
        match value {
            1 => ClipState::Queued,
            2 => ClipState::Playing,
            3 => ClipState::Stopping,
            _ => ClipState::Stopped,
        }
    }
}

/// Estado runtime de la voz que lee el buffer del clip.
/// Categórico: sin loop de clip activo y pasado `total_frames`, la voz
/// está `Finished` (dormida) y solo puede emitir silencio absoluto (0.0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceState {
    Active,
    Finished,
}

/// Alias pedido por la Session Matrix: `VoiceStatus::Finished` == voz
/// terminada (slot inactivo, el engine deja de pedir datos).
/// Se mantiene `VoiceState` como nombre canónico; ambos son el mismo tipo.
pub type VoiceStatus = VoiceState;

/// Modos de disparo del clip (Trigger Modes)
#[derive(Debug, Clone, Copy)]
pub enum TriggerMode {
    Trigger, // Se dispara y sigue hasta el final o hasta que se frene
    Toggle,  // Un clic arranca, otro clic frena (respetando cuantización)
    Repeat,  // Mientras se mantenga apretado (Gate)
    Legato,  // Cambia entre clips manteniendo la posición de transporte
}

pub struct Clip {
    pub id: u32,
    pub audio_buffer_id: u32, // Referencia al buffer precargado en hikaru_core
    pub state: AtomicU8,
    pub trigger_mode: TriggerMode,
    pub start_sample: u64,    // Cuándo empezó a sonar realmente
    pub loop_enabled: bool,
    /// Punto de loop inicial del clip individual (en samples).
    /// Independiente del transporte global.
    pub loop_start: u64,
    /// Punto de loop final del clip individual (en samples).
    /// Si `loop_end <= loop_start`, el clip loopea completo.
    pub loop_end: u64,
    /// Longitud real del audio fuente en samples (frames mono).
    /// DEBE fijarse desde la duración real del archivo decodificado
    /// (samples / canales); NUNCA hardcodearse a 1 compás
    /// (ej. `1 * ppqn * 4` ticks). 0 = desconocida (sin loop por longitud).
    pub total_frames: u64,
}

impl Clip {
    pub fn new(id: u32, audio_buffer_id: u32) -> Self {
        Self {
            id,
            audio_buffer_id,
            state: AtomicU8::new(ClipState::Stopped as u8),
            trigger_mode: TriggerMode::Trigger,
            start_sample: 0,
            // NOTA: `loop_enabled` arranca en true por defecto para que el
            // botón 🔁 Loop del Clip Editor ya venga activado. Con región
            // (0,0) `has_valid_clip_loop()` sigue siendo false (one-shot)
            // hasta que se fije una región válida (loop completo) vía
            // `set_clip_loop` / `set_loop_points` explícito en el engine.
            loop_enabled: true,
            loop_start: 0,
            loop_end: 0,
            total_frames: 0,
        }
    }

    /// Fija la longitud real del audio fuente (frames = samples / canales).
    /// Llamar al decodificar/cargar el sample en el engine. Clampea además
    /// una región de loop previa que exceda la longitud real.
    pub fn set_source_length_frames(&mut self, frames: u64) {
        self.total_frames = frames;
        if self.has_valid_clip_loop() && self.loop_end > frames {
            self.loop_end = frames;
            if self.loop_end <= self.loop_start {
                self.loop_start = 0;
                self.loop_end = 0;
            }
        }
    }

    /// Longitud efectiva de reproducción del clip en samples:
    /// región de loop individual si es válida, si no la duración real del
    /// audio. Nunca un valor hardcodeado en compases: si se desconoce la
    /// duración real (`total_frames == 0`) devuelve 0.
    pub fn loop_length_frames(&self) -> u64 {
        if self.has_valid_clip_loop() {
            self.loop_end - self.loop_start
        } else {
            self.total_frames
        }
    }

    /// Establece los puntos de loop del clip individual garantizando
    /// una ventana válida mínima (si `end <= start`, loopea completo).
    pub fn set_loop_points(&mut self, start: u64, end: u64) {
        if end <= start {
            self.loop_start = 0;
            self.loop_end = 0;
        } else {
            self.loop_start = start;
            self.loop_end = end;
        }
    }

    /// Indica si el clip tiene una región de loop individual válida.
    pub fn has_valid_clip_loop(&self) -> bool {
        self.loop_enabled && self.loop_end > self.loop_start
    }

    /// Fin efectivo del loop individual clampeado a la longitud real.
    /// Si `total_frames == 0` (desconocida) no se puede clampear: se usa
    /// `loop_end` tal cual.
    pub fn effective_loop_end(&self) -> u64 {
        if self.total_frames > 0 {
            self.loop_end.min(self.total_frames)
        } else {
            self.loop_end
        }
    }

    /// Resuelve el frame del buffer fuente a leer para una posición global
    /// del transporte (en samples, ya con wrap del loop global aplicado).
    ///
    /// - Si `global_pos < start_sample` → `None` (aún no emitió).
    /// - Sin loop explícito válido → gating estricto por `total_frames`:
    ///   si `elapsed >= total_frames` (o longitud desconocida `== 0`)
    ///   retorna `None` (mutear con 0.0) en lugar de hacer free-run
    ///   (`elapsed % len`).
    /// - Con loop explícito → wraparound sincronizado al Time Selection
    ///   global: si `global_loop` es `Some((start, end))` válido, la fase
    ///   se ancla a `global_loop_start`
    ///   (`loop_start + ((pos - g_start) % loop_len)`) para que el
    ///   wraparound coincida con el wrap del transporte en lugar de
    ///   derivar libremente de `start_sample`.
    pub fn resolve_voice_frame(
        &self,
        global_pos: u64,
        global_loop: Option<(u64, u64)>,
    ) -> Option<u64> {
        if global_pos < self.start_sample {
            return None;
        }
        let elapsed = global_pos - self.start_sample;

        if self.has_valid_clip_loop() {
            let loop_start = self.loop_start.min(self.effective_loop_end());
            let loop_end = self.effective_loop_end();
            if loop_end <= loop_start {
                return None;
            }
            let loop_len = loop_end - loop_start;
            if elapsed < loop_end {
                return Some(elapsed);
            }
            // Loop explícito: sincronizar wraparound con el Time Selection
            // global cuando hay región válida.
            if let Some((g_start, g_end)) = global_loop {
                if g_end > g_start {
                    let since_global = global_pos.saturating_sub(g_start);
                    return Some(loop_start + (since_global % loop_len));
                }
            }
            return Some(loop_start + ((elapsed - loop_start) % loop_len));
        }

        // Sin loop explícito: gating estricto, sin free-run.
        // CATEGÓRICO: pasado `total_frames` → None (el caller debe emitir
        // 0.0 y dormir la voz como `VoiceState::Finished`, jamás `%`).
        if self.total_frames == 0 {
            return None;
        }
        if elapsed >= self.total_frames {
            return None;
        }
        Some(elapsed)
    }

    /// Estado runtime de la voz para una posición global del transporte.
    ///
    /// - Sin loop de clip activo y `elapsed >= total_frames` (o longitud
    ///   desconocida) → `VoiceState::Finished` (dormida, solo 0.0).
    /// - En cualquier otro caso fuera de ventana (`None` de
    ///   `resolve_voice_frame`) → `Finished` también: el área sin datos
    ///   de audio es silencio, nunca free-run ni módulo.
    /// - Con frame válido → `VoiceState::Active`.
    ///
    /// Pura (no muta `ClipState`): el apagado del slot lo hace
    /// `poll_voice` / `next_frame` / `read_sample` al observar `Finished`.
    pub fn voice_state(
        &self,
        global_pos: u64,
        global_loop: Option<(u64, u64)>,
    ) -> VoiceState {
        match self.resolve_voice_frame(global_pos, global_loop) {
            Some(_) => VoiceState::Active,
            None => VoiceState::Finished,
        }
    }

    /// Si la voz está `Finished` y el slot seguía en `Playing`, lo pasa a
    /// `Stopped` de inmediato. Idempotente: si ya estaba `Stopped`/`Queued`
    /// no toca nada. Retorna `true` si desactivó el slot en esta llamada.
    fn finish_slot_if_playing(&self) -> bool {
        if self.get_state() == ClipState::Playing {
            self.set_state(ClipState::Stopped);
            true
        } else {
            false
        }
    }

    /// Polling de la máquina de estados de la voz para el transporte global.
    ///
    /// Cambio de estado REAL (no parche visual):
    /// - Calcula `voice_state(global_pos, global_loop)`.
    /// - Si es `Finished` (sin loop y `elapsed >= total_frames`, o fuera
    ///   de ventana) y el clip estaba en `Playing`, lo pasa a `Stopped`
    ///   inmediatamente para desactivar el trigger de esa escena: el engine
    ///   deja de pedir datos de esa voz.
    /// - Retorna el `VoiceState` resultante (`Finished` past-end sin loop,
    ///   jamás `Active` por defecto).
    pub fn poll_voice(
        &self,
        global_pos: u64,
        global_loop: Option<(u64, u64)>,
    ) -> VoiceState {
        let vs = self.voice_state(global_pos, global_loop);
        if vs == VoiceState::Finished {
            self.finish_slot_if_playing();
        }
        vs
    }

    /// Lectura categórica del buffer del clip (estéreo).
    ///
    /// Exigencia #1 del render callback:
    /// si `current_sample_index >= total_frames` y NO hay loop de clip
    /// activo, retorna `(0.0, 0.0, VoiceState::Finished)` — silencio
    /// absoluto en todos los canales — en lugar de avanzar o hacer
    /// módulo sobre el buffer. Nunca lee fuera de rango: cualquier OOB
    /// es 0.0.
    pub fn read_sample(
        &self,
        current_sample_index: u64,
        interleaved: &[f32],
        channels: usize,
    ) -> (f32, f32, VoiceState) {
        let ch = channels.max(1);
        // Corte categórico: sin loop válido, pasado el final → silencio.
        // Cambio de estado real: además de devolver `Finished`, el slot
        // sale de `Playing` a `Stopped` para que el engine deje de pedir
        // datos de esta voz.
        if !self.has_valid_clip_loop() {
            if self.total_frames == 0 || current_sample_index >= self.total_frames {
                self.finish_slot_if_playing();
                return (0.0, 0.0, VoiceState::Finished);
            }
        }
        let idx = current_sample_index as usize * ch;
        if idx >= interleaved.len() {
            self.finish_slot_if_playing();
            return (0.0, 0.0, VoiceState::Finished);
        }
        let l = interleaved[idx];
        let r = if ch > 1 {
            *interleaved.get(idx + 1).unwrap_or(&l)
        } else {
            l
        };
        // Chequeo final: NaN/infinitos o basura nunca pasan al mixer.
        if !l.is_finite() || !r.is_finite() {
            self.finish_slot_if_playing();
            return (0.0, 0.0, VoiceState::Finished);
        }
        (l, r, VoiceState::Active)
    }

    /// Avanza un frame del clip y retorna `(l, r, estado)`.
    /// Wrapper de `read_sample` sobre la posición global del transporte:
    /// resuelve el frame con `resolve_voice_frame` y, si es `None`
    /// (transporte en área sin datos), rellena con silencio absoluto
    /// (0.0) y marca `Finished` **apagando el slot** (`Playing` → `Stopped`).
    /// Jamás hace `% total_frames`.
    pub fn next_frame(
        &self,
        global_pos: u64,
        global_loop: Option<(u64, u64)>,
        interleaved: &[f32],
        channels: usize,
    ) -> (f32, f32, VoiceState) {
        match self.resolve_voice_frame(global_pos, global_loop) {
            Some(frame) => self.read_sample(frame, interleaved, channels),
            None => {
                self.finish_slot_if_playing();
                (0.0, 0.0, VoiceState::Finished)
            }
        }
    }

    pub fn get_state(&self) -> ClipState {
        ClipState::from(self.state.load(Ordering::Relaxed))
    }

    pub fn set_state(&self, new_state: ClipState) {
        self.state.store(new_state as u8, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_length_is_dynamic_not_one_bar() {
        let mut clip = Clip::new(1, 7);
        // Sin duración real conocida: 0, nunca 1 compás hardcodeado.
        assert_eq!(clip.loop_length_frames(), 0);
        // Longitud real del audio decodificado (ej. 3.2s @ 48kHz mono).
        clip.set_source_length_frames(153_600);
        assert_eq!(clip.loop_length_frames(), 153_600);
        // Con loop individual válido + activación explícita: la región manda.
        clip.loop_enabled = true;
        clip.set_loop_points(10_000, 100_000);
        assert_eq!(clip.loop_length_frames(), 90_000);
    }

    #[test]
    fn loop_clamped_to_real_length() {
        let mut clip = Clip::new(2, 7);
        clip.set_source_length_frames(50_000);
        clip.loop_enabled = true;
        clip.set_loop_points(10_000, 90_000);
        // El loop excede lo real → se clampa al fijar la duración.
        clip.set_source_length_frames(50_000);
        assert!(clip.loop_end <= 50_000);
    }

    #[test]
    fn no_clip_loop_means_silence_past_source_length() {
        // Exigencia #1: sin loop explícito, pasado total_frames → None (0.0).
        let mut clip = Clip::new(10, 7);
        assert!(!clip.has_valid_clip_loop());
        clip.start_sample = 0;
        clip.set_source_length_frames(1_000);
        assert_eq!(clip.resolve_voice_frame(0, None), Some(0));
        assert_eq!(clip.resolve_voice_frame(999, None), Some(999));
        assert_eq!(clip.resolve_voice_frame(1_000, None), None);
        assert_eq!(clip.resolve_voice_frame(5_000, None), None);
        // Con Time Selection global activo (compases 5-8) y sin sample:
        // la voz también debe mutear, nunca `elapsed % len`.
        assert_eq!(
            clip.resolve_voice_frame(5_000, Some((4_000, 7_000))),
            None
        );
    }

    #[test]
    fn unknown_length_never_free_runs() {
        // total_frames == 0 (desconocida): jamás free-run, siempre None.
        let mut clip = Clip::new(11, 7);
        clip.start_sample = 0;
        assert_eq!(clip.resolve_voice_frame(0, None), None);
        assert_eq!(clip.resolve_voice_frame(99_999, None), None);
    }

    #[test]
    fn before_start_is_silence() {
        let mut clip = Clip::new(12, 7);
        clip.start_sample = 500;
        clip.set_source_length_frames(1_000);
        assert_eq!(clip.resolve_voice_frame(499, None), None);
        assert_eq!(clip.resolve_voice_frame(500, None), Some(0));
    }

    #[test]
    fn explicit_clip_loop_wraps_inside_region() {
        // Exigencia #2: CON loop explícito sí se repite (dentro de su región).
        let mut clip = Clip::new(13, 7);
        clip.start_sample = 0;
        clip.set_source_length_frames(1_000);
        clip.loop_enabled = true;
        clip.set_loop_points(200, 800);
        assert!(clip.has_valid_clip_loop());
        // Dentro del primer pase: directo.
        assert_eq!(clip.resolve_voice_frame(100, None), Some(100));
        // Pasado loop_end: wraparound dentro de [200, 800).
        let wrapped = clip.resolve_voice_frame(900, None).unwrap();
        assert!((200..800).contains(&wrapped));
        // Anclado al Time Selection global cuando hay región válida.
        let synced = clip.resolve_voice_frame(900, Some((0, 10_000))).unwrap();
        assert!((200..800).contains(&synced));
    }

    #[test]
    fn past_end_without_loop_returns_finished_and_stops_slot() {
        // Bug compás 5: sin loop, `current_frame >= total_frames` → Finished
        // y el slot sale de Playing a Stopped de inmediato.
        let mut clip = Clip::new(20, 7);
        clip.start_sample = 0;
        clip.set_source_length_frames(1_000);
        clip.set_state(ClipState::Playing);
        // Dentro del clip: Active, sigue Playing.
        assert_eq!(clip.poll_voice(999, None), VoiceState::Active);
        assert_eq!(clip.get_state(), ClipState::Playing);
        // Past-end: Finished + Stopped (alias VoiceStatus incluido).
        let vs: VoiceStatus = clip.poll_voice(1_000, None);
        assert_eq!(vs, VoiceStatus::Finished);
        assert_eq!(clip.get_state(), ClipState::Stopped);
        // Con Time Selection global (compases 5-8) y sin sample: igual.
        clip.set_state(ClipState::Playing);
        assert_eq!(clip.poll_voice(5_000, Some((4_000, 7_000))), VoiceState::Finished);
        assert_eq!(clip.get_state(), ClipState::Stopped);
    }

    #[test]
    fn read_sample_past_end_deactivates_playing_slot() {
        let mut clip = Clip::new(21, 7);
        clip.set_source_length_frames(4);
        clip.set_state(ClipState::Playing);
        let buf = vec![0.5f32; 4 * 2]; // estéreo, 4 frames
        let (_, _, vs) = clip.read_sample(3, &buf, 2);
        assert_eq!(vs, VoiceState::Active);
        assert_eq!(clip.get_state(), ClipState::Playing);
        let (l, r, vs) = clip.read_sample(4, &buf, 2);
        assert_eq!((l, r), (0.0, 0.0));
        assert_eq!(vs, VoiceState::Finished);
        assert_eq!(clip.get_state(), ClipState::Stopped);
    }

    #[test]
    fn looped_voice_never_stops_slot() {
        let mut clip = Clip::new(22, 7);
        clip.start_sample = 0;
        clip.set_source_length_frames(1_000);
        clip.loop_enabled = true;
        clip.set_loop_points(200, 800);
        clip.set_state(ClipState::Playing);
        assert_eq!(clip.poll_voice(5_000, None), VoiceState::Active);
        assert_eq!(clip.get_state(), ClipState::Playing);
    }
}