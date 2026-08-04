# Changelog

All notable changes to Nexus (formerly Tempo) are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.28.0] — 2026-08-03

### Added: log a contact from the Satellites section

Reported after a clean pass: "the doppler shift change are working amazing, I was following the
bird with my manual rotor and it was perfect. The problem came when I tried to log someone, as I
dont have a spot to log within the satellites section to log my sat qso's."

There is now a log strip in the bird's detail column, directly under the Doppler readout. It is
not a new form — it is the same log strip the Phone and CW cockpits use, with the same callbook
lookup, the same recall card and the same prior-contact history, put where you are working the
pass. It sits above the transponder cards and the globe on purpose: with both hands on a rotator
and seconds between overs, a form at the bottom of the column is a form you don't use. It is
there whether or not a pass is armed, and it stays after the bird sets, so you can catch up on a
contact once your hands are free.

**It logs an ordinary contact.** The call, your dial, your band, the mode you are on, the time —
exactly what the Phone strip logs from the same state. It does **not** tag the contact as a
satellite QSO: LoTW wants the ADIF `PROP_MODE` and `SAT_NAME` fields for that, and Nexus does not
write them. The strip says so under the fields, so nobody waits on satellite credit that isn't
coming. If you want it, add **both** fields yourself in whatever you upload from — LoTW turns away
a contact carrying only one of the pair, so half the tag is worse than none.

**Three things it does not do yet.** None of them is a decision that satellite work should stay
this way — they are the price of dropping the existing strip in unchanged instead of building a
satellite-aware one, and each is meant to be closed.

- **Your satellite grids land in the wrong place, in Nexus and at ARRL.** Nexus decides "was this a
  satellite QSO?" from `PROP_MODE=SAT` alone; LoTW wants that *and* the satellite's name. With
  neither written, a contact logged here counts toward neither the **Satellite VUCC** totals on the
  Awards screen nor the satellite needs board — and its grid does not simply go uncounted. Nexus
  keeps per-band grid counts for 160 m through 2 m only, so anywhere outside that range — 2200 m
  and 630 m below it, and 1.25 m, 70 cm, 23 cm, 13 cm, 9 cm, 5 cm, 3 cm and 1.2 cm above it — the
  grid lands nowhere at all. On the bands that do have a slot — for satellite work that means
  **2 m**, the downlink of every U/V bird, and **10 m**, where AO-7's mode A comes down — it lands
  in your **terrestrial** VUCC count for that band, which is a grid ARRL's rules say a satellite QSO
  does not earn. LoTW files the upload the same way. If you chase VUCC, add both fields by hand
  before you sign.
- **During Field Day it logs to the general log, not the contest log.** The Phone and CW strips
  switch to the Field Day log while a session runs; this one is not wired to Field Day yet, so a
  satellite contact made during FD scores the club nothing. Log those from the Phone or CW
  cockpit for the duration — adding them to the FD log later files them on the band you are on
  then, not the band you worked.
- **The recorded mode comes from your sideband**, so it names a voice mode when you are on a data
  mode: `SSB` on the WSJT-X tiers (every channel there commands USB), `FM` on Tempo's three FM
  simplex channels. Right for voice and CW, wrong on a data mode. **Log a contact from another
  radio** lets you set the mode by hand, but its picker offers SSB / FM / AM / CW / RTTY / FT8 /
  FT4 and nothing else — so of the data tiers it covers FT8 and FT4 only. On Q65, JT65, MSK144,
  WSPR, FST4 or Tempo, log the contact and then correct its **Mode** in the Logbook, whose mode
  field takes any text.

All three are written up in the guide's [satellite chapter](docs/guide/satellites.md).

### Fixed: contacts logged during a pass no longer carry a satellite name LoTW rejects

This one is in the version you are running now. Since 0.24.0, any contact logged while you had a
transponder held was written with `PROP_MODE=SAT` and a `SAT_NAME` — and the name was the bird's
*catalog* name, "SAUDISAT 1C (SO-50)" rather than "SO-50". LoTW rejects a record naming a
satellite it doesn't recognise (ARRL: "if you enter the satellite name as AO7 instead of AO-7 the
data will be rejected") — the signing tool won't even sign it — so those contacts could never earn
satellite credit, and the batch each one rode in came back marked **Rejected**.

It also caught contacts that had nothing to do with a satellite. The hold is only handed back when
the pass ends — and a transponder you pick without arming a pass is never handed back at all — so
an HF contact made an hour later got tagged as a satellite QSO too, and went out that way to LoTW,
eQSL, ClubLog, QRZ Logbook and Cloudlog.

Nexus no longer writes either field for any contact it logs. Records that arrive carrying them —
a foreign ADIF import, or one you fixed by hand — are untouched, on import, on export and in the
logbook.

**That has a cost inside Nexus too, and it is not permanent.** The same `PROP_MODE=SAT` field is
what Nexus reads to decide a contact was a satellite QSO, so with nothing writing it your
**Satellite VUCC** totals and the satellite needs board no longer see contacts Nexus logs for you
either — and on a band Nexus keeps a per-band grid count for (2 m most commonly, since it is every
U/V bird's downlink) the grid is counted toward your terrestrial VUCC for that band instead, which
is not a grid a satellite QSO earns. Writing a satellite name Nexus can stand behind is work that
has not been done yet, not work that was ruled out.

**Writing just `PROP_MODE=SAT` is not a shortcut** — that was looked at and rejected, not
overlooked. TQSL refuses to sign a contact whose propagation mode is `SAT` when it names no
satellite ("PROP_MODE = 'SAT' but no SAT_NAME"), exactly as it refuses a name it doesn't
recognise. Half the tag costs you the whole QSO at LoTW — including the DXCC and WAS credit an
untagged upload *does* earn — and it takes the upload with it: Nexus asks TQSL to skip bad records
rather than abort, so the batch signs without that one and comes back marked **Rejected**, and it
will keep coming back that way every time you upload while the record is in your log. Until Nexus
can write a name it can stand behind it writes neither, and adding **both** fields by hand restores
everything: the LoTW credit on your next upload, and the in-app totals the next time Nexus starts
and re-reads the log file.

**Contacts already logged that way are left alone**, and deliberately: some of them were real
satellite QSOs that want the name corrected, some were ordinary contacts that want the tag gone,
and nothing in the record tells them apart — that call is yours. To find them, search your log
file (`~/.config/tempo/log.adi`, or `%APPDATA%\tempo\log.adi` on Windows) for `SAT_NAME`, and edit
it there with Nexus closed. The guide's [satellite chapter](docs/guide/satellites.md) walks
through it.

### Fixed: the microwave bands go out to N1MM / N3FJP in metres

The `band` field on the club-log wire is a **metre count** — "20", "0.7". Nexus converted 70 cm,
33 cm and 23 cm by hand and guessed at everything else by chopping the letters off the end, which
cannot tell centimetres from metres. So every other centimetre band left as a bare number: a
13 cm contact was broadcast as "13", a 3 cm contact as "3".

Those are not hypothetical bands. The Q65 band plan ships 13 cm, 9 cm, 5 cm, 3 cm and 1.2 cm
channels (JT65 the first three), and the band you pick is the band that goes on the wire. Nobody
is known to have been caught by this — what is on record is that Nexus could send it.

Centimetre bands are now converted by the same rule the three hand-written ones already followed,
so 13 cm goes out as "0.13". **The three values that have always gone out are unchanged**, no band
was added to any list, and nothing else about what Nexus broadcasts moved.

### Fixed: the ⊞ Panels menu no longer offers a checkbox that changes nothing, and it gains a Voice Keyer entry

From the bench: "what do the Panels selection / deselection of Rig Scope Controls and TX meters
do on the Phone tab? I don't see them anywhere on my screen whether enabled or disabled. Nothing
falls away when I uncheck either of them."

Both entries were live checkboxes with nothing behind them on that station. **Rig Scope Controls**
— and its twin, **Scope Controls** in CW — drive the radio's *own* panadapter: the span you set
there changes the hardware sweep, not the on-screen zoom. On the audio bandscope there is no rig
scope to command, so the pane cannot appear however the box is ticked. **TX Meters** work, but in
Phone and CW the meters read only while you are keyed, so on receive there is nothing on screen
either way — and the one moment the tick shows is mid-over, when you are not in a menu.

The menu now says which is which, the same way the DSP row has always only offered the functions
your rig reports. An entry with nothing on screen behind it right now carries the reason in a line
under it — "your radio is not streaming its own scope — these appear with an Icom CI-V or
FlexRadio panadapter" — so you learn why instead of hunting for a pane that was never going to
mount, and the line clears by itself the moment a native scope streams. TX Meters carry "readings
appear on transmit" in Phone, CW and Operate alike. Nothing is removed from the menu: an entry
with a reason on it tells you more than a missing one.

The same two questions turned out to have five more answers in the same menus, and they read the
same way now. In Phone and CW, **DSP Functions / DSP Toggles** and **RX DSP Levels** depend on
what your radio reports over CAT — on a rig that reports neither, those panes could never appear
and the boxes did nothing. In CW, **Sent Echo** holds what you have transmitted this session, so
at every start-up it is empty and its box moved nothing at all until your first over; it now says
so, and the line clears when you send.

The line explains the screen; the tick is still yours. Every box in the menu can be ticked and
unticked whenever you like, whatever your rig is doing, and what you choose applies the moment
that panel has something to show — untick **Sent Echo** at start-up and it stays away after your
first over, instead of making you transmit before you can hide a panel you do not want. Once you
have unticked one, its line goes: the panel is off your screen because you said so, and a note
still claiming your rig is the reason would be telling you something that is no longer true. Tick
it back and the line is there again in the same instant, if it still applies.

The menu also gains one entry it never had: **Voice Keyer** in Phone. If you work with a mic and
never touch the F-key slots, you can now put that pane away and the panes around it take the room.
It was held out until now by a blunt rule — nothing that transmits gets a checkbox — and the rule
is narrower now, because what it was really protecting is one thing: **you must never be unable to
stop a transmission.** That is held by the controls that sit outside the panes entirely: on Phone,
PTT in the transmit bar (and the Space bar, which is the same key), Stop TX and Tune in the header;
on CW, Stop TX and Tune, and Esc, which does what Stop TX does; on the Operating screen, Stop TX and
Tune in the operating strip, and Esc; on RTTY, Stop TX, the TX-enable button and both aborts in the
transmit bar; on SSTV, Stop and the TX-enable button. None of them is in a Panels menu, so no
combination of ticks and no saved layout can put them out of reach — that is the guarantee, and it
is the only thing the rule protects.

Two buttons on the Operating screen are worth naming because they are *not* on that list, and an
earlier draft of this note said they were. **TX On/Off** turns transmit off for the next cycle and
lets the over already going out finish — that is deliberate, it is how WSJT-X's Enable Tx behaves,
and the button says so when you hover it. **S&P** stops a CQ run and drops what is queued behind it;
it does not cut the over on the air. Stop TX is the immediate halt on that screen, and Esc reaches
it without the mouse. On RTTY and SSTV the TX-enable button *does* cut the over in flight, because
those modes have no next cycle to wait for — which is why it is on their lists and not on Operate's.

Everything else follows from it. Whether a pane can *start* a transmission has nothing to do with
whether you can put it away — the Operating screen's Tx messages and its decode lists have always
been yours to hide, and Stop TX sits in the operating strip either way. And a pane may carry a stop
button of its own, which goes away with the pane: the voice keyer's **■ Stop** is one, and RTTY's
**Auto on** toggle is another. Those are conveniences sitting on top of the guarantee, not the
thing holding it up. Put the voice keyer away mid-message and you lose its ■ Stop — and Stop TX,
Tune and PTT are all still exactly where they were.

Separately, and because it is worth telling you rather than because anything requires it: unticking
Voice Keyer while a message is playing stops the message rather than leaving it transmitting behind
a pane you just closed, and it throws away a recording you are part-way through making. Neither is
what "hide a pane" sounds like, so the entry says both before you tick.

**Undo last change** puts the keyer away by the same door, so it now says the same thing before
you press it. (Untick the keyer, tick it back, start a recording, press Undo — the recording used
to go with no word at all.) And whichever door you came through, Nexus says what it did when it
did it: an over it cut short, or a recording it binned — so walking off the Phone screen without
opening a menu tells you too. Stopping an over used to be the silent one of the two, which was
backwards. If the recorder cannot be stopped, that is what it says, instead of reporting a discard
that did not happen.

Reaching all of this from the keyboard works the way the rest of Nexus does: every entry is a tab
stop, takes focus with an undimmed focus ring, and reads its reason aloud with the panel name —
the operator who cannot see the line under it is the one it was written for. **Esc** closes the
menu and puts focus back on the ⊞ button you opened it with, instead of dropping you at the top of
the app to tab your way back down. A panel you have torn off still reads as "popped out", but the
words no longer run into the panel's own name when a screen reader speaks it — and that tag no
longer sticks. Only the Operating screen's panels were being un-popped at start-up, so a "popped
out" recorded anywhere else stayed on the entry for good, over a panel that was in fact sitting
docked in front of you. Every screen's panels are checked at start-up now. And the menu itself no
longer runs off a short screen: with eight entries and their reasons it scrolls inside its own
box, so **Undo last change** and **Reset layout** stay where you can reach them at any window size
or UI zoom.

### Fixed: the focus outline in light mode was a hair under the readable minimum

The ring Nexus draws around whatever has keyboard focus is one colour used everywhere. In light
mode it measured 2.999:1 against the page background, where the accessibility standard for a focus
indicator asks for 3:1 — not enough to look broken, and enough to be genuinely hard to follow on a
bright screen or a laptop panel at an angle. It is the same blue, one shade darker, and it clears
the bar on the page, on panels and inside the ⊞ Panels popover. Contrast for the ring is measured
in the test suite now rather than inferred, in both themes.

### Fixed: a rotator that stops answering no longer takes the dial with it

Reported from a live pass: "I saw a first Doppler shift, then it snapped back to the none
statement", and "after 5 seconds it snaps back to 'None — leave the dial to me', even though I
didn't change it."

One cause behind both. When rotor commands stopped getting answers, the track gave up on the
rotator — correctly, rather than hammering it for the rest of the pass — but it ended the whole
pass on the way out. That ran the end-of-pass handback: your transponder went back to none,
Doppler stopped, and the picker in the Satellites section reset itself a couple of seconds later.
A track that never had a rotator was fine; a track whose rotator quit lost the radio too.

The mast and the dial are separate things now, and losing one does not surrender the other. A
rotator that stops answering is let go and nothing else changes: the pass clock, the Doppler
correction and the transponder you picked all run to a real LOS. The track stops claiming an
antenna — it drops to Doppler only and shows no commanded angles, because it is not commanding
anything — and it tells you, once, that the rotator went quiet and the pointing is yours. The sky
dome keeps showing where the bird actually is, which is what you need to turn the antenna by hand.

Losing the pass itself is unchanged: the bird setting hands your dial back, and so does the rare
case of orbital elements the propagator can no longer follow — there, the same model computes both
the pointing and the Doppler shift, so there is no correction left to make and holding your radio
would be a lie.

The rotor readout in a cockpit header keeps up with it. That strip shows a dim "ROTOR —" when a
rotator is configured but silent, and it used to show only that — so a pass that had just lost its
rotator, and was still steering your radio, went nameless everywhere outside the Satellites
section. It now shows both: the honest dash for the mast, and beside it the bird, what Doppler is
holding, and the ■ that stops the track.

At the end of a pass whose rotator went quiet, Nexus still sends one stop — the controller may be
back by then, and a stop can only ever take motion away. It does **not** run "park" or "go to
ready" for that pass: you were told the pointing was yours, so the antenna stays where you left it.
A pass that kept its rotator parks as configured, unchanged.

### Fixed: changing band or radio no longer kills your PTT

Reported from the bench: work a satellite on the Icom, come back to Phone, pick 20 m — which
hands the station back to the Yaesu — and the PTT button pressed but the radio would not key.
Nothing on screen said why. The cure the operator found was leaving the section entirely: go to
FT8, come back to Phone, and PTT worked again.

Under the hood, a band change and a radio handoff both stop transmit — they have to, or the FT8
sequencer would keep calling a station that is no longer on your band, and a switch would leave a
carrier up on the rig you just left. Stopping transmit also drops the Enable-TX latch, which is
right for FT8: the sequencer would otherwise re-arm itself on the very next slot. But in Phone,
CW and RTTY that same latch *is* the microphone, and the Phone cockpit has no switch for it — so
the latch went down and stayed down, your press was quietly discarded, and only re-entering an
operating section (which arms transmit on the way in) put it back.

Those halts now put the latch back exactly where you left it, in Phone, CW and RTTY only. The
carrier is still cut, a held PTT is still released, and the newly selected radio still comes up
unkeyed — the switch just does not take your microphone away with it. Everything it never should
have touched is untouched: FT8 still stands down on a band change, TX Off still means off across
a QSY, a tripped transmit watchdog still holds, and Stop TX still means stop in every mode.

This covers every way the active radio can change — band routing, satellite routing, band
coverage, and the radio button in the top left — plus a band change with no switch at all, and
spinning the rig's own VFO across a band edge.

Keeping the microphone across a switch means the radio loop can now be asked to transmit during
the moment a two-radio handoff is still in flight — the fraction of a second where the app is
holding the radio you just switched *away* from. It will not: nothing keys in that window, and the
same rule covers a tune carrier, a queued CW word, an RTTY over, an APRS beacon, a voice message,
an SSTV image and an FT8 slot. Most of that work is held rather than dropped and goes out on the
right radio a moment later — a mic press you are still holding, a tune, a queued CW word. What is
timed to a slot is not held: an FT8 over whose slot boundary passes during the handoff is missed
rather than sent late, which is the right answer for a mode where a late transmission is a wrong
one. Testing CAT works the same way: while the port is handed to the baud-ladder probe, the app
will not report a key it cannot actually send.

One thing deliberately does not follow you: another program sharing the radio through the CAT
broker. Your own microphone stays live across a band change because you made it; a shared client
cannot see that you moved until its next poll, so its key is refused until you arm transmit
again — exactly as before.

### Fixed: picking 160, 80 or 40 m in Phone no longer lands on a locked-out frequency

Pick 40 m from the Phone band list as an Extra and the radio landed on 7.1250 — the exact bottom
of the Extra phone band. On the low bands you transmit below the dial, so a signal there runs
2.8 kHz *under* the band edge: the app locked transmit out, the PTT button came up 🔒 TX LOCKED,
and it did it on a band you are fully licensed for. The same thing happened on 160 and 80 m, at
every license class.

The band list now parks you a full sideband clear of the edge — 7.1278 for that Extra, 7.1778 for
a General — which is the first frequency you can legally key on that band. It is the same place
switching *into* Phone has always landed; the band list was the one path that had its own idea.
The high bands are unchanged: there you transmit above the dial, so the segment start was already
clear.

### Fixed: "pin this radio" on a pass now holds

Reported from the bench: "I tried pin this radio and it goes pinned, then goes unpinned." The
switch showed 🔒 for a moment and flipped itself back to 🔓 a couple of seconds later.

Peg-lock belongs to the live radio roster, alongside which rig is active and how bands route to
it. Saving the settings form can never move any of that — a form loaded before you switched rigs
would otherwise yank you back to the old one — so the roster is put back exactly as it was after
every save. The pin on the pass rail was written as a settings save, which meant it was one of
the things being put back: the click went nowhere, the switch lit anyway, and the rail's own two-
second re-read of the setting showed the truth and turned it off again. It now goes through the
control that owns the setting, the same one behind the 🔒 in the top bar, so it sticks until you
click it again.

Worth knowing if you saw this: the pin was not merely failing to display. It never engaged, so a
transponder pick could still hand the bird to whichever rig your routing preferred, exactly the
thing you clicked to prevent.

### Fixed: a Doppler correction no longer rewrites the mode every three seconds

From an operator's CI-V trace of a live pass: 110 seconds carried 38 mode commands and 38
data-mode commands — one pair per Doppler correction — on a bus already busy with the dial, the
meters and the panadapter. On an Icom a mode write can bump the filter with it.

A Doppler step was arming the same "apply this now" flag an operator's own QSY uses, and that
path re-asserts the mode deliberately: picking CW while already on a CW frequency has to command
the radio to CW. It also clears the give-up ladders — and that was the worse half. Re-armed every
three seconds they could never fire, so a radio that cannot reach a downlink would be asked for
the whole pass instead of being given up on once.

Corrections now move the dial and nothing else. They reach the radio on exactly the same loop
pass they always did — the mode still follows a bird change, a section change or a re-arm, and
the dial still gets there every few seconds.

### Added: Lock on — put the radio back on the bird

Move the dial inside the transponder's passband and Nexus already follows you: that is you
chasing a station across the passband, and your uplink moves with you (mirrored, on an inverting
bird). Move it *outside* — by hand, or because the rig came back somewhere else — and the dial is
somewhere the pass does not describe, with no way back short of hunting for the transponder you
already had selected. **Lock on** is that way back: one click re-runs the pick you already made,
so the routing, the band, the commanded mode and both legs all come with it, and you land back in
the middle of the passband.

It sits on the **Dial** line under the bird's name, with the line that names the rig being driven,
and it is there from the moment you pick a transponder. That is the point: picking one tunes the
radio straight away, so the dial is live — and can get away from you — long before you arm a pass
and long before the bird is up. The way back is there through all of it: pick made and nothing
armed, a pass armed and waiting for AOS, and mid-pass with Doppler correcting. The one state it
stays out of is the one where it would have to guess. With no transponder picked there is nothing
to put you back onto, and picking one for you would be picking your uplink.

### Fixed: the radio mode now follows the transponder, instead of staying in FM

Working a pass, picking an FM bird and then a linear one left the radio in FM — on a linear
transponder, which is silence. Reported from a live session: "should I expect the radio modes to
change when I move to each one like in other areas? It's not; it's staying in FM."

The satellite path could force FM and could force nothing else. An FM bird (SO-50, AO-91, the
packet digipeaters) parked the rig in FM, correctly — but a linear, CW or beacon transponder
asserted no mode at all, so the radio fell back to whatever the terrestrial section policy said.
For a station whose Phone mode is FM — one station-wide setting, written whenever you tune an FM
repeater, and reset by nothing — that policy answers FM on every frequency above 29 MHz. SO-50
and RS-44 are both on 70 cm, so nothing incidental cleared it either. Picking a hand-chosen FM in
the Phone cockpit during an FM pass had the same effect and lasted longer: that choice only
expired on a band change, so it outranked every later linear pick for the rest of the session.

Now a held transponder names the mode in both directions, and a new pick re-asserts it over a
mode chosen for the previous bird. What the transponder decides is FM-versus-linear and **which
sideband**; what your operating section decides is the form — plain SSB in Phone, the DATA
submode in Digital, CW in the CW section. So FT8 through a GEO transponder still gets DATA (plain
USB there transmits no RF on a normally-wired rig).

**And you can always overrule it.** The transponder's mode comes from SatNOGS, whose vocabulary is
open and crowd-maintained, and plenty of birds get worked in ways nobody wrote down — so picking a
mode by hand in the Phone cockpit during a pass now wins for the rest of that pass, on an FM bird
as well as a linear one. The uplink stands down at the same moment rather than swapping sidebands
underneath you, which is what it already did. Picking a transponder again is you asking for that
bird's tuning back, and re-asserts it. (Previously an FM bird could not be overruled at all: it
was decided above the mode picker, which is exactly the case Nexus is most likely to have wrong.)

Two judgement calls worth stating, because they are deliberate:

- **A transponder that advertises CW is worked in USB, not CW.** Through a linear transponder you
  copy the tone inside the SSB passband — that is how it is done, and it is what everything in
  Nexus's passband and Doppler model already assumes. Commanding the rig's CW mode would make the
  displayed dial mean something rig- and menu-dependent while Doppler is steering it, and it would
  lose an inverting transponder's sideband swap entirely. A CW **beacon** is not special-cased for
  the same reasons; it copies perfectly well as an audio tone in USB.
- **A transponder that publishes an LSB downlink is now put in LSB** and its uplink mirrored
  accordingly. That was an inverted-sideband error before — the exact mistake the transponder
  machinery exists to prevent — and on an inverting bird it inverted the uplink with it.

Data downlinks (BPSK, PSK31, FT8, MFSK and the rest) are worked on the linear path as they always
were; only the FM-carried ones (AFSK, FSK, GMSK, DUV, satellite SSTV) command FM, unchanged.

### Fixed: a satellite downlink Nexus cannot name is no longer refused

"There are some frequencies that say in sat that it isn't in my band plan — you should allow me
to go to those." Correct, and the refusal was ours, not the law's. Nexus's band table stops at
23 cm, so QO-100 at 10.489 GHz — and every IC-905-class microwave bird — was declined with
"outside the band plan", on an operator running Open class with no restriction of any kind.

The band table exists to route a QSY to the right radio and to label a contact. It is not a
permission system, and it was never asked whether the tune was legal. A downlink it cannot name
now tunes like any other, and the missing label is reported as **missing** rather than guessed:
the pass rail drops its band chip but still names the rig and the frequencies, the log strip
drops the band slot instead of printing an empty one, and the logged band is left empty with the
frequency carrying the truth.

Routing still works, and works from what you actually wrote. A rule that names bands cannot claim
a dial that has no band, and no per-radio band list is consulted (asking either with a blank would
have matched every catch-all and sent the bird to whichever rig owns "everything else"). But a
rule with no band selector — "satellite work goes to the IC-9700", the shape Nexus itself
recommends — answers the same for every band and therefore for no band, so it still routes these
birds to the rig you designated. Where nothing answers, the existing fallback applies: if the
active rig cannot reach the downlink and exactly one other radio could, the pass goes to it.

Nothing about transmit permission moved. The licence-class gate is a separate check on the
transmit path and is untouched: an Open-class operator may key 10 GHz, and a Technician or
General is still locked out there exactly as before. The 0.27.0 refusal that declines an
unverifiable cross-band split on the Main/Sub Icoms is also unchanged — it deliberately fails
closed when it cannot prove both legs share one band, which is still the case above the table's
ceiling, so the band table was fixed around rather than extended.

### Fixed: a refused satellite uplink now says what to change

Both satellite split refusals were true and neither told the operator what to do about it. On
an IC-9700 holding a cross-band bird under a VFO A/B mapping, the CAT status explained that
Nexus has no verified cross-band A/B split for that radio and that nothing was written — and
stopped there, with the working layout one selector away and unnamed.

Each refusal now ends with a cure clause chosen by the radio in play, using the same rule the
CI-V daemon itself uses to decide whether it can serve a rig:

- **An IC-9700 or IC-905 on a serial/USB port.** The A/B refusal names the layout that does
  carry a cross-band uplink there — Main = downlink / Sub = uplink — and both places it can
  be picked (Settings ▸ Radio ▸ Satellite Doppler, or the mapping selector on the pass rail).
  Ask for that layout while something other than Nexus's own CI-V backend is serving the
  radio and the refusal names the switch that changes it (Settings ▸ Radio ▸ Rig Control ▸
  Native Icom CI-V), plus Test CAT for what is serving right now. It names the switch rather
  than telling you to turn it on, because it may already be on.
- **An IC-910 or IC-9100, and any radio reached over the network rather than a serial port.**
  There is no cure to name, so none is invented: the refusal says the native CI-V backend has
  no path to that radio, so no VFO mapping carries that pass's uplink there. No switch to
  hunt for, and no selector to work through one entry at a time.
- **Every other radio that can reach the Main/Sub refusal** — the mapping can be picked on
  any rig — gets the same honest dead end.

What is refused has not changed, and neither has the transmit path: these refusals still
write nothing to the radio, still transmit nothing, and still leave the receive dial being
corrected wherever it already was.

### Added: the pass rail offers the mapping that can carry the pass

Naming the working layout in the CAT status is one thing; you still had to go and set it. The
pass rail now offers it — one click, on the button that is already there.

Nexus suppresses every uplink offer once you have chosen and confirmed a mapping, so it can
never nag you into replacing your own choice. That rule is right everywhere except one place:
when the mapping you chose **provably cannot carry the pass you are on**. Then the rail shows
**switch mapping**, naming the layout that can and the radio it applies to, and the Doppler
row stops reporting an uplink it is computing and having refused every tick.

Offering a correction to a choice that cannot work is not overwriting your choice. Nothing
changes until you click:

- The offer appears **only** when the uplink was genuinely written and genuinely refused for
  this pass, on a radio where Nexus's own CI-V backend is switched on and can serve it — i.e.
  only when switching actually gets you an uplink.
- A mapping that works is **never** second-guessed. Work a V/V bird on VFO A/B on the same
  IC-9700 and the rail says nothing: that is how that pass is worked, and the split lands.
- Where no working mapping is known — an IC-910, an IC-9100, a network-connected rig, or an
  Icom whose native CI-V is switched off — nothing is offered, because there is nothing that
  one click could fix. Those stations get the refusal text above instead.

The click goes through the same confirmation path as every other mapping change, records it
for the radio the button names, and still means Nexus writes nothing to the transmit VFO you
have not consented to.

## [0.27.0] — 2026-08-02

### Fixed: the log strip no longer invites an accidental log entry

On the Phone and CW log panel, the callbook button sat in the same row as Log — and that row
wraps to fit whatever width the log column has, so Log did not stay in one place. Reaching for
a lookup mid-QSO could commit the contact instead, and several contacts got logged that way.

