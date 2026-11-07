/*
 * Hikaru OpenStudio - Audio DAW
 * Copyright (C) 2026 Hikaru OpenStudio Developers
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 *
 * Icon outlines in the `tabler` section originate from Tabler Icons 3.31.0
 * (MIT License, https://tabler.io/icons; see `assets/icons/TABLER-LICENSE.txt`)
 * via Lemon-Studio (`crates/os-ui/src/tabler.rs`).
 */

//! Sistema vectorial de iconografía DAW sin dependencia de glifos Unicode.
//!
//! Combina:
//! 1. Contornos vectoriales **Tabler Icons 24×24** (MIT, traídos de Lemon-Studio)
//!    teselados con curvas Bézier cuadráticas/cúbicas (`M/L/Q/C/Z`).
//! 2. Iconos geométricos pixel-snapped específicos de **Bitwig Studio 5.1.7**
//!    (emblema central de 10 puntos, grilla Clip Launcher 3×2, carriles Arranger,
//!    automatización con nodos, corona Master, chip DSP, badge de navegador, etc.).

use egui::{pos2, vec2, Color32, Painter, Pos2, Rect, Shape, Stroke};

/// Definición de un icono vectorial sobre grilla `24×24` (`M/L/Q/C/Z` absolutos).
#[derive(Clone, Copy, Debug)]
pub struct VectorIcon {
    pub filled: bool,
    pub d: &'static str,
}

// =========================================================================
// CATÁLOGO TABLER ICONS (24x24, MIT License — adaptado de Lemon-Studio)
// =========================================================================

pub const MINUS: VectorIcon = VectorIcon { filled: false, d: "M 5 12 L 19 12" };
pub const PLUS: VectorIcon = VectorIcon { filled: false, d: "M 12 5 L 12 19 M 5 12 L 19 12" };
pub const X_CLOSE: VectorIcon = VectorIcon { filled: false, d: "M 18 6 L 6 18 M 6 6 L 18 18" };

pub const PLAYER_PLAY: VectorIcon = VectorIcon {
    filled: true,
    d: "M 6 4 L 6 20 L 20.5 12 Z",
};
pub const PLAYER_STOP: VectorIcon = VectorIcon {
    filled: true,
    d: "M 5 5 L 19 5 L 19 19 L 5 19 Z",
};

pub const METRONOME: VectorIcon = VectorIcon {
    filled: false,
    d: "M 14.153 8.188 L 13.433 4.952 C 13.179 3.812 12.168 3 10.999 3 C 9.831 3 8.82 3.812 8.566 4.952 L 5.541 18.566 C 5.409 19.158 5.554 19.778 5.933 20.251 C 6.313 20.724 6.886 21 7.493 21 L 14.507 21 C 15.114 21 15.687 20.724 16.067 20.251 C 16.446 19.778 16.591 19.158 16.459 18.566 L 15.935 16.209 M 11 18 L 20 5",
};

pub const REPEAT: VectorIcon = VectorIcon {
    filled: false,
    d: "M 4 12 L 4 9 C 4 7.343 5.343 6 7 6 L 20 6 M 17 3 L 20 6 L 17 9 M 20 12 L 20 15 C 20 16.657 18.657 18 17 18 L 4 18 M 7 21 L 4 18 L 7 15",
};

pub const UNDO: VectorIcon = VectorIcon {
    filled: false,
    d: "M 9 14 L 5 10 L 9 6 M 5 10 L 16 10 C 18.209 10 20 11.791 20 14 C 20 16.209 18.209 18 16 18 L 13 18",
};

pub const REDO: VectorIcon = VectorIcon {
    filled: false,
    d: "M 15 14 L 19 10 L 15 6 M 19 10 L 8 10 C 5.791 10 4 11.791 4 14 C 4 16.209 5.791 18 8 18 L 11 18",
};

pub const CUT: VectorIcon = VectorIcon {
    filled: false,
    d: "M 4 17 C 4 18.657 5.343 20 7 20 C 8.657 20 10 18.657 10 17 C 10 15.343 8.657 14 7 14 C 5.343 14 4 15.343 4 17 M 14 17 C 14 18.657 15.343 20 17 20 C 18.657 20 20 18.657 20 17 C 20 15.343 18.657 14 17 14 C 15.343 14 14 15.343 14 17 M 9.15 14.85 L 18 4 M 6 4 L 14.85 14.85",
};

pub const DEVICE_FLOPPY: VectorIcon = VectorIcon {
    filled: false,
    d: "M 6 4 L 16 4 L 20 8 L 20 18 C 20 19.105 19.105 20 18 20 L 6 20 C 4.895 20 4 19.105 4 18 L 4 6 C 4 4.895 4.895 4 6 4 M 10 14 C 10 15.105 10.895 16 12 16 C 13.105 16 14 15.105 14 14 C 14 12.895 13.105 12 12 12 C 10.895 12 10 12.895 10 14 M 14 4 L 14 8 L 8 8 L 8 4",
};

