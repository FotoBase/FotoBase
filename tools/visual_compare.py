#!/usr/bin/env python3
"""Per-channel RMS delta between two screenshots (visual-comparison protocol).

Usage: python3 tools/visual_compare.py a.png b.png
Requires Pillow. Exit 0 with the RMS value printed; non-zero on I/O errors.
"""

from __future__ import annotations

import sys
from pathlib import Path

from PIL import Image


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__, file=sys.stderr)
        return 2
    a = Image.open(Path(sys.argv[1])).convert("RGB")
    b = Image.open(Path(sys.argv[2])).convert("RGB")
    if a.size != b.size:
        # Crop both to the common area so window chrome differences don't dominate.
        w, h = min(a.width, b.width), min(a.height, b.height)
        a, b = a.crop((0, 0, w, h)), b.crop((0, 0, w, h))
        print(f"note: sizes differed, cropped to common {w}x{h}", file=sys.stderr)
    pa, pb = a.tobytes(), b.tobytes()
    sum_sq = sum((x - y) * (x - y) for x, y in zip(pa, pb))
    rms = (sum_sq / len(pa)) ** 0.5
    print(f"RMS delta: {rms:.3f} (0-255 scale) over {len(pa)} channels")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
