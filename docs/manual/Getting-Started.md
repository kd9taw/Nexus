# Getting Started

This page takes you from download to your first decode. Tempo's primary platform is **Windows**. macOS/Linux desktop builds are on the [Roadmap](Roadmap.md).

> **Beta reminder.** Tempo is simulation-validated, not yet on-air-validated, and the Windows binaries are cross-compiled. Treat it as experimental and operate within your license. See the honest beta note on [Home](README.md).

---

## 1. Download

Grab the latest installer from the releases page:

- **<https://github.com/kd9taw/tempo/releases/latest>**

The file is named like `Tempo_0.1.0_x64-setup.exe` (~199 MB). It is a **per-user** installer (no administrator rights needed) and it bundles everything offline:

- the **WebView2** runtime (so it installs clean even on an air-gapped PC), and
- **Hamlib** (`rigctld` + DLLs), so CAT rig control works with **no separate install**.

---

## 2. Install — and the SmartScreen warning

Run the installer. Because the published binaries are **cross-compiled and unsigned**, Windows **SmartScreen** may show a blue *"Windows protected your PC"* dialog.

To proceed: click **More info**, then **Run anyway**.

This is expected for an unsigned beta build. If you'd rather not trust the binary, you can [build from source](Building-from-Source.md) yourself.

The app installs per-user and creates a Start-menu entry. Settings persist to `%APPDATA%\tempo\settings.json`; your logbook lives in `log.adi`.

---

## 3. First launch

On first launch, open **Settings** (the gear at the bottom of the left mode bar). At minimum, set the items below. Save (the button is disabled until your callsign is filled in).

### Identity (required)
- **Callsign** — your station call (used in CQ/beacons and exchanges). Stored uppercase.
- **Grid** — your Maidenhead locator (e.g. `EN52`).
- **Field Day Class / Section** — e.g. `1D` / `WI` (only needed for Field Day).

### Band & frequency
- Pick a Tempo channel from the **Band / Channel** preset dropdown, **or** type a **dial frequency (MHz)** and choose **USB** or **FM**. Tempo labels the band automatically and (with CAT) retunes the rig live.
- Tempo ships its **own** calling frequencies — *not* the FT8/FT4/JS8 watering holes. See [Frequency Plan](Frequency-Plan.md). **Confirm these against your local band plan and your license privileges before transmitting.**

### Rig & PTT
- Choose a **PTT Method**: **CAT (via rigctld)**, **Serial RTS**, **Serial DTR**, or **VOX**.
- For CAT, also pick your **Rig Model** (56-model Hamlib dropdown), **Serial Port** (COM/tty), and **Baud**.
- Full walkthrough: [Rig and Audio Setup](Rig-and-Audio-Setup.md).

### Audio in / out + levels
- Point the **Input Device (RX)** at the sound card carrying your rig's receive audio, and the **Output Device (TX)** at the card feeding the rig. Leave either as **System default** to use Windows' default device.
- Set **Tx Power** (the transmit drive slider) conservatively to avoid ALC overdrive.
- Watch the **RX Level** meter while receiving: aim for the green zone; **red means clipping** (back the rig's audio down). See [Rig and Audio Setup](Rig-and-Audio-Setup.md) for the details.

Pick **Fast (FT1)** or **Robust (DX1)** in the top bar tier toggle — see [Tiers FT1 vs DX1](Tiers-FT1-vs-DX1.md).

---

## 4. It starts passive

Tempo **starts passive (hunt-and-pounce)**: on startup it only listens and decodes. It will **not** transmit until you act — send a message, answer a station, call CQ, or turn on the beacon.

The **CQ beacon is opt-in** (off by default). Turn it on under **Settings → Operating → Beacon** only when you want Tempo to periodically announce your presence with `CQ <call> <grid>`.

You can also globally mute transmit at any time using the **Monitor / Muted** button in the top bar, and stop an in-progress transmission instantly with **Stop TX**.

---

## 5. Next steps

- Learn the layout and the modes: **[Operating Guide](Operating-Guide.md)**.
- Dial in your rig, PTT, audio levels, and time sync: **[Rig and Audio Setup](Rig-and-Audio-Setup.md)**.
- Pick the right tier for conditions: **[Tiers FT1 vs DX1](Tiers-FT1-vs-DX1.md)**.
- Something not working? **[Troubleshooting](Troubleshooting.md)**.
