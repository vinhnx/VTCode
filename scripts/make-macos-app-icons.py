#!/usr/bin/env python3
"""Apply the macOS squircle mask to VT Code app-icon artwork.

macOS does not clip ``.icns`` art or ``NSApplication.setApplicationIconImage``
images to the system icon shape: full-bleed opaque squares render as squares
in the Dock and Finder. (Only the layered Icon Composer path in
``resources/icons/AppIcon.icon`` is system-shaped, on macOS 26+.) The shipped
``vtcode-app-*`` PNGs and the ``vtcode-app-dynamic.icns`` fallback must
therefore carry the rounded shape with transparency baked in. This script
applies that mask reproducibly.

Geometry: stock macOS tile — the art is scaled to an 824px content box centered
on the 1024 canvas (100px transparent margins, matching Finder) and masked with
a superellipse (|x|^n + |y|^n <= 1, n = 5) fitted to that box, rendered with 4x
supersampling for antialiased edges. Full-bleed art would render oversized next
to stock icons. The mask is idempotent, so re-running on already-masked art is
a no-op for the shape.

Usage (from the repo root):
    pip install pillow  # once
    python3 scripts/make-macos-app-icons.py \
        --light /path/to/flat-light-1024.png \
        --dark /path/to/flat-dark-1024.png \
        --dynamic /path/to/flat-dynamic-1024.png

This rewrites the six ``resources/icons/vtcode-app-*.png`` files and stages
``/tmp/VTCode.iconset``; build the fallback with:
    iconutil -c icns /tmp/VTCode.iconset -o resources/icons/vtcode-app-dynamic.icns
    iconutil -c iconset resources/icons/vtcode-app-dynamic.icns -o /tmp/check.iconset

Inputs may be any square PNG (flat or already masked); non-1024 inputs are
scaled to the 1024 working size first (exact 2x NEAREST for 512px flat
geometric art, LANCZOS otherwise).
"""

from __future__ import annotations

import argparse
import math
import pathlib
import sys

try:
    from PIL import Image
except ImportError:  # pragma: no cover - dependency hint, not logic
    print("Error: Pillow is required (pip install pillow).", file=sys.stderr)
    sys.exit(1)

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent
ICONS_DIR = REPO_ROOT / "resources" / "icons"
WORKING_SIZE = 1024
CONTENT_SIZE = 824
CONTENT_OFFSET = (WORKING_SIZE - CONTENT_SIZE) // 2
SUPERSAMPLE = 4
SQUIRCLE_EXPONENT = 5.0

# iconutil slot filename -> pixel size, rendered from the Dynamic master.
ICONSET_SLOTS = {
    "icon_16x16.png": 16,
    "icon_16x16@2x.png": 32,
    "icon_32x32.png": 32,
    "icon_32x32@2x.png": 64,
    "icon_128x128.png": 128,
    "icon_128x128@2x.png": 256,
    "icon_256x256.png": 256,
    "icon_256x256@2x.png": 512,
    "icon_512x512.png": 512,
    "icon_512x512@2x.png": 1024,
}


def squircle_alpha(size: int) -> Image.Image:
    """White-on-black superellipse mask for a ``size`` square canvas."""
    hires = size * SUPERSAMPLE
    half = hires / 2.0
    mask = Image.new("L", (hires, hires))
    px = mask.load()
    assert px is not None
    exponent = SQUIRCLE_EXPONENT
    for y in range(hires):
        ny = abs((y + 0.5 - half) / half)
        ny_pow = math.pow(ny, exponent)
        for x in range(hires):
            nx = abs((x + 0.5 - half) / half)
            inside = math.pow(nx, exponent) + ny_pow <= 1.0
            px[x, y] = 255 if inside else 0
    return mask.resize((size, size), Image.Resampling.LANCZOS)


def load_working_master(path: pathlib.Path) -> Image.Image:
    """Load square art scaled to the 1024 working size as RGBA."""
    art = Image.open(path).convert("RGBA")
    if art.size[0] != art.size[1]:
        raise ValueError(f"icon art must be square: {path} is {art.size[0]}x{art.size[1]}")
    if art.size[0] == WORKING_SIZE:
        return art
    if art.size[0] * 2 == WORKING_SIZE:
        # Exact 2x for flat geometric masters: preserves hard edges pixel-perfect.
        return art.resize((WORKING_SIZE, WORKING_SIZE), Image.Resampling.NEAREST)
    return art.resize((WORKING_SIZE, WORKING_SIZE), Image.Resampling.LANCZOS)


def apply_mask(master: Image.Image, alpha: Image.Image) -> Image.Image:
    """Intersect the master's alpha channel with the squircle mask."""
    r, g, b, a = master.split()
    masked_alpha = Image.new("L", master.size)
    a_px = a.load()
    m_px = alpha.load()
    out_px = masked_alpha.load()
    assert a_px is not None and m_px is not None and out_px is not None
    for y in range(master.size[1]):
        for x in range(master.size[0]):
            out_px[x, y] = (a_px[x, y] * m_px[x, y] + 127) // 255
    merged = Image.merge("RGBA", (r, g, b, masked_alpha))
    return merged


