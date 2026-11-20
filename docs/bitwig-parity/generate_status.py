#!/usr/bin/env python3
"""Generate the Hikaru OpenLive vs Bitwig Studio 5.1.7 status figures.

Two figures are produced from the same database:

* ``status.svg`` — squarified treemap: one tile per area, its area proportional
  to the number of tracked features (with a small legibility floor so tiny
  areas can still show their name and percentage), filled from the left by the
  share already done. A six-pixel status bar at the bottom of every tile
  carries Done (green) and In progress (orange); the red badge counts the
  area's open bugs.
* ``parity-detail.svg`` — a tall poster that names every tracked feature,
  grouped by area and ordered In progress, Done, Planned, so the list itself
  answers "what do I already have and what is missing".

Only the Python standard library is used, so it runs unchanged in GitHub
Actions.

Usage:
    python3 docs/bitwig-parity/generate_status.py
    python3 docs/bitwig-parity/generate_status.py [features.json] [status.svg] [parity-detail.svg] [audit.md]
    python3 docs/bitwig-parity/generate_status.py --watch [--interval=0.25]
    python3 docs/bitwig-parity/generate_status.py --check

When the file is generated, the ``summary`` object in the JSON is refreshed and
the parity table in ``bitwig-5.1.7-parity-audit.md`` is rewritten from the same
counts, so the database, the document and both figures can never disagree.
"""

from __future__ import annotations

import json
import math
import os
import re
import sys
import time
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

# Legibility floor for the treemap. Tile area is proportional to the number of
# tracked features, except that small areas count as at least this many so
# their name and percentage stay readable. It only affects ``status.svg``:
# every percentage, count and the audit table still come from the real data.
LEGIBILITY_FLOOR = 22


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
        area_summary = area_counts(area)
        bugs += area_summary["bugs"]
        for status in STATUSES:
            counts[status] += area_summary[status]
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


def area_counts(area: dict) -> dict:
    """Count one area's features and open bugs."""
    counts = {status: 0 for status in STATUSES}
    for feature in area.get("features", []):
        status = feature.get("status")
        if status not in counts:
            raise SystemExit(
                f"invalid status {status!r} in area {area.get('id')!r} "
                f"(feature {feature.get('id')!r})"
            )
        counts[status] += 1
    counts["total"] = sum(counts[status] for status in STATUSES)
    counts["bugs"] = int(area.get("bugs", 0))
    return counts


def sync_audit_markdown(text: str, document: dict) -> str:
    """Refresh the parity table and global figure in the audit document.

    The audit markdown is part of the same source of truth, so its summary
    table is rewritten from the JSON exactly like the JSON ``summary`` and the
    two SVGs. Lines that do not match the expected shapes are left untouched.
    """
    summary = summary_of(document)
    areas = document.get("areas", [])

    missing_rows = []
    for area in areas:
        name = area.get("name", area.get("id", ""))
        counts = area_counts(area)
        row = (
            f"| {name} | {counts['total']} | {counts['done']} | "
            f"{counts['in_progress']} | {counts['planned']} | {counts['bugs']} |"
        )
        pattern = re.compile(rf"^\| {re.escape(name)} \|.*$", re.MULTILINE)
        text, replaced = pattern.subn(lambda _match, row=row: row, text, count=1)
        if not replaced:
            missing_rows.append(row)

    # A brand new area has no row to overwrite yet: insert it just before the
    # total so the document and the JSON never drift apart.
    if missing_rows:
        text = re.sub(
            r"^(\| \*\*Total\*\* \|.*)$",
            lambda match: "\n".join(missing_rows + [match.group(1)]),
            text,
            count=1,
            flags=re.MULTILINE,
        )

    text = re.sub(
        r"contiene \d+ áreas y \d+ features",
        f"contiene {summary['areas']} áreas y {summary['features']} features",
        text,
        count=1,
    )

    total_row = (
        f"| **Total** | **{summary['features']}** | **{summary['done']}** | "
        f"**{summary['in_progress']}** | **{summary['planned']}** | "
        f"**{summary['bugs']}** |"
    )
    text = re.sub(r"^\| \*\*Total\*\* \|.*$", lambda _match: total_row, text,
                  count=1, flags=re.MULTILINE)

    parity = summary["parity_percent"]
    parity_line = (
        f"Paridad global: **{summary['done']} / {summary['features']} = "
        f"{parity} %**."
    )
    text = re.sub(r"^Paridad global: .*$", lambda _match: parity_line, text,
                  count=1, flags=re.MULTILINE)
    return text


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
            area_totals.append(
                (
                    area.get("id", area.get("name", "")),
                    float(max(count, LEGIBILITY_FLOOR)),
                )
            )

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