Log and Spot now have a row of their own, directly above the caller card and immediately under
the line that spells out what will be written ("logs to the shared logbook as SSB · 20m ·
14.200 MHz"). Read it, then commit — and Spot stays right beside Log, where it has always been.
Nothing else about logging changed: the same fields, the same Enter behaviour (Enter on a fresh
call still pulls the callbook first, then logs), the same Clear button beside the callsign.

The callbook button is also now labelled **Lookup** rather than QRZ, and is styled as the quiet
utility it is instead of a second action button. Lookup is the honest name: it asks QRZ first
and falls through to HamQTH, so it has never been QRZ-only, and it works with a free HamQTH
account and no QRZ subscription at all.
### Fixed: a satellite uplink Nexus cannot verify is now refused instead of reported applied

Hold a linear bird on an IC-9700, pick "Main = downlink, Sub = uplink" — the layout the
radio is actually built for — and the pass rail answered "the uplink was not written". It
never said why, and the mapping the operator picked was the right one for their rig.

The missing piece was which CAT backend was serving. Nexus can drive an IC-9700's satellite
mode two ways, and only one of them is wired: **Native CI-V**, where Nexus owns the CI-V port
itself, engages the rig's satellite mode, writes the uplink into the Sub band and reads every
step back off the radio before reporting it done. Native CI-V is off by default, so the usual
IC-9700 station runs on Hamlib's `rigctld` instead — and this build has no verified satellite
split there.

- **"Main = downlink, Sub = uplink" when Native CI-V is not what is serving.** Nexus now
  sends nothing at all — no split command, no transmit frequency — and the CAT status line
  says so in one sentence: Nexus drives that layout only through its own native CI-V
  backend, that backend is not what is serving this radio, nothing was written and nothing
  was transmitted, and your receive dial is still being corrected. It does not guess which
  backend *is* serving. Nexus may have started `rigctld` itself, fallen back to it when its
  own daemon failed to start, or simply attached to a `rigctld` someone else launched — and
  in that last case it has never even read the radio's model. Test CAT names the one in use.
  Before, the split went out, and a `rigctld` that answered "ok" was enough for the pass rail
  to display a transmit frequency the radio was not on.
- **Every uplink mapping that rides VFO A/B on the IC-9700, IC-910, IC-9100 and IC-905 — a
  behaviour change.** Nexus has no verified cross-band VFO A/B satellite split for these four
  radios; sending one unverified is how 0.24.2's "uplink" went nowhere. The rule is now that
  the uplink is written only where Nexus can place **both** legs of the pass on the same
  band. A cross-band pass is refused — and so is a pass it cannot place on the band plan at
  all, because then it cannot prove the two legs share a band. That second case is not
  hypothetical: the band plan stops at 23 cm, so an IC-905 microwave bird is refused even
  when both legs really are on 10 GHz. "Transmit only" is refused on these four radios for
  the same reason — that mapping never writes a receive dial, so there is no second leg to
  compare against. **V/V and U/U birds up to 23 cm keep the A/B split**: both legs on one
  band is the case A/B carries, and it still carries it. Every other radio — the FT-847,
  TS-2000, TS-790 and FT-736R class, and every ordinary HF rig — is untouched and still
  takes a cross-band uplink on VFO B as before.
- **Settings no longer pre-fills a mapping it cannot drive.** The one-click "Main = downlink,
  Sub = uplink" offer now appears only where Nexus's own CI-V engine can actually serve the
  radio — an IC-9700 or IC-905 on a serial/USB port. The IC-910 and IC-9100 are outside that
  engine's coverage in this build, and so is any radio reached over the network rather than a
  serial port, so those stations are asked instead of pre-filled. "Serial" here means exactly
  what the CI-V daemon itself means by it, so a settings file written before Nexus had a
  connection setting counts as serial, the same as it does everywhere else in the app.

Doppler keeps correcting your **receive** dial wherever it was already correcting one, so no
refusal above costs you the downlink; work the uplink from the radio's own front panel for
that pass. On the A/B side a refusal does cost something real, and it is worth being plain
about: the IC-910, IC-9100 and IC-905 used to get an uplink attempt on a cross-band pass, and
now they do not. That attempt was only ever *acknowledged* — the radio answered "ok" and
nothing in this build read the frequency back off it — so what it bought was a number on the
pass rail that may or may not have been where the radio was transmitting. Losing it is the
safer half of the trade: on a linear bird, a split you believe and the radio did not take
puts your carrier in the transponder's downlink passband, on top of everyone else working
it. (On the IC-9700 that attempt is known not to land — it is the 0.24.2 bug.) Ordinary A/B
splits — including every terrestrial pile-up "UP 5" — are untouched on every radio and on
both backends.

The Hamlib satellite recipe stays deliberately unwired rather than sent hopefully. Hamlib can
answer a frequency read-back out of its own cache, so a read-back a stale value can satisfy
cannot tell a landed uplink from an echo of what we just asked for, and confirming the
difference needs a real radio on a real `rigctld` — which neither this tree nor CI has.
Guessing wrong there puts your carrier in the transponder's downlink passband, on top of
everyone else working the bird.
### Fixed: switching modes no longer forgets your frequency

Working a station on 20 m phone at 14.240, a stray click into CW and back reset the dial to
14.225 — the start of the phone segment — and the contact was gone. Nexus now remembers the
frequency you were using, per band and per mode, for the whole session. Switch from Phone to
CW and back and you are on your own frequency again, not the segment default.

Every band keeps its own memory. Leave 20 m phone for 40 m CW and your 14.240 waits: switch
to Phone on 40 m and you get your last 40 m phone frequency (or the usual segment start if
you have not used one yet), and picking 20 m from the band dropdown brings 14.240 back. Every
mode remembers on a mode switch — Phone, CW, RTTY, and the digital Operate section — and the
band dropdowns in the Phone and CW cockpits restore the band's memory too. Anything that
names an exact frequency still goes exactly there, with no memory overlay — a typed MHz, a
band-plan channel, a spot or Needed click carrying the spot's own frequency.

The first visit to any band/mode this session behaves as it always has, and a restart starts
genuinely fresh: the memory is never saved to disk, and the frequency Nexus starts on is not
counted as one you chose — neither the dial in the settings file nor the one read off the rig
when the CAT link first opens. It cannot be: those say where the radio IS, never who put it
there, and that could as easily have been APRS, a repeater, an ISS pass or a satellite as you.
So closing Nexus parked on the national APRS channel and opening it the next morning will
never turn 144.390 into "your 2 m phone frequency". Memory starts the moment you tune
somewhere yourself. License privileges are still checked on every return — if your class
changed mid-session, the dial falls back to the legal segment start.

Turning the VFO knob counts as choosing a frequency, with two exceptions. The first dial the
CAT link reports is not a knob move at all — it only says where the rig already was, as above.
And while Nexus is holding the dial on one of its own channels — APRS, an FM repeater or
simplex channel, the ISS SSTV downlink, a satellite transponder — tuning around inside it is
working that channel, not picking a frequency for the band, so it is not remembered. Move the
knob to another band and the dial is yours again. The knob reports a frequency and nothing
else, so a frequency you reach that way comes back on the band's usual sideband rather than
the one you happened to be using on the band you left.

Two coarsenesses worth naming. An FM channel or FM-bird hold stays in force until you tune
somewhere explicitly (that is what keeps the rig in FM instead of dropping it the moment you
nudge the VFO), and while it is set any knob move at 29 MHz or above counts as the
machinery's, whichever VHF/UHF band you are on; APRS is held to 2 m only. The channel mark
Nexus sets when it tunes one of its own channels lasts the same way: it is dropped at the next
frequency you choose yourself — a band pick, a typed MHz, a mode home, or a knob move onto
another band — and until then a knob move inside that band is the channel's. It names the one
band the app parked on, so anywhere else it says nothing at all.

Frequencies the app tunes for its own machinery are never mistaken for yours, and never the
other way round either. Each part of Nexus that moves the dial says whose frequency it is at
the moment it moves it, so the answer never depends on when the question is asked:

- Work a satellite from 20 m phone and your 14.240 is still waiting when you come back.
- A pass that ends leaves the radio parked on the bird. That Doppler-corrected frequency is
  never adopted as your frequency on 2 m, however long you stay there afterwards — and
  neither is a spot you hand-tuned to inside a linear bird's passband.
- Tuning APRS from 146.520 keeps 146.520 as your 2 m phone frequency, even though 146.520 and
  144.390 are the same band and nothing about the move looks like leaving it.
- Switching radios banks the dial you were on before the handoff. The memory is station-wide:
  a recalled frequency routes to the radio that owns that band and mode, exactly as any other
  band change does.
- The ISS SSTV auto-arm's 145.800 is one of those channels too: your 2 m frequency banks on
  the way in, the 145.800 itself is never remembered, and hand-chasing the pass's Doppler
  inside the armed window does not overwrite what you were on either.

Two smaller things. Picking the band you are already on from the dropdown now keeps the
frequency you are on, instead of snapping back to the segment start. And that dropdown's
tooltip says what the control does: your last frequency on that band in that mode, else the
start of your licensed segment.
### Added: hear the pass — AOS and LOS alerts for the armed track

Arm a pass with "Work this pass" and walk away from the screen: the moment the bird rises,
Nexus now plays a rising three-tone and shows a loud popup with the facts you need at the
rotor — the bird, the azimuth it rises on, how high the pass peaks, and how long you have.
At LOS a falling tone (tellable from the rising one blind) accompanies the pass-complete
notice, which keeps saying exactly what was handed back — the dial, the uplink split, and
whether the rotor is about to park or move to ready on its own.

The alerts fire wherever you are in the app, not only with the Satellites section open —
the pass-complete notice used to be the section's alone, so working the Logbook at LOS
meant missing it. They report the track's own state transitions, never a wall-clock timer:
if the machine slept through the rise, waking mid-pass says "pass in progress" with the
minutes left instead of a stale "starting now", and waking long after LOS says nothing.
Alerts never touch the radio, the rotor, or the track — they only tell you what happened.

This is the armed track's own channel, separate from the per-pass ⏰ alarm on the schedule.
An alarm lead ("wake me 15 minutes before") still fires on its own moment; at AOS the two
channels coordinate so you never get two popups for the same rise.

The popup never steals keyboard focus and never blocks the screen. The tones are on by
default — that was the ask — with a "Pass alert sounds" switch beside the satellite
Doppler settings for operating from a quiet shack; the popups stay either way.
### Changed: the pass strip says what it ranks — "Next" and "Best 24 h", over every bird

The strip above the Satellites schedule was labelled "Next up" but ranked by pass quality,
so a spectacular pass hours away could sit above a workable one rising in ten minutes —
entries "further away than what's in the schedule", as the field report put it. It also
read only your ★ favorites, so a good pass on an unstarred bird could never surface there.

The strip is now two labelled pairs, drawn from every workable bird over your grid: "Next"
is the two soonest passes in clock order — a pass already in progress leads, marked
"already up" when it rose before the scan window — and "Best 24 h" is the two
highest-quality passes in the next 24 hours, by the same elevation-first rank the old
strip used. A pass that earns both spots shows once, under Next, and Best fills in with
the next one down. ★ marks your favorites, the mode pill ([FM voice], [Linear SSB/CW], …)
rides every classified row, and ▶ Work runs the same chain for any bird, starred or not.
Needed-grid chips stay on ★ rows — they come from the favorites schedule, which is the
only place that computes them.

The strip admits exactly the birds the "Other birds overhead" band admits — one rule, so
a bird that band refuses (reported dead or re-entered, placeholder rideshare names) never
appears in the strip either. The schedule below and the band are unchanged.
### Added: hide the countries you have stopped chasing

On a busy evening most of a waterfall is a handful of countries you worked years ago. Band
Activity gains a **Countries** button beside the filter chips: tick the ones you do not want
to see and they leave Band Activity and the Call Roster.

The list is 18 — the high-density entities an experienced chaser actually wants gone, rather
than the full DXCC table with a search box:

United States (K/W/N) · Canada (VE) · Mexico (XE) · Germany (DL) · Italy (I) · Spain (EA) ·
England (G) · France (F) · Japan (JA) · Brazil (PY) · Argentina (LU) · Poland (SP) ·
European Russia (UA) · Ukraine (UR) · Netherlands (PA) · Czech Republic (OK) ·
Slovenia (S5) · China (BY)

It is a view filter and only a view filter. Decoding, logging, the QSO sequencer and your
needed-entity alerts never see it, so nothing you have switched off can cost you a contact:

- a station **calling you** always shows, whatever its country;
- the station you are **working** stays put mid-exchange;
- **needed outranks excluded** — a new entity or a new band slot from a country you have
  hidden still comes through.

Matching is on the DXCC entity the callsign resolves to, not the letters it starts with, so
`VE3XYZ/W1` counts as United States and `W1ABC/VE3` as Canada, and hiding Germany catches
DA, DB, DJ and DK as well as DL.

While anything is hidden both panes carry a **"3 countries hidden"** chip with a Clear
beside it, so a thinned band is never a mystery — and the "N heard" and roster counts state
what is on screen, not what arrived. The Rx Frequency pane is deliberately left alone: it
answers "what is on my frequency", and a station you are not chasing sitting on top of you
is exactly what you need to see.

Your choice is remembered per station, not per window — pop out a band map and it shows the
same band as the main window.
### Added: full FT8/FT4 decoder configuration — AP on/off, CQ-only AP, single decode

The Decoder section (Settings ▸ Weak-signal digital) grows from depth-and-passband to the
full set of receive controls the built-in WSJT-X decoder actually takes. A-priori (AP)
decoding — the hypothesis-assisted passes that pull marginal replies out of the noise using
your call, the DX call and the QSO state, including the cross-cycle replay — now has its
WSJT-X "Enable AP" switch, on by default exactly as before. A second control restricts AP to
the bare CQ hypothesis, the guard WSJT-X applies by itself after five idle minutes; Nexus
makes it an explicit choice. And Single decode narrows the search to ±25 Hz around your RX
marker — the same one-station window WSJT-X uses for a double-click re-decode — to isolate
one weak station instead of working the whole passband. Single decode is FT8 and FT4 only:
50 Hz is narrower than a single JT65, Q65 or MSK144 signal, so those modes keep the full
passband whatever the switch says.

Every control is wired through to the decoder itself and proven there: tests assert at the
decode-job boundary that flipping a switch changes what the Fortran is called with, and
behavioural tests show AP-off really does silence the FT8 AP passes and the cross-cycle
replay, and that CQ-only really does cost FT4 the deep-hypothesis recoveries.
Nothing placebo. Honesty notes, stated in the UI too: the AP on/off switch is FT8-only —
FT4's decoder has no such flag (its AP is part of Normal/Deep depth), though the CQ-only
restriction applies to both; and on a WSJT-X UDP companion source, decodes arrive already
made, so none of the Decoder settings act. Defaults are untouched — leave the section alone
and the decoder behaves byte-for-byte as it did.

### Fixed: the caller card's distance and bearing now agree with QRZ

Work a station on Phone or CW and the card that appears when the call resolves reported a
bearing that did not match the one on the station's QRZ page. The maths was right — the
inputs were coarse. QRZ measures between the two stations' exact coordinates; Nexus was
measuring between the centres of grid squares, and a square is a box, not a point.

QRZ and HamQTH both hand us the station's real position in the same lookup that fills in
the name and QTH, and Nexus was throwing it away. It now keeps it, and the caller card
measures from it. Worked against a 4-character locator that is worth about a degree of
bearing and a dozen miles — W1AW from EN52 read 823 mi · 89°, and now reads 835 mi · 88°,
measured to the same pin QRZ computes its own figures from.

Nexus only accepts a position the callbook actually vouches for. QRZ returns coordinates
for every record, but tags where they came from — a real pin, a geocoded address, or a
fallback back-derived from the grid square or the DXCC entity. The last of those can sit
hundreds of miles from the station, so Nexus uses only the first two and falls back to the
locator otherwise. A station with no position on file is unchanged.

The larger half of the gap is your own grid. Everything is measured from where you say you
are, and a 4-character locator says only "somewhere in this ~100-mile square" — so Nexus
measures from the middle of it. On a DX path that costs about a degree, but on a station a
couple of hours away it is worth up to ~30°, whatever the other end does, and it puts every
distance out by up to ~60 miles at any range. The setup wizard used to tell you 4 characters
was plenty. It no longer does, Settings says the same, and the caller card's tooltip now
names whichever side is still a square instead of presenting the number as exact. Setting a
6-character grid closes it.

Distance is unchanged in method (great-circle, statute miles) and gains the same precision.
The Operate roster's bearing column and the map are untouched. So is rotator pointing, which
answers a deliberately different question — "point at this callsign's DXCC entity" — and
still swings to the entity's nominal centre, not to the station. On DX that is within a
beamwidth; for a stateside contact the two numbers are not meant to agree.
### Changed: the displays answer the audio — waterfalls, scopes and meters respond faster

The voice/CW/RTTY/SSTV displays lagged the ear. You heard a signal and the screen answered
late and rounded-off: the waterfall took a third of a second to bring a new signal to full
brightness, the RTTY and SSTV waterfalls threw away five of every six spectrum rows, and
both meters answered roughly half a second after the audio. Nothing here adds animation or
invented motion — every change removes real delay between the antenna and the pixels.

- The spectrum's analysis window is half as long. A key-down or a voice syllable reaches
  full brightness in ~170 ms instead of ~340 ms, and short CW elements stop blurring
  together. Nothing visible is lost: the finer raw resolution the long window bought was
  below what the display can draw, and computing the shorter window costs less CPU.
- The RTTY cockpit and the SSTV band waterfall scroll at 20 rows per second — the same
  cadence the Phone/CW rig scope already runs — instead of 8. The FT8/FT4 waterfall is
  unchanged: those modes are slot-synchronous and their display is right as it is.
- The RX audio meter has real instrument ballistics: it snaps up with the audio (90% of a
  step within ~60 ms) and falls smoothly, like a hardware S-meter. It used to smooth the
  attack and the decay equally, so the needle registered a signal ~150–300 ms late.
- Both meters read through a new fast lane, polled every 100 ms, that no longer rides the
  radio loop or the 300 ms status poll — so a slow CAT read can no longer freeze the
  needles for seconds at a time, and a meter whose readings stop arriving falls back to
  rest within about two-thirds of a second instead of holding a stale needle. On a
  healthy link the CAT S-meter is also re-read every 360 ms instead of every 750 ms —
  twice as fresh, without crowding the link the radio dial shares. Slow serial links
  keep the gentler cadence; that limit is the radio's, not the app's.
- The meter bars no longer ease their width in CSS. The eased bar smeared every real
  reading by another 80–120 ms; color changes still ease, measurements do not.
### Fixed: "TLE 26 days — STALE" on a catalog that is hours old

The Satellites screen carried an amber "TLE 26 days — STALE · refresh" chip, a warning
chip in the status bar on every screen, and a Settings line reading "The element mirror
is unreachable and your elements are 26 d old — import a fresh element file or retry
later." None of it was true, and there was nothing to do about it.

Every one of those surfaces was reading the age of the single oldest satellite in the
catalog and presenting it as the age of your elements. The catalog is 367 birds, and the
typical one is a few hours old — but a handful are legitimately old. AO-7 launched in
1974 and gets fresh elements when someone observes it; a few dozen more are re-observed
every few weeks rather than every day. One of those set the number for all of them.

Worse, the number could not come down. A satellite whose elements pass 30 days is
dropped from the set, so the oldest one still counted is always just under 30 — as one
aged out, the next in line inherited the badge. The chip was going to read stale forever
no matter how fresh your elements were, and the calm "your elements are current" message
the app already had could never appear.

The chip, the status-bar chip, the Settings line and the Connect Passes badge now report
the median age of the elements they draw on: what the typical satellite in your catalog
carries, not what the slowest-observed one does. On the catalog shipped with this
release that reads a fraction of a day. A set that genuinely goes stale still says so —
once more than half the birds pass the 14-day line the median goes with them.

Satellites held back by the 30-day ceiling are counted and reported in their own right,
so "my catalog is current and a few birds sit out" no longer reads identically to "my
whole set has gone stale". The Satellites header states the number beside the age ("367
birds · 30 sit out past 30 d"); Settings ▸ Radio ▸ Orbital elements carries it on the
line that is always there, not only while a refresh has failed; and every refresh result
— landed, blocked or failed — accounts for those birds.

Satellites in the 14-to-30-day band are counted too: still used, still drifting. A
median can hide that band, and this is the shape it hides — half your birds sitting at
29 days while the other half arrived this morning reads as a current catalog, because
the typical bird is one. The Satellites header carries both counts whenever they are not
zero, and when most of the elements you hold are past the 14-day line the status bar and
the Connect Passes pane say so as well. A slow-cadence tail on an otherwise current
catalog stays quiet, which is what the shipped catalog looks like.

The Satellites section always has a way to refresh elements. The amber chip was also the
section's refresh button, and it correctly disappears now that the reading is honest, so
a quiet "⟳ refresh elements" chip sits in the header whenever the amber one does not.

Unchanged: the per-satellite rules. A satellite past 30 days is still refused by name and
age when you arm it, arming a satellite with elements past 14 days still asks first, the
Birds list still explains every excluded bird individually, and a fresh bird still arms
cleanly no matter how old anything else in the catalog is.
### Changed: the sky dome carries azimuth and elevation on the satellite

Working a pass with a manual rotator means reading the azimuth and elevation off the
screen and turning the mast to match. Both numbers lived in a tooltip, so following the
pass meant holding the mouse on a moving dot to see where to point next.

Azimuth and elevation now ride on the satellite and stay there for the whole pass. They
sit on a plate that holds them legible over the pass track, and they move to the other
side of the satellite near the edge of the dome or when the antenna marker is on that
side — so they never run off the dome, and never cover the gap between where the antenna
is pointed and where the satellite is.

The satellite is drawn as a spacecraft — body and solar panels — rather than a plain dot,
and matches the mark the world map already uses for the same bird. The text readout under
the dome and the hover tooltip are unchanged.

### Changed: the rise and set marks on the sky dome carry their bearings

The two triangles on the rim of the dome are where a pass starts and ends, and they are
what you pre-point a manual rotator by. Their bearings lived in a tooltip. The set mark is
drawn as an outline rather than a solid triangle, and an outline answers the mouse only on
the line itself — so on that one, catching the tooltip meant landing the pointer on a
hairline.

Both marks now print their bearing on the dome: the rise or set bearing with its compass
point, on a plate beside the triangle it belongs to. Each plate names which mark it is, in
words and with the same up and down arrows the readout under the dome uses, so a pass that
rises and sets close together never leaves two bare numbers you have to tell apart.

The plates are azimuth only. Both marks sit on the horizon, so an elevation there would be
restating the picture rather than reporting anything.

A plate sits inside the dome next to its triangle and steps aside when that spot is taken —
it never covers either triangle, the satellite, the satellite's own readout, or the antenna
marker, and it stays inside the horizon at every bearing. The whole body of the set mark is
now hoverable, so its tooltip, which also carries the time, is reachable without threading
the mouse onto a line.
### Added: the satellite surfaces say how high the bird is

The Birds list, the world map's satellite hover and the sky dome each show the bird's
altitude — how far above the earth it is, right now.

It is the number that tells you what kind of pass you are about to work before you open
anything: a 630 km LEO screams over in ten minutes with the Doppler running away from
you, while an elliptical bird near apogee loiters for the better part of an hour with a
shift that barely moves. Down the Birds list the figures line up in a column, so a low
circular bird and a high elliptical one tell themselves apart at a glance.

Altitude is always labelled, and on the sky dome it sits directly under the range it is
easily confused with — range is how far the bird is from you, altitude is how far it is
from the ground. The dome's number is recomputed with the rest of the pass geometry every
three seconds, because on an elliptical orbit it genuinely moves.

A bird nothing carries current elements for shows no altitude rather than a zero, and
neither does an armed pass before the bird rises: there is no position computed yet, and
0 km would put a satellite on the ground.
### Fixed: the starter favorites now cover the birds you can actually work

The one-time favorites seed ranked every bird on pass count, and pass count is mostly a
statement about altitude. FO-29 flies an 800 × 1320 km ellipse at 13.5 orbits a day; a
480 km cubesat flies 15.3, so the cubesat shows more passes over your grid every day of
the year. Run the app's own pass predictor over the catalog it ships with — 24-hour
windows from six grids at four different start times — and the old ten came out 3 to 9
beacon-only telemetry cubesats out of ten, 6.5 on average: birds you can hear and cannot
work. That is the shape of the catalog rather than bad luck. 305 of the 367 birds that
carry elements have a downlink and no uplink, and 18 of the top 20 by orbital rate are
among them, so a pass-count ranking fills its ten mostly from birds nothing can be
worked on.

What happened to FO-29 is the other half of it. Across those same 24 runs it ranked
anywhere from 2nd to 81st, and it reached the top ten in 6 of them; some SSB/CW
transponder reached the ten in 14 of 24. Ranked on passes alone, whether you got a
workable bird at all came down to the geometry of the day you first opened Satellites.

The catalog now records what each bird is worked WITH — an FM voice repeater, a linear
(SSB/CW) transponder, a digital/packet channel, or a beacon downlink and nothing else —
and the seed fills its ten by rotating through those three workable kinds, each one
offering its own best bird under the same pass-count-and-elevation rank as before. A
beacon-only bird takes a slot only after the three are exhausted. On today's catalog that
is four linear, three FM and three digital — at every one of those 24 grid-and-window
measurements, with no beacon-only bird in any of them.

Nothing else about the seed changed. It still runs once ever, still never seeds over
anyone who has stars or ever had them, still requires a bird to be alive, to carry a live
amateur transmitter, to hold current elements and to fly a workable pass over your grid,
and still waits rather than spending itself when your grid or the catalog is not there
yet. A bird can hold more than one kind at once, which 60 of today's 372 active amateur
birds do — QO-100 carries a linear transponder, digital segments and a beacon — and each
one counts wherever it belongs.

Existing stars are untouched. This affects a first run only.

### Fixed: satellite SSTV puts the radio in FM

Clicking an SSTV row on the ISS — 145.800 "Mode V Imaging", 437.800 or 437.550 "Mode U -
SSTV" — set the radio to USB. Every SSTV downlink in orbit is narrowband FM, so the
picture arrived garbled. Satellite SSTV rows now set FM. HF SSTV is unaffected: it really
is an SSB mode, and no satellite transmits SSTV below 30 MHz.

### Known: a satellite that publishes an LSB downlink is still tuned USB

FO-82 (NORAD 40320) declares its transponder downlink as LSB. Picking it sets the radio to
USB and the uplink is then derived from that, so both ends come out on the wrong sideband.
It is one bird, and the fix belongs with the wider transponder-tuning work rather than
here.
### Added: other birds, without leaving your schedule

The Satellites schedule shows your ★ favorites, and the Birds list names every bird — but
nothing connected the two. Finding a workable pass on a bird you had not starred meant a
trip to an alphabetical name list that carries no pass information at all.

The schedule now carries a second, collapsed band under your favorites: "Other birds
overhead · N workable · 24 h". The count is live. Expand it and you get one row per bird —
its best pass in the next 24 hours, ranked by how workable it is — with the same star and
▶ Work controls your own rows have. Star a row and the bird moves up into your schedule,
gaining the needed-grid chips, the pass alarm and the 48-hour view. Collapsed, the band
adds nothing to the screen: your schedule is exactly what it was. Twelve rows show by
default, with "show all N" for the rest; placeholder rideshare objects that share a name
with their siblings are left to the Birds list, where starring one cannot star six.

With no favorites starred at all, the section no longer replaces the whole planning column
with one sentence — the schedule renders with the band open, so the answer is on screen
from the first visit.

Birds' mode class shows as a pill beside the name ([FM voice], [Linear SSB/CW], [Digital],
[Beacon]) on schedule and discovery rows, wherever the catalog can say — no class, no
pill, never a guess.

### Added: a way back out of a bird's detail

Opening a bird pinned its detail to the side column for the rest of the session — no
close, no Escape, and the Birds list starts more than a full screen below it. The detail
heading is now sticky with a ✕, Escape closes it too, and the tracking badge in the header
opens the tracked bird again with one click. Closing a detail never touches the track, the
rotor or the dial.

### Fixed: a pass already in progress reported a wrong rise time

The Satellites view's 24-hour pass scan started at "now", so a bird already above the
horizon was reported as rising at the moment you looked, with its peak elevation
understated to what remained — and the cached scan repeated that answer for up to ten
minutes. The scan now looks back six hours, the same way the schedule always has: a pass
in progress keeps its real rise time and its full peak. This also corrects the map view's
pass pane and the first-run favorites seeding, which read the same scan.
### Changed: the FT8 Classic view puts the decode window where you operate

Field feedback from an advanced DX operator, running Nexus beside WSJT-X on an
ultrawide: the Rx Frequency pane — the stream you actually run a QSO from — was a
small box at the bottom of the side rail, and too much of the screen went to chrome.

The Classic layout is now three columns: Band Activity on the left, a full-height
Rx Frequency pane beside it with the Tx1–Tx6 message machine docked underneath —
the same bottom-right geometry as WSJT-X — and the Stations roster on the right.
Your own transmissions appear in the Rx Frequency stream in yellow, interleaved
with the answers, so you click a call and watch the whole exchange line by line.
Clicking a decode there does exactly what it does in Band Activity. A drag handle
between the pane column and the roster lets you tune the balance; the split is
remembered.

The chrome got out of the way. The status line, the QSO panel's three rows and the
permanent TX-meters line are now ONE compact strip under the waterfall. Call CQ /
S&P and TX On / Tune / Stop TX / Hold Tx come first, anchored at the strip's left
edge, and they hold the same position whether the strip is idle, mid-QSO or
transmitting — the DX call appearing or the rig keying never moves Stop TX. After
the buttons: the TX state and sequencer readout with the DX call and report, what
is being sent now, the free text box (sized to the 13 characters it can actually
hold), and — moved into the strip from the old status line — TX AUTO / Skip Tx1
and the next-slot countdown, plus the SWR/ALC/Po/COMP meters in a fixed-width cell
that shows live bars while transmitting and the dimmed last readings between
overs, so nothing jumps when transmit starts. Every control that was on screen
before is still on screen, always visible, just smaller and closer together. The
header packs into one row on wide screens: the DXpedition mode is a dropdown,
Spot and Pop out are icon buttons, Record QSO is a dot, and the memory chips keep
to a single line.

At ultrawide half-screen the strip is a single row and the Rx Frequency pane
grows from roughly 15 visible decode lines to over 60; Band Activity and the
roster both gain height. On a 1366x768 laptop the three columns hold and the
strip wraps at a fixed point into two stable rows — buttons and readout above,
sending/free-text/meters below; on narrow windows everything stacks in one
column as before.
### Changed: Doppler corrects your downlink without being asked

Arming a pass and picking a transponder is the whole ask. Nexus corrects the receive dial
for the pass from that moment, with nothing to switch on first.

Before this, two settings stood between an armed pass and a moving dial — Satellite
Doppler, and the VFO mapping — and both shipped off. A station that had never opened
Settings ▸ Radio armed a pass, held a transponder, and watched the dial sit still with
nothing on screen saying why. Staying on an SSB signal as it walks several kHz across a
70 cm pass is most of what Doppler correction is for, and it was behind two switches.

The two switches were never the same kind of thing, and only one of them is kept as a
precondition:

- **The downlink is automatic.** Correcting the receive dial cannot transmit. The worst
  case is that you do not hear the bird, so it asks for nothing.
- **The uplink is still confirmed, once per radio.** A wrong VFO mapping transmits on
  your own downlink — into the satellite's output passband, on top of everyone working
  the bird — so nothing reaches your transmit VFO until you have said which VFO carries
  the uplink on the radio in use.

The confirmation now happens where you already are. On the pass readiness rail, the
Doppler row reads what your radio can do and offers it in plain words — "Confirm the
uplink and Doppler drives IC-9700 as Main = downlink, Sub = uplink" — with one button and
the mapping list beside it if the derived answer is wrong. Confirm it once and that radio
is never asked again.

Where the answer is not certain, Nexus asks instead of guessing. An IC-9700, IC-910,
IC-9100 or IC-905 running on Nexus's own CI-V connection has exactly one full-duplex
layout it can express, and that is the one offered. A full-duplex radio Nexus drives
through Hamlib, an FT-847, FT-736R, TS-2000 or TS-790, or a radio it cannot identify gets
the question, not a pre-filled answer. A single-VFO radio is offered nothing at all: its
downlink is corrected and the transmit dial stays yours.

The confirmation is recorded per radio because the satellite path routes. A pass can hand
the QSY to whichever rig covers the band, and peg-lock or a mid-pass handoff can change
which radio is under the split — so the uplink is driven only on a radio you confirmed,
re-checked every correction rather than assumed at arm time.

**If you were already correcting the uplink — Satellite Doppler on, a VFO mapping set —
nothing changes for you.** The mapping is kept exactly as written, it is not re-derived,
and you are not asked to confirm it again on any radio your station had when you
upgraded: the old station-wide grant is recorded as a confirmation for each of those
radios, and it survives every save and relaunch. A radio you add later gets its own
one-time confirmation, like any second radio. If you had picked a mapping but never
turned the old Satellite Doppler switch on, that pair never tuned anything — so the
mapping is kept, your downlink now corrects automatically, and the transmit VFO waits for
the same one-per-radio confirmation a fresh install gets. Nothing reaches a transmit VFO
on upgrade that was not already being driven before it.

**Satellite Doppler is still a switch you can turn off** — Settings ▸ Radio ▸ Satellite
Doppler — and off still means off, both legs, no dial and no split. What is gone is
having it off by default. Non-satellite stations are untouched either way: the correction
runs only inside a pass you armed, on a transponder you are holding, on a dial that pass
owns.

The pass rail now says which legs are actually being driven, separately — downlink
corrected with the transmit VFO still yours reads as exactly that, and the Doppler
readout shows an uplink frequency only when there is an uplink being written. That
honesty extends to the birds with nothing to split: a one-channel (simplex) bird rides
one dial and the rail says so instead of offering an uplink confirmation the pass cannot
use, and a beacon — downlink only — never puts anything on the transmit VFO. A radio you
remove takes its uplink confirmation with it (a replacement radio starts unconfirmed),
config profiles carry neither the VFO mapping nor the confirmations between stations,
and an operator who answers "Downlink only" is not asked again.

The confirmation itself has one writer. Picking a mapping — on the pass rail or in
Settings ▸ Radio — applies immediately and is recorded for the radio in play at that
moment; a Settings window left open across radio changes cannot re-point the mapping or
revive a removed radio's confirmation when you later press Save. When your chosen
mapping is not yet confirmed for the radio under the split — a second rig, or an
upgraded uplink-only station — the rail's confirm button offers exactly that mapping for
exactly that radio; it never swaps in a derived one over your choice. And every surface
that names who owns a frequency keys on what is actually driven: under an uplink-only
mapping the tracking badge, the rotor strip and the end-of-pass notice say the transmit
(split) VFO is Doppler's and the dial stays yours, instead of claiming a dial that never
moved.

### Known limitations

- Multi-window per-radio profiles keep their files under a directory named by the radio's
  internal id, and ids are reused: remove a radio and add another, and the new radio can
  inherit the removed radio's window settings, geometry and journals. Pre-existing and
  unchanged in this release — recorded here so it is not mistaken for a new fault. The
  uplink confirmation is NOT affected: it is pruned when a radio is removed, so a
  replacement radio always starts unconfirmed.

## [0.25.0] — 2026-08-01

### Fixed: torn-off windows open at a readable size

Pop out the CW band map and the type was tiny — and stayed tiny at every size the window
could be dragged to, docked or free. Every torn-off window sized itself by asking whether
the full Operate cockpit fit inside it. A 420-pixel-wide band-map strip never fits a
cockpit, so the answer was always "no" and every pop-out opened at the smallest scale the
app has, permanently.

Each pop-out is now measured against its own contents instead. The band map, the
waterfall strip, the Needed board, Connect, DXpeditions, Memories, Satellites and the
Field Day scoreboard all open at full size, and shrink only when you actually squash the
window down toward its minimum — where shrinking is the right answer. The Operate pop-out
is unchanged: it really does host the cockpit, so the cockpit is the right question for
it. The main window is unchanged at every size.

If you have pinned a scale in Settings, that pin now applies in a pop-out as far as the
window can take it, instead of being crushed to the smallest step. A pin is still never
rewritten by a pop-out, and still never enlarged past what the window can show.

The Needed board no longer carries its 25% font compensation — it existed only to offset
the scale bug, and the board now reads larger than before without it.

### Added: the satellite catalog ships with the app

A new install used to start with nothing and go looking for orbital elements. If the
element mirror could not be reached, the fallback fetch brought back CelesTrak's
`amateur` group — 97 objects, no status information — so the Satellites screen listed a
fraction of the birds and could not say a word about any of them: nothing marked dead,
re-entered or not yet launched, nothing marked alive but silent, no row explaining a
bird it holds no elements for, and a starred bird that stopped working simply vanished
from the list with the star still set.

The installer now carries a snapshot of the full catalog — 430 birds, 367 of them with
elements — so the Satellites screen is complete from the first launch, with no network
at all. The snapshot is a floor, never a ceiling: it fills in what your copy is missing
and never replaces newer elements with its own, never overwrites the statuses you
already have, and never counts as a fetch, so Nexus still refreshes from the mirror at
the first opportunity exactly as before. Upgrading from an earlier version keeps
everything already cached and gains the catalog beside it. Elements from the snapshot
age like any other: past 30 days they stop being used, because SGP4 accuracy is gone by
then and a position would be a guess.

Satellite population, names and status are derived from the SatNOGS DB (CC BY-SA 4.0);
orbital elements are courtesy of CelesTrak and the SatNOGS TLE API.

### Fixed: SSTV receives pictures without being told to, and says what it is hearing

You could hear a picture coming in on the speaker, watch it march up the waterfall, and
still get nothing — no image, no progress, no explanation. Opening the SSTV screen now
starts the receiver. Until now it had to be armed by hand every session, and until it
was, the decoder was handed no audio at all, so the ordinary way to use SSTV (open the
screen, tune 14.230, wait) decoded nothing. The waterfall and the decoder are fed by two
different paths, which is why a live waterfall never proved the decoder was being fed —
and why the failure was invisible. Stopping the receiver yourself is still respected: it
stays stopped for the rest of the session, and nothing here can transmit.

The line under the waterfall now states what the receiver actually hears rather than
one fixed hint. It tells apart a stopped receiver, a capture device delivering nothing,
an input that is alive but silent, a picture in progress, and — new — a station sending
in a mode Nexus cannot decode, which previously looked exactly like a dead band because
it was only ever written to a console log. Hearing the signal on the speaker says
nothing about what the app is capturing, and the screen now says so instead of leaving
you to guess.

**More SSTV frequencies, and the right one.** The band list gained 160, 17, 12 and 6 m,
the European 40 m calling frequency, and the 20 m overflow channels 14.233 and 14.236 —
the last of which is where the original report came from, and which existed nowhere in
the app. The on-screen hint now names the calling frequency for the band you are on
instead of reciting 14.230 and 145.800 whatever you are tuned to. While a picture is
coming in, the caption says how long that mode takes on the air, so a 110-second
Scottie 1 no longer looks like a hang.

Three more things that came out of reviewing the fix. An ISS pass ending no longer
leaves SSTV switched off for the rest of the session — the automatic disarm at loss of
signal was indistinguishable from you pressing Stop, so anyone using the ISS auto-arm
option would have lost SSTV decoding from their first pass of the day onward. The new
status line no longer blames your sound card in the second before the decoder has
reported anything; until it has heard something it says so plainly. And 28.680 is no
longer described as Technician-accessible: US Technicians have 10 m images only on
28.300–28.500.

### Fixed: the map stopped throwing away its canvas every second

On a wide display Nexus's memory use oscillated by ~20 MB once a second — visible in
Task Manager as a steady sawtooth. The map's draw pass reassigned the canvas size on
every run, and assigning that size discards the whole image buffer and allocates a
fresh one even when nothing about the size changed. Since the draw pass runs on a
one-second pulse whenever an animated layer is showing (band openings and the heat
layer are on by default), a full-window buffer — about 20 MB at 3440x1440 — was
thrown away and rebuilt every tick. Nothing leaked; the memory was reclaimed each
time. But it was work for no result, and on a slower machine that kind of churn is
felt as stutter rather than seen in a graph.

The map now resizes its canvas only when the size genuinely changes, matching what
the waterfall has always done. The flare overlay got the same treatment (it was
rebuilding a full-window buffer on every zoom and drag), as did the small spectrum
display in Connect and Settings, which was doing it eight times a second. A test now
fails the build if any drawing surface goes back to resizing on every frame.

### Added: satellite operating without the box — and passes that know their worth

**No rotator? Tracking still works.** Arming a pass no longer needs a rotator: the pass clock, the
sky dome and Doppler all run, and only the pointing is skipped — the handheld-antenna operator
gets everything but the mast, including where to swing the antenna. The tracking badge, the
readiness rail and the rotor strip say exactly which surfaces a track actually drives — rotor,
dial, both, or neither — and a track driving neither says "pass timing only" instead of implying
more. While a rotor-less pass holds the dial, an ownership chip appears in the cockpit header
naming the bird, with a stop button that hands the dial straight back; stopping a live track
always releases the dial.

**The uplink's sideband is set with its frequency.** On an inverting bird you listen USB and
transmit LSB. Nexus now puts the TX (split) VFO in the right sideband along with its frequency
while a pass owns the uplink — commanded once per answer, only when the two legs genuinely differ,
and only onto the satellite's own uplink: a terrestrial pile-up split worked while a transponder
is held keeps its own sideband. Reach for the rig's mode knob yourself and Nexus stands down for
the rest of the pass rather than fighting you. If the rig refuses the command, the status line
says which sideband to set by hand — a wrong uplink sideband sounds exactly like nobody answering.
The Doppler readout and the transponder chooser show what is actually being commanded, and say so
plainly when nothing is.

**Passes ranked by what they would earn you.** The schedule and the "Next up" strip now say what
each pass is worth: the grids you have never worked via satellite and the entities you have never
worked at all, wherever the pass's footprint crosses them — in the same need-chip language as the
Needed board, with the sample squares on the pass timeline. "Needed" is a column you click to sort
by; the default order stays soonest-first, and a bird SatNOGS reports dead still shows its dead
tag here. Satellite VUCC is now its own card on the Awards screen — ARRL counts a satellite
contact toward that category only — so the terrestrial VUCC card counts terrestrial grids, and a
satellite QSO no longer silences a NEW GRID call-out for a band slot that is genuinely still open.

**One click to work a pass.** ▶ Work this pass opens the bird, picks a workable transponder for
you — never a beacon, never one reported dead, and never overriding a "None — leave the dial to
me" you said for that bird — and arms the track. A readiness rail under the bird's name shows the
whole chain (pass, rotor, transponder, Doppler), each gate fixable where you are standing. The
transponder chooser is a card list beside the tuning instruments with dead entries folded behind
one line, and the sky dome, timeline, chooser and passband strip sit together above the globe.
What the rail and chooser show is what the engine actually holds: when a pass ends or is stopped,
the hold is released and the display follows.

**Picking a transponder tunes the radio.** Choosing a bird — by hand or through ▶ Work this pass —
puts the rig on that transponder's downlink there and then, with the uplink on the transmit VFO
your mapping calls for. No waiting for AOS: the pass takes over from where the pick left the radio.
The click is consent for the dial only, exactly as clicking a spot or a repeater favourite already
is; every transmit gate is unchanged, and "None — leave the dial to me" still means the radio does
not move. If Doppler is switched off, your mapping is None, or the rig cannot reach the band, the
transponder is still held and the section tells you plainly that nothing was tuned.

**The Satellites section says which radio it drives.** A dual-radio station no longer has to guess.
Picking a transponder routes on band and mode class the same way a repeater tune does, so a
VHF/UHF bird reaches the VHF/UHF rig even when you were sitting on HF, and an FM bird follows your
FM routing rule while a linear bird follows your SSB one. A line under the held bird's name names
the rig, the band and the class it routed on, and the frequencies it actually wrote — with a pin
to hold the current radio when you want to override the routing.

**A routing rule can now say "Satellite".** Asked for from the field: mode-class routing sends a
packet bird through your FM & APRS rule — right by the rules, wrong for the shack where APRS
lives on one rig and the satellite antennas on another. The rule editor's mode dropdown gains
**Satellite**: a rule so designated is checked before the mode rules, but only when a satellite
tune is asking — picking a transponder finds it, a terrestrial FM, APRS or FT8 tune never does.
Without a Satellite rule nothing changes: satellite picks keep routing on band and mode class
exactly as before. One caution: builds before this one don't know the designation and read a
Satellite rule as a plain terrestrial rule — with no band or mode set, one that catches every
tune — so if you ever roll back, delete the Satellite rule first.

**No more FT8 frequencies under Satellites.** The band dropdown at the top of the window is fed the
digital watering holes, which meant it offered 14.074 beside a bird on 435 MHz. Satellites now owns
its own frequency surfaces, like Phone, CW, RTTY, SSTV and APRS already do.

**Orbital elements stay current — and say so when they can't.** Elements now refresh in the
background from the project's mirror of CelesTrak's amateur list, held in one shared on-disk
snapshot: no more re-fetch on every launch, nothing ever waits on the network, and CelesTrak
itself is asked directly only as a narrow, rate-respecting fallback. The stale badge is now a
refresh button, and Settings gains an Orbital elements block with **Update now** and **Import
from file** — the path for offline shacks and brand-new launches. Update now means now: if the
mirror can't deliver when you press it, the same attempt goes straight to CelesTrak instead of
telling you to come back tomorrow — still honoring CelesTrak's update cycle, and never after
CelesTrak has said stop. And the answer speaks operator, not protocol: a fetch that fell through
to CelesTrak says so with the bird count, an unreachable mirror over current elements says your
elements are fine (the mirror goes live with the next site release), and a real failure says
what failed and what to do next — no more "HTTP 404" as the answer to a button press, with the
raw error riding the tooltip for troubleshooting. Element age is enforced where it matters: past
14 days arming a pass asks first; past 30 days the detail pane, tracking and the
SSTV auto-arm refuse plainly, naming the bird and the age, instead of pointing the antenna with a
fiction. A pass keeps the elements it armed with (their age shown on the readiness rail), a bad
or empty download never replaces a good cache, and a CelesTrak rename no longer orphans your
starred birds, alarms or schedule — Nexus remembers the catalog number behind each name.

