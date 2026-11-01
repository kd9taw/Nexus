# Nexus 1.22.0

The map and the globes show space weather the way NOAA sees it. During an M- or X-class flare the absorption on the
sunlit side comes from NOAA's own D-RAP map, with coloured rings round the point under the sun showing, band by band,
where HF is weakened and where it is out. The flare chip gives NOAA's start and peak times, the bands out under the sun
and at your QTH, and how long until HF recovers; click it for a six-hour X-ray graph. When NOAA issues a geomagnetic
storm watch, a chip counts it down ("G1 watch · NOAA expects 31 Oct–1 Nov (in 21 h)"), then says when it is in force,
and says "arrived" only once the solar-wind monitor at L1 measures the jump.

Nexus also tells you which way round the world a path goes. With the ITU-R P.533 engine, the Path card and the
selected station's card say which route the model favours now and which way to beam ("Beam 80° for 20m: long path
(possible)"), and the map and the 3-D globe draw the short path solid and the long path dashed, coloured by how well
each should work. **Path over the day** shows a station's path across the 24 UTC hours, band by band, with the better
route marked for each hour, and **Paths to the regions** does the same for ten DX regions. The Logbook has a **Path**
column that shows which way each contact most likely went. All of it is advice from a monthly-median model, and
nothing turns the rotator.

Nexus is now in Russian (Русский). Every screen is translated, with Russian plural forms, and frequencies, callsigns,
reports, bands and modes stay as they are. Pick Русский in Settings ▸ Appearance ▸ Language, or Nexus picks it up from
a Russian system language. Translated by loloka; completed and reviewed for this release.

This release also fixes the problems you reported most. Two radios in two windows no longer fight over each other's
radio. Card received, LoTW received and eQSL received show their dates. A Flex never puts the shack's microphone on
the air in place of a voice message, an APRS beacon or an SSTV picture. Split Operation works as it does in WSJT-X.
Mac CAT works on macOS 12 and 13. Settings backups and restores keep your service keys safe, and a restore always
keeps this computer's own TQSL program, JS8 sound files and voice-keyer recordings. And Nexus uses much less of the
computer when nothing is happening on screen.

**Updating from inside Nexus never worked on the stable channel.** Install 1.22.0 from the download page once, and
later updates come through Nexus.

## The short version

- **A solar flare as NOAA sees it**, on the Flat map, the Globe and the 3-D globe, with band rings and a flare chip.
- **NOAA's storm watch, counted down** on the map and the globe, with the WSA-Enlil model's view and the L1 arrival.
- **Short path or long path, and which way to beam**, on the Path card, the station card, the map and the globe.
- **Path over the day and Paths to the regions**: when a path opens, on which band, and which way round.
- **A Path column and Path filter in the Logbook**: which way each contact most likely went.
- **Nexus in Russian (Русский)**, every screen, translated by loloka and completed and reviewed for this release.
- **Two radios in two windows** each keep their own radio, and PSK Reporter stays live in both.
- **QSL received dates:** a card you mark received is dated, and LoTW and eQSL downloads bring their dates.
- **A Flex never transmits the shack's microphone** in place of a voice message, a beacon, a picture or a Remote
  operator's voice.
- **Split Operation as in WSJT-X:** no over on the wrong frequency, split left on between overs, and Rig split on
  Nexus's own Flex client.
- **Mac CAT on macOS 12 and 13**, and a Mac build for Intel Macs.
- **Your service keys stay safe** in settings backups and restores, and a restore keeps this computer's own TQSL
  program, JS8 sound files and voice-keyer recordings.
- **Much less CPU** from waterfalls, decode lists, hidden screens, idle decoders, the 3-D globe and a missing sound card.
- **The in-app updater works**, once you have installed 1.22.0 from the download page.
- **A contact card in the Logbook**, a ⋯ menu on every row, Upload to LoTW for one contact, and columns you can move.
- **QSL labels and a cards-to-send list**, award status (Submitted and Granted), the US Counties award and new IOTA
  needs.
- **Talk through this computer's microphone** in Phone.
- **APRS through the ISS**, with Doppler correction.
- **Set up radios**, a walk-through for a station with more than one radio, and a line when Nexus switches radio by
  itself.
- **Search Band Activity** in FT8 and FT4.

## Upgrading from 1.21

- **Install this version from the download page.** The in-app update fails on 1.21 and earlier with "Cannot read
  properties of undefined (reading 'rid')". From 1.22.0 on, Install restarts into the new version.
