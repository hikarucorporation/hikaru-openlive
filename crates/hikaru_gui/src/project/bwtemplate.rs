// Importador de plantillas Bitwig Template (.bwtemplate) y flujo "New from Template".
// GNU AGPLv3
// crates/hikaru_gui/src/project/bwtemplate.rs

use std::path::{Path, PathBuf};

use crate::app::AppState;
use crate::project::{btwg, bwproject, dawproject};
use crate::project::common::ProjectError;
use crate::views::matrix::ClipData;
use crate::views::playlist::ClipType;

pub const EXTENSION: &str = "bwtemplate";
pub const FILE_FILTER: &str = "Bitwig Template (*.bwtemplate)";

/// Exporta el proyecto actual como un `.bwtemplate` empaquetado en **ZIP**
/// con estructura nativa de Bitwig Studio 5.x:
/// - `template-info.json` (metadatos HOCON)
/// - `template.bwproject` (binario nativo `BtWg0003000200b8`)
/// - `view-settings` (binario `BtWg` `0x987`)
/// - `project/samples/<archivo>` (samples de audio referenciados embebidos)
/// - `hikaru-session.bwproject` (copia TLV sin pérdida para fidelidad 100% al
///   reabrir en Hikaru OpenLive)
pub fn export_to_path(state: &AppState, path: &Path) -> Result<(), ProjectError> {
    let title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Hikaru Template");

    // 1. Recolectar samples de audio del Launcher y Arranger para embeber en `project/samples/`
    let mut seen_paths: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    let mut sample_infos: Vec<btwg::BtWgSampleInfo> = Vec::new();
    let mut embedded_samples: Vec<(String, Vec<u8>)> = Vec::new();
    let mut used_names: std::collections::HashSet<String> = std::collections::HashSet::new();

    let mut register_sample = |src: &Path, fallback_dur: f64| {
        if src.as_os_str().is_empty() || !seen_paths.insert(src.to_path_buf()) {
            return;
        }
        if let Ok(bytes) = std::fs::read(src) {
            let base_name = src
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("sample.wav")
                .to_string();
            let mut unique = base_name.clone();
            let mut counter = 2usize;
            while !used_names.insert(unique.clone()) {
                unique = format!("{}_{}", counter, base_name);
                counter += 1;
            }
            let rel_in_project = format!("samples/{}", unique);
            let zip_entry = format!("project/{}", rel_in_project);
            let (sample_rate, channels, duration_secs) =
                btwg::parse_wav_metadata(&bytes, fallback_dur);
            let hash16 = btwg::sample_hash16(&bytes);
            sample_infos.push(btwg::BtWgSampleInfo {
                src_path: src.to_path_buf(),
                filename: unique,
                rel_path: rel_in_project,
                file_size: bytes.len() as i32,
                sample_rate,
                channels,
                duration_secs,
                hash16,
            });
            embedded_samples.push((zip_entry, bytes));
        }
    };

    for col in &state.matrix_state.grid {
        for slot in col {
            if let Some(mclip) = &slot.clip {
                if let ClipData::Audio { events, .. } = &mclip.content {
                    if !mclip.path.as_os_str().is_empty() {
                        register_sample(&mclip.path, mclip.duration_secs);
                    } else if let Some(ev_path) =
                        events.iter().find_map(|e| e.source_path.as_ref())
                    {
                        register_sample(ev_path, mclip.duration_secs);
                    }
                }
            }
        }
    }
    for (_, pclip) in &state.playlist_state.clips {
        if let ClipType::Audio { sample_path, .. } = &pclip.clip_type {
            if !sample_path.is_empty() {
                register_sample(Path::new(sample_path), 4.0);
            }
        }
    }

    // 2. Construir entradas del ZIP
    let template_info = btwg::encode_template_info(title);
    let template_bwproject = btwg::encode_state_to_btwg(state, title, &sample_infos);
    let view_settings = btwg::encode_minimal_view_settings();
    let hikaru_tlv = bwproject::build_bwproject_bytes(state)
        .map_err(|msg| ProjectError::Io(std::io::Error::other(msg)))?;

    let mut entries: Vec<(&str, &[u8])> = Vec::with_capacity(4 + embedded_samples.len());
    entries.push(("template-info.json", &template_info));
    entries.push(("template.bwproject", &template_bwproject));
    entries.push(("view-settings", &view_settings));
    entries.push(("hikaru-session.bwproject", &hikaru_tlv));
    for (zip_name, bytes) in &embedded_samples {
        entries.push((zip_name.as_str(), bytes.as_slice()));
    }

    let zip_bytes = dawproject::build_zip_store(&entries);
    std::fs::write(path, zip_bytes).map_err(ProjectError::Io)
}

