// Integration tests for Bitwig 5.1.7 Parity — Block 1 (5 features):
//   ts-12: Tap tempo
//   cl-15: Legato clip launch
//   cl-16: Add / remove scenes
//   cl-17: Clip loop start/end editor
//   cl-18: Global launch-quantization override per scene

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

use hikaru_audio_engine::{AudioEngine, EngineMode, QuantizationGrid};
use hikaru_core::{AudioBuffer, SampleRate};
use hikaru_gui::app::AppState;
use hikaru_gui::audio_proxy::{AudioProxy, GuiCommand};
use hikaru_gui::project::openlive;
use hikaru_gui::views::detail_editor_panel::ClipEditorTarget;
use hikaru_gui::views::matrix::{self, AudioEvent, ClipData, MatrixClip, MatrixSlot, SlotState};
use hikaru_gui::views::mixer::Track;
use hikaru_gui::views::playlist::PlaylistState;

fn make_app_state() -> (AppState, Receiver<GuiCommand>) {
    let (tx, rx) = channel();
    let proxy = AudioProxy::new(tx);
    let state = AppState::new(
        proxy,
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU32::new(0)),
        None,
    );
    // Drain startup commands so each test inspects only its own commands.
    while rx.try_recv().is_ok() {}
    (state, rx)
}

fn make_audio_clip(id: usize, sr: u32, frames: usize) -> MatrixClip {
    let samples = vec![0.25f32; frames];
    let dur = frames as f64 / sr as f64;
    let ev = AudioEvent {
        id: 1,
        name: format!("Audio {id}"),
        source_path: Some(PathBuf::from("loop.wav")),
        samples: samples.clone(),
        channels: 1,
        sample_rate: sr,
        start_secs: 0.0,
        trim_left_frames: 0,
        visible_frames: frames,
        gain: 1.0,
        fade_in_secs: 0.0,
        fade_out_secs: 0.0,
    };
    MatrixClip {
        id,
        name: format!("Clip {id}"),
        path: PathBuf::from("loop.wav"),
        duration_secs: dur,
        content: ClipData::Audio {
            events: vec![ev],
            next_event_id: 2,
            preview_mix: samples,
            preview_sr: sr,
        },
        local_state: PlaylistState::default(),
        local_track: Track::new(0, "Audio".into(), false),
        local_bar: 1.0,
        loop_start: 0,
        loop_end: frames as u64,
        loop_enabled: true,
        launch_legato: true,
        has_time_selection: true,
        peaks: Vec::new(),
        muted: false,
        color: None,
    }
}

#[test]
fn ts12_tap_tempo_updates_transport_engine_and_persists_in_oplf() {
    let (mut state, rx) = make_app_state();
    assert!(!state.tap_tempo_enabled);

    // Primer tap: activa estado pero no cambia BPM todavía.
    assert_eq!(state.tap_tempo(1.0), None);
    assert!(state.tap_tempo_enabled);
    assert!(rx.try_recv().is_err());

    // Taps constantes cada 0.5 s => 120.0 BPM
    assert_eq!(state.tap_tempo(1.5), Some(120.0));
    assert_eq!(state.tap_tempo(2.0), Some(120.0));
    assert!((state.transport.bpm - 120.0).abs() < 1e-6);

    let mut saw_set_bpm_120 = false;
    while let Ok(cmd) = rx.try_recv() {
        if let GuiCommand::SetBpm(b) = cmd {
            if (b - 120.0).abs() < 1e-3 {
                saw_set_bpm_120 = true;
            }
        }
    }
    assert!(saw_set_bpm_120, "tap_tempo debe enviar GuiCommand::SetBpm");

    // Roundtrip en .oplf
    let snapshot = openlive::capture(&state);
    assert!(snapshot.transport.tap_tempo_enabled);
    assert!((snapshot.transport.bpm - 120.0).abs() < 1e-6);

    let (mut restored, _rx2) = make_app_state();
    openlive::apply(&mut restored, &snapshot, None);
    assert!(restored.tap_tempo_enabled);
    assert!((restored.transport.bpm - 120.0).abs() < 1e-6);
}

