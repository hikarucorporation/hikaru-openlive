// Copyright (C) Hikaru Corporation - 2026
// Miyu's Audio Engine
// GNU Affero General Public License v3
// crates/hikaru_audio_engine/src/lib.rs

pub mod preview_player;
pub mod audio_drivers;

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use hikaru_core::{AudioBuffer, SampleRate};
use hikaru_dsp::effects::filter::StateVariableFilter;
use hikaru_dsp::synth::open_grid::{OpenGridPatch, OpenGridVoicePool};
use hikaru_dsp::synth::open_poly::{OpenPolyDspParams, OpenPolyVoicePool, OpenPolyWavetableData};
use hikaru_dsp::synth::sampler::{SamplerDspParams, SamplerSampleData, SamplerVoicePool};
use hikaru_dsp::synth::wavetable::WavetableOscillator;
use hikaru_sequencer::TrackMatrix;
pub use hikaru_sequencer::{ClipLaunchMode, QuantizationEngine, QuantizationGrid};
use hikaru_transport::{TransportPlaybackState, TransportPosition};
use hikaru_plugin_host::{PluginDescriptor, PluginFormat, PluginInstance};

const MAX_TRACK_BUF: usize = 2048 * 2;

// --- Definición de estructuras de clips MIDI ---
pub use hikaru_core::MidiNoteInstance;

#[derive(Clone, Debug)]
pub struct MidiClipInstance {
    pub id: u32,
    pub track_index: usize,
    pub scene_index: usize,
    pub notes: Vec<MidiNoteInstance>,
    pub is_playing: bool,
    pub start_frame: u64,
    pub start_absolute: u64,
    pub duration_ticks: u64,
    pub clip_loop_enabled: bool,
    pub clip_loop_start_ticks: u64,
    pub clip_loop_end_ticks: u64,
    pub is_matrix_clip: bool,
    /// Override explícito de lanzamiento Legato (`None` = default del motor:
    /// legato al cambiar pad, reinicio en 0 al cambiar escena; `Some(true)` =
    /// siempre legato; `Some(false)` = siempre reinicia en 0).
    pub legato_launch: Option<bool>,
    pub launch_mode: ClipLaunchMode,
    pub active_notes: [bool; 128],
}

impl Default for MidiClipInstance {
    fn default() -> Self {
        Self {
            id: 0,
            track_index: 0,
            scene_index: 0,
            notes: Vec::new(),
            is_playing: false,
            start_frame: 0,
            start_absolute: 0,
            duration_ticks: 960 * 4,
            clip_loop_enabled: true,
            clip_loop_start_ticks: 0,
            clip_loop_end_ticks: 960 * 4,
            is_matrix_clip: true,
            legato_launch: None,
            launch_mode: ClipLaunchMode::Trigger,
            active_notes: [false; 128],
        }
    }
}

impl MidiClipInstance {
    pub fn new(track_index: usize, scene_index: usize, notes: Vec<MidiNoteInstance>) -> Self {
        let max_tick = notes.iter().map(|n| n.end_tick()).max().unwrap_or(960 * 4);
        let duration_ticks = max_tick.max(960 * 4);
        Self {
            id: (track_index * 32 + scene_index) as u32,
            track_index,
            scene_index,
            notes,
            is_playing: false,
            start_frame: 0,
            start_absolute: 0,
            duration_ticks,
            clip_loop_enabled: true,
            clip_loop_start_ticks: 0,
            clip_loop_end_ticks: duration_ticks,
            is_matrix_clip: true,
            legato_launch: None,
            launch_mode: ClipLaunchMode::Trigger,
            active_notes: [false; 128],
        }
    }

    pub fn has_valid_clip_loop(&self) -> bool {
        self.clip_loop_enabled && self.clip_loop_end_ticks > self.clip_loop_start_ticks
    }

    pub fn loop_len_ticks(&self) -> u64 {
        if self.has_valid_clip_loop() {
            self.clip_loop_end_ticks - self.clip_loop_start_ticks
        } else {
            self.duration_ticks
        }
    }

    pub fn quantized_loop_len(&self, transport: &TransportPosition) -> u64 {
        transport.ticks_to_samples(self.loop_len_ticks())
    }

    pub fn emission_len_frames(&self, transport: &TransportPosition) -> u64 {
        transport.ticks_to_samples(self.duration_ticks)
    }