- **If you re-ran the setup wizard from Settings on an earlier version, check Settings ▸ Station ▸ License Class.**
  The wizard could save "Outside the US" over your class, and with it the transmit lockout for your privileges.
- **Settings is regrouped.** Satellite Doppler and the orbital elements have their own **Satellites** tab. Logging &
  Connectors is now **Services**. The old Config tab and the computer settings from Appearance are on **App**. Every
  setting keeps its value, and search finds each one where it is now.
- **Logbook rows show two buttons, ✎ Edit and ⋯.** Uploads to QRZ, Club Log, HRDLog, World Radio League and LoTW, QSL
  marks, Spot and Delete are in the ⋯ menu. Nothing in a row takes a click unless you can see it.
- **Digital modes leave your radio's filter alone.** Nexus no longer asks for a 3 kHz filter on FT8 and the other data
  modes, the same as WSJT-X. The radio's own DATA filter is used.
- **Split Operation:** with Rig or Fake It, if the radio does not take the split frequency, nothing is sent and the
  status bar says so. Rig split stays on between overs and is turned off when you change band, radio or Split
  Operation, turn TX off or leave the digital screens. A TX offset of exactly 2000 Hz is now shifted.
- **FT8 hides rover (/R) calls and "TU;" messages** unless Contest mode is on or you are a Hound, as WSJT-X does.
- **"Prompt before logging" opens the popup by itself with auto-log off too**, in FT8, FT4 and RTTY.
- **A band change ends the FT contact you were working** and clears the DX call. A contact already complete logs at
  once with auto-log on; one that had exchanged reports stays loggable with Log QSO.
- **Clicking a station always starts the contact fresh** from what it sent last, as in WSJT-X.
- **A contact's power is the power the radio measured** during its over, where the radio reports watts. Settings ▸
  Station power is used only when it does not.
- **A callbook lookup no longer fills in the IOTA reference.** It is offered as a chip when you log, for you to Use.
- **On an FT8 or FT4 contact logged with no grid, a callbook grid is for the distance and the map only.** It counts
  toward no grid award and goes to no logging service. A grid the station sent on the air counts as before.
- **The first QRZ Logbook, ClubLog or eQSL login you save asks before sending your old log.** The default sends only
  contacts made from then on.
- **Club sync needs this release on every laptop.** An older Nexus is turned away when it joins, and told to update.
- **JTTY's dials moved** to its published table: 160 m 1.838, 80 m 3.575, 40 m 7.090 and 17 m 18.100 MHz.
- **Each mode's waterfall keeps its own span**, and every mode's waterfall opens the full width of the window and a
  quarter of the cockpit's height. A height you already dragged is kept.
- **The FT8/FT4 strip puts Tune right after TX On/Off**, where every other screen has it.
- **The A gauge is NOAA's daily A index**, and a gauge NOAA did not give a reading for shows "—", never a made-up
  value.
- **Older eQSL cards in your InBox:** to have them counted, open Logbook ▸ Check confirmations, press Check, then
  Change.
- **Received dates for older confirmations** arrive only when a download brings that confirmation again. Cards marked
  before this version stay without a date.
- **Sharing the radio moves to the next free port** if another program already holds 4532. Settings ▸ Radio shows the
  address it really uses.
- **On Linux, Play receive audio here needs a sound card picked by name.** It refuses the system default, PulseAudio or
  PipeWire output when it could be the radio's card.
- **Pounce listens only to spots heard from the "Spotted from" chips** you pick in the Spots view.
- **On a new install**, space weather is logged with each contact and ALL.TXT is written. An existing install keeps its
  settings.

## Space weather on the map and globes

### Solar flares

- **NOAA's own absorption map during a flare.** During an M- or X-class flare the sunlit side is drawn from NOAA's
  D-RAP map, which NOAA updates every minute. Between updates, or when NOAA cannot be reached, Nexus's estimate from
  the X-ray reading stands in, and the flare chip says which one you see and when it is from.
- **Band rings round the point under the sun**, band by band: dashed where a band is weakened, solid where it is out.
- **The flare chip** gives NOAA's start and peak times, which bands are out under the sun and at your QTH, and how long
  until HF recovers. Click it for a six-hour X-ray graph. The flare heads-up quotes the same figures.
- **Below M class the map stays quiet** and only the sun turns from yellow toward orange.
- **Recovery estimates for the biggest flares** follow NOAA's estimate all the way up, about 150 minutes for an X10.
- **The proton-event chip** has its own violet edge and sits under the flare chip.

### Storms on the way

