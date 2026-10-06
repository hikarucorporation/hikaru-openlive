// crates/hikaru_gui/tests/project_roundtrip.rs
//
// Tests del formato de proyecto `.hikaru`.
//
// Lo que se verifica es el CONTRATO DEL ARCHIVO, no la UI: que un estado con
// contenido pase por disco sin perder lo que el formato promete guardar. El
// caso que importa es guardar → tocar el estado → abrir, y que lo que NO se
// guarda (picos de waveform) se recalcule solo al abrir.

use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Arc;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{App, AppContext, Context, Entity, TestAppContext, Window};

use hikaru_gui::app::{AppMode, AppState, HikaruApp, OpenStudioView, PanMode};
use hikaru_gui::audio_proxy::{AudioProxy, GuiCommand};
use hikaru_gui::project;
use hikaru_gui::views::matrix::{self, ClipData, MatrixClip};
use hikaru_gui::views::mixer;
use hikaru_gui::views::playlist::{self, ClipType};

fn test_app(window: &mut Window, cx: &mut Context<HikaruApp>) -> HikaruApp {
    // El receptor se descarta a propósito: `AudioProxy::send` ignora el error
    // de envío, así que los comandos del proyecto no bloquean el test.
    let (tx, _rx) = std::sync::mpsc::channel::<GuiCommand>();
    HikaruApp::build(
        window,
        cx,
        AudioProxy::new(tx),
        None,
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU32::new(0.0f32.to_bits())),
        None,
    )
}

/// Abre la app real y devuelve la entidad de estado.
///
/// Se hace así (y no con un `AppState` armando a mano) para que los tests corran
/// contra los mismos defaults que ve el usuario al abrir el programa.
fn open_state(cx: &mut TestAppContext) -> Entity<AppState> {
    cx.update(gpui_kit::init);
    let handle = cx.add_window(|window, cx| test_app(window, cx));
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
    })
    .unwrap();
    cx.update_window(handle.into(), |view, _, cx| {
        view.downcast::<HikaruApp>()
            .expect("raíz HikaruApp")
            .read(cx)
            .state
            .clone()
    })
    .unwrap()
}

/// `Entity::update` pide `&mut App`, que fuera del ciclo de render sólo se
/// consigue por `TestAppContext::update`.
fn mutate<R>(
    cx: &mut TestAppContext,
    st: &Entity<AppState>,
    f: impl FnOnce(&mut AppState) -> R,
) -> R {
    cx.update(|app| st.update(app, |state, _| f(state)))
}

/// Lectura del estado, con el mismo rodeo que [`mutate`].
fn inspect<R>(cx: &TestAppContext, st: &Entity<AppState>, f: impl FnOnce(&AppState) -> R) -> R {
    cx.read(|app| f(st.read(app)))
}

/// Directorio temporal que se borra al terminar el test.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!("hikaru_proj_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }

    fn join(&self, name: &str) -> std::path::PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// WAV de un segundo a 48k, seno de 220Hz.
///
/// Va al disco porque el formato guarda RUTAS y no audio: un clip sin archivo
/// detrás no se puede recargar y el test no probaría nada.
fn write_test_wav(path: &std::path::Path) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..48_000i32 {
        let s = ((i as f32) * 0.013).sin();
        writer.write_sample((s * 20_000.0) as i16).unwrap();
    }
    writer.finalize().unwrap();
}

// =========================================================================
// MEZCLA
// =========================================================================

