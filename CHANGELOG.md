# Changelog

All notable changes to Nexus (formerly Tempo) are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

## [Unreleased]

### APRS says which radio it is listening to

If more than one of your radios covers the APRS band, the decode readout now names the one it is
actually listening to — "on FT-991A" — and its tooltip explains that APRS follows the active radio
and that routing rules decide which radio a band goes to.

This is the missing piece behind the bug below. When the app picked the other radio, the only
symptom was silence, and nothing on screen said which radio was being listened to — so a working
station looked like a dead band. With the radio named, that is a glance instead of an afternoon.

On a single-radio station, or when only one radio covers the band, nothing is shown: there was no
choice to make and saying so would just be clutter.

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

### Fixed: tuning a repeater could pick the wrong radio, and the wrong mode

Program's per-repeater Tune button now tunes in a single step that knows it is FM.

Two things were wrong before, both only visible on a multi-radio station or when you had been
operating something other than voice. The tune announced no mode intent, so with a routing rule
sending 2 m FM to one radio and 2 m digital to another, tuning a 2 m repeater while the app was in
FT8 handed the frequency to the FT8 radio. And because the rig's mode follows whichever section you
last operated, the same tune could leave the radio in a data mode, where a repeater is inaudible.

The Tune button now names FM explicitly for both decisions, so the machine's frequency, shift, offset
and tone all land together on the radio you mapped for FM. It does not move you out of Program or
arm transmit; it puts the radio on the repeater so you can listen. Any later retune, section change,
radio switch, or a turn of the VFO knob down to HF releases the FM hold, so FM never follows you
somewhere it does not belong.
### Stations can be kept on the APRS map forever

Setting "Keep stations for" to 0 now means exactly that: no fade, no removal — every station
stays until the 2000-station ceiling. Added while chasing a field report of stations vanishing
far too quickly; with removal off entirely, anything that still disappears proves the fault is
somewhere else. It stays because some operators genuinely want an all-day picture.

### Fixed: APRS went silent on a three-radio station

0.21.4 could send an APRS activation to the wrong radio. With three radios where two of them cover
2 m, nothing in the app had a reason to prefer one over the other, and the one it picked changed in
0.21.4 — so a station whose APRS audio was set up on one rig suddenly found the app listening to
the other. Audio configured for a different mode means silence, and nothing on screen said which
radio was being listened to.

A tie between two equally capable radios now goes to the radio you nominated as your default. If
you have not set one, the choice stays consistent rather than arbitrary, and a routing rule still
overrides everything — a rule for `2m` + `FM` pointing at a specific radio is the way to say this
unambiguously.

### Fixed: APRS stations disappearing off the map again

A tester reported real off-air decodes vanishing from the APRS map within seconds, while stations
coming from the internet feed looked fine. This was our own doing, and it was introduced by the
previous fix for the flashing icons.

That fix was right that the whole map was being redrawn every two seconds whether or not anything
had changed. It was wrong that the redraw was merely wasteful. The map is a canvas — whatever was
painted stays painted until something repaints it — and on the APRS screen that two-second redraw
was the **only** thing repainting it. The next scheduled redraw after it was removed could be up to
a **minute** away, so a station could sit invisible until the clock came round. Internet stations
escaped it because the feed changes the data constantly, which forces a redraw of its own; a quiet
2 m channel with a handful of decodes does not.

The APRS map now keeps its own repaint schedule instead of relying on a side effect, and a station's
fade is measured against the actual time rather than a clock that only advanced once a minute — so a
decode from five seconds ago no longer draws as if it were a minute old.

**Station retention itself was never at fault.** An hour's retention, the twenty-minute fade, the
timestamps on off-air decodes, and the behaviour of an upgraded settings file were all measured
directly and all behave as designed.

### APRS packet decoder measured against tone imbalance ("twist")

Twist — the two packet tones arriving at unequal volume, the net effect of the sending TNC, the
transmitter's deviation and the receiver's audio shaping — is the classic reason packet decoders
struggle on real signals. Nothing in our test suite had ever modelled it, because the suite
generated its own perfectly balanced tones.

It now does, and the decoder came out clean: packets still decode with the tones up to 24 dB apart,
far beyond the roughly 9 dB that real signals show. That is a property of how this decoder works
rather than luck — it compares the two tones against each other within each bit, so making one
quieter scales both sides of the comparison equally. What twist does cost is noise margin on the
quieter tone: about 4 dB of it at the realistic worst case, leaving decodes working down to 9 dB
signal-to-noise. Comfortably clear of any packet you can actually hear.

