// crates/hikaru_mcp/src/project.rs
// SPDX-License-Identifier: AGPL-3.0-or-later

//! El backend de archivos: lee proyectos `.oplf` y `.opsf` y los traduce a
//! las vistas JSON que consumen los agentes.
//!
//! Los dos formatos tienen estructuras distintas (la matriz de pads del modo
//! Live contra la playlist del modo OpenStudio), así que el módulo las
//! traduce a un JSON común: `project_info`, `matrix_grid` y `audit`.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use hikaru_openlive_file::OpenLiveProject;
use hikaru_openstudio_file::OpenStudioProject;

/// Un proyecto cargado, con su formato y de dónde salió.
#[derive(Debug, Clone)]
pub enum LoadedProject {
    /// Un proyecto del modo Live (`.oplf`): pistas, escenas y matriz de pads.
    OpenLive {
        /// De qué archivo se leyó.
        path: PathBuf,
        /// El proyecto en sí.
        project: Box<OpenLiveProject>,
    },
    /// Un proyecto del modo OpenStudio (`.opsf`): pistas y playlist.
    OpenStudio {
        /// De qué archivo se leyó.
        path: PathBuf,
        /// El proyecto en sí.
        project: Box<OpenStudioProject>,
    },
}

impl LoadedProject {
    /// El path del archivo de donde salió el proyecto.
    pub fn path(&self) -> &Path {
        match self {
            LoadedProject::OpenLive { path, .. } => path,
            LoadedProject::OpenStudio { path, .. } => path,
        }
    }

