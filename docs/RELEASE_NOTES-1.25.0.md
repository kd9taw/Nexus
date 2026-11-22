# Nexus 1.25.0

FT8 and FT4 now decode with a JTDX-style decoder by default, which finds more weak signals, more stations in a crowded
band and, on FT4, stations whose clocks run late. You can now build your own contest, from its exchange to its
Cabrillo log, and share it with your club. DARC's WAE DX Contest is on the contest list, and Nexus is now in Italian.

Nexus now keeps the reports of who heard you, which PSK Reporter forgets after a day. The POTA/SOTA board filters by
continent and country, you can pick the island within an IOTA group, and Remote recovers by itself when your network
changes.

Many of this release's fixes concern keying lines: CW and RTTY FSK on the same serial port as PTT, the way a port is
typed on Windows and the Mac, and radios with no data mode. Your own transmission is no longer decoded back as received
text on RTTY, PSK, SSTV or APRS.

With more than one radio, each radio now keeps its own PTT lines, keyers and power caps, and each Nexus window is its
own station in GridTracker and JTAlert. Split puts the transmit VFO in the mode you are operating, and going to FT8
turns a CW or Phone split off. The log prompt can play a sound and be made wider, and macros send the report in your
log.

## The short version

- **A JTDX-style decoder for FT8 and FT4, now the default,** with four presets from Easy to Ludicrous.
- **Build your own contest,** export it to share, and run it on FT8 and FT4 with WSJT-X's contest messages where its
  exchange matches one.
- **The WAE DX Contest** (CW, SSB and RTTY).
- **Nexus in Italian.**
- **Nexus keeps the reports of who heard you,** on your computer, past PSK Reporter's one day.
- **POTA/SOTA board: filter by continent and country.**
- **Pick the island within an IOTA group,** logged and sent with the contact.
- **Remote recovers by itself when your network changes,** and can use less data through the relay.
- **CW and RTTY FSK key on the same serial port as PTT,** the usual interface wiring.
- **RTTY overs start and end on a new line,** and with two LTRS characters, as fldigi sends them.
- **Your own transmission is no longer decoded** on RTTY, PSK, SSTV or APRS.
- **Each radio keeps its own PTT lines, keyers and power caps.**
- **Split puts the transmit VFO in your mode,** and a split +/- press can no longer leave your licence.
- **A sound when a contact is ready to log,** and a log prompt you can widen.
- **A sound card that opens but sends no audio is named and reopened.**

## Upgrading from 1.24

### Check this first if you run more than one radio

- **PTT lines, keyers and power caps are now each radio's own:** the RTS/DTR keying lines and serial handshake, the CW
  keyer and WinKeyer or keyline port, the RTTY FSK port and line, the headphone monitor and voice-keyer microphone, the
  split and scope source, and the power caps, Tune power and SSTV power. The radio you are using keeps every value it
  had. Your other radios start with the values you had set, power caps included, so no radio transmits with more power
  than before, but with the usual defaults for the PTT wiring and keyers, so no radio takes another radio's PTT line or
  keyer port. Switch to each radio and check those settings once. The CAT broker, the JS8 API and the TCI server stay
  one for the whole station.
- **GridTracker and JTAlert see each window as its own station.** The single window is now called "Nexus" (it was
  "Tempo"), and each radio window "Nexus - r1", "Nexus - r2" and so on. If JTAlert remembers settings for the old
  "Tempo" name, choose the new name once.

### Check this first if you operate FT8 or FT4

- **FT8 and FT4 move to the JTDX-style decoder,** at the Easy preset, when you update. Settings ▸ Digital ▸ Decoder
  takes you back to WSJT-X's standard decoder, for each mode separately.
- **A decode marked `^` was not error-checked,** or is the kind of line this decoder can find in noise. Nexus shows it
  but does not act on it: it starts or advances no contact, is never logged automatically, and goes to no other program
  or PSK Reporter. You can still double-click it.
- **The FT4 decoder drops a doubtful decode of a call that is not in JTDX's list of active callsigns,** as JTDX does,
  so a new or rare call can go unseen. Choose Standard for FT4 to see everything.
- **Double-clicking a line that starts with DE now sends your report,** as WSJT-X does. "DE" followed by a call and grid
  used to be answered with your grid.
