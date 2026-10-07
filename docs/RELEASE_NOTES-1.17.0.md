# Nexus 1.17.0

The headline is chasing states through POTA and SOTA, and it came from a user's request. Every
activator on the POTA/SOTA board now shows the state or province they are in, ahead of the park or
summit name, and a state you still need for WAS on that band is lit, so a needed ND or MT jumps out.
A park on a state line shows both states and asks which one when you log, a SOTA summit shows its
state (W7M is MT), and the Needed board and Pounce now flag a state you still need. A contact you
hunt now logs the park's state, not the activator's home state, so your WAS credit lands where it
should. Your own activations record the park's state as MY_STATE, and **Check park states** in the
Logbook finds older park contacts logged with the wrong state and fixes only the ones you tick.

The map is sharp at every zoom, on the flat map and the 3-D globe, and the globe shows the Earth as
NASA photographed it, with the city lights coming on where the sun has set. Conditions has a fifth
map, **Street**: download a street-level map of your area once, and your paths, the greyline,
satellites and grid squares draw on it. On the radio side, the five digital cockpits can show the
radio's own scope as a pane, the Phone and CW scopes tag the spots on your band and shade what your
licence class may not transmit, you can drag the filter's edges on the scope, and Phone has a
full-height left side for Band Activity, Spots and Needed.

On Windows, Nexus no longer asks for administrator rights by itself: a **Repair clock** button
appears only when the clock has a real fault, and asks only when you press it. Distances follow your
Units setting everywhere. A radio that refuses to key no longer has the over played into it, and
FT8, FT4 and JS8 turn TX off when the radio refuses PTT, as WSJT-X does. Remote can run over your
own network with no internet (Windows), streaming now works from the shack's own network, and on a
phone the stream has a layout of its own.

## The short version

- **POTA/SOTA states for WAS.** The board shows each activator's state or province, lit when you
  still need it on that band. A park on a state line asks which state when you log, a hunted contact
  logs the park's state, your activations record MY_STATE, and **Check park states** fixes older
  contacts. The Needed board and Pounce flag a state you still need.
- **A sharper map** at every zoom, and a 3-D globe with NASA's Blue Marble by day and the city
  lights by night.
- **Street**, a street-level map of your area, as a fifth map choice in Conditions, downloaded only
  when you ask. Map data © OpenStreetMap contributors (ODbL).
- **The radio's own scope as a pane** in FT8, JS8, RTTY, PSK and SSTV, off until you tick it.
- **Phone and CW scopes:** spot tags you can click to work the station, a tint where your licence
  class may not transmit, filter edges you can drag, and display settings under ⚙.
- **Phone:** a full-height left side for Band Activity, Spots and Needed.
- **No surprise administrator prompts on Windows.** **Repair clock** appears only for a real clock
  fault, asks only when you press it, and nothing transmits while it runs.
- **Distances in your units everywhere:** Getting Out, the openings, DXpeditions, satellites, the
  map, Journey and the rest follow **Units** in Settings ▸ Station.
- **Safer keying.** When the radio refuses to key, nothing is played into it and Nexus says so. FT8,
  FT4, JS8 and the other timed modes then turn TX off, as WSJT-X does.
- **Remote:** over your own network with no internet (Windows, new), streaming from the shack's own
  network, a relay where a direct connection fails, a phone layout, a sharper and steadier picture,
  Listen with a volume control, a Mic level, and a stream that proves it comes from your station.
- **Also:** Wavelog says why it refused a QSO, the Conditions pop-out keeps its own map, the 3-D
  globes recover after sleep, and a Flex native client to try (Beta).

## Upgrading from 1.16

- **Windows: no administrator prompt at start.** Nexus still checks the clock and says what it found
  (hover the clock readout). **Repair clock** shows only for a real fault and asks for administrator
  rights only when you press it. While a repair runs, two minutes at most, nothing starts
  transmitting. Nexus no longer offers to change how often Windows checks the time.
- **When the radio refuses PTT in FT8, FT4, JS8 and the other timed modes,** the over is not sent
  and TX turns off, and the status bar shows **PTT REFUSED** with the time and what the radio
  answered. Check your PTT method and CAT port, then turn TX on again.
- **The Phone scope now smooths over 250 ms out of the box.** Set ⚙ ▸ **Smooth** to Off to draw
  every sweep as it arrives.
- **The satellite layer**, with no ★ birds picked, shows the satellites you can work instead of the
  whole catalog of about 350. Pick **All** for every one.