    /// El formato del proyecto: `"openlive"` u `"openstudio"`.
    pub fn format(&self) -> &'static str {
        match self {
            LoadedProject::OpenLive { .. } => "openlive",
            LoadedProject::OpenStudio { .. } => "openstudio",
        }
    }

    /// El resumen del proyecto: metadatos, transporte y pistas.
    pub fn info(&self) -> Value {
        match self {
            LoadedProject::OpenLive { path, project } => json!({
                "formato": "openlive",
                "archivo": path.display().to_string(),
                "format_version": project.format_version,
                "created_by": project.created_by,
                "titulo": project.metadata.title,
                "artista": project.metadata.artist,
                "genero": project.metadata.genre,
                "bpm": project.transport.bpm,
                "time_signature": project.transport.time_signature,
                "pistas": project.live_tracks.len(),
                "escenas": project.matrix.scenes.len(),
                "clips_audio": project.metadata.stats.audio_clips,
                "clips_midi": project.metadata.stats.midi_clips,
                "pistas_detalle": project.live_tracks.iter().map(track_view).collect::<Vec<_>>(),
                "master_volumen": project.dsp_global.master_volume,
            }),
            LoadedProject::OpenStudio { path, project } => json!({
                "formato": "openstudio",
                "archivo": path.display().to_string(),
                "format_version": project.format_version,
                "created_by": project.created_by,
                "titulo": project.metadata.title,
                "artista": project.metadata.artist,
                "genero": project.metadata.genre,
                "bpm": project.transport.bpm,
                "pistas": project.studio_tracks.len(),
                "clips_audio": project.metadata.stats.audio_clips,
                "clips_midi": project.metadata.stats.midi_clips,
                "ppqn": project.playlist.ppqn,
                "pistas_detalle": project.studio_tracks.iter().map(track_view).collect::<Vec<_>>(),
            }),
        }
    }

    /// La matriz de clips.
    ///
    /// En el modo Live es la grilla de pads (filas = pistas, columnas = escenas).
    /// En el modo OpenStudio no hay matriz: lo que hay es una playlist, y se
    /// devuelve como la lista de celdas equivalente para que el agente no
    /// tenga que saber la diferencia.
    pub fn matrix_grid(&self) -> Value {
        match self {
            LoadedProject::OpenLive { project, .. } => {
                let pistas: Vec<Value> = project
                    .matrix
                    .tracks
                    .iter()
                    .enumerate()
                    .map(|(i, track)| {
                        json!({
                            "indice": i,
                            "nombre": track.name,
                            "muted": track.muted,
                            "solo": track.soloed,
                            "volumen": track.volume,
                            "pan": track.pan,
                        })
                    })
                    .collect();

                let escenas: Vec<Value> = project
                    .matrix
                    .scenes
                    .iter()
                    .enumerate()
                    .map(|(i, scene)| json!({ "indice": i, "nombre": scene.name }))
                    .collect();

                let grilla: Vec<Value> = project
                    .matrix
                    .grid
                    .iter()
                    .enumerate()
                    .map(|(indice_pista, fila)| {
                        let celdas: Vec<Value> = fila
                            .iter()
                            .enumerate()
                            .map(|(indice_escena, celda)| match celda {
                                Some(clip) => json!({
                                    "pista": indice_pista,
                                    "escena": indice_escena,
                                    "vacia": false,
                                    "id": clip.id,
                                    "nombre": clip.name,
                                    "ruta": clip.path,
                                    "tipo": if clip.midi_notes.is_empty() { "audio" } else { "midi" },
                                    "volumen": clip.volume,
                                    "pan": clip.pan,
                                    "mute": clip.mute,
                                    "loop": clip.loop_enabled,
                                    "notas_midi": clip.midi_notes.len(),
                                    "eventos": clip.events.len(),
                                }),
                                None => json!({
                                    "pista": indice_pista,
                                    "escena": indice_escena,
                                    "vacia": true,
                                }),
                            })
                            .collect();
                        json!({ "pista": indice_pista, "celdas": celdas })
                    })
                    .collect();

                json!({
                    "formato": "openlive",
                    "pistas": pistas,
                    "escenas": escenas,
                    "grilla": grilla,
                    "proxima_clip_id": project.matrix.next_clip_id,
                })
            }
            LoadedProject::OpenStudio { project, .. } => {
                let celdas: Vec<Value> = project
                    .playlist
                    .clips
                    .iter()
                    .map(|(indice_pista, clip)| {
                        let tipo = match &clip.kind {
                            hikaru_openstudio_file::ClipKind::Pattern { pattern_id } => {
                                json!({ "tipo": "pattern", "pattern_id": pattern_id })
                            }
                            hikaru_openstudio_file::ClipKind::Audio {
                                sample_path,
                                sample_offset_ticks,
                                total_sample_ticks,
                            } => json!({
                                "tipo": "audio",
                                "ruta": sample_path,
                                "offset_ticks": sample_offset_ticks,
                                "ticks_totales": total_sample_ticks,
                            }),
                            hikaru_openstudio_file::ClipKind::Automation {
                                target_param,
                                points,
                            } => json!({
                                "tipo": "automation",
                                "parametro": target_param,
                                "puntos": points.len(),
                            }),
                        };

                        json!({
                            "pista": indice_pista,
                            "id": clip.id,
                            "nombre": clip.name,
                            "inicio_tick": clip.start_tick,
                            "duracion_ticks": clip.duration_ticks,
                            "detalle": tipo,
                        })
                    })
                    .collect();

                json!({
                    "formato": "openstudio",
                    "nota": "el modo OpenStudio no tiene matriz de escenas: la celda equivalente es la playlist",
                    "pistas": project.studio_tracks.iter().map(track_view).collect::<Vec<_>>(),
                    "clipes": celdas,
                    "ppqn": project.playlist.ppqn,
                })
            }
        }
    }

    /// La auditoría del proyecto: lo que un agente tiene que saber antes de
    /// tocarlo.
    ///
    /// Busca tres cosas: recursos que el proyecto referencia y no están en
    /// disco, plugins que el proyecto pide y no aparecen en el host, y buses
    /// de ruteo que apuntan a una pista que no existe.
    pub fn audit(&self, plugins_conocidos: &[String]) -> Value {
        let mut problemas: Vec<Value> = Vec::new();

        match self {
            LoadedProject::OpenLive { path, project } => {
                // 1. Archivos de audio referenciados que no están.
                for (indice_pista, fila) in project.matrix.grid.iter().enumerate() {
                    for (indice_escena, celda) in fila.iter().enumerate() {
                        let Some(clip) = celda else { continue };
                        for evento in &clip.events {
                            if !evento.source_path.is_empty()
                                && !Path::new(&evento.source_path).exists()
                            {
                                problemas.push(json!({
                                    "tipo": "recurso_faltante",
                                    "severidad": "error",
                                    "pista": indice_pista,
                                    "escena": indice_escena,
                                    "clip": clip.name,
                                    "ruta": evento.source_path,
                                    "detalle": "el sample referenciado no existe en disco",
                                }));
                            }
                        }
                    }
                }

                // 2. Plugins pedidos que el host no conoce.
                for pista in &project.live_tracks {
                    for slot in &pista.effects {
                        let Some(ruta) = &slot.plugin_path else { continue };
                        if !plugins_conocidos.iter().any(|p| p == ruta) {
                            problemas.push(json!({
                                "tipo": "plugin_no_encontrado",
                                "severidad": "error",
                                "pista": pista.name,
                                "slot": slot.name,
                                "ruta": ruta,
                                "formato": slot.plugin_format,
                                "detalle": "el proyecto pide este plugin y el host no lo tiene",
                            }));
                        }
                    }
                }

                // 3. Ruteo a una pista que no existe.
                for pista in &project.live_tracks {
                    if !pista.is_master
                        && !project
                            .live_tracks
                            .iter()
                            .any(|t| t.id == pista.route_destination_id)
                    {
                        problemas.push(json!({
                            "tipo": "ruteo_invalido",
                            "severidad": "aviso",
                            "pista": pista.name,
                            "destino": pista.route_destination_id,
                            "detalle": "la pista rutea a un destino que no existe en el proyecto",
                        }));
                    }
                }

                json!({
                    "formato": "openlive",
                    "archivo": path.display().to_string(),
                    "problemas": problemas,
                    "total_problemas": problemas.len(),
                })
            }
            LoadedProject::OpenStudio { path, project } => {
                for (indice_pista, clip) in &project.playlist.clips {
                    if let hikaru_openstudio_file::ClipKind::Audio { sample_path, .. } = &clip.kind {
                        if !sample_path.is_empty() && !Path::new(sample_path).exists() {
                            problemas.push(json!({
                                "tipo": "recurso_faltante",
                                "severidad": "error",
                                "pista": indice_pista,
                                "clip": clip.name,
                                "ruta": sample_path,
                                "detalle": "el sample referenciado no existe en disco",
                            }));
                        }
                    }
                }

                for pista in &project.studio_tracks {
                    for slot in &pista.effects {
                        let Some(ruta) = &slot.plugin_path else { continue };
                        if !plugins_conocidos.iter().any(|p| p == ruta) {
                            problemas.push(json!({
                                "tipo": "plugin_no_encontrado",
                                "severidad": "error",
                                "pista": pista.name,
                                "slot": slot.name,
                                "ruta": ruta,
                                "formato": slot.plugin_format,
                                "detalle": "el proyecto pide este plugin y el host no lo tiene",
                            }));
                        }
                    }
                }

                json!({
                    "formato": "openstudio",
                    "archivo": path.display().to_string(),
                    "problemas": problemas,
                    "total_problemas": problemas.len(),
                })
            }
        }
    }
}

