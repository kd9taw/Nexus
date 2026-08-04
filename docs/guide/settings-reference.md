# Settings reference

Settings is organized into eight tabs. Only the active tab renders, so typing in
one field doesn't lag the whole panel. **Save** at the bottom applies your
changes; most take effect live (a few say "takes effect on restart", and a few
apply the instant you touch them — those say so too).

Your callsign is required, but **Save is never greyed out**. Clicking it with an
empty callsign takes you to the Station tab with a message saying so, rather than
leaving a dead button with no reason and no fix.

The tabs, in the order they appear:

[Station](#station) · [Radio](#radio) · [Modes](#modes) ·
[Frequencies](#frequencies) · [Spots & Alerts](#spots--alerts) ·
[Logging & Connectors](#logging--connectors) · [Contesting](#contesting) ·
[Appearance](#appearance)

The panel header carries the **build stamp** (confirm a fresh install actually
took) and a **Check for updates** button.

<!-- TODO: capture screenshot — the Settings panel with its eight tabs across the top -->

---

## Station

Your operator identity, license privileges, and default frequency.

### Operator & Radio

- **Callsign** — "Your station callsign (required)." Everything keys off this.
- **Grid** — "Maidenhead locator. All 6 characters — 4 measures every distance
  and bearing from the middle of a ~100-mile square." Drives satellite passes,
  propagation anchoring, and distance math.
- **Operator name** — "Used by the CW `{NAME}` macro and logging."
- **State** — "Your US state/province — the CW `{MYSTATE}` macro (ragchew QTH)."
- **License Class** — Technician / General / Amateur Extra (US), or **Open** for
  non-US operators. "Sets your transmit privileges + the licensed-segment band
  dropdown. Open = no limits (outside the US)." This is a software transmit guard
  in every Nexus TX path, checked against the Part 97 sub-band table — Nexus
  refuses to key the rig outside your segment.
- **Band & Frequency** — "Pick a band-plan channel, or type a dial frequency in
  MHz."

---

## Radio

Everything about the rig itself: the roster, CAT, PTT, satellite Doppler, the
rotator, and sound-card routing.

### Setup health

A strip above the roster answering "is the station actually working?" — three
live indicators, so setup stops running on faith:

- **Rig** — responding / not answering / untested (a live **Test CAT** result
  wins over the passive CAT state).
- **RX audio** — the live level in dB, or the audio error.
- **TX** — on / off, and while a tune carrier is keying, the forward power.

**Prove TX** keys a ~2-second tune carrier to verify the CAT → PTT → RF path. It
asks for confirmation first, every time, and reminds you to have an antenna or
dummy load connected.

### Radios

Run more than one rig. Always shown — with one radio it is just a card and an
**+ Add radio** button.

- **Per-radio cards** — rename in place; **Active** marks your operating radio;
  **Edit** loads that radio's CAT + audio into the form below *without* changing
  the radio you're operating on (no swap, no dropped carrier) and shows an
  **Editing** badge; **Make active** swaps rigs (dropping any carrier first);
  **Remove** deletes a non-active radio. The card's meta line shows model,
  port/address, audio device, and rigctld port.
- **Covers bands** — band chips per radio for auto band-routing. None = covers
  all. Appears once you have two radios.
- **+ Add radio** — the discovery affordance. "Run two rigs at once — e.g. an HF
  radio plus a VHF/UHF radio on a different antenna?"

With two or more radios, three more controls appear:

- **Routing rules** — band coverage sends a whole band to one radio; a rule adds
  the **mode**, for when two radios share a band (2 m FT8 to the digital rig,
  2 m FM and APRS to the FM rig). Each rule is band chips + a mode class
  (Weak-signal digital, FM & APRS, SSB phone, CW, RTTY) → a radio. Rules are
  checked top to bottom and the **first match wins**; ↑ / ↓ reorder, ✕ removes.
  **Satellite** rides the same dropdown but is a *context*, not a sixth mode: it
  is matched only by transponder picks, at a tier above the mode rules, so a
  terrestrial tune never matches it.
- **Everything else** — the default radio for anything no rule and no band
  coverage matched, or "Stay on the current radio".
- **Test a band + mode** — pick a band and mode, click **Where would this go?**
  and the answer comes from the same resolver the radio loop uses. It does not
  QSY anything.
- **Run both radios at the same time** — launch Nexus and it asks which radio
  this window drives; open a second window for the other. Both share one logbook.
  Leave off if you only ever use one radio at a time — you can still switch
  between them from the top bar.

### Profiles

- **Saved profiles** — **Load** applies a profile merged onto your current
  settings; **Delete** removes it. "Switch a whole rig / antenna / CAT / band
  setup in one move." Your callsign, license class, radio roster and sync history
  never come from a profile, and anything the profile predates keeps its current
  value.
- **Save current as** — snapshots the current settings under a name.

### Rig Control

- **PTT Method** — "How transmit is keyed": CAT (via rigctld), Serial RTS, Serial
  DTR, or VOX (no keying). PTT and CAT are independent axes — VOX PTT with full
  CAT control is a valid setup.
- **PTT Serial Port** — appears on RTS/DTR. The COM port your keying line is on,
  for an SO2R controller (u2R/MK2R) that routes PTT separately from CAT. Blank =
  keying shares the CAT port, which is how a single-cable interface like a
  Digirig Mobile is wired. Per radio.
- **Zero-config setup ▸ Detect my radio** — "One scan for everything: USB radios
  (fills model, port, sound device) AND FlexRadios on the network (fills the
  SmartSDR CAT config). Review, then Save." Each hit gets a **Use this** button.
  The list is honest about what it found: a recognised interface cable says so
  and tells you to pick the rig yourself; a generic USB bridge chip says the port
  is right but the model isn't known; a missing Windows driver links the driver.
  Dual-UART Icoms show two rows and the CI-V one is tagged.
- **Rig Model** — "Hamlib rig model." A curated ~50-rig list by default; tick
  **Show all models** for the full Hamlib catalog, or type a model number
  directly ("Hamlib may still support it even without a friendly name here").
- **Connection** — "Serial for a USB/COM rig (most, incl. Xiegu); Network for a
  FlexRadio via SmartSDR or a remote rigctld over TCP."
- **Network Address** (Network only) — host:port. For a Flex, the WSJT-X-proven
  path is the SmartSDR CAT app on **this** PC: its default TCP port 5002 is
  directed at slice A, so `127.0.0.1:5002` with the FLEX-6xxx model works out of
  the box and audio rides DAX. Multi-slice ports are B=60001, C=60002, D=60003 —
  Nexus drives one slice. (Direct-to-radio `:4992` needs Hamlib's experimental
  native model and failed on real hardware.) A one-click **⚡ Pair DAX audio**
  button appears when SmartSDR's DAX devices are detected and neither audio side
  is already a DAX device — it bootstraps, it does not override a working
  hand-picked config.
- **Serial Port** (Serial only) — "COM / tty device for rig control — or
  Auto-test to find it." **Refresh** re-scans; **Auto-test** probes each port
  read-only (never transmitting) and selects the one that drives your rig. You
  can type a port that never enumerated. Rig-specific warnings appear inline:
  Xiegu CAT is on the SERIAL-B port; the Icom CI-V port is the CP210x one marked
  *Enhanced*.
- **Baud** (Serial only) — "match your rig's CAT setting (most modern rigs:
  38,400 or 115,200). Native Icom CI-V scope needs 115,200 here *and* on the rig."
- **Antenna rotator** — pick your rotator model and its COM port and baud, and
  "Nexus runs the control daemon for you (same as the rig)." Then use the Rotor
  pane in [Connect](connect.md), the ↗ on [Needed](needed-dx.md) rows, or the
  compass anywhere. **Dummy (testing — no hardware)** lets you try it without a
  rotator; **Other Hamlib model #…** takes any model number. A separate advanced
  field takes an external `rotctld` host:port that overrides all of the above.
- **ISS SSTV auto-arm** — "tunes 145.800 FM and arms the decoder when the ISS is
  overhead, restores your dial at LOS. Off by default."
- **Split operation** — None / Rig / Fake It. "Keeps your transmitted audio
  between 1500–2000 Hz by shifting the TX dial in 500 Hz steps, so audio
  harmonics fall outside the transmit filter — cleaner signal. Rig = uses VFO B
  split. Fake It = retunes the VFO around each over (works on any CAT rig). None
  = stock WSJT-X default."
- **Wheel tuning sensitivity** — how far the dial moves per mouse-wheel notch.
  Lower it if a free-spin mouse tunes too far per flick. Applies to the frequency
  readout and the Phone/CW scope wheel.

**Advanced** (a collapsed group) holds the rest:

- **rigctld TCP Port** — "Port Nexus launches rigctld on" (default 4532).
- **Data modes use plain SSB** — **leave this off unless you know you need it.**
  Nexus normally puts the radio in its DATA submode (DATA-U / USB-D / PKTUSB) for
  FT8, FT4, RTTY-AFSK and SSTV, because on most rigs that is the only mode where
  the USB codec reaches the transmitter. On a rig whose codec feeds only the data
  port, plain SSB takes audio from the mic and the radio transmits **no RF at
  all**. Only correct when your transmit audio goes in the microphone path (some
  RIGblaster models). Per radio. True FSK RTTY is unaffected.
- **Native Icom CI-V (early access)** — appears on a serial IC-7300/7610/9700/
  705/905. Nexus drives CI-V directly instead of launching rigctld, unlocking the
  rig's real spectrum scope ("CI-V RF") and instant dial tracking. Needs 115200
  baud set on **both** the radio and Nexus, plus "CI-V USB Port = Unlink from
  [REMOTE]" on the rig; below that the rig refuses to stream the scope (CAT still
  works, the panadapter just stays off).
- **Flex native panadapter (early access)** and **Flex native DAX audio (early
  access)** — appear on a network Flex. Stream the real SmartSDR panadapter
  (VITA-49 FFT) into the cockpit scope, and take RX audio straight off the
  network instead of the "DAX Audio RX" sound device, **which is invisible under
  Remote Desktop**. Both are **unverified on hardware**, so both are opt-in; if
  the scope stays blank or decodes stop, turn them back off.
- **CI-V bus diagnostic log** — appears once native CI-V is on. Records every
  byte to and from the radio to a file in your Downloads, for hardware-only
  issues like the IC-9700 PTT flicker. Turn on, reproduce, turn off, send the
  file. It keeps running while you're on other screens.
- **Flex radio IP (native panadapter)** — the FlexRadio's own LAN IP (SmartSDR
  API, port 4992). "This is the *radio's* address, not the SmartSDR-CAT port."
- **Share my radio (CAT broker)** — "Run a rigctld-compatible server so WSJT-X /
  N1MM / loggers share this radio THROUGH Nexus." Takes effect right away, no
  restart, and works even when Nexus is sharing an external rigctld. When on:
  - **CAT broker port** — "Other apps connect here (Hamlib NET rigctl default
    4532)."
  - **Broker PTT** — "Let the connected app key transmit when Nexus is idle. Off
    = other apps control the rig but never key it (Nexus owns TX)." Default off.