- **Remote streaming: update Nexus at the shack.** The Remote page takes a stream only from a
  station that signs it, so a shack still on 1.16 is refused, with a note saying to update it. A
  browser approved before browser keys existed is asked for again at the shack before it can take
  control.
- **Flex native DAX audio** now needs the Flex native client (Beta). On SmartSDR CAT the switch does
  nothing and the audio stays on SmartSDR's DAX devices. Your setting is kept.
- **Contacts already in your log keep their state.** Run **Check park states** in the Logbook to see
  the park and summit contacts that hold another state, and change only the ones you tick.

## POTA and SOTA

- **The state on every activator.** For a park the state comes from pota.app's own spot. A park on a
  state line shows each state, as MT·ND, and a long trail shows two and how many more. A summit
  takes its state from its SOTA association and region, and a Canadian park shows its province. A US
  state your log does not yet hold on that spot's band is lit in the New State colour, by the same
  rule Band Activity uses. Hover the state for its name and, on a state-line park, a reminder that
  the contact counts for the state the activator is actually in.
- **The right state in your log.** A contact you hunt, from the POTA/SOTA board, the map, the Needed
  board or Band Activity, takes the state the park or summit is in, and the log form shows it in the
  State box before you log. A state you type yourself still wins. While the log form holds a park on
  a state line, it offers that park's states: pick the one the activator says they are in. A contact
  logged without a pick has no state, and the Logbook marks it with a **?** before the park so you
  can set it later.
- **Your own activations.** Start an activation at a park in one state, or on a summit SOTA places
  in one, and every contact you log carries that state as MY_STATE, in your log, your exports and
  your uploads. At a park on a state line the activation strip asks which state you are in. LoTW
  credits the state of the TQSL Station Location you sign with, so sign an activation's contacts
  with a location in that state. Not yet checked: how TQSL treats a MY_STATE that differs from the
  Station Location you sign with.
- **Check park states** in the Logbook lists every park or summit contact whose park is in one state
  but which holds another state, or none, with the old state beside the park's. Tick the ones to
  change and press the button. A confirmed contact starts unticked, and nothing is uploaded again.
- **The Needed board and Pounce.** An activator in a state you still need shows as a new state on
  the Needed board, whether it was heard from the POTA/SOTA feed, the cluster or your own radio, and
  the STATE chip says where its state came from. Pounce's "New entity, zone, or US state" alert now
  fires for a US state you still need. Alaska and Hawaii stations on cluster and PSK Reporter rows
  are placed in AK and HI, and with **Confirmation opportunities** off, POTA/SOTA rows no longer
  show a Confirm.
- **The greens.** Every POTA activator gets a dim POTA green in the Phone and CW cockpits' Band
  activity and band map, and a park you still need keeps the full green; the colour key names both.
  A park you still need comes before a confirmation everywhere, a needed park calling CQ wears the
  needed-park colour in Band Activity, and FT4's badge on the Needed board is mint in the dark
  themes, so it no longer matches the park green. The park reference, NEW PARK and the Hunting line
  read clearly in the dark themes.

## Maps

- **Sharp at every zoom.** Coastlines, lakes, the major rivers, country borders and US state lines
  stay crisp from the whole world down to a single state, on the flat map and both 3-D globes. The
  flat map lays shaded relief under them, and its lighter look makes the greyline's day and night
  easy to tell apart. The detailed map loads only when you zoom in, so the map opens as quickly as
  before, and a built-in dark theme keeps its own map colours.
- **The globe as NASA photographed it.** The 3-D globe and the Logbook's globe show NASA's Blue
  Marble by day and its Black Marble by night, meeting at the greyline where it is right now, with
  your layers and the Logbook's QSO dots drawn on top. With City lights turned off, the night side
  is dark.
- **Street.** **Street** joins Globe, 3D, Flat and Beam in the Conditions map picker. Press it to
  download an area: around your station or the map's centre, a square 50 to 400 km across, with all
  streets or main roads, and the exact size and your free disk space shown before anything
  downloads. The map comes from maps.hamradiotools.io, only when you ask, and is kept on your
  computer. The download carries on while you use Nexus and picks up after a restart. Your paths,
  the greyline, satellites and grid squares draw on it, and from city zoom a station known only by
  its grid square shows as that square. **Settings ▸ Appearance ▸ Map & globe ▸ Street maps** lists
  your areas, checks for newer map data when you ask, and removes one. Map data © OpenStreetMap
  contributors (ODbL).
