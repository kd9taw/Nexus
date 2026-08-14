# Nexus 1.3.0 — Settings you can find your way around, and RTTY that keys like MMTTY

*2026-08-13*

Settings has been rebuilt around how you actually operate: a tab for Phone, one for CW, one
for Digital, a search box that finds a control by the words printed on it, and every
"Settings ▸ …" pointer in the app turned into a button that takes you there. RTTY gained a
TX button — key up once and type, the way MMTTY does it. And you can finally tune off the
ham bands from the app and listen there without the app throwing your context away.

**Take this one if you set Nexus up recently and could not find something, or if you run
RTTY.** Everything here is upgrade-safe: no setting moves value, only location.

---

## Settings you can navigate

The Modes page held eleven sections, so the mode you came for sat behind every other mode's
— a CW operator scrolled past the whole FT8 section and six weak-signal modes to reach their
keyer. It now splits into the three the left-hand rail already shows: **Phone**, **CW** and
**Digital**. SSTV, APRS, RTTY, Tempo and the weak-signal modes (Q65, MSK144, JT65, FST4,
WSPR) each keep their own named section under Digital. The Frequencies tab folds in there
too, honestly renamed **Working frequencies (FT8/FT4)** — that table only ever held FT8 and
FT4 rows while its name promised every band plan you own.

**The audio settings sit with the COM port again.** Picking the serial port and picking the
sound card are the same job — on a one-cable interface they are the same cable — but they
had drifted more than a thousand lines apart, with the satellite and rotator settings in
between. Audio now renders directly under the CAT settings, the way WSJT-X and fldigi have
always had it.

**Rig Control is now "Rig & CAT", and holds only that.** It had quietly become the place
everything rig-adjacent landed. Band-edge tones, the per-mode power caps, the setup backup,
rig sharing and the permission for other programs to key your transmitter move to a new
**Transmit limits & sharing** section. Nothing changed about what they do.

**A search box that speaks your language.** It searches the words on the control rather than
only the headings above them — "COM port", "sound card", "PL tone", "WPM", "keps", "stop
bits", "no RF" all land on the right page even though none of those phrases is a heading.
Picking a result takes you there, and if the setting lives inside a collapsed **Advanced**
group it opens the group with the control in view.

**Every "Settings ▸ …" pointer is now a button.** The app named a Settings path in about 228
places and none of them were clickable — including several that named tabs which no longer
existed. Setup health's own "no RX audio" light told you to check the audio device *below*
and then left you to find it.

**And Settings uses the whole window.** It rendered in a 1100px column down the middle, so on
an ordinary monitor a page that would have fitted was scrolled instead, with half the screen
empty beside it. Controls that gain nothing from being wider are still held to a readable
width; nothing changes on a small laptop screen, where the column was never the constraint.

## RTTY: key up once and type

The RTTY dock gained a **TX** button. Click it and the transmitter comes up and stays up:
type and the characters go out as you type them, and between keystrokes the air carries
diddle (the LTRS idle every RTTY station sends), so the far end holds sync instead of hearing
you drop out at the end of every line. Click TX again and it sends the rest of what you typed
and unkeys. Enter-per-line still works exactly as before when the TX button is off.

Because this is the one transmission in Nexus that keys with no fixed end, it is wrapped in
more stops than anything else, not fewer. Stop TX, the dock's Esc/Stop macro, the TX-enable
switch and **Esc** each cut it instantly. So does anything that takes RTTY off the rig
without you pressing a stop — leaving the RTTY section, tuning to a frequency you are not
licensed for, starting a tune carrier, or switching radios. Your TX watchdog applies as it
does to any other over, and above it sits a hard ten-minute ceiling on a single continuous
over that nothing can extend. If the app itself locks up, the transmitter unkeys on its own
within about a second.

## Listening off the ham bands

Two things were wrong once you got off-band, and both are fixed. First, you could not get
there: the wheel, the scope click, the ◄/► buttons and typing a frequency all refused
anything outside the band plan, so WWV or a shortwave broadcaster could only be reached by
turning the knob on the radio. They all tune anywhere now. A fast wheel flick still stops at
the band edge so you cannot leave a band by accident, and one more scroll goes past.

Second, the parts of the app that ask "is this station new *on this band*" now stay quiet
while there is no band to ask about. The all-time questions still answer, so an entity you
have never worked anywhere is still flagged wherever you hear it.

Transmitting is deliberately unchanged. Nexus does not refuse to transmit merely because a
frequency is off *its* band table — that table is written around the US allocations, and the
table is not the law everywhere. The UK 60 m allocation starts at 5.2585 MHz, below the 5.3
the table calls the bottom of 60 m, so a blanket refusal would have blocked operators working
their own legal frequencies. If you have told Nexus your US licence class it still fails
closed off-band, exactly as before.

## A beam heading next to every country

