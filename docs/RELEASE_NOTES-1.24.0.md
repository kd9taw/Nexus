# Nexus 1.24.0

Nexus can now hold your audio back while an amplifier switches, so the first moment of every over reaches the antenna
through a settled relay. A new **Best times** box tells you whether you can work a station now, on which band, and when
the next good window opens. Predictions now use the P.533 model by default, with your own power and antenna.

When a spot or your callbook names a station's island group, the IOTA reference is now logged for you and sent with the
contact. Confirmation reports and card inbox files you download by hand are read as confirmations, not as new contacts.
A contact you delete stops going to every service it has not reached yet.

Most of this release's fixes concern Stop TX and the end of an over. Stop TX silences the tones at once, stays exactly
where it is on screen, and works on the first click over a dialog or an open menu. A slow radio no longer holds your
transmitter past the end of an over.

## The short version

- **An amplifier TX delay**, 0 to 500 ms per radio, so an amplifier keyed from the radio has time to switch.
- **Best times to work a station:** type a call or right-click one almost anywhere.
- **P.533 is the default prediction engine.** Some band answers change; see Upgrading, below.
- **IOTA references logged for you** from spots and your callbook, sent with the contact, with the island group's name.
- **Stop TX silences the tones first,** stays put on screen, and works over dialogs and menus on the first click.
- **A large TX meters pane you can size** on FT, Phone, CW and SSTV, and TX meters on RTTY, PSK and JTTY.
- **A memory opens the screen you saved it from,** so an SSTV memory brings you back to SSTV.
- **A virtual serial port can be your CAT port,** for a rig-control program that shares your radio.
- **Field Day bonuses that count per unit,** and the natural power list.
- **TempoFast copies far more of what it hears,** in simulation: under 1 % of strong frames missed, was about 7 %.

## Upgrading from 1.23

### Check this first if you use the fast prediction heuristic

**P.533 is now the default prediction engine, and stations on the fast heuristic move to it once when they update.** A
notice in the pop-up corner says so until you dismiss it. P.533 uses your station power and antenna gain, models the
long path and answers per mode, so Band Outlook, the path card, the region strips and the DXpedition windows may name a
different band or hour than before. "Excellent" now means phone is usable most days at its best hour, not just that FT8
gets through. To go back, set Settings ▸ Services ▸ Prediction engine to the heuristic, and it stays.

### Check this first if you chase islands

- **The IOTA reference is filled in for you** when a cluster spot or your callbook names the station's island group. The
  log strip, the Logbook's Add form and the confirm-before-log popup show it already filled, with where it came from;
  change or clear it (×) before logging. The Use button is gone.
- **It now goes with the contact to every service and logging program**, and counts for the award like one you typed.
- **A reference an earlier version held back for you to confirm is no longer held.** It goes out the next time that
  contact is sent. The contact card's Confirm button and the Awards view's "waiting for you to confirm" count are gone;
  the card's Clear takes a wrong one out.