- **A CQ or a call with no grid square is now a CQ or a call,** so you can double-click it, and Nexus answers a caller
  who leaves the grid off.

### Check this first if you operate RTTY

- **Every RTTY over starts and ends on a new line,** a line you type, an F-key and every Auto over. Turn it off in
  Settings ▸ Digital ▸ RTTY ▸ "Start and end each RTTY over on a new line".
- **Every transmission starts with two LTRS characters,** so a station whose decoder was left in figures prints your
  over correctly. They add about a third of a second at 45.45 baud.
- **Reverse now also swaps mark and space on true FSK transmit,** as its help always said. If your radio's FSK input
  simply keys the other way round, use the radio's own FSK polarity menu instead, because Reverse flips the decoder too.

### Behaviour you may notice

- **Restoring a settings backup keeps your licence class and your transmit safety settings** as this computer has them:
  the TX watchdog, the Tune timeout, the SWR stop, the power limits and whether other programs may transmit through
  Nexus, among others. On a new computer, set them in Settings there.
- **Going back to an older Nexus keeps your settings.** Only a choice the older version does not know goes back to its
  default; a licence class it does not know reads as Open, so pick yours again.
- **"Data modes use plain SSB" starts on** for radios with no data mode (the Kenwood TS-480, TS-570, TS-690 and TS-870,
  and the Icom IC-703, IC-706 family, IC-718 and IC-7000) when it has never been saved.
- **The Tune timeout moved** to Settings ▸ Radio ▸ Transmit limits & sharing, next to the SWR cutoff. Your value is kept.
- **The large TX meters pane is smaller,** with dials half the size, and the panes beside it get the room.
- **A frequency you type is used when you click away** in Nexus, as if you had pressed Enter. Esc still cancels.
- **The listening level goes past 100 %,** up to +20 dB, for a radio whose USB audio is quiet in your headphones.
- **Going to the FT8, FT4 or FT2 screens turns a split you set on CW or Phone off,** as WSJT-X does; FT then uses its
  own Split Operation setting. A satellite pass keeps its uplink split.
- **`{RST}` in a CW, RTTY or PSK macro sends the report in the log's Sent box,** not always 599 (5NN), and 599 only when
  the box is empty. A macro you edited and saved keeps its own text.
- **Ludicrous and a Custom thread count leave one processor core free.**
- **The Windows installer is about 350 MB, about 80 MB larger than 1.24.0's.** The difference is Microsoft's WebView2
  runtime, which the installer carries so that Nexus starts on a computer that does not have it; Microsoft's current
  runtime is larger. Nexus itself is about the same size as before.

## Operating FT8 and FT4

- **A JTDX-style decoder for FT8,** the multithreaded decoder WSJT-X 3.0 offers, which is WSJT-X's port of JTDX's. It
  finds more weak signals and more stations in a crowded band. Its presets: **Easy**, a light decode on one core for a
  small computer; **Keen**, WSJT-X's own settings; **Ludicrous**, Keen with JTDX's SWL mode on up to 12 cores; and
  **Custom**, every setting. If a decode runs so long that your reply would be late, Nexus moves down one preset for the
  session and says so; it never goes below Easy.
- **JTDX's SWL mode** searches up to 3.5 seconds early or late instead of 2.5, catching stations whose clocks are off.
- **The FT8 decoder looks for the station in DX Call** across the whole band, even when you are not in a contact with
  it, until you have logged it.
- **A JTDX-style decoder for FT4,** whose search follows the timing of the stations you have been decoding. In our tests
  a signal 1.2 seconds late decoded every time, where the standard decoder found nothing.
- **"QRZ" or "DE" followed by a call** is read as a message from that station, as in WSJT-X.
- **Calling CQ, Nexus no longer answers a station that signs with your own call.**
- **ARRL Field Day: running, Nexus answers a caller who sends no grid, or a grid,** with your class and section, as
  WSJT-X does.
- **A GridTracker or JTAlert click on your own CQ no longer turns TX on,** and in a contest a click on a station's CQ
  now shows that station on the FT screen.
