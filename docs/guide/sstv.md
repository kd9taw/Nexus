# SSTV

The SSTV section is a receive-first slow-scan station. Opening it starts the
receiver for you, and from then on any VIS header that arrives on the receive
audio decodes on its own: the picture appears where the waterfall was and is
saved to a local gallery with its mode, frequency and time. Transmit lives here
too — choose a picture, choose a mode, press **Send** — but it is always an
explicit act, and nothing on this screen keys the rig until you press that
button. It is not an image editor (your picture is cover-cropped to the mode's
size and nothing else) and it does not log: no QSO is written from this section.

SSTV sits in the left rail's Digital group (FT · Tempo · RTTY · SSTV · APRS) and
is on by default. A goal profile picked in the first-run wizard leaves it out, so
if the rail has no SSTV button, turn it on in
[Settings ▸ Appearance ▸ Features](settings-reference.md#features).

<!-- TODO: capture screenshot — the SSTV section receiving: the band waterfall replaced by a half-decoded Scottie 1 picture, the "decoding Scottie 1…" caption under it, two images already in the Gallery, the Transmit composer empty -->

## The tour

**The receiver arms itself when you open the section.** There is exactly one
reason to be on this screen with a receiver, so entering the view starts it —
you never have to know to arm anything. The header's **Arm** button reads
**Armed** while it is running and turns it off again; hovering it says what it
is doing: "Armed — any VIS header heard auto-decodes and auto-saves to the
gallery (RX only). Click to disarm." Stopping it that way is your decision and
is remembered for the rest of the session, so re-entering the section will not
restart it behind you; pressing **Arm** again clears that. The decoder is
RX-only — arming can never key the rig.

The section stays alive when you navigate away: the receiver keeps listening and
pictures keep landing in the gallery while you are on the map or in the logbook.
Only the on-screen readout pauses while it is hidden, and it catches up on the
first poll when you come back. None of this survives a restart, though — a fresh
launch is not armed until you open the section (or an ISS pass arms it, below).

**The header.** Left to right: a mode badge that reads **SSTV** and fills in to
**SSTV · Scottie 1** when a header lands, or **SSTV · TX PD-120** while you are
transmitting; the big frequency readout, which you can type into (a dial outside
the band plan is refused with a toast); the SSTV band-plan picker; the **Slant**
trim, which is deliberately disabled — "Auto-corrected by the decoder; the manual
trim comes in a later build"; **Arm**; and the **▼ TX On / ■ TX Off** latch, which
is this section's transmit arm — the top bar's TX cluster is hidden here, so
without it a send would sit at the "TX is off" gate with nothing on screen to
open it. **⊞ Panels** is in the same row.

**The band picker** carries the standard SSTV calling frequencies, filtered to
the bands your license class can key phone on:

| Band | Dial | Mode | |
|---|---|---|---|
| 160 m | 1.890 | LSB | rare, and a winter-night band |
| 80 m | 3.845 / 3.730 | LSB | NA calling / EU (IARU R1) calling |
| 40 m | 7.171 / 7.165 | LSB | US calling / EU calling |
| 20 m | 14.230 | USB | **the** worldwide SSTV calling frequency |
| 20 m alt | 14.233, 14.236 | USB | the overflow channels when 14.230 is busy |
| 17 m | 18.160 | USB | |
| 15 m | 21.340 | USB | |
| 12 m | 24.975 | USB | |
| 10 m | 28.680 | USB | General and above — US Technicians have 10 m image only 28.300–28.500 |
| 6 m | 50.680 | USB | activity follows sporadic-E openings |
| 2 m | 145.800 | FM | the ISS downlink — ARISS events transmit PD-120 here |
| 2 m | 144.500 | FM | terrestrial VHF calling (regional conventions vary) |

**One region shows the band, then the picture.** Until a header arrives that
space is a live waterfall so you can see what is actually on the frequency; the
moment a VIS lands the picture takes the same space, and takes it back when the
image finishes. It is the full waterfall instrument — palette, zoom span
(Std 0–3 kHz / Full 0–4 kHz or a window around the marker), the G and Z contrast
knobs, pause-and-scroll-back, the 3-D stacked view — running at a 50 ms row
cadence rather than the FT surfaces' slower default, so a signal appearing on
frequency shows up immediately. It does not tune: clicking it moves nothing,
because there is no audio-offset to place in SSTV.

**The line under the waterfall says what the receiver is hearing.** It is a
status readout, not a fixed hint, and it exists because "I hear a signal but the
SSTV is not decoding" has four completely different causes that used to look
identical on screen. Where it helps, it names the SSTV frequency for the band you
are on — derived from the plan above, so an operator sitting on 14.236 is told
about 14.236:

| State | What it says, and what it means |
|---|---|
| Stopped | "The receiver is stopped — nothing is being decoded. Press Arm to start it." You disarmed it, so nothing is being fed to the decoder. |
| Started | "Receiver started — no audio has reached the decoder yet." The first seconds after arming, before the decode thread has reported anything. It is not a fault and does not blame your sound card. |
| No capture | "Listening, but no audio is reaching the decoder at all — the capture device is not delivering anything." Nothing has arrived for 10 s and the decoder has polled at least 20 times (~2 s), so this is a real dead input: check that [Settings ▸ Radio ▸ Audio](settings-reference.md#audio) input is the radio. Hearing the signal on the speaker does not mean the app is capturing it. |
| Silent | "Audio is arriving but it is silent." Samples are arriving with a peak under 0.002 of full scale — a routing or level problem (wrong input, or RX Gain far too low), not a dead band. |
| Listening | "Hearing audio, no SSTV header yet — a picture decodes automatically when one starts." The healthy idle state, and the one thing the old fixed hint could never say. |
| Unsupported mode | "Heard an SSTV header *n* min ago in a mode this build cannot decode (VIS 0x…)." A clean header arrived and was thrown away. Your signal and audio path are fine; that transmission is in a mode outside the fifteen below. It stays on screen for five minutes, then stops being news. |
| Decoded | "*n* images decoded since arming, last one *n* min ago. Listening for the next header." A completed picture is a durable fact about the whole chain, so this one latches. |

If the app itself stops answering the readout says so — "Cannot read the receiver
state — the app is not answering. The decoder may still be running." — rather
than drawing an idle receiver it has no evidence for. The counters behind all of
this are cumulative since you armed; arming again starts them over. Changes of
state are announced once for screen readers, and the line is not a live region:
the age in it reticks every second and would otherwise be read aloud each time.

**While a picture is coming in**, the caption under it names the mode and how
long the wait is — "decoding Scottie 1… the picture lands when the transmission
ends (≈1:50)" — and switches to a line count when lines land. If the decoder
measured your radio more than 10 Hz off the transmission's leader tone it adds
"· tuning +40 Hz", which is the one thing the spectrum would have told you and
the picture cannot.

**Transmit** (a ⊞ pane) is the image chooser: a drop zone that takes a dragged
file, a **Choose image…** button for the file dialog, and a preview canvas that
shows the picture cover-cropped to the selected mode's exact pixels — scaled up
until it fills the frame, centred, overflow cropped. What you see there is
byte-for-byte what goes out. Under it, the file name and the size it was cropped
to (`sunset.jpg → 320×256`). Change the mode and it re-crops.

**Gallery** (a ⊞ pane) holds the received images, newest first, each a thumbnail
with its mode, the decoded FSK callsign ID if the sender appended one, and the
UTC time and dial frequency it finished on. Hovering a card shows the file's full
path. The pane is the one on this screen that grows: it takes the height left
over under the picture, keeping at least its head and a band of content, and once
the images out-run that it scrolls inside itself rather than pushing the Send bar
off the bottom. Thumbnails size to a column at least 140 px wide and keep their
own shape, so a 640×496 PD image and a 320×240 Robot image are the same width but
different heights and rows do not line up. Until the first image arrives the pane
holds a dashed placeholder: "Received images collect here — auto-saved with
callsign (FSK ID), mode, frequency, and time."

**The transmit bar** is pinned across the bottom and is not a panel. It carries
the mode picker — grouped Scottie / Martin / Robot / PD, each option labelled
with its airtime and pixel size (`Scottie 1 · ≈110s · 320×256`) — **Send**,
**Stop**, and, while an image is going out, a progress bar reading
"TX — Scottie 1 · 1:12 remaining". It sticks to the bottom of the scroll area, so
on a short window **Stop** is on screen at every scroll position.

**⊞ Panels** offers exactly two entries here, **Transmit** and **Gallery**, plus
**Undo last change** and **Reset layout**. The picture, the transmit bar and the
header are not panels: no layout you save can put Send, Stop or the TX latch out
of reach.

## Core workflows

### Receive your first picture

1. Open **SSTV**. The receiver starts; the header's Arm button reads **Armed**.
2. Tune 14.230 USB — type it into the readout or take it from the band picker.
   Entering the section does not touch the rig, so you can also just spin the
   knob and watch the readout follow.
3. Read the line under the waterfall. "Hearing audio, no SSTV header yet" means
   the whole chain is working and you are waiting on the band.
4. When a transmission starts, the waterfall is replaced by the picture and the
   caption names the mode and the airtime. Expect a black frame for most of that
   time — see the limits below.
5. The finished picture saves itself: it appears in the Gallery, and a BMP lands
   in the Nexus `sstv-gallery` folder beside a `gallery.json` of the metadata.

<!-- TODO: capture screenshot — the Gallery pane with four received images: mixed modes (a PD-120 and two Robot 36), one card carrying a decoded FSK callsign badge, each caption showing UTC and dial frequency -->

### Work out why nothing is decoding

The status line is the diagnosis, in order of what to do about it:

1. **"The receiver is stopped"** — press **Arm**.
2. **"no audio is reaching the decoder at all"** — the capture device is dead to
   the app. Point [Settings ▸ Radio ▸ Audio](settings-reference.md#audio) at the radio.
   What you hear on the speaker says nothing about what Nexus is capturing, and
   neither does the waterfall: it rides a separate tap.
3. **"Audio is arriving but it is silent"** — right device, no level. Check the
   rig's audio output and RX Gain.
4. **"a mode this build cannot decode (VIS 0x…)"** — nothing is wrong with your
   station. That station is transmitting outside the fifteen modes below.
5. **"Hearing audio, no SSTV header yet"** — nothing is wrong at all. Check you
   are on the frequency the line names for your band.

### Send an image

1. Arm transmit with the header's **■ TX Off** latch (it turns into **▼ TX On**).
2. Drag a picture onto the Transmit pane, or use **Choose image…**. Any format
   the webview can decode (PNG, JPEG, …) works; it is cropped to the mode.
3. Pick a mode in the bottom bar. Until you choose one, Nexus follows the band:
   Scottie 1 on HF (the North American calling-frequency convention) and PD-120
   above 30 MHz (what ARISS uses).
4. Press **Send**. Nexus switches the app to Phone so the image rides the phone
   segment — without moving your dial — then hands the encoded transmission to
   the gated transmit path. A refusal is a toast that names the reason: TX off,
   outside your license privileges, the transmitter already busy with the voice
   keyer or mic PTT, or a mode whose key-down would out-run your Tx Watchdog.
5. Watch the progress bar count down. **Stop** aborts the image, drops the queued
   job and unkeys; so does turning the TX latch off.

On 145.800 MHz — the ISS downlink — Send asks first: "Transmit only during a
sanctioned ARISS uplink event. Send anyway?"

### Catch an ISS pass

1. Turn on **ISS SSTV auto-arm** in
   [Settings ▸ Radio ▸ Rig Control](settings-reference.md#rig-control). It is off by default.
2. At AOS of a pass, Nexus saves your dial, tunes 145.800 FM and arms the
   receiver, telling you it has done so.
3. ARISS transmits PD-120, which decodes here like anything else.
4. At LOS it disarms and puts your dial back — but only if you are still parked
   on 145.800 FM, so a mid-pass QSY of your own is left alone. That automatic
   stop is not treated as your decision, so opening the section later still
   starts the receiver normally.

See [Satellites](satellites.md) for pass prediction and the rest of the ISS
picture.

## Honest limits

- **The picture lands at the end, not line by line.** The decoder needs the whole
  image buffered before it can place any line, so a Scottie 1 preview is a black
  rectangle for about 110 seconds and then the picture appears nearly all at
  once. The caption says as much, with the airtime, because otherwise it reads as
  a hang.
- **You have to catch the header.** Decoding starts from a VIS header; tuning in
  mid-picture decodes nothing, and you wait for the next transmission. There is
  no partial-image recovery.
- **Fifteen modes, and nothing else** — Scottie 1 / 2 / DX, Martin 1 / 2,
  Robot 24 / 36 / 72, and PD-50 / 90 / 120 / 160 / 180 / 240 / 290, for both
  receive and transmit. A header in any other mode is counted, named by its VIS
  code on screen, and discarded.
- **Only PD-120, PD-180 and Robot 36 are validated against real off-air
  recordings** (ARISS captures). The Scottie and Martin families are validated by
  encoding and decoding synthetic images round-trip, because no reference
  recordings were available.
- **The live picture is a preview, not the decode.** What you watch on screen is
  a thumbnail no more than 160 px wide, upscaled in whole multiples so it stays
  crisp (and capped at 6×, leaving margin rather than blur). The full-resolution
  image exists only once it is saved.
- **The gallery is a gallery, not a viewer.** There is no click-to-enlarge, no
  delete, no rename, no export or share. To see an image full size, open the BMP
  from the folder — the card's tooltip gives you the path. The in-app list keeps
  the 200 newest; images past that stay on disk but drop off the screen.
- **Images are 24-bit BMP.** Universally openable, and larger than a PNG of the
  same picture.
- **The slant trim is disabled.** The decoder re-anchors the line rate itself;
  the manual trim control is on screen but inert, and its tooltip says so.
- **Nothing here logs.** No QSO, no callsign field, no dupe check. The only
  callsign SSTV recovers is the FSK ID some stations append after the picture,
  which is best-effort, at most ten characters, and simply absent when the burst
  is missing or garbled.
- **Robot 24 and Robot 36 carry a colour cast on the very top row**, an artifact
  of how those modes alternate colour information between lines. It is faithful
  to the reference decoder and it is in the saved image too.
- **Transmit is sound card and PTT**, the same path FT8 uses: Nexus generates the
  audio and keys the rig. There is no rig-side SSTV generator, and a station whose
  audio is not routed to the transmitter sends nothing.
- **One image at a time, no queue**, and the transmitter is shared: a send is
  refused while the voice keyer or mic PTT owns it, and refused again if you try
  to start a second image.
- **Long modes can be refused before they start.** An image whose key-down would
  out-run your **Tx Watchdog** (default 6 minutes, with 15 seconds of head-room)
  is refused up front, naming the mode's airtime and the setting, rather than
  keyed and guillotined half-drawn. There is also a hard ceiling of 330 seconds
  of key-down regardless of the watchdog — above every legitimate mode, PD-290
  included.
- **The receiver is session state.** A restart leaves it stopped until you open
  the section again; the gallery, being on disk, comes back with the app.

## Related guides

- [Phone (SSB)](phone.md) — SSTV rides the phone segment and shares its
  transmitter
- [RTTY](rtty.md) — the other free-running mode in the Digital group
- [Satellites](satellites.md) — ISS passes, prediction and tracking
- [Operate — FT8/FT4 digital](operate-digital.md) — the same waterfall
  instrument, and the audio path SSTV transmits through
- [Settings reference](settings-reference.md)