**Test CAT** saves, launches the bundled `rigctld` (Hamlib ships with Nexus on
Windows — no separate install), and reads the rig's frequency to confirm the
link.

Two more controls sit below the Advanced group:

- **Band-edge tones** — "A short audio cue when the dial crosses your license
  privileges — a rising 'ding' back in band, a falling 'dong' past an edge."
  Applies on every mode, not just digital. On by default.
- **Max power by mode (safety)** — a percentage ceiling on RF output for Phone,
  CW and Digital; blank = full power. FT8/FT4/RTTY run ~100% duty cycle, so
  capping Digital (e.g. 30%) protects your finals and any amplifier. The rig is
  brought down to the cap the moment you enter a capped mode, not only when you
  touch the power slider.

### Satellite Doppler

Corrects both legs of a pass — the downlink you listen on and the uplink you
transmit on. Nexus tunes only while auto-track is following a pass and you have
picked a transponder in the Satellites section.

- **Doppler correction** — on by default. "Retunes the radio through a pass so
  you stay on the station you are working." Clearing it stops both legs.
- **VFO mapping** — which VFO carries your uplink; match it to how your radio is
  wired. **A wrong mapping transmits on your own downlink** — into the
  satellite's output passband, on top of everyone else working the bird. Picking
  one applies immediately and confirms it for the radio you are *operating*; a
  second radio gets its own confirmation on the pass rail. The control is
  disabled while you are editing a non-active radio, and says why.
