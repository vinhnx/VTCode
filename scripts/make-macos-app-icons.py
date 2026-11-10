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

Clear additionally gets a subtle glass tile baked under the V (translucent
fill, bright rim, edge hairline): flat PNGs get no system glass composition,
so without it the Clear V floats borderless. The ``Clear`` layer background
stays transparent as the Icon Composer source; only the shipped flat PNGs
carry the baked tile. Opaque tiles (Light/Dark/Tinted/Dynamic) get the
matching Liquid Glass shading baked over the tile (top sheen, bright rim,
hairline) so a Dock override still answers the modern glass sheen a
system-composed icon carries; see :func:`opaque_glass_shading`.

Usage (from the repo root):
    pip install pillow  # once
    python3 scripts/make-macos-app-icons.py \
        --light /path/to/flat-light-1024.png \
        --dark /path/to/flat-dark-1024.png \
        --dynamic /path/to/flat-dynamic-1024.png \
        --clear resources/icons/liquid-glass/Clear \
        --tinted resources/icons/liquid-glass/Tinted

This rewrites the ``resources/icons/vtcode-app-*.png`` files for the given
variants and, when ``--dynamic`` is passed, stages ``/tmp/VTCode.iconset``;
build the fallback with:
    iconutil -c icns /tmp/VTCode.iconset -o resources/icons/vtcode-app-dynamic.icns
    iconutil -c iconset resources/icons/vtcode-app-dynamic.icns -o /tmp/check.iconset

