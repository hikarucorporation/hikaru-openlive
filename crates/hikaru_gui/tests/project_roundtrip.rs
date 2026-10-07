// crates/hikaru_gui/tests/project_roundtrip.rs
//
// Tests de los DOS formatos de proyecto: `.oplf` (OpenLive) y `.opsf`
// (OpenStudio).
//
// Lo que se verifica es el CONTRATO DEL ARCHIVO, no la UI: que un estado con
// contenido pase por disco sin perder lo que el formato promete guardar, y --
// tan importante como eso -- SIN guardar lo que el formato prohíbe guardar.
//
// Los tests de pureza del formato son los que protegen la frontera: un `.oplf` a
// que se le colara un campo `playlist` volvería a ser el formato monolítico que
// este refactor eliminó.

use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Arc;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{App, AppContext, Context, Entity, TestAppContext, Window};

use hikaru_gui::app::{AppMode, AppState, HikaruApp, OpenLiveView, OpenStudioView, PanMode};
use hikaru_gui::audio_proxy::{AudioProxy, GuiCommand};
use hikaru_gui::project;
use hikaru_gui::views::matrix::{self, ClipData, MatrixClip};
use hikaru_gui::views::mixer;
use hikaru_gui::views::playlist::{self, ClipType};
use hikaru_gui::version::VERSION;

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

/// Igual que [`open_state`], pero CONSERVA el canal de comandos.
///
/// Los tests que necesitan observar lo que la app le manda al motor no pueden
/// usar [`open_state`], que tira el receptor: sin él, un `GuiCommand` que se
/// pierde es indistinguible de uno que nunca se mandó.
fn open_state_with_proxy(
    cx: &mut TestAppContext,
) -> (Entity<AppState>, std::sync::mpsc::Receiver<GuiCommand>) {
    cx.update(gpui_kit::init);
    let (tx, rx) = std::sync::mpsc::channel::<GuiCommand>();
    let handle = cx.add_window(|window, cx| {
        HikaruApp::build(
            window,
            cx,
            AudioProxy::new(tx),
            None,
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU32::new(0.0f32.to_bits())),
            None,
        )
    });
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
    })
    .unwrap();
    let st = cx
        .update_window(handle.into(), |view, _, cx| {
            view.downcast::<HikaruApp>()
                .expect("raíz HikaruApp")
                .read(cx)
                .state
                .clone()
        })
        .unwrap();
    (st, rx)
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

/// El JSON del archivo, como `serde_json::Value`, para poder mirar la FORMA del
/// documento y no sólo lo que el deserializador de Rust acepta.
fn json_of(path: &std::path::Path) -> serde_json::Value {
    let text = std::fs::read_to_string(path).expect("el archivo se escribió");
    serde_json::from_str(&text).expect("el archivo es JSON válido")
}

// =========================================================================
// PUREZA DEL FORMATO: LA FRONTERA ENTRE .oplf Y .opsf
// =========================================================================

/// Un `.oplf` NO puede llevar datos del timeline lineal.
///
/// Es el test que justifica toda la partición. Si mañana alguien "agrega un
/// campo más" a `OpenLiveProject` y ese campo termina siendo del Arranger, este
/// test lo frena. La lista es la del OPLF-README.md §3.2.
#[gpui_kit::gpui::test]
fn un_oplf_no_lleva_datos_del_timeline_lineal(cx: &mut TestAppContext) {
    let dir = TempDir::new("puro_oplf");
    let path = dir.join("sesion.oplf");
    let st = open_state(cx);

    mutate(cx, &st, |state| {
        state.transport.bpm = 160.0;
        // Se llena TODO de las dos mitades: si el `.oplf` se contaminara, el
        // contaminado tiene que tener de dónde contaminarse.
        state.studio_tracks[1].name = "LEAD".into();
        state.studio_tracks[1].volume = 0.11;
        state.playlist_state.clips.push((1, playlist_placeholder_clip()));
        state.playlist_state.loop_end_ticks = 7680;
        project::save(state, &path).unwrap();
    });

    let json = json_of(&path);
    let root = json.as_object().expect("la raíz del JSON es un objeto");

    for prohibido in ["playlist", "ppqn", "studio_tracks", "arranger_clips"] {
        assert!(
            !root.contains_key(prohibido),
            "un .oplf no puede contener `{}` (OPLF-README.md §3.2). Raíz: {:?}",
            prohibido,
            root.keys().collect::<Vec<_>>()
        );
    }

    // Y lo que SÍ tiene que estar, para que el test no passara por omitir
    // todo: la matriz, los canales de live y el transporte.
    for exigido in ["matrix", "live_tracks", "transport", "metadata", "created_by"] {
        assert!(
            root.contains_key(exigido),
            "un .oplf tiene que traer `{}`. Raíz: {:?}",
            exigido,
            root.keys().collect::<Vec<_>>()
        );
    }
    assert_eq!(root["engine_mode"], "OpenLive");
}