# --------------------------------------------------------------------------- #
# Detailed poster: every tracked feature by area, with its status.
# --------------------------------------------------------------------------- #
DETAIL_WIDTH = 1400
DETAIL_MARGIN = 32
DETAIL_TOP = 250.0
CHIP_HEIGHT = 22.0
CHIP_ROW_GAP = 4.0
CHIP_COL_GAP = 14.0
GROUP_GAP = 16.0
CHIP_FONT = 12.5
AREA_HEADER_H = 56.0
AREA_BOTTOM_PAD = 22.0

STATUS_ORDER = {"in_progress": 0, "done": 1, "planned": 2}
STATUS_COLOR = {"done": DONE, "in_progress": IN_PROGRESS, "planned": PLANNED}


def truncate_to_width(text: str, max_width: float, size: float) -> str:
    """Cut ``text`` to fit ``max_width`` and append an ellipsis when needed."""
    if text_width(text, size) <= max_width:
        return text
    ellipsis = "…"
    available = max_width - text_width(ellipsis, size)
    cut = len(text)
    while cut > 0 and text_width(text[:cut], size) > available:
        cut -= 1
    return text[:cut].rstrip() + ellipsis


def detail_columns(area_id: str) -> int:
    """Pick a column count that keeps the longest names of the area readable."""
    if area_id in ("builtin-devices-fx", "modulators"):
        return 6
    if area_id in ("browser-files", "ui-workflow", "clip-launcher", "arranger"):
        return 4
    return 5


def ordered_features(features: list[dict]) -> list[dict]:
    """Order by status (in progress, done, planned) keeping document order."""
    return sorted(
        features,
        key=lambda feature: STATUS_ORDER.get(feature.get("status"), 99),
    )


def layout_area(
    area: dict, top: float
) -> tuple[list[tuple[float, float, dict]], list[tuple[str, float]], float]:
    """Place one area's chips and return (chips, group starts, bottom y).

    Each chip is ``(x, y, feature)`` in absolute poster coordinates. Rows are
    fixed height and long names are truncated, so the layout is deterministic.
    A status change always starts a new row, leaving the previous row as-is.
    """
    features = ordered_features(area.get("features", []))
    columns = detail_columns(area.get("id", ""))
    content_w = DETAIL_WIDTH - 2 * DETAIL_MARGIN
    column_w = (content_w - (columns - 1) * CHIP_COL_GAP) / columns

    chips: list[tuple[float, float, dict]] = []
    groups: list[tuple[str, float]] = []
    column = 0
    row_top = top + AREA_HEADER_H
    if features:
        groups.append((features[0].get("status"), row_top))
    previous_status = None
    for feature in features:
        status = feature.get("status")
        if previous_status is not None and status != previous_status:
            row_top += CHIP_HEIGHT + CHIP_ROW_GAP + GROUP_GAP
            groups.append((status, row_top))
            column = 0
        elif column == columns:
            row_top += CHIP_HEIGHT + CHIP_ROW_GAP
            column = 0
        x = DETAIL_MARGIN + column * (column_w + CHIP_COL_GAP)
        chips.append((x, row_top, feature))
        column += 1
        previous_status = status

    bottom = row_top + CHIP_HEIGHT + AREA_BOTTOM_PAD
    return chips, groups, bottom


