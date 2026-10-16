// Copyright (C) Hikaru Corporation - 2026
// GNU Lesser General Public License v3
// crates/hikaru_plugin_host/tests/block_size_resolution.rs

//! Tests del cálculo de largo de bloque del host VST3.
//!
//! `vst3_host::audio::resolve_block_frames` decide cuántos frames se le piden al
//! processor en cada bloque. Está en el host y no en este crate, pero los tests
//! viven acá a propósito: `vst3-host-patched` NO es miembro del workspace
//! (ver el comentario en el `Cargo.toml` raíz), así que sus tests unitarios no se
//! correrían. Este crate sí es miembro y alcanza la API pública del host.
//!
//! El escenario que motivó la función: Hikaru entrega al host canales a
//! capacidad completa (el scratch de 4096 frames) declarando `block_size` de
//! 1024. Si el largo del bloque se derivara de `channel.len()`, el `min` contra
//! el máximo configurado se comería la mitad del bloque.

use vst3_host::audio::{resolve_block_frames, AudioBuffers};

fn buffers(input_lens: &[usize], output_lens: &[usize], block_size: usize) -> AudioBuffers {
    AudioBuffers {
        inputs: input_lens.iter().map(|n| vec![0.0; *n]).collect(),
        outputs: output_lens.iter().map(|n| vec![0.0; *n]).collect(),
        sample_rate: 48_000.0,
        block_size,
    }
}

/// El caso de Hikaru: scratch a capacidad completa, bloque real más corto.
///
/// Antes esto devolvía 512 en vez de 1024, y los frames 512..1024 nunca llegaban
/// al plugin: la mitad de cada bloque salía como silencio digital.
#[test]
fn oversized_scratch_does_not_truncate_the_block() {
    let b = buffers(&[4096, 4096], &[4096, 4096], 1024);
    assert_eq!(resolve_block_frames(&b, 4096), 1024);
}

/// Un plugin configurado para 512 no puede recibir 1024 aunque el host lo pida.
#[test]
fn never_exceeds_the_configured_maximum() {
    let b = buffers(&[1024, 1024], &[1024, 1024], 1024);
    assert_eq!(resolve_block_frames(&b, 512), 512);
}

/// Un dispositivo que entrega menos muestras de las que declara `block_size`
/// (stream de tamaño variable) no puede dejar `numSamples` apuntando más allá
/// de la entrada realmente copiada.
#[test]
fn short_channels_win_over_a_lying_block_size() {
    let b = buffers(&[256, 256], &[256, 256], 1024);
    assert_eq!(resolve_block_frames(&b, 4096), 256);
}

/// Buffers desparejos: sólo se pueden confiar los canales escritos por completo.
#[test]
fn mismatched_channel_lengths_use_the_shorter_one() {
    let b = buffers(&[1024, 512], &[512, 512], 1024);
    assert_eq!(resolve_block_frames(&b, 4096), 512);
}

/// Sin canales no hay audio que copiar, pero `process()` igual se llama con
/// `numSamples` para que un plugin sin buses de audio siga avanzando su reloj de
/// eventos (MIDI). Devolver 0 lo saltaría por completo.
#[test]
fn a_plugin_with_no_audio_buses_still_advances_time() {
    let b = AudioBuffers::new(0, 0, 512, 48_000.0);
    assert_eq!(resolve_block_frames(&b, 512), 512);
}

/// Regresión del caso "synth sin entrada": un bus de entrada vacío pero con
/// salida real. El mínimo son los canales, no el máximo, así que la señal de
/// salida no debe decidir el largo del bloque.
#[test]
fn output_only_plugin_still_processes_full_block() {
    let b = buffers(&[], &[1024, 1024], 1024);
    assert_eq!(resolve_block_frames(&b, 4096), 1024);
}