/// Un `.opsf` NO puede llevar datos de la performance en vivo.
///
/// La frontera va en los dos sentidos: sin esto, un `.opsf` con `matrix` y
/// `live_tracks` adentro volvería a ser el archivo híbrido.
#[gpui_kit::gpui::test]
fn un_opsf_no_lleva_datos_de_la_performance_en_vivo(cx: &mut TestAppContext) {
    let dir = TempDir::new("puro_opsf");
    let path = dir.join("arreglo.opsf");
    let st = open_state(cx);

    mutate(cx, &st, |state| {
        state.mode = AppMode::OpenStudio;
        state.matrix_state.tracks[0].name = "BASS".into();
        state.live_tracks[1].volume = 0.9;
        state.playlist_state.clips.push((1, playlist_placeholder_clip()));
        project::save(state, &path).unwrap();
    });

    let json = json_of(&path);
    let root = json.as_object().expect("la raíz del JSON es un objeto");

    for prohibido in ["matrix", "live_tracks", "launch_quantization", "dsp_global"] {
        assert!(
            !root.contains_key(prohibido),
            "un .opsf no puede contener `{}`. Raíz: {:?}",
            prohibido,
            root.keys().collect::<Vec<_>>()
        );
    }
    for exigido in ["playlist", "studio_tracks", "transport", "metadata"] {
        assert!(
            root.contains_key(exigido),
            "un .opsf tiene que traer `{}`. Raíz: {:?}",
            exigido,
            root.keys().collect::<Vec<_>>()
        );
    }
    // El PPQN vive adentro de la playlist: es un dato del timeline, no del
    // transporte.
    assert_eq!(json["playlist"]["ppqn"], 960);
    assert_eq!(root["engine_mode"], "OpenStudio");
}

/// Guardar desde OpenLive no toca el estado del timeline, y viceversa.
///
/// Esta es la mitad del aislamiento que NO se ve mirando el JSON: que el
/// `apply` además de no escribir esos datos, tampoco los borre. Un `.oplf`
/// abierto encima de una sesión con clips en el Arranger tiene que dejar esos
/// clips como estaban.
#[gpui_kit::gpui::test]
fn cargar_un_oplf_no_toca_el_timeline(cx: &mut TestAppContext) {
    let dir = TempDir::new("aisla_oplf");
    let path = dir.join("sesion.oplf");
    let st = open_state(cx);

    mutate(cx, &st, |state| {
        state.mode = AppMode::OpenStudio;
        state.playlist_state.clips.push((1, playlist_placeholder_clip()));
        state.playlist_state.next_clip_id = 42;
        state.playlist_state.loop_end_ticks = 7680;
        state.studio_tracks[1].name = "LEAD".into();
        // Se pasa a OpenLive ANTES de guardar: es el modo el que decide el
        // formato, así que esto tiene que producir un `.oplf` (y por lo tanto
        // dejar el timeline abajo, sin escribir).
        state.mode = AppMode::OpenLive;
        project::save(state, &path).unwrap();
    });

    mutate(cx, &st, |state| {
        state.mode = AppMode::OpenLive;
        project::load(state, &path).unwrap();
    });

    inspect(cx, &st, |state| {
        assert_eq!(state.mode, AppMode::OpenLive, "el archivo manda sobre el modo");
        // La mitad lineal quedó como estaba: el `.oplf` ni la miró.
        assert_eq!(state.playlist_state.clips.len(), 1);
        assert_eq!(state.playlist_state.next_clip_id, 42);
        assert_eq!(state.playlist_state.loop_end_ticks, 7680);
        assert_eq!(state.studio_tracks[1].name, "LEAD");
    });
}

