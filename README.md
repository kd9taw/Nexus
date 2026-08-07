<div align="center">

<img src="docs/img/nexus-banner.jpg" alt="Nexus — the all-mode amateur radio operations center" width="840" />

**One app from antenna to award. Eleven modes, real rig control, APRS, propagation, DX chasing, and a logbook that computes your awards offline.**

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](COPYING)
![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20Raspberry%20Pi-0078D6)
[![Release](https://img.shields.io/github/v/release/kd9taw/Nexus?label=release)](https://github.com/kd9taw/Nexus/releases/latest)
[![Downloads](https://img.shields.io/sourceforge/dt/nexus-ham-radio?label=downloads)](https://sourceforge.net/projects/nexus-ham-radio/files/)
![Status](https://img.shields.io/badge/status-1.0-brightgreen)

[![Download](https://img.shields.io/badge/⬇_Download-Windows_·_Linux_·_Pi-0078D6?style=for-the-badge&logo=windows)](https://github.com/kd9taw/Nexus/releases/latest)
[![Operator manual](https://img.shields.io/badge/📖_Operator_manual-docs-8957e5?style=for-the-badge)](docs/manual/)

<sub>Offline installer, bundles WebView2 **and Hamlib**, per-user install, no admin rights. ·
**[Operator manual](docs/manual/)** · **[Comprehensive overview](docs/OVERVIEW.md)**</sub>

</div>

---

**Nexus** is a free, GPLv3, all-mode amateur radio operations center for the desktop. It puts a
WSJT-X-parity **FT8/FT4** cockpit, seven more WSJT-X modes that **transmit as well as receive**,
**RTTY**, **SSTV**, **APRS** with a live symbol map and an iGate, a **CW** keying station with an
**AI decoder** (DeepCW model by e04), an **SSB phone** cockpit with a real RF panadapter, a live
**propagation map**, an evidence-backed **"work this now"** board, a **POTA/SOTA** hunter, a
club-ready **Field Day** mode, **satellites** with rotor tracking, and a **DXCC-first logbook** with
LoTW / QRZ / ClubLog / eQSL connectors into one window, across up to three radios, with one log.

It is built in **Rust** (DSP, sequencing, CAT, networking) behind a **Tauri** desktop shell, and it
treats **WSJT-X as the golden standard** for FT8/FT4 behavior. The auto-sequencer state machine,
decode cadence, split handling, Hound mode, and the UDP ecosystem protocol all match stock, so your
WSJT-X muscle memory works unchanged inside a UI built this decade.

## Why Nexus

- **🎛️ All-mode, one app.** Digital, CW, and phone cockpits share one rig, one CAT layer, one
  logbook, and one needs engine. Entering a cockpit sets the rig up for it automatically: a DATA
  submode for digital, CW for the keyer, the correct sideband by band for phone. No mode fumbling,
  no app switching, no five stitched programs.
- **📡 WSJT-X operational parity in a modern shell.** The FT8/FT4 core replicates the stock behavior
  the whole ecosystem depends on: the QSO sequencer state table, the early decode pass at 11.8 s,
  Split Operation (Rig / Fake It), Hound mode, directed CQ, the Tx1–Tx6 panel, the F-key shortcuts,
  and the full WSJT-X UDP protocol, so JTAlert, GridTracker, and loggers see Nexus as a WSJT-X. A
  **Classic ↔ Roster** toggle gives you the stock layout or a modern sortable call roster.
- **📻 Every mode transmits, each with its own rhythm.** FT8, FT4, **Q65, FST4, FST4W, MSK144, JT65
  and WSPR**, plus the two Tempo tiers. Every waveform was verified by generating a transmission in
  Nexus and having **stock WSJT-X decode it**, rather than by testing Nexus against its own decoder,
  since both halves come from the same vendored source and a shared misreading would pass unnoticed.
  Q65 was additionally matched sample-for-sample against WSJT-X's own generator at 0.9985
  correlation. Each mode keeps its own operating discipline: MSK144 waits twelve transmit periods
  before giving up on a contact where FT8 waits three, and WSPR and FST4W never touch the QSO
  sequencer at all.
- **🔬 A decoder measured against the reference.** The FT8/FT4 modem is built from WSJT-X's own DSP
  sources, tracking upstream **3.0.2**. On identical recorded audio, the decode floor measures
  **−21.3 dB against stock WSJT-X's −20.7**, with zero false decodes, and the suite includes
  false-alarm tests that feed the decoder pure noise and require it to produce nothing at all.
  **Rovers keep decoding here**: upstream 3.0.2 discards any decode containing `/R` outside contest
  mode, which is exactly the VHF traffic worth catching, and a test fails if anyone reintroduces
  that filter.
- **🗺️ APRS that shows you the network and tells you the truth about your receiver.** Stations draw
  as their **actual APRS symbol** rather than identical dots: cars, trucks, bicycles and people,
  weather stations, digipeaters and iGates, campsites, balloons, boats and aircraft, with vehicles
  under way pointing their heading and an operator's overlay character (the `I` on a full iGate, the
  `R` on a receive-only one, a digipeater's hop count) drawn on top. Colour says what a station *is*,
  and the ring around it says how it reached you: solid for your own RF, doubled when you heard it
  both ways, dashed and dimmed for internet-only. **Click any station** for a detail card with what
  the symbol means in words, per-source ages kept separate ("your receiver decoded this 4 min ago;
  the internet feed reported it 20 s ago" are two different facts and only one says anything about
  your antenna), grid, distance and bearing, course, speed and altitude, the digipeater path,
  whether it arrived direct or digipeated, decoded **weather-station readings** (temperature, wind,
  gusts, rainfall, humidity, pressure, with a sensor the station does not have left out rather than
  reported as zero), the raw packet, and one click to QRZ or aprs.fi.
- **🔍 An APRS screen that names the actual fault.** An empty APRS map used to mean half a dozen
  different things and looked identical for all of them. Now it says which one: the radio is on the
  wrong frequency (**"on 144.174 USB — APRS needs 144.390 FM"**, with a Tune button, judged against
  the channel *you* selected so 144.800 in Europe is correct and not a warning), the right frequency
  in the wrong mode, a **closed squelch** (alive, no level, not a fault) as distinct from **no input
  at all** (which really is a dead capture device), the input level in dBFS with the window it
  measures, or **which of your radios it is listening to**. Every claim is dated, so a count from six
  minutes ago can no longer assert something about the present. The packet decoder is measured
  against tone imbalance ("twist") and still decodes with the tones **24 dB apart**, far beyond the
  roughly 9 dB real signals show.
- **🌐 APRS-IS, in and out.** Connect to the internet side of APRS and plot what the wider network
  reports near you alongside what you actually hear, with a radius around your grid, watched
  callsigns that come through from anywhere, and read-only access that needs no passcode. It doubles
  as a diagnostic: internet stations arriving while your RF chip stays silent tells you the fault is
  in the radio chain rather than the app. You can also run a **receive-only iGate** and contribute
  what your own antenna hears to the global map. Nexus honours every rule the network asks of an
  iGate (never re-gates an internet packet, respects `NOGATE` and `RFONLY`, suppresses duplicates,
  caps its own upload rate) and **never gates the other way**, because that direction means a radio
  keying up unattended.
- **🎚️ Real rig control, not just CAT.** On a native-CI-V Icom (IC-7300 / 7610 / 9700 / 705 / 905)
  Nexus drives the radio directly and streams the rig's **own spectrum scope** into the cockpit as a
  real RF panadapter, alongside AGC speed, noise reduction and blanker, notch, filter width, mic
  gain, and SWR / ALC / power / compression meters on the rig's own calibration curves. FlexRadio
  streams the SmartSDR panadapter and DAX audio over the network (VITA-49), which keeps working
  under Remote Desktop where the DAX sound devices are invisible. Every other rig runs through
  bundled Hamlib and still gets click-to-tune and drag-a-passband on the audio scope.
- **🔧 Rig control that recovers by itself.** Nexus stops polling a radio that has stopped
  answering, which is right, but that state used to be permanent: any hiccup cost you rig control
  until you restarted the app. It now retries quietly, backing off to about once every thirty
  seconds, and picks the radio back up within a couple of seconds of it answering again. It also
  reads what your radio actually covers out of the rig's own capability table, so an HF-only radio
  is never commanded to 2 m in the first place, and a refused command is treated as a refusal rather
  than believed.
- **⏸️ A waterfall you can rewind.** Pause and roll the wheel back through the last few minutes of
  the band, with a time tape showing how far back you are, then snap to live. History keeps
  recording while paused. A 3-D stacked-spectrum view reads band activity at a glance. Both work on
  the FT waterfall and on the Phone/CW panadapter.
- **🧭 An honest "work this now" board.** Every Needed row shows its evidence, as in *"heard by K9LC
  (EN52, 26 km), 4 min ago"*, and admission is gated by corroboration rules: multiple receivers near
  **you**, Es-patch locality on VHF, reciprocal-path checks. A superstation on a mountain can't tell
  you a band is open when it isn't open *at your QTH*. One click QSYs the rig, opens the right
  cockpit, and prefills the log, with split offsets parsed from cluster comments. A packed
  **callsign→state index** built from the FCC license file resolves NEW STATE on CW, SSB and cluster
  spots that carry no grid at all.
- **⚡ Pounce.** Working a rare station is a race, and once the pileup builds you have lost it. Nexus
  scores every skimmer and cluster spot as it arrives and, when something you actually need turns
  up, plays a distinct tone whether or not Nexus is the focused window, raises a desktop
  notification, and shows a banner with the call, the entity and the frequency. One click works it.
  **Nexus never touches the radio on its own for this.** It tells you, and you decide. Off until you
  switch it on, because how often it fires depends entirely on how much you still have to chase.
- **🏆 A logbook that chases awards properly.** DXCC, DXCC Challenge, Honor Roll, WAS, WAZ, per-band
  VUCC and IOTA computed offline from your log, with **source-aware confirmations** (eQSL and QRZ
  never silently count toward LoTW-grade awards), two-pull incremental LoTW sync, hourly QRZ
  confirmation pulls as a delta, a per-QSO upload state machine that survives restarts, callbook
  autofill, and a local-only **Journey** achievement layer that makes the first 100 QSOs as
  motivating as the last 10 entities. Editing a contact that has already gone out re-sends it, so
  the online logbooks can't quietly disagree with yours.
- **🗺️ Propagation you can act on.** A live world map (3-D globe, azimuthal beam map, or flat) fused
  from PSK Reporter, RBN/DX-cluster, and NOAA space weather, with a solar-flare layer, aurora, and
  measured MUF. A native in-app port of **ITU-R P.533**, the VOACAP-class link-budget standard, with
  no external binary to install. An opening detector anchored to *your* QTH with reciprocity gates
  and propagation-mode classification (Es / F2 / aurora / tropo), tiered alerts that go loud only
  for the rare and brief, opening sectors drawn on the map, and a persistent openings log so you can
  ask "how many real 2 m openings this month, and did I catch them?"
- **🛰️ Satellites and DXpeditions.** Pass schedules, polar plots, Doppler, favorites, true 3-D
  orbits at real altitude with footprint rings, and **rotor auto-track through a pass**. A
  DXpedition month calendar where each operation is a single coloured bar across the days it is on
  the air, carrying its announced bands low-first, with the modelled best band and time for each,
  wake-me alarms, and one click through to the team's own page (or the callsign's QRZ page when
  there isn't one).
- **🏞️ POTA, SOTA and Field Day.** Live activator spots with **NEW PARK** and **BAND OPEN** badges
  and a one-click **HUNT**. ARRL FD and Winter FD event modes with correct date rules and scoring
  (per-mode points, dupes per band *per mode*, legal power tiers, live bonus checklist), a
  worked-sections board for all 83 sections, submittable Cabrillo 3.0, and every contact pushed in
  real time to the club's **N3FJP** master log over its official TCP API and broadcast as native
  **N1MM+** `<contactinfo>` datagrams. The event log is journalled, so a mid-event crash comes back
  with your log intact.
- **♿ Accessibility that is always on.** Screen-reader support for JAWS and NVDA with no
  "accessibility mode" to enable and nothing changed for sighted users: the QSO sequencer is
  announced as it advances, transmit/receive switches are announced assertively, and the band
  activity, Call Roster and Needed lists are keyboard-navigable (arrow to read a row, Enter to
  select, Shift+Enter to work, Alt+Enter to ignore). Optional spoken decodes, a TX/RX earcon, and a
  per-cycle decode tick are there for operating by ear.
- **🔌 Zero-config setup, with a real transmit lockout.** Plug in the radio and click **Detect my
  radio**: Nexus reads the USB descriptors, matches the rig model, pairs the audio CODEC, and links
  the one driver you need if it's missing. Digirig and RIGblaster interfaces are recognised by name.
  Hamlib ships inside the installer. A goal-driven first-run wizard shapes the app to you, and your
  declared license class becomes a real Part 97 transmit lockout, a software guard in **every** TX
  path. Launching Nexus opens the rig **read-only** and commands nothing until you act, so a radio
  parked on 40 m LSB for a net stays there.
- **💬 The Tempo protocols.** Original weak-signal waveforms: **TempoFast** (a coherent 4-CPM tier on
  a four-second conversational cycle with **incremental-redundancy HARQ**, where a failed decode is
  not wasted because retransmissions *combine* until the message lands) and **TempoDeep** (a
  non-coherent 8-FSK tier for fading paths). No other amateur text mode accumulates redundancy
  across retransmissions. Both have closed real links on the air, including a completed two-station
  QSO on 6 m. Sensitivity figures are still simulation numbers, and on-air decode reports are the
  single most useful contribution you can make.
- **🔁 It updates itself.** A new version downloads quietly in the background and then offers to
  install. Nothing installs behind your back and nothing happens on a schedule: the button waits for
  you, and stands down while you are transmitting, tuning, in a contact or running CQ, and tells you
  which. Every update is signed and verified before it is applied.
- **📻 Program and Memories.** Search the repeaters around any location and **star one straight from
  the results**: it saves into Memories as a proper FM channel with the machine's shift, offset and
  access tone, and lands on the quick-recall strip in the Phone, Operate and CW cockpits, where one
  click or **Ctrl+1** through **Ctrl+9** tunes it. Starred repeaters remember where the machine
  physically is, so Memories shows distance and bearing measured from your current grid each time,
  which means the numbers follow you when you operate portable. A per-repeater **Tune** names FM
  explicitly, so the frequency, shift, offset and tone all land together on the radio you mapped for
  FM rather than on whichever rig was last in use. Export a **CHIRP-ready CSV** (CHIRP flashes
  roughly a thousand radio models), and Nexus warns you when the directory has a whole band missing
  rather than letting a short list look finished. Memories also ships 11 curated packs (172
  channels) with opt-in reminders before a net starts.

## Who it's for

**The new ham.** Nexus compresses the painful first month into an afternoon: auto rig detection
instead of driver archaeology, a wizard that asks what you want to *do* rather than what you know,
a license-class lockout so you can't transmit out of segment, an auto-sequencer that completes
FT8 QSOs while you learn the flow by watching, a Needed board that says *who* to work and *why*,
and a Journey layer that celebrates your firsts. When something doesn't work, the app tries to name
the actual fault rather than leaving you to guess: which radio it is listening to, whether the dial
is even on the right frequency, whether that silence is a closed squelch or a dead sound card. You
can be making contacts the day your callsign hits the FCC database.

**The WSJT-X operator.** Same gestures, same flow: double-click semantics straight from
`processMessage`, Esc / F4 / F6 / Alt+1–6, Band Activity bottom-pinned in chronological order,
early decodes at 11.8 s, Fake-It split keeping TX audio in 1500–2000 Hz, Hound auto-move on the
Fox's report. Plus what stock never had: country and need annotations on every decode row, a
sortable roster view, in-app LoTW/QRZ sync, always-on decode (no accidental deaf Monitor-off), the
rig's real panadapter, and seven more modes without launching anything else.

**The DX chaser.** ATNO / new-band / new-mode / new-zone ranking across **all** bands
simultaneously rather than just the one you're tuned to, DXpedition tracking with workable-now
cards and modelled windows, cluster split comments ("UP 2") parsed and applied to the rig at click
time, Pounce on the skimmer feed, Honor Roll math, and confirmation diagnostics that explain
exactly why a QSO isn't credited yet, with a one-click fix where one exists.

**The rag chewer.** Awards and rate are not the point for everyone. Phone and CW get a real
panadapter with click-to-tune, DSP controls that mirror the front panel, and a one-line recall card
showing their name and your history with them while you operate. CW macros greet the other operator
by name and send your state (`{HISNAME}` / `{MYSTATE}` / `{HISSTATE}`). Your local repeaters and nets
are a hotkey away, with distance and bearing to each machine.

**The club.** Field Day mode reshapes the app for the event weekend and feeds club infrastructure
natively, with no JTAlert bridge and no end-of-day log merges. Plus a built-in CAT broker so other
shack software can share the radio *through* Nexus.

## Two or three radios, routed by band and mode

A band on its own is not a fine enough rule. If you run a 2 m/70 cm rig for weak-signal digital and
a different rig for FM and APRS, both cover 2 m, and something has to decide which one a 2 m FT8
spot goes to and which one an APRS tune goes to.

- **Route on band *and* mode.** **Settings ▸ Radio** carries a routing table under your radios: pick
  a set of bands, pick a mode class (weak-signal digital, FM & APRS, SSB phone, CW, RTTY), pick the
  radio. Rules are checked top to bottom, first match wins, and the arrows beside each rule reorder
  them. Anything unmatched falls back to each radio's band coverage and then to a default radio you
  nominate. A three-radio shack usually fits in two rules. There is a **"Where would this go?"**
  control that resolves a band and mode to a radio without touching a rig, and it asks the same code
  the radio does, so it cannot tell you one thing and then do another. Add no rules and routing
  stays band-only, exactly as before.
- **One window, up to three rigs.** Every radio stays **permanently connected**. The non-active rigs
  are monitored live (frequency and S-meter in the top-bar switcher) and switching is an instant
  **handoff** with no CAT teardown, so the dial never bounces. Each radio carries its own CAT, audio,
  rotator, keying port and band coverage, and the routing table is shared across windows, because
  which rig does 2 m FM is a fact about your station rather than about one window.
- **Two windows, one logbook.** For simultaneous decode on more than one rig, launch a second
  instance pointed at another radio. Both share **one logbook**, reconciled field by field, so a
  contact edited in one window is merged rather than clobbered in the other, and each window keeps
  its needs fresh as the shared log changes.
- **Peg-lock** pins the active radio and stops all automatic switching.

## Works with your shack

| Integration | What it does |
|---|---|
| **WSJT-X UDP protocol** | Full outbound Decode / Status / QsoLogged / Heartbeat + inbound HaltTx, Clear, Replay, Location, Highlight — JTAlert and GridTracker see a WSJT-X |
| **CAT broker** | Nexus serves a rigctld-compatible TCP port so WSJT-X, N1MM+, and loggers share the radio through it |
| **Companion mode** | Ride an upstream WSJT-X / JTDX / MSHV decode stream over UDP instead of owning the rig |
| **N1MM+** | Native `<contactinfo>` UDP broadcast. **Broadcast every QSO** sends each logged contact, event or not, from every mode. Point OpenHamClock or GridTracker at the address and contacts plot as you log them. Off until you switch it on |
| **N3FJP** | Real-time Field Day and everyday ACLog QSO push over the official TCP API, with a connection Test button |
| **APRS-IS** | Read-only network feed with a radius and watched callsigns, plus an optional receive-only iGate that contributes only what your antenna heard |
| **DXKeeper (DXLab)** | Each logged QSO pushed to DXKeeper's TCP Network Service |
| **Cloudlog / Wavelog** | Per-QSO push to your self-hosted instance |
| **LoTW** | TQSL upload + two-pull incremental confirmation sync |
| **QRZ.com** | Callbook autofill, logbook push, two-way sync, and optional hourly confirmation pulls |
| **ClubLog / eQSL / HRDLog** | Real-time push (official installers bundle the ClubLog API key; source builds supply their own free key) and InBox confirmation import |
| **DX cluster / RBN** | Telnet feed with locality-gated VHF admission |
| **PSK Reporter** | MQTT firehose in for propagation; standard UDP spot uploads out (you appear on the map) |
| **CHIRP** | Full CSV round-trip, so channels flow Nexus ⇄ CHIRP ⇄ roughly 1,000 radio models |

Credentials for every service live **only in the OS keychain**, never in config files, never in
logs, and never shown back to the UI beyond "configured."

## Quick start

1. **[Download](https://github.com/kd9taw/Nexus/releases/latest)** the file for your platform.
   Every release carries all five, built from the same tree:

   | File | Platform |
   |---|---|
   | `Nexus_1.0.0_x64-setup.exe` | **Windows 10/11 x64** — NSIS, per-user, no admin rights, bundles WebView2 **and** Hamlib so it works offline |
   | `Nexus_1.0.0_amd64.AppImage` | **Linux on a PC, portable** — one file, runs from anywhere, updates itself in place |
   | `Nexus_1.0.0_pc_amd64.deb` | **Debian / Ubuntu on a PC** — apt-managed, pulls `libhamlib-utils` in for CAT |
   | `Nexus_1.0.0_pi_arm64_bookworm.deb` | **Raspberry Pi OS bookworm**, 64-bit (Pi 3/4/5) |
   | `Nexus_1.0.0_pi_arm64_trixie.deb` | **Raspberry Pi OS trixie**, 64-bit (Pi 3/4/5) |

   The `.deb` names are new in 1.0.0. The PC and Pi packages used to be told apart only by `amd64`
   versus `arm64`, so picking the right one meant already knowing that `amd64` means "PC" here —
   the names say `pc` and `pi` now, and the Pi files name their OS base.

   **The two PC Linux files need Ubuntu 24.04 or newer** (Debian 13, Fedora 40+, Mint 22 — anything
   with glibc 2.39 or later; check with `ldd --version`). The AppImage is no exception: it carries
   the app's own libraries but not the system C library. On an older distro both install cleanly and
   then fail to start, so it is worth checking first. See
   [Install → What you need](docs/install.md#what-you-need).
2. Plug in the radio, open **Settings ▸ Rig & Audio**, click **Detect my radio**.
3. Answer the first-run wizard: callsign, grid, license class, and what you want to do.
4. Watch decodes arrive. Double-click a station, the sequencer runs the QSO, and the contact lands
   in the logbook, on PSK Reporter, and (if configured) on QRZ and LoTW.

New here? Start with **[Getting Started](docs/manual/Getting-Started.md)**.

> The installer is **unsigned** (cross-compiled on Linux), so SmartScreen may warn: *More info →
> Run anyway*. Verify the download against the `SHA-256` published on the
> [release page](https://github.com/kd9taw/Nexus/releases/latest).
>
> On a slower Raspberry Pi, **Settings ▸ Decode depth ▸ Fast** keeps FT8/FT4 decoding in real time.

## Status — the honest version

**1.0.0 closes the beta period**, and what that claims is narrow: the modes, the rig control, the
logbook and the awards engine have been run on the air through that period, on more than one
station and on rigs the author does not own. It does not claim nothing is left. Every line below
still says what a thing does *not* do, and so does every entry in the changelog.

- The **FT8/FT4 tier is the production core**: operational parity with stock WSJT-X, verified
  against a 207-row behavior matrix, over a thousand automated tests, wire formats pinned by test,
  and exercised on the air daily.
- **Field-verified end to end** on Yaesu (FTDX10, FT-991A) and on native-CI-V Icom, where the
  IC-9700's real panadapter streams live.
- **APRS is on the air**, receiving and mapping, with the internet feed and the receive-only iGate
  confirmed against a live APRS-IS connection. Transmit (beacons, messages, acks) stays behind an
  explicit TX arm; opening the screen only ever starts a receive-only decoder.
- **The FlexRadio native path** (SmartSDR panadapter, DAX RX and TX audio, native meters) is opt-in,
  off by default, and **not yet confirmed on hardware here**. Turn it back off if decodes stop.
- The **TempoFast/TempoDeep tiers work on the air** (first decode 2026-07-21, first two-station QSO
  2026-07-26), and their **sensitivity figures are still simulation numbers**. On-air
  decode-rate-vs-SNR is the open gate and the single most useful contribution you can make.
- **CW and Phone cockpits** are casual and ragchew grade by design: macros, voice keyer, panadapter,
  live decoder, WinKeyer support, full logging. No contest exchanges.
- **Windows, Linux and Raspberry Pi** builds ship together every release.
- Not implemented yet: **contest modes** (NA VHF, RTTY RU, WW Digi), the **Fox role** (running a
  DXpedition end), **transmit-side iGating** (deliberately never, since it means a radio keying up
  unattended), and programming DMR / D-STAR / Fusion repeaters (they are listed with badges so you
  know they're there). Shared RepeaterBook access for every user, with no token to set up, is
  pending RepeaterBook's approval; until then Program uses the open hearham.com directory by default
  and your own RepeaterBook token if you add one.

## Architecture

```
┌────────────────────────────────────────────────────────────────┐
│ Tauri v2 desktop shell (src-tauri) + web UI (ui/, React + TS)  │
│   cockpits: Operate · CW · Phone · RTTY · SSTV · APRS · Tempo  │
│   boards: Needed · Spots · POTA/SOTA · Logbook · Awards ·      │
│           Journey · Connect map · Satellites · Memories        │
├────────────────────────────────────────────────────────────────┤
│ Rust core (crates/)                                            │
│   tempo-app    live TX/RX engine · settings · DTOs             │
│   tempo-core   slot timing · 77-bit messages · QSO sequencer · │
│                logbook/ADIF · Field Day · RTTY · reconcile     │
│   tempo-audio  cpal audio · rigctld CAT · native CI-V · CW     │
│                keyer · voice keyer · decode scheduler · broker │
│   tempo-net    WSJT-X UDP · PSK Reporter · DX cluster ·        │
│                APRS-IS · LoTW/QRZ/ClubLog/eQSL · N3FJP ·       │
│                N1MM · DXKeeper                                 │
│   propagation  needs engine · opening detector · P.533 ·       │
│                space wx · awards · Journey                     │
│   per-mode     ft8 · ft4 · q65 · fst4 · msk144 · jt65 · wspr · │
│                modes · tempo-sstv · deepcw                     │
│   tempo-fast / tempo-fast-sys   safe wrapper + FFI over libtempo│
├────────────────────────────────────────────────────────────────┤
│ libtempo (Fortran → C ABI, FFTW3, no Qt)                       │
│   FT8/FT4/Q65/FST4/FST4W/MSK144/JT65/WSPR encode+decode        │
│   (vendored WSJT-X DSP) · TempoFast 4-CPM turbo + IR-HARQ ·    │
│   TempoDeep non-coherent 8-FSK + soft LDPC                     │
└────────────────────────────────────────────────────────────────┘
```

See **[ARCHITECTURE.md](ARCHITECTURE.md)** and **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)** for
the full design.

## Building from source

```bash
git clone https://github.com/kd9taw/Nexus.git nexus
cd nexus
cargo test --workspace                  # Rust core (NOTE: this excludes src-tauri)
cargo test --manifest-path src-tauri/Cargo.toml --lib --features radio
cd ui && npm install && npm test        # UI suites (vitest)
# Windows installer, cross-compiled from Linux/WSL2:
./scripts/build-windows-cross.sh
# Linux .deb + AppImage:
./scripts/build-linux.sh
```

The modem is Fortran + C behind a Rust FFI, so the **GNU toolchain** is required. See
**[Building from Source](docs/manual/Building-from-Source.md)** and [WINDOWS.md](WINDOWS.md).

## Documentation

- **[Comprehensive overview](docs/OVERVIEW.md)** — every surface, in depth
- **[Operator manual](docs/manual/)** — setup, per-mode operating guides, integrations, troubleshooting
- **[Tempo protocol specification](docs/Tempo-Protocol.md)** — the native waveforms, for implementers
- **[Frequency plan](docs/FREQUENCIES.md)** — where the TempoFast/TempoDeep tiers live on the bands
- **[Changelog](CHANGELOG.md)** — every release, in operator-facing prose

## License & credits

Nexus is **free software under the [GNU GPL v3](COPYING)** (GPL-3.0-only).

- **WSJT-X** — Joe Taylor **K1JT**, Steve Franke **K9AN**, Bill Somerville **G4WJS**, and the WSJT
  Development Group. Nexus's digital modem (`libtempo/`) is **derived from WSJT-X**: the FT8/FT4 codec,
  the 77-bit message packing, the LDPC(174,91) FEC, the CRC-14 check, and the Q65 / FST4 / FST4W /
  MSK144 / JT65 / WSPR mode sources are their GPL-licensed work, vendored and reused via a
  foreign-function interface (see **[NOTICE](NOTICE)** for the full lineage and marked
  modifications). Nexus interoperates with their ecosystem over the standard WSJT-X UDP protocol;
  its auto-sequencer is original Rust modeled on WSJT-X's on-air behavior.
  **Nexus is not endorsed by nor affiliated with the WSJT Development Group.** GPLv3.
- **[qracodes](https://github.com/IV3NWV/qracodes)** — **Nico Palermo IV3NWV** / Microtelecom Srl
  (© 2016, GPL-3.0-or-later). The Q-ary Repeat-Accumulate LDPC codec that **Q65** decodes with,
  vendored at `libtempo/vendor/wsjtx/lib/qra/` with its per-file license headers intact. A separate
  copyright from the WSJT Development Group's, credited separately for that reason. Nexus does not
  vendor or link the same author's SuperFox polar-code tables, which are not free software — see
  **[NOTICE](NOTICE)**.
- **TempoFast / TempoDeep** — the native weak-signal waveforms by **KD9TAW**.
- **[AetherSDR](https://github.com/aethersdr/AetherSDR)** (GPLv3) — the waterfall's 3D
  stacked-spectrum view (`ui/src/dss.ts`) and retained-history model
  (`ui/src/waterfallHistory.ts`) are ported from AetherSDR's `DssRenderer` /
  `WaterfallHistoryBuffer`; its `PanadapterStream` was a wire-format reference for the
  native Flex DAX/VITA path (see **[NOTICE](NOTICE)**).
- **[fldigi](http://www.w1hkj.com/)** (GPL-3.0-or-later) — **Dave Freese W1HKJ** and **Stefan Fendt
  DL1SMF** (descended from **Tomi Manninen OH2BNS**'s gmfsk). Nexus's RTTY demodulator
  (`crates/tempo-core/src/rtty/demod.rs`) is **ported from fldigi's receive path** (`rtty.cxx` +
  `fftfilt.cxx`), implementing **Kok Chen W7AY**'s published ATC design as fldigi does; lineage and
  the deliberate differences are in **[NOTICE](NOTICE)**.
- **[slowrx](https://github.com/windytan/slowrx)** (ISC) — **Oona Räisänen OH2EIQ**'s SSTV decoder,
  by way of the MIT **[`slowrx.rs`](https://github.com/jasonherald/slowrx.rs)** Rust port by
  **Jason Herald**. Nexus's SSTV *receiver* (`crates/tempo-sstv/`) is vendored from it; the SSTV
  transmitter is original Nexus code (see **[NOTICE](NOTICE)**).
- **[DeepCW](https://github.com/e04/deepcw-engine)** (AGPL-3.0) — **e04**. The neural-network model
  behind Nexus's primary CW decoder, shipped as an app resource (`resources/deepcw/`) with its full
  AGPL-3.0 license text bundled in the installer; its corresponding source is the upstream
  repository (see **[NOTICE](NOTICE)**).
- **[SatNOGS](https://satnogs.org)** / the **Libre Space Foundation** (CC BY-SA 4.0) — the open
  satellite database and network of volunteer ground stations. Nexus's amateur-satellite list is
  *derived from* the SatNOGS DB: which birds are in orbit, what they are transmitting, and whether
  they are still alive — the curation no orbital-element source can provide. That derived catalog is
  published by this project every six hours and downloaded by every install, and a snapshot of it
  ships inside the installer so a new install has the full bird list before its first fetch — under
  **CC BY-SA 4.0** with attribution carried inside the file. Their Doppler data serves a second purpose: Nexus's
  Doppler prediction is checked against carrier tracks recovered from eight real recorded passes on
  their network, so it is validated against signals that actually arrived at an antenna and not only
  against another implementation of the same theory. Thanks to stations 1696, 4803, 5049 and 5062
  and their operators (see **[NOTICE](NOTICE)** for what is redistributed where).
- **[Hamlib](https://hamlib.github.io/)** — bundled `rigctld` for CAT control (GPL/LGPL).
- **[FFTW](https://www.fftw.org/)** (GPL), **[Tauri](https://tauri.app/)**, React,
  [cpal](https://github.com/RustAudio/cpal),
  [alsa-rs](https://github.com/diwic/alsa-rs) (Linux device names),
  Natural Earth basemap (public domain),
  repeater data courtesy of [hearham.com](https://hearham.com) and
  [RepeaterBook.com](https://repeaterbook.com), city search powered by OpenStreetMap.

This is **experimental amateur-radio software**. You are responsible for operating within your
license privileges and local regulations. Nexus never transmits on launch; ARRL Field Day
prohibits fully-automated contacts, and Nexus's Field Day workflow is operator-initiated by design.

**Author:** **KD9TAW** · kd9taw@protonmail.com ·
contributions welcome — see **[CONTRIBUTING.md](CONTRIBUTING.md)** and the
[Code of Conduct](CODE_OF_CONDUCT.md).

<div align="center"><sub>

**[⬇ Download](https://github.com/kd9taw/Nexus/releases/latest)** ·
**[📖 Manual](docs/manual/)** ·
**[💬 Discussion group](https://groups.io/g/hamradiotools)** ·
**[🐛 Report a bug](https://github.com/kd9taw/Nexus/issues)** ·
**[🌐 hamradiotools.io](https://hamradiotools.io)**

</sub></div>
