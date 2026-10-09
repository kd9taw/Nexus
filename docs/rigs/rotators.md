# Antenna Rotator Setup

> **Field status:** Rotator control is **dummy-verified** — the full path
> (config, compass, satellite auto-track) is exercised against Hamlib's dummy
> rotator. Control of **real azimuth/elevation hardware is pending** field
> testing. Set it up now with the dummy, and send a report when you point real
> iron.

Nexus drives rotators through Hamlib's `rotctld` — bundled in the Windows
installer and launched for you, the same way CAT is. You never run `rotctld` by
hand.

> **Every platform now bundles it,** macOS and Linux included, since 1.9.0 — so
> there is nothing to install. If Nexus ever reports it could not start its own
> `rotctld`, a system Hamlib is the fallback (`brew install hamlib` /
> `sudo apt install libhamlib-utils`). Note that WSJT-X working proves only the
> Hamlib *library* is there — Nexus needs the `rotctld` *program*.

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
| Yaesu G-5500 / G-5500DC — GS-232B interface (az/el); also any GS-232B, and ERC V4 in its recommended mode (9600) | 603 |
| Yaesu G-5500 / G-5500DC — GS-232A interface (az/el); also any GS-232A | 601 |
| GS-232 (generic, az/el) — GS-232 clone boards; also EA4TX ARS-USB, LVB, ST2 | 602 |
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
| DF9GR ERC, DCU-1 mode (az) | 404 |
| Green Heron RT-21 | 405 |
| M2 RC2800 (az/el) | 1001 |
| Prosistel D (az) | 1701 |
| Prosistel Combi-Track (az/el) | 1703 |
| PstRotatorAz / PstRotator (UDP) | 3 |
| Dummy (testing — no hardware) | 1 |

> **Yaesu G-5500 / G-5500DC owners:** the rotator has no computer port of its
> own, and Hamlib has no model for it: it is driven through a Yaesu **GS-232B**
> (or the older **GS-232A**) interface, so it is named on those two entries.
> Pick the one for your interface, put in the interface's serial port, and set
> the baud to the rate the interface itself is set to (the GS-232 family takes a
> range, so Nexus leaves it to you). A GS-232 clone board goes on **GS-232
> (generic)**. The Rotor pane then shows the elevation too (0–180° on a G-5500).

> **EA4TX ARS owners:** there is no EA4TX entry, deliberately. Hamlib's ARS
> backend (1101/1102) drives a **parallel port**, which Nexus does not offer —
> it could never have worked with the serial port and baud the picker asks for.
> An **ARS-USB** speaks GS-232, so use **GS-232 (generic)** with the ARS's own
> COM port.

> **PstRotatorAz users:** pick **PstRotatorAz / PstRotator (UDP)**, Hamlib model
> 3. PstRotatorAz takes its commands over UDP, so the port is its address, not a
> serial port: `127.0.0.1:12000` when it runs on the same PC. The baud does not
> matter. Turn on **UDP Control** in PstRotatorAz's Setup and keep it on its
> default port 12000, because Hamlib listens for its position reply on 12001
> only. Leave **External rotctld (advanced)** empty: that box is for a
> `rotctld`, which PstRotatorAz is not. One caveat: Hamlib's PstRotator backend,
> including the one Nexus ships, writes the bearing it sends with a formatting
> bug, so 123.4° goes out as `123.400002.2`. Nobody has tried that against a
> real PstRotatorAz yet, so if the antenna does not turn to where you point it,
> that is the likely reason: please report it.

There's also an **External rotctld (advanced)** field: enter a `host:port` to
point Nexus at a `rotctld` you run yourself (or one on another machine). It
overrides the model/port picker above and stops the integrated daemon. The port
is required — `192.168.1.50` on its own is not an address.

---

## Where the rotator shows up

Once it's configured and answering, rotator control appears throughout the app:

- **Rotor pane in Conditions** — a full rose you can click to slew, with a STOP
  control. The heading reads in true degrees with magnetic beside it, e.g.
  `312°T (316°M)` (WMM2025 declination). A rotator that **cannot report its
  position** (the Hy-Gain DCU-1 is one — its Hamlib backend has no read-back at
  all) keeps the pane, the slew and the STOP, and shows `—°T` instead of a
  needle. On a rotator with an **elevation axis** (a G-5500 on its GS-232B, a
  SPID Rot2Prog, EasyComm) the pane also shows the elevation, `EL 45°`, and an
  `el°` box: type an elevation and press Enter. It takes only what the rotator
  reaches (0–180° on a G-5500) and keeps the bearing where it is, and turning
  the beam keeps the elevation where it is. The one STOP stops both motors.
  Nexus asks the rotator's own control program (Hamlib's rotctld) whether there
  is an elevation axis, so an azimuth-only rotator's pane is exactly as before.
  Beside a cockpit (in the dashboard rail, or in a cockpit's own columns) the
  pane has a second line: the call in that cockpit's log entry (in FT8/FT4, the
  QSO in progress), the short-path bearing and distance to it, e.g.
  `→ EC1DD 227° (1531 km)`, and **Point**, which turns the antenna there. The
  bearing shown is the one Point turns to. So are the Chase box's heading beside
  its ↗ and the distance and bearing on the recall card under a log entry, and a
  browser on Nexus Remote turns its → CALL to the same bearing.
- **RotorStrip in the Phone, CW, Operate, RTTY, PSK, SSTV and JS8 cockpits** — a compact heading strip.
  It **hides when there's nothing to show**, and displays **"ROTOR —"** when a
  rotator is configured but not answering, so you can tell "no rotator" from
  "rotator not responding" at a glance. Click it to land on the model and port.
- **↗ on Needed-board rows** — point the antenna at a spotted station.

<!-- TODO: capture screenshot — Conditions compass pane showing 312°T (316°M) with the slew rose -->

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
