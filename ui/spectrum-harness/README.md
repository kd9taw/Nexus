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
| pixel | four fixtures (`frames.ts`: a carrier, a two-tone, a noise-floor step, an FT8 period) through each component, against `baselines/*.png` within `compare.mjs`'s tolerance. Rows are served by call, so the picture does not depend on timing. | yes |
| cadence | sources on the real clock at their producers' rates — audio 50/s, CI-V 3/s and 10/s, Flex 15/s, FT-710 84/s — each sweep carrying a barcode of its own number. The canvas is read back: rows committed, distinct sweeps shown, and **repeats** (a committed row showing the same sweep as the row before it). | repeats = 0 |
| perf | each component filling a 1024×768 and a 3440×1440 window: frame pacing, long animation frames, main-thread time per frame, the cost of one committed row | measured only |
| ipc | a `Spectrum` row parsed from its JSON on every frame at 60 Hz, 512 and 2048 bins, beside the same values taken from a binary buffer | measured only |

Timings come from `performance.now()` on a cross-origin-isolated page (5 µs, not 100 µs).

## Controls — run every time

An instrument that cannot fail proves nothing, so two controls are part of every run and the run is
red if either comes back clean: the carrier fixture rendered in a **wrong palette** must fail the
pixel comparison, and a **planted extra row** (one ask answered with the previous sweep again) must
be found by the cadence probe, exactly once. The long-frame observer gets its own: an 80 ms frame
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
only if the two renders are identical. Re-record only for a deliberate change to what the operator
sees, and say what changed in the commit. The pictures are keyed on the backend; a run under any
other backend refuses to compare rather than reporting a difference.

## What it cannot tell you

- It is Chrome, not the app's webview (WebView2, WKWebView, WebKitGTK), and it rasterises in
  software. Perf numbers are this machine's and this browser's; compare runs on one machine only.
- IPC is the webview's half: the parse on the main thread. The backend's serialisation and each
  platform's IPC transport are not measured here.
- The CI-V rates are not measured on a radio; 3/s is a planning figure and 10/s an estimate from the
  waveform's serial load. Put the IC-9700's measured rate in `TIMED_SETS` (`frames.ts`) when it has one.
