# Nexus 1.15.0

The headline is the logbook. It now lives in a database, so a big log no longer slows the station
down: a change writes only the contacts it touched, the Logbook opens in a quarter of a second on
150,000 contacts, and, measured on logs of 150,000 and 500,000 contacts, nothing the logbook does
holds up the radio for more than 5 ms. The first start converts your log once, keeps your file as
it was, and goes on keeping `log.adi` up to date for your other programs and backups. The second
headline is the Sub receiver of the IC-7610 and IC-9700: run through Nexus's own CI-V connection,
it gets its own row on the Phone and CW screens, with sliders that change only the Sub. Satellite
operators can also find the times when they and another station can both see a bird, and FT8
goes up an inverting transponder on the right sideband.

If your log is small and your radio has one receiver, these are the changes most likely to matter
to you. The Phone cockpit is laid out as a receiver and a transmitter, with AF gain, RF gain,
squelch, the attenuator, the preamp and an S-meter. The transmit lock checks more of what actually
goes out: the sideband you picked in Phone, a repeater's input, RTTY on the upper sideband and the
soundcard CW tone. Switching radios keeps each radio's keying line and asks the radio you leave
to stop a voice memory it may be playing. And the private note on a contact is no longer sent to
QRZ, Club Log and the other logbook services.

**Operating CQ WW RTTY this weekend?** Finish the contest before you update: the first start
converts your logbook.

## The short version

- **Your logbook is a database.** It is kept in `log.sqlite3` beside `log.adi`, and a change
  writes only the contacts it touched. The first start converts your log once (several seconds
  for a lifetime log) and keeps the file exactly as it was, as `log.adi.pre-sqlite`. `log.adi`
  keeps up with every change, so other loggers, backup scripts and sync tools still see every
  contact.
- **Big logs stop holding up the radio.** Each window asks for the contacts it shows instead of
  holding its own copy of the log, and exports, the start of a LoTW upload, Awards, the Needed
  board and the statistics read the database while the radio carries on.
- **Main and Sub on the IC-7610 and IC-9700**, through Nexus's own CI-V: a SUB row shows the
  Sub's frequency where Nexus knows it and has its own RF gain, plus AF gain and squelch on the
  IC-7610. On the IC-7610 the S-meter, the receive controls, the frequency and the mode stay on
  Main whichever band the radio has selected.
- **Satellites:** "Sked with a station" finds the windows when a bird is up for you and another
  station at once. FT8 and the other data modes go up an inverting transponder on the mirrored
  sideband, QO-100's narrowband transponder is worked in SSB rather than FM, KOSEN-1's uplink is
  no longer put in FM, and a satellite tag can be fixed or removed from the Logbook.
- **The Phone cockpit** has Receiver and Transmitter panes, AF gain, RF gain, squelch, an S-meter,
  the attenuator and preamp steps your radio has, the transmit monitor level, and a line that
  shows what goes out when you key.
- **The transmit lock checks more of what goes out**: the sideband picked in Phone, a repeater's
  input, RTTY on the upper sideband, the soundcard CW tone, satellite uplinks and the CW ID after
  an FT 73. XIT is no longer offered on the IC-9700 and 56 other radios that have none, or
  through OmniRig.
- **Switching radios:** each radio keeps its keying line, a switch can no longer leave the radio
  you switched from transmitting, requests meant for the new radio no longer reach the old one,
  and the radio you leave is asked to stop a voice memory it may be playing.
