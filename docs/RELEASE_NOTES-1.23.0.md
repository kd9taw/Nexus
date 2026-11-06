# Nexus 1.23.0

CW and Phone can now show the radio's scope and the audio view together, and an Icom's scope draws every sweep it sends.
Every audio waterfall reaches 5 kHz, and every waterfall and scope has a frequency scale along its foot. On a 1080p
screen the parts of Nexus around your decodes and your log take less height, so the band gets it back.

Nexus also keeps your log in more situations. Other programs can log contacts into it: WSJT-X, JTDX, MSHV, JTAlert,
GridTracker, N1MM, anything that sends ADIF over UDP, a FreeDV beside it, and the S.A.T. satellite tracker. Satellite
contacts now go to LoTW with the uplink band, so they can confirm, and one click re-uploads the ones that went out
wrong. "My station" profiles let you pick Home, a portable kit or a club call from the top bar.

The map shows what a flare or a proton event does to your path right now, and you can replay the last six hours. WWFF
joins POTA and SOTA. Nexus is now in Chinese, Simplified and Traditional. The rotator holds on over a slow link and can
follow the Moon.

Many of this release's fixes concern where the radio transmits. Tune, the ATU, an APRS beacon and the voice keyer are
judged where they really go out, and Split Operation = Rig no longer sends them from the last over's TX dial.

## The short version

- **A logger on another computer now needs your say-so** before it can answer a station or turn TX on; see Upgrading, below.
- **Radio scope and audio view together in CW**, with an RF / Audio / Both chip, and one control for the scope's span.
- **An Icom's scope draws every sweep it sends**, and comes back after a power cycle. Not yet confirmed on a real Icom; see
  Scopes and waterfalls, below.
- **Other programs can log contacts into Nexus**, over the contact port.
- **Satellite contacts go to LoTW on the uplink band**, with one click to fix past uploads, and a favourite transponder
  per satellite.
- **"My station" profiles**, picked from the top bar, each able to name its TQSL Station Location.
- **Needle TX meters with peak hold**, if you prefer them to bars.
- **FT drive remembered per band and mode**, on each radio.
- **A dark map, whatever the theme**, with the night side shaded on it.
- **What a flare or proton event does to your path**, a six-hour replay, the polar caps and aurora oval with their
  edges, and NOAA's active regions.
- **WWFF** beside POTA and SOTA.
- **Follow the Moon with your rotator (EME)**, and an LP button on the Rotor box.
- **RTTY at 50 baud with your tone pair saved, PSK63, and 12 or 16 macro keys** on CW, RTTY and PSK.
- **FreeDV beside Nexus:** contacts from FreeDV's own app land in your log, and a Phone pane shows the calls it hears.
  Nexus does not send or receive FreeDV itself.
- **More room on a 1080p screen**, and a better default size on a scaled Windows display.
- **Stop TX and Send stay reachable** on a small window at a high zoom.
- **Nexus in Chinese**, Simplified and Traditional.

## Upgrading from 1.22

### Check this first if a logger on another computer, or through multicast, answers stations

**A double-click in a logger on another computer no longer turns TX on and calls the station. Nexus ignores it unless you
turn on Settings ▸ Services ▸ Integrations & Feeds ▸ Accept requests from other computers, which is off by default.** The
same goes for that computer's free text and grid changes.

- **A logger on another computer needs the switch** to carry on as before.
- **A logger on this computer reached through a multicast address needs it too,** because it answers from this computer's
  network address, not 127.0.0.1. Or leave the switch off and list one 127.0.0.1 address per logger, with commas:
  `127.0.0.1:2237, 127.0.0.1:2238`.
- **A logger on this computer sending from 127.0.0.1 works as it did.**

### Check this first if you use Split Operation = Rig

**With TX off on the FT8, FT4 and FT2 screens, your radio can still be in split. A hand mic or foot switch then keys
VFO B on the last TX dial, and the licence check does not see it.**

Turning TX off used to take the radio out of split. Now it stays in split between overs and with TX off, with VFO B on
the last TX dial, which is your dial moved by a multiple of 500 Hz. After a band change on FT8 or FT4, VFO B is on the
new band's TX dial instead. This is what WSJT-X does, but it is new for Nexus operators.

- It applies once an over or a band change has put the radio in split, which needs a TX offset outside 1500–1999 Hz.
- A transmission you key on the radio itself does not go through Nexus. Nexus does not check it against your licence
  class. With TX off, nothing on screen warns you either; only the diagnostic log records it.
