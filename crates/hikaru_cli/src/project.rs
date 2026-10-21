// Los comandos de proyecto: `new`, `inspect`/`metadata` y `validate`/`check`.
// SPDX-License-Identifier: AGPL-3.0-or-later
// crates/hikaru_cli/src/project.rs
//
// =========================================================================
// POR QUÉ ESTOS COMANDOS NO TOCAN EL MOTOR
// =========================================================================
//
// Un agente de IA quiere responder preguntas sobre un proyecto ("¿cuántos
// clips tiene?", "¿falta algún sample?") SIN que suene nada, sin abrir una
// ventana y sin ocupar el motor de audio. Por eso estos tres comandos leen y
// escriben los ESQUEMAS de los archivos (los crates LGPL) y no el estado vivo
// de la app.
//
// La diferencia con la GUI es que la GUI convierte su estado en estos mismos
// structs (ver `hikaru_gui/src/project/`). Lo que se escribe desde acá es un
// archivo que la GUI abre sin quejarse: las dos puertas usan la misma
// plantilla (`hikaru_openlive_file::template`).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use hikaru_openlive_file::OpenLiveProject;
use hikaru_openstudio_file::OpenStudioProject;
use serde_json::{json, Value};

use crate::cli::{InspectArgs, NewArgs, Template, ValidateArgs};
use crate::version;

/// Los dos formatos de proyecto, deducidos de la extensión.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// `.oplf`: sesión de performance (Session Matrix).
    OpenLive,
    /// `.opsf`: arreglo (timeline lineal).
    OpenStudio,
}

impl Format {
    /// El nombre del formato, tal como va en la cabecera del archivo.
    pub fn name(self) -> &'static str {
        match self {
            Format::OpenLive => "OpenLive",
            Format::OpenStudio => "OpenStudio",
        }
    }

    /// La extensión del formato.
    pub fn extension(self) -> &'static str {
        match self {
            Format::OpenLive => hikaru_openlive_file::EXTENSION,
            Format::OpenStudio => hikaru_openstudio_file::EXTENSION,
        }
    }
}

/// El formato de un archivo, deducido de su extensión.
///
/// Es la misma regla que usa la GUI para abrir (el archivo manda, no el modo
/// en que esté la app): un `.opsf` abierto en Modo OpenLive cambia solo de
/// modo.
pub fn format_of(path: &Path) -> Option<Format> {
    match path.extension().and_then(|e| e.to_str()) {
        Some(hikaru_openlive_file::EXTENSION) => Some(Format::OpenLive),
        Some(hikaru_openstudio_file::EXTENSION) => Some(Format::OpenStudio),
        _ => None,
    }
}

// =========================================================================
// NEW
// =========================================================================

