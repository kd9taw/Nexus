#!/usr/bin/env python3
"""Generate the independent look-angle / range-rate reference for `sat.rs`.

WHY THIS EXISTS
---------------
Nexus computes satellite Doppler from a range-rate it derives itself. The SGP4
propagator underneath is not the risk: the `sgp4` crate already runs the
published AFSPC/Vallado verification vectors (33 cases, position asserted to
1e-6 km and velocity to 1e-9 km/s), so orbital state is a solved, externally
validated problem.

What is OURS, and therefore what needs an independent check, is everything
above the propagator:

  * the TEME -> ECEF rotation (sidereal angle, and its sense),
  * the observer's position from a Maidenhead grid,
  * the topocentric geometry that turns two ECEF vectors into az/el/range,
  * the range-rate, including the `omega x r` term for the OBSERVER's own
    motion as the Earth turns -- which is ~0.46 km/s at mid latitudes and the
    single easiest term to omit and never notice, because the result still
    looks like a plausible Doppler curve.

A sign error or a missing term in any of those produces frequencies that are
confidently wrong. The classic way to catch that is to diff against a
reference implementation, so this script emits one from Skyfield -- which
implements the same physics independently, with proper time scales -- and the
Rust side asserts against it.

Skyfield is NOT a build dependency: this writes a fixture, the fixture is
committed, and the Rust test reads it. Regenerate only when adding cases.

    python3 generate.py > golden.json

Cases are chosen for the terms they stress, not for tidiness -- see CASES.
"""

import json
from datetime import datetime, timezone
import sys

from skyfield.api import EarthSatellite, load, wgs84

# REAL element sets only. Every one below except the ISS is taken verbatim from
# the AIAA-2006-6753 ("Revisiting Spacetrack Report #3") SGP4 verification set,
# which ships as the `sgp4` crate's own test data -- so these are the same
# elements the propagator underneath is verified against, and they are known to
# propagate without diverging.
#
# Two things worth recording about getting these right, because both are
# silent failure modes:
#
#  1. An earlier draft used plausible-looking element sets that were made up.
#     Skyfield returned NaN for them -- which is the good outcome: invented
#     orbital elements are not a test case, they are a fabrication that happens
#     to parse. The generator now refuses to emit a non-finite sample.
#  2. A single mis-transcribed digit in the Molniya line 1 was accepted by
#     Skyfield without complaint and rejected by the Rust side on the TLE
#     CHECKSUM, which is what that last digit is for. Skyfield does not verify
#     it; `sgp4::Elements::from_tle` does. If this fixture is ever regenerated,
#     copy the lines from the crate's `tests/test_cases.toml` rather than
#     retyping them.
TLES = {
    # Low Earth orbit, 51.6 deg inclination: fast, big Doppler, the case every
    # satellite operator actually flies. Same element set as the sat.rs unit
    # tests, and itself a published Vallado example.
    "ISS": (
        "1 25544U 98067A   08264.51782528 -.00002182  00000-0 -11606-4 0  2927",
        "2 25544  51.6416 247.4627 0006703 130.5360 325.0288 15.72125391563537",
    ),
    # Near-circular LEO at 58 deg -- the shape most amateur satellites fly.
    "LEO_CIRC": (
        "1 06251U 62025E   06176.82412014  .00008885  00000-0  12808-3 0  3985",
        "2 06251  58.0579  54.0425 0030035 139.1568 221.1854 15.56387291  6774",
    ),
    # Sun-synchronous polar, essentially zero eccentricity: a pass goes almost
    # over the pole, which is where azimuth changes fastest and a sidereal-angle
    # error is most visible.
    "LEO_POLAR": (
        "1 28057U 03049A   06177.78615833  .00000060  00000-0  35940-4 0  1836",
        "2 28057  98.4283 247.6961 0000884  88.1964 271.9322 14.35478080140550",
    ),
    # Geostationary, inclination 0.02 deg. THE case for the observer-rotation
    # term: the satellite is nearly fixed overhead, so almost all of the
    # range-rate comes from the ground station being carried east at ~0.35 km/s.
    # An implementation that forgets `omega x r` is wrong by essentially the
    # whole quantity here, while a LEO case would bury the same bug under a much
    # larger satellite velocity.
    "GEO": (
        "1 26900U 01039A   06106.74503247  .00000045  00000-0  10000-3 0  8290",
        "2 26900   0.0164 266.5378 0003319  86.1794 182.2590  1.00273847 16981",
    ),
    # Molniya-class: e = 0.71, 12 h period, deep-space (SDP4) regime. Range-rate
    # swings across a huge dynamic range and the geometry is nothing like a
    # circular orbit, so anything that quietly assumes near-circular fails.
    "MOLNIYA": (
        "1 09880U 77021A   06176.56157475  .00000421  00000-0  10000-3 0  9814",
        "2 09880  64.5968 349.3786 7069051 270.0229  16.3320  2.00813614112380",
    ),
}