def render_detail(document: dict) -> str:
    """Render the tall poster that lists every tracked feature."""
    summary = summary_of(document)
    total = summary["features"]
    done = summary["done"]
    in_progress = summary["in_progress"]
    planned = summary["planned"]
    parity = summary["parity_percent"]
    bugs = summary["bugs"]
    reference = document.get("reference", {})
    ref_name = reference.get("name", "Bitwig Studio")
    ref_version = reference.get("version", "")
    ref_revision = reference.get("revision", "")
    ref = f"{ref_name} {ref_version}".strip()
    product = document.get("product", "Hikaru OpenLive")
    audit = document.get("audit", {})
    audit_date = audit.get("date", "")
    workspace_version = audit.get("hikaru_workspace_version", "")
    catalog = audit.get("bitwig_catalog", {})
    devices = int(catalog.get("devices", 0))
    modulators = int(catalog.get("modulators", 0))
    catalog_total = devices + modulators

    areas = document.get("areas", [])
    content_w = DETAIL_WIDTH - 2 * DETAIL_MARGIN
    body: list[str] = []
    add = body.append

    add(f'<rect width="{DETAIL_WIDTH}" height="__HEIGHT__" fill="{BG}"/>')

    # ---------------------------------------------------------------- header
    add(
        f'<text x="{DETAIL_MARGIN}" y="48" font-size="25" font-weight="700" '
        f'fill="{TEXT}">{escape(product)} vs {escape(ref)}</text>'
    )
    subtitle = f"Paridad funcional sobre un catálogo de {total} features"
    if ref_revision:
        subtitle += f" · revisión {ref_revision}"
    if audit_date:
        subtitle += f" · auditoría {audit_date}"
    if workspace_version:
        subtitle += f" · workspace {workspace_version}"
    add(
        f'<text x="{DETAIL_MARGIN}" y="72" font-size="12.5" fill="{MUTED}">'
        f"{escape(subtitle)}</text>"
    )

    # Legend, top right.
    legend_x = DETAIL_WIDTH - DETAIL_MARGIN
    for label, color in (
        ("Pendiente", PLANNED),
        ("En progreso", IN_PROGRESS),
        ("Hecho", DONE),
    ):
        width = text_width(label, 12.0)
        legend_x -= width
        add(
            f'<rect x="{round1(legend_x - 16)}" y="38" width="9" height="9" '
            f'rx="2" fill="{color}"/>'
        )
        add(
            f'<text x="{round1(legend_x)}" y="47" font-size="12" fill="{MUTED}">'
            f"{escape(label)}</text>"
        )
        legend_x -= 34

    # Headline number and the three status counters.
    add(
        f'<text x="{DETAIL_MARGIN}" y="138" font-size="52" font-weight="700" '
        f'fill="{TEXT}">{parity} %</text>'
    )
    add(
        f'<text x="{DETAIL_MARGIN}" y="160" font-size="12" fill="{MUTED}">'
        f"{done} de {total} features implementadas</text>"
    )

    stat_x = DETAIL_MARGIN + 210.0
    for offset, (value, label, color) in enumerate(
        (
            (done, "Hecho", DONE),
            (in_progress, "En progreso", IN_PROGRESS),
            (planned, "Pendiente", PLANNED),
        )
    ):
        x = stat_x + offset * 150.0
        add(
            f'<text x="{round1(x)}" y="118" font-size="28" font-weight="700" '
            f'fill="{color}">{value}</text>'
        )
        add(
            f'<text x="{round1(x)}" y="138" font-size="11.5" fill="{MUTED}">'
            f"{escape(label)}</text>"
        )

    if bugs:
        x = stat_x + 3 * 150.0
        add(
            f'<text x="{round1(x)}" y="118" font-size="28" font-weight="700" '
            f'fill="{BUG}">{bugs}</text>'
        )
        add(
            f'<text x="{round1(x)}" y="138" font-size="11.5" fill="{MUTED}">'
            f"bug{'s' if bugs != 1 else ''} abierto{'s' if bugs != 1 else ''}</text>"
        )

    # Stacked progress bar.
    bar_y = 180.0
    bar_h = 12.0
    add(
        f'<rect x="{DETAIL_MARGIN}" y="{bar_y}" width="{content_w}" '
        f'height="{bar_h}" rx="6" fill="{TILE_BG}"/>'
    )
    cursor = float(DETAIL_MARGIN)
    for value, color in ((done, DONE), (in_progress, IN_PROGRESS)):
        width = content_w * value / total if total else 0.0
        if width > 0:
            add(
                f'<rect x="{round1(cursor)}" y="{bar_y}" width="{round1(width)}" '
                f'height="{bar_h}" fill="{color}"/>'
            )
            cursor += width

    # Caption.
    catalog_done = sum(
        1
        for area in areas
        if area.get("id") in ("builtin-devices-fx", "modulators")
        for feature in area.get("features", [])
        if feature.get("status") == "done"
    )
    catalog_left = catalog_total - catalog_done
    add(
        f'<text x="{DETAIL_MARGIN}" y="{bar_y + 30}" font-size="12" fill="{MUTED}">'
        f"{catalog_left} de {catalog_total} features del catálogo de Bitwig "
        f"({devices} devices + {modulators} modulators) siguen pendientes.</text>"
    )
    add(
        f'<text x="{DETAIL_WIDTH - DETAIL_MARGIN}" y="{bar_y + 30}" font-size="12" '
        f'text-anchor="end" fill="{MUTED}">Verde = implementado · Naranja = en '
        f"progreso · Gris = pendiente</text>"
    )

    # --------------------------------------------------------- area summary
    y = DETAIL_TOP
    add(
        f'<text x="{DETAIL_MARGIN}" y="{round1(y)}" font-size="11" '
        f'letter-spacing="1.4" fill="{MUTED}">RESUMEN POR ÁREA</text>'
    )
    y += 14.0
    row_h = 24.0
    rows = (len(areas) + 1) // 2
    column_w = (content_w - 28.0) / 2
    for index, area in enumerate(areas):
        col = index // rows
        row = index % rows
        x = DETAIL_MARGIN + col * (column_w + 28.0)
        row_y = y + row * row_h
        features = area.get("features", [])
        area_total = len(features)
        area_done = sum(1 for f in features if f.get("status") == "done")
        pct = round(area_done * 100.0 / area_total) if area_total else 0
        name = area.get("name", area.get("id", ""))
        add(
            f'<text x="{round1(x)}" y="{round1(row_y + 11)}" font-size="12" '
            f'fill="{TEXT}">{escape(name)}</text>'
        )
        bar_x = x + column_w * 0.52
        bar_w = column_w * 0.30
        add(
            f'<rect x="{round1(bar_x)}" y="{round1(row_y + 5)}" '
            f'width="{round1(bar_w)}" height="5" rx="2.5" fill="{TILE_BG}"/>'
        )
        if area_total and area_done:
            add(
                f'<rect x="{round1(bar_x)}" y="{round1(row_y + 5)}" '
                f'width="{round1(bar_w * area_done / area_total)}" height="5" '
                f'rx="2.5" fill="{DONE}"/>'
            )
        add(
            f'<text x="{round1(x + column_w)}" y="{round1(row_y + 11)}" '
            f'font-size="11.5" text-anchor="end" fill="{MUTED}">'
            f"{area_done}/{area_total} · {pct} %</text>"
        )
    y += rows * row_h + 22.0

    # ------------------------------------------------------- feature blocks
    catalog_heading = f"CATÁLOGO DE BITWIG · {catalog_total} FEATURES"
    catalog_shown = False

    for area in areas:
        area_id = area.get("id", "")
        features = area.get("features", [])
        if not features:
            continue

        if area_id in ("builtin-devices-fx", "modulators") and not catalog_shown:
            add(
                f'<line x1="{DETAIL_MARGIN}" y1="{round1(y + 10)}" '
                f'x2="{DETAIL_WIDTH - DETAIL_MARGIN}" y2="{round1(y + 10)}" '
                f'stroke="{PLANNED}" stroke-opacity="0.35" stroke-width="1"/>'
            )
            add(
                f'<text x="{DETAIL_MARGIN}" y="{round1(y + 2)}" font-size="11" '
                f'letter-spacing="1.4" fill="{MUTED}">{catalog_heading}</text>'
            )
            y += 24.0
            catalog_shown = True

        area_done = sum(1 for f in features if f.get("status") == "done")
        area_ip = sum(1 for f in features if f.get("status") == "in_progress")
        area_total = len(features)
        pct = round(area_done * 100.0 / area_total) if area_total else 0
        name = area.get("name", area_id)

        add(
            f'<text x="{DETAIL_MARGIN}" y="{round1(y + 16)}" font-size="15" '
            f'font-weight="600" fill="{TEXT}">{escape(name)}</text>'
        )
        count_text = f"{area_done} de {area_total} · {pct} %"
        count_width = text_width(count_text, 11.5)
        add(
            f'<text x="{DETAIL_WIDTH - DETAIL_MARGIN}" y="{round1(y + 16)}" '
            f'font-size="11.5" text-anchor="end" fill="{MUTED}">'
            f"{escape(count_text)}</text>"
        )
        anchor_right = DETAIL_WIDTH - DETAIL_MARGIN - count_width - 14
        if area_ip:
            ip_text = f"{area_ip} en progreso"
            ip_width = text_width(ip_text, 11.0)
            add(
                f'<text x="{round1(anchor_right)}" y="{round1(y + 16)}" '
                f'font-size="11" text-anchor="end" fill="{MUTED}">'
                f"{escape(ip_text)}</text>"
            )
            anchor_right -= ip_width + 10.0
        if int(area.get("bugs", 0)):
            badge_w = 52.0
            badge_x = anchor_right - badge_w
            add(
                f'<rect x="{round1(badge_x)}" y="{round1(y + 2)}" '
                f'width="{badge_w}" height="16" rx="4" fill="{BUG}"/>'
            )
            add(
                f'<text x="{round1(badge_x + badge_w / 2)}" '
                f'y="{round1(y + 14)}" font-size="10.5" font-weight="600" '
                f'text-anchor="middle" fill="#ffffff">'
                f"{int(area.get('bugs', 0))} bug</text>"
            )

        add(
            f'<rect x="{DETAIL_MARGIN}" y="{round1(y + 25)}" width="{content_w}" '
            f'height="4" rx="2" fill="{TILE_BG}"/>'
        )
        cursor = float(DETAIL_MARGIN)
        for value, color in ((area_done, DONE), (area_ip, IN_PROGRESS)):
            width = content_w * value / area_total
            if width > 0:
                add(
                    f'<rect x="{round1(cursor)}" y="{round1(y + 25)}" '
                    f'width="{round1(width)}" height="4" fill="{color}"/>'
                )
                cursor += width

        chips, groups, y = layout_area(area, y)
        columns = detail_columns(area_id)
        column_w = (content_w - (columns - 1) * CHIP_COL_GAP) / columns
        text_max = column_w - 20.0
        status_counts = {
            status: sum(1 for f in features if f.get("status") == status)
            for status in STATUSES
        }
        group_labels = {
            "in_progress": "EN PROGRESO",
            "done": "HECHO",
            "planned": "PENDIENTE",
        }
        for status, group_y in groups:
            add(
                f'<text x="{DETAIL_MARGIN}" y="{round1(group_y - 4)}" '
                f'font-size="9.5" font-weight="600" letter-spacing="1.2" '
                f'fill="{STATUS_COLOR.get(status, PLANNED)}" fill-opacity="0.9">'
                f"{group_labels.get(status, '')} · {status_counts.get(status, 0)}"
                f"</text>"
            )
        for x, chip_y, feature in chips:
            status = feature.get("status")
            color = STATUS_COLOR.get(status, PLANNED)
            label = feature.get("name", feature.get("id", ""))
            short = truncate_to_width(label, text_max, CHIP_FONT)
            feature_id = feature.get("id", "")
            evidence = feature.get("evidence", "")
            tooltip = label
            if feature_id:
                tooltip += f" ({feature_id})"
            if evidence:
                tooltip += f" — {evidence}"
            add(
                f'<g><title>{escape(tooltip)}</title>'
                f'<rect x="{round1(x)}" y="{round1(chip_y)}" width="9" height="9" '
                f'rx="2" fill="{color}"'
                + (' fill-opacity="0.45"' if status == "planned" else "")
                + "/>"
                f'<text x="{round1(x + 16)}" y="{round1(chip_y + 9)}" '
                f'font-size="{CHIP_FONT}" fill="'
                + (TEXT if status != "planned" else MUTED)
                + f'">{escape(short)}</text></g>'
            )

    # ---------------------------------------------------------------- footer
    y += 6.0
    add(
        f'<line x1="{DETAIL_MARGIN}" y1="{round1(y)}" '
        f'x2="{DETAIL_WIDTH - DETAIL_MARGIN}" y2="{round1(y)}" '
        f'stroke="{PLANNED}" stroke-opacity="0.35" stroke-width="1"/>'
    )
    footer = (
        "Fuente: docs/bitwig-parity/features_bitwig.json · "
        "generado por generate_status.py"
    )
    if audit_date:
        footer += f" · auditoría {audit_date}"
    footer += f" · referencia {ref}"
    if ref_revision:
        footer += f" (rev {ref_revision})"
    add(
        f'<text x="{DETAIL_MARGIN}" y="{round1(y + 22)}" font-size="11" '
        f'fill="{MUTED}">{escape(footer)}</text>'
    )
    add(
        f'<text x="{DETAIL_WIDTH - DETAIL_MARGIN}" y="{round1(y + 22)}" '
        f'font-size="11" text-anchor="end" fill="{MUTED}">'
        f"{done} hechas · {in_progress} en progreso · {planned} pendientes</text>"
    )
    height = y + 44.0

    # ---------------------------------------------------------------- output
    aria = (
        f"{product} vs {ref}: every tracked feature by area, "
        f"{done} of {total} done"
    )
    head = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{DETAIL_WIDTH}" '
        f'height="{round1(height)}" viewBox="0 0 {DETAIL_WIDTH} {round1(height)}" '
        f'font-family="{FONT_FAMILY}" role="img" aria-label="{escape(aria)}">',
        f"<title>{escape(aria)}</title>",
        (
            f"<desc>Feature-by-feature parity of {escape(product)} against "
            f"{escape(ref)}: {done} done, {in_progress} in progress, "
            f"{planned} planned, {bugs} open bug(s).</desc>"
        ),
    ]
    rendered = "\n".join(head + body).replace("__HEIGHT__", f"{round1(height)}")
    return rendered + "\n</svg>\n"