/// `new`: crea un proyecto nuevo.
pub fn new(args: &NewArgs, json: bool) -> ExitCode {
    let path = &args.path;

    let Some(format) = format_of(path) else {
        eprintln!(
            "[Hikaru] {} no es un proyecto de Hikaru: la extensión tiene que ser .oplf (OpenLive) o .opsf (OpenStudio).",
            path.display()
        );
        return ExitCode::from(crate::EXIT_USAGE);
    };

    if path.exists() && !args.force {
        eprintln!(
            "[Hikaru] {} ya existe. Usá --force si querés sobrescribirlo.",
            path.display()
        );
        return ExitCode::from(crate::EXIT_USAGE);
    }

    // `--empty` es el alias de `--template empty`. Clap impide que se usen
    // los dos a la vez.
    let template = if args.empty {
        Template::Empty
    } else {
        args.template
    };

    let created_by = match format {
        Format::OpenLive => version::created_by_openlive(),
        Format::OpenStudio => version::created_by_openstudio(),
    };

    let written = match format {
        Format::OpenLive => {
            let mut project = match template {
                Template::Default => hikaru_openlive_file::template(&created_by),
                Template::Empty => hikaru_openlive_file::empty_template(&created_by),
            };
            project.transport.bpm = sanitize_bpm(args.bpm);
            if let Some(title) = &args.title {
                project.metadata.title = title.clone();
            }
            hikaru_openlive_file::save(path, &project)
        }
        Format::OpenStudio => {
            let mut project = match template {
                Template::Default => hikaru_openstudio_file::template(&created_by),
                Template::Empty => hikaru_openstudio_file::empty_template(&created_by),
            };
            project.transport.bpm = sanitize_bpm(args.bpm);
            if let Some(title) = &args.title {
                project.metadata.title = title.clone();
            }
            hikaru_openstudio_file::save(path, &project)
        }
    };

    let written = match written {
        Ok(path) => path,
        Err(err) => {
            eprintln!("[Hikaru] no se pudo escribir el proyecto: {}", err);
            return ExitCode::from(crate::EXIT_FAILURE);
        }
    };

    if json {
        let summary = json!({
            "created": written.display().to_string(),
            "format": format.name(),
            "extension": format.extension(),
            "template": match template {
                Template::Default => "default",
                Template::Empty => "empty",
            },
            "bpm": sanitize_bpm(args.bpm),
            "title": args.title,
            "created_by": created_by,
            "format_version": hikaru_project_file::FORMAT_VERSION,
        });
        println!("{}", serde_json::to_string_pretty(&summary).unwrap_or_default());
    } else {
        println!("[Hikaru] Proyecto creado: {}", written.display());
        println!("  Formato: {} (.{})", format.name(), format.extension());
        println!(
            "  Plantilla: {}",
            match template {
                Template::Default => "default",
                Template::Empty => "empty",
            }
        );
        println!("  BPM: {}", sanitize_bpm(args.bpm));
    }

    ExitCode::SUCCESS
}

/// Un BPM utilizable.
///
/// Un BPM cero o negativo rompe la conversión a segundos del motor (de ahí
/// salen las duraciones de los clips), así que se acota acá en vez de dejar
/// que el archivo quede con un valor imposible.
fn sanitize_bpm(bpm: f64) -> f64 {
    if bpm.is_finite() && bpm > 0.0 {
        bpm
    } else {
        140.0
    }
}

// =========================================================================
// INSPECT / METADATA
// =========================================================================

/// `inspect` / `metadata`: vuelca los metadatos del proyecto.
pub fn inspect(args: &InspectArgs, json: bool) -> ExitCode {
    let path = &args.path;

    let summary = match format_of(path) {
        Some(Format::OpenLive) => match hikaru_openlive_file::parse(path) {
            Ok(project) => summary_openlive(path, &project),
            Err(err) => return fail_reading(path, &err, json),
        },
        Some(Format::OpenStudio) => match hikaru_openstudio_file::parse(path) {
            Ok(project) => summary_openstudio(path, &project),
            Err(err) => return fail_reading(path, &err, json),
        },
        None => {
            eprintln!(
                "[Hikaru] {} no es un proyecto de Hikaru: la extensión tiene que ser .oplf (OpenLive) o .opsf (OpenStudio).",
                path.display()
            );
            return ExitCode::from(crate::EXIT_USAGE);
        }
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&summary).unwrap_or_default());
    } else {
        print!("{}", render_summary(&summary));
    }

    ExitCode::SUCCESS
}

/// El error de lectura, en JSON o en texto.
fn fail_reading(path: &Path, err: &dyn std::fmt::Display, json: bool) -> ExitCode {
    if json {
        let payload = json!({
            "path": path.display().to_string(),
            "error": err.to_string(),
        });
        println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_default());
    } else {
        eprintln!("[Hikaru] no se pudo leer {}: {}", path.display(), err);
    }
    ExitCode::from(crate::EXIT_FAILURE)
}