/// La vista de una pista, común a los dos formatos.
fn track_view(track: &hikaru_project_file::Track) -> Value {
    json!({
        "id": track.id,
        "nombre": track.name,
        "volumen": track.volume,
        "pan": track.pan,
        "mute": track.mute,
        "solo": track.solo,
        "arm": track.arm,
        "es_master": track.is_master,
        "destino": track.route_destination_id,
        "envios": track.sends.len(),
        "slots_dsp": track.effects.len(),
        "plugins": track.effects.iter().filter_map(|s| s.plugin_path.as_ref()).collect::<Vec<_>>(),
    })
}

/// Carga un proyecto desde el disco, según la extensión.
pub fn load(path: &Path) -> Result<LoadedProject, String> {
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    match extension.as_str() {
        "oplf" => {
            let project = hikaru_openlive_file::parse(path)
                .map_err(|err| format!("no se pudo leer {}: {err}", path.display()))?;
            Ok(LoadedProject::OpenLive {
                path: path.to_path_buf(),
                project: Box::new(project),
            })
        }
        "opsf" => {
            let project = hikaru_openstudio_file::parse(path)
                .map_err(|err| format!("no se pudo leer {}: {err}", path.display()))?;
            Ok(LoadedProject::OpenStudio {
                path: path.to_path_buf(),
                project: Box::new(project),
            })
        }
        otra => Err(format!(
            "extensión desconocida '{otra}': se esperaba .oplf o .opsf"
        )),
    }
}

/// Busca un proyecto en el directorio actual.
///
/// El agente muchas veces arranca el servidor sin decir qué proyecto quiere:
/// en ese caso se busca uno en el directorio de trabajo, y si hay varios se
/// elige el primero por orden de nombre (determinista).
pub fn discover_in(dir: &Path) -> Option<PathBuf> {
    let mut candidatos: Vec<PathBuf> = Vec::new();

    for patron in ["*.oplf", "*.opsf"] {
        let Ok(entradas) = std::fs::read_dir(dir) else {
            continue;
        };
        for entrada in entradas.flatten() {
            let path = entrada.path();
            if path.is_file() {
                let coincide = path
                    .file_name()
                    .map(|n| {
                        let nombre = n.to_string_lossy();
                        nombre.ends_with(".oplf") || nombre.ends_with(".opsf")
                    })
                    .unwrap_or(false);
                if coincide {
                    candidatos.push(path);
                }
            }
        }
        let _ = patron;
    }

    candidatos.sort();
    candidatos.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_archivo_inexistente_falla_con_mensaje() {
        let err = load(Path::new("/no/existe.oplf")).unwrap_err();
        assert!(err.contains("no se pudo leer"));
    }

    #[test]
    fn una_extension_desconocida_falla_con_mensaje() {
        let err = load(Path::new("/tmp/x.txt")).unwrap_err();
        assert!(err.contains("extensión desconocida"));
    }
}