- **The Call Roster holds still for a second after you click a row,** so the row you double-click is the station you work.
- **On WSPR and FST4W, Tx 1 to Tx 5 no longer start the beacon.**
- **A very crowded period can no longer overrun the FT8, FT4 or FST4 decoder.** It shows the first 200 (FT8) or 100
  (FT4, FST4) decodes it finds.
- **MSK144 overs are as long as WSJT-X's.**
- **JS8:** TX On and Tune in the top bar are no longer greyed out, and an FT answer from Remote or a logger no longer
  turns TX on while you are on JS8.
- **GridTracker and JTAlert:** Nexus says it is there every 15 seconds, as WSJT-X does, and the Connections log says
  when the WSJT-X UDP API cannot send. With two radio windows, a GridTracker or JTAlert click starts a contact only in
  the window it names; a Halt TX still stops every window.
- **FT and SSTV: the waterfall's divider follows the mouse again** as you drag it.

## Contests

- **Build your own contest.** The contest setup has a **Your contests** list with **Build your own…** and **Import…**.
  Give it a name, its Cabrillo name, its modes and when it runs; choose up to five exchange fields from a fixed list,
  whether a station counts once, once per band or once per band and mode, and the points per mode. The score is the
  contact count and points; there are no multipliers. Export a contest to share it with your club. Once contacts are
  logged in it, its exchange and Cabrillo name can no longer change; save it as a new contest instead. Club sync and
  RTTY Auto do not run contests you build.
- **A contest you build speaks WSJT-X's contest messages on FT8 and FT4** when its exchange matches one: ARRL Field Day,
  NA VHF / ARRL Digi, WW Digi, ARRL RTTY Roundup (if you say so) or EU VHF. The builder shows the CQ and exchange Nexus
  sends, and Nexus never sends an exchange the message cannot carry. Any other exchange uses standard messages.
- **A contest you built that has gone missing refuses to start,** instead of running as ARRL Field Day.
- **The WAE DX Contest** (CW, SSB and RTTY), each on its own weekend, with the serial counted for you and DARC's Cabrillo
  names. QTCs and the multipliers are not supported yet, so Nexus shows your contact count rather than a score.
- **With the contest switch on, Nexus says so even in plain FT8:** the mode badge names the contest and the FT screen
  shows a **Contest** chip.
- **Sweepstakes no longer asks for the call twice:** the received exchange is NR, PREC, CK and SEC beside the call box.

## RTTY, CW and keying lines

- **CW over a serial key line keys with PTT on the other line of the same port,** and so does RTTY over a serial FSK
  line. Windows lets a port be opened only once, so whichever came second used to be refused. If a CW key-up does not
  get through, the CW pane says the key may be stuck down.
- **Each FSK over starts with about 90 ms of mark after PTT,** so the first character is not clipped.
- **CW Tune keys a real carrier through a WinKeyer or a serial key line,** instead of a tune tone a radio in CW does not
  send.
- **When Nexus closes, it releases a serial CW key line or FSK line it was holding.**
- **A Stop pressed at the very start of an FSK message is no longer lost.**
- **A port is one port however it is typed:** `COM7` and `\\.\COM7` on Windows, `/dev/tty.…` and `/dev/cu.…` on a Mac,
  and a `~/` path. That fixes a single-cable interface that tuned but did not transmit, a Mac adapter opened twice, and
  the check that refuses FSK or a CW key line on the same wire as serial PTT.
- **On a Mac, the FSK keyer and the CW key line open the `/dev/cu.…` port,** as CAT and PTT do.
- **RTTY and PSK no longer print your own transmission as received text,** and **SSTV and APRS no longer receive it.**
  Each ignores the radio's audio while you transmit and for 0.6 seconds after.
- **The FSK serial port box says what to put in it,** and points a two-port Yaesu at its Standard COM port.
- **The RTTY waterfall's title bar stays on one line** on a small laptop at 175 %.
- **RTTY and PSK macros can send names:** `{NAME}` is yours and `{HISNAME}` the other station's, as in CW.
- **RTTY: Stop from a Remote client also ends an Auto run.**
- **CW by CAT says when CAT itself is down,** instead of blaming the radio, and **says when your rig model cannot key
  CW over CAT at all** (every Xiegu model, among others), suggesting the Serial keyline or Soundcard keyer.

