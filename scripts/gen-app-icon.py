"""Regenerate the app icon from the source artwork.

The artwork (app-icon-src.png) is a soft light-grey tile, so instead of cutting
it out we square the frame by extending the border pixels (np.pad edge mode),
then round the corners like an app icon. Writes the master icon plus the web
favicon assets. Afterwards run:  npx tauri icon app-icon.png

Usage: from the repo root, with a Python that has Pillow:
    python scripts/gen-app-icon.py
"""

from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "app-icon-src.png"
MASTER = 1024
MARGIN = 0.06  # breathing room around the artwork inside the square tile
RADIUS = 0.215  # corner radius as a fraction of the tile (iOS-ish squircle)


def tile() -> Image.Image:
    art = Image.open(SRC).convert("RGB")
    pad = round(max(art.size) * MARGIN)
    arr = np.asarray(art)
    # extend the flat background outward instead of cropping the artwork
    square = np.pad(arr, ((pad, pad), (pad, pad), (0, 0)), mode="edge")
    img = Image.fromarray(square).resize((MASTER, MASTER), Image.Resampling.LANCZOS)

    r = round(MASTER * RADIUS)
    mask = Image.new("L", (MASTER, MASTER), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, MASTER - 1, MASTER - 1], r, fill=255)
    img.putalpha(mask)
    return img


def main() -> None:
    src = SRC.resolve()
    if ROOT not in src.parents:
        raise SystemExit(f"refusing to read outside the repo: {src}")
    if not src.is_file():
        raise SystemExit(f"missing icon source: put the artwork at {SRC}")
    master = tile()
    master.save(ROOT / "app-icon.png")
    for name, size in [
        ("public/icon-new.png", 256),
        ("public/icon.png", 256),
        ("public/favicon-32.png", 32),
    ]:
        master.resize((size, size), Image.Resampling.LANCZOS).save(ROOT / name)
    master.resize((256, 256), Image.Resampling.LANCZOS).save(
        ROOT / "public/favicon.ico",
        sizes=[
            (16, 16),
            (24, 24),
            (32, 32),
            (48, 48),
            (64, 64),
            (128, 128),
            (256, 256),
        ],
    )
    print("app-icon.png + public favicons written from", SRC)


if __name__ == "__main__":
    main()
