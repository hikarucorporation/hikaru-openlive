// crates/hikaru_sequencer/src/matrix.rs

use hikaru_transport::TransportPosition;

use crate::clip::{Clip, ClipState, VoiceState};
use crate::quantizer::{QuantizationEngine, QuantizationGrid};

pub const MAX_TRACKS: usize = 16;
pub const MAX_SCENES: usize = 32;
pub const DEFAULT_SCENE_COUNT: usize = 8;

pub struct TrackMatrix {
    // Usamos una matriz fija de slots para evitar alocaciones en runtime.
    // Option::None significa slot vacío.
    pub slots: [[Option<Clip>; MAX_SCENES]; MAX_TRACKS],
    /// Cantidad activa de escenas visibles/editables (`1..=MAX_SCENES`).
    pub scene_count: usize,
    /// Cuantización global por defecto para lanzamiento de escenas/clips.
    pub global_quantization: QuantizationGrid,
    /// Override opcional de cuantización por escena (`None` = hereda global).
    pub scene_quantizations: [Option<QuantizationGrid>; MAX_SCENES],
}

impl TrackMatrix {
    pub fn new() -> Self {
        // Inicialización de matriz vacía
        const EMPTY_SCENE: [Option<Clip>; MAX_SCENES] = [const { None }; MAX_SCENES];
        Self {
            slots: [EMPTY_SCENE; MAX_TRACKS],
            scene_count: DEFAULT_SCENE_COUNT,
            global_quantization: QuantizationGrid::Quarter,
            scene_quantizations: [None; MAX_SCENES],
        }
    }

    /// Agrega una escena nueva al final si aún no se alcanzó `MAX_SCENES`.
    pub fn add_scene(&mut self) -> Option<usize> {
        if self.scene_count >= MAX_SCENES {
            return None;
        }
        let idx = self.scene_count;
        self.scene_count += 1;
        self.scene_quantizations[idx] = None;
        for track in 0..MAX_TRACKS {
            self.slots[track][idx] = None;
        }
        Some(idx)
    }

    /// Inserta una escena vacía en `scene_idx`, desplazando las escenas
    /// posteriores hacia la derecha (si hay espacio `< MAX_SCENES`).
    pub fn insert_scene(&mut self, scene_idx: usize) -> Option<usize> {
        if self.scene_count >= MAX_SCENES {
            return None;
        }
        let idx = scene_idx.min(self.scene_count);
        for s in (idx..self.scene_count).rev() {
            self.scene_quantizations[s + 1] = self.scene_quantizations[s];
            for track in 0..MAX_TRACKS {
                self.slots[track][s + 1] = self.slots[track][s].take();
            }
        }
        self.scene_quantizations[idx] = None;
        for track in 0..MAX_TRACKS {
            self.slots[track][idx] = None;
        }
        self.scene_count += 1;
        Some(idx)
    }

    /// Elimina la escena `scene_idx` (manteniendo al menos 1 escena activa),
    /// deteniendo sus clips y desplazando las columnas posteriores a la izquierda.
    pub fn remove_scene(&mut self, scene_idx: usize) -> bool {
        if self.scene_count <= 1 || scene_idx >= self.scene_count {
            return false;
        }
        for track in 0..MAX_TRACKS {
            if let Some(clip) = &self.slots[track][scene_idx] {
                clip.set_state(ClipState::Stopped);
            }
            self.slots[track][scene_idx] = None;
            for s in scene_idx..(self.scene_count - 1) {
                self.slots[track][s] = self.slots[track][s + 1].take();
            }
            self.slots[track][self.scene_count - 1] = None;
        }
        for s in scene_idx..(self.scene_count - 1) {
            self.scene_quantizations[s] = self.scene_quantizations[s + 1];
        }
        self.scene_quantizations[self.scene_count - 1] = None;
        self.scene_count -= 1;
        true
    }

    /// Configura la cuantización global de lanzamiento.
    pub fn set_global_quantization(&mut self, grid: QuantizationGrid) {
        self.global_quantization = grid;
    }

