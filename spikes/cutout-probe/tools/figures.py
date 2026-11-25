"""Contact sheets of the probe's cutouts, for looking at and for
docs/spikes/product-cutout.md.

    python tools/figures.py CUTOUTS_DIR PHOTOS_DIR DEST_DIR [--cell 300]
        [--models a,b,…] [--quality 80] [--only PHOTO,…] [--name NAME]
        [--zoom]

One sheet per kind of photo (the part of a photo's name before the last
`-`): a row per photo, with the photo and then each model's cutout over
the probe's flat backdrop (`PHOTO.MODEL.jpg`, written by `cutout-probe
cut`). `--only` makes one sheet, DEST_DIR/NAME.jpg, of the photos named.
`--zoom` shows, in place of each whole photo, the square of `--cell`
pixels of it, pixel for pixel, where the last two models' masks differ
most.
"""

import argparse
import pathlib
import tomllib

import numpy as np
from PIL import Image, ImageDraw

HERE = pathlib.Path(__file__).resolve().parent
MODELS = [m["file"][: -len(".onnx")] for m in tomllib.loads((HERE.parent / "models.toml").read_text())["model"]]
LABELS = {
    "u2netp_320x320": "U2-Netp",
    "u2net_320x320": "U2-Net",
    "isnet_general_use_1024x1024": "IS-Net",
    "birefnet_lite_1024x1024": "BiRefNet lite",
    "birefnet_lite_matting_1024x1024": "BiRefNet lite matting",
}
HEAD, GAP = 18, 4


def fit(image, cell):
    image = image.convert("RGB")
    image.thumbnail((cell, cell), Image.LANCZOS)
    tile = Image.new("RGB", (cell, cell), "white")
    tile.paste(image, ((cell - image.width) // 2, (cell - image.height) // 2))
    return tile


def differ_most(photo, cutouts, models, cell):
    """The square of `cell` pixels where the last two models' masks
    differ most."""
    a, b = (
        np.asarray(Image.open(cutouts / f"{photo.stem}.{m}.png").split()[3], dtype=np.float32) / 255
        for m in models[-2:]
    )
    d = np.abs(a - b)
    side = min(cell, *d.shape)
    total = np.pad(d.cumsum(0).cumsum(1), ((1, 0), (1, 0)))
    window = total[side:, side:] - total[:-side, side:] - total[side:, :-side] + total[:-side, :-side]
    y, x = np.unravel_index(window.argmax(), window.shape)
    return (int(x), int(y), int(x) + side, int(y) + side)


def sheet(photos, cutouts, models, cell, zoom=False):
    columns = ["photo"] + [LABELS.get(m, m) for m in models]
    width = len(columns) * (cell + GAP) - GAP
    out = Image.new("RGB", (width, HEAD + len(photos) * (cell + GAP) - GAP), "white")
    draw = ImageDraw.Draw(out)
    for c, label in enumerate(columns):
        draw.text((c * (cell + GAP) + 3, 3), label, fill="black")
    for r, photo in enumerate(photos):
        y = HEAD + r * (cell + GAP)
        box = differ_most(photo, cutouts, models, cell) if zoom else None
        out.paste(fit(Image.open(photo).crop(box), cell), (0, y))
        for c, model in enumerate(models, start=1):
            cut = cutouts / f"{photo.stem}.{model}.jpg"
            if cut.exists():
                out.paste(fit(Image.open(cut).crop(box), cell), (c * (cell + GAP), y))
    return out


def main():
    p = argparse.ArgumentParser()
    p.add_argument("cutouts", type=pathlib.Path)
    p.add_argument("photos", type=pathlib.Path)
    p.add_argument("dest", type=pathlib.Path)
    p.add_argument("--cell", type=int, default=300)
    p.add_argument("--models", default=",".join(MODELS))
    p.add_argument("--quality", type=int, default=80)
    p.add_argument("--only")
    p.add_argument("--name", default="sheet")
    p.add_argument("--zoom", action="store_true")
    a = p.parse_args()
    a.dest.mkdir(parents=True, exist_ok=True)
    kinds = {}
    if a.only:
        kinds[a.name] = [a.photos / f"{stem}.jpg" for stem in a.only.split(",")]
    else:
        for photo in sorted(a.photos.glob("*.jpg")):
            kinds.setdefault(photo.stem.rsplit("-", 1)[0], []).append(photo)
    for kind, photos in kinds.items():
        out = a.dest / f"{kind}.jpg"
        sheet(photos, a.cutouts, a.models.split(","), a.cell, a.zoom).save(out, quality=a.quality, optimize=True)
        print(out, out.stat().st_size // 1024, "KB")


if __name__ == "__main__":
    main()
