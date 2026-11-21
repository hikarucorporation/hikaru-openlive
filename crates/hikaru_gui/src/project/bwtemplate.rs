// Importador de plantillas Bitwig Template (.bwtemplate) y flujo "New from Template".
// GNU AGPLv3
// crates/hikaru_gui/src/project/bwtemplate.rs

use std::path::{Path, PathBuf};

use crate::app::AppState;
use crate::project::bwproject;
use crate::project::common::ProjectError;

pub const EXTENSION: &str = "bwtemplate";
pub const FILE_FILTER: &str = "Bitwig Template (*.bwtemplate)";

/// Parsea y aplica un archivo `.bwtemplate` sobre `state`.
///
/// Reutiliza el parser de `.bwproject` (contenedor `BWPROJ\x01\x00` TLV y
/// detección de cabecera nativa `BtWg`), y aplica el saneamiento de pistas por
/// defecto, ruteo, envíos (`bt-02`) y *channel strips* (`bt-03`).
pub fn load(state: &mut AppState, path: &Path) -> Result<(), ProjectError> {
    bwproject::load(state, path)?;
    ensure_default_tracks_and_routing(state);
    apply_channel_strip_defaults(state);
    Ok(())
}

/// Garantiza pistas por defecto, índice de selección válido y envíos (`sends`)
/// acotados a `fx_tracks` (`bt-02`).
pub fn ensure_default_tracks_and_routing(state: &mut AppState) {
    if state.live_tracks.is_empty() {
        state
            .live_tracks
            .push(crate::views::mixer::Track::new(0, "Master".to_string(), true));
        state
            .live_tracks
            .push(crate::views::mixer::Track::new(1, "Track 1".to_string(), false));
    }
    if state.selected_track_index >= state.live_tracks.len() {
        state.selected_track_index = 0;
    }
    let fx_count = state.fx_tracks.len();
    for track in &mut state.live_tracks {
        if track.sends.len() > fx_count {
            track.sends.truncate(fx_count);
        }
    }
}

/// Sanea los parámetros de *channel strip* (volumen, paneo) de las pistas
/// cargadas desde plantilla (`bt-03`).
pub fn apply_channel_strip_defaults(state: &mut AppState) {
    for track in &mut state.live_tracks {
        track.volume = track.volume.clamp(0.0, 2.0);
        track.pan = track.pan.clamp(-1.0, 1.0);
    }
}

/// Crea un nuevo proyecto en memoria a partir de una plantilla `.bwtemplate`.
///
/// A diferencia de abrir un proyecto normal, limpia `state.project_path` y
/// `state.project_dirty` para que al guardar se solicite una ruta nueva y no
/// se sobrescriba la plantilla.
pub fn new_from_template(state: &mut AppState, path: &Path) -> Result<(), ProjectError> {
    load(state, path)?;
    state.project_path = None;
    state.project_dirty = false;
    Ok(())
}

