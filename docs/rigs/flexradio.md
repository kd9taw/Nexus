# FlexRadio Setup

> **Field status:** LAN discovery is verified on a real **FLEX-6400M** — Nexus
> finds the radio on the network and one-clicks the config. The full CAT
> control chain is in **final verification**. Expect this path to be solid;
> please send field reports if anything on your Flex behaves differently.
>
> The **native SmartSDR path** (the two *early access* toggles further down —
> native panadapter and native DAX audio) is a different story: it is **off by
> default and has never been run against a real Flex**. Treat it as an
> experiment you opt into, not part of the setup.

FlexRadio is the one brand that connects over **Network**, not a serial port.
Nexus drives a Flex the same way WSJT-X does — through the **SmartSDR CAT** app
running on your PC — with audio over **DAX** virtual sound devices.

---

## What you need running first

Because Nexus talks to SmartSDR (not the radio directly), on the PC that runs
your Flex you need:

1. **SmartSDR** connected to the radio, with at least one slice active.
2. The **SmartSDR CAT** app running — it creates the TCP CAT ports Nexus
   connects to.
3. **DAX** running, if you want digital audio (FT8/FT4) — DAX presents the
   radio's audio as virtual sound devices.

---

## Quick setup (the proven path)

1. **Settings ▸ Radio ▸ Rig & CAT ▸ Detect my radio.** The same scan that reads USB rigs
   also **discovers FlexRadios on your LAN**. Your Flex appears as a row like
   *"FLEX-6400M — network · via SmartSDR CAT on this PC (slice A, TCP 5002)"*.
2. Click **Use this**. Nexus sets everything for the proven path automatically:
   - **Connection:** Network
   - **Network Address:** `127.0.0.1:5002` (SmartSDR CAT's default port, aimed
     at **slice A**)
   - **Rig Model:** *FlexRadio FLEX-6xxx (SmartSDR CAT)* (Hamlib 2036) — the
     WSJT-X-proven Kenwood-dialect model
   - **Flex radio IP:** the address discovery found, kept for the native
     toggles below (CAT does not use it)
3. Pair audio: when DAX is running, a **⚡ Pair DAX audio** button appears under
   the Network Address. One click sets Nexus's audio in/out to the DAX RX and TX
   endpoints (Nexus prefers the live **DAX TX** endpoint that actually keys, and
   **DAX Audio RX 1** for slice A). This is bit-clean digital audio with no sound
   card in the path.
4. **Save**, then **Test CAT**.

`127.0.0.1:5002` assumes SmartSDR CAT is running **on the same machine as
Nexus**. If it runs on another PC — which is the normal arrangement if Nexus is
on a Mac — put that PC's LAN address in Network Address instead
(`192.168.1.20:5002`), and select its DAX devices there, not here.

<!-- TODO: capture screenshot — FlexRadio discovered on the LAN with the Use this button -->

<!-- TODO: capture screenshot — Network Address 127.0.0.1:5002 with the Pair DAX audio button -->

---

## Running more than one slice

SmartSDR CAT gives **each slice its own TCP port**, and Nexus drives **one slice
per radio**. Two slices means two *radios* in Nexus, not two copies of the app:

1. **Settings ▸ Radio** — add a second radio and give it Connection **Network**,
   Rig Model **FlexRadio FLEX-6xxx (SmartSDR CAT)**, and the second slice's CAT
   port as its Network Address.
2. Tick **Run both radios at the same time**.
3. Relaunch. Nexus asks which radio this window drives; open a second window for
   the other. Both windows share one logbook, and each keeps its own settings.

| Slice | SmartSDR CAT port |
|---|---|
| A | 5002 |
| B | 60001 |
| C | 60002 |
| D | 60003 |

Confirm the port in the SmartSDR CAT window rather than trusting the table — it
is what actually assigns them.

**Audio for the second slice is a manual pick.** The ⚡ Pair DAX audio button
always chooses **DAX Audio RX 1**, so on the slice-B window select *DAX Audio
RX 2* by hand in **Settings ▸ Radio ▸ Audio**. Nexus keeps a hand-picked DAX
device — the button only bootstraps, it never overrides you. The **native DAX
audio** toggle cannot do this at all: it is hard-wired to DAX channel 1, so
leave it off on every window but the slice-A one.

---

## The native SmartSDR path (early access — read this first)

Two toggles in **Settings ▸ Radio ▸ Rig & CAT** talk to the radio's own SmartSDR
API on port `4992`, alongside the CAT path above:

- **Flex native panadapter** — streams the radio's real panadapter (VITA-49 FFT)
  into the cockpit scope instead of the audio FFT.