- **Satellites.** With no ★ birds picked, the map shows the satellites with a transponder, an FM
  repeater or a digital channel, not every beacon. Star the birds you want, or pick **All**.
- **Conditions** keeps the pop-out's own map and ★ / All when you reopen it, and says so when the
  3-D globe can't draw on your computer and Globe stands in. A closed 3-D globe gives back its
  graphics memory, a globe the graphics card dropped after sleep comes back (or offers **Reload**),
  and the Logbook's globe shows day and night every time it opens.

## Scopes and the Phone screen

- **The radio's own scope as a pane.** ⊞ Panels has a new **RF scope** entry in FT8, JS8, RTTY, PSK
  and SSTV, off until you tick it. It shows an Icom's scope over CI-V at 115200 baud, or a
  FlexRadio's with the native panadapter on, and the audio waterfall stays where it was. It only
  shows the band: a click does not tune. In FT8 and FT4 it marks the last slot's decodes and your RX
  and TX offsets. Not yet checked on a radio.
- **Spots and your licence edges on the scope.** The Phone and CW scopes tag the spots on your band
  at their frequencies, in Band Activity's colours, and a click on a tag works the station. Every
  scope tints the frequencies your licence class may not transmit the current mode on; 🔒 TX LOCKED
  still decides. Not yet checked on a radio.
- **The filter on the scope.** Where the radio reports its filter width, the scope shades the
  passband, and you can drag its edges to set the width (the far edge on SSB, both edges on CW); the
  radio follows within a second or two. With the scope focused, ← and → tune and [ and ] narrow and
  widen the filter. On an IC-7300, IC-705, IC-905, IC-7610 or IC-9700 on Nexus's own CI-V
  connection, the width is now read from the radio and set on it. Checked on an FTDX10 through
  Hamlib and an IC-9700 on Nexus's own CI-V; not yet on the other Icoms.
- **Display settings under ⚙** on the Phone and CW scope: Resolution, Smooth, the detector (Peak or
  Avg), and Smooth scroll or Row per sweep for a radio's slow scope, kept for each cockpit.
- **The CW scope's frequency scale** matches the spot tags and where a click tunes, instead of
  reading the CW pitch (600 Hz by default) off. It runs the right way round on CW-R and reads to 100
  Hz on its narrow window.
- **Phone's left side.** ⊞ Panels ▸ Arrange ▸ **Left side** puts Band Activity, Spots or Needed in a
  column the full height of the window, beside the scope, on a window about 1280 pixels wide or
  wider. PTT, Tune and Stop TX stay where they are.

## Transmit and the clock

- **Repair clock (Windows).** The clock check used to run its own repair, so a prompt to let
  "Windows Command Processor" make changes could appear a few seconds after Nexus started, with
  nobody having asked. Now **Repair clock** appears beside the clock readout only for a real fault
  (the Windows Time service off, not synchronised, or a clock that just jumped), and the repair runs
  only when you press it. It is refused while you transmit or while a JS8 or Tempo message is
  part-way through, and nothing starts transmitting until it finishes, so the clock never moves in
  the middle of an over.
- **When the radio refuses to key, nothing is played into it.** Tune, an APRS packet, the voice
  keyer, SSTV, soundcard CW, RTTY, PSK and your voice from the Remote stream used to go into a radio
  that was still receiving; nothing went on the air. Now none of them plays, Nexus says the radio
  did not accept PTT, and what was refused is dropped, never sent later. A radio that is only slow
  to answer still sends its over, with a warning. Checked on an FTDX10 through Hamlib and an IC-9700
  on Nexus's own CI-V; not yet checked on a slow serial CAT rig or a Flex radio.
- **FT8, FT4, JS8 and the other timed modes stop TX as WSJT-X does** when the radio refuses PTT on
  or PTT off, and the status bar says so until you turn TX on again. Not yet checked with a logger's
  Halt TX, serial (RTS/DTR) PTT, a Flex radio or a slow serial CAT rig.
- **RTTY and PSK macros:** a key caption that reads as a stop, halt, abort, cancel or Esc is refused
  in English, German, Spanish, French and Japanese, whichever language the screen is in, so a key
  that transmits never looks like the one that stops.

## Remote

