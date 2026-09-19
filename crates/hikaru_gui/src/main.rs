// Copyright (C) Hikaru Corporation - 2026
// GNU Affero General Public License v3
// Hikaru OpenLive - Código fuente del Main
// crates/hikaru_gui/src/main.rs

use std::sync::mpsc::channel;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use hikaru_core::SampleRate;
use hikaru_audio_engine::EngineMode; // <--- Importamos EngineMode
use hikaru_gui::app::HikaruApp;
use hikaru_gui::audio_proxy::{AudioProxy, GuiCommand};

static DEFAULT_WAVETABLE: [f32; 2048] = [0.0; 2048];

fn main() -> eframe::Result<()> {
    #[cfg(target_os = "linux")]
    {
        // Force X11 backend via XWayland — Wayland native causes transparency/blur
        // issues with KDE Plasma compositor and plugin X11 windows render incorrectly.
        std::env::set_var("WINIT_UNIX_BACKEND", "x11");
    }
    
    let (tx, rx) = channel();
    let audio_proxy = AudioProxy::new(tx);

    let sample_rate = SampleRate::new(44100.0);
    
    // 1. Instanciamos el reloj atómico compartido
    let position_clock = Arc::new(AtomicU64::new(0));

    // 2. Se lo pasamos a AudioEngine::new
    let engine = hikaru_audio_engine::AudioEngine::new(
        sample_rate, 
        &DEFAULT_WAVETABLE, 
        position_clock.clone()
    );
    let engine_arc = Arc::new(Mutex::new(engine));

    let engine_for_commands = engine_arc.clone();
    std::thread::spawn(move || {
        set_thread_realtime();
        while let Ok(command) = rx.recv() {
            match command {
                GuiCommand::SetAppMode(is_studio) => {
                    if let Ok(mut engine) = engine_for_commands.lock() {
                        if is_studio {
                            engine.set_mode(EngineMode::OpenStudio);
                        } else {
                            engine.set_mode(EngineMode::OpenLive);
                        }
                    }
                }
                GuiCommand::Play => {
                    if let Ok(mut engine) = engine_for_commands.lock() {
                        engine.play();
                    }
                }
                GuiCommand::Pause => {
                    if let Ok(mut engine) = engine_for_commands.lock() {
                        engine.pause();
                    }
                }
                GuiCommand::Stop => {
                    if let Ok(mut engine) = engine_for_commands.lock() {
                        engine.stop();
                    }
                }
                GuiCommand::Seek { sample_count } => {
                    if let Ok(mut engine) = engine_for_commands.lock() {
                        let secs = sample_count as f32 / engine.sample_rate;
                        engine.seek(secs);
                    }
                }
                GuiCommand::UpdateClipBounds { clip_id, track_index, scene_index, position_secs, duration_secs, offset_secs } => {
                    if let Ok(mut engine) = engine_for_commands.lock() {
                        engine.update_clip_bounds(clip_id, track_index, scene_index, position_secs, duration_secs, offset_secs);
                    }
                }
                GuiCommand::LoadClip { clip_id, path, position_secs, duration_secs, offset_secs, track_index, scene_index } => {
                    println!("[Hikaru Engine] Cargando clip de Matrix/Playlist: {} en Track {}", path, track_index);

                    if let Ok(mut reader) = hound::WavReader::open(&path) {
                        let spec = reader.spec();
                        let file_sr = spec.sample_rate as f32;
                        let channels = spec.channels as usize;

                        let raw_samples: Vec<f32> = match spec.sample_format {
                            hound::SampleFormat::Float => reader.samples::<f32>().filter_map(Result::ok).collect(),
                            hound::SampleFormat::Int => {
                                let max_val = (1 << (spec.bits_per_sample - 1)) as f32;
                                reader.samples::<i32>()
                                    .filter_map(Result::ok)
                                    .map(|s| s as f32 / max_val)
                                    .collect()
                            }
                        };

                        if let Ok(mut engine) = engine_for_commands.lock() {
                            let target_sr = engine.sample_rate;

                            let final_samples = if (file_sr - target_sr).abs() > 1.0 {
                                resample_linear(&raw_samples, channels, file_sr, target_sr)
                            } else {
                                raw_samples
                            };

                            engine.add_clip(
                                clip_id,
                                track_index,
                                scene_index,
                                final_samples,
                                position_secs,
                                duration_secs,
                                offset_secs,
                                channels,
                                true,
                            );
                        }
                    } else {
                        eprintln!("[Hikaru Engine Error] No se pudo abrir el WAV: {}", path);
                    }
                }
                GuiCommand::PreviewSample { path, volume, speed: _ } => {
                    if let Ok(mut engine) = engine_for_commands.lock() {
                        engine.preview_player.stop();
                        engine.preview_player.set_volume(volume);
                    }

                    let engine_clone = engine_for_commands.clone();
                    std::thread::spawn(move || {
                        let decoded = match decode_wav(&path) {
                            Ok(d) => d,
                            Err(e) => {
                                eprintln!("[Hikaru Engine Error] decode failed for {}: {}", path, e);
                                return;
                            }
                        };

                        let channels = decoded.channels;
                        let target_sr = engine_clone
                            .lock()
                            .map(|e| e.sample_rate)
                            .unwrap_or(44100.0);

                        let final_samples = if (decoded.file_sr - target_sr).abs() > 1.0 {
                            resample_linear(&decoded.samples, decoded.channels, decoded.file_sr, target_sr)
                        } else {
                            decoded.samples
                        };

                        if let Ok(mut engine) = engine_clone.lock() {
                            engine.preview_player.play(final_samples, channels);
                        }
                    });
                }
                GuiCommand::StopPreview => {
                    if let Ok(mut engine) = engine_for_commands.lock() {
                        engine.preview_player.stop();
                    }
                }
                GuiCommand::SetPreviewVolume(vol) => {
                    if let Ok(mut engine) = engine_for_commands.lock() {
                        engine.preview_player.set_volume(vol);
                    }
                }

                GuiCommand::SyncPlaylistClips { clips } => {
                    println!("[Hikaru Engine] Sincronizando {} clips de Playlist...", clips.len());

                    let target_sr = if let Ok(engine) = engine_for_commands.try_lock() {
                        engine.sample_rate
                    } else {
                        44100.0
                    };

                    let mut decoded_clips: Vec<(usize, usize, usize, Vec<f32>, f32, f32, f32, usize)> = Vec::with_capacity(clips.len());
                    for clip_data in clips.into_iter() {
                        if let Ok(mut reader) = hound::WavReader::open(&clip_data.path) {
                            let spec = reader.spec();
                            let file_sr = spec.sample_rate as f32;
                            let channels = spec.channels as usize;

                            let raw_samples: Vec<f32> = match spec.sample_format {
                                hound::SampleFormat::Float => reader.samples::<f32>().filter_map(Result::ok).collect(),
                                hound::SampleFormat::Int => {
                                    let max_val = (1 << (spec.bits_per_sample - 1)) as f32;
                                    reader.samples::<i32>()
                                        .filter_map(Result::ok)
                                        .map(|s| s as f32 / max_val)
                                        .collect()
                                }
                            };

                            let final_samples = if (file_sr - target_sr).abs() > 1.0 {
                                resample_linear(&raw_samples, channels, file_sr, target_sr)
                            } else {
                                raw_samples
                            };

                            decoded_clips.push((
                                clip_data.clip_id,
                                clip_data.track_index,
                                0,
                                final_samples,
                                clip_data.start_secs,
                                clip_data.duration_secs,
                                clip_data.offset_secs,
                                channels,
                            ));
                        } else {
                            eprintln!("[Hikaru Engine Error] No se pudo abrir: {}", clip_data.path);
                        }
                    }

                    if let Ok(mut engine) = engine_for_commands.lock() {
                        engine.clips.retain(|c| c.is_matrix_clip);
                        for (clip_id, track_index, scene_index, samples, start, duration, offset, channels) in decoded_clips {
                            engine.add_clip(clip_id, track_index, scene_index, samples, start, duration, offset, channels, false);
                        }
                    }
                }
                GuiCommand::TriggerClip { track_idx, scene_idx } => {
                    if let Ok(mut engine) = engine_for_commands.lock() {
                        engine.trigger_clip(track_idx, scene_idx);
                    }
                }
                GuiCommand::StopTrack { track_idx } => {
                    if let Ok(mut engine) = engine_for_commands.lock() {
                        engine.stop_track(track_idx);
                    }
                }
                GuiCommand::SetClipLoop { track_idx, scene_idx, loop_start_secs, loop_end_secs, enabled } => {
                    if let Ok(mut engine) = engine_for_commands.lock() {
                        engine.set_clip_loop(track_idx, scene_idx, loop_start_secs, loop_end_secs, enabled);
                    }
                }
                GuiCommand::SetGlobalLoop { start_samples, end_samples, enabled } => {
                    if let Ok(mut engine) = engine_for_commands.lock() {
                        engine.set_global_loop(start_samples, end_samples, enabled);
                    }
                }
                GuiCommand::TriggerScene { scene_idx } => {
                    if let Ok(mut engine) = engine_for_commands.lock() {
                        engine.trigger_scene(scene_idx);
                    }
                }
                GuiCommand::SetBpm(bpm) => {
                    if let Ok(mut engine) = engine_for_commands.lock() {
                        engine.transport.set_bpm(bpm as f64);
                    }
                }
                GuiCommand::SetTrackVolume { track_idx, volume_db } => {
                    if let Ok(engine) = engine_for_commands.try_lock() {
                        engine.set_track_volume(track_idx, volume_db);
                    }
                }
                GuiCommand::SetTrackPan { track_idx, pan } => {
                    if let Ok(engine) = engine_for_commands.try_lock() {
                        engine.set_track_pan(track_idx, pan);
                    }
                }
                GuiCommand::SetTrackMute { track_idx, mute } => {
                    if let Ok(engine) = engine_for_commands.try_lock() {
                        engine.set_track_mute(track_idx, mute);
                    }
                }
                GuiCommand::SetTrackSolo { track_idx, solo } => {
                    if let Ok(engine) = engine_for_commands.try_lock() {
                        engine.set_track_solo(track_idx, solo);
                    }
                }
                GuiCommand::SetMasterVolume { volume_db } => {
                    if let Ok(engine) = engine_for_commands.try_lock() {
                        engine.set_master_gain(volume_db);
                    }
                }

                // Los MIDI Clips porongos:
                GuiCommand::UpdateMidiClipNotes { track_idx, scene_idx, notes } => {
                    if let Ok(mut engine) = engine_for_commands.lock() {
                        let converted_notes = notes
                            .into_iter()
                            .map(|(start_tick, pitch, velocity, duration_ticks)| {
                                hikaru_audio_engine::MidiNoteInstance {
                                    start_tick,
                                    pitch,
                                    velocity,
                                    duration_ticks,
                                }
                            })
                            .collect();

                        engine.update_midi_clip(track_idx, scene_idx, converted_notes);
                    }
                }
                // Antes del final `_ => {}`
                _ => {}
            }
        }
    });

    let audio_stream = match init_cpal_stream(engine_arc.clone()) {
        Ok(stream) => {
            println!("[Hikaru] Stream de audio CPAL iniciado correctamente.");
            Some(stream)
        }
        Err(e) => {
            eprintln!("[Hikaru Error] No se pudo iniciar el dispositivo de audio: {}", e);
            None
        }
    };

    let hardware_sr: f32 = engine_arc
        .lock()
        .map(|engine| engine.sample_rate)
        .unwrap_or(44100.0);

    let output_level_bits = engine_arc
        .lock()
        .map(|engine| engine.output_level_bits.clone())
        .unwrap_or_else(|_| std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)));

    use eframe::egui;

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 720.0])
            .with_min_inner_size([800.0, 480.0]),
        #[cfg(target_os = "windows")]
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "Hikaru OpenLive",
        native_options,
        Box::new(move |cc| {
            // FORZAR MODO OSCURO EN EGUI INDEPENDIENTEMENTE DEL TEMA DEL SISTEMA OPERATIVO
            cc.egui_ctx.set_visuals(egui::Visuals::dark());

            let mut app = HikaruApp::new(
                cc,
                audio_proxy,
                audio_stream,
                position_clock,
                output_level_bits,
                Some(engine_arc.clone()),
            );
            app.sync_hardware_sample_rate(hardware_sr);
            Box::new(app)
        }),
    )
}