/// Parsea y aplica un archivo `.bwtemplate` sobre `state`.
///
/// Soporta:
/// - **Plantillas nativas de Bitwig Studio (`.bwtemplate` ZIP)**: extrae
///   `template.bwproject` (`BtWg`) y los samples en `project/samples/*` a un
///   directorio temporal para que los clips de audio puedan reproducirse.
/// - **Plantillas en formato TLV de Hikaru (`BWPROJ\x01\x00`)** o `BtWg` plano.
///
/// Luego aplica el saneamiento de pistas por defecto, ruteo, envíos (`bt-02`)
/// y *channel strips* (`bt-03`).
pub fn load(state: &mut AppState, path: &Path) -> Result<(), ProjectError> {
    let bytes = std::fs::read(path).map_err(ProjectError::Io)?;
    if bytes.len() >= 4 && &bytes[0..4] == b"PK\x03\x04" {
        load_native_zip_template(state, path, &bytes)?;
    } else {
        bwproject::load(state, path)?;
    }
    ensure_default_tracks_and_routing(state);
    apply_channel_strip_defaults(state);
    Ok(())
}

fn load_native_zip_template(
    state: &mut AppState,
    template_path: &Path,
    zip_bytes: &[u8],
) -> Result<(), ProjectError> {
    let entries = crate::project::dawproject::read_zip_entries(zip_bytes).map_err(|msg| {
        ProjectError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, msg))
    })?;

    let bwproj_bytes = entries
        .get("hikaru-session.bwproject")
        .or_else(|| entries.get("template.bwproject"))
        .ok_or_else(|| {
            ProjectError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "template.bwproject not found within template",
            ))
        })?;

    // Extraer samples embebidos (`project/samples/*`) a un caché temporal
    // para que `apply_parsed_bwproject` pueda resolver rutas relativas `samples/...`.
    let stem = template_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("bitwig_template");
    let cache_project_dir = std::env::temp_dir()
        .join("hikaru_bwtemplate_cache")
        .join(stem)
        .join("project");
    for (entry_name, data) in &entries {
        if let Some(rel) = entry_name.strip_prefix("project/") {
            if rel.contains("..") || rel.is_empty() || rel.ends_with('/') {
                continue;
            }
            let out_path = cache_project_dir.join(rel);
            if let Some(parent) = out_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&out_path, data);
        }
    }

    let parsed = bwproject::parse_bwproject_bytes(bwproj_bytes).map_err(|msg| {
        ProjectError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, msg))
    })?;
    let synthetic_bwproj_path = cache_project_dir.join("template.bwproject");
    bwproject::apply_parsed_bwproject(state, &parsed, &synthetic_bwproj_path);
    state.project_path = Some(template_path.to_path_buf());
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
            launch_legato: true,
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
        // Para clips de audio, loop_end está en frames (4.0s * 44100 = 176400), no en ticks.
        assert!(
            loaded.loop_end >= 170_000,
            "expected audio loop_end in frames, got {}",
            loaded.loop_end
        );
        assert!(
            loaded.launch_legato,
            "launcher audio clip should preserve launch_legato = true"
        );

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

    #[test]
    fn loads_native_bitwig_zip_bwtemplate_with_btwg_tracks_bpm_and_clips() {
        // Construir un template.bwproject sintético con cabecera BtWg real (42 bytes)
        let mut btwg = Vec::new();
        btwg.extend_from_slice(b"BtWg0003000200b80000002a000000000000000000");
        assert_eq!(btwg.len(), 42);

        // BPM = 128.0 (campo 0x000002c8 + tag 0x07 + f64 BE)
        btwg.extend_from_slice(&[0x00, 0x00, 0x02, 0xc8, 0x07]);
        btwg.extend_from_slice(&128.0f64.to_be_bytes());

        // Pista "Classic Drums" + color verde
        btwg.extend_from_slice(&[0x00, 0x00, 0x01, 0x5b, 0x08]);
        btwg.extend_from_slice(&(13u32).to_be_bytes());
        btwg.extend_from_slice(b"Classic Drums");
        btwg.extend_from_slice(&[0x00, 0x00, 0x05, 0x4c, 0x16]);
        for c in [0.0f32, 0.616, 0.278, 1.0] {
            btwg.extend_from_slice(&c.to_be_bytes());
        }

        // Clip "Full Beat 1" en Classic Drums
        btwg.extend_from_slice(&[0x00, 0x00, 0x02, 0x86, 0x08]);
        btwg.extend_from_slice(&(11u32).to_be_bytes());
        btwg.extend_from_slice(b"Full Beat 1");

        // Pista "Mono Bass" + color rosa
        btwg.extend_from_slice(&[0x00, 0x00, 0x01, 0x5b, 0x08]);
        btwg.extend_from_slice(&(9u32).to_be_bytes());
        btwg.extend_from_slice(b"Mono Bass");
        btwg.extend_from_slice(&[0x00, 0x00, 0x05, 0x4c, 0x16]);
        for c in [0.851f32, 0.22, 0.443, 1.0] {
            btwg.extend_from_slice(&c.to_be_bytes());
        }

        // Clip "Bass Mel 1" en Mono Bass
        btwg.extend_from_slice(&[0x00, 0x00, 0x02, 0x86, 0x08]);
        btwg.extend_from_slice(&(10u32).to_be_bytes());
        btwg.extend_from_slice(b"Bass Mel 1");

        let zip_bytes = crate::project::dawproject::build_zip_store(&[
            ("template.bwproject", &btwg),
            ("template-info.json", b"{ name : \"Performance Set\" }"),
        ]);
        let path = temp_template_path("native_btwg_zip");
        std::fs::write(&path, &zip_bytes).expect("write native zip bwtemplate");

        let mut dst = test_state();
        new_from_template(&mut dst, &path).expect("load native bitwig .bwtemplate");

        assert!((dst.transport.bpm - 128.0).abs() < 0.001);
        assert!(dst.project_path.is_none());
        assert_eq!(dst.live_tracks[1].name, "Classic Drums");
        assert_eq!(dst.live_tracks[2].name, "Mono Bass");
        assert_eq!(
            dst.matrix_state.grid[0][0].clip.as_ref().map(|c| c.name.as_str()),
            Some("Full Beat 1")
        );
        assert_eq!(
            dst.matrix_state.grid[1][0].clip.as_ref().map(|c| c.name.as_str()),
            Some("Bass Mel 1")
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn export_to_path_writes_bitwig_zip_with_valid_btwg_and_embedded_samples() {
        use crate::views::matrix::{ClipData, MatrixClip, SlotState};
        use crate::views::playlist::PlaylistState;

        let sample_path = std::env::temp_dir().join(format!(
            "hikaru_btwg_sample_{}.wav",
            std::process::id()
        ));
        std::fs::write(&sample_path, b"RIFF\x24\x00\x00\x00WAVEfmt ").expect("write fake wav");

        let mut src = test_state();
        src.transport.bpm = 150.0;
        src.live_tracks[1].name = "Riddim Drums".to_string();
        src.matrix_state.grid[0][0].state = SlotState::Stopped;
        src.matrix_state.grid[0][0].clip = Some(MatrixClip {
            id: 1,
            name: "Cymatics - Energize".to_string(),
            path: sample_path.clone(),
            duration_secs: 3.2,
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
            loop_end: 141120,
            loop_enabled: true,
            launch_legato: true,
            has_time_selection: false,
            peaks: Vec::new(),
            muted: false,
            color: Some(egui::Color32::from_rgb(0xE0, 0x60, 0x20)),
        });

        let out_path = temp_template_path("zip_btwg_export");
        export_to_path(&src, &out_path).expect("export_to_path");

        let zip_bytes = std::fs::read(&out_path).expect("read exported zip");
        assert_eq!(&zip_bytes[0..4], b"PK\x03\x04", "must be a ZIP archive");

        let entries = dawproject::read_zip_entries(&zip_bytes).expect("parse exported zip");
        assert!(entries.contains_key("template-info.json"));
        assert!(entries.contains_key("view-settings"));
        assert!(entries.contains_key("template.bwproject"));
        assert!(
            entries
                .keys()
                .any(|k| k.starts_with("project/samples/") && k.ends_with(".wav")),
            "exported ZIP must embed audio sample under project/samples/"
        );

        let btwg_bytes = &entries["template.bwproject"];
        assert_eq!(&btwg_bytes[0..16], b"BtWg0003000200b8");
        let body_off = usize::from_str_radix(
            std::str::from_utf8(&btwg_bytes[16..24]).unwrap(),
            16,
        )
        .unwrap();
        assert_eq!(
            btwg_bytes[body_off - 1],
            b'\n',
            "reserved metadata space must end with newline 0x0a before body_offset"
        );

        // Verificar que el `template.bwproject` binario BtWg se parsea de vuelta
        let parsed = bwproject::parse_bwproject_bytes(btwg_bytes).expect("parse generated BtWg");
        assert!((parsed.transport.bpm - 150.0).abs() < 0.001);
        assert!(parsed.tracks.iter().any(|t| t.name == "Riddim Drums"));
        assert!(parsed.clips.iter().any(|c| c.name == "Cymatics - Energize"));

        // Y que `new_from_template` reabre la plantilla ZIP completa en Hikaru
        let mut dst = test_state();
        new_from_template(&mut dst, &out_path).expect("reopen exported zip template");
        assert!((dst.transport.bpm - 150.0).abs() < 0.001);
        assert_eq!(
            dst.matrix_state.grid[0][0].clip.as_ref().map(|c| c.name.as_str()),
            Some("Cymatics - Energize")
        );

        let _ = std::fs::remove_file(&out_path);
        let _ = std::fs::remove_file(&sample_path);
    }
}