- **Minimum shift (Hz)** — corrections smaller than this aren't sent. "20 Hz is
  inaudible on SSB and keeps the CAT link quiet. 0 sends every update."
- **Update interval (ms)** — shortest gap between corrections. "1000 ms is what a
  low-orbit pass needs. Shorter fights your own tuning knob and saturates a
  serial CAT link."
- **Pass alert sounds** — a rising tone at AOS and a falling one at LOS,
  alongside the popup. On by default; clearing it silences only the tones, never
  the popups.

### Orbital elements

Keplerian elements (TLEs) for the amateur satellites — pass times, pointing and
Doppler all come from them. Refreshed every 6 h from hamradiotools.io: the bird
list from the SatNOGS database (CC BY-SA 4.0), the elements from CelesTrak and
SatNOGS.

- **Update now** — fetch immediately.
- **Import from file** — a Celestrak TLE, AMSAT keps, or a new launch's SupGP
  set; the offline-shack escape hatch. Imports persist across refreshes and the
  newest epoch per satellite wins.

The status line always shows the bird count, the band coverage, the fetch date
and the source. A failed refresh adds a plain-language "Last refresh" line.

### Rotator

Pointing manners for the rotator picked under **Rig Control**. They apply to
satellite auto-track.

- **Park position (° az / el)** — "The stow position — wind-safe, or wherever
  your mast rests. Used only when After a pass is set to Park."
- **Ready position (° az / el)** — "Where the antenna waits for the next pass."
- **After a pass** — Stop / Park / Ready. "Stop is the default and moves nothing:
  the antenna stays pointed where the bird set." Park and Ready drive the rotator
  on their own at LOS, so set those positions first.
- **Tolerance (° az / el)** — a new target closer than this isn't commanded.
  "Without a deadband the rotator hunts and the relays chatter for the whole
  pass. 2° is about a G-5500's own resolution."
- **Calibration trim (° az / el)** — added to every command. "Use it when the
  controller reads one heading and the boom points at another."
- **Allow flip** — takes a high pass by turning azimuth 180° and running
  elevation past 90°. Off by default: **many rotators cannot mechanically go past
  90° elevation.** Check your controller first.

### Audio

With two or more radios, a banner names which radio these devices belong to.

- **Input Device (RX)** — "Sound card carrying receive audio." **Refresh**
  re-scans.
- **Output Device (TX)** — "Sound card feeding the rig (transmit)."
- **Live input spectrum** — what the selected input hears, live. "Band noise
  should show as a moving floor. Confirms the RIGHT device before you leave
  Settings." Flat means no audio on that input.
- **Tx Power** — the audio **drive** into the rig, the same control as the
  cockpit **Pwr** slider (they always match). "Trim down until your rig's ALC is
  just zero. This is *not* the rig's RF watts — set those on the radio."
- **RX Level** — a live dB meter like WSJT-X. "Aim for around 30 dB. Anything
  from ~15–60 dB decodes fine; red means too hot." An audio error shows here.
- **RX Gain** — "Boost a quiet interface until RX Level reads around 30 dB — the
  meter responds as you release the slider. Leave at ×1.0 unless the meter reads
  low (under ~15 dB) — FT8 decodes on a small signal, so you rarely need much."

### Headphone monitor

- **Enable monitor** — plays "the exact audio the decoder hears — for level / RFI
  diagnosis and listening to the band. Off by default; UNVERIFIED on-air until
  the attended session." It guards against the rig's TX device by name; if your
  devices go by multiple names, pick your headphones explicitly rather than
  System default.
- **Monitor Output Device** — "must NOT be the rig's TX output device."
- **Monitor Level** — "Headphone listening volume (does not affect TX)."

---

## Modes

One fieldset per mode. Anything that changes the on-air signal or the decode
frame lives here, not with the radio.

### Digital (FT8/FT4)

**Transmit & Sequencing**

- **Transmit period — Tx 1st (even)** — "On = transmit in the even/1st T/R slots;
  off = odd/2nd. The two stations in a QSO must pick **opposite** periods." Also
  on the top bar.
- **Tx Watchdog (min)** — "Auto-halt TX after this many minutes (0 = off)."
- **Disable TX after sending 73** — "After your final 73 goes out, Enable TX
  drops — working the next station is a deliberate arm (WSJT-X default). A CQ run
  is unaffected: it returns to CQ."
- **Double-click arms TX** — "Double-clicking a station enables TX so the answer
  goes straight out." Off = you arm TX yourself each time.
- **Tune timeout (s)** — "Auto-release the tune carrier after this many seconds —
  never leave a key-down unattended" (default 12).

**Auto-CQ & Caller Selection**

- **Stop CQ after N calls** — "Blank = WSJT-X behavior: CQ repeats until you stop
  it (the TX watchdog is the backstop). Set a number to auto-stop an unanswered
  CQ run." The Tempo chat CQ run always stops (default 10 unanswered); this
  number overrides that budget too.
- **Tempo chat: send cycles per message** — "A chat message transmits at most
  this many cycles, then shows 'no ack' (tap the bubble to re-send). Blank = 3
  (TempoDeep uses 5). Never affects FT8/FT4."
- **Tempo chat: a reply counts as received** — when the station you messaged
  sends a complete message back, stop re-sending and mark yours "confirmed"
  (works even when the other side isn't Nexus). A real ACK still upgrades it to
  "Delivered ✓".
- **Auto-CQ: drop a silent caller after N overs** — abandon a station that
  answered then went quiet and return to CQ. "Blank = 3; 0 = never abandon (wait
  for you, like stock WSJT-X)."
- **Best caller (auto-CQ pick)** — when several stations answer, which to work
  first: First to answer (default), Strongest signal, Farthest away, or Prefer CQ
  callers, with an optional minimum SNR.

**Logging Behavior**

- **Auto-log QSOs** — "Automatically log completed contacts to the ADIF logbook."
- **Prompt before logging** — a WSJT-X-style confirm-and-edit popup instead of
  logging silently. "No effect unless Auto-log is on."
- **Roger with RRR (not RR73)** — "Acknowledge the final report with a bare RRR
  (partner still owes a 73) instead of the combined RR73. Off = RR73 (modern FT8
  practice)."
