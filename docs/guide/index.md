# Nexus User Guide

Nexus is a free, open-source ham radio workstation for Windows, macOS, Linux and
Raspberry Pi that puts the whole station — digital, phone, CW, satellites,
propagation, DX chasing, logging, and awards — into one modern app. This guide
is the per-section reference: pick the section you're working in and jump to its
page.

Nexus left beta at 1.0.0. The habit the beta was written with does not
close with it: where a feature is opt-in, or a number comes from simulation
rather than the air, these pages say so, and each page ends with what its
section does **not** do.

The supported window floor is **1024×768**. Everything is reachable there —
some columns scroll to reach the bottom of themselves, and the pages that
measure it say where the fold falls at which size.

<!-- TODO: capture screenshot — the Nexus main window with the left nav rail labelled -->

## How the app is laid out

The left rail is your section switcher. At the top is the **FT8/FT4 ⇄ Tempo**
mode switch — it swaps only the mode-specific operating cockpit (the digital
FT8/FT4 cockpit vs. the Tempo TempoFast/TempoDeep chat cockpit). Everything else — the map,
the Needed board, the logbook, awards, settings — is shared across both modes.

The **Now-Bar** runs across the top from every section: UTC clock, current band,
TX/RX state, and the "is the band open / am I getting out / what do I need"
answer, with feed-health pills that tell "connected but quiet" apart from "down."

Any panel can tear off into its own OS window (the ⧉ pop-out control) for a
multi-monitor shack.

## The sections

### Operating
- **[Operate — FT8/FT4 digital](operate-digital.md)** — the digital cockpit with
  WSJT-X-grade sequencing, country/worked-before flags on every decode, and
  one-click "work it."
- **[Phone (SSB)](phone.md)** — a traditional rig panel: live dial read-back,
  fast colored bandscope, voice keyer, QSO recording.
- **[CW](cw.md)** — a casual/ragchew keyboard CW station with F-key macros.
- **[RTTY](rtty.md)** — a 45.45 baud Baudot teleprinter: per-character decode
  confidence, a click-to-net waterfall, macros, and AFSK or true FSK keying.
- **[SSTV](sstv.md)** — receive-first slow-scan; pictures decode themselves into
  a local gallery, and transmit is always an explicit **Send**.
- **[APRS](aprs.md)** — a 2 m AFSK-1200 packet monitor with its own map, plus
  position beacons and short messages you send by hand.
- **[Memories](memories.md)** — the saved-channel bank behind the cockpit MEM
  strip: one click tunes the rig, applies the shift and tone, and opens the
  cockpit that mode belongs in.
- **[Tempo chat (TempoFast/TempoDeep)](operate-digital.md#the-tempo-chat-layer-tempofasttempodeep)** — the
  original weak-signal chat tiers, covered at the end of the Operate guide.

### DX & awards
- **[Needed — DX that's on the air now](needed-dx.md)** — every station on the
  air ranked by what it's worth to *your* log, each row carrying the evidence.
- **[Spots](spots.md)** — the same cluster and RBN traffic raw: the last twenty
  minutes unranked and unscored, sorted however you like.
- **[DXpeditions](dxpeditions.md)** — active and upcoming expeditions, your
  modelled best window per day, and a wake-me alarm.
- **[Logbook & QSL](logbook-qsl.md)** — the ADIF logbook, confirmation sources,
  and the online-service connectors (LoTW/QRZ/ClubLog/eQSL/HRDLog).
- **[Awards & Journey](awards-journey.md)** — offline DXCC/Challenge/Honor
  Roll/WAS/WAZ, plus the local-only Journey achievement layer.
- **[Stats](stats.md)** — the same logbook counted rather than judged: QSOs by
  band, mode, year, hour, entity and confirmation.

### Propagation & satellites
- **[Connect — map + propagation](connect.md)** — the shaded 3-D globe, greyline,
  live spots, aurora, MUF, moving satellites, the opening detector, and the
  assignable pane grid.
- **[Satellites](satellites.md)** — pass predictions for your grid, favorites,
  polar plots, frequencies, and rotor auto-track.

### Contesting & portable
- **[Field Day & POTA/SOTA](contesting-pota.md)** — ARRL/Winter Field Day mode
  with Cabrillo and club interop, plus the POTA/SOTA hunter.

### System
- **[Program](program.md)** — the radio-programming workbench: the repeaters
  around a location become a channel list, and the list becomes a CHIRP CSV.
- **[Settings reference](settings-reference.md)** — a walk through all nine
  Settings tabs, field by field.

## First run

On first launch a four-step wizard — Station, Rig, Log, Finish — gets you on the
air. Every step is skippable and everything it sets stays editable later in
Settings. Every section and mode starts ON; you can turn any section
on or off in [Settings ▸ Appearance ▸ Features](settings-reference.md#features).
If you'd rather set things by hand, the
[Settings reference](settings-reference.md) covers every field.

## Keyboard shortcuts

Shortcuts are section-specific and only fire in the active view (never while
you're typing in a field):

| Section | Keys |
|---|---|
| Operate | `Esc` halt TX · `F4` clear DX call · `F6` re-decode last period · `Alt+1`–`Alt+6` fire a Tx slot |
| CW | `F1`–`F8` fire macros · `Esc` abort keying · `PgUp`/`PgDn` nudge WPM (±2, Shift ±4) |
| Phone | `Space` push-to-talk (hold) |
