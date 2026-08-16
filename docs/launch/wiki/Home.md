# Nexus: the whole ham radio station in one modern app

**Nexus** is a free, open-source amateur radio operations center. Digital, phone,
CW, APRS, satellites, propagation, logging and awards in one modern app, built for
everyone from a new Technician making a first FT8 contact to a DXCC Honor Roll
chaser. Windows, Linux and Raspberry Pi. GPL-3.0, built in Rust.

[**⬇ Download the latest release**](https://github.com/kd9taw/Nexus/releases/latest)
&nbsp;·&nbsp; [Source](https://github.com/kd9taw/Nexus)
&nbsp;·&nbsp; [hamradiotools.io](https://hamradiotools.io)

> **Nexus left beta at 1.0.0, and here is what that claims.** The FT8/FT4 core
> is the production tier: over a thousand automated tests, wire formats pinned by
> test, field-verified end to end on a Yaesu FTDX10 and FT-991A and on native-CI-V
> Icom. APRS is on the air with its internet feed and receive-only iGate confirmed
> against a live connection. It does **not** claim nothing is left, and what is
> left stays printed rather than buried: the FlexRadio native panadapter is opt-in
> and not yet confirmed on hardware here, and TempoFast and TempoDeep have closed
> real links on the air — including a completed two-station QSO on 6 m — with
> their sensitivity figures still bench numbers. The Windows installer is
> unsigned, so verify the published SHA-256.

---

## Why it exists

Four problems, stated as problems. Nexus ships an answer to each, and where an
answer is partial this says so.

### 1. Every failure looks like silence

A dead band, a deaf receiver, the wrong sound card, a two-second clock error, a
missing tone, a radio parked on the wrong frequency: they all present identically,
as nothing happening. That is where people give up.

Nexus names the fault instead. The APRS readout says *"on 144.174 USB, APRS needs
144.390 FM"* and puts a Tune button beside it, judged against the channel **you**
selected so 144.800 in Europe is correct rather than a warning. A closed squelch
is reported separately from a dead capture device, because only one of them is a
problem, and the input level reads in dBFS so you can see what the decoder hears.
On a multi-radio station it says which rig it is listening to. Every claim carries
its age, so a count from six minutes ago cannot assert something about now.

### 2. Nobody can tell whether a band is open for them

Cluster and skimmer feeds are global. A superstation on a mountain hearing a rare
one says little about what your antenna reaches from your yard.

Nexus scores openings on receivers near **you**, and every Needed row shows who
nearby actually heard the station and when, so you know the path is real before
you call. On 2 m and 4 m a single genuine path past 700 km opens a band, because a
tropo or aurora opening is often one distant station rather than a crowd; 6 m asks
for two, because 700–1400 km there is the meteor-scatter regime and one station
pinging a few times is a rock, not an opening. Predictions come from a native
in-app port of **ITU-R P.533**, the VOACAP-class standard, with no external program
to install, and are labelled *modelled* wherever they appear.

*Where this stops:* it measures the paths other stations report. It does not map
your own antenna pattern or calibrate against a beacon ladder, so it can tell you
a path is open and still not know your null is pointing at it.

### 3. Delivery and credit are dishonest end to end

Uploads are usually fire-and-forget booleans, and a silently rejected LoTW record
is a named phenomenon in this hobby. Confirmation truth is scattered across five
services with nothing keeping a per-contact record of which confirmed, when, and
at what award level.

An eQSL or QRZ match shows a contact as confirmed here and **never** counts toward
DXCC or WAS, because counting it would inflate them. Awards are computed offline
from your own log with no account required. A record LoTW refused is reported as
rejected rather than stamped sent and dropped. Editing a logged contact re-sends
it, so the online logbooks cannot quietly disagree with yours. Tempo chat messages
show waiting, sending, no ack or delivered, and a part-received message reads
"2 of 3 received" rather than waiting forever with a blank window.

### 4. The interface evicts the operators who know the most

The median operator is 60 to 70 and the fastest-growing group of new licensees is
over 50. Hearing loss pushes people off SSB and a waterfall-first interface pushes
them off digital.

Screen readers work here with nothing to switch on. JAWS and NVDA hear the QSO
sequencer advance, and transmit and receive switches are announced assertively.
Band activity, the Call Roster and the Needed list are keyboard-navigable, each row
read aloud, with Enter to select, Shift+Enter to work and Alt+Enter to ignore.
Optional spoken decode announcements, a TX/RX earcon and a per-cycle tick let you
operate by ear. Nothing changes for sighted operators.

---

## What is in it

**Setup is the fast part.** A three-step wizard finds your radio over USB or the
network (FlexRadio included), fills in CAT and audio, and pairs the sound device.
Around fifty rigs are curated out of the box, from the IC-9700 up to 23 cm to the
Xiegu G90, with Hamlib bundled. Digirig and RIGblaster interfaces are recognised
by name. Your licence class becomes a hard transmit lockout in every keying path,
and opening the app never touches your rig.

**Fifteen modes, all of them transmitting.** FT8 and FT4 with a sequencer built
to WSJT-X's behaviour and checked against a 207-row parity matrix, plus **Q65,
FST4, FST4W, MSK144, JT65 and WSPR**, each keeping its own operating rhythm rather
than inheriting FT8's. Add RTTY, SSTV, APRS, CW, SSB and FM, and the two Tempo
tiers. Every waveform that gained transmit was verified by generating a
transmission in Nexus and having **stock WSJT-X decode it**, rather than by testing
Nexus against its own decoder.

**A decoder you can check.** Built from WSJT-X's own DSP sources, tracking upstream
3.0.2. On identical recorded audio its floor measures **−21.3 dB against stock
WSJT-X's −20.7** with no false decodes, and it is tested against pure noise so
silence stays silent. Rovers keep decoding here, where upstream 3.0.2 discards any
call carrying `/R` outside contest mode. It speaks WSJT-X's UDP protocol
byte-for-byte, so **GridTracker, JTAlert and your logger keep working**.

**Real rig control.** On a native-CI-V Icom or a network FlexRadio the rig's own RF
panadapter streams into the cockpit, with AGC, noise reduction, filter width and
mic gain mirroring the front panel. The waterfall pauses and rewinds through the
last few minutes of the band, with a 3-D stacked-spectrum view. Rig control
recovers on its own, where a wedged link used to cost you CAT until restart.

**APRS with the whole network.** Stations draw as their real APRS symbol, coloured
by what they are, with a ring saying whether your own antenna heard them or the
internet did. Click one for what the symbol means in words, distance and bearing,
the digipeater path and decoded weather readings. Connect to APRS-IS to see what
the wider network reports near you, and run a **receive-only iGate** to contribute
what you hear. Nexus never gates internet traffic back onto the air.

**Up to three radios, routed by band and mode.** A band alone is not fine enough
when two rigs cover 2 m. Send 2 m weak-signal digital to one and 2 m FM and APRS
to another. All rigs stay permanently connected with an instant handoff.

**DX chasing that knows your log.** The Needed board with heard-by evidence,
**Pounce** telling you the instant a new one appears (it never moves the radio
itself), a DXpedition calendar with one coloured bar per operation and modelled
best windows, and DXCC, Challenge, Honor Roll, WAS, WAZ, per-band VUCC and IOTA
computed offline.

**Every mode is first-class.** Phone with a bandscope, TX meters and a voice keyer.
CW with keying through CAT, a serial keyline, a K1EL WinKeyer or the soundcard, and
a neural-net decoder. Satellites with pass schedules, polar plots and rotor
auto-track through a pass. Field Day, POTA and SOTA built in. Star a repeater from
a search and it lands on a hotkey with distance and bearing from wherever you are.

**TempoFast and TempoDeep.** Original weak-signal waveforms. TempoFast is a
coherent four-second cycle with **incremental-redundancy HARQ**, where a failed
decode is not wasted because retransmissions *combine* until the message lands. No
other amateur text mode accumulates redundancy that way. TempoDeep is a
non-coherent tier for fading paths. TempoFast trades about 6 dB of raw
single-shot sensitivity against FT8 for a cycle nearly four times faster. Both
have closed real links on the air, and every sensitivity figure is still a bench
number.

**It updates itself**, signed and verified, standing down while you are
transmitting, tuning, in a contact or running CQ.

---

## Get started

- **[Quick Start](Quick-Start)** from install to your first FT8 contact.
- **[Install & Verify](Install)** download, SmartScreen, SHA-256, where data lives.
- **[Rig Setup](Rig-Setup)** Yaesu, Icom including the IC-9700 to 23 cm,
  FlexRadio, Xiegu, rotators, and multi-radio routing.
- **[FAQ](FAQ)** the common questions.
- **[Documentation](Documentation)** the full manual set.

## Report a bug or an on-air result

Bug reports and **on-air Tempo decode reports** are the most valuable thing you can
send. Both go to
**[GitHub Issues](https://github.com/kd9taw/Nexus/issues/new/choose)**, where there
is a form for each kind and it asks for the details that actually decide an
investigation, so you are not left guessing what to include. A failure is as useful
as a success, and a partial report beats a missing one.

Setup questions and general discussion belong in
[Discussions](https://github.com/kd9taw/Nexus/discussions),
[Discord](https://discord.gg/3Ugtz6MjRE), or the
[groups.io list](https://groups.io/g/hamradiotools).

---

*Nexus is GPL-3.0-only. Not affiliated with ARRL, the WSJT project, or any rig
manufacturer. Built by KD9TAW.*