    pub fn clear_active_notes(&mut self) {
        self.active_notes = [false; 128];
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
static ORIGINAL_MXCSR: std::sync::OnceLock<u32> = std::sync::OnceLock::new();

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub fn enable_ftz_daz() {
    unsafe {
        let mut mxcsr: u32 = 0;
        std::arch::asm!("stmxcsr [{}]", in(reg) &mut mxcsr as *mut u32, options(nostack));
        let _ = ORIGINAL_MXCSR.set(mxcsr);
        mxcsr |= (1 << 15) | (1 << 6);
        std::arch::asm!("ldmxcsr [{}]", in(reg) &mxcsr as *const u32, options(nostack));
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub fn restore_mxcsr() {
    if let Some(&original) = ORIGINAL_MXCSR.get() {
        unsafe {
            std::arch::asm!("ldmxcsr [{}]", in(reg) &original as *const u32);
        }
    }
}

#[inline(always)]
pub fn flush_denormals(buf: &mut [f32]) {
    for s in buf.iter_mut() {
        let bits = s.to_bits();
        if (bits & 0x7F800000) == 0 && bits != 0 {
            *s = 0.0;
        }
    }
}

use crate::preview_player::PreviewPlayer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineMode {
    OpenLive,
    OpenStudio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceState {
    Active,
    Finished,
}

/// Un evento (región) dentro de un pad de la Session Matrix.
///
/// Cada evento guarda su propio PCM recortado, de modo que un pad puede
/// contener **varios samples apilados o secuenciados**, cada uno con su
/// posición (`clip_start`), ganancia y fundidos. El motor los mezcla por
/// suma en `process()`.
#[derive(Clone, Debug)]
pub struct EngineAudioEvent {
    pub id: u64,
    /// PCM interleaved ya recortado a la región del evento.
    pub samples: Vec<f32>,
    pub channels: usize,
    /// Posición dentro del timeline del clip (frames).
    pub clip_start: u64,
    pub gain: f32,
    pub fade_in: u64,
    pub fade_out: u64,
}

impl EngineAudioEvent {
    pub fn frames(&self) -> u64 {
        (self.samples.len() / self.channels.max(1)) as u64
    }

    pub fn end(&self) -> u64 {
        self.clip_start.saturating_add(self.frames())
    }

    fn gain_at(&self, clip_frame: u64) -> f32 {
        if clip_frame < self.clip_start || clip_frame >= self.end() {
            return 0.0;
        }
        let rel = clip_frame - self.clip_start;
        let len = self.frames();
        let mut g = self.gain;
        if self.fade_in > 0 && rel < self.fade_in {
            g *= rel as f32 / self.fade_in as f32;
        }
        let from_end = len.saturating_sub(rel);
        if self.fade_out > 0 && from_end < self.fade_out {
            g *= from_end as f32 / self.fade_out as f32;
        }
        g
    }

    fn read_at(&self, clip_frame: u64) -> (f32, f32) {
        if clip_frame < self.clip_start || clip_frame >= self.end() {
            return (0.0, 0.0);
        }
        let rel = (clip_frame - self.clip_start) as usize;
        let ch = self.channels.max(1);
        let idx = rel.saturating_mul(ch);
        let l = self.samples.get(idx).copied().unwrap_or(0.0);
        let r = if ch > 1 {
            self.samples.get(idx + 1).copied().unwrap_or(l)
        } else {
            l
        };
        if !l.is_finite() || !r.is_finite() {
            return (0.0, 0.0);
        }
        let g = self.gain_at(clip_frame);
        (l * g, r * g)
    }
}

/// Time-stretching WSOLA (Waveform Similarity Overlap-Add) preservando el pitch original.
/// `rate = target_bpm / source_bpm` (> 1.0 acorta/acelera el clip, < 1.0 lo alarga/desacelera).
pub fn time_stretch_wsola(samples: &[f32], channels: usize, rate: f64) -> Vec<f32> {
    let ch = channels.max(1);
    let in_frames = samples.len() / ch;
    let rate = rate.clamp(0.25, 4.0);
    const GRAIN: usize = 2048;
    const HOP: usize = 1024;
    const SEARCH: isize = 512;

    if (rate - 1.0).abs() < 1e-4 || in_frames <= GRAIN + SEARCH as usize {
        return samples.to_vec();
    }

    let out_frames = ((in_frames as f64) / rate).round().max(1.0) as usize;
    let mut out = vec![0.0f32; out_frames * ch];
    let mut win_sum = vec![0.0f32; out_frames];

    // Ventana Hann de tamaño GRAIN
    let mut win = Vec::with_capacity(GRAIN);
    for i in 0..GRAIN {
        win.push((0.5 * (1.0 - (std::f64::consts::TAU * i as f64 / GRAIN as f64).cos())) as f32);
    }

    // Señal mono auxiliar para correlación cruzada rápida y coherente entre canales
    let mut mono = Vec::with_capacity(in_frames);
    if ch == 1 {
        mono.extend_from_slice(samples);
    } else {
        for f in 0..in_frames {
            let mut sum = 0.0f32;
            for c in 0..ch {
                sum += samples[f * ch + c];
            }
            mono.push(sum / ch as f32);
        }
    }

    let max_src_start = (in_frames - GRAIN) as isize;
    let mut prev_src_start: usize = 0;

    // Grano 0: empieza en 0
    {
        let copy_len = GRAIN.min(out_frames);
        for i in 0..copy_len {
            let w = win[i];
            win_sum[i] += w;
            for c in 0..ch {
                out[i * ch + c] += samples[i * ch + c] * w;
            }
        }
    }

    let mut out_start = HOP;
    while out_start < out_frames {
        let natural_next = prev_src_start + HOP;
        let nom_src_start = (out_start as f64 * rate).round() as isize;
        let cmp_len = HOP.min(in_frames.saturating_sub(natural_next)).min(GRAIN);

        let best_src_start = if cmp_len >= 64 {
            let mut e_ref = 0.0f64;
            let mut i = 0;
            while i < cmp_len {
                let r = mono[natural_next + i] as f64;
                e_ref += r * r;
                i += 4;
            }

            let min_cand = (nom_src_start - SEARCH).max(0);
            let max_cand = (nom_src_start + SEARCH).min(max_src_start);

            let score_cand = |cand: usize, step: usize| -> f64 {
                let mut dot = 0.0f64;
                let mut e_cand = 0.0f64;
                let mut idx = 0;
                while idx < cmp_len {
                    let r = mono[natural_next + idx] as f64;
                    let s = mono[cand + idx] as f64;
                    dot += r * s;
                    e_cand += s * s;
                    idx += step;
                }
                let denom = (e_ref * e_cand).sqrt();
                if denom > 1e-12 { dot / denom } else { -1.0 }
            };

            // Búsqueda gruesa paso 4
            let mut best_cand = nom_src_start.clamp(0, max_src_start) as usize;
            let mut best_score = f64::NEG_INFINITY;
            let mut cand = min_cand;
            while cand <= max_cand {
                let sc = score_cand(cand as usize, 4);
                if sc > best_score {
                    best_score = sc;
                    best_cand = cand as usize;
                }
                cand += 4;
            }

            // Refinamiento fino paso 1 alrededor del mejor candidato
            let fine_lo = (best_cand as isize - 4).max(min_cand);
            let fine_hi = (best_cand as isize + 4).min(max_cand);
            let mut fine_best = best_cand;
            let mut fine_score = f64::NEG_INFINITY;
            for c_pos in fine_lo..=fine_hi {
                let sc = score_cand(c_pos as usize, 2);
                if sc > fine_score {
                    fine_score = sc;
                    fine_best = c_pos as usize;
                }
            }
            fine_best
        } else {
            nom_src_start.clamp(0, max_src_start) as usize
        };

        let copy_len = GRAIN.min(out_frames - out_start);
        for i in 0..copy_len {
            let w = win[i];
            win_sum[out_start + i] += w;
            let src_idx = (best_src_start + i) * ch;
            let dst_idx = (out_start + i) * ch;
            for c in 0..ch {
                out[dst_idx + c] += samples[src_idx + c] * w;
            }
        }

        prev_src_start = best_src_start;
        out_start += HOP;
    }

    for f in 0..out_frames {
        let ws = win_sum[f];
        if ws > 1e-6 {
            let inv = 1.0 / ws;
            for c in 0..ch {
                out[f * ch + c] *= inv;
            }
        }
    }

    out
}

fn sanitize_warp_markers(warp_markers: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut pts: Vec<(f64, f64)> = warp_markers
        .iter()
        .copied()
        .filter(|(o, s)| o.is_finite() && s.is_finite())
        .map(|(o, s)| (o.clamp(0.0, 1.0), s.clamp(0.0, 1.0)))
        .collect();
    pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    if pts.is_empty() || pts[0].0 > 1e-6 || pts[0].1 > 1e-6 {
        pts.insert(0, (0.0, 0.0));
    } else {
        pts[0] = (0.0, 0.0);
    }
    if pts.last().map_or(true, |&(o, s)| o < 1.0 - 1e-6 || s < 1.0 - 1e-6) {
        pts.push((1.0, 1.0));
    } else if let Some(last) = pts.last_mut() {
        *last = (1.0, 1.0);
    }
    // Garantizar monotonicidad estricta en `out_norm` y no decreciente en `src_norm`.
    let mut clean: Vec<(f64, f64)> = Vec::with_capacity(pts.len());
    for (o, s) in pts {
        if let Some(prev) = clean.last().copied() {
            if o <= prev.0 + 1e-6 {
                continue;
            }
            clean.push((o, s.max(prev.1)));
        } else {
            clean.push((o, s));
        }
    }
    if let Some(last) = clean.last_mut() {
        *last = (1.0, 1.0);
    }
    clean
}

#[inline]
fn warp_out_to_src_norm(markers: &[(f64, f64)], out_norm: f64) -> f64 {
    let u = out_norm.clamp(0.0, 1.0);
    for w in markers.windows(2) {
        if u <= w[1].0 {
            let span = (w[1].0 - w[0].0).max(1e-9);
            let t = ((u - w[0].0) / span).clamp(0.0, 1.0);
            return (w[0].1 + t * (w[1].1 - w[0].1)).clamp(0.0, 1.0);
        }
    }
    u
}

/// Time-stretching WSOLA con marcadores de warp por tramos `(out_norm, src_norm)` en `[0.0..1.0]`.
/// Mantiene un único bucle OLA continuo de Hann para evitar clics o discontinuidades entre segmentos.
pub fn time_stretch_wsola_warped(
    samples: &[f32],
    channels: usize,
    total_out_frames: usize,
    warp_markers: &[(f64, f64)],
) -> Vec<f32> {
    let ch = channels.max(1);
    let in_frames = samples.len() / ch;
    if in_frames == 0 || total_out_frames == 0 {
        return Vec::new();
    }
    let clean = sanitize_warp_markers(warp_markers);
    if clean.len() <= 2 {
        let rate = (in_frames as f64 / total_out_frames as f64).clamp(0.25, 4.0);
        return time_stretch_wsola(samples, ch, rate);
    }

    const GRAIN: usize = 2048;
    const HOP: usize = 1024;
    const SEARCH: isize = 512;

    if in_frames <= GRAIN + SEARCH as usize {
        // Clips muy cortos (one-shots): interpolación lineal por tramo.
        let mut out = vec![0.0f32; total_out_frames * ch];
        for f in 0..total_out_frames {
            let u = f as f64 / total_out_frames.max(1) as f64;
            let src_pos = warp_out_to_src_norm(&clean, u) * (in_frames.saturating_sub(1)) as f64;
            let i0 = (src_pos.floor() as usize).min(in_frames - 1);
            let i1 = (i0 + 1).min(in_frames - 1);
            let frac = (src_pos - i0 as f64) as f32;
            for c in 0..ch {
                let a = samples[i0 * ch + c];
                let b = samples[i1 * ch + c];
                out[f * ch + c] = a + (b - a) * frac;
            }
        }
        return out;
    }

    let mut out = vec![0.0f32; total_out_frames * ch];
    let mut win_sum = vec![0.0f32; total_out_frames];

    let mut win = Vec::with_capacity(GRAIN);
    for i in 0..GRAIN {
        win.push((0.5 * (1.0 - (std::f64::consts::TAU * i as f64 / GRAIN as f64).cos())) as f32);
    }

    let mut mono = Vec::with_capacity(in_frames);
    if ch == 1 {
        mono.extend_from_slice(samples);
    } else {
        for f in 0..in_frames {
            let mut sum = 0.0f32;
            for c in 0..ch {
                sum += samples[f * ch + c];
            }
            mono.push(sum / ch as f32);
        }
    }

    let max_src_start = (in_frames - GRAIN) as isize;
    let mut prev_src_start: usize = 0;

    {
        let copy_len = GRAIN.min(total_out_frames);
        for i in 0..copy_len {
            let w = win[i];
            win_sum[i] += w;
            for c in 0..ch {
                out[i * ch + c] += samples[i * ch + c] * w;
            }
        }
    }

    let mut out_start = HOP;
    while out_start < total_out_frames {
        let natural_next = prev_src_start + HOP;
        let u = out_start as f64 / total_out_frames as f64;
        let nom_src_start = (warp_out_to_src_norm(&clean, u) * in_frames as f64).round() as isize;
        let cmp_len = HOP.min(in_frames.saturating_sub(natural_next)).min(GRAIN);

        let best_src_start = if cmp_len >= 64 {
            let mut e_ref = 0.0f64;
            let mut i = 0;
            while i < cmp_len {
                let r = mono[natural_next + i] as f64;
                e_ref += r * r;
                i += 4;
            }

            let min_cand = (nom_src_start - SEARCH).max(0);
            let max_cand = (nom_src_start + SEARCH).min(max_src_start);

            let score_cand = |cand: usize, step: usize| -> f64 {
                let mut dot = 0.0f64;
                let mut e_cand = 0.0f64;
                let mut idx = 0;
                while idx < cmp_len {
                    let r = mono[natural_next + idx] as f64;
                    let s = mono[cand + idx] as f64;
                    dot += r * s;
                    e_cand += s * s;
                    idx += step;
                }
                let denom = (e_ref * e_cand).sqrt();
                if denom > 1e-12 { dot / denom } else { -1.0 }
            };

            let mut best_cand = nom_src_start.clamp(0, max_src_start) as usize;
            let mut best_score = f64::NEG_INFINITY;
            let mut cand = min_cand;
            while cand <= max_cand {
                let sc = score_cand(cand as usize, 4);
                if sc > best_score {
                    best_score = sc;
                    best_cand = cand as usize;
                }
                cand += 4;
            }

            let fine_lo = (best_cand as isize - 4).max(min_cand);
            let fine_hi = (best_cand as isize + 4).min(max_cand);
            let mut fine_best = best_cand;
            let mut fine_score = f64::NEG_INFINITY;
            for c_pos in fine_lo..=fine_hi {
                let sc = score_cand(c_pos as usize, 2);
                if sc > fine_score {
                    fine_score = sc;
                    fine_best = c_pos as usize;
                }
            }
            fine_best
        } else {
            nom_src_start.clamp(0, max_src_start) as usize
        };

        let copy_len = GRAIN.min(total_out_frames - out_start);
        for i in 0..copy_len {
            let w = win[i];
            win_sum[out_start + i] += w;
            let src_idx = (best_src_start + i) * ch;
            let dst_idx = (out_start + i) * ch;
            for c in 0..ch {
                out[dst_idx + c] += samples[src_idx + c] * w;
            }
        }

        prev_src_start = best_src_start;
        out_start += HOP;
    }

    for f in 0..total_out_frames {
        let ws = win_sum[f];
        if ws > 1e-6 {
            let inv = 1.0 / ws;
            for c in 0..ch {
                out[f * ch + c] *= inv;
            }
        }
    }

    out
}

/// FFT radix-2 in-place (Cooley-Tukey) sobre pares `(re, im)`. `buf.len()` debe ser potencia de 2.
fn fft_radix2(buf: &mut [(f32, f32)], inverse: bool) {
    let n = buf.len();
    if n <= 1 {
        return;
    }
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            buf.swap(i, j);
        }
    }
    let sign = if inverse { 1.0f32 } else { -1.0f32 };
    let mut len = 2usize;
    while len <= n {
        let ang = sign * std::f32::consts::TAU / len as f32;
        let (wlen_sin, wlen_cos) = ang.sin_cos();
        let half = len >> 1;
        let mut i = 0;
        while i < n {
            let (mut w_re, mut w_im) = (1.0f32, 0.0f32);
            for k in 0..half {
                let u = buf[i + k];
                let v = buf[i + k + half];
                let t_re = v.0 * w_re - v.1 * w_im;
                let t_im = v.0 * w_im + v.1 * w_re;
                buf[i + k] = (u.0 + t_re, u.1 + t_im);
                buf[i + k + half] = (u.0 - t_re, u.1 - t_im);
                let nw_re = w_re * wlen_cos - w_im * wlen_sin;
                let nw_im = w_re * wlen_sin + w_im * wlen_cos;
                w_re = nw_re;
                w_im = nw_im;
            }
            i += len;
        }
        len <<= 1;
    }
    if inverse {
        let inv_n = 1.0 / n as f32;
        for s in buf.iter_mut() {
            s.0 *= inv_n;
            s.1 *= inv_n;
        }
    }
}

/// Desplaza la envolvente espectral (formantes) mediante STFT con ventana de Hann,
/// preservando el pitch fundamental y la fase.
pub fn apply_formant_curve_stft(
    samples: &[f32],
    channels: usize,
    formant_curve: &hikaru_core::ExpressionCurve,
) -> Vec<f32> {
    let ch = channels.max(1);
    let total_frames = samples.len() / ch;
    if total_frames == 0 || !formant_curve.is_active(0.0) {
        return samples.to_vec();
    }
    const N: usize = 1024;
    const HOP: usize = 512;
    const HALF: usize = N / 2;
    const SMOOTH_RAD: usize = 6;

    let mut win = vec![0.0f32; N];
    for i in 0..N {
        win[i] = 0.5 * (1.0 - (std::f32::consts::TAU * i as f32 / N as f32).cos());
    }

    let mut out = vec![0.0f32; samples.len()];
    let mut win_sum = vec![0.0f32; total_frames];
    let mut fft_buf = vec![(0.0f32, 0.0f32); N];
    let mut log_mag = vec![0.0f32; HALF + 1];
    let mut env = vec![0.0f32; HALF + 1];

    let mut frame_start = 0usize;
    while frame_start < total_frames {
        let u = (frame_start as f64 + (N as f64 * 0.5)) / total_frames.max(1) as f64;
        let formant_st = formant_curve.evaluate(u.clamp(0.0, 1.0), 0.0).clamp(-24.0, 24.0);
        let copy_len = N.min(total_frames - frame_start);

        if formant_st.abs() < 0.02 {
            for i in 0..copy_len {
                let w = win[i];
                win_sum[frame_start + i] += w;
                for c in 0..ch {
                    out[(frame_start + i) * ch + c] += samples[(frame_start + i) * ch + c] * w;
                }
            }
            frame_start += HOP;
            continue;
        }

        let ratio = 2.0f32.powf(formant_st / 12.0);
        for i in 0..copy_len {
            win_sum[frame_start + i] += win[i] * win[i];
        }

        for c in 0..ch {
            for i in 0..N {
                let s = if i < copy_len {
                    samples[(frame_start + i) * ch + c] * win[i]
                } else {
                    0.0
                };
                fft_buf[i] = (s, 0.0);
            }
            fft_radix2(&mut fft_buf, false);

            for k in 0..=HALF {
                let (re, im) = fft_buf[k];
                log_mag[k] = (re * re + im * im).sqrt().max(1e-6).ln();
            }
            for k in 0..=HALF {
                let lo = k.saturating_sub(SMOOTH_RAD);
                let hi = (k + SMOOTH_RAD).min(HALF);
                let sum: f32 = log_mag[lo..=hi].iter().sum();
                env[k] = sum / (hi - lo + 1) as f32;
            }

            for k in 1..HALF {
                let src_k = k as f32 / ratio;
                let warped_env = if src_k <= 0.0 {
                    env[0]
                } else if src_k >= HALF as f32 {
                    env[HALF]
                } else {
                    let k0 = src_k.floor() as usize;
                    let k1 = (k0 + 1).min(HALF);
                    let frac = src_k - k0 as f32;
                    env[k0] + (env[k1] - env[k0]) * frac
                };
                let gain = (warped_env - env[k]).exp().clamp(0.125, 6.0);
                fft_buf[k].0 *= gain;
                fft_buf[k].1 *= gain;
                fft_buf[N - k] = (fft_buf[k].0, -fft_buf[k].1);
            }

            fft_radix2(&mut fft_buf, true);
            for i in 0..copy_len {
                let w = win[i];
                out[(frame_start + i) * ch + c] += fft_buf[i].0 * w;
            }
        }

        frame_start += HOP;
    }

    for f in 0..total_frames {
        let ws = win_sum[f];
        if ws > 1e-6 {
            let inv = 1.0 / ws;
            for c in 0..ch {
                let v = out[f * ch + c] * inv;
                out[f * ch + c] = if v.is_finite() { v } else { 0.0 };
            }
        }
    }
    out
}

/// Time-stretching WSOLA combinado con marcadores de warp, curva continua de `Pitch` (en semitonos)
/// y curva opcional de `Formant` (en semitonos), manteniendo la duración exacta `total_out_frames`.
pub fn time_stretch_and_pitch_wsola(
    samples: &[f32],
    channels: usize,
    total_out_frames: usize,
    warp_markers: &[(f64, f64)],
    pitch_curve: &hikaru_core::ExpressionCurve,
    formant_curve: &hikaru_core::ExpressionCurve,
) -> Vec<f32> {
    let ch = channels.max(1);
    let in_frames = samples.len() / ch;
    if in_frames == 0 || total_out_frames == 0 {
        return Vec::new();
    }

    let has_pitch = pitch_curve.is_active(0.0);
    let has_formant = formant_curve.is_active(0.0);
    let clean = sanitize_warp_markers(warp_markers);

    if !has_pitch {
        let base = if clean.len() > 2 || in_frames != total_out_frames {
            time_stretch_wsola_warped(samples, ch, total_out_frames, &clean)
        } else {
            samples.to_vec()
        };
        return if has_formant {
            apply_formant_curve_stft(&base, ch, formant_curve)
        } else {
            base
        };
    }

    const GRAIN: usize = 2048;
    const HOP: usize = 1024;
    const SEARCH: isize = 512;

    if in_frames <= GRAIN + SEARCH as usize {
        // Clips cortos (one-shots): acumulador de fase con curva de pitch y warp.
        let mut out = vec![0.0f32; total_out_frames * ch];
        let mut src_pos = 0.0f64;
        let base_step = in_frames as f64 / total_out_frames.max(1) as f64;
        for f in 0..total_out_frames {
            let u = f as f64 / total_out_frames.max(1) as f64;
            let st = pitch_curve.evaluate(u, 0.0).clamp(-24.0, 24.0) as f64;
            let pitch_ratio = 2.0f64.powf(st / 12.0);
            let clamped = src_pos.clamp(0.0, (in_frames - 1) as f64);
            let i0 = clamped.floor() as usize;
            let i1 = (i0 + 1).min(in_frames - 1);
            let frac = (clamped - i0 as f64) as f32;
            for c in 0..ch {
                let a = samples[i0 * ch + c];
                let b = samples[i1 * ch + c];
                out[f * ch + c] = a + (b - a) * frac;
            }
            src_pos += base_step * pitch_ratio;
        }
        return if has_formant {
            apply_formant_curve_stft(&out, ch, formant_curve)
        } else {
            out
        };
    }

    let mut out = vec![0.0f32; total_out_frames * ch];
    let mut win_sum = vec![0.0f32; total_out_frames];
    let mut win = Vec::with_capacity(GRAIN);
    for i in 0..GRAIN {
        win.push((0.5 * (1.0 - (std::f64::consts::TAU * i as f64 / GRAIN as f64).cos())) as f32);
    }

    let mut mono = Vec::with_capacity(in_frames);
    if ch == 1 {
        mono.extend_from_slice(samples);
    } else {
        for f in 0..in_frames {
            let mut sum = 0.0f32;
            for c in 0..ch {
                sum += samples[f * ch + c];
            }
            mono.push(sum / ch as f32);
        }
    }

    let max_src_start = (in_frames - GRAIN) as isize;
    let mut prev_src_start: usize = 0;
    let half_grain = GRAIN as f64 * 0.5;
    let max_frame_f = (in_frames - 1) as f64;

    let mut out_start = 0usize;
    while out_start < total_out_frames {
        let u = (out_start as f64 + half_grain) / total_out_frames.max(1) as f64;
        let nom_src_start = (warp_out_to_src_norm(&clean, out_start as f64 / total_out_frames.max(1) as f64)
            * in_frames as f64)
            .round() as isize;

        let best_src_start = if out_start == 0 {
            0usize
        } else {
            let natural_next = prev_src_start + HOP;
            let cmp_len = HOP.min(in_frames.saturating_sub(natural_next)).min(GRAIN);
            if cmp_len >= 64 {
                let mut e_ref = 0.0f64;
                let mut i = 0;
                while i < cmp_len {
                    let r = mono[natural_next + i] as f64;
                    e_ref += r * r;
                    i += 4;
                }
                let min_cand = (nom_src_start - SEARCH).max(0);
                let max_cand = (nom_src_start + SEARCH).min(max_src_start);
                let score_cand = |cand: usize, step: usize| -> f64 {
                    let mut dot = 0.0f64;
                    let mut e_cand = 0.0f64;
                    let mut idx = 0;
                    while idx < cmp_len {
                        let r = mono[natural_next + idx] as f64;
                        let s = mono[cand + idx] as f64;
                        dot += r * s;
                        e_cand += s * s;
                        idx += step;
                    }
                    let denom = (e_ref * e_cand).sqrt();
                    if denom > 1e-12 { dot / denom } else { -1.0 }
                };
                let mut best_cand = nom_src_start.clamp(0, max_src_start) as usize;
                let mut best_score = f64::NEG_INFINITY;
                let mut cand = min_cand;
                while cand <= max_cand {
                    let sc = score_cand(cand as usize, 4);
                    if sc > best_score {
                        best_score = sc;
                        best_cand = cand as usize;
                    }
                    cand += 4;
                }
                let fine_lo = (best_cand as isize - 4).max(min_cand);
                let fine_hi = (best_cand as isize + 4).min(max_cand);
                let mut fine_best = best_cand;
                let mut fine_score = f64::NEG_INFINITY;
                for c_pos in fine_lo..=fine_hi {
                    let sc = score_cand(c_pos as usize, 2);
                    if sc > fine_score {
                        fine_score = sc;
                        fine_best = c_pos as usize;
                    }
                }
                fine_best
            } else {
                nom_src_start.clamp(0, max_src_start) as usize
            }
        };

        let st = pitch_curve.evaluate(u.clamp(0.0, 1.0), 0.0).clamp(-24.0, 24.0) as f64;
        let pitch_ratio = 2.0f64.powf(st / 12.0);
        let grain_center = best_src_start as f64 + half_grain;

        let copy_len = GRAIN.min(total_out_frames - out_start);
        for i in 0..copy_len {
            let w = win[i];
            win_sum[out_start + i] += w;
            let src_pos = (grain_center + (i as f64 - half_grain) * pitch_ratio).clamp(0.0, max_frame_f);
            let i0 = src_pos.floor() as usize;
            let i1 = (i0 + 1).min(in_frames - 1);
            let frac = (src_pos - i0 as f64) as f32;
            let dst_idx = (out_start + i) * ch;
            for c in 0..ch {
                let a = samples[i0 * ch + c];
                let b = samples[i1 * ch + c];
                out[dst_idx + c] += (a + (b - a) * frac) * w;
            }
        }

        prev_src_start = best_src_start;
        out_start += HOP;
    }

    for f in 0..total_out_frames {
        let ws = win_sum[f];
        if ws > 1e-6 {
            let inv = 1.0 / ws;
            for c in 0..ch {
                out[f * ch + c] *= inv;
            }
        }
    }

    if has_formant {
        apply_formant_curve_stft(&out, ch, formant_curve)
    } else {
        out
    }
}

#[derive(Clone, Debug)]
pub struct StretchJob {
    pub clip_id: usize,
    pub samples: Vec<f32>,
    pub channels: usize,
    pub rate: f64,
    pub warp_markers: Vec<(f64, f64)>,
    pub pitch_curve: hikaru_core::ExpressionCurve,
    pub formant_curve: hikaru_core::ExpressionCurve,
}

#[derive(Clone, Debug)]
pub struct AudioClipInstance {
    pub id: usize,
    pub track_index: usize,
    pub scene_index: usize,
    pub samples: Vec<f32>,
    pub start_frame: u64,
    pub start_absolute: u64,
    pub duration_frames: u64,
    pub sample_offset: usize,
    pub channels: usize,
    pub is_playing: bool,
    pub clip_loop_enabled: bool,
    pub clip_loop_start: u64,
    pub clip_loop_end: u64,
    pub is_matrix_clip: bool,
    /// Override explícito de lanzamiento Legato (`None` = default del motor,
    /// `Some(true)` = hereda fase en pad y escena, `Some(false)` = reinicia en 0).
    pub legato_launch: Option<bool>,
    pub launch_mode: ClipLaunchMode,
    /// Ganancia lineal del clip (`0.0..=2.0`, default `1.0` = 0 dB).
    pub gain: f32,
    pub prev_frame: usize,
    pub xfade_remaining: u32,
    pub xfade_len: u32,
    /// Eventos editables del pad. Vacío = modo legacy (un solo `samples`).
    /// No vacío = el pad mezcla estos eventos (cortar/pegar/multi-sample).
    pub events: Vec<EngineAudioEvent>,
    /// BPM original del sample (si se conoce), para sincronización en tiempo real con el transporte.
    pub source_bpm: Option<f32>,
    /// Cache del audio estirado con WSOLA para evitar artefactos de comb filter.
    pub stretched_samples: Vec<f32>,
    pub stretched_rate: f64,
    /// Marcadores de warp por tramos `(out_norm, src_norm)` en `[0.0..1.0]`.
    pub warp_markers: Vec<(f64, f64)>,
    pub stretched_warp_markers: Vec<(f64, f64)>,
    /// Curvas de expresión del clip (Gain, Pan, Pitch, Formant).
    pub expressions: hikaru_core::AudioClipExpressions,
    pub stretched_pitch: Vec<hikaru_core::ExpressionPoint>,
    pub stretched_formant: Vec<hikaru_core::ExpressionPoint>,
}

impl AudioClipInstance {
    pub fn natural_frames(&self) -> u64 {
        if !self.events.is_empty() {
            self.events.iter().map(|e| e.end()).max().unwrap_or(0)
        } else {
            (self.samples.len() / self.channels.max(1)) as u64
        }
    }

    pub fn has_valid_clip_loop(&self) -> bool {
        self.clip_loop_enabled
            && self.clip_loop_end > self.clip_loop_start
            && self.clip_loop_start < self.natural_frames()
    }

    /// Longitud del loop EXPRESSADA en frames, sin tocar.
    fn raw_loop_len(&self) -> u64 {
        let natural = self.natural_frames();
        let end = self.clip_loop_end.min(natural.max(1)).max(self.clip_loop_start + 1);
        end.saturating_sub(self.clip_loop_start).max(1)
    }

    /// Longitud del loop CUANTIZADA a un número entero de beats del transporte.
    ///
    /// Por qué hace falta: `natural_frames` sale de `samples.len()/channels`, y
    /// `samples` viene de `resample_linear`, que calcula los frames con float y
    /// TRUNCA (`as usize`). A 150 BPM la duración real de un WAV cae en un
    /// número de frames que casi nunca es múltiplo de `samples_per_beat`. Con
    /// esa longitud, el wrap del loop va cayendo fuera de la grilla de compases
    /// en cada vuelta: el clip arranca desfasado respecto del pulso (phase
    /// drift), y como cada clip tiene una longitud de WAV distinta, dos clips
    /// que deberían sonar juntos se desfasan entre sí después de la primera
    /// vuelta.
    ///
    /// Redondeando HACIA ABAJO al múltiplo de beat más cercano, dos clips que
    /// duran lo mismo en beats obtienen la misma longitud de loop, así que
    /// vuelven a envolver en el mismo instante y el sync entre ellos se
    /// conserva.
    ///
    /// Todo el cálculo es ENTERO a propósito: `samples_per_beat` se redondea una
    /// sola vez a `u64` y de ahí en adelante se dividen frames, nunca se suman
    /// floats. Un readout flotante acumulado por sample arrastraría error de
    /// redondeo y volvería a producir drift en las vueltas largas; con `u64` el
    /// reset del loop cae exacto en `loop_start`.
    ///
    /// Un loop más corto que un beat NO se toca: los loops sub-beat (stutters,
    /// flams) son intencionales y cuantizarlos los destruiría.
    pub fn tempo_rate(&self, transport: &TransportPosition) -> f64 {
        match self.source_bpm {
            Some(sbpm) if sbpm > 0.0 && transport.bpm > 0.0 => {
                (transport.bpm / sbpm as f64).clamp(0.25, 4.0)
            }
            _ => 1.0,
        }
    }

    pub fn has_active_warp(&self) -> bool {
        if self.warp_markers.len() > 2 {
            return true;
        }
        if self.warp_markers.len() == 2 {
            let a = self.warp_markers[0];
            let b = self.warp_markers[1];
            return (a.0 - 0.0).abs() > 1e-4
                || (a.1 - 0.0).abs() > 1e-4
                || (b.0 - 1.0).abs() > 1e-4
                || (b.1 - 1.0).abs() > 1e-4;
        }
        false
    }

    pub fn has_active_pitch_or_formant(&self) -> bool {
        self.expressions.has_pitch_or_formant()
    }

    /// Lee un frame estéreo en `out_frame` usando el buffer pre-estirado con WSOLA si coincide
    /// con `rate`, o fallback OLA cuando `rate != 1.0`.
    pub fn sample_stereo_stretched(&self, out_frame: u64, rate: f64) -> (f32, f32) {
        let ch = self.channels.max(1);
        let active_warp = self.has_active_warp();
        let active_pf = self.has_active_pitch_or_formant();
        if (rate - 1.0).abs() < 1e-4 && !active_warp && !active_pf {
            let total_frames = self.samples.len() / ch;
            if total_frames == 0 {
                return (0.0, 0.0);
            }
            let f = (out_frame as usize).min(total_frames - 1);
            let idx = f * ch;
            let l = self.samples[idx];
            let r = if ch > 1 { self.samples[idx + 1] } else { l };
            return (l, r);
        }

        if !self.stretched_samples.is_empty() && (active_warp || active_pf || (self.stretched_rate - rate).abs() < 0.05) {
            let total_stretched = self.stretched_samples.len() / ch;
            if total_stretched > 0 {
                let f = (out_frame as usize).min(total_stretched - 1);
                let idx = f * ch;
                let l = self.stretched_samples[idx];
                let r = if ch > 1 { self.stretched_samples[idx + 1] } else { l };
                return (l, r);
            }
        }

        let total_frames = self.samples.len() / ch;
        if total_frames == 0 {
            return (0.0, 0.0);
        }
        let f = ((out_frame as f64 * rate).round() as usize).min(total_frames - 1);
        let idx = f * ch;
        let l = self.samples[idx];
        let r = if ch > 1 { self.samples[idx + 1] } else { l };
        (l, r)
    }

    pub fn quantized_loop_len(&self, transport: &TransportPosition) -> u64 {
        let rate = self.tempo_rate(transport);
        let raw_len = ((self.raw_loop_len() as f64) / rate).round().max(1.0) as u64;
        let spb = transport.samples_per_beat();
        if !(spb > 1.0) {
            // Sin BPM o SR utilizable no hay grilla: se usa la longitud cruda.
            return raw_len;
        }
        let spb_frames = spb.round() as u64;
        if spb_frames == 0 || raw_len < spb_frames {
            return raw_len;
        }
        let beats = raw_len / spb_frames;
        (beats * spb_frames).max(1).min(raw_len)
    }

    pub fn playback_length_frames(&self) -> u64 {
        if self.has_valid_clip_loop() {
            let end = self.clip_loop_end.min(self.natural_frames());
            end.saturating_sub(self.clip_loop_start)
        } else {
            self.natural_frames()
        }
    }

    pub fn voice_frame(
        &self,
        global_pos: u64,
        transport: &TransportPosition,
    ) -> Option<usize> {
        if global_pos < self.start_frame {
            return None;
        }
        let natural = self.natural_frames();
        if natural == 0 {
            return None;
        }
        let elapsed = global_pos - self.start_frame;
        let rate = self.tempo_rate(transport);

        if self.has_valid_clip_loop() {
            let natural = self.natural_frames();
            let loop_start_src = self.clip_loop_start.min(natural);
            let loop_start_out = ((loop_start_src as f64) / rate).round() as u64;
            let loop_len = self.quantized_loop_len(transport);
            let loop_end_out = loop_start_out + loop_len;
            let out_frame = if elapsed < loop_end_out {
                elapsed
            } else if transport.has_valid_loop() {
                let since_global = global_pos.saturating_sub(transport.loop_start_samples);
                loop_start_out + (since_global % loop_len)
            } else {
                loop_start_out + ((elapsed - loop_start_out) % loop_len)
            };
            let src_frame = ((out_frame as f64) * rate).round() as usize;
            return Some(src_frame.min(natural.saturating_sub(1) as usize));
        }

        let emission_len_out = ((self.emission_len_frames() as f64) / rate).round() as u64;
        if elapsed >= emission_len_out {
            return None;
        }
        let src_frame = ((elapsed as f64) * rate).round() as usize;
        Some(src_frame.min(natural.saturating_sub(1) as usize))
    }

    pub fn voice_frame_linear(
        &self,
        abs_pos: u64,
        transport: &TransportPosition,
    ) -> Option<usize> {
        if abs_pos < self.start_absolute {
            return None;
        }
        let natural = self.natural_frames();
        if natural == 0 {
            return None;
        }
        let elapsed = abs_pos - self.start_absolute;
        let rate = self.tempo_rate(transport);

        if self.has_valid_clip_loop() {
            let loop_start_src = self.clip_loop_start.min(natural);
            let loop_start_out = ((loop_start_src as f64) / rate).round() as u64;
            let loop_len = self.quantized_loop_len(transport);
            let out_frame = loop_start_out + (elapsed % loop_len);
            let src_frame = ((out_frame as f64) * rate).round() as usize;
            return Some(src_frame.min(natural.saturating_sub(1) as usize));
        }

        let emission_len_out = ((self.emission_len_frames() as f64) / rate).round() as u64;
        if elapsed >= emission_len_out {
            return None;
        }
        let src_frame = ((elapsed as f64) * rate).round() as usize;
        Some(src_frame.min(natural.saturating_sub(1) as usize))
    }

    pub fn voice_state_linear(
        &self,
        abs_pos: u64,
        transport: &TransportPosition,
    ) -> VoiceState {
        // Modo eventos: activo si hay al menos un evento audible en la posición.
        if !self.events.is_empty() {
            if abs_pos < self.start_absolute {
                return VoiceState::Finished;
            }
            let elapsed = abs_pos - self.start_absolute;
            let natural = self.natural_frames();
            if natural == 0 {
                return VoiceState::Finished;
            }
            let clip_frame = if self.has_valid_clip_loop() {
                let loop_start = self.clip_loop_start.min(natural);
                let loop_end = loop_start + self.quantized_loop_len(transport);
                let loop_len = loop_end - loop_start;
                (loop_start + (elapsed % loop_len)) as u64
            } else {
                if elapsed >= self.emission_len_frames() {
                    return VoiceState::Finished;
                }
                elapsed
            };
            let audible = self.events.iter().any(|e| {
                clip_frame >= e.clip_start && clip_frame < e.end()
            });
            return if audible {
                VoiceState::Active
            } else if !self.has_valid_clip_loop() && elapsed >= self.emission_len_frames() {
                VoiceState::Finished
            } else {
                // Hueco entre eventos: silencio pero la voz sigue viva.
                VoiceState::Active
            };
        }
        match self.voice_frame_linear(abs_pos, transport) {
            Some(_) => VoiceState::Active,
            None => VoiceState::Finished,
        }
    }

    /// Mezcla todos los eventos del pad en una posición del timeline del
    /// clip (ya con loop aplicado). Suma overlaps, aplica gain/fades.
    pub fn mix_events_at(&self, clip_frame: u64) -> (f32, f32) {
        let mut l = 0.0f32;
        let mut r = 0.0f32;
        for ev in &self.events {
            let (el, er) = ev.read_at(clip_frame);
            l += el;
            r += er;
        }
        (l, r)
    }

    /// Resuelve el frame del timeline (con loop) para un `elapsed` absoluto.
    /// Retorna `None` solo si el clip terminó (sin loop y pasado emisión).
    pub fn clip_frame_for_elapsed(
        &self,
        elapsed: u64,
        transport: &TransportPosition,
    ) -> Option<u64> {
        let natural = self.natural_frames();
        if natural == 0 {
            return None;
        }
        if self.has_valid_clip_loop() {
            let loop_start = self.clip_loop_start.min(natural);
            let loop_end = loop_start + self.quantized_loop_len(transport);
            let loop_len = loop_end - loop_start;
            Some((loop_start + (elapsed % loop_len)) as u64)
        } else {
            if elapsed >= self.emission_len_frames() {
                return None;
            }
            Some(elapsed)
        }
    }

    pub fn emission_len_frames(&self) -> u64 {
        let natural = self.natural_frames();
        if self.duration_frames > 0 {
            self.duration_frames.min(natural)
        } else {
            natural
        }
    }

    pub fn voice_state(&self, global_pos: u64, transport: &TransportPosition) -> VoiceState {
        match self.voice_frame(global_pos, transport) {
            Some(_) => VoiceState::Active,
            None => VoiceState::Finished,
        }
    }

    pub fn read_sample(&self, current_sample_index: usize) -> f32 {
        if !self.has_valid_clip_loop() {
            if (current_sample_index as u64) >= self.natural_frames() {
                return 0.0;
            }
        }
        let ch = self.channels.max(1);
        let idx = current_sample_index.saturating_mul(ch);
        match self.samples.get(idx) {
            Some(&s) if s.is_finite() => s,
            _ => 0.0,
        }
    }

    pub fn read_sample_r(&self, current_sample_index: usize) -> f32 {
        if !self.has_valid_clip_loop() {
            if (current_sample_index as u64) >= self.natural_frames() {
                return 0.0;
            }
        }
        let ch = self.channels.max(1);
        let idx = current_sample_index.saturating_mul(ch);
        if ch > 1 {
            match self.samples.get(idx + 1) {
                Some(&s) if s.is_finite() => s,
                _ => 0.0,
            }
        } else {
            self.read_sample(current_sample_index)
        }
    }
}

// ─── OpenDMS Polyphonic Voice Pool ────────────────────────────────

const MAX_DMS_VOICES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DmsEnvStage {
    Attack,
    Decay,
    Sustain,
    Release,
    Free,
}

#[derive(Debug, Clone)]
pub struct DmsVoice {
    active: bool,
    samples: *const [f32],
    channels: usize,
    sample_len: usize,
    play_pos: f64,
    play_speed: f64,
    gain: f32,
    pan: f32,
    velocity: f32,
    pad_idx: usize,
    /// Índice de pista del engine (0..15) que disparó esta voz.
    /// Antes todas las voces compartían un único `dms_track_idx` global
    /// (siempre 0), así que solo el fader del Track 1 afectaba a la batería.
    pub track_idx: usize,
    env_stage: DmsEnvStage,
    env_level: f32,
    attack_rate: f32,
    decay_rate: f32,
    sustain_level: f32,
    release_rate: f32,
    age: u64,
}

unsafe impl Send for DmsVoice {}
unsafe impl Sync for DmsVoice {}

impl DmsVoice {
    fn new() -> Self {
        Self {
            active: false,
            samples: &[],
            channels: 0,
            sample_len: 0,
            play_pos: 0.0,
            play_speed: 1.0,
            gain: 1.0,
            pan: 0.0,
            velocity: 1.0,
            pad_idx: 0,
            track_idx: 0,
            env_stage: DmsEnvStage::Free,
            env_level: 0.0,
            attack_rate: 0.0,
            decay_rate: 0.0,
            sustain_level: 1.0,
            release_rate: 0.0,
            age: 0,
        }
    }

    fn process_frame(&mut self, sr: f32) -> (f32, f32) {
        if !self.active || self.env_stage == DmsEnvStage::Free {
            return (0.0, 0.0);
        }

        let (env_out, stage_done) = self.advance_envelope(sr);
        if stage_done {
            match self.env_stage {
                DmsEnvStage::Attack => {
                    self.env_stage = DmsEnvStage::Decay;
                    self.env_level = 1.0;
                }
                DmsEnvStage::Decay => {
                    self.env_stage = DmsEnvStage::Sustain;
                    self.env_level = self.sustain_level;
                }
                DmsEnvStage::Release => {
                    self.active = false;
                    self.env_stage = DmsEnvStage::Free;
                    return (0.0, 0.0);
                }
                _ => {}
            }
        }

        let idx = self.play_pos as usize;
        if idx >= self.sample_len {
            self.active = false;
            self.env_stage = DmsEnvStage::Free;
            return (0.0, 0.0);
        }

        let ch = self.channels.max(1);
        let raw_idx = idx * ch;
        let slice = unsafe { &*self.samples };
        let l = if raw_idx < slice.len() {
            slice[raw_idx]
        } else {
            0.0
        };
        let r = if ch > 1 && raw_idx + 1 < slice.len() {
            slice[raw_idx + 1]
        } else {
            l
        };

        self.play_pos += self.play_speed;
        if self.play_pos >= self.sample_len as f64 {
            self.active = false;
            self.env_stage = DmsEnvStage::Free;
            return (0.0, 0.0);
        }

        let amp = env_out * self.gain * self.velocity;
        let pan_l = if self.pan <= 0.0 { 1.0 } else { 1.0 - self.pan };
        let pan_r = if self.pan >= 0.0 { 1.0 } else { 1.0 + self.pan };

        (l * amp * pan_l, r * amp * pan_r)
    }

    fn advance_envelope(&mut self, sr: f32) -> (f32, bool) {
        match self.env_stage {
            DmsEnvStage::Attack => {
                if self.attack_rate <= 0.0 {
                    return (1.0, true);
                }
                self.env_level += self.attack_rate / sr;
                if self.env_level >= 1.0 {
                    (1.0, true)
                } else {
                    (self.env_level, false)
                }
            }
            DmsEnvStage::Decay => {
                if self.decay_rate <= 0.0 {
                    return (self.sustain_level, true);
                }
                self.env_level -= self.decay_rate / sr;
                if self.env_level <= self.sustain_level {
                    (self.sustain_level, true)
                } else {
                    (self.env_level, false)
                }
            }
            DmsEnvStage::Sustain => (self.sustain_level, false),
            DmsEnvStage::Release => {
                if self.release_rate <= 0.0 {
                    return (0.0, true);
                }
                self.env_level -= self.release_rate / sr;
                if self.env_level <= 0.0 {
                    (0.0, true)
                } else {
                    (self.env_level.max(0.0), false)
                }
            }
            DmsEnvStage::Free => (0.0, true),
        }
    }
}

pub struct DmsVoicePool {
    voices: Vec<DmsVoice>,
    global_age: u64,
}

impl DmsVoicePool {
    pub fn new() -> Self {
        let voices = (0..MAX_DMS_VOICES).map(|_| DmsVoice::new()).collect();
        Self {
            voices,
            global_age: 0,
        }
    }

    pub fn trigger(
        &mut self,
        samples: &[f32],
        channels: usize,
        pad_idx: usize,
        gain: f32,
        pan: f32,
        velocity: f32,
        play_speed: f64,
        adsr: &DmsAdsrParams,
        track_idx: usize,
    ) -> Option<usize> {
        self.global_age = self.global_age.wrapping_add(1);

        let free = self.voices.iter().position(|v| !v.active);
        let idx = match free {
            Some(i) => i,
            None => {
                let oldest = self.voices.iter().enumerate()
                    .min_by_key(|(_, v)| v.age)
                    .map(|(i, _)| i)?;
                self.voices[oldest].active = false;
                oldest
            }
        };

        let v = &mut self.voices[idx];
        v.active = true;
        v.samples = samples as *const [f32];
        v.channels = channels;
        v.sample_len = samples.len() / channels.max(1);
        v.play_pos = 0.0;
        v.play_speed = play_speed;
        v.gain = gain;
        v.pan = pan;
        v.velocity = velocity;
        v.pad_idx = pad_idx;
        v.track_idx = track_idx.min(15);
        v.env_stage = DmsEnvStage::Attack;
        v.env_level = 0.0;
        v.attack_rate = adsr.attack_rate;
        v.decay_rate = adsr.decay_rate;
        v.sustain_level = adsr.sustain_level;
        v.release_rate = adsr.release_rate;
        v.age = self.global_age;

        Some(idx)
    }

    pub fn release_pad(&mut self, pad_idx: usize) {
        for v in self.voices.iter_mut() {
            if v.active && v.pad_idx == pad_idx && v.env_stage != DmsEnvStage::Release {
                v.env_stage = DmsEnvStage::Release;
            }
        }
    }

    pub fn kill_pad(&mut self, pad_idx: usize) {
        for v in self.voices.iter_mut() {
            if v.active && v.pad_idx == pad_idx {
                v.active = false;
                v.env_stage = DmsEnvStage::Free;
            }
        }
    }

    pub fn process_all(&mut self, output_l: &mut [f32], output_r: &mut [f32], sr: f32) {
        let unity_l = [1.0f32; 16];
        let unity_r = [1.0f32; 16];
        self.process_all_with_gains(output_l, output_r, &unity_l, &unity_r, sr);
    }

    /// Mezcla aplicando a cada voz el volumen/paneo/mute/solo de SU pista.
    /// `gains_l/r` ya incluyen mute/solo (0.0 si la pista está silenciada).
    pub fn process_all_with_gains(
        &mut self,
        output_l: &mut [f32],
        output_r: &mut [f32],
        gains_l: &[f32; 16],
        gains_r: &[f32; 16],
        sr: f32,
    ) {
        let buf_len = output_l.len();
        for v in self.voices.iter_mut() {
            if !v.active {
                continue;
            }
            let ti = v.track_idx.min(15);
            let gl = gains_l[ti];
            let gr = gains_r[ti];
            if gl == 0.0 && gr == 0.0 {
                // Igual hay que avanzar la envolvente/posición para que la
                // voz termine aunque su pista esté muteada.
                for i in 0..buf_len {
                    let (l, r) = v.process_frame(sr);
                    let _ = (l, r, i);
                }
                continue;
            }
            for i in 0..buf_len {
                let (l, r) = v.process_frame(sr);
                output_l[i] += l * gl;
                output_r[i] += r * gr;
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DmsAdsrParams {
    pub attack_rate: f32,
    pub decay_rate: f32,
    pub sustain_level: f32,
    pub release_rate: f32,
}

impl Default for DmsAdsrParams {
    fn default() -> Self {
        Self {
            attack_rate: 1.0,
            decay_rate: 1.0,
            sustain_level: 1.0,
            release_rate: 0.05,
        }
    }
}

/// Pads por pista en el banco global de samples del OpenDMS.
/// Cada pista tiene su propio banco de 64 pads: la clave global es
/// `track * DMS_PADS_PER_TRACK + pad`. Antes el banco era único por
/// `pad_idx` y todas las pistas que usaban el mismo pad se pisaban:
/// el mixer quedaba sonando todo por un solo canal/track.
pub const DMS_PADS_PER_TRACK: usize = 64;

#[inline]
pub fn dms_pad_key(track_idx: usize, pad_idx: usize) -> usize {
    track_idx.min(15) * DMS_PADS_PER_TRACK + pad_idx.min(DMS_PADS_PER_TRACK - 1)
}

pub const MAX_WAVETABLE_VOICES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WavetableEnvStage {
    Attack,
    Decay,
    Sustain,
    Release,
    Free,
}

#[derive(Debug, Clone, Copy)]
pub struct WavetableAdsrParams {
    pub attack_rate: f32,
    pub decay_rate: f32,
    pub sustain_level: f32,
    pub release_rate: f32,
}

impl Default for WavetableAdsrParams {
    fn default() -> Self {
        Self {
            attack_rate: 200.0, // ~5ms
            decay_rate: 8.0,    // ~125ms
            sustain_level: 0.7,
            release_rate: 6.0,  // ~160ms
        }
    }
}

pub struct WavetableVoice {
    pub active: bool,
    pub pitch: u8,
    pub velocity: f32,
    pub track_idx: usize,
    pub phase: f32,
    pub phase_inc: f32,
    pub env_stage: WavetableEnvStage,
    pub env_level: f32,
    pub attack_rate: f32,
    pub decay_rate: f32,
    pub sustain_level: f32,
    pub release_rate: f32,
    pub age: u64,
}

impl WavetableVoice {
    pub fn new() -> Self {
        Self {
            active: false,
            pitch: 60,
            velocity: 1.0,
            track_idx: 0,
            phase: 0.0,
            phase_inc: 0.0,
            env_stage: WavetableEnvStage::Free,
            env_level: 0.0,
            attack_rate: 200.0,
            decay_rate: 8.0,
            sustain_level: 0.7,
            release_rate: 6.0,
            age: 0,
        }
    }

    #[inline]
    pub fn advance_envelope(&mut self, sr: f32) -> f32 {
        match self.env_stage {
            WavetableEnvStage::Attack => {
                if self.attack_rate <= 0.0 {
                    self.env_level = 1.0;
                    self.env_stage = WavetableEnvStage::Decay;
                } else {
                    self.env_level += self.attack_rate / sr;
                    if self.env_level >= 1.0 {
                        self.env_level = 1.0;
                        self.env_stage = WavetableEnvStage::Decay;
                    }
                }
            }
            WavetableEnvStage::Decay => {
                if self.decay_rate <= 0.0 {
                    self.env_level = self.sustain_level;
                    self.env_stage = WavetableEnvStage::Sustain;
                } else {
                    self.env_level -= self.decay_rate / sr;
                    if self.env_level <= self.sustain_level {
                        self.env_level = self.sustain_level;
                        self.env_stage = WavetableEnvStage::Sustain;
                    }
                }
            }
            WavetableEnvStage::Sustain => {
                self.env_level = self.sustain_level;
            }
            WavetableEnvStage::Release => {
                if self.release_rate <= 0.0 {
                    self.env_level = 0.0;
                    self.env_stage = WavetableEnvStage::Free;
                    self.active = false;
                } else {
                    self.env_level -= self.release_rate / sr;
                    if self.env_level <= 0.0 {
                        self.env_level = 0.0;
                        self.env_stage = WavetableEnvStage::Free;
                        self.active = false;
                    }
                }
            }
            WavetableEnvStage::Free => {
                self.env_level = 0.0;
                self.active = false;
            }
        }
        self.env_level
    }

    #[inline]
    pub fn process_frame(&mut self, table: &[f32], sr: f32) -> (f32, f32) {
        if !self.active || table.is_empty() {
            return (0.0, 0.0);
        }
        let env = self.advance_envelope(sr);
        if !self.active {
            return (0.0, 0.0);
        }

        let table_len = table.len();
        let idx0 = (self.phase as usize) % table_len;
        let idx1 = (idx0 + 1) % table_len;
        let frac = self.phase - self.phase.floor();
        let s0 = table[idx0];
        let s1 = table[idx1];
        let sample = (s0 + frac * (s1 - s0)) * env * self.velocity;

        self.phase = (self.phase + self.phase_inc) % (table_len as f32);

        let pan = ((self.pitch as f32 - 60.0) / 60.0 * 0.25).clamp(-0.4, 0.4);
        let pan_l = if pan > 0.0 { 1.0 - pan } else { 1.0 };
        let pan_r = if pan < 0.0 { 1.0 + pan } else { 1.0 };

        (sample * pan_l, sample * pan_r)
    }
}

pub struct WavetableVoicePool {
    voices: Vec<WavetableVoice>,
    global_age: u64,
}

impl WavetableVoicePool {
    pub fn new() -> Self {
        let voices = (0..MAX_WAVETABLE_VOICES).map(|_| WavetableVoice::new()).collect();
        Self {
            voices,
            global_age: 0,
        }
    }

    pub fn trigger(
        &mut self,
        pitch: u8,
        velocity: f32,
        track_idx: usize,
        adsr: &WavetableAdsrParams,
        table_len: usize,
        sr: f32,
    ) -> Option<usize> {
        self.global_age = self.global_age.wrapping_add(1);

        let existing = self.voices.iter().position(|v| v.active && v.track_idx == track_idx && v.pitch == pitch);

        let idx = if let Some(i) = existing {
            i
        } else if let Some(free_i) = self.voices.iter().position(|v| !v.active) {
            free_i
        } else {
            let oldest = self.voices.iter().enumerate()
                .min_by_key(|(_, v)| v.age)
                .map(|(i, _)| i)?;
            oldest
        };

        let freq = 440.0 * 2.0_f32.powf((pitch as f32 - 69.0) / 12.0);
        let t_len = if table_len > 0 { table_len } else { 2048 };
        let phase_inc = (freq * t_len as f32) / sr.max(8000.0);

        let v = &mut self.voices[idx];
        v.active = true;
        v.pitch = pitch;
        v.velocity = velocity.clamp(0.0, 1.0);
        v.track_idx = track_idx.min(15);
        v.phase_inc = phase_inc;
        v.env_stage = WavetableEnvStage::Attack;
        v.env_level = 0.0;
        v.attack_rate = adsr.attack_rate;
        v.decay_rate = adsr.decay_rate;
        v.sustain_level = adsr.sustain_level;
        v.release_rate = adsr.release_rate;
        v.age = self.global_age;

        Some(idx)
    }

    pub fn release_note(&mut self, pitch: u8, track_idx: usize) {
        for v in self.voices.iter_mut() {
            if v.active && v.track_idx == track_idx && v.pitch == pitch && v.env_stage != WavetableEnvStage::Release {
                v.env_stage = WavetableEnvStage::Release;
            }
        }
    }

    pub fn kill_note(&mut self, pitch: u8, track_idx: usize) {
        for v in self.voices.iter_mut() {
            if v.active && v.track_idx == track_idx && v.pitch == pitch {
                v.active = false;
                v.env_stage = WavetableEnvStage::Free;
            }
        }
    }

    pub fn kill_track_notes(&mut self, track_idx: usize) {
        for v in self.voices.iter_mut() {
            if v.active && v.track_idx == track_idx {
                v.active = false;
                v.env_stage = WavetableEnvStage::Free;
            }
        }
    }

    pub fn kill_all(&mut self) {
        for v in self.voices.iter_mut() {
            v.active = false;
            v.env_stage = WavetableEnvStage::Free;
        }
    }

    pub fn process_track(
        &mut self,
        track_idx: usize,
        table: &[f32],
        track_buf: &mut [f32],
        gain_l: f32,
        gain_r: f32,
        frames: usize,
        sr: f32,
    ) {
        if gain_l == 0.0 && gain_r == 0.0 {
            for v in self.voices.iter_mut() {
                if v.active && v.track_idx == track_idx {
                    for _ in 0..frames {
                        let _ = v.process_frame(table, sr);
                    }
                }
            }
            return;
        }

        let has_active = self.voices.iter().any(|v| v.active && v.track_idx == track_idx);
        if !has_active {
            return;
        }

        for v in self.voices.iter_mut() {
            if !v.active || v.track_idx != track_idx {
                continue;
            }
            for i in 0..frames {
                let (l, r) = v.process_frame(table, sr);
                if i * 2 + 1 < track_buf.len() {
                    track_buf[i * 2] += l * gain_l;
                    track_buf[i * 2 + 1] += r * gain_r;
                }
            }
        }
    }

    pub fn process_all_with_track_tables(
        &mut self,
        tables: &[Option<Vec<f32>>; 16],
        output_l: &mut [f32],
        output_r: &mut [f32],
        gains_l: &[f32; 16],
        gains_r: &[f32; 16],
        sr: f32,
    ) {
        let buf_len = output_l.len().min(output_r.len());
        for v in self.voices.iter_mut() {
            if !v.active {
                continue;
            }
            let ti = v.track_idx.min(15);
            let Some(table) = tables[ti].as_ref() else {
                v.active = false;
                v.env_stage = WavetableEnvStage::Free;
                continue;
            };
            let gl = gains_l[ti];
            let gr = gains_r[ti];
            if gl == 0.0 && gr == 0.0 {
                for _ in 0..buf_len {
                    let _ = v.process_frame(table, sr);
                }
                continue;
            }
            for i in 0..buf_len {
                let (l, r) = v.process_frame(table, sr);
                output_l[i] += l * gl;
                output_r[i] += r * gr;
            }
        }
    }

    pub fn process_all_with_gains(
        &mut self,
        table: &[f32],
        output_l: &mut [f32],
        output_r: &mut [f32],
        gains_l: &[f32; 16],
        gains_r: &[f32; 16],
        sr: f32,
    ) {
        let buf_len = output_l.len();
        for v in self.voices.iter_mut() {
            if !v.active {
                continue;
            }
            let ti = v.track_idx.min(15);
            let gl = gains_l[ti];
            let gr = gains_r[ti];
            if gl == 0.0 && gr == 0.0 {
                for _ in 0..buf_len {
                    let _ = v.process_frame(table, sr);
                }
                continue;
            }
            for i in 0..buf_len {
                let (l, r) = v.process_frame(table, sr);
                output_l[i] += l * gl;
                output_r[i] += r * gr;
            }
        }
    }
}

pub fn create_default_synth_table() -> Vec<f32> {
    let size = 2048;
    let mut table = Vec::with_capacity(size);
    for i in 0..size {
        let phase = i as f32 / size as f32;
        let s1 = (phase * std::f32::consts::TAU).sin();
        let s2 = (2.0 * phase * std::f32::consts::TAU).sin() * 0.5;
        let s3 = (3.0 * phase * std::f32::consts::TAU).sin() * 0.28;
        let s4 = (4.0 * phase * std::f32::consts::TAU).sin() * 0.15;
        let s5 = (5.0 * phase * std::f32::consts::TAU).sin() * 0.08;
        let val = (s1 + s2 + s3 + s4 + s5) * 0.48;
        table.push(val);
    }
    table
}

/// Empaqueta `(scene_index, frame)` en un `u64` atómico para reportar el
/// playhead interno del clip activo por pista sin locks desde el hilo de audio.
#[inline]
pub fn pack_clip_playhead(scene_index: usize, frame: u64) -> u64 {
    (1u64 << 63) | (((scene_index as u64) & 0x7FFF) << 48) | (frame & 0x0000_FFFF_FFFF_FFFF)
}

/// Desempaqueta `(scene_index, frame)` de un `u64` producido por [`pack_clip_playhead`].
/// Retorna `None` si no hay ningún clip reproduciéndose en esa pista.
#[inline]
pub fn unpack_clip_playhead(packed: u64) -> Option<(usize, u64)> {
    if (packed & (1u64 << 63)) == 0 {
        return None;
    }
    let scene_index = ((packed >> 48) & 0x7FFF) as usize;
    let frame = packed & 0x0000_FFFF_FFFF_FFFF;
    Some((scene_index, frame))
}

/// Lanzamiento cuantizado de escena pendiente en el motor de audio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingSceneLaunch {
    pub scene_index: usize,
    pub target_sample: u64,
    pub grid: QuantizationGrid,
}

/// Detención cuantizada de pista pendiente en el motor de audio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingTrackStop {
    pub track_index: usize,
    pub target_sample: u64,
    pub grid: QuantizationGrid,
}

pub struct AudioEngine<'a> {
    pub transport: TransportPosition,
    pub mode: EngineMode,
    pub matrix: TrackMatrix,
    /// Cuantización global de lanzamiento de escenas (default `None` en el
    /// motor para disparo inmediato salvo configuración explícita de sesión).
    pub global_launch_quantization: QuantizationGrid,
    /// Override opcional de cuantización por índice de escena.
    pub scene_quantization_overrides: Vec<Option<QuantizationGrid>>,
    /// Escena en cola esperando su próximo límite de cuantización.
    pub pending_scene_launch: Option<PendingSceneLaunch>,
    /// Índice atómico de la escena en cola (`-1` si no hay ninguna) para UI.
    pub pending_scene_index_atomic: Arc<AtomicI32>,
    /// Pistas en cola para detenerse en el próximo límite de cuantización.
    pub pending_track_stops: Vec<PendingTrackStop>,
    /// Máscara de bits (1 << track_index) de pistas con stop cuantizado pendiente.
    pub pending_stop_tracks_mask: Arc<AtomicU32>,
    pub main_filter: StateVariableFilter,
    pub oscillator: WavetableOscillator<'a>,
    pub sample_rate: f32,
    pub clips: Vec<AudioClipInstance>,
    pub preview_player: PreviewPlayer,
    pub position_clock: Arc<AtomicU64>,
    /// Reloj monotónico en frames (nunca wrapea con el loop global ni se
    /// resetea con Stop/Seek). La GUI lo usa para derivar la fase de los
    /// MIDI clips con la misma base temporal que las voces de audio
    /// (`start_absolute`), así el loop MIDI no se desincroniza del audio.
    pub absolute_clock: Arc<AtomicU64>,
    pub absolute_frame: u64,
    pub output_level_bits: Arc<AtomicU32>,
    pub track_peak_bits: [Arc<AtomicU32>; 16],
    pub track_clip_playhead: [Arc<AtomicU64>; 16],
    track_output_bufs: Vec<Vec<f32>>,
    pub track_volumes: [AtomicU32; 16],
    pub track_pans: [AtomicU32; 16],
    pub track_mutes: [AtomicBool; 16],
    pub track_solos: [AtomicBool; 16],
    pub master_gain: AtomicU32,
    pub midi_clips: Vec<MidiClipInstance>,
    pub dms_voice_pool: DmsVoicePool,
    pub dms_samples: Vec<Option<Vec<f32>>>,
    pub dms_channels: Vec<usize>,
    pub dms_track_idx: usize,
    pub wavetable_voice_pool: WavetableVoicePool,
    pub wavetable_data: Vec<f32>,
    pub track_wavetables: [Option<Vec<f32>>; 16],
    pub track_wavetable_pitch_offsets: [i8; 16],
    pub track_wavetable_volumes: [f32; 16],
    pub sampler_voice_pool: SamplerVoicePool,
    pub track_sampler_samples: [Option<Arc<SamplerSampleData>>; 16],
    pub track_sampler_params: [SamplerDspParams; 16],
    pub open_poly_voice_pool: OpenPolyVoicePool,
    pub track_open_poly_enabled: [bool; 16],
    pub track_open_poly_params: [OpenPolyDspParams; 16],
    pub track_open_poly_wavetables: [Option<Arc<OpenPolyWavetableData>>; 16],
    pub open_grid_voice_pool: OpenGridVoicePool,
    pub track_open_grid_enabled: [bool; 16],
    /// Cadena de efectos externos (VST3/CLAP) por pista.
    ///
    /// Cada pista tiene su propia cadena: los plugins se instancian en el
    /// hilo de control (nunca en el de audio) y se procesan bloque a bloque
    /// en `process()`. El `Box<dyn PluginInstance>` es `Send` porque el
    /// ownership vive aquí, en el motor, y el hilo de audio sólo llama
    /// `process()` sobre él.
    pub track_effects: Vec<Vec<hikaru_dsp::DspPluginNode>>,
    /// Sample rate actualizado para los efectos externos.
    ///
    /// Los plugins VST3/CLAP necesitan conocer el sample rate para
    /// procesar correctamente. Se actualiza cuando cambia el stream.
    pub effects_sample_rate: f32,
    dms_scratch_l: Vec<f32>,
    dms_scratch_r: Vec<f32>,
    fx_scratch_buf: Vec<f32>,
    track_midi_events: Vec<Vec<hikaru_midi::MidiEvent>>,
    track_pdc_delays: Vec<PdcDelayLine>,
    dms_pdc_delay: PdcDelayLine,
    current_pdc_latency: u32,
}

/// Máximo de frames estéreo que soporta la compensación automática de latencia
/// (PDC): ~2.7 s a 48 kHz, sobrado para phasers espectrales FFT, limitadores
/// lookahead y cadenas largas de VST3/CLAP.
const MAX_PDC_DELAY_FRAMES: usize = 131_072;

/// Línea de retardo estéreo interleaved para Plugin Delay Compensation (PDC).
///
/// Cuando `delay_frames == 0` es un passthrough sin trabajo ni memoria.
/// Cuando alguna pista tiene plugins con latencia `L > 0`, las pistas más rápidas
/// se retardan exactamente `max_latency - track_latency` samples para que los
/// clips de audio, sintetizadores y baterías lleguen alineados al sample en el
/// bus Master.
#[derive(Clone)]
struct PdcDelayLine {
    buf: Vec<f32>,
    cap_frames: usize,
    write_pos: usize,
    last_delay: usize,
}

impl PdcDelayLine {
    fn new() -> Self {
        Self {
            buf: Vec::new(),
            cap_frames: 0,
            write_pos: 0,
            last_delay: 0,
        }
    }

    fn ensure_capacity(&mut self, delay_frames: usize) {
        let needed = delay_frames.min(MAX_PDC_DELAY_FRAMES);
        if needed == 0 || self.cap_frames > needed {
            return;
        }
        let new_cap = (needed + 1).next_power_of_two().max(4096).min(MAX_PDC_DELAY_FRAMES + 1);
        self.buf.clear();
        self.buf.resize(new_cap * 2, 0.0);
        self.cap_frames = new_cap;
        self.write_pos = 0;
    }

    fn clear(&mut self) {
        if !self.buf.is_empty() {
            self.buf.fill(0.0);
        }
        self.write_pos = 0;
    }

    fn process_interleaved(&mut self, samples: &mut [f32], delay_frames: usize) {
        let delay = delay_frames.min(MAX_PDC_DELAY_FRAMES);
        if delay == 0 {
            if self.last_delay != 0 {
                self.clear();
                self.last_delay = 0;
            }
            return;
        }
        if self.cap_frames <= delay {
            self.ensure_capacity(delay);
        } else if self.last_delay == 0 {
            self.clear();
        }
        self.last_delay = delay;

        let cap = self.cap_frames;
        let frames = samples.len() / 2;
        let mut wp = self.write_pos;
        for f in 0..frames {
            let rp = if wp >= delay { wp - delay } else { wp + cap - delay };
            let r_idx = rp * 2;
            let w_idx = wp * 2;
            let out_l = self.buf[r_idx];
            let out_r = self.buf[r_idx + 1];
            self.buf[w_idx] = samples[f * 2];
            self.buf[w_idx + 1] = samples[f * 2 + 1];
            samples[f * 2] = out_l;
            samples[f * 2 + 1] = out_r;
            wp += 1;
            if wp >= cap {
                wp = 0;
            }
        }
        self.write_pos = wp;
    }
}

fn build_host_transport(
    transport: &TransportPosition,
    frame_start: u64,
    upstream_latency: u32,
) -> hikaru_plugin_host::HostTransportInfo {
    let sr = (transport.sample_rate.get() as f64).max(1.0);
    let bpm = transport.bpm.max(1.0);
    let spb = (sr * 60.0) / bpm;
    let effective_sample = frame_start as i64 - upstream_latency as i64;
    let beats_position = if spb > 0.0 {
        effective_sample as f64 / spb
    } else {
        0.0
    };
    let bpb = transport.beats_per_bar.max(1) as f64;
    let bar_number = (beats_position / bpb).floor() as i32;
    let bar_start_beats = (bar_number as f64) * bpb;
    let loop_start_beats = if spb > 0.0 {
        transport.loop_start_samples as f64 / spb
    } else {
        0.0
    };
    let loop_end_beats = if spb > 0.0 {
        transport.loop_end_samples as f64 / spb
    } else {
        0.0
    };
    hikaru_plugin_host::HostTransportInfo {
        is_playing: transport.playback_state == TransportPlaybackState::Playing,
        tempo: bpm,
        sample_rate: sr,
        sample_position: effective_sample,
        beats_position,
        bar_start_beats,
        bar_number,
        time_sig_num: transport.beats_per_bar.clamp(1, u16::MAX as u32) as u16,
        time_sig_den: transport.beat_division.clamp(1, u16::MAX as u32) as u16,
        loop_enabled: transport.has_valid_loop(),
        loop_start_beats,
        loop_end_beats,
    }
}

impl<'a> AudioEngine<'a> {
    pub fn new(sr: SampleRate, wavetable: &'a [f32], position_clock: Arc<AtomicU64>) -> Self {
        Self::with_clocks(sr, wavetable, position_clock, Arc::new(AtomicU64::new(0)))
    }

    pub fn with_clocks(
        sr: SampleRate,
        wavetable: &'a [f32],
        position_clock: Arc<AtomicU64>,
        absolute_clock: Arc<AtomicU64>,
    ) -> Self {
        let mut filter = StateVariableFilter::new();
        filter.set_params(2000.0, 0.707, sr.get());

        Self {
            transport: TransportPosition::new(sr, 128.0),
            mode: EngineMode::OpenStudio,
            matrix: TrackMatrix::new(),
            global_launch_quantization: QuantizationGrid::None,
            scene_quantization_overrides: vec![None; hikaru_sequencer::matrix::DEFAULT_SCENE_COUNT],
            pending_scene_launch: None,
            pending_scene_index_atomic: Arc::new(AtomicI32::new(-1)),
            pending_track_stops: Vec::new(),
            pending_stop_tracks_mask: Arc::new(AtomicU32::new(0)),
            main_filter: filter,
            oscillator: WavetableOscillator::new(wavetable, sr),
            sample_rate: sr.get(),
            clips: Vec::new(),
            preview_player: PreviewPlayer::new(),
            position_clock,
            absolute_clock,
            absolute_frame: 0,
            output_level_bits: Arc::new(AtomicU32::new(0.0f32.to_bits())),
            track_peak_bits: std::array::from_fn(|_| Arc::new(AtomicU32::new(0.0f32.to_bits()))),
            track_clip_playhead: std::array::from_fn(|_| Arc::new(AtomicU64::new(0))),
            track_output_bufs: (0..16).map(|_| Vec::with_capacity(MAX_TRACK_BUF)).collect(),
            track_volumes: std::array::from_fn(|_| AtomicU32::new(0.75f32.to_bits())),
            track_pans: std::array::from_fn(|_| AtomicU32::new(0.0f32.to_bits())),
            track_mutes: std::array::from_fn(|_| AtomicBool::new(false)),
            track_solos: std::array::from_fn(|_| AtomicBool::new(false)),
            master_gain: AtomicU32::new(0.75f32.to_bits()),
            midi_clips: Vec::new(),
            dms_voice_pool: DmsVoicePool::new(),
            dms_samples: Vec::new(),
            dms_channels: Vec::new(),
            dms_track_idx: 0,
            wavetable_voice_pool: WavetableVoicePool::new(),
            wavetable_data: if wavetable.iter().any(|&s| s != 0.0) {
                wavetable.to_vec()
            } else {
                create_default_synth_table()
            },
            track_wavetables: std::array::from_fn(|_| None),
            track_wavetable_pitch_offsets: [0; 16],
            track_wavetable_volumes: [1.0; 16],
            sampler_voice_pool: SamplerVoicePool::new(),
            track_sampler_samples: std::array::from_fn(|_| None),
            track_sampler_params: std::array::from_fn(|_| SamplerDspParams::default()),
            open_poly_voice_pool: OpenPolyVoicePool::new(),
            track_open_poly_enabled: [false; 16],
            track_open_poly_params: std::array::from_fn(|_| OpenPolyDspParams::default()),
            track_open_poly_wavetables: std::array::from_fn(|_| None),
            open_grid_voice_pool: OpenGridVoicePool::new(),
            track_open_grid_enabled: [false; 16],
            track_effects: (0..16).map(|_| Vec::new()).collect(),
            effects_sample_rate: sr.get(),
            dms_scratch_l: vec![0.0; MAX_TRACK_BUF],
            dms_scratch_r: vec![0.0; MAX_TRACK_BUF],
            fx_scratch_buf: vec![0.0; MAX_TRACK_BUF],
            track_midi_events: (0..16).map(|_| Vec::with_capacity(128)).collect(),
            track_pdc_delays: (0..16).map(|_| PdcDelayLine::new()).collect(),
            dms_pdc_delay: PdcDelayLine::new(),
            current_pdc_latency: 0,
        }
    }

    /// Latencia global compensada actualmente por PDC (en samples).
    pub fn pdc_latency_samples(&self) -> u32 {
        self.current_pdc_latency
    }

    /// Limpia las líneas de retardo PDC (al detener o hacer seek) para evitar colas viejas.
    fn clear_pdc_delays(&mut self) {
        for d in &mut self.track_pdc_delays {
            d.clear();
        }
        self.dms_pdc_delay.clear();
    }

    /// Pre-asigna en el hilo de control los ring buffers PDC según la latencia
    /// actual de los plugins cargados (evita allocaciones en el audio thread).
    fn prepare_pdc_buffers(&mut self) {
        let mut any_plugin = false;
        let mut max_lat = 0usize;
        for chain in &self.track_effects {
            if !chain.is_empty() {
                any_plugin = true;
            }
            let sum: u32 = chain
                .iter()
                .filter(|n| n.is_active)
                .map(|n| n.latency_samples())
                .fold(0u32, |acc, x| acc.saturating_add(x));
            max_lat = max_lat.max(sum as usize);
        }
        // Si hay algún plugin cargado, pre-reservamos al menos 8192 frames por
        // pista en el hilo de control porque muchos VST3/CLAP recién definen su
        // latencia real (p. ej. FFT 1024/2048/4096) tras `activate()`/`start_processing()`
        // en el primer bloque de audio.
        let prealloc = if any_plugin { max_lat.max(8192) } else { max_lat };
        if prealloc > 0 {
            for d in &mut self.track_pdc_delays {
                d.ensure_capacity(prealloc);
            }
            self.dms_pdc_delay.ensure_capacity(prealloc);
        }
        self.current_pdc_latency = max_lat.min(MAX_PDC_DELAY_FRAMES) as u32;
    }

    pub fn output_peak(&self) -> f32 {
        f32::from_bits(self.output_level_bits.load(Ordering::Relaxed))
    }

    pub fn track_peak(&self, track_idx: usize) -> f32 {
        if track_idx < 16 {
            f32::from_bits(self.track_peak_bits[track_idx].load(Ordering::Relaxed))
        } else {
            0.0
        }
    }

    pub fn output_db(&self) -> f32 {
        let peak = self.output_peak();
        if peak <= 0.0 || !peak.is_finite() {
            f32::NEG_INFINITY
        } else {
            20.0 * peak.log10()
        }
    }

    pub fn set_mode(&mut self, mode: EngineMode) {
        self.mode = mode;
    }

    pub fn set_sample_rate(&mut self, new_sr: f32) {
        self.sample_rate = new_sr;
        self.transport.sample_rate = hikaru_core::SampleRate::new(new_sr);
        self.main_filter.set_params(2000.0, 0.707, new_sr);
        // Los efectos externos también necesitan el sample rate nuevo.
        self.effects_sample_rate = new_sr;
        for chain in self.track_effects.iter_mut() {
            for node in chain.iter_mut() {
                node.instance.set_sample_rate(new_sr);
                // La cinta hacia el editor nativo también necesita el cambio:
                // la ventana abierta la lee para saber a qué frecuencia corre
                // su propia instancia.
                if let Some(tap) = &node.tap {
                    tap.set_sample_rate(new_sr);
                }
            }
        }
    }

    /// Carga un plugin externo (VST3/CLAP) en un slot de una pista.
    ///
    /// Se llama desde el hilo de control (nunca desde el de audio): el
    /// `dlopen` + init puede tardar cientos de ms. El plugin queda en la
    /// cadena de efectos de la pista y se procesa bloque a bloque en
    /// `process()`. Devuelve `Ok(())` si se cargó, o `Err` con el motivo.
    pub fn load_external_plugin(
        &mut self,
        track_idx: usize,
        slot_idx: usize,
        descriptor: &PluginDescriptor,
    ) -> Result<(), String> {
        if track_idx >= 16 {
            return Err(format!("Pista {track_idx} fuera de rango (máx 16)"));
        }
        // La cinta anterior sale del registro antes de tocar la cadena: si el
        // slot se reemplaza, el editor que quedó escuchando la vieja tiene que
        // quedarse sin blocks, no con audio del plugin anterior.
        hikaru_plugin_host::tap::unregister((track_idx, slot_idx));

        let chain = &mut self.track_effects[track_idx];

        // Reemplazar si ya hay un plugin en ese slot.
        if let Some(pos) = chain.iter().position(|n| n.slot_idx == slot_idx) {
            chain.remove(pos);
        }

        let mut instance: Box<dyn PluginInstance> = match descriptor.format {
            PluginFormat::VST3 => {
                match hikaru_plugin_host::Vst3Instance::load(&descriptor.path) {
                    Ok(inst) => Box::new(inst),
                    Err(e) => return Err(format!("VST3: {e}")),
                }
            }
            PluginFormat::CLAP => {
                match hikaru_plugin_host::ClapInstance::load(&descriptor.path) {
                    Ok(inst) => Box::new(inst),
                    Err(e) => return Err(format!("CLAP: {e}")),
                }
            }
        };

        // El plugin se acaba de instanciar, así que el `set_sample_rate` que hizo el
        // motor al arrancar (main.rs, antes de que existiera este plugin) no lo alcanza.
        // Sin esto cada uno arranca con el default de su cargador y VST3, que sella la
        // tasa en `setupProcessing`, queda corriendo a 44100 Hz mientras el motor
        // reproduce a la del dispositivo.
        instance.set_sample_rate(self.sample_rate);

        let category = match descriptor.format {
            PluginFormat::VST3 => hikaru_dsp::PluginCategory::Vst3Fx,
            PluginFormat::CLAP => hikaru_dsp::PluginCategory::ClapFx,
        };

        let mut node = hikaru_dsp::DspPluginNode::new(
            descriptor.name.clone(),
            category,
            instance,
        )
        .with_slot_idx(slot_idx);

        // Cinta hacia la ventana del editor nativo, si algún día se abre. Se
        // registra acá y no cuando se abre la ventana porque el registro tiene
        // que existir del lado del motor (que publica) y del lado de la GUI (que
        // busca), y el orden de apertura es al revés.
        let tap = std::sync::Arc::new(hikaru_plugin_host::PluginTap::new(
            self.sample_rate,
            hikaru_plugin_host::scratch::MAX_BLOCK_FRAMES,
        ));
        hikaru_plugin_host::tap::register((track_idx, slot_idx), std::sync::Arc::clone(&tap));
        node.set_tap(Some(tap));

        let insert_pos = chain
            .iter()
            .position(|n| n.slot_idx > slot_idx)
            .unwrap_or(chain.len());
        chain.insert(insert_pos, node);
        self.prepare_pdc_buffers();

        println!(
            "[AudioEngine] Plugin '{}' cargado en pista {} slot {} (PDC max={} samples)",
            descriptor.name, track_idx, slot_idx, self.current_pdc_latency
        );
        Ok(())
    }

    pub fn load_native_effect(
        &mut self,
        track_idx: usize,
        slot_idx: usize,
        name: &str,
    ) -> Result<(), String> {
        if track_idx >= 16 {
            return Err(format!("Índice de pista fuera de rango: {track_idx}"));
        }
        hikaru_plugin_host::tap::unregister((track_idx, slot_idx));

        let chain = &mut self.track_effects[track_idx];
        if let Some(pos) = chain.iter().position(|n| n.slot_idx == slot_idx) {
            chain.remove(pos);
        } else if chain.iter().any(|n| n.name == name) {
            return Ok(());
        }

        let instance: Box<dyn PluginInstance> = match name {
            "Hikaru Ultracomb" | "Hikaru Ultracomb Filter" | "Ultracomb" => {
                Box::new(hikaru_dsp::effects::UltracombFilter::new(self.sample_rate))
            }
            _ => return Err(format!("Efecto nativo desconocido: {name}")),
        };

        let node = hikaru_dsp::DspPluginNode::new(
            name.to_string(),
            hikaru_dsp::PluginCategory::Vst3Fx,
            instance,
        )
        .with_slot_idx(slot_idx);
        let insert_pos = chain
            .iter()
            .position(|n| n.slot_idx > slot_idx)
            .unwrap_or(chain.len());
        chain.insert(insert_pos, node);
        println!(
            "[AudioEngine] Efecto nativo '{}' cargado en pista {} slot {}",
            name, track_idx, slot_idx
        );
        Ok(())
    }

    /// Quita un plugin externo de un slot y desplaza los `slot_idx` posteriores
    /// para mantener la correspondencia exacta con `Track::effects` en la GUI.
    pub fn remove_external_plugin(&mut self, track_idx: usize, slot_idx: usize) {
        if track_idx >= 16 {
            return;
        }
        let chain = &mut self.track_effects[track_idx];
        // Limpiar todos los taps viejos de esta pista antes de re-indexar.
        for n in chain.iter() {
            hikaru_plugin_host::tap::unregister((track_idx, n.slot_idx));
        }
        hikaru_plugin_host::tap::unregister((track_idx, slot_idx));

        if let Some(pos) = chain.iter().position(|n| n.slot_idx == slot_idx) {
            chain.remove(pos);
            println!(
                "[AudioEngine] Plugin removido de pista {} slot {}",
                track_idx, slot_idx
            );
        }
        for n in chain.iter_mut() {
            if n.slot_idx > slot_idx {
                n.slot_idx -= 1;
            }
            if let Some(tap) = &n.tap {
                hikaru_plugin_host::tap::register((track_idx, n.slot_idx), std::sync::Arc::clone(tap));
            }
        }
    }

    /// Activa o desactiva (bypass) un plugin externo sin destruirlo.
    pub fn set_external_plugin_active(&mut self, track_idx: usize, slot_idx: usize, active: bool) {
        if track_idx >= 16 {
            return;
        }
        if let Some(node) = self.track_effects[track_idx]
            .iter_mut()
            .find(|n| n.slot_idx == slot_idx)
        {
            node.is_active = active;
        }
    }

    /// Mueve un plugin externo de una pista/slot a otra pista/slot.
    pub fn move_external_plugin(
        &mut self,
        from_track: usize,
        from_slot: usize,
        to_track: usize,
        to_slot: usize,
    ) -> bool {
        if from_track >= 16 || to_track >= 16 {
            return false;
        }
        let Some(pos) = self.track_effects[from_track]
            .iter()
            .position(|n| n.slot_idx == from_slot)
            .or_else(|| (from_slot < self.track_effects[from_track].len()).then_some(from_slot))
        else {
            return false;
        };

        for n in &self.track_effects[from_track] {
            hikaru_plugin_host::tap::unregister((from_track, n.slot_idx));
        }
        let mut node = self.track_effects[from_track].remove(pos);

        // Desplazar slots en pista origen y re-registrar taps
        for n in self.track_effects[from_track].iter_mut() {
            if n.slot_idx > from_slot {
                n.slot_idx -= 1;
            }
            if let Some(tap) = &n.tap {
                hikaru_plugin_host::tap::register((from_track, n.slot_idx), std::sync::Arc::clone(tap));
            }
        }

        // Desplazar slots en pista destino >= to_slot e insertar el nodo movido
        for n in &self.track_effects[to_track] {
            hikaru_plugin_host::tap::unregister((to_track, n.slot_idx));
        }
        for n in self.track_effects[to_track].iter_mut() {
            if n.slot_idx >= to_slot {
                n.slot_idx += 1;
            }
        }
        node.slot_idx = to_slot;
        let insert_pos = self.track_effects[to_track]
            .iter()
            .position(|n| n.slot_idx > to_slot)
            .unwrap_or(self.track_effects[to_track].len());
        self.track_effects[to_track].insert(insert_pos, node);

        for n in self.track_effects[to_track].iter() {
            if let Some(tap) = &n.tap {
                hikaru_plugin_host::tap::register((to_track, n.slot_idx), std::sync::Arc::clone(tap));
            }
        }

        println!(
            "[AudioEngine] Plugin movido de pista {} slot {} a pista {} slot {}",
            from_track, from_slot, to_track, to_slot
        );
        true
    }

    /// Actualiza un parámetro de un plugin externo en la cadena.
    pub fn set_external_plugin_param(
        &mut self,
        track_idx: usize,
        slot_idx: usize,
        param_id: u32,
        value: f32,
    ) {
        if track_idx < 16 {
            if let Some(node) = self.track_effects[track_idx]
                .iter_mut()
                .find(|n| n.slot_idx == slot_idx)
            {
                node.set_param_normalized(param_id, value);
            } else if slot_idx < self.track_effects[track_idx].len() {
                let node = &mut self.track_effects[track_idx][slot_idx];
                node.set_param_normalized(param_id, value);
            } else if let Some(node) = self.track_effects[track_idx]
                .iter_mut()
                .find(|n| n.name == "Hikaru Ultracomb")
            {
                node.set_param_normalized(param_id, value);
            }
        }
    }

    /// Aplica modulación no destructiva (CLAP `ParamModEvent`) a un parámetro
    /// de plugin externo.
    pub fn set_external_plugin_param_mod(
        &mut self,
        track_idx: usize,
        slot_idx: usize,
        param_id: u32,
        delta: f32,
    ) {
        if track_idx < 16 {
            if let Some(node) = self.track_effects[track_idx]
                .iter_mut()
                .find(|n| n.slot_idx == slot_idx)
            {
                node.set_param_modulation(param_id, delta);
            }
        }
    }

    /// Sincroniza parámetros pendientes (dirty) de un slot con el motor.
    pub fn sync_external_plugin_params(
        &mut self,
        track_idx: usize,
        slot_idx: usize,
        params: &[(u32, f32)],
    ) {
        if track_idx < 16 {
            if let Some(node) = self.track_effects[track_idx]
                .iter_mut()
                .find(|n| n.slot_idx == slot_idx)
            {
                node.sync_params(params);
            } else if slot_idx < self.track_effects[track_idx].len() {
                let node = &mut self.track_effects[track_idx][slot_idx];
                node.sync_params(params);
            }
        }
    }

    /// Restaura el estado binario completo (preset + ajustes internos) de un
    /// plugin externo en el motor de audio y recalcula buffers PDC si cambió su latencia.
    pub fn load_external_plugin_state(
        &mut self,
        track_idx: usize,
        slot_idx: usize,
        state: &[u8],
    ) {
        if track_idx >= 16 || state.is_empty() {
            return;
        }
        let pos = self.track_effects[track_idx]
            .iter()
            .position(|n| n.slot_idx == slot_idx)
            .unwrap_or(slot_idx);
        if let Some(node) = self.track_effects[track_idx].get_mut(pos) {
            if let Err(e) = node.load_state(state) {
                eprintln!("[AudioEngine] load_state '{}' falló: {e}", node.name);
            } else {
                self.prepare_pdc_buffers();
            }
        }
    }

    /// Devuelve la lista de parámetros de un plugin cargado (para la UI).
    pub fn external_plugin_params(
        &mut self,
        track_idx: usize,
        slot_idx: usize,
    ) -> Option<Vec<(u32, String, f32)>> {
        if track_idx < 16 {
            let pos = self.track_effects[track_idx]
                .iter()
                .position(|n| n.slot_idx == slot_idx)
                .unwrap_or(slot_idx);
            let node = self.track_effects[track_idx].get_mut(pos)?;
            let mut result = Vec::new();
            for i in 0..node.instance.param_count() {
                if let Some(info) = node.instance.param_info(i) {
                    result.push((info.id, info.name.clone(), info.default_value));
                }
            }
            Some(result)
        } else {
            None
        }
    }

    pub fn set_track_volume(&self, track_idx: usize, volume: f32) {
        if track_idx < 16 {
            self.track_volumes[track_idx].store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        }
    }

    pub fn set_track_pan(&self, track_idx: usize, pan: f32) {
        if track_idx < 16 {
            // La GUI trabaja en rango normalizado -1.0..=1.0 (mixer, arranger,
            // matrix) pero históricamente el engine guardaba -100..100.
            // Aceptamos ambas escalas: |pan| <= 1.5 se interpreta como
            // normalizado y se escala a -100..100; si no, se usa tal cual.
            // Sin esto, un paneo full-R (1.0) se guardaba como 1.0/100 = 0.01
            // en `process()` y el paneo parecía no andar.
            let raw = if pan.abs() <= 1.5 { pan * 100.0 } else { pan };
            self.track_pans[track_idx].store(raw.clamp(-100.0, 100.0).to_bits(), Ordering::Relaxed);
        }
    }

    pub fn set_track_mute(&self, track_idx: usize, mute: bool) {
        if track_idx < 16 {
            self.track_mutes[track_idx].store(mute, Ordering::Relaxed);
        }
    }

    pub fn set_track_solo(&self, track_idx: usize, solo: bool) {
        if track_idx < 16 {
            self.track_solos[track_idx].store(solo, Ordering::Relaxed);
        }
    }

    pub fn set_master_gain(&self, volume: f32) {
        self.master_gain.store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn set_clip_source_bpm(&mut self, clip_id: usize, source_bpm: Option<f32>) {
        let bpm = self.transport.bpm;
        if let Some(c) = self.clips.iter_mut().find(|c| c.id == clip_id) {
            c.source_bpm = source_bpm;
            if let Some(sbpm) = source_bpm {
                if sbpm > 0.0 && bpm > 0.0 {
                    let rate = (bpm / sbpm as f64).clamp(0.25, 4.0);
                    if (rate - 1.0).abs() >= 1e-4 {
                        let (src, ch) = if !c.samples.is_empty() {
                            (&c.samples[..], c.channels)
                        } else if let Some(ev) = c.events.first() {
                            (&ev.samples[..], ev.channels)
                        } else {
                            (&[][..], c.channels)
                        };
                        if !src.is_empty() {
                            c.stretched_samples = time_stretch_wsola(src, ch, rate);
                            c.stretched_rate = rate;
                        }
                    } else {
                        c.stretched_samples.clear();
                        c.stretched_rate = 1.0;
                    }
                }
            } else {
                c.stretched_samples.clear();
                c.stretched_rate = 1.0;
            }
        }
    }

    pub fn set_clip_source_bpm_deferred(&mut self, clip_id: usize, source_bpm: Option<f32>) {
        if let Some(c) = self.clips.iter_mut().find(|c| c.id == clip_id) {
            c.source_bpm = source_bpm;
            if source_bpm.is_none() && !c.has_active_warp() && !c.has_active_pitch_or_formant() {
                c.stretched_samples.clear();
                c.stretched_rate = 1.0;
                c.stretched_warp_markers.clear();
                c.stretched_pitch.clear();
                c.stretched_formant.clear();
            }
        }
    }

    pub fn set_clip_warp_markers_deferred(
        &mut self,
        clip_id: usize,
        source_bpm: Option<f32>,
        warp_markers: Vec<(f64, f64)>,
    ) {
        let bpm = self.transport.bpm;
        if let Some(c) = self.clips.iter_mut().find(|c| c.id == clip_id) {
            c.source_bpm = source_bpm;
            c.warp_markers = sanitize_warp_markers(&warp_markers);
            let rate = match source_bpm {
                Some(sbpm) if sbpm > 0.0 && bpm > 0.0 => (bpm / sbpm as f64).clamp(0.25, 4.0),
                _ => 1.0,
            };
            if (rate - 1.0).abs() < 1e-4 && !c.has_active_warp() && !c.has_active_pitch_or_formant() {
                c.stretched_samples.clear();
                c.stretched_rate = 1.0;
                c.stretched_warp_markers.clear();
                c.stretched_pitch.clear();
                c.stretched_formant.clear();
            }
        }
    }

    pub fn set_clip_expressions_deferred(
        &mut self,
        clip_id: usize,
        expressions: hikaru_core::AudioClipExpressions,
    ) {
        let bpm = self.transport.bpm;
        if let Some(c) = self.clips.iter_mut().find(|c| c.id == clip_id) {
            c.expressions = expressions;
            let rate = match c.source_bpm {
                Some(sbpm) if sbpm > 0.0 && bpm > 0.0 => (bpm / sbpm as f64).clamp(0.25, 4.0),
                _ => 1.0,
            };
            if (rate - 1.0).abs() < 1e-4 && !c.has_active_warp() && !c.has_active_pitch_or_formant() {
                c.stretched_samples.clear();
                c.stretched_rate = 1.0;
                c.stretched_warp_markers.clear();
                c.stretched_pitch.clear();
                c.stretched_formant.clear();
            }
        }
    }

    /// Devuelve los trabajos de síntesis en segundo plano para clips cuyo `source_bpm`,
    /// `warp_markers`, `pitch` o `formant` requiere recalcular WSOLA/STFT.
    pub fn pending_stretch_jobs(&self) -> Vec<StretchJob> {
        let mut jobs = Vec::new();
        for c in &self.clips {
            let rate = c.tempo_rate(&self.transport);
            let active_warp = c.has_active_warp();
            let active_pf = c.has_active_pitch_or_formant();
            if (rate - 1.0).abs() >= 1e-4 || active_warp || active_pf {
                let dirty = (rate - c.stretched_rate).abs() > 0.002
                    || c.warp_markers != c.stretched_warp_markers
                    || c.expressions.pitch.points != c.stretched_pitch
                    || c.expressions.formant.points != c.stretched_formant
                    || c.stretched_samples.is_empty();
                if dirty {
                    let (src, ch) = if !c.samples.is_empty() {
                        (c.samples.clone(), c.channels)
                    } else if let Some(ev) = c.events.first() {
                        (ev.samples.clone(), ev.channels)
                    } else {
                        continue;
                    };
                    if !src.is_empty() {
                        jobs.push(StretchJob {
                            clip_id: c.id,
                            samples: src,
                            channels: ch,
                            rate,
                            warp_markers: c.warp_markers.clone(),
                            pitch_curve: c.expressions.pitch.clone(),
                            formant_curve: c.expressions.formant.clone(),
                        });
                    }
                }
            }
        }
        jobs
    }

    pub fn apply_stretched_samples(
        &mut self,
        clip_id: usize,
        samples: Vec<f32>,
        rate: f64,
        warp_markers: Vec<(f64, f64)>,
        pitch_points: Vec<hikaru_core::ExpressionPoint>,
        formant_points: Vec<hikaru_core::ExpressionPoint>,
    ) {
        if let Some(c) = self.clips.iter_mut().find(|c| c.id == clip_id) {
            c.stretched_samples = samples;
            c.stretched_rate = rate;
            c.stretched_warp_markers = warp_markers;
            c.stretched_pitch = pitch_points;
            c.stretched_formant = formant_points;
        }
    }

    pub fn sync_clips(&mut self, new_clips: Vec<AudioClipInstance>) {
        self.clips = new_clips;
    }

    pub fn update_clip_bounds(
        &mut self,
        clip_id: usize,
        track_index: usize,
        scene_index: usize,
        start_secs: f32,
        duration_secs: f32,
        offset_secs: f32,
    ) {
        let is_studio = self.mode == EngineMode::OpenStudio;
        if let Some(clip) = self
            .clips
            .iter_mut()
            .find(|c| {
                if is_studio {
                    c.id == clip_id
                } else {
                    c.track_index == track_index && c.scene_index == scene_index
                }
            })
        {
            clip.start_frame = (start_secs * self.sample_rate) as u64;
            let natural = clip.natural_frames();
            clip.duration_frames = if duration_secs > 0.0 {
                (duration_secs * self.sample_rate) as u64
            } else {
                natural
            };
            clip.sample_offset = (offset_secs * self.sample_rate) as usize * clip.channels;
        }
    }

    pub fn set_clip_loop(
        &mut self,
        track_index: usize,
        scene_index: usize,
        start_secs: f32,
        end_secs: f32,
        enabled: bool,
    ) {
        let sr = self.sample_rate.max(1.0);
        if let Some(clip) = self
            .clips
            .iter_mut()
            .find(|c| c.track_index == track_index && c.scene_index == scene_index)
        {
            let natural = clip.natural_frames();
            let start = (start_secs.max(0.0) * sr) as u64;
            let mut end = (end_secs.max(0.0) * sr) as u64;
            if natural > 0 {
                end = end.min(natural);
            }
            if enabled && start < natural {
                // Región vacía (`end <= start`) NO significa "sin loop": un clip
                // con el Loop activado y sin región elegida loopea el clip
                // entero. Es la semántica que ya usan `add_clip` (default de
                // los pads de Matrix) y `Clip::new` en `hikaru_sequencer`.
                //
                // Tratar `end <= start` como "desactivar" rompía el loop justo al
                // abrir un proyecto: `apply_matrix` manda `SetClipLoop` para
                // cada clip con (0, 0, enabled=true) — que es lo que producen
                // los pads recién cargados — y eso apagaba el loop que
                // `add_clip` acababa de dejar puesto.
                let loop_end = if end > start {
                    end.min(natural)
                } else {
                    natural
                };
                if loop_end > start {
                    clip.clip_loop_enabled = true;
                    clip.clip_loop_start = start;
                    clip.clip_loop_end = loop_end;
                } else {
                    clip.clip_loop_enabled = false;
                    clip.clip_loop_start = 0;
                    clip.clip_loop_end = 0;
                }
            } else {
                clip.clip_loop_enabled = false;
                clip.clip_loop_start = 0;
                clip.clip_loop_end = 0;
            }
        }
    }

    pub fn add_clip(
        &mut self,
        id: usize,
        track_index: usize,
        scene_index: usize,
        samples: Vec<f32>,
        start_secs: f32,
        duration_secs: f32,
        offset_secs: f32,
        channels: usize,
        is_matrix_clip: bool,
    ) {
        let ch = channels.max(1);
        let start_frame = (start_secs * self.sample_rate) as u64;
        let natural_frames = (samples.len() / ch) as u64;
        let duration_frames = if duration_secs > 0.0 {
            (duration_secs * self.sample_rate) as u64
        } else {
            natural_frames
        };
        let sample_offset = (offset_secs * self.sample_rate) as usize * ch;

        // Los clips de la Session Matrix loopear por defecto (clip entero).
        // Los de Playlist/Studio mantienen one-shot salvo SetClipLoop explícito.
        let (default_loop_enabled, default_loop_end) = if is_matrix_clip && natural_frames > 0 {
            (true, natural_frames)
        } else {
            (false, 0)
        };

        let instance = AudioClipInstance {
            id,
            track_index,
            scene_index,
            samples,
            start_frame,
            start_absolute: 0,
            duration_frames,
            sample_offset,
            channels: ch,
            is_playing: false,
            clip_loop_enabled: default_loop_enabled,
            clip_loop_start: 0,
            clip_loop_end: default_loop_end,
            is_matrix_clip,
            legato_launch: None,
            launch_mode: ClipLaunchMode::Trigger,
            gain: 1.0,
            prev_frame: usize::MAX,
            xfade_remaining: 0,
            xfade_len: 0,
            events: Vec::new(),
            source_bpm: None,
            stretched_samples: Vec::new(),
            stretched_rate: 1.0,
            warp_markers: Vec::new(),
            stretched_warp_markers: Vec::new(),
            expressions: hikaru_core::AudioClipExpressions::default(),
            stretched_pitch: Vec::new(),
            stretched_formant: Vec::new(),
        };

        let is_studio = self.mode == EngineMode::OpenStudio;
        if let Some(existing) = self
            .clips
            .iter_mut()
            .find(|c| {
                if is_studio {
                    c.id == id
                } else {
                    c.track_index == track_index && c.scene_index == scene_index
                }
            })
        {
            *existing = instance;
        } else {
            self.clips.push(instance);
        }
    }

    /// Reemplaza los eventos editables de un pad (cortar/pegar/split/multi).
    /// Además actualiza el `samples` mezclado legacy para compatibilidad con
    /// el playhead y previsualizaciones que lean el buffer completo.
    pub fn set_clip_events(
        &mut self,
        track_index: usize,
        scene_index: usize,
        events: Vec<EngineAudioEvent>,
    ) {
        if let Some(clip) = self
            .clips
            .iter_mut()
            .find(|c| c.track_index == track_index && c.scene_index == scene_index)
        {
            // Si el loop cubría el clip entero antes de editar, debe seguir
            // cubriendo el timeline nuevo (caso apilar/split/paste).
            let was_full_loop = clip.clip_loop_enabled
                && clip.clip_loop_start == 0
                && (clip.clip_loop_end == 0
                    || clip.clip_loop_end
                        == (clip.samples.len() / clip.channels.max(1)) as u64
                    || (clip.events.is_empty()
                        && clip.clip_loop_end == clip.natural_frames()));
            clip.events = events;
            // Refrescar loop por defecto al tamaño del nuevo timeline.
            let natural = clip.natural_frames();
            if clip.clip_loop_enabled && natural > 0 {
                if clip.clip_loop_end == 0 || clip.clip_loop_end > natural || was_full_loop {
                    clip.clip_loop_start = 0;
                    clip.clip_loop_end = natural;
                }
            } else if natural > 0 && clip.clip_loop_end == 0 {
                // Los pads de Matrix looapean el clip entero por defecto.
                clip.clip_loop_enabled = true;
                clip.clip_loop_start = 0;
                clip.clip_loop_end = natural;
            }
            clip.prev_frame = usize::MAX;
            clip.xfade_remaining = 0;
        }
    }

    pub fn remove_clip(&mut self, clip_id: usize, track_index: usize, scene_index: usize) {
        let is_studio = self.mode == EngineMode::OpenStudio;
        self.clips.retain(|c| {
            if is_studio {
                c.id != clip_id
            } else {
                !(c.track_index == track_index && c.scene_index == scene_index)
            }
        });
    }

    pub fn stop_matrix_clip(&mut self, track_index: usize, scene_index: usize) {
        for clip in self.clips.iter_mut() {
            if clip.track_index == track_index && clip.scene_index == scene_index {
                clip.is_playing = false;
            }
        }
        for clip in self.midi_clips.iter_mut() {
            if clip.track_index == track_index && clip.scene_index == scene_index {
                clip.is_playing = false;
                for pitch in 0..128 {
                    if clip.active_notes[pitch] {
                        self.dms_voice_pool.kill_pad(dms_pad_key(track_index, pitch));
                        clip.active_notes[pitch] = false;
                    }
                }
            }
        }
    }

    pub fn clear_matrix_slot(&mut self, track_index: usize, scene_index: usize) {
        self.stop_matrix_clip(track_index, scene_index);
        self.clips.retain(|c| !(c.track_index == track_index && c.scene_index == scene_index));
        self.midi_clips.retain(|c| !(c.track_index == track_index && c.scene_index == scene_index));
    }

    pub fn move_matrix_clip(
        &mut self,
        src_track: usize,
        src_scene: usize,
        dst_track: usize,
        dst_scene: usize,
    ) {
        if src_track == dst_track && src_scene == dst_scene {
            return;
        }

        // Stop active voices on destination and remove existing clips there
        self.clear_matrix_slot(dst_track, dst_scene);

        // Stop active voices on source
        self.stop_matrix_clip(src_track, src_scene);

        // Move audio clips from src to dst
        for clip in self.clips.iter_mut() {
            if clip.track_index == src_track && clip.scene_index == src_scene {
                clip.track_index = dst_track;
                clip.scene_index = dst_scene;
                clip.is_playing = false;
            }
        }

        // Move midi clips from src to dst
        for clip in self.midi_clips.iter_mut() {
            if clip.track_index == src_track && clip.scene_index == src_scene {
                clip.track_index = dst_track;
                clip.scene_index = dst_scene;
                clip.is_playing = false;
            }
        }
    }

    pub fn copy_matrix_clip(
        &mut self,
        src_track: usize,
        src_scene: usize,
        dst_track: usize,
        dst_scene: usize,
        new_clip_id: usize,
    ) {
        if src_track == dst_track && src_scene == dst_scene {
            return;
        }

        self.clear_matrix_slot(dst_track, dst_scene);

        let audio_to_clone = self
            .clips
            .iter()
            .find(|c| c.track_index == src_track && c.scene_index == src_scene)
            .cloned();

        if let Some(mut clone) = audio_to_clone {
            clone.id = new_clip_id;
            clone.track_index = dst_track;
            clone.scene_index = dst_scene;
            clone.is_playing = false;
            self.clips.push(clone);
        }

        let midi_to_clone = self
            .midi_clips
            .iter()
            .find(|c| c.track_index == src_track && c.scene_index == src_scene)
            .cloned();

        if let Some(mut clone) = midi_to_clone {
            clone.track_index = dst_track;
            clone.scene_index = dst_scene;
            clone.is_playing = false;
            clone.clear_active_notes();
            self.midi_clips.push(clone);
        }
    }

    pub fn play(&mut self) {
        self.preview_player.stop();
        self.transport.playback_state = TransportPlaybackState::Playing;
    }

    pub fn pause(&mut self) {
        self.transport.playback_state = TransportPlaybackState::Paused;
        for clip in self.midi_clips.iter_mut() {
            for pitch in 0..128 {
                if clip.active_notes[pitch] {
                    self.dms_voice_pool.release_pad(dms_pad_key(clip.track_index, pitch));
                    clip.active_notes[pitch] = false;
                }
            }
        }
    }

    pub fn stop(&mut self) {
        self.transport.playback_state = TransportPlaybackState::Stopped;
        self.transport.sample_count = 0;
        self.clear_pdc_delays();
        self.pending_scene_launch = None;
        self.pending_scene_index_atomic.store(-1, Ordering::Relaxed);
        self.pending_track_stops.clear();
        self.pending_stop_tracks_mask.store(0, Ordering::Relaxed);
        self.wavetable_voice_pool.kill_all();
        self.sampler_voice_pool.kill_all();
        self.open_poly_voice_pool.kill_all();
        self.open_grid_voice_pool.kill_all();
        for clip in self.midi_clips.iter_mut() {
            for pitch in 0..128 {
                if clip.active_notes[pitch] {
                    self.dms_voice_pool.kill_pad(dms_pad_key(clip.track_index, pitch));
                    clip.active_notes[pitch] = false;
                }
            }
            clip.is_playing = false;
        }
    }

    /// Configura explícitamente el lanzamiento Legato de un clip (audio o MIDI).
    pub fn set_clip_legato(&mut self, track_index: usize, scene_index: usize, legato: bool) {
        for clip in self.clips.iter_mut() {
            if clip.track_index == track_index && clip.scene_index == scene_index {
                clip.legato_launch = Some(legato);
            }
        }
        for clip in self.midi_clips.iter_mut() {
            if clip.track_index == track_index && clip.scene_index == scene_index {
                clip.legato_launch = Some(legato);
            }
        }
    }

    /// Configura el modo de lanzamiento (`Trigger` / `Gate` / `Toggle`) de un clip.
    pub fn set_clip_launch_mode(
        &mut self,
        track_index: usize,
        scene_index: usize,
        mode: ClipLaunchMode,
    ) {
        for clip in self.clips.iter_mut() {
            if clip.track_index == track_index && clip.scene_index == scene_index {
                clip.launch_mode = mode;
            }
        }
        for clip in self.midi_clips.iter_mut() {
            if clip.track_index == track_index && clip.scene_index == scene_index {
                clip.launch_mode = mode;
            }
        }
        let _ = self.matrix.set_clip_launch_mode(track_index, scene_index, mode);
    }

    /// Configura la ganancia lineal (`0.0..=2.0`) de un clip de audio.
    pub fn set_clip_gain(&mut self, track_index: usize, scene_index: usize, gain: f32) {
        if !gain.is_finite() {
            return;
        }
        let clamped = gain.clamp(0.0, 2.0);
        for clip in self.clips.iter_mut() {
            if clip.track_index == track_index && clip.scene_index == scene_index {
                clip.gain = clamped;
            }
        }
        let _ = self.matrix.set_clip_gain(track_index, scene_index, clamped);
    }

    /// Libera (detiene) un clip en modo `Gate` al soltar el pad/botón.
    pub fn release_clip(&mut self, track_index: usize, scene_index: usize) {
        for clip in self.clips.iter_mut() {
            if clip.track_index == track_index
                && clip.scene_index == scene_index
                && clip.launch_mode == ClipLaunchMode::Gate
            {
                clip.is_playing = false;
            }
        }
        for clip in self.midi_clips.iter_mut() {
            if clip.track_index == track_index
                && clip.scene_index == scene_index
                && clip.launch_mode == ClipLaunchMode::Gate
                && clip.is_playing
            {
                clip.is_playing = false;
                for pitch in 0..128 {
                    if clip.active_notes[pitch] {
                        self.dms_voice_pool
                            .kill_pad(dms_pad_key(clip.track_index, pitch));
                        clip.active_notes[pitch] = false;
                    }
                }
            }
        }
        let _ = self.matrix.release_clip(track_index, scene_index);
    }

    /// Configura la cuantización global de lanzamiento de escenas.
    pub fn set_global_launch_quantization(&mut self, grid: QuantizationGrid) {
        self.global_launch_quantization = grid;
        self.matrix.set_global_quantization(grid);
    }

    /// Configura o limpia el override de cuantización de lanzamiento de una escena.
    pub fn set_scene_launch_quantization(
        &mut self,
        scene_idx: usize,
        grid: Option<QuantizationGrid>,
    ) {
        if scene_idx >= self.scene_quantization_overrides.len() {
            self.scene_quantization_overrides.resize(scene_idx + 1, None);
        }
        self.scene_quantization_overrides[scene_idx] = grid;
        self.matrix.set_scene_quantization(scene_idx, grid);
    }

    /// Cuantización efectiva de una escena (override si existe, si no global).
    pub fn effective_scene_quantization(&self, scene_idx: usize) -> QuantizationGrid {
        let override_grid = self
            .scene_quantization_overrides
            .get(scene_idx)
            .copied()
            .flatten();
        QuantizationEngine::resolve_scene_grid(self.global_launch_quantization, override_grid)
    }

    /// Escena actualmente en cola esperando su borde de cuantización.
    pub fn pending_scene_launch(&self) -> Option<PendingSceneLaunch> {
        self.pending_scene_launch
    }

    /// Agrega una escena nueva al final del motor y la matriz interna.
    pub fn add_scene(&mut self) {
        self.scene_quantization_overrides.push(None);
        let _ = self.matrix.add_scene();
    }

    /// Inserta una escena vacía en `scene_idx`, desplazando todos los clips
    /// y overrides de escenas posteriores (`>= scene_idx`) en +1.
    pub fn insert_scene(&mut self, scene_idx: usize) {
        for clip in self.clips.iter_mut() {
            if clip.is_matrix_clip && clip.scene_index >= scene_idx {
                clip.scene_index += 1;
            }
        }
        for clip in self.midi_clips.iter_mut() {
            if clip.is_matrix_clip && clip.scene_index >= scene_idx {
                clip.scene_index += 1;
            }
        }
        if let Some(pending) = &mut self.pending_scene_launch {
            if pending.scene_index >= scene_idx {
                pending.scene_index += 1;
                self.pending_scene_index_atomic
                    .store(pending.scene_index as i32, Ordering::Relaxed);
            }
        }
        let pos = scene_idx.min(self.scene_quantization_overrides.len());
        self.scene_quantization_overrides.insert(pos, None);
        let _ = self.matrix.insert_scene(scene_idx);
    }

    /// Elimina la escena `scene_idx`: detiene y borra sus clips (audio y MIDI),
    /// y desplaza los índices de escenas posteriores (`> scene_idx`) en -1.
    pub fn remove_scene(&mut self, scene_idx: usize) {
        for clip in self.clips.iter_mut() {
            if clip.is_matrix_clip && clip.scene_index == scene_idx {
                clip.is_playing = false;
            }
        }
        for clip in self.midi_clips.iter_mut() {
            if clip.is_matrix_clip && clip.scene_index == scene_idx {
                clip.is_playing = false;
                for pitch in 0..128 {
                    if clip.active_notes[pitch] {
                        self.dms_voice_pool
                            .kill_pad(dms_pad_key(clip.track_index, pitch));
                        clip.active_notes[pitch] = false;
                    }
                }
            }
        }
        self.clips
            .retain(|c| !(c.is_matrix_clip && c.scene_index == scene_idx));
        self.midi_clips
            .retain(|c| !(c.is_matrix_clip && c.scene_index == scene_idx));

        for clip in self.clips.iter_mut() {
            if clip.is_matrix_clip && clip.scene_index > scene_idx {
                clip.scene_index -= 1;
            }
        }
        for clip in self.midi_clips.iter_mut() {
            if clip.is_matrix_clip && clip.scene_index > scene_idx {
                clip.scene_index -= 1;
            }
        }

        if let Some(pending) = &mut self.pending_scene_launch {
            if pending.scene_index == scene_idx {
                self.pending_scene_launch = None;
                self.pending_scene_index_atomic.store(-1, Ordering::Relaxed);
            } else if pending.scene_index > scene_idx {
                pending.scene_index -= 1;
                self.pending_scene_index_atomic
                    .store(pending.scene_index as i32, Ordering::Relaxed);
            }
        }

        if scene_idx < self.scene_quantization_overrides.len()
            && self.scene_quantization_overrides.len() > 1
        {
            self.scene_quantization_overrides.remove(scene_idx);
        }
        let _ = self.matrix.remove_scene(scene_idx);
    }

    /// Reordena la escena `from_idx` a la posición `to_idx`: reindexa todos los
    /// clips de matriz (audio y MIDI), la escena pendiente en cola y los
    /// overrides de cuantización.
    pub fn move_scene(&mut self, from_idx: usize, to_idx: usize) {
        if from_idx == to_idx
            || from_idx >= self.matrix.scene_count
            || to_idx >= self.matrix.scene_count
        {
            return;
        }
        let remap = |idx: usize| -> usize {
            if idx == from_idx {
                to_idx
            } else if from_idx < to_idx && idx > from_idx && idx <= to_idx {
                idx - 1
            } else if to_idx < from_idx && idx >= to_idx && idx < from_idx {
                idx + 1
            } else {
                idx
            }
        };

        for clip in self.clips.iter_mut() {
            if clip.is_matrix_clip {
                clip.scene_index = remap(clip.scene_index);
            }
        }
        for clip in self.midi_clips.iter_mut() {
            if clip.is_matrix_clip {
                clip.scene_index = remap(clip.scene_index);
            }
        }
        if let Some(pending) = &mut self.pending_scene_launch {
            pending.scene_index = remap(pending.scene_index);
            self.pending_scene_index_atomic
                .store(pending.scene_index as i32, Ordering::Relaxed);
        }
        if from_idx < self.scene_quantization_overrides.len()
            && to_idx < self.scene_quantization_overrides.len()
        {
            let ov = self.scene_quantization_overrides.remove(from_idx);
            self.scene_quantization_overrides.insert(to_idx, ov);
        }
        let _ = self.matrix.move_scene(from_idx, to_idx);
    }

    pub fn set_global_loop(&mut self, start_samples: u64, end_samples: u64, enabled: bool) {
        self.transport.set_loop_region_samples(start_samples, end_samples);
        self.transport.set_loop_enabled(enabled);
    }

    pub fn seek(&mut self, position_secs: f32) {
        self.transport.sample_count = (position_secs * self.sample_rate) as u64;
        self.clear_pdc_delays();
    }

    /// Disparo legato de la Session Matrix: el loopeo entre pads es continuo.
    ///
    /// Antes cada pad arrancaba con `start = now` (playhead interno desde
    /// cero), por eso al cambiar de pad en caliente se escuchaba
    /// desincronizado. Ahora el pad nuevo hereda el origen de fase
    /// (`start_frame` / `start_absolute`) del clip que venía sonando:
    /// es como si todos los pads hubieran estado loopeando muteados en
    /// segundo plano y el trigger solo desmuteara en la fase correcta.
    ///
    /// - Prioridad 1: clip sonando en el mismo track (caso "pad de abajo").
    /// - Prioridad 2: clip sonando más antiguo de cualquier otro track
    ///   (caso "cualquier pad").
    /// - Sin nada sonando: arranca desde cero como antes (`now`).
    pub fn trigger_clip(&mut self, track_index: usize, scene_index: usize) {
        self.cancel_pending_track_stop(track_index);
        let now = self.transport.sample_count;
        let now_abs = self.absolute_frame;
        let target_audio_playing = self
            .clips
            .iter()
            .find(|c| c.track_index == track_index && c.scene_index == scene_index)
            .map(|c| c.is_playing)
            .unwrap_or(false);
        let target_midi_playing = self
            .midi_clips
            .iter()
            .find(|c| c.track_index == track_index && c.scene_index == scene_index)
            .map(|c| c.is_playing)
            .unwrap_or(false);
        let target_was_playing = target_audio_playing || target_midi_playing;

        // Capturar origen de referencia ANTES de mutar (legato).
        let legato_origin: Option<(u64, u64)> = if target_was_playing {
            None
        } else {
            self.legato_reference(Some(track_index))
        };
        for clip in self.clips.iter_mut().filter(|c| c.track_index == track_index) {
            if clip.scene_index == scene_index {
                clip.is_playing = match clip.launch_mode {
                    ClipLaunchMode::Toggle => !target_was_playing,
                    ClipLaunchMode::Trigger | ClipLaunchMode::Gate => true,
                };
                if clip.is_playing {
                    let use_legato = clip.legato_launch.unwrap_or(true);
                    if use_legato {
                        if let Some((ref_frame, ref_abs)) = legato_origin {
                            clip.start_frame = ref_frame;
                            clip.start_absolute = ref_abs;
                        } else {
                            clip.start_frame = now;
                            clip.start_absolute = now_abs;
                        }
                    } else {
                        clip.start_frame = now;
                        clip.start_absolute = now_abs;
                    }
                    clip.prev_frame = usize::MAX;
                    clip.xfade_remaining = 0;
                    clip.xfade_len = 0;
                }
            } else {
                clip.is_playing = false;
            }
        }
        for clip in self.midi_clips.iter_mut().filter(|c| c.track_index == track_index) {
            if clip.scene_index == scene_index {
                clip.is_playing = match clip.launch_mode {
                    ClipLaunchMode::Toggle => !target_was_playing,
                    ClipLaunchMode::Trigger | ClipLaunchMode::Gate => true,
                };
                if clip.is_playing {
                    let use_legato = clip.legato_launch.unwrap_or(true);
                    if use_legato {
                        if let Some((ref_frame, ref_abs)) = legato_origin {
                            clip.start_frame = ref_frame;
                            clip.start_absolute = ref_abs;
                        } else {
                            clip.start_frame = now;
                            clip.start_absolute = now_abs;
                        }
                    } else {
                        clip.start_frame = now;
                        clip.start_absolute = now_abs;
                    }
                    clip.clear_active_notes();
                } else {
                    for pitch in 0..128 {
                        if clip.active_notes[pitch] {
                            self.dms_voice_pool.release_pad(dms_pad_key(clip.track_index, pitch));
                            clip.active_notes[pitch] = false;
                        }
                    }
                }
            } else {
                clip.is_playing = false;
                for pitch in 0..128 {
                    if clip.active_notes[pitch] {
                        self.dms_voice_pool.release_pad(dms_pad_key(clip.track_index, pitch));
                        clip.active_notes[pitch] = false;
                    }
                }
            }
        }
    }

    /// Disparo imperativo sin toggle: asegura que el clip especificado quede
    /// en reproducción activa (`is_playing = true`) y sincronizado con el transporte.
    pub fn start_clip(&mut self, track_index: usize, scene_index: usize) {
        let now = self.transport.sample_count;
        let now_abs = self.absolute_frame;
        let mut found_midi = false;
        for clip in self.midi_clips.iter_mut().filter(|c| c.track_index == track_index) {
            if clip.scene_index == scene_index {
                clip.is_playing = true;
                clip.start_frame = now;
                clip.start_absolute = now_abs;
                clip.clear_active_notes();
                found_midi = true;
            } else {
                clip.is_playing = false;
                for pitch in 0..128 {
                    if clip.active_notes[pitch] {
                        self.dms_voice_pool.release_pad(dms_pad_key(clip.track_index, pitch));
                        clip.active_notes[pitch] = false;
                    }
                }
            }
        }
        if !found_midi {
            let mut new_clip = MidiClipInstance::new(track_index, scene_index, Vec::new());
            new_clip.is_playing = true;
            new_clip.start_frame = now;
            new_clip.start_absolute = now_abs;
            self.midi_clips.push(new_clip);
        }

        for clip in self.clips.iter_mut().filter(|c| c.track_index == track_index) {
            if clip.scene_index == scene_index {
                clip.is_playing = true;
                clip.start_frame = now;
                clip.start_absolute = now_abs;
                clip.prev_frame = usize::MAX;
                clip.xfade_remaining = 0;
                clip.xfade_len = 0;
            } else {
                clip.is_playing = false;
            }
        }
    }

    /// Origen de fase a heredar para un arranque legato.
    ///
    /// Devuelve `(start_frame, start_absolute)` del clip en `Playing` más
    /// antiguo: primero busca en `preferred_track` (cambio de pad dentro
    /// del mismo track), y si no hay nada sonando ahí, cae al más antiguo
    /// de cualquier track (sincronía global entre pads).
    fn legato_reference(&self, preferred_track: Option<usize>) -> Option<(u64, u64)> {
        if let Some(t) = preferred_track {
            let same_track_audio = self
                .clips
                .iter()
                .filter(|c| c.track_index == t && c.is_playing)
                .min_by_key(|c| c.start_absolute)
                .map(|c| (c.start_frame, c.start_absolute));
            if same_track_audio.is_some() {
                return same_track_audio;
            }
            let same_track_midi = self
                .midi_clips
                .iter()
                .filter(|c| c.track_index == t && c.is_playing)
                .min_by_key(|c| c.start_absolute)
                .map(|c| (c.start_frame, c.start_absolute));
            if same_track_midi.is_some() {
                return same_track_midi;
            }
        }
        let oldest_audio = self.clips
            .iter()
            .filter(|c| c.is_playing)
            .min_by_key(|c| c.start_absolute)
            .map(|c| (c.start_frame, c.start_absolute));
        let oldest_midi = self.midi_clips
            .iter()
            .filter(|c| c.is_playing)
            .min_by_key(|c| c.start_absolute)
            .map(|c| (c.start_frame, c.start_absolute));
        match (oldest_audio, oldest_midi) {
            (Some(a), Some(m)) => if a.1 <= m.1 { Some(a) } else { Some(m) },
            (Some(a), None) => Some(a),
            (None, Some(m)) => Some(m),
            (None, None) => None,
        }
    }

    /// Disparo de escena con soporte de cuantización global + override por escena
    /// y modo Legato por clip.
    ///
    /// - Si la cuantización efectiva (`scene_override.unwrap_or(global)`) es
    ///   `QuantizationGrid::None` o el transporte no está reproduciendo, dispara
    ///   de inmediato.
    /// - Si está reproduciendo y falta tiempo hasta el próximo borde de grilla,
    ///   encola un [`PendingSceneLaunch`] que `process()` ejecutará exactamente
    ///   en ese sample.
    /// - Al ejecutarse la escena, los clips con `legato_launch == Some(true)`
    ///   heredan la fase del clip saliente; el resto reinicia en fase 0.
    pub fn trigger_scene(&mut self, scene_index: usize) {
        let grid = self.effective_scene_quantization(scene_index);
        if grid != QuantizationGrid::None
            && self.transport.playback_state == TransportPlaybackState::Playing
        {
            let wait = QuantizationEngine::samples_until_next_sync(
                &self.transport,
                grid,
                self.sample_rate as f64,
            );
            if wait > 0 {
                let target_sample = self.transport.sample_count.saturating_add(wait);
                self.pending_scene_launch = Some(PendingSceneLaunch {
                    scene_index,
                    target_sample,
                    grid,
                });
                self.pending_scene_index_atomic
                    .store(scene_index as i32, Ordering::Relaxed);
                return;
            }
        }
        self.pending_scene_launch = None;
        self.pending_scene_index_atomic.store(-1, Ordering::Relaxed);
        self.pending_track_stops.clear();
        self.pending_stop_tracks_mask.store(0, Ordering::Relaxed);
        let now = self.transport.sample_count;
        let now_abs = self.absolute_frame;
        self.execute_scene_launch(scene_index, now, now_abs);
    }

    fn execute_scene_launch(&mut self, scene_index: usize, now: u64, now_abs: u64) {
        let mut tracks_with_clip = std::collections::HashSet::new();
        for clip in self.clips.iter() {
            if clip.scene_index == scene_index {
                tracks_with_clip.insert(clip.track_index);
            }
        }
        for clip in self.midi_clips.iter() {
            if clip.scene_index == scene_index {
                tracks_with_clip.insert(clip.track_index);
            }
        }
        let mut track_legato_refs: std::collections::HashMap<usize, Option<(u64, u64)>> =
            std::collections::HashMap::new();
        for &ti in &tracks_with_clip {
            track_legato_refs.insert(ti, self.legato_reference(Some(ti)));
        }

        for clip in self.clips.iter_mut() {
            if clip.scene_index == scene_index {
                clip.is_playing = true;
                let use_legato = clip.legato_launch == Some(true);
                if use_legato {
                    if let Some(Some((ref_frame, ref_abs))) =
                        track_legato_refs.get(&clip.track_index)
                    {
                        clip.start_frame = *ref_frame;
                        clip.start_absolute = *ref_abs;
                    } else {
                        clip.start_frame = now;
                        clip.start_absolute = now_abs;
                    }
                } else {
                    clip.start_frame = now;
                    clip.start_absolute = now_abs;
                }
                clip.prev_frame = usize::MAX;
                clip.xfade_remaining = 0;
                clip.xfade_len = 0;
            } else if tracks_with_clip.contains(&clip.track_index) {
                clip.is_playing = false;
            }
        }
        for clip in self.midi_clips.iter_mut() {
            if clip.scene_index == scene_index {
                clip.is_playing = true;
                let use_legato = clip.legato_launch == Some(true);
                if use_legato {
                    if let Some(Some((ref_frame, ref_abs))) =
                        track_legato_refs.get(&clip.track_index)
                    {
                        clip.start_frame = *ref_frame;
                        clip.start_absolute = *ref_abs;
                    } else {
                        clip.start_frame = now;
                        clip.start_absolute = now_abs;
                    }
                } else {
                    clip.start_frame = now;
                    clip.start_absolute = now_abs;
                }
                clip.clear_active_notes();
            } else if tracks_with_clip.contains(&clip.track_index) {
                clip.is_playing = false;
                for pitch in 0..128 {
                    if clip.active_notes[pitch] {
                        self.dms_voice_pool.release_pad(dms_pad_key(clip.track_index, pitch));
                        clip.active_notes[pitch] = false;
                    }
                }
            }
        }
    }

    pub fn voice_active(&self, track_index: usize, scene_index: usize) -> bool {
        let audio_active = match self
            .clips
            .iter()
            .find(|c| c.track_index == track_index && c.scene_index == scene_index)
        {
            None => false,
            Some(clip) => {
                clip.is_playing && clip.voice_state_linear(self.absolute_frame, &self.transport) == VoiceState::Active
            }
        };

        if audio_active {
            return true;
        }

        match self
            .midi_clips
            .iter()
            .find(|c| c.track_index == track_index && c.scene_index == scene_index)
        {
            None => false,
            Some(clip) => {
                if !clip.is_playing {
                    return false;
                }
                if clip.clip_loop_enabled {
                    true
                } else {
                    let emission = clip.emission_len_frames(&self.transport);
                    let elapsed = self.absolute_frame.saturating_sub(clip.start_absolute);
                    elapsed < emission
                }
            }
        }
    }

    pub fn voice_elapsed_frames(
        &self,
        track_index: usize,
        scene_index: usize,
    ) -> Option<u64> {
        if let Some(clip) = self
            .clips
            .iter()
            .find(|c| c.track_index == track_index && c.scene_index == scene_index)
        {
            if clip.is_playing && clip.voice_state_linear(self.absolute_frame, &self.transport) == VoiceState::Active {
                return Some(self.absolute_frame.saturating_sub(clip.start_absolute));
            }
        }
        if let Some(clip) = self
            .midi_clips
            .iter()
            .find(|c| c.track_index == track_index && c.scene_index == scene_index)
        {
            if clip.is_playing {
                let elapsed = self.absolute_frame.saturating_sub(clip.start_absolute);
                if clip.clip_loop_enabled || elapsed < clip.emission_len_frames(&self.transport) {
                    return Some(elapsed);
                }
            }
        }
        None
    }

    /// Posición exacta del playhead dentro del clip: `(frame_actual, duración_natural)`.
    /// `None` si no existe el clip, no está sonando o la voz ya terminó.
    pub fn voice_playhead_frame(
        &self,
        track_index: usize,
        scene_index: usize,
    ) -> Option<(u64, u64)> {
        if let Some(clip) = self
            .clips
            .iter()
            .find(|c| c.track_index == track_index && c.scene_index == scene_index)
        {
            if clip.is_playing && clip.voice_state_linear(self.absolute_frame, &self.transport) == VoiceState::Active {
                let frame = clip.voice_frame_linear(self.absolute_frame, &self.transport)? as u64;
                let total = clip.natural_frames().max(1);
                return Some((frame, total));
            }
        }
        if let Some(clip) = self
            .midi_clips
            .iter()
            .find(|c| c.track_index == track_index && c.scene_index == scene_index)
        {
            if clip.is_playing {
                let elapsed = self.absolute_frame.saturating_sub(clip.start_absolute);
                let emission = clip.emission_len_frames(&self.transport).max(1);
                if clip.clip_loop_enabled {
                    let loop_len = clip.quantized_loop_len(&self.transport).max(1);
                    return Some((elapsed % loop_len, loop_len));
                } else if elapsed < emission {
                    return Some((elapsed, emission));
                }
            }
        }
        None
    }

    fn refresh_pending_stop_mask(&self) {
        let mut mask = 0u32;
        for p in &self.pending_track_stops {
            if p.track_index < 32 {
                mask |= 1u32 << p.track_index;
            }
        }
        self.pending_stop_tracks_mask.store(mask, Ordering::Relaxed);
    }

    fn cancel_pending_track_stop(&mut self, track_index: usize) {
        let before = self.pending_track_stops.len();
        self.pending_track_stops.retain(|p| p.track_index != track_index);
        if self.pending_track_stops.len() != before {
            self.refresh_pending_stop_mask();
        }
    }

    pub fn stop_track(&mut self, track_index: usize) {
        let track_has_playing = self
            .clips
            .iter()
            .any(|c| c.track_index == track_index && c.is_playing)
            || self
                .midi_clips
                .iter()
                .any(|c| c.track_index == track_index && c.is_playing);
        let grid = self.global_launch_quantization;
        if track_has_playing
            && grid != QuantizationGrid::None
            && self.transport.playback_state == TransportPlaybackState::Playing
        {
            let wait = QuantizationEngine::samples_until_next_sync(
                &self.transport,
                grid,
                self.sample_rate as f64,
            );
            if wait > 0 {
                let target_sample = self.transport.sample_count.saturating_add(wait);
                if let Some(existing) = self
                    .pending_track_stops
                    .iter_mut()
                    .find(|p| p.track_index == track_index)
                {
                    existing.target_sample = target_sample;
                    existing.grid = grid;
                } else {
                    self.pending_track_stops.push(PendingTrackStop {
                        track_index,
                        target_sample,
                        grid,
                    });
                }
                self.refresh_pending_stop_mask();
                return;
            }
        }
        self.cancel_pending_track_stop(track_index);
        self.execute_stop_track(track_index);
    }

    fn execute_stop_track(&mut self, track_index: usize) {
        for clip in self.clips.iter_mut().filter(|c| c.track_index == track_index) {
            clip.is_playing = false;
        }
        for clip in self.midi_clips.iter_mut().filter(|c| c.track_index == track_index) {
            clip.is_playing = false;
            for pitch in 0..128 {
                if clip.active_notes[pitch] {
                    self.dms_voice_pool.kill_pad(dms_pad_key(clip.track_index, pitch));
                    clip.active_notes[pitch] = false;
                }
            }
        }
        self.wavetable_voice_pool.kill_track_notes(track_index);
        self.sampler_voice_pool.kill_track_notes(track_index);
        self.open_poly_voice_pool.kill_track_notes(track_index);
        self.open_grid_voice_pool.kill_track_notes(track_index);
    }

    // ─── OpenDMS Polyphonic Methods ──────────────────────────────

    /// Carga un sample para un pad en la pista indicada.
    /// Clave interna: `dms_pad_key(track_idx, pad_idx)`. De esta forma
    /// dos canales del mixer con el mismo `pad_idx` (ej. Pad 0 = Kick)
    /// no se pisan entre sí y conservan su propio routing, fader y medidor.
    pub fn load_dms_sample_for_track(&mut self, track_idx: usize, pad_idx: usize, samples: Vec<f32>, channels: usize) {
        let key = dms_pad_key(track_idx, pad_idx);
        while self.dms_samples.len() <= key {
            self.dms_samples.push(None);
            self.dms_channels.push(1);
        }
        self.dms_channels[key] = channels;
        self.dms_samples[key] = Some(samples);
    }

    /// Legacy: por compatibilidad carga en track 0.
    pub fn load_dms_sample(&mut self, pad_idx: usize, samples: Vec<f32>, channels: usize) {
        self.load_dms_sample_for_track(0, pad_idx, samples, channels);
    }

    pub fn trigger_dms_note(
        &mut self,
        pad_idx: usize,
        gain: f32,
        pan: f32,
        velocity: f32,
        play_speed: f64,
        adsr: &DmsAdsrParams,
        track_idx: usize,
    ) {
        let ti = track_idx.min(15);
        let key = dms_pad_key(ti, pad_idx);
        // Si el track específico tiene sample, usarlo; si no, fallback al track 0
        // (por si el pad se cargó de manera legacy sin track_idx).
        let (samples_ref, channels) = if let Some(Some(ref s)) = self.dms_samples.get(key) {
            (s.as_slice(), self.dms_channels.get(key).copied().unwrap_or(1))
        } else if let Some(Some(ref s)) = self.dms_samples.get(pad_idx) {
            (s.as_slice(), self.dms_channels.get(pad_idx).copied().unwrap_or(1))
        } else {
            return;
        };

        self.dms_voice_pool.trigger(
            samples_ref,
            channels,
            key, // Usar key única para release/kill sin pisar otros tracks
            gain,
            pan,
            velocity,
            play_speed,
            adsr,
            ti,
        );
    }

    pub fn release_dms_note_for_track(&mut self, track_idx: usize, pad_idx: usize) {
        let key = dms_pad_key(track_idx, pad_idx);
        self.dms_voice_pool.release_pad(key);
    }

    pub fn release_dms_note(&mut self, pad_idx: usize) {
        self.dms_voice_pool.release_pad(pad_idx);
        // Liberar también para cualquier pista si se usó pad puro
        for t in 0..16 {
            self.dms_voice_pool.release_pad(dms_pad_key(t, pad_idx));
        }
    }

    pub fn kill_dms_note_for_track(&mut self, track_idx: usize, pad_idx: usize) {
        let key = dms_pad_key(track_idx, pad_idx);
        self.dms_voice_pool.kill_pad(key);
    }

    pub fn kill_dms_note(&mut self, pad_idx: usize) {
        self.dms_voice_pool.kill_pad(pad_idx);
        for t in 0..16 {
            self.dms_voice_pool.kill_pad(dms_pad_key(t, pad_idx));
        }
    }

    pub fn set_dms_track(&mut self, track_idx: usize) {
        self.dms_track_idx = track_idx.min(15);
    }

    pub fn trigger_virtual_note(&mut self, track_idx: usize, pitch: u8, velocity: f32) {
        let ti = track_idx.min(15);
        if self.track_open_grid_enabled[ti] {
            self.open_grid_voice_pool.trigger(pitch, velocity, ti);
        }
        if self.track_open_poly_enabled[ti] {
            let params = self.track_open_poly_params[ti];
            self.open_poly_voice_pool.trigger(pitch, velocity, ti, &params);
        } else if let Some(table) = &self.track_wavetables[ti] {
            let adsr = WavetableAdsrParams::default();
            let table_len = table.len();
            let offset = self.track_wavetable_pitch_offsets[ti];
            let eff_pitch = (pitch as i32 + offset as i32).clamp(0, 127) as u8;
            self.wavetable_voice_pool.trigger(eff_pitch, velocity, ti, &adsr, table_len, self.sample_rate);
        }

        if let Some(sample) = &self.track_sampler_samples[ti] {
            let params = &self.track_sampler_params[ti];
            if params.note_trigger {
                self.sampler_voice_pool.trigger(pitch, velocity, ti, sample, params);
            }
        }

        let key = dms_pad_key(ti, pitch as usize);
        let (samples_ref, channels) = if let Some(Some(ref s)) = self.dms_samples.get(key) {
            (s.as_slice(), self.dms_channels.get(key).copied().unwrap_or(1))
        } else if let Some(Some(ref s)) = self.dms_samples.get(pitch as usize) {
            (s.as_slice(), self.dms_channels.get(pitch as usize).copied().unwrap_or(1))
        } else {
            (&[] as &[f32], 1)
        };
        if !samples_ref.is_empty() {
            let dms_adsr = DmsAdsrParams {
                attack_rate: self.sample_rate,
                decay_rate: self.sample_rate,
                sustain_level: 1.0,
                release_rate: self.sample_rate / 0.05,
            };
            self.dms_voice_pool.trigger(
                samples_ref,
                channels,
                key,
                1.0,
                0.0,
                velocity,
                1.0,
                &dms_adsr,
                ti,
            );
        }

        let vel_byte = (velocity * 127.0).clamp(1.0, 127.0) as u8;
        self.track_midi_events[ti].push(hikaru_midi::MidiEvent {
            status: 0x90 | (ti as u8 & 0x0F),
            data1: pitch,
            data2: vel_byte,
        });
    }

    pub fn release_virtual_note(&mut self, track_idx: usize, pitch: u8) {
        let ti = track_idx.min(15);
        self.open_grid_voice_pool.release_note(pitch, ti);
        self.open_poly_voice_pool.release_note(pitch, ti);
        let offset = self.track_wavetable_pitch_offsets[ti];
        let eff_pitch = (pitch as i32 + offset as i32).clamp(0, 127) as u8;
        self.wavetable_voice_pool.release_note(eff_pitch, ti);
        if let Some(sample) = &self.track_sampler_samples[ti] {
            let params = &self.track_sampler_params[ti];
            if params.release_trigger {
                self.sampler_voice_pool.trigger(pitch, 1.0, ti, sample, params);
            } else {
                self.sampler_voice_pool.release_note(pitch, ti);
            }
        } else {
            self.sampler_voice_pool.release_note(pitch, ti);
        }
        let key = dms_pad_key(ti, pitch as usize);
        self.dms_voice_pool.release_pad(key);
        self.dms_voice_pool.release_pad(pitch as usize);
        self.track_midi_events[ti].push(hikaru_midi::MidiEvent {
            status: 0x80 | (ti as u8 & 0x0F),
            data1: pitch,
            data2: 0,
        });
    }

    pub fn kill_virtual_note(&mut self, track_idx: usize, pitch: u8) {
        let ti = track_idx.min(15);
        self.open_grid_voice_pool.kill_note(pitch, ti);
        self.open_poly_voice_pool.kill_note(pitch, ti);
        let offset = self.track_wavetable_pitch_offsets[ti];
        let eff_pitch = (pitch as i32 + offset as i32).clamp(0, 127) as u8;
        self.wavetable_voice_pool.kill_note(eff_pitch, ti);
        self.sampler_voice_pool.kill_note(pitch, ti);
        let key = dms_pad_key(ti, pitch as usize);
        self.dms_voice_pool.kill_pad(key);
        self.dms_voice_pool.kill_pad(pitch as usize);
        self.track_midi_events[ti].push(hikaru_midi::MidiEvent {
            status: 0x80 | (ti as u8 & 0x0F),
            data1: pitch,
            data2: 0,
        });
    }

    pub fn set_track_wavetable(&mut self, track_idx: usize, samples: Vec<f32>, pitch_offset: i8) {
        self.set_track_wavetable_with_volume(track_idx, samples, pitch_offset, 1.0);
    }

    pub fn set_track_wavetable_with_volume(
        &mut self,
        track_idx: usize,
        samples: Vec<f32>,
        pitch_offset: i8,
        volume: f32,
    ) {
        let ti = track_idx.min(15);
        if samples.len() >= 256 {
            self.track_wavetables[ti] = Some(samples);
            self.track_wavetable_pitch_offsets[ti] = pitch_offset;
            self.track_wavetable_volumes[ti] = volume.max(0.0);
        } else {
            self.track_wavetables[ti] = None;
            self.track_wavetable_pitch_offsets[ti] = 0;
            self.track_wavetable_volumes[ti] = 1.0;
            self.wavetable_voice_pool.kill_track_notes(ti);
        }
    }

    pub fn set_track_wavetable_volume(&mut self, track_idx: usize, volume: f32) {
        let ti = track_idx.min(15);
        self.track_wavetable_volumes[ti] = volume.max(0.0);
    }

    pub fn clear_track_wavetable(&mut self, track_idx: usize) {
        let ti = track_idx.min(15);
        self.track_wavetables[ti] = None;
        self.track_wavetable_pitch_offsets[ti] = 0;
        self.track_wavetable_volumes[ti] = 1.0;
        self.wavetable_voice_pool.kill_track_notes(ti);
    }

    pub fn set_track_sampler_sample(
        &mut self,
        track_idx: usize,
        samples: Vec<f32>,
        channels: usize,
        sample_rate: f32,
    ) {
        let ti = track_idx.min(15);
        if samples.is_empty() {
            self.track_sampler_samples[ti] = None;
            self.sampler_voice_pool.kill_track_notes(ti);
        } else {
            self.track_sampler_samples[ti] =
                Some(Arc::new(SamplerSampleData::new(samples, channels, sample_rate)));
        }
    }

    pub fn set_track_sampler_params(&mut self, track_idx: usize, params: SamplerDspParams) {
        let ti = track_idx.min(15);
        self.track_sampler_params[ti] = params;
    }

    pub fn sampler_playhead(&self, track_idx: usize) -> Option<f32> {
        let ti = track_idx.min(15);
        let sample = self.track_sampler_samples[ti].as_ref()?;
        self.sampler_voice_pool.active_playhead(ti, sample.total_frames)
    }

    pub fn set_track_open_poly_params(
        &mut self,
        track_idx: usize,
        enabled: bool,
        params: OpenPolyDspParams,
    ) {
        let ti = track_idx.min(15);
        self.track_open_poly_enabled[ti] = enabled;
        self.track_open_poly_params[ti] = params;
        if !enabled {
            self.open_poly_voice_pool.kill_track_notes(ti);
        }
    }

    pub fn set_track_open_poly_wavetable(
        &mut self,
        track_idx: usize,
        samples: Vec<f32>,
        frames: usize,
        frame_len: usize,
    ) {
        let ti = track_idx.min(15);
        if samples.len() >= 64 && frames > 0 && frame_len >= 64 {
            self.track_open_poly_wavetables[ti] =
                Some(Arc::new(OpenPolyWavetableData::new(samples, frames, frame_len)));
        } else {
            self.track_open_poly_wavetables[ti] = None;
        }
    }

    pub fn open_poly_meter(&self, track_idx: usize) -> (f32, f32) {
        self.open_poly_voice_pool.track_meter(track_idx)
    }

    pub fn set_track_open_grid_patch(
        &mut self,
        track_idx: usize,
        enabled: bool,
        patch: Option<OpenGridPatch>,
    ) {
        let ti = track_idx.min(15);
        self.track_open_grid_enabled[ti] = enabled && patch.is_some();
        self.open_grid_voice_pool
            .set_track_patch(ti, if enabled { patch.map(Arc::new) } else { None });
    }

    pub fn open_grid_meter(&self, track_idx: usize) -> (f32, f32) {
        self.open_grid_voice_pool.track_meter(track_idx)
    }

    pub fn open_grid_scope(&self, track_idx: usize) -> Vec<f32> {
        self.open_grid_voice_pool.track_scope(track_idx)
    }

    pub fn process(&mut self, out_buffer: &mut AudioBuffer<'_>) {
        let samples = out_buffer.get_samples_mut();
        let num_channels = 2;

        samples.fill(0.0);

        if self.preview_player.is_active() {
            let preview = self.preview_player.shared_buffer();
            preview.process(samples);
        }

        // Ejecutar lanzamiento cuantizado de escena si su límite cae dentro de este bloque.
        if self.transport.playback_state == TransportPlaybackState::Playing {
            let buffer_frames = (samples.len() / num_channels) as u64;
            if buffer_frames > 0 {
                if let Some(pending) = self.pending_scene_launch {
                    let frame_start = self.transport.sample_count;
                    let frame_end = frame_start.saturating_add(buffer_frames);
                    let loop_wraps_before_target = self.transport.has_valid_loop()
                        && frame_start < self.transport.loop_end_samples
                        && frame_end >= self.transport.loop_end_samples
                        && pending.target_sample >= self.transport.loop_end_samples;
                    if pending.target_sample < frame_end || loop_wraps_before_target {
                        let fire_sample =
                            if loop_wraps_before_target && pending.target_sample >= frame_end {
                                self.transport.loop_end_samples
                            } else {
                                pending.target_sample
                            };
                        let offset = fire_sample.saturating_sub(frame_start);
                        self.pending_scene_launch = None;
                        self.pending_scene_index_atomic.store(-1, Ordering::Relaxed);
                        let fire_abs = self.absolute_frame.saturating_add(offset);
                        self.execute_scene_launch(pending.scene_index, fire_sample, fire_abs);
                    }
                }
                if !self.pending_track_stops.is_empty() {
                    let frame_start = self.transport.sample_count;
                    let frame_end = frame_start.saturating_add(buffer_frames);
                    let mut due_tracks = Vec::new();
                    let mut remaining = Vec::with_capacity(self.pending_track_stops.len());
                    for pending in self.pending_track_stops.drain(..) {
                        let loop_wraps_before_target = self.transport.has_valid_loop()
                            && frame_start < self.transport.loop_end_samples
                            && frame_end >= self.transport.loop_end_samples
                            && pending.target_sample >= self.transport.loop_end_samples;
                        if pending.target_sample < frame_end || loop_wraps_before_target {
                            due_tracks.push(pending.track_index);
                        } else {
                            remaining.push(pending);
                        }
                    }
                    self.pending_track_stops = remaining;
                    if !due_tracks.is_empty() {
                        for ti in due_tracks {
                            self.execute_stop_track(ti);
                        }
                        self.refresh_pending_stop_mask();
                    }
                }
            }
        }

        let sample_rate = self.sample_rate;
        let dms_samples = &self.dms_samples;
        let dms_channels = &self.dms_channels;
        let dms_voice_pool = &mut self.dms_voice_pool;
        let wavetable_voice_pool = &mut self.wavetable_voice_pool;
        let track_wavetables = &self.track_wavetables;
        let track_wavetable_pitch_offsets = &self.track_wavetable_pitch_offsets;
        let sampler_voice_pool = &mut self.sampler_voice_pool;
        let track_sampler_samples = &self.track_sampler_samples;
        let track_sampler_params = &self.track_sampler_params;
        let open_poly_voice_pool = &mut self.open_poly_voice_pool;
        let track_open_poly_enabled = &self.track_open_poly_enabled;
        let track_open_poly_params = &self.track_open_poly_params;
        let open_grid_voice_pool = &mut self.open_grid_voice_pool;
        let track_open_grid_enabled = &self.track_open_grid_enabled;
        let track_midi_events = &mut self.track_midi_events;
        let midi_clips = &mut self.midi_clips;

        for evs in track_midi_events.iter_mut() {
            evs.clear();
        }

        if self.transport.playback_state == TransportPlaybackState::Playing {
            let buffer_frames = (samples.len() / num_channels) as u64;
            if buffer_frames > 0 {
                let frame_start = self.transport.sample_count;
                let abs_start = self.absolute_frame;
                let transport = self.transport;
                let mode = self.mode;

                // ─── Real-Time MIDI Clip Scheduling ──────────────────
                for clip in midi_clips.iter_mut() {
                    let ti = clip.track_index.min(15);
                    if mode == EngineMode::OpenStudio {
                        let clip_frame_start = clip.start_frame;
                        let duration_frames = transport.ticks_to_samples(clip.duration_ticks);
                        let clip_frame_end = clip_frame_start.saturating_add(duration_frames);
                        let quantum_start = frame_start;
                        let quantum_end = frame_start + buffer_frames;

                        if quantum_end <= clip_frame_start || quantum_start >= clip_frame_end {
                            if quantum_start >= clip_frame_end {
                                for pitch in 0..128 {
                                    if clip.active_notes[pitch] {
                                        dms_voice_pool.kill_pad(dms_pad_key(ti, pitch));
                                        let offset = track_wavetable_pitch_offsets[ti];
                                        let eff_pitch = (pitch as i32 + offset as i32).clamp(0, 127) as u8;
                                        wavetable_voice_pool.kill_note(eff_pitch, ti);
                                        sampler_voice_pool.kill_note(pitch as u8, ti);
                                        open_poly_voice_pool.kill_note(pitch as u8, ti);
                                        open_grid_voice_pool.kill_note(pitch as u8, ti);
                                        clip.active_notes[pitch] = false;
                                        track_midi_events[ti].push(hikaru_midi::MidiEvent {
                                            status: 0x80 | (ti as u8 & 0x0F),
                                            data1: pitch as u8,
                                            data2: 0,
                                        });
                                    }
                                }
                            }
                            continue;
                        }

                        for note in &clip.notes {
                            let note_start = clip_frame_start + transport.ticks_to_samples(note.start_tick);
                            let note_end = clip_frame_start + transport.ticks_to_samples(note.end_tick());

                            if note_start >= quantum_start && note_start < quantum_end && note_start < clip_frame_end {
                                let pitch = note.pitch as usize;
                                let vel = note.velocity;
                                if vel > 0 {
                                    let adsr = DmsAdsrParams {
                                        attack_rate: sample_rate,
                                        decay_rate: sample_rate,
                                        sustain_level: 1.0,
                                        release_rate: sample_rate / 0.05,
                                    };
                                    let key = dms_pad_key(ti, pitch);
                                    let (samples_ref, ch) = if let Some(Some(ref s)) = dms_samples.get(key) {
                                        (s.as_slice(), dms_channels.get(key).copied().unwrap_or(1))
                                    } else if let Some(Some(ref s)) = dms_samples.get(pitch) {
                                        (s.as_slice(), dms_channels.get(pitch).copied().unwrap_or(1))
                                    } else {
                                        (&[] as &[f32], 1)
                                    };
                                    if !samples_ref.is_empty() {
                                        dms_voice_pool.trigger(samples_ref, ch, key, 1.0, 0.0, (vel as f32) / 127.0, 1.0, &adsr, ti);
                                    } else if track_open_grid_enabled[ti] {
                                        open_grid_voice_pool.trigger(note.pitch, (vel as f32) / 127.0, ti);
                                    } else if track_open_poly_enabled[ti] {
                                        open_poly_voice_pool.trigger(note.pitch, (vel as f32) / 127.0, ti, &track_open_poly_params[ti]);
                                    } else if let Some(table) = &track_wavetables[ti] {
                                        let wt_adsr = WavetableAdsrParams::default();
                                        let offset = track_wavetable_pitch_offsets[ti];
                                        let eff_pitch = (note.pitch as i32 + offset as i32).clamp(0, 127) as u8;
                                        wavetable_voice_pool.trigger(eff_pitch, (vel as f32) / 127.0, ti, &wt_adsr, table.len(), sample_rate);
                                    }
                                    if let Some(sample) = &track_sampler_samples[ti] {
                                        let sp = &track_sampler_params[ti];
                                        if sp.note_trigger {
                                            sampler_voice_pool.trigger(note.pitch, (vel as f32) / 127.0, ti, sample, sp);
                                        }
                                    }
                                    clip.active_notes[pitch.min(127)] = true;
                                    track_midi_events[ti].push(hikaru_midi::MidiEvent {
                                        status: 0x90 | (ti as u8 & 0x0F),
                                        data1: note.pitch,
                                        data2: vel,
                                    });
                                }
                            }

                            if note_end >= quantum_start && note_end < quantum_end {
                                let pitch = note.pitch as usize;
                                dms_voice_pool.release_pad(dms_pad_key(ti, pitch));
                                let offset = track_wavetable_pitch_offsets[ti];
                                let eff_pitch = (note.pitch as i32 + offset as i32).clamp(0, 127) as u8;
                                wavetable_voice_pool.release_note(eff_pitch, ti);
                                open_poly_voice_pool.release_note(pitch as u8, ti);
                                open_grid_voice_pool.release_note(pitch as u8, ti);
                                            if let Some(sample) = &track_sampler_samples[ti] {
                                                let sp = &track_sampler_params[ti];
                                                if sp.release_trigger {
                                                    sampler_voice_pool.trigger(pitch as u8, 1.0, ti, sample, sp);
                                                } else {
                                                    sampler_voice_pool.release_note(pitch as u8, ti);
                                                }
                                            } else {
                                                sampler_voice_pool.release_note(pitch as u8, ti);
                                            }
                                clip.active_notes[pitch.min(127)] = false;
                                track_midi_events[ti].push(hikaru_midi::MidiEvent {
                                    status: 0x80 | (ti as u8 & 0x0F),
                                    data1: note.pitch,
                                    data2: 0,
                                });
                            }
                        }

                        if clip_frame_end >= quantum_start && clip_frame_end < quantum_end {
                            for pitch in 0..128 {
                                if clip.active_notes[pitch] {
                                    dms_voice_pool.kill_pad(dms_pad_key(ti, pitch));
                                    let offset = track_wavetable_pitch_offsets[ti];
                                    let eff_pitch = (pitch as i32 + offset as i32).clamp(0, 127) as u8;
                                    wavetable_voice_pool.kill_note(eff_pitch, ti);
                                    open_poly_voice_pool.kill_note(pitch as u8, ti);
                                        open_grid_voice_pool.kill_note(pitch as u8, ti);
                                    clip.active_notes[pitch] = false;
                                    track_midi_events[ti].push(hikaru_midi::MidiEvent {
                                        status: 0x80 | (ti as u8 & 0x0F),
                                        data1: pitch as u8,
                                        data2: 0,
                                    });
                                }
                            }
                        }
                    } else {
                        // OpenLive mode
                        if !clip.is_playing {
                            continue;
                        }
                        let (elapsed_start, sched_frames) = if abs_start >= clip.start_absolute {
                            (abs_start - clip.start_absolute, buffer_frames)
                        } else if clip.start_absolute < abs_start + buffer_frames {
                            let offset = clip.start_absolute - abs_start;
                            (0, buffer_frames - offset)
                        } else {
                            continue;
                        };
                        let has_loop = clip.has_valid_clip_loop();
                        let duration_frames = transport.ticks_to_samples(clip.duration_ticks);

                        if !has_loop {
                            if elapsed_start >= duration_frames {
                                clip.is_playing = false;
                                for pitch in 0..128 {
                                    if clip.active_notes[pitch] {
                                        dms_voice_pool.kill_pad(dms_pad_key(ti, pitch));
                                        let offset = track_wavetable_pitch_offsets[ti];
                                        let eff_pitch = (pitch as i32 + offset as i32).clamp(0, 127) as u8;
                                        wavetable_voice_pool.kill_note(eff_pitch, ti);
                                        sampler_voice_pool.kill_note(pitch as u8, ti);
                                        open_poly_voice_pool.kill_note(pitch as u8, ti);
                                        open_grid_voice_pool.kill_note(pitch as u8, ti);
                                        clip.active_notes[pitch] = false;
                                        track_midi_events[ti].push(hikaru_midi::MidiEvent {
                                            status: 0x80 | (ti as u8 & 0x0F),
                                            data1: pitch as u8,
                                            data2: 0,
                                        });
                                    }
                                }
                                continue;
                            }

                            let elapsed_end = elapsed_start + sched_frames;
                            for note in &clip.notes {
                                let n_start = transport.ticks_to_samples(note.start_tick);
                                let n_end = transport.ticks_to_samples(note.end_tick());

                                if n_start >= elapsed_start && n_start < elapsed_end && n_start < duration_frames {
                                    let pitch = note.pitch as usize;
                                    let vel = note.velocity;
                                    if vel > 0 {
                                        let adsr = DmsAdsrParams {
                                            attack_rate: sample_rate,
                                            decay_rate: sample_rate,
                                            sustain_level: 1.0,
                                            release_rate: sample_rate / 0.05,
                                        };
                                        let key = dms_pad_key(ti, pitch);
                                        let (samples_ref, ch) = if let Some(Some(ref s)) = dms_samples.get(key) {
                                            (s.as_slice(), dms_channels.get(key).copied().unwrap_or(1))
                                        } else if let Some(Some(ref s)) = dms_samples.get(pitch) {
                                            (s.as_slice(), dms_channels.get(pitch).copied().unwrap_or(1))
                                        } else {
                                            (&[] as &[f32], 1)
                                        };
                                        if !samples_ref.is_empty() {
                                            dms_voice_pool.trigger(samples_ref, ch, key, 1.0, 0.0, (vel as f32) / 127.0, 1.0, &adsr, ti);
                                        } else if track_open_grid_enabled[ti] {
                                        open_grid_voice_pool.trigger(note.pitch, (vel as f32) / 127.0, ti);
                                    } else if track_open_poly_enabled[ti] {
                                            open_poly_voice_pool.trigger(note.pitch, (vel as f32) / 127.0, ti, &track_open_poly_params[ti]);
                                        } else if let Some(table) = &track_wavetables[ti] {
                                            let wt_adsr = WavetableAdsrParams::default();
                                            let offset = track_wavetable_pitch_offsets[ti];
                                            let eff_pitch = (note.pitch as i32 + offset as i32).clamp(0, 127) as u8;
                                            wavetable_voice_pool.trigger(eff_pitch, (vel as f32) / 127.0, ti, &wt_adsr, table.len(), sample_rate);
                                        }
                                        if let Some(sample) = &track_sampler_samples[ti] {
                                            let sp = &track_sampler_params[ti];
                                            if sp.note_trigger {
                                                sampler_voice_pool.trigger(note.pitch, (vel as f32) / 127.0, ti, sample, sp);
                                            }
                                        }
                                        clip.active_notes[pitch.min(127)] = true;
                                        track_midi_events[ti].push(hikaru_midi::MidiEvent {
                                            status: 0x90 | (ti as u8 & 0x0F),
                                            data1: note.pitch,
                                            data2: vel,
                                        });
                                    }
                                }

                                if n_end >= elapsed_start && n_end < elapsed_end {
                                    let pitch = note.pitch as usize;
                                    dms_voice_pool.release_pad(dms_pad_key(ti, pitch));
                                    let offset = track_wavetable_pitch_offsets[ti];
                                    let eff_pitch = (note.pitch as i32 + offset as i32).clamp(0, 127) as u8;
                                    wavetable_voice_pool.release_note(eff_pitch, ti);
                                open_poly_voice_pool.release_note(pitch as u8, ti);
                                open_grid_voice_pool.release_note(pitch as u8, ti);
                                            if let Some(sample) = &track_sampler_samples[ti] {
                                                let sp = &track_sampler_params[ti];
                                                if sp.release_trigger {
                                                    sampler_voice_pool.trigger(pitch as u8, 1.0, ti, sample, sp);
                                                } else {
                                                    sampler_voice_pool.release_note(pitch as u8, ti);
                                                }
                                            } else {
                                                sampler_voice_pool.release_note(pitch as u8, ti);
                                            }
                                    clip.active_notes[pitch.min(127)] = false;
                                    track_midi_events[ti].push(hikaru_midi::MidiEvent {
                                        status: 0x80 | (ti as u8 & 0x0F),
                                        data1: note.pitch,
                                        data2: 0,
                                    });
                                }
                            }

                            if elapsed_end >= duration_frames {
                                clip.is_playing = false;
                                for pitch in 0..128 {
                                    if clip.active_notes[pitch] {
                                        dms_voice_pool.kill_pad(dms_pad_key(ti, pitch));
                                        let offset = track_wavetable_pitch_offsets[ti];
                                        let eff_pitch = (pitch as i32 + offset as i32).clamp(0, 127) as u8;
                                        wavetable_voice_pool.kill_note(eff_pitch, ti);
                                        sampler_voice_pool.kill_note(pitch as u8, ti);
                                        open_poly_voice_pool.kill_note(pitch as u8, ti);
                                        open_grid_voice_pool.kill_note(pitch as u8, ti);
                                        clip.active_notes[pitch] = false;
                                        track_midi_events[ti].push(hikaru_midi::MidiEvent {
                                            status: 0x80 | (ti as u8 & 0x0F),
                                            data1: pitch as u8,
                                            data2: 0,
                                        });
                                    }
                                }
                            }
                        } else {
                            // Looping MIDI clip in OpenLive
                            let ls_ticks = clip.clip_loop_start_ticks;
                            let le_ticks = clip.clip_loop_end_ticks;
                            let loop_len_ticks = le_ticks.saturating_sub(ls_ticks).max(240);
                            let loop_start_frames = transport.ticks_to_samples(ls_ticks);
                            let loop_len_frames = transport.ticks_to_samples(loop_len_ticks).max(1);
                            let loop_end_frames = loop_start_frames + loop_len_frames;

                            let mut cur_elapsed = elapsed_start;
                            let mut frames_left = sched_frames;

                            while frames_left > 0 {
                                let (cycle_sample, dist_to_wrap) = if cur_elapsed < loop_end_frames {
                                    let pos = cur_elapsed;
                                    let dist = loop_end_frames - cur_elapsed;
                                    (pos, dist)
                                } else {
                                    let offset = cur_elapsed - loop_end_frames;
                                    let cycle_pos = offset % loop_len_frames;
                                    let pos = loop_start_frames + cycle_pos;
                                    let dist = loop_len_frames - cycle_pos;
                                    (pos, dist)
                                };

                                let seg_frames = frames_left.min(dist_to_wrap);
                                let seg_start_sample = cycle_sample;
                                let seg_end_sample = cycle_sample + seg_frames;

                                for note in &clip.notes {
                                    let n_start = transport.ticks_to_samples(note.start_tick);
                                    let n_end = transport.ticks_to_samples(note.end_tick());

                                    if n_start >= seg_start_sample && n_start < seg_end_sample {
                                        let pitch = note.pitch as usize;
                                        let vel = note.velocity;
                                        if vel > 0 {
                                            let adsr = DmsAdsrParams {
                                                attack_rate: sample_rate,
                                                decay_rate: sample_rate,
                                                sustain_level: 1.0,
                                                release_rate: sample_rate / 0.05,
                                            };
                                            let key = dms_pad_key(ti, pitch);
                                            let (samples_ref, ch) = if let Some(Some(ref s)) = dms_samples.get(key) {
                                                (s.as_slice(), dms_channels.get(key).copied().unwrap_or(1))
                                            } else if let Some(Some(ref s)) = dms_samples.get(pitch) {
                                                (s.as_slice(), dms_channels.get(pitch).copied().unwrap_or(1))
                                            } else {
                                                (&[] as &[f32], 1)
                                            };
                                            if !samples_ref.is_empty() {
                                                dms_voice_pool.trigger(samples_ref, ch, key, 1.0, 0.0, (vel as f32) / 127.0, 1.0, &adsr, ti);
                                            } else if track_open_grid_enabled[ti] {
                                        open_grid_voice_pool.trigger(note.pitch, (vel as f32) / 127.0, ti);
                                    } else if track_open_poly_enabled[ti] {
                                                open_poly_voice_pool.trigger(note.pitch, (vel as f32) / 127.0, ti, &track_open_poly_params[ti]);
                                            } else if let Some(table) = &track_wavetables[ti] {
                                                let wt_adsr = WavetableAdsrParams::default();
                                                let offset = track_wavetable_pitch_offsets[ti];
                                                let eff_pitch = (note.pitch as i32 + offset as i32).clamp(0, 127) as u8;
                                                wavetable_voice_pool.trigger(eff_pitch, (vel as f32) / 127.0, ti, &wt_adsr, table.len(), sample_rate);
                                            }
                                            if let Some(sample) = &track_sampler_samples[ti] {
                                                let sp = &track_sampler_params[ti];
                                                if sp.note_trigger {
                                                    sampler_voice_pool.trigger(note.pitch, (vel as f32) / 127.0, ti, sample, sp);
                                                }
                                            }
                                            clip.active_notes[pitch.min(127)] = true;
                                            track_midi_events[ti].push(hikaru_midi::MidiEvent {
                                                status: 0x90 | (ti as u8 & 0x0F),
                                                data1: note.pitch,
                                                data2: vel,
                                            });
                                        }
                                    }

                                    if n_end >= seg_start_sample && n_end < seg_end_sample {
                                        let pitch = note.pitch as usize;
                                        dms_voice_pool.release_pad(dms_pad_key(ti, pitch));
                                        let offset = track_wavetable_pitch_offsets[ti];
                                        let eff_pitch = (note.pitch as i32 + offset as i32).clamp(0, 127) as u8;
                                        wavetable_voice_pool.release_note(eff_pitch, ti);
                                open_poly_voice_pool.release_note(pitch as u8, ti);
                                open_grid_voice_pool.release_note(pitch as u8, ti);
                                            if let Some(sample) = &track_sampler_samples[ti] {
                                                let sp = &track_sampler_params[ti];
                                                if sp.release_trigger {
                                                    sampler_voice_pool.trigger(pitch as u8, 1.0, ti, sample, sp);
                                                } else {
                                                    sampler_voice_pool.release_note(pitch as u8, ti);
                                                }
                                            } else {
                                                sampler_voice_pool.release_note(pitch as u8, ti);
                                            }
                                        clip.active_notes[pitch.min(127)] = false;
                                        track_midi_events[ti].push(hikaru_midi::MidiEvent {
                                            status: 0x80 | (ti as u8 & 0x0F),
                                            data1: note.pitch,
                                            data2: 0,
                                        });
                                    }
                                }

                                cur_elapsed += seg_frames;
                                frames_left -= seg_frames;

                                if seg_frames == dist_to_wrap {
                                    for pitch in 0..128 {
                                        if clip.active_notes[pitch] {
                                            dms_voice_pool.release_pad(dms_pad_key(ti, pitch));
                                            let offset = track_wavetable_pitch_offsets[ti];
                                            let eff_pitch = (pitch as i32 + offset as i32).clamp(0, 127) as u8;
                                            wavetable_voice_pool.release_note(eff_pitch, ti);
                                open_poly_voice_pool.release_note(pitch as u8, ti);
                                open_grid_voice_pool.release_note(pitch as u8, ti);
                                            if let Some(sample) = &track_sampler_samples[ti] {
                                                let sp = &track_sampler_params[ti];
                                                if sp.release_trigger {
                                                    sampler_voice_pool.trigger(pitch as u8, 1.0, ti, sample, sp);
                                                } else {
                                                    sampler_voice_pool.release_note(pitch as u8, ti);
                                                }
                                            } else {
                                                sampler_voice_pool.release_note(pitch as u8, ti);
                                            }
                                            clip.active_notes[pitch] = false;
                                            track_midi_events[ti].push(hikaru_midi::MidiEvent {
                                                status: 0x80 | (ti as u8 & 0x0F),
                                                data1: pitch as u8,
                                                data2: 0,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // ─── OpenDMS Polyphonic Voice Mixing ──────────────────────
        // Always active (not gated by transport), like preview player.
        // Cada voz lleva su propio `track_idx`: el fader/pan/mute/solo del
        // mixer se aplica por voz, no con un único `dms_track_idx` global.
        // (Ese era el bug "solo baja el volumen del Track 1".)
        {
            let buf_frames = samples.len() / 2;
            let any_solo_dms = self.track_solos.iter().any(|s| s.load(Ordering::Relaxed));
            let mut dms_gain_l = [0.0f32; 16];
            let mut dms_gain_r = [0.0f32; 16];
            for t in 0..16 {
                let vol = f32::from_bits(self.track_volumes[t].load(Ordering::Relaxed));
                let muted = self.track_mutes[t].load(Ordering::Relaxed);
                let soloed = self.track_solos[t].load(Ordering::Relaxed);
                let effective_gain = if muted {
                    0.0
                } else if any_solo_dms && !soloed {
                    0.0
                } else {
                    vol
                };
                let pan_norm = f32::from_bits(self.track_pans[t].load(Ordering::Relaxed)) / 100.0;
                dms_gain_l[t] = effective_gain * if pan_norm > 0.0 { 1.0 - pan_norm } else { 1.0 };
                dms_gain_r[t] = effective_gain * if pan_norm < 0.0 { 1.0 + pan_norm } else { 1.0 };
            }

            if self.dms_scratch_l.len() < buf_frames {
                self.dms_scratch_l.resize(buf_frames, 0.0);
            }
            if self.dms_scratch_r.len() < buf_frames {
                self.dms_scratch_r.resize(buf_frames, 0.0);
            }
            self.dms_scratch_l[..buf_frames].fill(0.0);
            self.dms_scratch_r[..buf_frames].fill(0.0);
            self.dms_voice_pool.process_all_with_gains(
                &mut self.dms_scratch_l[..buf_frames],
                &mut self.dms_scratch_r[..buf_frames],
                &dms_gain_l,
                &dms_gain_r,
                self.sample_rate,
            );
            if self.transport.playback_state != TransportPlaybackState::Playing {
                let mut wt_gains_l = [0.0f32; 16];
                let mut wt_gains_r = [0.0f32; 16];
                for t in 0..16 {
                    let w_vol = self.track_wavetable_volumes[t];
                    wt_gains_l[t] = dms_gain_l[t] * w_vol;
                    wt_gains_r[t] = dms_gain_r[t] * w_vol;
                }
                self.wavetable_voice_pool.process_all_with_track_tables(
                    &self.track_wavetables,
                    &mut self.dms_scratch_l[..buf_frames],
                    &mut self.dms_scratch_r[..buf_frames],
                    &wt_gains_l,
                    &wt_gains_r,
                    self.sample_rate,
                );
                self.sampler_voice_pool.process_all_idle(
                    &self.track_sampler_samples,
                    &self.track_sampler_params,
                    &mut self.dms_scratch_l[..buf_frames],
                    &mut self.dms_scratch_r[..buf_frames],
                    &dms_gain_l,
                    &dms_gain_r,
                    self.sample_rate,
                );
                self.open_poly_voice_pool.process_all_idle(
                    &self.track_open_poly_enabled,
                    &self.track_open_poly_params,
                    &self.track_open_poly_wavetables,
                    &mut self.dms_scratch_l[..buf_frames],
                    &mut self.dms_scratch_r[..buf_frames],
                    &dms_gain_l,
                    &dms_gain_r,
                    self.sample_rate,
                );
                self.open_grid_voice_pool.process_all_idle(
                    &self.track_open_grid_enabled,
                    &mut self.dms_scratch_l[..buf_frames],
                    &mut self.dms_scratch_r[..buf_frames],
                    &dms_gain_l,
                    &dms_gain_r,
                    self.sample_rate,
                );
                for i in 0..buf_frames {
                    samples[i * 2] += self.dms_scratch_l[i];
                    samples[i * 2 + 1] += self.dms_scratch_r[i];
                }
            }
        }

        if self.transport.playback_state == TransportPlaybackState::Playing {
            let buffer_frames = (samples.len() / num_channels) as u64;
            if buffer_frames == 0 {
                self.output_level_bits.store(0.0f32.to_bits(), Ordering::Relaxed);
                for p in &self.track_peak_bits {
                    p.store(0.0f32.to_bits(), Ordering::Relaxed);
                }
                return;
            }
            let buf_len = samples.len();
            let frame_start = self.transport.sample_count;
            let abs_start = self.absolute_frame;
            let transport = self.transport;
            let mode = self.mode;

            let any_solo = self.track_solos.iter().any(|s| s.load(Ordering::Relaxed));
            let mut track_gain_l = [0.0f32; 16];
            let mut track_gain_r = [0.0f32; 16];
            for t in 0..16 {
                let vol = f32::from_bits(self.track_volumes[t].load(Ordering::Relaxed));
                let muted = self.track_mutes[t].load(Ordering::Relaxed);
                let soloed = self.track_solos[t].load(Ordering::Relaxed);
                let effective_gain = if muted {
                    0.0
                } else if any_solo && !soloed {
                    0.0
                } else {
                    vol
                };
                let pan_norm = f32::from_bits(self.track_pans[t].load(Ordering::Relaxed)) / 100.0;
                track_gain_l[t] = effective_gain * if pan_norm > 0.0 {
                    1.0 - pan_norm
                } else {
                    1.0
                };
                track_gain_r[t] = effective_gain * if pan_norm < 0.0 {
                    1.0 + pan_norm
                } else {
                    1.0
                };
            }

            for buf in self.track_output_bufs.iter_mut() {
                if buf.len() != buf_len {
                    buf.resize(buf_len, 0.0);
                }
                buf.fill(0.0);
            }

            for t in 0..16 {
                let w_vol = self.track_wavetable_volumes[t];
                let gl = track_gain_l[t] * w_vol;
                let gr = track_gain_r[t] * w_vol;
                if let Some(table) = &self.track_wavetables[t] {
                    self.wavetable_voice_pool.process_track(
                        t,
                        table,
                        &mut self.track_output_bufs[t],
                        gl,
                        gr,
                        buffer_frames as usize,
                        self.sample_rate,
                    );
                }
                if let Some(sample) = &self.track_sampler_samples[t] {
                    self.sampler_voice_pool.process_track(
                        t,
                        sample,
                        &self.track_sampler_params[t],
                        &mut self.track_output_bufs[t],
                        track_gain_l[t],
                        track_gain_r[t],
                        buffer_frames as usize,
                        self.sample_rate,
                    );
                }
                if self.track_open_poly_enabled[t] {
                    self.open_poly_voice_pool.process_track(
                        t,
                        &self.track_open_poly_params[t],
                        self.track_open_poly_wavetables[t].as_deref(),
                        &mut self.track_output_bufs[t],
                        track_gain_l[t],
                        track_gain_r[t],
                        buffer_frames as usize,
                        self.sample_rate,
                    );
                }
                if self.track_open_grid_enabled[t] {
                    self.open_grid_voice_pool.process_track(
                        t,
                        &mut self.track_output_bufs[t],
                        track_gain_l[t],
                        track_gain_r[t],
                        buffer_frames as usize,
                        self.sample_rate,
                    );
                }
            }

            for clip in self.clips.iter_mut() {
                let ti = clip.track_index.min(15);
                let gl = track_gain_l[ti] * clip.gain;
                let gr = track_gain_r[ti] * clip.gain;
                if gl == 0.0 && gr == 0.0 {
                    continue;
                }
                let track_buf = &mut self.track_output_bufs[ti];

                let ch = clip.channels.max(1);
                let buf_ptr = clip.samples.as_ptr();
                let src_len = clip.samples.len();
                let offset_frames = clip.sample_offset / ch;
                let has_loop = clip.has_valid_clip_loop();

                if mode == EngineMode::OpenStudio {
                    let clip_frame_end = clip.start_frame.saturating_add(clip.duration_frames);
                    for f in 0..buffer_frames as usize {
                        let global_frame = transport.wrap_sample_count(frame_start + f as u64);
                        if global_frame < clip.start_frame || global_frame >= clip_frame_end {
                            continue;
                        }
                        let relative_frame = (global_frame - clip.start_frame) as usize;
                        let idx = (offset_frames + relative_frame) * ch;

                        if idx + ch <= src_len {
                            let l = unsafe { *buf_ptr.add(idx) } * gl;
                            let r = if ch > 1 {
                                (unsafe { *buf_ptr.add(idx + 1) }) * gr
                            } else {
                                l
                            };
                            track_buf[f * 2] += l;
                            track_buf[f * 2 + 1] += r;
                        }
                    }
                } else {
                    if !clip.is_playing {
                        continue;
                    }

                    if abs_start < clip.start_absolute {
                        continue;
                    }

                    let elapsed_start = abs_start - clip.start_absolute;
                    let emission = clip.emission_len_frames();

                    if !has_loop && elapsed_start >= emission {
                        clip.is_playing = false;
                        continue;
                    }

                    let natural = clip.natural_frames();
                    if natural == 0 {
                        continue;
                    }

                    // ── Modo multi-evento: el pad mezcla sus regiones ──
                    let has_rt_expr = clip.expressions.has_gain_or_pan();
                    if !clip.events.is_empty() {
                        let rate = clip.tempo_rate(&transport);
                        if ((rate - 1.0).abs() >= 1e-4 || clip.has_active_warp() || clip.has_active_pitch_or_formant())
                            && !clip.stretched_samples.is_empty()
                        {
                            let emission_out = ((emission as f64) / rate).round() as u64;
                            let loop_start_out = ((clip.clip_loop_start.min(natural) as f64) / rate).round() as u64;
                            let loop_len = if has_loop { clip.quantized_loop_len(&transport) } else { 0 };
                            let norm_denom = emission_out.max(1) as f64;
                            let mut clip_finished = false;
                            for f in 0..buffer_frames as usize {
                                let elapsed = elapsed_start + f as u64;
                                if !has_loop && elapsed >= emission_out {
                                    clip_finished = true;
                                    break;
                                }
                                let out_frame = if has_loop && loop_len > 0 {
                                    let loop_end = loop_start_out + loop_len;
                                    if elapsed < loop_end {
                                        elapsed
                                    } else {
                                        loop_start_out + ((elapsed - loop_start_out) % loop_len)
                                    }
                                } else {
                                    elapsed
                                };
                                let (l, r) = clip.sample_stereo_stretched(out_frame, rate);
                                let (el, er) = if has_rt_expr {
                                    clip.expressions.eval_rt_gain_pan((out_frame as f64 / norm_denom).clamp(0.0, 1.0))
                                } else {
                                    (1.0, 1.0)
                                };
                                track_buf[f * 2] += l * gl * el;
                                track_buf[f * 2 + 1] += r * gr * er;
                            }
                            if clip_finished {
                                clip.is_playing = false;
                            }
                            continue;
                        }
                        let norm_denom = natural.max(1) as f64;
                        let mut clip_finished = false;
                        for f in 0..buffer_frames as usize {
                            let elapsed = elapsed_start + f as u64;
                            let Some(clip_frame) = clip.clip_frame_for_elapsed(elapsed, &transport) else {
                                clip_finished = true;
                                break;
                            };
                            let (l, r) = clip.mix_events_at(clip_frame);
                            let (el, er) = if has_rt_expr {
                                clip.expressions.eval_rt_gain_pan((clip_frame as f64 / norm_denom).clamp(0.0, 1.0))
                            } else {
                                (1.0, 1.0)
                            };
                            track_buf[f * 2] += l * gl * el;
                            track_buf[f * 2 + 1] += r * gr * er;
                        }
                        if clip_finished {
                            clip.is_playing = false;
                        }
                        continue;
                    }

                    let rate = clip.tempo_rate(&transport);
                    let emission_out = ((emission as f64) / rate).round() as u64;
                    let norm_denom = emission_out.max(1) as f64;
                    let (loop_start, loop_len) = if has_loop {
                        let ls = ((clip.clip_loop_start.min(natural) as f64) / rate).round() as u64;
                        // Longitud CUANTIZADA a beats: sin esto el wrap cae
                        // fuera de la grilla en cada vuelta y el clip se
                        // desfasage del pulso (ver `quantized_loop_len`).
                        let len = clip.quantized_loop_len(&transport);
                        (ls, len)
                    } else {
                        (0, 0)
                    };

                    let xfade_frames: usize = if has_loop { 32 } else { 0 };
                    let mut clip_finished = false;

                    for f in 0..buffer_frames as usize {
                        let elapsed = elapsed_start + f as u64;

                        if !has_loop && elapsed >= emission_out {
                            clip_finished = true;
                            break;
                        }

                        let relative_frame = if has_loop {
                            let loop_end = loop_start + loop_len;
                            if elapsed < loop_end {
                                elapsed as usize
                            } else {
                                (loop_start + ((elapsed - loop_start) % loop_len)) as usize
                            }
                        } else {
                            elapsed as usize
                        };

                        let (el, er) = if has_rt_expr {
                            clip.expressions.eval_rt_gain_pan((relative_frame as f64 / norm_denom).clamp(0.0, 1.0))
                        } else {
                            (1.0, 1.0)
                        };
                        let gl_f = gl * el;
                        let gr_f = gr * er;

                        if (rate - 1.0).abs() >= 1e-4 || clip.has_active_warp() || clip.has_active_pitch_or_formant() {
                            let (l, r) = clip.sample_stereo_stretched(relative_frame as u64, rate);
                            track_buf[f * 2] += l * gl_f;
                            track_buf[f * 2 + 1] += r * gr_f;
                            continue;
                        }

                        let idx = relative_frame * ch;

                        if xfade_frames > 0
                            && clip.prev_frame != usize::MAX
                            && relative_frame < clip.prev_frame
                            && clip.prev_frame - relative_frame > (emission / 2) as usize
                        {
                            clip.xfade_remaining = xfade_frames as u32;
                            clip.xfade_len = xfade_frames as u32;
                        }

                        if clip.xfade_remaining > 0 && idx + ch <= src_len {
                            let fade_pos = clip.xfade_len - clip.xfade_remaining;
                            let fade_total = clip.xfade_len as f32;
                            let out_gain = fade_pos as f32 / fade_total;
                            let in_gain = 1.0 - out_gain;

                            let l_new = unsafe { *buf_ptr.add(idx) } * gl_f;
                            let r_new = if ch > 1 {
                                (unsafe { *buf_ptr.add(idx + 1) }) * gr_f
                            } else {
                                l_new
                            };

                            let prev_idx = clip.prev_frame * ch;
                            let (l_old, r_old) = if prev_idx + ch <= src_len {
                                let lo = unsafe { *buf_ptr.add(prev_idx) } * gl_f;
                                let ro = if ch > 1 {
                                    (unsafe { *buf_ptr.add(prev_idx + 1) }) * gr_f
                                } else {
                                    lo
                                };
                                (lo, ro)
                            } else {
                                (0.0, 0.0)
                            };

                            track_buf[f * 2] += l_old * in_gain + l_new * out_gain;
                            track_buf[f * 2 + 1] += r_old * in_gain + r_new * out_gain;
                            clip.xfade_remaining -= 1;
                        } else if idx + ch <= src_len {
                            let l = unsafe { *buf_ptr.add(idx) } * gl_f;
                            let r = if ch > 1 {
                                (unsafe { *buf_ptr.add(idx + 1) }) * gr_f
                            } else {
                                l
                            };
                            track_buf[f * 2] += l;
                            track_buf[f * 2 + 1] += r;
                        }

                        clip.prev_frame = relative_frame;
                    }

                    if clip_finished {
                        clip.is_playing = false;
                    }
                }
            }

            // Paso 1: procesar cadenas de efectos externos (VST3/CLAP) in-place sobre
            // `track_output_bufs[t]`, pasando el transporte real del proyecto (BPM,
            // posición en beats/samples, compás, firma y loop) a cada plugin.
            for t in 0..16 {
                if self.track_effects[t].is_empty() {
                    continue;
                }
                let mut upstream_latency = 0u32;
                let mut audio_buffer = AudioBuffer::new(&mut self.track_output_bufs[t][..buf_len]);
                for node in self.track_effects[t].iter_mut() {
                    if node.is_active {
                        let host_t = build_host_transport(&transport, frame_start, upstream_latency);
                        node.set_transport(host_t);
                        node.process(&mut audio_buffer, &self.track_midi_events[t]);
                        upstream_latency = upstream_latency.saturating_add(node.latency_samples());
                    }
                }
            }

            // Paso 2: calcular la latencia total por pista y el máximo global (PDC).
            // Se evalúa después de `process()` para que el bloque #0 (tras `activate` /
            // `start_processing`) y cualquier cambio de tamaño FFT queden alineados al instante.
            let mut track_latency = [0u32; 16];
            let mut max_latency = 0u32;
            for t in 0..16 {
                let mut sum = 0u32;
                for node in &self.track_effects[t] {
                    if node.is_active {
                        sum = sum.saturating_add(node.latency_samples());
                    }
                }
                let clamped = sum.min(MAX_PDC_DELAY_FRAMES as u32);
                track_latency[t] = clamped;
                if clamped > max_latency {
                    max_latency = clamped;
                }
            }
            self.current_pdc_latency = max_latency;

            // Paso 3: compensación automática de latencia (PDC) en DMS y en todas las pistas
            // más rápidas antes de mezclar al bus Master.
            if self.fx_scratch_buf.len() < buf_len {
                self.fx_scratch_buf.resize(buf_len, 0.0);
            }
            let dms_frames = buffer_frames as usize;
            for f in 0..dms_frames {
                self.fx_scratch_buf[f * 2] = self.dms_scratch_l[f];
                self.fx_scratch_buf[f * 2 + 1] = self.dms_scratch_r[f];
            }
            self.dms_pdc_delay
                .process_interleaved(&mut self.fx_scratch_buf[..buf_len], max_latency as usize);
            for i in 0..buf_len {
                samples[i] += self.fx_scratch_buf[i];
            }

            for t in 0..16 {
                let comp_delay = max_latency.saturating_sub(track_latency[t]) as usize;
                let track_buf = &mut self.track_output_bufs[t][..buf_len];
                self.track_pdc_delays[t].process_interleaved(track_buf, comp_delay);
                let mut track_peak = 0.0f32;
                for i in 0..buf_len {
                    let v = track_buf[i];
                    let a = if v.is_finite() { v.abs() } else { 0.0 };
                    if a > track_peak {
                        track_peak = a;
                    }
                    samples[i] += v;
                }
                self.track_peak_bits[t].store(track_peak.to_bits(), Ordering::Relaxed);
            }

            self.transport.sample_count = self.transport.wrap_sample_count(frame_start + buffer_frames);
            self.absolute_frame = self.absolute_frame.saturating_add(buffer_frames);

            let mut track_playheads = [0u64; 16];
            if mode == EngineMode::OpenLive {
                for clip in &self.clips {
                    if !clip.is_playing || clip.track_index >= 16 || self.absolute_frame < clip.start_absolute {
                        continue;
                    }
                    let elapsed = self.absolute_frame - clip.start_absolute;
                    let natural = clip.natural_frames();
                    if natural == 0 {
                        continue;
                    }
                    let frame_opt = if !clip.events.is_empty() {
                        clip.clip_frame_for_elapsed(elapsed, &self.transport)
                    } else if clip.has_valid_clip_loop() {
                        let ls = clip.clip_loop_start.min(natural);
                        let len = clip.quantized_loop_len(&self.transport);
                        let le = ls + len;
                        Some(if elapsed < le {
                            elapsed
                        } else {
                            ls + ((elapsed - le) % len)
                        })
                    } else {
                        let emission = clip.emission_len_frames();
                        if elapsed < emission {
                            Some(elapsed)
                        } else {
                            None
                        }
                    };
                    if let Some(frame) = frame_opt {
                        track_playheads[clip.track_index] = pack_clip_playhead(clip.scene_index, frame);
                    }
                }
            } else {
                let global_frame = self.transport.sample_count;
                for clip in &self.clips {
                    if clip.track_index >= 16 {
                        continue;
                    }
                    let clip_frame_end = clip.start_frame.saturating_add(clip.duration_frames);
                    if global_frame >= clip.start_frame && global_frame < clip_frame_end {
                        let rel = global_frame - clip.start_frame;
                        track_playheads[clip.track_index] = pack_clip_playhead(clip.scene_index, rel);
                    }
                }
            }
            for (t, &packed) in track_playheads.iter().enumerate() {
                self.track_clip_playhead[t].store(packed, Ordering::Relaxed);
            }
        } else {
            // Incluso con el transporte detenido, mezclar el audio de monitoreo
            // proveniente de ventanas de plugins externos abiertas (p. ej. al
            // tocar el teclado en pantalla de Vital) respetando volumen/paneo/mute.
            let buf_len = samples.len();
            let any_solo = self.track_solos.iter().any(|s| s.load(Ordering::Relaxed));
            for t in 0..16 {
                let has_monitor = !self.track_effects[t].is_empty()
                    && self.track_effects[t]
                        .iter()
                        .any(|n| n.is_active && n.tap.is_some());
                if !has_monitor {
                    self.track_peak_bits[t].store(0.0f32.to_bits(), Ordering::Relaxed);
                    continue;
                }
                if self.track_output_bufs[t].len() != buf_len {
                    self.track_output_bufs[t].resize(buf_len, 0.0);
                }
                let track_buf = &mut self.track_output_bufs[t][..buf_len];
                track_buf.fill(0.0);
                let mut mixed_any = false;
                for node in &self.track_effects[t] {
                    if node.is_active {
                        if let Some(tap) = &node.tap {
                            if tap.mix_monitor_into(track_buf) > 0 {
                                mixed_any = true;
                            }
                        }
                    }
                }
                if mixed_any {
                    let vol = f32::from_bits(self.track_volumes[t].load(Ordering::Relaxed));
                    let muted = self.track_mutes[t].load(Ordering::Relaxed);
                    let soloed = self.track_solos[t].load(Ordering::Relaxed);
                    let effective_gain = if muted || (any_solo && !soloed) {
                        0.0
                    } else {
                        vol
                    };
                    let pan_norm = f32::from_bits(self.track_pans[t].load(Ordering::Relaxed)) / 100.0;
                    let gl = effective_gain * if pan_norm > 0.0 { 1.0 - pan_norm } else { 1.0 };
                    let gr = effective_gain * if pan_norm < 0.0 { 1.0 + pan_norm } else { 1.0 };
                    let mut track_peak = 0.0f32;
                    for f in 0..(buf_len / 2) {
                        let l = track_buf[f * 2] * gl;
                        let r = track_buf[f * 2 + 1] * gr;
                        let al = if l.is_finite() { l.abs() } else { 0.0 };
                        let ar = if r.is_finite() { r.abs() } else { 0.0 };
                        if al > track_peak {
                            track_peak = al;
                        }
                        if ar > track_peak {
                            track_peak = ar;
                        }
                        samples[f * 2] += l;
                        samples[f * 2 + 1] += r;
                    }
                    self.track_peak_bits[t].store(track_peak.to_bits(), Ordering::Relaxed);
                } else {
                    self.track_peak_bits[t].store(0.0f32.to_bits(), Ordering::Relaxed);
                }
            }
            for p in &self.track_clip_playhead {
                p.store(0, Ordering::Relaxed);
            }
        }

        flush_denormals(samples);

        let mg = f32::from_bits(self.master_gain.load(Ordering::Relaxed));
        if mg != 1.0 {
            for s in samples.iter_mut() {
                *s *= mg;
            }
        }

        let mut peak = 0.0f32;
        for s in samples.iter_mut() {
            *s = s.tanh();
            let a = (*s).abs();
            if a > peak {
                peak = a;
            }
        }
        self.output_level_bits.store(peak.to_bits(), Ordering::Relaxed);
        self.position_clock.store(self.transport.sample_count, Ordering::Relaxed);
        self.absolute_clock.store(self.absolute_frame, Ordering::Relaxed);
    }

    pub fn update_midi_clip(
        &mut self,
        track_index: usize,
        scene_index: usize,
        notes: Vec<MidiNoteInstance>,
    ) {
        if let Some(clip) = self
            .midi_clips
            .iter_mut()
            .find(|c| c.track_index == track_index && c.scene_index == scene_index)
        {
            let max_tick = notes.iter().map(|n| n.end_tick()).max().unwrap_or(960 * 4);
            clip.duration_ticks = max_tick.max(960 * 4);
            if clip.clip_loop_end_ticks <= clip.clip_loop_start_ticks || clip.clip_loop_end_ticks < clip.duration_ticks {
                clip.clip_loop_end_ticks = clip.duration_ticks;
            }
            clip.notes = notes;
        } else {
            self.midi_clips.push(MidiClipInstance::new(track_index, scene_index, notes));
        }
    }

    pub fn set_midi_clip_loop(
        &mut self,
        track_index: usize,
        scene_index: usize,
        loop_start_ticks: u64,
        loop_end_ticks: u64,
    ) {
        if let Some(clip) = self
            .midi_clips
            .iter_mut()
            .find(|c| c.track_index == track_index && c.scene_index == scene_index)
        {
            clip.clip_loop_start_ticks = loop_start_ticks;
            clip.clip_loop_end_ticks = loop_end_ticks.max(loop_start_ticks + 240);
            let max_tick = clip.notes.iter().map(|n| n.end_tick()).max().unwrap_or(clip.clip_loop_end_ticks);
            clip.duration_ticks = clip.clip_loop_end_ticks.max(max_tick);
        }
    }

    pub fn set_midi_clip_loop_enabled(
        &mut self,
        track_index: usize,
        scene_index: usize,
        enabled: bool,
    ) {
        if let Some(clip) = self
            .midi_clips
            .iter_mut()
            .find(|c| c.track_index == track_index && c.scene_index == scene_index)
        {
            clip.clip_loop_enabled = enabled;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    use std::sync::Mutex;

    static TABLE: [f32; 2048] = [0.0; 2048];

    fn test_engine() -> AudioEngine<'static> {
        let clock = Arc::new(AtomicU64::new(0));
        let engine = AudioEngine::new(SampleRate::new(44100.0), &TABLE, clock);
        for t in 0..16 {
            engine.track_volumes[t].store(1.0f32.to_bits(), Ordering::Relaxed);
        }
        engine.master_gain.store(1.0f32.to_bits(), Ordering::Relaxed);
        engine
    }

    fn run_frames(engine: &mut AudioEngine, frames: u64) {
        let mut raw = vec![0.0f32; (frames * 2) as usize];
        let mut buf = AudioBuffer::new(&mut raw);
        engine.process(&mut buf);
    }

    #[test]
    fn openlive_clip_duration_is_real_audio_length() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        let samples = vec![0.5f32; 44100 * 2];
        engine.add_clip(1, 0, 0, samples, 0.0, 0.0, 0.0, 2, false);
        assert_eq!(engine.clips[0].duration_frames, 44100);
        assert_eq!(engine.clips[0].playback_length_frames(), 44100);
    }

    #[test]
    fn trigger_clip_toggles_and_restarts_phase() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.transport.sample_count = 8000;
        engine.add_clip(1, 0, 0, vec![0.5f32; 100 * 2], 0.0, 0.0, 0.0, 2, false);
        engine.trigger_clip(0, 0);
        assert!(engine.clips[0].is_playing);
        assert_eq!(engine.clips[0].start_frame, 8000);
        engine.trigger_clip(0, 0);
        assert!(!engine.clips[0].is_playing);
    }

    #[test]
    fn trigger_scene_plays_whole_row() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.add_clip(1, 0, 0, vec![0.5f32; 100 * 2], 0.0, 0.0, 0.0, 2, false);
        engine.add_clip(2, 1, 0, vec![0.5f32; 100 * 2], 0.0, 0.0, 0.0, 2, false);
        engine.add_clip(3, 0, 1, vec![0.5f32; 100 * 2], 0.0, 0.0, 0.0, 2, false);
        engine.trigger_scene(1);
        assert!(!engine.clips[0].is_playing);
        assert!(!engine.clips[1].is_playing);
        assert!(engine.clips[2].is_playing);
    }

    #[test]
    fn global_loop_wraps_in_engine_not_gui() {
        let mut engine = test_engine();
        engine.play();
        engine.set_global_loop(0, 44100, true);
        for _ in 0..100 {
            run_frames(&mut engine, 512);
        }
        assert_eq!(engine.transport.sample_count, 51200 - 44100);
        assert_eq!(
            engine.position_clock.load(Ordering::Relaxed),
            engine.transport.sample_count
        );
    }

    #[test]
    fn global_loop_disabled_never_wraps() {
        let mut engine = test_engine();
        engine.play();
        for _ in 0..100 {
            run_frames(&mut engine, 512);
        }
        assert_eq!(engine.transport.sample_count, 51200);
    }

    #[test]
    fn voice_frame_mutes_past_natural_without_clip_loop() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.add_clip(1, 0, 0, vec![0.5f32; 1000 * 2], 0.0, 0.0, 0.0, 2, false);
        let t = engine.transport;
        assert!(!engine.clips[0].has_valid_clip_loop());
        assert_eq!(engine.clips[0].voice_frame(0, &t), Some(0));
        assert_eq!(engine.clips[0].voice_frame(999, &t), Some(999));
        assert_eq!(engine.clips[0].voice_frame(1000, &t), None);
        assert_eq!(engine.clips[0].voice_frame(5000, &t), None);
    }

    #[test]
    fn voice_frame_with_explicit_clip_loop_wraps() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.add_clip(1, 0, 0, vec![0.5f32; 1000 * 2], 0.0, 0.0, 0.0, 2, false);
        engine.set_clip_loop(0, 0, 0.0, 0.25, true);
        assert!(engine.clips[0].has_valid_clip_loop());
        let t = engine.transport;
        let wrapped = engine.clips[0].voice_frame(5000, &t).unwrap();
        assert!((0..1000).contains(&(wrapped as u64)));
    }

    #[test]
    fn clip_loop_region_is_honored() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.add_clip(1, 0, 0, vec![0.5f32; 44100 * 2], 0.0, 0.0, 0.0, 2, false);
        engine.set_clip_loop(0, 0, 0.0, 0.5, true);
        let clip = &engine.clips[0];
        assert!(clip.has_valid_clip_loop());
        assert_eq!(clip.clip_loop_start, 0);
        assert_eq!(clip.clip_loop_end, 22050);
        assert_eq!(clip.playback_length_frames(), 22050);
        engine.set_clip_loop(0, 0, 0.0, 0.0, false);
        assert!(!engine.clips[0].has_valid_clip_loop());
        assert_eq!(engine.clips[0].playback_length_frames(), 44100);
    }

    #[test]
    fn loop_activado_sin_region_loopea_el_clip_entero() {
        // Es el caso que produce `apply_matrix` al abrir un proyecto: un pad
        // recién cargado tiene `loop_enabled = true` y región (0, 0).
        // `set_clip_loop` NO debe tomar eso como "desactivar": tiene que dejar
        // el loop puesto sobre el clip entero.
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.add_clip(1, 0, 0, vec![0.5f32; 1000 * 2], 0.0, 0.0, 0.0, 2, true);
        engine.set_clip_loop(0, 0, 0.0, 0.0, true);

        let clip = &engine.clips[0];
        assert!(
            clip.has_valid_clip_loop(),
            "loop activado sin región debe loopear el clip entero, no quedar off"
        );
        assert_eq!(clip.clip_loop_start, 0);
        assert_eq!(clip.clip_loop_end, 1000);
    }

    #[test]
    fn un_clip_con_loop_sigue_sonando_pasada_la_longitud_del_buffer() {
        // Requisito (a): con el loop puesto, la voz entrega muestras VÁLIDAS
        // indefinidamente. Antes, al llegar al final se terminaba.
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        // 100 frames, con un rampa para poder distinguir dónde estamos.
        let samples: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        engine.add_clip(1, 0, 0, samples.clone(), 0.0, 0.0, 0.0, 1, true);
        // Loop sobre la región completa.
        engine.set_clip_loop(0, 0, 0.0, 100.0 / engine.sample_rate, true);
        assert!(engine.clips[0].has_valid_clip_loop());

        let t = engine.transport;
        // Dentro del clip: la rampa avanza.
        assert_eq!(engine.clips[0].voice_frame(0, &t), Some(0));
        assert_eq!(engine.clips[0].voice_frame(50, &t), Some(50));
        // PASADA la longitud del buffer: sigue Active, no None.
        assert_eq!(engine.clips[0].voice_frame(100, &t), Some(0));
        assert_eq!(engine.clips[0].voice_frame(150, &t), Some(50));
        // Y mucho más allá: sigue activo y dentro del rango.
        for pos in [1_000u64, 10_000, 100_000] {
            let f = engine.clips[0]
                .voice_frame(pos, &t)
                .unwrap_or_else(|| panic!("la voz murió en {} con el loop puesto", pos));
            assert!(f < 100, "lejo fuera del clip: {}", f);
        }
        assert_eq!(
            engine.clips[0].voice_state(10_000, &t),
            VoiceState::Active
        );
    }

    #[test]
    fn sin_loop_el_clip_se_termina_en_el_final() {
        // Contracara del test anterior: apagado el loop, la voz se apaga en el
        // final del buffer (no queda sonando para siempre).
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.add_clip(1, 0, 0, vec![0.5f32; 100 * 2], 0.0, 0.0, 0.0, 2, true);
        engine.set_clip_loop(0, 0, 0.0, 0.0, false);
        assert!(!engine.clips[0].has_valid_clip_loop());

        let t = engine.transport;
        assert_eq!(engine.clips[0].voice_frame(99, &t), Some(99));
        assert_eq!(engine.clips[0].voice_frame(100, &t), None);
        assert_eq!(
            engine.clips[0].voice_state(500, &t),
            VoiceState::Finished
        );
    }

    #[test]
    fn dos_clips_de_igual_duracion_en_beats_no_se_desfasan() {
        // Este es el bug de phase drift: dos clips que duran LO MISMO en beats
        // tienen longitudes de WAV distintas (por ejemplo uno 지하 una barra y
        // el otro cuatro, o el mismo sample recortado), así que la longitud de
        // loop cruda no coincide. Con la longitud libre, cada uno envuelve en un
        // instante distinto y se desfasan entre sí a partir de la segunda vuelta.
        //
        // 150 BPM a 44100 Hz = 17640 samples por beat exactos.
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.transport.set_bpm(150.0);
        let spb = 17640u64;
        assert_eq!(engine.transport.samples_per_beat().round() as u64, spb);

        // Un beat y medio exacto de audio en cada uno, pero con "colas" de
        // distinta longitud que los dejan en 19200 y 21000 frames (ninguno
        // múltiplo de 17640).
        engine.add_clip(1, 0, 0, vec![0.5f32; 19200], 0.0, 0.0, 0.0, 1, true);
        engine.add_clip(2, 0, 1, vec![0.5f32; 21000], 0.0, 0.0, 0.0, 1, true);
        engine.set_clip_loop(0, 0, 0.0, 19200.0 / 44100.0, true);
        engine.set_clip_loop(0, 1, 0.0, 21000.0 / 44100.0, true);

        let len_a = engine.clips[0].quantized_loop_len(&engine.transport);
        let len_b = engine.clips[1].quantized_loop_len(&engine.transport);
        assert_eq!(
            len_a, len_b,
            "clips de la misma duración en beats deben loopear el mismo largo"
        );
        assert_eq!(
            len_a % spb,
            0,
            "la longitud del loop tiene que caer exacto en la grilla de beats"
        );
        assert_eq!(len_a, spb, "un solo beat de loop (17640 frames)");

        // Y ahora la prueba de verdad: leer MUCHAS vueltas y comprobar que los
        // dos clips devuelven exactamente la misma fase en cada sample.
        engine.clips[0].start_absolute = 0;
        engine.clips[0].start_frame = 0;
        engine.clips[1].start_absolute = 0;
        engine.clips[1].start_frame = 0;
        for pos in 0..(spb * 12) {
            let a = engine.clips[0].clip_frame_for_elapsed(pos, &engine.transport);
            let b = engine.clips[1].clip_frame_for_elapsed(pos, &engine.transport);
            assert_eq!(
                a, b,
                "los clips se desfasaron en el sample {} (12 vueltas): {:?} vs {:?}",
                pos, a, b
            );
        }
    }

#[test]
    fn el_loop_cai_exacto_en_el_pulso_sin_drift_de_punto_flotante() {
        // 150 BPM, sample rate 44100: 17640 samples/beat exacto. Tras 50 vueltas
        // el wrap tiene que seguir cayendo en el frame 0 del clip, sin deriva.
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.transport.set_bpm(150.0);
        let spb = 17640u64;
        // WAV de 1.234 beats: 21771 frames (no múltiplo de 17640).
        engine.add_clip(1, 0, 0, vec![0.5f32; 21771], 0.0, 0.0, 0.0, 1, true);
        engine.set_clip_loop(0, 0, 0.0, 21771.0 / 44100.0, true);
        let len = engine.clips[0].quantized_loop_len(&engine.transport);
        assert_eq!(len % spb, 0);

        for vuelta in 1..50u64 {
            let wrap_at = len * vuelta;
            assert_eq!(
                engine.clips[0].clip_frame_for_elapsed(wrap_at, &engine.transport),
                Some(0),
                "el wrap de la vuelta {} no cayó en el inicio del clip",
                vuelta
            );
            assert_eq!(
                engine.clips[0]
                    .clip_frame_for_elapsed(wrap_at + len - 1, &engine.transport),
                Some(len - 1),
                "la vuelta {} no terminó justo antes del wrap",
                vuelta
            );
        }
    }

    #[test]
    fn un_loop_menor_a_un_beat_no_se_cuantiza() {
        // Los loops sub-beat (stutter/flam) son intencionales: cuantizarlos los
        // rompería, así que se respetan tal cual.
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.transport.set_bpm(150.0);
        // 500 frames de loop, muy por debajo de un beat (17640).
        engine.add_clip(1, 0, 0, vec![0.5f32; 20000], 0.0, 0.0, 0.0, 1, true);
        engine.set_clip_loop(0, 0, 0.0, 500.0 / 44100.0, true);
        assert_eq!(engine.clips[0].quantized_loop_len(&engine.transport), 500);
    }

    #[test]
    fn play_stops_preview_immediately() {
        let mut engine = test_engine();
        engine.preview_player.play(vec![0.5f32; 1024], 1);
        assert!(engine.preview_player.is_playing());
        engine.play();
        assert!(!engine.preview_player.is_playing());
    }

    #[test]
    fn process_in_playing_always_mixes_preview() {
        let mut engine = test_engine();
        engine.preview_player.play(vec![0.5f32; 4096], 1);
        engine.play();
        engine.preview_player.play(vec![0.5f32; 4096], 1);
        assert!(engine.preview_player.is_playing());
        let mut raw = vec![0.0f32; 512 * 2];
        let mut buf = AudioBuffer::new(&mut raw);
        engine.process(&mut buf);
        let peak = raw.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert!(peak > 0.0);
        assert!(engine.output_peak() > 0.0);
    }

    #[test]
    fn clip_length_frames_uses_real_transport_tempo() {
        let mut engine = test_engine();
        engine.transport.set_bpm(120.0);
        let len_120 = engine.transport.clip_length_frames(3840);
        engine.transport.set_bpm(135.0);
        let len_135 = engine.transport.clip_length_frames(3840);
        assert_ne!(len_120, len_135);
        assert_eq!(len_120, 88200);
        assert_eq!(len_135, 78400);
        assert_eq!(len_135, engine.transport.ticks_to_samples(3840));
    }

    #[test]
    fn openlive_bar10_is_silent_with_15bar_time_selection() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        let spb = engine.transport.samples_per_bar();
        let bar_frames = spb.round() as u64;
        let nine_bars = bar_frames * 9;
        let fifteen_bars = bar_frames * 15;
        engine.add_clip(
            1, 0, 0,
            vec![0.5f32; (nine_bars as usize) * 2],
            0.0, 0.0, 0.0, 2, false,
        );
        assert!(!engine.clips[0].has_valid_clip_loop());
        engine.set_global_loop(0, fifteen_bars, true);
        engine.trigger_clip(0, 0);
        engine.play();
        let mut remaining = nine_bars;
        while remaining > 0 {
            let step = remaining.min(512);
            run_frames(&mut engine, step);
            remaining -= step;
        }
        let mut raw = vec![0.777f32; 512 * 2];
        let mut buf = AudioBuffer::new(&mut raw);
        engine.process(&mut buf);
        let peak = raw.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert_eq!(peak, 0.0);
        assert_eq!(engine.output_peak(), 0.0);
        assert_eq!(engine.output_db(), f32::NEG_INFINITY);
        assert!(!engine.clips[0].is_playing);
    }

    #[test]
    fn openlive_short_time_selection_does_not_extend_voice() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        let spb = engine.transport.samples_per_bar();
        let bar_frames = spb.round() as u64;
        let nine_bars = bar_frames * 9;
        engine.add_clip(
            1, 0, 0,
            vec![0.5f32; (nine_bars as usize) * 2],
            0.0, 0.0, 0.0, 2, false,
        );
        engine.set_global_loop(0, bar_frames * 4, true);
        engine.trigger_clip(0, 0);
        engine.play();
        let mut remaining = bar_frames * 10;
        let mut last_peak = 1.0f32;
        while remaining > 0 {
            let step = remaining.min(512);
            let mut raw = vec![0.0f32; (step * 2) as usize];
            let mut buf = AudioBuffer::new(&mut raw);
            engine.process(&mut buf);
            last_peak = raw.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
            remaining -= step;
        }
        assert_eq!(last_peak, 0.0);
        assert!(!engine.clips[0].is_playing);
        assert_eq!(engine.output_db(), f32::NEG_INFINITY);
    }

    #[test]
    fn openlive_explicit_clip_loop_keeps_sounding_past_bar9() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        let spb = engine.transport.samples_per_bar();
        let bar_frames = spb.round() as u64;
        let nine_bars = bar_frames * 9;
        engine.add_clip(
            1, 0, 0,
            vec![0.5f32; (nine_bars as usize) * 2],
            0.0, 0.0, 0.0, 2, false,
        );
        let sr = engine.sample_rate;
        engine.set_clip_loop(0, 0, 0.0, (bar_frames as f32 / sr) as f32, true);
        assert!(engine.clips[0].has_valid_clip_loop());
        engine.set_global_loop(0, bar_frames * 15, true);
        engine.trigger_clip(0, 0);
        engine.play();
        let mut remaining = bar_frames * 10;
        let mut last_peak = 0.0f32;
        while remaining > 0 {
            let step = remaining.min(512);
            let mut raw = vec![0.0f32; (step * 2) as usize];
            let mut buf = AudioBuffer::new(&mut raw);
            engine.process(&mut buf);
            last_peak = raw.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
            remaining -= step;
        }
        let expected_peak = 0.5f32.tanh();
        assert!(
            (last_peak - expected_peak).abs() < 1e-4,
            "peak {} debe ser ≈ tanh(0.5) = {}",
            last_peak, expected_peak
        );
        assert!(engine.clips[0].is_playing);
    }

    #[test]
    fn openlive_trigger_starts_at_sample_zero() {
        for flow in 0..3 {
            let mut engine = test_engine();
            engine.set_mode(EngineMode::OpenLive);
            let n = 8192u64;
            let mut samples = Vec::with_capacity((n * 2) as usize);
            for i in 0..n {
                let v = i as f32 / n as f32;
                samples.push(v);
                samples.push(v);
            }
            engine.add_clip(1, 0, 0, samples, 0.0, 0.0, 0.0, 2, false);
            match flow {
                0 => {
                    engine.trigger_clip(0, 0);
                    engine.play();
                }
                1 => {
                    engine.play();
                    run_frames(&mut engine, 200_000);
                    engine.trigger_clip(0, 0);
                }
                _ => {
                    engine.trigger_clip(0, 0);
                    engine.play();
                    run_frames(&mut engine, 1000);
                    engine.stop();
                    engine.trigger_clip(0, 0);
                    engine.trigger_clip(0, 0);
                    engine.play();
                }
            }
            let mut raw = vec![0.0f32; 512 * 2];
            let mut buf = AudioBuffer::new(&mut raw);
            engine.process(&mut buf);
            for f in 0..512usize {
                let expect = f as f32 / n as f32;
                assert!(
                    (raw[f * 2] - expect).abs() < 2e-3,
                    "flow {}: frame L{} = {} (esperado {})",
                    flow, f, raw[f * 2], expect
                );
                assert!(
                    (raw[f * 2 + 1] - expect).abs() < 2e-3,
                    "flow {}: frame R{} = {} (esperado {})",
                    flow, f, raw[f * 2 + 1], expect
                );
            }
        }
    }

    #[test]
    fn openlive_strict_close_past_native_length() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        let n = 4096u64;
        engine.add_clip(1, 0, 0, vec![0.5f32; (n * 2) as usize], 0.0, 0.0, 0.0, 2, false);
        engine.trigger_clip(0, 0);
        engine.play();
        let mut remaining = n;
        while remaining > 0 {
            let step = remaining.min(512);
            run_frames(&mut engine, step);
            remaining -= step;
        }
        for _ in 0..3 {
            let mut raw = vec![0.777f32; 512 * 2];
            let mut buf = AudioBuffer::new(&mut raw);
            engine.process(&mut buf);
            let peak = raw.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
            assert_eq!(peak, 0.0);
        }
        assert!(!engine.clips[0].is_playing);
        assert_eq!(engine.output_db(), f32::NEG_INFINITY);
    }

    #[test]
    fn openstudio_bar1_starts_at_sample_zero_and_ends_silent() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenStudio);
        let n = 4096u64;
        let mut samples = Vec::with_capacity((n * 2) as usize);
        for i in 0..n {
            let v = i as f32 / n as f32;
            samples.push(v);
            samples.push(v);
        }
        let duration_secs = n as f32 / engine.sample_rate;
        engine.add_clip(1, 0, 0, samples, 0.0, duration_secs, 0.0, 2, false);
        engine.transport.sample_count = 0;
        engine.play();
        let mut raw = vec![0.0f32; 512 * 2];
        let mut buf = AudioBuffer::new(&mut raw);
        engine.process(&mut buf);
        for f in 0..512usize {
            let expect = f as f32 / n as f32;
            assert!(
                (raw[f * 2] - expect).abs() < 2e-3,
                "frame L{} = {} (esperado {})",
                f, raw[f * 2], expect
            );
        }
        let mut remaining = n;
        while remaining > 0 {
            let step = remaining.min(512);
            run_frames(&mut engine, step);
            remaining -= step;
        }
        let mut raw = vec![0.777f32; 512 * 2];
        let mut buf = AudioBuffer::new(&mut raw);
        engine.process(&mut buf);
        let peak = raw.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert_eq!(peak, 0.0);
        assert_eq!(engine.output_db(), f32::NEG_INFINITY);
    }

    #[test]
    fn voice_elapsed_frames_tracks_linear_voice_clock() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.add_clip(1, 0, 0, vec![0.5f32; 8192 * 2], 0.0, 0.0, 0.0, 2, false);
        assert_eq!(engine.voice_elapsed_frames(0, 0), None);
        assert_eq!(engine.voice_elapsed_frames(9, 9), None);
        engine.trigger_clip(0, 0);
        engine.transport.sample_count = 200_000;
        assert_eq!(engine.voice_elapsed_frames(0, 0), Some(0));
        engine.play();
        run_frames(&mut engine, 512);
        assert_eq!(engine.voice_elapsed_frames(0, 0), Some(512));
        engine.transport.sample_count = 0;
        assert_eq!(engine.voice_elapsed_frames(0, 0), Some(512));
    }

    #[test]
    fn voice_playhead_frame_returns_voice_position_and_natural_frames() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.add_clip(1, 0, 0, vec![0.5f32; 8192 * 2], 0.0, 0.0, 0.0, 2, false);
        assert_eq!(engine.voice_playhead_frame(0, 0), None);
        engine.trigger_clip(0, 0);
        assert_eq!(engine.voice_playhead_frame(0, 0), Some((0, 8192)));
        engine.play();
        run_frames(&mut engine, 512);
        assert_eq!(engine.voice_playhead_frame(0, 0), Some((512, 8192)));
        engine.trigger_clip(0, 0); // stop
        assert_eq!(engine.voice_playhead_frame(0, 0), None);
    }

    #[test]
    fn openlive_full_clip_loop_wraps_past_natural_end() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);

        let natural = 4096u64;
        let mut samples = Vec::with_capacity((natural * 2) as usize);
        for i in 0..natural {
            // Mitad silenciosa, mitad fuerte: permite verificar el wrap.
            let v = if i < natural / 2 { 0.0f32 } else { 0.5f32 };
            samples.push(v);
            samples.push(v);
        }
        engine.add_clip(1, 0, 0, samples, 0.0, 0.0, 0.0, 2, false);
        engine.set_clip_loop(
            0,
            0,
            0.0,
            natural as f32 / engine.sample_rate as f32,
            true,
        );
        assert!(engine.clips[0].has_valid_clip_loop());

        engine.trigger_clip(0, 0);
        engine.play();

        // Reproducir hasta justo ANTES del final natural (mitad fuerte).
        let to_loud_half = natural / 2 + 100;
        run_frames(&mut engine, to_loud_half);
        let mut raw = vec![0.0f32; 256 * 2];
        let mut buf = AudioBuffer::new(&mut raw);
        engine.process(&mut buf);
        let peak_loud = raw.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert!(peak_loud > 0.1, "mitad fuerte debe sonar, peak={}", peak_loud);

        // Saltar hasta pasada la mitad silenciosa del primer pase
        // (posición natural/4, que tras wrap cae en la primera mitad = silencio).
        let remaining_to_quarter = natural - to_loud_half + natural / 4;
        run_frames(&mut engine, remaining_to_quarter);
        assert!(engine.clips[0].is_playing, "el clip debe seguir sonando tras el loop");

        let mut raw2 = vec![0.0f32; 256 * 2];
        let mut buf2 = AudioBuffer::new(&mut raw2);
        engine.process(&mut buf2);
        let peak_wrapped = raw2.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert!(
            peak_wrapped < 0.05,
            "tras wrap debe caer en zona silenciosa, peak={}",
            peak_wrapped
        );

        // Avanzar hasta la segunda mitad tras wrap: debe sonar de nuevo.
        let to_loud_after_wrap = natural / 2;
        run_frames(&mut engine, to_loud_after_wrap);
        assert!(engine.clips[0].is_playing);
        let mut raw3 = vec![0.0f32; 256 * 2];
        let mut buf3 = AudioBuffer::new(&mut raw3);
        engine.process(&mut buf3);
        let peak_loud2 = raw3.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert!(
            peak_loud2 > 0.1,
            "segunda mitad tras wrap debe sonar, peak={}",
            peak_loud2
        );
    }

    fn ramp_samples(n: u64) -> Vec<f32> {
        let mut samples = Vec::with_capacity((n * 2) as usize);
        for i in 0..n {
            let v = i as f32 / n as f32;
            samples.push(v);
            samples.push(v);
        }
        samples
    }

    #[test]
    fn legato_pad_switch_same_track_keeps_phase() {
        // Reporte: loopear un pad y disparar el de abajo entraba
        // desincronizado porque el playhead rearrancaba de cero.
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        let n = 8192u64;
        engine.add_clip(1, 0, 0, ramp_samples(n), 0.0, 0.0, 0.0, 2, true);
        engine.add_clip(2, 0, 1, ramp_samples(n), 0.0, 0.0, 0.0, 2, true);
        engine.trigger_clip(0, 0);
        engine.play();
        run_frames(&mut engine, 1000);
        // Cambio de pad en caliente: debe heredar la fase (frame 1000),
        // no rearrancar en 0.
        engine.trigger_clip(0, 1);
        let clip1 = engine.clips.iter().find(|c| c.scene_index == 1).unwrap();
        assert_eq!(clip1.start_absolute, 0);
        let mut raw = vec![0.0f32; 64 * 2];
        let mut buf = AudioBuffer::new(&mut raw);
        engine.process(&mut buf);
        let expect = (1000.0f32 / n as f32).tanh();
        assert!(
            (raw[0] - expect).abs() < 2e-3,
            "pad nuevo debe continuar en fase: {} vs esperado {}",
            raw[0], expect
        );
    }

    #[test]
    fn legato_pad_switch_other_track_inherits_phase() {
        // "o cualquiera": disparar un pad de otro track también entra en fase.
        // Ojo: tracks distintos suenan mezclados (ambos quedan en Playing),
        // así que se verifica herencia de fase + mezcla en fase.
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        let n = 8192u64;
        engine.add_clip(1, 0, 0, ramp_samples(n), 0.0, 0.0, 0.0, 2, true);
        engine.add_clip(2, 1, 0, ramp_samples(n), 0.0, 0.0, 0.0, 2, true);
        engine.trigger_clip(0, 0);
        engine.play();
        run_frames(&mut engine, 2000);
        engine.trigger_clip(1, 0);
        // Herencia de fase: mismo origen que el clip de referencia.
        let c0 = engine.clips.iter().find(|c| c.track_index == 0).unwrap();
        let c1 = engine.clips.iter().find(|c| c.track_index == 1).unwrap();
        assert!(c0.is_playing && c1.is_playing);
        assert_eq!(c1.start_absolute, c0.start_absolute);
        assert_eq!(c1.start_frame, c0.start_frame);
        let mut raw = vec![0.0f32; 64 * 2];
        let mut buf = AudioBuffer::new(&mut raw);
        engine.process(&mut buf);
        // Ambas voces en fase 2000/n, mezcladas y con tanh del máster.
        let expect = (2.0 * 2000.0f32 / n as f32).tanh();
        assert!(
            (raw[0] - expect).abs() < 2e-3,
            "pads de distintos tracks deben sonar en fase: {} vs esperado {}",
            raw[0], expect
        );
    }

    #[test]
    fn legato_first_trigger_starts_at_zero() {
        // Sin nada sonando, el primer pad arranca de cero como siempre.
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        let n = 8192u64;
        engine.add_clip(1, 0, 0, ramp_samples(n), 0.0, 0.0, 0.0, 2, true);
        engine.trigger_clip(0, 0);
        engine.play();
        let mut raw = vec![0.0f32; 64 * 2];
        let mut buf = AudioBuffer::new(&mut raw);
        engine.process(&mut buf);
        assert!((raw[0] - 0.0).abs() < 2e-3);
    }

    #[test]
    fn scene_switch_restarts_phase_from_zero() {
        // El Scene Launcher reinicia la fase: la escena nueva rearranca
        // desde la muestra 0 aunque otra escena llevara 1500 frames sonando.
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        let n = 8192u64;
        engine.add_clip(1, 0, 0, ramp_samples(n), 0.0, 0.0, 0.0, 2, true);
        engine.add_clip(2, 0, 1, ramp_samples(n), 0.0, 0.0, 0.0, 2, true);
        engine.trigger_scene(0);
        engine.play();
        run_frames(&mut engine, 1500);
        engine.trigger_scene(1);
        let mut raw = vec![0.0f32; 64 * 2];
        let mut buf = AudioBuffer::new(&mut raw);
        engine.process(&mut buf);
        let expect = (0.0f32 / n as f32).tanh();
        assert!(
            (raw[0] - expect).abs() < 2e-3,
            "escena nueva debe rearrancar desde 0: {} vs esperado {}",
            raw[0], expect
        );
    }

    #[test]
    fn multi_event_pad_mixes_overlapping_regions() {
        // Un pad con dos eventos solapados debe mezclar por suma.
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.add_clip(1, 0, 0, vec![0.0f32; 100 * 2], 0.0, 0.0, 0.0, 2, true);
        engine.set_clip_events(
            0,
            0,
            vec![
                EngineAudioEvent {
                    id: 1,
                    samples: vec![1.0f32; 500 * 2],
                    channels: 2,
                    clip_start: 0,
                    gain: 1.0,
                    fade_in: 0,
                    fade_out: 0,
                },
                EngineAudioEvent {
                    id: 2,
                    samples: vec![1.0f32; 500 * 2],
                    channels: 2,
                    clip_start: 250,
                    gain: 1.0,
                    fade_in: 0,
                    fade_out: 0,
                },
            ],
        );
        let clip = &engine.clips[0];
        assert_eq!(clip.natural_frames(), 750);
        // Zona solo del primero → 1.0; zona solapada → 2.0.
        assert_eq!(clip.mix_events_at(100), (1.0, 1.0));
        assert_eq!(clip.mix_events_at(300), (2.0, 2.0));
        assert_eq!(clip.mix_events_at(600), (1.0, 1.0));
        // Reproducción real: el frame 300 (solapado) debe sonar al doble.
        engine.trigger_clip(0, 0);
        engine.play();
        run_frames(&mut engine, 300);
        let mut raw = vec![0.0f32; 64 * 2];
        let mut buf = AudioBuffer::new(&mut raw);
        engine.process(&mut buf);
        let expect = (2.0f32).tanh();
        assert!(
            (raw[0] - expect).abs() < 1e-4,
            "solape debe sumar voces: {} vs {}",
            raw[0], expect
        );
    }

    #[test]
    fn track_clip_playhead_tracks_playing_looping_and_stopped_clip() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        let sr = engine.sample_rate as usize;
        // 120 BPM => 0.5s por beat. Clip de 1.0s (2 beats).
        engine.transport.set_bpm(120.0);
        engine.add_clip(1, 0, 2, vec![0.25f32; sr * 2], 0.0, 1.0, 0.0, 2, true);
        // Loop en el segundo beat [0.5s .. 1.0s]
        engine.set_clip_loop(0, 2, 0.5, 1.0, true);

        assert_eq!(unpack_clip_playhead(engine.track_clip_playhead[0].load(Ordering::Relaxed)), None);

        engine.trigger_clip(0, 2);
        engine.play();
        run_frames(&mut engine, 1000);
        assert_eq!(
            unpack_clip_playhead(engine.track_clip_playhead[0].load(Ordering::Relaxed)),
            Some((2, 1000))
        );

        // Avanzar exactamente 1.0s (sr frames): elapsed = 1000 + sr -> envuelve en [sr/2 .. sr)
        let beat_frames = (sr / 2) as u64;
        run_frames(&mut engine, sr as u64);
        assert_eq!(
            unpack_clip_playhead(engine.track_clip_playhead[0].load(Ordering::Relaxed)),
            Some((2, beat_frames + 1000))
        );

        engine.stop();
        run_frames(&mut engine, 512);
        assert_eq!(unpack_clip_playhead(engine.track_clip_playhead[0].load(Ordering::Relaxed)), None);
    }