- **With the island list on,** the group's name is added to the end of the contact's Comment ("IOTA EU-005 Great
  Britain"), and the Comment is sent as before.

### Behaviour you may notice

- **FT4 tones start 0.2 s earlier, at the same point in the slot as WSJT-X,** so your DT matches other stations. FT8 is
  unchanged.
- **FT1 overs start 0.3 seconds into their slot.** See Operating FT, below, for the limits this brings.
- **A blind call keeps your Tx 1st / Tx 2nd period,** as WSJT-X does, instead of copying whoever was decoded last.
- **Field Day bonus ticks you already had:** Emergency Power still scores 100; a ticked Youth box now scores 20 (one
  young operator) until you type how many; the old GOTA tick adds nothing, because GOTA points now come from the club log.
- **Editing a CW key during a contest** now saves to a profile of its own named for that contest. Your everyday keys stay
  as they were.
- **SSTV's picture composer is at the bottom,** just above Send and Stop. The gallery's **Reveal** is now **Open folder**.
- **Transmit alarms show on the transmit strip,** beside the TX state, instead of as a row above it. The TX watchdog's halt
  is one of these messages now, no longer a chip of its own.
- **When the radio stops answering CAT,** transmit turns off once and the strip says "CAT lost: press TX On". When CAT
  comes back it says "CAT is back: press TX On"; transmit stays off until you turn it on.
- **A new screen layout gets the Best times box once,** as a tab. If you remove it, it stays removed.
- **The large TX meters pane starts hidden,** so your layout does not change.

## Transmitting and Stop TX

- **An amplifier TX delay.** Settings ▸ Radio ▸ Amplifier ▸ Amplifier TX delay, 0 to 500 ms in steps of 10, per radio.
  After Nexus keys the radio it waits this long before sending audio. It is off unless you set it. It works wherever
  Nexus makes the audio: the FT and other digital modes, the computer or remote microphone on Phone, the voice keyer,
  soundcard CW, and Tune when Nexus makes the tune carrier. It cannot help with FM, AM, FSK RTTY, CW the radio sends
  itself, the radio's own microphone, VOX, or an amplifier keyed by RF sense; use the radio's own TX delay for those.
  FT8 already starts each over with half a second of quiet, so a delay changes nothing there unless the radio is slow to
  key. FT4 and JS8 keep their own timing and ignore the setting, as WSJT-X does. Stop TX during the delay unkeys at once
  with nothing sent.
- **Stop TX silences the tones at once on a radio slow to answer.** The audio now stops first and the radio unkeys
  straight after, as at the end of every over and as WSJT-X does. Before, the tones went on for up to a second or more
  while Nexus waited for a network radio or a slow serial link.
- **Stop TX stays where it is.** A status message, the amplifier first answering, CAT dropping or coming back, or a
  transmit alarm no longer moves Stop TX or the transmit strip. Urgent messages also show on the transmit strip next to
  Stop TX.
- **Stop TX is on screen the moment a screen opens,** on small windows at large text sizes too.
- **Stop TX works on the first click over a dialog or an open menu.** The dialog stays open with your typing in it.
  Tune, PTT and TX On/Off still wait until it closes.
- **A slow meter reading no longer holds your transmitter past the end of an over.** Near the end of an over each reading
  gets only the time that is left, so PTT drops on time.
- **An internal error during Stop TX no longer delays the unkey.**
- **A silent CAT link no longer cuts your transmission or keeps turning transmit off.** Nexus waits for your over, tune
  or PTT to end, turns transmit off once, and tries the port less often the longer the link stays silent.

## Operating FT8, FT4 and FT1

- **FT4 starts its tones where WSJT-X does,** 0.2 s earlier than before.
- **FT1 overs start 0.3 seconds into their slot, as intended,** so a station whose clock runs a little fast still
  decodes you. The other station needs no update. The cost: an over now decodes when it arrives up to about 0.6 s late
  (counting your radio's key-up time and your clock error), where before it was about 0.9 s. A radio that takes more than
  about 0.6 s to key, such as some remote setups, now loses its FT1 overs. A Call CQ or double-click during your own FT1
  slot now goes out at once only in its first 0.16 s or so; after that it waits for your next slot.
- **A band change just before your own slot no longer starts that over late** with Split Operation = Rig.
- **FT8 and FT4 key after Call CQ on a band change, on a radio slow to answer split commands.** That over now sends only
  the split command before keying, so a radio taking up to half a second per command keys on time.
- **The first over after connecting is on time** on some older radio drivers, which used to hold up Stop TX and the end of
  the over for a second or more while Nexus asked about the ALC scale.
- **Calling a station you have not decoded keeps your period;** a decoded line, a roster entry or a heard station still
  answers on the opposite of theirs. Clicking a line too old to hold its decode now asks you to wait for the station's
  next transmission.
- **TempoFast copies far more of what it hears.** In simulation the missed share at +15 dB falls from about 7 % to under
  1 %, and at -10 dB from about 43 % to about 9 %, with no more false decodes. It helps every station you receive, whatever
  version they run. A very crowded band takes about a fifth longer to decode.
- **The guide explains keeping good time with no internet,** for FT8 and FT4 at a portable or club station.

## Meters and screens

- **A large TX meters pane you can size.** ⊞ Panels ▸ "TX Meters, large" on FT, Phone, CW and SSTV shows power, ALC and
  SWR as big needle meters that grow with the pane. It only shows readings.
- **TX meters on RTTY, PSK and JTTY,** from a medium-sized window up, so you can set the audio drive against the ALC.
- **The needle TX meters are big enough to read,** about twice as wide on a 1080p window, with the name and value beside
  the dial.
- **The S-meter holds its peak** for a second on Phone.
- **Listening volume in the top bar:** click the RX level meter for a slider on every screen.
- **Pick the RTTY baud rate on the RTTY screen,** next to Arm RX.
- **A memory opens the screen you saved it from,** for FT, CW, Phone, RTTY, PSK, JTTY, JS8, SSTV and APRS. The memory
  editor's new **Opens on** box shows and changes it; memories you already have recall as before. A memory recalled to
  SSTV, RTTY, PSK or JTTY leaves Enable TX as it was.
- **"TX off" is the same colour on every screen,** and the "TX on" light in Settings ▸ Radio is green when transmit is on.
- **The SWR tip names your own cutoff** when "Stop transmitting when SWR is high" is on.
- **A scope clears when the radio goes away** and says "Scope data unavailable".
- **FT's meters show "⊘ not reported" in full,** and "No decodes yet" no longer loses its heading in a short pane.
- **Settings search finds the Tune timeout** under "tuner".

## Radios and CAT

- **A virtual serial port can be your CAT port,** such as one made by a rig-control program sharing your radio.
  Nexus recognises a virtual port without opening it. Every real USB or serial port, and anything
  Nexus cannot positively identify as virtual, still has RTS and DTR held low exactly as before.
- **CAT no longer hangs with an older system copy of the rig-control library,** and asking it about the radio no longer
  opens your computer's first serial port with nothing held low.
- **CAT Auto-test waits for each radio it tries,** and finds radios that take longer to open. A port where nothing answers
  can now take a minute or more, and the screen shows how long it has run.
- **Output power on radios whose driver gives it only as a share of the meter,** shown as **PO≈** against the rated power
  you set. Your log keeps using Station power.
- **An SDR program that refuses a data mode no longer shows an error at every transmit.** Nexus notices while you receive
  and uses plain USB or LSB for that connection.
- **Network rig control set up with a COM port now says what is wrong** and what to do, instead of "No such file or
  directory".
- **An ATU press that comes to nothing now says why:** refused, no answer, or dropped and why.
- **Rotators:** a rotator program that reports elevation moves both axes again on satellite and Moon tracks, and a high
  satellite pass no longer lays an az/el antenna on the horizon when the rotator refuses an elevation.

## Licence checks and bands

- **MSK144 is judged where it transmits,** 1000 to 2000 Hz above the dial, so it can no longer slip into a CW-only
  segment. For US licence classes, Q65-30E and Q65-15D are refused below 28.3 MHz, and MSK144 on 2200 m and 630 m. Nothing
  is allowed that was refused before.
- **A band no longer opens on its own edge.** 3 cm and 13 cm open on the QO-100 narrowband transponder with the Open
  class, and on the calling frequency with a US class; a radio that transmits on 3 cm itself opens on 10368.100. US
  classes on 1.25 m, 70 cm, 33 cm, 23 cm and 6 cm open on the calling frequency, 160 m Phone at 1.8458 LSB, and 2200 m
  Phone no longer tunes off the band. Your privileges are unchanged.
- **QO-100 in the Phone and CW band menu,** when Satellites is on. It opens the Satellites screen and tunes nothing itself.

## Predictions and finding stations

- **Best times to work a station.** On Conditions, or from the right-click menu on a call almost anywhere, Nexus says
  whether you can work it now and on which band, the next good windows over the coming day in UTC and your time, and
  which days this week a Kp forecast reaches the path. It says what the answer rests on, never gives a percentage, skips
  bands your licence keeps you off, and gives no answer under 300 km.
- **P.533 is the default prediction engine,** lowered by the same storm penalty during a geomagnetic storm, and it says
  which power it assumed. Live spots still win over any model. Predictions run below normal priority, so decoding comes
  first.
- **Only new parks and summits:** an **Only new** chip on the parks and summits board shows just the ones you have never
  logged.
- **The parks board always shows the band you picked,** and the Spots board remembers your modes and band.
- **A caller's grid square opens on a web map.** Only the square goes to the website.

## Logging and confirmations

- **Confirmation reports and card inbox files you download by hand are read as confirmations.** Before, Import read them
  as a log, adding contacts you never made and, for cards, duplicate copies of your contacts. Now each one confirms the
  contact it matches and nothing is added; Undo takes them back off.
- **Build a log from a confirmation report:** after such an import, **Add N unmatched rows as new contacts** adds the
  confirmations that match nothing in your log. Nothing is added unless you press it, and the added contacts are never
  uploaded back.
- **Check the "QSL sent" marks electronic cards left on your contacts, and clear them.** Earlier versions copied the
  sender's own mark onto your contact. The Logbook lists each one, all ticked; untick any you set yourself, and Clear
  takes the rest off. Undo puts them back.
- **Electronic cards bring the sender's message,** a missing grid, and an Authenticity Guaranteed card shows as **AG** in
  the Logbook's QSL column, which now holds every mark in full.
- **Moving your log from another program does not upload it again** to the card service when the file says it was sent.
- **A contact you delete stops going anywhere it has not gone yet,** on every service, including retries, contacts held in
  a contest, and ones waiting for a logging program. Undo brings the contact back without sending the cancelled uploads.
- **A master logging program on the same computer gets each contact once,** instead of again on every retry elsewhere, and
  contacts wait for it instead of being lost when it is not running, across a restart too. Copies already sent are not
  removed.
- **Another logbook program that was closed gets the contacts logged meanwhile,** up to 256, in order.
- **The path Nexus grades for a contact stays in Nexus,** out of every upload and export.
- **One click from an activation's file to the parks program's upload page,** with the file shown in its folder. Nexus
  sends nothing itself and holds no login.
- **The logbook's database engine is updated for a security fix.** Nothing changes for your log.

## Contests and Field Day

- **Field Day bonuses that count per unit score per unit:** emergency power per transmitter, youth per young operator,
  message handling per message, and GOTA contacts counted from the club log. New boxes for the GOTA coach and site
  responsibilities. Older Nexus versions keep scoring each box once.
- **The natural power list:** tick "On natural power now", and Export ▸ Natural power saves the list for this laptop.
- **A Field Day host's club log survives a sudden power cut,** and each laptop's own log, message queue and JS8 inbox save
  safely too (Linux and macOS).
- **The host's own contacts after Contest over? stay out of the club's log,** as at every other contest.
- **Contest over? puts back the Band category,** so the next contest's Cabrillo does not claim it.
- **A legal contact after one in a banned mode with the same station now counts,** at state QSO parties and Winter Field
  Day that credit it.
- **The offer to merge old contest contacts can be dismissed** for good.
- **The ×1 Field Day power hint is right:** more than 100 W from any power source.

## Other fixes

- **Saving a CW macro key no longer interrupts anything.** It used to end a remote browser's control and drop a waiting FT
  message.
- **Send once keeps your text** in RTTY, PSK and CW when the message does not go out.
- **Closing a satellite pass alarm stops it,** and **Open in Satellites** takes you to the pass.
- **SSTV's composer** leaves the middle of the screen to the gallery, with a smaller preview on a short window.

## Known issues

These have been tested in software or against a simulated radio, not yet on the air. Please tell us how they work on
yours.

- The amplifier TX delay with a real amplifier.
- Stop TX silencing the tones first, and FT1's quiet start.
- A virtual serial port with a real rig-control program sharing the radio, on Linux or a Mac, and whether its scope comes
  through.
- The first-over ALC check on older radio drivers, CAT Auto-test's longer wait, and the ATU's new messages.
- The SDR program data-mode fallback.
- A rotator program reporting elevation.
- TempoFast's decoding gains, which are simulation figures.

FT1's limits on a radio slow to key, above, are known and still open.

**More than 70 changes since 1.23.0.** The full list is in the CHANGELOG.
