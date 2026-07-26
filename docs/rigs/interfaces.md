# Sound-Card Interface Setup (Digirig, RIGblaster)

> **Field status:** the code paths below are covered by tests, but interface
> hardware is **not yet field-verified** on the author's bench. If you run a
> Digirig or a RIGblaster, your field report is genuinely valuable.

Most modern radios carry CAT and audio over one USB cable. Older and smaller
rigs don't — an FT-857, an FT-818, an IC-7100 or a Xiegu G90 needs an outboard
box that provides the sound card and the keying line. That box is what this page
is about.

An interface is **a cable, not a radio**. Nexus recognises the ones that name
themselves, but it will never guess which rig is on the other end — you still
pick your **Rig Model** yourself.

---

## The one thing that matters: how many serial ports?

Every sound-card interface breaks down to three jobs — RX/TX audio, CAT, and
keying (PTT). What differs is whether CAT and keying share a serial port.

### One port for both — Digirig Mobile

A Digirig Mobile presents **one** COM port carrying CAT *and* the RTS keying
line, plus its own sound device.

| Setting | Value |
|---|---|
| **Rig Model** | your radio (the cable can't tell Nexus this) |
| **Serial Port** | the Digirig's COM port |
| **Baud** | whatever your rig's CAT menu says |
| **PTT Method** | `RTS` |
| **PTT Serial Port** | **leave blank** |

Leaving PTT Serial Port blank is the whole trick. Nexus hands the port to
Hamlib and asks it to do both — Hamlib shares one connection to the port rather
than opening it twice, so you get full CAT *and* RTS keying on a single cable.

> **If you used Nexus before 0.19 with a Digirig:** keying worked and CAT
> silently didn't. The band never followed, and nothing said why. That's fixed —
> the same settings now give you both.

### Separate ports — SO2R controllers, some RIGblasters

An SO2R controller (microHAM u2R, MK2R) gives each radio its **own** keying
port, separate from CAT.

| Setting | Value |
|---|---|
| **Serial Port** | the port carrying CAT |
| **PTT Method** | `RTS` (or `DTR`) |
| **PTT Serial Port** | the controller's keying port, e.g. `COM9` |

**PTT Serial Port is per radio.** Each rig on the box has its own keying port,
and it follows the radio you switch to.

### RIGblaster

West Mountain's family spans both shapes. A PTT-only box (Plug & Play) provides
a keying port and no CAT — set **PTT Method** to `RTS` and leave Rig Model
unset if the radio has no CAT of its own. A model that also carries CAT follows
whichever of the two layouts above matches your cabling. Nexus recognises a
RIGblaster by name but deliberately does **not** pre-fill the port question,
because guessing it wrong keys the wrong thing.

---

## Detect my radio

**Detect my radio** reads your USB devices and fills what it can. For a
recognised interface it names the cable, pre-fills `RTS`, and pairs the sound
device — a Digirig enumerates on Windows as *USB PnP Sound Device*, which is why
it used to pair nothing at all.

It will **not** fill your Rig Model from an interface, on purpose. A stock
Digirig is a Silicon Labs CP2102, USB id `10C4:EA60` — the **same id** as an
FTDX10, an FT-710 and several Xiegu radios. Identifying by that number would
label a working FTDX10 as a cable. So Nexus matches the product *name* and, when
a device reports only its bridge chip, leaves it alone for you to configure.

If your rig isn't identified, **Auto-test** sweeps the port read-only and tries
the common interface-cable radios — FT-891, FT-857, FT-817/818, IC-7100,
IC-705, Xiegu G90/X6100, TS-480 — at each family's factory baud.

---

## Audio levels

The interface is your sound card, so the usual rules apply: set **TX Level** so
your rig shows no ALC deflection on a data mode, and set RX gain for a waterfall
that isn't clipping. Nexus never transmits on launch, and TX is always an
explicit action.

---

## Digirig Lite and CM108 dongles

Some interfaces — Digirig **Lite**, and most cheap modded dongles — key over
**CM108 HID GPIO** rather than a serial line. **Nexus does not support CM108
keying yet.** Use VOX, or a radio that keys over CAT, or a serial-keying
interface. Nexus names the Lite when it sees one and says so rather than
offering a PTT method that would never key.

---

## Troubleshooting

**Keying works, the band doesn't follow.** No CAT. Check Rig Model is set —
Nexus now says this outright in the Test CAT result rather than reporting a bare
success.

**"Could not open serial port."** Something else holds it: WSJT-X, another
Nexus window, or a stale `rigctld`. Close it and retry.

**Test CAT fails on a single-cable interface.** Baud is the usual cause — match
your rig's CAT menu exactly. If CAT can't start, Nexus falls back to keying the
line directly, so you keep TX while you sort it out; the result message says so.

---

*See also: [Rig Setup Guides](index.md) · [Troubleshooting](../troubleshooting.md)*