/// Simétrico: cargar un `.opsf` no toca la Session Matrix ni los canales de live.
#[gpui_kit::gpui::test]
fn cargar_un_opsf_no_toca_la_matriz(cx: &mut TestAppContext) {
    let dir = TempDir::new("aisla_opsf");
    let path = dir.join("arreglo.opsf");
    let st = open_state(cx);

    mutate(cx, &st, |state| {
        state.mode = AppMode::OpenStudio;
        state.matrix_state.tracks[0].name = "BASS".into();
        state.live_tracks[1].volume = 0.9;
        project::save(state, &path).unwrap();
    });

    mutate(cx, &st, |state| {
        // El `.opsf` no tiene la matriz, así que estos valores NO se pueden
        // restaurar desde el archivo: el punto del test es que cargar no los
        // toque, así que el nombre se deja en "OTRO" a propósito y se verifica
        // que sigue ahí.
        state.matrix_state.tracks[0].name = "OTRO".into();
        state.live_tracks[1].volume = 0.1;
        project::load(state, &path).unwrap();
    });

    inspect(cx, &st, |state| {
        assert_eq!(state.mode, AppMode::OpenStudio, "el archivo manda sobre el modo");
        assert_eq!(
            state.matrix_state.tracks[0].name, "OTRO",
            "cargar un .opsf no puede reescribir la Session Matrix"
        );
        assert!(
            (state.live_tracks[1].volume - 0.1).abs() < 1e-6,
            "cargar un .opsf no puede reescribir los canales de live"
        );
    });
}

/// La cabecera dice la versión del BINARIO, no la del formato ni una vieja.
///
/// El bug que se está cazando acá es concreto: `created_by` salía de
/// `CARGO_PKG_VERSION`, que era `0.1.0`, y el OPLF-README.md §3.1 lo prohíbe
/// explícitamente.
#[gpui_kit::gpui::test]
fn la_cabecera_usa_la_version_dinamica_del_software(cx: &mut TestAppContext) {
    let dir = TempDir::new("cabecera");
    let oplf = dir.join("sesion.oplf");
    let opsf = dir.join("arreglo.opsf");
    let st = open_state(cx);

    mutate(cx, &st, |state| {
        project::save(state, &oplf).unwrap();
        state.mode = AppMode::OpenStudio;
        project::save(state, &opsf).unwrap();
    });

    assert_eq!(VERSION, "2.22.2", "la versión del producto se define en el Cargo.toml");

    let oplf_json = json_of(&oplf);
    assert_eq!(oplf_json["created_by"], "Hikaru OpenLive 2.22.2");
    assert_eq!(oplf_json["format_version"], project::FORMAT_VERSION);

    let opsf_json = json_of(&opsf);
    assert_eq!(opsf_json["created_by"], "Hikaru OpenStudio 2.22.2");
    assert_eq!(opsf_json["format_version"], project::FORMAT_VERSION);

    // La versión del formato y la del software son dos números distintos, y
    // confundirlos es justo lo que rompe a un lector externo.
    assert_ne!(
        project::FORMAT_VERSION.to_string(),
        VERSION,
        "format_version y la versión del software son cosas distintas"
    );
}

// =========================================================================
// EXTENSIONES Y GUARDADO
// =========================================================================

/// El MODO decide la extensión, y el `rfd` ofrece una sola.
///
/// Es el comportamiento que pedía el formato nuevo: en OpenLive la única
/// opción es `.oplf`.
#[gpui_kit::gpui::test]
fn el_modo_decide_la_extension_del_archivo(cx: &mut TestAppContext) {
    let dir = TempDir::new("ext_modo");
    let st = open_state(cx);

    let en_live = mutate(cx, &st, |state| {
        state.mode = AppMode::OpenLive;
        project::save(state, &dir.join("sesion")).unwrap()
    });
    assert_eq!(en_live.extension().unwrap(), "oplf");
    assert!(en_live.exists());

    let en_studio = mutate(cx, &st, |state| {
        state.mode = AppMode::OpenStudio;
        project::save(state, &dir.join("arreglo")).unwrap()
    });
    assert_eq!(en_studio.extension().unwrap(), "opsf");
    assert!(en_studio.exists());
}

