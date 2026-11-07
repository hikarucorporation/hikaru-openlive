/*
 * Hikaru OpenStudio - Audio DAW
 * Copyright (C) 2026 Hikaru OpenStudio Developers
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 */

//! Pipeline GPU (`wgpu` + WGSL SDF) para moldear controles nativos estilo
//! **Bitwig Studio 5.1.7** directamente en el `RenderPass` principal de `eframe`.
//!
//! En lugar de depender de widgets genéricos de `egui` (`egui::Slider`, botones
//! planos), cada cápsula biselada, dial rotativo de `270°`, fader empotrado de
//! cobre-ámbar, barra de paneo bipolar, vúmetro estéreo con Peak-Hold y visor LCD
//! se evalúa por píxel con funciones de distancia firmada (SDF) y anti-aliasing
//! analítico en [`BITWIG_SDF_SHADER`].

use std::sync::atomic::{AtomicU32, Ordering};

use bytemuck::{Pod, Zeroable};
use egui::{Color32, PaintCallbackInfo, Painter, Rect};
use egui_wgpu::wgpu;

/// Tipo de primitiva SDF evaluada en [`BITWIG_SDF_SHADER`].
pub const KIND_CAPSULE: u32 = 0;
pub const KIND_ROTARY_KNOB: u32 = 1;
pub const KIND_FADER_VERTICAL: u32 = 2;
pub const KIND_FADER_HORIZONTAL: u32 = 3;
pub const KIND_STEREO_METER: u32 = 4;
pub const KIND_LCD_BEZEL: u32 = 5;

/// Banderas por instancia empacadas en `meta[1]`.
pub const FLAG_ACTIVE: u32 = 1 << 0;
pub const FLAG_HOVERED: u32 = 1 << 1;
pub const FLAG_SUNKEN: u32 = 1 << 2;
pub const FLAG_JOIN_LEFT: u32 = 1 << 3;
pub const FLAG_JOIN_RIGHT: u32 = 1 << 4;
pub const FLAG_CORNER_NOTCH: u32 = 1 << 5;
pub const FLAG_BIPOLAR: u32 = 1 << 6;
pub const FLAG_SRGB_TARGET: u32 = 1 << 7;

const INITIAL_INSTANCE_CAPACITY: usize = 1024;

/// Instancia GPU de 80 bytes (5 × `vec4`, alineación 16 bytes exacta en Rust y WGSL).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct SdfWidgetGpu {
    /// `(width_pt, height_pt, corner_radius_pt, border_width_pt)`
    pub size_radius: [f32; 4],
    /// Color base RGBA en espacio sRGB `[0..1]`.
    pub col_primary: [f32; 4],
    /// Color de acento / relleno activo RGBA en espacio sRGB `[0..1]`.
    pub col_accent: [f32; 4],
    /// Parámetros continuos según `kind` (ej. `[val, default_val, aux0, aux1]` o `[L, R, holdL, holdR]`).
    pub params: [f32; 4],
    /// `[kind, flags, reserved0, reserved1]`.
    pub meta: [u32; 4],
}

impl SdfWidgetGpu {
    #[inline]
    pub fn rgba(c: Color32) -> [f32; 4] {
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        [
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        ]
    }
}

/// Shader WGSL de primitivas SDF para la interfaz estilo Bitwig Studio 5.1.7.
///
/// Es público para que `tests/gui_tests.rs` lo valide con `naga` en CI sin requerir GPU.
pub const BITWIG_SDF_SHADER: &str = r#"
struct SdfWidget {
    size_radius: vec4<f32>,
    col_primary: vec4<f32>,
    col_accent: vec4<f32>,
    params: vec4<f32>,
    kind_flags: vec4<u32>,
};

@group(0) @binding(0) var<storage, read> widgets: array<SdfWidget>;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) local_px: vec2<f32>,
    @location(1) @interpolate(flat) inst_idx: u32,
};

const CORNERS = array<vec2<f32>, 6>(
    vec2<f32>(0.0, 0.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(1.0, 1.0),
);

const PI: f32 = 3.14159265359;

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VsOut {
    let inst = widgets[ii];
    let uv = CORNERS[vi];
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);

    var vs_out: VsOut;
    vs_out.clip_pos = vec4<f32>(ndc, 0.0, 1.0);
    vs_out.local_px = uv * inst.size_radius.xy;
    vs_out.inst_idx = ii;
    return vs_out;
}

