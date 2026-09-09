**This is a beta.** It reaches you only if you turned on Settings ▸ App updates ▸ beta updates —
nobody is auto-updated onto it. If something is wrong, say so on GitHub and I'll fix it before
1.11.0 goes out properly.

**JS8 is built in, and it's on.**

Nexus now decodes and transmits JS8 natively — no second application, no rig sharing, no audio
routing between programs. It's a new section in the sidebar, on by default, and it works without
being configured: open it and Nexus tunes the JS8 watering hole for the band you're on and starts
decoding. Nothing transmits until you arm it.

- **All four speeds at once.** Slow, Normal, Fast and Turbo are decoded together, every period, not
  one at a time. The header chips show which are running.
- **CQ and the heartbeat repeat on their own**, with the countdown on the button — `CQ (12)` —
  the way JS8Call does it. Set a heartbeat going and walk away. Both stay behind the transmit
  latch and the idle watchdog, so an unattended beacon still stops on its own.
- **Directed messages, relay and the inbox** work as JS8Call does, including the full command set,
  `@ALLCALL`, and store-and-forward.
- **The station list carries what a DX operator needs** — distance, beam heading, whether you've
  worked them before, their name and comment, and a pin for the ones you're watching.
- **Band activity is listed by audio offset with its time delta**, so picking a clear slot is a
  glance rather than a hunt.

On-air compatibility with JS8Call is the whole point, so anything that doesn't interoperate is a
bug worth reporting. Fast and Turbo have had less on-air time than Slow and Normal — those two are
where I'd most like to hear how it does.

**The manual has pictures now.**

Every one of the 22 chapters is illustrated — 137 screenshots, up from 11 — and the pages you reach
first got the most attention: the first-run wizard, the waterfall, the settings reference. Reading
the app against the manual also turned up a stack of things it claimed that were no longer true: the
wizard is four steps and not three, the waterfall takes three mouse gestures and not two, the grid
wants all six characters, and several mode chapters described transmitting that those modes don't
do. All corrected.

**Four things about your credentials.**

These are worth reading even if nothing looked wrong, because two of them left something on disk.

- **A logbook service's error text no longer goes into your log.** When a QRZ Logbook or ClubLog
  upload was *rejected*, the service's reply was recorded on that contact inside `log.adi`. Both
  services answer a rejection by quoting your request back, and the request carries your API key —
  so a failed upload wrote the key into your logbook, and from there it travelled out through every
  export built on it, including the batch TQSL signs with your callsign certificate and sends to
  LoTW. What gets recorded now is Nexus's own description of why the upload failed. **On upgrade,
  Nexus also cleans the key out of `log.adi` and its backups** — not just new contacts. If your
  uploads have been going through, nothing was ever written.
- **`settings.json` is no longer world-readable.** It holds your ClubLog key, and it was readable by
  any other account on the machine. Now owner-only.
- **Upgrading no longer destroys a stored Cloudlog key** when the OS keychain isn't reachable.
- **The QRZ callbook row no longer claims a subscription it can't prove.** It used to go green
  whenever QRZ answered at all — including when QRZ answered "not a subscriber". If your XML
  subscription lapsed, lookups quietly stopped filling in grids and names while the panel still read
  green. It now goes green only on a lookup that proves it was entitled. The trade: a lookup whose
  record carries neither grid nor state now reads not-confirmed until the next real one.

**Also new**

- The **FT-710 can draw its own band scope**.
- A **POTA activity map in FT mode**, with an optional audible alert for new activations.
- **Hound mode is one click** on the FT8 screen, and Nexus warns you when a DXpedition is running
  SuperFox before you call it.
- A per-radio switch to **hold FM-D while receiving SSTV**.

**Also fixed**

- The JS8 screen was drawing the Tempo screen underneath it — tier buttons, roster, conversation and
  a second waterfall, all on top of the JS8 cockpit.
- Turning Hound off mid-QSO with a Fox could strand the contact.
- The satellite catalogue was publishing years-old orbits for birds that had stopped being tracked.
- The RTTY auto-sequencer couldn't log a single Winter Field Day contact.
- Your log never recorded **which callsign made a contact**, so a special-event batch uploaded under
  the wrong one.
- The Chase pane stopped updating once you selected a station.
- CW and SSB park activations were missing from the roster for digital-only stations.
- The 60 m FT8 dial, and what changed underneath it.
- An upload code pasted with a stray space or newline made every upload fail, silently.
- Cloudlog and Wavelog threw away the reason an upload was rejected; a LoTW upload TQSL turns down
  now says what TQSL said, and which of the two problems it was.
- Nothing could clear the callsign card in the FT cockpit, and F4 did nothing while you were in it.

73 — KD9TAW