pub const FILE_EXPORT: VectorIcon = VectorIcon {
    filled: false,
    d: "M 14 3 L 14 7 C 14 7.552 14.448 8 15 8 L 19 8 M 11.5 21 L 7 21 C 5.895 21 5 20.105 5 19 L 5 5 C 5 3.895 5.895 3 7 3 L 14 3 L 19 8 L 19 13 M 14 19 L 21 19 M 18 16 L 21 19 L 18 22",
};

pub const INFO_CIRCLE: VectorIcon = VectorIcon {
    filled: false,
    d: "M 3 12 C 3 16.971 7.029 21 12 21 C 16.971 21 21 16.971 21 12 C 21 7.029 16.971 3 12 3 C 7.029 3 3 7.029 3 12 M 12 8 L 12 9.2 M 11.2 12 L 12 12 L 12 16.5 L 12.8 16.5",
};

pub const KEYBOARD: VectorIcon = VectorIcon {
    filled: false,
    d: "M 2 8 C 2 6.895 2.895 6 4 6 L 20 6 C 21.105 6 22 6.895 22 8 L 22 16 C 22 17.105 21.105 18 20 18 L 4 18 C 2.895 18 2 17.105 2 16 Z M 6 10 L 6 11 M 10 10 L 10 11 M 14 10 L 14 11 M 18 10 L 18 11 M 8 14.5 L 16 14.5",
};

pub const PIANO: VectorIcon = VectorIcon {
    filled: false,
    d: "M 3 7 C 3 5.895 3.895 5 5 5 L 19 5 C 20.105 5 21 5.895 21 7 L 21 17 C 21 18.105 20.105 19 19 19 L 5 19 C 3.895 19 3 18.105 3 17 Z M 9 19 L 9 13 M 8 5 L 8 13 L 10 13 L 10 5 M 15 19 L 15 13 M 14 5 L 14 13 L 16 13 L 16 5",
};

pub const LINK: VectorIcon = VectorIcon {
    filled: false,
    d: "M 9 15 L 15 9 M 11 6 L 11.463 5.464 C 13.416 3.511 16.582 3.512 18.535 5.465 C 20.487 7.417 20.487 10.583 18.534 12.536 L 18 13 M 13 18 L 12.603 18.534 C 10.629 20.487 7.45 20.487 5.476 18.534 C 4.531 17.6 4 16.327 4 14.998 C 4 13.67 4.531 12.397 5.476 11.463 L 6 11",
};

pub const MAGNET: VectorIcon = VectorIcon {
    filled: false,
    d: "M 4 13 L 4 5 C 4 3.895 4.895 3 6 3 L 7 3 C 8.105 3 9 3.895 9 5 L 9 13 C 9 14.657 10.343 16 12 16 C 13.657 16 15 14.657 15 13 L 15 5 C 15 3.895 15.895 3 17 3 L 18 3 C 19.105 3 20 3.895 20 5 L 20 13 C 20 17.418 16.418 21 12 21 C 7.582 21 4 17.418 4 13 M 4 8 L 9 8 M 15 8 L 19 8",
};

pub const FOLDER: VectorIcon = VectorIcon {
    filled: false,
    d: "M 5 4 L 9 4 L 12 7 L 19 7 C 20.105 7 21 7.895 21 9 L 21 17 C 21 18.105 20.105 19 19 19 L 5 19 C 3.895 19 3 18.105 3 17 L 3 6 C 3 4.895 3.895 4 5 4 Z",
};

pub const FOLDER_OPEN: VectorIcon = VectorIcon {
    filled: false,
    d: "M 5 19 L 7.757 11.649 C 7.903 11.259 8.276 11 8.693 11 L 21 11 L 20.2 17.5 C 20.0 18.4 19.3 19 18.4 19 L 5 19 C 3.895 19 3 18.105 3 17 L 3 6 C 3 4.895 3.895 4 5 4 L 9 4 L 12 7 L 19 7 C 20.105 7 21 7.895 21 9 L 21 11",
};

pub const FOLDERS: VectorIcon = VectorIcon {
    filled: false,
    d: "M 9 3 L 12 3 L 14 5 L 19 5 C 20.105 5 21 5.895 21 7 L 21 14 C 21 15.105 20.105 16 19 16 L 9 16 C 7.895 16 7 15.105 7 14 L 7 5 C 7 3.895 7.895 3 9 3 M 17 16 L 17 18 C 17 19.105 16.105 20 15 20 L 5 20 C 3.895 20 3 19.105 3 18 L 3 9 C 3 7.895 3.895 7 5 7 L 7 7",
};

pub const FILE_PAGE: VectorIcon = VectorIcon {
    filled: false,
    d: "M 14 3 L 14 7 C 14 7.552 14.448 8 15 8 L 19 8 M 17 21 L 7 21 C 5.895 21 5 20.105 5 19 L 5 5 C 5 3.895 5.895 3 7 3 L 14 3 L 19 8 L 19 19 C 19 20.105 18.105 21 17 21 Z",
};

