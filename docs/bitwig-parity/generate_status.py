#!/usr/bin/env python3
"""Generate the Hikaru OpenLive vs Bitwig Studio 5.1.7 status treemap.

The figure is a squarified treemap: one tile per area, its area proportional to
the number of tracked features, filled from the left by the share already done.
A six-pixel status bar at the bottom of every tile carries Done (green) and
In progress (orange); the red badge counts the area's open bugs.

Only the Python standard library is used, so it runs unchanged in GitHub
Actions.

Usage:
    python3 docs/bitwig-parity/generate_status.py
    python3 docs/bitwig-parity/generate_status.py [features.json] [status.svg]
    python3 docs/bitwig-parity/generate_status.py --check

When the file is generated, the ``summary`` object in the JSON is refreshed so
the database and the figure can never disagree.
"""

from __future__ import annotations

import json
import math
import os
import sys
from xml.sax.saxutils import escape

# --------------------------------------------------------------------------- #
# Palette and canvas (matched to the dark status figure used by Suhina).
# --------------------------------------------------------------------------- #
WIDTH = 680
HEIGHT = 480
MARGIN = 1.5
CONTENT_TOP = 34.0
CONTENT_BOTTOM = float(HEIGHT)

BG = "#0d1117"
TILE_BG = "#2C2C2A"
TEXT = "#F1EFE8"
MUTED = "#B4B2A9"
DONE = "#1D9E75"
IN_PROGRESS = "#EF9F27"
PLANNED = "#5F5E5A"
BUG = "#E24B4A"
FONT_FAMILY = (
    "-apple-system, BlinkMacSystemFont, 'Segoe UI', Helvetica, Arial, sans-serif"
)

STATUSES = ("done", "in_progress", "planned")


# --------------------------------------------------------------------------- #
# Small helpers.
# --------------------------------------------------------------------------- #
def round1(value: float) -> float:
    return round(value + 0.0, 1)


def text_width(text: str, size: float) -> float:
    """Approximate advance width of a sans-serif string.

    Good enough for wrapping and deciding whether a label fits inside a tile;
    the figure is meant to be read at a glance, not typeset.
    """
    narrow = set("ilIjtfr.,;:'|![]()")
    wide = set("mwMW@%")
    total = 0.0
    for char in text:
        if char == " ":
            total += 0.29
        elif char in narrow:
            total += 0.32
        elif char in wide:
            total += 0.86
        elif char.isupper():
            total += 0.64
        else:
            total += 0.53
    return total * size


def wrap_text(text: str, max_width: float, size: float) -> list[str]:
    words = text.split()
    if not words:
        return [""]
    lines: list[str] = []
    current = ""
    for word in words:
        candidate = word if not current else f"{current} {word}"
        if text_width(candidate, size) <= max_width:
            current = candidate
            continue
        if current:
            lines.append(current)
        # Hard-break a single word that is wider than the tile.
        while text_width(word, size) > max_width and len(word) > 1:
            cut = len(word)
            while cut > 1 and text_width(word[:cut], size) > max_width:
                cut -= 1
            lines.append(word[:cut])
            word = word[cut:]
        current = word
    if current:
        lines.append(current)
    return lines


def fit_lines(text: str, max_width: float, size: float, max_lines: int) -> list[str]:
    lines = wrap_text(text, max_width, size)
    if len(lines) <= max_lines:
        return lines
    lines = lines[:max_lines]
    last = lines[-1]
    while last and text_width(last + "…", size) > max_width:
        last = last[:-1]
    lines[-1] = last + "…"
    return lines


# --------------------------------------------------------------------------- #
# Squarified treemap (Bruls, Huizing & van Wijk).
# --------------------------------------------------------------------------- #
def worst_ratio(row: list[float], side: float) -> float:
    if not row or side <= 0:
        return math.inf
    total = sum(row)
    if total <= 0:
        return math.inf
    ratios = []
    for area in row:
        if area <= 0:
            continue
        ratios.append(max((side * side * area) / (total * total),
                          (total * total) / (side * side * area)))
    return max(ratios) if ratios else math.inf