- **NOAA's storm watch on the map and the 3-D globe:** "🧲 G1 watch · NOAA expects 31 Oct–1 Nov (in 21 h)", then "in
  force", then nothing once it is over. One quiet notification for each new watch.
- **The WSA-Enlil model's view** on a second line, labelled as a model.
- **The arrival at L1**, about an hour upstream of Earth: "Arrived at L1 02:14Z · reaching Earth ~03:10Z
  (measured)".
- **NOAA's forecasters' reasoning** behind the three-day outlook, on hover in the Kp outlook box.
- **The G↗ tmrw chip reads tomorrow's forecast.** It was reading today's.

### Only what NOAA measured

- **Every Space Wx number carries the time NOAA measured it.** A reading NOAA could not give shows "—"; an old one
  shows its age, dimmed. Neither colours a gauge, gives band advice or raises an alert.
- **A band nobody has heard reads "—"** when there is no current solar flux or Kp, never "Open".
- **The SFI is the day's noon reading**, the same number as the 30-day trend beside it.
- **The aurora oval is labelled as a forecast**, with the time it is for.
- **Logged space weather** puts on each contact only the K index, A index and flux NOAA measured close to that
  contact. Nothing is filled with a guess.

## Which way the path goes

- **The Path card says why a path works**, in one sentence: the best band open now, the hops and layer, whether it
  crosses the auroral zone or the polar cap, and the grey line. A storm warning appears only when the forecast puts
  the path in the auroral zone. Click it for the numbers.
- **Short path or long path, and which way to beam**, on the Path card and on the selected station's card in the
  Classic Operate layout. When both routes work it says so, with both headings. The long path is labelled "modelled,
  unvalidated for long path" and is never called more than "possible".
- **Both routes on the map and the 3-D globe:** short path solid, long path dashed, green, amber or grey for how well
  each should work, violet past the auroral-zone edge and gold on the grey line. The LP button is ringed when the long
  path is favoured.
- **Path over the day:** the selected station's bands against the 24 UTC hours, each hour marked ▸ for the short path
  or ◂ for the long path, with a best-of line ("Best: 20m 12Z–16Z by the long path"), auroral and grey-line marks and
  a Kp ribbon. Click an hour and the 2-D map shows the grey line and the path at that hour.
- **Paths to the regions:** the same strip for ten DX regions, each on its best band. Regions where the Needed board
  hears stations come first, then those you have logged most.
- **The Logbook's Path column and Path filter** (Long path, Across the polar cap). A contact made with the Path card
  open on that station is graded as you log it; older contacts are graded in the background, for display only, and
  nothing is written to them. Nexus never fills in ANT_AZ or ANT_PATH by itself, and sends the grade to no service.

## Radios and CAT

### Two radios

- **Two radios in two windows each keep their own radio.** With Simultaneous radios on, each window connects only to
  its own radio, so neither loses it when the other reconnects.
- **PSK Reporter stays live in both windows.** For the DX cluster, give each window its own Cluster login SSID.
- **Set up radios…** in Settings ▸ Radio ▸ Radios walks through your radios, which radio for which band and mode, the
  sound cards and keying, with Test CAT and a "Where would this go?" check.
- **A line when Nexus switches radio by itself**, with Undo and Why?. New installs get it; an existing station picks
  "Tell me, with Undo" in Settings ▸ Radio ▸ Radios.
- **Each radio's routing in plain words** on its card in Settings.

### Split Operation

- **No over on the wrong frequency.** If the radio refuses or does not answer the split frequency, nothing is sent, TX
  turns off and the status bar suggests Fake It or None. It used to go out 500 Hz or more off.
- **Rig split stays on between overs**, so a Flex through SmartSDR CAT no longer opens a new slice every over.
- **Rig split on Nexus's own Flex client (Beta)**, done the way SmartSDR does it, with a second slice for TX. Needs a
  check on a Flex.

### Flex audio

- **A voice-keyer message never hands the transmission to the shack's microphone.** If the radio leaves Nexus's DAX
  mid-message, the message stops and the radio unkeys.
- **APRS and SSTV right after a voice message** keep refusing to send while the radio is on its own mic, so a beacon or
  picture never goes out on the mic. Needs a check on a Flex.
- **A Remote operator's press keys only while the radio takes Nexus's DAX.** Needs a check on a Flex.
- **A one-time note about DAX** the first time a voice message plays on a Flex through SmartSDR CAT.

### CAT and keying

- **Tune and PTT never reach the radio after you let go**, even behind a slow rigctld or bridge. The status bar says
  PTT NOT CONFIRMED when the radio did not answer the key in time.
