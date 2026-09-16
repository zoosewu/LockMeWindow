#!/usr/bin/env python3
"""Draws the LockMeWindow icons.

The app icon is a rounded tile with a cursor inside corner brackets; the tray
icons are separate pixel-aligned glyphs on a transparent background, one for a
light taskbar and one for a dark one.

Usage: python3 tools/make-icons.py [output-dir]
"""

import sys
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

SS = 8  # supersampling factor for the drawn (non pixel-aligned) sizes

TILE_TOP = (74, 132, 240)
TILE_BOTTOM = (39, 31, 99)
TILE_BLOOM = (126, 96, 235)
TILE_FLAT = (58, 92, 208)
BRACKET = (125, 214, 255)
ARROW = (255, 255, 255)
SHADOW = (8, 12, 40)

ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]

# Classic arrow outline in a unit box, tip at the top left.
ARROW_SHAPE = [
    (0.00, 0.00),
    (0.00, 0.72),
    (0.19, 0.56),
    (0.31, 0.86),
    (0.46, 0.80),
    (0.34, 0.51),
    (0.56, 0.50),
]

# 16x16 tray glyph: 1px corner ticks and a solid arrow, drawn pixel by pixel.
TRAY_ARROW = [
    "#......",
    "##.....",
    "###....",
    "####...",
    "#####..",
    "######.",
    "#######",
    "####...",
    "#..##..",
    "....##.",
]


# The arrow's mass sits toward its top left, so the pixel-drawn sizes are
# centred on its centre of gravity instead of its bounding box.
ARROW_BALANCE = (2.1, 4.9)


def arrow_origin(size):
    return tuple(round((size - 1) / 2 - weight) for weight in ARROW_BALANCE)


def arrow_polygon(height, origin):
    x, y = origin
    return [(x + px * height, y + py * height) for px, py in ARROW_SHAPE]


def gradient(size):
    """Diagonal blue ramp lifted by a violet bloom in the lower left."""
    image = Image.new("RGB", (size, size))
    pixels = image.load()
    span = 2 * (size - 1)
    reach = size * 0.72
    for y in range(size):
        for x in range(size):
            t = (x + y) / span
            base = [
                top + (bottom - top) * t
                for top, bottom in zip(TILE_TOP, TILE_BOTTOM)
            ]
            dx, dy = x - size * 0.10, y - size * 0.98
            distance = (dx * dx + dy * dy) ** 0.5
            bloom = max(0.0, 1 - distance / reach) ** 2 * 0.40
            pixels[x, y] = tuple(
                round(value + (glow - value) * bloom)
                for value, glow in zip(base, TILE_BLOOM)
            )
    return image


def tile_mask(size, radius):
    mask = Image.new("L", (size * SS, size * SS), 0)
    draw = ImageDraw.Draw(mask)
    inset = size * SS * 0.03
    draw.rounded_rectangle(
        (inset, inset, size * SS - inset - 1, size * SS - inset - 1),
        radius=radius * SS,
        fill=255,
    )
    return mask.resize((size, size), Image.LANCZOS)


def corner_brackets(canvas):
    """Four L shapes cut from a rounded frame, so their corners echo the tile."""
    inset = canvas * 0.155
    radius = canvas * 0.11
    thickness = round(canvas * 0.05)
    reach = radius + canvas * 0.115
    near, far = inset, canvas - inset

    frame = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    ImageDraw.Draw(frame).rounded_rectangle(
        (near, near, far, far), radius=radius, outline=BRACKET, width=thickness
    )

    corners = Image.new("L", (canvas, canvas), 0)
    draw = ImageDraw.Draw(corners)
    for left, top in (
        (near, near),
        (far - reach, near),
        (near, far - reach),
        (far - reach, far - reach),
    ):
        draw.rectangle(
            (
                left - thickness,
                top - thickness,
                left + reach + thickness,
                top + reach + thickness,
            ),
            fill=255,
        )
    # Trim the strokes back to the corners; the straight runs stop square.
    draw.rectangle((near + reach, 0, far - reach, canvas), fill=0)
    draw.rectangle((0, near + reach, canvas, far - reach), fill=0)

    frame.putalpha(ImageChops.multiply(frame.getchannel("A"), corners))
    return frame