// SDF de caja redondeada con radios independientes por esquina:
// r = (top_right, bottom_right, top_left, bottom_left)
fn sd_round_box(p: vec2<f32>, b: vec2<f32>, r: vec4<f32>) -> f32 {
    var rx = r.zw;
    if (p.x > 0.0) {
        rx = r.xy;
    }
    var rad = rx.x;
    if (p.y > 0.0) {
        rad = rx.y;
    }
    let q = abs(p) - b + vec2<f32>(rad, rad);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0, 0.0))) - rad;
}

fn sd_segment(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-5), 0.0, 1.0);
    return length(pa - ba * h);
}

fn srgb_to_linear_chan(c: f32) -> f32 {
    if (c <= 0.04045) {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

fn finalize_color(rgba: vec4<f32>, flags: u32) -> vec4<f32> {
    let a = clamp(rgba.a, 0.0, 1.0);
    if ((flags & 128u) != 0u) {
        return vec4<f32>(
            srgb_to_linear_chan(clamp(rgba.r, 0.0, 1.0)),
            srgb_to_linear_chan(clamp(rgba.g, 0.0, 1.0)),
            srgb_to_linear_chan(clamp(rgba.b, 0.0, 1.0)),
            a
        );
    }
    return vec4<f32>(clamp(rgba.rgb, vec3<f32>(0.0), vec3<f32>(1.0)), a);
}

// 0u: Cápsula biselada estilo Bitwig Studio (soporta segmentos unidos + esquina triangular)
fn shade_capsule(px: vec2<f32>, extents: vec2<f32>, radius: f32, inst: SdfWidget) -> vec4<f32> {
    let flags = inst.kind_flags.y;
    let is_active = (flags & 1u) != 0u;
    let is_hovered = (flags & 2u) != 0u;
    let is_sunken = (flags & 4u) != 0u;
    let join_l = (flags & 8u) != 0u;
    let join_r = (flags & 16u) != 0u;
    let has_notch = (flags & 32u) != 0u;

    let half_ext = extents * 0.5;
    let p = px - half_ext;
    let r_left = select(radius, 0.0, join_l);
    let r_right = select(radius, 0.0, join_r);
    let radii = vec4<f32>(r_right, r_right, r_left, r_left);

    let d = sd_round_box(p, half_ext - vec2<f32>(0.5, 0.5), radii);
    let cov = 1.0 - smoothstep(-0.55, 0.55, d);
    if (cov <= 0.001) {
        return vec4<f32>(0.0);
    }

    let vy = clamp(px.y / max(extents.y, 1.0), 0.0, 1.0);
    var body = inst.col_primary.rgb;

    if (is_active) {
        body = mix(inst.col_accent.rgb * 1.06, inst.col_accent.rgb * 0.84, vy);
    } else if (is_sunken) {
        body = mix(body * 0.86, body * 1.04, vy);
    } else {
        // Bisel metálico mate vertical característico de la barra superior de Bitwig
        body = mix(body * 1.08, body * 0.88, vy);
    }

    if (is_hovered && !is_active) {
        body = body + vec3<f32>(0.055, 0.055, 0.06);
    }

    // Muesca triangular diagonal inferior-derecha (FILE, PLAY, ADD, EDIT, TRACK)
    if (has_notch) {
        let nx = extents.x - 1.5 - px.x;
        let ny = extents.y - 1.5 - px.y;
        if (nx >= 0.0 && ny >= 0.0) {
            let nd = (nx + ny) - 5.2;
            let n_cov = 1.0 - smoothstep(-0.6, 0.6, nd);
            let notch_col = select(vec3<f32>(0.68, 0.70, 0.73), vec3<f32>(0.18, 0.18, 0.20), is_active);
            body = mix(body, notch_col, n_cov * 0.85);
        }
    }

    // Brillo especular superior (1px interno) o sombra interna si está hundido
    if (!is_sunken && px.y >= 1.0 && px.y <= 2.1 && d < -0.8) {
        body = mix(body, vec3<f32>(1.0), select(0.09, 0.18, is_active));
    } else if (is_sunken && px.y >= 1.0 && px.y <= 2.4 && d < -0.6) {
        body = body * 0.72;
    }

    // Borde perimetral oscuro nítido de 1px (#16171A)
    let border_col = vec3<f32>(0.082, 0.086, 0.098);
    let edge_t = smoothstep(-1.35, -0.35, d);
    var rgb = mix(body, border_col, edge_t);

    // Costura vertical derecha cuando va unido al siguiente segmento
    if (join_r && px.x > extents.x - 1.1) {
        rgb = border_col;
    }

    return vec4<f32>(rgb, cov * inst.col_primary.a);
}

// 1u: Perilla rotativa 270° estilo Bitwig / Lemon-Studio con arco luminoso y aguja SDF
fn shade_rotary_knob(px: vec2<f32>, size: vec2<f32>, inst: SdfWidget) -> vec4<f32> {
    let flags = inst.kind_flags.y;
    let hovered = (flags & 2u) != 0u;
    let bipolar = (flags & 64u) != 0u;
    let val = clamp(inst.params.x, 0.0, 1.0);

    let c = size * 0.5;
    let p = px - c;
    let r_outer = min(size.x, size.y) * 0.47;
    let r_arc_mid = r_outer * 0.90;
    let arc_half_w = max(r_outer * 0.095, 1.15);
    let r_cap = r_outer * 0.72;

    let dist = length(p);
    if (dist > r_outer + 1.5) {
        return vec4<f32>(0.0);
    }

    var col = vec3<f32>(0.0);
    var alpha = 0.0;

    // Sombra proyectada suave bajo el cuerpo de la perilla
    let shadow_d = length(p - vec2<f32>(0.0, 1.3)) - r_cap;
    let shadow_a = (1.0 - smoothstep(-0.5, 2.2, shadow_d)) * 0.45;
    alpha = max(alpha, shadow_a);

    // Ángulo horario desde las 12 en punto: [-PI .. +PI], corte en las 6 en punto
    let theta = atan2(p.x, -p.y);
    let min_ang = -0.75 * PI;
    let max_ang = 0.75 * PI;
    let val_ang = mix(min_ang, max_ang, val);

    // Ranura de recorrido de 270°
    let in_sweep = theta >= min_ang && theta <= max_ang;
    let ring_d = abs(dist - r_arc_mid) - arc_half_w;
    if (in_sweep) {
        let track_cov = 1.0 - smoothstep(-0.55, 0.55, ring_d);
        if (track_cov > 0.0) {
            let track_col = vec3<f32>(0.086, 0.090, 0.102);
            col = mix(col, track_col, track_cov);
            alpha = max(alpha, track_cov * 0.96);

            var lit = false;
            if (bipolar) {
                let lo = min(0.0, val_ang);
                let hi = max(0.0, val_ang);
                lit = theta >= lo && theta <= hi;
            } else {
                lit = theta <= val_ang;
            }
            if (lit && ( val > 0.003 || bipolar )) {
                let glow_col = inst.col_accent.rgb;
                col = mix(col, glow_col, track_cov);
            }
        }
    }

    // Cuerpo circular biselado con sombreado direccional arriba-izquierda
    let cap_d = dist - r_cap;
    let cap_cov = 1.0 - smoothstep(-0.6, 0.6, cap_d);
    if (cap_cov > 0.0) {
        let n_uv = p / max(r_cap, 1.0);
        let light = clamp(-n_uv.x * 0.32 - n_uv.y * 0.58 + 0.5, 0.0, 1.0);
        var cap_col = mix(inst.col_primary.rgb * 0.72, inst.col_primary.rgb * 1.24, light);
        if (hovered) {
            cap_col = cap_col + vec3<f32>(0.05);
        }
        // Anillo oscuro exterior del dial
        let rim_t = smoothstep(-1.4, -0.3, cap_d);
        cap_col = mix(cap_col, vec3<f32>(0.09, 0.095, 0.11), rim_t);

        // Aguja indicadora blanca/cálida
        let dir = vec2<f32>(sin(val_ang), -cos(val_ang));
        let needle_d = sd_segment(p, dir * (r_cap * 0.18), dir * (r_cap * 0.84)) - 0.95;
        let needle_cov = 1.0 - smoothstep(-0.5, 0.5, needle_d);
        cap_col = mix(cap_col, vec3<f32>(0.96, 0.96, 0.98), needle_cov);

        col = mix(col, cap_col, cap_cov);
        alpha = max(alpha, cap_cov);
    }

    return vec4<f32>(col, alpha);
}

// 2u: Fader vertical empotrado cobre-ámbar de Bitwig Mix View
fn shade_fader_vertical(px: vec2<f32>, extents: vec2<f32>, radius: f32, inst: SdfWidget) -> vec4<f32> {
    let flags = inst.kind_flags.y;
    let is_hovered = (flags & 2u) != 0u;
    let val = clamp(inst.params.x, 0.0, 1.0);

    let half_ext = extents * 0.5;
    let p = px - half_ext;
    let d = sd_round_box(p, half_ext - vec2<f32>(0.5, 0.5), vec4<f32>(radius, radius, radius, radius));
    let cov = 1.0 - smoothstep(-0.55, 0.55, d);
    if (cov <= 0.001) {
        return vec4<f32>(0.0);
    }

    // Pozo oscuro empotrado (#18191C) con viñeta interior sutil
    let hx = clamp(px.x / max(extents.x, 1.0), 0.0, 1.0);
    var rgb = mix(vec3<f32>(0.082, 0.086, 0.098), vec3<f32>(0.105, 0.110, 0.125), sin(hx * PI));

    // Línea de referencia 0 dB (~78% de altura desde abajo)
    let zero_y = mix(extents.y - 2.0, 2.0, 0.78);
    if (abs(px.y - zero_y) < 0.6) {
        rgb = mix(rgb, vec3<f32>(0.24, 0.25, 0.27), 0.55);
    }

    // Columna rectangular cobre-ámbar desde la base hasta `cap_y`
    let cap_y = mix(extents.y - 2.0, 2.0, val);
    if (px.y >= cap_y && px.x >= 1.2 && px.x <= extents.x - 1.2 && px.y <= extents.y - 1.2) {
        let fill_t = clamp((extents.y - px.y) / max(extents.y - cap_y, 1.0), 0.0, 1.0);
        var amber = mix(inst.col_primary.rgb * 0.86, inst.col_primary.rgb * 1.12, fill_t);
        if (is_hovered) {
            amber = amber * 1.12;
        }
        rgb = amber;
    }

    // Aguja horizontal naranja brillante de 2px en el nivel exacto
    let cap_d = abs(px.y - cap_y) - 1.05;
    if (px.x >= 1.0 && px.x <= extents.x - 1.0) {
        let cap_cov = 1.0 - smoothstep(-0.45, 0.45, cap_d);
        let cap_col = select(inst.col_accent.rgb, inst.col_accent.rgb * 1.15 + vec3<f32>(0.08), is_hovered);
        rgb = mix(rgb, cap_col, cap_cov);
    }

    // Borde hundido de 1px
    let edge_t = smoothstep(-1.35, -0.35, d);
    rgb = mix(rgb, vec3<f32>(0.055, 0.058, 0.068), edge_t);

    return vec4<f32>(rgb, cov);
}

// 3u: Fader horizontal cobre-ámbar / Barra de paneo bipolar estilo Bitwig
fn shade_fader_horizontal(px: vec2<f32>, extents: vec2<f32>, radius: f32, inst: SdfWidget) -> vec4<f32> {
    let flags = inst.kind_flags.y;
    let is_hovered = (flags & 2u) != 0u;
    let is_bipolar = (flags & 64u) != 0u;
    let val = clamp(inst.params.x, 0.0, 1.0);

    let half_ext = extents * 0.5;
    let p = px - half_ext;
    let d = sd_round_box(p, half_ext - vec2<f32>(0.5, 0.5), vec4<f32>(radius, radius, radius, radius));
    let cov = 1.0 - smoothstep(-0.55, 0.55, d);
    if (cov <= 0.001) {
        return vec4<f32>(0.0);
    }

    let vy = clamp(px.y / max(extents.y, 1.0), 0.0, 1.0);
    var rgb = mix(vec3<f32>(0.090, 0.094, 0.106), vec3<f32>(0.125, 0.130, 0.145), vy);

    let cap_x = mix(2.0, extents.x - 2.0, val);
    let start_x = select(1.2, extents.x * 0.5, is_bipolar);
    let lo_x = min(start_x, cap_x);
    let hi_x = max(start_x, cap_x);

    if (is_bipolar && abs(px.x - extents.x * 0.5) < 0.55) {
        rgb = mix(rgb, vec3<f32>(0.28, 0.29, 0.32), 0.65);
    }

    if (px.x >= lo_x && px.x <= hi_x && px.y >= 1.2 && px.y <= extents.y - 1.2) {
        var amber = mix(inst.col_primary.rgb * 1.08, inst.col_primary.rgb * 0.86, vy);
        if (is_hovered) {
            amber = amber * 1.14;
        }
        rgb = amber;
    }

    // Aguja vertical naranja brillante (#FF7A00)
    if (px.y >= 1.0 && px.y <= extents.y - 1.0) {
        let cap_d = abs(px.x - cap_x) - 0.95;
        let cap_cov = 1.0 - smoothstep(-0.45, 0.45, cap_d);
        let cap_col = select(inst.col_accent.rgb, inst.col_accent.rgb * 1.15 + vec3<f32>(0.08), is_hovered);
        rgb = mix(rgb, cap_col, cap_cov);
    }

    let edge_t = smoothstep(-1.35, -0.35, d);
    rgb = mix(rgb, vec3<f32>(0.058, 0.062, 0.072), edge_t);

    return vec4<f32>(rgb, cov);
}

// 4u: Vúmetro estéreo dual con zonas Verde -> Ámbar -> Rojo y línea Peak-Hold
fn meter_zone_color(h_frac: f32) -> vec3<f32> {
    let green = vec3<f32>(0.14, 0.84, 0.34);
    let lime = vec3<f32>(0.55, 0.88, 0.20);
    let amber = vec3<f32>(0.96, 0.72, 0.14);
    let red = vec3<f32>(0.98, 0.22, 0.16);
    if (h_frac < 0.62) {
        return mix(green, lime, h_frac / 0.62);
    } else if (h_frac < 0.82) {
        return mix(lime, amber, (h_frac - 0.62) / 0.20);
    }
    return mix(amber, red, clamp((h_frac - 0.82) / 0.18, 0.0, 1.0));
}

fn shade_stereo_meter(px: vec2<f32>, extents: vec2<f32>, radius: f32, inst: SdfWidget) -> vec4<f32> {
    let half_ext = extents * 0.5;
    let p = px - half_ext;
    let d = sd_round_box(p, half_ext - vec2<f32>(0.5, 0.5), vec4<f32>(radius, radius, radius, radius));
    let cov = 1.0 - smoothstep(-0.5, 0.5, d);
    if (cov <= 0.001) {
        return vec4<f32>(0.0);
    }

    let mid_x = extents.x * 0.5;
    var rgb = vec3<f32>(0.055, 0.058, 0.068);

    // Separador central de 1px entre canal L y canal R
    if (abs(px.x - mid_x) >= 0.55 && px.x > 0.8 && px.x < extents.x - 0.8 && px.y > 0.8 && px.y < extents.y - 0.8) {
        let is_right = px.x > mid_x;
        let level = clamp(select(inst.params.x, inst.params.y, is_right), 0.0, 1.0);
        let hold = clamp(select(inst.params.z, inst.params.w, is_right), 0.0, 1.0);
        let h_frac = clamp(1.0 - px.y / max(extents.y, 1.0), 0.0, 1.0);
        let zone = meter_zone_color(h_frac);

        // Fondo tenue del canal apagado
        rgb = zone * 0.10 + vec3<f32>(0.035, 0.038, 0.044);

        if (h_frac <= level && level > 0.002) {
            // Sutil modulación de micro-segmentos LED cada 2px
            let seg = 0.92 + 0.08 * step(0.35, fract(px.y * 0.5));
            rgb = zone * seg;
        }

        // Línea de Peak-Hold nítida
        if (hold > 0.01) {
            let hold_y = (1.0 - hold) * extents.y;
            if (abs(px.y - hold_y) < 0.85) {
                rgb = mix(zone * 1.15, vec3<f32>(1.0, 0.96, 0.90), 0.25);
            }
        }
    }

    let edge_t = smoothstep(-1.2, -0.3, d);
    rgb = mix(rgb, vec3<f32>(0.035, 0.038, 0.045), edge_t);
    return vec4<f32>(rgb, cov);
}

// 5u: Pantalla LCD empotrada de la barra de transporte Bitwig
fn shade_lcd_bezel(px: vec2<f32>, extents: vec2<f32>, radius: f32, inst: SdfWidget) -> vec4<f32> {
    let half_ext = extents * 0.5;
    let p = px - half_ext;
    let d = sd_round_box(p, half_ext - vec2<f32>(0.5, 0.5), vec4<f32>(radius, radius, radius, radius));
    let cov = 1.0 - smoothstep(-0.55, 0.55, d);
    if (cov <= 0.001) {
        return vec4<f32>(0.0);
    }

    let vy = clamp(px.y / max(extents.y, 1.0), 0.0, 1.0);
    var rgb = mix(vec3<f32>(0.068, 0.074, 0.086), vec3<f32>(0.092, 0.100, 0.114), vy);

    // Sombra superior interna de cristal hundido
    if (px.y < 3.0) {
        rgb = rgb * mix(0.65, 1.0, px.y / 3.0);
    }
    // Filo inferior biselado claro
    if (px.y > extents.y - 1.6 && d > -1.8) {
        rgb = vec3<f32>(0.29, 0.30, 0.33);
    } else {
        let edge_t = smoothstep(-1.35, -0.35, d);
        rgb = mix(rgb, vec3<f32>(0.035, 0.038, 0.044), edge_t);
    }

    return vec4<f32>(rgb, cov);
}

@fragment
fn fs_main(frag_in: VsOut) -> @location(0) vec4<f32> {
    let inst = widgets[frag_in.inst_idx];
    let extents = inst.size_radius.xy;
    let radius = inst.size_radius.z;
    let kind = inst.kind_flags.x;
    let flags = inst.kind_flags.y;

    var out_col = vec4<f32>(0.0);
    if (kind == 0u) {
        out_col = shade_capsule(frag_in.local_px, extents, radius, inst);
    } else if (kind == 1u) {
        out_col = shade_rotary_knob(frag_in.local_px, extents, inst);
    } else if (kind == 2u) {
        out_col = shade_fader_vertical(frag_in.local_px, extents, radius, inst);
    } else if (kind == 3u) {
        out_col = shade_fader_horizontal(frag_in.local_px, extents, radius, inst);
    } else if (kind == 4u) {
        out_col = shade_stereo_meter(frag_in.local_px, extents, radius, inst);
    } else {
        out_col = shade_lcd_bezel(frag_in.local_px, extents, radius, inst);
    }

    return finalize_color(out_col, flags);
}
"#;

/// Recursos persistentes en `egui_wgpu::CallbackResources`.
pub struct BitwigGpuResources {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    instance_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    capacity: usize,
    is_srgb_target: bool,
    uploaded_frame: u64,
    staged: Vec<SdfWidgetGpu>,
}

impl BitwigGpuResources {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hikaru::bitwig_sdf_shader"),
            source: wgpu::ShaderSource::Wgsl(BITWIG_SDF_SHADER.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hikaru::bitwig_sdf_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hikaru::bitwig_sdf_pl"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("hikaru::bitwig_sdf_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let (instance_buffer, bind_group) =
            Self::create_storage(device, &bind_group_layout, INITIAL_INSTANCE_CAPACITY);

        Self {
            pipeline,
            bind_group_layout,
            instance_buffer,
            bind_group,
            capacity: INITIAL_INSTANCE_CAPACITY,
            is_srgb_target: target_format.is_srgb(),
            uploaded_frame: u64::MAX,
            staged: Vec::with_capacity(INITIAL_INSTANCE_CAPACITY),
        }
    }

    fn create_storage(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        capacity: usize,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        let byte_size = (capacity.max(16) * std::mem::size_of::<SdfWidgetGpu>()) as u64;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hikaru::bitwig_sdf_instances"),
            size: byte_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hikaru::bitwig_sdf_bg"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        (buffer, bind_group)
    }

    fn push_instance(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame_nr: u64,
        mut widget: SdfWidgetGpu,
    ) -> u32 {
        if self.uploaded_frame != frame_nr {
            self.staged.clear();
            self.uploaded_frame = frame_nr;
        }
        if self.is_srgb_target {
            widget.meta[1] |= FLAG_SRGB_TARGET;
        }

        let idx = self.staged.len();
        self.staged.push(widget);

        if self.staged.len() > self.capacity {
            self.capacity = (self.capacity * 2).max(self.staged.len());
            let (buf, bg) = Self::create_storage(device, &self.bind_group_layout, self.capacity);
            self.instance_buffer = buf;
            self.bind_group = bg;
            queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&self.staged));
        } else {
            let offset = (idx * std::mem::size_of::<SdfWidgetGpu>()) as u64;
            queue.write_buffer(&self.instance_buffer, offset, bytemuck::bytes_of(&widget));
        }

        idx as u32
    }
}

