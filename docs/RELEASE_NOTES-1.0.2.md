# Nexus 1.0.2 — a transmit-safety fix, FT-mode compound callsigns, and a rebuilt CAT rate table

*2026-08-07*

A transmit-safety fix, FT-mode compound callsigns, CAT rate handling, SSTV station
identification, and a set of logbook and 6 m corrections.

**Everyone should take this one.** The RTS fix alone is worth the install if you key a
serial interface, and the FT-mode work fixes QSOs that could never complete at all.

---

## Transmit safety — install this if you key from a serial port

**Connecting to a serial rig could put it straight into transmit, and hold it there.**
Opening the port left RTS asserted for the whole session. On an interface wired to key
from RTS — a great many homebrew and commercial cables — that is the radio keyed the
moment Nexus connects, with nothing on screen saying so. Nexus now lowers the line it
does *not* key with, and keeps holding the one it does, so PTT still works normally
whichever line you use.

If you have ever seen a rig key on connect and blamed the cable, this was probably us.

---

## FT8 / FT4 — compound callsigns

If your call has a `/P` or `/R` on it, or you chase stations whose calls do, this
release changes what goes on the air.

**A `/P` or `/R` station now sends its grid and exchanges signal reports.** Nexus was
treating any callsign containing a slash as one the protocol cannot carry, and dropping
it onto the message type that has room for neither a locator nor a number. `/P` and
`/R` are the *only* two suffixes the 77-bit protocol carries natively — they ride in
full, with a grid and a report, exactly as WSJT-X sends them. A portable station was
being downgraded to a call-only exchange for no reason.

**A `/P` station working a `/R` rover no longer transmits a callsign belonging to
nobody.** The protocol has one suffix bit per call and one field saying what that bit
means, so a mixed pair cannot be expressed — and every over was going out with the
rover's call rendered as `/P`. Your own log stayed correct and the contact looked
complete, so nothing flagged it; the other operator logged a call you had never used,
and the contact would never confirm. Such a pair now uses the hashed message form,
which carries both callsigns exactly as entered.

**Nine of the sixteen callsign-class pairings could not complete a QSO at all.** The
most common is a home station working a DXpedition with a compound call such as
`PJ4/K1ABC`. When the message form that names both stations correctly has room for
nothing else, "I am calling you" and "here is your report" become the identical
transmission — and the auto-sequencer had no rule for that. Each station read the
other's report as a call, answered it, and then waited for a roger the other was
equally waiting to receive. Nothing was malformed and nothing was mis-addressed, so
nothing on screen said anything was wrong. All sixteen now run to 73 in the usual five
or six overs.

**A contact you never transmitted into could be written to your log.** This is the one
to know about: it reached LoTW and QRZ under your call, for a QSO that never happened,
and nothing on screen would have told you. Logging now requires evidence from both ends
— that the other station answered you on the air, *and* that you put an over on the air
yourself.

**Two stations that called each other at the same instant never got past exchanging
grids.** Plain standard callsigns, nothing exotic. They traded grids for twenty slots
without advancing; they now finish in five overs.

**A contact whose closing `73` never arrived was lost, and a CQ run stopped there.**

**A `/P` or `/R` station could acknowledge a signal report nobody had sent it.**

> Seven of the sixteen pairings genuinely cannot exchange a numeric signal report in
> FT8 — the message form that carries both callsigns has no field for one, and WSJT-X
> cannot send one either. Those contacts now complete and log with the report fields
> **empty**, rather than a number nobody sent.

---

## CAT and rig control

**Picking a rig no longer overwrites a working baud rate.** The rate table was built
from 61 hardware manuals transcribed by hand, and wrong rows moved working radios onto
rates they could not run — an IC-746 at 4,800 was silently moved to 19,200. It now
carries only what the rig's own driver declares: 22 radios that have exactly one legal
rate. Every other radio keeps whatever you set.

**The IC-725, IC-726, IC-728 and IC-729 are in the rig list.** From a field report: an
IC-728 owner could not find his radio, picked the nearest-looking Icom, and Nexus drove
a 1990s rig with a modern radio's CI-V address — every read timed out forever while
WSJT-X worked on the same cable at the same 1,200 baud.
**If you substituted a different model to get going, re-pick your radio.**

**Test CAT finds a wrong baud rate on any serial radio**, not just the five Icoms with a
built-in scope. Twenty-one radios are covered. It runs only after a test has already
failed, so choosing a radio stays instant. A full sweep can take 30–40 seconds, and up
to 90 on an FT-817/818 — deliberately, because the probe waits exactly as long as the
real connection does.