- Before you use a hand mic or foot switch, go to the Phone or CW screen, which takes the radio out of split. Or set
  Split Operation to Fake It or None.
- Keys that go through Nexus are not affected. Tune, CW, PTT, an APRS beacon and the ATU tune-up put VFO B back on your
  dial first, and they key nothing if the radio does not confirm the move.

### Controls that moved

- **RTTY and PSK: Esc/Stop and TX (Continuous) now sit at the right end of the Send row, at every window size.** They
  used to be in the row above, beside Call. They are a little taller, the same size as JTTY's Esc/Stop, and they do what
  they did. On a narrow window they wrap onto their own line. Continuous keys and holds the transmitter, and it is now
  directly beside Send, so look before you click.
- **The eleven mode buttons in the top bar are one button** on a 1080p screen, showing the mode you are on, for example
  "FT8 ▾". The NOW line moves into the top bar. On a screen 2400 or more points wide the bar is as it was.
- **CW and Phone headers are one row:** memories, the rotator, split and REC are behind ⋯. Phone's and CW's meters move
  into the TX strip.
- **The separate span buttons in the Rig scope and Flex pan boxes are gone.** The chips above the scope are now the only
  span control, and every span those buttons offered is a chip.
- **Controls are 30 points tall at Standard density** (form fields, Settings inputs, Logbook buttons, send rows), and
  Logbook rows are closer together. Settings ▸ Appearance ▸ Workspace ▸ Density ▸ Comfortable brings back the old size.

### New names

- **Drive** is the audio level into your radio, everywhere (it was "Tx Power" and "Pwr").
- **RF power** is the radio's own output over CAT, everywhere (it was "Power" and "Transmit power").

### What a scope opens on

- **CW** opens in **Both** on a radio whose scope Nexus draws, at ±25 kHz, with the strip at 40 % of the screen. A span
  or height you already set is kept.
- **The audio CW scope** opens on **Std** and **the audio Phone scope** on **Full**. A chip you already picked is kept.

### Behaviour you may notice

- **A station answering your CQ is worked only when it calls your exact call,** as in WSJT-X. Once a contact is under
  way, nothing changes.
- **Your closing RR73 goes once to a station that has gone quiet,** and with "Disable Tx after sending 73" on, Enable
  Tx goes off after it.
- **Tune, the ATU, an APRS beacon and the voice keyer can now be refused** where they would go out outside your
  privileges. A Tune pressed during an FT over with Rig or Fake It is refused too.
- **PSK's licence check judges the whole signal,** not just its centre. Very near a segment edge, move in a little.
- **One radio, one window.** A radio another Nexus window is running is refused, with a message saying so. The radio
  picker answers itself after 30 seconds.
- **Changing your callsign drops messages waiting to go out** under the old call, and the screen says why. In the FT
  modes the waiting messages are made again under the new call.
- **On a Windows display scaled to 125 % or 150 %,** Nexus opens at 80 % or 70 %. It asks once whether to keep that, and
  Settings ▸ Appearance ▸ UI scale can change it.
- **The contact port is on by default, for this computer only,** at `127.0.0.1:2239`. Nothing sent to it can key or tune
  the radio.
- **Outside the US,** band picks park inside what every IARU region allows, spots are classed by the Region 1 band plan,
  and an amber "Outside band plan" chip appears in Phone. The chip is advice only. Nothing is blocked by it.
- **"New grid" on the roster** counts only a grid the station sent you, or one you logged for it before.

## Operating FT8, FT4 and FT2

- **FT drive is remembered per band and mode, on each radio.** Change band, mode or radio and the drive you last used
  there comes back. It never changes during an over: a band change on the air brings it back as the radio unkeys.
