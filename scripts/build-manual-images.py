#!/usr/bin/env python3
"""build-manual-images.py — turn raw Nexus screen captures into manual images.

Deterministic and re-runnable: point it at a directory of captures and it writes
docs/img/manual/*.webp. Re-run it after a recapture and only the changed files move.

WHY WEBP. Measured on a real 1915x1151 capture of the Satellites console:

    format          1920 px wide
    PNG (optimized)     774 KB
    JPEG q88            446 KB      + artifacts on UI text, which is most of the frame
    WEBP q92            218 KB      <- half the source PNG, at full resolution

The site's own hand-optimized screenshots sit at 82-147 KB, but those are marketing
crops; a reference manual needs the readable detail. ~218 KB x ~24 images lands near
5 MB, which roughly doubles the site's public/ and stays far inside every limit.

WHY 1920 WIDE. The capture guidance is a 1920x1080 LOGICAL window, because the app's
auto-zoom is min(w/1200, h/900) capped at 100% -- at 1920x1080 that resolves to exactly
100%, so type renders un-shrunk. Captures from a HiDPI display arrive larger than 1920
and are downscaled here; the extra source detail is what makes the downscale clean.
Captures are never UPSCALED -- a small source stays small rather than being blurred up.
"""

from __future__ import annotations

import argparse
import pathlib
import sys

try:
    from PIL import Image
except ImportError:  # pragma: no cover
    sys.exit("Pillow is required:  pip install --user Pillow")

TARGET_WIDTH = 1920
WEBP_QUALITY = 92

# Capture stem -> published image name. The operator names captures for the section
# they shot; the manual names them for what the image SHOWS, so the mapping lives here
# rather than being imposed on whoever is holding the camera.
NAMES = {
    "phone": "phone-cockpit",
    "cw": "cw-cockpit",
    "ft8": "operate-classic",
    "ft8roster": "operate-roster",
    "sat": "satellites-console",
    "sstv": "sstv",
    "awardsofficial": "awards-official",
    "rtty": "rtty-cockpit",
    "aprs": "aprs",
    "spots": "spots",
    "stats": "stats",
    "memories": "memories",
    "program": "program",
    "connect": "connect",
    "logbook": "logbook",
    "needed": "needed",
    "potasota": "pota-sota",
    "tempo": "tempo",
    "settingsradio": "settings-radio",
    "settingscontesting": "settings-contesting",
    "settingsmodes": "settings-modes",
    "settingsstation": "settings-station",
    "settingsfrequencies": "settings-frequencies",
    "settingsspots": "settings-spots-alerts",
    "settingslogging": "settings-logging-connectors",
    "settingsappearance": "settings-appearance",
}


def slug(name: str) -> str:
    """Normalize a capture filename to a lookup key: 'FS Awards Official.png' -> 'awardsofficial'."""
    stem = pathlib.Path(name).stem.lower()
    if stem.startswith("fs "):
        stem = stem[3:]
    for drop in (" main", "_main", "-main"):
        stem = stem.replace(drop, "")
    return "".join(c for c in stem if c.isalnum())


def convert(src: pathlib.Path, dst: pathlib.Path) -> tuple[int, int, int]:
    im = Image.open(src)
    if im.mode not in ("RGB", "RGBA"):
        im = im.convert("RGB")
    w, h = im.size
    if w > TARGET_WIDTH:  # never upscale
        im = im.resize((TARGET_WIDTH, round(h * TARGET_WIDTH / w)), Image.LANCZOS)
    dst.parent.mkdir(parents=True, exist_ok=True)
    im.save(dst, "WEBP", quality=WEBP_QUALITY, method=6)
    return w, im.size[0], dst.stat().st_size


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("src", type=pathlib.Path, help="directory of raw captures")
    ap.add_argument("--out", type=pathlib.Path,
                    default=pathlib.Path(__file__).resolve().parent.parent / "docs/img/manual",
                    help="output directory (default: docs/img/manual)")
    args = ap.parse_args()

    if not args.src.is_dir():
        sys.exit(f"not a directory: {args.src}")

    captures = sorted(p for p in args.src.iterdir() if p.suffix.lower() in (".png", ".jpg", ".jpeg"))
    if not captures:
        sys.exit(f"no captures found in {args.src}")

    total, unknown = 0, []
    for src in captures:
        key = slug(src.name)
        name = NAMES.get(key)
        if not name:
            unknown.append(f"{src.name}  (key '{key}')")
            continue
        dst = args.out / f"{name}.webp"
        w_in, w_out, size = convert(src, dst)
        total += size
        note = f"{w_in} -> {w_out}px" if w_in != w_out else f"{w_in}px"
        print(f"  {src.name:34s} -> {dst.name:28s} {note:16s} {size/1024:6.0f} KB")

    print(f"\n  {len(captures) - len(unknown)} images, {total/1048576:.1f} MB total")
    if unknown:
        print("\n  NOT CONVERTED — add a NAMES entry for each:")
        for u in unknown:
            print(f"    {u}")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