The WAV analysis tool now reports the measured twist of each burst, so a real recording will say
what the local digipeater's signal actually looks like.

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
not accept that frequency, and stops asking after a few tries instead of hammering the link.

**Rig control recovers on its own.** Nexus stops polling a radio that has stopped answering, which
is right — but that state used to be permanent, so any hiccup meant no rig control until you
restarted. It now retries quietly, backing off to about once every thirty seconds, and picks the
radio back up within a couple of seconds of it answering again. This one is not specific to APRS:
anything that interrupted the link used to cost you rig control for the rest of the session.

**In the cockpit**, an HF-only station now reads *"No 2 m radio"* with an explanation, instead of a
Tune button that could only ever fail. The internet feed is genuinely useful without a VHF radio —
it shows APRS traffic other stations have reported — so the view tells you that rather than
looking broken.

### Click an APRS station for everything known about it

Clicking a station used to highlight it and nothing else. It now opens a detail card, from either the
map or the list:

- The symbol at readable size, with what it actually **means** in words.
- **How it reached you, per source, with separate ages** — "your receiver decoded this station
  4 min ago; the internet feed reported it 20 s ago". Those are two different facts and only one of
  them says anything about your antenna, so they are never merged into a single "last heard".
- Position with grid square, and distance and bearing from your station.
- Course, speed and altitude when the station is moving.
- The comment text, the digipeater path, and whether the packet reached you **direct or digipeated**.
- The raw packet, collapsed until you want it.
- One click to QRZ, or to the station's page on aprs.fi.

### Weather stations now report the weather

A weather station's readings were arriving and being shown as the raw field string —
`220/004g011t085r000p000P000h68b10156`. Nexus now reads it: temperature, wind direction and speed,
gusts, rainfall, humidity and barometric pressure, in the station's detail card.

A sensor a station does not have is left out rather than shown as zero. `r...` on the wire means "no
rain gauge fitted", not "no rain", and reporting 0.00 in would be inventing a measurement.

### The internet feed switches off from the APRS screen

Turning the feed off meant a trip to Settings. The internet status chip on the APRS board is now
also its control: click it for the feed switch, the range radius, and your watched callsigns. The
radius is there because the chip's own advice when the feed goes quiet is "widen the radius" — the
control belongs where the advice is.

Server, port, which kinds of traffic to subscribe to, how long stations are remembered, and the
receive-only iGate stay in **Settings ▸ Modes ▸ APRS**. Those are set once. The iGate especially:
contributing to a global network under your callsign should be a considered decision, not something
a stray click on a cockpit can start.

Both places edit the same settings, so they can never disagree about whether the feed is on.

### Fixed: APRS map icons flashed on and off

With the internet feed running, stations blinked in and out constantly. Two separate faults, both
fixed.

The map was built on the **last 300 packets** rather than on stations. Three hundred packets is two
to five minutes of a busy feed, so a station beaconing on a perfectly ordinary ten-minute cycle was
pushed out before its next beacon — it disappeared, came back, disappeared again. The map now keeps
**stations**, with their own history: last position, when each was last heard by your radio and by
the internet, symbol, course and speed. A station stays for an hour after its last packet and starts
to fade after twenty minutes of silence, so a quiet station recedes instead of vanishing. You can
change the hour in **Settings ▸ Modes ▸ APRS**.

Separately, the whole map was being torn down and repainted **every two seconds** whether anything
had changed or not. That alone made icons flicker even for stations that never went away. The map now
repaints only when something has actually moved, arrived, or aged.

### APRS stations are coloured by what they are

Symbols now carry a colour for their family: homes and portable stations, vehicles, aircraft, boats,
weather stations, digipeaters and gateways, and hand-placed objects. Colour says what a station *is*
— nothing here means urgency — and it is independent of the ring that tells you whether your own
antenna heard it, so the two never compete. The palette varies brightness as well as hue so the
families stay apart for colourblind operators, and it has a separate version for the light theme.

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
on with the address field empty and Nexus fills in the usual local target for you.

The address field now also states which of the two it is doing, so a configured-but-silent output
can never look like a working one again.

It is off after an upgrade, and nothing but that switch can turn it on — your contacts do not
start going out over the network because you installed a new version.

Field Day is untouched. During an event, contest contacts still go out the way they always have,
carrying your class, section and points; the standing broadcast only ever carries the contacts in
your regular log. A contact is never sent twice, so it is safe to leave the switch on through a
Field Day weekend. An ordinary QSO carries what a map needs — call, grid, band, frequency, mode,
time — and honestly claims no contest points.

