# Operating Guide

This is the day-to-day manual for actually working stations with Tempo. If you haven't set up your station yet, start with [Getting Started](Getting-Started.md) and [Rig and Audio Setup](Rig-and-Audio-Setup.md).

---

## The three-zone layout

Tempo is a single window with three vertical zones:

1. **Left — mode bar.** Switch views: **Chat**, **QSO**, **Field Day**, **Band**, **Logbook**, **Field Log**, and **Settings** (gear at the bottom). The currently-active operating mode is shown as a live badge.
2. **Center — the active view.** Conversations, the QSO sequencer panel, the Field Day workspace, or the band-activity broadcast feed, depending on what you've selected.
3. **Around the edges — the radio.** The **top bar** always shows your call/grid, the frequency control, TX/RX state, the RX level meter, the operating controls (**Monitor/Muted**, **Tune**, **Stop TX**), the slot-clock countdown, the **UTC clock**, time-sync health, the current **dT**, and the **tier toggle**.

The waterfall and station list sit alongside, so the radio is never hidden behind the chat.

---

## The top bar (always visible)

- **Call / grid** — your identity.
- **Frequency control** — a band-plan preset dropdown plus a manual **MHz** entry and a **USB/FM** toggle. Changing it retunes the rig live (with CAT). See [Frequency Plan](Frequency-Plan.md).
- **TX / RX indicator** — lit when transmitting.
- **RX level meter** — your receive audio level; aim mid-scale (green), red = clipping.
- **Monitor / Muted** — the global transmit enable. *Monitor* = transmit allowed; *Muted* = listen-only (Tempo decodes but never keys). Click to toggle.
- **Tune** — keys a steady carrier so you can set power / tune an antenna; click again to stop.
- **Stop TX** — drops PTT and halts the sequencer immediately.
- **TX watchdog chip** — appears if the transmit watchdog auto-halted you after too long keyed; re-enable Monitor to clear it.
- **Slot clock** — counts down to the next T/R slot boundary (4 s for FT1, 15 s for DX1).
- **UTC clock + time-sync** — ticking UTC time and a **Sync / No Sync** health dot derived from observed decode timing (dT). Accurate time is essential for decoding — see [Rig and Audio Setup](Rig-and-Audio-Setup.md).
- **dT readout** — the measured decode time offset of the stations you're hearing.
- **Tier toggle** — **Fast (FT1)** vs **Robust (DX1)**. This is **never silent** — the active tier is always shown. See [Tiers FT1 vs DX1](Tiers-FT1-vs-DX1.md).
- **Theme switcher** — Light / Dark / Amber-Night.

---

## The operating modes

Tempo's engine runs one of three sequencers at a time. All three carry the **same 77-bit messages**, so they work identically whether you're on the Fast or Robust tier.

### Chat
Free-form, conversational text with presence and a directed inbox.

- **Roster / station list** — built passively from decodes. Filter by **All**, **Heard now** (active presence), or **Beaconing** (heard repeatedly). Stations sort by presence then SNR.
- **Free text** — type any message; Tempo word-wraps and **auto-chunks** it across multiple frames (each FT1 free-text frame carries ~13 characters) and reassembles on the far end, even if chunks arrive out of order.
- **Directed inbox** — because free-text frames carry no callsign, Tempo attributes them by temporal association: an identifying frame (a CQ, beacon, or directed `TO FROM …` frame) names the current talker, and the following free-text chunks are attributed to that station. In practice Tempo precedes your free text with an identifying frame automatically.
- **Store-and-forward** — directed messages to a station that isn't reachable right now are queued and released as a burst when that station becomes **active** (heard within a window). Attempts back off between tries and stop on confirmed delivery. This is the off-grid net feature.
- **Quick-reply macros** — editable one-tap chips (defaults include `73`, `QSL`, `Name?`, `QTH?`, `CQ`). Edit them under **Settings → Quick-reply Macros**.

### QSO
A 1:1 auto-sequenced contact. Two roles, chosen with the buttons in the QSO panel:

- **Call CQ (Running)** — Tempo calls CQ and works answers as they arrive.
- **Monitor (Search & Pounce)** — Tempo answers the next CQ it decodes.