/// Registra [`BitwigGpuResources`] al arrancar `eframe` con el formato exacto del swapchain.
pub fn install_render_resources(render_state: Option<&egui_wgpu::RenderState>) {
    let Some(rs) = render_state else {
        return;
    };
    let resources = BitwigGpuResources::new(&rs.device, rs.target_format);
    rs.renderer.write().callback_resources.insert(resources);
}

/// Callback unitario enviado a `egui::Painter` para pintar una primitiva SDF en GPU.
struct BitwigSdfCallback {
    frame_nr: u64,
    widget: SdfWidgetGpu,
    slot: AtomicU32,
}

impl egui_wgpu::CallbackTrait for BitwigSdfCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if !callback_resources.contains::<BitwigGpuResources>() {
            // Fallback perezoso si no se invocó `install_render_resources` en main.rs
            let fallback = BitwigGpuResources::new(device, wgpu::TextureFormat::Bgra8Unorm);
            callback_resources.insert(fallback);
        }
        if let Some(res) = callback_resources.get_mut::<BitwigGpuResources>() {
            let idx = res.push_instance(device, queue, self.frame_nr, self.widget);
            self.slot.store(idx, Ordering::Relaxed);
        }
        Vec::new()
    }

    fn paint(
        &self,
        info: PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'_>,
        callback_resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(res) = callback_resources.get::<BitwigGpuResources>() else {
            return;
        };
        let vp = info.viewport_in_pixels();
        let clip = info.clip_rect_in_pixels();
        if vp.width_px <= 0 || vp.height_px <= 0 || clip.width_px <= 0 || clip.height_px <= 0 {
            return;
        }

        render_pass.set_viewport(
            vp.left_px as f32,
            vp.top_px as f32,
            vp.width_px as f32,
            vp.height_px as f32,
            0.0,
            1.0,
        );
        render_pass.set_scissor_rect(
            clip.left_px.max(0) as u32,
            clip.top_px.max(0) as u32,
            clip.width_px.max(0) as u32,
            clip.height_px.max(0) as u32,
        );
        render_pass.set_pipeline(&res.pipeline);
        render_pass.set_bind_group(0, &res.bind_group, &[]);
        let idx = self.slot.load(Ordering::Relaxed);
        render_pass.draw(0..6, idx..idx + 1);
    }
}