#[test]
fn cl16_add_insert_remove_scenes_syncs_selection_editor_and_engine() {
    let (mut state, rx) = make_app_state();
    let initial_scenes = state.matrix_state.scenes.len();
    assert!(initial_scenes >= 2);

    // Seleccionar slot en escena 1 y apuntar clip_editor allí
    state.matrix_state.selected_slot = Some((0, 1));
    state.clip_editor.target = Some(ClipEditorTarget::Matrix {
        track_idx: 0,
        slot_idx: 1,
    });
    state.clip_editor.active_target = state.clip_editor.target;

    // Insertar escena en índice 1 -> selección y editor se desplazan a 2
    let inserted = state.insert_scene_at(1);
    assert_eq!(inserted, 1);
    assert_eq!(state.matrix_state.scenes.len(), initial_scenes + 1);
    assert_eq!(state.matrix_state.selected_slot, Some((0, 2)));
    assert_eq!(
        state.clip_editor.active_target,
        Some(ClipEditorTarget::Matrix {
            track_idx: 0,
            slot_idx: 2
        })
    );
    assert!(matches!(
        rx.try_recv(),
        Ok(GuiCommand::InsertScene { scene_idx: 1 })
    ));

    // Agregar escena al final sincronizada
    let added = state.matrix_state.add_scene_synced(&state.audio_proxy);
    assert_eq!(added, initial_scenes + 1);
    assert!(matches!(rx.try_recv(), Ok(GuiCommand::AddScene)));

    // Eliminar escena 1 -> selección y editor vuelven a 1
    assert!(state.remove_scene_at(1));
    assert_eq!(state.matrix_state.selected_slot, Some((0, 1)));
    assert_eq!(
        state.clip_editor.target,
        Some(ClipEditorTarget::Matrix {
            track_idx: 0,
            slot_idx: 1
        })
    );
    assert!(matches!(
        rx.try_recv(),
        Ok(GuiCommand::RemoveScene { scene_idx: 1 })
    ));

    // Eliminar la escena seleccionada limpia selección y clip_editor
    assert!(state.remove_scene_at(1));
    assert_eq!(state.matrix_state.selected_slot, None);
    assert_eq!(state.clip_editor.target, None);
    assert_eq!(state.clip_editor.active_target, None);

    // Nunca baja de 1 escena
    while state.matrix_state.scenes.len() > 1 {
        assert!(state.remove_scene_at(0));
    }
    assert!(!state.remove_scene_at(0));
    assert_eq!(state.matrix_state.scenes.len(), 1);
}

#[test]
fn cl15_legato_clip_launch_toggle_syncs_engine_and_persists_in_oplf() {
    let (mut state, rx) = make_app_state();
    matrix::create_empty_midi_clip(&mut state.matrix_state, &state.audio_proxy, 0, 0);
    while rx.try_recv().is_ok() {}

    assert!(state.matrix_state.slot(0, 0).unwrap().clip.as_ref().unwrap().launch_legato);

    // Desactivar legato launch y verificar comando al motor
    assert!(state
        .matrix_state
        .set_clip_legato(0, 0, false, &state.audio_proxy));
    assert!(!state.matrix_state.slot(0, 0).unwrap().clip.as_ref().unwrap().launch_legato);
    assert!(matches!(
        rx.try_recv(),
        Ok(GuiCommand::SetClipLegato {
            track_idx: 0,
            scene_idx: 0,
            legato: false
        })
    ));

    // Roundtrip .oplf conserva launch_legato=false
    let snapshot = openlive::capture(&state);
    assert_eq!(
        snapshot.matrix.grid[0][0].as_ref().unwrap().launch_legato,
        Some(false)
    );
    let (mut restored, rx2) = make_app_state();
    openlive::apply(&mut restored, &snapshot, None);
    assert!(!restored.matrix_state.slot(0, 0).unwrap().clip.as_ref().unwrap().launch_legato);

    let mut saw_legato_false = false;
    while let Ok(cmd) = rx2.try_recv() {
        if let GuiCommand::SetClipLegato {
            track_idx: 0,
            scene_idx: 0,
            legato: false,
        } = cmd
        {
            saw_legato_false = true;
        }
    }
    assert!(saw_legato_false, "openlive::apply debe reenviar SetClipLegato");
}

