#!/usr/bin/env python3
"""Generate webcloner Tauri icon files (globe + clone panel design)."""

from __future__ import annotations

import math
import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ICON_DIRS = [
    ROOT / "gui" / "src-tauri" / "icons",
    ROOT / "desktop-app-template" / "src-tauri" / "icons",
]


def _lerp(a: float, b: float, t: float) -> float:
    return a + (b - a) * t


def _bg_color(x: int, y: int, size: int) -> tuple[int, int, int]:
    t = (x + y) / max(1, 2 * (size - 1))
    r = int(_lerp(37, 124, t))
    g = int(_lerp(99, 58, t))
    b = int(_lerp(235, 237, t))
    return r, g, b


def _rounded_rect_mask(x: int, y: int, size: int, radius_ratio: float = 0.22) -> bool:
    margin = size * 0.06
    rx = margin
    ry = margin
    rw = size - 2 * margin
    rh = size - 2 * margin
    rad = size * radius_ratio
    if x < rx or y < ry or x >= rx + rw or y >= ry + rh:
        return False
    corners = (
        (rx + rad, ry + rad),
        (rx + rw - rad, ry + rad),
        (rx + rad, ry + rh - rad),
        (rx + rw - rad, ry + rh - rad),
    )
    for cx, cy in corners:
        if (x < rx + rad and y < ry + rad and (x - cx) ** 2 + (y - cy) ** 2 > rad**2) or (
            x > rx + rw - rad and y < ry + rad and (x - cx) ** 2 + (y - cy) ** 2 > rad**2
        ) or (x < rx + rad and y > ry + rh - rad and (x - cx) ** 2 + (y - cy) ** 2 > rad**2) or (
            x > rx + rw - rad and y > ry + rh - rad and (x - cx) ** 2 + (y - cy) ** 2 > rad**2
        ):
            return False
    return True


def _draw_icon(size: int) -> list[tuple[int, int, int, int]]:
    pixels: list[tuple[int, int, int, int]] = []
    cx_globe = int(size * 0.41)
    cy_globe = int(size * 0.45)
    r_globe = int(size * 0.19)

    for y in range(size):
        for x in range(size):
            if not _rounded_rect_mask(x, y, size):
                pixels.append((0, 0, 0, 0))
                continue

            r, g, b = _bg_color(x, y, size)
            alpha = 255

            # Shine overlay
            if y < size * 0.45:
                shine = 1.0 - y / (size * 0.45)
                r = min(255, int(r + 40 * shine))
                g = min(255, int(g + 40 * shine))
                b = min(255, int(b + 40 * shine))

            # Globe ring
            dx = x - cx_globe
            dy = y - cy_globe
            dist = math.hypot(dx, dy)
            if abs(dist - r_globe) < size * 0.018:
                r, g, b, alpha = 224, 231, 255, 240
            elif dist < r_globe:
                # meridian
                if abs(dx) < size * 0.012 or abs(dy) < size * 0.012:
                    r, g, b, alpha = 199, 210, 254, 210
                elif abs((dx / max(1, r_globe)) ** 2 + (dy / max(1, r_globe)) ** 2 - 1) < 0.08 and abs(dx) < r_globe * 0.45:
                    r, g, b, alpha = 199, 210, 254, 180

            # Clone panel (white card)
            px0, py0 = int(size * 0.58), int(size * 0.28)
            pw, ph = int(size * 0.27), int(size * 0.34)
            if px0 <= x < px0 + pw and py0 <= y < py0 + ph:
                r, g, b, alpha = 255, 255, 255, 235
                line_y = [0.12, 0.24, 0.36, 0.48]
                line_w = [0.62, 0.52, 0.58, 0.42]
                for i, ly in enumerate(line_y):
                    ly_px = py0 + int(ph * ly)
                    lw = int(pw * line_w[i])
                    lx0 = px0 + int(pw * 0.18)
                    if ly_px <= y < ly_px + max(1, int(size * 0.025)) and lx0 <= x < lx0 + lw:
                        r, g, b = (99, 102, 241) if i == 0 else (129, 140, 248)

            # Arrow
            ax0, ay = int(size * 0.52), int(size * 0.68)
            ax1 = int(size * 0.72)
            if ay - int(size * 0.03) <= y <= ay + int(size * 0.03) and ax0 <= x <= ax1:
                r, g, b, alpha = 191, 219, 254, 240
            if int(size * 0.68) <= x <= int(size * 0.74) and int(size * 0.62) <= y <= int(size * 0.74):
                if y - int(size * 0.68) <= x - int(size * 0.68) <= int(size * 0.06):
                    r, g, b, alpha = 191, 219, 254, 240

            pixels.append((r, g, b, alpha))
    return pixels


def _png_chunk(tag: bytes, data: bytes) -> bytes:
    crc = zlib.crc32(tag + data) & 0xFFFFFFFF
    return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", crc)


def make_png(size: int) -> bytes:
    pixels = _draw_icon(size)
    rows = bytearray()
    for y in range(size):
        rows.append(0)
        for x in range(size):
            r, g, b, a = pixels[y * size + x]
            rows.extend((r, g, b, a))

    compressed = zlib.compress(bytes(rows), 9)
    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + _png_chunk(b"IHDR", ihdr)
        + _png_chunk(b"IDAT", compressed)
        + _png_chunk(b"IEND", b"")
    )


def make_ico(sizes: tuple[int, ...]) -> bytes:
    images = [(size, make_png(size)) for size in sizes]
    header = struct.pack("<HHH", 0, 1, len(images))
    entries = bytearray()
    data = bytearray()
    offset = 6 + 16 * len(images)

    for size, png in images:
        width = 0 if size >= 256 else size
        height = width
        entries.extend(struct.pack("<BBBBHHII", width, height, 0, 0, 1, 32, len(png), offset))
        data.extend(png)
        offset += len(png)

    return header + bytes(entries) + bytes(data)


def make_icns(png_128: bytes) -> bytes:
    png_type = b"ic08"
    body = png_type + struct.pack(">I", len(png_128)) + png_128
    return b"icns" + struct.pack(">I", len(body) + 8) + body


def write_icons(target_dir: Path) -> None:
    target_dir.mkdir(parents=True, exist_ok=True)
    png32 = make_png(32)
    png128 = make_png(128)
    (target_dir / "32x32.png").write_bytes(png32)
    (target_dir / "128x128.png").write_bytes(png128)
    (target_dir / "icon.ico").write_bytes(make_ico((16, 32, 48, 64, 128, 256)))
    (target_dir / "icon.icns").write_bytes(make_icns(png128))
    print(f"Generated icons in {target_dir}")


def main() -> None:
    for icon_dir in ICON_DIRS:
        write_icons(icon_dir)


if __name__ == "__main__":
    main()