- **Remote over this network (Windows).** Operate the station from Nexus on another computer on the
  same network, with no internet. At the station, turn on **Remote over this network** in Settings ▸
  Station ▸ Remote access and press **Pair a computer**. On the other computer, **Stations on this
  network…** finds the station by name, or takes its address, and pairs with the code the station
  shows. The code works once, within ten minutes, and whoever types it in time can operate the
  station, transmit included, so keep it to yourself. The paired computer streams the station in a
  window of its own and keeps access until you remove it at the station. Both computers need Windows
  for now.
- **Streaming from the shack's own network works.** A browser on the shack's Wi-Fi or wired network
  now connects straight to the shack. The shack shares only the one private address it streams from,
  and only with a browser it has admitted.
- **A relay** carries the stream, still encrypted, where a direct connection fails, as on some
  mobile and office networks. A direct connection is always tried first.
- **On a phone** the stream has a layout of its own, with Stop TX first and largest, a big Hold PTT,
  Mic, Listen and a **Keyboard** that types into Nexus at the shack. Two fingers zoom the picture
  instead of pressing anything at the shack, and the page can go on your home screen.
- **The picture** stays sharp on a large shack window, stays live on a slow connection, and reaches
  a browser when the shack has a public internet address of its own.
- **Listen and the mic.** Listen no longer breaks into bursts of static, plays loud enough to hear,
  and has a **Volume** slider. A **Mic level** raises a quiet microphone, and the microphone message
  names the real cause when the browser can't use it.
- **Stopping.** Esc is Stop TX anywhere on the stream page, **Full screen** keeps Stop TX on screen,
  and leaving full screen any other way than its own button sends Stop TX. Stop TX from the browser
  works right after another stop, where it was refused for about a second, and a held PTT no longer
  lets go by itself mid-over.
- **Security.** Nexus at the shack signs every stream, the browser signs taking control, Listen and
  every command with its own key, your browser keeps your station's key and warns if it ever
  changes, and the Remote page opens only over https://. Keys now show as eight groups of four
  characters at both ends.
- **Remote stations window (Windows).** Settings ▸ Station ▸ Remote access ▸ **Remote stations…**
  opens the Remote page in a Nexus window of its own. F11 is full screen, and Esc over the picture
  still stops transmitting.
- Streaming is still in Beta and off by default (Windows). The older watch-and-control page stays
  hidden while it is; its POTA/SOTA board shows the states too.

## FlexRadio

- **Flex native client (Beta).** With this switch in Settings ▸ Radio ▸ Rig & CAT, Nexus connects to
  a FlexRadio itself, as one of its SmartSDR clients, and run CAT and PTT on a slice of its own. It
  is off until you turn it on, and falls back to SmartSDR CAT if it cannot connect. Not yet checked
  on a real Flex: try transmit into a dummy load first.
- **Native DAX audio (Beta)** now works through the native client: receive audio comes straight from
  the radio, the digital modes transmit over DAX, and Phone keeps the radio's own mic. Before you
  use it for FT8, check your own signal's DT on a second receiver.
- **A "may still be transmitting" alarm** stays on screen, in an amber banner with the radio's name
  and the time, until you dismiss it. Not yet checked on a Flex radio.
- **The native panadapter (Beta)** draws the right way up, at its full height. Not yet checked on a
  radio.

## Logging

- **Wavelog and Cloudlog:** a refused QSO now shows Wavelog's own reason, such as a duplicate, and
  Settings ▸ Logging & Connectors warns when the station location you pick has a callsign other than
  yours or a different grid.
- **Licenses.** Every installer now carries the license texts of the packages Nexus is built from,
  and the **Licenses** button in the Settings header shows them.

## Known issues

- At a large pinned text size (150 to 175 % on a 1024×768 or 1366×768 screen), FT's own transmit
  strip can start just below the window; scroll a little to reach it. It is not fixed in 1.17.0.

## Corrections to the 1.16.0 notes

- The 1.16.0 notes said two fixes were planned for 1.16.1. There was no 1.16.1. Streaming from the
  shack's own network is fixed in 1.17.0; FT's transmit strip at a large text size is not fixed yet
  (see Known issues).

## Credits

Map geography and shaded relief: Natural Earth. Globe pictures: NASA Earth Observatory (Blue Marble:
Next Generation by Reto Stöckli; Black Marble images by Joshua Stevens, using Suomi NPP VIIRS data
from Miguel Román, NASA GSFC). Street map: © OpenStreetMap contributors (ODbL), from a Protomaps
basemap build. The Flex native client is ported from AetherSDR.

**67 changes since 1.16.0.** The full list is in the CHANGELOG.
