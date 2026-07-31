#!/usr/bin/env python3
"""
Build `observed.json` -- real recorded satellite Doppler tracks for validating a predictor.

    python3 generate.py            # rebuild observed.json from the curated observation list
    python3 generate.py --qc       # additionally cross-check with Skyfield and print residuals
    python3 generate.py --survey N # score N recent candidates to find new fixture material

WHAT THIS IS
------------
Each entry is one real satellite pass recorded by a real SatNOGS ground station: the TLE
that station actually used, the station's coordinates, and the carrier frequency measured
out of the recorded waterfall, sample by sample, in absolute Hz.

A Doppler predictor is correct if, for each observation, there is a SINGLE constant
frequency offset that makes the predicted curve match `observed_hz` across the whole pass.


>>> THE ONE THING A CONSUMER MUST DO, AND THE LIMITATION IT ENCODES <<<
----------------------------------------------------------------------
`observed_hz` is the RAW measured absolute frequency. It is NOT offset-corrected here,
on purpose. Every observation carries an unknown constant frequency error -- the
satellite's own transmitter error (cheap cubesat TCXOs run tens of ppm off) plus the
ground station's local-oscillator error. The consumer must fit ONE constant per
observation, e.g.

    offset = mean(observed_hz[i] - predicted_hz(t[i]))          # least-squares optimal
    residual[i] = observed_hz[i] - predicted_hz(t[i]) - offset

That constant is physically real, not a fudge. Measured across independent passes:

    STARS   @ 4803, obs 14635786 / 14645738 (both IN this fixture)  -56.82 / -56.75 ppm
    CUTE1.7 @ 4803, two passes                                      -60.52 / -60.28 ppm
    OBJECT J@ 4803, three passes                                -3.85 / -3.81 / -3.69 ppm

The same (station, satellite) pair reproduces to 0.07-0.2 ppm -- tens of Hz at 437 MHz --
across passes days apart, while three different satellites at the SAME station sit ~15x
apart from each other. So the offset is dominated by each satellite's own transmitter
error, with a station LO term on top: a property of the hardware, not a free parameter
invented to make the fit work. The two STARS entries are in the fixture specifically so a
consumer can verify this for itself.

(Earlier recon claimed this reproduces to ~2 Hz. It does not. That figure came from
offsets estimated after prediction-based outlier gating; measured ungated, the spread is
~0.2 ppm. The conclusion -- the offset is physical and must be fitted out -- is unchanged,
but do not expect Hz-level reproducibility.)

    ==> THEREFORE: this fixture validates the SHAPE of the Doppler curve over a pass.
    ==> It CANNOT validate absolute frequency. A predictor with a constant frequency
    ==> bias of any size passes. Only the time-varying part is under test.

Fitting anything BEYOND that one constant -- a time bias, a scale factor, a polynomial --
would let the fit absorb real predictor error. Don't.


HOW `observed_hz` WAS MEASURED (and why it cannot agree with a predictor by construction)
-----------------------------------------------------------------------------------------
SatNOGS publishes each observation's waterfall as a PNG carrying two tEXt chunks:

    satnogs:wf-dat   {"timestamp", "nchan", "samp_rate", "nfft_per_row", "center_freq", ...}
    satnogs:wf-plot  {"xlim_kHz", "ylim_s", "ylim_num", "figsize", "gridspec"}

Together those calibrate both image axes absolutely: `center_freq` + `xlim_kHz` give the
frequency axis, and `ylim_num` (a matplotlib datenum, days since the 1970 epoch) gives a
UTC time axis. The render is nearest-neighbour from a 256-entry viridis colormap, so
inverting the colormap recovers the underlying power array up to 8-bit quantisation.

The carrier is then found by a continuity tracker (`track_carrier`) that sees ONLY the
image. It never reads the TLE, never computes a prediction, and never compares anything
to one. It:
  * normalises each row by its own median and MAD -> a per-row SNR in sigma;
  * seeds on the strongest signal PERSISTING over several rows (carriers persist, noise
    does not);
  * walks outward accepting, per row, the strongest pixel within a window set by the
    largest frequency step ANY Earth satellite could produce (f_c * 1.05e-6 Hz/s, from
    v=7.8 km/s at h=200 km) -- a universal physical bound computed from PNG metadata
    alone, never from this satellite's orbit;
  * drops rows whose peak is weak relative to the pass's own median peak.

No step in the chain consults the prediction, so the stored samples cannot have been
pulled toward it.

>>> Deliberately NOT done: no smoothing, no curve fitting, no polynomial, no outlier
>>> rejection against a predicted curve. Any of those would make the fixture agree with a
>>> predictor by construction and make the test worthless. Samples are stored exactly as
>>> measured.

The prediction-based numbers (residual RMS etc.) are used ONLY to CURATE -- to decide
which whole observations are admitted. That is selection at the level of an entire pass,
not per-sample cleaning: within an admitted observation, every tracked sample is present.


DOWNSAMPLING: STRIDING, not averaging
-------------------------------------
Raw tracks run 300-1500 rows. Each is strided (every k-th sample, k chosen for ~200
points) to keep the committed file small.

Striding was chosen over bin-averaging after measuring both. Averaging to the same point
count reduced residual RMS by under 15% -- far less than the sqrt(k) a white-noise-limited
measurement would give -- which shows the residual is dominated by slowly-varying
systematics (residual TLE error), not per-row noise. Averaging therefore
bought almost nothing while making every stored number a derived quantity. Striding keeps
each stored sample an untouched measurement. (Measured curvature bias from averaging was
<1.5 Hz, so that was not the deciding factor -- the lack of benefit was.)


KNOWN LIMITATIONS -- read before tightening any threshold
---------------------------------------------------------
1. Absolute frequency is NOT validated (see above). Shape only.
2. Resolution floor: ~80 Hz per pixel, 8-bit power quantisation. Parabolic sub-pixel
   interpolation gets well below the pixel pitch, but this is a picture of a spectrum,
   not IQ.
3. The residual floor is SYSTEMATICS, not noise, and it is dominated by TLE error.
   Per-row measurement scatter is only ~0.2 px (10-25 Hz); what is left is a slowly
   varying trend. Fitting a time shift as well as an offset collapses most admitted
   observations to 12-25 Hz, meaning the bulk of the residual is along-track orbit error
   -- the satellite is a second or two off where its elements say it is. That varies per
   PASS, not per station: at station 4803 the implied shift is -0.16 s and -0.13 s for the
   two STARS passes but -3.19 s for OBJECT J and -1.90 s for SEEDS on the same equipment.
   (An earlier read of this data as a per-station clock offset was wrong; the two tight
   STARS passes rule that out.)
   The fixture does NOT fit a time shift, because a second free parameter would also
   absorb genuine predictor error. So a perfect predictor cannot drive these residuals to
   zero -- roughly 15-60 Hz per observation is orbit error you inherit with the data.
4. Curation used residual statistics, so admitted observations are ones where SGP4 with
   the supplied TLE happens to be accurate. A bug shared between this validation
   (Skyfield/SGP4) and the implementation under test would not be caught. This fixture
   tests an independent implementation against reality; it is not an SGP4 correctness
   proof.
5. Only observations that PASSED curation are here, so per-observation residuals are
   biased low relative to a random pass. That is the intent, but do not read these
   numbers as typical SatNOGS quality.

WHAT WAS REJECTED, AND WHY (see REJECTED below for the record)
--------------------------------------------------------------
* missing `satnogs:wf-dat` / `satnogs:wf-plot` tEXt chunks -> unusable, no axis calibration
* ISS -- FM voice/APRS is intermittent; a gappy track lets a tracker bug look like a result
* ORBCOMM -- its SDPSK spectrum has TWO lines about 390 Hz apart with similar amplitude and
  the tracker flips between them, making the residual strictly bimodal. Earlier recon rated
  ORBCOMM cleanest at ~16 Hz, but that figure came from an iterative residual gate that had
  discarded the second mode. Ungated, ORBCOMM is worse than every beacon kept here.
* stale TLEs -- a smooth S-shaped or trending residual means bad elements, not a bad
  predictor
* passes with a small total Doppler span -- they do not exercise the predictor
* passes whose result moved when tracker parameters were varied -- ambiguous tracks

WHAT THIS FIXTURE FOUND ON ITS FIRST RUN
-----------------------------------------
A real bug in `sat.rs`, which is the whole argument for measuring against the sky.

`prepare()` read the element-set epoch with `.timestamp()` — whole seconds — discarding the
fractional second a TLE actually carries (`26211.01342813` is sub-millisecond). The discarded
fraction is effectively random per element set, and a LEO covers ~7.6 km in a second. Across
these eight passes the resulting range error tracked the discarded fraction with r = 0.96, and
the implied speed came out at 4.0-6.8 km/s: orbital velocity, which is the signature.

Cost before the fix, per observation RMS (Hz):

    STARS 14635786   13.8 -> 15.6      OBJECT J       38.9 -> 43.6
    STARS 14645738   13.7 -> 24.2      ORIGAMISAT-2   35.5 -> 34.9
    SEEDS            18.9 -> 20.5      GEOSAT         52.3 -> 56.1
    CUTE-1.7         23.0 -> 20.3      OBJECT BT      59.4 -> 80.1
                                       mean           31.9 -> 36.9
    (Skyfield reference -> Nexus with the truncated epoch)

After the fix Nexus reproduces the Skyfield column to 0.1 Hz on every observation.

Why the existing model-versus-model test could not see it: `sat_golden.rs`'s generator stamped
each sample with the SAME truncated epoch while evaluating Skyfield at the true one, so the two
errors cancelled and that comparison reported 0.41 km worst range agreement while both sides were
wrong together. Fixing sat.rs alone made sat_golden FAIL by 5.3 km, which is how the second bug
surfaced. Both are fixed and its agreement improved (range-rate 0.0017 -> 0.0010 km/s).

The transferable point: a reference implementation constrains you only where it does not share
your mistakes, and a fixture generated alongside the code under test can quietly inherit them.
Real recorded signals have no such loyalty. That is what this fixture is for, and it earned its
keep the first time it ran.

MEASURED RESULT AND SUGGESTED CI THRESHOLD
------------------------------------------
Per-observation residual RMS under ONE fitted constant offset (Skyfield/SGP4 reference):

    14635786 STARS (KUKAI)  stn 4803  51 deg   13.8 Hz   max  31.3   span  4463 Hz
    14645738 STARS (KUKAI)  stn 4803  47 deg   13.7 Hz   max  38.9   span  4624 Hz
    14649456 SEEDS          stn 4803  12 deg   18.9 Hz   max  55.9   span 10871 Hz
    14649394 CUTE-1.7+APD2  stn 4803  28 deg   23.0 Hz   max  70.4   span  3197 Hz
    14601253 ORIGAMISAT-2   stn 5049  15 deg   35.5 Hz   max  99.2   span 17159 Hz
    14645749 OBJECT J       stn 4803  15 deg   38.9 Hz   max 100.3   span 13169 Hz
    14653331 GEOSAT         stn 1696  34 deg   52.3 Hz   max 132.7   span  3287 Hz
    14648099 OBJECT BT      stn 5062  59 deg   59.4 Hz   max 121.1   span 15580 Hz

                                          mean 32.0 Hz, median 29 Hz, worst 59.4 Hz

FAULT INJECTION -- what this fixture actually catches
-----------------------------------------------------
Do not pick a threshold from the passing numbers alone. The table below was produced by
breaking the predictor under test (crates/propagation/src/sat.rs) in realistic ways and
re-running this fixture. Measured through that Rust predictor, not through Skyfield:

    injected bug                      mean Hz   worst Hz
    (correct)                            31.9       59.5
    Doppler sign flipped               5611.8    12643.3
    station longitude sign flipped     2853.0     5991.2
    omega x r dropped (observer fixed)   80.9      146.9
    geocentric instead of geodetic lat   64.3      137.6
    time base +3 s                       63.5      150.0
    Doppler scaled by 1.01               44.8       74.4
    time base +1 s                       40.0       88.9
    Doppler scaled by 1.005              36.2       55.9
    time base +0.25 s                    33.5       66.7
    Doppler scaled by 1.002              32.9       56.1
    station altitude ignored             31.9       59.5   <- literally no change

    ==> GATE, two levels, both needed:
    ==>   (a) every observation:   residual RMS <= 75 Hz   [worst measured 59.5 -> 1.26x]
    ==>   (b) mean over all eight: residual RMS <= 40 Hz   [measured      31.9 -> 1.25x]

They are complementary. A 1% Doppler scale error never breaks any single observation
(worst 74.4) but moves the mean clearly; a +1 s time base barely moves the mean (40.0)
but pushes one observation to 88.9. Either gate alone misses one of them.

An earlier draft suggested 150 Hz. That was wrong: at 150 BOTH the geodetic-latitude and
the dropped-omega-x-r bug pass. Tightness here is not fastidiousness -- it is the
difference between a test that catches those two and one that does not.

Headroom rationale: the fixture is deterministic -- committed samples, frozen TLEs, no
network at test time -- so run-to-run variance is exactly zero. 1.25x only has to cover a
deliberate code change, and nothing legitimate should RAISE these numbers (modelling
UT1-UTC, the one simplification left in sat.rs, would lower them).

BLIND SPOTS -- be explicit about these when reading a green test
----------------------------------------------------------------
* Station altitude is NOT tested at all. Altitudes here are 5-192 m, which changes range
  rate far below the noise floor. A predictor that ignores altitude entirely passes.
* Sub-second to ~1 s time-base errors are NOT reliably caught, because each pass already
  carries 0.1-3 s of along-track TLE error (limitation 3). The fixture cannot separate
  "your clock is off by a second" from "these elements are a second stale".
* Doppler scale errors below ~1% are NOT caught. Sensitivity to scale is proportional to
  Doppler span, so the three wide-span passes (OBJECT J, ORIGAMISAT-2, OBJECT BT;
  13-17 kHz) carry nearly all of it. Keep those three if you ever trim the set.
* Absolute frequency is NOT tested (the fitted offset absorbs it, by design).

Data: SatNOGS Network (https://network.satnogs.org), CC BY-SA 4.0. Requires `requests` and
`pillow`, `numpy`, `matplotlib`; `--qc` additionally needs `skyfield` and `scipy`.
"""

