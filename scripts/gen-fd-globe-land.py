#!/usr/bin/env python3
"""Regenerate the fd_scoreboard globe coastline blob.

Reads ui/node_modules/world-atlas/land-110m.json (Natural Earth 1:110m land,
PUBLIC DOMAIN data; the npm packaging is ISC/Bostock), decodes the TopoJSON,
Douglas-Peucker simplifies at 0.6 deg, drops rings whose bbox diagonal is
< 1.0 deg, quantizes to 0.1 deg and delta-codes.

Output charset is ONLY  0-9 , - |   -- safe inside a single-quoted JS string,
and contains no "http://" / "https://" (the zero-external-URL test).
Deterministic: same input -> same 8812-byte string.
"""
import json, math, os

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(REPO, 'ui', 'node_modules', 'world-atlas', 'land-110m.json')
EPS = 0.6      # Douglas-Peucker tolerance, degrees
MIN_DIAG = 1.0 # drop rings smaller than this bbox diagonal, degrees
Q = 10         # quantization: 1/Q degree == 0.1 deg

d = json.load(open(SRC))
sc, tr = d['transform']['scale'], d['transform']['translate']

def decode(arc):
    x = y = 0; out = []
    for dx, dy in arc:
        x += dx; y += dy
        out.append((x * sc[0] + tr[0], y * sc[1] + tr[1]))
    return out

arcs = [decode(a) for a in d['arcs']]

def stitch(idxs):
    pts = []
    for i in idxs:
        a = arcs[~i][::-1] if i < 0 else arcs[i]
        pts.extend(a[1:] if pts else a)
    return pts

rings = []
for g in d['objects']['land']['geometries']:
    polys = [g['arcs']] if g['type'] == 'Polygon' else g['arcs']
    for poly in polys:
        for r in poly:
            rings.append(stitch(r))

def bbox_diag(r):
    xs = [p[0] for p in r]; ys = [p[1] for p in r]
    return math.hypot(max(xs) - min(xs), max(ys) - min(ys))

def rdp(pts, eps):
    if len(pts) < 3: return pts
    keep = [False] * len(pts); keep[0] = keep[-1] = True
    stack = [(0, len(pts) - 1)]
    while stack:
        i, j = stack.pop()
        if j <= i + 1: continue
        x1, y1 = pts[i]; x2, y2 = pts[j]
        dx, dy = x2 - x1, y2 - y1
        den = math.hypot(dx, dy)
        best = -1.0; bi = -1
        for k in range(i + 1, j):
            x0, y0 = pts[k]
            dist = math.hypot(x0 - x1, y0 - y1) if den == 0 else \
                   abs(dy * x0 - dx * y0 + x2 * y1 - y2 * x1) / den
            if dist > best: best, bi = dist, k
        if best > eps:
            keep[bi] = True
            stack.append((i, bi)); stack.append((bi, j))
    return [p for p, k in zip(pts, keep) if k]

out = []
for r in rings:
    if bbox_diag(r) < MIN_DIAG: continue
    s = rdp(r, EPS)
    if len(s) >= 4: out.append(s)

parts = []
for r in out:
    nums = []; px = py = 0
    for x, y in r:
        xi = round(x * Q); yi = round(y * Q)
        nums.append(str(xi - px)); nums.append(str(yi - py))
        px, py = xi, yi
    parts.append(','.join(nums))
blob = '|'.join(parts)

assert 'http' not in blob and "'" not in blob and '\\' not in blob
print(f'rings={len(out)} points={sum(len(r) for r in out)} bytes={len(blob)}')
# Paste the contents into `var LAND_RAW = '...'` in
# crates/tempo-app/assets/fd_scoreboard.html. Written to a file rather than stdout so the
# stats line above stays readable.
out_path = os.path.join(REPO, 'land110_delta.txt')
open(out_path, 'w').write(blob)
print('wrote', os.path.normpath(out_path))