    #[test]
    fn wsola_time_stretch_preserves_pitch_and_energy() {
        let sr = 44100.0;
        let freq = 220.0;
        let in_frames = 44100; // 1 segundo
        let mut input = Vec::with_capacity(in_frames * 2);
        for i in 0..in_frames {
            let s = (std::f64::consts::TAU * freq * (i as f64) / sr).sin() as f32 * 0.5;
            input.push(s);
            input.push(s);
        }
        // Estirar de 150 BPM a 128 BPM (rate = 128 / 150 < 1.0 => más largo)
        let rate = 128.0 / 150.0;
        let out = time_stretch_wsola(&input, 2, rate);
        let out_frames = out.len() / 2;
        let expected_frames = (in_frames as f64 / rate).round() as usize;
        assert!((out_frames as isize - expected_frames as isize).abs() <= 2);

        // Verificar que la energía RMS se mantiene sin cancelación tipo comb filter
        let rms_in = (input.iter().map(|&x| (x * x) as f64).sum::<f64>() / input.len() as f64).sqrt();
        let rms_out = (out.iter().map(|&x| (x * x) as f64).sum::<f64>() / out.len() as f64).sqrt();
        assert!(
            (rms_out - rms_in).abs() < 0.05,
            "RMS drop indica comb cancellation: in={:.4}, out={:.4}",
            rms_in,
            rms_out
        );

        // Verificar por cruce por cero que la frecuencia fundamental sigue siendo ~220 Hz (sin pitch shift)
        let mut zero_crossings = 0usize;
        let start = 4096;
        let end = out_frames - 4096;
        for f in start..end {
            let a = out[(f - 1) * 2];
            let b = out[f * 2];
            if a <= 0.0 && b > 0.0 {
                zero_crossings += 1;
            }
        }
        let est_freq = zero_crossings as f64 * sr / ((end - start) as f64);
        assert!(
            (est_freq - freq).abs() < 5.0,
            "Frecuencia alterada (pitch shift): esperada {:.1} Hz, medida {:.1} Hz",
            freq,
            est_freq
        );
    }