/// Todo lo que el formato declara guardar de una pista tiene que volver: no
/// alcanza con probar el nombre.
///
/// Ojo con la fila 1 de la matriz: el canal 1 de OpenLive es el MISMO objeto en
/// el mixer y en el header de la Session Matrix, así que el test lo edita por
/// los dos lados (como hace la UI) y verifica que no queden contradictorios.
#[gpui_kit::gpui::test]
fn pistas_viajan_por_el_archivo(cx: &mut TestAppContext) {
    let dir = TempDir::new("tracks");
    let path = dir.join("mix.hikaru");
    let st = open_state(cx);

    mutate(cx, &st, |state| {
        state.transport.bpm = 128.0;
        state.transport.beats_per_bar = 3;
        state.live_tracks[1].name = "BASS".into();
        state.live_tracks[1].volume = 0.33;
        state.live_tracks[1].pan = -0.5;
        state.live_tracks[1].mute = true;
        state.live_tracks[1].arm = true;
        state.live_tracks[1].pan_mode = PanMode::MidSide;
        state.live_tracks[1].effects = vec![mixer::DspSlot::new(0, "Reverb".into())];
        state.live_tracks[1].effects[0].active = false;
        state.studio_tracks[1].name = "LEAD".into();
        state.studio_tracks[1].volume = 0.11;
        // El header de la matriz escribe en los dos lados: se replica.
        state.matrix_state.tracks[0].name = "BASS".into();
        state.matrix_state.tracks[0].volume = 0.33;
        state.matrix_state.tracks[0].pan = -0.5;
        state.matrix_state.tracks[0].muted = true;
        state.matrix_state.tracks[2].name = "PERC".into();
        state.matrix_state.scenes[3].name = "DROP".into();
        // El nombre de la fila 2 lo edita el HEADER de la matriz, así que el
        // canal del mixer lo tiene por sincronización y no al revés.
        state.live_tracks[3].name = "PERC".into();
        project::save(state, &path).unwrap();
    });

    // Se arruina todo y se restaura.
    mutate(cx, &st, |state| {
        state.live_tracks[1].name = "ARRASADO".into();
        state.live_tracks[1].volume = 1.0;
        state.live_tracks[1].mute = false;
        state.live_tracks[1].effects.clear();
        state.matrix_state.tracks[0].name = "OTRO".into();
        state.matrix_state.tracks[2].name = "OTRO".into();
        state.matrix_state.scenes[3].name = "OTRA".into();
        state.transport.bpm = 90.0;
        project::load(state, &path).unwrap();
    });

    inspect(cx, &st, |state| {
        assert_eq!(state.transport.bpm, 128.0);
        assert_eq!(state.transport.beats_per_bar, 3);
        assert_eq!(state.live_tracks[1].name, "BASS");
        assert!((state.live_tracks[1].volume - 0.33).abs() < 1e-6);
        assert!((state.live_tracks[1].pan + 0.5).abs() < 1e-6);
        assert!(state.live_tracks[1].mute);
        assert!(state.live_tracks[1].arm);
        assert!(matches!(state.live_tracks[1].pan_mode, PanMode::MidSide));
        assert_eq!(state.live_tracks[1].effects.len(), 1);
        assert_eq!(state.live_tracks[1].effects[0].name, "Reverb");
        assert!(!state.live_tracks[1].effects[0].active);
        assert_eq!(state.studio_tracks[1].name, "LEAD");
        assert!((state.studio_tracks[1].volume - 0.11).abs() < 1e-6);
        assert_eq!(state.matrix_state.tracks[2].name, "PERC");
        assert_eq!(state.matrix_state.scenes[3].name, "DROP");
        // Matriz y mixer quedan de acuerdo: no hay dos verdades en el archivo.
        assert_eq!(state.matrix_state.tracks[0].name, "BASS");
        assert!((state.matrix_state.tracks[0].volume - 0.33).abs() < 1e-6);
        assert!((state.matrix_state.tracks[0].pan + 0.5).abs() < 1e-6);
        assert!(state.matrix_state.tracks[0].muted);
    });
}

// =========================================================================
// CLIPS DE LA MATRIZ
// =========================================================================

/// El clip de audio del pad se recarga desde el WAV (el archivo sólo guarda la
/// ruta) y los picos se releen, porque son caché y no van al `.hikaru`.
#[gpui_kit::gpui::test]
fn clip_de_audio_se_recarga_desde_el_disco(cx: &mut TestAppContext) {
    let dir = TempDir::new("matrix_clip");
    let wav = dir.join("kick.wav");
    write_test_wav(&wav);
    let path = dir.join("sesion.hikaru");
    let st = open_state(cx);

    mutate(cx, &st, |state| {
        matrix::load_clip_into_slot(
            &mut state.matrix_state,
            &state.audio_proxy,
            0,
            0,
            wav.clone(),
            state.transport.bpm,
        );
        state.matrix_state.grid[0][0]
            .clip
            .as_mut()
            .expect("el clip se cargó")
            .loop_end = 1920;
        project::save(state, &path).unwrap();
    });

    mutate(cx, &st, |state| {
        state.matrix_state.grid[0][0].clip = None;
        project::load(state, &path).unwrap();
    });

    inspect(cx, &st, |state| {
        let clip = state.matrix_state.grid[0][0]
            .clip
            .as_ref()
            .expect("el clip volvió desde el archivo");
        assert_eq!(clip.name, "kick");
        assert_eq!(clip.path, wav);
        assert_eq!(clip.loop_end, 1920);
        assert_eq!(clip.peaks.len(), MatrixClip::PEAK_BINS);
        assert!(clip.peaks.iter().any(|p| *p > 0.0));
        match &clip.content {
            ClipData::Audio { events, .. } => {
                assert_eq!(events.len(), 1);
                assert_eq!(events[0].sample_rate, 48000);
                assert_eq!(events[0].source_path.as_deref(), Some(wav.as_path()));
                assert!(!events[0].samples.is_empty());
            }
            other => panic!("esperaba un clip de audio, encontré {:?}", other),
        }
    });
}