- **Clear DX call after logging** — wipe the DX Call / DX Grid fields once a
  contact is logged. Off by default.

**Decoder**

All Decoder settings drive the *native* decoder. On a WSJT-X UDP source
(Companion mode) decodes arrive already made and **none of them apply**.

- **Decode depth** — Fast / Normal / Deep. "Deep finds the most signals (WSJT-X
  default); Fast saves CPU on old hardware."
- **Decoder passband (Hz)** — F low / F high, default 200–2900 Hz. "Raise F high
  toward 4000 Hz to decode stations calling above ~2.9 kHz (common on crowded FT8
  bands); lower the range to focus on a narrow filter or dodge strong close-in
  QRM."
- **A-priori (AP) decoding — FT8** — retry marginal signals against hypotheses
  built from your call, the DX call and the QSO state, including the cross-cycle
  replay of last cycle's QSOs. On by default. FT8 only: FT4's AP is part of its
  Normal/Deep depth and has no separate switch.
- **AP: CQ hypothesis only** — limit AP to the "CQ" guess, no MyCall/DxCall
  hypotheses (FT8 and FT4). "WSJT-X switches to this by itself after 5 minutes
  without transmitting, as a guard against stale-context false decodes; here it
  is your explicit choice."
- **Single decode** — decode only within ±25 Hz of your green RX marker instead
  of the whole passband. Isolates one weak station and saves CPU. FT8 and FT4
  only: 50 Hz is narrower than a single JT65, Q65 or MSK144 signal, so those
  modes keep the full passband.
