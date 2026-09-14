If you're coming from 1.10.3, this release also carries everything from the 1.11 betas — built-in
JS8 among it. That's at the end.

**Contests beyond Field Day.**

Pick a contest in Settings ▸ Contesting and the whole operating path follows it: the entry strip
shows that contest's exchange, dupes follow that sponsor's rule, the scoreboard counts its
multipliers, and the Cabrillo and ADIF exports carry the right names. Every rule was read from the
sponsor's current rules page.

- **ARRL November Sweepstakes**, CW and Phone. The long exchange is handled, callsign and check
  included, and Nexus tells you if your check changes partway through the weekend.
- **CQ World-Wide DX and CQ WPX**, CW and SSB. Their multipliers come from the other station's
  callsign — country, CQ zone, prefix — so Nexus works out each contact's country and zone as you
  log it.
- **ARRL VHF** — January, June and September, each scored its own way.
- **Four state QSO parties** — California, Ohio, Tennessee and Texas. Tennessee's and Texas's
  bonus points aren't in the on-screen total, and the scoreboard says so.
- **A rate meter** on the scoreboard: contacts per hour over your last 10, your last 100 and the last
  60 minutes. It drops when you stop rather than showing your best run on a dead band.
- **One button merges the contest into your logbook** afterwards, and tells you how many it added.
- A club running multi-op can declare it, and a solo entry no longer tells the sponsor it was
  multi-op — which every solo Field Day log Nexus exported used to do.

If a score or an export looks wrong for a contest you know well, that's exactly the report I want.

**Two things worth reading even if nothing looked wrong.**

- **A long transmission could go silent and leave your rig keyed.** On WSPR the audio stopped about
  twenty seconds in and the radio stayed keyed for the rest of the two-minute over, every over. It
  wasn't only WSPR: JT65, Q65 and FST4/FST4W at 30 seconds and longer did the same, and so did a
  one-shot PSK31 message longer than a couple of lines. FT8, FT4 and the other short modes never
  had the problem. The whole transmission now goes out, and the rig unkeys when the audio ends.
- **Your transmit timing leaned on a single network reply.** Nexus corrects for your PC clock
  being off, and that correction came from whichever time server answered first. It now asks three
  and needs two to agree. It also holds the last good correction when you lose internet, refuses to
  steer by a clock more than a minute out and tells you instead, and re-measures straight away when
  a laptop wakes from sleep instead of applying a correction that no longer fits.

The clock chip in the top bar now says what Nexus is doing about your clock rather than showing a
red alarm on a station whose timing is fine. On Windows, if the time service itself is broken —
switched off by a "debloat" script, or never synced — Nexus fixes it with one administrator
prompt, and leaves a healthy machine, or one running its own time program, completely alone.

**Nexus Remote — an early pilot.**

Run your station from a browser. Pair it under Settings ▸ Station ▸ Remote access, approve the
browser at the radio, and you get the same Nexus you use at the shack — decodes, waterfalls, the
logbook, spots, awards, the lot. Your log, settings and radio stay on your computer. The service
only carries the picture and your commands, and if it goes down nothing at the station changes.

- **You decide at the shack what a browser may do**: station controls (tuning, modes, filters and
  DSP, power, the amplifier, which radio is active), logging contacts, and FT8/FT4 transmit.
- **Remote stays on when Nexus restarts.** A browser you approved keeps its station control and
  logging, so a power blip at the shack doesn't lock you out. FT8/FT4 transmit is the exception:
  that permission ends with every restart, and you give it again at the radio.
- **Start Nexus when you sign in** — a new switch under Settings ▸ Appearance, off unless you turn
  it on. The first time you turn Remote on, Nexus offers it once.
- **FT8 and FT4 transmit from the browser** — CQ, answering, Tx1–Tx6, free text. It goes through
  your normal TX switch and watchdog, Stop TX works from the browser, and if the browser drops off
  the station stops transmitting within five seconds. No other mode transmits remotely.
- **If the link is slow, a click can be refused rather than sent late.** The browser now says "Not
  sent" when nothing reached the station, so you know to try again, and keeps "not confirmed" for a
  command that was sent but never answered.
- **No audio yet**, in either direction.

It runs on a test server that may be reset, and approving a station starts a 14-day trial. Most of
the radio controls haven't met many real radios yet, so if something misbehaves on yours, tell me
which rig.

**Connect: less clutter, and much lighter on your PC.**

- **Close any pane you don't use**, with the ✕ on its header, and bring it back from the Panels
  menu. Close a whole side and the map takes the room. Drag the edge of a side column to make the
  map bigger. Reset layout puts everything back the way it started.
- **Each view keeps its own map.** Chase DX, POTA/SOTA and the others each remember their own
  projection, layers and 2-D or 3-D, so a trip to POTA/SOTA no longer resets your Chase DX map.
