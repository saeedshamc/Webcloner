#!/usr/bin/env python3
"""Generate placeholder Tauri icon files for Windows/macOS/Linux builds."""

from __future__ import annotations

import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ICON_DIRS = [
    ROOT / "gui" / "src-tauri" / "icons",
    ROOT / "desktop-app-template" / "src-tauri" / "icons",
]


def _png_chunk(tag: bytes, data: bytes) -> bytes:
    crc = zlib.crc32(tag + data) & 0xFFFFFFFF
    return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", crc)


def make_png(size: int) -> bytes:
    """Build a simple blue gradient PNG (RGBA)."""
    rows = bytearray()
    for y in range(size):
        rows.append(0)  # filter type None
        for x in range(size):
            t = (x + y) / max(1, 2 * (size - 1))
            r = int(37 + (59 - 37) * (1 - t))
            g = int(99 + (130 - 99) * (1 - t))
            b = int(235 + (246 - 235) * (1 - t))
            rows.extend((r, g, b, 255))

    compressed = zlib.compress(bytes(rows), 9)
    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + _png_chunk(b"IHDR", ihdr)
        + _png_chunk(b"IDAT", compressed)
        + _png_chunk(b"IEND", b"")
    )


def make_ico(sizes: tuple[int, ...]) -> bytes:
    """ICO container with embedded PNG images (Windows Vista+)."""
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
    """Minimal ICNS with one 128x128 PNG resource."""
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