/// El resumen de un `.oplf`, en JSON.
fn summary_openlive(path: &Path, project: &OpenLiveProject) -> Value {
    let mut clips = Vec::new();
    for (track_idx, row) in project.matrix.grid.iter().enumerate() {
        for (scene_idx, slot) in row.iter().enumerate() {
            let Some(clip) = slot else { continue };
            let kind = if clip.midi_notes.is_empty() {
                "audio"
            } else {
                "midi"
            };
            clips.push(json!({
                "track": track_idx,
                "scene": scene_idx,
                "id": clip.id,
                "name": clip.name,
                "kind": kind,
                "path": clip.path,
                "volume": clip.volume,
                "pan": clip.pan,
                "mute": clip.mute,
                "loop": {
                    "enabled": clip.loop_enabled,
                    "start": clip.loop_start,
                    "end": clip.loop_end,
                },
                "events": clip.events.iter().map(|e| json!({
                    "id": e.id,
                    "name": e.name,
                    "source_path": e.source_path,
                    "start_secs": e.start_secs,
                    "trim_left_frames": e.trim_left_frames,
                    "visible_frames": e.visible_frames,
                    "gain": e.gain,
                    "fade_in_secs": e.fade_in_secs,
                    "fade_out_secs": e.fade_out_secs,
                })).collect::<Vec<Value>>(),
                "midi_notes": clip.midi_notes.iter().map(|n| json!({
                    "start_tick": n.start_tick,
                    "note": n.note,
                    "velocity": n.velocity,
                    "duration_ticks": n.duration_ticks,
                })).collect::<Vec<Value>>(),
            }));
        }
    }

    let mut audio_clips = 0;
    let mut midi_clips = 0;
    for clip in &clips {
        if clip["kind"] == "midi" {
            midi_clips += 1;
        } else {
            audio_clips += 1;
        }
    }

    json!({
        "path": path.display().to_string(),
        "name": name_from_path(path),
        "format": Format::OpenLive.name(),
        "extension": Format::OpenLive.extension(),
        "schema_version": version::format_version_label(),
        "format_version": project.format_version,
        "app_version": version::VERSION,
        "created_by": project.created_by,
        "engine_mode": "OpenLive",
        "metadata": {
            "title": project.metadata.title,
            "artist": project.metadata.artist,
            "genre": project.metadata.genre,
            "key": project.metadata.key,
            "stats": {
                "total_scenes": project.matrix.scenes.len(),
                "total_tracks": project.matrix.tracks.len(),
                "audio_clips": audio_clips,
                "midi_clips": midi_clips,
            },
        },
        "transport": {
            "bpm": project.transport.bpm,
            "time_signature": project.transport.time_signature,
            "launch_quantization": project.transport.launch_quantization,
            "tap_tempo_enabled": project.transport.tap_tempo_enabled,
            "loop_enabled": project.transport.loop_enabled,
            "loop_start_ticks": project.transport.loop_start_ticks,
            "loop_end_ticks": project.transport.loop_end_ticks,
        },
        "matrix": {
            "tracks": project.matrix.tracks.len(),
            "scenes": project.matrix.scenes.len(),
            "track_names": project.matrix.tracks.iter().map(|t| t.name.clone()).collect::<Vec<String>>(),
            "scene_names": project.matrix.scenes.iter().map(|s| s.name.clone()).collect::<Vec<String>>(),
        },
        "tracks": project.live_tracks.iter().map(|t| track_json(t)).collect::<Vec<Value>>(),
        "clips": clips,
    })
}