    #[test]
    fn wsola_warped_shifts_transient_to_target_position() {
        let in_frames = 44100;
        let mut input = vec![0.0f32; in_frames * 2];
        // Colocar un transiente (pulso tonal fuerte) alrededor del 60% del clip (frame 26460)
        let transient_src = (in_frames as f64 * 0.60) as usize;
        for i in 0..1024 {
            let env = 1.0 - (i as f32 / 1024.0);
            let s = (i as f32 * 0.25).sin() * env;
            let idx = (transient_src + i) * 2;
            input[idx] = s;
            input[idx + 1] = s;
        }

        // Mover el transiente del 60% al 30% mediante marcadores de warp
        let warp = vec![(0.0, 0.0), (0.30, 0.60), (1.0, 1.0)];
        let out = time_stretch_wsola_warped(&input, 2, in_frames, &warp);
        assert_eq!(out.len(), in_frames * 2);

        let peak_frame = out
            .chunks_exact(2)
            .enumerate()
            .max_by(|a, b| a.1[0].abs().partial_cmp(&b.1[0].abs()).unwrap())
            .map(|(idx, _)| idx)
            .unwrap();
        let expected = (in_frames as f64 * 0.30) as isize;
        assert!(
            (peak_frame as isize - expected).abs() < 2048,
            "El transiente warpeado cayó en {} en lugar de ~{}",
            peak_frame,
            expected
        );
    }