- **Contests:** the RTTY cockpit shows what you are sending (#379), and a contest merged into
  your logbook keeps the state or province, both signal reports and the frequency of each
  contact.
- **The Logbook window** works from the keyboard, shows Date and Time as two columns (#239),
  opens a contact's full record on a double-click (#313), and shows and edits a contact's end
  time.
- **Your private note stays private.** It is no longer uploaded with your contacts.
- **Also:** a WATCH tile for stations on your watch list, alerts limited to the continents or
  countries you pick (#174), editable F-key macros in PSK (#316), and a timer for RTTY Auto call
  (#304).

## Your logbook

**The database.** Every change used to rewrite the whole of `log.adi`: every upload stamp, every
confirmation, every edit. On a big log that rewrite made the radio and the screen hesitate each
time a contact was saved or an upload was marked. The log is now kept in `log.sqlite3` beside
`log.adi`, and a change writes only the contacts it touched.

- **The first start converts your log.** A lifetime log can take several seconds, once, and the
  start-up screen says so while it works. Your file is kept exactly as it was, as
  `log.adi.pre-sqlite`. Starting Nexus again while it converts does not open a second copy.
- **`log.adi` stays where it always was** and keeps up with every change a moment later. Nexus
  no longer rewrites it at start-up, and quitting waits for the last change to reach the disk.
- **Two things behave differently.** If something other than Nexus writes to `log.adi` (an older
  Nexus on the same folder, a restored backup), Nexus takes that file's contacts the way the
  Logbook's Import does: new contacts are added and confirmations come across, but a contact
  changed or removed only in that file is not changed or removed in your log. And if Nexus stops
  partway through importing a large file, the contacts it had taken in are kept; importing the
  file again adds the rest without duplicating any.
- **Restoring a backup** takes more than dropping a copy over `log.adi` now. The install guide
  has the steps, under "Restoring the logbook from a backup copy". The dated backups in the
  `backups/` folder keep up to four copies of your log again, however big it is.
- **Network drives.** A new data folder on a Windows share or an NFS or SMB mount is refused,
  with the reason and what to do instead, because file locking across a network is the classic
  way to corrupt a database. A folder already on a network drive is never moved for you: it keeps
  the log in `log.adi` as before, and Nexus says so. A folder that looks like Dropbox, OneDrive,
  iCloud or Google Drive is flagged rather than refused. One Nexus at a time on a logbook still
  applies there.
- **Two Nexus windows on one data folder** see each other's corrections and deletions: a contact
  corrected in one is corrected in the other, and a contact deleted in one stays deleted.
- **When the logbook is too busy to take a change** (a big import, another window, a sync), Nexus
  tries a few times, then changes nothing and says the logbook was busy, so doing it again
  retries. **Mark on LoTW** works a few thousand contacts at a time and can be finished by marking
  again.
- **Closing Nexus while changes are still on their way to the disk** keeps the window open with
  **Saving your logbook…** and the number of changes left. If the disk has not taken them after a
  minute, or refuses one, Nexus asks: **Keep trying**, or **Quit without the last N changes**.
- **A change that cannot be saved** because the disk is full or failing, or another program holds
  the logbook, is kept in memory and sent again, with a message on screen until everything is
  saved.
- **Moving your data folder** takes the logbook's safety copies and the database with it, all
  checked before the new folder is used.
- **Two computers sharing one `log.adi` on a network drive:** Nexus looks again the moment before
  it saves over the file, and reads in anything the other one wrote first.
- If Nexus cannot open a logbook at all, it stops before anything can be logged and says why.

**Big logs and the radio.** With 150,000 contacts a window now uses about 5 MB for the log
instead of 67 MB, and loading the log no longer keeps a spare copy's worth of memory (about
220 MB at that size). Exports, the start of a LoTW upload, the Needed board, a satellite pass's
needs, the statistics and the Confirmations diagnosis read the log while the radio carries on,
and an upload being marked no longer makes Awards, the Needed board and the Journey start over.
Logging a Field Day contact no longer makes the radio wait for the disk. The countries and US
states Nexus fills in for contacts that lack them are saved into the log in the background, and
the diagnostic log no longer reports a failure for DX contacts that could never have a state.

**The Logbook window.**

- Tab into the list and move with the arrow keys, Page Up and Page Down, Home and End. Enter
  edits the contact you are on; Delete asks first.
- Date and Time are two columns (#239). A contact imported with a date and no time shows a dash
  in the Time column.
- Double-click a row for everything the contact holds: name and QTH, grid, their rig and
  antenna, your power, the comment, which services have it and which confirmed it, and any
  imported ADIF fields with no column of their own (#313).
- **More columns** has an **End (UTC)** column, and the edit form an **End (UTC)** box.
- A private note opens in its row, the way a long comment does.
- The **SAT▸** menu sets, corrects or removes a contact's satellite tag, from the satellites LoTW
  accepts.
- Sorting, searching or filtering goes back to the first contact. When the log changes above
  you, the rows you are looking at stay where they are, and an edit mark or an open comment stays
  on its own contact.
- A change made in another window or by a sync is never overwritten: Nexus says so and shows the
  contact as it is now.
- A paper QSL card you tick as received counts toward your awards at once, and an imported
  contact carries its US state from the moment it arrives.

## Two receivers: the IC-7610 and IC-9700

On an IC-7610 or IC-9700 run through Nexus's own CI-V control, Phone's Receiver pane and CW's rig
controls show a **SUB** row for the second receiver, and Main's controls are labelled **MAIN**
while it is there.

- The row shows the Sub's frequency where Nexus knows it, which is the uplink during a satellite
  pass.
- Its sliders set the Sub's RF gain, and on an IC-7610 its AF gain and squelch. They change only
  the Sub. An IC-9700 gets RF gain only, because Icom's documentation does not say its Sub has an
  audio stage of its own.
- Nexus does not read the Sub's levels back from the radio. Each slider shows the last value the
  radio accepted from Nexus, and a knob turned on the radio is not reflected.
- The Remote page gets the same row, with sliders that work while you hold control of the station,
  once its hosted page is updated for this version. Until then it shows no SUB row.
- Other two-receiver radios (FTDX101, TS-990S, IC-9100, IC-910H, FTDX5000), an Icom run through
  Hamlib, and every radio with one receiver look exactly as before.

On an IC-7610 through native CI-V, the S-meter, the receive controls and the CTCSS tone act on the
Main receiver even with the Sub band selected on the radio, and the frequency and mode Nexus shows
and sets are Main's. Split works as before. The attenuator offers all fifteen of the radio's
pads, 3 to 45 dB.

## Satellites

- **Sked with a station.** Under the schedule, type another station's grid square or callsign
  (Nexus finds the square from the last time you worked them) and get the windows over the next
  fortnight when one of your ★ birds is up for both of you: the day, the times, how long it lasts,
  which bird and where to point. A window needs the bird at least 5° up at both ends at the same
  moment, each side's elevation is shown, and a window far enough out that the orbital elements
  will be stale is marked "re-check". If there is none, it says how far apart you are.
- **FT8 and the other data modes go up an inverting transponder on the mirrored data mode**
  (DATA-L up for DATA-U down), so an over sent up RS-44 or AO-7's mode B comes back down the right
  way round to be decoded. FT timing, sequencing and audio are unchanged.
- **QO-100's narrowband transponder** is worked as the linear transponder it is: USB for phone,
  CW for CW and DATA-U for digital, not FM.
- **KOSEN-1's uplink** is no longer put in FM, which that part of 15 m does not allow.
- The transponder list shows a packet downlink's data rate (`AFSK · 1200 bd`), and each card
  shows the downlink's mode as well as the uplink's.
- A same-band pass through an inverting transponder logs the frequency you sent on.
- On a cross-band pass on an IC-9700 or IC-905 through native CI-V, the band strip shades the
  phone segment of the band you transmit on.
- **Field Day:** a contact typed in the Satellites log strip goes into the Field Day log on the
  bird's band, a satellite counts as its own band, a single-channel FM bird allows one contact
  per station, and the 100-point satellite bonus is on the checklist.

## Phone and CW

**The Phone cockpit** is laid out the way the signal runs. The **Receiver** pane starts with an
S-meter and holds filter width, the attenuator and preamp, RF gain, NB, NR, the notches, AGC, AF
gain and squelch. The **Transmitter** pane holds mic gain, the speech processor, VOX and the
monitor level.

- AF gain, RF gain and squelch are new, through Hamlib and through Nexus's own CI-V. With AF
  nearly off, or the squelch up outside FM, the control shows **DECODE?**: if your soundcard is fed
  from the radio's speaker or headphone jack, that silences FT8, RTTY and PSK too.
- The attenuator and preamp are offered as the steps your radio has. A control your radio has
  not reported is named once at the foot of its pane, **Not on this radio: …**, rather than
  taking a row.
- A line above the transmit meters shows the frequency your next over goes out on (with split and
  XIT in it), the mode, the split offset, XIT and your power, marked where the radio confirmed a
  figure and where Nexus only commanded it. On FM through a repeater it gives no transmit
  frequency rather than the one you are listening on. With VOX on it warns that Stop TX cannot
  unkey a transmitter your voice is keying.
- The SWR, ALC, power and compression meters in Phone and CW keep the last over's reading until
  you key again. The power meter's full scale is your radio's **Rated power (W)**, a new setting
  in Settings ▸ Radio ▸ Rig & CAT, 100 W unless you change it.
- The notch buttons are named **Auto notch** and **Manual notch**, the same on every radio. On a
  Yaesu the old names read the wrong way round.
- On an Icom through Nexus's own CI-V, the manual notch and the speech processor's depth are back.
- **→ CALL** turns the beam toward the station you are working, now from Phone as well as CW and
  Operate, and **LP** beside it turns to the long path (#338).
- VFO A and B swap from the tuning strip with **A⇄B**, and the A/B indicator follows the radio's
  own A/B button on rigs that can report it.
- The scope's frequency numbers grow with the UI scale (#215).

## What the transmit lock checks

Near a band or segment edge, each of these could put your signal outside your privileges with
nothing locked. Each is now judged where the signal actually goes:

- **Phone:** the sideband you picked (USB, LSB or AM), not the one the band normally uses.
- **FM through a repeater:** the repeater's input, where you transmit, not the output you listen
  on. A shift set only on the radio's own front panel is still invisible to Nexus.
- **RTTY sent as audio tones:** the sideband the radio is actually on.
- **CW from the soundcard keyer:** where the tone goes, a pitch away from the dial.
- **Satellite uplinks:** data modes, PSK31 and RTTY on the side the transmit VFO is set to,
  including right after each Doppler correction.
- **The CW ID after an FT 73:** where it is sent. An ID that would fall outside your CW
  privileges is not sent.

Changing your licence class cancels an FT over that was about to go out under the old one. And
XIT is no longer offered on the IC-9700, on 56 other radios that have none, or through OmniRig:
those radios transmitted without the offset while Nexus showed it and judged it. The lock now
checks the frequency they really transmit on, which can lift a lock the unused offset caused, or
add one it hid.

## Switching radios

- The connection that keeps your other radios live no longer reaches for the port of the radio
  you just left before Nexus has let go of it. On Windows that logged "Access is denied" (seen
  with an IC-9700 on native CI-V) and could drop the radio back to Hamlib.
- A rare switch could leave the radio you left transmitting, on a radio connected through Hamlib.
  Its connection is now closed only after it has stopped transmitting.
- A radio keyed by RTS or DTR keeps keying on its own line after a switch, a radio with its own
  PTT port switches as quickly as any other, and two radios keyed through one controller port
  (SO2R) each key after a switch. From a Remote browser, both of those radios can now be chosen.
- Requests meant for the radio you chose no longer reach the radio you are leaving. A tune-up or
  a voice-memory playback asked for at the moment of a switch is dropped: press it again.
- **Voice memories.** A switch asks the radio you leave to stop a voice memory it may be playing,
  and Stop TX, a logger's Halt TX and the high-SWR cutoff ask the radio to stop its voice memory
  after they unkey it. The stop goes to Icom radios on Nexus's own CI-V connection (IC-7300,
  IC-7610, IC-9700, IC-705 and IC-905) as the radio's own Voice TX memory stop, and through
  Hamlib, which has the command for Kenwood radios. Other radios on Hamlib may refuse it, and then
  nothing changes.
- After a switch, an Icom on Nexus's own CI-V connection uses its own Data mode setting (D1, D2
  or D3), which picks the audio input it transmits from, not the previous radio's.

## Contests and Field Day

- **The RTTY cockpit shows what you are sending** on a line beside the typing field: the part
  already sent underlined, the rest after it, and anything a Stop kept off the air struck
  through. Every over also appears in the decoded-text window as it goes out, in the accent
  colour. Both cover F-key macros, Enter, Auto calls and continuous TX (#379).
- **A contest merged into your logbook keeps what the other station sent.** In CQ WW RTTY, the
  state or province a US or Canadian station sends goes into the contact's State field as the
  standard ADIF code, and both signal reports go in for any contest whose exchange has them, so
  World Radio League, QRZ and the other services a merge uploads to receive them. World Radio
  League has no field for a CQ zone, so the zone stays in your own log and your Cabrillo file.
- **Merged contacts keep their frequency**, including under split, and a contest phone contact
  logged from now on records its sideband (an FM contact is no longer logged as SSB). RTTY
  contacts, and contest contacts in phone, CW and PSK31, are logged on the frequency the signal
  was on. A contest merged before this release is not repaired, and merging it again does not
  fix it.
- Correcting a busted callsign reaches the Cabrillo you submit, with the country, multipliers,
  dupes and score worked out again.
- The station you called first keeps its serial when you move on, and clicking a spot ties the
  serial to that station.
- The dupe warnings name the band and the mode only where the contest's rule uses them, so a
  Sweepstakes dupe no longer points you at the band you are on.
- Turning Field Day on or off re-arms the RTTY auto-sequencer after a finished contact, and
  turning it on no longer stops the radio to read a busy logbook.

## Digital modes

- **PSK** has eight F-key macros in two sets, Everyday and Contest, that you can edit and send
  with F1 to F8 (#316).
- **RTTY Auto call** has a listen time and a repeat count in Settings ▸ Digital ▸ RTTY (#304).
- A decoded line counts as a message to you only if both callsigns in it are callsigns, so
  garbled text can no longer be answered while you call CQ, and JS8 lines stay out of the FT8
  Rx Frequency pane (#303).
- The RR73 that finishes your QSO stays in the Rx Frequency pane with −B4 or Hide confirmed on
  (#268).
- FT4, Q65, FST4 and FST4W contacts export as `MODE=MFSK` with the submode, the way WSJT-X writes
  them, so LoTW accepts them. Contacts already in your log are unchanged until you export them
  again.
- On a Flex, clicking a digital section from phone or CW checks that the radio took the data
  filter, sets it again if not, and says so if it kept its 6 kHz one (#349).
- The waterfall says "TRANSMITTING — display held" during a tune as well (#230).

## Spots, alerts and parks

- A station on your watch list wears a **WATCH** tile on the Call Roster, the Stations list and
  Spots, counts as a need for **Needed only** and **Hide worked**, and goes to the top of the
  Needed board whenever it is heard.
- The new-one, new-grid and CQ alerts can be limited to the continents and countries you pick in
  Settings ▸ Spots & Alerts. A station calling you and your watch list always get through (#174).
- A park activator you work from the Call Roster or Band Activity is logged with their park
  (#351), and **Needed only** and **Hide worked** no longer keep the station you just worked
  (#350).
- The POTA/SOTA board marks the activation you are hunting and keeps it marked while you scroll.
- A POTA spot with an emoji after the callsign no longer stops the Needed board, as it did on 1.13
  and 1.14.
- A POTA reference beside a WWFF or SOTA reference survives an import, and a two-fer you activate
  names one park in `MY_SIG_INFO`.

## Uploads and online logbooks

- **Your private note is no longer uploaded.** It went out with every contact sent to QRZ, Club
  Log, eQSL, HRDLog and Cloudlog/Wavelog, and in the WSJT-X UDP feed. It now stays in `log.adi`,
  your exports, and DXKeeper or HRD Logbook if Nexus forwards to them. A note sent by an earlier
  version is not taken back.
- Saving your QRZ Logbook key or eQSL password sends again every contact that service never
  accepted, as saving the Club Log password already did.
- An upload to QRZ, Club Log or eQSL is marked on the contact that was actually sent, and a LoTW
  upload's result lands on the contacts that went, even if you delete one while TQSL works.
- LoTW confirmation downloads keep going while the report is still arriving, instead of giving up
  after sixty seconds.
- Awards' upload and push buttons act on exactly the contact they list, including one logged
  after Awards was opened.

## Radios and CAT

- A refused repeater tone is sent again instead of being written off (#319), one missed reading
  no longer costs a radio its S-meter, and Nexus stops asking a transmitting radio for meters it
  never answers (#331).
- When the radio refuses CAT CW keying, the warning quotes what the radio said (#332).
- The SWR meter shows `SWR?` without colour coding on a rig whose SWR scale Nexus cannot vouch
  for. On a FlexRadio, Settings no longer calls the high-SWR cutoff verified while **Flex native
  panadapter (early access)** is off, since the cutoff then has no SWR to read.
- "NO RF POWER" no longer shows on an FT-710 that is transmitting normally.
- A radio found by **Detect my radio** is named for the radio, not its USB chip, so your contacts
  no longer record "CP2105 Dual USB to UART Bridge Controller" as your rig.
- The FT-890 gets its 4,800 baud CAT rate, the setup wizard can reach every rig in the catalog
  with **Show all models**, and the IC-7600 is in its short list.
- When another program already has the sharing port, Settings ▸ Radio says so instead of showing
  an address nothing is listening on (#165).

## Screen and setup

- **Field mode** is in Settings ▸ Appearance ▸ Workspace, and **High contrast** is a switch of its
  own that leaves your UI scale alone (#215).
- The spots on the 3-D map no longer flicker.
- A station in the Stations list no longer runs under its SNR badge at 1024×768.
- After a question or a dialog closes, the keyboard goes back to where you were.
- The callsign card no longer shows "New DXCC!" for a country you have worked while the log is
  still loading, and a previous contact's long comment opens in the card (#162).
- Getting started names the **WSJT-X UDP API** switch that JTAlert and GridTracker need, and says
  it ships off (#353). The SSTV screen links to the switch that keeps an IC-7300 or IC-7100 in its
  data mode (#191).
- A repeater search that could not reach one state names it. Spanish and French no longer show
  `\u00a0` in labels, and the Linux AppImage no longer complains about `canberra-gtk-module`.
- A slow space-weather, POTA/SOTA or callbook server can no longer hold up the rest of Nexus.
- Nexus Remote: a logging request sent again after a lost reply gets its receipt instead of "the
  station was busy".

## Corrections to the 1.14.0 notes

- **#290** was credited as done in 1.14.0, but only the give-up notice shipped. The catch-up for
  QRZ and eQSL is in this release.
- **#334** was credited as done. Only the band-display half shipped; the band jumps reported with
  it are not addressed.

**139 changes since 1.14.0.** The full list is in the CHANGELOG.