def place_row(row: list[float], x: float, y: float, w: float, h: float):
    """Lay one row out and return the remaining rectangle."""
    total = sum(row)
    if total <= 0 or w <= 0 or h <= 0:
        return x, y, w, h
    placed = []
    if w >= h:
        thickness = total / h
        cursor = y
        for area in row:
            height = area / thickness
            placed.append((x, cursor, thickness, height))
            cursor += height
        return placed, x + thickness, y, max(0.0, w - thickness), h
    thickness = total / w
    cursor = x
    for area in row:
        width = area / thickness
        placed.append((cursor, y, width, thickness))
        cursor += width
    return placed, x, y + thickness, w, max(0.0, h - thickness)


def squarify(areas: list[tuple[str, float]], x: float, y: float, w: float, h: float):
    """Return {id: (x, y, w, h)} with tile areas proportional to ``area``."""
    if not areas or w <= 0 or h <= 0:
        return {}
    ordered = sorted(areas, key=lambda item: (-item[1], item[0]))
    total = sum(area for _, area in ordered)
    if total <= 0:
        return {}
    scale = (w * h) / total
    scaled = [area * scale for _, area in ordered]
    ids = [key for key, _ in ordered]

    rects: dict[str, tuple[float, float, float, float]] = {}
    row: list[float] = []
    row_ids: list[str] = []
    index = 0
    while index < len(scaled):
        side = min(w, h)
        candidate = scaled[index]
        if not row or worst_ratio(row + [candidate], side) <= worst_ratio(row, side):
            row.append(candidate)
            row_ids.append(ids[index])
            index += 1
            continue
        placed, x, y, w, h = place_row(row, x, y, w, h)
        for key, rect in zip(row_ids, placed):
            rects[key] = rect
        row, row_ids = [], []
    if row:
        placed, x, y, w, h = place_row(row, x, y, w, h)
        for key, rect in zip(row_ids, placed):
            rects[key] = rect
    return rects


# --------------------------------------------------------------------------- #
# Figure rendering.
# --------------------------------------------------------------------------- #
def summary_of(document: dict) -> dict:
    counts = {status: 0 for status in STATUSES}
    bugs = 0
    areas = document.get("areas", [])
    for area in areas:
        bugs += int(area.get("bugs", 0))
        for feature in area.get("features", []):
            status = feature.get("status")
            if status not in counts:
                raise SystemExit(
                    f"invalid status {status!r} in area {area.get('id')!r} "
                    f"(feature {feature.get('id')!r})"
                )
            counts[status] += 1
    total = sum(counts.values())
    done = counts["done"]
    return {
        "areas": len(areas),
        "features": total,
        "done": done,
        "in_progress": counts["in_progress"],
        "planned": counts["planned"],
        "bugs": bugs,
        "parity_percent": round((done * 100.0) / total) if total else 0,
    }