**Connect's satellite layer follows your stars.** Turning on satellites in Connect now shows the
birds you have starred in the Satellites section — on the 2-D map, the 3-D globe and the Passes
pane alike — instead of the whole amateur catalog. A small ★/All chip flips between your birds
and everything — on the Passes pane and beside Satellites in the map and globe Layers panels —
one choice all three surfaces share and apply the moment you flip it, and with no stars set
the full sky still shows so a fresh install is never empty. If every starred bird has dropped
out of the current elements, the map and globe say so rather than drawing a silently blank
sky. Every bird on the map and globe now
carries its designation ("ISS", "RS-44") beside its satellite icon, and map labels that would
overprint each other in a cluster shuffle apart so both names stay readable. Stars recognized by
catalog number too, so an upstream rename never drops a bird from your filtered sky.

**The bird list is the birds that are actually up there — and it says when one stops being one.**
Nexus used to show one Celestrak list of 97 objects, and measured against AMSAT's live reports
roughly 60 of those had not been heard in a month, while eight birds heard on the air that same
day were missing from it entirely (IO-86, Foresail-1p, SAKHACUBE, QMR-KWT 2, Ten-Koh 2, Marina and
two more). The list is now built the other way round: start from the satellites the SatNOGS
database says carry an amateur transmitter, then go and find current orbital elements for the ones
that are actually workable — from the two Celestrak groups and the SatNOGS element service
together. That is **430 birds listed and 367 with live elements**, against 97 before: 274 of them
are birds the old Celestrak group never carried at all. Birds catalogued under a placeholder
number until Space Command assigns a real one (normal for the first months after launch) now
appear under one entry instead of two or none.

Every bird carries its status with it, so the Birds list, the map and the globe can show what only
the Satellites schedule could show before — and a bird that stops being workable keeps its row and
says why: **dead**, **re-entered**, **pre-launch**, or **alive but silent** when nothing amateur is
transmitting on it any more. A starred bird that dies no longer disappears out from under your ★;
it stays in the list, wearing the reason, until you take the star off yourself. Birds nobody holds
current elements for are reported the same way — no elements at all, elements too old to trust, or
an orbit decayed past the point where a position can be computed. The Connect Passes pane names
those birds under the passes, and searching the Birds list reaches the whole catalog, elements or
not, so a bird you unstar is always a bird you can find again. A bird that re-entered more than
six months ago drops off: by then it is history, not a catalog.

**And your first ★ birds are set for you — once.** A fresh install used to open Satellites on an
empty schedule beside a list of hundreds of birds, with nothing to say which are worth chasing.
Nexus now stars the ten most workable active birds over your grid the first time it can: alive,
carrying a live amateur transmitter, with current elements, and flying real passes over you —
ranked by how many of those passes you get and how high they climb. A line at the top of the
section says it happened and disappears when you dismiss it, and every star it set is one you turn
off where you are standing. It happens once. Clear the whole set and you get an empty sky, not the
same ten back next launch; a station that already has favourites is never seeded over, and neither
is one that deliberately cleared theirs — including one that cleared them on an older version.
With no grid square set, or before the bird catalog has landed, nothing is starred at all: a guess
is worse than waiting.

The mirror that feeds all this refreshes every six hours and refuses to publish at all if any of
its sanity checks fail, so a bad upstream day leaves your last good list in place rather than
replacing it with a short one. If the mirror itself is unreachable for a day, Nexus falls back to
fetching Celestrak's amateur group directly — a shorter list, and every bird it does not carry
keeps its row, marked as having no current elements.

### Fixed: the pick reaches the radio — and the IC-9700's uplink goes where it can transmit

**A dual-radio pick finds the satellite rig even with nothing configured.** Field report: on an
FTdx10 + IC-9700 station, clicking a transponder moved nothing — no routing rule named the 9700
for the band, neither rig listed bands, and the pick stopped at "this radio doesn't cover
435 MHz" while the 9700 sat idle. When no rule or band list answers but the active rig is known
unable to reach the downlink and exactly one other radio exists, the pick now hands the QSY to
that radio. With several candidates and nothing naming one, the section refuses and says exactly
what to configure instead of guessing which rig to move. A radio whose band list explicitly
leaves the downlink's band out is never the fallback — that list is your word that the rig
doesn't take the band, and the pick refuses rather than switching to it just to be turned down.

**A packet bird is FM to the radio — and routes like it.** Field report: on the same station,
picking the ISS APRS digipeater (145.825 up *and* down) selected the HF rig and called the bird
SSB. The mode class behind routing recognised only the literal names "FM"/"FMN", so every packet
mode SatNOGS uses — AFSK, FSK, GMSK and the rest of the family — fell through to the SSB class:
the FM routing rule never matched, and USB would have been commanded on an FM channel. One
mode-name map now classifies the whole packet family as FM everywhere the class is consulted —
routing, the commanded rig mode, and the uplink's sideband — so the digipeater reaches the
VHF/UHF rig, in FM. Linear birds (the RS-44 class) behave exactly as before, and a mode name the
map has never seen still reads SSB, as it always did.

**A simplex bird rides one dial.** 145.825 up and down is one channel, not a cross-band pair: the
pick no longer writes a split for it, nothing engages the rig's satellite mode, and mid-pass
Doppler holds the dial on the published frequency — the two legs' corrections are equal and
opposite, so steering the one dial to either leg lands the other twice as far off, outside an FM
passband. Parking on the channel is what every 145.825 operator does by hand, and now the tracker
does the same. The radio line shows the one frequency once, and the Doppler row says the two legs
share the dial instead of printing the VFO mapping twice. Cross-band FM channels (SO-50's
145.850 ↑ / 436.795 ↓) keep both dials steered as before.

**Cross-band uplink on the IC-9700 rides satellite mode.** The uplink used to be written as an
A/B split — which on a dual-band rig lands in the *downlink's own band* and goes nowhere. Under a
Main = downlink / Sub = uplink mapping, Nexus now engages the rig's satellite mode, writes the
uplink (and its sideband, on an inverting bird) into the Sub band with a read-back check, and
hands the tuning selection straight back to Main, so the dial, the scope and every poll keep the
downlink. Releasing the split releases satellite mode; switch it off on the front panel yourself
and Nexus reports that instead of re-engaging over you. A rig without a Sub band refuses the
mapping honestly, Main = uplink / Sub = downlink is refused as undrivable (satellite mode always
transmits on Sub), and A/B rigs keep the existing split behaviour byte for byte. An ordinary A/B
split commanded while satellite mode is engaged — WSJT-X setting up split for a digital over
mid-pass — releases satellite mode first, so its transmit dial can never land in the Sub band
and go out on the downlink. And the sequence trusts nothing it cannot confirm: an engage whose
confirming read-back is lost is backed out rather than left half-set, a hand-back to Main the
rig refuses is remembered and re-asserted before the next tuning write or dial poll, and a rig
that will not leave satellite mode is reported in the status line instead of being shown as
simplex.

**The radio line reports what the rig acknowledged, not what was computed.** The binding under the
held bird used to print both frequencies the moment you clicked — including when nothing had been
sent. Each leg now shows as still tuning ("435.640 ↓ …") until the radio actually accepts it, the
dot fills only when every requested leg is confirmed on the wire, and a leg the rig refuses turns
into the reason in plain words. A refused pick no longer flashes a green "Working …" toast — the
toast tells the truth the read-back found.

**Clicking a transponder no longer freezes the app (0.24.3 tester build).** Field report: on the
IC-9700 under the native CI-V daemon, picking a bird and clicking a transponder frequency froze
the whole window until Windows killed it, every time. The radio loop deadlocked on itself while
applying the uplink split — it took the engine's own lock a second time while still holding it —
and every part of the UI then queued behind that lock forever. The same wedge caught terrestrial
pile-up splits ("UP 5" spots) and every mid-pass Doppler correction. The apply now releases the
lock before talking to the rig, and a liveness test drives the real pick against a simulated
9700 under a watchdog so this class of freeze fails the build instead of the operator. And
because the rig conversation now runs unlocked, a split the operator requests during it is
safe: a rejection names the dial it was rejecting and resolves only that request, so a fresh
"UP 5" click in that window still applies on the next cycle instead of being silently dropped.

### Fixed: RTTY no longer freezes the window when the copy gets rough

**Field report: "getting some application hangs on RTTY."** The decoded-text pane fades each
character by how confidently the demodulator copied it. That confidence is a continuous
measurement, so on clean copy the whole transcript was one solid block — but as conditions
degraded it crossed the fade thresholds constantly, and the pane ended up drawing the transcript
one character at a time: up to four thousand separate pieces, redrawn twice a second, forever,
because the transcript keeps scrolling. Bench-testing on a strong signal could never show it; a
noisy band or an armed decoder listening to empty air brought it on, which is why it came and
went. The pane now draws at most a couple of hundred pieces no matter how bad the copy is — the
same "cap the feed" rule the decode history already follows. Every decoded character still
prints, and the fade is still scored character by character for any transcript under that
ceiling, which is every transcript you can actually read. Only when the copy is breaking up
badly enough to blow the ceiling does the fade get scored over short stretches instead of single
characters; from that point a brief marginal burst can be averaged in with the good copy around
it and shown solid. That is a deliberate trade, and it is only ever charged on copy already too
broken to trust.

**A stalled radio can no longer stop the window from responding.** Nexus talks to the rig over
a serial link that can take up to 2.5 seconds to answer, and while it waits it holds the lock
that the rest of the app needs to read anything. Those reads were running on the same thread
Windows uses to paint and respond to clicks, so a slow or wedged rig could park the window long
enough for Windows to declare it "not responding" — with nothing in any log, since nothing had
actually crashed. Those reads now run off the painting thread: a slow rig costs a late reading,
not a dead window. The long jobs got stronger treatment — a LoTW, eQSL or QRZ sync can sit on the
network for a minute, and those now run on a pool set aside for waiting, so a slow QSL server
can't hold up the rest of the app either. A build-time check keeps it that way; it is keyed on
which commands can reach the shared radio state, so a new one can't slip through by reaching for
it in a different way.

**One misbehaving companion app can no longer freeze the radio loop.** A logger that jams the
WSJT-X control port with stop-transmit commands was previously handled all in one pass, each one
costing a full round trip to the rig — with the app-wide lock held the whole time. Nexus now
handles a bounded batch per cycle; the rest waits for the next cycle a few milliseconds later,
so nothing is dropped.

**And the class of bug behind the recent satellite-picker hang is now caught at build time.**
That freeze came from a Rust pattern where a lock is accidentally kept open longer than it
looks — invisible in review, with no symptom until the app is already frozen. The build now
rejects the pattern in the command layer where it does the damage, and flags it everywhere else.
Two spots in the FlexRadio streaming code that matched it were rewritten (both harmless today,
neither harmless to leave).

### Fixed: the Openings Log square works — and a broken screen can no longer black out the app

**Turning on the Openings Log square blacked out the whole app (0.24.6 tester build).** Field
report: assigning "Openings Log" to a Connect slot turned the entire window black — rail, top
bar, everything — and the black screen came back on every launch, because both the slot
assignment and the openings journal itself outlive the session (the journal even outlives a
wiped browser profile). The pane paints in two steps — an empty first paint while the log
loads, then the real render — and its sort state was declared on the wrong side of the
"nothing yet" exit, so the two paints disagreed about the component's shape and React tore
down the entire window. The sort state now sits above that exit, the pane renders whatever the
journal holds — including rows written by older builds with fields missing — and a build-time
sweep now fails on this pattern anywhere in the UI, so the class is closed, not just this
instance.

**A broken screen can no longer black out the app.** A section that crashes while drawing now
shows an error panel in its place — what crashed, the technical details ready to copy into a
bug report, and a button back to a known-good section — while the navigation rail and top bar
stay alive around it. Pop-out windows carry the same net. And the section the app reopens on
is checked against what this build can actually render before it is restored, so a saved id
from an older or newer version can never wedge startup into a dead screen.
### Added

- **The recall card's picture opens their QRZ page.** Click the callbook photo on the
  station you are working — or the initials circle when they have no photo — and QRZ opens
  in your browser, so you can read the page while you are still in the QSO. The circle is
  a link only once the callbook has actually resolved the call; before that it is the same
  plain badge it always was. Clicking it does not move the cursor out of the log field you
  are part-way through typing, and it is reachable from the keyboard like any other control.

- **CQ+73 filter chip** in Band Activity (a tester request): the CQ view plus RR73/73
  signoffs — a signoff means that frequency is about to free up. Sits between CQ and
  To me; the plain CQ chip is unchanged. Detection rides the message parser, so a
  `DM73` grid or an `RRR` roger never counts as a signoff.

### Fixed

- **TX Off follows WSJT-X's Enable-Tx contract in the FT modes.** Turning TX Off lets a
  transmission already in flight finish its over, and the next cycle simply doesn't start;
  turning it back on before the next cycle transmits normally. Stop TX is unchanged — it
  halts immediately, mid-over. Previously both controls cut the transmission on the spot.
  Only the FT-style cycle gets to finish: in CW, RTTY, SSTV, the voice keyer and APRS,
  TX Off still stops the over on the spot, and the cockpit Stop buttons now unkey even in
  the last fraction of a second of an over instead of leaving the rig keyed to the tail.

- **The Band Activity filter chips wrap onto a second line instead of running off the edge
  of a narrow rail.** The chip row could not wrap, which made it a single unbreakable
  block: with the rail dragged to its narrowest the last chips sat past the panel edge,
  and nothing in that column scrolls sideways, so there was no way to reach them. Adding a
  seventh chip turned that from marginal into certain.

## [0.24.0] — 2026-07-31

### Added: satellite operating — full Doppler, and a rotator that behaves

Nexus now works a satellite pass end to end. Your rotator, a standard computer interface, and
Nexus — no separate tracking appliance.

**Doppler on both legs, continuously.** Your downlink is corrected so you hear the bird where it
actually is, and your uplink is corrected so the bird hears *you* where it is listening. Both,
always: correcting only the downlink sounds right while your signal slides off the far end of the
passband and nobody comes back to you.

**Inverting transponders are handled properly.** On an RS-44 or AO-7 the passband is mirrored —
tune up the band and your uplink goes down, and the sidebands swap. That comes from the satellite
database per transponder, not from a checkbox you have to remember, so the common way to land on
somebody else's QSO is closed by default.

**Tune the downlink and the uplink follows.** Chase a station drifting through the passband the way
you would on HF — turn the VFO knob and your transmit frequency tracks them, mirrored if the
transponder inverts, while Doppler moves the whole band underneath. Tuning outside the passband is
treated as what it is, leaving the transponder, so a QSY away from the pass never drags your uplink
to a passband edge.

**You can see where you are in the passband.** A strip shows the transponder as a band with your
position marked on both legs at once. On an inverting transponder the two markers sit on opposite
sides of centre and move in opposite directions as you tune — which is the clearest way to learn the
one rule that catches everybody out, and no other display draws it. Birds with no passband to tune
inside, like the FM repeater satellites, say so instead of drawing an axis that does not exist.

**Digital modes are handled honestly.** For slot-timed modes like FT8 the dial is held still for the
length of a transmission and re-corrected between overs, because stepping the frequency underneath a
transmission in progress smears it. The physical Doppler on a fast low-orbit pass still applies and
no software can remove it, so FT modes remain best suited to the high-orbit birds. SSB, CW and FM
steer continuously, which is what you want.

**Rotator control worth using.** Park and ready positions with a post-pass action, an optional flip
above 90° elevation so a high pass goes over the top instead of spinning the mast at zenith, an
az/el deadband so the rotator stops hunting for ten minutes straight, and calibration trim for the
difference between where the controller thinks it points and where the boom actually points.

Nothing moves until you say so: Doppler is off until enabled, the VFO mapping starts at "off" until
you say which VFO carries which leg, flip is off, and the post-pass action leaves the antenna where
the pass ended.

**Satellite contacts count.** A QSO logged during a pass carries SAT_NAME and PROP_MODE, which is
exactly what LoTW needs to credit it.

**You can see the pass, not just read it.** The sky dome is now the main view while a pass is
running — a proper polar plot with labelled elevation rings and the track weighted so the high,
workable part of the pass reads first. The satellite moves along it as the pass runs.

Beside the satellite, Nexus draws where your *antenna* was actually pointed, and states the gap
between the two as a single angle. Every other display prints satellite position and antenna
position as two rows of numbers and leaves you to do the subtraction, which matters because a
deadband means the two legitimately differ by a degree or two — knowing that is normal is the
difference between trusting the rotator and chasing a fault that is not there. If your rotator is
azimuth-only, Nexus draws an azimuth line and says in words that no elevation was sent, rather than
drawing an antenna lying on the horizon. Before the pass, when auto-track is deliberately keeping
its hands off your rotator, it draws nothing at all and tells you the azimuth the bird will rise at.

**The 3-D globe becomes the pass view.** When a pass is tracked the globe frames your station and
the satellite together, draws the orbit behind and ahead, the footprint, and a line of sight from
your antenna to the bird — the range figure, drawn. The flat map stays the "where is everything"
view. Neither shows anything before the satellite is above your horizon.

The Doppler readout shows both legs with their live frequency and correction, and both views carry
a written description for screen readers.


### Fixed: Test CAT now root-causes the Icom "answers nothing" failure

An IC-7610 (or IC-9700/7300/705) that never answers CAT — "isn't answering (got \"\")" in both
normal and native CI-V mode — is almost never a broken radio. It is one of two setup traps, and
Test CAT now tells you which one you are in:

- **Wrong baud.** When the configured rate gets silence on a serial Icom, Test CAT now re-probes
  the same COM port directly at every rate the rig's CI-V menu offers (19200 / 9600 / 4800 /
  38400 / 57600 / 115200, read-only — it only ever asks for the dial frequency). If the rig answers at another rate, the
  result says so and gives you both fixes: change Baud in Settings, or set the rig itself —
  MENU » SET » Connectors » CI-V » "CI-V USB Baud Rate", with "CI-V USB Port" = "Unlink from
  [REMOTE]". That last menu is the usual culprit: from the factory the USB CI-V port is linked to
  the [REMOTE] jack and tops out at 19200, so the 115200 the native scope needs gets you nothing
  at all, not even garbage.
- **Wrong COM port.** The IC-7610 and IC-9700 present TWO COM ports and only one speaks CI-V.
  If no rate answers, Test CAT now walks you through telling them apart (Device Manager: the
  CP210x port marked "Enhanced" — Icom's driver calls it "Serial Port A (CI-V)"), and Detect /
  the setup wizard / port Auto-test now label and prefer the CI-V side of the pair instead of
  showing two identical "Icom IC-7610" rows. On the single-port models (IC-7300/705/905) the
  silent-at-every-rate verdict instead checks that the chosen COM port is really the rig.

Test CAT also now says **which backend it exercised** — native CI-V or Hamlib rigctld — in green
and red results alike, including when the native daemon failed to start and CAT quietly fell back
to Hamlib (previously that fallback was invisible, so you could spend an evening debugging the
wrong one). And the result you read is the result of the probe you clicked: the button used to
report a stale status whenever the rig rebuild took longer than its fixed wait.

### Changed: the SSTV screen — a big crisp picture, and panes that take only what they need

A decoding SSTV picture now grows to fit the window in exact whole steps of its native
resolution (up to 6×), so a big monitor shows a big, still-crisp image instead of a postage
stamp surrounded by dead space — and it steps down cleanly on small windows instead of
spilling out of its area. Below the picture, the Transmit box now takes only the height it
needs and the Gallery gets everything left over; the old layout gave each exactly half the
space whatever they held, which left a fresh install staring at a large empty Gallery band.
Both panes carry the same title-bar frame as the other screens, and the ⊞ Panels menu hides
them exactly as before. On a window too short for all of it, the screen scrolls: the picture
area and the Gallery keep a usable minimum instead of vanishing, and the Send/Stop bar sits
below the panes and stays parked at the bottom edge — Stop is reachable at any scroll
position. RTTY's decoded-text pane keeps its minimum height on short windows the same way.

### Changed: the full caller card is back while operating — QRZ photo, bearing and all

Resolving a call in the Phone or CW log strip once again shows the full recall card: the QRZ
profile photo, name, QTH, country, the distance and beam heading from your own grid, your most
recent note on the station, and the real list of previous contacts (date, band, mode, reports).
The card had been cut to a single line in 0.18.0 because it could push the operating controls
off screen; the new pane layout scrolls the log column internally, so the card can take its
full height without squeezing anything. A long history scrolls inside the card rather than
growing it.


### Fixed: satellite predictions were off by a second of orbital motion

Every satellite prediction — Doppler, pass times, look angles, the ground track — was computed
from an element-set epoch rounded down to the nearest whole second. A TLE states its epoch to
sub-millisecond precision, and the fraction that was being discarded is effectively random from
one element set to the next. A low-orbit satellite travels about 7.6 km in the second that was
being thrown away.

In practice that put the bird up to several kilometres from where it actually was, and moved the
Doppler correction by up to about 75 Hz on 70 cm — enough to sit noticeably off a narrow CW or SSB
signal on a fast pass, and over 1.5 kHz on a 10 GHz downlink. Pass rise and set times were off by
up to a second for the same reason.

The epoch is now kept at full precision. Nothing about how you operate changes; the numbers are
simply right.

This was found by a new test that checks predicted Doppler against carriers actually recorded off
the air by volunteer ground stations, rather than against another copy of the same theory. The
existing cross-check against an independent implementation had missed it, because its reference
data had been generated with the same rounding and the two errors cancelled — which is exactly the
blind spot that testing against real recorded signals exists to close. That reference has been
regenerated correctly, and agreement on the quantity that reaches the radio improved by 40%.

### Fixed: the propagation advisor stops changing its mind on identical data

On a quiet band with one or two spots — exactly when you lean on it — the advisor's "best
region", the beam heading, and the confidence word could flip between polls with nothing
changed on the air, and when two openings tied, which one got the single "just opened — jump
on it now" alert was re-rolled every poll. Ties are now broken deterministically everywhere
one reaches the screen, so identical data always gives the same answer. The same fix covers
the Journey "most QSOs in a day" date, which station leaves a full APRS list, and the link
readout's frequency on equal-strength decodes.

### Fixed: the torn-off band map disagreed with the docked one about who you need

On a pop-out band map or a second-monitor Connect map, a station that is an all-time-new
entity on the band in front of you could be painted in the dim "needs a confirmation" colour
because it also wants a QSL on another band — while the docked window showed it correctly.
Both windows now colour from the same shared logic, and a test pins the rule.

### Fixed: the 3-D globe's open-band glow was frozen

The globe's band heat updated its "breathing" only once a minute, so an open band could sit
dimmer than a closed one and the opening wedges never pulsed — on the 2-D map, that motion is
how you tell a live opening from stale spots at a glance. The globe now breathes exactly like
the map, from one shared clock, and stays still only when nothing is open (or the tab is
hidden).

### Fixed: logging could freeze the app when HRD forwarding pointed at a slow hostname

Forwarding a logged QSO to Ham Radio Deluxe resolved the target hostname while holding the
app's main lock — with DNS slow or down, every logged contact froze the interface for the
timeout and could cost the transmit slot right after logging. The send now runs off-thread,
like the N1MM forwarder always has.

### Fixed: watch-list stations vanished from a filtered Needed board

Turning on any Needed-board filter hid the callsigns you explicitly asked to be told about —
the watch-list tier had no filter bucket, even though it outranks everything. It has its own
chip now, and the dupe cue ("already worked this station on this band") also lights for
contacts that arrived from a LoTW or QRZ import, which a case-sensitive compare left dark.

### Fixed: the shipped AI CW decoder was on a vulnerable model loader

Turning on the supply-chain scan for the desktop build's own dependency set (it was never
scanned — only the test tree was) surfaced a known out-of-bounds read in the neural-net model
loader the AI CW decoder ships with, plus two denial-of-service advisories in an XML parser.
All three are gone: the decoder moved to the patched inference library (also ending a
situation where the tested decoder version differed from the shipped one), and the XML
parser was replaced by its patched line. CI now scans the shipped dependency set, runs the
38 propagation tests that previously ran nowhere — including the ones that prove connector
credentials never travel unencrypted — and keeps the two dependency trees aligned. Building
Nexus from source now needs Rust 1.91 (the patched inference library's own minimum).

### Fixed: every transmitting mode now refuses to key without a real callsign and grid

The blank-identity guard that has always protected FT8 and FT4 now covers every mode that can
transmit. Before this, selecting Q65, FST4, MSK144 or JT65 and pressing Call CQ with no callsign
set would key the rig and send a standard message with the identity missing — an unidentified
transmission. WSPR and FST4W beaconing now insist on a real callsign and a real grid too, not
merely a non-empty box, since those reports are published to wsprnet under whatever you typed.
The check is wired into each mode's own capability declaration, so a future mode gets the guard
the day it learns to transmit.

### Fixed: an over planned just before you changed something could still go out against the old settings

There is a short window while a transmission is being prepared in which the app used to re-check
only the mode tier before keying. Stop TX pressed in that window, a QSY to a new frequency on the
same band, a sideband or operating-mode change, or the T/R period rolling over could all slip
through, and the prepared over went out anyway — against the frequency, mode or slot you had just
left. The commit step now re-checks everything the go/no-go decision was based on and quietly
drops the over if any of it moved; the next slot plans a fresh one. The PTT hold time is also now
measured from when the audio actually starts rather than from the start of the preparation, so
the tail is never cut short.

### Fixed: one internal crash could silently kill receive — or all of TX/RX — for the whole session

A crash inside one decode used to be contained but leave the decoder's lock unusable, so every
later period failed instantly: the waterfall kept painting and the app looked alive while it had
gone completely deaf, and switching modes or opening the snapshot could freeze the UI with it. In
the worst case the radio loop thread itself stopped — with nothing left to drop PTT if it died
mid-over, and the "RADIO ENGINE STOPPED" banner unable to appear in exactly that case. All of the
app's shared state now recovers from a crashed thread instead of seizing, the radio loop drops
PTT before it ever exits on an error, and a contained decode crash now shows up as a visible
notice instead of a line in a log nobody sees.

### Fixed: an APRS beacon or auto-ack could transmit on top of your live microphone or a logger's over

With the mic keyed on FM — or WSJT-X keying the rig through Nexus's CAT broker — a queued APRS
beacon or an armed auto-ack would key anyway and lay packet audio across the transmission in
progress. Every transmit gate now asks one shared "who owns the transmitter" arbiter that knows
about all of it: a slot over, the tune carrier, your mic, a broker client's key, the voice keyer,
CW, RTTY and SSTV. APRS holds its queue until the air is actually free, and a broker client asking
to key while an SSTV image or voice message is playing is now refused too.

### Fixed: the tune carrier's auto-release could be configured into a minutes-long unattended carrier

The "Tune after t s" auto-release honoured any number typed into it, and settings.json is
hand-editable — a mistyped 120 for 12 meant a two-minute continuous carrier into your finals or a
dead load with nothing to catch it. The auto-release is now capped at 60 seconds however the
setting got its value; deliberate longer settings up to that minute still work.

### Fixed: one malformed APRS packet or log record could kill APRS receive or the whole radio session

A single APRS position report carrying an 8-bit character — a latin-1 degree sign from a real
tracker or an APRS-IS feed is enough — crashed the APRS receive thread, and the station list
simply stopped updating for the session with no sign anything was wrong. Roughly one in ten
corrupted position packets could do it. Separately, an ADIF record whose end-of-contact time
carried a multibyte character — from a hand-edited file, another logger's export, or a bad
WSJT-X logging datagram — could crash the radio loop itself, ending TX/RX until restart. Both
parsers now treat malformed input as the one bad packet or record it is and carry on.

### Fixed: Tempo QSOs no longer lose their protocol identity when the app restarts

A TempoFast or TempoDeep contact rides in ADIF as MFSK plus a submode (that is what TQSL and
the services accept), but the importer read only the MODE field — so the app's own log re-read
every Tempo QSO as plain "MFSK" at launch, and the next save wrote that loss to disk
permanently. The importer now reads the identity fields the exporter writes, and a WSJT-X log's
FT4/Q65/FST4 rows (which ride the same MFSK-plus-submode shape) import as their real modes too.
A Tempo row already collapsed by an earlier build reads as MFSK on disk; the original
identity is recoverable from the one-time `log.adi.bak` made on first load.

### Fixed: logging from a named channel mis-filed the QSO's band forever

Working a contact from a suffixed band-plan channel (the 2 m FM simplex data channel, the 6 m
second channel, the DX/EU windows) stored the channel id — "2m-fm", "6m-2" — as the QSO's band,
pushed that exact string to QRZ and eQSL, earned no DXCC/VUCC/WAS credit, and could never be
confirmed. The channel id is now translated to the real band the moment it enters the app, so
the log, the uploads, the awards engine and the Needed board all see the same "2m". A QSO that
was already logged with a channel id keeps it as stored; the fix protects everything logged from
now on.

### Fixed: imported contacts claiming midnight could never confirm at LoTW or eQSL

Contacts imported without a time of day used to become 00:00:00 — and were then uploaded
asserting that midnight as fact. LoTW and eQSL match on the two operators' times agreeing, so
those contacts sat unmatched forever while the "Upload to LoTW" count never went down. The app
now remembers that a time is unknown, never writes an invented one, accepts the 4-digit HHMM
time form other loggers use, and leaves time-less contacts out of upload batches — the button's
tooltip says how many and why. Contacts an earlier build already stamped with an invented
00:00:00 are recognized too: a bare midnight with no end-of-contact time reads as "time
unknown" (a genuine 00:00 UTC contact carries one, and still counts as timed).

### Fixed: correcting a busted callsign now actually reaches LoTW and the other services

Fixing a mis-copied call used to change it only inside Nexus: the record still counted as
"already uploaded", so the correction never went out, the QSO could never confirm — and any
confirmation that had matched the WRONG call stayed attached to the corrected contact. A
callsign correction now clears the upload record (the QSO re-queues to every service under the
right call) and removes confirmations and credit earned under the wrong one. Ordinary edits —
band, grid, name — keep everything, as before. Note that LoTW itself still holds the record
uploaded under the old call; nothing an upload can send retracts it.

### Fixed: the NEW ONE badge cried wolf on entire countries, and the two DXCC counts disagreed

The awards engine identifies a DXCC entity from the callsign; the log screens compared the
free-text country name, which QRZ spells its own way — "Germany" can never match the
"Fed. Rep. of Germany" already in your log, so every German and Russian contact showed NEW ONE
forever, and Statistics and Awards counted entities differently. Every comparison now uses the
callsign-resolved entity; the country text is display only. Your log carries both spelling
families today, so you should see both numbers agree for the first time.

### Fixed: loading a saved config profile could silently remove your RF power ceiling

A profile saved before a given setting existed used to load with that setting reset to its
default — and for the per-mode power caps the default is "no cap", so loading a three-week-old
profile quietly re-armed full power at FT8's 100% duty cycle. Loading is now a merge: anything
the profile doesn't carry keeps its current value, and your callsign, license class, radio
roster and sync history never come from a profile at all. Saving a profile now snapshots the
last-saved settings rather than a half-edited form.

### Fixed: importing a master log from another logger silently threw fields away

The importer kept only the fields Nexus models and discarded the rest — contest exchanges, QSL
dates, COUNTY, and the satellite fields LoTW requires for satellite credit were gone from the
moment of import, while the manual claimed a full round-trip. Every field now survives import
and export verbatim, and the award-relevant ones (numeric DXCC entity, PROP_MODE/SAT_NAME,
OPERATOR, STATION_CALLSIGN) are first-class. Upload bookkeeping also stopped re-reading the
whole log file before every stamp, which matters on a multi-megabyte log.
### Fixed: the Phone and CW screens no longer cut off the log form — the scrollbar is back

At the default window size the Phone screen hid the entire LOG THIS QSO form below the bottom
edge with no scrollbar and no way to reach it, while the Band Activity box sat mostly empty. The
cause was a set of layout rules that promised "the view never scrolls" while a guaranteed-minimum
region and the controls below it added up to more height than any window could hold — and the
rule meant to let the page scroll had shipped in a form that could never take effect. The view
scrolls again when it must, the empty box is gone (Band Activity takes the height its content
needs), and spare room goes to the waterfall instead of to blank space. The same repair covers
CW, RTTY and SSTV, and the CW decode text now fills its pane instead of six fixed lines floating
in a tall empty panel.

### Changed: Phone and CW arrange themselves to the window — wide screens get columns

On a wide monitor the log form now takes its own column beside Band Activity and the voice keyer
instead of stretching metre-wide input fields below them; very wide screens get three columns. On
a narrow window everything stacks in a single scrolling column. Push-to-talk, the CW send bar and
Stop TX live in a fixed dock at the bottom of the screen that can never scroll away or shrink,
and switching column layouts can never interrupt a log entry you are typing or a voice-keyer
message in flight. Panes you remove in the ⊞ Panels menu behave exactly as before.

### Fixed: window sizes, splitter drags and UI-scale settings are checked before they are applied

A pane width or splitter position saved on one monitor was replayed unchecked on another: a rail
dragged wide on an ultrawide could reopen on a laptop with the centre workspace squeezed to
nothing, a UI scale pinned for a big screen carried into small pop-out windows that have no
control to undo it, and a band-map window left on a since-removed second monitor came back
entirely off-screen. Every stored size, position and scale is now validated against the window
and monitors actually present before it is used. The first paint also matches the final layout —
no more one-frame flash of wrong sizes on launch.

### Fixed: screens respond to the size they really are, and dialogs respect your UI scale

The rules that adapt layouts to narrow windows measured the raw window width, which the app's own
UI scaling makes wrong in both directions: the whole app flipped to a phone-style stack at the
minimum window size where there was actually plenty of room, and an operator who raises the UI
scale to read the screen never got the narrow layouts at all. All of it now keys on the effective
size. Satellites, DXpeditions and APRS gain the narrow layouts they always declared but that were
keyed to names that never matched anything. The setup wizard, dialogs and tooltips — which
ignored the UI scale entirely and could render at half size for exactly the operator who had
raised it — now follow it. The Logbook and radio-programming screens use more of an ultrawide
monitor instead of a fixed centre column.

### Fixed: live text panes no longer yank you back down while you read

The CW transcript re-pinned itself to the newest text every half-tick, so scrolling back to
re-read a callsign mid-copy was impossible; the SENT echo, RTTY stream, Field Day log and Tempo
conversation did the same. All now follow only while you are at the bottom, the way Band Activity
already did, and switching Tempo conversations opens at the newest message instead of wherever
the previous conversation was scrolled. Logging a contact no longer makes the screen jump.

### Fixed: opening Field Day bonuses no longer hides the Sections board

The bonus checklist grew without limit and pushed the Sections board and the newest log rows out
of a screen that could not scroll. It now scrolls within its own bounded area.

### Fixed: the call resolution details are reachable again

When a callsign resolves, the prior-contact history sits inside the log form — with the layout
repairs above, it is on screen or a short scroll away instead of clipped below the window edge at
every size, which is how it has behaved since the compact recall card arrived.
### Added: Unassisted mode — one switch for a no-assistance contest entry

Settings ▸ Contesting has a new **Unassisted entry** switch. It turns off the AI CW decoder, DX
cluster / RBN spots, and the PSK Reporter needs feed together, in one action, and takes effect the
moment you press it rather than at the next restart. Every change is written to a dated record
beside your settings, so if you are ever asked what was running during an event you have an answer
with timestamps on it.

This matters because the shipped defaults put you in an assisted category without saying so. The
AI CW decoder is on by default, and CQ WW rule VIII.2 counts "a CW decoder, DX cluster, DX
spotting Web sites … local or remote call sign and frequency decoding technology (e.g., CW Skimmer
or Reverse Beacon Network)" as QSO-finding assistance, which places an entry in Single Operator
Assisted. ARRL calls it spotting assistance and names "PSKReporter, Telnet, DX spotting websites
or bulletin board systems, automated multi-channel decoders"; Single Operator may not use it,
Single Operator Unlimited may. A footer in the CW cockpit now states which of those sources are
running, with the rule citations one click away.

Two details worth knowing. Your own settings are never rewritten: the switch overrides them while
it is on, so ending unassisted mode brings your decoder and feeds back exactly as you had them.
And your own radio's decodes keep feeding the Needed board, as do your outbound PSK Reporter
uploads, because ARRL says plainly that "Generating spotting information for use by other stations
is not considered to be spotting assistance."

Rules differ by contest and change between years. The note reports what CQ WW and ARRL currently
publish and tells you to check the contest you are entering. It does not rule on your category.

### Fixed: beacons and W1AW bulletins were offered as new countries

14.100 MHz carries the international beacon network: eighteen beacons in eighteen different
countries, each transmitting every three minutes, all day and all night. Anything watching that
frequency therefore offered 4U1UN as an all-time-new country, then the Canadian beacon, then the
Californian one, and around again forever. W1AW's code practice and bulletin frequencies did the
same thing with a station that is broadcasting one-way and will never come back to your call.

Beacons and bulletins are no longer scored as needs, no longer fire a Pounce alert, and no longer
take a new-country colour. They are still shown, because hearing a beacon is real evidence that
the band is open, and that is worth knowing. They now carry a **B** or **W** badge saying what they
are.

The suppression is by frequency, not by callsign, and that is deliberate: 4U1UN is the United
Nations headquarters station as well as a beacon site, so blanking the callsign would have hidden
a genuine new country. Heard on 14.100 it is the beacon; heard anywhere else it is the station, and
it still counts. W1AW is suppressed only on its own published bulletin frequencies, so an ordinary
contact on those frequencies, or W1AW itself operating elsewhere, still scores normally.

### Fixed: the Call Roster and Band Activity filters reset on every restart

"Needed only" and "Hide worked" on the Operate Call Roster, and the Band Activity filter chip
(All / CQ / To me / On RX / B4 / New), now come back the way you left them. They were held in
screen state only, so every launch put them back to showing everything and you re-ticked them at
the start of each session.

Each pane remembers its own set, so a torn-off Operate window can sit on Needed-only while the
docked one still shows the whole band. A window that has never been given its own filters opens on
the ones you are already using rather than on defaults. If you have never touched these controls,
nothing changes: both checkboxes start off and the chip starts on All, exactly as before.

A stored value that is damaged, or left over from a build whose filters were named differently, is
ignored rather than applied. The roster can never come up quietly hiding rows with no ticked
checkbox to explain why.

### Fixed: "sort by need" on the Call Roster had no discernible order

Sorting the roster by Need now ranks by how much the station is worth working: a call you asked
for by name, then a new entity, new zone, new state, new grid, new band, new mode, then one you
have worked but not confirmed. That is the same ranking the Needed board uses, so the two agree
row for row, and a rare grid or a live park activation keeps the extra pull it has on the board.

Two things were wrong. A station heard on more than one band was ranked by its WEAKEST need
instead of its best, so a new country on 20 metres that also needed a confirmation on 40 sorted
as the confirmation, well down the list. And among stations of equal need the roster listed the
quietest first, which is backwards: of two equally-needed stations the louder one is the better
bet. Both are fixed, and the row's colour now names the same need the sort ranked it by.

That weakest-need mix-up was not confined to the roster. The same per-station need was feeding the
map and the band strip, so a station worth chasing could be painted in the colour of the least
interesting thing about it anywhere it appeared. Every surface now takes a station's strongest
need, from one shared piece of logic rather than three.

### Fixed: the Needed board no longer claims a "new mode" you already worked

An operator with roughly 11,000 FT8 contacts was shown a new mode needed for Asiatic Russia on
30m, on a band and entity they have six confirmed FT8 contacts with. The need behind it was real —
they have never worked that entity on CW — but nothing on the screen said so, so the board looked
like it could not read the log.

Two separate things were wrong, and both are fixed.

- **The row said the wrong thing.** A mode need is judged per DXCC entity and mode class across
  *all* bands, which is what the per-mode DXCC awards count. The headline appended whichever band
  the station happened to be spotted on, turning "you have never worked Asiatic Russia on CW" into
  "New mode — CW Asiatic Russia 30m". Mode rows now read **"New mode — CW Asiatic Russia (any
  band)"** and state exactly the claim being made. Band slots remain the separate **new band** row.
