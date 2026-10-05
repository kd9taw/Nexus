#!/usr/bin/env python3
"""Regenerate the 3-D globes' day and night pictures, ui/src/assets/earth-day.webp and earth-night.webp.

SOURCES: two NASA Earth Observatory maps, works of the US government and PUBLIC DOMAIN (NASA asks for
the credit lines given in NOTICE). Each download is checked against the SHA-256 below before it is read.
- DAY: Blue Marble: Next Generation with topography and bathymetry, July 2004, a 5400 x 2700 JPEG:
  https://eoimages.gsfc.nasa.gov/images/imagerecords/73000/73751/world.topo.bathy.200407.3x5400x2700.jpg
- NIGHT: Earth at Night 2016 (Black Marble), the colour map at 3 km, a 13500 x 6750 JPEG:
  https://eoimages.gsfc.nasa.gov/images/imagerecords/144000/144898/BlackMarble_2016_3km.jpg

WHAT THEY BECOME: two 4096 x 2048 WebPs, equirectangular like the sources, that both globes wear
(ui/src/features/globeBasemap.ts): the day picture as the sphere's own colour, lit by the sun, and the
night picture as its glow, shown only where the sun has set and faded in across the terminator. 4096 px
is about 11 px per degree, and the most a globe texture should ask of a modest GPU (32 MB decoded).
The flat map does not use them: it keeps its painted map and shaded relief (scripts/gen-relief.py).

Run:  python3 scripts/gen-globe-textures.py [--cache DIR]      (needs Pillow with WebP; deterministic)
"""
import hashlib
import io
import os
import sys
import tempfile
import urllib.request

from PIL import Image

SOURCES = [
    ('https://eoimages.gsfc.nasa.gov/images/imagerecords/73000/73751/world.topo.bathy.200407.3x5400x2700.jpg',
     '4f4240673a3a1b173d61b92ca4b07bac5fd17059ea5f725ba6da5a9c5386b7ba', 'earth-day.webp'),
    ('https://eoimages.gsfc.nasa.gov/images/imagerecords/144000/144898/BlackMarble_2016_3km.jpg',
     '230aac448ae68c358be433dd518888cccb3a85ccf66f7b44326441c324ad6725', 'earth-night.webp'),
]
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ASSETS = os.path.join(REPO, 'ui', 'src', 'assets')
SIZE = (4096, 2048)
QUALITY = 80

cache = sys.argv[sys.argv.index('--cache') + 1] if '--cache' in sys.argv else os.path.join(tempfile.gettempdir(), 'nasa-globe-imagery')
os.makedirs(cache, exist_ok=True)
Image.MAX_IMAGE_PIXELS = None  # the night map is 91 megapixels
for url, sha256, name in SOURCES:
    path = os.path.join(cache, url.rsplit('/', 1)[1])
    if not os.path.exists(path):
        urllib.request.urlretrieve(url, path)
    with open(path, 'rb') as f:
        data = f.read()
    got = hashlib.sha256(data).hexdigest()
    if got != sha256:
        sys.exit(f'{path}: SHA-256 {got}, expected {sha256}')
    src = Image.open(io.BytesIO(data)).convert('RGB')
    out = os.path.join(ASSETS, name)
    src.resize(SIZE, Image.LANCZOS).save(out, 'WEBP', quality=QUALITY, method=6)
    print(f'{url.rsplit("/", 1)[1]} ({src.width}x{src.height}) -> {out}: {SIZE[0]}x{SIZE[1]}, {os.path.getsize(out)} bytes')
