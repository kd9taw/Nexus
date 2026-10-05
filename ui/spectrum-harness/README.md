# Spectrum harness — the scope and waterfall in a real browser

jsdom never lays out and never has a graphics context, so every unit gate in `ui/` stays green
through a broken waterfall paint path. This harness mounts the **shipped** `PhoneScope` (Phone and
CW), `Waterfall` (FT, JS8, RTTY, PSK, SSTV) and `MiniSpectrum` in headless Chrome, unmodified, with the desktop IPC
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
| pixel | four fixtures (`frames.ts`: a carrier, a two-tone, a noise-floor step, an FT8 period) through each component, against `baselines/*.png` within `compare.mjs`'s tolerance. Rows are served by call, so the picture does not depend on timing; PhoneScope's trace hold and averaging, which run on the real clock, are set to none for this probe, so its trace is the last row's own shape. PhoneScope's picture is read from the spectrum renderer's canvas (WebGL2 here, asked for by the renderer's hidden backend setting — its own choice on this browser is canvas-2D, below — and asked to keep its drawing buffer so it can be read after the frame), not its overlay. The Waterfall and MiniSpectrum also draw through the spectrum renderer, and theirs run on **both** backends (the renderer's backend asserted each time): the Waterfall's band against its one stored picture, which canvas-2D reproduces exactly; MiniSpectrum, all trace, against a picture per backend (the two rasterise a line differently), on two fixtures. | yes |
| cadence | sources on the real clock at their producers' rates — audio 50/s, CI-V 3/s and 10/s, Flex 15/s, FT-710 84/s — each sweep carrying a barcode of its own number. The canvas is read back: rows committed, distinct sweeps shown, and **repeats** (a committed row showing the same sweep as the row before it). Every row entering PhoneScope's renderer ring is logged with the frame number it carries, so each repeat is also read as **marked** (the number of the row before it: a repeat that says so) or **unmarked** (a new number on an old sweep: the defect); the Waterfall numbers its rows itself, so its ring is not read. PhoneScope runs in both of its slow-scope looks: a **row per sweep** and **smooth scroll** (below); the Waterfall on both backends. | per look, below; the Waterfall, repeats = 0 |
| perf | each component filling a 1024×768 and a 3440×1440 window: frame pacing, long animation frames, main-thread time per frame, the cost of one committed row (the Waterfall's, which draws on the next frame, is its fetch-to-commit work only); the Waterfall on both backends. PhoneScope draws on the backend the renderer chooses (canvas-2D on this browser), named on each line | measured only |
| ipc | a `Spectrum` row parsed from its JSON on every frame at 60 Hz, 512 and 2048 bins, beside the same values taken from a binary buffer | measured only |
| render | the renderer core (`ui/src/spectrum`, mounted bare, without the component around it) on **both** backends, WebGL2 and canvas-2D: nine fixtures (`renderer.ts`) against `baselines/renderer/<backend>-<fixture>.png`; the two backends against each other on the waterfall band, where both run the same per-pixel mapping (and on a zoomed 23 cm scope span reached from audio, where canvas-2D's float64 is the reference for WebGL2's float32 hertz); and the 3-D stack against a one-row burst | yes. The whole-picture difference between backends (the trace line and the 3-D stack are rasterised differently) is printed |
| capability | which backend the renderer picks (`src/spectrum/choose.ts`). WebGL2 here is SwiftShader, a software rasteriser, so the automatic choice must be canvas-2D: by the renderer string, and by the timed probe when the string is masked (as WebKitGTK masks it), which must stop at its timed frame here and take under 100 ms: two WebGL2 frames are its floor, so the bound holds on CI's runners, not on every machine (`PROBE_BUDGET_MS` in `run.mjs` says where it breaks). A GPU's renderer string must get WebGL2; WebGL2 asked for, by the option or the hidden setting, must still be built here; and canvas-2D (with the reason) when there is no context or when the context takes float uploads and keeps nothing, asked for or not | yes |
| loss | a forced WebGL2 context loss (`WEBGL_lose_context`): canvas-2D must stand in at once from the same history, and WebGL2 must come back without a reload and draw exactly what a renderer that never lost its context draws | yes |
| rperf | the renderer at 2048 bins × 2048 rows filling a 1024×768 window, one new row and one redraw per frame, on each backend, flat and 3-D: the renderer's own main-thread time, the main-thread task time, and draw-to-pixels (plus the GPU timer query where the context has one), beside the renderer's budget (under 2 ms of GPU and 1 ms of main thread a frame) | measured only |
| axis | the scale's axis (`ui/src/spectrum/scale.ts`) against the picture: a −20 dBFS level, encoded as the producer encodes it and drawn by each backend in the range `scaleRange.ts` gives it, must sit on the axis's −20 dBFS tick (the line's centre within a pixel) | yes |
| offsets | what a click on the Waterfall sets: **real** mouse events (DevTools `Input.dispatchMouseEvent`, so the browser's own hit-testing picks the element), every gesture the waterfall answers plus the middle button it refuses, 13 points across the canvas, in the picture and on the axis strip, for every view the picker offers and WSPR's fixed sub-band, with the RX marker walked so a zoomed window holds and then pages, on both backends. Every click must reach the waterfall canvas, set exactly the hertz and target in `baselines/waterfall-offsets.json` (recorded from the waterfall before it drew through the renderer, `recordedFrom`), and cancel the default where that did. The TX offset is where an FT over is keyed. | yes, exactly |
| overlays | PhoneScope's overlays (`src/spectrum/overlays.ts`) on their own canvas, on the Flex set with the dial at 14.100 MHz: the licence-class tint from a stand-in `get_privilege_spans` (one span), a spot's tick and the FT RX offset, each read back by value off the overlays' canvas (tinted outside the span and not inside it, its edges marked, the tick and the line at their frequencies). Then a spot storm with the scope paused: the picture's draw calls are counted beside the overlays' redraws, and a spot change may redraw the overlays only. Live, the picture's draw calls with and without a storm | the tint, the marks and the storm, yes; live, measured only |

Timings come from `performance.now()` on a cross-origin-isolated page (5 µs, not 100 µs).

## Controls — run every time

An instrument that cannot fail proves nothing, so controls are part of every run and the run is red
if any comes back clean: the carrier fixture rendered in a **wrong palette** must fail the pixel
comparison (the components' fixture, and the renderer's on each backend); a **planted extra row**
(one ask answered with the previous sweep again) must be found by the cadence probe, exactly once;
a **broken context** (none at all, or one that drops float uploads) must fail the renderer's
self-test and leave it on canvas-2D; the backend probe with every canvas-2D frame made **slow** must
keep WebGL2 (a probe that always answered canvas-2D would pass the masked-string check), and with every WebGL2 read-back made **slow** it must stop at its timed frame and break the 100 ms budget (a budget nothing can break proves nothing); the loss check run with the renderer's **restore handler
dropped** (the page swallows every `webglcontextrestored` listener) must fail; the axis's −20 dBFS
tick on an axis pinned to a **wrong reference** (a −100 dBFS floor) must miss the drawn line; the same
clicks **2 px off** must not set the recorded offsets; palette changes under the overlays probe's draw count must redraw the picture, or its "no picture redraw" proves nothing; and the burst the 3-D
stack rejects must show on the 2-D waterfall, or that check proves nothing. The long-frame observer gets its own: an 80 ms frame
is planted during warm-up and must be seen, or long frames are reported as unmeasured rather than
as zero. `--palette NAME` and `--plant N` apply the same controls to every check by hand.

## The slow-scope looks, and how each is gated

PhoneScope commits a waterfall row only when the source's frame number advances, and the operator
picks what a poll with nothing new does (`nexus.phonescope.rows`; the page takes `rows=` from the URL):

- **`sweep`, one row per sweep.** Nothing: the waterfall moves when the source does. Gated on zero
  repeated rows, one row per answered ask, and every ring row carrying the number of the sweep its
  pixels show — for audio, FT-710, CI-V at 3 and 10 and Flex at 15 a second.
- **`smooth`, smooth scroll (the default).** The newest sweep is committed again, as a repeat that
  carries its own number. Repeats are expected on every source slower than the poll; gated on every one
  being marked, none unmarked or misaligned, and one row per ask, new or not.

Both looks have a control: a planted row (an old sweep under a NEW number, the one way a copy could
reach a scope that commits only new frames) must be found as exactly one repeated row under `sweep`,
and as exactly one unmarked repeat among the marked ones under `smooth`.

## Known failures

A check can be marked as an expected red (`expect: 'known-failure'` in `CADENCE`). It keeps the run
green while it fails, and turns the run red the day it passes, so the marker cannot outlive the
defect. None today: the repeated-row defect (`civ-3`, `civ-10`, `flex-15` on PhoneScope, a row on
every 50 ms poll whether or not the source had swept, so at 3 sweeps a second five rows in six were
copies) was fixed by committing on the frame number, and its markers were removed in that change.

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
- The backend choice's probe is timed here on SwiftShader only. Where it decides in the field (WebKitGTK, which masks
  the renderer string, on llvmpipe or a weak GPU) it has not been measured: that needs a real Linux box and a Pi.
- The renderer's GPU numbers are SwiftShader's, a CPU rasteriser, and the pinned backend composites
  in software, which reads a WebGL canvas back on the main thread every frame (about 6 ms at
  1024×768 here; 0.3 ms with GPU compositing on the same SwiftShader). Neither is a hardware GPU's
  number: those need a real machine. The Waterfall's WebGL2 perf at 3440×1440 is that readback:
  5.6–7.1 ms a frame and 51–53 fps pinned, 0.2 ms at 60 fps with GPU compositing (2026-10-04).
- The CI-V rates are not measured on a radio; 3/s is a planning figure and 10/s an estimate from the
  waveform's serial load. Put the IC-9700's measured rate in `TIMED_SETS` (`frames.ts`) when it has one.