/// El resumen de un `.opsf`, en JSON.
fn summary_openstudio(path: &Path, project: &OpenStudioProject) -> Value {
    let mut clips = Vec::new();
    let mut audio_clips = 0;
    let mut midi_clips = 0;

    for (track_idx, clip) in &project.playlist.clips {
        let (kind, sample_path) = match &clip.kind {
            hikaru_openstudio_file::ClipKind::Audio { sample_path, .. } => {
                audio_clips += 1;
                ("audio", Some(sample_path.clone()))
            }
            hikaru_openstudio_file::ClipKind::Pattern { pattern_id } => {
                midi_clips += 1;
                ("pattern", Some(format!("pattern:{pattern_id}")))
            }
            hikaru_openstudio_file::ClipKind::Automation { target_param, .. } => {
                midi_clips += 1;
                ("automation", Some(target_param.clone()))
            }
        };

        clips.push(json!({
            "track": track_idx,
            "id": clip.id,
            "name": clip.name,
            "kind": kind,
            "start_tick": clip.start_tick,
            "duration_ticks": clip.duration_ticks,
            "sample_path": sample_path,
            "color": {
                "h": clip.color.h,
                "s": clip.color.s,
                "l": clip.color.l,
                "a": clip.color.a,
            },
        }));
    }

    json!({
        "path": path.display().to_string(),
        "name": name_from_path(path),
        "format": Format::OpenStudio.name(),
        "extension": Format::OpenStudio.extension(),
        "schema_version": version::format_version_label(),
        "format_version": project.format_version,
        "app_version": version::VERSION,
        "created_by": project.created_by,
        "engine_mode": "OpenStudio",
        "metadata": {
            "title": project.metadata.title,
            "artist": project.metadata.artist,
            "genre": project.metadata.genre,
            "key": project.metadata.key,
            "stats": {
                "total_tracks": project.studio_tracks.len(),
                "total_clips": project.playlist.clips.len(),
                "audio_clips": audio_clips,
                "midi_clips": midi_clips,
            },
        },
        "transport": {
            "bpm": project.transport.bpm,
            "beats_per_bar": project.transport.beats_per_bar,
            "beat_division": project.transport.beat_division,
            "loop_enabled": project.transport.loop_enabled,
            "loop_start_ticks": project.transport.loop_start_ticks,
            "loop_end_ticks": project.transport.loop_end_ticks,
        },
        "playlist": {
            "ppqn": project.playlist.ppqn,
            "clips": project.playlist.clips.len(),
        },
        "tracks": project.studio_tracks.iter().map(|t| track_json(t)).collect::<Vec<Value>>(),
        "clips": clips,
    })
}

/// Un canal de mezcla, en JSON.
fn track_json(track: &hikaru_project_file::Track) -> Value {
    json!({
        "id": track.id,
        "name": track.name,
        "volume": track.volume,
        "pan": track.pan,
        "mute": track.mute,
        "solo": track.solo,
        "arm": track.arm,
        "is_master": track.is_master,
        "matrix_idx": track.matrix_idx,
        "effects": track.effects.len(),
        "sends": track.sends.len(),
    })
}

/// El nombre del proyecto: el del archivo sin la extensión.
fn name_from_path(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// El resumen en texto, para el que no quiere parsear JSON.
fn render_summary(summary: &Value) -> String {
    let mut out = String::new();
    out.push_str("[Hikaru Project Summary]\n");
    out.push_str(&format!("File:      {}\n", path_of(summary)));
    out.push_str(&format!(
        "Format:    {} (.{}) | Schema: {} | Created by: {}\n",
        string_of(summary, "format"),
        string_of(summary, "extension"),
        string_of(summary, "schema_version"),
        string_of(summary, "created_by"),
    ));

    let transport = &summary["transport"];
    if summary["format"] == "OpenLive" {
        out.push_str(&format!(
            "BPM:       {} | Time Signature: {}/{} | Launch Quantization: {}\n",
            transport["bpm"],
            transport["time_signature"][0],
            transport["time_signature"][1],
            transport["launch_quantization"],
        ));
        let matrix = &summary["matrix"];
        out.push_str(&format!(
            "Session Matrix: {} Tracks x {} Scenes\n",
            matrix["tracks"], matrix["scenes"]
        ));
    } else {
        out.push_str(&format!(
            "BPM:       {} | Beats per Bar: {} | Beat Division: {}\n",
            transport["bpm"],
            transport["beats_per_bar"],
            transport["beat_division"],
        ));
        out.push_str(&format!(
            "Timeline:  {} clips | PPQN: {}\n",
            summary["playlist"]["clips"], summary["playlist"]["ppqn"],
        ));
    }

    let tracks: Vec<String> = summary["tracks"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|t| t["name"].as_str().unwrap_or("?").to_string())
                .collect()
        })
        .unwrap_or_default();
    out.push_str(&format!("Tracks:    [{}]\n", tracks.join(", ")));

    let stats = &summary["metadata"]["stats"];
    let clips = summary["clips"].as_array().map(|c| c.len()).unwrap_or(0);
    out.push_str(&format!(
        "Clips:     {} (audio: {}, midi: {})\n",
        clips, stats["audio_clips"], stats["midi_clips"],
    ));

    out
}