/// Encola un [`SdfWidgetGpu`] en el `Painter` sobre `rect`.
///
/// En tests headless sin backend `wgpu`, `egui` ignora `PaintCallback` de forma
/// segura sin hacer panic.
#[inline]
pub fn paint_sdf_widget(painter: &Painter, rect: Rect, widget: SdfWidgetGpu) {
    if !rect.is_positive() || rect.width() < 0.5 || rect.height() < 0.5 {
        return;
    }
    let frame_nr = painter.ctx().cumulative_pass_nr();
    let cb = egui_wgpu::Callback::new_paint_callback(
        rect,
        BitwigSdfCallback {
            frame_nr,
            widget,
            slot: AtomicU32::new(0),
        },
    );
    painter.add(egui::Shape::Callback(cb));
}

/// Pinta una cápsula biselada estilo Bitwig en GPU.
#[allow(clippy::too_many_arguments)]
pub fn paint_capsule(
    painter: &Painter,
    rect: Rect,
    radius: f32,
    bg_color: Color32,
    accent_color: Color32,
    flags: u32,
) {
    let mut w = rect.width();
    let mut draw_rect = rect;
    // Solapar 1px cuando une por la derecha evita huecos sub-píxel entre segmentos
    if (flags & FLAG_JOIN_RIGHT) != 0 {
        w += 0.5;
        draw_rect.max.x += 0.5;
    }
    paint_sdf_widget(
        painter,
        draw_rect,
        SdfWidgetGpu {
            size_radius: [w, draw_rect.height(), radius, 1.0],
            col_primary: SdfWidgetGpu::rgba(bg_color),
            col_accent: SdfWidgetGpu::rgba(accent_color),
            params: [0.0; 4],
            meta: [KIND_CAPSULE, flags, 0, 0],
        },
    );
}