/// Un pad MIDI no toca el disco: sus notas son datos y viajan tal cual.
#[gpui_kit::gpui::test]
fn clip_midi_no_toca_el_disco(cx: &mut TestAppContext) {
    let dir = TempDir::new("midi_clip");
    let path = dir.join("notas.hikaru");
    let st = open_state(cx);

    mutate(cx, &st, |state| {
        let mut clip = MatrixClip {
            id: 7,
            name: "bajo.mid".into(),
            path: dir.join("bajo.mid"),
            duration_secs: 2.0,
            content: ClipData::Midi {
                notes: vec![(0, 36, 100, 480), (480, 43, 80, 240)],
            },
            local_state: Default::default(),
            local_track: mixer::Track::new(0, "bajo".into(), false),
            local_bar: 1.0,
            loop_start: 0,
            loop_end: 960,
            loop_enabled: true,
            has_time_selection: true,
            peaks: Vec::new(),
        };
        clip.local_track.pan = 0.25;
        state.matrix_state.grid[2][1].clip = Some(clip);
        state.matrix_state.next_clip_id = 8;
        project::save(state, &path).unwrap();
    });

    mutate(cx, &st, |state| {
        state.matrix_state.grid[2][1].clip = None;
        project::load(state, &path).unwrap();
    });

    inspect(cx, &st, |state| {
        let clip = state.matrix_state.grid[2][1].clip.as_ref().expect("volvió");
        assert_eq!(clip.id, 7);
        assert_eq!(clip.loop_end, 960);
        assert!(clip.loop_enabled);
        assert!((clip.local_track.pan - 0.25).abs() < 1e-6);
        assert_eq!(state.matrix_state.next_clip_id, 8);
        match &clip.content {
            ClipData::Midi { notes } => {
                assert_eq!(notes.len(), 2);
                assert_eq!(notes[0], (0, 36, 100, 480));
                assert_eq!(notes[1], (480, 43, 80, 240));
            }
            other => panic!("esperaba un clip MIDI, encontré {:?}", other),
        }
    });
}

// =========================================================================
// PLAYLIST
// =========================================================================

/// Los ticks del clip (posición, duración, trim del sample) son datos del
/// proyecto; los picos de la waveform se recalculan del WAV al abrir.
#[gpui_kit::gpui::test]
fn clip_de_playlist_conserva_ticks_y_recalcula_picos(cx: &mut TestAppContext) {
    let dir = TempDir::new("playlist");
    let wav = dir.join("stab.wav");
    write_test_wav(&wav);
    let path = dir.join("linea.hikaru");
    let st = open_state(cx);

    mutate(cx, &st, |state| {
        let ppqn = state.playlist_state.ppqn;
        let bpm = state.transport.bpm;
        let mut clip = playlist::build_audio_clip(
            1,
            "stab".into(),
            &wav,
            960,
            ppqn,
            bpm,
            gpui_kit::rgb(0x205F91).into(),
        );
        // El trim del sample va aparte de los ticks del clip.
        match &mut clip.clip_type {
            ClipType::Audio { sample_offset_ticks, .. } => *sample_offset_ticks = 480,
            other => panic!("esperaba audio, {:?}", other),
        }
        state.playlist_state.clips.push((1, clip));
        state.playlist_state.loop_start_ticks = 0;
        state.playlist_state.loop_end_ticks = 7680;
        state.playlist_state.loop_region_active = true;
        project::save(state, &path).unwrap();
    });

    mutate(cx, &st, |state| {
        state.playlist_state.clips.clear();
        state.playlist_state.loop_end_ticks = 0;
        project::load(state, &path).unwrap();
    });

    inspect(cx, &st, |state| {
        assert_eq!(state.playlist_state.clips.len(), 1);
        assert_eq!(state.playlist_state.loop_end_ticks, 7680);
        assert!(state.playlist_state.loop_region_active);
        let (track_idx, clip) = &state.playlist_state.clips[0];
        assert_eq!(*track_idx, 1);
        assert_eq!(clip.id, 1);
        assert_eq!(clip.name, "stab");
        assert_eq!(clip.start_tick, 960);
        match &clip.clip_type {
            ClipType::Audio {
                sample_path,
                peaks,
                sample_offset_ticks,
                ..
            } => {
                assert_eq!(sample_path, &wav.to_string_lossy());
                assert_eq!(*sample_offset_ticks, 480);
                assert!(!peaks.is_empty());
                assert!(peaks.iter().any(|p| *p > 0.0));
            }
            other => panic!("esperaba un clip de audio, encontré {:?}", other),
        }
    });
}

