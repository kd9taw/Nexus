# Nexus 1.14.0

The headline is contest operating, in time for the CQ World-Wide RTTY DX Contest (0000Z Saturday
26 September). The RTTY cockpit has eight F-key macros you can edit, a double-click grabs a callsign
(and, in a contest, the exchange), and CQ WW RTTY and the Illinois QSO Party are both built in, with
the sponsors' own exchange, scoring and Cabrillo. Contest logging got the same attention:
duplicates are now kept and scored zero where the sponsor asks for that, serial numbers
actually go out and are tied to the station that copied them, and the already-worked warning
follows each contest's own rule instead of Field Day's. **72 changes since 1.13.0.**

If you don't contest, these are the changes most likely to matter to you. CW spots worked with the
soundcard keyer now land on the spot instead of 600 Hz off. A satellite contact you log after the
bird sets keeps its LoTW satellite credit. A two-radio station no longer hands the radio you're using
over to the default. And phone spots are back, because Nexus now picks working DX-cluster nodes for
you.

## The short version

- **Duplicates are kept in the contests that cross-check, and scored zero.** In CQ WW (including
  RTTY), CQ WPX, ARRL Sweepstakes and the three ARRL VHF contests, working someone twice now logs
  the contact and scores it nothing, instead of refusing it. That is what both sponsors ask for:
  CQ's own instruction is not to remove duplicates, because deleting one costs *the other station*
  its credit. The log table marks the duplicate — and no longer marks your first, real contact
  beside it. Field Day is unchanged, because ARRL does not check Field Day logs.
- **The "already worked" warning now asks each contest's own question.** It had been asking Field
  Day's — same call, same band, same mode — in every contest, and only the two Field Days work that
  way. In Sweepstakes you work a station once on any band, so someone worked on 40m CW showed as new
  on 20m phone and the log then refused you, after the over. In the QSO parties and on VHF, a mobile
  from a new county or a rover from a new grid is a fresh contact worth points, and the warning said
  "already worked", so you passed over a station you should have called.
- **Serial numbers are sent, logged, and belong to the station that copied them.** In CQ WPX, ARRL
  Sweepstakes and the California QSO Party, an `{EXCH}` macro used to leave the serial out entirely
  and every logged row recorded zero. The number now goes out, and it is tied to the station you are
  working from the moment you enter their call — so calling one station, working another, and coming
  back no longer sends one number to both.
- **RTTY for contests.** F1–F8 work from the keyboard, with a built-in Everyday set and a Contest
  set (CQ, exchange, TU, my call, his call, S&P exchange, AGN, B4), and every key can be edited.
  Each macro goes out on a line of its own. Double-click a call in the decoded text to grab it; in a
  contest, the zone, state or section is grabbed too.
- **CQ WW RTTY and the Illinois QSO Party**, both built from the sponsors' published rules:
  exchange, points, multipliers and the Cabrillo template. The contest strip also shows a faint CQ
  zone hint and warns before you log a US or Canadian station without its QTH.
- **CW spots land on the spot with the soundcard keyer.** Clicking a spot used to put your
  transmitted tone 600 Hz away from the station you clicked. And below 10 MHz, a scope click applied
  the pitch in the wrong direction.
- **Satellite contacts logged after the bird sets keep their satellite tag** (`PROP_MODE=SAT` and
  `SAT_NAME`), so LoTW gives satellite credit. This holds while the logged frequency is inside that
  bird's downlink, until you pick another transponder or restart Nexus.
- **Two radios:** a default radio no longer takes over the radio you're using, and switching radios
  never leaves the old one transmitting.
- **Phone spots are back, and should stay back.** Nexus keeps two working DX-cluster nodes connected,
  chosen from eight built into this release, and shows how each is doing. There is also an optional
  login SSID if you run two programs on one callsign.
- **Stations you've already worked step aside** on the POTA/SOTA board and the Spots panel. This is
  on by default, and one click shows everything again.
- **Nexus Remote:** the controls stay lit while a command confirms, the dial follows every notch of
  the wheel, and about a dozen reliability fixes land: Release, Listen, slow reads, busy stations,
  background tabs, log edits, and controls that blinked off after a clock correction.

## Before CQ WW RTTY

- **Update to 1.14.0 first.** CQ WW RTTY's exchange, scoring and Cabrillo exist only in this
  release — and so does keeping duplicates in the log, which is what CQ asks entrants to do.
- In **Settings › Contesting**, set your CQ zone. In the US or Canada, also set your contest state
  or province, or you'd send the DX exchange with no QTH. Nexus warns you if it's missing. The
  optional **Email for contest logs** goes into the Cabrillo header.
- Pick **CQ World-Wide RTTY DX Contest** under Settings › Contesting › Contest, and switch the RTTY
  dock to the **Contest** macro set.

## What was verified, and what was not

Every change went through the full local gate, and CI built and tested every commit on Linux,
Windows and macOS (compile check) and for the Raspberry Pi.

On the air, the 1.14.0 test build passed its on-air test list on 2026-09-19, including:
- RTTY F1–F8, macro editing and persistence, and a CQ WW RTTY practice session checked against the
  sponsor's Cabrillo example;
- soundcard-CW spot clicks on 20 m and 40 m, and a 40 m scope click;
- two-radio switching mid-over;
- the Needed board's park tagging;
- the Remote browser on a phone and a desktop.

Not run on real hardware for this release:
- **The macOS fixes** (the Prove TX and SSTV-on-145.800 confirmation dialogs) are covered by tests
  and the macOS compile check. No Mac test build was made, so they have not been tried on a Mac.
