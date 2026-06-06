# Rig and Audio Setup

This page covers getting Tempo talking to your radio: CAT control and PTT, audio devices and levels, time sync, and the transmit safeguards. For first-time setup in context, see [Getting Started](Getting-Started.md); for the calling frequencies, see [Frequency Plan](Frequency-Plan.md).

All of these live under **Settings** (the gear at the bottom of the left mode bar). Settings persist to `%APPDATA%\tempo\settings.json`.

---

## PTT and CAT — pick a method

Tempo handles rig control **in-app**. With CAT you do **not** run `rigctld` yourself — Tempo launches it for you. Choose one **PTT Method** in **Settings → Rig Control**:

| Method | What it does | When to use it |
|--------|--------------|----------------|
| **CAT (via rigctld)** | Tempo launches Hamlib's `rigctld` and keys PTT + retunes the rig over CAT. | Modern rigs with a CAT/USB connection (recommended). |
| **Serial RTS** | Keys PTT by asserting the serial **RTS** line. | PTT-only interfaces / older rigs. |
| **Serial DTR** | Keys PTT by asserting the serial **DTR** line. | PTT-only interfaces / older rigs. |
| **VOX** | No keying — the rig keys itself on transmit audio. | Anything, as a fallback; the safe default. |

The default out of the box is **VOX** (`rig_model = 0`), so Tempo runs even before you've wired CAT.

### CAT setup (recommended)

1. **PTT Method → CAT (via rigctld)**.
2. **Rig Model** — pick your radio from the Hamlib model dropdown (56 curated models: Icom, Yaesu, Kenwood, Elecraft, FlexRadio, Ten-Tec, and more). If your exact model isn't listed, the curated list is best-effort — confirm the right model number with `rigctl -l`.
3. **Serial Port** — choose the **COM** port (Windows) or **/dev/tty…** device. Hit **Refresh** to re-scan if you plugged the rig in after opening Settings.
4. **Baud** — match your rig's CAT baud rate (default `38400`).
5. **rigctld TCP Port** — the local TCP port Tempo launches `rigctld` on (default `4532`, the Hamlib standard). Only change this if something else already uses 4532.

When you save, Tempo spawns `rigctld` with the right `-m <model> -r <port> -s <baud> -t <tcp_port>` line, sets your dial/mode once, then keys and retunes per slot. The daemon is **kill-on-drop** — it exits when Tempo does.

> **Installer users:** the installer **bundles** `rigctld` and its DLLs, so CAT works **offline** with no separate Hamlib install. Tempo prefers the bundled copy and only falls back to a `rigctld` on your `PATH`. If you run a *from-source* build that skipped the Hamlib fetch, put Hamlib's `rigctld.exe` on `PATH`.

### Serial RTS / DTR
Pick the **COM port** and choose **RTS** or **DTR** as the PTT method. (This is enabled by the `serial` feature, which the standard `radio` build includes.)

### VOX
No CAT, no keying — just make sure your rig's VOX is enabled and tuned so transmit audio keys it.

---

## Audio devices

Under **Settings → Audio**, point Tempo at your sound card:

- **Input Device (RX)** — the device carrying your rig's **receive** audio (a USB CODEC, SignaLink, or the rig's built-in USB audio). Leave it as **System default** to use Windows' default recording device. Use **Refresh** to re-scan.
- **Output Device (TX)** — the device feeding the rig's **data/mic input** for transmit. **System default** uses Windows' default playback device.

Tempo resamples to/from the modem's internal 12 kHz automatically, so you don't need a specific sample rate on the device.

---

## Setting Tx power and reading the RX level meter

### Tx power
**Settings → Audio → Tx Power** is the transmit drive level (shown as a percentage). Set it **conservatively** and watch your rig's ALC: too much drive causes ALC overdrive, splatter, and a distorted (less-decodable) signal. Start low and bring it up only until the rig reaches its rated power without ALC pumping. Use the top-bar **Tune** button to key a steady carrier while you set this.

### RX level meter
The **RX Level** meter appears in both the top bar and Settings → Audio. It follows a low / good / hot zoning:

- **Aim for the middle (green) zone** — roughly mid-scale.
- **Red / hot** (near the top) = **clipping**; turn the rig's audio output (or the sound card's input gain) **down**.
- **Too low** (barely moving) = raise the input gain so the modem has signal to work with.

A target marker on the meter shows where to aim.

---

## Time-sync health and the Tx watchdog

### Time sync
Decoding depends on an accurate clock — the T/R slots are UTC-aligned. The top bar shows a **Sync / No Sync** dot and a **dT** readout (the measured timing offset of stations you hear). If it shows **No Sync** or dT is consistently large:

- Make sure Windows is syncing time (Settings → Time & language → Date & time → *Sync now*; or `w32tm /resync` from an elevated prompt).
- For **off-grid** operation with no internet, use a **GPS** or local **NTP** time source.

A few hundred milliseconds is fine; seconds of offset will cost you decodes.

### Tx watchdog
**Settings → Operating → Tx Watchdog (min)** auto-halts transmit after that many minutes of continuous keying (default **6**; `0` = off). If it fires you'll see a **TX watchdog** chip in the top bar — re-enable **Monitor** to clear it. This protects you (and the band) from a stuck transmit.

---

## Tune / Monitor / Stop TX

Three top-bar controls govern transmit, available in every mode:

- **Tune** — keys a steady carrier for tuning an antenna or setting power. Click again to stop.
- **Monitor / Muted** — the global transmit enable. **Monitor** = transmit allowed; **Muted** = listen-only (Tempo still decodes but never keys). Use Muted any time you want to watch the band without risk of transmitting.
- **Stop TX** — drops PTT and halts the sequencer **immediately**. The panic button.

---

## Network interop (optional)

Under **Settings → Network**:

- **WSJT-X UDP API** — emits the WSJT-X-compatible UDP protocol so **JTAlert / GridTracker / N1MM+ / loggers** can consume Tempo's decodes and QSOs (and double-click-to-call back). Default target `127.0.0.1:2237` (same as WSJT-X). For another machine on the LAN, set its `host:port` and allow the UDP port through Windows Firewall.
- **PSK Reporter** — uploads your heard stations (call / freq / mode / SNR) to `report.pskreporter.info:4739` so your reception shows on the global maps. Outbound UDP — allow it through the firewall.

More detail in [Architecture and Protocol](Architecture-and-Protocol.md).

---

If CAT won't connect, audio levels look wrong, PTT won't key, or time sync is off, the **[Troubleshooting](Troubleshooting.md)** page has a checklist for each.