def resize_rgba(art: Image.Image, size: int) -> Image.Image:
    """Rescale RGBA art without dark fringes, via premultiplied alpha.

    Plain channel-independent resampling blends interior colors with the black
    RGB of transparent pixels, leaving a dark halo along the tile edge. Scaling
    premultiplied keeps edge RGB at the tile color under partial alpha.
    """
    if art.size == (size, size):
        return art.copy()
    bands = [band.tobytes() for band in art.split()]
    *rgb, alpha = bands
    premultiplied = [bytes((v * a + 127) // 255 for v, a in zip(band, alpha)) for band in rgb]
    scaled = [
        Image.frombytes("L", art.size, data).resize((size, size), Image.Resampling.LANCZOS).tobytes()
        for data in (*premultiplied, alpha)
    ]
    *scaled_rgb, scaled_alpha = scaled
    straight = [
        bytes(min(255, (v * 255 + a // 2) // a) if a else 0 for v, a in zip(band, scaled_alpha))
        for band in scaled_rgb
    ]
    return Image.merge(
        "RGBA", [Image.frombytes("L", (size, size), data) for data in (*straight, scaled_alpha)]
    )


def inset_masked_master(master: Image.Image, alpha: Image.Image) -> Image.Image:
    """Scale the 1024 master into the centered content box and mask it."""
    content = resize_rgba(master, CONTENT_SIZE)
    masked = apply_mask(content, alpha)
    canvas = Image.new("RGBA", (WORKING_SIZE, WORKING_SIZE), (0, 0, 0, 0))
    canvas.paste(masked, (CONTENT_OFFSET, CONTENT_OFFSET), masked)
    return canvas


def check_shape(name: str, art: Image.Image) -> None:
    """Fail loudly when the masked master misses the stock inset geometry."""
    size = art.size[0]
    mid = size // 2
    last = size - 1
    corners = [art.getpixel((0, 0))[3], art.getpixel((last, 0))[3],
               art.getpixel((0, last))[3], art.getpixel((last, last))[3]]
    if any(a > 8 for a in corners):
        raise ValueError(f"{name}: corners must be transparent, got alpha {corners}")
    # The 100px margins stay transparent: edge midpoints must be clear.
    margins = [art.getpixel((mid, 0))[3], art.getpixel((0, mid))[3],
               art.getpixel((last, mid))[3], art.getpixel((mid, last))[3]]
    if any(a > 8 for a in margins):
        raise ValueError(f"{name}: edge midpoints must be transparent (inset), got alpha {margins}")
    # Just inside the content box the tile must be fully opaque.
    inner = CONTENT_OFFSET + 28
    edges = [art.getpixel((mid, inner))[3], art.getpixel((inner, mid))[3],
             art.getpixel((last - inner, mid))[3], art.getpixel((mid, last - inner))[3]]
    if any(a < 248 for a in edges):
        raise ValueError(f"{name}: just inside the content box must be opaque, got alpha {edges}")
    if art.getpixel((mid, mid))[3] < 248:
        raise ValueError(f"{name}: center must stay opaque")


def write_png(art: Image.Image, path: pathlib.Path) -> None:
    art.save(path, "PNG", optimize=True)
    print(f"Wrote {path.relative_to(REPO_ROOT)} ({art.size[0]}x{art.size[1]})")


def main() -> int:
    parser = argparse.ArgumentParser(description="Bake the macOS squircle mask into app-icon art.")
    parser.add_argument("--light", type=pathlib.Path, required=True, help="Flat Light master (square PNG).")
    parser.add_argument("--dark", type=pathlib.Path, required=True, help="Flat Dark master (square PNG).")
    parser.add_argument("--dynamic", type=pathlib.Path, required=True, help="Flat Dynamic master (square PNG).")
    parser.add_argument("--icons-dir", type=pathlib.Path, default=ICONS_DIR, help="Output dir for shipped PNGs.")
    parser.add_argument("--iconset-dir", type=pathlib.Path, default=pathlib.Path("/tmp/VTCode.iconset"),
                        help="Staging dir for the Dynamic iconset.")
    args = parser.parse_args()

    try:
        alpha = squircle_alpha(CONTENT_SIZE)
        masters = {
            "light": inset_masked_master(load_working_master(args.light), alpha),
            "dark": inset_masked_master(load_working_master(args.dark), alpha),
            "dynamic": inset_masked_master(load_working_master(args.dynamic), alpha),
        }
        for name, art in masters.items():
            check_shape(name, art)
    except (ValueError, OSError) as exc:
        print(f"Error: {exc}", file=sys.stderr)
        return 1

    args.icons_dir.mkdir(parents=True, exist_ok=True)
    for name, art in masters.items():
        for size in (512, 128):
            write_png(resize_rgba(art, size), args.icons_dir / f"vtcode-app-{name}-{size}.png")

    args.iconset_dir.mkdir(parents=True, exist_ok=True)
    for filename, size in sorted(ICONSET_SLOTS.items(), key=lambda item: item[1]):
        slot = resize_rgba(masters["dynamic"], size)
        slot.save(args.iconset_dir / filename, "PNG", optimize=True)
    print(f"Staged {len(ICONSET_SLOTS)} iconset slots in {args.iconset_dir}")
    print("Next: iconutil -c icns "
          f"{args.iconset_dir} -o {ICONS_DIR.relative_to(REPO_ROOT)}/vtcode-app-dynamic.icns")
    return 0


if __name__ == "__main__":
    sys.exit(main())