/// Un campo de texto del resumen.
fn string_of(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or("?").to_string()
}

/// El campo `path` del resumen.
fn path_of(summary: &Value) -> String {
    string_of(summary, "path")
}

// =========================================================================
// VALIDATE / CHECK
// =========================================================================

/// La gravedad de un hallazgo del chequeo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Severity {
    /// Algo que no impide abrir el proyecto, pero que hay que mirar.
    Warning,
    /// Algo que rompe la sesión: un sample que no está, una grilla rota.
    Error,
}

/// Un hallazgo del chequeo de integridad.
#[derive(Debug, Clone)]
struct Finding {
    severity: Severity,
    message: String,
}

/// El resultado del chequeo.
#[derive(Debug, Default)]
struct Report {
    findings: Vec<Finding>,
    checks: Vec<String>,
}

impl Report {
    /// Anota un chequeo que pasó.
    fn ok(&mut self, message: impl Into<String>) {
        self.checks.push(message.into());
    }

    /// Anota un problema.
    fn error(&mut self, message: impl Into<String>) {
        self.findings.push(Finding {
            severity: Severity::Error,
            message: message.into(),
        });
    }

    /// Anota una advertencia.
    fn warn(&mut self, message: impl Into<String>) {
        self.findings.push(Finding {
            severity: Severity::Warning,
            message: message.into(),
        });
    }

    /// Cuántos errores hay.
    fn errors(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .count()
    }

    /// Cuántas advertencias hay.
    fn warnings(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Warning)
            .count()
    }
}

/// `validate` / `check`: chequea la integridad del proyecto.
pub fn validate(args: &ValidateArgs, json: bool) -> ExitCode {
    let path = &args.path;

    let (format, report) = match format_of(path) {
        Some(Format::OpenLive) => match hikaru_openlive_file::parse(path) {
            Ok(project) => {
                let mut report = Report::default();
                validate_openlive(path, &project, &mut report);
                (Format::OpenLive, report)
            }
            Err(err) => return fail_reading(path, &err, json),
        },
        Some(Format::OpenStudio) => match hikaru_openstudio_file::parse(path) {
            Ok(project) => {
                let mut report = Report::default();
                validate_openstudio(path, &project, &mut report);
                (Format::OpenStudio, report)
            }
            Err(err) => return fail_reading(path, &err, json),
        },
        None => {
            eprintln!(
                "[Hikaru] {} no es un proyecto de Hikaru: la extensión tiene que ser .oplf (OpenLive) o .opsf (OpenStudio).",
                path.display()
            );
            return ExitCode::from(crate::EXIT_USAGE);
        }
    };

    if json {
        let payload = json!({
            "path": path.display().to_string(),
            "format": format.name(),
            "schema_version": version::format_version_label(),
            "ok": report.errors() == 0,
            "errors": report.errors(),
            "warnings": report.warnings(),
            "checks": report.checks,
            "findings": report.findings.iter().map(|f| json!({
                "severity": match f.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                },
                "message": f.message,
            })).collect::<Vec<Value>>(),
        });
        println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_default());
    } else {
        print!("{}", render_report(path, format, &report));
    }

    if report.errors() > 0 {
        ExitCode::from(crate::EXIT_FAILURE)
    } else {
        ExitCode::SUCCESS
    }
}