    /// Configura o limpia el override de cuantización de una escena.
    pub fn set_scene_quantization(&mut self, scene_idx: usize, grid: Option<QuantizationGrid>) {
        if scene_idx < MAX_SCENES {
            self.scene_quantizations[scene_idx] = grid;
        }
    }

    /// Cuantización efectiva de una escena (override si existe, si no global).
    pub fn effective_scene_quantization(&self, scene_idx: usize) -> QuantizationGrid {
        let override_grid = self.scene_quantizations.get(scene_idx).copied().flatten();
        QuantizationEngine::resolve_scene_grid(self.global_quantization, override_grid)
    }

    /// Samples hasta el próximo borde de lanzamiento para la escena indicada.
    pub fn samples_until_scene_launch(
        &self,
        scene_idx: usize,
        current_pos: &TransportPosition,
        sample_rate: f64,
    ) -> u64 {
        let override_grid = self.scene_quantizations.get(scene_idx).copied().flatten();
        QuantizationEngine::samples_until_scene_sync(
            current_pos,
            self.global_quantization,
            override_grid,
            sample_rate,
        )
    }

    /// Busca el `start_sample` de referencia para disparo Legato: primero el
    /// clip en reproducción en `preferred_track`, y si no hay, el más antiguo
    /// en cualquier pista.
    pub fn legato_reference_start(&self, preferred_track: Option<usize>) -> Option<u64> {
        if let Some(track) = preferred_track {
            if track < MAX_TRACKS {
                if let Some(start) = self.slots[track]
                    .iter()
                    .flatten()
                    .filter(|c| c.get_state() == ClipState::Playing)
                    .map(|c| c.start_sample)
                    .min()
                {
                    return Some(start);
                }
            }
        }
        self.slots
            .iter()
            .flat_map(|row| row.iter())
            .flatten()
            .filter(|c| c.get_state() == ClipState::Playing)
            .map(|c| c.start_sample)
            .min()
    }

    /// Dispara un clip individual respetando su modo Legato y deteniendo el
    /// resto de los slots de esa misma pista.
    pub fn trigger_clip(&mut self, track_idx: usize, scene_idx: usize, now_sample: u64) -> bool {
        if track_idx >= MAX_TRACKS || scene_idx >= MAX_SCENES {
            return false;
        }
        if self.slots[track_idx][scene_idx].is_none() {
            return false;
        }
        let legato_ref = self.legato_reference_start(Some(track_idx));
        for s in 0..MAX_SCENES {
            if s != scene_idx {
                if let Some(other) = &self.slots[track_idx][s] {
                    if other.get_state() == ClipState::Playing {
                        other.set_state(ClipState::Stopped);
                    }
                }
            }
        }
        if let Some(clip) = &mut self.slots[track_idx][scene_idx] {
            clip.launch(now_sample, legato_ref);
            true
        } else {
            false
        }
    }