- **The roster showed chips you could not act on.** Need chips on the Call Roster and Stations
  panel were keyed by callsign alone and pooled every alert for that call, so a CW mode need painted
  an unlabelled `MODE` chip onto a 30m FT8 roster. Chips, row colour, and the **Needed** filter are
  now scoped to the band and mode class actually in front of you, matching what Band Activity has
  always done. A genuinely cross-band need still shows on the Needed board, where the band is named.

Three further mis-classifications surfaced in the same audit, each of which could invent or hide a
need:

- **Phone contacts logged as `PH`** — the token the N3FJP family exports, and present in real
  imported logs — were counted as *digital*. They credited a digital mode slot they never earned
  and left the phone slot reading unworked. `PH` and the digital-voice modes (`DIGITALVOICE`,
  `DSTAR`, `FUSION`, `M17`, `FREEDV`) are now classed as phone everywhere, including in LoTW
  confirmation matching, where the mismatch could leave a confirmed contact showing as a
  confirmation opportunity forever. This is read-side: existing logs reclassify with no re-import.
- **Band labels are compared case-insensitively.** A log that carries both `30m` and `30M` (both
  spellings occur in a real logbook) credited the same slot in the awards engine but not in the
  decode feed's chip gate.
- **Nexus's own tier names are recognised on POTA/SOTA spots.** `TempoFast`/`TempoDeep`/`FT1`/`DX1`
  fell through to guessing the mode from the frequency, which on a CW-only band segment could invent
  a CW mode need.

This works together with the strongest-need ranking above rather than against it: the band and mode
gate decides which of a station's needs count on the surface you are looking at, and the ranking then
picks the most valuable of those. So a row is ranked by the best reason to work that station **that
you can actually act on right now**, and its colour, its chip, its screen-reader label and its place
in "sort by need" all name that same need.


### APRS says which radio it is listening to

## [0.21.5] — 2026-07-29

### APRS now sees the whole network, and can contribute to it

APRS used to show you exactly what your own antenna decoded, and nothing else. That is the honest
picture of what your radio can reach, but on a quiet channel it is also indistinguishable from a
broken receiver — which is what several operators were looking at.

Nexus can now also connect to **APRS-IS**, the internet side of APRS, and plot what the wider
network is reporting near you alongside what you actually hear. Turn it on in
**Settings ▸ Modes ▸ APRS**, where every APRS setting now lives, beside RTTY and CW.

- **Every station is tagged with how it reached you** — `RF` when your own receiver decoded it,
  `net` when only the internet reported it, `RF+net` when both did. You can never mistake "the
  network says this station exists" for "my antenna can hear this station", and one click hides the
  internet stations entirely, leaving the view of what this radio genuinely reaches.
- **It is also a diagnostic.** The internet feed runs whether or not the APRS decoder is armed, and
  gets its own status chip beside the decoder's. Internet stations appearing while the RF chip stays
  silent tells you the fault is in the radio chain — antenna, cable, sound card, tuning — and not in
  the app. That was previously guesswork.
- **You choose what comes through.** A radius around your grid square (150 km by default — APRS is
  a local mode), a list of watched callsigns that come through from anywhere however far away they
  are, and switches for weather stations, objects and items, and text messages.
- **No passcode needed to watch.** The feed connects read-only, which every APRS-IS server accepts
  from any licensed operator.

The internet status chip on the APRS board is also its control: click it for the feed switch, the
range radius, and your watched callsigns. The radius is there because the chip's own advice when the
feed goes quiet is "widen the radius" — the control belongs where the advice is. Both places edit
the same settings, so they can never disagree about whether the feed is on.

With the feed running you can also switch on a **receive-only iGate**: packets *your own antenna
hears* are contributed to APRS-IS, so stations around you reach the global map through your station.
It is a separate switch from the feed, and it stays in Settings rather than on the cockpit, because
it publishes under your callsign — contributing to a global network under your own call should be a
considered decision, not something a stray click can start.

Nexus only ever sends packets it actually heard on the air, and honours every rule the network asks
of an iGate: it never re-sends a packet that already came from the internet, never sends one whose
sender marked it `NOGATE` or `RFONLY`, suppresses duplicates, and caps its own upload rate so a
stuck transmitter nearby cannot flood the network in your name.

**Nexus does not gate the other way** — internet traffic is never transmitted on the air. That
direction means a radio keying up unattended, which is not something this app will do.

### The APRS map grows up

Every station on the APRS map was the same grey dot. The packets were carrying the answer the whole
time — APRS stations pick their own icon, and Nexus was throwing it away.

Stations now draw as their **actual APRS symbol**, on the map and in the station list: cars, trucks,
bicycles and people, weather stations, digipeaters and iGates, campsites, balloons, boats and
aircraft. Vehicles under way point the way they are heading. Where an operator has put an **overlay
character** on their symbol — the `I` on a full iGate, the `R` on a receive-only one, the hop count
on a digipeater — it shows on top of the icon, because that character is often the most useful thing
about the station. A symbol Nexus does not recognise draws the standard "unknown" glyph, never a
blank. The icons are drawn in Nexus rather than borrowed, so there is nothing extra to install.

Symbols also carry a colour for their family: homes and portable stations, vehicles, aircraft,
boats, weather stations, digipeaters and gateways, and hand-placed objects. Colour says what a
station *is* — nothing here means urgency. The palette varies brightness as well as hue so the
families stay apart for colourblind operators, and it has a separate version for the light theme.

**You can still tell what your own antenna heard.** That used to be the solid-versus-hollow dot.
The shape now says what a station IS, so the ring around it says how it reached you: solid for RF,
doubled when you heard it both ways, dashed and dimmed for internet-only. Solid still means yours.
Below a local scale the map goes back to plain dots — a continent covered in icons answers a
question nobody asked.

**The map opens on the local picture.** APRS is a local mode — 2 m simplex plus a digipeater or two
reaches tens of kilometres — so the map now opens reaching about 275 km in each direction, and you
can zoom in much further than before. Previously it opened at a scale where roughly 23 km fell on a
single pixel, so a station 40 km away drew less than two pixels from your own marker and an entire
local net stacked up underneath it as one dot. A freshly decoded station now appears the moment it
lands rather than waiting up to a minute for something unrelated to repaint the screen, and clicking
a station in the list highlights it on the map immediately. With no grid square set, the map centres
on the traffic you are hearing instead of painting an empty box with no coastline and no stations.

**Click a station for everything known about it.** Clicking used to highlight it and nothing else.
It now opens a detail card, from either the map or the list:

- The symbol at readable size, with what it actually **means** in words.
- **How it reached you, per source, with separate ages** — "your receiver decoded this station
  4 min ago; the internet feed reported it 20 s ago". Those are two different facts and only one of
  them says anything about your antenna, so they are never merged into a single "last heard".
- Position with grid square, and distance and bearing from your station.
- Course, speed and altitude when the station is moving.
- **The weather, when it is a weather station.** Those readings used to be shown as the raw field
  string — `220/004g011t085r000p000P000h68b10156`. Nexus now reads it: temperature, wind direction
  and speed, gusts, rainfall, humidity and barometric pressure. A sensor a station does not have is
  left out rather than shown as zero — `r...` on the wire means "no rain gauge fitted", not "no
  rain", and reporting 0.00 in would be inventing a measurement.
- The comment text, the digipeater path, and whether the packet reached you **direct or digipeated**.
- The raw packet, collapsed until you want it.
- One click to QRZ, or to the station's page on aprs.fi.

Behind all of it, the map keeps **stations**, each with its own history: last position, when it was
last heard by your radio and by the internet, symbol, course and speed. A station stays for an hour
after its last packet and starts to fade after twenty minutes of silence, so a quiet station recedes
instead of vanishing. You can change the hour in **Settings ▸ Modes ▸ APRS**, and setting "Keep
stations for" to 0 means exactly that — no fade, no removal, every station kept until the
2000-station ceiling — because some operators genuinely want an all-day picture.

### APRS tells you exactly why it is not decoding

An empty APRS screen used to mean half a dozen very different things and looked identical for all of
them: the app listening to the wrong sound card, the radio parked on another frequency, a signal
arriving too corrupted to check, or a genuinely quiet channel. Only packets that passed their
checksum ever reached the screen, so everything else vanished without trace.

The APRS header now carries a decode readout that names which one you are looking at, in six honest
states: **no input**, **silent**, **wrong frequency or mode**, **bursts heard but failing their
checksum**, **listening on a quiet channel**, and **decoding** with a count and how long ago the
last packet landed. Beside them it shows the **input level in dBFS**, so what the decoder is hearing
is a number you can read rather than something to infer from which message appeared. Hovering
explains what to check, and the empty list and empty map say the same thing rather than a generic
"nothing here".

**A closed squelch is not a broken audio device.** A squelched radio does not send the app silence
in the sense of *nothing*; its USB codec keeps streaming a continuous run of digital zeros. Audio is
arriving the whole time — it just has no level. So **"Silent"** (the input alive with nothing on it,
almost always the squelch closed between packets) is a separate state from **"No input"** (no audio
samples arriving at all), and only the second is a fault. An idle FM channel between packets is what
APRS looks like nearly all the time, so "Silent" is not coloured as a problem; it says to open the
squelch and watch for hiss if you want to confirm the routing. "No input" really does mean the
capture device is wrong or gone, and still points you at Settings.

**A mistuned radio is named as one.** FT8 decoding beautifully on 2 m at the same moment the APRS
screen insists there is no audio are both true statements: the radio has one receiver and one dial,
and parked on the FT8 frequency in USB it is never receiving the APRS channel at all — so every
message about audio levels would be advice about the wrong problem. The readout now looks at the
radio itself, says so first, and offers a one-click fix: **"The radio is on 144.174 USB — APRS needs
144.390 FM"**, with a Tune button beside it. It judges against the APRS channel *you have selected*,
so 144.800 in Europe or 145.175 in Australia is correct, not a warning. Sitting on the right
frequency in the wrong mode is its own trap — the signal looks strong and decodes nothing — so that
case reads **"on 144.390 but in USB — APRS needs FM"** and explains that FM packet audio demodulated
as SSB is garbled. Data-FM submodes such as PKTFM count as FM, because on the air they are. Tuning
while an FT8 over is in flight cannot move the radio immediately — the rig will not accept a
frequency change mid-transmission — so rather than appearing to do nothing, the Tune button says the
radio will move when the over ends.

**It says which radio it is listening to.** If more than one of your radios covers the APRS band,
the readout names the one it is actually listening to — "on FT-991A" — and its tooltip explains that
APRS follows the active radio and that routing rules decide which radio a band goes to. Without
that, a station whose APRS audio is set up on one rig while the app listens to the other has exactly
one symptom: silence. A working station looks like a dead band. With the radio named, that is a
glance instead of an afternoon. On a single-radio station, or when only one radio covers the band,
nothing is shown — there was no choice to make and saying so would just be clutter.

**Every claim says when it was true.** The packet counts run from the moment you arm the decoder,
while the level is whatever the radio is doing this instant, and mixing the two produces sentences
that contradict themselves: *"2 packets were heard but none passed the checksum... peak -99 dBFS."*
Nothing is heard at -99 dBFS. A failed-checksum count now only speaks in the present tense while
bursts are still arriving, within the last minute, and dates itself when it does: *"2 bursts heard
since arming, last one 20s ago — none passed the checksum"*, with the live level on its own clause.
Decodes are treated differently on purpose — a packet that passed its checksum proves the whole
chain works, and that stays worth knowing however long ago it was, so it keeps its place and carries
its age instead: *"18 packets decoded since arming, last one 12m ago."* The level reading says what
window it measures, the most recent tenth of a second, so a low number reads as the gap between
packets rather than something being wrong. And once packets are decoding, the readout stays on the
decode count instead of flicking back to a warning during the quiet gaps between them.

Three smaller pieces of the same honesty. A failed-checksum count explains that a packet caught
part-way through — which is what happens when the squelch opens mid-burst — can never pass its
checksum, so some failures on a busy channel are expected rather than a sign of a misconfigured
radio. Packet-shaped patterns found in silence do not count as packets at all: given enough minutes
the decoder will eventually find one in the noise floor, and reporting that as "packets heard"
invents evidence for a problem that is not there. And the Monitor button, the decode readout and the
empty-state text all report the decoder's actual state rather than the button's own guess, so
leaving the APRS screen and coming back can never show "Monitor" — as though nothing were running —
while packets keep decoding into the list beside it.

None of this is covering for a fragile decoder. The packet decoder was measured against "twist" —
the two packet tones arriving at unequal volume, which is the classic reason packet decoders
struggle on real signals — and packets still decode with the tones up to 24 dB apart, far beyond the
roughly 9 dB that real signals show.

### APRS starts listening when you open it — receive only

Opening APRS now starts the decoder for you, so the screen is not dead until you find the Monitor
button. This is strictly receive: a decoder started this way will **never** send an automatic ack,
whatever your TX setting.

Automatic acks stay behind two deliberate acts, and opening a screen is not one of them: you arm
Monitor yourself, **and** TX is on. That is now enforced rather than assumed — an unattended
transmission should never follow from navigating somewhere. The Monitor button says which state you
are in, reading "Monitoring (auto)" when APRS started it for you, and its tooltip spells out whether
acks can go out.

Clicking Monitor always means start or stop, as before. It never quietly upgrades an
automatically-started decoder into one that can transmit — to allow acks, stop it and start it
yourself. And if you stop the decoder, it stays stopped: coming back to the APRS screen will not
restart it behind you.

### Route each mode to the radio that does it best

Nexus already handed a band to the radio configured for it: pick 2 m and it switched to your VHF
rig. But a band is not fine enough. If you have a 2 m/70 cm rig for weak-signal digital and a
different rig for FM and APRS, both of them cover 2 m — and Nexus had no way to tell them apart, so
a 2 m FT8 spot and an APRS tune went to whichever radio it happened to pick first.

You can now route on the band **and the mode**. In **Settings ▸ Radio** there is a routing table
under your radios: pick a set of bands, pick a mode class, pick the radio. Rules are checked top to
bottom and the first match wins, so a specific rule above a broad one takes precedence — and the
arrows beside each rule let you reorder them. Anything no rule matches falls back to the band
coverage you already set on each radio, and then to a default radio you can nominate for everything
else.

A three-radio shack maps onto two rules. Digital to the 9700, APRS and repeaters to the 991A, HF to
the FTdx10:

| Bands | Mode | Radio |
| --- | --- | --- |
| 2 m, 70 cm | FM & APRS | FT-991A |
| 2 m, 70 cm | Weak-signal digital | IC-9700 |
| *(everything else)* | | FTdx10 |

The mode classes are deliberately coarse — weak-signal digital, FM & APRS, SSB phone, CW, RTTY —
so a whole station fits in a handful of rules rather than one per submode. Every action that used
to consult the band table now consults band + mode: the band picker, a typed frequency, clicking a
spot on the Needed board or a DXpedition card, and APRS Tune. Peg-lock still pins your radio and
stops all of it, exactly as before.

There is a **"Where would this go?"** control under the table. Pick a band and a mode and it tells
you which radio that combination resolves to, without touching a rig — it asks the same code the
radio does, so it cannot tell you one thing and then do another.

If you never add a rule, nothing changes: routing stays band-only, as it was.

**And a third radio now works properly.** Two radios worked. A third did not, for a reason that
only ever shows up at three: each radio's window keeps its own settings file, seeded once from the
shared one the first time that window opens. With two radios you always add the second one before
those per-window files exist, so both windows learn about both radios. The third radio is the first
one you add *after* they exist — so it landed in exactly one window's settings and nowhere else. The
launch picker (which reads the shared file) never offered it, the other window never monitored it,
and there was no way to repair it from inside the app.

Adding or removing a radio now updates the shared config too, and every window picks up radios added
elsewhere when it starts. The routing table above is shared the same way, since which rig does 2 m
FM is a decision about your station, not about one window.

Three smaller things that also only bite at three radios: a band claimed by two rigs now always goes
to the same one (it used to depend on the order they happened to sit in the list); adding a radio
after removing one no longer produces two radios with the same name, which made the port and audio
conflict warnings ambiguous; and a window launched pointing at a radio that no longer exists now
says so instead of quietly driving the first radio's serial port — which is the port another window
is already using.

### Star a repeater straight from the search results

Program's repeater search has a star on every result row. Starring one saves it into Memories as a
proper FM channel, with the machine's shift, offset and access tone, and puts it on the quick-recall
strip in the Phone, Operate and CW cockpits — where one click, or Ctrl+1 through Ctrl+9, tunes it.
Previously the only route from a search result to your favorites ran through the channel-list
builder and a second trip into the Memories section to star each row by hand.

Starring the same machine twice does not duplicate it: if that frequency, mode and tone are already
saved, the star lights on the row you already have. The star toggles back off and leaves the channel
in Memories, so unstarring only takes it off the cockpit strip.

Starred repeaters also remember where the machine physically is, so Memories shows how far away and
in what direction each one is. That is measured from your current grid every time it is displayed
rather than stored, so the distances follow you when you operate portable.

Program's per-repeater Tune button tunes in a single step that knows it is FM, which is what makes
it land correctly on a multi-radio station and after you have been operating something other than
voice. Naming FM explicitly settles both decisions at once: the machine's frequency, shift, offset
and tone all go to the radio you mapped for FM, and the rig ends up in FM rather than in whatever
data mode the last section you operated left it in — a repeater is inaudible in a data mode. Tuning
does not move you out of Program or arm transmit; it puts the radio on the repeater so you can
listen. Any later retune, section change, radio switch, or a turn of the VFO knob down to HF
releases the FM hold, so FM never follows you somewhere it does not belong.

### DXpedition calendar: one operation, one bar

A multi-day DXpedition was drawn as a separate little chip on each of its days, so a ten-day
operation looked like ten unrelated things. Each operation is now a single bar running across the
days it is on the air. Where a run crosses into the next week it picks up again on the following
row, named and flagged so you can follow it.

Every operation also gets its own colour, and keeps it — on its calendar bar, on its dot in the
"what to chase" summary, and on the rail beside its entry in Details. The colour means nothing but
"this is that one", which is what lets you pick an operation out of a busy fortnight without
reading a single callsign. Today is still the strongest thing on the grid, and an operation you are
chasing still stands out from the rest.

Bars wide enough to hold it now carry the bands the operation announced, low bands first, so
whether they are bringing 160 and 80 is visible without opening anything. Hovering any bar gives
the full picture: entity, dates, every band, the modes, and your modelled best shot.

When more operations overlap than a week has room for, the day says "+2" instead of quietly hiding
them; clicking opens that week out and clicking again closes it. Operations that do not overlap in
time now share a row rather than each burning one, so the calendar stays short.

**Clicking an operation also opens its webpage** in your browser, so the announcement you are
looking at is one click from the team's own page — bands, schedule, QSL route, pilot station. The
Details rail carries the same link on each entry, labelled, so you can see where it goes before you
click it. About a third of announced operations publish a website, and the calendar source has been
carrying those links all along — Nexus was throwing them away while reading the page. The rest now
open the callsign's QRZ page instead, which is where their details and QSL route live when there is
no expedition site. Either way the tooltip names the destination first, and says plainly when it is
the QRZ fallback rather than the operation's own page. Clicking a calendar bar still selects that
operation in the Details rail as it did before, so nothing that used to work costs you an extra
click now.

### Fixed: the N1MM contact broadcast sent nothing unless Field Day was running

Set the N1MM address, log QSOs, watch the network: nothing. An operator running it alongside Ham
Radio Deluxe saw HRD's packets go out on 12060 and not one from Nexus on 12061. The address had
looked like a standing integration sitting next to HRD, and it was not one — the broadcast only
ever fired during a Field Day event, and said so nowhere.

**Settings ▸ Logging & Connectors ▸ N1MM+ Integration** now has a **Broadcast every QSO** switch.
Turn it on and each logged contact goes out as an N1MM contact packet, event or not — from the
digital modes, from the CW and Phone cockpits, from a hand-typed logbook entry, all of them. Point
OpenHamClock or GridTracker at the address and every QSO plots on its map as you log it. The
packet leaves at the moment the QSO is logged, in the same breath as the HRD one. Turn the switch
on with the address field empty and Nexus fills in the usual local target for you. The address
field now also states which of the two it is doing, so a configured-but-silent output can never
look like a working one again.

It is off after an upgrade, and nothing but that switch can turn it on — your contacts do not
start going out over the network because you installed a new version.

Field Day is untouched. During an event, contest contacts still go out the way they always have,
carrying your class, section and points; the standing broadcast only ever carries the contacts in
your regular log. A contact is never sent twice, so it is safe to leave the switch on through a
Field Day weekend. An ordinary QSO carries what a map needs — call, grid, band, frequency, mode,
time — and honestly claims no contest points.

If you run several consumers on one machine, name the port. 12060 is often already taken (HRD
listens there), and the port you type is the port that is used.

### CW keying now works with rigs that refuse 1200 baud on their keying port

A tester with a new Yaesu FTX-1 could not key CW through the rig's built-in Standard COM port.
Nexus reported that it could not open the port; Windows, asked directly, said "a device attached to
the system is not functioning." The port was fine. Nexus was asking for it at 1200 baud, and the
FTX-1's firmware refuses that one rate while accepting every other.

A keying port sends no data at all — Nexus only flips a control line up and down, and the rig shapes
the CW — so the baud rate never meant anything on the air. It was a number we had to name to open
the port, and 1200 was an arbitrary choice that eventually met a radio that says no. Nexus now asks
for 9600, and if a port refuses that it works down through 19200, 4800, 2400 and 1200 until one is
accepted, then keys normally. Nothing to set, and nothing to notice: existing keying interfaces
behave exactly as before.

The same fix covers the other two places a control line is used this way — **true-FSK RTTY keying**
and **serial PTT** — because the same port on the same radio would have refused those too.

When a keying port genuinely cannot be opened, the message now quotes what the system actually said
and which rates were tried, instead of guessing at causes. The tester above had to diagnose this in
PowerShell because our error message withheld the one useful sentence.

### Opening APRS on an HF-only radio no longer breaks CAT

Reported on an FTdx10, which covers HF and 6 m and has no 2 m at all. Rig control worked normally
in the Phone and CW cockpits; clicking into APRS killed it, and it stayed dead until Nexus was
restarted. Going back to Phone afterwards showed the dial parked on 144.390 — a frequency the
radio had never been on.

Opening the APRS cockpit tunes your radio to the APRS channel, which is on 2 m. On a radio that
cannot go there the radio refused the command, and Nexus did not notice: it took the refusal for
success, wrote 144.390 into its own idea of where the radio was, and stopped checking. Everything
after that followed from believing a thing that never happened.

Three fixes, and each one stands on its own:

**Nexus now knows what your radio covers before it commands it anywhere.** It reads the receive
range straight out of the radio's own capability table over CAT, so an HF-only radio is never sent
to 2 m in the first place. Where the ranges cannot be read — no rig control, or a rig-control
daemon that does not report them — nothing is blocked; the check only ever refuses on information
it actually has.

**A refused command is now treated as a refusal.** Nexus checks what the radio said back, keeps
showing where the radio really is rather than where it was asked to go, tells you the radio would
not accept that frequency, and stops asking after a few tries instead of hammering the link. A
command your rig will not take no longer wedges rig control until you restart the app, whatever the
rig and whatever the command.

**Rig control recovers on its own.** Nexus stops polling a radio that has stopped answering, which
is right — but that state used to be permanent, so any hiccup meant no rig control until you
restarted. It now retries quietly, backing off to about once every thirty seconds, and picks the
radio back up within a couple of seconds of it answering again. This one is not specific to APRS:
anything that interrupted the link used to cost you rig control for the rest of the session, on
every rig Nexus talks to.

**In the cockpit**, an HF-only station now reads *"No 2 m radio"* with an explanation, instead of a
Tune button that could only ever fail. The internet feed is genuinely useful without a VHF radio —
it shows APRS traffic other stations have reported — so the view tells you that rather than
looking broken.

### Credit where the code came from

Two of the modes Nexus decodes stand on other people's work, and the NOTICE file — the document
that records exactly what Nexus borrowed and from whom — did not say so. It does now.

The RTTY decoder is a port of **fldigi**'s receive path, by Dave Freese W1HKJ and Stefan Fendt
DL1SMF, whose own lineage runs back to Tomi Manninen OH2BNS's gmfsk. The threshold detector that
makes it print through noise is a design Kok Chen W7AY published and gave away. The SSTV receiver
is vendored from **slowrx** by Oona Räisänen OH2EIQ, reaching Nexus through Jason Herald's Rust
port of it. Each now has a full entry in NOTICE naming the project, the author, the license, and
which files came from where, plus a line in the README credits.

Nothing about how the radio behaves changes — these are comments and documents. What changes is
that anyone reading the source can now trace every borrowed line to the person who wrote it.

Two smaller corrections in the same pass. The RTTY *transmitter* is Nexus's own code, not fldigi's,
and its file header now says so outright, so no future reader assumes the transmit side came along
with the receive side. That header also credited "the W7AY dual-oscillator scheme" without naming
Kok Chen or linking what he actually published; it now cites the paper, and is honest that the
shaped edge treatment is Nexus's answer to the problem that paper measures, not something taken
from it.

## [0.21.0] — 2026-07-29

### APRS gets a map

APRS had no map. Everything sat in a small area at the top left of the screen with the rest of the
window empty. Stations, their tracks and their paths now plot geographically, with the controls and
lists moved to a rail beside it. On a narrow window the map comes first.

Nothing new is decoded for this — position, course and speed were already in the packets, with
nowhere to draw them. Clicking a station on the map highlights its row in the list, and the reverse.

### SSTV shows you the band, then shows you the picture

The SSTV screen had no waterfall at all, so there was no way to see what was on the frequency
before an image arrived. That space is now a live waterfall — and when a signal starts decoding,
the same space becomes the picture, building downward as it comes in.

Because the picture stands where the spectrum was, you cannot see whether the radio is off
frequency while an image is arriving. So the mistuning is now stated outright: a "tuning +12 Hz"
readout beside the line count, whenever it drifts past 10 Hz. The decoder already worked this out
from the header and had simply never shown it to you.

### A DXpedition calendar you can actually read at a glance

The DXpedition view now opens on a traditional month calendar with today clearly marked and each
operation drawn across the days it runs. Clicking one opens its detail.

Above it, a plain-language summary of what to chase: which are on the air now, which start soonest,
the best band and time for each, and the best day or two to try. All of that was already being
calculated and simply spread across the page for you to assemble yourself.

The dense band-by-hour heatmaps move behind a "Details" tab and are toned down when shown, so the
page is no longer a wall of yellow, orange and red when you scroll it.

### Satellites: one pass at a time, on a bigger globe

Clicking a satellite drew every OTHER satellite's ground track too, so the pass you had just chosen
was buried under a dozen unrelated lines. Now only the selected bird is drawn.

The globe was also locked to a fixed width no matter how large the window was. It now grows with
the space available.

### QRZ confirmations arrive on their own

Nexus could already pull your QRZ logbook down — QSOs logged elsewhere and their confirmations —
but only when you pressed Sync. Turn on Settings ▸ Logbook & QSL ▸ QRZ ▸ "Pull confirmations
automatically" and it happens hourly instead, so confirmations appear as people post them.

Only what CHANGED is fetched after the first run, so an hourly check is a small request rather than
your whole logbook twenty-four times a day. It is off by default, a failed check never skips the
span it missed, and the schedule survives a restart.

As before, a QRZ confirmation shows the contact as confirmed but never counts toward DXCC or WAS —
those need LoTW or a paper card, and counting QRZ would inflate them.

### Fixed: alerts repeating on every cycle

A new-DXCC alert would fire again and again for the same station, once per transmission, instead
of once when it appeared. Plain CQ alerts did the same.

Two causes, and they compounded. An alert was identified partly by the station's measured audio
frequency — which drifts a few hertz between transmissions — so the same station saying the same
thing looked like a brand new event each time. And because every one of those counted as a
separate remembered alert, a busy band filled the "already alerted" memory in a minute or two; the
oldest entries were then discarded first, which included the record saying the new one had already
been announced. So it announced it again.

Alerts are now identified by who transmitted and what they said. The things that should only ever
alert once — a new entity, a new grid, a watch-list hit — are remembered separately from the ones
that legitimately repeat, so no amount of band traffic can push them out.

### Fixed: one internal error could leave the radio deaf until you restarted

A safety lock guards the shared decoder, and if anything ever failed while holding it, that lock
stayed broken for the rest of the session. Every decode and every transmit after it failed too —
silently. The app kept running and the waterfall kept painting while nothing was being heard, and
the only sign was a line in a log file you would never see. It now recovers and carries on.

Not something that was reported on the air. It was found while tracking down the JT65 crash, and
it is exactly the failure that crash would have triggered.

### Fixed: the window could stop responding while a decode was running

Transmitting and decoding both need the same audio engine, and the transmit side used to wait its
turn while holding a lock the interface also needed. If a decode was still running when the next
transmit came due, the whole window froze until it finished — under a second on a fast PC, several
seconds on a Raspberry Pi.

The transmission is now prepared without holding that lock. Nothing changes on the air: the same
work happens at the same moment, the interface just stays alive through it.

## [0.20.0] — 2026-07-28

### Fixed: JT65 could crash Nexus outright, and it is transmitting again

On Windows, pressing Call CQ on JT65 killed the app the moment the transmit cycle came
round — before the radio was keyed. Transmit was switched off in 0.19.17 as a stopgap.
The cause is now found and fixed, and **JT65 transmits again**.

Nothing was ever wrong with the transmit path. The crash came from the *decode* that runs
at the same instant, which is why it looked like a transmit bug and why it appeared right
when you pressed Call CQ.

Nexus decodes a full minute of audio for JT65. When it has not yet collected a full
minute — the first minute after you select the mode, or after the buffer is reset as
transmit begins — it pads the front of that minute with silence. Past about 28 % silence,
a brightness reference inside the decoder went to zero, everything downstream became
"not a number", and a peak-search step then read from an essentially random memory
address. On Windows that is an instant, uncatchable process kill. On Linux the same code
happened to land somewhere harmless, which is why it never showed up in testing here or
in CI, and why only one mode was affected: this sync code is JT65's alone, which is what
kept Q65 at the same 60-second period working perfectly throughout.

Three fixes: the reference can no longer be zero, the peak-search variable can no longer
escape unset, and a second variable on the same path with the same flaw was closed too.
A partly-filled minute now simply reports nothing, quietly. Both defects are inherited
from upstream WSJT-X, which never meets them because it only ever decodes a full window
of live audio.

### Added: native crash reports on Windows

When Nexus dies from a fault in the DSP layer rather than a normal error, Windows tells
you nothing and the window just disappears. Nexus now writes `nexus-crash.txt` — beside
the program, or in your `%TEMP%` folder — naming the component at fault and the call path
into it. Sending that file with a bug report turns a crash like the one above from a
multi-day hunt into a single look. It records only addresses and module names: no
callsign, no log, no personal information.


### Six more modes now transmit

Nexus decoded eight WSJT-X modes. It now transmits six of them: **Q65, FST4, FST4W,
MSK144, JT65 and WSPR**, alongside FT8, FT4 and the Tempo tiers.

Every waveform was checked by generating a transmission in Nexus and having **stock
WSJT-X decode it**, rather than by testing Nexus against its own decoder — both halves
come from the same vendored source, so a shared misreading would pass unnoticed. That is
not hypothetical: FST4 at the 15-second period was going out half a second late and every
in-house test passed, because the transmit duration and the modulation start time are two
different numbers in the upstream source. Stock WSJT-X reported the offset. Q65's waveform
was additionally compared sample by sample against WSJT-X's own generator and matched at
0.9985 correlation.

JT65 is the exception: upstream's JT65 decoder depends on KVASD, a non-free component
Nexus does not ship, so there is no stock decoder to check against. It is verified by
round-trip against WSJT-X's own signal generator instead.

Each mode keeps its own operating rhythm rather than inheriting FT8's. MSK144 waits twelve
transmit periods before giving up on a contact, against three for FT8, and its CQ runs are
uncapped — on meteor scatter silence is normal rather than a sign the other station has
gone, and FT8's settings abandoned live contacts. WSPR and FST4W never touch the QSO
sequencer at all; they transmit on a percentage schedule, and below 40% avoid two
transmissions in a row while still hitting the requested rate.

### Every mode now lands on the right frequency

Mode frequencies are read from WSJT-X's own frequency table rather than typed from memory.
Previously every new mode inherited FT8's list, which is wrong for most of them: MSK144 and
Q65 have no HF presence at all, FST4 and FST4W are LF and MF, and WSPR on 20 m is 14.0956
rather than 14.074, so "20 m WSPR" was listening to FT8. Selecting a mode with nothing on
your current band now moves the radio to that mode's own calling frequency.

### Transmit safety

A review of the transmit paths before any of this reached a radio found four real defects.
The most serious: entering the Phone, CW or RTTY section arms transmit for you, and the
beacon path was being reached before the check that stops digital modes keying while those
sections own the radio — so a configured WSPR beacon would key on schedule while the
operator worked SSB, putting 111 seconds of data tones into the 20 m phone band.

Also fixed: the transmit watchdog did not cover beacons and could not bound a long
transmission; "Transmit 0%" did not stop a beacon with a Round Robin slot configured; and
switching modes mid-transmission did not release the radio.

Selecting a receive-only mode and pressing Call CQ used to report that calls were going out
while nothing was transmitted. Modes that cannot transmit now say so.

### Fixed

- **A second radio that was switched off could spawn a CAT process every second, forever.**
  There was no retry backoff. On Windows this is expensive process creation plus a 12 MB
  driver library re-scanned by antivirus each time, so it appeared as system CPU rather
  than as Nexus. Retries now back off to once a minute and recover when the radio returns.
- **A decoder crash could silently stop all receive.** The app kept running and the
  waterfall kept painting, so it looked alive while it had gone deaf until restart.
- **A slow decode could delay or prevent a transmission.** Modes other than FT8 and FT4
  waited for the previous period's decode before keying, so the over went out late or, on
  longer modes, not at all. All modes now key at the slot boundary, as WSJT-X does.
- **Mode settings now take effect immediately.** Changing a Q65 period or JT65 submode did
  nothing until you switched modes and back, while the rest of the app reported the new
  value.
- **The Phone cockpit gained its ⊞ Panels menu**, which CW, RTTY and SSTV already had.


### Program tells you when the repeater list is missing a band

The Program section's default source, hearham.com, is an open directory with real holes in rural
country. Around Bozeman MT it lists nine repeaters and not one of them on 2 m, which is not a true
description of Montana. That is worse than a short list: a channel list with no 2 m on it looks
finished, and you find out it wasn't when you key up and nobody answers.

Program now checks the results for a major band with nothing on it at all, and says so, pointing at
the RepeaterBook token in Settings as the fix. It looks for a missing **band**, not a low count —
genuinely empty country stays balanced across 2 m and 70 cm (Amarillo TX has three of each), so
counting repeaters would cry wolf in the plains while staying quiet where the data is actually
wrong. It also counts what the directory *lists* rather than what is on the air, so a town whose
2 m machines are simply off-air, as Fairbanks AK's are, does not trigger it. Checked against the
full 22,574-record hearham feed at eight locations, it fires at one.

### Fixed — the app and the README disagreed about where repeater data comes from

Settings told you the Program section "gets RepeaterBook data through Nexus's shared access
automatically". It does not. Shared access is still pending RepeaterBook's approval, so every
install has been using hearham.com, and the README described a third arrangement again. Both now
say the same true thing: hearham by default, your own RepeaterBook token if you add one, shared
access when and if RepeaterBook approves it.

## [0.19.7] — 2026-07-27

