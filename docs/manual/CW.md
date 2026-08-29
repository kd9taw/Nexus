# CW Cockpit

Nexus ships a complete casual/ragchew CW operating surface: four keyer back-ends, eight F-key macros with live token expansion, a narrow AF scope for zero-beating, and privilege-gated transmit — all wired into the same CAT/PTT infrastructure the digital and phone modes share.

---

## Choosing a Keyer Back-End

Nexus offers four keyer back-ends. Pick one from the **Keyer** dropdown in the CW cockpit's header
— as of 1.0.0 they are a single dropdown there, not four buttons and not a Settings tab. Each
back-end's own port settings still live in Settings.

### CAT (default)

The rig generates the actual Morse. Nexus sends the character string to Hamlib via `send_morse`; Hamlib drives the rig's internal keyer over CAT. Speed is synchronized to the rig via the Hamlib `KEYSPD` parameter and is pushed to the rig only when it changes.

**When to use:** your rig has a stable internal keyer and you want its sidetone and QSK timing. This is the most reliable path on any Hamlib-supported rig.

### Serial keyline

Nexus toggles **DTR or RTS** on a serial port into the rig's KEY jack; the rig is in CW and shapes
the signal itself. This is the N1MM/fldigi method, and it is the clean answer for a rig that has no
CAT CW keying. Set the keyline port and which line to use in **Settings ▸ CW**. The service holds
the port open for the session (`crates/tempo-audio/src/service.rs:1515-1518`).

**When to use:** your rig has a key jack and no usable CAT keyer, and you want the rig's own
timing and QSK rather than software-generated audio.

### Soundcard

Nexus generates PCM Morse audio at 12 kHz sample rate following PARIS timing (dit = 1.2/wpm s, dah = 3 dits, inter-character gap = 3 dits, word gap = 7 dits). A 5 ms raised-cosine attack and decay on every element eliminates key clicks. Audio is fed through the TX sound card path and PTT is keyed over the configured PTT method (CAT, RTS, DTR, or VOX).

**When to use:** your rig does not support CAT keying, you are operating into a dummy load or audio interface, or you need provably click-free keying generated in software.

### K1EL WinKeyer

A K1EL WinKeyer (WK1/WK2/WK3) generates the Morse timing in hardware and keys the rig directly. Nexus opens the keyer over serial (1200 baud 8N2, host mode) on the port set in `winkeyer_port` (e.g. `COM6`) and streams the character text to it; WPM changes are pushed to the keyer (clamped to the WK range 5–99).

**When to use:** you already run a WinKeyer in the shack, or you want hardware-timed keying independent of CAT and soundcard latency.

The back-end is switchable live mid-session; no restart is required.

---

## Rig-Mode Policy

Entering the CW section commands the rig via CAT before any transmit:

- **CAT and WinKeyer back-ends** → rig mode set to `CW`.
- **Soundcard back-end** → rig mode set to `USB` (≥10 MHz) or `LSB` (<10 MHz).

The mode is re-asserted on section entry even if the frequency has not changed. You do not need a separate mode button. TX is armed automatically on CW section entry (`tx_enabled = true`), consistent with a live-key rig. The FT8 auto-sequencer never applies to CW.