pub const PLUG: VectorIcon = VectorIcon {
    filled: false,
    d: "M 9.785 6 L 18 14.215 L 15.946 16.269 C 14.488 17.785 12.326 18.395 10.291 17.864 C 8.256 17.333 6.667 15.744 6.136 13.709 C 5.605 11.674 6.215 9.512 7.731 8.054 L 9.785 6 Z M 4 20 L 7.5 16.5 M 15 4 L 11.5 7.5 M 20 9 L 16.5 12.5",
};

pub const WAVE_SINE: VectorIcon = VectorIcon {
    filled: false,
    d: "M 21 12 L 19 12 C 18.106 12 17.338 11.143 17.239 10 C 16.943 6.55 16.49 4 14.49 4 C 12.49 4 11.99 7.582 11.99 12 C 11.99 16.418 11.49 20 9.49 20 C 7.49 20 7.038 17.453 6.741 14 C 6.641 12.853 5.874 12 4.978 12 L 2.978 12",
};

pub const SEARCH: VectorIcon = VectorIcon {
    filled: false,
    d: "M 3 10 C 3 13.866 6.134 17 10 17 C 13.866 17 17 13.866 17 10 C 17 6.134 13.866 3 10 3 C 6.134 3 3 6.134 3 10 M 21 21 L 15 15",
};

pub const VOLUME: VectorIcon = VectorIcon {
    filled: false,
    d: "M 15 8 C 16.259 8.944 17 10.426 17 12 C 17 13.574 16.259 15.056 15 16 M 17.7 5 C 19.814 6.709 21.043 9.282 21.043 12 C 21.043 14.718 19.814 17.291 17.7 19 M 6 15 L 4 15 C 3.448 15 3 14.552 3 14 L 3 10 C 3 9.448 3.448 9 4 9 L 6 9 L 10.5 4.2 L 11 5 L 11 19 L 10.5 19.8 L 6 15",
};

pub const MUSIC: VectorIcon = VectorIcon {
    filled: false,
    d: "M 3 17 C 3 18.657 4.343 20 6 20 C 7.657 20 9 18.657 9 17 C 9 15.343 7.657 14 6 14 C 4.343 14 3 15.343 3 17 M 13 17 C 13 18.657 14.343 20 16 20 C 17.657 20 19 18.657 19 17 C 19 15.343 17.657 14 16 14 C 14.343 14 13 15.343 13 17 M 9 17 L 9 4 L 19 4 L 19 17 M 9 8 L 19 8",
};

pub const USER: VectorIcon = VectorIcon {
    filled: false,
    d: "M 8 7 C 8 9.209 9.791 11 12 11 C 14.209 11 16 9.209 16 7 C 16 4.791 14.209 3 12 3 C 9.791 3 8 4.791 8 7 M 6 21 L 6 19 C 6 16.791 7.791 15 10 15 L 14 15 C 16.209 15 18 16.791 18 19 L 18 21",
};

pub const SETTINGS: VectorIcon = VectorIcon {
    filled: false,
    d: "M 10.325 4.317 C 10.751 2.561 13.249 2.561 13.675 4.317 C 13.805 4.852 14.182 5.293 14.691 5.504 C 15.199 5.714 15.778 5.67 16.248 5.383 C 17.791 4.443 19.558 6.209 18.618 7.753 C 18.332 8.223 18.287 8.801 18.498 9.309 C 18.708 9.818 19.148 10.195 19.683 10.325 C 21.439 10.751 21.439 13.249 19.683 13.675 C 19.148 13.805 18.707 14.182 18.496 14.691 C 18.286 15.199 18.33 15.778 18.617 16.248 C 19.557 17.791 17.791 19.558 16.247 18.618 C 15.777 18.332 15.199 18.287 14.691 18.498 C 14.182 18.708 13.805 19.148 13.675 19.683 C 13.249 21.439 10.751 21.439 10.325 19.683 C 10.195 19.148 9.818 18.707 9.309 18.496 C 8.801 18.286 8.222 18.33 7.752 18.617 C 6.209 19.557 4.442 17.791 5.382 16.247 C 5.668 15.777 5.713 15.199 5.502 14.691 C 5.292 14.182 4.852 13.805 4.317 13.675 C 2.561 13.249 2.561 10.751 4.317 10.325 C 4.852 10.195 5.293 9.818 5.504 9.309 C 5.714 8.801 5.67 8.222 5.383 7.752 C 4.443 6.209 6.209 4.442 7.753 5.382 C 8.753 5.99 10.049 5.452 10.325 4.317 Z M 9 12 C 9 13.657 10.343 15 12 15 C 13.657 15 15 13.657 15 12 C 15 10.343 13.657 9 12 9 C 10.343 9 9 10.343 9 12",
};

pub const STAR: VectorIcon = VectorIcon {
    filled: false,
    d: "M 12 17.75 L 5.828 20.995 L 7.007 14.122 L 2.007 9.255 L 8.907 8.255 L 11.993 2.002 L 15.079 8.255 L 21.979 9.255 L 16.979 14.122 L 18.158 20.995 Z",
};

pub const ARROW_LEFT: VectorIcon = VectorIcon {
    filled: false,
    d: "M 5 12 L 19 12 M 5 12 L 11 18 M 5 12 L 11 6",
};