### Decoder: vendored WSJT-X modem sources moved from 2.7.0 to 3.0.2
Nexus builds its FT8/FT4 decoder from WSJT-X's own DSP sources. Those were pinned at WSJT-X 2.7.0;
upstream has since released 3.0.2. This build takes the parts of that update worth having, one
change at a time, each measured against the previous build on identical recorded audio.

Most of it changes nothing you can see, and that is the honest summary: eight of the nine changes
produce byte-for-byte identical decodes. The value is that the decoder no longer drifts from the
reference implementation, which keeps future updates cheap and low-risk.

What does change:

- **Callsigns that cannot exist are rejected.** The 28-bit callsign field can represent strings no
  real callsign could ever be. Those now get thrown out instead of reaching the log. Verified
  against rare-prefix calls (9A1AA, 2E1ABC, 3D2AB, 4X4AA, 8P9AA, KH6ABC) plus short calls, so no
  legitimate callsign is affected.
- **One fewer wrong decode.** The FT8 timing search was clipping at its own boundary and
  occasionally producing a decode from the artifact. Widening it removed a measured false decode,
  at the cost of one very weak signal on the sensitivity floor. A wrong decode reaches the log and
  gets uploaded to LoTW, QRZ and ClubLog; a missed one just means the station calls again.
- **FT4 considers twice as many signals per pass.** Should mean the same or more decodes on a busy
  band.

### Rovers keep decoding
WSJT-X 3.0.2 discards any decode containing `/R` outside contest mode. `/R` is the rover flag —
stations that drive between grid squares during the VHF contests, which is exactly the traffic
worth catching on 6 m and 2 m. Nexus does not take that filter, and there is now a test that fails
if anyone reintroduces it.

### Under the hood
Fixed a build fault where 52 of the decoder's source files were not tracked for rebuilds: editing
one linked a stale library with no warning, so a change could appear to have no effect when it had
simply not been compiled in.

Added false-alarm tests for FT8 and FT4 — the decoder is now checked against pure noise and must
produce nothing at all. Previously the tests only checked that real signals still decoded, never
that silence stayed silent.

## [0.19.6] — 2026-07-26

### TempoFast decoding on a real link
The first two-station Tempo QSO turned up a fault that had been there all along. TempoFast's
decoder cannot look for a signal that arrives EARLY — its timing search starts at zero and goes
forward. FT8 and FT4 both search backwards as well, which is why they were unaffected on the same
radios.

TempoFast was also the one mode that started transmitting at the very beginning of its slot,
sitting exactly on that limit with no room to spare. Any ordinary timing error — the other
station's PC a quarter-second off UTC was enough — pushed frames off the edge, where they are not
merely weak but invisible. About half of all frames were lost in each direction, so short messages
arrived and longer ones never finished assembling.

TempoFast now starts transmitting 0.4 s into its slot, the same way FT8 and FT4 do, which leaves
room for normal clock error on both sides. **Both stations need this version** for a Tempo
conversation to benefit.

If your Tempo contacts have been unreliable, check the clock reading in the top bar at BOTH ends —
a few tenths of a second is invisible to FT8 and was fatal to Tempo.

### Chat messages that never fully arrive
A Tempo message is split into 10-character pieces and reassembled. If a piece never arrived, the
message used to wait for it forever: nothing appeared in the conversation, and nothing said why —
you could see the pieces in Band Activity while the chat window stayed empty.

Now the conversation shows what did arrive, marked **"2 of 3 received"**. Half a message tells you
which half to ask about.

Two stations sending at the same time could also have their pieces mixed into one garbled message,
because messages were matched by number without checking who sent them. They are now matched per
station.

### Pounce: Work is always available
The Work button used to disable itself and explain why — "In a QSO with…" — which replaced the very
button you were reaching for. Whether to leave your current contact to chase a new one is your
call, so the button is always there. It moves the radio and the mode over.

### Waterfall: right-click sets transmit
JTDX's mapping: left click sets receive, right click sets transmit. Shift+click still sets transmit
too, so both conventions work.

### Settings
The collapsible "Advanced" sections were styled like plain labels and easy to walk straight past.
They now look like controls, with a show/hide affordance — the per-radio data-mode setting lives
inside one of them.

## [0.19.4] — 2026-07-26

### Worked stations stop showing as needed
Working a station in a US state you had already worked left it lit in the Needed roster with its
"why you need this" pills, so a worked station kept looking new. One question — what state is this
call in — was being answered by two different sources on the two sides of the same comparison: the
heard side resolved it from the FCC callsign index, while the worked side could only read a state
written into the log, and auto-logged contacts never wrote one. So a worked state could never be
learned. Contacts now carry the state, resolved from the same source both sides use, and existing
contacts are filled in once on first launch.

Your Worked All States **worked** counts will jump the first time you run this. That is the
correction, not a bug — they were understated for every auto-logged contact. Confirmed counts are
unchanged. The state is written into your log and into uploads to QRZ, ClubLog, eQSL and LoTW,
exactly as the country already was.

A contact logged with no grid now reuses a grid you logged for that station before, so a grid you
have already worked stops reporting as new. A station whose grid has never been seen still logs
blank, because a grid that is not known cannot be credited.

### Single-cable interfaces keep CAT
A Digirig Mobile carries CAT and the keying line on one port. Nexus only recognised the opposite
arrangement — a separate keying port, as on an SO2R controller — and everything else fell back to
keying with no CAT at all, while reporting success. The band never followed and nothing said why.
That configuration now keeps full CAT and keying together on the one cable.

Detect recognises Digirig and RIGblaster interfaces, pairs their sound device, and fills in the
keying method. It will not guess which radio is on the other end of a cable, so you still pick your
Rig Model. Auto-test now also tries the radios these interfaces are usually paired with — FT-891,
FT-857, FT-817/818, IC-7100, IC-705, Xiegu G90 and X6100, TS-480.

Keying with no rig model set now says outright that there is no CAT and the radio will not follow
the band, instead of reporting a bare success.

### Connect shows everything by default
Connect had a Basic / Expert detail level, and new installs started on Basic — one plain sentence
per pane. That toggle is gone and every pane now shows its full data. In practice you also get the
map layer panels without switching anything, the modelled band-by-hour chart, more satellite passes
(14 instead of 5) and more contests (20 instead of 8), and the chase feed no longer stops at three
rows. Panes still fall back to a one-line summary while they are waiting on data or a feed is
offline — that part was never the detail setting.

### Map fixes
Opening sectors on the 3-D globe tore into green spikes that stabbed through the Earth. Nothing in
that layer draws curved lines, so the wedge's two long straight edges cut through the sphere and
came back out the other side — on a 3000 km opening they passed about 78 km under the surface. The
wedge is now drawn in short steps that follow the curve. The 2-D map was never affected, because a
straight line on a flat map is straight.

The POTA/SOTA map opened as a flat world map while Chase DX, Ragchew and 6m/VHF all opened as
globes. It is a globe now, like the rest.

### Digital modes can run plain SSB, per radio
Nexus puts the radio in its DATA submode (DATA-U / USB-D) for FT8, FT4, RTTY and SSTV, because on
most rigs that is the only mode where the USB sound device actually reaches the transmitter. That
is still the default and nothing changes unless you go looking for this.

If your transmit audio goes in through the **microphone** instead — an interface wired to the mic
jack, as several RIGblaster models are — there is now a per-radio setting, **Settings ▸ Radio ▸
Data modes use plain SSB**. Nexus then commands plain USB or LSB for those modes and stays there,
through band changes and when you call a station.

It is per radio because it depends on how that particular rig is cabled. On a normally-wired radio
turning it on means the transmitter gets no audio at all — a red TX light and nothing on the air —
so leave it off unless you know your interface needs it. True FSK RTTY is unaffected.

### Fixes
- Logging a contact in Voice and CW shows your previous contacts with that station again — the
  date, band and reports — not just a count of them.
- The keying port of a radio you were not currently operating could be edited and silently not
  saved. It saves.
- Native Flex audio that fails to start, or starts and never delivers any audio, now says so and
  falls back to the sound card. Previously it left you hearing nothing, with silence that looked
  exactly like a dead band.
- Raspberry Pi packages build again; 0.18.0 shipped without them.

## [0.18.0] — 2026-07-25

The last public release was 0.17.12. This gathers everything since.

### The waterfall no longer stalls
Operators reported the waterfall freezing for about a second, over and over, on voice, CW and FT8
alike. The waterfall line was being built by the same part of Nexus that talks to your radio, and
a radio that is slow to answer can hold that up for as long as two and a half seconds. Nothing new
could be drawn for the whole of that time, so the last line was redrawn again and again, which is
the vertical streaking people saw. The waterfall is now built from the incoming receive audio
directly and cannot be held up by the radio at all. The Flex and Icom panadapter displays were
being held up the same way and are fixed with it.

### Nexus can update itself
When a new version is out it downloads quietly in the background, then offers to install. Nothing
installs behind your back and nothing happens on a schedule: the button waits for you, and stands
down while you are transmitting, tuning, in a contact or running CQ, telling you which. Restarting
mid-contact would lose the contact, so it will not. Every update is signed and verified before it
is applied. Windows and the Linux AppImage update in place; the .deb packages, including both
Raspberry Pi builds, are managed by your package system and continue to notify you instead.

### Pounce: know about a new one the moment it appears
Working a rare station is a race, and once the pileup builds you have lost it. Nexus can now score
every skimmer and cluster spot as it arrives and, when something you actually need turns up, play
a distinct tone whether or not Nexus is the window you are looking at, raise a desktop
notification, and show a banner with the call, the country and the frequency. One click works it.
It is off until you switch it on, because how often it would fire depends entirely on how much you
still have to chase. Settings, under Spots and Alerts, explains when to turn it on. Nexus never
touches the radio on its own for this: it tells you, and you decide. The Work button stands down
while you are transmitting or already in a contact.

### PTT follows the radio you switch to
If you key with RTS or DTR on a dedicated port, an SO2R controller such as a u2R or MK2R where each
radio has its own keying line, that port was a single setting shared across every radio. Switching
rigs moved the CAT port but left the keying line pointing at the previous radio, so transmit could
key the rig you had just switched away from. The keying port is now part of each radio's own
configuration and travels with it.

### The operating cockpits hold their shape
In Phone and CW the areas you operate from, the decode, DSP controls and band activity, now have a
guaranteed minimum height that nothing below can take; if the window is short the cockpit scrolls
instead. Typing a callsign used to bring up the station card under the log form and collapse the
whole operating area. That card is now a single line while you are operating, showing the call,
whether they are a dupe or a new one, how many times you have worked them and their name, with the
full card still in the Logbook. Clicking a spot in a cockpit's own band activity no longer throws
you into a different cockpit; the rig moves and you stay where you are. The Needed board and the
map still take you to the matching cockpit, which is what you want there.

### Logging by hand
The manual log form now takes the UTC date and time, so logging a contact after the fact no longer
stamps it with the moment you typed it. It also takes the US state, which Worked All States counts
and which a hand-logged contact has no other way to learn, and transmit power. Editing a contact
that has already gone to LoTW, QRZ, eQSL or Club Log now re-sends it; previously the correction
stayed on your machine and the online logbooks kept the old version with nothing to tell you they
disagreed.

### Under the hood
Incoming skimmer spots cost half as much to process on a busy band, and building the spots list no
longer holds up the rest of the app while it runs.

## [0.17.22] — 2026-07-25 — The operating panes hold their ground

- **The panes you operate from can no longer be squeezed away by what sits below them.** In Phone
  and CW the decode, DSP and band-activity area now has a guaranteed minimum height; if the window
  is too short for everything, the cockpit scrolls instead of crushing them. Previously typing a
  callsign brought up the station card under the log form and the whole operating area collapsed
  to nothing.
- **The station recall card is one line in the operating cockpits.** While you are working someone
  it shows what you glance at — their call, whether they are a dupe on this band or a new one, how
  many times you have worked them, and their name. The full card, with location, notes and your
  complete history with them, is still there in the Logbook.

## [0.17.21] — 2026-07-25 — Clicking a spot keeps you where you are

- **Clicking a spot in a cockpit's own band activity no longer throws you into a different
  cockpit.** Working a spot sends you to the cockpit that matches the spot's mode, which is right
  from the Needed board but wrong from inside Phone or CW: Band Activity shows the whole band, so
  clicking a CW spot from Phone navigated away and the entire Phone view vanished. It looked like
  the layout collapsing. Now the rig moves to the spot and you stay where you were. The Needed
  board and the map still take you to the matching cockpit, which is what you want there.
- **Band activity is visibly its own window**, with a title, sitting apart from the DSP and level
  controls instead of blending into them. They were already separate sections but the dividing
  line was too faint to see against a dark background.
- **The push-to-talk and voice keyer sections take less height**, so band activity gets the room.
  PTT stays a comfortable hold-target — a transmit control you have to aim at is a worse problem
  than a shorter spot list.

## [0.17.20] — 2026-07-25 — Phone's panes are fixed in place

- **Removed the removable/pop-out panels from the Phone cockpit.** The sections under the scope —
  DSP, the RX level controls, Band Activity — are now permanent, each in its own box, and Band
  Activity can no longer be taken out of the main window. Operators reported the whole area
  collapsing and the band activity disappearing when clicking a spot; two narrower fixes each
  corrected a real fault without stopping it, so the machinery that can remove a pane is gone
  from this view. The CW cockpit reached the same conclusion about its drag-to-resize seams
  earlier: in a cockpit you operate from, panes that can move or vanish cost more than they give.

## [0.17.19] — 2026-07-25 — Phone panes stay put, and Pounce starts quiet

- **The DSP controls no longer vanish when you click a spot.** Changing frequency makes Nexus
  re-check what your rig supports, and while that check is in flight the answer is briefly
  "unknown". The Phone view was treating that as "your rig doesn't have these" and removing the
  NB/NR/notch controls and the noise-reduction sliders, which made the area collapse and the band
  activity jump. Once your radio has reported a control, it stays on screen.
- **The panes under the scope are visibly separate now.** DSP, the RX level controls and Band
  Activity were always separate sections but had no boundary between them, so they read as one
  block — which is why one of them disappearing looked like the whole area had gone. Each has its
  own frame.
- **Pounce is off until you turn it on.** It alerts on stations you still need, and how often that
  fires depends entirely on how much you have left to chase: for a well-established log a new
  entity is a rare event worth interrupting for, but earlier on almost every DX spot is a new one
  and the alert would never stop. Rather than guess, it now ships off, and Settings explains when
  to switch it on.

## [0.17.18] — 2026-07-25 — Phone layout, fixed the way CW was

- **Phone's Band Activity keeps its spot lines.** The same fault CW had: panes could be squeezed
  below their own content and then clipped it, so the vertical spot lines vanished. Every pane
  under the scope now holds its content height and the region scrolls instead, with Band Activity
  the one pane that grows. This is the treatment CW got in 0.17.11, applied to Phone.
- **Removed leftover pane-resize plumbing from Phone.** The drag-to-resize seams were taken out of
  CW because they were fragile and added little, but Phone kept the sizing variable behind them.
  With no slider left to correct it, a stale size could still skew the Band Activity pane. Phone
  never showed those seams, so this was machinery that could only misbehave.
- **The extra band/frequency/time fields under "Log a contact from another radio" no longer push
  the log form off the bottom.** They are capped and scroll on their own now, so opening them
  cannot shove the thing you were about to use out of reach on a short window.

## [0.17.17] — 2026-07-25 — Updates that install themselves, and PTT that follows the radio

- **PTT now follows the radio you switch to.** If you key with RTS or DTR on a dedicated port —
  an SO2R controller like a u2R or MK2R, where each radio has its own keying line — that port was
  a single setting shared across every radio. Switching rigs moved the CAT port but left the
  keying line pointing at the previous radio, so transmit could key the rig you had just switched
  away from. The keying port is now part of each radio's own configuration and travels with it.
  The only workaround before was re-loading the radio's profile in Settings by hand.
- **Nexus can update itself.** When a new version is out it downloads quietly in the background,
  then offers to install it. Nothing is ever installed behind your back and nothing happens on
  its own schedule: the button waits for you, and it stands down — telling you why — while you
  are transmitting, tuning, in a contact, or running CQ. Restarting mid-contact would lose the
  contact, so it simply will not. Every update is cryptographically signed and verified before it
  is applied; an installer that has been altered is refused. Windows and the Linux AppImage
  update in place; the .deb packages, including both Raspberry Pi builds, are managed by your
  package system and continue to notify you instead.

## [0.17.16] — 2026-07-25 — Pounce, and hand-logging that keeps the right time

- **Pounce: you get told the instant a new one appears, not when the board next refreshes.**
  Working a rare station is a race — once the pileup builds you have lost it. Nexus now scores
  every skimmer and cluster spot the moment it arrives and, when something you actually need
  shows up, plays a distinct tone (whether or not Nexus is the window you are looking at), raises
  a desktop notification, and puts a banner up with the call, the entity and the frequency. One
  click works it. Deliberately rare so it stays worth trusting: the default is all-time-new DXCC
  entities only, with new zone and new state available as wider settings, and each station alerts
  once per band and mode. Set it under Settings, Spots and Alerts; it can be turned off entirely.
  Nexus never touches the radio on its own for this — it tells you, and you decide. The Work
  button stands down while you are transmitting or already in a contact, and says so.
- **Hand-logged contacts keep the time they actually happened.** The manual log form now takes the
  UTC date and time, so logging a 2 m contact after the fact no longer stamps it with the moment
  you typed it. It also takes the US state (which Worked All States counts, and which a
  hand-logged contact has no other way to learn) and transmit power.
- **Editing an already-uploaded contact re-sends it.** Previously the correction stayed on your
  machine and the online logbooks kept the old version, with nothing to tell you they disagreed.

## [0.17.15] — 2026-07-25 — The waterfall is drawn where the audio arrives

- **The waterfall is no longer built by the part of Nexus that talks to your radio.** This is the
  real fix for the periodic stall; 0.17.13 and 0.17.14 each addressed a piece of it and neither
  was the cause. The waterfall line was being computed by the same thread that sends and receives
  every CAT command, and a radio that is slow to answer can hold that thread for up to two and a
  half seconds. Nothing new could be drawn for the whole of that time, so the last line was
  redrawn over and over, which is the vertical streaking operators reported. The line is now built
  on its own from a direct copy of the incoming receive audio, and it cannot be held up by the
  radio at all. What it means on the air: the waterfall keeps scrolling no matter what the radio
  is doing, on voice, CW and FT alike.
- **The Flex and Icom panadapter displays were being held up the same way**, even though they
  already had their own connections. They now publish independently too.
- **A dead audio device stops the waterfall cleanly** instead of leaving the last line frozen on
  screen looking like live signal.

## [0.17.14] — 2026-07-25 — The waterfall stall, properly this time

- **The waterfall stops stalling every 30 seconds.** 0.17.13 attacked the wrong half of this. The
  display was not waiting on anything; the radio loop was, so no new waterfall line was being
  produced and the last one got drawn over and over, which is the vertical streaking operators
  reported. The cause: Nexus asks the radio whether it supports each DSP function (noise blanker,
  noise reduction, notch, compression, VOX), one per cycle. A radio that does not cleanly answer
  one of those makes Nexus wait up to two and a half seconds for a reply that never comes, and
  that wait happens on the same thread that draws the waterfall. Worse, a function that had been
  given up on was retried every 30 seconds for the whole session, so the stall came back forever.
  Retries now back off, from 30 seconds out to about half an hour, and reset the instant the radio
  answers. What it means on the air: a rig that is quiet about one of its DSP functions no longer
  costs you a frozen waterfall every half minute.

## [0.17.13] — 2026-07-25 — The waterfall stops freezing

- **The waterfall no longer hangs for a second at a time.** Operators reported it stopping dead
  for about a second every 10 to 20 seconds, in voice, CW and FT alike, right from launch. The
  waterfall row was being read through the same lock that guards the whole application state, and
  that lock is held while the radio is commanded over CAT at each 15-second slot boundary. A CAT
  round-trip takes up to a second on a slow serial link, and the waterfall sat waiting for the
  whole of it, drawing nothing. The row is now published separately, so the display never waits on
  radio or logbook work again. What it means on the air: the waterfall scrolls smoothly and keeps
  scrolling, whatever else the app is doing.
- **The spot buffer costs less to fill.** Every incoming skimmer spot was scanned against the whole
  buffer twice; it now takes one pass. On a busy band with the RBN firehose running, that halves
  the work done on the app's busiest data path.
- **The spots list no longer blocks the rest of the app while it is built.** It held the shared
  application lock across the entire build, so the waterfall and every other status read queued
  behind it. It now takes what it needs and lets go first.

## [0.17.12] — 2026-07-25 — Dual-radio setup, honest rig mode, FT exchange fields

- **Setting up a second radio no longer overwrites the first one's COM port.** Pressing *Test CAT*
  or *Auto-test* while editing a radio you are not operating on used to save that radio's port,
  model and audio devices onto your **active** radio's profile, silently and permanently, leaving
  both radios pointing at one set of ports. Every write from the rig form now goes to the radio the
  form is actually describing. On the air: your two rigs stay two rigs.
- **Auto-test now probes for the radio you are configuring.** It seeded every port with the *active*
  radio's Hamlib model, and an Icom only ever answers at its own CI-V address — so with two radios
  set up, the second one's port could never answer and Auto-test kept handing back the first radio's
  port. It also no longer claims a CAT test passed when the test it ran was on the other radio.
- **The top bar tells the truth about your rig's mode.** Its USB/FM buttons stopped reaching the
  radio back in June, when the transmit path moved to per-section modes. Clicking FM could not
  command FM; all it did was force a retune that re-asserted the section's own mode, which is what
  dragged a rig sitting in FM into USB/USB-D. The dead buttons are gone, and when your radio is
  actually in a different mode than Nexus thinks, the top bar now says so (`rig: FM`) instead of
  confidently printing the wrong one.
- **FT8/FT4: the DX call and grid fill in however the QSO started.** They were only ever populated
  by a single click on a decode row, so working a caller any other way — the Work/Call buttons, the
  roster, Shift+Enter, JTAlert/GridTracker, or a station simply answering your CQ while the
  sequencer handled it — left the exchange panel blank, with Tx1–Tx4 showing "—" and the Tx buttons
  dead, even though the QSO ran and logged correctly. They now track the live QSO, and the grid
  resolves exactly the way the logged GRIDSQUARE does. This also removes a real hazard: pressing a
  Tx row while a stale call was showing could retarget the contact to the wrong station.
- **RST_SENT no longer goes missing when you work a station that answered your CQ.** The report the
  sequencer had already armed was being discarded at the moment you clicked, and the only other
  place that captured it does not run during your own transmit slot — so the contact logged with a
  blank sent report. This is the "the log has it right in almost every case" case.
- **CI runs in minutes again.** The 15 SSTV transmit/receive loopback cases were built unoptimized
  and each took over a minute, pushing the test job past an hour and starving the gates queued
  behind it. The DSP crates are now optimized under `cargo test`: the same suite runs in 13 seconds
  with every case and every assertion intact.

## [0.17.11] — 2026-07-25 — Decode-first CW cockpit + cross-mode layout fixes

- **The CW decode transcript is now the dominant pane.** It grows to fill the space under the
  waterfall and floors large, so the live decode is the biggest thing on screen instead of the last
  one fighting for room. What it means on the air: you can actually read a run of copy without the
  decode being a two-line sliver.
- **Removed the CW inter-pane resize sliders** (the drag-seams between Band Activity / Copilot /
  Decode / Sent added in 0.17.4). They proved low-value in CW and made the layout fragile; the CW
  lower region is now a simple, predictable stack. Removable panels (⊞ menu) and the
  waterfall-height slider stay. (SSTV keeps its seams.)
- **CW copilot is Expert-only.** The Guided/Expert selector box + bar are gone, reclaiming that
  vertical space for the decode; the copilot is just the decoded-call chips.
- **Panes no longer step on each other (CW / Phone / RTTY).** A layout audit across every cockpit
  fixed a class of bug where a side pane got crushed below its content and clipped: the CW Band
  Activity spot lines were covered when the decode was on; Phone's control panes + spot strip could
  be cut off with the DSP panes open; RTTY's Stop/Send could be clipped off the bottom. Panes now
  keep their size and the region scrolls instead of covering. SSTV and Operate were already correct.
- **Fixed the "First contact — new station" status line** cluttering the log area (it duplicated the
  Previous-contacts list) and tightened the F-key + log spacing so the decode gets the height.

## [0.17.6] — 2026-07-25 — WSJT-X-tight decode rows

- **FT8 decode rows are now a single tight line each** (Band Activity / Rx Frequency), like WSJT-X.
  The per-row **Work button is gone** — double-click a decode to work it (the row already worked
  that way) — which removed the second line every decode was carrying, and the QRZ chip no longer
  forces a 28px row height. You now see many more decodes per screen.

## [0.17.5] — 2026-07-25 — Left rail scrolls instead of overflowing

- **The left mode rail no longer overflows.** With many sections enabled, the icons used to grow
  out of view and push the layout. Now the mode-icon column scrolls within the rail (thin
  scrollbar) while the bottom cluster (settings, etc.) stays pinned and always reachable — the rail
  keeps its width and the rest of the UI never shrinks or scrolls to accommodate it.

## [0.17.4] — 2026-07-25 — Panels everywhere: CW + RTTY

- **CW panels.** The waterfall stays pinned with the keyer / macros / send / log always reachable
  below; the scope controls, DSP toggles, RX DSP levels, TX meters, and the four content panes
  (Band Activity, Copilot, Decode, Sent) are removable, and you can drag the seams between the
  content panes to size each one.
- **RTTY panels.** The decoded-text stream is now removable via the ⊞ Panels menu.
- Panels are now everywhere under the waterfall — Operate, SSTV, Phone, CW, and RTTY — with TX
  controls locked in place in every cockpit by construction.

## [0.17.3] — 2026-07-25 — Panels reach Phone; tighter decode rows

- **Phone panels.** The bandscope stays pinned on top with the PTT row / voice keyer / log always
  reachable below; the rig-scope controls, DSP toggles, RX DSP levels, TX meters, and Band Activity
  are now removable (⊞ Panels menu), and Band Activity fills the space when you hide the rest.
- **WSJT-X decode density.** FT8 decode rows in Band Activity and Rx Frequency were far too tall;
  they're now a tight single line each (like WSJT-X), so you see many more decodes at once.
- Panels rollout continues: SSTV + Phone done, CW and RTTY next.

## [0.17.2] — 2026-07-24 — Removable + resizable panels reach SSTV

- **SSTV panels.** The RX image stays pinned at top with the transmit bar (mode / Send / Stop /
  progress) always reachable below it; the **Transmit composer** and the **Gallery** are now
  removable (⊞ Panels menu) and drag-resizable at the seam between them. First cockpit in the
  "panels everywhere under the waterfall" rollout — Phone, CW, and RTTY follow.

## [0.17.1] — 2026-07-24 — Settings & auto-detect + a batch of needed/roster fixes

This release reworks Settings and radio auto-detection end to end — the setup flow that new
operators hit first, and the multi-radio configuration that was the clunkiest part of the app —
plus a batch of needed-intelligence, roster, and FT-sequencing fixes.

**Needed & roster**

- **"Sort by need" now ranks states above grids.** The chase gradient is force-ranked
  consistently everywhere — Wanted > new DXCC/ATNO > new zone > new state > new grid > new band —
  so the most valuable need surfaces first (a genuinely rare grid still floats up via its rarity
  boost). Fixed across the backend and every board that had drifted out of sync.
- **New-zone floods stop once you've worked all zones.** The board no longer keeps flagging
  per-band "new CQ zone" slots once you hold complete any-band Worked-All-Zones; zone-chasers still
  working toward WAZ keep seeing them.
- **A worked station drops off the roster immediately.** Logging now refreshes the needed board at
  once instead of leaving the just-worked call flagged for up to 30 seconds.

**FT operating**

- **Calling a station now stops after 8 unanswered overs.** In FT8/FT4 search-and-pounce, calling a
  station that goes silent used to repeat indefinitely (only the 6-minute watchdog stopped it). It
  now stalls after 8 overs (adjustable); Resend re-arms it. CQ behavior is unchanged.

**Waterfall & layout**

- **FT8 waterfall defaults to the Turbo palette, with a black background** (the low end was a dark
  maroon).
- **Resizable side-rail panes in Operate (roster mode).** Band Activity and Rx Frequency can be
  drag-resized at the seam between them, and Rx Frequency auto-fills the rail when Band Activity is
  removed — no more being pinned to a small box.
- Tightened the spacing of the "log a contact from another radio" line so it eats less room.

**Settings & auto-detect** (from the 0.17.0 work)

This reworks Settings and radio auto-detection end to end — the setup flow that new
operators hit first, and the multi-radio configuration that was the clunkiest part of the app.

- **Settings went from 14 tabs to 8.** Grouped into Station, Radio, Modes, Frequencies, Spots,
  Logging, Contesting, and Appearance. The catch-all "Features" tab is gone — its switches moved to
  where they belong (Field Day's master toggle now lives on Contesting).
- **Per-radio configuration no longer hijacks your active radio.** Editing a radio profile used to
  silently switch the app onto that radio. Now "Configure" edits a radio's settings in place and
  "Make active" is a separate, deliberate action — so setting up radio 2 doesn't take you off
  radio 1.
- **A setup-health strip** shows Rig / RX / TX status at a glance, with a **"Prove TX"** button that
  keys the radio briefly (with a confirmation) so you can confirm transmit is wired correctly
  without guessing.
- **Auto-detect fixes.** Detected radios now suggest the correct **transmit** audio device (it was
  pairing TX to the wrong output — audio came out the speakers); Flex radios fill in their IP
  correctly; port auto-testing chains through candidates instead of stopping at the first; and a
  detection failure now surfaces an error instead of looking like "nothing found."
- **Decode depth moved to the Operate cockpit.** Fast / Normal / Deep is now a set of chips right in
  the operating view, so you can trade decode sensitivity against CPU on the fly instead of digging
  into Settings.

## [0.16.4] — 2026-07-24 — APRS gets its own TX-enable

- **The APRS window now has a TX On/Off toggle.** This view hides the top bar's transmit controls,
  so there was no way to enable TX from APRS — a beacon or message was silently gated off with
  *"TX is off"* on a fresh launch (TX defaults off and isn't remembered). RTTY/SSTV already carry
  their own; APRS now does too.

## [0.16.3] — 2026-07-24 — APRS frequency dropdown tunes on select

- **Picking an APRS frequency now tunes the rig immediately** (band-picker behavior) instead of
  only setting a selection you then had to "Tune". The button remains as an explicit **Re-tune**.

## [0.16.2] — 2026-07-24 — APRS defaults to your VHF radio on entry + shows the dial

- **Opening APRS now defaults to your 2 m radio.** Entering the APRS section auto-tunes: it hands
  off to the 2 m-capable rig (e.g. the IC-9700), lands on the selected APRS frequency, and sets FM —
  you no longer have to click Tune first. (Still RX-only; nothing keys.)
- **APRS shows its own dial readout** (`144.390 MHz · 2m · FM`) in the header, since this view hides
  the top bar's frequency readout — so you can see the hand-off and tune actually land.

## [0.16.1] — 2026-07-24 — Rebuild so testers can confirm the 0.16.0 fixes

Same content as 0.16.0. The first 0.16.0 installer was built *before* the APRS radio-switch/FM
fix and the CAT-diagnostics landed, but carried the same version number — so a tester who installed
it saw the pre-fix behavior and couldn't tell the builds apart. 0.16.1 exists purely so the wordmark
is an unambiguous marker: **if it says 0.16.1, you have the APRS Tune → FM + VHF-radio-switch fix,
the FT-chrome removed from the APRS window, and the model-aware CAT-failure message.**

## [0.16.0] — 2026-07-24 — APRS messaging (send, threaded, auto-ack) + decode coverage

Rounds out the APRS feature after a completeness review, and cuts a minor release.

### Added

- **Send APRS text messages.** The APRS cockpit has a Message box: enter a callsign and up to 67
  characters and send. Each message carries a rolling line number so the recipient can acknowledge
  it — same up-front TX gate as a beacon (TX must be enabled and the frequency in your privileges),
  so nothing keys unexpectedly.
- **Auto-acknowledge.** An incoming message addressed to your callsign that asks for an ack is
  acknowledged automatically — but only when TX is enabled and allowed; with TX off, Nexus stays
  silent (RX-only), exactly as before.
- **More decode coverage.** Compressed position reports (base-91), object reports (`;`), and
  third-party / I-gated traffic (`}`) now decode to the real originating station.

### Changed

- **Messages are threaded, not collapsed.** Received messages get their own chronological list
  instead of being folded into the sender's position row, so a multi-line exchange all shows
  (previously only the last message per station survived).

### Fixed

- **APRS Tune now switches to your VHF radio and sets FM.** On a dual-radio (HF + VHF) setup,
  tuning an APRS frequency hands off to the 2 m-capable radio and puts it in **FM simplex** — APRS
  isn't a Phone/Digital section, so it previously kept the prior mode (DATA/USB) and the packet
  never decoded. FM is band-guarded, so it never follows you onto another band.
- **The APRS window no longer shows the FT8/FT4 tier chrome** (it's a packet mode with its own
  band picker) — same treatment as RTTY/SSTV.
- **Clearer CAT failures.** When the rig stops answering, Nexus now says *which* rig, on *which*
  port and baud, isn't responding — and for an Icom points at the two-USB-port / CI-V-baud gotcha —
  instead of a silent reconnect loop. The rig-control diagnostic also captures rigctld's own error
  output and the launch config, so a "rig never answered" fault is finally diagnosable.

## [0.15.24] — 2026-07-24 — Native Flex, the rest of it (meters, slice, DAX TX)

### Added

Rounding out native FlexRadio support (all **opt-in, off by default**, and **unverified on
hardware** — for testers with a Flex):

- **Native meters.** With the native panadapter on, the S-meter, forward power, SWR, and ALC read
  straight off the radio's VITA stream (no CAT polling).
- **Native DAX TX audio.** With the native-DAX-audio toggle on, your *transmit* audio also goes to
  the rig over the network (VITA-49 DAX) — the driverless, RDP-proof complement to DAX RX. The TX
  schedule/timing is unchanged; it's the same audio on another route.
- **Slice awareness.** DAX binds the *active* receive slice instead of assuming slice A, so it's
  correct on multi-slice setups.

## [0.15.23] — 2026-07-24 — APRS station roster, native Flex DAX audio

### Added

- **Native FlexRadio DAX RX audio (early access).** Settings ▸ Rig, for a network Flex, now has a
  "Flex native DAX audio" toggle: take the rig's receive audio straight off the network (VITA-49
  DAX) instead of the "DAX Audio RX" sound device — which is invisible under Remote Desktop.
  Decoders read the rig's audio directly. RX-only, opt-in, off by default; unverified on hardware
  (turn it back off if decodes stop).

### Changed

- **The APRS list is a station roster now.** Instead of a firehose of repeated packets, it shows
  one row per station (latest position), newest first, with a distance + bearing column from your
  grid.

## [0.15.22] — 2026-07-24 — APRS, and an Icom auto-test fix

### Added

- **APRS (AFSK-1200 packet).** A new APRS section (Digital group) monitors the band and decodes
  position reports, Mic-E (what most mobile/tracker radios send), messages, and status packets —
  showing who, where, speed/course, and their comment. You can also send a **position beacon**
  (your grid pre-fills the coordinates; pick a symbol, add a comment and digipeater path). RX-first
  and self-contained; a beacon is an explicit, gated one-shot send. Tune to 144.390 FM (NA).

### Fixed

- **CAT Auto-test finds an Icom set to 19200.** The IC-7300/7610/9700 auto-test seeds now try both
  115200 and 19200 baud, so a rig whose CI-V USB baud isn't the default still connects.

## [0.15.21] — 2026-07-24 — Mode designation on the boards, one clean Spots filter

### Added

- **The Needed board now names the specific digital mode.** An FT4 opportunity reads **FT4**,
  an FT8 one reads **FT8** (RBN skimmer wire), instead of both showing "Digital." FT4 and FT8 of
  the same station/band are listed as separate rows, and clicking a board row switches the
  decoder to that mode. The Digital filter chip still governs all of them.

### Changed

- **The Spots panel has ONE mode filter now.** It used to show two overlapping rows with
  opposite behavior (one hid a class, the other showed only a submode, and they duplicated
  CW/Phone/Digital). Now it's a single row of the modes actually on the band (CW, Phone, FT8,
  FT4, RTTY, …) — every chip a plain show/hide toggle, all on by default.

## [0.15.20] — 2026-07-24 — Pause + 3D on the Voice/CW scope, FT4 spot fix

### Added

- **Pause, rewind, and 3D on the Voice and CW rig scope too.** The ⏸ (pause + mouse-wheel
  scrollback) and ◭ (3D stacked-spectrum) buttons that arrived on the FT8/Tempo waterfall now
  live on the Phone and CW cockpit scope. Because that scope is a panadapter (live trace on top,
  waterfall band below), the 3D view *maximizes* — it hides the trace and draws the stacked
  spectrum over the whole panel; ▤ brings the trace back. Your choice is remembered per window.

### Fixed

- **Clicking an FT4 spot now switches the decoder to FT4.** Previously it tuned to the right
  frequency but left the decoder on FT8. The spot's specific mode is now honored, so FT8↔FT4
  follows the spot you click (spots list / cluster / cockpit spot panels).
- **The waterfall's ⏸ / ◭ / pop-out buttons no longer get clipped** off the docked Operate
  cockpit when the panel is narrow — the header wraps instead of hiding controls.

## [0.15.18] — 2026-07-24 — A waterfall you can pause and rewind, plus a 3D view

### Added

- **Pause and scroll back through the waterfall.** Hit ⏸ and roll the mouse wheel to look
  back through the last few minutes of the band — a time tape down the right edge shows how far
  back you are. Great for "did anyone call while I was logging?" History keeps recording while
  paused; ▶ snaps back to live.
