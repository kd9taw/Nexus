#!/usr/bin/env python3
"""Regenerate ui/src/assets/earth-relief.webp, the map's shaded relief.

SOURCE: Natural Earth "Shaded Relief, high resolution" (SR_HR, version 2.0.0, PUBLIC DOMAIN):
https://naciscdn.org/naturalearth/10m/raster/SR_HR.zip -- a 21600 x 10800 greyscale hillshade of the
whole world, equirectangular, one arc-minute per pixel. The download is checked against the SHA-256
below before it is read.

WHAT IT BECOMES: a 4096 x 2048 greyscale WebP that the map lays over its own land colour (the
theme's --map-land) with the canvas 'hard-light' blend, on the flat map and on the 3-D globe's
texture (ui/src/basemap.ts paintRelief). Natural Earth paints flat ground and all water one grey
(206); that grey is moved to the blend's neutral, 128, so the sea and the plains keep the theme's
colours exactly and only slopes are shaded: darker away from the light, a little lighter towards
it. 4096 px is about 11 px per degree -- sharp on a full-window world map, and the most a 3-D
globe texture should ask of a modest GPU (32 MB decoded).

Run:  python3 scripts/gen-relief.py [--cache DIR]      (needs Pillow with WebP; deterministic)
"""
import hashlib
import io
import os
import sys
import tempfile
import urllib.request
import zipfile

from PIL import Image

URL = 'https://naciscdn.org/naturalearth/10m/raster/SR_HR.zip'
SHA256 = 'b2619fff2fc73c17152983c066adfaa25c4b626916b822bad2fae8bcd9be41a5'
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(REPO, 'ui', 'src', 'assets', 'earth-relief.webp')
SIZE = (4096, 2048)
FLAT = 206      # Natural Earth's grey for flat ground and water
NEUTRAL = 128   # the hard-light blend's no-op
GAIN = 0.75     # the deepest shadow (36) lands near 0; the brightest slope (255) near 165
QUALITY = 85

cache = sys.argv[sys.argv.index('--cache') + 1] if '--cache' in sys.argv else os.path.join(tempfile.gettempdir(), 'natural-earth-raster')
os.makedirs(cache, exist_ok=True)
zpath = os.path.join(cache, 'SR_HR.zip')
if not os.path.exists(zpath):
    urllib.request.urlretrieve(URL, zpath)
with open(zpath, 'rb') as f:
    data = f.read()
got = hashlib.sha256(data).hexdigest()
if got != SHA256:
    sys.exit(f'SR_HR.zip: SHA-256 {got}, expected {SHA256}')

Image.MAX_IMAGE_PIXELS = None
with zipfile.ZipFile(io.BytesIO(data)) as z:
    version = z.read('SR_HR.VERSION.txt').decode().strip()
    src = Image.open(io.BytesIO(z.read('SR_HR.tif')))
    src.load()
if src.mode != 'L':
    sys.exit(f'SR_HR.tif: expected greyscale, got {src.mode}')

small = src.resize(SIZE, Image.LANCZOS)
lut = [max(0, min(255, round(NEUTRAL + (v - FLAT) * GAIN))) for v in range(256)]
shade = small.point(lut)
shade.save(OUT, 'WEBP', quality=QUALITY, method=6)
print(f'SR_HR {version} -> {OUT}: {SIZE[0]}x{SIZE[1]}, {os.path.getsize(OUT)} bytes')