import argparse, json, os, sys, time, re, math

import numpy as np
import matplotlib
from PIL import Image

API = "https://network.satnogs.org/api/observations/"
HERE = os.path.dirname(os.path.abspath(__file__))
CACHE = os.path.join(HERE, ".cache")          # scratch only; not committed
OUT = os.path.join(HERE, "observed.json")
TARGET_POINTS = 200

# --------------------------------------------------------------------------------------
# THE CURATED SET.
# Chosen for: unambiguous single-carrier spectra, low residual under a single constant
# offset, spread over ground stations and satellites, and a mix of pass geometries.
# --------------------------------------------------------------------------------------
#
# Admission gate (every one of these had to pass; all are measured, none are eyeballed):
#     carrier isolation  <= 0.5      one dominant carrier, no competing spectral feature
#     residual RMS       <= 60 Hz    under a SINGLE fitted constant offset
#     max |residual|     <= 200 Hz   no gross tracker excursion anywhere in the pass
#     structured residual<= 150 Hz   max |moving-median|; catches stale-TLE S-curves
#     samples            >= 150      after striding
#     Doppler span       >= 2500 Hz  the pass must actually exercise the predictor
#     tuning-stable                  result unchanged across a tracker parameter sweep
#
# Coverage this buys: 4 ground stations on 3 continents (42N/-72E Massachusetts,
# 34N/131E Japan, 52N/5E Netherlands, 29N/-82E Florida), 7 satellites, 3 bands
# (400.0, 401.6, 435-437 MHz), 4 modulations, max elevation 12-59 deg, Doppler spans
# 3.2-17.2 kHz. No single station's clock, coordinates or hardware can carry the test.
# --------------------------------------------------------------------------------------
CURATED = [
    (14635786, "Tightest pass in the whole survey: 13 Hz RMS, 31 Hz worst sample, and a "
               "structured component of only 16 Hz, so almost nothing is left over from "
               "orbit-element error. 51 deg elevation makes it the reference case."),
    (14645738, "Independent second STARS pass at the same station 10 days earlier, with "
               "different elements. Equally tight (14 Hz). Its fitted offset lands within "
               "0.1 ppm of the pass above, which is the fixture's own check that the "
               "constant offset is hardware and not a per-pass fudge."),
    (14649456, "Low oblique pass (12 deg) with a very large 10.9 kHz Doppler span -- a low "
               "pass sweeps nearly the full range-rate range, so this exercises the curve's "
               "extremes at a low slew rate. 19 Hz RMS."),
    (14649394, "Cleanest spectrum measured (isolation 0.03): an isolated CW beacon with no "
               "competing feature within +-1 kHz. Included as the case where the extraction "
               "itself is least in doubt."),
    (14645749, "Non-CW modulation (GMSK) and a 13.2 kHz span, so the fixture is not made "
               "entirely of pure-carrier beacons. 39 Hz RMS."),
    (14601253, "Different station (5049, Japan) and a third modulation (AFSK), with the "
               "second-largest span in the set at 17.2 kHz. Puts an eastern-hemisphere "
               "station geometry into the fixture."),
    (14653331, "Different station (1696, Netherlands) and a different band (400.032 MHz), "
               "on a 1986 satellite with well-determined elements. NOTE: this is the one "
               "admitted pass whose RMS moves under the tracker sweep (42-58 Hz) because a "
               "second signal sits 876 Hz away (isolation 0.34). Every setting still lands "
               "inside the admission band. Drop this entry if you want the strictest set."),
    (14648099, "Highest elevation admitted (59 deg) and a 15.6 kHz span, from a fourth "
               "station (5062, Florida) on 401.577 MHz. This is the pass with the fastest "
               "Doppler slew, where a predictor's time-varying error shows most."),
]

