// crates/hikaru_sequencer/src/matrix.rs

use crate::clip::{Clip, ClipState, VoiceState};

pub const MAX_TRACKS: usize = 16;
pub const MAX_SCENES: usize = 32;

pub struct TrackMatrix {
    // Usamos una matriz fija de slots para evitar alocaciones en runtime.
    // Option::None significa slot vacío.
    pub slots: [[Option<Clip>; MAX_SCENES]; MAX_TRACKS],
}

impl TrackMatrix {
    pub fn new() -> Self {
        // Inicialización de matriz vacía
        const EMPTY_SCENE: [Option<Clip>; MAX_SCENES] = [const { None }; MAX_SCENES];
        Self {
            slots: [EMPTY_SCENE; MAX_TRACKS],
        }
    }

    /// Dispara todos los clips de una escena (fila) respetando la cuantización
    pub fn trigger_scene(&self, scene_index: usize) {
        if scene_index >= MAX_SCENES { return; }
        
        for track in 0..MAX_TRACKS {
            if let Some(clip) = &self.slots[track][scene_index] {
                // Acá le mandamos el comando de Queued. 
                // El motor de audio se encargará de pasarlo a Playing usando el Quantizer.
                clip.set_state(crate::clip::ClipState::Queued);
            }
        }
    }

    /// Polling de la máquina de estados de las voces (llamar cada bloque o
    /// cada avance del transporte).
    ///
    /// Cambio de estado REAL de la Session Matrix:
    /// - Para cada slot en `Playing`, evalúa
    ///   `clip.poll_voice(global_pos, global_loop)`.
    /// - Al recibir `VoiceState::Finished` (sin loop y
    ///   `current_frame >= total_frames`, o fuera de ventana), el slot pasa
    ///   a `Stopped` de inmediato para desactivar el trigger de esa escena:
    ///   el engine deja de pedir datos de esa voz.
    /// - Los slots con loop válido siguen en `Playing` (voice `Active`).
    ///
    /// Retorna cuántos slots se desactivaron (`Playing` → `Stopped`) en
    /// esta llamada.
    pub fn poll_finished_voices(
        &self,
        global_pos: u64,
        global_loop: Option<(u64, u64)>,
    ) -> usize {
        let mut deactivated = 0;
        for track in 0..MAX_TRACKS {
            for scene in 0..MAX_SCENES {
                if let Some(clip) = &self.slots[track][scene] {
                    if clip.get_state() == ClipState::Playing
                        && clip.poll_voice(global_pos, global_loop) == VoiceState::Finished
                    {
                        deactivated += 1;
                    }
                }
            }
        }
        deactivated
    }

    /// Alias explícito del apagado por fin de muestra: mismo `poll` pero con
    /// nombre orientado al render callback (`VoiceStatus::Finished` → slot
    /// inactivo).
    pub fn deactivate_finished_slots(
        &self,
        global_pos: u64,
        global_loop: Option<(u64, u64)>,
    ) -> usize {
        self.poll_finished_voices(global_pos, global_loop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finished_voice_deactivates_session_slot() {
        // Escenario del reporte: clip de 1000 frames disparado en escena 0,
        // playhead supera el compás 5 (pos 1000+) sin loop → slot a Stopped.
        let mut matrix = TrackMatrix::new();
        let mut clip = Clip::new(1, 7);
        clip.start_sample = 0;
        clip.set_source_length_frames(1_000);
        clip.set_state(ClipState::Playing);
        matrix.slots[0][0] = Some(clip);

        // Dentro del clip: sigue Playing, 0 desactivados.
        assert_eq!(matrix.poll_finished_voices(999, None), 0);
        assert_eq!(
            matrix.slots[0][0].as_ref().unwrap().get_state(),
            ClipState::Playing
        );
        // Past-end: Finished → Stopped, 1 desactivado.
        assert_eq!(matrix.poll_finished_voices(1_000, None), 1);
        assert_eq!(
            matrix.slots[0][0].as_ref().unwrap().get_state(),
            ClipState::Stopped
        );
        // Con Time Selection (compases 5-8) y sin sample: también inactivo.
        matrix.slots[0][0].as_ref().unwrap().set_state(ClipState::Playing);
        assert_eq!(matrix.poll_finished_voices(5_000, Some((4_000, 7_000))), 1);
        assert_eq!(
            matrix.slots[0][0].as_ref().unwrap().get_state(),
            ClipState::Stopped
        );
    }

    #[test]
    fn looped_clip_keeps_slot_playing() {
        let mut matrix = TrackMatrix::new();
        let mut clip = Clip::new(2, 7);
        clip.start_sample = 0;
        clip.set_source_length_frames(1_000);
        clip.loop_enabled = true;
        clip.set_loop_points(200, 800);
        clip.set_state(ClipState::Playing);
        matrix.slots[0][0] = Some(clip);
        assert_eq!(matrix.poll_finished_voices(5_000, None), 0);
        assert_eq!(
            matrix.slots[0][0].as_ref().unwrap().get_state(),
            ClipState::Playing
        );
    }
}