- **One map picker: Globe, 3D, Flat or Beam.** The separate 2D/3D switch is gone — the four views
  sit in one row, Globe is where a new view starts, and each view remembers the one you picked.
- **Layers folds away** like Conditions, and sits in the same corner whichever map you're on.
- **Far less CPU.** The 3-D globe used to redraw constantly even with nothing moving, and the 2-D
  map redrew every country outline every few hundred milliseconds. Both now sit near idle until
  something on them changes. If Connect made your laptop fan spin up, try it again.
- On a 1024×768 screen the map toolbar was cut off, hiding Reset and Full screen. It now wraps.

**Also new**

- **Export one POTA activation.** The Logbook lists your activations — park, date, contact count —
  and exports exactly the one you pick, named the way POTA wants it. An activation that crosses UTC
  midnight shows as two, so you can see before uploading that neither day reached ten.
- **Full-screen map**: one button in, the same button or Escape out.
- **The map draws your paths**: green great circles to the stations that heard you, and (off by
  default, one checkbox in Layers) blue ones to the stations you heard.

**Also fixed**

- Nexus was switching your radio to CW-R on 40, 80 and 160 m. It now asks for plain CW everywhere,
  and there's a Reverse CW checkbox under Settings ▸ CW if you want it.
- An AM contact was logged as SSB. The log now follows the mode your radio is actually on, and AM
  is on the Phone screen's mode picker on every band — 14.286 had no AM button.
- The ARRL section list was out of date: 85 sections now. `GTA` and `NT` are renamed for you; if you
  had `MAR`, Nexus asks which of `NB`, `NS` or `PE` you're in. Field Day also refuses a section
  ARRL doesn't list, where before it let you start anyway.
- Correcting a busted callsign in the Logbook now re-sends the contact to QRZ, ClubLog, eQSL and
  the other services you upload to. Before, they all kept the wrong call. The Logbook's CALL box
  also shows the whole callsign now.
- Three Cabrillo contest names were wrong (Field Day, Ohio, Texas), and N1MM broadcasts named the
  wrong contest.
- The WSJT-X and N1MM feeds stamped your current exchange on every contact, so a mobile station
  that changed county mislabelled everything it had already worked.
- The RTTY sequencer's sign-off sent the next contact's serial number.
- The beta-updates switch could quietly turn itself off after you touched an APRS control.
- A torn-off POTA/SOTA board couldn't be scrolled.
- Map markers were blurry at UI scales above 100%.
- Amplifier band-following could show one setting while using another, and now re-checks your setup
  before every command it sends to the amp.
- A radio that won't connect backs off between retries instead of trying again and again.
- On Windows, a sound card glitch could fill the log with "capture stream died" when nothing had
  died. Receive audio also gets more buffer, and the "no decodes" log line now says whether any
  audio is arriving at all.
- On Xiegu radios, trust the radio's own SWR meter — Nexus reads it on an Icom scale, so a G90 at
  1.2:1 can show 6:1. The rig guide now says so.

**New since 1.10.3 — first shipped in the 1.11 betas**

- **JS8 is built in, and it's on.** Nexus decodes and transmits JS8 itself — no second program, no
  rig sharing, no audio routing. Open it and it tunes the JS8 watering hole for your band and starts
  decoding all four speeds at once; nothing transmits until you arm it. CQ and the heartbeat repeat
  on their own with the countdown on the button, directed messages, relay and the inbox work as in
  JS8Call, and the station list shows distance, heading and whether you've worked them. Fast and
  Turbo have had the least air time — reports on those are welcome.
- **The manual has pictures now** — all 22 chapters illustrated, and a stack of out-of-date claims
  corrected along the way.
- **Four things about your credentials, worth reading even if nothing looked wrong.** A rejected
  QRZ Logbook or ClubLog upload could write your API key into `log.adi` (and from there into
  exports); Nexus no longer does, and cleans it out of the log and its backups on upgrade.
  `settings.json` is now owner-only. Upgrading no longer destroys a stored Cloudlog key when the
  keychain isn't reachable. The QRZ callbook row only goes green when a lookup proves your
  subscription.
- The **FT-710 can draw its own band scope**; a **POTA activity map in FT mode** with an optional
  alert for new activations; **Hound is one click** on the FT8 screen, with a warning when a
  DXpedition runs SuperFox; a per-radio switch to **hold FM-D while receiving SSTV**.
- Fixed: turning Hound off mid-QSO could strand the contact; the RTTY sequencer couldn't log Winter
  Field Day; your log didn't record which callsign made each contact, so special-event uploads went
  under the wrong call; the satellite catalogue served years-old orbits; CW and SSB park activations
  were missing for digital-only stations; a pasted upload code with a stray space failed silently;
  and upload rejections from Cloudlog, Wavelog and LoTW now say why.

73 — KD9TAW