#[test]
fn cl17_clip_loop_start_end_editor_validates_and_syncs_audio_and_midi() {
    let (mut state, rx) = make_app_state();

    // Audio clip de 2.0 s a 44100 Hz (88200 frames)
    state.matrix_state.grid[0][0] = MatrixSlot {
        state: SlotState::Stopped,
        has_stop_button: true,
        clip: Some(make_audio_clip(1, 44100, 88200)),
    };

    // Rechaza NaN
    assert!(!matrix::set_audio_clip_loop_secs(
        &mut state.matrix_state,
        &state.audio_proxy,
        0,
        0,
        f64::NAN,
        1.0,
        true,
    ));

    // Fijar región válida [0.5s, 1.5s]
    assert!(matrix::set_audio_clip_loop_secs(
        &mut state.matrix_state,
        &state.audio_proxy,
        0,
        0,
        0.5,
        1.5,
        true,
    ));
    {
        let clip = state.matrix_state.slot(0, 0).unwrap().clip.as_ref().unwrap();
        assert_eq!(clip.loop_start, 22050);
        assert_eq!(clip.loop_end, 66150);
        assert!(clip.loop_enabled);
    }
    match rx.try_recv() {
        Ok(GuiCommand::SetClipLoop {
            track_idx: 0,
            scene_idx: 0,
            start_secs,
            end_secs,
            enabled: true,
        }) => {
            assert!((start_secs - 0.5).abs() < 1e-4);
            assert!((end_secs - 1.5).abs() < 1e-4);
        }
        _ => panic!("Se esperaba GuiCommand::SetClipLoop"),
    }

    // Invertido / fuera de rango se clampa con start < end <= duration
    assert!(matrix::set_audio_clip_loop_secs(
        &mut state.matrix_state,
        &state.audio_proxy,
        0,
        0,
        1.5,
        0.2,
        true,
    ));
    {
        let clip = state.matrix_state.slot(0, 0).unwrap().clip.as_ref().unwrap();
        assert!(clip.loop_end > clip.loop_start);
        assert!(clip.loop_end <= 88200);
    }
    while rx.try_recv().is_ok() {}

    // Toggle loop en audio clip apaga y sincroniza
    assert_eq!(
        matrix::toggle_clip_loop(&mut state.matrix_state, &state.audio_proxy, 0, 0),
        Some(false)
    );
    assert!(matches!(
        rx.try_recv(),
        Ok(GuiCommand::SetClipLoop {
            track_idx: 0,
            scene_idx: 0,
            enabled: false,
            ..
        })
    ));

    // MIDI clip: editar loop start/end en ticks y toggle loop
    matrix::create_empty_midi_clip(&mut state.matrix_state, &state.audio_proxy, 0, 1);
    while rx.try_recv().is_ok() {}

    assert!(matrix::set_midi_clip_loop_ticks(
        &mut state.matrix_state,
        &state.audio_proxy,
        0,
        1,
        960,
        1000, // menor a 960+240 -> debe clampearse a 1200
        true,
        120.0,
    ));
    {
        let clip = state.matrix_state.slot(0, 1).unwrap().clip.as_ref().unwrap();
        assert_eq!(clip.loop_start, 960);
        assert_eq!(clip.loop_end, 1200);
        assert!(clip.loop_enabled);
    }
    assert!(matches!(
        rx.try_recv(),
        Ok(GuiCommand::SetMidiClipLoop {
            track_idx: 0,
            scene_idx: 1,
            loop_start_ticks: 960,
            loop_end_ticks: 1200,
        })
    ));
    assert!(matches!(
        rx.try_recv(),
        Ok(GuiCommand::SetMidiClipLoopEnabled {
            track_idx: 0,
            scene_idx: 1,
            enabled: true,
        })
    ));
}