If you run several consumers on one machine, name the port. 12060 is often already taken (HRD
listens there), and the port you type is the port that is used.

### Route each mode to the radio that does it best

Nexus already handed a band to the radio configured for it: pick 2 m and it switched to your VHF
rig. But a band is not fine enough. If you have a 2 m/70 cm rig for weak-signal digital and a
different rig for FM and APRS, both of them cover 2 m — and Nexus had no way to tell them apart,
so a 2 m FT8 spot and an APRS tune went to whichever radio it happened to pick first.

You can now route on the band **and the mode**. In **Settings ▸ Radio** there is a routing table
under your radios: pick a set of bands, pick a mode class, pick the radio. Rules are checked top to
bottom and the first match wins, so a specific rule above a broad one takes precedence — and the
arrows beside each rule let you reorder them. Anything no rule matches falls back to the band
coverage you already set on each radio, and then to a default radio you can nominate for
everything else.

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

### A third radio now works properly

Two radios worked. A third did not, for a reason that only ever shows up at three: each radio's
window keeps its own settings file, seeded once from the shared one the first time that window
opens. With two radios you always add the second one before those per-window files exist, so both
windows learn about both radios. The third radio is the first one you add *after* they exist — so
it landed in exactly one window's settings and nowhere else. The launch picker (which reads the
shared file) never offered it, the other window never monitored it, and there was no way to repair
it from inside the app.

Adding or removing a radio now updates the shared config too, and every window picks up radios
added elsewhere when it starts. The routing table is shared the same way, since which rig does 2 m
FM is a decision about your station, not about one window.

Three smaller things that also only bite at three radios: a band claimed by two rigs now always
goes to the same one (it used to depend on the order they happened to sit in the list); adding a
radio after removing one no longer produces two radios with the same name, which made the port and
audio conflict warnings ambiguous; and a window launched pointing at a radio that no longer exists
now says so instead of quietly driving the first radio's serial port — which is the port another
window is already using.

### APRS decode readout stops mixing up "now" with "a while ago"

The new input-level reading immediately caught a sentence that contradicted itself: *"2 packets
were heard but none passed the checksum... peak -99 dBFS."* Nothing is heard at -99 dBFS. The
packet counts run from the moment you arm the decoder, while the level is whatever the radio is
doing this instant — so two candidates from six minutes ago sat there asserting something about
the present, right beside a live reading that flatly disagreed.

Every claim now says when it was true. A failed-checksum count only speaks in the present tense
while bursts are still arriving (within the last minute); after that it steps aside and the
readout goes back to describing what the radio is actually doing. When it does speak it dates
itself: *"2 bursts heard since arming, last one 20s ago — none passed the checksum"*, with the
live level on its own clause.

Decodes are treated differently on purpose. A packet that passed its checksum proves the whole
chain works, and that stays worth knowing however long ago it was — so it keeps its place and
carries its age instead: *"18 packets decoded since arming, last one 12m ago."*

The level reading now says what window it measures (the most recent tenth of a second), so a low
number reads as the gap between packets rather than something being wrong.

Finally, packet-shaped patterns found in silence no longer count as packets at all. Given enough
minutes the decoder will eventually find one in the noise floor, and reporting that as "packets
heard" invented evidence for a problem that was not there.

### APRS-IS settings moved into the APRS section

They were filed under Logging & Connectors with the other network feeds, which is where they
belong by type and not where anyone looked for them. They are now in **Settings ▸ Modes ▸ APRS**,
beside RTTY and CW. Nothing about the feed or the iGate changed — only where you find them.

### APRS stations look like what they are

Every station on the APRS map was the same grey dot. The packets were carrying the answer the whole
time — APRS stations pick their own icon, and Nexus was throwing it away.

Stations now draw as their **actual APRS symbol**, on the map and in the station list: cars, trucks,
bicycles and people, weather stations, digipeaters and iGates, campsites, balloons, boats and
aircraft. Vehicles under way point the way they are heading. Where an operator has put an **overlay
character** on their symbol — the `I` on a full iGate, the `R` on a receive-only one, the hop count
on a digipeater — it shows on top of the icon, because that character is often the most useful thing
about the station.

- **You can still tell what your own antenna heard.** That used to be the solid-versus-hollow dot.
  The shape now says what a station IS, so the ring around it says how it reached you: solid for RF,
  doubled when you heard it both ways, dashed and dimmed for internet-only. Solid still means yours.