# Observations examined and deliberately excluded, kept so the choices stay auditable.
# Survey: 1161 vetted good/with-signal observations; 540 had no tEXt metadata, 458 could
# not be tracked for >=150 rows, 163 scored, 8 admitted.
REJECTED = [
    (14649562, "ORBCOMM FM-112, 83 deg, station 4806. Carrier isolation 0.74: the SDPSK "
               "spectrum has a shoulder at 74% of peak only ~400 Hz from the peak, so the "
               "tracker flips between them and the residual is strictly BIMODAL (17% of "
               "samples in the second mode). Ungated RMS 138 Hz. Recon rated ORBCOMM the "
               "cleanest source at ~16 Hz, but that came from an iterative residual gate "
               "that had thrown the second mode away; locking to one line does give 17 Hz, "
               "but no prediction-free tracker can stay locked. This was the most painful "
               "rejection -- it was the only clean high-elevation, second-station pass."),
    (14649565, "ORBCOMM FM-116, same bimodal SDPSK failure (isolation 0.70, RMS 129 Hz)."),
    (14649528, "HILAT, wideband FM, isolation 0.63 -- no usable single carrier."),
    (14656249, "ORIGAMISAT-2, 80 deg, station 5096. RMS 117 Hz with a smooth "
               "elevation-correlated -278 -> +105 Hz swing (structured 315 Hz) that a time "
               "shift does NOT explain, so it is orbit-element error, not predictor error. "
               "Rejected despite being the only high-elevation third-station candidate."),
    (14649443, "STARS, 62 deg. Smooth monotonic -107 -> +239 Hz residual (RMS 112, "
               "structured 350 Hz); a 4.7 s time shift collapses it to 13 Hz, i.e. pure "
               "along-track element error. Same satellite and station as two ADMITTED "
               "passes -- which is the point: staleness is per-pass, not per-satellite."),
    (14657362, "JAS-2, station 1696. RMS 266 Hz, structured 480 Hz, 41 s implied time "
               "shift. This is the stale-TLE case recon flagged; confirmed and excluded."),
    (14649457, "CUTE-1.7 second pass. RMS 55 Hz but structured 166 Hz -- over the 150 Hz "
               "gate. A 3.9 s time shift collapses it to 13 Hz."),
    (14650896, "SNUGLITE. RMS 66 Hz, over the gate; a 3.5 s shift collapses it to 16 Hz."),
    (14649450, "OBJECT J third pass. RMS 94 Hz, structured 169 Hz."),
    (14657533, "OBJECT AY. Doppler span only 608 Hz -- does not exercise the predictor."),
    (14630926, "FUNCUBE-1. Doppler span only 234 Hz; also a 172 s implied time shift."),
    (14590939, "ORIGAMISAT-2 at station 5024. RMS 35 Hz and structured only 21 Hz, but one "
               "372 Hz tracker excursion breaks the max-residual gate. Kept out rather "
               "than deleting the offending sample, which would be exactly the "
               "prediction-based cleaning this fixture refuses to do."),
    (25544, "ALL ISS observations (30 in the survey). FM voice/APRS is intermittent, so the "
            "track is gappy and a tracker bug could masquerade as a predictor result."),
    (14642992, "MARINA, station 4434, 145.925 MHz, 64 deg -- would have added a 5th "
               "station, a third band and the highest elevation in the set, and it has the "
               "cleanest spectrum measured (isolation 0.02). Rejected anyway: RMS 91 Hz, "
               "structured 210 Hz, and a 7.0 s implied time shift collapses it to 23 Hz, so "
               "it is stale elements. Adding it measurably weakens the gate (see above)."),
    (14590936, "ORIGAMISAT-2 at station 5024 (would have been a 6th station). RMS 86 Hz, "
               "structured 196 Hz. Same dilution problem."),
    (0, "540 of 1161 observations had no satnogs:wf-dat / satnogs:wf-plot tEXt chunks "
        "(older renderer) -- no way to calibrate the axes, so unusable."),
]