#[test]
fn cl18_scene_launch_quantization_override_persists_and_drives_engine() {
    let (mut state, rx) = make_app_state();

    state
        .matrix_state
        .set_global_launch_quantization(QuantizationGrid::Bar, &state.audio_proxy);
    assert!(state.matrix_state.set_scene_launch_quantization(
        1,
        Some(QuantizationGrid::Sixteenth),
        &state.audio_proxy,
    ));
    assert_eq!(
        state.matrix_state.effective_scene_quantization(0),
        QuantizationGrid::Bar
    );
    assert_eq!(
        state.matrix_state.effective_scene_quantization(1),
        QuantizationGrid::Sixteenth
    );

    assert!(matches!(
        rx.try_recv(),
        Ok(GuiCommand::SetGlobalLaunchQuantization {
            grid: QuantizationGrid::Bar
        })
    ));
    assert!(matches!(
        rx.try_recv(),
        Ok(GuiCommand::SetSceneLaunchQuantization {
            scene_idx: 1,
            grid: Some(QuantizationGrid::Sixteenth)
        })
    ));

    // Roundtrip .oplf conserva global + override por escena
    let snapshot = openlive::capture(&state);
    assert_eq!(snapshot.transport.launch_quantization, "1/1");
    assert_eq!(snapshot.matrix.scenes[0].launch_quantization, None);
    assert_eq!(
        snapshot.matrix.scenes[1].launch_quantization.as_deref(),
        Some("1/16")
    );

    let (mut restored, _rx2) = make_app_state();
    openlive::apply(&mut restored, &snapshot, None);
    assert_eq!(
        restored.matrix_state.global_launch_quantization,
        QuantizationGrid::Bar
    );
    assert_eq!(restored.matrix_state.scenes[0].launch_quantization, None);
    assert_eq!(
        restored.matrix_state.scenes[1].launch_quantization,
        Some(QuantizationGrid::Sixteenth)
    );

    // Verificación directa contra AudioEngine real
    static TABLE: [f32; 64] = [0.0; 64];
    let clock = Arc::new(AtomicU64::new(0));
    let mut engine = AudioEngine::new(SampleRate::new(44100.0), &TABLE, clock);
    engine.set_mode(EngineMode::OpenLive);
    engine.transport.set_bpm(120.0); // 1 beat = 22050, 1/16 = 5512.5 -> 5513, 1 bar = 88200
    engine.set_global_launch_quantization(QuantizationGrid::Bar);
    engine.set_scene_launch_quantization(1, Some(QuantizationGrid::Sixteenth));
    engine.add_clip(1, 0, 1, vec![0.5f32; 4096 * 2], 0.0, 0.0, 0.0, 2, true);

    engine.play();
    let mut raw = vec![0.0f32; 1000 * 2];
    engine.process(&mut AudioBuffer::new(&mut raw));

    engine.trigger_scene(1);
    let pending = engine.pending_scene_launch().expect("debe encolarse en 1/16");
    assert_eq!(pending.grid, QuantizationGrid::Sixteenth);
    assert!(pending.target_sample < 6000);

    let mut raw2 = vec![0.0f32; 5000 * 2];
    engine.process(&mut AudioBuffer::new(&mut raw2));
    assert!(engine.pending_scene_launch().is_none());
    assert!(engine.clips[0].is_playing);
}