fn init_cpal_stream(
    engine: Arc<Mutex<hikaru_audio_engine::AudioEngine<'static>>>,
) -> Result<cpal::Stream, Box<dyn std::error::Error>> {
    let hosts = cpal::available_hosts();
    println!("[Hikaru] Hosts disponibles: {:?}", hosts);

    #[cfg(target_os = "linux")]
    let host = cpal::host_from_id(cpal::HostId::PulseAudio)
        .unwrap_or_else(|_| cpal::default_host());

    #[cfg(not(target_os = "linux"))]
    let host = cpal::default_host();

    let devices: Vec<_> = host
        .output_devices()?
        .collect();

    println!("[Hikaru] Dispositivos de salida encontrados: {}", devices.len());
    for (i, d) in devices.iter().enumerate() {
        println!("  [{}] {}", i, d);
    }

    let device = host
        .default_output_device()
        .ok_or("No se encontró dispositivo de salida de audio")?;

    println!("[Hikaru] Dispositivo SELECCIONADO: {}", device);

    let supported_config = device.default_output_config()?;
    let device_format = supported_config.sample_format();
    let device_channels = supported_config.channels();
    let device_sr = supported_config.sample_rate();

    println!(
        "[Hikaru] Config del dispositivo: format={:?}, channels={}, sr={}, buffer={:?}",
        device_format, device_channels, device_sr,
        supported_config.buffer_size()
    );

    let stream_config = cpal::StreamConfig {
        channels: supported_config.channels(),
        sample_rate: supported_config.sample_rate(),
        buffer_size: cpal::BufferSize::Fixed(1024),
    };

    let hardware_sr = device_sr as f32;
    println!("[Hikaru] Hardware SR para engine: {}Hz", hardware_sr);

    if let Ok(mut lock) = engine.lock() {
        lock.set_sample_rate(hardware_sr);
    }

    let engine_cb = engine.clone();
    let ftz_init = std::sync::Once::new();
    let stream = device.build_output_stream(
        stream_config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            ftz_init.call_once(|| {
                hikaru_audio_engine::enable_ftz_daz();
            });
            data.fill(0.0);
            match engine_cb.try_lock() {
                Ok(mut lock) => {
                    let mut buffer = hikaru_core::AudioBuffer { samples: data };
                    lock.process(&mut buffer);
                }
                Err(_) => {
                    data.fill(0.0);
                }
            }
        },
        |err| {
            let err_str = err.to_string();
            if !err_str.contains("underrun") && !err_str.contains("overrun") {
                eprintln!("[Hikaru Error] CPAL stream error: {}", err);
            }
        },
        None,
    )?;

    stream.play()?;
    println!("[Hikaru] Stream CPAL: play() OK — {}", device);

    Ok(stream)
}