def cursor_layer(canvas):
    layer = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    ImageDraw.Draw(layer).polygon(
        arrow_polygon(canvas * 0.46, (canvas * 0.40, canvas * 0.30)), fill=ARROW
    )
    return layer


def draw_app_icon(size):
    """Rounded tile, corner brackets, and a cursor floating above them."""
    mask = tile_mask(size, size * 0.22)
    tile = gradient(size)
    tile.putalpha(mask)

    # A soft highlight across the upper half gives the tile some depth.
    highlight = Image.new("L", (size, size), 0)
    pixels = highlight.load()
    for y in range(size):
        value = max(0, 1 - y / (size * 0.55))
        for x in range(size):
            pixels[x, y] = round(38 * value)
    tile.alpha_composite(
        Image.merge("RGBA", (*[Image.new("L", (size, size), 255)] * 3, highlight))
        .convert("RGBA"),
    )
    tile.putalpha(mask)

    canvas = size * SS
    if size >= 32:
        tile.alpha_composite(corner_brackets(canvas).resize((size, size), Image.LANCZOS))

    cursor = cursor_layer(canvas).resize((size, size), Image.LANCZOS)

    # Two shadows under the cursor alone: a tight one to seat it, a wide one to
    # lift it off the brackets.
    if size >= 40:
        silhouette = cursor.getchannel("A")
        for blur, drop, opacity in (
            (size * 0.016, size * 0.012, 150),
            (size * 0.055, size * 0.040, 95),
        ):
            shadow = Image.new("RGBA", (size, size), (0, 0, 0, 0))
            shadow.paste((*SHADOW, opacity), (0, 0), silhouette)
            tile.alpha_composite(
                shadow.filter(ImageFilter.GaussianBlur(blur)), (0, round(drop))
            )

    tile.alpha_composite(cursor)
    return tile


def draw_small_app_icon(size):
    """16 and 20 pixel icons: tile plus cursor, no brackets, no gradient."""
    image = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    radius = 3 if size >= 20 else 2
    draw.rounded_rectangle((0, 0, size - 1, size - 1), radius=radius, fill=TILE_FLAT)

    left, top = arrow_origin(size)
    for y, row in enumerate(TRAY_ARROW):
        for x, cell in enumerate(row):
            if cell == "#":
                draw.point((left + x, top + y), fill=ARROW)
    return image


def draw_tray_icon(size, ink):
    """Pixel-aligned tray glyph: corner ticks plus cursor, transparent behind."""
    scale = size // 16
    image = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)

    def plot(x, y, width=1, height=1):
        draw.rectangle(
            (
                x * scale,
                y * scale,
                (x + width) * scale - 1,
                (y + height) * scale - 1,
            ),
            fill=ink,
        )

    for x, y, dx, dy in ((1, 1, 1, 1), (14, 1, -1, 1), (1, 14, 1, -1), (14, 14, -1, -1)):
        plot(min(x, x + dx * 2), y, 3, 1)
        plot(x, min(y, y + dy * 2), 1, 3)

    left, top = arrow_origin(16)
    for row, cells in enumerate(TRAY_ARROW):
        for column, cell in enumerate(cells):
            if cell == "#":
                plot(left + column, top + row)
    return image


def app_icon(size):
    return draw_small_app_icon(size) if size < 24 else draw_app_icon(size)


def main():
    out = Path(sys.argv[1] if len(sys.argv) > 1 else "assets")
    out.mkdir(parents=True, exist_ok=True)

    icons = {size: app_icon(size) for size in ICO_SIZES}
    icons[256].save(out / "lock-me-window.ico", sizes=[(s, s) for s in ICO_SIZES],
                    append_images=[icons[s] for s in ICO_SIZES if s != 256])
    draw_app_icon(512).save(out / "lock-me-window.png")

    for name, ink in (("light", (26, 26, 26, 255)), ("dark", (255, 255, 255, 255))):
        for size in (16, 32):
            draw_tray_icon(size, ink).save(out / f"tray-{name}-{size}.png")

    print(f"wrote icons to {out}")


if __name__ == "__main__":
    main()
