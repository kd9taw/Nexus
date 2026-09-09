# Nexus User Guide

<!-- CHAPTER ORDER LIVES HERE: the PDF and EPUB builders take the book's order from the -->
<!-- FIRST bare link to a chapter page in this file, so a chapter link added above "The -->
<!-- sections" silently moves that chapter to the front of the book. A cross-reference -->
<!-- higher up carries an anchor (cw.md#macros), which does not claim the slot. -->
<!-- House rules for editing the guide: docs/manual-style-guide.md -->

Nexus is a free, open-source ham radio workstation for Windows, macOS, Linux and
Raspberry Pi that puts the whole station — digital, phone, CW, satellites,
propagation, DX chasing, logging, and awards — into one app. This guide is the
per-section reference: pick the section you are working in and jump to its page.

Nexus left beta at 1.0.0. The habit the beta was written with does not close with
it: where a feature is opt-in, or a number comes from simulation rather than the
air, these pages say so, and each page ends with what its section does **not** do.

**This guide documents Nexus 1.11.1.** These pages ship inside the app, and the same files are
synced to the website and built into the PDF and EPUB at each release — so a copy matches the
release it came out with. It cannot promise to match the build *you* are running. The website
and the downloads track the latest release; your installed copy may be older, or newer than a
PDF you saved months ago. **Check it:** the Settings panel's header carries your build stamp —
version and build date — beside a **Check for updates** button. If that version is not the one
named above, read the difference as a difference rather than a mistake, and prefer the app's
own tooltips and hint text, which are generated from the shipped build.

The supported window floor is **1024×768**. Everything is reachable there —
some columns scroll to reach the bottom of themselves, and the pages that
measure it say where the fold falls at which size.

## Start here

If Nexus is not installed yet, [install it](../install.md) first, then run the
[quick start](../quick-start.md).

What the app needs from you before it can do anything:

- **Your callsign and grid.** Nothing else is required. Set them in
  [Settings ▸ Station ▸ Operator & Radio](settings-reference.md#operator--radio).
- **A radio, if you want CAT and transmit.** Receive-only operation works with
  audio alone. Rig setup is
  [Settings ▸ Radio ▸ Rig & CAT](settings-reference.md#rig--cat).
- **A sound card the radio is connected to**, for any mode that decodes or
  transmits audio — [Settings ▸ Radio ▸ Audio](settings-reference.md#audio).

On first launch a four-step wizard — Station, Rig, Log, Finish — walks those.
Every step is skippable and everything it sets stays editable later in Settings.
Every section and mode starts ON; turn any section on or off in
[Settings ▸ Appearance ▸ Features](settings-reference.md#features).

The supported window floor is **1024×768**. Everything is reachable there — some
columns scroll to reach the bottom of themselves, and the pages that measure it
say where the fold falls at which size.

## How to read this guide

Every section chapter is built the same way, so you can skip to the part you
need:

| Heading | What is in it |
|---|---|
| **The tour** | What is on the screen and what each control does. |
| **Core workflows** | Numbered procedures. One action per step, and what you should see after it. |
| **Honest limits** | What the section does not do, and what it will not tell you. |
| **Related guides** | Where the neighboring job is written up. |

**Illustrative settings are not recommendations.** Screenshots are of one real
station. The numbers in them — power, WPM, band, cluster host — are that
operator's, not advice.

### How this guide marks evidence

Claims about what the app does are not all the same strength, and the guide keeps
them apart:

- **Plain statement** — read off the source or observed in the shipped build. If
  a page says a control does something, that is the claim.
- **"reported"** — behavior we have from operators or a bug report but have not
  reproduced here. The page names who reported it and on what.
- **"not verified on hardware"** — the code path is written and tested, but no
  one has run it against that rig, rotator or amplifier. Common on serial CAT
  work, where the vendor's own document and the driver library disagree.

The app marks evidence too, and means something narrower by it: on the
[Needed board](needed-dx.md#reading-an-evidence-line) an *evidence line* is the receipt for one row —
where the report came from and how old it is. It has four forms:

| Evidence line | Where it comes from | Age shown |
|---|---|---|
| `decoded by YOUR radio on this band` | Your own receiver, this band, inside the freshness window. | None — the row exists only while the decode is fresh. |
| `heard by <call> (<grid>, <km>) + …` | PSK Reporter reception reports from receivers near you, nearest first, three named then `+N more`. | Time of the most recent report. |
| `spotted by <call> + … via cluster/RBN` | DX cluster and RBN skimmer spots. | The spot's own receive time off the wire. |
| `your signal reaches their area (via <call>)` | An inference: a third party hears the DX, and your signal has been copied in that area on that band. Nobody near you has heard this station. | Time of the reception report the inference rests on. |

Ages read `just now` under 90 seconds, then `N min ago`. Spots age out at 15
minutes.

## How the app is laid out

The left rail is your section switcher. At the top is the **FT8/FT4 ⇄ Tempo**
mode switch — it swaps only the mode-specific operating cockpit (the digital
FT8/FT4 cockpit vs. the Tempo TempoFast/TempoDeep chat cockpit). Everything else
— the map, the Needed board, the logbook, awards, settings — is shared across
both modes.

The **Now-Bar** runs across the top from every section: UTC clock, current band,
TX/RX state, and the "is the band open / am I getting out / what do I need"
answer, with feed-health pills that tell "connected but quiet" apart from "down."

Thirteen panels tear off into their own OS window — see
[Windows you can tear off](#windows-you-can-tear-off) below.

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
- **[Chat (Tempo)](chat.md)** — Nexus's own weak-signal text mode: a roster, threaded
  conversations, and messages that queue until the other station is actually heard.
- **[PSK](psk.md)** — PSK31 and QPSK31 keyboard ragchewing at 31.25 baud: click a
  warble on the waterfall and it prints, type into a live transmission, four macros.
- **[SSTV](sstv.md)** — receive-first slow-scan; pictures decode themselves into
  a local gallery, and transmit is always an explicit **Send**.
- **[APRS](aprs.md)** — a 2 m AFSK-1200 packet monitor with its own map, plus
  position beacons and short messages you send by hand.
- **[JS8](js8.md)** — JS8Call-compatible keyboard chat on FT8's waveform: heartbeats,
  directed messages, relay and a store-and-forward inbox, all four speeds decoded at once.
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
- **[Settings reference](settings-reference.md)** — a walk through every Settings
  tab, field by field.

### When something is wrong
- **[Scenarios & troubleshooting](scenarios.md)** — jobs that cross several
  chapters: stopping a transmission, getting a dead receive path decoding again,
  moving over from WSJT-X, running two radios, a club event, and operating with
  the feeds down.

## Find it fast

Where the common jobs live. These links land on the exact section, not the top
of a page.

| I want to… | Go to |
|---|---|
| Set my callsign, grid or license class | [Settings ▸ Station ▸ Operator & Radio](settings-reference.md#operator--radio) |
| Pick a COM port, rig model or baud rate | [Settings ▸ Radio ▸ Rig & CAT](settings-reference.md#rig--cat) |
| Pick the sound card the radio is on | [Settings ▸ Radio ▸ Audio](settings-reference.md#audio) |
| Hear the radio through the computer's speakers | [Settings ▸ Radio ▸ Receive audio on this computer](settings-reference.md#receive-audio-on-this-computer) |
| Check why the rig is not connecting | The [Setup health strip](settings-reference.md#setup-health) in Settings ▸ Radio |
| Add a second radio, or route bands between radios | [Settings ▸ Radio ▸ Radios](settings-reference.md#radios) |
| Cap transmit power or share the rig with another program | [Settings ▸ Radio ▸ Transmit limits & sharing](settings-reference.md#transmit-limits--sharing) |
| Set up a rotator | [Settings ▸ Radio ▸ Rotator](settings-reference.md#rotator) |
| Set up an amplifier | [Settings ▸ Radio ▸ Amplifier](settings-reference.md#amplifier) |
| Turn auto-sequencing, TX enable or decode depth up or down | [Settings ▸ Digital ▸ Digital (FT8/FT4)](settings-reference.md#digital-ft8ft4) |
| Change the dial frequency a mode starts on | [Settings ▸ Digital ▸ Working Frequencies](settings-reference.md#working-frequencies) |
| Set keyer speed, sidetone or the keying backend | [Settings ▸ CW](settings-reference.md#cw) |
| Set repeater shift, CTCSS tone or mic gain | [Settings ▸ Phone ▸ Phone (SSB / FM)](settings-reference.md#phone-ssb--fm) |
| Add a DX cluster, RBN or PSK Reporter feed | [Settings ▸ Logging & Connectors ▸ Integrations & Feeds](settings-reference.md#integrations--feeds) |
| Feed WSJT-X, GridTracker or JTAlert over UDP | [Settings ▸ Logging & Connectors ▸ Integrations & Feeds](settings-reference.md#integrations--feeds) |
| Turn on LoTW, QRZ, ClubLog, eQSL or HRDLog upload | [Settings ▸ Logging & Connectors ▸ Confirmations](settings-reference.md#confirmations) |
| See whether an upload actually went | [Settings ▸ Logging & Connectors ▸ Connections](settings-reference.md#connections) |
| Change how "worked before" is decided | [Settings ▸ Logging & Connectors ▸ Worked-before (B4) & dupes](settings-reference.md#worked-before-b4--dupes) |
| Get an alert when a new one appears | [Settings ▸ Spots & Alerts ▸ Pounce](settings-reference.md#pounce--new-one-alert) |
| Set the Field Day class, section and exchange | [Settings ▸ Contesting ▸ Field Day Setup](settings-reference.md#field-day-setup) |
| Change theme, UI scale or density | [Settings ▸ Appearance ▸ Workspace](settings-reference.md#workspace) |
| Turn a whole section off | [Settings ▸ Appearance ▸ Features](settings-reference.md#features) |
| Turn on screen-reader announcements and earcons | [Settings ▸ Appearance ▸ Accessibility & eyes-free](settings-reference.md#accessibility--eyes-free) |
| Back up, restore or reset everything | [Settings ▸ Config ▸ Backup & reset](settings-reference.md#backup--reset) |
| Stop a transmission that is already going out | [Scenarios ▸ Stop a transmission](scenarios.md#stop-a-transmission) |
| Work out why nothing is decoding | [Scenarios ▸ Nothing is decoding](scenarios.md#nothing-is-decoding) |
| Work out why the rig will not key | [Scenarios ▸ The rig will not key](scenarios.md#the-rig-will-not-key) |

Settings also has its own **Find a setting** box at the top of the panel, which
searches the same vocabulary these rows are named for.

## Keyboard shortcuts

Shortcuts fall into three scopes:

- **Global** — anywhere in the main window. Suppressed while the cursor is in a
  text field, and absent from torn-off windows, which run their own panel and
  not the app shell.
- **Section** — only while that section is the one on screen. RTTY, PSK, JS8 and
  SSTV keep decoding in the background while you are elsewhere, but their keys
  still belong to the section you are looking at: `Esc` in the Needed board does
  not stop an RTTY over.
- **List** — only while that list has keyboard focus.

A few keys deliberately ignore the typing rule, because the moment you reach for
them is the moment your hands are in a field. Those are called out below.

| Keys | Scope | What it does |
|---|---|---|
| `Ctrl+1`–`Ctrl+9` (`⌘+1`–`⌘+9` on macOS) | Global | Recall favorite memory 1–9: tunes the rig and opens that mode's cockpit. |
| `Esc` | Operate — **fires while typing too** | Halt TX. |
| `F4` | Operate — **fires while typing too** | Clear the DX call. |
| `F6` | Operate | Re-decode the last period, adding only what the first pass missed. |
| `Alt+1`–`Alt+6` | Operate | Send that Tx slot. |
| `F1`–`F8` | CW — **fire while typing too** | Fire that macro. |
| `Esc` | CW — **fires while typing too** | Abort keying: clears the queue and stops the rig. |
| `PgUp` / `PgDn` | CW — **fire while typing too** | Nudge WPM ±2, with Shift ±4. |
| `Space` (hold) | Phone | Push-to-talk. The press is ignored in a text field; the release always unkeys, wherever focus has moved. |
| `F1`–`F6` | Phone, while the Voice keyer pane is on screen | Play that recorded message. Ignored while a recording is in progress. |
| `Esc` | Phone, while the Voice keyer pane is on screen | Stop playback. |
| `Esc` | RTTY / PSK / JS8 | Stop TX: drops the queue, drops the continuous-TX latch and unkeys. |
| `↑` `↓` `Home` `End` | List | Move between rows. The list is one Tab stop. |
| `Enter` / `Space` | List | Work the focused row. |
| `Esc` | Any open dialog, menu or station card | Close it. |

**Mac keyboards eat bare F-keys as media keys.** Hold `fn`, or turn on "Use F1,
F2, etc. as standard function keys" in macOS keyboard settings.

**Macro buttons labelled `F1`–`F4` in RTTY and PSK are click-only.** The caption
names the slot; the key is not bound in those two cockpits. CW's `F1`–`F8` and
Phone's voice-keyer `F1`–`F6` are bound.

## Windows you can tear off

Thirteen panels open in their own OS window for a multi-monitor shack. The
control sits on the panel's own header — **⧉ Pop out** in most places, **↗ Pop
out** on Memories, **⧉ Pop out board** on the club band board, and a bare **⧉**
on Operate and Satellites. Where a panel has no such control, it does not
detach, and **most sections do not**: there is no pop-out for the logbook,
awards, stats, Spots, Program, Settings, chat, or the Phone, CW, RTTY, PSK,
SSTV, APRS and JS8 cockpits.

| Panel | Control | Notes |
|---|---|---|
| Operate cockpit | ⧉ in the cockpit header | The full cockpit, keyboard shortcuts included. |
| Waterfall | ⧉ on the waterfall pane head, in Operate | The only pop-out that vacates its slot in the main window; closing it re-docks the pane. There is also a manual re-dock control if it does not. |
| Map | **Map** in the Operate header | Opens with the Parks layer on, independent of the Connect map's layer picks. |
| Connect | ⧉ Pop out, Connect header | The globe with its own pane grid. |
| Needed board | ⧉ Pop out, Needed header | A header checkbox, **open at launch**, force-opens it every start. |
| DXpeditions | ⧉ Pop out, DXpeditions header | |
| Satellites | ⧉ in the Satellites header | |
| Memories | ↗ Pop out, Memories toolbar | |
| POTA / SOTA | POTA/SOTA header | |
| Field Day scoreboard | Field Day view | |
| Club band board | ⧉ Pop out board — Field Day view, and the Field Day header | Opens wider and shorter, set in glance type: it is read across a tent. |
| Band map (Phone) | ⧉ on the band-map pane head, in Phone | Docks to a screen edge as a full-height strip; size, position and dock survive a restart. |
| Band map (CW) | ⧉ on the band-map pane head, in CW | Same. |

The POTA/SOTA, Field Day and club-board buttons hide themselves inside the
window they already opened, so you cannot stack a second copy.

How they behave:

- **One window per panel.** Clicking ⧉ again focuses the window that is already
  open rather than making a second one.
- **The engine is shared.** A pop-out is another client of the same radio
  engine, so the rig, the log, TX state and the selected station stay consistent
  across every window. Working a station in a torn-off Needed board moves the
  same radio.
- **View preferences are per window.** Zoom, map projection, filters and pane
  choices start from whatever the main window is using, then go their own way
  the first time you change them in the pop-out.
- **They are ordinary windows.** Nothing is pinned always-on-top; you can send a
  pop-out behind the main window.
- **They do not survive a restart.** Only the main window is restored. Band-map
  geometry is remembered, but the window itself is not reopened.
- **The memory hotkeys do not work in a pop-out.** `Ctrl+1`–`Ctrl+9` belong to
  the app shell, which a torn-off window does not run.

Nexus 1.10.3.