struct WavData {
    samples: Vec<f32>,
    file_sr: f32,
    channels: usize,
}

fn decode_wav(path: &str) -> Result<WavData, Box<dyn std::error::Error>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let file_sr = spec.sample_rate as f32;
    let channels = spec.channels as usize;

    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().filter_map(Result::ok).collect(),
        hound::SampleFormat::Int => {
            let max_val = (1 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .filter_map(Result::ok)
                .map(|s| s as f32 / max_val)
                .collect()
        }
    };

    Ok(WavData { samples, file_sr, channels })
}

#[cfg(target_os = "linux")]
fn set_thread_realtime() {
    unsafe {
        let param = libc::sched_param {
            sched_priority: 50,
        };
        let ret = libc::sched_setscheduler(0, libc::SCHED_FIFO, &param as *const libc::sched_param);
        if ret == 0 {
            println!("[Hikaru] Thread configurado en SCHED_FIFO prio=50");
        } else {
            eprintln!("[Hikaru] No se pudo setear SCHED_FIFO (¿permisos?). Usando SCHED_NORMAL.");
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn set_thread_realtime() {
    // No-op en plataformas no-Linux (macOS/Windows usan APIs diferentes).
}

fn resample_linear(samples: &[f32], channels: usize, from_sr: f32, to_sr: f32) -> Vec<f32> {
    if samples.is_empty() || channels == 0 {
        return Vec::new();
    }

    let channels = channels.max(1);
    let input_frames = samples.len() / channels;
    let ratio = from_sr / to_sr;
    let output_frames = ((input_frames as f32) / ratio) as usize;
    let mut output = Vec::with_capacity(output_frames * channels);

    for frame in 0..output_frames {
        let input_pos = frame as f32 * ratio;
        let idx0 = input_pos.floor() as usize;
        let idx1 = (idx0 + 1).min(input_frames.saturating_sub(1));
        let t = input_pos - idx0 as f32;

        for ch in 0..channels {
            let s0 = samples[idx0 * channels + ch];
            let s1 = samples[idx1 * channels + ch];
            output.push(s0 + t * (s1 - s0));
        }
    }

    output
}