- **Stop TX on an FT-710 or FTX-1 unkeys at once**, where it was 2 to 4 seconds late.
- **An Icom keeps its filter (FIL1, FIL2, FIL3) when Nexus retunes it.** Needs a check on the radios.
- **A network radio that is off or behind a firewall** no longer freezes Nexus: it gives up after 3 seconds and keeps
  trying in the background.
- **No received audio is lost** while Nexus reconnects to a radio whose CAT has gone quiet.
- **Icom forward power reads in watts on each radio's own scale** (IC-705, IC-905, IC-9700 and the rest).
- **Mac CAT on macOS 12 and 13.** Hamlib inside the app is built for macOS 12. Needs a check on those Macs.
- **The Icom IC-7850 and IC-7851** in the radio list, through Hamlib. Not yet on a bench.

### At the operating position

- **Talk through this computer's microphone in Phone**, per radio, through the sound card or TCI audio, and on a Flex
  over DAX. The microphone is open only while you hold PTT and every transmit check passes. Off by default. Needs a
  check on the air.
- **The RF scope fits the band**, edge to edge, and asks the radio for the finest span that covers it. Not yet checked
  on a radio.
- **HiDef on the CW scope:** a carrier draws about 4 to 5 Hz wide. Fast/HiDef switches back.
- **Filter, Std and Full spans** on the CW audio scope, and the Phone audio scope remembers its span.
- **Set the speech processor from the COMP meter**, and on Nexus's Icom connection, PROC and a WIDE/MID/NAR chip.
- **ATU ON** while the radio's tuner is in line; right-click to switch it out.
- **Finals temperature** beside the TX meters on a Flex and an FTDX101D/MP, and an amplifier's beside it.
- **Choose the audio waterfall on an Icom** in place of the radio's scope.
- **Play receive audio here** works with Audio over TCI and native Flex audio.

## FT8, FT4 and the WSJT-X modes

- **Search Band Activity** for a call, grid, report, CQ modifier or country. The list says "12 of 87 match" while it
  filters. Decoding, alerts and transmitting are unchanged.
- **Recent contacts in the FT cockpit**, in ⊞ Panels. It starts hidden.
- **A 73 after clicking a station's RR73 or RRR is offered for the log**, as WSJT-X opens Log QSO.
- **A click never loses a contact waiting for Log QSO.** A count beside Log says how many wait, and they survive a
  restart.
- **The prompt-to-log popup has Prop mode, Power, Operator and My grid**, with the prop mode kept for the next contact.
- **A contact logged with no grid takes the grid the station sent** earlier in the session, within 24 hours.
- **Your own transmission lists after the message it answered**, in the decode lists and in ALL.TXT.
- **WSPR and FST4W beacons from a compound call** alternate WSJT-X's two messages.
- **A contest contact reaches your logger complete** over the WSJT-X UDP link, with QSO Logged and Logged ADIF.
- **A cluster spot on an FT4 dial says FT4.**
- **The Tx panel's dot follows the message actually going out.**

## The Logbook

- **A contact card.** Double-click a contact to correct every field it holds, with the callbook photo, Look up, the
  last 10 contacts with the call, and the contact's history with Undo.
- **Card received, LoTW received and eQSL received show the date.** A card you mark received gets today's UTC date,
  and LoTW and eQSL downloads bring the date each confirmed. A date a contact already has is never replaced.
- **Older eQSL cards in your InBox confirm their contacts.** Run Check confirmations once to count them.
- **A ⋯ menu on every row** and **Upload to LoTW for one contact**.
- **Move columns** by dragging a header, **sort on every column**, and the Logbook uses the whole window.
- **QSL labels and Cards to send**, on Avery 5160, Avery L7160 or a Dymo/Brother roll. Cards are marked sent only when
  you say they printed.
- **Award status:** Not submitted, Submitted or Granted for DXCC, WAS, WAZ, IOTA and USA-CA, and a list of what is ready
  to submit.
- **The US Counties award (USA-CA).**
- **IOTA:** island names from IOTA Ltd's list, a New IOTA chip on the Needed board, Spots and Pounce, a reference
  offered as you log, and LoTW confirmations counted toward the award.
- **Clean up ▸ Blank grids** fills a missing grid from what the station sent on the air, or its home QTH for the map
  only.
- **Download confirmations** fetches LoTW and eQSL in one press.
- **Test buttons for LoTW, eQSL and ClubLog** in Settings ▸ Services ▸ Connections. They only read.
- **A wrong LoTW or eQSL password is called one**, and eQSL's "no new entries" is no longer an error.
- **Correcting a callsign clears what belonged to the old call**, including received dates, and Undo puts it back.
- **Editing a contact keeps its stored country.**
- **Update from callbook** counts only Nexus's own QRZ lookups against its daily limit.

