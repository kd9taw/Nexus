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
the signal itself. This is the clean answer for a rig that has no
CAT CW keying. Set the keyline port and which line to use in **Settings ▸ CW**. Nexus opens the
port the first time it sends and keeps it open while the serial keyline is the keyer; changing the
port or the line reopens it, and picking another keyer releases it. If the port cannot be opened,
the CW cockpit shows the error the system gave.

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

- **CAT, serial keyline and WinKeyer back-ends** → rig mode set to `CW`.
- **Soundcard back-end** → rig mode set to the DATA submode `PKTUSB` (≥10 MHz) or `PKTLSB`
  (<10 MHz), so the keyed tone reaches the transmitter from the sound card. With the radio's
  **Data modes use plain SSB** setting on (an interface wired to the MIC socket), it is plain `USB`
  or `LSB` instead.

The mode is re-asserted on section entry even if the frequency has not changed. You do not need a separate mode button. TX is armed automatically on CW section entry (`tx_enabled = true`), consistent with a live-key rig, unless you stopped it: after Stop TX, Esc, TX Off or the watchdog, TX stays off, here and on every other screen, until you turn it back on yourself by sending with an F-key or typed text, or with TX On. The FT8 auto-sequencer never applies to CW.

A prominent **band picker** in the cockpit header shows and selects your band. It is a large, bold control colored by the active band (matching the Conditions map's per-band spot colors), so your operating band reads at a glance. Selecting a band parks the VFO at the **start of your licensed CW segment** on that band and lists only bands your license class permits for CW. A **🔒 TX locked** chip appears when the current frequency/mode is outside your privileges.

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

Eight macros are fired by `F1`–`F8` or the corresponding on-screen buttons. One press sends once,
however long you hold the key, and a key pressed with Ctrl, Alt or Cmd held sends nothing (Alt+F4
closes the window, as it should). **Which eight depends
on state**, resolved in this order (`ui/src/components/CwCockpit.tsx:832`):

1. a saved **macro profile**, if one is active — see *Custom macro profiles* below;
2. otherwise the **Field Day set**, in ARRL Field Day and Winter Field Day;
3. otherwise the **contest set**, in any other contest you have picked whose exchange has a
   signal report, or the **contest set without a report** in one whose exchange has none;
4. otherwise the **default set**.

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

The contest sets and the Field Day set use the standard contest F-key layout, one key per
step of a contact, and they are what [Enter Sends Message](Field-Day.md#enter-sends-message-esm)
sends from: `F1` calls CQ, `F2` sends his call and your exchange, `F3` is TU, `F4` your call,
`F5` his call alone, `F6` your exchange when you answer someone else's CQ, `F7` tells a dupe
QSO B4 and `F8` asks for a repeat. Your exchange goes once, and your call goes out on `F1`, `F3` and
`F4`.

### The Field Day set

Active in ARRL Field Day and Winter Field Day, where `{EXCH}` expands to your class and section:
working K9AAA as 3A WI, `F2` sends `K9AAA 3A WI` and `F6` sends `TU 3A WI`.

| Key | Label | Content |
|---|---|---|
| `F1` | CQ FD | `CQ FD DE {MYCALL} {MYCALL} K` |
| `F2` | Exch | `! {EXCH}` |
| `F3` | TU | `TU {MYCALL}` |
| `F4` | My Call | `{MYCALL}` |
| `F5` | His Call | `! ` |
| `F6` | S&P exch | `TU {EXCH}` |
| `F7` | B4 | `! QSO B4` |
| `F8` | AGN | `AGN AGN` |

### The contest set

Active in any other contest you pick on the Contesting tab whose exchange has a signal report:
the QSO parties, CQ WW and CQ WPX. It calls `CQ TEST`: Field Day's `CQ FD` belongs to Field Day,
and keying it in the Illinois QSO Party or CQ WW CW calls for somebody else's event. `{EXCH}` is
that contest's own exchange without the signal report — your zone in CQ WW CW, your county in a
QSO party — so the keys that send it send `{RST}` before it. In the Illinois QSO Party from Cook
County, working K9AAA: `F2` sends `K9AAA 5NN COOK`, `F3` sends `TU <your call>`, `F6` sends
`TU 5NN COOK` and `F7` sends `K9AAA QSO B4`.

| Key | Label | Content |
|---|---|---|
| `F1` | CQ TEST | `CQ TEST DE {MYCALL} {MYCALL} K` |
| `F2` | Exch | `! {RST} {EXCH}` |
| `F3` | TU | `TU {MYCALL}` |
| `F4` | My Call | `{MYCALL}` |
| `F5` | His Call | `! ` |
| `F6` | S&P exch | `TU {RST} {EXCH}` |
| `F7` | B4 | `! QSO B4` |
| `F8` | AGN | `AGN AGN` |

### The contest set without a report

Active in a contest whose exchange has no signal report: Sweepstakes, the California QSO Party
and the ARRL VHF contests. A 5NN there would be a wrong exchange, so `F2` and `F6` leave
`{RST}` out.

| Key | Label | Content |
|---|---|---|
| `F1` | CQ TEST | `CQ TEST DE {MYCALL} {MYCALL} K` |
| `F2` | Exch | `! {EXCH}` |
| `F3` | TU | `TU {MYCALL}` |
| `F4` | My Call | `{MYCALL}` |
| `F5` | His Call | `! ` |
| `F6` | S&P exch | `TU {EXCH}` |
| `F7` | B4 | `! QSO B4` |
| `F8` | AGN | `AGN AGN` |

**These sets changed with Enter Sends Message: `F3` is now TU.** Before, `F2` answered with his
call and yours, `F3` sent the exchange twice with "DE <your call> … K", `F4` the TU, `F5` your
call, `F6` his call and `F8` a question mark. A macro profile you copied from one of these sets before keeps the texts it
had: Nexus never changes a profile of your own.

### Macro Tokens

| Token | Expands to |
|---|---|
| `{MYCALL}` | Your callsign (from Settings) |
| `{NAME}` | Your name (`op_name` in Settings; empty by default until set) |
| `{MYGRID}` | Your Maidenhead grid square |
| `{RST}` | `5NN` (hardcoded 599 with cut numbers: 9→N, 0→T) |
| `{EXCH}` | Your contest exchange without the report: class and section in Field Day, the running contest's own exchange otherwise, with the serial number where the contest has one. Empty outside a contest. |
| `!` | The worked callsign: the call in His Call, which is also the log strip's call (typed in either, a decoded call you clicked, a Needed-board click, or until one of those the decoder's best guess) |

If `{NAME}` or `!` is empty, the token collapses and surrounding whitespace is normalized — no double-space appears mid-message.

**RST note:** the RST token always sends `5NN`; there is no per-QSO RST input.

### Custom macro profiles

Macro text is editable and savable. A **macro profile** is a named set of all eight; save as many
as you like and switch the active one from the CW cockpit (`ui/src/types.ts`, `cwProfiles`). An
active profile takes precedence over every built-in set above, Field Day included. A profile also
keeps which of its keys [Enter Sends Message](Field-Day.md#enter-sends-message-esm) sends for each
step, mapped in Settings → Contesting ▸ Enter Sends Message.

A key can also be changed where it is, in the cockpit: its **✎**, a right-click on it, or
`Ctrl+Shift` with its F-key opens the macro editor RTTY and PSK use, and none of them sends.
**Save** writes that one key into the ACTIVE profile (`cwProfiles[activeCwProfile]`), the list
Settings → CW → Macros edits. A profile with no keys of its own first takes the eight the dock showed
(the running contest's or Field Day's set during one, else the casual set), with that set's Enter
Sends Message steps when it is one of the contest layouts, so Enter keeps sending the same keys.
**Send once** sends the edited text through the key's own send path (the His call commit, the
licence check, `send_cw` and every engine gate behind it) and saves nothing.

**RST stays `5NN` regardless.** There is no per-QSO RST input, and a profile does not change that.

---

## Typed Text Input

Type any free-form text in the text box and press `Enter` or click **Send**. The box clears after send. Both macros and typed text join the same queue and are sent in order.

---

## Turn Cue

If you copy CW with the decoder rather than by ear, it is hard to hear when the other station has
finished and it is your turn. The **turn cue** says so. It is a small lamp at the start of the send
row, in front of His Call, and it lights the F-key that fits:

| Lamp | What it means |
|---|---|
| `◌ Listening` | Nothing heard lately. |
| `≋ W1ABC sending` | The other station is sending. |
| `≋ W1ABC finishing…` | Their over has just ended on K, KN or a call. The key that fits gets a dashed outline. |
| `▶ Your turn · F3` | Your turn to send. The key that fits glows: F2 to answer a CQ, F3 when a station has come back to you, F5 for your call once. With no key that fits (in a ragchew), it says `Your turn · type`. |
| `▶ Probably you · F5 once` | The other station sent part of a call that matches yours, like `KD9?`. |
| `⏸ Hold · W2XYZ` | Not your turn: the over was for another station. |
| `⇡ UP · split off` | The DX listens above his frequency and your split is off. |
| `? Not sure · F7` | A long silence, and nothing said whose turn it is. F7 (AGN) gets a dotted outline. |
| `● You're sending` | You are keying. |
| `◔ Listening for a reply` | Your over has ended. After a CQ or a call the ring empties while you wait, then the lamp says `No reply`. |
| `✓ 73 · log it` | They are signing. F4 (73) glows. |

**Your turn needs silence AND an end marker** — K, KN, BK, QRZ, AGN, a `?` after part of a call,
UP, a CQ ending on the caller's call, or TU in a contest. Silence alone only gives `Not sure`. The
lamp waits for what the decoder has actually heard, and it takes the turn back the moment the
classic decoder hears keying again at your pitch. Your own over is never read as theirs: letters
heard while you are keying (a radio that feeds its sidetone back) are ignored.

When a DX station comes back to you in a pileup (`KD9TAW 5NN`), no key glows: F3 would send a whole
ragchew reply. The lamp says `Your turn · send 5NN TU`. In a contest the keys are the contest set's
(F4 your call, F6 your exchange, F3 TU), and with **Enter Sends Message** on, ESM lights its own
keys and the lamp says `Your turn · Enter`.

**It never transmits.** It only lights a key and says what fits. Every send is still your own key
press, and nothing is held back: a key pressed while the lamp says the other station is sending
still sends.

**Pace.** With the turn cue on, the Decode pane's speed readout reads `them 18 · you 25 WPM`, with a `⇣`
and an amber outline when you are 3 WPM or more faster. Click it to send at their speed for this
contact; your saved speed does not change.

**When it shows.** Only with the AI decoder on and the Decode pane shown, and never in an
unassisted contest entry (no CW decoder output shows there). To hide it for good, untick
**Turn Cue** in ⊞ Panels. A screen reader hears "Your turn" and "Hold" once per over through the
announcements in **Settings → Appearance ▸ Accessibility & eyes-free** ("Not sure" and "No reply"
only at the fullest setting). A sound is there too, off until you turn on **CW turn sound** in the
same section: a short rising chirp, never while you transmit.

**Timing.** The AI decoder's copy lands about a second after a letter is sent, so the cue comes about
a second after the other station stops. That is in time for a ragchew or an answer to a CQ, tight
for a contest's search and pounce, and too slow to win a pileup race. There its value is telling you
when *not* to call.

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

The boards can also sit on the CW screen itself: **⊞ Panels ▸ Spots** and **⊞ Panels ▸ Needed** (both unticked until you tick them, as on the Phone screen) add them as panes. Spots opens on the CW spots on the band your radio is on and follows the band; Needed opens on the CW needs. Each keeps its own filters, apart from the Spots and Needed screens'. A click on a row in either is the same click as on its screen — the same QSY and the same prefill — and nothing keys.

---

## Log Strip

The log strip at the bottom of the CW cockpit pre-fills:

- **Mode:** `CW`
- **RST sent/received:** `599`

**Its callsign is His Call.** The log strip's callsign and His Call, on the send row, are one
field shown twice: a call typed in either shows in both, and it is the call every F-key sends for
`!`, in a contest and out of one. A decoded call goes into both when you click its chip.

**Until you work a station or type a call, it holds the decoder's best guess.** Both boxes show
the guess, unconfirmed (the strip does not look a guess up in the callbook), and it follows a
better guess as the decode goes on. The F-keys send it unless you correct it first: type over it
in either box, or click another call's chip. Once you type, it stops following. A station you
click to work, a spot or a chip, replaces it at once. Logging a contact empties both; the next
guess then fills them again, but not the call you just logged.

Complete the callsign (or accept the prefill from a Needed click), adjust RST if needed, and press **Log** to commit the QSO. The entry goes to the main logbook, triggers LoTW/QRZ/eQSL/ClubLog sync if connectors are configured, and updates awards tracking.

---

## Split TX Indicator

If the rig has a split TX frequency set (`splitTxMhz` in the radio snapshot), a **SPLIT ▲** badge appears in the CW cockpit bar showing the TX dial frequency. Nexus does not command split for CW automatically; this badge reflects a split the operator or another app has set on the rig.

---

## Settings Reference

| Setting | Default | Notes |
|---|---|---|
| `cw_keyer` | `cat` | `cat`, `serial`, `soundcard`, or `winkeyer` |
| `cw_key_port` | *(empty)* | Serial port of the DTR/RTS keyline (e.g. `COM7`), separate from the CAT port; used when `cw_keyer` is `serial` |
| `cw_key_line` | `dtr` | `dtr` or `rts`: the line that keys the rig on the serial keyline |
| `winkeyer_port` | *(empty)* | Serial port of the K1EL WinKeyer (e.g. `COM6`); used when `cw_keyer` is `winkeyer` |
| `cw_wpm` | `25` | Range 5–50; WPM_MIN=5, WPM_MAX=50 |
| `cw_pitch_hz` | `600.0` | Range 300–1200, step 10; used as scope hairline and soundcard tone |
| `op_name` | *(empty)* | Expands `{NAME}` in macros; set in **Settings → Station** |
| TX tail (soundcard) | `250 ms` | Fixed; applied after audio flush on PTT release |

---

## Limits / Not Yet

- **No auto-sequencer:** every CW transmission starts with your own key press. In a contest,
  [Enter Sends Message](Field-Day.md#enter-sends-message-esm) sends the contact's next message each
  time you press Enter in the log strip (off until you turn it on), and never sends on its own. The
  FT8 seven-state sequencer is not used for CW.
- **No paddle/iambic input through the app:** the only input paths are F-key macros and typed text. For live paddle feel, connect paddles to the rig or to the WinKeyer directly.
- **AI decoder (the default):** CW copy is powered by a neural-network decoder (the
  DeepCW model by e04, AGPL-3.0 — see NOTICE) reading the whole 400–1200 Hz window:
  dramatically better weak-signal copy than pitch-tracking decoders, as a flowing transcript.
  Letters land about a second after they are sent (a little later on a slow computer); the last
  few, which the decoder may still correct, show at once in a dimmer color. The **Clear** button wipes it; the AI toggle in the
  DECODE header falls back to the classic pitch decoder (also used automatically if the
  model file is missing). Fused prosigns (<AR>, <BT>) aren't in the model's alphabet and
  can appear as letter fragments.
- **Off in an unassisted contest entry:** with Settings → Contesting ▸ Unassisted entry on, neither
  decoder runs (CQ WW counts a CW decoder as assistance): no copy, call chips, His Call guess or
  speed, and the Decode pane says why.
- **Not a full-band skimmer:** the decode follows the audio passband, not every signal on
  the band at once; the WPM readout is estimated by the classic decoder underneath.
- **Turn cue limits:** it reads the one signal you are copying, about a second behind the air. A
  run-together prosign the AI decoder prints as other letters is no end marker to it, so such an
  over ends in `Not sure` rather than `Your turn`; and a second station inside the decoder's window
  can hold `sending` up or look like the end of the over.
- **CAT abort reliability:** `\stop_morse` varies by Hamlib version and rig manufacturer; older builds and some rigs may not halt mid-element.
- **Soundcard PTT tail:** 250 ms, fixed; not configurable.
- **Desktop-only:** Tauri v2; no web or mobile build.

---

[← Phone Cockpit](Phone.md) | [Operating Guide](Operate-FT8-FT4.md) | [Rig and Audio Setup →](Rig-and-Audio-Setup.md)