- **Zoomed out, it stays calm.** Below a local scale the map goes back to plain dots — a continent
  covered in icons answers a question nobody asked.
- A symbol Nexus does not recognise draws the standard "unknown" glyph. Never a blank.

The icons are drawn in Nexus rather than borrowed, so there is nothing extra to install.

### APRS tells you when the radio is simply on the wrong frequency

The clearest report from testing: FT8 was decoding beautifully on 2 m at the very moment the APRS
screen insisted there was no audio. Both were true. The radio has one receiver and one dial, and
it was parked on the FT8 frequency in USB — so the APRS channel was never being received at all,
and every message about audio levels was advice about the wrong problem.

The APRS readout now looks at the radio itself. When the dial or the mode is not where APRS lives
it says so first and offers a one-click fix: **"The radio is on 144.174 USB — APRS needs 144.390
FM"**, with a Tune button beside it. It judges against the APRS channel *you have selected*, so
144.800 in Europe or 145.175 in Australia is correct, not a warning.

It names whichever thing is actually wrong. Sitting on the right frequency in the wrong mode is
its own trap — the signal looks strong and decodes nothing — so that case reads **"on 144.390 but
in USB — APRS needs FM"** and explains that FM packet audio demodulated as SSB is garbled. Data-FM
submodes such as PKTFM count as FM, because on the air they are.

The Tune control also speaks now. Tuning while an FT8 over is in flight cannot move the radio
immediately — the rig will not accept a frequency change mid-transmission — so instead of
appearing to do nothing it says the radio will move when the over ends.

### APRS no longer calls a closed squelch a broken audio device

Testing 0.21.1 on the air, the APRS decode readout sat on "no audio is reaching the decoder —
check your audio input" for most of a session, on a rig whose audio was set up correctly.

A squelched radio does not send the app silence in the sense of *nothing*; its USB codec keeps
streaming a continuous run of digital zeros. Audio was arriving the whole time — it just had no
level. The readout tested for those two things in one breath and reported the wrong one, so an
idle FM channel between packets, which is what APRS looks like nearly all the time, was announced
as a fault in your audio routing.

Those are now separate, and only one of them is a fault:

- **"Silent"** — the input is alive and delivering audio with nothing on it. Almost always just
  the squelch being closed between packets. The message says so, and says to open the squelch and
  watch for hiss if you want to confirm the routing. It is no longer coloured as a problem.
- **"No input"** — no audio samples are arriving at all. This one really does mean the capture
  device is wrong or gone, and still points you at Settings.

The readout also now shows the **input level in dBFS**, so what the decoder is hearing is a number
you can read rather than something to infer from which message appeared. And arming Monitor no
longer flashes a capture warning in the instant before the first audio arrives.

Two related honesty fixes: failed-checksum counts now explain that packets caught part-way through
— which is what happens when the squelch opens mid-burst — can never pass their checksum, so some
failures on a busy channel are expected rather than a sign of a misconfigured radio. And once
packets are decoding, the readout stays on the decode count instead of flicking back to a warning
during the quiet gaps between them.

### APRS now sees the whole network, and can contribute to it

APRS used to show you exactly what your own antenna decoded, and nothing else. That is the honest
picture of what your radio can reach, but on a quiet channel it is also indistinguishable from a
broken receiver — which is what several operators were looking at.

Nexus can now also connect to **APRS-IS**, the internet side of APRS, and plot what the wider
network is reporting near you alongside what you actually hear. Turn it on in
**Settings ▸ Integrations & Feeds ▸ APRS-IS**.

- **Every station is tagged with how it reached you** — `RF` when your own receiver decoded it,
  `net` when only the internet reported it, `RF+net` when both did. On the map an RF station is a
  solid dot and an internet-only station is a hollow one, so you can never mistake "the network
  says this station exists" for "my antenna can hear this station". One click hides the internet
  stations entirely, leaving the view of what this radio genuinely reaches.
- **It is also a diagnostic.** The internet feed runs whether or not the APRS decoder is armed, and
  gets its own status chip beside the decoder's. Internet stations appearing while the RF chip
  stays silent tells you the fault is in the radio chain — antenna, cable, sound card, tuning —
  and not in the app. That was previously guesswork.
- **You choose what comes through.** A radius around your grid square (150 km by default — APRS is
  a local mode), a list of watched callsigns that come through from anywhere however far away they
  are, and switches for weather stations, objects and items, and text messages.
- **No passcode needed to watch.** The feed connects read-only, which every APRS-IS server accepts
  from any licensed operator.

### APRS: you can put your corner of the map on the network