pub const ARROW_RIGHT: VectorIcon = VectorIcon {
    filled: false,
    d: "M 19 12 L 5 12 M 19 12 L 13 18 M 19 12 L 13 6",
};

pub const ARROW_UP: VectorIcon = VectorIcon {
    filled: false,
    d: "M 12 5 L 12 19 M 12 5 L 6 11 M 12 5 L 18 11",
};

pub const HOME: VectorIcon = VectorIcon {
    filled: false,
    d: "M 5 12 L 3 12 L 12 3 L 21 12 L 19 12 M 5 12 L 5 19 C 5 20.1 5.9 21 7 21 L 17 21 C 18.1 21 19 20.1 19 19 L 19 12 M 10 21 L 10 15 L 14 15 L 14 21",
};

pub const REFRESH: VectorIcon = VectorIcon {
    filled: false,
    d: "M 20 11 C 19.5 6.8 15.9 3.8 11.6 4 C 7.8 4.2 4.6 7.1 4 11 M 4 5 L 4 11 L 10 11 M 4 13 C 4.5 17.2 8.1 20.2 12.4 20 C 16.2 19.8 19.4 16.9 20 13 M 20 19 L 20 13 L 14 13",
};

pub const EYE: VectorIcon = VectorIcon {
    filled: false,
    d: "M 10 12 C 10 13.105 10.895 14 12 14 C 13.105 14 14 13.105 14 12 C 14 10.895 13.105 10 12 10 C 10.895 10 10 10.895 10 12 M 21 12 C 18.6 16 15.6 18 12 18 C 8.4 18 5.4 16 3 12 C 5.4 8 8.4 6 12 6 C 15.6 6 18.6 8 21 12",
};

pub const COMPUTER: VectorIcon = VectorIcon {
    filled: false,
    d: "M 3 5 C 3 4.448 3.448 4 4 4 L 20 4 C 20.552 4 21 4.448 21 5 L 21 15 C 21 15.552 20.552 16 20 16 L 4 16 C 3.448 16 3 15.552 3 15 Z M 7 20 L 17 20 M 9 16 L 9 20 M 15 16 L 15 20",
};

pub const DUPLICATE_PLUS: VectorIcon = VectorIcon {
    filled: false,
    d: "M 8 8 L 19 8 L 19 19 L 8 19 Z M 16 8 L 16 5 L 5 5 L 5 16 L 8 16 M 13.5 11 L 13.5 16 M 11 13.5 L 16 13.5",
};

pub const CIRCLE_X: VectorIcon = VectorIcon {
    filled: false,
    d: "M 3 12 C 3 16.971 7.029 21 12 21 C 16.971 21 21 16.971 21 12 C 21 7.029 16.971 3 12 3 C 7.029 3 3 7.029 3 12 M 9.5 9.5 L 14.5 14.5 M 14.5 9.5 L 9.5 14.5",
};

// =========================================================================
// PARSER & TESELADOR BÉZIER (`M / L / Q / C / Z`) SOBRE GRILLA 24x24
// =========================================================================

/// Parsea comandos absolutos `M`, `L`, `Q`, `C`, `Z` separados por espacios y
/// devuelve `(puntos_en_grilla_0_24, cerrado)` por cada sub-trazo.
pub fn parse_svg_subpaths(d: &str) -> Vec<(Vec<[f32; 2]>, bool)> {
    let mut subpaths: Vec<(Vec<[f32; 2]>, bool)> = Vec::new();
    let mut current: Vec<[f32; 2]> = Vec::new();
    let mut it = d.split_ascii_whitespace();

    let next_pt = |it: &mut std::str::SplitAsciiWhitespace<'_>| -> Option<[f32; 2]> {
        let x: f32 = it.next()?.parse().ok()?;
        let y: f32 = it.next()?.parse().ok()?;
        Some([x, y])
    };

    while let Some(cmd) = it.next() {
        match cmd {
            "M" => {
                if current.len() >= 2 {
                    subpaths.push((std::mem::take(&mut current), false));
                } else {
                    current.clear();
                }
                if let Some(p) = next_pt(&mut it) {
                    current.push(p);
                } else {
                    break;
                }
            }
            "L" => {
                if let Some(p) = next_pt(&mut it) {
                    current.push(p);
                } else {
                    break;
                }
            }
            "Q" => {
                let (Some(c), Some(e)) = (next_pt(&mut it), next_pt(&mut it)) else {
                    break;
                };
                let s = current.last().copied().unwrap_or(c);
                const STEPS: usize = 6;
                for step in 1..=STEPS {
                    let t = step as f32 / STEPS as f32;
                    let mt = 1.0 - t;
                    let x = mt * mt * s[0] + 2.0 * mt * t * c[0] + t * t * e[0];
                    let y = mt * mt * s[1] + 2.0 * mt * t * c[1] + t * t * e[1];
                    current.push([x, y]);
                }
            }
            "C" => {
                let (Some(c1), Some(c2), Some(e)) =
                    (next_pt(&mut it), next_pt(&mut it), next_pt(&mut it))
                else {
                    break;
                };
                let s = current.last().copied().unwrap_or(c1);
                const STEPS: usize = 8;
                for step in 1..=STEPS {
                    let t = step as f32 / STEPS as f32;
                    let mt = 1.0 - t;
                    let b0 = mt * mt * mt;
                    let b1 = 3.0 * mt * mt * t;
                    let b2 = 3.0 * mt * t * t;
                    let b3 = t * t * t;
                    current.push([
                        b0 * s[0] + b1 * c1[0] + b2 * c2[0] + b3 * e[0],
                        b0 * s[1] + b1 * c1[1] + b2 * c2[1] + b3 * e[1],
                    ]);
                }
            }
            "Z" => {
                if current.len() >= 2 {
                    subpaths.push((std::mem::take(&mut current), true));
                } else {
                    current.clear();
                }
            }
            _ => break,
        }
    }

    if current.len() >= 2 {
        subpaths.push((current, false));
    }

    subpaths
}