- **DXpedition mode** — Off or **Hound**. "Hound = DXpedition pile-up discipline
  (calls above 1000 Hz; your report auto-moves to the Fox's frequency)."

**Station Housekeeping**

- **Journey — track a weekly streak** — off by default. "A gentle 'weeks on the
  air' counter on the Journey board — never a daily streak, never a penalty for a
  break."
- **Beacon — announce presence (CQ)** — "Off = passive (hunt & pounce): Nexus
  listens and only transmits when you act. On = periodically calls CQ to announce
  you're on frequency."
- **IR-HARQ — combine retransmissions** — on by default. "A weak frame that fails
  is recovered by joint-combining its retransmissions (RV0+RV1+RV2), and
  unacknowledged QSO overs escalate redundancy. Off = RV0-only." (TempoFast/
  TempoDeep — see
  [the Tempo chat layer](operate-digital.md#the-tempo-chat-layer-tempofasttempodeep).)
- **Clock check (NTP)** — check the PC clock against an NTP server and show the
  offset in the top bar. "TempoFast/TempoDeep are slot-timed to UTC — keep it
  within ~0.5 s." Turn off for fully-offline operation (no network calls).
- **Station power (W)** — "Your transmit power in watts — unlocks the Journey
  miles-per-watt & QRP feats." It also feeds the P.533 link budget. Leave blank
  if unknown.

### JT65 — classic EME

- **Submode (tone spacing)** — A (HF standard, narrowest), B (2× spacing), or C
  (4× spacing, most Doppler-tolerant). "JT65 always uses a 60 s T/R period, so
  spacing is the only choice. A is what you want on HF; EME operators move up to
  B or C as Doppler spread on the higher bands smears the tones. Both stations
  must use the same submode."

JT65 transmits and receives. Its messages are the older 22-character format, not
the 37-character one FT8 and friends use — nothing downstream cares, decodes are
just shorter.

### MSK144 — meteor scatter

- **T/R period** — 5 s (fast turnaround, big showers), 10 s, 15 s (the 6 m
  standard), or 30 s (sparse pings, more to stack). Both stations must match.

MSK144 transmits for nearly the whole period, sending the same 72 ms frame
hundreds of times — that is how meteor scatter works, and a contact can take many
minutes of apparent silence. The audio frequency is fixed at a 1500 Hz centre and
the signal is 1 kHz wide, so **there is nowhere to tune it**. Shorthand (MSK40)
messages are off, matching WSJT-X's default.

### Beacons — WSPR & FST4W

A separate surface from the QSO modes: there is no exchange, only a schedule. Off
by default — beaconing keys the radio unattended, so it is always an explicit
choice.

- **Transmit %** — "Fraction of intervals to transmit on. 0 = listen only. A
  beacon that transmits every interval hears nothing, so a minority is the
  convention — 20–30% is typical." Below 40% Nexus also avoids back-to-back
  transmissions while still hitting the rate you asked for.
- **Transmit power (dBm)** — **required, and it has to be real.** "WSPR reports
  are published to a public propagation database that other operators draw
  conclusions from, so a wrong figure corrupts their data as well as yours. The
  beacon stays silent until this is set. 23 = 200 mW, 30 = 1 W, 37 = 5 W,
  43 = 20 W."
- **FST4W Round Robin slot** — "0 = use the transmit-% schedule. Otherwise your
  slot in a coordinated rotation: stations agreeing on the same slot count and
  each taking a different slot never transmit at the same time, because the
  assignment is fixed by UTC."
- **Round Robin slots** — how many stations are in the rotation. Ignored when the
  slot is 0.

Beacons transmit your callsign, grid and power, so Call CQ and S&P are inactive
on these tiers. Transmit still has to be armed as usual: **the schedule never
keys a radio whose transmit you have not enabled.**

### FST4 (QSO) / FST4W (beacon)

- **T/R period** — 15 / 30 / 60 / 120 / 300 / 900 / 1800 s, shared by both tiers.
  "Longer periods hear weaker signals at fewer exchanges per hour. FST4W beacons
  run at 120/300/900/1800 s; FST4 QSO work is usually 15–60 s."

**FST4** is the QSO mode, **FST4W** the WSPR-like beacon mode — pick which on the
tier selector. Both transmit; the difference is that only FST4 has an exchange to
sequence, so FST4W keys on the schedule set under **Beacons** above. FST4W hashed
callsigns show as `<...>`: the lookup table upstream fills from a file this build
does not carry. (The fieldset's in-app note still says Nexus transmits neither;
that text is stale — both report `tx: true`.)

### Q65 — EME / VHF+ scatter

- **T/R period** — 15 s (troposcatter), 30 s (6 m meteor / ionoscatter), 60 s
  (EME, most common), 120 s (deep EME), 300 s (deepest, microwave EME). Changing
  it changes the decode frame length, so it takes effect on the next slot.
- **Submode (tone spacing)** — A through E. "Wider spacing survives more Doppler
  spread and frequency drift but costs sensitivity. Move up the letters as the
  path degrades — EME on the higher bands usually needs B or C."

Q65 transmits and receives, and **both stations must match**: a correspondent on
a different period or submode will not decode you.

### Quick-reply macros

Comma-separated chip lists for the quick text you fire from each surface:

- **Chat** — chips for Chat.
- **QSO** — chips for sequenced QSOs.
- **Band / CQ** — open broadcasts: the Call CQ launchpad and band feed.

### Phone (SSB / FM)

**Mode**

- **Phone mode** — SSB (USB/LSB by band) or FM. "FM drives the rig to FM + the
  shift/tone below."
- **Repeater shift** (FM only) — simplex / plus / minus. "Offset is the band
  standard (2 m 600 k, 70 cm 5 M…)."
- **CTCSS (PL) tone** (FM only) — the repeater access tone, off or a standard EIA
  tone.

**Microphone**

- **Voice mic (recording)** — "Mic used when RECORDING a voice-keyer message.
  Default records from the audio input device — but on a digital setup that's the
  rig's RX audio, so you'd record the band, not your voice. Pick your actual mic
  here." If it can't open, recording falls back to the input device — never
  silent.

Mic gain and voice-keyer message recording are in the Phone cockpit, not here.

### CW

**Keyer**

- **Keyer backend** — four ways to send, also switchable live from the CW
  cockpit. **CAT** uses the rig's internal keyer (Hamlib `send_morse`), but older
  rigs (e.g. IC-756PRO III) don't support it. **Serial keyline** toggles DTR/RTS
  into the rig's KEY jack — the clean N1MM/fldigi method, needs only a keying
  cable. **WinKeyer** drives a K1EL. **Soundcard** keys an audio tone through SSB
  — a workaround; set drive so ALC reads zero.
- **Sidetone pitch (Hz)** — 300–1200 Hz. Sets the soundcard keyer tone and the CW
  scope zero-beat marker.
- **WinKeyer port** — "For the WinKeyer CW keyer (select it above). 1200 baud."
- **Keyline serial port** (serial keyline only) — the USB-to-serial into your
  keying interface (Buxcomm, US Navigator, a homebrew DTR cable) that plugs into
  the rig's KEY jack. "Must be a SEPARATE port from CAT. Set the rig to CW and
  its key-jack to straight-key / bug."
- **Keying line** (serial keyline only) — DTR (the CW convention) or RTS. "DTR is
  standard (RTS = PTT); flip to RTS if your interface is wired the other way."
- **CW ID after 73** — keys your callsign in CW once the final 73 has fully left
  the air (stock WSJT-X option, default off). It uses the normal CW keying path —
  PTT + tone — after the FT8 over, never on top of it.

**Macros (F-key profiles)**

- **CW cockpit F-keys** — named macro profiles (**New** / **Rename** /
  **Delete**, at least one always kept), switchable here or in one click from the
  CW cockpit bar. The grid edits the active profile: a label and a template per
  key. **Customize** starts from the built-in F1–F8 set; **Reset to defaults**
  returns to it.
  Tokens: `{MYCALL}` `{NAME}` `{MYGRID}` `{MYSTATE}` `{RST}`, `!` = the worked
  call, and `{HISNAME}` `{HISSTATE}` = the worked station's QRZ name and state.
  Each key **keeps its role** (F1 CQ, F2 answer, F3 report, F4 sign off, F5 my
  call, F6 his call, F7 ask repeat, F8 query), so the Guided copilot's next-step
  highlight still rolls F1→F2→F3→F4 through customized text.

### RTTY

**Keying**

- **Keying backend** — **AFSK** plays the two-tone waveform through the same TX
  audio path as FT8 (soundcard-clocked, jitter-free; set drive so ALC reads just
  zero). **True FSK** bit-bangs the rig's FSK input over a serial control line
  with the rig in RTTY mode, unlocking its narrow RTTY filters. "Software FSK
  timing is casual/Field-Day grade; AFSK is the timing-cleanest path."
- **FSK serial port** (True FSK only) — the port whose control line feeds the
  rig's FSK input. Empty = the CAT serial port.
- **FSK data line** (True FSK only) — DTR (the common wiring, leaving RTS free
  for PTT) or RTS. "PTT must ride its OWN path — CAT PTT or the separate PTT
  line, never this one; Nexus refuses a send if they collide."

**Signal**

- **Baud rate** — 45.45 (the HF standard) or 75. Drives the TX bit clock and the
  RX demodulator — true 45.45, never rounded to 45.
- **Shift (Hz)** — 170 (the HF standard), 425 or 850. The TX tone pair and the RX
  demodulator both.
- **Reverse (swap mark/space)** — "The convention is LSB with mark on the lower
  audio tone. Turn this on when deliberately running the opposite sideband (e.g.
  AFSK in USB/DATA-U) so the on-air sense stays correct." Applies to TX and the
  RX decoder.

### APRS

**APRS-IS (internet feed)**

- **APRS-IS feed** — "Plot stations the internet reports alongside the ones your
  own antenna hears — each one tagged so you can always tell which is which. Runs
  whether or not the APRS decoder is armed: it uses no radio and never
  transmits." If internet stations appear while your receiver stays silent, the
  fault is in the RF chain.
- **Server** — "Your regional Tier 2 rotate is best — noam / soam / euro / asia /
  aunz .aprs2.net. `rotate.aprs2.net` works anywhere."
- **Port** — "14580 is the filtered port clients and iGates should use. The
  full-feed ports would send you the entire planet."
- **Radius (km)** — how far around your grid to subscribe. "APRS is a local mode;
  150 km is a generous 2 m-plus-digipeater horizon. 0 = no distance limit
  (busy)."
- **Watched calls** — comma separated. "These come through from anywhere on
  earth, however far outside your radius they are — the club tracker on a road
  trip, a friend chasing a summit."
- **Weather stations** — include weather reports in the feed.
- **Objects & items** — repeaters, NWS alerts and event markers other stations
  have placed on the map.
- **Messages** — show APRS text messages from the feed. **Display only —
  replying to an internet message is not wired up.**
- **Keep stations for (min)** — how long a station stays on the map after its
  last packet; they start to fade at a third of this. An hour by default, because
  fixed stations often beacon only every ten to thirty minutes and a shorter
  window makes the slow ones blink off between their own beacons. 0 keeps every
  station forever (the 2000-station ceiling still applies).
- **Receive-only iGate** — contribute packets **your own antenna hears** to
  APRS-IS, so stations in your area reach the global map through you. It
  publishes under your callsign, so it is a separate choice from watching the
  feed, and it needs the APRS decoder running to have anything to send. **Nexus
  never sends the other way**: gating the internet back onto the air means
  transmitting unattended.

---

## Frequencies

### Working Frequencies

The dial frequency used when a band/mode is selected. These are **overrides** of
the stock WSJT-X working-frequency table — "leave the list empty to use stock
everywhere. An override replaces the stock row for its band + mode."

- **Standard table (read-only)** — the stock WSJT-X dial frequencies. A row with
  an active override shows your value, highlighted.
- **Your overrides** — rows of band + mode + dial MHz. **Add override** adds a
  row, **Reset to standard** clears them all, **✕** removes one. "MHz is the dial
  (suppressed-carrier) frequency." A duplicate band+mode is flagged inline and
  the last row wins. Save to apply — band switches then use your value.

---

## Spots & Alerts

What Nexus tells you about, and how loudly. Kept quiet by default so the app
doesn't cry wolf.

### Pounce — new-one alert

Interrupts you the **instant** a needed station appears on the cluster or RBN,
rather than waiting for the spot board to refresh. A loud tone plays whether or
not Nexus is the window you are looking at, and a banner offers one-click Work.
Each station alerts once per band and mode.

- **Alert me for** — Off (default) / New DXCC entity only / New entity or CQ zone
  / New entity, zone, or US state.

How rare "rare" is depends on your own totals: if you are chasing your first
hundred entities then almost every DX spot is a new one and this would never stop
talking. Start with *New DXCC entity only* once your log is far enough along that
a new one is genuinely an event.

### Accessibility & eyes-free

Speech and sound cues for operating by ear. The keyboard and screen-reader labels
throughout Nexus are **always on** — these settings only control what comes out
of the speakers.

- **Announce decodes (screen reader)** — Off / Needed only (calling you / new /
  watched) / All (adds a per-cycle CQ summary). Silent without a reader running.
- **TX / RX earcon** — "A rising tone when you key up, falling when you unkey —
  know your TX state by ear."
- **Decode-batch tick** — "A soft tick each cycle new signals are decoded — the
  band's rhythm, eyes-free."

### Alerts

- **My call** — "Beep + flash when someone directs a call at you."
- **CQ calls** — "Alert on any decoded CQ. Off by default — CQs are constant."
- **New DXCC** — Off / HF only / VHF+ (6 m and up) / All bands. "Loud alert on a
  new DXCC entity — a 'new one'. **Does NOT alert on every decode.**"
- **New grid** — same band scopes. "Quiet toast on a grid you haven't worked.
  Default VHF+ only — grid awards (VUCC/FFMA) start at 6 m; on HF nearly every
  decode is an unworked grid."
- **Rare grid 💎** — same band scopes. "The loud 💎 alert for rare/water-only
  grids (rovers, maritime, DXpeditions) — separate from plain grids so silencing
  HF chatter keeps the gems."
- **Watch list** — the calls you want flagged wherever they turn up.

---

## Logging & Connectors

Where QSOs go and what feeds come in. Credentials live in the **OS keychain**,
never on disk; a saved password or key isn't shown again after you click **Set**,
and **Forget** removes it.

### Connections

A status grid of every connector — a dot for "credential stored", plus the stored
identity — and a **Test** button on QRZ Logbook that round-trips the API without
logging anything. Below it, a session **Connection log**: "every save, sync,
push, and failure lands here."

### Integrations & Feeds

**Local APIs & Loggers**

- **WSJT-X UDP API** + **UDP Address** — "for JTAlert / GridTracker / loggers"
  (default `127.0.0.1:2237`).
- **Ham Radio Deluxe logging** + **HRD UDP Address** — push each QSO to HRD
  Logbook over its QSO-Forwarding UDP port (default `127.0.0.1:2333`). HRD must
  be running, and don't also run JTAlert/QSO Relay into HRD or you'll double-log.
- **Companion UDP address** — "Where Nexus listens for WSJT-X/JTDX in Companion
  source mode."
- **Write ALL.TXT decode log** — WSJT-X-format decode log for GridTracker /
  loggers to tail. "Written only while this is on, and it first appears after the
  next decode." The saved path is shown, with **Reveal in folder**.
- **Save a WAV per logged QSO** — "Auto-records the last ~60 s of RX audio to the
  recordings folder on log."
- **Save received audio (.wav per period)** — None / periods with decodes / all
  periods. WAVs land in `recordings/periods` (12 kHz mono, ~360 KB each). "'All'
  writes ~2 GB/day of continuous monitoring — use for decoder debugging, not
  always-on."

**Spot Sources**

- **PSK Reporter** — "upload spots to the global map."
- **DX Cluster / RBN spots** — "Surface 'new ones' from the Reverse Beacon
  Network on the Needed board + Connect." Takes effect on restart.
- **Phone/SSB cluster nodes** — human DX-cluster nodes for SSB/phone spots, since
  RBN only carries CW and digital. "We connect to ALL listed nodes and union
  their human SSB/phone spots — more nodes = wider phone coverage." Add from the
  **+ Add a known node…** presets (VE7CC-1 recommended; WA9PIE-2 on port 8000 if
  23 is blocked; W1NR phone-rich; W3LPL the skimmer-heavy firehose) or
  **+ Custom**. An added node connects on the next Save; removing one takes
  effect on restart.

**Propagation**

- **Near-region opening watch** — "Watch VHF/10 m activity near your QTH (not
  just your own contacts) so openings flag 'open around you' before you've worked
  anyone." Takes effect on restart.
- **Prediction engine** — Modelled (fast heuristic) or ITU-R P.533 (full
  physics). "P.533 is the real circuit-reliability method (validated against the
  ITU reference; ~0.1 s per prediction, uses your station power). **Live spots
  always win over any model.**" See [Connect](connect.md).
