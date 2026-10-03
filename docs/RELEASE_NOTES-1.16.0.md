# Nexus 1.16.0

The headline is Conditions (formerly Connect). It is now a view to keep in sight while you operate,
on a second monitor, a TV or beside the cockpit, and it opens in a new view, Frame + bar: a framed
map with Bands for you, Openings, Chase and Getting Out around it, and a bar across the top with a
big UTC clock and the day's numbers, where Kp, X-ray and the solar wind turn amber in a storm. Bands
for you shows each band as a tile you can read from across the shack. There are new boxes for Spots,
POTA / SOTA and a Clock, Space Wx adds the sunspot number and 30-day trends, the sun and the moon
are on the map, and panes can share a slot as tabs or take a text size of their own. The radio
controls are off Conditions now: you transmit from an operating screen, and Esc still stops transmit
while Conditions is showing.

Transmit works the same way on every screen. TX On/Off, Tune, ATU and Stop TX sit in one strip under
the scope on Phone, CW, RTTY, PSK, JS8, SSTV, Tempo and APRS, in FT's order, so Stop TX is where you
expect it whichever mode you are in. Esc stops transmit on every operating screen and on Satellites,
even with the cursor in a text box. And a message that cannot go out, because TX was off or the dial
was outside your privileges, is dropped instead of going out by itself later, in CW, RTTY, PSK, the
voice keyer, SSTV, APRS and JS8, and the screen says why.

The repeater view, now called Repeaters, is rebuilt around a map: fetch the machines near you, along
a route or on one frequency, and save any of them to Memories with one press. JS8 now works the way
JS8Call does on some forty points. Nexus can look the way you want, with ten ready-made themes, your
own colours, Night for after dark, three text sizes and dividers you can move on every screen. A
file Nexus cannot read is now kept instead of saved over, and the TS-590S, the FTX-1 and a sound
card on a rear data jack each get a fix that matters on the air.

## The short version

- **Conditions (formerly Connect) opens in Frame + bar**, a framed map with a dashboard bar (Kp,
  X-ray and the solar wind, amber in a storm) and a UTC clock. New boxes for Spots, POTA / SOTA, a
  Clock and the band advice as tiles; Space Wx with the sunspot number and 30-day trends; the sun
  and the moon on the map; several panes in one slot as tabs; a text size per pane; a dashboard
  window, and a dashboard rail beside the cockpits.
- **One transmit strip on every screen.** TX On/Off, Tune, ATU and Stop TX sit under the scope, in
  FT's order, and stay reachable at a large text size. ON AIR is a solid red sign, and the top bar's
  TX plate, the amplifier lock and the screen-reader announcements follow every transmission, not
  only FT overs.
- **Esc stops transmit everywhere**: every operating screen, Satellites and Conditions, even with
  the cursor in a text box.
- **A message that can't go out is dropped, never sent later on its own**, in CW, RTTY, PSK, the
  voice keyer, SSTV, APRS and JS8, and the screen says why. An SSTV picture stops the moment its
  transmission stops.
- **Repeaters, rebuilt around a map.** Fetch the machines near you, along a route or on one
  frequency, see them on a map beside the list, and save any of them to Memories with one press. UK
  searches use the RSGB's own list, and more of the hearham directory comes through with the right
  tone, DCS code and narrow setting.
- **JS8 works the way JS8Call does**, on some forty points: replies that ask first and go out in the
  next period, acknowledgements, the heartbeat, the locator, aging, the idle watchdog, and no more
  relaying traffic meant for other stations.