A prominent **band picker** in the cockpit header shows and selects your band. It is a large, bold control colored by the active band (matching the Connect map's per-band spot colors), so your operating band reads at a glance. Selecting a band parks the VFO at the **start of your licensed CW segment** on that band and lists only bands your license class permits for CW. A **🔒 TX locked** chip appears when the current frequency/mode is outside your privileges.

---

## Speed Control

| Action | Effect |
|---|---|
| WPM slider | Sets speed immediately; range 5–50 WPM; default **25** |
| `PgUp` | +2 WPM |
| `PgDn` | −2 WPM |
| `Shift`+`PgUp` | +4 WPM |
| `Shift`+`PgDn` | −4 WPM |

Speed changes apply to the next word keyed, on every back-end.

On the **WinKeyer** back-end the new speed is also sent to the keyer the moment you move the slider — while it is idle between overs, and part-way through a message too, because a WinKeyer holds a speed of its own and would otherwise keep sending at whatever its front-panel pot or power-on default gave it. Nexus opens the keyer's port on the first word you send, and re-sends the speed whenever that port opens, so a keyer plugged in late or power-cycled mid-session comes back at your speed rather than its own.

On the **CAT** back-end the new WPM reaches the rig via `KEYSPD` at the start of the next send, not while you are idle. That is deliberate: the CAT link also carries dial, mode and meter traffic, and one drag of the slider is dozens of values.

---

## Zero-Beat Scope and Pitch

The scope shows a narrow AF spectrum with a dashed vertical hairline at your configured pitch frequency. To zero-beat a received CW signal, tune the VFO until the signal's peak lands on the hairline.

Pitch is adjustable **300–1200 Hz** in 10 Hz steps; default is **600 Hz**. Changing pitch repositions the hairline and updates the soundcard tone frequency in the same call. The pitch setting persists across sessions.

The scope window is **centred on your pitch** and follows your rig's CW filter width (the filter plus a quarter of it in skirt, floored at 300 Hz), so what you are tuned to sits in the middle of the picture the way it does on a rig.

### The zero-beat indicator

Beside the scope title, **ZERO BEAT** reads the tone actually coming in and compares it with your pitch:

- The **light** comes on when you are on pitch. How close counts follows your CW filter — 5% of the filter width, so 25 Hz behind the usual 500 Hz filter, tighter behind a narrow one and never tighter than 15 Hz (below that you would be chasing the measurement rather than the signal).
- The **needle and the offset in Hz** say which way and how far. The needle runs in the same direction as the scope below it — low audio left, high audio right — so "centre the needle" and "put the peak on the hairline" are the same move. Full scale either way is 400 Hz.
- With **several signals in the passband** it follows the one nearest the hairline, with the stronger winning a tie. That is deliberate: once you are closing in on somebody, a louder station 300 Hz away must not steal the reading.
- On a **dead band** it reads *no signal* and draws no needle. It will hold a reading across keying gaps for about two seconds — CW spends most of its time between elements — but a station that has stopped goes blank rather than leaving a number standing.

It is a display only. Nexus never moves your dial to zero-beat for you.

---

## Eight F-Key Macros

Eight macros are fired by `F1`–`F8` or the corresponding on-screen buttons. **Which eight depends
on state**, resolved in this order (`ui/src/components/CwCockpit.tsx:470`):

1. a saved **macro profile**, if one is active — see *Custom macro profiles* below;
2. otherwise the **Field Day set**, whenever Field Day mode is on;
3. otherwise the **default set**.

### The default set

| Key | Label | Content |
|---|---|---|
| `F1` | CQ | `CQ CQ DE {MYCALL} {MYCALL} K` |
| `F2` | Call | `! DE {MYCALL} {MYCALL} K` |
| `F3` | Reply | `! DE {MYCALL} UR {RST} {RST} NAME {NAME} {NAME} HW? KN` |
| `F4` | 73 | `! DE {MYCALL} TU 73 SK` |
| `F5` | My Call | `{MYCALL}` |
| `F6` | His Call | `! ` |
| `F7` | AGN | `AGN AGN` |
| `F8` | ? | `? ` |

**`F3` sends the report and `F4` ends the contact.**

### The Field Day set

Active whenever Field Day mode is on. `{EXCH}` expands to your class and section.

| Key | Label | Content |
|---|---|---|
| `F1` | CQ FD | `CQ FD DE {MYCALL} {MYCALL} K` |
| `F2` | Call | `! DE {MYCALL} K` |
| `F3` | Exch | `! DE {MYCALL} {EXCH} {EXCH} K` |
| `F4` | TU | `! TU {EXCH} DE {MYCALL} K` |
| `F5` | My Call | `{MYCALL}` |
| `F6` | His Call | `! ` |
| `F7` | AGN | `AGN AGN` |
| `F8` | ? | `? ` |

### Macro Tokens

| Token | Expands to |
|---|---|
| `{MYCALL}` | Your callsign (from Settings) |
| `{NAME}` | Your name (`op_name` in Settings; empty by default until set) |
| `{MYGRID}` | Your Maidenhead grid square |
| `{RST}` | `5NN` (hardcoded 599 with cut numbers: 9→N, 0→T) |
| `{EXCH}` | Your Field Day exchange — class and section (Field Day macro set only) |
| `!` | The worked callsign (the callsign prefilled by a Needed-board click or typed by you) |

If `{NAME}` or `!` is empty, the token collapses and surrounding whitespace is normalized — no double-space appears mid-message.

**RST note:** the RST token always sends `5NN`. There is no serial-number field and no per-QSO RST input; the CW cockpit is casual/ragchew only by design.

### Custom macro profiles

Macro text is editable and savable. A **macro profile** is a named set of all eight; save as many
as you like and switch the active one from the CW cockpit (`ui/src/types.ts`, `cwProfiles`). An
active profile takes precedence over both built-in sets above, Field Day included.

**RST stays `5NN` regardless.** There is no serial-number field and no per-QSO RST input — the CW
cockpit is casual/ragchew by design, and a profile does not change that.

---

## Typed Text Input

Type any free-form text in the text box and press `Enter` or click **Send**. The box clears after send. Both macros and typed text join the same queue and are sent in order.

---

## Tune

Click **Tune** to key a steady, unmodulated carrier for tuning an antenna tuner (ATU) or setting an amplifier. It shows **TUNING…** while the carrier is on; click again to stop. The Transmit Watchdog also drops it automatically, and it's disabled when the current frequency is outside your license privileges.

## Stop TX

Press `Esc` or click the **Stop TX** button at any time to:

1. Clear the entire CW send queue immediately.
2. On **CAT back-end**: send Hamlib `\stop_morse` to halt the rig's keyer in place.
3. On **Soundcard back-end**: flush the audio output ring and release PTT (250 ms TX tail remains, not configurable).
4. On **WinKeyer back-end**: send the WK Clear-Buffer command, which stops keying immediately and flushes the keyer's send buffer.
5. Drop any **tune carrier** and release PTT (a true stop-everything).

The abort flag is consumed exactly once; a subsequent send starts cleanly.

> **Note:** `\stop_morse` reliability varies by Hamlib version and rig manufacturer. If your rig does not stop mid-element on CAT abort, switch to the Soundcard back-end.

---

## Privilege Gating

TX is blocked when the operating frequency falls outside your declared license class's FCC sub-band allocation:

- The **engine** guards `poll_cw` with `tx_allowed()` before keying anything.
- The **UI** pre-checks `txAllowed` before calling the send command and shows a toast: *"TX locked — this frequency is outside your license privileges."*

A locked frequency does not prevent you from changing the VFO; it only prevents transmitting until you move to a legal segment.

**Technician privileges on 80/40/15 m:** Technician licensees are permitted CW only in those bands. Nexus allows CW TX in those segments and blocks Digital and Phone. Move to a Technician CW sub-band and the CW cockpit transmits normally.

---

## Needed Board — Click-to-Work

The Needed board surfaces stations you have not yet worked (ATNO, new band-slot, new mode, etc.) alongside live propagation evidence. From the CW cockpit's perspective:

- **Single click** on a Needed row → atomically QSYs the rig (band + frequency + mode), opens the CW cockpit, and prefills the callsign in both the macro `!` token and the log strip.
- **Map double-click** on a CW spot → same `workSpot` path; the cockpit opens ready to call.

Focus lands on the RST field in the log strip after prefill so you can tab to confirm and log immediately after the QSO.

---

## Log Strip

The log strip at the bottom of the CW cockpit pre-fills:

- **Mode:** `CW`
- **RST sent/received:** `599`

Complete the callsign (or accept the prefill from a Needed click), adjust RST if needed, and press **Log** to commit the QSO. The entry goes to the main logbook, triggers LoTW/QRZ/eQSL/ClubLog sync if connectors are configured, and updates awards tracking.

---

## Split TX Indicator

If the rig has a split TX frequency set (`splitTxMhz` in the radio snapshot), a **SPLIT ▲** badge appears in the CW cockpit bar showing the TX dial frequency. Nexus does not command split for CW automatically; this badge reflects a split the operator or another app has set on the rig.

---

## Settings Reference

| Setting | Default | Notes |
|---|---|---|
| `cw_keyer` | `cat` | `cat`, `soundcard`, or `winkeyer` |
| `winkeyer_port` | *(empty)* | Serial port of the K1EL WinKeyer (e.g. `COM6`); used when `cw_keyer` is `winkeyer` |
| `cw_wpm` | `25` | Range 5–50; WPM_MIN=5, WPM_MAX=50 |
| `cw_pitch_hz` | `600.0` | Range 300–1200, step 10; used as scope hairline and soundcard tone |
| `op_name` | *(empty)* | Expands `{NAME}` in macros; set in **Settings → Station** |
| TX tail (soundcard) | `250 ms` | Fixed; applied after audio flush on PTT release |

---

## Limits / Not Yet

- **No ESM auto-sequencer:** CW is manual-only. The FT8 seven-state sequencer is explicitly excluded from CW.
- **No paddle/iambic input through the app:** the only input paths are F-key macros and typed text. For live paddle feel, connect paddles to the rig or to the WinKeyer directly.
- **AI decoder (the default):** CW copy is powered by a neural-network decoder (the
  DeepCW model by e04, AGPL-3.0 — see NOTICE) reading the whole 400–1200 Hz window:
  dramatically better weak-signal copy than pitch-tracking decoders, updating every few
  seconds as a flowing transcript. The **Clear** button wipes it; the AI toggle in the
  DECODE header falls back to the classic pitch decoder (also used automatically if the
  model file is missing). Fused prosigns (<AR>, <BT>) aren't in the model's alphabet and
  can appear as letter fragments.
- **Not a full-band skimmer:** the decode follows the audio passband, not every signal on
  the band at once; the WPM readout is estimated by the classic decoder underneath.
- **Macros not user-editable:** the 8 slots and their text are compiled in; no UI for custom macro text in this version.
- **No contest exchange:** RST is hardcoded to 599, no serial-number field exists. This cockpit is casual/ragchew only.
- **CAT abort reliability:** `\stop_morse` varies by Hamlib version and rig manufacturer; older builds and some rigs may not halt mid-element.
- **Soundcard PTT tail:** 250 ms, fixed; not configurable.
- **Desktop-only:** Tauri v2; no web or mobile build.

---

[← Phone Cockpit](Phone.md) | [Operating Guide](Operate-FT8-FT4.md) | [Rig and Audio Setup →](Rig-and-Audio-Setup.md)