- The Remote fix for controls that blinked off after a clock correction is covered by tests.
  Its trigger was measured on a development machine whose clock steps. How often it happens in
  the field is not known.

## [1.14.0] — 2026-09-20

### Added

- **During a contest, the Operate cockpit now tells you whether you have worked a station *in this
  contest*.** The callsign card's `Dupe 20m` badge has always meant "worked on this band, ever",
  which is what you want day to day but not mid-contest — a contact from years ago lit it even
  though the station was a fresh one to work. A second badge now appears beside it while a contest
  is running: **Contest dupe** if this contest's own log already has them, and an amber **Club
  dupe** if another position at your club has. The lifetime badge is unchanged and still means what
  it always did, so the two can be read at a glance without either one changing under you.

- **RTTY: double-click a callsign to grab it, and F1–F8 you can edit.** Double-click a call in
  RTTY's Decoded text and it fills the Their call box and the log strip's callsign together —
  a portable or compound call such as VE3/K1ABC comes across whole, and a call garbled by a
  lost figures shift is filled as printed for you to correct. Anything that is not a call (599,
  CQ, TEST, a grid square) is ignored, and the caret stays where you were typing. The two call
  fields are now one: type in either and the other follows, and logging the contact clears
  both. The dock now has eight macro keys, and **F1–F8 work from the keyboard** while the RTTY
  cockpit is on screen. Two sets are built in and switched from the dock: **Everyday**, the
  four you had plus four empty keys, and **Contest** — CQ, exchange, TU, my call, his call,
  S&P exchange, AGN and B4. Hover a key and click ✎, or click an empty key, to change its title
  and message; it is saved straight away, and you can put one key or a whole set back to the
  built-in messages. Every macro now goes out on a line of its own and ends with a space, the
  way RTTY contest messages are written, so your call starts a line on the other station's
  screen. The Contest set's exchange keys send the exchange of the contest you are running —
  the zone and QTH, state or section you are sending this weekend, not last June's Field Day
  class — and with no contest running they tell you so instead of sending. **In a contest the
  double-click also fills the exchange**: a zone lands in the zone box, a section or state in
  the QTH box, by that contest's own rules rather than a fixed list, and **Enter** in one of
  those boxes logs the contact. A word that could be two things, or one Nexus has no list of
  values for, is left for you to type.

- **CQ World-Wide RTTY DX Contest.** Pick it under **Settings › Contesting › Contest** and the log
  strip takes its exchange: RST, your CQ zone and, for stations in the continental USA and Canada,
  a state or Canadian call area, using the sponsor's own codes (NWT, NF, LB and PEI among them).
  Contacts score the way the sponsor counts them: 3 points between continents, 2 within a
  continent, 1 within your own country, with zones, countries and W/VE QTHs each counted once per
  band. The strip tells you when the rig is on a band the contest does not use (it runs on 80, 40,
  20, 15 and 10 m) and still logs the contact. The Cabrillo export follows the sponsor's template:
  a two-digit zone, DX where a station sent no QTH, the LOCATION spelling the sponsor's list uses,
  and the category, claimed-score, name and email headers. The email comes from a new optional
  **Email for contest logs** setting; leave it blank and the line is left out. If your callsign is
  in the US or Canada but your contest state is blank or not on the sponsor's list, you would send
  the DX exchange with no QTH, so Nexus warns you in Settings, on the log strip and when the
  contest starts, and suggests the code a section means (EMA is MA). It never stops you, because
  operating from outside the US and Canada really is DX. Typing NT or PE is read as NWT or PEI.