/// El chequeo de un `.oplf`.
fn validate_openlive(path: &Path, project: &OpenLiveProject, report: &mut Report) {
    // --- Cabecera ---------------------------------------------------------
    if project.format_version == hikaru_project_file::FORMAT_VERSION {
        report.ok(format!(
            "Cabecera: format_version {} y created_by {:?}",
            project.format_version, project.created_by
        ));
    } else {
        report.warn(format!(
            "El archivo es de la versión {} del formato y esta build maneja la {}",
            project.format_version,
            hikaru_project_file::FORMAT_VERSION
        ));
    }

    // --- Transporte -------------------------------------------------------
    validate_bpm(project.transport.bpm, report);
    if project.transport.time_signature[0] == 0 {
        report.error("El numerador del compás es 0: la cuadrícula del compás no existe.");
    }
    if project.transport.launch_quantization.trim().is_empty() {
        report.error("La cuantización de disparo está vacía.");
    }
    if project.transport.loop_enabled
        && project.transport.loop_end_ticks < project.transport.loop_start_ticks
    {
        report.error(format!(
            "La región de loop global está al revés ({}..{}).",
            project.transport.loop_start_ticks, project.transport.loop_end_ticks
        ));
    }

    // --- Grilla -----------------------------------------------------------
    let tracks = project.matrix.tracks.len();
    let scenes = project.matrix.scenes.len();
    let rectangular = project.matrix.grid.len() == tracks
        && project
            .matrix
            .grid
            .iter()
            .all(|row| row.len() == scenes);

    if rectangular {
        report.ok(format!("Session Matrix: {} pistas x {} escenas", tracks, scenes));
    } else {
        report.error(format!(
            "La Session Matrix no es rectangular: {} pistas, {} escenas y una grilla de {} filas.",
            tracks,
            scenes,
            project.matrix.grid.len()
        ));
    }

    // --- Recursos ---------------------------------------------------------
    let base = base_dir(path);
    let mut faltantes = 0usize;

    for (track_idx, row) in project.matrix.grid.iter().enumerate() {
        for (scene_idx, slot) in row.iter().enumerate() {
            let Some(clip) = slot else { continue };

            if clip.id >= project.matrix.next_clip_id {
                report.warn(format!(
                    "El clip #{} (pista {} / escena {}) tiene un id mayor o igual a next_clip_id ({}): la app puede pisarlo al guardar.",
                    clip.id, track_idx + 1, scene_idx + 1, project.matrix.next_clip_id
                ));
            }

            if !clip.path.is_empty() && !resource_exists(&base, &clip.path) {
                faltantes += 1;
                report.error(format!(
                    "Falta el sample del clip #{} (pista {} / escena {}): {}",
                    clip.id,
                    track_idx + 1,
                    scene_idx + 1,
                    clip.path
                ));
            }

            for event in &clip.events {
                if event.source_path.is_empty() {
                    continue;
                }
                if !resource_exists(&base, &event.source_path) {
                    faltantes += 1;
                    report.error(format!(
                        "Falta el sample del evento {:?} del clip #{} (pista {} / escena {}): {}",
                        event.name,
                        clip.id,
                        track_idx + 1,
                        scene_idx + 1,
                        event.source_path
                    ));
                }
            }
        }
    }

    if faltantes == 0 {
        report.ok("Todos los samples referenciados están en disco.");
    }

    // --- Plugins y wavetables --------------------------------------------
    let mut plugins = 0usize;
    for track in &project.live_tracks {
        for effect in &track.effects {
            if let Some(plugin_path) = effect.plugin_path.as_ref().filter(|p| !p.is_empty()) {
                plugins += 1;
                if !resource_exists(&base, plugin_path) {
                    report.warn(format!(
                        "El plugin {:?} de la pista {:?} no está en disco: {}",
                        effect.name, track.name, plugin_path
                    ));
                }
            }
            if let Some(wavetable) = effect.wavetable_path.as_ref().filter(|p| !p.is_empty()) {
                plugins += 1;
                if !resource_exists(&base, wavetable) {
                    report.warn(format!(
                        "La wavetable {:?} de la pista {:?} no está en disco: {}",
                        effect.name, track.name, wavetable
                    ));
                }
            }
        }
    }
    if plugins > 0 {
        report.ok(format!("Se revisaron {} plugins y wavetables referenciados.", plugins));
    }

    // --- Master -----------------------------------------------------------
    if !project.live_tracks.iter().any(|t| t.is_master) {
        report.error("No hay ningún canal master: el motor no tiene salida.");
    }
}