With the feed running you can also switch on a **receive-only iGate**: packets *your own antenna
hears* are contributed to APRS-IS, so stations around you reach the global map through your
station. It is a separate switch from the feed, because it publishes under your callsign.

Nexus only ever sends packets it actually heard on the air, and honours every rule the network
asks of an iGate: it never re-sends a packet that already came from the internet, never sends one
whose sender marked it `NOGATE` or `RFONLY`, suppresses duplicates, and caps its own upload rate so
a stuck transmitter nearby cannot flood the network in your name.

**Nexus does not gate the other way** — internet traffic is never transmitted on the air. That
direction means a radio keying up unattended, which is not something this app will do.

### DXpedition calendar: click an operation to read up on it

Clicking an operation on the calendar now opens its webpage in your browser, so the announcement
you are looking at is one click from the team's own page — bands, schedule, QSL route, pilot
station. The Details rail carries the same link on each entry, labelled, so you can see where it
goes before you click it.

About a third of announced operations publish a website, and the calendar source has been
carrying those links all along — Nexus was throwing them away while reading the page. The rest
now open the callsign's QRZ page instead, which is where their details and QSL route live when
there is no expedition site. Either way the tooltip names the destination first, and says plainly
when it is the QRZ fallback rather than the operation's own page.

Clicking a calendar bar still selects that operation in the Details rail as it did before, so
nothing that used to work costs you an extra click now.

### APRS: stations you could hear now actually show up on the map

Operators reported hearing plenty of APRS traffic on 144.390 while the map stayed empty. The
packets were decoding the whole time. The map was the problem, in three separate ways:

- **It opened showing the whole planet.** APRS is a local mode — 2 m simplex plus a digipeater or
  two reaches tens of kilometres — but the map opened at a scale where roughly 23 km fell on a
  single pixel. A station 40 km away drew less than two pixels from your own marker, so an entire
  local net stacked up underneath it as one dot. The APRS map now opens on the local picture,
  reaching about 275 km in each direction, and you can zoom in much further than before.
- **New decodes did not redraw it.** A freshly decoded station did not appear until something
  unrelated repainted the map, which on a resting screen meant waiting up to a minute. Clicking a
  station in the list had the same delay before the map highlighted it. Both are immediate now.
- **With no grid set it drew nothing at all.** If your station's grid square was empty the APRS
  map had no centre and painted an empty box — no coastline, no stations. It now centres on the
  traffic you are hearing instead.

### APRS tells you what it is hearing

An empty APRS screen used to mean three very different things, and looked identical for all of
them: the app listening to the wrong sound card, a signal arriving too corrupted to check, or a
genuinely quiet channel. Only packets that passed their checksum ever reached the screen, so
everything else vanished without trace.

The APRS header now carries a decode readout that says which one you are looking at — no audio
reaching the decoder, packets heard but failing their checksum, listening on a quiet channel, or
decoding normally with a count and how long ago the last one landed. Hovering it explains what to
check. The empty list and empty map say the same thing rather than a generic "nothing here".

The first two are worth calling out because both are fixable in seconds: what comes out of your
speaker tells you nothing about which device the app is capturing, and packets that all fail their
checksum usually mean the radio is off frequency or the receive audio is driven hard enough to
clip.

### APRS starts listening when you open it — receive only

Opening APRS now starts the decoder for you, so the screen is not dead until you find the Monitor
button. This is strictly receive: a decoder started this way will **never** send an automatic ack,
whatever your TX setting.

Automatic acks stay behind two deliberate acts, and opening a screen is not one of them: you arm
Monitor yourself, **and** TX is on. That is unchanged except that it is now enforced rather than
assumed — an unattended transmission should never follow from navigating somewhere. The Monitor
button says which state you are in, reading "Monitoring (auto)" when APRS started it for you, and
its tooltip spells out whether acks can go out.

Clicking Monitor always means start or stop, as before. It never quietly upgrades an
automatically-started decoder into one that can transmit — to allow acks, stop it and start it
yourself. And if you stop the decoder, it stays stopped: coming back to the APRS screen will not
restart it behind you.

### APRS Monitor button now reports the decoder, not its own guess

The Monitor button tracked its own idea of whether the decoder was running, which could disagree
with the decoder itself. Leaving the APRS screen and coming back showed "Monitor" — as though
nothing was running — while packets kept decoding into the list beside it, and the next click then
re-armed an already-armed decoder instead of stopping it. An arm the app refused also still lit the
button up as if it had worked. The button, the decode readout and the empty-state text now all
report the decoder's actual state.

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