/// Escribir "sesion" sin extensión tiene que producir "sesion.oplf": nadie
/// quiere descubrir el fallo recién en el segundo guardado.
#[gpui_kit::gpui::test]
fn save_agrega_la_extension(cx: &mut TestAppContext) {
    let dir = TempDir::new("ext");
    let st = open_state(cx);

    let written = mutate(cx, &st, |state| project::save(state, &dir.join("sesion")).unwrap());
    assert_eq!(written.extension().unwrap(), "oplf");
    assert!(written.exists());
}

/// `File > Save` reusa el camino anterior: si el modo cambió desde entonces, el
/// nombre viejo tiene que corregirse.
///
/// Sin esto se escribiría un `.opsf` (con el timeline adentro) DENTRO de un
/// `sesion.oplf`, que al reabrirse se leería como OpenLive y perdería el
/// arreglo entero. Es el peor bug posible de esta partición.
#[gpui_kit::gpui::test]
fn save_corrije_la_extension_si_cambio_el_modo(cx: &mut TestAppContext) {
    let dir = TempDir::new("ext_cambio");
    let st = open_state(cx);

    let written = mutate(cx, &st, |state| {
        state.mode = AppMode::OpenLive;
        let path = project::save(state, &dir.join("trabajo")).unwrap();
        assert_eq!(path.extension().unwrap(), "oplf");

        // El usuario cambia de modo y le da Save otra vez: `project_path` sigue
        // apuntando al `.oplf`.
        state.mode = AppMode::OpenStudio;
        project::save(state, &path).unwrap()
    });

    assert_eq!(
        written.extension().unwrap(),
        "opsf",
        "el nombre del archivo tiene que seguir al formato del contenido"
    );

    // Y lo que quedó en el `.oplf` viejo es realmente OpenLive.
    let json = json_of(&dir.join("trabajo.oplf"));
    assert_eq!(json["engine_mode"], "OpenLive");
}

// =========================================================================
// MEZCLA (OpenLive)
// =========================================================================

/// Todo lo que el `.oplf` declara guardar de un canal tiene que volver: no
/// alcanza con probar el nombre.
///
/// Ojo con la fila 1 de la matriz: el canal 1 de OpenLive es el MISMO objeto en
/// el mixer y en el header de la Session Matrix, así que el test lo edita por
/// los dos lados (como hace la UI) y verifica que no queden contradictorios.
#[gpui_kit::gpui::test]
fn pistas_de_live_viajan_por_el_oplf(cx: &mut TestAppContext) {
    let dir = TempDir::new("tracks_live");
    let path = dir.join("mix.oplf");
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
        assert_eq!(state.matrix_state.tracks[2].name, "PERC");
        assert_eq!(state.matrix_state.scenes[3].name, "DROP");
        // Matriz y mixer quedan de acuerdo: no hay dos verdades en el archivo.
        assert_eq!(state.matrix_state.tracks[0].name, "BASS");
        assert!((state.matrix_state.tracks[0].volume - 0.33).abs() < 1e-6);
        assert!((state.matrix_state.tracks[0].pan + 0.5).abs() < 1e-6);
        assert!(state.matrix_state.tracks[0].muted);
    });
}