## Radios and CAT

- **FT8 on a radio with no data mode stays in USB.** A Kenwood TS-480 through OmniRig no longer goes to FSK-R at every
  band change and key-down.
- **OmniRig no longer keys a CAT port left over from an earlier USB setup,** and says at connect when PTT on RTS or DTR
  has no PTT Serial Port.
- **A network radio keyed by RTS or DTR on a serial cable keeps its CAT,** and Test CAT no longer shows green while CAT
  is down on a radio keyed by a separate cable.
- **Record CAT traffic:** a switch under the diagnostic log writes the commands Nexus gives the radio, with their answers
  and timing, for chasing a radio that misbehaves. No callsigns, text, audio or passwords are recorded. It turns itself
  off after 30 minutes or 2 MB, and is off every time Nexus starts.
- **Split Operation = Rig: tuning outside the ham bands no longer rewrites the transmit VFO** on every knob step.
- **Letting go of PTT, or pressing Stop TX, unkeys on time** when the radio stops answering a meter read, and **Test CAT
  waits until you are not transmitting.**
- **Test CAT no longer tries other baud rates on a port Nexus refused to open.**
- **IC-9700 satellite passes: the uplink is plain USB, LSB, CW or FM,** not its DATA version.
- **FTX-1: the ALC meter reads on the radio's own scale.**
- **Flex: Settings ▸ Radio ▸ Audio says when the waterfall is hearing the computer, not the radio.**
- **Mac: wfview's virtual serial port shows up in the Serial Port list.**
- **Phone on FM: the notch buttons no longer look switched on.**
- **Split puts the transmit VFO in the mode you are operating:** USB or LSB on Phone, CW on CW, the data mode on RTTY
  and PSK. Before, VFO B kept the mode it was last in, usually the DATA-U your last FT8 session left, so Phone could go
  out in DATA-U and CW did not key (reported on an FTDX10). It works on every radio Nexus runs through Hamlib and on
  Nexus's own Icom and Flex connections; OmniRig and TCI cannot set it, so set it on the radio there.
- **Split +/- can no longer move your transmit frequency outside your licence,** and a split from a spot or a
  satellite's Doppler correction is checked the same way. On the FT-991/A, FT-891, FT-710, FTDX10 and FTDX101, each
  press now reaches the radio as you make it.
- **IC-7300, IC-705, IC-7100 and IC-905 through Hamlib: the A/B indicator follows the radio around split,** pressing B
  with split on no longer snaps the radio back to A, and with B selected under split Nexus checks your licence at VFO A,
  where these radios then transmit.
- **PTT NOT ACCEPTED says what to check first,** hovering it shows what the radio or Hamlib answered, and the diagnostic
  log records each refused press. A press the radio refused no longer freezes the scope.
- **A sound card that opens but sends no audio is named and reopened:** the status line shows **NO RECEIVE AUDIO** and
  Nexus reopens the card by itself, never while you are transmitting.

## Remote

- **The connection recovers by itself when your network changes,** usually within two or three seconds, and nothing
  transmits while it does. Both ends need this version.
- **Use less data when a stream goes through the relay,** in Settings ▸ Remote, off unless you turn it on.
- **A guest can no longer use up your relay allowance,** and reconnecting after a network change no longer piles up
  relay permissions.
- **The transmit hold on a device added by signing in is kept whatever the Nexus Remote service says,** until you lift
  it at the shack or its 15 minutes end.
- **The internet connection ignores malformed and replayed connection packets.**
- **A streamed screen can no longer use the station's saved keys,** or change the addresses they go to, while the stream
  runs.
- **A refused Subscribe or Manage subscription says why right under the button.**
- **The Stations on this network window opens in your Nexus language.**

## Logging, callbook and spots

- **Nexus keeps the reports of who heard you,** up to half a million (about 47 MB), and asks PSK Reporter only for new
  ones, catching up on the time you were away. It sends nothing anywhere.
- **POTA/SOTA board: filter by continent and country,** listing only where someone is on the air from right now. Each
  board remembers its own.
- **Pick the island within an IOTA group.** With a group in the IOTA box, an **Island** list offers that group's islands.
  Nothing is picked for you; a picked island is logged and sent with the contact.
