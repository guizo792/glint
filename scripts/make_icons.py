"""Draws the Glint icon (a four-point spark) into app/icons. Needs Pillow."""
from pathlib import Path
from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parent.parent / "app" / "icons"
SCALE = 4


def draw(size: int) -> Image.Image:
    s = size * SCALE
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([0, 0, s - 1, s - 1], radius=s * 0.22, fill=(29, 31, 44, 255))
    c = s / 2
    outer, inner = s * 0.38, s * 0.07
    points = [
        (c, c - outer), (c + inner, c - inner),
        (c + outer, c), (c + inner, c + inner),
        (c, c + outer), (c - inner, c + inner),
        (c - outer, c), (c - inner, c - inner),
    ]
    d.polygon(points, fill=(255, 205, 96, 255))
    return img.resize((size, size), Image.LANCZOS)


OUT.mkdir(parents=True, exist_ok=True)
for size, name in [(32, "32x32.png"), (128, "128x128.png"), (256, "128x128@2x.png"), (512, "icon.png")]:
    draw(size).save(OUT / name)
draw(256).save(OUT / "icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
