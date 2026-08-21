# Rig Setup Guides

Nexus talks to your radio through **Hamlib** — the same CAT library WSJT-X uses.
The Windows installer bundles it, so CAT and rotor control work offline out of the
box with nothing else to install. Around fifty rigs are curated into the model
picker; the definitive list for your installed Hamlib version is always
`rigctl -l`.

> **Where Hamlib comes from depends on the platform.** The bundled `rigctld` (and
> rotator daemon) ship in the Windows installer only. Linux and Raspberry Pi use
> the system Hamlib instead: the `.deb` pulls `libhamlib-utils` in for you, and
> AppImage users run `sudo apt install libhamlib-utils` once. Either way Nexus
> never transmits on launch — TX is always an explicit operator action, and a
> declared license class is enforced as a real Part 97 sub-band lockout.

---

## Pick your brand

| Brand | Guide | Field status |
|---|---|---|
| **Yaesu** | [yaesu.md](yaesu.md) | Verified on FTDX10 and FT-991A |
| **Icom** | [icom.md](icom.md) | IC-9700 incl. 23 cm; IC-7300-class single-USB |
| **FlexRadio** | [flexradio.md](flexradio.md) | LAN discovery verified on a FLEX-6400M; CAT chain in final verification |
| **Xiegu** | [xiegu.md](xiegu.md) | Supported but **not yet field-verified** |
| **Sound-card interfaces** | [interfaces.md](interfaces.md) | Digirig / RIGblaster; **not yet field-verified** |
| **Rotators** | [rotators.md](rotators.md) | Dummy-verified; real az/el hardware pending |

Kenwood, Elecraft, Ten-Tec, QRP Labs, and Alinco are also in the curated model
picker and follow the generic serial path below — they just don't have a
brand-specific page yet. If you run one and want notes added, open an issue.

---

## The three connection types

Everything in **Settings ▸ Radio ▸ Rig & CAT** comes down to one choice — the
**Connection** dropdown:

### Serial (USB / COM) — most rigs

This is the default and covers nearly every radio, including all Yaesu, Icom,
Kenwood, Elecraft, and Xiegu models. You set three things:

1. **Rig Model** — pick your radio from the dropdown (it shows the Hamlib model
   number in parentheses).
2. **Serial Port** — the COM port (Windows) or tty device (Linux/macOS) that
   carries CAT. Use **Refresh** to re-scan, or **Auto-test** to have Nexus probe
   each port read-only (it never transmits) and select the one that actually
   drives your rig.
3. **Baud** — match whatever your rig's CAT menu is set to. Nexus defaults to
   **38400**; set the rig and Nexus to the same value.

The fastest way to fill all three is **Detect my radio** (the *Zero-config
setup* button): one scan reads your USB devices, fills the model, port, and
paired sound device, and — if Windows is missing a USB-serial driver — shows the
exact download link. Review what it filled, then **Save**.

<!-- TODO: capture screenshot — Settings ▸ Radio ▸ Rig & CAT with the Connection dropdown and Detect my radio button -->

### Network (FlexRadio / remote)

Choose this for a **FlexRadio** driven through SmartSDR CAT, or for any rig
served by a **remote `rigctld`** over TCP. You set a single **Network Address**
as `host:port` — for a Flex that's `127.0.0.1:5002`. See the
[FlexRadio guide](flexradio.md) for the full picture.

### OmniRig (Windows only)

**OmniRig** is VE3NEA's rig-control server — the one a lot of Windows logging
and contest software already uses. If you run it, your radio is already set up
once, in one place, and every program shares it instead of fighting over the COM
port. Pick this and Nexus becomes another one of those programs.

**The radio is configured in OmniRig, not in Nexus.** Rig type, COM port, baud,
polling interval — all of that lives in OmniRig's own settings window. So when
you pick OmniRig here, Nexus stops asking for **Rig Model**, **Serial Port** and
**Baud**: they would be a second copy of settings OmniRig owns, and a second copy
is a second thing to get wrong.

What Nexus does ask for is **which** radio:

- **OmniRig radio** — `RIG 1` or `RIG 2`, matching the two tabs in OmniRig's own
  window. Default is RIG 1. If you run two radios, give each Nexus radio its own
  slot.

**PTT.** With OmniRig selected, leaving **PTT Method** on `CAT` keys the radio
through OmniRig — nothing else to set up. The other choices still work exactly as
they always have, and they are deliberately independent of OmniRig: plenty of
operators key a hardware line while OmniRig drives CAT. Pick `RTS` or `DTR` and
set **PTT Serial Port** to your keying cable's port, and Nexus asserts that line
itself; pick `VOX` and the radio keys off transmit audio. Only the `CAT` choice
goes through OmniRig.

Two things to know before you pick it:

- **It is Windows only.** OmniRig is a Windows COM server, so on macOS and Linux
  the choice is shown greyed out with that reason. Use Serial or Network there.
- **OmniRig has to be installed.** Install it from
  [dxatlas.com/OmniRig](https://dxatlas.com/OmniRig/) and run it once so it
  registers itself with Windows. If it isn't there, Nexus says so plainly rather
  than reporting a dead radio — and if OmniRig is running but your rig is off, on
  a busy port, or not answering, Nexus shows OmniRig's own words for it.

---

## Confirming it works: Test CAT

After you Save, click **Test CAT**. It saves your settings, launches the bundled
`rigctld`, and reads your rig's dial frequency back. A result like
`✓ 14.074 MHz` means the port opened, the model is right, and the rig answered.
An `RPRT` error or timeout points at a wrong model, wrong baud, wrong port, or
another app holding the port. Test CAT mirrors the WSJT-X "Test CAT" workflow —
run it any time you change model, port, or baud.

<!-- TODO: capture screenshot — Test CAT showing a green frequency read-back -->

---

## My rig isn't listed

The picker is curated to ~50 common radios, not the whole Hamlib catalog. If
yours isn't there, the way in is an external `rigctld`:

- **Talk to an external `rigctld` (NET rigctl, model 2).** Run `rigctld`
  yourself for your rig — `rigctl -l` lists every model number your installed
  Hamlib knows, so you can launch the daemon with the exact one — then in Nexus
  select **NET rigctl (remote rigctld)**, set **Connection ▸ Network**, and
  point the Network Address at that daemon's `host:port`. Nexus becomes just
  another Hamlib network client.

The picker also always includes two Hamlib pseudo-rigs: **Dummy** (model 1) for
click-through testing with no hardware, and **FLRig** (model 4) to route through
an flrig instance.

---

*Stuck on any of this? See [Troubleshooting](../troubleshooting.md) — CAT
connect failures, driver installs, port conflicts, and audio device selection.*
