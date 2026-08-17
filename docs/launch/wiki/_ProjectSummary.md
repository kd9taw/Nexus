# Nexus: the whole ham radio station in one modern app

**Nexus is a free, open-source amateur radio operations center.** It brings the
whole station into one modern app: digital, phone, CW, APRS, satellites,
propagation, logging and awards. A brand-new Technician can set it up in an
afternoon and a seasoned DXer will not outgrow it. Windows, Linux and Raspberry
Pi. GPL-3.0, built in Rust.

**[⬇ Download the latest release](https://github.com/kd9taw/Nexus/releases/latest)**
&nbsp;·&nbsp; [Documentation](https://sourceforge.net/p/nexus-ham-radio/wiki/Documentation/)
&nbsp;·&nbsp; [Source](https://github.com/kd9taw/Nexus)
&nbsp;·&nbsp; [hamradiotools.io](https://hamradiotools.io)

> **Nexus left beta at 1.0.0, and here is what that claims.** The FT8/FT4 core
> is the production tier: over a thousand automated tests, wire formats pinned by
> test, field-verified end to end on Yaesu FTDX10 and FT-991A and on native-CI-V
> Icom. APRS is on the air with its internet feed and receive-only iGate confirmed
> against a live connection. It does **not** claim nothing is left: the FlexRadio
> native panadapter is opt-in and not yet confirmed on hardware here, and the
> TempoFast and TempoDeep sensitivity figures are still bench numbers. The Windows
> installer is unsigned, so verify the published SHA-256.

---

## What it does about four real problems

**Every failure looks like silence.** A dead band, a deaf receiver, the wrong
sound card, a two-second clock error, a radio on the wrong frequency: they all
present identically as nothing happening. Nexus names the fault instead. The APRS
readout says "on 144.174 USB, APRS needs 144.390 FM" and puts a Tune button beside
it, judged against the channel you selected so 144.800 in Europe is correct rather
than a warning. A closed squelch is reported separately from a dead sound card,
because only one of them is a problem. On a multi-radio station it tells you which
rig it is listening to. Every claim carries its age.

**Nobody can tell whether a band is open for them.** Cluster and skimmer feeds are
global, and a superstation on a mountain hearing a rare one says little about what
your antenna reaches. Nexus scores openings on receivers near you, and every
Needed row shows who nearby actually heard the station and when. On 2 m and 4 m a
single genuine path past 700 km opens a band; 6 m asks for two, because one station
at that distance is meteor scatter. Predictions come from a native in-app port
of ITU-R P.533, the VOACAP-class standard, and are labelled modelled. What it does
not yet do is map your own antenna pattern, so it can tell you a path is open and
still not know your null is pointing at it.

**Delivery and credit are dishonest end to end.** An eQSL or QRZ match shows a
contact as confirmed and never counts toward DXCC or WAS, because counting it
would inflate them. Awards are computed offline from your own log with no account
required. A record LoTW rejected is reported as rejected rather than stamped sent.
Editing a logged contact re-sends it, so the online logbooks cannot quietly
disagree with yours.

**The interface evicts the operators who know the most.** Screen readers work with
nothing to switch on: JAWS and NVDA hear the QSO sequencer advance, and transmit
and receive switches are announced assertively. Band activity, the Call Roster and
the Needed list are keyboard-navigable, each row read aloud. Optional spoken
decodes and earcons let you operate by ear. Nothing changes for sighted operators.

---

## Features

- **On the air in minutes.** A three-step wizard finds your radio over USB or the
  network, fills in CAT and audio, and pairs the sound device. Around fifty rigs
  curated, Hamlib bundled, Digirig and RIGblaster recognised by name. Your licence
  class becomes a hard transmit lockout in every keying path. Opening the app
  never touches your rig.
- **Fifteen modes, all of them transmitting.** FT8 and FT4 plus **Q65, FST4,
  FST4W, MSK144, JT65 and WSPR**, each keeping its own operating rhythm. Add RTTY,
  SSTV, APRS, CW, SSB and FM, and the two Tempo tiers. Every waveform was verified
  by having *stock WSJT-X* decode a transmission Nexus generated, rather than by
  testing Nexus against its own decoder.
- **A decoder you can check.** Built from WSJT-X's own DSP sources, tracking
  upstream 3.0.2. On identical recorded audio the decode floor measures **−21.3 dB
  against stock WSJT-X's −20.7**, with no false decodes, and it is tested against
  pure noise so silence stays silent. Rovers keep decoding here, where upstream
  3.0.2 discards any call carrying `/R` outside contest mode.
- **Real rig control.** On a native-CI-V Icom or a network FlexRadio, the rig's
  **own RF panadapter** streams into the cockpit with AGC, noise reduction, filter
  width and mic gain mirroring the front panel. The waterfall pauses and rewinds
  through the last few minutes of the band, with a 3-D stacked-spectrum view. Rig
  control recovers on its own, where a wedged link used to need a restart.
- **APRS with the whole network.** Stations draw as their real APRS symbol,
  coloured by what they are, with a ring saying whether your own antenna heard
  them or the internet did. Click one for its meaning in words, distance and
  bearing, digipeater path, and decoded weather readings. Connect to APRS-IS to
  see what the wider network reports near you, and run a **receive-only iGate** to
  contribute what you hear. Nexus never gates internet traffic back onto the air.
- **Up to three radios, routed by band and mode.** A band alone is not a fine
  enough rule when two rigs cover 2 m. Route 2 m weak-signal digital to one rig
  and 2 m FM and APRS to another. All rigs stay permanently connected with an
  instant handoff.
- **DX chasing.** The Needed board with heard-by evidence, **Pounce** alerts the
  instant a new one appears (it never moves the radio itself), a DXpedition
  calendar with one coloured bar per operation and modelled best windows, and
  DXCC, Challenge, Honor Roll, WAS, WAZ, per-band VUCC and IOTA computed offline.
- **Every mode is first-class.** Phone with a bandscope, TX meters and voice
  keyer. CW with keying through CAT, a serial keyline, a K1EL WinKeyer or the
  soundcard, plus a neural-net decoder. Satellites with pass schedules, polar
  plots and rotor auto-track. Field Day, POTA and SOTA built in. Star a repeater
  from a search and it lands on a hotkey with distance and bearing.
- **TempoFast and TempoDeep.** Original weak-signal waveforms. TempoFast is a
  coherent four-second cycle with **incremental-redundancy HARQ**, where a failed
  decode is not wasted because retransmissions combine until the message lands. No
  other amateur text mode does that. TempoDeep is a non-coherent tier for fading
  paths. Both have closed real links on the air, including a completed two-station
  QSO on 6 m. TempoFast trades about 6 dB of raw sensitivity against FT8 for a
  cycle nearly four times faster, and every sensitivity figure is still a bench
  number.
- **It joins your station.** WSJT-X's UDP protocol byte-for-byte, so GridTracker,
  JTAlert and your logger keep working. ADIF logging with LoTW, QRZ, ClubLog,
  eQSL, HRDLog, Cloudlog and DXKeeper, credentials in the OS keychain. N1MM+ and
  N3FJP, PSK Reporter, DX cluster and RBN, CHIRP round-trip, and a CAT broker so
  other software shares the radio through Nexus.
- **It updates itself**, signed and verified, standing down while you are
  transmitting or in a contact.

---

## Report a bug or an on-air result

Bug reports and **on-air Tempo decode reports** are the most valuable thing you
can send. Both go to **[GitHub Issues](https://github.com/kd9taw/Nexus/issues/new/choose)**,
where there is a form for each and it asks for the details that decide an
investigation, so you are not left guessing what to include.

For an on-air Tempo report, the form covers the path, the numbers, and the clock
reading at both ends, which has been the cause more than once. A failure is as
useful as a success, and a partial report beats a missing one.

Setup questions and general discussion belong in
[Discussions](https://github.com/kd9taw/Nexus/discussions),
[Discord](https://discord.gg/3Ugtz6MjRE), or the
[groups.io list](https://groups.io/g/hamradiotools).

---

*Nexus is GPL-3.0-only. Not affiliated with ARRL, the WSJT project, or any rig
manufacturer. Built by KD9TAW.*