# ======================================================================================
# Fetching (cached; re-runnable)
# ======================================================================================

def _session():
    import requests
    s = requests.Session()
    s.headers["User-Agent"] = "nexus-doppler-fixture/1.0"
    return s


def fetch_observation(obsid, sess=None):
    """Observation metadata, including the TLE the station actually used."""
    os.makedirs(CACHE, exist_ok=True)
    fp = os.path.join(CACHE, f"obs_{obsid}.json")
    if os.path.exists(fp):
        return json.load(open(fp))
    sess = sess or _session()
    for _ in range(30):
        r = sess.get(f"{API}{obsid}/", params={"format": "json"}, timeout=90)
        if r.status_code == 429:                       # the API throttles hard; be polite
            wait = int(r.headers.get("Retry-After", 300)) + 5
            print(f"  throttled, sleeping {wait}s", file=sys.stderr, flush=True)
            time.sleep(wait); continue
        r.raise_for_status()
        o = r.json()
        json.dump(o, open(fp, "w"))
        return o
    raise RuntimeError(f"throttled out fetching {obsid}")


def fetch_waterfall(o, sess=None):
    """Waterfall PNG (public S3, not throttled)."""
    os.makedirs(CACHE, exist_ok=True)
    fp = os.path.join(CACHE, f"wf_{o['id']}.png")
    if os.path.exists(fp) and os.path.getsize(fp) > 10000:
        return fp
    if not o.get("waterfall"):
        raise RuntimeError(f"observation {o['id']} has no waterfall")
    sess = sess or _session()
    r = sess.get(o["waterfall"], timeout=180)
    r.raise_for_status()
    open(fp, "wb").write(r.content)
    return fp