/// Dibuja un [`VectorIcon`] centrado dentro de `rect`.
pub fn paint_icon(painter: &Painter, rect: Rect, icon: VectorIcon, color: Color32, stroke_width: f32) {
    let side = rect.width().min(rect.height()).max(1.0);
    let ox = rect.center().x - side * 0.5;
    let oy = rect.center().y - side * 0.5;
    let k = side / 24.0;
    let map = |p: [f32; 2]| -> Pos2 { pos2(ox + p[0] * k, oy + p[1] * k) };

    for (pts, closed) in parse_svg_subpaths(icon.d) {
        let mapped: Vec<Pos2> = pts.into_iter().map(map).collect();
        if icon.filled && mapped.len() >= 3 {
            painter.add(Shape::convex_polygon(mapped, color, Stroke::NONE));
        } else if closed && mapped.len() >= 3 {
            painter.add(Shape::closed_line(mapped, Stroke::new(stroke_width, color)));
        } else if mapped.len() >= 2 {
            painter.add(Shape::line(mapped, Stroke::new(stroke_width, color)));
        }
    }
}

// =========================================================================
// ICONOS NATIVOS DE BITWIG STUDIO 5.1.7 (PIXEL-SNAPPED)
// =========================================================================

/// Emblema central superior estilo Bitwig Studio (fila superior de 4 puntos,
/// fila inferior de 6 puntos). Abre el Dashboard/Preferencias al hacer clic.
pub fn paint_bitwig_emblem(painter: &Painter, rect: Rect, color: Color32) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    let dx = (rect.width() / 7.2).clamp(2.8, 4.2);
    let dy = (rect.height() * 0.22).clamp(2.2, 3.4);
    let r = (dx * 0.31).clamp(1.0, 1.55);

    // Fila superior: 4 puntos centrados (-1.5, -0.5, +0.5, +1.5)
    for i in 0..4 {
        let offset = (i as f32 - 1.5) * dx;
        painter.circle_filled(pos2(cx + offset, cy - dy), r, color);
    }
    // Fila inferior: 6 puntos (-2.5 .. +2.5)
    for i in 0..6 {
        let offset = (i as f32 - 2.5) * dx;
        painter.circle_filled(pos2(cx + offset, cy + dy), r, color);
    }
}

/// Triángulo Play nítido estilo Bitwig.
pub fn paint_play_triangle(painter: &Painter, rect: Rect, color: Color32) {
    let s = rect.width().min(rect.height()) * 0.36;
    let c = rect.center();
    let pts = vec![
        pos2(c.x - s * 0.72, c.y - s * 0.92),
        pos2(c.x + s * 0.98, c.y),
        pos2(c.x - s * 0.72, c.y + s * 0.92),
    ];
    painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
}

/// Cuadrado Stop nítido estilo Bitwig.
pub fn paint_stop_square(painter: &Painter, rect: Rect, color: Color32) {
    let s = (rect.width().min(rect.height()) * 0.50).round().max(4.0);
    let sq = Rect::from_center_size(rect.center(), vec2(s, s));
    painter.rect_filled(sq, 0.8, color);
}

/// Círculo de Record / Arm estilo Bitwig.
pub fn paint_record_circle(painter: &Painter, rect: Rect, color: Color32, filled: bool) {
    let r = (rect.width().min(rect.height()) * 0.30).max(2.5);
    if filled {
        painter.circle_filled(rect.center(), r, color);
    } else {
        painter.circle_stroke(rect.center(), r, Stroke::new(1.4_f32, color));
    }
}

/// Caret pequeño (abajo/derecha/izquierda/arriba) sin depender de glifos Unicode.
pub fn paint_caret_down(painter: &Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let w = (rect.width().min(rect.height()) * 0.26).max(2.2);
    let h = w * 0.68;
    painter.add(Shape::convex_polygon(
        vec![pos2(c.x - w, c.y - h * 0.5), pos2(c.x + w, c.y - h * 0.5), pos2(c.x, c.y + h * 0.7)],
        color,
        Stroke::NONE,
    ));
}