- **Flex native DAX audio** — takes RX audio straight off the network instead of
  the *DAX Audio RX* sound device, which is invisible under Remote Desktop.

Both are **off by default and unverified on hardware** — nobody has run either
against a real Flex. They need **Flex radio IP** filled in (Detect fills it); with
it empty they do nothing at all. If the scope stays blank or decodes stop, turn
them back off.

Three limits worth knowing before you switch one on:

- **Same LAN only — not SmartLink, not through NAT.** Both workers bind a local
  UDP port and hand the radio *that* number, and the radio streams back to it.
  Behind NAT that number means nothing on the far side, and the panadapter
  socket never sends anything outbound, so no mapping is ever created. LAN
  discovery has the same limit: it listens for the radio's broadcast, which does
  not cross a router. **If you reach your radio over SmartLink or a
  port-forward, use the SmartSDR CAT path and leave these toggles off** — a
  routed VPN that puts you on the same subnet is the only remote arrangement the
  native path can work on.
- **Native DAX changes transmit audio too.** Switching it on sends
  `transmit set dax=1`, which is a **radio-wide** setting: while it is on, the
  Flex's modulator takes its audio from DAX, not the microphone, for every
  client. Nexus sends the restore on a clean shutdown — but if it is killed, or
  if the DAX transmit stream never came up, the radio is left taking transmit
  audio from DAX and your microphone stays dead until you put it back in
  SmartSDR.
- **One DAX channel.** Native DAX is hard-wired to channel 1, so it cannot serve
  a second slice — see *Running more than one slice* above.

---

## macOS

Every Flex flow leaves your Mac and goes out to the LAN: discovery, CAT to a
SmartSDR CAT host, and the native path's connections to the radio on `4992` /
`4993`. macOS 15 gates that behind the **Local Network** privacy permission.

**Nexus does not ship a local-network usage description**, so you may never see
a permission prompt — and a denial does not produce an error. It produces
silence: Detect finds no Flex ("No radios found"), Test CAT reports nothing
answering at the address, the native scope stays blank with no message, and the
DAX banner blames the IP or a firewall. None of those name the real cause.

If a Flex works from another machine but not from your Mac, check
**System Settings ▸ Privacy & Security ▸ Local Network** and make sure **Nexus**
is enabled, then relaunch the app. If Nexus is not listed there at all, that is
consistent with the missing usage description — the permission was never
requested. There is no workaround inside the app today. A CAT address on
`127.0.0.1` is unaffected: loopback is not local-network traffic.

---

## PowerSDR, Thetis and other SDR programs

If you run **PowerSDR**, **Thetis** or another SDR program that serves CAT, pick
the entry that names your program — Connection and port follow that program's
own CAT settings (usually a com0com virtual COM pair, or TCP).

| Program | Model | Hamlib # |
|---|---|---|
| PowerSDR / mRX PS (older Flex, Apache ANAN) | *PowerSDR / mRX PS (Apache ANAN / legacy FLEX)* | 2048 |
| Thetis (Hermes Lite 2 / ANAN / HPSDR) | *Thetis (Hermes Lite 2 / ANAN / HPSDR)* | 2054 |
| piHPSDR / OpenHPSDR | *piHPSDR / OpenHPSDR (Hermes Lite 2 / ANAN)* | 2040 |
| SDR Console | *SDR Console (SDR-Radio.com)* | 2056 |

These are **not** a TS-2000 emulation, whatever older guides say: 2048 and 2054
speak PowerSDR's own `ZZ`-prefixed command set, which is why they have their own
entries rather than sharing a Kenwood one.

---

## Curated FlexRadio models

| Model | Hamlib # | Use it for |
|---|---|---|
| FlexRadio FLEX-6xxx (SmartSDR CAT) | 2036 | **The recommended path** — CAT through the SmartSDR CAT app |
| FlexRadio SmartSDR native (experimental) | 23005 | Direct-to-radio, **not recommended** — see below |

### Why not the native model?

The **SmartSDR native** model (23005) talks the radio's own API directly over
`:4992`, bypassing SmartSDR CAT. It's alpha-grade in Hamlib and **failed on real
hardware** (a FLEX-6400M returned a socket error), so nothing auto-picks it. It
stays in the list for the curious, but the SmartSDR CAT path (2036 at
`127.0.0.1:5002`) is the one that works. It is a different thing from the two
native *toggles* above, which keep CAT on the 2036 path.

---

*Discovery finds the radio but Test CAT fails? Confirm SmartSDR and the SmartSDR
CAT app are both running on the PC your Network Address points at — and on a Mac,
check the Local Network permission above. More in
[Troubleshooting](../troubleshooting.md).*