def render(document: dict) -> str:
    summary = summary_of(document)
    total = summary["features"]
    done = summary["done"]
    parity = summary["parity_percent"]
    reference = document.get("reference", {})
    ref_name = reference.get("name", "Bitwig Studio")
    ref_version = reference.get("version", "")
    ref = f"{ref_name} {ref_version}".strip()
    product = document.get("product", "Hikaru OpenLive")

    areas = document.get("areas", [])
    area_totals = []
    for area in areas:
        count = len(area.get("features", []))
        if count:
            area_totals.append((area.get("id", area.get("name", "")), float(count)))

    rects = squarify(
        area_totals,
        0.0,
        CONTENT_TOP,
        float(WIDTH),
        CONTENT_BOTTOM - CONTENT_TOP,
    )

    out: list[str] = []
    add = out.append
    aria = f"{product} vs {ref}: {done} of {total} features done"
    add(
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{HEIGHT}" '
        f'viewBox="0 0 {WIDTH} {HEIGHT}" font-family="{FONT_FAMILY}" role="img" '
        f'aria-label="{escape(aria)}">'
    )
    add(f"<title>{escape(aria)}</title>")
    add(
        f"<desc>Feature parity of {escape(product)} against {escape(ref)}. "
        f"{done} of {total} tracked features are done; {summary['bugs']} open bug"
        f"{'' if summary['bugs'] == 1 else 's'}.</desc>"
    )
    add(f'<rect width="{WIDTH}" height="{HEIGHT}" fill="{BG}"/>')

    # Global percentage, top right.
    add(
        f'<text x="{WIDTH - 2:.1f}" y="20.0" font-size="12" font-weight="600" '
        f'text-anchor="end" fill="{TEXT}">{parity}% · {done} of {total} done</text>'
    )

    # Legend, top left. Fixed x positions match the Suhina figure.
    legend = [
        ("Done", DONE, 2.0),
        ("In progress", IN_PROGRESS, 64.0),
        ("Planned", PLANNED, 159.8),
        ("Open bugs", BUG, 238.4),
    ]
    for label, color, x in legend:
        add(f'<rect x="{x:.1f}" y="11" width="10" height="10" rx="2" fill="{color}"/>')
        add(
            f'<text x="{x + 15:.1f}" y="20.0" font-size="12" fill="{MUTED}">'
            f"{escape(label)}</text>"
        )

    # Tiles. Render in document order for stable output.
    for index, area in enumerate(areas):
        key = area.get("id", area.get("name", ""))
        rect = rects.get(key)
        if not rect:
            continue
        rx, ry, rw, rh = rect
        x = rx + MARGIN
        y = ry + MARGIN
        w = rw - 2 * MARGIN
        h = rh - 2 * MARGIN
        if w <= 1 or h <= 1:
            continue

        features = area.get("features", [])
        area_total = len(features)
        if not area_total:
            continue
        count = {status: 0 for status in STATUSES}
        for feature in features:
            count[feature.get("status")] += 1
        area_done = count["done"]
        area_ip = count["in_progress"]
        area_planned = count["planned"]
        percent = round((area_done * 100.0) / area_total)
        bugs = int(area.get("bugs", 0))
        name = area.get("name", key)

        clip_id = f"b{index}"
        right = x + w
        bottom = y + h
        add(
            f"<g><title>{escape(name)}: {area_done} done, {area_ip} in progress, "
            f"{area_planned} planned"
            + (f", {bugs} open bugs" if bugs else "")
            + "</title>"
        )
        add(
            f'<clipPath id="{clip_id}"><rect x="{round1(x)}" y="{round1(y)}" '
            f'width="{round1(w)}" height="{round1(h)}" rx="6"/></clipPath>'
        )
        add(f'<g clip-path="url(#{clip_id})">')
        add(
            f'<rect x="{round1(x)}" y="{round1(y)}" width="{round1(w)}" '
            f'height="{round1(h)}" fill="{TILE_BG}"/>'
        )
        done_w = w * area_done / area_total
        if done_w > 0:
            add(
                f'<rect x="{round1(x)}" y="{round1(y)}" width="{round1(done_w)}" '
                f'height="{round1(h)}" fill="{DONE}" fill-opacity="0.25"/>'
            )
        bar_y = bottom - 6.0
        if done_w > 0:
            add(
                f'<rect x="{round1(x)}" y="{round1(bar_y)}" width="{round1(done_w)}" '
                f'height="6" fill="{DONE}"/>'
            )
        ip_w = w * area_ip / area_total
        if ip_w > 0:
            add(
                f'<rect x="{round1(x + done_w)}" y="{round1(bar_y)}" '
                f'width="{round1(ip_w)}" height="6" fill="{IN_PROGRESS}"/>'
            )
        add("</g>")

        # Title: 13 px normally, 12 px in narrow tiles, wrapped to the width.
        title_size = 13 if w >= 110 else 12
        line_height = title_size + 2
        title_x = x + 8.0
        title_y = y + 21.0
        max_lines = max(1, int((h - 14.0) / line_height))

        # First lay the title out at full width. If the tile cannot fit the big
        # percentage, a small percentage is placed at the top right and the
        # title is re-wrapped to keep clear of it.
        full_width = max(10.0, w - 16.0)
        title_lines = fit_lines(name, full_width, title_size, max_lines)
        title_bottom = title_y + (len(title_lines) - 1) * line_height
        show_big = w >= 92 and h >= 96 and title_bottom + 47.0 <= bar_y - 6.0
        show_small = not show_big and w >= 110 and h >= 40
        if show_small:
            inner_width = max(10.0, w - 16.0 - 42.0) * 0.92
            title_lines = fit_lines(name, inner_width, title_size, max_lines)
            title_bottom = title_y + (len(title_lines) - 1) * line_height

        for line_index, line in enumerate(title_lines):
            add(
                f'<text x="{round1(title_x)}" y="{round1(title_y + line_index * line_height)}" '
                f'font-size="{title_size}" font-weight="600" fill="{TEXT}">'
                f"{escape(line)}</text>"
            )

        if show_big:
            add(
                f'<text x="{round1(title_x)}" y="{round1(title_bottom + 33.3)}" '
                f'font-size="28" font-weight="600" fill="{TEXT}">{percent}%</text>'
            )
            add(
                f'<text x="{round1(title_x)}" y="{round1(title_bottom + 48.3)}" '
                f'font-size="11" fill="{MUTED}">{area_done} of {area_total} done</text>'
            )
        elif show_small:
            add(
                f'<text x="{round1(right - 8)}" y="{round1(y + 21.0)}" '
                f'font-size="13" font-weight="600" text-anchor="end" fill="{TEXT}">'
                f"{percent}%</text>"
            )

        # Open-bug badge, bottom right.
        if bugs > 0 and w >= 36 and h >= 32:
            badge_w = 22.0 if bugs < 10 else (24.0 if bugs < 100 else 30.0)
            badge_h = 16.0
            badge_x = right - 8.0 - badge_w
            badge_y = bottom - 26.0
            add(
                f'<rect x="{round1(badge_x)}" y="{round1(badge_y)}" '
                f'width="{badge_w}" height="{badge_h}" rx="4" fill="{BUG}"/>'
            )
            add(
                f'<text x="{round1(badge_x + badge_w / 2)}" '
                f'y="{round1(badge_y + 12)}" font-size="11" font-weight="600" '
                f'text-anchor="middle" fill="#ffffff">{bugs}</text>'
            )
        add("</g>")

    add("</svg>")
    return "\n".join(out) + "\n"