def generate_files(
    features_path: str, svg_path: str, detail_path: str, audit_path: str
) -> dict:
    """Read the database and refresh the JSON summary, both SVGs and audit."""
    with open(features_path, "r", encoding="utf-8") as handle:
        document = json.load(handle)

    summary = summary_of(document)
    if document.get("summary") != summary:
        document["summary"] = summary
        with open(features_path, "w", encoding="utf-8") as handle:
            json.dump(document, handle, ensure_ascii=False, indent=2)
            handle.write("\n")

    with open(svg_path, "w", encoding="utf-8") as handle:
        handle.write(render(document))
    with open(detail_path, "w", encoding="utf-8") as handle:
        handle.write(render_detail(document))

    if os.path.exists(audit_path):
        with open(audit_path, "r", encoding="utf-8") as handle:
            original = handle.read()
        synced = sync_audit_markdown(original, document)
        if synced != original:
            with open(audit_path, "w", encoding="utf-8") as handle:
                handle.write(synced)
    return summary


def watch_files(
    features_path: str,
    svg_path: str,
    detail_path: str,
    audit_path: str,
    interval: float,
) -> int:
    """Regenerate on every save of the database until interrupted.

    Polling the mtime keeps the script dependency-free (stdlib only) and works
    on every editor. A half-written JSON is reported but not fatal: the next
    save triggers another attempt.
    """

    def fingerprint() -> tuple[int, int] | None:
        try:
            stat = os.stat(features_path)
        except OSError:
            return None
        return (stat.st_mtime_ns, stat.st_size)

    print(
        f"watching {features_path} every {interval:g}s · Ctrl+C to stop",
        flush=True,
    )
    last = None
    while True:
        try:
            current = fingerprint()
            if current is not None and current != last:
                try:
                    summary = generate_files(
                        features_path, svg_path, detail_path, audit_path
                    )
                except json.JSONDecodeError as exc:
                    last = current
                    print(f"esperando JSON válido: {exc}", file=sys.stderr, flush=True)
                except OSError as exc:
                    last = current
                    print(f"no pude leer/escribir: {exc}", file=sys.stderr, flush=True)
                else:
                    last = fingerprint()
                    stamp = time.strftime("%H:%M:%S")
                    print(
                        f"[{stamp}] {summary['done']}/{summary['features']} · "
                        f"{summary['parity_percent']}% · "
                        f"{summary['in_progress']} en progreso · "
                        f"{summary['planned']} pendientes · "
                        f"{summary['bugs']} bug(s)",
                        flush=True,
                    )
            time.sleep(interval)
        except KeyboardInterrupt:
            print("\nwatch detenido", flush=True)
            return 0