- **The first over on a new band, on a radio slow to take split (FT8 and FT4).** With Split Operation = Rig, the first
  over on a new band had to move VFO B just before keying, and a slow radio had the over refused: "FT8 not keyed: the
  radio did not take the split TX frequency". Now, when the band changes, Nexus puts the new band's TX frequency and mode
  on the radio while you are receiving, whether TX is on or off, as WSJT-X does. It writes nothing while you transmit or
  in the last 4 seconds before a slot boundary, and turning the radio's own dial writes nothing.
  - **Known limit, FT2: it does not get this early write, so a slow radio can still refuse the first over after a band
    change.** FT2 stays in split the same way, but its 3.75 s slot is shorter than the 4 seconds Nexus requires before a
    boundary, so it still writes at the key. If you see the refusal, try Fake It or None, as the message says.
  - **Known limit, FT8 and FT4: a radio slower than about 150–200 ms per write can still refuse the first over after Call
    CQ on a new band.** Call CQ sends the mode again, so the key has to send the TX frequency, mode and split again inside
    its half-second limit. If you see the refusal, TX is turned off and the message suggests Fake It or None. Please tell us
    your radio and what you saw.
  - **Known limit, FT8 and FT4: a band change in the last 4.3 seconds or so before your own slot can start your over late on
    a slow radio.** There is no time for the early write, so the key does it all, and the over stays on the slot clock and
    loses its start. On a serial link we measured about 0.6–1.2 s late; on a slow network link up to about 4 s, where FT4 was
    not decoded at all. On a fast radio it costs nothing you would notice. On these radios the over used to be refused
    outright.
  - **Known limit, rare, changing to FT2 with TX on:** with Split Operation = Rig and a slow radio, changing to FT2 with TX on
    can key a whole FT2 over in the other station's period. Turn TX off before you change to FT2.
  - All of these timings come from a simulated radio, not a real one.
- **A station shown in brackets is the same station as without them** for previous contacts, B4 and the dupe badges.
- **Band Activity and Rx Frequency fill in the periods they missed** when a pane comes back, in italics. They raise no
  alert and change nothing Nexus sends.
- **FST4W lists all of WSJT-X's frequencies,** and picking FST4 from another band lands on a band your radios are set for.
- **Field Day: a reply to your old call still completes the contact** after you add /P.
- **See the FT8 meters larger:** click the small meters cell in the TX strip.

## Scopes and waterfalls

- **CW shows the radio's scope and the audio view together** on an Icom over CI-V USB, a Yaesu over its LAN scope link or a
  FlexRadio. An RF / Audio / Both chip switches views. Phone gets RF / Audio.
- **An Icom's own scope draws every sweep it sends,** on Native Icom CI-V at 115,200 baud or the Icom network connection, so
  the waterfall no longer moves in steps. Settings ▸ Radio ▸ Rig & CAT ▸ Advanced ▸ Radio scope stream shows sweeps a second,
  how many were not drawn, and the radio's scope SPEED, which Nexus never changes.

  **This has only been tested against a simulated radio, not a real Icom. Please check it on yours,** with the radio's scope
  SPEED at Fast, Mid and Slow. Look for the waterfall trailing the radio or jumping, "not drawn" climbing on a computer that
  is not busy, or CW, PTT, frequency or meters slower while the scope runs.
  - *If the scope misbehaves:* Settings ▸ Radio ▸ Rig & CAT ▸ Advanced ▸ **Waterfall source ▸ Audio (sound card)**, then
    Save. The radio stops sending its scope, CAT stays connected, and every waterfall draws the sound card. It is set per
    radio, and **Radio's scope** turns it back on. The Audio chip above the scope only changes what you see.
  - *If keying or CAT still misbehaves on USB:* turn off Native Icom CI-V in the same place. On the Icom network connection
    that switch is always on, so Waterfall source is the way back there.
- **An Icom's scope comes back after you switch the radio off and on,** without restarting Nexus. Needs a check on an
  IC-7300 or IC-9700.
- **A hint when the radio's scope is slow,** below four sweeps a second or with its SPEED on SLOW.
- **Every audio waterfall reaches 5 kHz,** and a wide view keeps a dark background. Decoding and where you can transmit do
  not change.
- **A frequency scale under every waterfall and scope,** with steps that follow the span.
- **The radio's filter edge is marked** on USB, LSB and their data modes, at 1.8 kHz or wider. The CW and Phone scopes
  also say when the filter is all they can show, and how to get the wide view.

## Transmit meters

- **Needle TX meters with peak hold.** Settings ▸ Appearance ▸ Workspace ▸ TX meters picks Bars (still the default) or
  Needle. Needle draws power, ALC and SWR as analog faces, with a peak mark that holds
  the highest reading for a second. It applies on the FT screens and their larger meters window, Phone, CW, Settings ▸
  Radio ▸ Audio, and SSTV, JS8 and Tempo on any window but the smallest. RTTY, PSK and JTTY show no transmit meters.
- **ALC beside the drive** in SSTV, JS8, Tempo and Settings ▸ Radio ▸ Audio, so you can set Drive against the radio's ALC.
- **Buttons look like buttons, and status lights do not,** on every operating screen's transmit strip and the top bar.
- **With an amplifier, a contact logs the amplifier's power,** the highest it reported during the contact. In Standby, it
  logs the radio's own reading.