    /// Dispara inmediatamente una escena completa respetando el flag Legato
    /// individual de cada clip de esa columna.
    pub fn trigger_scene_now(&mut self, scene_idx: usize, now_sample: u64) {
        if scene_idx >= MAX_SCENES {
            return;
        }
        let mut refs = [None; MAX_TRACKS];
        for track in 0..MAX_TRACKS {
            if self.slots[track][scene_idx].is_some() {
                refs[track] = self.legato_reference_start(Some(track));
            }
        }
        for track in 0..MAX_TRACKS {
            if self.slots[track][scene_idx].is_some() {
                for s in 0..MAX_SCENES {
                    if s != scene_idx {
                        if let Some(other) = &self.slots[track][s] {
                            if other.get_state() == ClipState::Playing {
                                other.set_state(ClipState::Stopped);
                            }
                        }
                    }
                }
                if let Some(clip) = &mut self.slots[track][scene_idx] {
                    clip.launch(now_sample, refs[track]);
                }
            }
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

    #[test]
    fn add_insert_remove_scenes_shifts_clips_and_quantization_overrides() {
        let mut matrix = TrackMatrix::new();
        assert_eq!(matrix.scene_count, DEFAULT_SCENE_COUNT);

        // Colocar clip id=10 en escena 1 y override 1/16 en escena 1
        matrix.slots[0][1] = Some(Clip::new(10, 1));
        matrix.set_scene_quantization(1, Some(QuantizationGrid::Sixteenth));

        // Insertar escena en índice 1 -> lo que estaba en 1 pasa a 2
        assert_eq!(matrix.insert_scene(1), Some(1));
        assert_eq!(matrix.scene_count, DEFAULT_SCENE_COUNT + 1);
        assert!(matrix.slots[0][1].is_none());
        assert_eq!(matrix.slots[0][2].as_ref().unwrap().id, 10);
        assert_eq!(matrix.scene_quantizations[1], None);
        assert_eq!(matrix.scene_quantizations[2], Some(QuantizationGrid::Sixteenth));

        // Eliminar escena 1 -> lo que estaba en 2 vuelve a 1
        assert!(matrix.remove_scene(1));
        assert_eq!(matrix.scene_count, DEFAULT_SCENE_COUNT);
        assert_eq!(matrix.slots[0][1].as_ref().unwrap().id, 10);
        assert_eq!(matrix.scene_quantizations[1], Some(QuantizationGrid::Sixteenth));

        // No puede bajar de 1 escena
        while matrix.scene_count > 1 {
            assert!(matrix.remove_scene(0));
        }
        assert!(!matrix.remove_scene(0));
        assert_eq!(matrix.scene_count, 1);

        // add_scene hasta MAX_SCENES y límite duro
        while matrix.scene_count < MAX_SCENES {
            assert!(matrix.add_scene().is_some());
        }
        assert_eq!(matrix.add_scene(), None);
        assert_eq!(matrix.insert_scene(0), None);
    }

    #[test]
    fn matrix_legato_clip_and_scene_trigger_and_scene_quantization() {
        use hikaru_core::SampleRate;

        let mut matrix = TrackMatrix::new();
        matrix.set_global_quantization(QuantizationGrid::Bar);
        matrix.set_scene_quantization(1, Some(QuantizationGrid::Eighth));

        let mut pos = TransportPosition::new(SampleRate::new(44100.0), 120.0);
        pos.sample_count = 11025; // 0.5 beat
        assert_eq!(matrix.effective_scene_quantization(0), QuantizationGrid::Bar);
        assert_eq!(matrix.effective_scene_quantization(1), QuantizationGrid::Eighth);
        assert_eq!(matrix.samples_until_scene_launch(0, &pos, 44100.0), 77175);
        assert_eq!(matrix.samples_until_scene_launch(1, &pos, 44100.0), 0);

        let mut c0 = Clip::new(1, 1);
        c0.set_source_length_frames(4000);
        c0.set_loop_points(0, 4000);
        let mut c1 = Clip::new(2, 2);
        c1.set_source_length_frames(4000);
        c1.set_loop_points(0, 4000);
        c1.set_legato(true);
        let mut c2 = Clip::new(3, 3);
        c2.set_source_length_frames(4000);
        c2.set_loop_points(0, 4000);
        c2.set_legato(false);

        matrix.slots[0][0] = Some(c0);
        matrix.slots[0][1] = Some(c1);
        matrix.slots[0][2] = Some(c2);

        assert!(matrix.trigger_clip(0, 0, 500));
        assert_eq!(matrix.slots[0][0].as_ref().unwrap().start_sample, 500);

        // Disparo legato a t=1700 hereda start_sample=500 y detiene escena 0
        assert!(matrix.trigger_clip(0, 1, 1700));
        assert_eq!(matrix.slots[0][0].as_ref().unwrap().get_state(), ClipState::Stopped);
        assert_eq!(matrix.slots[0][1].as_ref().unwrap().get_state(), ClipState::Playing);
        assert_eq!(matrix.slots[0][1].as_ref().unwrap().start_sample, 500);

        // Disparo normal (Trigger) a t=2500 reinicia en 2500
        matrix.trigger_scene_now(2, 2500);
        assert_eq!(matrix.slots[0][1].as_ref().unwrap().get_state(), ClipState::Stopped);
        assert_eq!(matrix.slots[0][2].as_ref().unwrap().start_sample, 2500);
    }
}