Each variant accepts a square PNG master or a layer directory holding
``01-background.png`` + ``02-V-foreground.png`` (composited back-to-front,
as in ``resources/icons/liquid-glass/``). PNG inputs may be flat or already
masked; non-1024 inputs are scaled to the 1024 working size first (exact 2x
NEAREST for 512px flat geometric art, LANCZOS otherwise). At least one
variant is required; omitted variants keep their shipped files untouched.
"""

from __future__ import annotations

import argparse
import math
import pathlib
import sys

try:
    from PIL import Image, ImageChops, ImageFilter
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

# Clear glass tile (alpha at the 824 content-box scale): macOS 26+ composes
# Clear icons as a frosted glass tile with a bright edge, but flat PNGs get
# no system composition (the Dock override, iTerm2 profile art, and picker
# previews all show the raw pixels), so the tile is baked here. Without it
# the Clear V floats borderless next to neighbors that all carry tiles.
# Values were picked from dark/light Dock-size previews: the fill lifts a
# dark Dock faintly and vanishes on light backgrounds, while the rim and
# hairline keep the tile boundary defined on both.
CLEAR_TILE_FILL = 22
CLEAR_RIM_TOP = 96
CLEAR_RIM_BOTTOM = 48
CLEAR_RIM_RADIUS = 5
CLEAR_SHEEN = 34
CLEAR_SHEEN_FRACTION = 0.28
CLEAR_HAIRLINE = 30
CLEAR_HAIRLINE_RADIUS = 2

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


def composite_layers(directory: pathlib.Path) -> Image.Image:
    """Composite a layer directory back-to-front into square RGBA art."""
    background = Image.open(directory / "01-background.png").convert("RGBA")
    foreground = Image.open(directory / "02-V-foreground.png").convert("RGBA")
    if background.size != foreground.size:
        raise ValueError(
            f"layer sizes differ in {directory}: {background.size} vs {foreground.size}"
        )
    return Image.alpha_composite(background, foreground)


def load_working_master(path: pathlib.Path) -> Image.Image:
    """Load square art scaled to the 1024 working size as RGBA.

    ``path`` is a PNG master or a layer directory (see
    :func:`composite_layers`).
    """
    art = composite_layers(path) if path.is_dir() else Image.open(path).convert("RGBA")
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


def _solid_layer(size: int, rgb: tuple[int, int, int], alpha: Image.Image) -> Image.Image:
    """Opaque-RGB layer carrying the given alpha channel."""
    bands = [Image.new("L", (size, size), value) for value in rgb]
    bands.append(alpha)
    merged = Image.merge("RGBA", bands)
    return merged


def _vertical_ramp(size: int, top: int, bottom: int, fraction: float = 1.0) -> Image.Image:
    """L gradient holding ``top`` at row 0, fading to ``bottom`` over the top ``fraction``."""
    rows = max(1, int(size * fraction))
    column = Image.new("L", (1, size), bottom)
    column_px = column.load()
    assert column_px is not None
    for y in range(rows):
        column_px[0, y] = top + (bottom - top) * y // max(1, rows - 1)
    return column.resize((size, size))


def _erode(alpha: Image.Image, radius: int) -> Image.Image:
    """Min-filter erosion that treats out-of-bounds as transparent.

    Plain ``MinFilter`` clamps the image edge, so erosion never bites the
    straight squircle edges (only the corners) and the rim band would miss
    them. Padding with zeros first keeps the band uniform all around.
    """
    pad = radius + 1
    size = alpha.size[0]
    padded = Image.new("L", (size + 2 * pad, size + 2 * pad), 0)
    padded.paste(alpha, (pad, pad))
    eroded = padded.filter(ImageFilter.MinFilter(2 * radius + 1))
    return eroded.crop((pad, pad, pad + size, pad + size))


def clear_glass_tile(alpha: Image.Image) -> Image.Image:
    """Frosted glass tile clipped by the content-box squircle ``alpha`` mask.

    Layers back-to-front: translucent fill, top sheen, bright inner rim
    (stronger at the top, like glass lit from above), and a dark outer
    hairline for definition on light backgrounds. The V foreground is
    composited over the result by the caller.
    """
    size = alpha.size[0]
    tile = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    tile = Image.alpha_composite(tile, _solid_layer(size, (255, 255, 255),
                                                   alpha.point(lambda v: v * CLEAR_TILE_FILL // 255)))
    sheen = ImageChops.multiply(alpha, _vertical_ramp(size, CLEAR_SHEEN, 0, CLEAR_SHEEN_FRACTION))
    tile = Image.alpha_composite(tile, _solid_layer(size, (255, 255, 255), sheen))
    band = ImageChops.subtract(alpha, _erode(alpha, CLEAR_RIM_RADIUS))
    gradient = _vertical_ramp(size, 255, 255 * CLEAR_RIM_BOTTOM // CLEAR_RIM_TOP)
    rim = ImageChops.multiply(band, gradient).point(lambda v: v * CLEAR_RIM_TOP // 255)
    tile = Image.alpha_composite(tile, _solid_layer(size, (255, 255, 255), rim))
    edge = ImageChops.subtract(alpha, _erode(alpha, CLEAR_HAIRLINE_RADIUS))
    hairline = edge.point(lambda v: v * CLEAR_HAIRLINE // 255)
    tile = Image.alpha_composite(tile, _solid_layer(size, (0, 0, 0), hairline))
    return tile


def opaque_glass_shading(alpha: Image.Image) -> Image.Image:
    """Subtle Liquid Glass shading overlay for opaque tiles.

    Flat PNGs get no system composition (the Dock override,
    iTerm2 profile art, and picker previews all show raw pixels), so
    opaque Light/Dark/Tinted/Dynamic tiles need baked shading to mimic
    the modern macOS glass sheen a system-composed icon carries.
    Layers: faint top sheen, bright inner rim (stronger at the top,
    like glass lit from above), and a dark outer hairline.
    """
    size = alpha.size[0]
    overlay = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    sheen = ImageChops.multiply(alpha, _vertical_ramp(size, 22, 0, 0.28))
    overlay = Image.alpha_composite(overlay, _solid_layer(size, (255, 255, 255), sheen))
    band = ImageChops.subtract(alpha, _erode(alpha, 5))
    gradient = _vertical_ramp(size, 255, 255 * 42 // 72)
    rim = ImageChops.multiply(band, gradient).point(lambda v: v * 72 // 255)
    overlay = Image.alpha_composite(overlay, _solid_layer(size, (255, 255, 255), rim))
    edge = ImageChops.subtract(alpha, _erode(alpha, 2))
    hairline = edge.point(lambda v: v * 28 // 255)
    overlay = Image.alpha_composite(overlay, _solid_layer(size, (0, 0, 0), hairline))
    return overlay


def apply_opaque_shading(canvas: Image.Image, alpha: Image.Image) -> Image.Image:
    """Composite opaque glass shading over an opaque tile on the 1024 canvas.

    The overlay reuses the caller's squircle mask, so the shading aligns
    exactly with the tile inset. Placement paste carries no mask (opaque
    tile alphas are unaffected by the self-mask quirk noted in
    :func:`apply_clear_glass_tile`).
    """
    overlay = Image.new("RGBA", (WORKING_SIZE, WORKING_SIZE), (0, 0, 0, 0))
    overlay.paste(opaque_glass_shading(alpha), (CONTENT_OFFSET, CONTENT_OFFSET))
    return Image.alpha_composite(canvas, overlay)


def apply_clear_glass_tile(canvas: Image.Image, alpha: Image.Image) -> Image.Image:
    """Lay the glass tile under the masked Clear V on the 1024 canvas.

    The tile reuses the caller's squircle mask, so its boundary aligns
    exactly with the V inset. The placement paste carries no mask: a
    self-mask paste would square the translucent alphas (opaque masters
    are unaffected, which is why :func:`inset_masked_master` gets away
    with it).
    """
    tile = Image.new("RGBA", (WORKING_SIZE, WORKING_SIZE), (0, 0, 0, 0))
    tile.paste(clear_glass_tile(alpha), (CONTENT_OFFSET, CONTENT_OFFSET))
    return Image.alpha_composite(tile, canvas)


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


def check_shape_clear(name: str, art: Image.Image) -> None:
    """Fail loudly when the Clear master misses the glass-tile geometry.

    Clear carries a subtle translucent tile under the glass V: the canvas
    margins stay transparent, the V counter shows only the faint fill,
    just inside the top edge the bright rim reads well above the fill,
    and the opaque fraction stays in the narrow glass-V band (only the
    strokes are opaque). Fails on fully transparent backgrounds (no
    tile), on opaque tiles passed as Clear, and on empty/missing glyphs.
    """
    size = art.size[0]
    mid = size // 2
    last = size - 1
    clear_points = [(0, 0), (last, 0), (0, last), (last, last),
                    (mid, 0), (0, mid), (last, mid), (mid, last)]
    for x, y in clear_points:
        if art.getpixel((x, y))[3] > 8:
            raise ValueError(f"{name}: margin ({x}, {y}) must be transparent")
    fill = art.getpixel((mid, mid))[3]
    if not 8 <= fill <= 80:
        raise ValueError(f"{name}: V counter ({mid}, {mid}) must show only the faint fill, got alpha {fill}")
    rim = art.getpixel((mid, CONTENT_OFFSET + 3))[3]
    if rim <= 70 or rim > 200 or rim <= fill + 40:
        raise ValueError(
            f"{name}: just inside the top edge must catch the bright-but-subtle rim,"
            f" got alpha {rim} over fill {fill}"
        )
    histogram = art.getchannel("A").histogram()
    opaque = sum(histogram[248:]) / (size * size)
    if not 0.005 <= opaque <= 0.15:
        raise ValueError(f"{name}: opaque fraction {opaque:.3f} outside the 0.005-0.15 glass-V band")


def write_png(art: Image.Image, path: pathlib.Path) -> None:
    art.save(path, "PNG", optimize=True)
    print(f"Wrote {path.relative_to(REPO_ROOT)} ({art.size[0]}x{art.size[1]})")


def main() -> int:
    parser = argparse.ArgumentParser(description="Bake the macOS squircle mask into app-icon art.")
    parser.add_argument("--light", type=pathlib.Path, help="Light master (square PNG or layer dir).")
    parser.add_argument("--dark", type=pathlib.Path, help="Dark master (square PNG or layer dir).")
    parser.add_argument("--dynamic", type=pathlib.Path, help="Dynamic master (square PNG or layer dir).")
    parser.add_argument("--clear", type=pathlib.Path,
                        help="Clear master (square PNG or layer dir, e.g. resources/icons/liquid-glass/Clear).")
    parser.add_argument("--tinted", type=pathlib.Path,
                        help="Tinted master (square PNG or layer dir, e.g. resources/icons/liquid-glass/Tinted).")
    parser.add_argument("--icons-dir", type=pathlib.Path, default=ICONS_DIR, help="Output dir for shipped PNGs.")
    parser.add_argument("--iconset-dir", type=pathlib.Path, default=pathlib.Path("/tmp/VTCode.iconset"),
                        help="Staging dir for the Dynamic iconset.")
    args = parser.parse_args()

    requested = [(name, getattr(args, name)) for name in ("light", "dark", "dynamic", "clear", "tinted")]
    requested = [(name, path) for name, path in requested if path is not None]
    if not requested:
        print("Error: pass at least one of --light/--dark/--dynamic/--clear/--tinted.", file=sys.stderr)
        return 2

    try:
        alpha = squircle_alpha(CONTENT_SIZE)
        masters = {
            name: inset_masked_master(load_working_master(path), alpha) for name, path in requested
        }
        if "clear" in masters:
            masters["clear"] = apply_clear_glass_tile(masters["clear"], alpha)
        for name, art in list(masters.items()):
            if name != "clear":
                masters[name] = apply_opaque_shading(art, alpha)
        for name, art in masters.items():
            if name == "clear":
                check_shape_clear(name, art)
            else:
                check_shape(name, art)
    except (ValueError, OSError) as exc:
        print(f"Error: {exc}", file=sys.stderr)
        return 1

    args.icons_dir.mkdir(parents=True, exist_ok=True)
    for name, art in masters.items():
        for size in (512, 128):
            write_png(resize_rgba(art, size), args.icons_dir / f"vtcode-app-{name}-{size}.png")

    if "dynamic" in masters:
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