/// Icono de Clip Launcher (grilla 3×2 de bloques nítidos, sub-barra Arrange y Mix).
pub fn paint_clip_launcher_grid(painter: &Painter, rect: Rect, color: Color32) {
    let w = (rect.width() * 0.58).round().max(9.0);
    let h = (rect.height() * 0.44).round().max(7.0);
    let area = Rect::from_center_size(rect.center(), vec2(w, h));
    let gap = 1.5;
    let cw = (area.width() - gap * 2.0) / 3.0;
    let ch = (area.height() - gap) / 2.0;
    for row in 0..2 {
        for col in 0..3 {
            let x = area.min.x + col as f32 * (cw + gap);
            let y = area.min.y + row as f32 * (ch + gap);
            painter.rect_filled(Rect::from_min_size(pos2(x, y), vec2(cw, ch)), 0.5, color);
        }
    }
}

/// Icono de Arranger Timeline (dos barras horizontales apiladas estilo Bitwig).
pub fn paint_arranger_lanes(painter: &Painter, rect: Rect, color: Color32) {
    let w = (rect.width() * 0.56).round().max(9.0);
    let h = (rect.height() * 0.40).round().max(6.0);
    let area = Rect::from_center_size(rect.center(), vec2(w, h));
    let bar_h = ((area.height() - 2.0) * 0.5).max(2.0);
    painter.rect_filled(
        Rect::from_min_size(area.min, vec2(area.width(), bar_h)),
        0.5,
        color,
    );
    painter.rect_filled(
        Rect::from_min_size(pos2(area.min.x, area.max.y - bar_h), vec2(area.width(), bar_h)),
        0.5,
        color,
    );
}

/// Icono de Note Editor del footer de Bitwig (3 notas MIDI escalonadas).
pub fn paint_note_editor(painter: &Painter, rect: Rect, color: Color32) {
    let side = (rect.width().min(rect.height()) * 0.58).max(8.0);
    let area = Rect::from_center_size(rect.center(), vec2(side, side * 0.85));
    let bh = 2.0;
    painter.rect_filled(
        Rect::from_min_size(pos2(area.min.x, area.min.y), vec2(side * 0.45, bh)),
        0.5,
        color,
    );
    painter.rect_filled(
        Rect::from_min_size(
            pos2(area.min.x + side * 0.35, area.center().y - bh * 0.5),
            vec2(side * 0.55, bh),
        ),
        0.5,
        color,
    );
    painter.rect_filled(
        Rect::from_min_size(pos2(area.min.x + side * 0.12, area.max.y - bh), vec2(side * 0.48, bh)),
        0.5,
        color,
    );
}

/// Icono de Automatización estilo Bitwig (recta diagonal con dos nodos circulares).
pub fn paint_automation_nodes(painter: &Painter, rect: Rect, color: Color32, with_w_badge: bool) {
    let s = (rect.width().min(rect.height()) * 0.30).max(3.5);
    let c = rect.center();
    let p0 = pos2(c.x - s, c.y + s * 0.75);
    let p1 = pos2(c.x + s * 0.85, c.y - s * 0.85);
    painter.line_segment([p0, p1], Stroke::new(1.4_f32, color));
    painter.circle_stroke(p0, 2.0, Stroke::new(1.3_f32, color));
    painter.circle_stroke(p1, 2.0, Stroke::new(1.3_f32, color));
    if with_w_badge {
        painter.text(
            pos2(c.x + s * 0.65, c.y + s * 0.75),
            egui::Align2::CENTER_CENTER,
            "W",
            crate::ui::fonts::semibold(7.5),
            color,
        );
    }
}

/// Icono del Device Panel del footer de Bitwig (cuadrado redondeado: sólido cuando está abierto).
pub fn paint_device_box(painter: &Painter, rect: Rect, color: Color32, active: bool) {
    let s = (rect.width().min(rect.height()) * 0.54).round().max(8.0);
    let box_rect = Rect::from_center_size(rect.center(), vec2(s, s));
    if active {
        painter.rect_filled(box_rect, 2.0, color);
    } else {
        painter.rect_stroke(box_rect, 2.0, Stroke::new(1.4_f32, color), egui::StrokeKind::Inside);
    }
}

/// Icono de Mixer del footer de Bitwig (3 faders verticales con tapas).
pub fn paint_mixer_faders(painter: &Painter, rect: Rect, color: Color32) {
    let w = (rect.width() * 0.56).max(9.0);
    let h = (rect.height() * 0.56).max(9.0);
    let area = Rect::from_center_size(rect.center(), vec2(w, h));
    let xs = [area.min.x + 1.5, area.center().x, area.max.x - 1.5];
    let caps = [0.65_f32, 0.32, 0.52];
    for (x, cap_t) in xs.into_iter().zip(caps) {
        painter.line_segment(
            [pos2(x, area.min.y), pos2(x, area.max.y)],
            Stroke::new(1.2_f32, color),
        );
        let cy = area.min.y + area.height() * cap_t;
        painter.rect_filled(Rect::from_center_size(pos2(x, cy), vec2(3.4, 1.8)), 0.4, color);
    }
}

