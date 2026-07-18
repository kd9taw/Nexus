# Intentional deviations from slowrx

Translating slowrx 1:1 in Rust isn't always the right call — a few
behaviors are deliberately different. This file lists every deviation
we know about, why we chose it, and the conditions under which we'd
revisit. Future audits should consult this list before flagging any
of these as "missing".

For the parity audit reports themselves, see
[`docs/audits/`](./audits/).

---

## VIS stop-bit boundary: precise vs. ±20 ms slop

**Files:** `src/vis.rs::take_residual_buffer` ↔ slowrx `vis.c:168-170`.
**Tracking issue:** [#39](https://github.com/jasonherald/slowrx.rs/issues/39).

### What slowrx does

After the VIS stop bit, slowrx unconditionally skips a fixed 20 ms
(`readPcm(20e-3 * 44100)`) regardless of which `i` (phase-offset slot
in the 9-iteration loop) matched. The actual stop-bit end can be
0–20 ms before that point depending on `i`.

### What we do

`vis.rs` computes the **exact `i`-aware stop-bit end** as
`stop_end_abs = (hops_completed + i) * HOP_SAMPLES`. The residual buffer
begins precisely there.

### Why we deviated

For real radio capture, slowrx's slop is fine — the receiver re-locks
on the post-burst SYNC pulse. But during Phase 1 (VIS rewrite, before
Phase 2's FindSync existed) the synthetic round-trip needed exact
alignment to test pixel decode without the line-zero find absorbing
the misalignment. Computing the exact boundary keeps per-pixel image
alignment tight without depending on FindSync to clean up.

After Phase 2 landed, FindSync's Skip computation absorbs ±175 ms of
misalignment via convolution — so technically we *could* relax to
slowrx's slop. But there's no functional reason to: the exact-boundary
code is simpler and more predictable, and we'd lose nothing by keeping
it.

### When to revisit

If we ever observe a real-radio capture where the exact-boundary
computation is *worse* than slowrx's slop (e.g., burst timing where
slowrx's slop happens to land on a SYNC edge that helps Skip lock
faster). Has not happened in the Dec-2017 ARISS validation set.

---

## FindSync 90° slant deadband

**Files:** `src/sync.rs::find_sync` ↔ slowrx `sync.c:79`.
**Tracking issue:** [#42](https://github.com/jasonherald/slowrx.rs/issues/42).

### What slowrx does

slowrx applies a Hough-derived rate correction unconditionally, even
when the detected slant is already ~90° (i.e., no slant). The
correction term `tan(90 - slant_angle) / line_width * Rate` is small
near 90° but non-zero, so a clean image still gets a tiny rate nudge
each call.

### What we do

We apply a 0.5° deadband around 90° — if `|slant - 90| <= 0.5°`, no
correction is applied.

### Why we deviated

slowrx's "harmless" tiny correction compounds over multiple `find_sync`
calls, eventually producing visible drift on long images. The deadband
gives us a stable "lock" state that's a strict improvement.

### When to revisit

If a future test reveals a case where the 0.5° deadband prevents
necessary corrections (extremely tilted slant near the lock window).
Not observed.

---

## VIS retry behavior on parity failure

**Files:** `src/vis.rs::match_vis_pattern` ↔ slowrx `vis.c:140-160`.
**Tracking issue:** Documented inline (round-2 audit Finding 5).

### What slowrx does

slowrx terminates the (i, j) alignment loop on the first `(i, j)` whose
bits decode without a parity error. If a tone-classification mistake at
one `(i, j)` yields a parity-failing code, slowrx aborts the whole
detection and waits for the next 10 ms hop to retry.

### What we do

We exhaust all 9 `(i, j)` candidates before giving up. If a later
`(i, j)` decodes a parity-passing code, we accept it.

### Why we deviated

More recovery on borderline real-radio bursts. slowrx's early-exit is
mostly an artifact of its `HedrShift`-set-before-parity-check pattern;
the strict "first parity-passing match wins" semantics aren't
load-bearing.

### When to revisit

If a real burst gives Rust a *different* valid VIS code than slowrx
(borderline tones that pass parity at multiple `(i, j)`). Not observed.

---

## VIS: keep-searching for a known code; surface clean unknown bursts

### What slowrx does

`vis.c`'s pattern matcher tries all 9 `(i, j)` alignments. For each alignment
that passes parity it does `VISmap[VIS]` — and if the code is unknown it
`printf`s `"Unknown VIS %d"`, leaves `gotvis = false`, and keeps trying the
remaining alignments. Only a *known* code stops the search (`gotvis = true`).
If no alignment yields a known code, `GetVIS()` returns `0` and `Listen()`'s
`do { ... } while (Mode == 0)` loop re-detects from the audio stream.

### What we do

`match_vis_pattern` takes an `is_known` predicate (`|c| modespec::lookup(c).is_some()`)
and mirrors slowrx: it tries all 9 alignments and returns the **first known**
code. If no alignment is known but at least one parity-passing alignment maps
to an **unknown** code, it returns the first such code as a fallback. The
decoder then emits `SstvEvent::UnknownVis { code, hedr_shift_hz, sample_offset }`
(our equivalent of slowrx's `printf`), reseeds the VIS detector on the
post-stop-bit residue (the `#40` re-anchor contract), and stays in
`AwaitingVis` — the same "try again" loop as slowrx.

### Why we deviated

slowrx's `printf` is a console side effect with no programmatic surface; a
library decoder should let callers observe "a burst arrived but I can't decode
it" (stream monitors, diagnostics). Returning the unknown code as a fallback
from `match_vis_pattern` (rather than `None`, which slowrx effectively does) is
what makes that event possible. The keep-searching-for-a-known-code behavior
otherwise matches slowrx's (modulo the separate `HedrShift != 0` early-exit
quirk documented above, which we also deliberately diverge from).

### When to revisit

If R12BW (or any other currently-unimplemented mode) gains a `ModeSpec`, it
becomes "known" automatically (the predicate is `lookup(...).is_some()`), and
bursts for it stop surfacing as `UnknownVis` and start decoding. Nothing else
to change.

---

## Synthetic round-trip max_diff tolerance

**Files:** `tests/roundtrip.rs`.
**Context:** Phase 7 (PR #60).

### What changed

Round-trip test originally asserted `max_diff <= 25` (and `mean < 5`).
With Phase 3 deferrals (#44 SNR-adaptive Hann, #45 channel-mask drop)
engaged, isolated synthetic boundary pixels hit `max_diff = 234–255`.
The `max_diff` check was dropped; only `mean < 5.0` remains.

### Why

The synthetic encoder produces instant frequency-step transitions at
pixel boundaries. Real radio's FM-modulator slewing softens these.
slowrx's behavior (which our deferral engagement matches) is correct
on real radio — verified visually against the Dec-2017 ARISS captures
— but the synthetic "instant step" inputs trip the SNR-adaptive
selector + boundary FFT in ways the slewed real-audio doesn't.

Mean diff stays excellent across the PD family (1.5–1.9 on PD120/PD180,
similarly low on PD240) — the decoder is mostly fine; the `max` is
dominated by a handful of boundary pixels per image.

### When to revisit

Either:
1. Upgrade the synthetic encoder to model FM slewing (tunable risetime
   between adjacent pixel frequencies). Then `max_diff` becomes
   meaningful again.
2. Add a real-audio cross-validation suite (gitignored fixtures already
   exist in `docs/wav_files/`; the `slowrx-cli` binary covers ad-hoc
   smokes).

---

## Robot family pixel-time offset: `(x + 0.5)` vs slowrx C `(x - 0.5)`

**Files:** `src/mode_pd.rs::decode_one_channel_into` ↔ slowrx `video.c:140-142` (PD case) vs. `:196-198` (non-PD case).
**Tracking:** Surfaced during V2.2 P3 (Robot 72) code review.

### What slowrx does

slowrx C uses **two different per-pixel time formulas**:

- **PD modes** (`video.c:140-142`):
  `Time = round(Rate * (y/2 * LineTime + ChanStart + PixelTime * (x + 0.5)))`
  Pixel sampling centered at `(x + 0.5) * PixelTime` from channel start.

- **Non-PD modes** including Robot 72 (`video.c:196-198`):
  `Time = round(Rate * (y * LineTime + ChanStart + (x - 0.5) / Width * ChanLen[Channel]))`
  Pixel sampling centered at `(x - 0.5) * (ChanLen / Width)` from channel start —
  i.e., `(x - 0.5) * PixelTime` for non-Robot-alt modes where ChanLen = PixelTime * Width.

The two forms differ by **1 pixel-time** in the per-pixel sampling offset.

### What we do

The Rust port reuses `mode_pd::decode_one_channel_into` for both PD and Robot 72.
That helper uses the PD `(x + 0.5)` formula. So Robot 72 in slowrx.rs samples each
pixel `1 * pixel_seconds` later than slowrx C would.

### Why we deviated

Sharing one helper between PD and R72 keeps the codebase smaller and the FFT
windowing logic single-source. The synthetic round-trip (`tests/roundtrip.rs::robot72_roundtrip`)
passes at the same `mean < 5.0` threshold as PD because the encoder
(`robot_test_encoder::encode_r72`) ALSO emits at the same per-pixel timing — the
encoder/decoder pair is internally consistent.

### Real-radio impact

Against real-radio audio (e.g. ARISS Fram2 Robot 36 corpus — which this V2.2
work uses as the merge gate), the deviation manifests as a **half-pixel
horizontal shift** in the decoded image relative to slowrx C's output. For
real audio the FFT window is wider than a half-pixel, so visual quality is
unaffected at the per-image scale. The Phase 5 visual validation against the
12 ARISS Fram2 reference JPGs is the empirical test.

### When to revisit

Three triggers would prompt revisiting:

1. **Phase 4 R36/R24 round-trip fails** because Y has 2× pixel-time and the
   asymmetric `(x ± 0.5)` formula amplifies a per-channel offset error that
   was tolerable for R72.
2. **Fram2 visual validation surfaces a measurable horizontal shift** vs. the
   reference JPGs.
3. **A future audit cross-validates pixel-by-pixel against slowrx C output**
   on the same audio file — that would expose the half-pixel offset directly.

If any of these fires, the fix is to introduce a per-mode pixel-offset selector
(e.g., a `pixel_offset_within_channel: f64` field on `ModeSpec` set to 0.5 for
PD and -0.5 for non-PD), and route it through `decode_one_channel_into`.

### Status

- ✅ Trigger #1 cleared as of V2.2 Phase 4: R36/R24 synthetic round-trips
  pass at `mean < 5.0` despite Y being at 2× pixel-time. The encoder
  emits at the same `(x + 0.5)` offset the decoder reads at, so the
  R36/R24 round-trip is internally consistent — the deviation is invisible
  to the synthetic gate.
- Triggers #2 and #3 remain open. Phase 5 (Fram2 visual validation) is
  the next empirical test; a future cross-validation against slowrx C
  output on the same audio file would expose the half-pixel offset
  directly.

---

## Faint vertical squiggle artifacts in Robot real-radio decode

**Files:** `src/mode_robot.rs` (and plausibly `src/mode_pd.rs::decode_one_channel_into`).
**Tracking issue:** [#71](https://github.com/jasonherald/slowrx.rs/issues/71).

### What we observe

When decoding real-radio Robot 36 audio (verified against the 12 ARISS
Fram2 WAVs during V2.2 Phase 5), our output PNGs exhibit faint vertical
squiggle artifacts every ~20–30 pixels. The image content is correct
and recognizable — the artifacts are a fine pattern overlaid on the
content.

The reference JPGs ARISS publishes alongside the WAVs do NOT show these
artifacts, which means whatever decoder produced the references handles
this case better than ours.

### Why this isn't a "deviation" yet

This is more of an **open quality gap** than a deliberate deviation.
We're tracking it here so future audits know it's a known and
documented behavior, not a missed bug. V2.2 ships with this gap because
the image content is correct and visually validates against the
reference.

### When to revisit

When [#71](https://github.com/jasonherald/slowrx.rs/issues/71) is
prioritized, or whenever a downstream consumer asks for cleaner output.
The investigation paths in #71 cover SNR-adaptive Hann selector
re-engagement (V1 deferral #44), slowrx C cross-validation, and per-
pixel sub-bin interpolation.

---

## SNR hysteresis on adaptive Hann window selection

**Files:** `src/snr.rs::window_idx_for_snr_with_hysteresis` ↔ slowrx `video.c:354-367`.
**Tracking issue:** [#71](https://github.com/jasonherald/slowrx.rs/issues/71).
**Shipped in:** 0.3.2.

### What slowrx does

slowrx C selects the per-pixel Hann window length using pure-threshold
logic on the SNR estimate (`video.c:354-367`):

```c
if      (!Adaptive)  WinIdx = 0;
else if (SNR >=  20) WinIdx = 0;
else if (SNR >=  10) WinIdx = 1;
else if (SNR >=   9) WinIdx = 2;
else if (SNR >=   3) WinIdx = 3;
else if (SNR >=  -5) WinIdx = 4;
else if (SNR >= -10) WinIdx = 5;
else                 WinIdx = 6;
```

No hysteresis. When SNR fluctuates near a threshold (e.g., real-radio
SNR oscillating ±0.5 dB across the 9 dB boundary between `WinIdx=2` and
`WinIdx=3`), the selector flips every SNR re-estimation cadence (5.8 ms
wall-clock).

### What we do

We use the same threshold table but apply a 1 dB hysteresis band at
each transition. The function `window_idx_for_snr_with_hysteresis(snr_db,
prev_idx)` ratchets one band per call toward
`window_idx_for_snr(snr_db)`, applying a 0.5 dB hysteresis at the
adjacent boundary:

1. Compute `baseline = window_idx_for_snr(snr_db)`. If it equals
   `prev_idx` the SNR is in `prev_idx`'s band — return immediately.
2. Pick `target_idx` one band closer to `baseline` than `prev_idx`.
3. Re-evaluate `window_idx_for_snr` at `snr_db ± 0.5` (away from
   `target_idx`).
4. If the shifted lookup confirms the SNR is past `target_idx`'s side
   of the boundary, accept `target_idx`. Otherwise stay at `prev_idx`.

Per-pixel FFTs converge in O(`n_bands`) calls. Ratcheting one step at
a time (rather than jumping straight to `baseline`) keeps the selector
convergent even when `prev_idx` is far from `baseline` — e.g.
cold-start at idx 6 with a strong signal — without breaking the 1 dB
hysteresis guarantee at any individual boundary.

### Why we deviated

V2.2 Phase 5 visual validation against the 12 ARISS Fram2 R36 reference
WAVs revealed faint vertical squiggle artifacts every ~20–30 pixels in
the decoded PNGs. The squiggle period (5.8 ms of audio at R36 Y's
0.275 ms/pixel cadence ≈ 21 px) matches the SNR re-estimation cadence
exactly. A code-only audit (#71) found the DSP otherwise arithmetically
equivalent to slowrx C. Hypothesis: real-radio SNR fluctuates near a
window-selection threshold, the selector flip-flops, and that produces
periodic vertical banding.

slowrx C exhibits the same algorithmic property (no hysteresis) and
would in principle produce the same artifact at its 5.8 ms cadence.
The reference JPGs ARISS published were almost certainly decoded by a
different tool (MMSSTV or RX-SSTV in the ARISS community) that either
uses a fixed window or has hysteresis.

The 1 dB band is small enough that real SNR changes still propagate
quickly (≥ 0.5 dB past threshold = one cadence delay max), and large
enough that typical real-radio fluctuation (0.5–1.5 dB) doesn't cause
flip-flop.

### When to revisit

Three triggers would prompt revisiting:

1. **Empirical Fram2 validation shows the squiggles persist or worsen
   after this hysteresis lands.** Re-run the procedure at
   [`tests/ariss_fram2_validation.md`](../tests/ariss_fram2_validation.md)
   and compare visually against the V2.2 baseline. Persistence means
   hysteresis isn't the root cause; move on to other paths in #71
   (sub-pixel FFT interpolation, resampler quality).
2. **Decode quality regresses at SNR edges** (e.g., images with
   alternating bands of high and low SNR show banding at the band
   boundaries because hysteresis is filtering legitimate SNR changes).
   Tune the band size or switch to a debouncer/smoother strategy.
3. **A future audit cross-validates pixel-by-pixel against slowrx C
   output on the same WAV** and finds slowrx's pure-threshold behavior
   matters in some specific way. Unlikely but possible.

---

## FFT frequency resolution exceeds slowrx C by 4×

**Files:** `src/snr.rs::FFT_LEN`, `src/mode_pd.rs::FFT_LEN` ↔ slowrx `video.c::FFTLen`.
**Tracking issue:** [#71](https://github.com/jasonherald/slowrx.rs/issues/71) (squiggle context).
**Shipped in:** 0.3.3.

### What slowrx does

slowrx C uses `FFTLen = 1024` at `44_100` Hz, giving
`44100 / 1024 ≈ 43.07` Hz/bin frequency resolution for the per-pixel
demod and SNR estimator (`video.c:303-340, 369-395`).

### What we do

We use `FFT_LEN = 1024` at [`crate::resample::WORKING_SAMPLE_RATE_HZ`]
= `11_025` Hz, giving `11025 / 1024 ≈ 10.77` Hz/bin —
**4× finer than slowrx C**.

The bump produces two coupled DSP changes:

1. **Per-pixel demod (`mode_pd::PdDemod::pixel_freq`)**: 4× finer
   bin density only. `HANN_LENS` is unchanged at
   `[12, 16, 24, 32, 64, 128, 256]` (slowrx's
   `[48, 64, 96, 128, 256, 512, 1024]` divided by 4) so the Hann is
   applied to the first `HANN_LENS[idx]` samples of the FFT input
   and the rest is zero-padded — time-domain support identical to
   slowrx C, only the FFT bin density changes.
2. **SNR estimator (`SnrEstimator::estimate`)**: the long Hann window
   `hann_long = build_hann(FFT_LEN)` scales with `FFT_LEN`, so it
   grows from 256 samples (~23 ms at 11_025 Hz, matching slowrx C) to
   1024 samples (~93 ms, 4× longer than slowrx C). The SNR estimator
   therefore integrates over a 4× longer time window. This is a
   second, real deviation that comes "for free" with the FFT_LEN
   bump and is desirable: the longer integration produces a cleaner
   SNR estimate, which in turn reduces flip-flop in the
   adaptive-Hann selector beyond what the 0.3.2 hysteresis already
   delivers.

Both effects were validated together on the parallel experiment
branch and contribute to the "WAY clearer" visual finding on the
12 ARISS Fram2 R36 reference WAVs. We do not attempt to decouple
them — the longer SNR-estimator window is part of the package, not
a regression to mitigate.

### Why we deviated

0.3.2 shipped a 1 dB SNR hysteresis band as a partial fix for the
real-radio squiggle artifacts ([#71]). Hysteresis reduced but didn't
eliminate the squiggles. While CodeRabbit reviewed PR #74, a
parallel experiment branch tested bumping `FFT_LEN` to 1024. Result
on the 12 ARISS Fram2 R36 reference WAVs: synthetic round-trips all
passed at the unchanged `mean < 5.0` threshold, and visual inspection
showed **noticeably clearer pixel content** vs. the 0.3.2 baseline
(by-eye comparison; the user judged it "WAY clearer").

The squiggle artifacts themselves were unchanged — that's a separate
concern tracked in [#71]. The finer Hz/bin is a complementary DSP
improvement that's worth shipping on its own.

[#71]: https://github.com/jasonherald/slowrx.rs/issues/71

### When to revisit

1. **Squiggle root cause turns out to require coarser FFT.** Unlikely
   given the 0.3.2 hypothesis (SNR-cadence flip-flop) was unaffected
   by FFT_LEN. But if a future audit finds an FFT-resolution-dependent
   artifact, this is the knob.
2. **CPU cost becomes an issue.** The 4× FFT compute per call is
   negligible at SSTV's per-pixel cadence. If a future profile shows
   the per-pixel FFT dominating wall-clock time on resource-constrained
   targets, consider reverting or adding a `cli`-feature-gated coarse
   mode.
3. **A future audit cross-validates pixel-by-pixel against slowrx C
   output** and finds slowrx's `usize` bin counts matter in some
   specific way. Unlikely — bandwidth integration is in Hz domain —
   but possible.

---

## FindSync skip_samples rounding: round-to-nearest vs slowrx's truncation

**Files:** `src/sync.rs::find_sync` ↔ slowrx `sync.c:120`.
**Tracking issue:** (none — sub-sample effect, no observable behavior change).

### What slowrx does

`Skip = s * Rate;` is an implicit `double → int` cast, which in C
truncates toward zero. A `s_secs * rate` of `0.6` lands at `0`.

### What we do

`let skip_samples = (s_secs * rate).round() as i64;` rounds to nearest.

### Why we deviated

Truncation in slowrx isn't a deliberate choice — it's the side effect
of an implicit C cast idiom. Round-to-nearest minimizes the max
sub-sample error (½ sample vs 1 sample). The difference is at most
1 sample at our 11025 Hz working rate (~91 µs) — well below SSTV's
per-pixel duration (~0.5 ms at PD120) and invisible in real-radio
capture.

### When to revisit

If a bit-exact parity test against slowrx-C reference output ever
requires matching to the integer sample, switch back to truncation.

---

## FindSync retry-exhaustion: keep last estimate vs slowrx's reset to 44100

**Files:** `src/sync.rs::find_sync` ↔ slowrx `sync.c:86-90`.
**Tracking issue:** (none).

### What slowrx does

After `MAX_SLANT_RETRIES` Hough passes without locking inside the
`(89°, 91°)` window, slowrx resets `Rate` to 44100 (its working
sample rate) — i.e. throws away all the slant-correction progress
made over the retries.

### What we do

We keep the last adjusted `rate` even when the lock window isn't
reached.

### Why we deviated

Re-anchoring a near-locked input is harmful: a borderline lock that
converged to ~91.1° (one 0.5° Hough bin outside the lock window)
would be reset back to 44100 in slowrx, discarding all the
correction the retry loop did. Keeping the last estimate gives a
better decode on those borderline cases.

### When to revisit

If a regression surfaces where rate-correction overshoots and an
explicit reset gives a better outcome. Has not happened in the
Dec-2017 ARISS validation set.