    #[test]
    fn explicit_clip_legato_toggle_controls_pad_and_scene_phase() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        let n = 8192u64;
        engine.add_clip(1, 0, 0, ramp_samples(n), 0.0, 0.0, 0.0, 2, true);
        engine.add_clip(2, 0, 1, ramp_samples(n), 0.0, 0.0, 0.0, 2, true);
        engine.add_clip(3, 0, 2, ramp_samples(n), 0.0, 0.0, 0.0, 2, true);

        // Clip en escena 1 con legato desactivado explícitamente -> reinicia en 0 al cambiar pad
        engine.set_clip_legato(0, 1, false);
        // Clip en escena 2 con legato activado explícitamente -> hereda fase incluso al cambiar escena
        engine.set_clip_legato(0, 2, true);

        engine.trigger_clip(0, 0);
        engine.play();
        run_frames(&mut engine, 1200);

        engine.trigger_clip(0, 1);
        let c1 = engine.clips.iter().find(|c| c.scene_index == 1).unwrap();
        assert_eq!(c1.start_absolute, 1200, "legato=false debe reiniciar en 1200");

        run_frames(&mut engine, 800);
        // Ahora c1 lleva 800 frames desde 1200 (start_absolute=1200, abs=2000).
        // Disparar escena 2 con legato=true debe heredar start_absolute=1200.
        engine.trigger_scene(2);
        let c2 = engine.clips.iter().find(|c| c.scene_index == 2).unwrap();
        assert!(c2.is_playing);
        assert_eq!(c2.start_absolute, 1200, "legato=true debe heredar fase en trigger_scene");
    }

    #[test]
    fn insert_and_remove_scene_reindex_clips_and_overrides() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.add_clip(1, 0, 0, ramp_samples(4096), 0.0, 0.0, 0.0, 2, true);
        engine.add_clip(2, 0, 1, ramp_samples(4096), 0.0, 0.0, 0.0, 2, true);
        engine.set_scene_launch_quantization(1, Some(QuantizationGrid::Sixteenth));

        // Insertar escena en índice 1: el clip 2 y el override 1/16 pasan a escena 2
        engine.insert_scene(1);
        assert_eq!(engine.clips.iter().find(|c| c.id == 1).unwrap().scene_index, 0);
        assert_eq!(engine.clips.iter().find(|c| c.id == 2).unwrap().scene_index, 2);
        assert_eq!(engine.scene_quantization_overrides[1], None);
        assert_eq!(
            engine.scene_quantization_overrides[2],
            Some(QuantizationGrid::Sixteenth)
        );

        // Disparar clip 1 (escena 0) y luego eliminar escena 0: clip 1 se borra y clip 2 pasa a escena 1
        engine.trigger_clip(0, 0);
        engine.remove_scene(0);
        assert!(engine.clips.iter().all(|c| c.id != 1));
        assert_eq!(engine.clips.iter().find(|c| c.id == 2).unwrap().scene_index, 1);
        assert_eq!(
            engine.scene_quantization_overrides[1],
            Some(QuantizationGrid::Sixteenth)
        );
    }

    #[test]
    fn move_scene_reindexes_clips_and_overrides_in_engine() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.add_clip(1, 0, 0, ramp_samples(4096), 0.0, 0.0, 0.0, 2, true);
        engine.add_clip(2, 0, 1, ramp_samples(4096), 0.0, 0.0, 0.0, 2, true);
        engine.add_clip(3, 0, 2, ramp_samples(4096), 0.0, 0.0, 0.0, 2, true);
        engine.set_scene_launch_quantization(0, Some(QuantizationGrid::Eighth));
        engine.set_scene_launch_quantization(2, Some(QuantizationGrid::Bar));

        // Mover escena 0 -> 2: [0, 1, 2] -> [1, 2, 0]
        engine.move_scene(0, 2);
        assert_eq!(engine.clips.iter().find(|c| c.id == 2).unwrap().scene_index, 0);
        assert_eq!(engine.clips.iter().find(|c| c.id == 3).unwrap().scene_index, 1);
        assert_eq!(engine.clips.iter().find(|c| c.id == 1).unwrap().scene_index, 2);
        assert_eq!(engine.scene_quantization_overrides[1], Some(QuantizationGrid::Bar));
        assert_eq!(engine.scene_quantization_overrides[2], Some(QuantizationGrid::Eighth));

        // Mover escena 2 -> 0: vuelve a [0, 1, 2]
        engine.move_scene(2, 0);
        assert_eq!(engine.clips.iter().find(|c| c.id == 1).unwrap().scene_index, 0);
        assert_eq!(engine.clips.iter().find(|c| c.id == 2).unwrap().scene_index, 1);
        assert_eq!(engine.clips.iter().find(|c| c.id == 3).unwrap().scene_index, 2);
        assert_eq!(engine.scene_quantization_overrides[0], Some(QuantizationGrid::Eighth));
        assert_eq!(engine.scene_quantization_overrides[2], Some(QuantizationGrid::Bar));
    }

    #[test]
    fn clip_launch_modes_and_gain_handle_in_engine() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.add_clip(1, 0, 0, vec![0.8f32; 4096 * 2], 0.0, 0.0, 0.0, 2, true);
        engine.add_clip(2, 0, 1, vec![0.8f32; 4096 * 2], 0.0, 0.0, 0.0, 2, true);
        engine.add_clip(3, 0, 2, vec![0.8f32; 4096 * 2], 0.0, 0.0, 0.0, 2, true);

        engine.set_clip_launch_mode(0, 0, ClipLaunchMode::Trigger);
        engine.set_clip_launch_mode(0, 1, ClipLaunchMode::Toggle);
        engine.set_clip_launch_mode(0, 2, ClipLaunchMode::Gate);

        // Trigger: dos triggers consecutivos mantienen is_playing=true
        engine.trigger_clip(0, 0);
        assert!(engine.clips.iter().find(|c| c.id == 1).unwrap().is_playing);
        engine.trigger_clip(0, 0);
        assert!(engine.clips.iter().find(|c| c.id == 1).unwrap().is_playing);

        // Toggle: primer trigger arranca, segundo detiene
        engine.trigger_clip(0, 1);
        assert!(engine.clips.iter().find(|c| c.id == 2).unwrap().is_playing);
        engine.trigger_clip(0, 1);
        assert!(!engine.clips.iter().find(|c| c.id == 2).unwrap().is_playing);

        // Gate: trigger arranca, release_clip detiene
        engine.trigger_clip(0, 2);
        assert!(engine.clips.iter().find(|c| c.id == 3).unwrap().is_playing);
        engine.release_clip(0, 2);
        assert!(!engine.clips.iter().find(|c| c.id == 3).unwrap().is_playing);

        // Clip gain: comparar amplitud con gain=1.0 vs gain=0.25
        engine.play();
        engine.trigger_clip(0, 0);
        engine.set_clip_gain(0, 0, 1.0);
        let mut out_full = vec![0.0f32; 256 * 2];
        engine.process(&mut AudioBuffer::new(&mut out_full));
        let peak_full = out_full.iter().map(|s| s.abs()).fold(0.0f32, f32::max);

        engine.set_clip_gain(0, 0, 0.25);
        let mut out_quarter = vec![0.0f32; 256 * 2];
        engine.process(&mut AudioBuffer::new(&mut out_quarter));
        let peak_quarter = out_quarter.iter().map(|s| s.abs()).fold(0.0f32, f32::max);

        assert!(peak_full > 0.1, "debe emitir audio con gain=1.0");
        assert!(
            (peak_quarter - peak_full * 0.25).abs() < 0.05,
            "gain=0.25 debe escalar salida (~25%): full={peak_full}, quarter={peak_quarter}"
        );
    }

    #[test]
    fn scene_launch_quantization_and_per_scene_override_in_engine() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.transport.set_bpm(120.0); // 44100 Hz -> 22050 samples/beat, 88200 samples/bar
        engine.set_global_launch_quantization(QuantizationGrid::Bar);
        engine.set_scene_launch_quantization(1, Some(QuantizationGrid::None));
        engine.set_scene_launch_quantization(2, Some(QuantizationGrid::Eighth));

        engine.add_clip(1, 0, 0, ramp_samples(8192), 0.0, 0.0, 0.0, 2, true);
        engine.add_clip(2, 0, 1, ramp_samples(8192), 0.0, 0.0, 0.0, 2, true);
        engine.add_clip(3, 0, 2, ramp_samples(8192), 0.0, 0.0, 0.0, 2, true);

        engine.play();
        run_frames(&mut engine, 1000); // pos = 1000

        // Escena 0 hereda global Bar -> queda en cola para sample 88200
        engine.trigger_scene(0);
        let pending = engine.pending_scene_launch().expect("debe quedar en cola");
        assert_eq!(pending.scene_index, 0);
        assert_eq!(pending.target_sample, 88200);
        assert!(!engine.clips.iter().find(|c| c.id == 1).unwrap().is_playing);

        // Escena 1 tiene override None -> dispara inmediatamente y cancela la cola
        engine.trigger_scene(1);
        assert!(engine.pending_scene_launch().is_none());
        assert!(engine.clips.iter().find(|c| c.id == 2).unwrap().is_playing);

        // Escena 2 tiene override Eighth (11025 samples) -> pos=1000 => target=11025
        engine.trigger_scene(2);
        let pending2 = engine.pending_scene_launch().expect("escena 2 en cola");
        assert_eq!(pending2.scene_index, 2);
        assert_eq!(pending2.target_sample, 11025);
        assert!(!engine.clips.iter().find(|c| c.id == 3).unwrap().is_playing);

        // Avanzar hasta 10500: todavía no disparó
        run_frames(&mut engine, 9500);
        assert!(!engine.clips.iter().find(|c| c.id == 3).unwrap().is_playing);

        // Avanzar 1000 más (cruza 11025): se dispara exactamente en 11025
        run_frames(&mut engine, 1000);
        assert!(engine.pending_scene_launch().is_none());
        let c3 = engine.clips.iter().find(|c| c.id == 3).unwrap();
        assert!(c3.is_playing);
        assert_eq!(c3.start_frame, 11025);
    }

    #[test]
    fn midi_clip_loop_start_end_and_enabled_toggle_work() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.update_midi_clip(
            0,
            0,
            vec![MidiNoteInstance::new(480, 60, 100, 240)],
        );
        engine.set_midi_clip_loop(0, 0, 480, 960);
        {
            let mc = engine.midi_clips.iter().find(|c| c.track_index == 0 && c.scene_index == 0).unwrap();
            assert_eq!(mc.clip_loop_start_ticks, 480);
            assert_eq!(mc.clip_loop_end_ticks, 960);
            assert!(mc.clip_loop_enabled);
        }
        engine.set_midi_clip_loop_enabled(0, 0, false);
        {
            let mc = engine.midi_clips.iter().find(|c| c.track_index == 0 && c.scene_index == 0).unwrap();
            assert!(!mc.clip_loop_enabled);
            assert!(!mc.has_valid_clip_loop());
        }
    }

    #[test]
    fn stop_track_is_quantized_to_next_bar_while_playing() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.transport.set_bpm(120.0); // 44100 Hz -> 88200 samples/bar
        engine.set_global_launch_quantization(QuantizationGrid::Bar);
        engine.add_clip(1, 0, 0, ramp_samples(200_000), 0.0, 0.0, 0.0, 2, true);

        engine.play();
        engine.trigger_clip(0, 0);
        run_frames(&mut engine, 10_000); // mid-bar 1
        assert!(engine.clips[0].is_playing);

        // Stop track mid-bar -> queued until bar 2 (sample 88200), still playing now.
        engine.stop_track(0);
        assert!(engine.clips[0].is_playing);
        assert_eq!(engine.pending_track_stops.len(), 1);
        assert_eq!(engine.pending_track_stops[0].target_sample, 88200);
        assert_eq!(
            engine.pending_stop_tracks_mask.load(Ordering::Relaxed) & 1,
            1
        );

        // Advance to sample 80_000 (still in bar 1): still playing.
        run_frames(&mut engine, 70_000);
        assert!(engine.clips[0].is_playing);

        // Cross bar 2 boundary (88200): clip stops, mask clears, transport stays Playing.
        run_frames(&mut engine, 10_000);
        assert!(!engine.clips[0].is_playing);
        assert!(engine.pending_track_stops.is_empty());
        assert_eq!(
            engine.pending_stop_tracks_mask.load(Ordering::Relaxed) & 1,
            0
        );
        assert_eq!(
            engine.transport.playback_state,
            TransportPlaybackState::Playing
        );
    }

    #[test]
    fn pitch_and_formant_wsola_preserve_length_and_shift_pitch() {
        let sr = 44100.0f32;
        let frames = 16384usize;
        let freq = 220.0f32;
        let mut mono = Vec::with_capacity(frames);
        for i in 0..frames {
            mono.push((std::f32::consts::TAU * freq * i as f32 / sr).sin() * 0.5);
        }

        // +12 semitonos (una octava arriba) manteniendo exacta duración
        let mut pitch = hikaru_core::ExpressionCurve::default();
        pitch.insert_point(0.0, 12.0);
        pitch.insert_point(1.0, 12.0);
        let flat = hikaru_core::ExpressionCurve::default();

        let pitched = time_stretch_and_pitch_wsola(&mono, 1, frames, &[], &pitch, &flat);
        assert_eq!(pitched.len(), frames);
        assert!(pitched.iter().all(|s| s.is_finite()));

        // Contar cruces por cero en la mitad estable: debería ~duplicarse para +12 st
        let zc = |buf: &[f32]| -> usize {
            buf[2048..14336]
                .windows(2)
                .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
                .count()
        };
        let zc_orig = zc(&mono);
        let zc_up = zc(&pitched);
        assert!(
            zc_up > (zc_orig as f32 * 1.7) as usize && zc_up < (zc_orig as f32 * 2.3) as usize,
            "expected ~2x zero-crossings for +12st: orig={zc_orig}, up={zc_up}"
        );

        // Formant shift preserva longitud y produce muestras finitas con energía
        let mut formant = hikaru_core::ExpressionCurve::default();
        formant.insert_point(0.0, 4.0);
        formant.insert_point(1.0, -4.0);
        let formanted = apply_formant_curve_stft(&mono, 1, &formant);
        assert_eq!(formanted.len(), frames);
        assert!(formanted.iter().all(|s| s.is_finite()));
        let energy: f32 = formanted.iter().map(|s| s * s).sum();
        assert!(energy > 1.0, "formant output should retain signal energy");
    }

    struct MockLatencyPlugin {
        latency: u32,
        delay: PdcDelayLine,
        seen_transports: Arc<Mutex<Vec<hikaru_plugin_host::HostTransportInfo>>>,
    }

    impl MockLatencyPlugin {
        fn new(
            latency: u32,
            seen_transports: Arc<Mutex<Vec<hikaru_plugin_host::HostTransportInfo>>>,
        ) -> Self {
            Self {
                latency,
                delay: PdcDelayLine::new(),
                seen_transports,
            }
        }
    }

    impl PluginInstance for MockLatencyPlugin {
        fn process(&mut self, buffer: &mut AudioBuffer) {
            self.delay.process_interleaved(buffer.samples, self.latency as usize);
        }
        fn latency_samples(&self) -> u32 {
            self.latency
        }
        fn set_transport(&mut self, transport: hikaru_plugin_host::HostTransportInfo) {
            if let Ok(mut guard) = self.seen_transports.lock() {
                guard.push(transport);
            }
        }
        fn show_gui_embedded(&mut self, _: hikaru_plugin_host::RawWindowHandle) -> Result<(), String> {
            Ok(())
        }
        fn show_gui_floating(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn hide_gui(&mut self) {}
        fn get_name(&self) -> &str {
            "MockLatency"
        }
    }

    #[test]
    fn pdc_aligns_latency_plugin_track_with_dry_drum_track() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.transport.set_bpm(150.0);

        // Track 0: audio clip with an impulse at frame 0 + FFT plugin reporting 1024 samples latency
        let mut track0_samples = vec![0.0f32; 4096 * 2];
        track0_samples[0] = 0.25;
        track0_samples[1] = 0.25;
        engine.add_clip(1, 0, 0, track0_samples, 0.0, 0.0, 0.0, 2, false);

        // Track 1: dry dubstep drum loop with an impulse at frame 0 (0 plugins)
        let mut track1_samples = vec![0.0f32; 4096 * 2];
        track1_samples[0] = 0.25;
        track1_samples[1] = 0.25;
        engine.add_clip(2, 1, 0, track1_samples, 0.0, 0.0, 0.0, 2, false);

        let log = Arc::new(Mutex::new(Vec::new()));
        let plugin = Box::new(MockLatencyPlugin::new(1024, Arc::clone(&log)));
        let node = hikaru_dsp::DspPluginNode::new(
            "MockUZU".to_string(),
            hikaru_dsp::PluginCategory::Vst3Fx,
            plugin,
        );
        engine.track_effects[0].push(node);

        engine.play();
        engine.trigger_scene(0);

        // Render 4 blocks of 512 frames (total 2048 frames)
        let mut rendered_l = Vec::new();
        for _ in 0..4 {
            let mut raw = vec![0.0f32; 512 * 2];
            let mut buf = AudioBuffer::new(&mut raw);
            engine.process(&mut buf);
            for f in 0..512 {
                rendered_l.push(raw[f * 2]);
            }
        }

        assert_eq!(engine.pdc_latency_samples(), 1024);
        // Sample 0 must be silent (both tracks wait for the 1024-sample plugin latency)
        assert!(rendered_l[0].abs() < 1e-6, "frame 0 should be silent, got {}", rendered_l[0]);
        // Sample 1024 must receive BOTH impulses aligned (tanh(0.25 + 0.25) = tanh(0.5))
        let expected = (0.5f32).tanh();
        assert!(
            (rendered_l[1024] - expected).abs() < 1e-5,
            "both tracks must land on exact frame 1024: got {}, expected {}",
            rendered_l[1024],
            expected
        );
        // No ghost/flam impulse anywhere else
        for (idx, &v) in rendered_l.iter().enumerate() {
            if idx != 1024 {
                assert!(v.abs() < 1e-6, "unexpected flam at frame {idx}: {v}");
            }
        }
    }

    #[test]
    fn pdc_updates_when_plugin_bypassed_and_syncs_project_tempo() {
        let mut engine = test_engine();
        engine.set_mode(EngineMode::OpenLive);
        engine.transport.set_bpm(150.0);

        let log1 = Arc::new(Mutex::new(Vec::new()));
        let log2 = Arc::new(Mutex::new(Vec::new()));
        engine.track_effects[0].push(
            hikaru_dsp::DspPluginNode::new(
                "Up".to_string(),
                hikaru_dsp::PluginCategory::ClapFx,
                Box::new(MockLatencyPlugin::new(512, Arc::clone(&log1))),
            )
            .with_slot_idx(0),
        );
        engine.track_effects[0].push(
            hikaru_dsp::DspPluginNode::new(
                "Down".to_string(),
                hikaru_dsp::PluginCategory::Vst3Fx,
                Box::new(MockLatencyPlugin::new(256, Arc::clone(&log2))),
            )
            .with_slot_idx(1),
        );

        engine.play();
        run_frames(&mut engine, 512);
        run_frames(&mut engine, 512);

        assert_eq!(engine.pdc_latency_samples(), 768);
        let t1 = log1.lock().unwrap();
        let t2 = log2.lock().unwrap();
        assert_eq!(t1.len(), 2);
        assert!((t1[0].tempo - 150.0).abs() < 1e-6);
        assert!(t1[0].is_playing);
        assert_eq!(t1[0].sample_position, 0);
        assert_eq!(t1[1].sample_position, 512);
        assert!(t1[1].beats_position > t1[0].beats_position);
        // Downstream plugin sees timeline offset by upstream plugin's 512-sample latency
        assert_eq!(t2[1].sample_position, 0);
        drop(t1);
        drop(t2);

        // Bypass both plugins -> PDC drops back to 0 immediately
        engine.set_external_plugin_active(0, 0, false);
        engine.set_external_plugin_active(0, 1, false);
        run_frames(&mut engine, 256);
        assert_eq!(engine.pdc_latency_samples(), 0);
    }

    #[test]
    fn external_plugin_window_monitor_sounds_when_transport_is_stopped() {
        struct SilentStub;
        impl hikaru_plugin_host::PluginInstance for SilentStub {
            fn process(&mut self, _buffer: &mut AudioBuffer) {}
            fn show_gui_embedded(
                &mut self,
                _h: hikaru_plugin_host::RawWindowHandle,
            ) -> Result<(), String> {
                Ok(())
            }
            fn show_gui_floating(&mut self) -> Result<(), String> {
                Ok(())
            }
            fn hide_gui(&mut self) {}
            fn get_name(&self) -> &str {
                "SilentStub"
            }
        }

        let mut engine = test_engine();
        let tap = Arc::new(hikaru_plugin_host::PluginTap::new(48_000.0, 1024));
        let mut node = hikaru_dsp::DspPluginNode::new(
            "Vital".into(),
            hikaru_dsp::PluginCategory::ClapFx,
            Box::new(SilentStub),
        )
        .with_slot_idx(0);
        node.set_tap(Some(Arc::clone(&tap)));
        engine.track_effects[0].push(node);

        // Transporte detenido: la ventana de Vital publica audio de una nota tocada en su teclado.
        let note_audio = vec![0.4f32; 256 * 2];
        tap.push_monitor(&note_audio);

        let mut out = vec![0.0f32; 256 * 2];
        engine.process(&mut AudioBuffer::new(&mut out));
        let peak = out.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            peak > 0.2,
            "El audio del teclado de la ventana debe sonar aunque el transporte esté detenido (peak={peak})"
        );
        assert!(engine.track_peak(0) > 0.2);
    }
}