/// Los canales del Arranger viajan por el `.opsf`, con la misma forma.
#[gpui_kit::gpui::test]
fn pistas_de_studio_viajan_por_el_opsf(cx: &mut TestAppContext) {
    let dir = TempDir::new("tracks_studio");
    let path = dir.join("mix.opsf");
    let st = open_state(cx);

    mutate(cx, &st, |state| {
        state.mode = AppMode::OpenStudio;
        state.studio_tracks[1].name = "LEAD".into();
        state.studio_tracks[1].volume = 0.11;
        state.studio_tracks[1].pan = 0.4;
        state.studio_tracks[1].arm = true;
        state.studio_tracks[1].effects = vec![mixer::DspSlot::new(0, "Delay".into())];
        project::save(state, &path).unwrap();
    });

    mutate(cx, &st, |state| {
        state.studio_tracks[1].name = "ARRASADO".into();
        state.studio_tracks[1].volume = 1.0;
        state.studio_tracks[1].effects.clear();
        project::load(state, &path).unwrap();
    });

    inspect(cx, &st, |state| {
        assert_eq!(state.studio_tracks[1].name, "LEAD");
        assert!((state.studio_tracks[1].volume - 0.11).abs() < 1e-6);
        assert!((state.studio_tracks[1].pan - 0.4).abs() < 1e-6);
        assert!(state.studio_tracks[1].arm);
        assert_eq!(state.studio_tracks[1].effects.len(), 1);
        assert_eq!(state.studio_tracks[1].effects[0].name, "Delay");
    });
}

// =========================================================================
// CLIPS DE LA MATRIZ (.oplf)
// =========================================================================