- **3D stacked-spectrum view.** The ◭ button flips the waterfall into a rolling perspective
  "3DSS" display — the last ~96 lines stacked front-to-back, newest across the front. An
  alternate way to read band activity at a glance. (Ported, with attribution, from AetherSDR.)

### Changed

- **The waterfall renders from data now, not pixels.** Switching palettes recolors the WHOLE
  visible waterfall instantly (not just new lines), zooming and resizing re-render without
  smearing, and — the quiet win — the per-line canvas readback that caused the "everything gets
  laggy" stall on laptop GPUs is gone. Same treatment on the Phone/CW scope's waterfall band.

## [0.15.17] — 2026-07-24 — CW follows the band, pop-out Memories, live-now roster

### Fixed

- **CW now follows the band sideband convention** — CW-L (reverse) on 160/80/40 m, CW-U at
  30 m and up. 40 m CW was commanding CW-U.
- **The FT Stations panel shows who's on the band NOW** — a station drops off after 3 missed
  decode cycles (the Call Roster rule) instead of lingering for minutes on time buckets. The
  Tempo chat roster keeps its long retention (store-and-forward needs it).

### Added

- **Memories pops out into its own window** (↗ Pop out) — like Needed/Connect/Operate; edits
  sync live between windows.

## [0.15.16] — 2026-07-23 — Tempo chats like a chat app now

### Changed

- **Tempo stops "sending and sending."** A chat message now transmits a bounded number of
  cycles (default 3; Settings ▸ Auto-CQ) with a real 16-second listening gap after each burst,
  then shows **"no ack"** — tap the bubble to re-send, no re-typing. Resends also stop the
  moment the other station **answers** (shown as *confirmed*) or their **ACK** arrives
  (**Delivered ✓** — still the only source of that checkmark). After every burst Tempo yields
  two of its own transmit slots to listening, so a conversation alternates like a real chat.
  The chat **CQ run stops after 10 unanswered calls** instead of calling forever, and an
  unanswered Tempo QSO step gives up cleanly after 6 overs. Message bubbles now show the real
  lifecycle: *waiting → sending (try k) → confirmed / Delivered ✓ / no ack*.
- **Working an FT1/Tempo station from a decode alert now opens the Tempo conversation** —
  it no longer wrongly launched the FT8 call sequence.
- **TempoDeep chat is a first-class citizen:** its messages can now be marked delivered,
  fold into conversation threads, and get a 5-cycle resend budget (it was unbounded before).
- **FT8/FT4 are untouched** — their WSJT-X transmit behavior is now pinned by a byte-level
  golden test that fails if anything perturbs it.

## [0.15.15] — 2026-07-23 — CW keying fidelity, no more log-click window jump, and Memories grouped by band

### Fixed

- **CW: a deliberate send always transmits.** After Stop TX, hitting an F-key macro (or typing CW
  and sending) did nothing until you switched contact/band and back. CW is manual keying — the
  key press *is* the transmit action, so it now always keys (privilege permitting). The FT8
  auto-sequencer is untouched.
- **Clicking a contact in CW no longer snaps the window down.** The log prefill focused the RST
  field, which scrolled the log into view — yanking you down from the decode feed every time.
  It now readies the RST field without moving your scroll.

### Changed

- **Memories are grouped by band — HF, then VHF/UHF.** The channel list (on the main Memories
  screen and inside each pack) now organizes into clean HF (< 30 MHz) and VHF/UHF sections.

## [0.15.14] — 2026-07-23 — Run two radios at once, a New-State hint on every spot, and a much richer Memories section

_A batched release consolidating the work since 0.15.1 (0.15.2–0.15.11)._

### Added

- **Run two radios at the same time.** Nexus can now launch a second full instance pointed at a
  second rig, each with its own settings, while both share **one logbook**. A launch picker lets
  you choose which radio a window drives — no shortcuts or command-line flags. The shared log
  reconciles field-by-field (a contact edited in one window is merged, not clobbered, in the
  other), and each window keeps its needs fresh as the shared log changes. Set a portable/NAS log
  location with `NEXUS_DATA_DIR`.
- **"New State" now lights up on cluster, CW, and SSB spots — not just FT8.** Those spots carry a
  callsign but no grid, so a needed US state used to stay invisible. Nexus now ships a compact
  **callsign→state index** (built from the FCC license file) that resolves the licensed state
  precisely — no 4-character-grid border guessing. It downloads on first launch and refreshes
  itself; Settings ▸ Confirmations has a manual **Update now** button.
- **A much bigger Memories section — 11 curated packs, 172 channels.** One-click installable sets
  for FT8/FT4, digital watering holes (JS8, PSK31, RTTY, SSTV, VarAC), CW & QRP, EmComm, HF nets,
  VHF+ weak-signal, satellites, POTA/SOTA/WWFF, DX & contest, and reference (time signals,
  beacons, WEFAX). Re-installing a pack refreshes its channels without touching ones you've edited.
- **Per-band VUCC and IOTA awards.** VUCC grid-square progress is tracked per band with its own
  Awards card and a grids-by-band panel; IOTA (Islands On The Air) is parsed, exported, and shown
  as an award.
- **Live TX meters in the CW and Operate cockpits.** The power / SWR / ALC metering that was
  Phone-only now shows while you transmit in CW and the digital Operate cockpit too.
- **Click a callsign to open QRZ.** In the Spots board, Needed board, and decode feed.
- **CAT Auto-test now finds the IC-7610 and IC-9700.** Each Icom answers CI-V only at its own
  address, so the auto-detect sweep now seeds those two models (not just the IC-7300) — and the
  "found the port but not the model" hint no longer says "common on Yaesu" to Icom/Kenwood/Elecraft
  operators.
- **The app version shows under the Nexus wordmark** (top-left), so you can tell at a glance which
  build you're running.

### Changed

- **The FT waterfall defaults to the familiar 0–3 kHz view** (the WSJT-X span), with the full-width
  view still one click away.

### Fixed

- **The two-radio launch picker can't trap you anymore.** If you turned multi-radio on, the picker
  showed on every launch — and because it blocked the base window's Settings, turning it back off
  never took. Now the off toggle works from any window, and the picker itself has a **"Use one
  radio (follow bands)"** escape that drops straight into the single-window band-following mode.
- **ADIF import no longer silently drops QSOs.** Imports deduplicated on the UTC *day*, so a second
  contact with the same station on the same day could be discarded. Dedup is now on the exact time,
  and the store-and-forward path keeps its journal — no more quiet log loss.

## [0.15.1] — 2026-07-22 — A nav rail you can reorder, per-mode power limits, a clearer decode feed, and a batch of quiet fixes

### Added

- **Reorder the left nav rail.** Drag the situational/logging section icons (Connect, Needed,
  Spots, Logbook, Awards, Stats…) into whatever order you want; it sticks across restarts, and a
  **Reset order** button appears once you've customized. The operating group (Phone/CW/Digital)
  and Settings keep their fixed spots. *(Fixing this surfaced that drag-and-drop was dead
  app-wide — see Fixed.)*
- **Per-mode maximum-power ceiling.** Settings ▸ Rig now takes a separate power cap for Phone,
  CW, and Digital. Set one and Nexus clamps commanded RF power to it — and re-clamps when you
  switch *into* a capped mode from a hotter one. A safety rail for the duty-cycle-heavy modes so
  a full-power SSB setting can't carry into an FT8 or RTTY session.
- **US state borders on the Logbook globe.** The 3-D "world of contacts" globe now draws state
  lines under your contact dots, so you can read which state a dot sits in — the same reference
  layer Connect uses.
- **DXCC vs BAND in the decode feed.** The old highlight tagged any entity new on the current
  band as `DXCC`, so an entity you'd worked before on another band looked identical to a genuine
  new country. Now a true all-time-new one shows **DXCC** (magenta, matching the Needed board's
  NEW ONE) and a new band-slot shows a dimmer **BAND** (orange) — a band-slot never masquerades
  as a new country again.
- **Log a contact from another radio.** The "Log this QSO" form now has editable band, frequency,
  mode, and UTC time, so a contact made on a rig Nexus isn't driving can be logged correctly.

### Changed

- **The Logbook map is the 3-D globe only.** The 2-D flat map was removed — the globe is the map.
- **The Needed board is band- and privilege-aware.** A grid or entity worked on 20 m reads as new
  again on 2 m (per-band, as awards are counted), and a spot you don't have TX privileges for is
  no longer flagged as a "need."

### Fixed

- **FT8: the closing 73 now goes out before auto-CQ resumes.** When a caller answered your CQ
  with a bare report, Nexus could jump straight back to calling CQ without sending the final 73.
  Fixed and **confirmed on the air.**
- **Drag-and-drop worked nowhere in the app.** Tauri's OS-level drag-drop handler was intercepting
  every HTML5 drag before the page saw it; it's now disabled on the main window (the app uses no
  OS file-drop, so nothing else is affected).
- **A zero FREQ is omitted on export.** A `FREQ 0` in exported ADIF made downstream loggers
  (Swisslog and others) reject the imported QSOs — the likely cause of contacts "missing" after
  an import.
- **The raw logbook is backed up on load.** A lossy ADIF parse could permanently truncate the
  log; a `.bak` is now written before load so the original is always recoverable.
- **FM stopped following the operator down to HF** — changing bands no longer commands FM on 20 m.
- **Two windows no longer fight over layout.** Per-window (surface-scoped) browser storage, so a
  popped-out window keeps its own arrangement instead of overwriting the main window's.
- **Activity-by-hour** no longer piles time-less imported QSOs at midnight.
- A caller's **grid is backfilled from the roster** when they answer your CQ with a bare report.

### Under the hood

- The per-chain decoder foundation for multi-radio (Phase 1a) landed but stays **inert** — no
  behavior change; groundwork for simultaneous decode across radios in a later release.

## [0.15.0] — 2026-07-21 — TempoFast & TempoDeep, panels you can remove, DXKeeper, and two silent data-loss bugs found

### Fixed — two ways QSOs were quietly being lost

- **A QSO rejected by LoTW was stamped "sent" and never retried.** Nexus invokes TQSL with
  `-x -a compliant`, which sets `ignore_err`, so a record TQSL refuses is skipped **silently
  and unidentified**. Exit 9 (some suppressed) was mapped to `Pending` and exit 8 (none
  processed) unconditionally to `Duplicate` — both count as *sent* — and one outcome is stamped
  across the whole batch. The rejected QSO therefore left the unsent list permanently while
  never reaching LoTW. Exit 9 is now `Rejected`, and exit 8 stays `Duplicate` only when the
  stderr shows no rejection. Re-offering an accepted QSO costs nothing (LoTW dedupes); losing
  one is forever. **This was never mode-specific — it could swallow any rejected record.**
- **POTA park references never reached HRDLog, or anything else keying on `POTA_REF`.** Exports
  wrote only `SIG`/`SIG_INFO`, the older overloaded convention that WWFF and special events
  also use. ADIF 3.1.4 added dedicated `POTA_REF`/`MY_POTA_REF` precisely to disambiguate it.
  Now both go out. The giveaway that this was an oversight rather than a choice: our own
  importer already *read* the dedicated fields. We were reading modern and writing legacy.

### Added — panels you can actually remove

- **A panel can now be removed outright**, not merely popped out to another window. `⊞ Panels`
  in the Operate header: untick and it is gone — no placeholder, no window, and the decode
  lists and roster grow into the space. It stays gone across restarts. Removable: waterfall,
  Band Activity, Call Roster, Rx Frequency, Stations, Tx Messages.
  Because the component truly unmounts, a removed waterfall also stops its 120 ms spectrum
  poll — a small performance win, not only a space win. **Undo last change** and **Reset
  layout** ship in the same menu, so there is no state you can strand yourself in.
  Layout is per-surface, so a popped-out Operate window keeps its own arrangement.
- **DXKeeper (DXLab Suite) integration.** Settings ▸ Integrations. Each logged QSO is pushed
  to DXKeeper's TCP Network Service.
  Note the field asks for the **Base Port** (default 52000), matching DXKeeper's own config
  panel — DXKeeper listens on base **+1**, and nothing listens on the base itself, which is why
  "use port 52000" is such a common report. The hint shows the resolved port live.
  Uploads default OFF, since Nexus already pushes to LoTW/eQSL/ClubLog/QRZ and enabling both
  would upload every QSO twice to four services.
- **State and Country are editable in Log this QSO.** Both were always auto-filled from the
  QRZ lookup and written to the record — they were simply never shown, so correcting a
  misheard state meant logging the QSO and then editing it in the Logbook.

### Changed — FT1 is now TempoFast, DX1 is now TempoDeep

- The two native protocols are renamed throughout: on screen, in the logbook, in the source
  tree, and in the build. Nothing about the on-air protocols changed — grep confirms neither
  name ever appeared in a transmitted payload, so a station worked before the rename is
  unaffected.
- **TempoFast QSOs now upload to LoTW as `MODE=MFSK` + `SUBMODE=TEMPOFAST`.** The ADIF Mode
  enumeration is closed, so the previous bare `<MODE:9>TempoFast` was rejected outright by TQSL
  ("Invalid MODE") — a TempoFast QSO could not have been confirmed anywhere. MFSK is the honest
  family, not a flag of convenience: TempoFast is 4-CPM h=1/2 BT=0.3, the same continuous-phase
  FSK family as FST4, which already lives under MFSK. Your local logbook still records
  `TempoFast`, because MFSK would erase the distinction from TempoDeep.
  Verified against live LoTW `config.xml` v11.34: MFSK resolves to the accepted `DATA` group.
- **Band-edge tones moved from Digital to Rig settings.** The cue already fired on phone and CW
  identically — it was only grouped under Digital by accident.
- **POTA/SOTA spots are sortable** (workable-now, activator, reference, band, mode), and the
  Sort / Band / Program / Mode filters now survive leaving and returning to the view.

### Fixed — other

- **POTA/SOTA default sort was inverted**, putting the least workable activators on top. The
  arrow glyph also disagreed with the list on that one key.
- Closed a latent `.bss` overflow in the FT8 a7 path. `ft8::decode_frame` documented itself as
  "a7-inert" while passing `a7_final = true`, so its decode counter grew unbounded; `msg0` is
  byte-adjacent to `jseq` in `.bss`. Unreachable in production, but one future call site away
  from memory corruption.

## [0.14.0] — 2026-07-21 — Read-only launch, a 3-D logbook globe, on-time FT8 transmit

*(Backfilled: 0.14.0 shipped on all five artifacts but was never written up here.)*

### Changed — launching Nexus no longer touches your rig

- Nexus now opens the radio **read-only**: it reads the actual frequency and mode and displays
  them, and commands nothing. Park on 40 m LSB for a net, open Nexus, and the rig stays put.
  The first command happens when *you* act. Underneath, every transmit path now asserts the
  correct mode immediately before keying, so a transmit can never silently key into the wrong
  mode.
- **FT8 transmits on the slot boundary**, like WSJT-X. Previously Nexus finished decoding the
  prior slot before keying, costing ~1 s of your own over. Decoding now runs in parallel.
- **TX audio is a clean, flat signal.** The transmit path gained a proper anti-aliased
  resampler; the FT8/FT4 envelope previously carried a periodic amplitude ripple.

### Added

- **A 3-D globe of your contacts on the Logbook** — every worked grid a band-coloured dot, with
  a per-band (VUCC-style) picker. It fully unloads when you leave the Logbook.
- **Tempo messages survive restarts**, and a reply to a just-decoded station now transmits on
  the next cycle. **Work keeps Tempo contacts in Tempo.**
- Logbook: Sync QRZ, Fetch LoTW, Import POTA, every column sortable, click a callsign for QRZ,
  and a per-row Spot.
- Spots: a "My privileges" filter, and filters that survive leaving the view.

### Fixed

- Tuning step is remembered per cockpit; Classic ↔ Roster switching no longer clears decodes;
  Icom IC-7760 added; the FT-710 setup no longer points at a dead Silicon Labs driver link.

## [0.13.0] — 2026-07-19 — Decode off the UI thread, a QSO that can't be lost, honest message status

### Changed — the decode no longer stalls the interface

- **FT8/FT4/FT1/DX1 decoding moved onto its own worker thread.** It used to run inside the
  50 Hz radio loop *while holding the engine lock*, so for the 1–2 seconds a decode took, the
  waterfall stopped receiving new spectrum rows and every UI poll blocked — the whole app went
  sluggish once per slot, every slot. The decode now locks only the decoder, never the engine.
  Waterfall stays fluid, buttons stay responsive.
  Transmit timing is unchanged: the TX decision is still deferred until the boundary decode is
  folded in, so FT1/DX1 (which have no early pass) still react to the slot that just ended
  before keying. This is also groundwork for running two radios at once.

### Fixed — CW cockpit tester punch-list (SourceForge tickets #1–#3, tomsk666)

- **CW Pitch field was unreadable.** The box showed a sliver of a digit instead of the value.
  A shared input style declared later in the stylesheet overrode the field's own padding,
  leaving almost no room once the browser drew its spinner arrows. It reproduced at every
  window size — an ultrawide just made it obvious. Proper width, spinner suppressed.
- **CW speed is remembered.** WPM was runtime-only with no saved setting, so every launch
  reset it to 25 — while the keyer backend and pitch beside it *did* save, which is what made
  it look arbitrary. Now persisted, written once when you finish adjusting rather than on every
  slider tick. The decoder's automatic speed-matching deliberately does NOT overwrite your
  stored speed.
- **Nexus reopens where you left it, and no longer reconfigures your radio at launch.** The
  app always reopened on the FT4/8 pane AND commanded the rig into DATA — worse, it *saved*
  that over your real operating mode, so a station left on 40m LSB for a net came up in DATA-L
  and relaunching could not recover it. The section is restored, and launching no longer
  overwrites your mode.

### Fixed — a completed contact can no longer be lost

- **A QSO waiting in the confirm-before-log popup is now journalled to disk the moment it is
  held.** Previously it existed only in memory: a crash, power cut, or unattended reboot while
  the popup waited destroyed a real contact the other station had already logged, with no trace
  anywhere. It is restored on the next launch, and cleared once you confirm or discard.

### Changed — Tempo chat: message status tells the truth

- **A queued message says whether it actually went out.** Every directed message goes through
  store-and-forward, so "waiting for the recipient to be heard" and "transmitted, awaiting
  acknowledgement" both rendered an identical "Sent". A held message now reads **"Waiting to
  send"** until it first transmits.
- **A message that can never be sent says so.** The queue does not survive a restart, so a
  message still held when you close is gone. It now reads **"Not sent — abandoned on restart"**
  instead of claiming it was sent. (Persisting the queue itself is still to come.)
- **Deleting a conversation now stops the radio.** The ✕ removed the thread but left its queued
  messages transmitting — up to eight more attempts, and indefinitely for a station never heard.
  Deleting now cancels that traffic, confirms first, and persists immediately. The ✕ is also
  visible without hovering and reachable by keyboard.

### Fixed — Linux serial ports

- **Virtual serial ports now appear in the port list on Linux.** Only real hardware ports were
  listed, so anyone bridging Nexus to another program through a virtual pair (a rigctld or flrig
  bridge, WSJT-X interop, a GPS feed) saw an empty list — while CAT itself worked, because it
  connects to a path or a network host and never needs the list. The underlying enumeration
  cannot see PTY-backed ports at all, so Nexus now finds them itself. Ordinary terminal sessions
  are deliberately excluded: listing those would bury your real ports.

### Changed — smaller things

- **The "confirmed" need tag reads `CNF`** instead of `CFM`, which scanned as "C-FM".
- **The Stations roster gets a bigger share of the Classic cockpit rail**, so it shows several
  calls instead of collapsing to about one row next to the (often empty) decode list.

### Fixed — layout

- **Reverted a pixel floor on the Classic-rail Stations roster.** It was reintroducing the
  vertical-clipping bug that adaptive layout fixed (hard floors sum past a short window and
  clip). The roster keeps its larger share of the rail.

## [0.12.0] — 2026-07-18 — RTTY goes hands-free, SSTV FSK-ID + a real FT8 sensitivity fix

### Fixed — on-air transmit pass (RTTY/SSTV) + Raspberry Pi

- **RTTY and SSTV now key with power.** Both armed and asserted PTT but radiated nothing on
  the common Icom / default-Yaesu setup: they commanded plain LSB/USB, where the rig takes TX
  audio from the mic, not the USB codec. They now command a DATA submode (PKTLSB/PKTUSB)
  before keying — the same routing FT8 uses — so the soundcard audio actually modulates.
  Rig-agnostic through Hamlib (Yaesu DATA / Icom -D / Kenwood DATA).
- **Enable-TX arm in the RTTY and SSTV cockpits.** Transmit is off by default (WSJT-X
  "Enable Tx"), but those screens gave no way to arm it, so every send hit "TX is off." The
  cockpit header's TX pill is now a click-to-arm control.
- **Raspberry Pi (aarch64) support.** Nexus now builds an arm64 `.deb` for 64-bit Raspberry
  Pi OS (Pi 3/4/5). On a slower Pi, Settings ▸ Decode depth ▸ Fast keeps FT8/FT4 decoding
  real-time. (Fixed an ARM-only `c_char` signedness bug in the modem FFI.)
- **CW copilot recovers space-split callsigns.** When CW copy dropped a gap mid-call
  ("W1 ABC"), the clean call you read never became a clickable chip. It now rejoins a real
  prefix|suffix split (validated against DXCC) so those calls are clickable again.
- **Phone push-to-talk is a normal button, not a full-width bar** — reclaims the row.
- **Clicking an FT4 spot switches the decoder to FT4** (then QSYs to the spot) instead of
  leaving you on FT8.
- **The live S-meter reading is ~3× larger** on the Phone and CW scopes.

### Fixed — FT8/FT4 decode sensitivity (measured)

- **Anti-aliased receive audio.** The capture path's 48 kHz→12 kHz conversion previously
  took every 4th sample with no filtering, folding all supersonic noise (6–24 kHz) from
  the soundcard/interface straight into the decode band. It now runs a proper 64-tap
  anti-alias decimator (fc 4500 Hz — same spec as WSJT-X's, with deeper stopband).
  Measured on paired test audio: up to **+4 dB of effective sensitivity** on a noisy
  audio chain, and a doubled-to-tripled decode rate at the −21 dB weak tail even on a
  clean chain. Benchmarked against stock WSJT-X's decoder on identical audio, Nexus's
  decode floor now sits at −21.3 dB vs stock's −20.7, with zero false decodes.
- **Busy slots no longer drop decodes.** The per-slot decode limit was 64 (weakest
  arrivals silently discarded on crowded bands); now 200, matching WSJT-X. Applies to
  FT8 and FT4.
- **Cross-cycle deep recovery (a7) fixed**: the early decode pass was double-writing the
  a7 candidate table (halving its capacity), and the table wasn't cleared on radio swap
  or a VFO-knob band change. Both fixed — a7 recoveries now work at full strength.
- **Field Day AP decoding**: your callsign now feeds the a-priori decoder during Field
  Day operation, so "MyCall ???" deep recoveries work there like normal operation.

### Fixed — rig control, RTTY, roster, scaling

- **Dual same-model Icom radios now work.** With two Icoms configured, mode-setting
  failed on both ("rig has no PKTUSB mode") and only worked after deselecting one — a
  radio-handoff isolation bug that double-commanded the outgoing rig on every contended
  switch. Fixed. (Plus: rigs on a slow CAT link — ≤19200 baud, the IC-7610's factory
  default — now get a longer reply deadline, a mode-set fallback ladder, and honest
  "link too slow / press the rig's DATA key" messages instead of a dead-end.)
- **RTTY no longer prints garbage on an empty frequency.** The Baudot demod had no
  squelch, so band noise decoded into a stream of random characters. Added a
  signal-presence squelch (calibrated so noise is silent but a −2 dB signal still copies).
- **The FT call roster reads as "live now."** Tightened the drop-off to 3 T/R cycles
  (~45 s on FT8) and added an age fade — stations dim as they go quiet, so who's active
  right now stands out.
- **UI scaling controls work correctly.** The Manual scale strip no longer overflows its
  container (options past 110% are reachable again), Auto's max-scale chips are disabled
  when the window can't use them (no more "150% = 175%"), the Comfortable/Compact density
  switch now actually changes row spacing, and Settings tabs can't be clipped at any scale.
- **3-D globe (Connect) spot hover** now shows the same rich tooltip as the 2-D map
  (callsign, band/mode, frequency, age, "heard you") instead of just the callsign.

### Fixed — controls & frequencies

- **TX Power controls now match and apply live.** The Settings "Tx Power" and the
  cockpit "Pwr" slider are the same value (the audio drive into the rig — not the rig's
  RF watts); Settings now applies on release and both stay in sync in both directions.
- **RTTY/SSTV band-plan corrections** (checked against ARRL + IARU R1 + community
  convention): RTTY 80 m moved 3.580 → 3.590 (3.580 is PSK31), RTTY 40 m split into
  7.080 (US) + 7.045 (EU/DX), SSTV 80 m split into 3.845 (US) + 3.730 (EU), 12 m RTTY
  segment note corrected.

### Added

- **SSTV transmit — send pictures on the air.** The SSTV cockpit now has a Transmit
  panel: drop in an image, pick a mode (all 15 — Scottie, Martin, PD, Robot), see a live
  preview cropped to that mode's exact resolution, and Send. It transmits as USB voice
  audio through the safety-gated TX path (nothing keys until you press Send, guaranteed
  unkey, a hard duration cap), with a progress bar and one-click Stop. Verified
  end-to-end: every mode encodes and decodes back through Nexus's own receiver.
- **RTTY auto-sequencer — hands-free QSOs.** Turn on **Auto** in the RTTY cockpit, then
  click **CQ** to run or **Answer** a decoded caller: the exchange sends, the contact
  auto-logs (mode RTTY), and the closing 73 goes out — the same operating discipline as
  the FT8 sequencer, over the safety-gated RTTY keyer. Nothing ever transmits on launch
  or on toggling Auto; only an explicit CQ/Answer keys up.
- **RTTY waterfall with mark/space cursors + click-to-net.** The RTTY cockpit now shows a
  waterfall with cursors marking the mark and space tones; click a signal to net the
  decoder onto it (re-acquires AFC around the new center).
- **RTTY spots on the Needed board.** Reverse Beacon Network RTTY skimmer spots now appear
  as **RTTY** rows (governed by the Digital filter chip); one click QSYs and opens the RTTY
  cockpit.
- **RTTY & SSTV in the setup wizard.** The first-run wizard now offers RTTY and SSTV as
  operating modes alongside Phone and CW.
- **SSTV FSK-ID capture.** The callsign FSK ID that trails an SSTV image is decoded and
  shown on the gallery entry (best-effort — a callsign appears only when cleanly recovered).
- **Auto-arm SSTV for ISS passes (opt-in).** When enabled in Settings, Nexus tunes 145.800
  FM and arms the SSTV decoder when the ISS is overhead, then restores your dial at LOS.
  Off by default; never retunes without the opt-in.

### Fixed

- **The RX Gain slider now applies to the live audio as you use it.** Previously the
  slider only updated its label and didn't reach the running capture stream until you
  hit Save — so the RX Level meter never moved while dragging and the control looked
  dead. It now commits the new gain to the live stream when you release the slider (or
  after a keyboard adjustment), so the meter responds immediately. (Decoding was never
  affected — the gain always applied on Save.)
- **The "update available" notice now appears reliably on launch.** The launch check
  was gated by a once-per-day throttle that also suppressed the *display* (not just the
  network fetch), and every manual "Check for updates" reset that timer — so for anyone
  who launches often or uses the button, the launch prompt was effectively never shown
  while the manual check always worked. The check now runs on every launch (a single
  small request) and surfaces the prompt whenever a newer build exists and that version
  hasn't been dismissed via Download.

### Changed

- **Update checks now read the app's own endpoint** (`hamradiotools.io/nexus/version.json`),
  falling back to SourceForge's `best_release.json` if it's unreachable — so update
  accuracy no longer depends on the per-release SourceForge "Default Download" flip. The
  "Download" button now opens the GitHub Releases page (primary distribution; SourceForge
  mirrors it).

## [0.11.1] — 2026-07-18 — fill-to-bottom fix

### Fixed

- **The interface now truly fills to the bottom of the window on every view and at
  every UI scale.** The app shell's height is measured against the real rendered box
  each resize/zoom change and corrected in pixels, instead of trusting a zoom formula
  whose semantics vary across WebView versions — the persistent dead band at the
  bottom of the screen is gone. (Operator-verified live.)

## [0.11.0] — 2026-07-18 — RTTY + SSTV (beta), openings intelligence, and a decode-accuracy milestone

### Added

- **RTTY — a first-class modern RTTY mode (BETA: receive and transmit).** A new RTTY
  entry in the Digital rail with a real cockpit: arm the decoder and decoded text streams
  live off your rig's audio with **per-character confidence fading** (weak copy renders
  faint — you can see *how sure* the decoder is), an AFC readout that locks to the signal,
  and a band selector preloaded with the classic RTTY watering holes (14.083, 7.080, 3.580…,
  license-filtered). Under the hood: a full ITA2 Baudot codec and a demodulator ported from
  fldigi's proven W7AY design (mark/space matched filters, optimal ATC, acquire-then-freeze
  AFC) — solid copy down to −2 dB SNR in testing. Transmit works on BOTH paths from day
  one: soundcard AFSK (rig in LSB, audio through the same TX route as FT8 so your drive/ALC
  setup carries over) and true FSK via a DTR/RTS keyline (rig in RTTY mode — narrow RTTY
  filters unlock), with a compose line, one-tap macros (CQ/Answer/Exchange/73), a hard Stop,
  and plain-language refusals when a send isn't safe (TX off, out of privileges, tuning).
  Beta note: the transmit path is new this release — verify your first over at low power.
- **SSTV — receive slow-scan images into a gallery (BETA).** A new SSTV section: arm the receiver
  and images decode off the air (Martin, Scottie, Robot, PD — including **PD120 for ISS
  events**) with live progressive preview, auto slant correction, and every completed image
  saved to a browsable gallery folder stamped with mode, frequency, and UTC time. The band
  selector includes **145.800 FM — the ISS downlink** — plus the HF calling frequencies
  (14.230 and friends).
- **Tempo: Call CQ is now a RUN.** Toggle it on and Nexus keeps calling on every idle TX
  slot until someone answers — then it auto-pauses while you chat and resumes when the
  conversation goes quiet (or on your Resume click). The control lives in the Tempo header
  with its state always visible; no more one-shot CQ dead-end.
- **FT8/FT4: cross-cycle AP decoding (WSJT-X a7).** Stations you decoded in the previous
  cycle are recovered a few dB deeper this cycle — their RR73s and reports especially.
  Matches WSJT-X's a7 machinery exactly; resets on band change.
- **Spots: freeform search.** A search box over the firehose — terms combine across
  callsign, entity, spotter, mode, band, and frequency ("w1 20m cw").
