# Spectrum harness — the scope and waterfall in a real browser

jsdom never lays out and never has a graphics context, so every unit gate in `ui/` stays green
through a broken waterfall paint path. This harness mounts the **shipped** `PhoneScope` (Phone and
CW) and `Waterfall` (FT, JS8, RTTY, PSK, SSTV) in headless Chrome, unmodified, with the desktop IPC
bridge (`window.__TAURI_INTERNALS__.invoke`) stood in by synthetic frames and the real clock left
alone — a virtual-time budget starves a `requestAnimationFrame` loop, which both components are.

```
npm --prefix ui ci            # once
node ui/spectrum-harness/run.mjs                 # everything, about 3 minutes
node ui/spectrum-harness/run.mjs --only cadence  # one probe (the backend check always runs)
```

Exit `0` everything as expected, `1` something is not, `2` it could not run. Output: a line per
check, and `results.json` plus every rendered picture (and a diff for a failed one) under `--out`
(default `$TMPDIR/nexus-spectrum-harness`). It is the `spectrum-harness` job in CI.

## What it measures

| Probe | What | Asserted? |
|---|---|---|
| backend | Chrome is pinned to software rasterisation and SwiftShader WebGL (`PINNED_FLAGS` in `run.mjs`) and the run checks it got exactly that. No GPU is needed. | yes |
| pixel | four fixtures (`frames.ts`: a carrier, a two-tone, a noise-floor step, an FT8 period) through each component, against `baselines/*.png` within `compare.mjs`'s tolerance. Rows are served by call, so the picture does not depend on timing. The Waterfall draws through the spectrum renderer, so its run on **both** backends (the renderer's backend asserted each time), against its one stored picture, which canvas-2D reproduces exactly. | yes |
| cadence | sources on the real clock at their producers' rates — audio 50/s, CI-V 3/s and 10/s, Flex 15/s, FT-710 84/s — each sweep carrying a barcode of its own number. The canvas is read back: rows committed, distinct sweeps shown, and **repeats** (a committed row showing the same sweep as the row before it). The Waterfall's on both backends. | repeats = 0 |
| perf | each component filling a 1024×768 and a 3440×1440 window: frame pacing, long animation frames, main-thread time per frame, the cost of one committed row (the Waterfall's, which draws on the next frame, is its fetch-to-commit work only); the Waterfall on both backends | measured only |
| ipc | a `Spectrum` row parsed from its JSON on every frame at 60 Hz, 512 and 2048 bins, beside the same values taken from a binary buffer | measured only |
| render | the renderer core (`ui/src/spectrum`, mounted bare: no component uses it yet) on **both** backends, WebGL2 and canvas-2D: nine fixtures (`renderer.ts`) against `baselines/renderer/<backend>-<fixture>.png`; the two backends against each other on the waterfall band, where both run the same per-pixel mapping (and on a zoomed 23 cm scope span reached from audio, where canvas-2D's float64 is the reference for WebGL2's float32 hertz); and the 3-D stack against a one-row burst | yes. The whole-picture difference between backends (the trace line and the 3-D stack are rasterised differently) is printed |
| capability | which backend the renderer picks: WebGL2 on a healthy context, canvas-2D (with the reason) when there is no context or when the context takes float uploads and keeps nothing | yes |
| loss | a forced WebGL2 context loss (`WEBGL_lose_context`): canvas-2D must stand in at once from the same history, and WebGL2 must come back without a reload and draw exactly what a renderer that never lost its context draws | yes |
| rperf | the renderer at 2048 bins × 2048 rows filling a 1024×768 window, one new row and one redraw per frame, on each backend, flat and 3-D: the renderer's own main-thread time, the main-thread task time, and draw-to-pixels (plus the GPU timer query where the context has one), beside the renderer's budget (under 2 ms of GPU and 1 ms of main thread a frame) | measured only |
| axis | the scale's axis (`ui/src/spectrum/scale.ts`) against the picture: a −20 dBFS level, encoded as the producer encodes it and drawn by each backend in the range `scaleRange.ts` gives it, must sit on the axis's −20 dBFS tick (the line's centre within a pixel) | yes |
| offsets | what a click on the Waterfall sets: **real** mouse events (DevTools `Input.dispatchMouseEvent`, so the browser's own hit-testing picks the element), every gesture the waterfall answers plus the middle button it refuses, 13 points across the canvas, in the picture and on the axis strip, for every view the picker offers and WSPR's fixed sub-band, with the RX marker walked so a zoomed window holds and then pages, on both backends. Every click must reach the waterfall canvas, set exactly the hertz and target in `baselines/waterfall-offsets.json` (recorded from the waterfall before it drew through the renderer, `recordedFrom`), and cancel the default where that did. The TX offset is where an FT over is keyed. | yes, exactly |

Timings come from `performance.now()` on a cross-origin-isolated page (5 µs, not 100 µs).

## Controls — run every time

An instrument that cannot fail proves nothing, so controls are part of every run and the run is red
if any comes back clean: the carrier fixture rendered in a **wrong palette** must fail the pixel
comparison (the components' fixture, and the renderer's on each backend); a **planted extra row**
(one ask answered with the previous sweep again) must be found by the cadence probe, exactly once;
a **broken context** (none at all, or one that drops float uploads) must fail the renderer's
self-test and leave it on canvas-2D; the loss check run with the renderer's **restore handler
dropped** (the page swallows every `webglcontextrestored` listener) must fail; the axis's −20 dBFS
tick on an axis pinned to a **wrong reference** (a −100 dBFS floor) must miss the drawn line; the same
clicks **2 px off** must not set the recorded offsets; and the burst the 3-D
stack rejects must show on the 2-D waterfall, or that check proves nothing. The long-frame observer gets its own: an 80 ms frame
is planted during warm-up and must be seen, or long frames are reported as unmeasured rather than
as zero. `--palette NAME` and `--plant N` apply the same controls to every check by hand.

## Known failures

A check can be marked as an expected red. It keeps the run green while it fails, and turns the run
red the day it passes, so the marker cannot outlive the defect. Today's:

- **Repeated rows** (`civ-3`, `civ-10`, `flex-15` on `PhoneScope`). The scope commits a waterfall row
  on every 50 ms poll, new sweep or not, so any source slower than ~20 sweeps a second scrolls copies
  of its last sweep: at 3 sweeps a second about five of every six rows are repeats. It flips green
  when the scope commits a row only when the source's frame counter advances; delete the marker in
  `CADENCE` (`run.mjs`) in that same change.

## Re-recording the pictures

`--record` renders every fixture twice and writes `baselines/*.png` and `baselines/backend.json`
only if the two renders are identical. It records the probes `--only` selects, so
`--only render --record` re-records the renderer's pictures (`baselines/renderer/`) and leaves the
components' alone. Re-record only for a deliberate change to what the operator
sees, and say what changed in the commit. The pictures are keyed on the backend; a run under any
other backend refuses to compare rather than reporting a difference.

`--only offsets --record` re-records `baselines/waterfall-offsets.json`, and **must not** be used to
make a change pass: those numbers are where the station receives and transmits, so a difference is
a change to the FT transmit offset, not a stale picture. It records the waterfall it runs (twice,
written only if identical) and says which commit that was and whether its `Waterfall.tsx` was that
commit's own.

## What it cannot tell you

- It is Chrome, not the app's webview (WebView2, WKWebView, WebKitGTK), and it rasterises in
  software. Perf numbers are this machine's and this browser's; compare runs on one machine only.
- IPC is the webview's half: the parse on the main thread. The backend's serialisation and each
  platform's IPC transport are not measured here.
- The renderer's GPU numbers are SwiftShader's, a CPU rasteriser, and the pinned backend composites
  in software, which reads a WebGL canvas back on the main thread every frame (about 6 ms at
  1024×768 here; 0.3 ms with GPU compositing on the same SwiftShader). Neither is a hardware GPU's
  number: those need a real machine. The Waterfall's WebGL2 perf at 3440×1440 is that readback:
  5.6–7.1 ms a frame and 51–53 fps pinned, 0.2 ms at 60 fps with GPU compositing (2026-10-04).
- The CI-V rates are not measured on a radio; 3/s is a planning figure and 10/s an estimate from the
  waveform's serial load. Put the IC-9700's measured rate in `TIMED_SETS` (`frames.ts`) when it has one.