# ======================================================================================
# Waterfall decode: PNG pixels -> calibrated (time, frequency, power) grid
# ======================================================================================

_LUT = (np.array(matplotlib.colormaps["viridis"](np.linspace(0, 1, 256)))[:, :3]
        * 255).round().astype(np.int64)


class NoMetadata(Exception):
    """PNG predates the metadata-emitting renderer; there is no way to calibrate it."""


def decode_waterfall(png_path):
    im = Image.open(png_path)
    if "satnogs:wf-dat" not in im.info or "satnogs:wf-plot" not in im.info:
        raise NoMetadata(png_path)
    wfdat = json.loads(im.info["satnogs:wf-dat"])
    wfplot = json.loads(im.info["satnogs:wf-plot"])

    a = np.array(im.convert("RGB"))
    # Locate the axes box: the waterfall is a solid (non-white) block. The figure holds
    # two such blocks -- the waterfall on the left, the colorbar on the right -- so take
    # the first contiguous run of filled columns.
    nw = a.sum(axis=2) < 720
    cols = np.where(nw.mean(axis=0) > 0.8)[0]
    if len(cols) == 0:
        raise NoMetadata(png_path)
    brk = np.where(np.diff(cols) > 1)[0]
    x0, x1 = cols[0], (cols[brk[0]] if len(brk) else cols[-1])
    rows = np.where(nw[:, x0:x1 + 1].mean(axis=1) > 0.9)[0]
    y0, y1 = rows[0], rows[-1]

    # Invert the viridis colormap. The render is nearest-neighbour, so each pixel is
    # exactly one of 256 LUT colours and the original 8-bit power index comes back.
    blk = a[y0 + 1:y1, x0 + 1:x1].astype(np.int64)
    key = (blk[:, :, 0] << 16) | (blk[:, :, 1] << 8) | blk[:, :, 2]
    uk, inv = np.unique(key, return_inverse=True)
    uc = np.stack([(uk >> 16) & 255, (uk >> 8) & 255, uk & 255], axis=1)
    d = ((uc[:, None, :] - _LUT[None, :, :]) ** 2).sum(axis=2)
    lev = d.argmin(axis=1).astype(np.float64)[inv].reshape(blk.shape[:2])
    rgb_err = float(np.sqrt(d.min(axis=1)).max())

    H, W = lev.shape
    fc = float(wfdat["center_freq"])
    kx0, kx1 = wfplot["xlim_kHz"]
    n0, n1 = wfplot["ylim_num"]
    # matplotlib imshow extent semantics: the axis limits are the DATA EDGES of the image,
    # so pixel i spans [lim0 + i/N*span, lim0 + (i+1)/N*span] and is centred at (i+0.5)/N.
    # ylim_num is a matplotlib datenum -- days since the 1970-01-01 epoch -- so x86400
    # gives POSIX seconds directly. (Cross-checks against wf-dat.timestamp + ylim_s[0].)
    f_abs = fc + (kx0 + (np.arange(W) + 0.5) / W * (kx1 - kx0)) * 1000.0
    unix = (n0 + (np.arange(H) + 0.5) / H * (n1 - n0)) * 86400.0
    return dict(lev=lev, f_abs=f_abs, unix=unix, fc=fc, wfdat=wfdat, wfplot=wfplot,
                hz_per_px=(kx1 - kx0) * 1000.0 / W,
                s_per_row=(n1 - n0) * 86400.0 / H, rgb_err=rgb_err)


# ======================================================================================
# Carrier tracking -- PREDICTION-FREE
# ======================================================================================

