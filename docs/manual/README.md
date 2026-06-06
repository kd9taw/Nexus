# Tempo — Operator Manual

**Tempo is a modern, chat-first HF text-messaging app for the off-grid / preparedness ham community.**

It wraps a fast weak-signal waveform (**FT1**) and a new fading-resilient mode (**DX1**) behind a clean, single-window interface that feels like a modern messenger — without hiding the radio. You get fast conversational two-way text, fading-resilient regional-to-national reach, presence/heartbeat, store-and-forward relay, and a one-click ARRL **Field Day** exchange.

> ### Try it / get it
> - **Download for Windows:** <https://github.com/kd9taw/tempo/releases/latest>
> - **New here? Start with [Getting Started](Getting-Started.md).**

---

## Honest beta note — please read

Tempo is **beta**. Its two waveforms (FT1 and DX1) are **validated by simulation only** — AWGN and Rayleigh-fading sweeps — and have **not yet been proven on the air**. On-air decode-rate-vs-SNR is the project's remaining gate. The published Windows binaries are **cross-compiled** builds. Treat this as experimental software, verify it on your own station, and operate within your license privileges. Honest on-air reports are the single most useful thing you can contribute.

The **FT8/FT4 tier is Phase 2** (its internals are compiled into the modem, but no decode pipeline is wired). DX1 now does **full-passband acquisition** — it decodes every signal across 200–2900 Hz per slot, like FT1's Costas search; your tuned RX offset is just a waterfall marker / TX-pairing hint. **IR-HARQ** is live end-to-end too (on by default), joint-combining retransmissions for weak-signal rescue. Both are simulation- and Windows-cross-build-validated, **not yet proven on the air**.

---

## The two-tier idea

Weak-signal text faces a real physics tradeoff: **cycle time vs. weak-signal reach**. You can't have a single waveform that is both the fastest *and* the most sensitive. Tempo's answer is a **tiered architecture** with a chat layer on top and an **always-visible tier toggle** — the operator picks Fast or Robust and keeps talking; the tier is never switched silently.

| Tier | Waveform | T/R | Character | Sim AWGN 50% threshold |
|------|----------|-----|-----------|------------------------|
| **Fast** | **FT1** — 4-CPM, coherent, IR-HARQ | **4 s** | conversational | ≈ −15 dB |
| **Robust** | **DX1** — non-coherent 8-FSK + soft LDPC(174,91) | **15 s** | fading-immune | ≈ −18.6 dB (≈ 3.7 dB fading penalty) |

Both tiers carry the **same 77-bit messages**, so Chat, QSO, and Field Day work identically on either. (Thresholds above are **simulation results**, not on-air claims.) See [Tiers FT1 vs DX1](Tiers-FT1-vs-DX1.md).

---

## Features at a glance

- **Chat-first, ham-aware UI** — people and conversations like a modern messenger, but SNR, audio offset, dT, dial/band/sideband, mode/tier, and T/R timing stay first-class. Three field themes: **Light**, **Dark**, **Amber-Night** (night-vision-safe).
- **Prominent, modernized waterfall** with palettes, RX/TX markers, and telemetry.
- **Operating modes:** Chat (presence, free-text auto-chunked across frames, directed inbox, store-and-forward), QSO (auto-sequencer), and Field Day (native exchange, dupe-checked log, ADIF/Cabrillo export).
- **Open broadcast** plus a color-coded **live decode feed** (CQ / calling-you / worked-before / new).
- **Coordinated QSY (Roam)** — opt-in "move together" that hops channels with the station you're working, **announced in the clear** (legal anti-QRM + casual obscurity — *not* privacy). See [Privacy & Coordinated QSY](Privacy-and-Coordinated-QSY.md).
- **Work-a-station + ADIF logbook** — click a heard station or decode to start a directed QSO; contacts auto-log and drive **worked-before (B4)** highlighting.
- **WSJT-X-familiar controls** — RX level meter, Tx power, audio-device pick, Tune / Monitor / Stop-TX, alerts, UTC clock + bearing, editable macros, time-sync health, Tx watchdog.
- **Rig control + band/frequency selection** — Hamlib `rigctld` for CAT (bundled), serial RTS/DTR, or VOX; one-tap band selector + manual frequency entry.
- **Tempo's own calling frequencies** — off the FT8/FT4/JS8 watering holes, on a US-General-legal, CW-clear plan across HF and VHF/UHF. See [Frequency Plan](Frequency-Plan.md).
- **Starts passive (hunt-and-pounce)** — listens and only transmits when you act; the CQ beacon is **opt-in** (default off).
- **Ecosystem interop** — WSJT-X-compatible UDP API and PSK Reporter spotting.

---

## All wiki pages

**Getting started**
- [Getting Started](Getting-Started.md) — download, install, first launch, the minimum settings.

**Operating**
- [Operating Guide](Operating-Guide.md) — the layout, the modes, working a station, the decode feed, the logbook.
- [Tiers FT1 vs DX1](Tiers-FT1-vs-DX1.md) — when to pick Fast vs Robust, and the physics behind it.
- [Privacy & Coordinated QSY](Privacy-and-Coordinated-QSY.md) — "move together" off QRM and casual listeners — legal, in the clear, **not** private.

**Setup & reference**
- [Rig and Audio Setup](Rig-and-Audio-Setup.md) — CAT/PTT, audio devices, levels, time-sync, Tune/Monitor.
- [Frequency Plan](Frequency-Plan.md) — Tempo's calling frequencies, HF + VHF/UHF.
- [Architecture and Protocol](Architecture-and-Protocol.md) — an operator-friendly tour of how Tempo is built.

**Development**
- [Building from Source](Building-from-Source.md) — Windows native and Linux/WSL2 cross-compile.

**About**
- [FAQ](FAQ.md) — the questions everyone asks.
- [Troubleshooting](Troubleshooting.md) — when something isn't working.
- [Roadmap](Roadmap.md) — what's next (forward-looking).

---

*GPL-3.0 · Author: Seth McCallister (KD9TAW). This wiki is the operator manual; the repository `docs/` are the developer reference.*