- **A big Operate/Standby button** in the Amplifier box, showing what the amplifier last reported.

## Small windows

- **The transmit panel folds where it does not fit.** On CW, Phone, RTTY, PSK, JS8 and JTTY, on a small window at a high zoom,
  the macro keys and meters now scroll with the screen and only the row with Send or PTT stays at the bottom, so Stop TX and
  Send stay on screen.
- **The ⊞ Panels menu stays inside the window and never covers Stop TX.**
- **CW's Send no longer runs off a narrow window.**
- **While the panel is folded, Tab keeps the focused key out from behind the bottom row.** JTTY's notice or refusal stays on
  screen in about two lines (hover over it or click it for the whole text), and JS8's heartbeat menu opens in sight.
- **SSTV, RTTY and PSK are laid out like CW and Phone,** and their top bar stays on screen as they scroll.

**Known limits on a small window, while the panel is folded:**

- **A second click on a macro's ✎ pencil can send that macro.** This is on CW, at a window about 800 px wide or less at
  175 %, when the pencil is just under the parked Send row, partly showing. The editor opens out of sight, so nothing seems to
  happen, and a second click at the same spot 500 ms or later can land on **Send once**. Don't click a pencil there twice:
  scroll until it is fully in view. Stop TX and Esc/Stop stay clear, so you can always stop it.
- **The same second click can land on Reset this button,** on CW, RTTY and PSK at similar sizes. It does not ask first, so it
  can erase a customised macro.
- **At the smallest windows a key reached with Tab may be only partly visible.**

## Logging and confirmations

- **Other programs can log contacts into Nexus.** Settings ▸ Radio ▸ Transmit limits & sharing ▸ Accept contacts other
  programs log. Each contact is logged only under your own call, and is not written twice if you also logged it in Nexus
  within five minutes. Turn on "Accept from devices on my network" for something like the S.A.T. tracker. N1MM's edits
  and deletes are not read yet.
- **Companion mode:** a contact WSJT-X or JTDX logs now reaches your log, and Nexus no longer hears its own decodes back.
- **Satellite contacts go to LoTW with the uplink band.** Inside Nexus they still count on their downlink band.
- **Fix satellite uploads (N)** appears beside Upload to LoTW when past uploads went out with the wrong band. Nothing is
  changed or sent until you click.
- **Every satellite LoTW accepts can be picked,** and more satellites are tagged automatically on a pass.
- **A LoTW satellite confirmation lands only on a satellite contact.**
- **"My station" profiles,** picked from your callsign in the top bar, only between contacts. Each can name its TQSL Station
  Location. A profile under another licensee's call does not transmit while a licence class is set.
- **Satellite, Propagation and Park name columns** in the Logbook. Turn them on under Columns.
- **A Start button** on the CW and Phone log line for long contacts.
- **WWFF** spots on the POTA/SOTA board, with hunt, logging, export and the Awards page.
- **FreeDV contacts reach QRZ, Club Log, eQSL and your other services,** and a FreeDV station is its own "New mode" on the
  Needed board.

## Digital modes

- **RTTY at 50 baud, and your RTTY tones saved,** in Settings ▸ Digital ▸ RTTY. 2125/2295 stays the default.
- **PSK63 on the PSK screen,** to receive and transmit, with the same TX switch, licence check, Stop and watchdog as PSK31.
- **12 or 16 macro keys on CW, RTTY and PSK.** 8 stays the default.
- **Type the JS8 offsets exactly,** in Rx and Tx Hz boxes in the header.
- **SSTV:** lost audio no longer shears the picture. It can tune to the band's SSTV frequency when it opens (off by default),
  and remembers "My picture already shows my callsign" per picture.
- **FreeDV pane on the Phone screen:** its last contact and the calls FreeDV 2.4 hears.

## Licence checks and split

- **Tune, the ATU tune-up, an APRS beacon and the voice keyer are judged where they actually transmit.**
- **Split Operation = Rig: Tune, CW, PTT, an APRS beacon and the ATU go out on your dial between FT overs.** RTTY, PSK,
  JTTY, the voice keyer, SSTV and the Remote and computer microphones wait for split to be let go. If the radio does not
  confirm, nothing keys and the status line says why.
- **US General, Advanced and Extra can transmit on 2200 m and 630 m.** The power limits and notifying the Utilities
  Technology Council are yours to handle.
- **60 m in the Phone and CW band pickers,** and every band the pickers list now tunes.
- **60 m below 5.3 MHz, 630 m, 2200 m, 8 m and 5 m are ham bands now,** so an over there is not cut.