The sequencer advances one step per slot and picks up the contact automatically — no manual targeting needed once you've chosen a role. The panel shows the sequencer **state**, the **DX call**, the **RX report**, and the current **role**. QSO macros default to `R-09`, `RRR`, `RR73`, `73`.

### Field Day
The ARRL Field Day workspace. The exchange is **Class + ARRL/RAC Section** (e.g. `3A WI`), carried natively in one frame.

- Two roles: **Running** (calls `CQ FD`) and **S&P** (search-and-pounce). Field Day contacts are **operator-initiated** — Field Day prohibits fully-automated QSOs, so this is by design.
- **Scoreboard** — live **QSO count**, **distinct Sections** (the multiplier), and **points** (digital QSOs score 2 points each), plus the sequencer state.
- **Dupe-checked log** — duplicate callsigns are flagged, and a section's first appearance is tagged **Mult!**.
- **Export** — both **ADIF** and **Cabrillo** from the Field Log view.

---

## Working a station

There are two one-tap ways to start a directed contact:

- In the **decode feed** (Band Activity), each row has a button — **Call** for a station calling CQ, **Work** for anyone else. Click it to start a directed QSO with that station.
- In the **station list**, select or call a heard station the same way.

Tempo takes it from there with the QSO sequencer.

### Inbound double-click-to-call
If you run **GridTracker** or **JTAlert** alongside Tempo, their double-click-to-call sends a WSJT-X **Reply** over the UDP API, and Tempo will act on it. See the interop notes in [Architecture and Protocol](Architecture-and-Protocol.md).

---

## The live decode feed (color coding)

The **Band Activity** decode list shows the decodes from the last RX slot. Each row is color-coded by priority (highest wins):

| Class | Meaning |
|-------|---------|
| **Directed to you** (`YOU` tag) | Someone is calling *you*. |
| **Worked before** (`B4` chip) | You've already worked this station (from your logbook). |
| **CQ** (`CQ` tag) | A station calling CQ — tap **Call** to answer. |
| **New** | A station not previously highlighted. |

Each row also shows the **SNR** (color-graded good / ok / weak), the audio **frequency** in Hz, and the message. The `B4` highlighting is driven by your persistent ADIF logbook.

---

## Alerts, UTC clock, and bearing

- **Alerts** (Settings → Alerts) give an audible beep + visual flash. Three independent toggles: **My call** (someone directs a call at you — on by default), **CQ calls** (any decoded CQ), and **New stations** (a station not heard this session).
- The **UTC clock** ticks in the top bar; the **time-sync** dot warns if your decode timing drifts (fix the PC clock — see [Rig and Audio Setup](Rig-and-Audio-Setup.md)).
- Tempo computes a great-circle **bearing** to stations from your grid, so you know where to point a directional antenna.

---

## Open broadcast + the band feed

The **Band** view is a to-everyone broadcast feed — an FT8-style "to all" message rather than a directed one. Tempo embeds your call as a `DE <CALL> …` prefix so receivers can attribute it, and routes inbound `DE <CALL> …` traffic into this feed. A clear **BROADCAST** pill reminds you it goes to *everyone* on frequency, not to one station. Band macros default to `CQ CQ`, `QRZ?`, `Net check-in`, `73 to all`.

---

## Passive vs. the opt-in beacon

Tempo **starts passive** — it listens and decodes but won't transmit until you act. The **CQ beacon** (Settings → Operating → Beacon) is **off by default**; turn it on to periodically announce your presence with `CQ <call> <grid>` in Chat mode. Even with the beacon on, the **Monitor / Muted** top-bar control lets you instantly drop to listen-only.

---

## The ADIF logbook + worked-before (B4)

- The **Logbook** view shows your persistent ADIF contacts. Completed auto-sequenced QSOs are **auto-logged** (Settings → Operating → Auto-log QSOs, on by default), and you can add a contact by hand with the **Log QSO** form (call, grid, band, freq, mode, RST sent/rcvd).
- Logged contacts drive the **B4** highlighting in the decode feed, so you can see at a glance who you've already worked.
- The log lives in `log.adi` and exports as standard ADIF; Field Day additionally exports **Cabrillo**.
