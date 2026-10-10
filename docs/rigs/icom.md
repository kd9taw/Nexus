# Icom Setup

Icom rigs connect over **Serial (USB / COM)** and speak Icom's CI-V protocol,
which Hamlib handles. Modern Icoms report their model name in the USB product
string, so **Detect my radio** usually identifies the exact radio in one scan.
The six with a network port built in can also connect over your LAN or Wi-Fi
(Beta, receive and control only): see [below](#nexuss-own-network-connection-beta).

---

## Quick setup

1. Plug the rig into USB and power it on.
2. **Settings ▸ Radio ▸ Rig & CAT ▸ Detect my radio.** For a radio that embeds its model
   in the USB descriptor (IC-705, IC-7300, IC-9700, and similar), Nexus fills the
   **Rig Model**, the serial port, and the paired USB Audio Codec in one click.
3. On Windows, if the USB-serial driver is missing, Detect shows the exact
   download link — install it and re-scan.
4. Set **Baud** to match the rig's CI-V menu (Nexus defaults to 38400; use the
   same value on both sides).
5. **Save**, then **Test CAT** for a frequency read-back.

<!-- TODO: capture screenshot — Icom IC-7300 auto-detected with model, port, and audio filled -->

### The IC-7300 class — one cable, done

The IC-7300 (and the 705/7610/905/9700) carry CAT and a USB Audio Codec over a
single USB cable. That's the whole hardware setup: one cable, Detect fills
everything, Test CAT confirms. This is the smoothest first-contact path in Nexus
for a brand-new operator.

---

## IC-7760, and Icoms on the network

The IC-7760 works over its **USB cable**, like the rest of the family: in **Settings ▸ Radio ▸ Rig & CAT**, pick
**Rig Model: Icom IC-7760** and the radio's COM port. If **Detect** doesn't fill it in, choose it yourself.

### Nexus's own network connection (Beta)

Nexus can also reach the six Icoms with a network port built in, the **IC-7610, IC-9700, IC-705, IC-905, IC-7760 and
IC-7300MK2**, straight over your LAN or Wi-Fi, with no bridge program in between. It is **Beta, and receive and control
only**: Nexus reads and sets the frequency and mode, follows the dial, reads the meters and draws the radio's own
panadapter, but **it does not transmit on this connection**. Tune, PTT, the FT sequencer, CW, the voice keyer and every
other transmit path say so and stop before the radio is keyed. There is no audio over it yet either: keep the radio's
USB cable for audio, or use a bridge (below) if you need to transmit over the network.

On the radio:

1. **Network Control ON** in the radio's network remote settings (on an IC-7760: MENU ▸ SET ▸ Network ▸ Remote
   Settings). It takes effect after the radio is turned off and on.
2. A **network user** and password in the same menu (16 characters at most each).
3. **CI-V Transceive ON**, so Nexus follows the dial as you turn it.
4. Note the radio's address: **IP Address (LAN)** on an IC-7760.

In Nexus, **Settings ▸ Radio ▸ Rig & CAT**: pick your Icom as the **Rig Model**, then **Connection: Icom network (LAN /
Wi-Fi) — Beta**, and fill in the **Radio address**, the **Network user** and the **Network password** (type it and press
**Set**). The **Control port** stays at 50001 unless you moved it on the radio. **Save**, then **Test CAT**.

What to know:

- **One program at a time.** The radio takes one network client. Close wfview or RS-BA1 before Nexus connects. After a
  program lets go, the radio can hold its old session for up to 3 minutes before it answers again.
- **Your own network only.** For remote use, use a VPN. Don't forward ports to the radio: its network login is
  scrambled on the wire, not encrypted.
- **The password stays in your computer's keychain**, one per radio profile. It is never written to Nexus's settings,
  its log or the CI-V diagnostic file. Removing the radio profile removes its password.
- **What Nexus reads, and writes nothing.** At connect it reads the radio's **Time-Out Timer** and **MOD Input**
  settings and shows them under the connection status. A later Beta that transmits over the network will ask for the
  time-out timer on and the DATA MOD source set to LAN; nothing is changed for you.
- **When the session drops**, Nexus says why and reconnects by itself after 1, 2, 4, 8 and 16 seconds, then every 30
  seconds. If the radio refused the login, or another program took the radio, it waits until you press **Test CAT** or
  save the connection again.
- **Not yet tried on a radio** (NEEDS-BENCH): it is tested against a simulated radio only, and an IC-7760 is the
  first real one. Please report how it goes on yours.
- **Going back** is one setting: **Connection: Serial**.

### Through a bridge

To transmit or carry audio over the network today, put a program in between. Hamlib, which Nexus uses for Icoms over
USB, doesn't speak Icom's network protocol. wfview and Icom's own RS-BA1 both do the job, but they hand the radio to
Nexus in different ways.

