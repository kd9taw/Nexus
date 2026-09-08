# Scenarios & troubleshooting

Jobs that cross several chapters. Each one names what you need before you start,
the steps in order, what you should see when it worked, and what to do when it
did not.

The per-section chapters stay the reference for one screen. This page is for the
work that does not live on one screen: stopping a transmission, getting a dead
receive path decoding again, arriving from another program, and running a station
that is not the simple one-radio-online case.

## Stop a transmission

**When you need this:** a transmission is going out and you want it off the air
now — wrong frequency, wrong message, a stuck key, a neighbor on the phone.

**Before you start:** nothing. This is the one job in Nexus that has no
prerequisites by design.

### The rule

**Every cockpit keeps at least one stop control on screen at all times.** The ⊞
Panels menu can hide a great deal of a cockpit; it can never hide the last way to
stop transmitting. That is enforced by a test that hides every panel in every
cockpit, one at a time and then all at once, and looks for the stop controls by
name.

### Steps

1. **Press `Esc`.** In Operate, CW, RTTY, PSK and JS8 this stops the
   transmission from anywhere in that section — in Operate and CW it works even
   with the cursor in a text field. In Phone, release the space bar.
2. **If `Esc` did nothing, press `Stop TX`** in the cockpit header. It is never
   disabled and never hidden.
3. **Drop the TX latch**, where the cockpit has one on screen — the ▼ TX On /
   ■ TX Off control in the header of Operate, RTTY, PSK, JS8 and SSTV. This does
   not just cut the over; it disarms transmit, so nothing can start another one.
   Phone and CW arm elsewhere and have no latch in the header.
4. **If a tune carrier is up, press `Tune` again.** Tune is a toggle — the button
   reads TUNING… while the carrier is up, and the second press drops it. It also
   auto-stops on its own watchdog.

**Expected result:** the rig unkeys, the TX meters fall to zero, and the header's
TX indicator goes to off. In RTTY and PSK the continuous-TX latch drops too, so
nothing you typed ahead goes out.

### Which control is where

Every cockpit's on-screen stop line:

| Cockpit | Controls that stop a transmission |
|---|---|
| Operate (FT8/FT4) | `Esc` · **Stop TX** · **TX Off** · **Tune** |
| Phone | The **PTT** button (release) · space bar (release) · **Stop TX** · **Tune** |
| CW | `Esc` · **Stop TX** · **Tune** |
| RTTY | `Esc` · **Stop TX** · the dock's **Esc / Stop** macro · the TX latch · **Tune** |
| PSK | `Esc` · **Stop TX** · the dock's **Esc / Stop** macro · the TX latch · **Tune** |
| JS8 | `Esc` · **Stop TX** · **Tune** |
| SSTV | **Stop** · the TX latch |

Two stop controls live *inside* removable panes and can be hidden: Phone's voice
keyer **■ Stop**, and RTTY's **Auto on** toggle in the `stream` pane. Both are
conveniences on top of the guarantee, not the guarantee.

### If the rig is still keyed

**A latched RTTY or PSK over has one more stop that nobody presses.** Those two
modes hold the transmitter up with no pre-computed end, so the engine re-checks
every transmit gate continuously and drops the latch — not just the text feed —
the moment one goes down. Leaving the section, a QSY out of your privileges, a
tune, or a radio handoff each unkey it within a tick. That is a property of the
continuous-TX latch; do not count on it in the other modes.

Otherwise:

1. **Unkey at the radio.** The rig's own PTT, or its power switch, is the
   backstop and it always wins. Nothing in software outranks it.
2. **A held mic PTT is not something Nexus can release.** If the app says "Mic
   PTT is held", the key is down outside the app: a stuck foot switch, a mic
   button, or a CAT-broker client holding PTT. Nexus deliberately does not
   override a held key.

If a stuck transmitter survived a **Stop TX**, that is a defect worth reporting —
say which cockpit, which control you pressed, and what the header said.

## The rig will not key

**When you need this:** you press send and nothing goes out, or a refusal message
appears.

**Before you start:** know your callsign is set, and know which band and mode the
rig is actually on.

Nexus refuses a transmission for one of a small number of reasons, and it always
says which. Work down the list:

1. **"TX locked — this frequency is outside your license privileges."** The dial
   and mode you are on are not permitted for your license class. Change band, or
   correct the class in
   [Settings ▸ Station ▸ Operator & Radio](settings-reference.md#operator--radio).
   Under split, the message names the frequency it is judging — the *transmit*
   one, which is not the one you are listening on.
2. **The TX latch is off.** The header control reads **TX Off**. Click it on.
   Receive works with it off; nothing transmits.
3. **Your callsign is not set.** Every network- and air-facing path checks it.
   [Settings ▸ Station](settings-reference.md#operator--radio).
4. **Something else holds the transmitter.** Nexus arbitrates one transmitter
   between eight owners, and names the one holding it:

   | What it says | Who has it | What to do |
   |---|---|---|
   | Another transmission is in flight — stop it first | The FT8/FT4 slot sequencer | `Esc`, or **Stop TX** |
   | Tune carrier is up — stop tuning first | The tune carrier | Press **Tune** again |
   | Mic PTT is held — release it first | A held mic key, yours or a CAT-broker client's | Release the key at the rig |
   | A voice message is transmitting — stop it first | The voice keyer | **■ Stop** in the keyer pane, or **Stop TX** |
   | CW is sending — stop it first | The CW queue | `Esc` |
   | RTTY is transmitting — stop it first | An RTTY over, or the continuous-TX latch | `Esc` |
   | PSK is transmitting — stop it first | A PSK over, or the continuous-TX latch | `Esc` |
   | An SSTV image is transmitting — stop it first | An image in flight | **Stop** |

5. **The transmit watchdog tripped.** A continuous-TX limit was reached and
   transmit auto-halted. Re-enable TX to clear it.
6. **RTTY only, on the FSK backend:** the data line and a serial PTT line are
   configured onto the same physical line. Fix it in
   [Settings ▸ Digital ▸ RTTY](settings-reference.md#rtty).

**Nothing refused and still no RF?** Then the software thinks it transmitted and
the problem is downstream. Use **Prove TX** on the
[Setup health strip](settings-reference.md#setup-health) in Settings ▸ Radio: it keys a
~2-second tune carrier to test the whole CAT → PTT → RF path, asks for
confirmation first, and shows forward power while it keys. Have an antenna or
dummy load connected. If Prove TX produces power and your mode does not, the
fault is in that mode's audio path or drive; if Prove TX produces nothing, it is
CAT, PTT wiring, or the radio.

## Nothing is decoding

**When you need this:** the waterfall is blank, or it looks alive but no text
appears.

**Before you start:** know which sound card the radio's receive audio arrives on.

1. **Look at the waterfall first.** Band noise should show as a moving floor. A
   flat, dead waterfall means no audio is reaching the app — this is a device
   problem, not a decoder problem. Go to step 2. A lively waterfall with no text
   means the decoder is not armed or is looking in the wrong place — go to step 4.
2. **Check the input device.**
   [Settings ▸ Radio ▸ Audio](settings-reference.md#audio) ▸ **Input Device
   (RX)**. The **Live input spectrum** on that page shows what the selected
   device hears; if it is flat there too, the device is wrong. **Refresh**
   re-scans, which matters after plugging a USB interface in.
3. **Check the level.** **RX Level** on the same page should read around 30 dB.
   Anything from about 15 to 60 dB decodes. Red is too hot — turn the rig's audio
   output down. Under 15 dB, raise **RX Gain** until the meter comes up; the
   meter responds when you release the slider.
4. **Check the section is armed.** RTTY and PSK do not decode until their
   receiver is armed — see [RTTY](rtty.md) and [PSK](psk.md) for what arms them
   and what the pane head shows when it is not armed. FT8/FT4, JS8 and SSTV
   decode without arming.
5. **Check you are on the right frequency and mode.** The rig has to be in a data
   mode with a wide enough filter, on the dial the mode expects.
   [Settings ▸ Digital ▸ Working Frequencies](settings-reference.md#working-frequencies)
   holds the dial each mode starts on.
6. **Check the clock.** FT8, FT4, JS8, Tempo and the other slotted modes decode
   against UTC slots. The Now-Bar shows the clock offset; a machine more than a
   second or two out decodes badly or not at all. Fix it with the operating
   system's time sync, not in Nexus.

**Expected result:** the waterfall shows a moving noise floor, RX Level sits near
30 dB, and decodes appear at the end of each period.

## Move over from WSJT-X

**When you need this:** you have been running WSJT-X and want Nexus to be the
station instead — or alongside.

**Before you start:** a copy of your existing log as ADIF, and WSJT-X closed if
it is holding the same sound card and CAT port. Two programs cannot own one
serial port.

1. **Set your station up** — callsign, grid, license class, rig, sound card.
   The first-run wizard covers all four.
2. **Bring the log across.** **Import ADIF** in the logbook takes a WSJT-X
   `wsjtx_log.adi` (or an export from any other logger). It adds contacts you do
   not have and applies confirmations and award credits to ones you do. A toast
   reports the two counts separately. See
   [Logbook & QSL](logbook-qsl.md).
3. **Point your existing tools at Nexus.** If you run JTAlert or GridTracker,
   turn on **WSJT-X UDP API** in
   [Settings ▸ Logging & Connectors ▸ Integrations & Feeds](settings-reference.md#integrations--feeds)
   — default `127.0.0.1:2237`, the address those tools already expect. For
   loggers that tail a decode file, turn on **Write ALL.TXT decode log**; it is
   written only while that is on, and the file first appears after the next
   decode.
4. **Turn on your confirmation services** in
   [Settings ▸ Logging & Connectors ▸ Confirmations](settings-reference.md#confirmations)
   — LoTW, QRZ, ClubLog, eQSL, HRDLog. Then pull your confirmations down once;
   the first pull covers your whole history.

**Expected result:** your entity, band and mode counts in
[Awards & Journey](awards-journey.md) match what you had. If they do not, the
import is the thing to check first — Awards counts the local log and nothing
else.

**What is different from WSJT-X.** The keyboard shortcuts match on the keys that
matter (`Esc`, `F4`, `F6`, `Alt+1`–`Alt+6`), and F4 clears the DX call even with
the cursor in a field, which is WSJT-X's own behavior. See the
[keyboard reference](index.md#keyboard-shortcuts).

## Run two radios

**When you need this:** a second rig on the desk, or one rig for HF and another
for VHF.

**Before you start:** each radio needs its own CAT connection and its own sound
card. Add and route them in
[Settings ▸ Radio ▸ Radios](settings-reference.md#radios) — the roster, band
routing, and which radio is ACTIVE.

The chapter that carries the worked example is
[Settings reference ▸ Radio](settings-reference.md#radios). What matters for
every other page: **one transmitter is arbitrated at a time**, and a radio
handoff is one of the events that unkeys within a tick. If a transmission stops
when you did not stop it, a band change or a handoff is the usual reason.

## Run a club event

**When you need this:** Field Day, Winter Field Day, or any multi-operator event
on one log.

**Before you start:** the class, section and exchange for your entry, set in
[Settings ▸ Contesting ▸ Field Day Setup](settings-reference.md#field-day-setup);
and, for more than one position, a host on the LAN in
[Settings ▸ Contesting ▸ Field Day Club Sync](settings-reference.md#field-day-club-sync).

Both are written up in [Field Day & POTA/SOTA](contesting-pota.md). The
cross-cutting pieces:

- **The club band board tears off** onto a second monitor or a TV, set in glance
  type because it is read across a tent rather than at the keyboard. See
  [Windows you can tear off](index.md#windows-you-can-tear-off).
- **The CW macros change** during a Field Day event — the exchange set replaces
  the ragchew set. [CW](cw.md) has both tables.
- **Dupes are shared live.** With club sync on, each position's contacts stream
  to the host as they are logged, and the host pushes back the club-wide dupe
  list that drives the while-typing dupe warning. It is not a merge afterwards.
- **The host is the only time Nexus listens beyond this computer**, and only
  while the toggle is on. Contacts logged while the network is down re-send on
  reconnect, and hosting can move to any other position if the host PC dies.

## Operate with the feeds down

**When you need this:** no internet at the site, or the cluster and PSK Reporter
are unreachable.

Nexus is built so the station still works. What changes:

| Still works offline | Needs a feed |
|---|---|
| Every mode: decode, transmit, log | The [Needed board](needed-dx.md) — every row is a spot or a reception report |
| The [logbook](logbook-qsl.md), including edits and export | [Spots](spots.md) — the raw cluster and RBN traffic |
| [Awards & Journey](awards-journey.md) — counted from the local log, no network at all | [DXpeditions](dxpeditions.md) — the expedition list |
| [Stats](stats.md) | Confirmation upload and download |
| [Satellites](satellites.md) pass prediction, from the orbital elements already downloaded | Fresh orbital elements — refresh them before you leave |
| [Memories](memories.md) and [Program](program.md) channel lists you already have | Repeater lookups for a new location |

**Before you go:** refresh the orbital elements
([Settings ▸ Radio ▸ Orbital elements](settings-reference.md#orbital-elements)),
the country file
([Settings ▸ Logging & Connectors ▸ Country file (DXCC)](settings-reference.md#country-file-dxcc)),
and the LoTW users list
([Settings ▸ Logging & Connectors ▸ LoTW users list](settings-reference.md#lotw-users-list)).
All three are local files once fetched.

The Now-Bar's feed-health pills tell "connected but quiet" apart from "down", so
a dead cluster reads as dead rather than as a quiet band.

## Honest limits

- **This page is not a substitute for the section chapters.** It carries the
  cross-cutting path only; the control-by-control detail stays where the control
  lives.
- **Nexus cannot release a PTT it does not hold.** A stuck foot switch, a mic
  button or a CAT-broker client holding PTT is outside the app, and only the
  radio ends it.
- **A refusal message is the app's own reasoning, not a measurement of the
  air.** "TX locked" means the software gate said no; it does not prove the rig
  did or did not emit.
- **Nothing here has been verified against every rig.** The CAT, PTT and amplifier
  paths differ per model, and several are documented as not verified on hardware.

## Related guides

- [Settings reference](settings-reference.md) — every field, by tab
- [Operate — FT8/FT4 digital](operate-digital.md) — the cockpit most of these
  scenarios start in
- [Logbook & QSL](logbook-qsl.md) — import, export and the connectors
- [Guide overview](index.md) — the keyboard reference and the pop-out matrix