- **Logbook: update one contact from the callbook,** from its ⋯ menu.
- **Update from callbook no longer stops when a few calls in a row are not on QRZ.**
- **The N3FJP connection test says what to do when it cannot connect.**
- **Space Wx shows today's sunspot number so far,** next to yesterday's.
- **The WSJT-X UDP address says that each saved profile has its own.**
- **SSTV: the ISS's 70 cm downlink, 437.550 MHz FM, is in the band menu.** Nexus asks before sending a picture there.
- **A sound when a contact is ready to log,** in Settings ▸ Appearance ▸ Accessibility & eyes-free, off unless you turn
  it on; with Auto-log on and the prompt off, it plays when Auto-log logs the contact.
- **The log prompt can be made wider** by dragging its right edge, and remembers the width.
- **Log your Station power when an amplifier Nexus can't read is inline,** per radio, in Settings ▸ Digital.
- **QRZ sync works with a big logbook,** 250 contacts at a time, and carries on where it stopped.
- **Two windows no longer upload the same contacts to LoTW twice,** and several radio windows no longer undo each
  other's changes to the shared radio list.
- **World Radio League: a contact WRL already has counts as uploaded** instead of being sent again.
- **N3FJP: Test tries the host and port you typed,** not the ones last saved.
- **Chase and Chase Feed rows name the frequency and mode to tune to,** not only the band.
- **Linux AppImage: LoTW uploads work again on Fedora,** and links, files and folders open in your own browser and file
  manager.
- **Every installer carries the IOTA island lists.**

## Screens

- **Stop TX stays put** on FT at large text sizes, on a 1080p screen at 150 % when you transmit, and when contest mode
  comes on in French or Russian.
- **Small laptops at large text:** every screen shows a waterfall, CW and Phone no longer scroll, and nothing is out of
  reach.
- **The prompt to log a contact no longer stands over Stop TX,** no longer scrolls sideways, and shows a propagation
  mode's whole name.
- **FT's callsign card keeps the name on one line,** and **⤢** opens the whole card over the screen.
- **On FT, the line with your next over starts at the left again.**
- **The left rail shows more of its sections on a laptop,** and says when the rest are below.
- **The first-run wizard's Next and Finish stay on screen.**
- **The JT65 mode button no longer says receive-only.**
- **Tempo: the waterfall can be dragged taller,** to about half the window at 1080p and two thirds at 1440p and up.
- **Stop TX works when a menu covers only part of it,** and pop-ups and cards keep clear of every stop button (Stop TX,
  Esc/Stop, Abort and SSTV's Stop), an open menu and the control you are using from the keyboard.
- **Phone and CW: the ⋯ panel opens on screen.**
- **Conditions: the Openings box says "No band openings right now."** instead of standing empty.
- **Settings: the LoTW download buttons no longer cover Station Location** in a narrow column.

## Other fixes

- **60 m: the ATU note no longer reads like you may not transmit,** and the Needed view always has a 60 m chip, with the
  band chips in band order.
- **Satellites: the space-weather notes no longer cover the small globe.**
- **Windows: Nexus clears the installer an automatic update leaves in your Temp folder.**
- **Beta testers are offered beta 10 and later;** before, beta 9 was the last one offered.

## Known issues

These have been tested in software or against a simulated radio, not yet on the air. Please tell us how they work on
yours.

- The JTDX-style FT8 and FT4 decoders, and FT8's search for the station in DX Call.
- CW and RTTY FSK on the same serial port as PTT, CW Tune through a WinKeyer or key line, and RTTY Reverse on FSK.
- Field Day answers to a caller with or without a grid, CQs and calls with no grid, and the WSPR and FST4W button lock.
- The listening level's boost with a radio.
- The IC-9700 satellite uplink and the FTX-1's ALC scale.
- wfview's virtual port in the Mac's Serial Port list.
- MSK144's longer over, the JS8 fixes, and Split Operation = Rig outside the ham bands.
- Going to FT8 turning a CW or Phone split off, and a GridTracker or JTAlert click with two radio windows.
- The 15-second Heartbeat and the new window names with JTAlert.

**Over 130 changes since 1.24.0.** The full list is in the CHANGELOG.
