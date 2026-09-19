#!/usr/bin/env python3
"""Regenerate InterFire brand icons: transparent canvas + distinct tray states."""
from pathlib import Path

from PIL import Image

brand = Path(__file__).resolve().parents[1] / "docs" / "brand"


def make_transparent(src: Path, dst: Path, threshold: int = 28) -> None:
    im = Image.open(src).convert("RGBA")
    px = im.load()
    w, h = im.size
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if r < threshold and g < threshold and b < threshold:
                px[x, y] = (r, g, b, 0)
    dst.parent.mkdir(parents=True, exist_ok=True)
    im.save(dst, "PNG")
    print(f"wrote {dst.name} ({dst.stat().st_size} bytes)")


def recolor_muted(src: Path, dst: Path) -> None:
    im = Image.open(src).convert("RGBA")
    px = im.load()
    w, h = im.size
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if a < 8:
                continue
            if r < 28 and g < 28 and b < 28:
                px[x, y] = (0, 0, 0, 0)
                continue
            lum = int(0.3 * r + 0.5 * g + 0.2 * b)
            grey = int(lum * 0.55 + 90)
            px[x, y] = (grey, min(255, grey + 4), min(255, grey + 10), a)
    im.save(dst, "PNG")
    print(f"wrote {dst.name}")


def recolor_blocked(src: Path, dst: Path) -> None:
    im = Image.open(src).convert("RGBA")
    px = im.load()
    w, h = im.size
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if a < 8:
                continue
            if r < 28 and g < 28 and b < 28:
                px[x, y] = (0, 0, 0, 0)
                continue
            px[x, y] = (min(255, int(r * 0.4 + 180)), int(g * 0.25), int(b * 0.25), a)
    im.save(dst, "PNG")
    print(f"wrote {dst.name}")


def ensure_size(size: int) -> Path:
    path = brand / f"icon-app-phoenix-{size}.png"
    if path.exists():
        return path
    base = brand / "icon-app-phoenix-256.png"
    if not base.exists():
        base = brand / "icon-app-phoenix.png"
    im = Image.open(base).convert("RGBA").resize((size, size), Image.Resampling.LANCZOS)
    im.save(path)
    return path


def main() -> None:
    for size in (16, 32, 48, 64, 128, 256):
        src = ensure_size(size)
        make_transparent(src, brand / f"icon-app-phoenix-{size}.png")

    make_transparent(brand / "icon-app-phoenix-64.png", brand / "icon-app-phoenix-64.png")
    make_transparent(
        brand / "icon-app-phoenix-gradient-64.png", brand / "icon-app-phoenix-gradient-64.png"
    )
    recolor_muted(brand / "icon-app-phoenix-64.png", brand / "icon-tray-paused-64.png")
    recolor_muted(brand / "icon-app-phoenix-light-64.png", brand / "icon-tray-degraded-64.png")
    recolor_blocked(brand / "icon-app-phoenix-64.png", brand / "icon-tray-blocked-64.png")
    make_transparent(brand / "icon-tray-unavailable-64.png", brand / "icon-tray-unavailable-64.png")

    for name in ("mark-phoenix-head-64.png", "icon-app-phoenix-light-64.png"):
        path = brand / name
        if path.exists():
            make_transparent(path, path)


if __name__ == "__main__":
    main()