def main(argv: list[str]) -> int:
    # Defaults live next to this script, so the parity directory is portable:
    # it can be executed from the repo root, from CI, or from anywhere else.
    base = os.path.dirname(os.path.abspath(__file__))
    args = [arg for arg in argv[1:] if not arg.startswith("--")]
    check = "--check" in argv
    features_path = args[0] if args else os.path.join(base, "features_bitwig.json")
    svg_path = args[1] if len(args) > 1 else os.path.join(base, "status.svg")

    with open(features_path, "r", encoding="utf-8") as handle:
        document = json.load(handle)

    summary = summary_of(document)
    if document.get("summary") != summary and not check:
        document["summary"] = summary
        with open(features_path, "w", encoding="utf-8") as handle:
            json.dump(document, handle, ensure_ascii=False, indent=2)
            handle.write("\n")

    svg = render(document)
    if check:
        problems = []
        if document.get("summary") != summary:
            problems.append("summary in features JSON is stale")
        if os.path.exists(svg_path):
            with open(svg_path, "r", encoding="utf-8") as handle:
                if handle.read() != svg:
                    problems.append("status.svg is stale")
        else:
            problems.append("status.svg is missing")
        if problems:
            for problem in problems:
                print(f"error: {problem}", file=sys.stderr)
            print("run: python3 docs/bitwig-parity/generate_status.py", file=sys.stderr)
            return 1
        print("status figure is up to date")
        return 0

    with open(svg_path, "w", encoding="utf-8") as handle:
        handle.write(svg)

    print(
        f"wrote {svg_path}: {summary['done']} of {summary['features']} features done "
        f"({summary['parity_percent']}%), {summary['bugs']} open bug(s)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