- **Field Day / Winter Field Day correctness:** the WFD window is now the full 30 hours
  (was 24 — QSOs in the final 6 hours weren't counted), digital contacts export their REAL
  mode (an RTTY WFD log no longer exports as "FT8" — a mode WFD bans), and the ruleset now
  knows which modes WFD prohibits.

### Fixed

- **RTTY and SSTV no longer show the FT8 frequency bar and tier tiles** — each cockpit's
  own band selector is the only dial control there, like Phone and CW.
- **The CW/Phone bandscope no longer paints a quiet band as full-width rainbow.** The
  scope's auto-contrast stretched the noise floor across the whole palette, so filtered-out
  stopband noise looked like signals. It now enforces a 10 dB minimum visual span (quiet
  water renders dark; real signals unchanged), adds the FT8 waterfall's Gain/Zero controls,
  and shows a "Δ dB" readout of the view's true dynamic range.
- **Linux: caught driver panics are no longer silent, and shipped binaries strip debug
  info.** A quirky serial/audio stack could panic on every device poll — invisibly costing
  CPU (sluggishness) and memory (the panic machinery's ~68 MB symbol cache). Caught panics
  now log with a count, and release builds carry no DWARF for that cache to parse.

- **2m openings are now detected — and every opening is classified, tiered, mapped, and
  logged.** The detector needed several distinct stations to call a VHF band open (right
  for a 6m Es cloud, impossible for 2m tropo/aurora, which are often ONE distant
  station): now a single genuine-DX station beyond 700 km — past the everyday
  troposcatter ceiling, at the floor of the real opening modes — opens a VHF band. Two
  more graduated triggers round it out: **two distinct stations at 500 km+** catch the
  quick short tropo lifts (one alone is routine scatter and stays quiet — corroboration
  keeps false positives out), and **two independent receivers near you** each copying a
  700 km+ path open the band even when you're parked on another band and transmitting
  nothing — your neighbors' ears become your sentinel. On top of that:
  - **Tiered opening alerts by propagation mode.** Sporadic-E and F2 go loud (rare and
    brief — grab-it-now, with a beep); **Aurora** goes loud with operating guidance
    ("beam north — signals sound raspy, CW & SSB work best"); **tropo** raises an
    informative note (lifts last hours). Routine local/scatter activity never alerts.
  - **Opening sectors on the map.** Both the 2-D map and the 3-D globe now draw each
    live opening as a wedge from your QTH toward the opening — amber for tropo, green
    for Es, violet for aurora, cyan for F2 — sized to the longest path heard, so you
    can see where and what kind at a glance. The live Openings pane's mode chips use
    the same colors.
  - **A persistent openings log.** Every opening episode is journaled when it ends
    (band, mode, start/duration, peak strength, longest DX, station count, direction)
    and survives restarts — an opening in progress when you quit is saved too. A new
    **Openings Log** pane in Connect reviews the history with 6m/2m filters: "how many
    real 2m openings this month, and did I catch them?"

### Changed

- **RX audio level meter now reads in dB, like WSJT-X.** It was a linear 0–1 bar whose
  "good" zone (0.45–0.9) was a voice-style target too hot for FT8, so a perfectly good
  weak-signal input read as "low" and pushed you to over-crank RX Gain. The meter now
  shows `20·log10(rms)+90.3` — the same scale as WSJT-X (aim ~30 dB; ~15–60 decodes
  fine; red is too hot) — so the reading is directly comparable and you can see you
  don't need much gain. The RX Level / RX Gain hints were reworded to match.

### Fixed

- **The interface fills to the bottom of the window at every zoom level.** Below ~900 px
  of usable height the UI scales down, and the app shell was being laid out at full
  height and *then* scaled — leaving a dead band at the bottom of the screen. The shell
  height now compensates for the zoom, so it fills the viewport exactly (no change at
  100%).
- **Core "always on" features (Operate, Logbook, Settings, …) show an "always on" badge
  instead of a disabled toggle** that looked like a broken control next to the real,
  toggleable feature settings.

## [0.10.0] — 2026-07-17 — Memories section + a big rig-control & reliability batch

### Fixed

- **"Share my radio" (CAT broker) turns on without a restart.** Enabling the broker — or changing its
  port — now takes effect immediately; you no longer have to restart Nexus. It also works while Nexus
  is sharing an external rigctld, so a logger (WSJT-X / N1MM) pointed at the broker connects right away.
- **A rig that rejects PTT no longer transmits into silence.** On FT8/FT4 and phone, if the radio
  NAK'd or timed out the key command, Nexus played (or armed) modem audio while the rig stayed in
  receive — dead air on the band with no warning. It now surfaces "the rig didn't accept PTT — check
  your PTT method and CAT/port," so you know the key didn't take instead of calling into a void.
- **AI CW decoder now finds its model on Linux.** The DeepCW model ships bundled inside the .deb and
  AppImage, but the app located it in a Windows-only way (next to the exe), so on Linux it reported
  "model not installed." It now uses the platform resource directory, so the model loads on all
  platforms — there's nothing extra to download or install.
- **"Sync from QRZ" now actually imports your QSOs.** QRZ returns the fetched logbook as ADIF with its
  angle brackets HTML-escaped (`&lt;call:5&gt;…`), which Nexus was treating as literal — so the importer
  saw no records and reported 0 QSOs with no error, even after a full re-sync. Nexus now decodes the
  ADIF before importing, matching how established QRZ clients read the response.
- **The ALL.TXT decode log is now findable.** It moved to an app-named folder in your local app data
  (`%LOCALAPPDATA%\Nexus\ALL.TXT` on Windows — the same class of place WSJT-X keeps its own), the folder
  is created if missing, and Settings ▸ shows the exact path with a **"Reveal in folder"** button. The
  hint now says what tripped people up: it's written only while the toggle is on, and the file first
  appears after the next decode. (It can't live in the install folder — Program Files isn't writable
  without elevation, so writes there would silently fail.)
- **WSJT-X UDP (GridTracker, JTAlert) and PSK Reporter now turn on without restarting Nexus.** The
  UDP emitters were built once at startup, so enabling them *after* launch — the normal order when you
  set up GridTracker first, then point Nexus at it — did nothing until a restart. They're now rebuilt
  live when you flip the toggle or change the target address, re-announcing on connect so GridTracker
  registers Nexus immediately.

### Changed

- **The Program section (radio programming) is now on by default.** It works on open hearham.com
  repeater data with no setup, so it no longer waits behind an opt-in toggle. (If you'd previously
  customized your sections, enable it any time in Settings ▸ Features.)

### Added

- **Separate PTT serial port, for SO2R and external keying interfaces.** RTS/DTR PTT can now key on
  its **own** COM port, independent of CAT — so a controller like the microHAM u2R/MK2R (or a homebrew
  keyer) that routes PTT on, say, COM16 while CAT rides the radio's USB now works. Set it in
  Settings ▸ Rig Control when PTT method is Serial RTS/DTR; leave it blank to keep the old behavior
  (key on the CAT port). Selecting serial PTT no longer disables CAT — frequency and mode still track.
- **Type a COM port when it's not in the dropdown.** The Serial Port and PTT Serial Port fields are now
  editable comboboxes: some driver setups (virtual/SO2R COM ports) make Windows enumeration come back
  empty, and you can now just type the port (e.g. `COM16`) instead of being stuck.
- **Skip Tx1 (FT8/FT4), like WSJT-X.** A "Skip Tx1" checkbox in the Tx panel: when you answer a CQ,
  the QSO opens with your signal report (Tx2) instead of your grid (Tx1), saving a cycle. Standard
  callsigns only — a compound call (e.g. KD9TAW/P) still opens with the grid, since the report message
  can't carry it. Like WSJT-X, it's a per-session toggle and resets to off each launch.
- **A first-class Memories section — repeaters, HF nets, calling frequencies, POTA/SOTA and digital
  watering holes in one place.** Replaces the small saved-frequency bank with a full manager: a sidebar
  of groups and ★ favorites, a clean list with an inline editor, and a CHIRP-style grid on demand.
  One-click **Tune** sets frequency, mode, repeater shift and tone in one atomic step and opens the
  right cockpit (CW → CW, SSB/FM → Phone, FT8 → Digital) — no wrong-mode flash. Star a memory and it
  rides the **MEM strip** in every cockpit for instant recall.
- **Starter packs.** One click installs a curated channel set — *VHF/UHF Calling & Simplex*, *HF Digital
  Watering Holes*, *POTA Activity*, and *Well-Known HF Nets* — deduped, so re-installing is safe.
  Offered both in first-run setup ("Start with some channels?") and from the empty Memories view.
  Re-installing a pack also **refreshes** it:
  if a later Nexus release corrects a net's time or a note, installing again applies the correction.
  Any channel you've edited yourself becomes yours and is never overwritten — and turning a net
  reminder on won't stop that net receiving schedule corrections.
- **Quick-recall hotkeys.** Press **Ctrl+1** through **Ctrl+9** from any section to tune your first
  nine ★ favorites — the same one-click tune (frequency, mode, shift, tone + cockpit switch) as the
  MEM strip, without reaching for the mouse. The strip's tooltips show each chip's hotkey.
- **Opt-in net reminders.** Give an HF-net memory its meeting days and UTC time, tick **Remind me**, and
  Nexus raises a one-click *Tune* reminder a few minutes before it starts. Reminders are per-net — only
  the nets you enable, never a firehose.
- **Full CHIRP CSV round-trip.** Import and export the standard CHIRP format, so channels flow
  Nexus ⇄ CHIRP ⇄ ~1,000 real radio models. The Program section still feeds repeaters straight into
  Memories.

## [0.9.7] — 2026-07-17 — Serial CW keying + slow-rig CAT fix

### Added

- **A serial DTR/RTS CW keyline keyer — the clean way to key an older rig from the PC.** For rigs that
  don't support CAT CW keying (the IC-756PRO III and most pre-2016 radios), Nexus can now toggle a DTR
  or RTS line into the rig's KEY jack the way N1MM and fldigi do: the rig stays in CW mode and shapes
  the CW envelope itself, so the signal is clean. Pick **Serial keyline (DTR/RTS)** in Settings ▸ CW,
  set the keying serial port (a separate USB-to-serial into your keying interface — a Buxcomm, US
  Navigator, or a homebrew DTR cable) and the line (DTR by default), put the rig in CW with its key jack
  set to straight key, and send. It's also on the CW cockpit's keyer switcher. This joins the existing
  CAT, WinKeyer, and soundcard keyers; the soundcard option is now labeled as the SSB-audio workaround
  it is (keep its drive below ALC).

### Fixed

- **Xiegu G90 and vintage Kenwoods no longer drop CAT with "rig reply incomplete after 700 ms".** These
  radios have a slower CI-V / serial backend whose reply can arrive just after the old 700 ms cutoff, so
  Nexus was giving up on a command the rig would have answered. They now get the same longer,
  retry-tolerant window that network and native-CI-V rigs already use. No change to any other rig.

## [0.9.6] — 2026-07-16 — Fits any window or screen size + Program (radio programming)

### Changed

- **Nexus now fits any window size and screen resolution, not just 1080p.** The whole
  interface auto-scales to the window so the full cockpit stays visible instead of getting
  cut off at the bottom or the right rail. At 1080p and larger it sits at 100% as before;
  on a shorter or smaller window it scales down just enough to keep everything on screen,
  and it re-fits live while you drag the window, down to a 900×600 minimum. Content that
  still cannot fit scrolls inside its own panel rather than clipping. Two new controls live
  in Settings ▸ Appearance: an **Auto (fit) / Manual** UI-scale switch with an adjustable
  maximum for big monitors, and a **Comfortable / Compact** density switch. This retires the
  old fixed layout that was tuned for 1080p and clipped on laptops, 1280-wide windows, and
  smaller screens.

### Accessibility

- **Nexus now speaks and can be driven by keyboard — a first pass at full accessibility for blind
  and low-vision operators.** These work with JAWS or NVDA on Windows (and are invisible to everyone
  else — no "accessibility mode" to turn on):
  - **The operating loop is now announced.** A screen reader hears the QSO sequencer advance
    (calling CQ → report → RR73 → logged), the "now sending" message, and — assertively — every
    switch between transmit and receive. The section you're in is announced and titles the window.
  - **The band-activity, Call Roster, and Needed lists are keyboard-navigable.** Arrow through the
    rows (each is read aloud), Enter to select, Shift+Enter to work the station, Alt+Enter to
    ignore — the mouse's click and double-click, from the keyboard.
  - **New Settings ▸ Alerts ▸ Accessibility & eyes-free:** optional spoken decode announcements
    (off / needed-only / all), a TX/RX earcon, and a soft per-cycle decode tick — for operating by
    ear. All default to quiet so nothing changes for sighted users.
  - Phone's hands-free PTT Lock is now keyable (Enter toggles TX), dialog focus is trapped, and the
    setup wizard announces a bad grid instead of silently disabling Next.

### Fixed

- **Click-and-hold tuning on the Phone/CW scope now works on every rig, not just those with a
  native panadapter.** On Yaesu (and any audio-scope rig), grabbing the scope brings up the
  passband box and dragging slides the band with your hand — the grabbed signal follows the
  cursor — and holding near a scope edge keeps scrolling, exactly as on Icom/Flex. A click is an
  in-passband fine-tune (snap to the signal under the cursor); the across-the-band jump needs the
  real RF panadapter that Icom/Flex provide. The Icom/Flex behavior is unchanged.
- **The FT8 Classic layout's right column no longer clips at 1080p.** The standard-message panel
  is tighter, Rx Frequency and Stations shrink and scroll inside themselves, and if a window is
  still too short the column itself scrolls instead of cutting off the station filters. The
  Stations panel also stopped wasting height: the band row is one compact line and the Tempo
  "Recent chats" list no longer renders in the FT8/FT4 cockpit (it belongs to Tempo).
- **The AI CW decoder's copy now flows.** Decoded text used to arrive in blocks every ~6 seconds;
  the decoder now runs passes every ~2 seconds (self-throttling on slower machines) and the panel
  reveals new text character by character, so copy reads like a live operator. Same model, same
  decoding — typical delay from key-down to on-screen drops from ~5 s to ~2 s.
- **Vintage Kenwood rigs connect out of the box.** Picking a TS-140S, TS-440S, TS-850, TS-940S
  (and the rest of the IF-232C era) now auto-sets their fixed 4800 baud, and the TS-870S/TS-570
  set their factory 9600 — the 38400 default left CAT silent on these radios.
- **Switching to CW now lands on the CW calling frequency, not the band edge.** Changing mode
  to CW on 20 m used to park the dial at 14.000, the very bottom of the band; it now tunes to
  the CW activity frequency (14.030 on 20 m, and the equivalent on every other band).

### Added

- **A new Program section: build channel lists for your radios** (ships hidden while our
  RepeaterBook API access is pending — turn it on in Settings ▸ Features to try it on the open
  hearham.com directory). Pick a location —
  your station grid by default, or any grid square or city (for a trip) — set a radius, and fetch
  the repeaters around it. Add the ones you want to a channel list with automatic offsets, tones,
  channel numbers, and radio-ready names (6–16 characters, picked for your radio), then:
  - **Export for CHIRP** — a CSV that CHIRP (free) imports and flashes to roughly a thousand radio
    models, Baofeng to Kenwood. Nexus builds the list; CHIRP drives the cable.
  - **Export CSV** — a plain spreadsheet-friendly listing for Anytone CPS, RT Systems, or printing.
  - **Tune** — with a CAT rig connected, one click puts the rig on a repeater right now: FM, the
    machine's exact shift and offset (odd splits included), and its CTCSS tone.
  - **Save to Memory Bank** — the channels land in the Phone cockpit's MEMORY recall list, and
    recalling one now applies the repeater shift and tone, not just the frequency.
  The channel list persists across restarts, recent locations are one click to reuse, and off-air
  machines are filtered out by default. DMR / D-STAR / Fusion repeaters are listed with badges so
  you know they're there; programming them comes in a later version.
- **Repeater data sources.** Out of the box the section uses the open hearham.com directory. A
  RepeaterBook API token (Settings ▸ Integrations & Feeds) switches it to RepeaterBook's much
  larger North-American directory — data courtesy of RepeaterBook.com. City search is powered by
  OpenStreetMap. Directory data is cached for a week per state so repeat sessions are instant and
  the sources aren't hammered.

## [0.9.5] — 2026-07-16 — one shared cockpit header across every mode + FT8 layout cleanup

### Changed

- **Every operating mode now shares one cockpit header.** Phone, CW, FT8/FT4, and Tempo show the same
  base rig controls — frequency readout, band, mode, power, and CAT status — in the same position, so
  switching modes keeps the controls where you left them. Each mode still keeps its own unique controls
  (CW keyer/speed, phone sideband, FT8 tier and DXped, and so on).
- **FT8/FT4 frequency gained the full tuning strip** (nudge, step, VFO A/B, RIT, XIT) that Phone and CW
  already had, and its band/frequency picker is restyled to match the bold band control used elsewhere.
- **The band shows its color everywhere.** The FT8/FT4 and Tempo frequency picker now carries the same
  band-colored dot and glow as the Phone/CW band control (the same colors as the map's spots), so the
  band you're on reads the same across every mode.
- **Tempo now has the shared header too** — frequency, band, mode, and CAT. Before, those only lived in
  the top bar; Tempo now reads like the other cockpits.
- **FT8 Classic layout redesigned to the WSJT-X two-pane shape.** The standard-message machine (Tx1–Tx6)
  moved from a wide band full of empty space into a compact panel in the right rail, so Band Activity now
  takes the full height on the left.

### Fixed

- **The Tune button in the CW cockpit is visible again.** It was rendering without its styling, so it was
  nearly invisible on the dark theme.
- **The cockpit header keeps a steady height** when you switch between modes instead of jumping.

## [0.9.4] — 2026-07-16 — Icom CI-V: FT8/FT4 waterfall no longer blank

### Fixed

- **The FT8/FT4 waterfall showed only a flat colored field on Icom radios in native CI-V mode.** The
  Icom's built-in band scope kept feeding its RF spectrum into the display even in FT8, where the
  waterfall shows the received *audio* (0–4000 Hz) instead — so the wide radio-frequency sweep mapped
  off the edge and painted flat. (Decoding was never affected.) Nexus now turns the native scope off
  in FT8/FT4 so the audio waterfall shows normally, and keeps it on for SSB and CW where it belongs.
  Yaesu and other rigs were unaffected.

## [0.9.3] — 2026-07-16 — tester batch: marker fix, instant Tune-off, faster CW, freq-clip, wheel sensitivity

### Fixed

- **The FT8/FT4 waterfall no longer leaves a trail of Rx/Tx marker lines when you retune.** The green
  Rx and red Tx markers were painted into the scrolling spectrum image, so each time you moved one the
  old position froze and scrolled up as a streak. Markers now draw on a separate overlay that's cleared
  every frame — one Rx line and one Tx line, always.
- **Tune turns off instantly again.** On rigs with a slow CAT link (native Icom CI-V, or a networked
  chain like the K4 over QK4 Remote), releasing Tune could hang for up to a second or two waiting on the
  radio's acknowledgement. PTT commands now use a short fixed timeout so the un-key fires promptly,
  while the slower rig read-backs keep their longer window. (Regression from the 0.9.1 K4 CAT work.)
- **The CW decoder keeps up in near real time.** The CW window was only reading new decoded text a few
  times a second, which added visible lag; it now refreshes several times faster.
- **The frequency display no longer scrolls off-screen when the window isn't maximized** (or at
  110–125% UI zoom) — it wraps instead of clipping.

### Added

- **Adjustable wheel-tune sensitivity** (Settings ▸ Rig / CAT) for high-resolution "free-spin" mice
  that tuned too far per flick.

## [0.9.2] — 2026-07-15 — click-to-tune on the Phone/CW scopes + layout cutoff fixes

### Added

- **Click a signal on the Phone or CW scope to tune to it, the way a FlexRadio slice works.**
  Nexus finds the signal near your click and puts the dial where it belongs for the mode:
  - **SSB:** on the signal's suppressed carrier (detected energy edge minus the 300 Hz voice
    low-cut), so the voice sounds natural immediately. No clear signal under the click parks the
    dial on the nearest 500 Hz.
  - **CW:** zero-beat — the signal lands exactly at your sidetone pitch. Works with the CAT and
    WinKeyer keyers (dial on the signal) and the soundcard keyer (dial offset by the pitch).
  - **FM/AM:** centered on the carrier.
  Works on the native RF panadapters (FlexRadio, Icom CI-V scope) and on the audio scope every
  other rig gets — there a click shifts the dial so the clicked signal lands at your pitch (CW)
  or settles the voice into the passband (SSB).
- **Hold the left button and drag a passband box to tune by hand.** The box is the width of the
  rig's current RX filter and shows exactly where the rig is listening (above the dial on USB,
  below on LSB, centered on CW). The rig follows live while you drag, throttled to one CAT write
  per 120 ms. Push the box into the outer edge of the scope and the whole band scrolls under it —
  ease in for a slow readable cruise, shove to the very edge for about 3 screen-widths per second.
  The box stays pinned under your cursor the whole time.

- **Per-alert band scopes — new-grid alerts default to VHF+ only.** Settings ▸ Alerts now gives
  **New DXCC**, **New grid**, and **Rare grid 💎** each their own control: Off / HF only / VHF+
  (6 m and up) / All bands. Grid chasing is a VHF pursuit (VUCC/FFMA start at 6 m) — on HF nearly
  every decode is an unworked grid, so plain new-grid alerts now stay quiet below 6 m unless you
  ask for them. The rare/water-only 💎 alerts are a separate control and stay on everywhere by
  default, so silencing HF grid chatter keeps the gems. "My call" and "CQ" alerts are unchanged.

### Changed

- **Settings reorganized to match how you operate.** The tabs now mirror the app's Phone · Digital ·
  CW layout instead of being grouped by subsystem. New **Phone**, **Digital (FT8/FT4)**, and **CW**
  tabs gather each mode's own settings — most notably a real **CW** home with the keyer backend,
  sidetone pitch, WinKeyer port, "CW ID after 73", and the F-key macro profiles all in one place
  (the CW macros used to sit under Alerts). Misfiled panels were also moved to where they belong:
  the N3FJP and N1MM+ logger integrations and the connector-status panel now live under
  **Integrations & Feeds**. No settings were lost or renamed — everything you'd saved carries over.
- **The panadapter trace no longer strobes with bursty signals.** The colored spectrum trace above
  the waterfall used to flash at frame rate with every syllable gap and CW dit. It now rises
  instantly when a signal appears and fades over about a second when it pauses (the classic rig
  peak-hold with decay). The waterfall below is unchanged.

### Fixed

- **The setup wizard no longer cuts off its bottom on shorter screens.** Its last step is the tallest,
  and the dialog had no height cap or scroll, so on a laptop-height display the mode cards and the
  Back/Next/Finish buttons ran off the bottom edge — you couldn't reach Finish. Dialogs now cap to the
  viewport and scroll their content. Every modal shares this shell, so they all benefit.
- **A batch of related cut-off fixes across the app**, all the same family (content running off-screen
  with no scroll), mostly visible at ~1366×768 or at 110–125% UI zoom:
  - **Operate cockpit:** the right-hand control cluster (Pwr/drive slider, Pop-out, Spot) wraps to a
    second line instead of clipping off the right edge; the long Companion address is ellipsized so it
    can't push the row wide.
  - **Logbook:** the per-row QRZ/ClubLog push buttons no longer clip off the left edge; long compound
    callsigns show the full call on hover.
  - **Roam (coordinated QSY) panel and torn-off panel windows:** heights are zoom-corrected, so at
    110–125% zoom the close button / panel bottom no longer sit off-screen.
  - **Toast alerts** and the **3-D globe layer list** now scroll when they'd otherwise overflow.
  - **Call Roster:** a station's full set of "need" reasons shows on hover even when a chip is clipped.

## [0.9.1] — 2026-07-15 — late-start TX, K4 CAT stability, wider FT8 passband

### Added

- **FT8/FT4 decode passband is now adjustable up to 4 kHz.** Operators regularly call above the old
  2.9 kHz ceiling on crowded bands. Settings ▸ Digital ▸ Decoder passband now lets you raise **F high**
  toward 4000 Hz, and the waterfall, the click-to-tune range, and the Rx/Tx offset entry all extend to
  match — so a station calling at 3.3 kHz is visible, decodable, and answerable. The default stays
  200–2900 Hz, so nothing changes unless you widen it. *What this means:* you can now work the people
  who park themselves up high where it's less crowded. (This setting also existed before but never took
  effect — the saved value used a key the backend didn't read; that round-trip is fixed.)

### Fixed

- **You can start a transmission a second or two into a period instead of waiting a full cycle.**
  Previously, if you keyed up more than ~2 s late you'd be deferred to the next same-parity slot — the
  "clicked one second too late, now I wait 30 seconds" complaint. Nexus now keys the *current* period
  the WSJT-X way: the over stays time-aligned and just drops its leading samples, which the far-end
  decoder still syncs on. The budget is per mode and preserves the sync tones — up to ~6 s late for FT8,
  ~3 s for FT4.
- **CAT no longer drops and reconnects every few seconds with the Elecraft K4 (QK4 Remote).** Nexus
  polls the rig for RF power, mic gain, NR level and AGC to mirror the knobs into the UI. The K4 over
  QK4 Remote is slow or silent on those reads, so each one hit the command timeout and tore down the
  CAT socket — the ~5 s hang. Those reads are now capability-cached the same way the S-meter and DSP
  toggles already were: after a few misses Nexus stops issuing the read, so a rig that doesn't answer
  it quickly keeps a stable connection. (WSJT-X, HRD and DXLab were unaffected because they don't poll
  those levels.)

## [0.9.0] — 2026-07-15 — Linux build + decode-regression fix + globe fix

### Added

- **Linux build.** Nexus now ships a **.deb and an AppImage** alongside the Windows installer, built
  with `scripts/build-linux.sh` (native Tauri, system FFTW). CAT on Linux uses the system Hamlib —
  the .deb pulls `libhamlib-utils` automatically; AppImage users run `sudo apt install libhamlib-utils`.

### Fixed

- **FT8/FT4 decode restored on stereo audio interfaces (FlexRadio DAX, Xiegu DE-19).** The 0.8.9
  mono-fold change picked the "loudest" channel per capture block with no memory, so on a 2-channel
  codec whose idle channel carries hiss it thrashed between channels and destroyed the phase coherence
  the decoder needs — audio and the waterfall showed activity, but nothing decoded. Reverted the fold
  to **channel averaging** (what decoded before), which is phase-coherent regardless of how a rig lays
  mono onto a stereo stream. Mono interfaces (most Yaesu) were never affected. The **RX Gain** control
  stays as the lever for a quiet interface — raise it if the RX level reads low.
- **The 3-D Connect globe no longer washes out to a blown-out glare after a window resize.** The
  globe's bloom pass was being re-added on every resize (stacking glow); it's now added once and
  simply resized, with cleanup so a remount can't accumulate another.

## [0.8.9] — 2026-07-15 — RX audio level fix + RX gain + 1080p window fit

### Fixed

- **RX audio no longer reads much lower than WSJT-X on the same interface.** Many rig USB codecs
  (the Xiegu DE-19 among them) are 2-channel but carry the receive audio on ONE channel, with the
  other silent or just hiss. Nexus folded to mono by *averaging* the channels, which halved the
  level (−6 dB) and mixed the dead channel's noise into the signal (worse SNR). Nexus now takes the
  **channel that actually carries the signal**, restoring full level. Single-channel and true
  dual-mono devices are unchanged.
- **Windows no longer cut off at 1080p while looking perfect at 4K.** The auto-zoom picked its
  level from screen *width* only, so 1920×1080 got 110% — too tall, pushing the bottom of the
  layout past the window edge. The auto-fit is now **height-aware**: 1080p lands on 100%, and 4K
  still gets 125%. (You can always override the zoom in the top bar.)

### Added

- **RX Gain control (Settings ▸ Audio).** A software boost (×1.0–×8.0) applied to received audio
  before decode — headroom for a quiet interface whose line-out reads low in Nexus. Watch the RX
  Level meter and raise it until the level reaches the green zone. Default ×1.0 (unchanged).

## [0.8.8] — 2026-07-14 — Xiegu CAT fix ("os error 10049") + auto-baud

### Fixed

- **CAT no longer fails with "the requested address is not valid in its context (os error 10049)"
  on a radio whose rigctld port was left at 0.** Nexus runs a separate rigctld per radio, each on
  its own TCP port, and connects to `127.0.0.1:<port>`. A profile that carried port 0 (from an older
  or imported config) made Nexus try to reach `127.0.0.1:0`, which Windows rejects with
  WSAEADDRNOTAVAIL — so that one radio's CAT failed on **Test CAT** and on every mode change while
  its siblings (Yaesu, Icom) kept working. The on-load port repair now reassigns a 0/invalid port
  (not just *duplicate* ports), and the connection coerces a stray 0 to the default 4532, so this
  can't resurface. If you hit it, just re-open **Settings ▸ Rig Control ▸ Advanced** and the port is
  already fixed.

### Changed

- **Selecting a Xiegu (G90 / X6100 / X6200 / X5105 / X108G) now sets CAT to 19200 automatically.**
  These rigs run CI-V at 19200 and have no baud menu on the radio, so the previous 38400 default left
  CAT silent (rigctld connected but the radio never answered). Picking or auto-applying a Xiegu now
  sets 19200; you can still change it by hand.

## [0.8.7] — 2026-07-14 — CW ragchew macro tokens + FlexRadio panadapter (early access)

### Added

- **CW macro tokens for ragchew exchanges: `{HISNAME}`, `{MYSTATE}`, `{HISSTATE}`.** Beyond
  `{MYCALL}` / `{NAME}` / `!`, you can now greet the other op by name and send/confirm QTH:
  `{HISNAME}` is the worked station's QRZ nickname (falling back to name), `{HISSTATE}` their
  state, and `{MYSTATE}` your own state (set it once in **Settings ▸ Station ▸ State**).
  `{HISNAME}`/`{HISSTATE}` fill from the callbook lookup and are keyed to the callsign, so a
  stale lookup can never key the wrong name; empty until a lookup resolves. Example:
  `! DE {MYCALL} UR {RST} QTH {MYSTATE} NAME {NAME} HW CPY {HISNAME}? KN`.
- **FlexRadio native SmartSDR panadapter — early access (opt-in).** For FlexRadio owners:
  **Settings ▸ Rig ▸ "Flex native panadapter (early access)"** streams the radio's real RF
  spectrum (SmartSDR VITA-49) into the cockpit scope, with **Flex-pan bandwidth + reference**
  controls in both the CW and Phone cockpits. Off by default and clearly marked unverified —
  needs a network Flex with its IP set (from Find Radios). If it doesn't paint or the app
  hitches, turn it back off. (Enable, test, and it becomes the default once proven on hardware.)

## [0.8.6] — 2026-07-14 — CI-V controls both cockpits, spot colours, two-way QRZ sync, tester fixes

### Added

- **CW + Phone cockpits: panadapter controls for the native scope (span + reference level).** When a
  FlexRadio or Icom CI-V scope is streaming, a control row sets the RF span (±2.5k up to ±250k) and
  the reference level directly from Nexus — the same knobs you'd reach for on the rig's own scope. On
  dual-scope Icoms (IC-9700/7610) the commands target the Main scope; single-scope rigs
  (IC-7300/705/905) omit the selector, matching each rig's CI-V format.
- **CW + Phone cockpits: RX DSP level controls (noise reduction + AGC speed).** Beside the DSP
  toggles, an NR-depth slider and a Fast/Mid/Slow AGC selector — read back from and written to the
  rig over CI-V (native path) or Hamlib, so what the cockpit shows matches the radio. Capability-gated
  (only appears for rigs that report it).
- **The CW cockpit reaches CI-V parity with Phone.** AGC speed, NR depth, and — when a native CI-V
  scope streams — the real RF panadapter (with RF-zoom + rig span/ref controls) now live in the CW
  cockpit too; the CW-narrow zero-beat audio view stays for rigs without a native scope. (Mic gain
  and the SSB TX meters remain Phone-only by design.)
- **Band Activity + Band map: spot colours now mean something, with a legend.** The flat Band
  Activity strip colours each spot by need tier (new entity / band / mode / grid / state / wanted),
  matching the vertical band map, and both show a P / S / ✈ badge for POTA / SOTA / DXpedition
  regardless of the need colour. A toggleable **Legend** explains the colours + badges (remembered).
- **The torn-off Band map remembers its place — and docks to a screen edge.** The vertical band-map
  pop-out reopens at the size + position you left it (no more re-arranging every launch), and new
  **◧ / ◨** buttons snap it to the left/right screen edge as a full-height strip.
- **Two-way QRZ logbook sync — pull your online QRZ logbook back down.** Until now Nexus only
  *pushed* QSOs to QRZ. **Settings ▸ Logbook & QSL ▸ QRZ ▸ "Sync from QRZ now"** now FETCHes your
  online QRZ logbook and merges it in: it **adds QSOs you logged elsewhere** (e.g. a phone logger in
  the field) and marks **QRZ-confirmed** contacts. QRZ-native confirmations count as confirmations
  but **not** toward DXCC/WAS (a separate tier, like eQSL) — so a QRZ match can never inflate your
  award counts. Safe to run repeatedly. Uses the per-logbook API key (not your QRZ password).

### Fixed

- **CW/Phone macro F-keys show your label again, not just "F1."** The label text had no explicit
  colour, so it inherited the button's default and could paint invisibly (dark-on-dark) — only the
  small F-key badge showed. Now pinned to the theme colour.
- **The torn-off Waterfall no longer stays always-on-top** — you can send it behind the main window.
- **The Connect tab renders correctly at 110% display scaling.** The 2-D map no longer collapses to
  zero height (and the side panes no longer clip) when the app is zoomed.
- **AGC speed buttons light up instantly** when clicked (they lagged ~1 s behind the rig read-back).

## [0.8.5] — 2026-07-14 — Native Icom phone toolkit (RF panadapter, TX meters, mic gain) + CI-V PTT fix

### Fixed