- **Antenna gain (dBi) — TX / RX** (under *Antenna gain (advanced)*) — "Used by
  the P.533 link budget only. 0 = a simple wire/vertical (isotropic); a
  3-element yagi ≈ 6–8. Honest v1: a plain dB shift — no pattern or
  takeoff-angle modelling, and the fast heuristic ignores it."

### DXKeeper (DXLab Suite)

Pushes each logged QSO into DXKeeper over its TCP Network Service. Enable it in
DXKeeper under *Configuration ▸ Defaults ▸ Network Service* first.

- **DXKeeper host** — "Usually 127.0.0.1 — same PC. Leave blank to disable."
- **DXLab Base Port** — the *Base Port* from DXKeeper's Network Service panel
  (default 52000). DXKeeper itself listens on base + 1 and **Nexus adds the 1 for
  you**, so the number you read off DXKeeper is the number that works.
- **Let DXKeeper do the uploads** — "Off by default: Nexus already uploads to
  LoTW / eQSL / ClubLog / QRZ, so turning this on would upload every QSO twice."
  DXKeeper ignores it for Club Log and QRZ if *Auto upload* is ticked on its own
  QSL Configuration tab — untick it there.

### N3FJP Integration (club master log)

"Each FD contact lands in the club's **N3FJP Field Day Contest Log** the moment
you log it — so the whole club's score updates in real time." Run N3FJP on the
master computer and point Nexus at its IP and port.