/// Diálogo para elegir una plantilla `.bwtemplate`. `None` si el usuario cancela.
pub fn ask_template_path(current: Option<&Path>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .set_title("Nuevo proyecto desde plantilla (.bwtemplate)")
        .add_filter(FILE_FILTER, &[EXTENSION]);
    if let Some(current) = current.and_then(|p| p.parent()) {
        dialog = dialog.set_directory(current);
    }
    dialog.pick_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_template_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "hikaru_bwtemplate_test_{}_{}.bwtemplate",
            std::process::id(),
            name
        ))
    }

    fn test_state() -> AppState {
        use std::sync::atomic::{AtomicU32, AtomicU64};
        use std::sync::mpsc::channel;
        use std::sync::Arc;
        use crate::audio_proxy::AudioProxy;

        let (tx, _rx) = channel();
        AppState::new(
            AudioProxy::new(tx),
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU32::new(0)),
            None,
        )
    }

    #[test]
    fn new_from_template_loads_state_without_setting_project_path() {
        let mut src = test_state();
        src.transport.bpm = 132.0;

        let path = temp_template_path("new_workflow");
        bwproject::export_to_path(&src, &path).expect("export template");

        let mut dst = test_state();
        dst.project_path = Some(PathBuf::from("/tmp/previous.oplf"));
        dst.project_dirty = true;

        new_from_template(&mut dst, &path).expect("new_from_template");

        assert!((dst.transport.bpm - 132.0).abs() < 0.001);
        assert!(dst.project_path.is_none());
        assert!(!dst.project_dirty);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn template_ensures_default_tracks_sends_and_channel_strip_bounds() {
        let mut state = test_state();
        state.live_tracks.clear();
        state.selected_track_index = 99;

        ensure_default_tracks_and_routing(&mut state);
        assert!(state.live_tracks.len() >= 2);
        assert_eq!(state.selected_track_index, 0);

        let fx_count = state.fx_tracks.len();
        for track in &state.live_tracks {
            assert!(track.sends.len() <= fx_count);
        }

        state.live_tracks[0].volume = 5.0;
        state.live_tracks[0].pan = -3.0;
        apply_channel_strip_defaults(&mut state);
        assert!((state.live_tracks[0].volume - 2.0).abs() < 0.001);
        assert!((state.live_tracks[0].pan - (-1.0)).abs() < 0.001);
    }

    #[test]
    fn bwtemplate_round_trip_preserves_launcher_audio_clips_and_notifies_engine() {
        use crate::audio_proxy::{AudioProxy, GuiCommand};
        use crate::views::matrix::{ClipData, MatrixClip, SlotState};
        use crate::views::playlist::PlaylistState;
        use std::sync::atomic::{AtomicU32, AtomicU64};
        use std::sync::mpsc::channel;
        use std::sync::Arc;

        let mut src = test_state();
        let sample_path = PathBuf::from("/tmp/cymatics_test_loop.wav");
        src.matrix_state.grid[0][0].state = SlotState::Stopped;
        src.matrix_state.grid[0][0].clip = Some(MatrixClip {
            id: 1,
            name: "Cymatics - Energize".to_string(),
            path: sample_path.clone(),
            duration_secs: 4.0,
            content: ClipData::Audio {
                events: Vec::new(),
                next_event_id: 1,
                preview_mix: Vec::new(),
                preview_sr: 44100,
            },
            local_state: PlaylistState::default(),
            local_track: src.live_tracks[1].clone(),
            local_bar: 0.0,
            loop_start: 0,
            loop_end: 3840,
            loop_enabled: true,
            launch_legato: false,
            has_time_selection: false,
            peaks: Vec::new(),
            muted: false,
            color: Some(egui::Color32::from_rgb(0xE0, 0x60, 0x20)),
        });

        let path = temp_template_path("audio_clips");
        bwproject::export_to_path(&src, &path).expect("export template with audio clip");

        let (tx, rx) = channel();
        let mut dst = AppState::new(
            AudioProxy::new(tx),
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU32::new(0)),
            None,
        );

        new_from_template(&mut dst, &path).expect("load template with audio clip");

        let loaded = dst.matrix_state.grid[0][0]
            .clip
            .as_ref()
            .expect("audio clip should be restored in slot [0][0]");
        assert_eq!(loaded.name, "Cymatics - Energize");
        assert_eq!(loaded.path, sample_path);
        assert!(matches!(loaded.content, ClipData::Audio { .. }));

        let cmds: Vec<GuiCommand> = rx.try_iter().collect();
        let has_load_clip = cmds.iter().any(|c| {
            matches!(
                c,
                GuiCommand::LoadClip {
                    track_index: 0,
                    scene_index: 0,
                    ..
                }
            )
        });
        assert!(
            has_load_clip,
            "loading .bwtemplate must register audio clip in AudioEngine via GuiCommand::LoadClip"
        );

        let _ = std::fs::remove_file(&path);
    }
}