- **Make it yours.** Ten ready-made themes and a System theme, your own colours, Night, three text
  sizes (#215), a touch density, one-tap looks, and dividers you can move on every screen, by mouse
  or keyboard.
- **Your data is safer.** A file Nexus cannot read is kept under a name of its own and never saved
  over, and Nexus tells you where. Two windows sharing one log no longer undo each other's changes.
- **Radios and rotators.** The TS-590S keeps its power (#381), a sound card on the rear data jack
  transmits under CAT PTT (#381), the FTX-1 and other slow radios keep their S-meter and controls
  (#385, #376), and following the radio's own split has a switch at last. The G-5500 and
  PstRotatorAz are in the rotator picker by name, an az/el rotator's elevation is in the Rotor pane,
  and pointing at a callsign aims at the station, not the middle of its country.
- **Also:** a note on each watch-list entry (#390), a distance filter on the Call Roster (#386), a
  switch for the pop-ups (#391), the mouse wheel on Phone's sliders (#384), Wavelog and Cloudlog
  over plain http on your own network (#378), the New York QSO Party, and a POTA park box that fills
  in for the activator you click (#383).

## Upgrading from 1.15

- **Conditions opens once in Frame + bar.** If you had arranged it yourself, your arrangement is
  kept: **Layout ▸ Your earlier layout** brings it back exactly, in one tap. **Standard**, the way
  Connect opened in 1.15, is the first choice in **Layout**, and **Reset layout** gives Standard.
- **Conditions has no radio bar.** Transmit from an operating screen. Esc still stops transmit while
  Conditions is showing, and every other screen has Stop TX.
- **Program is now called Repeaters**, in the same place in the navigation. Your saved channel lists
  and settings are kept.
- **Two new settings in Settings ▸ Radio ▸ Rig & CAT.** *Transmit audio source (CAT PTT)*:
  Front/Mic, the default, keys every radio as before; choose Rear/Data only for a sound card on the
  radio's rear jack. *Follow the radio's split*: off by default.
- **Two Nexus windows on one data folder?** Update both. A window still on 1.15 can still undo the
  other's changes.

## Conditions (formerly Connect)

Conditions is the view the navigation bar called Connect, built to be watched while you operate. It
opens in **Frame + bar**: Bands for you over Openings on the left, Chase over Getting Out on the
right, a full-height map between them, and across the top a bar with your callsign and grid, a big
UTC clock, your local time and the day's SFI, Kp, sunspot number, A, X-ray and solar wind. Kp, X-ray
and the wind turn amber when a storm is on.

- **Layout** in the header picks a ready-made arrangement: Standard, Frame + bar, Map first, List
  first, Dashboard and Frame, which also puts the satellites on the map. **Undo last change** takes
  a pick back.
- **Bands for you** shows each band as a big tile: Open in green, Marginal in amber, Closed in grey,
  with a dot for what you are hearing now, ★ on the best band and a ring on the band your radio is
  on. On 6 m, 4 m and 2 m the tile names the opening (Es, Tropo, Aurora, F2, MS). Hover a tile for
  who hears you and when the band should open or close; click it to show the band on the map.
- **New boxes.** **Spots** and **POTA / SOTA**: a click on a spot, or on HUNT, moves the radio to
  the station and opens its screen, and nothing transmits. A **Clock** with UTC in large digits,
  your local time, and today's sunrise and sunset at your grid. **Space Wx** adds the solar-wind
  speed, the sunspot number, and 30-day lines of the solar flux and the sunspot number. **Getting
  Out** now lists everyone who has heard you in the last half hour, not only the first six.
- **Tabs and text sizes.** Several panes can share one slot as tabs, and take turns in the dashboard
  window and on the TV page. Each pane has its own text size, and its ⋯ menu opens its page in the
  manual.
- **The sun and the moon** are on the map and the globe, the moon in its phase. The **Sun and moon**
  layer turns them off.
- **The dashboard window.** ⧉ Pop out opens a large window with the full layout and the dashboard
  bar, and it comes back where you left it. On Windows, **Stay behind** keeps it behind your other
  windows.
- **A dashboard rail beside the cockpits** keeps the Clock, Bands for you, Space Wx and Getting Out
  in view while you operate. Tick **Dashboard rail** in a cockpit's ⊞ Panels, or press **Dashboard**
  on the NOW bar; it needs a window about 1600 pixels wide.
- **A band says the same thing everywhere.** The map's Band conditions list, the Band Advisor, the
  band menu, the NOW bar and the tiles show the same word and colour, and a band you are hearing now
  is Open, even when the model calls it closed.
- **Clearer names.** The headline box is **Best band**, the card over the map is **Propagation**,
  and the box menus are grouped under Bands, Space weather, Activity and Station.

## Transmit

Stop TX is now in the same place on every screen. TX On/Off, Tune, ATU and Stop TX sit in one strip
under the scope or waterfall on Phone, CW, RTTY, PSK, JS8 and SSTV, in FT's order and at the size
the header's buttons had. On Tempo the strip sits under the Tempo header with Hold Tx, and on APRS
it brings a Stop TX of its own. The strip stays in the window when a large text size makes the
screen scroll.

- **Esc stops transmit** on every operating screen and on Satellites, from anywhere on the screen, a
  text field included. It used to do nothing on Tempo, SSTV, APRS and Satellites, and on Phone it
  stopped only the voice keyer.
- **A message that can't go out is dropped.** When TX was off, for example after you left the screen
  part-way through, or the dial was outside your privileges, CW, RTTY, PSK, the voice keyer, SSTV,
  APRS and JS8 held what was waiting and sent it by itself as soon as transmitting was allowed
  again. It is now dropped, the screen says why, and nothing keys when you come back: send it again.
- **An SSTV picture stops when its transmission stops.** Leaving SSTV for FT8 part-way through a
  picture dropped PTT but kept feeding the picture to the sound card, which put a radio keyed by VOX
  or by data-port audio straight back on the air for up to five minutes.
- **ON AIR is a solid red sign** in every theme, and nothing beside it moves when you key. The top
  bar's TX plate, Operate's ▲ TRANSMITTING, the screen reader's "Transmitting" and the amplifier
  strip's lock now follow every transmission (a Phone over, CW, RTTY, PSK, SSTV, a tune, or the mic
  keyed at the radio), where they followed only FT overs. So the amplifier's band buttons stay
  locked through every over.
- **Stop TX stands out** with a solid red outline in every theme.
- **Phone:** with a button in ⊞ Panels focused, Space presses it instead of keying the transmitter.

## Repeaters

Repeaters is rebuilt around a map and fills the window. Say where in **Near** (your station, a grid
or a city, or **Route to…** for a trip), fetch, and on a wide window the map and the list come up
side by side, each dot linked to its row.

- **Save to Memories** on any FM repeater's row, or **Save all to Memories** for everything the list
  shows: frequency, offset, tone or DCS code, narrow FM, the callsign and the site, with the town
  and the links in the notes. A saved repeater shows **In Memories**, and nothing is saved twice.
- **Along a route.** Repeaters lists the machines within a corridor of the straight line between two
  places, in the order you pass them, with how far along the route each one is. **Add all to channel
  list** makes one CHIRP or CSV list for the drive.
- **On a frequency.** Type 147.18 into the list's filter to see every repeater on that frequency,
  whatever the filters, and never the channel next to it.
- **Links.** A repeater's AllStar, IRLP and DMR numbers and its colour code show under its row and
  go into the exported channel's comment.
- **UK searches** read the RSGB's own repeater list (ukrepeater.net) and merge it with hearham, one
  row per machine, taking the coordinator's tone and input where the two differ.
- **More machines, programmed right.** 514 FM machines whose directory entry also names a digital
  mode were hidden unless the digital filter was on; they are listed now. Narrow machines export as
  NFM, DCS codes are exported (138 machines went out with no code and would not open), a code used
  only on the input is programmed as transmit-only DCS, and 40 machines that write their FM tone
  beside their digital settings get their tone.
- A search no longer says "No FM repeaters" because the list kept on your PC could not be read: it
  fetches the list again, or says what went wrong, with Retry.
- The list's filter clears with one ✕, a place typed into it is offered as a new search, and the
  count line says what the filters hide, with **Show all**.

## JS8

JS8 now behaves like JS8Call on some forty points, so a JS8Call station working you sees what it
expects.

- **Replies.** Each automatic reply asks you first, as JS8Call does out of the box (Yes or No in the
  dock; no answer is No), and goes out in the next period instead of two periods late. With
  AUTOREPLY off, the answer waits in your message box for you to send. Questions sent to @ALLCALL
  get no automatic answer, as in JS8Call, and a HEARING? reply no longer names the station that
  asked.
- **Messages.** A message to you is kept in the Inbox and acknowledged, and so is one a station
  leaves with you for someone else. With RELAY off nothing is held for other stations, and Nexus no
  longer relays, stores or answers traffic addressed to other stations.
- **The heartbeat** goes out once per interval as one frame, on the offset JS8Call would use, and
  one that comes due while TX is off is skipped.
- **The locator.** Nothing is transmitted without a valid locator in Settings, and a six-character
  locator no longer splits a CQ or a heartbeat over two periods.
- **The idle watchdog** holds every automatic reply while it stands, and your next send, switch or
  Inbox action brings back AUTOREPLY, RELAY and HB ACK as Settings has them.
- **Band activity aging is on by default**, at 2 minutes as in JS8Call, so the Band activity pane
  looks different out of the box. Set **Band activity aging** to 0 in Settings ▸ Digital ▸ JS8 to
  keep every row.
- **Also:** JS8Call's allow and deny lists, callsign aging, a GRID button for each station, the
  `<MYGRID4>` and `<MYGRID12>` macros, messages to @APRSIS and @JS8NET, the idle time in STATUS
  replies, and the Band activity pane grouping signals as JS8Call does.

## Make it yours

- **Themes.** Light, Dark and System, which follows your computer, plus ten more: Amber LCD, Green
  LCD, Blue VFD and Silver chassis, and Midnight, Slate, Lagoon, Ember, Nebula and Paper. The scopes
  and meters stay dark, and the transmit red, ON AIR, alerts and the Needed colours are the same in
  every theme.
- **Your own colours** for the accent, the frequency digits, the greens, the ambers and the
  information chips, each checked for contrast. Teal is the green furthest from the reds for a
  red-green colour-blind operator.
- **Night** dims and warms the screen after dark, and **Auto** turns it on at dusk and off at dawn
  at your grid square.
- **Text size**: Normal, Large or Larger (#215), separate from UI scale. **Touch** density gives
  bigger places to tap. **One-tap looks** (Shack, Field / POTA, Night, Contest, Big & clear and
  Touch) set several of these at once.
- **Dividers everywhere.** Drag to resize on Phone, CW, JS8, RTTY, PSK, SSTV, Operate, APRS,
  Conditions, the Logbook, Satellites and Awards, and move any divider from the keyboard. Phone, CW
  and JS8 can also **arrange** their panes, and Phone and CW can show **Spots** and **Needed**
  (#345).
- **Easier reading.** The light theme keeps the frequency readout, meters and scopes dark, and many
  coloured words, chips and buttons are easier to read in the light and the dark theme.
- **Also:** Ctrl+K finds a setting, the Cividis waterfall palette is colour-blind safe, Reduce
  motion turns the animations off, and your backup carries how Nexus looks.

## Your data

- **A file Nexus cannot read is kept.** The QSOs waiting in "Log this QSO?", your Repeaters channel
  lists, the JS8 inbox, Tempo's waiting messages and conversations, the Field Day and contest backup
  and the Assistance record could each be saved over when Nexus could not read them, and a second
  unreadable settings file replaced the first one set aside. Now each is kept under a dated name
  beside it, and Nexus says where.
- **Two windows on one data folder** no longer undo each other's changes or bring back a deleted
  contact. Clear log removes only this window's contacts, and a contact the other window logs while
  this one starts is not missed.
- **Saving in one place no longer undoes a change made in another**, so "Use one radio" stays chosen
  and the radio is not sent back to an old frequency.
- Armed satellite and DXpedition alarms survive a reinstall.

## Radios and rotators

- **The TS-590S keeps its power (#381).** Asking it for its power made Hamlib turn the radio up to
  full power for an instant and leave it at 5 W, on every connect and mode change. Nexus no longer
  asks it, or the other Kenwood, Elecraft, QRP Labs and SDR models whose drivers do the same.
- **A sound card on the rear jack transmits under CAT PTT (#381).** **Transmit audio source (CAT
  PTT)** in Settings ▸ Radio ▸ Rig & CAT is Front/Mic, as before, or Rear/Data, for radios whose
  Hamlib driver can key either input: the Kenwood TS-480, TS-590S, TS-590SG, TS-890S and TS-990S,
  the Yaesu FTDX-5000, the ELAD FDM-DUO and a few more. On an FTDX-5000, Hamlib picks the rear input
  by changing menu 103 and nothing changes it back, so check that menu before you work phone.
- **The FTX-1 and other slow radios** keep their S-meter and receive controls (#385, #376). On the
  FTX-1 Nexus no longer sends the three commands its Hamlib driver gets wrong: there is no ATU
  button (press TUNE on the radio), and the monitor is set on the radio (#385).
- **Follow the radio's split** is a switch in Settings ▸ Radio ▸ Rig & CAT, off by default, and
  Settings names the seven Rig & CAT settings shared by all your radios.
- **Rotators.** The Yaesu G-5500 and PstRotatorAz are in the picker by name. The Rotor pane shows
  and sets an az/el rotator's elevation, and turning the beam keeps it where it is. Pointing at a
  callsign aims at the station, not the middle of its country, LP turns the long way (#338), a USB
  rotator controller switched on after Nexus comes online by itself, and the rotator strip is on
  RTTY, PSK, SSTV and JS8 too.

## Logging, spots and contests

- **Watch-list notes (#390)**, a **distance filter on the Call Roster (#386)**, and a switch for the
  **pop-ups** in the bottom-right corner (#391). Errors and transmit notices always pop up, and the
  sounds are unchanged.
- **The park box (#383)** fills in for the POTA or SOTA activator you click and clears once the
  contact is logged, so a park is never logged on someone else's contact.
- **Wavelog and Cloudlog** on your own network can take uploads over plain `http://` (#378), with a
  warning that your API key travels unencrypted there.
- **The New York QSO Party**, with the NYQP committee's 2026 rules, county names or codes, and the
  Cabrillo the sponsor asks for.
- **A WSPR beacon** is spotted on the frequency you set, not 2.2 Hz higher.
- **The mouse wheel** moves the Phone cockpit's sliders (#384), and nothing done with the wheel keys
  the transmitter.

## Remote

- **Running the shack over Parsec?** A new switch in Settings ▸ Radio ▸ Transmit limits & sharing
  (Windows) stops a latched PTT, continuous RTTY or PSK, and Tune if your Parsec session drops. It
  only ever stops.
- **Listen** plays the station's audio on the Remote site, which it had not done since it arrived in
  1.13.0.
- **Streaming the Nexus window to a browser** is new, marked **Beta** and off by default (Windows;
  Settings ▸ Station ▸ Remote access). It connects from another network, but there is no relay for
  the picture yet, so from the shack's own network it connects only on routers that loop traffic
  back to their own public address, which many home routers don't. A fix is planned for 1.16.1.
  While streaming is in Beta, the Remote site's old watch-and-control page is hidden; Listen stays.

## Screen and setup

- On a small window the top bar takes two lines instead of four.
- Operate, Tempo and Conditions stay usable at a large zoom on a small screen: Operate's decode
  panes keep their rows and its QSO strip stays reachable, Tempo's panes no longer sit on top of
  each other, and Conditions' side panes keep their titles.
- The Globe map draws only what is on your side of the planet.
- Space Wx says how old its solar-wind reading is, and never reports a wind of 0 km/s.
- The waterfall's RX and TX markers show how wide the signal is, as WSJT-X's Wide Graph does.
- Most settings changes no longer make the radio wait while the settings file is written.
- One update prompt instead of two, and **Check for updates** in Settings downloads the update and
  offers to install it.
- Settings' list fields take commas, and the quick-reply chips take spaces (#370).
- Nexus does less work while nothing is happening.

## Known issues

- At a large pinned text size (150–175 % on a 1024×768 or 1366×768 screen), FT's own transmit strip
  can start just below the window; scroll a little to reach it. A fix is planned for 1.16.1.

## Corrections to the 1.15.0 notes

- **#174** was credited as keeping every new-one alert to the continents and countries you pick. It
  covers the alerts raised from your own decodes: new-one, new-grid and CQ. The **Pounce** new-one
  alert works from cluster and RBN spots and still sounds for a spot from anywhere, because it has
  no "spotted from" filter yet.

## Credits

Repeater data: hearham.com, and RSGB ETCC (ukrepeater.net) for UK searches.

**124 changes since 1.15.0.** The full list is in the CHANGELOG.