# Universal bound on |d(Doppler)/dt|, used ONLY to size the tracker's search window.
# At closest approach d(f_dop)/dt = -f_c * v^2 / (c*h); the worst case any Earth satellite
# presents (v = 7.8 km/s, h = 200 km) is f_c * 1.014e-6 Hz/s. Depends only on f_c, a PNG
# metadata field, and universal orbital mechanics -- never on this satellite's TLE.
MAX_DOPPLER_RATE_PER_HZ = 1.05e-6


def track_carrier(wf, snr_min=4.0, max_gap=25, seed_span=9, rate_margin=1.6, snr_rel=0.6,
                  lock_px=2.0, hist_len=15):
    """Follow the carrier through the waterfall. Sees ONLY the image.

    Row normalisation: subtract the row median (removes receiver passband shape and the
    renderer's per-row auto-scaling) and divide by the row MAD, giving a per-row SNR in
    sigma units.

    Seeding: a carrier persists across rows, noise does not, so seed at the argmax of the
    SNR map averaged over `seed_span` rows. Over 9 rows a LEO carrier moves only a few
    pixels, so the average stays coherent on signal while averaging noise down ~3x. Rows
    whose MAD collapsed (blank/uniform render rows) are excluded so they cannot spike.

    Walking: from the seed outward both ways, accept in each row the strongest pixel
    inside a search window, widened by (1+gap) while coasting through weak rows. Over a
    full 600-px row the max of pure noise runs ~3.2 sigma; inside a few-pixel window it
    runs ~2.3 sigma, so a 4-sigma accept is safe and many more rows survive than a naive
    global argmax could keep.

    The window is placed two ways. For the first few rows there is no drift estimate, so
    it is centred on the last accepted frequency and sized by the universal rate bound
    above. Once >=5 rows are in hand, the recent track is fit with a straight line and
    the window is centred on that extrapolation and narrowed to +-`lock_px` pixels.
    That is justified because knowing the current drift rate leaves only the ACCELERATION
    to cover: for a circular pass the peak |d2f/dt2| is 0.859 * f_c * v^3 / (c * h^2),
    which at v=7.8 km/s, h=200 km is f_c * 3.4e-8 Hz/s^2 -- under 0.5 Hz over one row.
    So +-2 px is many times the physically reachable deviation, and both bounds still
    come only from f_c and universal orbital mechanics, never from this satellite's TLE.

    Narrowing matters: it stops the track being dragged onto a neighbouring spectral
    feature. It is a purely local continuity constraint -- it never references a
    predicted curve, and it only selects WHICH peak to accept, never alters a measured
    value.

    Chosen parameters are not delicate: across the admitted observations, varying
    `snr_rel` over 0.0-0.8 and `rate_margin` over 1.0-3.0 changed RMS, max residual and
    sample count by 0. Observations whose result DID move under that sweep were rejected
    as ambiguous rather than tuned into agreement.
    """
    from scipy.ndimage import uniform_filter1d
    lev = wf["lev"]
    H, W = lev.shape
    step_max = MAX_DOPPLER_RATE_PER_HZ * wf["fc"] * wf["s_per_row"]
    win_hz = rate_margin * step_max + 1.5 * wf["hz_per_px"]
    win = max(2, int(round(win_hz / wf["hz_per_px"])))

    z = lev - np.median(lev, axis=1, keepdims=True)
    mad = 1.4826 * np.median(np.abs(z), axis=1, keepdims=True)
    if not (mad > 0).any():
        return dict(n_kept=0, n_rows=H)
    good_row = mad[:, 0] > 0.3 * np.median(mad[mad > 0])
    zn = z / np.where(mad > 0, mad, np.inf)

    pers = uniform_filter1d(np.where(good_row[:, None], zn, 0.0), size=seed_span, axis=0)
    seed_row, seed_col = np.unravel_index(
        np.argmax(np.where(good_row[:, None], pers, -np.inf)), pers.shape)

    def subpx(i, j):
        if 0 < j < W - 1:
            a_, b_, c_ = z[i, j - 1], z[i, j], z[i, j + 1]
            den = a_ - 2 * b_ + c_
            if den != 0:
                return float(np.clip(0.5 * (a_ - c_) / den, -1, 1))
        return 0.0

    col = np.full(H, -1, dtype=int)
    snr = np.zeros(H)
    col[seed_row], snr[seed_row] = seed_col, zn[seed_row, seed_col]
    for direction in (+1, -1):
        prev, gap = float(seed_col), 0
        hist = [(float(seed_row), seed_col + subpx(seed_row, seed_col))]
        i = seed_row + direction
        while 0 <= i < H:
            if len(hist) >= 5:
                hh = np.array(hist[-hist_len:])
                slope, icept = np.polyfit(hh[:, 0], hh[:, 1], 1)
                centre, w = slope * i + icept, lock_px * (1 + gap)
            else:
                centre, w = prev, win * (1 + gap)
            lo = max(0, int(math.floor(centre - w)))
            hi = min(W, int(math.ceil(centre + w)) + 1)
            if hi <= lo:
                break
            j = lo + int(np.argmax(zn[i, lo:hi]))
            if good_row[i] and zn[i, j] >= snr_min:
                col[i], snr[i], prev, gap = j, zn[i, j], float(j), 0
                hist.append((float(i), j + subpx(i, j)))
            else:
                gap += 1
                if gap > max_gap:
                    break
            i += direction

    # Relative-SNR gate: keep only rows whose peak is at least `snr_rel` of THIS pass's
    # typical peak. Where the signal fades the tracker coasts and the "peak" it finds is
    # noise -- the source of every gross outlier seen in testing. Judged against the
    # pass's own median strength, never against a predicted curve.
    ok = col >= 0
    if ok.sum() >= 20:
        floor = max(snr_min, snr_rel * float(np.median(snr[ok])))
        col[ok & (snr < floor)] = -1
        ok = col >= 0

    idx = np.where(ok)[0]
    if len(idx) == 0:
        return dict(n_kept=0, n_rows=H)
    # 3-point parabolic sub-pixel refinement of the peak location.
    sub = np.array([subpx(i, col[i]) for i in idx])
    return dict(rows=idx, unix=wf["unix"][idx],
                f_obs=wf["f_abs"][0] + (col[idx] + sub) * wf["hz_per_px"],
                snr=snr[idx], n_kept=len(idx), n_rows=H, win_hz=win_hz)


