#!/usr/bin/env python3
"""Fotobase test-fixture generator (PLAN §9, task 00-013).

Generates a deterministic photo library tree for perf acceptance tests.
Never commit the data — commit this generator only.

Usage:
    python3 tools/fixture_gen.py --count 50000 --seed 42 --out /tmp/lib
    python3 tools/fixture_gen.py --count 1000  --seed 42 --out /tmp/lib-smoke

Determinism: same (seed, count) => identical file list, names, dates, bytes.
Requires: Pillow (JPEG/PNG/WebP/TIFF). HEIC files are duplicated from the
tiny pre-made samples committed under tools/fixture-samples/ (PLAN §11 Q8).
"""

from __future__ import annotations

import argparse
import hashlib
import random
import shutil
import sys
from datetime import datetime, timedelta
from pathlib import Path

try:
    from PIL import Image, ImageOps
except ImportError:  # pragma: no cover
    sys.exit("Pillow is required: pip install Pillow")

SAMPLES_DIR = Path(__file__).parent / "fixture-samples"
# TASK-01-012: one entry per supported format (JPEG weighted like real
# libraries). AVIF needs the optional pillow-avif-plugin at generation time;
# the format is dropped (with a warning) when the plugin is missing.
FORMATS = ["jpeg", "jpeg", "jpeg", "png", "webp", "tiff", "heic", "gif", "bmp", "avif"]
EXTENSIONS = {
    "jpeg": "jpg", "png": "png", "webp": "webp", "tiff": "tif", "heic": "heic",
    "gif": "gif", "bmp": "bmp", "avif": "avif",
}
SAVE_FORMATS = {
    "jpeg": "JPEG", "png": "PNG", "webp": "WEBP", "tiff": "TIFF",
    "gif": "GIF", "bmp": "BMP", "avif": "AVIF",
}
SIZES = [(160, 120), (640, 480), (1280, 960), (2048, 1536)]  # sprinkle thumbnail..full-res
START = datetime(2005, 1, 1, 0, 0, 0)
DATE_SPAN_DAYS = 20 * 365  # spread over ~20 years
MAX_DEPTH = 4  # year/month + up to 2 event-subfolder levels


def make_image(rng: random.Random) -> Image.Image:
    """Deterministic photo-like gradient image (C-speed PIL ops — must generate
    50k files in minutes per PLAN §9, so no Python per-pixel loops)."""
    w, h = rng.choice(SIZES)
    grad = Image.linear_gradient("L").resize((w, h))  # vertical 0..255 ramp
    c1 = (rng.randrange(256), rng.randrange(256), rng.randrange(256))
    c2 = (rng.randrange(256), rng.randrange(256), rng.randrange(256))
    return ImageOps.colorize(grad, black=c1, white=c2)


def gen(count: int, seed: int, out: Path) -> None:
    rng = random.Random(seed)
    out.mkdir(parents=True, exist_ok=True)
    manifest_lines: list[str] = []
    heic_samples = sorted(SAMPLES_DIR.glob("*.heic"))
    if not heic_samples:
        sys.exit(f"No HEIC samples in {SAMPLES_DIR} (see tools/fixture-samples/README.md)")
    avif_samples = sorted(SAMPLES_DIR.glob("*.avif"))
    formats = list(FORMATS)
    if not avif_samples:
        # AVIF sample generation needs pillow-avif-plugin (not a hard dep);
        # without a committed sample the format cannot be duplicated.
        formats = [f for f in formats if f != "avif"]
        print("warning: no AVIF samples — AVIF dropped from fixture mix", file=sys.stderr)

    for i in range(count):
        fmt = rng.choice(formats)
        taken = START + timedelta(
            days=rng.randrange(DATE_SPAN_DAYS),
            seconds=rng.randrange(86400),
        )
        rel = [f"{taken.year:04d}", f"{taken.month:02d}"]
        for _ in range(rng.randrange(MAX_DEPTH - 2)):  # 0..2 event subfolders
            rel.append(f"event-{rng.randrange(100):02d}")
        name = (
            f"IMG_{taken.strftime('%Y%m%d')}_{rng.randrange(1_000_000):06d}."
            f"{EXTENSIONS[fmt]}"
        )
        dest = out.joinpath(*rel, name)
        dest.parent.mkdir(parents=True, exist_ok=True)

        if fmt == "heic":
            src = heic_samples[rng.randrange(len(heic_samples))]
            shutil.copyfile(src, dest)
        elif fmt == "avif":
            src = avif_samples[rng.randrange(len(avif_samples))]
            shutil.copyfile(src, dest)
        else:
            img = make_image(rng)
            save_kw = {"jpeg": {"quality": 85}}.get(fmt, {})
            img.save(dest, format=SAVE_FORMATS[fmt], **save_kw)

        digest = hashlib.sha256(dest.read_bytes()).hexdigest()
        manifest_lines.append(f"{digest}  {dest.relative_to(out).as_posix()}")

    manifest = "\n".join(sorted(manifest_lines)) + "\n"
    (out / "fixture-manifest.sha256").write_text(manifest, encoding="utf-8")
    print(f"Generated {count} files under {out} (seed={seed})")
    print(f"Manifest: {out / 'fixture-manifest.sha256'}")


def main() -> None:
    p = argparse.ArgumentParser(description="Deterministic Fotobase fixture generator")
    p.add_argument("--count", type=int, required=True)
    p.add_argument("--seed", type=int, default=42)
    p.add_argument("--out", type=Path, required=True)
    args = p.parse_args()
    gen(args.count, args.seed, args.out)


if __name__ == "__main__":
    main()
