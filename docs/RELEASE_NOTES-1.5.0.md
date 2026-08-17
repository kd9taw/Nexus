# Nexus 1.5.0 — Meteor scatter you can see

*2026-08-16*

MSK144 stops pretending to be FT8. An operator put it plainly: every signal in this mode sits
at 1500 Hz and lives for milliseconds, so a frequency waterfall shows one unmoving stripe and
the actual event — the ping — is invisible. *"You don't need a 3 kHz display, you need a
15/30/60 second display."* He was right, and this release is built around it.

This is also the first release carrying community code: seven contributions from ON8ST are in,
covering sound-card recovery, multi-radio safety, and the whole macOS side. Thank you — the
review was a pleasure.

**Take this one if you run meteor scatter or satellite voice, if your sound card has ever died
mid-session, or if you run more than one radio.**

---

## MSK144

**The display is a time display now.** Switch to MSK144 and the waterfall becomes the Fast
Graph, the way WSJT-X draws it: seconds across one T/R period, a green power trace where a
ping is a spike you can watch land, the current period above the previous one so a ping that
just ended stays readable, and a marker with the callsign at each decode.

**Pings decode while the period is still running.** Nexus used to decode once at the boundary,
so you watched a silent screen for fifteen seconds and then got history. A ping now reaches
the decode list about two seconds after it lands, and the old DT column reads as **T** — the
time of the ping within the period, which on meteor scatter is information, not clock error,
so it stops being painted red.

**The mode behaves like WSJT-X's.** Entering MSK144 parks both offsets on 1500 Hz, where the
mode actually transmits — the TX marker could previously sit at 2400 Hz while the rig keyed
1500. A 5/10/15/30 s T/R selector sits beside the mode pills, because on meteor scatter the
period is an operating decision you make with the other station, not configuration.

## From the community

All seven of these are ON8ST's work.

**A dead sound card recovers.** A capture-stream error used to be logged once and acted on
never — the audio thread kept running against a card that would not deliver another sample,
and the waterfall froze until you restarted. A stream error now puts the card on probation,
silence confirms the death, and the card is rebuilt — after waiting for your key to come up,
so a recovery never cuts an over. A flapping card is rebuilt at a steady rate rather than the
flap rate.

**Adding a radio no longer switches the station onto it.** Pressing "Add radio" silently moved
the station onto the new, empty profile — tearing down the working rig's CAT to bring up one
with no port and no model, which froze the interface and blanked the settings pane. A new
roster entry is just a roster entry; "Make active" stays the deliberate act.

**The Mac side got a real pass.** Serial ports no longer show twice (the tty twin of each
cu port could hang a CAT probe waiting for a modem line no radio asserts), a rigctld that
cannot actually run is skipped for one that can instead of leaving CAT dead with no
diagnosis, and Nexus builds from source on Apple Silicon out of the box — linker paths and
test suite included.

## Satellites

**Working the ISS voice repeater no longer transmits on the downlink.** Two faults, both
found on the air. A same-band V/V pass asked the Main/Sub VFOs to do something those radios
cannot — Main and Sub can't both sit on 2 m — so the split was never written; V/V passes now
ride the A/B split, which is how they are worked. And a rig keyed from the hand mic did not
count as transmitting, so half a second into an over the Doppler tick would retune the
transmit VFO out from under you. A mic-keyed rig now counts as keyed, everywhere.

## Logging and operating

**Worked-before comes in two strengths.** The B4 chip is hollow when you have worked the call
anywhere and solid when you have worked them on this band — the same two scopes WSJT-X colours
separately. A new setting makes mode count too, off by default like WSJT-X's. And **CONFIRM is
now NEEDS QSL**: the tag marks an award slot on this band still waiting on a confirmation — a
fact about your award, not a claim about the station, which is why it can appear without B4.

**"Call CQ" stops claiming you are calling CQ.** The toggle lit through every S&P contact and
forever after; it and the AUTO-CQ pill now read the real CQ-run state, and the idle button
drops its red border — red means TX-armed here, which an idle station is not.

**Your own transmission scrolls as a dark band on the waterfall**, the honest "not listening"
gap, instead of painting a full-width red band after every over.

**Two silent losses are fixed:** every one-character CW word — the closing K, a bare ? — was
rejected by the keying library over CAT (it reads a single character as a stored-memory
number), and Ham Radio Deluxe forwarding sent nothing when HRD was the only connector enabled,
while the app said "Logged".

## Also

The Spots panel shows the **state** it already filters by, as a column. **Program** takes
channels you type and imports CHIRP CSV. The rig scope's waterfall gets the **scroll-direction
button**. **HF is grid-quiet by default** — rare-grid alerts scope to VHF and up, where a grid
is news; an HF grid-chaser can widen it back in Settings ▸ Spots & Alerts. The update banner's
**"Not now" sticks** for that version. And the FT8 cockpit no longer hosts the memory
favorites strip — a favorite chip one click from retuning the rig mid-sequence was a Phone/CW
convenience in the wrong room.

Full detail, as always, in the [CHANGELOG](../CHANGELOG.md).

73 — KD9TAW