- **Native Icom CI-V: transmit no longer flickers the PTT (IC-9700 and friends).** With the native
  CI-V path on, hitting Tune or transmitting keyed the rig and then unkeyed it ~50 ms later — a fast
  "click," TX light but no RF. Two stacked root causes, found via the new CI-V diagnostic log:
  **(1) A Windows-only socket bug killed every CAT connection after ~one command.** On WinSock —
  unlike Linux, where all our tests run — a socket returned by `accept()` inherits the listener's
  non-blocking mode. The native daemon's rigctld listener is non-blocking, so every accepted
  connection's first idle read errored and the server closed it: Nexus's own rig-control link was
  silently reconnecting for *every command* all session. Accepted connections are now reset to
  blocking. **(2) The disconnect fail-safe stole our own transmit.** The daemon's rigctld server
  unkeys the radio when a PTT-asserting client disconnects (so a crashing WSJT-X/N1MM can't strand
  the rig keyed) — and the constant churn from (1) meant the connection that keyed always died
  moments later, unkeying the over. The fail-safe now stands down while Nexus itself is
  transmitting (published to the broker at every keying site, so there's no race), and still fires
  for a genuine external-client crash. (The scope-waveform stream is a separate matter — see the
  115200-baud fix below.)

### Added

- **Native Icom scope: the IC-9700's "no scope" mystery solved — it's the rig's baud requirement.**
  Per Icom's own CI-V reference, wave-data output over USB requires CI-V USB Baud Rate = 115200
  ("Unlink from [REMOTE]"); at lower rates the rig refuses to stream (NAKs the enable) even though
  CAT works fine. Nexus now: gates the scope stream at 115200 (matching the rig instead of inviting
  the refusal), pins the **Main** scope on dual-receiver rigs (IC-9700/7610) before enabling the
  stream, and spells out the exact rig menu settings in the native CI-V hint. If your waterfall
  shows no "CI-V RF": set the rig and Nexus to 115200.
- **Phone cockpit: the native scope is now a real RF panadapter.** When a FlexRadio or Icom CI-V
  scope is streaming, the Phone cockpit drops the audio-passband framing (the "RX audio" label and
  the audio-Hz span chips) and shows the rig's actual RF spectrum full-width, with RF zoom presets
  (Full / ±25k / ±10k / ±5k) instead of a passband-width sliver. Audio-derived scope is unchanged.
- **Phone cockpit: transmit meters (SWR / ALC / Po / COMP).** While you transmit, colored meter
  bars appear where the S-meter sits — SWR (antenna match), ALC (set your mic gain against it on
  SSB), output power in watts, and speech compression — using the exact IC-9700 calibration curves,
  so the readings match the rig. Only the meters your rig actually reports show; all blank on unkey.
- **Phone cockpit: mic-gain slider.** A mic-gain control beside the power slider (when the rig
  reports it) so you set SSB mic gain from Nexus while watching the ALC meter — no reaching for the
  radio. Mirrors the real rig level.
- **Native Icom CI-V: the DSP buttons (NB / NR / ANF / COMP / VOX) now work.** They were live only on
  the Hamlib path; on the native CI-V path the rig never reported the states, so the buttons stayed
  hidden. Nexus now reads and sets them over CI-V, so the cockpit's DSP toggles light up and work.
- **CI-V bus diagnostic log (Settings ▸ native Icom CI-V).** An opt-in support tool that records the
  raw CI-V bus traffic — every byte to and from the radio, timestamped and decoded (PTT on/off, mode
  set, scope waveform, ack…) — to a file in your Downloads. It's the way to root-cause hardware-only
  native-CI-V faults (like the IC-9700 PTT flicker on transmit): turn it on, reproduce the issue,
  turn it off, and the capture shows exactly what's on the bus during the fault. Off by default,
  not persisted, and free when off (the engine only taps the wire while it's armed).

### Changed

- **FT8 Call Roster now leads with the callsign, then the Need column.** Callsign is the first thing
  operators scan, so it moves to the front; the Need column (need chips + rarity pill) follows it,
  reading as "why you'd want this station" right after the call.

## [0.8.4] — 2026-07-13 — Spot to cluster, band-edge tones, LoTW count

### Fixed

- **Icom stays in DATA-U on FT8 through Tune and Transmit.** Tuning used to drop an Icom already in a
  data mode (PKTUSB / DATA-U) back to plain USB: the tune keys in DATA mode (a plain-USB Icom needs
  that to radiate a tune tone), but on release it forced DATA back *off* unconditionally. It now
  restores the mode you were in before tuning, so an FT8 operator holds DATA-U while a plain-USB tune
  still keys with output and returns to USB.
- **Native Icom CI-V (early access): the scope stream now pauses during transmit** to keep the
  shared CI-V bus clear while keyed — part of ongoing work on IC-9700 TX reliability on the native
  path. (If you hit PTT trouble on native CI-V, the standard Hamlib CAT path is the stable one.)

### Added

- **Startup splash screen** — a borderless splash window shows a branded image on launch for a few
  seconds while the app loads behind it, then the main window opens (classic desktop-app style).
- **Spot a callsign to the DX cluster** — a "📢 Spot" button in both the FT8/Digital and Phone
  cockpits opens a popup pre-filled with the callsign, dial frequency, and an editable comment, and
  posts it to your connected cluster (rejects if none is connected). In FT8, the roster's per-station
  spot now opens the same reviewable popup.
- **Band-edge audio cues** — a rising "ding" when you dial back into your license privileges and a
  falling "dong" when you stray past an edge, so you hear the band edge without watching the readout.
  New toggle in Settings ▸ Operating ▸ Transmit & Sequencing (on by default).
- **"Mark on LoTW" bulk action** (Logbook) — if you imported a log that's already on LoTW via another
  tool, one click marks it so the "Upload to LoTW" count reflects reality instead of offering a large
  redundant re-upload. Nothing is sent; only Nexus's own record changes.

### Fixed

- **The "Upload to LoTW (N)" count no longer over-counts an imported log.** Import now honors the ADIF
  `LOTW_QSL_SENT` field, so a QSO already uploaded to LoTW isn't counted as still needing an upload.
- **FT8 Call Roster "Need" column is wider** so all the need chips are visible, and the 💎 rarity pill
  now shows there (it was being clipped in the narrow grid column).

## [0.8.3] — 2026-07-13 — CW/POTA fixes + phantom-log guard

### Fixed

- **Logbook "Export ADIF/CSV" reliably saves a file.** It now writes the export straight to your
  Downloads folder and shows the exact saved path, instead of a browser-style download that could
  silently fail in the app window. (Audited every Logbook button in the process — the rest were fine.)
- **The CW decoder's AI on/off switch stays put.** It no longer jumps from mid-row to the left when
  the AI decoder's status text appears and clears — it's parked next to the DECODE label.
- **No more phantom or duplicate auto-logged QSOs.** A single decoded `RR73`/`73` addressed to you —
  from a double-click, or a companion app auto-replying across cycles — could log a "completed" QSO you
  never actually worked, and with no duplicate guard the same contact could be logged (and uploaded)
  more than once. Auto-log now requires real evidence the contact happened (you transmitted *and* a
  signal report was exchanged), and a duplicate guard blocks logging the same call/band/mode twice in a
  short window — across every path into the log (auto, cockpit button, manual, companion).
- **CAT errors now name the actual fault instead of blaming the mode.** A failed mode change used to
  always read *"rig rejected PKTUSB"*, even when the real problem was the CAT connection. It now tells
  the three faults apart: *"can't reach the radio's CAT link"* when nothing is listening (rigctld or
  SmartSDR not running — the Windows `os error 10061` / *"target machine actively refused it"* case);
  *"no reply from the rig over CAT"* when the link is up but the radio never answers (rig off/asleep,
  wrong CAT port or model, serial baud mismatch, or SmartSDR not actually connected to the radio — the
  *"rig reply incomplete"* case); and *"rig rejected …"* only for a true rejection, where the radio
  answered but has no such mode (e.g. no DATA/PKT submode).
- **A clearer message when a QRZ callbook lookup has no password.** Looking up a call with a QRZ
  username set but no QRZ *password* stored used to report *"… is not in the callbook"* — even for calls
  that clearly are. It now says the lookup needs your QRZ password, and points out that the callbook
  lookup uses your QRZ.com login password, not the separate Logbook API key (a common mix-up). The
  Settings row is relabelled **"QRZ callbook (name/QTH)"** to match.
- **The Connect tab no longer breaks its layout at 110%+ UI zoom.** Its propagation panes now reflow on
  the zoom-adjusted width like the rest of the app.

### Added

- **Clear button on the log form** — one click resets the fields and returns focus to the callsign.
- **QRZ nickname** is shown in place of the full name when the operator has set one on QRZ.
- **CW cockpit Band Activity shows only the CW portion** of the band, instead of the whole allocation.
- **POTA/SOTA spot mode-filter is remembered** across sessions — pick CW (or SSB, FT8…) once and it
  sticks. Defaults to All so phone hunters see every spot out of the box.
- **Import your POTA "Hunted Parks.CSV"** (from the POTA stats page) to drive the NEW PARK flags — so
  hunts made on CW, where the park number never reaches your log, still show as worked.
- **Waterfall pop-out frees the main-window space** — the docked waterfall unmounts while it's popped
  out, and re-docks when you close the pop-out (or via an always-there "re-dock" button).
- **LoTW "sign from the ADIF location"** (Settings ▸ Rig/LoTW) — for travelers who set TQSL to use the
  location in the ADIF and never create named Station Locations. Nexus stamps `STATION_CALLSIGN` /
  `MY_GRIDSQUARE` into the upload and omits the `-l` argument. Default stays named-location.

## [0.8.2] — 2026-07-13 — Settings declutter + upload/credential hardening

### Improved

- **Settings are much easier to navigate.** Every crowded screen is now grouped into labelled
  sub-sections: **Operating** (Transmit & Sequencing · Auto-CQ · Logging · Decoder · Housekeeping);
  **Logbook & QSL** (a section per service — LoTW · eQSL · QRZ · HamQTH · ClubLog · HRDLog ·
  Cloudlog); and **Integrations & Feeds** (Local Loggers · Spot Sources · Propagation). Rarely-touched
  Rig/CAT controls (CAT broker, Flex IP, Icom CI-V, rigctld port) and the phone-only FM knobs now sit
  behind collapsible **Advanced** / **Phone / FM** groups so the everyday settings aren't buried.

### Fixed

- **Auto-upload no longer drops a QSO on a network hiccup.** A transient failure (connection down,
  service busy) now re-queues just the connectors that failed and retries them — without re-sending
  the ones that already succeeded — instead of silently giving up. A definitive rejection (bad key)
  isn't retried, and a permanently-down service stops after 20 attempts.

### Security

- **The Cloudlog/Wavelog API key is now stored in the OS keychain**, not in `settings.json`. Any key
  saved by an earlier build is migrated into the keychain on first launch and scrubbed from the file;
  the Settings field is now write-only, matching every other credential.

## [0.8.1] — 2026-07-12 — Field Day run fix + audit hardening

A fast-follow after a full white-box QA + security audit of 0.8.0.

### Improved

- **Ultra-rare grids are now unmistakable.** An open-water (rover/maritime/DXpedition-only) grid gets
  a loud, glowing **💎 ULTRA** pill on the primary line of the Call Roster and in the band-activity
  feed — the old marker was a tiny ◆◆ that was easy to miss — and it now persists through the whole
  QSO, not just the CQ. Rare grids stay a quiet marker so the boards don't become confetti.
- **The Call Roster shows every reason a station is worth working.** It previously showed only the
  single top need; it now shows one chip per need form (new-DXCC, band, zone, grid…), matching the
  band-activity feed.
- **Focus returns to the callsign field after you log a contact** in the CW and Phone cockpits, so
  you can type the next call immediately (rapid logging / a Field Day run).
- **Settings are easier to navigate.** The two most overloaded screens are now grouped: **Operating**
  is split into Transmit & Sequencing / Auto-CQ & Caller Selection / Logging Behavior / Decoder /
  Station Housekeeping, and **Confirmations** is renamed **Logbook & QSL** with a section per service
  (LoTW · eQSL · QRZ · HamQTH · ClubLog · HRDLog · Cloudlog) — and Cloudlog is no longer stranded in
  the Field Day tab.

### Fixed

- **Field Day RUN mode now works a whole run.** A running station (calling CQ FD) worked exactly
  ONE contact and then went silent. It now returns to calling CQ after each logged QSO (and
  Search-&-Pounce returns to listening), so you can actually run a pileup.
- **A corrupt or crafted ADIF file can no longer crash the app.** A stray multibyte character in a
  date/time field, or a bogus field length, could panic or hang the log parser (taking TX/RX/CAT
  down until restart). Malformed records are now read safely — this covers imported logs and
  downloaded LoTW/eQSL reports.
- **A CAT-sharing client that drops mid-transmit now unkeys the rig.** If WSJT-X or N1MM crashed
  or closed while keyed through Nexus's rig broker, the radio could stay transmitting; a dropped
  broker connection now fail-safe unkeys.
- **CW stops cleanly on Monitor / TX-off** — queued CW no longer survives to key the rig when you
  re-enable transmit.
- **Completed QSOs aren't lost with "Auto-log QSOs" off** — the cockpit's Log QSO button now
  captures the finished contact instead of it being discarded.
- **Field Day Cabrillo export** stamps each QSO with its own band's frequency (a multi-band log
  used to write one frequency on every line).
- **Field Day log** no longer flags legal multi-band / multi-mode contacts of the same station as
  duplicates.
- **eQSL upload** failures are now labeled "eQSL" (they were mislabeled "QRZ").
- **Cloudlog / Wavelog upload** reports a real failure instead of a false "✓" when the instance
  rejects a record, and requires the API key + station id up front.
- **A "Spots" section you enable in Settings is now reachable** from the navigation rail.
- Assorted correctness: manual Field Day entry requires a valid ARRL/RAC section (no phantom
  multiplier); the WAS "by US state" stats and the "New state" needed-tag only count US contacts;
  "First DX" unlocks on your first foreign entity even before a domestic one; a manual rotor slew
  halts an active satellite track instead of fighting it; the "Contesting" setup goal lands on a
  reachable view; and the CW/Phone keyboard shortcuts read your live transmit-allowed state.

### Security

No critical or remotely-exploitable issues were found in the audit; these are defense-in-depth on
a single-user desktop app. Hardened the ADIF parser (UTF-8 char-boundary panic + integer-overflow
DoS), the LoTW upload temp file (unique unpredictable name, no symlink-follow, removed after use),
Cloudlog HTTPS + no-redirect enforcement (matching every other connector), and sanitized the band
value used in the debug period-WAV filename. Bumped `anyhow` to clear an advisory.

## [0.8.0] — 2026-07-12 — Field Day mode, readable light theme, and operating fixes

### Added

- **One-switch Field Day mode.** A single "Field Day mode" toggle in Settings turns on
  everything at once across Phone, CW, and digital — the Class+Section exchange, logging,
  scoring, dupe-checking, and the connectors. It's off (and completely invisible) the rest of
  the year, never turns itself on, and — once you enable it — survives a restart so a crash
  mid-event comes back operating with your log intact. Summer Field Day and Winter Field Day
  are selected automatically by date (with a manual override), each with its own rules.
- **Worked-sections board.** A colored ARRL/RAC section grid (all 83 sections, grouped by
  division) that lights up each section as you work it — see your coverage at a glance.
- **Club Log / N3FJP Field Day networking.** Nexus now logs into N3FJP using the contest-correct
  ENTER path (so your Class and Section actually score), and can report your band to the club's
  N3FJP network display without needing CAT on the N3FJP side.
- **CW Field Day macros** — new `{CLASS}` / `{SECTION}` / `{EXCH}` macro tokens send your
  exchange, plus a default Field Day macro set; a "Give: 3A WI" exchange prompt on Phone; and
  Winter-Field-Day operating from the Tempo chat cockpit.
- **Field Day exports** — one-page score summary and a dupe sheet alongside Cabrillo/ADIF, and a
  section-validated setup so you can't mistype your ARRL section.
- **Pop-out Field Day scoreboard** with a settable operator call that's passed straight through to
  N3FJP, plus timestamps on the Field Day call log and a larger Call/Class/Section entry.
- **Custom F-key macro profiles for CW** — save multiple named macro sets (per operator or per
  activity) and switch the active one from the CW cockpit; your existing macros become the
  "Default" profile automatically.
- **Roster is the default FT8/FT4 layout** (the friendlier at-a-glance view) — Classic is still
  one click away and your choice sticks.

### Changed

- **Light theme is much easier to read** — stronger surface hierarchy (panels lift off the page),
  softer off-white surfaces instead of harsh pure white, and clearer tables, chips, and status
  tints. Dark mode is unchanged.
- **Amber theme removed** — its monochrome palette flattened the color language; anyone on amber
  is moved to dark. (The amber-CRT *waterfall* color scheme stays.)

### Fixed

- **CW decode clears on QSY** — changing bands or clicking a Needed contact while operating CW
  now clears the CW decode window instead of leaving stale copy from the old frequency.
- **Two radios on one COM port now warns you** — configuring two radios on the same serial port
  (which left one showing a mysterious red status) now shows a clear "same COM port" message.
- **Light/Dark toggle now reachable in the Phone and CW views** — it was rendering but bunched to
  the left where it was easy to miss; it's now pinned to the top-right in every view.

## [0.7.1] — 2026-07-12 — Club Log upload enabled

### Added

- **Club Log realtime upload** is now active in the official builds — the app's developer
  API key is baked in, so you just add your own Club Log email + application password (and
  callsign if it differs) in Settings and enable auto-upload; each logged QSO is pushed to
  Club Log in real time. (The developer key is injected at build time and never committed to
  source, per Club Log's terms.)

### Fixed

- **The Field Day contest log now survives restarts.** Contacts are journaled to
  `fieldday_backup.adi` as they are logged and restored whenever you re-enter Field Day
  mode — a mid-event restart, crash, or Run ↔ Search-&-Pounce switch no longer clears the
  log or the dupe sheet. The journal carries real timestamps, so a recovered log still
  produces a valid Cabrillo entry. Entries from a previous event (over 4 days old) are
  not restored.
- **Settings can no longer be lost to a torn write.** The settings file is flushed to disk
  before the atomic swap, and a corrupt or unreadable `settings.json` (disk fault, hand
  edit, a virus scanner holding the file at startup) is preserved as
  `settings.json.corrupt` for recovery instead of being discarded. The app still starts
  from defaults in that case — re-check your callsign and license class — but your
  original settings can be recovered from the `.corrupt` file.
- **The Phone/CW scope now shows the right slice of the band on a native panadapter**
  (Flex SmartSDR / Icom CI-V). The view window was collapsing to a sliver ~100 kHz below
  the dial; it now centers on the dial with the CW zero-beat marker exactly on frequency,
  and the scope label reports the true RF span in MHz. Span and pitch changes also
  retarget the scope immediately instead of waiting for a re-open.
- **A dead audio stream no longer scrolls a frozen waterfall.** If the RX capture stops
  (device unplugged, DAX stream lost — e.g. RDP remote audio hiding the devices), the
  scope goes quiet instead of replaying the last captured row as phantom signals. A new
  Troubleshooting entry covers the RDP/DAX device-visibility case.

## [0.7.0] — 2026-07-12 — Optional 3-D WebGL Connect globe

### Added

- **3-D Connect globe (opt-in)** — a cinematic WebGL globe for the Connect map, toggled with
  the 🌐 button in the map header. A dark night-earth with dimmed city lights, a day/night
  terminator + greyline, atmosphere and bloom, band-colored clickable spots, and great-circle
  arcs to the stations you're working / that heard you.
- **Full layer parity in 3-D** — the same operating layers as the 2-D map, in the Layers
  panel: solar-flare blackout, aurora, MUF, proton polar cap, band-heat openings, CQ zones,
  range rings, coverage, your decodes, DXpeditions, US states, and the greyline.
- **Satellites with real 3-D orbits** — amateur birds actually orbit the globe at their true
  altitude, with footprint rings and live motion — not a flat ground track.
- **Automatic 3-D on capable machines** — on first run, PCs with a real GPU default to the
  3-D globe; low-end or software-rendered machines stay on the universal 2-D map. Your choice
  always overrides, and the 3-D engine is lazy-loaded so the 2-D default never pays for it.

## [0.6.0] — 2026-07-11 — AI CW decoder as primary, dual-radio TX-safety, operating polish

### Added

- **AI CW decoder is now THE decoder** — the neural-net (DeepCW) copy powers the CW
  cockpit's DECODE pane as a flowing transcript with a Clear button; dramatically better
  weak-signal copy. The CW copilot's call chips + guided next-step now read the AI copy.
  The classic decoder remains as the automatic fallback (and supplies the WPM estimate).
- **Customizable CW F-keys** — Settings ▸ Quick-reply Macros: edit each F1–F8 label +
  template (N1MM-style; {MYCALL}/{RST}/{NAME}, ! = worked call). Keys keep their roles, so
  the guided copilot's recommended-key highlight keeps working with custom text.
- **Waterfall pop-out** — tear the FT8 waterfall off into its own always-on-top window.
- **Resizable panels** — drag the FT8 waterfall height and the CW/Phone scope heights;
  sizes persist.
- **Live input spectrum in Settings audio** — confirms the right input device at a glance.
- **Band Scope pane for Connect** — the active radio's spectrum on the map screen.
- **Connect globe upgrade** — US state borders (read which state a spot or your QTH is in),
  a clear "you are here" QTH marker, and a moodier night-earth globe so the colored spots
  stand out. All in the universal 2D map (a high-fidelity 3D mode is planned for later).
- **Prominent band picker** — the CW/Phone band selector is now a large, band-colored
  control (matching the map's per-band spot colors) so your operating band reads at a glance.
- **Open-source compliance** — the DeepCW model's full AGPL-3.0 license text now ships with
  the installer (`resources/deepcw/`), and NOTICE credits the model and its corresponding
  source (e04/deepcw-engine) plus us-atlas for the runtime map data.

### Fixed

- **A stuck transmitter now recovers by itself.** A transient CAT failure could leave the
  radio keyed with the app unaware (TX/RX light on until a radio reboot). PTT tracking is
  now fail-safe, every teardown path force-unkeys, the native CI-V daemon sends a safety
  key-up as it closes, and an idle self-heal retries key-up until the radio acknowledges.
- **Tune on Icoms in SSB now makes RF** (DATA mode is engaged for the tune so the tone
  modulates; plain USB takes TX audio from the mic jack).
- Radio-switcher pill no longer flashes on a single slow poll; wedged native-CAT sessions
  no longer freeze the UI; several native-daemon robustness fixes.
- **Switching radios now moves control instantly.** A switch could leave the pill on the new
  radio while CAT kept commanding the old one for a while before catching up — the handoff
  no longer applies any change until it has fully taken over the new radio, so control
  follows the pill the moment you switch.

## [0.5.2] — 2026-07-11 — native panadapter (early access) + logger forwarding + watch list

### Added

- **Native Icom CI-V (early access, off by default)** — a per-radio toggle in Settings ▸ Rig
  for scope-capable Icoms (IC-7300 / 7610 / 9700 / 705 / 905) on a serial connection. Nexus
  drives the rig's CI-V directly instead of launching Hamlib's rigctld: the waterfall shows
  the radio's **real spectrum scope** ("CI-V RF" badge) instead of soundcard audio, and dial
  tracking becomes instant (the rig pushes frequency changes as you turn the knob). All CAT —
  frequency, mode (incl. USB-D for FT8), PTT, S-meter, power, CW keying, split, RIT, FM
  repeater duplex/tone — runs over the same native link. Requires the rig's CI-V USB baud at
  115200 for the scope stream (lower rates stay CAT-only). Turn the toggle off any time to
  return to the classic Hamlib path.
- **FlexRadio native panadapter** — when the active radio is a Flex (SmartSDR, network CAT)
  with its radio IP set, the waterfall streams the radio's true RF FFT ("FLEX RF" badge),
  with automatic fallback to the audio scope if the stream drops.
- **Watch list** — tell Nexus the calls, prefixes (`VP8*`), or entities you're hunting
  (Settings ▸ Alerts) and a decode or spot of one fires the loudest alert tier, above
  needed/new-DXCC.
- **N3FJP ACLog forwarding for everyday logging** — every QSO you log can now push to N3FJP
  ACLog in real time (not just Field Day), with duplicate protection.
- **Cloudlog / Wavelog forwarding** — log each QSO straight to your self-hosted
  Cloudlog/Wavelog instance (URL + station profile + API key in Settings ▸ Logging).
- **"My coverage" map layer** — shade the globe by where you've been heard/worked, by grid
  square or CQ zone, as a proper toggleable map layer with its own opacity.

## [0.5.1] — 2026-07-10 — dual-radio on-rig fixes

On-rig fixes from testing 0.5.0 with an FTDX10 + IC-9700 (HF + VHF on separate antennas).

### Fixed

- **Transmit worked on only one radio after switching.** After swinging to the other rig, its
  frequency and mode still tracked but PTT/transmit did nothing (it "keyed once, then never again").
  The switch adopted the radio's live background connection, which is opened read-only for
  monitoring — so it stayed in listen-only keying. The handoff now restores the radio's real PTT
  method (CAT / RTS / DTR) when it becomes active, and puts the radio you switched *away* from back
  into read-only monitoring.

### Added

- **Automatic band-routing.** Selecting a band (or typing a frequency) now switches to the radio
  configured for that band — pick 2 m and it moves to the VHF rig, pick an HF band and it swings
  back — instead of retuning whichever radio was active. A radio's explicit band list wins the bands
  it claims; a radio left with no band list is the catch-all for everything else. Turn on **peg-lock**
  in the top-bar switcher to pin the active radio and stop any auto-switching.

## [0.5.0] — 2026-07-10 — operating experience + dual-radio

Field-test-driven work on the day-to-day operating experience (waterfall fidelity, a prominent
frequency readout, dial latency, logbook scale) plus the start of true dual-radio support.

### Added

- **Dual-radio — run two rigs at once** (e.g. an HF radio + a VHF/UHF radio on separate antennas).
  Add a second radio in Settings ▸ Rig; a switcher appears in the top bar. Both rigs stay
  **permanently connected** — the non-active radio is monitored live (its frequency/S-meter show in
  the switcher) and switching is an instant **handoff** with no CAT teardown, so the dial never
  bounces. Invisible for single-radio stations (only a quiet "+ Add radio" button appears). Each
  radio has its own CAT/audio/rotator config and band-coverage set; daemon ports are auto-assigned
  distinct and auto-repaired on load.
- **Prominent, unified frequency readout** — a large, accent-colored MHz display shared across the
  digital, CW, and Phone cockpits; click to type an exact frequency.
- **Universal FFT waterfall** — every rig's audio scope now uses a real 4096-point FFT (~7.8 Hz/bin
  across 0–4000 Hz) instead of the old coarse filter bank, so even a Yaesu's soundcard waterfall
  resolves close signals.
- **Mouse-wheel tuning** — scroll over the scope **or the big frequency readout** to tune by the
  selected step (Shift = ×10); great for hunting CW/phone signals off the FTx default frequencies.
- **POTA park auto-load by reference** — type a park number in the log entry and its name/location
  fills in from the local index, with a live `api.pota.app` fallback.
- **Optional ADIF import at first-run** — the setup wizard now offers to import your existing log up
  front (skippable), so the needed/worked-before/awards intelligence works from day one.
- **Per-radio standard baud dropdown** in the Rig settings (1200–115,200) instead of free text.
- **Tune & Stop-TX controls in the Phone and CW cockpits** — a **Tune** button keys a steady carrier to
  tune an ATU or amplifier (auto-released by the TX watchdog), and **Stop TX** unkeys everything instantly
  (PTT, tune carrier, and CW keying). Restored — these were missing from the voice/CW cockpits.

### Changed

- **Fast dial tracking** — the rig's frequency is now polled on a short (~180 ms) sub-cadence,
  separate from the slower S-meter/mode/power reads, with transport-aware read deadlines, so the
  dial keeps up with the VFO knob (matching HRD-class responsiveness on Yaesu).
- **Mode changes keep the rig's filter width** — switching bands/modes no longer forces the rig's
  passband to its default (which was popping the Width display); explicit width changes still apply.
- **Logbook performance at 10k+ QSOs** — the logbook list is virtualized and its filter/sort
  memoized, so large logs scroll smoothly instead of lagging.

### Fixed

- **FTx Call Roster overlap** — need-chips (e.g. NewZone) no longer spill over the callsign, and the
  Call column fits longer calls like VE2OPR.
- **Settings-tab crash hardening** — audio/serial device enumeration is now panic-isolated, so a
  quirky/virtual device (some Flex DAX / RDP-remote-audio setups) can't crash the app when opening
  Settings.
- **Dual-radio CAT no longer dies on the background radio.** Saving a radio's config could leave the
  active radio and the monitored radio fighting over the same daemon port, so CAT went dead on whichever
  radio wasn't active — and flipped when you switched. The daemon port is now always re-synced after
  de-confliction, so CAT stays live on **both** radios in either direction.
- **Per-radio audio on rigs with a generic USB codec.** Two rigs that both enumerate as "USB Audio CODEC"
  are now listed as distinct entries ("USB Audio CODEC", "USB Audio CODEC #2"), so each radio can point at
  its own soundcard; previously both silently resolved to the first codec.
- **Radio soundcards that use 8-bit or 24-bit audio** (some Icom USB codecs) now open correctly for RX
  capture, TX, and the headphone monitor — they were failing with an "unsupported format" error.

_(Protocol decoders for a native FlexRadio panadapter and a per-radio native scope are in progress
behind the scenes; not yet user-visible.)_

## [0.4.1] — Phone / POTA / CAT punch-list

Field-test fixes and polish for voice/CW operating, park activations, and rig tuning.

### Added

- **POTA/SOTA logging** — a park/summit field in the log entry, an OTA column in the logbook, an
  activation mode that tags every QSO, and standard `SIG`/`SIG_INFO`/`SOTA_REF` ADIF.
- **Local POTA park search** — a bundled, refreshable park index for offline park lookup.
- **CAT tuning from the Phone/CW cockpits** — direct frequency entry, VFO up/down step tuning,
  RIT/XIT, and A/B VFO select (a Win4-style rig-control panel).

### Changed

- **De-FT8'd Phone & CW cockpits** — the top bar no longer shows FT8/digital furniture in voice/CW;
  each mode keeps its own controls. Sortable logbook columns; clearer hunt-chip visibility;
  smart-Enter QRZ lookup.
- **Smoother FTdx10 (and general rig) setup** — Auto-test seeds the detected model, with a callout
  when no model is set, and clearer rig hints.
- **Phone bandscope perf + clarity** — cached spectrum row, a you-are-here dial marker, a passband
  overlay, and honest labels.

### Fixed

- Auto-test wrong-model guard, park-prefill honesty, CSV BOM on export, and tuning-entry fixes from
  the review pass.

## [0.4.0] — band map, log stats, weak-signal CW, callbook photo, filter width

### Added

- **Vertical pop-out band map** — an N1MM-style frequency map of live cluster spots for the Phone
  and CW cockpits, colored by award need with worked calls struck through; click a spot to QSY to
  its exact frequency and prefill the log (including from the pop-out window).
- **Full-band activity strip** — a clickable spot strip spanning the whole band with a you-are-here
  dial marker; your licensed phone sub-band is shaded per US license class.
- **Logbook Statistics** — QSOs by band / mode / year / hour-of-day, top DXCC entities, WAS states,
  confirmation rate, plus continent, CQ-zone, and DX-vs-domestic breakdowns (cty.dat-resolved).
- **Weak-signal CW decode** — the decoder now gates on true SNR against off-pitch band noise, so the
  sensitivity slider genuinely trades copy against noise and the "E E E" storm between signals is gone.
- **Real CAT S-meter** — the Phone scope meter reads the rig's actual STRENGTH over CAT (S0–S9+60);
  shows "—" rather than faking a level when the rig doesn't report it or during TX.
- **RX filter-width control** — read/set the rig's passband over CAT from the Phone and CW cockpits
  (CW defaults narrow at 500 Hz to dig signals out of QRM).
- **Rig DSP toggles** — NB / NR / auto-notch on Phone and CW, plus COMP and VOX on Phone;
  capability-probed so only functions your rig reports are shown.
- **Manual split + sideband override on Phone** — one-click "work up N" split with an offset stepper,
  and a USB/LSB/FM override that reverts to the band-correct sideband on a band change.
- **Callbook photo + worked-before recall card** — the "B4" hint grew into a full recall panel:
  QRZ/HamQTH profile photo, prior contacts, distance/bearing from your QTH, and a same-band dupe flag.
- **Split RST fields** — separate Sent / Rcvd reports in the log entry (the CW decoder fills Rcvd).
- **Auto callbook lookup** — name/QTH fill shortly after you stop typing a call, no Tab needed.
- **Update check** — on launch (throttled to once a day) Nexus checks SourceForge for a newer
  release and shows a dismissible notice, with a manual check in Settings; it only opens the
  download page, never downloads or runs anything.

### Changed

- The redundant top-bar band dropdown (fed by the digital band plan, so a wrong-dial control on
  voice/CW) is hidden on Phone and CW; each cockpit keeps its own band picker.

### Fixed

- A periodic scope/passband stall: the slower CAT reads (mode, S-meter, DSP functions) are now
  staggered across poll cycles instead of stacking into one.
- The 4 m band (70.0–70.5 MHz) is now recognized by the UI band ranges, matching the backend plan.

## [0.3.0] — the Nexus transformation

**Tempo became Nexus.** What began as a chat-first app for the FT1/DX1 waveforms
is now an **all-mode amateur radio operations center**; the Tempo name lives on
as the FT1/DX1 chat layer inside it. Builds now ship as
`Nexus_0.3.0_x64-setup.exe` — the first versioned Nexus release.

### Added

- **FT8/FT4 operating tier with WSJT-X operational parity** — a five-phase
  program against a 207-row behavior matrix: the WSJT-X auto-sequencer state
  table (double-click semantics, sender lock, return-to-CQ, disable-after-73),
  early decode pass (11.8 s FT8 / 5.5 s FT4) + 2 s time-aligned late start,
  Split Operation (Rig / Fake It) with a single teardown drain, Hound mode with
  safe Fox-frame splitting, directed CQ, Tx1–Tx6 panel, WSJT-X keyboard
  shortcuts, F6 redecode, decode depth/passband controls, logbook hash-table
  seeding, Classic ↔ Roster layout toggle, and chronological bottom-pinned Band
  Activity with period separators.
- **Full WSJT-X UDP ecosystem surface** — outbound Heartbeat/Status/Decode/
  QsoLogged and inbound Reply, HaltTx, Clear, Replay, Location,
  HighlightCallsign, using the canonical NetworkMessage.hpp type numbers
  (pinned by test); JTAlert and GridTracker interop verified. Plus **Companion
  mode** (ride an upstream WSJT-X/JTDX decode stream) and a **rigctld-compatible
  CAT broker** so other shack software shares the radio through Nexus.
- **CW cockpit** — CAT (`send_morse`) and soundcard keyer back-ends, 5–50 WPM
  with on-the-fly nudge, eight token-expanding macros, zero-beat scope,
  automatic rig-mode policy, license-privilege TX gating, 599-default logging.
- **Phone cockpit** — live dial read-back, band-correct sideband policy, fast
  colored bandscope, spacebar/button/rig PTT with stuck-TX safeties, six-slot
  voice keyer (record/import WAV), crash-safe QSO recording, RF power control.
- **Needed board 2.0** — eight need types ranked by award value with a per-row
  **evidence line** ("heard by K9LC (EN52, 26 km), 4 min ago"), corroboration
  gates (near-receiver geometry, VHF two-receiver rule, Es-patch locality),
  persisted filters, atomic one-click work with cluster split-comment parsing
  ("UP 2" → rig split), and a pop-out second-monitor window.
- **POTA/SOTA hunter** — live activator spots, NEW PARK and BAND OPEN badges,
  one-click HUNT (QSY + cockpit + pending park tag with a 4 h TTL and base-call
  matching) writing standard `SIG`/`SIG_INFO`/`SOTA_REF` ADIF.
- **Field Day event mode** — ARRL FD + Winter FD with correct date rules and
  scoring (per-mode points, dupes per band per mode, legal power tiers, bonus
  checklist), all-mode event logging from the CW/Phone cockpits, band-follows-
  QSY, submittable Cabrillo 3.0/ADIF, **real-time N3FJP push** over the official
  TCP API (with Test button) and **native N1MM+ `<contactinfo>` broadcast**.
- **Logbook, awards & connectors** — ADIF 3.1.4 round-trip logbook; offline
  DXCC / Challenge / Honor Roll / WAS / WAZ from cty.dat; **source-aware
  confirmations** (eQSL never counts toward LoTW-grade awards); LoTW TQSL-signed
  upload + two-pull incremental confirmation sync over direct HTTPS; QRZ callbook
  autofill + logbook push + Test; ClubLog (bring your own free API key) and eQSL
  connectors; per-QSO upload state machine persisted in ADIF;
  prior-QSO history panel; credentials exclusively in the OS keychain; and the
  local-only **Journey** achievement layer.
- **Connect** — three-projection world map (3-D globe / azimuthal beam / flat)
  with 12 layers, intent presets, hover/click/double-click-to-work; an
  operator-anchored **opening detector** with reciprocity gates and Es/F2/
  aurora/tropo classification; band advisor; getting-out panel; NOAA space
  weather; and the persistent Now-Bar with feed-health pills.
- **Zero-config setup** — **Detect my radio** (USB descriptor → rig model +
  driver hint + paired audio CODEC), goal-driven first-run wizard, license-class
  transmit lockout (FCC Part 97 sub-bands incl. the 2026 60 m rules), DAG-
  validated feature registry, detached panel windows, NTP slot-grid steering.

### Changed

- **App renamed Tempo → Nexus**; repository moved to `kd9taw/nexus`.
- FT8/FT4 is now the production tier; FT1/DX1 remain beta pending on-air
  validation (unchanged honest framing).
- Field Log merged into the Field Day workspace; the Logbook is the single log.

### Removed

- **SuperFox** — investigated and abandoned: the WSJT-X QPC table file is
  licensed "only for use with WSJT-X", which bars vendoring. Hound remains.
- **Broadcasts section** — removed from the UI (the underlying announce/Roam
  machinery remains for Coordinated QSY).

### Fixed

- PSK Reporter uploads declared the mode string under IPFIX enterprise field 7
  (iMD — a PSK31 distortion metric) instead of field 10 (mode), so every spot
  arrived modeless and pskreporter.info displayed its default, PSK31 — FT8
  decodes showed up as "PSK31" on FT8 frequencies. Field id corrected to match
  WSJT-X's PSKReporter.cpp; spots now carry FT8/FT4/FT1/DX1 correctly.
- WSJT-X UDP message type numbers were shifted +1 for types ≥ 8 (a real JTAlert
  FreeText datagram parsed as HaltTx and killed TX) — now canonical and pinned.
- FT4 transmitted at slot +0.0 s instead of the standard +0.5 s timing.
- Split restore could strand a shifted VFO through the UDP HaltTx and tune
  paths; Rig split could latch VFO B.
- Field Day log band was frozen at event entry — post-QSY contacts exported
  with the wrong band and corrupted dupe checking.
- Winter Field Day date math used "last Saturday of January", a week late in
  years like 2026 — now "last full weekend".

## [0.2.0] - 2026-06-03

This is a **beta / pre-release**: everything below is simulation- and
Windows-cross-build-validated, **not yet proven on the air**. On-air
decode-rate-vs-SNR remains the open gate.

### Added

- **IR-HARQ is live end-to-end.** The incremental-redundancy retransmission
  combiner — previously designed-but-dormant (simulation-only) — now runs
  through the full live pipeline and is **on by default**. A frame that fails
  to decode standalone (RV0) is buffered and **joint-turbo-combined** with its
  retransmissions: RV0 carries the base 174 bits; RV1/RV2 each carry 87 new
  punctured LDPC(348,91) parity + 87 repeated systematic, each with a distinct
  Costas sync (RV0 `[0,2,3,1]`, RV1 `[1,3,2,0]`, RV2 `[3,0,2,1]`). Slot expiry
  30 s, freq tolerance +-10 Hz. A coherent CPM-Costas discriminator
  (`ft1_rv_detect`) identifies the RV (>99% accurate, <1% false to -11 dB),
  and the QSO sequencer drives RV escalation (0->1->2 on implicit NAK, reset on
  implicit ACK). Simulated (AWGN/fading sweeps): combiner **+1.3 dB** AWGN and **+3.2 dB** under
  1 Hz / 1 ms fading (3-TX); through the full live pipeline ~**+2.5 dB**
  threshold shift and ~**2x QSO completion** in the -11..-13 dB zone. UI adds a
  **HARQ.RVn decode badge**, a **HARQ on/off toggle** (default on), and a
  **session rescue counter**; `Decode.rv` reports how many RVs were combined.
- **DX1 full-passband acquisition.** DX1 RX now decodes **every** signal across
  200-2900 Hz per slot (like FT1's Costas search) instead of a single carrier
  at the tuned RX offset; `rx_offset_hz` is demoted to a waterfall marker /
  TX-pairing hint. Three-stage scan: a coarse chirp-correlation carrier sweep
  (12.5 Hz grid, pre-folded replicas, trig-free hot loop) -> median-threshold
  peak-pick -> full CRC-14-gated decode per survivor. ~3-4 s/slot.
- **Transmit period (Tx 1st / Tx 2nd).** Choose whether you transmit on the even
  ("1st") or odd ("2nd") T/R slots — like WSJT-X's "Tx even/1st". A top-bar
  toggle + a Settings mirror; persisted. (Two stations must pick opposite
  periods to complete a QSO — previously TX was hardcoded to even, which is why
  QSO timing "felt off".)
- **Click-to-tune waterfall.** Click the waterfall to set your **RX** audio
  offset (green marker); shift-click sets **TX** (red marker), with a **Hold Tx**
  toggle to keep TX fixed. FT1 transmits at the chosen offset and hears the whole
  band; DX1 decodes at your tuned offset. The waterfall now marks **real** decoded
  signals at their audio frequencies.
- **Live clock-offset check (NTP).** Tempo periodically queries an NTP server and
  shows your real PC-clock-vs-UTC offset in the top bar (e.g. "clock +0.3 s"),
  warning when it drifts past the slot tolerance. On by default; fails silently
  off-grid and can be disabled in Settings.
- **Operator manual + visual launch surface.** A full operator manual in
  [docs/manual/](docs/manual/) (Getting Started, Operating Guide, Rig & Audio
  Setup, Frequency Plan, Tiers, Building, FAQ, Troubleshooting, Architecture,
  Roadmap), a screenshot-rich README with a hero banner and an animated demo
  GIF, a `CODE_OF_CONDUCT.md`, a `SUPPORT.md`, an on-air-report issue template,
  and enabled Discussions for on-air reports.

- **Tempo band plan + frequency controls.** Dedicated, US-General-legal and
  CW-clear calling frequencies across HF and VHF/UHF (USB weak-signal + FM
  simplex), placed clear of the FT8/FT4/JS8/WSPR/PSK watering holes and the FM
  national calling / APRS / satellite / repeater segments — see
  [docs/FREQUENCIES.md](docs/FREQUENCIES.md). New one-tap **band selector** and
  **manual frequency entry** in the top bar and Settings, retuning the rig live.
- **On-air operating controls** (from a WSJT-X gap audit): RX **input-level
  meter** + **Tx power** + **audio-device selection**; **Tune** (key a carrier),
  **Monitor** (RX-only) and **Stop TX**; DT-derived **time-sync health**; and a
  **Tx watchdog** auto-stop.
- **Windows cross-build validated.** All modem self-tests, `tempo.exe`, and the
  NSIS installer cross-build clean, and **5/5 Windows test exes pass** (FT1
  -15 dB, DX1 -18.6 dB, the 3-signal full-band scan, and FT1 acquisition +
  IR-HARQ `rv` through the C-ABI). Test exes now **statically link the gfortran
  runtime**, so they are self-contained.
- **Work a station + ADIF logbook.** Click a heard station (or a decode) to start
  a directed QSO with them; a persistent **ADIF logbook** (`log.adi`) that
  auto-logs completed QSOs and powers **worked-before (B4)** highlighting, with a
  manual Log-QSO form; inbound WSJT-X **Reply** (GridTracker/JTAlert
  double-click-to-call) now drives Tempo.
- **Live decode feed + alerts + comforts.** A color-coded WSJT-X-style decode
  list (CQ / directed-to-you / worked / new); **audio + visual alerts** on your
  call / CQ / new station; a **UTC clock** and great-circle **bearing**; and
  **editable quick-reply macros**.

### Changed

- **Starts passive (hunt-and-pounce).** Tempo no longer auto-calls CQ on startup;
  the presence beacon is an opt-in setting (default off), so the app listens and
  only transmits when the operator acts.

### Fixed

- **CAT now connects when you Save.** The radio loop read the rig/PTT config only
  once at startup, so choosing a rig in Settings did nothing until a full restart
  (and the VOX default never launched rigctld). It now applies rig/PTT/audio
  changes live — rebuilding the rig and launching rigctld the moment you save.
- **Test CAT.** New WSJT-X-style **Test CAT** button (Settings → Rig Control):
  opens the rig, reads its frequency, and reports green (with the frequency) or a
  specific error. A live rig/CAT status and an audio-device error are now shown
  in the app instead of failing silently to a hidden console.
- **Waterfall shows live receive audio.** The spectrum was computed from the
  decoder's once-per-slot frame (blank before the first decode, frozen during TX);
  it now reflects the continuously-captured sound-card input every cycle.
- **Tune** keys through the connected CAT rig (previously a VOX no-op on the
  startup snapshot) and auto-releases after 12 s as a safety.
- Installed app could fall back to the in-browser demo mock (fake stations / QSOs)
  if the Tauri backend wasn't detected; it now always uses the real engine.

## [0.1.0] - TBD

Initial pre-release. This is an **unreleased beta**: the protocol and tooling
are simulation-validated but have not been proven on the air, and the published
Windows binaries are cross-compiled. Treat this build as experimental.

### Added

- **Fast tier (FT1).** 4-CPM turbo modem with IR-HARQ, 4 s T/R, coherent.
  AWGN 50%-decode threshold of roughly -15 dB in simulation.
- **Robust tier (DX1).** Non-coherent 8-FSK with soft-decision LDPC(174,91),
  15 s T/R, fading-resilient. AWGN 50% near -18.6 dB with about a 3.7 dB fading
  penalty in simulation. Operator-visible tier toggle; the tier is never
  switched silently. Both tiers carry the same 77-bit messages, so all
  operating modes work on either.
- **Chat-first UI.** Vite + React + TypeScript desktop UI with three themes
  (Light, Dark, and night-vision-safe Amber-Night) and a modernized waterfall.
- **Operating modes.** Chat, QSO (run / monitor), and Field Day (run / S&P),
  driven by the headless-testable TX/RX engine in `tempo-app`.
- **Presence and messaging.** Passive roster built from decodes, free-text
  chunking and reassembly, a directed inbox, and presence-gated
  store-and-forward for off-grid nets.
- **Open broadcast and band feed.** To-all free-text broadcasts plus a band
  feed of decoded traffic.
- **Rig control.** PTT/CAT via Hamlib `rigctld` (launched by Tempo, default
  TCP `127.0.0.1:4532`), direct serial keying on the RTS or DTR line, or VOX
  for rigs without CAT.
- **WSJT-X UDP API.** WSJT-X-compatible UDP interface (magic `0xADBCCBDA`,
  schema 3, default `127.0.0.1:2237`; also listens for Reply / HaltTx /
  FreeText), with PSK Reporter spotting (outbound UDP to
  `report.pskreporter.info:4739`).
- **Windows installer.** NSIS `Tempo_0.1.0_x64-setup.exe` (per-user install)
  bundling the offline WebView2 runtime and Hamlib (`rigctld` + DLLs) so it
  installs clean and CAT works offline.
- **Build scripts.** Native Windows build (`scripts/build-windows.sh` for MSYS2
  UCRT64, with the `scripts/build-windows.ps1` PowerShell wrapper) and
  Linux/WSL2 cross-compile (`scripts/build-windows-cross.sh`), plus
  `scripts/fetch-hamlib.sh` to stage the bundled Hamlib.

### Known limitations

- On-air validation is pending; all performance figures above are from
  simulation only.
- The FT8/FT4 tier is Phase 2 — the internals are compiled in libtempo, but no
  decode pipeline is wired up yet.
- Published Windows binaries are cross-compiled and should be treated as beta.

[0.2.0]: https://github.com/kd9taw/nexus/releases
[0.1.0]: https://github.com/kd9taw/nexus/releases