/// Pinta una perilla rotativa `270°` en GPU.
pub fn paint_rotary_knob(
    painter: &Painter,
    rect: Rect,
    norm_value: f32,
    cap_color: Color32,
    arc_color: Color32,
    hovered: bool,
    bipolar: bool,
) {
    let mut flags = 0u32;
    if hovered {
        flags |= FLAG_HOVERED;
    }
    if bipolar {
        flags |= FLAG_BIPOLAR;
    }
    paint_sdf_widget(
        painter,
        rect,
        SdfWidgetGpu {
            size_radius: [rect.width(), rect.height(), 0.0, 1.0],
            col_primary: SdfWidgetGpu::rgba(cap_color),
            col_accent: SdfWidgetGpu::rgba(arc_color),
            params: [norm_value.clamp(0.0, 1.0), 0.0, 0.0, 0.0],
            meta: [KIND_ROTARY_KNOB, flags, 0, 0],
        },
    );
}

/// Pinta un fader vertical empotrado cobre-ámbar en GPU.
pub fn paint_fader_vertical(
    painter: &Painter,
    rect: Rect,
    norm_value: f32,
    fill_color: Color32,
    cap_color: Color32,
    hovered: bool,
) {
    let flags = if hovered { FLAG_HOVERED } else { 0 };
    paint_sdf_widget(
        painter,
        rect,
        SdfWidgetGpu {
            size_radius: [rect.width(), rect.height(), 2.0, 1.0],
            col_primary: SdfWidgetGpu::rgba(fill_color),
            col_accent: SdfWidgetGpu::rgba(cap_color),
            params: [norm_value.clamp(0.0, 1.0), 0.78, 0.0, 0.0],
            meta: [KIND_FADER_VERTICAL, flags, 0, 0],
        },
    );
}