- **N3FJP host** — IP or hostname of the master log computer. Blank = off.
- **N3FJP port** — N3FJP's API TCP port (default 1100).
- **Use ENTER for Field Day scoring** — on by default. Logs each FD contact with
  N3FJP's ENTER sequence, "which scores the contest — the correct path." Off
  falls back to a plain `ADDDIRECT` insert, which may not score.
- **Report my band to N3FJP** — "Tell N3FJP which band you're on (no CAT needed),
  so the club's Network Status Display band board shows this position." Off by
  default.
- **Forward every QSO** — also push **every** logged QSO, not just Field Day, to
  N3FJP ACLog. "N3FJP dedupes, so it's safe to run alongside the Field-Day push."
- **Connection test** — **Test N3FJP** saves, then tests the TCP link. "Run this
  at the club site before the event starts."

### N1MM+ Integration

- **N1MM contact broadcast address** — host:port, UDP. "Name the port — consumers
  stack on one host, and 12060 is often already taken by another logger." Blank
  = off. An address alone sends nothing outside a Field Day event.
- **Broadcast every QSO** — send the contact packet for **every** logged QSO, not
  just Field Day: point OpenHamClock or GridTracker at the address and each
  contact plots as you log it. "One packet per QSO: this never doubles up with
  the Field Day broadcast." Off by default; turning it on with a blank address
  fills in `127.0.0.1:12060` visibly, rather than as a hidden default.

### LoTW users list

- **Fetch now** — downloads ARRL's weekly activity list; the status line shows
  the call count and date. This powers the teal **L** marks on decode and roster
  rows.
- **Count as a LoTW user if uploaded within (days)** — the recency window
  (default 365).

"ARRL's activity list updates weekly — refetching more often just returns
'unchanged'. Manual fetch by design (WSJT-X convention)."

### Callsign → state database

- **Update now** — "A callsign→state index (from the FCC license file) so a New
  State lights up on cluster / CW / SSB spots that carry no grid." Downloads on
  first launch, then auto-refreshes weekly from hamradiotools.io; a live decode
  grid refines it for rovers.

### Confirmations

**LoTW**

- **LoTW username** — "Often your callsign, but not always — use your LoTW
  account login."
- **LoTW password** — your LoTW **website** password, not your TQSL certificate
  password.
- **LoTW sync** — **Sync LoTW now** "pulls new confirmations into your log and
  marks which of your uploads LoTW now holds on file (so they read 'waiting on
  the other op,' not 'never uploaded')." The first sync pulls your whole history
  and can be slow; later syncs are incremental.
- **LoTW Station Location** — for **uploading**. Signing is done by your
  installed **TQSL** against this named Station Location — set it up in TQSL
  first; the name must match exactly. **No certificate or password is stored by
  Nexus.**
- **Sign from ADIF location (travelers)** — turn on if you set TQSL to *"use the
  location in the ADIF file"* rather than creating named Station Locations. Nexus
  then stamps your call and grid into the upload and omits `-l`. **The whole
  batch is signed from your current grid**, so if you operate from more than one
  location, upload *before* you move.
- **TQSL path (optional)** — "Only if TQSL is installed somewhere non-standard;
  otherwise leave blank to auto-detect."

**eQSL**

- **eQSL username** / **eQSL password** — your eQSL.cc login, often your
  callsign.
- **eQSL confirmations** — **Sync eQSL now** downloads confirmations into your
  log. "These count as confirmations but **not** for DXCC/WAS (ARRL doesn't
  accept eQSL) — a separate tier."
- **Auto-upload QSOs to eQSL** — upload each logged QSO as you log it.

**QRZ**

- **QRZ username** / **QRZ password** — "this is what powers callbook lookups"
  (name, QTH, grid). Separate from the Logbook API key below, which only uploads
  QSOs. "Grid & state need a QRZ XML subscription; free accounts return only
  name/address/country."
- **QRZ Logbook API key** — a **separate** key from your QRZ logbook's settings
  page, used to upload logged QSOs.
- **Auto-upload QSOs to QRZ** — push each logged QSO to your QRZ logbook.
- **Pull confirmations automatically** — "As people confirm on QRZ, the
  confirmations flow in on their own — no need to press Sync. After the first run
  only what CHANGED is fetched." The last pull time is shown. QRZ confirmations
  **never count toward DXCC or WAS**, which need LoTW or a card.
- **Two-way sync** — **Sync from QRZ now** pulls your QRZ logbook **down**,
  adding QSOs you logged elsewhere (e.g. a phone app in the field) and marking
  QRZ-confirmed contacts. Safe to run repeatedly (deduped).

**HamQTH**

- **HamQTH username** / **HamQTH password** — "A **free** callbook used as a
  fallback when QRZ isn't configured or has no match — a HamQTH account returns
  name, grid & US state at no charge."

**ClubLog**

- **ClubLog email** — your ClubLog login email, not a callsign.
- **ClubLog callsign** — "The ClubLog logbook to upload into (empty = your
  callsign)."