## APRS, SSTV, JTTY and JS8

- **The ISS on the APRS channel menu**, with **Doppler correction** during a pass and a Use ARISS prompt. Your licence
  and the TX switch hold as on any 2 m channel.
- **APRS path quick picks**, and **the APRS map with the Connect map's tools**: tracks, labels, filters and Follow.
- **APRS on DATA-FM / FM-D** on the FT-991/991A and the IC-7100, IC-9700, IC-705 and IC-905. Off by default. Needs a
  check on the radios.
- **Delete several SSTV pictures at once**, and a deleted picture stays deleted.
- **JTTY:** your own F1 to F8, WSJT-X's FT8-style set, a Log the contact prompt after TU, Decode again, and the
  smaller WSJT-X controls. JTTY contacts log as JTTY, on JTTY's frequency.
- **JS8 sounds from any screen**, each with its own WAV file, the new-message notice from any screen, and JS8Call's
  screen options.

## Contesting and clubs

- **Every laptop at a club shows the club log**, the newest 100 contacts, live from the host.
- **Import a laptop's file** into the club log on the host, for a host PC that died or a laptop that never synced.
- **ARRL RTTY Roundup.**
- **ARRL VHF Cabrillo LOCATION is your ARRL section.**

## Less load on the computer

- **The waterfall does nothing while it is off screen**, where it woke 60 times a second.
- **Band Activity and Rx Frequency** redraw only the lines that change.
- **Screens you are not looking at** redraw only when something on them changes.
- **Receive decoders stop when you leave their screen** (RTTY, PSK, JTTY, SSTV and APRS), unless you started them
  yourself.
- **The 3-D globe rests while you cannot see it.**
- **A missing sound card** is looked for less and less often, settling at every 30 seconds.
- **A red alarm in the top bar** pulses three times and then stays lit.
- **A Flex or TCI connection that fails inside Nexus** no longer keeps a processor core busy.
- **Nexus asks ClubLog for DXpedition data** at most once a day.

## Your screen and setup

- **Setup asks how much you have done**, once, on a new install, and opens the screens you need first.
- **Nexus in Russian**, in Settings ▸ Appearance ▸ Language. The kit for corrections is in `translations/ru/`.
- **Nexus for Intel Macs**, signed and notarized, for macOS 12 or later. Needs a check on Intel Macs.
- **Start fresh in Getting Out**, to count only the reports that arrive after you change an antenna.
- **Save the settings backup where you like**, with Save as….
- **The main window opens on the screen**, borders and all.
- **The Spots window works at its smallest size**, and a status message no longer pushes the top bar onto a second
  line.

## Also in this release

- **Settings backups never carry a profile's Club Log or Cloudlog key**, and a restore keeps the keys saved on this
  computer.
- **A restore that points Cloudlog or an Icom radio at a different address asks before using a saved key.** A notice
  asks "Use your saved Cloudlog key with …?", with Use and Remove, and a restore is refused while you transmit.
- **A settings restore always keeps this computer's own TQSL program, JS8 sound files and voice-keyer recordings.**
  Where TQSL lives, which sound files JS8 plays and which recordings the voice keyer uses stay as you set them on this
  computer, whatever the backup says. The voice keyer's slot names still come from the backup, so on a new computer
  those slots have no recording until you record or import one there.
- **Deleting an SSTV picture can only delete a picture**, never another file in that folder.
- **Callbook answers and downloads are read only up to a sane size.**
- **Closing Nexus right after logging waits for the log.adi copy.**
- **The diagnostic log no longer counts a transmit underrun at the end of every over.**

## Known issues

- The RF scope's band span, Tune and PTT on a slow rigctld, and the Icom filter and digital-filter changes have not
  yet been checked on a radio.
- The Flex changes (DAX voice messages, Remote presses, APRS and SSTV after a message, Rig split on the native client)
  need a check on a Flex.
- Talking through this computer's microphone needs a check on the air with a sound-card radio, a TCI radio and a Flex.
- Mac CAT on macOS 12 and 13, and the Intel Mac build, need a check on those Macs.
- The Test buttons and the eQSL download changes need a check against real LoTW, eQSL and ClubLog accounts.
- The IC-7850/7851 has not been on a bench.
- The long path is modelled with a method validated for the short path only.

**167 changes since 1.21.0.** The full list is in the CHANGELOG.