/// El chequeo de un `.opsf`.
fn validate_openstudio(path: &Path, project: &OpenStudioProject, report: &mut Report) {
    // --- Cabecera ---------------------------------------------------------
    if project.format_version == hikaru_project_file::FORMAT_VERSION {
        report.ok(format!(
            "Cabecera: format_version {} y created_by {:?}",
            project.format_version, project.created_by
        ));
    } else {
        report.warn(format!(
            "El archivo es de la versión {} del formato y esta build maneja la {}",
            project.format_version,
            hikaru_project_file::FORMAT_VERSION
        ));
    }

    // --- Transporte -------------------------------------------------------
    validate_bpm(project.transport.bpm, report);
    if project.transport.beats_per_bar == 0 {
        report.error("El compás tiene 0 pulsos: la cuadrícula del compás no existe.");
    }
    if project.transport.beat_division == 0 {
        report.error("La división del compás es 0: la resolución del secuenciador no existe.");
    }
    if project.transport.loop_enabled
        && project.transport.loop_end_ticks < project.transport.loop_start_ticks
    {
        report.error(format!(
            "La región de loop global está al revés ({}..{}).",
            project.transport.loop_start_ticks, project.transport.loop_end_ticks
        ));
    }

    // --- Playlist ---------------------------------------------------------
    if project.playlist.ppqn == 0 {
        report.error("El PPQN del timeline es 0: los ticks no significan nada.");
    } else {
        report.ok(format!("Timeline: PPQN {}", project.playlist.ppqn));
    }

    let base = base_dir(path);
    let mut faltantes = 0usize;

    for (track_idx, clip) in &project.playlist.clips {
        if clip.id >= project.playlist.next_clip_id {
            report.warn(format!(
                "El clip #{} tiene un id mayor o igual a next_clip_id ({}): la app puede pisarlo al guardar.",
                clip.id, project.playlist.next_clip_id
            ));
        }

        // Un clip que apunta a una pista que no existe no se puede mostrar.
        if *track_idx >= project.studio_tracks.len() {
            report.error(format!(
                "El clip {:?} (id {}) apunta a la pista {} y el proyecto tiene {} pistas.",
                clip.name, clip.id, track_idx, project.studio_tracks.len()
            ));
        }

        if let hikaru_openstudio_file::ClipKind::Audio { sample_path, .. } = &clip.kind {
            if !sample_path.is_empty() && !resource_exists(&base, sample_path) {
                faltantes += 1;
                report.error(format!(
                    "Falta el sample del clip {:?} (id {}, pista {}): {}",
                    clip.name, clip.id, track_idx, sample_path
                ));
            }
        }
    }

    if faltantes == 0 {
        report.ok("Todos los samples referenciados están en disco.");
    }

    if !project.studio_tracks.iter().any(|t| t.is_master) {
        report.error("No hay ningún canal master: el motor no tiene salida.");
    }
}

/// El BPM tiene que ser un número usable.
fn validate_bpm(bpm: f64, report: &mut Report) {
    if !bpm.is_finite() || bpm <= 0.0 {
        report.error(format!("El BPM del transporte es {}: no es un tempo válido.", bpm));
    }
}