/// Corona de pista `Master` / pestaña `PROJECT` de Bitwig.
pub fn paint_crown(painter: &Painter, rect: Rect, color: Color32) {
    let w = (rect.width().min(rect.height()) * 0.62).max(7.0);
    let h = w * 0.66;
    let a = Rect::from_center_size(rect.center(), vec2(w, h));
    let pts = vec![
        pos2(a.min.x, a.max.y),
        pos2(a.min.x, a.min.y + h * 0.22),
        pos2(a.min.x + w * 0.28, a.min.y + h * 0.58),
        pos2(a.center().x, a.min.y),
        pos2(a.max.x - w * 0.28, a.min.y + h * 0.58),
        pos2(a.max.x, a.min.y + h * 0.22),
        pos2(a.max.x, a.max.y),
    ];
    painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
}

/// Flecha curva de pista de retorno `FX` estilo Bitwig.
pub fn paint_return_arrow(painter: &Painter, rect: Rect, color: Color32) {
    let s = (rect.width().min(rect.height()) * 0.30).max(3.2);
    let c = rect.center();
    painter.add(Shape::line(
        vec![
            pos2(c.x - s, c.y - s * 0.6),
            pos2(c.x, c.y - s * 0.6),
            pos2(c.x, c.y + s * 0.6),
            pos2(c.x + s, c.y + s * 0.6),
        ],
        Stroke::new(1.3_f32, color),
    ));
    painter.add(Shape::line(
        vec![
            pos2(c.x + s * 0.35, c.y + s * 0.1),
            pos2(c.x + s, c.y + s * 0.6),
            pos2(c.x + s * 0.35, c.y + s * 1.1),
        ],
        Stroke::new(1.3_f32, color),
    ));
}

/// Chip DSP cian dentro del visor LCD de transporte.
pub fn paint_cpu_chip(painter: &Painter, rect: Rect, color: Color32) {
    let s = (rect.width().min(rect.height()) * 0.52).max(6.0);
    let core = Rect::from_center_size(rect.center(), vec2(s, s));
    painter.rect_stroke(core, 1.0, Stroke::new(1.2_f32, color), egui::StrokeKind::Inside);
    for off in [-s * 0.26, s * 0.26] {
        // Pines arriba/abajo/izq/der
        painter.line_segment(
            [pos2(core.center().x + off, core.min.y - 2.0), pos2(core.center().x + off, core.min.y)],
            Stroke::new(1.1_f32, color),
        );
        painter.line_segment(
            [pos2(core.center().x + off, core.max.y), pos2(core.center().x + off, core.max.y + 2.0)],
            Stroke::new(1.1_f32, color),
        );
        painter.line_segment(
            [pos2(core.min.x - 2.0, core.center().y + off), pos2(core.min.x, core.center().y + off)],
            Stroke::new(1.1_f32, color),
        );
        painter.line_segment(
            [pos2(core.max.x, core.center().y + off), pos2(core.max.x + 2.0, core.center().y + off)],
            Stroke::new(1.1_f32, color),
        );
    }
}

/// Flechas bidireccionales MIDI/Audio I/O del visor LCD.
pub fn paint_io_arrows(painter: &Painter, rect: Rect, color: Color32) {
    let w = (rect.width() * 0.58).max(6.0);
    let c = rect.center();
    let y1 = c.y - 2.2;
    let y2 = c.y + 2.2;
    painter.line_segment([pos2(c.x - w * 0.5, y1), pos2(c.x + w * 0.5, y1)], Stroke::new(1.2_f32, color));
    painter.line_segment([pos2(c.x + w * 0.2, y1 - 1.8), pos2(c.x + w * 0.5, y1)], Stroke::new(1.2_f32, color));
    painter.line_segment([pos2(c.x - w * 0.5, y2), pos2(c.x + w * 0.5, y2)], Stroke::new(1.2_f32, color));
    painter.line_segment([pos2(c.x - w * 0.2, y2 + 1.8), pos2(c.x - w * 0.5, y2)], Stroke::new(1.2_f32, color));
}

/// Óvalo de Loop dentro del visor LCD de transporte.
pub fn paint_loop_pill(painter: &Painter, rect: Rect, color: Color32) {
    let pill = Rect::from_center_size(
        rect.center(),
        vec2((rect.width() * 0.64).max(10.0), (rect.height() * 0.46).max(6.5)),
    );
    painter.rect_stroke(
        pill,
        pill.height() * 0.5,
        Stroke::new(1.3_f32, color),
        egui::StrokeKind::Inside,
    );
}

/// Escalón Punch-In / Punch-Out dentro del visor LCD.
pub fn paint_punch_step(painter: &Painter, rect: Rect, color: Color32, punch_in: bool) {
    let w = (rect.width() * 0.62).max(8.0);
    let h = (rect.height() * 0.36).max(3.5);
    let c = rect.center();
    let pts = if punch_in {
        vec![
            pos2(c.x - w * 0.5, c.y + h * 0.5),
            pos2(c.x - w * 0.1, c.y + h * 0.5),
            pos2(c.x + w * 0.1, c.y - h * 0.5),
            pos2(c.x + w * 0.5, c.y - h * 0.5),
        ]
    } else {
        vec![
            pos2(c.x - w * 0.5, c.y - h * 0.5),
            pos2(c.x - w * 0.1, c.y - h * 0.5),
            pos2(c.x + w * 0.1, c.y + h * 0.5),
            pos2(c.x + w * 0.5, c.y + h * 0.5),
        ]
    };
    painter.add(Shape::line(pts, Stroke::new(1.25_f32, color)));
}