/// El clip de audio del pad se recarga desde el WAV (el archivo sólo guarda la
/// ruta) y los picos se releen, porque son caché y no van al `.oplf`.
#[gpui_kit::gpui::test]
fn clip_de_audio_se_recarga_desde_el_disco(cx: &mut TestAppContext) {
    let dir = TempDir::new("matrix_clip");
    let wav = dir.join("kick.wav");
    write_test_wav(&wav);
    let path = dir.join("sesion.oplf");
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
        // El flag de loop tiene que viajar al `.oplf` y volver: si se pierde,
        // el proyecto reabre en one-shot aunque el usuario lo hubiera dejado
        // loopeando.
        state.matrix_state.grid[0][0]
            .clip
            .as_mut()
            .expect("el clip se cargó")
            .loop_enabled = true;
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
        // Requisito 4b: el flag de loop y su región sobreviven al viaje.
        assert!(
            clip.loop_enabled,
            "el flag de loop se perdió en el viaje por el .oplf"
        );
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

/// Al ABRIR un `.oplf`, cada clip de audio tiene que quedar REGISTRADO en el
/// motor, no sólo reconstruido en la GUI.
///
/// Este es el test del bug "el proyecto abre con nombres y waveforms correctos
/// pero los pads no suenan". `build_clip` decodificaba el WAV, armaba los
/// eventos y leía los picos (por eso la Session Matrix se veía bien), pero
/// `apply_matrix` no mandaba ningún `LoadClip` al motor. Como `AudioEngine`
/// indexa sus clips por `(track_index, scene_index)` y `trigger_clip` itera esa
/// lista, un pad sin `LoadClip` es un pad silencioso: se podía disparar
/// cuantas veces se quisiera y no sonaba nada.
///
/// El orden también es contrato: `SetClipEvents` y `SetClipLoop` buscan un clip
/// que ya exista (hacen `find` y no hacen nada si no está), así que `LoadClip`
/// tiene que ir primero.
#[gpui_kit::gpui::test]
fn abrir_un_oplf_registra_los_clips_en_el_motor(cx: &mut TestAppContext) {
    let dir = TempDir::new("motor_registro");
    let wav = dir.join("kick.wav");
    write_test_wav(&wav);
    let path = dir.join("sesion.oplf");
    let (st, rx) = open_state_with_proxy(cx);

    // Un pad con sample, guardado desde el gesto que sí funciona.
    mutate(cx, &st, |state| {
        matrix::load_clip_into_slot(
            &mut state.matrix_state,
            &state.audio_proxy,
            0,
            0,
            wav.clone(),
            state.transport.bpm,
        );
        project::save(state, &path).unwrap();
    });

    // Drenar lo que mandó el gesto de arrastrar: no es lo que se está probando.
    while rx.try_recv().is_ok() {}

    // Ahora el ciclo de carga del proyecto.
    mutate(cx, &st, |state| {
        state.matrix_state.grid[0][0].clip = None;
        project::load(state, &path).unwrap();
    });

    let comandos: Vec<GuiCommand> = std::iter::from_fn(|| rx.try_recv().ok()).collect();

    let pos_load = comandos.iter().position(
        |c| matches!(c, GuiCommand::LoadClip { track_index: 0, scene_index: 0, .. }),
    );
    let pos_eventos = comandos.iter().position(
        |c| matches!(c, GuiCommand::SetClipEvents { track_idx: 0, scene_idx: 0, .. }),
    );

    let pos_load = pos_load.expect(
        "al abrir el proyecto hay que registrar el clip en el motor (GuiCommand::LoadClip); \
         sin esto el pad queda mudo aunque la GUI muestre nombre y waveform",
    );
    let pos_eventos = pos_eventos.expect(
        "el pad tiene un evento de audio cargado: hay que mandarlo al motor \
         (GuiCommand::SetClipEvents)",
    );

    assert!(
        pos_load < pos_eventos,
        "LoadClip debe ir ANTES que SetClipEvents: set_clip_events busca un clip que ya \
         exista y, si no, descarta los eventos en silencio (found load@{pos_load}, events@{pos_eventos})"
    );

    // La ruta registrada tiene que ser la RESUELTA (absoluta o relativa al
    // proyecto), no la del motor de render: el motor corre en otro hilo/proceso
    // lógico y no tiene el `base_dir` del archivo.
    let ruta_registrada = match &comandos[pos_load] {
        GuiCommand::LoadClip { path, .. } => path.clone(),
        _ => unreachable!("pos_load es un LoadClip"),
    };
    assert_eq!(
        std::path::Path::new(&ruta_registrada),
        wav.as_path(),
        "el motor tiene que recibir la ruta del sample tal como quedó resuelta al cargar"
    );
}

/// Un pad MIDI no toca el disco: sus notas son datos y viajan tal cual.
#[gpui_kit::gpui::test]
fn clip_midi_no_toca_el_disco(cx: &mut TestAppContext) {
    let dir = TempDir::new("midi_clip");
    let path = dir.join("notas.oplf");
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

/// La cuantización de disparo del `.oplf` se escribe en notación musical y
/// vuelve a la división del compás.
///
/// El campo `launch_quantization` es el canónico de la especificación, así que
/// el round-trip tiene que pasar por la cadena string -> u32.
#[gpui_kit::gpui::test]
fn la_cuantizacion_de_disparo_hace_round_trip(cx: &mut TestAppContext) {
    let dir = TempDir::new("quant");
    let path = dir.join("sesion.oplf");
    let st = open_state(cx);

    mutate(cx, &st, |state| {
        state.transport.beat_division = 16;
        state.transport.beats_per_bar = 5;
        project::save(state, &path).unwrap();
    });

    let json = json_of(&path);
    assert_eq!(json["transport"]["launch_quantization"], "1/16");
    assert_eq!(json["transport"]["time_signature"][0], 5);

    mutate(cx, &st, |state| {
        state.transport.beat_division = 4;
        state.transport.beats_per_bar = 4;
        project::load(state, &path).unwrap();
    });

    inspect(cx, &st, |state| {
        assert_eq!(state.transport.beat_division, 16);
        assert_eq!(state.transport.beats_per_bar, 5);
    });
}

/// Una `launch_quantization` corrupta no puede dejar el transporte en 0.
///
/// La división en cero rompe la conversión a segundos y manda NaN al motor.
#[gpui_kit::gpui::test]
fn una_cuantizacion_corrupta_no_deja_la_division_en_cero(cx: &mut TestAppContext) {
    let dir = TempDir::new("quant_mala");
    let path = dir.join("sesion.oplf");
    let st = open_state(cx);

    mutate(cx, &st, |state| project::save(state, &path).unwrap());

    let texto = std::fs::read_to_string(&path).unwrap();
    let roto = texto.replace("\"launch_quantization\": \"1/4\"", "\"launch_quantization\": \"1/0\"");
    assert_ne!(texto, roto, "el archivo debería traer 1/4");
    std::fs::write(&path, roto).unwrap();

    mutate(cx, &st, |state| project::load(state, &path).unwrap());

    inspect(cx, &st, |state| {
        assert!(
            state.transport.beat_division >= 1,
            "la división del compás no puede quedar en 0 (encontré {})",
            state.transport.beat_division
        );
    });
}

// =========================================================================
// PLAYLIST (.opsf)
// =========================================================================

/// Clip de patrón sin sample detrás, para ensuciar el estado sin tocar disco.
///
/// Los tests de pureza y de aislamiento sólo necesitan QUE HAYA un clip en la
/// playlist; no les interesa de qué tipo sea.
fn playlist_placeholder_clip() -> playlist::PlaylistClip {
    playlist::PlaylistClip {
        id: 1,
        name: "placeholder".into(),
        start_tick: 960,
        duration_ticks: 960,
        clip_type: ClipType::Pattern { pattern_id: 3 },
        color: gpui_kit::rgb(0x205F91).into(),
    }
}

/// Los ticks del clip (posición, duración, trim del sample) son datos del
/// proyecto; los picos de la waveform se recalculan del WAV al abrir.
#[gpui_kit::gpui::test]
fn clip_de_playlist_conserva_ticks_y_recalcula_picos(cx: &mut TestAppContext) {
    let dir = TempDir::new("playlist");
    let wav = dir.join("stab.wav");
    write_test_wav(&wav);
    let path = dir.join("linea.opsf");
    let st = open_state(cx);

    mutate(cx, &st, |state| {
        state.mode = AppMode::OpenStudio;
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

/// El PPQN es del timeline y viaja en el `.opsf`: es el número que define dónde
/// cae el tick de cada clip, así que perderlo descentra la sesión entera.
#[gpui_kit::gpui::test]
fn el_opsf_conserva_el_ppqn(cx: &mut TestAppContext) {
    let dir = TempDir::new("ppqn");
    let path = dir.join("arreglo.opsf");
    let st = open_state(cx);

    mutate(cx, &st, |state| {
        state.mode = AppMode::OpenStudio;
        state.playlist_state.ppqn = 480;
        project::save(state, &path).unwrap();
    });

    let json = json_of(&path);
    assert_eq!(json["playlist"]["ppqn"], 480);

    mutate(cx, &st, |state| {
        state.playlist_state.ppqn = 960;
        project::load(state, &path).unwrap();
    });

    inspect(cx, &st, |state| {
        assert_eq!(state.playlist_state.ppqn, 480);
    });
}

// =========================================================================
// VISTA
// =========================================================================

/// Cada formato reabre la app en SU modo, que es lo único coherente: un `.oplf`
/// en Modo OpenStudio no significaría nada.
#[gpui_kit::gpui::test]
fn cada_formato_reabre_en_su_modo(cx: &mut TestAppContext) {
    let dir = TempDir::new("modo");
    let oplf = dir.join("live.oplf");
    let opsf = dir.join("linea.opsf");
    let st = open_state(cx);

    mutate(cx, &st, |state| {
        state.mode = AppMode::OpenStudio;
        state.openstudio_view = OpenStudioView::ArrangerMixer;
        state.show_dsp_rack = true;
        project::save(state, &opsf).unwrap();

        state.mode = AppMode::OpenLive;
        state.openlive_view = OpenLiveView::ArrangerView;
        project::save(state, &oplf).unwrap();
    });

    mutate(cx, &st, |state| {
        state.mode = AppMode::OpenLive;
        state.openstudio_view = OpenStudioView::Playlist;
        state.show_dsp_rack = false;
        project::load(state, &opsf).unwrap();
    });

    inspect(cx, &st, |state| {
        assert_eq!(state.mode, AppMode::OpenStudio);
        assert_eq!(state.openstudio_view, OpenStudioView::ArrangerMixer);
        assert!(state.show_dsp_rack);
    });

    mutate(cx, &st, |state| {
        state.mode = AppMode::OpenStudio;
        project::load(state, &oplf).unwrap();
    });

    inspect(cx, &st, |state| {
        assert_eq!(state.mode, AppMode::OpenLive);
        assert_eq!(state.openlive_view, OpenLiveView::ArrangerView);
    });
}

// =========================================================================
// REGLAS DEL FORMATO
// =========================================================================

/// Abrir un WAV con `File > Open` tiene que fallar con un mensaje que lo diga,
/// no con un error de serde sobre un campo faltante.
#[gpui_kit::gpui::test]
fn rechaza_un_archivo_que_no_es_proyecto(cx: &mut TestAppContext) {
    let dir = TempDir::new("basura");
    let path = dir.join("wav.oplf");
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
    let st = open_state(cx);

    for nombre in ["futuro.oplf", "futuro.opsf"] {
        let path = dir.join(nombre);
        std::fs::write(
            &path,
            format!("{{ \"format_version\": {} }}", project::FORMAT_VERSION + 1),
        )
        .unwrap();

        let err = mutate(cx, &st, |state| project::load(state, &path))
            .expect_err("una versión futura no se abre");
        match err {
            project::ProjectError::UnsupportedVersion { found, expected } => {
                assert_eq!(found, project::FORMAT_VERSION + 1);
                assert_eq!(expected, project::FORMAT_VERSION);
            }
            other => panic!("error inesperado para {}: {:?}", nombre, other),
        }
    }
}

/// `.hikaru` no es un formato: un archivo con esa extensión no abre.
#[gpui_kit::gpui::test]
fn la_extension_legacy_no_es_un_proyecto(cx: &mut TestAppContext) {
    let dir = TempDir::new("legacy");
    let path = dir.join("viejo.hikaru");
    let st = open_state(cx);

    // Un archivo con la cabecera del formato viejo tampoco: `.hikaru` quedó
    // huérfano y ya no hay código que lo lea.
    std::fs::write(
        &path,
        format!(
            "{{ \"format_version\": 1, \"playlist\": {{ \"ppqn\": 960 }}, \"studio_tracks\": [] }}"
        ),
    )
    .unwrap();

    let err = mutate(cx, &st, |state| project::load(state, &path))
        .expect_err("un .hikaru ya no es un proyecto de Hikaru");
    assert!(
        matches!(err, project::ProjectError::NotAProject(_)),
        "error inesperado: {:?}",
        err
    );
}

/// `File > New Project` deja la sesión como recién abierta, no como un proyecto
/// guardado con todo borrado.
#[gpui_kit::gpui::test]
fn new_project_deja_la_sesion_limpia(cx: &mut TestAppContext) {
    let st = open_state(cx);

    mutate(cx, &st, |state| {
        state.mode = AppMode::OpenLive;
        state.live_tracks[1].name = "MODIFICADA".into();
        state.matrix_state.grid[0][0].clip = Some(MatrixClip {
            id: 1,
            name: "x".into(),
            path: ruta_midi_de_prueba(),
            duration_secs: 1.0,
            content: ClipData::Midi { notes: Vec::new() },
            local_state: Default::default(),
            local_track: mixer::Track::new(0, "x".into(), false),
            local_bar: 1.0,
            loop_start: 0,
            loop_end: 960,
            loop_enabled: true,
            has_time_selection: true,
            peaks: Vec::new(),
        });
        project::reset(state);
    });

    inspect(cx, &st, |state| {
        assert_eq!(state.live_tracks[1].name, "Track 1");
        assert!((state.transport.bpm - 140.0).abs() < 1e-9);
        assert_eq!(state.matrix_state.tracks.len(), 8);
        assert_eq!(state.matrix_state.scenes.len(), 8);
        assert_eq!(state.matrix_state.grid.len(), 8);
    });

    // Y lo mismo en el otro modo: el timeline arranca vacío.
    mutate(cx, &st, |state| {
        state.mode = AppMode::OpenStudio;
        state.playlist_state.next_clip_id = 99;
        state.studio_tracks[1].name = "MODIFICADA".into();
        project::reset(state);
    });

    inspect(cx, &st, |state| {
        assert_eq!(state.studio_tracks[1].name, "TRACK 01");
        assert_eq!(state.playlist_state.next_clip_id, 1);
        assert!(state.playlist_state.clips.is_empty());
    });
}

/// Ruta de MIDI cualquiera, para poblar un pad sin tocar el disco.
///
/// El clip no lo lee nadie en estos tests: sólo importa que la grilla tenga
/// algo, para que `reset` tenga que limpiarlo.
fn ruta_midi_de_prueba() -> std::path::PathBuf {
    std::path::PathBuf::from("/tmp/no-se-lee.mid")
}