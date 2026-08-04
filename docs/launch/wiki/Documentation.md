# Documentation

This wiki gets you installed and on the air. The **full manual set** — the deep,
per-section reference — lives with the source on GitHub, where it's the canonical,
always-current version, updated alongside the code it documents.

**Full docs on GitHub:**
<https://github.com/kd9taw/Nexus/tree/main/docs>

> **1.0.0 closes the beta period, and the habit it was written with stays.** Where
> a feature is opt-in, or a number comes from the bench rather than the air, these
> pages say so, and each section guide ends with what that section does **not** do.
> The FT8/FT4 core is the production tier and is built to WSJT-X's behaviour.
> TempoFast and TempoDeep have closed real links on the air, and their sensitivity
> figures are still bench numbers.

New here? Start with [Quick Start](Quick-Start), [Install](Install), and
[Rig Setup](Rig-Setup) on this wiki, then come back for the depth below.

---

## Section guides

The per-section reference — pick the section you're working in. All eighteen pages
are indexed at
[docs/guide](https://github.com/kd9taw/Nexus/blob/main/docs/guide/index.md).

### Operating

| Guide | What it covers |
|---|---|
| [Operate — FT8 / FT4 digital](https://github.com/kd9taw/Nexus/blob/main/docs/guide/operate-digital.md) | The digital cockpit: WSJT-X-grade sequencing, country / worked-before flags, one-click "work it", and the Tempo TempoFast/TempoDeep chat layer |
| [Phone (SSB / FM)](https://github.com/kd9taw/Nexus/blob/main/docs/guide/phone.md) | The traditional rig panel: live dial read-back, colored bandscope, voice keyer, QSO recording |
| [CW](https://github.com/kd9taw/Nexus/blob/main/docs/guide/cw.md) | The keyboard CW station: keyer back-ends, F-key macros, live decoder |
| [RTTY](https://github.com/kd9taw/Nexus/blob/main/docs/guide/rtty.md) | A 45.45 baud Baudot teleprinter: per-character decode confidence, a click-to-net waterfall, macros, and AFSK or true FSK keying |
| [SSTV](https://github.com/kd9taw/Nexus/blob/main/docs/guide/sstv.md) | Receive-first slow-scan — pictures decode themselves into a local gallery, and transmit is always an explicit **Send** |
| [APRS](https://github.com/kd9taw/Nexus/blob/main/docs/guide/aprs.md) | A 2 m AFSK-1200 packet monitor with its own map, plus position beacons, short messages, and the receive-only iGate |
| [Memories](https://github.com/kd9taw/Nexus/blob/main/docs/guide/memories.md) | The saved-channel bank behind the cockpit MEM strip: one click tunes the rig, applies the shift and tone, and opens the cockpit that mode belongs in |

### DX & awards

| Guide | What it covers |
|---|---|
| [Needed — DX on the air now](https://github.com/kd9taw/Nexus/blob/main/docs/guide/needed-dx.md) | Every station ranked by value to *your* log, each row carrying the evidence |
| [Spots](https://github.com/kd9taw/Nexus/blob/main/docs/guide/spots.md) | The same cluster and RBN traffic raw: the last twenty minutes, unranked and unscored, sorted however you like |
| [DXpeditions](https://github.com/kd9taw/Nexus/blob/main/docs/guide/dxpeditions.md) | Active and upcoming expeditions, your modelled best window per day, wake-me alarms |
| [Logbook & QSL](https://github.com/kd9taw/Nexus/blob/main/docs/guide/logbook-qsl.md) | The ADIF logbook, confirmation sources, and the LoTW / QRZ / ClubLog / eQSL / HRDLog connectors |
| [Awards & Journey](https://github.com/kd9taw/Nexus/blob/main/docs/guide/awards-journey.md) | Offline DXCC / Challenge / Honor Roll / WAS / WAZ, plus the local-only Journey layer |
| [Stats](https://github.com/kd9taw/Nexus/blob/main/docs/guide/stats.md) | The same logbook counted rather than judged: QSOs by band, mode, year, hour, entity and confirmation |

### Propagation & satellites

| Guide | What it covers |
|---|---|
| [Connect — map + propagation](https://github.com/kd9taw/Nexus/blob/main/docs/guide/connect.md) | The shaded 3-D globe, greyline, live spots, aurora, MUF, moving satellites, the opening detector, and the assignable pane grid |
| [Satellites](https://github.com/kd9taw/Nexus/blob/main/docs/guide/satellites.md) | Pass predictions for your grid, favorites, polar plots, frequencies, and rotor auto-track |

### Contesting & portable

| Guide | What it covers |
|---|---|
| [Contesting, POTA & SOTA](https://github.com/kd9taw/Nexus/blob/main/docs/guide/contesting-pota.md) | ARRL / Winter Field Day with Cabrillo and club interop, plus the POTA / SOTA hunter |

### System

| Guide | What it covers |
|---|---|
| [Program](https://github.com/kd9taw/Nexus/blob/main/docs/guide/program.md) | The radio-programming workbench: the repeaters around a location become a channel list, and the list becomes a CHIRP CSV |
| [Settings reference](https://github.com/kd9taw/Nexus/blob/main/docs/guide/settings-reference.md) | A walk through all eight Settings tabs, field by field |

---

## Protocols

The weak-signal protocol references. Every performance figure in them is a **bench
number**. Both tiers have closed real links on the air, including a completed
two-station QSO on 6 m, and a proper characterisation of decode rate against signal
level on real paths is the open gate.

- [Protocol overview](https://github.com/kd9taw/Nexus/blob/main/docs/protocols/index.md)
  — how TempoFast and TempoDeep relate to FT8/FT4 and to each other.
- [TempoFast](https://github.com/kd9taw/Nexus/blob/main/docs/protocols/tempofast.md)
  — the 4-second chat-speed tier with IR-HARQ retransmission combining (trades
  ~6 dB of single-shot sensitivity vs FT8 for a nearly 4× faster cycle).
- [TempoDeep](https://github.com/kd9taw/Nexus/blob/main/docs/protocols/tempodeep.md)
  — the robust non-coherent 8-FSK tier built to shrug off fading.

---

## Reference & interop

- [Interop & companion setup](https://github.com/kd9taw/Nexus/blob/main/docs/interop.md)
  — the WSJT-X UDP protocol, GridTracker / JTAlert / loggers, the CAT broker, and
  cluster / RBN feeds.
- [Troubleshooting](https://github.com/kd9taw/Nexus/blob/main/docs/troubleshooting.md)
  — CAT connect failures, driver installs, port conflicts, and audio device
  selection.

---

## This wiki

- [Home](Home) — what Nexus is, at a glance.
- [Quick Start](Quick-Start) — from install to your first FT8 contact.
- [Install](Install) — download, SmartScreen, SHA-256, where data lives.
- [Rig Setup](Rig-Setup) — Yaesu, Icom, FlexRadio, Xiegu, rotators.
- [FAQ](FAQ) — the common questions.

Found something out of date, or have an on-air result to share? Open a ticket at
<https://github.com/kd9taw/Nexus/issues/new/choose>.

---

*Nexus is GPL-3.0-only. Built by KD9TAW.*