## Radios and CAT

- **Two radios in two windows:** one cluster login between them, and the radio picker or a second window opening no
  longer upsets a radio's CAT.
- **NET rigctl to another computer:** Tune and PTT key on time, and a key Nexus gave up on is taken straight back off.
- **OmniRig:** a band change now reaches more radios.
- **Serial handshake = Hardware works** on radios such as the TS-480SAT, and is refused where RTS keys your transmitter.
- **Test CAT's baud search no longer keys an interface that keys on RTS.**
- **An Icom on Native CI-V goes back to the filter you had in each mode.** Needs a check on a real Icom.
- **The Yaesu FTX-1 gets the AGC speed you pick.** Not yet tried on an FTX-1.
- **CW no longer goes out through a keyer you did not pick** when the keyline or WinKeyer port is blank.
- **Flex switches no longer vanish on a Serial connection.**
- **The FT-710 now shows the "CAT CW keying is unproven" notice.** With the CAT keyer on this radio, Nexus can show sending
  with nothing on the air. It is a notice only: the keyer stays selectable and keys as before. If CW doesn't go out, use the
  Serial keyline, WinKeyer or Soundcard keyer.
- **A port name with non-English letters no longer stops Nexus from starting.** A name such as a Cyrillic `СОМ4` in your
  settings crashed Nexus at every launch. Nexus now starts, but the radio will not connect until you pick its real port in
  Settings.
- **A radio pointed at your own rigctld says which setting to change.**
- **The diagnostic log says who keyed your radio, and how.** Nothing about keying changed.

## This computer's microphone and audio

- **PTT never keys the radio if its voice has no way out,** and a held PTT that sent nothing says why.
- **Settings says what the radio must be set to:** SSB MOD SOURCE on REAR (USB), on radios such as the FTDX10.
- **A note beside Tune** for the same setting on the FT-991/991A, FT-710, FTDX10 and FTDX101.
- **Linux:** a headphone DAC named after your radio's codec works with Play receive audio here. Needs a check on a Linux
  station.
- **Windows:** a sound device renamed after a USB change can be picked again in one click.

## Rotator

- **Follow the Moon (EME).** It moves the rotator only: no transmitting, tuning or Doppler.
- **LP on the Rotor box,** beside Point.
- **A slow or lossy link no longer calls the rotator gone,** and ■ STOP no longer queues behind the other boxes.
- **A rotator with an IP address in its Port box reconnects** after the network drops it.
- **A satellite pass takes the rotator back** when it answers again.

## Map and space weather

- **A dark map, whatever the theme,** with the night side shaded, on the Flat map, 2-D Globe and Beam map.
- **What a flare or a proton event does to your path, right now,** with every figure saying where it came from and when.
  They are estimates, and they change no model numbers or logged grades.
- **Replay the last six hours of the sun and a flare,** and see NOAA's active regions on a small sun disc.
- **The polar caps and aurora oval, with their edges,** drawn only from current readings.
- **The Kp says whether it was measured or estimated.**

## Finding stations

- **The Needed board's filters match the Spots board's.**
- **A Park column on the Call Roster,** a colour for each watched call, and right-click on a station to watch or hide it.
- **Sort the stations hearing you** in the Getting Out box.
- **Choose the corner your pop-ups appear in.** In every corner they keep clear of the transmit controls.
- **Downloads are size-limited,** and a garbled NOAA time no longer blanks the Coming card.

## Language

- **Nexus in Chinese, Simplified and Traditional (Taiwan wording).** Every screen is translated. Pick one in Settings ▸
  Appearance ▸ Language. On Linux, Chinese needs a CJK font installed.

## Known issues

These have not been confirmed on a radio, or only against a simulated one. Please tell us how they work on yours.

- An Icom's scope drawing every sweep, and CW keying while it runs.
- An Icom's scope coming back after a power cycle, on an IC-7300 or IC-9700.
- An Icom going back to the filter you had in each mode.
- The FTX-1's AGC speeds.
- Whether CAT CW keys an FT-710 at all. The notice is there because nobody has settled it.
- The early split write after a band change, and every timing quoted for it above.
- A port name with non-English letters on a real Windows port.
- A headphone DAC named after the radio's codec, on a Linux station with both cards plugged in.
- RTTY at 50 baud and PSK63 against real fldigi.

The small-window limits, the split limits and the FT2 limit above are known and still open.

**More than 130 changes since 1.22.0.** The full list is in the CHANGELOG.
