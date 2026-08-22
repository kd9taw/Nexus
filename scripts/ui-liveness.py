#!/usr/bin/env python3
"""ui-liveness — click a control in the REAL app and prove something actually happened.

WHY THIS EXISTS, in the project's own words (CLAUDE.md, "What the stop-line guards do NOT
prove"):

    neither sweep can see a stop control that is PRESENT, ENABLED AND INERT
    (an `onClick={() => {}}` on Stop TX passed the whole suite — 2106 tests at the time)

That is a hole in a TRANSMIT-SAFETY rule. `stop-line.test.tsx` proves a control is in the
document, findable by accessible name, and no more disabled than it was. It cannot prove the
handler does anything: jsdom paints no pixels, and a mocked IPC layer returns whatever it is
told to. A Stop TX wired to nothing passes every gate the project has.

This drives the real Tauri binary on a virtual display and injects a real click through XTEST —
the X server's own extension, indistinguishable to the app from a human — then looks.

── ONE REGION, WATCHED SOMEWHERE ELSE ────────────────────────────────────────────────────────
The verdict comes from the EFFECT region — the TX-state readout — and deliberately NOT from the
button. A stop control's job is to change something ELSEWHERE (drop the latch, flip the
indicator); watching the button alone would see its own hover glow and call that success.

  LIVE   the effect region moved after the click.
  INERT  it did not. That is the finding worth having.

⚠️ A HOVER PROBE WAS TRIED AS AN AIM CHECK AND REMOVED, because it does not work in this UI.
The idea was to detect a coordinate that had rotted after a relayout — hover the target first,
and if nothing highlights, blame the table rather than the app. Measured on 1.7.6, EVERY point
tried responds to the pointer: empty Call-Roster space 166px, the banner strip 230px, a gap in
the roster header 368px, bare waterfall 80px, below the nav rail 56px. Row highlights, the
waterfall crosshair and hover targets cover nearly the whole window, so "did anything highlight"
answers yes everywhere and certifies nothing. Tuning a threshold until it agreed would have been
fitting the check to the answer.
The stale-coordinate guard is therefore the POSITIVE CONTROL instead: if the layout moves, the
control click misses too, reports INERT, and the run is declared broken rather than believed.

── THE NOISE FLOOR IS LOAD-BEARING ───────────────────────────────────────────────────────────
The first version compared captures for byte-equality and called any difference a reaction. It
reported a click on EMPTY SPACE as LIVE, because parts of this UI animate whether you touch
them or not — measured on 1.7.6: the `Tune` button repaints ~950 of its ~1150 px continuously,
while Call CQ, TX Off, Stop TX and the state readout sit at exactly 0. A tool that answers LIVE
for everything is worse than no tool: it certifies the very defect it was built to catch.

So every probe MEASURES what changes while nothing is done and demands the click beat it. That
is the project's own rule — a check that found nothing is not a result until something that
must trip it does — turned on the tool itself.

── WHAT IT STILL CANNOT PROVE ────────────────────────────────────────────────────────────────
That the RIGHT thing happened. A Stop TX that repaints the indicator and sends nothing to the
radio passes here; proving the command reaches the rig needs hardware on a bench. It says
nothing about Windows or macOS, and nothing that needs live decodes: with no sound card the
radio engine stops at launch, so cockpit panes stay empty.

That last point BOUNDS WHAT MAY BE PROBED, and it is not a detail. A control whose whole effect
is on the radio — Tune is the clear case — has nothing to show when there is no radio, so it
reads INERT here for an honest reason. Only probe controls whose effect is VISIBLE IN THE UI
regardless of hardware: the TX-enable latch, Stop TX dropping that latch, a pane toggle. When in
doubt leave it out; a false transmit-safety finding costs more than a missing line.

── SETUP (no sudo; nothing installed system-wide) ────────────────────────────────────────────
    Xvfb :99 -screen 0 1920x1080x24 &
    python3 -m venv /tmp/xvenv && /tmp/xvenv/bin/pip install python-xlib
    cargo build --release --manifest-path src-tauri/Cargo.toml --features radio,custom-protocol
    DISPLAY=:99 ./src-tauri/target/release/Nexus --profile liveness &     # dismiss the wizard
    /tmp/xvenv/bin/python scripts/ui-liveness.py --display :99 --cockpit operate

1920x1080 is not arbitrary: the app's auto-zoom is min(w/1200, h/900) capped at 100%, so at
that size it renders un-zoomed and the coordinates below mean what they say.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import time
from pathlib import Path

from Xlib import X, display
from Xlib.ext.xtest import fake_input

# The TX-state readout ("TX off / Receiving / Listening"). Measured idle noise: 0 px.
EFFECT = (495, 448, 200, 30)

# Coordinates reused as SETUP steps.
TX_TOGGLE = (279, 462)   # arms / disarms TX — visible in EFFECT
STOP_TX = (391, 462)     # drops the latch; idempotent, so safe to use for normalising

# name, click x, y, and SETUP — clicks that put the app in the state where this control has
# something to do. Setup clicks are not judged; they exist so the verdict does not depend on
# what the previous run left behind.
# Operate's stop-line census is Stop TX / Tune / Esc; TX On/Off is explicitly NOT a stop
# control, which is what makes it the right positive control — its effect is unmistakable and
# it is not the thing under judgement. It MUST be first in every list.
COCKPITS = {
    "operate": [
        ("TX On/Off  [positive control]", 279, 462, [TX_TOGGLE]),
        # Normalise to TX-off (Stop TX is idempotent), THEN arm. Without this the verdict flips
        # with whatever the last run left behind — observed doing exactly that on 1.7.6.
        ("Stop TX", 391, 462, [STOP_TX, TX_TOGGLE]),
        # ⚠️ TUNE IS DELIBERATELY NOT PROBED HERE, and the reason is a limit of this rig rather
        # than a fact about the button. Probed on 1.7.6 it came back INERT — no change in the
        # state readout, and 1964px whole-screen against a 1945px idle floor, i.e. nothing at
        # all. That is what a tune carrier SHOULD do with no radio: the engine is stopped, so
        # there is no transmission to start. "Guarded no-op with no rig" and "handler wired to
        # nothing" look identical from here, and only a bench can separate them.
        # Listing it would hand somebody a transmit-safety defect that may not exist.
        # ("Tune", 335, 462),
    ],
    # Proves the tool can REPORT a failure and not only a pass: clicking empty Call-Roster space
    # must come back INERT. A run where it does not means the discriminator has stopped
    # discriminating and no verdict from this tool should be believed.
    "selftest": [
        ("TX On/Off  [positive control]", 279, 462, [TX_TOGGLE]),
        ("empty roster space  [must be INERT]", 620, 800, [STOP_TX, TX_TOGGLE]),
    ],
}


def grab(disp: str, region) -> bytes:
    """Raw rgb24 pixels. Raw, not PNG: encoded bytes differ for reasons the pixels do not."""
    x, y, w, h = region
    r = subprocess.run(
        ["ffmpeg", "-hide_banner", "-loglevel", "error", "-f", "x11grab", "-video_size",
         f"{w}x{h}", "-i", f"{disp}+{x},{y}", "-frames:v", "1", "-pix_fmt", "rgb24",
         "-f", "rawvideo", "-"],
        check=True, capture_output=True,
        env={"DISPLAY": disp, "PATH": "/usr/bin:/bin:" + str(Path.home() / "bin")},
    )
    return r.stdout


def moved(a: bytes, b: bytes) -> int:
    n = min(len(a), len(b)) // 3
    return sum(1 for i in range(0, n * 3, 3) if a[i:i + 3] != b[i:i + 3])


class Pointer:
    PARK = (960, 950)  # away from every control, so a baseline carries no hover styling

    def __init__(self, disp: str):
        self.d = display.Display(disp)

    def move(self, x: int, y: int) -> None:
        fake_input(self.d, X.MotionNotify, x=x, y=y)
        self.d.sync()
        time.sleep(0.3)

    def park(self) -> None:
        self.move(*self.PARK)

    def click(self, x: int, y: int) -> None:
        self.move(x, y)
        fake_input(self.d, X.ButtonPress, 1)
        self.d.sync()
        time.sleep(0.1)
        fake_input(self.d, X.ButtonRelease, 1)
        self.d.sync()
        time.sleep(0.7)


def noise_of(disp: str, region) -> int:
    """Pixels that change in `region` while nothing is done. Two intervals, worst case."""
    a = grab(disp, region)
    time.sleep(1.1)
    b = grab(disp, region)
    time.sleep(1.1)
    c = grab(disp, region)
    return max(moved(a, b), moved(b, c))


def probe(ptr: Pointer, disp: str, x: int, y: int, setup) -> tuple[str, str]:
    """Put the app in a state where this control HAS something to do, then judge the click.

    ⚠️ WITHOUT THE SETUP THIS TEST LIES, and it was caught doing so: Stop TX reported LIVE on one
    run and INERT on the next with no code change between them, because its only visible effect
    is dropping an armed TX latch — and whether TX was armed depended on what the previous run
    happened to leave behind. A verdict that depends on run order is not a verdict.
    """
    for sx, sy in setup:
        ptr.click(sx, sy)
    ptr.park()
    time.sleep(0.4)

    # Generous margin on purpose: a false INERT sends somebody hunting a transmit-safety bug
    # that is not there, which is worse than missing a subtle control.
    gate = max(noise_of(disp, EFFECT) * 3, 40)

    ptr.park()
    before = grab(disp, EFFECT)
    ptr.click(x, y)
    ptr.park()
    time.sleep(0.5)
    delta = moved(before, grab(disp, EFFECT))
    return ("LIVE" if delta > gate else "INERT"), f"effect moved {delta}px, gate {gate}px"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--display", default=":99")
    ap.add_argument("--cockpit", default="operate", choices=sorted(COCKPITS))
    args = ap.parse_args()

    ptr = Pointer(args.display)
    results = []
    for name, x, y, setup in COCKPITS[args.cockpit]:
        verdict, why = probe(ptr, args.display, x, y, setup)
        results.append((name, verdict))
        print(f"  {verdict:<10} {name}")
        print(f"             {why}")

    ctl_name, ctl_verdict = results[0]
    if ctl_verdict != "LIVE":
        print(f"\nHARNESS BROKEN — the positive control ({ctl_name}) came back {ctl_verdict}, so "
              f"every line above is meaningless. Check the app is on this display, its window is "
              f"1920x1080, and the wizard has been dismissed.")
        return 2

    if args.cockpit == "selftest":
        # Inverted on purpose: a clean pass here would mean the tool stopped discriminating.
        ok = results[1][1] == "INERT"
        print("\nOK — the tool can still tell empty space from a working control." if ok else
              "\nSELFTEST FAILED — empty space did not report INERT, so this tool can no longer "
              "detect an inert control and its verdicts must not be trusted.")
        return 0 if ok else 1

    bad = [n for n, v in results[1:] if v != "LIVE"]
    if bad:
        print("\nFAIL — a control that should act does not:")
        for n in bad:
            print(f"  {n}")
        print("\nAn INERT stop control is a transmit-safety defect: the operator presses it and "
              "the transmission carries on. Before believing that, check the coordinate still "
              "lands on the control — a relayout moves them.")
        return 1

    print(f"\nOK — every probed control in '{args.cockpit}' reacted, positive control included.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