def main(argv: list[str]) -> int:
    # Defaults live next to this script, so the parity directory is portable:
    # it can be executed from the repo root, from CI, or from anywhere else.
    base = os.path.dirname(os.path.abspath(__file__))
    check = "--check" in argv
    watch = "--watch" in argv
    interval = 0.25
    args: list[str] = []
    for arg in argv[1:]:
        if arg.startswith("--interval="):
            try:
                interval = max(0.05, float(arg.split("=", 1)[1]))
            except ValueError:
                print(f"intervalo inválido: {arg!r}", file=sys.stderr)
                return 2
        elif not arg.startswith("--"):
            args.append(arg)

    features_path = args[0] if args else os.path.join(base, "features_bitwig.json")
    svg_path = args[1] if len(args) > 1 else os.path.join(base, "status.svg")
    detail_path = args[2] if len(args) > 2 else os.path.join(base, "parity-detail.svg")
    audit_path = (
        args[3]
        if len(args) > 3
        else os.path.join(os.path.dirname(features_path), "bitwig-5.1.7-parity-audit.md")
    )

    if watch:
        if check:
            print("error: --watch y --check son mutuamente excluyentes", file=sys.stderr)
            return 2
        return watch_files(features_path, svg_path, detail_path, audit_path, interval)

    if check:
        with open(features_path, "r", encoding="utf-8") as handle:
            document = json.load(handle)

        summary = summary_of(document)
        problems = []
        if document.get("summary") != summary:
            problems.append("summary in features JSON is stale")
        for path, generated, label in (
            (svg_path, render(document), "status.svg"),
            (detail_path, render_detail(document), "parity-detail.svg"),
        ):
            if os.path.exists(path):
                with open(path, "r", encoding="utf-8") as handle:
                    if handle.read() != generated:
                        problems.append(f"{label} is stale")
            else:
                problems.append(f"{label} is missing")
        if os.path.exists(audit_path):
            with open(audit_path, "r", encoding="utf-8") as handle:
                audit_original = handle.read()
            if sync_audit_markdown(audit_original, document) != audit_original:
                problems.append("parity table in bitwig-5.1.7-parity-audit.md is stale")
        if problems:
            for problem in problems:
                print(f"error: {problem}", file=sys.stderr)
            print("run: python3 docs/bitwig-parity/generate_status.py", file=sys.stderr)
            return 1
        print("status figures are up to date")
        return 0

    summary = generate_files(features_path, svg_path, detail_path, audit_path)
    print(
        f"wrote {svg_path} and {detail_path}: {summary['done']} of "
        f"{summary['features']} features done ({summary['parity_percent']}%), "
        f"{summary['bugs']} open bug(s)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
