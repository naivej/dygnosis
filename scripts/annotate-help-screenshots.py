#!/usr/bin/env python3
"""Crop captured VS Code screenshots and add numbered callout badges."""

from __future__ import annotations

import json
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1] / "help" / "assets"
ORIGINALS = ROOT / "originals"
CROP_RIGHT = 300  # Remove unrelated host chrome (Cursor chat panel).

SCENES = {
    "help-panel": {
        "crop": (0, 0, 980, 1024),
        "callouts": [(150, 130), (700, 95), (430, 280), (860, 95)],
    },
    "get-started": {
        "crop": (0, 0, 980, 1024),
        "callouts": [(150, 520), (150, 780), (150, 900)],
    },
    "model-overview": {
        "crop": (0, 0, 980, 1024),
        "callouts": [(150, 520), (520, 250), (430, 990)],
    },
    "appearance": {
        "crop": (0, 0, 980, 720),
        "callouts": [(520, 180), (520, 320), (520, 450)],
    },
    "edit-assistance": {
        "crop": (0, 0, 980, 720),
        "callouts": [(520, 120), (520, 260)],
    },
    "diagnostics": {
        "crop": (0, 0, 980, 1024),
        "callouts": [(520, 180), (520, 760)],
    },
    "navigate-code": {
        "crop": (0, 0, 980, 1024),
        "callouts": [(150, 520), (520, 260)],
    },
    "effective-model": {
        "crop": (0, 0, 980, 1024),
        "callouts": [(300, 260), (700, 260), (520, 990)],
    },
    "structural-diff": {
        "crop": (0, 0, 980, 1024),
        "callouts": [(700, 180), (700, 320), (700, 520)],
    },
    "project-checks": {
        "crop": (0, 0, 980, 1024),
        "callouts": [(150, 780), (430, 990)],
    },
    "agents-mcp": {
        "crop": (0, 0, 980, 1024),
        "callouts": [(150, 780), (150, 520), (430, 990)],
    },
    "troubleshoot": {
        "crop": (0, 0, 980, 1024),
        "callouts": [(520, 760), (430, 990)],
    },
}


def badge(draw: ImageDraw.ImageDraw, center: tuple[int, int], number: int, font) -> None:
    x, y = center
    radius = 16
    draw.ellipse((x - radius, y - radius, x + radius, y + radius), fill="#0078D4", outline="#FFFFFF", width=2)
    text = str(number)
    box = draw.textbbox((0, 0), text, font=font)
    tw, th = box[2] - box[0], box[3] - box[1]
    draw.text((x - tw / 2, y - th / 2 - 1), text, fill="#FFFFFF", font=font)


def annotate(name: str, spec: dict) -> None:
    source = ORIGINALS / f"{name}.png"
    image = Image.open(source).convert("RGBA")
    crop = spec["crop"]
    image = image.crop(crop)
    draw = ImageDraw.Draw(image)
    try:
        font = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", 18)
    except OSError:
        font = ImageFont.load_default()
    for index, point in enumerate(spec["callouts"], start=1):
        badge(draw, point, index, font)
    target = ROOT / f"{name}.png"
    image.convert("RGB").save(target, optimize=True)


def main() -> None:
    for name, spec in SCENES.items():
        annotate(name, spec)
    manifest = json.loads((ROOT / "manifest.json").read_text())
    manifest["annotated"] = list(SCENES.keys())
    (ROOT / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")


if __name__ == "__main__":
    main()