def carrier_isolation(wf, tr, half=14, guard=3):
    """Prediction-free purity metric: is there ONE carrier, or competing features?

    Stack every tracked row aligned on its own peak column, average, then compare the
    strongest feature OUTSIDE +-`guard` px of the peak against the peak. A clean CW or
    beacon carrier scores 0.03-0.16. A wideband modulated signal whose spectrum has
    competing humps scores ~0.7 -- ORBCOMM's SDPSK has a shoulder at 74% of peak only
    ~400 Hz away, so no tracker can stay on one feature and the residual goes bimodal.
    Uses only the image and the track; never a prediction. Admission needs <= 0.5.
    """
    lev = wf["lev"]
    W = lev.shape[1]
    z = lev - np.median(lev, axis=1, keepdims=True)
    mad = 1.4826 * np.median(np.abs(z), axis=1, keepdims=True)
    zn = z / np.where(mad > 0, mad, np.inf)
    cols = np.round((tr["f_obs"] - wf["f_abs"][0]) / wf["hz_per_px"]).astype(int)
    acc = [zn[i, c - half:c + half + 1]
           for i, c in zip(tr["rows"], cols) if half <= c < W - half]
    if len(acc) < 20:
        return float("nan")
    prof = np.mean(acc, axis=0)
    pk = prof[half]
    side = np.concatenate([prof[:half - guard], prof[half + guard + 1:]])
    return float(side.max() / pk) if pk > 0 else float("nan")


def stride_to(unix, f_obs, target=TARGET_POINTS):
    """Keep every k-th sample. No averaging: stored values stay untouched measurements."""
    k = max(1, int(round(len(unix) / float(target))))
    return unix[::k], f_obs[::k], k


# ======================================================================================
# Optional QC: independent Skyfield prediction + ONE fitted constant
# ======================================================================================

def _predict(o, unix, fc):
    import datetime as dt
    from skyfield.api import EarthSatellite, wgs84, load as skload
    global _TS
    try:
        ts = _TS
    except NameError:
        ts = _TS = skload.timescale()
    sat = EarthSatellite(o["tle1"], o["tle2"], (o.get("tle0") or "").strip(), ts)
    gs = wgs84.latlon(o["station_lat"], o["station_lng"], elevation_m=o["station_alt"])
    # POSIX seconds ignore leap seconds, so they must become a UTC CALENDAR instant.
    # ts.utc(1970,1,1,0,0,unix) is WRONG -- it treats them as elapsed UTC time and lands
    # 27 s early (27 leap seconds inserted since 1972). That error mimics a time bias and
    # silently inflates every residual.
    t = ts.from_datetimes([dt.datetime.fromtimestamp(float(u), dt.timezone.utc)
                           for u in np.atleast_1d(unix)])
    d = (sat - gs).at(t)
    p, v = d.position.m, d.velocity.m_per_s
    rr = (p * v).sum(axis=0) / np.linalg.norm(p, axis=0)
    return fc * (1.0 - rr / 299792458.0), d.altaz()[0].degrees