- **Illinois QSO Party.** Pick it under **Settings › Contesting › Contest** and the workspace runs
  the Western Illinois ARC's own rules: 1700Z Sunday of the third full weekend of October for eight
  hours, 160 through 2 m without the WARC bands, phone 1 point and CW or digital 2. Illinois
  stations send RST and their county; everyone else sends RST and their state, province or
  **country** — this party asks a DX station for its country rather than the word DX. **The county
  box takes the county name as well as the code**: type `Cook` and it offers COOK, `st clair` offers
  SCLA, and space turns a full name into the code that goes in the log. A half-typed name is not
  completed for you and neither is one that could be several counties — pick from the list or
  finish typing, because nothing is ever guessed onto the air. **CW and digital count as one mode for dupes here**, so
  a station worked on CW shows as a dupe on RTTY on that band — the strip says so before you call
  them. **FT8 and FT4 earn no credit** (the sponsor's own rule); other digital modes are
  encouraged. Illinois entrants multiply by counties, states, provinces and up to five DXCC
  entities; everyone else by the Illinois counties worked. **The club's two calls, W9AWE and
  W9OAB, are worth 100 bonus points each** and Nexus adds them to your score and your claimed
  score as soon as they are in the log — there is no box to tick. The Cabrillo export writes
  `CONTEST: ILLINOIS QSO PARTY` and an `IL-COUNTY:` header for an Illinois entry, exactly as the
  sponsor's sample log does.

- **The contest strip suggests a CQ zone and flags a missing QTH.** Type a call in a contest that
  exchanges CQ zones and the zone box shows the zone the country file gives that call as a faint
  hint — it never fills the box, because a station outside its prefix's zone sends its own. And
  when a USA or Canada station is about to be logged without a QTH in a contest where they send
  one, the strip says so; you can still log it.

- **Nexus now picks working DX-cluster nodes for you, and shows how each node is doing.** Phone
  spots on the Needed board and in the Spots panel come from human-run DX-cluster nodes, and a
  node can stop working without warning: both nodes Nexus 1.13.0 shipped with did, on the same
  day. Nexus now keeps two nodes connected, chosen from eight nodes built into this release, each
  checked before it went in. A node that stops answering for five minutes while your other spot
  feeds keep working is skipped for a day and another node takes its place, and the connection
  log says which node and why. A quiet band never counts against a node: once a node has logged
  you in, it stays in use however long the band is silent. Operators are spread across the nodes
  by callsign, so everyone does not end up on the same two. **Settings › Logging & Connectors ›
  Integrations & Feeds › Spot Sources** shows each node as In use, Standby, or Not answering and
  why. If your install still had the node list Nexus shipped, it switches to this by itself. If
  you edited the list yourself, it stays exactly as you left it and nothing is switched for you:
  Spot Sources shows how each of your nodes is doing, and **Pick working nodes automatically** is
  there when you want it. A node you remove from your own list now disconnects when you save,
  instead of at the next restart.

- **Cluster login SSID — stop two stations on one callsign knocking each other off.** A cluster
  node allows one session per callsign and disconnects the older one, so running a second Nexus,
  or Nexus beside another cluster program on the same call, made the two bump each other
  indefinitely: a cluster connection that flapped with no explanation on screen. Under
  **Settings › Logging & Connectors › Integrations & Feeds › Spot Sources › Cluster login SSID**
  you can now give each one its own SSID (`2` logs in as `W9XYZ-2`), which the node treats as a
  separate user, and both stay connected.
  **Nexus does not choose one for you** — it is empty by default and logs in exactly as before,
  because a node that requires registration treats the suffixed call as a different, unregistered
  user and will not let it post spots, and because an SSID picked for you could collide with the
  one you already use in the other program. Your spots still reach the network under your plain
  callsign either way; nodes relaying them strip the suffix.

- **Stations you have already worked now step aside on the POTA/SOTA board and the Spots panel.**
  Both are **on by default**, and each says how many rows it is hiding.
  - **POTA/SOTA board — Hide worked today.** An activator you logged at their park since 0000Z
    (UTC) is hidden, so the board lists the activations you still need today. It comes back at
    0000Z, or as soon as that activator is spotted at a different park. Only a contact logged
    **with the park** counts: HUNT, double-clicking a park on the Connect map and Work on the
    Needed board all add it for you, but a contact logged without the park reference hides
    nothing.
  - **Spots panel — Hide worked.** A station you logged is hidden on every band, for the rest of
    the UTC day. The picker beside the chip changes that window to 1 hour, 4 hours, 24 hours or
    7 days — for 13 Colonies, Route 66 and other events where one callsign is on the air all
    week. A station you still need on the band and mode it was spotted on always stays.
  - **To see everything again,** click the chip — **Hide worked today · 3** on the board,
    **Hide worked · 12** on the Spots panel. Every row comes back marked **WORKED TODAY** or
    **worked 2h ago**. The board remembers your choice per window; the Spots panel until you
    close Nexus.

- **Remote: a responsiveness check you can run on your own link.** In the browser's Awards view,
  *Responsiveness → Run check* makes ten small dial steps, changes band and back, and sends one Stop
  while the radio is idle, then reports how long the screen took to respond, how long the radio took
  to confirm, how far the readout trailed, and how often controls went off by themselves — and
  whether each confirmation came by the station's own push or by polling — with a copy-as-text
  button so a result can be pasted into a report. It changes nothing while it is not
  running. It is the yardstick every later Remote responsiveness change is measured against; the
  same check runs in CI against a scripted station on 100 ms and 400 ms links.

### Changed

- **Duplicate contacts are now kept in the log for the contests that cross-check, and still
  refused in Field Day.** Work a station you have already worked in **CQ WW (CW, SSB or RTTY),
  CQ WPX (CW or SSB), ARRL Sweepstakes (CW or SSB) or the ARRL VHF contests (January, June and
  September)** and the contact is logged instead of turned away. It is marked as a duplicate, it
  is worth no points and no multiplier, and your QSO count and claimed score do not include it.
  It goes into your Cabrillo as an ordinary QSO line. This is what both sponsors ask for: their
  log checkers remove duplicates with no penalty, and CQ asks entrants not to delete them,
  because a contact missing from your log becomes a Not-In-Log penalty for the station that
  worked you — worth twice the contact at CQ.
  **ARRL and Winter Field Day are unchanged**: a duplicate there is still refused and nothing is
  written, because Field Day logs are not checked at all, so there is no penalty to spare anyone
  and its summary sheet asks for raw non-duplicate counts. The QSO parties are also unchanged —
  they are run by five different sponsors and we have not read a rule from any of them that
  settles it. The duplicate warning you get while typing a callsign is the same as it always
  was; what changed is only what happens if you log the contact anyway.

- **Large logbooks are much faster.** With a 150,000-contact log, Nexus had become slow enough to
  stop working: every status update re-scanned the whole log, and logging one contact sent the whole
  log (about 100 MB) to every window. Now an unchanged log costs the status update nothing, a logged
  contact sends just that contact to each window, each window keeps one copy of the log instead of
  three or four, and awards, needs and Journey are counted once per change to the log instead of on
  every refresh. Reading the log no longer holds up the radio while it converts, and a contact
  WSJT-X logs in companion mode is added to the end of the log instead of rewriting the whole file.
- **Contest Cabrillo logs record the frequency you were on.** Outside Field Day, each QSO line now
  carries the dial the contact was logged on instead of the band's lower edge, which sponsors
  such as CQ WW ask award entrants for. Contacts logged before this update keep the band edge.
  Field Day logs are unchanged.

- **Remote: the dial follows the wheel, and it stops swallowing your corrections.** Spinning the
  readout digits over a remote link used to ignore every notch made while a command was still out —
  on a 100 ms link four of ten notches reached the radio, on a 400 ms link three — and the digits
  themselves did not move until the station read the new frequency back. Every notch now counts: a
  step made while the radio is still working joins the next command and is sent from the dial the
  station read back, so ten notches move the dial ten notches. The digits move on the gesture
  itself rather than when the radio answers, shown dimmed with a trailing "…" until the station's
  own reading confirms them. A value the station never confirms is never left standing — after two
  and a half seconds the digits go back to the station's dial and say the tune was not confirmed.
  Only the dial digits are shown ahead of the radio; the band, the mode, the sideband, the
  privilege shading, the S-meter and every transmit control keep reading the station and nothing
  else.

- **Remote: a tune, band or mode change confirms the moment the radio does it.** Until now every
  rig-touching control was answered "pending" and the browser found out it had landed by asking —
  a state read, then a result read, each a round trip through the relay, with a one-second wait
  between them — so a control felt half a second to two seconds slow, and the tuning wheel ignored
  input for all of it. The station now tells the browser the outcome itself (operation protocol
  v5), together with the fresh control state, the instant its radio loop reads the new dial back;
  the browser installs both and is ready for the next gesture with nothing in between. Polling
  stays as the safety net, so a pushed message that is lost costs the old delay and never the
  outcome. A desktop or site on the previous version keeps polling exactly as before; the push
  needs both updated.

- **Remote: the station controls stop blinking off after every command, and a call pressed in that
  moment is held instead of refused.** Every held control used to go grey for about a second after
  each command while the browser re-read the station, and a call, a Resend or an exchange pressed
  while the browser was still re-reading came back "Not sent. Nothing reached the station" — most
  often right after turning TX on. The controls now stay lit throughout, and once the station has
  answered, a gesture made while the browser is still re-reading waits for the fresh state and goes
  out on it, a second and a half at the outside. It is never sent late or behind your back: if the
  re-read does not come, or you press Stop or turn TX off while it waits, or the station's transmit
  permission moves under it, the gesture is refused and says so, with nothing reaching the radio.
  While a command is still on its way to the station a second one is refused the same way, "Not
  sent", with nothing reaching the radio — where before, the control simply went dead until the
  first had landed.

### Fixed

- **Your CW and RTTY macro keys now send a serial number in the contests that use one — the text
  they transmit has changed.** In CQ WPX (CW and SSB), ARRL November Sweepstakes (CW and SSB) and
  the California QSO Party, an `{EXCH}` macro used to leave the serial out entirely: an F-key set to
  `{CALL} 599 {EXCH} {EXCH}` keyed `K1ABC 599` and stopped there, and Sweepstakes sent
  `A W9XYZ 74 WI` with no number in front. The same keys now send `K1ABC 599 1 1` and
  `1 A W9XYZ 74 WI`. The serial is counted for you: the log strip shows the number for the contact
  you are working, and the contact is logged and exported with the number that went out. Until now
  there was no number anywhere, and every contact was logged and exported as serial `0`, which is a
  log a sponsor cannot check.
- **The log strip and the phone cockpit show the number you are actually sending.** In a serial
  contest they read the exchange straight off the session, whose serial slot is a permanent
  placeholder, so the "Sent:" line said `599 0` while the contact was logged with the real
  number. On SSB that is what you read aloud — and it is what the voice-keyer hint told you to
  record into a slot — so your log would have recorded serials that never went on the air.
  Both now show the issued number. Contests with no serial are unchanged.
- **The number belongs to the station you are working, from the moment you enter their call.**
  Search and pounce: call one station, get no answer, work somebody else and come back — the first
  station is given the number they already copied, not a later one, and the number you actually
  sent is the number that reaches your log. Correcting a busted call keeps the number too, so
  fixing `K1ABC` to `K1ABD` does not hand them a second one. Reading the strip, previewing an
  F-key, or sending your exchange twice never moves the run; logging the contact does, so the next
  station gets the next number. Contests with no serial in their exchange, Field Day among them,
  are unchanged.

- **The contest dupe warning now follows the contest's own rule instead of Field Day's, and it was
  wrong in both directions.** While you type a callsign, the log strip and the Operate callsign
  card tell you whether this contest has already worked that station. That check asked Field Day's
  question — same call, same band, same mode class — whatever contest was running, and **only the
  two Field Days actually work that way**. The other fifteen were wrong, seven of them one way and
  eight the other. **In Sweepstakes you work a station once, on any band and any mode**, so someone
  you had already worked on 40m CW showed as new on 20m phone and you called them and were refused
  at the log, after the over. **CQ WW and CQ WPX count one contact per band in either mode**, so
  the same thing happened across a mode change. The other eight went wrong the opposite way: in
  **the five QSO parties and the three ARRL VHF contests**, working a mobile again from a new
  county — or a rover from a new grid — is a fresh contact worth points, and the warning said
  "already worked", so you passed over a station you should have called. Where a contest's rule
  depends on an exchange you have not copied yet, the warning now says nothing rather than
  guessing: being told nothing costs you a duplicate that scores zero, while a wrong "already
  worked" costs you the contact.

- **A duplicate in the contest log table is marked as the duplicate — and the real contact beside
  it no longer is.** Now that a duplicate is kept in the log rather than refused (see above), the
  table has rows that are worth nothing, and you need to see which when you check your score
  against the sponsor's. It had been working its own dupes out by looking for a callsign that
  turned up twice on the same band and mode, which went wrong two ways: it marked **both** rows,
  so your first, real, scoring contact was flagged as a duplicate for the crime of being worked
  again later; and it used Field Day's rule everywhere, so a Sweepstakes duplicate on another band
  was not marked at all, while two perfectly legal QSO-party contacts with a mobile in two
  different counties were both marked. The table now shows what the log itself recorded.

- **Remote: the Awards list no longer gets squeezed off a phone screen.** In the hosted browser
  the Awards view carries a status line above the summary and, since the responsiveness check was
  added, a strip below it. On a phone-sized window with the text enlarged there was no height left
  for the summary between the two: it collapsed to nothing, and the strip drew over the award
  cards, so a tap landed on the strip instead of the card under it. The summary now keeps a
  minimum height of its own whatever the window does — a card scroller worth using — and anything
  that will not fit scrolls with the column instead of being cut off at the bottom. Windows at
  1024×768 and above look exactly as they did.

- **A Field Day exchange copied by the RTTY sequencer now reaches the log as fields, not just as a
  note.** If Auto was running in Field Day and you switched Field Day off while a contact was still
  on the air, that contact finished under the Field Day exchange but no longer had a contest log to
  go to. Its class and section landed in the record's comment as plain text — `2A EMA` — and
  nowhere else, so they were not in the ADIF you exported and no other program could read them back
  as an exchange. They are now written to their proper ADIF fields as well, and the comment still
  reads the same as before. Contacts made the ordinary way were never affected.

- **Editing a contest contact no longer wipes its exchange.** Correcting anything on a contest QSO —
  a busted call, a report, a grid — silently dropped the contest data from the record: the contest
  name, the serials you sent and received, and both exchanges, none of which the edit form shows you
  in the first place. The contact stayed in your log, but its contest fields were gone from the log
  and from every ADIF export made afterwards, so a corrected contact would not score. Editing now
  keeps all of it, including on a callsign correction — the exchange is what went over the air, and
  fixing the call does not change what was sent.
- **The CW and Phone band-activity strips no longer rebuild themselves every time the spots refresh.** On a busy
  band — a contest evening can put over 600 stations on the 20 m CW strip — one new spot made Nexus throw away and
  redraw every flag above it, which on a slower PC was enough to stall the waterfall for a moment before the new flag
  appeared. The flags of stations still on the air stay put now; only the spots that arrived or aged out change.
  (#320)
- **Prompt before logging now queues contacts instead of replacing them.** If a second contact finished while the
  popup was still open, the popup kept showing the first station while Nexus had moved on to the second — and logging
  then filed the first station's call on the second contact's time and frequency. The popup now keeps the contact it
  is showing, tells you how many are waiting, and brings up the next one as soon as you log or discard it. Nothing is
  mixed, and no contact is lost.
- **If the queue ever fills, Nexus says so rather than logging quietly.** Leaving an FT8 run to itself with
  Prompt before logging on can stack up contacts; past 64 waiting, the oldest is logged as it stands rather than
  lost. That contact is now named in the Connections log with its call, band, mode and time, and the popup says how
  many went in without your confirmation since you last answered it — because those contacts upload to your
  services and join the LoTW batch like any other.
- **Correcting a callsign in that popup corrects what was looked up from the wrong one.** The country, state and a
  name that came from the busted call are re-derived for the call you actually worked, and a grid that had only been
  looked up is dropped — a grid the station itself sent, or one you typed, is kept. Correcting a call from Nexus
  Remote now does the same. An edited report no longer leaves the old report behind in the comment.
- **The ATU button says when your radio's tuner cannot be started over CAT.** On Icom and Kenwood radios connected
  through Hamlib the command only switches the tuner in or out, so the button is greyed out and points at the TUNER
  button on the radio — instead of quietly switching the tuner in, tuning nothing, and, in the digital section,
  ending your QSO for a tune-up that never happened. Where a tune-up can be started, it now ends your QSO only once
  the radio has actually accepted it. (#322)
- **Dates you type are UTC everywhere.** The Logbook's edit form, "Log a contact from another radio" and the export
  date range took dates through the Windows date control, which follows the PC's own calendar and silently discarded
  a date it could not read — so an impossible date looked like a cleared one, and on the export that quietly widened
  the range to your whole log. All three now take the date as plain UTC text, refuse a date that cannot be, and hold
  the button rather than exporting something other than what you asked for. (#280)
- **The waterfall no longer freezes a few minutes into a session.** While the radio was slow to
  answer (a CAT read on a slow link, a log save), each Nexus window kept asking for its next update
  several times a second without waiting for the last one, and those waiting requests could take
  every thread the waterfall and meters are drawn from. Each screen now waits for its answer before
  asking again, and the waiting happens where it cannot hold up the waterfall. (#335)
- **Contact times in the Logbook's edit form and in "Log a contact from another radio" are always
  24-hour UTC.** Those time boxes followed the Windows clock format, so on a PC set to a 12-hour
  clock a contact at 00:58 UTC showed as "12:58 AM", and changing it to 00 put it straight back to
  12. Contacts that were right got "corrected" by 12 hours. You now type the time as HH:MM or
  HH:MM:SS. A time that can't be right, such as 25:00, is refused with a message rather than saved
  or replaced with the current time. An edit that doesn't touch the time now keeps it to the
  second. The times Nexus stored were always correct. (#280)
- **Confirming a contact with Prompt before logging keeps its end time** and, on a split contact,
  its receive frequency. Ham Radio Deluxe no longer shows 00:00 as the end time. (#329)
- **Digital section: running the radio's ATU now ends like Tune.** TX switches off, and Nexus no
  longer goes back to calling the station of an unfinished QSO. (#322)
- **Uploads are never dropped silently.** When the upload queue fills, the Connections log names
  each dropped contact and the service it never reached, and a ClubLog catch-up can no longer push
  out contacts you just made. Every upload service now says so in the Connections log when it gives
  up on a contact after its retries — ClubLog, World Radio League, N3FJP and Cloudlog/Wavelog as
  well as QRZ and eQSL — naming the contact and how to send it again. (#290)
- **The band menu outlines the band you are on.** It was a thin bar on the left edge that looked
  like a "(". (#323)
- **With two radios, switching back no longer shows a band as "custom".** A radio running FT8
  reports its data mode, and the band menu didn't recognise that as its FT8 channel, so after a
  switch 80 m read "80m (custom)" although the radio hadn't moved. (#334)
- **Remote: revoking a browser's logging or station-control permission always takes effect**, even
  while the station is busy. (#318)
- **Remote: the Release button no longer flickers.** It greyed out on every heartbeat — about once a
  second — so a click could land while it was disabled and do nothing. It now stays lit, and a Release
  clicked while the station is answering waits for that answer and then releases the station, once.

- **Two radios: a default radio no longer takes over the one you are using.** With a "default
  radio" set for everything else, changing frequency on your active radio could hand control to the
  default one — even on a band the active radio covers. Tuning, a spot click, or another program
  setting the frequency through Nexus's CAT sharing (WSJT-X or JTDX split, for example) could all do
  it, so a radio could switch away mid-session. The default radio now only takes bands the active
  radio does not cover.

- **Two radios: switching radios never leaves the old one transmitting.** When you switch radios
  while one is transmitting, Nexus unkeys it first. If that unkey did not go through, the switch
  went ahead anyway and left the old radio keyed in the background, where nothing could unkey it.
  The switch now waits until the old radio has really stopped transmitting, and keeps trying to
  unkey it until it does. Nexus also now writes to its log which audio device it opened after a
  switch, or why it could not, so a radio that loses its audio can be diagnosed.

- **CW with the soundcard keyer: clicking a spot now puts you on the spot.** The soundcard keyer
  sends CW as an audio tone in the radio's data mode, where the tone goes out a pitch above the dial
  (below it on 80 and 40 m). Clicking a spot tuned the dial straight to the spot, so you transmitted
  about 600 Hz off the station you clicked, and heard them at a pitch too low to hear or decode. The
  dial now sits a pitch away from the spot so your signal lands on it, from every place a spot can be
  clicked, and a pile-up split moves with it. On 80 and 40 m a click on the CW scope also went the
  wrong way by twice the pitch; it now follows the side the radio is really on. The CAT, WinKeyer and
  serial keyers were not affected: they put the radio in CW, which is already on the spot.

- **A satellite contact you log after the bird sets now keeps its satellite tag.** The guide says you
  can log a contact once your hands are free, but a contact logged after the pass ended — or after
  choosing "None" or stopping the track — lost `PROP_MODE` and `SAT_NAME`, so it earned no LoTW
  satellite credit and a 2 m contact counted toward terrestrial VUCC instead. Nexus now remembers the
  last satellite you worked until you pick another transponder or restart, and still tags a contact
  only when its frequency is on that satellite's downlink, so an ordinary contact is never tagged.

- **Icom: a refused scope span now tells you why, instead of guessing.** When an Icom refused a
  span change, Nexus always said the scope was in Fixed mode and told you to switch it to Center —
  even when it was already in Center, so you were sent to check a setting that was already right.
  It now asks the radio. If the scope really is in Fixed mode it says so; if it is in Center it
  says that plainly, and suggests the other likely cause: a span change while the radio is
  transmitting or tuning. Nothing about how Nexus sets the span has changed.

- **macOS: Prove TX now works, and SSTV can be sent on 145.800.** Two confirmation prompts asked
  their question through the browser's own `confirm()` box, which this app's macOS webview does
  not draw. No dialog appeared and the answer came back as "no", so on macOS **Prove TX** did
  nothing at all — the button looked live and never keyed the radio — and **SSTV refused to
  transmit on 145.800 MHz**, the frequency that asks you to confirm first because it is the ISS
  downlink. Both now ask in the app's own dialog, with the same question and the same warning.
  Nothing was ever transmitted without asking: the prompts failed in the safe direction, so what
  was lost was the feature, not the protection. Windows and Linux were unaffected.

- **Work on the Needed board now tags the park.** Working a POTA or SOTA row from the Needed board
  moved the radio but, unlike HUNT and the map, never told the logbook which park it was — so the
  contact was logged without the park reference: it earned no hunter credit, exported without
  `SIG`/`SIG_INFO`, and pota.app could not match it. The next contact with that activator is now
  tagged with the park, exactly as HUNT does it, and a reference Nexus cannot read is reported
  instead of dropped. A row with more than one activation live — a summit that is also a park,
  spotted to both programmes — tags nothing and names them, **2 activations live (POTA US-0001,
  SOTA W7A/MN-001)**, so you can pick the one you worked on the POTA/SOTA board; a SOTA spot from
  hours ago does not count as live.

- **A contact logged at two parks at once counts for both of them.** A two-fer — one QSO at a
  site where two park boundaries overlap, logged as `US-0001,US-0002` — matched neither park, so
  both kept their NEW PARK badge and the Needed board went on offering both activations after you
  had worked them.

- **CW macros no longer call "CQ FD" in another contest.** The cockpit had two built-in macro sets,
  casual and Field Day, and used the Field Day one in every contest — so `F1` in CQ WW CW or a
  state QSO party called `CQ FD DE …`, Field Day's own call, on the air. There is now a contest set
  with the same cadence and `CQ TEST`; Field Day keeps `CQ FD`, and a macro profile of your own
  still wins over both.
- **A mistyped Sweepstakes section no longer counts as a multiplier.** A received section that is
  not on the list counted as a multiplier of its own. The contact is still logged; the section just
  counts for nothing.
- **Remote shows the contest screen for every contest.** Through Remote, the contest screen was
  blank for any contest except ARRL Field Day and Winter Field Day, because the browser refused the
  contest's rules as unknown. It now shows for all of them — CQ WW, Sweepstakes and the state QSO
  parties included.
- **A contest other than Field Day comes back after a restart.** If you left a QSO party or
  another contest running and restarted Nexus, it came back in Chat unless a Field Day class and
  section happened to be filled in, and the contacts you had logged stayed out of view. Nexus now
  reopens the contest you left running. A contest whose own exchange is incomplete still stays
  closed, and entering it tells you what to fill in.
- **`{EXCH}` in a CW macro sends the exchange of the contest you are running.** It always sent your
  Field Day class and section, so in any other contest a macro with `{EXCH}` sent the wrong
  exchange, or nothing at all. It now sends that contest's exchange without the signal report —
  your zone in CQ WW CW, for example. Field Day sends exactly what it did before. `{CLASS}` and
  `{SECTION}` are empty outside Field Day.
- **RTTY Auto no longer sends a Field Day exchange in other contests.** The auto-sequencer only
  knows the Field Day exchange, yet it turned on in any contest and would have sent your Field
  Day class and section to every station it worked. In any contest other than ARRL Field Day or
  Winter Field Day it now refuses to turn on and says why: send your exchange with the macros and
  log each contact yourself. Outside a contest, and in both Field Days, Auto works as before.
- **The contest log strip works for contests other than Field Day.** In a contest whose exchange
  includes a signal report, the report box came back blank after every contact; it now goes back to
  599 (59 on phone). A CQ zone has to be a number from 1 to 40, and the strip says so while you
  type. The strip's label, hint and button now say "contest log" instead of Field Day, and the zone
  box is captioned Zone. Field Day's strip is unchanged.
- **The contest screen names the contest you are running.** Outside Field Day the banner still
  read "ARRL Field Day", the header showed an empty class and section, the log table's columns were
  Class and Section (blank on every row), the Score Summary printed a station class, a power
  multiplier and a bonus list, and the Field Day bonus checklist sat under the score. The banner
  and header now name the contest and show what you are sending, the log table has one column
  per field that contest exchanges, the Score Summary shows QSO points × multipliers, and the
  bonus checklist appears only in Field Day. Field Day's screen is unchanged.
- **A cluster node that never asks for your callsign no longer kills that feed slot for the
  rest of the session.** Some nodes accept the connection and never send a login prompt — one
  of the nodes Nexus ships with was doing exactly that — and some send a prompt Nexus could not
  read (next entry). Nexus waited on them forever: it never logged in, never gave up, and never
  retried, so that node stayed dead until you restarted the app, and nothing on screen said why.
  Nexus now gives a node 30 seconds to ask for your callsign, then disconnects and tries again
  later, waiting longer after each try, up to ten minutes, so a node that never answers is not
  called over and over. **Settings › Connections** records which it was: the node said nothing at
  all, or it spoke and never prompted. Once a node has asked for your callsign and Nexus has
  answered, it is never dropped for being quiet afterwards — a cluster on a dead band can be
  silent for a long time and that is not a fault.
- **Nexus now logs in to nodes whose prompt ends with a new line.** A node that greets with
  "Please enter your call:" and then a line ending — AR-Cluster nodes do, and `k1ttt.net:7373`
  was measured doing it — was never answered, because Nexus replied only to a prompt left
  waiting at the end of what it had received. It now answers a prompt that is the last thing the
  node sent, in either shape. A prompt-looking line with more text behind it is still read as
  part of the greeting, so your callsign is never sent into a welcome message.
- **A cluster node that was working could be reported as dead.** Nexus asked the system for the
  node's address and then only ever tried the first one it was handed. Many nodes have both an
  IPv6 and an IPv4 address, and on a connection with no working IPv6 route the IPv6 one is
  commonly offered first — so the attempt failed and the IPv4 address sitting right behind it,
  which would have connected, was never tried. The node read as down while it was answering
  perfectly well, and no phone spots arrived from it. Nexus now tries every address the node
  has, in turn, and only reports it unreachable once they have all failed. When one does fail,
  **Settings › Connections** now says so and names what was tried, so "this node is down" can
  be told from "this connection cannot reach it over IPv6".
- **The Phone source could name a cluster node that was down.** The Now-Bar's Phone pill and the
  Phone source line on the Needed board named the first node Nexus had started, whether or not
  it was connected, so "live" could sit beside the name of a dead node. They now name only the
  nodes you are logged in to, or, while none is, the nodes being tried.
- **Remote: the bottom of the Awards view is no longer cut off on a phone screen.** On a
  phone-sized window with the text enlarged, the last part of the hosted Awards view was cut off
  with no way to reach it; anything that does not fit now scrolls with the column instead. Windows
  at 1024×768 and above look exactly as they did.
- **SSB/phone spots stopped arriving, and the Spots panel showed no Phone chip.** Both of the human
  DX-cluster nodes Nexus shipped with, `ve7cc.net:23` and `dxc.wa9pie.net:8000`, went down on the
  same day, and every install that still had the default list had no phone source left — RBN kept
  CW and the digital modes flowing, which is why only Phone (and the mode chip the Spots panel
  builds from what it is actually receiving) vanished. Two more nodes are now in the default list —
  `dx.w1nr.net:23` (DXSpider) and `dxspots.com:7300` (CC Cluster, on a high port for networks that
  block telnet port 23) — and an install still on the nodes Nexus shipped now picks working nodes
  for itself on the next launch (see the DX-cluster entry under Added); a list you edited yourself
  is left alone. Until you upgrade, add either one under **Settings › Logging & Connectors ›
  Integrations & Feeds › Spot Sources**: `dx.w1nr.net:23` is in the **+ Add a known node…**
  presets, and `dxspots.com:7300` goes in with **+ Custom**. Phone spots return at once.
- **The Listen button in the Remote browser did nothing.** Opening the workspace replaced the
  connecting page with the operating one, and that swap permanently closed the audio channel
  before the button you could see was ever wired to it — so it looked enabled and sent nothing,
  every session. It now hands the channel back instead of closing it, and Listen works. If you
  were already listening while the page was still connecting, audio stops once at that moment
  and you press Listen again.
- **One slow read no longer ends a Remote session.** A background read that took longer than
  three seconds — the needed board, a rare-DX lookup, a parks fetch — tore down the whole
  connection. Every control then greyed out, commands were refused, and audio dropped. Those
  reads now fail on their own and the session carries on.
- **A busy station no longer costs you control.** A momentary refusal while the radio was busy
  read as "control was lost", dropping audio with it. A busy answer to a routine check now
  keeps your control and your audio.
- **Remote no longer turns itself off permanently after a hiccup.** A single message the station
  could not read switched Remote off and remembered it off, so the only fix was to walk to the
  radio and turn it back on — which is the one thing you cannot do from somewhere else. The
  station now reconnects and retries; only a genuine refusal of access turns Remote off.
- **A slow moment at the station no longer stops the instrument feed for good.** A late credit
  on the live feed dropped the station's whole connection, taking every browser with it. The
  feed now waits for the credit and picks up where it left off.
- **Remote: the amplifier, DSP and decode-depth controls no longer blink off when a clock is
  corrected.** When a wall clock stepped at either end, the next station reading looked as though
  it had arrived before it was sent, and the browser threw away the good reading it already held
  along with it, so every control that depends on the radio's live readings (the amplifier
  buttons, the Phone DSP buttons, the FT decode-depth chips, the RX offset box) went blank until
  the next one came in. The odd reading is still never shown and the clock is still re-synced,
  but the reading already held now stays until it is three seconds old. A reading that arrives
  late still blanks them at once, because a late reading means the link has stalled.
- **A delete or edit from a Remote browser could make the shack's Logbook delete or overwrite the
  wrong contact.** The shack's list loaded once and remembered each row by its position; when the
  browser deleted a contact above it, every later row moved up one, and the next Delete or Edit at
  the shack went to the contact now sitting at that position — deleted, or rewritten with another
  contact's details — while the message on screen named the one you meant. The shack now identifies
  a contact by what it is, as the browser already did, not where it sits: an action from the shack
  is refused with a "reload the log" message if that exact contact is no longer there, and the list
  refreshes itself whenever the log changes, so that refusal is rare.
- **Editing a WWFF contact from a Remote browser no longer turns its park into a POTA one.** The
  hosted edit form only knew POTA and SOTA, so saving any correction — a grid, a name — rewrote
  the park's program to POTA. The program now rides through exactly as the log holds it, as it
  already did at the shack.
- **The Remote edit form no longer offers four fields it could not save.** My grid, Rig, QSL sent
  and Card received were shown in the hosted form, took your entry, and dropped it under an
  "updated" message. They are shack-only for now; QSL sent and Card received are still on the
  hosted row menu, where they do save.
- **A Remote tab left in the background now lets the station rest, and there is a switch for when
  you don't want that.** A Remote browser tab sitting behind your other windows kept the whole
  station session running, hour after hour, with nobody looking at it. Sending the tab to the
  background now pauses the feed the same way it already stops the audio and hands back station
  control, and bringing the tab back to the front picks it up again straight away. If you are
  watching a frequency on a second screen, **Keep watching** — next to Listen in the Remote
  header, or in that header's settings panel on a narrow screen — keeps it running; the choice is
  remembered for that station. The feed never pauses while your browser can still stop a
  transmission, and coming back from a pause says so.
- **A Remote browser that lost its station stopped hammering at it once a second.** When the
  connection dropped after the service had already answered, the browser retried exactly once every
  second, forever — a station that was off, or a shack PC mid-reboot, was called thousands of times
  an hour for nothing. It now waits longer between tries the longer the trouble lasts, up to half a
  minute, and goes straight back to trying the moment you look at the tab. That wait only builds up
  while the station has really gone quiet: one that is still sending you data reconnects straight
  away. This matters because the browser itself sometimes drops the link after a slow reply, and a
  station that was answering the whole time should not be the one made to wait for it.
- **A hunt that could not be set now says so** — on the Connect map, in the main window and in a
  torn-off one. When the feed's spelling of a reference was one Nexus could not read, Work went
  ahead with the QSY in silence and the contact was logged with no park at all — the only sign was
  its absence, hours later. It now tells you, and still works the station so you can add the
  reference by hand.