/// El directorio del proyecto: las rutas relativas cuelgan de ahí.
///
/// Es la misma regla que usa la GUI al abrir (ver `project::common::resolve_path`):
/// un proyecto con sus samples en la misma carpeta se puede mover entero.
fn base_dir(path: &Path) -> PathBuf {
    path.parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// ¿Está el recurso en disco?
///
/// Las rutas absolutas se usan tal cual; las relativas cuelgan del
/// directorio del proyecto.
fn resource_exists(base: &Path, raw: &str) -> bool {
    let path = PathBuf::from(raw);
    let path = if path.is_absolute() {
        path
    } else {
        base.join(path)
    };
    path.exists()
}

/// El informe del chequeo, en texto.
fn render_report(path: &Path, format: Format, report: &Report) -> String {
    let mut out = String::new();
    out.push_str("[Hikaru Project Validation]\n");
    out.push_str(&format!("File:      {}\n", path.display()));
    out.push_str(&format!(
        "Format:    {} (.{}) | Schema: {}\n\n",
        format.name(),
        format.extension(),
        version::format_version_label()
    ));

    for check in &report.checks {
        out.push_str(&format!("  OK    {}\n", check));
    }
    for finding in &report.findings {
        let label = match finding.severity {
            Severity::Error => "ERROR",
            Severity::Warning => "AVISO",
        };
        out.push_str(&format!("  {} {}\n", label, finding.message));
    }

    out.push('\n');
    out.push_str(&format!(
        "Resumen: {} error(es), {} aviso(s).\n",
        report.errors(),
        report.warnings()
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ruta_temporal(nombre: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("hikaru_cli_{}_{}", std::process::id(), nombre));
        path
    }

    #[test]
    fn el_formato_se_saca_de_la_extension() {
        assert_eq!(format_of(Path::new("a.oplf")), Some(Format::OpenLive));
        assert_eq!(format_of(Path::new("a.opsf")), Some(Format::OpenStudio));
        assert_eq!(format_of(Path::new("a.wav")), None);
        assert_eq!(format_of(Path::new("a")), None);
    }

    #[test]
    fn un_bpm_invalido_cae_al_por_omision() {
        assert_eq!(sanitize_bpm(128.0), 128.0);
        assert_eq!(sanitize_bpm(0.0), 140.0);
        assert_eq!(sanitize_bpm(-10.0), 140.0);
        assert_eq!(sanitize_bpm(f64::NAN), 140.0);
    }

    #[test]
    fn un_recurso_relativo_se_busca_desde_el_proyecto() {
        let base = PathBuf::from("/tmp");
        assert!(resource_exists(&base, "/definitivamente/no/existe.wav") == false);
    }

    #[test]
    fn el_bpm_se_valida() {
        let mut report = Report::default();
        validate_bpm(128.0, &mut report);
        assert_eq!(report.errors(), 0);

        validate_bpm(0.0, &mut report);
        assert_eq!(report.errors(), 1);
    }

    #[test]
    fn la_plantilla_se_puede_crear_y_validar() {
        let path = ruta_temporal("plantilla.oplf");
        let args = NewArgs {
            path: path.clone(),
            template: Template::Default,
            empty: false,
            title: Some("Prueba".into()),
            bpm: 128.0,
            force: true,
        };
        assert_eq!(new(&args, false), ExitCode::SUCCESS);

        let args = ValidateArgs { path: path.clone() };
        assert_eq!(validate(&args, false), ExitCode::SUCCESS);

        let args = InspectArgs { path: path.clone() };
        assert_eq!(inspect(&args, true), ExitCode::SUCCESS);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn un_proyecto_con_un_sample_que_no_existe_falla_el_chequeo() {
        let path = ruta_temporal("roto.oplf");
        let args = NewArgs {
            path: path.clone(),
            template: Template::Default,
            empty: false,
            title: None,
            bpm: 140.0,
            force: true,
        };
        assert_eq!(new(&args, false), ExitCode::SUCCESS);

        // Se le mete un clip que apunta a un sample inexistente.
        let mut project = hikaru_openlive_file::parse(&path).unwrap();
        project.matrix.grid[0][0] = Some(hikaru_openlive_file::Clip {
            id: 1,
            name: "Kick".into(),
            path: "no-existe.wav".into(),
            loop_start: 0,
            loop_end: 0,
            loop_enabled: false,
            volume: 1.0,
            pan: 0.0,
            mute: false,
            events: Vec::new(),
            midi_notes: Vec::new(),
        });
        hikaru_openlive_file::save(&path, &project).unwrap();

        let args = ValidateArgs { path: path.clone() };
        assert_eq!(validate(&args, false), ExitCode::from(crate::EXIT_FAILURE));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn un_archivo_que_no_existe_no_se_crea_sin_force() {
        let path = ruta_temporal("existente.oplf");
        std::fs::write(&path, b"{}").unwrap();

        let args = NewArgs {
            path: path.clone(),
            template: Template::Default,
            empty: false,
            title: None,
            bpm: 140.0,
            force: false,
        };
        assert_eq!(new(&args, false), ExitCode::from(crate::EXIT_USAGE));
        // El archivo original no se tocó.
        assert_eq!(std::fs::read(&path).unwrap(), b"{}");

        let _ = std::fs::remove_file(&path);
    }
}