**With wfview, or another bridge that offers a rigctld port:**

1. Run a program that speaks Icom's network protocol and offers a Hamlib rigctld port. wfview is the usual one; check
   that your version supports your radio.
2. Turn on its rigctld server and note the port it listens on (wfview's is 4533 unless you change it).
3. In Nexus, set **Connection** to **Network**, pick **Rig Model: NET rigctl (remote rigctld)** and set
   **Network Address** to the bridge's address and that port (for wfview on the same computer, `127.0.0.1:` and the
   port, such as `127.0.0.1:4533`). Leave **rigctld TCP Port** as it is: Nexus talks to a rigctld on the same
   computer directly, with no rigctld of its own in between.
4. The audio travels through the bridge too: pick the bridge's audio devices in Nexus's audio settings.

The bridge and Nexus can start in either order. With the bridge on this computer and nothing answering at the Network
Address yet, Nexus starts nothing in between: the CAT status reads "Nothing is answering at 127.0.0.1:4533 — start
wfview (or your rigctld)", with your address, and Nexus asks again on its own (after 10 seconds, then less often, at
most 5 minutes apart) and connects once the bridge answers. **Test CAT** asks again at once. For a bridge on another
computer, Nexus talks to it through a rigctld of its own, and with wfview that one refuses PTT whenever wfview reports
the radio switched off. If **Share this radio with other programs** is on, its port must be a different number from
the bridge's.

**With RS-BA1:** it runs no rigctld server, so NET rigctl finds nothing to talk to. RS-BA1 gives the computer a virtual
COM port instead, and Nexus uses that port the way it uses a USB cable:

1. Connect RS-BA1 to the radio.
2. In Nexus, set **Connection** to **Serial**, pick your Icom's own **Rig Model** (Icom IC-7760, for example, not NET
   rigctl) and set **Serial Port** to the virtual COM port RS-BA1 creates.
3. The audio travels through RS-BA1 too: pick the audio devices RS-BA1 uses on this computer in Nexus's audio
   settings.

## IC-9700 — VHF/UHF and 23 cm

The **IC-9700** is fully supported, including the **23 cm band**: Nexus knows the
1296 MHz plan (FT8 at **1296.174 MHz**), so digital, phone, and CW all work up
through 1.2 GHz with the same cockpits you use on HF.

### Satellite station

The IC-9700 is the classic satellite radio, and Nexus has a matching workflow:

- The **Satellites** section shows the next passes over *your* grid, favorites,
  polar plots, and each bird's uplink/downlink frequencies.
- Pair it with a rotator (see [rotators.md](rotators.md)) and Nexus **auto-tracks
  the pass** — arming before AOS, prepositioning the antenna, then following the
  bird across the sky (falling back to azimuth-only on an az-only rotator).
- The moving-satellite map layer draws each bird crawling in real time with its
  footprint ring.

For SO-50 / ISS-class FM/SSB work, set the 9700 up once here and let the pass
scheduler and rotor tracking do the pointing.

<!-- TODO: capture screenshot — IC-9700 satellite pass with rotor auto-track and polar plot -->

---

## What Nexus does automatically per section

- **Digital (FT8/FT4/TempoFast/TempoDeep)** → **USB-D** (Icom's data submode, Hamlib
  `PKTUSB`), opened to a wide data passband so decodes aren't clipped.
- **Phone (SSB)** → **USB** above 10 MHz, **LSB** below.
- **CW** → **CW** on the CAT keyer (the rig generates Morse); **USB/LSB** on the
  soundcard keyer. Nexus sets this on section entry — no manual mode change.

---

## Curated Icom models

Selectable in the Rig Model dropdown (Hamlib model number in parentheses).
`rigctl -l` lists everything your Hamlib knows.

| Model | Hamlib # |
|---|---|
| IC-7300 | 3073 |
| IC-705 | 3085 |
| IC-7610 | 3078 |
| IC-9700 | 3081 |
| IC-7100 | 3070 |
| IC-718 | 3013 |
| IC-7000 | 3060 |
| IC-746 | 3023 |
| IC-746PRO | 3046 |
| IC-756PROIII | 3057 |
| IC-910 | 3044 |
| IC-905 | 3090 |

> Xiegu radios share Icom's CI-V backend but are listed separately — see the
> [Xiegu guide](xiegu.md).

---

## PTT

Icom rigs support CAT PTT (Hamlib `T`). Choose **CAT (via rigctld)** for
straightforward keying, or **VOX** if your data path drives VOX — CAT still owns
frequency and mode either way. A CI-V address mismatch is the usual reason CAT
"connects but does nothing"; leave the rig's CI-V address at its default and let
Hamlib's default match it.

---

*CAT not answering, or CI-V acting up? See
[Troubleshooting](../troubleshooting.md).*