**A port another program is holding now says so**, instead of sweeping every rate and
telling you the radio never answered.

**When CAT fails you are told what Hamlib actually diagnosed** — "rig power is off?",
"serial port does not exist", "port is already open" — instead of the daemon's internal
socket bookkeeping. This previously worked only for Icom owners.

**Icoms no longer receive a Yaesu command on every mode change.** `MD0;` was going onto
every CI-V bus; at worst it dropped the CAT link. If you have had "modes won't switch"
on an Icom, retest that.

**Thetis, PowerSDR, piHPSDR and SDR Console are selectable by name.** A Hermes Lite 2
running Thetis could not get CAT working, and the hint sent its operator to the wrong
port.

**TCI is gone from the rig list** — the bundled Hamlib has no backend for it, so
selecting it produced a radio that never started. SunSDR and ExpertSDR owners use the
CAT server port.

**A WinKeyer port that will not open now says so**, instead of silently falling back to
CAT keying and looking dead.

---

## SSTV

**Your callsign is burned into every transmitted picture.** SSTV was transmitting with
no station identification of any kind — no overlay, no CW ident, no FSK ID — on the one
mode where a single over is up to five minutes of continuous key-down. Your call now
appears top-left, white on a solid black plate, and **Send is refused outright if you
have not set a callsign**.

**Send any picture: it is resized, rotated upright and cropped for you**, with a
drag-to-adjust box so you choose what survives the crop. EXIF rotation is handled, so a
photo taken sideways arrives the right way up.

**The mode picker understated every mode's airtime by about a second.**

---

## Logbook and awards

**Importing a confirmation report over the contacts it describes changed nothing.** A
LoTW, eQSL or QRZ download of contacts already in your log was discarded as duplicate
rather than merged, so confirmations never landed. Expect "0 imported · N existing QSOs
updated".

**A date-only import can no longer destroy a contact.** Importing a log with dates but
no times, where you worked one station twice on a band that day, could delete one
contact and duplicate the other with the wrong QSL.

**A confirmation marked `V` read as unconfirmed.**

**Purging the logbook left the LoTW and eQSL sync positions behind**, so the next sync
brought back nothing.

**Logging a contact re-read your whole logbook off the disk** — about 3× faster now on a
large log.

---

## 6 m and the Needed board

**A short-skip sporadic-E opening on 6 m did not raise an alert**, and a 6 m spot's
local reports were being discarded in favour of ones the gate cannot use. **6 m could
also open on activity that never left the neighbourhood**, and **changing digital mode
could make a dead band alert again**. The openings log under-counted the stations in a
VHF opening.

**The Needed board hid US-spotted rows on exactly the pileups where a US spotter
matters**, and changing digital mode could make its own-radio rows lie about their age.

---

## Appearance

**Light theme: the "NEW ONE — Work it" pounce banner had no readable text** — the
banner drew with a colour token that does not exist, so it painted on the page
background. The "needed" colours were the dark theme's colours everywhere they
appeared. In both themes, the ATNO, new-band and SOTA badges had unreadable labels.

---

## Other

**The window remembers its size, position and maximised state**, restored before the
window is drawn, per radio, and clamped to a screen you still have.

**The Memories strip is readable again** — chips were collapsing to ellipses. Ten
chips maximum, with ▲▼ ordering in the Favorites view.

---

## Known and not fixed

- Tuning into an SSTV picture already in progress still gives you nothing.
- A wrong rig model still reports as "the rig isn't answering." A CI-V radio silent at
  every baud is better evidence of a wrong model than a bad cable.
- The world map keeps dark-theme colours in light mode.
- An FT-817/818 baud sweep can exceed its time budget and report rates as untried.

---

## Verification

Everything above is verified in CI: 2,363 workspace tests, 440 audio, 83 backend, 2,649
UI tests across 208 files, plus clippy at `-D warnings`. The FT-mode work is additionally
checked against WSJT-X's own message packer, and the callsign predicates are measured
against upstream's real behaviour over 66,628 inputs.

**No radio was connected to any of it.** The CAT and transmit-path changes are verified
against Hamlib source and by driving the shipped daemon against stand-in rigs — your
bench is the first real test. If something is wrong, `v1.0.1` is the rollback, and a
report with the CAT status line verbatim is the most useful thing you can send.

73 — KD9TAW