- **ClubLog app-password** — "Use a ClubLog **Application Password** (Settings →
  App Passwords), not your main password."
- **ClubLog API key (application-level)** — "This is the **application**
  credential, not yours — official installer builds bundle one, and you only need
  email + app-password above. Building from source? Request a free key at
  clublog.org/requestapikey.php and paste it here (open-source can't ship one —
  ClubLog auto-revokes published keys)."
- **Auto-upload QSOs to ClubLog** — push each logged QSO in real time.

**HRDLog**

- **HRDLog.net upload code** — from your HRDLog.net account (Options → your
  code). Uploads log under your station callsign. "This is the online HRDLog.net
  service — separate from the HRD Logbook UDP push under Integrations & Feeds."
- **Auto-upload QSOs to HRDLog.net** — "HRDLog.net is a live-logging and awards
  site — it is **not** an ARRL confirmation source, so an upload here never earns
  DXCC/WAS credit."

**RepeaterBook**

- **RepeaterBook API token** — optional. "Without a token the **Program** section
  uses the open hearham.com directory. Add a personal token (from your
  RepeaterBook account's **API Apps** page) to pull from RepeaterBook.com under
  your own account instead." Shared RepeaterBook access for every Nexus user is
  pending RepeaterBook's approval; if RepeaterBook is unreachable, Program falls
  back to hearham.com.

**Cloudlog / Wavelog**

Auto-forward each logged QSO to your self-hosted Cloudlog or Wavelog logbook over
HTTP.

- **Base URL** — your site root. "Leave blank to disable."
- **Station profile id** — "The station-location profile to log against (Cloudlog
  ▸ Station Locations)."
- **API key** — "Cloudlog ▸ Account ▸ API Keys — a key with read/write." A
  per-instance token for your own server.
- **Auto-forward QSOs** — push every logged QSO to the instance above as it's
  logged.

---

## Contesting

Always visible — capability, not configuration, gates the tabs.

### Contest Category

- **Unassisted entry** — "Turns off the AI CW decoder, DX cluster / RBN spots and
  the PSK Reporter needs feed together, and records the change with a timestamp.
  **Takes effect at once**" — its own command, not Save, because an operator
  flips it as an event starts. "Your own settings for each of those are left
  alone and come back when you switch this off."
- **Assistance record** — the timestamped journal of every flip, with which
  sources were active. Kept in `assistance_journal.json` beside your settings, so
  it survives restarts. Newest first.

### Field Day Setup

- **Field Day mode** — the master switch. "Turn on for Field Day weekend —
  reveals the Field Day workspace and the Class/Section exchange across all
  modes. Off the rest of the year." It stays on across restarts until you turn it
  off. The **same switch** also appears in
  [Appearance ▸ Features](#features) under Contesting — Field Day visibility is
  owned by this persisted setting, not by a feature flag, so the Features group
  hosts the master rather than a separate toggle.
- **Event** — ARRL Field Day or Winter Field Day. "Affects scoring labels and
  export headers."
- **FD Class** / **WFD Category** (the label follows the Event) — "Number of
  transmitters + class letter: A=club/group portable, B=1–2 person portable,
  C=mobile, D=home (mains power), E=home (emergency power), F=EOC. E.g. 3A."
  For WFD: "Transmitters + location: H=Home, I=Indoor, M=Mobile, O=Outdoor."
- **ARRL Section** — "Your ARRL / RAC section (e.g. WI, ENY, ONN). Start typing
  the code or a state name and pick from the list." Every entry is validated
  against the full ARRL/RAC list and an unknown one is flagged inline, so it
  never silently reaches the Cabrillo log.
- **Power multiplier** — ×5 (QRP/battery, ≤5 W on natural power), ×2 (≤100 W),
  ×1 (>100 W). "Multiplies your QSO points. Choose before the event."

Class and Section **start empty on purpose** and the station won't enter Field
Day until both are set — a banner says so while the mode is on and they're blank.
See [Contesting & POTA/SOTA](contesting-pota.md).

---

## Appearance

UI-only preferences (applied live, not via Save) and the section toggles.

### Workspace

- **UI scale** — **Auto (fit)** scales the whole interface to the window so
  nothing is cut off, with **Max scale** cap chips so auto never overshoots on a
  big monitor. A cap this window can't reach is disabled and its tooltip says
  why ("a larger window or monitor unlocks it"). **Manual** picks a fixed
  percentage instead. The waterfall stays sharp either way.
- **Density** — Comfortable or Compact. "How tightly rows and controls pack.
  Compact fits more on screen."
- **Pane sizes** — **Reset pane sizes** restores the default pane widths. Pane
  layout itself is set in the cockpits: drag the dividers between panes to resize
  (double-click a divider to reset), and use the ⊞ menu to show or hide panes.

(The theme picker — dark / light / amber night-vision — lives in the app chrome,
not this tab.)

### Features

Turn sections on and off, and pick a goal profile.

- **Profile** — a goal (getting started, DX/awards, contesting, POTA/SOTA,
  6m/VHF) sets sensible defaults. "Pick a goal to set sensible defaults — every
  feature stays toggleable below." Hand-toggling produces a **Custom** set, and
  switching away from Custom asks first because it discards your hand-tuned set.
  A **Re-run setup…** link reopens the first-run wizard.
- **Core — always on** — the spine (Operate, Logbook, Settings, Now Bar, Chat,
  Connect, Needed) shows an "always on" badge instead of a toggle: a locked
  switch beside real ones just reads as broken.
- **Optional features**, grouped by category in this order: Operate, DX & Awards,
  Propagation, Contesting, POTA/SOTA, Logging, System. Each row is a toggle with
  a one-line "why you'd want it". Enabling a feature pulls in anything it depends
  on, and the hint names what else it will turn on.
- The **Contesting** group hosts the **Field Day mode** master switch (the same
  setting as [Contesting ▸ Field Day Setup](#field-day-setup)). Turning it on
  with no Class or Section set jumps you to the Contesting tab to fill them in.

---

## Related guides

- [Operate — FT8/FT4 digital](operate-digital.md)
- [Connect — map + propagation](connect.md)
- [Logbook & QSL](logbook-qsl.md)
- [Contesting & POTA/SOTA](contesting-pota.md)
- Back to the [guide index](index.md)