/// Pinta un fader horizontal o barra de paneo bipolar cobre-ámbar en GPU.
pub fn paint_fader_horizontal(
    painter: &Painter,
    rect: Rect,
    norm_value: f32,
    fill_color: Color32,
    cap_color: Color32,
    hovered: bool,
    bipolar: bool,
) {
    let mut flags = 0u32;
    if hovered {
        flags |= FLAG_HOVERED;
    }
    if bipolar {
        flags |= FLAG_BIPOLAR;
    }
    paint_sdf_widget(
        painter,
        rect,
        SdfWidgetGpu {
            size_radius: [rect.width(), rect.height(), 2.2, 1.0],
            col_primary: SdfWidgetGpu::rgba(fill_color),
            col_accent: SdfWidgetGpu::rgba(cap_color),
            params: [norm_value.clamp(0.0, 1.0), 0.5, 0.0, 0.0],
            meta: [KIND_FADER_HORIZONTAL, flags, 0, 0],
        },
    );
}

/// Pinta un vúmetro estéreo dual (`[level_l, level_r, hold_l, hold_r]`) en GPU.
pub fn paint_stereo_meter(painter: &Painter, rect: Rect, levels_and_holds: [f32; 4]) {
    paint_sdf_widget(
        painter,
        rect,
        SdfWidgetGpu {
            size_radius: [rect.width(), rect.height(), 1.5, 1.0],
            col_primary: [0.0, 0.0, 0.0, 1.0],
            col_accent: [1.0, 0.48, 0.0, 1.0],
            params: levels_and_holds,
            meta: [KIND_STEREO_METER, 0, 0, 0],
        },
    );
}

/// Pinta el bisel de pantalla LCD hundida en GPU.
pub fn paint_lcd_bezel(painter: &Painter, rect: Rect, radius: f32) {
    paint_sdf_widget(
        painter,
        rect,
        SdfWidgetGpu {
            size_radius: [rect.width(), rect.height(), radius, 1.0],
            col_primary: [0.07, 0.08, 0.09, 1.0],
            col_accent: [0.22, 0.70, 1.0, 1.0],
            params: [0.0; 4],
            meta: [KIND_LCD_BEZEL, FLAG_SUNKEN, 0, 0],
        },
    );
}