# (label, tle key, latitude, longitude) -- longitudes deliberately include a
# negative (western), a far-eastern one near the date line, and a southern
# latitude, because sign and wrap errors in the sidereal rotation survive a
# single mid-northern test site perfectly happily.
CASES = [
    ("iss_midlat_north", "ISS", 41.9, -88.0),          # EN52-ish, the operator's own grid
    ("iss_high_north", "ISS", 68.4, 17.4),             # inside the Arctic circle
    ("iss_south", "ISS", -33.9, 151.2),                # southern hemisphere, east
    ("iss_dateline", "ISS", 21.3, -157.9),             # far west, near the date line
    ("iss_equator", "ISS", 0.0, 0.5),                  # equator + prime meridian
    ("leo_circ_midlat", "LEO_CIRC", 41.9, -88.0),      # the amateur-satellite shape
    ("leo_polar_high_north", "LEO_POLAR", 68.4, 17.4), # overhead-ish polar pass
    ("leo_polar_south", "LEO_POLAR", -45.9, 170.5),    # southern, high latitude
    ("geo_midlat_north", "GEO", 41.9, -88.0),          # observer rotation dominates
    ("geo_equator", "GEO", 0.0, 266.5),                # nearly overhead, tiny range-rate
    ("molniya_north", "MOLNIYA", 55.8, 37.6),          # what the orbit was designed for
    ("molniya_west", "MOLNIYA", 61.2, -149.9),         # the other apogee lobe
]

# Sample window per case: a wide sweep at coarse steps to cover many geometries
# (above AND below the horizon), plus the sub-minute steps that matter for a
# rate. Times are seconds from the element-set epoch.
OFFSETS_S = (
    [i * 60 for i in range(0, 720, 7)]      # 12 h at 7 min steps
    + [i * 10 for i in range(0, 360)]       # 1 h at 10 s steps
)


def main() -> int:
    ts = load.timescale()
    out = {
        "_comment": (
            "Independent reference for crates/propagation/src/sat.rs, produced by "
            "Skyfield. See generate.py for why each case is here. Do not hand-edit."
        ),
        "cases": [],
    }

    for label, tle_key, lat, lon in CASES:
        l1, l2 = TLES[tle_key]
        sat = EarthSatellite(l1, l2, tle_key, ts)
        site = wgs84.latlon(lat, lon)
        epoch_unix = int(sat.epoch.utc_datetime().timestamp())

        samples = []
        for dt in OFFSETS_S:
            # Evaluate at the instant this sample is LABELLED with, not at
            # (true epoch + dt).
            #
            # ⭐ This used to step in TT from `sat.epoch.tt` while stamping the
            # sample `epoch_unix + dt`, where `epoch_unix` had been truncated to
            # a whole second. Those are different instants — by the fraction of a
            # second thrown away, up to ~7.6 km of LEO motion. The fixture
            # therefore described the sky a fraction of a second before the time
            # it claimed, and sat.rs had the SAME truncation in its own epoch, so
            # the two errors cancelled and this comparison looked excellent
            # (worst range Δ 0.41 km) while both sides were wrong together.
            #
            # Real recorded carriers broke the tie: see tests/sat_doppler_real.rs.
            # A reference must be independent of the thing it checks, which means
            # it has to be right about its own timestamps.
            stamp = epoch_unix + dt
            t = ts.from_datetime(datetime.fromtimestamp(stamp, timezone.utc))
            diff = (sat - site).at(t)
            alt, az, dist = diff.altaz()
            # Range-rate is the radial component of the relative velocity: the
            # projection of the topocentric velocity onto the line of sight.
            # Positive = receding, which is the convention sat.rs pins.
            pos = diff.position.km
            vel = diff.velocity.km_per_s
            rng = (pos[0] ** 2 + pos[1] ** 2 + pos[2] ** 2) ** 0.5
            rate = (pos[0] * vel[0] + pos[1] * vel[1] + pos[2] * vel[2]) / rng

            # A reference that emits NaN is not a reference. This fires when an
            # element set does not actually propagate -- which is exactly what
            # invented TLEs do -- and refusing here is what stops a fabricated
            # fixture from being committed and then "passing".
            vals = (az.degrees, alt.degrees, dist.km, rate)
            if not all(v == v and abs(v) != float("inf") for v in vals):
                raise SystemExit(
                    f"{label}: non-finite reference at +{dt}s ({vals}) — the element "
                    f"set for {tle_key} does not propagate. Use a real one."
                )

            samples.append(
                {
                    "unix": epoch_unix + dt,
                    "az_deg": round(az.degrees % 360.0, 6),
                    "el_deg": round(alt.degrees, 6),
                    "range_km": round(dist.km, 6),
                    "range_rate_km_s": round(rate, 9),
                }
            )

        out["cases"].append(
            {
                "label": label,
                "name": tle_key,
                "line1": l1,
                "line2": l2,
                "observer_lat": lat,
                "observer_lon": lon,
                "samples": samples,
            }
        )

    json.dump(out, sys.stdout, indent=1)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