Band Activity, the Call Roster, the Tempo roster, the Needed board, Spots, the chase panes,
the pounce banner, the selected-station card, the DXpedition views and the map's tooltips all
print the short-path bearing from your grid straight after the entity name — `Fed. Rep. of
Germany 47°`. When the station sent a grid the heading is measured to that square. When it
did not, it is measured to the middle of its DXCC entity and marked with a tilde, `~47°`,
because the middle of a continental country is nowhere near most of the stations in it: every
US callsign resolves to one point in Missouri. If your grid is not set, nothing is shown at
all — an honest gap rather than a confident 0° pointing due north.

## Microwave, to 24 GHz

The band table, the frequency pickers, the override editor and the typed-dial entry all know
33 cm, 13 cm, 9 cm, 6 cm, 3 cm and 1.25 cm — ADIF's registered names, so a QO-100 contact
finally logs `BAND:3cm` instead of an empty field LoTW rejects. US transmit privileges follow
the regulation exactly, including the 2310–2390 gap staying locked because it isn't amateur
spectrum. The microwave bands also join the per-band grid tracker as the real ARRL VUCC bands
they are. From the QO-100 field report.

## SSTV and APRS remember what you told them

**SSTV has a Settings section** — it was the only mode with a cockpit and no settings, so
every choice you made was gone by the next launch. It now holds a default transmit mode, a
remembered transmit power, and a switch for starting the receiver when the SSTV screen opens.

**SSTV on an FM channel now transmits in FM, not USB-D.** Reported from an FTDX10 and an
IC-9700: pick the 144.500 SSTV channel and the radio goes to FM, then press Send and it
jumped to USB-D — an SSB signal on an FM repeater. An image on an FM channel is now sent in
the rig's FM data submode, so the sound card still reaches the modulator and the repeater
shift and PL tone stay applied through the picture.

**SSTV now respects your high-duty power cap, not your SSB one.** An SSTV frame keys
continuously for up to 290 seconds at close to 100% duty. Worth watching your PA the first
time you send a picture.

**APRS remembers your station.** The beacon symbol, comment, digipeater path and the RF
channel lived in the screen and reset on every restart, so a European operator's rig was
retuned to the US channel each launch. The channel is now derived from your grid square when
you have not picked one — open APRS in Europe and it lands on 144.800 with nothing configured.

## Knowing whether your contacts got out

**The connector dots tell you whether your contacts are actually getting out.** Settings ▸
Connections painted every dot from "is a password saved?", which is not the question anyone
opens that panel to ask. Revoke your ClubLog app-password, rotate a QRZ Logbook key, mistype
an HRDLog upload code — the secret is still in your keychain, so the dot stayed green while
nothing reached the service. Each row now reports the last time Nexus really talked to that
service, including **stored — not verified yet** in amber for a credential nothing has been
sent through. No news is not good news, and it no longer pretends to be.

**"Sync LoTW now" is now "Download confirmations".** It only ever pulled confirmations down;
operators reasonably read "sync" as two-way and believed their contacts had gone to ARRL.

**Nexus can hand your log to TQSL on a timer.** Off until you turn it on. It refuses to run
while *Sign from ADIF location* is on, because a timer that fired after you moved would sign
last week's contacts with this week's grid.

## Also fixed

- **Clicking a spot no longer sends the radio haywire.** On a rig with no data submode — an
  FT-950 was the report — Nexus re-commanded the dial about fifty times a second,
  indefinitely, so the readout stopped following the knob and the meters froze.
- **Linux: the sound card you picked actually opens now.** Since 1.0.1 the device menu listed
  cards correctly, but the code that opened your pick checked it against a probe list that
  held each card open, so your saved pick never matched and Nexus fell back to the default.
  (#2, #8)
- **Test CAT tells you when it couldn't run**, instead of sending you to re-check settings
  that were fine.
- **ClubLog uploads no longer hammer**, and catch up after you fix the password.
- **QSOs logged while HRD is closed are no longer lost** — they queue and send when HRD is
  reachable.
- **Changing the audio device no longer risks taking the app down.** (This is not the startup
  crash reported against 1.2.0, which is still open.)
- A DXpedition Fox's two-in-one message is recognized outside Hound mode, and a decode
  addressed to a hashed or portable form of your call highlights as yours. From KR4FQG.
- The logbook edit form can view, correct and add a QSO's POTA park.
- A portable-callsign warning for QRZ, whose logbook is tied to one exact callsign.
- The Yaesu FT-890 is in the rig picker by name.

## Filtering the noise

A real **blocklist** — alt-double-click a decode or roster row and the call goes on a
persistent list your auto-CQ honors, so a blocked station answering your run is passed over
for the next caller. Plus **Hide calls** by name or prefix, **−Conf** to drop stations you
already have confirmed on this band, a **Pause** switch for the country filter, and search
over the full DXCC entity table rather than the common 18. Most of this came from F4MQS.

## One Units setting

Distances, temperature and wind speed now follow a single Units choice (Settings ▸ Station):
automatic from your system's region, or metric/imperial by hand. Every transmitted value
stays native; only the display converts. From F4MQS.