/// Puntero selector estilo herramienta `Pointer` de Bitwig.
pub fn paint_pointer_cursor(painter: &Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let pts = vec![
        pos2(c.x - 4.0, c.y - 5.5),
        pos2(c.x - 4.0, c.y + 4.5),
        pos2(c.x - 1.2, c.y + 2.0),
        pos2(c.x + 2.8, c.y + 2.0),
    ];
    painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
    painter.line_segment(
        [pos2(c.x - 1.4, c.y + 1.5), pos2(c.x + 1.0, c.y + 5.8)],
        Stroke::new(1.5_f32, color),
    );
}

/// Menú compacto de 3 líneas horizontales (`≡`) de cabecera de pista / launcher.
pub fn paint_menu_bars(painter: &Painter, rect: Rect, color: Color32) {
    let w = (rect.width() * 0.52).max(6.0);
    let c = rect.center();
    for dy in [-2.6_f32, 0.0, 2.6] {
        painter.line_segment(
            [pos2(c.x - w * 0.5, c.y + dy), pos2(c.x + w * 0.5, c.y + dy)],
            Stroke::new(1.2_f32, color),
        );
    }
}

/// Botón de encendido circular tipo Power (`⏻`) para ranuras del Device Panel.
pub fn paint_power_switch(painter: &Painter, rect: Rect, fill: Color32, fg: Color32) {
    let r = (rect.width().min(rect.height()) * 0.36).max(4.5);
    let c = rect.center();
    painter.circle_filled(c, r, fill);
    painter.line_segment([pos2(c.x, c.y - r * 0.55), pos2(c.x, c.y + r * 0.15)], Stroke::new(1.4_f32, fg));
}

/// Tipo de ítem en el navegador lateral derecho estilo Bitwig.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserBadgeKind {
    Folder,
    Audio,
    DeviceAudio,
    DeviceNote,
    Project,
    Other,
}

/// Insignia cuadrada coloreada (`14×14`) estilo lista de dispositivos/archivos de Bitwig.
pub fn paint_browser_badge(painter: &Painter, rect: Rect, kind: BrowserBadgeKind) {
    let s = (rect.width().min(rect.height()) * 0.78).round().clamp(11.0, 15.0);
    let b = Rect::from_center_size(rect.center(), vec2(s, s));
    let (bg, fg) = match kind {
        BrowserBadgeKind::Folder => (Color32::from_rgb(232, 114, 34), Color32::from_rgb(20, 14, 10)),
        BrowserBadgeKind::Audio => (Color32::from_rgb(235, 168, 32), Color32::from_rgb(20, 16, 8)),
        BrowserBadgeKind::DeviceAudio => (Color32::from_rgb(240, 108, 36), Color32::from_rgb(22, 12, 8)),
        BrowserBadgeKind::DeviceNote => (Color32::from_rgb(58, 168, 224), Color32::from_rgb(10, 20, 28)),
        BrowserBadgeKind::Project => (Color32::from_rgb(155, 110, 225), Color32::from_rgb(18, 12, 28)),
        BrowserBadgeKind::Other => (Color32::from_rgb(122, 126, 134), Color32::from_rgb(18, 20, 22)),
    };

    painter.rect_filled(b, 2.0, bg);

    let inner = b.shrink(2.2);
    match kind {
        BrowserBadgeKind::Folder => {
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(inner.min.x, inner.min.y + 1.5),
                    pos2(inner.max.x, inner.max.y),
                ),
                1.0,
                fg,
            );
        }
        BrowserBadgeKind::Audio => {
            // Mini forma de onda vertical
            let cx = inner.center().x;
            let cy = inner.center().y;
            for (dx, hh) in [(-3.0_f32, 1.8_f32), (-1.0, 3.8), (1.0, 2.6), (3.0, 1.4)] {
                painter.line_segment([pos2(cx + dx, cy - hh), pos2(cx + dx, cy + hh)], Stroke::new(1.2_f32, fg));
            }
        }
        BrowserBadgeKind::DeviceAudio | BrowserBadgeKind::DeviceNote => {
            // Dos mini-perillas superiores + barra inferior (exacto al badge Bitwig!)
            painter.circle_filled(pos2(inner.min.x + 2.2, inner.min.y + 2.2), 1.2, fg);
            painter.circle_filled(pos2(inner.max.x - 2.2, inner.min.y + 2.2), 1.2, fg);
            painter.line_segment(
                [pos2(inner.min.x + 1.2, inner.max.y - 2.0), pos2(inner.max.x - 1.2, inner.max.y - 2.0)],
                Stroke::new(1.3_f32, fg),
            );
        }
        BrowserBadgeKind::Project => {
            paint_crown(painter, inner, fg);
        }
        BrowserBadgeKind::Other => {
            painter.circle_filled(inner.center(), 1.8, fg);
        }
    }
}