// =========================================================================
// VISTA
// =========================================================================

/// El proyecto se reabre en la vista en la que se guardó: es un detalle chico
/// que se agradece mucho.
#[gpui_kit::gpui::test]
fn se_guarda_el_modo_y_la_vista(cx: &mut TestAppContext) {
    let dir = TempDir::new("ui");
    let path = dir.join("vista.hikaru");
    let st = open_state(cx);

    mutate(cx, &st, |state| {
        state.mode = AppMode::OpenStudio;
        state.openstudio_view = OpenStudioView::ArrangerMixer;
        state.show_dsp_rack = true;
        project::save(state, &path).unwrap();
    });

    mutate(cx, &st, |state| {
        state.mode = AppMode::OpenLive;
        state.openstudio_view = OpenStudioView::Playlist;
        state.show_dsp_rack = false;
        project::load(state, &path).unwrap();
    });

    inspect(cx, &st, |state| {
        assert_eq!(state.mode, AppMode::OpenStudio);
        assert_eq!(state.openstudio_view, OpenStudioView::ArrangerMixer);
        assert!(state.show_dsp_rack);
    });
}

// =========================================================================
// REGLAS DEL FORMATO
// =========================================================================

/// Escribir "sesion" sin extensión tiene que producir "sesion.hikaru": nadie
/// quiere descubrir el fallo recién en el segundo guardado.
#[gpui_kit::gpui::test]
fn save_agrega_la_extension(cx: &mut TestAppContext) {
    let dir = TempDir::new("ext");
    let st = open_state(cx);

    let written = mutate(cx, &st, |state| {
        project::save(state, &dir.join("sesion")).unwrap()
    });
    assert_eq!(written.extension().unwrap(), "hikaru");
    assert!(written.exists());
}

/// Abrir un WAV con `File > Open` tiene que fallar con un mensaje que lo diga,
/// no con un error de serde sobre un campo faltante.
#[gpui_kit::gpui::test]
fn rechaza_un_archivo_que_no_es_proyecto(cx: &mut TestAppContext) {
    let dir = TempDir::new("basura");
    let path = dir.join("wav.hikaru");
    std::fs::write(&path, b"RIFF....WAVE esto es audio, no un proyecto").unwrap();
    let st = open_state(cx);

    let err = mutate(cx, &st, |state| project::load(state, &path))
        .expect_err("un WAV no se abre como proyecto");
    assert!(
        matches!(err, project::ProjectError::NotAProject(_)),
        "error inesperado: {:?}",
        err
    );
}

/// Un proyecto de una build futura se rechaza con el número de versión, no con
/// un error de parseo que no dice nada.
#[gpui_kit::gpui::test]
fn rechaza_una_version_futura(cx: &mut TestAppContext) {
    let dir = TempDir::new("futuro");
    let path = dir.join("futuro.hikaru");
    std::fs::write(
        &path,
        format!("{{ \"format_version\": {} }}", project::FORMAT_VERSION + 1),
    )
    .unwrap();
    let st = open_state(cx);

    let err = mutate(cx, &st, |state| project::load(state, &path))
        .expect_err("una versión futura no se abre");
    match err {
        project::ProjectError::UnsupportedVersion { found, expected } => {
            assert_eq!(found, project::FORMAT_VERSION + 1);
            assert_eq!(expected, project::FORMAT_VERSION);
        }
        other => panic!("error inesperado: {:?}", other),
    }
}

/// `File > New Project` deja la sesión como recién abierta, no como un proyecto
/// guardado con todo borrado.
#[gpui_kit::gpui::test]
fn new_project_deja_la_sesion_limpia(cx: &mut TestAppContext) {
    let st = open_state(cx);

    mutate(cx, &st, |state| {
        state.live_tracks[1].name = "MODIFICADA".into();
        state.playlist_state.next_clip_id = 99;
        project::reset(state);
    });

    inspect(cx, &st, |state| {
        assert_eq!(state.live_tracks[1].name, "Track 1");
        assert_eq!(state.playlist_state.next_clip_id, 1);
        assert!(state.playlist_state.clips.is_empty());
        assert!((state.transport.bpm - 140.0).abs() < 1e-9);
        assert_eq!(state.matrix_state.tracks.len(), 8);
        assert_eq!(state.matrix_state.scenes.len(), 8);
        assert_eq!(state.matrix_state.grid.len(), 8);
    });
}