def _movmed(x, k):
    k = max(3, int(k) | 1)
    if len(x) < k:
        return x.copy()
    pad = np.pad(x, k // 2, mode="edge")
    return np.array([np.median(pad[i:i + k]) for i in range(len(x))])


def qc(o, unix, f_obs):
    """Residual stats under the exact model the Rust test uses: ONE constant offset."""
    fc_pred, elev = _predict(o, unix, o["_fc"])
    r = f_obs - fc_pred
    off = float(r.mean())
    res = r - off
    dop = fc_pred - o["_fc"]
    sm = _movmed(res, len(res) * 0.12)
    dfdt = np.gradient(fc_pred, unix)
    A = np.stack([np.ones_like(dfdt), dfdt], axis=1)
    coef, *_ = np.linalg.lstsq(A, r, rcond=None)
    return dict(n=len(res), rms=float(np.sqrt((res ** 2).mean())),
                mean=float(res.mean()), std=float(res.std()),
                p95=float(np.percentile(np.abs(res), 95)), max=float(np.abs(res).max()),
                struct=float(np.abs(sm).max()), offset_hz=off,
                ppm=off / o["_fc"] * 1e6, maxel=float(elev.max()), minel=float(elev.min()),
                span=float(dop.max() - dop.min()),
                dt_bias_s=float(coef[1]),
                rms_if_time_fitted=float(np.sqrt(((r - A @ coef) ** 2).mean())))


# ======================================================================================
# Build
# ======================================================================================

def extract(obsid, sess=None):
    o = fetch_observation(obsid, sess)
    wf = decode_waterfall(fetch_waterfall(o, sess))
    tr = track_carrier(wf)
    if tr["n_kept"] < 150:
        raise RuntimeError(f"obs {obsid}: only {tr['n_kept']} tracked rows")
    o["_fc"] = wf["fc"]
    return o, wf, tr


def build(do_qc):
    sess = _session()
    out, stats = [], []
    for obsid, note in CURATED:
        o, wf, tr = extract(obsid, sess)
        unix, f_obs, k = stride_to(tr["unix"], tr["f_obs"])
        if do_qc:
            s = qc(o, unix, f_obs)
            s["id"] = obsid
            s["sat"] = (o.get("tle0") or "").strip()
            s["stn"] = o["ground_station"]
            stats.append(s)
        out.append(dict(
            observation_id=obsid,
            satellite=(o.get("tle0") or "").strip(),
            norad_id=o["norad_cat_id"],
            line1=o["tle1"], line2=o["tle2"],
            station_id=o["ground_station"],
            station_lat=o["station_lat"], station_lng=o["station_lng"],
            station_alt_m=o["station_alt"],
            center_freq_hz=wf["fc"],
            transmitter_mode=o.get("transmitter_mode"),
            start_utc=o["start"], end_utc=o["end"],
            stride=k,
            samples=[dict(unix=round(float(u), 3), observed_hz=round(float(f), 1))
                     for u, f in zip(unix, f_obs)],
            notes=note,
        ))
        print(f"  {obsid} {out[-1]['satellite']:<22} {len(out[-1]['samples']):>4} samples "
              f"(stride {k} of {tr['n_kept']})", flush=True)

    doc = dict(
        _README=("Real recorded satellite Doppler tracks from SatNOGS (CC BY-SA 4.0). "
                 "observed_hz is the RAW measured absolute carrier frequency. The consumer "
                 "MUST fit exactly ONE constant frequency offset per observation "
                 "(satellite TCXO + station LO error) before comparing to a prediction; "
                 "fitting anything more would absorb real predictor error. This validates "
                 "Doppler SHAPE over a pass, never absolute frequency. See generate.py."),
        _source="https://network.satnogs.org/api/observations/<id>/",
        _generator="generate.py",
        observations=out,
    )
    with open(OUT, "w") as fh:
        json.dump(doc, fh, indent=1)
    print(f"\nwrote {OUT}  ({os.path.getsize(OUT)/1024:.0f} KB, "
          f"{len(out)} observations, {sum(len(x['samples']) for x in out)} samples)")

    if stats:
        print(f"\n{'obsid':>9} {'sat':<20} {'stn':>5} {'n':>4} {'RMS':>6} {'p95':>6} "
              f"{'max':>6} {'strct':>6} {'maxel':>5} {'span':>6} {'ppm':>7} {'dtb':>6}")
        for s in stats:
            print(f"{s['id']:>9} {s['sat'][:20]:<20} {s['stn']:>5} {s['n']:>4} "
                  f"{s['rms']:6.1f} {s['p95']:6.1f} {s['max']:6.1f} {s['struct']:6.1f} "
                  f"{s['maxel']:5.0f} {s['span']:6.0f} {s['ppm']:+7.1f} {s['dt_bias_s']:+6.2f}")
        allr = np.array([s["rms"] for s in stats])
        print(f"\nworst observation RMS {allr.max():.1f} Hz over {len(stats)} observations")


def survey(n):
    """Score recent vetted observations to find new fixture material."""
    import requests
    sess = _session()
    pool, url, params = [], API, {"waterfall_status": 1, "status": "good", "format": "json"}
    while len(pool) < n:
        r = sess.get(url, params=params if url == API else None, timeout=90)
        if r.status_code == 429:
            w = int(r.headers.get("Retry-After", 300)) + 5
            print(f"  throttled {w}s", file=sys.stderr); time.sleep(w); continue
        r.raise_for_status()
        pool.extend(r.json())
        nxt = None
        for part in r.headers.get("Link", "").split(","):
            m = re.search(r'<([^>]+)>;\s*rel="next"', part)
            if m:
                nxt = m.group(1)
        if not nxt:
            break
        url = nxt
        time.sleep(20)
    print(f"{'obsid':>9} {'sat':<20} {'stn':>5} {'RMS':>7} {'max':>7} {'strct':>7} {'maxel':>5} {'span':>6}")
    for o in pool[:n]:
        if o["norad_cat_id"] == 25544 or not o.get("waterfall") or not o.get("tle1"):
            continue
        try:
            wf = decode_waterfall(fetch_waterfall(o, sess))
            tr = track_carrier(wf)
            if tr["n_kept"] < 150:
                continue
            o["_fc"] = wf["fc"]
            u, f, _ = stride_to(tr["unix"], tr["f_obs"])
            s = qc(o, u, f)
            print(f"{o['id']:>9} {(o.get('tle0') or '').strip()[:20]:<20} "
                  f"{o['ground_station']:>5} {s['rms']:7.0f} {s['max']:7.0f} "
                  f"{s['struct']:7.0f} {s['maxel']:5.0f} {s['span']:6.0f}", flush=True)
        except Exception:
            continue


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--qc", action="store_true", help="cross-check with Skyfield")
    ap.add_argument("--survey", type=int, metavar="N", help="score N recent candidates")
    a = ap.parse_args()
    if a.survey:
        survey(a.survey)
    else:
        build(a.qc)
