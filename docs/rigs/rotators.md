# Antenna Rotator Setup

> **Field status:** Rotator control is **dummy-verified** — the full path
> (config, compass, satellite auto-track) is exercised against Hamlib's dummy
> rotator. Control of **real azimuth/elevation hardware is pending** field
> testing. Set it up now with the dummy, and send a report when you point real
> iron.

Nexus drives rotators through Hamlib's `rotctld` — bundled in the Windows
installer and launched for you, the same way CAT is. You never run `rotctld` by
hand.

> **macOS and Linux:** Hamlib is **not** bundled for you. Install it once —
> `brew install hamlib` on a Mac, `sudo apt install libhamlib-utils` on
> Debian/Ubuntu (the Nexus `.deb` pulls it in; the AppImage cannot, so it has to
> be installed by hand). WSJT-X working proves only the Hamlib *library* is
> there — Nexus needs the `rotctld` *program*.

---

## Setup

Everything lives in **Settings ▸ Radio ▸ Rotator**:

1. Pick your **rotator model** from the dropdown.
2. Set its **serial port** (COM7 on Windows, `/dev/cu.usbserial-…` on macOS,
   `/dev/ttyUSB1` on Linux). **The baud fills itself in from the model** —
   see below.
3. **Save.** Nexus launches the control daemon automatically.

That's it — no separate daemon, no hand-run commands.

<!-- TODO: capture screenshot — Rotator model, port, and baud in the Radio tab’s Rotator section -->

### The baud belongs to the model

This is the single most common way a rotator "doesn't work": the line rate is
wrong, so the controller never answers, and that looks exactly like broken
hardware or a dead port.

There is no universal rotator baud. Each Hamlib backend declares its own, and
Nexus reads them out of the very Hamlib that ships in the installer:

| Rate | Models |
|---|---|
| **600** | SPID Rot2Prog |
| **1200** | SPID Rot1Prog · SARtek-1 |
| **4800** | Idiom Press Rotor-EZ · Hy-Gain DCU-1/DCU-1X · Hy-Gain DCU2/DCU3/YRC-1 · Green Heron RT-21 · DF9GR ERC |
| **9600** | M2 RC2800 · Prosistel (all) · Celestron NexStar · Meade LX200 |
| a range, so **yours to set** | GS-232 family (150–9600) · EasyComm II/III (9600–19200) · SPID MD-01/02 (600–460800) |

Picking your model fills the box in for you where there is one right answer, and
leaves it alone where there isn't. If you are upgrading and your rotator has
never worked, **re-pick your model** — the hint under the baud box will tell you
in words if the saved number cannot work.

### No hardware? Test with the dummy

You can wire up and exercise the whole rotator UI with no rotator attached, two
ways:

- **In-app:** choose **Dummy (testing — no hardware)** as the model and Save —
  Nexus runs the dummy daemon for you.
- **External:** run `rotctld -m 1` in a terminal, then put `127.0.0.1:4533` in
  the **External rotctld (advanced)** field (it overrides the model/port above).

Either way the compass needle starts tracking within about 2 seconds; click the
rose to slew it and watch the readout follow.

---

## Curated rotator models

Selectable in the dropdown; `rotctl -l` lists every model your Hamlib knows, and
**Other Hamlib model #…** lets you type any number directly. **(az)** and
**(az/el)** are what the Hamlib backend itself declares, not a guess.

| Model | Hamlib # |
|---|---|
| Yaesu GS-232A (az/el) | 601 |
| Yaesu GS-232B (az/el) | 603 |
| GS-232 (generic, az/el) — also EA4TX ARS-USB, LVB, ST2 | 602 |
| Yaesu/Kenpro GS-23 (az/el) | 605 |
| Yaesu/Kenpro GS-232 (az/el) | 606 |
| AMSAT LVB Tracker (az/el) | 607 |
| SPID Rot2Prog (az/el) | 901 |
| SPID Rot1Prog (az) | 902 |
| SPID MD-01/02, ROT2 mode (az/el) | 903 |
| EasyComm II | 202 |
| EasyComm III | 204 |
| Idiom Press Rotor-EZ (az) | 401 |
| Hy-Gain DCU-1/DCU-1X (az) | 403 |
| Hy-Gain DCU2/DCU3/YRC-1 (az) | 406 |
| DF9GR ERC (az) | 404 |
| Green Heron RT-21 | 405 |
| M2 RC2800 (az/el) | 1001 |
| Prosistel D (az) | 1701 |
| Prosistel Combi-Track (az/el) | 1703 |
| Dummy (testing — no hardware) | 1 |

> **EA4TX ARS owners:** there is no EA4TX entry, deliberately. Hamlib's ARS
> backend (1101/1102) drives a **parallel port**, which Nexus does not offer —
> it could never have worked with the serial port and baud the picker asks for.
> An **ARS-USB** speaks GS-232, so use **GS-232 (generic)** with the ARS's own
> COM port.

There's also an **External rotctld (advanced)** field: enter a `host:port` to
point Nexus at a `rotctld` you run yourself (or one on another machine). It
overrides the model/port picker above and stops the integrated daemon. The port
is required — `192.168.1.50` on its own is not an address.

---

## Where the rotator shows up

Once it's configured and answering, rotator control appears throughout the app:

- **Rotor pane in Connect** — a full rose you can click to slew, with a STOP
  control. The heading reads in true degrees with magnetic beside it, e.g.
  `312°T (316°M)` (WMM2025 declination). A rotator that **cannot report its
  position** (the Hy-Gain DCU-1 is one — its Hamlib backend has no read-back at
  all) keeps the pane, the slew and the STOP, and shows `—°T` instead of a
  needle.
- **RotorStrip in the Phone, CW and Operate cockpits** — a compact heading strip.
  It **hides when there's nothing to show**, and displays **"ROTOR —"** when a
  rotator is configured but not answering, so you can tell "no rotator" from
  "rotator not responding" at a glance. Click it to land on the model and port.
- **↗ on Needed-board rows** — point the antenna at a spotted station.

<!-- TODO: capture screenshot — Connect compass pane showing 312°T (316°M) with the slew rose -->

---

## Satellite pass auto-track

Pair a rotator with the **Satellites** section and Nexus tracks a pass for you:
it **arms** ahead of the pass, **prepositions** the antenna toward the
acquisition point, then **tracks** the bird across the sky. On an **azimuth-only**
rotator it falls back to azimuth tracking (no elevation), which is the right
behavior for a typical az rotator working low-orbit birds. Combined with an
IC-9700 (see [icom.md](icom.md)), this is a complete hands-off satellite station.

**Allow flip** (Settings ▸ Radio ▸ Rotator) is for a mount that can drive past
90° elevation. On a pass above 85° it keeps the antenna on the bearing the bird
rose on and runs elevation up through 90° and out the far side, instead of
swinging the mast 180° at the top of the pass. Many rotators cannot mechanically
do this — check your controller before turning it on.

<!-- TODO: capture screenshot — satellite pass with the rotor auto-tracking az/el -->

---

*Compass reads "ROTOR —" and won't move, or the daemon won't start? See
[Troubleshooting ▸ Rotator](../troubleshooting.md#rotator).*
