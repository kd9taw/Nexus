# Troubleshooting

Work top-to-bottom; most problems are audio device, levels, time sync, frequency, or PTT. If you're stuck, open an issue at <https://github.com/kd9taw/tempo> with band/dial/mode/tier and what you saw vs. expected.

> **First thing to try:** make sure you're on the **latest release** — several early-build issues below are fixed in current builds. Download: <https://github.com/kd9taw/tempo/releases/latest>.

---

## The window is blank — "page cannot be displayed"

Seen on **older builds** where the WebView couldn't load the embedded UI.

- **Update to the latest release** — current builds embed the UI assets (`custom-protocol`) so this shouldn't occur.
- Make sure the **WebView2** runtime is present. It ships *offline* in the installer; on Windows 10 you can also install Microsoft's Evergreen WebView2 runtime manually.

## SmartScreen warning on install ("Windows protected your PC")

Expected — the published binaries are **cross-compiled and unsigned**. Click **More info → Run anyway**. If you'd rather not trust the binary, [build it from source](Building-from-Source.md). See also [Getting Started](Getting-Started.md).

## The app shows demo / fake stations and QSOs

An **older-build** fallback: if the installed app couldn't detect its Tauri backend, it dropped into the in-browser demo mock (fake stations / QSOs).

- **Update to the latest release** — current builds always use the real engine and no longer fall back to the mock.

---

## No decodes

If you're hearing the band by ear but Tempo decodes nothing, check, in order:

1. **Audio input device** — Settings → Audio → **Input Device (RX)** must point at the sound card carrying your rig's receive audio (or "System default" if that's your rig). Hit **Refresh** after plugging in.
2. **RX level** — watch the RX level meter (top bar / Settings → Audio). Aim for the green zone. **Too low** = nothing to decode; raise the input gain. **Red/clipping** = distortion; lower it.
3. **Time sync** — the top-bar **Sync** dot must be green and **dT** small. If it says **No Sync**, fix the PC clock (see below). Misaligned slots = no decodes.
4. **Frequency / sideband** — you must be on a **Tempo** calling frequency in the right mode (USB or FM), *not* an FT8/JS8 dial. Use the band-plan presets. See [Frequency Plan](Frequency-Plan.md).
5. **Tier** — both ends must be on the **same tier**. FT1 and DX1 are different waveforms with different slot timing; a DX1 station won't decode on FT1 and vice-versa. The tier toggle is in the top bar — see [Tiers FT1 vs DX1](Tiers-FT1-vs-DX1.md).

## CAT / rigctld won't connect

Symptoms: the rig won't retune, or PTT via CAT does nothing.

1. **Rig Model** — confirm the right Hamlib model (Settings → Rig Control). If unsure, run `rigctl -l` to find your exact model number; the curated 56-model list is best-effort.
2. **COM port** — pick the correct serial port; hit **Refresh** to re-scan. Make sure no other program (WSJT-X, another logger, a previous Tempo instance) already holds the port.
3. **Baud** — match the rig's CAT baud rate.
4. **rigctld TCP port** — default `4532`. If something else is using it, change the **rigctld TCP Port** in Settings.
5. **Bundled rigctld** — installer builds ship Hamlib offline and prefer the bundled copy. If you run a *from-source* build that skipped the Hamlib fetch, put Hamlib's `rigctld.exe` on your `PATH`.

## Audio levels — clipping or too low

- **Clipping (RX meter red):** turn down the rig's audio output (or the sound card's input gain) until the meter sits in the green zone.
- **Too low (meter barely moves):** raise the input gain.
- **Transmit too hot:** lower the **Tx Power** slider (Settings → Audio) and watch your rig's ALC — overdrive splatters and decodes poorly. Use **Tune** to set it. See [Rig and Audio Setup](Rig-and-Audio-Setup.md).

## Time sync is off (No Sync / large dT)

Decoding needs an accurate UTC clock.

- Windows: Settings → Time & language → Date & time → **Sync now**, or run `w32tm /resync` from an elevated prompt.
- **Off-grid / no internet:** use a **GPS** or local **NTP** time source.
- A few hundred ms is fine; seconds of offset will cost decodes.

## PTT not keying (or won't stop)

1. **PTT Method** — confirm it matches your wiring: **CAT**, **Serial RTS**, **Serial DTR**, or **VOX** (Settings → Rig Control).
   - **VOX:** the rig's VOX must be enabled and tuned to key on transmit audio.
   - **Serial RTS/DTR:** the right control line and COM port must be selected.
   - **CAT:** see "CAT / rigctld won't connect" above.
2. **Monitor / Muted** — if the top-bar control shows **Muted**, Tempo is listen-only and won't key. Click it to **Monitor**.
3. **TX watchdog** — if a **TX watchdog** chip appeared in the top bar, transmit was auto-halted after too long keyed; re-enable **Monitor** to clear it (or raise/disable the watchdog in Settings → Operating).
4. **Won't stop transmitting:** hit **Stop TX** in the top bar — it drops PTT and halts the sequencer immediately.

---

## Still stuck?

- Re-check the setup pages: [Getting Started](Getting-Started.md), [Rig and Audio Setup](Rig-and-Audio-Setup.md).
- File an issue with details (band, dial, mode/tier, conditions): <https://github.com/kd9taw/tempo>.
